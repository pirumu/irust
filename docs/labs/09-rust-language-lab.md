# Lab P9 — Rust language

> Đây là phần **thực hành** của phase P9 (Rust language), tách ra từ [tài liệu lý thuyết](../09-rust-language/00-tong-quan.md). Hãy đọc chương liên quan trước mỗi bài rồi mới làm: lab này không lặp lại lý thuyết mà bắt bạn **viết code cho test chạy qua**. Mọi khẳng định sự thật có chú thích cuối trang, kiểm bằng `python3 tools/verify_citations.py docs/labs/09-rust-language-lab.md`. Khác các lab khác, lab này **không in output mẫu**: đáp án nằm ở việc test xanh hay đỏ trên máy bạn.

## Chuẩn bị

Crate: `labs/crates/lab-language`, chỉ dùng `std`, không phụ thuộc crate ngoài. Mỗi bài là một module `src/exN_*.rs` gồm: đề bài trong doc của module (kèm các đoạn code **sai** được giữ bằng doctest `compile_fail`), phần `todo!()` để bạn điền, và `#[cfg(test)] mod tests` ở cuối file — **test là đặc tả**, đừng sửa test để cho qua.

Hai chế độ build:

| Chế độ | Lệnh (từ `labs/`) | Ý nghĩa |
|---|---|---|
| Làm bài | `cargo test -p lab-language exN_` | Mặc định là stub `todo!()`, test **đỏ** cho tới khi bạn cài xong. `exN_` lọc theo module, ví dụ `ex4_` |
| Kiểm lời giải mẫu | `cargo test -p lab-language --features solution` | Biên dịch `src/solutions/*.rs` thay cho stub; dùng để kiểm chính bộ lab, và để so sánh **sau khi** bạn đã tự làm |

Cách làm: mở file `src/exN_*.rs`, xóa khối `#[cfg(not(feature = "solution"))]` của hàm bạn đang làm và viết cài đặt thật (hoặc sửa thân `todo!()` tại chỗ; cfg ở đầu hàm có thể giữ nguyên nếu bạn chỉ thay thân). Đừng mở `src/solutions/` trước khi tự thử. Kiểm lint như CI của repo:

```bash
cd labs
cargo clippy -p lab-language --all-targets --all-features -- -D warnings
cargo test -p lab-language --features solution
```

Tám bài:

| Bài | Chủ đề | Chương |
|---|---|---|
| 1 | Sửa lỗi borrow | [01](../09-rust-language/01-ownership-borrowing-lifetime.md) |
| 2 | Trait, blanket impl, orphan rule, newtype | [02](../09-rust-language/02-trait.md) |
| 3 | Generic hay `dyn` | [03](../09-rust-language/03-generics-nang-cao.md), [02](../09-rust-language/02-trait.md) |
| 4 | `MyRc<T>` với `Cell` | [04](../09-rust-language/04-smart-pointer.md) |
| 5 | Kiểu lỗi riêng, `From`, `?` | [05](../09-rust-language/05-error-handling.md) |
| 6 | Iterator adapter tự viết | [06](../09-rust-language/06-iterator-va-closure.md) |
| 7 | `macro_rules!` | [07](../09-rust-language/07-macro.md) |
| 8 | Cargo features và workspace | [08](../09-rust-language/08-module-crate-cargo.md) |

## Bài 1 — Sửa lỗi borrow (`ex1_borrow`)

**Mục tiêu.** Nhìn một đoạn code bị từ chối, gọi tên được *ba mốc* của lỗi (nơi mượn, nơi bị đụng, nơi dùng sau đó) rồi sửa mà **không** đổi hành vi.

**Đọc trước.** [Chương 01](../09-rust-language/01-ownership-borrowing-lifetime.md): mục 3 (shared XOR mutable), mục 6 (NLL, Q4), mục 7 (sáu lỗi, Q5), mục 9 (mẫu sửa lỗi, Q7).

**Đề bài.** Trong doc của `src/ex1_borrow.rs` có bốn đoạn sai; mỗi đoạn tương ứng một hàm bạn phải viết:

| Hàm | Đoạn sai (mã lỗi ghi trong doctest) | Hành vi cần có |
|---|---|---|
| `read_then_push` | đọc `&v[0]` rồi `v.push` trong khi tham chiếu còn dùng (E0502) | đọc phần tử đầu `f`, đẩy `f * 10`, trả `Some(f)`; rỗng → `None` |
| `longest` | hàm trả `&str` từ hai tham số `&str` không có lifetime (E0106) | trả chuỗi dài hơn, hòa thì trả `a` |
| `take_first` | `let s = v[0];` với `String` (E0507) | move phần tử đầu ra, để `String::new()` lại tại chỗ |
| `remove_empty` | `v.remove(i)` trong vòng `for … in v.iter()` (E0502) | xóa mọi chuỗi rỗng, giữ thứ tự, trả số đã xóa |

Quy tắc nền của nhóm lỗi này: không được có `&mut` trong lúc còn `&` đang được dùng[^b-borrow]; và "còn được dùng" nghĩa là cho tới **lần dùng cuối**, không phải hết scope[^nll2]. Ở `read_then_push`, câu hỏi đáng hỏi là: giá trị nào bạn thật sự cần giữ lại *sau* `push` — cả tham chiếu hay chỉ một số?

**Test có sẵn.** Sáu test trong `ex1_borrow::tests`, gồm một test kiểm `longest` giữ đúng lifetime khi một đầu vào sống ở scope trong. Doctest `compile_fail` của module bảo đảm bốn đoạn sai vẫn sai, nên bạn không thể "sửa" bằng cách sửa đề.

**Câu hỏi thêm (không có test).** (1) Vì sao `take_first` nhận `&mut [String]` chứ không phải `&mut Vec<String>`? (2) Thử bỏ `'a` ở `b` trong `longest` rồi đọc thông điệp lỗi: trình biên dịch muốn bạn khẳng định điều gì về quan hệ giữa các lifetime?

## Bài 2 — Trait, blanket impl, orphan rule, newtype (`ex2_trait`)

**Mục tiêu.** Viết được ba thứ gần như ai cũng gặp khi thiết kế thư viện: blanket impl, newtype để vượt orphan rule, và extension trait.

**Đọc trước.** [Chương 02](../09-rust-language/02-trait.md): mục 4 (blanket impl, Q4), mục 5 (orphan rule, coherence, newtype, Q5), mục 6 (dyn-compatible, Q6).

**Đề bài.**

1. **Blanket impl.** `Describe::describe` trả `«{giá trị}»` cho **mọi** `T: Display + ?Sized`. Đây là loại impl mà Book gọi là blanket implementation[^blanket].
2. **Newtype.** Bạn không thể `impl Display for Vec<i32>` ở crate này: orphan rule chỉ cho phép khi trait hoặc ít nhất một kiểu trong impl thuộc crate hiện tại[^orphan]. Cài `Display` cho `Csv(pub Vec<i32>)` (số cách nhau bằng `,`) và quan sát `Csv` tự có `describe()` nhờ blanket impl ở mục 1.
3. **Extension trait.** `JoinWith::join_with(sep)` cho mọi iterator có `Item: Display`.

Doc của module có hai đoạn sai: một đoạn vi phạm orphan rule, một đoạn để blanket impl chồng lên impl cụ thể. Reference gọi cả hai là impl *incoherent*[^incoherent]. Hãy viết ra bằng lời mình vì sao Rust cần cấm hai trường hợp đó, rồi đối chiếu với chương 02, mục 5.

**Test có sẵn.** Bốn test: blanket trên nhiều kiểu (kể cả `str` không có kích thước biên dịch); newtype; `Vec<Box<dyn Describe>>` (trait object của trait có blanket impl); `join_with` trên nhiều loại iterator, kể cả iterator rỗng.

**Mở rộng.** Thử thêm `impl Describe for Csv` thủ công khi blanket impl còn đó và đọc lỗi; rồi thử bỏ `?Sized` và xem test nào không còn biên dịch.

## Bài 3 — Generic hay `dyn`? (`ex3_dispatch`)

