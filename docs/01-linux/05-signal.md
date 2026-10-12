# P1 · Chương 05 — Signal

> Mọi khẳng định về kernel/syscall là về **Linux** (man-pages 6.03, Debian bookworm). Ví dụ "đã chạy" chạy trong container Linux aarch64 của `./labs/linux.sh`; kết quả là *quan sát* một lần chạy, không phải bảo đảm của tài liệu.

## Bạn sẽ hiểu được gì

1. **Q1.** Một signal đi từ lúc phát sinh tới lúc handler của `orders` chạy qua những bước nào? Disposition, pending, mask, delivery là gì và handler thực sự chạy lúc nào? → mục 2
2. **Q2.** Vì sao ba `SIGCHLD` đến sát nhau có thể chỉ gọi handler một lần, còn signal real-time thì không? → mục 3
3. **Q3.** `sigaction`, `sigprocmask`, `pthread_sigmask` mỗi cái chỉnh cái gì? → mục 4
4. **Q4.** Vì sao `read` của một handler thread bỗng trả `EINTR`, và `SA_RESTART` cứu được đến đâu? → mục 5
5. **Q5.** Vì sao trong handler chỉ được gọi hàm async-signal-safe? → mục 6
6. **Q6.** Vì sao handler `SIGCHLD` phải lặp `waitpid`, và `SIGPIPE`/`EPIPE` hoạt động ra sao? → mục 7
7. **Q7.** `SIGTERM` khác `SIGKILL` thế nào, graceful shutdown dựa vào đâu, và PID 1 trong container đặc biệt chỗ nào? → mục 8
8. **Q8.** `signalfd(2)` và `sigwaitinfo(2)` giải quyết vấn đề gì, và signal tương tác với thread ra sao? → mục 9

## Kiến thức cần trước

Chương này dùng lại service `orders` của [Chương 01](01-os-va-kernel.md): một process Rust chạy trên Linux, có main thread `accept` kết nối và vài handler thread. Ở đây ta cho nó chạy trong một Pod Kubernetes.

- [Chương 01 — OS và kernel](01-os-va-kernel.md): user mode/kernel mode, trap, interrupt (cần cho mục 2).
- [Chương 02 — System call](02-system-call.md): trap, `errno`, wrapper libc.
- [Chương 03 — Process](03-process.md): `fork`/`exec`/`wait`, zombie, orphan, process group.
- [Chương 04 — File descriptor và VFS](04-file-descriptor-va-vfs.md): fd, pipe (cần cho `SIGPIPE` và `signalfd`).

---

## 1. Signal giải quyết vấn đề gì

Một service chạy lâu luôn có những sự kiện mà nó không tự đi hỏi. Kubelet quyết định dừng Pod chứa `orders`. Một process con `pdf-gen`, chuyên render hóa đơn PDF, vừa chết (nhận xét của người soạn: `pdf-gen` là ví dụ giả định). Một handler thread ghi vào pipe tới process con mà con đã đóng đầu đọc. Khi chạy thử trên máy mình, bạn bấm Ctrl-C. Trong cả bốn trường hợp `orders` đang làm việc khác, và OS cần một cách để báo tin cho nó.

Cách đó là signal: hạ tầng của OS để chuyển các sự kiện từ bên ngoài tới process, kèm cách để process nhận và xử lý chúng[^ostep-signals]. Process cũng tự gửi được: `kill()` gửi một signal bất kỳ tới một process hoặc một process group[^kill-send].

Signal khó dùng đúng vì hai lẽ, và ta sẽ gặp cả hai ở các mục sau (nhận xét của người soạn). Thứ nhất, signal *bất đồng bộ*: nó đến lúc nào không ai báo trước, và handler, tức hàm bạn đăng ký để chạy khi signal tới, chen vào chương trình ở một chỗ tùy ý (mục 2 và mục 6). Thứ hai, signal chuẩn có thể bị gộp, nên đếm số signal nhận được không cho ra số sự kiện (mục 3).

---

## 2. Mô hình: disposition, pending, mask, delivery (Q1)

> **Trả lời ngắn.** Mỗi signal có một *disposition*, tức cách process phản ứng khi signal được *deliver* (giao tới nó): làm hành động mặc định, bỏ qua, hoặc chạy một hàm do bạn đăng ký, gọi là *handler*[^sig-disp]. Signal đã phát sinh mà đang bị chặn (block) thì nằm chờ ở trạng thái *pending* cho tới khi được bỏ chặn[^sig-pending]. Disposition là của cả process, còn danh sách signal đang bị chặn, gọi là *mask*, là của từng thread[^sig-thread-mask]. Handler không chạy ngay lúc signal phát sinh mà chạy khi kernel đưa thread về user mode[^sig-delivery-check].

**Giải thích.**

*Với service `orders`.* Giả sử Pod chứa `orders` bị dừng và kubelet gửi `SIGTERM` tới nó (mục 8 nói kỹ phần Kubernetes). Lúc đó một handler thread có thể đang ghi dở một đơn hàng. Câu hỏi của mục này: signal tới thì process phản ứng thế nào, thread nào nhận, và handler xen vào chỗ nào của code đang chạy? Để trả lời cần ba khái niệm: disposition, pending và mask.

### 2.1 Disposition và hành động mặc định

Mỗi signal có một disposition hiện tại, quyết định process phản ứng ra sao khi signal được deliver[^sig-disp]. Nếu bạn không đăng ký gì thì kernel chạy hành động mặc định của signal đó, và hành động này thuộc một trong năm loại: `Term` (kết thúc process), `Ign` (bỏ qua), `Core` (kết thúc và ghi core dump), `Stop` (dừng process), `Cont` (tiếp tục nếu đang dừng)[^sig-actions].

Muốn đổi, process gọi `sigaction(2)` (hoặc `signal(2)`, kém portable khi đặt handler) để chọn một trong ba cách: giữ mặc định, bỏ qua, hoặc *bắt* signal bằng một handler, là hàm do lập trình viên viết và kernel tự gọi khi signal tới[^sig-choose].

Có hai signal bạn không đổi được. `SIGKILL` và `SIGSTOP` không thể bị catch, block hay ignore[^sig-uncatchable]. Nếu thử đổi hành động của chúng thì `sigaction` trả `EINVAL`[^sigaction-einval].

### 2.2 Phạm vi và thừa kế

Disposition là thuộc tính của process: process nhiều thread thì mọi thread dùng chung một disposition cho mỗi signal[^sig-perproc]. Vì vậy handler `SIGTERM` do thread nào của `orders` cài thì cũng có hiệu lực cho cả main thread lẫn các handler thread.

Khi `orders` sinh process con `pdf-gen`, hai lời gọi xử lý disposition khác nhau. `fork` cho con một bản sao disposition của cha. `execve` thì đặt lại disposition của các signal *có handler* về mặc định, còn signal đang *bị ignore* thì giữ nguyên[^sig-fork-exec]. Lý do có lẽ là sau `execve` code handler của chương trình cũ không còn trong bộ nhớ (suy luận của người soạn; man page không nêu lý do).

### 2.3 Pending và mask

Một signal có thể bị *block*: kernel sẽ không deliver nó cho tới khi nó được unblock. Khoảng thời gian từ lúc signal phát sinh tới lúc được deliver gọi là *pending*[^sig-pending]. Mỗi thread có một *signal mask* riêng, là tập signal mà thread đó đang block[^sig-thread-mask]. Chẳng hạn nếu mọi thread của `orders` đều block `SIGTERM` thì signal không mất; nó nằm pending cho tới khi một thread unblock (nhận xét của người soạn, suy ra từ hai định nghĩa trên).

### 2.4 Signal đi tới thread nào

Process nhiều thread còn một câu hỏi nữa: signal rơi vào thread nào? Có hai kiểu đích. Signal *process-directed* nhắm vào cả process, do `kill(2)`, `sigqueue(3)` hoặc do kernel sinh ra không vì hardware exception[^sig-procdir]. Signal *thread-directed* nhắm một thread cụ thể, ví dụ khi một lệnh máy gây hardware exception (`SIGSEGV` do truy cập bộ nhớ sai, `SIGFPE` do lỗi số học)[^sig-hw]. Gửi thẳng tới một thread bằng `tgkill(2)`/`pthread_kill(3)` cũng cho signal loại này[^sig-tgkill].

`SIGTERM` mà kubelet gửi cho `orders` thuộc loại process-directed. Kernel có thể giao nó cho bất kỳ thread nào đang không block, và nếu nhiều thread cùng không block thì chọn tùy ý[^sig-directed]. Nên bạn đừng giả định main thread là nơi nhận.

### 2.5 Handler thực sự chạy lúc nào

Hình dưới gộp các bước trên thành một đường đi của signal:

```mermaid
flowchart TD
  G["Signal được sinh ra<br/>(kill, kernel, hardware exception...)"] --> B{"bị block<br/>(trong mask)?"}
  B -->|có| P["pending — chờ unblock"]
  P -->|"unblock"| D
  B -->|không| D{"disposition"}
  D -->|"mặc định"| T["Term / Core / Stop / Cont / Ign"]
  D -->|"ignore"| I["bỏ qua"]
  D -->|"handler"| H["kernel dựng frame trên stack,<br/>gọi handler"]
```

Handler không chạy ngay lúc signal phát sinh. Mỗi khi một thread chuyển từ kernel mode về user mode, chẳng hạn khi return từ system call hoặc khi được xếp lịch chạy lại, kernel kiểm tra xem có signal nào đang pending, không bị block, mà process đã đặt handler hay không[^sig-delivery-check]. Đây không phải interrupt phần cứng (nhận xét của người soạn; về trap, interrupt và chuyển giữa hai chế độ xem [Chương 01, mục 3 và 4](01-os-va-kernel.md)). Nếu có, kernel dựng bối cảnh gọi handler như sau:

```mermaid
sequenceDiagram
  participant K as Kernel
  participant U as Thread (user mode)
  participant H as handler()
  participant T as signal trampoline
  K->>K: kiểm tra pending & unblocked khi quay về user mode
  K->>K: gỡ signal khỏi tập pending
  K->>U: lưu PC, thanh ghi, signal mask vào frame trên stack
  K->>K: thêm sa_mask và chính signal đó vào mask
  K->>H: nhảy vào handler; địa chỉ return trỏ tới trampoline
  H-->>T: handler return
  T->>K: sigreturn(2)
  K->>U: khôi phục thanh ghi + mask, chạy tiếp chỗ bị ngắt
```

Mặc định handler chạy trên stack bình thường của chính thread bị ngắt[^sig-stack-default]. Khi handler return, thread chạy tiếp đúng chỗ nó bị ngắt[^sig-resume]. Từ đó suy ra (suy luận của người soạn) rằng handler có thể xen vào giữa một hàm thư viện đang chạy dở trên thread đó, và đây là gốc của vấn đề ở mục 6.

<details><summary>Đi sâu: frame trên stack, trampoline, số hiệu signal</summary>

Trước khi chạy handler, kernel gỡ signal khỏi tập pending. Nếu handler được cài bằng `sigaction(2)` với cờ `SA_ONSTACK` và thread đã khai báo stack thay thế bằng `sigaltstack(2)` thì kernel dùng stack đó. Rồi nó lưu ngữ cảnh vào một frame đặc biệt trên stack[^sig-frame]. Frame chứa PC, trạng thái thanh ghi cần để chạy lại chương trình, signal mask và thiết lập stack thay thế của thread[^sig-frame-list]. Trong lúc handler chạy, `sa_mask` và chính signal đang deliver bị thêm vào mask, trừ khi dùng `SA_NODEFER`[^sig-mask-during]. Khi handler return, signal trampoline gọi `sigreturn(2)` để khôi phục trạng thái, kể cả mask[^sig-trampoline].

Số hiệu signal phụ thuộc kiến trúc: bảng của man page có cột riêng cho x86/ARM (và đa số kiến trúc khác), Alpha/SPARC, MIPS, PARISC[^sig-num-arch]. Vì vậy code portable nên dùng tên macro thay vì số (nhận xét của người soạn). Ở cột x86/ARM:

- `SIGKILL` là 9[^sig-num-kill].
- `SIGTERM` là 15[^sig-num-term].
- `SIGCHLD` là 17[^sig-num-chld].

Hành động mặc định của `SIGCHLD` là `Ign`[^sig-tbl-chld].

</details>

**Quay lại Q1:** disposition (theo process) quyết định phản ứng, mask (theo thread) quyết định signal nào phải chờ ở pending, và handler được gọi lúc kernel đưa thread về user mode, khi thấy có signal pending, không bị block và có handler.

