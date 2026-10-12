# P9 · Chương 07 — Macro

## Bạn sẽ hiểu được gì

1. **Q1.** Vì sao Rust có macro khi đã có hàm, generic và trait; macro làm được gì mà hàm không làm được? → mục 1
2. **Q2.** `macro_rules!` hoạt động thế nào: matcher, fragment specifier (`expr`, `ident`, `ty`, `tt`…), repetition `$(...)*`? → mục 2
3. **Q3.** "Hygiene" nghĩa là gì; `$crate`, `#[macro_export]` và thứ tự định nghĩa (textual scope) quyết định macro nào gọi được ở đâu? → mục 3
4. **Q4.** Ba loại procedural macro khác nhau ra sao; vì sao phải nằm trong crate riêng? → mục 4
5. **Q5.** `syn` và `quote` làm gì trong một derive macro; một derive macro nhỏ chạy thật trông thế nào? → mục 5
6. **Q6.** Macro được expand ở giai đoạn nào của compiler, và xem code sau khi expand bằng cách nào? → mục 6
7. **Q7.** `vec!`, `println!`, `#[derive(Debug)]`, `#[tokio::main]` thực chất làm gì? → mục 7
8. **Q8.** serde derive và sqlx `query!` làm gì, và cái giá phải trả của macro là gì? → mục 8

## Kiến thức cần trước

Chương này đứng độc lập về cú pháp; nên đã quen với trait và generic (xem [02-trait](02-trait.md), [03-generics-nang-cao](03-generics-nang-cao.md) — cùng phase P9). Phần "macro expand ở đâu trong pipeline compiler" chỉ nêu mức đủ dùng; chi tiết xem chương pipeline rustc ở P10·01 (`docs/10-rust-internals/01-…`, viết sau).

Mọi ví dụ trong chương được chạy thật với `rustc`/`cargo` 1.95.0 trên macOS (aarch64). Phần xem code đã expand dùng `cargo expand` 1.0.118, vốn cần nightly (nightly đã có trên máy).

## 1. Vì sao Rust cần macro (Q1)

> **Trả lời ngắn.** Hàm phải khai báo trước số lượng và kiểu tham số, và chạy lúc runtime. Macro được mở rộng thành code trước khi compiler hiểu nghĩa code, nên nhận được số tham số tùy ý và sinh được cả `impl`[^book-variadic][^book-expand-before]. Macro cũng giúp viết một lần thay cho nhiều đoạn code lặp[^book-meta].

**Giải thích.**

Hình dung (nhận xét của người soạn): hàm giống biểu mẫu có sẵn các ô cố định; macro giống người soạn văn bản mẫu — viết ra đoạn code đúng chỗ rồi compiler mới đọc đoạn code đó.

*Số tham số.* Chữ ký hàm phải khai báo số lượng và kiểu tham số[^book-fn-sig]; macro thì nhận số tham số thay đổi, ví dụ `println!("hello")` một đối số hoặc `println!("hello {}", name)` hai đối số[^book-variadic]. Vì vậy `println!` là macro (suy luận của người soạn từ ví dụ của Book).

*Sinh item.* Macro được expand trước khi compiler diễn giải nghĩa của code nên có thể implement trait cho một kiểu[^book-expand-before]. Hàm không làm được vì hàm được gọi lúc runtime còn trait phải được implement lúc compile[^book-fn-runtime]. Nghĩa là macro sinh được *item* (struct, impl, fn), không chỉ giá trị (suy luận của người soạn).

*Bớt code lặp.* Book định nghĩa macro là cách viết code sinh ra code khác, gọi là metaprogramming[^book-meta]. Ví dụ minh họa (người soạn): 12 kiểu số nguyên cần cùng một `impl` — viết một macro rồi gọi 12 lần thay vì copy-paste.


<details><summary>Đi sâu: hai họ macro và bảng so sánh</summary>

Book chia macro thành hai họ: declarative macro (`macro_rules!`) và ba loại procedural macro — custom `#[derive]`, attribute-like, function-like.[^book-three-kinds]

| | `macro_rules!` (declarative) | Procedural |
|---|---|---|
| Cách viết | Các luật `matcher => transcriber`, giống `match` | Một hàm Rust nhận và trả `TokenStream` |
| Nơi định nghĩa | Cùng crate được | Phải trong crate riêng kiểu `proc-macro` |
| Sức mạnh | Pattern-matching trên token | Chạy code Rust tùy ý trên token |
| Phụ thuộc ngoài | Không | Thường `syn` + `quote` |

</details>

**Quay lại Q1:** hàm cố định chữ ký và chạy lúc runtime; macro sinh code lúc compile nên linh hoạt hơn về số tham số và về thứ sinh ra.

## 2. `macro_rules!`: matcher, fragment, repetition (Q2)

> **Trả lời ngắn.** Một macro `macro_rules!` là danh sách luật; mỗi luật gồm khuôn để khớp (matcher) và khuôn để sinh code (transcriber), compiler thử lần lượt và dùng luật đầu tiên khớp[^ref-transcribe]. Trong khuôn khớp, `$tên:loại` bắt một mảnh cú pháp thuộc đúng *loại* đó (biểu thức, định danh, kiểu…) và gán vào tên[^ref-metavar]. Lặp `$( ... ),*` bắt nhiều mảnh cùng dạng[^ref-repetition-sep].

**Giải thích.**

Hình dung (nhận xét của người soạn): như mẫu thư mail-merge — chỗ trống `$tên` được điền bằng phần người gọi truyền vào, và mẫu có thể có nhiều phiên bản cho các dạng yêu cầu khác nhau.

*Ví dụ đầu tiên: `max` nhiều đối số.*

```rust
macro_rules! maxm {
    ($x:expr) => ($x);
    ($x:expr, $($rest:expr),+) => {{
        let a = $x;
        let b = maxm!($($rest),+);
        if a > b { a } else { b }
    }};
}
fn main() { println!("{}", maxm!(3, 9, 4)); }
```

Chạy thật: in `9`. Macro có hai luật; compiler thử từng luật từ trên xuống, luật đầu khớp thì dùng[^ref-transcribe]. Luật 2 gọi lại chính macro (đệ quy) với phần đuôi. Reference ghi rằng macro vẫn có thể được dùng đệ quy, vì tên được tra từ nơi gọi.[^ref-textual-scope]

*Matcher và fragment specifier (loại mảnh cú pháp).*

Trong matcher, `$name:fragment` khớp một mảnh cú pháp thuộc loại `fragment` và gán vào metavariable `$name`.[^ref-metavar] Danh sách của Reference gồm `block`, `expr`, `ident`, `item`, `lifetime`, `literal`, `meta`, `pat`, `path`, `stmt`, `tt`, `ty`, `vis`…[^ref-fragspec-list] Vài loại hay dùng:

| Specifier | Khớp | Theo Reference |
|---|---|---|
| `expr` | một biểu thức | một Expression[^ref-expr] |
| `ident` | một định danh | định danh hoặc từ khóa, trừ `_`, raw identifier và `$crate`[^ref-ident] |
| `ty` | một kiểu | một Type[^ref-ty] |
| `tt` | một token tree | một token đơn, hoặc các token nằm trong cặp ngoặc khớp nhau `()`, `[]`, `{}`[^ref-tt] |

