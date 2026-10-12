# P2 · Chương 07 — Rust và máy

## Bạn sẽ hiểu được gì

1. **Q1.** Làm sao nhìn thấy mã máy `rustc` sinh ra, và `opt-level` thay đổi gì? → mục 1
2. **Q2.** `#[inline]` thực sự hứa gì, và kiểm chứng một hàm đã inline thế nào? → mục 2
3. **Q3.** `v[i]` sinh lệnh gì? Khi nào bounds check biến mất, và `get_unchecked` đánh đổi gì? → mục 3
4. **Q4.** `target-cpu`, `target-feature`, `#[target_feature]`, `is_*_feature_detected!` khác nhau ra sao? → mục 4
5. **Q5.** `std::arch` và `asm!` cho phép gì, bạn phải tự giữ ràng buộc nào? → mục 5
6. **Q6.** `black_box` giải quyết gì trong benchmark, và *không* bảo đảm gì? → mục 6
7. **Q7.** Vì sao hai atomic nằm cạnh nhau có thể làm chương trình chậm hơn cả một bậc độ lớn (false sharing), và `#[repr(align)]`/`CachePadded` sửa thế nào? → mục 7

## Kiến thức cần trước

Bạn đã viết service bằng Rust và quen với `cargo build --release`, nhưng chưa phải nhìn xuống mã máy mà `rustc` sinh ra. Chương này lấp chỗ đó, bằng cách đọc asm và đo thử trên máy thật. Cần ôn thì xem:

- [Chương 03 — Assembly x86-64 và AArch64](03-assembly-x86-64-va-aarch64.md): đọc được vài lệnh cơ bản (`add`, `ldr`/`mov`, `cmp`, nhánh có điều kiện). Mục 7 của chương đó đã giới thiệu cách sinh asm bằng `--emit=asm`; mục 1 dưới đây chỉ nhắc lại ngắn gọn để thêm LLVM IR và `opt-level`.
- [Chương 04 — Calling convention và stack frame](04-calling-convention-va-stack-frame.md): tham số nằm ở thanh ghi nào, vì sao hàm có prologue/epilogue.
- [Chương 05 — Pipeline, branch prediction và speculative execution](05-pipeline-va-speculation.md) và [Chương 06 — Cache hierarchy và NUMA](06-cache-va-numa.md): cache line và coherence, cần cho mục false sharing. Mục 8 của chương 05 tóm tắt `std::hint`, `#[cold]`, `#[inline]`; chương này nhìn `black_box` và `#[inline]` từ góc "kiểm chứng bằng asm".
- Các giai đoạn trước LLVM: [P10 · Chương 01 — Pipeline của rustc](../10-rust-internals/01-pipeline-rustc.md). Layout kiểu dữ liệu: [P10 · Chương 02 — Type layout](../10-rust-internals/02-type-layout.md).

**Về máy thử nghiệm.** Mọi asm và số đo trong chương được chạy thật bằng `rustc 1.95.0` trên Apple M3 Pro, `aarch64-apple-darwin`. Ví dụ x86-64 sinh bằng cross-compile (`--target x86_64-unknown-linux-gnu`), chỉ xem asm, không chạy. Số đo là quan sát trên một máy, mỗi phép đo chạy vài lần (số lần ghi ở từng chỗ). Chúng minh họa hiện tượng, không phải con số để mang sang máy khác.

---

## 1. Từ mã Rust đến mã máy, và `opt-level` (Q1)

> **Trả lời ngắn.** `rustc` không tự sinh mã máy mà giao việc đó cho một backend, thường là LLVM[^llvm-backend]. Cờ `--emit=llvm-ir` cho bạn xem bản trung gian[^emit-ir]. Cờ `--emit=asm` cho xem assembly dạng chữ[^emit-asm]. Cờ `-C opt-level` chọn mức tối ưu[^opt-levels], và cùng một hàm ở mức `0` với mức `3` cho ra hai đoạn asm rất khác nhau (quan sát ở ví dụ dưới).

**Giải thích.**

*Với service `ingest`.* Từ chương này ta bám theo `ingest`, một service Rust nhận từng lô (batch) sự kiện qua mạng, kiểm tra và cộng dồn từng lô rồi đẩy vào hàng đợi. Nó chạy vài worker thread, mỗi worker giữ bộ đếm riêng, và được deploy lên một node pool Kubernetes. Mỗi mục sau mở đầu bằng một câu hỏi về `ingest`. (Nhận xét của người soạn: đây là ví dụ giả định, không phải code trong repo. Các hàm nhỏ trong chương như `add`, `get`, `popc` là những mảnh cắt ra từ loại hot path này.) Câu hỏi của mục này đơn giản nhất: khi `ingest` chạy chậm, CPU thực sự đang chạy những lệnh nào, và làm sao bạn đọc được chúng?

### 1.1 Ai sinh ra mã máy

Bước cuối của việc biên dịch là sinh mã máy, và `rustc` không tự làm bước này[^llvm-backend]. Nó đưa chương trình sang LLVM IR rồi để LLVM lo phần còn lại. LLVM IR về cơ bản là assembly có thêm kiểu dữ liệu mức thấp và chú thích. Các chú thích đó giúp tối ưu hóa cả trên IR lẫn trên mã máy xuất ra[^llvm-ir].

```mermaid
flowchart LR
    A[Mã Rust] --> B[HIR / MIR<br/>kiểm tra kiểu, borrow check]
    B --> C[LLVM IR<br/>--emit=llvm-ir]
    C --> D[Tối ưu hóa<br/>mức do -C opt-level]
    D --> E[Mã máy theo target<br/>--emit=asm]
    E --> F[Linker → file thực thi]
```

(Sơ đồ rút gọn. Các giai đoạn trước LLVM, như HIR và MIR, là tên nội bộ của `rustc` và do [P10](../10-rust-internals/01-pipeline-rustc.md) mô tả.)

Trên đường đó có hai điểm dừng đáng xem: LLVM IR, dễ đọc, và asm, gắn với một ISA cụ thể. `--emit=llvm-ir` sinh file chứa LLVM IR[^emit-ir]. `--emit=asm` sinh file assembly của crate[^emit-asm]. Muốn lấy nhiều loại cùng lúc thì liệt kê, cách nhau bằng dấu phẩy[^emit-list].

### 1.2 Đọc asm của một hàm

Lệnh dùng xuyên suốt chương:

```bash
rustc --crate-type=lib -C opt-level=3 --emit=asm -o out.s file.rs
```

Ta thử với hàm nhỏ nhất có thể, `fn add(a: u64, b: u64) -> u64 { a + b }` (quan sát). Sau tối ưu, IR chỉ còn `%_0 = add i64 %b, %a; ret i64 %_0`, và asm AArch64 ở `opt-level=3` chỉ còn `add x0, x1, x0; ret` (tham số ở `x0`, `x1`, kết quả ở `x0`, xem [Chương 04](04-calling-convention-va-stack-frame.md)). Ở `opt-level=0`, cùng hàm đó sinh khoảng 17 dòng (bỏ `.cfi_*`): `sub sp, sp, #32; stp x29, x30, [sp, #16]; add x8, x0, x1; str x8, [sp, #8]; subs x8, x8, x0; b.lo …` (nhánh tới `bl …panic_const_add_overflow`), rồi `ldr x0, [sp, #8]; … ret`. Ví dụ `i64` ở [Chương 03 mục 7](03-assembly-x86-64-va-aarch64.md) kiểm tra tràn bằng cờ tràn có dấu. Ở đây là `u64` nên đoạn kiểm tra trông khác (suy luận của người soạn: `subs` lấy tổng trừ `a`, và `b.lo` nhảy khi tổng nhỏ hơn `a`, tức đã tràn).

### 1.3 Ba khác biệt giữa `-O0` và `-O3`

Đặt hai bản asm cạnh nhau, ba điểm khác nhau hiện ra ngay:

1. **Biến sống trên stack.** Ở `-O0`, kết quả bị `str` ra stack rồi `ldr` lại. Ở `-O3`, nó nằm trong thanh ghi suốt.
2. **Kiểm tra tràn số.** `-O0` sinh `subs` cùng nhánh tới `panic_const_add_overflow`; `-O3` chỉ còn `add`. Lý do nằm ở tài liệu: `opt-level=0` bật luôn `cfg(debug_assertions)`[^opt-levels]. Debug assertions, nếu bạn không chỉ định, chỉ tự bật khi opt-level là 0[^debug-assertions]. Còn kiểm tra tràn số, nếu không chỉ định `-C overflow-checks`, bật theo debug assertions[^overflow-flag]. Chuyện tràn số debug/release được bàn kỹ hơn ở [Chương 02 mục 3](02-bieu-dien-du-lieu.md).
3. **Prologue/epilogue.** `-O0` lập frame (`stp x29, x30` …). `-O3` là *leaf function* (hàm không gọi hàm nào khác) nên không cần frame ([Chương 04 mục 6](04-calling-convention-va-stack-frame.md)).

Với `ingest`, hệ quả thực tế là build debug của nó vừa dùng stack nhiều hơn, vừa có thêm kiểm tra tràn mà bản release không có. Vì vậy số đo lấy từ build debug không nói gì về `ingest` đang chạy thật (nhận xét của người soạn).

<details><summary>Đi sâu: các giá trị <code>opt-level</code>, profile của Cargo, mẹo đặt tên symbol, và đo quy mô chênh lệch</summary>

- `-C opt-level` nhận các giá trị sau: `0` là không tối ưu và bật luôn `cfg(debug_assertions)`, `1` là cơ bản, `2` là một phần, `3` là toàn bộ, `s`/`z` tối ưu kích thước[^opt-levels]. `-O` là bí danh của `-C opt-level=3`, và giá trị mặc định khi không chỉ định là `0`[^opt-level].
- Cargo đặt giá trị khác với `rustc` trần. Profile `dev` (mặc định của `cargo build`) dùng `opt-level = 0`, `debug-assertions = true`, `overflow-checks = true`[^cargo-dev]. Profile `release` (khi có `--release`) dùng `opt-level = 3`, `debug-assertions = false`, `overflow-checks = false`[^cargo-release]. Bảng đầy đủ mọi khóa của hai profile (LTO, `codegen-units`, `strip`…) nằm ở [P9 · Chương 08, mục 7](../09-rust-language/08-module-crate-cargo.md).
- Mẹo (nhận xét của người soạn): đặt `#[no_mangle] pub fn` cho hàm cần xem để tên symbol đúng bằng tên hàm (trên macOS có thêm `_` đằng trước). Lưu ý attribute `inline` đặt trên chính hàm `no_mangle` bị bỏ qua (mục 2).
- Một phép đo thô cho thấy quy mô chênh lệch: tổng 2²⁴ phần tử `u32` trong một `Vec` (hàm `sum` dùng `for i in 0..v.len() { s = s.wrapping_add(v[i]) }`, mỗi mức biên dịch chạy 3 lần, `black_box` bọc đầu vào):

| `opt-level` | thời gian mỗi lần (quan sát) |
|---|---|
| 0 | ≈ 75 ms |
| 1 | ≈ 8,3–8,8 ms |
| 3 | ≈ 0,86–1,46 ms |