---

## 3. Signal chuẩn và signal real-time (Q2)

> **Trả lời ngắn.** Signal chuẩn (như `SIGINT`, `SIGTERM`, `SIGCHLD`) không xếp hàng: nếu cùng một loại phát sinh nhiều lần trong lúc đang bị chặn thì chỉ còn một cái pending[^sig-noqueue]. Signal real-time (dải `SIGRTMIN`…`SIGRTMAX`) thì xếp hàng được và được deliver theo đúng thứ tự gửi[^sig-rt-order]. Gửi bằng `sigqueue(3)` thì còn kèm được một giá trị[^sig-rt-sigqueue]. Vì vậy với signal chuẩn, đừng lấy số lần nhận signal làm số lần sự kiện (nhận xét của người soạn).

**Giải thích.**

*Với service `orders`.* Giả sử ba process `pdf-gen` render xong gần như cùng lúc và cùng thoát. Kernel gửi cho `orders` ba `SIGCHLD`, và bạn nghĩ handler sẽ chạy ba lần. Có thể nó chỉ chạy một lần. Mục 7 sẽ cho thấy hậu quả, còn mục này giải thích vì sao.

### 3.1 Signal chuẩn bị gộp, real-time thì không

Với signal chuẩn, ba lần phát sinh khi đang bị block chỉ để lại một signal pending, nên sau khi unblock nó được deliver đúng một lần[^sig-noqueue]. Signal real-time thì khác: mỗi lần gửi là một phần tử trong hàng đợi. Bảng dưới so sánh hai loại:

| | Signal chuẩn (standard) | Real-time |
|---|---|---|
| Hàng đợi | không queue: nhiều lần sinh khi đang bị block chỉ còn một pending[^sig-noqueue] | nhiều instance cùng loại được queue[^sig-rt-queue] |
| Thứ tự deliver | không xác định[^sig-order-std] | có bảo đảm: cùng loại theo thứ tự gửi[^sig-rt-order] |
| Dữ liệu đi kèm | lần đến sau không ghi đè `siginfo_t` của lần đang pending: process nhận thông tin của lần đầu[^sig-siginfo-first] | `sigqueue(3)` gửi kèm một số nguyên hoặc con trỏ[^sig-rt-sigqueue] |

Real-time signal không có ý nghĩa định sẵn; ứng dụng tự quy ước dùng chúng vào việc gì[^sig-rt-meaning].

### 3.2 Thử: signal chuẩn bị gộp, real-time thì không

Đã chạy trong container Linux aarch64: block `SIGUSR1` và `SIGRTMIN`, gửi mỗi loại 3 lần (`raise` cho `SIGUSR1`, `sigqueue` cho `SIGRTMIN`), rồi `sigpending`, rồi unblock. Kết quả:

```text
pending USR1=1 RTMIN=1
handler calls after unblock (3 USR1 + 3 RT sent) = 4
```

3 lần `SIGUSR1` thành đúng **1** lần gọi handler, 3 lần `SIGRTMIN` thành **3**: tổng 4. Đây là **quan sát một lần chạy** khớp với bảo đảm của man page, không phải bằng chứng độc lập.

<details><summary>Đi sâu: dải số real-time, SIGRTMIN, giới hạn hàng đợi</summary>

Kernel Linux hỗ trợ 33 signal real-time đánh số từ 32 đến 64. Nhưng glibc dùng nội bộ một vài signal trong số đó và chỉnh lại `SIGRTMIN`[^sig-rt-range]. Vì vậy chương trình phải viết `SIGRTMIN+n` thay vì số cứng[^sig-rt-hard]. Khi nhiều signal real-time khác loại cùng pending thì deliver từ số thấp tới số cao[^sig-rt-order2]. Signal real-time không có handler thì mặc định cũng kết thúc process[^sig-rt-default]. Số signal có thể queue bị giới hạn theo từng user bằng `RLIMIT_SIGPENDING`[^sig-rtlimit] (về giới hạn tài nguyên nói chung: [Chương 06, mục 6](06-user-quyen-proc-sys.md)).

</details>

**Quay lại Q2:** signal chuẩn bị gộp khi chờ, real-time thì xếp hàng có thứ tự và mang dữ liệu; vì vậy một `SIGCHLD` có thể đại diện cho nhiều process con vừa chết.

---

## 4. Các system call điều khiển: `sigaction`, `sigprocmask`, `pthread_sigmask` (Q3)

> **Trả lời ngắn.** `sigaction` cho process đổi cách phản ứng với một signal: đặt handler, đặt cờ, và chọn những signal bị chặn trong lúc handler chạy[^sigaction-desc]. `sigprocmask` và `pthread_sigmask` đổi danh sách signal đang bị chặn của thread gọi[^sigprocmask-desc]. Chương trình nhiều thread nên dùng `pthread_sigmask`[^pthsig-same].

**Giải thích.**

*Với service `orders`.* Muốn shutdown êm, `orders` phải bắt `SIGTERM`, và việc đó làm bằng `sigaction`. Muốn các thread của nó không bị signal chen vào lúc không muốn, nó chặn signal bằng `pthread_sigmask`. Mục này là bản liệt kê hai nhóm API đó. Lưu ý một phân công: `sigaction` đổi chuyện của cả process, còn mask thì chỉ đổi cho một thread.

### 4.1 `sigaction(2)`

`sigaction` đổi hành động của process khi nhận một signal. Tham số `signum` là signal hợp lệ bất kỳ trừ `SIGKILL` và `SIGSTOP`[^sigaction-desc]. Gọi với `act` là `NULL` thì nó chỉ *đọc* hành động hiện tại[^sigaction-query]. Các trường quan trọng của `struct sigaction`:

- `sa_handler`: một trong `SIG_DFL` (mặc định), `SIG_IGN` (bỏ qua), hoặc con trỏ hàm nhận số signal làm đối số duy nhất[^sigaction-handler].
- `sa_sigaction` cùng cờ `SA_SIGINFO`: handler nhận ba đối số thay vì một[^sigaction-siginfo]. Đối số thứ hai là con trỏ `siginfo_t`, chứa thêm thông tin về signal[^sigaction-info-arg]. Với `SIGCHLD`, nó điền `si_pid`, `si_uid`, `si_status`… cho con liên quan[^sigaction-chld-info].
- `sa_mask`: các signal bị block *trong khi handler chạy*. Signal gây ra handler cũng bị block, trừ khi dùng `SA_NODEFER`[^sigaction-mask].
- `sa_flags`, đáng nhớ gồm:
  - `SA_RESTART`: cho một số system call tự chạy lại sau khi bị signal cắt ngang (mục 5)[^sigaction-restart].
  - `SA_ONSTACK`: chạy handler trên stack thay thế khai báo bằng `sigaltstack(2)` (xem phần Đi sâu ở mục 2)[^sig-frame].
  - `SA_NOCLDSTOP`: không nhận `SIGCHLD` khi con chỉ dừng hoặc tiếp tục, chỉ có nghĩa với `SIGCHLD`[^sigaction-nocldstop].
  - `SA_NOCLDWAIT`: con kết thúc không thành zombie, cũng chỉ dành cho `SIGCHLD`[^sigaction-nocldwait].
  - `SA_RESETHAND`: về mặc định ngay khi vào handler[^sigaction-resethand].

Còn `signal(2)` thì kém portable khi dùng để đặt handler[^sig-signal-portable]. Lý do chi tiết nằm ở man page `signal(2)`, chương này không đối chiếu trang đó. Quy tắc thực dụng (nhận xét của người soạn): cần tự chọn mask, restart hay one-shot cho handler thì dùng `sigaction`.

### 4.2 `sigprocmask(2)` và `pthread_sigmask(3)`

`sigprocmask` đọc và đổi signal mask của thread gọi[^sigprocmask-desc]. Tham số `how` chọn cách đổi: `SIG_BLOCK` đặt mask mới là hợp của mask hiện tại và `set`[^sigprocmask-block]; `SIG_UNBLOCK` bỏ các signal trong `set` khỏi mask, còn `SIG_SETMASK` đặt mask bằng đúng `set`[^sigprocmask-others]. Nếu `set` là `NULL` thì mask không đổi, nhưng giá trị hiện tại vẫn trả về trong `oldset`[^sigprocmask-query]. Thói quen thường gặp là lấy mask cũ ra, chặn tạm một số signal, rồi khôi phục mask cũ (nhận xét của người soạn).

Với chương trình đa thread, hành vi của `sigprocmask` là *unspecified*, tức chuẩn không quy định[^sigprocmask-mt]. Thay vào đó hãy dùng `pthread_sigmask(3)`: nó giống hệt `sigprocmask`, chỉ khác là POSIX quy định rõ cách dùng trong chương trình đa thread[^pthsig-same].

### 4.3 Mask đi theo thread, theo `fork` và `execve`

Thread mới thừa kế một bản sao mask của thread tạo ra nó[^pthsig-inherit]. Con tạo bằng `fork` thừa kế mask của cha, và mask được giữ nguyên qua `execve`[^sig-maskinherit]. Tập pending thì khác: con `fork` bắt đầu với tập rỗng, còn `execve` giữ nguyên tập pending[^sig-pendfork].

Từ những điều trên rút ra một mẫu chống race (nhận xét của người soạn). `orders` gọi `SIG_BLOCK` *trước* khi tạo các handler thread hoặc process con, để mọi thread mới sinh ra đã mang mask chặn signal. Sau đó nó dành riêng một thread, hoặc một fd (mục 9), để nhận signal một cách đồng bộ.

<details><summary>Đi sâu: các bẫy với signal phần cứng</summary>

Theo POSIX, hành vi của process là *không xác định* sau khi nó ignore `SIGFPE`, `SIGILL` hoặc `SIGSEGV` mà signal đó không do `kill(2)`/`raise(3)` sinh ra. Man page nêu ví dụ: chia số nguyên cho 0 có thể sinh `SIGFPE` trên một số kiến trúc, và ignore signal này có thể dẫn tới vòng lặp vô tận[^sigaction-hwignore]. Hiểu đơn giản (nhận xét của người soạn): ignore không làm lỗi phần cứng biến mất.

Với `sigprocmask`, việc block `SIGKILL`/`SIGSTOP` bị lờ đi *âm thầm*[^sigprocmask-silent]. Còn nếu `SIGBUS`, `SIGFPE`, `SIGILL` hoặc `SIGSEGV` phát sinh trong lúc đang bị block (không do `kill`/`sigqueue`/`raise`) thì kết quả là undefined[^sigprocmask-hw].

</details>

**Quay lại Q3:** `sigaction` đặt *cách phản ứng* của cả process, `pthread_sigmask` đặt *cái gì tạm phải chờ* của từng thread; nên block trước khi tạo thread hoặc process con.

---

## 5. `EINTR` và `SA_RESTART` (Q4)

> **Trả lời ngắn.** Nếu signal đến lúc thread đang ngủ trong một system call chặn và handler được gọi, thì khi handler chạy xong call đó hoặc tự chạy lại, hoặc trả lỗi `EINTR` ("bị ngắt"), tùy call và tùy handler có cờ `SA_RESTART` hay không[^sig-eintr]. `SA_RESTART` chỉ cứu được một số call; `epoll_wait`, `poll`, `select` không bao giờ được chạy lại[^sig-norestart]. Vì vậy code gọi syscall thô phải luôn sẵn sàng thử lại khi gặp `EINTR` (nhận xét của người soạn).

**Giải thích.**

*Với service `orders`.* Một handler thread của `orders` đang `read` trên socket, chờ client gửi tiếp. Đúng lúc đó `SIGTERM` đến và handler chạy. Khi handler xong, `read` của thread làm gì: chờ tiếp như chưa có chuyện gì, hay trả lỗi cho code của bạn? Câu trả lời quyết định bạn có phải viết vòng retry hay không.

### 5.1 Hai kết cục và ba nhóm call

Sau khi handler return, call đang bị ngắt có hai kết cục: tự khởi động lại, hoặc lỗi `EINTR`. Kết cục nào xảy ra tùy interface và tùy handler có được cài với `SA_RESTART` hay không[^sig-eintr]. `SA_RESTART` làm "một số" system call restartable qua signal, tương thích với ngữ nghĩa BSD[^sigaction-restart]. Danh sách trong man page chia thành ba nhóm.

**Nhóm 1: restart nếu có `SA_RESTART`, nếu không thì `EINTR`.**[^sig-restart-rule] Gồm:

- `read`/`write` trên thiết bị chậm, như terminal, pipe, socket[^sig-slowdev].
- Họ `wait`[^sig-restart-wait].
- `accept`/`connect`/`recv`/`send` trên socket, *trừ khi* socket đã đặt timeout[^sig-restart-list].

**Nhóm 2: không bao giờ restart dù có `SA_RESTART`, luôn `EINTR`.**[^sig-norestart-rule] Gồm:

- `epoll_wait`, `epoll_pwait`, `poll`, `ppoll`, `select`, `pselect`[^sig-norestart].
- Các call nhận dữ liệu (`accept`, `recv`…) trên socket đã đặt timeout `SO_RCVTIMEO`[^sig-norestart-sock].
- Các call chờ signal: `pause`, `sigsuspend`, `sigtimedwait`, `sigwaitinfo`[^sig-norestart-wait].
- Các hàm sleep như `nanosleep`[^sig-norestart-sleep].

**Nhóm 3: đã chuyển được một phần dữ liệu.** `read`/`write` trên thiết bị chậm mà đã chuyển được một ít dữ liệu thì khi bị ngắt sẽ trả *thành công* với số byte đã chuyển[^sig-partial]. Riêng I/O đĩa cục bộ không bị signal ngắt, vì đĩa không tính là thiết bị chậm[^sig-disk].

### 5.2 Thử: `read` trên pipe với và không có `SA_RESTART`

Đã chạy (C, container Linux aarch64): handler rỗng cho `SIGALRM`, `alarm(1)`, rồi `read` trên pipe rỗng. Ở trường hợp có `SA_RESTART`, một process con ghi một byte vào pipe sau 2 giây.

```text
SA_RESTART=0: read returned -1 errno=Interrupted system call
SA_RESTART=1: read returned 1 errno=-
```

Không có `SA_RESTART`, `read` bị cắt và trả `-1` với `EINTR`. Có `SA_RESTART`, `read` tự chạy lại sau khi handler xong và cuối cùng lấy được byte con ghi.

### 5.3 Hệ quả cho `orders`

`SA_RESTART` không phải thuốc chữa mọi thứ (nhận xét của người soạn). Event loop của `orders` mà dùng `epoll_wait` hay `poll` thì vẫn nhận `EINTR` dù có `SA_RESTART`[^sig-norestart], và phải tự gọi lại. Nên mọi chỗ gọi syscall thô cần một vòng retry-on-`EINTR`. Ngay cả `wait` cũng có thể trả `EINTR`[^wait-eintr].

<details><summary>Đi sâu: helper retry của std</summary>

std có helper `cvt_r` làm đúng việc này: gặp `-1` với `errno` là `EINTR` thì gọi lại[^rs-cvt-r]. Nó được dùng ở đâu thì xem [Chương 07, mục 2](07-rust-tren-giao-dien-he-thong.md).

</details>

**Quay lại Q4:** `EINTR` là cách kernel báo "call bị signal cắt ngang"; `SA_RESTART` chỉ che được một phần, nên vòng retry-on-`EINTR` vẫn cần.

---

## 6. Async-signal-safety: vì sao handler nguy hiểm (Q5)

> **Trả lời ngắn.** Handler chạy chen vào chương trình chính, nên có thể ngắt một hàm thư viện đang sửa dữ liệu dở dang. Nếu handler lại gọi chính hàm đó thì nó chạy trên dữ liệu chưa nhất quán và kết quả không đoán trước được[^safety-stdio]. Vì vậy handler chỉ nên gọi những hàm được đánh dấu *async-signal-safe*, tức gọi an toàn được từ trong handler[^safety-def].

**Giải thích.**

*Với service `orders`.* Handler `SIGTERM` đầu tiên người ta hay viết cho `orders` là ghi một dòng log "đang tắt", dọn dẹp rồi thoát. Nhưng handler chạy trên thread đang làm việc dở. Nếu lúc đó thread ấy đang ở giữa một hàm thư viện (in log, cấp phát bộ nhớ) mà handler lại gọi chính thư viện đó thì sao? Mục này trả lời câu hỏi đó và cho một cách viết handler an toàn.

### 6.1 Vì sao hàm thư viện không chịu được bị chen ngang

Man page lấy thư viện stdio làm ví dụ kinh điển: mọi hàm của nó đều không async-signal-safe. Khi làm buffered I/O, stdio phải giữ một buffer cấp phát tĩnh cùng các bộ đếm và con trỏ cho biết lượng dữ liệu và vị trí hiện tại[^safety-stdio-all]. Giả sử chương trình chính đang ở giữa `printf(3)` và mới cập nhật buffer cùng các biến liên quan một phần. Nếu lúc đó handler cũng gọi `printf`, lần gọi thứ hai chạy trên dữ liệu không nhất quán và kết quả không đoán trước được[^safety-stdio].

Từ đó có định nghĩa: hàm async-signal-safe là hàm gọi an toàn được từ trong handler, và nhiều hàm không như vậy, nhất là các hàm không reentrant[^safety-def]. Một hàm an toàn khi nó *reentrant* (gọi lại được lúc lần gọi trước chưa xong, vì không đụng trạng thái dùng chung đang dở), hoặc *atomic đối với signal* (handler không thể ngắt giữa chừng)[^safety-why].

### 6.2 Hai cách đối phó

Man page nêu hai cách. Cách (a): handler chỉ gọi hàm async-signal-safe *và* bản thân handler reentrant đối với biến toàn cục của chương trình chính. Cách (b): block signal ở chương trình chính mỗi khi nó gọi hàm không an toàn, hoặc đụng dữ liệu toàn cục mà handler cũng dùng[^safety-choices]. Cách (b) khó áp dụng với chương trình phức tạp, nên thực tế người ta chọn (a)[^safety-choice-a].

### 6.3 Hai chi tiết dễ bỏ sót

Chi tiết thứ nhất là `errno`. Đọc và ghi `errno` trong handler là async-signal-safe với điều kiện handler lưu `errno` lúc vào và khôi phục trước khi return[^safety-errno]. Nếu quên, handler có thể ghi đè `errno` của một system call mà chương trình chính đang chờ kết quả (nhận xét của người soạn).

Chi tiết thứ hai là `longjmp`. Giả sử handler ngắt một hàm không an toàn rồi thoát bằng `longjmp`/`siglongjmp`, mà sau đó chương trình lại gọi tiếp một hàm không an toàn: hành vi lúc đó là undefined[^safety-longjmp].

### 6.4 Mẫu handler tối thiểu

Handler của `orders` nên chỉ đặt một cờ hoặc ghi một byte vào pipe rồi return, vì `write(2)` có trong bảng hàm async-signal-safe[^safety-write] (nhận xét của người soạn). Việc thật, như ghi log, giải phóng tài nguyên, đóng kết nối, do vòng lặp chính làm khi nó thấy cờ hoặc thấy byte. Giới lập trình gọi cách ghi một byte vào pipe này là *self-pipe trick* (tên thông dụng; các nguồn đã tải không dùng tên này). Mục 9 cho thấy `signalfd` bỏ hẳn handler, và mục 10 cho thấy tokio làm đúng mẫu này.

<details><summary>Đi sâu: bảng hàm của POSIX, vì sao <code>printf</code>/<code>malloc</code> không có trong đó</summary>

POSIX.1 quy định một tập hàm mà bản cài đặt bắt buộc phải làm cho async-signal-safe[^safety-posix]. Bảng của man page có, chẳng hạn:

- `write(2)`[^safety-write]
- `_exit(2)`[^safety-exit]
- `sem_post(3)`[^safety-sem]
- `wait(2)` và `waitpid(2)`[^safety-wait]

`printf` và `malloc` **không** có trong bảng. Người soạn đã tìm văn bản man page bookworm (6.03): `malloc`/`free` không xuất hiện ở đâu cả, còn `printf` chỉ xuất hiện trong đoạn giải thích ví dụ, không nằm trong bảng. Với `printf`, man page còn nói thẳng: stdio được chọn làm ví dụ vì mọi hàm của nó đều không async-signal-safe[^safety-stdio-all].

Cần đọc bảng cho đúng mức. Bảng liệt kê các hàm POSIX *bắt buộc* async-signal-safe, và một bản cài đặt *có thể* cung cấp thêm hàm an toàn nhưng chuẩn không đòi hỏi[^safety-posix]. Vì vậy với `malloc`, kết luận có nguồn chỉ là "POSIX không bảo đảm". Còn "không an toàn" là suy luận của người soạn: `malloc` giữ cấu trúc dữ liệu chung của allocator, cùng loại vấn đề với buffer stdio ở trên. Trong code portable, hãy coi `malloc` là không an toàn.

</details>

**Quay lại Q5:** handler chạy chen giữa chương trình, nên chỉ hàm không dựa vào trạng thái dở dang (async-signal-safe) mới được gọi; cách an toàn nhất là chỉ đặt cờ hoặc ghi một byte.

---

## 7. `SIGCHLD` và `SIGPIPE` (Q6)

> **Trả lời ngắn.** `SIGCHLD` báo một process con vừa dừng hoặc kết thúc[^sig-tbl-chld]. Nó là signal chuẩn nên không xếp hàng[^sig-noqueue]: nhiều con chết gần nhau có thể chỉ để lại một `SIGCHLD` đang chờ, và handler chỉ `waitpid` một lần sẽ bỏ sót con. Vì vậy handler phải gọi `waitpid` lặp lại, mỗi lần với cờ `WNOHANG` (trả về ngay thay vì chờ), cho tới khi không còn con nào để thu[^wait-nohang] (nhận xét của người soạn, rút ra từ hai điều trên). Còn `SIGPIPE`: ghi vào pipe không còn ai đọc thì process nhận `SIGPIPE`, mặc định bị kết thúc[^sig-tbl-pipe]. Nếu process ignore nó thì `write` trả lỗi `EPIPE`[^pipe-sigpipe].

**Giải thích.**

*Với service `orders`.* `orders` sinh vài process con `pdf-gen` để render hóa đơn, và có handler thread ghi dữ liệu vào pipe tới một con. Hai chuyện có thể xảy ra mà bạn không chủ động: con chết (cha nhận `SIGCHLD`), và con đóng đầu đọc pipe trong khi cha vẫn ghi (cha nhận `SIGPIPE`). Mục này xem `orders` phải làm gì cho mỗi chuyện.

### 7.1 `SIGCHLD`: cha phải thu hoạch con

`SIGCHLD` báo một con vừa dừng hoặc kết thúc, và hành động mặc định của nó là `Ign`, tức bỏ qua[^sig-tbl-chld]. Con kết thúc mà cha chưa `wait` thì thành zombie, chiếm một slot trong bảng process của kernel[^wait-zombie]. Cơ chế zombie, orphan và việc `init` nhận con mồ côi nằm ở [Chương 03, mục 7](03-process.md).

Có hai cách để con không thành zombie. Cách thứ nhất là *bắt* `SIGCHLD` rồi gọi `wait`/`waitpid`. Hành vi lịch sử của BSD và System V về việc ignore `SIGCHLD` khác nhau, nên đây là cách portable duy nhất[^sigaction-ignchld]. Cách thứ hai là đặt disposition của `SIGCHLD` là `SIG_IGN`, hoặc dùng cờ `SA_NOCLDWAIT`: con kết thúc không thành zombie, nhưng `wait`/`waitpid` sẽ chặn tới khi mọi con kết thúc rồi lỗi `ECHILD`[^wait-sigign]. Nghĩa là cha không còn lấy được exit status của từng con (suy luận của người soạn). Man page cũng lưu ý riêng: dù disposition mặc định của `SIGCHLD` là "ignore", đặt tường minh `SIG_IGN` cho kết quả khác về zombie[^wait-sigign-default].

### 7.2 Bẫy: một signal, nhiều con chết

Đây là chỗ mục 3 trở thành chuyện thật. Vì signal chuẩn không queue[^sig-noqueue], ba `pdf-gen` chết gần nhau có thể chỉ cho **một** lần `SIGCHLD`. Handler chỉ `waitpid` một lần sẽ thu một con và bỏ sót hai zombie. Cách đúng là **lặp `waitpid(-1, ..., WNOHANG)` tới khi hết** (nhận xét của người soạn, rút ra từ hai điều trên). `WNOHANG` làm call trả về ngay nếu chưa có con nào kết thúc[^wait-nohang], nên vòng lặp dừng lại thay vì chặn.

