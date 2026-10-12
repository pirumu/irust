# P4 · Chương 03 — Race condition và lock

## Bạn sẽ hiểu được gì

1. **Q1.** Vì sao `counter += 1` là một race condition, nó biên dịch ra những lệnh máy nào, và critical section, mutual exclusion, atomicity khác nhau thế nào? → mục 1
2. **Q2.** Một lock tốt được đánh giá theo ba tiêu chí nào? Vì sao tắt interrupt hay "một cờ + load/store thường" đều không đủ? → mục 2
3. **Q3.** Test-and-set, compare-and-swap, LL/SC, fetch-and-add là gì, và ticket lock dùng chúng ra sao? → mục 3
4. **Q4.** Spin, yield, ngủ trong hàng đợi, two-phase lock: mỗi cách tốn gì? → mục 4
5. **Q5.** `futex(2)` giải quyết "wakeup/waiting race" thế nào? → mục 5
6. **Q6.** `std::sync::Mutex` trên Linux dùng futex ra sao (3 trạng thái, 100 vòng spin)? → mục 6
7. **Q7.** Các lock khác nhau đo ra sao trên cùng một bộ đếm? → mục 7
8. **Q8.** Một lock to và nhiều lock nhỏ khác nhau thế nào; reader-writer lock giải quyết gì? → mục 8

## Kiến thức cần trước

- [Chương 01 — Thread](01-thread.md) và [Chương 02 — Scheduling](02-scheduling.md): thread có register riêng nhưng chung bộ nhớ, timer interrupt có thể chuyển CPU giữa hai lệnh bất kỳ.
- [Assembly x86-64 và AArch64](../02-cpu/03-assembly-x86-64-va-aarch64.md): đọc được `mov`, `add`, `ldr`, `str`.

## 1. Race condition: `counter += 1` không phải một bước (Q1)

> **Trả lời ngắn.** `counter += 1` không phải một bước mà là ba: nạp từ bộ nhớ, cộng, ghi lại[^ostep-ctr-seq]. Nếu thread khác chen vào giữa, một lần cập nhật bị mất và kết quả không biết trước[^ostep-lost-update][^ostep-indeterminate]. Critical section là đoạn code không được chạy đồng thời[^ostep-cs]; mutual exclusion là tính chất ngăn thread khác vào[^ostep-mutex]; atomicity là chạy như một đơn vị[^ostep-atomic-def].

**Giải thích.**

Hình dung (nhận xét của người soạn): hai nhân viên cùng sửa một con số trên bảng bằng cách đọc, tính ra giấy nháp rồi chép lại; ai chép sau ghi đè cập nhật của người kia.

**Ba lệnh thay vì một.**

Hai thread cùng tăng một biến chung. OSTEP chỉ ra rằng mã máy x86 cho "cộng 1 vào counter" có thể là ba lệnh: nạp từ bộ nhớ vào register, cộng, ghi lại[^ostep-ctr-seq].

```mermaid
sequenceDiagram
  participant T1 as Thread 1
  participant M as counter (bộ nhớ)
  participant T2 as Thread 2
  Note over M: counter = 50
  T1->>M: load → eax = 50
  Note over T1: bị ngắt (timer interrupt)
  T2->>M: load → eax = 50
  T2->>M: store 51
  T1->>M: store 51 (từ eax cũ)
  Note over M: 2 lần tăng, kết quả 51 — mất một cập nhật
```

(Biểu đồ là minh họa của người soạn cho kịch bản mất cập nhật; OSTEP mô tả đúng kịch bản này với giá trị khởi đầu 50[^ostep-lost-update].) Kết quả phụ thuộc thời điểm thực thi — OSTEP gọi là *race condition* (cụ thể hơn là data race)[^ostep-race], và kết quả là "indeterminate": không biết trước, và có thể khác nhau giữa các lần chạy[^ostep-indeterminate].


**Ba khái niệm cần phân biệt.**

- **Critical section**: đoạn code truy cập biến/tài nguyên chung và không được chạy đồng thời bởi nhiều hơn một thread[^ostep-cs].
- **Mutual exclusion**: tính chất bảo đảm nếu một thread đang trong critical section thì các thread khác bị ngăn không cho vào[^ostep-mutex].
- **Atomicity**: "atomically" nghĩa là "as a unit", đôi khi hiểu là "all or none"[^ostep-atomic-def]. Thứ ta muốn là cả chuỗi ba lệnh chạy như một đơn vị; OSTEP đặt câu hỏi liệu phần cứng có nên có lệnh "cập nhật nguyên B-tree" và trả lời "Probably not", nên ta xin vài lệnh hữu ích rồi dựng các *synchronization primitive* từ đó[^ostep-sync-prims].


<details><summary>Đi sâu: assembly thật và race condition so với data race</summary>

**Nhìn vào assembly thật.**

Ba hàm Rust dưới đây tăng một `AtomicU64` theo ba cách. Tôi biên dịch với `rustc -C opt-level=3 --emit asm` (rustc 1.95.0) cho x86-64 và AArch64; đây là kết quả quan sát trên máy người soạn, không phải bảo đảm của ngôn ngữ:

```rust
use std::sync::atomic::{AtomicU64, Ordering::*};
pub fn racy(c: &AtomicU64)       { let v = c.load(Relaxed); c.store(v + 1, Relaxed); }
pub fn atomic_add(c: &AtomicU64) { c.fetch_add(1, Relaxed); }
pub fn cas_loop(c: &AtomicU64)   { let mut cur = c.load(Relaxed);
    loop { match c.compare_exchange_weak(cur, cur + 1, Relaxed, Relaxed) { Ok(_) => break, Err(v) => cur = v } } }
```

| Hàm | x86-64 (`x86_64-unknown-linux-gnu`) | AArch64 (`aarch64-apple-darwin`) |
|---|---|---|
| `racy` (load rồi store) | `incq (%rdi)` — một lệnh, **không** có tiền tố `lock` | `ldr` → `add` → `str` |
| `atomic_add` (`fetch_add`) | `lock incq (%rdi)` | `ldadd x8, x8, [x0]` |
| `cas_loop` | `lock cmpxchgq %rcx, (%rdi)` + `jne` quay lại | `cas x8, x10, [x0]` + `b.ne` quay lại |

Hai điều rút ra:

1. Trên AArch64, `racy` là đúng ba bước rời như OSTEP mô tả. Trên x86-64, trình biên dịch gộp thành *một* lệnh `inc` trên bộ nhớ — nhưng một lệnh không có nghĩa là atomic giữa các core: *Rust Atomics and Locks* nói rõ phiên bản x86-64 của phép cộng non-atomic vẫn "not atomic" vì lệnh `add` bị bộ xử lý tách thành nhiều vi lệnh[^atomics-x86-notatomic]. Mục 7 đo `racy` trên máy lab aarch64 (không phải x86-64), nên chỉ quan sát được mất cập nhật ở AArch64; với x86-64 ta chỉ có lập luận của sách.
2. Phần cứng cung cấp lệnh atomic thật: x86 có tiền tố `lock` biến các lệnh như `add`, `sub`, `and`, `or`, `xor` thành atomic[^atomics-lockprefix]; ARMv8.1 có `ldadd` tương đương `fetch_add` và `cas` tương đương `compare_exchange`[^atomics-ldadd].

Bảng là cho `aarch64-apple-darwin`; trong container Linux aarch64 (target mặc định) `fetch_add` thành `bl __aarch64_ldadd8_relax`, tức gọi hàm helper (quan sát của người review).

**Race condition không đồng nghĩa data race.**

*Data race* là khái niệm hẹp của memory model. Docs `std::sync::atomic` của Rust định nghĩa nó là các truy cập xung đột, không đồng bộ, trong đó ít nhất một truy cập là non-atomic; và data race là undefined behavior[^std-datarace]. Nomicon phát biểu bằng ba điều kiện: hai thread trở lên truy cập đồng thời cùng một vị trí nhớ, ít nhất một là ghi, ít nhất một không đồng bộ[^nomicon-race-def]. (Hai cách phát biểu khác chữ nhau; người soạn không khẳng định chúng trùng. Cả khóa dùng định nghĩa của `std`; phần so sánh nằm ở [chương 06, mục 3](06-memory-model.md).)

*Race condition* là bug logic về thứ tự thời gian, rộng hơn. Nomicon nói Rust không ngăn được race condition nói chung[^nomicon-race-cond]. Ví dụ ngay trong bảng trên: `racy` chỉ dùng atomic nên không có data race, nhưng vẫn mất cập nhật vì `load` rồi `store` là hai thao tác tách rời; thao tác đúng là một lệnh đọc-sửa-ghi atomic như `fetch_add`[^atomics-fetch].

</details>

**Quay lại Q1:** `counter += 1` là chuỗi nhiều bước nên cần mutual exclusion hoặc một thao tác atomic thật sự.

