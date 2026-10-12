# P4 · Chương 08 — Concurrency trong Rust

## Bạn sẽ hiểu được gì

1. **Q1.** Vì sao `std::thread::spawn` đòi `F: Send + 'static`, mỗi vế của ràng buộc chặn loại lỗi nào, và `thread::scope` nới ràng buộc nào? → mục 2
2. **Q2.** `Send` và `Sync` định nghĩa chính xác là gì, vì sao `T: Sync` ⇔ `&T: Send`, và vì sao `impl` thủ công là `unsafe`? → mục 3
3. **Q3.** Vì sao `Rc` không qua được thread, và `Arc<T>` chỉ qua được khi nào? → mục 4
4. **Q4.** `Cell`, `RefCell`, `UnsafeCell` khác `Mutex` ở đâu, vì sao `Cell` là `!Sync`, và vì sao `Arc<Cell<i32>>` vẫn bị từ chối? → mục 5
5. **Q5.** `Mutex<T>` cần `T: Send` chứ không cần `T: Sync` là sao; poisoning là gì, vì sao `RwLock` chỉ bị poisoned khi panic ở chế độ ghi? → mục 6
6. **Q6.** `OnceLock`/`LazyLock` giải bài toán gì? → mục 7
7. **Q7.** `mpsc` (`channel`, `sync_channel`) khác nhau thế nào và bên trong là gì? → mục 9
8. **Q8.** Safe Rust chặn được gì (data race) và không chặn được gì (race condition, deadlock)? → mục 10

## Kiến thức cần trước

- [Chương 01 — Thread](./01-thread.md): thread là đơn vị mà kernel lên lịch, chia sẻ address space.
- [Chương 03 — Lock](./03-lock.md) và [Chương 04 — Condition variable và semaphore](./04-condition-variable-va-semaphore.md): mutex, condvar, predicate trong vòng lặp.
- [Chương 05 — Concurrency bug và deadlock](./05-concurrency-bug-va-deadlock.md): data race, atomicity violation, deadlock.
- [Chương 06 — Memory model](./06-memory-model.md) và [Chương 07 — Lock-free](./07-lock-free.md): `happens-before`, `Ordering`, CAS. Chương này chỉ *dùng* các khái niệm đó, không dạy lại.
- Ownership và borrowing của Rust (xem thêm [P3 chương 08 — Rust và bộ nhớ](../03-memory/08-rust-va-bo-nho.md)).

Ghi chú: API theo Rust 1.95.0; mọi lỗi biên dịch trong chương được `rustc 1.95.0` (edition 2021) in ra thật, rút gọn đường dẫn nhưng giữ nguyên mã lỗi và dòng chính.

## 1. Ý tưởng chung: để type system trả lời "giá trị này đi qua thread được không?"

Các chương trước đã cho thấy concurrency khó vì data race: hai thread cùng truy cập một vị trí nhớ, ít nhất một bên ghi, và không có đồng bộ[^nomicon-race-def] (đây là cách phát biểu của Nomicon; định nghĩa của `std` mà cả khóa dùng, có điều kiện "non-atomic", nằm ở [chương 06, mục 3](./06-memory-model.md)). Rust trả lời bằng hệ thống kiểu: giá trị kiểu `T` có được phép chuyển sang thread khác không (`Send`), có được phép để nhiều thread cùng giữ `&T` không (`Sync`). (Nhận xét của người soạn: thread, mutex, channel ở đây đều là kiểu của thư viện chuẩn, nên các ràng buộc ấy được kiểm lúc biên dịch như mọi ràng buộc kiểu khác.) Nomicon nói data race "được chặn chủ yếu chỉ bằng ownership": không thể alias một `&mut`, nên không thể có data race qua tham chiếu; *interior mutability* làm bài toán phức tạp hơn, và đó phần lớn là lý do tồn tại của `Send`/`Sync`[^nomicon-race-ownership].

## 2. `spawn`, `move` và `scope`: vì sao `F: Send + 'static` (Q1)

> **Trả lời ngắn.** `'static` chặn thread giữ tham chiếu tới dữ liệu cục bộ, vì thread có thể sống lâu hơn nơi tạo ra nó[^std-spawn-static]. `Send` chặn việc chuyển sang thread khác những giá trị không an toàn để chuyển, vì closure và kết quả đi qua ranh giới thread[^std-spawn-send]. `thread::scope` nới `'static` thành `'scope`, vì scope bảo đảm mọi thread được join ở cuối scope[^std-scope].

**Giải thích.**

Hình dung (nhận xét của người soạn): giao việc cho đồng nghiệp ca sau thì phải đưa hẳn bản gốc, không đưa tờ giấy "xem trên bàn tôi" vì có thể tôi đã nghỉ; `scope` là lời hứa "tôi chưa về cho tới khi bạn làm xong".


`std::thread::spawn` tạo một OS thread; thư viện chuẩn dùng mô hình 1:1, mỗi thread của ngôn ngữ ứng với một thread của hệ điều hành[^book-11] (khớp với [chương 01](./01-thread.md)). Chữ ký của nó, theo docs 1.95.0, là `F: FnOnce() -> T + Send + 'static, T: Send + 'static`[^std-spawn-sig]. Docs giải thích từng vế:

- **`'static`**: closure và giá trị trả về phải sống suốt thời gian chạy của chương trình, vì thread có thể sống lâu hơn scope đã tạo ra nó[^std-spawn-static]. Chương trình không biết khi nào thread kết thúc, nên không thể cho phép nó giữ tham chiếu tới biến cục bộ của hàm gọi.
- **`Send`**: closure được chuyển *theo giá trị* từ thread gọi sang thread mới, còn giá trị trả về được chuyển từ thread mới về thread gọi `join`[^std-spawn-send].

Ví dụ vế `'static` (đã chạy):

```rust
use std::thread;
fn main() {
    let v = vec![1, 2, 3];
    let h = thread::spawn(|| println!("{:?}", v));
    h.join().unwrap();
}
```

```text
error[E0373]: closure may outlive the current function, but it borrows `v`, which is owned by the current function
 --> borrow.rs:4:27
  |
4 |     let h = thread::spawn(|| println!("{:?}", v));
  |                           ^^                  - `v` is borrowed here
  |                           |
  |                           may outlive borrowed value `v`
note: function requires argument type to outlive `'static`
help: to force the closure to take ownership of `v` (and any other referenced variables), use the `move` keyword
```

Lỗi này là `E0373`, không phải `E0277`: nó thuộc vế `'static`/lifetime. Cách sửa mà compiler gợi ý là `move`: closure *lấy ownership* các giá trị nó dùng, tức chuyển ownership từ thread này sang thread kia[^book-move]. Sau `move`, `v` thuộc về thread con nên không còn tham chiếu nào có thể treo.

**`thread::scope`: mượn biến cục bộ một cách hợp lệ.** Ràng buộc `'static` tồn tại vì `spawn` không biết khi nào thread kết thúc. `thread::scope` cho Rust biết điều đó: các thread trong scope được phép mượn dữ liệu không-`'static`, vì scope bảo đảm mọi thread được join ở cuối scope[^std-scope]; thread nào chưa join thủ công sẽ được join tự động trước khi `scope` trả về[^std-scope-auto]. Ràng buộc của `Scope::spawn` vì vậy là `F: FnOnce() -> T + Send + 'scope` — vẫn `Send`, nhưng `'scope` thay cho `'static`[^std-scope-spawn].

```rust
let mut v = vec![1, 2, 3];
let mut total = 0;
thread::scope(|s| {
    s.spawn(|| println!("len = {}", v.len()));
    s.spawn(|| total = v.iter().sum());
});
v.push(4);
println!("total = {total}, v = {v:?}");
```

Kết quả chạy thật: `len = 3` rồi `total = 6, v = [1, 2, 3, 4]`. Nếu một thread trong scope panic mà chưa được join thủ công, `scope` sẽ panic; muốn tự xử lý panic thì join trước khi hết scope[^std-scope-panic].

<details><summary>Đi sâu: <code>JoinHandle</code>, detach và panic trong thread</summary>

`spawn` trả về `JoinHandle<T>`, một giá trị owned mà khi gọi `join` sẽ chờ thread kết thúc[^book-spawn-join]. Hai điểm hay bị bỏ qua:

- **Drop handle = detach.** Nếu `JoinHandle` bị drop, thread tự động bị *detach*; docs nhắc chương trình có trách nhiệm hoặc join hoặc detach thread, nếu không sẽ rò rỉ tài nguyên[^std-spawn-detach].
- **Panic không làm sập process mặc định.** Nếu thread con panic, `join` trả `Err` chứa tham số của `panic!`[^std-spawn-panic].

Quan sát (chạy thật):

```rust
let h = thread::spawn(|| { if true { panic!("boom"); } 7 });
let r = h.join();
println!("join is_err = {}", r.is_err());
println!("payload = {:?}", r.unwrap_err().downcast_ref::<&str>());
```

```text
thread '<unnamed>' (...) panicked at panic.rs:4:42:
boom
join is_err = true
payload = Some("boom")
```

Thread con in thông báo panic ra stderr; thread cha nhận `Err(Box<dyn Any + Send>)` và có thể `downcast_ref` payload.

</details>

**Quay lại Q1:** `'static` chặn tham chiếu treo tới biến cục bộ, `Send` chặn chuyển giá trị không an toàn qua thread, và `scope` thay `'static` bằng `'scope` nhờ bảo đảm join cuối scope.

## 3. `Send` và `Sync` (Q2)