Quan sát: chênh cỡ 50–90 lần (≈ 75 ms so với 0,86–1,46 ms) cho vòng lặp đơn giản này. Chương trình thật thường chênh ít hơn (nhận xét của người soạn). **Không bao giờ đo hiệu năng trên build debug.**

</details>

**Quay lại Q1:** `rustc` đưa IR cho LLVM; `--emit=llvm-ir`/`--emit=asm` cho bạn xem kết quả, và `opt-level` quyết định asm là bản "nhiều stack, nhiều kiểm tra" (`0`) hay gọn trong thanh ghi (`3`). Với `ingest`, chỉ bản release mới đáng để đo và đọc.

## 2. Inlining (Q2)

> **Trả lời ngắn.** `#[inline]` chỉ là gợi ý để compiler đặt một bản sao thân hàm vào chỗ gọi thay vì sinh lời gọi[^inline-suggest]. Kể cả `#[inline(always)]` cũng chỉ là gợi ý, compiler được phép bỏ qua[^inline-hint]. Cách kiểm chứng trực tiếp (nhận xét của người soạn) là đọc asm của chỗ gọi và tìm lệnh `call`/`bl` tới hàm đó.

**Giải thích.**

*Với service `ingest`.* Với mỗi sự kiện trong batch, `ingest` gọi vài hàm rất nhỏ, chẳng hạn đọc độ dài một trường hay kiểm tra byte đầu của header. Hàm nhỏ mà gọi hàng triệu lần thì phần đáng kể không còn là thân hàm mà là chính lời gọi. Inline bỏ lời gọi đó, và còn cho compiler nhìn thân hàm cùng đoạn code quanh nó để gộp, bỏ bớt lệnh, tức tối ưu xuyên ranh giới hàm chứ không chỉ tiết kiệm `call`/`ret` (nhận xét của người soạn). Câu hỏi của mục này: bạn bảo được compiler inline đến đâu, và biết hàm đã inline thật chưa bằng cách nào?

### 2.1 `#[inline]` hứa gì

Attribute `inline` là một *gợi ý* về việc có nên đặt bản sao mã hàm vào caller hay không[^inline-suggest]. Nó có ba dạng: `#[inline]` gợi ý nên inline, `#[inline(always)]` gợi ý nên luôn inline, `#[inline(never)]` gợi ý không bao giờ inline[^inline-modes]. Ở cả ba dạng nó vẫn chỉ là hint, và compiler có thể bỏ qua[^inline-hint]. Nói cách khác, quyền quyết định cuối cùng thuộc về compiler. Bạn ghi `#[inline(always)]` lên một hàm trong `ingest` thì hàm đó chưa chắc đã được inline.

### 2.2 Kiểm chứng trong asm

Vì attribute không bảo đảm gì, thứ đáng tin là asm. Ví dụ sau có một hàm bị gợi ý đừng inline và một hàm bị gợi ý luôn inline:

```rust
#[inline(never)]  pub fn callee(x: u64)  -> u64 { x.wrapping_mul(3) + 1 }
#[inline(always)] pub fn callee2(x: u64) -> u64 { x.wrapping_mul(5) + 2 }
#[no_mangle] pub fn caller(x: u64) -> u64 { callee(x) + callee2(x) }
```

Asm AArch64 `opt-level=3` của `caller` vẫn còn `bl` tới `callee` (không inline). Còn thân `callee2` (`x*5 + 2`) nằm ngay trong `caller` dưới dạng `add x8, x19, x19, lsl #2` rồi `add x0, x8, #2` (quan sát). Muốn biết hàm đã inline chưa, đọc asm và tìm `call`/`bl`, đừng đọc attribute. ([Chương 05 mục 8](05-pipeline-va-speculation.md) tóm tắt thêm các gợi ý khác như `#[cold]`.)

<details><summary>Đi sâu: cảnh báo của Reference, ngoại lệ với hàm export, và hệ quả phụ của lời gọi còn lại</summary>

- Reference nhắc thêm: `rustc` tự inline khi thấy đáng, và quyết định inline tồi có thể làm chương trình chậm đi[^inline-careful].
- Ngoại lệ: attribute `inline` bị bỏ qua nếu hàm được export ra ngoài bằng `no_mangle` hoặc `export_name`[^inline-noexport]. Vì vậy mẹo `#[no_mangle]` ở mục 1 (để dễ tìm symbol trong asm) làm `#[inline]` trên chính hàm đó mất tác dụng. Trong ví dụ trên, `no_mangle` chỉ đặt ở `caller`, không đặt ở `callee`/`callee2`.
- Hệ quả phụ trong ví dụ (quan sát): vì còn `bl`, `caller` phải lập frame và cất `x0` vào thanh ghi callee-saved `x19` ([Chương 04 mục 3](04-calling-convention-va-stack-frame.md)).
- (Nhận xét của người soạn: đừng ép `#[inline(always)]` tràn lan trong `ingest`; chỉ làm khi đã đo và đã đọc asm.)

</details>

**Quay lại Q2:** `inline` là gợi ý, sự thật nằm trong asm: còn `bl`/`call` là chưa inline.

## 3. Bounds check và `get_unchecked` (Q3)

> **Trả lời ngắn.** `v[i]` trên slice sinh một phép so sánh và một nhánh trước khi nạp dữ liệu (quan sát asm bên dưới); nếu chỉ số vượt biên thì thread panic[^bounds-ref]. Compiler đôi khi gộp hoặc bỏ kiểm tra khi chứng minh được chỉ số hợp lệ. Đó là kết quả tối ưu chứ không phải quy tắc ngôn ngữ (nhận xét của người soạn). `get_unchecked` bỏ kiểm tra bằng cách chuyển trách nhiệm sang bạn, và nếu bạn sai thì là undefined behavior (UB)[^get-unchecked].

**Giải thích.**

*Với service `ingest`.* Hot path của `ingest` đọc `batch[i]` cho từng sự kiện, và mỗi lần đọc như vậy Rust kiểm tra chỉ số trước. Ba câu hỏi theo thứ tự: kiểm tra đó tốn gì ở mức lệnh máy, khi nào compiler xóa được nó, và có nên dùng `get_unchecked` để bỏ hẳn không?

### 3.1 Kiểm tra tốn gì

Với array và slice, ngôn ngữ kiểm tra lúc chạy và đưa thread vào trạng thái panic nếu kiểm tra thất bại[^bounds-ref]. Kiểu tự cài trait `Index` thì có hợp đồng riêng: `Index::index` có thể panic khi chỉ số vượt biên[^index-panic]. Với `&[u32]`, phần cứng làm việc này khá trực diện (quan sát). Hàm:

```rust
#[no_mangle] pub fn get(v: &[u32], i: usize) -> u32 { v[i] }
```

AArch64, `opt-level=3` (bỏ `.cfi_*` và các `.loh`):

```asm
_get:                              ; x0=ptr, x1=len, x2=i
    cmp   x2, x1
    b.hs  LBB1_2                   ; i >= len (không dấu) → panic
    ldr   w0, [x0, x2, lsl #2]     ; w0 = ptr[i]
    ret
LBB1_2:
    ...
    bl    __RNvNtCsl8K0bEFm1U0_4core9panicking18panic_bounds_check
```

Trên x86-64 (cross-compile, cú pháp AT&T, mặc định của asm do `rustc` in ra, xem [Chương 03 mục 1](03-assembly-x86-64-va-aarch64.md)) cũng vậy: `cmpq %rsi, %rdx; jae …; movl (%rdi,%rdx,4), %eax`. Cả hai kiến trúc đều là **một `cmp` cộng một nhánh** trước khi load, và nhánh panic nằm ngoài đường thường chạy. (Nhận xét của người soạn: nhánh luôn không nhảy thường được dự đoán tốt, xem [Chương 05 mục 3](05-pipeline-va-speculation.md). Chi phí đáng kể hơn là thêm lệnh và việc kiểm tra có thể chặn vector hóa.)

### 3.2 Khi nào kiểm tra biến mất

Compiler bỏ được bounds check không phải vì ngôn ngữ hứa vậy. Ngôn ngữ chỉ nêu kiểm tra lúc chạy, và kiểm tra lúc biên dịch khi chỉ số là hằng[^bounds-ref]. Phần còn lại là *kết quả tối ưu* mà ta phải xác nhận bằng asm (nhận xét của người soạn).

Hai ví dụ trái ngược nhau (quan sát). Hàm `sum_iter(v: &[u32])` chạy `for i in 0..v.len()`: asm `-O3` không chứa chữ `panic` nào (đã đếm: 0), vì `i < v.len()` chính là điều kiện vòng lặp (suy luận của người soạn). Ngược lại, khi chỉ số đến từ nơi compiler không biết gì, ví dụ mảng chỉ số ngẫu nhiên `idx: &[usize]`, compiler không có cơ sở chứng minh `idx[k] < len`, nên kiểm tra còn nguyên trong vòng lặp (suy luận của người soạn). Với `ingest`, bài học là duyệt batch theo `len` của chính nó thì thường không tốn kiểm tra, còn tra bảng bằng chỉ số lấy từ gói tin thì có.

### 3.3 `get_unchecked`: bỏ kiểm tra, nhận lấy rủi ro

`get_unchecked` là `unsafe` vì gọi nó với chỉ số ngoài biên là UB, kể cả khi tham chiếu kết quả không được dùng[^get-unchecked]. Asm của

```rust
#[no_mangle]
pub fn get_unchecked(v: &[u32], i: usize) -> u32 { unsafe { *v.get_unchecked(i) } }
```

trên AArch64 `-O3` chỉ còn `ldr w0, [x0, x2, lsl #2]; ret`; trên x86-64 là `movl (%rdi,%rdx,4), %eax; retq`.

<details><summary>Đi sâu: hoist bounds check ra khỏi vòng lặp, <code>llvm.assume</code>, và cái giá đo được</summary>

- **Hoist (đưa ra ngoài vòng lặp).** `sum_idx(v: &[u32], n: usize)` cộng `v[i]` với `i in 0..n`. Trong asm AArch64 `-O3`, lời gọi `panic_bounds_check` xuất hiện **đúng một chỗ**, chỉ một nhánh dẫn tới nó, ngay trước vòng lặp (`sub x8, x2, #1; cmp x1, x8; b.ls …`: nếu `n-1 >= len` thì panic). Thân vòng lặp được vector hóa NEON: mỗi vòng nạp 16 phần tử (`ldp q4, q5, …`, `ldp q6, q7, …`) rồi `add.4s`. Tức LLVM thay n lần kiểm tra bằng một lần. Quan sát này chỉ đúng cho đúng hàm này, `rustc 1.95.0`, target này.
- Trong LLVM IR của `get_unchecked`, thay cho nhánh, ta thấy `icmp ult i64 %i, %v.1` đưa vào `llvm.assume` (quan sát). Khớp với mã nguồn: với chỉ số `usize`, `get_unchecked` của slice gọi `intrinsics::assume(self < slice.len())`[^gu-assume]. Intrinsic đó báo cho optimizer rằng điều kiện luôn đúng, và nếu điều kiện sai thì hành vi là undefined[^assume-doc]. Đây là chi tiết cài đặt của 1.95.0, có thể đổi giữa các bản.
- Cái giá thực nghiệm: một benchmark tổng hợp 2²⁴ phần tử từ mảng 4096 phần tử theo chỉ số giả ngẫu nhiên (hàm `#[inline(never)]`, `black_box` bọc đầu vào, 3 lần chạy):