**Mục tiêu.** Cùng một bài toán viết bằng hai cách, rồi chỉ ra *vì sao* một số việc chỉ làm được bằng một cách.

**Đọc trước.** [Chương 03](../09-rust-language/03-generics-nang-cao.md): mục 7 (`impl Trait` ở return position, Q7), mục 8 (generic hay `dyn`, Q8); [chương 02](../09-rust-language/02-trait.md) mục 6; [chương 06](../09-rust-language/06-iterator-va-closure.md) mục 3 (`impl Fn` hay `Box<dyn Fn>`).

**Đề bài.** Sáu hàm trong `src/ex3_dispatch.rs`:

1. `total_area_static<S: Shape>(&[S])` — generic: compiler tạo một bản cho mỗi kiểu cụ thể, và biết lúc biên dịch method nào được gọi[^static].
2. `total_area_dyn(&[Box<dyn Shape>])` — các phần tử có thể khác kiểu; với trait object, Rust phải dùng dynamic dispatch[^dyn-must].
3. `largest` — phần tử diện tích lớn nhất, hòa thì lấy phần tử **đứng trước**. Chú ý: khi nhiều phần tử cùng lớn nhất, `Iterator::max_by_key` trả phần tử **cuối**[^max-last] — ngược với yêu cầu của bài, nên đừng dùng nó nguyên xi.
4. `make_shape("rect:2x5")` — chọn kiểu từ chuỗi lúc chạy. Hỏi: vì sao chữ ký này **không** viết được bằng generic?
5. `apply_pipeline(&[Box<dyn Fn(i32) -> i32>], x)` — danh sách closure khác kiểu.
6. `compose(f, g)` trả `impl Fn(i32) -> i32` — không cần `Box` vì chỉ có một kiểu cụ thể được trả về; điều kiện của `impl Trait` ở vị trí return là mọi nhánh trả cùng một kiểu[^impl-single].

Doc của module có hai đoạn sai: `vec![Square(1), Rect(1, 2)]`, và một trait có method trả `Self` đem ra làm `&dyn`. Đoạn thứ hai liên quan đến điều kiện dyn-compatible của method: method phải *dispatchable* thì trait mới dùng làm trait object được[^dyn-compat].

**Test có sẵn.** Sáu test, mỗi hàm một test; test `make_shape` kiểm sáu chuỗi sai định dạng.

**Câu hỏi thêm.** Liệt kê hai việc bạn chọn `dyn` và hai việc bạn chọn generic trong một service backend thật (nhận xét của bạn; đối chiếu chương 03 mục 8).

## Bài 4 — Tự viết `MyRc<T>` (`ex4_myrc`)

**Mục tiêu.** Cài một con trỏ đếm tham chiếu từ đầu để thấy `Deref`, `Clone`, `Drop` và interior mutability khớp vào nhau thế nào.

**Đọc trước.** [Chương 04](../09-rust-language/04-smart-pointer.md): mục 1 (`Deref`, Q1), mục 2 (`Drop`, Q2), mục 4 (`Rc`, Q4), mục 5 (`Cell`/`RefCell`, Q5), mục 7 (tự viết smart pointer, Q7).

**Đề bài.** `Rc` của std là con trỏ đếm tham chiếu **một thread**[^std-rc-def]. Bạn viết phiên bản tối giản:

- `new(value)`: cấp phát `RcBox { count: Cell<usize>, value }` trên heap với `count = 1`.
- `Clone`: tăng đếm qua `&self`. Việc này cần interior mutability: `Cell` đổi giá trị bằng cách chuyển giá trị vào/ra khỏi ô[^std-cell-moves].
- `Deref<Target = T>`.
- `Drop`: giảm đếm; về 0 thì giải phóng đúng một lần.
- `strong_count(&Self)`, `ptr_eq(&Self, &Self)`.

Đây là bài **duy nhất** cần `unsafe` (con trỏ thô tới heap, khôi phục `Box` bằng `Box::from_raw`). Đề đã nêu bất biến phải giữ trong doc của module; mỗi khối `unsafe` bạn viết hãy kèm `// SAFETY:` viết ra bất biến nào làm nó đúng. `std::rc::Rc` không `Send`[^std-rc-notsend]; doctest `compile_fail` của module kiểm rằng `MyRc` của bạn cũng không `Send` — và bạn nên tự trả lời vì sao `NonNull` làm điều đó xảy ra mà không cần `impl !Send`.