> **Trả lời ngắn.** `Send`: giá trị của kiểu đó chuyển được sang thread khác[^book-send]. `Sync`: `T: Sync` khi và chỉ khi `&T: Send`[^std-sync]. Cả hai là `unsafe auto trait`[^std-send-auto]: compiler tự suy từ các thành phần[^nomicon-auto], còn `unsafe impl` thủ công là lời hứa mà compiler không kiểm, cài sai có thể gây Undefined Behavior[^nomicon-send-ub].

**Giải thích.**

Hình dung (nhận xét của người soạn): `Send` là "giao hẳn món đồ cho người khác"; `Sync` là "cho nhiều người cùng nhìn vào món đồ" — và định nghĩa quy việc nhìn chung về việc đưa *tờ ghi chú tham chiếu* cho người khác.


| Trait | Định nghĩa |
|---|---|
| `Send` | Giá trị của kiểu đó có thể được chuyển ownership sang thread khác[^book-send]; Nomicon: "an toàn khi gửi sang thread khác"[^nomicon-send-def] |
| `Sync` | Định nghĩa chính xác trong docs: `T: Sync` khi và chỉ khi `&T: Send`[^std-sync] |

Định nghĩa của `Sync` đáng đọc chậm: "chia sẻ giữa các thread" được *quy về* `Send` — một kiểu chia sẻ được khi việc gửi tham chiếu `&T` sang thread khác là an toàn. Docs còn tóm tắt các quan hệ với tham chiếu: `&T` là `Send` ⇔ `T` là `Sync`; `&mut T` là `Send` ⇔ `T` là `Send`; `&T` và `&mut T` là `Sync` ⇔ `T` là `Sync`[^std-sync-ref]. Cách hiểu "`&mut T: Send` ⇔ `T: Send`" (cách diễn giải của người soạn): một `&mut T` là quyền truy cập độc quyền, nên gửi nó đi chẳng khác gì gửi quyền sở hữu tạm thời.

**Auto trait và `unsafe trait`.**

`Send` và `Sync` khai báo là `pub unsafe auto trait`[^std-send-auto]. Hai chữ đó có nghĩa:

- **auto**: khác mọi trait khác, nếu một kiểu gồm toàn thành phần `Send`/`Sync` thì nó tự động là `Send`/`Sync`[^nomicon-auto]; vì là marker trait không có method nên không có gì để "cài"[^book-sendsync-manual]. Docs: trait được cài tự động "khi compiler xác định là phù hợp"[^std-send-auto2].
- **unsafe**: `impl` thủ công phải viết `unsafe impl`. Nomicon: chúng là unsafe trait, nghĩa là unsafe để cài, và code unsafe khác được quyền *giả định* chúng đã được cài đúng[^nomicon-unsafe]; cài sai có thể gây Undefined Behavior[^nomicon-send-ub]. Đây chính là chỗ thư viện như `Mutex` đặt lời hứa với toàn hệ thống kiểu.

Ngoại lệ chính của auto-derivation theo Nomicon: raw pointer không `Send`/`Sync`, `UnsafeCell` không `Sync` (kéo theo `Cell`, `RefCell` cũng không)[^nomicon-exceptions], và `Rc` không `Send`/`Sync`[^nomicon-rc]. Nomicon giải thích raw pointer bị đánh dấu thread-unsafe "nhiều hơn như một lint": dùng được raw pointer thì phải dereference vốn đã `unsafe`, nhưng đánh dấu như vậy là để các kiểu chứa nó không tự động được coi là thread-safe, vì tác giả của chúng nhiều khả năng chưa nghĩ kỹ về thread safety[^nomicon-rawptr-lint].

<details><summary>Đi sâu: thử <code>MyBox</code> chứa raw pointer và <code>MutexGuard</code> không <code>Send</code></summary>

Thử với một kiểu chứa raw pointer:

```rust
struct MyBox(*mut u8);
// ... thread::spawn(move || { let _b = b; }) với b: MyBox
```

```text
error[E0277]: `*mut u8` cannot be sent between threads safely
  = help: within `{closure@mybox.rs:6:27: 6:34}`, the trait `Send` is not implemented for `*mut u8`
note: required because it appears within the type `MyBox`
```

Thêm `unsafe impl Send for MyBox {}` (đúng dạng Nomicon minh họa cho "kiểu không tự derive thì tự cài"[^nomicon-mybox]) thì chương trình biên dịch được — đã chạy thật, 0 lỗi. Compiler *không* kiểm lời hứa đó: người viết `unsafe impl` chịu trách nhiệm toàn bộ.

**Ví dụ phản diện: `MutexGuard` không `Send`.**

`MutexGuard` cố ý không `Send`: Nomicon lấy nó làm ví dụ đẹp cho một kiểu không `Send`[^nomicon-mutexguard], vì implementation của lock đòi không giải phóng lock từ thread khác với thread đã lấy; gửi guard sang thread khác tức là destructor (nhả lock) chạy ở thread nhận[^nomicon-mutexguard-why]. Thử gửi guard:

```text
error[E0277]: `std::sync::MutexGuard<'_, i32>` cannot be sent between threads safely
  = help: within `{closure@guard.rs:6:19: 6:26}`, the trait `Send` is not implemented for `std::sync::MutexGuard<'_, i32>`
```

</details>

**Quay lại Q2:** `Sync` được định nghĩa qua `Send` của `&T`; cả hai tự suy từ thành phần, và chỉ `unsafe impl` mới ép được chúng — bằng lời hứa của người viết.

## 4. `Rc` và `Arc` (Q3)

> **Trả lời ngắn.** `Rc` đếm tham chiếu bằng phép cộng/trừ thường, nên hai thread cùng clone có thể cập nhật bộ đếm cùng lúc và đó là UB; vì vậy `Rc` không `Send` hay `Sync`[^std-send-rc][^nomicon-rc]. `Arc` dùng phép atomic cho refcount nên thread-safe[^std-arc], nhưng `Arc<T>` chỉ là `Send + Sync` khi `T` cũng vậy[^std-arc-bound].

**Giải thích.**

Hình dung (nhận xét của người soạn): `Rc` là sổ đếm người mượn sách ghi bằng bút chì ở một quầy; hai thủ thư cùng sửa một dòng là sai số. `Arc` là sổ mà mỗi lần sửa một dòng là một nét bút không thể chia cắt — nhưng nét bút đó chỉ bảo vệ con số đếm, không bảo vệ chính cuốn sách.


`Rc` đếm tham chiếu bằng phép cộng/trừ thường. Docs `Send` nêu đúng kịch bản: hai thread cùng clone `Rc` trỏ tới một giá trị có thể cập nhật bộ đếm cùng lúc, là UB vì `Rc` không dùng atomic[^std-send-rc]. Nomicon tóm tắt: `Rc` không `Send` hay `Sync` vì refcount được chia sẻ mà không đồng bộ[^nomicon-rc]. Docs `Sync` bổ sung góc nhìn `Sync`: từ một `&Rc<T>` bất kỳ bạn đã có thể clone ra `Rc<T>` mới, tức sửa bộ đếm theo kiểu không atomic[^std-sync-rc] — nên `Rc` cũng không `Sync`, không chỉ không `Send`.

`Arc` là bản dùng atomic cho refcount nên thread-safe[^std-arc]; Book mô tả `Arc<T>` là kiểu như `Rc<T>` nhưng an toàn trong ngữ cảnh đồng thời[^book-arc]; sách Mara Bos: giống `Rc` trừ việc bảo đảm thay đổi bộ đếm là phép atomic không chia cắt được[^atomics-arc]. Cái giá là chi phí atomic (docs `Send` nói `Arc` dùng atomic "incurring some overhead")[^std-send-rc]. Hiểu lầm hay gặp: `Arc` *không* tự làm cho nội dung thread-safe — `Arc<T>` chỉ là `Send + Sync` khi `T` cũng `Send + Sync`[^std-arc-bound]. Mục 5 cho thấy hậu quả.

<details><summary>Đi sâu: lỗi biên dịch khi gửi <code>Rc</code> vào <code>thread::spawn</code></summary>

Chạy thật:

```rust
let r = Rc::new(5);
let h = thread::spawn(move || println!("{}", r));
```

```text
error[E0277]: `Rc<i32>` cannot be sent between threads safely
  --> rc.rs:5:27
   |
 5 |     let h = thread::spawn(move || println!("{}", r));
   |             ------------- -------^^^^^^^^^^^^^^^^^^
   |             |             |
   |             |             `Rc<i32>` cannot be sent between threads safely
   |             |             within this `{closure@rc.rs:5:27: 5:34}`
   |             required by a bound introduced by this call
   = help: within `{closure@rc.rs:5:27: 5:34}`, the trait `Send` is not implemented for `Rc<i32>`
note: required by a bound in `spawn`
128 |     F: Send + 'static,
    |        ^^^^ required by this bound in `spawn`
