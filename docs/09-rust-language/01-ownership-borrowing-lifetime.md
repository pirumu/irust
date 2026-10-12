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

Đã đọc Rust Book tới chương generics/traits/lifetimes. Bộ nhớ (stack/heap, drop) xem [P3·08 — Rust và bộ nhớ](../03-memory/08-rust-va-bo-nho.md); `Send`/`'static` trong thread xem [P4·08 — Rust concurrency](../04-concurrency/08-rust-concurrency.md). Cơ chế bên trong borrow checker thuộc [P10·06 — Borrow checker](../10-rust-internals/06-borrow-checker.md); chương này dừng ở mức người dùng.

## 1. Nhắc nhanh: ba luật ownership và hai luật borrow

Book ch04 nêu ba luật: mỗi giá trị có một owner[^own1], tại một thời điểm chỉ có một owner[^own2], và khi owner ra khỏi scope thì giá trị bị drop[^own3]. Về tham chiếu: tại một thời điểm có thể có *hoặc* một tham chiếu mutable *hoặc* nhiều tham chiếu immutable[^bor1].

Hai câu này là tất cả "luật". Phần còn lại của chương là hệ quả: *"owner" là gì khi giá trị nằm trong field, trong `Vec`, sau một con trỏ?* — đây là câu hỏi về **place**; và *"tham chiếu còn hợp lệ" trong bao lâu?* — đây là câu hỏi về **lifetime**.

## 2. Place expression và "move ra khỏi place" (Q1)

> **Trả lời ngắn.** Move chỉ hợp lệ với một số place nhất định: biến không đang bị borrow, temporary, field của place movable mà kiểu không implement `Drop`, và kết quả dereference một `Box`[^move3][^move4][^move5]. `v[0]` (index vào `Vec`) hay `u.name` sau một tham chiếu không nằm trong danh sách đó (suy luận của người soạn) nên bị E0507; còn `u.name` trên struct *sở hữu* (không `Drop`) nằm trong danh sách nên được.

**Giải thích.**

Hình dung (nhận xét của người soạn): cuốn sách trên kệ nhà bạn thì mang đi đâu cũng được (move); cuốn trên kệ thư viện công cộng, hay đang cho người khác mượn, thì bạn chỉ được đọc tại chỗ hoặc photo (clone), không được khiêng đi.

### 2.1 Hai loại biểu thức

Reference chia biểu thức thành place expression — biểu thức đại diện cho một vị trí trong bộ nhớ[^place1] — và value expression. Place expression gồm: biến cục bộ, static, dereference `*expr`, index `expr[expr]`, truy cập field `expr.f`[^place2].

Điểm mấu chốt nằm ở **ngữ cảnh**: khi một place expression xuất hiện ở *value context* (vế phải của `let`, đối số hàm…) hoặc bị bind *by value* trong pattern, nó biểu thị giá trị đang nằm ở vị trí bộ nhớ đó[^move1]. Nếu kiểu là `Copy` thì giá trị được sao chép[^move2]; nếu không, đó là một **move** — và move chỉ hợp lệ với một số place nhất định.

### 2.2 Place nào được move ra

Reference liệt kê đủ, mở đầu bằng "chỉ những place expression sau mới được move ra"[^move7] — gồm biến không đang bị borrow[^move3], temporary, field của một place có thể move ra *và không implement `Drop`*[^move4], và kết quả dereference một `Box<T>` (nếu cũng move ra được)[^move5]. Không có dòng nào cho "bên trong `Vec`" hay "sau `&`/`&mut`" — đó chính là E0507.

Sau khi move ra khỏi biến cục bộ, vị trí đó bị deinitialize và không đọc được cho đến khi gán lại[^move6]. Ví dụ:

```rust
fn main() {
    let a = String::from("a");
    let b = a;           // `a` là place expression ở value context: move
    println!("{a}");
}
```
Compiler báo `E0382 borrow of moved value: a` (đã chạy).

Cùng cơ chế, `for _ in 0..2 { consume(s); }` cho `E0382` "value moved here, in previous iteration of loop".

### 2.3 E0507: move ra khỏi sau tham chiếu hoặc khỏi `Vec`

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

Lý do (suy luận của người soạn từ danh sách ở 2.2): `v[0]` và `u.name` sau `&` là place sau tham chiếu, không nằm trong danh sách được move; với `&mut` cũng vậy (`E0507 … behind a mutable reference`, đã chạy) — mượn chỉ cho đọc/ghi tại chỗ, không cho "lấy mất".