## 2. Lock: giao diện, ba tiêu chí và hai lối tắt thất bại (Q2)

> **Trả lời ngắn.** Lock là một biến có hai trạng thái, rảnh hoặc đang bị giữ[^ostep-lock-states]; thread gọi `lock()` khi lock đang bị giữ phải chờ đến khi nó được nhả[^ostep-lock-block]. Lock được đánh giá theo tính đúng, fairness và performance[^ostep-lock-criteria][^ostep-lock-fairness][^ostep-lock-perf]. Một cờ với load/store thường không đủ vì "kiểm tra rồi đặt" lại là hai bước[^ostep-flag-fail]; tắt interrupt thì không chạy trên đa xử lý[^ostep-intr-mp].

**Giải thích.**

Hình dung (nhận xét của người soạn): khóa cửa phòng họp — nếu hai người cùng thấy cửa "trống" rồi cùng đẩy cửa vào thì tấm biển không có tác dụng; việc kiểm tra và khóa phải là một động tác không thể chen vào.

**Giao diện.**

Một lock chỉ là một biến giữ trạng thái: *available* (unlocked/free — không thread nào giữ) hoặc *acquired* (locked/held — đúng một thread giữ và có lẽ đang ở trong critical section)[^ostep-lock-states]. Ngữ nghĩa: nếu thread khác gọi `lock()` trên cùng biến khi lock đang bị giữ, lời gọi không trả về cho đến khi lock được nhả[^ostep-lock-block]. Với ví dụ `balance = balance + 1;`, ta bọc `lock(&mutex); … unlock(&mutex);` quanh nó.


**Ba tiêu chí đánh giá.**

OSTEP đánh giá một lock theo ba tiêu chí:

1. **Đúng**: có làm đúng việc cơ bản là mutual exclusion không[^ostep-lock-criteria].
2. **Fairness**: mỗi thread tranh lock có cơ hội công bằng khi lock rảnh không, hay có thread bị đói (starve)[^ostep-lock-fairness].
3. **Performance**: chi phí thời gian mà lock thêm vào[^ostep-lock-perf]. Sách tách ra ba tình huống: một thread không tranh chấp, nhiều thread tranh chấp trên một CPU, và nhiều thread tranh chấp trên nhiều CPU[^ostep-lock-perf-cases].


**Một cờ và load/store thường.** Thử xây lock chỉ bằng một biến `flag`: `lock()` quay `while (flag == 1)` rồi đặt `flag = 1`; `unlock()` đặt `flag = 0`. OSTEP chỉ ra lỗi đúng-sai: nếu một interrupt chen vào giữa lúc Thread 1 thấy `flag == 0` và lúc nó ghi `flag = 1`, thì cả hai thread đều đặt cờ và cả hai vào critical section[^ostep-flag-fail]. Đó là chính race condition của mục 1, chuyển từ biến `counter` sang biến `flag`: "kiểm tra rồi đặt" cũng là hai bước. Lỗi thứ hai là hiệu năng: thread chờ quay liên tục (spin-waiting)[^ostep-spinwait].

<details><summary>Đi sâu: tắt interrupt</summary>

Giải pháp cũ cho máy một CPU: tắt interrupt trước critical section rồi bật lại, nên đoạn code bên trong không thể bị ngắt và chạy "như thể atomic"[^ostep-intr-works]. Ưu điểm chính theo OSTEP là đơn giản[^ostep-intr-simple]. Nhược điểm theo OSTEP:

- Cho phép chương trình bất kỳ thực hiện thao tác đặc quyền (tắt/bật interrupt), nghĩa là phải tin chương trình đó; một chương trình có thể độc chiếm CPU, hoặc lặp vô hạn và hệ điều hành không bao giờ giành lại quyền điều khiển[^ostep-intr-trust].
- Không chạy trên đa xử lý: các thread trên CPU khác vẫn vào được critical section[^ostep-intr-mp].
- Tắt interrupt quá lâu có thể làm mất interrupt, ví dụ không biết đĩa đã đọc xong[^ostep-intr-lost].

</details>

**Quay lại Q2:** lock tốt phải đúng, công bằng và nhanh; cờ thường và tắt interrupt đều thiếu một thao tác atomic thật.

## 3. Các lệnh atomic phần cứng và lock xây trên chúng (Q3)

> **Trả lời ngắn.** Phần cứng cung cấp các lệnh đọc-sửa-ghi atomic: test-and-set[^ostep-tas-code], compare-and-swap[^ostep-cas], load-linked/store-conditional[^ostep-llsc] và fetch-and-add[^ostep-faa]. Spin lock xây trên test-and-set hoặc CAS[^ostep-spin]; ticket lock dùng fetch-and-add để mọi thread đều tiến triển[^ostep-ticket-progress].

**Giải thích.**

Hình dung (nhận xét của người soạn): lệnh atomic giống một cú "bốc số và đóng dấu" làm trong một động tác — không ai chen vào giữa lúc bốc và lúc đóng.

**Test-and-set (atomic exchange).**

Test-and-set là hỗ trợ phần cứng đơn giản nhất để hiểu (theo sách)[^ostep-tas]. Theo mã trong sách, nó lấy giá trị cũ tại một địa chỉ, ghi giá trị mới vào đó và trả giá trị cũ về — cả chuỗi này thực hiện atomic[^ostep-tas-code]. Tên khác nhau theo kiến trúc: trên x86 nó là phiên bản có khóa của `xchg`[^ostep-tas-names]; *Rust Atomics and Locks* lưu ý `xchg` có tiền tố `lock` ngầm định, và nó tương ứng với phép `swap` atomic[^atomics-xchg]. (Nhận xét của người soạn: nếu giá trị cũ trả về là "đã khóa" thì thread biết lock đang bị giữ; nếu là "rảnh" thì đồng thời nó đã tự đặt thành "khóa", tức đã giành được lock.)

Nhờ chỉ một thao tác atomic, chỉ một thread giành được lock; loại lock cứ quay vòng đến khi lock rảnh gọi là **spin lock**[^ostep-spin]. Trong Rust, sách *Rust Atomics and Locks* viết đúng như vậy với `swap`[^atomics-spin-code], như đoạn code sau:

```rust
// Khái niệm: test-and-set bằng AtomicBool::swap (rút từ chương 4 của sách)
while self.locked.swap(true, Acquire) { std::hint::spin_loop(); }   // lock
self.locked.store(false, Release);                                  // unlock
```

Cặp `Acquire`/`Release` bảo đảm mỗi `unlock()` thiết lập quan hệ happens-before với các `lock()` sau nó; thiếu nó thì có data race và là UB[^atomics-spin-hb][^atomics-spin-race]. Memory ordering là đề tài của [chương 06](06-memory-model.md).

**Fetch-and-add và ticket lock.**

Fetch-and-add tăng một giá trị atomic và trả về giá trị cũ[^ostep-faa]. Spin lock đơn giản không đảm bảo fairness: thread đang quay có thể quay mãi dưới tranh chấp[^ostep-spin-fair]. **Ticket lock** dùng fetch-and-add để khắc phục: thay vì một giá trị, nó dùng cặp biến `ticket` và `turn`[^ostep-ticket-vars]. Khi muốn lock, thread `fetch_add` trên `ticket` lấy "số thứ tự" `myturn`, rồi quay đến khi `turn == myturn`; unlock chỉ việc tăng `turn` để thread kế tiếp được vào[^ostep-ticket-algo]. Hệ quả: lock đảm bảo mọi thread đều tiến triển[^ostep-ticket-progress] — giống phát số ở quầy giao dịch.

<details><summary>Đi sâu: compare-and-swap và load-linked/store-conditional</summary>

**Compare-and-swap.**

Compare-and-swap (x86 gọi là compare-and-exchange): kiểm tra giá trị tại địa chỉ có bằng `expected` không; nếu có thì ghi giá trị mới[^ostep-cas]. Sách nói CAS là lệnh mạnh hơn test-and-set[^ostep-cas-stronger], nhưng nếu chỉ dùng nó để xây spin lock đơn giản thì hành vi y hệt[^ostep-cas-same]. Trong Rust, `compare_exchange` trả `Result` mang giá trị cũ, và có biến thể `compare_exchange_weak` có thể thất bại giả (trả `Err` dù giá trị khớp) nhưng trên một số nền tảng chạy hiệu quả hơn[^atomics-weak]. Trên x86-64 hai biến thể biên dịch ra cùng một `lock cmpxchg`[^atomics-weak-x86] — khớp với bảng ở mục 1. Mẫu "đọc, tính giá trị mới, CAS, thất bại thì thử lại" là nền của lock-free ([chương 07](07-lock-free.md)).

**Load-linked / store-conditional.**

