# P9 · Chương 04 — Smart pointer

Chương này đi qua họ smart pointer của thư viện chuẩn Rust: `Box`, `Rc`, `Weak`, `Arc`, `Cell`, `RefCell`, `Cow`, `OnceCell`/`LazyCell`, và cách tự viết một smart pointer bằng `Deref` + `Drop`. Mọi ví dụ đã chạy bằng `rustc 1.95.0` (edition 2024), macOS aarch64; kích thước là **quan sát**, không phải hợp đồng ngôn ngữ.

## Bạn sẽ hiểu được gì

1. **Q1.** Smart pointer khác reference ở đâu, hai trait nào làm nên "tính smart", và `Deref` làm cho `*` và `.` hoạt động ra sao (deref coercion)? → mục 1
2. **Q2.** `Drop` chạy lúc nào và theo thứ tự nào; `mem::drop` và `let _` khác drop tự động thế nào? → mục 2
3. **Q3.** `Box<T>` giải quyết bài toán nào (kiểu đệ quy, trait object, giá trị lớn), và `Box<dyn Error>`, `Pin<Box<T>>` nằm ở đâu? → mục 3
4. **Q4.** `Rc<T>` + `Weak<T>` dựng cây cha–con thế nào mà không rò bộ nhớ, và khi nào dùng `Arc<T>` thay `Rc<T>`? → mục 4
5. **Q5.** Interior mutability là gì; `Cell`, `RefCell` khác nhau thế nào; `BorrowMutError` xảy ra khi nào; vì sao `Rc<RefCell<T>>` hay gặp và trả giá gì; `OnceCell`/`LazyCell` dùng để làm gì? → mục 5
6. **Q6.** `Cow<'a, B>` giúp API trả về "có thể mượn, có thể sở hữu" ra sao? → mục 6
7. **Q7.** Tự viết một guard bằng `Deref`/`DerefMut`/`Drop` như thế nào? → mục 7

## Kiến thức cần trước

- [P3 · Chương 08 — Rust và bộ nhớ](../03-memory/08-rust-va-bo-nho.md): ownership, move, `Drop`, thứ tự drop, bố cục `Box`/`Rc`, leak là safe. Chương này **không lặp** các phần đó; chỗ nào cần sẽ nhắc ngắn và link.
- [P4 · Chương 08 — Concurrency trong Rust](../04-concurrency/08-rust-concurrency.md): `Send`/`Sync`, `Rc` vs `Arc`, `Mutex`. Ở đây chỉ nhắc lý do chọn `Arc`.
- [P7 · Chương 06 — async/await và Pin](../07-io-async/06-async-await-pin.md): `Pin` được giải thích đầy đủ ở đó.
- Đã đọc Rust Book tới generics/traits/lifetimes (ch15 sẽ được dùng làm nguồn chính).

## 1. Smart pointer là gì, và `Deref` (Q1)

> **Trả lời ngắn.** Reference chỉ trỏ tới dữ liệu và không có khả năng đặc biệt nào khác[^book-ref-plain]; smart pointer là cấu trúc dữ liệu hoạt động như con trỏ nhưng có thêm metadata và khả năng[^book-sp-def]. Hai trait làm nên "tính smart" là `Deref` (dùng như reference) và `Drop` (code chạy khi ra khỏi scope)[^book-sp-traits]. `Deref` cho phép `*p` và deref coercion: compiler tự đổi reference tới smart pointer thành reference tới kiểu bên trong, ví dụ `&String` thành `&str`[^book-deref-coerce]. Nhờ vậy gọi được method của `T` trực tiếp trên smart pointer.

**Giải thích.**

Hình dung (nhận xét của người soạn): reference là tấm biển chỉ đường tới một căn nhà; smart pointer là chiếc chìa khóa kèm sổ quản lý — biết mình đang giữ nhà nào và tự trả nhà khi hết người dùng.

### 1.1 Reference và smart pointer

**Vấn đề.** Reference `&T` chỉ trỏ vào dữ liệu: nó không sở hữu, không quyết định dữ liệu sống bao lâu, không đếm được ai đang dùng. Rust Book mô tả: reference "không có khả năng đặc biệt nào ngoài việc trỏ tới dữ liệu và không có overhead"[^book-ref-plain], còn smart pointer là cấu trúc dữ liệu hoạt động như con trỏ nhưng có thêm metadata và khả năng.[^book-sp-def]

**Cơ chế.** Một smart pointer, theo Book, thường được cài bằng `struct` và nhiều khi sở hữu dữ liệu nó trỏ tới;[^book-sp-own] khác struct thường ở chỗ cài đặt hai trait `Deref` và `Drop`.[^book-sp-traits] Hai trait đó chia việc rõ ràng:

| Trait | Việc | Hệ quả quan sát được |
|---|---|---|
| `Deref` / `DerefMut` | cho phép `*p` và deref coercion, để smart pointer dùng như reference | gọi method của `T` trực tiếp trên `p` |
| `Drop` | code chạy khi giá trị ra khỏi scope | giải phóng heap, giảm bộ đếm, nhả lock… |

Bảng tổng quan để định hướng cả chương (phần "chi phí" là **nhận xét của người soạn** dựa trên các ví dụ chạy bên dưới):

| Kiểu | Sở hữu | Thread | Mutation | Chi phí chính |
|---|---|---|---|---|
| `Box<T>` | duy nhất | theo `T` | qua `&mut` bình thường | 1 allocation |
| `Rc<T>` | chung | một thread (`!Send`, `!Sync`)[^std-rc-notsend] | không (chỉ `&T`) | bộ đếm không-atomic |
| `Arc<T>` | chung | nhiều thread | không (chỉ `&T`) | bộ đếm atomic |
| `Cell<T>` | duy nhất | chuyển được nếu `T: Send`[^std-cell-send], nhưng `!Sync` | thay cả giá trị | gần như không |
| `RefCell<T>` | duy nhất | chuyển được nếu `T: Send`[^std-refcell-send], nhưng `!Sync` | `borrow_mut()` kiểm tra lúc chạy | cờ đếm + nguy cơ panic |
| `Cow<'a, B>` | mượn *hoặc* sở hữu | theo `B` | `to_mut()` clone khi cần | enum tag |
| `OnceCell<T>` / `LazyCell<T>` | duy nhất | chuyển được nếu `T` (và `F`) là `Send`[^std-oncecell-send][^std-lazycell-send], nhưng `!Sync` | ghi một lần | enum Option |

<details><summary>Đi sâu: smart pointer trong tài liệu std của <code>Deref</code></summary>

Tài liệu std của `Deref` nói thêm: các kiểu cài đặt `Deref`/`DerefMut` thường được gọi là smart pointer, và cơ chế deref coercion được thiết kế riêng để hỗ trợ hành vi giống con trỏ ấy.[^std-deref-sp] Mục đích của smart pointer thường là đổi *ownership semantics* (như `Rc`, `Cow`) hoặc *storage semantics* (như `Box`) của giá trị bên trong (cũng theo std docs).[^std-deref-purpose]

</details>

### 1.2 `Deref`: `*` và `.`

Hình dung (nhận xét của người soạn): deref coercion là nhân viên lễ tân tự mở từng lớp cửa giúp bạn cho tới khi tới đúng phòng cần vào.

Ví dụ tối thiểu — một `MyBox<T>` chỉ cài `Deref`:

```rust
use std::ops::Deref;
struct MyBox<T>(T);
impl<T> Deref for MyBox<T> { type Target = T; fn deref(&self) -> &T { &self.0 } }
fn hello(s: &str) -> usize { s.len() }
fn main() {
    let m = MyBox(String::from("rust"));
    println!("deref coercion: {}", hello(&m));   // &MyBox<String> -> &String -> &str
}
```

Kết quả chạy thật: `deref coercion: 4`. Chuỗi biến đổi `&MyBox<String>` → `&String` → `&str` do compiler tự chèn. Book định nghĩa: deref coercion chuyển một reference tới kiểu cài `Deref` thành reference tới kiểu khác, ví dụ `&String` thành `&str`.[^book-deref-coerce] Nó áp dụng cho ba trường hợp `&T→&U`, `&mut T→&mut U`, `&mut T→&U`; chiều ngược lại (`&T→&mut U`) bị cấm, vì Book nói Rust có thể ép tham chiếu mutable thành immutable nhưng *ngược lại thì không*.[^book-deref-mut]

<details><summary>Đi sâu: hai điểm hay bị hiểu nhầm (<code>Rc</code> không có <code>DerefMut</code>, không move ra bằng <code>*</code>)</summary>

Hai điểm hay bị hiểu nhầm:

- `Rc<T>` **không** có `DerefMut`. Thử `Rc::new(vec![1]).push(2)` cho lỗi thật:

  ```
  error[E0596]: cannot borrow data in an `Rc` as mutable
     = help: trait `DerefMut` is required to modify through a dereference, but it is not implemented for `Rc<Vec<i32>>`
  ```