Cách thoát (mục 9 trình bày kỹ): mượn `&v[0]`, `clone()`, hoặc **thay thế** thứ bị lấy bằng một giá trị khác (`mem::take`, `mem::replace`, `Option::take`, `Vec::swap_remove`).

### 2.4 Partial move

Với *owner* (không phải tham chiếu), field được phép move ra riêng lẻ — đây là dòng "field của một place movable và không implement Drop"[^move4]:

```rust
struct User { name: String, email: String, age: u32 }
fn main() {
    let u = User { name: "an".into(), email: "a@x".into(), age: 30 };
    let n = u.name;                       // partial move
    println!("{} {}", u.email, u.age);    // OK: in "a@x 30"
    println!("{n}");                      // in "an"
}
```

Nhưng dùng *cả* `u` sau đó là lỗi:

Dùng cả `u` (`{:?}`) sau đó cho `E0382: borrow of partially moved value: u` (đã chạy).

<details><summary>Đi sâu: kiểu có <code>Drop</code> (E0509) và partial move qua pattern</summary>

Khi `u` ra khỏi scope, chỉ các field còn khởi tạo bị drop[^dtor1]. Điều này **không** áp dụng nếu kiểu implement `Drop`: `Guard { name }` có `impl Drop` thì `let n = g.name;` cho `E0509: cannot move out of type Guard, which implements the Drop trait` (đã chạy) — khớp điều kiện kiểu không được implement `Drop` ở [^move4]: `drop` cần toàn bộ `self` còn nguyên.

Một nguồn partial move hay gặp nhưng ít để ý là **pattern**: `if let Some(s) = o2 { … }` với `o2: Option<String>` bind `s` *by value* nên move `String` ra khỏi `o2` (Reference xếp việc bind by value trong pattern cùng loại với value context[^move1]); dùng `o2` tiếp cho `E0382 … partially moved`. Viết `if let Some(s) = &o2` thì `s: &String` và `o2` còn nguyên (đã chạy cả hai).

</details>

**Quay lại Q1:** move chỉ ra khỏi những place nằm trong danh sách của Reference; place sau tham chiếu hay bên trong `Vec` thì không, nên phải mượn, `clone` hoặc thay thế (mục 9).

## 3. Shared XOR mutable, aliasing

Book giải thích động cơ: hai hay nhiều con trỏ cùng truy cập một dữ liệu cùng lúc là một trong ba điều kiện của data race[^race1]. Reference mô tả cơ chế ở mức biểu thức: toán tử borrow đặt vị trí vào trạng thái bị mượn suốt thời gian tồn tại của reference[^bor2]; với `&` thì place không được ghi nhưng được đọc/chia sẻ tiếp, với `&mut` thì không được truy cập bằng bất cứ cách nào cho tới khi borrow hết hạn[^bor3].

Nomicon nhìn từ góc tối ưu hóa: biến/con trỏ **alias** nếu chúng trỏ tới các vùng bộ nhớ chồng lên nhau[^alias1]; vì `&mut` không được alias, compiler loại trừ được `compute(&x, &mut x)` và tối ưu theo giả định đó[^alias2].

- `&T` là `Copy`, `&mut T` thì không — tài liệu `Copy` nói rõ tham chiếu chia sẻ sao chép được còn tham chiếu mutable thì không[^copy1], với lý do: sao chép `&mut T` sẽ tạo mutable reference bị alias[^copy2].
- Quy tắc áp dụng theo **place**: hai field rời nhau mượn `&mut` đồng thời được, `split_at_mut` làm tương tự cho slice (đã chạy).

## 4. Reborrow và `&mut` tự động reborrow (Q2)

> **Trả lời ngắn.** `&mut T` không `Copy`[^copy1], nhưng khi kiểu tham số đích *đã biết là* `&mut T`, compiler chèn một **reborrow** `&mut *r`: một borrow mới, ngắn hơn, của place `*r`, nên xong hàm là `r` dùng lại được. Phần "tự động" là quan sát từ rustc 1.95.0, không phải lời hứa trích từ docs; với tham số generic thì `r` bị move.

**Giải thích.**

Hình dung (nhận xét của người soạn): bạn giữ chiếc chìa khóa duy nhất (`&mut`). Đưa hẳn chìa cho thợ (move) thì bạn mất chìa; cho thợ mượn *một bản tạm, xong việc là trả* (reborrow) thì xong việc bạn lại dùng chìa gốc.