| biến thể | thời gian (quan sát) |
|---|---|
| `v[i]` | ≈ 5,1–5,5 ms |
| `get_unchecked(i)` | ≈ 2,8–2,9 ms |

Quan sát: unchecked nhanh khoảng 1,8–2,0 lần *trong vi benchmark này* (5,1–5,5 ÷ 2,8–2,9). Đừng ngoại suy sang code thật, nơi load bộ nhớ thường chi phối ([Chương 06](06-cache-va-numa.md)) và compiler thường đã loại bỏ kiểm tra. (Nhận xét của người soạn: vì UB là cái giá, hãy (1) viết lại bằng iterator/`chunks`/`zip` để compiler thấy bất biến, (2) đo, (3) chỉ khi đó cân nhắc `unsafe`.)

</details>

**Quay lại Q3:** `v[i]` là một `cmp` cộng một nhánh; compiler có thể hoist hoặc loại bỏ khi chứng minh được, còn `get_unchecked` bỏ hẳn kiểm tra và đổi lại UB nếu bạn sai. Với `ingest`: duyệt theo `len` trước, đo rồi mới tính chuyện `unsafe`.

## 4. `target-cpu`, `target-feature`, `#[target_feature]` và phát hiện lúc chạy (Q4)

> **Trả lời ngắn.** `-C target-cpu` và `-C target-feature` đổi loại CPU mà *cả* chương trình được biên dịch cho, nên binary chỉ chạy đúng trên máy có các tính năng đó[^arch-run-on]. Attribute `#[target_feature]` chỉ bật tính năng cho *một* hàm[^arch-attr]. Còn `is_*_feature_detected!` hỏi CPU thật lúc chạy để chọn đường đi[^arch-dyn].

**Giải thích.**

*Với service `ingest`.* Giả sử `ingest` được build trên máy CI đời mới, nhưng node pool Kubernetes chạy nó lại lẫn nhiều thế hệ CPU (nhận xét của người soạn: tình huống giả định). Build riêng cho CPU của máy CI thì hot path có thể nhanh hơn, nhưng node cũ có chạy nổi binary đó không? Mục này trả lời câu hỏi ấy và chỉ cách để một binary dùng lệnh mới mà vẫn chạy được trên máy cũ.

### 4.1 "Nền" CPU của một build

Mặc định, `rustc` sinh mã cho một CPU "nền" của target[^target-cpu]. Với `x86_64-unknown-linux-gnu`, nền này không có `popcnt` (quan sát asm ngay dưới đây). Hai cờ nâng nền lên:

- `-C target-cpu`: chỉ thị cho `rustc` sinh mã riêng cho một bộ xử lý cụ thể. Giá trị đặc biệt: `native` dùng bộ xử lý của máy đang build; `generic` là một LLVM target có tính năng tối thiểu nhưng tinh chỉnh hiện đại[^target-cpu].
- `-C target-feature`: bật `+x` hoặc tắt `-x` từng tính năng[^target-feature-syntax]. Tài liệu cảnh báo cờ này là unsafe và có thể gây hành vi không xác định lúc chạy[^target-feature-flag]. Mỗi cặp target/CPU có tập tính năng mặc định riêng[^default-features].

Ví dụ thấy được: `count_ones` trên x86-64.

```rust
#[no_mangle] pub fn popc(x: u64) -> u32 { x.count_ones() }
```

Biên dịch cross cho `x86_64-unknown-linux-gnu`, `opt-level=3`, cú pháp Intel (khác với các đoạn AT&T ở mục 3, xem [Chương 03 mục 1](03-assembly-x86-64-va-aarch64.md)):

- **Không có cờ** (quan sát): asm không dùng `popcnt` mà sinh thuật toán đếm bit bằng phép dịch, mask và nhân, 19 lệnh trước `ret` (`shr`, `and`, `sub`, `imul`, `shr rax, 56`…). Suy luận của người soạn: tập tính năng mặc định của target không gồm `popcnt`, nên compiler không được giả định nó.
- **Với `-C target-feature=+popcnt`:** chỉ còn `popcnt rax, rdi; ret`.

### 4.2 Cái giá khi deploy

Binary build với tính năng thêm *chỉ chạy được trên máy có tính năng đó*. Tài liệu `std::arch` nhấn mạnh phải bảo đảm chỉ chạy binary trên hệ thống thỏa tập tính năng yêu cầu[^arch-run-on]. Reference của `#[target_feature]` nói chặt hơn: gọi một hàm được biên dịch với tính năng mà nền tảng đang chạy không hỗ trợ là undefined behavior, trừ khi nền tảng ghi rõ là an toàn[^tf-ub]. Với `ingest`, nghĩa là build `target-cpu=native` trên máy CI mới rồi deploy lên node pool lẫn nhiều thế hệ CPU có thể làm process crash bằng lệnh không hợp lệ, và crash theo từng node (suy luận của người soạn).

### 4.3 Một binary, nhiều đường chạy

Vậy nếu chỉ hot path của `ingest` cần lệnh mới thì làm sao? Trang `std::arch` nêu hai hướng: phát hiện tĩnh (biên dịch có điều kiện bằng `cfg`) và phát hiện động (macro kiểm tra lúc chạy, kèm `#[target_feature]` để bật tính năng cho đúng một hàm). Tách ra, ta có ba mảnh ghép:

1. **Tĩnh bằng `cfg`.** Tính năng CPU tương ứng với `cfg` `target_feature`[^arch-cfg], dùng cùng `-C target-cpu` / `-C target-feature`. Chương trình chỉ chạy được trên máy có tính năng.
2. **Động.** `is_x86_feature_detected!` kiểm tra lúc chạy xem CPU đang chạy chương trình có hỗ trợ tính năng hay không[^arch-dyn]. Trên ARM64, macro tương đương là `is_aarch64_feature_detected!`, kiểm tra sự hiện diện của một tính năng CPU lúc chạy[^a64-detect].
3. **`#[target_feature(enable = "...")]` trên từng hàm.** Cờ biên dịch như `-C target-feature=+avx2` bật AVX2 cho toàn chương trình, còn attribute chỉ bật cho một hàm[^arch-attr].

Mẫu thường dùng có cùng dạng với ví dụ trong trang `std::arch`: một hàm `#[target_feature(enable = "avx2")] fn add4(a: __m256i, b: __m256i) -> __m256i { _mm256_add_epi64(a, b) }`, và một hàm `sum` gọi nó trong `unsafe` chỉ sau `if is_x86_feature_detected!("avx2") { … } else { /* vòng lặp thường */ }`. Hàm `#[target_feature]` an toàn chỉ được gọi an toàn từ caller đã bật mọi `target_feature` mà callee bật[^tf-safe]. Vì thế `add4` gọi từ `sum` phải nằm trong `unsafe`, và `is_x86_feature_detected!` ngay trước là điều biện minh cho khối `unsafe` đó.

<details><summary>Đi sâu: liệt kê CPU, AArch64 có "nền" khác, asm của mẫu dispatch, và lưu ý nền tảng</summary>

- Liệt kê CPU bằng `rustc --print target-cpus`[^target-cpu]; trên máy soạn, dòng đầu là `native - Select the CPU of the current host (currently apple-m3)` (quan sát). Xem cấu hình `cfg` của một target bằng `rustc --print cfg --target $TARGET`[^print-cfg]. Mỗi tính năng nền tảng có sẵn hiện thành một dòng `target_feature="..."`[^cfg-target-feature].
- Trên AArch64 (quan sát với target Apple), `count_ones` sinh `cnt.8b` và `addv.8b`, không cần cờ nào. Suy luận của người soạn: tập tính năng mặc định của target này đã có NEON. Hai ISA có "nền" khác nhau, nên cùng một dòng Rust ra mã khác nhau.
- Mẫu dispatch trên đã biên dịch cross cho x86-64 và xem asm; không chạy được trên máy ARM của người soạn. Asm chứa `vpaddq (%rsi), %ymm0, %ymm0` (AVX2, cú pháp AT&T) cùng một lần đọc biến cache `std_detect::detect::cache::CACHE` (gọi `detect_and_initialize` ở lần đầu). Tức kiểm tra lúc chạy là đọc một biến cache, không phải `cpuid` mỗi lần gọi (quan sát từ asm). Điều này khớp với mô tả trong mã nguồn rằng kết quả phát hiện được lưu đệm để chỉ phải tính một lần[^sd-cache]; đó là chi tiết cài đặt `std_detect`, có thể đổi giữa các bản.
- Macro phụ thuộc kiến trúc: `is_x86_feature_detected!` mang tính nền tảng riêng, giống module `arch`, và tài liệu nói gọi nó trên ARM là lỗi biên dịch[^arch-platform]. Trên máy ARM của người soạn, lời gọi `is_x86_feature_detected!("avx2")` cho lỗi `This macro cannot be used on the current target` (quan sát), nên cần `#[cfg(target_arch = ...)]`.
- Với ARM64: khi tính năng đã biết là bật lúc biên dịch (ví dụ qua `-Ctarget-feature`), macro mở rộng thành `true`[^a64-const]. Trang tài liệu của macro còn viết rằng hiện phần lớn tính năng chỉ được hỗ trợ trên nền tảng dựa trên Linux, và trên nền tảng khác kiểm tra lúc chạy luôn trả `false`[^a64-linux]. Nhưng mã nguồn `std_detect` ở 1.95.0 có backend riêng cho AArch64 của Apple: `mod.rs` chọn `os/darwin/aarch64.rs` khi `target_vendor = "apple"` và `target_arch = "aarch64"`[^sd-wiring]. File đó đọc tính năng bằng `sysctlbyname`[^sd-darwin]. Hai nguồn không khớp nhau (suy luận của người soạn: câu trong tài liệu macro chưa được cập nhật cho Apple; nhận xét của người soạn: với macOS, mã nguồn của đúng phiên bản đáng tin hơn lời văn tài liệu). Chương trình thử trên macOS in `neon=true sve=false crc=true lse=true`. Các `true` đó có thể đến từ tính năng đã bật lúc biên dịch **hoặc** từ phát hiện lúc chạy qua `sysctl`, nên kết quả một mình không chứng minh gì về detection động. Muốn phân biệt, in thêm `cfg!(target_feature = "crc")` hoặc xem `rustc --print cfg`[^print-cfg] (người soạn chưa làm; cũng chưa chạy trường hợp Linux/AArch64).
- Khi đối chiếu hai trang tài liệu: ví dụ trong trang `std::arch` viết rằng dùng `#[target_feature]` hiện đòi hàm phải `unsafe`[^arch-tf-old]. Reference 1.95.0 thì có quy tắc cho hàm `#[target_feature]` *an toàn*[^tf-safe]. Chương này theo Reference (nhận xét của người soạn: hai trang chưa được cập nhật đồng bộ).