- Không thể move giá trị ra khỏi smart pointer chỉ bằng `*`: `let s: String = *r;` với `r: Rc<String>` cho `error[E0507]: cannot move out of an `Rc``. (`Box` cho phép move ra bằng `*`: `let val: u8 = *boxed;` nằm trong ví dụ của std docs.[^std-box-intro])

</details>

**Quay lại Q1:** smart pointer là struct sở hữu dữ liệu và cài `Deref` + `Drop`, nên dùng như reference mà vẫn tự dọn dẹp; `Deref` + deref coercion cho smart pointer dùng như reference; chiều `&T → &mut U` bị cấm.

## 2. `Drop` và thứ tự: tự động vs `mem::drop` (Q2)

> **Trả lời ngắn.** `Drop` là code chạy khi giá trị ra khỏi scope, dùng để giải phóng tài nguyên như file hoặc kết nối mạng[^book-drop]. Biến cục bộ bị drop theo thứ tự ngược lúc tạo[^book-drop-rev], field của struct theo thứ tự khai báo[^ref-field-order]. Không gọi `Drop::drop` bằng tay được; muốn drop sớm thì dùng `std::mem::drop`[^book-drop-explicit].

**Giải thích.**

Hình dung (nhận xét của người soạn): như dọn bàn ăn theo chồng đĩa — đĩa xếp sau cùng được dỡ ra trước; bạn không tự tay giật đĩa khỏi chồng mà nhờ người phục vụ (`mem::drop`) bê đi sớm.

`Drop` là phần còn lại của "smart". Book: có thể cài `Drop` cho bất kỳ kiểu nào, và code đó dùng để giải phóng tài nguyên như file hoặc kết nối mạng.[^book-drop] Các quy tắc cốt lõi (move xem [P3·08 mục 2](../03-memory/08-rust-va-bo-nho.md), thứ tự drop xem [P3·08 mục 3](../03-memory/08-rust-va-bo-nho.md)):

- Biến cục bộ bị drop theo thứ tự ngược lại với lúc tạo.[^book-drop-rev] Reference cũng viết: khi ra khỏi scope, mọi biến bị drop theo thứ tự ngược khai báo (biến) hoặc ngược tạo (temporary).[^ref-scope-order]
- Field của struct bị drop theo thứ tự khai báo.[^ref-field-order]
- Không thể gọi `Drop::drop` bằng tay; muốn drop sớm dùng `std::mem::drop`.[^book-drop-explicit] Hàm này "không có phép màu": nó được định nghĩa đúng là `pub fn drop<T>(_x: T) {}` — `_x` bị move vào hàm nên tự bị drop khi hàm return.[^std-mem-drop]
- Không thể vừa `Copy` vừa `Drop`.[^std-copy-drop]

<details><summary>Đi sâu: thí nghiệm thứ tự drop, <code>let _</code> và temporary</summary>

Chạy thật một chương trình quan sát thứ tự (code đầy đủ bên dưới):

```rust
struct D(&'static str);
impl Drop for D { fn drop(&mut self) { println!("drop {}", self.0); } }
struct Pair { a: D, b: D }
fn main() {
    let _x = D("x"); let _y = D("y");
    let p = Pair { a: D("a"), b: D("b") };
    let e = D("early"); drop(e); println!("after early");
    let _ = D("underscore"); println!("after underscore");
    let _p = p;
    println!("end main");
}
```

Đầu ra thật:

```
drop early
after early
drop underscore
after underscore
end main
drop a
drop b
drop y
drop x
```

Đọc kết quả: (1) `drop(e)` chạy ngay. (2) `let _ = D(...)` **không** gắn giá trị vào biến. Pattern `_` "không copy, move hay borrow" giá trị nó khớp,[^ref-wildcard] còn vế phải `D("underscore")` là một temporary mà phạm vi (temporary scope) là câu lệnh chứa nó,[^ref-temp-scope] nên bị drop ngay cuối câu lệnh — khác `let _p = ...` giữ đến hết scope. Hệ quả cần nhớ: `let _ = x;` với `x` là **biến có sẵn** (place expression) không move và không drop `x` — `x` vẫn sống đến hết scope (chạy thật: `let z = D("z"); let _ = z;` không in `drop z` ngay; nó chỉ in ở cuối `main`, sau `_p`, trước `_y`). Vậy "`let _` drop ngay" chỉ đúng khi vế phải là temporary (kết quả hàm, constructor), không đúng với biến. (3) Cuối `main`, `_p` (khai báo sau cùng) bị drop trước: field `a` rồi `b` (đúng thứ tự khai báo field), rồi `_y`, rồi `_x`. Lưu ý `p` đã bị move vào `_p`, nên nó không bị drop lần thứ hai.

</details>

**Quay lại Q2:** biến drop ngược thứ tự khai báo, field theo thứ tự khai báo, `mem::drop` chỉ là hàm nhận ownership; `let _ = <temporary>` drop ngay.

## 3. `Box<T>`: sở hữu duy nhất (kèm `Box<dyn Error>` và `Pin<Box<T>>`) (Q3)

> **Trả lời ngắn.** `Box<T>` là con trỏ sở hữu duy nhất một heap allocation kiểu `T`[^std-box-def]. Nó giải ba bài toán: kiểu đệ quy (box có kích thước biết trước nên làm kiểu hữu hạn)[^book-box-recursive], trait object (con trỏ tới instance và bảng tra method)[^book-trait-obj], và chuyển ownership dữ liệu lớn mà không sao chép[^book-box-large]. `Box<dyn Error>` là một trait object dùng làm kiểu lỗi chung, để toán tử `?` hội tụ nhiều kiểu lỗi khác nhau trong cùng một `Result`[^book-box-dyn-error]. `Pin` bọc một con trỏ để "ghim" pointee tại chỗ, ngăn nó bị move hoặc vô hiệu trừ khi nó là `Unpin`[^std-pin-def]. `Pin<Box<T>>` là con trỏ ghim sở hữu tới `T` được ghim[^std-pin-box].

**Giải thích.**

Hình dung (nhận xét của người soạn): `Box` là tờ giấy nhận hàng nhỏ gọn cầm trong tay, còn món hàng lớn hoặc chưa biết kích cỡ nằm trong kho (heap).

std docs định nghĩa `Box<T>` là kiểu con trỏ sở hữu duy nhất một heap allocation kiểu `T`.[^std-box-def] Bố cục `Box` trong bộ nhớ xem [P3·08 mục 4](../03-memory/08-rust-va-bo-nho.md), allocator xem [P3·08 mục 5](../03-memory/08-rust-va-bo-nho.md); ở đây ta tập trung vào ba cách dùng mà Book liệt kê.

### 3.1 Kiểu đệ quy

Một enum chứa chính nó thì kích thước vô hạn. Thử:

```rust
enum List { Cons(i32, List), Nil }
```

Lỗi thật:

```
error[E0072]: recursive type `List` has infinite size
 --> i.rs:1:1
1 | enum List { Cons(i32, List), Nil }
  | ^^^^^^^^^             ---- recursive without indirection
help: insert some indirection (e.g., a `Box`, `Rc`, or `&`) to break the cycle
```

Book (cùng lỗi E0072) giải thích: vì box có kích thước biết trước, ta dùng nó chen vào định nghĩa đệ quy để kiểu hữu hạn.[^book-box-recursive] Sửa thành `Cons(i32, Box<List>)` rồi chạy:

```rust
enum List { Cons(i32, Box<List>), Nil }
use List::{Cons, Nil};
fn sum(l: &List) -> i32 { match l { Cons(v, r) => v + sum(r), Nil => 0 } }
// Cons(1, Box::new(Cons(2, Box::new(Cons(3, Box::new(Nil)))))) → sum = 6
```

Đầu ra thật: `sum=6`. Kích thước đo được: `size_of::<List>() = 16`, `size_of::<Box<List>>() = 8`. Giải thích (suy luận của người soạn từ số đo): `i32` (4 byte) cộng con trỏ 8 byte cần căn lề 8 nên thành 16; việc phân biệt `Nil` có thể dùng chỗ trống (niche) của con trỏ hoặc một tag riêng — chưa đo riêng nên không khẳng định.


### 3.2 Trait object

Trait object là một con trỏ "béo": Book nói nó trỏ tới cả instance của kiểu cài trait và một bảng để tra method lúc chạy; tạo trait object bằng cách đặt sau một loại con trỏ như reference hoặc `Box<T>` rồi `dyn Trait`.[^book-trait-obj]