`tt` khớp một token đơn hoặc cả cụm trong cặp ngoặc[^ref-tt]; nhờ vậy hay dùng để chuyển tiếp token nguyên xi cho macro khác (nhận xét của người soạn).

Ví dụ dùng `ident` và `ty` để sinh hàm:

```rust
macro_rules! make_fn {
    ($name:ident, $t:ty, $v:expr) => { fn $name() -> $t { $v } };
}
make_fn!(answer, u32, 42);
fn main() { println!("{}", answer()); }   // in 42
```

(Chạy thật: in `42`.) Đây là điều hàm không làm được: tên hàm `answer` được *tạo ra* từ đối số.

*Repetition (lặp).*

Lặp được viết bằng `$( ... )` theo sau là toán tử lặp, có thể kèm dấu phân cách: `$( $i:ident ),*` biểu diễn một số lượng bất kỳ định danh ngăn cách bằng dấu phẩy.[^ref-repetition-sep] Các toán tử: `*` (không hoặc nhiều), `+` (một hoặc nhiều[^ref-plus]), `?` (một mảnh tùy chọn, xuất hiện không hoặc một lần[^ref-opt]).

<details><summary>Đi sâu: ràng buộc số lần lặp, ví dụ <code>hashmap!</code>, khác biệt edition 2024</summary>

Reference còn đặt ràng buộc: một metavariable phải xuất hiện trong transcriber với đúng số lượng, loại và độ lồng của lặp như trong matcher.[^ref-rep-rules] Ví dụ cụ thể, `hashmap!`:

```rust
macro_rules! hashmap {
    ($($k:expr => $v:expr),* $(,)?) => {{
        let mut m = ::std::collections::HashMap::new();
        $( m.insert($k, $v); )*
        m
    }};
}
fn main() {
    let m = hashmap!{"a" => 1, "b" => 2,};
    println!("{}", m.len());   // in 2
}
```

Chạy thật: in `2`. Dấu `,` cuối được chấp nhận nhờ `$(,)?`. Chú ý đường dẫn đầy đủ `::std::collections::HashMap` trong transcriber: macro sẽ được gọi ở nơi người dùng có thể đã `use` một thứ khác tên `HashMap`, nên dùng đường dẫn tuyệt đối là thói quen an toàn (nhận xét của người soạn). Với macro export ra crate khác, chuyện này giải quyết bằng `$crate` (mục 3).

Một lưu ý về phiên bản: Reference nêu rằng trước edition 2024, `expr` không khớp `_` và const block ở cấp cao nhất.[^ref-expr2024] Nếu macro của bạn dùng `expr` và hành xử khác giữa các edition, đây là chỗ cần kiểm tra.

</details>

**Quay lại Q2:** luật = matcher + transcriber, thử từ trên xuống; fragment specifier định loại mảnh bắt được; `$(...)sep op` bắt nhiều mảnh.

## 3. Hygiene, `$crate`, `#[macro_export]` và textual scope (Q3)

> **Trả lời ngắn.** Hygiene của `macro_rules!` giữ biến cục bộ và nhãn (label) trong macro tách khỏi của người gọi; các tên khác như hàm, kiểu, trait vẫn được tra tại nơi gọi[^ref-hygiene]. Vì thế macro của thư viện dùng `$crate` để trỏ về crate hiện tại[^ref-dollar-crate], còn `#[macro_export]` đổi visibility ngầm định từ `pub(crate)` sang `pub`[^ref-macro-export]. Macro `macro_rules!` chỉ dùng được sau chỗ định nghĩa vì scope mặc định tính theo thứ tự trong file[^ref-textual-scope].

**Giải thích.**

Hình dung (nhận xét của người soạn): hai người dùng chung một bảng trắng, mỗi người một màu bút — hai chữ `x` khác màu không bao giờ bị nhầm là một.

*Hygiene: biến trong macro không đụng biến ngoài.*

Reference nói macro-by-example có hygiene kiểu "mixed-site": nhãn vòng lặp, nhãn block và biến cục bộ được tra tại nơi định nghĩa macro, còn mọi ký hiệu khác được tra tại nơi gọi.[^ref-hygiene] Tức là biến cục bộ `x` mà macro khai báo *không* trùng với `x` người gọi.

```rust
macro_rules! hyg {
    ($e:expr) => {{ let x = 100; $e + x }};
}
fn main() {
    let x = 1;
    println!("{}", hyg!(x * 2));
}
```

Chạy thật: in `102`. Biểu thức `x * 2` của người gọi dùng `x = 1` của người gọi (cho 2), còn `x` trong `$e + x` là `x = 100` do macro khai báo: 2 + 100 = 102. Nếu macro không hygienic, `let x = 100` sẽ che `x` của người gọi và kết quả là 300 (suy luận số học của người soạn, chưa chạy thử).

Chiều ngược lại: biến `let` bên trong macro cũng không "rò" ra ngoài.

```rust
macro_rules! bad { () => { let x = 100; } }
fn main() { bad!(); println!("{}", x); }
```

Chạy thật, compile lỗi `E0425: cannot find value 'x' in this scope`, kèm gợi ý "an identifier with the same name is defined here, but is not accessible due to macro hygiene". Muốn macro tạo biến mà người gọi thấy được, tên phải do người gọi truyền vào dưới dạng `ident`:

```rust
macro_rules! show_ident { ($i:ident) => { let $i = 5; }; }
fn main() { show_ident!(z); println!("{}", z); }   // in 5
```

(Chạy thật: in `5`.) Hygiene của `macro_rules!` chỉ áp dụng cho biến cục bộ và label; mọi ký hiệu khác được tra tại nơi gọi[^ref-hygiene]: tên hàm, kiểu, trait… cũng vậy. Vì vậy mới cần `$crate`.

*`$crate` và `#[macro_export]`.*

Từ khóa metavariable `$crate` dùng để trỏ tới crate hiện tại[^ref-dollar-crate]. Trong macro của thư viện, viết `$crate::path::item` để trỏ tới item của crate chứa định nghĩa macro (cách dùng điển hình — nhận xét của người soạn; Reference chỉ nêu `$crate` trỏ tới crate hiện tại).

Về visibility: macro có visibility ngầm định là `pub(crate)`, và `#[macro_export]` đổi nó thành `pub`.[^ref-macro-export] Ví dụ trong một crate:

```rust
mod inner {
    #[macro_export]
    macro_rules! helper_pub { () => { $crate::inner::f() }; }
    pub fn f() -> i32 { 7 }
}
fn main() { println!("{}", helper_pub!()); }   // in 7
```

Chạy thật: in `7`. Dù `helper_pub` định nghĩa trong `mod inner`, `#[macro_export]` làm nó có mặt ở gốc crate để phân giải theo đường dẫn[^ref-macro-export-root], nên dependency gọi là `tên_crate::helper_pub!()` (suy luận của người soạn từ câu đó); `$crate::inner::f()` vẫn trỏ đúng vị trí thật của `f`.

*Thứ tự định nghĩa: textual scope.*