Vì `&mut T` không `Copy`, `emit(r)` với `r: &mut Vec<u8>` trông như **move** `r` vào hàm — sau đó dùng `r` lần hai sẽ là E0382. Nhưng thực tế đoạn sau chạy được:

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

Lý do: khi kiểu tham số đích *đã biết là* `&mut T`, compiler chèn một **reborrow** `&mut *r` thay vì move `r`. Reborrow là một borrow mới của place `*r`, sống ngắn hơn `r`; xong là `r` dùng lại được. Tôi không tìm thấy một mục Reference mô tả tên "reborrow" cho hành vi ngầm này (trang coercion chỉ liệt kê `&mut T` → `&T` như một coercion hợp lệ) — phần "tự động" dưới đây là **quan sát** từ rustc 1.95.0, không phải trích dẫn bảo đảm.

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

Sửa đúng như compiler gợi ý: `emit(&mut *r); emit(&mut *r); r.push(b'!');` — chạy ra `xx!`. Biểu thức `&mut *r` là borrow-operator áp trên place `*r`[^bor2].

Reborrow tuân theo shared-XOR-mutable với place cha: khi `r2 = &mut *r` còn được dùng sau đó thì `*r += 1` cho `E0503: cannot use *r because it was mutably borrowed`; đổi thứ tự (dùng `r2` trước) thì chạy, in `12`.

Two-phase borrow (`v.push(v.len())` biên dịch, cho `[1, 2, 2]`) thuộc [P10·06](../10-rust-internals/06-borrow-checker.md).

</details>

**Quay lại Q2:** `&mut` không copy được nhưng được reborrow ngầm khi đích rõ là `&mut`; với generic thì viết `&mut *r`.

## 5. Lifetime thật sự là gì (Q3)

> **Trả lời ngắn.** Lifetime là *vùng mã* mà một reference phải còn hợp lệ, không phải "thời gian sống của biến"[^lt1]. Vùng này có thể có "lỗ" và kết thúc ở lần dùng cuối của reference[^lt2][^nll2]; annotation `'a` chỉ mô tả quan hệ giữa các vùng chứ không kéo dài sự sống của dữ liệu[^lt4].

**Giải thích.**

Hình dung (nhận xét của người soạn): hạn của phiếu mượn sách là lần cuối bạn còn *cần đọc*, không phải lúc bạn rời thư viện; scope của biến giống giờ đóng cửa thư viện — chỉ là cận trên mà phiếu không được vượt.

### 5.1 Không phải "thời gian sống của biến"

Nomicon định nghĩa lifetime là các vùng mã được đặt tên mà một reference phải hợp lệ trong đó[^lt1]. Vùng đó có thể phức tạp vì tương ứng với đường thực thi, thậm chí có "lỗ" — có thể làm vô hiệu một reference miễn là gán lại trước khi dùng[^lt2]. Trong ví dụ đơn giản lifetime trùng scope chỉ vì ví dụ đơn giản[^lt3].

Hai khái niệm hay bị trộn:

Scope của biến là khoảng mã biến tồn tại tới khi bị drop (cú pháp `{}`, thứ tự khai báo); lifetime của borrow là vùng mã reference phải còn hợp lệ, kết thúc ở lần dùng cuối của reference (hoặc của thứ chứa nó).

Book diễn đạt ý "annotation không đổi lifetime": annotation không thay đổi các reference sống bao lâu, chỉ mô tả quan hệ giữa lifetime của nhiều reference với nhau[^lt4]. Tức `'a` là *ràng buộc* gửi cho borrow checker chứ không phải lệnh kéo dài sự sống của dữ liệu.

### 5.2 Ví dụ: cùng một hàm, hai kết quả

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

Chữ ký `longest` nói kết quả sống trong *giao* của hai đầu vào; Book: lifetime kết quả bằng cái nhỏ hơn trong hai lifetime đầu vào[^lt5]. Compiler không đoán được `a` hay `b` sẽ được trả về, nên bắt cả hai.

### 5.3 Compiler thực sự kiểm tra gì

Book mô tả borrow checker so sánh các scope để xác định mọi borrow có hợp lệ không[^bc1] — và mục đích chính của lifetime là ngăn dangling reference[^bc2]: compiler bảo đảm dữ liệu không ra khỏi scope trước tham chiếu tới nó[^dang1]. Mọi lỗi borrow có ba mốc (cách phân loại của người soạn, rút từ các thông điệp lỗi đã chạy): nơi mượn, nơi dữ liệu bị drop/ghi/move, và nơi tham chiếu *được dùng sau đó*; lỗi chỉ phát sinh khi cả ba xảy ra theo thứ tự đó.

