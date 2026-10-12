# P9 · Chương 01 — Ownership, borrowing và lifetime nhìn sâu

Mọi ví dụ trong chương được biên dịch/chạy bằng `rustc 1.95.0` (edition 2024); các đoạn thông điệp lỗi dưới đây là đầu ra thật, đã rút gọn phần trang trí.

## Bạn sẽ hiểu được gì

1. **Q1.** Vì sao `let s = v[0];` bị E0507 còn `let n = u.name;` trên struct sở hữu thì được? → mục 2
2. **Q2.** `&mut` không `Copy` mà truyền được hai lần vào hàm: reborrow là gì? → mục 4
3. **Q3.** Lifetime thật sự là gì, vì sao "lifetime = thời gian sống của biến" gây lỗi? → mục 5
4. **Q4.** NLL đổi gì so với "borrow kéo dài tới hết scope"? → mục 6
5. **Q5.** Đọc và sửa E0499, E0502, E0505, E0506, E0597, E0716 thế nào? → mục 7
6. **Q6.** Ba luật elision áp dụng thế nào; `T: 'static` khác `&'static T` ra sao? → mục 8
7. **Q7.** Khi nào dùng `clone`, tách scope, index, `take`/`replace`/`mem::take`? → mục 9

## Kiến thức cần trước

Bạn đã đọc Rust Book tới chương về generics, trait và lifetime. Nếu cần ôn bộ nhớ (stack, heap, drop) thì xem [P3·08 — Rust và bộ nhớ](../03-memory/08-rust-va-bo-nho.md); `Send` và `'static` trong thread nằm ở [P4·08 — Rust concurrency](../04-concurrency/08-rust-concurrency.md). Chương này chỉ dừng ở mức người dùng, tức là đọc và sửa lỗi borrow checker báo; cơ chế bên trong của nó thuộc [P10·06 — Borrow checker](../10-rust-internals/06-borrow-checker.md).

## 1. Nhắc nhanh: ba luật ownership và hai luật borrow

*Với service của bạn.* Giả sử bạn viết `catalog`, một service HTTP bằng Rust trả thông tin sản phẩm (nhận xét của người soạn: ví dụ giả định, không phải code trong repo). Mỗi request vào, một handler nhận struct `Request` gồm `user` và `body`, tra tên sản phẩm trong một cache nằm trong bộ nhớ (`HashMap<u32, String>`), rồi giao việc ghi log cho một worker thread. Ba thứ đó, `Request`, cache và worker thread, là nơi chương này đặt câu hỏi: dữ liệu thuộc về ai, ai được mượn, và mượn tới lúc nào. Ta sẽ bám theo `catalog` suốt chương.

Câu trả lời bắt đầu từ ba luật của Rust Book. Mỗi giá trị có một owner[^own1]. Tại một thời điểm chỉ có một owner[^own2]. Khi owner ra khỏi scope thì giá trị bị drop[^own3]. Với tham chiếu thì có thêm một luật: tại một thời điểm bạn có thể có *hoặc* một tham chiếu mutable *hoặc* nhiều tham chiếu immutable[^bor1].

Cả chương là hệ quả của các luật này, và nó xoay quanh hai câu hỏi mà luật chưa trả lời. Một là "owner" nghĩa là gì khi giá trị nằm trong field, trong `Vec`, hay sau một con trỏ; đó là chuyện về **place** (mục 2). Hai là tham chiếu "còn hợp lệ" trong bao lâu; đó là chuyện về **lifetime** (mục 5).

## 2. Place expression và "move ra khỏi place" (Q1)

> **Trả lời ngắn.** Move chỉ hợp lệ với một số place nhất định, và Reference liệt kê hết chúng[^move7]: biến không đang bị borrow, temporary, field của place movable mà kiểu không implement `Drop`, và kết quả dereference một `Box`. `v[0]` (index vào `Vec`) hay `u.name` sau một tham chiếu không có trong danh sách (suy luận của người soạn) nên bị E0507; còn `u.name` trên struct *sở hữu* (không `Drop`) có trong danh sách nên được.

**Giải thích.**

*Với service `catalog`.* Handler nhận `req: Request` theo giá trị và muốn lấy `req.user` ra để giao cho worker thread; việc này chạy tốt. Nhưng nếu handler cầm `&Request`, hay chỉ có một `Vec<String>` các tên sản phẩm và viết `let s = v[0];`, compiler từ chối bằng E0507. Hai trường hợp giống nhau về hình thức mà khác nhau về kết quả, vì chỗ phân biệt không nằm ở kiểu dữ liệu mà ở **place**, tức vị trí trong bộ nhớ mà biểu thức chỉ tới.

### 2.1 Hai loại biểu thức

Reference chia biểu thức làm hai loại. Place expression là biểu thức đại diện cho một vị trí trong bộ nhớ[^place1]; mọi thứ còn lại là value expression. Place expression gồm biến cục bộ, static, dereference `*expr`, index `expr[expr]` và truy cập field `expr.f`[^place2].

Điều quyết định là **ngữ cảnh**. Khi một place expression đứng ở *value context* (vế phải của `let`, đối số hàm…) hoặc được bind *by value* trong pattern, nó có nghĩa là "lấy giá trị đang nằm ở vị trí đó"[^move1]. Nếu kiểu giá trị là `Copy` thì giá trị được sao chép[^move2]. Nếu không, đó là một **move**, và move chỉ hợp lệ với một số place nhất định.

### 2.2 Place nào được move ra

Danh sách đó nằm trọn trong Reference, mở đầu bằng câu "chỉ những place expression sau mới được move ra"[^move7]. Trong danh sách có:

- Biến không đang bị borrow[^move3].
- Temporary.
- Field của một place move ra được, với điều kiện kiểu của place đó *không* implement `Drop`[^move4].
- Kết quả dereference một `Box<T>`, nếu place đó cũng move ra được[^move5].

Danh sách không có dòng nào cho "bên trong `Vec`" hay "sau `&`/`&mut`". Đó chính là E0507.

Sau khi move ra khỏi một biến cục bộ, vị trí đó bị deinitialize và không đọc lại được cho tới khi gán lại[^move6]. Ví dụ:

```rust
fn main() {
    let a = String::from("a");
    let b = a;           // `a` là place expression ở value context: move
    println!("{a}");
}
```
Compiler báo `E0382 borrow of moved value: a` (đã chạy).

Cùng cơ chế đó, `for _ in 0..2 { consume(s); }` cho `E0382` "value moved here, in previous iteration of loop".

### 2.3 E0507: move ra khỏi sau tham chiếu hoặc khỏi `Vec`

Đoạn dưới gom hai cách hay vấp nhất: lấy một `String` ra khỏi `Vec`, và lấy một field ra khỏi `&User`.

```rust
struct User { name: String, age: u32 }
fn main() {
    let v = vec![String::from("a"), String::from("b")];
    let s = v[0];                                   // (1)
    let u = &User { name: "x".into(), age: 1 };
    let n = u.name;                                 // (2)
}
```
Hai lỗi `E0507`: `cannot move out of index of Vec<String>` (cho `v[0]`) và `cannot move out of u.name which is behind a shared reference`; rustc gợi ý mượn `&v[0]` hoặc `clone()`.

Lý do (suy luận của người soạn từ danh sách ở 2.2): `v[0]` và `u.name` sau `&` đều là place nằm sau một tham chiếu, và danh sách không có dòng nào cho loại place đó. Với `&mut` cũng vậy (`E0507 … behind a mutable reference`, đã chạy): mượn chỉ cho phép đọc hoặc ghi tại chỗ, không cho lấy mất.

Có ba cách thoát (mục 9 nói kỹ): mượn `&v[0]`, `clone()`, hoặc **thay thế** thứ bị lấy bằng một giá trị khác (`mem::take`, `mem::replace`, `Option::take`, `Vec::swap_remove`).

### 2.4 Partial move

