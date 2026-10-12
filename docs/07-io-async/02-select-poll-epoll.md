# P7 · Chương 02 — select, poll và epoll

> Phạm vi: mọi khẳng định về kernel và syscall trong chương này là về **Linux** (man page Debian bookworm, tức Linux man-pages 6.03; source kernel ghim tag `v6.8`). Các thí nghiệm chạy trong container lab (`./labs/linux.sh`, kernel 6.8, **aarch64**, glibc, `gcc`, `strace`); kết quả chỉ là *quan sát* một lần chạy, không phải bảo đảm của tài liệu. epoll là API riêng của Linux;[^epoll-linux] cơ chế tương ứng trên BSD và macOS nằm ở [Chương 03](03-kqueue-io-uring.md).

## Bạn sẽ hiểu được gì

1. **Q1.** Giới hạn 1024 fd của `select(2)` là của kernel hay của glibc? Vì sao mỗi vòng lặp phải dựng lại `fd_set`? → mục 2
2. **Q2.** `poll(2)` sửa được gì so với `select`; `revents` khác `events` ở đâu; fd âm hoặc fd chưa mở thì chuyện gì xảy ra? → mục 3
3. **Q3.** Interest list và ready list của epoll là gì; ai đưa fd vào ready list, ai lấy ra; vì sao epoll không phải quét cả tập mỗi lần? → mục 4
4. **Q4.** Level-triggered khác edge-triggered thế nào; vì sao ví dụ 2 KB trong `epoll(7)` làm `epoll_wait` treo ở chế độ ET, và quy tắc "đọc tới `EAGAIN`" ra đời từ đó? → mục 5
5. **Q5.** Vì sao `close(fd)` đôi khi *không* làm fd biến mất khỏi epoll (và event cache là gì)? Vì sao `epoll_ctl` với file thường trả `EPERM`? → mục 6
6. **Q6.** `EPOLLONESHOT` và `EPOLLEXCLUSIVE` giải quyết vấn đề gì; "một hoặc nhiều" nghĩa là gì; `EPOLLRDHUP`, `EPOLLHUP`, `EPOLLERR` và giới hạn bộ nhớ ra sao? → mục 7
7. **Q7.** mio đăng ký mọi fd với epoll ở chế độ nào, docs mio nói gì về nó, và điều đó đặt nghĩa vụ gì lên code của bạn? → mục 8

## Kiến thức cần trước

- [Chương 01 — Mô hình I/O](01-mo-hinh-io.md), mục 2–3: `O_NONBLOCK`, `EAGAIN`, khái niệm *readiness* (fd "sẵn sàng" nghĩa là gì) và vì sao cần chờ nhiều fd cùng lúc.
- [P1 · Chương 04 — File descriptor và VFS](../01-linux/04-file-descriptor-va-vfs.md), mục 1–3: fd, *open file description* (OFD), `dup` và `fork`. Đây là nền cho mục 6, nơi fd đã đóng mà vẫn nằm trong interest list.

## 1. Bài toán: một thread, nhiều fd

Suốt chương này ta bám theo `gateway`, một service giả định (nhận xét của người soạn: ví dụ minh họa, không phải code trong repo). `gateway` là một process Rust giữ kết nối TCP lâu dài với hàng chục nghìn client để đẩy thông báo xuống cho họ. Phần lớn kết nối nằm im, và ở mỗi thời điểm chỉ một nhúm nhỏ có dữ liệu cần đọc. Ta muốn một thread phục vụ hết chúng.

Muốn vậy, mọi socket phải ở chế độ non-blocking, nếu không một `read` trên socket im lặng sẽ treo cả thread. Nhưng `O_NONBLOCK` mới giải được nửa bài toán: nó cho `read` trả lỗi ngay khi chưa có dữ liệu, chứ không cho thread biết *khi nào* nên thử lại và thử trên socket nào. Gọi `read` quay vòng qua 10.000 socket thì chỉ đốt CPU (nhận xét của người soạn). Thứ ta cần là một syscall cho thread nằm chờ, và chỉ dậy khi có socket đáng đọc. Đó là việc của `select`, `poll` và `epoll`. Cả ba chỉ báo fd *sẵn sàng*, tức một thao tác I/O trên fd đó (với `O_NONBLOCK` tắt) sẽ không chặn.[^open-poll] [Chương 01, mục 2–3](01-mo-hinh-io.md) đã bàn kỹ phần readiness này.

Chương này đi qua ba giao diện đó theo thứ tự `select`, `poll`, `epoll`, vì mỗi cái bỏ bớt một vướng mắc của cái trước. Ta dừng ở epoll: trên Linux nó là nền của mio, và qua mio là của Tokio ([Chương 07](07-executor-va-mio.md), [Chương 08](08-tokio.md)).

## 2. `select(2)`: giới hạn 1024 và value-result (Q1)

> **Trả lời ngắn.** Giới hạn 1024 là của glibc, không phải của kernel: glibc làm `fd_set` thành kiểu có kích thước cố định, `FD_SETSIZE` = 1024.[^sel-glibc] Nó áp lên *số* của fd (phải nhỏ hơn 1024), không phải lên số lượng fd.[^sel-warn] Mỗi vòng lặp phải dựng lại `fd_set` vì `select` sửa chính các tập đó để báo fd nào ready.[^sel-reinit]

**Giải thích.**

*Với `gateway`.* Vòng lặp của `gateway` dùng `select` sẽ như sau: dựng tập gồm mọi socket cần theo dõi, gọi `select`, duyệt tập xem socket nào ready, đọc ở những socket đó, rồi lặp lại. Hai chỗ vướng xuất hiện ngay khi số kết nối tăng: tập này có kích thước cố định, và sau mỗi lần gọi nó không còn là tập bạn đã đưa vào. Mục này giải thích cả hai.

### 2.1 Giao diện

`select` nhận ba `fd_set`, mỗi tập dành cho một loại sự kiện: đọc, ghi và "exceptional". Ngoài ba tập còn có `nfds` và `timeout`.[^sel-sets] Một fd được coi là ready khi có thể làm I/O tương ứng mà không chặn, ví dụ `read`, hoặc một `write` đủ nhỏ.[^sel-ready] Tham số `nfds` phải bằng số fd lớn nhất trong ba tập, cộng 1.[^sel-nfds] Bạn thao tác trên tập bằng bốn macro `FD_ZERO`, `FD_SET`, `FD_CLR` và `FD_ISSET`.[^sel-macros]

### 2.2 Giới hạn 1024 là của glibc

Giới hạn này hay bị hiểu nhầm ở hai chỗ. Chỗ thứ nhất: nó nói về **số** của fd, không phải số lượng. `select` chỉ theo dõi được fd có số nhỏ hơn `FD_SETSIZE` (1024); man page nói giới hạn này sẽ không đổi và khuyên ứng dụng hiện đại dùng `poll` hoặc `epoll`.[^sel-warn] Vậy nếu `gateway` mới có 10 kết nối nhưng một socket của nó mang số 1500, socket đó vẫn không đưa vào `fd_set` được.

Chỗ thứ hai: giới hạn không nằm trong kernel. Kernel Linux không áp giới hạn cố định nào; chính glibc làm `fd_set` thành kiểu kích thước cố định, với `FD_SETSIZE` bằng 1024.[^sel-glibc]

Tôi đo thử trong container (glibc, aarch64): `FD_SETSIZE` = 1024 và `sizeof(fd_set)` = 128 byte, tức 1024 bit, mỗi fd một bit. Con số 128 khớp với việc `fd_set` là bitmap cỡ cố định, nhưng đó là kết quả đo, không phải điều man page hứa.

### 2.3 Value-result: phải dựng lại tập mỗi lần

Khi `select` trả về, kernel ghi đè lên chính các `fd_set` bạn truyền vào để đánh dấu fd nào đang ready. Vì vậy nếu gọi trong vòng lặp, bạn phải khởi tạo lại các tập trước mỗi lần.[^sel-reinit] Với `gateway`, nghĩa là mỗi vòng lặp lại `FD_SET` từng socket đang theo dõi (nhận xét của người soạn). Chính man page cũng coi việc dùng `fd_set` làm đối số value-result là một lỗi thiết kế (design error), và nói `poll` với `epoll` tránh được lỗi đó.[^sel-design]

Tôi thử như sau trong container: hai pipe `a` và `b` cùng nằm trong `fd_set` đọc, chỉ ghi vào `a`, rồi gọi `select` với timeout 0. Nó trả về 1; sau lời gọi, `FD_ISSET(a)` = 1 còn `FD_ISSET(b)` = 0. Bit của `b` đã bị xóa khỏi tập, nên lần sau mà không `FD_SET(b)` lại thì `b` rơi khỏi danh sách theo dõi.

<details><summary>Đi sâu: <code>select</code> báo ready giả; tên syscall thật</summary>

**Ready giả.** Trên Linux, `select` có thể báo một socket sẵn sàng đọc mà `read` ngay sau đó vẫn chặn, chẳng hạn khi dữ liệu đã đến nhưng bị bỏ vì sai checksum.[^sel-spurious] Man page khuyên đặt `O_NONBLOCK` cho socket nào không được phép chặn.[^sel-spurious-nb] ([Chương 01, mục 3](01-mo-hinh-io.md) nêu cùng điểm này từ góc độ readiness.)

**Tên syscall thật.** Lời gọi `select` trong C hiện ra dưới `strace` (aarch64) là `pselect6(6, [3 5], NULL, NULL, {tv_sec=0, tv_nsec=0}, NULL)`. Tên syscall khác tên hàm libc: man page nói syscall nằm dưới `pselect()` của glibc tên là `pselect6()`.[^sel-pselect6] Trên aarch64, đây có vẻ là đường duy nhất (suy luận của người soạn). Lý do: arm64 lấy bảng syscall từ `asm-generic/unistd.h`;[^k-arm64-unistd] bảng đó có `pselect6` với số 72;[^k-generic-pselect6] và khi grep tệp v6.8, tôi không tìm thấy dòng nào cho `select`. Hiện tượng tương tự xảy ra với `epoll_wait` (mục 4).

</details>

**Quay lại Q1:** giới hạn 1024 là `FD_SETSIZE` của glibc, áp lên *số* của fd; `fd_set` là value-result nên `gateway` phải dựng lại nó ở mỗi vòng lặp.

## 3. `poll(2)`: mảng `pollfd`, `events` và `revents` (Q2)

