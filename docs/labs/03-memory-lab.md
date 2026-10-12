# Lab P3 — Bộ nhớ

> Đây là phần thực hành của [P3 · Bộ nhớ](../03-memory/00-tong-quan.md). **Hãy đọc lý thuyết trước** — các lab dưới đây giả định bạn đã nắm address space, page fault, allocator, `Vec`/`Box`/`Drop`; mỗi bài lab ghi rõ chương và mục của P3 cần đọc.
>
> Quy ước chú thích: mọi sự thật có chú thích cuối trang trỏ về nguồn và câu trích nguyên văn ở cuối file (chạy `python3 tools/verify_citations.py docs/labs/03-memory-lab.md`). Câu ghi "(nhận xét của người soạn)" hoặc "[INFERENCE]" là lời khuyên/suy luận, không phải sự thật có nguồn. Output của lab là output thật đã chạy trong container `./labs/linux.sh` (Debian bookworm, **aarch64**, Rust 1.95); địa chỉ, PID, số fault sẽ khác trên máy bạn. Tài liệu không ghi output chạy trên macOS (aarch64, rustc 1.95.0 chỉ dùng để build và clippy, mục 1.6); các bin Linux-only không có output macOS.

## 1. Chuẩn bị và các bài lab

Crate: `labs/crates/lab-memory`, phụ thuộc duy nhất `libc = "0.2"` (để gọi `mmap`, `getrusage`, `getrlimit`, `sysconf` — std không bọc các syscall này). Helper chung ở `src/lib.rs` (đọc `/proc/self/maps`, `minflt` từ `/proc/self/stat`, `ru_minflt` từ `getrusage`). Trên macOS, `layout` và `page_faults` chỉ in hướng dẫn chạy bằng `./labs/linux.sh`; `count_alloc`, `big_alloc` và `stack` chạy được cả trên macOS (nhưng `strace` của lab (d) chỉ có trên Linux).

### 1.1 Lab (a): địa chỉ stack/heap/static so với `/proc/self/maps`

Lý thuyết: [chương 01 mục 2](../03-memory/01-address-space.md) (address space và địa chỉ ảo), [chương 05 mục 3](../03-memory/05-bo-nho-tien-trinh-linux.md) (đọc `maps`, ASLR), [chương 07 mục 2](../03-memory/07-stack-alignment-padding.md) (hướng tăng của stack).

```bash
./labs/linux.sh cargo run -q -p lab-memory --bin layout
```

Output (rút gọn — chỉ phần map của binary, heap, stack; địa chỉ sẽ khác trên máy bạn):

```text
what                                      address
code   fn main                     0xb01331fb9840
rodata "xin chao" bytes            0xb01331ff2772
static GREETING (the &str)         0xb0133201db48
static mut COUNTER                 0xb013320209c0
stack  local on_stack              0xffffec458ca8
heap   Box<u64> (8 B)              0xb01334e7fd60
heap   Vec<u8> 1 MiB buffer        0xff286152f010

region in /proc/self/maps for each address:
code   fn main                 -> b01331fb0000-b01332006000 r-xp 00000000 00:25 1833   /work/labs/target-linux/debug/layout
rodata "xin chao" bytes        -> b01331fb0000-b01332006000 r-xp 00000000 00:25 1833   /work/labs/target-linux/debug/layout
static GREETING (the &str)     -> b0133201d000-b01332020000 r--p 0005d000 00:25 1833   /work/labs/target-linux/debug/layout
static mut COUNTER             -> b01332020000-b01332021000 rw-p 00060000 00:25 1833   /work/labs/target-linux/debug/layout
stack  local on_stack          -> ffffec43a000-ffffec45b000 rw-p 00000000 00:00 0      [stack]
heap   Box<u64> (8 B)          -> b01334e7f000-b01334ea0000 rw-p 00000000 00:00 0      [heap]
heap   Vec<u8> 1 MiB buffer    -> ff286152f000-ff2861630000 rw-p 00000000 00:00 0
```

Đọc output:

- `fn main` và chuỗi `"xin chao"` nằm trong mapping `r-xp` của chính file thực thi: code (và literal ở trong cùng mapping trong build debug này). Static `GREETING` nằm ở mapping `r--p` (chỉ đọc), `static mut COUNTER` ở mapping `rw-p` — cùng file binary, khác quyền. Quyền `p` = private copy-on-write[^proc-perms].
- Biến cục bộ ở `[stack]`[^proc-stack]; `Box<u64>` nhỏ ở `[heap]`[^proc-heap].
- `Vec<u8>` 1 MiB **không** ở `[heap]` mà ở một mapping ẩn danh (pathname trống) — dấu hiệu của anonymous `mmap`[^proc-anon]. Điều này khớp với `mallopt(3)`: 1 MiB vượt ngưỡng 128 KiB[^mallopt-default]; lab (d) kiểm chứng bằng `strace`.
- Địa chỉ code ở dạng cao `0xb013…` (quan sát trên aarch64; tôi không kiểm tra binary có phải PIE hay không, [INFERENCE]); kernel có thể ngẫu nhiên hóa vị trí các vùng nên địa chỉ có thể đổi giữa các lần chạy[^kernel-aslr]. Trên kiến trúc khác địa chỉ sẽ khác.

