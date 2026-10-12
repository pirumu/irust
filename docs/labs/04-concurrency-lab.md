# Lab P4 — Concurrency

> Phần thực hành của P4 · Concurrency. Lý thuyết nằm ở các chương của [P4](../04-concurrency/00-tong-quan.md) — **hãy đọc lý thuyết trước** rồi mới làm lab này; mỗi bài lab ghi rõ chương và mục cần đọc. Quy ước trích dẫn như `00-roadmap.md`: mọi fact có chú thích cuối trang với câu trích nguyên văn; phần ghi "(nhận xét của người soạn)" là lời khuyên/suy luận. Output lab bên dưới là output thật đã chạy trên máy người soạn (MacBook Apple M3 Pro, 11 core, macOS aarch64, rustc 1.95.0; container Linux aarch64 trong Docker, 4 CPU, cùng rustc 1.95) — **thời gian, PID, địa chỉ sẽ khác trên máy bạn**. Phần chỉ chạy được trên Linux (`futex_contention`/strace, trạng thái `/proc` của `deadlock`) có output từ container; macOS không có strace nên không có output tương ứng. Chưa chạy trên x86-64 (các nhận xét về memory model yếu chỉ dựa trên aarch64).

## Chuẩn bị và các bài lab

Crate `labs/crates/lab-concurrency`. Bin: `counter`, `spinlock`, `false_sharing_counters`, `sync_primitives`, `deadlock` (chạy được trên macOS), `futex_contention` (chỉ Linux). Thư viện chứa `SpinLock` ở `src/spinlock.rs` và các doctest `compile_fail`. Toàn bộ crate không có dependency ngoài std.

Chuẩn bị:

```bash
cd labs
cargo build --release -p lab-concurrency
cargo clippy -p lab-concurrency --all-targets -- -D warnings   # đã chạy: sạch, trên macOS và trong container
cargo test -p lab-concurrency                                    # unit test spinlock + doctest compile_fail
```

### Lab (a) — Lỗi biên dịch `Send`/`Sync` thật

Lý thuyết: [chương 08 mục 2](../04-concurrency/08-rust-concurrency.md) (`spawn`, `move`, `scope`, `F: Send + 'static`), [mục 3](../04-concurrency/08-rust-concurrency.md) (`Send`/`Sync`), [mục 4](../04-concurrency/08-rust-concurrency.md) (`Rc`/`Arc`), [mục 5](../04-concurrency/08-rust-concurrency.md) (`Cell`/`RefCell`).

Ba file trong `labs/crates/lab-concurrency/compile-fail/` **không thuộc target của crate**; biên dịch riêng bằng `rustc` (rustc 1.95.0, edition 2024). Mỗi lệnh thoát với exit code 1 (lỗi có chủ đích). `.stderr` đã lưu cạnh file `.rs`, đường dẫn thư mục home được thay bằng `~`.

**1. `Rc` chuyển vào thread** — `rustc --edition 2024 compile-fail/rc_across_threads.rs -o /tmp/p03-cf`:

```text
error[E0277]: `Rc<i32>` cannot be sent between threads safely
   --> compile-fail/rc_across_threads.rs:9:32
    |
  9 |     let handle = thread::spawn(move || println!("{clone}"));
    |                  ------------- -------^^^^^^^^^^^^^^^^^^^^
    |                  |             |
    |                  |             `Rc<i32>` cannot be sent between threads safely
    |                  |             within this `{closure@compile-fail/rc_across_threads.rs:9:32: 9:39}`
    |                  required by a bound introduced by this call
    |
    = help: within `{closure@compile-fail/rc_across_threads.rs:9:32: 9:39}`, the trait `Send` is not implemented for `Rc<i32>`
note: required because it's used within this closure
note: required by a bound in `spawn`
    |
128 |     F: Send + 'static,
    |        ^^^^ required by this bound in `spawn`
```

(rút gọn: bỏ vài dòng đường dẫn std; file `.stderr` có bản đầy đủ). Đọc lỗi: `thread::spawn` đòi closure `F: Send + 'static`; closure `move` bắt `Rc<i32>`, mà `Rc` không `Send`, nên closure không `Send`. Đây chính là cảnh báo trong sách: Rust Book cũng gặp lỗi `the trait Send is not implemented for Rc<Mutex<i32>>` khi thử chia sẻ `Rc`, và chuyển sang `Arc<T>` thì biên dịch được[^book-send-rc]. Lý do cụ thể mà std docs đưa ra: hai thread cùng clone `Rc` trỏ tới cùng giá trị có thể cập nhật reference count cùng lúc[^std-send-rc].

**2. `RefCell` chia sẻ giữa thread** — `compile-fail/refcell_shared.rs`:

```text
error[E0277]: `RefCell<i32>` cannot be shared between threads safely
   --> compile-fail/refcell_shared.rs:9:17
    |
  9 |         s.spawn(|| *cell.borrow_mut() += 1);
    |           ----- ^^^^^^^^^^^^^^^^^^^^^^^^^^ `RefCell<i32>` cannot be shared between threads safely
    |           |
    |           required by a bound introduced by this call
    |
    = help: the trait `Sync` is not implemented for `RefCell<i32>`
    = note: if you want to do aliasing and mutation between multiple threads, use `std::sync::RwLock` instead
    = note: required for `&RefCell<i32>` to implement `Send`
```

(rút gọn). Chuỗi suy luận trong chính thông báo: closure mượn `&RefCell<i32>`; muốn `&T: Send` thì cần `T: Sync` (đúng định nghĩa `Sync` ở [chương 08 mục 3](../04-concurrency/08-rust-concurrency.md)); `RefCell` không `Sync` nên bị từ chối. Std docs: `Cell`/`RefCell` có interior mutability dạng không thread-safe nên không `Sync`[^std-sync-cell].

**3. Hai `&mut` cùng lúc** — `compile-fail/two_mut_borrows.rs`:

```text
error[E0499]: cannot borrow `counter` as mutable more than once at a time
   --> compile-fail/two_mut_borrows.rs:9:17
    |
  7 |     thread::scope(|s| {
    |                    - has type `&'1 Scope<'1, '_>`
  8 |         s.spawn(|| counter += 1);
    |         ------------------------
    |         |       |  |
    |         |       |  first borrow occurs due to use of `counter` in closure
    |         |       first mutable borrow occurs here
    |         argument requires that `counter` is borrowed for `'1`
  9 |         s.spawn(|| counter += 1);
    |                 ^^ ------- second borrow occurs due to use of `counter` in closure
    |                 |
    |                 second mutable borrow occurs here