> **Trả lời ngắn.** `poll` nhận tập fd dưới dạng một mảng `struct pollfd`. Trong mỗi phần tử, `events` là đầu vào còn `revents` là đầu ra do kernel điền,[^poll-revents] nên giữa các lần gọi bạn không phải dựng lại tập như với `select`. fd âm bị bỏ qua (`revents` = 0),[^poll-neg] còn fd chưa mở được báo bằng `POLLNVAL` trong `revents`.[^poll-nval]

**Giải thích.**

*Với `gateway`.* Chuyển `gateway` từ `select` sang `poll`, bitmap cố định nhường chỗ cho một mảng `pollfd`, mỗi socket một phần tử. Cột "tôi muốn biết gì" của mảng đó kernel không đụng tới, còn kết quả nó ghi sang một cột riêng. Nhờ vậy vòng lặp không còn phải dựng lại tập sau mỗi lần gọi. Mục này xem giao diện đó cụ thể ra sao và hai trường hợp biên: fd âm và fd chưa mở.

### 3.1 Giao diện

`poll` làm việc giống `select`: chờ tới khi một trong một tập fd sẵn sàng I/O.[^poll-desc] Khác ở chỗ tập fd là một **mảng** `struct pollfd`, truyền qua đối số `fds`:[^poll-fds]

```c
struct pollfd {
    int   fd;         /* file descriptor */
    short events;     /* requested events */
    short revents;    /* returned events */
};
```

`events` là tham số vào: mặt nạ bit những sự kiện bạn quan tâm. `revents` là tham số ra: kernel điền những sự kiện thực sự xảy ra.[^poll-revents] Vì đầu vào và đầu ra nằm ở hai trường khác nhau, bạn **không cần dựng lại `events`** giữa các lần gọi. Đó đúng là điểm man page `select(2)` nói `poll` tránh được lỗi value-result.[^sel-design] Trên máy thử, `sizeof(struct pollfd)` là 8 byte (quan sát một lần chạy).

### 3.2 Hai trường hợp biên

**fd âm bị bỏ qua.** Với phần tử có `fd` âm, kernel bỏ qua trường `events` và trả `revents` bằng 0; man page nêu đây là cách dễ để "tắt" một phần tử trong một lần gọi.[^poll-neg] Tôi thử với phần tử `fd = ~5` và thấy `revents = 0`.

**`POLLNVAL` cho fd chưa mở.** Bit `POLLNVAL` ("invalid request: fd not open") chỉ xuất hiện trong `revents`.[^poll-nval] Tôi thử `poll` trên fd 999 chưa mở và nhận `revents = 0x20`, đúng giá trị của `POLLNVAL`.

<details><summary>Đi sâu: <code>POLLHUP</code>, <code>POLLERR</code> không cần đăng ký</summary>

Ba bit `POLLERR`, `POLLHUP`, `POLLNVAL` vô nghĩa trong `events`, và được đặt vào `revents` bất cứ khi nào điều kiện tương ứng đúng.[^poll-always] Với pipe hoặc stream socket, `POLLHUP` chỉ nghĩa là peer đã đóng đầu của nó. `read` chỉ trả 0 (EOF) *sau khi* dữ liệu còn lại đã được đọc hết.[^poll-hup]

</details>

**Quay lại Q2:** `poll` tách `events` (vào) khỏi `revents` (ra) nên `gateway` không phải dựng lại tập; fd âm bị bỏ qua, fd chưa mở báo `POLLNVAL`.

## 4. epoll: interest list, ready list và vì sao không phải quét (Q3)

> **Trả lời ngắn.** Một epoll instance trong kernel chứa hai danh sách: *interest list* là các fd bạn đã đăng ký, *ready list* là các fd đang sẵn sàng.[^epoll-lists] Khi một fd có I/O, một callback cài sẵn trên wait queue của nó đưa fd vào ready list (kernel 6.8).[^k-ep-ready] `epoll_wait` chỉ việc lấy các phần tử từ ready list ra.[^ep-wait-fetch] Vì thế epoll không phải quét lại toàn bộ tập đã đăng ký ở mỗi lần gọi như `select` và `poll`, mà chỉ xét các fd đã nằm trong ready list (suy luận của người soạn từ source).

**Giải thích.**

*Với `gateway`.* Với 10.000 kết nối mà mỗi lượt chỉ vài chục cái có dữ liệu, `select` và `poll` bắt `gateway` đưa lại cả 10.000 fd vào từng lần gọi. epoll đảo ngược cách làm: `gateway` đăng ký mỗi socket một lần, kernel nhớ danh sách đó, và ở mỗi lần chờ `gateway` chỉ nhận về những socket đã sẵn sàng. Câu hỏi của mục này là kernel làm được điều đó bằng cấu trúc nào.

### 4.1 Vì sao `select` và `poll` duyệt cả tập mỗi lần

Trước hết, lưu ý các man page `select(2)`, `poll(2)` và `epoll(7)` **không** nêu độ phức tạp tiệm cận, nên chương này không ghi Big-O cho chúng. Điều có nguồn chỉ là: `select` kiểm tra các fd trong ba tập tới giới hạn `nfds`;[^sel-nfds2] `poll` nhận tập fd làm đối số `fds` ở mỗi lần gọi;[^poll-fds] còn epoll chỉ được man page mô tả là mở rộng tốt với số lượng lớn fd được theo dõi.[^epoll-intro]

Để thấy cấu trúc cụ thể, ta đọc source Linux `v6.8`, `fs/select.c`. Đây là **chi tiết cài đặt của một phiên bản**, không phải bảo đảm giao diện.

- `do_poll` có vòng ngoài `for (;;)`; bên trong, nó duyệt từng phần tử `pollfd` và gọi `do_pollfd` cho mỗi phần tử. Comment trong code mô tả việc kernel dò sự kiện ở từng fd.[^k-poll-fish] Vòng duyệt từng entry nằm ở dòng `for (; pfd != pfd_end; pfd++) {`.[^k-poll-loop]
- `do_select` cũng có `for (;;)`; nó duyệt các bit của bitmap và gọi `vfs_poll` (hỏi file "bây giờ có sự kiện không?") cho mỗi fd có bit đặt.[^k-select-loop]

Đọc cấu trúc đó, ta thấy ở **mỗi lần gọi** (và mỗi lần vòng ngoài chạy lại sau khi bị đánh thức) kernel đi qua từng fd được truyền vào và hỏi từng file. Chi phí một lần gọi vì thế gắn với *kích thước tập truyền vào*, chứ không chỉ với số fd ready (suy luận của người soạn từ source). Tôi không nêu hàm Big-O vì không có nguồn tài liệu nào phát biểu; muốn con số thì phải đo.

Cũng đừng hiểu `select` và `poll` là vòng lặp bận thuần túy. `do_poll` đăng ký waiter vào wait queue của các file ở lượt quét đầu, và comment nói các lượt sau không đăng ký lại.[^k-poll-waiters] Khác biệt nằm ở chỗ tập fd là đối số của *từng lần gọi*, nên việc theo dõi này gắn với một lần gọi. epoll thì giữ interest list trong kernel qua nhiều lần `epoll_wait` (suy luận của người soạn, dựa trên cấu trúc code ở trên và mô tả interest list trong `epoll(7)`).

### 4.2 Hai danh sách của epoll instance

epoll làm việc giống `poll`: theo dõi nhiều fd xem I/O có khả thi trên fd nào không. Nó dùng được ở cả kiểu edge-triggered lẫn level-triggered, và mở rộng tốt với số lượng lớn fd.[^epoll-intro] Khái niệm trung tâm là **epoll instance**, một cấu trúc dữ liệu trong kernel mà từ phía user space trông như một container chứa hai danh sách:[^epoll-lists]

- **interest list** (còn gọi là epoll set): các fd mà process đã đăng ký quan tâm. Với `gateway`, đó là 10.000 socket.
- **ready list**: các fd đang "ready" cho I/O. Chính xác hơn, nó là tập *tham chiếu* tới một phần các fd trong interest list, và kernel *điền động* theo hoạt động I/O trên các fd đó.[^epoll-lists]

```mermaid
flowchart LR
  subgraph user[User space]
    L[event loop của gateway]
  end
  subgraph kernel[Kernel: epoll instance]
    I[interest list<br/>fd đã đăng ký]
    R[ready list<br/>fd sẵn sàng]
  end
  L -- "epoll_ctl(ADD/MOD/DEL)" --> I
  D["driver / socket / pipe<br/>có I/O"] -- "callback khi có sự kiện" --> R
  I -. "tham chiếu" .-> R
  L -- "epoll_wait: ngủ nếu R rỗng<br/>(đến khi có sự kiện, tín hiệu hoặc hết timeout),<br/>lấy tối đa maxevents phần tử" --> R
```

### 4.3 Ba system call

`gateway` làm việc với epoll qua ba syscall:

| Syscall | Việc |
|---|---|
| `epoll_create1(flags)` | Tạo instance và trả về một fd tham chiếu tới nó; cờ `EPOLL_CLOEXEC` đặt close-on-exec cho fd đó.[^ep-create1] |
| `epoll_ctl(epfd, op, fd, &event)` | `EPOLL_CTL_ADD`, `MOD`, `DEL` lần lượt thêm, sửa, gỡ entry trong interest list.[^ep-ctl] |
| `epoll_wait(epfd, events, maxevents, timeout)` | Chờ sự kiện và trả về tối đa `maxevents` sự kiện.[^ep-wait] |

Về `timeout` của `epoll_wait`: -1 nghĩa là chặn vô hạn, 0 nghĩa là trả ngay kể cả khi chưa có sự kiện nào.[^ep-wait-timeout] `epoll(7)` mô tả lời gọi này là "lấy các phần tử từ ready list".[^ep-wait-fetch] Còn trường `data` bạn đặt trong `epoll_ctl` sẽ được trả lại nguyên vẹn ở `epoll_wait`;[^ep-wait-data] vì vậy đó là chỗ nhét số fd hoặc token để biết sự kiện thuộc về kết nối nào.

### 4.4 Ai đưa fd vào ready list

Phần này đọc từ source kernel 6.8 (chi tiết cài đặt, có thể đổi). Một comment minh họa epoll lồng nhau trong `fs/eventpoll.c` mô tả đường đi như sau: khi gói tin đến thiết bị bên dưới một socket, code mạng gọi `wake_up()` trên *poll wake list* của file, và epoll đã cài sẵn một *callback entry* trên hàng đợi đó.[^k-ep-callback] Hàm `ep_poll_callback` là hàm đánh thức của entry này, gán trong `ep_ptable_queue_proc` bằng dòng `init_waitqueue_func_entry(&pwq->wait, ep_poll_callback)`.[^k-ep-func-entry] Ở trường hợp thông thường, hàm đó thêm entry vào ready list.[^k-ep-ready]