Reference: macro-by-example có hai dạng scope, textual và path-based; textual scope dựa trên thứ tự các thứ xuất hiện trong file nguồn và là scope mặc định.[^ref-scope-two] Với `macro_rules!`, macro vào scope sau chỗ định nghĩa, cho đến khi scope bao quanh (thường là module) đóng lại.[^ref-textual-scope] Khi gọi bằng tên trơn, compiler tra textual scope trước rồi mới tới path-based scope.[^ref-lookup-order]

Hệ quả thực tế: gọi macro *trước* chỗ định nghĩa thì lỗi.

```rust
fn main(){ early!(); }
macro_rules! early { () => {} }
```

Chạy thật, compile lỗi: `cannot find macro 'early' in this scope`, với gợi ý "consider moving the definition of 'early' before this call". Khác với `fn`, thứ tự khai báo macro quan trọng.

<details><summary>Đi sâu: <code>#[macro_use]</code> và re-export</summary>

Để macro tồn tại qua ranh giới module: `#[macro_use]` trên module mở rộng scope của các macro định nghĩa bên trong ra ngoài module; trên `extern crate` thì nhập macro của crate khác vào `macro_use` prelude.[^ref-macro-use] Cách khác là `#[macro_export]` rồi `use crate::tên_macro;` hoặc gọi theo đường dẫn (thường gọn hơn — nhận xét của người soạn); Reference cũng nói macro có thể được re-export để có path-based scope từ module khác ngoài gốc crate.[^ref-reexport]

</details>

**Quay lại Q3:** hygiene chỉ che biến cục bộ và nhãn; tên còn lại tra tại nơi gọi nên cần `$crate`; macro dùng được từ sau chỗ định nghĩa, hoặc qua đường dẫn nếu `#[macro_export]`.

## 4. Procedural macro: ba loại và crate riêng (Q4)

> **Trả lời ngắn.** Procedural macro là một hàm Rust chạy lúc compile[^ref-proc-run], làm việc trên dãy token (`TokenStream`) thay vì AST[^ref-tokenstream]. Có ba loại — derive, attribute, function-like[^book-three-kinds] — và hàm phải nằm ở gốc crate kiểu `proc-macro`, không dùng được ngay trong crate định nghĩa nó[^ref-proc-root].

**Giải thích.**

Hình dung (nhận xét của người soạn): `macro_rules!` giống bộ lọc theo mẫu; procedural macro giống một script chạy trong compiler, đọc code của bạn rồi in ra code mới.

Khác với việc khớp pattern, procedural macro nhận và trả `TokenStream`[^ref-tokenstream]; ba loại là custom derive, attribute-like, function-like và Book nói chúng hoạt động theo cách tương tự nhau[^book-three-kinds].

*Phải nằm trong crate riêng.*

Book nói định nghĩa của procedural macro phải nằm trong crate riêng với một crate type đặc biệt.[^book-own-crate] Reference nói cụ thể hơn: procedural macro phải được định nghĩa ở gốc crate có crate type `proc-macro`; chúng không dùng được trong chính crate định nghĩa, chỉ dùng được khi được import vào crate khác.[^ref-proc-root] Với Cargo, đặt trong `Cargo.toml`:

```toml
[lib]
proc-macro = true
```

[^ref-proc-cargo]

Vì thế một dự án có derive macro riêng thường là workspace hai crate: `foo` và `foo_derive` (nhận xét của người soạn, theo cấu trúc của ví dụ bên dưới và của Book).

*Ba loại.*

| Loại | Cách dùng | Hàm nhận / trả |
|---|---|---|
| Function-like | `my_macro!(...)` | Nhận nội dung trong ngoặc, thay toàn bộ lời gọi bằng kết quả[^ref-fnlike] |
| Derive | `#[derive(MyTrait)]` | Nhận token của struct/enum/union, có thể phát ra item mới đặt sau nó[^ref-derive] |
| Attribute | `#[my_attr(...)]` trên item | Tham số 1 là token trong ngoặc của attribute; tham số 2 là phần còn lại của item, kể cả các attribute khác trên item[^ref-attr-second] |

Khác biệt quan trọng: derive không *thay thế* item gốc, chỉ thêm item mới sau nó[^ref-derive]; Reference nói syntax trả về thay thế hoặc thêm tùy loại macro[^ref-proc-result], nên attribute macro nhận cả item và trả về thứ thay cho nó (suy luận của người soạn từ hai tham số và câu đó).

<details><summary>Đi sâu: <code>TokenStream</code>, <code>Span</code> và hygiene của procedural macro</summary>

Procedural macro làm việc trên `TokenStream` chứ không phải AST: Reference giải thích đó là giao diện ổn định hơn nhiều theo thời gian cho cả compiler lẫn procedural macro.[^ref-tokenstream] Mỗi token có `Span` — vị trí trong mã nguồn, dùng chủ yếu cho báo lỗi.[^ref-span]

Hygiene của procedural macro *khác* `macro_rules!`: chúng không hygienic, tức hành xử như thể token đầu ra được viết thẳng vào chỗ gọi[^ref-proc-hygiene]. Nên macro sinh `let tmp = ...` có thể đụng biến của người dùng; docs của `quote` nhắc `quote_spanned!` là quan trọng để viết procedural macro hygienic[^quote-spanned] (hướng dẫn chi tiết thuộc docs của chúng).

</details>

**Quay lại Q4:** procedural macro là hàm Rust chạy trong compiler trên `TokenStream`, có ba loại, và bắt buộc ở crate `proc-macro` riêng.

## 5. `syn`, `quote` và một derive macro chạy thật (Q5)

> **Trả lời ngắn.** `syn` đọc dãy token đầu vào thành cây cú pháp có cấu trúc; với derive, đầu vào được biểu diễn bằng kiểu `DeriveInput`[^syn-deriveinput]. `quote!` cho viết code mẫu rồi chèn biến vào bằng `#var` để sinh dãy token đầu ra[^quote-interp]. Ví dụ `FieldNames` bên dưới ghép hai thứ đó thành một derive macro nhỏ.

**Giải thích.**

Hình dung (nhận xét của người soạn): `syn` giống bộ parse JSON ra struct để bạn đọc field; `quote!` giống template engine điền biến vào mẫu rồi in ra chuỗi code.

*`syn`, `quote`, `proc-macro2`.*

Ba crate hay đi cùng nhau (phiên bản dùng trong ví dụ dưới, theo `Cargo.lock` thật: `syn 2.0.119`, `quote 1.0.47`, `proc-macro2 1.0.107`):

- **`syn`** parse `TokenStream` thành cây cú pháp: docs nói Syn cung cấp một cây cú pháp đầy đủ, biểu diễn được mọi mã Rust hợp lệ.[^syn-tree] Với derive, kiểu đáng chú ý là `syn::DeriveInput`, đại diện cho một trong ba loại item hợp lệ làm đầu vào của derive macro.[^syn-deriveinput] Nó cũng được feature-gate rất chặt, để macro không phải trả chi phí compile cho phần còn lại.[^syn-features]
- **`quote`** sinh token: ý tưởng của quasi-quoting là viết code rồi xem nó như dữ liệu.[^quote-idea] Trong `quote!`, cú pháp `#var` chèn biến (lúc chạy macro) vào dãy token được trích.[^quote-interp]
- **`proc-macro2`** bọc API `proc_macro` của compiler, vì kiểu của `proc_macro` chỉ tồn tại trong procedural macro còn kiểu `proc_macro2` có thể tồn tại ở mọi nơi[^pm2]. Docs của nó nói lý do này giúp helper library test được độc lập[^pm2-test].