</details>

**Quay lại Q4:** hai cờ đổi "nền" CPU cho cả build; attribute bật tính năng cho một hàm; macro `is_*_feature_detected!` kiểm tra lúc chạy. Ghép ba thứ lại, `ingest` có một binary dùng lệnh đặc biệt khi CPU có, và chạy đường thường khi CPU không có.

## 5. `std::arch` và `asm!` (Q5)

> **Trả lời ngắn.** `std::arch` đưa ra các hàm intrinsic, thường ứng với một lệnh máy[^arch-overview]. Chúng không portable giữa các kiến trúc[^arch-nonportable]. Về nguyên tắc chúng đều là `unsafe`[^arch-unsafe]. `asm!` cho nhúng assembly viết tay, nhưng compiler coi nó là hộp đen và chỉ tin phần khai báo của bạn[^asm-blackbox]. Vi phạm các quy tắc khai báo là UB[^asm-ub-rules].

**Giải thích.**

*Với service `ingest`.* Giả sử `ingest` tính checksum cho mỗi batch và bạn muốn dùng thẳng lệnh SIMD hay lệnh CRC của CPU, thứ mà compiler không tự sinh ra từ code Rust thường. Có hai cách: gọi hàm intrinsic của `std::arch`, hoặc tự viết assembly bằng `asm!`. Cả hai đều cho bạn quyền kiểm soát tối đa và đều bắt bạn giữ vài lời hứa với compiler. Mục này nói rõ đó là những lời hứa nào.

### 5.1 Intrinsic trong `std::arch`

`std::arch` là cổng vào các hàm intrinsic riêng cho từng kiến trúc, thường liên quan SIMD[^arch-gateway]. Nó không portable: có sẵn hay không phụ thuộc kiến trúc, và không phải máy nào của kiến trúc đó cũng có intrinsic[^arch-nonportable]. Tài liệu viết rằng mọi intrinsic trong module là unsafe và cần cẩn trọng khi gọi[^arch-unsafe]. Lý do là gọi intrinsic mà CPU thiếu tính năng là UB (điều kiện ở mục 4). Mỗi intrinsic thường ứng với một lệnh máy[^arch-overview]. Ví dụ chạy thật trên AArch64: tính tổng bằng NEON.

```rust
use std::arch::aarch64::*;

fn sum_neon(v: &[u32]) -> u32 {
    let mut chunks = v.chunks_exact(4);
    let mut acc = unsafe { vdupq_n_u32(0) };
    for c in &mut chunks {
        unsafe { acc = vaddq_u32(acc, vld1q_u32(c.as_ptr())); }
    }
    let mut s = unsafe { vaddvq_u32(acc) };
    for x in chunks.remainder() { s = s.wrapping_add(*x); }
    s
}
// sum_neon(&(1..=10).collect::<Vec<u32>>()) in ra 55
```

Chương trình in `55` (đã chạy). Nhưng mục 3 cho thấy vòng `for` đơn giản đã được vector hóa thành `add.4s` mà không cần intrinsic. (Nhận xét của người soạn: intrinsic đáng dùng khi compiler không tự vector hóa được, hoặc khi bạn cần một lệnh đặc biệt.) Ví dụ lệnh đặc biệt cho checksum của `ingest`: `core::arch::aarch64::__crc32w(x, y)` biên dịch thành đúng `crc32w w0, w0, w1; ret` (đã xem asm).

### 5.2 `asm!`: tự viết lệnh, tự giữ lời hứa

Khi `std::arch` chưa có intrinsic ổn định cho lệnh bạn cần thì dùng inline assembly. Chẳng hạn lệnh rào cản bộ nhớ `__dmb` của AArch64 ở 1.95.0 mới là API nightly[^dmb-nightly]. Không phải lệnh nào cũng cần `asm!`: x86-64 có sẵn `_rdtsc` để đọc time-stamp counter của CPU[^rdtsc]. Inline assembly được cung cấp qua các macro `asm!`, `naked_asm!`, `global_asm!`, và dùng để nhúng assembly viết tay vào output compiler sinh ra[^asm-intro]. Ví dụ chạy thật (AArch64):

```rust
use std::arch::asm;

fn add_asm(a: u64, b: u64) -> u64 {
    let r: u64;
    unsafe {
        asm!("add {r}, {a}, {b}",
             r = out(reg) r, a = in(reg) a, b = in(reg) b,
             options(pure, nomem, nostack));
    }
    r
}
```

`add_asm(40, 2)` in `42` (đã chạy). Asm sinh ra có `add x8, x0, x1` nằm giữa hai dòng `InlineAsm Start/End`, rồi `mov x0, x8; ret`. Compiler tự chọn thanh ghi cho `{r}`, `{a}`, `{b}` (quan sát).

Điều cần nhớ là compiler không được giả định các lệnh trong asm chính là thứ sẽ thực sự chạy. Nó phải coi asm là hộp đen và chỉ dựa vào phần khai báo giao diện[^asm-blackbox]. Nói cách khác, compiler *chỉ tin khai báo* (`in`, `out`, `options`) và không đọc chuỗi asm. Từ đó ra quy tắc quan trọng nhất: thanh ghi không khai là output phải có cùng giá trị khi thoát khỏi asm như lúc vào, nếu không là UB[^asm-regs]. Vì vậy thanh ghi nháp mà asm ghi đè phải được khai là output bị bỏ giá trị, ví dụ `out("x9") _`[^asm-out-underscore].

<details><summary>Đi sâu: <code>unsafe</code> của intrinsic trong 1.95.0, các <code>options</code>, <code>clobber_abi</code>, kiến trúc ổn định</summary>

- Trang `std::arch` viết mọi intrinsic là unsafe, nhưng trang tài liệu của từng hàm (`core::arch::aarch64`, 1.95.0) phân biệt hơn. `vdupq_n_u32` khai báo `pub fn` (không `unsafe`) kèm ghi chú chỉ có sẵn khi tính năng `neon` bật[^vdup-doc]. `__crc32w` cũng là `pub fn` với ghi chú tính năng `crc`[^crc32w-doc]. Còn `vld1q_u32`, nhận con trỏ thô, khai báo `pub unsafe fn`[^vld1-doc].
- Quan sát khi biên dịch bằng `rustc 1.95.0`: trên AArch64, gọi `vdupq_n_u32(3)` trong `fn main` bị từ chối với lỗi E0133 ("call to function … with `#[target_feature]` is unsafe"), kèm ghi chú rằng việc `neon` đã bật trong cấu hình build *không* thay thế việc khai báo `#[target_feature]`. Điều này đúng với ví dụ trong Reference: gọi hàm `#[target_feature]` từ nơi chưa khai báo tính năng vẫn là `unsafe`, dù tính năng đã bật mặc định hoặc bằng cờ biên dịch[^tf-flags-not-enough]. Ngược lại, trong một hàm *không* `unsafe` đánh dấu `#[target_feature(enable = "neon")]`, cùng lời gọi biên dịch được không cần `unsafe`; còn chính hàm đó, gọi từ `main`, lại đòi `unsafe`. Intrinsic nhận con trỏ thô (như `vld1q_u32`) có thêm lý do cần `unsafe` là con trỏ (suy luận của người soạn: `unsafe` ở đây biểu diễn lời hứa "CPU có tính năng này", khớp quy tắc của Reference ở mục 4). Vì vậy `unsafe` trong ví dụ `sum_neon` đúng với 1.95.0 chứ không thừa.
- Có thể dùng `clobber_abi` để áp sẵn một bộ clobber mặc định theo một calling convention, thay vì khai từng thanh ghi nháp[^asm-clobber-abi]. Unwind ra khỏi khối asm cũng là UB[^asm-unwind].
- Inline assembly ổn định cho x86/x86-64, ARM, AArch64, RISC-V và một số kiến trúc khác[^asm-stable].
- `options(pure)`: asm không có side effect, phải kết thúc, và output chỉ phụ thuộc vào input trực tiếp của nó (hoặc giá trị đọc từ bộ nhớ nếu không có `nomem`)[^asm-pure]. Nhờ vậy compiler được phép chạy nó ít lần hơn chương trình ghi, ví dụ đem ra khỏi vòng lặp, hoặc xóa hẳn nếu output không dùng. `pure` phải đi kèm `nomem` hoặc `readonly`, nếu không là lỗi biên dịch[^asm-pure-hoist].
- `options(nomem)`: asm không đọc/ghi bộ nhớ nào truy cập được từ ngoài asm; nhờ đó compiler có thể giữ giá trị biến toàn cục trong thanh ghi qua khối asm[^asm-nomem].
- `options(nostack)`: asm không push dữ liệu lên stack hoặc ghi vào red-zone của stack (nếu target hỗ trợ)[^asm-nostack].
- Khai báo sai làm hỏng giả định của compiler nên là UB, vì compiler chỉ tin khai báo. Reference minh họa đúng trường hợp `nomem` mà asm ghi bộ nhớ ngoài: đó là undefined behavior[^asm-nomem-ub].
- (Nhận xét của người soạn: ưu tiên intrinsic hơn `asm!` vì compiler hiểu ngữ nghĩa của chúng; `asm!` hầu như chỉ cần cho code hệ thống/embedded.)

</details>

**Quay lại Q5:** intrinsic là lệnh máy dưới dạng hàm Rust (bạn phải giữ lời hứa "CPU có tính năng này"); `asm!` cho tự do tối đa nhưng bạn phải khai báo trung thực vì compiler không đọc nội dung. Với checksum của `ingest`, thử intrinsic trước, `asm!` là phương án cuối.

## 6. `black_box` và đo hiệu năng đúng (Q6)

> **Trả lời ngắn.** Compiler có thể xóa hoặc tính trước thứ bạn định đo, khiến benchmark không còn phản ánh thực tế[^bb-why]. `std::hint::black_box` là hàm identity gợi ý compiler bi quan tối đa về nó[^bb-def]. Vì vậy nó hữu ích khi bạn không muốn một số tối ưu, như trong benchmark[^bb-bench]. Nó chỉ là nỗ lực tối đa (best-effort): chương trình không được dựa vào nó cho tính đúng đắn ngoài việc nó là identity[^bb-limits].

**Giải thích.**

*Với service `ingest`.* Bạn thêm vào CI một benchmark cho hàm tính toán của `ingest` để bắt ngay những commit làm nó chậm đi. Chạy lên, benchmark báo vài chục nano giây cho một khối việc mà bạn biết phải tốn cỡ mili giây. Con số đẹp đến mức đáng ngờ. Mục này giải thích vì sao, và `black_box` sửa được đến đâu.

### 6.1 Benchmark ngây thơ

Ta mô phỏng bằng một hàm tính tổng:

```rust
fn work(n: u64) -> u64 { (1..=n).sum() }
fn main() {
    let t = std::time::Instant::now();
    let mut r = 0;
    for _ in 0..1000 { r += work(1_000_000); }
    println!("r={r} {:?}", t.elapsed());
}
```

