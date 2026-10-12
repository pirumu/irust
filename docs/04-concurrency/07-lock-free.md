# P4 · Chương 07 — Atomic operation và lock-free

## Bạn sẽ hiểu được gì

1. **Q1.** Các phép `load`, `store`, `swap`, `fetch_add` và `compare_exchange` làm gì, và vì sao mọi primitive đồng bộ (mutex, condvar…) cuối cùng đều dựng trên chúng? → mục 1
2. **Q2.** `compare_exchange` khác `compare_exchange_weak` ở đâu, một vòng CAS (compare-and-swap loop) trông thế nào, và khi nào nên chọn bản nào? → mục 2
3. **Q3.** "Spurious failure" đến từ phần cứng nào, và rustc biến các phép atomic thành lệnh máy gì trên x86-64 và AArch64 (có và không có LSE)? → mục 3
4. **Q4.** "Lock-free", "wait-free", "obstruction-free" nghĩa là gì, nguồn nào định nghĩa, và `std` hứa gì (và không hứa gì)? → mục 4
5. **Q5.** ABA problem là gì, vì sao giải phóng node rồi cấp phát lại cùng địa chỉ làm nó nguy hiểm, và hazard pointer / epoch-based reclamation giải quyết phần "thu hồi bộ nhớ" ra sao? → mục 5
6. **Q6.** Futex cho "atomic wait/notify" thế nào, và vì sao fast path không cần syscall? → mục 6
7. **Q7.** Khi nào *không* nên dùng lock-free? → mục 7
8. **Q8.** Vì sao `Arc::clone` chỉ cần `Relaxed` còn `drop` cần `Release` + fence `Acquire`? → mục 8

## Kiến thức cần trước

- [Chương 03 — Lock](03-lock.md): test-and-set, CAS, spin lock, futex fast/slow path (chương này đào sâu phần "atomic" nằm dưới chúng).
- [Chương 06 — Memory model](06-memory-model.md): `Ordering`, happens-before, acquire/release. Chương này dùng các ordering đó mà không giải thích lại.
- [P2 · Chương 06 — Cache và NUMA](../02-cpu/06-cache-va-numa.md): cache line, MESI, false sharing.
- [P2 · Chương 03 — Assembly x86-64 và AArch64](../02-cpu/03-assembly-x86-64-va-aarch64.md): đọc được vài dòng assembly.

## 1. Atomic operation và các phép cơ bản (Q1)

> **Trả lời ngắn.** Atomic operation là thao tác không chia cắt được: nó xảy ra trọn vẹn trước hoặc trọn vẹn sau thao tác của thread khác, không bao giờ dở dang[^atomics-intro]. Có bốn họ phép cơ bản: load/store, swap, fetch-and-modify (như `fetch_add`) và compare-and-exchange[^atomics-types]. Mutex, condition variable và các primitive đồng bộ khác đều được cài bằng chính những phép này[^atomics-building-block].

**Giải thích.**

*Với service của bạn.* Hãy hình dung `ingest`, một service Rust nhận event từ nhiều agent: mỗi kết nối có một handler thread, ngoài ra có vài worker thread xử lý job. Nó cần năm thứ chung giữa các thread: bộ đếm `requests_total`, bộ cấp ID cho job (có trần), một kho buffer dùng lại, hàng đợi job mà worker phải chờ, và một `Arc<Config>` mà thread nào cũng đọc. Ta sẽ bám theo `ingest` suốt chương (nhận xét của người soạn: đây là ví dụ giả định, không phải code trong repo). Mục này bắt đầu từ thứ đơn giản nhất: đếm số request.

### 1.1 Vì sao bộ đếm thường không đủ

Nếu `requests_total` là một `u64` thường, hai handler thread cùng đọc-sửa-ghi nó là data race, tức undefined behavior trong Rust[^std-datarace]. Atomic giải chuyện này: nhiều thread đọc và sửa cùng một biến vẫn an toàn, vì mỗi thao tác xảy ra trọn vẹn trước hoặc trọn vẹn sau thao tác khác[^atomics-intro]. Cũng vì thế mà mutex hay condvar không cần phép màu nào riêng: chúng chỉ là code dựng trên các thao tác atomic[^atomics-building-block].

Trong Rust, các phép này là method của những kiểu `Atomic*` (`AtomicI32`, `AtomicUsize`…) trong `std::sync::atomic`. Gọi được qua `&` nhờ interior mutability, và chia thành ba nhóm: load/store, fetch-and-modify, compare-and-exchange[^atomics-types]. Mỗi thao tác nhận một `Ordering`, chủ đề của [chương 06](06-memory-model.md); chương này chủ yếu dùng `Relaxed`.

### 1.2 Bốn họ phép toán

| Họ | Phép | Ý nghĩa |
|---|---|---|
| Load/store | `load`, `store` | đọc / ghi nguyên giá trị |
| Swap | `swap` | ghi giá trị mới, trả giá trị cũ (sách gọi là "fetch_store" nhưng đặt tên `swap`)[^atomics-swap] |
| Fetch-and-modify | `fetch_add`, `fetch_sub`, `fetch_or`, `fetch_and`, `fetch_xor`, `fetch_max`, `fetch_min`… | sửa rồi trả giá trị **trước** khi sửa, tất cả là một thao tác atomic[^atomics-fetch] |
| Compare-and-exchange | `compare_exchange`, `compare_exchange_weak`, `fetch_update` | chỉ ghi nếu giá trị hiện tại bằng giá trị mong đợi[^atomics-cae] |

Họ `fetch_*` trả giá trị cũ chứ không phải giá trị mới. Sách minh họa bằng `a = 100`: `a.fetch_add(23)` trả `100`, và `load` kế tiếp thấy `123`[^atomics-fetch-demo].

### 1.3 Đếm request cho đúng

Giờ quay lại bộ đếm. Có một lỗi kinh điển: viết `x.store(x.load() + 1)` rồi tưởng là atomic, vì cả hai bước đều atomic.

```rust
// SAI: hai handler thread có thể cùng đọc N rồi cùng ghi N+1, mất một lượt đếm
REQUESTS_TOTAL.store(REQUESTS_TOTAL.load(Relaxed) + 1, Relaxed);
// ĐÚNG: một thao tác đọc-sửa-ghi duy nhất
REQUESTS_TOTAL.fetch_add(1, Relaxed);
```

Dòng đầu không phải data race theo định nghĩa của `std`, vì không có truy cập non-atomic nào (xem [chương 06, mục 3](06-memory-model.md))[^std-datarace]. Nó là race condition: giữa `load` và `store` có thread khác chen vào, nên cập nhật của thread đó bị ghi đè. `fetch_add` tránh được chuyện này vì đọc, cộng và ghi diễn ra như một thao tác duy nhất[^atomics-fetch].

<details><summary>Đi sâu: tràn số trong <code>fetch_*</code></summary>

`fetch_add` và `fetch_sub` khi tràn số thì wrap chứ không panic[^atomics-wrap]. Docs `std` ghi y như vậy cho `AtomicUsize::fetch_add`[^std-fetchadd-wrap]. Với bộ đếm `requests_total` thì không sao, nhưng nếu bạn cần chặn hẳn việc tràn thì phải dùng compare-and-exchange: sách liệt kê ba cách xử lý tràn số và gọi cách thứ ba, ngăn phép cộng xảy ra khi nó sẽ tràn, là cách "arguably the only truly correct one"[^atomics-overflow-cae]. `Arc::clone` trong `std` chọn abort khi refcount vượt ngưỡng (xem §8)[^atomics-arc-abort].

</details>

**Quay lại Q1:** mỗi phép atomic là một thao tác không chia cắt; `fetch_add` là một lần đọc-sửa-ghi, còn `load` rồi `store` thì không, và mọi primitive đồng bộ khác đứng trên các phép này.

## 2. `compare_exchange`, CAS loop và bản weak (Q2)

> **Trả lời ngắn.** `compare_exchange` chỉ ghi giá trị mới nếu giá trị hiện tại đúng bằng giá trị ta mong đợi, tất cả trong một thao tác[^atomics-cae]. Muốn cập nhật có điều kiện thì bọc nó trong vòng lặp: đọc, tính, thử ghi, thất bại thì thử lại với giá trị mới nhất. Bản `compare_exchange_weak` được phép trả lỗi ngay cả khi giá trị khớp[^atomics-weak]. Dùng nó khi một lần lỗi giả không đáng kể (như trong vòng lặp), còn không lặp thì dùng bản strong[^atomics-strong-when].

**Giải thích.**

*Với service `ingest`.* Mỗi job `ingest` nhận được cần một ID, và service chỉ chấp nhận tối đa 1000 job. `fetch_add` không làm được việc này: nó cộng rồi mới cho ta biết kết quả, nên khi phát hiện vượt trần thì ID đã bị cấp mất. Ta phải kiểm tra rồi mới ghi, mà hai bước đó phải thành một. Đó là việc của `compare_exchange`. (Nhận xét của người soạn: nó giống `UPDATE … WHERE version = ?` trong DB với optimistic locking. Chỉ ghi nếu chưa ai đổi dòng đó từ lúc ta đọc; nếu rồi thì đọc lại và làm lại.)

### 2.1 `compare_exchange` và kết quả của nó

`compare_exchange` kiểm tra xem giá trị atomic có bằng một giá trị cho trước không, và chỉ khi đúng mới thay bằng giá trị mới, cả hai bước là một thao tác[^atomics-cae]. Sách giáo khoa hệ điều hành mô tả compare-and-swap y hệt như vậy[^ostep-cas]. Kết quả trả về là `Result<T, T>`: `Ok(giá trị cũ)` khi thành công, `Err(giá trị thực đang có)` khi thất bại.

### 2.2 Mẫu CAS loop

Gần như lúc nào `compare_exchange` cũng đi trong một vòng lặp. Ta `load` giá trị hiện tại, tính giá trị mới mà không cần để ý thread khác, rồi `compare_exchange`. Nếu thất bại thì thread khác đã đổi biến trong lúc ta tính; ta lấy luôn giá trị `Err` trả về và thử lại. Khoảng giữa `load` và lần ghi ngắn đến mức vòng này hiếm khi phải lặp quá vài lần[^atomics-cas-loop].