Khi bạn là *owner* (không phải tham chiếu), việc move riêng từng field ra là hợp lệ. Đó là dòng "field của một place move ra được và không implement `Drop`" trong danh sách[^move4]:

```rust
struct User { name: String, email: String, age: u32 }
fn main() {
    let u = User { name: "an".into(), email: "a@x".into(), age: 30 };
    let n = u.name;                       // partial move
    println!("{} {}", u.email, u.age);    // OK: in "a@x 30"
    println!("{n}");                      // in "an"
}
```

Các field còn lại vẫn dùng được, nhưng dùng *cả* `u` thì không: `{:?}` của cả `u` ở sau đó cho `E0382: borrow of partially moved value: u` (đã chạy).

<details><summary>Đi sâu: kiểu có <code>Drop</code> (E0509) và partial move qua pattern</summary>

Khi `u` ra khỏi scope, chỉ các field còn khởi tạo mới bị drop[^dtor1]. Điều này **không** áp dụng nếu kiểu implement `Drop`. Chẳng hạn `Guard { name }` có `impl Drop` thì `let n = g.name;` cho `E0509: cannot move out of type Guard, which implements the Drop trait` (đã chạy). Đó chính là điều kiện "kiểu không implement `Drop`" của danh sách[^move4], và lý do là `drop` cần toàn bộ `self` còn nguyên.

Partial move còn xuất hiện ở một chỗ ít ai để ý: **pattern**. `if let Some(s) = o2 { … }` với `o2: Option<String>` bind `s` *by value* nên move `String` ra khỏi `o2` (Reference xếp việc bind by value trong pattern cùng loại với value context[^move1]); dùng `o2` tiếp thì `E0382 … partially moved`. Viết `if let Some(s) = &o2` thì `s: &String` và `o2` còn nguyên (đã chạy cả hai).

</details>

**Quay lại Q1:** move chỉ ra khỏi những place nằm trong danh sách của Reference; place sau tham chiếu hay bên trong `Vec` thì không, nên với `catalog` bạn phải mượn, `clone` hoặc thay thế (mục 9).

## 3. Shared XOR mutable, aliasing

*Với service `catalog`.* Hãy hình dung một handler đang đọc tên sản phẩm trong cache, trong lúc một chỗ khác trong code ghi đè đúng ô đó. Ở ngôn ngữ khác, bạn chỉ phát hiện ra lỗi này khi chạy. Rust chặn nó ngay lúc biên dịch bằng luật "chia sẻ hoặc mutable, không cả hai", và mục này nói luật đó từ đâu ra và hoạt động trên cái gì.

Động cơ trước hết là data race. Hai hay nhiều con trỏ cùng truy cập một dữ liệu tại cùng một lúc là một trong ba điều kiện gây ra data race[^race1]. Còn ở mức biểu thức, cơ chế thực hiện nằm ở toán tử borrow: nó đặt vị trí được mượn vào trạng thái "bị mượn" suốt thời gian reference còn tồn tại, và với `&` thì place không được ghi nhưng vẫn đọc và chia sẻ tiếp được[^bor2]. Với `&mut` thì place không được truy cập bằng bất cứ cách nào cho tới khi borrow hết hạn[^bor3].

Luật này còn có lợi cho tối ưu hóa. Hai biến hay hai con trỏ gọi là **alias** khi chúng trỏ tới các vùng bộ nhớ chồng lên nhau[^alias1]. Vì `&mut` không được phép alias, compiler loại trừ được những lời gọi như `compute(&x, &mut x)` và tối ưu theo giả định đó[^alias2].

Hai điều cần nhớ khi áp dụng:

- `&T` là `Copy`, `&mut T` thì không: tài liệu của `Copy` ghi rõ tham chiếu chia sẻ sao chép được còn tham chiếu mutable thì không[^copy1]. Lý do là sao chép `&mut T` sẽ tạo ra một mutable reference bị alias[^copy2].
- Luật áp dụng theo **place**. Hai field rời nhau mượn `&mut` đồng thời được; `split_at_mut` làm điều tương tự cho slice (đã chạy).

## 4. Reborrow và `&mut` tự động reborrow (Q2)

> **Trả lời ngắn.** `&mut T` không `Copy`[^copy1], nhưng khi kiểu tham số đích *đã biết là* `&mut T`, compiler chèn một **reborrow** `&mut *r`: một borrow mới, ngắn hơn, của place `*r`, nên xong hàm là `r` dùng lại được. Phần "tự động" là quan sát từ rustc 1.95.0, không phải lời hứa trích từ docs; với tham số generic thì `r` bị move.

**Giải thích.**

*Với service `catalog`.* Giả sử handler giữ một `r: &mut Vec<..>` để gom các dòng log của request, rồi truyền `r` lần lượt cho vài hàm phụ. Vì `&mut` không `Copy`, lần truyền đầu trông như đã giao hẳn `r` đi, và lần dùng thứ hai lẽ ra phải lỗi. Thực tế bạn truyền được nhiều lần. Mục này giải thích vì sao, và khi nào mẹo đó không còn đúng.

Vì `&mut T` không `Copy`, `emit(r)` với `r: &mut Vec<u8>` trông như **move** `r` vào hàm, và dùng `r` lần hai sẽ là E0382. Nhưng đoạn sau chạy được:

```rust
fn push_one(v: &mut Vec<i32>) { v.push(1); }
fn main() {
    let mut v = vec![];
    let r = &mut v;
    push_one(r);
    push_one(r);   // OK
    r.push(2);
    println!("{v:?}");   // in "[1, 1, 2]"
}
```

Lý do: khi kiểu tham số đích *đã biết là* `&mut T`, compiler chèn một **reborrow** `&mut *r` thay vì move `r`. Reborrow là một borrow mới của place `*r`, sống ngắn hơn `r`; hết lời gọi thì `r` dùng lại được. Người soạn không tìm thấy mục nào của Reference mô tả hành vi ngầm này dưới tên "reborrow" (trang coercion chỉ liệt kê `&mut T` → `&T` như một coercion hợp lệ). Vì vậy phần "tự động" ở đây là **quan sát** từ rustc 1.95.0, không phải trích dẫn bảo đảm.

<details><summary>Đi sâu: tham số generic, <code>&amp;mut *r</code> tường minh và E0503</summary>

Khi tham số là kiểu **generic** (`W` suy ra là `&mut Vec<u8>`), compiler không biết đích là `&mut` nên không reborrow, và `r` bị move:

```rust
use std::io::Write;
fn emit<W: Write>(mut w: W) { w.write_all(b"x").unwrap(); }
fn main() {
    let mut buf: Vec<u8> = vec![];
    let r = &mut buf;
    emit(r);
    emit(r);
}
```
Compiler báo `E0382: use of moved value: r` kèm gợi ý `help: consider creating a fresh reborrow of r here: emit(&mut *r)`.

Sửa đúng như compiler gợi ý: `emit(&mut *r); emit(&mut *r); r.push(b'!');` chạy ra `xx!`. Biểu thức `&mut *r` là toán tử borrow áp trên place `*r`[^bor2].

Reborrow vẫn tuân theo luật shared-XOR-mutable với place cha: khi `r2 = &mut *r` còn được dùng ở phía sau thì `*r += 1` cho `E0503: cannot use *r because it was mutably borrowed`; đổi thứ tự (dùng `r2` trước) thì chạy, in `12`.

Two-phase borrow (`v.push(v.len())` biên dịch, cho `[1, 2, 2]`) thuộc [P10·06](../10-rust-internals/06-borrow-checker.md).

</details>

**Quay lại Q2:** `&mut` không copy được nhưng được reborrow ngầm khi đích rõ là `&mut`; với generic thì viết `&mut *r`.

## 5. Lifetime thật sự là gì (Q3)