Ghép các bước lại, một kết nối của `gateway` đi như sau:

```mermaid
sequenceDiagram
  participant App as App (epoll_wait)
  participant EP as epoll instance
  participant F as file/socket
  App->>EP: epoll_ctl(ADD, fd)
  EP->>F: cài callback vào wait queue của fd
  App->>EP: epoll_wait() — ready list rỗng, thread ngủ
  Note over F: dữ liệu đến
  F->>EP: wake_up → ep_poll_callback
  EP->>EP: đưa entry của fd vào ready list, đánh thức waiter
  EP-->>App: epoll_wait trả sự kiện lấy từ ready list
```

So với phần quét ở §4.1, điểm khác là giữa hai lần `epoll_wait`, kernel đã tích lũy sẵn các fd ready nhờ callback. Đó là suy luận của người soạn từ cấu trúc code; man page không phát biểu như vậy, nên mục này không kèm khẳng định nào về tốc độ.

<details><summary>Đi sâu: quan sát strace một server echo dùng epoll thô</summary>

Trong container (aarch64), một server echo dùng epoll thô (`labs/crates/lab-io-async`, `epoll_echo`) chạy dưới `strace` cho vòng đời sau (rút gọn):

```text
epoll_create1(EPOLL_CLOEXEC)      = 4
epoll_ctl(4, EPOLL_CTL_ADD, 3, {events=EPOLLIN, data={u32=3, u64=3}}) = 0
epoll_pwait(4, [{events=EPOLLIN, data={u32=3, u64=3}}], 64, -1, NULL, 8) = 1
accept4(3, {...}, [128 => 16], SOCK_CLOEXEC) = 5
epoll_ctl(4, EPOLL_CTL_ADD, 5, {events=EPOLLIN|EPOLLOUT|EPOLLRDHUP|EPOLLET, data={u32=5, u64=5}}) = 0
accept4(3, ..., SOCK_CLOEXEC) = -1 EAGAIN (Resource temporarily unavailable)
epoll_pwait(4, [{events=EPOLLIN|EPOLLOUT, data={u32=5, u64=5}}], 64, -1, NULL, 8) = 1
read(5, ..., 4096) = 64
read(5, ..., 4096) = 64
read(5, "", 4096)                 = 0
epoll_ctl(4, EPOLL_CTL_DEL, 5, 0xffffc4cdce90) = 0
```

Đây là quan sát một lần chạy. Đọc từng bước:

- `epoll_create1(EPOLL_CLOEXEC) = 4`: tạo epoll instance và trả fd 4 tham chiếu tới nó, có cờ close-on-exec.[^ep-create1] Từ `accept4(3, …)` bên dưới ta suy ra fd 3 là socket lắng nghe (các lời gọi tạo nó đã bị rút gọn). Bản `epoll_create(size)` cũ bỏ qua `size` từ Linux 2.6.8, nhưng `size` vẫn phải lớn hơn 0.[^ep-create]
- `epoll_ctl(4, EPOLL_CTL_ADD, 3, {events=EPOLLIN, …})`: đăng ký listener chỉ với `EPOLLIN`, không có `EPOLLET`, nên là level-triggered (mặc định).[^ep-et] Code nhét chính số fd vào `data`, và `epoll_wait` trả lại đúng giá trị đó để biết sự kiện thuộc fd nào.[^ep-wait-data]
- `epoll_pwait(4, [...], 64, -1, NULL, 8) = 1`: `64` là `maxevents`; `-1` là timeout vô hạn nên thread ngủ ở đây tới khi có sự kiện;[^ep-wait-timeout] `= 1` là số fd ready (listener có kết nối đến).
- Tên `epoll_pwait` cùng hai đối số cuối `NULL, 8`: code gọi `epoll_wait` của libc nhưng strace hiện `epoll_pwait`. Man page nói khi `sigmask` là `NULL` thì `epoll_pwait()` tương đương `epoll_wait()`.[^ep-pwait] Syscall thô có đối số thứ sáu `sigsetsize`, là kích thước tính bằng byte của `sigmask`;[^ep-pwait-size] số `8` ở cuối ứng với đối số này (suy luận của người soạn). Trên aarch64, `epoll_pwait` có vẻ là đường duy nhất (suy luận của người soạn): arm64 lấy bảng syscall từ `asm-generic/unistd.h`,[^k-arm64-unistd] bảng đó có `epoll_pwait` với số 22,[^k-generic-pwait] và khi grep tệp v6.8, tôi không tìm thấy dòng nào cho `epoll_wait`.
- `accept4(3, …) = 5` rồi `epoll_ctl(4, EPOLL_CTL_ADD, 5, {EPOLLIN|EPOLLOUT|EPOLLRDHUP|EPOLLET})`: kết nối mới (fd 5) được đăng ký edge-triggered, một lần cho cả đọc lẫn ghi. Man page nêu cách này để khỏi `EPOLL_CTL_MOD` mỗi lần chuyển giữa đọc và ghi.[^ep-both] `EPOLLRDHUP` để biết peer đóng hoặc shutdown.[^ep-rdhup]
- `accept4(3, …) = -1 EAGAIN`: vòng accept chạy tới khi hết kết nối chờ rồi mới quay lại chờ. Listener là LT nên không bắt buộc phải làm vậy; đây là lựa chọn của code (nhận xét của người soạn).
- `epoll_pwait(…) = 1` với `EPOLLIN|EPOLLOUT` trên fd 5: kết nối vừa đọc được vừa ghi được. Sau đó là hai `read` 64 byte rồi `read = 0`. 0 byte là EOF, và `read` chỉ trả 0 sau khi dữ liệu còn lại đã được đọc hết.[^poll-hup] Vòng đọc kết thúc ở EOF nên trích đoạn này không có `read = -1 EAGAIN`.
- `epoll_ctl(4, EPOLL_CTL_DEL, 5, 0xffffc4cdce90) = 0`: gỡ fd 5 khỏi interest list. Với `DEL`, đối số `event` bị bỏ qua và có thể là `NULL`.[^ep-ctl-del] Việc strace in một địa chỉ thay vì `NULL` nghĩa là code truyền con trỏ không-NULL mà kernel không dùng (suy luận của người soạn). Gỡ tường minh tránh được bẫy ở mục 6 nếu fd có bản sao (nhận xét của người soạn).

</details>

**Quay lại Q3:** epoll giữ interest list (fd đã đăng ký) và ready list (fd sẵn sàng); callback trên wait queue của file điền ready list, `epoll_wait` lấy ra, nên `gateway` chỉ nhận về những socket đã có dữ liệu.

## 5. Level-triggered và edge-triggered (Q4)

> **Trả lời ngắn.** Mặc định epoll là level-triggered (LT): còn dữ liệu thì còn báo, cùng ngữ nghĩa với `poll`.[^ep-lt] Cờ `EPOLLET` yêu cầu edge-triggered (ET).[^ep-et] Chế độ này chỉ báo khi có thay đổi trên fd.[^ep-et-change] Vì thế ở ET, ví dụ 2 KB của `epoll(7)` (đọc 1 KB rồi chờ lại) nhiều khả năng treo dù buffer còn dữ liệu.[^ep-et-scenario] Cách dùng ET đúng là fd non-blocking, và chỉ chờ sau khi `read` hoặc `write` trả `EAGAIN`.[^ep-et-rule]

**Giải thích.**

*Với `gateway`.* Giả sử một client gửi cho `gateway` một frame 2 KB, còn handler đọc mỗi lần vào buffer 1 KB (nhận xét của người soạn: tình huống giả định, cùng khuôn với ví dụ trong `epoll(7)`). Đọc xong 1 KB đầu rồi quay lại chờ, thread có được báo về 1 KB còn lại không? Tùy chế độ: ở LT thì có, ở ET nhiều khả năng không. Mục này giải thích vì sao, và từ đó rút ra quy tắc mà mọi code dùng ET phải theo.

### 5.1 Hai chế độ

Mặc định epoll là **level-triggered**; cờ `EPOLLET` chuyển sang **edge-triggered**.[^ep-et] Ở LT, epoll đơn giản là một `poll(2)` nhanh hơn, dùng được ở mọi chỗ `poll` dùng được vì cùng ngữ nghĩa.[^ep-lt] Ở ET, epoll chỉ phát sự kiện khi có thay đổi trên fd được theo dõi.[^ep-et-change]

### 5.2 Ví dụ 2 KB trong `epoll(7)`

Kịch bản trong man page gồm năm bước: (1) đầu đọc của pipe, `rfd`, được đăng ký vào epoll; (2) writer ghi 2 KB; (3) `epoll_wait` trả `rfd` ready; (4) reader đọc 1 KB; (5) gọi `epoll_wait` lần nữa. Nếu `rfd` đăng ký với `EPOLLET`, lần gọi ở bước 5 nhiều khả năng sẽ treo dù dữ liệu vẫn còn trong buffer.[^ep-et-scenario] Lý do: sự kiện sinh ra từ lần ghi và đã bị tiêu thụ ở bước 3; lần đọc ở bước 4 không rút cạn buffer, nhưng cũng không tạo thay đổi mới, nên `epoll_wait` ở bước 5 có thể chặn vô thời hạn.[^ep-et-hang]

```mermaid
sequenceDiagram
  participant W as Writer
  participant K as Kernel (epoll, ET)
  participant R as Reader
  W->>K: write 2 KB vào pipe (edge!)
  R->>K: epoll_wait
  K-->>R: rfd ready (sự kiện được tiêu thụ)
  R->>K: read 1 KB (còn 1 KB trong buffer)
  R->>K: epoll_wait
  Note over K,R: không có thay đổi mới → không có sự kiện → treo
```

### 5.3 Quy tắc dùng ET đúng

Cách dùng ET được man page đề nghị gồm hai vế: (1) dùng fd non-blocking, và (2) chỉ chờ sự kiện *sau khi* `read` hoặc `write` trả `EAGAIN`.[^ep-et-rule] Vế đầu nhằm tránh chuyện một `read` hoặc `write` bị chặn làm đói các fd khác mà thread đang phục vụ.[^ep-et-nonblock]