```mermaid
flowchart TD
  L["cur = load()"] --> C["new = f(cur)"]
  C --> X{"compare_exchange(cur, new)<br/>thành công?"}
  X -- "Ok" --> D["xong"]
  X -- "Err(actual)" --> U["cur = actual"]
  U --> C
```

Bộ cấp ID có trần của `ingest` chính là một CAS loop như thế; code đầy đủ và kết quả chạy nằm ở phần Đi sâu cuối mục.

### 2.3 Hai ordering và bản weak

`compare_exchange` nhận hai ordering. `success` áp dụng cho cả load lẫn store khi so sánh đúng, còn `failure` chỉ áp dụng cho load khi so sánh sai, và chỉ có thể là `SeqCst`, `Acquire` hoặc `Relaxed`[^std-failure-order]. Ví dụ thật: fast path của `std::sync::Mutex` là `compare_exchange(UNLOCKED, LOCKED, Acquire, Relaxed)` (xem [Chương 03, mục 6](03-lock.md))[^std-futex-fast].

`compare_exchange_weak` được phép trả `Err` ngay cả khi giá trị khớp[^atomics-weak]. Đổi lại, trên một số nền tảng nó sinh code hiệu quả hơn[^std-weak]. Vậy chọn bản nào? Nếu đã nằm trong vòng lặp như bộ cấp ID, một lần thất bại giả chỉ làm ta lặp thêm một vòng, nên dùng weak. Nếu không lặp thì dùng strong, ví dụ khởi tạo khóa lười `KEY.compare_exchange(0, new_key, …)`, nơi ta không muốn thất bại giả làm hàm trả nhầm kết quả[^atomics-strong-when]. Thất bại giả đến từ đâu thì mục 3 trả lời.

<details><summary>Đi sâu: bộ cấp ID có trần và <code>fetch_update</code></summary>

Ví dụ đã chạy (rustc 1.95.0, tám thread, mỗi thread xin ID 500 lần, trần 1000): bộ cấp ID không bao giờ vượt trần, điều mà `fetch_add` đơn thuần không làm được vì nó cộng trước khi kiểm tra.

```rust
static NEXT_ID: AtomicU32 = AtomicU32::new(0);

fn allocate_new_id() -> Option<u32> {
    let mut id = NEXT_ID.load(Relaxed);
    loop {
        if id >= 1000 { return None; }
        match NEXT_ID.compare_exchange_weak(id, id + 1, Relaxed, Relaxed) {
            Ok(_) => return Some(id),
            Err(actual) => id = actual,
        }
    }
}
```

Kết quả: `granted=1000 final=1000` (4000 lần xin, đúng 1000 được cấp). Cùng ý có thể viết bằng `fetch_update`, mà sách gọi là tiện ích cho mẫu compare-and-exchange loop[^atomics-fetchupdate]. Với `a` khởi đầu 998 và closure `|v| if v < 1000 { Some(v+1) } else { None }`, bốn lần gọi trả `Ok(998)`, `Ok(999)`, `Err(1000)`, `Err(1000)` (đã chạy). Docs `std` dặn rằng `fetch_update` "is not magic": phần cứng không cung cấp nó, nó không phải critical section hay mutex, và vẫn chịu các nhược điểm thông thường của CAS, nhất là ABA[^std-fetchupdate-magic].

</details>

**Quay lại Q2:** `compare_exchange` là "ghi nếu chưa ai đổi"; vòng CAS lặp lại cho đến khi thắng, và `_weak` chấp nhận thất bại giả để rẻ hơn khi đã nằm trong vòng lặp.

## 3. Phần cứng thực hiện thế nào (Q3)

> **Trả lời ngắn.** Thất bại giả sinh ra từ phần cứng kiểu load-linked/store-conditional (LL/SC): lệnh store-conditional được phép thất bại dù vùng nhớ không đổi[^atomics-llsc-fn]. Trên x86-64 thì khác: `compare_exchange` và bản weak biên dịch ra cùng một lệnh `lock cmpxchg`[^atomics-x86-weak]. ARMv8.1 thêm lệnh atomic đơn như `ldadd` và `cas`; khi dùng `cas`, strong và weak không còn khác nhau[^atomics-lse].

**Giải thích.**

*Với service `ingest`.* Bạn build `ingest` ra binary cho server x86-64, và nhiều đội còn build thêm cho máy AArch64 (nhận xét của người soạn). Cùng dòng `REQUESTS_TOTAL.fetch_add(1, Relaxed)` hay `compare_exchange_weak` ở bộ cấp ID sẽ thành lệnh máy rất khác nhau tùy kiến trúc. Mục này xem từng kiến trúc làm gì, và qua đó thấy vì sao bản weak tồn tại.

### 3.1 x86-64: tiền tố `lock`

Vấn đề bắt đầu từ chỗ ngay cả một lệnh `add` cũng không atomic. Bộ xử lý tách nó thành vài vi lệnh (load giá trị, cộng, store kết quả), nên khi nhiều core chạy song song, thread khác có thể chen vào giữa các vi lệnh đó[^atomics-add-micro].

Intel giải quyết bằng tiền tố `lock`, biến các lệnh như `add` thành atomic. Ban đầu tiền tố này chặn mọi core khác truy cập bộ nhớ trong suốt lệnh; các bộ xử lý mới cài tinh vi hơn[^atomics-lock-prefix]. Tiền tố chỉ dùng được với một số lệnh (`add`, `sub`, `and`, `or`, `xor`…). Riêng `xchg`, tương ứng với `swap`, có `lock` ngầm định[^atomics-xchg]. Khi cần giá trị cũ thì có `xadd`[^atomics-xadd]. Phép nào không có lệnh đơn, như `fetch_or` nhiều bit hay `fetch_max`, được compiler chuyển thành một CAS loop trên `lock cmpxchg`[^atomics-cmpxchg-loop]. Cuối cùng, `compare_exchange` và `compare_exchange_weak` trên x86-64 là một lệnh giống hệt nhau[^atomics-x86-weak].

### 3.2 AArch64: load-exclusive/store-exclusive

Chip RISC như AArch64 đi hướng khác. Thay vì một lệnh atomic, nó có một cặp lệnh dùng cùng địa chỉ, gọi là **load-linked/store-conditional**. Store-conditional từ chối ghi nếu từ lần load-linked tới giờ có ai ghi đè vùng nhớ đó[^atomics-llsc-def].

Để cài đặt cho hiệu quả, phần cứng cho phép store-conditional thất bại oan (false negative): nó có thể từ chối ghi dù không ai đụng vào vùng nhớ[^atomics-llsc-fn]. Lý do là độ chính xác khi theo dõi truy cập có thể thô: theo từng khối 64 byte, từng kilobyte, thậm chí cả bộ nhớ[^atomics-llsc-coarse]. **Đây là nguồn gốc của spurious failure** mà `compare_exchange_weak` được phép để lộ ra ngoài.

Trên ARM64, ít nhất ở ARMv8 bản đầu, không phép fetch-and-modify hay compare-and-exchange nào là một lệnh đơn; thay vào đó là cặp `ldxr`/`stxr`[^atomics-llsc]. Compiler cố giữ số lệnh nằm giữa `ldxr` và `stxr` càng ít càng tốt[^atomics-llsc-few].

### 3.3 ARMv8.1 LSE

ARMv8.1 thêm lệnh atomic kiểu CISC. `ldadd` tương đương `fetch_add` mà không cần vòng LL/SC, có cả lệnh cho `fetch_max`, và `cas` ứng với `compare_exchange`. Khi dùng `cas`, strong và weak không còn khác nhau, giống x86-64[^atomics-lse]. Sách nói các lệnh này có thể nhanh hơn vì dễ tối ưu bằng phần cứng chuyên dụng[^atomics-lse-faster].

### 3.4 Chi phí khi CAS thất bại

Một `load` có thể rẻ hơn đáng kể so với một `compare_exchange` thất bại, phần vì `compare_exchange` thường đòi quyền truy cập độc quyền vào cache line[^atomics-cas-fail-cost] (MESI ở [P2 · Chương 06](../02-cpu/06-cache-va-numa.md)). Vì vậy vòng spin của `std::sync::Mutex` chỉ dùng `load`[^std-futex-spin-load]. Điều này cũng báo trước một ý ở mục 7: dưới tranh chấp cao, CAS thất bại liên tục không rẻ.

<details><summary>Đi sâu: assembly rustc 1.95.0 sinh ra cho x86-64 và AArch64 (không/có LSE)</summary>

Phần này đối chiếu sách ch.7 với assembly do rustc 1.95.0 sinh thật (`--emit asm -O`) cho `fetch_add`, `swap`, `compare_exchange`, `compare_exchange_weak` trên `AtomicU32` và `fetch_or(10)` trên `AtomicI32`, với ordering `AcqRel` (CAS: `AcqRel`/`Acquire`) và dùng giá trị trả về.

Assembly thật x86-64 (`x86_64-unknown-linux-gnu`, rút gọn):

```asm
fa:  lock xaddl %eax, (%rdi)      ; fetch_add dùng giá trị cũ
sw:  xchgl %eax, (%rdi)           ; swap (lock ngầm định)
cas: lock cmpxchgl %edx, (%rdi)   ; compare_exchange và _weak: cùng một lệnh
fo:  movl (%rdi), %eax            ; fetch_or(10): không có lệnh đơn → CAS loop
.L:  movl %eax, %ecx
     orl $10, %ecx
     lock cmpxchgl %ecx, (%rdi)
     jne .L
```

Ordering `AcqRel` và `Relaxed` cho cùng lệnh ở đây, khớp lời sách rằng trên x86-64 mọi thao tác bộ nhớ đã có ngữ nghĩa acquire/release[^atomics-x86-arm].

Tắt LSE (`-C target-feature=-lse`, target `aarch64-apple-darwin`) để thấy LL/SC thật:

```asm
_fa:   ldaxr w8,[x0] ; add w9,w8,#10 ; stlxr w10,w9,[x0] ; cbnz w10,_fa   ; vòng
_casw: ldaxr w1,[x0] ; cmp w1,#5 ; b.ne fail ; stlxr w9,w8,[x0]         ; MỘT lần thử, không vòng
_cas:  ldaxr w1,[x0] ; cmp w1,#5 ; b.ne fail ; stlxr w9,w8,[x0] ; cbnz w9,_cas  ; strong phải lặp
```