*Một derive macro chạy thật: `FieldNames`.*

Mục tiêu: `#[derive(FieldNames)]` thêm hằng `FIELD_NAMES: &[&str]` liệt kê tên field của struct.

Crate `mymac_derive/Cargo.toml`:

```toml
[package]
name = "mymac_derive"
version = "0.1.0"
edition = "2024"
[lib]
proc-macro = true
[dependencies]
syn = "2"
quote = "1"
```

`mymac_derive/src/lib.rs`:

```rust
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

#[proc_macro_derive(FieldNames)]
pub fn field_names(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    let name = &ast.ident;
    let (ig, tg, wc) = ast.generics.split_for_impl();
    let names: Vec<String> = match &ast.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => f.named.iter()
                .map(|x| x.ident.as_ref().unwrap().to_string()).collect(),
            _ => return syn::Error::new_spanned(&ast.ident,
                "FieldNames chi ho tro struct co ten truong")
                .to_compile_error().into(),
        },
        _ => return syn::Error::new_spanned(&ast.ident,
            "FieldNames chi ho tro struct").to_compile_error().into(),
    };
    quote! {
        impl #ig #name #tg #wc {
            pub const FIELD_NAMES: &'static [&'static str] = &[#(#names),*];
        }
    }.into()
}
```

Crate `app` (có `mymac_derive = { path = "../mymac_derive" }` trong dependencies):

```rust
use mymac_derive::FieldNames;

#[derive(FieldNames)]
struct User { id: u64, email: String }

fn main() { println!("{:?}", User::FIELD_NAMES); }
```

Chạy `cargo run` thật: in `["id", "email"]` (kèm cảnh báo `dead_code` vì hai field không được đọc — không liên quan macro).

Các bước trong hàm:

1. `parse_macro_input!` parse `TokenStream` đầu vào thành `DeriveInput`; nếu token không parse được, nó kích hoạt lỗi biên dịch[^syn-parse-input].
2. Lấy `ident` (tên kiểu) và `generics.split_for_impl()` để `impl` đúng cho kiểu generic.
3. Duyệt `Data::Struct` → `Fields::Named` lấy tên field; các trường hợp khác trả `compile_error!` gắn `Span` của tên kiểu.
4. `quote!` dựng `impl`; `#(#names),*` lặp qua `Vec<String>` rồi phân cách bằng dấu phẩy — cùng ý tưởng với `$(...),*` của `macro_rules!`, nhưng cú pháp là `#`.

<details><summary>Đi sâu: lỗi do chính macro báo ra và <code>Span</code></summary>

Cái lỗi do chính macro báo ra, khi áp dụng vào `enum E { A }` (chạy thật):

```text
error: FieldNames chi ho tro struct
 --> src/main.rs:3:6
  |
3 | enum E { A }
  |      ^
```

Dấu `^` nằm đúng tên `E` vì ta truyền `ast.ident` làm span. Syn docs nói API dựa trên token cho khả năng kiểm soát tốt nơi thông báo lỗi của compiler hiện ra trong code người dùng.[^syn-errors] Nếu quên span, lỗi sẽ trỏ vào dòng `#[derive(...)]`, khó đoán hơn (suy luận của người soạn, chưa chạy thử riêng).

</details>

**Quay lại Q5:** `syn` parse thành `DeriveInput`, `quote!` sinh `impl`; gắn `Span` đúng thì lỗi trỏ đúng chỗ.

## 6. Expand ở đâu và xem code đã expand (Q6)

> **Trả lời ngắn.** Parser đặt macro sang một bên; sau đó compiler lặp việc expand cho đến khi cây cú pháp (AST) không còn macro[^rdg-parse][^rdg-ast]. Để xem kết quả, chạy `cargo expand` (cần nightly)[^cargo-expand].

**Giải thích.**

Hình dung (nhận xét của người soạn): như template engine render HTML trước khi gửi cho trình duyệt — compiler chỉ xử lý tiếp bản đã render; `cargo expand` là nút "xem bản đã render".

*Xem code đã expand.*

`cargo expand` in kết quả macro expansion (và `#[derive]`) của crate hiện tại; nó cần nightly vì dựa vào các cờ compiler chưa ổn định.[^cargo-expand] Cài bằng `cargo install cargo-expand`. Chạy trên `app` ở trên (chạy thật, 1.0.118 với nightly):

```rust
struct User { id: u64, email: String }
impl User {
    pub const FIELD_NAMES: &'static [&'static str] = &["id", "email"];
}
fn main() {
    {
        ::std::io::_print(format_args!("{0:?}\n", User::FIELD_NAMES));
    };
}
```

Thấy cả hai: `#[derive(FieldNames)]` đã biến mất và thay bằng `impl` của ta, và `println!` đã expand thành `::std::io::_print(format_args!(...))`. README của cargo-expand cho biết lệnh này bọc một lệnh dài hơn dùng `-Zunpretty=expanded` của rustc.[^cargo-expand-wrap][^cargo-expand-unpretty] Vì `-Z` là cờ không ổn định, định dạng đầu ra có thể thay đổi theo bản nightly — đừng coi là hợp đồng (nhận xét của người soạn).

*Expand xảy ra ở giai đoạn nào.*

Theo rustc-dev-guide (*không ghim phiên bản; đây là tài liệu sống, có thể đổi*): trong giai đoạn parsing, parser thông thường của Rust đặt nội dung macro và các lời gọi sang một bên; sau đó macro được expand dựa trên các phần mã đó.[^rdg-parse] Rồi compiler lặp việc expand cho đến khi có AST hoàn chỉnh cho crate, không còn macro chưa expand (hoặc có lỗi compile).[^rdg-ast] Thuật toán cần phân giải import và tên macro.[^rdg-resolve]

<details><summary>Đi sâu: sơ đồ vị trí của expand, vì sao macro không biết kiểu, macro lồng nhau</summary>

Tóm tắt vị trí trong dòng chảy (nhận xét của người soạn dựa vào các câu trên, mức độ khái quát):

```mermaid
flowchart LR
    A[Source] --> B[Parse: đặt macro sang bên]
    B --> C[Expand lặp lại<br/>resolve tên macro + import]
    C --> D[AST không còn macro]
    D --> E[Name resolution đầy đủ,<br/>type check, ...]
```

Vì sao "macro không biết kiểu"? Vì lúc expand chưa có type check: macro chỉ thấy token. Do đó `$x:ty` chỉ cho bạn *chữ* `u32`, không cho bạn biết `u32` có implement trait nào (suy luận của người soạn từ thứ tự trên). Pipeline đầy đủ ở P10·01.

