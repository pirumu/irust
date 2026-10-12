# Lab P2 — CPU: asm, calling convention, branch, cache, endianness

Đây là phần thực hành của [Phase P2: CPU và kiến trúc máy](../02-cpu/00-tong-quan.md). Lý thuyết nằm ở các chương `docs/02-cpu/`; mỗi bài lab dưới đây ghi rõ chương và mục cần đọc trước. Hãy đọc lý thuyết trước, rồi mới chạy lab.

> **Cách dùng tài liệu này (đọc kỹ).** Người soạn **không chạy** các lab này khi viết tài liệu, nên tài liệu **không có output hay số đo nào**. Mỗi bài có mục **Dự đoán (nếu lý thuyết đúng)**: đó là điều bạn *nên thấy* suy ra từ lý thuyết, mỗi ý kèm chú thích tới nguồn hoặc ghi rõ "(suy luận của người soạn)". Bạn chạy lab, ghi lại điều mình thấy, rồi đối chiếu. Chỗ khác dự đoán là phần có giá trị học nhất: hoặc bạn hiểu sai lý thuyết, hoặc dự đoán quá mạnh so với máy của bạn (nhiều điều phụ thuộc vi kiến trúc, compiler, OS). Quy ước chú thích: mọi sự thật có chú thích (dấu mũ trong ngoặc vuông, đúng định dạng của các chương) trỏ về nguồn và câu trích nguyên văn ở cuối file (chạy `python3 tools/verify_citations.py docs/labs/02-cpu-lab.md`). Câu ghi "(nhận xét của người soạn)" hoặc "(suy luận của người soạn)" là lời khuyên/suy luận, không phải sự thật có nguồn.

## 1. Chuẩn bị chung

Crate: `labs/crates/lab-cpu`, **không có dependency ngoài `std`**. Nó có một thư viện (`src/lib.rs`: các hàm `lab_*` làm đối tượng đọc assembly, cộng helper `best_of` lấy lần đo nhanh nhất và PRNG xorshift seed cố định) và năm binary trong `src/bin/`. Mọi binary chạy được trên macOS lẫn Linux; không bài nào cần quyền đặc biệt. Máy bạn là x86-64 hay AArch64 đều được — bài 1 và 2 chính là để so hai kiến trúc.

Với bài đo thời gian (3, 4, 5) **bắt buộc dùng `--release`**: ở build debug mọi thứ chạy ở `opt-level=0` và chương trình đo thứ khác hẳn thứ bạn định đo; Cargo ghi rõ release là profile tối ưu hoá, còn dev dùng cho phát triển thường ngày[^cargo-release]. Ba binary đo thời gian (`branch`, `stride`, `false_sharing`) in cảnh báo nếu bạn quên.

Kiểm tra compile (không chạy) của crate:

```bash
cd labs && cargo clippy -q -p lab-cpu --all-targets -- -D warnings
```

Hai cách lấy assembly dùng xuyên suốt bài 1 và 2:

```bash
# Cách A: rustc trực tiếp (chạy từ gốc repo). lib.rs chỉ dùng std nên biên dịch đứng một mình được.
rustc --edition 2024 --crate-type lib -C opt-level=3 --emit asm -o /tmp/lab_cpu_o3.s labs/crates/lab-cpu/src/lib.rs
rustc --edition 2024 --crate-type lib -C opt-level=0 --emit asm -o /tmp/lab_cpu_o0.s labs/crates/lab-cpu/src/lib.rs

# Cách B: qua cargo (đường dẫn file .s do cargo chọn, thường dưới labs/target/<profile>/deps/)
cd labs && cargo rustc -p lab-cpu --lib --release -- --emit asm
```

`--emit asm` tạo một file chứa assembly của crate[^emit]; `--crate-type` chọn loại crate cần dựng[^crate-type]; `--edition` chọn edition[^edition]. Với `cargo rustc`, phần sau `--` được chuyển cho lần gọi compiler cuối cùng chứ không phải các dependency[^cargo-rustc]. Lọc nhiễu: bỏ các dòng directive bắt đầu bằng `.`, ví dụ `grep -v '^[[:space:]]*\.' /tmp/lab_cpu_o3.s`; tìm một hàm bằng `grep -n -A30 -E '^_?lab_sum8:' file.s` (trên macOS symbol có thêm `_` đằng trước, trên Linux thì không — nhận xét của người soạn, theo mẹo ở [chương 07 mục 1](../02-cpu/07-rust-va-may.md)). Để xem kiến trúc còn lại, thêm `--target x86_64-unknown-linux-gnu` hoặc `--target aarch64-unknown-linux-gnu`: cờ `--target` chọn target tuple[^target]. Nếu rustc báo thiếu thư viện chuẩn cho target đó, cài bằng `rustup target add <triple>` (nhận xét của người soạn; chỉ phát asm, không liên kết, nên không cần linker của target).

Danh sách bài:

1. [Đọc assembly của hàm Rust: debug vs release, bounds check](#2-lab-1--đọc-assembly-của-hàm-rust)
2. [Calling convention và stack frame](#3-lab-2--calling-convention-và-stack-frame)
3. [Branch prediction: dữ liệu sắp xếp vs không](#4-lab-3--branch-prediction)
4. [Cache line và stride truy cập mảng](#5-lab-4--cache-line-và-stride)
5. [False sharing](#6-lab-5--false-sharing)
6. [Endianness và biểu diễn số](#7-lab-6--endianness-và-biểu-diễn-số)

## 2. Lab 1 — Đọc assembly của hàm Rust

**Mục tiêu.** Sinh asm từ code Rust của chính bạn và nhận ra ba "dấu vân tay": kiểm tra tràn số, kiểm tra biên chỉ số, và việc compiler loại bỏ chúng. Hiểu rằng asm là kết quả của LLVM ở một mức tối ưu cụ thể, không phải dịch từng dòng.

**Lý thuyết cần đọc trước.**

- [Chương 03 mục 7](../02-cpu/03-assembly-x86-64-va-aarch64.md) — sinh và đọc asm do `rustc` tạo; cách lọc nhiễu; debug vs release.
- [Chương 07 mục 1](../02-cpu/07-rust-va-may.md) — `opt-level` và pipeline từ Rust đến mã máy.
- [Chương 07 mục 3](../02-cpu/07-rust-va-may.md) — bounds check và `get_unchecked`.

**Mã.** `src/lib.rs`: `lab_add` (cộng `u64`), `lab_get` (`v[i]`), `lab_get_unchecked`, `lab_sum_iter` (`for i in 0..v.len()`), `lab_sum_idx` (`for i in 0..n` với `n` từ ngoài). Mỗi hàm có `#[unsafe(no_mangle)]` để tên symbol trong `.s` đúng bằng tên hàm.

**Lệnh chạy.**

```bash
# từ gốc repo, sinh hai bản asm như ở mục 1, rồi:
grep -n -A12 -E '^_?lab_add:'           /tmp/lab_cpu_o0.s
grep -n -A12 -E '^_?lab_add:'           /tmp/lab_cpu_o3.s
grep -n -A20 -E '^_?lab_get:'           /tmp/lab_cpu_o3.s
grep -n -A12 -E '^_?lab_get_unchecked:' /tmp/lab_cpu_o3.s
grep -c  'panic'                        /tmp/lab_cpu_o3.s        # đếm số chỗ nhắc panic trong cả file
grep -n -B2 -A40 -E '^_?lab_sum_iter:'  /tmp/lab_cpu_o3.s
grep -n -B2 -A60 -E '^_?lab_sum_idx:'   /tmp/lab_cpu_o3.s
# lặp lại với --target x86_64-unknown-linux-gnu và --target aarch64-unknown-linux-gnu
```

**Dự đoán (nếu lý thuyết đúng).**

1. **`lab_add`, `opt-level=0`.** Rust Reference nói các toán tử số nguyên panic khi tràn nếu biên dịch ở chế độ debug[^ovf]. Compiler quyết định có chèn kiểm tra hay không theo cờ: nếu không chỉ định, `overflow-checks` bật khi `debug-assertions` bật[^ovf-default], và `debug-assertions` tự bật khi `opt-level` là 0 nếu không chỉ định[^debug-assertions]. Vậy ở `opt-level=0` bạn kỳ vọng thấy, ngoài phép cộng, một lệnh nhảy có điều kiện dựa trên cờ tràn dẫn tới một hàm có chữ `panic` (và giá trị trung gian bị cất ra stack — [chương 07 mục 1](../02-cpu/07-rust-va-may.md) ghi lại quan sát này trên AArch64).
2. **`lab_add`, `opt-level=3`.** Không còn kiểm tra tràn (suy luận từ cùng hai cờ trên: `-C opt-level=3` không bật `debug-assertions`) nên thân hàm chỉ còn phép cộng rồi `ret`; hàm không gọi hàm nào nên không cần prologue (suy luận của người soạn, khớp [chương 07 mục 1](../02-cpu/07-rust-va-may.md)). Chỉ cần `-O` là đủ, vì `-O` là bí danh của `-C opt-level=3`[^opt].
3. **`lab_get`.** Chỉ mục trên slice kèm một kiểm tra lúc chạy: nếu thất bại thì thread vào trạng thái panic[^bounds]; `Index::index` có thể panic khi chỉ số vượt biên[^index-panic]. Kỳ vọng: một lệnh so sánh `i` với `len`, một nhánh có điều kiện tới đoạn gọi hàm panic cho chỉ số vượt biên, rồi mới đến lệnh load; đường chạy bình thường không nhảy (suy luận của người soạn; hình dạng cụ thể là kết quả của LLVM và khớp quan sát ở [chương 07 mục 3](../02-cpu/07-rust-va-may.md) cho một phiên bản rustc).
4. **`lab_get_unchecked`.** Gọi với chỉ số ngoài biên là undefined behavior, kể cả khi tham chiếu kết quả không được dùng[^get-unchecked]; vì vậy compiler không cần (và không nên) chèn kiểm tra: kỳ vọng chỉ còn lệnh load (suy luận của người soạn).
5. **`lab_sum_iter`.** Điều kiện vòng lặp `i < v.len()` chính là điều kiện bounds check, nên compiler có thể chứng minh chỉ số hợp lệ và bỏ kiểm tra; kỳ vọng không có lời gọi panic trong hàm này. Đây là *kết quả tối ưu*, không phải quy tắc ngôn ngữ (nhận xét của người soạn, theo [chương 07 mục 3](../02-cpu/07-rust-va-may.md)): nếu rustc/LLVM của bạn khác phiên bản, bạn có thể thấy khác — ghi lại.
6. **`lab_sum_idx`.** `n` đến từ ngoài nên compiler không biết `n <= len`: kỳ vọng ít nhất một lời gọi hàm panic còn trong hàm, có thể được đưa ra trước vòng lặp thay vì lặp mỗi vòng (suy luận của người soạn).
7. **x86-64 so với AArch64.** Cùng một hàm, hai ISA sinh hai chuỗi lệnh khác nhau về tên lệnh, thanh ghi và cú pháp nhưng cùng cấu trúc logic (cùng nhánh, cùng so sánh) — xem [chương 01 mục 6](../02-cpu/01-mo-hinh-may-va-isa.md) và [chương 03 mục 1–2](../02-cpu/03-assembly-x86-64-va-aarch64.md) để đọc cú pháp.

**Câu hỏi tự đối chiếu.**

1. Trong asm `opt-level=0` của `lab_add`, tìm lệnh đặt cờ tràn và lệnh nhảy theo cờ đó. Chúng biến mất ở `opt-level=3`: cờ nào của `rustc` điều khiển việc này, và bạn sẽ sinh một bản `opt-level=3` *có* kiểm tra tràn bằng cách nào?
<details><summary>Đáp án</summary>

Cờ `-C overflow-checks`[^ovfflags]; mặc định nó đi theo `debug-assertions`[^ovf-default]. Thêm `-C overflow-checks=yes` vào lệnh `opt-level=3` (cờ nhận `y, yes, on, true` để bật[^ovf-default]) thì kiểm tra trở lại (suy luận từ tài liệu cờ). Bạn kiểm chứng bằng cách chạy và so asm.

</details>

2. `lab_get` ở mức tối ưu cao có cần nhánh panic không, nếu bạn gọi nó từ `main` với chỉ số hằng nhỏ hơn độ dài mảng hằng? Điều đó nói gì về "bounds check luôn tốn một nhánh"?
<details><summary>Đáp án</summary>

Có thể không: khi hàm được inline và compiler chứng minh được chỉ số hợp lệ thì kiểm tra có thể biến mất — đó là kết quả tối ưu chứ không phải quy tắc ngôn ngữ (nhận xét của người soạn, theo [chương 07 mục 3](../02-cpu/07-rust-va-may.md)); ngôn ngữ chỉ bảo đảm hành vi "panic nếu vượt biên"[^bounds]. Để kiểm chứng, viết `fn main` gọi `lab_get(&[1,2,3], 1)` rồi sinh asm.

</details>

3. Vì sao `lab_get_unchecked` là `unsafe` trong khi asm của nó "chỉ ngắn hơn"?
<details><summary>Đáp án</summary>

Vì tài liệu quy định chỉ số ngoài biên là undefined behavior[^get-unchecked]; việc bỏ kiểm tra chuyển trách nhiệm chứng minh `i < len` sang người gọi.

</details>

## 3. Lab 2 — Calling convention và stack frame

**Mục tiêu.** Nhìn thấy bằng mắt các quy tắc ABI: tham số vào thanh ghi nào, khi nào tràn lên stack, giá trị trả về ở đâu, hàm nào cần prologue/epilogue, và đo khoảng cách giữa các frame đệ quy để đối chiếu với `sub sp`/`push` trong asm.

**Lý thuyết cần đọc trước.**

- [Chương 04 mục 2](../02-cpu/04-calling-convention-va-stack-frame.md) — System V AMD64: tham số và giá trị trả về.
- [Chương 04 mục 3](../02-cpu/04-calling-convention-va-stack-frame.md) — caller-saved và callee-saved.
- [Chương 04 mục 4](../02-cpu/04-calling-convention-va-stack-frame.md) — căn lề stack 16 byte và red zone.
- [Chương 04 mục 5](../02-cpu/04-calling-convention-va-stack-frame.md) — AAPCS64.
- [Chương 04 mục 6](../02-cpu/04-calling-convention-va-stack-frame.md) — prologue, epilogue, frame pointer.

**Mã.**

- `src/lib.rs` (`extern "C"`, nên theo ABI của nền tảng chứ không phải ABI Rust): `lab_sum8` (8 tham số `u64`), `lab_mk2` (trả `struct` 16 byte), `lab_mk` (trả `struct` 24 byte), `lab_two` (giá trị phải sống qua hai lời gọi `lab_ext`), `lab_leaf_local` (hàm lá có mảng cục bộ).
- `src/bin/frames.rs`: hàm đệ quy `descend` ghi địa chỉ một biến cục bộ ở mỗi tầng rồi in khoảng cách giữa các tầng và phần dư khi chia cho 16.

**Lệnh chạy.**

```bash
# asm (bản -C panic=abort cho bớt nhiễu, theo cách sinh asm ở chương 04):
rustc --edition 2024 --crate-type lib -C opt-level=2 -C panic=abort --emit asm -o /tmp/lab_cpu_abi.s labs/crates/lab-cpu/src/lib.rs
grep -n -A14 -E '^_?lab_sum8:'       /tmp/lab_cpu_abi.s
grep -n -A12 -E '^_?lab_mk:'         /tmp/lab_cpu_abi.s
grep -n -A8  -E '^_?lab_mk2:'        /tmp/lab_cpu_abi.s
grep -n -A24 -E '^_?lab_two:'        /tmp/lab_cpu_abi.s
grep -n -A14 -E '^_?lab_leaf_local:' /tmp/lab_cpu_abi.s
# lặp lại với --target x86_64-unknown-linux-gnu và --target aarch64-unknown-linux-gnu

# frame đệ quy: asm của descend và số đo, debug rồi release
rustc --edition 2024 -C opt-level=3 -C panic=abort --emit asm -o /tmp/frames_o3.s labs/crates/lab-cpu/src/bin/frames.rs
cd labs
cargo run -q -p lab-cpu --bin frames
cargo run -q -p lab-cpu --release --bin frames
```

**Dự đoán (nếu lý thuyết đúng).**

1. **`lab_sum8` trên x86-64.** Sáu tham số nguyên đầu đi qua `rdi, rsi, rdx, rcx, r8, r9`[^sysv-int]; hết thanh ghi thì cả đối số được truyền trên stack[^sysv-nothing]. Vậy `g` và `h` được đọc từ bộ nhớ quanh `rsp`. Vì lúc vào hàm `rsp` trỏ vào địa chỉ trả về do `call` vừa đẩy[^sysv-entry], tham số stack đầu tiên nằm ở `rsp+8` (suy luận của người soạn, khớp ví dụ ở [chương 04 mục 2](../02-cpu/04-calling-convention-va-stack-frame.md)). Kết quả trả về nằm trong `rax`[^sysv-ret-int].
2. **`lab_sum8` trên AArch64.** `x0–x7` dùng để truyền tham số và trả kết quả[^aapcs-r0r7]; tám tham số vừa đủ nên kỳ vọng **không có lệnh đọc stack** nào và kết quả ở `x0` (suy luận từ quy tắc trên).
3. **`lab_mk` (24 byte).** x86-64: caller cấp vùng nhớ cho giá trị trả về và truyền địa chỉ như tham số ẩn đầu tiên[^sysv-ret-mem] — nên tham số thật `x` bị đẩy sang thanh ghi *thứ hai* (`rsi`), một chi tiết dễ bỏ sót khi đọc (suy luận từ "tham số ẩn đầu tiên"). AArch64: địa chỉ vùng nhớ nằm trong `x8`, không chiếm `x0`[^aapcs-x8].
4. **`lab_mk2` (16 byte).** Giá trị hai eightbyte kiểu INTEGER dùng dãy thanh ghi trả về, không cần vùng nhớ cấp từ ngoài[^sysv-ret-int]; kỳ vọng hàm ngắn, không có lệnh ghi bộ nhớ (suy luận; x86-64 dùng `rax` và `rdx`, theo [chương 04 mục 2](../02-cpu/04-calling-convention-va-stack-frame.md)).
5. **`lab_two`: thanh ghi callee-saved.** Giá trị `a` phải sống qua lời gọi `lab_ext` thứ hai. Đặc tả SysV yêu cầu hàm được gọi giữ nguyên một tập thanh ghi cho caller[^sysv-preserve], còn AAPCS64 yêu cầu bảo toàn `r19–r29` và `SP`[^aapcs-preserve]. Vì vậy kỳ vọng `lab_two` đặt `a` vào một thanh ghi *callee-saved*, và để làm vậy nó phải cất giá trị cũ ở prologue và khôi phục ở epilogue: x86-64 thường thấy `push`/`pop`, AArch64 thường thấy `stp`/`ldp` (suy luận từ quy tắc; mẫu cụ thể là lựa chọn của compiler). Trên AArch64 còn có lý do thứ hai: `bl` ghi địa chỉ trả về vào link register[^aapcs-bl], nên hàm không phải lá phải tự cất nó trước khi gọi `bl` tiếp theo (suy luận từ cơ chế `bl`, xem [chương 04 mục 5](../02-cpu/04-calling-convention-va-stack-frame.md)). Hàm lá (`lab_sum8`, `lab_mk`) không cần frame (suy luận của người soạn).
6. **Căn lề stack.** x86-64: stack phải căn 16 byte ngay trước lệnh `call`[^sysv-align]; AArch64: `SP mod 16 = 0` ở mọi thời điểm truy cập bộ nhớ qua SP[^aapcs-sp16]. Một hệ quả bạn có thể tìm trong asm của `lab_two` trên x86-64: số byte `push` + `sub rsp` cộng 8 byte địa chỉ trả về phải làm `rsp` lại là bội của 16 trước hai lệnh `call` — nếu số lượng thanh ghi cần cất là lẻ, compiler thêm một lệnh chỉ để chỉnh stack ([chương 04 mục 6](../02-cpu/04-calling-convention-va-stack-frame.md), suy luận của người soạn).
7. **`lab_leaf_local` và red zone.** SysV cho phép dùng vùng 128 byte ngay dưới `rsp` mà handler tín hiệu/ngắt không được sửa[^sysv-red]. Hàm lá có mảng cục bộ có thể (không bắt buộc) đặt mảng ở địa chỉ âm so với `rsp` và không có `sub rsp` nào ở x86-64; kiểm tra xem compiler của bạn có làm vậy không, và AArch64 có làm tương tự không ([chương 04 mục 4](../02-cpu/04-calling-convention-va-stack-frame.md) ghi một quan sát: bản AArch64 của hàm này trừ SP thật, và suy luận từ đặc tả AAPCS64 rằng không có red zone như SysV — hãy tự đối chiếu với asm của bạn).
8. **`frames`: phần dư mod 16.** Mỗi tầng đệ quy chạy cùng một hàm, nên biến cục bộ nằm ở cùng một độ lệch so với SP của tầng đó; do đó `delta` giữa hai tầng liên tiếp chính là kích thước một frame. Vì SP luôn bội của 16 trên AArch64[^aapcs-sp16] và `rsp` bội của 16 tại điểm gọi trên x86-64[^sysv-align], kỳ vọng cột `delta % 16` toàn **0** ở cả hai kiến trúc và cả hai profile (suy luận của người soạn từ hai quy tắc căn lề, giả định mọi tầng gọi từ cùng một điểm trong hàm).
9. **`frames`: khớp với asm.** Cộng từ asm của `descend`: AArch64 là mức trừ SP của prologue; x86-64 là 8 byte địa chỉ trả về + 8 × số `push` + hằng số của `sub rsp` — kỳ vọng tổng bằng đúng `|delta|` in ra (suy luận của người soạn; nếu LLVM ở `opt-level=3` đổi hình dạng hàm đệ quy thì tổng này không còn khớp, hãy ghi lại điều bạn thấy). Dấu của `delta`: prologue *trừ* SP nên frame của tầng gọi sâu hơn nằm ở địa chỉ thấp hơn, tức `delta` âm (suy luận từ các lệnh `push`/`sub sp` trong asm). Ở debug (`opt-level=0`) biến trung gian bị cất ra stack nhiều hơn ([chương 07 mục 1](../02-cpu/07-rust-va-may.md)), nên frame nhiều khả năng *không nhỏ hơn* bản release — đối chiếu với asm thay vì tin dự đoán này.

**Câu hỏi tự đối chiếu.**

1. Một hàm Rust (ABI Rust, không phải `extern "C"`) có được bảo đảm dùng đúng các thanh ghi ở trên không? Vì sao các hàm `lab_*` dùng `extern "C"`?
<details><summary>Đáp án</summary>

Không: ABI của Rust không ổn định, chỉ `extern "C"` bám vào calling convention của nền tảng ([chương 04 mục 8](../02-cpu/04-calling-convention-va-stack-frame.md)); các hàm `extern "C"` ở đây được chọn để dự đoán ở trên áp dụng được cho đặc tả SysV/AAPCS64 đã trích.

</details>

2. Trong asm của `lab_two` trên AArch64, tìm lệnh lưu `x30` và `x29`. `x30` chứa gì, và tại sao `lab_sum8` không cần lưu nó?
<details><summary>Đáp án</summary>

`x30` là link register chứa địa chỉ trả về do `bl` ghi vào[^aapcs-bl]. Hàm lá không gọi `bl` nên không có thứ gì ghi đè `x30` (suy luận từ cơ chế `bl`). `x29` là frame pointer: AAPCS64 yêu cầu code tuân thủ dựng danh sách liên kết các frame qua frame record[^aapcs-fp]. Đặc tả không bắt buộc *cách* lưu, nên mẫu `stp` cụ thể là lựa chọn của compiler.

</details>

3. Frame (activation record) là gì và dùng để làm gì?
<details><summary>Đáp án</summary>

Phần stack được cấp cho một lần gọi hàm; Dive into Systems gọi nó là stack frame, còn có tên activation frame hoặc activation record[^dis-frame].

</details>

## 4. Lab 3 — Branch prediction

**Mục tiêu.** Cùng một vòng lặp, cùng giá trị dữ liệu, chỉ khác *thứ tự* (ngẫu nhiên vs đã sắp xếp), đo xem nhánh phụ thuộc dữ liệu đắt cỡ nào so với bản không nhánh, và so với bản `if` viết thẳng để thấy compiler có tự làm giúp không.

**Lý thuyết cần đọc trước.**

- [Chương 05 mục 3](../02-cpu/05-pipeline-va-speculation.md) — branch prediction và giá của đoán sai.
- [Chương 05 mục 7](../02-cpu/05-pipeline-va-speculation.md) — hệ quả với code: nhánh khó đoán và branchless.
- [Chương 05 mục 8](../02-cpu/05-pipeline-va-speculation.md) — `black_box`, `select_unpredictable`.
- [Chương 03 mục 4](../02-cpu/03-assembly-x86-64-va-aarch64.md) — nhảy có điều kiện và `cmov`/`csel` (để nhận ra bản branchless trong asm).

**Mã.** `src/bin/branch.rs` gọi ba hàm trong `src/lib.rs`, mỗi hàm cộng các byte `>= 128` của một `&[u8]`: `lab_sum_branchy` (có `if`, bọc `black_box` để compiler không đổi thành lệnh chọn có điều kiện), `lab_sum_branchless` (mặt nạ bit, không `if`), `lab_sum_plain` (`if` viết thẳng, compiler tự do). Dữ liệu: `n` byte từ xorshift seed cố định, và một bản sao đã `sort_unstable`. Chương trình `assert_eq!` rằng tổng bằng nhau giữa hai thứ tự và giữa ba biến thể, rồi in bảng thời gian (µs, lần nhanh nhất trong số lần đo).

**Lệnh chạy.**

```bash
cd labs
cargo run -q -p lab-cpu --release --bin branch                 # 1 Mi phần tử, 15 lần
cargo run -q -p lab-cpu --release --bin branch -- 4194304 25   # đổi cỡ dữ liệu / số lần

# asm của ba hàm (từ gốc repo); tìm lệnh nhảy có điều kiện phụ thuộc dữ liệu trong thân vòng lặp
rustc --edition 2024 --crate-type lib -C opt-level=3 --emit asm -o /tmp/lab_cpu_o3.s labs/crates/lab-cpu/src/lib.rs
grep -n -B2 -A40 -E '^_?lab_sum_(branchy|branchless|plain):' /tmp/lab_cpu_o3.s
```

**Dự đoán (nếu lý thuyết đúng).**

1. **Cơ chế.** Vì chờ mọi nhánh quá đắt, CPU đoán hướng nhánh và chạy tiếp theo hướng đã đoán ([chương 05 mục 3](../02-cpu/05-pipeline-va-speculation.md)); bộ dự đoán hiện đại rất chính xác[^dis-bp] và dựa vào lịch sử kết quả các nhánh gần đây[^sp-hist]. Đoán sai thì phải bỏ phần công việc suy đoán sai, và xả pipeline là việc tốn kém[^dis-flush].
2. **`lab_sum_branchy`.** Trên dữ liệu đã sắp xếp, nhánh `x >= 128` cho một chuỗi dài "không" rồi một chuỗi dài "có" — mẫu cực dễ đoán; trên dữ liệu ngẫu nhiên gần như không có mẫu để học. Kỳ vọng: cột *random* **chậm hơn rõ** cột *sorted* (suy luận của người soạn từ hai điểm trên; tôi không đưa con số vì chưa có nguồn cho chi phí đoán sai cụ thể và nó phụ thuộc vi kiến trúc — [chương 05 mục 3](../02-cpu/05-pipeline-va-speculation.md) cũng không đưa con số chung). [Chương 05 mục 7](../02-cpu/05-pipeline-va-speculation.md) có một phép đo của người soạn trên một chip khác để bạn so tỉ lệ; đừng so số tuyệt đối.
3. **`lab_sum_branchless`.** Không có nhánh phụ thuộc dữ liệu thì không có gì để đoán sai: kỳ vọng thời gian **gần như như nhau** giữa random và sorted (suy luận của người soạn). Điều kiện: asm của hàm này phải thật sự không chứa lệnh nhảy có điều kiện nào ngoài điều kiện lặp — kiểm tra bằng `grep`. Eager execution/chuyển dữ liệu có điều kiện giữ pipeline chạy liên tục nhưng không phải code nào cũng dùng được, và nguy hiểm khi có dereference con trỏ hay side effect[^dis-eager-limit].
4. **`black_box`.** Nó là hàm identity gợi ý compiler bi quan tối đa về nó[^bb-def], hữu ích khi không muốn một số tối ưu như trong benchmark[^bb-bench]; ở đây vai trò của nó là chặn việc đổi `if` thành lệnh chọn không nhánh (xem thêm [chương 07 mục 6](../02-cpu/07-rust-va-may.md) về `black_box` và đo hiệu năng). Docs cũng nói rõ nó chỉ là best-effort: chương trình không được dựa vào nó cho tính đúng đắn ngoài việc nó là identity[^bb-limits].
5. **`lab_sum_plain`.** Không có nguồn nào cho biết compiler của bạn sẽ làm gì với `if` viết thẳng; có hai khả năng: (a) vòng lặp vẫn có nhánh phụ thuộc dữ liệu → hành vi như `lab_sum_branchy`; (b) compiler đã biến nó thành code không nhánh hoặc vector hoá → hành vi như `lab_sum_branchless` hoặc còn nhanh hơn (suy luận của người soạn). Quyết định bằng asm, không bằng đoán: đó là toàn bộ ý nghĩa của bài. Tài liệu `select_unpredictable` nói rõ phép biến đổi sang lệnh chọn có điều kiện không được bảo đảm trên bất kỳ nền tảng nào[^rs-su].
6. **Kiểm tra tính đúng.** Cả ba tổng bằng nhau và không đổi theo thứ tự (chương trình `assert_eq!`; nếu assert fail thì đó là lỗi code chứ không phải hiện tượng CPU).

**Câu hỏi tự đối chiếu.**

1. Nếu bạn đổi ngưỡng `128` trong `lab_sum_branchy` thành `255` (nhánh hầu như luôn "không"), bạn kỳ vọng khoảng cách random vs sorted thế nào, và vì sao?
<details><summary>Đáp án</summary>

Kỳ vọng thu hẹp mạnh: nhánh gần như luôn đi một hướng nên có mẫu dễ đoán dù dữ liệu ngẫu nhiên (suy luận của người soạn từ cơ chế dự đoán dựa trên lịch sử nhánh[^sp-hist]). Hãy kiểm chứng; nếu khoảng cách vẫn lớn, hiểu sai ở đâu?

</details>

2. Bản branchless nhanh hơn bản có nhánh ở *mọi* dữ liệu không? Điều gì trong lý thuyết khiến bạn nghi ngờ?
<details><summary>Đáp án</summary>

Không nên kỳ vọng vậy: hint chuyển sang lệnh chọn có điều kiện có thể *giảm* hiệu năng nếu điều kiện vốn dễ đoán, và docs khuyên đo[^rs-su]; branchless mua tính ổn định chứ không miễn phí (nhận xét của người soạn, theo [chương 05 mục 7](../02-cpu/05-pipeline-va-speculation.md)). Bạn có thể xem cột *sorted* của hai hàm để kiểm tra.

</details>

3. Vì sao phải `--release`?
<details><summary>Đáp án</summary>

Ở debug, `opt-level=0` nên hai hàm không còn là thứ bạn định so sánh (kiểm tra tràn số, biến trung gian ra stack — xem lab 1); release dùng profile tối ưu hoá[^cargo-release].

</details>

## 5. Lab 4 — Cache line và stride

**Mục tiêu.** Quan sát spatial locality và đơn vị dòng cache: duyệt mảng với bước nhảy (stride) tăng dần, tìm "điểm gãy" nơi thời gian mỗi lượt bắt đầu giảm, rồi so thứ tự truy cập tuần tự với ngẫu nhiên trên cùng tập dòng cache.

**Lý thuyết cần đọc trước.**

- [Chương 06 mục 2](../02-cpu/06-cache-va-numa.md) — locality và cache line (kể cả phần "dòng bao nhiêu byte").
- [Chương 06 mục 7](../02-cpu/06-cache-va-numa.md) — prefetching.
- [Chương 06 mục 1](../02-cpu/06-cache-va-numa.md) — hệ phân cấp bộ nhớ (để hiểu vì sao buffer phải lớn hơn cache).

**Mã.** `src/bin/stride.rs`. Buffer `Vec<u32>` 64 MiB.

- *Phần A*: với `stride = 1, 2, 4, …` phần tử (đến `4 × dòng cache` byte), mỗi lượt duyệt `buf.iter_mut().step_by(stride)` và cộng 1 vào mỗi phần tử được chạm. Bảng in: stride (phần tử và byte), số lần truy cập, số dòng cache bị chạm (**ước tính** theo kích thước dòng bạn truyền vào), ms mỗi lượt, ns mỗi lần truy cập. `stride` đi qua `black_box` để mọi hàng của bảng dùng cùng một vòng lặp máy (không bị đặc hóa riêng cho stride = 1).
- *Phần B*: lấy đúng một phần tử mỗi dòng (theo kích thước dòng đã giả định), duyệt theo thứ tự tuần tự rồi theo thứ tự xáo trộn (Fisher–Yates, seed cố định) — cùng số lần truy cập, cùng tập dòng. Hai lần duyệt `assert_eq!` tổng bằng nhau.

Chương trình **không đọc** kích thước dòng thật của máy; tham số dòng chỉ ảnh hưởng cột "lines(est)" và chọn khoảng cách ở phần B. Điểm gãy ở phần A mới là *phép đo* kích thước dòng.

**Lệnh chạy.**

```bash
cd labs
cargo run -q -p lab-cpu --release --bin stride          # giả định 64 B
cargo run -q -p lab-cpu --release --bin stride -- 128   # giả định 128 B (đổi bảng và phần B, không đổi máy)

# Tuỳ chọn: xem kích thước dòng mà hệ điều hành báo (Linux), để đối chiếu với điểm gãy
cat /sys/devices/system/cpu/cpu0/cache/index0/coherency_line_size   # Linux; có thể không tồn tại
sysctl hw.cachelinesize                                               # macOS
```

**Dự đoán (nếu lý thuyết đúng).**

1. **Dòng cache là đơn vị chuyển.** Cache lưu theo dòng gồm nhiều byte liền kề nhờ spatial locality; khi truy cập một phần tử, cả các phần tử kế bên cũng được nạp, số lượng tuỳ block size của cache[^dis-block]. Drepper (viết năm 2007) nói dòng 32 byte ở các cache đời đầu, còn "chuẩn" lúc đó là 64 byte[^drep-line] — đó là mốc thời gian của bài viết chứ không phải khẳng định cho mọi CPU hôm nay ([chương 06 mục 2](../02-cpu/06-cache-va-numa.md) có dữ liệu một chip cho dòng 128 byte).
2. **Phần A, stride nhỏ hơn dòng.** Với `u32` (4 byte), stride 1, 2, 4, … vẫn chạm *mọi* dòng của buffer (mỗi dòng có ít nhất một phần tử được chạm). Dive into Systems: duyệt theo thứ tự lưu trữ nhanh hơn vì "trả chi phí đến bộ nhớ một lần cho mỗi block giá trị"[^dis-matrix]. Kỳ vọng: nếu chương trình bị giới hạn bởi bộ nhớ (buffer lớn hơn cache — nhận xét của người soạn, điều kiện *phải kiểm tra* trên máy có L3 lớn), `ms/pass` **gần như không đổi** từ stride 1 đến khi `stride(B)` tiến sát kích thước dòng, dù số lần truy cập giảm một nửa mỗi bước; hệ quả là `ns/access` **tăng gần tỉ lệ** với stride trong đoạn này (suy luận của người soạn từ "số dòng không đổi").
3. **Phần A, điểm gãy.** Khi `stride(B)` ≥ kích thước dòng thật, mỗi lần truy cập là một dòng riêng: số dòng chạm bằng số lần truy cập và giảm một nửa mỗi bước, nên kỳ vọng `ms/pass` **bắt đầu giảm** từ điểm đó và `ns/access` đi ngang (xấp xỉ giá của việc nạp một dòng — suy luận của người soạn). *Vị trí điểm gãy chính là kích thước dòng của máy bạn*; nếu bạn truyền `64` mà điểm gãy rơi ở 128 byte, máy bạn có dòng 128 byte hoặc cơ chế kéo hai dòng một lúc ([chương 06 mục 2](../02-cpu/06-cache-va-numa.md), phần Đi sâu, nêu lý do `crossbeam-utils` chọn 128 bi quan). Điểm gãy có thể mờ vì prefetch: phần cứng thường kích hoạt prefetch bởi một chuỗi từ hai cache miss trở lên theo một mẫu nhất định[^drep-hwpf]; truy cập stride cố định là một mẫu đều (suy luận của người soạn), nên prefetcher có thể che bớt chi phí ở các stride lớn — không có nguồn về prefetcher của CPU cụ thể nào nên không khẳng định mức độ.
4. **Phần B.** Hai lần duyệt đọc đúng cùng tập phần tử (một phần tử mỗi dòng), chỉ khác thứ tự. Prefetch phần cứng nhận ra chuỗi miss theo mẫu đều; nếu *mọi* miss đều kích hoạt prefetch thì có hại, nên truy cập không có mẫu không nên kích hoạt nó[^drep-hwpf2]; với thứ tự ngẫu nhiên Drepper nhận xét prefetcher khó có tác dụng[^drep-pfrand]. Kỳ vọng: duyệt **xáo trộn chậm hơn** duyệt tuần tự dù số lần truy cập và tập dòng giống hệt (suy luận của người soạn từ hai nguồn trên). Mức chênh phụ thuộc mức độ CPU chồng lấn các lần nạp độc lập (xem [chương 05 mục 4](../02-cpu/05-pipeline-va-speculation.md)); phần TLB (bảng dịch địa chỉ ảo) cũng đóng góp nhưng chương này không có nguồn nên không dự đoán tỉ lệ.
5. **Giới hạn của thí nghiệm.** Đây là *một* thí nghiệm trên *một* máy; kết luận hợp lệ duy nhất là điểm gãy của máy đó, không phải một con số chung cho "ARM" hay "x86".

**Câu hỏi tự đối chiếu.**

1. Duyệt `buf.iter_mut().step_by(16)` chạm 1/16 số phần tử của buffer. Vì sao thời gian không giảm 16 lần so với `step_by(1)`?
<details><summary>Đáp án</summary>

Vì đơn vị di chuyển giữa DRAM và cache là cả dòng, không phải từng phần tử[^dis-block]; nếu dòng 64 byte chứa 16 `u32` thì `step_by(16)` vẫn chạm mọi dòng của buffer (suy luận từ các nguồn trên, với giả định dòng 64 byte). Kỳ vọng giảm 16 lần chỉ đúng khi phép toán chi phối thời gian, không phải truy cập bộ nhớ.

</details>

2. Bạn truyền `64` nhưng điểm gãy ở `stride(B) = 128`. Hai lời giải thích khả dĩ?
<details><summary>Đáp án</summary>

(a) Dòng thật là 128 byte; (b) phần cứng kéo từng cặp dòng 64 byte — lý do mà tài liệu `crossbeam-utils` nêu cho việc chọn N = 128 trên Intel ([chương 06 mục 2](../02-cpu/06-cache-va-numa.md)). Bạn phân biệt được bằng cách đối chiếu với `coherency_line_size`/`sysctl` ở lệnh tuỳ chọn, nhớ rằng Linux VM có thể không xuất tệp đó ([chương 06 mục 2](../02-cpu/06-cache-va-numa.md)); ngay cả khi có, "dòng" mà OS báo có thể khác "khoảng chuyển" thực tế của phần cứng (suy luận của người soạn).

</details>

3. Vì sao phải dùng buffer lớn (64 MiB) và vì sao không so `ms/pass` ở buffer 16 KiB?
<details><summary>Đáp án</summary>

Buffer nhỏ nằm gọn trong cache nên các lượt sau không phải đến DRAM; cái bạn đo là tốc độ lệnh chứ không phải tốc độ nạp dòng ([chương 06 mục 1](../02-cpu/06-cache-va-numa.md)). Hãy thử đổi `LEN` trong `stride.rs` và đối chiếu (bài tập mở rộng).

</details>

## 6. Lab 5 — False sharing

**Mục tiêu.** Cho hai thread mỗi thread ghi vào biến *riêng của nó* và quan sát việc chỉ bố cục bộ nhớ (hai biến chung hay khác dòng cache) đã đổi thời gian.

**Lý thuyết cần đọc trước.**

- [Chương 06 mục 5](../02-cpu/06-cache-va-numa.md) — nhiều core, cache coherence, MESI.
- [Chương 06 mục 6](../02-cpu/06-cache-va-numa.md) — false sharing.
- [Chương 07 mục 7](../02-cpu/07-rust-va-may.md) — `#[repr(align)]` và `CachePadded`.

**Mã.** `src/bin/false_sharing.rs`: hai thread (`std::thread::scope`), mỗi thread lặp `n` lần `a.store(a.load(Relaxed) + 1, Relaxed)` lên atomic của mình (không dùng `fetch_add`: chỉ thread đó ghi nên không cần thao tác đọc-sửa-ghi nguyên tử). Ba bố cục: `[AtomicU64; 2]` (hai atomic liền nhau), `#[repr(align(64))]` bọc mỗi atomic, `#[repr(align(128))]` bọc mỗi atomic; thêm một mốc *một thread, một atomic*. Chương trình in `available_parallelism`, `size_of` của các bố cục, thời gian, và hai cột `bool` cho biết địa chỉ hai atomic có thuộc cùng một "dòng" 64 B / 128 B hay không (tính từ địa chỉ thật, chia nguyên cho 64 hoặc 128).

**Lệnh chạy.**

```bash
cd labs
cargo run -q -p lab-cpu --release --bin false_sharing               # 50 triệu vòng mỗi thread
cargo run -q -p lab-cpu --release --bin false_sharing -- 20000000   # chạy nhiều lần, so độ dao động
```

**Dự đoán (nếu lý thuyết đúng).**

1. **Kích thước.** `[AtomicU64; 2]` có kích thước `2 × size_of::<AtomicU64>()` và cùng alignment với phần tử[^array-layout] — nên hai atomic cách nhau 8 byte (suy luận, với `size_of::<AtomicU64>() == 8`) và nằm cùng dòng 64 B, trừ trường hợp mảng vắt qua ranh giới dòng. `#[repr(align(N))]` nâng alignment của struct[^repr-align] và kích thước luôn là bội của alignment[^size-align], nên `Pad64` có `size_of` 64 và `Pad128` có 128 (suy luận từ hai quy tắc; nếu alignment chỉ định nhỏ hơn alignment tự nhiên thì nó không có tác dụng[^repr-align-less] — ở đây `AtomicU64` có alignment 8 nên 64 và 128 đều lớn hơn).
2. **Cột "chung dòng".** Hàng `adjacent`: `true` ở cột 64 B (gần như chắc chắn, trừ khi vắt ranh giới) và `true` ở cột 128 B. Hàng `align(64)`: `false` ở cột 64 B vì hai phần tử cách nhau đúng 64 byte và bắt đầu ở địa chỉ bội của 64; cột 128 B tuỳ vị trí mảng, có thể `true` — bạn chạy nhiều lần và xem. Hàng `align(128)`: `false` ở cả hai cột.
3. **Cơ chế.** Coherence hoạt động ở độ chi tiết cả dòng: khi một core ghi, snoopy cache vô hiệu hoá cả dòng chứa biến đó chứ không chỉ biến đó[^dis-fs]. Nếu hai thread ghi hai biến khác nhau nhưng cùng dòng, hai core tranh quyền sở hữu dòng; Drepper nêu rõ vấn đề vẫn xuất hiện khi các thread dùng ô nhớ khác nhau và tưởng là độc lập[^drep-fs]. Tài liệu `CachePadded` nói đúng điều đó với atomic: cập nhật một giá trị atomic làm mất hiệu lực cả dòng cache của nó, khiến lần truy cập kế tiếp của core khác chậm hơn[^cp-why].
4. **Kỳ vọng thời gian.** Hàng `adjacent` **chậm hơn** hàng mà hai atomic nằm ở hai dòng khác nhau, miễn máy có ít nhất hai core cùng chạy hai thread (suy luận của người soạn từ cơ chế trên). Hàng `align(128)` (hai dòng khác nhau chắc chắn) kỳ vọng gần với mốc "một thread" (mỗi core giữ dòng riêng, không có gì để vô hiệu hoá — suy luận của người soạn; mốc này còn chứa chi phí tạo thread rất nhỏ so với `n` lớn, và hai thread chạy song song nên tổng thời gian ≈ thời gian của một thread). Hàng `align(64)`: gần với `align(128)` nếu dòng của máy bạn là 64 byte; nếu máy bạn có dòng 128 byte hoặc kéo cặp dòng, hàng này có thể chậm gần như `adjacent` khi cột "chung dòng 128 B" là `true` — đó cũng là một cách đo kích thước dòng.
5. **Điều kiện để dự đoán đúng.** `available_parallelism` ≥ 2 và OS xếp hai thread lên hai core khác nhau (chương trình không ghim thread — OS tự chọn). Nếu chỉ có một core, hoặc OS xếp hai thread xen kẽ trên cùng một core, bạn không thấy hiện tượng; chạy lại vài lần để nhìn độ dao động.
6. **Chọn N khi viết thật.** Crate `crossbeam-utils` đặt N = 128 trên x86-64, aarch64 và powerpc64[^cp-n] và nói rõ N chỉ là phỏng đoán hợp lý, không bảo đảm khớp độ dài dòng thật của máy[^cp-guess]. Lab này không dùng crate đó (không thêm dependency) mà dùng `repr(align)` trực tiếp để bạn thấy cơ chế bên dưới.
7. **So với P4.** [Lab P4](04-concurrency-lab.md) có bài `false_sharing_counters` đo cùng hiện tượng nhưng dùng `fetch_add(Relaxed)` trên counter riêng của mỗi thread, còn lab này dùng `load` rồi `store`; false sharing cũng được nhắc ở phần Đi sâu của [chương 07 mục 5 của P4](../04-concurrency/07-lock-free.md). Hai bài khác nhau ở thao tác ghi, nên đừng so số tuyệt đối giữa chúng.

**Câu hỏi tự đối chiếu.**

1. Mỗi thread chỉ ghi vào biến của mình và không đọc biến của thread kia. Vì sao vẫn chậm?
<details><summary>Đáp án</summary>

Vì coherence làm việc theo dòng, không theo biến[^dis-fs]; hai biến chung một dòng nên mỗi lần ghi của một core vô hiệu hoá dòng ở core kia[^cp-why], dù không có dữ liệu logic nào được chia sẻ[^drep-fs].

</details>

2. Tại sao hàng `align(64)` không phải lúc nào cũng nhanh bằng `align(128)`?
<details><summary>Đáp án</summary>

Hai khả năng, tuỳ máy: dòng thật của máy là 128 byte, hoặc phần cứng nạp theo cặp dòng 64 byte — cùng lý do `crossbeam-utils` chọn N = 128 "bi quan" cho Intel ([chương 06 mục 2](../02-cpu/06-cache-va-numa.md)). Nếu đúng vậy thì hai `Pad64` có thể thuộc cùng một dòng 128 B (cột cuối).

</details>

3. Cái giá của việc "vá" false sharing bằng `repr(align)` là gì?
<details><summary>Đáp án</summary>

Mỗi biến chiếm cả một dòng (`size_of` 64 hoặc 128 thay vì 8), nên chỉ đệm các trường nóng (nhận xét của người soạn, theo [chương 06 mục 6](../02-cpu/06-cache-va-numa.md)); kích thước của `CachePadded<T>` là bội nhỏ nhất của N đủ chứa `T`[^cp-size].

</details>

## 7. Lab 6 — Endianness và biểu diễn số

**Mục tiêu.** Nhìn trực tiếp byte của số trong bộ nhớ, phân biệt thứ tự byte (endianness) với cách diễn giải bit (có dấu/không dấu), và thấy `f64` là một mẫu bit chứ không phải "số thập phân".

**Lý thuyết cần đọc trước.**

- [Chương 02 mục 1–2](../02-cpu/02-bieu-dien-du-lieu.md) — bit, byte, two's complement, `as` giữa số nguyên.
- [Chương 02 mục 4](../02-cpu/02-bieu-dien-du-lieu.md) — endianness và network byte order.
- [Chương 02 mục 5](../02-cpu/02-bieu-dien-du-lieu.md) — `f64` IEEE 754.

**Mã.** `src/bin/endian.rs` (không có phần đo thời gian, chạy debug hay release đều được). Chương trình in: `target_endian`; các byte của `0x12345678u32` qua `to_le_bytes`, `to_be_bytes`, `to_ne_bytes` và qua *đọc thẳng vùng nhớ của biến* (một khối `unsafe` nhỏ tạo slice `u8` trên `x`, có chú thích `SAFETY`); giải mã trường 16 bit `00 50` (port 80 trên dây) bằng `from_be_bytes`, `from_le_bytes`, `from_ne_bytes`; các phép `as` giữa số nguyên có dấu/không dấu, `wrapping_neg`/`checked_neg` của `i8::MIN`; `to_bits` và byte big-endian của vài giá trị `f64`; và ba phép so sánh (`0.1 + 0.2 == 0.3`, `NaN == NaN`, `0.0 == -0.0`).

**Lệnh chạy.**

```bash
cd labs
cargo run -q -p lab-cpu --bin endian
# (tuỳ chọn) so với một target big-endian chỉ ở mức biên dịch: cargo check -p lab-cpu --bin endian --target <triple big-endian có cài sẵn>
```

**Dự đoán (nếu lý thuyết đúng).**

1. **Thứ tự byte.** `to_be_bytes` trả byte theo thứ tự big-endian (network)[^std-be]; thứ tự theo `0x12345678` là `12 34 56 78`. `to_le_bytes` là thứ tự ngược lại: `78 56 34 12`. `to_ne_bytes` dùng endianness của nền tảng đích nên mã portable nên dùng `to_be_bytes` hoặc `to_le_bytes`[^std-ne]. Kỳ vọng: `to_ne_bytes` và dòng "bộ nhớ thật của x" **giống nhau** (cả hai đều là biểu diễn trong bộ nhớ trên máy của bạn) và bằng `to_le_bytes` nếu máy little-endian. Dùng man page `byteorder(3)`: trên i386 byte thứ tự host là Least Significant Byte first còn network order là Most Significant Byte first[^man-byteorder2]. Với AArch64: giao diện dữ liệu và lệnh là little-endian tự nhiên, dù giao diện dữ liệu có thể được dựng cho big-endian[^arm-endian] — nên Linux/macOS aarch64 thông thường in `little` (suy luận của người soạn: tôi không có nguồn nói về cấu hình của hệ điều hành cụ thể của bạn; dòng `target_endian` đầu output mới là sự thật về bản biên dịch). Với x86-64: nguồn trên chỉ nói về i386; việc x86-64 cùng thứ tự là suy luận của người soạn từ cùng họ ISA.
2. **Network byte order.** Internet truyền byte có nghĩa lớn nhất trước[^rfc791]. Trường 16 bit `00 50` bên dây là port 80: `from_be_bytes` ra `0x0050` = 80; `from_le_bytes` ra `0x5000` = 20480; `from_ne_bytes` trùng một trong hai tuỳ `target_endian` (suy luận của người soạn từ định nghĩa; con số 20480 cũng đã xuất hiện trong [chương 02 mục 4](../02-cpu/02-bieu-dien-du-lieu.md)). Hàm `htonl` đổi số nguyên không dấu từ host byte order sang network byte order[^man-byteorder], khái niệm Rust tương ứng là `to_be`/`to_be_bytes`.
3. **Dấu và không dấu: cùng bit, hai cách đọc.** Số nguyên có dấu của Rust luôn biểu diễn bằng two's complement[^ref-twos]. Ép giữa hai số nguyên cùng kích thước là no-op trên mẫu bit[^ref-noop]; vậy `-1i8 as u8` là cùng mẫu bit `1111_1111` đọc không dấu = 255, `-1i32 as u32` là `0xffffffff` và `200u8 as i8` là `-56` (suy luận: 200 − 256; bạn kiểm tra bằng máy). Ép sang kiểu hẹp hơn sẽ cắt bỏ các bit cao[^ref-trunc]; sang kiểu rộng hơn thì sign-extend nếu nguồn có dấu[^ref-ext] (chương trình lab không in trường hợp này; thử thêm `-1i8 as u32` rồi dự đoán trước).
4. **`i8::MIN` không có số đối.** Reference nói phủ định các giá trị âm nhất này giữ nguyên giá trị do quy ước tràn của two's complement[^ref-neg]. Kỳ vọng: `i8::MIN.wrapping_neg()` là `-128` và `i8::MIN.checked_neg()` là `None` (suy luận từ tên và ngữ nghĩa các phương thức wrapping/checked; xem [chương 02 mục 3](../02-cpu/02-bieu-dien-du-lieu.md)).
5. **`f64` trong bộ nhớ.** `f64` có bit dấu, exponent có bias — 1023 với độ chính xác kép[^gb-bias] — và fraction. Từ đó suy luận (người soạn): `1.0` có dấu 0, exponent lưu `1023 = 0x3ff` và fraction 0 nên `to_bits() = 0x3ff0000000000000`; `-2.0` có bit dấu 1 và exponent lưu `1024 = 0x400` nên `0xc000000000000000`. Có hai số không (+0 và −0) vì bit dấu nhận hai giá trị[^gb-zero]; mẫu bit của `-0.0` khác `0.0` ở bit dấu, nhưng phép so sánh định nghĩa sao cho +0 = −0[^gb-zerocmp], nên `0.0 == -0.0` in `true`. [Chương 02 mục 5](../02-cpu/02-bieu-dien-du-lieu.md) ghi các giá trị `to_bits` của `0.1` và `-0.0` đã chạy để bạn so.
6. **`0.1 + 0.2 == 0.3` in `false`.** Số thập phân hữu hạn này có biểu diễn nhị phân lặp vô hạn nên bị làm tròn khi nạp[^gb-01], và phép cộng tính chính xác rồi làm tròn tới số gần nhất (round to even)[^gb-round]; hai lần làm tròn đủ để hai vế khác mẫu bit.
7. **`NaN == NaN` in `false`.** NaN không bao giờ bằng số nào, kể cả một NaN khác[^gb-naneq]; vì vậy float chỉ cài `PartialEq` chứ không `Eq`[^std-partialeq]. (Mã dùng `black_box` để rustc/clippy không cảnh báo so sánh hằng — chỉ là kỹ thuật tránh lint, không ảnh hưởng ngữ nghĩa.)

**Câu hỏi tự đối chiếu.**

1. Một header nhị phân có trường độ dài 32 bit big-endian. Bạn thấy bốn byte `00 00 01 00` trên dây. Giá trị là bao nhiêu, và đoạn code nào sai trên máy little-endian?
<details><summary>Đáp án</summary>

`u32::from_be_bytes([0, 0, 1, 0]) = 0x0000_0100 = 256` (byte có nghĩa lớn nhất truyền trước[^rfc791]). Ép con trỏ sang `*const u32` rồi đọc trên máy little-endian sẽ ra `0x0001_0000` = 65536 (suy luận từ bảng thứ tự byte ở [chương 02 mục 4](../02-cpu/02-bieu-dien-du-lieu.md)); an toàn hơn là giải mã từng trường từ byte bằng `from_be_bytes`.

</details>

2. Vì sao `u16::from_ne_bytes` không nên xuất hiện trong code đọc giao thức mạng?
<details><summary>Đáp án</summary>

Vì kết quả phụ thuộc endianness của nền tảng đích; tài liệu khuyên code portable dùng `to_be_bytes`/`to_le_bytes`[^std-ne], và trường mạng có thứ tự byte cố định theo giao thức[^rfc791].

</details>

3. `(-0.0f64).to_bits() != 0.0f64.to_bits()` nhưng `-0.0 == 0.0`. Cái nào là "đúng"?
<details><summary>Đáp án</summary>

Cả hai: `to_bits` so sánh mẫu bit (khác bit dấu[^gb-zero]) còn `==` theo định nghĩa IEEE coi +0 = −0[^gb-zerocmp]. Hậu quả: không dùng so sánh mẫu bit để thay `==` cho số thực khi muốn ngữ nghĩa số học.

</details>

## 8. Bài tập mở rộng

Mỗi bài hãy ghi **dự đoán trước**, kèm nguồn hoặc lý do, rồi mới chạy:

1. (Lab 1) Thêm vào `lib.rs` một hàm `lab_div(a: u64, b: u64) -> u64 { a / b }` và đọc asm debug/release. Tìm nhánh dẫn tới panic khi chia cho 0 và so với `lab_add`; ghi lại điều bạn dự đoán trước khi mở asm.
2. (Lab 1) Thêm `#[inline(always)]` rồi `#[inline(never)]` lên `lab_get` và gọi nó từ một hàm `lab_use_get` với chỉ số hằng; xem `call`/`bl` có còn không. Đối chiếu [chương 07 mục 2](../02-cpu/07-rust-va-may.md).
3. (Lab 2) Thêm hàm `extern "C"` nhận 3 tham số `f64` và hai `u64`; dự đoán thanh ghi nào chứa từng tham số trên x86-64 (đọc [chương 04 mục 2](../02-cpu/04-calling-convention-va-stack-frame.md) về `xmm0`–`xmm7` rồi kiểm chứng) và trên AArch64 (đọc mục 5 cùng chương).
4. (Lab 3) Cho `lab_sum_branchy` chạy trên dữ liệu có chu kỳ ngắn (ví dụ lặp lại một dãy 16 byte) thay vì ngẫu nhiên. Dự đoán trước: lịch sử nhánh[^sp-hist] có đủ học mẫu chu kỳ này không?
5. (Lab 4) Đổi `LEN` thành 16 KiB, 256 KiB, 4 MiB, 256 MiB và chạy lại phần A; dự đoán điểm gãy dịch chuyển hay biến mất ở kích thước nào, rồi đối chiếu kích thước L1/L2/L3 của máy bạn.
6. (Lab 5) Thêm bố cục thứ tư: hai `AtomicU64` nằm trong một struct `repr(C)` cùng với 56 byte đệm giữa chúng (tự tính `size_of`). Dự đoán: tương đương `align(64)` hay `adjacent`? Vì sao?
7. (Lab 6) Thêm ép `-1i8 as u32` và `0xFFu8 as i8 as i32 as u32`; dự đoán trước hai kết quả bằng quy tắc sign-extend[^ref-ext].

## Nguồn

[^cargo-release]: The Cargo Book 1.95.0, cargo rustc, Package Selection / Compilation Options — <https://doc.rust-lang.org/1.95.0/cargo/commands/cargo-rustc.html> — "Build optimized artifacts with the release profile."
[^emit]: rustc book 1.95.0, Command-line arguments, --emit — <https://doc.rust-lang.org/1.95.0/rustc/command-line-arguments.html> — "asm — Generates a file with the crate’s assembly code. The default output filename is CRATE_NAME.s."
[^crate-type]: rustc book 1.95.0, Command-line arguments, --crate-type — <https://doc.rust-lang.org/1.95.0/rustc/command-line-arguments.html> — "This instructs rustc on which crate type to build."
[^edition]: rustc book 1.95.0, Command-line arguments, --edition — <https://doc.rust-lang.org/1.95.0/rustc/command-line-arguments.html> — "--edition: specify the edition to use"
[^cargo-rustc]: The Cargo Book 1.95.0, cargo rustc, Description — <https://doc.rust-lang.org/1.95.0/cargo/commands/cargo-rustc.html> — "The specified args will all be passed to the final compiler invocation, not any of the dependencies."
[^target]: rustc book 1.95.0, Command-line arguments, --target — <https://doc.rust-lang.org/1.95.0/rustc/command-line-arguments.html> — "--target: select a target tuple to build"
[^opt]: rustc book 1.95.0, Command-line arguments, -O — <https://doc.rust-lang.org/1.95.0/rustc/command-line-arguments.html> — "A synonym for -C opt-level=3."
[^ovf]: The Rust Reference 1.95.0, Operator expressions, Overflow — <https://doc.rust-lang.org/1.95.0/reference/expressions/operator-expr.html> — "Integer operators will panic when they overflow when compiled in debug mode."
[^ovfflags]: The Rust Reference 1.95.0, Operator expressions, Overflow — <https://doc.rust-lang.org/1.95.0/reference/expressions/operator-expr.html> — "The -C debug-assertions and -C overflow-checks compiler flags can be used to control this more directly."
[^ovf-default]: rustc book 1.95.0, Codegen options, overflow-checks — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "If not specified, overflow checks are enabled if debug-assertions are enabled, disabled otherwise."
[^debug-assertions]: rustc book 1.95.0, Codegen options, debug-assertions — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "If not specified, debug assertions are automatically enabled only if the opt-level is 0."
[^bounds]: The Rust Reference 1.95.0, Array and slice indexing expressions — <https://doc.rust-lang.org/1.95.0/reference/expressions/array-expr.html> — "Otherwise a check will be performed at run-time that will put the thread in a panicked state if it fails."
[^index-panic]: Rust std 1.95.0, std::ops::Index — <https://doc.rust-lang.org/1.95.0/std/ops/trait.Index.html> — "May panic if the index is out of bounds."
[^get-unchecked]: Rust std 1.95.0, primitive slice, get_unchecked — <https://doc.rust-lang.org/1.95.0/std/primitive.slice.html> — "Calling this method with an out-of-bounds index is undefined behavior even if the resulting reference is not used."
[^sysv-int]: System V AMD64 psABI, 3.2.3 Parameter Passing — <https://gitlab.com/x86-psABIs/x86-64-ABI/-/raw/e1ce098331da5dbd66e1ffc74162380bcc213236/x86-64-ABI/low-level-sys-info.tex> — "If the class is INTEGER, the next available register of the sequence"
[^sysv-nothing]: System V AMD64 psABI, 3.2.3 Parameter Passing — <https://gitlab.com/x86-psABIs/x86-64-ABI/-/raw/e1ce098331da5dbd66e1ffc74162380bcc213236/x86-64-ABI/low-level-sys-info.tex> — "If there are no registers available for any eightbyte of an argument, the whole argument is passed on the stack."
[^sysv-entry]: System V AMD64 psABI, 3.2.2 The Stack Frame — <https://gitlab.com/x86-psABIs/x86-64-ABI/-/raw/e1ce098331da5dbd66e1ffc74162380bcc213236/x86-64-ABI/low-level-sys-info.tex> — "immediately after the return address has been pushed,"
[^sysv-ret-int]: System V AMD64 psABI, Returning of Values — <https://gitlab.com/x86-psABIs/x86-64-ABI/-/raw/e1ce098331da5dbd66e1ffc74162380bcc213236/x86-64-ABI/low-level-sys-info.tex> — "If the class is INTEGER, the next available register of the sequence"
[^sysv-ret-mem]: System V AMD64 psABI, Returning of Values — <https://gitlab.com/x86-psABIs/x86-64-ABI/-/raw/e1ce098331da5dbd66e1ffc74162380bcc213236/x86-64-ABI/low-level-sys-info.tex> — "the caller provides space for the return value and passes the address of this storage in"
[^sysv-preserve]: System V AMD64 psABI, 3.2.1 Registers and the Stack Frame — <https://gitlab.com/x86-psABIs/x86-64-ABI/-/raw/e1ce098331da5dbd66e1ffc74162380bcc213236/x86-64-ABI/low-level-sys-info.tex> — "a called function must preserve these registers' values for its caller."
[^sysv-align]: System V AMD64 psABI, 3.2.2 The Stack Frame — <https://gitlab.com/x86-psABIs/x86-64-ABI/-/raw/e1ce098331da5dbd66e1ffc74162380bcc213236/x86-64-ABI/low-level-sys-info.tex> — "In other words, the stack needs to be 16 (32 or 64) byte aligned immediately before the call instruction is executed."
[^sysv-red]: System V AMD64 psABI, 3.2.2 The Stack Frame — <https://gitlab.com/x86-psABIs/x86-64-ABI/-/raw/e1ce098331da5dbd66e1ffc74162380bcc213236/x86-64-ABI/low-level-sys-info.tex> — "to be reserved and shall not be modified by signal or interrupt handlers."
[^aapcs-r0r7]: AAPCS64, General-purpose registers — <https://raw.githubusercontent.com/ARM-software/abi-aa/2025Q4/aapcs64/aapcs64.rst> — "The first eight registers, r0-r7, are used to pass argument values into a subroutine and to return result values from a function."
[^aapcs-preserve]: AAPCS64, General-purpose registers — <https://raw.githubusercontent.com/ARM-software/abi-aa/2025Q4/aapcs64/aapcs64.rst> — "A subroutine invocation must preserve the contents of the registers r19-r29 and SP."
[^aapcs-x8]: AAPCS64, Result return — <https://raw.githubusercontent.com/ARM-software/abi-aa/2025Q4/aapcs64/aapcs64.rst> — "the caller shall reserve a block of memory of sufficient size and alignment to hold the result. The address of the memory block shall be passed as an additional argument to the function in x8."
[^aapcs-bl]: AAPCS64, Subroutine calls — <https://raw.githubusercontent.com/ARM-software/abi-aa/2025Q4/aapcs64/aapcs64.rst> — "performs a branch-with-link operation. The effect of executing BL is to transfer the sequentially next value of the program counter—the return address—into the link register (LR)"
[^aapcs-sp16]: AAPCS64, Universal stack constraints — <https://raw.githubusercontent.com/ARM-software/abi-aa/2025Q4/aapcs64/aapcs64.rst> — "SP mod 16 = 0. The stack must be quad-word aligned."
[^aapcs-fp]: AAPCS64, The Frame Pointer — <https://raw.githubusercontent.com/ARM-software/abi-aa/2025Q4/aapcs64/aapcs64.rst> — "Conforming code shall construct a linked list of stack-frames. Each frame shall link to the frame of its caller by means of a frame record of two 64-bit values on the stack"
[^dis-frame]: Dive into Systems, IA32 functions — <https://diveintosystems.org/book/C8-IA32/functions.html> — "The stack frame (also known as the activation frame or the activation record) refers to the"
[^dis-bp]: Dive into Systems, 5.8 Advanced Pipelining Considerations — <https://diveintosystems.org/book/C5-Arch/pipelining_advanced.html> — "Modern branch predictors are really good and accurate."
[^dis-flush]: Dive into Systems, 5.8 Advanced Pipelining Considerations — <https://diveintosystems.org/book/C5-Arch/pipelining_advanced.html> — "Flushing the pipeline is expensive."
[^dis-eager-limit]: Dive into Systems, 5.8 Advanced Pipelining Considerations — <https://diveintosystems.org/book/C5-Arch/pipelining_advanced.html> — "However, not all code is capable of taking advantage of eager execution, which can be dangerous in the case of pointer dereferences and side effects."
[^sp-hist]: Spectre Attacks: Exploiting Speculative Execution, 2.3 Branch Prediction — <https://arxiv.org/pdf/1801.01203> — "the processor maintains a record of recent branches outcomes."
[^bb-def]: Rust std 1.95.0, std::hint::black_box — <https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html> — "An identity function that hints to the compiler to be maximally pessimistic about what black_box could do."
[^bb-bench]: Rust std 1.95.0, std::hint::black_box — <https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html> — "This property makes black_box useful for writing code in which certain optimizations are not desired, such as benchmarks."
[^bb-limits]: Rust std 1.95.0, std::hint::black_box — <https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html> — "Programs cannot rely on black_box for correctness, beyond it behaving as the identity function."
[^rs-su]: Rust std 1.95.0, std::hint::select_unpredictable — <https://doc.rust-lang.org/1.95.0/std/hint/fn.select_unpredictable.html> — "Note however that this lowering is not guaranteed (on any platform)"
[^dis-block]: Dive into Systems, 11.3 Locality — <https://diveintosystems.org/book/C11-MemHierarchy/locality.html> — "Exactly how many additional integers get moved into the cache depends on the cache’s block size"
[^dis-matrix]: Dive into Systems, 11.3 Locality — <https://diveintosystems.org/book/C11-MemHierarchy/locality.html> — "Thus, it benefits from a system that loads large blocks from memory into the cache because it pays the cost of going to memory only once for every block of values."
[^drep-line]: Drepper, What Every Programmer Should Know About Memory, §3.2 — <https://www.akkadia.org/drepper/cpumemory.pdf> — "In early caches these lines were 32 bytes long; nowadays the norm is 64 bytes."
[^drep-hwpf]: Drepper, What Every Programmer Should Know About Memory, §6.3.1 — <https://www.akkadia.org/drepper/cpumemory.pdf> — "The trigger for the CPU to start hardware prefetching is usually a sequence of two or more cache misses in a certain pattern."
[^drep-hwpf2]: Drepper, What Every Programmer Should Know About Memory, §6.3.1 — <https://www.akkadia.org/drepper/cpumemory.pdf> — "It would be bad for performance if every single cache miss triggered a hardware prefetch."
[^drep-pfrand]: Drepper, What Every Programmer Should Know About Memory, §3.3.2 — <https://www.akkadia.org/drepper/cpumemory.pdf> — "Due to the randomized order of the list elements it is unlikely that the hardware prefetcher has any effect"
[^dis-fs]: Dive into Systems, 14.5.2 False Sharing — <https://diveintosystems.org/book/C14-SharedMemory/cache_coherence.html> — "the snoopy cache invalidates not only g, but the entire cache line that g is a part of."
[^drep-fs]: Drepper, What Every Programmer Should Know About Memory, §6.4.1 — <https://www.akkadia.org/drepper/cpumemory.pdf> — "The problem is also visible, though, when all the threads are using different memory locations and are supposedly independent."
[^cp-why]: crossbeam-utils 0.8.23, CachePadded — <https://docs.rs/crossbeam-utils/0.8.23/crossbeam_utils/struct.CachePadded.html> — "Updating an atomic value invalidates the whole cache line it belongs to, which makes the next access to the same cache line slower for other CPU cores."
[^cp-n]: crossbeam-utils 0.8.23, CachePadded — <https://docs.rs/crossbeam-utils/0.8.23/crossbeam_utils/struct.CachePadded.html> — "On x86-64, aarch64, and powerpc64, N = 128."
[^cp-guess]: crossbeam-utils 0.8.23, CachePadded — <https://docs.rs/crossbeam-utils/0.8.23/crossbeam_utils/struct.CachePadded.html> — "Note that N is just a reasonable guess and is not guaranteed to match the actual cache line length of the machine the program is running on."
[^cp-size]: crossbeam-utils 0.8.23, CachePadded — <https://docs.rs/crossbeam-utils/0.8.23/crossbeam_utils/struct.CachePadded.html> — "The size of CachePadded\<T> is the smallest multiple of N bytes large enough to accommodate a value of type T."
[^repr-align]: The Rust Reference 1.95.0, Type layout, The alignment modifiers — <https://doc.rust-lang.org/1.95.0/reference/type-layout.html> — "The align and packed modifiers can be used to respectively raise or lower the alignment of structs and unions."
[^repr-align-less]: The Rust Reference 1.95.0, Type layout, The alignment modifiers — <https://doc.rust-lang.org/1.95.0/reference/type-layout.html> — "For align, if the specified alignment is less than the alignment of the type without the align modifier, then the alignment is unaffected."
[^size-align]: The Rust Reference 1.95.0, Type layout — <https://doc.rust-lang.org/1.95.0/reference/type-layout.html> — "The size of a value is always a multiple of its alignment."
[^array-layout]: The Rust Reference 1.95.0, Type layout — <https://doc.rust-lang.org/1.95.0/reference/type-layout.html> — "An array of [T; N] has a size of size_of::\<T>() * N and the same alignment of T."
[^rfc791]: RFC 791, Internet Protocol, Notation — <https://www.rfc-editor.org/rfc/rfc791.html> — "When a multi-octet quantity is transmitted the most significant octet is transmitted first."
[^man-byteorder]: byteorder(3), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/byteorder.3.en.html> — "The htonl() function converts the unsigned integer hostlong from host byte order to network byte order."
[^man-byteorder2]: byteorder(3), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/byteorder.3.en.html> — "On the i386 the host byte order is Least Significant Byte first, whereas the network byte order, as used on the Internet, is Most Significant Byte first."
[^arm-endian]: Arm, Porting to 64-bit ARM (white paper, 07/2014, mô tả ARMv8-A thời kỳ đầu), Memory model — <https://community.arm.com/cfs-file/__key/telligent-evolution-components-attachments/01-2142-00-00-00-00-52-01/Porting-to-ARM-64_2D00_bit.pdf> — "The data and instruction interfaces are natively little-endian, although the data interface can be built for big-endian support."
[^std-be]: Rust std 1.95.0, primitive u32, to_be_bytes — <https://doc.rust-lang.org/1.95.0/std/primitive.u32.html> — "Returns the memory representation of this integer as a byte array in big-endian (network) byte order."
[^std-ne]: Rust std 1.95.0, primitive u32, to_ne_bytes — <https://doc.rust-lang.org/1.95.0/std/primitive.u32.html> — "portable code should use to_be_bytes or to_le_bytes, as appropriate,"
[^ref-twos]: The Rust Reference 1.95.0, Operator expressions — <https://doc.rust-lang.org/1.95.0/reference/expressions/operator-expr.html> — "Remember that signed integers are always represented using two’s complement."
[^ref-neg]: The Rust Reference 1.95.0, Operator expressions, Overflow — <https://doc.rust-lang.org/1.95.0/reference/expressions/operator-expr.html> — "Negation of these most negative values leaves the value unchanged due to two’s complement overflow conventions."
[^ref-noop]: The Rust Reference 1.95.0, Numeric cast — <https://doc.rust-lang.org/1.95.0/reference/expressions/operator-expr.html> — "Casting between two integers of the same size (e.g. i32 -> u32) is a no-op"
[^ref-trunc]: The Rust Reference 1.95.0, Numeric cast — <https://doc.rust-lang.org/1.95.0/reference/expressions/operator-expr.html> — "Casting from a larger integer to a smaller integer (e.g. u32 -> u8) will truncate"
[^ref-ext]: The Rust Reference 1.95.0, Numeric cast — <https://doc.rust-lang.org/1.95.0/reference/expressions/operator-expr.html> — "sign-extend if the source is signed"
[^gb-bias]: Goldberg, What Every Computer Scientist Should Know About Floating-Point Arithmetic, Formats — <https://docs.oracle.com/cd/E19957-01/806-3568/ncg_goldberg.html> — "In the case of single precision, where the exponent is stored in 8 bits, the bias is 127 (for double precision it is 1023)."
[^gb-zero]: Goldberg, Signed Zero — <https://docs.oracle.com/cd/E19957-01/806-3568/ncg_goldberg.html> — "Since the sign bit can take on two different values, there are two zeros, +0 and -0."
[^gb-zerocmp]: Goldberg, Signed Zero — <https://docs.oracle.com/cd/E19957-01/806-3568/ncg_goldberg.html> — "Thus the IEEE standard defines comparison so that +0 = -0, rather than -0 < +0."
[^gb-naneq]: Goldberg, Languages and Compilers — <https://docs.oracle.com/cd/E19957-01/806-3568/ncg_goldberg.html> — "a NaN is never equal to any other number (including another NaN)"
[^gb-01]: Goldberg, Relative Error and Ulps — <https://docs.oracle.com/cd/E19957-01/806-3568/ncg_goldberg.html> — "Although it has a finite decimal representation, in binary it has an infinite repeating representation."
[^gb-round]: Goldberg, Operations — <https://docs.oracle.com/cd/E19957-01/806-3568/ncg_goldberg.html> — "the result must be computed exactly and then rounded to the nearest floating-point number (using round to even)."
[^std-partialeq]: Rust std 1.95.0, trait PartialEq — <https://doc.rust-lang.org/1.95.0/std/cmp/trait.PartialEq.html> — "numbers NaN != NaN, so floating point types implement PartialEq but not Eq."