```

Chuỗi nhân quả đọc từ lỗi: closure chứa `Rc<i32>` ⇒ closure không `Send` (auto trait suy từ thành phần) ⇒ vi phạm ràng buộc `F: Send` của `spawn`. Đây là cơ chế mục 3 hoạt động.

</details>

**Quay lại Q3:** `Rc` đếm bằng phép không atomic nên không qua thread; `Arc` đếm atomic, nhưng chỉ `Send + Sync` khi nội dung `T` cũng vậy.

## 5. Interior mutability: `UnsafeCell`, `Cell`, `RefCell` (Q4)

> **Trả lời ngắn.** `UnsafeCell<T>` là nền của interior mutability — cho đổi dữ liệu qua `&T`[^std-unsafecell-core]. `Cell` là `!Sync` vì `set` chỉ cần `&self` mà không đồng bộ gì[^std-sync-cell]; vì thế `Arc<Cell<i32>>` bị từ chối (`Arc<T>: Send` đòi `T: Sync`[^std-arc-bound]). `Mutex<T>` thêm khóa nên `Sync` khi `T: Send`[^std-mutex-sync].

**Giải thích.**

Hình dung (nhận xét của người soạn): `Cell` là ngăn kéo không khóa trong văn phòng riêng — an toàn khi một người dùng, nhưng hai người cùng mở thì lẫn lộn; `Mutex` là ngăn kéo có khóa.


Quy tắc nền của Rust: với một object, chỉ có thể có nhiều tham chiếu bất biến *hoặc* một tham chiếu khả biến[^std-cell-rule]. Interior mutability là cách "lách" có kiểm soát: cho đổi giá trị qua `&T`, trái với kiểu thường chỉ đổi được qua `&mut T`[^std-cell-interior]. Gốc của mọi kiểu như vậy là `UnsafeCell<T>`, primitive lõi của interior mutability[^std-unsafecell-core]: nó bỏ đảm bảo bất biến của `&T` — `&UnsafeCell<T>` có thể trỏ tới dữ liệu đang bị đổi — và `Cell`, `RefCell` đều dùng nó bên trong[^std-unsafecell]. Docs `Sync` còn nhấn mạnh: mọi kiểu có interior mutability *phải* dùng `UnsafeCell` bọc giá trị, vì không làm vậy là UB (ví dụ `transmute` từ `&T` sang `&mut T` là không hợp lệ)[^std-sync-unsafecell].

| Kiểu | Cơ chế | Qua thread? |
|---|---|---|
| `Cell<T>` | đổi bằng cách chuyển giá trị vào/ra, không bao giờ lấy được `&T` bên trong[^std-cell-cell] | `!Sync`: `set` chỉ cần `&self` mà không đồng bộ gì[^std-sync-cell] |
| `RefCell<T>` | kiểm tra luật mượn **lúc chạy** thay vì lúc biên dịch[^std-refcell] | `!Sync` (Nomicon liệt kê)[^nomicon-exceptions] |
| `Mutex<T>` | kiểm tra bằng lock; Book: `Mutex<T>` cung cấp interior mutability như họ `Cell`[^book-interior] | `Sync` khi `T: Send`[^std-mutex-sync] |
| `Atomic*` | phép atomic; chương [06](./06-memory-model.md)–[07](./07-lock-free.md) | `Sync`[^std-atomic-sync] |

**Vì sao `Cell` là `!Sync` — suy luận từng bước.** `Cell::set(&self, v)` chỉ cần `&Cell<T>`. Nếu `Cell<T>: Sync` thì theo định nghĩa `&Cell<T>: Send`, tức hai thread có thể cùng cầm `&Cell<T>` và cùng gọi `set`: hai lần ghi không đồng bộ vào một vị trí nhớ, đúng định nghĩa data race. Docs `Sync` nói gọn: method này không thực hiện đồng bộ nào, nên `Cell` không thể `Sync`[^std-sync-cell]. Còn `Send`: docs `Cell` liệt kê `impl<T> Send for Cell<T> where T: Send + ?Sized`[^std-cell-send] — tức `Cell<T>` là `Send` khi `T: Send`. Lý do (suy luận của người soạn): chuyển *quyền sở hữu* nguyên cả cell sang thread khác thì không ai còn alias nó. Mục 6 cho thấy `Mutex<Cell<i32>>` chạy được, khớp với hệ quả này; ví dụ `move` một `Cell<i32>` thẳng vào `thread::spawn` cũng biên dịch và chạy (quan sát của người soạn, in `2`).

Đây là lý do thực hành `Arc<Mutex<T>>` thay `Rc<RefCell<T>>`: Book so sánh đúng hai cặp này[^book-interior].

<details><summary>Đi sâu: lỗi biên dịch với <code>Arc&lt;Cell&lt;i32&gt;&gt;</code> và <code>&amp;Rc&lt;i32&gt;</code> trong <code>thread::scope</code></summary>

Thử `Arc<Cell<i32>>` (đã chạy):

```rust
let a = Arc::new(Cell::new(1));
let b = Arc::clone(&a);
thread::spawn(move || b.set(2)).join().unwrap();
```

```text
error[E0277]: `Cell<i32>` cannot be shared between threads safely
 --> cell.rs:7:19
  |
7 |     thread::spawn(move || b.set(2)).join().unwrap();
  |     ------------- ^^^^^^^^^^^^^^^^ `Cell<i32>` cannot be shared between threads safely
  = help: the trait `Sync` is not implemented for `Cell<i32>`
  = note: if you want to do aliasing and mutation between multiple threads, use `std::sync::RwLock` or `std::sync::atomic::AtomicI32` instead
  = note: required for `Arc<Cell<i32>>` to implement `Send`
note: required by a bound in `spawn`
```

Dòng `required for Arc<Cell<i32>> to implement Send` cho thấy `Arc<T>: Send` đòi `T: Send + Sync` (mục 4) mà `Cell<i32>: !Sync`.

Một kiểm tra thú vị của định nghĩa `Sync ⇔ &T: Send`: `thread::scope` dùng `Send` trên closure, mà closure *mượn* `Rc` chỉ giữ `&Rc<i32>`. Chạy thật:

```text
error[E0277]: `Rc<i32>` cannot be shared between threads safely
  = help: the trait `Sync` is not implemented for `Rc<i32>`
  = note: required for `&Rc<i32>` to implement `Send`
```

Compiler tự chuyển "`&Rc<i32>` phải `Send`" thành "`Rc<i32>` phải `Sync`" đúng theo định nghĩa.

</details>

**Quay lại Q4:** interior mutability đổi dữ liệu qua `&T`; `Cell`/`RefCell` không đồng bộ nên `!Sync`, còn `Mutex` thêm khóa nên dùng được qua thread.

## 6. `Mutex`, `RwLock`, `Condvar` và poisoning (Q5)

> **Trả lời ngắn.** `Mutex<T>` bọc dữ liệu và chỉ cho truy cập qua guard, nên chỉ chạm dữ liệu khi đang khóa[^std-mutex-guard]. `T` chỉ cần `Send` vì `&T` chỉ được đưa cho *một* thread tại một thời điểm[^std-mutex-notsync]; `RwLock` cho nhiều reader đồng thời nên cần `T: Send + Sync`[^std-rwlock-bounds]. Mutex bị poisoned khi thread đang giữ nó panic[^std-poison]; `RwLock` chỉ bị poisoned khi panic ở chế độ ghi[^std-rwlock-poison].

**Giải thích.**

Hình dung (nhận xét của người soạn): `Mutex<T>` là phòng có một chìa — ai cầm chìa mới vào được; poisoning là biển "người dùng phòng trước ngất giữa chừng, đồ đạc có thể lộn xộn".

**`Mutex<T>`: dữ liệu nằm *trong* lock.**

Khác mutex ở [chương 03](./03-lock.md), `Mutex<T>` bọc dữ liệu, và dữ liệu chỉ truy cập được qua RAII guard, bảo đảm chỉ chạm dữ liệu khi mutex đang bị khóa[^std-mutex-guard]. `MutexGuard` cài `Deref` và `Drop`, nhả lock khi ra khỏi scope[^book-guard]; muốn nhả sớm thì tạo scope con hoặc `drop` guard thủ công[^std-mutex-drop]. Vì không có đường nào tới dữ liệu mà không qua guard, "quên lock" trở thành lỗi biên dịch (suy luận từ hai nguồn trên). Việc `unlock` rồi `lock` tạo quan hệ happens-before giữa các thread — một trong số ít trường hợp có sẵn theo Mara Bos[^atomics-hb-cases] ([chương 06](./06-memory-model.md)).

Locking đệ quy: nếu thread đã giữ lock mà gọi `lock` lần nữa, hành vi chính xác "không được chỉ định", nhưng hàm chắc chắn không trả về ở lần gọi thứ hai (có thể panic hoặc deadlock)[^std-mutex-twice].

**Ràng buộc kiểu — vì sao `T: Send` mà không cần `T: Sync`.** Docs: `Mutex<T>: Sync` khi `T: Send`, bảo đảm dữ liệu được bảo vệ truy cập an toàn từ nhiều thread[^std-mutex-sync]; và `T` không cần `Sync` vì `&T` chỉ được đưa cho *một* thread tại một thời điểm[^std-mutex-notsync]. Cũng có `impl Send for Mutex<T>` khi `T: Send`[^std-mutex-send]. Lập luận (của người soạn): `Sync` của `T` nghĩa là nhiều thread cùng giữ `&T` đồng thời an toàn; mutex chỉ đưa `&T` cho một thread mỗi lúc nên thứ cần thiết chỉ là *chuyển* `T` giữa các thread lần lượt — đúng nghĩa `Send`. Hệ quả kiểm chứng được ở phần Đi sâu.

**Poisoning.**

Mutex bị *poisoned* khi nó nhận ra thread đang giữ nó đã panic[^std-poison]. Từ đó mọi thread khác mặc định không truy cập được dữ liệu vì nhiều khả năng nó bị "nhiễm" (một invariant không còn được giữ)[^std-poison-tainted]; hệ quả là `lock`/`try_lock` trả `Result`. Phần lớn code chỉ `unwrap()`, nhằm lan truyền panic giữa các thread để không ai nhìn thấy invariant có thể đã hỏng[^std-poison-unwrap]. Poisoning chỉ mang tính tư vấn: `PoisonError::into_inner` vẫn trả được guard như thể lock thành công[^std-poison-advisory]. Việc phát hiện panic "không lý tưởng", nên ngay cả mutex chưa poisoned cũng cần cẩn thận vì có thể có panic bị bỏ sót[^std-poison-detect].

`RwLock` có quy tắc riêng: chỉ bị poisoned khi panic xảy ra lúc đang khóa ở chế độ ghi; panic trong reader thì không[^std-rwlock-poison].

`Condvar::wait` cũng trả lỗi nếu mutex bị poisoned khi thread giành lại lock[^std-condvar-poison].

**`RwLock<T>`.**

`RwLock` cho nhiều reader đồng thời miễn là không có writer đang giữ[^std-rwlock]. Docs không bảo đảm chính sách ưu tiên — phụ thuộc OS — và writer đang chờ có thể chặn hoặc không chặn `read` mới[^std-rwlock-policy]; docs đưa ví dụ cùng thread gọi `read` hai lần trong khi có writer chờ có thể deadlock[^std-rwlock-deadlock]. Ràng buộc kiểu khác `Mutex`: cần `T: Send` để chia sẻ qua thread **và** `T: Sync` để nhiều reader truy cập đồng thời[^std-rwlock-bounds] — cụ thể `impl<T: Send + Sync> Sync for RwLock<T>`[^std-rwlock-sync].

**`Condvar`.**

`Condvar` cho thread chờ mà không tốn CPU[^std-condvar-cpu]; `wait(guard)` nhả mutex và chặn thread một cách atomic[^std-condvar-atomic]. Phải kiểm tra predicate trong vòng lặp vì spurious wakeup[^std-condvar-spurious] — đúng như [chương 04](./04-condition-variable-va-semaphore.md). Rust đóng gói vòng lặp đó trong `wait_while`, nhận sẵn điều kiện để chờ[^std-condvar-waitwhile]. Dùng nhiều mutex khác nhau cho cùng một condvar có thể gây panic lúc chạy[^std-condvar-mutex].

<details><summary>Đi sâu: kiểm chứng ràng buộc kiểu và poisoning (chạy thật)</summary>

- `Arc<Mutex<Cell<i32>>>` qua thread: **biên dịch và chạy** (in `2`), dù `Cell: !Sync`.
- `Arc<Mutex<Rc<i32>>>` qua thread: bị từ chối vì `Rc: !Send`:

```text
error[E0277]: `Rc<i32>` cannot be sent between threads safely
  = help: the trait `Send` is not implemented for `Rc<i32>`
  = note: required for `std::sync::Mutex<Rc<i32>>` to implement `Sync`
  = note: required for `Arc<std::sync::Mutex<Rc<i32>>>` to implement `Send`
