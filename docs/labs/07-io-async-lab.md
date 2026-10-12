# Lab P7 — I/O models và async (`O_NONBLOCK`, epoll, mio, `block_on`, Tokio)

> Đây là phần thực hành của [P7 · I/O và async](../07-io-async/00-tong-quan.md). Hãy đọc lý thuyết trước: các lab dưới đây chỉ minh họa những khái niệm đã giải thích ở các chương P7, và mỗi lab ghi rõ chương và mục cần đọc ([chương 01](../07-io-async/01-mo-hinh-io.md) và [chương 02](../07-io-async/02-select-poll-epoll.md) cho `O_NONBLOCK`/epoll, [chương 07](../07-io-async/07-executor-va-mio.md) cho mio, [chương 05](../07-io-async/05-future-poll-waker.md) và [chương 06](../07-io-async/06-async-await-pin.md) cho `Future`/`Waker`/`Pin`, [chương 08](../07-io-async/08-tokio.md) cho Tokio). Quy ước chú thích như ở [00-roadmap.md](../00-roadmap.md): mọi fact có chú thích trỏ tới nguồn và câu trích nguyên văn; phần ghi "(nhận xét của người soạn)" là diễn giải, không phải fact. Output là output thật, chạy trên Linux 6.8 (container `./labs/linux.sh`, aarch64, 4 CPU) và macOS; PID, port, địa chỉ, thời gian sẽ khác trên máy bạn. Đường dẫn `labs/...` trong các lệnh tính từ thư mục gốc của repo.

## Chuẩn bị và các bài lab

Crate: `labs/crates/lab-io-async`. Phụ thuộc: `libc` (gọi `fcntl`/`read`/`epoll_*` trực tiếp), `mio =1.2.4` (lab b), `tokio =1.53.2` (lab d, chỉ bật `rt`, `rt-multi-thread`, `net`, `io-util`). Các server dùng chung `ServerArgs` (`SERVER [PORT] [CONNS] [FLAGS...]`; cờ như `--read-once`, `--current-thread` tùy binary) và thoát sau `CONNS` kết nối để `strace -c` ra bảng tổng kết; `echo_load` (`echo_load [PORT] [CONNS] [MSGS] [MSG_SIZE]`) là client tải: mở nhiều kết nối, gửi `MSGS` message `SIZE` byte, kiểm tra byte nào gửi đi thì byte đó trở về.

Chuẩn bị:

```bash
(cd labs && cargo build -p lab-io-async && cargo clippy -p lab-io-async -- -D warnings)   # macOS: không cảnh báo
./labs/linux.sh cargo build -p lab-io-async                                              # trong container
```

### L0 `nonblocking_pipe`: blocking vs `O_NONBLOCK` (chạy được cả macOS và Linux)

Lý thuyết: [chương 01 mục 2](../07-io-async/01-mo-hinh-io.md) (`O_NONBLOCK`, `EAGAIN`, busy-polling) và [mục 7](../07-io-async/01-mo-hinh-io.md) (`WouldBlock` trong Rust).

```bash
./labs/linux.sh cargo run -p lab-io-async --bin nonblocking_pipe
```

Output trên Linux (exit code 0):

```text
blocking read   -> 5 bytes "hello" after 200 ms (the thread slept in read(2))
F_GETFL         -> 0o0 (O_NONBLOCK set: false)
F_SETFL         -> 0o4000 (O_NONBLOCK set: true)
non-block read  -> errno 11 (EAGAIN), ErrorKind::WouldBlock, after 1 us
EAGAIN = 11, EWOULDBLOCK = 11 on this OS
non-block read  -> 5 bytes "world"
non-block read  -> WouldBlock again: the pipe is drained
```

Trên macOS cùng binary cho `F_SETFL -> 0o4`, `errno 35`, `EAGAIN = 35, EWOULDBLOCK = 35` (số errno và giá trị `O_NONBLOCK` khác nhau theo OS; trên Linux `EAGAIN` là 11 và man page `read(2)` chỉ nói POSIX không bắt buộc hai hằng số bằng nhau[^man-read-eagain]).

Giải thích: read đầu tiên ngủ ~200 ms đến khi thread khác ghi. Sau `fcntl(F_SETFL, flags | O_NONBLOCK)` cùng lời gọi trên pipe rỗng trả lỗi sau ~1 µs, và `std::io::Error::kind()` là `WouldBlock`, đúng ánh xạ ở [chương 01 mục 7](../07-io-async/01-mo-hinh-io.md). Khi pipe có dữ liệu, read non-blocking hoạt động như read thường.

### L1 `epoll_echo` — lab (a): echo server non-blocking trên epoll thô