> **Trả lời ngắn.** Lifetime là *vùng mã* mà một reference phải còn hợp lệ, không phải "thời gian sống của biến"[^lt1]. Vùng này có thể có "lỗ"[^lt2]. Nó kết thúc ở lần dùng cuối của reference[^nll2], còn annotation `'a` chỉ mô tả quan hệ giữa các vùng chứ không kéo dài sự sống của dữ liệu.

**Giải thích.**

*Với service `catalog`.* Handler mượn `&str` là tên sản phẩm nằm trong cache để dựng response. Reference đó phải còn hợp lệ trong bao lâu? Nhiều người trả lời "tới hết scope của biến bị mượn", và câu trả lời đó sinh ra khá nhiều hiểu lầm khi đọc lỗi borrow. Mục này nói lifetime thực sự đo theo cái gì.

### 5.1 Không phải "thời gian sống của biến"

Lifetime là các vùng mã được đặt tên mà một reference phải hợp lệ trong đó[^lt1]. Vùng đó có thể phức tạp vì nó đi theo các đường thực thi của chương trình, thậm chí có "lỗ": bạn được phép làm vô hiệu một reference ở giữa chừng, miễn là gán lại nó trước khi dùng tiếp[^lt2]. Các ví dụ đơn giản thường cho lifetime trùng với scope[^lt3], nên dễ sinh cách hiểu "lifetime = scope" (nhận xét của người soạn).

Hai khái niệm này hay bị trộn, nên cần tách ra:

- **Scope của biến** là khoảng mã biến tồn tại cho tới khi bị drop (cú pháp `{}`, thứ tự khai báo).
- **Lifetime của một borrow** là vùng mã mà reference phải còn hợp lệ. Nó kết thúc ở lần dùng cuối của reference (hoặc của thứ chứa nó).

Scope chỉ là cận trên: vùng borrow không bao giờ vượt scope của dữ liệu, nhưng thường ngắn hơn.

Annotation cũng không đổi gì ở đây. Nó không thay đổi các reference sống bao lâu; nó chỉ mô tả quan hệ giữa lifetime của nhiều reference với nhau[^lt4]. Nói cách khác, `'a` là một *ràng buộc* gửi cho borrow checker, không phải lệnh kéo dài sự sống của dữ liệu.

### 5.2 Ví dụ: cùng một hàm, hai kết quả

Hàm `longest` dưới đây giống một hàm của `catalog` chọn giữa hai tên và trả về tên nào dài hơn. Trong ví dụ, `r` được dùng ngay trong scope của `t`:

```rust
fn longest<'a>(a: &'a str, b: &'a str) -> &'a str { if a.len() > b.len() { a } else { b } }
fn main() {
    let s = String::from("long");
    let r;
    {
        let t = String::from("hi");
        r = longest(&s, &t);
        println!("{r}");          // dùng r trong scope của t: OK, in "long"
    }
}
```

Dời `println!("{r}")` ra *sau* dấu `}`:

```text
error[E0597]: `t` does not live long enough
 --> life_region_err.rs:7:25
6 |         let t = String::from("hi");
  |             - binding `t` declared here
7 |         r = longest(&s, &t);
  |                         ^^ borrowed value does not live long enough
8 |     }
  |     - `t` dropped here while still borrowed
9 |     println!("{r}");
  |                - borrow later used here
```

Chữ ký `longest` nói rằng kết quả sống trong *giao* của hai đầu vào: lifetime của kết quả bằng cái nhỏ hơn trong hai lifetime đầu vào[^lt5]. Compiler không đoán được lúc chạy `a` hay `b` sẽ được trả về, nên nó ràng buộc cả hai.

### 5.3 Compiler thực sự kiểm tra gì

Borrow checker so sánh các scope để xác định mọi borrow có hợp lệ không[^bc1]. Mục đích chính của lifetime là ngăn dangling reference[^bc2]: compiler bảo đảm dữ liệu không ra khỏi scope trước tham chiếu tới nó[^dang1].

Từ các thông điệp lỗi đã chạy, người soạn rút ra một cách phân loại: lỗi borrow nào cũng có ba mốc, gồm nơi mượn, nơi dữ liệu bị drop/ghi/move, và nơi tham chiếu *được dùng sau đó* (cách phân loại của người soạn). Chỉ khi cả ba xảy ra theo đúng thứ tự đó mới có lỗi. Vì thế, nếu `r` không bao giờ được dùng sau điểm B thì vùng borrow kết thúc *trước* B và không có lỗi; đây là NLL ở mục 6.

<details><summary>Đi sâu: thứ tự drop của biến trong cùng scope</summary>

Khi luồng điều khiển rời một drop scope, các biến gắn với scope đó bị drop theo thứ tự ngược với thứ tự khai báo[^dtor2] (đã chạy: khai báo `_a`, `_b` → in `end`, `drop b`, `drop a`).

</details>

**Quay lại Q3:** lifetime là vùng mã theo các lần *dùng* reference, không phải scope của biến; `'a` chỉ là ràng buộc.

## 6. NLL: borrow kết thúc ở lần dùng cuối (Q4)

> **Trả lời ngắn.** Phạm vi của một reference chạy tới lần dùng cuối của nó[^nll1], không phải tới hết scope, nên đọc `r` xong rồi mới `s.push(...)` là hợp lệ. NLL không phải "borrow checker hiểu mọi thứ an toàn": mẫu cache "tìm, không thấy thì chèn rồi trả tham chiếu" vẫn bị từ chối (đã chạy).

**Giải thích.**

*Với service `catalog`.* Handler đọc một giá trị từ cache để log, rồi mới cập nhật cache. Nếu việc đọc xong trước lúc cập nhật thì code có chạy được không? Với NLL (non-lexical lifetimes) thì được, và mục này nói vì sao.

Phạm vi của một reference bắt đầu từ chỗ nó được tạo và kéo dài tới lần cuối nó được dùng[^nll1]. Rustonomicon mô tả cùng ý đó: reference còn sống từ chỗ được tạo tới lần dùng cuối[^nll2]. Hệ quả cụ thể:

```rust
fn main() {
    let mut s = String::from("hi");
    let r = &s;
    println!("{r}");   // lần dùng cuối của r
    s.push('!');       // OK
    println!("{s}");   // in "hi!"
}
```

Chỉ cần đổi chỗ `println!("{r}")` xuống sau `s.push('!')` là lỗi. Thông điệp là `E0502` với ba nhãn: `immutable borrow occurs here` (dòng 3), `mutable borrow occurs here` (dòng 4), `immutable borrow later used here` (dòng 5).

Lifetime còn có thể có chỗ ngắt, như ở ví dụ vòng lặp của Nomicon: có thể coi đó là hai borrow riêng biệt cùng gắn với một biến cục bộ[^nll3]. Cũng vì thế, nhãn `later used here` của compiler là công cụ chẩn đoán tốt nhất: luôn đọc dòng đó trước.

<details><summary>Đi sâu: giới hạn của NLL — cache "tìm, không thấy thì chèn"</summary>

NLL **không** phải "borrow checker hiểu mọi thứ an toàn". Ví dụ phổ biến nhất trong backend là cache "tìm, không thấy thì chèn rồi trả tham chiếu", đúng cái cache của `catalog`:

```rust
fn get_or_load(c: &mut HashMap<u32, String>, k: u32) -> &String {
    if let Some(v) = c.get(&k) { return v; }
    c.insert(k, format!("v{k}"));
    &c[&k]
}
```
```text
error[E0502]: cannot borrow `*c` as mutable because it is also borrowed as immutable
3 |     if let Some(v) = c.get(&k) { return v; }
  |                      -                  - returning this value requires that `*c` is borrowed for `'1`