**Test có sẵn.** Sáu test: đếm theo clone/drop, `Deref`, chia sẻ cùng một allocation, `T` bị drop **đúng một lần** khi owner cuối đi (đo bằng một kiểu có `Drop` đếm), dùng với `RefCell` bên trong, và 100 clone trong `Vec`. Nếu máy bạn có Miri, chạy `cargo +nightly miri test -p lab-language ex4_` là cách tốt để tìm lỗi `unsafe` mà test thường bỏ sót (Miri không chứng minh code đúng: khi nó không báo gì, có thể bạn chỉ cần thử thêm input[^miri-limit]).

**Mở rộng.** Thêm `Weak` (đếm thứ hai), `get_mut` (chỉ khi `count == 1`), `try_unwrap`.

## Bài 5 — Kiểu lỗi riêng, `From`, `?` (`ex5_error`)

**Mục tiêu.** Thiết kế một `enum` lỗi cho thư viện, để `?` tự chuyển lỗi, giữ chuỗi nguyên nhân qua `source()`.

**Đọc trước.** [Chương 05](../09-rust-language/05-error-handling.md): mục 2 (toán tử `?`, Q2), mục 3 (trait `Error` và chain lỗi, Q3), mục 4 (`Box<dyn Error>`, Q4), mục 5 (thiết kế kiểu lỗi, Q5).

**Đề bài.** Parse file cấu hình `key=value` (quy tắc đầy đủ trong doc của `src/ex5_error.rs`). Bạn viết:

- `Display` cho `ConfigError` với thông điệp **cố định** (test kiểm chuỗi chính xác).
- `Error::source`: biến thể bọc lỗi cấp thấp phải cho phép truy cập lỗi đó qua `source()`[^err-source].
- `From<ParseIntError> for ConfigError`, để trong `parse_config` bạn viết `value.parse::<u16>()?` mà không cần `map_err`. Cơ chế: giá trị lỗi gặp `?` được đưa qua `from` của trait `From`[^book-from].
- `parse_config` — duyệt từng dòng, đánh số dòng từ 1 kể cả dòng trống/chú thích.
- `load_port` trả `Result<u16, Box<dyn Error + Send + Sync>>`: `?` đổi `ConfigError` thành hộp lỗi nhờ impl `From` của `Box` cho kiểu lỗi `Send + Sync`[^box-from]; cổng `0` bị từ chối bằng lỗi tạo từ chuỗi (`.into()`).

**Test có sẵn.** Chín test: cấu hình hợp lệ có chú thích và khoảng trắng; số dòng của `Syntax`; thứ tự báo `Missing`; `InvalidNumber` còn `source()` downcast được về `ParseIntError`; tràn `u16` và số âm; biến thể khác không có `source`; `From` độc lập; `load_port` thành công, bị từ chối, và `downcast_ref::<ConfigError>()` sau khi đi qua `Box<dyn Error>`.

**Câu hỏi thêm.** Vì sao `ConfigError::Missing` mang `&'static str` mà không phải `String`? Khi nào lựa chọn đó thành gánh nặng?

## Bài 6 — Iterator adapter tự viết (`ex6_iter`)

**Mục tiêu.** Hiểu adapter của std là struct + `impl Iterator` thông thường, không phép màu; viết adapter **lười** và kèm `size_hint` đúng.

**Đọc trước.** [Chương 06](../09-rust-language/06-iterator-va-closure.md): mục 4 (`for`, ba dạng duyệt, lười, `collect`, Q4), mục 5 (zero-cost, Q5).

**Đề bài.** Hai adapter `Dedup` (bỏ phần tử bằng phần tử liền trước) và `Pairs` (cặp liên tiếp chồng lấn). Std mô tả hai bước tạo iterator riêng: một struct giữ trạng thái và `impl Iterator` cho nó[^it-impl]. Struct và extension trait (blanket impl `impl<I: Iterator> IterExt for I {}`) đã có sẵn; bạn viết hai `impl Iterator`.