Lý thuyết: [chương 02 mục 4](../07-io-async/02-select-poll-epoll.md) (interest list, ready list), [mục 5](../07-io-async/02-select-poll-epoll.md) (level-triggered vs edge-triggered, quy tắc đọc tới `EAGAIN`), [chương 01 mục 3](../07-io-async/01-mo-hinh-io.md) (I/O multiplexing và readiness).

Server: `listener` non-blocking và level-triggered; mỗi kết nối được `accept4` rồi đặt `O_NONBLOCK`, đăng ký `EPOLLIN|EPOLLOUT|EPOLLRDHUP|EPOLLET`; khi có event thì `read` cho tới `EAGAIN`, ghi lại mọi thứ đã đọc.

```bash
./labs/linux.sh bash -c 'cd /work/labs && cargo build -p lab-io-async &&
  ./target-linux/debug/epoll_echo 7101 4 & sleep 0.5; ./target-linux/debug/echo_load 7101 4 200 64; wait'
```

Output (exit code của server và client: 0; địa chỉ/cổng client sẽ khác):

```text
[epoll] listening on 127.0.0.1:7101 (epfd=4, listener fd=3, mode=ET, drain until EAGAIN)
[epoll] accepted 127.0.0.1:34832 as fd 5
  ... (3 dòng accepted nữa, fd 6, 7, 8) ...
[load] 4 conns x 200 msgs x 64 B: 4 ok, 0 failed, 11 ms
[epoll] served 4 conns: epoll_wait calls=514, events=803, reads with data=800, reads -> EAGAIN=798, bytes echoed=51200, threads=1
```

Đọc output: 4 kết nối × 200 message × 64 B = 51200 byte được echo (khớp `bytes echoed`), chỉ 1 thread phục vụ cả 4 kết nối. 800 lần `read` có dữ liệu (một lần mỗi message) và 798 lần `read` kết thúc bằng `EAGAIN`: đó là bước "rút cạn" bắt buộc của ET.[^man-epoll-et-rule] Số `epoll_wait` (514) và số event (803) *không cố định* giữa các lần chạy; chúng phụ thuộc vào việc các message của các client đến cùng lúc hay lần lượt.

**strace thấy `epoll_wait`.** Chạy dưới strace (1 kết nối, 3 message × 8 byte; rút gọn, bỏ các dòng `write` ra stderr):

```bash
./labs/linux.sh bash -c 'cd /work/labs && strace -e trace=epoll_create1,epoll_ctl,epoll_pwait,accept4,read,write \
  -o /tmp/e.txt ./target-linux/debug/epoll_echo 7106 1 >/dev/null 2>&1 & sleep 1; ./target-linux/debug/echo_load 7106 1 3 8; wait; grep -v "write(2" /tmp/e.txt'
```

```text
epoll_create1(EPOLL_CLOEXEC)            = 4
epoll_ctl(4, EPOLL_CTL_ADD, 3, {events=EPOLLIN, data={u32=3, u64=3}}) = 0
epoll_pwait(4, [{events=EPOLLIN, data={u32=3, u64=3}}], 64, -1, NULL, 8) = 1
accept4(3, {sa_family=AF_INET, sin_port=htons(34304), sin_addr=inet_addr("127.0.0.1")}, [128 => 16], SOCK_CLOEXEC) = 5
epoll_ctl(4, EPOLL_CTL_ADD, 5, {events=EPOLLIN|EPOLLOUT|EPOLLRDHUP|EPOLLET, data={u32=5, u64=5}}) = 0
accept4(3, 0xfffff250eba0, [128], SOCK_CLOEXEC) = -1 EAGAIN (Resource temporarily unavailable)
epoll_pwait(4, [{events=EPOLLIN|EPOLLOUT, data={u32=5, u64=5}}], 64, -1, NULL, 8) = 1
read(5, "\0\1\2\3\4\5\6\7", 4096)       = 8
write(5, "\0\1\2\3\4\5\6\7", 8)         = 8
read(5, "\1\2\3\4\5\6\7\10", 4096)      = 8
write(5, "\1\2\3\4\5\6\7\10", 8)        = 8
read(5, "\2\3\4\5\6\7\10\t", 4096)      = 8
write(5, "\2\3\4\5\6\7\10\t", 8)        = 8
read(5, "", 4096)                       = 0
epoll_ctl(4, EPOLL_CTL_DEL, 5, 0xfffff250ee60) = 0
```