```

(rút gọn). Lỗi này không phải của `Send`/`Sync` mà của borrow checker: data race bị chặn ở quy tắc aliasing trước khi trait kịp vào cuộc. Sửa bằng `Mutex`, `AtomicUsize` hoặc để mỗi thread có biến riêng (lab (b)).

Trong `src/lib.rs` ba chương trình này cũng nằm trong doctest `compile_fail,E0277`/`E0499`; `cargo test -p lab-concurrency` pass chỉ khi cả ba bị từ chối đúng mã lỗi (đã chạy: 3 compile-fail + 1 doctest "đúng" pass).

### Lab (b) — Counter: Mutex vs Atomic vs SpinLock

Lý thuyết: [chương 03 mục 1](../04-concurrency/03-lock.md) (`counter += 1` là race condition), [mục 3](../04-concurrency/03-lock.md) (lệnh atomic và lock xây trên chúng), [mục 6](../04-concurrency/03-lock.md) (`std::sync::Mutex`), [mục 7](../04-concurrency/03-lock.md) (đo các lock trên cùng một bộ đếm), [chương 06 mục 3](../04-concurrency/06-memory-model.md) (data race), [chương 07 mục 1](../04-concurrency/07-lock-free.md) (`fetch_add`).

`counter.rs` chạy 4 thread, mỗi thread cộng 1 một triệu lần, theo sáu cách. Chạy trên macOS (M3 Pro, 11 core), exit code 0:

```bash
cd labs && ./target/release/counter      # = cargo run --release -p lab-concurrency --bin counter
```

```text
threads=4 iters/thread=1000000 available_parallelism=Some(11)
Mutex<u64>                   total=   4000000 expected=   4000000 ok               78.9 ms
SpinLock<u64> (ch.4)         total=   4000000 expected=   4000000 ok              344.2 ms
AtomicU64 fetch_add SeqCst   total=   4000000 expected=   4000000 ok               56.1 ms
AtomicU64 fetch_add Relaxed  total=   4000000 expected=   4000000 ok               52.8 ms
racy load+store Relaxed      total=   2227686 expected=   4000000 LOST UPDATES      3.2 ms
thread-local, 1 fetch_add    total=   4000000 expected=   4000000 ok                2.1 ms
```

Và trong container Linux (aarch64, 4 CPU), `./labs/linux.sh bash -c 'cd /work/labs; ./target-linux/release/counter'`, exit code 0:

```text
threads=4 iters/thread=1000000 available_parallelism=Some(4)
Mutex<u64>                   total=   4000000 expected=   4000000 ok               50.7 ms
SpinLock<u64> (ch.4)         total=   4000000 expected=   4000000 ok              482.1 ms
AtomicU64 fetch_add SeqCst   total=   4000000 expected=   4000000 ok               41.1 ms
AtomicU64 fetch_add Relaxed  total=   4000000 expected=   4000000 ok               22.0 ms
racy load+store Relaxed      total=   1528762 expected=   4000000 LOST UPDATES      3.5 ms
thread-local, 1 fetch_add    total=   4000000 expected=   4000000 ok                3.9 ms
```

Cách đọc (diễn giải của người soạn; con số là của một lần chạy, không phải benchmark nghiêm túc — đã không lặp nhiều lần hay cố định CPU affinity):

- **"racy load+store" mất cập nhật** (2.23M/4M ở macOS; 1.53M/4M ở Linux; số cụ thể thay đổi mỗi lần chạy). Mỗi `load` và `store` riêng lẻ đều atomic, nhưng cặp đọc-rồi-ghi thì không: thread khác có thể ghi xen giữa. Đây là race condition **không** phải data race, đúng như phân biệt ở [chương 03 mục 1](../04-concurrency/03-lock.md) và [chương 06 mục 3](../04-concurrency/06-memory-model.md). Nó "nhanh" (3 ms) chỉ vì làm sai.
- **`fetch_add`** là một phép đọc-sửa-ghi atomic duy nhất nên đủ; `SeqCst` vs `Relaxed` ở đây cùng đúng, vì ta chỉ cần tổng cuối cùng và `join` của scope đã đồng bộ việc đọc kết quả. Chênh lệch giữa hai dòng khác nhau theo máy (macOS 56,1 vs 52,8 ms; Linux 41,1 vs 22,0 ms) và từ một lần chạy không kết luận được `SeqCst` đắt hơn bao nhiêu.
- **Mutex chậm hơn `fetch_add`** khoảng 1,2–2,3× trong hai lần chạy, **SpinLock tự viết chậm hơn hẳn Mutex** của std khi 4 thread tranh một lock (344 ms và 482 ms so với 79 ms và 51 ms). Nhớ [chương 03 mục 4](../04-concurrency/03-lock.md) (quay, nhường CPU hay ngủ) và [mục 6](../04-concurrency/03-lock.md) (Mutex của std): std Mutex spin có giới hạn (100 vòng, xem `fn spin` trong source[^std-futex-spin100]) rồi ngủ, còn spin lock tự viết `swap` liên tục (suy đoán của người soạn: mỗi `swap` là một lệnh ghi vào cache line chung, nên có thể góp phần gây chậm; không đo riêng), trong khi `spin()` của std chỉ `load` để thân thiện với cache — đúng điều comment trong source nói[^std-futex-spin-load].
- **thread-local rồi một `fetch_add`**: nhanh nhất vì không chia sẻ gì trong lúc đếm. (nhận xét của người soạn) Bài học thiết kế: tránh chia sẻ rẻ hơn tối ưu cách chia sẻ.

### Lab (c) — `strace -f -e trace=futex` khi tranh chấp mutex

Lý thuyết: [chương 03 mục 5](../04-concurrency/03-lock.md) (futex: fast path ở user space, slow path ở kernel) và [mục 6](../04-concurrency/03-lock.md) (`Mutex` trên Linux: ba trạng thái, 100 vòng spin), [chương 07 mục 6](../04-concurrency/07-lock-free.md) (futex và atomic wait/notify).

`futex_contention.rs`: giữa các pha in một dòng marker ra stderr (mỗi dòng đúng một `write(2)` nên hiện trong strace): hai pha có đo (pha 1 — một thread khóa/mở `Mutex` 100.000 lần; pha 2 — 4 thread, mỗi thread 20.000 lần, có làm việc nhỏ trong critical section), cộng một marker `phase0` in địa chỉ mutex. Chạy trong container (exit code 0):

```bash
./labs/linux.sh bash -c 'cd /work/labs; cargo build -p lab-concurrency --bin futex_contention &&
  strace -f -e trace=futex,write -o /tmp/st.log ./target-linux/debug/futex_contention; wc -l /tmp/st.log'