Vì sao phải đi "đến `EAGAIN`"? Vì khi nhận được sự kiện, bạn phải coi fd là ready cho tới khi lần `read` hoặc `write` non-blocking kế tiếp trả `EAGAIN`.[^ep-qa-eagain] Với file hướng gói hay token (datagram socket, terminal ở canonical mode), cách *duy nhất* để biết đã hết là đọc hoặc ghi tới `EAGAIN`.[^ep-qa-packet] Với file hướng stream (pipe, FIFO, stream socket), `read` trả ít byte hơn mức yêu cầu thì có thể chắc là đã rút cạn.[^ep-qa-short] Nhưng man page dặn tránh đường tắt này nếu bạn không chắc fd luôn thuộc loại stream.[^ep-qa-short-caveat] Thêm một lý do nữa: nhiều sự kiện xảy ra giữa hai lần `epoll_wait` sẽ được gộp lại thành một báo cáo,[^ep-combine] nên một lần đánh thức có thể ứng với nhiều đợt dữ liệu (suy luận của người soạn).

### 5.4 Cái giá, phần được lợi và starvation

Cái giá của ET là ít tha thứ lỗi lập trình: bỏ sót một sự kiện thì kết nối đó kẹt vĩnh viễn.[^c10k-edge] Phần được lợi (nhận xét của người soạn) là ít sự kiện lặp, và `gateway` có thể đăng ký một lần `EPOLLIN|EPOLLOUT` để khỏi `epoll_ctl(MOD)` mỗi khi đổi giữa đọc và ghi. Man page nêu cách này vì lý do hiệu năng.[^ep-both]

ET còn một bẫy nữa là starvation. Nếu một fd có lượng I/O rất lớn và bạn cố rút cạn nó trong một lần, các fd khác có thể bị bỏ đói.[^ep-starve] Man page đề xuất giữ một ready list ở user space và round-robin giữa các fd ready.[^ep-starve-fix] Với `gateway`, một client đẩy dữ liệu liên tục là loại fd như vậy (nhận xét của người soạn).

<details><summary>Đi sâu: cơ chế LT/ET trong kernel 6.8, thí nghiệm "đọc một lần" và tái hiện ví dụ 2 KB (quan sát một lần chạy)</summary>

**Cơ chế trong source (kernel 6.8, chi tiết cài đặt).** Khi `epoll_wait` lấy sự kiện, `ep_send_events` duyệt các entry của ready list và gọi lại `ep_item_poll` để lấy mặt nạ sự kiện *hiện tại* của từng entry. Nó chỉ giao entry cho user space nếu mặt nạ đó giao với mặt nạ người gọi yêu cầu.[^k-ep-send-mask] Entry vừa được giao bị gỡ khỏi ready list; chỉ entry *không* đặt `EPOLLET` (và không `EPOLLONESHOT`) mới được xếp lại vào cuối ready list để lần `epoll_wait` kế tiếp kiểm tra lại.[^k-ep-lt-requeue] Từ đó suy ra khác biệt LT/ET ở mức code (suy luận của người soạn): fd LT còn nằm trong ready list nên lần sau được hỏi lại và thấy còn dữ liệu; fd ET chỉ quay lại ready list khi callback ở mục 4 chạy lần nữa, nên buffer còn dữ liệu mà không có đánh thức mới thì `epoll_wait` không thấy gì. Callback ấy còn lọc theo mặt nạ: đánh thức có mặt nạ không giao với `events` đã đăng ký thì bị bỏ qua.[^k-ep-mask-filter]

**Bug "đọc một lần" (thí nghiệm của lab).** Server echo `epoll_echo` có cờ `--read-once` cố tình vi phạm quy tắc ET: mỗi sự kiện chỉ `read` đúng 16 byte rồi quay lại `epoll_wait`. Chạy lại trong container với một kết nối gửi một message 64 byte:

```text
[epoll] listening on 127.0.0.1:7107 (epfd=4, listener fd=3, mode=ET, read once (broken))
[epoll] accepted 127.0.0.1:60960 as fd 5
[load] 1 conns x 1 msgs x 64 B: 0 ok, 1 failed, 3022 ms
[load] conn 0: message 0: Resource temporarily unavailable (os error 11)
[epoll] served 1 conns: epoll_wait calls=4, events=4, reads with data=3, reads -> EAGAIN=0, bytes echoed=48, threads=1
```

(Dòng `[epoll] fd 5: Broken pipe` bị trộn vào output vì hai tiến trình cùng ghi stderr.) Server echo được 48 byte (3 lần đọc × 16) rồi ngủ trong `epoll_wait`; 16 byte còn lại nằm trong buffer nhưng không có edge mới. Client có read timeout khoảng 3 s nên báo `WouldBlock` (`os error 11` = `EAGAIN`). Không có lỗi mạng nào ở đây, chỉ là một phản hồi không bao giờ tới.

Cách đọc số liệu: `reads -> EAGAIN=0` cho thấy server không bao giờ chạy tới `EAGAIN`, đúng điều quy tắc ET cấm; `bytes echoed=48` khớp 3 × 16. Đừng đọc `events=4` như "bốn edge do dữ liệu": theo ghi chú của lab (`docs/labs/07-io-async-lab.md`), server còn được đánh thức thêm một lần sau khi client timeout và đóng kết nối, và tôi không tách được từng sự kiện thuộc thời điểm nào. Đối chứng ở chế độ đọc tới `EAGAIN` (số liệu có sẵn trong lab, tải khác: 4 kết nối × 200 message × 64 B): `bytes echoed=51200` và `reads -> EAGAIN=798`.

**Tái hiện thật.** Người soạn dựng đúng kịch bản bằng C (pipe non-blocking, `epoll_wait` timeout 200 ms để "treo" hiện ra thành `0`), chạy trong container:

| Chế độ | wait #1 | read 1024 | wait #2 | read 1024 nữa | wait #3 |
|---|---|---|---|---|---|
| LT | 1 | 1024 | **1** | 1024 | 0 |
| ET (`EPOLLET`) | 1 | 1024 | **0** (treo) | 1024 | 0 |

(Sau khi đọc nốt, `read` tiếp theo trả `-1 EAGAIN` ở cả hai chế độ.) Cách đọc bảng: `wait #1 = 1` ở cả hai chế độ vì lần ghi đầu tiên là thay đổi mới. Sau `read 1024` còn 1024 byte trong buffer, và ô đậm là chỗ hai chế độ tách ra. LT báo lại vì fd còn dữ liệu (khớp cơ chế xếp lại ready list ở trên, suy luận của người soạn). ET không có đánh thức mới nên `epoll_wait` hết timeout 200 ms và trả `0`. `wait #3 = 0` ở cả hai chế độ vì buffer đã rỗng, không còn gì để báo, kể cả với LT; đây là đối chứng cho thấy LT im lặng đúng khi hết dữ liệu chứ không phải vì timeout quá ngắn. Strace xác nhận chuỗi `epoll_pwait(..., 200, ...) = 1, 1, 0` cho LT và `= 1, 0` cho ET. `0` chỉ nghĩa là hết timeout 200 ms không có sự kiện, trong một lần chạy; man page chỉ nói nhiều khả năng sẽ treo.

</details>

**Quay lại Q4:** ET chỉ báo khi có thay đổi, nên `gateway` phải dùng fd non-blocking và đọc, ghi tới `EAGAIN`; bỏ sót thì fd kẹt như ở ví dụ 2 KB.

## 6. Đóng fd, event cache và file thường (Q5)

> **Trả lời ngắn.** Interest list nhận diện fd bằng cả số fd lẫn open file description (OFD).[^ep-key] Vì vậy `close(fd)` chưa chắc gỡ fd khỏi epoll: nó chỉ bị gỡ khi mọi fd cùng trỏ vào OFD đó đã đóng.[^ep-close] Còn `epoll_ctl` trả `EPERM` khi file đích không hỗ trợ epoll, ví dụ file thường.[^ep-eperm]

**Giải thích.**

*Với `gateway`.* Giả sử `gateway` đóng một kết nối, nhưng ở đâu đó trong process còn một bản `dup` của socket đó, hoặc một process con vừa `fork` đã thừa kế nó (`fork` nhân bản các fd đang mở, xem P1 · Chương 04). Lúc này `gateway` tưởng đã dọn xong, nhưng epoll vẫn có thể báo sự kiện cho kết nối ấy. Mục này giải thích vì sao, rồi nói thêm hai cái bẫy liên quan: cái event cache, và cái fd mà epoll không nhận.

### 6.1 Đóng fd không chắc gỡ nó khỏi interest list

Interest list phân biệt các entry bằng khóa gồm *số fd* **và** *open file description*.[^ep-key] Khi bạn `EPOLL_CTL_ADD`, entry giữ một tham chiếu tới OFD tương ứng.[^ep-ctl-ofd] Từ đó có hệ quả mà phần Q&A của `epoll(7)` nêu rõ: fd chỉ bị gỡ sau khi *mọi* fd cùng tham chiếu OFD đó đã đóng. Nên kể cả sau khi bạn `close` fd đã đăng ký, sự kiện vẫn có thể được báo nếu còn fd khác (do `dup`, `fork`…) trỏ tới OFD đó.[^ep-close]

Cách phòng là `EPOLL_CTL_DEL` tường minh *trước khi* nhân bản fd, hoặc bảo đảm mọi bản sao đều đóng. Việc sau khó làm khi một thư viện tự `dup` hay `fork` sau lưng bạn (nhận xét của người soạn). Về OFD và `dup`, xem [P1 · Chương 04, mục 1–3](../01-linux/04-file-descriptor-va-vfs.md).

### 6.2 Event cache: sự kiện thứ 47 đóng fd của sự kiện thứ 13

Bẫy thứ hai đến từ việc giữ nguyên mảng sự kiện mà một lần `epoll_wait` trả về. Mảng đó là một *event cache*, nơi lưu các sự kiện hoặc fd để xử lý dần. Giả sử `gateway` nhận 100 sự kiện trong một lần `epoll_wait`; khi xử lý sự kiện #47, nó phát hiện phải đóng kết nối của sự kiện #13. Nếu cứ `close(2)` fd đó, mảng vẫn còn sự kiện #13 chờ xử lý cho một fd đã đóng.[^ep-cache] Man page đề xuất: ở #47, gọi `EPOLL_CTL_DEL` cho fd 13 rồi `close(2)`, và đánh dấu cấu trúc dữ liệu của nó là `removed` để nhận ra khi gặp sự kiện #13 trong cùng lô.[^ep-cache-fix]

### 6.3 File thường không dùng được với epoll