Yêu cầu về tính lười: iterator và adapter đều lười, không làm gì cho tới khi được tiêu thụ[^it-lazy]. Test đo điều này bằng `inspect` đếm số phần tử bị kéo ra khỏi iterator gốc.

Yêu cầu về `size_hint`: cận dưới `0` và cận trên bằng cận trên của iterator gốc. Mặc định của `size_hint` là `(0, None)`, đúng với mọi iterator[^it-sizehint] — test của bài đòi chặt hơn mặc định, vì bạn biết thêm một điều về `Dedup`: nó không bao giờ trả nhiều phần tử hơn iterator gốc.

**Test có sẵn.** Tám test: dedup chỉ bỏ trùng *liên tiếp* (`1,1,2,2,2,3,1,1` → `1,2,3,1`), biên (rỗng, một phần tử, toàn trùng), tính lười, `size_hint`, `pairs` và biên của nó, ghép với `map`/`sum`, dùng `by_ref` + `take` giữa chừng.

**Mở rộng.** Viết `Pairs` không đòi `Clone` (gợi ý: đổi `Item` sang tham chiếu hoặc dùng `Peekable`); thêm `DoubleEndedIterator` cho `Pairs` trên nguồn `DoubleEndedIterator + ExactSizeIterator` và tự nghĩ test.

## Bài 7 — `macro_rules!` nhỏ (`ex7_macro`)

**Mục tiêu.** Viết macro có repetition và đệ quy; nắm hygiene và `$crate`.

**Đọc trước.** [Chương 07](../09-rust-language/07-macro.md): mục 2 (matcher, fragment, repetition, Q2), mục 3 (hygiene, `$crate`, `#[macro_export]`, Q3), mục 7 (macro quen thuộc nhìn từ bên trong, Q7).

**Đề bài.** Ba macro; stub hiện trả giá trị sai để test đỏ nhưng vẫn biên dịch:

1. `count_exprs!(a, b, c)` → `3usize`. Không được đánh giá biểu thức (test truyền `panic!(..)` vào). Dùng đệ quy trên `$head:expr $(, $tail:expr)*`.
2. `hashmap!{ k => v, … }` có dấu phẩy cuối tùy chọn và dạng rỗng. Hình mẫu để đối chiếu: luật của `vec!` trong std có cùng dạng lặp `$(,)?`[^std-vec-rules]. Dùng `HashMap::with_capacity($crate::count_exprs!(…))`; `$crate` là keyword metavariable trỏ tới crate hiện tại[^ref-dollar-crate].
3. `max_of!(a, b, …)` — mỗi đối số đánh giá **đúng một lần, từ trái qua phải**. Gợi ý: bind vào biến cục bộ. Với macro by example, biến cục bộ được tra theo vị trí *định nghĩa* macro chứ không theo vị trí gọi[^ref-hygiene], nên biến `a`/`b` bên trong macro không đè biến cùng tên của người gọi; test `max_of_is_hygienic_with_callers_variables` đặt đúng tên đó để bạn tự thấy.

Để các macro dùng được bằng đường dẫn `crate::tên!` từ test, chúng có `#[macro_export]`, thuộc tính đưa macro lên gốc crate cho path-based resolution[^ref-macro-export-root].

**Test có sẵn.** Tám test; mọi test gọi macro bằng đường dẫn `crate::…!`. Doctest `compile_fail` trong doc module minh họa hygiene: biến `let x` do macro tạo ra không nhìn thấy được từ code gọi.

**Mở rộng.** Thêm `btreemap!` dùng chung phần lặp; đọc code sau khi expand bằng `cargo expand` (cần nightly và cài `cargo-expand`; xem chương 07 mục 6).

## Bài 8 — Cargo features và workspace (`ex8_features`)

**Mục tiêu.** Dùng `cfg!` và feature đúng kiểu *additive*; nhìn feature đi qua workspace bằng `cargo tree`.

**Đọc trước.** [Chương 08](../09-rust-language/08-module-crate-cargo.md): mục 4 (conditional compilation, Q4), mục 6 (features và unification, Q6), mục 7 (workspace và profile, Q7).