4 |     c.insert(k, format!("v{k}"));
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ mutable borrow occurs here
```

(Đã chạy trên 1.95.0, edition 2024: bị từ chối.) Lý do: nhánh trả `v` buộc borrow `c.get` sống bằng toàn bộ lifetime `'1` của giá trị trả về, nên borrow đó trùm cả lệnh `insert` ở nhánh còn lại. Cách sửa đúng ngôn ngữ là `c.entry(k).or_insert_with(|| format!("v{k}"))`, chỉ một lần mượn `&mut` (đã chạy, in `v1`). Vì sao compiler (hiện tại) không chấp nhận dạng đầu, xem [P10·06 mục 7](../10-rust-internals/06-borrow-checker.md) (problem case #3; ở đó dùng biến thể `get_mut` nên báo `E0499`, bản `get` ở đây báo `E0502`, cùng một cơ chế).

</details>

**Quay lại Q4:** NLL kết thúc borrow ở lần dùng cuối; đọc dòng "later used here" trước, và nhớ nó vẫn từ chối vài mẫu an toàn như cache get-or-insert.

## 7. Sáu lỗi bạn sẽ gặp hằng ngày (Q5)

> **Trả lời ngắn.** Lỗi borrow nào cũng có ba mốc: nơi mượn, nơi dữ liệu bị move/ghi/drop, và nơi reference được dùng sau đó (cách phân loại của người soạn, rút từ các thông điệp lỗi đã chạy). Hãy đọc nhãn `later used here` trước. Sáu lỗi hay gặp là E0499 (hai `&mut`), E0502 (đọc/ghi chồng), E0505 (move khi đang mượn), E0506 (gán đè khi đang mượn), E0597 (dữ liệu không sống đủ lâu) và E0716 (temporary bị drop sớm); mỗi lỗi có cách sửa ngắn bên dưới.

**Giải thích.**

*Với service `catalog`.* Khi viết handler, bạn sẽ gặp đúng sáu lỗi này hết lần này tới lần khác: hai chỗ cùng cầm `&mut` tới một buffer, giữ tham chiếu vào `Vec` rồi `push`, drop hay gán đè dữ liệu đang được mượn, và trả về tham chiếu tới thứ đã bị giải phóng. Cả sáu có chung một cách đọc theo ba mốc ở trên, nên khi gặp lỗi, hãy tìm ba mốc đó trong thông điệp trước khi sửa. Mỗi mục đi theo thứ tự: chương trình tối thiểu → thông điệp thật → cách đọc → cách sửa.

### 7.1 E0499 — hai `&mut` cùng lúc

```rust
let mut s = String::from("a");
let a = &mut s;
let b = &mut s;
a.push('x'); b.push('y');
```
```text
error[E0499]: cannot borrow `s` as mutable more than once at a time
3 |     let a = &mut s;
  |             ------ first mutable borrow occurs here
4 |     let b = &mut s;
  |             ------ second mutable borrow occurs here
5 |     a.push('x'); b.push('y');
  |     - first borrow later used here
```

Code này vi phạm luật "một `&mut` tại một thời điểm"[^bor1]. `a` còn được dùng ở dòng 5 nên borrow thứ nhất phủ luôn dòng 4, nơi `b` được tạo. Sửa: dùng xong `a` rồi mới tạo `b`.

### 7.2 E0502 — đọc và ghi chồng nhau

Trường hợp kinh điển là giữ `&v[0]` rồi `push`.

```rust
let mut v = vec![1, 2, 3];
let first = &v[0];
v.push(4);
println!("{first}");
```
```text
error[E0502]: cannot borrow `v` as mutable because it is also borrowed as immutable
3 |     let first = &v[0];
  |                  - immutable borrow occurs here
4 |     v.push(4);
  |     ^^^^^^^^^ mutable borrow occurs here
5 |     println!("{first}");
  |                ----- immutable borrow later used here
```

Lý do cụ thể: `push` có thể cấp phát lại buffer, khi đó `first` sẽ trỏ vào vùng đã giải phóng (suy luận của người soạn từ hành vi `Vec`).

Biến thể thường gặp: `for x in &v { if … { v.push(9); } }` cũng `E0502`, vì iterator giữ `&v` suốt vòng lặp (đã chạy).

Sửa: thu thập thay đổi cần làm rồi áp dụng sau vòng lặp; hoặc dùng index (mục 9.3); hoặc `clone` có chủ đích.

### 7.3 E0505 — move khi đang bị mượn

```rust
let s = String::from("a");
let r = &s;
drop(s);
println!("{r}");
```
Thông điệp: `E0505: cannot move out of s because it is borrowed` (đã chạy), nhãn `borrow later used here` trỏ vào `println!`.

Lỗi này khớp luật ở mục 2: chỉ biến không đang bị borrow mới được move ra[^move3]. Sửa: dùng `r` xong rồi mới `drop(s)`, hoặc truyền `s.clone()`.

### 7.4 E0506 — gán đè khi đang bị mượn

```rust
let mut x = 5;
let r = &x;
x = 6;
println!("{r}");
```
Thông điệp: `E0506: cannot assign to x because it is borrowed` (đã chạy).

Với borrow chia sẻ `&`, place không được mutate trong suốt thời gian borrow[^bor2], và gán là một dạng mutation. Cách sửa giống E0502: để `r` hết dùng trước khi gán.

<details><summary>Đi sâu: E0597 và E0716 — dữ liệu không sống đủ lâu, temporary bị drop sớm</summary>

### 7.5 E0597 — dữ liệu "does not live long enough"

```rust
let r;
{
    let x = 5;
    r = &x;
}
println!("{r}");
```
```text
error[E0597]: `x` does not live long enough
4 |         let x = 5;
  |             - binding `x` declared here
5 |         r = &x;
  |             ^^ borrowed value does not live long enough
6 |     }
  |     - `x` dropped here while still borrowed
7 |     println!("{r}");
  |                - borrow later used here
```

Đây là ca Book dùng để mở đầu mục lifetime[^bc2]. Lỗi tương tự xảy ra khi nhét borrow vào struct: `h = Holder { s: &owned };` trong khối trong, rồi dùng `h.s` ngoài khối thì `E0597` (đã chạy). Khi viết hàm trả reference tới biến cục bộ, compiler đưa ra lỗi liên quan: `fn dangle() -> &String` cho `E0106 missing lifetime specifier`; thêm `<'a>` để qua E0106 thì thành `E0515: cannot return reference to local variable 's'` (đã chạy cả hai). Sửa: trả về giá trị sở hữu (`String`), hoặc mở rộng scope của chủ sở hữu.

### 7.6 E0716 — temporary bị drop khi còn đang mượn

```rust
let s: &str = String::from("hello").as_str();
println!("{s}");
```
```text
error[E0716]: temporary value dropped while borrowed
2 |     let s: &str = String::from("hello").as_str();
  |                   ^^^^^^^^^^^^^^^^^^^^^         - temporary value is freed at the end of this statement
  |                   |
  |                   creates a temporary value which is freed while still in use
3 |     println!("{s}");
  |                - borrow later used here
help: consider using a `let` binding to create a longer lived value
```

Nguyên nhân: `String::from("hello")` là temporary. Temporary scope là phạm vi nhỏ nhất bao biểu thức và thuộc một danh sách cố định, trong đó có *một câu lệnh*[^tmp1]; ở đây đó là câu lệnh `let`. `as_str()` mượn temporary đó, nhưng hết câu lệnh thì temporary bị drop và `s` treo. Edition 2024 còn thu hẹp scope temporary ở biểu thức đuôi của khối và ở `if let`[^tmp2]. Nếu bạn nâng cấp từ edition cũ rồi gặp E0716 mới, đây là chỗ nên nhìn.

Có một ngoại lệ: `let s: &str = &String::from("hello");` **biên dịch được** (đã chạy, in `hello`) nhờ lifetime extension của temporary, còn `.as_str()` thì không (suy ra từ hai lần chạy; điều kiện đầy đủ nằm ở mục lifetime extension của Reference, chương Destructors). Cách sửa chắc chắn là gán vào một biến có tên rồi mượn biến đó.