Cũng vì expand lặp, một macro có thể sinh lời gọi macro khác; Reference minh họa: macro ngoài `example` được expand trước, rồi macro trong `println` mới được expand.[^ref-nested] Thứ tự này có hệ quả khi bạn truyền `$e:expr` chứa macro vào trong macro khác.

</details>

**Quay lại Q6:** expand diễn ra sau parse và lặp đến hết macro; `cargo expand` cho thấy kết quả nhưng định dạng không phải hợp đồng.

## 7. Macro quen thuộc nhìn từ bên trong (Q7)

> **Trả lời ngắn.** Trang std của `vec!` hiển thị nó với chữ ký dạng `macro_rules!`[^std-vec-rules]; `println!` mở thành lời gọi dùng `format_args!` (kết quả `cargo expand` ở mục 6), và `format_args!` tạo giá trị kiểu `fmt::Arguments`[^std-format-args]; `#[derive(Debug)]` là derive macro sinh một `impl`[^std-debug-derive]. `#[tokio::main]` giúp dựng runtime để chạy hàm async[^tokio-main].

**Giải thích.**

Hình dung (nhận xét của người soạn): như alias rút gọn trong shell — gõ ngắn, bên dưới là chuỗi lệnh dài.

*`vec!`.*

Tài liệu std: `vec!` tạo một `Vec` chứa các đối số[^std-vec]. Trang đó hiện chữ ký `macro_rules! vec` với các luật như `() => ...` và `($elem:expr; $n:expr) => ...`[^std-vec-rules]. Trên nightly 2026-05-01 trên máy, `cargo expand` cho `vec![1, 2, 3]` ra chuỗi hàm nội bộ của `alloc` (`Box::new_uninit`, `write_box_via_move`, `box_assume_init_into_vec_unsafe`) — đó là chi tiết cài đặt của std, thay đổi theo phiên bản, đừng dựa vào.

*`println!` và `format_args!`.*

`println!` ghi ra stdout và khóa stdout mỗi lần gọi[^std-println]. `format_args!` được khai báo trong `core` với thân `/* compiler built-in */`, tức compiler tự cài đặt[^core-format-args]; docs std nói nó chuẩn bị tham số để đầu ra đọc được như chuỗi[^std-format-args-prep] và tạo giá trị kiểu `fmt::Arguments`[^std-format-args]. Kết quả expand ở mục 6 cho thấy `println!` mở thành lời gọi `_print` kèm `format_args!`.

*`#[derive(Debug)]`.*

Std mô tả đây là derive macro sinh một `impl` của trait `Debug`.[^std-debug-derive] Expand thật (nightly 2026-05-01) cho `struct P { x: i32 }`:

```rust
#[automatically_derived]
impl ::core::fmt::Debug for P {
    #[inline]
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        ::core::fmt::Formatter::debug_struct_field1_finish(f, "P", "x", &&self.x)
    }
}
```

Đây là đúng điều derive làm: thêm một `impl` sau struct, struct gốc nguyên vẹn (khớp mô tả của derive ở mục 4). Tên hàm `debug_struct_field1_finish` là chi tiết nội bộ.

<details><summary>Đi sâu: <code>#[tokio::main]</code> tương đương code nào</summary>

Docs tokio 1.52.1: macro đánh dấu một hàm async để chạy bởi runtime được chọn, và giúp dựng runtime mà người dùng không phải dùng `Runtime` hay `Builder` trực tiếp.[^tokio-main] Docs có mục "Equivalent code not using #[tokio::main]" cho thấy:

```rust
fn main() {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            println!("Hello world");
        })
}
```

[^tokio-equiv]

Tức là: `async fn main` không tồn tại ở cấp ngôn ngữ; macro bọc thân hàm vào `block_on` trên một runtime. Docs còn lưu ý: dùng macro này trên hàm không phải `main` làm hàm cư xử như hàm đồng bộ, vì mỗi lần được gọi nó khởi động một runtime mới.[^tokio-nonmain] (Chạy thử `#[tokio::main]` bằng `cargo expand` không làm ở đây — chương không khẳng định đầu ra cụ thể ngoài những gì docs nói.)

</details>

**Quay lại Q7:** các macro quen thuộc chỉ là code được sinh sẵn: `vec!` bằng `macro_rules!`, `println!` qua `format_args!`, derive thêm `impl`, `#[tokio::main]` dựng runtime.

## 8. serde, sqlx và cái giá của macro (Q8)

> **Trả lời ngắn.** serde derive sinh code lúc compile thay cho reflection lúc chạy[^serde-design]; sqlx `query!` kiểm câu SQL với schema của database ngay lúc build[^sqlx-url]. Cái giá (nhận xét của người soạn): thời gian build, lỗi khó đọc hơn, và code compiler biên dịch khác code bạn thấy.

**Giải thích.**

Hình dung (nhận xét của người soạn): như codegen từ OpenAPI/protobuf thay cho reflection runtime, và `query!` như một bước CI chạy mỗi lần build để đối chiếu SQL với schema.

**serde derive.** Docs serde: thay vì runtime reflection, serde dựa trên hệ trait của Rust; dùng derive là cách tự sinh implementation lúc compile, nên tránh được chi phí của reflection hay thông tin kiểu lúc chạy[^serde-design]. Hai derive `Serialize`, `Deserialize` chỉ có khi bật feature `derive` của serde.[^serde-feature] Nhận xét của người soạn: với một backend engineer quen Jackson/GORM tag dùng reflection, đây là khác biệt cốt lõi — cái "reflection" đó diễn ra lúc compile, nên chi phí là *thời gian build*, không phải latency request.

**sqlx `query!`.** Docs mô tả đây là truy vấn SQL được kiểm tra tĩnh, với cú pháp kiểu `println!()`.[^sqlx-static] Lúc build, biến môi trường `DATABASE_URL` phải trỏ tới một database server có schema để kiểm câu truy vấn.[^sqlx-url] Có thể thay bằng thư mục `.sqlx` ở gốc workspace ("Offline Mode").[^sqlx-offline] Bản ghim: sqlx 0.8.6. (Nhận xét của người soạn: CI build của bạn vì vậy phụ thuộc vào một DB sống hoặc file `.sqlx` được commit và giữ đồng bộ — đáng cân nhắc trong pipeline Docker/Kubernetes.)

*Chi phí và cạm bẫy.*

- **Thông báo lỗi.** Lỗi trong `macro_rules!` thường trỏ vào chỗ gọi hoặc luật không khớp, không phải ý định của bạn. Ví dụ phía trên (`early!`) cho thấy compiler vẫn có gợi ý trong trường hợp đơn giản. Với procedural macro chất lượng lỗi phụ hoàn toàn vào tác giả macro: có dùng `Span` đúng như ví dụ `FieldNames` hay không. (Nhận xét của người soạn — chỉ dựa trên hai thí nghiệm nhỏ của chương này, không phải khảo sát.)
- **Khó đọc/khó debug.** Code người đọc thấy khác code compiler biên dịch. Công cụ: `cargo expand` (mục 6). Giữ macro nhỏ; nếu hàm hoặc generic làm được thì dùng chúng (nhận xét của người soạn).
- **Hygiene không đối xứng.** `macro_rules!` hygienic cho biến cục bộ, procedural macro thì không (mục 3, 4).

