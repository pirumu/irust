//! Project 2: a mini async runtime (epoll reactor + single-thread executor) and the same
//! HTTP server written as `async fn` on top of it.
//!
//! Run (Linux only): `./labs/linux.sh cargo run -p lab-capstone --bin mini_runtime -- 8081`
//!
//! Pieces, bottom to top:
//! - `Reactor`: one epoll instance; per descriptor it stores the `Waker` of the task waiting
//!   to read and the one waiting to write, and wakes them when epoll reports an edge.
//! - `Executor`: owns the tasks (`Pin<Box<dyn Future>>`), a ready queue of task ids, and a
//!   `Waker` per task built from `Arc<TaskWaker>` via `std::task::Wake`.
//! - `TcpListener` / `TcpStream`: leaf futures (`Accept`, `Read`, `Write`) that try the
//!   syscall first and only park the task after `EAGAIN`.

#[cfg(target_os = "linux")]
mod rt {
    use lab_capstone::{LAB_SNDBUF, http, sys};
    use std::cell::{Cell, RefCell};
    use std::collections::{HashMap, VecDeque};
    use std::future::Future;
    use std::io::{self, ErrorKind};
    use std::os::fd::{AsRawFd, OwnedFd, RawFd};
    use std::pin::Pin;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};

    // ------------------------------------------------------------------ counters

    #[derive(Default)]
    pub struct Stats {
        accepted: Cell<u64>,
        requests: Cell<u64>,
        polls: Cell<u64>,
        epoll_waits: Cell<u64>,
        io_wakes: Cell<u64>,
        read_pending: Cell<u64>,
        write_pending: Cell<u64>,
        io_errors: Cell<u64>,
        live_tasks: Cell<usize>,
    }

    fn bump(c: &Cell<u64>) {
        c.set(c.get() + 1);
    }

    impl Stats {
        fn render(&self) -> String {
            format!(
                "accepted={} requests={} polls={} epoll_waits={} io_wakes={} read_pending={} write_pending={} io_errors={} live_tasks={}",
                self.accepted.get(),
                self.requests.get(),
                self.polls.get(),
                self.epoll_waits.get(),
                self.io_wakes.get(),
                self.read_pending.get(),
                self.write_pending.get(),
                self.io_errors.get(),
                self.live_tasks.get()
            )
        }
    }

    // ------------------------------------------------------------------ reactor

    #[derive(Default)]
    struct Waiters {
        reader: Option<Waker>,
        writer: Option<Waker>,
    }

    pub struct Reactor {
        epoll: sys::Epoll,
        waiters: RefCell<HashMap<RawFd, Waiters>>,
        events: RefCell<Vec<libc::epoll_event>>,
        stats: Stats,
    }

    impl Reactor {
        pub fn new() -> io::Result<Rc<Self>> {
            Ok(Rc::new(Self {
                epoll: sys::Epoll::new()?,
                waiters: RefCell::new(HashMap::new()),
                events: RefCell::new(Vec::with_capacity(256)),
                stats: Stats::default(),
            }))
        }

        /// Registered once, edge-triggered, both directions, token = fd. The descriptor is in
        /// the interest list before the first read, so no edge after that is missed.
        fn register(&self, fd: RawFd) -> io::Result<()> {
            self.epoll.add(fd, sys::EPOLLIN | sys::EPOLLOUT | sys::EPOLLRDHUP | sys::EPOLLET, fd as u64)?;
            self.waiters.borrow_mut().insert(fd, Waiters::default());
            Ok(())
        }

        fn deregister(&self, fd: RawFd) {
            // Must run before close(fd): the number may be reused by the next accept4.
            let _ = self.epoll.delete(fd);
            self.waiters.borrow_mut().remove(&fd);
        }

        fn park(&self, fd: RawFd, waker: &Waker, write: bool) {
            let mut map = self.waiters.borrow_mut();
            let w = map.get_mut(&fd).expect("fd registered with the reactor");
            let slot = if write { &mut w.writer } else { &mut w.reader };
            match slot {
                Some(old) if old.will_wake(waker) => {}
                _ => *slot = Some(waker.clone()),
            }
        }

        /// Blocks in epoll_wait, then wakes every task parked on a descriptor that got an edge.
        fn turn(&self) -> io::Result<()> {
            let mut events = self.events.borrow_mut();
            self.epoll.wait(&mut events, -1)?;
            bump(&self.stats.epoll_waits);
            for ev in events.iter() {
                let (token, flags) = sys::event_parts(ev);
                let (reader, writer) = {
                    let mut map = self.waiters.borrow_mut();
                    let Some(w) = map.get_mut(&(token as RawFd)) else { continue };
                    let fail = flags & (sys::EPOLLHUP | sys::EPOLLERR) != 0;
                    let readable = fail || flags & (sys::EPOLLIN | sys::EPOLLRDHUP) != 0;
                    let writable = fail || flags & sys::EPOLLOUT != 0;
                    (
                        if readable { w.reader.take() } else { None },
                        if writable { w.writer.take() } else { None },
                    )
                };
                for waker in [reader, writer].into_iter().flatten() {
                    bump(&self.stats.io_wakes);
                    waker.wake();
                }
            }
            Ok(())
        }
    }

    // ------------------------------------------------------------------ executor

    type LocalTask = Pin<Box<dyn Future<Output = ()>>>;
    type ReadyQueue = Arc<Mutex<VecDeque<u64>>>;

    /// The `Waker` of one task: waking just puts the task id back on the ready queue.
    /// `Wake` requires `Send + Sync` for the conversion into `Waker`, hence `Arc<Mutex<_>>`
    /// even though this executor only ever runs on one thread.
    struct TaskWaker {
        id: u64,
        ready: ReadyQueue,
    }

    impl Wake for TaskWaker {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.ready.lock().unwrap().push_back(self.id);
        }
    }

    /// Handle that lets a running task spawn more tasks. The futures need not be `Send`:
    /// they live in the executor's map and never leave this thread.
    #[derive(Clone)]
    pub struct Spawner {
        incoming: Rc<RefCell<Vec<LocalTask>>>,
    }

    impl Spawner {
        pub fn spawn(&self, fut: impl Future<Output = ()> + 'static) {
            self.incoming.borrow_mut().push(Box::pin(fut));
        }
    }

    pub struct Executor {
        reactor: Rc<Reactor>,
        spawner: Spawner,
        tasks: HashMap<u64, (LocalTask, Waker)>,
        ready: ReadyQueue,
        next_id: u64,
    }

    impl Executor {
        pub fn new(reactor: Rc<Reactor>) -> Self {
            Self {
                reactor,
                spawner: Spawner { incoming: Rc::default() },
                tasks: HashMap::new(),
                ready: Arc::default(),
                next_id: 0,
            }
        }

        pub fn spawner(&self) -> Spawner {
            self.spawner.clone()
        }

        /// Runs until no task is left: poll ready tasks; when none is ready, block in the
        /// reactor until I/O wakes one.
        pub fn run(mut self) -> io::Result<()> {
            loop {
                for fut in self.spawner.incoming.borrow_mut().drain(..) {
                    let id = self.next_id;
                    self.next_id += 1;
                    let waker = Waker::from(Arc::new(TaskWaker { id, ready: Arc::clone(&self.ready) }));
                    self.tasks.insert(id, (fut, waker));
                    self.ready.lock().unwrap().push_back(id);
                }
                self.reactor.stats.live_tasks.set(self.tasks.len());

                // `let` (not `match` on the expression) so the lock is released before polling;
                // a task that wakes itself would otherwise deadlock on this mutex.
                let next = self.ready.lock().unwrap().pop_front();
                match next {
                    Some(id) => {
                        // Ids are never reused, so a stale wake for a finished task is skipped.
                        let Some((fut, waker)) = self.tasks.get_mut(&id) else { continue };
                        let mut cx = Context::from_waker(waker);
                        bump(&self.reactor.stats.polls);
                        if fut.as_mut().poll(&mut cx).is_ready() {
                            self.tasks.remove(&id);
                        }
                    }
                    None if self.tasks.is_empty() => return Ok(()),
                    None => self.reactor.turn()?,
                }
            }
        }
    }

    // ------------------------------------------------------------------ I/O types

    pub struct TcpListener {
        fd: OwnedFd,
        reactor: Rc<Reactor>,
    }

    impl TcpListener {
        pub fn bind(port: u16, reactor: Rc<Reactor>) -> io::Result<Self> {
            let fd = sys::listen_tcp(port)?;
            reactor.register(fd.as_raw_fd())?;
            Ok(Self { fd, reactor })
        }

        pub fn accept(&self) -> Accept<'_> {
            Accept { listener: self }
        }
    }

    impl Drop for TcpListener {
        fn drop(&mut self) {
            self.reactor.deregister(self.fd.as_raw_fd());
        }
    }

    pub struct Accept<'a> {
        listener: &'a TcpListener,
    }

    impl Future for Accept<'_> {
        type Output = io::Result<TcpStream>;

        // Accept holds only a reference, so it is Unpin and `self` can be used directly.
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let l = self.listener;
            loop {
                match sys::accept(l.fd.as_raw_fd()) {
                    Ok(fd) => return Poll::Ready(TcpStream::new(fd, Rc::clone(&l.reactor))),
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        l.reactor.park(l.fd.as_raw_fd(), cx.waker(), false);
                        return Poll::Pending;
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(e) => return Poll::Ready(Err(e)),
                }
            }
        }
    }

    pub struct TcpStream {
        fd: OwnedFd,
        reactor: Rc<Reactor>,
    }

    impl TcpStream {
        fn new(fd: OwnedFd, reactor: Rc<Reactor>) -> io::Result<Self> {
            sys::set_sndbuf(fd.as_raw_fd(), LAB_SNDBUF)?;
            reactor.register(fd.as_raw_fd())?;
            Ok(Self { fd, reactor })
        }

        pub fn read<'a>(&'a self, buf: &'a mut [u8]) -> Read<'a> {
            Read { stream: self, buf }
        }

        pub fn write<'a>(&'a self, buf: &'a [u8]) -> Write<'a> {
            Write { stream: self, buf }
        }

        pub async fn write_all(&self, mut buf: &[u8]) -> io::Result<()> {
            while !buf.is_empty() {
                let n = self.write(buf).await?;
                buf = &buf[n..];
            }
            Ok(())
        }
    }

    impl Drop for TcpStream {
        fn drop(&mut self) {
            self.reactor.deregister(self.fd.as_raw_fd());
        }
    }

    pub struct Read<'a> {
        stream: &'a TcpStream,
        buf: &'a mut [u8],
    }

    impl Future for Read<'_> {
        type Output = io::Result<usize>;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.get_mut(); // fine: Read is Unpin
            let fd = this.stream.fd.as_raw_fd();
            loop {
                match sys::read(fd, this.buf) {
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        bump(&this.stream.reactor.stats.read_pending);
                        this.stream.reactor.park(fd, cx.waker(), false);
                        return Poll::Pending;
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    r => return Poll::Ready(r),
                }
            }
        }
    }

    pub struct Write<'a> {
        stream: &'a TcpStream,
        buf: &'a [u8],
    }

    impl Future for Write<'_> {
        type Output = io::Result<usize>;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let fd = self.stream.fd.as_raw_fd();
            loop {
                match sys::send(fd, self.buf) {
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        bump(&self.stream.reactor.stats.write_pending);
                        self.stream.reactor.park(fd, cx.waker(), true);
                        return Poll::Pending;
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    r => return Poll::Ready(r),
                }
            }
        }
    }

    // ------------------------------------------------------------------ the HTTP server

    async fn serve(listener: TcpListener, spawner: Spawner, reactor: Rc<Reactor>) {
        loop {
            match listener.accept().await {
                Ok(stream) => {
                    bump(&reactor.stats.accepted);
                    let reactor = Rc::clone(&reactor);
                    spawner.spawn(async move {
                        if handle(&stream, &reactor).await.is_err() {
                            bump(&reactor.stats.io_errors);
                        }
                    });
                }
                Err(e) if e.kind() == ErrorKind::ConnectionAborted => {}
                Err(e) => {
                    // EMFILE etc.: stop instead of spinning on an error that repeats at once.
                    eprintln!("mini_runtime: accept4: {e}");
                    return;
                }
            }
        }
    }

    async fn handle(stream: &TcpStream, reactor: &Reactor) -> io::Result<()> {
        let mut inbuf = Vec::new();
        let mut chunk = [0u8; 4096];
        let response = loop {
            let n = stream.read(&mut chunk).await?;
            if n == 0 {
                return Ok(()); // peer closed before sending a full head
            }
            inbuf.extend_from_slice(&chunk[..n]);
            match http::parse_request(&inbuf) {
                Ok(None) => continue,
                Ok(Some((req, _))) => {
                    bump(&reactor.stats.requests);
                    break http::route(&req, || reactor.stats.render());
                }
                Err(e) => break http::error_response(e),
            }
        };
        stream.write_all(&response).await
    }

    pub fn main() -> io::Result<()> {
        let port = std::env::args().nth(1).map_or(Ok(8081), |p| p.parse()).map_err(io::Error::other)?;
        let reactor = Reactor::new()?;
        let executor = Executor::new(Rc::clone(&reactor));
        let listener = TcpListener::bind(port, Rc::clone(&reactor))?;
        println!("mini_runtime: listening on 127.0.0.1:{port} (single-thread executor + epoll reactor, EPOLLET)");
        let spawner = executor.spawner();
        spawner.spawn(serve(listener, spawner.clone(), reactor));
        executor.run()
    }
}

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    rt::main()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("mini_runtime dùng epoll, chỉ chạy trên Linux. Chạy trong container:");
    eprintln!("  ./labs/linux.sh cargo run -p lab-capstone --bin mini_runtime -- 8081");
    std::process::exit(1);
}