### 1.2 Lab (b): global allocator đếm alloc/dealloc

Lý thuyết: [chương 08 mục 3](../03-memory/08-rust-va-bo-nho.md) (`Copy`, `Drop`, thứ tự drop), [mục 4](../03-memory/08-rust-va-bo-nho.md) (bố cục `Box`/`Vec`/`String`, tăng capacity), [mục 5](../03-memory/08-rust-va-bo-nho.md) (`GlobalAlloc`, `Layout`), [mục 6](../03-memory/08-rust-va-bo-nho.md) (`Rc`, leak vẫn safe); [chương 06 mục 10](../03-memory/06-heap-allocator.md) (global allocator).

```bash
./labs/linux.sh cargo run -q -p lab-memory --bin count_alloc
```

(`count_alloc` chạy được cả trên macOS: `cd labs && cargo run -p lab-memory --bin count_alloc`; output dưới đây là từ container Linux.) Output (rút gọn các dòng size/align, phần đầu và drop order giữ nguyên):

```text
u64=8 &u64=8 Box<u64>=8 Option<Box<u64>>=8 &[u8]=16 Vec<u8>=24 String=24 Rc<u64>=8
align_of: u8=1 u32=4 u64=8 (u8,u64)=8 size_of (u8,u64)=16

== Vec<u64>::push growth ==
Vec::new(): len=0 capacity=0
push #1     capacity    0 -> 4     alloc=1 realloc=0 bytes requested=32
push #5     capacity    4 -> 8     alloc=0 realloc=1 bytes requested=64
push #9     capacity    8 -> 16    alloc=0 realloc=1 bytes requested=128
push #17    capacity   16 -> 32    alloc=0 realloc=1 bytes requested=256
...
push #513   capacity  512 -> 1024  alloc=0 realloc=1 bytes requested=8192
total for 1000 pushes                        alloc=1 realloc=8 dealloc=0
Vec::with_capacity(1000) + 1000 pushes + drop alloc=1 realloc=0 dealloc=1
clear() then refill same Vec to len 1000     alloc=0 realloc=0 dealloc=0

== Box, String, Rc ==
Box::new(5u64), then dropped                 alloc=1 realloc=0 dealloc=1
Box::new(()) (zero-sized), then dropped      alloc=0 realloc=0 dealloc=0
String::from("hello")                        alloc=1 realloc=0 dealloc=0
move the String into another binding         alloc=0 realloc=0 dealloc=0
drop the moved String                        alloc=0 realloc=0 dealloc=1
Rc::new + Rc::clone, then both dropped       alloc=1 realloc=0 dealloc=1
  strong_count was 2; the one allocation asked for 24 bytes

== leaking is safe ==
mem::forget(Box::new(1))                     alloc=1 realloc=0 dealloc=0
Box::leak(Box::new(2)) -> &'static mut u64   alloc=1 realloc=0 dealloc=0
leaked value is still usable: 3

== drop order ==
  drop(moved into drop() early)
  end of scope
  drop(tuple.0)
  drop(tuple.1)
  drop(b, declared second)
  drop(a, declared first)
```

Đọc output:

- `Vec<u8>`/`String` = 24 byte, `&[u8]` = 16: bộ ba (ptr, cap, len)[^vec-triplet] là 3 × 8 byte; slice là (ptr, len). `Box<u64>` và `Option<Box<u64>>` cùng 8 byte[^opt-npo].
- `Vec::new()` capacity 0 (chưa cấp phát[^vec-new]); push đầu tiên lên capacity **4** vì phần tử `u64` (8 byte ≤ 1 KiB) có capacity tối thiểu 4[^rawvec-min]; các lần sau gấp đôi[^rawvec-double] bằng **một `realloc`** mỗi lần — 1000 phần tử tốn 1 alloc + 8 realloc thay vì 1000 lần. `realloc` có thể mở rộng tại chỗ hoặc di chuyển; counter chỉ đếm lời gọi (không biết).
- `with_capacity(1000)`: đúng một allocation[^vec-exact]. `clear()` giữ nguyên buffer nên refill không cấp phát lại.
- `Box::new(())`: `alloc=0` (ZST[^box-zst], và `GlobalAlloc::alloc` không được gọi với size 0[^ga-zero]).
- `Rc<u64>`: một allocation 24 byte[^rc-inner].
- `forget`/`leak`: có `alloc` nhưng không `dealloc` — rò rỉ mà vẫn safe[^forget-safe].
- Drop order khớp [chương 08 mục 3](../03-memory/08-rust-va-bo-nho.md)[^ref-drop-scope][^ref-drop-fields], cụ thể `moved` bị drop ngay khi gọi `drop`, cuối scope là `_pair` (khai báo sau cùng, field theo thứ tự), rồi `_b`, `_a`.