Bản `weak` chỉ có một `stlxr` và báo lỗi nếu nó thất bại; bản `strong` phải có `cbnz` để *che* thất bại giả. Đó là phần chi phí mà `weak` tiết kiệm được khi đã nằm trong vòng lặp của người dùng (suy luận của người soạn từ asm trên; docs `std` nói gọn rằng weak cho phép compiler sinh assembly tốt hơn khi compare-and-swap được dùng trong vòng lặp[^std-weak-loop]). Hậu tố `a`/`l` trong `ldaxr`/`stlxr` xuất hiện khi dùng `AcqRel`; với `Relaxed` cùng mã sinh `ldxr`/`stxr` (đã chạy lại cả hai, quan sát trong asm).

Với `aarch64-apple-darwin` mặc định, `rustc --print cfg` báo `target_feature="lse"` bật sẵn, và cùng mã Rust với `AcqRel` sinh `ldaddal` (fetch_add), `swpal` (swap), `ldsetal` (fetch_or, không cần CAS loop), `casal` (cả strong lẫn weak). Với `Relaxed` là `ldadd`, `swp`, `ldset`, `cas`; hậu tố `al` là acquire+release[^atomics-lse-al]. Các mnemonic này là output thật của rustc; sách chỉ nêu `ldadd` và `cas`. Cùng một dòng Rust cho mã khác hẳn tùy target feature (nhận xét của người soạn).

| Phép Rust | x86-64 | AArch64 không LSE | AArch64 có LSE |
|---|---|---|---|
| `fetch_add` | `lock xadd` | `ldaxr/add/stlxr` + `cbnz` | `ldaddal` |
| `swap` | `xchg` | `ldaxr/stlxr` + `cbnz` | `swpal` |
| `compare_exchange` | `lock cmpxchg` | `ldaxr/cmp/stlxr` + `cbnz` | `casal` |
| `compare_exchange_weak` | `lock cmpxchg` | `ldaxr/cmp/stlxr` (một lần) | `casal` |
| `fetch_or` nhiều bit | vòng `lock cmpxchg` | `ldaxr/orr/stlxr` + `cbnz` | `ldsetal` |

</details>

**Quay lại Q3:** thất bại giả đến từ store-conditional của LL/SC; x86-64 và AArch64 có LSE cho cùng một lệnh CAS cho cả hai bản, còn AArch64 không LSE thì bản strong phải bọc một vòng lặp.

## 4. Lock-free, wait-free, obstruction-free (Q4)

> **Trả lời ngắn.** Wait-free: mọi thread đều tiến triển dù thread khác bị trì hoãn hay hỏng[^hlm-waitfree]. Lock-free: chỉ đảm bảo *luôn có một thread nào đó* tiến triển[^hlm-lockfree]. Obstruction-free: chỉ đảm bảo cho thread chạy một mình, yếu hơn lock-free[^hlm-weaker]. `std` hứa các kiểu atomic là lock-free[^std-lockfree]. Nhưng nó không hứa wait-free[^std-not-waitfree].

**Giải thích.**

*Với service `ingest`.* Giả sử worker của `ingest` lấy job từ một hàng đợi dùng chung. Nếu hàng đợi bảo vệ bằng mutex và worker đang giữ mutex bị OS cho ra khỏi CPU, mọi worker khác đứng chờ theo (nhận xét của người soạn). Ba thuật ngữ trong mục này mô tả chính xác mức độ một cấu trúc dữ liệu chịu được tình huống đó: thread khác bị hoãn thì các thread còn lại có tiến triển tiếp được không.

### 4.1 Ba mức bảo đảm

Cả ba là *progress guarantee* của cấu trúc dữ liệu đồng thời. Các định nghĩa dưới đây lấy từ hai bài báo, không phải từ docs Rust:

| Thuật ngữ | Định nghĩa (theo nguồn) |
|---|---|
| **Wait-free** | đảm bảo mọi thread đều tiếp tục tiến triển dù các thread khác bị trì hoãn tùy ý (hoặc thậm chí hỏng)[^hlm-waitfree] |
| **Lock-free** | chỉ đảm bảo rằng luôn có một thread nào đó tiến triển[^hlm-lockfree]; cách phát biểu chặt hơn của Michael: nếu một thread thực hiện một số hữu hạn bước hướng tới một thao tác, thì *một thread nào đó* (có thể khác) phải đã tiến triển trong các bước đó[^michael-lockfree] |
| **Obstruction-free** | đảm bảo tiến triển cho mọi thread mà cuối cùng chạy "in isolation"[^hlm-of]; yếu hơn lock-free vì không đảm bảo tiến triển khi hai hay nhiều thread xung đột chạy đồng thời[^hlm-weaker] |

### 4.2 Quan hệ giữa ba mức

Bài báo về obstruction-free nói thẳng rằng nó yếu hơn lock-free[^hlm-weaker]. Còn wait-free thì mạnh hơn lock-free, và điều này suy ra trực tiếp từ hai định nghĩa: mọi thread tiến triển thì dĩ nhiên có một thread tiến triển (suy luận của người soạn). Bài báo cũng coi wait-free là hành vi lý tưởng, còn lock-free thường đã đủ tốt trong thực tế[^hlm-lockfree-enough]. Gộp lại: wait-free ⟹ lock-free ⟹ obstruction-free, và lock-free vẫn cho phép *một* thread bị bỏ đói.

Với `ingest`, cái lợi của lock-free chính là tình huống ở đầu mục. Michael viết rằng đối tượng lock-free không bị deadlock khi thread lỗi, và giữ hiệu năng ổn định kể cả khi thread bị trì hoãn[^michael-benefit]. Riêng obstruction-free, bài báo nói nó bảo đảm không thread nào bị chặn bởi sự chậm trễ hay hỏng của thread khác[^hlm-nonblocking].

### 4.3 `std` hứa gì?

Docs `std::sync::atomic` nói mọi kiểu atomic trong module là "guaranteed to be lock-free if they're available", nghĩa là chúng không nội bộ lấy một global mutex[^std-lockfree]. Nhưng `std` *không* hứa wait-free: `fetch_or`, chẳng hạn, có thể được cài bằng một vòng compare-and-swap[^std-not-waitfree]. Điều này khớp với asm ở §3: `fetch_or` nhiều bit trên x86-64 là vòng `cmpxchg`, còn khi có LSE là một lệnh `ldsetal` (hoặc `ldset`).

Cần phân biệt hai nghĩa của "lock-free". Ở `std`, nó nói về cách cài *một thao tác atomic đơn*. "Cấu trúc lock-free" thì là thuộc tính của cả thuật toán (nhận xét của người soạn).

<details><summary>Đi sâu: nguồn không cho gì</summary>

Hai bài báo chỉ định nghĩa ở mức văn xuôi; không có định nghĩa hình thức (ví dụ qua linearizability) trong nguồn đã tải, nên chương này không đưa ra.

</details>

**Quay lại Q4:** wait-free ⟹ lock-free ⟹ obstruction-free về độ mạnh; `std` hứa lock-free cho từng thao tác atomic nhưng không hứa wait-free.

## 5. ABA problem và thu hồi bộ nhớ (Q5)

> **Trả lời ngắn.** CAS so sánh *giá trị*, không so sánh *lịch sử*: nếu biến đổi A→B→A giữa `load` và `compare_exchange`, thao tác vẫn thành công[^atomics-aba]. Cùng địa chỉ không có nghĩa là cùng một object[^std-aba], nên đặc biệt nguy hiểm khi node đã bị giải phóng rồi cấp phát lại cùng địa chỉ (suy luận của người soạn). Hazard pointer giải quyết phần thu hồi bộ nhớ: chỉ hủy node khi không thread nào còn tham chiếu được[^michael-hp-scan]. Epoch-based reclamation đi theo cùng hướng đó[^ce-mechanism].

**Giải thích.**

*Với service `ingest`.* Mỗi lần xử lý event, handler thread cần một buffer. Để khỏi cấp phát mỗi lần, `ingest` giữ một kho buffer tự do dưới dạng stack lock-free: lấy buffer là `pop`, trả buffer là `push`. Đây là ví dụ quen thuộc để thấy ABA, vì `pop` đọc hai thứ rồi mới CAS, và giữa hai bước đó stack có thể đổi nhiều thứ mà `head` vẫn quay về giá trị cũ.

### 5.1 CAS không thấy lịch sử

CAS so sánh giá trị chứ không so sánh lịch sử. Nếu biến đổi từ A sang B rồi quay lại A sau `load` nhưng trước `compare_exchange`, thao tác vẫn thành công dù biến đã bị đổi (và đổi ngược lại)[^atomics-aba]. Với bộ đếm thông thường điều đó không sao. Nhưng có những thuật toán, thường là thuật toán dùng atomic pointer, mà đây là vấn đề thật. Docs `std` cũng cảnh báo rằng một con trỏ giữ cùng địa chỉ không có nghĩa là vẫn cùng một object ở địa chỉ đó[^std-aba]. Bài báo hazard pointer nói ABA ảnh hưởng hầu hết thuật toán lock-free[^michael-aba-almost].

### 5.2 Ví dụ: `pop` trên stack buffer

Stack là danh sách liên kết, `head` là `AtomicPtr<Node>`. `pop` của thread 1 làm ba bước: đọc `old = head`, đọc `next = old.next`, rồi `CAS(head, old, next)`. Kịch bản dưới đây đã chạy deterministic: một thread giả lập hai thread xen kẽ.

```mermaid
sequenceDiagram
  participant T1 as Thread 1 (pop)
  participant H as head
  participant T2 as Thread 2
  Note over H: head → A → B → C
  T1->>H: old = load() = A, next = A.next = B
  Note over T1: bị hoãn
  T2->>H: pop A (CAS A→B)
  T2->>H: pop B (CAS B→C)
  T2->>H: push lại A (A.next = C, CAS C→A)
  Note over H: head → A → C (B đã bị lấy ra khỏi stack)
  T1->>H: CAS(head, A, B) → THÀNH CÔNG vì head vẫn == A
  Note over H: head → B → C (B "sống lại", A biến mất)
```