```

- `Arc<RwLock<Cell<i32>>>` bị từ chối (`Cell<i32>` không `Sync`), ngược với `Mutex<Cell<i32>>` — vì nhiều reader song song có thể cùng gọi `Cell::set`.
- Poisoning:

```text
mutex poisoned = true
into_inner = 0
rwlock poisoned after reader panic = false
rwlock poisoned after writer panic = true
```

</details>

**Quay lại Q5:** `Mutex` chỉ cần `T: Send` vì `&T` chỉ được đưa cho một thread tại một thời điểm, `RwLock` cần thêm `Sync` vì có nhiều reader đồng thời, và poisoning đánh dấu dữ liệu có thể hỏng sau panic.

## 7. `OnceLock` và `LazyLock`: khởi tạo một lần, an toàn qua thread (Q6)

> **Trả lời ngắn.** `OnceLock<T>` là ô chỉ ghi được một lần[^std-oncelock]; `LazyLock<T>` khởi tạo giá trị ở lần truy cập đầu tiên[^std-lazylock]. Chúng giải bài toán giá trị global khởi tạo lười đúng một lần, an toàn khi nhiều thread chạm vào lần đầu cùng lúc: `get_or_init` bảo đảm chỉ một hàm khởi tạo được thực thi[^std-oncelock-once].

**Giải thích.**

Hình dung (nhận xét của người soạn): như cấu hình đọc từ file khi request đầu tiên tới — người đến sau phải chờ người đến trước nạp xong, rồi mọi người dùng chung kết quả.


Bài toán (nhận xét của người soạn): một giá trị global (config, bảng tra, regex đã compile) cần khởi tạo lười, đúng một lần, an toàn khi nhiều thread chạm vào lần đầu cùng lúc. `static mut` cần `unsafe` để truy cập, còn `Mutex<Option<T>>` trả phí lock ở mọi lần đọc. Std 1.95.0 có hai kiểu:

- **`OnceLock<T>`**: "primitive đồng bộ mà nominally chỉ ghi được một lần"[^std-oncelock]. `get` trả `None` nếu chưa khởi tạo; `set` trả `Ok(())` nếu cell chưa khởi tạo, `Err(value)` nếu đã có[^std-oncelock-set]. `get_or_init`: nhiều thread có thể gọi đồng thời với các hàm khởi tạo khác nhau, nhưng bảo đảm chỉ một hàm được thực thi (nếu hàm không panic)[^std-oncelock-once].
- **`LazyLock<T>`**: giá trị được khởi tạo ở lần truy cập đầu tiên[^std-lazylock]; vì có thể bị gọi từ nhiều thread, mọi lời gọi dereference sẽ block nếu một routine khởi tạo khác đang chạy[^std-lazylock-block]. Nếu closure khởi tạo panic thì lock bị poisoned[^std-lazylock-poison] — và khác `Mutex`, poisoning ở đây không phục hồi được: mọi lần truy cập sau đều panic[^std-lazylock-unrecoverable].

<details><summary>Đi sâu: kết quả chạy thật</summary>

Chạy thật:

```text
get before = None
first
first            <- get_or_init lần hai KHÔNG chạy closure "second"
set again = Err("third")
before use
init table       <- LazyLock chỉ chạy closure ở lần deref đầu
[1, 2, 3] 3
```

</details>

**Quay lại Q6:** `OnceLock` ghi một lần, `LazyLock` lười ở lần deref đầu; cả hai đều bảo đảm khởi tạo an toàn khi nhiều thread chạm vào lần đầu (nhận xét của người soạn: nhờ vậy không cần `static mut` hay lock ở mọi lần đọc).

## 8. Atomics trong Rust (nhắc ngắn)

Kiểu atomic an toàn để chia sẻ giữa thread (cài `Sync`) nhưng không tự cung cấp cơ chế chia sẻ — thường đặt trong `Arc` hoặc `static`[^std-atomic-sync]. Docs định nghĩa data race là truy cập xung đột không đồng bộ mà ít nhất một bên là non-atomic, và nói data race là UB[^std-datarace]; do đó truy cập atomic không thuộc định nghĩa này (suy luận từ định nghĩa). Docs còn nêu nguồn UB khác của memory model: các truy cập atomic xung đột, không đồng bộ, chồng lấn một phần nhau (mixed-size)[^std-mixed-size]. Cách chọn `Ordering`, `happens-before`, `compare_exchange` nằm ở [chương 06](./06-memory-model.md) và [chương 07](./07-lock-free.md); điều cần mang theo từ chương này là: `Mutex` cho bạn đồng bộ "tặng kèm" (unlock→lock tạo happens-before[^atomics-hb-cases]), còn atomic bạn phải tự chọn ordering.

## 9. Channel `std::sync::mpsc` (Q7)

> **Trả lời ngắn.** `mpsc` là hàng đợi FIFO nhiều producer, một consumer[^std-mpsc]. `channel()` không giới hạn nên `send` không bao giờ block[^std-mpsc-async]; `sync_channel(n)` có bộ đệm cố định, `send` block đến khi có chỗ[^std-mpsc-sync], và `n = 0` là rendezvous[^std-mpsc-rendezvous]. Bên trong là kênh MPMC port từ crossbeam-channel[^std-mpsc-wrap].

**Giải thích.**

Hình dung (nhận xét của người soạn): `channel()` là hộp thư không đáy; `sync_channel(n)` là hộp thư chỉ chứa n lá, đầy thì người gửi phải đợi; `n = 0` là trao tận tay.


Triết lý (câu khẩu hiệu trong Book): đừng giao tiếp bằng cách chia sẻ bộ nhớ; hãy chia sẻ bộ nhớ bằng cách giao tiếp[^book-msg]. `mpsc` là hàng đợi FIFO multi-producer, single-consumer[^std-mpsc]; cả hai loại sender đều clone được để nhiều thread gửi đồng thời tới một receiver[^std-mpsc-clone]. Hai dạng:

| | `channel()` | `sync_channel(n)` |
|---|---|---|
| Bộ đệm | không giới hạn, `send` không bao giờ block[^std-mpsc-async] | cố định; `send` block đến khi có chỗ[^std-mpsc-sync] |
| `n = 0` | — | rendezvous: `send` không trả về cho đến khi có `recv` ghép cặp[^std-mpsc-rendezvous] |

Thao tác thất bại thường nghĩa là nửa còn lại đã "hung up" do bị drop[^std-mpsc-hungup].

**Kiểu phản ánh "single-consumer".** Source 1.95.0 khai báo `impl<T> !Sync for Receiver<T>`[^std-mpsc-recv-nosync] và `unsafe impl<T: Send> Sync for Sender<T>`[^std-mpsc-sender-sync]. Suy luận của người soạn từ định nghĩa `Sync ⇔ &T: Send` (mục 3): vì `Receiver` là `!Sync` nên không thể gửi `&Receiver` sang thread khác, tức không thể cho nhiều thread cùng nhận qua một tham chiếu chung.

<details><summary>Đi sâu: kết quả chạy thật và cài đặt bên trong</summary>

Kết quả chạy thật:

```text
[0, 10, 20]                                  # 3 sender clone, drop(tx) gốc, rx.iter() tự kết thúc
try_send 1: Ok(())                           # sync_channel(1)
try_send 2: Err(TrySendError::Full(..))      # đầy
send after rx drop: Err(SendError { .. })    # receiver đã drop
rendezvous try_send without receiver: Err(TrySendError::Full(..))
recv after all senders dropped: Err(RecvError)
```

**Bên trong (chi tiết cài đặt, có thể đổi giữa các phiên bản).** Source 1.95.0: `mpsc` là lớp bọc của kênh MPMC được port từ crossbeam-channel[^std-mpsc-wrap]; `mpsc::sync_channel` gọi `mpmc::sync_channel(bound)`[^std-mpsc-calls-mpmc]. Hàm tạo MPMC chọn một trong ba "flavor": `channel()` dùng `list::Channel`[^std-mpmc-pick-list] — danh sách liên kết không giới hạn[^std-mpmc-list]; `sync_channel(n>0)` dùng `array::Channel`[^std-mpmc-pick-array] — mảng cấp phát sẵn, dựa trên hàng đợi MPMC có giới hạn của Dmitry Vyukov[^std-mpmc-array][^std-mpmc-vyukov]; `sync_channel(0)` dùng `zero::Channel`[^std-mpmc-pick-zero] — kênh rendezvous[^std-mpmc-zero]. Tức là ba cài đặt riêng biệt, không phải một khuôn "mutex + condvar + buffer" chung (suy luận từ ba flavor). Với `sync_channel`, bộ đệm có giới hạn chính là bounded buffer của bài producer/consumer ([chương 04](./04-condition-variable-va-semaphore.md)); trong backend, nó là cơ chế *backpressure* tự nhiên: producer nhanh hơn consumer thì bị chặn thay vì phình bộ nhớ (nhận xét của người soạn). `channel()` không giới hạn thì không có tính chất đó.

</details>

**Quay lại Q7:** `channel()` không giới hạn, `sync_channel(n)` có giới hạn và `n = 0` là rendezvous; `Receiver` là `!Sync` theo source (suy luận của người soạn: không chia sẻ `&Receiver` được giữa các thread).

## 10. Rust chặn gì và không chặn gì (Q8)

> **Trả lời ngắn.** Safe Rust bảo đảm không có data race[^nomicon-race-def]. Nhưng nó không chặn race condition tổng quát[^nomicon-race-cond], và deadlock vẫn được xem là "safe"[^nomicon-race-deadlock]; muốn tránh chúng thì bạn phải tự thiết kế đúng.

**Giải thích.**

Hình dung (nhận xét của người soạn): compiler như thanh tra an toàn lao động kiểm tra dây điện không hở, nhưng không kiểm tra quy trình làm việc của bạn có hợp lý hay không.


Safe Rust bảo đảm không có data race[^nomicon-race-def]; Nomicon nói data race phần lớn bị chặn chỉ bằng ownership[^nomicon-race-ownership]. Nhưng Rust **không** chặn race condition tổng quát[^nomicon-race-cond]: Nomicon nói việc đó là bất khả về mặt toán học khi bạn không kiểm soát scheduler, như trong môi trường OS thông thường[^nomicon-race-impossible]; kiểm soát được preemption thì có thể chặn (framework như RTIC), nhưng đó là trường hợp rất hiếm[^nomicon-race-rtic]. Vì thế deadlock hay hành vi vô nghĩa do đồng bộ sai vẫn được xem là "safe"[^nomicon-race-deadlock]; Book nói thẳng `Mutex<T>` đi kèm rủi ro deadlock[^book-deadlock]. Một race condition tự nó không phá memory safety — chỉ khi kết hợp với code `unsafe` khác[^nomicon-race-safe].

Sơ đồ dưới tóm tắt các chú thích ở trên (nhận xét của người soạn):

```mermaid
flowchart TD
  X["Lỗi concurrency"] --> D["Data race<br/>(UB)"]
  X --> RC["Race condition logic<br/>(atomicity / order violation)"]
  X --> DL["Deadlock"]
  D -- "Send/Sync + ownership" --> OK["biên dịch từ chối"]
  RC -- "KHÔNG chặn" --> BAD["bạn tự thiết kế đúng"]
  DL -- "KHÔNG chặn" --> BAD
