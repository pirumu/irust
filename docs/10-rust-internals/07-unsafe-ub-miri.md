# P10 · Chương 07 — unsafe, undefined behavior và Miri

> Mọi khẳng định về ngôn ngữ theo tài liệu Rust **1.95.0** (Reference, Rustonomicon, Rust Book, `std`). Miri được trích từ README ở một commit ghim cố định. Chương này **không có thí nghiệm**: các đoạn code chỉ để minh họa, không kèm output; chương không khẳng định một chương trình cụ thể "in ra gì" hay "Miri báo đúng dòng nào". Service `ingest` và pool `BufPool` xuyên suốt chương là ví dụ giả định của người soạn, không phải code trong repo.

## Bạn sẽ hiểu được gì

1. **Q1.** `unsafe` thực sự cho phép thêm những việc gì, và nó là "tắt kiểm tra" hay là một hợp đồng? → mục 1
2. **Q2.** Undefined behavior (UB) là gì, vì sao nó nguy hiểm hơn một lỗi thông thường, và danh sách UB của Rust gồm những nhóm nào? → mục 2
3. **Q3.** "Sound" và "unsound" nghĩa là gì; vì sao một dòng code *safe* vẫn có thể làm code *unsafe* sai, và ranh giới module giúp gì? → mục 3
4. **Q4.** Aliasing là gì, vì sao `&mut` không được alias, và `UnsafeCell` đứng ở đâu trong luật này? → mục 4
5. **Q5.** Raw pointer "hợp lệ" nghĩa là gì, và provenance là gì mà một địa chỉ số nguyên không đủ? → mục 5
6. **Q6.** Vì sao không được "tạo biến chưa khởi tạo" và `MaybeUninit<T>` giải quyết việc đó thế nào? → mục 6
7. **Q7.** Miri bắt được gì, chạy thế nào, và nó **không** chứng minh được điều gì? → mục 7

## Kiến thức cần trước

- [Chương 06 — Borrow checker hoạt động thế nào](06-borrow-checker.md): borrow checker chỉ kiểm quy tắc mượn của *reference* (kể cả reference nằm trong khối `unsafe`); raw pointer nằm ngoài tầm của nó, như mục 8 của chương đó nói rõ. Chương này đi vào phần compiler không chứng minh được, tức phần người viết phải tự chịu trách nhiệm.
- [Chương 05 — Subtyping, variance và drop check](05-variance-va-dropck.md): mục 2–4 cho thấy `&mut T` và `UnsafeCell<T>` đều invariant theo `T`; mục 4 ở đây nhìn lại hai kiểu đó từ góc aliasing. Mục 7 của chương đó có một ví dụ `unsafe impl` thật (`#[may_dangle]`).
- [Chương 02 — Type layout: size, alignment, repr, enum và niche](02-type-layout.md): mục 5–6 giải thích niche và vì sao `MaybeUninit` làm mất niche. Mục 2 và mục 6 ở đây dùng cùng khái niệm "giá trị hợp lệ".

Chương [08 — Panic, unwinding, ABI và FFI](08-panic-abi-ffi.md) đọc *sau* chương này: nó dùng định nghĩa UB ở mục 2 để trả lời câu hỏi panic băng qua ranh giới FFI là abort hay UB (mục 6–7 của chương 08). Mục 1, 2, 6 và 7 ở đây có chỗ trỏ sang đó.

## 1. `unsafe` cho phép gì — và nó là hợp đồng (Q1)

> **Trả lời ngắn.** `unsafe` không tắt kiểm tra nào của compiler: quy tắc và ngữ nghĩa của Rust vẫn như cũ[^nomicon-same-rules]. Reference dùng trong khối `unsafe` vẫn bị borrow checker soi[^book-unsafe-checks]. Thứ nó mở thêm là một danh sách ngắn thao tác mà compiler không tự kiểm được, như dereference raw pointer hay gọi hàm `unsafe`[^nomicon-five]. Dùng sai thì hậu quả là UB, không phải lỗi biên dịch[^nomicon-five-why]. Vì vậy chữ `unsafe` đọc đúng nhất như một lời cam kết: "điều kiện mà compiler không kiểm được, tôi đã kiểm rồi"[^nomicon-unsafe-keyword].

**Giải thích.**

*Với service của bạn.* Hãy hình dung `ingest`, một service Rust nhận batch event qua TCP. Mỗi request nó đọc vào một buffer 64 KiB lấy từ `BufPool`, pool buffer do chính team viết để khỏi cấp phát lại cho từng request. Để pool đủ nhanh, tác giả muốn bỏ bounds check ở đường nóng, giữ raw pointer vào vùng nhớ của pool và không zero buffer trước khi dùng. Cả ba việc đều nằm ngoài những gì compiler tự chứng minh được (các mục sau sẽ nói rõ vì sao). Ta sẽ bám theo `BufPool` suốt chương (nhận xét của người soạn: ví dụ giả định). Câu hỏi của mục này: viết `unsafe` trong `BufPool` thì compiler thôi kiểm điều gì, và phần còn lại là trách nhiệm của ai?

### 1.1 Năm việc `unsafe` mở khóa

Nomicon liệt kê đúng năm việc khác nhau giữa Unsafe Rust và Safe Rust: dereference raw pointer; gọi hàm `unsafe` (kể cả hàm C, intrinsic của compiler, raw allocator); implement `unsafe trait`; truy cập hoặc sửa `static mut`; truy cập field của `union`[^nomicon-five]. Năm việc này bị tách riêng vì dùng sai bất kỳ việc nào cũng dẫn tới Undefined Behavior[^nomicon-five-why]. Với `BufPool`, việc giữ pointer vào vùng nhớ rồi đọc qua nó nằm ngay đầu danh sách. Danh sách trên là của Nomicon; Reference liệt kê chi tiết hơn (xem phần Đi sâu cuối mục).

Ngoài năm việc đó, `unsafe` không đổi gì. Một đoạn code Rust vẫn sai nếu nó có bất kỳ hành vi nào trong danh sách UB, kể cả khi nằm trong khối hay hàm `unsafe`[^ref-ub-incorrect]. Reference nói gọn: `unsafe` chỉ có nghĩa là việc tránh UB thuộc về lập trình viên, còn chương trình Rust vẫn không bao giờ được gây UB[^ref-ub-intro].

### 1.2 Hợp đồng: một bên đặt, một bên ký

Từ khóa `unsafe` làm hai việc: khai báo rằng có một hợp đồng mà compiler không kiểm được, và khai báo rằng lập trình viên đã kiểm hợp đồng đó được giữ[^nomicon-unsafe-keyword]. Hai việc này rơi về hai phía, tùy `unsafe` đứng ở đâu.

Đứng trước `fn` hoặc `trait`, `unsafe` *đặt* hợp đồng. Người gọi hàm phải đọc tài liệu của hàm để dùng đúng cách; người implement trait phải đọc tài liệu của trait và giữ đúng điều khoản[^nomicon-unsafe-contract]. Đứng trước một khối hay một `impl`, `unsafe` *ký* hợp đồng. Khối `unsafe {}` tuyên bố mọi thao tác unsafe bên trong đã được kiểm và giữ đúng hợp đồng của chúng[^nomicon-block-keyword]. Còn `unsafe impl` tuyên bố bản implement giữ đúng hợp đồng của trait[^nomicon-impl-keyword]. (Cách gọi "đặt" và "ký" là của người soạn.)

Trong `BufPool`, ví dụ: `unsafe fn slot_unchecked(&self, i: usize)` đặt hợp đồng "`i` phải nhỏ hơn số slot". Mỗi chỗ gọi nó nằm trong một khối `unsafe {}`, và khối đó là chữ ký rằng điều kiện ấy đúng *ở đúng chỗ này* (nhận xét của người soạn).

### 1.3 Hệ quả khi review

Chỉ khối `unsafe` mới ký được hợp đồng, nên `grep unsafe` trong code của `ingest` đã khoanh được các chỗ cần soi kỹ (nhận xét của người soạn). Nomicon còn nhắc rằng có thể thêm `#![forbid(unsafe_code)]` để bảo đảm tĩnh rằng code base chỉ chứa Safe Rust[^nomicon-forbid]. Lint này chỉ áp dụng cho crate đặt nó, không phủ dependency (nhận xét của người soạn).

<details><summary>Đi sâu: danh sách thao tác unsafe theo Reference, <code>extern</code>, <code>static mut</code></summary>

- Reference định nghĩa *unsafe operations* là những thao tác có thể vi phạm bảo đảm memory-safety trong ngữ nghĩa tĩnh của Rust[^ref-unsafe-intro]. Nó cũng liệt kê các tính năng ngôn ngữ không dùng được trong tập con safe[^ref-unsafe-list]. Ba mục đầu trùng danh sách của Nomicon: dereference raw pointer, gọi hàm `unsafe`, implement `unsafe trait`[^nomicon-five]. Hai mục kế cũng trùng, với chi tiết hơn. Một là đọc hoặc ghi `static mut` hay `static` ngoại (khai báo trong khối `extern`) loại unsafe[^ref-unsafe-static]. Hai là truy cập field `union` trừ khi chỉ để gán[^ref-unsafe-ops]. Còn lại là những mục chỉ Reference có. Một là gọi hàm safe có `target_feature` từ hàm không bật cùng feature[^ref-unsafe-tf]. Hai là áp dụng attribute `unsafe` cho item[^ref-unsafe-attr]. Ba là khai báo khối `extern` (xem bullet kế). Đó là chỗ danh sách của Reference dài hơn danh sách năm mục của Nomicon (suy luận của người soạn).
- Khai báo khối `extern` nằm trong danh sách vì từ khóa `unsafe` về ngữ nghĩa là bắt buộc trước `extern` trên khối[^ref-ext-unsafe-required]. Trước edition 2024, khối `extern` vẫn được phép khai báo không cần `unsafe`[^ref-unsafe-extern-ed]. Cú pháp khối `extern` và qualifier `safe`/`unsafe` cho từng item được nói ở [Chương 08, mục 4](08-panic-abi-ffi.md).
- Tài liệu `std` nói hàm và static khai báo trong khối `extern` mặc định là `unsafe` (trừ hàm khai báo kiểu `extern "something" fn ...`), còn static có thể sửa thì luôn unsafe, khai báo ở đâu cũng vậy[^kw-extern-static].
- Nomicon nhấn mạnh mọi hàm FFI đều unsafe để gọi, vì ngôn ngữ kia có thể làm bất cứ điều gì mà compiler Rust không kiểm được[^nomicon-ffi-unsafe]. [Chương 08](08-panic-abi-ffi.md) đi tiếp vào ABI, kiểu dữ liệu và panic qua ranh giới đó.
- Hợp đồng không chỉ là "tránh UB trực tiếp": hàm unsafe được tự khai báo thêm ràng buộc riêng. Ví dụ API cấp phát nói giải phóng vùng nhớ chưa được cấp phát là UB[^nomicon-alloc-ub].

</details>

**Quay lại Q1:** `unsafe` mở khóa một danh sách thao tác nhỏ và chuyển trách nhiệm chứng minh từ compiler sang người viết; nó không tắt kiểm tra và không làm UB trở nên được phép.

## 2. Undefined behavior là gì (Q2)

> **Trả lời ngắn.** UB là hành vi mà chương trình Rust không bao giờ được gây ra[^ref-ub-intro]. Chương trình đã có UB thì compiler được toàn quyền làm những điều tệ tùy ý với nó[^nomicon-fullrights]. Ảnh hưởng đó lan ra *toàn bộ* chương trình chứ không dừng ở dòng gây lỗi[^ref-ub-whole]. Reference liệt kê các nhóm UB chính (data race, truy cập con trỏ dangling hay lệch alignment, phá luật aliasing, tạo giá trị không hợp lệ, gọi sai ABI), nhưng nói rõ danh sách đó chưa đầy đủ[^ref-ub-notexh].

**Giải thích.**

*Với `ingest`.* Giả sử `BufPool` có một bug: handler trả buffer về pool, buffer bị giải phóng, vậy mà một handler khác vẫn đọc qua pointer cũ (use-after-free; mục 5 sẽ nói vì sao đây là UB). Nếu đây chỉ là lỗi logic thông thường, ta thấy dữ liệu lẫn lộn ở một request, lần theo log là ra chỗ sai. UB thì khác: hậu quả không nằm lại ở dòng đọc lỗi. Mục này giải thích vì sao, rồi liệt kê UB gồm những gì.

### 2.1 Vì sao compiler được "làm bất cứ gì"

Nomicon nói gây UB trao cho compiler "toàn quyền" làm những điều tệ tùy ý với chương trình của bạn[^nomicon-fullrights]. Phạm vi của "toàn quyền" này không dừng ở một dòng, cũng không dừng ở Rust. Reference nhấn mạnh UB ảnh hưởng toàn chương trình[^ref-ub-whole]. Ví dụ, gọi một hàm C có UB (theo luật của C) nghĩa là cả chương trình có UB, gồm cả phần Rust; ngược lại, UB trong Rust có thể làm hỏng code ở ngôn ngữ khác được gọi qua FFI[^ref-ub-ffi].

Lý do thực tế là compiler biên dịch cả chương trình với giả định UB không bao giờ xảy ra, và tối ưu *dựa trên* giả định đó (suy luận của người soạn). Mục 4 cho thấy cơ chế này qua ví dụ aliasing, còn mục 6 cho thấy compiler khai thác bất biến "biến đã khởi tạo đúng kiểu" ra sao.

### 2.2 UB của Rust hẹp hơn C nhưng không biến mất

Nomicon nói UB trong Rust có phạm vi giới hạn hơn C[^nomicon-ub-short]. Những thứ core language quan tâm gồm: dereference con trỏ dangling hoặc lệch alignment, phá luật aliasing, gọi sai ABI hoặc unwind khỏi hàm có ABI unwind sai, data race, chạy code biên dịch với target feature mà máy không hỗ trợ, và tạo giá trị không hợp lệ[^nomicon-ub-list].

Reference có danh sách chi tiết hơn và tự nhận là không đầy đủ: danh sách có thể mở rộng hoặc thu hẹp[^ref-ub-notexh]. Hiện chưa có mô hình hình thức nào của ngữ nghĩa Rust cho unsafe code, nên có thể còn hành vi khác cũng bị coi là UB[^ref-ub-nomodel]. Reference cũng bảo lưu quyền biến một số mục trong danh sách thành "có định nghĩa" trong tương lai[^ref-ub-reserve].

### 2.3 Các nhóm chính

Dưới đây là phần trọng tâm của phase này, theo Reference, kèm điều nó nghĩa với `BufPool`:

- **Data race.** Nomicon định nghĩa: hai luồng cùng truy cập một vùng nhớ, ít nhất một ghi, ít nhất một không đồng bộ. Data race là UB và vì thế không thể xảy ra trong Safe Rust[^nomicon-race]. Hai handler thread cùng ghi vào một buffer mà không đồng bộ là ví dụ. (Định nghĩa chuẩn dùng xuyên suốt giáo trình, và cách đặt hai cách phát biểu cạnh nhau, ở [P4·06 mục 3](../04-concurrency/06-memory-model.md).)
- **Truy cập place dangling hoặc dựa trên pointer lệch alignment**, đọc hay ghi[^ref-ub-access]. Đây là nhóm chứa use-after-free ở ví dụ đầu mục.
- **Phá luật aliasing**[^ref-ub-alias]. Mục 4 nói riêng về nhóm này.
- **Gọi hàm sai ABI, hoặc unwind qua frame không cho phép unwind**[^ref-ub-call]. Chi tiết (và một điểm dễ hiểu nhầm) nằm ở phần Đi sâu cuối mục.
- **Tạo giá trị không hợp lệ.** "Tạo" (produce) một giá trị xảy ra mỗi khi giá trị được gán hoặc đọc từ một place, truyền hay trả về từ hàm hoặc phép toán nguyên thủy[^ref-ub-invalid]. Compiler giả định mọi giá trị sinh ra khi chạy đều hợp lệ, nên tạo giá trị không hợp lệ là UB *ngay lập tức*[^ref-ub-valid-assume].

```mermaid
flowchart TD
  S["Handler của ingest (Safe Rust)<br/>(compiler chứng minh)"] -->|"gọi qua API"| W["BufPool: API safe bao quanh unsafe<br/>(tác giả chứng minh bằng lý lẽ + test + Miri)"]
  W -->|"unsafe block"| U["thao tác unsafe<br/>(deref raw ptr, FFI, union, ...)"]
  U -.->|"vi phạm hợp đồng"| UB["UB: compiler 'full rights'"]
```

(Sơ đồ là tóm tắt của người soạn; các mũi tên khớp với phần trích ở mục 1 và 3.)

<details><summary>Đi sâu: bảng "giá trị hợp lệ" theo kiểu, dangling, misaligned, unwind qua ranh giới</summary>

- Đừng suy ra rằng *mọi* panic chạm một hàm `extern "C"` đều là UB. Khi một thao tác unwind chạm ranh giới không cho unwind, Nomicon phân biệt panic của Rust (process abort an toàn) với exception ngoại lai đi vào Rust (UB)[^nomicon-ffi-boundary]. Bảng hành vi đầy đủ và cách đối chiếu hai điều này nằm ở [Chương 08, mục 6](08-panic-abi-ffi.md).
- Reference định nghĩa hợp lệ theo từng kiểu. `bool` phải là 0 hoặc 1[^ref-ub-bool]. Số nguyên, số thực và raw pointer không được lấy từ bộ nhớ chưa khởi tạo[^ref-ub-scalar]. Enum phải có discriminant hợp lệ, và mọi field của variant tương ứng cũng hợp lệ[^ref-ub-enum]. Kiểu có dải giá trị hợp lệ tùy biến thì giá trị phải nằm trong dải; trong standard library điều này áp dụng cho `NonNull<T>` và `NonZero<T>`[^ref-ub-validrange]. [Chương 02, mục 5](02-type-layout.md) dùng chính các điều kiện này để giải thích niche.
- Tham chiếu (`&`, `&mut`) và `Box<T>` phải căn đúng, khác null, không dangling và trỏ tới giá trị hợp lệ[^ref-ub-ref]. Riêng vế cuối, "trỏ tới giá trị hợp lệ", Reference ghi chú là vẫn còn tranh luận[^ref-ub-ref-debate].
- Một reference hay pointer là "dangling" nếu không phải mọi byte nó trỏ tới đều thuộc cùng một allocation còn sống[^ref-ub-dangling]. Nếu kích thước vùng đó là 0 thì pointer không bao giờ dangling, kể cả pointer null[^ref-ub-zero]. Trường hợp kích thước 0 gặp lại ở kiểu ZST: xem [Chương 02, mục 7](02-type-layout.md).
- Với kiểu DST (slice, string), pointer trỏ tới *toàn bộ* phạm vi nên metadata độ dài không được quá lớn[^ref-ub-dst-len]. Kích thước động của một giá trị cũng không được vượt `isize::MAX`[^ref-ub-isize]. (Fat pointer và metadata: [Chương 03, mục 2](03-dst-fat-pointer-vtable.md).)
- Về alignment: một place dựa trên pointer lệch chỉ gây UB khi nó được *load* hoặc *store*[^ref-ub-misaligned-access]. Viết `&raw const` hay `&raw mut` trên place đó vẫn được phép[^ref-ub-misaligned-raw].
- Vì tạo giá trị không hợp lệ là UB ngay lúc tạo[^ref-ub-invalid], tham chiếu và `Box` phải căn đúng *ngay khi được tạo*, không chỉ khi dùng. Reference ghi đúng điều này: `&`/`&mut` trên một place đòi alignment của kiểu field, nếu không chương trình đang "tạo giá trị không hợp lệ"[^ref-ub-misaligned-ref].
- Nomicon nhắc rằng vi phạm các ràng buộc do unsafe function tự khai báo thường dẫn tới một trong các UB ở trên theo dây chuyền[^nomicon-ub-transitive].

</details>

**Quay lại Q2:** UB không phải "một lỗi tại một dòng" mà là việc phá giả định mà compiler dùng để biên dịch cả chương trình; vì thế phải tránh nó, không chỉ "hy vọng nó không chạy tới".

## 3. Sound, unsound và ranh giới module (Q3)

> **Trả lời ngắn.** Code unsafe là *sound* nếu mọi client safe, viết thế nào cũng không kích hoạt được UB qua nó; chỉ cần một client safe làm được thì nó *unsound*[^ref-sound]. Tính an toàn là **không cục bộ**: sửa một dòng safe có thể làm một khối `unsafe` ở chỗ khác thành sai[^nomicon-wwu-nonlocal]. Cách chắc chắn nhất để khoanh vùng là dùng privacy ở ranh giới module[^nomicon-wwu-privacy].

**Giải thích.**

*Với `ingest`.* `BufPool` giữ một pointer tới vùng nhớ lớn và số slot trong vùng đó. Code unsafe trong `BufPool::get` đúng *với điều kiện* hai thứ này luôn khớp nhau. Nếu một handler (code safe, nằm ở module khác) ghi bừa được vào số slot, đoạn unsafe kia hỏng mà không ai đổi một chữ của nó. Muốn giữ đúng thì phải giấu số slot đi. Mục này nói vì sao chuyện đó đúng nói chung, và giấu bằng cách nào.

### 3.1 Soundness là tính chất của Safe Rust

Nomicon gọi đây là *soundness property*: dù thế nào, Safe Rust cũng không thể gây UB[^nomicon-soundness]. Tính chất đó dựa trên một quan hệ tin cậy lệch một chiều. Safe Rust phải tin rằng Unsafe Rust mà nó chạm vào đã được viết đúng[^nomicon-asym]. Ngược lại, Unsafe Rust không thể tin Safe Rust nếu thiếu cẩn trọng[^nomicon-unsafe-cant-trust]. Với `ingest`: handler tin `BufPool` đúng, nhưng `BufPool` không được tin rằng handler luôn gọi nó đúng cách.

Ví dụ kinh điển là `BTreeMap` và `Ord`. `BTreeMap` cần key implement `Ord`, nhưng một `Ord` sai vẫn viết được hoàn toàn bằng Safe Rust, mà Safe Rust thì không được phép gây UB. Vì vậy code unsafe trong `BTreeMap` phải chịu được cả những `Ord` không thực sự là total order[^nomicon-btree-robust]. Kết quả: `BTreeMap` có thể hành xử thất thường nếu key không có thứ tự toàn phần, nhưng không bao giờ gây UB[^nomicon-btree-erratic]. Đây là ranh giới giữa *logic sai* (hành vi lạ nhưng vẫn memory-safe) và *UB* (compiler mất giả định nền) (nhận xét của người soạn).

### 3.2 An toàn không cục bộ

Nomicon đưa một hàm `index` kiểm `idx < arr.len()` rồi mới gọi `get_unchecked`; hàm này sound[^nomicon-wwu-sound]. Chỉ đổi `<` thành `<=`, một thay đổi nằm hoàn toàn trong phần safe, là hàm đã unsound[^nomicon-wwu-unsound]. Nomicon gọi đây là vấn đề nền tảng của safety: nó không cục bộ[^nomicon-wwu-nonlocal]. Lý do là độ đúng của thao tác unsafe phụ thuộc vào trạng thái do các thao tác "safe" khác thiết lập[^nomicon-wwu-state].

```rust
// Minh họa theo Nomicon "Working with Unsafe": sound hay không tùy một ký tự.
fn index(idx: usize, arr: &[u8]) -> Option<u8> {
    if idx < arr.len() {                 // đổi thành `<=` là unsound
        unsafe { Some(*arr.get_unchecked(idx)) }
    } else {
        None
    }
}
```

Nếu `BufPool::get` viết theo kiểu này, một commit "chỉ sửa một ký tự" khi review cũng đủ biến nó thành unsound (nhận xét của người soạn).

Bất biến của struct cũng chịu cùng luật. Xét một `Vec` ngây thơ gồm `ptr`, `len`, `cap`: `push` của nó đúng chỉ khi `cap` phản ánh đúng vùng nhớ đã cấp[^nomicon-wwu-invariant]. Giờ thêm một phương thức `make_room` chỉ làm `self.cap += 1`. Code đó vẫn là 100% Safe Rust nhưng hoàn toàn unsound[^nomicon-wwu-makeroom]. Hệ quả: unsafe code dựa vào bất biến của field thì không chỉ làm ô nhiễm một hàm mà cả module[^nomicon-wwu-pollute]. Trong `BufPool`, một phương thức safe `fn set_slots(&mut self, n: usize) { self.slots = n }` hỏng theo đúng cách đó (nhận xét của người soạn).

### 3.3 Privacy là công cụ khoanh vùng

Nomicon nói: nhìn chung, cách đáng tin cậy duy nhất để giới hạn phạm vi của unsafe code là ở ranh giới module, bằng privacy[^nomicon-wwu-privacy]. `make_room` không phải vấn đề nếu nó không `pub`, vì chỉ module định nghĩa nó gọi được[^nomicon-wwu-private-ok]. Nó đụng trực tiếp vào field private của `Vec` nên chỉ viết được trong cùng module với `Vec`[^nomicon-wwu-private-access]. Nhờ vậy ta viết được một abstraction hoàn toàn safe dựa trên bất biến phức tạp[^nomicon-wwu-abstraction].

```rust
// Minh họa (nhận xét của người soạn), không phải code trong repo.
mod bufpool {
    pub struct BufPool {
        base: *mut u8, // trỏ tới vùng nhớ đủ cho `slots` buffer 64 KiB
        slots: usize,  // bất biến: `base` và `slots` luôn khớp nhau
    }

    impl BufPool {
        pub fn len(&self) -> usize { self.slots }
        // ... các hàm unsafe bên trong dựa vào bất biến trên
    }
}
```

Handler của `ingest` nằm ngoài `mod bufpool`, nên chỉ gọi được API `pub` chứ không chạm được `base` và `slots`:

```mermaid
flowchart LR
  subgraph M["module bufpool (ranh giới privacy)"]
    P["field private: base, slots"]
    U["unsafe code dựa trên bất biến"]
    A["API pub: lease, release, len..."]
    P --> U --> A
  end
  C["handler safe bên ngoài"] -->|"chỉ gọi API pub"| A
  C -.->|"không với tới field private"| P
```

(Sơ đồ là tóm tắt của người soạn từ các câu trích về privacy ở trên.)

<details><summary>Đi sâu: giữ ranh giới trong thực hành, thư viện chuẩn, "an toàn nhưng sai"</summary>

- Nomicon nói thư viện chuẩn dùng Unsafe Rust nhiều ở bên trong. Các cài đặt đó thường được kiểm tra thủ công rất kỹ, nên giao diện Safe Rust xây trên chúng có thể coi là an toàn[^nomicon-std-unsafe]. (Nhận xét của người soạn: đây là *quy trình*, con người xem xét, không phải chứng minh tự động; vì vậy Miri vẫn hữu ích.)
- Tài liệu `std` (từ khóa `unsafe`) cho thấy `unsafe fn` "đẩy" việc kiểm tra cho *người gọi* qua mục `# Safety`. Ví dụ `Vec::set_len` yêu cầu `new_len <= capacity()`[^kw-set-len]. Nó cũng yêu cầu các phần tử trong `old_len..new_len` đã khởi tạo[^kw-set-len-init]. (Xem mục 8.)
- Câu của Nomicon ở trên ("chỉ viết được trong cùng module") là cách nói gọn. Reference chính xác hơn: item private truy cập được từ module hiện tại *và các module con của nó*[^ref-vis-private]. Vì vậy ranh giới tin cậy thực ra là cả cây module bên dưới, không chỉ một file (suy luận của người soạn).
- Reference định nghĩa `sound`/`unsound` theo "mọi client safe"[^ref-sound]. Theo định nghĩa đó, một thư viện có chứa `unsafe` vẫn có thể sound, còn một hàm *không* chứa `unsafe`, như `make_room`, vẫn có thể làm thư viện unsound vì nó phá bất biến mà code `unsafe` ở chỗ khác dựa vào (nhận xét của người soạn, nối từ ví dụ `make_room`).

</details>

**Quay lại Q3:** sound nghĩa là "client safe nào cũng không gây UB được"; vì unsafe phụ thuộc vào bất biến, ta phải đóng gói chúng sau privacy ở ranh giới module.

## 4. Aliasing và luật của `&T`, `&mut T` (Q4)

> **Trả lời ngắn.** Hai thứ *alias* nhau khi chúng trỏ tới những vùng nhớ chồng lên nhau[^nomicon-alias-def]. Đó là cách hiểu rộng nhất mà Nomicon dùng để bàn luận; định nghĩa của Rust nhiều khả năng hẹp hơn[^nomicon-alias-broad]. Rust cho compiler giả định `&mut` không bị alias[^nomicon-alias-correct], và nhờ vậy nó tối ưu được mạnh tay. Unsafe code phá giả định đó là UB[^ref-ub-alias]. Luật chính xác thì chưa được chốt, Reference mới chỉ phác thảo nguyên tắc[^ref-ub-alias-open].

**Giải thích.**