Thiết kế allocator trong lab: chỉ đếm bằng `AtomicUsize` rồi chuyển tiếp sang `System`; không in/cấp phát bên trong vùng đo vì `println!` có thể cấp phát (nhận xét của người soạn, thể hiện trong comment mã).

### 1.3 Lab (c): `mmap` ẩn danh và page fault

Lý thuyết: [chương 04 mục 1](../03-memory/04-page-fault-va-swapping.md) (present bit và page fault), [mục 2](../03-memory/04-page-fault-va-swapping.md) (demand paging/demand zeroing), [mục 6](../03-memory/04-page-fault-va-swapping.md) (minor/major fault, `getrusage`), [chương 05 mục 4](../03-memory/05-bo-nho-tien-trinh-linux.md) (`mmap`, copy-on-write).

```bash
./labs/linux.sh cargo run -q -p lab-memory --bin page_faults
```

Output (số fault tuyệt đối phụ thuộc máy/thời điểm; hiệu số mới là ý nghĩa):

```text
page size = 4096 B, mapping 67108864 B = 16384 pages
before mmap                        ru_minflt=   3633 ru_majflt=2 /proc minflt=   3633
after mmap (nothing touched)       ru_minflt=   3633 ru_majflt=2 /proc minflt=   3633
after reading the first byte       ru_minflt=   3635 ru_majflt=2 /proc minflt=   3635
first byte reads as 0 (anonymous memory is zero-filled)
after writing 1 byte per page      ru_minflt=  20019 ru_majflt=2 /proc minflt=  20019
after a second pass                ru_minflt=  20019 ru_majflt=2 /proc minflt=  20019

faults during mmap itself         : 0
faults during the 1-byte read     : 2
faults during first write pass    : 16384 (pages = 16384)
faults during second write pass   : 0
```

Đọc output:

- `getrusage` (`ru_minflt`) và cột 10 `minflt` của `/proc/self/stat`[^proc-minflt] cho **cùng số** trong output này (nhiều khả năng cùng một bộ đếm của kernel — suy luận của người soạn, [INFERENCE]).
- Gọi `mmap` 64 MiB: **0** fault. Đây là bằng chứng quan sát rằng `mmap` chỉ tạo mapping[^mmap-new]; chưa có page nào bị chạm.
- Ghi 1 byte vào mỗi page đầu tiên: đúng **16384** fault = số page. Lượt ghi thứ hai: **0** fault, vì page đã có sẵn.
- Byte đầu đọc ra `0`, khớp với man page ("initialized to zero")[^mmap-anon].
- Lần đọc 1 byte có 2 fault chứ không phải 1. Tôi không xác định được nguyên nhân của fault thừa (có thể do code in kết quả chạm vào page mới của stdout/stack, hoặc cách kernel xử lý lần đọc đầu); không có nguồn nên không khẳng định. Số 16384 ở lượt ghi cũng bao gồm page đầu tiên đã bị đọc trước đó, nên điều đo được là "ghi vào mọi page lần đầu → mỗi page một fault", không hơn.
- `ru_majflt=2` không đổi suốt lab: không có I/O đĩa mới. (Hai major fault ban đầu đến từ việc nạp binary/thư viện — suy đoán của người soạn, [INFERENCE].)

### 1.4 Lab (d): `strace` thấy `brk`/`mmap` khi cấp phát lớn

Lý thuyết: [chương 06 mục 8](../03-memory/06-heap-allocator.md) (`brk` hay `mmap`, ngưỡng động, vì sao `free` không giảm RSS), [chương 05 mục 4](../03-memory/05-bo-nho-tien-trinh-linux.md) (`mmap` và `brk`).

```bash
./labs/linux.sh bash -c 'cargo build -q -p lab-memory --bin big_alloc && \
  strace -e trace=brk,mmap,munmap,write ./target-linux/debug/big_alloc'
```