Tương tự, `&42` ra được `&'static i32` nhờ *constant promotion*[^promo1]; ví dụ `let n: &'static i32 = &42;` chạy bình thường.

</details>

**Quay lại Q5:** xác định ba mốc (mượn, bị đụng, dùng sau), rồi thu hẹp borrow, đổi thứ tự, hoặc đổi sang dữ liệu sở hữu.

## 8. Lifetime trong chữ ký: elision, struct, `'static` (Q6)

> **Trả lời ngắn.** Elision là ba luật compiler tự thử để điền lifetime ở chữ ký hàm. Luật 1: mỗi lifetime bị lược ở tham số là một tham số riêng[^el2]. Luật 2: nếu có đúng một lifetime ở tham số thì gán cho đầu ra[^el3]. Luật 3: nếu có `&self`/`&mut self` thì lifetime của self gán cho đầu ra[^el4]. Hết luật mà chưa suy ra được thì là lỗi E0106[^el6]. `T: 'static` là *bound*: kiểu đó không chứa borrow non-static nào[^sta3]. Còn `&'static T` là tham chiếu có thể sống suốt chương trình[^sta1].

**Giải thích.**

*Với service `catalog`.* Khi bạn viết hàm phụ cho handler, ví dụ một parser giữ `&str` của request body, hoặc một closure giao cho worker thread, compiler hỏi về lifetime ở chữ ký. Hầu hết lúc bạn không phải viết gì, vì ba luật elision tự điền; lúc bạn phải viết thì thường vì luật 3 gắn kết quả vào chỗ bạn không muốn. Còn chữ `'static` xuất hiện ngay khi bạn đưa việc cho một thread. Mục này đi qua cả ba chuyện đó.

### 8.1 Lifetime elision — ba luật

Lifetime có thể được lược ở chữ ký hàm theo một số luật[^el0]. Book nhấn mạnh rằng đây không phải luật để bạn tuân theo mà là các trường hợp compiler thử; nếu code khớp thì bạn không cần viết lifetime tường minh[^el1]. Ba luật là:

1. Mỗi lifetime bị lược trong tham số là một tham số lifetime *riêng biệt*[^el2].
2. Nếu có đúng một lifetime trong các tham số (lược hay không), lifetime đó gán cho mọi lifetime đầu ra bị lược[^el3].
3. Nếu receiver là `&Self`/`&mut Self`, lifetime của reference tới `self` gán cho mọi lifetime đầu ra bị lược[^el4].

Các luật áp dụng cho khối `impl` cũng như cho `fn`[^el5]. Nếu hết luật mà vẫn không suy ra được thì việc lược các tham số lifetime đó là lỗi[^el6].

```rust
fn first(s: &str) -> &str { &s[..1] }                       // luật 1+2
struct S { s: String }
impl S { fn get(&self, _other: &str) -> &str { &self.s } }  // luật 3: gắn với self
fn pick(a: &str, b: &str) -> &str { a }                     // không luật nào cứu
```
Dòng cuối cho `E0106: missing lifetime specifier` — "the signature does not say whether it is borrowed from `a` or `b`".

Hai hàm đầu biên dịch và chạy ra `abc q`. Một lưu ý quan trọng khi viết backend: luật 3 "gắn kết quả với `self`" thường *không* phải điều bạn muốn khi hàm trả dữ liệu lấy từ tham số khác; lúc đó phải viết lifetime tường minh.

### 8.2 Lifetime trong struct

Struct chứa reference thì phải khai báo lifetime. Với `ImportantExcerpt` của Book, annotation đó có nghĩa là một instance không thể sống lâu hơn reference nó giữ trong field `part`[^st1]. Với `struct Parser<'a> { input: &'a str }`, hãy phân biệt hai chữ ký:

```rust
impl<'a> Parser<'a> {
    fn first_word(&self) -> &'a str { self.input.split(' ').next().unwrap_or("") }
}
```

Chữ ký trên gắn kết quả với `'a` (dữ liệu gốc), *không* gắn với `&self`. Nếu viết `-> &str` thì luật 3[^el4] gắn kết quả với `&self`, vốn ngắn hơn, nên (suy luận của người soạn) người gọi không giữ được kết quả sau khi `Parser` bị drop. Trong chương trình thử, `Parser` bị drop cuối khối nhưng `w` vẫn dùng được (in `hello`), vì `input` là literal `'static`; nếu `input` trỏ vào một `String` cục bộ thì sẽ có E0597 như ở 7.5.

### 8.3 `'static`: bound khác reference

`'static` có hai cách dùng, giống nhau về chữ nhưng khác nghĩa. Rust By Example nói chúng có liên quan nhưng khác nhau tinh tế, và đó là nguồn nhầm lẫn thường gặp khi học Rust[^sta0]. Hai cách đó là:

- **`&'static T`** là tham chiếu có thể sống suốt chương trình. Book viết rằng `'static` chỉ ra reference bị ảnh hưởng có thể sống suốt thời gian chạy chương trình[^sta1]. Literal chuỗi luôn là `'static`.
- **`T: 'static`** là *bound* trên kiểu. Reference định nghĩa `T: 'a` nghĩa là *mọi tham số lifetime của `T`* outlive `'a`[^sta2]. Vì thế `i32: 'static` thỏa, `&'static str: 'a` thỏa, còn `Vec<&'a ()>: 'static` thì không. Nói theo Rust By Example, khi dùng như trait bound thì nó có nghĩa kiểu đó không chứa tham chiếu non-static nào[^sta3]. Hệ quả là dữ liệu sở hữu luôn qua được bound `'static`, còn *tham chiếu tới* dữ liệu đó nói chung thì không[^sta4].

```rust
fn needs_static<T: 'static>(_t: T) {}
fn main() {
    needs_static(String::from("x"));   // OK: sở hữu hết, không chứa borrow
    needs_static(5u32);                // OK
    needs_static("literal");           // OK
    let local = String::from("y");
    needs_static(&local);              // lỗi
}
```
Kết quả: `E0597: local does not live long enough … argument requires that local is borrowed for 'static`.

<details><summary>Đi sâu: <code>thread::spawn</code> đòi <code>'static</code>, lời khuyên của Book, và <code>Box::leak</code></summary>

Vì thế `thread::spawn` đòi `F: … + Send + 'static`: closure và giá trị trả về phải có lifetime bằng cả thời gian chạy chương trình, vì thread có thể sống lâu hơn nơi tạo ra nó[^sta5]. Với worker thread của `catalog`, cách đáp ứng thông thường là `move` ownership vào closure (đã chạy: thread in `t`), chứ không phải "kéo dài" một biến cục bộ. Chi tiết về `Send` và scoped thread ở [P4·08](../04-concurrency/08-rust-concurrency.md).

Book cảnh báo: khi lỗi gợi ý `'static`, thường là bạn đang tạo dangling reference hoặc lệch lifetime, và giải pháp là sửa các vấn đề đó chứ không phải chỉ định lifetime `'static`[^sta6].

`Box::leak` biến dữ liệu sở hữu thành `&'static mut T` nếu T chỉ chứa tham chiếu static hoặc không có[^sta7]. Nếu bỏ reference đó đi thì bộ nhớ bị rò[^sta8] (đã chạy: `Box::leak(s.into_boxed_str())` ra `&'static str`). Nhận xét của người soạn: chỉ hợp cho cấu hình nạp một lần lúc khởi động.

</details>

**Quay lại Q6:** elision là ba luật điền lifetime ở chữ ký (hết luật thì E0106); `T: 'static` là bound trên kiểu, không phải "sống mãi".

## 9. Mẫu sửa lỗi thường gặp (Q7)

> **Trả lời ngắn.** Có bốn lối thoát thường dùng: `clone` có chủ đích, thu hẹp scope, giữ index/key thay vì reference, và dùng `Option::take`/`mem::replace`/`mem::take` để move ra khỏi sau `&mut` mà vẫn để lại một giá trị hợp lệ thay chỗ[^std2]. Chọn theo việc bạn thật sự cần: một bản sao, một thứ tự thời gian khác, hay quyền sở hữu.