**Đề bài (code).** `Cargo.toml` của `lab-language` có hai feature: `solution` (đã giải thích ở trên) và `shout`. Mặc định mọi feature tắt trừ khi được bật rõ ràng[^featdefault]. Bạn viết:

- `enabled_features()` bằng `cfg!(feature = "…")` — macro này cho biểu thức bool tại thời điểm biên dịch[^cfgmacro]. Test kiểm danh sách trả về khớp với cấu hình thật.
- Hàm `shout` chỉ tồn tại khi bật `shout`. Feature được thiết kế **additive**: bật thêm một feature chỉ nên thêm chức năng, không đổi hay bỏ chức năng sẵn có[^additive]. `shout` tuân theo: nó thêm một hàm, không đổi hàm nào.

Chạy ba cấu hình và để ý test nào được biên dịch trong mỗi cấu hình:

```bash
cd labs
cargo test -p lab-language --features solution
cargo test -p lab-language --features solution,shout
cargo test -p lab-language --all-features
```

**Đề bài (workspace, không có test chấm).** Workspace là tập các package được quản lý chung[^wsdef], và các member kế thừa khóa chung bằng `{key}.workspace = true`[^wspkg] — `Cargo.toml` của `lab-language` đã dùng cho `edition`, `rust-version`, `publish`. Thực hành, rồi **xóa** thư mục tạo ra khi xong (vì `labs/Cargo.toml` có `members = ["crates/*"]`, mọi thư mục mới dưới `crates/` tự thành member):

1. Tạo `labs/crates/lab-language-demo` (một `cargo new --lib` rồi sửa manifest cho kế thừa workspace giống `lab-language`), thêm dependency `lab-language = { path = "../lab-language", features = ["shout"] }`.
2. Chạy `cargo tree -p lab-language-demo -e features`, rồi `cargo tree -e features -i lab-language` từ gốc workspace; `-i` giúp thấy feature chảy vào package đó từ đâu[^treefeat].
3. Dự đoán rồi kiểm: nếu một member khác phụ thuộc `lab-language` **không** bật `shout`, lúc build cả workspace thì `shout` có được biên dịch không? Chương 08 mục 6 mô tả unification: khi nhiều dependent cùng yêu cầu một package, package được build với **hợp** các feature[^featunion]. Hãy tự suy ra vì sao feature phải additive.

## Câu hỏi tự kiểm tra

1. Trong `read_then_push`, vì sao lấy `*v.first()?` (một `i32`) sửa được lỗi còn giữ `&v[0]` thì không?
   <details><summary>Đáp án</summary>

   Không được có `&mut` trong lúc `&` còn được dùng[^b-borrow]; borrow sống đến lần dùng cuối[^nll2]. Sao chép `i32` ra (Copy) làm tham chiếu chia sẻ chết trước `push`.

   </details>

2. Vì sao `impl Display for Vec<i32>` bị từ chối, và newtype giải quyết thế nào?
   <details><summary>Đáp án</summary>

   Orphan rule: impl chỉ được phép khi trait hoặc ít nhất một kiểu thuộc crate hiện tại[^orphan]; `Display` và `Vec<i32>` đều ngoài crate. `Csv` là kiểu cục bộ nên `impl Display for Csv` hợp lệ.

   </details>

3. Vì sao `make_shape` phải trả `Box<dyn Shape>` chứ không viết được bằng generic?
   <details><summary>Đáp án</summary>

   Generic được monomorphize lúc biên dịch với kiểu cụ thể cho mỗi nơi gọi[^static], còn kiểu kết quả của `make_shape` phụ thuộc nội dung chuỗi lúc chạy; chỉ trait object (dynamic dispatch[^dyn-must]) che được kiểu cụ thể.

   </details>

4. Vì sao `MyRc::clone` chỉ cần `&self` mà vẫn tăng được bộ đếm?
   <details><summary>Đáp án</summary>

   Bộ đếm nằm trong `Cell`, kiểu interior mutability đổi giá trị bằng cách chuyển giá trị vào/ra khỏi ô[^std-cell-moves] nên không cần `&mut`.

   </details>