Biên dịch `-O` và chạy: chương trình in thời gian khoảng **42 ns** cho "1000 lần tính tổng 1 triệu số". Đây không phải phép đo thật. Asm của hàm bọc tương đương vòng ngoài chỉ còn bốn lệnh `mov`/`movk` dựng hằng 64-bit rồi `ret`: compiler đã tính trước kết quả cuối (500000500000000). Bản thân `work` cũng không lặp 10⁶ lần, vì asm của nó dùng `mul`/`umulh` để tính theo công thức cấp số cộng.

Chuyện này chính là điều tài liệu cảnh báo: compiler hoàn toàn có thể thực hiện những tối ưu làm benchmark sai lệch nặng[^bb-why]. Với `ingest`, benchmark kiểu này sẽ không bao giờ bắt được commit làm chậm hot path, vì thứ nó đo đã không còn tồn tại trong binary.

### 6.2 Bọc đầu vào và đầu ra

Cách chữa là bọc đầu vào và đầu ra bằng `black_box`: `r += black_box(work(black_box(1_000_000)))`. Tài liệu của `black_box` cũng khuyên đúng như vậy: khi đo một hàm, thường nên bọc đầu vào để compiler không thực hiện những tối ưu phi thực tế[^bb-wrap-inputs]. Compiler không thể giả định gì về giá trị đi qua khối đó, nên vòng 1000 lần không bị gộp. Thời gian đo tăng từ ≈ 42 ns lên ≈ 0,8–1,1 µs (hai lần chạy).

Con số mới vẫn **không** phải chi phí cộng 10⁶ số thật, vì `work` vẫn được rút gọn thành công thức. Bài học: `black_box` chặn một số tối ưu cụ thể chứ không biến phép đo thành "đúng", và bạn phải đọc asm để biết mình đang đo gì.

### 6.3 Danh sách kiểm cho benchmark của `ingest`

Danh sách này là nhận xét của người soạn:

- Dùng release build.
- `black_box` đầu vào và đầu ra, rồi mở asm kiểm tra thứ được đo còn đó.
- Lặp lại và nhìn độ ồn. Lần đầu thường chậm hơn: ở mục 1, `opt-level=3` đo 1,46 ms rồi 0,97 và 0,86 ms.
- Phân biệt dữ liệu vừa trong cache với dữ liệu lớn hơn cache ([Chương 06](06-cache-va-numa.md)).
- Đừng đem số của máy này sang máy khác.

<details><summary>Đi sâu: hai tác dụng của <code>black_box</code>, giới hạn, và asm của vòng ngoài</summary>

- Tài liệu mô tả `black_box` có hai tác dụng trong thực tế: chặn các tối ưu dựa trên giá trị nó trả về, và buộc giá trị được truyền vào phải được tính ra dù kết quả không dùng[^bb-two]. Vì vậy bọc *đầu vào* (để compiler không biết giá trị) và bọc *kết quả* (để lời gọi không bị xóa) là hai việc khác nhau. `r += black_box(work(black_box(1_000_000)))` làm cả hai (nhận xét của người soạn).
- Giới hạn: tài liệu viết rõ `black_box` không đem lại bảo đảm nào cho mục đích mật mã hay bảo mật, và cả ngôn ngữ Rust không có cơ chế nào cung cấp bảo đảm cần cho mật mã thời gian hằng (constant-time)[^bb-crypto]. Bảng trạng thái các hàm khác của `std::hint` nằm ở [Chương 05 mục 8](05-pipeline-va-speculation.md).
- Asm của vòng ngoài sau khi bọc: mỗi vòng có một `str` giá trị ra stack, một khối `InlineAsm` rỗng, rồi `ldr` lại (quan sát từ asm; chi tiết cài đặt, có thể đổi giữa các bản). Suy luận của người soạn: khối asm rỗng là cách `black_box` buộc compiler coi giá trị đi qua là không biết được.

</details>

**Quay lại Q6:** `black_box` giữ cho compiler không "biết trước" giá trị, nhưng không hứa gì ngoài việc là identity; đo xong vẫn phải đọc asm. Benchmark của `ingest` chỉ có giá trị khi thứ nó đo còn nằm trong asm.

## 7. False sharing, `#[repr(align)]` và `CachePadded` (Q7)

> **Trả lời ngắn.** Khi nhiều thread ghi vào các biến *khác nhau* nhưng nằm chung một cache line, mỗi lần ghi làm line đó mất hiệu lực ở các core khác. Các thread vì thế chậm đi dù không chia sẻ dữ liệu logic nào[^cp-why]. Hiện tượng này vẫn xuất hiện khi các thread dùng những vị trí nhớ khác nhau và tưởng là độc lập[^drep-fs]. Sửa bằng cách ép mỗi biến vào cache line riêng: `#[repr(align(N))]` hoặc kiểu `CachePadded` của crossbeam[^cp-def].

**Giải thích.**

*Với service `ingest`.* Mỗi worker thread của `ingest` đếm số sự kiện đã xử lý vào một `AtomicU64` của riêng mình, và bạn gom bốn bộ đếm đó vào một `Vec<AtomicU64>` cho gọn (nhận xét của người soạn: kiểu bộ đếm metrics per-worker này là ứng viên điển hình của false sharing). Mỗi worker chỉ ghi vào ô của nó, không worker nào đọc ô của worker khác, nên code đúng và không có race. Vậy mà thêm worker lại làm `ingest` chậm đi. Mục này giải thích vì sao, và sửa bằng cách nào.

### 7.1 Vì sao ghi vào ô riêng vẫn chậm

Chương 06 đã giải thích cơ chế ([mục 5](06-cache-va-numa.md) về coherence, [mục 6](06-cache-va-numa.md) về false sharing). Ở đây ta chỉ cần nhắc ý chính: khi nhiều core cùng ghi vào một cache line, coherence buộc line đó chuyền qua lại giữa các core. Dive into Systems mô tả cùng hiện tượng ở mức ví dụ: khi một thread cập nhật biến `g`, snoopy cache vô hiệu hóa không chỉ `g` mà cả cache line chứa `g`[^dis-fs]. Crossbeam nói tương tự: cập nhật một atomic làm mất hiệu lực cả cache line chứa nó, khiến lần truy cập kế tiếp vào cùng line chậm hơn với các core khác[^cp-why].

Với `ingest`: bốn `AtomicU64` liền nhau chỉ chiếm 32 byte, nên bốn bộ đếm của bốn worker nhiều khả năng nằm chung một line. Mỗi lần worker này tăng bộ đếm của mình là line bị vô hiệu hóa ở core của ba worker còn lại.

### 7.2 `repr(align)` đẩy mỗi biến sang line riêng

Modifier `align` và `packed` dùng để nâng hoặc hạ alignment của struct/union[^repr-align]. Nếu alignment chỉ định nhỏ hơn alignment tự nhiên của kiểu thì alignment không đổi[^repr-align-less]. Kích thước của một giá trị luôn là bội của alignment[^size-align]. Vì vậy `#[repr(align(128))] struct Padded(AtomicU64)` phải có `size_of` là bội của 128; với một trường 8 byte, bội nhỏ nhất là 128 (suy luận của người soạn từ quy tắc trên, khớp kết quả đo bên dưới).

### 7.3 Đo thử

Thí nghiệm đã chạy: 4 thread, mỗi thread gọi `fetch_add(1, Relaxed)` 20 triệu lần lên *atomic của riêng nó*. Hai biến thể chỉ khác bố cục:

```rust
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

#[repr(align(128))] struct Padded(AtomicU64);
struct Plain(AtomicU64);
// plain: Vec<Plain> (4 phần tử); pad: Vec<Padded> (4 phần tử)
```

Kết quả: `size Plain=8 align=8 Padded=128 align=128`. Thời gian (3 lần):

| bố cục | thời gian (quan sát) |
|---|---|
| `Plain` — 4 atomic liền nhau (32 byte tổng) | ≈ 494–503 ms (đợt 1), ≈ 524–597 ms (đợt 2) |
| `Padded` — mỗi atomic 128 byte | ≈ 38,7–40,0 ms (cả hai đợt) |

Quan sát: chênh ≈ 12–15 lần trên Apple M3 Pro (hai đợt, mỗi đợt 3 lần chạy; số tuyệt đối dao động giữa các lần nhưng tỉ lệ ổn định). `sysctl hw.cachelinesize` trên máy này báo 128 (quan sát), nên 4 atomic 8 byte liền nhau (khối 32 byte) nhiều khả năng chung một line. Khối đó chỉ vắt qua hai line nếu khối cấp phát bắt đầu sát cuối một line (suy luận của người soạn). Người soạn không khẳng định kích thước line trên máy khác.

Các thread *không chia sẻ dữ liệu logic nào*, vậy mà chậm hơn một bậc độ lớn. Logic đúng, chỉ bố cục bộ nhớ sai, và loại lỗi này rất khó thấy trong code review.

Con số này khác với cỡ 3 lần ở [Chương 06 mục 6](06-cache-va-numa.md) vì hai phép đo khác workload. Ở đó có 2 thread, mỗi thread `load` rồi `store` (không dùng `fetch_add`). Ở đây có 4 thread dùng `fetch_add`, là phép đọc-sửa-ghi nguyên tử. Số thread và loại phép toán đều có thể làm tranh chấp line nặng hơn (suy luận của người soạn, chưa đo tách từng yếu tố). Vì vậy đừng so hai con số trực tiếp; cả hai chỉ cho thấy cùng một chiều của hiện tượng.

### 7.4 `CachePadded`: không phải tự canh số

Thay vì tự canh số, crate `crossbeam-utils` (phiên bản 0.8.23) có `CachePadded<T>`: đệm và canh một giá trị theo độ dài cache line[^cp-def]. Với `ingest`, bạn đổi bộ đếm thành `Vec<CachePadded<AtomicU64>>` và tạo từng phần tử bằng `CachePadded::new(AtomicU64::new(0))`.

<details><summary>Đi sâu: layout được bảo đảm gì, <code>N</code> của <code>CachePadded</code>, thứ tự duyệt mảng</summary>