**Giải thích.**

*Với service `catalog`.* Giả sử handler vừa dính một trong các lỗi ở mục 7 và bạn cần sửa. Bốn cách dưới đây đi từ rẻ nhất tới đắt nhất về mặt thiết kế, và cách nào đúng tùy vào bạn thật sự cần gì: một bản sao tên sản phẩm, một thứ tự thời gian khác giữa đọc và ghi cache, hay quyền sở hữu hẳn dữ liệu.

### 9.1 `clone` có chủ đích

`let first = v[0].clone();` sửa được E0507 ở mục 2.3, nhưng trả một lần cấp phát và sao chép. Cách này hợp khi dữ liệu nhỏ hoặc nằm ngoài hot path; còn dùng chỉ để "cho compiler im lặng" thì sai (nhận xét của người soạn). Với dữ liệu lớn dùng chung thì dùng `Rc`/`Arc`, xem [P9·04](04-smart-pointer.md).

### 9.2 Tách scope

Thu hẹp phạm vi dùng reference (bằng khối `{}` hoặc tách hàm) để vùng borrow kết thúc trước lần mutate. Nhờ NLL (mục 6), nhiều trường hợp không cần khối tường minh.

### 9.3 Index thay vì reference

Giữ `usize` hay key thay vì `&T` thì không khóa cả collection:

```rust
let mut v = vec![1, 2, 3];
let i = v.iter().position(|&x| x == 2).unwrap();
v.push(4);        // không có borrow nào đang sống
v[i] += 10;
println!("{v:?}"); // in "[1, 12, 3, 4]"
```

Đánh đổi là index có thể cũ đi khi collection đổi thứ tự hoặc xóa phần tử. Vì vậy với `HashMap` hãy dùng key, còn với dữ liệu hay bị xóa thì dùng ID ổn định.

### 9.4 `Option::take`, `mem::replace`, `mem::take`

Ba hàm này là cách hợp lệ để "move ra khỏi sau `&mut`": chúng để lại một giá trị hợp lệ thay chỗ, nên owner không bao giờ rơi vào trạng thái bị rút ruột như ở mục 2.3.

- `Option::take` lấy giá trị ra khỏi option và để lại `None` tại chỗ[^std1].
- `mem::replace(dest, src)` chuyển `src` vào `dest` được tham chiếu và trả về giá trị cũ của `dest`[^std2].
- `mem::take(dest)` thay `dest` bằng giá trị mặc định của `T` và trả về giá trị cũ của `dest`[^std3].

<details><summary>Đi sâu: ví dụ <code>pop</code> danh sách liên kết và <code>replace</code>/<code>take</code></summary>

```rust
struct Node { val: i32, next: Option<Box<Node>> }
struct List { head: Option<Box<Node>> }
impl List {
    fn pop(&mut self) -> Option<i32> {
        self.head.take().map(|n| { self.head = n.next; n.val })
    }
}
// pop liên tiếp trên danh sách [1, 2] cho Some(1) Some(2) None

let mut name = String::from("abc");
let old   = std::mem::replace(&mut name, String::from("new")); // old = "abc"
let taken = std::mem::take(&mut name);                         // taken = "new", name = ""
```

Nếu viết `let n = self.head; …` thì lại là E0507 ("cannot move out of `self.head` which is behind a mutable reference"); `take()` là lối thoát. Với `Vec<String>`: `mem::take(&mut v[0])` lấy phần tử ra (để lại `""`), còn `v.swap_remove(1)` lấy ra trong O(1) bằng cách đổi chỗ với phần tử cuối (đã chạy: `a b  ["z", "c"]`).

</details>

**Quay lại Q7:** clone khi cần bản sao, tách scope hoặc index khi chỉ là vấn đề thời điểm, `take`/`replace` khi cần lấy quyền sở hữu từ sau `&mut`.

## 10. Liên hệ backend (nhận xét của người soạn)

**Request/response.** Handler của `catalog` nhận `Request` *theo giá trị*, đọc `&req.body` (một borrow ngắn), rồi destructure `let Request { user, .. } = req;` để lấy field sở hữu mà không cần clone (đã chạy: `u:4`). Đó là partial move hợp lệ (mục 2.4), miễn struct không `Drop`. Response nên là kiểu sở hữu (`String`, `Vec<u8>`) để khỏi dính lifetime của request.

**Giữ reference vào cache.** Hàm trả `&V` mượn cả cache, nên suốt thời gian người gọi giữ kết quả, cache không mutate được (mục 6). Có hai hướng. Một là `entry`/`or_insert_with`, chỉ mượn một lần. Hai là cache lưu `Arc<V>` và trả bản clone; hướng này hợp khi giá trị phải sống qua `.await` hay sang thread khác (cần `'static`, mục 8.3), và đổi lại mỗi lần clone tốn một bộ đếm tham chiếu atomic.

## Tóm tắt

- Move xảy ra khi **place expression** ở value context; chỉ biến không bị borrow, temporary, field (không `Drop`) và `*Box` move ra được. Còn lại là E0507.
- Partial move: dùng được field còn lại, không dùng được cả struct; kiểu có `Drop` thì E0509.
- `&T` là `Copy`, `&mut T` thì không; `&mut` tự reborrow khi đích rõ là `&mut`, với tham số generic phải viết `&mut *r`.
- Lifetime là **vùng mã** mà borrow phải hợp lệ, xác định bởi các lần *dùng*; annotation chỉ mô tả quan hệ.
- NLL: borrow kết thúc ở lần dùng cuối; đọc dòng "later used here" trước.
- Elision có ba luật; hết luật mà chưa suy ra → E0106.
- `T: 'static` = kiểu không chứa borrow không-static; `&'static T` = tham chiếu sống suốt chương trình.
- Sửa lỗi: clone có chủ đích, tách scope, index/key, `take`/`replace`/`mem::take`, `entry`.

## Câu hỏi tự kiểm tra

1. Vì sao `let s = v[0];` với `v: Vec<String>` bị E0507 còn `let s = &v[0];` thì không?
<details><summary>Đáp án</summary>

`v[0]` là place sau một tham chiếu/index, không nằm trong danh sách place được move ra[^move7]. `&v[0]` chỉ mượn, không move (mục 2.3).

</details>

2. Sau `let n = u.name;`, dòng nào dùng được: `u.age`, `u.email`, `{:?}` của `u`?
<details><summary>Đáp án</summary>

`u.age` và `u.email` dùng được, còn `{:?}` của cả `u` thì E0382 "partially moved" (mục 2.4). Khi `u` ra scope, chỉ field còn khởi tạo bị drop[^dtor1].

</details>

3. Vì sao `let n = g.name;` lỗi nếu `Guard` có `impl Drop`?
<details><summary>Đáp án</summary>

Điều kiện để move field là kiểu không implement Drop[^move4]; rustc báo E0509 (mục 2.4).

</details>

4. Khi nào `emit(r)` hai lần với `r: &mut T` biên dịch, khi nào không?
<details><summary>Đáp án</summary>

Biên dịch khi tham số có kiểu `&mut T` rõ ràng (reborrow ngầm). Với tham số generic `W` thì `r` bị move và lần hai là E0382, sửa bằng `&mut *r` (mục 4; phần "ngầm" là quan sát, không phải trích dẫn).

</details>

5. "Lifetime của `&x` bằng scope của `x`" sai ở đâu?
<details><summary>Đáp án</summary>

Lifetime là vùng mã mà reference phải hợp lệ[^lt1]. Vùng đó có thể có lỗ[^lt2]. Nó kết thúc ở lần dùng cuối của reference[^nll2]. Scope của `x` chỉ là cận trên mà vùng đó không được vượt.