```

Output chương trình (stderr + stdout):

```text
phase0 mutex at 0xffffc303b890, size 16 bytes
phase1 start: 1 thread, uncontended
phase1 end
phase2 start: 4 threads, contended
phase2 end
total = 180000 (expected 180000)
```

Phần đầu của `/tmp/st.log` (4 dòng đầu, tất cả là `write` marker; số đầu dòng là tid):

```text
16    write(2, "phase0 mutex at 0xffffc303b890, "..., 46) = 46
16    write(2, "phase1 start: 1 thread, unconten"..., 36) = 36
16    write(2, "phase1 end\n", 11)      = 11
16    write(2, "phase2 start: 4 threads, contend"..., 35) = 35
```

**Quan sát 1 — pha 1 không có futex.** Hai marker `phase1 start` và `phase1 end` nằm liền nhau trong log: 100.000 lần lock/unlock không sinh một `futex` nào. Đây là fast path ở [chương 03 mục 5](../04-concurrency/03-lock.md): một `compare_exchange` ở lần lock và một `swap` ở lần unlock, đều ở user space[^futex-uncontended][^std-futex-fast].

**Quan sát 2 — pha 2 có hàng nghìn futex.** Log có 8548 dòng; giữa `phase2 start` (dòng 4) và `phase2 end` (dòng 8545) toàn là futex. Một đoạn đầu tiên của pha 2 (rút gọn, tid khác nhau theo máy):

```text
12    futex(0xffffe9588f40, FUTEX_WAIT_BITSET_PRIVATE, 2, NULL, FUTEX_BITSET_MATCH_ANY <unfinished ...>
11    futex(0xffffe9588f40, FUTEX_WAKE_PRIVATE, 1 <unfinished ...>
12    <... futex resumed>)              = -1 EAGAIN (Resource temporarily unavailable)
11    <... futex resumed>)              = 0
12    futex(0xffffe9588f40, FUTEX_WAKE_PRIVATE, 1) = 0
```

Từng dấu hiệu khớp với source đã đọc:

- Tham số `FUTEX_WAIT_BITSET_PRIVATE` và `val = 2`: đúng `FUTEX_WAIT_BITSET | FUTEX_PRIVATE_FLAG` ở `sys/pal/unix/futex.rs`[^std-sys-futex], và `2` là `CONTENDED` mà `lock_contended` truyền vào `futex_wait`[^std-futex-wait]. (Dòng thứ 6 của log chỉ-futex, ở một lần chạy khác, có `val = 4294967295` trên một địa chỉ khác — người soạn suy luận (INFERENCE) đó không phải mutex của lab mà là futex nội bộ của thread/join; không đọc source để khẳng định.)
- `EAGAIN`: giá trị tại địa chỉ futex đã khác `2` lúc kernel so sánh, nên `FUTEX_WAIT` thất bại ngay[^futex-eagain]. Trong tranh chấp ngắn đây là chuyện thường: rất nhiều lời gọi ngủ thất bại vì lock vừa được nhả.
- `FUTEX_WAKE_PRIVATE, 1`: unlock thấy giá trị cũ là `CONTENDED` nên đánh thức một thread[^std-futex-unlock][^futex-wake].
- Địa chỉ `0xffffe9588f40` lặp lại ở mọi dòng futex của mutex trong lần chạy đó, nên đó là địa chỉ futex word của `m`; mỗi lần chạy một địa chỉ khác (excerpt này lấy từ lần chạy khác với lần in `phase0 mutex at ...` ở trên).

Đếm trên một lần chạy khác (số liệu thay đổi mỗi lần): `FUTEX_WAIT` được bắt đầu 1161 lần, `FUTEX_WAKE` 1476 lần, và 1149 lần trả `EAGAIN`; với `strace -f -c -e trace=futex` một lần khác: `2898` lời gọi, `1241` lỗi. Tổng `180000` đúng bằng kỳ vọng nên mutex vẫn bảo vệ được dữ liệu dù strace làm chương trình chậm đi nhiều.

Lưu ý khi dùng strace: `-f` theo dõi mọi thread, và strace làm thay đổi thời gian biểu của chương trình (nhận xét của người soạn), nên đừng dùng số lần đếm để suy ra hiệu năng thật.

### Lab (d) — Spin lock Acquire/Release (Rust Atomics and Locks, chương 4)

Lý thuyết: [chương 06 mục 5](../04-concurrency/06-memory-model.md) (`Relaxed`, `Release`/`Acquire`), [mục 7](../04-concurrency/06-memory-model.md) (spin lock bằng Acquire/Release), [mục 4](../04-concurrency/06-memory-model.md) (happens-before); [chương 03 mục 4](../04-concurrency/03-lock.md) (spin hay ngủ).

`src/spinlock.rs` cài `SpinLock<T>` theo chương 4: `lock` dùng `swap(true, Acquire)` và `spin_loop()` khi giá trị cũ vẫn là `true`; guard `Drop` thì `store(false, Release)`. Đây là đúng cặp mà sách mô tả: swap `Acquire` trong `lock`, store `Release` trong `unlock`[^atomics-spin-code], còn `spin_loop` là gợi ý cho bộ xử lý rằng ta đang quay chờ[^atomics-spin-hint]. Phần `unsafe impl<T: Send> Sync for SpinLock<T>` đúng vì lock chỉ trao `&mut T` cho một thread tại một thời điểm, nên chia sẻ lock chỉ di chuyển `T` giữa các thread; sách chương 4 chọn đúng ràng buộc `T: Send` với lý do này[^atomics-spin-sync].

`spinlock.rs` (bin) có ba phần. Chạy trên macOS (exit code 0):

```text
cpus=11 iters/thread=1000000 arch=aarch64
-- part 1: SpinLock (Acquire/Release), 4 threads
total=4000000 expected=4000000 in 373.929416ms
-- part 2: BrokenRelaxedSpinLock (all Relaxed), 4 threads, 5 rounds
round 1: total=3999909 expected=4000000 lost=91
round 2: total=3999900 expected=4000000 lost=100
round 3: total=3999834 expected=4000000 lost=166
round 4: total=3999884 expected=4000000 lost=116
round 5: total=3999888 expected=4000000 lost=112
-- part 3: 44 threads (4 x cpus), 50000 iters/thread
SpinLock: total=2200000 in 894.180083ms
Mutex:    total=2200000 in 43.509791ms
```

Trong container Linux aarch64 (4 CPU), exit code 0: part 1 đúng (`total=4000000`), part 2 mất `5674, 5928, 3396, 1019, 2180` lần cộng trong 5 vòng; part 3 (16 thread): `SpinLock` 17,35 ms, `Mutex` 5,24 ms.

Giải thích:

- **Part 1**: Acquire/Release đủ để giữ đúng 4.000.000 — happens-before giữa `unlock` và `lock` kế tiếp làm cho các ghi của critical section trước hiển thị với critical section sau[^atomics-spin-hb].
- **Part 2** là phản ví dụ cố ý: cùng thuật toán nhưng mọi ordering thành `Relaxed` (`BrokenRelaxedSpinLock`). Không còn happens-before nào giữa unlock và lock, nên các truy cập thường vào dữ liệu là data race — tức UB[^std-datarace] — và trên máy này thấy hậu quả cụ thể là mất cập nhật. Cả macOS lẫn container ở đây là aarch64, kiến trúc có memory model yếu[^nomicon-weak]; số lần mất khác nhau mỗi lần chạy. **Cảnh báo (nhận xét của người soạn):** data race là UB nên "không mất cập nhật" không chứng minh code đúng — phần này chỉ để thấy một triệu chứng khả dĩ, không bao giờ copy `BrokenRelaxedSpinLock`. Người soạn không kiểm chứng trên x86, nên không khẳng định hành vi ở đó.
- **Part 3**: nhiều thread hơn CPU (4×). Spin lock chậm hơn Mutex rất nhiều (894 ms so với 44 ms ở macOS), nhất quán với phân tích của OSTEP: khi thread nắm lock bị preempt, mọi thread còn lại quay hết cả time slice[^ostep-spin-preempt], và spin-waiting đặc biệt phí khi thread được chờ không chạy được[^ostep-uni]. Mutex của std ngủ sau khi spin có giới hạn[^std-futex-wait] nên (suy luận của người soạn) thread chờ không đốt hết time slice như vậy.

### Lab thêm — False sharing

Lý thuyết: [chương 06 mục 2](../04-concurrency/06-memory-model.md) (cache coherence), [P2 · chương 06 — cache và NUMA](../02-cpu/06-cache-va-numa.md) (cache line).

`false_sharing_counters.rs` (`cargo run --release -p lab-concurrency --bin false_sharing_counters`): hai thread, mỗi thread cập nhật counter riêng 50 triệu lần bằng `fetch_add(Relaxed)`. Khác biệt duy nhất: hai counter nằm liền kề (cách 8 byte) hay mỗi cái bọc trong `#[repr(align(128))]`. macOS, exit code 0:

```text
iters/thread=50000000
adjacent (addresses   8 bytes apart):    526.1 ms
padded   (addresses 128 bytes apart):    105.1 ms
```

Container Linux (cùng lệnh, exit code 0): `adjacent` 394,8 ms; `padded` 139,6 ms. Hai thread không dùng chung biến nào nhưng vẫn chậm hơn khoảng 2,8–5× khi hai counter chung cache line; đây đúng là hiện tượng sách mô tả[^atomics-false-sharing] (diễn giải: lệnh ghi của core này làm invalid line ở core kia theo giao thức coherence ở [chương 06 mục 2](../04-concurrency/06-memory-model.md); sách mô tả giao thức, còn việc gán nguyên nhân cho từng con số của máy người soạn thì không đo trực tiếp). Số liệu là của một lần chạy.

### Lab thêm — Condvar, mpsc, poisoning (`sync_primitives`)

Lý thuyết: [chương 04 mục 2](../04-concurrency/04-condition-variable-va-semaphore.md) (`wait`), [mục 3](../04-concurrency/04-condition-variable-va-semaphore.md) (`while` chứ không `if`), [mục 4](../04-concurrency/04-condition-variable-va-semaphore.md) (producer/consumer), [chương 08 mục 6](../04-concurrency/08-rust-concurrency.md) (`Mutex`, `Condvar`, poisoning) và [mục 9](../04-concurrency/08-rust-concurrency.md) (`mpsc`).

macOS, exit code 0 (rút gọn đoạn đầu):