Đây chính là vòng đời ở [chương 02 mục 4](../07-io-async/02-select-poll-epoll.md): `epoll_create1` → `epoll_ctl(ADD)` listener → `epoll_pwait` ngủ (timeout `-1` = chờ vô hạn) → `accept4` → `epoll_ctl(ADD)` kết nối với `EPOLLET` → `accept4` tiếp đến `EAGAIN` (listener non-blocking, rút cạn accept queue) → `epoll_pwait` → `read`/`write` → `read` trả 0 (EOF, peer đóng[^man-read-eof]) → `epoll_ctl(DEL)`. Trên aarch64 tên syscall là `epoll_pwait` (về quan hệ tên hàm và tên syscall xem [P1 · chương 02 mục 3](../01-linux/02-system-call.md)). Dưới strace server chậm đi, nên các message dồn lại và chỉ cần 5 lần `epoll_pwait` thay vì 514 như lần chạy không strace; đừng so các con số đó với nhau.

**Tái hiện bug edge-triggered.** `--read-once` cố tình vi phạm hợp đồng ET: mỗi event chỉ `read` đúng 16 byte rồi quay lại `epoll_wait`.

```bash
./labs/linux.sh bash -c 'cd /work/labs && ./target-linux/debug/epoll_echo 7105 1 --read-once & sleep 0.5; ./target-linux/debug/echo_load 7105 1 1 64; wait'
```

```text
[epoll] listening on 127.0.0.1:7105 (epfd=4, listener fd=3, mode=ET, read once (broken))
[epoll] accepted 127.0.0.1:54772 as fd 5
[load] 1 conns x 1 msgs x 64 B: 0 ok, 1 failed, 3051 ms
[load] conn 0: message 0: Resource temporarily unavailable (os error 11)
[epoll] served 1 conns: epoll_wait calls=4, events=4, reads with data=3, reads -> EAGAIN=0, bytes echoed=48, threads=1
```

(exit code của `echo_load` là 1 như thiết kế. Sau khi client timeout và đóng kết nối, `epoll_wait` được đánh thức, server đọc nốt, thử echo và gặp `Broken pipe` — dòng `fd 5: Broken pipe` bị trộn vào dòng `[load]` trong output vì hai tiến trình cùng ghi stderr — rồi thoát với exit code 0.) Một message 64 byte, server chỉ echo được 48 byte (3 lần đọc × 16) rồi ngủ trong `epoll_wait`; 16 byte còn lại nằm trong buffer nhưng không có edge mới nên không có event. Client đặt read timeout 3 s nên báo lỗi kind `WouldBlock` (`os error 11` trên Linux); tài liệu `std` nói Unix thường trả `WouldBlock` khi read timeout, còn Windows có thể trả `TimedOut`.[^std-read-timeout] Đây chính là kịch bản "dữ liệu còn trong buffer mà `epoll_wait` treo" trong `epoll(7)`.[^man-epoll-et]

### L2 `mio_echo` — lab (b): cùng server viết bằng mio

Lý thuyết: [chương 07 mục 4](../07-io-async/07-executor-va-mio.md) (`mio` từng kiểu một), [mục 6](../07-io-async/07-executor-va-mio.md) (spurious event, draining readiness, edge-triggered), [chương 02 mục 8](../07-io-async/02-select-poll-epoll.md) (mio và `EPOLLET`).

```bash
./labs/linux.sh bash -c 'cd /work/labs && ./target-linux/debug/mio_echo 7102 4 & sleep 0.5; ./target-linux/debug/echo_load 7102 4 200 64; wait'
```

```text
[mio] listening on 127.0.0.1:7102
[mio] accepted 127.0.0.1:39788 as Token(1)
  ... (Token(2), Token(3), Token(4)) ...
[load] 4 conns x 200 msgs x 64 B: 4 ok, 0 failed, 11 ms
[mio] served 4 conns: Poll::poll calls=589, events=802, reads -> WouldBlock=797, threads=1
```

So với L1: cùng vòng `poll` → rút cạn đến `WouldBlock`, `Token` thay cho số fd tự lưu trong `epoll_event.data`. Chạy dưới strace, mio đăng ký như sau (1 kết nối, 2 message):

```text
epoll_create1(EPOLL_CLOEXEC)            = 3
epoll_ctl(3, EPOLL_CTL_ADD, 4, {events=EPOLLIN|EPOLLRDHUP|EPOLLET, data={u32=0, u64=0}}) = 0
epoll_ctl(3, EPOLL_CTL_ADD, 5, {events=EPOLLIN|EPOLLOUT|EPOLLRDHUP|EPOLLET, data={u32=1, u64=1}}) = 0
epoll_ctl(3, EPOLL_CTL_DEL, 5, NULL) = 0
```