```

<details><summary>Đi sâu: hai ví dụ chạy thật trong safe Rust</summary>

Hai ví dụ chạy thật, hoàn toàn safe Rust:

**Check-then-act (atomicity violation, [chương 05](./05-concurrency-bug-va-deadlock.md)).** Bốn thread chạy `if *m.lock().unwrap() > 0 { *m.lock().unwrap() -= 1; }` trên `Mutex` khởi tạo bằng `1`, hai lần `lock` tách rời (có `yield_now` giữa hai lần để dễ lộ bug). Biên dịch không cảnh báo; trong lần chạy của người soạn kết quả cuối là `final = -1`. Mỗi thao tác riêng lẻ đều được lock bảo vệ — không có data race — nhưng khoảng giữa hai lần lock là khe hở logic. (Đây là một lần quan sát, kết quả phụ thuộc scheduler; cách sửa là giữ một guard cho cả check và act.)

**Deadlock thứ tự lock ([chương 05](./05-concurrency-bug-va-deadlock.md)).** Thread A giữ `a` rồi xin `b`; thread chính giữ `b` rồi xin `a`, mỗi bên ngủ 100 ms giữa hai lần lock. Biên dịch được; chạy thật bị treo cho tới khi `timeout 3` kill (exit 124). Type system không thấy lỗi này; phòng bằng quy ước thứ tự lock, hoặc giữ lock ngắn, hoặc dùng `try_lock` có chiến lược lùi (nhận xét của người soạn).

</details>

**Quay lại Q8:** Rust loại data race ở thời điểm biên dịch nhờ ownership và `Send`/`Sync`, còn atomicity violation và deadlock vẫn là trách nhiệm thiết kế của bạn.

## Tóm tắt

- `spawn` đòi `F: Send + 'static`: `'static` vì thread có thể sống lâu hơn nơi tạo ra nó; `Send` vì closure và kết quả đi qua ranh giới thread. `thread::scope` thay `'static` bằng `'scope` nhờ bảo đảm join cuối scope.
- Drop `JoinHandle` là detach; panic của thread con hiện ra ở `join()` dưới dạng `Err`.
- `T: Sync` ⇔ `&T: Send`. Cả hai là `unsafe auto trait`: tự suy từ thành phần; `unsafe impl` là lời hứa mà compiler không kiểm.
- `Rc` không `Send`/`Sync` (refcount không atomic); `Arc<T>` chỉ `Send + Sync` khi `T` cũng vậy — nên `Arc<Cell<i32>>` bị từ chối với `E0277`.
- `Cell`/`RefCell` là `!Sync` vì mutate qua `&self` không đồng bộ; `Mutex<T>` cần `T: Send` chứ không cần `Sync` vì `&T` chỉ được đưa cho một thread tại một thời điểm; `RwLock<T>` cần cả hai.
- `MutexGuard` là RAII và `!Send`; poisoning lan truyền panic, chỉ mang tính tư vấn; `RwLock` chỉ poisoned khi panic ở chế độ ghi.
- `OnceLock` (ghi một lần, `set`/`get_or_init`) và `LazyLock` (lười ở lần deref đầu) khởi tạo global an toàn qua thread.
- `mpsc`: `channel` không giới hạn, `sync_channel(n)` có giới hạn, `n = 0` là rendezvous; `Receiver` là `!Sync`; bên trong là MPMC port từ crossbeam.
- Safe Rust chặn data race, không chặn race condition logic và deadlock.

## Câu hỏi tự kiểm tra

1. Hai vế `Send` và `'static` của `spawn` mỗi vế chặn loại lỗi nào, và mã lỗi biên dịch tương ứng?
<details><summary>Đáp án</summary>

`'static`: thread có thể sống lâu hơn nơi tạo ra nó nên không được giữ tham chiếu tới dữ liệu cục bộ[^std-spawn-static] (chạy thật: `E0373`). `Send`: closure và giá trị trả về đi qua ranh giới thread[^std-spawn-send] (chạy thật: `E0277`, ví dụ với `Rc`).

</details>

2. Định nghĩa `Sync` qua `Send` thế nào? Dùng nó giải thích dòng `required for &Rc<i32> to implement Send` khi mượn `Rc` trong `thread::scope`.
<details><summary>Đáp án</summary>

`T: Sync` ⇔ `&T: Send`[^std-sync]. Closure mượn `Rc` chứa `&Rc<i32>`, đòi `&Rc<i32>: Send` ⇔ `Rc<i32>: Sync`; `Rc` không `Sync` vì từ `&Rc` vẫn clone được và sửa bộ đếm không atomic[^std-sync-rc].

</details>

3. Vì sao `impl Send`/`impl Sync` thủ công phải là `unsafe`?
<details><summary>Đáp án</summary>

Code unsafe khác được quyền giả định chúng đúng, và cài sai có thể gây UB[^nomicon-unsafe][^nomicon-send-ub]; compiler không kiểm lời hứa của `unsafe impl`.

</details>

4. Vì sao `Arc<Cell<i32>>` không qua được `thread::spawn` dù `Arc` là thread-safe?
<details><summary>Đáp án</summary>

`Arc<T>` chỉ `Send + Sync` khi `T` cũng vậy[^std-arc-bound]; `Cell: !Sync` vì `set(&self)` không đồng bộ[^std-sync-cell]. Chạy thật: `E0277` "`Cell<i32>` cannot be shared between threads safely".

</details>

5. Vì sao `Mutex<T>` chỉ cần `T: Send` còn `RwLock<T>` cần `T: Send + Sync`?
<details><summary>Đáp án</summary>

Mutex đưa `&T` cho một thread tại một thời điểm[^std-mutex-notsync]; `RwLock` cho nhiều reader đồng thời nên cần `Sync`[^std-rwlock-bounds]. Chạy thật: `Arc<Mutex<Cell<i32>>>` được, `Arc<RwLock<Cell<i32>>>` bị từ chối.

</details>

6. `MutexGuard` có `Send` không? Vì sao?
<details><summary>Đáp án</summary>