*Với `ingest`.* `BufPool` đưa cho handler một `&mut [u8]` trỏ vào một slot để đọc request vào đó, trong khi bản thân pool vẫn giữ `base`, pointer gốc của cả vùng nhớ. Nếu lúc handler còn dùng `&mut` mà pool đọc cùng slot qua `base` (chẳng hạn để thống kê), hai đường truy cập đang alias nhau. Mục này nói vì sao compiler quan tâm tới điều đó và luật chính xác là gì.

### 4.1 Vì sao compiler quan tâm

Nomicon lấy hàm `compute(input: &u32, output: &mut u32)` làm ví dụ. Compiler muốn đọc `*input` một lần vào register thay vì đọc lại[^nomicon-alias-cache]. Việc đó chỉ an toàn nếu `input` và `output` không chồng nhau; cụ thể, ta phải lo những lời gọi làm chúng chồng nhau, như `compute(&x, &mut x)`[^nomicon-alias-compute]. Trong Rust, compiler loại được trường hợp này vì `&mut` không được phép bị alias[^nomicon-alias-correct]. Với hầu hết ngôn ngữ khác, tối ưu này không hợp lệ nếu thiếu phân tích toàn cục[^nomicon-alias-sound-opt]. Nomicon liệt kê các tối ưu mà phân tích alias mở khóa, ví dụ giữ giá trị trong register nhờ chứng minh không pointer nào khác chạm vào vùng nhớ[^nomicon-alias-opts].

Áp vào `ingest`: compiler có quyền đọc `buf[0]` một lần rồi dùng lại giá trị, vì nó tin không ai khác ghi vào `buf` giữa hai lần đọc. Nếu pool ghi vào slot qua `base` đúng lúc đó, chương trình đã phá giả định ấy (suy luận của người soạn từ ví dụ trên).

### 4.2 Luật theo Reference

Reference viết luật thành hai vế. Một `&T` phải trỏ tới vùng nhớ không bị mutate khi reference còn live (ngoại lệ: dữ liệu bên trong `UnsafeCell`). Một `&mut T` phải trỏ tới vùng nhớ không bị đọc hoặc ghi bởi bất kỳ pointer nào *không dẫn xuất từ nó*, và không reference nào khác trỏ tới vùng nhớ đó khi nó còn live[^ref-ub-alias]. `Box<T>` được coi tương tự `&'static mut T` cho mục đích các luật này[^ref-ub-box].

Bất biến của `&T` đi xa hơn chỉ một reference. Các byte mà một shared reference trỏ tới, kể cả gián tiếp qua reference và `Box` khác, đều là bất biến[^ref-ub-immutable-shared]. Mutation là bất kỳ lần ghi nào dài hơn 0 byte mà chồng với các byte liên quan, *kể cả khi nó không đổi nội dung*[^ref-ub-immutable]. Tài liệu `UnsafeCell` nói thêm: mutate dữ liệu sau `&T`, ví dụ qua alias hoặc bằng cách transmute `&T` thành `&mut T`, là UB[^uc-transmute].

### 4.3 Luật chính xác chưa được chốt

Reference nói thẳng đây là phác thảo nguyên tắc chung, chưa phải luật cuối cùng[^ref-ub-alias-open]. Tài liệu `std::ptr` cũng vậy: aliasing rules chưa được quyết định nên chỉ có tổng quan sơ lược[^ptr-alias-undecided]. Miri kiểm tra vi phạm luật này theo hai mô hình. Mô hình thứ nhất là Stacked Borrows, được README đánh dấu **experimental**[^miri-sb]. Mô hình thứ hai là Tree Borrows, cũng experimental và được nêu như lựa chọn thay thế tùy chọn cho Stacked Borrows[^miri-tb]. Chương này không mô tả nội bộ của hai mô hình vì chưa ghim nguồn đặc tả (nhận xét của người soạn).

### 4.4 `UnsafeCell`: ngoại lệ có kiểm soát

Nếu `&T` luôn bất biến thì interior mutability (như `Cell`, `RefCell`) dựa vào đâu? Câu trả lời là `UnsafeCell<T>`: nó opt-out khỏi bảo đảm bất biến của `&T`, tức một `&UnsafeCell<T>` có thể trỏ tới dữ liệu đang bị mutate. Đó là *interior mutability*[^uc-optout]. `Cell<T>`, `RefCell<T>` và mọi kiểu khác có interior mutability đều dùng `UnsafeCell` để bọc dữ liệu bên trong[^uc-others].

Nhưng `UnsafeCell` chỉ nới một vế của luật. Chỉ bảo đảm bất biến của shared reference bị ảnh hưởng; bảo đảm tính duy nhất (uniqueness) của `&mut` thì không[^uc-unique]. Nói cách khác, không có cách hợp lệ nào để có `&mut` alias, kể cả với `UnsafeCell<T>`[^uc-mutref]. `UnsafeCell` cũng không chống data race[^uc-race]. (Suy luận của người soạn: theo câu "mọi kiểu khác có interior mutability" ở trên, `Mutex` và atomic cũng dựa vào `UnsafeCell`; Phase 4 xây cơ chế đồng bộ *trên* nó, xem [P4 · Chương 08, mục 5–6](../04-concurrency/08-rust-concurrency.md).)

```mermaid
flowchart TD
  Q["reference sống: &T hay &mut T?"]
  Q -->|"&T"| S["vùng nhớ không bị mutate<br/>(trừ trong UnsafeCell)"]
  Q -->|"&mut T"| X["không ai khác đọc/ghi<br/>trừ pointer dẫn xuất từ nó"]
  S -.->|"vi phạm"| UB["UB"]
  X -.->|"vi phạm"| UB
```

(Sơ đồ là tóm tắt của người soạn từ luật của Reference ở trên[^ref-ub-alias].)

<details><summary>Đi sâu: "live" là bao lâu, tạo reference từ raw pointer, <code>UnsafeCell::get</code></summary>

- "Live" không có định nghĩa chính xác nhưng có vài cận[^ref-ub-live-unspec]. Mỗi lần reference hoặc box bị dereference hay reborrow thì nó được coi là live[^ref-ub-live]. Một reference (không phải `Box`) truyền vào hàm thì live ít nhất trong suốt lời gọi, trừ khi `&T` chứa `UnsafeCell`[^ref-ub-live-fn]. Thời gian live của reference bị chặn trên bởi lifetime cú pháp mà borrow checker gán[^ref-ub-live-bound].
- `std::ptr` nói khi đổi pointer thành reference (`&*ptr`, `&mut *ptr`) có một danh sách quy tắc phải theo: pointer căn đúng, khác null, "dereferenceable", trỏ tới giá trị hợp lệ của `T`, và bạn phải tự bảo đảm luật aliasing của Rust[^ptr-ptr2ref]. Khi *tạo* `&mut`, suốt thời gian nó tồn tại vùng nhớ không được truy cập bởi pointer hay reference nào không dẫn xuất từ nó[^ptr-ptr2ref-mut]. Khi tạo `&`, vùng nhớ không được mutate (trừ trong `UnsafeCell`)[^ptr-ptr2ref-alias].
- Chiều ngược lại: kết quả của việc đổi *reference* thành pointer hợp lệ chừng nào allocation còn sống và *không* có reference nào (chỉ raw pointer) được dùng để truy cập cùng vùng nhớ[^ptr-ref-cast]. Nói cách khác, truy cập qua reference và qua pointer không được đan xen[^ptr-ref-interleave].
- Với `UnsafeCell`, cách duy nhất hợp lệ để lấy `*mut T` từ một `UnsafeCell<T>` được chia sẻ là `.get()` hoặc `.raw_get()`[^uc-get]. Dù `T` và `UnsafeCell<T>` cùng layout, "ép" trực tiếp là không được phép và là UB[^uc-get-cast].
- Chi tiết "UB nằm ở đâu theo thời gian" (khi nào một reference bị coi là "chết" và pointer dẫn xuất từ nó còn dùng được hay không) nhiều khả năng thuộc các mô hình như Stacked/Tree Borrows (suy luận của người soạn). Chương này không trình bày vì không ghim nguồn đặc tả.

</details>

**Quay lại Q4:** aliasing quan trọng vì nó cho compiler giả định để tối ưu; `&mut` luôn độc quyền và `&T` bất biến (trừ `UnsafeCell`), còn luật chi tiết vẫn đang được chốt.

## 5. Raw pointer, validity và provenance (Q5)

> **Trả lời ngắn.** Raw pointer là pointer không có bảo đảm an toàn hay liveness[^ref-raw]. Dereference nó là thao tác unsafe[^ref-raw-deref]. Một raw pointer "hợp lệ" chỉ có nghĩa *cho một truy cập cụ thể*[^ptr-valid]. Về mặt ngữ nghĩa nó không chỉ là một địa chỉ mà còn mang **provenance**: thông tin nó được dẫn xuất từ allocation nào và được làm gì[^ptr-prov].

**Giải thích.**

*Với `ingest`.* `BufPool` giữ `base: *mut u8` trỏ tới một vùng nhớ lớn và chia nó thành các slot 64 KiB. Có hai câu hỏi tự nhiên. Pointer `base.add(i * 65536)` có dùng được để đọc đủ 64 KiB không? Và handler còn giữ pointer vào một slot sau khi vùng nhớ đã bị giải phóng thì sao? Cả hai câu trả lời nằm ở validity và provenance.

### 5.1 Raw pointer là gì

Raw pointer tạo được trực tiếp bằng `&raw const` (cho `*const`) và `&raw mut` (cho `*mut`)[^ref-raw-new]. (Nhận xét của người soạn: tạo raw pointer bằng ép kiểu hay `&raw` không cần `unsafe`; còn các hàm như `ptr.add`/`ptr.read` có hợp đồng riêng và là `unsafe fn`, xem mục 8.)

### 5.2 Hợp lệ theo từng truy cập

`std::ptr` nói hợp lệ phụ thuộc vào thao tác (đọc hay ghi) và độ lớn vùng nhớ được truy cập. Hỏi "pointer này có hợp lệ không" là hỏi chưa đủ; phải hỏi "nó có hợp lệ cho truy cập *này* không"[^ptr-valid]. Quy tắc chính xác của validity cũng chưa được xác định, chỉ có vài bảo đảm tối thiểu[^ptr-valid-open]:

- Với truy cập kích thước 0, mọi pointer đều hợp lệ, kể cả null[^ptr-zero].
- Pointer null không bao giờ hợp lệ cho truy cập khác 0[^ptr-null].
- Pointer phải *dereferenceable*, một điều kiện cần nhưng chưa đủ để hợp lệ[^ptr-deref-nec].
- Dereferenceable nghĩa là đoạn nhớ cần truy cập nằm hoàn toàn trong biên của allocation mà pointer được dẫn xuất từ đó[^ptr-deref-def].
- Mỗi biến cục bộ trên stack được coi là một allocation riêng[^ptr-deref].

Với `base.add(i * 65536)`: đoạn 64 KiB đó phải nằm gọn trong allocation mà `base` dẫn xuất ra, nếu không thì pointer không hợp lệ cho việc đọc 64 KiB (nhận xét của người soạn, áp dụng điều kiện dereferenceable).

### 5.3 Vì sao cần provenance

Để giải thích những ràng buộc như vậy, pointer phải "nhiều hơn địa chỉ của nó": chúng phải có provenance[^ptr-prov]. Cấu trúc chính xác của provenance chưa được đặc tả, nhưng quyền của một pointer có ba thành phần: *không gian* (vùng nhớ nào), *thời gian* (bao lâu) và *khả biến* (đọc hay ghi)[^ptr-prov-parts].

Provenance đi theo quy tắc dẫn xuất. Khi một allocation được tạo, nó có một "Original Pointer" duy nhất[^ptr-original]. Mọi pointer dẫn xuất từ đó qua offset, borrowing hay pointer cast đều kế thừa provenance[^ptr-inherit]. Một số phép dẫn xuất *thu hẹp* quyền; ví dụ borrow một subslice thu hẹp thành phần không gian[^ptr-prov-some-shrink]. Nhưng không phép nào *nới* được quyền: ngay cả khi bạn "biết" có một allocation lớn hơn, bạn không thể dẫn xuất ra pointer có provenance lớn hơn[^ptr-prov-shrink]. Riêng shared reference chỉ có provenance cho phép *đọc*, không bao giờ cho phép ghi, trừ bên trong `UnsafeCell`[^ptr-shared-prov].

```mermaid
flowchart TD
  O["Original Pointer (base)<br/>(toàn bộ vùng nhớ của pool, suốt thời gian sống)"]
  O -->|"offset / &mut v[2..4]"| D["pointer dẫn xuất (một slot)<br/>(chỉ vùng con, thời gian ngắn hơn)"]
  D -->|"không thể nới"| X["✗ không dẫn xuất ra vùng lớn hơn"]
```

(Sơ đồ là tóm tắt của người soạn từ các câu trích ở đoạn trên.)

Hệ quả cho `BufPool` là ba kiểu UB do provenance. Truy cập bộ nhớ qua một pointer không có provenance trên vùng nhớ đó là UB[^ptr-access-prov]. Offset pointer ra ngoài phạm vi allocation nó dẫn xuất từ đó cũng là UB[^ptr-offset-ub]. Còn use-after-free, như handler giữ pointer cũ ở ví dụ đầu mục 2, là UB *ngay cả khi bạn "gặp may"* và vùng nhớ vừa giải phóng được cấp phát lại trước lúc đọc hay ghi[^ptr-uaf].

### 5.4 Pointer và số nguyên

Giả sử `BufPool` lưu địa chỉ slot dưới dạng `usize` để so sánh hoặc ghi log, rồi sau đó đổi ngược thành pointer. `std::ptr` nói `usize` không biểu diễn đúng một pointer: chuyển pointer sang `usize` thường chỉ trích ra địa chỉ, còn chuyển ngược lại đòi hỏi trả lời câu hỏi "provenance nào?"[^ptr-usize]. Cách xử lý của Rust (Strict và Exposed Provenance) nằm ở phần Đi sâu.

<details><summary>Đi sâu: những thứ vẫn sound với pointer, Strict vs Exposed, atomic/volatile</summary>