Quan sát: mio đăng ký cả listener lẫn socket với `EPOLLET`, và `data` chứa giá trị `Token` (0, 1). Điều này nhất quán với yêu cầu rút cạn readiness của mio ([chương 07 mục 6](../07-io-async/07-executor-va-mio.md)):[^mio-drain] tài liệu mio chỉ nói không có bảo đảm event kế tiếp nếu không rút cạn; còn việc mio đặt `EPOLLET` trong mọi lần đăng ký được xác nhận bằng source (xem [chương 02 mục 8](../07-io-async/02-select-poll-epoll.md)),[^mio-src] strace ở trên là quan sát tương ứng. Cùng binary chạy được trên macOS (selector là kqueue,[^mio-impl] không có `/proc` nên `threads=n/a`); lần chạy trên macOS: `[mio] served 4 conns: Poll::poll calls=505, events=1443, reads -> WouldBlock=1435` và `[load] 4 conns x 200 msgs x 64 B: 4 ok, 0 failed, 17 ms`.

### L3 `mini_executor` — lab (c): `block_on` với `Waker` dựa trên park/unpark

Lý thuyết: [chương 05 mục 2](../07-io-async/05-future-poll-waker.md) (`Future`, `poll`), [mục 3](../07-io-async/05-future-poll-waker.md) (hợp đồng của `poll`), [mục 4](../07-io-async/05-future-poll-waker.md) (`Context`, `Waker`), [mục 6](../07-io-async/05-future-poll-waker.md) (`Wake`), [mục 7](../07-io-async/05-future-poll-waker.md) (Timer thủ công và `block_on` bằng `park`/`unpark`).

Chạy ở macOS hay Linux đều được:

```bash
cd labs && cargo run -q -p lab-io-async --bin mini_executor
```

Output (exit code 0, rút gọn các dòng `poll #` của mục 3):

```text
1) leaf future, 3 Pending then Ready:
  countdown: poll #1 -> Pending (2 left)
  countdown: poll #2 -> Pending (1 left)
  countdown: poll #3 -> Pending (0 left)
  countdown: poll #4 -> Ready
   output=4 polls=4 parks=3 wakes=3

2) future that wakes itself before returning Pending:
   polls=2 parks=0 wakes=1

3) async fn with two awaits (one task, one Waker for both leaves):
   (first: 3 polls, second: 2 polls)
   output=5 polls=4 parks=4 wakes=3

4) size of the compiler-generated futures (not started yet):
   two_steps                  :    32 bytes
   keeps_buffer_across_await  :  1056 bytes
   drops_buffer_before_await  :    32 bytes
   Countdown (hand-written)   :    24 bytes
  keep: poll #1 -> Ready
   keeps_buffer_across_await ran: polls=1 parks=0 wakes=0
```

Cách `block_on` hoạt động (xem `src/bin/mini_executor.rs`): `pin!(fut)` ghim future trên stack; `ThreadWaker` implement `std::task::Wake` (nó có `Thread` handle và gọi `unpark`);[^std-wake] vòng lặp `poll` → nếu `Pending` thì `thread::park()` cho tới khi cờ `woken` được đặt. Executor kiểm tra cờ chứ không tin việc `park` trả về, vì `park` được phép trả về giả (spurious) mà không tiêu thụ token.[^std-park]

Đọc từng ca:

1. `Countdown` trả `Pending` 3 lần, mỗi lần nhân bản `Waker` đưa cho một thread hẹn giờ 20 ms (đóng vai reactor). `polls=4` = 3 `Pending` + 1 `Ready`; `wakes=3` = một wakeup cho mỗi `Pending`; `parks=3`. Đúng quy tắc: `Pending` đi kèm một wakeup đã lên lịch.[^std-future-clone]
2. `YieldOnce` tự gọi `wake_by_ref()` *trước* khi trả `Pending`. Token park đã có sẵn nên executor không bao giờ ngủ (`parks=0`) và poll lần hai ngay.
3. `async fn two_steps` có hai `.await` nối tiếp: `first` cần 3 lần poll, `second` cần 2, nhưng executor chỉ thấy *một* future và `polls=4` (không phải 5): lần poll thứ ba của `first` chạy tiếp thẳng vào lần poll đầu của `second` trong cùng một lời gọi `poll` của executor. Đó là cách `.await` hoạt động: poll future bên trong và trả `Pending` ra ngoài nếu nó `Pending`.[^ref-await] `wakes=3` chứ không phải 4: lần `Ready` của `first` không sinh wakeup mới. Chú ý `parks=4` > `wakes=3`: lần `park` đầu tiên của ca 3 trả về ngay vì còn token thừa từ ca 2 (ca 2 gọi `unpark` mà `block_on` không bao giờ `park` để tiêu thụ). Điều này khớp lời tài liệu: `unpark` rồi `park` thì lần `park` thứ hai trả ngay.[^std-park] Cờ `woken` giúp vòng lặp bỏ qua lần trả về giả này và `park` lại — đó là lý do cần cờ. (Giải thích này là suy luận từ code và tài liệu `park`, khớp với số đếm quan sát được; tôi không dùng debugger để xác nhận token.)
4. Kích thước: hai `async fn` chỉ khác nhau ở chỗ mảng `[7u8; 1024]` có còn sống qua `.await` hay không → 1056 byte so với 32 byte. Đúng với mô tả "biến cục bộ vắt qua `.await` là một phần trạng thái của future";[^std-pin-macro] mảng được dùng trước `.await` rồi bỏ thì không nằm trong state. Con số cụ thể là kết quả đo trên rustc 1.95.0, không phải bảo đảm.