```text
-- 1. Condvar bounded queue (capacity 2)
  producer pushed 0 (len 1)
  producer pushed 1 (len 2)
  consumer popped 0 (len 1)
  ...
-- 2. mpsc: 3 producers, 1 consumer
  producer 0 message 0
  ...
  received 6 messages; rx.recv() now = Err(RecvError)
-- 3. Mutex poisoning
thread '<unnamed>' (133114561) panicked at crates/lab-concurrency/src/bin/sync_primitives.rs:79:13:
panic while holding the lock
  worker join() is_err = true
  is_poisoned() = true
  lock() = Err(PoisonError); into_inner() still gives the data: [1, 2, 3, 4]
```

- (1) Producer chờ trong vòng `while q.len() == CAPACITY { q = not_full.wait(q).unwrap(); }`: `wait` nhả mutex và ngủ một cách atomic[^std-condvar-atomic], và vòng `while` bảo vệ trước spurious wakeup[^std-condvar-spurious][^ostep-while].
- (2) `rx.iter()` kết thúc vì mọi `Sender` (kể cả bản gốc đã `drop(tx)`) bị drop: nửa còn lại "hung up"[^std-mpsc-hungup]. Thứ tự giữa các producer là không xác định nên bin sắp xếp trước khi in.
- (3) Thread panic khi đang giữ guard ⇒ mutex bị đánh dấu poisoned[^std-poison]; `lock()` trả `Err(PoisonError)` nhưng dữ liệu vẫn lấy được qua `into_inner()` (quan sát từ chính output này). Mặc định std coi dữ liệu có thể "tainted" nên chặn truy cập[^std-poison-tainted].

### Lab thêm — Deadlock

Lý thuyết: [chương 05 mục 3](../04-concurrency/05-concurrency-bug-va-deadlock.md) (deadlock), [mục 4](../04-concurrency/05-concurrency-bug-va-deadlock.md) (bốn điều kiện Coffman, lock ordering), [mục 6](../04-concurrency/05-concurrency-bug-va-deadlock.md) (deadlock trong Rust).

`deadlock.rs`: t1 khóa A rồi B, t2 khóa B rồi A, một `Barrier` bảo đảm cả hai đã giữ khóa đầu trước khi xin khóa thứ hai. Trong container (exit code 0):

```text
-- consistent order: both threads lock A then B
  t1 A->B finished
  t2 A->B finished
  completed = true
-- opposite order: t1 locks A then B, t2 locks B then A
  completed = false
  no progress after 2 s: deadlock (circular wait). Threads of this process:
  tid     94 comm=deadlock   state=R wchan=0
  tid     97 comm=t1 A->B    state=S wchan=futex_wait_queue
  tid     98 comm=t2 B->A    state=S wchan=futex_wait_queue
```

Hai thread kia ở trạng thái `S` (ngủ có thể ngắt)[^proc-state], tức không tốn CPU; cột `wchan` là địa chỉ/tên hàm trong kernel nơi tiến trình đang chờ[^proc-wchan], ở đây là hàng đợi của futex — đúng với [chương 03 mục 5](../04-concurrency/03-lock.md): cả hai ngủ trong `futex_wait` của Mutex. Cả bốn điều kiện deadlock đều thỏa[^ostep-dl4]; lấy khóa cùng thứ tự (nhánh đầu) thì không deadlock[^ostep-order]. Trên macOS bin chạy được nhưng không in trạng thái thread (không có `/proc`).

### Bài tập mở rộng

1. Sửa `Mutex::lock().unwrap()` trong `futex_contention` thành `try_lock` rồi quan sát futex có còn xuất hiện không (nhận xét của người soạn: dự đoán trước khi chạy).
2. Đổi `fetch_add(Relaxed)` thành `SeqCst` trong `false_sharing_counters` và đo lại; giải thích bằng các bảo đảm ordering ở [chương 06 mục 5](../04-concurrency/06-memory-model.md) và [mục 6](../04-concurrency/06-memory-model.md) (`SeqCst`).
3. Trong `spinlock.rs`, đổi riêng `Release` thành `Relaxed` (giữ `Acquire`) rồi chạy nhiều lần; dựa vào chuỗi lập luận ở [chương 06 mục 7](../04-concurrency/06-memory-model.md) giải thích vì sao thiếu một phía là đủ làm mất happens-before. Nhớ: kết quả "không lỗi" không chứng minh gì.
4. Viết lại lock-order bằng cách luôn lấy lock theo địa chỉ tăng dần; xác nhận `deadlock` không treo.
5. Chạy `strace -f -c` cho `counter` (bản debug) và tìm xem bin nào gọi futex nhiều nhất.

## Câu hỏi tự kiểm tra (về output lab)

1. Trong lab (c), vì sao pha không tranh chấp không sinh `futex` nào?
<details><summary>Đáp án</summary>

Fast path của `std::sync::Mutex` chỉ là một `compare_exchange` Acquire, không syscall[^std-futex-fast]; ở trường hợp không tranh chấp, thread đổi trạng thái bằng lệnh atomic hoàn toàn ở user mode và kernel không biết gì về lock[^futex-uncontended].

</details>