Mỗi bước in một dòng đánh dấu lên stderr (một `write` duy nhất) rồi mới cấp `Vec<u8>`. Output (rút gọn: lọc các dòng đánh dấu và syscall ngay sau chúng; địa chỉ sẽ khác):

```text
write(2, "--- small 1 KiB: Vec<u8> of 1024"..., 43--- small 1 KiB: Vec<u8> of 1024 bytes ---
) = 43
write(2, "--- medium 64 KiB: Vec<u8> of 65"..., 46--- medium 64 KiB: Vec<u8> of 65536 bytes ---
) = 46
write(2, "--- large 1 MiB: Vec<u8> of 1048"..., 46--- large 1 MiB: Vec<u8> of 1048576 bytes ---
) = 46
mmap(NULL, 1052672, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANONYMOUS, -1, 0) = 0xe321235df000
write(2, "--- free the 1 MiB block ---\n", 29--- free the 1 MiB block ---
) = 29
munmap(0xe321235df000, 1052672)         = 0
write(2, "--- large 1 MiB again, after the"..., 68--- large 1 MiB again, after the free: Vec<u8> of 1048576 bytes ---
) = 68
brk(0xb3384da34000)                     = 0xb3384da34000
write(2, "--- done ---\n", 13--- done ---
) = 13
munmap(0xe321238ff000, 20480)           = 0
```

Đọc output:

1. **1 KiB và 64 KiB**: giữa hai dòng đánh dấu và sau chúng không có `brk`/`mmap` mới — chúng được phục vụ từ heap đã có sẵn (giả thuyết của người soạn: heap đã được khởi tạo từ lúc chương trình khởi động, [INFERENCE]; dòng lọc trong output trên không hiển thị phần đó). 64 KiB < 128 KiB nên dưới ngưỡng mặc định[^mallopt-default].
2. **1 MiB lần đầu**: một `mmap(NULL, 1052672, ..., MAP_PRIVATE|MAP_ANONYMOUS, -1, 0)` — đúng kiểu `mmap` ẩn danh của lab (c)[^mmap-anon]. Độ dài 1052672 lớn hơn 1048576 đúng 4096 byte; phần dư là chi phí bookkeeping của allocator (suy đoán của người soạn, [INFERENCE]).
3. **`drop(large)`**: `munmap` trả ngay khối cho hệ thống — đúng ưu điểm "độc lập trả lại hệ thống" của `mmap`[^mallopt-adv].
4. **1 MiB lần hai**: *không có* `mmap` mà chỉ một `brk` (heap được mở rộng). Phù hợp với ngưỡng động: sau khi free khối lớn hơn ngưỡng (và ≤ trần), ngưỡng được nâng lên bằng kích thước khối đó[^mallopt-dyn]; 1 MiB nhỏ hơn trần 32 MiB của 64-bit[^mallopt-max], nên lần cấp sau của cùng kích thước đi vào heap. Đây là một chứng minh bằng quan sát; tôi không chạy các kích thước khác nên **không** khẳng định thêm điều gì về ngưỡng ngoài những gì man page nói.
5. Cuối chương trình, `munmap(…, 20480)` trả lại một mapping 20480 byte. Theo source std 1.95.0, handler tràn stack của Rust cài một *alternate signal stack* bằng `mmap` với cờ `MAP_STACK`[^std-sigstack-flags] và khi dọn dẹp thì `munmap` từ một page trước vùng stack đó[^std-sigstack-unmap]; kích thước 20480 = 5 page 4 KiB khớp dạng "vùng sigaltstack + một page guard" (chi tiết đó là suy luận của người soạn, [INFERENCE]; tôi không đối chiếu từng hằng số). Cơ chế sigaltstack được giải thích ở [chương 07 mục 4](../03-memory/07-stack-alignment-padding.md).

Lưu ý: đây là hành vi của glibc (Debian bookworm). `System` của Rust "dựa trên `malloc`" trên Unix[^std-system], nên allocator thật sự là cái mà libc cung cấp.

### 1.5 Lab (e): stack frame và stack overflow

Lý thuyết: [chương 07 mục 2](../03-memory/07-stack-alignment-padding.md) (hướng tăng stack), [mục 3](../03-memory/07-stack-alignment-padding.md) (stack process/thread, `RLIMIT_STACK`), [mục 4](../03-memory/07-stack-alignment-padding.md) (guard page, tràn stack → `abort`); về PID 1 trong container xem [P1 · chương 03 mục 7](../01-linux/03-process.md).

```bash
./labs/linux.sh cargo run -q -p lab-memory --bin stack
./labs/linux.sh bash -c 'cargo build -q -p lab-memory --bin stack && ./target-linux/debug/stack overflow; echo "exit=$?"'
```