```rust
trait Shape { fn area(&self) -> f64; }
struct Sq(f64); struct Ci(f64);
impl Shape for Sq { fn area(&self) -> f64 { self.0*self.0 } }
impl Shape for Ci { fn area(&self) -> f64 { 3.0*self.0*self.0 } }
let v: Vec<Box<dyn Shape>> = vec![Box::new(Sq(2.0)), Box::new(Ci(1.0))];
// areas: [4.0, 3.0]
```

Số đo thật trên aarch64: `Box<i32> = 8`, `Box<dyn Shape> = 16`, `&dyn Shape = 16`, `Option<Box<i32>> = 8`. Nghĩa là `Box<dyn Trait>` gồm hai từ — con trỏ dữ liệu và con trỏ vtable (đây là số đo hiện tại, không phải bảo đảm ngôn ngữ: Reference chỉ hứa con trỏ tới kiểu unsized "ít nhất bằng" con trỏ thường; vtable và fat pointer xem [P10·03 mục 2](../10-rust-internals/03-dst-fat-pointer-vtable.md) và [mục 4](../10-rust-internals/03-dst-fat-pointer-vtable.md), hợp đồng `dyn` xem [02-trait.md mục 6](02-trait.md)). Riêng `Option<Box<T>>` cùng size với `Box<T>` thì khác: std *bảo đảm* điều đó cho `Box<U, Global>`, không chỉ là quan sát (xem [P10·02 mục 6](../10-rust-internals/02-type-layout.md)).

### 3.3 Giá trị lớn

Book nêu tình huống: có nhiều dữ liệu và muốn chuyển ownership mà không muốn bị sao chép — lúc đó đặt dữ liệu lên heap trong box.[^book-box-large] Ví dụ chạy thật: `let big = [0u8; 100_000]; let b = Box::new(big);` in `box big len 100000`. (Nhận xét của người soạn: hãy phân biệt hai chuyện — *move* một `Box` chỉ copy 8 byte con trỏ; còn `Box::new(big)` vẫn có thể copy 100 KB vào heap một lần. Không đo thì đừng khẳng định cách tối ưu.)

<details><summary>Đi sâu: allocator của <code>Box</code></summary>

Với giá trị kích thước khác 0, `Box` dùng `Global` allocator để cấp phát.[^std-box-layout]

</details>

### 3.4 `Box<dyn Error>`

Hình dung (nhận xét của người soạn): thùng đựng lỗi chung — mọi loại lỗi bỏ vào được, nhưng lấy ra thì chỉ còn biết nó là "một lỗi".

Book dùng `Box<dyn Error>` làm kiểu lỗi chung: `main() -> Result<(), Box<dyn Error>>` cho phép `?` trên nhiều kiểu lỗi; và đây là một trait object.[^book-box-dyn-error] Chạy thật:

```rust
fn parse(s: &str) -> Result<i32, Box<dyn Error>> {
    let n: i32 = s.parse()?;                // ParseIntError -> Box<dyn Error>
    if n < 0 { return Err("negative".into()); }   // &str -> Box<dyn Error>
    Ok(n)
}
```

<details><summary>Đi sâu: kết quả chạy thật và giá phải trả</summary>

`parse("12") = Ok(12)`, `parse("x")` in lỗi `invalid digit found in string`, `parse("-1")` in `negative`. Giá phải trả (nhận xét của người soạn): kiểu lỗi bị xoá, người gọi chỉ còn `downcast` để biết loại; thiết kế lỗi đầy đủ (`thiserror`, `anyhow`) và vì sao cần `+ Send + Sync` ở [05-error-handling.md, mục 4–5](05-error-handling.md).

</details>

### 3.5 `Pin<Box<T>>`: nhắc

Hình dung (nhận xét của người soạn): `Pin` như đóng đinh một món đồ vào bàn — nó không được dời chỗ nữa, trừ khi món đồ vốn không bận tâm bị dời (`Unpin`).

`Pin` bọc một con trỏ để "ghim" pointee tại chỗ, ngăn nó bị move hoặc vô hiệu trừ khi nó là `Unpin`.[^std-pin-def] `Pin<Box<T>>` là con trỏ ghim *sở hữu* tới `T` được ghim;[^std-pin-box] và với `T: Unpin` nó hoạt động y hệt `Box<T>` thường.[^std-pin-unpin]

<details><summary>Đi sâu: quan sát và nơi giải thích đầy đủ</summary>

Chạy thật: `Box::pin(7)` cho `Pin<Box<i32>>` deref ra `7`; `size_of::<Pin<Box<i32>>>() = 8` (wrapper không tốn thêm byte trong đo này). Vì sao cần ghim future và `Box::pin` để poll: [P7·06 §5](../07-io-async/06-async-await-pin.md).

</details>

**Quay lại Q3:** `Box` là con trỏ sở hữu duy nhất tới heap, dùng cho kiểu đệ quy, trait object và giá trị lớn. `Box<dyn Error>` gom mọi lỗi cho `?`, đổi lại kiểu lỗi bị xóa. `Pin<Box<T>>` là `Box` ghim; với `Unpin` nó như `Box` thường.

## 4. `Rc<T>`, `Weak<T>` và `Arc<T>`: chia sẻ ownership (Q4)

> **Trả lời ngắn.** `Rc<T>` là con trỏ đếm tham chiếu đơn thread: theo dõi số tham chiếu để biết giá trị còn được dùng hay không[^std-rc-def][^book-rc-count]. Trong cây cha–con, cha sở hữu con bằng `Rc`, còn con trỏ ngược lên cha bằng `Weak` — tham chiếu không sở hữu nên không tạo vòng và không rò[^book-tree-own][^std-weak-def]. `Arc` là con trỏ đếm tham chiếu thread-safe, dùng thao tác atomic cho bộ đếm nên chia sẻ được giữa các thread, với cái giá là atomic đắt hơn truy cập bộ nhớ thường[^std-arc-def][^std-arc-atomic]. Nếu không chia sẻ allocation giữa các thread thì docs khuyên dùng `Rc`[^std-arc-rc].

**Giải thích.**

Hình dung (nhận xét của người soạn): `Rc` là thẻ thư viện chung của một cuốn sách — sách chỉ được trả khi thẻ cuối cùng bị hủy; `Weak` là tờ ghi chú "cuốn này ở đâu" không giữ sách lại.

### 4.1 `Rc<T>`

Std docs: `Rc` là "con trỏ đếm tham chiếu đơn thread".[^std-rc-def] Book mô tả `Rc<T>` theo dõi số tham chiếu tới một giá trị để biết nó còn được dùng hay không.[^book-rc-count] Nó chỉ cho truy cập qua reference bất biến, để tránh vi phạm quy tắc borrowing,[^book-rc-immut] và Book nhấn mạnh nó chỉ dành cho kịch bản một thread.[^book-rc-single] Trạng thái "không `Send`" được hiện thực qua `impl !Send for Rc<T, A>`.[^std-rc-notsend] Lỗi thật khi đưa `Rc` sang thread:

```
error[E0277]: `Rc<i32>` cannot be sent between threads safely
  = help: within `{closure@d.rs:2:51: 2:58}`, the trait `Send` is not implemented for `Rc<i32>`
```

Số đo thật: `size_of::<Rc<i32>>() = 8`, `Option<Rc<i32>> = 8` (quan sát: `Rc` không nằm trong danh sách kiểu std bảo đảm niche, xem [P10·02 mục 6](../10-rust-internals/02-type-layout.md)). Chi tiết "bộ đếm nằm ở đâu" xem [P3·08 mục 6](../03-memory/08-rust-va-bo-nho.md).

### 4.2 Cây cha–con với `Weak`

Nếu cha giữ `Rc` tới con *và* con giữ `Rc` tới cha thì tạo vòng, không bao giờ về 0. Book dùng đúng ví dụ này: cha nên sở hữu con (cha bị drop thì con cũng bị drop), con không nên sở hữu cha — đó là trường hợp dùng weak reference.[^book-tree-own] `Weak` là phiên bản `Rc` giữ tham chiếu không sở hữu: nó không ngăn giá trị bị drop, nên `upgrade` có thể trả `None`; nhưng nó *có* ngăn vùng nhớ nền (backing store) bị giải phóng.[^std-weak-def][^std-weak-backing]

```rust
use std::cell::RefCell;
use std::rc::{Rc, Weak};
#[derive(Debug)]
struct Node { v: i32, parent: RefCell<Weak<Node>>, children: RefCell<Vec<Rc<Node>>> }
```

Chạy thật (code đầy đủ theo Book Listing 15-28/29, `leaf.v=3`, `branch.v=5`):

```
leaf parent = None
leaf strong=1 weak=0
branch strong=1 weak=1
leaf strong=2 weak=0
leaf parent = Some(5)
leaf parent = None        // sau khi branch ra khỏi scope
leaf strong=1 weak=0
```