Mã giả lập dùng `AtomicPtr<Node>` và `compare_exchange` thật, và in ra `CAS của thread 1: Ok(())` cùng `stack sau đó: B C`. CAS thành công, nhưng B, buffer thread 2 đã lấy ra, lại được gắn làm đỉnh stack, còn A biến mất. Trong hệ thống thật B có thể đã bị `free` rồi cấp phát lại, tức use-after-free; còn nếu chưa bị free thì thread khác `pop` sẽ nhận lại đúng buffer mà thread 2 đang dùng (suy luận của người soạn). Docs `std` cảnh báo đúng chỗ này: `load` rồi `compare_exchange` thành công không bảo đảm rằng thread khác đã không đổi giá trị trong khoảng giữa hai bước[^std-aba].

### 5.3 Cách xử lý

Chỉ kể những cách mà nguồn nói:

- **Tag / update counter** (phương pháp "IBM tag"): gắn một bộ đếm vào con trỏ để "A rồi lại A" thành hai giá trị khác nhau. Michael nhận xét phương pháp này cản trở việc thu hồi bộ nhớ tùy ý và đòi lệnh double-width mà các bộ xử lý 64-bit không có[^michael-tag].
- **Thu hồi bộ nhớ an toàn** (hazard pointer, xem 5.4): Michael viết rằng các lời giải cho thu hồi bộ nhớ, như GC và hazard pointer, thường ngăn luôn ABA như một tác dụng phụ, với ít hoặc không tốn thêm chi phí[^michael-aba-side-effect].
- GC *không* tự động ngăn ABA trong mọi trường hợp: chương trình chuyển node qua lại giữa hai stack vẫn có ABA dù GC hoàn hảo[^michael-gc-misconception].

### 5.4 Thu hồi bộ nhớ (memory reclamation)

Vấn đề thứ hai nằm ở chỗ nào được `free` node. Khi thread 1 `pop` một node, thread 2 có thể vừa đọc con trỏ tới node đó và sắp truy cập nội dung. Nếu thread 1 `free` ngay, thread 2 có thể làm hỏng object, trả kết quả sai, hoặc dereference con trỏ không hợp lệ; nếu bộ nhớ đã trả về OS (`munmap`), truy cập còn có thể thành lỗi nghiêm trọng[^michael-reclaim-problem]. Trong cấu trúc dùng lock thì dễ bảo đảm không ai truy cập node đã gỡ. Với cấu trúc lock-free điển hình trong môi trường không có GC thì không, vì để giữ tiến triển lock-free, mỗi thread phải được tự do thao tác bất cứ lúc nào[^michael-reclaim-problem].

Các cách làm trước đó dựa trên reference counter tổng hợp hoặc timestamp theo từng thread. Chúng đều *blocking* nếu thiếu hỗ trợ từ scheduler: một thread hỏng hay chậm có thể giữ counter không về 0 hoặc timestamp không tiến lên, nên bộ nhớ không giới hạn không được tái sử dụng[^michael-three-groups].

**Hazard pointer.** Ý tưởng của Michael: mỗi thread có một vài (thường một hoặc hai) *hazard pointer*. Chỉ chủ nhân ghi được vào nó, nhưng mọi thread đều đọc được; mỗi cái hoặc null, hoặc trỏ tới node mà thread có thể truy cập tiếp mà không validate lại[^michael-hp-core]. Thread gỡ node (retire) giữ nó trong danh sách riêng. Khi gom đủ R node, nó quét hazard pointer của các thread khác; node nào không bị ai chỉ tới thì thu hồi được[^michael-hp-scan]. Trên stack, `pop` cần hazard pointer (truy cập node ở dòng 10 nguy hiểm, CAS ở dòng 11 dễ dính ABA), còn `push` thì không[^michael-stack-hp]. Các thao tác lõi của kỹ thuật này chỉ cần đọc/ghi single-word[^michael-hp-waitfree].

**Epoch-based reclamation (crossbeam-epoch).** Docs `crossbeam-epoch` 0.9.18 mô tả cùng vấn đề: thread gỡ một phần tử khỏi map lock-free trong khi thread khác đang đọc nó thì phải chờ, rồi mới hủy được[^ce-problem]. Ngôn ngữ có garbage collector giải quyết chuyện này "trivially"[^ce-gc]. Cơ chế epoch hoạt động như sau: phần tử bị gỡ đi vào đống rác gắn epoch hiện tại; mỗi lần thread truy cập collection, nó kiểm tra epoch, cố tăng epoch, và hủy những rác đã đủ cũ để không thread nào còn tham chiếu tới[^ce-mechanism]. Trước khi load một `Atomic`, participant phải được *pin*, nghĩa là tuyên bố rằng object nào bị gỡ từ lúc này chưa được hủy ngay[^ce-pin].

**So sánh (nhận xét của người soạn, suy luận):** hazard pointer bảo vệ *từng node* thread đang giữ; epoch bảo vệ *theo thời kỳ* (pin một lần cho cả đoạn truy cập, nhưng thread pin lâu giữ lại rác của mọi người). Docs crossbeam-epoch đã tải không so sánh hai cách.

<details><summary>Đi sâu: stack, queue Michael–Scott và false sharing</summary>

**Lịch sử.** Michael cho biết ABA do IBM nêu lần đầu trong tài liệu về CAS trên System/370[^michael-aba].

**Stack (IBM freelist).** Michael trình bày stack lock-free dựa trên thuật toán IBM freelist[^michael-stack]. Dạng đơn giản: `head: AtomicPtr<Node>`.

- **push**: tạo node, đặt `node.next = head.load()`, rồi `CAS(head, old, node)`; nếu thất bại thì cập nhật `node.next = head hiện tại` và thử lại.
- **pop**: `old = head.load()`, `next = old.next`, `CAS(head, old, next)`; thất bại thì lặp lại. Đây là chỗ ABA và use-after-free xuất hiện (§5).

Ví dụ đã chạy: bốn thread, mỗi thread push 10 000 phần tử bằng `compare_exchange_weak(h, n, Release, Relaxed)` trên `AtomicPtr`; sau khi mọi push xong, một thread `pop` hết. Kết quả `pushed=40000 popped=40000 sum=799980000`, đúng bằng tổng 0..39999. Bản `pop` **cố ý không `free`** và chỉ chạy sau khi mọi `push` xong, nên né ABA và reclamation, hai chỗ khó. Đây là minh họa `AtomicPtr`, không phải mã nên dùng (§7).

**Queue (Michael–Scott).** Michael dùng queue FIFO lock-free của Michael–Scott làm case study. Đó là danh sách liên kết đơn có node giả ở đầu. Enqueue gắn node vào cuối rồi cập nhật `Tail` (nếu `Tail` chưa trỏ đúng node cuối thì cập nhật nó trước); dequeue dịch `Head` sau khi đọc dữ liệu từ node thứ hai[^michael-queue]. Nếu một thread bị hoãn khi để đối tượng ở trạng thái chưa ổn định, thread khác đưa nó về ổn định rồi mới làm việc của mình[^michael-helping] ("helping"). Cơ chế này là lý do một thread bị hoãn không chặn thread khác (suy luận của người soạn, nối với §4). Với queue, Michael chỉ ra các CAS ở dòng 7, 8, 9 của enqueue đều là "ABA hazard"[^michael-queue-hazards]. Tức là enqueue cần hazard pointer cùng bước validate, khác `push` của stack vốn không cần hazard pointer (nhận xét của người soạn).

**False sharing.** Hai biến atomic không liên quan nằm chung một cache line thì thao tác trên biến này làm chậm biến kia; muốn tránh thì đặt chúng xa nhau[^atomics-false-sharing]. Tùy bộ xử lý, có thể cần alignment 128 byte mới thấy hiệu ứng[^atomics-128]. Chi tiết ở [P2 · Chương 06](../02-cpu/06-cache-va-numa.md). Với queue, `Head` và `Tail` chung cache line sẽ làm enqueue và dequeue tranh nhau dù không tranh chấp logic (suy luận của người soạn).

</details>

**Quay lại Q5:** CAS không thấy lịch sử nên ABA lọt qua; muốn an toàn thì phải có cách thu hồi bộ nhớ (hazard pointer, epoch) hoặc tag để A-lần-hai khác A-lần-một.

## 6. Atomic wait/notify và futex (Q6)

> **Trả lời ngắn.** Chờ lâu thì thread phải ngủ; trên Linux việc đó là syscall futex, hoạt động trên một số nguyên atomic 32-bit với `FUTEX_WAIT` và `FUTEX_WAKE`[^atomics-futex-basic]. `FUTEX_WAIT` nhận giá trị mong đợi và từ chối chặn nếu không khớp, nên wake-up không bị mất[^atomics-futex-race]. Khi không tranh chấp thì chỉ cần lệnh atomic, không cần vào kernel[^futex-uncontended].

**Giải thích.**

*Với service `ingest`.* Khi hàng đợi job trống, worker của `ingest` không có gì làm. Nó không thể quay trong CAS loop mãi, vì sẽ đốt CPU của cả máy cho việc chờ. Nó phải ngủ, và đến khi handler thread đẩy job vào thì được đánh thức. Mục này xem việc ngủ-và-đánh-thức ấy đứng trên atomic ra sao.

### 6.1 Vì sao CAS loop không đủ

CAS loop hợp với những đoạn tranh chấp ngắn (nhận xét của người soạn). Chờ lâu thì thread cần ngủ, và trên Linux việc ngủ đi qua syscall futex: các primitive pthread đều được cài bằng nó[^atomics-futex-linux].

Cơ chế `FUTEX_WAIT`/`FUTEX_WAKE`, phép so sánh-rồi-chặn nguyên tử trả `EAGAIN` để không mất wake-up, và state machine futex 0/1/2 của `std::sync::Mutex` (fast path `compare_exchange(UNLOCKED, LOCKED, Acquire, Relaxed)`, unlock `swap(UNLOCKED, Release)`) đã giải thích ở [Chương 03, mục 5 và 6](03-lock.md). Phần dưới chỉ ghi các điểm riêng của góc nhìn atomic.

### 6.2 Kernel nhớ ai chờ, giá trị do bạn giữ

Kernel chỉ nhớ thread nào đang chờ ở địa chỉ nào; còn giá trị trong biến atomic hoàn toàn do chương trình của bạn quản lý[^atomics-futex-kernel]. Vì vậy với worker của `ingest`, biến atomic có thể là số job đang chờ trong hàng đợi, và ý nghĩa của từng giá trị là do code `ingest` quy định, kernel không biết (nhận xét của người soạn). Một chi tiết cần nhớ: futex wait có thể thức dậy giả, nên worker thường phải bọc lệnh chờ trong một vòng lặp kiểm tra lại điều kiện[^atomics-futex-spurious].