`epoll_ctl` trả `EPERM` khi file đích không hỗ trợ epoll, ví dụ `fd` là file thường hoặc thư mục.[^ep-eperm] Trong source 6.8, `do_epoll_ctl` kiểm tra file đích có hỗ trợ poll không, và trả `-EPERM` nếu không.[^k-ep-eperm] Hệ quả với `gateway` (nhận xét của người soạn): muốn đọc một file trên đĩa, nó không thể nhờ readiness để tránh bị chặn; phải dùng thread pool hoặc cơ chế completion, xem [Chương 03](03-kqueue-io-uring.md).

<details><summary>Đi sâu: quan sát bẫy <code>dup</code>, <code>EEXIST</code>, <code>EPOLL_CTL_MOD</code> và <code>EPERM</code></summary>

**Bẫy `dup`.** Tôi thử trong container: đăng ký đầu đọc pipe `q0`, `dup` ra `d`, `close(q0)`, rồi ghi vào pipe. `epoll_wait` trả **1 sự kiện** với `data.fd` bằng số của `q0`, tức số fd *đã đóng*. Đối chứng: đăng ký, ghi, `close` mà không `dup` thì `epoll_wait` trả 0. Đây là bẫy kinh điển (nhận xét của người soạn): ứng dụng nhận sự kiện cho một số fd mà nó tưởng đã dọn xong.

**Ba điều liên quan trong Q&A của `epoll(7)`.**

- Hai epoll instance cùng chờ một fd thì sự kiện được báo cho cả hai.[^ep-two-instances]
- Đăng ký *cùng* fd hai lần trên một instance nhiều khả năng nhận `EEXIST` (quan sát: `errno=File exists`), nhưng thêm một bản `dup` thì được.[^ep-eexist]
- `EPOLL_CTL_MOD` trên fd đã có sẽ đọc lại I/O khả dụng.[^ep-mod]

**`EPERM`.** Tôi thử `EPOLL_CTL_ADD` cho `/etc/hostname` mở bằng `open` và nhận `-1 EPERM`. Trong khi đó `poll` trên cùng fd trả ngay `revents = 0x1` (`POLLIN`). Đây là quan sát trên một file, không phải bảo đảm cho mọi filesystem.

</details>

**Quay lại Q5:** epoll giữ tham chiếu tới OFD nên `close` chưa chắc gỡ fd, event cache có thể giữ sự kiện của fd đã đóng, và file thường bị `EPERM`.

## 7. `EPOLLONESHOT`, `EPOLLEXCLUSIVE` và các cờ trạng thái (Q6)

> **Trả lời ngắn.** `EPOLLONESHOT` vô hiệu fd trong interest list sau khi báo một sự kiện.[^ep-oneshot] Muốn nhận sự kiện tiếp, bạn phải `EPOLL_CTL_MOD` để bật lại.[^ep-oneshot-rearm] `EPOLLEXCLUSIVE` có thể giúp tránh thundering herd (nhiều bên cùng bị đánh thức) trong một số tình huống, nhưng chỉ hứa "một hoặc nhiều" epoll fd nhận event, không hứa đúng một.[^ep-excl-one]

**Giải thích.**

*Với `gateway`.* Một thread có thể không đủ khi `gateway` phải parse và xử lý nhiều frame. Giả sử ta chạy vài worker thread, mỗi thread gọi `epoll_wait` trên cùng một epoll fd, hoặc mỗi worker có epoll fd riêng cùng theo dõi một socket (nhận xét của người soạn: hai cách tổ chức giả định). Hai cờ trong mục này sinh ra để giữ cho các worker khỏi giẫm chân nhau; ngoài ra mục còn nói tới các cờ báo trạng thái kết nối và giới hạn bộ nhớ.

### 7.1 `EPOLLONESHOT`

Với `EPOLLONESHOT`, sau khi `epoll_wait` báo một sự kiện cho fd, fd bị **vô hiệu hóa trong interest list** và epoll không báo thêm sự kiện nào cho nó.[^ep-oneshot] Muốn nhận sự kiện tiếp, bạn phải gọi `epoll_ctl(EPOLL_CTL_MOD)` với mặt nạ mới để *re-arm* (bật lại).[^ep-oneshot-rearm]

Lý do cờ này tồn tại, theo `epoll(7)`: ngay cả ở ET, việc nhận nhiều "chunk" dữ liệu có thể sinh ra nhiều sự kiện, nên caller có thể đặt `EPOLLONESHOT` để epoll vô hiệu fd sau khi nhận một sự kiện, và việc bật lại là trách nhiệm của caller.[^ep-oneshot-et]

```mermaid
stateDiagram-v2
  [*] --> Armed: ADD với EPOLLONESHOT
  Armed --> Disabled: epoll_wait báo 1 sự kiện
  Disabled --> Armed: epoll_ctl MOD (re-arm)
  Disabled --> Disabled: dữ liệu mới đến — không có sự kiện
```

### 7.2 Thundering herd: hai tình huống khác nhau

Có hai tình huống dễ lẫn với nhau.

**Tình huống 1: nhiều thread hoặc process cùng chờ trên *một* epoll fd.** Nếu một fd trong interest list được đánh dấu ET và trở nên ready, thì *chỉ một* thread (hoặc process) được đánh thức khỏi `epoll_wait`. Man page gọi đây là một tối ưu hữu ích để tránh "thundering herd" ở một số kịch bản.[^ep-herd] Với `gateway`, đây là kịch bản các worker chung epoll fd.

**Tình huống 2: nhiều epoll fd (instance khác nhau) cùng gắn vào *một* fd đích.** Mặc định *tất cả* các epoll fd đều nhận sự kiện. `EPOLLEXCLUSIVE` (từ Linux 4.5), đặt khi gắn, đổi điều đó thành: **một hoặc nhiều** epoll fd sẽ nhận event.[^ep-excl-one] Vì vậy cờ này hữu ích để tránh thundering herd trong một số tình huống.[^ep-excl]

Chữ "một hoặc **nhiều**" quan trọng: tài liệu **không hứa đúng một**. Nếu cần độc quyền tuyệt đối (đúng một worker xử lý), bạn phải tự bảo đảm bằng cơ chế khác (nhận xét của người soạn). Ngoài ra `EPOLLEXCLUSIVE` có mấy quy tắc riêng:

- Chỉ dùng được trong `EPOLL_CTL_ADD`; gắn nó vào `EPOLL_CTL_MOD` là lỗi.[^ep-excl-add]
- Cụ thể, `EPOLL_CTL_MOD` mà `events` có `EPOLLEXCLUSIVE` thì trả `EINVAL`.[^ep-excl-einval]
- Nếu cùng một fd nằm trong nhiều instance mà có instance không đặt cờ, sự kiện đến *tất cả* instance không đặt cờ, và ít nhất một instance có đặt cờ.[^ep-excl-mixed]
- Đi kèm cờ này mà đặt các cờ ngoài một tập nhỏ (`EPOLLIN`, `EPOLLOUT`, `EPOLLWAKEUP`, `EPOLLET`, cùng `EPOLLHUP` và `EPOLLERR`) thì gây `EINVAL`.[^ep-excl-flags]

Tôi không có thí nghiệm nào cho `EPOLLEXCLUSIVE`; phần này chỉ dựa trên man page.

### 7.3 `EPOLLRDHUP`, `EPOLLHUP`, `EPOLLERR`

Ba cờ này báo trạng thái của kết nối, và `gateway` dùng chúng để biết client đã rời.

- `EPOLLRDHUP` (từ Linux 2.6.17) báo peer của stream socket đã đóng kết nối, hoặc đóng nửa ghi. Man page ghi cờ này đặc biệt hữu ích để viết code đơn giản phát hiện peer shutdown khi dùng ET.[^ep-rdhup] Tôi thử trong container với `socketpair`: đăng ký `EPOLLIN|EPOLLRDHUP`, phía kia `shutdown(SHUT_WR)`, thì `epoll_wait` trả `events = 0x2001`, tức `EPOLLIN | EPOLLRDHUP` (không có `EPOLLHUP`).
- `EPOLLHUP` luôn được `epoll_wait` báo, không cần đặt trong `events`.[^ep-hup-always] Với pipe hoặc stream socket, nó chỉ nghĩa là peer đã đóng đầu của nó; các `read` tiếp theo chỉ trả 0 sau khi dữ liệu còn lại đã được tiêu thụ.[^ep-hup]
- `EPOLLERR` cũng luôn được báo mà không cần đặt trong `events`.[^ep-err-always] Nó còn được báo cho đầu ghi của pipe khi đầu đọc đã đóng.[^ep-err]

### 7.4 Giới hạn bộ nhớ

Mỗi fd đăng ký tốn khoảng 160 byte bộ nhớ kernel trên kernel 64-bit.[^ep-watches] Với 10.000 socket, `gateway` tốn cỡ 1,6 MB cho phần này (tính nhẩm của người soạn từ con số trên). Tổng số fd mà một user đăng ký bị giới hạn bởi `/proc/sys/fs/epoll/max_user_watches`; vượt giới hạn thì `epoll_ctl(ADD)` lỗi `ENOSPC`.[^ep-enospc]

<details><summary>Đi sâu: quan sát <code>EPOLLONESHOT</code> và vì sao nó hữu ích cho đa luồng (suy luận)</summary>

**Quan sát và cài đặt.** Tôi thử trong container: đăng ký fd với `EPOLLIN|EPOLLET|EPOLLONESHOT`; ghi 1 byte thì `epoll_wait` = 1; đọc xong rồi ghi thêm 1 byte thì `epoll_wait` = **0**; gọi `EPOLL_CTL_MOD` để re-arm thì `epoll_wait` = 1. Về cài đặt (kernel 6.8): sau khi giao một sự kiện cho entry có `EPOLLONESHOT`, `ep_send_events` xóa các bit sự kiện công khai khỏi mặt nạ của entry.[^k-ep-oneshot-clear] Trong `ep_poll_callback`, comment nói khi mặt nạ không còn bit nào của `poll(2)` thì fd được coi là bị vô hiệu, nhiều khả năng do bit `EPOLLONESHOT`, cho tới khi có `EPOLL_CTL_MOD` kế tiếp.[^k-ep-oneshot] Hai đoạn code ghép lại giải thích vì sao quan sát cho `0` (suy luận của người soạn).