Đã chạy: block `SIGCHLD`, tạo 3 con thoát ngay, đợi chúng chết, rồi unblock để handler chạy. Hai biến thể handler cho kết quả:

```text
single-waitpid handler: reaped inside handler=1, left as zombie after handler=2
loop handler: reaped inside handler=3, left as zombie after handler=0
```

Handler `waitpid` một lần thu 1 con và để lại 2 zombie. Handler lặp với `WNOHANG` thu đủ 3. Block rồi unblock ở đây chỉ để *tái hiện* việc gộp một cách xác định; ngoài thực tế, việc gộp phụ thuộc thời điểm các con chết (suy luận của người soạn).

### 7.3 `SIGPIPE`: ghi vào pipe không còn người đọc

Quay lại handler thread ghi vào pipe tới `pdf-gen`. Nếu mọi fd của đầu *đọc* pipe đã đóng thì `write(2)` làm kernel sinh `SIGPIPE` cho process gọi, còn nếu process đang ignore signal này thì `write` lỗi `EPIPE`[^pipe-sigpipe]. Pipe và EOF xem thêm ở [Chương 04, mục 8](04-file-descriptor-va-vfs.md). Mặc định `SIGPIPE` là `Term`[^sig-tbl-pipe], nên với một chương trình không ignore signal này, một `pdf-gen` chết sớm có thể kéo theo việc cha bị kết thúc (nhận xét của người soạn; runtime Rust xử lý khác, xem mục 10).

Đã chạy (C): con ghi vào pipe không còn reader.

```text
ign=0: child killed by signal 13
ign=1: child exit 0        (write returned -1, errno=Broken pipe)
```

Không ignore thì con chết vì signal 13; ignore thì `write` trả `-1` với `EPIPE`. (Output thật của thử nghiệm bị in lặp do buffer stdio bị `fork` nhân đôi: đó là lỗi của chương trình thử, không phải hành vi signal.)

Mục này chỉ khẳng định hành vi với **pipe**. Với socket (kết nối đã đóng) và cờ `MSG_NOSIGNAL`, xem [P6 — Socket API, mục 6](../06-networking/07-socket-va-kernel.md). Server chạy dài thường không muốn bị giết chỉ vì một kết nối hỏng, nên ignore `SIGPIPE` để lỗi hiện ra là `EPIPE` (nhận xét của người soạn).

<details><summary>Đi sâu: <code>SIGCHLD</code> và trạng thái con, <code>SA_NOCLDSTOP</code>, <code>Child</code> của Rust</summary>

Cách đọc exit status của con (`WIFEXITED`, `WEXITSTATUS`, `WIFSIGNALED`, `WTERMSIG`, và việc status chỉ giữ 8 bit thấp) nằm ở [Chương 03, mục 6](03-process.md). Hai chi tiết riêng cho `SIGCHLD`: mặc định `waitpid` chỉ đợi con *kết thúc*[^wait-options]. Còn cờ `SA_NOCLDSTOP` tắt `SIGCHLD` khi con dừng hoặc tiếp tục[^sigaction-nocldstop].

Phía Rust: std không tự `wait` con kể cả khi `Child` bị drop[^rs-child-zombie] ([Chương 07, mục 4](07-rust-tren-giao-dien-he-thong.md)).

</details>

**Quay lại Q6:** `SIGCHLD` có thể gộp nhiều cái chết thành một lần, nên handler phải lặp `waitpid(-1, WNOHANG)`; `SIGPIPE` bị ignore thì lỗi hiện ra ở `write` là `EPIPE`.

---

## 8. `SIGTERM` vs `SIGKILL`, graceful shutdown và PID 1 (Q7)

> **Trả lời ngắn.** `SIGTERM` là signal yêu cầu process kết thúc, nhưng process bắt (catch) được nó để dọn dẹp rồi mới thoát[^sigaction-desc]. `SIGKILL` thì không bắt, không chặn, không bỏ qua được nên process không có cơ hội dọn[^sig-uncatchable]. "Graceful shutdown" là quy ước của ứng dụng xây trên cơ chế đó chứ không phải tính năng của kernel (nhận xét của người soạn). Trong container, process PID 1 có luật riêng: signal gửi từ bên trong PID namespace chỉ tới được nó nếu nó đã cài handler[^pidns-onlyhandled].

**Giải thích.**

*Với service `orders`.* Khi Pod chứa `orders` bị dừng, `orders` cần xử lý nốt các request đang chạy, đóng kết nối, flush log rồi mới thoát; nếu bị giết giữa chừng thì đơn hàng đang ghi dở có thể hỏng. Câu hỏi của mục này: `orders` biết mình sắp bị dừng bằng cách nào, được bao nhiêu thời gian, và vì sao đôi khi `SIGTERM` có vẻ "không tới"?

### 8.1 `SIGTERM` và `SIGKILL`

`SIGTERM` mặc định là `Term`[^sig-tbl-term], và vì bắt được nên process có thể coi nó là lời yêu cầu: dọn dẹp rồi thoát có trật tự (nhận xét của người soạn). `SIGKILL` cũng mặc định là `Term`[^sig-tbl-kill]. Nhưng không ai đặt handler cho nó được[^sig-uncatchable], nên process không có cơ hội dọn dẹp (suy luận của người soạn).

Vì vậy graceful shutdown là **quy ước của ứng dụng xây trên cơ chế signal**, không phải tính năng của kernel (nhận xét của người soạn). Nội dung quy ước thường là: bắt `SIGTERM` (và thường cả `SIGINT`), ngừng nhận việc mới, chờ việc đang chạy xong trong hạn, đóng tài nguyên, rồi thoát.

### 8.2 Kubernetes dừng một Pod

Khi một Pod bị dừng có thứ tự, kubelet nhờ container runtime dừng các container bằng cách trước hết gửi `TERM` (`SIGTERM`), kèm một thời hạn (grace period), tới main process của mỗi container[^k8s-term]. Phần mô tả luồng dừng Pod nói rõ đó là process 1 bên trong container[^k8s-pid1]. Thời hạn mặc định là 30 giây[^k8s-grace-default]. Hết hạn mà còn process chạy thì runtime gửi `KILL` cho chúng[^k8s-kill]. Nhiều runtime còn tôn trọng `STOPSIGNAL` của image và gửi signal đó thay vì `TERM`[^k8s-stopsignal].

Với `orders`, điều này nghĩa là: coi `SIGTERM` là *lệnh bắt đầu* graceful shutdown và phải hoàn tất *trong grace period*; sau đó không còn cơ hội (nhận xét của người soạn).

### 8.3 PID 1 trong container

Có một chi tiết làm `SIGTERM` có thể "không tới" `orders`: nếu `orders` là process PID 1 trong container. Ta cần hiểu PID namespace trước. PID namespace cô lập không gian số PID, nên process ở namespace khác nhau có thể trùng PID[^pidns-isolate]. PID trong namespace mới bắt đầu từ 1[^pidns-start]. Process đầu tiên trong namespace là "init" của namespace đó và nhận nuôi các process mồ côi[^pidns-orphan] (chi tiết: [Chương 03, mục 7](03-process.md) và [P8 — Namespaces](../08-containers/02-namespaces.md)).

PID 1 có ba luật signal riêng:

1. Signal gửi từ *bên trong* namespace chỉ tới được init nếu init đã cài handler cho nó. Hạn chế này áp dụng cả với process đặc quyền, để chặn việc vô tình giết init[^pidns-onlyhandled]. Trên hệ thống thường cũng vậy: `kill(2)` ghi rằng chỉ những signal mà init đã cài handler mới gửi được tới process ID 1[^kill-init].
2. Signal gửi từ *namespace tổ tiên* theo cùng quy tắc, nhưng có ngoại lệ: `SIGKILL` và `SIGSTOP` được deliver cưỡng bức[^pidns-sigkill-ancestor]. Signal khác chỉ tới init khi init có handler cho nó[^pidns-ancestor].
3. Nếu init của namespace kết thúc, kernel kết thúc mọi process còn lại trong namespace bằng `SIGKILL`[^pidns-initdie].

### 8.4 Ghép lại: `SIGTERM` có tới được `orders` không

Từ luật 1, một PID 1 *không có handler* `SIGTERM` sẽ không bị `SIGTERM` từ bên trong namespace giết, còn có handler thì nhận bình thường (suy luận của người soạn từ luật 1).

Còn với Kubernetes, kubelet nhờ runtime gửi `SIGTERM` tới process 1 của container[^k8s-pid1]. Runtime nằm ngoài PID namespace của container, tức ở namespace tổ tiên (suy luận của người soạn), nên theo luật 2 signal chỉ tới được ứng dụng PID 1 nếu ứng dụng có handler. Không có handler thì container sống tới hết grace period rồi bị `SIGKILL`, và `SIGKILL` thì được deliver cưỡng bức[^pidns-sigkill-ancestor] (nhận xét của người soạn). [P8·08, mục 3](../08-containers/08-rust-trong-container.md) đo đúng trường hợp này với `docker stop`.

Vì vậy nếu `orders` làm PID 1 thì nó cần tự cài handler `SIGTERM`; hoặc cho container chạy một chương trình init nhỏ làm PID 1 để chuyển signal tới `orders`. PID 1 còn phải `wait` các con mồ côi được giao cho nó ([Chương 03, mục 7](03-process.md)). Cơ chế PID namespace đầy đủ (`--fork`, `/proc`, lồng nhau) nằm ở [P8 Chương 02, mục 4](../08-containers/02-namespaces.md).

<details><summary>Đi sâu: quyền gửi signal, <code>kill(pid, 0)</code> và thí nghiệm PID 1</summary>

**Quyền gửi signal.** Người gửi phải có đặc quyền (trên Linux là `CAP_KILL` trong user namespace của đích), hoặc real/effective UID của người gửi phải bằng real/saved set-user-ID của đích[^kill-perm]. Tham số `pid` dương nghĩa là gửi tới một process, `pid` bằng 0 nghĩa là gửi tới cả process group của người gọi[^kill-pid]. `kill(pid, 0)` không gửi gì nhưng vẫn kiểm tra tồn tại và quyền[^kill-sig0]. Man page nhắc rằng process "tồn tại" có thể là một zombie chưa được `wait`[^kill-zombie], nên `kill(pid, 0)` thành công không chứng minh process còn *chạy* (suy luận của người soạn). Về các loại ID và capability: [Chương 06](06-user-quyen-proc-sys.md).

**Thí nghiệm PID 1.** Đã chạy (C, trong container Linux aarch64 của lab, chương trình là PID 1 qua `exec`, một child gửi `kill(1, SIGTERM)` cho nó):

```text
my pid = 1
child: kill(1,TERM)=0
PID1: got SIGTERM in handler
PID1 still alive
```

Ở một lần chạy khác, *không đặt handler*, child gửi cả `SIGTERM` lẫn `SIGKILL`: cả hai `kill` trả `0` và PID 1 vẫn sống. Khớp với luật 1. Lưu ý `kill` trả `0` chỉ cho biết lời gọi được chấp nhận, không nói signal có tác dụng (suy luận của người soạn). Cả hai đều là **quan sát một lần chạy**.

</details>

**Quay lại Q7:** `SIGTERM` bắt được nên là chỗ để graceful shutdown, `SIGKILL` thì không; riêng PID 1 chỉ nhận signal mà nó có handler (trừ `SIGKILL`/`SIGSTOP` từ namespace tổ tiên).

---

## 9. `signalfd(2)`, `sigwaitinfo(2)` và signal với thread (Q8)

> **Trả lời ngắn.** Ngoài handler (chạy bất đồng bộ và bị ràng buộc bởi async-signal-safety, mục 6), Linux cho nhận signal *đồng bộ*: `sigwaitinfo` chặn thread gọi tới khi có signal[^sig-sync-wait]. Còn `signalfd` biến signal thành một fd để `read`[^sig-sync]. fd này dùng chung được với `poll`/`epoll`[^sfd-def], nên signal trở thành một nguồn sự kiện trong event loop. Với thread: disposition chung cả process, mask riêng từng thread[^sig-thread-mask].

**Giải thích.**

*Với service `orders`.* Giả sử `orders` có một event loop `epoll` canh các socket. Bạn muốn `SIGTERM` cũng hiện ra trong loop đó như một sự kiện bình thường, thay vì phải viết handler theo luật hạn chế của mục 6. Đó là việc của `signalfd`. Mục này cũng trả lời chuyện đã hứa ở mục 2 và 4: signal gửi cho cả process thì thread nào nên nhận?