Đọc: `branch` có 1 strong (biến) và 1 weak (trỏ từ `leaf.parent`); `leaf` có 2 strong (biến + `branch.children`). Khi `branch` hết scope, strong về 0 nên node bị drop, `upgrade()` trả `None`, và `leaf` về lại 1 strong — không rò. Book mô tả `Weak::upgrade` trả `Option<Rc<T>>` và cần gọi nó để bảo đảm giá trị còn tồn tại trước khi dùng.[^book-weak-upgrade]

### 4.3 Vòng tham chiếu vẫn rò, và rò là safe

Book: ngăn rò bộ nhớ hoàn toàn không phải là bảo đảm của Rust, nên leak là memory safe; ví dụ là `Rc` + `RefCell` tạo vòng, vì bộ đếm của mỗi phần tử không bao giờ về 0.[^book-leak-safe][^book-leak-cycle] Thí nghiệm thật: hai node `a`, `b` trỏ nhau bằng `Rc<N>` (có `impl Drop` in `drop a`/`drop b`). Chương trình in `a=2 b=2`, thoát với mã 0 và **không in dòng drop nào** — destructor không bao giờ chạy.

<details><summary>Đi sâu: <code>Rc&lt;T&gt;</code> còn lại gì khi bạn cần sửa (<code>get_mut</code>, <code>make_mut</code>)</summary>

### 4.4 `Rc<T>` còn lại gì khi bạn cần sửa?

`Rc::get_mut` chỉ trả `&mut` khi không còn `Rc` hay `Weak` nào khác trỏ tới cùng allocation (nếu không trả `None`);[^std-rc-getmut] `Rc::make_mut` clone-on-write (xem §6).[^std-cow-makemut] Còn khi bạn muốn nhiều owner **và** sửa — đó là lúc `RefCell` (§5.2) vào cuộc.

</details>

### 4.5 `Arc<T>`: khi nào

Hình dung (nhận xét của người soạn): `Arc` giống `Rc` nhưng sổ đếm được khóa chống nhiều người ghi cùng lúc — an toàn hơn, chậm hơn.

`Arc` là "Atomically Reference Counted": con trỏ đếm tham chiếu thread-safe cho giá trị cấp phát trên heap.[^std-arc-def] Khác `Rc`, nó dùng thao tác atomic cho bộ đếm, nên thread-safe; cái giá là atomic đắt hơn truy cập bộ nhớ thường.[^std-arc-atomic] Docs khuyên: nếu không chia sẻ allocation giữa các thread thì dùng `Rc` cho overhead thấp hơn.[^std-arc-rc] Cơ sở `Send`/`Sync` và ví dụ chia sẻ giữa thread xem [P4·08 §4](../04-concurrency/08-rust-concurrency.md).

Chạy thật: 2 thread đọc `Arc::new(5)` cộng `i` in `5` và `6`. Số đo thật: `size_of::<Arc<i32>>() = 8`.

<details><summary>Đi sâu: <code>Arc</code> chỉ cho <code>&amp;T</code></summary>

Dù đã là `Arc`, bạn vẫn chỉ có `&T`; muốn sửa thì `Arc<Mutex<T>>`/`Arc<RwLock<T>>` — bàn ở P4·08, không lặp ở đây.

</details>

**Quay lại Q4:** `Rc` cho nhiều owner đọc, `Weak` làm con trỏ ngược để cây không rò; vòng `Rc` thuần vẫn rò dù safe. `Arc` khi qua ranh giới thread, `Rc` khi không.

## 5. Interior mutability: `Cell`, `RefCell`, `Rc<RefCell<T>>`, `OnceCell`/`LazyCell` (Q5)

> **Trả lời ngắn.** Interior mutability cho phép sửa dữ liệu dù chỉ có reference bất biến tới nó[^book-interior]. `Cell` chuyển giá trị vào/ra khỏi ô và không bao giờ cho mượn `&T`[^std-cell-moves]; `RefCell` kiểm quy tắc borrow lúc chạy và panic nếu vi phạm[^book-refcell-panic]; `try_borrow_mut` là bản không panic[^std-try-borrowmut]. Book kết hợp `Rc<T>` (nhiều owner nhưng chỉ đọc) với `RefCell<T>` để có giá trị nhiều owner **và** sửa được[^book-rc-refcell]. Giá phải trả (nhận xét của người soạn): panic lúc chạy khi vi phạm borrow, rò bộ nhớ qua vòng, và không thread-safe; với cây/đồ thị lớn có thể dùng `Vec` + chỉ số. `OnceCell<T>` là ô về danh nghĩa chỉ ghi được một lần, cho lấy `&T` mà không cần kiểm borrow lúc chạy[^std-oncecell-def]. `LazyCell<T, F>` là giá trị khởi tạo ở lần truy cập đầu tiên[^std-lazycell-def].

**Giải thích.**

Hình dung (nhận xét của người soạn): `Cell` là hộp thư — chỉ bỏ vào hoặc lấy ra cả thư, không ai được thò tay đọc giữa chừng; `RefCell` là phòng đọc có người gác cổng, vi phạm nội quy lúc đang chạy thì bị đuổi (panic).

**Vấn đề.** Quy tắc borrow thông thường là "một `&mut` hoặc nhiều `&`", kiểm lúc compile. Có những thiết kế hợp lệ nhưng compiler không chứng minh được (static analysis "thận trọng" tự nhiên — Book).[^book-conservative] Interior mutability cho phép sửa dữ liệu dù đang có reference bất biến tới nó; mẫu này dùng `unsafe` bên trong cấu trúc dữ liệu để "bẻ" luật thông thường.[^book-interior]

### 5.1 `Cell<T>`: thay cả giá trị, không bao giờ cho mượn

`Cell<T>` cài interior mutability bằng cách *chuyển giá trị vào/ra* khỏi ô: không bao giờ lấy được `&T` tới bên trong.[^std-cell-moves] Không có borrow nên không có gì để panic:

```rust
let c = Cell::new(5); c.set(c.get()+1);       // 6
let c = Cell::new(String::from("a"));
let old = c.take();                            // "a", để lại String rỗng
```

Chạy thật: `cell 6`; `take` trả `"a"` rồi lần sau `""`. `Cell<T>` là `!Sync`,[^std-cell-notsync] nên không chia sẻ `&Cell<T>` giữa các thread được; nhưng `Cell<T>` vẫn là `Send` khi `T: Send`,[^std-cell-send] tức là chuyển (move) được sang thread khác — khác `Rc`, vốn `!Send` (xem [P4·08 mục 5](../04-concurrency/08-rust-concurrency.md)). `size_of::<Cell<i32>>() = 4` (quan sát: không thêm cờ đếm).

### 5.2 `RefCell<T>`: borrow checker chạy lúc runtime

std: `RefCell` là "vùng nhớ mutable với quy tắc borrow được kiểm tra động".[^std-refcell-def] Book nói với `RefCell<T>` các bất biến này được thực thi lúc runtime, và nếu vi phạm thì chương trình panic rồi thoát.[^book-refcell-runtime][^book-refcell-panic] Quy tắc: nhiều `borrow()` đồng thời được phép;[^std-borrow-multi] `borrow()` panic nếu đang bị mượn mutable;[^std-borrow-panic] `borrow_mut()` panic nếu đang bị mượn bất kỳ kiểu nào.[^std-borrowmut-panic] Có bản không panic: `try_borrow_mut` trả `Result<RefMut, BorrowMutError>` ("variant không panic của `borrow_mut`").[^std-try-borrowmut]

Chạy thật:

```rust
let r = RefCell::new(vec![1]);
{ let a = r.borrow();
  let t = r.try_borrow_mut();
  println!("try_borrow_mut while borrowed: {:?}", t.is_err());   // true
}
let _g1 = r.borrow_mut();
println!("{:?}", r.try_borrow_mut().map(|_|()).unwrap_err());    // BorrowMutError
let _g2 = r.borrow_mut();                                        // panic
```

```
try_borrow_mut while borrowed: true
BorrowMutError

thread 'main' panicked at b.rs:23:17:
RefCell already borrowed
```

Tiến trình thoát mã **101** (panic). Chiều ngược lại: giữ `borrow_mut()` rồi `try_borrow()` cho `BorrowError`.

`RefCell` nặng hơn `Cell`: `size_of::<RefCell<i32>>() = 16` so với `Cell<i32> = 4` (quan sát; phần chênh là trạng thái borrow).

<details><summary>Đi sâu: vì sao panic này khó thấy (reentrancy)</summary>

### 5.3 Vì sao panic này khó thấy

Vi phạm borrow lúc runtime thường đến từ **reentrancy**, không phải hai dòng cạnh nhau như ví dụ trên. Thí nghiệm thật:

```rust
let shared = Rc::new(RefCell::new(vec![1,2,3]));
let s2 = shared.clone();
for x in shared.borrow().iter() { if *x==2 { s2.borrow_mut().push(9); } }
```