Output chạy thường:

```text
address of a local in each nested call (main thread):
  depth 0: 0xfffffd20fd70  next frame is 160 bytes lower
  depth 1: 0xfffffd20fcd0  next frame is 160 bytes lower
  ...
  depth 4: 0xfffffd20faf0  next frame is 160 bytes lower
RLIMIT_STACK soft=8192 KiB hard=unlimited
main-thread stack mapping: fffffd1f0000-fffffd211000 rw-p 00000000 00:00 0      [stack] (132 KiB mapped right now)
a spawned thread's local lives at 0xe4d757cee587 (a different stack)
```

Output với `overflow` (thread 256 KiB đệ quy vô hạn):

```text
thread 'small-stack' (14) has overflowed its stack
fatal runtime error: stack overflow, aborting
bash: line 1:    13 Aborted                 (core dumped) ./target-linux/debug/stack overflow
exit=134
```

Đọc output:

- Mỗi frame của `descend` cách nhau 160 byte (64 byte mảng + phần còn lại là bookkeeping của frame; phép chia chi tiết phụ thuộc compiler, không khẳng định). Địa chỉ **giảm** khi gọi sâu hơn trên aarch64 này.
- `RLIMIT_STACK` soft = 8 MiB: chạm giới hạn thì kernel gửi `SIGSEGV`[^rlimit-stack]. Mapping `[stack]` hiện chỉ 132 KiB đã được map, ít hơn nhiều so với 8 MiB giới hạn (chỉ là quan sát; tài liệu không khẳng định cơ chế vì sao mapping nhỏ hơn giới hạn).
- Thread con có stack ở dải địa chỉ khác hẳn[^std-thread-model]; Rust đặt `stack_size` qua `Builder`[^std-thread-stack] (xem [chương 07 mục 9](../03-memory/07-stack-alignment-padding.md)).
- Stack overflow trong Rust: std có signal handler cho `SIGSEGV`/`SIGBUS`; nó dựa vào các *guard page* (page không map) ở cuối stack mỗi thread, để in thông báo "has overflowed its stack" rồi abort[^std-guard]. Đường đi của abort có thể đọc thẳng trong source: handler gọi `rtabort!("stack overflow")`[^std-rtabort-call], macro này in "fatal runtime error: …, aborting" rồi gọi `crate::process::abort()`[^std-rtabort-macro], và trên Unix `abort_internal` gọi `libc::abort()`[^std-abort-internal]. `abort(3)` raise tín hiệu `SIGABRT` cho chính process[^abort3]. Bash báo exit status 128+N khi lệnh chết vì tín hiệu N[^bash-128n]; `SIGABRT` là 6 trên x86/ARM[^signal-nums], nên ra **134** = 128 + 6 (dòng `Aborted (core dumped)` là bash in ra). Process bị abort, destructor không chạy[^std-process-abort]; đây không phải panic có thể bắt được.
- Bẫy khi đo exit code trong container: nếu chạy binary làm **PID 1** của container, exit code nhận được ở host là **133** chứ không phải 134. Ví dụ `./labs/linux.sh cargo run … -- overflow`: `cargo` là PID 1, và `cargo run` thay chính nó bằng binary[^cargo-exec-replace], nên binary trở thành PID 1. Người soạn đã chạy cả hai cách và quan sát đúng như vậy. Có nguồn cho thấy init của một PID namespace được đối xử đặc biệt với tín hiệu[^pidns-init]. Nhưng man page đó nói về tín hiệu do process *khác* gửi tới, còn việc 133 = 128 + 5 (`SIGTRAP`) xuất phát từ đường dự phòng của glibc khi `SIGABRT` không giết được PID 1 là **suy luận của người soạn, chưa kiểm chứng bằng nguồn**. Bài học thực tế: muốn đọc đúng exit status, chạy binary dưới một shell trong container chứ đừng để nó làm PID 1.

### 1.6 Build và clippy

```bash
cd labs && cargo build -p lab-memory && cargo clippy -p lab-memory --all-targets -- -D warnings   # macOS
./labs/linux.sh bash -c 'rustup component add clippy && cargo clippy -p lab-memory --all-targets -- -D warnings'
```

Cả hai đã chạy sạch cảnh báo khi soạn tài liệu này (container không cài sẵn clippy nên cần `rustup component add clippy`).

### 1.7 Bài tập mở rộng

