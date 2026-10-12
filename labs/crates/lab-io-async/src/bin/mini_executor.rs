//! Lab (c): a minimal executor. `block_on` polls one future on the current thread
//! and parks the thread between polls; the Waker (built from `std::task::Wake`)
//! unparks it.
//!
//! Runs anywhere: `cargo run -p lab-io-async --bin mini_executor`.

use std::future::Future;
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};
use std::time::Duration;

/// Waker for `block_on`: records that a wake happened, then unparks the thread.
struct ThreadWaker {
    thread: Thread,
    /// Set by `wake`, cleared by `block_on`. `park` may return spuriously, so the
    /// executor trusts this flag, not the fact that `park` returned.
    woken: AtomicBool,
    wakes: AtomicU32,
}

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.wakes.fetch_add(1, Ordering::Relaxed);
        self.woken.store(true, Ordering::Release);
        self.thread.unpark();
    }
}

struct Report {
    polls: u32,
    parks: u32,
    wakes: u32,
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "polls={} parks={} wakes={}", self.polls, self.parks, self.wakes)
    }
}

/// Runs `fut` to completion on this thread.
fn block_on<F: Future>(fut: F) -> (F::Output, Report) {
    // `poll` takes `Pin<&mut Self>`: pin the future on this stack frame. It is
    // never moved again, so a self-referential async state machine stays valid.
    let mut fut = pin!(fut);
    let tw = Arc::new(ThreadWaker {
        thread: thread::current(),
        woken: AtomicBool::new(false),
        wakes: AtomicU32::new(0),
    });
    let waker = Waker::from(Arc::clone(&tw));
    let mut cx = Context::from_waker(&waker);
    let (mut polls, mut parks) = (0, 0);
    loop {
        polls += 1;
        if let Poll::Ready(out) = fut.as_mut().poll(&mut cx) {
            let wakes = tw.wakes.load(Ordering::Relaxed);
            return (out, Report { polls, parks, wakes });
        }
        // Pending: sleep until some Waker clone says progress is possible.
        while !tw.woken.swap(false, Ordering::Acquire) {
            parks += 1;
            thread::park();
        }
    }
}

/// A hand-written leaf future: Pending `steps` times, then Ready(number of polls).
/// Each Pending hands a clone of the Waker to a timer thread, the way a reactor
/// would keep it until the fd becomes ready.
struct Countdown {
    name: &'static str,
    steps: u32,
    polls: u32,
}

impl Countdown {
    fn new(name: &'static str, steps: u32) -> Self {
        Self { name, steps, polls: 0 }
    }
}

impl Future for Countdown {
    type Output = u32;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u32> {
        // Countdown has no self-references, so it is Unpin and Pin derefs to &mut.
        self.polls += 1;
        if self.steps == 0 {
            println!("  {}: poll #{} -> Ready", self.name, self.polls);
            return Poll::Ready(self.polls);
        }
        self.steps -= 1;
        println!("  {}: poll #{} -> Pending ({} left)", self.name, self.polls, self.steps);
        let waker = cx.waker().clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            waker.wake();
        });
        Poll::Pending
    }
}

/// Wakes itself before returning Pending: the token is already there when the
/// executor would park, so it never parks.
struct YieldOnce(bool);

impl Future for YieldOnce {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            return Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

/// Two awaits in sequence: the compiler turns this body into a state machine
/// with one state per await point.
async fn two_steps() -> u32 {
    let a = Countdown::new("first", 2).await;
    let b = Countdown::new("second", 1).await;
    a + b
}

/// `buf` is used after the await, so it must live inside the future's state.
async fn keeps_buffer_across_await() -> u8 {
    let buf = [7u8; 1024];
    Countdown::new("keep", 0).await;
    buf[0]
}

/// `buf` is dead before the await, so the state machine does not need to store it.
async fn drops_buffer_before_await() -> u8 {
    let first = {
        let buf = [7u8; 1024];
        buf[0]
    };
    Countdown::new("drop", 0).await;
    first
}

fn main() {
    println!("1) leaf future, 3 Pending then Ready:");
    let (polls, r) = block_on(Countdown::new("countdown", 3));
    println!("   output={polls} {r}\n");

    println!("2) future that wakes itself before returning Pending:");
    let ((), r) = block_on(YieldOnce(false));
    println!("   {r}\n");

    println!("3) async fn with two awaits (one task, one Waker for both leaves):");
    let (sum, r) = block_on(two_steps());
    println!("   output={sum} {r}\n");

    println!("4) size of the compiler-generated futures (not started yet):");
    println!("   two_steps                  : {:>5} bytes", size_of_val(&two_steps()));
    println!("   keeps_buffer_across_await  : {:>5} bytes", size_of_val(&keeps_buffer_across_await()));
    println!("   drops_buffer_before_await  : {:>5} bytes", size_of_val(&drops_buffer_before_await()));
    println!("   Countdown (hand-written)   : {:>5} bytes", size_of::<Countdown>());
    let (_, r) = block_on(keeps_buffer_across_await());
    println!("   keeps_buffer_across_await ran: {r}");
}