Nếu `r` không bao giờ được dùng sau điểm B thì vùng borrow kết thúc *trước* B và không có lỗi — NLL (mục 6).

<details><summary>Đi sâu: thứ tự drop của biến trong cùng scope</summary>

Theo Reference, khi luồng điều khiển rời một drop scope thì các biến gắn với scope bị drop theo thứ tự ngược với thứ tự khai báo[^dtor2] (đã chạy: khai báo `_a`, `_b` → in `end`, `drop b`, `drop a`).

</details>

**Quay lại Q3:** lifetime là vùng mã theo các lần *dùng* reference, không phải scope của biến; `'a` chỉ là ràng buộc.

## 6. NLL: borrow kết thúc ở lần dùng cuối (Q4)

> **Trả lời ngắn.** Phạm vi của một reference chạy tới lần dùng cuối của nó[^nll1][^nll2], không phải tới hết scope, nên đọc `r` xong rồi mới `s.push(...)` là hợp lệ. NLL không phải "borrow checker hiểu mọi thứ an toàn": mẫu cache "tìm, không thấy thì chèn rồi trả tham chiếu" vẫn bị từ chối (đã chạy).

**Giải thích.**

Hình dung (nhận xét của người soạn): phòng họp chỉ bị giữ đến cuộc họp cuối của bạn, không phải đến hết ngày làm việc; ai cần phòng sau đó vào được.

Book ch04: phạm vi của một reference bắt đầu từ chỗ nó được tạo và kéo dài tới lần cuối reference đó được dùng[^nll1]; Nomicon: reference còn sống từ chỗ được tạo tới lần dùng cuối[^nll2]. Hệ quả cụ thể:

```rust
fn main() {
    let mut s = String::from("hi");
    let r = &s;
    println!("{r}");   // lần dùng cuối của r
    s.push('!');       // OK
    println!("{s}");   // in "hi!"
}
```

Chỉ cần đổi chỗ `println!("{r}")` xuống sau `s.push('!')`:

Thông điệp là `E0502` với ba nhãn: `immutable borrow occurs here` (dòng 3), `mutable borrow occurs here` (dòng 4), `immutable borrow later used here` (dòng 5).

Nomicon còn lưu ý lifetime có thể có chỗ ngắt quanh vòng lặp: hai borrow riêng biệt gắn với cùng một biến cục bộ[^nll3]. Lỗi `later used here` của compiler chính là công cụ chẩn đoán: luôn đọc dòng đó trước.

<details><summary>Đi sâu: giới hạn của NLL — cache "tìm, không thấy thì chèn"</summary>

NLL **không** phải "borrow checker hiểu mọi thứ an toàn". Ví dụ phổ biến nhất trong backend — cache "tìm, không thấy thì chèn rồi trả tham chiếu":

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