Một số kiến trúc cung cấp cặp lệnh thay cho CAS: *load-linked* đọc như load thường; *store-conditional* chỉ thành công (và cập nhật giá trị) nếu không có store nào khác vào địa chỉ đó kể từ lần load-linked[^ostep-llsc]. OSTEP nêu MIPS làm ví dụ và nói Alpha, PowerPC, ARM có lệnh tương tự[^ostep-llsc-arch]. Lock xây trên cặp này: quay đến khi cờ bằng 0, rồi thử `store-conditional` đặt thành 1; nếu thread khác chen vào giữa thì store-conditional thất bại và thử lại từ đầu[^ostep-llsc-lock].

Trên ARM64 (phiên bản ARMv8 đầu tiên) cặp lệnh này là `ldxr` / `stxr`[^atomics-ldxr]. Mỗi core chỉ theo dõi được một địa chỉ, và `store-conditional` được phép thất bại giả, nên phần cứng có thể theo dõi thô mà vẫn đúng, đổi lại thêm vài vòng lặp[^atomics-llsc-weak] — Rust Atomics and Locks nói đây là lý do `compare_exchange_weak` tồn tại[^atomics-weak-reason].

</details>

**Quay lại Q3:** lock xây từ lệnh atomic phần cứng — TAS/CAS cho spin lock, fetch-and-add cho ticket lock công bằng.


## 4. Quay, nhường CPU, hay ngủ (Q4)

> **Trả lời ngắn.** Quay (spin) phí CPU, nhất là khi thread giữ lock bị preempt[^ostep-spin-preempt]. Yield nhường CPU nhưng tốn context switch và không chống đói[^ostep-yield-99][^ostep-yield-starve]. Ngủ trong hàng đợi (park/unpark) kiểm soát thread nào nhận lock tiếp theo[^ostep-queue-why]; two-phase lock quay một lúc rồi mới ngủ[^ostep-twophase].

**Giải thích.**

Hình dung (nhận xét của người soạn): chờ một phòng họp — đứng ngay cửa nhìn chằm chằm (quay), ghé qua hỏi liên tục (yield), hay ngồi ghế chờ và nhờ lễ tân gọi khi phòng trống (ngủ).

**Quay phí CPU.**

Spin-waiting phí thời gian chờ thread khác nhả lock[^ostep-spinwait]. Tệ nhất khi thread đang giữ lock bị preempt giữa critical section: scheduler có thể chạy lần lượt mọi thread còn lại, mỗi thread quay hết cả một time slice[^ostep-spin-preempt]; với N thread tranh lock là N − 1 time slice bị phí[^ostep-spin-nminus1]. Trên một CPU, spin lock còn đòi scheduler preemptive mới hoạt động[^ostep-uni-preempt].

**Yield.**

Cách đầu tiên: khi sắp quay, hãy nhường CPU cho thread khác[^ostep-yield]. `yield` là system call chuyển thread gọi từ trạng thái running sang ready[^ostep-yield-state]. Với hai thread trên một CPU cách này chạy tốt; nhưng với 100 thread, nếu thread giữ lock bị preempt thì 99 thread còn lại mỗi thread gọi `lock()`, thấy lock bị giữ và yield[^ostep-yield-99] — tốt hơn quay, nhưng chi phí context switch đáng kể. Và yield không giải quyết đói: một thread có thể kẹt trong vòng yield vô tận[^ostep-yield-starve].

**Ngủ trong hàng đợi (park/unpark).**

OSTEP chỉ ra gốc vấn đề: các cách trước để quá nhiều thứ cho may rủi, nên cần chủ động kiểm soát thread nào nhận lock tiếp theo[^ostep-queue-why]. Sách dùng hai lời gọi của Solaris: `park()` đưa thread gọi vào trạng thái ngủ và `unpark(threadID)` đánh thức một thread cụ thể[^ostep-park]. Lock gồm cờ `flag`, một `guard` (spin lock ngắn bảo vệ `flag` và hàng đợi) và hàng đợi các thread chờ. Khi unlock mà hàng đợi không rỗng, lock được **chuyển thẳng** cho thread kế tiếp mà không đặt `flag` về 0[^ostep-handoff]; spin ở `guard` chỉ kéo dài vài lệnh trong code `lock`/`unlock`, không phải suốt critical section của người dùng[^ostep-guard-short].

**Two-phase lock.** Spin hữu ích khi lock sắp được nhả; ngủ hữu ích khi phải chờ lâu. **Two-phase lock** kết hợp: pha một quay một lúc, hy vọng giành được lock; không được thì sang pha hai là ngủ[^ostep-twophase]. OSTEP nhận xét lock Linux ở dạng futex là một dạng two-phase nhưng chỉ quay một lần; có thể tổng quát hóa thành quay trong vòng lặp[^ostep-twophase-linux]. Mục 6: `std::sync::Mutex` có cấu trúc tương tự — quay có giới hạn rồi `futex_wait` (suy luận của người soạn).

<details><summary>Đi sâu: wakeup/waiting race</summary>

**Wakeup/waiting race.** Nếu thread bị ngắt ngay trước `park()`, và thread giữ lock nhả lock rồi `unpark` trong khoảng đó, thì `park()` sau đó có thể ngủ mãi — vấn đề này OSTEP gọi là "wakeup/waiting race"[^ostep-park-race]. Solaris thêm `setpark()`: nếu `unpark` đến trước khi `park` được gọi thật, thì `park` trả về ngay thay vì ngủ[^ostep-setpark]. Cách khác là đưa `guard` vào kernel để nhả lock và đưa thread vào hàng đợi một cách atomic[^ostep-guard-kernel]. Futex (mục 5) đi theo hướng so sánh giá trị trong kernel.

</details>

**Quay lại Q4:** quay phí CPU, yield tốn context switch, ngủ trong hàng đợi chống đói nhưng có wakeup/waiting race; two-phase lock lấy phần tốt của hai bên.

## 5. Futex: fast path ở user space, slow path ở kernel (Q5)

> **Trả lời ngắn.** Futex là một số nguyên trong shared memory: khi không tranh chấp, thread đổi trạng thái bằng lệnh atomic và kernel không biết gì[^futex-uncontended]. `FUTEX_WAIT` so sánh giá trị rồi ngủ một cách atomic, trả `EAGAIN` nếu giá trị đã đổi nên không mất wake-up[^futex-wait-atomic][^futex-eagain]; `FUTEX_WAKE` đánh thức tối đa `val` thread[^futex-wake].

**Giải thích.**

Hình dung (nhận xét của người soạn): tấm biển "đang dùng" trên cửa phòng — chỉ khi có người đứng chờ mới phải gọi bảo vệ (kernel).


Spin mãi thì phí CPU, còn ngủ/đánh thức bằng syscall mỗi lần lock thì chậm. futex(7) mô tả futex là khối xây rất cơ bản, phù hợp để xây mutex, condition variable, read-write lock, barrier, semaphore ở tầng cao hơn[^futex7-build]. Hầu hết lập trình viên không gọi futex trực tiếp mà dùng thư viện hệ thống xây trên nó[^futex7-lib]; glibc không cung cấp wrapper cho `futex()` nên muốn gọi phải dùng `syscall(2)`[^futex-nowrapper]. Mara Bos tóm phía Linux: các primitive đồng bộ của pthread đều được cài bằng syscall futex[^atomics-futex-linux]; OSTEP cũng nói Linux cung cấp futex, tương tự giao diện của Solaris nhưng thêm nhiều chức năng trong kernel[^ostep-futex-linux].

**Cơ chế.**

`futex(2)` là phương tiện chờ đến khi một điều kiện đúng, thường dùng làm khối chặn cho đồng bộ shared-memory; phần lớn thao tác đồng bộ diễn ra ở user space và chỉ gọi syscall khi cần chặn hoặc đánh thức[^futex-userspace]. futex(7) nhấn mạnh: trường hợp không tranh chấp chạy hoàn toàn ở user space, kernel chỉ phân xử khi có tranh chấp[^futex7-nonc].

- **Fast path**: trạng thái lock là một số nguyên ("futex word") trong shared memory. Khi không tranh chấp, thread đổi trạng thái chỉ bằng lệnh atomic như compare-and-exchange; kernel không biết gì về lock[^futex-uncontended].
- **`FUTEX_WAIT`** kiểm tra giá trị tại địa chỉ futex còn bằng giá trị mong đợi `val` không, và nếu còn thì ngủ chờ `FUTEX_WAKE`. Việc đọc giá trị, so sánh và bắt đầu ngủ được thực hiện atomic[^futex-wait-atomic]. Nếu giá trị khác thì thất bại ngay với `EAGAIN`; phép so sánh này để tránh mất wake-up — nếu thread khác đổi giá trị rồi đã gọi wake trước khi ta kịp ngủ, ta nhìn thấy giá trị mới và không ngủ[^futex-eagain]. Man page gọi đây là phép so sánh-rồi-chặn atomic (atomic compare-and-block)[^futex-cab]. Đây chính là lời giải cho wakeup/waiting race ở mục 4, và OSTEP mô tả `futex_wait(address, expected)` đúng như vậy[^ostep-futex-wait].
- **`FUTEX_WAKE`** đánh thức tối đa `val` thread đang chờ trên futex word đó[^futex-wake].