5. Dòng `value.parse::<u16>()?` trong hàm trả `Result<_, ConfigError>` biên dịch được nhờ gì?
   <details><summary>Đáp án</summary>

   `?` đưa giá trị lỗi qua `from` của trait `From`[^book-from], nên cần `impl From<ParseIntError> for ConfigError`.

   </details>

6. Vì sao hai adapter của bài 6 không làm gì khi chỉ được tạo mà chưa gọi `next`?
   <details><summary>Đáp án</summary>

   Iterator và adapter đều lười: chúng không làm gì cho tới khi được tiêu thụ[^it-lazy]; mọi công việc nằm trong `next`.

   </details>

7. Vì sao biến `let` bên trong `max_of!` không đè biến `a` của người gọi?
   <details><summary>Đáp án</summary>

   Macro by example tra biến cục bộ tại vị trí định nghĩa macro, không phải vị trí gọi[^ref-hygiene].

   </details>

8. Vì sao feature `shout` chỉ nên thêm hàm mà không đổi hành vi hàm có sẵn?
   <details><summary>Đáp án</summary>

   Feature nên additive: bật thêm feature không được làm mất hay đổi chức năng, để bật tổ hợp bất kỳ đều an toàn[^additive]; và nhiều dependent sẽ cùng yêu cầu feature khác nhau rồi bị hợp lại[^featunion].

   </details>

