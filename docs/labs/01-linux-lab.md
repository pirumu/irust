# Lab P1 — Linux và system call interface

Đây là phần thực hành của P1. Lý thuyết nằm ở các chương của [P1 · Linux và system interface](../01-linux/00-tong-quan.md); mỗi bài lab dưới đây ghi rõ chương và mục cần đọc trước. Hãy đọc xong lý thuyết trước, vì các giải thích dưới đây dẫn lại khái niệm (user/kernel mode, system call, fd và open file description, pipe, signal, `/proc`) mà các chương đó trình bày.

> Output lab trong tài liệu là output **đã chạy thật** trong container Linux của repo (Debian bookworm, Rust 1.95, **aarch64**); PID, số inode, địa chỉ, dung lượng sẽ khác trên máy bạn. Mọi output đều lấy từ container; không có output nào chạy trên macOS (ở đó binary chỉ in hướng dẫn dùng `./labs/linux.sh`). Footnote đã được kiểm bằng `tools/verify_citations.py`. Quy ước chú thích như ở [00-roadmap.md](../00-roadmap.md).

## 1. Lab

Crate: `labs/crates/lab-linux`, 6 binary, chỉ có nghĩa trên Linux (trên macOS `cargo build` vẫn qua, chạy binary chỉ in hướng dẫn dùng `./labs/linux.sh`). Phụ thuộc duy nhất: `libc = "0.2"` (để gọi `fork`, `pipe`, `dup2`, `waitpid`, `write`, `syscall` trực tiếp). Mọi lệnh chạy từ thư mục gốc repo.

```bash
./labs/linux.sh cargo build -p lab-linux          # build trong container
./labs/linux.sh bash -c 'cargo build -p lab-linux && strace ... ./target-linux/debug/<bin>'
```

### 1.1 Lab (a) — `fileio`: mở, ghi, đọc, đóng

Lý thuyết: [chương 02 mục 3](../01-linux/02-system-call.md) (wrapper libc và tên syscall), [chương 04 mục 5](../01-linux/04-file-descriptor-va-vfs.md) (fd là số nhỏ nhất chưa dùng), [chương 07 mục 1](../01-linux/07-rust-tren-giao-dien-he-thong.md) (`OwnedFd`/`Drop`) và [mục 2](../01-linux/07-rust-tren-giao-dien-he-thong.md) (`io::Error`/`errno`).

Mã: `src/bin/fileio.rs`. Mở file bằng `OpenOptions`, in `as_raw_fd()`, ghi, `seek`, đọc lại, mở lần hai, ép về `OwnedFd`, thả, rồi cố mở file không tồn tại để xem `errno`.

```bash
./labs/linux.sh bash -c 'cargo build -p lab-linux -q && ./target-linux/debug/fileio'
```

```text
[fileio] open /tmp/p01-fileio.txt -> fd 3
[fileio] read back: hello from p01
[fileio] as_fd() = BorrowedFd { fd: 3 }
[fileio] second open -> fd 4
[fileio] dropping OwnedFd 4
[fileio] end of scope: File (fd 3) is dropped
[fileio] open error: No such file or directory (os error 2) | raw_os_error = Some(2) | kind = NotFound
[fileio] reopen -> fd 3
```

Giải thích: fd đầu tiên là **3** vì 0/1/2 đã bị stdio chiếm và `open` trả số nhỏ nhất chưa dùng[^open-lowest]. Lần mở thứ hai nhận 4. Sau khi cả hai bị drop, lần `reopen` lại nhận 3 — bằng chứng fd được tái sử dụng sau `close`[^close-desc]. `os error 2` là `ENOENT`, `kind = NotFound`.

Giờ xem syscall thật:

```bash
./labs/linux.sh bash -c 'strace -e trace=openat,read,write,close ./target-linux/debug/fileio 2>&1 | grep -E "p01-fileio|hello|definitely|close\((3|4)\)" | grep -v "^write(1\|^\[fileio"'
```

```text
openat(AT_FDCWD, "/tmp/p01-fileio.txt", O_RDWR|O_CREAT|O_TRUNC|O_CLOEXEC, 0666) = 3
write(3, "hello from p01\n", 15)        = 15
read(3, "hello from p01\n", 15)         = 15
openat(AT_FDCWD, "/tmp/p01-fileio.txt", O_RDONLY|O_CLOEXEC) = 4
close(4)                                = 0
close(3)                                = 0
openat(AT_FDCWD, "/definitely/not/here", O_RDONLY|O_CLOEXEC) = -1 ENOENT (No such file or directory)
openat(AT_FDCWD, "/tmp/p01-fileio.txt", O_RDONLY|O_CLOEXEC) = 3
close(3)                                = 0
```

(Rút gọn: bỏ bốn dòng `close(3)` ở đầu — chúng đến từ bước nạp thư viện trước `main`, không phải từ code lab.) Lưu ý strace chạy trên aarch64; trên x86-64 phần lớn dòng giống nhau nhưng có thể khác ở vài syscall.

Đọc output:

- Rust `OpenOptions::open` → `openat(AT_FDCWD, ..., O_CLOEXEC, 0666)`. Source std: `File::open_c` dựng `flags` bắt đầu từ `O_CLOEXEC`[^rs-fs-cloexec] rồi gọi `open64(path, flags, mode)` bọc trong `cvt_r`[^rs-fs-open]. `openat` với `AT_FDCWD` hoạt động giống `open`, khác ở cách hiểu đường dẫn tương đối[^openat-same] — nên ở đây tên hàm C (`open64`) khác tên syscall (`openat`), đúng như [chương 02 mục 3](../01-linux/02-system-call.md).
- `write(3, ..., 15) = 15`: ghi 15 byte vào fd 3. Phía std, `write` của fd gọi `libc::write(self.as_raw_fd(), ...)` rồi `cvt`[^rs-fd-write].
- Lần `openat` thất bại in `= -1 ENOENT`: đây là *giá trị trả về của wrapper* (-1) kèm `errno` mà strace giải mã. Std biến nó thành `io::Error` qua `last_os_error`; `cvt` trong std làm đúng việc này: nếu trả -1 thì `Err(io::Error::last_os_error())`[^rs-cvt].
- `close(4)` rồi `close(3)` đến từ `Drop`: std gọi `libc::close` và bỏ qua lỗi[^rs-drop-close].

**Bài tập mở rộng (a):** (1) thêm `-e trace=fcntl,lseek` để thấy `seek(Start(0))` thành syscall nào; (2) dùng `BufWriter` bọc `File` và ghi 1000 lần 1 byte, đếm số `write` trong strace so với không bọc — đối chiếu với câu "File does not buffer"[^rs-file]; (3) chạy `ulimit -n 8` trong shell rồi mở nhiều file đến khi lỗi, ghi lại `errno` (đọc trước [chương 06 mục 6](../01-linux/06-user-quyen-proc-sys.md) về `RLIMIT_NOFILE` và `EMFILE`/`ENFILE`).

### 1.2 Lab (b) — `procfs`: `/proc/self/fd`, `status`, `maps`

Lý thuyết: [chương 06 mục 4](../01-linux/06-user-quyen-proc-sys.md) (chẩn đoán process bằng `/proc/<pid>`), [chương 04 mục 1](../01-linux/04-file-descriptor-va-vfs.md) (fd → open file description → inode) và [mục 6](../01-linux/04-file-descriptor-va-vfs.md) (VFS, "everything is a file").

Mã: `src/bin/procfs.rs`. Giữ mở một pipe (`io::pipe`), một `TcpListener`, một file, rồi liệt kê `/proc/self/fd`, in vài dòng `status` và vài vùng `maps`.

```bash
./labs/linux.sh bash -c 'cargo build -p lab-linux -q && ./target-linux/debug/procfs'
```

```text
[procfs] std::process::id() = 15
[procfs] /proc/self/fd:
    0 -> pipe:[37281]
    1 -> pipe:[37282]
    2 -> pipe:[37282]
    3 -> pipe:[38475]
    4 -> pipe:[38475]
    5 -> socket:[38476]
    6 -> /etc/hostname
    7 -> /proc/15/fd
[procfs] /proc/self/status (selected fields):
  Name:	procfs
  State:	R (running)
  Pid:	15
  PPid:	1
  Uid:	0	0	0	0
  Gid:	0	0	0	0
  FDSize:	64
  VmRSS:	    1712 kB
  Threads:	1
[procfs] /proc/self/maps (binary, [heap], [stack], [vdso]):
  b963ae090000-b963ae0f2000 r-xp 00000000 00:25 1552                       /work/labs/target-linux/debug/procfs
  b963b12c7000-b963b12e8000 rw-p 00000000 00:00 0                          [heap]
  fd3db669a000-fd3db669b000 r-xp 00000000 00:00 0                          [vdso]
  ffffc3732000-ffffc3753000 rw-p 00000000 00:00 0                          [stack]
```

Giải thích:

- Mục 0/1/2 là stdin/stdout/stderr[^proc-fd]. Ở đây chúng là `pipe:[...]` vì tôi chạy qua công cụ ghi lại output (không phải terminal); chạy với TTY (`docker run -t`) thì reviewer đã quan sát được cả ba mục là `/dev/pts/0` — đây là quan sát chạy thật, không có nguồn trích dẫn. Dạng `pipe:[inode]` / `socket:[inode]` đúng định dạng `type:[inode]` mà `proc(5)` mô tả[^proc-fd].
- fd 3 và 4 chung inode `38475`: hai đầu của `io::pipe()`. fd 5 là socket của `TcpListener`. fd 6 là file thường. Đây là phần "mọi thứ là fd" ([chương 04 mục 6](../01-linux/04-file-descriptor-va-vfs.md)) nhìn thấy được.
- fd 7 là `/proc/15/fd`: chính thư mục đang được `read_dir` — lời gọi liệt kê mở một fd mới và nó hiện trong kết quả. `/proc/self` đã được resolve thành `/proc/<pid>`[^proc-self] (PID 15 ở đây).
- `status`: `PPid: 1` nghĩa là cha là process PID 1 của container. Reviewer chạy lại đúng lệnh ở trên (`bash -c 'cargo build … && ./target-linux/debug/procfs'`) thì thấy `Pid: 1`, `PPid: 0`, vì `bash -c` thay chính nó bằng lệnh cuối (`exec`) — nên output `Pid: 15 / PPid: 1` ở trên đến từ một cách gọi khác (có process khác làm PID 1 và `procfs` chạy như con của nó); không dùng lệnh `bash -c` đơn lẻ để giải thích PID; `Uid`/`Gid` bốn giá trị là real, effective, saved, filesystem[^proc-status-fields]; `FDSize: 64` là số slot fd đang cấp[^proc-status-fields].
- `maps`: mỗi dòng đúng định dạng `address perms offset dev inode pathname`[^proc-maps]; có binary của chính chương trình, `[heap]`, `[stack]`, `[vdso]`. Địa chỉ ngẫu nhiên mỗi lần chạy. Ý nghĩa sâu hơn (đọc `maps`, ASLR) nằm ở [P3 · chương 05 mục 3 — Bộ nhớ của tiến trình Linux](../03-memory/05-bo-nho-tien-trinh-linux.md); `[vdso]` xem [chương 02 mục 4](../01-linux/02-system-call.md).