### 9.1 Nhận signal đồng bộ

Handler là cách nhận signal bất đồng bộ: bạn phải tuân thủ async-signal-safety (mục 6), và handler không phải một fd để đưa vào `poll`/`epoll` (nhận xét của người soạn). Linux cho hai cách nhận signal *đồng bộ*, tức chủ động chờ tới khi signal được deliver rồi lấy thông tin về nó. Cách thứ nhất là họ `sigwaitinfo(2)`/`sigtimedwait(2)`/`sigwait(3)`: chúng chặn tới khi một signal trong tập đã chọn được deliver[^sig-sync-wait]. Cách thứ hai là `signalfd(2)`: nó trả về một fd để đọc thông tin về các signal được deliver cho caller, và mỗi `read` chặn tới khi có signal trong tập[^sig-sync].

### 9.2 Cơ chế `signalfd`

1. Block các signal muốn nhận bằng `sigprocmask(2)`, để chúng không bị xử lý theo disposition mặc định[^sfd-block].
2. Gọi `signalfd(-1, &mask, flags)`. Giá trị `-1` nghĩa là tạo fd mới gắn với tập signal; truyền một signalfd có sẵn thay cho `-1` thì thay tập signal của fd đó[^sfd-replace]. `SFD_NONBLOCK` đặt `O_NONBLOCK` ngay lúc tạo nên đỡ một lần `fcntl`, còn `SFD_CLOEXEC` đặt close-on-exec[^sfd-flags] (về `O_CLOEXEC`: [Chương 04, mục 4](04-file-descriptor-va-vfs.md)).
3. `read(2)` trả một hoặc nhiều `struct signalfd_siginfo` mô tả signal[^sfd-read-struct]. Mỗi struct dài 128 byte[^sfd-128]. Đọc là *tiêu thụ* signal: nó không còn pending, handler sẽ không chạy cho nó và `sigwaitinfo` cũng không nhận được[^sfd-read]. Nếu không có signal nào pending thì `read` chặn, hoặc trả `EAGAIN` nếu fd non-blocking[^sfd-read2].
4. fd đọc được (với `select`/`poll`) khi có signal trong tập đang pending[^sfd-readable]. Nó dùng được cả với `epoll`[^sfd-def]. Đây là lợi ích chính: signal thành một nguồn sự kiện như socket, nằm trong cùng event loop (xem [P7 — select/poll/epoll](../07-io-async/02-select-poll-epoll.md)).

Đã chạy: block `SIGUSR2`, tạo `signalfd`, `sigqueue` một lần, rồi `read` một `struct signalfd_siginfo`:

```text
signalfd read 128 bytes (sizeof=128) signo=12 pid_matches=1
```

`SIGUSR2` là số 12 ở cột x86/ARM của bảng số hiệu (mục 2)[^sig-num-usr2], và `ssi_pid` khớp PID của người gửi.

### 9.3 `sigwaitinfo` và thread

`sigwaitinfo` cùng ý tưởng với `signalfd` nhưng chặn ngay thread gọi. Trong chương trình đa thread, signal cần được block ở **mọi thread**, kẻo ở một thread không gọi `sigwaitinfo` nó bị xử lý theo disposition mặc định[^sigwait-mt]. Nếu nhiều thread cùng chờ một signal thì chỉ một thread nhận, và không xác định được là thread nào[^sigwait-multi].

### 9.4 Signal và thread nói chung

Gộp lại các luật ở mục 2 và 4: disposition theo process[^sig-perproc]. Mask theo thread[^sig-thread-mask]. Thread mới thừa kế mask của thread tạo ra nó[^pthsig-inherit]. Hệ quả là handler do bất kỳ thread nào đặt đều có hiệu lực cho cả process, và signal gửi cho process có thể rơi vào *bất kỳ* thread nào không block nó[^sig-directed]. Nếu không kiểm soát mask, handler của `orders` có thể chạy giữa lúc một handler thread đang giữ lock hoặc đang trong `malloc` (nhận xét của người soạn, từ mục 6 và mục 2).

Cách kiểm soát đã có sẵn trong ví dụ của `pthread_sigmask(3)`: block một số signal ở main thread, rồi tạo một thread riêng lấy chúng bằng `sigwait(3)`[^pthsig-example]. Thread tạo sau thừa kế mask đã block (mục 4), nên chỉ thread chuyên trách chờ bằng `sigwait` nhận chúng (suy luận của người soạn). Đây là mô hình thay thế handler, cùng họ với `signalfd`. Về thread nói chung: [P4 — Thread](../04-concurrency/01-thread.md).

<details><summary>Đi sâu: giới hạn của signalfd</summary>

- Không nhận được signal sinh *đồng bộ* như `SIGSEGV`, `SIGFPE`; chúng chỉ bắt được bằng handler[^sfd-limits]. `SIGKILL`/`SIGSTOP` bị lờ đi âm thầm[^sfd-kill].
- Signal phải được block, và mask được thừa kế qua `fork`/`exec`, nên child chạy chương trình khác thường phải unblock chúng trước `execve`[^sfd-exec]. Khi child do một hàm thư viện tự spawn ngầm thì bạn không làm được việc đó, và phải dùng handler ghi vào một fd (cùng ý tưởng với *self-pipe* ở mục 6)[^sfd-lib].
- Nếu cha đã thêm signalfd vào một `epoll` instance rồi `fork`, child thừa kế fd và `read` được signal gửi cho child[^sfd-fork]. Nhưng `epoll_wait` không báo fd đó sẵn sàng[^sfd-epoll].
- Trong chương trình đa thread, thread đọc signalfd nhận signal nhắm vào chính nó và signal nhắm vào cả process, không nhận signal nhắm vào thread khác[^sfd-thread].
- API **chỉ có trên Linux**[^sfd-linux]. Hệ BSD có filter `EVFILT_SIGNAL` của kqueue cho việc tương tự ([P7 — kqueue](../07-io-async/03-kqueue-io-uring.md); nguồn ở đó là man page FreeBSD, chưa kiểm chứng trên macOS).

</details>

**Quay lại Q8:** `signalfd` và `sigwaitinfo` đưa signal về dạng đồng bộ, tránh hạn chế của handler; vì mask theo thread còn disposition theo process, hãy block signal trước khi tạo thread và để một nơi duy nhất nhận.

---

## 10. Liên hệ với Rust

Với `orders` viết bằng Rust, các điều ở trên vẫn nguyên giá trị: Rust không đổi cách kernel gửi signal. Phần này nói runtime Rust đã làm sẵn gì quanh `SIGPIPE`, `Child::kill` khác graceful shutdown ra sao, và tokio xử lý signal thế nào.

**`SIGPIPE` trong Rust** (cơ chế đầy đủ, hai đường spawn và bẫy `println!`: [Chương 07, mục 5](07-rust-tren-giao-dien-he-thong.md)). Từ 2014, runtime Rust trên Unix đặt `SIGPIPE` thành `SIG_IGN` lúc khởi động[^rs-rt-sigpipe]. Lý do: nếu không thì trên một số nền tảng chương trình bị giết ngay khi gặp `EPIPE`[^rs-sigpipe-comment]. Hệ quả quan sát được (đã chạy, rustc 1.95.0, Linux): ghi vào một `std::io::pipe()` đã đóng reader trả `Err` thay vì giết process, với `kind=BrokenPipe raw=Some(32)`. Tuy vậy runtime init "không bảo đảm chạy" khi Rust code được gọi từ bên ngoài[^rs-init-notrun], nên đừng giả định `SIGPIPE` luôn bị ignore ở đó.

Con do `Command` spawn thì không thừa kế việc ignore này: std đặt lại `SIGPIPE` về `SIG_DFL` trong con trước khi `exec`, trừ khi dùng `-Zon-broken-pipe`[^rs-child-sigpipe-reset]. Code của std làm việc đó bằng lời gọi `signal(SIGPIPE, SIG_DFL)`[^rs-child-sigpipe-fork]. Việc ignore sống sót qua `execve` (mục 2) là một lý do dễ hiểu cho bước này, nhưng comment của std chỉ nói "tương thích ngược" (suy luận của người soạn).

**`Child::kill` là `SIGKILL`.** `Child::kill` buộc child thoát, và tương đương gửi `SIGKILL` trên Unix[^rs-child-kill]. Nên nó *không phải* graceful shutdown: nếu `orders` muốn dừng `pdf-gen` nhẹ nhàng thì phải gửi `SIGTERM` bằng `libc::kill` (FFI, cần `unsafe`) hoặc một thư viện bọc (nhận xét của người soạn). Việc `Child` không tự `wait`: [Chương 07, mục 4](07-rust-tren-giao-dien-he-thong.md).

**std không có API signal cấp cao.** Người soạn đã tìm văn bản trang chủ tài liệu `std` 1.95.0, gồm danh sách module và macro, và không có mục nào nhắc tới signal; đây là kết quả tìm kiếm, không phải trích dẫn. Bắt signal trong Rust nghĩa là FFI tới `sigaction`/`signalfd` qua crate `libc`, hoặc dùng crate bọc. Hàm extern gọi là `unsafe`[^rs-unsafe-extern] (xem [Chương 07, mục 7](07-rust-tren-giao-dien-he-thong.md)). Hạn chế async-signal-safety của mục 6 vẫn đúng với handler viết bằng Rust (suy luận của người soạn).

**tokio.** Tài liệu của `tokio::signal` cảnh báo rằng xử lý signal nói chung là một chủ đề rất rắc rối, cần dùng cẩn thận[^tokio-warn]. Trong source tokio 1.53.2, handler mức thấp làm đúng mẫu handler tối thiểu ở mục 6: đặt một cờ báo signal đã đến, rồi ghi một byte để đánh thức driver của tokio. Comment trong source nói cả hai thao tác phải async-signal-safe[^tokio-selfpipe]. Comment nhắc "pipe", nhưng code ghi byte vào một `UnixStream` ghép đôi (socket pair) do `UnixStream::pair()` tạo ra[^tokio-pair]; ý tưởng vẫn là self-pipe (nhận xét của người soạn).

Về hành vi: tài liệu `signal()` nêu rằng signal có thể bị gộp *nhiều hơn* mức kernel đã gộp[^tokio-coalesce]. Handler libc đã đăng ký *không bao giờ được gỡ*[^tokio-unregister]. Suy ra (suy luận của người soạn): sau khi đăng ký `SignalKind::terminate()`, `SIGTERM` không còn tự kết thúc process, và chương trình *phải* chủ động thoát. Dùng `tokio::signal` cho graceful shutdown: [P7·08 — Tokio](../07-io-async/08-tokio.md) và [P8·08, mục 4](../08-containers/08-rust-trong-container.md). Vòng retry `EINTR` (`cvt_r`) của std: [Chương 07, mục 2](07-rust-tren-giao-dien-he-thong.md).

---

## Tóm tắt

- Disposition theo process, mask theo thread; signal ở trạng thái pending giữa lúc phát sinh và lúc deliver.
- Kernel chạy handler khi đưa thread về user mode: dựng frame trên stack, thêm `sa_mask` và chính signal vào mask, rồi `sigreturn` khôi phục.
- Signal chuẩn không queue; real-time queue được, có thứ tự, mang được dữ liệu.
- Dùng `sigaction` và `pthread_sigmask`; block signal *trước* khi tạo thread hoặc process con.
- `SA_RESTART` không restart `epoll_wait`/`poll`/`select`/sleep: luôn retry khi `EINTR`.
- Handler chỉ gọi hàm async-signal-safe và lưu/khôi phục `errno`; tốt nhất chỉ đặt cờ hoặc ghi một byte.
- Handler `SIGCHLD` phải lặp `waitpid(-1, WNOHANG)`; `SIGPIPE` bị ignore thì `write` lỗi `EPIPE` (runtime Rust ignore sẵn).
- `SIGTERM` bắt được, `SIGKILL` không; graceful shutdown là quy ước của ứng dụng.
- PID 1 chỉ nhận signal nó có handler (trừ `SIGKILL`/`SIGSTOP` từ namespace tổ tiên).
- `signalfd` (chỉ Linux) và `sigwaitinfo` nhận signal đồng bộ, tránh handler.

## Câu hỏi tự kiểm tra

1. Disposition và signal mask thuộc process hay thread? Qua `fork` và `execve` chúng ra sao?
<details><summary>Đáp án</summary>