2. Dòng "racy load+store" ở lab (b) mất cập nhật dù chỉ dùng atomic. Đó là data race hay race condition?
<details><summary>Đáp án</summary>

Không có truy cập non-atomic nên không phải data race theo định nghĩa của std[^std-datarace]; đó là race condition vì khoảng giữa `load` và `store` không atomic, còn `fetch_add` là một thao tác đọc-sửa-ghi duy nhất[^atomics-fetch].

</details>

3. Ở lab `deadlock`, `wchan` của hai thread kẹt cho biết điều gì, và cách sửa nào đúng theo OSTEP?
<details><summary>Đáp án</summary>

`wchan` là "kênh" mà tiến trình đang chờ, địa chỉ trong kernel nơi nó ngủ[^proc-wchan]; ở đây cả hai ngủ trong hàng đợi futex. Đủ bốn điều kiện thì deadlock[^ostep-dl4]; cách thực tế nhất là bắt mọi thread lấy lock cùng thứ tự[^ostep-order].

</details>

4. Ở `false_sharing_counters`, hai thread không dùng chung biến nào mà bản `adjacent` vẫn chậm hơn bản `padded`. Điều gì khác nhau giữa hai bản?
<details><summary>Đáp án</summary>

Chỉ khác vị trí: hai counter cách 8 byte (cùng cache line) so với cách 128 byte nhờ `#[repr(align(128))]`. Lab: 526,1 ms so với 105,1 ms trên macOS, 394,8 ms so với 139,6 ms trong container. Sách gọi hiện tượng này là false sharing[^atomics-false-sharing]; việc gán nguyên nhân cho từng con số cụ thể là diễn giải, không đo trực tiếp.

</details>

5. Ở lab (d) part 2, vì sao `BrokenRelaxedSpinLock` mất cập nhật còn `SpinLock` thì không, và "không mất cập nhật" có chứng minh bản `Relaxed` đúng không?
<details><summary>Đáp án</summary>

`SpinLock` dùng `Acquire` ở `swap` và `Release` ở `store`, tạo happens-before giữa unlock và lock kế tiếp[^atomics-spin-hb]. Bản `Relaxed` không có happens-before đó nên các truy cập thường vào dữ liệu là data race, tức UB[^std-datarace]; aarch64 có memory model yếu[^nomicon-weak]. Không thấy mất cập nhật trong một lần chạy không chứng minh code đúng (nhận xét của người soạn).

</details>