<details><summary>Đi sâu: mutex tự viết trong Rust Atomics and Locks</summary>

Với mutex tự viết ở chương 9, Mara Bos nói: ở trường hợp không tranh chấp, cả `wait()` lẫn `wake_one()` đều bị bỏ qua hoàn toàn[^atomics-fastpath].

</details>

**Quay lại Q5:** futex để fast path hoàn toàn ở user space và chỉ vào kernel khi phải chờ, còn việc so sánh-rồi-ngủ atomic loại bỏ wakeup/waiting race.

## 6. `std::sync::Mutex` trên Linux (Rust 1.95.0) (Q6)

> **Trả lời ngắn.** Trên Linux (source Rust 1.95.0), `Mutex` là một futex word có ba trạng thái 0/1/2[^std-futex-states]. Lock không tranh chấp là một `compare_exchange` không syscall[^std-futex-fast]; nếu bị chiếm thì spin tối đa 100 vòng[^std-futex-spin100] rồi `futex_wait`[^std-futex-wait]; unlock chỉ gọi wake khi trạng thái cũ là `CONTENDED`[^std-futex-unlock].

**Giải thích.**

Hình dung (nhận xét của người soạn): thử mở cửa; không được thì đứng đợi một lúc, rồi ngồi ghế chờ; người ra khỏi phòng chỉ gọi người ngồi ghế chờ nếu thật sự có người.


Trên Linux, `std::sync::Mutex` dùng hiện thực `library/std/src/sys/sync/mutex/futex.rs`. Ba trạng thái được định nghĩa ngay đầu file: `UNLOCKED = 0`, `LOCKED = 1` (khóa, không ai chờ) và `CONTENDED = 2` (khóa, có thread khác đang chờ)[^std-futex-states]. (Đây là chi tiết cài đặt trong source, không phải bảo đảm của docs — có thể đổi giữa các phiên bản; trên macOS thư mục `sys/sync/mutex/` có cài đặt khác, người soạn không đọc nên không khẳng định.)

```mermaid
stateDiagram-v2
  [*] --> UNLOCKED_0
  UNLOCKED_0 --> LOCKED_1: lock(): compare_exchange(0,1, Acquire) thành công — fast path, không syscall
  LOCKED_1 --> UNLOCKED_0: unlock(): swap(0, Release) trả về 1 — không syscall
  LOCKED_1 --> CONTENDED_2: lock_contended(): spin xong (hết bộ đếm) mà vẫn LOCKED, swap(2, Acquire) trả về 1
  UNLOCKED_0 --> LOCKED_1: lock_contended(): spin thấy UNLOCKED, compare_exchange(0,1, Acquire)
  UNLOCKED_0 --> CONTENDED_2: lock_contended(): swap(2, Acquire) trả về 0 — giành được lock nhưng để trạng thái 2
  CONTENDED_2 --> CONTENDED_2: futex_wait(2) rồi thức dậy, spin lại
  CONTENDED_2 --> UNLOCKED_0: unlock(): swap(0, Release) trả về 2 — futex_wake 1 thread
```

- **Fast path**: `lock()` thử `compare_exchange(UNLOCKED, LOCKED, Acquire, Relaxed)`; chỉ khi lỗi mới gọi `lock_contended()`[^std-futex-fast].
- **Pha spin** (cấu trúc giống two-phase lock): `lock_contended` mở đầu bằng một comment nói nên spin trước để tăng tốc nếu lock được nhả nhanh[^std-futex-spin], và trong vòng spin chỉ dùng `load` (không `swap`/`compare_exchange`) để thân thiện với cache[^std-futex-spin-load]. Bộ đếm spin khởi đầu ở 100, mỗi vòng gọi `spin_loop()` rồi giảm 1[^std-futex-spin100]. Vòng spin dừng sớm khi trạng thái không còn là `LOCKED` (tức đã `UNLOCKED` hoặc đã `CONTENDED`)[^std-futex-spin-stop]. Nếu spin trả về `UNLOCKED`, thread thử `compare_exchange(UNLOCKED, LOCKED)` — giành được thì trả về mà không đánh dấu `CONTENDED`[^std-futex-spin-cas].
- **Pha ngủ**: nếu vẫn bị chiếm thì đặt trạng thái `CONTENDED` (và nếu `swap` thấy giá trị cũ là `UNLOCKED` thì đã giành được lock)[^std-futex-contended-swap], rồi gọi `futex_wait(&self.futex, CONTENDED, None)` — chờ cho futex đổi khỏi `CONTENDED`[^std-futex-wait].
- **Unlock**: `swap(UNLOCKED, Release)`; chỉ khi giá trị cũ là `CONTENDED` mới gọi wake[^std-futex-unlock]. Comment trong source giải thích chỉ đánh thức một thread vì thread đó khi lock lại sẽ đặt `CONTENDED`, đảm bảo các thread còn chờ cuối cùng cũng được đánh thức[^std-futex-wake-one].

<details><summary>Đi sâu: lớp syscall, happens-before, fairness và quan sát bằng strace</summary>

- **Lớp syscall**: trên Linux, `futex_wait` gọi `SYS_futex` với `FUTEX_WAIT_BITSET | FUTEX_PRIVATE_FLAG`[^std-sys-futex]; hàm trả về ngay nếu futex không giữ giá trị mong đợi[^std-sys-futex-ret].
- **Happens-before**: `lock` dùng `Acquire`, `unlock` dùng `Release` trên cùng futex word (suy luận của người soạn từ source). *Rust Atomics and Locks* ch.3 nêu cho mutex nói chung rằng có happens-before giữa unlock và lock kế tiếp[^atomics-mutex-ex] (chi tiết ở [chương 06, mục 4 và 7](06-memory-model.md)); ch.9 nói với mutex tự viết cùng thiết kế rằng nó được thiết lập khi `swap` acquire thấy giá trị do `swap` release lưu[^atomics-mutex-hb]. Người review không thấy docs `std::sync::Mutex` 1.95.0 nói điều này riêng cho `Mutex` của `std`.
- **Fairness**: người review không thấy docs `std::sync::Mutex` 1.95.0 hứa thứ tự cấp lock, nên đừng giả định FIFO. Suy luận của người soạn từ source: `unlock` đặt `UNLOCKED` *rồi mới* đánh thức một thread[^std-futex-unlock], nên thread đang chạy có thể `compare_exchange` thắng trước thread vừa được đánh thức. Ticket lock thì cấp theo số thứ tự (mục 3).


**Quan sát bằng strace** (chạy thật trong container Linux aarch64; người review chạy lại, cùng kết quả): thread chính giữ `Mutex` 200 ms trong khi thread con gọi `lock()`. Thread con không giành được lock trong 100 vòng spin nên ngủ bằng `futex(addr, FUTEX_WAIT_BITSET_PRIVATE, 2, NULL, …)` — đối số `2` chính là `CONTENDED`. Khi thread chính nhả lock, nó gọi `futex(addr, FUTEX_WAKE_PRIVATE, 1)` và nhận về `1` (đã đánh thức một thread). 1000 lần lock/unlock không tranh chấp trước đó không sinh lệnh futex nào, khớp với fast path.

</details>

**Quay lại Q6:** `Mutex` trên Linux là state machine futex 0/1/2: fast path không syscall, spin tối đa 100 vòng, rồi ngủ bằng `futex_wait`.


## 7. Đo thử: các lock trên cùng một bộ đếm (Q7)

> **Trả lời ngắn.** Trong thí nghiệm nhỏ này (4 thread, mỗi thread tăng 1.000.000 lần, một máy, **quan sát**), `load` rồi `store` atomic mất cập nhật, còn `fetch_add`, `Mutex`, spin lock và ticket lock đều ra đúng 4.000.000; thời gian tăng dần theo thứ tự đó.

**Giải thích.**

Hình dung (nhận xét của người soạn): cách nhanh nhất để đếm là cách sai — bỏ khóa thì nhanh nhưng mất số.

Chương trình lab (rustc 1.95.0 `-O`, container Linux aarch64, 4 vCPU): 4 thread, mỗi thread tăng một bộ đếm 1.000.000 lần, kỳ vọng 4.000.000. Hai lần chạy:

| Cách tăng | Kết quả cuối (lần 1 / lần 2) | Thời gian (lần 1 / lần 2) |
|---|---|---|
| `load` rồi `store` atomic (racy) | 1.670.161 / 1.030.726 | 0,003 s / 0,002 s |
| `fetch_add` | 4.000.000 / 4.000.000 | 0,026 s / 0,025 s |
| `std::sync::Mutex` | 4.000.000 / 4.000.000 | 0,050 s / 0,046 s |
| spin lock (`swap`) | 4.000.000 / 4.000.000 | 0,143 s / 0,132 s |
| ticket lock | 4.000.000 / 4.000.000 | 0,238 s / 0,258 s |