### 6.3 Fast path không cần syscall

Chính vì trạng thái nằm trong biến atomic nên có đường nhanh. Bên báo hiệu (handler thread đẩy job) có thể bỏ qua `FUTEX_WAKE` khi không ai đang chờ, và kernel chỉ bị gọi khi thật sự cần chặn một thread. Đó là lý do các primitive dựa trên futex nhanh[^atomics-futex-fast].

<details><summary>Đi sâu: chuẩn hóa và futex trong <code>std</code></summary>

C++ năm 2020 thêm `atomic_wait` và `atomic_notify`[^atomics-cxx-wait]. Trong trang `AtomicUsize` docs 1.95.0 đã tải, danh sách method không có `wait`/`notify` (quan sát của người soạn; biên dịch `AtomicU32::new(0).wait(0)` bằng rustc 1.95.0 báo `no method named wait`). `std` dùng futex bên trong; lớp syscall (`SYS_futex` với `FUTEX_WAIT_BITSET | FUTEX_PRIVATE_FLAG`) nằm ở phần Đi sâu của [Chương 03, mục 6](03-lock.md).

</details>

**Quay lại Q6:** futex là "so sánh rồi chặn" nguyên tử trên một biến atomic, nên không mất wake-up và bên báo hiệu bỏ qua syscall khi không ai chờ.

## 7. Khi nào *không* nên dùng lock-free (Q7)

> **Trả lời ngắn.** Thường là không nên (nhận xét của người soạn). Lock-free khó viết đúng, và không tự động nhanh hơn lock: CAS thất bại tốn kém[^atomics-cas-fail-cost]. Còn fast path của mutex futex chỉ là một CAS[^std-futex-fast]. Nó cũng không hứa wait-free[^std-not-waitfree].

**Giải thích.**

*Với service `ingest`.* Sau khi biết stack lock-free, ABA và hazard pointer, bạn có thể muốn viết lại hàng đợi job của `ingest` thành lock-free cho "nhanh". Mục này giải thích vì sao thường không nên. Tất cả các ý dưới đây là nhận xét của người soạn; nguồn chỉ được dẫn ở chỗ có.

1. **Khó đúng.** Bài báo obstruction-free viết rằng CAS và LL/SC là "an awkward match" với đồng bộ lock-free[^hlm-rare]. Ta vừa thấy lý do cụ thể ở §5: ABA, thu hồi bộ nhớ, và nhiều con trỏ phải nhất quán với nhau.
2. **Không tự động nhanh hơn.** CAS thất bại tốn kém[^atomics-cas-fail-cost]. Dưới tranh chấp cao, nhiều handler thread cùng CAS một `head` có thể lãng phí chu kỳ (suy luận của người soạn). Dưới tranh chấp thấp, mutex futex đã rẻ: fast path chỉ là một CAS, không syscall[^std-futex-fast].
3. **Lock-free không có nghĩa là không chờ.** Một thread vẫn có thể quay mãi trong CAS loop (§4), và `std` không hứa wait-free[^std-not-waitfree].
4. **Chi phí bảo trì.** Review, kiểm chứng và giải thích khó hơn `Mutex<T>` nhiều.
5. **Thực tế cho backend.** Hầu hết service như `ingest` nên dùng `Mutex`, `RwLock`, channel hoặc bộ đếm `fetch_add`. Khi thật sự cần cấu trúc lock-free, dùng crate đã được kiểm chứng như `crossbeam-epoch` (§5) thay vì tự viết phần thu hồi bộ nhớ, và đo trước khi chọn.

<details><summary>Đi sâu: số đo của Michael</summary>

Michael đo lock-free dùng hazard pointer và thấy hiệu năng ngang lock khi không tranh chấp[^michael-perf]. Đó là kết quả trên máy IBM năm 2004, không phải bảo đảm cho phần cứng hiện nay.

</details>

**Quay lại Q7:** lock-free là công cụ chuyên dụng, khó đúng và không mặc định nhanh hơn; chọn nó sau khi đo.

## 8. `Arc`: refcount atomic với ordering cụ thể (Q8)

> **Trả lời ngắn.** `Arc::clone` chỉ cần `Relaxed` vì tham chiếu mới chỉ sinh ra từ một tham chiếu đã có[^arc-clone-relaxed]. `drop` trừ refcount bằng `Release`[^arc-drop-sub]. Tham chiếu cuối cùng thêm fence `Acquire` để việc dùng dữ liệu không bị sắp xếp lại ra sau việc xóa[^arc-drop-fence].

**Giải thích.**

*Với service `ingest`.* `ingest` giữ cấu hình trong một `Arc<Config>`: đầu mỗi request, handler thread `clone` nó; cuối request thì `drop`. Mỗi request đụng refcount hai lần, nên ordering của hai thao tác này đáng để biết. Tham chiếu cuối cùng bị `drop` còn phải xóa `Config`, và lúc đó phải chắc rằng mọi thread khác đã dùng xong.

Docs `std` cho biết `Arc` dùng phép atomic cho refcount nên thread-safe, khác `Rc`[^std-arc]. Phần dưới đọc source `library/alloc/src/sync.rs` của Rust 1.95.0 để thấy ordering cụ thể (đây là chi tiết cài đặt, có thể đổi giữa các phiên bản).

### 8.1 `clone` = `fetch_add(1, Relaxed)`

Source ghi `let old_size = self.inner().strong.fetch_add(1, Relaxed);`[^arc-clone-fetchadd]. Dùng `Relaxed` được là vì tham chiếu mới chỉ sinh ra từ một tham chiếu đã có, và việc chuyển tham chiếu giữa các thread đã phải tự mang đồng bộ cần thiết; comment trong source giải thích đúng như vậy[^arc-clone-relaxed]. Với `ingest`, handler thread chỉ clone được `Config` khi nó đã nắm một `Arc` rồi.

Sau đó source kiểm tra `if old_size > MAX_REFCOUNT { abort(); }` (`MAX_REFCOUNT = isize::MAX`), phòng trường hợp ai đó `mem::forget` Arc liên tục[^arc-clone-abort]. Đây chính là cách "abort khi tràn" mà sách nhắc ở §1[^atomics-arc-abort].

### 8.2 `drop` = `fetch_sub(1, Release)`, rồi fence `Acquire` nếu là tham chiếu cuối

```rust
if self.inner().strong.fetch_sub(1, Release) != 1 { return; }
acquire!(self.inner().strong);   // atomic::fence(Acquire)
// ... drop dữ liệu, giải phóng bộ nhớ
```

Chưa phải tham chiếu cuối thì `drop` return ngay[^arc-drop-sub]. Với tham chiếu cuối, comment trong source giải thích: fence `Acquire` ngăn việc sắp xếp lại giữa *dùng* và *xóa* dữ liệu. Giảm refcount là `Release` nên nó đồng bộ với fence này, tức việc dùng dữ liệu xảy ra trước giảm refcount, trước fence, trước việc xóa[^arc-drop-fence]. Đây chính là mô hình Release/Acquire của [chương 06](06-memory-model.md). Áp vào `ingest`: mọi lần đọc `Config` của các handler thread xảy ra trước lúc `Config` bị xóa.

<details><summary>Đi sâu: macro <code>acquire!</code>, lý do với <code>Mutex&lt;T&gt;</code>, và CAS loop của <code>Weak</code></summary>

`acquire!` là `atomic::fence(Acquire)`, hoặc `load(Acquire)` khi build với ThreadSanitizer vì TSan không hỗ trợ fence[^arc-acquire-macro]. Cần đến mức này vì nội dung `Arc` có thể có ghi nội bộ (như `Mutex<T>`), và mutex không được lock lúc bị drop nên không thể dựa vào đồng bộ của nó[^arc-drop-mutex].

**`Weak::upgrade` và `Arc::downgrade` dùng CAS loop.** `upgrade` dùng `try_update(Acquire, Relaxed, checked_increment)` thay vì `fetch_add`, vì không bao giờ được đưa refcount từ 0 lên 1[^arc-upgrade-cas]. `downgrade` lặp `compare_exchange_weak(cur, cur + 1, Acquire, Relaxed)` và coi `usize::MAX` ở weak count như một "khóa" tạm[^arc-downgrade-cas]. Đây là mẫu của §2 trong `std`, với ràng buộc ("không đếm lên từ 0") mà `fetch_add` không diễn đạt được. ABA không đáng lo ở đây vì giá trị so sánh là bộ đếm, không phải identity của object (nhận xét của người soạn; sách cũng nói với bộ đếm thường thì không sao[^atomics-aba]).

</details>

**Quay lại Q8:** tạo thêm tham chiếu chỉ cần `Relaxed` vì đã có sẵn một tham chiếu; trả tham chiếu cuối phải `Release` + fence `Acquire` để mọi truy cập dữ liệu xảy ra trước khi xóa.

## Liên hệ với Rust

Với `ingest` viết bằng Rust, các thao tác ở chương này đều có sẵn trong `std::sync::atomic`; ba điểm dưới đây là chỗ bạn dễ vấp khi dùng chúng.

- `AtomicUsize` cùng kích thước và bit validity với `usize`, nhưng alignment luôn bằng kích thước, kể cả khi `usize` có alignment nhỏ hơn trên target đó[^std-au-align].
- `AtomicPtr<T>` có cùng kích thước và bit validity với `*mut T`[^std-ap]. Nó là viên gạch của stack/queue ở §5, và nó chỉ giữ **con trỏ**, không quản lý vòng đời. Vì vậy vấn đề ở §5 thuộc về bạn: docs `std` cảnh báo trực tiếp về ABA[^std-aba].
- Kiểu atomic là `Sync`, nhưng không tự cung cấp cơ chế chia sẻ[^std-atomic-sync]. Thường bạn đặt nó vào `Arc` hoặc `static`, như `REQUESTS_TOTAL` của `ingest`.

## Tóm tắt