[^book-send-rc]: The Rust Programming Language, 16.4 Extensible Concurrency with Sync and Send — <https://doc.rust-lang.org/1.95.0/book/ch16-04-extensible-concurrency-sync-and-send.html> — "When we switched to Arc\<T>, which does implement Send, the code compiled."
[^std-send-rc]: Rust std 1.95.0, `std::marker::Send` — <https://doc.rust-lang.org/1.95.0/std/marker/trait.Send.html> — "An example of a non-Send type is the reference-counting pointer rc::Rc. If two threads attempt to clone Rcs that point to the same reference-counted value, they might try to update the reference count at the same time"
[^std-sync-cell]: Rust std 1.95.0, `std::marker::Sync` — <https://doc.rust-lang.org/1.95.0/std/marker/trait.Sync.html> — "The method performs no synchronization, thus Cell cannot be Sync."
[^std-futex-spin100]: rust-lang/rust tag 1.95.0, library/std/src/sys/sync/mutex/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/sync/mutex/futex.rs> — "fn spin(&self) -> State { let mut spin = 100;"
[^std-futex-spin-load]: rust-lang/rust tag 1.95.0, library/std/src/sys/sync/mutex/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/sync/mutex/futex.rs> — "// We only use `load` (and not `swap` or `compare_exchange`) while spinning, to be easier on the caches."
[^futex-uncontended]: futex(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/futex.2.en.html> — "In the uncontended case, a thread can access or modify the lock state with atomic instructions, for example atomically changing it from not acquired to acquired using an atomic compare-and-exchange instruction."
[^std-futex-fast]: rust-lang/rust tag 1.95.0, library/std/src/sys/sync/mutex/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/sync/mutex/futex.rs> — "if self.futex.compare_exchange(UNLOCKED, LOCKED, Acquire, Relaxed).is_err() { self.lock_contended(); }"
[^std-sys-futex]: rust-lang/rust tag 1.95.0, library/std/src/sys/pal/unix/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/pal/unix/futex.rs> — "libc::FUTEX_WAIT_BITSET | libc::FUTEX_PRIVATE_FLAG,"
[^std-futex-wait]: rust-lang/rust tag 1.95.0, library/std/src/sys/sync/mutex/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/sync/mutex/futex.rs> — "// Wait for the futex to change state, assuming it is still CONTENDED. futex_wait(&self.futex, CONTENDED, None);"
[^futex-eagain]: futex(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/futex.2.en.html> — "If the futex value does not match val, then the call fails immediately with the error EAGAIN. The purpose of the comparison with the expected value is to prevent lost wake-ups."
[^std-futex-unlock]: rust-lang/rust tag 1.95.0, library/std/src/sys/sync/mutex/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/sync/mutex/futex.rs> — "if self.futex.swap(UNLOCKED, Release) == CONTENDED {"
[^futex-wake]: futex(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/futex.2.en.html> — "This operation wakes at most val of the waiters that are waiting (e.g., inside FUTEX_WAIT) on the futex word at the address uaddr."
[^atomics-spin-code]: Rust Atomics and Locks, ch.4 Building Our Own Spin Lock — <https://marabos.nl/atomics/building-spinlock.html> — "while self.locked.swap(true, Acquire) { std::hint::spin_loop(); } } pub fn unlock(&self) { self.locked.store(false, Release); }"
[^atomics-spin-hint]: Rust Atomics and Locks, ch.4 Building Our Own Spin Lock — <https://marabos.nl/atomics/building-spinlock.html> — "we use a spin loop hint, which tells the processor that we're spinning while waiting for something to change."
[^atomics-spin-sync]: Rust Atomics and Locks, ch.4 Building Our Own Spin Lock — <https://marabos.nl/atomics/building-spinlock.html> — "Note that we don't need to require that T is Sync, because our SpinLock\<T> will only allow one thread at a time to access the T it protects."
[^atomics-spin-hb]: Rust Atomics and Locks, ch.4 Building Our Own Spin Lock — <https://marabos.nl/atomics/building-spinlock.html> — "We use acquire and release memory ordering to make sure that every unlock() call establishes a happens-before relationship with the lock() calls that follow."
[^std-datarace]: Rust std 1.95.0, module `std::sync::atomic` — <https://doc.rust-lang.org/1.95.0/std/sync/atomic/index.html> — "The most important aspect of this model is that data races are undefined behavior. A data race is defined as conflicting non-synchronized accesses where at least one of the accesses is non-atomic."
[^nomicon-weak]: The Rustonomicon, Atomics — <https://doc.rust-lang.org/1.95.0/nomicon/atomics.html> — "Most notably x86/64 provides strong ordering guarantees, while ARM provides weak ordering guarantees."
[^ostep-spin-preempt]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "imagine the case where the thread holding the lock is preempted within a critical section. The scheduler might then run every other thread (imagine there are N − 1 others), each of which tries to acquire the lock."
[^ostep-uni]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "The waste is exceptionally high on a uniprocessor, where the thread that the waiter is waiting for cannot even run"
[^atomics-false-sharing]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "This effect is called false sharing. We can avoid this by spacing the atomic variables further apart, so they each get their own cache line."
[^std-condvar-atomic]: Rust std 1.95.0, `std::sync::Condvar::wait` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Condvar.html> — "This function will atomically unlock the mutex specified (represented by guard) and block the current thread."
[^std-condvar-spurious]: Rust std 1.95.0, `std::sync::Condvar` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Condvar.html> — "Note that this function is susceptible to spurious wakeups. Condition variables normally have a boolean predicate associated with them, and the predicate must always be checked each time this function returns to protect against spurious wakeups."
[^ostep-while]: OSTEP, ch.30 Condition Variables — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-cv.pdf> — "a simple rule to remember with condition variables is to always use while loops."
[^std-mpsc-hungup]: Rust std 1.95.0, module `std::sync::mpsc` — <https://doc.rust-lang.org/1.95.0/std/sync/mpsc/index.html> — "An unsuccessful operation is normally indicative of the other half of a channel having “hung up” by being dropped in its corresponding thread."
[^std-poison]: Rust std 1.95.0, `std::sync::Mutex` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html> — "a mutex becomes poisoned if it recognizes that the thread holding it has panicked."
[^std-poison-tainted]: Rust std 1.95.0, `std::sync::Mutex` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html> — "Once a mutex is poisoned, all other threads are unable to access the data by default as it is likely tainted (some invariant is not being upheld)."
[^proc-state]: proc(5), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "S Sleeping in an interruptible wait"
[^proc-wchan]: proc(5), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "This is the \"channel\" in which the process is waiting. It is the address of a location in the kernel where the process is sleeping."
[^ostep-dl4]: OSTEP, ch.32 Common Concurrency Problems — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-bugs.pdf> — "Four conditions need to hold for a deadlock to occur"
[^ostep-order]: OSTEP, ch.32 Common Concurrency Problems — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-bugs.pdf> — "if Thread 1 and 2 both made sure to grab locks in the same order, the deadlock would never arise."
[^atomics-fetch]: Rust Atomics and Locks, ch.2 Atomics — <https://marabos.nl/atomics/atomics.html> — "also load (fetch) the original value, as a single atomic operation. The most commonly used ones are fetch_add and fetch_sub"