- Vẫn sound: tạo pointer *không có provenance* từ một địa chỉ[^ptr-without-sound]. Pointer như vậy không dùng được để truy cập bộ nhớ (trừ truy cập kích thước 0)[^ptr-without-prov]. Nó dùng được làm giá trị sentinel, hoặc pointer gắn tag mà không bao giờ dereference[^ptr-sentinel]. So sánh pointer bất kỳ theo địa chỉ cũng vẫn sound[^ptr-compare].
- *Strict Provenance* là một tập API làm provenance tường minh, dùng thay cho việc ép pointer sang số nguyên rồi ngược lại[^ptr-strict]. `std::ptr` khuyến khích chuyển code sang các API này nếu có thể, để tránh phải dùng Exposed Provenance[^ptr-prefer-strict]. Càng nhiều code theo kiểu này càng tốt, vì tránh được độ phức tạp của đặc tả và giúp các công cụ như CHERI và Miri được dùng rộng hơn[^ptr-strict-win].
- *Exposed Provenance* cung cấp qua `expose_provenance` và `with_exposed_provenance`, tương đương phép ép `as` giữa pointer và số nguyên[^ptr-exposed-api]. Ngữ nghĩa của nó kém vững hơn nhiều so với Strict Provenance[^ptr-exposed-fragile]. Theo tài liệu, nó sẽ không hoạt động (tốt) với các công cụ như Miri và CHERI[^ptr-exposed-miri]. Nếu không có provenance đã "exposed" nào biện minh cho cách pointer trả về được dùng thì chương trình có UB[^ptr-exposed-ub].
- Các hàm trong `std::ptr` thực hiện truy cập *không atomic*: hai truy cập đồng thời tới cùng vị trí từ các thread khác nhau là UB trừ khi cả hai chỉ đọc[^ptr-nonatomic]. Điều này bao gồm cả `read_volatile` và `write_volatile`, vốn không dùng được để đồng bộ hóa giữa thread[^ptr-nonatomic-volatile].
- Phần lớn hàm trong `std::ptr` đòi pointer căn đúng, có ngoại lệ như `read_unaligned` và `write_unaligned`[^ptr-align-most]. Đòi hỏi này áp dụng cả với truy cập kích thước 0[^ptr-align].
- Một pointer là "dangling" theo `std::ptr` nếu nó không hợp lệ cho bất kỳ truy cập nào có kích thước khác 0, bao gồm pointer ra ngoài biên, trỏ vào bộ nhớ đã giải phóng, null, hoặc tạo bởi `NonNull::dangling`[^ptr-dangling-def]. (Nhận xét của người soạn: định nghĩa này hơi khác định nghĩa "dangling" của Reference ở mục 2. Reference tính theo allocation còn sống và loại trừ trường hợp kích thước 0, còn `std::ptr` tính theo truy cập kích thước khác 0.)

</details>

**Quay lại Q5:** raw pointer hợp lệ hay không là câu hỏi *theo từng truy cập*, và hợp lệ phụ thuộc vào provenance (vùng nhớ nào, bao lâu, đọc hay ghi) chứ không chỉ vào con số địa chỉ.

## 6. `MaybeUninit<T>` và vì sao "chưa khởi tạo" không phải "giá trị tùy ý" (Q6)

> **Trả lời ngắn.** Compiler giả định mỗi biến luôn được khởi tạo đúng theo yêu cầu của kiểu[^mu-invariant]. Vì vậy tạo một `bool` chưa khởi tạo là UB ngay lúc tạo[^mu-bool]. `MaybeUninit<T>` là kiểu dành riêng để giữ chỗ cho giá trị *có thể chưa khởi tạo*, với cùng size, alignment và ABI như `T`[^mu-layout].

**Giải thích.**

*Với `ingest`.* Mỗi request cần một buffer 64 KiB để đọc từ socket. Zero cả 64 KiB cho từng request là việc thừa nếu `read` sắp ghi đè lên đó, nên tác giả `BufPool` muốn bỏ bước zero. Nhưng nếu chỉ "tạo một `[u8; 65536]` chưa khởi tạo" rồi coi nó là mảng `u8` bình thường thì compiler không chấp nhận, kể cả khi `u8` nhận mọi bit pattern. Mục này giải thích vì sao, và `MaybeUninit<T>` làm gì để việc bỏ zero trở thành hợp lệ.

### 6.1 Vì sao "đọc rác" không đơn giản

`std::mem` đưa ra ba lớp lý do.

1. **Bất biến của kiểu.** Biến kiểu reference phải căn đúng và khác null. Vì vậy zero-initialize một biến kiểu reference gây UB *tức thời*, kể cả khi reference đó chưa bao giờ được dùng để đọc bộ nhớ[^mu-zero-ref].
2. **`bool` chỉ có hai giá trị.** Bộ nhớ chưa khởi tạo có thể chứa nội dung bất kỳ, trong khi `bool` phải luôn là `true` hoặc `false`; nên tạo `bool` chưa khởi tạo là UB[^mu-bool].
3. **Bộ nhớ chưa khởi tạo không có giá trị cố định.** Đọc cùng một byte chưa khởi tạo nhiều lần có thể cho kết quả khác nhau[^mu-fixed]. Vì vậy có dữ liệu chưa khởi tạo trong một biến là UB ngay cả với kiểu số nguyên, vốn chấp nhận bất kỳ bit pattern cố định nào[^mu-int].

Lớp thứ ba chính là chuyện của `[u8; 65536]`. Reference nói cùng ý ở cấp ngôn ngữ: số nguyên, số thực và raw pointer không được lấy từ bộ nhớ chưa khởi tạo[^ref-ub-scalar].

### 6.2 Compiler khai thác bất biến đó

Std docs nói compiler khai thác bất biến "biến được khởi tạo đúng yêu cầu của kiểu" cho nhiều tối ưu, ví dụ bỏ kiểm tra lúc chạy và tối ưu layout enum[^mu-exploit]. Vì thế UB ở đây không đơn thuần là "đọc ra giá trị lạ": đường code còn có thể bị cắt bỏ hoàn toàn (suy luận của người soạn).

### 6.3 Cách dùng đúng

Tạo `MaybeUninit<&i32>::uninit()` là hợp lệ, vì compiler biết dữ liệu bên trong `MaybeUninit<T>` có thể không hợp lệ[^mu-may-invalid]. Việc cần làm là ghi giá trị trước, rồi mới `assume_init`, và chỉ được phép *sau khi* đã khởi tạo đúng[^mu-assume]. Có thể coi `MaybeUninit<T>` giống `Option<T>` nhưng không có theo dõi lúc chạy và không có kiểm tra an toàn[^mu-option].

```rust
use std::mem::MaybeUninit;

// Minh họa theo ví dụ trong tài liệu std::mem::MaybeUninit.
let mut x = MaybeUninit::<&i32>::uninit();
x.write(&0);                          // khởi tạo bằng một giá trị hợp lệ
let x: &i32 = unsafe { x.assume_init() }; // chỉ sau khi đã ghi
```

Với `BufPool`, cách dùng tương ứng là cấp slot dưới dạng `MaybeUninit<u8>`, để `read` ghi `n` byte vào đầu slot, rồi chỉ coi đúng `n` byte đó là đã khởi tạo (nhận xét của người soạn: thiết kế minh họa).

Cuối cùng là hệ quả cho layout. Mọi bit pattern đều hợp lệ với `MaybeUninit<T>`, nên compiler không áp dụng được tối ưu niche[^mu-niche]. Tài liệu std đưa ví dụ so sánh `size_of::<Option<bool>>()` và `size_of::<Option<MaybeUninit<bool>>>()` với kết quả 1 và 2[^mu-niche-assert]. Đó là ví dụ minh họa trong docs, không nằm trong danh sách kiểu mà std *bảo đảm* về layout của `Option`; xem [Chương 02, mục 5–6](02-type-layout.md).

<details><summary>Đi sâu: out-pointer, khởi tạo mảng, khởi tạo từng field, <code>Drop</code></summary>

- *Out-pointer*: thay vì trả dữ liệu từ hàm, truyền vào một pointer tới vùng nhớ chưa khởi tạo để hàm ghi kết quả vào. Cách này hữu ích khi người gọi cần kiểm soát nơi lưu và muốn tránh di chuyển (move) thừa[^mu-outptr]. Ví dụ trong docs có chú thích rằng `write` không drop nội dung cũ, và điều này quan trọng[^mu-write].
- *Khởi tạo mảng từng phần tử*: docs có riêng một mục cho việc này, dùng `MaybeUninit<T>` để khởi tạo một mảng lớn từng phần tử[^mu-array]. Cách làm là tạo mảng `MaybeUninit`, ghi từng phần tử, rồi mới biến đổi (transmute) thành mảng đã khởi tạo[^mu-array-transmute]. Docs ghi chú `Drop` của `MaybeUninit` không làm gì, nên nếu panic giữa vòng lặp thì rò bộ nhớ nhưng không gây vấn đề memory safety[^mu-drop]. (Panic và exception safety: [Chương 08](08-panic-abi-ffi.md), mục 1 và 3.)
- *Khởi tạo struct từng field*: dùng `MaybeUninit<T>` cùng cú pháp `&raw mut` để lấy pointer tới field mà không tạo reference tới dữ liệu chưa khởi tạo[^mu-raw-field]. (Suy luận của người soạn: đây là lý do `&raw` tồn tại; tạo `&` tới field chưa khởi tạo sẽ là "tạo giá trị không hợp lệ" ở mục 2.)
- "Khởi tạo" ở mức kiểu chưa phải "hợp lệ ở mức logic". Docs cảnh báo một `Vec<T>` có toàn bit 1 được coi là đã khởi tạo (theo cài đặt hiện tại, không phải bảo đảm ổn định), vì yêu cầu duy nhất compiler biết là data pointer khác null; nhưng với hầu hết thao tác safe, kể cả drop, nó vẫn gây UB[^mu-vec].
- Về layout: `MaybeUninit<T>` là union, và `#[repr(transparent)]` trên union còn là tính năng không ổn định. Tài liệu cảnh báo bảo đảm của `#[repr(transparent)]` trên union có thể thay đổi, nhưng `MaybeUninit<T>` vẫn *luôn* bảo đảm cùng size, alignment và ABI với `T`[^mu-repr-union].

</details>

**Quay lại Q6:** "chưa khởi tạo" không phải một giá trị mà là *không có giá trị*; `MaybeUninit<T>` là kiểu được dành riêng để giữ chỗ cho nó, và chỉ sau khi ghi đủ thì mới `assume_init`.

## 7. Miri: phát hiện UB trong lúc chạy (Q7)

> **Trả lời ngắn.** Miri là công cụ phát hiện UB: nó chạy binary hoặc test suite của một cargo project và bắt những chỗ unsafe code không giữ đúng yêu cầu an toàn của nó[^miri-what]. Nó bắt được các lỗi như truy cập ngoài biên và use-after-free[^miri-oob]. Nhưng Miri chỉ kiểm *các lần chạy cụ thể*: nó báo UB thì code chắc chắn unsound, còn không báo thì chưa chứng minh được gì[^miri-limit].

**Giải thích.**

*Với `ingest`.* `BufPool` có unsafe, nên team muốn thêm một bước CI chạy test của crate này qua Miri. Mục này nói bước đó bắt được gì, chạy ra sao, và vì sao một bước CI xanh không có nghĩa `BufPool` đúng.

### 7.1 Nó bắt gì

README của Miri liệt kê các loại lỗi sau (kèm ví dụ trong `BufPool`):

- Truy cập ngoài biên và use-after-free[^miri-oob], như đọc `slot[n]` với `n` vượt buffer, hoặc đọc buffer đã trả về pool.
- Dùng dữ liệu chưa khởi tạo không hợp lệ[^miri-uninit].
- Truy cập hoặc reference không đủ alignment[^miri-align].
- Vi phạm bất biến kiểu cơ bản, như `bool` không phải 0 hoặc 1, hay discriminant enum không hợp lệ[^miri-invariant].
- Data race, và mô phỏng *một phần* weak memory effects (atomic read có thể trả về giá trị cũ)[^miri-race].
- Vi phạm luật aliasing của Stacked Borrows, được ghi là thử nghiệm[^miri-sb].
- Vi phạm luật aliasing của Tree Borrows, cũng thử nghiệm và là lựa chọn thay thế tùy chọn[^miri-tb].

Ngoài ra Miri báo memory leak: bộ nhớ vẫn đang được cấp ở cuối chương trình mà không còn truy cập được từ `static` toàn cục[^miri-leaks].

### 7.2 Cài và chạy

Cài trên nightly bằng `rustup +nightly component add miri`[^miri-install]. Chạy toàn bộ test qua Miri bằng `cargo miri test`[^miri-test-all]. Với project dạng binary thì dùng `cargo miri run`[^miri-test]. Lần chạy đầu Miri thực hiện một số bước setup và cài một số dependency[^miri-setup]. Cờ truyền qua biến môi trường `MIRIFLAGS`[^miri-flags]. Khi biên dịch qua `cargo miri`, cờ cấu hình `cfg(miri)` được bật cho code sẽ chạy dưới Miri[^miri-cfg]. Bạn dùng nó để bỏ qua test mà Miri không hỗ trợ[^miri-ignore].

```sh
# Minh họa, không kèm output
rustup +nightly component add miri
cargo +nightly miri test
MIRIFLAGS="-Zmiri-many-seeds=0..16" cargo +nightly miri test
```

### 7.3 Giới hạn

README nêu thẳng: Miri **không** bắt mọi vi phạm đặc tả của Rust, một phần vì không có đặc tả đầy đủ[^miri-notall]. Nó dùng *xấp xỉ riêng* của nó về cái gì là UB[^miri-approx]. Khi Miri thấy UB thì code chắc chắn unsound; còn khi Miri không thấy, có thể bạn chỉ cần thử nhiều input hoặc nhiều lựa chọn non-deterministic hơn[^miri-limit].

Có hai ví dụ cụ thể. Nếu chương trình dựa vào chi tiết layout *không được đặc tả*, Miri vẫn chạy ngon nhưng chương trình có thể hỏng (kể cả UB) trên compiler hoặc nền tảng khác[^miri-layout]. Và Miri chỉ kiểm *một* trong nhiều lần thực thi khả dĩ, nên bỏ sót lỗi chỉ xảy ra ở lần thực thi khác[^miri-nondet].

Nói cách khác, Miri là một *thực thi cụ thể*, không phải chứng minh (nhận xét của người soạn). Với `BufPool` nó đáng nằm trong CI, nhưng một lần chạy "sạch" chỉ có nghĩa "với các test này, không có UB nào Miri nhận ra", chứ không phải `BufPool` sound.

<details><summary>Đi sâu: isolation, <code>-Zmiri-many-seeds</code>, FFI, cờ tắt aliasing, cross-interpretation</summary>