Chương trình panic tại dòng `borrow_mut` với `RefCell already borrowed`, mã thoát 101: vòng `for` giữ `Ref` suốt thời gian lặp, và code bên trong lại mượn mutable qua một handle khác. (Nhận xét của người soạn: loại lỗi này dễ chỉ nổ ở một đường dữ liệu hiếm mà test không chạm tới.)

</details>

### 5.4 `Rc<RefCell<T>>`: pattern phổ biến và cảnh báo

Hình dung (nhận xét của người soạn): nhiều người cùng giữ chìa khóa một phòng có người gác cổng — tiện, nhưng bạn đổi lỗi lúc biên dịch lấy lỗi lúc chạy.

Book: kết hợp `Rc<T>` (nhiều owner nhưng chỉ đọc) với `RefCell<T>` cho ra giá trị có nhiều owner **và** sửa được.[^book-rc-refcell]

```rust
let shared = Rc::new(RefCell::new(vec![1]));
let s2 = Rc::clone(&shared);
s2.borrow_mut().push(2);
// shared.borrow() = [1, 2], strong_count = 2
```

Chạy thật in `[1, 2] rc=2`. Pattern này chính là công cụ để dựng đồ thị, observer, cây có tham chiếu ngược — những cấu trúc ownership không thành cây.

**Cảnh báo (nhận xét của người soạn, dựa trên hai thí nghiệm ở §4.3 và §5.3):**

1. *Panic runtime*: tham chiếu động nghĩa là bạn đổi lỗi compile lấy lỗi chạy; mỗi `borrow_mut` là một chỗ có thể panic.
2. *Rò bộ nhớ qua vòng*: `Rc<RefCell<…>>` hai chiều rò mà compiler không báo (§4.3) — phải chủ động dùng `Weak`.
3. *Không qua thread*: cả `Rc` lẫn `RefCell` đều không thread-safe; sang đa luồng phải đổi thành `Arc<Mutex<…>>`, tức đổi cả API.

<details><summary>Đi sâu: khi nào tránh — arena/index</summary>

### 5.5 Khi nào tránh — arena/index

Với cây hoặc đồ thị mà vòng đời node gắn với cả cấu trúc, lựa chọn thường dùng là **lưu node trong một `Vec` và tham chiếu nhau bằng chỉ số** (arena/index). Toàn bộ phần này là nhận xét của người soạn. Ví dụ chạy thật:

```rust
struct Tree { nodes: Vec<TNode> }
struct TNode { val: i32, parent: Option<usize>, children: Vec<usize> }
impl Tree {
  fn add(&mut self, val: i32, parent: Option<usize>) -> usize {
    let id = self.nodes.len();
    self.nodes.push(TNode { val, parent, children: vec![] });
    if let Some(p) = parent { self.nodes[p].children.push(id); }
    id
  }
  fn depth(&self, mut i: usize) -> usize {
    let mut d = 0; while let Some(p) = self.nodes[i].parent { d += 1; i = p; } d
  }
}
```

Dựng cây 3 mức, in `depth g=2 children of r=[1] val=3`. Ưu điểm so với `Rc<RefCell>`: một owner (`Tree`), borrow checker thường vẫn kiểm tra tại compile time, không vòng tham chiếu (vì không có con trỏ), không panic borrow, dễ `Send`. Nhược điểm: chỉ số là "con trỏ không an toàn kiểu" — xóa node làm chỉ số cũ trỏ sai node (kiểu lỗi ABA); cần thêm generation hoặc không bao giờ xóa. Chọn theo bài toán, không theo thói quen.

</details>

### 5.6 `OnceCell` và `LazyCell`: khởi tạo một lần

Hình dung (nhận xét của người soạn): `OnceCell` là biên bản ký một lần rồi niêm phong; `LazyCell` là món ăn chỉ nấu khi có khách đầu tiên gọi.

**`OnceCell<T>`** là ô "về danh nghĩa chỉ ghi được một lần": lấy được `&T` mà không copy hay thay thế (khác `Cell`) và không kiểm borrow runtime (khác `RefCell`).[^std-oncecell-def] `set` trả `Ok(())` nếu ô chưa khởi tạo, `Err(value)` nếu đã có.[^std-oncecell-set] `get_or_init` còn cảnh báo: khởi tạo reentrant từ chính closure là lỗi và panic.[^std-oncecell-reent]

**`LazyCell<T, F>`** là giá trị khởi tạo lúc truy cập đầu tiên; phiên bản thread-safe là `std::sync::LazyLock`.[^std-lazycell-def]

```rust
let cell: OnceCell<String> = OnceCell::new();
println!("{:?}", cell.get());                                  // None
println!("{}", cell.get_or_init(|| "first".into()));           // first
println!("{:?}", cell.set("second".into()));                   // Err("second")
let lazy = LazyCell::new(|| { println!("init!"); 42 });
println!("before"); println!("{}", *lazy); println!("{}", *lazy);
```

<details><summary>Đi sâu: kết quả chạy thật, bản đa luồng và <code>!Sync</code></summary>

Đầu ra thật: `None`, `first`, `Err("second")`, rồi `before`, `init!`, `42`, `42` — closure chạy đúng một lần, ở lần deref đầu. Cả hai kiểu biên dịch trên 1.95.0 mà không cần `#![feature]` (quan sát từ ví dụ trên). Phiên bản đa luồng (`OnceLock`/`LazyLock`) xem [P4·08 mục 7](../04-concurrency/08-rust-concurrency.md). Cả `OnceCell` lẫn `LazyCell` đều `!Sync` (không chia sẻ `&` giữa thread được)[^std-oncecell-notsync][^std-lazycell-notsync], nhưng vẫn `Send` khi giá trị bên trong là `Send`.[^std-oncecell-send][^std-lazycell-send]

</details>

**Quay lại Q5:** `Cell` không bao giờ cho mượn nên không panic; `RefCell` kiểm borrow lúc chạy và panic khi vi phạm. `Rc<RefCell<T>>` cho nhiều owner + sửa được, đổi lấy panic runtime, rò vòng và không thread-safe. `OnceCell` ghi một lần, `LazyCell` khởi tạo ở lần truy cập đầu; bản đa luồng là `OnceLock`/`LazyLock`.

## 6. `Cow<'a, B>`: mượn nếu được, sở hữu nếu cần (Q6)

> **Trả lời ngắn.** `Cow` là smart pointer clone-on-write: bao bọc và cho truy cập bất biến tới dữ liệu được mượn, và chỉ clone khi cần sửa hoặc cần sở hữu[^std-cow-def]. Nhờ vậy hàm trả `Cow` không phải cấp phát khi input đã sạch.

**Giải thích.**

Hình dung (nhận xét của người soạn): bạn mượn bản photo của đồng nghiệp để đọc; chỉ khi cần gạch chú thích mới photo riêng bản của mình.

**Vấn đề.** Hàm chuẩn hoá chuỗi: đa số input đã chuẩn, chỉ vài cái cần sửa. Trả `String` luôn thì cấp phát thừa; trả `&str` thì không sửa được. std: `Cow` là smart pointer clone-on-write — có thể bao bọc và cho truy cập bất biến tới dữ liệu được mượn, và clone dữ liệu một cách lazy khi cần sửa hoặc cần sở hữu.[^std-cow-def] Nó cài `Deref`, nên gọi được method không mutating trực tiếp; muốn sửa thì `to_mut` lấy `&mut` tới giá trị sở hữu, clone nếu cần.[^std-cow-deref][^std-cow-tomut]

```rust
use std::borrow::Cow;
fn norm(s: &str) -> Cow<'_, str> {
    if s.contains(' ') { Cow::Owned(s.replace(' ', "_")) } else { Cow::Borrowed(s) }
}
```

Chạy thật:

```
a b -> a_b borrowed=false
ab -> ab borrowed=true
xy! owned=true      // norm("xy") rồi c.to_mut().push('!')
```

<details><summary>Đi sâu: kích thước, <code>make_mut</code> và trade-off</summary>

Tức: input cần sửa thì được cấp phát; input sạch thì **không** cấp phát. `size_of::<Cow<str>>() = 24`, bằng `size_of::<String>()` trong đo này (quan sát; do niche, không phải bảo đảm). Với `Rc`/`Arc` có biến thể tương tự: `Rc::make_mut` và `Arc::make_mut`.[^std-cow-makemut]

Trade-off (nhận xét của người soạn): `Cow` thêm lifetime `'a` vào chữ ký; chỉ đáng khi đa số trường hợp không cần sở hữu.

</details>

**Quay lại Q6:** `Cow` mượn khi đọc, chỉ clone khi cần sửa; đổi lại thêm lifetime `'a` vào chữ ký.