<details><summary>Đi sâu: một lần đo thời gian compile của macro nhỏ</summary>

Compile time: procedural macro là code Rust phải được build và chạy trong compiler. Docs `syn` thừa nhận việc này bằng cách cho tắt bớt tính năng để macro không phải trả thời gian compile cho phần còn lại.[^syn-features] Quan sát duy nhất của tôi: build lạnh crate `mymac_derive` (syn + quote + proc-macro2, profile dev) mất khoảng 1–1,5 giây trên máy này — đó là một lần đo nhỏ của macro nhỏ, không đại diện; đo trên dự án thật của bạn bằng `cargo build --timings` (nhận xét của người soạn).

</details>

**Quay lại Q8:** serde thay reflection bằng code sinh lúc compile, sqlx kiểm SQL lúc build; cái giá là thời gian build, lỗi khó đọc và code đã expand khác code nguồn.

## Tóm tắt

- Macro tồn tại vì hàm phải khai báo cố định số tham số, không implement được trait, còn macro giúp bớt code lặp.[^book-variadic][^book-expand-before][^book-meta]
- `macro_rules!` = các luật `matcher => transcriber`, thử từ trên xuống; fragment specifier (`expr`, `ident`, `ty`, `tt`…) định loại mảnh cú pháp; lặp bằng `$(...)sep op`.
- Hygiene của `macro_rules!` áp dụng cho biến cục bộ và label: `hyg!(x * 2)` cho 102, và biến `let` trong macro không rò ra ngoài.
- `$crate` trỏ tới crate định nghĩa; `#[macro_export]` đổi visibility sang `pub`; macro textual scope nên phải định nghĩa trước khi dùng.
- Procedural macro có ba loại (derive, attribute, function-like), phải ở crate `proc-macro` riêng, làm việc trên `TokenStream`, và không hygienic.
- Derive macro nhỏ cần `syn` (parse `DeriveInput`) + `quote` (sinh token); lỗi nên gắn `Span` để trỏ đúng chỗ.
- Macro expand sau parse, lặp cho đến khi hết macro (rustc-dev-guide, không ghim); xem kết quả bằng `cargo expand` (cần nightly).
- `#[tokio::main]` chỉ dựng runtime và `block_on`; serde derive thay reflection runtime bằng code sinh lúc compile; sqlx `query!` kiểm SQL với DB hoặc `.sqlx` lúc build.
- Cái giá: thời gian compile, lỗi khó đọc, code đã expand khác code bạn thấy.

## Câu hỏi tự kiểm tra

1. Nêu ba lý do chính Rust cần macro bên cạnh hàm.
<details><summary>Đáp án</summary>

Số tham số thay đổi (<code>println!</code>), sinh item/impl trước khi compiler diễn giải nghĩa, và giảm code lặp — xem [^book-variadic][^book-expand-before][^book-meta].

</details>

2. `$x:ident` và `$x:expr` khác nhau thế nào? Cho ví dụ dùng `ident` để *tạo tên hàm*.
<details><summary>Đáp án</summary>

<code>ident</code> khớp một định danh, <code>expr</code> một biểu thức [^ref-ident][^ref-expr]. Ví dụ <code>make_fn!(answer, u32, 42)</code> ở mục 2 sinh <code>fn answer() -&gt; u32 { 42 }</code> và in 42.

</details>

3. Macro <code>hyg!(x * 2)</code> với <code>let x = 1;</code> ngoài macro in gì và vì sao?
<details><summary>Đáp án</summary>

102. Biến local trong macro được tra tại nơi định nghĩa macro nên <code>let x = 100</code> không che <code>x</code> của người gọi [^ref-hygiene].

</details>

4. Vì sao đoạn <code>fn main(){ early!(); } macro_rules! early {...}</code> lỗi?
<details><summary>Đáp án</summary>

Textual scope: macro chỉ vào scope sau chỗ định nghĩa [^ref-textual-scope]; chạy thật báo "cannot find macro 'early' in this scope".

</details>

5. Dùng <code>$crate</code> để làm gì?
<details><summary>Đáp án</summary>

Trỏ tới crate hiện tại [^ref-dollar-crate]; vì tên khác ngoài biến cục bộ và label được tra tại nơi gọi [^ref-hygiene], dùng <code>$crate</code> trong macro của thư viện để trỏ về crate định nghĩa là cách dùng điển hình (suy luận của người soạn).

</details>

6. Ba loại procedural macro là gì và loại nào nhận hai <code>TokenStream</code>?
<details><summary>Đáp án</summary>

Function-like, derive, attribute [^book-three-kinds]; attribute macro nhận hai (tham số attribute và phần còn lại của item) [^ref-attr-second].

</details>

7. Vì sao không dùng được procedural macro trong chính crate định nghĩa nó?
<details><summary>Đáp án</summary>

Reference: phải ở crate type <code>proc-macro</code> và chỉ dùng được khi import từ crate khác [^ref-proc-root]; Book cũng nói phải ở crate riêng [^book-own-crate].

</details>

8. Hai việc của <code>syn</code> và <code>quote</code> trong derive macro?
<details><summary>Đáp án</summary>

<code>syn</code> parse thành <code>DeriveInput</code> [^syn-deriveinput]; <code>quote</code> sinh token với nội suy <code>#var</code> [^quote-interp].

</details>

9. Làm sao xem code sau khi expand, và có điều kiện gì?
<details><summary>Đáp án</summary>

<code>cargo expand</code>, cần nightly [^cargo-expand]; đầu ra là chi tiết không ổn định.

</details>

10. Macro expand ở giai đoạn nào so với parse?
<details><summary>Đáp án</summary>

Sau parse (parser đặt macro sang bên), lặp đến khi AST hết macro [^rdg-parse][^rdg-ast]. Nguồn không ghim phiên bản.

</details>

11. <code>#[tokio::main]</code> tương đương code nào?
<details><summary>Đáp án</summary>

Dựng <code>Builder::new_multi_thread().enable_all().build().unwrap().block_on(async {...})</code> [^tokio-equiv].

</details>

12. sqlx <code>query!</code> kiểm SQL bằng gì lúc build?
<details><summary>Đáp án</summary>

Database trỏ bởi <code>DATABASE_URL</code> hoặc thư mục <code>.sqlx</code> [^sqlx-url][^sqlx-offline].

</details>

## Đọc thêm