- Mặc định Miri chạy deterministic bằng cách cô lập chương trình khỏi host: API như entropy, biến môi trường, đồng hồ được thay bằng bản "giả" deterministic; cờ `-Zmiri-disable-isolation` để dùng API thật[^miri-isolation].
- Một số thứ do Miri chọn ngẫu nhiên: địa chỉ cơ sở của allocation và cách các thread đan xen nhau. `-Zmiri-many-seeds` thử nhiều seed để khám phá nhiều thực thi khác nhau[^miri-many-seeds].
- Miri chạy chương trình như một *interpreter độc lập nền tảng*, nên chương trình không có quyền truy cập hầu hết API đặc thù nền tảng hay FFI[^miri-ffi]. (FFI là chủ đề của [Chương 08](08-panic-abi-ffi.md).) Khi gặp thao tác không hỗ trợ, Miri báo rõ ràng[^miri-unsupported].
- Cờ `-Zmiri-disable-stacked-borrows` làm Miri chạy nhanh hơn, nhưng sẽ không phát hiện vi phạm aliasing nào. README gọi việc dùng cờ này là unsound, kèm chú thích rằng các quy tắc soundness bị ảnh hưởng là thử nghiệm[^miri-disable-sb].
- Miri có thể "cross-interpret" cho target khác, ví dụ chạy test trên target big-endian `s390x-unknown-linux-gnu`[^miri-cross].
- Miri cập nhật cùng compiler để bảo vệ trước UB *như compiler hiện tại hiểu*, nhưng không hứa gì cho phiên bản rustc tương lai[^miri-keepup].
- Mọi trích dẫn README ở đây là bản ghim ở commit `0923e3ee8eccff5912b668cbccf3bc464ff3c0e1`; cờ có thể đổi theo thời gian.

</details>

**Quay lại Q7:** Miri là kiểm toán viên động cho unsafe code: bắt nhiều loại UB, nhưng chỉ trên đường chạy mà bạn đưa cho nó, nên dùng như một lưới an toàn, không phải bằng chứng.

## 8. Liên hệ với Rust: `unsafe fn`, `unsafe {}`, `unsafe impl`, `// SAFETY:`

Phần này nối các mục trước vào thói quen viết code Rust, với `BufPool` làm ví dụ.

*Bốn vị trí của `unsafe`.* Tài liệu `std` tóm tắt từng vị trí:

- `unsafe fn`: gọi hàm nghĩa là tuân thủ một hợp đồng mà compiler không ép được[^kw-four].
- `unsafe trait`: implement trait nghĩa là tuân thủ hợp đồng đó[^kw-trait].
- `unsafe {}`: hợp đồng cần để gọi các thao tác bên trong khối đã được lập trình viên kiểm và được bảo đảm tuân thủ[^kw-block].
- `unsafe impl`: hợp đồng cần để implement trait đã được kiểm[^kw-impl].

*Viết hợp đồng.* Vì `unsafe fn` và `unsafe trait` chỉ ra hợp đồng mà compiler không ép, tài liệu `std` nhấn mạnh việc *ghi lại* nó[^kw-document]. Quy ước là mục `# Safety` trong doc comment, giải thích điều kiện người gọi phải giữ để gọi hàm an toàn; ví dụ `Vec::set_len` ghi các điều kiện đó[^kw-safety-section]. Với `BufPool`, `slot_unchecked` sẽ có một mục `# Safety` nói `i` phải nhỏ hơn số slot.

```rust
/// Dereference the given pointer.
///
/// # Safety
///
/// `ptr` must be aligned and must not be dangling.
unsafe fn deref_unchecked(ptr: *const i32) -> i32 {
    // SAFETY: the caller is required to ensure that `ptr` is aligned and dereferenceable.
    unsafe { *ptr }
}
```

*Ký hợp đồng bằng `// SAFETY:`.* Ví dụ trong tài liệu `std` đặt một comment `// SAFETY:` ngay trên khối `unsafe` để nói vì sao các điều kiện được thỏa[^kw-safety-comment]. std nói thực hiện thao tác unsafe đòi một khối `unsafe {}`[^kw-unsafe-blocks]. Ví dụ gốc còn bật `#![deny(unsafe_op_in_unsafe_fn)]` ở đầu (dòng này được bỏ khỏi đoạn trích trên cho gọn)[^kw-deny-lint], nên ngay trong một `unsafe fn`, mỗi thao tác unsafe vẫn cần khối `unsafe {}` của nó. Nghĩa là hợp đồng của hàm (người gọi phải đảm bảo) và việc kiểm hợp đồng của từng thao tác bên trong là hai việc riêng. (Nhận xét của người soạn: một comment `SAFETY:` tốt trả lời "vì sao điều kiện của thao tác này đúng *ở đây*", dựa trên hợp đồng của hàm bao ngoài hoặc một kiểm tra runtime ngay trước đó.)

*`unsafe trait` và `unsafe impl`.* Xem ví dụ `MakeEven` trong tài liệu `std`. Trait khai báo hợp đồng là `make_even` phải trả về số chẵn[^kw-makeeven-contract]. Bản implement ký bằng `unsafe impl` kèm comment `// SAFETY:`[^kw-makeeven-safety]. Hàm *safe* `use_make_even` sau đó dựa vào lời hứa này để gọi `unreachable_unchecked`[^kw-makeeven]. Điểm mấu chốt: hợp đồng của trait được dùng để thỏa hợp đồng của một hàm unsafe khác; nếu implementor nói dối, hàm safe sẽ trở thành unsound (suy luận của người soạn, từ ví dụ trên).

*Ví dụ thực tế: `Send` và `Sync`.* Giả sử `BufPool` phải được chia sẻ giữa các handler thread của `ingest`, và team định viết tay `unsafe impl Send`/`Sync` cho nó. Nomicon định nghĩa: một kiểu là `Send` nếu an toàn để chuyển sang thread khác, và `Sync` nếu an toàn để chia sẻ giữa các thread (`T: Sync` khi và chỉ khi `&T: Send`)[^nomicon-sendsync]. Chúng là `unsafe trait`: implement chúng là unsafe, và code unsafe khác được quyền giả định chúng được implement đúng[^nomicon-sendsync-unsafe]. Vì vậy implement sai `Send` hay `Sync` có thể gây UB[^nomicon-sendsync-wrong], tức là unsound.

Nomicon cũng liệt kê `Send`, `Sync`, `GlobalAlloc` là các unsafe trait mà std định nghĩa (trang này ghi mốc Rust 1.29.2; còn trait khác chưa ổn định)[^nomicon-unsafe-traits-list]. Chi tiết `Send`/`Sync` từ phía concurrency: [P4 · Chương 08, mục 3](../04-concurrency/08-rust-concurrency.md). (Nhận xét của người soạn: với một backend engineer, đây là lý do ta không bao giờ viết `unsafe impl Send` để "cho compiler im lặng" khi một kiểu bị báo là không `Send`. Bạn đang ký một hợp đồng mà compiler không kiểm được, và code unsafe khác sẽ tin vào lời ký đó.)

*Soundness của API công khai.* Mục 3 áp dụng ở đây: một hàm `pub` *safe* chứa `unsafe` phải đúng với *mọi* đầu vào safe có thể; nếu không, hàm là unsound. Reference coi đây là trách nhiệm của người viết unsafe code[^ref-soundness-duty]. Thư viện chuẩn làm đúng như vậy: Nomicon nhắc rằng các giao diện Safe Rust của nó xây trên những cài đặt unsafe đã được kiểm thủ công cẩn thận[^nomicon-std-unsafe].

## Tóm tắt

- `unsafe` mở khóa một danh sách nhỏ thao tác: dereference raw pointer, gọi hàm unsafe, implement unsafe trait, `static mut`, field của union[^nomicon-five]. Nó không đổi luật: chương trình Rust không bao giờ được gây UB[^ref-ub-intro].
- `unsafe fn` và `unsafe trait` là nơi *đặt* hợp đồng[^kw-four]. `unsafe {}` và `unsafe impl` là nơi *ký* hợp đồng[^kw-block].
- UB cho compiler toàn quyền[^nomicon-fullrights]. Nó ảnh hưởng toàn chương trình[^ref-ub-whole]. Danh sách UB của Reference không đầy đủ và chưa có mô hình hình thức[^ref-ub-nomodel].
- Danh sách UB có mục "unwind qua frame không cho phép unwind"[^ref-ub-call]. Nhưng panic Rust chạm ranh giới không-unwind (ví dụ hàm `extern "C"`) là abort an toàn, còn exception ngoại lai đi vào Rust mới là UB; chi tiết ở chương 08[^nomicon-ffi-boundary].
- Code unsafe là *sound* nếu mọi client safe không kích hoạt được UB[^ref-sound]. Tính an toàn không cục bộ, nên cách đáng tin cậy để khoanh vùng là privacy ở ranh giới module[^nomicon-wwu-privacy].
- `&T` bất biến (trừ `UnsafeCell`), `&mut T` độc quyền[^ref-ub-alias]. Luật chính xác còn đang được xác định[^ref-ub-alias-open]. Stacked/Tree Borrows là các mô hình thử nghiệm trong Miri[^miri-tb].
- Raw pointer hợp lệ *theo từng truy cập*[^ptr-valid]. Provenance (không gian, thời gian, khả biến) có thể thu hẹp qua dẫn xuất nhưng không bao giờ nới[^ptr-prov-shrink].
- Tạo giá trị không hợp lệ là UB ngay lúc tạo[^ref-ub-valid-assume]. `MaybeUninit<T>` là kiểu giữ chỗ cho giá trị có thể chưa khởi tạo, cùng size, alignment và ABI với `T`[^mu-layout].
- Miri bắt nhiều loại UB trong lúc chạy test hoặc binary, nhưng chỉ trên các thực thi cụ thể: báo UB thì chắc chắn unsound, không báo thì chưa kết luận được[^miri-limit].
- Viết mục `# Safety` cho mỗi `unsafe fn`[^kw-safety-section]. Viết comment `// SAFETY:` cho từng khối `unsafe`[^kw-safety-comment]. Implement `Send`/`Sync` bằng tay là cam kết nghiêm trọng, vì sai có thể gây UB[^nomicon-sendsync-wrong].

## Câu hỏi tự kiểm tra

1. Nomicon liệt kê những khả năng nào chỉ có trong Unsafe Rust? Khối `unsafe` có tắt borrow checker không?
2. `unsafe fn` và `unsafe {}` khác nhau thế nào về "ai chịu trách nhiệm"?
3. Vì sao UB trong một chỗ có thể ảnh hưởng chỗ khác của chương trình, kể cả code C qua FFI?
4. Một hàm `index` dùng `get_unchecked` chỉ đổi `<` thành `<=`. Nó sound hay unsound? Điều này nói gì về tính "cục bộ" của unsafe?
5. Vì sao thêm `fn make_room(&mut self) { self.cap += 1 }` vào `Vec` ngây thơ là unsound, và vì sao để nó private thì ổn?
6. Hai biến alias nhau khi nào? Vì sao compiler quan tâm đến `&mut` không alias?
7. Vai trò của `UnsafeCell` trong luật aliasing là gì? Nó có cho phép `&mut` alias không?
8. "Pointer hợp lệ" nghĩa là gì? Provenance gồm những thành phần nào, và vì sao một `usize` không đủ?
9. Vì sao tạo `bool` chưa khởi tạo là UB, và vì sao ngay cả `i32` chưa khởi tạo cũng là UB?
10. Miri bắt được những loại lỗi nào, và vì sao "Miri không báo gì" không phải chứng minh code đúng?
11. Một crate cần `unsafe impl Send for MyType`. Bạn phải kiểm những gì, và để lại ở đâu?

<details><summary>Đáp án</summary>

1. Năm thứ: dereference raw pointer, gọi hàm unsafe, implement unsafe trait, truy cập hoặc sửa `static mut`, truy cập field của union[^nomicon-five]. Không, khối `unsafe` không tắt borrow checker: reference dùng trong khối `unsafe` vẫn được kiểm tra[^book-unsafe-checks]. Reference xác nhận code trong khối `unsafe` vẫn phải tránh mọi UB[^ref-ub-incorrect]. (Danh sách năm mục là của Nomicon; Reference liệt kê chi tiết hơn, xem phần Đi sâu của mục 1.)
2. `unsafe fn` đặt hợp đồng: *người gọi* chịu trách nhiệm đọc tài liệu và giữ đúng[^nomicon-unsafe-contract]. `unsafe {}` *ký* hợp đồng: người viết khối cam kết các thao tác bên trong đã được kiểm[^nomicon-block-keyword]. Tài liệu `std` tóm tắt cả bốn vị trí của `unsafe` ở mục 8[^kw-four].
3. UB cho compiler toàn quyền làm hành vi tùy ý với chương trình[^nomicon-fullrights]. Reference nói UB ảnh hưởng toàn chương trình[^ref-ub-whole]. Ngay cả UB nằm trong code C gọi qua FFI cũng làm cả chương trình có UB, gồm cả phần Rust[^ref-ub-ffi].
4. Unsound: đổi thành `<=` làm hàm cho phép Safe Rust gây UB dù ta chỉ sửa phần safe[^nomicon-wwu-unsound]. Nghĩa là an toàn không cục bộ[^nomicon-wwu-nonlocal].
5. Nó phá bất biến "`cap` phản ánh vùng nhớ đã cấp" mà code unsafe dựa vào[^nomicon-wwu-invariant]. Để private thì chỉ module định nghĩa gọi được, nên ranh giới module khoanh được vùng cần tin[^nomicon-wwu-private-ok].
6. Hai biến alias nhau khi chúng trỏ tới vùng nhớ chồng nhau (cách hiểu rộng nhất của Nomicon)[^nomicon-alias-def]. Compiler muốn giữ giá trị trong register và bỏ hoặc dịch chuyển đọc-ghi[^nomicon-alias-opts]. Điều này chỉ đúng nếu `&mut` không bị alias, vì khi đó mới loại được trường hợp như `compute(&x, &mut x)`[^nomicon-alias-compute].
7. `UnsafeCell` opt-out khỏi bảo đảm bất biến của `&T`[^uc-optout]. Không, nó không cho phép `&mut` alias: không có cách hợp lệ để có `&mut` alias, kể cả với `UnsafeCell`[^uc-mutref].
8. Hợp lệ *cho một truy cập cụ thể* (đọc hay ghi, bao nhiêu byte)[^ptr-valid]. Provenance có thành phần không gian, thời gian và khả biến[^ptr-prov-parts]. `usize` không biểu diễn đúng một pointer: chuyển pointer sang `usize` thường chỉ trích ra địa chỉ[^ptr-usize].
9. Compiler giả định biến luôn khởi tạo đúng yêu cầu của kiểu[^mu-invariant]. Bộ nhớ chưa khởi tạo có thể chứa nội dung bất kỳ, trong khi `bool` chỉ được là `true` hoặc `false`[^mu-bool]. Với `i32`, bộ nhớ chưa khởi tạo không có giá trị cố định: đọc cùng một byte nhiều lần có thể ra kết quả khác nhau[^mu-fixed]. Vì vậy dữ liệu chưa khởi tạo trong một biến là UB ngay cả với kiểu số nguyên[^mu-int].
10. Miri bắt truy cập ngoài biên và use-after-free[^miri-oob]. Nó cũng bắt dùng dữ liệu chưa khởi tạo không hợp lệ[^miri-uninit]. Nó bắt truy cập không đủ alignment[^miri-align]. Nó bắt vi phạm bất biến kiểu[^miri-invariant]. Nó bắt cả data race[^miri-race]. Với aliasing, nó bắt vi phạm Stacked Borrows, ở mức thử nghiệm[^miri-sb]. Miri chỉ kiểm một trong nhiều lần thực thi khả dĩ[^miri-nondet]. Nó còn dùng xấp xỉ riêng về UB[^miri-approx]. Vì vậy "không báo" chỉ nghĩa là có thể chưa thử đủ input[^miri-limit].
11. Phải kiểm kiểu đúng là an toàn để chuyển sang thread khác, theo hợp đồng `Send` của tài liệu[^nomicon-sendsync]. `unsafe impl` là chỗ tuyên bố bản implement giữ đúng hợp đồng của trait[^nomicon-impl-keyword]. Lý do kiểm ghi trong comment `// SAFETY:` ngay trên `unsafe impl`, như ví dụ `MakeEven` của std[^kw-makeeven-safety]. Implement sai có thể gây UB[^nomicon-sendsync-wrong].