## 7. Tự viết smart pointer: guard với `Deref`/`DerefMut`/`Drop` (Q7)

> **Trả lời ngắn.** Một guard tự viết gồm ba mảnh: giữ một tham chiếu, cài `Deref`/`DerefMut` để dùng như kiểu bên trong, và cài `Drop` để dọn dẹp khi hết scope. Ví dụ bên dưới chạy thật.

**Giải thích.**

Hình dung (nhận xét của người soạn): guard là thẻ ra vào tạm — dùng thẻ để mở cửa, và khi bạn rời phòng thẻ tự được trả lại.

Ghép lại ba mảnh: một guard mượn `&mut Vec<i32>`, cho dùng như `Vec` (qua `Deref`/`DerefMut`), và làm việc dọn dẹp khi bị drop — mô hình của `MutexGuard`/`Ref`/`RefMut`:

```rust
use std::ops::{Deref, DerefMut};
struct Guard<'a> { data: &'a mut Vec<i32>, name: &'static str }
impl Deref for Guard<'_> { type Target = Vec<i32>; fn deref(&self) -> &Vec<i32> { self.data } }
impl DerefMut for Guard<'_> { fn deref_mut(&mut self) -> &mut Vec<i32> { self.data } }
impl Drop for Guard<'_> {
    fn drop(&mut self) { println!("drop guard {} (len={})", self.name, self.data.len()); }
}
fn main() {
    let mut v = vec![1];
    { let mut g = Guard { data: &mut v, name: "g" };
      g.push(2);                                   // DerefMut: Vec::push
      println!("len via deref {}", g.len()); }     // Deref: Vec::len
    println!("{:?}", v);
}
```

Đầu ra thật:

```
len via deref 2
drop guard g (len=2)
[1, 2]
```

`g.push(2)` gọi được vì auto-deref đi qua `DerefMut`; `drop` chạy ở cuối khối trong, đúng lúc `v` được mượn lại (ở đây `Guard` giữ `&'a mut`, nên dòng in `v` chỉ biên dịch được sau khi `g` hết đời — borrow checker kiểm).

<details><summary>Đi sâu: guard và dọn dẹp gắn với scope</summary>

Guard gắn việc dọn dẹp vào scope mà không cần gọi tay (xem [P3·08 mục 3](../03-memory/08-rust-va-bo-nho.md)). (Nhận xét của người soạn: chỉ cài `Deref` cho kiểu thực sự là con trỏ; dùng nó để "kế thừa" method kiểu OOP dễ gây bất ngờ.)

</details>

**Quay lại Q7:** `Deref`/`DerefMut` cho dùng như kiểu bên trong, `Drop` gắn việc dọn dẹp vào scope.

## Liên hệ với Rust

Toàn chương này là Rust, nên phần "liên hệ" là bản đồ chọn kiểu (nhận xét của người soạn, từ các nguồn và thí nghiệm ở trên):

| Tình huống | Chọn | Vì sao |
|---|---|---|
| Kiểu đệ quy, trait object sở hữu, giá trị lớn | `Box<T>` | sở hữu duy nhất, 1 con trỏ |
| Nhiều owner, một thread, chỉ đọc | `Rc<T>` | bộ đếm rẻ |
| Quan hệ ngược (con→cha, observer) | `Weak<T>` | không giữ sống, không vòng |
| Nhiều owner, nhiều thread | `Arc<T>` (+ `Mutex`) | atomic |
| Sửa qua `&`, kiểu `Copy`/`take`-able | `Cell<T>` | không borrow, không panic |
| Sửa qua `&`, cần `&T`/`&mut T` bên trong | `RefCell<T>` | kiểm runtime, panic nếu sai |
| Trả về "thường mượn, đôi khi sở hữu" | `Cow<'a, B>` | tránh cấp phát thừa |
| Khởi tạo lười một lần | `OnceCell`/`LazyCell` (`OnceLock`/`LazyLock` nếu đa luồng) | không cần `RefCell<Option<T>>` |
| Kiểu lỗi chung | `Box<dyn Error>` | `?` hội tụ mọi lỗi |
| Cây/đồ thị lớn, vòng đời chung | `Vec` + chỉ số | tránh `Rc<RefCell>` |

## Tóm tắt

- Smart pointer = struct sở hữu dữ liệu, cài `Deref` (dùng như reference) + `Drop` (dọn dẹp).
- `Box<T>`: sở hữu duy nhất một allocation; giải bài toán kiểu đệ quy (E0072), trait object (`Box<dyn Trait>` = 16 byte hai từ trên aarch64), chuyển giá trị lớn.
- `Rc<T>` chỉ cho `&T`, một thread (`!Send`); `Weak<T>` phá vòng, `upgrade()` trả `Option`.
- Vòng `Rc` rò mà vẫn safe: destructor không chạy, thí nghiệm in `a=2 b=2` và không có `drop`.
- `Arc<T>` = `Rc` với atomic; chọn khi qua ranh giới thread.
- `Cell` chuyển giá trị vào/ra (không panic); `RefCell` kiểm borrow lúc chạy, vi phạm → panic (mã thoát 101), `try_borrow_mut` trả `BorrowMutError`.
- `Rc<RefCell<T>>` cho nhiều owner + sửa được, nhưng đánh đổi: panic runtime, rò vòng, không thread-safe; cân nhắc arena/index.
- `Cow` tránh cấp phát khi không cần; `OnceCell`/`LazyCell` khởi tạo một lần; `Box<dyn Error>` gom lỗi; `Pin<Box<T>>` là `Box` ghim (xem P7·06).
- `Drop`: biến drop ngược thứ tự khai báo, field theo thứ tự khai báo; `mem::drop` chỉ là hàm nhận ownership; `let _ = <temporary>` drop ngay cuối câu lệnh, còn `let _ = x` với biến có sẵn không move và không drop `x`.

## Câu hỏi tự kiểm tra

1. Hai trait nào phân biệt smart pointer với struct thường, mỗi trait lo việc gì?
<details><summary>Đáp án</summary>

`Deref` (và `DerefMut`) cho dùng như reference/deref coercion; `Drop` cho dọn dẹp khi ra scope.[^book-sp-traits][^std-deref-sp]

</details>

2. Vì sao `enum List { Cons(i32, List), Nil }` không biên dịch và sửa thế nào?
<details><summary>Đáp án</summary>

E0072 vì kích thước vô hạn; thêm indirection như `Box<List>` vì box có kích thước biết trước.[^book-box-recursive]

</details>

3. `Box<dyn Shape>` khác `Box<i32>` về kích thước ra sao trên máy này, và vì sao?
<details><summary>Đáp án</summary>

16 byte vs 8 (quan sát trên aarch64): con trỏ dữ liệu cộng thông tin tra method lúc chạy; trait object trỏ tới instance và bảng tra method.[^book-trait-obj]

</details>

4. Trong cây cha–con, phía nào dùng `Rc`, phía nào dùng `Weak`, vì sao?
<details><summary>Đáp án</summary>

Cha sở hữu con (`Rc` trong `children`), con trỏ ngược cha bằng `Weak` vì con không sở hữu cha.[^book-tree-own]

</details>

5. Vòng tham chiếu `Rc` có làm chương trình undefined behavior không?
<details><summary>Đáp án</summary>

Không: leak là memory safe trong Rust; hậu quả là destructor không chạy.[^book-leak-safe]

</details>

6. Khi nào chọn `Arc` thay `Rc`?
<details><summary>Đáp án</summary>

Khi chia sẻ qua thread; `Arc` dùng atomic nên đắt hơn, nên nếu không qua thread thì dùng `Rc`.[^std-arc-atomic][^std-arc-rc]

</details>

7. `RefCell::borrow_mut` trong lúc đang có `Ref` sống thì chuyện gì xảy ra; cách không panic?
<details><summary>Đáp án</summary>

Panic ("RefCell already borrowed", mã thoát 101 trong thí nghiệm); dùng `try_borrow_mut` trả `Err(BorrowMutError)`.[^std-borrowmut-panic][^std-try-borrowmut]

</details>

8. `Cell` và `RefCell` khác nhau ở cách truy cập nội dung như thế nào?
<details><summary>Đáp án</summary>

`Cell` chỉ chuyển giá trị vào/ra, không bao giờ đưa `&T`; `RefCell` đưa `Ref`/`RefMut` với borrow check động.[^std-cell-moves][^std-refcell-def]

</details>

9. Nêu ba rủi ro của `Rc<RefCell<T>>` và một thay thế.
<details><summary>Đáp án</summary>

Panic runtime, rò vòng, không thread-safe (nhận xét của người soạn, §5.4); thay thế: `Vec` + chỉ số. Nền: [^book-rc-refcell][^book-refcell-panic][^book-leak-cycle].

</details>