### L4 `tokio_echo` và `thread_echo` — lab (d): so sánh thread và syscall

Lý thuyết: [chương 08 mục 1](../07-io-async/08-tokio.md) (runtime của Tokio) và [mục 2](../07-io-async/08-tokio.md) (multi-thread vs current-thread), [chương 08 mục 9](../07-io-async/08-tokio.md) (đo bằng `strace -f -c`), [chương 04 mục 2](../07-io-async/04-kien-truc-event-loop.md) (thread-per-connection).

Cùng giao thức, cùng client tải (`echo_load 4 200 64`), mỗi server thoát sau 4 kết nối. `tokio_echo` có hai chế độ: mặc định multi-thread, hoặc `--current-thread`. `thread_echo` là baseline thread-per-connection (blocking `read`/`write` trong thread riêng).

```bash
./labs/linux.sh bash -c 'cd /work/labs && ./target-linux/debug/tokio_echo 7103 4 & sleep 0.5; ./target-linux/debug/echo_load 7103 4 200 64; wait'
```

```text
[tokio] scheduler=multi_thread, threads right after build=5
[tokio] listening on 127.0.0.1:7103
  ... (4 dòng accepted) ...
[tokio] served 4 conns, threads=5
[load] 4 conns x 200 msgs x 64 B: 4 ok, 0 failed, 11 ms
```

Với `--current-thread`: `threads right after build=1`, `served 4 conns, threads=1`. `thread_echo`: `[thread] served 4 conns, threads at peak=5` (1 main + 4 worker).

Thread của Tokio multi-thread: 5 = main + 4 worker; máy chạy có `nproc` = 4. Khớp với tài liệu: mặc định một worker mỗi core.[^tokio-multi][^tokio-workers] (Nếu số thread của bạn khác, đó là do số core của máy bạn.) Thread của `thread_echo` tăng theo số kết nối (4 kết nối → 4 worker), còn số thread của Tokio phụ thuộc số core, không phụ thuộc số kết nối; còn task của Tokio do runtime lập lịch chứ không phải hệ điều hành, và tài liệu nói tạo/chuyển task không cần context switch, rẻ hơn OS thread.[^tokio-light] (Tôi chỉ chạy với 4 kết nối; "số thread Tokio không phụ thuộc số kết nối" là suy luận của người soạn từ số worker mặc định theo số core, chưa đo ở tải lớn; tài liệu Tokio không nói về số thread theo số kết nối.)

**`strace -f -c`.** Mẫu lệnh (đổi `<bin>`, `<port>` và thêm `--current-thread` nếu cần):

```bash
./labs/linux.sh bash -c 'cd /work/labs && strace -f -c -o /tmp/s.txt ./target-linux/debug/<bin> <port> 4 >/dev/null 2>&1 &
  sleep 1.5; ./target-linux/debug/echo_load <port> 4 200 64 >/dev/null; wait; cat /tmp/s.txt'
```

Các dòng liên quan của năm lần chạy (đã lọc và rút gọn; "total" là tổng mọi syscall; số lượng và thời gian phụ thuộc máy và mỗi lần chạy):

| Server | threads | `clone` | `epoll_pwait` | `epoll_ctl` | `futex` | `read`/`recvfrom` | `write`/`sendto` | total syscalls |
|---|---|---|---|---|---|---|---|---|
| `epoll_echo` (libc) | 1 | 0 | 2 | 9 | 0 | 809 `read` | 862 `write`\* | 1778 |
| `mio_echo` | 1 | 0 | 2 | 9 | 0 | 804 `recvfrom` | 800 `sendto` | 1782 |
| `tokio_echo --current-thread` | 1 | 0 | 203 | 11 | 1 | 804 `recvfrom` | 800 `sendto` | 1992 |
| `tokio_echo` (multi-thread) | 5 | 4 | 431 | 11 | 863 (105 lỗi) | 804 `recvfrom` | 800 `sendto` | 3208 |
| `thread_echo` | 5 | 4 | 0 | 0 | 11 | 804 `recvfrom` | 800 `sendto` | 1844 |