<details><summary>Đi sâu: đọc kết quả và nguyên nhân (suy luận)</summary>

Đây là **một thí nghiệm nhỏ trên một máy**, không phải benchmark. Điều chắc chắn là cột kết quả: `racy` mất khoảng 58–74% số lần tăng và con số khác nhau giữa hai lần chạy — đúng nghĩa "indeterminate"[^ostep-indeterminate]. Cột thời gian chỉ cho thấy thứ tự `fetch_add` < `Mutex` < spin < ticket (người review chạy lại 3 lần, thứ tự giữ nguyên). Nguyên nhân là **suy luận của người soạn, chưa đo riêng**: thread giữ lock bị preempt làm các thread quay đốt CPU (mục 4), và/hoặc cache line bị chuyển qua lại giữa các core.

</details>

**Quay lại Q7:** chỉ cột kết quả là chắc chắn (racy mất cập nhật); thứ tự thời gian chỉ là quan sát một máy.


## 8. Cấu trúc dữ liệu có lock (Q8)

> **Trả lời ngắn.** Một lock to bọc cả cấu trúc thì đúng và đơn giản; nếu cấu trúc không quá chậm thì thế là đủ[^ostep-biglock][^ostep-biglock-done]. Nhiều lock nhỏ (fine-grained) cho nhiều thread cùng vào hơn[^ostep-fine] nhưng không chắc nhanh hơn: hand-over-hand thường chậm hơn một lock[^ostep-hoh-slow]. Reader-writer lock cho nhiều reader chạy đồng thời khi không có writer[^ostep-rw-idea].

**Giải thích.**

Hình dung (nhận xét của người soạn): một cổng soát vé cho cả sân vận động thì đơn giản nhưng nghẽn; nhiều cổng thì nhanh hơn nhưng phải lo người đi nhầm cổng.

**Một lock to: đúng, đơn giản, có thể không scale.**

Thêm lock vào một cấu trúc dữ liệu để thread dùng được gọi là làm nó *thread safe*[^ostep-threadsafe]. Cách đơn giản nhất: một lock duy nhất, lấy khi gọi hàm thao tác cấu trúc và nhả khi trả về[^ostep-biglock]. Cách này đúng, và sách nhấn mạnh: nếu cấu trúc không quá chậm thì bạn đã xong, không cần làm gì cầu kỳ[^ostep-biglock-done]. POSIX cho phép nhiều lock cho nhiều dữ liệu: một lock lớn cho mọi critical section là *coarse-grained*[^ostep-coarse]; dùng lock khác nhau cho dữ liệu khác nhau cho phép nhiều thread cùng ở trong code có lock hơn (*fine-grained*)[^ostep-fine].

**Bộ đếm: lock đúng nhưng không scale.**

OSTEP đo bộ đếm có lock: một thread làm 1 triệu lần cập nhật trong khoảng 0,03 giây, còn hai thread cùng cập nhật 1 triệu lần mỗi thread thì chậm hẳn đi (hơn 5 giây trên máy của họ)[^ostep-counter-slow]. Con số là của máy trong sách nên chỉ có hình dạng là đáng tin: nhiều core tranh một lock thì chậm hơn một core. Cách khắc phục là **approximate counter**: mỗi CPU một bộ đếm cục bộ (có lock riêng) cộng một bộ đếm toàn cục[^ostep-approx]. Cập nhật thường chỉ động tới bộ đếm cục bộ; khi nó đạt ngưỡng S thì được chuyển sang bộ đếm toàn cục[^ostep-approx-flush]. S nhỏ thì chính xác nhưng chậm; S lớn thì nhanh nhưng giá trị toàn cục tụt hậu[^ostep-approx-tradeoff]. (Nhận xét của người soạn: cùng ý tưởng với metrics counter shard theo core/thread rồi cộng khi scrape.)

<details><summary>Đi sâu: danh sách liên kết, hand-over-hand locking và reader-writer lock</summary>

**Danh sách liên kết: lock chỉ bao quanh critical section thật.**

Với linked list, OSTEP nêu hai bài học. Một là **đường lỗi**: gần 40% bug trong một nghiên cứu patch Linux kernel nằm ở các đường code hiếm khi chạy[^ostep-errorpaths] — ví dụ `malloc` thất bại khi đang giữ lock mà quên `unlock`. Hai là **thu hẹp phạm vi lock**: `lock`/`unlock` chỉ bao đúng critical section còn `malloc` làm ngoài lock, và hợp nhất các điểm thoát để ít chỗ quên `unlock`[^ostep-narrow]. (Mục 9: RAII guard của `Mutex` xử lý đúng điều này.)

**Hand-over-hand locking.**

Để tăng độ đồng thời, *hand-over-hand locking* (lock coupling) đặt một lock cho mỗi node; khi duyệt, lấy lock node kế tiếp rồi mới nhả node hiện tại[^ostep-handoverhand]. Nghe hợp lý, nhưng khó nhanh hơn một lock duy nhất vì chi phí lấy/nhả lock từng node quá lớn[^ostep-hoh-slow]. Bài học của sách: thêm lock và độ phức tạp có thể làm hỏng hơn là giúp; cách duy nhất để biết là xây cả hai và đo[^ostep-tip-concurrency].

**Reader-writer lock (mức khái niệm).**

Nhiều thao tác chỉ đọc (lookup), một số thao tác ghi (insert). Miễn là không có insert đang chạy, ta có thể cho nhiều lookup chạy đồng thời — loại lock hỗ trợ điều đó là *reader-writer lock*[^ostep-rw-idea]. Trong hiện thực mẫu của OSTEP, nhiều reader được vào cùng lúc, còn writer phải chờ đến khi mọi reader ra; reader cuối cùng mở đường cho writer[^ostep-rw-impl]. Sách chỉ ra điểm yếu của hiện thực đơn giản này là fairness: reader dễ làm đói writer[^ostep-rw-starve]. Và rw-lock nên dùng thận trọng: chúng thường thêm overhead nên có thể không nhanh hơn lock đơn giản[^ostep-rw-caution].

</details>

**Quay lại Q8:** bắt đầu bằng một lock to, chia nhỏ chỉ khi đo thấy cần; reader-writer lock cho nhiều reader nhưng có overhead và nguy cơ đói writer.

## 9. Liên hệ với Rust

- **`Mutex<T>` bọc dữ liệu.** Dữ liệu chỉ truy cập được qua RAII guard trả về từ `lock`/`try_lock`, bảo đảm dữ liệu chỉ được chạm khi mutex đang khóa[^std-mutex-guard]. `MutexGuard` cài `Deref` và `Drop`: lock tự nhả khi guard ra khỏi scope[^book-guard]. Suy luận từ hai nguồn: không thể "quên lock", cũng không thể "quên unlock trên đường lỗi" như ví dụ `malloc` ở mục 8. Dưới Linux, nó chính là state machine futex 0/1/2 ở mục 6.
- **`RwLock<T>`** cho nhiều reader miễn là không có writer đang giữ[^std-rwlock]. Docs không bảo đảm chính sách ưu tiên — nó phụ thuộc OS — và một writer đang chờ có thể chặn hoặc không chặn các lời gọi `read` mới[^std-rwlock-policy]. Docs đưa ví dụ có thể deadlock khi cùng một thread gọi `read` hai lần trong lúc có writer chờ[^std-rwlock-deadlock]. Yêu cầu kiểu: `T: Send` để chia sẻ qua thread và `T: Sync` để cho nhiều reader truy cập đồng thời[^std-rwlock-bounds]. Đây là cùng chủ đề fairness/starvation của mục 8 (nhận xét của người soạn): docs không cam kết một chính sách cố định.
- **Poisoning, `Condvar`, `Send`/`Sync`**: `Condvar` ở [chương 04](04-condition-variable-va-semaphore.md); poisoning, `Send`/`Sync` và `Mutex`/`RwLock` chi tiết ở [chương 08](08-rust-concurrency.md) (mục 6 cho poisoning).

## Tóm tắt

- `counter += 1` là chuỗi nhiều bước (load, add, store); thread khác hoặc interrupt chen vào giữa gây race condition với kết quả "indeterminate".
- Tắt interrupt không chạy trên đa xử lý và đòi quá nhiều tin cậy; cờ + load/store thường lặp lại đúng race của counter. Cần lệnh atomic phần cứng.
- Spin lock xây trên TAS/CAS; ticket lock xây trên fetch-and-add để có fairness.
- Quay phí CPU, yield tốn context switch và không chống đói, hàng đợi + park/unpark chống đói nhưng có race wakeup/waiting; two-phase lock quay một lúc rồi ngủ.
- futex = số nguyên trong user space + `FUTEX_WAIT` (so sánh rồi ngủ atomic, `EAGAIN` nếu giá trị đã đổi) + `FUTEX_WAKE`; không tranh chấp thì không có syscall.
- `std::sync::Mutex` trên Linux (1.95.0): trạng thái 0/1/2, fast path `compare_exchange`, tối đa 100 vòng spin chỉ bằng `load`, rồi `futex_wait`; unlock chỉ gọi wake khi trạng thái cũ là 2.
- Một lock to thì đúng và đơn giản; lock mịn hơn có thể nhanh hơn nhưng không chắc (hand-over-hand thường chậm hơn); đo trước khi tối ưu. Reader-writer lock cho nhiều reader nhưng có overhead và nguy cơ đói writer.