[^b-borrow]: Rust Book, References and Borrowing — <https://doc.rust-lang.org/1.95.0/book/ch04-02-references-and-borrowing.html> — "We also cannot have a mutable reference while we have an immutable one"
[^nll2]: Rustonomicon, Lifetimes — <https://doc.rust-lang.org/1.95.0/nomicon/lifetimes.html> — "A reference (sometimes called a borrow) is alive from the place it is created to its last use."
[^blanket]: Rust Book, ch10-02 Traits — <https://doc.rust-lang.org/1.95.0/book/ch10-02-traits.html> — "Implementations of a trait on any type that satisfies the trait bounds are called blanket implementations and are used extensively in the Rust standard library."
[^orphan]: Rust Reference, Implementations — <https://doc.rust-lang.org/1.95.0/reference/items/implementations.html> — "The orphan rule states that a trait implementation is only allowed if either the trait or at least one of the types in the implementation is defined in the current crate."
[^incoherent]: Rust Reference, Implementations — <https://doc.rust-lang.org/1.95.0/reference/items/implementations.html> — "A trait implementation is considered incoherent if either the orphan rules check fails or there are overlapping implementation instances."
[^static]: Rust Book, ch18-02 Trait objects — <https://doc.rust-lang.org/1.95.0/book/ch18-02-trait-objects.html> — "The code that results from monomorphization is doing static dispatch, which is when the compiler knows what method you’re calling at compile time."
[^dyn-must]: Rust Book, ch18-02 Trait objects — <https://doc.rust-lang.org/1.95.0/book/ch18-02-trait-objects.html> — "When we use trait objects, Rust must use dynamic dispatch."
[^impl-single]: Rust Reference, Impl trait type — <https://doc.rust-lang.org/1.95.0/reference/types/impl-trait.html> — "Each possible return value from the function must resolve to the same concrete type."
[^dyn-compat]: Rust Reference, Traits (dyn compatibility) — <https://doc.rust-lang.org/1.95.0/reference/items/traits.html> — "Dispatchable functions must: Not have any type parameters (although lifetime parameters are allowed). Be a method that does not use Self except in the type of the receiver."
[^std-rc-def]: std::rc::Rc — <https://doc.rust-lang.org/1.95.0/std/rc/struct.Rc.html> — "A single-threaded reference-counting pointer."
[^std-cell-moves]: std::cell — <https://doc.rust-lang.org/1.95.0/std/cell/index.html> — "Cell\<T> implements interior mutability by moving values in and out of the cell."
[^std-rc-notsend]: std::rc::Rc, Trait Implementations — <https://doc.rust-lang.org/1.95.0/std/rc/struct.Rc.html> — "impl\<T, A> !Send for Rc\<T, A> where A: Allocator, T: ?Sized"
[^err-source]: Rust std, trait `Error` (Error source) — <https://doc.rust-lang.org/1.95.0/std/error/trait.Error.html> — "If one module must report an error that is caused by an error from a lower-level module, it can allow accessing that error via Error::source()."
[^book-from]: Rust Book, ch09-02 The ? Operator Shortcut — <https://doc.rust-lang.org/1.95.0/book/ch09-02-recoverable-errors-with-result.html> — "Error values that have the ? operator called on them go through the from function, defined in the From trait in the standard library, which is used to convert values from one type into another."
[^box-from]: Rust std, `Box` (impl From\<E> for Box\<dyn Error + Sync + Send>) — <https://doc.rust-lang.org/1.95.0/std/boxed/struct.Box.html> — "Converts a type of Error + Send + Sync into a box of dyn Error + Send + Sync."
[^it-impl]: std::iter — <https://doc.rust-lang.org/1.95.0/std/iter/index.html> — "Creating an iterator of your own involves two steps: creating a struct to hold the iterator’s state, and then implementing Iterator for that struct."
[^it-lazy]: std::iter — <https://doc.rust-lang.org/1.95.0/std/iter/index.html> — "Iterators (and iterator adapters) are lazy."
[^it-sizehint]: std::iter::Iterator, size_hint — <https://doc.rust-lang.org/1.95.0/std/iter/trait.Iterator.html> — "The default implementation returns (0, None) which is correct for any iterator."
[^std-vec-rules]: Rust std 1.95.0, macro vec — <https://doc.rust-lang.org/1.95.0/std/macro.vec.html> — "($elem:expr; $n:expr) => { ... }; ($($x:expr),+ $(,)?) => { ... };"
[^ref-dollar-crate]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "The keyword metavariable $crate can be used to refer to the current crate."
[^ref-hygiene]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "Macros by example have mixed-site hygiene. This means that loop labels, block labels, and local variables are looked up at the macro definition site while other symbols are looked up at the macro invocation site."
[^ref-macro-export-root]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "The macro_export attribute exports the macro from the crate and makes it available in the root of the crate for path-based resolution."
[^featdefault]: Cargo Book 1.95.0, Features — <https://doc.rust-lang.org/1.95.0/cargo/reference/features.html> — "By default, all features are disabled unless explicitly enabled."
[^cfgmacro]: Rust Reference 1.95.0, Conditional compilation — <https://doc.rust-lang.org/1.95.0/reference/conditional-compilation.html> — "evaluates to the true literal when the predicate is true and the false literal when it is false."
[^additive]: Cargo Book 1.95.0, Features — <https://doc.rust-lang.org/1.95.0/cargo/reference/features.html> — "A consequence of this is that features should be additive. That is, enabling a feature should not disable functionality, and it should usually be safe to enable any combination of features."
[^wsdef]: Cargo Book 1.95.0, Workspaces — <https://doc.rust-lang.org/1.95.0/cargo/reference/workspaces.html> — "A workspace is a collection of one or more packages, called workspace members, that are managed together."
[^wspkg]: Cargo Book 1.95.0, Workspaces — <https://doc.rust-lang.org/1.95.0/cargo/reference/workspaces.html> — "These keys can be inherited by defining them in the member package with {key}.workspace = true."
[^treefeat]: Cargo Book 1.95.0, cargo-tree — <https://doc.rust-lang.org/1.95.0/cargo/commands/cargo-tree.html> — "When using -e features it can be helpful to use -i flag to show how the features flow into a package."
[^featunion]: Cargo Book 1.95.0, Features — <https://doc.rust-lang.org/1.95.0/cargo/reference/features.html> — "then winapi will be built with all four of those features enabled."
[^max-last]: std::iter::Iterator, max_by_key — <https://doc.rust-lang.org/1.95.0/std/iter/trait.Iterator.html> — "If several elements are equally maximum, the last element is returned."
[^miri-limit]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "When Miri finds UB, your code is definitely unsound, but when Miri does not find UB, then you may just have to test more inputs or more possible non-deterministic choices."