- Reference định nghĩa: giá trị có alignment n chỉ được lưu ở địa chỉ là bội của n[^align-def]. Kích thước thì luôn là bội của alignment[^size-align]. Một mảng `[T; N]` có kích thước `size_of::<T>() * N` và cùng alignment với `T`[^array-layout]. Nên các phần tử nằm liên tiếp trong bộ nhớ, và duyệt tuần tự thân thiện với cache và prefetcher (suy luận của người soạn; xem [Chương 06](06-cache-va-numa.md)).
- Với struct không có `repr` (biểu diễn Rust), bảo đảm layout duy nhất là những gì cần cho soundness; ngoài ra không có bảo đảm nào khác[^repr-rust]. Thứ tự field trong bộ nhớ không nhất thiết trùng thứ tự khai báo[^repr-order], nên muốn kiểm soát thì dùng `repr(C)` (xem [P10 · Chương 02](../10-rust-internals/02-type-layout.md)).
- Chọn 128 hay 64? `CachePadded` giả định kích thước line theo kiến trúc: trên x86-64, aarch64 và powerpc64, N = 128[^cp-n]. Lý do crate nêu cho CPU Intel hiện đại: prefetcher không gian kéo từng cặp line 64 byte một lúc, nên crate bi quan coi line dài 128 byte[^cp-why128]. Crate cảnh báo N chỉ là phỏng đoán hợp lý, không bảo đảm khớp độ dài cache line thật của máy đang chạy[^cp-guess]. Kích thước của `CachePadded<T>` là bội nhỏ nhất của N byte đủ chứa một giá trị `T`, và alignment của nó là max(N, alignment của `T`)[^cp-size].
- Đã biên dịch và chạy trong một project cargo tạm với `crossbeam-utils = "=0.8.23"`: `size_of::<CachePadded<AtomicU64>>()` và `align_of` đều in `128` trên `aarch64-apple-darwin`, khớp quy tắc "N = 128" và "bội nhỏ nhất của N" ở trên (kích thước này là mặc định theo kiến trúc của crate, không phải đo từ phần cứng). Cái giá là bộ nhớ: mỗi phần tử tốn ≥128 byte.
- Thứ tự duyệt (liên quan locality, [Chương 06 mục 2](06-cache-va-numa.md)): cộng 4096×4096 `u32` (64 MiB) row-major, `-O`: duyệt theo hàng ≈ 1,14–1,20 ms; theo cột (bước 16 KiB) ≈ 54–55 ms, chênh ≈ 45–48 lần (3 lần chạy). Chênh lệch gộp cả cache lẫn vector hóa (suy luận của người soạn — chưa đọc asm hai vòng này).

</details>

**Quay lại Q7:** false sharing là lỗi bố cục bộ nhớ chứ không phải lỗi logic. `#[repr(align)]` hoặc `CachePadded` đặt mỗi biến nóng vào cache line riêng, đổi lại tốn bộ nhớ. Với `ingest`: bộ đếm per-worker nên là `Vec<CachePadded<AtomicU64>>`.

## Liên hệ với Rust

Toàn chương là phần này, chia theo Q1–Q7 ở trên. Với `ingest`, thông điệp chung là: mã máy đến từ LLVM, còn Rust cho bạn cờ và attribute để điều khiển nó. Các cơ chế gần phần cứng (`std::arch`, `asm!`, `get_unchecked`, gọi hàm `#[target_feature]` từ nơi chưa bật tính năng đó) đều cần `unsafe`, vì mỗi cái kèm một lời hứa mà người gọi phải giữ.

Compiler *hứa* rất ít. Layout chỉ được bảo đảm ở mức soundness[^repr-rust]. `black_box` chỉ là identity[^bb-limits]. `inline` chỉ là gợi ý[^inline-hint]. Nhưng nó *làm* rất nhiều: inline, vector hóa, hoist bounds check. Phần "làm" đó bạn chỉ biết được bằng cách đọc asm đúng phiên bản và đúng target, nên mỗi lần sửa hot path của `ingest` hãy mở asm ra xem.

## Tóm tắt

- `rustc` sinh LLVM IR, LLVM sinh mã máy[^llvm-backend]. `--emit=asm` và `--emit=llvm-ir` cho xem cả hai.
- `opt-level=0` giữ biến trên stack và có kiểm tra tràn; `opt-level=3` dùng thanh ghi và vector hóa (quan sát). Không bao giờ đo bản debug.
- `#[inline]` chỉ là gợi ý, kể cả `inline(always)`[^inline-hint]. Kiểm tra bằng `call`/`bl` trong asm.
- `v[i]` là một `cmp` cộng một nhánh (quan sát). Compiler có thể hoist hoặc loại bỏ kiểm tra khi chứng minh được (tối ưu, không bảo đảm). `get_unchecked` chuyển trách nhiệm sang người gọi, sai là UB[^get-unchecked].
- `target-cpu`/`target-feature` mở khóa lệnh mới nhưng binary không chạy được trên CPU cũ[^arch-run-on]. Muốn một binary chạy mọi nơi, dùng `#[target_feature]` cùng `is_*_feature_detected!` để dispatch lúc chạy.
- `std::arch` là intrinsic, thường ứng với một lệnh máy và `unsafe`[^arch-overview]. `asm!` là hộp đen với compiler[^asm-blackbox]. `black_box` chỉ là identity "best-effort"[^bb-limits].
- False sharing là lỗi bố cục bộ nhớ, sửa bằng `#[repr(align)]`/`CachePadded`. Trên máy soạn, workload ở mục 7 chênh ≈ 13 lần.

## Câu hỏi tự kiểm tra

1. Hai cờ `rustc` nào cho phép xem LLVM IR và asm của một crate? Đầu ra mặc định được đặt tên thế nào?
<details><summary>Đáp án</summary>

Là `--emit=llvm-ir` và `--emit=asm`. `--emit=llvm-ir` ra file `CRATE_NAME.ll` theo mặc định[^emit-ir]. `--emit=asm` ra file `CRATE_NAME.s`[^emit-asm].

</details>

2. `#[inline(always)]` có bảo đảm hàm được inline không? Làm sao kiểm tra?
<details><summary>Đáp án</summary>

Không. Reference ghi rằng ở mọi dạng, attribute chỉ là hint và compiler có thể bỏ qua[^inline-hint]. Kiểm tra bằng cách đọc asm xem còn `call`/`bl` hay không (mục 2). Với hàm export bằng `no_mangle`, attribute còn bị bỏ qua hẳn[^inline-noexport].

</details>

3. Vì sao `sum_idx(v, n)` có một bounds check nhưng `sum_iter(v)` không có cái nào?
<details><summary>Đáp án</summary>

Quan sát asm: ở `sum_idx`, LLVM gom kiểm tra thành một phép so sánh `n-1` với `len` trước vòng lặp. Ở `sum_iter`, chỉ số bị chặn bởi chính `v.len()` nên kiểm tra thừa (suy luận từ asm). Đây là kết quả tối ưu, không phải bảo đảm ngôn ngữ (mục 3).

</details>

4. Vì sao `x.count_ones()` ra hơn chục lệnh trên `x86_64-unknown-linux-gnu` mặc định nhưng chỉ 1 lệnh `popcnt` khi bật `+popcnt`? Hệ quả khi deploy?
<details><summary>Đáp án</summary>

Mỗi target có tập tính năng mặc định riêng[^default-features], và asm quan sát được không dùng `popcnt` ở nền mặc định. Bật feature thì compiler được dùng lệnh đó, nhưng binary chỉ chạy đúng trên CPU có nó[^arch-run-on]. Chạy trên CPU thiếu tính năng là UB[^tf-ub] (mục 4).

</details>

5. Trên macOS/AArch64, `is_aarch64_feature_detected!("crc")` trả `true`. Có thể kết luận gì về phát hiện động?
<details><summary>Đáp án</summary>

Không kết luận được gì nếu chưa biết tính năng đó đã bật lúc biên dịch hay chưa: khi đã bật, macro mở rộng thành `true` mà không cần hỏi CPU[^a64-const]. Đừng dựa vào câu "nền tảng khác Linux luôn trả `false`" của trang tài liệu[^a64-linux]. Mã `std_detect` 1.95.0 có backend Apple AArch64[^sd-wiring]. Backend đó đọc tính năng bằng `sysctlbyname`[^sd-darwin], nên một `true` trên macOS có thể đến từ lúc biên dịch hoặc từ `sysctl` (mục 4).

</details>

6. Với `asm!`, compiler có đọc chuỗi lệnh của bạn để tối ưu không? Hệ quả của `options(nomem)` khi asm thực sự ghi bộ nhớ?
<details><summary>Đáp án</summary>

Không, compiler chỉ tin phần khai báo[^asm-blackbox]. `nomem` nghĩa là asm không đọc/ghi bộ nhớ ngoài[^asm-nomem]. Khai báo sai vi phạm hợp đồng và là undefined behavior[^asm-nomem-ub] (mục 5).

</details>

7. Vì sao benchmark ngây thơ báo 42 ns cho 1000 lần cộng 10⁶ số? `black_box` giải quyết được bao nhiêu?
<details><summary>Đáp án</summary>

Compiler tính trước toàn bộ ra hằng số và thay vòng lặp bằng công thức; tài liệu cũng nêu khả năng compiler tối ưu mất phần code cần đo[^bb-why]. `black_box` chặn việc tính trước vòng ngoài, nhưng `work` vẫn bị rút gọn thành công thức. Nó chỉ là identity nỗ lực tối đa nên phải kiểm bằng asm[^bb-limits] (mục 6).

</details>

8. Vì sao `size_of::<Padded>() == 128` với `#[repr(align(128))] struct Padded(AtomicU64)`?
<details><summary>Đáp án</summary>

`align` nâng alignment[^repr-align]. Kích thước luôn là bội của alignment[^size-align]. Bội nhỏ nhất của 128 đủ chứa 8 byte là 128 (suy luận của người soạn, khớp kết quả đo).

</details>

9. Vì sao `get_unchecked` là `unsafe`, và bản `usize` của nó báo gì cho optimizer?
<details><summary>Đáp án</summary>

Gọi với chỉ số ngoài biên là UB, kể cả khi không dùng tham chiếu kết quả[^get-unchecked]. Mã nguồn 1.95.0 dùng `intrinsics::assume(self < slice.len())`[^gu-assume] để báo optimizer điều kiện luôn đúng. Nếu điều kiện sai thì hành vi undefined[^assume-doc] (mục 3).

</details>

10. Hai thread ghi hai `AtomicU64` riêng nhưng nằm cùng cache line. Vì sao chậm dù không chia sẻ dữ liệu, và sửa thế nào?
<details><summary>Đáp án</summary>

Cập nhật một atomic làm mất hiệu lực cả cache line chứa nó, nên lần truy cập kế tiếp của core khác chậm hơn[^cp-why]. Sửa bằng cách đặt mỗi biến trên line riêng: `#[repr(align(N))]` hoặc `CachePadded`[^cp-def]. Đổi lại tốn bộ nhớ (mục 7).

</details>

11. `#[target_feature(enable = "avx2")]` trên một hàm khác `-C target-feature=+avx2` ở đâu? Vì sao gọi hàm đó thường vẫn cần `unsafe`?
<details><summary>Đáp án</summary>

Cờ bật AVX2 cho cả chương trình, còn attribute chỉ bật cho một hàm[^arch-attr]. Hàm `#[target_feature]` an toàn chỉ được gọi an toàn từ caller đã bật mọi `target_feature` mà nó bật[^tf-safe]. Nếu không thì phải `unsafe` và tự bảo đảm CPU có tính năng, thường bằng `is_x86_feature_detected!` ngay trước[^arch-dyn] (mục 4).

</details>

## Đọc thêm