## Câu hỏi tự kiểm tra

1. Vì sao hai thread cùng `counter += 1` có thể cho kết quả nhỏ hơn kỳ vọng? Vẽ interleaving với counter bắt đầu 50.
<details><summary>Đáp án</summary>

Thread 1 load 50 vào register, bị ngắt; Thread 2 load 50, store 51; Thread 1 resume và store 51 từ register cũ — hai lần tăng chỉ ra 51[^ostep-ctr-seq][^ostep-lost-update].

</details>

2. Ba tiêu chí đánh giá lock là gì?
<details><summary>Đáp án</summary>

Mutual exclusion, fairness (chống đói), performance (chi phí thời gian)[^ostep-lock-criteria][^ostep-lock-fairness][^ostep-lock-perf].

</details>

3. Vì sao lock chỉ dùng một biến cờ và load/store thường thất bại?
<details><summary>Đáp án</summary>

"Kiểm tra cờ" và "đặt cờ" là hai bước; một interrupt chen vào giữa khiến cả hai thread đều đặt cờ và vào critical section[^ostep-flag-fail].

</details>

4. Test-and-set và compare-and-swap làm gì? Chúng khác `load` rồi `store` ở đâu?
<details><summary>Đáp án</summary>

TAS atomically trả giá trị cũ và ghi giá trị mới[^ostep-tas-code]; CAS ghi giá trị mới chỉ khi giá trị hiện tại bằng `expected`[^ostep-cas]. Cả đọc và ghi diễn ra như một thao tác atomic, không có khoảng hở.

</details>

5. "Wakeup/waiting race" là gì và futex ngăn nó thế nào?
<details><summary>Đáp án</summary>

Thread bị ngắt giữa lúc quyết định ngủ và lúc ngủ thật; thread khác nhả lock và đánh thức trước, nên thread kia ngủ mãi[^ostep-park-race]. `FUTEX_WAIT` so sánh giá trị rồi ngủ atomic, thất bại với `EAGAIN` nếu giá trị đã đổi[^futex-eagain][^futex-cab].

</details>

6. Mô tả ba trạng thái của `std::sync::Mutex` trên Linux và khi nào có syscall.
<details><summary>Đáp án</summary>

UNLOCKED=0, LOCKED=1, CONTENDED=2[^std-futex-states]. Lock/unlock không tranh chấp không có syscall (fast path `compare_exchange`, unlock `swap`)[^std-futex-fast][^std-futex-unlock]; syscall `futex_wait` khi spin (tối đa 100 vòng) kết thúc mà lock vẫn chưa được nhả[^std-futex-spin100][^std-futex-wait], `futex_wake` khi unlock thấy trạng thái cũ CONTENDED[^std-futex-unlock].

</details>

7. Khi nào nên chọn một lock to, khi nào chia nhỏ? Cho ví dụ cho thấy chia nhỏ không luôn nhanh hơn.
<details><summary>Đáp án</summary>

Bắt đầu bằng một lock to nếu đủ nhanh[^ostep-biglock-done]; chỉ chia nhỏ khi đo thấy cần. Hand-over-hand locking cho linked list khó nhanh hơn một lock vì chi phí lấy/nhả lock từng node[^ostep-hoh-slow]; bộ đếm có lock thì lại scale kém và approximate counter giải quyết[^ostep-counter-slow][^ostep-approx].

</details>

8. Vì sao vòng spin của `std::sync::Mutex` chỉ dùng `load`?
<details><summary>Đáp án</summary>

Source comment nói đây là để thân thiện với cache[^std-futex-spin-load]; cơ chế (ghi cần quyền sở hữu cache line, load thì không) là suy luận của người soạn.

</details>

## Đọc thêm

- OSTEP ch.28 *Locks* — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — nguồn chính của chương này; đọc kỹ mã Figure 28.9–28.10 (hàng đợi và futex lock của glibc).
- futex(2) — <https://manpages.debian.org/bookworm/manpages-dev/futex.2.en.html> — các thao tác `FUTEX_WAIT`, `FUTEX_WAKE` và mã ví dụ.
- *Rust Atomics and Locks*, ch.4 và ch.9 — <https://marabos.nl/atomics/building-spinlock.html> và <https://marabos.nl/atomics/building-locks.html> — tự xây spin lock và mutex trên futex.
- Tiếp theo: [Chương 04 — Condition variable và semaphore](04-condition-variable-va-semaphore.md); nền memory model ở [Chương 06](06-memory-model.md).