**Mục đích đa luồng** (suy luận của người soạn). Tôi đã đọc lại `epoll(7)`, `epoll_ctl(2)` và `epoll_wait(2)` (bookworm) và không trang nào nêu thẳng mục đích đa luồng của `EPOLLONESHOT`; chỉ có lý do "nhiều chunk → nhiều sự kiện" ở trên. Nếu nhiều worker thread cùng `epoll_wait` trên một epoll fd, hai sự kiện liên tiếp của một kết nối có thể đánh thức hai thread khác nhau và cả hai cùng xử lý kết nối đó. `EPOLLONESHOT` giữ cho mỗi lúc chỉ một sự kiện "mở" cho fd, nên thread nhận nó xử lý độc quyền cho tới khi tự re-arm. **Không** có câu nào trong tài liệu nói thẳng điều này, đừng trích như tài liệu.

</details>

**Quay lại Q6:** `EPOLLONESHOT` tắt fd sau một sự kiện cho tới khi re-arm; `EPOLLEXCLUSIVE` chỉ hứa "một hoặc nhiều" bên nhận event, không hứa đúng một.

## 8. Liên hệ với Rust: mio và `EPOLLET` (Q7)

> **Trả lời ngắn.** Source mio 1.2.4 đặt `EPOLLET` trong mọi lần đăng ký fd với epoll trên Linux.[^mio-src] Tài liệu `Poll` của mio không gọi tên chế độ này và không nêu lý do. Nó chỉ đặt nghĩa vụ lặp thao tác tới khi nhận `WouldBlock` sau mỗi readiness event,[^mio-drain] khớp với quy tắc "đọc tới `EAGAIN`" của ET ở mục 5.

**Giải thích.**

*Với `gateway`.* Nếu `gateway` viết bằng Rust, bạn sẽ không gọi `epoll_ctl` trực tiếp mà đi qua mio, hoặc qua Tokio (Tokio dùng mio, xem Chương 08). Câu hỏi thực tế là: mio đã chọn sẵn chế độ nào cho các socket của bạn, và code đọc của `gateway` phải tuân theo điều gì.

### 8.1 Hợp đồng "lặp tới `WouldBlock`"

Mio dùng epoll trên Linux và kqueue trên macOS, theo bảng Implementation notes của `Poll`.[^mio-impl] Hợp đồng chung của `Poll` là: sau khi nhận một readiness event, thao tác tương ứng phải được lặp lại cho tới khi trả `WouldBlock`.[^mio-drain] Đó chính là quy tắc "đọc tới `EAGAIN`" của `epoll(7)`, chỉ khác tên lỗi theo kiểu Rust. Quên nó, `gateway` sẽ gặp lại đúng bug `--read-once` ở mục 5. Quan hệ nhân quả (ET là lý do hợp đồng này cần thiết trên Linux) là suy luận của người soạn: docs mio không nối hai điều đó với nhau.

### 8.2 Vì sao mio chọn ET

Không tài liệu hay source nào tôi đã tải nêu lý do. Điều có nguồn là backend kqueue của mio đăng ký với `EV_CLEAR`,[^mio-kq] và `EV_CLEAR` đặt lại trạng thái sự kiện sau khi người dùng lấy nó.[^kq-clear] Từ đó tôi suy luận (suy luận của người soạn) rằng mio muốn hai backend cùng một ngữ nghĩa "báo theo thay đổi", để dùng chung một hợp đồng drain. [Chương 03, mục 2](03-kqueue-io-uring.md) so sánh `EV_CLEAR` với `EPOLLET`.

Phần còn lại của mio (`Poll`, `Registry`, `Token`, `Waker`; spurious event và draining readiness với log `strace` đầy đủ) thuộc [Chương 07, mục 4–6](07-executor-va-mio.md). Tokio ở [Chương 08](08-tokio.md), còn kiến trúc event loop ở [Chương 04](04-kien-truc-event-loop.md).

<details><summary>Đi sâu: bằng chứng mio đặt <code>EPOLLET</code> (source và strace)</summary>

- Source mio `v1.2.4`, `src/sys/unix/selector/epoll.rs`, hàm `interests_to_epoll`: hàm bắt đầu bằng `let mut kind = EPOLLET;`, rồi cộng `EPOLLIN | EPOLLRDHUP` cho interest đọc và `EPOLLOUT` cho ghi.[^mio-src] Đây là chi tiết cài đặt của phiên bản 1.2.4.
- Quan sát (strace `mio_echo`; log đầy đủ ở [Chương 07, mục 6](07-executor-va-mio.md)): `epoll_ctl(3, EPOLL_CTL_ADD, 5, {events=EPOLLIN|EPOLLOUT|EPOLLRDHUP|EPOLLET, data={u32=1, u64=1}})` cho kết nối, và `EPOLLIN|EPOLLRDHUP|EPOLLET` cho listener. **Kể cả listener** đều ET; `data` mang `Token` của mio. (Server echo thô ở mục 4 chọn khác: listener giữ LT.)

</details>

**Quay lại Q7:** source mio đặt `EPOLLET` cho mọi fd (docs không gọi tên nó), nên code của `gateway` phải lặp tới `WouldBlock` sau mỗi readiness event.

## Tóm tắt

- `select` chặn ở fd có *số* ≥ 1024 vì glibc làm `fd_set` cố định, không phải kernel; `fd_set` là value-result nên phải dựng lại mỗi lần gọi.
- `poll` tách `events` (vào) khỏi `revents` (ra), bỏ qua fd âm, báo `POLLNVAL` cho fd không mở.
- Man page không nêu Big-O. Trong source 6.8, `select` và `poll` duyệt từng fd truyền vào ở mỗi lần gọi, còn epoll dùng callback đưa fd vào ready list; hệ quả hiệu năng là suy luận, hãy đo.
- epoll = interest list + ready list, qua `epoll_create1`, `epoll_ctl`, `epoll_wait`; trên aarch64 strace hiện `epoll_pwait`.
- ET chỉ báo khi có thay đổi: dùng fd non-blocking, đọc và ghi tới `EAGAIN`; chế độ `--read-once` làm echo kẹt ở 48/64 byte.
- `EPOLLONESHOT` vô hiệu fd tới khi `EPOLL_CTL_MOD`; `EPOLLEXCLUSIVE` cho "một hoặc nhiều" epoll fd nhận event.
- Đóng fd không chắc gỡ nó khỏi interest list khi OFD còn được tham chiếu; file thường bị `EPERM`.
- mio (source 1.2.4) đặt `EPOLLET` cho mọi fd; docs mio đòi lặp tới `WouldBlock`.

## Câu hỏi tự kiểm tra

1. Giới hạn 1024 của `select` nằm ở kernel hay glibc? Giới hạn "số lượng" hay "số của" fd?
<details><summary>Đáp án</summary>

Ở glibc: kernel không áp giới hạn cố định.[^sel-glibc] `select` chỉ theo dõi fd có *số* nhỏ hơn `FD_SETSIZE`,[^sel-warn] tức giới hạn theo số của fd.

</details>

2. Vì sao `select` trong vòng lặp phải khởi tạo lại `fd_set`, còn `poll` thì không?
<details><summary>Đáp án</summary>

`select` sửa tập tại chỗ để chỉ ra fd ready.[^sel-reinit] `poll` tách `events` vào và `revents` ra.[^poll-revents]

</details>

3. Interest list và ready list khác nhau thế nào; ai điền ready list?
<details><summary>Đáp án</summary>

Interest list là tập fd đã đăng ký; ready list là tập tham chiếu tới fd đang ready, được kernel điền động theo I/O.[^epoll-lists] Theo source 6.8, callback cài trên wait queue của file đưa entry vào ready list.[^k-ep-ready]

</details>

4. Trong ví dụ 2 KB của `epoll(7)`, vì sao bước 5 treo ở ET mà không treo ở LT?
<details><summary>Đáp án</summary>

ET chỉ phát sự kiện khi có thay đổi;[^ep-et-change] sự kiện bị tiêu thụ ở bước 3 và lần đọc 1 KB không rút cạn buffer.[^ep-et-hang] LT về bản chất là một `poll(2)` nhanh hơn nên còn dữ liệu thì còn báo.[^ep-lt]

</details>

5. Hai quy tắc dùng ET đúng cách? Với stream socket có đường tắt nào, và vì sao dặn thận trọng?
<details><summary>Đáp án</summary>

Fd non-blocking và chỉ chờ sau khi `read` hoặc `write` trả `EAGAIN`.[^ep-et-rule] `read` trả ít byte hơn yêu cầu cho biết đã rút cạn,[^ep-qa-short] nhưng nên tránh kỹ thuật này nếu không chắc fd luôn là stream-oriented.[^ep-qa-short-caveat]

</details>

6. `EPOLLONESHOT` làm gì? Câu nào trong tài liệu nói về dùng nó cho đa luồng?
<details><summary>Đáp án</summary>

Sau một sự kiện, fd bị vô hiệu hóa đến khi `EPOLL_CTL_MOD`.[^ep-oneshot] Tài liệu chỉ nêu lý do "nhiều chunk → nhiều sự kiện";[^ep-oneshot-et] phần đa luồng là suy luận của người soạn.

</details>

7. "Một hoặc nhiều" của `EPOLLEXCLUSIVE` nghĩa là gì; khác gì ET trên một epoll fd dùng chung?
<details><summary>Đáp án</summary>

Nhiều epoll fd gắn vào cùng fd đích: một hoặc nhiều nhận event, không hứa đúng một.[^ep-excl-one] ET trên *một* epoll fd có nhiều thread chờ: chỉ một thread được đánh thức.[^ep-herd]

</details>

8. `close(fd)` mà epoll vẫn báo sự kiện cho số fd đó: nguyên nhân và cách chữa?
<details><summary>Đáp án</summary>

OFD còn được tham chiếu qua `dup` hoặc `fork`;[^ep-close] chữa bằng `EPOLL_CTL_DEL` trước. Hoặc event cache còn sự kiện cũ trong cùng lô;[^ep-cache] chữa bằng đánh dấu "removed".[^ep-cache-fix]

</details>

9. Vì sao `epoll_ctl(ADD)` trên file thường thất bại, và mio đặt cờ nào cho mọi fd?
<details><summary>Đáp án</summary>

`EPERM`: file đích không hỗ trợ epoll.[^ep-eperm] mio đặt `EPOLLET`;[^mio-src] docs mio đòi lặp tới `WouldBlock`.[^mio-drain]

</details>

## Đọc thêm