Disposition theo process[^sig-perproc]. Mask theo thread[^sig-thread-mask]. `fork` thừa kế cả hai. `execve` đặt lại handler về mặc định và giữ ignore[^sig-fork-exec]. Mask thì `execve` giữ nguyên[^sig-maskinherit]. Tập pending thì con `fork` bắt đầu rỗng, còn `execve` giữ nguyên[^sig-pendfork].

</details>

2. Gửi `SIGUSR1` 5 lần khi bị block rồi unblock: handler chạy mấy lần? Với `SIGRTMIN` qua `sigqueue` thì sao?
<details><summary>Đáp án</summary>

Signal chuẩn chạy 1 lần[^sig-noqueue]. Real-time chạy 5 lần[^sig-rt-queue].

</details>

3. Vì sao `printf` trong handler nguy hiểm, và có hai cách khắc phục nào?
<details><summary>Đáp án</summary>

stdio có buffer và biến trạng thái tĩnh; bị ngắt giữa chừng rồi gọi lại thì chạy trên dữ liệu không nhất quán[^safety-stdio]. Hai cách: (a) chỉ gọi hàm async-signal-safe và để handler reentrant, hoặc (b) block signal quanh chỗ không an toàn[^safety-choices]. Thực tế chọn (a)[^safety-choice-a].

</details>

4. `SA_RESTART` có bảo đảm `epoll_wait` không trả `EINTR`?
<details><summary>Đáp án</summary>

Không; `epoll_wait`, `poll`, `select` không bao giờ được restart[^sig-norestart].

</details>

5. Handler `SIGCHLD` gọi `waitpid` một lần mà vẫn còn zombie: vì sao, sửa sao?
<details><summary>Đáp án</summary>

Signal chuẩn không queue nên nhiều con chết có thể chỉ cho một `SIGCHLD`[^sig-noqueue]. Sửa bằng cách lặp `waitpid(-1, ..., WNOHANG)` tới hết[^wait-nohang].

</details>

6. Ghi vào pipe không còn reader: mặc định và khi ignore `SIGPIPE` xảy ra gì?
<details><summary>Đáp án</summary>

Mặc định process nhận `SIGPIPE` và (hành động `Term`) bị kết thúc[^sig-tbl-pipe]. Nếu ignore thì `write` lỗi `EPIPE`[^pipe-sigpipe].

</details>

7. Vì sao `signalfd` đi cùng `sigprocmask(SIG_BLOCK)`, và nó không nhận loại signal nào?
<details><summary>Đáp án</summary>

Để signal không bị xử lý theo disposition mặc định[^sfd-block]. Nó không nhận signal sinh đồng bộ như `SIGSEGV`/`SIGFPE`[^sfd-limits]. Nó cũng không nhận `SIGKILL`/`SIGSTOP`[^sfd-kill].

</details>

8. PID 1 trong PID namespace không có handler `SIGTERM`: `SIGTERM` từ trong namespace thì sao? `SIGKILL` từ namespace tổ tiên thì sao?
<details><summary>Đáp án</summary>

`SIGTERM` từ bên trong không tới được init[^pidns-onlyhandled]. `SIGKILL` từ namespace tổ tiên được deliver cưỡng bức[^pidns-sigkill-ancestor].

</details>

9. Kubernetes dừng một Pod: signal nào được gửi tới đâu, bao lâu thì bị `SIGKILL`, và ứng dụng nên làm gì với `SIGTERM`?
<details><summary>Đáp án</summary>

Kubelet nhờ container runtime gửi `TERM` kèm grace period tới main process của mỗi container[^k8s-term]. Đó là process 1 trong container[^k8s-pid1]. Mặc định là 30 giây[^k8s-grace-default]. Hết hạn thì gửi `KILL` cho các process còn chạy[^k8s-kill]. Ứng dụng nên coi `TERM` là lệnh bắt đầu graceful shutdown và hoàn tất trong grace period (nhận xét của người soạn).

</details>

10. Process nhiều thread: signal gửi cho cả process sẽ rơi vào thread nào, và làm sao để chỉ một thread xử lý nó?
<details><summary>Đáp án</summary>

Signal process-directed có thể tới bất kỳ thread nào không block nó; nếu nhiều thread cùng không block thì kernel chọn tùy ý[^sig-directed]. Cách làm: block signal ở mọi thread rồi để một thread chuyên trách chờ bằng `sigwait`/`sigwaitinfo`[^sigwait-mt]. Ví dụ của `pthread_sigmask(3)` làm đúng như vậy[^pthsig-example]. Hoặc đọc `signalfd` (suy luận của người soạn từ mục 4 và mục 9).

</details>

## Đọc thêm