</details>

6. Viết lại `fn f(a: &str, b: &str) -> &str` cho đúng nếu kết quả luôn lấy từ `a`.
<details><summary>Đáp án</summary>

`fn f<'a>(a: &'a str, b: &str) -> &'a str`. Không luật elision nào cứu được trường hợp hai tham số. Luật 2 cần đúng một lifetime[^el3]. Luật 3 cần `self`[^el4]. Còn hết luật mà chưa suy ra được thì là lỗi[^el6].

</details>

7. `T: 'static` có nghĩa `T` sống mãi không? Cho ví dụ kiểu thỏa và kiểu không thỏa.
<details><summary>Đáp án</summary>

Không: nghĩa là mọi tham số lifetime của `T` outlive `'static`[^sta2]. `i32` và `String` thỏa, còn `Vec<&'a ()>` với `'a` không-static thì không. Tóm lại, bound này đòi kiểu không chứa tham chiếu non-static nào[^sta3]. Dữ liệu sở hữu luôn qua được bound này[^sta4].

</details>

8. Vì sao `String::from("hello").as_str()` gán vào `let` gây E0716?
<details><summary>Đáp án</summary>

Temporary bị drop ở cuối câu lệnh (scope temporary nhỏ nhất bao biểu thức, gồm một câu lệnh[^tmp1]) trong khi `s` còn dùng sau đó. Sửa bằng biến có tên.

</details>

9. Vì sao `get_or_load` dạng `if let Some(v) = c.get(&k) { return v; } c.insert(…)` bị từ chối còn `entry().or_insert_with` thì được?
<details><summary>Đáp án</summary>

Nhánh trả `v` buộc borrow của `c.get` sống bằng lifetime của giá trị trả về, nên borrow đó trùm cả `insert` (thông điệp "returning this value requires that `*c` is borrowed for `'1`"); `entry` chỉ mượn `&mut` một lần (mục 6).

</details>

## Đọc thêm

- Rust Book 1.95.0, ch04-01/02 — <https://doc.rust-lang.org/1.95.0/book/ch04-02-references-and-borrowing.html>: ví dụ gốc của luật borrow và dangling reference.
- Rust Book 1.95.0, ch10-03 — <https://doc.rust-lang.org/1.95.0/book/ch10-03-lifetime-syntax.html>: annotation, elision, struct, `'static`.
- Rust Reference 1.95.0, Expressions (place/value, move) — <https://doc.rust-lang.org/1.95.0/reference/expressions.html>: định nghĩa chính xác của move.
- Rustonomicon 1.95.0, Lifetimes — <https://doc.rust-lang.org/1.95.0/nomicon/lifetimes.html>: lifetime như vùng mã, có "lỗ".
- [P10·06 — Borrow checker](../10-rust-internals/06-borrow-checker.md): NLL, region inference, two-phase borrow ở mức cơ chế.