- [Rust Book 1.95.0, ch20.5 Macros](https://doc.rust-lang.org/1.95.0/book/ch20-05-macros.html) — ví dụ `hello_macro` và `vec!` từng bước.
- [Reference 1.95.0, Macros By Example](https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html) — định nghĩa chính xác về fragment, repetition, hygiene, scope.
- [Reference 1.95.0, Procedural Macros](https://doc.rust-lang.org/1.95.0/reference/procedural-macros.html) — chữ ký chính xác của ba loại, helper attribute.
- [syn 2.0.119](https://docs.rs/syn/2.0.119/syn/) — ví dụ derive đầy đủ kèm báo lỗi có `Span`.
- [quote 1.0.47](https://docs.rs/quote/1.0.47/quote/) — cú pháp `#var`, `#(...)*`, `quote_spanned!`.
- [rustc-dev-guide, Macro expansion](https://rustc-dev-guide.rust-lang.org/macro-expansion.html) — thuật toán expand (không ghim); chi tiết ở P10·01.
- [The Little Book of Rust Macros](https://veykril.github.io/tlborm/) — kỹ thuật `macro_rules!` nâng cao (không ghim, bổ trợ).

[^book-variadic]: Rust Book 1.95.0, ch20.5 — <https://doc.rust-lang.org/1.95.0/book/ch20-05-macros.html> — "Macros, on the other hand, can take a variable number of parameters"
[^book-expand-before]: Rust Book 1.95.0, ch20.5 — <https://doc.rust-lang.org/1.95.0/book/ch20-05-macros.html> — "macros are expanded before the compiler interprets the meaning of the code, so a macro can, for example, implement a trait on a given type."
[^book-meta]: Rust Book 1.95.0, ch20.5 — <https://doc.rust-lang.org/1.95.0/book/ch20-05-macros.html> — "Fundamentally, macros are a way of writing code that writes other code, which is known as metaprogramming."
[^book-three-kinds]: Rust Book 1.95.0, ch20.5 — <https://doc.rust-lang.org/1.95.0/book/ch20-05-macros.html> — "The three kinds of procedural macros are custom derive, attribute-like, and function-like, and all work in a similar fashion."
[^book-own-crate]: Rust Book 1.95.0, ch20.5 — <https://doc.rust-lang.org/1.95.0/book/ch20-05-macros.html> — "the definitions must reside in their own crate with a special crate type."
[^ref-metavar]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "In the matcher, $ name : fragment-specifier matches a Rust syntax fragment of the kind specified and binds it to the metavariable $name."
[^ref-fragspec-list]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "block: a BlockExpression expr: an Expression"
[^ref-expr]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "expr: an Expression expr_2021: an Expression except UnderscoreExpression and ConstBlockExpression"
[^ref-ident]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "ident: an IDENTIFIER_OR_KEYWORD except _, RAW_IDENTIFIER, or $crate"
[^ref-ty]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "tt: a TokenTree (a single token or tokens in matching delimiters (), [], or {}) ty: a Type"
[^ref-tt]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "tt: a TokenTree (a single token or tokens in matching delimiters (), [], or {})"
[^ref-expr2024]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "Before the 2024 edition, expr fragment specifiers do not match UnderscoreExpression or ConstBlockExpression at the top level."
[^ref-repetition-sep]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "For instance, $( $i:ident ),* represents any number of identifiers separated by commas."
[^ref-plus]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "+ — indicates any number but at least one."
[^ref-opt]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "? — indicates an optional fragment with zero or one occurrence."
[^ref-rep-rules]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "A metavariable must appear in exactly the same number, kind, and nesting order of repetitions in the transcriber as it did in the matcher."
[^ref-hygiene]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "Macros by example have mixed-site hygiene. This means that loop labels, block labels, and local variables are looked up at the macro definition site while other symbols are looked up at the macro invocation site."
[^ref-dollar-crate]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "The keyword metavariable $crate can be used to refer to the current crate."
[^ref-macro-export]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "Macros have an implicit visibility of pub(crate). #[macro_export] changes the implicit visibility to pub."
[^ref-scope-two]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "Macros have two forms of scope: textual scope, and path-based scope."
[^ref-textual-scope]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "the macro enters the scope after the definition (note that it can still be used recursively, since names are looked up from the invocation site)"
[^ref-lookup-order]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "it is first looked up in textual scoping. If this does not yield any results, then it is looked up in path-based scoping."
[^ref-macro-use]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "it may be used on modules to extend the scope of macros defined within them, and it may be used on extern crate to import macros from another crate into the macro_use prelude."
[^ref-reexport]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "Macros can be re-exported to give them path-based scope from a module other than the crate root."
[^ref-proc-root]: Rust Reference 1.95.0, Procedural Macros — <https://doc.rust-lang.org/1.95.0/reference/procedural-macros.html> — "Procedural macros must be defined in the root of a crate with the crate type of proc-macro. The macros may not be used from the crate where they are defined, and can only be used when imported in another crate."
[^ref-proc-cargo]: Rust Reference 1.95.0, Procedural Macros — <https://doc.rust-lang.org/1.95.0/reference/procedural-macros.html> — "When using Cargo, Procedural macro crates are defined with the proc-macro key in your manifest:"
[^ref-fnlike]: Rust Reference 1.95.0, Procedural Macros — <https://doc.rust-lang.org/1.95.0/reference/procedural-macros.html> — "will pass what is inside the delimiters of the macro invocation as the input TokenStream argument and replace the entire macro invocation with the output TokenStream of the function."
[^ref-derive]: Rust Reference 1.95.0, Procedural Macros — <https://doc.rust-lang.org/1.95.0/reference/procedural-macros.html> — "These macros are given the token stream of a struct, enum, or union definition and can emit new items after it."
[^ref-attr-second]: Rust Reference 1.95.0, Procedural Macros — <https://doc.rust-lang.org/1.95.0/reference/procedural-macros.html> — "The second TokenStream is the rest of the item, including other attributes on the item."
[^ref-tokenstream]: Rust Reference 1.95.0, Procedural Macros — <https://doc.rust-lang.org/1.95.0/reference/procedural-macros.html> — "Procedural macros operate over token streams instead of AST nodes, which is a far more stable interface over time for both the compiler and for procedural macros to target."
[^ref-span]: Rust Reference 1.95.0, Procedural Macros — <https://doc.rust-lang.org/1.95.0/reference/procedural-macros.html> — "Spans represent an extent of source code within a program and are primarily used for error reporting."
[^ref-proc-hygiene]: Rust Reference 1.95.0, Procedural Macros — <https://doc.rust-lang.org/1.95.0/reference/procedural-macros.html> — "Procedural macros are unhygienic. This means they behave as if the output token stream was simply written inline"
[^ref-nested]: Rust Reference 1.95.0, Macros — <https://doc.rust-lang.org/1.95.0/reference/macros.html> — "Outer macro `example` is expanded, then inner macro `println` is expanded."
[^syn-tree]: syn 2.0.119 docs — <https://docs.rs/syn/2.0.119/syn/> — "Syn provides a complete syntax tree that can represent any valid Rust source code."
[^syn-deriveinput]: syn 2.0.119 docs — <https://docs.rs/syn/2.0.119/syn/> — "Of particular interest to derive macros is syn::DeriveInput which is any of the three legal input items to a derive macro."
[^syn-features]: syn 2.0.119 docs — <https://docs.rs/syn/2.0.119/syn/> — "Functionality is aggressively feature gated so your procedural macros enable only what they need, and do not pay in compile time for all the rest."
[^syn-errors]: syn 2.0.119 docs — <https://docs.rs/syn/2.0.119/syn/> — "The token-based procedural macro API provides great control over where the compiler’s error messages are displayed in user code."
[^syn-parse-input]: syn 2.0.119 docs — <https://docs.rs/syn/2.0.119/syn/> — "Parse the input TokenStream of a macro, triggering a compile error if the tokens fail to parse."
[^quote-idea]: quote 1.0.47 docs — <https://docs.rs/quote/1.0.47/quote/> — "The idea of quasi-quoting is that we write code that we treat as data."
[^quote-interp]: quote 1.0.47 docs — <https://docs.rs/quote/1.0.47/quote/> — "The #var syntax performs interpolation of runtime variables into the quoted tokens."
[^pm2]: proc-macro2 1.0.107 docs — <https://docs.rs/proc-macro2/1.0.107/proc_macro2/> — "Types from proc_macro are entirely specific to procedural macros and cannot ever exist in code outside of a procedural macro. Meanwhile proc_macro2 types may exist anywhere including non-macro code."
[^cargo-expand]: cargo-expand 1.0.118 README — <https://raw.githubusercontent.com/dtolnay/cargo-expand/1.0.118/README.md> — "Cargo expand relies on unstable compiler flags so it requires a nightly toolchain to be installed"
[^cargo-expand-wrap]: cargo-expand 1.0.118 README — <https://raw.githubusercontent.com/dtolnay/cargo-expand/1.0.118/README.md> — "This is a wrapper around the more verbose compiler command:"
[^rdg-parse]: rustc-dev-guide, Macro expansion (không ghim phiên bản) — <https://rustc-dev-guide.rust-lang.org/macro-expansion.html> — "During the parsing phase, the normal Rust parser will set aside the contents of macros and their invocations."
[^rdg-ast]: rustc-dev-guide, Macro expansion (không ghim phiên bản) — <https://rustc-dev-guide.rust-lang.org/macro-expansion.html> — "we have a complete Abstract Syntax Tree (AST) for our crate with no unexpanded macros (or a compile error)."
[^rdg-resolve]: rustc-dev-guide, Macro expansion (không ghim phiên bản) — <https://rustc-dev-guide.rust-lang.org/macro-expansion.html> — "we need to resolve imports and macro names in the above algorithm."
[^std-vec]: Rust std 1.95.0, macro vec — <https://doc.rust-lang.org/1.95.0/std/macro.vec.html> — "Creates a Vec containing the arguments."
[^std-vec-rules]: Rust std 1.95.0, macro vec — <https://doc.rust-lang.org/1.95.0/std/macro.vec.html> — "($elem:expr; $n:expr) => { ... }; ($($x:expr),+ $(,)?) => { ... };"
[^std-println]: Rust std 1.95.0, macro println — <https://doc.rust-lang.org/1.95.0/std/macro.println.html> — "The println! macro will lock the standard output on each call."
[^std-format-args]: Rust std 1.95.0, macro format_args — <https://doc.rust-lang.org/1.95.0/std/macro.format_args.html> — "This macro produces a value of type fmt::Arguments."
[^std-debug-derive]: Rust std 1.95.0, derive Debug — <https://doc.rust-lang.org/1.95.0/std/fmt/derive.Debug.html> — "Derive macro generating an impl of the trait Debug."
[^tokio-main]: tokio 1.52.1 docs, attr main — <https://docs.rs/tokio/1.52.1/tokio/attr.main.html> — "Marks async function to be executed by the selected runtime. This macro helps set up a Runtime without requiring the user to use Runtime or Builder directly."
[^tokio-equiv]: tokio 1.52.1 docs, attr main — <https://docs.rs/tokio/1.52.1/tokio/attr.main.html> — "Equivalent code not using #[tokio::main]"
[^tokio-nonmain]: tokio 1.52.1 docs, attr main — <https://docs.rs/tokio/1.52.1/tokio/attr.main.html> — "Using it on a non-main function makes the function behave as if it was synchronous by starting a new runtime each time it is called."
[^serde-design]: serde 1.0.228 docs — <https://docs.rs/serde/1.0.228/serde/> — "This avoids any overhead of reflection or runtime type information."
[^serde-feature]: serde 1.0.228 docs — <https://docs.rs/serde/1.0.228/serde/> — "Derive macro available if serde is built with features"
[^sqlx-static]: sqlx 0.8.6 docs, macro query — <https://docs.rs/sqlx/0.8.6/sqlx/macro.query.html> — "Statically checked SQL query with println!() style syntax."
[^sqlx-url]: sqlx 0.8.6 docs, macro query — <https://docs.rs/sqlx/0.8.6/sqlx/macro.query.html> — "The DATABASE_URL environment variable must be set at build-time to point to a database server with the schema that the query string will be checked against."
[^sqlx-offline]: sqlx 0.8.6 docs, macro query — <https://docs.rs/sqlx/0.8.6/sqlx/macro.query.html> — "Or, .sqlx must exist at the workspace root."
[^book-fn-sig]: Rust Book 1.95.0, ch20.5 — <https://doc.rust-lang.org/1.95.0/book/ch20-05-macros.html> — "A function signature must declare the number and type of parameters the function has."
[^book-fn-runtime]: Rust Book 1.95.0, ch20.5 — <https://doc.rust-lang.org/1.95.0/book/ch20-05-macros.html> — "A function can’t, because it gets called at runtime and a trait needs to be implemented at compile time."
[^ref-transcribe]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "It transcribes the first successful match; if this results in an error, then future matches are not tried."
[^ref-macro-export-root]: Rust Reference 1.95.0, Macros By Example — <https://doc.rust-lang.org/1.95.0/reference/macros-by-example.html> — "The macro_export attribute exports the macro from the crate and makes it available in the root of the crate for path-based resolution."
[^ref-proc-run]: Rust Reference 1.95.0, Procedural Macros — <https://doc.rust-lang.org/1.95.0/reference/procedural-macros.html> — "Procedural macros run during compilation, and thus have the same resources that the compiler has."
[^ref-proc-result]: Rust Reference 1.95.0, Procedural Macros — <https://doc.rust-lang.org/1.95.0/reference/procedural-macros.html> — "Returned syntax either replaces or adds the syntax depending on the kind of procedural macro."
[^quote-spanned]: quote 1.0.47 docs — <https://docs.rs/quote/1.0.47/quote/> — "See also the quote_spanned! macro which is important for implementing hygienic procedural macros."
[^pm2-test]: proc-macro2 1.0.107 docs — <https://docs.rs/proc-macro2/1.0.107/proc_macro2/> — "In order for helper libraries or components of a macro to be testable in isolation, they must be implemented using proc_macro2."
[^cargo-expand-unpretty]: cargo-expand 1.0.118 README — <https://raw.githubusercontent.com/dtolnay/cargo-expand/1.0.118/README.md> — "cargo rustc --profile=check -- -Zunpretty=expanded"
[^core-format-args]: rust-lang/rust tag 1.95.0, library/core/src/macros/mod.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/core/src/macros/mod.rs> — "($fmt:expr, $($args:tt)*) => {{ /* compiler built-in */ }};"
[^std-format-args-prep]: Rust std 1.95.0, macro format_args — <https://doc.rust-lang.org/1.95.0/std/macro.format_args.html> — "format_args! prepares the additional parameters to ensure the output can be interpreted as a string and canonicalizes the arguments into a single type."