- [signal(7)](https://manpages.debian.org/bookworm/manpages/signal.7.en.html) — mô hình đầy đủ, danh sách syscall bị/không bị restart.
- [signal-safety(7)](https://manpages.debian.org/bookworm/manpages/signal-safety.7.en.html) — bảng hàm async-signal-safe và ghi chú về `errno`, `longjmp`.
- [signalfd(2)](https://manpages.debian.org/bookworm/manpages-dev/signalfd.2.en.html) — ví dụ hoàn chỉnh và giới hạn với epoll/fork.
- [pid_namespaces(7)](https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html) — luật init và signal giữa namespace; tiếp tục ở [P8 — Namespaces](../08-containers/02-namespaces.md).
- [Kubernetes — Pod lifecycle](https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/) — luồng TERM rồi KILL khi Pod bị xóa.
- [P6 — Socket API, mục 6](../06-networking/07-socket-va-kernel.md) — `SIGPIPE` và `MSG_NOSIGNAL` khi ghi vào socket đã đóng.
- [P8·08 — Rust trong container, mục 3 và 4](../08-containers/08-rust-trong-container.md) — PID 1 không handler với số đo `docker stop`, và graceful shutdown bằng `tokio::signal`.

[^ostep-signals]: OSTEP, ch.5 Interlude: Process API — <https://pages.cs.wisc.edu/~remzi/OSTEP/cpu-api.pdf> — "The entire signals subsystem provides a rich infrastructure to deliver external events to processes, including ways to receive and process those signals within individual processes"
[^kill-send]: kill(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/kill.2.en.html> — "The kill() system call can be used to send any signal to any process group or process."
[^sig-disp]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "Each signal has a current disposition, which determines how the process behaves when it is delivered the signal."
[^sig-actions]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "Term Default action is to terminate the process. Ign Default action is to ignore the signal. Core Default action is to terminate the process and dump core (see core(5)). Stop Default action is to stop the process. Cont Default action is to continue the process if it is currently stopped."
[^sig-choose]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "A process can change the disposition of a signal using sigaction(2) or signal(2). (The latter is less portable when establishing a signal handler; see signal(2) for details.) Using these system calls, a process can elect one of the following behaviors to occur on delivery of the signal: perform the default action; ignore the signal; or catch the signal with a signal handler, a programmer-defined function that is automatically invoked when the signal is delivered."
[^sig-perproc]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "The signal disposition is a per-process attribute: in a multithreaded application, the disposition of a particular signal is the same for all threads."
[^sig-fork-exec]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "A child created via fork(2) inherits a copy of its parent's signal dispositions. During an execve(2), the dispositions of handled signals are reset to the default; the dispositions of ignored signals are left unchanged."
[^sig-uncatchable]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "The signals SIGKILL and SIGSTOP cannot be caught, blocked, or ignored."
[^sigaction-einval]: sigaction(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigaction.2.en.html> — "This will also be generated if an attempt is made to change the action for SIGKILL or SIGSTOP, which cannot be caught or ignored."
[^sig-tbl-kill]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "SIGKILL P1990 Term Kill signal"
[^sig-num-kill]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "SIGKILL 9 9 9 9"
[^sig-tbl-pipe]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "SIGPIPE P1990 Term Broken pipe: write to pipe with no readers; see pipe(7)"
[^sig-tbl-term]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "SIGTERM P1990 Term Termination signal"
[^sig-num-term]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "SIGTERM 15 15 15 15"
[^sig-tbl-chld]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "SIGCHLD P1990 Ign Child stopped or terminated"
[^sig-num-chld]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "SIGCHLD 17 20 18 18"
[^sig-num-usr2]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "SIGUSR2 12 31 17 17"
[^sig-num-arch]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "The numeric value for each signal is given in the table below. As shown in the table, many signals have different numeric values on different architectures. The first numeric value in each table row shows the signal number on x86, ARM, and most other architectures; the second value is for Alpha and SPARC; the third is for MIPS; and the last is for PARISC."
[^sig-pending]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "A signal may be blocked, which means that it will not be delivered until it is later unblocked. Between the time when it is generated and when it is delivered a signal is said to be pending."
[^sig-thread-mask]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "Each thread in a process has an independent signal mask, which indicates the set of signals that the thread is currently blocking."
[^sig-hw]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "A signal may be thread-directed because it was generated as a consequence of executing a specific machine-language instruction that triggered a hardware exception (e.g., SIGSEGV for an invalid memory access, or SIGFPE for a math error)"
[^sig-procdir]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "A process-directed signal is one that is targeted at (and thus pending for) the process as a whole. A signal may be process-directed because it was generated by the kernel for reasons other than a hardware exception, or because it was sent using kill(2) or sigqueue(3)."
[^sig-directed]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "A process-directed signal may be delivered to any one of the threads that does not currently have the signal blocked. If more than one of the threads has the signal unblocked, then the kernel chooses an arbitrary thread to which to deliver the signal."
[^sig-delivery-check]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "Whenever there is a transition from kernel-mode to user-mode execution (e.g., on return from a system call or scheduling of a thread onto the CPU), the kernel checks whether there is a pending unblocked signal for which the process has established a signal handler."
[^sig-frame]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "The signal is removed from the set of pending signals. (1.2) If the signal handler was installed by a call to sigaction(2) that specified the SA_ONSTACK flag and the thread has defined an alternate signal stack (using sigaltstack(2)), then that stack is installed. (1.3) Various pieces of signal-related context are saved into a special frame that is created on the stack."
[^sig-frame-list]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "The saved information includes: the program counter register (i.e., the address of the next instruction in the main program that should be executed when the signal handler returns); architecture-specific register state required for resuming the interrupted program; the thread's current signal mask; the thread's alternate signal stack settings."
[^sig-stack-default]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "By default, a signal handler is invoked on the normal process stack. It is possible to arrange that the signal handler uses an alternate stack; see sigaltstack(2) for a discussion of how to do this and when it might be useful."
[^sig-mask-during]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "The signal being delivered is also added to the signal mask, unless SA_NODEFER was specified when registering the handler. These signals are thus blocked while the handler executes."
[^sig-trampoline]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "The signal trampoline calls sigreturn(2), a system call that uses the information in the stack frame created in step 1 to restore the thread to its state before the signal handler was called. The thread's signal mask and alternate signal stack settings are restored as part of this procedure."
[^sig-resume]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "Upon completion of the call to sigreturn(2), the kernel transfers control back to user space, and the thread recommences execution at the point where it was interrupted by the signal handler."
[^sig-noqueue]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "Standard signals do not queue. If multiple instances of a standard signal are generated while that signal is blocked, then only one instance of the signal is marked as pending (and the signal will be delivered just once when it is unblocked)."
[^sig-siginfo-first]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "In the case where a standard signal is already pending, the siginfo_t structure (see sigaction(2)) associated with that signal is not overwritten on arrival of subsequent instances of the same signal. Thus, the process will receive the information associated with the first instance of the signal."
[^sig-rt-queue]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "Multiple instances of real-time signals can be queued. By contrast, if multiple instances of a standard signal are delivered while that signal is currently blocked, then only one instance is queued."
[^sig-order-std]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "If multiple standard signals are pending for a process, the order in which the signals are delivered is unspecified."
[^sig-rt-order]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "Real-time signals are delivered in a guaranteed order. Multiple real-time signals of the same type are delivered in the order they were sent."
[^sig-rt-sigqueue]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "If the signal is sent using sigqueue(3), an accompanying value (either an integer or a pointer) can be sent with the signal."
[^sig-rt-meaning]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "Unlike standard signals, real-time signals have no predefined meanings: the entire set of real-time signals can be used for application-defined purposes."
[^sig-rt-hard]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "programs should never refer to real-time signals using hard-coded numbers, but instead should always refer to real-time signals using the notation SIGRTMIN+n"
[^sig-rt-default]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "The default action for an unhandled real-time signal is to terminate the receiving process."
[^sig-rtlimit]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "In Linux 2.6.8, these /proc interfaces were replaced by the RLIMIT_SIGPENDING resource limit, which specifies a per-user limit for queued signals"
[^sigaction-desc]: sigaction(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigaction.2.en.html> — "The sigaction() system call is used to change the action taken by a process on receipt of a specific signal. (See signal(7) for an overview of signals.) signum specifies the signal and can be any valid signal except SIGKILL and SIGSTOP."
[^sigaction-handler]: sigaction(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigaction.2.en.html> — "SIG_DFL for the default action. SIG_IGN to ignore this signal. A pointer to a signal handling function. This function receives the signal number as its only argument."
[^sigaction-siginfo]: sigaction(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigaction.2.en.html> — "SA_SIGINFO (since Linux 2.2) The signal handler takes three arguments, not one."
[^sigaction-info-arg]: sigaction(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigaction.2.en.html> — "info A pointer to a siginfo_t, which is a structure containing further information about the signal, as described below."
[^sigaction-chld-info]: sigaction(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigaction.2.en.html> — "SIGCHLD fills in si_pid, si_uid, si_status, si_utime, and si_stime, providing information about the child."
[^sigaction-mask]: sigaction(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigaction.2.en.html> — "sa_mask specifies a mask of signals which should be blocked (i.e., added to the signal mask of the thread in which the signal handler is invoked) during execution of the signal handler. In addition, the signal which triggered the handler will be blocked, unless the SA_NODEFER flag is used."
[^sigaction-resethand]: sigaction(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigaction.2.en.html> — "SA_RESETHAND Restore the signal action to the default upon entry to the signal handler."
[^sigaction-query]: sigaction(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigaction.2.en.html> — "sigaction() can be called with a NULL second argument to query the current signal handler."
[^sigaction-hwignore]: sigaction(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigaction.2.en.html> — "According to POSIX, the behavior of a process is undefined after it ignores a SIGFPE, SIGILL, or SIGSEGV signal that was not generated by kill(2) or raise(3). Integer division by zero has undefined result. On some architectures it will generate a SIGFPE signal. (Also dividing the most negative integer by -1 may generate SIGFPE.) Ignoring this signal might lead to an endless loop."
[^sigaction-restart]: sigaction(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigaction.2.en.html> — "SA_RESTART Provide behavior compatible with BSD signal semantics by making certain system calls restartable across signals."
[^sigaction-ignchld]: sigaction(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigaction.2.en.html> — "Nevertheless, the historical BSD and System V behaviors for ignoring SIGCHLD differ, so that the only completely portable method of ensuring that terminated children do not become zombies is to catch the SIGCHLD signal and perform a wait(2) or similar."
[^sigaction-nocldstop]: sigaction(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigaction.2.en.html> — "SA_NOCLDSTOP If signum is SIGCHLD, do not receive notification when child processes stop (i.e., when they receive one of SIGSTOP, SIGTSTP, SIGTTIN, or SIGTTOU) or resume (i.e., they receive SIGCONT) (see wait(2)). This flag is meaningful only when establishing a handler for SIGCHLD."
[^sigaction-nocldwait]: sigaction(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigaction.2.en.html> — "If signum is SIGCHLD, do not transform children into zombies when they terminate."
[^sigprocmask-desc]: sigprocmask(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigprocmask.2.en.html> — "sigprocmask() is used to fetch and/or change the signal mask of the calling thread."
[^sigprocmask-block]: sigprocmask(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigprocmask.2.en.html> — "SIG_BLOCK The set of blocked signals is the union of the current set and the set argument."
[^sigprocmask-others]: sigprocmask(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigprocmask.2.en.html> — "SIG_UNBLOCK The signals in set are removed from the current set of blocked signals. It is permissible to attempt to unblock a signal which is not blocked. SIG_SETMASK The set of blocked signals is set to the argument set."
[^sigprocmask-query]: sigprocmask(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigprocmask.2.en.html> — "If set is NULL, then the signal mask is unchanged (i.e., how is ignored), but the current value of the signal mask is nevertheless returned in oldset (if it is not NULL)."
[^sigprocmask-silent]: sigprocmask(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigprocmask.2.en.html> — "It is not possible to block SIGKILL or SIGSTOP. Attempts to do so are silently ignored."
[^sigprocmask-hw]: sigprocmask(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigprocmask.2.en.html> — "If SIGBUS, SIGFPE, SIGILL, or SIGSEGV are generated while they are blocked, the result is undefined, unless the signal was generated by kill(2), sigqueue(3), or raise(3)."
[^sigprocmask-mt]: sigprocmask(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigprocmask.2.en.html> — "The use of sigprocmask() is unspecified in a multithreaded process; see pthread_sigmask(3)."
[^pthsig-same]: pthread_sigmask(3), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/pthread_sigmask.3.en.html> — "The pthread_sigmask() function is just like sigprocmask(2), with the difference that its use in multithreaded programs is explicitly specified by POSIX.1."
[^pthsig-inherit]: pthread_sigmask(3), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/pthread_sigmask.3.en.html> — "A new thread inherits a copy of its creator's signal mask."
[^pthsig-example]: pthread_sigmask(3), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/pthread_sigmask.3.en.html> — "The program below blocks some signals in the main thread, and then creates a dedicated thread to fetch those signals via sigwait(3)."
[^sig-maskinherit]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "A child created via fork(2) inherits a copy of its parent's signal mask; the signal mask is preserved across execve(2)."
[^sig-pendfork]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "A child created via fork(2) initially has an empty pending signal set; the pending signal set is preserved across an execve(2)."
[^safety-def]: signal-safety(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal-safety.7.en.html> — "An async-signal-safe function is one that can be safely called from within a signal handler. Many functions are not async-signal-safe. In particular, nonreentrant functions are generally unsafe to call from a signal handler."
[^safety-stdio]: signal-safety(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal-safety.7.en.html> — "Suppose that the main program is in the middle of a call to a stdio function such as printf(3) where the buffer and associated variables have been partially updated. If, at that moment, the program is interrupted by a signal handler that also calls printf(3), then the second call to printf(3) will operate on inconsistent data, with unpredictable results."
[^safety-why]: signal-safety(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal-safety.7.en.html> — "In general, a function is async-signal-safe either because it is reentrant or because it is atomic with respect to signals (i.e., its execution can't be interrupted by a signal handler)."
[^safety-choices]: signal-safety(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal-safety.7.en.html> — "(a) Ensure that (1) the signal handler calls only async-signal-safe functions, and (2) the signal handler itself is reentrant with respect to global variables in the main program. (b) Block signal delivery in the main program when calling functions that are unsafe or operating on global data that is also accessed by the signal handler."
[^safety-choice-a]: signal-safety(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal-safety.7.en.html> — "Generally, the second choice is difficult in programs of any complexity, so the first choice is taken."
[^safety-posix]: signal-safety(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal-safety.7.en.html> — "POSIX.1 specifies a set of functions that an implementation must make async-signal-safe. (An implementation may provide safe implementations of additional functions, but this is not required by the standard and other implementations may not provide the same guarantees.)"
[^safety-write]: signal-safety(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal-safety.7.en.html> — "wmemset(3) Added in POSIX.1-2008 TC2 write(2) Notes:"
[^safety-exit]: signal-safety(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal-safety.7.en.html> — "execve(2) _exit(2) _Exit(2) faccessat(2) Added in POSIX.1-2008"
[^safety-sem]: signal-safety(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal-safety.7.en.html> — "rmdir(2) select(2) sem_post(3) send(2) sendmsg(2) sendto(2) setgid(2)"
[^safety-wait]: signal-safety(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal-safety.7.en.html> — "wait(2) waitpid(2) wcpcpy(3) Added in POSIX.1-2008 TC2"
[^safety-errno]: signal-safety(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal-safety.7.en.html> — "Fetching and setting the value of errno is async-signal-safe provided that the signal handler saves errno on entry and restores its value before returning."
[^safety-longjmp]: signal-safety(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal-safety.7.en.html> — "If a signal handler interrupts the execution of an unsafe function, and the handler terminates via a call to longjmp(3) or siglongjmp(3) and the program subsequently calls an unsafe function, then the behavior of the program is undefined."
[^sig-eintr]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "If a signal handler is invoked while a system call or library function call is blocked, then either: the call is automatically restarted after the signal handler returns; or the call fails with the error EINTR. Which of these two behaviors occurs depends on the interface and whether or not the signal handler was established using the SA_RESTART flag (see sigaction(2))."
[^sig-restart-list]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "Socket interfaces: accept(2), connect(2), recv(2), recvfrom(2), recvmmsg(2), recvmsg(2), send(2), sendto(2), and sendmsg(2), unless a timeout has been set on the socket (see below)."
[^sig-restart-rule]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "If a blocked call to one of the following interfaces is interrupted by a signal handler, then the call is automatically restarted after the signal handler returns if the SA_RESTART flag was used; otherwise the call fails with the error EINTR:"
[^sig-slowdev]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "read(2), readv(2), write(2), writev(2), and ioctl(2) calls on "slow" devices. A "slow" device is one where the I/O call may block for an indefinite time, for example, a terminal, pipe, or socket."
[^sig-restart-wait]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "wait(2), wait3(2), wait4(2), waitid(2), and waitpid(2)."
[^sig-norestart]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "File descriptor multiplexing interfaces: epoll_wait(2), epoll_pwait(2), poll(2), ppoll(2), select(2), and pselect(2)."
[^sig-norestart-rule]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "The following interfaces are never restarted after being interrupted by a signal handler, regardless of the use of SA_RESTART; they always fail with the error EINTR when interrupted by a signal handler:"
[^sig-norestart-wait]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "Interfaces used to wait for signals: pause(2), sigsuspend(2), sigtimedwait(2), and sigwaitinfo(2)."
[^sig-norestart-sock]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — ""Input" socket interfaces, when a timeout (SO_RCVTIMEO) has been set on the socket using setsockopt(2): accept(2), recv(2), recvfrom(2), recvmmsg(2) (also with a non-NULL timeout argument), and recvmsg(2)."
[^sig-norestart-sleep]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "Sleep interfaces: clock_nanosleep(2), nanosleep(2), and usleep(3)."
[^sig-partial]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "If an I/O call on a slow device has already transferred some data by the time it is interrupted by a signal handler, then the call will return a success status (normally, the number of bytes transferred)."
[^sig-disk]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "Note that a (local) disk is not a slow device according to this definition; I/O operations on disk devices are not interrupted by signals."
[^wait-eintr]: wait(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/wait.2.en.html> — "EINTR WNOHANG was not set and an unblocked signal or a SIGCHLD was caught; see signal(7)."
[^wait-zombie]: wait(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/wait.2.en.html> — "As long as a zombie is not removed from the system via a wait, it will consume a slot in the kernel process table, and if this table fills, it will not be possible to create further processes."
[^wait-sigign-default]: wait(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/wait.2.en.html> — "Note that even though the default disposition of SIGCHLD is "ignore", explicitly setting the disposition to SIG_IGN results in different treatment of zombie process children."
[^wait-sigign]: wait(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/wait.2.en.html> — "POSIX.1-2001 specifies that if the disposition of SIGCHLD is set to SIG_IGN or the SA_NOCLDWAIT flag is set for SIGCHLD (see sigaction(2)), then children that terminate do not become zombies and a call to wait() or waitpid() will block until all children have terminated, and then fail with errno set to ECHILD."
[^wait-nohang]: wait(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/wait.2.en.html> — "WNOHANG return immediately if no child has exited."
[^wait-options]: wait(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/wait.2.en.html> — "By default, waitpid() waits only for terminated children, but this behavior is modifiable via the options argument, as described below."
[^rs-child-zombie]: Rust std 1.95.0, `std::process::Child` — <https://doc.rust-lang.org/1.95.0/std/process/struct.Child.html> — "The standard library does not automatically wait on child processes (not even if the Child is dropped), it is up to the application developer to do so."
[^pipe-sigpipe]: pipe(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/pipe.7.en.html> — "If all file descriptors referring to the read end of a pipe have been closed, then a write(2) will cause a SIGPIPE signal to be generated for the calling process. If the calling process is ignoring this signal, then write(2) fails with the error EPIPE."
[^kill-perm]: kill(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/kill.2.en.html> — "For a process to have permission to send a signal, it must either be privileged (under Linux: have the CAP_KILL capability in the user namespace of the target process), or the real or effective user ID of the sending process must equal the real or saved set-user-ID of the target process."
[^kill-sig0]: kill(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/kill.2.en.html> — "If sig is 0, then no signal is sent, but existence and permission checks are still performed"
[^kill-pid]: kill(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/kill.2.en.html> — "If pid is positive, then signal sig is sent to the process with the ID specified by pid. If pid equals 0, then sig is sent to every process in the process group of the calling process."
[^kill-zombie]: kill(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/kill.2.en.html> — "Note that an existing process might be a zombie, a process that has terminated execution, but has not yet been wait(2)ed for."
[^kill-init]: kill(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/kill.2.en.html> — "The only signals that can be sent to process ID 1, the init process, are those for which init has explicitly installed signal handlers."
[^k8s-term]: Kubernetes docs, Pod Lifecycle (Termination of Pods) — <https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/> — "kubelet makes requests to the container runtime to attempt to stop the containers in the pod by first sending a TERM (aka. SIGTERM) signal, with a grace period timeout, to the main process in each container."
[^k8s-kill]: Kubernetes docs, Pod Lifecycle (Termination of Pods) — <https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/> — "Once the grace period has expired, the KILL signal is sent to any remaining processes, and the Pod is then deleted from the API Server."
[^k8s-pid1]: Kubernetes docs, Pod Lifecycle (Pod Termination Flow) — <https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/> — "The kubelet triggers the container runtime to send a TERM signal to process 1 inside each container."
[^k8s-grace-default]: Kubernetes docs, Pod Lifecycle (Pod Termination Flow) — <https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/> — "The default terminationGracePeriodSeconds setting is 30 seconds."
[^k8s-stopsignal]: Kubernetes docs, Pod Lifecycle (Termination of Pods) — <https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/> — "Many container runtimes respect the STOPSIGNAL value defined in the container image and, if different, send the container image configured STOPSIGNAL instead of TERM."
[^sig-sync-wait]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "it is possible to synchronously accept the signal, that is, to block execution until the signal is delivered, at which point the kernel returns information about the signal to the caller. There are two general ways to do this: sigwaitinfo(2), sigtimedwait(2), and sigwait(3) suspend execution until one of the signals in a specified set is delivered."
[^sig-sync]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "signalfd(2) returns a file descriptor that can be used to read information about signals that are delivered to the caller. Each read(2) from this file descriptor blocks until one of the signals in the set specified in the signalfd(2) call is delivered to the caller."
[^sfd-def]: signalfd(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/signalfd.2.en.html> — "This provides an alternative to the use of a signal handler or sigwaitinfo(2), and has the advantage that the file descriptor may be monitored by select(2), poll(2), and epoll(7)."
[^sfd-block]: signalfd(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/signalfd.2.en.html> — "Normally, the set of signals to be received via the file descriptor should be blocked using sigprocmask(2), to prevent the signals being handled according to their default dispositions."
[^sfd-flags]: signalfd(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/signalfd.2.en.html> — "SFD_NONBLOCK Set the O_NONBLOCK file status flag on the open file description (see open(2)) referred to by the new file descriptor. Using this flag saves extra calls to fcntl(2) to achieve the same result. SFD_CLOEXEC Set the close-on-exec (FD_CLOEXEC) flag on the new file descriptor."
[^sfd-replace]: signalfd(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/signalfd.2.en.html> — "If the fd argument is -1, then the call creates a new file descriptor and associates the signal set specified in mask with that file descriptor. If fd is not -1, then it must specify a valid existing signalfd file descriptor, and mask is used to replace the signal set associated with that file descriptor."
[^sfd-read-struct]: signalfd(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/signalfd.2.en.html> — "If one or more of the signals specified in mask is pending for the process, then the buffer supplied to read(2) is used to return one or more signalfd_siginfo structures (see below) that describe the signals."
[^sfd-read]: signalfd(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/signalfd.2.en.html> — "As a consequence of the read(2), the signals are consumed, so that they are no longer pending for the process (i.e., will not be caught by signal handlers, and cannot be accepted using sigwaitinfo(2))."
[^sfd-read2]: signalfd(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/signalfd.2.en.html> — "If none of the signals in mask is pending for the process, then the read(2) either blocks until one of the signals in mask is generated for the process, or fails with the error EAGAIN if the file descriptor has been made nonblocking."
[^sfd-readable]: signalfd(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/signalfd.2.en.html> — "The file descriptor is readable (the select(2) readfds argument; the poll(2) POLLIN flag) if one or more of the signals in mask is pending for the process."
[^sfd-limits]: signalfd(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/signalfd.2.en.html> — "The signalfd mechanism can't be used to receive signals that are synchronously generated, such as the SIGSEGV signal that results from accessing an invalid memory address or the SIGFPE signal that results from an arithmetic error. Such signals can be caught only via signal handler."
[^sfd-kill]: signalfd(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/signalfd.2.en.html> — "It is not possible to receive SIGKILL or SIGSTOP signals via a signalfd file descriptor; these signals are silently ignored if specified in mask."
[^sfd-exec]: signalfd(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/signalfd.2.en.html> — "If spawning a child process to execute a helper program (that does not need the signalfd file descriptor), then, after the call to fork(2), you will normally want to unblock those signals before calling execve(2), so that the helper program can see any signals that it expects to see."
[^sfd-lib]: signalfd(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/signalfd.2.en.html> — "In such cases, one must fall back to using a traditional signal handler that writes to a file descriptor monitored by select(2), poll(2), or epoll(7)."
[^sfd-fork]: signalfd(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/signalfd.2.en.html> — "After a fork(2), the child inherits a copy of the signalfd file descriptor. A read(2) from the file descriptor in the child will return information about signals queued to the child."
[^sfd-epoll]: signalfd(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/signalfd.2.en.html> — "then the child will be able to read(2) signals that are sent to it using the signalfd file descriptor, but epoll_wait(2) will not indicate that the signalfd file descriptor is ready."
[^sfd-thread]: signalfd(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/signalfd.2.en.html> — "when a thread reads from a signalfd file descriptor, it will read the signals that are directed to the thread itself and the signals that are directed to the process (i.e., the entire thread group)."
[^sfd-linux]: signalfd(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/signalfd.2.en.html> — "signalfd() and signalfd4() are Linux-specific."
[^sigwait-mt]: sigwaitinfo(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigwaitinfo.2.en.html> — "In a multithreaded program, the signal should be blocked in all threads, in order to prevent the signal being treated according to its default disposition in a thread other than the one calling sigwaitinfo() or sigtimedwait())."
[^sigwait-multi]: sigwaitinfo(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/sigwaitinfo.2.en.html> — "If multiple threads of a process are blocked waiting for the same signal(s) in sigwaitinfo() or sigtimedwait(), then exactly one of the threads will actually receive the signal if it becomes pending for the process as a whole; which of the threads receives the signal is indeterminate."
[^rs-rt-sigpipe]: rust-lang/rust tag 1.95.0, library/std/src/rt.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/rt.rs> — "Since 2014, the Rust runtime on Unix has set the `SIGPIPE` handler to `SIG_IGN`."
[^rs-sigpipe-comment]: rust-lang/rust tag 1.95.0, library/std/src/sys/pal/unix/mod.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/pal/unix/mod.rs> — "By default, some platforms will send a *signal* when an EPIPE error would otherwise be delivered. This runtime doesn't install a SIGPIPE handler, causing it to kill the program, which isn't exactly what we want! Hence, we set SIGPIPE to ignore when the program starts up in order to prevent this problem."
[^rs-init-notrun]: rust-lang/rust tag 1.95.0, library/std/src/rt.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/rt.rs> — "NOTE: this is not guaranteed to run, for example when Rust code is called externally."
[^rs-child-sigpipe-reset]: rust-lang/rust tag 1.95.0, library/std/src/sys/process/unix/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/process/unix/unix.rs> — "If -Zon-broken-pipe is not used, reset SIGPIPE to SIG_DFL for backward compatibility."
[^rs-child-sigpipe-fork]: rust-lang/rust tag 1.95.0, library/std/src/sys/process/unix/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/process/unix/unix.rs> — "let ret = sys::signal(libc::SIGPIPE, libc::SIG_DFL);"
[^rs-child-kill]: Rust std 1.95.0, `std::process::Child` — <https://doc.rust-lang.org/1.95.0/std/process/struct.Child.html> — "This is equivalent to sending a SIGKILL on Unix platforms."
[^rs-unsafe-extern]: The Rust Programming Language 1.95.0, ch.20.1 Unsafe Rust — <https://doc.rust-lang.org/1.95.0/book/ch20-01-unsafe-rust.html> — "Functions declared within extern blocks are generally unsafe to call from Rust code, so extern blocks must also be marked unsafe."
[^tokio-warn]: tokio 1.53.2, `tokio::signal` — <https://docs.rs/tokio/1.53.2/tokio/signal/index.html> — "Note that signal handling is in general a very tricky topic and should be used with great care."
[^tokio-selfpipe]: tokio-rs/tokio tag tokio-1.53.2, tokio/src/signal/unix.rs — <https://raw.githubusercontent.com/tokio-rs/tokio/tokio-1.53.2/tokio/src/signal/unix.rs> — "Flag that our specific signal was received (e.g. store an atomic flag) 2. Wake up the driver by writing a byte to a pipe Those two operations should both be async-signal safe."
[^tokio-pair]: tokio-rs/tokio tag tokio-1.53.2, tokio/src/signal/unix.rs — <https://raw.githubusercontent.com/tokio-rs/tokio/tokio-1.53.2/tokio/src/signal/unix.rs> — "let (receiver, sender) = UnixStream::pair().expect("failed to create UnixStream");"
[^tokio-coalesce]: tokio 1.53.2, `tokio::signal::unix::signal` — <https://docs.rs/tokio/1.53.2/tokio/signal/unix/fn.signal.html> — "Signals may be coalesced beyond what the kernel already does."
[^tokio-unregister]: tokio 1.53.2, `tokio::signal::unix::signal` — <https://docs.rs/tokio/1.53.2/tokio/signal/unix/fn.signal.html> — "Once a signal handler is registered with the process the underlying libc signal handler is never unregistered."
[^pidns-isolate]: pid_namespaces(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "PID namespaces isolate the process ID number space, meaning that processes in different PID namespaces can have the same PID."
[^pidns-start]: pid_namespaces(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "PIDs in a new PID namespace start at 1, somewhat like a standalone system"
[^pidns-orphan]: pid_namespaces(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "has the PID 1, and is the "init" process for the namespace (see init(1)). This process becomes the parent of any child processes that are orphaned because a process that resides in this PID namespace terminated"
[^pidns-onlyhandled]: pid_namespaces(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "Only signals for which the "init" process has established a signal handler can be sent to the "init" process by other members of the PID namespace. This restriction applies even to privileged processes, and prevents other members of the PID namespace from accidentally killing the "init" process."
[^pidns-sigkill-ancestor]: pid_namespaces(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "SIGKILL or SIGSTOP are treated exceptionally: these signals are forcibly delivered when sent from an ancestor PID namespace."
[^pidns-ancestor]: pid_namespaces(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "send signals to the "init" process of a child PID namespace only if the "init" process has established a handler for that signal."
[^pidns-initdie]: pid_namespaces(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "If the "init" process of a PID namespace terminates, the kernel terminates all of the processes in the namespace via a SIGKILL signal."
[^sig-tgkill]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "or because it was targeted at a specific thread using interfaces such as tgkill(2) or pthread_kill(3)."
[^sig-rt-order2]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "If different real-time signals are sent to a process, they are delivered starting with the lowest-numbered signal."
[^sig-rt-range]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "The Linux kernel supports a range of 33 different real-time signals, numbered 32 to 64. However, the glibc POSIX threads implementation internally uses two (for NPTL) or three (for LinuxThreads) real-time signals (see pthreads(7)), and adjusts the value of SIGRTMIN suitably (to 34 or 35)."
[^sig-signal-portable]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "(The latter is less portable when establishing a signal handler; see signal(2) for details.)"
[^safety-stdio-all]: signal-safety(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal-safety.7.en.html> — "The kinds of issues that render a function unsafe can be quickly understood when one considers the implementation of the stdio library, all of whose functions are not async-signal-safe. When performing buffered I/O on a file, the stdio functions must maintain a statically allocated data buffer along with associated counters and indexes (or pointers) that record the amount of data and the current position in the buffer."
[^sfd-128]: signalfd(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/signalfd.2.en.html> — "Pad size to 128 bytes (allow for additional fields in the future)"
[^rs-cvt-r]: rust-lang/rust tag 1.95.0, library/std/src/sys/pal/unix/mod.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/pal/unix/mod.rs> — "`-1` → look at `errno` → retry on `EINTR`. Otherwise `Ok()`-wrap the closure return value."