**Bài tập mở rộng (b):** (1) chạy `./labs/linux.sh bash -c 'sleep 30 & ls -l /proc/$!/fd; grep -E "^(Name|State|PPid)" /proc/$!/status'` để xem fd của một process khác; (2) đọc `/proc/self/maps` hai lần và so sánh địa chỉ; (3) mở 100 file và quan sát `FDSize` trong `status` thay đổi ra sao.

### 1.3 Lab (c1) — `pipe_fork`: pipe, fork, waitpid, exit status

Lý thuyết: [chương 03 mục 3](../01-linux/03-process.md) (`fork`) và [mục 6](../01-linux/03-process.md) (`wait`, exit status), [chương 04 mục 8](../01-linux/04-file-descriptor-va-vfs.md) (pipe, EOF, `EPIPE`), [chương 05 mục 7](../01-linux/05-signal.md) (`SIGPIPE`).

Mã: `src/bin/pipe_fork.rs`, gọi thẳng `libc::pipe/fork/read/write/waitpid`. Ba phần: (1) con ghi vào pipe rồi `_exit(7)`, cha đọc đến EOF rồi `waitpid`; (2) con tự `kill(getpid(), SIGTERM)`; (3) ghi vào pipe không có reader.

```bash
./labs/linux.sh bash -c 'cargo build -p lab-linux -q && ./target-linux/debug/pipe_fork'
```

```text
[pipe_fork] parent pid 16 | pipe: read end fd 3, write end fd 4
[pipe_fork] parent read 32 bytes: hello parent, this is the child
[pipe_fork] child 17: exited, WEXITSTATUS = 7
[pipe_fork] child 18: killed by signal, WTERMSIG = 15
[pipe_fork] write to pipe without reader -> -1, errno -> Broken pipe (os error 32)
```

Giải thích: phần (1) đúng sơ đồ fork/wait ở [chương 03 mục 3](../01-linux/03-process.md) và [mục 6](../01-linux/03-process.md); `WEXITSTATUS = 7` là giá trị con truyền cho `_exit`[^exit-status]. Phần (2): con không `exit` mà bị signal 15 (`SIGTERM`)[^signal-nums], nên `WIFSIGNALED` đúng. Phần (3): `write` trả -1 với `EPIPE` (32) vì Rust đã để `SIGPIPE` ở `SIG_IGN`[^rs-sigpipe-ign][^pipe-eof][^pipe-epipe] (xem [chương 07 mục 5](../01-linux/07-rust-tren-giao-dien-he-thong.md)).

Mã cha đóng `write_end` *trước* khi đọc; nếu bạn xóa dòng `libc::close(write_end)` ở cha, vòng `read` sẽ treo mãi vì vẫn còn một fd ghi mở[^pipe-eof]. Reviewer đã chạy thử biến thể bỏ dòng đó trong container: chương trình in dòng `parent pid … | pipe: …` rồi không in gì thêm cho tới khi `timeout 5` giết nó (exit 124).

Quan sát bằng `strace -f`:

```bash
./labs/linux.sh bash -c 'strace -f -e trace=pipe2,clone,write,read,wait4,kill,close,exit_group ./target-linux/debug/pipe_fork 2>&1 | grep -v "^\[pipe_fork\]" | grep -A30 "^pipe2"'
```

```text
pipe2([3, 4], 0)                        = 0
clone(child_stack=NULL, flags=CLONE_CHILD_CLEARTID|CLONE_CHILD_SETTID|SIGCHLDstrace: Process 19 attached
, child_tidptr=0xeeb6397510f0) = 19
[pid    18] close(4)                    = 0
[pid    19] close(3 <unfinished ...>
[pid    18] read(3,  <unfinished ...>
[pid    19] write(4, "hello parent, this is the child\n", 32 <unfinished ...>
[pid    18] <... read resumed>"hello parent, this is the child\n", 64) = 32
[pid    18] read(3,  <unfinished ...>
[pid    19] close(4 <unfinished ...>
[pid    18] <... read resumed>"", 64)   = 0
[pid    19] exit_group(7 <unfinished ...>
[pid    19] +++ exited with 7 +++
--- SIGCHLD {si_signo=SIGCHLD, si_code=CLD_EXITED, si_pid=19, si_uid=0, si_status=7, si_utime=0, si_stime=0} ---
wait4(19, [{WIFEXITED(s) && WEXITSTATUS(s) == 7}], 0, NULL) = 19
...
pipe2([3, 4], 0)                        = 0
close(3)                                = 0
write(4, "x", 1)                        = -1 EPIPE (Broken pipe)
--- SIGPIPE {si_signo=SIGPIPE, si_code=SI_USER, si_pid=18, si_uid=0} ---
```

(Rút gọn: bỏ vài dòng `close`/`write` xen kẽ và phần signal-handling của phần (2); PID 18/19 là của lần chạy strace này, khác lần chạy trước.) Điểm cần đọc:

- `libc::fork()` hiện ra là `clone(... SIGCHLD)`, khớp man page `fork(2)`: một lời gọi `fork()` tương đương `clone` với flags chỉ là `SIGCHLD`[^fork-clone] (strace in thêm `CLONE_CHILD_*` do glibc đặt — chi tiết triển khai, không cần nhớ).
- Cha `close(4)` rồi `read(3)` bị chặn; con ghi 32 byte, đóng đầu ghi và `exit_group(7)`. `read` thứ hai của cha trả `""` = 0: EOF, vì mọi đầu ghi đã đóng[^pipe-eof].
- Khi con kết thúc, cha nhận `SIGCHLD`[^exit-status]; `wait4(19, [{WIFEXITED(s) && WEXITSTATUS(s) == 7}]...)` là strace giải mã status. Hàm C bạn gọi là `waitpid` nhưng syscall là `wait4` — thêm một ví dụ tên hàm ≠ tên syscall[^syscalls-extra].
- Phần (3): `write(4, "x", 1) = -1 EPIPE` kèm dòng `--- SIGPIPE ... ---`: signal vẫn được *sinh ra* nhưng bị `SIG_IGN` nên process không chết, và `write` trả `EPIPE` đúng như pipe(7) mô tả[^pipe-eof][^pipe-epipe].

### 1.4 Lab (c2) — `spawn`: `Command` + `Stdio::piped` và `strace -f`

Lý thuyết: [chương 07 mục 3](../01-linux/07-rust-tren-giao-dien-he-thong.md) (`Command::spawn`: `posix_spawn` hay `fork`+`exec`) và [mục 4](../01-linux/07-rust-tren-giao-dien-he-thong.md) (`Stdio` ↔ `dup2`, `Child` không `wait`), [chương 03 mục 4](../01-linux/03-process.md) (`exec`), [chương 04 mục 7](../01-linux/04-file-descriptor-va-vfs.md) (redirect/pipe ở mức syscall).

Mã: `src/bin/spawn.rs`. Bốn phần: (1) bắt stdout của `echo`; (2) `tr` với stdin *và* stdout là pipe; (3) `sh -c 'exit 3'` và `sh -c 'kill -TERM $$'` để xem `code()`/`signal()`; (4) một `Command` có `pre_exec`.

```bash
./labs/linux.sh bash -c 'cargo build -p lab-linux -q && ./target-linux/debug/spawn'
```

```text
[spawn] echo said: hello via Command
[spawn] echo pid 20 -> exit status: 0
[spawn] tr said: SHOUT THIS
[spawn] sh exit 3 -> code() = Some(3), signal() = None
[spawn] sh kill -TERM $$ -> code() = None, signal() = Some(15)
[spawn] true with pre_exec -> exit status: 0
```

Hai dòng giữa khớp doc: `code()` là `None` khi bị signal giết[^rs-exitstatus-code], và `signal()` lấy signal đó.

```bash
./labs/linux.sh bash -c 'strace -f -e trace=pipe2,clone,clone3,vfork,fork,execve,wait4,dup2,dup3 ./target-linux/debug/spawn 2>&1 | grep -v ENOENT | grep -v "^\[spawn\]"'
```

(Tôi lọc bỏ các `execve ... = -1 ENOENT`: đó là lần thử từng thư mục trong `PATH` — `/usr/local/cargo/bin/echo`, `/usr/local/sbin/echo`, ... đến `/usr/bin/echo` mới thành công. Std gọi `libc::posix_spawnp` trong đường `posix_spawn`[^rs-spawnp], và `posix_spawnp` tìm file trong các thư mục của `PATH`[^posix-spawnp-path].) Rút gọn:

```text
execve("./target-linux/debug/spawn", ["./target-linux/debug/spawn"], ...) = 0
pipe2([3, 4], O_CLOEXEC)                = 0
clone(child_stack=0xf5ceb58d2000, flags=CLONE_VM|CLONE_VFORK|SIGCHLDstrace: Process 23 attached
[pid    23] dup3(4, 1, 0)               = 1
[pid    23] execve("/usr/bin/echo", ["echo", "hello via Command"], ...) <unfinished ...>
[pid    23] +++ exited with 0 +++
wait4(23, [{WIFEXITED(s) && WEXITSTATUS(s) == 0}], 0, NULL) = 23
...
pipe2([3, 4], O_CLOEXEC)                = 0
pipe2([5, 6], O_CLOEXEC)                = 0
clone(child_stack=0xf5ceb58d2000, flags=CLONE_VM|CLONE_VFORK|SIGCHLD ...
[pid    24] dup3(3, 0, 0)               = 0
[pid    24] dup3(6, 1, 0)               = 1
[pid    24] execve("/usr/bin/tr", ["tr", "a-z", "A-Z"], ...)
...
[pid    26] --- SIGTERM {si_signo=SIGTERM, si_code=SI_USER, si_pid=26, si_uid=0} ---
[pid    26] +++ killed by SIGTERM +++
...
clone(child_stack=NULL, flags=CLONE_CHILD_CLEARTID|CLONE_CHILD_SETTID|SIGCHLD ...
[pid    18] execve("/usr/bin/true", ["true"], ...) = 0
```

(Rút gọn và ghép từ các đoạn của cùng một lần chạy; riêng dòng `execve("/usr/bin/true")` lấy từ một lần chạy strace khác với bộ lọc `trace=clone,clone3,fork,vfork,execve`, nên PID trong đó khác — PID luôn thay đổi giữa các lần chạy.)

Đọc output:

- `Stdio::piped()` → `pipe2([3,4], O_CLOEXEC)`: tạo pipe với close-on-exec[^open-cloexec]. Trong con: `dup3(4, 1, 0)` — đưa đầu ghi lên fd 1 (stdout) rồi mới `execve`. Đó là đúng cơ chế redirect: dup trong con, giữ fd qua exec[^exec-fds][^ostep-redirect]. Với `tr`, hai pipe: `dup3(3, 0)` (stdin) và `dup3(6, 1)` (stdout).
- Cha giữ đầu kia. `child.stdin.take()` rồi drop đóng đầu ghi → `tr` thấy EOF và thoát; nếu quên drop, `tr` chờ mãi — cùng quy tắc EOF của pipe[^pipe-eof].
- Các lệnh không có `pre_exec` (bốn lệnh đầu của lab) dùng `clone(... CLONE_VM|CLONE_VFORK|SIGCHLD)`: đúng dấu hiệu `posix_spawn` của glibc[^posix-spawn-clone], cha bị treo đến khi con `execve`/`_exit`[^clone-vfork]. Lệnh có `pre_exec` dùng `clone` kiểu `fork` (không `CLONE_VM`) — khớp việc std từ chối `posix_spawn` khi có closure[^rs-spawn-conds]. Lưu ý (nhận xét của người soạn): đây là hành vi của std 1.95.0 trên glibc của container; đừng suy ra nó đúng cho mọi phiên bản/libc.
- Trong con, việc đưa pipe lên fd 0/1 hiện là `dup3(..., 0)`. Ở mục 1.5 bạn sẽ thấy chính `libc::dup2` cũng hiện thành `dup3` trên máy aarch64 này ([chương 04 mục 3](../01-linux/04-file-descriptor-va-vfs.md) giải thích họ `dup`/`dup2`/`dup3`).

**Bài tập mở rộng (c):** (1) đổi `Command::new("echo")` thành đường dẫn tuyệt đối `/usr/bin/echo` và so sánh số `execve` ENOENT; (2) bỏ `child.wait()` và dùng `ps` (hoặc đọc `/proc/<pid>/status`, trường `State`) để quan sát zombie — đối chiếu với doc `Child`[^rs-child-zombie] và `wait(2)`[^wait-zombie]; (3) chạy `strace -f -e trace=pipe2,clone,execve,dup3,wait4 sh -c 'echo hi | tr a-z A-Z'` để xem shell dựng pipeline (tôi đã chạy: shell gọi `pipe2`, hai `clone`, `dup3` trong từng con rồi `execve /usr/bin/tr`).

### 1.5 Lab (d) — `rawwrite` (và `redirect`): bỏ qua `std::io`

Lý thuyết: [chương 02 mục 3](../01-linux/02-system-call.md) (wrapper vs `syscall(2)`, `errno`), [chương 04 mục 3](../01-linux/04-file-descriptor-va-vfs.md) (`dup`/`dup2`/`dup3`) và [mục 7](../01-linux/04-file-descriptor-va-vfs.md) (redirect), [chương 07 mục 2](../01-linux/07-rust-tren-giao-dien-he-thong.md) (`io::Error`) và [mục 7](../01-linux/07-rust-tren-giao-dien-he-thong.md) (FFI `libc`).

Mã: `src/bin/rawwrite.rs`. Ghi lên fd 1 bằng ba cách rồi cố ghi vào fd 99 chưa mở để xem `errno`.

```bash
./labs/linux.sh bash -c 'cargo build -p lab-linux -q && ./target-linux/debug/rawwrite'
```

```text
[rawwrite] 1: libc::write(1, ...)
[rawwrite]    -> returned 34
[rawwrite] 2: own unsafe extern "C" fn write
[rawwrite]    -> returned 45
[rawwrite] 3: libc::syscall(SYS_write, 1, ...)
[rawwrite]    -> returned 47
[rawwrite] stdin/stdout/stderr raw fds = 0/1/2; stdout.as_fd() = BorrowedFd { fd: 1 }
[rawwrite] 4: this line goes to fd 2 (stderr)
[rawwrite] write(99, ...) -> -1; errno = 9 (EBADF = 9)
[rawwrite] io::Error::last_os_error() = Os { code: 9, kind: Uncategorized, message: "Bad file descriptor" }
[rawwrite] Display: Bad file descriptor (os error 9)
```

Giải thích: ba cách đều trả số byte đã ghi (34, 45, 47 — đúng độ dài chuỗi). Cách 2 tự khai báo `unsafe extern "C" { #[link_name = "write"] fn c_write(...) }` — đúng cú pháp khối `extern` như Nomicon[^nomicon-extern]; cách 3 dùng `syscall(2)` — hàm thư viện gọi syscall theo số hiệu[^syscall-fn]. `write(99, ...)` trả -1 và `errno = 9 = EBADF`, khớp mô tả lỗi `EBADF` của `write(2)`[^write-ebadf]; `last_os_error()` biến đúng giá trị đó thành `io::Error` (`code: 9`)[^rs-last-os-error]. Lưu ý chương trình đọc `errno` bằng `*libc::__errno_location()`; std cũng làm vậy, với `link_name = "__errno_location"` trên Linux[^rs-errno-loc].

Strace các `write`:

```bash
./labs/linux.sh bash -c 'strace -e trace=write ./target-linux/debug/rawwrite 2>&1 | grep -E "^write\((1|2|99)," | cut -c1-70'
```