- Atomic operation là viên gạch dưới mọi primitive đồng bộ[^atomics-building-block]. `fetch_add`/`swap` là một thao tác đọc-sửa-ghi, còn `load` rồi `store` thì không. `fetch_add` wrap khi tràn[^std-fetchadd-wrap].
- `compare_exchange_weak` được phép thất bại giả vì LL/SC có false negative[^atomics-llsc-fn]. Asm AArch64 không LSE cho thấy weak chỉ một lần `stlxr`, strong có vòng.
- x86-64 dùng `lock xadd`, `xchg`, `lock cmpxchg`. AArch64 dùng `ldxr/stxr`, hoặc từ ARMv8.1 là `ldadd/cas`[^atomics-lse].
- Wait-free mạnh hơn lock-free, lock-free mạnh hơn obstruction-free. `std` hứa lock-free[^std-lockfree]. `std` không hứa wait-free[^std-not-waitfree]. Nguồn đã tải không có định nghĩa hình thức.
- ABA: CAS thấy cùng giá trị nhưng lịch sử đã đổi[^atomics-aba]. Thu hồi bộ nhớ (hazard pointer, epoch) là phần khó; theo Michael, hazard pointer và GC thường ngăn ABA như tác dụng phụ[^michael-aba-side-effect].
- `Arc::clone` là `fetch_add(1, Relaxed)`[^arc-clone-fetchadd]. `drop` là `fetch_sub(1, Release)` rồi fence `Acquire` ở tham chiếu cuối[^arc-drop-sub].
- Futex là atomic compare-and-block[^futex-cab]. Fast path không syscall là lý do các primitive dựa trên futex nhanh[^atomics-futex-fast]. Lock-free không mặc định nhanh hơn lock.

## Câu hỏi tự kiểm tra

1. Vì sao `REQUESTS_TOTAL.store(REQUESTS_TOTAL.load(Relaxed) + 1, Relaxed)` của `ingest` có thể mất cập nhật? Đó có phải data race không?
<details><summary>Đáp án</summary>

Không phải data race, vì không có truy cập non-atomic[^std-datarace]. Đó là race condition: giữa load và store có thể có thread khác chen vào. `fetch_add` là một thao tác đọc-sửa-ghi duy nhất[^atomics-fetch].

</details>

2. `fetch_add(1, Relaxed)` trên `u32::MAX` làm gì? Cách nào ngăn hẳn việc tràn?
<details><summary>Đáp án</summary>

Nó wrap[^atomics-wrap]. Docs `std` cũng ghi vậy[^std-fetchadd-wrap]. Muốn ngăn hẳn thì dùng compare-and-exchange[^atomics-overflow-cae].

</details>

3. Vì sao `compare_exchange_weak` có thể thất bại dù giá trị khớp? Trên x86-64 có khác strong không?
<details><summary>Đáp án</summary>

LL/SC cho phép store-conditional thất bại giả[^atomics-llsc-fn]. Docs `std` cho phép như vậy để sinh code tốt hơn[^std-weak]. Trên x86-64 cả hai là `lock cmpxchg`[^atomics-x86-weak].

</details>

4. Khi nào chọn `compare_exchange` strong?
<details><summary>Đáp án</summary>

Khi không nằm trong vòng lặp và không muốn một thất bại giả bị hiểu nhầm, ví dụ khởi tạo khóa lười[^atomics-strong-when].

</details>

5. `fetch_add` trên `aarch64-apple-darwin` mặc định biên dịch thành gì, và khi tắt LSE?
<details><summary>Đáp án</summary>

`ldadd` khi có LSE[^atomics-lse]. Khi tắt LSE là vòng `ldaxr/add/stlxr/cbnz` (`AcqRel`; `ldxr/stxr` với `Relaxed`, asm ở §3), vì ARMv8 bản đầu không có lệnh đơn[^atomics-llsc].

</details>

6. Lock-free khác wait-free thế nào? `std` hứa gì?
<details><summary>Đáp án</summary>

Wait-free: mọi thread tiến triển[^hlm-waitfree]. Lock-free: luôn có thread tiến triển[^hlm-lockfree]. `std` hứa lock-free[^std-lockfree]. `std` không hứa wait-free[^std-not-waitfree].

</details>

7. Mô tả ABA trên `pop` của stack buffer ở `ingest` và hai hướng xử lý.
<details><summary>Đáp án</summary>

Thread 1 đọc head=A, next=B. Thread khác pop A, pop B, rồi push A. CAS của thread 1 vẫn thành công[^atomics-aba]. Docs `std` cảnh báo đúng tình huống này[^std-aba]. Hướng thứ nhất là tag/update counter, cần lệnh double-width[^michael-tag]. Hướng thứ hai là thu hồi bộ nhớ an toàn như hazard pointer[^michael-aba-side-effect].

</details>

8. Hazard pointer khác epoch ở mức ý tưởng?
<details><summary>Đáp án</summary>

Hazard pointer: thread công bố con trỏ đang dùng[^michael-hp-core]. Thread retire quét các con trỏ đó trước khi thu hồi[^michael-hp-scan]. Epoch: participant được pin[^ce-pin]. Rác gắn epoch và chỉ bị hủy khi đã đủ cũ[^ce-mechanism].

</details>

9. Vì sao `Arc::clone` dùng `Relaxed` còn `drop` dùng `Release` + fence `Acquire`?
<details><summary>Đáp án</summary>

Clone chỉ sinh từ tham chiếu đã có[^arc-clone-relaxed]. Còn drop phải bảo đảm mọi truy cập dữ liệu happens-before việc xóa ở thread khác[^arc-drop-fence].

</details>

10. `FUTEX_WAIT` nhận giá trị mong đợi để làm gì?
<details><summary>Đáp án</summary>

Để tránh lost wake-up: nếu giá trị đã đổi thì lời gọi thất bại ngay với `EAGAIN`[^futex-eagain]. Nhờ đó nó là một phép compare-and-block atomic[^futex-cab].

</details>

## Đọc thêm