Không[^nomicon-mutexguard]; gửi guard sang thread khác thì destructor (nhả lock) chạy ở thread nhận, vi phạm yêu cầu của implementation[^nomicon-mutexguard-why].

</details>

7. Khi nào `Mutex` bị poisoned, khi nào `RwLock`? Lấy lại dữ liệu được không?
<details><summary>Đáp án</summary>

Mutex: khi thread đang giữ nó panic[^std-poison]. RwLock: chỉ khi panic lúc khóa chế độ ghi[^std-rwlock-poison]. Lấy lại được bằng `PoisonError::into_inner`[^std-poison-advisory].

</details>

8. `sync_channel(0)` khác `channel()` thế nào, và vì sao `Receiver` là `!Sync`?
<details><summary>Đáp án</summary>

`channel()` không giới hạn, `send` không block[^std-mpsc-async]; `sync_channel(0)` là rendezvous, `send` chờ `recv` ghép cặp[^std-mpsc-rendezvous]. `Receiver` là `!Sync` theo source[^std-mpsc-recv-nosync]; theo định nghĩa `Sync ⇔ &T: Send`, không thể gửi `&Receiver` sang thread khác (suy luận của người soạn).

</details>

9. Safe Rust có thể có deadlock hay atomicity violation không? Vì sao compiler không bắt?
<details><summary>Đáp án</summary>

Có. Rust chỉ bảo đảm không data race[^nomicon-race-def]; race condition tổng quát không thể chặn khi không kiểm soát scheduler[^nomicon-race-impossible]; deadlock vẫn được xem là "safe"[^nomicon-race-deadlock]. Chạy thật: check-then-act cho `final = -1`, deadlock thứ tự lock bị treo.

</details>

## Đọc thêm