```text
write(1, "[rawwrite] 1: libc::write(1, ..."..., 34[rawwrite] 1: libc::
write(1, "[rawwrite]    -> returned 34\n", 29[rawwrite]    -> returned
write(1, "[rawwrite] 2: own unsafe extern "..., 45[rawwrite] 2: own un
write(1, "[rawwrite]    -> returned 45\n", 29[rawwrite]    -> returned
write(1, "[rawwrite] 3: libc::syscall(SYS_"..., 47[rawwrite] 3: libc::
write(1, "[rawwrite]    -> returned 47\n", 29[rawwrite]    -> returned
write(1, "[rawwrite] stdin/stdout/stderr r"..., 86[rawwrite] stdin/std
write(2, "[rawwrite] 4: this line goes to "..., 46[rawwrite] 4: this l
write(99, "[rawwrite] 1: libc::write(1, ..."..., 34) = -1 EBADF (Bad f
write(1, "[rawwrite] write(99, ...) -> -1;"..., 55[rawwrite] write(99,
write(1, "[rawwrite] io::Error::last_os_er"..., 108[rawwrite] io::Erro
write(1, "[rawwrite] Display: Bad file des"..., 53[rawwrite] Display: Bad file des
```

(Dòng bị `cut -c1-70` nên mất phần `= N` ở cuối; chạy không `cut` để xem đủ.) Ba cách khác nhau ở tầng Rust đều ra *cùng một* `write(1, ..., N)` ở tầng syscall với độ dài 34/45/47. Mỗi `println!` cũng thành một `write(1, ...)` riêng cho mỗi dòng — điều quan sát được ở trên, không phải khẳng định về chính sách buffer của std.

**Lab phụ `redirect`** (`src/bin/redirect.rs`): mô phỏng `cmd > file` — `dup` stdout cũ, `dup2(file, 1)`, in một dòng, `dup2` trả lại.

```bash
./labs/linux.sh bash -c 'cargo build -p lab-linux -q && ./target-linux/debug/redirect'
```

```text
[redirect] file fd = 3, saved copy of stdout = fd 4
[redirect] file contains: this line was written to fd 1, which is now the file
```

Dòng `this line was written...` **không** hiện ra terminal mà vào file — đó là tác dụng của `dup2`: fd 1 giờ trỏ tới description của file[^dup2-desc]. Strace:

```text
openat(AT_FDCWD, "/tmp/p01-redirect.txt", O_WRONLY|O_CREAT|O_TRUNC|O_CLOEXEC, 0666) = 3
dup(1)                                  = 4
dup3(3, 1, 0)                           = 1
write(1, "this line was written to fd 1, w"..., 53) = 53
dup3(4, 1, 0)                           = 1
```

Chú ý: mã gọi `libc::dup2`, nhưng strace trên aarch64 in `dup3(3, 1, 0)`. Đây là *quan sát* trên máy này; nó khớp với nguyên tắc wrapper có thể làm khác lời gọi C[^syscalls-extra]. Về kernel: header `asm-generic/unistd.h` (bản v6.1) định nghĩa `__NR_dup3` nhưng không có `__NR_dup2`, và header syscall của arm64 include chính file đó[^arm64-unistd][^generic-dup3] — nên trên arm64 không có syscall `dup2` để gọi. Việc glibc chuyển `dup2` thành `dup3` là suy luận của reviewer, tôi không tìm được nguồn glibc; trên x86-64 bạn có thể thấy `dup2`.

**Bài tập mở rộng (d):** (1) viết thêm một hàm gọi `libc::getpid()` và `libc::syscall(libc::SYS_getpid)` rồi so sánh với `std::process::id()`; xác nhận cả ba bằng nhau và trùng `Pid:` trong `/proc/self/status`; (2) thêm lời gọi `libc::open` không đóng fd, chạy lặp trong vòng và quan sát khi nào gặp `EMFILE`; (3) bọc fd thô trong `OwnedFd::from_raw_fd` và xác nhận strace thấy `close` tự động — đối chiếu doc `FromRawFd`[^rs-fromrawfd].

### 1.6 Kiểm tra chất lượng code lab

Trong repo: `cd labs && cargo build -p lab-linux && cargo clippy -p lab-linux --all-targets -- -D warnings` (macOS) và `./labs/linux.sh bash -c 'rustup component add clippy && cargo clippy -p lab-linux --all-targets -- -D warnings'` (container; image chưa cài sẵn `clippy`, nên phải thêm component mỗi lần chạy `--rm`).

## 2. Câu hỏi tự kiểm tra (về output lab)

Các câu hỏi lý thuyết thuần nằm ở phần "Câu hỏi tự kiểm tra" của từng chương trong [P1](../01-linux/00-tong-quan.md); các câu dưới đây cần output của lab.

1. Vì sao hàm `waitpid` bạn gọi lại hiện là `wait4` trong `strace`, và `libc::fork` hiện là `clone`? Điều đó nói gì về quan hệ hàm libc ↔ syscall?
   <details><summary>Đáp án</summary>

   Wrapper của glibc có thể làm thêm việc hoặc dùng syscall khác để thực hiện hàm C[^syscalls-extra][^syscalls-thin]; riêng `fork()` của glibc gọi `clone(2)`, tương đương `clone` với flags `SIGCHLD`[^fork-clone]. Trong lab (c1) strace thấy `clone` và `wait4`.

   </details>

2. `open` trả fd mấy trong chương trình mới khởi động, và vì sao? Điều gì khiến lần `reopen` ở lab (a) lại nhận đúng số cũ?
   <details><summary>Đáp án</summary>

   Số nhỏ nhất chưa dùng[^open-lowest]; 0/1/2 đã bị chiếm nên ra 3. Sau `close`, fd được trả về để dùng lại[^close-desc].

   </details>

3. Vì sao `ExitStatus::code()` có thể là `None`, và làm sao biết signal nào?
   <details><summary>Đáp án</summary>

   Trên Unix `code()` là `None` khi process bị signal giết[^rs-exitstatus-code]; dùng `ExitStatusExt::signal()`. Ở tầng C đó là `WIFSIGNALED`/`WTERMSIG`[^wait-wifsignaled]. Lab (c2) in `signal() = Some(15)`.

   </details>