\* `write` của `epoll_echo` gồm cả các lần `write` ra stderr của `eprintln!` (mỗi mảnh chuỗi được format là một syscall, xem strace ở L1); `std` dùng `recvfrom`/`sendto` cho socket còn code libc thô của lab dùng `read`/`write`. 804 = 800 message + 4 lần đọc trả 0 (EOF). Các con số lấy từ MỘT lần chạy lọc bằng `awk` (số `epoll_pwait` của `epoll_echo` là 5 ở lần chạy đầu và 2 ở lần này; số `futex` cũng dao động giữa các lần chạy), nên chỉ so sánh *bậc độ lớn*.

Những gì rút ra được (phần số liệu là quan sát, phần diễn giải là nhận xét của người soạn):

1. Cả năm server đều làm cùng một lượng I/O thật: ~800 lần đọc socket và ~800 lần ghi socket cho 800 message. Sự khác biệt nằm ở *phần điều phối*.
2. `epoll_echo` và `mio_echo` chỉ có `epoll_create1` (1), `epoll_ctl` (9 = 1 listener + 4 `ADD` + 4 `DEL`, phép cộng của tôi khớp số đếm) và rất ít `epoll_pwait` dưới strace. Không có `clone`, không có `futex`.
3. Tokio current-thread có số `epoll_pwait` cao hơn nhiều (203) so với hai bản kia trong cùng điều kiện strace; tôi không điều tra nguyên nhân (có thể do cách runtime quay lại driver giữa các lần poll task — [INFERENCE], chưa kiểm chứng).
4. Tokio multi-thread có 4 `clone` (4 worker) và 863 `futex` (105 lỗi; tôi không in chi tiết mã lỗi). `futex` là syscall nhiều nhất trong lần chạy này (863/3208): các worker đánh thức/ngủ nhau (diễn giải của người soạn, tôi không truy vết từng futex). Với tải nhỏ như thế này (4 kết nối, mỗi kết nối hầu như lần lượt gửi–nhận), multi-thread scheduler tốn hơn current-thread; đây là đặc điểm của *bài đo này*, không phải kết luận chung về hiệu năng Tokio.
5. `thread_echo` có `clone` = 4 (một thread mỗi kết nối) và `futex` ít (11) vì mỗi thread chặn trực tiếp trong `recvfrom`; nó không cần epoll. Chi phí của nó nằm ở số thread tăng theo số kết nối (xem [chương 04 mục 2](../07-io-async/04-kien-truc-event-loop.md)), chứ không phải ở số syscall ở quy mô 4 kết nối.

Các dòng `epoll_ctl` trong strace của Tokio giống cách mio đăng ký ở L2 — ví dụ listener/stream được thêm với `events=EPOLLIN|EPOLLOUT|EPOLLRDHUP|EPOLLET`:

```text
epoll_create1(EPOLL_CLOEXEC)            = 3
eventfd2(0, EFD_CLOEXEC|EFD_NONBLOCK)   = 4
epoll_ctl(3, EPOLL_CTL_ADD, 4, {events=EPOLLIN|EPOLLRDHUP|EPOLLET, data={u32=0, u64=0}}) = 0
clone(...) = 74   (x4: các worker thread, tid 74..77)
epoll_ctl(5, EPOLL_CTL_ADD, 6, {events=EPOLLIN|EPOLLOUT|EPOLLRDHUP|EPOLLET, ...}) = 0
```

`eventfd2` tạo ra một fd dùng để *đánh thức driver từ thread khác* (đây là diễn giải của tôi từ việc nó được đăng ký vào epoll ngay sau khi tạo; mio có kiểu `Waker` "allows cross-thread waking of Poll" — xem docs.rs[^mio-waker] và [chương 07 mục 5](../07-io-async/07-executor-va-mio.md) về `mio::Waker` so với `std::task::Waker`).

### Bài tập mở rộng