- [Nomicon — Send and Sync](https://doc.rust-lang.org/1.95.0/nomicon/send-and-sync.html): ví dụ `Carton<T>` cho thấy khi nào `unsafe impl` đúng.
- [Nomicon — Data Races and Race Conditions](https://doc.rust-lang.org/1.95.0/nomicon/races.html): ranh giới chính xác giữa cái Rust bảo đảm và không bảo đảm.
- [Book ch.16](https://doc.rust-lang.org/1.95.0/book/ch16-00-concurrency.html): thread, message passing, shared state, `Send`/`Sync` ở mức giới thiệu.
- [docs `std::sync::Mutex`](https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html): mục Poisoning và các ví dụ giữ/nhả guard.
- [Rust Atomics and Locks, ch.1](https://marabos.nl/atomics/basics.html): `Rc`/`Arc`, `scope`, interior mutability, `Send`/`Sync` từ góc nhìn tác giả thư viện.
- [Chương 06 — Memory model](./06-memory-model.md) và [Chương 07 — Lock-free](./07-lock-free.md): `Ordering`, happens-before, CAS.

[^nomicon-race-def]: The Rustonomicon, Data Races and Race Conditions — <https://doc.rust-lang.org/1.95.0/nomicon/races.html> — "Safe Rust guarantees an absence of data races, which are defined as: two or more threads concurrently accessing a location of memory one or more of them is a write one or more of them is unsynchronized"
[^nomicon-race-ownership]: The Rustonomicon, Data Races and Race Conditions — <https://doc.rust-lang.org/1.95.0/nomicon/races.html> — "Data races are prevented mostly through Rust’s ownership system alone: it’s impossible to alias a mutable reference, so it’s impossible to perform a data race. Interior mutability makes this more complicated, which is largely why we have the Send and Sync traits"
[^book-11]: The Rust Programming Language, 16.1 Using Threads — <https://doc.rust-lang.org/1.95.0/book/ch16-01-threads.html> — "The Rust standard library uses a 1:1 model of thread implementation, whereby a program uses one operating system thread per one language thread."
[^std-spawn-sig]: Rust std 1.95.0, `std::thread::spawn` — <https://doc.rust-lang.org/1.95.0/std/thread/fn.spawn.html> — "F: FnOnce() -> T + Send + 'static, T: Send + 'static,"
[^std-spawn-static]: Rust std 1.95.0, `std::thread::spawn` — <https://doc.rust-lang.org/1.95.0/std/thread/fn.spawn.html> — "The 'static constraint means that the closure and its return value must have a lifetime of the whole program execution. The reason for this is that threads can outlive the lifetime they have been created in."
[^std-spawn-send]: Rust std 1.95.0, `std::thread::spawn` — <https://doc.rust-lang.org/1.95.0/std/thread/fn.spawn.html> — "The Send constraint is because the closure will need to be passed by value from the thread where it is spawned to the new thread. Its return value will need to be passed from the new thread to the thread where it is joined."
[^book-move]: The Rust Programming Language, 16.1 Using Threads — <https://doc.rust-lang.org/1.95.0/book/ch16-01-threads.html> — "the closure will then take ownership of the values it uses from the environment, thus transferring ownership of those values from one thread to another."
[^book-spawn-join]: The Rust Programming Language, 16.1 Using Threads — <https://doc.rust-lang.org/1.95.0/book/ch16-01-threads.html> — "A JoinHandle\<T> is an owned value that, when we call the join method on it, will wait for its thread to finish."
[^std-spawn-detach]: Rust std 1.95.0, `std::thread::spawn` — <https://doc.rust-lang.org/1.95.0/std/thread/fn.spawn.html> — "If the join handle is dropped, the spawned thread will implicitly be detached."
[^std-spawn-panic]: Rust std 1.95.0, `std::thread::spawn` — <https://doc.rust-lang.org/1.95.0/std/thread/fn.spawn.html> — "If the spawned thread panics, join will return an Err containing the argument given to panic!."
[^std-scope]: Rust std 1.95.0, `std::thread::scope` — <https://doc.rust-lang.org/1.95.0/std/thread/fn.scope.html> — "Unlike non-scoped threads, scoped threads can borrow non-'static data, as the scope guarantees all threads will be joined at the end of the scope."
[^std-scope-auto]: Rust std 1.95.0, `std::thread::scope` — <https://doc.rust-lang.org/1.95.0/std/thread/fn.scope.html> — "All threads spawned within the scope that haven’t been manually joined will be automatically joined before this function returns."
[^std-scope-spawn]: Rust std 1.95.0, `std::thread::Scope` — <https://doc.rust-lang.org/1.95.0/std/thread/struct.Scope.html> — "F: FnOnce() -> T + Send + 'scope,"
[^std-scope-panic]: Rust std 1.95.0, `std::thread::scope` — <https://doc.rust-lang.org/1.95.0/std/thread/fn.scope.html> — "If any of the automatically joined threads panicked, this function will panic."
[^book-send]: The Rust Programming Language, 16.4 Extensible Concurrency with Sync and Send — <https://doc.rust-lang.org/1.95.0/book/ch16-04-extensible-concurrency-sync-and-send.html> — "The Send marker trait indicates that ownership of values of the type implementing Send can be transferred between threads."
[^nomicon-send-def]: The Rustonomicon, Send and Sync — <https://doc.rust-lang.org/1.95.0/nomicon/send-and-sync.html> — "A type is Send if it is safe to send it to another thread. A type is Sync if it is safe to share between threads (T is Sync if and only if &T is Send)."
[^std-sync]: Rust std 1.95.0, `std::marker::Sync` — <https://doc.rust-lang.org/1.95.0/std/marker/trait.Sync.html> — "The precise definition is: a type T is Sync if and only if &T is Send."
[^std-sync-ref]: Rust std 1.95.0, `std::marker::Sync` — <https://doc.rust-lang.org/1.95.0/std/marker/trait.Sync.html> — "&T is Send if and only if T is Sync &mut T is Send if and only if T is Send &T and &mut T are Sync if and only if T is Sync"
[^std-send-auto]: Rust std 1.95.0, `std::marker::Send` — <https://doc.rust-lang.org/1.95.0/std/marker/trait.Send.html> — "pub unsafe auto trait Send { }"
[^nomicon-auto]: The Rustonomicon, Send and Sync — <https://doc.rust-lang.org/1.95.0/nomicon/send-and-sync.html> — "if a type is composed entirely of Send or Sync types, then it is Send or Sync."
[^book-sendsync-manual]: The Rust Programming Language, 16.4 Extensible Concurrency with Sync and Send — <https://doc.rust-lang.org/1.95.0/book/ch16-04-extensible-concurrency-sync-and-send.html> — "As marker traits, they don’t even have any methods to implement."
[^std-send-auto2]: Rust std 1.95.0, `std::marker::Send` — <https://doc.rust-lang.org/1.95.0/std/marker/trait.Send.html> — "This trait is automatically implemented when the compiler determines it’s appropriate."
[^nomicon-unsafe]: The Rustonomicon, Send and Sync — <https://doc.rust-lang.org/1.95.0/nomicon/send-and-sync.html> — "they're unsafe traits. This means that they are unsafe to implement, and other unsafe code can assume that they are correctly implemented."
[^nomicon-send-ub]: The Rustonomicon, Send and Sync — <https://doc.rust-lang.org/1.95.0/nomicon/send-and-sync.html> — "Incorrectly implementing Send or Sync can cause Undefined Behavior."
[^nomicon-exceptions]: The Rustonomicon, Send and Sync — <https://doc.rust-lang.org/1.95.0/nomicon/send-and-sync.html> — "raw pointers are neither Send nor Sync (because they have no safety guards). UnsafeCell isn't Sync (and therefore Cell and RefCell aren't)."
[^nomicon-rawptr-lint]: The Rustonomicon, Send and Sync — <https://doc.rust-lang.org/1.95.0/nomicon/send-and-sync.html> — "However raw pointers are, strictly speaking, marked as thread-unsafe as more of a lint."
[^nomicon-mybox]: The Rustonomicon, Send and Sync — <https://doc.rust-lang.org/1.95.0/nomicon/send-and-sync.html> — "Types that aren’t automatically derived can simply implement them if desired:"
[^nomicon-mutexguard]: The Rustonomicon, Send and Sync — <https://doc.rust-lang.org/1.95.0/nomicon/send-and-sync.html> — "A nice example where this does not happen is with a MutexGuard: notice how it is not Send."
[^nomicon-mutexguard-why]: The Rustonomicon, Send and Sync — <https://doc.rust-lang.org/1.95.0/nomicon/send-and-sync.html> — "don’t try to free a lock that you acquired in a different thread. If you were able to Send a MutexGuard to another thread the destructor would run in the thread you sent it to, violating the requirement."
[^std-send-rc]: Rust std 1.95.0, `std::marker::Send` — <https://doc.rust-lang.org/1.95.0/std/marker/trait.Send.html> — "An example of a non-Send type is the reference-counting pointer rc::Rc. If two threads attempt to clone Rcs that point to the same reference-counted value, they might try to update the reference count at the same time"
[^nomicon-rc]: The Rustonomicon, Send and Sync — <https://doc.rust-lang.org/1.95.0/nomicon/send-and-sync.html> — "Rc isn't Send or Sync (because the refcount is shared and unsynchronized)."
[^std-sync-rc]: Rust std 1.95.0, `std::marker::Sync` — <https://doc.rust-lang.org/1.95.0/std/marker/trait.Sync.html> — "Given any reference &Rc\<T>, you can clone a new Rc\<T>, modifying the reference counts in a non-atomic way."
[^std-arc]: Rust std 1.95.0, `std::sync::Arc` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Arc.html> — "Unlike Rc\<T>, Arc\<T> uses atomic operations for its reference counting. This means that it is thread-safe."
[^book-arc]: The Rust Programming Language, 16.3 Shared-State Concurrency — <https://doc.rust-lang.org/1.95.0/book/ch16-03-shared-state.html> — "Arc\<T> is a type like Rc\<T> that is safe to use in concurrent situations."
[^atomics-arc]: Rust Atomics and Locks, ch.1 Basics of Rust Concurrency — <https://marabos.nl/atomics/basics.html> — "It’s identical to Rc, except it guarantees that modifications to the reference counter are indivisible atomic operations, making it safe to use it with multiple threads."
[^std-arc-bound]: Rust std 1.95.0, `std::sync::Arc` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Arc.html> — "Arc\<T> will implement Send and Sync as long as the T implements Send and Sync."
[^std-cell-rule]: Rust std 1.95.0, module `std::cell` — <https://doc.rust-lang.org/1.95.0/std/cell/index.html> — "Rust memory safety is based on this rule: Given an object T, it is only possible to have one of the following: Several immutable references (&T) to the object (also known as aliasing). One mutable reference (&mut T) to the object (also known as mutability)."
[^std-cell-interior]: Rust std 1.95.0, module `std::cell` — <https://doc.rust-lang.org/1.95.0/std/cell/index.html> — "We say these cell types provide ‘interior mutability’ (mutable via &T), in contrast with typical Rust types that exhibit ‘inherited mutability’ (mutable only via &mut T)."
[^std-unsafecell-core]: Rust std 1.95.0, `std::cell::UnsafeCell` — <https://doc.rust-lang.org/1.95.0/std/cell/struct.UnsafeCell.html> — "The core primitive for interior mutability in Rust."
[^std-unsafecell]: Rust std 1.95.0, `std::cell::UnsafeCell` — <https://doc.rust-lang.org/1.95.0/std/cell/struct.UnsafeCell.html> — "UnsafeCell\<T> opts-out of the immutability guarantee for &T: a shared reference &UnsafeCell\<T> may point to data that is being mutated. This is called “interior mutability”. All other types that allow internal mutability, such as Cell\<T> and RefCell\<T>, internally use UnsafeCell to wrap their data."
[^std-sync-unsafecell]: Rust std 1.95.0, `std::marker::Sync` — <https://doc.rust-lang.org/1.95.0/std/marker/trait.Sync.html> — "Any types with interior mutability must also use the cell::UnsafeCell wrapper around the value(s) which can be mutated through a shared reference. Failing to doing this is undefined behavior."
[^std-cell-cell]: Rust std 1.95.0, module `std::cell` — <https://doc.rust-lang.org/1.95.0/std/cell/index.html> — "Cell\<T> implements interior mutability by moving values in and out of the cell. That is, a &T to the inner value can never be obtained"
[^std-sync-cell]: Rust std 1.95.0, `std::marker::Sync` — <https://doc.rust-lang.org/1.95.0/std/marker/trait.Sync.html> — "The method performs no synchronization, thus Cell cannot be Sync."
[^std-refcell]: Rust std 1.95.0, module `std::cell` — <https://doc.rust-lang.org/1.95.0/std/cell/index.html> — "RefCell\<T> uses Rust’s lifetimes to implement “dynamic borrowing”, a process whereby one can claim temporary, exclusive, mutable access to the inner value. Borrows for RefCell\<T>s are tracked at runtime, unlike Rust’s native reference types which are entirely tracked statically, at compile time."
[^book-interior]: The Rust Programming Language, 16.3 Shared-State Concurrency — <https://doc.rust-lang.org/1.95.0/book/ch16-03-shared-state.html> — "this means Mutex\<T> provides interior mutability, as the Cell family does."
[^std-mutex-sync]: Rust std 1.95.0, `std::sync::Mutex` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html> — "T must be Send for Mutex to be Sync. This ensures that the protected data can be accessed safely from multiple threads without causing data races or other unsafe behavior."
[^std-atomic-sync]: Rust std 1.95.0, module `std::sync::atomic` — <https://doc.rust-lang.org/1.95.0/std/sync/atomic/index.html> — "Atomic variables are safe to share between threads (they implement Sync) but they do not themselves provide the mechanism for sharing and follow the threading model of Rust."
[^std-mutex-guard]: Rust std 1.95.0, `std::sync::Mutex` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html> — "The data can only be accessed through the RAII guards returned from lock and try_lock, which guarantees that the data is only ever accessed when the mutex is locked."
[^book-guard]: The Rust Programming Language, 16.3 Shared-State Concurrency — <https://doc.rust-lang.org/1.95.0/book/ch16-03-shared-state.html> — "The MutexGuard type implements Deref to point at our inner data; the type also has a Drop implementation that releases the lock automatically when a MutexGuard goes out of scope"
[^std-mutex-drop]: Rust std 1.95.0, `std::sync::Mutex` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html> — "To unlock a mutex guard sooner than the end of the enclosing scope, either create an inner scope or drop the guard manually."
[^atomics-hb-cases]: Rust Atomics and Locks, ch.3 Memory Ordering — <https://marabos.nl/atomics/memory-ordering.html> — "Between threads, however, happens-before relationships only occur in a few specific cases, such as when spawning and joining a thread, unlocking and locking a mutex, and through atomic operations that use non-relaxed memory ordering."
[^std-mutex-twice]: Rust std 1.95.0, `std::sync::Mutex::lock` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html> — "The exact behavior on locking a mutex in the thread which already holds the lock is left unspecified. However, this function will not return on the second call (it might panic or deadlock, for example)."
[^std-mutex-notsync]: Rust std 1.95.0, `std::sync::Mutex` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html> — "Also note that it is not necessary for T to be Sync as &T is only made available to one thread at a time if T is not Sync."
[^std-mutex-send]: Rust std 1.95.0, `std::sync::Mutex` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html> — "impl\<T: ?Sized + Send> Send for Mutex\<T>"
[^std-poison]: Rust std 1.95.0, `std::sync::Mutex` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html> — "a mutex becomes poisoned if it recognizes that the thread holding it has panicked."
[^std-poison-tainted]: Rust std 1.95.0, `std::sync::Mutex` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html> — "Once a mutex is poisoned, all other threads are unable to access the data by default as it is likely tainted (some invariant is not being upheld)."
[^std-poison-unwrap]: Rust std 1.95.0, `std::sync::Mutex` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html> — "Most usage of a mutex will simply unwrap() these results, propagating panics among threads to ensure that a possibly invalid invariant is not witnessed."
[^std-poison-advisory]: Rust std 1.95.0, `std::sync::Mutex` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html> — "Poisoning is only advisory: the PoisonError type has an into_inner method which will return the guard that would have otherwise been returned on a successful lock."
[^std-poison-detect]: Rust std 1.95.0, `std::sync::Mutex` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html> — "In addition, the panic detection is not ideal, so even unpoisoned mutexes need to be handled with care, since certain panics may have been skipped."
[^std-rwlock-poison]: Rust std 1.95.0, `std::sync::RwLock` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.RwLock.html> — "an RwLock may only be poisoned if a panic occurs while it is locked exclusively (write mode). If a panic occurs in any reader, then the lock will not be poisoned."
[^std-condvar-poison]: Rust std 1.95.0, `std::sync::Condvar::wait` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Condvar.html> — "This function will return an error if the mutex being waited on is poisoned when this thread re-acquires the lock."
[^std-rwlock]: Rust std 1.95.0, `std::sync::RwLock` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.RwLock.html> — "An RwLock will allow any number of readers to acquire the lock as long as a writer is not holding the lock."
[^std-rwlock-policy]: Rust std 1.95.0, `std::sync::RwLock` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.RwLock.html> — "The priority policy of the lock is dependent on the underlying operating system’s implementation, and this type does not guarantee that any particular policy will be used. In particular, a writer which is waiting to acquire the lock in write might or might not block concurrent calls to read"
[^std-rwlock-deadlock]: Rust std 1.95.0, `std::sync::RwLock` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.RwLock.html> — "let _rg1 = lock.read(); | | // will block | let _wg = lock.write(); // may deadlock | let _rg2 = lock.read();"
[^std-rwlock-bounds]: Rust std 1.95.0, `std::sync::RwLock` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.RwLock.html> — "It is required that T satisfies Send to be shared across threads and Sync to allow concurrent access through readers."
[^std-rwlock-sync]: Rust std 1.95.0, `std::sync::RwLock` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.RwLock.html> — "impl\<T: ?Sized + Send + Sync> Sync for RwLock\<T>"
[^std-condvar-cpu]: Rust std 1.95.0, `std::sync::Condvar` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Condvar.html> — "Condition variables represent the ability to block a thread such that it consumes no CPU time while waiting for an event to occur."
[^std-condvar-atomic]: Rust std 1.95.0, `std::sync::Condvar::wait` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Condvar.html> — "This function will atomically unlock the mutex specified (represented by guard) and block the current thread."
[^std-condvar-spurious]: Rust std 1.95.0, `std::sync::Condvar` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Condvar.html> — "Note that this function is susceptible to spurious wakeups. Condition variables normally have a boolean predicate associated with them, and the predicate must always be checked each time this function returns to protect against spurious wakeups."
[^std-condvar-waitwhile]: Rust std 1.95.0, `std::sync::Condvar::wait_while` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Condvar.html> — "Blocks the current thread until the provided condition becomes false."
[^std-condvar-mutex]: Rust std 1.95.0, `std::sync::Condvar` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Condvar.html> — "any attempt to use multiple mutexes on the same condition variable may result in a runtime panic."
[^std-oncelock]: Rust std 1.95.0, `std::sync::OnceLock` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.OnceLock.html> — "A synchronization primitive which can nominally be written to only once."
[^std-oncelock-set]: Rust std 1.95.0, `std::sync::OnceLock::set` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.OnceLock.html> — "Returns Ok(()) if the cell was uninitialized and Err(value) if the cell was already initialized."
[^std-oncelock-once]: Rust std 1.95.0, `std::sync::OnceLock::get_or_init` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.OnceLock.html> — "Many threads may call get_or_init concurrently with different initializing functions, but it is guaranteed that only one function will be executed if the function doesn’t panic."
[^std-lazylock]: Rust std 1.95.0, `std::sync::LazyLock` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.LazyLock.html> — "A value which is initialized on the first access."
[^std-lazylock-block]: Rust std 1.95.0, `std::sync::LazyLock` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.LazyLock.html> — "Since initialization may be called from multiple threads, any dereferencing call will block the calling thread if another initialization routine is currently running."
[^std-lazylock-poison]: Rust std 1.95.0, `std::sync::LazyLock` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.LazyLock.html> — "If the initialization closure passed to LazyLock::new panics, the lock will be poisoned."
[^std-lazylock-unrecoverable]: Rust std 1.95.0, `std::sync::LazyLock` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.LazyLock.html> — "A key difference, however, is that poisoning in LazyLock is unrecoverable."
[^std-datarace]: Rust std 1.95.0, module `std::sync::atomic` — <https://doc.rust-lang.org/1.95.0/std/sync/atomic/index.html> — "The most important aspect of this model is that data races are undefined behavior. A data race is defined as conflicting non-synchronized accesses where at least one of the accesses is non-atomic."
[^std-mixed-size]: Rust std 1.95.0, module `std::sync::atomic` — <https://doc.rust-lang.org/1.95.0/std/sync/atomic/index.html> — "The other possible cause of undefined behavior in the memory model are mixed-size accesses: Rust inherits the C++ limitation that non-synchronized conflicting atomic accesses may not partially overlap."
[^book-msg]: The Rust Programming Language, 16.2 Using Message Passing — <https://doc.rust-lang.org/1.95.0/book/ch16-02-message-passing.html> — "Do not communicate by sharing memory; instead, share memory by communicating."
[^std-mpsc]: Rust std 1.95.0, module `std::sync::mpsc` — <https://doc.rust-lang.org/1.95.0/std/sync/mpsc/index.html> — "Multi-producer, single-consumer FIFO queue communication primitives."
[^std-mpsc-clone]: Rust std 1.95.0, module `std::sync::mpsc` — <https://doc.rust-lang.org/1.95.0/std/sync/mpsc/index.html> — "Both senders are clone-able (multi-producer) such that many threads can send simultaneously to one receiver (single-consumer)."
[^std-mpsc-async]: Rust std 1.95.0, module `std::sync::mpsc` — <https://doc.rust-lang.org/1.95.0/std/sync/mpsc/index.html> — "An asynchronous, infinitely buffered channel. The channel function will return a (Sender, Receiver) tuple where all sends will be asynchronous (they never block)."
[^std-mpsc-sync]: Rust std 1.95.0, module `std::sync::mpsc` — <https://doc.rust-lang.org/1.95.0/std/sync/mpsc/index.html> — "All sends will be synchronous by blocking until there is buffer space available."
[^std-mpsc-rendezvous]: Rust std 1.95.0, `std::sync::mpsc::sync_channel` — <https://doc.rust-lang.org/1.95.0/std/sync/mpsc/fn.sync_channel.html> — "Note that a buffer size of 0 is valid, in which case this becomes “rendezvous channel” where each send will not return until a recv is paired with it."
[^std-mpsc-hungup]: Rust std 1.95.0, module `std::sync::mpsc` — <https://doc.rust-lang.org/1.95.0/std/sync/mpsc/index.html> — "An unsuccessful operation is normally indicative of the other half of a channel having “hung up” by being dropped in its corresponding thread."
[^std-mpsc-recv-nosync]: rust-lang/rust tag 1.95.0, library/std/src/sync/mpsc.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sync/mpsc.rs> — "impl\<T> !Sync for Receiver\<T> {}"
[^std-mpsc-sender-sync]: rust-lang/rust tag 1.95.0, library/std/src/sync/mpsc.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sync/mpsc.rs> — "unsafe impl\<T: Send> Sync for Sender\<T> {}"
[^std-mpsc-wrap]: rust-lang/rust tag 1.95.0, library/std/src/sync/mpsc.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sync/mpsc.rs> — "MPSC channels are built as a wrapper around MPMC channels, which"
[^std-mpmc-list]: rust-lang/rust tag 1.95.0, library/std/src/sync/mpmc/list.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sync/mpmc/list.rs> — "Unbounded channel implemented as a linked list."
[^std-mpmc-array]: rust-lang/rust tag 1.95.0, library/std/src/sync/mpmc/array.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sync/mpmc/array.rs> — "Bounded channel based on a preallocated array."
[^std-mpmc-zero]: rust-lang/rust tag 1.95.0, library/std/src/sync/mpmc/zero.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sync/mpmc/zero.rs> — "This kind of channel is also known as *rendezvous* channel."
[^nomicon-race-cond]: The Rustonomicon, Data Races and Race Conditions — <https://doc.rust-lang.org/1.95.0/nomicon/races.html> — "However Rust does not prevent general race conditions."
[^nomicon-race-impossible]: The Rustonomicon, Data Races and Race Conditions — <https://doc.rust-lang.org/1.95.0/nomicon/races.html> — "This is mathematically impossible in situations where you do not control the scheduler, which is true for the normal OS environment."
[^nomicon-race-rtic]: The Rustonomicon, Data Races and Race Conditions — <https://doc.rust-lang.org/1.95.0/nomicon/races.html> — "If you do control preemption, it can be possible to prevent general races - this technique is used by frameworks such as RTIC. However, actually having control over scheduling is a very uncommon case."
[^nomicon-race-deadlock]: The Rustonomicon, Data Races and Race Conditions — <https://doc.rust-lang.org/1.95.0/nomicon/races.html> — "it is considered “safe” for Rust to get deadlocked or do something nonsensical with incorrect synchronization"
[^book-deadlock]: The Rust Programming Language, 16.3 Shared-State Concurrency — <https://doc.rust-lang.org/1.95.0/book/ch16-03-shared-state.html> — "Similarly, Mutex\<T> comes with the risk of creating deadlocks."
[^nomicon-race-safe]: The Rustonomicon, Data Races and Race Conditions — <https://doc.rust-lang.org/1.95.0/nomicon/races.html> — "a race condition cannot violate memory safety in a Rust program on its own. Only in conjunction with some other unsafe code can a race condition actually violate memory safety."
[^std-cell-send]: Rust std 1.95.0, `std::cell::Cell` — <https://doc.rust-lang.org/1.95.0/std/cell/struct.Cell.html> — "impl\<T> Send for Cell\<T> where T: Send + ?Sized"
[^std-mpmc-pick-list]: rust-lang/rust tag 1.95.0, library/std/src/sync/mpmc/mod.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sync/mpmc/mod.rs> — "let (s, r) = counter::new(list::Channel::new());"
[^std-mpmc-pick-array]: rust-lang/rust tag 1.95.0, library/std/src/sync/mpmc/mod.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sync/mpmc/mod.rs> — "let (s, r) = counter::new(array::Channel::with_capacity(cap));"
[^std-mpmc-pick-zero]: rust-lang/rust tag 1.95.0, library/std/src/sync/mpmc/mod.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sync/mpmc/mod.rs> — "let (s, r) = counter::new(zero::Channel::new());"
[^std-mpmc-vyukov]: rust-lang/rust tag 1.95.0, library/std/src/sync/mpmc/array.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sync/mpmc/array.rs> — "The implementation is based on Dmitry Vyukov's bounded MPMC queue."
[^std-mpsc-calls-mpmc]: rust-lang/rust tag 1.95.0, library/std/src/sync/mpsc.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sync/mpsc.rs> — "let (tx, rx) = mpmc::sync_channel(bound);"