- [`epoll(7)`](https://manpages.debian.org/bookworm/manpages/epoll.7.en.html) — đọc *toàn bộ* các mục "Level-triggered and edge-triggered", "Example for suggested usage", "Questions and answers", "Possible pitfalls": đó là đặc tả hành vi thật.
- [`epoll_ctl(2)`](https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html) — danh sách đầy đủ cờ (`EPOLLET`, `EPOLLONESHOT`, `EPOLLEXCLUSIVE`, `EPOLLWAKEUP`) và mã lỗi.
- [Chương 01 — Mô hình I/O](01-mo-hinh-io.md) — vì sao cần readiness; [Chương 03 — kqueue và io_uring](03-kqueue-io-uring.md) — hai hướng khác; [Chương 04](04-kien-truc-event-loop.md) — dựng event loop trên cơ chế này; [Chương 07](07-executor-va-mio.md), mục 4–6 — mio chi tiết.
- [P1 · Chương 04 — File descriptor và VFS](../01-linux/04-file-descriptor-va-vfs.md), mục 1–3 — OFD, `dup`: nền của bẫy mục 6.

[^epoll-linux]: epoll(7), mục STANDARDS — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "The epoll API is Linux-specific."
[^open-poll]: open(2), mục O_NONBLOCK — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "since those interfaces merely inform the caller about whether a file descriptor is ready, meaning that an I/O operation performed on the file descriptor with the O_NONBLOCK flag clear would not block"
[^sel-ready]: select(2), mục DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/select.2.en.html> — "A file descriptor is considered ready if it is possible to perform a corresponding I/O operation (e.g., read(2), or a sufficiently small write(2)) without blocking."
[^sel-nfds]: select(2), mục Arguments — <https://manpages.debian.org/bookworm/manpages-dev/select.2.en.html> — "This argument should be set to the highest-numbered file descriptor in any of the three sets, plus 1."
[^sel-nfds2]: select(2), mục Arguments — <https://manpages.debian.org/bookworm/manpages-dev/select.2.en.html> — "The indicated file descriptors in each set are checked, up to this limit (but see BUGS)."
[^sel-warn]: select(2), mục DESCRIPTION (WARNING) — <https://manpages.debian.org/bookworm/manpages-dev/select.2.en.html> — "select() can monitor only file descriptors numbers that are less than FD_SETSIZE (1024)"
[^sel-glibc]: select(2), mục BUGS — <https://manpages.debian.org/bookworm/manpages-dev/select.2.en.html> — "The Linux kernel imposes no fixed limit, but the glibc implementation makes fd_set a fixed-size type, with FD_SETSIZE defined as 1024"
[^sel-reinit]: select(2), mục DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/select.2.en.html> — "Thus, if using select() within a loop, the sets must be reinitialized before each call."
[^sel-design]: select(2), mục BUGS — <https://manpages.debian.org/bookworm/manpages-dev/select.2.en.html> — "The implementation of the fd_set arguments as value-result arguments is a design error that is avoided in poll(2) and epoll(7)."
[^sel-spurious]: select(2), mục BUGS — <https://manpages.debian.org/bookworm/manpages-dev/select.2.en.html> — "while nevertheless a subsequent read blocks. This could for example happen when data has arrived but upon examination has the wrong checksum and is discarded."
[^sel-spurious-nb]: select(2), mục BUGS — <https://manpages.debian.org/bookworm/manpages-dev/select.2.en.html> — "Thus it may be safer to use O_NONBLOCK on sockets that should not block."
[^sel-sets]: select(2), mục DESCRIPTION (File descriptor sets) — <https://manpages.debian.org/bookworm/manpages-dev/select.2.en.html> — "which allow the caller to wait for three classes of events on the specified set of file descriptors."
[^sel-macros]: select(2), mục DESCRIPTION (File descriptor sets) — <https://manpages.debian.org/bookworm/manpages-dev/select.2.en.html> — "The contents of a file descriptor set can be manipulated using the following macros:"
[^sel-pselect6]: select(2), mục NOTES (C library/kernel differences) — <https://manpages.debian.org/bookworm/manpages-dev/select.2.en.html> — "The underlying Linux system call is named pselect6()."
[^k-arm64-unistd]: Linux v6.8, arch/arm64/include/uapi/asm/unistd.h — <https://raw.githubusercontent.com/torvalds/linux/v6.8/arch/arm64/include/uapi/asm/unistd.h> — "#define __ARCH_WANT_SYS_CLONE3 #define __ARCH_WANT_MEMFD_SECRET #include \<asm-generic/unistd.h>"
[^k-generic-pselect6]: Linux v6.8, include/uapi/asm-generic/unistd.h — <https://raw.githubusercontent.com/torvalds/linux/v6.8/include/uapi/asm-generic/unistd.h> — "#define __NR_pselect6 72 __SC_COMP_3264(__NR_pselect6, sys_pselect6_time32, sys_pselect6, compat_sys_pselect6_time32)"
[^k-generic-pwait]: Linux v6.8, include/uapi/asm-generic/unistd.h — <https://raw.githubusercontent.com/torvalds/linux/v6.8/include/uapi/asm-generic/unistd.h> — "#define __NR_epoll_pwait 22 __SC_COMP(__NR_epoll_pwait, sys_epoll_pwait, compat_sys_epoll_pwait)"
[^k-poll-waiters]: Linux v6.8, fs/select.c, do_poll — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/select.c> — "All waiters have already been registered, so don't provide a poll_table->_qproc to them on the next loop iteration."
[^poll-desc]: poll(2), mục DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/poll.2.en.html> — "poll() performs a similar task to select(2): it waits for one of a set of file descriptors to become ready to perform I/O."
[^poll-fds]: poll(2), mục DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/poll.2.en.html> — "The set of file descriptors to be monitored is specified in the fds argument, which is an array of structures of the following form:"
[^poll-revents]: poll(2), mục DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/poll.2.en.html> — "The field revents is an output parameter, filled by the kernel with the events that actually occurred."
[^poll-neg]: poll(2), mục DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/poll.2.en.html> — "If this field is negative, then the corresponding events field is ignored and the revents field returns zero."
[^poll-nval]: poll(2), mục DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/poll.2.en.html> — "Invalid request: fd not open (only returned in revents; ignored in events)."
[^poll-hup]: poll(2), mục DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/poll.2.en.html> — "Subsequent reads from the channel will return 0 (end of file) only after all outstanding data in the channel has been consumed."
[^poll-always]: poll(2), mục DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/poll.2.en.html> — "These three bits are meaningless in the events field, and will be set in the revents field whenever the corresponding condition is true."
[^k-poll-fish]: Linux v6.8, fs/select.c, do_poll — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/select.c> — "Fish for events. If we found one, record it"
[^k-poll-loop]: Linux v6.8, fs/select.c, do_poll — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/select.c> — "for (; pfd != pfd_end; pfd++) {"
[^k-select-loop]: Linux v6.8, fs/select.c, do_select — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/select.c> — "for (j = 0; j < BITS_PER_LONG; ++j, ++i, bit <<= 1) {"
[^epoll-intro]: epoll(7), mục DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "The epoll API performs a similar task to poll(2): monitoring multiple file descriptors to see if I/O is possible on any of them. The epoll API can be used either as an edge-triggered or a level-triggered interface and scales well to large numbers of watched file descriptors."
[^epoll-lists]: epoll(7), mục DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "The ready list is a subset of (or, more precisely, a set of references to) the file descriptors in the interest list."
[^ep-create]: epoll_create(2), mục DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/epoll_create.2.en.html> — "Since Linux 2.6.8, the size argument is ignored, but must be greater than zero; see NOTES."
[^ep-create1]: epoll_create(2), mục epoll_create1() — <https://manpages.debian.org/bookworm/manpages-dev/epoll_create.2.en.html> — "EPOLL_CLOEXEC Set the close-on-exec (FD_CLOEXEC) flag on the new file descriptor."
[^ep-ctl]: epoll_ctl(2), mục DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "This system call is used to add, modify, or remove entries in the interest list of the epoll(7) instance referred to by the file descriptor epfd."
[^ep-ctl-del]: epoll_ctl(2), mục DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "The event argument is ignored and can be NULL (but see BUGS below)."
[^ep-pwait-size]: epoll_wait(2), mục C library/kernel differences — <https://manpages.debian.org/bookworm/manpages-dev/epoll_wait.2.en.html> — "The raw epoll_pwait() and epoll_pwait2() system calls have a sixth argument, size_t sigsetsize, which specifies the size in bytes of the sigmask argument."
[^k-ep-func-entry]: Linux v6.8, fs/eventpoll.c, ep_ptable_queue_proc — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/eventpoll.c> — "init_waitqueue_func_entry(&pwq->wait, ep_poll_callback); pwq->whead = whead; pwq->base = epi;"
[^k-ep-send-mask]: Linux v6.8, fs/eventpoll.c, ep_send_events — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/eventpoll.c> — "If the event mask intersect the caller-requested one, deliver the event to userspace."
[^k-ep-lt-requeue]: Linux v6.8, fs/eventpoll.c, ep_send_events — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/eventpoll.c> — "If this file has been added with Level Trigger mode, we need to insert back inside the ready list, so that the next call to epoll_wait() will check again the events availability."
[^k-ep-mask-filter]: Linux v6.8, fs/eventpoll.c, ep_poll_callback — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/eventpoll.c> — "if (pollflags && !(pollflags & epi->event.events)) goto out_unlock;"
[^k-ep-oneshot-clear]: Linux v6.8, fs/eventpoll.c, ep_send_events — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/eventpoll.c> — "if (epi->event.events & EPOLLONESHOT) epi->event.events &= EP_PRIVATE_BITS; else if (!(epi->event.events & EPOLLET)) {"
[^k-ep-eperm]: Linux v6.8, fs/eventpoll.c, do_epoll_ctl — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/eventpoll.c> — "The target file descriptor must support poll error = -EPERM; if (!file_can_poll(tf.file)) goto error_tgt_fput;"
[^ep-ctl-ofd]: epoll_ctl(2), mục DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "The entry includes the file descriptor, fd, a reference to the corresponding open file description (see epoll(7) and open(2)), and the settings specified in event."
[^ep-wait]: epoll_wait(2), mục DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/epoll_wait.2.en.html> — "Up to maxevents are returned by epoll_wait()."
[^ep-wait-timeout]: epoll_wait(2), mục DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/epoll_wait.2.en.html> — "Specifying a timeout of -1 causes epoll_wait() to block indefinitely, while specifying a timeout equal to zero cause epoll_wait() to return immediately, even if no events are available."
[^ep-wait-fetch]: epoll(7), mục DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "This system call can be thought of as fetching items from the ready list of the epoll instance."
[^ep-wait-data]: epoll_wait(2), mục DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/epoll_wait.2.en.html> — "The data field of each returned epoll_event structure contains the same data as was specified in the most recent call to epoll_ctl(2) (EPOLL_CTL_ADD, EPOLL_CTL_MOD) for the corresponding open file descriptor."
[^k-ep-callback]: Linux v6.8, fs/eventpoll.c, comment về nested epoll — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/eventpoll.c> — "issue a wake_up() on its poll wake list. Epoll (efd1) has installed a"
[^k-ep-ready]: Linux v6.8, fs/eventpoll.c, ep_poll_callback — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/eventpoll.c> — "In the usual case, add event to ready list."
[^ep-pwait]: epoll_wait(2), mục epoll_pwait() — <https://manpages.debian.org/bookworm/manpages-dev/epoll_wait.2.en.html> — "The sigmask argument may be specified as NULL, in which case epoll_pwait() is equivalent to epoll_wait()."
[^ep-et]: epoll_ctl(2), mục EPOLLET — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "EPOLLET Requests edge-triggered notification for the associated file descriptor. The default behavior for epoll is level-triggered."
[^ep-lt]: epoll(7), mục Level-triggered and edge-triggered — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "when used as a level-triggered interface (the default, when EPOLLET is not specified), epoll is simply a faster poll(2)"
[^ep-et-change]: epoll(7), mục Level-triggered and edge-triggered — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "edge-triggered mode delivers events only when changes occur on the monitored file descriptor."
[^ep-et-scenario]: epoll(7), mục Level-triggered and edge-triggered — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "call to epoll_wait(2) done in step 5 will probably hang despite the available data still present in the file input buffer"
[^ep-et-hang]: epoll(7), mục Level-triggered and edge-triggered — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "Since the read operation done in 4 does not consume the whole buffer data, the call to epoll_wait(2) done in step 5 might block indefinitely."
[^ep-et-rule]: epoll(7), mục Level-triggered and edge-triggered — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "(1) with nonblocking file descriptors; and (2) by waiting for an event only after read(2) or write(2) return EAGAIN."
[^ep-et-nonblock]: epoll(7), mục Level-triggered and edge-triggered — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "An application that employs the EPOLLET flag should use nonblocking file descriptors to avoid having a blocking read or write starve a task that is handling multiple file descriptors."
[^ep-qa-eagain]: epoll(7), mục Questions and answers — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "You must consider it ready until the next (nonblocking) read/write yields EAGAIN."
[^ep-qa-packet]: epoll(7), mục Questions and answers — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "the only way to detect the end of the read/write I/O space is to continue to read/write until EAGAIN."
[^ep-qa-short]: epoll(7), mục Questions and answers — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "if you call read(2) by asking to read a certain amount of data and read(2) returns a lower number of bytes, you can be sure of having exhausted the read I/O space for the file descriptor."
[^ep-qa-short-caveat]: epoll(7), mục Questions and answers — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "Avoid this latter technique if you cannot guarantee that the monitored file descriptor always refers to a stream-oriented file."
[^c10k-edge]: Dan Kegel, The C10K problem, mục Serve many clients with each thread, and use nonblocking I/O and readiness change notification — <http://www.kegel.com/c10k.html> — "It's a bit less forgiving of programming mistakes, since if you miss just one event, the connection that event was for gets stuck forever."
[^ep-both]: epoll(7), mục Example for suggested usage — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "This allows you to avoid continuously switching between EPOLLIN and EPOLLOUT calling epoll_ctl(2) with EPOLL_CTL_MOD."
[^ep-key]: epoll(7), mục Questions and answers — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "The key is the combination of the file descriptor number and the open file description"
[^ep-close]: epoll(7), mục Questions and answers — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "This means that even after a file descriptor that is part of an interest list has been closed, events may be reported for that file descriptor if other file descriptors referring to the same underlying file description remain open."
[^ep-eexist]: epoll(7), mục Questions and answers — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "You will probably get EEXIST. However, it is possible to add a duplicate (dup(2), dup2(2), fcntl(2) F_DUPFD) file descriptor to the same epoll instance."
[^ep-two-instances]: epoll(7), mục Questions and answers — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "Yes, and events would be reported to both."
[^ep-mod]: epoll(7), mục Questions and answers — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "Modify will reread available I/O."
[^ep-cache]: epoll(7), mục Possible pitfalls and ways to avoid them — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "Suppose you receive 100 events from epoll_wait(2), and in event #47 a condition causes event #13 to be closed."
[^ep-cache-fix]: epoll(7), mục Possible pitfalls and ways to avoid them — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "to delete file descriptor 13 and close(2), then mark its associated data structure as removed and link it to a cleanup list."
[^ep-combine]: epoll(7), mục Questions and answers — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "If more than one event occurs between epoll_wait(2) calls, are they combined or reported separately? They will be combined."
[^ep-starve]: epoll(7), mục Possible pitfalls and ways to avoid them — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "If there is a large amount of I/O space, it is possible that by trying to drain it the other files will not get processed causing starvation."
[^ep-starve-fix]: epoll(7), mục Possible pitfalls and ways to avoid them — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "thereby allowing the application to remember which files need to be processed but still round robin amongst all the ready files."
[^ep-oneshot]: epoll_ctl(2), mục EPOLLONESHOT — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "the file descriptor is disabled in the interest list and no other events will be reported by the epoll interface."
[^ep-oneshot-rearm]: epoll_ctl(2), mục EPOLLONESHOT — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "The user must call epoll_ctl() with EPOLL_CTL_MOD to rearm the file descriptor with a new event mask."
[^ep-oneshot-et]: epoll(7), mục Level-triggered and edge-triggered — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "the caller has the option to specify the EPOLLONESHOT flag, to tell epoll to disable the associated file descriptor after the receipt of an event with epoll_wait(2)."
[^k-ep-oneshot]: Linux v6.8, fs/eventpoll.c, ep_poll_callback — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/eventpoll.c> — "EPOLLONESHOT bit that disables the descriptor when an event is received,"
[^ep-herd]: epoll(7), mục Level-triggered and edge-triggered — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "just one of the threads (or processes) is awoken from epoll_wait(2)."
[^ep-excl]: epoll_ctl(2), mục EPOLLEXCLUSIVE — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "EPOLLEXCLUSIVE is thus useful for avoiding thundering herd problems in certain scenarios."
[^ep-excl-one]: epoll_ctl(2), mục EPOLLEXCLUSIVE — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "one or more of the epoll file descriptors will receive an event with epoll_wait(2)."
[^ep-excl-add]: epoll_ctl(2), mục EPOLLEXCLUSIVE — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "EPOLLEXCLUSIVE may be used only in an EPOLL_CTL_ADD operation; attempts to employ it with EPOLL_CTL_MOD yield an error."
[^ep-excl-mixed]: epoll_ctl(2), mục EPOLLEXCLUSIVE — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "then events will be provided to all epoll instances that did not specify EPOLLEXCLUSIVE, and at least one of the epoll instances that did specify EPOLLEXCLUSIVE."
[^ep-excl-flags]: epoll_ctl(2), mục EPOLLEXCLUSIVE — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "Attempts to specify other values in events yield the error EINVAL."
[^ep-excl-einval]: epoll_ctl(2), mục ERRORS — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "op was EPOLL_CTL_MOD and events included EPOLLEXCLUSIVE."
[^ep-rdhup]: epoll_ctl(2), mục EPOLLRDHUP — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "Stream socket peer closed connection, or shut down writing half of connection. (This flag is especially useful for writing simple code to detect peer shutdown when using edge-triggered monitoring.)"
[^ep-hup-always]: epoll_ctl(2), mục EPOLLHUP — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "epoll_wait(2) will always wait for this event; it is not necessary to set it in events when calling epoll_ctl()."
[^ep-hup]: epoll_ctl(2), mục EPOLLHUP — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "Subsequent reads from the channel will return 0 (end of file) only after all outstanding data in the channel has been consumed."
[^ep-err]: epoll_ctl(2), mục EPOLLERR — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "This event is also reported for the write end of a pipe when the read end has been closed."
[^ep-err-always]: epoll_ctl(2), mục EPOLLERR — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "epoll_wait(2) will always report for this event; it is not necessary to set it in events when calling epoll_ctl()."
[^ep-eperm]: epoll_ctl(2), mục ERRORS — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "The target file fd does not support epoll."
[^ep-watches]: epoll(7), mục /proc interfaces — <https://manpages.debian.org/bookworm/manpages/epoll.7.en.html> — "Each registered file descriptor costs roughly 90 bytes on a 32-bit kernel, and roughly 160 bytes on a 64-bit kernel."
[^ep-enospc]: epoll_ctl(2), mục ERRORS — <https://manpages.debian.org/bookworm/manpages-dev/epoll_ctl.2.en.html> — "The limit imposed by /proc/sys/fs/epoll/max_user_watches was encountered while trying to register (EPOLL_CTL_ADD) a new file descriptor on an epoll instance."
[^mio-impl]: mio 1.2.4, struct Poll, mục Implementation notes — <https://docs.rs/mio/1.2.4/mio/struct.Poll.html> — "Android epoll DragonFly BSD kqueue FreeBSD kqueue iOS kqueue illumos epoll Linux epoll NetBSD kqueue OpenBSD kqueue Solaris event ports Windows IOCP macOS kqueue"
[^mio-kq]: mio v1.2.4, src/sys/unix/selector/kqueue.rs, fn register — <https://raw.githubusercontent.com/tokio-rs/mio/v1.2.4/src/sys/unix/selector/kqueue.rs> — "let flags = libc::EV_CLEAR | libc::EV_RECEIPT | libc::EV_ADD;"
[^kq-clear]: FreeBSD kqueue(2), mục flags EV_CLEAR — <https://man.freebsd.org/cgi/man.cgi?query=kqueue&sektion=2&format=html> — "Reset the state of the event after it is retrieved by the user."
[^mio-src]: mio v1.2.4, src/sys/unix/selector/epoll.rs, fn interests_to_epoll — <https://raw.githubusercontent.com/tokio-rs/mio/v1.2.4/src/sys/unix/selector/epoll.rs> — "fn interests_to_epoll(interests: Interest) -> u32 { let mut kind = EPOLLET;"
[^mio-drain]: mio 1.2.4, struct Poll, mục Draining readiness — <https://docs.rs/mio/1.2.4/mio/struct.Poll.html> — "Once a readiness event is received, the corresponding operation must be performed repeatedly until it returns WouldBlock."