- [Rust Atomics and Locks, ch.2](https://marabos.nl/atomics/atomics.html): compare-and-exchange, ví dụ ID allocation và lazy init.
- [Rust Atomics and Locks, ch.7](https://marabos.nl/atomics/hardware.html): assembly x86-64/ARM64 đầy đủ, LL/SC, cache coherence.
- [Rust Atomics and Locks, ch.8](https://marabos.nl/atomics/os-primitives.html): futex và các thao tác futex khác.
- [Michael, Hazard Pointers (2004)](https://www.cs.otago.ac.nz/cosc440/readings/hazard-pointers.pdf): thuật toán đầy đủ, queue, stack, benchmark.
- [Herlihy–Luchangco–Moir, Obstruction-Free Synchronization](https://www.cs.brown.edu/~mph/HerlihyLM03/main.pdf): vì sao đề xuất điều kiện yếu hơn lock-free.
- [crossbeam-epoch 0.9.18](https://docs.rs/crossbeam-epoch/0.9.18/crossbeam_epoch/): API `pin`, `Atomic`, `Shared`, `Guard`.
- [`library/alloc/src/sync.rs` (1.95.0)](https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/alloc/src/sync.rs): mã `Arc` với ordering thật.
- [Chương 06](06-memory-model.md), [Chương 08](08-rust-concurrency.md): ordering và cách Rust ghép các mảnh vào type system.

[^atomics-intro]: Rust Atomics and Locks, ch.2 Atomics — <https://marabos.nl/atomics/atomics.html> — "However, atomic operations do allow for different threads to safely read and modify the same variable."
[^atomics-building-block]: Rust Atomics and Locks, ch.2 Atomics — <https://marabos.nl/atomics/atomics.html> — "All the other concurrency primitives, such as mutexes and condition variables, are implemented using atomic operations."
[^atomics-types]: Rust Atomics and Locks, ch.2 Atomics — <https://marabos.nl/atomics/atomics.html> — "Each of the available atomic types has the same interface with methods for storing and loading, methods for atomic"
[^atomics-swap]: Rust Atomics and Locks, ch.2 Atomics — <https://marabos.nl/atomics/atomics.html> — "Instead of fetch_store, it has been called swap."
[^atomics-fetch]: Rust Atomics and Locks, ch.2 Atomics — <https://marabos.nl/atomics/atomics.html> — "also load (fetch) the original value, as a single atomic operation. The most commonly used ones are fetch_add and fetch_sub"
[^atomics-fetch-demo]: Rust Atomics and Locks, ch.2 Atomics — <https://marabos.nl/atomics/atomics.html> — "The fetch_add operation incremented a from 100 to 123, but returned to us the old value of 100."
[^atomics-wrap]: Rust Atomics and Locks, ch.2 Atomics — <https://marabos.nl/atomics/atomics.html> — "An important thing to keep in mind is that fetch_add and fetch_sub implement wrapping behavior for overflows."
[^std-fetchadd-wrap]: Rust std 1.95.0, `AtomicUsize::fetch_add` — <https://doc.rust-lang.org/1.95.0/std/sync/atomic/struct.AtomicUsize.html> — "This operation wraps around on overflow."
[^atomics-overflow-cae]: Rust Atomics and Locks, ch.2 Atomics — <https://marabos.nl/atomics/atomics.html> — "The third way of handling overflows is arguably the only truly correct one, as it prevents the addition from happening at all if it would overflow."
[^atomics-arc-abort]: Rust Atomics and Locks, ch.2 Atomics — <https://marabos.nl/atomics/atomics.html> — "This is, in fact, how the overflow check in Arc::clone() in the standard library is implemented,"
[^std-datarace]: Rust std 1.95.0, module `std::sync::atomic` — <https://doc.rust-lang.org/1.95.0/std/sync/atomic/index.html> — "The most important aspect of this model is that data races are undefined behavior. A data race is defined as conflicting non-synchronized accesses where at least one of the accesses is non-atomic."
[^atomics-cae]: Rust Atomics and Locks, ch.2 Atomics — <https://marabos.nl/atomics/atomics.html> — "This operation checks if the atomic value is equal to a given value, and only if that is the case does it replace it with a new value, all atomically as a single operation."
[^ostep-cas]: OSTEP, ch.28 Locks — <https://pages.cs.wisc.edu/~remzi/OSTEP/threads-locks.pdf> — "The basic idea is for compare-and-swap to test whether the value at the address specified by ptr is equal to expected; if so, update the memory location pointed to by ptr with the new value."
[^atomics-cas-loop]: Rust Atomics and Locks, ch.2 Atomics — <https://marabos.nl/atomics/atomics.html> — "The brief moment between loading and updating is so short"
[^atomics-fetchupdate]: Rust Atomics and Locks, ch.2 Atomics — <https://marabos.nl/atomics/atomics.html> — "The atomic types have a convenience method called fetch_update for the compare-and-exchange loop pattern."
[^std-fetchupdate-magic]: Rust std 1.95.0, `AtomicUsize::fetch_update` — <https://doc.rust-lang.org/1.95.0/std/sync/atomic/struct.AtomicUsize.html> — "This method is not magic; it is not provided by the hardware, and does not act like a critical section or mutex."
[^std-failure-order]: Rust std 1.95.0, `AtomicUsize::compare_exchange` — <https://doc.rust-lang.org/1.95.0/std/sync/atomic/struct.AtomicUsize.html> — "The failure ordering can only be SeqCst, Acquire or Relaxed."
[^atomics-weak]: Rust Atomics and Locks, ch.2 Atomics — <https://marabos.nl/atomics/atomics.html> — "the weak version may still sometimes leave the value untouched and return an Err, even though the atomic value matched the expected value."
[^std-weak]: Rust std 1.95.0, `AtomicUsize::compare_exchange_weak` — <https://doc.rust-lang.org/1.95.0/std/sync/atomic/struct.AtomicUsize.html> — "this function is allowed to spuriously fail even when the comparison succeeds, which can result in more efficient code on some platforms."
[^std-weak-loop]: Rust std 1.95.0, `AtomicUsize::compare_and_swap` (deprecated) — <https://doc.rust-lang.org/1.95.0/std/sync/atomic/struct.AtomicUsize.html> — "which allows the compiler to generate better assembly code when the compare and swap is used in a loop."
[^atomics-strong-when]: Rust Atomics and Locks, ch.2 Atomics — <https://marabos.nl/atomics/atomics.html> — "We don’t run our compare-and-exchange operation in a loop, and we don’t want to return zero if the operation spuriously fails."
[^atomics-add-micro]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "The add instruction will be split by the processor into several microinstructions behind the scenes, with separate steps for loading the value and storing the result."
[^atomics-lock-prefix]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "The lock prefix originally caused the processor to temporarily block all other cores from accessing memory for the duration of the instruction."
[^atomics-xchg]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "has an implicit lock prefix: it behaves like lock xchg regardless of the lock prefix."
[^atomics-xadd]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "Instead, another instruction can be used: xadd (\"exchange and add\"), which puts the originally loaded value into a register."
[^atomics-cmpxchg-loop]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "since this architecture does include a (lock-prefixable) cmpxchg (compare and exchange) instruction."
[^atomics-x86-weak]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "On x86-64, there is no difference between compare_exchange and compare_exchange_weak. Both compile down to a lock cmpxchg instruction."
[^atomics-x86-arm]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "On x86-64, every memory operation has acquire and release semantics, making it exactly as cheap or expensive as a relaxed operation."
[^atomics-llsc-def]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "it refuses to store to memory if any other thread has overwritten that memory since the load-linked instruction."
[^atomics-futex-basic]: Rust Atomics and Locks, ch.8 Operating System Primitives — <https://marabos.nl/atomics/os-primitives.html> — "On Linux, SYS_futex is a syscall that implements various operations that all operate on a 32-bit atomic integer. The main two operations are FUTEX_WAIT and FUTEX_WAKE."
[^atomics-llsc-fn]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "(2) the store-conditional is allowed to have false negatives, meaning that it may fail to store even though nothing has changed that particular piece of memory."
[^atomics-futex-kernel]: Rust Atomics and Locks, ch.8 Operating System Primitives — <https://marabos.nl/atomics/os-primitives.html> — "Instead, the kernel remembers which threads are waiting on which memory address to allow a wake operation to wake up the right threads."
[^michael-aba-almost]: Michael, Hazard Pointers: Safe Memory Reclamation for Lock-Free Objects, §2.3 — <https://www.cs.otago.ac.nz/cosc440/readings/hazard-pointers.pdf> — "It affects almost all lock-free algorithms."
[^atomics-llsc-coarse]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "Access to memory could be tracked not per byte, but per chunk of 64 bytes, or per kilobyte, or even the entire memory as a whole."
[^atomics-llsc]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "On ARM64, or at least in the first version of ARMv8, no atomic fetch-and-modify or compare-and-exchange operation can be represented by a single instruction."
[^atomics-llsc-few]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "Generally, the compiler will attempt to keep the number of instructions in an LL/SC pattern as small as possible,"
[^atomics-lse]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "When this instruction is used, there's no difference between compare_exchange and compare_exchange_weak, just like on x86-64."
[^atomics-lse-faster]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "these new instructions can be more performant, as they can be easier to optimize for with specialized hardware."
[^atomics-cas-fail-cost]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "A load operation can be significantly cheaper than a failed compare-and-exchange operation, in part because the latter often demands exclusive access to a cache line."
[^std-futex-spin-load]: rust-lang/rust tag 1.95.0, library/std/src/sys/sync/mutex/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/sync/mutex/futex.rs> — "// We only use `load` (and not `swap` or `compare_exchange`) while spinning, to be easier on the caches."
[^hlm-waitfree]: Luchangco, Moir, Herlihy, Obstruction-Free Synchronization: Double-Ended Queues as an Example, §1 — <https://www.cs.brown.edu/~mph/HerlihyLM03/main.pdf> — "A synchronization technique is wait-free if it ensures that every thread will continue to make progress in the face"
[^hlm-lockfree]: Luchangco, Moir, Herlihy, Obstruction-Free Synchronization: Double-Ended Queues as an Example, Abstract — <https://www.cs.brown.edu/~mph/HerlihyLM03/main.pdf> — "lock-free if it ensures only that some thread always makes progress."
[^michael-lockfree]: Michael, Hazard Pointers: Safe Memory Reclamation for Lock-Free Objects, §1 — <https://www.cs.otago.ac.nz/cosc440/readings/hazard-pointers.pdf> — "shared object is lock-free (also called nonblocking) if it guarantees that whenever a thread executes some finite number of steps toward an operation on the object,"
[^hlm-of]: Luchangco, Moir, Herlihy, Obstruction-Free Synchronization: Double-Ended Queues as an Example, §1 — <https://www.cs.brown.edu/~mph/HerlihyLM03/main.pdf> — "A synchronization technique is obstruction-free if it guarantees progress for any thread that eventually executes in isolation."
[^hlm-weaker]: Luchangco, Moir, Herlihy, Obstruction-Free Synchronization: Double-Ended Queues as an Example, §1 — <https://www.cs.brown.edu/~mph/HerlihyLM03/main.pdf> — "This property is weaker than lock-free synchronization, because it does not guarantee progress when"
[^michael-benefit]: Michael, Hazard Pointers: Safe Memory Reclamation for Lock-Free Objects, §1 — <https://www.cs.otago.ac.nz/cosc440/readings/hazard-pointers.pdf> — "Thus, unlike conventional lock-based objects, lock-free objects are immune to deadlock when faced with thread failures, and offer robust performance, even when faced with arbitrary thread delays."
[^hlm-nonblocking]: Luchangco, Moir, Herlihy, Obstruction-Free Synchronization: Double-Ended Queues as an Example, §1 — <https://www.cs.brown.edu/~mph/HerlihyLM03/main.pdf> — "obstruction-free synchronization ensures that no thread can be blocked by delays or failures of"
[^std-lockfree]: Rust std 1.95.0, module `std::sync::atomic` — <https://doc.rust-lang.org/1.95.0/std/sync/atomic/index.html> — "All atomic types in this module are guaranteed to be lock-free if they’re available. This means they don’t internally acquire a global mutex."
[^std-not-waitfree]: Rust std 1.95.0, module `std::sync::atomic` — <https://doc.rust-lang.org/1.95.0/std/sync/atomic/index.html> — "Atomic types and operations are not guaranteed to be wait-free. This means that operations like fetch_or may be implemented with a compare-and-swap loop."
[^atomics-aba]: Rust Atomics and Locks, ch.2 Atomics — <https://marabos.nl/atomics/atomics.html> — "If the atomic variable changes from some value A to B and then back to A after the load operation, but before the compare_exchange operation, it would still succeed, even though the atomic variable was changed (and changed back) in the meantime."
[^std-aba]: Rust std 1.95.0, `AtomicUsize::compare_exchange`, Considerations — <https://doc.rust-lang.org/1.95.0/std/sync/atomic/struct.AtomicUsize.html> — "a pointer holding the same address does not imply that the same object exists at that address! In this case, compare_exchange can lead to the ABA problem."
[^michael-aba]: Michael, Hazard Pointers: Safe Memory Reclamation for Lock-Free Objects, §2.3 — <https://www.cs.otago.ac.nz/cosc440/readings/hazard-pointers.pdf> — "It was first reported in the documentation of CAS on the IBM System 370"
[^michael-tag]: Michael, Hazard Pointers: Safe Memory Reclamation for Lock-Free Objects, §1 — <https://www.cs.otago.ac.nz/cosc440/readings/hazard-pointers.pdf> — "The IBM tag (update counter) method [11], which hinders memory reclamation for arbitrary reuse and requires double-width instructions that are not available on 64-bit processors."
[^michael-aba-side-effect]: Michael, Hazard Pointers: Safe Memory Reclamation for Lock-Free Objects, §2.3 — <https://www.cs.otago.ac.nz/cosc440/readings/hazard-pointers.pdf> — "the new methodology, often prevent the ABA problem as a side-effect with little or no additional overhead."
[^michael-gc-misconception]: Michael, Hazard Pointers: Safe Memory Reclamation for Lock-Free Objects, §2.3 — <https://www.cs.otago.ac.nz/cosc440/readings/hazard-pointers.pdf> — "a common misconception is that GC inherently prevents the ABA problem in all cases."
[^michael-reclaim-problem]: Michael, Hazard Pointers: Safe Memory Reclamation for Lock-Free Objects, §1 — <https://www.cs.otago.ac.nz/cosc440/readings/hazard-pointers.pdf> — "the contending thread might corrupt the object or some other object that happens to occupy the space of the freed node, return the wrong result, or suffer an access error by dereferencing an invalid pointer value."
[^michael-three-groups]: Michael, Hazard Pointers: Safe Memory Reclamation for Lock-Free Objects, §1 — <https://www.cs.otago.ac.nz/cosc440/readings/hazard-pointers.pdf> — "Without special scheduler support, these methods are blocking."
[^michael-hp-core]: Michael, Hazard Pointers: Safe Memory Reclamation for Lock-Free Objects, §1 — <https://www.cs.otago.ac.nz/cosc440/readings/hazard-pointers.pdf> — "Each hazard pointer can be written only by its owner thread, but can be read by other threads."
[^michael-hp-scan]: Michael, Hazard Pointers: Safe Memory Reclamation for Lock-Free Objects, §1 — <https://www.cs.otago.ac.nz/cosc440/readings/hazard-pointers.pdf> — "After accumulating some number R of retired nodes, the thread scans the hazard pointers of other threads for matches for the addresses of the accumulated nodes."
[^michael-hp-waitfree]: Michael, Hazard Pointers: Safe Memory Reclamation for Lock-Free Objects, §1 — <https://www.cs.otago.ac.nz/cosc440/readings/hazard-pointers.pdf> — "It uses only singleword reads and writes for memory access in its core operations."
[^michael-stack-hp]: Michael, Hazard Pointers: Safe Memory Reclamation for Lock-Free Objects, §4.2 — <https://www.cs.otago.ac.nz/cosc440/readings/hazard-pointers.pdf> — "In the pop routine, the node access in line 10 is hazardous and the CAS in line 11 is ABA-prone."
[^michael-stack]: Michael, Hazard Pointers: Safe Memory Reclamation for Lock-Free Objects, §4.2 — <https://www.cs.otago.ac.nz/cosc440/readings/hazard-pointers.pdf> — "Fig. 8 shows a lock-free stack based on the IBM freelist algorithm [11] augmented with hazard pointers."
[^michael-queue]: Michael, Hazard Pointers: Safe Memory Reclamation for Lock-Free Objects, §4.1 — <https://www.cs.otago.ac.nz/cosc440/readings/hazard-pointers.pdf> — "the algorithm represents the queue as a singly linked list with a dummy node at its head."
[^michael-helping]: Michael, Hazard Pointers: Safe Memory Reclamation for Lock-Free Objects, §4.1 — <https://www.cs.otago.ac.nz/cosc440/readings/hazard-pointers.pdf> — "any other thread can take the object to a stable state and then proceed with its own operation."
[^michael-queue-hazards]: Michael, Hazard Pointers: Safe Memory Reclamation for Lock-Free Objects, §4.1 — <https://www.cs.otago.ac.nz/cosc440/readings/hazard-pointers.pdf> — "The CAS in line 8 is both an access hazard and an ABA hazard."
[^atomics-false-sharing]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "This effect is called false sharing. We can avoid this by spacing the atomic variables further apart, so they each get their own cache line."
[^atomics-128]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "Depending on the type of processor you're trying this on, you might need to use 128-byte alignment to see the same effect."
[^ce-problem]: crossbeam-epoch 0.9.18, crate docs — <https://docs.rs/crossbeam-epoch/0.9.18/crossbeam_epoch/> — "The first thread must wait until the second thread stops reading the element. Only then it is safe to destruct it."
[^ce-gc]: crossbeam-epoch 0.9.18, crate docs — <https://docs.rs/crossbeam-epoch/0.9.18/crossbeam_epoch/> — "Programming languages that come with garbage collectors solve this problem trivially."
[^ce-mechanism]: crossbeam-epoch 0.9.18, crate docs — <https://docs.rs/crossbeam-epoch/0.9.18/crossbeam_epoch/> — "Every time a thread accesses a collection, it checks the current epoch, attempts to increment it, and destructs some garbage that became so old that no thread can be referencing it anymore."
[^ce-pin]: crossbeam-epoch 0.9.18, crate docs, Pinning — <https://docs.rs/crossbeam-epoch/0.9.18/crossbeam_epoch/> — "By pinning a participant we declare that any object that gets removed from now on must not be destructed just yet."
[^atomics-futex-linux]: Rust Atomics and Locks, ch.8 Operating System Primitives — <https://marabos.nl/atomics/os-primitives.html> — "On Linux systems, the pthread synchronization primitives are all implemented using the futex syscall."
[^atomics-futex-race]: Rust Atomics and Locks, ch.8 Operating System Primitives — <https://marabos.nl/atomics/os-primitives.html> — "meaning that no wake signal can get lost between the check of the expected value and the moment it actually goes to sleep."
[^futex-eagain]: futex(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/futex.2.en.html> — "If the futex value does not match val, then the call fails immediately with the error EAGAIN. The purpose of the comparison with the expected value is to prevent lost wake-ups."
[^futex-cab]: futex(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/futex.2.en.html> — "blocking via a futex is an atomic compare-and-block operation."
[^atomics-futex-spurious]: Rust Atomics and Locks, ch.8 Operating System Primitives — <https://marabos.nl/atomics/os-primitives.html> — "a futex wait operation can spuriously wake up, even when nothing happened."
[^atomics-futex-fast]: Rust Atomics and Locks, ch.8 Operating System Primitives — <https://marabos.nl/atomics/os-primitives.html> — "This is what makes futex-based synchronization primitives so fast:"
[^futex-uncontended]: futex(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/futex.2.en.html> — "In the uncontended case, a thread can access or modify the lock state with atomic instructions, for example atomically changing it from not acquired to acquired using an atomic compare-and-exchange instruction."
[^std-futex-fast]: rust-lang/rust tag 1.95.0, library/std/src/sys/sync/mutex/futex.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/sync/mutex/futex.rs> — "if self.futex.compare_exchange(UNLOCKED, LOCKED, Acquire, Relaxed).is_err() { self.lock_contended(); }"
[^atomics-cxx-wait]: Rust Atomics and Locks, ch.8 Operating System Primitives — <https://marabos.nl/atomics/os-primitives.html> — "In 2020, the C++ language even added support for basic futex-like operations to its standard library, with the addition of the atomic_wait and atomic_notify functions."
[^hlm-rare]: Luchangco, Moir, Herlihy, Obstruction-Free Synchronization: Double-Ended Queues as an Example, §1 — <https://www.cs.brown.edu/~mph/HerlihyLM03/main.pdf> — "The underlying problem is that conventional synchronization primitives such as CAS and LL/SC are an awkward match for lock-free synchronization."
[^michael-perf]: Michael, Hazard Pointers: Safe Memory Reclamation for Lock-Free Objects, Abstract — <https://www.cs.otago.ac.nz/cosc440/readings/hazard-pointers.pdf> — "offer comparable performance to that of efficient lock-based implementations under no contention and no multiprogramming,"
[^std-au-align]: Rust std 1.95.0, `AtomicUsize` — <https://doc.rust-lang.org/1.95.0/std/sync/atomic/struct.AtomicUsize.html> — "However, the alignment of this type is always equal to its size, even on targets where usize has a lesser alignment."
[^std-ap]: Rust std 1.95.0, `AtomicPtr` — <https://doc.rust-lang.org/1.95.0/std/sync/atomic/struct.AtomicPtr.html> — "This type has the same size and bit validity as a *mut T."
[^std-atomic-sync]: Rust std 1.95.0, module `std::sync::atomic` — <https://doc.rust-lang.org/1.95.0/std/sync/atomic/index.html> — "Atomic variables are safe to share between threads (they implement Sync) but they do not themselves provide the mechanism for sharing and follow the threading model of Rust."
[^std-arc]: Rust std 1.95.0, `std::sync::Arc` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Arc.html> — "Unlike Rc\<T>, Arc\<T> uses atomic operations for its reference counting. This means that it is thread-safe."
[^arc-clone-relaxed]: rust-lang/rust tag 1.95.0, library/alloc/src/sync.rs, `Arc::clone` — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/alloc/src/sync.rs> — "// Using a relaxed ordering is alright here, as knowledge of the // original reference prevents other threads from erroneously deleting // the object."
[^arc-clone-fetchadd]: rust-lang/rust tag 1.95.0, library/alloc/src/sync.rs, `Arc::clone` — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/alloc/src/sync.rs> — "let old_size = self.inner().strong.fetch_add(1, Relaxed);"
[^arc-clone-abort]: rust-lang/rust tag 1.95.0, library/alloc/src/sync.rs, `Arc::clone` — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/alloc/src/sync.rs> — "// However we need to guard against massive refcounts in case someone is `mem::forget`ing"
[^arc-drop-sub]: rust-lang/rust tag 1.95.0, library/alloc/src/sync.rs, `Arc::drop` — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/alloc/src/sync.rs> — "if self.inner().strong.fetch_sub(1, Release) != 1 {"
[^arc-drop-fence]: rust-lang/rust tag 1.95.0, library/alloc/src/sync.rs, `Arc::drop` — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/alloc/src/sync.rs> — "// This fence is needed to prevent reordering of use of the data and // deletion of the data."
[^arc-acquire-macro]: rust-lang/rust tag 1.95.0, library/alloc/src/sync.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/alloc/src/sync.rs> — "// ThreadSanitizer does not support memory fences. To avoid false positive // reports in Arc / Weak implementation use atomic loads for synchronization // instead."
[^arc-drop-mutex]: rust-lang/rust tag 1.95.0, library/alloc/src/sync.rs, `Arc::drop` — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/alloc/src/sync.rs> — "// possible to have interior writes to something like a Mutex\<T>. Since a // Mutex is not acquired when it is deleted, we can't rely on its"
[^arc-upgrade-cas]: rust-lang/rust tag 1.95.0, library/alloc/src/sync.rs, `Weak::upgrade` — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/alloc/src/sync.rs> — "// We use a CAS loop to increment the strong count instead of a // fetch_add as this function should never take the reference count // from zero to one."
[^arc-downgrade-cas]: rust-lang/rust tag 1.95.0, library/alloc/src/sync.rs, `Arc::downgrade` — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/alloc/src/sync.rs> — "match this.inner().weak.compare_exchange_weak(cur, cur + 1, Acquire, Relaxed) {"
[^hlm-lockfree-enough]: Luchangco, Moir, Herlihy, Obstruction-Free Synchronization: Double-Ended Queues as an Example, Abstract — <https://www.cs.brown.edu/~mph/HerlihyLM03/main.pdf> — "While wait-free synchronization is the ideal behavior (thread starvation is unacceptable), lock-free synchronization is often good enough for practical purposes"
[^atomics-lse-al]: Rust Atomics and Locks, ch.7 Understanding the Processor — <https://marabos.nl/atomics/hardware.html> — "one for combined release and acquire (-al) semantics."