1. Sửa `layout.rs` để cấp `Vec<u8>` các kích thước 100 KiB, 127 KiB, 129 KiB, 200 KiB và xem khối nào nằm ở `[heap]`, khối nào ở mapping ẩn danh. Đối chiếu với `mallopt(3)`[^mallopt-default] — nhớ rằng ngưỡng là *động*[^mallopt-dyn], nên thứ tự cấp/free ảnh hưởng kết quả.
2. Trong `count_alloc`, thay `Vec::new()` bằng `Vec::<u8>::new()` và quan sát capacity đầu tiên (kỳ vọng từ mã nguồn: 8[^rawvec-min]); thử `Vec<[u8; 2048]>` (kỳ vọng: 1).
3. Thêm vào `page_faults` một lượt `madvise(MADV_DONTNEED)` rồi ghi lại, xem fault có xuất hiện lại không. Đọc `madvise(2)` trước và ghi lại câu bạn dựa vào.
4. Viết allocator bump trong 40 dòng (mẫu có trong docs `GlobalAlloc`[^globalalloc]) rồi chạy `count_alloc` với nó; điều gì hỏng khi `dealloc` không làm gì?
5. Chạy `strace -f -e trace=brk,mmap,munmap` trên một chương trình Rust spawn thread và liệt kê các `mmap` có cờ `MAP_STACK` (lab (d) cũng có một cái 20480 byte ở đầu chương trình). Đọc `mmap(2)` để biết `MAP_STACK` nghĩa là gì và đoán mapping nào là stack thread, mapping nào là việc khác; kiểm chứng bằng `/proc/<pid>/maps`.

## 2. Câu hỏi tự kiểm tra (về output lab)

1. Gọi `mmap` 64 MiB ẩn danh rồi ghi mỗi page một byte: bạn kỳ vọng bao nhiêu minor fault tại thời điểm `mmap`, và khi ghi? Vì sao lượt ghi thứ hai không fault?
<details><summary>Đáp án</summary>

Lab: 0 fault lúc `mmap`, 16384 khi ghi (= số page), 0 ở lượt hai. `mmap` chỉ tạo mapping[^mmap-new]; minor fault là fault không cần nạp từ đĩa[^proc-minflt]. Lượt hai không fault vì page đã được cấp trong lượt đầu (kết luận ở mức quan sát, mục 1.3).

</details>

2. Với glibc, 1 MiB `Vec<u8>` lần đầu đi vào `brk` hay `mmap`? Vì sao lần cấp thứ hai (sau free) có thể khác?
<details><summary>Đáp án</summary>

Lần đầu `mmap` (vượt ngưỡng mặc định 128 KiB[^mallopt-default]); sau khi free khối lớn hơn ngưỡng, ngưỡng động được nâng lên[^mallopt-dyn], nên lần sau có thể lấy từ heap (`brk`) — đúng như `strace` ở mục 1.4.

</details>

3. `Vec<u64>` push 1000 phần tử từ `Vec::new()` gọi allocator bao nhiêu lần, và đây có phải hợp đồng của `Vec` không?
<details><summary>Đáp án</summary>

Lab: 1 `alloc` + 8 `realloc`. Không phải hợp đồng: docs không đảm bảo chiến lược tăng, chỉ đảm bảo push amortized O(1)[^vec-growth][^vec-amort]; con số đến từ mã nguồn 1.95.0 (`max(cap*2, required)` với min cap 4 cho phần tử ≤ 1 KiB)[^rawvec-double][^rawvec-min].

</details>

4. Chạy `stack overflow` dưới `bash` trong container thì exit status là bao nhiêu, và nếu binary là PID 1 của container thì con số có đổi không?
<details><summary>Đáp án</summary>

Lab (mục 1.5): chạy dưới `bash` ra 134 = 128 + 6 (`SIGABRT`)[^bash-128n][^signal-nums], vì std gọi `abort`[^std-rtabort-macro][^abort3]. Khi binary là PID 1 của container thì ra 133; nguyên nhân (128 + 5 = `SIGTRAP`) là suy luận của người soạn chưa có nguồn, xem mục 1.5.

</details>