1. Sửa `epoll_echo` thành level-triggered (bỏ `EPOLLET`, chỉ `read` một lần mỗi event). Thêm cờ chạy kiểu "đọc một lần" cho bản LT này, chạy lại với `echo_load`, và giải thích vì sao client vẫn nhận đủ 64 byte.[^man-epoll-lt]
2. Thêm `EPOLLONESHOT` vào kết nối trong `epoll_echo` và `EPOLL_CTL_MOD` để bật lại sau mỗi lần xử lý. Quan sát sự khác biệt số `epoll_ctl` trong `strace -c`.[^man-epollctl-oneshot]
3. Trong `mini_executor` thêm một ca `block_on` cho future tự đếm số lần `wake` bị gọi từ hai thread cùng lúc. `Waker` có `Send + Sync` không? Dựa vào đâu?[^std-waker-send]
4. Chạy `echo_load` với 1000 kết nối vào `thread_echo` và `tokio_echo` (đổi `CONNS` của server cho khớp). Đo `ps -o nlwp` hoặc `/proc/<pid>/status` và `ulimit -n`; tự rút kết luận bằng số đo của bạn.
5. Viết lại ví dụ ở [chương 06 mục 3](../07-io-async/06-async-await-pin.md) (state machine: biến sống qua `.await` nằm trong future): tạo `async fn` giữ một mảng 4096 byte qua `.await` và dự đoán `size_of_val` trước khi chạy.
6. Chạy `mio_echo` trên macOS rồi dùng `dtruss`/`sudo fs_usage` (nếu có thể) để tìm `kevent`; so với `epoll_pwait` trên Linux.

## Câu hỏi tự kiểm tra (về output lab)

Các câu hỏi lý thuyết thuần nằm ở phần "Câu hỏi tự kiểm tra" của từng chương [P7](../07-io-async/00-tong-quan.md); các câu dưới đây cần output của lab.

1. Trong output của `nonblocking_pipe` (L0), vì sao cùng một lời gọi `read` lúc thì ngủ ~200 ms, lúc thì trả lỗi sau vài µs? Lỗi đó là gì?
<details><summary>Đáp án</summary>

Lần đầu fd ở chế độ blocking nên thread ngủ trong `read(2)` tới khi writer ghi; sau `fcntl(F_SETFL, flags | O_NONBLOCK)` read trên pipe rỗng trả `-1`/`EAGAIN`, `std` ánh xạ thành `ErrorKind::WouldBlock`. Trên Linux `EAGAIN` = 11, trên macOS = 35.[^man-read-eagain]

</details>

2. Với `epoll_echo --read-once`, chuyện gì xảy ra và vì sao? Quy tắc dùng ET đúng cách là gì?
<details><summary>Đáp án</summary>

Fd phải non-blocking và chỉ chờ sự kiện sau khi `read`/`write` trả `EAGAIN`.[^man-epoll-et-rule] Lab L1 với `--read-once`: server echo 48/64 byte rồi treo, client timeout sau ~3 s (xem output ở L1).

</details>

3. Mio ghi gì về việc xử lý readiness event, và `mio_echo` thực hiện nó ở đâu?
<details><summary>Đáp án</summary>

Phải lặp thao tác tới khi `WouldBlock`; không thì không bảo đảm có event kế tiếp.[^mio-drain] `drain_and_echo` đọc đến `WouldBlock`, và vòng `accept` cũng đọc đến `WouldBlock`.[^mio-accept]

</details>

4. Vì sao `block_on` trong lab không tin vào việc `park` trả về, mà dùng cờ `woken`? Hãy dùng số liệu `parks=4, wakes=3` để minh họa.
<details><summary>Đáp án</summary>

`park` có thể trả về giả, và token có thể còn từ lần `unpark` trước đó.[^std-park] Ở ca 3, token thừa từ ca 2 làm lần `park` đầu trả ngay, nên `parks` > `wakes`; cờ `woken` cho phép vòng lặp `park` lại thay vì poll sớm một cách vô nghĩa.

</details>

5. Vì sao `tokio_echo` multi-thread có đúng 5 thread với 4 kết nối trên máy thử, và con số đó sẽ đổi theo gì?
<details><summary>Đáp án</summary>

4 worker (tài liệu: mặc định mỗi core một) + thread main gọi `block_on` (phần "+ main" là diễn giải của người soạn từ số đếm quan sát được); đổi theo số core hoặc `worker_threads`.[^tokio-multi][^tokio-workers]

</details>