- [rustc book — Codegen options](https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html): `opt-level`, `target-cpu`, `target-feature`, LTO, `codegen-units`.
- [Cargo Book — Profiles](https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html): giá trị mặc định của profile `dev` và `release`.
- [`std::arch`](https://doc.rust-lang.org/1.95.0/std/arch/index.html): phát hiện tính năng CPU tĩnh và động, kèm ví dụ dispatch đầy đủ.
- [Rust Reference — Inline assembly](https://doc.rust-lang.org/1.95.0/reference/inline-assembly.html): đầy đủ quy tắc, `options` và `clobber_abi`.
- [Rust Reference — Type layout](https://doc.rust-lang.org/1.95.0/reference/type-layout.html): size, alignment, `repr(align)`.
- [`std::hint::black_box`](https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html): mục "When is this useful?" và "How to use this".
- [crossbeam-utils 0.8.23 — `CachePadded`](https://docs.rs/crossbeam-utils/0.8.23/crossbeam_utils/struct.CachePadded.html): cách chọn kích thước padding.
- [Chương 03 — mục 7](03-assembly-x86-64-va-aarch64.md): cách sinh và đọc asm; [Chương 05 — mục 8](05-pipeline-va-speculation.md): `std::hint`, `#[cold]`; [Chương 06 — mục 6](06-cache-va-numa.md): false sharing từ góc coherence.
- [P10 · Chương 01](../10-rust-internals/01-pipeline-rustc.md): các giai đoạn trước LLVM; [P10 · Chương 02](../10-rust-internals/02-type-layout.md): layout và `repr`.

[^llvm-backend]: rustc-dev-guide, Code generation — <https://rustc-dev-guide.rust-lang.org/backend/codegen.html> — "Usually, rustc uses LLVM for code generation, but there is also support for Cranelift and GCC. The key is that rustc doesn’t implement codegen itself."
[^llvm-ir]: rustc-dev-guide, Code generation — <https://rustc-dev-guide.rust-lang.org/backend/codegen.html> — "LLVM takes input in the form of LLVM IR. It is basically assembly code with additional low-level types and annotations added. These annotations are helpful for doing optimizations on the LLVM IR and outputted machine code."
[^emit-ir]: rustc book, Command-line arguments, --emit — <https://doc.rust-lang.org/1.95.0/rustc/command-line-arguments.html> — "llvm-ir — Generates a file containing LLVM IR. The default output filename is CRATE_NAME.ll."
[^emit-asm]: rustc book, Command-line arguments, --emit — <https://doc.rust-lang.org/1.95.0/rustc/command-line-arguments.html> — "asm — Generates a file with the crate’s assembly code. The default output filename is CRATE_NAME.s."
[^emit-list]: rustc book, Command-line arguments, --emit — <https://doc.rust-lang.org/1.95.0/rustc/command-line-arguments.html> — "This flag controls the types of output files generated by the compiler. It accepts a comma-separated list of values, and may be specified multiple times."
[^opt-level]: rustc book, Codegen options, opt-level — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "Note: The -O flag is an alias for -C opt-level=3. The default is 0."
[^opt-levels]: rustc book, Codegen options, opt-level — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "This flag controls the optimization level. 0: no optimizations, also turns on cfg(debug_assertions) (the default). 1: basic optimizations. 2: some optimizations. 3: all optimizations. s: optimize for binary size. z: optimize for binary size, but more aggressively."
[^debug-assertions]: rustc book, Codegen options, debug-assertions — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "If not specified, debug assertions are automatically enabled only if the opt-level is 0."
[^overflow-flag]: rustc book, Codegen options, overflow-checks — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "If not specified, overflow checks are enabled if debug-assertions are enabled, disabled otherwise."
[^cargo-dev]: The Cargo Book, Profiles, dev — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "It is the default for build commands like cargo build, and is used for cargo install --debug. The default settings for the dev profile are: [profile.dev] opt-level = 0 debug = true split-debuginfo = '...' # Platform-specific. strip = "none" debug-assertions = true overflow-checks = true"
[^cargo-release]: The Cargo Book, Profiles, release — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "This profile is used when the --release flag is used, and is the default for cargo install. The default settings for the release profile are: [profile.release] opt-level = 3 debug = false split-debuginfo = '...' # Platform-specific. strip = "none" debug-assertions = false overflow-checks = false"
[^inline-suggest]: Rust Reference, Codegen attributes, inline — <https://doc.rust-lang.org/1.95.0/reference/attributes/codegen.html> — "The inline attribute suggests whether a copy of the attributed function’s code should be placed in the caller rather than generating a call to the function."
[^inline-modes]: Rust Reference, Codegen attributes, inline — <https://doc.rust-lang.org/1.95.0/reference/attributes/codegen.html> — "#[inline] suggests performing inline expansion. #[inline(always)] suggests that inline expansion should always be performed. #[inline(never)] suggests that inline expansion should never be performed."
[^inline-hint]: Rust Reference, Codegen attributes, inline — <https://doc.rust-lang.org/1.95.0/reference/attributes/codegen.html> — "In every form the attribute is a hint. The compiler may ignore it."
[^inline-careful]: Rust Reference, Codegen attributes, inline — <https://doc.rust-lang.org/1.95.0/reference/attributes/codegen.html> — "rustc automatically inlines functions when doing so seems worthwhile. Use this attribute carefully as poor decisions about what to inline can slow down programs."
[^inline-noexport]: Rust Reference, Codegen attributes, inline — <https://doc.rust-lang.org/1.95.0/reference/attributes/codegen.html> — "The inline attribute is ignored if the function is externally exported with no_mangle or export_name."
[^index-panic]: std::ops::Index — <https://doc.rust-lang.org/1.95.0/std/ops/trait.Index.html> — "May panic if the index is out of bounds."
[^bounds-ref]: Rust Reference, Array and slice indexing expressions — <https://doc.rust-lang.org/1.95.0/reference/expressions/array-expr.html> — "Array access is a constant expression, so bounds can be checked at compile-time with a constant index value. Otherwise a check will be performed at run-time that will put the thread in a panicked state if it fails."
[^get-unchecked]: primitive slice, get_unchecked — <https://doc.rust-lang.org/1.95.0/std/primitive.slice.html> — "Calling this method with an out-of-bounds index is undefined behavior even if the resulting reference is not used."
[^gu-assume]: rust-lang/rust tag 1.95.0, library/core/src/slice/index.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/core/src/slice/index.rs> — "Use intrinsics::assume instead of hint::assert_unchecked so that we don't check the precondition of this function twice. crate::intrinsics::assume(self < slice.len());"
[^assume-doc]: rust-lang/rust tag 1.95.0, library/core/src/intrinsics/mod.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/core/src/intrinsics/mod.rs> — "Informs the optimizer that a condition is always true. If the condition is false, the behavior is undefined."
[^target-cpu]: rustc book, Codegen options, target-cpu — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "This instructs rustc to generate code specifically for a particular processor. You can run rustc --print target-cpus to see the valid options to pass and the default target CPU for the current build target. Each target has a default base CPU. Special values include: native can be passed to use the processor of the host machine. generic refers to an LLVM target with minimal features but modern tuning."
[^target-feature-syntax]: rustc book, Codegen options, target-feature — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "Individual targets will support different features; this flag lets you control enabling or disabling a feature. Each feature should be prefixed with a + to enable it or - to disable it."
[^target-feature-flag]: rustc book, Codegen options, target-feature — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "Using this flag is unsafe and might result in undefined runtime behavior."
[^default-features]: rustc book, Codegen options, target-feature — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "Each target and target-cpu has a default set of enabled features."
[^print-cfg]: Rust Reference, Conditional compilation — <https://doc.rust-lang.org/1.95.0/reference/conditional-compilation.html> — "Configuration values for a specified target can be displayed with rustc --print cfg --target $TARGET."
[^cfg-target-feature]: Rust Reference, Conditional compilation, target_feature — <https://doc.rust-lang.org/1.95.0/reference/conditional-compilation.html> — "Key-value option set for each platform feature available for the current compilation target."
[^arch-run-on]: std::arch, Static CPU Feature Detection — <https://doc.rust-lang.org/1.95.0/std/arch/index.html> — "Note that when you compile a binary with a particular feature enabled it’s important to ensure that you only run the binary on systems which satisfy the required feature set."
[^tf-ub]: Rust Reference, target_feature attribute — <https://doc.rust-lang.org/1.95.0/reference/attributes/codegen.html> — "It is undefined behavior to call a function that is compiled with a feature that is not supported on the current platform the code is running on, except if the platform explicitly documents this to be safe."
[^arch-gateway]: std::arch, Module arch — <https://doc.rust-lang.org/1.95.0/std/arch/index.html> — "This module is intended to be the gateway to architecture-specific intrinsic functions, typically related to SIMD (but not always!)."
[^arch-overview]: std::arch, Overview — <https://doc.rust-lang.org/1.95.0/std/arch/index.html> — "This module exposes vendor-specific intrinsics that typically correspond to a single machine instruction."
[^arch-nonportable]: std::arch, Overview — <https://doc.rust-lang.org/1.95.0/std/arch/index.html> — "These intrinsics are not portable: their availability is architecture-dependent, and not all machines of that architecture might provide the intrinsic."
[^arch-unsafe]: std::arch, Overview — <https://doc.rust-lang.org/1.95.0/std/arch/index.html> — "As a result of the latter of these guarantees all intrinsics in this module are unsafe and extra care needs to be taken when calling them!"
[^arch-cfg]: std::arch, Static CPU Feature Detection — <https://doc.rust-lang.org/1.95.0/std/arch/index.html> — "CPU features correspond to the target_feature cfg available, and can be used like so:"
[^arch-dyn]: std::arch, Dynamic CPU Feature Detection — <https://doc.rust-lang.org/1.95.0/std/arch/index.html> — "this macro will perform necessary runtime detection to determine whether the CPU the program is running on supports the specified feature."
[^a64-detect]: std::arch::is_aarch64_feature_detected — <https://doc.rust-lang.org/1.95.0/std/arch/macro.is_aarch64_feature_detected.html> — "Check for the presence of a CPU feature at runtime."
[^arch-attr]: std::arch, Dynamic CPU Feature Detection — <https://doc.rust-lang.org/1.95.0/std/arch/index.html> — "Using a compiler flag like -C target-feature=+avx2 will enable AVX2 for the entire program, but using an attribute will only enable it for the one function."
[^tf-safe]: Rust Reference, target_feature attribute — <https://doc.rust-lang.org/1.95.0/reference/attributes/codegen.html> — "Safe #[target_feature] functions (and closures that inherit the attribute) can only be safely called within a caller that enables all the target_features that the callee enables."
[^tf-flags-not-enough]: Rust Reference, target_feature attribute (ví dụ) — <https://doc.rust-lang.org/1.95.0/reference/attributes/codegen.html> — "Calling foo_sse here is unsafe, as we must ensure that SSE is available first, even if sse is enabled by default on the target platform or manually enabled as compiler flags."
[^arch-tf-old]: std::arch, Dynamic CPU Feature Detection — <https://doc.rust-lang.org/1.95.0/std/arch/index.html> — "Usage of the #[target_feature] attribute currently requires the function to also be unsafe, as we see here."
[^arch-platform]: std::arch, Dynamic CPU Feature Detection — <https://doc.rust-lang.org/1.95.0/std/arch/index.html> — "Note that this macro, like the arch module, is platform-specific. For example calling is_x86_feature_detected!("avx2") on ARM will be a compile time error."
[^sd-cache]: rust-lang/rust tag 1.95.0, library/std_detect/src/detect/cache.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std_detect/src/detect/cache.rs> — "Caches run-time feature detection so that it only needs to be computed once."
[^a64-const]: std::arch::is_aarch64_feature_detected — <https://doc.rust-lang.org/1.95.0/std/arch/macro.is_aarch64_feature_detected.html> — "When the feature is known to be enabled at compile time (e.g. via -Ctarget-feature) the macro expands to true."
[^a64-linux]: std::arch::is_aarch64_feature_detected — <https://doc.rust-lang.org/1.95.0/std/arch/macro.is_aarch64_feature_detected.html> — "Currently most features are only supported on linux-based platforms: on other platforms the runtime check will always return false."
[^sd-wiring]: rust-lang/rust tag 1.95.0, library/std_detect/src/detect/mod.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std_detect/src/detect/mod.rs> — "all(target_vendor = "apple", target_arch = "aarch64") => { #[path = "os/darwin/aarch64.rs"] mod os; }"
[^sd-darwin]: rust-lang/rust tag 1.95.0, library/std_detect/src/detect/os/darwin/aarch64.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std_detect/src/detect/os/darwin/aarch64.rs> — "Try to read the features using sysctlbyname."
[^dmb-nightly]: core::arch::aarch64::__dmb — <https://doc.rust-lang.org/1.95.0/core/arch/aarch64/fn.__dmb.html> — "This is a nightly-only experimental API. (stdarch_arm_barrier #117219)"
[^rdtsc]: core::arch::x86_64::_rdtsc — <https://doc.rust-lang.org/1.95.0/core/arch/x86_64/fn._rdtsc.html> — "Reads the current value of the processor’s time-stamp counter."
[^vdup-doc]: core::arch::aarch64::vdupq_n_u32 — <https://doc.rust-lang.org/1.95.0/core/arch/aarch64/fn.vdupq_n_u32.html> — "pub fn vdupq_n_u32(value: u32) -> uint32x4_t Available on (AArch64 or target_arch=arm64ec) and target feature neon only."
[^crc32w-doc]: core::arch::aarch64::__crc32w — <https://doc.rust-lang.org/1.95.0/core/arch/aarch64/fn.__crc32w.html> — "pub fn __crc32w(crc: u32, data: u32) -> u32 Available on (AArch64 or target_arch=arm64ec) and target feature crc only."
[^vld1-doc]: core::arch::aarch64::vld1q_u32 — <https://doc.rust-lang.org/1.95.0/core/arch/aarch64/fn.vld1q_u32.html> — "pub unsafe fn vld1q_u32(ptr: *const u32) -> uint32x4_t"
[^asm-intro]: Rust Reference, Inline assembly — <https://doc.rust-lang.org/1.95.0/reference/inline-assembly.html> — "Support for inline assembly is provided via the asm!, naked_asm!, and global_asm! macros. It can be used to embed handwritten assembly in the assembly output generated by the compiler."
[^asm-stable]: Rust Reference, Inline assembly — <https://doc.rust-lang.org/1.95.0/reference/inline-assembly.html> — "Support for inline assembly is stable on the following architectures:"
[^asm-blackbox]: Rust Reference, Inline assembly rules — <https://doc.rust-lang.org/1.95.0/reference/inline-assembly.html> — "The compiler cannot assume that the instructions in the assembly code are the ones that will actually end up executed. This effectively means that the compiler must treat the assembly code as a black box and only take the interface specification into account, not the instructions themselves."
[^asm-ub-rules]: Rust Reference, Inline assembly rules — <https://doc.rust-lang.org/1.95.0/reference/inline-assembly.html> — "To avoid undefined behavior, these rules must be followed when using function-scope inline assembly (asm!):"
[^asm-pure]: Rust Reference, Inline assembly options — <https://doc.rust-lang.org/1.95.0/reference/inline-assembly.html> — "pure: The assembly code has no side effects, must eventually return, and its outputs depend only on its direct inputs (i.e. the values themselves, not what they point to) or values read from memory (unless the nomem options is also set)."
[^asm-pure-hoist]: Rust Reference, Inline assembly options — <https://doc.rust-lang.org/1.95.0/reference/inline-assembly.html> — "This allows the compiler to execute the assembly code fewer times than specified in the program (e.g. by hoisting it out of a loop) or even eliminate it entirely if the outputs are not used. The pure option must be combined with either the nomem or readonly options, otherwise a compile-time error is emitted."
[^asm-nomem]: Rust Reference, Inline assembly options — <https://doc.rust-lang.org/1.95.0/reference/inline-assembly.html> — "nomem: The assembly code does not read from or write to any memory accessible outside of the assembly code. This allows the compiler to cache the values of modified global variables in registers across execution of the assembly code since it knows that they are not read from or written to by it."
[^asm-nomem-ub]: Rust Reference, Inline assembly options (ví dụ `nomem`) — <https://doc.rust-lang.org/1.95.0/reference/inline-assembly.html> — "Writing to outside memory from assembly when nomem is specified is also undefined behaviour"
[^asm-nostack]: Rust Reference, Inline assembly options — <https://doc.rust-lang.org/1.95.0/reference/inline-assembly.html> — "nostack: The assembly code does not push data to the stack, or write to the stack red-zone (if supported by the target)."
[^asm-regs]: Rust Reference, Inline assembly rules — <https://doc.rust-lang.org/1.95.0/reference/inline-assembly.html> — "Any registers not specified as outputs must have the same value upon exiting the assembly code as they had on entry, otherwise behavior is undefined."
[^asm-out-underscore]: Rust Reference, Inline assembly, Operand type — <https://doc.rust-lang.org/1.95.0/reference/inline-assembly.html> — "An underscore (_) may be specified instead of an expression, which will cause the contents of the register to be discarded at the end of the assembly code (effectively acting as a clobber)."
[^asm-clobber-abi]: Rust Reference, Inline assembly, ABI clobbers — <https://doc.rust-lang.org/1.95.0/reference/inline-assembly.html> — "The clobber_abi keyword can be used to apply a default set of clobbers to the assembly code."
[^asm-unwind]: Rust Reference, Inline assembly rules — <https://doc.rust-lang.org/1.95.0/reference/inline-assembly.html> — "Behavior is undefined if execution unwinds out of the assembly code."
[^bb-def]: std::hint::black_box — <https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html> — "An identity function that hints to the compiler to be maximally pessimistic about what black_box could do."
[^bb-bench]: std::hint::black_box — <https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html> — "This property makes black_box useful for writing code in which certain optimizations are not desired, such as benchmarks."
[^bb-why]: std::hint::black_box — <https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html> — "It is not likely that all of the above happens, but the compiler is definitely able to make some optimizations that could result in a very inaccurate benchmark."
[^bb-wrap-inputs]: std::hint::black_box, How to use this — <https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html> — "When benchmarking a function, you usually want to wrap its inputs in black_box so the compiler cannot make optimizations that would be unrealistic in real-life use."
[^bb-two]: std::hint::black_box, How to use this — <https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html> — "In practice, black_box serves two purposes: It prevents the compiler from making optimizations related to the value returned by black_box It forces the value passed to black_box to be calculated, even if the return value of black_box is unused"
[^bb-crypto]: std::hint::black_box — <https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html> — "This also means that this function does not offer any guarantees for cryptographic or security purposes. This limitation is not specific to black_box; there is no mechanism in the entire Rust language that can provide the guarantees required for constant-time cryptography."
[^bb-limits]: std::hint::black_box — <https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html> — "Note however, that black_box is only (and can only be) provided on a “best-effort” basis. The extent to which it can block optimisations may vary depending upon the platform and code-gen backend used. Programs cannot rely on black_box for correctness, beyond it behaving as the identity function."
[^align-def]: Rust Reference, Type layout — <https://doc.rust-lang.org/1.95.0/reference/type-layout.html> — "A value of alignment n must only be stored at an address that is a multiple of n."
[^size-align]: Rust Reference, Type layout — <https://doc.rust-lang.org/1.95.0/reference/type-layout.html> — "The size of a value is always a multiple of its alignment."
[^array-layout]: Rust Reference, Type layout — <https://doc.rust-lang.org/1.95.0/reference/type-layout.html> — "An array of [T; N] has a size of size_of::\<T>() * N and the same alignment of T."
[^repr-rust]: Rust Reference, The Rust representation — <https://doc.rust-lang.org/1.95.0/reference/type-layout.html> — "There are no other guarantees of data layout made by this representation."
[^repr-order]: Rust Reference, The Rust representation — <https://doc.rust-lang.org/1.95.0/reference/type-layout.html> — "The ordering does not have to be the same as the order in which the fields are specified in the declaration of the type."
[^repr-align]: Rust Reference, The alignment modifiers — <https://doc.rust-lang.org/1.95.0/reference/type-layout.html> — "The align and packed modifiers can be used to respectively raise or lower the alignment of structs and unions."
[^repr-align-less]: Rust Reference, The alignment modifiers — <https://doc.rust-lang.org/1.95.0/reference/type-layout.html> — "For align, if the specified alignment is less than the alignment of the type without the align modifier, then the alignment is unaffected."
[^cp-why]: crossbeam-utils 0.8.23, CachePadded — <https://docs.rs/crossbeam-utils/0.8.23/crossbeam_utils/struct.CachePadded.html> — "Updating an atomic value invalidates the whole cache line it belongs to, which makes the next access to the same cache line slower for other CPU cores."
[^cp-def]: crossbeam-utils 0.8.23, CachePadded — <https://docs.rs/crossbeam-utils/0.8.23/crossbeam_utils/struct.CachePadded.html> — "Pads and aligns a value to the length of a cache line."
[^cp-n]: crossbeam-utils 0.8.23, CachePadded — <https://docs.rs/crossbeam-utils/0.8.23/crossbeam_utils/struct.CachePadded.html> — "On x86-64, aarch64, and powerpc64, N = 128."
[^cp-why128]: crossbeam-utils 0.8.23, CachePadded — <https://docs.rs/crossbeam-utils/0.8.23/crossbeam_utils/struct.CachePadded.html> — "On modern Intel architectures, spatial prefetcher is pulling pairs of 64-byte cache lines at a time, so we pessimistically assume that cache lines are 128 bytes long."
[^cp-guess]: crossbeam-utils 0.8.23, CachePadded — <https://docs.rs/crossbeam-utils/0.8.23/crossbeam_utils/struct.CachePadded.html> — "Note that N is just a reasonable guess and is not guaranteed to match the actual cache line length of the machine the program is running on."
[^cp-size]: crossbeam-utils 0.8.23, CachePadded — <https://docs.rs/crossbeam-utils/0.8.23/crossbeam_utils/struct.CachePadded.html> — "The size of CachePadded\<T> is the smallest multiple of N bytes large enough to accommodate a value of type T. The alignment of CachePadded\<T> is the maximum of N bytes and the alignment of T."
[^dis-fs]: Dive into Systems, 14.5.2 False Sharing — <https://diveintosystems.org/book/C14-SharedMemory/cache_coherence.html> — "the snoopy cache invalidates not only g, but the entire cache line that g is a part of."
[^drep-fs]: Drepper, What Every Programmer Should Know About Memory, §6.4.1 — <https://www.akkadia.org/drepper/cpumemory.pdf> — "The problem is also visible, though, when all the threads are using different memory locations and are supposedly independent."