[^own1]: Rust Book, ch04-01 Ownership Rules — <https://doc.rust-lang.org/1.95.0/book/ch04-01-what-is-ownership.html> — "Each value in Rust has an owner."
[^own2]: Rust Book, ch04-01 Ownership Rules — <https://doc.rust-lang.org/1.95.0/book/ch04-01-what-is-ownership.html> — "There can only be one owner at a time."
[^own3]: Rust Book, ch04-01 Ownership Rules — <https://doc.rust-lang.org/1.95.0/book/ch04-01-what-is-ownership.html> — "When the owner goes out of scope, the value will be dropped."
[^bor1]: Rust Book, ch04-02 References and Borrowing — <https://doc.rust-lang.org/1.95.0/book/ch04-02-references-and-borrowing.html> — "At any given time, you can have either one mutable reference or any number of immutable references."
[^race1]: Rust Book, ch04-02 References and Borrowing — <https://doc.rust-lang.org/1.95.0/book/ch04-02-references-and-borrowing.html> — "Two or more pointers access the same data at the same time."
[^place1]: Rust Reference, Expressions (place and value) — <https://doc.rust-lang.org/1.95.0/reference/expressions.html> — "A place expression is an expression that represents a memory location."
[^place2]: Rust Reference, Expressions (place and value) — <https://doc.rust-lang.org/1.95.0/reference/expressions.html> — "These expressions are paths which refer to local variables, static variables, dereferences (*expr), array indexing expressions (expr[expr]), field references (expr.f) and parenthesized place expressions."
[^move1]: Rust Reference, Expressions (moved and copied types) — <https://doc.rust-lang.org/1.95.0/reference/expressions.html> — "When a place expression is evaluated in a value expression context, or is bound by value in a pattern, it denotes the value held in that memory location."
[^move2]: Rust Reference, Expressions (moved and copied types) — <https://doc.rust-lang.org/1.95.0/reference/expressions.html> — "If the type of that value implements Copy, then the value will be copied."
[^move3]: Rust Reference, Expressions (moved and copied types) — <https://doc.rust-lang.org/1.95.0/reference/expressions.html> — "Variables which are not currently borrowed."
[^move4]: Rust Reference, Expressions (moved and copied types) — <https://doc.rust-lang.org/1.95.0/reference/expressions.html> — "Fields of a place expression which can be moved out of and don't implement Drop."
[^move5]: Rust Reference, Expressions (moved and copied types) — <https://doc.rust-lang.org/1.95.0/reference/expressions.html> — "The result of dereferencing an expression with type Box\<T> and that can also be moved out of."
[^move6]: Rust Reference, Expressions (moved and copied types) — <https://doc.rust-lang.org/1.95.0/reference/expressions.html> — "After moving out of a place expression that evaluates to a local variable, the location is deinitialized and cannot be read from again until it is reinitialized."
[^dtor1]: Rust Reference, Destructors — <https://doc.rust-lang.org/1.95.0/reference/destructors.html> — "If a variable has been partially initialized, only its initialized fields are dropped."
[^dtor2]: Rust Reference, Destructors (drop scopes) — <https://doc.rust-lang.org/1.95.0/reference/destructors.html> — "When control flow leaves a drop scope all variables associated to that scope are dropped in reverse order of declaration (for variables) or creation (for temporaries)."
[^bor2]: Rust Reference, Operator expressions (borrow operators) — <https://doc.rust-lang.org/1.95.0/reference/expressions/operator-expr.html> — "The memory location is also placed into a borrowed state for the duration of the reference. For a shared borrow (&), this implies that the place may not be mutated, but it may be read or shared again."
[^bor3]: Rust Reference, Operator expressions (borrow operators) — <https://doc.rust-lang.org/1.95.0/reference/expressions/operator-expr.html> — "For a mutable borrow (&mut), the place may not be accessed in any way until the borrow expires."
[^move7]: Rust Reference, Expressions (moved and copied types) — <https://doc.rust-lang.org/1.95.0/reference/expressions.html> — "Only the following place expressions may be moved out of:"
[^alias1]: Rustonomicon, Aliasing — <https://doc.rust-lang.org/1.95.0/nomicon/aliasing.html> — "variables and pointers alias if they refer to overlapping regions of memory."
[^alias2]: Rustonomicon, Aliasing — <https://doc.rust-lang.org/1.95.0/nomicon/aliasing.html> — "In Rust we know this input should be impossible because &mut isn't allowed to be aliased. So we can safely reject its possibility and perform this optimization."
[^copy1]: std docs, Trait std::marker::Copy — <https://doc.rust-lang.org/1.95.0/std/marker/trait.Copy.html> — "Shared references can be copied, but mutable references cannot!"
[^copy2]: std docs, Trait std::marker::Copy — <https://doc.rust-lang.org/1.95.0/std/marker/trait.Copy.html> — "copying &mut T would create an aliased mutable reference."
[^lt1]: Rustonomicon, Lifetimes — <https://doc.rust-lang.org/1.95.0/nomicon/lifetimes.html> — "Lifetimes are named regions of code that a reference must be valid for."
[^lt2]: Rustonomicon, Lifetimes — <https://doc.rust-lang.org/1.95.0/nomicon/lifetimes.html> — "There may even be holes in these paths of execution, as it's possible to invalidate a reference as long as it's reinitialized before it's used again."
[^lt3]: Rustonomicon, Lifetimes — <https://doc.rust-lang.org/1.95.0/nomicon/lifetimes.html> — "In most of our examples, the lifetimes will coincide with scopes."
[^lt4]: Rust Book, ch10-03 Validating References with Lifetimes — <https://doc.rust-lang.org/1.95.0/book/ch10-03-lifetime-syntax.html> — "they describe the relationships of the lifetimes of multiple references to each other without affecting the lifetimes."
[^lt5]: Rust Book, ch10-03 Validating References with Lifetimes — <https://doc.rust-lang.org/1.95.0/book/ch10-03-lifetime-syntax.html> — "the lifetime of the reference returned by the longest function is the same as the smaller of the lifetimes of the references passed in."
[^bc1]: Rust Book, ch10-03 Validating References with Lifetimes — <https://doc.rust-lang.org/1.95.0/book/ch10-03-lifetime-syntax.html> — "The Rust compiler has a borrow checker that compares scopes to determine whether all borrows are valid."
[^bc2]: Rust Book, ch10-03 Validating References with Lifetimes — <https://doc.rust-lang.org/1.95.0/book/ch10-03-lifetime-syntax.html> — "The main aim of lifetimes is to prevent dangling references"
[^dang1]: Rust Book, ch04-02 References and Borrowing — <https://doc.rust-lang.org/1.95.0/book/ch04-02-references-and-borrowing.html> — "the compiler will ensure that the data will not go out of scope before the reference to the data does."
[^nll1]: Rust Book, ch04-02 References and Borrowing — <https://doc.rust-lang.org/1.95.0/book/ch04-02-references-and-borrowing.html> — "continues through the last time that reference is used."
[^nll2]: Rustonomicon, Lifetimes — <https://doc.rust-lang.org/1.95.0/nomicon/lifetimes.html> — "A reference (sometimes called a borrow) is alive from the place it is created to its last use."
[^nll3]: Rustonomicon, Lifetimes — <https://doc.rust-lang.org/1.95.0/nomicon/lifetimes.html> — "And a lifetime can have a pause in it. Or you might look at it as two distinct borrows just being tied to the same local variable."
[^tmp1]: Rust Reference, Destructors (temporary scopes) — <https://doc.rust-lang.org/1.95.0/reference/destructors.html> — "Apart from lifetime extension, the temporary scope of an expression is the smallest scope that contains the expression and is one of the following:"
[^tmp2]: Rust Reference, Destructors (temporary scopes) — <https://doc.rust-lang.org/1.95.0/reference/destructors.html> — "The 2024 edition added two new temporary scope narrowing rules: if let temporaries are dropped before the else block, and temporaries of tail expressions of blocks are dropped immediately after the tail expression is evaluated."
[^promo1]: Rust Reference, Destructors (constant promotion) — <https://doc.rust-lang.org/1.95.0/reference/destructors.html> — "Promotion of a value expression to a 'static slot occurs when the expression could be written in a constant and borrowed"
[^el0]: Rust Reference, Lifetime elision — <https://doc.rust-lang.org/1.95.0/reference/lifetime-elision.html> — "Rust has rules that allow lifetimes to be elided in various places where the compiler can infer a sensible default choice."
[^el1]: Rust Book, ch10-03 Validating References with Lifetimes — <https://doc.rust-lang.org/1.95.0/book/ch10-03-lifetime-syntax.html> — "These aren't rules for programmers to follow; they're a set of particular cases that the compiler will consider, and if your code fits these cases, you don't need to write the lifetimes explicitly."
[^el2]: Rust Reference, Lifetime elision — <https://doc.rust-lang.org/1.95.0/reference/lifetime-elision.html> — "Each elided lifetime in the parameters becomes a distinct lifetime parameter."
[^el3]: Rust Reference, Lifetime elision — <https://doc.rust-lang.org/1.95.0/reference/lifetime-elision.html> — "If there is exactly one lifetime used in the parameters (elided or not), that lifetime is assigned to all elided output lifetimes."
[^el4]: Rust Reference, Lifetime elision — <https://doc.rust-lang.org/1.95.0/reference/lifetime-elision.html> — "If the receiver has type &Self or &mut Self, then the lifetime of that reference to Self is assigned to all elided output lifetime parameters."
[^el5]: Rust Book, ch10-03 Validating References with Lifetimes — <https://doc.rust-lang.org/1.95.0/book/ch10-03-lifetime-syntax.html> — "These rules apply to fn definitions as well as impl blocks."
[^el6]: Rust Reference, Lifetime elision — <https://doc.rust-lang.org/1.95.0/reference/lifetime-elision.html> — "It is an error to elide lifetime parameters that cannot be inferred."
[^st1]: Rust Book, ch10-03 Validating References with Lifetimes — <https://doc.rust-lang.org/1.95.0/book/ch10-03-lifetime-syntax.html> — "This annotation means an instance of ImportantExcerpt can't outlive the reference it holds in its part field."
[^sta1]: Rust Book, ch10-03 The Static Lifetime — <https://doc.rust-lang.org/1.95.0/book/ch10-03-lifetime-syntax.html> — "One special lifetime we need to discuss is 'static, which denotes that the affected reference can live for the entire duration of the program."
[^sta2]: Rust Reference, Trait and lifetime bounds — <https://doc.rust-lang.org/1.95.0/reference/trait-bounds.html> — "T: 'a means that all lifetime parameters of T outlive 'a."
[^sta3]: Rust By Example, Static (trait bound) — <https://doc.rust-lang.org/1.95.0/rust-by-example/scope/lifetime/static_lifetime.html> — "As a trait bound, it means the type does not contain any non-static references."
[^sta4]: Rust By Example, Static (trait bound) — <https://doc.rust-lang.org/1.95.0/rust-by-example/scope/lifetime/static_lifetime.html> — "any owned data always passes a 'static lifetime bound, but a reference to that owned data generally does not"
[^sta5]: std docs, Function std::thread::spawn — <https://doc.rust-lang.org/1.95.0/std/thread/fn.spawn.html> — "The 'static constraint means that the closure and its return value must have a lifetime of the whole program execution. The reason for this is that threads can outlive the lifetime they have been created in."
[^sta6]: Rust Book, ch10-03 The Static Lifetime — <https://doc.rust-lang.org/1.95.0/book/ch10-03-lifetime-syntax.html> — "In such cases, the solution is to fix those problems, not to specify the 'static lifetime."
[^sta7]: std docs, Box::leak — <https://doc.rust-lang.org/1.95.0/std/boxed/struct.Box.html> — "Note that the type T must outlive the chosen lifetime 'a. If the type has only static references, or none at all, then this may be chosen to be 'static."
[^sta8]: std docs, Box::leak — <https://doc.rust-lang.org/1.95.0/std/boxed/struct.Box.html> — "Dropping the returned reference will cause a memory leak."
[^std1]: std docs, Option::take — <https://doc.rust-lang.org/1.95.0/std/option/enum.Option.html> — "Takes the value out of the option, leaving a None in its place."
[^std2]: std docs, Function std::mem::replace — <https://doc.rust-lang.org/1.95.0/std/mem/fn.replace.html> — "Moves src into the referenced dest, returning the previous dest value."
[^std3]: std docs, Function std::mem::take — <https://doc.rust-lang.org/1.95.0/std/mem/fn.take.html> — "Replaces dest with the default value of T, returning the previous dest value."
[^sta0]: Rust By Example, Static — <https://doc.rust-lang.org/1.95.0/rust-by-example/scope/lifetime/static_lifetime.html> — "Both are related but subtly different and this is a common source for confusion when learning Rust."