[^man-epoll-et]: epoll(7), mục Level-triggered and edge-triggered — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "edge-triggered mode delivers events only when changes occur on the monitored file descriptor."
[^man-epoll-et-rule]: epoll(7), mục Level-triggered and edge-triggered — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "(1) with nonblocking file descriptors; and (2) by waiting for an event only after read(2) or write(2) return EAGAIN."
[^man-epoll-lt]: epoll(7), mục Level-triggered and edge-triggered — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "when used as a level-triggered interface (the default, when EPOLLET is not specified), epoll is simply a faster poll(2)"
[^man-epollctl-oneshot]: epoll_ctl(2), mục EPOLLONESHOT — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "The user must call epoll_ctl() with EPOLL_CTL_MOD to rearm the file descriptor with a new event mask."
[^man-read-eagain]: read(2), mục ERRORS — <https://manpages.debian.org/bookworm/manpages-dev/read.2.en.html> — "POSIX.1-2001 allows either error to be returned for this case, and does not require these constants to have the same value"
[^man-read-eof]: read(2), mục RETURN VALUE — <https://manpages.debian.org/bookworm/manpages-dev/read.2.en.html> — "On success, the number of bytes read is returned (zero indicates end of file)"
[^mio-accept]: mio 1.2.4, struct net::TcpListener, method accept — <https://docs.rs/mio/1.2.4/mio/net/struct.TcpListener.html> — "This means a stream may be ready at a later point and one should wait for an event before calling accept again."
[^mio-drain]: mio 1.2.4, struct Poll, mục Draining readiness — <https://docs.rs/mio/1.2.4/mio/struct.Poll.html> — "Once a readiness event is received, the corresponding operation must be performed repeatedly until it returns WouldBlock."
[^mio-impl]: mio 1.2.4, struct Poll, mục Implementation notes — <https://docs.rs/mio/1.2.4/mio/struct.Poll.html> — "Poll is backed by the selector provided by the operating system."
[^mio-waker]: mio 1.2.4, crate mio, mục Structs — <https://docs.rs/mio/1.2.4/mio/> — "Waker allows cross-thread waking of Poll."
[^ref-await]: Rust Reference 1.95.0, Await expressions — <https://doc.rust-lang.org/1.95.0/reference/expressions/await-expr.html> — "If the call to poll returns Poll::Pending, then the future returns Poll::Pending, suspending its state so that, when the surrounding async context is re-polled, execution returns to step 3"
[^std-future-clone]: Rust std 1.95.0, trait Future, method poll — <https://doc.rust-lang.org/1.95.0/std/future/trait.Future.html> — "When a future is not ready yet, poll returns Poll::Pending and stores a clone of the Waker copied from the current Context."
[^std-park]: Rust std 1.95.0, fn thread::park — <https://doc.rust-lang.org/1.95.0/std/thread/fn.park.html> — "Because the token can be held by a thread even if it is currently not parked, unpark followed by park will result in the second call returning immediately."
[^std-pin-macro]: Rust std 1.95.0, macro pin! — <https://doc.rust-lang.org/1.95.0/std/pin/macro.pin.html> — "In async functions or blocks however, any locals crossing an .await point are part of the state captured by the Future, and will use the storage of those."
[^std-read-timeout]: Rust std 1.95.0, TcpStream::set_read_timeout — <https://doc.rust-lang.org/1.95.0/std/net/struct.TcpStream.html> — "For example Unix typically returns an error of the kind WouldBlock, but Windows may return TimedOut."
[^std-wake]: Rust std 1.95.0, trait Wake — <https://doc.rust-lang.org/1.95.0/std/task/trait.Wake.html> — "It supports the common executor design in which the data used to wake up a task is stored in an Arc."
[^std-waker-send]: Rust std 1.95.0, struct Waker — <https://doc.rust-lang.org/1.95.0/std/task/struct.Waker.html> — "Implements Clone, Send, and Sync; therefore, a waker may be invoked from any thread, including ones not in any way managed by the executor."
[^tokio-light]: Tokio 1.53.2, module tokio::task — <https://docs.rs/tokio/1.53.2/tokio/task/index.html> — "creating new tasks or switching between tasks does not require a context switch and has fairly low overhead. Creating, running, and destroying large numbers of tasks is quite cheap, especially compared to OS threads."
[^tokio-multi]: Tokio 1.53.2, module tokio::runtime, mục Multi-Thread Scheduler — <https://docs.rs/tokio/1.53.2/tokio/runtime/index.html> — "The multi-thread scheduler executes futures on a thread pool, using a work-stealing strategy. By default, it will start a worker thread for each CPU core available on the system."
[^tokio-workers]: Tokio 1.53.2, Builder::worker_threads — <https://docs.rs/tokio/1.53.2/tokio/runtime/struct.Builder.html> — "The default value is the number of cores available to the system."
[^mio-src]: mio v1.2.4, src/sys/unix/selector/epoll.rs, fn interests_to_epoll — <https://raw.githubusercontent.com/tokio-rs/mio/v1.2.4/src/sys/unix/selector/epoll.rs> — "fn interests_to_epoll(interests: Interest) -> u32 { let mut kind = EPOLLET;"