4. Ghi vào pipe không còn reader: chương trình C thông thường và chương trình Rust phản ứng khác nhau thế nào, và source nào chứng minh?
   <details><summary>Đáp án</summary>

   Mặc định `write` sinh `SIGPIPE` (action `Term`)[^pipe-eof][^pipe-epipe][^signal-pipe]; chương trình Rust có `SIGPIPE` bị đặt `SIG_IGN` từ lúc khởi động nên `write` trả `EPIPE`[^rs-rt-sigpipe][^rs-sigpipe-ign]. Lab (c1) in `Broken pipe (os error 32)`.

   </details>

5. Khi nào `Command::spawn` không dùng `posix_spawn`? Bạn kiểm chứng bằng cách nào mà không đọc source?
   <details><summary>Đáp án</summary>

   Khi có uid/gid, groups, chroot, `pre_exec` closure hoặc đổi PATH với program không phải path[^rs-spawn-conds]. Kiểm chứng bằng `strace -f -e trace=clone`: `CLONE_VM|CLONE_VFORK` là dấu hiệu `posix_spawn`[^posix-spawn-clone]; không có là đường fork/exec.

   </details>


[^open-lowest]: open(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "The file descriptor returned by a successful call will be the lowest-numbered file descriptor not currently open for the process."
[^close-desc]: close(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/close.2.en.html> — "close() closes a file descriptor, so that it no longer refers to any file and may be reused."
[^rs-fs-cloexec]: rust-lang/rust tag 1.95.0, library/std/src/sys/fs/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/fs/unix.rs> — "let flags = libc::O_CLOEXEC | opts.get_access_mode()? | opts.get_creation_mode()?"
[^rs-fs-open]: rust-lang/rust tag 1.95.0, library/std/src/sys/fs/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/fs/unix.rs> — "let fd = cvt_r(|| unsafe { open64(path.as_ptr(), flags, opts.mode as c_int) })?;"
[^openat-same]: open(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "The openat() system call operates in exactly the same way as open(), except for the differences described here."
[^rs-fd-write]: rust-lang/rust tag 1.95.0, library/std/src/sys/fd/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/fd/unix.rs> — "let ret = cvt(unsafe { libc::write( self.as_raw_fd(), buf.as_ptr() as *const libc::c_void, cmp::min(buf.len(), READ_LIMIT), ) })?;"
[^rs-cvt]: rust-lang/rust tag 1.95.0, library/std/src/sys/pal/unix/mod.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/pal/unix/mod.rs> — "if t.is_minus_one() { Err(io::Error::last_os_error()) } else { Ok(t) }"
[^rs-drop-close]: rust-lang/rust tag 1.95.0, library/std/src/os/fd/owned.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/os/fd/owned.rs> — "Note that errors are ignored when closing a file descriptor."
[^rs-file]: Rust std 1.95.0, `std::fs::File` — <https://doc.rust-lang.org/1.95.0/std/fs/struct.File.html> — "Files are automatically closed when they go out of scope. Errors detected on closing are ignored by the implementation of Drop."
[^proc-fd]: proc(5), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "This is a subdirectory containing one entry for each file which the process has open, named by its file descriptor, and which is a symbolic link to the actual file. Thus, 0 is standard input, 1 standard output, 2 standard error, and so on. For file descriptors for pipes and sockets, the entries will be symbolic links whose content is the file type with the inode."
[^proc-self]: proc(5), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "When a process accesses this magic symbolic link, it resolves to the process's own /proc/pid directory."
[^proc-status-fields]: proc(5), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "Uid, Gid Real, effective, saved set, and filesystem UIDs (GIDs). FDSize Number of file descriptor slots currently allocated."
[^proc-maps]: proc(5), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "A file containing the currently mapped memory regions and their access permissions."
[^exit-status]: _exit(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/_exit.2.en.html> — "Any open file descriptors belonging to the process are closed. Any children of the process are inherited by init(1) (or by the nearest \"subreaper\" process as defined through the use of the prctl(2) PR_SET_CHILD_SUBREAPER operation). The process's parent is sent a SIGCHLD signal."
[^signal-nums]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "SIGPIPE 13 13 13 13 SIGALRM 14 14 14 14 SIGTERM 15 15 15 15"
[^rs-sigpipe-ign]: rust-lang/rust tag 1.95.0, library/std/src/sys/pal/unix/mod.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/pal/unix/mod.rs> — "sigpipe::DEFAULT => (false, Some(libc::SIG_IGN)),"
[^pipe-eof]: pipe(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/pipe.7.en.html> — "If all file descriptors referring to the write end of a pipe have been closed, then an attempt to read(2) from the pipe will see end-of-file (read(2) will return 0). If all file descriptors referring to the read end of a pipe have been closed, then a write(2) will cause a SIGPIPE signal to be generated for the calling process."
[^pipe-epipe]: pipe(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/pipe.7.en.html> — "If the calling process is ignoring this signal, then write(2) fails with the error EPIPE."
[^fork-clone]: fork(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/fork.2.en.html> — "A call to fork() is equivalent to a call to clone(2) specifying flags as just SIGCHLD."
[^syscalls-extra]: syscalls(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/syscalls.2.en.html> — "Sometimes, however, the wrapper function does some extra work before invoking the system call."
[^rs-exitstatus-code]: Rust std 1.95.0, `std::process::ExitStatus` — <https://doc.rust-lang.org/1.95.0/std/process/struct.ExitStatus.html> — "On Unix, this will return None if the process was terminated by a signal."
[^rs-spawnp]: rust-lang/rust tag 1.95.0, library/std/src/sys/process/unix/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/process/unix/unix.rs> — "match libc::posix_spawnp(pid, file, file_actions, attrp, argv, envp)"
[^posix-spawnp-path]: posix_spawn(3), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/posix_spawn.3.en.html> — "With posix_spawnp(), the executable file is specified as a simple filename; the system searches for this file in the list of directories specified by PATH"
[^open-cloexec]: open(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "Enable the close-on-exec flag for the new file descriptor. Specifying this flag permits a program to avoid additional fcntl(2) F_SETFD operations to set the FD_CLOEXEC flag."
[^exec-fds]: execve(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/execve.2.en.html> — "File descriptors that are marked close-on-exec are closed; see the description of FD_CLOEXEC in fcntl(2)."
[^ostep-redirect]: OSTEP, ch.5 Interlude: Process API — <https://pages.cs.wisc.edu/~remzi/OSTEP/cpu-api.pdf> — "open file descriptors are kept open across the exec() call, thus enabling this behavior"
[^posix-spawn-clone]: posix_spawn(3), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/posix_spawn.3.en.html> — "the posix_spawn() function commences by calling clone(2) with CLONE_VM and CLONE_VFORK flags."
[^clone-vfork]: clone(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/clone.2.en.html> — "If CLONE_VFORK is set, the execution of the calling process is suspended until the child releases its virtual memory resources via a call to execve(2) or _exit(2) (as with vfork(2))."
[^rs-spawn-conds]: rust-lang/rust tag 1.95.0, library/std/src/sys/process/unix/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/process/unix/unix.rs> — "if self.get_gid().is_some() || self.get_uid().is_some() || (self.env_saw_path() && !self.program_is_path()) || !self.get_closures().is_empty() || self.get_groups().is_some() || self.get_chroot().is_some() { return Ok(None); }"
[^rs-child-zombie]: Rust std 1.95.0, `std::process::Child` — <https://doc.rust-lang.org/1.95.0/std/process/struct.Child.html> — "The standard library does not automatically wait on child processes (not even if the Child is dropped), it is up to the application developer to do so."
[^wait-zombie]: wait(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/wait.2.en.html> — "A child that terminates, but has not been waited for becomes a \"zombie\". The kernel maintains a minimal set of information about the zombie process (PID, termination status, resource usage information) in order to allow the parent to later perform a wait to obtain information about the child."
[^nomicon-extern]: The Rustonomicon, Foreign Function Interface — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "The extern block is a list of function signatures in a foreign library, in this case with the platform’s C ABI."
[^syscall-fn]: syscall(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/syscall.2.en.html> — "syscall() is a small library function that invokes the system call whose assembly language interface has the specified number with the specified arguments."
[^write-ebadf]: write(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/write.2.en.html> — "EBADF fd is not a valid file descriptor or is not open for writing."
[^rs-last-os-error]: Rust std 1.95.0, `std::io::Error` — <https://doc.rust-lang.org/1.95.0/std/io/struct.Error.html> — "This function reads the value of errno for the target platform (e.g. GetLastError on Windows) and will return a corresponding instance of Error for the error code. This should be called immediately after a call to a platform function, otherwise the state of the error value is indeterminate."
[^rs-errno-loc]: rust-lang/rust tag 1.95.0, library/std/src/sys/io/error/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/io/error/unix.rs> — "unsafe { (*errno_location()) as i32 }"
[^dup2-desc]: dup(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/dup.2.en.html> — "In other words, the file descriptor newfd is adjusted so that it now refers to the same open file description as oldfd. If the file descriptor newfd was previously open, it is closed before being reused"
[^arm64-unistd]: Linux v6.1, arch/arm64/include/uapi/asm/unistd.h — <https://raw.githubusercontent.com/torvalds/linux/v6.1/arch/arm64/include/uapi/asm/unistd.h> — "#include \<asm-generic/unistd.h>"
[^generic-dup3]: Linux v6.1, include/uapi/asm-generic/unistd.h — <https://raw.githubusercontent.com/torvalds/linux/v6.1/include/uapi/asm-generic/unistd.h> — "#define __NR_dup3 24 __SYSCALL(__NR_dup3, sys_dup3)"
[^rs-fromrawfd]: Rust std 1.95.0, `std::os::fd::FromRawFd` — <https://doc.rust-lang.org/1.95.0/std/os/fd/trait.FromRawFd.html> — "This function is typically used to consume ownership of the specified file descriptor. When used in this way, the returned object will take responsibility for closing it when the object goes out of scope."
[^syscalls-thin]: syscalls(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/syscalls.2.en.html> — "Often the glibc wrapper function is quite thin, doing little work other than copying arguments to the right registers before invoking the system call, and then setting errno appropriately after the system call has returned."
[^rs-rt-sigpipe]: rust-lang/rust tag 1.95.0, library/std/src/rt.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/rt.rs> — "Since 2014, the Rust runtime on Unix has set the `SIGPIPE` handler to `SIG_IGN`."
[^wait-wifsignaled]: wait(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/wait.2.en.html> — "WIFSIGNALED(wstatus) returns true if the child process was terminated by a signal. WTERMSIG(wstatus) returns the number of the signal that caused the child process to terminate."
[^signal-pipe]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "SIGPIPE P1990 Term Broken pipe: write to pipe with no readers; see pipe(7)"