(Đã chạy trên 1.95.0, edition 2024: bị từ chối.) Nhánh trả `v` buộc borrow `c.get` sống bằng toàn bộ lifetime `'1` của giá trị trả về, nên nó trùm cả lệnh `insert` ở nhánh còn lại. Cách sửa đúng ngôn ngữ là `c.entry(k).or_insert_with(|| format!("v{k}"))` — một lần mượn `&mut`, đã chạy, in `v1`. Vì sao compiler (hiện tại) không chấp nhận dạng đầu — xem [P10·06 mục 7](../10-rust-internals/06-borrow-checker.md) (problem case #3; ở đó dùng biến thể `get_mut` nên báo `E0499`, bản `get` ở đây báo `E0502`, cùng một cơ chế).

</details>

**Quay lại Q4:** NLL kết thúc borrow ở lần dùng cuối; đọc dòng "later used here" trước, và nhớ nó vẫn từ chối vài mẫu an toàn như cache get-or-insert.

## 7. Sáu lỗi bạn sẽ gặp hằng ngày (Q5)

> **Trả lời ngắn.** Theo cách phân loại của người soạn (rút từ các thông điệp lỗi đã chạy), mọi lỗi borrow có ba mốc: nơi mượn, nơi dữ liệu bị move/ghi/drop, và nơi reference được dùng sau đó; hãy đọc nhãn `later used here` trước. Sáu lỗi hay gặp là E0499 (hai `&mut`), E0502 (đọc/ghi chồng), E0505 (move khi đang mượn), E0506 (gán đè khi đang mượn), E0597 (dữ liệu không sống đủ lâu) và E0716 (temporary bị drop sớm); mỗi lỗi có cách sửa ngắn bên dưới.

**Giải thích.**

Hình dung (nhận xét của người soạn): như biên bản va chạm xe — ai đỗ ở đâu, ai tông vào lúc nào, và ai quay lại dùng chỗ đó sau va chạm; thiếu một trong ba thì không có tai nạn.

Mỗi mục: chương trình tối thiểu → thông điệp thật → cách đọc → cách sửa.

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
  |             ^^^^^^ second mutable borrow occurs here
5 |     a.push('x'); b.push('y');
  |     - first borrow later used here
```

Vi phạm "một `&mut` tại một thời điểm"[^bor1]; `a` còn dùng ở dòng 5 nên borrow thứ nhất phủ dòng 4. Sửa: dùng xong `a` rồi mới tạo `b`.

### 7.2 E0502 — đọc và ghi chồng nhau

Trường hợp kinh điển: giữ `&v[0]` rồi `push`.

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

Lý do cụ thể: `push` có thể cấp phát lại buffer, `first` sẽ trỏ vào vùng đã giải phóng (suy luận của người soạn từ hành vi `Vec`).

Biến thể: `for x in &v { if … { v.push(9); } }` cũng `E0502` vì iterator giữ `&v` suốt vòng lặp (đã chạy).

Sửa: thu thập thay đổi cần làm rồi áp dụng sau vòng lặp; hoặc dùng index (mục 9.3); hoặc `clone` có chủ đích.

### 7.3 E0505 — move khi đang bị mượn

```rust
let s = String::from("a");
let r = &s;
drop(s);
println!("{r}");
```
Thông điệp: `E0505: cannot move out of s because it is borrowed` (đã chạy), nhãn `borrow later used here` trỏ vào `println!`.

Khớp luật: chỉ biến không đang bị borrow mới được move ra[^move3]. Sửa: dùng `r` xong rồi mới `drop(s)`, hoặc truyền `s.clone()`.

### 7.4 E0506 — gán đè khi đang bị mượn

```rust
let mut x = 5;
let r = &x;
x = 6;
println!("{r}");
```
Thông điệp: `E0506: cannot assign to x because it is borrowed` (đã chạy).

Theo Reference, với borrow chia sẻ `&` thì place không được mutate trong thời gian borrow[^bor2]; gán là một dạng mutation. Sửa như E0502: để `r` hết dùng trước khi gán.

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

Đây là ca Book dùng để mở đầu mục lifetime[^bc2]. Cùng lỗi khi nhét borrow vào struct: `h = Holder { s: &owned };` trong khối trong, dùng `h.s` ngoài khối → `E0597` (đã chạy). Khi viết hàm trả reference tới biến cục bộ, compiler đưa ra lỗi liên quan: `fn dangle() -> &String` → `E0106 missing lifetime specifier`; thêm `<'a>` cho qua E0106 thì thành `E0515: cannot return reference to local variable 's'` (đã chạy cả hai). Sửa: trả về giá trị sở hữu (`String`), hoặc mở rộng scope của chủ sở hữu.

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

Nguyên nhân: `String::from("hello")` là temporary; Reference cho biết temporary scope là phạm vi nhỏ nhất bao biểu thức và thuộc một danh sách, trong đó có *một câu lệnh*[^tmp1] — ở đây là câu lệnh `let`. `as_str()` mượn temporary đó, kết thúc câu lệnh nó bị drop, `s` treo. (Edition 2024 còn thu hẹp scope temporary ở biểu thức đuôi của khối và ở `if let`[^tmp2] — nếu bạn nâng cấp từ edition cũ và gặp E0716 mới, đây là nơi nên nhìn.)

Ngoại lệ: `let s: &str = &String::from("hello");` **biên dịch được** (đã chạy, in `hello`) nhờ lifetime extension của temporary, còn `.as_str()` thì không (suy ra từ hai lần chạy; điều kiện đầy đủ nằm ở mục lifetime extension của Reference, chương Destructors). Sửa chắc chắn: gán vào biến có tên rồi mượn biến đó.

Tương tự `&42` ra `&'static i32` nhờ *constant promotion*[^promo1] — ví dụ `let n: &'static i32 = &42;` chạy bình thường.

</details>

**Quay lại Q5:** xác định ba mốc (mượn, bị đụng, dùng sau), rồi thu hẹp borrow, đổi thứ tự, hoặc đổi sang dữ liệu sở hữu.

## 8. Lifetime trong chữ ký: elision, struct, `'static` (Q6)

> **Trả lời ngắn.** Elision là ba luật compiler tự thử để điền lifetime ở chữ ký hàm: mỗi lifetime bị lược ở tham số là một tham số riêng; nếu có đúng một lifetime ở tham số thì gán cho đầu ra; nếu có `&self`/`&mut self` thì lifetime của self gán cho đầu ra[^el2][^el3][^el4]. Hết luật mà chưa suy ra được thì là lỗi E0106[^el6]. `T: 'static` là *bound* (kiểu không chứa borrow non-static)[^sta2][^sta3], khác với `&'static T` (tham chiếu có thể sống suốt chương trình)[^sta1].

**Giải thích.**

Hình dung (nhận xét của người soạn): như mẫu đơn có ô điền sẵn — nếu chỉ có một người ký ở các trang trước thì mọi chỗ trống mặc định là của người đó; nếu có hai người ký mà không ghi rõ, thủ tục dừng lại hỏi bạn (E0106).

### 8.1 Lifetime elision — ba luật

Reference: lifetime có thể lược bỏ ở chữ ký hàm theo một số luật[^el0]; Book nhấn mạnh đây không phải luật cho người viết mà là các trường hợp compiler thử, nếu khớp thì không cần viết[^el1]. Ba luật:

1. Mỗi lifetime bị lược trong tham số là một tham số lifetime *riêng biệt*[^el2].
2. Nếu có đúng một lifetime trong các tham số (lược hay không), lifetime đó gán cho mọi lifetime đầu ra bị lược[^el3].
3. Nếu receiver là `&Self`/`&mut Self`, lifetime của reference tới `self` gán cho mọi lifetime đầu ra bị lược[^el4].

Áp dụng cho `impl` blocks cũng như `fn`[^el5]. Nếu hết luật mà vẫn không suy ra được thì việc lược các tham số lifetime không suy ra được là lỗi[^el6].

```rust
fn first(s: &str) -> &str { &s[..1] }                       // luật 1+2
struct S { s: String }
impl S { fn get(&self, _other: &str) -> &str { &self.s } }  // luật 3: gắn với self
fn pick(a: &str, b: &str) -> &str { a }                     // không luật nào cứu
```
Dòng cuối cho `E0106: missing lifetime specifier` — "the signature does not say whether it is borrowed from `a` or `b`".

Hai hàm đầu biên dịch và chạy ra `abc q`. Lưu ý quan trọng với backend: luật 3 "gắn kết quả với `self`" thường *không* phải điều bạn muốn khi hàm trả dữ liệu lấy từ tham số khác — lúc đó phải viết lifetime tường minh.

### 8.2 Lifetime trong struct

Struct chứa reference cần khai báo lifetime; Book: một instance của `ImportantExcerpt` không thể sống lâu hơn reference nó giữ trong field `part`[^st1]. Với `struct Parser<'a> { input: &'a str }`, hãy phân biệt hai chữ ký:

```rust
impl<'a> Parser<'a> {
    fn first_word(&self) -> &'a str { self.input.split(' ').next().unwrap_or("") }
}
```

Kết quả gắn với `'a` (dữ liệu gốc), *không* gắn với `&self`. Nếu viết `-> &str` thì luật 3[^el4] gắn kết quả với `&self` — ngắn hơn, nên (suy luận của người soạn) người gọi không giữ được kết quả sau khi `Parser` bị drop. Trong chương trình thử `Parser` bị drop cuối khối nhưng `w` vẫn dùng được (in `hello`) vì `input` là literal `'static`; nếu `input` trỏ vào `String` cục bộ thì cho E0597 như ở 7.5.

### 8.3 `'static`: bound khác reference

Có hai cách dùng, gần nhau về chữ nhưng khác nghĩa — Rust By Example nói chúng có liên quan nhưng khác nhau tinh tế và là nguồn nhầm lẫn thường gặp khi học Rust[^sta0]. Hai cách:

- **`&'static T`**: tham chiếu có thể sống suốt chương trình. Book: `'static` chỉ ra reference bị ảnh hưởng có thể sống suốt thời gian chạy chương trình[^sta1], literal chuỗi luôn là `'static`.
- **`T: 'static`** là *bound* trên kiểu: Reference định nghĩa `T: 'a` nghĩa là *mọi tham số lifetime của `T`* outlive `'a`; `i32: 'static` thỏa, `&'static str: 'a` thỏa, `Vec<&'a ()>: 'static` thì không[^sta2]. Rust By Example: là trait bound, nó có nghĩa kiểu không chứa tham chiếu non-static nào[^sta3], và dữ liệu sở hữu luôn qua bound `'static` còn *tham chiếu tới* nó nói chung thì không[^sta4].

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

Vì thế `thread::spawn` đòi `F: … + Send + 'static`: tài liệu giải thích closure và giá trị trả về phải có lifetime bằng cả thời gian chạy chương trình vì thread có thể sống lâu hơn nơi tạo ra nó[^sta5]. Cách đáp ứng thông thường là `move` ownership vào closure (đã chạy: thread in `t`), không phải "kéo dài" biến cục bộ. Chi tiết `Send`/scoped thread ở [P4·08](../04-concurrency/08-rust-concurrency.md).

Book cảnh báo: khi lỗi gợi ý `'static`, thường là bạn đang tạo dangling reference hoặc lệch lifetime — giải pháp là sửa các vấn đề đó chứ không phải chỉ định lifetime `'static`[^sta6].

`Box::leak` biến dữ liệu sở hữu thành `&'static mut T` nếu T chỉ chứa tham chiếu static hoặc không có[^sta7]; bỏ reference thì rò bộ nhớ[^sta8] (đã chạy: `Box::leak(s.into_boxed_str())` ra `&'static str`). Nhận xét của người soạn: chỉ hợp cho cấu hình nạp một lần lúc khởi động.

</details>

**Quay lại Q6:** elision là ba luật điền lifetime ở chữ ký (hết luật thì E0106); `T: 'static` là bound trên kiểu, không phải "sống mãi".

## 9. Mẫu sửa lỗi thường gặp (Q7)

> **Trả lời ngắn.** Có bốn lối thoát thường dùng: `clone` có chủ đích, thu hẹp scope, giữ index/key thay vì reference, và `Option::take`/`mem::replace`/`mem::take` để move ra khỏi sau `&mut` bằng cách để lại một giá trị hợp lệ[^std1][^std2][^std3]. Chọn theo việc bạn thật sự cần: một bản sao, một thứ tự thời gian khác, hay quyền sở hữu.

**Giải thích.**

Hình dung (nhận xét của người soạn): muốn lấy cuốn sách khỏi giá chung — photo (clone), đợi người đọc xong (tách scope), ghi số kệ thay vì cầm sách (index), hoặc lấy sách ra nhưng đặt một cuốn trống vào chỗ cũ (take/replace).

### 9.1 `clone` có chủ đích

`let first = v[0].clone();` sửa E0507 ở mục 2.3 nhưng trả một lần cấp phát và sao chép: hợp khi dữ liệu nhỏ hoặc ngoài hot path; sai khi chỉ để "im lặng compiler" (nhận xét của người soạn). Dữ liệu lớn dùng chung: `Rc`/`Arc` — [P9·04](04-smart-pointer.md).

### 9.2 Tách scope

Thu hẹp phạm vi dùng reference (khối `{}` hoặc tách hàm) để vùng borrow kết thúc trước lần mutate; nhờ NLL (mục 6) nhiều trường hợp không cần khối tường minh.

### 9.3 Index thay vì reference

Giữ `usize`/key thay vì `&T` để không khóa collection:

```rust
let mut v = vec![1, 2, 3];
let i = v.iter().position(|&x| x == 2).unwrap();
v.push(4);        // không có borrow nào đang sống
v[i] += 10;
println!("{v:?}"); // in "[1, 12, 3, 4]"
```

Đánh đổi: index có thể cũ đi khi collection đổi thứ tự/xóa phần tử; với `HashMap` dùng key, với dữ liệu hay xóa dùng ID ổn định.

### 9.4 `Option::take`, `mem::replace`, `mem::take`

Ba hàm này là cách hợp pháp để "move ra khỏi sau `&mut`": chúng để lại một giá trị hợp lệ thay chỗ, nên owner không bao giờ ở trạng thái rút ruột (khác với mục 2.3).

- `Option::take`: lấy giá trị ra khỏi option và để lại `None` tại chỗ[^std1].
- `mem::replace(dest, src)`: chuyển `src` vào `dest` được tham chiếu và trả về giá trị cũ của `dest`[^std2].
- `mem::take(dest)`: thay `dest` bằng giá trị mặc định của `T` và trả về giá trị cũ của `dest`[^std3].

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

Nếu viết `let n = self.head; …` thì lại là E0507 ("cannot move out of `self.head` which is behind a mutable reference"); `take()` là lối thoát. Với `Vec<String>`: `mem::take(&mut v[0])` lấy phần tử (để lại `""`), `v.swap_remove(1)` lấy ra O(1) bằng cách đổi chỗ với phần tử cuối (đã chạy: `a b  ["z", "c"]`).

</details>

**Quay lại Q7:** clone khi cần bản sao, tách scope hoặc index khi chỉ là vấn đề thời điểm, `take`/`replace` khi cần lấy quyền sở hữu từ sau `&mut`.

## 10. Liên hệ backend (nhận xét của người soạn)

**Request/response.** Handler nhận `Request` *by value*, đọc `&req.body` (borrow ngắn), rồi destructure `let Request { user, .. } = req;` lấy field sở hữu mà không clone (đã chạy: `u:4`) — partial move hợp lệ (mục 2.4), miễn struct không `Drop`. Response nên là kiểu sở hữu (`String`, `Vec<u8>`) để khỏi vướng lifetime của request.

**Giữ reference vào cache.** Hàm trả `&V` mượn cả cache, nên suốt thời gian người gọi giữ kết quả, cache không mutate được (mục 6). Hai hướng: `entry`/`or_insert_with` (một lần mượn), hoặc cache lưu `Arc<V>` và trả bản clone — hợp khi giá trị phải sống qua `.await`/sang thread (cần `'static`, mục 8.3), đổi lại một bộ đếm tham chiếu atomic mỗi lần clone.

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

`v[0]` là place sau một tham chiếu/index, không nằm trong danh sách place được move ra[^move3][^move4][^move5]; `&v[0]` chỉ mượn, không move (mục 2.3).

</details>

2. Sau `let n = u.name;`, dòng nào dùng được: `u.age`, `u.email`, `{:?}` của `u`?
<details><summary>Đáp án</summary>

`u.age` và `u.email` dùng được, `{:?}` của cả `u` thì E0382 "partially moved" (mục 2.4); khi `u` ra scope chỉ field còn khởi tạo bị drop[^dtor1].

</details>

3. Vì sao `let n = g.name;` lỗi nếu `Guard` có `impl Drop`?
<details><summary>Đáp án</summary>

Điều kiện move field là kiểu không implement Drop[^move4]; rustc báo E0509 (mục 2.4).

</details>

4. Khi nào `emit(r)` hai lần với `r: &mut T` biên dịch, khi nào không?
<details><summary>Đáp án</summary>

Biên dịch khi tham số có kiểu `&mut T` rõ ràng (reborrow ngầm); với tham số generic `W` thì `r` bị move và lần hai là E0382, sửa bằng `&mut *r` (mục 4; phần "ngầm" là quan sát, không phải trích dẫn).

</details>

5. "Lifetime của `&x` bằng scope của `x`" sai ở đâu?
<details><summary>Đáp án</summary>

Lifetime là vùng mã mà reference phải hợp lệ, có thể có lỗ và kết thúc ở lần dùng cuối[^lt1][^lt2][^nll2]; scope của `x` chỉ là cận trên mà vùng đó không được vượt.

</details>

6. Viết lại `fn f(a: &str, b: &str) -> &str` cho đúng nếu kết quả luôn lấy từ `a`.
<details><summary>Đáp án</summary>

`fn f<'a>(a: &'a str, b: &str) -> &'a str`. Không luật elision nào cứu được trường hợp hai tham số (luật 2 cần đúng một lifetime; luật 3 cần `self`)[^el3][^el4][^el6].

</details>

7. `T: 'static` có nghĩa `T` sống mãi không? Cho ví dụ kiểu thỏa và kiểu không thỏa.
<details><summary>Đáp án</summary>

Không: nghĩa là mọi tham số lifetime của `T` outlive `'static`; `i32`, `String` thỏa, `Vec<&'a ()>` với `'a` không-static thì không[^sta2][^sta3][^sta4].

</details>

8. Vì sao `String::from("hello").as_str()` gán vào `let` gây E0716?
<details><summary>Đáp án</summary>

Temporary bị drop ở cuối câu lệnh (scope temporary nhỏ nhất bao biểu thức, gồm một câu lệnh[^tmp1]) trong khi `s` còn dùng sau đó. Sửa bằng biến có tên.

</details>

9. Vì sao `get_or_load` dạng `if let Some(v) = c.get(&k) { return v; } c.insert(…)` bị từ chối còn `entry().or_insert_with` thì được?
<details><summary>Đáp án</summary>

Nhánh trả `v` buộc borrow của `c.get` sống bằng lifetime của giá trị trả về, nên trùm `insert` (thông điệp "returning this value requires that `*c` is borrowed for `'1`"); `entry` chỉ mượn `&mut` một lần (mục 6).

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