[^proc-perms]: proc(5), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "s = shared p = private (copy on write)"
[^proc-stack]: proc(5), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "The initial process's (also known as the main thread's) stack."
[^proc-heap]: proc(5), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "The process's heap. [anon:name] (since Linux 5.17) A named private anonymous mapping."
[^proc-anon]: proc(5), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "If the pathname field is blank, this is an anonymous mapping as obtained via mmap(2)."
[^mallopt-default]: mallopt(3), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/mallopt.3.en.html> — "Balancing these factors leads to a default setting of 128*1024 for the M_MMAP_THRESHOLD parameter."
[^kernel-aslr]: Linux kernel docs, sysctl kernel.* — <https://docs.kernel.org/admin-guide/sysctl/kernel.html> — "This option can be used to select the type of process address space randomization that is used in the system, for architectures that support this feature."
[^vec-triplet]: Rust std 1.95.0, `Vec` (Guarantees) — <https://doc.rust-lang.org/1.95.0/std/vec/struct.Vec.html> — "Most fundamentally, Vec is and always will be a (pointer, capacity, length) triplet."
[^opt-npo]: Rust std 1.95.0, `std::option` (Representation) — <https://doc.rust-lang.org/1.95.0/std/option/index.html> — "Rust guarantees to optimize the following types T such that Option\<T> has the same size, alignment, and function call ABI as T."
[^vec-new]: Rust std 1.95.0, `Vec::new` — <https://doc.rust-lang.org/1.95.0/std/vec/struct.Vec.html> — "The vector will not allocate until elements are pushed onto it."
[^rawvec-min]: rust-lang/rust tag 1.95.0, library/alloc/src/raw_vec/mod.rs, `min_non_zero_cap` — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/alloc/src/raw_vec/mod.rs> — "8 if the element size is 1, because any heap allocator is likely to round up a request of less than 8 bytes to at least 8 bytes. 4 if elements are moderate-sized (<= 1 KiB). 1 otherwise, to avoid wasting too much space for very short Vecs."
[^rawvec-double]: rust-lang/rust tag 1.95.0, library/alloc/src/raw_vec/mod.rs, `grow_amortized` — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/alloc/src/raw_vec/mod.rs> — "This guarantees exponential growth. The doubling cannot overflow because `cap <= isize::MAX`"
[^vec-exact]: Rust std 1.95.0, `Vec` (Guarantees) — <https://doc.rust-lang.org/1.95.0/std/vec/struct.Vec.html> — "Vec::with_capacity(n) produce a Vec that requests an allocation of the exact size needed for precisely n elements from the allocator"
[^box-zst]: Rust std 1.95.0, `Box::new` — <https://doc.rust-lang.org/1.95.0/std/boxed/struct.Box.html> — "This doesn’t actually allocate if T is zero-sized."
[^ga-zero]: Rust std 1.95.0, `GlobalAlloc::alloc` (Safety) — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html> — "layout must have non-zero size. Attempting to allocate for a zero-sized layout will result in undefined behavior."
[^rc-inner]: rust-lang/rust tag 1.95.0, library/alloc/src/rc.rs, `RcInner` — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/alloc/src/rc.rs> — "struct RcInner\<T: ?Sized> { strong: Cell\<usize>, weak: Cell\<usize>, value: T, }"
[^forget-safe]: Rust std 1.95.0, `mem::forget` (Safety) — <https://doc.rust-lang.org/1.95.0/std/mem/fn.forget.html> — "forget is not marked as unsafe, because Rust’s safety guarantees do not include a guarantee that destructors will always run."
[^ref-drop-scope]: The Rust Reference, Destructors — <https://doc.rust-lang.org/1.95.0/reference/destructors.html> — "all variables associated to that scope are dropped in reverse order of declaration (for variables) or creation (for temporaries)."
[^proc-minflt]: proc(5), Linux man-pages (Debian bookworm), /proc/pid/stat — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "The number of minor faults the process has made which have not required loading a memory page from disk."
[^mmap-new]: mmap(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/mmap.2.en.html> — "mmap() creates a new mapping in the virtual address space of the calling process."
[^mmap-anon]: mmap(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/mmap.2.en.html> — "The mapping is not backed by any file; its contents are initialized to zero."
[^mallopt-adv]: mallopt(3), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/mallopt.3.en.html> — "Allocating memory using mmap(2) has the significant advantage that the allocated memory blocks can always be independently released back to the system."
[^mallopt-dyn]: mallopt(3), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/mallopt.3.en.html> — "Nowadays, glibc uses a dynamic mmap threshold by default. The initial value of the threshold is 128*1024, but when blocks larger than the current threshold and less than or equal to DEFAULT_MMAP_THRESHOLD_MAX are freed, the threshold is adjusted upward to the size of the freed block."
[^mallopt-max]: mallopt(3), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/mallopt.3.en.html> — "The upper limit is DEFAULT_MMAP_THRESHOLD_MAX: 512*1024 on 32-bit systems or 4*1024*1024*sizeof(long) on 64-bit systems."
[^std-system]: Rust std 1.95.0, `std::alloc::System` — <https://doc.rust-lang.org/1.95.0/std/alloc/struct.System.html> — "This is based on malloc on Unix platforms and HeapAlloc on Windows, plus related functions."
[^rlimit-stack]: getrlimit(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/getrlimit.2.en.html> — "This is the maximum size of the process stack, in bytes. Upon reaching this limit, a SIGSEGV signal is generated."
[^std-thread-model]: Rust std 1.95.0, module `std::thread` — <https://doc.rust-lang.org/1.95.0/std/thread/index.html> — "An executing Rust program consists of a collection of native OS threads, each with their own stack and local state."
[^std-thread-stack]: Rust std 1.95.0, module `std::thread` (Stack size) — <https://doc.rust-lang.org/1.95.0/std/thread/index.html> — "Set the RUST_MIN_STACK environment variable to an integer representing the desired stack size (in bytes). Note that setting Builder::stack_size will override this."
[^std-guard]: rust-lang/rust tag 1.95.0, library/std/src/sys/pal/unix/stack_overflow.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/pal/unix/stack_overflow.rs> — "We've got guard pages (unmapped pages) at the end of every thread's stack, so if a thread ends up running into the guard page it'll trigger this handler."
[^std-rtabort-call]: rust-lang/rust tag 1.95.0, library/std/src/sys/pal/unix/stack_overflow.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/pal/unix/stack_overflow.rs> — "rtprintpanic!("\nthread '{name}' ({tid}) has overflowed its stack\n"); rtabort!("stack overflow");"
[^std-rtabort-macro]: rust-lang/rust tag 1.95.0, library/std/src/rt.rs, `rtabort!` — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/rt.rs> — "rtprintpanic!("fatal runtime error: {}, aborting\n", format_args!($($t)*)); crate::process::abort();"
[^std-abort-internal]: rust-lang/rust tag 1.95.0, library/std/src/sys/pal/unix/mod.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/pal/unix/mod.rs> — "pub fn abort_internal() -> ! { unsafe { libc::abort() } }"
[^abort3]: abort(3), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/abort.3.en.html> — "The abort() function first unblocks the SIGABRT signal, and then raises that signal for the calling process (as though raise(3) was called)."
[^bash-128n]: bash(1) (Debian bookworm) — <https://manpages.debian.org/bookworm/bash/bash.1.en.html> — "When a command terminates on a fatal signal N, bash uses the value of 128+N as the exit status."
[^signal-nums]: signal(7), bảng số hiệu tín hiệu (cột đầu là x86/ARM), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "SIGTRAP 5 5 5 5 SIGABRT 6 6 6 6"
[^std-process-abort]: Rust std 1.95.0, `std::process::abort` — <https://doc.rust-lang.org/1.95.0/std/process/fn.abort.html> — "As a consequence, no destructors on the current stack or any other thread’s stack will be run"
[^cargo-exec-replace]: rust-lang/cargo tag rust-1.95.0, crates/cargo-util/src/process_builder.rs, `exec_replace` (được `cargo run` gọi ở cuối src/cargo/ops/cargo_run.rs) — <https://raw.githubusercontent.com/rust-lang/cargo/rust-1.95.0/crates/cargo-util/src/process_builder.rs> — "Replaces the current process with the target process. On Unix, this executes the process using the Unix syscall `execvp`, which will block this process, and will only return if there is an error."
[^pidns-init]: pid_namespaces(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "Only signals for which the \"init\" process has established a signal handler can be sent to the \"init\" process by other members of the PID namespace."
[^globalalloc]: Rust std 1.95.0, `GlobalAlloc` — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html> — "A memory allocator that can be registered as the standard library’s default through the #[global_allocator] attribute."
[^vec-growth]: Rust std 1.95.0, `Vec` (Guarantees) — <https://doc.rust-lang.org/1.95.0/std/vec/struct.Vec.html> — "Vec does not guarantee any particular growth strategy when reallocating when full, nor when reserve is called."
[^vec-amort]: Rust std 1.95.0, `Vec` (Guarantees) — <https://doc.rust-lang.org/1.95.0/std/vec/struct.Vec.html> — "Whatever strategy is used will of course guarantee O(1) amortized push."
[^ref-drop-fields]: The Rust Reference, Destructors — <https://doc.rust-lang.org/1.95.0/reference/destructors.html> — "The fields of a struct are dropped in declaration order."
[^std-sigstack-flags]: rust-lang/rust tag 1.95.0, library/std/src/sys/pal/unix/stack_overflow.rs, `get_stack` — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/pal/unix/stack_overflow.rs> — "let flags = MAP_PRIVATE | MAP_ANON | libc::MAP_STACK;"
[^std-sigstack-unmap]: rust-lang/rust tag 1.95.0, library/std/src/sys/pal/unix/stack_overflow.rs, `drop_handler` — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/pal/unix/stack_overflow.rs> — "a mapping that started one page earlier, so walk back a page and unmap from there."