</details>

## Đọc thêm

- [Rustonomicon — What Unsafe Rust Can Do](https://doc.rust-lang.org/1.95.0/nomicon/what-unsafe-does.html): danh sách năm khả năng và danh sách UB ngắn.
- [Rustonomicon — How Safe and Unsafe Interact](https://doc.rust-lang.org/1.95.0/nomicon/safe-unsafe-meaning.html): ba cách dùng `unsafe`, soundness, ví dụ `BTreeMap`/`Ord`.
- [Rustonomicon — Working with Unsafe](https://doc.rust-lang.org/1.95.0/nomicon/working-with-unsafe.html): vì sao unsafe không cục bộ, ví dụ `Vec`, privacy.
- [Rust Reference — Behavior considered undefined](https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html): danh sách UB đầy đủ nhất hiện có, định nghĩa dangling/misaligned/valid.
- [Rust Reference — Unsafety](https://doc.rust-lang.org/1.95.0/reference/unsafety.html): danh sách chính xác các thao tác unsafe.
- [Rustonomicon — Aliasing](https://doc.rust-lang.org/1.95.0/nomicon/aliasing.html): ví dụ `compute` và các tối ưu dựa trên alias analysis.
- [`std::ptr`](https://doc.rust-lang.org/1.95.0/std/ptr/index.html): validity, provenance, Strict vs Exposed Provenance.
- [`std::mem::MaybeUninit`](https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html): invariant khởi tạo, out-pointer, khởi tạo mảng/struct.
- [`UnsafeCell`](https://doc.rust-lang.org/1.95.0/std/cell/struct.UnsafeCell.html): nền của interior mutability.
- [Miri README (commit ghim)](https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md): cách cài, chạy, cờ và giới hạn.
- [`unsafe` keyword trong std](https://doc.rust-lang.org/1.95.0/std/keyword.unsafe.html): ví dụ `# Safety`, `// SAFETY:`, `unsafe trait`/`unsafe impl`.
- [Chương 02 — Type layout, mục 5–6](02-type-layout.md): niche (vì sao "giá trị không hợp lệ" ở mục 2 cho compiler chỗ giấu tag) và vì sao `MaybeUninit` làm mất niche (mục 6 ở đây).
- [Chương 08 — Panic, unwinding, ABI và FFI](08-panic-abi-ffi.md): mục 6–7 trả lời panic hoặc exception băng qua ranh giới FFI là abort hay UB (nối với danh sách UB ở mục 2); mục 3 nói exception safety khi unsafe code gặp panic.
- [P4 · Chương 08 — Concurrency trong Rust, mục 3 và 5](../04-concurrency/08-rust-concurrency.md): `Send`/`Sync` (mục 8 ở đây) và `UnsafeCell`/interior mutability (mục 4 ở đây) nhìn từ phía concurrency.
- [Lab 10 — Rust internals, bài (d) `miri_ub`](../labs/10-rust-internals-lab.md): các case UB cố ý (use-after-free, ngoài biên, aliasing, `bool` không hợp lệ) chỉ chạy dưới Miri, kèm output thật do người soạn lab chạy; đọc để thấy các loại UB ở mục 2, 4, 7 được Miri báo ra sao.

[^nomicon-five]: The Rustonomicon, What Unsafe Rust Can Do — <https://doc.rust-lang.org/1.95.0/nomicon/what-unsafe-does.html> — "The only things that are different in Unsafe Rust are that you can: Dereference raw pointers Call unsafe functions (including C functions, compiler intrinsics, and the raw allocator) Implement unsafe traits Access or modify mutable statics Access fields of unions"
[^nomicon-five-why]: The Rustonomicon, What Unsafe Rust Can Do — <https://doc.rust-lang.org/1.95.0/nomicon/what-unsafe-does.html> — "The reason these operations are relegated to Unsafe is that misusing any of these things will cause the ever dreaded Undefined Behavior."
[^nomicon-unsafe-keyword]: The Rustonomicon, How Safe and Unsafe Interact — <https://doc.rust-lang.org/1.95.0/nomicon/safe-unsafe-meaning.html> — "The unsafe keyword has two uses: to declare the existence of contracts the compiler can’t check, and to declare that a programmer has checked that these contracts have been upheld."
[^nomicon-unsafe-contract]: The Rustonomicon, How Safe and Unsafe Interact — <https://doc.rust-lang.org/1.95.0/nomicon/safe-unsafe-meaning.html> — "On functions, unsafe means that users of the function must check that function’s documentation to ensure they are using it in a way that maintains the contracts the function requires. On trait declarations, unsafe means that implementors of the trait must check the trait documentation to ensure their implementation maintains the contracts the trait requires."
[^nomicon-block-keyword]: The Rustonomicon, How Safe and Unsafe Interact — <https://doc.rust-lang.org/1.95.0/nomicon/safe-unsafe-meaning.html> — "You can use unsafe on a block to declare that all unsafe actions performed within are verified to uphold the contracts of those operations."
[^nomicon-impl-keyword]: The Rustonomicon, How Safe and Unsafe Interact — <https://doc.rust-lang.org/1.95.0/nomicon/safe-unsafe-meaning.html> — "You can use unsafe on a trait implementation to declare that the implementation upholds the trait’s contract."
[^nomicon-forbid]: The Rustonomicon, How Safe and Unsafe Interact — <https://doc.rust-lang.org/1.95.0/nomicon/safe-unsafe-meaning.html> — "you can even toss #![forbid(unsafe_code)] into your code base to statically guarantee that you’re only writing Safe Rust."
[^ref-ub-incorrect]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "Rust code is incorrect if it exhibits any of the behaviors in the following list. This includes code within unsafe blocks and unsafe functions."
[^ref-ub-intro]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "unsafe only means that avoiding undefined behavior is on the programmer; it does not change anything about the fact that Rust programs must never cause undefined behavior."
[^ref-unsafe-intro]: The Rust Reference, Unsafety — <https://doc.rust-lang.org/1.95.0/reference/unsafety.html> — "Unsafe operations are those that can potentially violate the memory-safety guarantees of Rust’s static semantics."
[^ref-unsafe-ops]: The Rust Reference, Unsafety — <https://doc.rust-lang.org/1.95.0/reference/unsafety.html> — "Accessing a field of a union, other than to assign to it."
[^ref-unsafe-tf]: The Rust Reference, Unsafety — <https://doc.rust-lang.org/1.95.0/reference/unsafety.html> — "Calling a safe function marked with a target_feature from a function that does not have a target_feature attribute enabling the same features"
[^ref-unsafe-extern-ed]: The Rust Reference, Unsafety — <https://doc.rust-lang.org/1.95.0/reference/unsafety.html> — "Prior to the 2024 edition, extern blocks were allowed to be declared without unsafe."
[^kw-extern-static]: Rust std, keyword unsafe — <https://doc.rust-lang.org/1.95.0/std/keyword.unsafe.html> — "Note that functions and statics declared in extern blocks are implicitly marked as unsafe (but not functions declared as extern "something" fn ...). Mutable statics are always unsafe, wherever they are declared."
[^nomicon-ffi-unsafe]: The Rustonomicon, How Safe and Unsafe Interact — <https://doc.rust-lang.org/1.95.0/nomicon/safe-unsafe-meaning.html> — "All FFI (Foreign Function Interface) functions are unsafe to call because the other language can do arbitrary operations that the Rust compiler can’t check."
[^nomicon-alloc-ub]: The Rustonomicon, What Unsafe Rust Can Do — <https://doc.rust-lang.org/1.95.0/nomicon/what-unsafe-does.html> — "the allocator APIs declare that deallocating unallocated memory is Undefined Behavior."
[^nomicon-fullrights]: The Rustonomicon, What Unsafe Rust Can Do — <https://doc.rust-lang.org/1.95.0/nomicon/what-unsafe-does.html> — "Invoking Undefined Behavior gives the compiler full rights to do arbitrarily bad things to your program."
[^ref-ub-whole]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "Undefined behavior affects the entire program."
[^ref-ub-ffi]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "Undefined behavior affects the entire program. For example, calling a function in C that exhibits undefined behavior of C means your entire program contains undefined behaviour that can also affect the Rust code. And vice versa, undefined behavior in Rust can cause adverse affects on code executed by any FFI calls to other languages."
[^ref-ub-notexh]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "The following list is not exhaustive; it may grow or shrink."
[^ref-ub-nomodel]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "There is no formal model of Rust’s semantics for what is and is not allowed in unsafe code, so there may be more behavior considered unsafe."
[^nomicon-ub-short]: The Rustonomicon, What Unsafe Rust Can Do — <https://doc.rust-lang.org/1.95.0/nomicon/what-unsafe-does.html> — "Unlike C, Undefined Behavior is pretty limited in scope in Rust. All the core language cares about is preventing the following things:"
[^nomicon-ub-transitive]: The Rustonomicon, What Unsafe Rust Can Do — <https://doc.rust-lang.org/1.95.0/nomicon/what-unsafe-does.html> — "However, violations of these constraints generally will just transitively lead to one of the above problems."
[^mu-exploit]: Rust std, MaybeUninit — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "This is exploited by the compiler for various optimizations, such as eliding run-time checks and optimizing enum layout."
[^nomicon-race]: The Rustonomicon, Data Races and Race Conditions — <https://doc.rust-lang.org/1.95.0/nomicon/races.html> — "Safe Rust guarantees an absence of data races, which are defined as: two or more threads concurrently accessing a location of memory one or more of them is a write one or more of them is unsynchronized A data race has Undefined Behavior, and is therefore impossible to perform in Safe Rust."
[^ref-ub-access]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "Accessing (loading from or storing to) a place that is dangling or based on a misaligned pointer."
[^ref-ub-alias]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "&T must point to memory that is not mutated while they are live (except for data inside an UnsafeCell\<U>), and &mut T must point to memory that is not read or written by any pointer not derived from the reference and that no other reference points to while they are live."
[^ref-ub-call]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "Calling a function with the wrong call ABI, or unwinding past a stack frame that does not allow unwinding (e.g. by calling a "C-unwind" function imported or transmuted as a "C" function or function pointer)."
[^ref-ub-invalid]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "Producing an invalid value. “Producing” a value happens any time a value is assigned to or read from a place, passed to a function/primitive operation or returned from a function/primitive operation."
[^ref-ub-valid-assume]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "The Rust compiler assumes that all values produced during program execution are “valid”, and producing an invalid value is hence immediate UB."
[^ref-ub-bool]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "A bool value must be false (0) or true (1)."
[^ref-ub-scalar]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "An integer (i*/u*), floating point value (f*), or raw pointer must be initialized, i.e., must not be obtained from uninitialized memory."
[^ref-ub-enum]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "An enum must have a valid discriminant, and all fields of the variant indicated by that discriminant must be valid at their respective type."
[^ref-ub-validrange]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "If a type has a custom range of a valid values, then a valid value must be in that range. In the standard library, this affects NonNull\<T> and NonZero\<T>."
[^ref-ub-ref]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "A reference or Box\<T> must be aligned and non-null, it cannot be dangling, and it must point to a valid value"
[^ref-ub-ref-debate]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "Note that the last point (about pointing to a valid value) remains a subject of some debate."
[^ref-ub-dangling]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "A reference/pointer is “dangling” if not all of the bytes it points to are part of the same live allocation (so in particular they all have to be part of some allocation)."
[^ref-ub-zero]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "If the size is 0, then the pointer is trivially never “dangling” (even if it is a null pointer)."
[^ref-ub-isize]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "the dynamic size of a Rust value (as determined by size_of_val) must never exceed isize::MAX"
[^ref-ub-misaligned-access]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "Note that a place based on a misaligned pointer only leads to undefined behavior when it is loaded from or stored to."
[^ref-ub-misaligned-raw]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "&raw const/&raw mut on such a place is allowed."
[^ref-sound]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "unsafe code that satisfies this property for any safe client is called sound; if unsafe code can be misused by safe code to exhibit undefined behavior, it is unsound."
[^ref-soundness-duty]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "It is the programmer’s responsibility when writing unsafe code to ensure that any safe code interacting with the unsafe code cannot trigger these behaviors."
[^nomicon-soundness]: The Rustonomicon, How Safe and Unsafe Interact — <https://doc.rust-lang.org/1.95.0/nomicon/safe-unsafe-meaning.html> — "the soundness property: No matter what, Safe Rust can’t cause Undefined Behavior."
[^nomicon-asym]: The Rustonomicon, How Safe and Unsafe Interact — <https://doc.rust-lang.org/1.95.0/nomicon/safe-unsafe-meaning.html> — "Safe Rust inherently has to trust that any Unsafe Rust it touches has been written correctly."
[^nomicon-unsafe-cant-trust]: The Rustonomicon, How Safe and Unsafe Interact — <https://doc.rust-lang.org/1.95.0/nomicon/safe-unsafe-meaning.html> — "On the other hand, Unsafe Rust cannot trust Safe Rust without care."
[^nomicon-btree-robust]: The Rustonomicon, How Safe and Unsafe Interact — <https://doc.rust-lang.org/1.95.0/nomicon/safe-unsafe-meaning.html> — "the Unsafe code in BTreeMap must be written to be robust against Ord implementations which aren’t actually total"
[^nomicon-btree-erratic]: The Rustonomicon, How Safe and Unsafe Interact — <https://doc.rust-lang.org/1.95.0/nomicon/safe-unsafe-meaning.html> — "BTreeMap will still behave completely erratically if you feed in values that don’t have a total ordering. It just won’t ever cause Undefined Behavior."
[^nomicon-std-unsafe]: The Rustonomicon, How Safe and Unsafe Interact — <https://doc.rust-lang.org/1.95.0/nomicon/safe-unsafe-meaning.html> — "Much of the Rust standard library also uses Unsafe Rust internally. These implementations have generally been rigorously manually checked, so the Safe Rust interfaces built on top of these implementations can be assumed to be safe."
[^nomicon-wwu-sound]: The Rustonomicon, Working with Unsafe — <https://doc.rust-lang.org/1.95.0/nomicon/working-with-unsafe.html> — "We say that such a correct unsafely implemented function is sound, meaning that safe code cannot cause Undefined Behavior through it"
[^nomicon-wwu-nonlocal]: The Rustonomicon, Working with Unsafe — <https://doc.rust-lang.org/1.95.0/nomicon/working-with-unsafe.html> — "This is the fundamental problem of safety: it’s non-local."
[^nomicon-wwu-state]: The Rustonomicon, Working with Unsafe — <https://doc.rust-lang.org/1.95.0/nomicon/working-with-unsafe.html> — "The soundness of our unsafe operations necessarily depends on the state established by otherwise “safe” operations."
[^nomicon-wwu-makeroom]: The Rustonomicon, Working with Unsafe — <https://doc.rust-lang.org/1.95.0/nomicon/working-with-unsafe.html> — "This code is 100% Safe Rust but it is also completely unsound."
[^nomicon-wwu-invariant]: The Rustonomicon, Working with Unsafe — <https://doc.rust-lang.org/1.95.0/nomicon/working-with-unsafe.html> — "Changing the capacity violates the invariants of Vec (that cap reflects the allocated space in the Vec)."
[^nomicon-wwu-pollute]: The Rustonomicon, Working with Unsafe — <https://doc.rust-lang.org/1.95.0/nomicon/working-with-unsafe.html> — "this unsafe code does more than pollute a whole function: it pollutes a whole module."
[^nomicon-wwu-privacy]: The Rustonomicon, Working with Unsafe — <https://doc.rust-lang.org/1.95.0/nomicon/working-with-unsafe.html> — "Generally, the only bullet-proof way to limit the scope of unsafe code is at the module boundary with privacy."
[^nomicon-wwu-private-ok]: The Rustonomicon, Working with Unsafe — <https://doc.rust-lang.org/1.95.0/nomicon/working-with-unsafe.html> — "The existence of make_room is not a problem for the soundness of Vec because we didn’t mark it as public. Only the module that defines this function can call it."
[^nomicon-wwu-abstraction]: The Rustonomicon, Working with Unsafe — <https://doc.rust-lang.org/1.95.0/nomicon/working-with-unsafe.html> — "It is therefore possible for us to write a completely safe abstraction that relies on complex invariants."
[^kw-set-len]: Rust std, keyword unsafe — <https://doc.rust-lang.org/1.95.0/std/keyword.unsafe.html> — "new_len must be less than or equal to capacity()."
[^nomicon-alias-def]: The Rustonomicon, Aliasing — <https://doc.rust-lang.org/1.95.0/nomicon/aliasing.html> — "variables and pointers alias if they refer to overlapping regions of memory."
[^nomicon-alias-compute]: The Rustonomicon, Aliasing — <https://doc.rust-lang.org/1.95.0/nomicon/aliasing.html> — "Specifically, we need to worry about function arguments that make input and output overlap, such as compute(&x, &mut x)."
[^nomicon-alias-correct]: The Rustonomicon, Aliasing — <https://doc.rust-lang.org/1.95.0/nomicon/aliasing.html> — "In Rust we know this input should be impossible because &mut isn’t allowed to be aliased."
[^nomicon-alias-sound-opt]: The Rustonomicon, Aliasing — <https://doc.rust-lang.org/1.95.0/nomicon/aliasing.html> — "In Rust, this optimization should be sound. For almost any other language, it wouldn’t be (barring global analysis)."
[^nomicon-alias-opts]: The Rustonomicon, Aliasing — <https://doc.rust-lang.org/1.95.0/nomicon/aliasing.html> — "keeping values in registers by proving no pointers access the value’s memory eliminating reads by proving some memory hasn’t been written to since last we read it eliminating writes by proving some memory is never read before the next write to it moving or reordering reads and writes by proving they don’t depend on each other"
[^ref-ub-alias-open]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "The exact aliasing rules are not determined yet, but here is an outline of the general principles"
[^ref-ub-box]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "Box\<T> is treated similar to &'static mut T for the purpose of these rules."
[^ref-ub-live]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "Each time a reference or box is dereferenced or reborrowed, it is considered live."
[^ref-ub-live-fn]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "When a reference (but not a Box!) is passed to a function, it is live at least as long as that function call, again except if the &T contains an UnsafeCell\<U>."
[^ref-ub-immutable]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "A mutation is any write of more than 0 bytes which overlaps with any of the relevant bytes (even if that write does not change the memory contents)."
[^ptr-alias-undecided]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "You must enforce Rust’s aliasing rules. The exact aliasing rules are not decided yet, so we only give a rough overview here."
[^ptr-ptr2ref]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "When converting a pointer to a reference (e.g. via &*ptr or &mut *ptr), there are several rules that must be followed: The pointer must be properly aligned. It must be non-null. It must be “dereferenceable” in the sense defined above. The pointer must point to a valid value of type T. You must enforce Rust’s aliasing rules."
[^ptr-ptr2ref-alias]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "When creating a shared reference, then while this reference exists, the memory it points to must not get mutated (except inside UnsafeCell)."
[^ptr-ref-cast]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "The result of casting a reference to a pointer is valid for as long as the underlying allocation is live and no reference (just raw pointers) is used to access the same memory."
[^miri-sb]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "Experimental: Violations of the Stacked Borrows rules governing aliasing for reference types"
[^miri-tb]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "Experimental: Violations of the Tree Borrows aliasing rules, as an optional alternative to Stacked Borrows"
[^uc-transmute]: Rust std, UnsafeCell — <https://doc.rust-lang.org/1.95.0/std/cell/struct.UnsafeCell.html> — "Mutating that data, for example through an alias or by transmuting a &T into a &mut T, is considered undefined behavior."
[^uc-optout]: Rust std, UnsafeCell — <https://doc.rust-lang.org/1.95.0/std/cell/struct.UnsafeCell.html> — "UnsafeCell\<T> opts-out of the immutability guarantee for &T: a shared reference &UnsafeCell\<T> may point to data that is being mutated."
[^uc-others]: Rust std, UnsafeCell — <https://doc.rust-lang.org/1.95.0/std/cell/struct.UnsafeCell.html> — "All other types that allow internal mutability, such as Cell\<T> and RefCell\<T>, internally use UnsafeCell to wrap their data."
[^uc-mutref]: Rust std, UnsafeCell — <https://doc.rust-lang.org/1.95.0/std/cell/struct.UnsafeCell.html> — "There is no legal way to obtain aliasing &mut, not even with UnsafeCell\<T>."
[^uc-race]: Rust std, UnsafeCell — <https://doc.rust-lang.org/1.95.0/std/cell/struct.UnsafeCell.html> — "UnsafeCell does nothing to avoid data races; they are still undefined behavior."
[^uc-get]: Rust std, UnsafeCell — <https://doc.rust-lang.org/1.95.0/std/cell/struct.UnsafeCell.html> — "Note that the only valid way to obtain a *mut T pointer to the contents of a shared UnsafeCell\<T> is through .get() or .raw_get()."
[^ref-raw]: The Rust Reference, Pointer types — <https://doc.rust-lang.org/1.95.0/reference/types/pointer.html> — "Raw pointers are pointers without safety or liveness guarantees."
[^ref-raw-deref]: The Rust Reference, Pointer types — <https://doc.rust-lang.org/1.95.0/reference/types/pointer.html> — "Dereferencing a raw pointer is an unsafe operation."
[^ref-raw-new]: The Rust Reference, Pointer types — <https://doc.rust-lang.org/1.95.0/reference/types/pointer.html> — "Raw pointers can be created directly using &raw const for *const pointers and &raw mut for *mut pointers."
[^ptr-valid]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "Whether a pointer is valid depends on the operation it is used for (read or write), and the extent of the memory that is accessed (i.e., how many bytes are read/written) – it makes no sense to ask “is this pointer valid”; one has to ask “is this pointer valid for a given access”."
[^ptr-valid-open]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "The precise rules for validity are not determined yet. The guarantees that are provided at this point are very minimal:"
[^ptr-zero]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "For memory accesses of size zero, every pointer is valid, including the null pointer."
[^ptr-null]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "A null pointer is never valid."
[^ptr-deref]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "Note that in Rust, every (stack-allocated) variable is considered a separate allocation."
[^ptr-prov]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "To rationalize claims like this, pointers need to somehow be more than just their addresses: they must have provenance."
[^ptr-prov-parts]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "The exact structure of provenance is not yet specified, but the permission defined by a pointer’s provenance have a spatial component, a temporal component, and a mutability component"
[^ptr-original]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "When an allocation is created, it has a unique Original Pointer."
[^ptr-inherit]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "Provenance is implicitly inherited by all pointers transitively derived from the Original Pointer through operations like offset, borrowing, and pointer casts."
[^ptr-prov-shrink]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "However, no operation can ever grow the permissions of the derived provenance: even if you “know” there is a larger allocation, you can’t derive a pointer with a larger provenance."
[^ptr-shared-prov]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "A shared reference only ever has provenance that permits reading from memory, and never permits writes, except inside UnsafeCell."
[^ptr-access-prov]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "It is undefined behavior to access memory through a pointer that does not have provenance over that memory."
[^ptr-offset-ub]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "It is undefined behavior to offset a pointer across a memory range that is not contained in the allocation it is derived from"
[^ptr-uaf]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "it’s uncontroversial to say that a Use After Free is clearly Undefined Behavior, even if you “get lucky” and the freed memory gets reallocated before your read/write"
[^ptr-usize]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "From this discussion, it becomes very clear that a usize cannot accurately represent a pointer, and converting from a pointer to a usize is generally an operation which only extracts the address."
[^ptr-strict]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "“Strict Provenance” refers to a set of APIs designed to make working with provenance more explicit. They are intended as substitutes for casting a pointer to an integer and back."
[^ptr-exposed-miri]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "Furthermore, Exposed Provenance will not work (well) with tools like Miri and CHERI."
[^ptr-exposed-api]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "Exposed Provenance is provided by the expose_provenance and with_exposed_provenance methods, which are equivalent to as casts between pointers and integers."
[^ptr-exposed-ub]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "if there is no previously ‘exposed’ provenance that justifies the way the returned pointer will be used, the program has undefined behavior."
[^ptr-prefer-strict]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "If at all possible, we encourage code to be ported to Strict Provenance APIs, thus avoiding the need for Exposed Provenance."
[^ptr-without-prov]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "Such a pointer cannot be used for memory accesses (except for zero-sized accesses)."
[^ptr-compare]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "Compare arbitrary pointers by address."
[^ptr-nonatomic]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "All accesses performed by functions in this module are non-atomic in the sense of atomic operations used to synchronize between threads. This means it is undefined behavior to perform two concurrent accesses to the same location from different threads unless both accesses only read from memory."
[^ptr-align]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "When a function requires proper alignment, it does so even if the access has size 0, i.e., even if memory is not actually touched."
[^ptr-dangling-def]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "We say that a pointer is “dangling” if it is not valid for any non-zero-sized accesses. This means out-of-bounds pointers, pointers to freed memory, null pointers, and pointers created with NonNull::dangling are all dangling."
[^mu-invariant]: Rust std, MaybeUninit — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "The compiler, in general, assumes that a variable is properly initialized according to the requirements of the variable’s type."
[^mu-zero-ref]: Rust std, MaybeUninit — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "zero-initializing a variable of reference type causes instantaneous undefined behavior, no matter whether that reference ever gets used to access memory"
[^mu-bool]: Rust std, MaybeUninit — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "Similarly, entirely uninitialized memory may have any content, while a bool must always be true or false. Hence, creating an uninitialized bool is undefined behavior"
[^mu-fixed]: Rust std, MaybeUninit — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "Reading the same uninitialized byte multiple times can give different results."
[^mu-int]: Rust std, MaybeUninit — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "This makes it undefined behavior to have uninitialized data in a variable even if that variable has an integer type, which otherwise can hold any fixed bit pattern"
[^mu-assume]: Rust std, MaybeUninit — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "Extract the initialized data -- this is only allowed *after* properly initializing `x`!"
[^mu-option]: Rust std, MaybeUninit — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "You can think of MaybeUninit\<T> as being a bit like Option\<T> but without any of the run-time tracking and without any of the safety checks."
[^mu-layout]: Rust std, MaybeUninit, Layout — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "MaybeUninit\<T> is guaranteed to have the same size, alignment, and ABI as T"
[^mu-niche]: Rust std, MaybeUninit, Layout — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "Furthermore because any bit value is valid for a MaybeUninit\<T> the compiler can’t apply non-zero/niche-filling optimizations, potentially resulting in a larger size:"
[^mu-write]: Rust std, MaybeUninit, out-pointers — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "`write` does not drop the old contents, which is important."
[^mu-drop]: Rust std, MaybeUninit, array initialization — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "Dropping a `MaybeUninit` does nothing, so if there is a panic during this loop, we have a memory leak, but there is no memory safety issue."
[^mu-raw-field]: Rust std, MaybeUninit, field-by-field — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "You can use MaybeUninit\<T> and the &raw mut syntax to initialize structs field by field:"
[^mu-vec]: Rust std, MaybeUninit — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "a 1-initialized Vec\<T> is considered initialized (under the current implementation; this does not constitute a stable guarantee) because the only requirement the compiler knows about it is that the data pointer must be non-null. Creating such a Vec\<T> does not cause immediate undefined behavior, but will cause undefined behavior with most safe operations (including dropping it)."
[^mu-repr-union]: Rust std, MaybeUninit, Layout — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "MaybeUninit is a union type, and #[repr(transparent)] on unions is unstable (see the tracking issue). Over time, the exact guarantees of #[repr(transparent)] on unions may evolve, and MaybeUninit may or may not remain #[repr(transparent)]. That said, MaybeUninit\<T> will always guarantee that it has the same size, alignment, and ABI as T"
[^miri-what]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "It can run binaries and test suites of cargo projects and detect unsafe code that fails to uphold its safety requirements."
[^miri-oob]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "Out-of-bounds memory accesses and use-after-free"
[^miri-uninit]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "Invalid use of uninitialized data"
[^miri-align]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "Not sufficiently aligned memory accesses and references"
[^miri-invariant]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "Violation of basic type invariants (a bool that is not 0 or 1, for example, or an invalid enum discriminant)"
[^miri-race]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "Data races and emulation of *some* weak memory effects, i.e., atomic reads can return outdated values"
[^miri-leaks]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "On top of that, Miri will also tell you about memory leaks: when there is memory still allocated at the end of the execution, and that memory is not reachable from a global static, Miri will raise an error."
[^miri-install]: Miri README, Using Miri — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "rustup +nightly component add miri"
[^miri-test]: Miri README, Using Miri — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "If you have a binary project, you can run it through Miri using cargo miri run."
[^miri-setup]: Miri README, Using Miri — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "The first time you run Miri, it will perform some extra setup and install some dependencies."
[^miri-flags]: Miri README, Using Miri — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "You can pass [flags][miri-flags] to Miri via `MIRIFLAGS`."
[^miri-cfg]: Miri README, Using Miri — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "When compiling code via cargo miri, the cfg(miri) config flag is set for code that will be interpreted under Miri."
[^miri-ignore]: Miri README, Using Miri — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "You can use this to ignore test cases that fail under Miri because they do things Miri does not support:"
[^miri-notall]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "be aware that Miri does not catch every violation of the Rust specification in your program, not least because there is no such specification."
[^miri-approx]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "Miri uses its own approximation of what is and is not Undefined Behavior in Rust."
[^miri-limit]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "When Miri finds UB, your code is definitely unsound, but when Miri does not find UB, then you may just have to test more inputs or more possible non-deterministic choices."
[^miri-layout]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "If the program relies on unspecified details of how data is laid out, it will still run fine in Miri -- but might break (including causing UB) on different compiler versions or different platforms."
[^miri-nondet]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "Miri tests one of many possible executions of your program, but it will miss bugs that only occur in a different possible execution."
[^miri-isolation]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "By default, Miri ensures a fully deterministic execution by isolating the program from the host system. Some APIs that would usually access the host, such as gathering entropy for random number generators, environment variables, and clocks, are replaced by deterministic “fake” implementations. Set MIRIFLAGS=“-Zmiri-disable-isolation” to access the real system APIs instead."
[^miri-many-seeds]: Miri README, Testing multiple different executions — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "Certain parts of the execution are picked randomly by Miri, such as the exact base address allocations are stored at and the interleaving of concurrently executing threads."
[^miri-ffi]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "Miri runs the program as a platform-independent interpreter, so the program has no access to most platform-specific APIs or FFI."
[^miri-unsupported]: Miri README, Using Miri — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "the interpreter will explicitly tell you when it finds something unsupported"
[^miri-disable-sb]: Miri README, flags — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "This can make Miri run faster, but it also means no aliasing violations will be detected. Using this flag is unsound (but the affected soundness rules are experimental)"
[^miri-cross]: Miri README, Cross-interpretation — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "For example, cargo miri test --target s390x-unknown-linux-gnu will run your test suite on a big-endian target"
[^miri-keepup]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "Miri will be updated with the Rust compiler to protect against UB as it is understood by the current compiler, but it makes no promises about future versions of rustc."
[^kw-four]: Rust std, keyword unsafe — <https://doc.rust-lang.org/1.95.0/std/keyword.unsafe.html> — "unsafe fn: calling this function means abiding by a contract the compiler cannot enforce."
[^kw-block]: Rust std, keyword unsafe — <https://doc.rust-lang.org/1.95.0/std/keyword.unsafe.html> — "unsafe {}: the contract necessary to call the operations inside the block has been checked by the programmer and is guaranteed to be respected."
[^kw-impl]: Rust std, keyword unsafe — <https://doc.rust-lang.org/1.95.0/std/keyword.unsafe.html> — "unsafe impl: the contract necessary to implement the trait has been checked by the programmer and is guaranteed to be respected."
[^kw-document]: Rust std, keyword unsafe — <https://doc.rust-lang.org/1.95.0/std/keyword.unsafe.html> — "Since unsafe fn and unsafe trait indicate that there is a safety contract that the compiler cannot enforce, documenting it is important."
[^kw-safety-section]: Rust std, keyword unsafe — <https://doc.rust-lang.org/1.95.0/std/keyword.unsafe.html> — "The # Safety section explains the contract that must be fulfilled to safely call the function."
[^kw-safety-comment]: Rust std, keyword unsafe — <https://doc.rust-lang.org/1.95.0/std/keyword.unsafe.html> — "// SAFETY: the caller is required to ensure that `ptr` is aligned and dereferenceable."
[^kw-deny-lint]: Rust std, keyword unsafe — <https://doc.rust-lang.org/1.95.0/std/keyword.unsafe.html> — "#![deny(unsafe_op_in_unsafe_fn)]"
[^kw-makeeven]: Rust std, keyword unsafe — <https://doc.rust-lang.org/1.95.0/std/keyword.unsafe.html> — "Note how the safety contract of the trait is upheld by the implementation, and is itself used to uphold the safety contract of the unsafe function unreachable_unchecked"
[^nomicon-sendsync]: The Rustonomicon, Send and Sync — <https://doc.rust-lang.org/1.95.0/nomicon/send-and-sync.html> — "A type is Send if it is safe to send it to another thread. A type is Sync if it is safe to share between threads (T is Sync if and only if &T is Send)."
[^nomicon-sendsync-unsafe]: The Rustonomicon, Send and Sync — <https://doc.rust-lang.org/1.95.0/nomicon/send-and-sync.html> — "First and foremost, they’re unsafe traits. This means that they are unsafe to implement, and other unsafe code can assume that they are correctly implemented."
[^nomicon-sendsync-wrong]: The Rustonomicon, Send and Sync — <https://doc.rust-lang.org/1.95.0/nomicon/send-and-sync.html> — "Incorrectly implementing Send or Sync can cause Undefined Behavior."
[^nomicon-unsafe-traits-list]: The Rustonomicon, How Safe and Unsafe Interact — <https://doc.rust-lang.org/1.95.0/nomicon/safe-unsafe-meaning.html> — "As of Rust 1.29.2, the standard library defines the following unsafe traits (there are others, but they are not stabilized yet and some of them may never be):"
[^book-unsafe-checks]: The Rust Programming Language, Unsafe Rust — <https://doc.rust-lang.org/1.95.0/book/ch20-01-unsafe-rust.html> — "It’s important to understand that unsafe doesn’t turn off the borrow checker or disable any of Rust’s other safety checks: If you use a reference in unsafe code, it will still be checked."
[^ref-unsafe-list]: The Rust Reference, Unsafety — <https://doc.rust-lang.org/1.95.0/reference/unsafety.html> — "The following language level features cannot be used in the safe subset of Rust:"
[^ref-unsafe-static]: The Rust Reference, Unsafety — <https://doc.rust-lang.org/1.95.0/reference/unsafety.html> — "Reading or writing a mutable or unsafe external static variable."
[^ref-unsafe-attr]: The Rust Reference, Unsafety — <https://doc.rust-lang.org/1.95.0/reference/unsafety.html> — "Applying an unsafe attribute to an item."
[^ref-ub-reserve]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "We also reserve the right to make some of the behavior in that list defined in the future."
[^ref-ub-dst-len]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "Note that dynamically sized types (such as slices and strings) point to their entire range, so it is important that the length metadata is never too large."
[^ref-ub-live-bound]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "For references, the liveness duration is upper-bounded by the syntactic lifetime assigned by the borrow checker; it cannot be live any longer than that lifetime."
[^kw-trait]: Rust std, keyword unsafe — <https://doc.rust-lang.org/1.95.0/std/keyword.unsafe.html> — "unsafe trait: implementing the trait means abiding by a contract the compiler cannot enforce."
[^kw-set-len-init]: Rust std, keyword unsafe — <https://doc.rust-lang.org/1.95.0/std/keyword.unsafe.html> — "The elements at old_len..new_len must be initialized."
[^nomicon-alias-broad]: The Rustonomicon, Aliasing — <https://doc.rust-lang.org/1.95.0/nomicon/aliasing.html> — "We will be using the broadest possible definition of aliasing for the sake of discussion. Rust’s definition will probably be more restricted to factor in mutations and liveness."
[^nomicon-wwu-private-access]: The Rustonomicon, Working with Unsafe — <https://doc.rust-lang.org/1.95.0/nomicon/working-with-unsafe.html> — "make_room directly accesses the private fields of Vec, so it can only be written in the same module as Vec."
[^uc-unique]: Rust std, UnsafeCell — <https://doc.rust-lang.org/1.95.0/std/cell/struct.UnsafeCell.html> — "Note that only the immutability guarantee for shared references is affected by UnsafeCell. The uniqueness guarantee for mutable references is unaffected."
[^uc-get-cast]: Rust std, UnsafeCell — <https://doc.rust-lang.org/1.95.0/std/cell/struct.UnsafeCell.html> — "Even though T and UnsafeCell\<T> have the same memory layout, the following is not allowed and undefined behavior:"
[^ptr-ptr2ref-mut]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "When creating a mutable reference, then while this reference exists, the memory it points to must not get accessed (read or written) through any other pointer or reference not derived from this reference."
[^ptr-ref-interleave]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "That is, reference and pointer accesses cannot be interleaved."
[^ptr-deref-nec]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "For a pointer to be valid, it is necessary, but not always sufficient, that the pointer be dereferenceable."
[^ptr-deref-def]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "a pointer is dereferenceable if the memory range of the given size starting at the pointer is entirely contained within the bounds of that allocation."
[^ptr-exposed-fragile]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "the semantics of Exposed Provenance are on much less solid footing than Strict Provenance"
[^ptr-without-sound]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "But it is still sound to: Create a pointer without provenance from just an address (see without_provenance)."
[^ptr-sentinel]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "This can still be useful for sentinel values like null or to represent a tagged pointer that will never be dereferenceable."
[^ptr-strict-win]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "Maximizing the amount of such code is a major win for avoiding specification complexity and to facilitate adoption of tools like CHERI and Miri"
[^ptr-nonatomic-volatile]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "Notice that this explicitly includes read_volatile and write_volatile: Volatile accesses cannot be used for inter-thread synchronization"
[^ptr-align-most]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "However, most functions require their arguments to be properly aligned, and will explicitly state this requirement in their documentation. Notable exceptions to this are read_unaligned and write_unaligned."
[^mu-may-invalid]: Rust std, MaybeUninit — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "The compiler knows that data inside // a `MaybeUninit<T>` may be invalid, and hence this is not UB:"
[^ref-ub-misaligned-ref]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "&/&mut on a place requires the alignment of the field type (or else the program would be “producing an invalid value”)"
[^nomicon-same-rules]: The Rustonomicon, Meet Safe and Unsafe — <https://doc.rust-lang.org/1.95.0/nomicon/meet-safe-and-unsafe.html> — "Unsafe Rust is exactly like Safe Rust with all the same rules and semantics. It just lets you do some extra things that are Definitely Not Safe"
[^nomicon-ub-list]: The Rustonomicon, What Unsafe Rust Can Do — <https://doc.rust-lang.org/1.95.0/nomicon/what-unsafe-does.html> — "Dereferencing (using the * operator on) dangling or unaligned pointers (see below) Breaking the pointer aliasing rules Calling a function with the wrong call ABI or unwinding from a function with the wrong unwind ABI. Causing a data race Executing code compiled with target features that the current thread of execution does not support Producing invalid values"
[^nomicon-ffi-boundary]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "If an unwinding operation does encounter an ABI boundary that is not permitted to unwind, the behavior depends on the source of the unwinding (Rust panic or a foreign exception): panic will cause the process to safely abort. A foreign exception entering Rust will cause undefined behavior."
[^ref-ext-unsafe-required]: The Rust Reference, External blocks — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "The unsafe keyword is semantically required to appear before the extern keyword on external blocks."
[^ref-vis-private]: The Rust Reference, Visibility and privacy — <https://doc.rust-lang.org/1.95.0/reference/visibility-and-privacy.html> — "If an item is private, it may be accessed by the current module and its descendants."
[^ref-ub-live-unspec]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "The exact liveness duration is not specified, but some bounds exist:"
[^ref-ub-immutable-shared]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "the bytes pointed to by a shared reference, including transitively through other references (both shared and mutable) and Boxes, are immutable; transitivity includes those references stored in fields of compound types."
[^ptr-prov-some-shrink]: Rust std, ptr — <https://doc.rust-lang.org/1.95.0/std/ptr/index.html> — "Some operations may shrink the permissions of the derived provenance, limiting how much memory it can access or how long it’s valid for (i.e. borrowing a subfield and subslicing can shrink the spatial component of provenance, and all borrowing can shrink the temporal component of provenance)."
[^mu-niche-assert]: Rust std, MaybeUninit, Layout — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "assert_eq!(size_of::\<Option\<bool>>(), 1); assert_eq!(size_of::\<Option\<MaybeUninit\<bool>>>(), 2);"
[^mu-outptr]: Rust std, MaybeUninit, out-pointers — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "instead of returning data from a function, pass it a pointer to some (uninitialized) memory to put the result into. This can be useful when it is important for the caller to control how the memory the result is stored in gets allocated, and you want to avoid unnecessary moves."
[^mu-array]: Rust std, MaybeUninit, array initialization — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "MaybeUninit\<T> can be used to initialize a large array element-by-element:"
[^mu-array-transmute]: Rust std, MaybeUninit, array initialization — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "Everything is initialized. Transmute the array to the initialized type."
[^miri-test-all]: Miri README, Using Miri — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "To run all tests in your project through Miri, use `cargo miri test`."
[^kw-unsafe-blocks]: Rust std, keyword unsafe — <https://doc.rust-lang.org/1.95.0/std/keyword.unsafe.html> — "Performing unsafe operations requires an unsafe {} block:"
[^kw-makeeven-contract]: Rust std, keyword unsafe — <https://doc.rust-lang.org/1.95.0/std/keyword.unsafe.html> — "`make_even` must return an even number."
[^kw-makeeven-safety]: Rust std, keyword unsafe — <https://doc.rust-lang.org/1.95.0/std/keyword.unsafe.html> — "// SAFETY: Our `make_even` always returns something even."
[^nomicon-wwu-unsound]: The Rustonomicon, Working with Unsafe — <https://doc.rust-lang.org/1.95.0/nomicon/working-with-unsafe.html> — "This program is now unsound, Safe Rust can cause Undefined Behavior, and yet we only modified safe code."
[^nomicon-alias-cache]: The Rustonomicon, Aliasing — <https://doc.rust-lang.org/1.95.0/nomicon/aliasing.html> — "In the previous example, we used the fact that &mut u32 can’t be aliased to prove that writes to *output can’t possibly affect *input. This lets us cache *input in a register, eliminating a read."