10. `Cow::to_mut` làm gì khi giá trị đang là `Borrowed`?
<details><summary>Đáp án</summary>

Clone thành giá trị sở hữu rồi trả `&mut`; thí nghiệm `norm("xy")` thành `Owned` sau `to_mut`.[^std-cow-tomut]

</details>

11. `let _ = D("x");` và `let _d = D("x");` khác nhau thế nào về lúc drop?
<details><summary>Đáp án</summary>

Cái đầu (vế phải là temporary) drop ngay cuối câu lệnh (thí nghiệm: `drop underscore` in trước `after underscore`); cái sau drop cuối scope theo thứ tự ngược khai báo.[^ref-scope-order][^ref-temp-scope] Lưu ý `let _ = x;` với biến `x` có sẵn thì `_` không move giá trị[^ref-wildcard] nên `x` vẫn drop ở cuối scope.

</details>

12. Vì sao không gọi được `x.drop()` và `mem::drop` làm gì?
<details><summary>Đáp án</summary>

Gọi tay sẽ gây double free vì compiler vẫn tự drop cuối scope; `mem::drop(x)` chỉ move `x` vào hàm rỗng để nó bị drop.[^book-drop-explicit][^std-mem-drop]

</details>

## Đọc thêm

- [Rust Book ch15 — Smart Pointers](https://doc.rust-lang.org/1.95.0/book/ch15-00-smart-pointers.html): nguồn gốc các ví dụ `Box`, `Rc`, `RefCell`, cây cha–con.
- [std::cell](https://doc.rust-lang.org/1.95.0/std/cell/index.html): khi nào chọn interior mutability, `Cell` vs `RefCell` vs `OnceCell`.
- [std::borrow::Cow](https://doc.rust-lang.org/1.95.0/std/borrow/enum.Cow.html): ví dụ `abs_all` clone-on-write.
- [Reference — Destructors](https://doc.rust-lang.org/1.95.0/reference/destructors.html): quy tắc đầy đủ về drop scope và thứ tự.
- [P3·08 — Rust và bộ nhớ](../03-memory/08-rust-va-bo-nho.md), [P4·08](../04-concurrency/08-rust-concurrency.md), [P7·06](../07-io-async/06-async-await-pin.md): các chương đã viết mà chương này dựa vào.

[^book-sp-def]: Rust Book, ch15 Smart Pointers — <https://doc.rust-lang.org/1.95.0/book/ch15-00-smart-pointers.html> — "Smart pointers, on the other hand, are data structures that act like a pointer but also have additional metadata and capabilities."
[^book-ref-plain]: Rust Book, ch15 Smart Pointers — <https://doc.rust-lang.org/1.95.0/book/ch15-00-smart-pointers.html> — "They don't have any special capabilities other than referring to data, and they have no overhead."
[^book-sp-own]: Rust Book, ch15 Smart Pointers — <https://doc.rust-lang.org/1.95.0/book/ch15-00-smart-pointers.html> — "While references only borrow data, in many cases smart pointers own the data they point to."
[^book-sp-traits]: Rust Book, ch15 Smart Pointers — <https://doc.rust-lang.org/1.95.0/book/ch15-00-smart-pointers.html> — "Unlike an ordinary struct, smart pointers implement the Deref and Drop traits."
[^std-deref-purpose]: std::ops::Deref — <https://doc.rust-lang.org/1.95.0/std/ops/trait.Deref.html> — "to change the ownership semantics of a contained value (for example, Rc or Cow) or the storage semantics of a contained value (for example, Box)"
[^std-deref-sp]: std::ops::Deref — <https://doc.rust-lang.org/1.95.0/std/ops/trait.Deref.html> — "the mechanism of deref coercion has been specifically designed to facilitate the pointer-like behavior that name suggests."
[^book-deref-coerce]: Rust Book, ch15-02 Deref — <https://doc.rust-lang.org/1.95.0/book/ch15-02-deref.html> — "Deref coercion converts a reference to a type that implements the Deref trait into a reference to another type."
[^book-deref-mut]: Rust Book, ch15-02 Deref — <https://doc.rust-lang.org/1.95.0/book/ch15-02-deref.html> — "Rust will also coerce a mutable reference to an immutable one."
[^std-box-intro]: std::boxed — <https://doc.rust-lang.org/1.95.0/std/boxed/index.html> — "Move a value from a Box back to the stack by dereferencing"
[^book-drop]: Rust Book, ch15-03 Drop — <https://doc.rust-lang.org/1.95.0/book/ch15-03-drop.html> — "You can provide an implementation for the Drop trait on any type, and that code can be used to release resources like files or network connections."
[^book-drop-rev]: Rust Book, ch15-03 Drop — <https://doc.rust-lang.org/1.95.0/book/ch15-03-drop.html> — "Variables are dropped in the reverse order of their creation"
[^ref-scope-order]: Rust Reference, Destructors — <https://doc.rust-lang.org/1.95.0/reference/destructors.html> — "all variables associated to that scope are dropped in reverse order of declaration (for variables) or creation (for temporaries)."
[^ref-field-order]: Rust Reference, Destructors — <https://doc.rust-lang.org/1.95.0/reference/destructors.html> — "The fields of a struct are dropped in declaration order."
[^book-drop-explicit]: Rust Book, ch15-03 Drop — <https://doc.rust-lang.org/1.95.0/book/ch15-03-drop.html> — "we use the std::mem::drop function"
[^std-mem-drop]: std::mem::drop — <https://doc.rust-lang.org/1.95.0/std/mem/fn.drop.html> — "This function is not magic; it is literally defined as pub fn drop\<T>(_x: T) {}"
[^std-copy-drop]: std::ops::Drop — <https://doc.rust-lang.org/1.95.0/std/ops/trait.Drop.html> — "You cannot implement both Copy and Drop on the same type."
[^std-box-def]: std::boxed::Box — <https://doc.rust-lang.org/1.95.0/std/boxed/struct.Box.html> — "A pointer type that uniquely owns a heap allocation of type T."
[^std-box-layout]: std::boxed, Memory layout — <https://doc.rust-lang.org/1.95.0/std/boxed/index.html> — "For non-zero-sized values, a Box will use the Global allocator for its allocation."
[^book-box-recursive]: Rust Book, ch15-01 Box — <https://doc.rust-lang.org/1.95.0/book/ch15-01-box.html> — "Because boxes have a known size, we can enable recursive types by inserting a box in the recursive type definition."
[^book-trait-obj]: Rust Book, ch18-02 Trait Objects — <https://doc.rust-lang.org/1.95.0/book/ch18-02-trait-objects.html> — "A trait object points to both an instance of a type implementing our specified trait and a table used to look up trait methods on that type at runtime."
[^book-box-large]: Rust Book, ch15-01 Box — <https://doc.rust-lang.org/1.95.0/book/ch15-01-box.html> — "we can store the large amount of data on the heap in a box."
[^std-rc-def]: std::rc::Rc — <https://doc.rust-lang.org/1.95.0/std/rc/struct.Rc.html> — "A single-threaded reference-counting pointer."
[^book-rc-count]: Rust Book, ch15-04 Rc — <https://doc.rust-lang.org/1.95.0/book/ch15-04-rc.html> — "type keeps track of the number of references to a value to determine whether or not the value is still in use."
[^book-rc-immut]: Rust Book, ch15-04 Rc — <https://doc.rust-lang.org/1.95.0/book/ch15-04-rc.html> — "Via immutable references, Rc\<T> allows you to share data between multiple parts of your program for reading only."
[^book-rc-single]: Rust Book, ch15-04 Rc — <https://doc.rust-lang.org/1.95.0/book/ch15-04-rc.html> — "Note that Rc\<T> is only for use in single-threaded scenarios."
[^std-rc-notsend]: std::rc::Rc, Trait Implementations — <https://doc.rust-lang.org/1.95.0/std/rc/struct.Rc.html> — "impl\<T, A> !Send for Rc\<T, A> where A: Allocator, T: ?Sized"
[^book-tree-own]: Rust Book, ch15-06 Reference Cycles — <https://doc.rust-lang.org/1.95.0/book/ch15-06-reference-cycles.html> — "a parent node should own its children: If a parent node is dropped, its child nodes should be dropped as well."
[^std-weak-def]: std::rc::Weak — <https://doc.rust-lang.org/1.95.0/std/rc/struct.Weak.html> — "Weak is a version of Rc that holds a non-owning reference to the managed allocation."
[^std-weak-backing]: std::rc::Weak — <https://doc.rust-lang.org/1.95.0/std/rc/struct.Weak.html> — "Note however that a Weak reference does prevent the allocation itself (the backing store) from being deallocated."
[^book-weak-upgrade]: Rust Book, ch15-06 Reference Cycles — <https://doc.rust-lang.org/1.95.0/book/ch15-06-reference-cycles.html> — "to do anything with the value that a Weak\<T> is pointing to you must make sure the value still exists."
[^book-leak-safe]: Rust Book, ch15-06 Reference Cycles — <https://doc.rust-lang.org/1.95.0/book/ch15-06-reference-cycles.html> — "meaning memory leaks are memory safe in Rust."
[^book-leak-cycle]: Rust Book, ch15-06 Reference Cycles — <https://doc.rust-lang.org/1.95.0/book/ch15-06-reference-cycles.html> — "This creates memory leaks because the reference count of each item in the cycle will never reach 0, and the values will never be dropped."
[^std-cow-makemut]: std::borrow::Cow — <https://doc.rust-lang.org/1.95.0/std/borrow/enum.Cow.html> — "Rc::make_mut and Arc::make_mut can provide clone-on-write functionality as well."
[^std-arc-def]: std::sync::Arc — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Arc.html> — "A thread-safe reference-counting pointer."
[^std-arc-atomic]: std::sync::Arc, Thread Safety — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Arc.html> — "Unlike Rc\<T>, Arc\<T> uses atomic operations for its reference counting. This means that it is thread-safe. The disadvantage is that atomic operations are more expensive than ordinary memory accesses."
[^std-arc-rc]: std::sync::Arc, Thread Safety — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Arc.html> — "If you are not sharing reference-counted allocations between threads, consider using Rc\<T> for lower overhead."
[^book-conservative]: Rust Book, ch15-05 RefCell — <https://doc.rust-lang.org/1.95.0/book/ch15-05-interior-mutability.html> — "Static analysis, like the Rust compiler, is inherently conservative."
[^book-interior]: Rust Book, ch15-05 RefCell — <https://doc.rust-lang.org/1.95.0/book/ch15-05-interior-mutability.html> — "Interior mutability is a design pattern in Rust that allows you to mutate data even when there are immutable references to that data"
[^std-cell-moves]: std::cell — <https://doc.rust-lang.org/1.95.0/std/cell/index.html> — "Cell\<T> implements interior mutability by moving values in and out of the cell."
[^std-cell-notsync]: std::cell::Cell, Trait Implementations — <https://doc.rust-lang.org/1.95.0/std/cell/struct.Cell.html> — "impl\<T> !Sync for Cell\<T> where T: ?Sized"
[^std-cell-send]: std::cell::Cell, Trait Implementations — <https://doc.rust-lang.org/1.95.0/std/cell/struct.Cell.html> — "impl\<T> Send for Cell\<T> where T: Send + ?Sized"
[^std-refcell-send]: std::cell::RefCell, Trait Implementations — <https://doc.rust-lang.org/1.95.0/std/cell/struct.RefCell.html> — "impl\<T> Send for RefCell\<T> where T: Send + ?Sized"
[^std-refcell-def]: std::cell::RefCell — <https://doc.rust-lang.org/1.95.0/std/cell/struct.RefCell.html> — "A mutable memory location with dynamically checked borrow rules"
[^book-refcell-runtime]: Rust Book, ch15-05 RefCell — <https://doc.rust-lang.org/1.95.0/book/ch15-05-interior-mutability.html> — "With RefCell\<T>, these invariants are enforced at runtime."
[^book-refcell-panic]: Rust Book, ch15-05 RefCell — <https://doc.rust-lang.org/1.95.0/book/ch15-05-interior-mutability.html> — "With RefCell\<T>, if you break these rules, your program will panic and exit."
[^std-borrow-multi]: std::cell::RefCell::borrow — <https://doc.rust-lang.org/1.95.0/std/cell/struct.RefCell.html> — "Multiple immutable borrows can be taken out at the same time."
[^std-borrow-panic]: std::cell::RefCell::borrow — <https://doc.rust-lang.org/1.95.0/std/cell/struct.RefCell.html> — "Panics if the value is currently mutably borrowed."
[^std-borrowmut-panic]: std::cell::RefCell::borrow_mut — <https://doc.rust-lang.org/1.95.0/std/cell/struct.RefCell.html> — "Panics if the value is currently borrowed. For a non-panicking variant, use try_borrow_mut."
[^std-try-borrowmut]: std::cell::RefCell::try_borrow_mut — <https://doc.rust-lang.org/1.95.0/std/cell/struct.RefCell.html> — "This is the non-panicking variant of borrow_mut."
[^ref-wildcard]: Rust Reference, Patterns, Wildcard pattern — <https://doc.rust-lang.org/1.95.0/reference/patterns.html> — "Unlike identifier patterns, it does not copy, move or borrow the value it matches."
[^ref-temp-scope]: Rust Reference, Destructors, Temporary scopes — <https://doc.rust-lang.org/1.95.0/reference/destructors.html> — "the temporary scope of an expression is the smallest scope that contains the expression and is one of the following: The entire function. A statement."
[^std-rc-getmut]: std::rc::Rc::get_mut — <https://doc.rust-lang.org/1.95.0/std/rc/struct.Rc.html> — "Returns a mutable reference into the given Rc, if there are no other Rc or Weak pointers to the same allocation."
[^book-rc-refcell]: Rust Book, ch15-05 RefCell — <https://doc.rust-lang.org/1.95.0/book/ch15-05-interior-mutability.html> — "If you have an Rc\<T> that holds a RefCell\<T>, you can get a value that can have multiple owners and that you can mutate!"
[^std-cow-def]: std::borrow::Cow — <https://doc.rust-lang.org/1.95.0/std/borrow/enum.Cow.html> — "it can enclose and provide immutable access to borrowed data, and clone the data lazily when mutation or ownership is required."
[^std-cow-deref]: std::borrow::Cow — <https://doc.rust-lang.org/1.95.0/std/borrow/enum.Cow.html> — "Cow implements Deref, which means that you can call non-mutating methods directly on the data it encloses."
[^std-cow-tomut]: std::borrow::Cow — <https://doc.rust-lang.org/1.95.0/std/borrow/enum.Cow.html> — "to_mut will obtain a mutable reference to an owned value, cloning if necessary."
[^std-oncecell-def]: std::cell::OnceCell — <https://doc.rust-lang.org/1.95.0/std/cell/struct.OnceCell.html> — "A cell which can nominally be written to only once."
[^std-oncecell-set]: std::cell::OnceCell::set — <https://doc.rust-lang.org/1.95.0/std/cell/struct.OnceCell.html> — "This method returns Ok(()) if the cell was uninitialized and Err(value) if it was already initialized."
[^std-oncecell-reent]: std::cell::OnceCell::get_or_init — <https://doc.rust-lang.org/1.95.0/std/cell/struct.OnceCell.html> — "It is an error to reentrantly initialize the cell from f. Doing so results in a panic."
[^std-oncecell-notsync]: std::cell::OnceCell, Trait Implementations — <https://doc.rust-lang.org/1.95.0/std/cell/struct.OnceCell.html> — "impl\<T> !Sync for OnceCell\<T>"
[^std-lazycell-notsync]: std::cell::LazyCell, Trait Implementations — <https://doc.rust-lang.org/1.95.0/std/cell/struct.LazyCell.html> — "impl\<T, F = fn() -> T> !Sync for LazyCell\<T, F>"
[^std-oncecell-send]: std::cell::OnceCell, Trait Implementations — <https://doc.rust-lang.org/1.95.0/std/cell/struct.OnceCell.html> — "impl\<T> Send for OnceCell\<T> where T: Send"
[^std-lazycell-send]: std::cell::LazyCell, Trait Implementations — <https://doc.rust-lang.org/1.95.0/std/cell/struct.LazyCell.html> — "impl\<T, F> Send for LazyCell\<T, F> where F: Send, T: Send"
[^std-lazycell-def]: std::cell::LazyCell — <https://doc.rust-lang.org/1.95.0/std/cell/struct.LazyCell.html> — "A value which is initialized on the first access. For a thread-safe version of this struct, see std::sync::LazyLock."
[^book-box-dyn-error]: Rust Book, ch09-02 Result — <https://doc.rust-lang.org/1.95.0/book/ch09-02-recoverable-errors-with-result.html> — "The Box\<dyn Error> type is a trait object"
[^std-pin-def]: std::pin::Pin — <https://doc.rust-lang.org/1.95.0/std/pin/struct.Pin.html> — "preventing the value referenced by that pointer from being moved or otherwise invalidated at that place in memory unless it implements Unpin."
[^std-pin-box]: std::pin — <https://doc.rust-lang.org/1.95.0/std/pin/index.html> — "a Pin\<Box\<T>> is an owned, pinning pointer to a pinned T"
[^std-pin-unpin]: std::pin — <https://doc.rust-lang.org/1.95.0/std/pin/index.html> — "When T: Unpin, Pin\<Box\<T>> functions identically to a non-pinning Box\<T>"