[^ostep-ctr-seq]: OSTEP, ch.26 Concurrency: An Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-intro.pdf> — "In this case, we wish to simply add a number (1) to counter. Thus, the code sequence for doing so might look something like this (in x86); mov 0x8049a1c, %eax add $0x1, %eax mov %eax, 0x8049a1c"
[^ostep-lost-update]: OSTEP, ch.26 Concurrency: An Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-intro.pdf> — "the code to increment counter has been run twice, but counter, which started at 50, is now only equal to 51."
[^ostep-race]: OSTEP, ch.26 Concurrency: An Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-intro.pdf> — "What we have demonstrated here is called a race condition (or, more specifically, a data race): the results depend on the timing of the code's execution."
[^ostep-indeterminate]: OSTEP, ch.26 Concurrency: An Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-intro.pdf> — "we call this result indeterminate, where it is not known what the output will be and it is indeed likely to be different across runs."
[^ostep-cs]: OSTEP, ch.26 Concurrency: An Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-intro.pdf> — "A critical section is a piece of code that accesses a shared variable (or more generally, a shared resource) and must not be concurrently executed by more than one thread."
[^ostep-mutex]: OSTEP, ch.26 Concurrency: An Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-intro.pdf> — "This property guarantees that if one thread is executing within the critical section, the others will be prevented from doing so."
[^ostep-atomic-def]: OSTEP, ch.26 Concurrency: An Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-intro.pdf> — "Atomically, in this context, means “as a unit”, which sometimes we take as “all or none.”"
[^ostep-sync-prims]: OSTEP, ch.26 Concurrency: An Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-intro.pdf> — "what we will instead do is ask the hardware for a few useful instructions upon which we can build a general set of what we call synchronization primitives."
[^atomics-x86-notatomic]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "While not directly obvious from the assembly itself, the x86-64 version is not atomic."
[^atomics-lockprefix]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "To support multi-core systems, Intel introduced an instruction prefix called lock. It is used as a modifier to instructions like add to make their operation atomic."
[^atomics-ldadd]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "the new ldadd (load and add) instruction is equivalent to an atomic fetch_add operation, without the need for an LL/SC loop."
[^std-datarace]: Rust std 1.95.0, module `std::sync::atomic` — <https://doc.rust-lang.org/1.95.0/std/sync/atomic/index.html> — "The most important aspect of this model is that data races are undefined behavior. A data race is defined as conflicting non-synchronized accesses where at least one of the accesses is non-atomic."
[^nomicon-race-def]: The Rustonomicon, Data Races and Race Conditions — <https://doc.rust-lang.org/1.95.0/nomicon/races.html> — "Safe Rust guarantees an absence of data races, which are defined as: two or more threads concurrently accessing a location of memory one or more of them is a write one or more of them is unsynchronized"
[^nomicon-race-cond]: The Rustonomicon, Data Races and Race Conditions — <https://doc.rust-lang.org/1.95.0/nomicon/races.html> — "However Rust does not prevent general race conditions."
[^atomics-fetch]: Rust Atomics and Locks, ch.2 Atomics — <https://marabos.nl/atomics/atomics.html> — "also load (fetch) the original value, as a single atomic operation. The most commonly used ones are fetch_add and fetch_sub"
[^ostep-lock-states]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "It is either available (or unlocked or free) and thus no thread holds the lock, or acquired (or locked or held), and thus exactly one thread holds the lock and presumably is in a critical section."
[^ostep-lock-block]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "it will not return while the lock is held by another thread; in this way, other threads are prevented from entering the critical section while the first thread that holds the lock is in there."
[^ostep-lock-criteria]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "The first is whether the lock does its basic task, which is to provide mutual exclusion."
[^ostep-lock-fairness]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "The second is fairness. Does each thread contending for the lock get a fair shot at acquiring it once it is free?"
[^ostep-lock-perf]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "The final criterion is performance, specifically the time overheads added by using the lock."
[^ostep-lock-perf-cases]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "Another is the case where multiple threads are contending for the lock on a single CPU; in this case, are there performance concerns?"
[^ostep-intr-works]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "we ensure that the code inside the critical section will not be interrupted, and thus will execute as if it were atomic."
[^ostep-intr-trust]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "an errant or malicious program could call lock() and go into an endless loop. In this latter case, the OS never regains control of the system, and there is only one recourse: restart the system."
[^ostep-intr-mp]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "Second, the approach does not work on multiprocessors."
[^ostep-intr-lost]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "turning off interrupts for extended periods of time can lead to interrupts becoming lost, which can lead to serious systems problems."
[^ostep-flag-fail]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "and both threads are thus able to enter the critical section."
[^ostep-spinwait]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "Spin-waiting wastes time waiting for another thread to release a lock."
[^ostep-tas]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "The simplest bit of hardware support to understand is known as a test-and-set"
[^ostep-tas-code]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "int old = *old_ptr; // fetch old value at old_ptr"
[^ostep-tas-names]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "on x86 it is the locked version of the atomic exchange (xchg)."
[^atomics-xchg]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "has an implicit lock prefix: it behaves like lock xchg regardless of the lock prefix."
[^ostep-spin]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "a single atomic operation, we ensure that only one thread acquires the lock. And that's how to build a working mutual exclusion primitive! You may also now understand why this type of lock is usually referred to as a spin lock."
[^atomics-spin-code]: Rust Atomics and Locks, ch.4 Building Our Own Spin Lock — <https://marabos.nl/atomics/building-spinlock.html> — "while self.locked.swap(true, Acquire) { std::hint::spin_loop(); }"
[^atomics-spin-hb]: Rust Atomics and Locks, ch.4 Building Our Own Spin Lock — <https://marabos.nl/atomics/building-spinlock.html> — "We use acquire and release memory ordering to make sure that every unlock() call establishes a happens-before relationship with the lock() calls that follow."
[^atomics-spin-race]: Rust Atomics and Locks, ch.4 Building Our Own Spin Lock — <https://marabos.nl/atomics/building-spinlock.html> — "A happens-before relationship between unlock and lock operations is necessary to prevent a data race, which would result in undefined behavior."
[^ostep-cas]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "The basic idea is for compare-and-swap to test whether the value at the address specified by ptr is equal to expected; if so, update the memory location pointed to by ptr with the new value."
[^ostep-cas-stronger]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "compare-and-swap is a more powerful instruction than test-and-set."
[^ostep-cas-same]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "its behavior is identical to the spin lock we analyzed above."
[^atomics-weak]: Rust Atomics and Locks, ch.2 Atomics — <https://marabos.nl/atomics/atomics.html> — "The difference is that the weak version may still sometimes leave the value untouched and return an Err, even though the atomic value matched the expected value."
[^atomics-weak-x86]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "On x86-64, there is no difference between compare_exchange and compare_exchange_weak. Both compile down to a lock cmpxchg instruction."
[^ostep-llsc]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "(and updates the value stored at the address just load-linked from) if no intervening store to the address has taken place."
[^ostep-llsc-arch]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "Alpha, PowerPC, and ARM provide similar instructions"
[^ostep-llsc-lock]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "and thus have to try to acquire the lock again."
[^atomics-ldxr]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "ARM64’s load-linked and store-conditional instructions are called ldxr (load exclusive register) and stxr (store exclusive register)."
[^atomics-llsc-weak]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "(1) only one memory address (per core) can be tracked at a time, and (2) the store-conditional is allowed to have false negatives, meaning that it may fail to store even though nothing has changed that particular piece of memory."
[^ostep-faa]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "which atomically increments a value while returning the old value at a particular address."
[^ostep-spin-fair]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "spin locks don’t provide any fairness guarantees. Indeed, a thread spinning may spin forever, under contention."
[^ostep-ticket-vars]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "Instead of a single value, this solution uses a ticket and turn variable in combination to build a lock."
[^ostep-ticket-algo]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "Unlock is accomplished simply by incrementing the turn such that the next waiting thread (if there is one) can now enter the critical section."
[^ostep-ticket-progress]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "it ensures progress for all threads."
[^ostep-spin-preempt]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "imagine the case where the thread holding the lock is preempted within a critical section. The scheduler might then run every other thread (imagine there are N − 1 others), each of which tries to acquire the lock."
[^ostep-spin-nminus1]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "The problem gets worse with N threads contending for a lock; N − 1 time slices may be wasted in a similar manner"
[^ostep-uni-preempt]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "To work correctly on a single processor, it requires a preemptive scheduler"
[^ostep-yield]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "when you are going to spin, instead give up the CPU to another thread."
[^ostep-yield-state]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "yield is simply a system call that moves the caller from the running state to the ready state"
[^ostep-yield-99]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "the other 99 will each call lock(), find the lock held, and yield the CPU."
[^ostep-yield-starve]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "this approach does not address starvation. A thread may get caught in an endless yield loop while other threads repeatedly enter and exit the critical section."
[^ostep-queue-why]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "we must explicitly exert some control over which thread next gets to acquire the lock after the current holder releases it."
[^ostep-park]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "park() to put a calling thread to sleep, and unpark(threadID) to wake a particular thread as designated by threadID."
[^ostep-handoff]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "we just pass the lock directly from the thread releasing the lock to the next thread acquiring it"
[^ostep-guard-short]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "the time spent spinning is quite limited (just a few instructions inside the lock and unlock code"
[^ostep-park-race]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "a problem sometimes called the wakeup/waiting race"
[^ostep-setpark]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "If it then happens to be interrupted and another thread calls unpark before park is actually called, the subsequent park returns immediately instead of sleeping."
[^ostep-guard-kernel]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "A different solution could pass the guard into the kernel."
[^ostep-twophase]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "A two-phase lock realizes that spinning can be useful, particularly if the lock is about to be released. So in the first phase, the lock spins for a while, hoping that it can acquire the lock."
[^ostep-twophase-linux]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "The Linux lock above is a form of such a lock, but it only spins once"
[^futex7-build]: futex(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/futex.7.en.html> — "Futexes are very basic and lend themselves well for building higher-level locking abstractions such as mutexes, condition variables, read-write locks, barriers, and semaphores."
[^futex7-lib]: futex(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/futex.7.en.html> — "Most programmers will in fact not be using futexes directly but will instead rely on system libraries built on them"
[^futex-nowrapper]: futex(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/futex.2.en.html> — "Note: glibc provides no wrapper for futex(), necessitating the use of syscall(2)."
[^atomics-futex-linux]: Rust Atomics and Locks, ch.8 Operating System Primitives — <https://marabos.nl/atomics/os-primitives.html> — "On Linux systems, the pthread synchronization primitives are all implemented using the futex syscall."
[^ostep-futex-linux]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "Linux provides a futex which is similar to the Solaris interface but provides more in-kernel functionality."
[^futex-userspace]: futex(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/futex.2.en.html> — "When using futexes, the majority of the synchronization operations are performed in user space."
[^futex7-nonc]: futex(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/futex.7.en.html> — "Futex operation occurs entirely in user space for the noncontended case. The kernel is involved only to arbitrate the contended case."
[^futex-uncontended]: futex(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/futex.2.en.html> — "In the uncontended case, a thread can access or modify the lock state with atomic instructions, for example atomically changing it from not acquired to acquired using an atomic compare-and-exchange instruction."
[^futex-wait-atomic]: futex(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/futex.2.en.html> — "This load, the comparison with the expected value, and starting to sleep are performed atomically and totally ordered with respect to other futex operations on the same futex word."
[^futex-eagain]: futex(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/futex.2.en.html> — "If the futex value does not match val, then the call fails immediately with the error EAGAIN. The purpose of the comparison with the expected value is to prevent lost wake-ups."
[^futex-cab]: futex(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/futex.2.en.html> — "blocking via a futex is an atomic compare-and-block operation."
[^ostep-futex-wait]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "puts the calling thread to sleep, assuming the value at the address address is equal to expected. If it is not equal, the call returns immediately."
[^futex-wake]: futex(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/futex.2.en.html> — "This operation wakes at most val of the waiters that are waiting (e.g., inside FUTEX_WAIT) on the futex word at the address uaddr."
[^atomics-fastpath]: Rust Atomics and Locks, ch.9 Building Our Own Locks — <https://marabos.nl/atomics/building-locks.html> — "in the uncontended case, the ideal situation where threads are not attempting to acquire the lock simultaneously, both the wait() and wake_one() calls are entirely avoided."
[^std-futex-states]: rust-lang/rust tag 1.95.0, library/std/src/sys/sync/mutex/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/sync/mutex/futex.rs> — "const CONTENDED: State = 2; // locked, and other threads waiting (contended)"
[^std-futex-fast]: rust-lang/rust tag 1.95.0, library/std/src/sys/sync/mutex/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/sync/mutex/futex.rs> — "if self.futex.compare_exchange(UNLOCKED, LOCKED, Acquire, Relaxed).is_err() { self.lock_contended(); }"
[^std-futex-spin]: rust-lang/rust tag 1.95.0, library/std/src/sys/sync/mutex/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/sync/mutex/futex.rs> — "// Spin first to speed things up if the lock is released quickly."
[^std-futex-spin-load]: rust-lang/rust tag 1.95.0, library/std/src/sys/sync/mutex/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/sync/mutex/futex.rs> — "// We only use `load` (and not `swap` or `compare_exchange`) while spinning, to be easier on the caches."
[^std-futex-spin100]: rust-lang/rust tag 1.95.0, library/std/src/sys/sync/mutex/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/sync/mutex/futex.rs> — "fn spin(&self) -> State { let mut spin = 100;"
[^std-futex-spin-stop]: rust-lang/rust tag 1.95.0, library/std/src/sys/sync/mutex/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/sync/mutex/futex.rs> — "// We stop spinning when the mutex is UNLOCKED, // but also when it's CONTENDED."
[^std-futex-contended-swap]: rust-lang/rust tag 1.95.0, library/std/src/sys/sync/mutex/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/sync/mutex/futex.rs> — "// We changed it from UNLOCKED to CONTENDED, so we just successfully locked it."
[^std-futex-wait]: rust-lang/rust tag 1.95.0, library/std/src/sys/sync/mutex/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/sync/mutex/futex.rs> — "// Wait for the futex to change state, assuming it is still CONTENDED. futex_wait(&self.futex, CONTENDED, None);"
[^std-futex-unlock]: rust-lang/rust tag 1.95.0, library/std/src/sys/sync/mutex/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/sync/mutex/futex.rs> — "if self.futex.swap(UNLOCKED, Release) == CONTENDED {"
[^std-futex-wake-one]: rust-lang/rust tag 1.95.0, library/std/src/sys/sync/mutex/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/sync/mutex/futex.rs> — "// We only wake up one thread. When that thread locks the mutex, it // will mark the mutex as CONTENDED (see lock_contended above), // which makes sure that any other waiting threads will also be // woken up eventually."
[^std-sys-futex]: rust-lang/rust tag 1.95.0, library/std/src/sys/pal/unix/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/pal/unix/futex.rs> — "libc::FUTEX_WAIT_BITSET | libc::FUTEX_PRIVATE_FLAG,"
[^std-sys-futex-ret]: rust-lang/rust tag 1.95.0, library/std/src/sys/pal/unix/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/pal/unix/futex.rs> — "Returns directly if the futex doesn't hold the expected value."
[^ostep-threadsafe]: OSTEP, ch.29 Lock-based Concurrent Data Structures — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks-usage.pdf> — "Adding locks to a data structure to make it usable by threads makes the structure thread safe."
[^ostep-biglock]: OSTEP, ch.29 Lock-based Concurrent Data Structures — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks-usage.pdf> — "it simply adds a single lock, which is acquired when calling a routine that manipulates the data structure, and is released when returning from the call."
[^ostep-biglock-done]: OSTEP, ch.29 Lock-based Concurrent Data Structures — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks-usage.pdf> — "Note that if the data structure is not too slow, you are done! No need to do something fancy if something simple will work."
[^ostep-coarse]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "instead of one big lock that is used any time any critical section is accessed (a coarse-grained locking strategy)"
[^ostep-fine]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "thus allowing more threads to be in locked code at once"
[^ostep-counter-slow]: OSTEP, ch.29 Lock-based Concurrent Data Structures — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks-usage.pdf> — "having two threads each update the counter one million times concurrently leads to a massive slowdown (taking over 5 seconds!)"
[^ostep-approx]: OSTEP, ch.29 Lock-based Concurrent Data Structures — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks-usage.pdf> — "The approximate counter works by representing a single logical counter via numerous local physical counters, one per CPU core, as well as a single global counter."
[^ostep-approx-flush]: OSTEP, ch.29 Lock-based Concurrent Data Structures — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks-usage.pdf> — "the local values are periodically transferred to the global counter, by acquiring the global lock and incrementing it by the local counter’s value; the local counter is then reset to zero."
[^ostep-approx-tradeoff]: OSTEP, ch.29 Lock-based Concurrent Data Structures — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks-usage.pdf> — "If S is low, performance is poor (but the global count is always quite accurate); if S is high, performance is excellent, but the global count lags"
[^ostep-errorpaths]: OSTEP, ch.29 Lock-based Concurrent Data Structures — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks-usage.pdf> — "a recent study of Linux kernel patches found that a huge fraction of bugs (nearly 40%) are found on such rarely-taken code paths"
[^ostep-narrow]: OSTEP, ch.29 Lock-based Concurrent Data Structures — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks-usage.pdf> — "so that the lock and release only surround the actual critical section in the insert code, and that a common exit path is used in the lookup code."
[^ostep-handoverhand]: OSTEP, ch.29 Lock-based Concurrent Data Structures — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks-usage.pdf> — "When traversing the list, the code first grabs the next node’s lock and then releases the current node’s lock (which inspires the name hand-over-hand)."
[^ostep-hoh-slow]: OSTEP, ch.29 Lock-based Concurrent Data Structures — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks-usage.pdf> — "in practice, it is hard to make such a structure faster than the simple single lock approach, as the overheads of acquiring and releasing locks for each node of a list traversal is prohibitive."
[^ostep-tip-concurrency]: OSTEP, ch.29 Lock-based Concurrent Data Structures — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks-usage.pdf> — "Adding more locks and complexity can be your downfall."
[^ostep-rw-idea]: OSTEP, ch.31 Semaphores — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-sema.pdf> — "as long as we can guarantee that no insert is on-going, we can allow many lookups to proceed concurrently."
[^ostep-rw-impl]: OSTEP, ch.31 Semaphores — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-sema.pdf> — "any thread that wishes to acquire the write lock will have to wait until all readers are finished; the last one to exit the critical section calls sem_post() on “writelock” and thus enables a waiting writer to acquire the lock."
[^ostep-rw-starve]: OSTEP, ch.31 Semaphores — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-sema.pdf> — "it would be relatively easy for readers to starve writers."
[^ostep-rw-caution]: OSTEP, ch.31 Semaphores — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-sema.pdf> — "They often add more overhead (especially with more sophisticated implementations)"
[^std-mutex-guard]: Rust std 1.95.0, `std::sync::Mutex` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html> — "The data can only be accessed through the RAII guards returned from lock and try_lock, which guarantees that the data is only ever accessed when the mutex is locked."
[^book-guard]: The Rust Programming Language, 16.3 Shared-State Concurrency — <https://doc.rust-lang.org/1.95.0/book/ch16-03-shared-state.html> — "The MutexGuard type implements Deref to point at our inner data; the type also has a Drop implementation that releases the lock automatically when a MutexGuard goes out of scope"
[^std-rwlock]: Rust std 1.95.0, `std::sync::RwLock` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.RwLock.html> — "An RwLock will allow any number of readers to acquire the lock as long as a writer is not holding the lock."
[^std-rwlock-policy]: Rust std 1.95.0, `std::sync::RwLock` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.RwLock.html> — "The priority policy of the lock is dependent on the underlying operating system’s implementation, and this type does not guarantee that any particular policy will be used. In particular, a writer which is waiting to acquire the lock in write might or might not block concurrent calls to read"
[^std-rwlock-deadlock]: Rust std 1.95.0, `std::sync::RwLock` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.RwLock.html> — "let _rg1 = lock.read(); | | // will block | let _wg = lock.write(); // may deadlock | let _rg2 = lock.read();"
[^std-rwlock-bounds]: Rust std 1.95.0, `std::sync::RwLock` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.RwLock.html> — "It is required that T satisfies Send to be shared across threads and Sync to allow concurrent access through readers."
[^atomics-weak-reason]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "In fact, this is the reason why a weak version of compare_exchange exists."
[^atomics-mutex-hb]: Rust Atomics and Locks, ch.9 Building Our Own Locks — <https://marabos.nl/atomics/building-locks.html> — "the happens-before relationship is established through the acquire swap operation observing the value stored by the release swap operation."
[^atomics-mutex-ex]: Rust Atomics and Locks, ch.3 Memory Ordering — <https://marabos.nl/atomics/memory-ordering.html> — "This means that there will be a happens-before relationship between unlocking a mutex and subsequently locking it."
[^std-futex-spin-cas]: rust-lang/rust tag 1.95.0, library/std/src/sys/sync/mutex/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/sync/mutex/futex.rs> — "// If it's unlocked now, attempt to take the lock // without marking it as contended."
[^ostep-intr-simple]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "The main positive of this approach is its simplicity."
