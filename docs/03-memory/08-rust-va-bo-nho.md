# P3 · Chương 08 — Rust và bộ nhớ

Bảy chương trước dựng nên nền: address space, page table, heap allocator, stack, alignment. Chương này là chương **thuần Rust**: Rust đặt những thứ đó vào hệ thống kiểu như thế nào, và vì sao nhờ vậy nó không cần garbage collector mà vẫn chặn được phần lớn lỗi bộ nhớ kinh điển của C.

## Bạn sẽ hiểu được gì

1. **Q1.** "Ownership" là gì xét như một cơ chế quản lý tài nguyên, và `move` thực sự làm gì với các bit trong bộ nhớ (có sao chép dữ liệu heap không)? → mục 2
2. **Q2.** Vì sao một kiểu có `Drop` thì không thể là `Copy`, và thứ tự các giá trị bị drop được quy định thế nào? → mục 3
3. **Q3.** `Box`, `Vec`, `String` có bố cục (con trỏ / độ dài / capacity) ra sao; docs *hứa* gì về cách `Vec` tăng capacity và source 1.95.0 *thực tế* làm gì? → mục 4
4. **Q4.** `GlobalAlloc`, `System`, `Layout`, `#[global_allocator]` hợp đồng với nhau ra sao; Allocator API ở trạng thái nào? → mục 5
5. **Q5.** `Rc`/`Arc`/`Weak` giữ bộ đếm ở đâu, vì sao vòng tham chiếu gây leak, và vì sao leak vẫn là *safe*? → mục 6
6. **Q6.** `MaybeUninit` giải quyết bài toán gì mà "cứ để biến chưa khởi tạo" không giải được? → mục 7
7. **Q7.** Từng lỗi bộ nhớ trong C (use-after-free, double free, đọc vùng chưa khởi tạo, tràn buffer, data race) bị Rust chặn bằng cơ chế nào, và lỗi nào *không* bị chặn? → mục 8
8. **Q8.** Khi viết Rust, chọn stack hay heap dựa trên điều gì? → mục 9

## Kiến thức cần trước

- [Chương 01 — Address space, mục 6](01-address-space.md): `malloc`/`free` và các lỗi bộ nhớ kinh điển của C (OSTEP).
- [Chương 05 — Bộ nhớ của tiến trình trên Linux](05-bo-nho-tien-trinh-linux.md): vùng stack/heap/mmap của một process.
- [Chương 06 — Heap allocator](06-heap-allocator.md): `malloc`/`free`, free list, vì sao `free` cần biết kích thước.
- [Chương 07 — Stack, alignment và padding](07-stack-alignment-padding.md): stack frame, guard page, size/align.

Mọi con số "chạy thật" đo bằng `rustc 1.95.0` trên macOS aarch64 (64-bit); chúng là quan sát, không phải hợp đồng ngôn ngữ, trừ khi có chú thích trỏ về docs.

## 1. Bối cảnh: ai chịu trách nhiệm giải phóng?

Quên `free` thì leak[^ostep-leak]; `free` hai lần là double free và kết quả là undefined[^ostep-doublefree]. Ngôn ngữ có garbage collector (GC) giải bài toán bằng một chương trình chạy nền, theo dõi bộ nhớ không còn dùng và dọn nó[^book-gc]. Rust chọn đường thứ ba: bộ nhớ được trả tự động khi biến sở hữu nó ra khỏi scope[^book-auto].

Quyết định "khi nào free" vì thế do compiler đảm nhiệm, với một quy tắc đủ đơn giản để kiểm tra tĩnh: ownership.

## 2. Ownership và move: chuyện gì xảy ra với các bit? (Q1)

> **Trả lời ngắn.** Ownership là ba quy tắc: mỗi giá trị có đúng một owner, và khi owner ra khỏi scope thì giá trị bị drop[^book-rules]. Nhờ vậy không ai free hai lần[^book-doublefree]. Khi `move`, chỉ "cái vỏ" nhỏ nằm trên stack (con trỏ, độ dài, capacity) được chuyển; dữ liệu heap không bị sao chép, còn biến cũ bị vô hiệu hóa[^book-move].

**Giải thích.**

Hình dung (nhận xét của người soạn): giá trị như chìa khóa một căn phòng thuê. Chỉ một người cầm chìa; đưa chìa cho người khác thì mình không còn vào được, và người cầm chìa cuối cùng có trách nhiệm trả phòng.

**Từng bước với một `String`.** (1) `String::from("hello")` xin allocator một buffer heap và dựng "vỏ" ba từ trên stack; biến nhận giá trị là owner duy nhất của buffer đó. (2) Vì owner là duy nhất và biết được ngay lúc biên dịch, compiler biết chính xác chỗ owner ra khỏi scope và chèn lời gọi dọn dẹp (drop) ở đó[^book-drop-auto] (suy luận của người soạn từ ba quy tắc[^book-rules]; chi tiết drop ở mục 3). (3) Với một owner, buffer được free đúng một lần. Các bước dưới đây cho thấy điều gì sẽ hỏng nếu cho phép hai owner.

**Vì sao phải "chỉ một owner".** Nếu hai biến cùng sở hữu một `String` (cùng con trỏ), khi cả hai ra scope cả hai sẽ cố free cùng một vùng; Book gọi đó là double free và xếp vào các lỗi memory-safety[^book-doublefree]. Vì thế sau `let s2 = s1;`, Rust coi `s1` không còn hợp lệ. Book gọi thao tác này là **move** thay vì shallow copy, chính vì biến cũ bị vô hiệu hóa[^book-move].

Chạy thật `let s1 = String::from("a"); let s2 = s1; println!("{s1} {s2}");` → `error[E0382]: borrow of moved value: s1`: Error index mô tả E0382 là biến được dùng sau khi nội dung đã bị chuyển đi nơi khác[^e0382].

```mermaid
flowchart LR
  subgraph stack["Stack"]
    s1["s1: ptr, len=5, cap=5<br/>(vô hiệu sau move)"]
    s2["s2: ptr, len=5, cap=5"]
  end
  subgraph heap["Heap"]
    buf["h e l l o"]
  end
  s1 -. "không còn hợp lệ" .-> buf
  s2 --> buf
```

**Move ở mức bit.** Book mô tả `String` gồm ba phần — con trỏ tới bộ nhớ chứa nội dung, độ dài, và capacity — và nhóm dữ liệu này nằm trên stack[^book-string]. Book mô tả move là việc sao chép con trỏ, độ dài, capacity *mà không sao chép dữ liệu*, rồi vô hiệu hóa biến cũ[^book-move]; tức buffer heap không bị đụng đến.

<details><summary>Đi sâu: Reference và docs <code>Copy</code> nói gì về move; kiểm chứng bằng allocator đếm</summary>

The Rust Reference định nghĩa ở mức ngữ nghĩa, không nói đến memcpy: khi một place expression được dùng ở ngữ cảnh value expression (hoặc bind theo giá trị trong pattern) thì nó biểu thị giá trị đang nằm ở ô nhớ đó[^ref-move-intro]; nếu kiểu implement `Copy` thì giá trị được copy[^ref-move-copy]; ở các trường hợp còn lại, nếu kiểu là `Sized`, có thể move giá trị[^ref-move-sized]. Docs của `Copy` nói rõ hơn về phần cứng: về mặt bit, cả copy lẫn move đều có thể dẫn đến việc các bit được sao chép trong bộ nhớ, dù đôi khi bị tối ưu bỏ đi[^copy-bits]; trong hai ví dụ của docs đó, khác biệt duy nhất mà lập trình viên thấy là sau phép gán có được phép dùng biến cũ hay không.

Kiểm chứng bằng một allocator đếm lời gọi (mục 5): với `let s2 = s1;` trên `String::from("hello")`, số lần `alloc` không đổi (delta = 0), con trỏ heap (`as_ptr()`) giữ nguyên, còn địa chỉ biến `String` trên stack thì **khác** (`&s1 as *const _ != &s2 as *const _`). Tức dữ liệu heap đứng yên, chỉ "cái vỏ" ba từ được chuyển. Quan sát cả với `-O` lẫn không; đừng dựa vào địa chỉ stack cụ thể vì compiler được phép tối ưu.

</details>

**Quay lại Q1:** ownership là "một owner, ra scope thì drop"; move chỉ chuyển vỏ ba từ trên stack, dữ liệu heap đứng yên và biến cũ bị cấm dùng.

## 3. `Copy`, `Drop` và thứ tự drop (Q2)

> **Trả lời ngắn.** Kiểu `Drop` quản lý một tài nguyên ngoài chính các byte của nó, nên sao chép bit sẽ nhân đôi quyền sở hữu; vì vậy kiểu có `Drop` không thể là `Copy`[^copy-doc-drop]. Compiler chèn lời gọi drop tự động khi giá trị ra khỏi scope[^book-drop-auto]; biến bị drop theo thứ tự ngược khai báo[^ref-drop-scope].

**Giải thích.**

Hình dung (nhận xét của người soạn): `Copy` như photo một tờ giấy, thoải mái; còn kết nối DB hay file đang mở thì không photo được — phải có một chỗ duy nhất đóng nó.

**`Copy` loại trừ `Drop`.** Kiểu `Copy` là kiểu có giá trị "nhân bản được chỉ bằng cách sao chép bit"[^copy-def]. Với kiểu đó, gán không làm biến cũ mất hiệu lực. Nhưng nếu bất kỳ phần nào của kiểu implement `Drop` thì kiểu không thể là `Copy`[^book-copy-drop]. Lý do theo docs của `Copy`: mọi kiểu `Drop` đều quản lý một tài nguyên ngoài chính `size_of::<T>` byte của nó, nên sao chép bit sẽ nhân đôi quyền sở hữu tài nguyên[^copy-doc-drop]. Compiler từ chối cả khi bạn cố làm vậy: chạy thật

```rust
#[derive(Clone, Copy)] struct X(u8);
impl Drop for X { fn drop(&mut self) {} }
```

báo `error[E0184]: the trait Copy cannot be implemented for this type; the type has a destructor`, khớp mô tả error index[^e0184].

**`Drop`: destructor do compiler chèn.** Trait `Drop` cho phép tùy biến điều xảy ra khi giá trị sắp ra khỏi scope[^book-drop-trait]. Compiler chèn lời gọi tự động, nên không cần rải code dọn dẹp khắp chương trình[^book-drop-auto]. Bạn không được gọi `x.drop()` thủ công; muốn drop sớm phải dùng `std::mem::drop`[^book-drop-manual].

Một destructor gồm hai phần: gọi `Drop::drop` nếu kiểu có implement, rồi "drop glue" do compiler sinh, gọi đệ quy destructor của mọi field[^drop-two-parts][^drop-glue]. Reference ghi cùng ý ở dạng khác: destructor của `T` là gọi `<T as Drop>::drop` nếu `T: Drop`, rồi chạy đệ quy destructor của các field[^ref-destr-op]. Vì thế bạn hiếm khi phải tự viết `Drop`: struct chứa `Vec` tự động giải phóng buffer của vec.

Tài nguyên ở đây không chỉ là bộ nhớ: docs của `Drop` nêu rõ tài nguyên có thể là bộ nhớ, file descriptor hay network socket[^drop-resources]. Nomicon gọi mô hình này là "ownership-based resource management": tài nguyên lấy khi tạo đối tượng và nhả khi nó bị hủy[^nomicon-orm].

**Thứ tự drop.**

| Đối tượng | Thứ tự drop | Nguồn |
|---|---|---|
| Biến trong một scope | ngược thứ tự khai báo (temporary: ngược thứ tự tạo) | [^ref-drop-scope] (Book: [^book-drop-order]) |
| Field của struct | theo thứ tự khai báo | [^ref-drop-fields] |
| Phần tử của tuple | theo thứ tự | [^ref-drop-tuple] |
| Phần tử của array / owned slice | từ phần tử đầu đến cuối | [^ref-destr-array] |

<details><summary>Đi sâu: ví dụ chạy thật in thứ tự drop, và <code>let _</code></summary>

Chạy thật với kiểu `P(&str)` in tên khi drop:

```rust
let _x = P("x"); let _y = P("y");
let _s = S { a: P("s.a"), b: P("s.b") };
let _t = (P("t0"), P("t1"));
```

Kết quả in ra: `t0 t1 s.a s.b y x`. Đọc từ dưới lên: `_t` khai báo cuối nên bị drop đầu tiên (và bên trong nó `t0` trước `t1`), rồi `_s` (field `a` trước `b`), rồi `_y`, `_x`.

Điểm tinh tế (chạy thật): `let _ = P("underscore");` drop **ngay** (in `drop underscore` trước dòng kế tiếp), còn `let _n = P("named");` sống đến hết scope, vì `_` không bind giá trị vào biến.

</details>

**Quay lại Q2:** `Copy` nhân đôi bit nên không đi với `Drop`; giá trị được drop tự động khi ra scope, biến theo thứ tự ngược khai báo còn field và tuple theo thứ tự xuôi.

## 4. `Box`, `Vec`, `String`: bố cục trong bộ nhớ (Q3)

> **Trả lời ngắn.** `Box<T>` là một con trỏ sở hữu một vùng heap[^box-owns]. `Vec<T>` và `String` là bộ ba (con trỏ, capacity, length) nằm trên stack, trỏ tới buffer ở heap[^vec-triplet][^string-repr]. Docs không hứa chiến lược tăng capacity cụ thể, chỉ hứa `push` O(1) amortized[^vec-growth][^vec-amort]; "gấp đôi" là chi tiết của source 1.95.0.

**Giải thích.**

Hình dung (nhận xét của người soạn): `Vec` như phiếu ghi "kho ở địa chỉ nào, kho rộng bao nhiêu, đã chất bao nhiêu hàng"; phiếu nhỏ cầm trong tay, còn kho nằm ở nơi khác.

| Kiểu | Biểu diễn (theo docs) | Nằm ở đâu |
|---|---|---|
| `Box<T>` | con trỏ sở hữu duy nhất một heap allocation kiểu `T`[^box-owns] | con trỏ ở stack; `T` ở heap; `Box::new` không cấp phát nếu `T` kích thước 0[^box-zst] |
| `Vec<T>` | bộ ba (pointer, capacity, length)[^vec-triplet] | bộ ba nằm nơi `Vec` được đặt; nếu đã cấp phát thì vùng nó trỏ tới nằm trên heap theo allocator mặc định[^vec-heap] |
| `String` | con trỏ tới byte, length, capacity[^string-repr]; sở hữu nội dung trong buffer cấp phát heap[^string-doc] | như `Vec<u8>` |

Đo thật (aarch64, 64-bit): `size_of::<Box<u64>>() = 8`, `Vec<u64> = 24`, `String = 24`. `Box<[u8]>` là 16 byte, và `&str` cũng 16 byte: con trỏ tới kiểu không biết kích thước lúc biên dịch (DST, như `[u8]` hay `str`) hiện có kích thước gấp đôi con trỏ tới kiểu `Sized` (con trỏ + độ dài), đúng như chương DST của Reference mô tả[^dst-fat]. Đây là con số quan sát được chứ không phải lời hứa: mục Type layout của Reference chỉ bảo đảm kích thước "ít nhất bằng" con trỏ thường và dặn không nên dựa vào "gấp đôi" (xem [P10·03, mục 2](../10-rust-internals/03-dst-fat-pointer-vtable.md)).

Con trỏ heap có kích thước cố định nên bản thân nó lưu trên stack được, còn dữ liệu thật phải đi theo con trỏ[^book-ptr-on-stack].

**`Box`.** Book liệt kê ba tình huống dùng `Box`; hai trong số đó: khi kiểu có kích thước không biết lúc biên dịch mà ngữ cảnh đòi kích thước chính xác (kiểu đệ quy), và khi có dữ liệu lớn muốn chuyển ownership mà không copy[^book-box-unknown][^book-box-large]. `Box<T, A>` còn có tham số allocator (mục 5).

**`Vec`: length, capacity.** Capacity là lượng chỗ đã cấp cho các phần tử sẽ thêm sau, khác với length[^vec-cap]. `Vec::new()` không cấp phát cho đến khi có phần tử được push[^vec-new]. Docs hứa chặt hơn: `Vec` cấp phát khi và chỉ khi `size_of::<T>() * capacity() > 0`[^vec-zst]. Lý do nằm ở Nomicon: allocator toàn cục không cho phép cấp khối kích thước 0 (UB), nên `Vec` rỗng không cấp phát và dùng con trỏ "dangling" thay chỗ[^nomicon-vec-empty].

**Vì sao Rust bắt mình quan tâm.** Khi `Vec` đầy và realloc, buffer có thể bị dời sang chỗ khác; Book nói thêm phần tử có thể cần cấp phát bộ nhớ mới và chép phần tử cũ sang đó[^book-vec-realloc]. Cho nên tham chiếu vào phần tử cũ có thể trở thành treo — và borrow checker chặn việc đó lúc biên dịch. Chạy thật:

```rust
let mut v = vec![1]; let first = &v[0]; v.push(2); println!("{first}");
```

→ `error[E0502]: cannot borrow v as mutable because it is also borrowed as immutable`, khớp mô tả "đã mượn với một mutability rồi lại mượn với mutability khác"[^e0502]. Đây chính là use-after-free của C bị biến thành lỗi biên dịch (xem mục 8).

*Liên hệ backend (nhận xét của người soạn):* khi biết trước số phần tử (số dòng kết quả truy vấn), `Vec::with_capacity(n)` tránh các lần realloc trung gian; docs cũng khuyên dùng khi có thể[^vec-recommend]. Cùng ý với cấp trước buffer trong connection/buffer pool.

<details><summary>Đi sâu: chiến lược tăng capacity (docs hứa vs source), kiểm chứng allocator đếm, niche của <code>Option&lt;Box&gt;</code></summary>

**Chiến lược tăng capacity: bảo đảm vs quan sát.**

- *Bảo đảm (docs):* không có chiến lược tăng cụ thể nào được hứa khi đầy hay khi `reserve`[^vec-growth]; chỉ hứa `push` O(1) amortized[^vec-amort]; `with_capacity(n)` yêu cầu allocator đúng kích thước cho n phần tử[^vec-exact].
- *Quan sát (source 1.95.0, chi tiết cài đặt, có thể đổi):* công thức tăng trưởng của `grow_amortized` và capacity tối thiểu theo cỡ phần tử được đọc từ source ở [P10·09, mục 2](../10-rust-internals/09-collections-internals.md). Chạy thật, push 40 phần tử vào `Vec<u64>` cho dãy capacity `4 8 16 32 64`, còn `Vec<u8>` cho `8 16 32 64` — khớp công thức đọc từ source đó, nhưng chương trình không được dựa vào điều đó.

Kiểm chứng bằng allocator đếm: `Vec::<u64>::new()` làm số lần `alloc` tăng 0; `Vec::with_capacity(0)` rồi `push(1)` tăng 1; `vec![0u8; 1<<20]` tăng 1 lần, và `let moved = big;` tăng 0. Với `Vec<()>`, `capacity()` in ra `18446744073709551615` (= `usize::MAX`, quan sát) và `Box::new(())` không gọi allocator, khớp [^box-zst].

Về niche, std bảo đảm `Option<Box<U>>` (chỉ với allocator `Global`) có cùng size, alignment và ABI với `Box<U>`[^opt-npo][^opt-box]; cách đọc bảng `std::option` và điều kiện `U: Sized` ở [P10·02, mục 6](../10-rust-internals/02-type-layout.md). Đo thật: `size_of::<Option<Box<u64>>>() = 8`.

</details>

**Quay lại Q3:** `Box` là một con trỏ; `Vec`/`String` là bộ ba (ptr, cap, len) trỏ vào heap; docs chỉ hứa `push` O(1) amortized, còn dãy capacity cụ thể là chi tiết source.

## 5. `GlobalAlloc`, `Layout` và allocator (Q4)

> **Trả lời ngắn.** `Box` và `Vec` lấy bộ nhớ từ một allocator toàn cục của chương trình; allocator mặc định hiện chưa được chỉ định[^alloc-global], còn `System` là allocator do OS cung cấp[^std-system]. Muốn thay, bạn implement trait `GlobalAlloc` và đăng ký bằng `#[global_allocator]`[^globalalloc]. Allocator API tổng quát hơn (`Allocator`) vẫn chỉ có trên nightly[^allocator-unstable].

**Giải thích.**

Hình dung (nhận xét của người soạn): allocator như bộ phận cấp phát kho của công ty; `Box`/`Vec` chỉ điền phiếu yêu cầu (size + alignment), và khi trả kho thì phải đưa lại đúng phiếu đó.

**Global allocator.** Trong một chương trình, thư viện chuẩn có một allocator "toàn cục" được dùng bởi `Box<T>` và `Vec<T>`[^alloc-one]; allocator mặc định hiện **chưa được chỉ định** (unspecified)[^alloc-global]. `System` là allocator do OS cung cấp; trên Unix nó dựa trên `malloc`[^std-system] (cơ chế `malloc` ở [chương 06](06-heap-allocator.md); phần nhận diện global allocator ở [chương 06 mục 10](06-heap-allocator.md)).

`GlobalAlloc` là trait cho allocator có thể được đăng ký làm mặc định bằng `#[global_allocator]`[^globalalloc]. Reference: thuộc tính này chỉ áp dụng được cho một `static` có kiểu implement `GlobalAlloc`[^ref-global-alloc].

**`Layout` và hợp đồng của `GlobalAlloc`.** `Layout` mô tả một khối nhớ: size kèm alignment là lũy thừa của 2[^layout]; ràng buộc: size làm tròn lên bội của align không được tràn `isize::MAX`[^layout-isize]. Hợp đồng đáng nhớ khi tự viết allocator:

- `alloc` với layout kích thước 0 là UB[^ga-zero] — vì vậy `Vec` rỗng và `Box::new(())` không gọi allocator.
- Allocator mà unwind là UB[^ga-unwind].
- `dealloc` phải nhận đúng `Layout` đã dùng để cấp khối đó, nếu không là UB[^ga-dealloc].

Điểm cuối là khác biệt lớn với C. `free(ptr)` không nhận kích thước, nên allocator phải tự ghi nhớ (OSTEP minh họa bằng header đặt trước khối)[^ostep-header][^ostep-freesize]; cơ chế đầy đủ ở [chương 06 mục 2](06-heap-allocator.md). `dealloc` của Rust nhận lại `Layout`; `Vec` vốn biết capacity của mình nên truyền lại được, và allocator có thể không cần lưu kích thước mỗi khối (suy luận của người soạn từ hợp đồng).

**Allocator API: vẫn unstable.** Trait `Allocator` mô tả một allocator có thể cấp, nới, co và giải phóng các khối dữ liệu bất kỳ được mô tả bằng `Layout`[^allocator-doc]. Trong docs 1.95.0 nó được đánh dấu là API thử nghiệm chỉ có trên nightly (`allocator_api`, issue #32838)[^allocator-unstable]. Hệ quả (suy luận của người soạn): trên stable chỉ chọn được allocator toàn chương trình qua `#[global_allocator]`.

<details><summary>Đi sâu: allocator đếm lời gọi đầy đủ, và lý do thiết kế theo RFC 1398</summary>

Đây là code đã chạy thật, bọc `System` để đếm:

```rust
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
struct Count;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Count {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator] static G: Count = Count;
```

Mọi số "allocs delta" ở chương này đo bằng allocator kiểu này, lấy *hiệu* hai lần đọc (runtime đã cấp phát trước `main`).

Docs của `GlobalAlloc` chỉ nêu hợp đồng, không nêu lý do; lý do ở mức thiết kế API allocator được RFC 1398 phát biểu: malloc buộc allocator tự theo dõi kích thước, trong khi client code thường đã biết, nên RFC muốn client cung cấp metadata như size và alignment cho cả hai lời gọi cấp phát và giải phóng[^rfc1398-why]. (Đây là RFC, không phải tài liệu hợp đồng của std 1.95.0.)

</details>

**Quay lại Q4:** `Box`/`Vec` gọi allocator toàn cục; bạn thay nó bằng `GlobalAlloc` + `#[global_allocator]`, tuân hợp đồng `Layout` (khác 0, `dealloc` đúng layout), còn Allocator API vẫn là nightly.

## 6. `Rc`, `Arc`, `Weak`, vòng tham chiếu và leak (Q5)

> **Trả lời ngắn.** `Rc`/`Arc` cho phép nhiều owner bằng cách đặt bộ đếm tham chiếu cạnh giá trị trong cùng một khối heap[^rc-inner][^arc-inner]; giá trị chỉ bị dọn khi bộ đếm mạnh về 0[^book-rc][^book-leak-rc]. Hai giá trị giữ `Rc` của nhau thì bộ đếm không bao giờ về 0 nên leak; `Weak` không biểu diễn sở hữu nên phá được vòng[^book-weak]. Leak vẫn là *safe* vì bảo đảm của Rust không gồm việc ngăn memory leak[^book-leak].

**Giải thích.**

Hình dung (nhận xét của người soạn): `Rc` như một phòng họp có nhiều người cầm thẻ; phòng chỉ được dọn khi người cuối cùng trả thẻ. Hai người mỗi bên giữ thẻ của nhau và không ai chịu trả trước thì phòng bị khóa mãi.

**Nhiều owner, trả bằng bộ đếm.** Ownership "một owner" không đủ cho đồ thị dữ liệu dùng chung. `Rc<T>` là con trỏ reference-counted đơn luồng[^rc-doc], nó đếm số tham chiếu để biết giá trị còn dùng không[^book-rc]. Trong source 1.95.0, nó cấp phát một `RcInner` chứa hai bộ đếm `strong` và `weak` cạnh `value`[^rc-inner]; `Arc` có `ArcInner` cùng dạng, bộ đếm là số nguyên atomic[^arc-inner].

**Vì sao có hai kiểu.** `Rc` không thể `Send`: clone rồi chuyển clone sang thread khác thì hai thread có thể cập nhật bộ đếm cùng lúc[^book-rc-send]. `Arc` ("Atomically Reference Counted") là bản thread-safe[^arc-doc]; khi con trỏ cuối cùng bị hủy thì giá trị bên trong bị drop[^arc-last]. Book nói thẳng: an toàn luồng có giá về hiệu năng nên chỉ trả khi cần[^book-arc-cost]. Đây là chỗ Rust bước ra khỏi mô hình "một owner, kiểm tra lúc biên dịch" một cách *tường minh*: refcount là chi phí runtime để có nhiều owner (nhận xét của người soạn). Chi tiết `Rc`/`Arc` trong đa luồng ở [P4·08 mục 4](../04-concurrency/08-rust-concurrency.md); atomic và ordering của refcount ở [P4·07 mục 8](../04-concurrency/07-lock-free.md).

**Vòng tham chiếu và `Weak`.** Book giải thích: `Rc` chỉ được dọn khi `strong_count` về 0; nếu hai giá trị giữ `Rc` của nhau thì bộ đếm không bao giờ về 0 và bộ nhớ leak. Book minh họa bằng `Rc<T>` kết hợp `RefCell<T>`[^book-leak-rc].

Cách phá vòng: `Weak<T>`. Book: tham chiếu yếu không biểu diễn quan hệ sở hữu và số đếm của chúng không ảnh hưởng đến lúc `Rc` được dọn; `weak_count` không cần về 0[^book-weak][^book-weak-count]. Docs của `Weak`: nó không ngăn *giá trị* bị drop, nhưng ngăn *vùng cấp phát* (backing store) bị giải phóng[^weak-alloc]; vì thế `upgrade()` trả `Option<Rc<T>>` và có thể là `None`[^weak-upgrade]. Docs cũng nêu công dụng chặn vòng tham chiếu giữa các `Rc`[^weak-cycle].

```mermaid
flowchart LR
  subgraph tree["Cây có con trỏ cha"]
    P["parent: Rc"] -- "Rc (sở hữu)" --> C["child: Rc"]
    C -- "Weak (không sở hữu)" --> P
  end
```

**Leak là safe.** `mem::forget` nhận ownership và "quên" giá trị mà không chạy destructor[^forget]; mọi tài nguyên giá trị đó quản lý (heap, file handle) sẽ nằm lại mãi trạng thái không với tới được[^forget-linger]. Nó *không* `unsafe`, vì các bảo đảm an toàn của Rust không gồm việc destructor luôn chạy[^forget-safe]. Reference nói cùng ý: ngăn destructor chạy là safe, và kiểu không được dựa vào việc destructor chạy để giữ soundness (ngoài những chỗ Reference bảo đảm destructor chạy)[^ref-forget-sound]. `Box::leak` tiêu thụ Box và trả `&'a mut T`[^box-leak]; làm rơi tham chiếu đó gây leak[^box-leak-drop]. Chạy thật: `Box::leak(Box::new(vec![1,2,3]))` cho một `&'static mut Vec<i32>` dùng được đến hết chương trình.

Book kết luận: ngăn memory leak hoàn toàn không phải bảo đảm của Rust, tức leak là memory safe[^book-leak]. Nomicon thừa nhận niềm tin "Rust loại trừ rò rỉ tài nguyên" về cơ bản đúng trong thực tế nhưng không đúng về lý thuyết[^nomicon-leak].

<details><summary>Đi sâu: số đo khối heap 24 byte, thí nghiệm vòng <code>Rc</code> và <code>Weak</code></summary>

Đo thật bằng allocator đếm (mục 5): `Rc::new(7u64)` làm allocator nhận một yêu cầu với `layout.size()` = **24**, `Arc::new(7u64)` cũng 24, còn `Box::new(7u64)` là 8. Đây là **kích thước khối heap được cấp**, không phải `size_of` của các kiểu đó: với `u64`, 24 = hai bộ đếm 8 byte + giá trị 8 byte (`Rc<u8>` cũng 24 do padding; `Box<u64>` không có bộ đếm nên chỉ 8). Còn `size_of` của `Rc<u64>`, `Arc<u64>`, `Box<u64>` đều là 8 (đo thật): chỉ là con trỏ. Con số 24 là quan sát, không phải hợp đồng.

Chạy thật: hai `Node` trỏ vòng tròn A → B → A, mỗi node giữ một `Rc`. Trong scope: `strong a=2 b=2`. Ra khỏi scope, allocator đếm được `allocs=2 deallocs=0`, và dòng `drop cycA`/`drop cycB` (in từ `Drop` của node) **không bao giờ xuất hiện**. Lưu ý đây là leak trong *safe Rust*, không cần `unsafe`.

Chạy thật với `Weak`: sau `Rc::downgrade`, `strong=1 weak=1`; `drop(r)` in `drop weak-target` ngay (giá trị bị drop dù còn `Weak`), và `w.upgrade().is_none()` là `true`.

</details>

**Quay lại Q5:** `Rc`/`Arc` đếm owner bằng bộ đếm cạnh giá trị; vòng `Rc` không bao giờ về 0 nên leak, `Weak` phá vòng, và leak vẫn safe vì ngăn leak không nằm trong bảo đảm của Rust.

## 7. `MaybeUninit`: bộ nhớ chưa khởi tạo trong hệ thống kiểu (Q6)

> **Trả lời ngắn.** Compiler Rust giả định mọi biến luôn được khởi tạo đúng yêu cầu của kiểu nó[^maybe-invariant], nên "cứ để biến chưa khởi tạo" không được phép[^e0381]. `MaybeUninit<T>` là kiểu bao dùng để tạo thể hiện chưa khởi tạo của `T`[^maybe-doc], đồng thời báo cho compiler biết dữ liệu ở đó có thể chưa khởi tạo[^maybe-signal].

**Giải thích.**

Hình dung (nhận xét của người soạn): `MaybeUninit` như ô "đang để trống" trên phiếu — ghi rõ chưa có gì, thay vì điền bừa một giá trị rác trông như hợp lệ.

**Vấn đề.** `malloc` trả về bộ nhớ chưa khởi tạo[^malloc-man], và OSTEP xếp việc đọc vùng này vào các lỗi kinh điển: chương trình sẽ gặp một "uninitialized read" đọc dữ liệu không biết giá trị từ heap[^ostep-uninit]. Rust cấm dùng biến chưa khởi tạo ở safe code (mục 8), nhưng đôi khi bạn cần chính vùng nhớ chưa khởi tạo, ví dụ buffer vừa cấp phát và sắp được ghi vào (nhận xét của người soạn).

**Vì sao không thể "cứ để trống".** Compiler giả định biến luôn được khởi tạo đúng yêu cầu của kiểu (ví dụ tham chiếu phải căn đúng và không null)[^maybe-invariant]; do đó khởi tạo một tham chiếu bằng 0 là UB tức thời, kể cả khi chưa dùng để truy cập bộ nhớ[^maybe-zeroed]. Docs còn cho biết compiler khai thác giả định này để tối ưu, như bỏ kiểm tra lúc chạy và tối ưu layout enum[^maybe-exploit]. Nghĩa là không có cách "giả vờ" một ô nhớ rác là một `T` mà không rủi ro.

**Lời giải.** Đặt vùng chưa khởi tạo vào bên trong `MaybeUninit<T>`: thể hiện này không buộc phải thỏa yêu cầu hợp lệ của `T` (suy luận của người soạn từ docs về tín hiệu cho compiler[^maybe-signal]). Mẫu dùng: `uninit()`, `write` giá trị vào, chỉ `assume_init()` (là `unsafe`) khi chắc đã khởi tạo xong; chi tiết ở [P10·07 mục 6](../10-rust-internals/07-unsafe-ub-miri.md).

<details><summary>Đi sâu: bảo đảm layout của <code>MaybeUninit</code></summary>

`MaybeUninit<T>` được bảo đảm cùng size, alignment, ABI với `T`[^maybe-layout]; đo thật `size_of::<MaybeUninit<u64>>() = 8` và `MaybeUninit<String> = 24`.

</details>

**Quay lại Q6:** `MaybeUninit<T>` là kiểu bao dành cho bộ nhớ chưa khởi tạo, nên compiler không coi nó là một `T` đã hợp lệ[^maybe-signal]; chỉ `assume_init` khi đã chắc.

## 8. Lỗi bộ nhớ của C bị Rust chặn ra sao (Q7)

> **Trả lời ngắn.** Rust không dùng GC: bộ nhớ được trả khi owner ra scope[^book-auto], và compiler quyết định lúc biên dịch. Nhờ ownership, borrow checker (phần của compiler kiểm tra việc mượn tham chiếu) và kiểm tra chỉ số, safe code chặn được use-after-free, double free, đọc vùng chưa khởi tạo, tràn buffer và data race. Leak thì *không* bị chặn[^book-leak].

**Giải thích.**

Hình dung (nhận xét của người soạn): các lỗi của C là những quy ước "nhớ làm"; Rust biến từng quy ước thành một kiểm tra mà compiler không cho bỏ qua.

Nomicon: ownership cho phép Rust hoàn toàn memory-safe và hiệu quả, đồng thời tránh garbage collection[^nomicon-nogc]. Hệ quả vận hành (nhận xét của người soạn): không có pause của GC, nhưng đổi lại thời điểm giải phóng là một phần ngữ nghĩa chương trình bạn phải hiểu (mục 3), và vòng `Rc` không được ai dọn.

Đối chiếu các lỗi mà OSTEP liệt kê cho C ([chương 01 mục 6](01-address-space.md); chương 01 mục 7 có bản tóm lược ngắn của bảng này, bảng dưới là bản đầy đủ) với cơ chế Rust:

| Lỗi trong C | Rust chặn bằng | Bằng chứng |
|---|---|---|
| Dangling pointer / use-after-free: free rồi còn dùng[^ostep-dangling] | borrow checker: compiler bảo đảm dữ liệu không ra khỏi scope trước tham chiếu tới nó[^book-dangling]; mục đích chính của lifetime là chặn dangling reference[^book-lifetime-aim] | `let r; { let x = 5; r = &x; } println!("{r}");` → `E0597`: "giá trị bị drop khi vẫn đang được mượn"[^e0597] (chạy thật) |
| Double free[^ostep-doublefree] | một owner; move vô hiệu hóa biến cũ[^book-doublefree] | `E0382` (mục 2) |
| Quên khởi tạo, đọc rác[^ostep-uninit] | không cho dùng biến chưa khởi tạo[^e0381]; muốn bộ nhớ chưa khởi tạo phải qua `MaybeUninit` | mục 7 |
| Tràn buffer / đọc quá cuối[^ostep-overflow] | chỉ số `[]` ngoài miền thì panic; C thì là UB và có thể thành lỗ hổng[^book-overread-c][^book-panic] | chạy thật: `v[10]` trên `Vec` 3 phần tử → `panicked ... index out of bounds: the len is 3 but the index is 10`, exit code 101 |
| Data race | borrow checker: tại một thời điểm hoặc một `&mut` hoặc nhiều `&`[^book-ref-rule]; Book nói Rust từ chối biên dịch code có data race[^book-datarace] | Book 4.2; qua ranh giới thread còn có `Send`/`Sync`, xem [P4·08](../04-concurrency/08-rust-concurrency.md) |
| Quên free (leak)[^ostep-leak] | **không chặn** — leak là safe[^book-leak] | mục 6 |

<details><summary>Đi sâu: phạm vi bảo đảm của Rust — leak, tràn stack và <code>unsafe</code></summary>

Bảo đảm của Rust là *safe code không gây UB* (nhận xét của người soạn, tổng hợp từ các nguồn trên); leak vẫn xảy ra được[^book-leak], và tràn stack vẫn xảy ra (mục 9; cơ chế ở [chương 07 mục 4](07-stack-alignment-padding.md)). Code `unsafe` (và allocator tự viết ở mục 5) là nơi bạn tự gánh các hợp đồng này.

</details>

**Quay lại Q7:** safe Rust chặn use-after-free, double free, đọc rác, tràn buffer và data race bằng kiểm tra lúc biên dịch hoặc panic; leak và tràn stack vẫn xảy ra được.

## 9. Chọn stack hay heap trong Rust (Q8)

> **Trả lời ngắn.** Stack chỉ chứa được dữ liệu có kích thước biết trước và cố định; dữ liệu có kích thước chưa biết lúc biên dịch hoặc có thể đổi thì phải ở heap[^book-stack-fixed]. Khi cả hai chỗ đều dùng được, nên ưu tiên stack vì rẻ hơn: đẩy lên stack không cần allocator đi tìm chỗ trống[^book-push-faster] (khuyến nghị của người soạn).

**Giải thích.**

Hình dung (nhận xét của người soạn): stack như bàn làm việc — đồ nhỏ, biết trước thì để ngay trên bàn; heap như nhà kho — cần xin chỗ, và bàn chỉ giữ tờ phiếu ghi vị trí.

**Hai con đường cấp chỗ cho một giá trị.**

1. *Stack:* đẩy một giá trị lên stack nhanh hơn cấp phát heap vì không có allocator nào phải tìm chỗ[^book-push-faster]; đổi lại kích thước phải biết trước và cố định[^book-stack-fixed].
2. *Heap:* allocator tìm một chỗ trống đủ lớn, đánh dấu đang dùng rồi trả về con trỏ[^book-heap-alloc]. Con trỏ có kích thước cố định nên lưu được trên stack, còn dữ liệu thật phải đi theo con trỏ[^book-ptr-on-stack].
3. Vì thế `Vec`/`String`/`Box` là "vỏ" nhỏ trên stack trỏ tới dữ liệu ở heap (mục 4), và quyết định stack hay heap thực chất là quyết định *biết kích thước trước hay không* và *có cần sống lâu / chuyển ownership rẻ không*.

Bảng dưới là hướng dẫn chọn rút ra từ các tính chất trên (nhận xét của người soạn):

| Tình huống | Chọn | Lý do |
|---|---|---|
| Số nguyên, struct nhỏ, tuple, array cỡ cố định nhỏ | stack (mặc định) | kích thước biết trước; không gọi allocator |
| Kích thước chỉ biết lúc chạy hoặc co giãn (danh sách, chuỗi) | `Vec`, `String` | dữ liệu ở heap, bản thân handle 3 từ ở stack (mục 4) |
| Kiểu đệ quy, hoặc cần kiểu có kích thước cố định cho giá trị kích thước không rõ | `Box<T>` | Book: kích thước không biết lúc biên dịch[^book-box-unknown] |
| Dữ liệu lớn, muốn chuyển ownership mà không copy | `Box<T>`, hoặc `Vec` | chỉ chuyển con trỏ (mục 2)[^book-box-large] |
| Nhiều owner, đơn luồng / đa luồng | `Rc<T>` / `Arc<T>` | trả bằng refcount (mục 6) |
| Sống đến hết chương trình, cần `&'static` | `Box::leak` | leak có chủ đích (mục 6) |

<details><summary>Đi sâu: stack có giới hạn — tràn stack và handler của std</summary>

Stack có giới hạn nên array rất lớn hoặc đệ quy sâu có thể làm tràn stack (nhận xét của người soạn). Trên Unix, comment trong source std 1.95.0 ghi rằng std cài signal handler cho `SIGSEGV` và `SIGBUS`, và mỗi stack của thread có guard page ở cuối: chạm vào guard page thì handler này được kích hoạt[^std-guard]. Cơ chế đầy đủ (guard page, alternate signal stack, vì sao kết cục là `abort`) ở [chương 07 mục 4](07-stack-alignment-padding.md).

</details>

**Quay lại Q8:** kích thước biết trước và nhỏ thì stack; co giãn, đệ quy hoặc lớn thì heap qua `Box`/`Vec`/`String`; nhiều owner thì `Rc`/`Arc`.

## Tóm tắt

- Ownership: một owner, drop khi ra scope[^book-rules]; move chỉ copy "vỏ" ba từ của `String` (đo: 0 cấp phát thêm, con trỏ heap không đổi).
- `Copy` và `Drop` loại trừ nhau[^copy-doc-drop]; thứ tự drop: biến ngược khai báo, field/tuple xuôi.
- `Vec`/`String` là bộ ba (ptr, cap, len); docs chỉ hứa `push` O(1) amortized, "4, 8, 16…" là chi tiết source.
- `GlobalAlloc` đòi layout khác 0 và `dealloc` nhận đúng `Layout`; allocator mặc định unspecified; Allocator API vẫn chỉ trên nightly.
- `Rc`/`Arc` đặt bộ đếm cạnh giá trị (khối heap 24 byte cho `u64`); vòng `Rc` leak mà không gọi `Drop`; `Weak` phá vòng.
- Leak là safe: `forget`, `Box::leak`, vòng `Rc` đều không cần `unsafe`; vì vậy code `unsafe` không được dựa vào việc destructor chạy.
- `MaybeUninit<T>` biểu diễn bộ nhớ chưa khởi tạo mà không vi phạm giả định của compiler.
- Chọn stack cho dữ liệu cỡ cố định, nhỏ; heap (`Box`/`Vec`/`String`) cho dữ liệu co giãn, đệ quy hoặc lớn; `Rc`/`Arc` khi thật sự cần nhiều owner.

## Câu hỏi tự kiểm tra

1. Ba quy tắc ownership là gì, và quy tắc nào chặn double free?
<details><summary>Đáp án</summary>

Mỗi giá trị có một owner, một owner tại một thời điểm, owner ra scope thì giá trị bị drop[^book-rules]. Quy tắc "một owner" chặn double free vì sau move biến cũ không còn hợp lệ[^book-doublefree][^book-move]; dùng lại nó là `E0382`[^e0382].

</details>

2. Sau `let s2 = s1;` với `s1: String`, dữ liệu heap có bị sao chép không? Bằng chứng?
<details><summary>Đáp án</summary>

Không. Book: struct ba phần (ptr, len, cap) nằm trên stack được move[^book-string]. Đo thật bằng allocator đếm: số `alloc` không tăng và `as_ptr()` giữ nguyên (mục 2).

</details>

3. Vì sao `#[derive(Clone, Copy)] struct X; impl Drop for X {…}` không biên dịch?
<details><summary>Đáp án</summary>

Kiểu `Drop` quản lý tài nguyên ngoài chính các byte của nó nên không thể `Copy`[^copy-doc-drop]; compiler báo `E0184`[^e0184].

</details>

4. Đoán thứ tự in khi drop: `let _a = P("a"); let _b = (P("b0"), P("b1"));`.
<details><summary>Đáp án</summary>

`b0`, `b1`, rồi `a`. Biến drop ngược thứ tự khai báo[^ref-drop-scope]; phần tử tuple drop theo thứ tự[^ref-drop-tuple].

</details>

5. Docs `Vec` hứa gì và không hứa gì về chiến lược tăng capacity?
<details><summary>Đáp án</summary>

Không hứa chiến lược cụ thể[^vec-growth]; chỉ hứa `push` O(1) amortized[^vec-amort] và `with_capacity(n)` yêu cầu đúng kích thước cho n phần tử[^vec-exact]. Cấp số nhân với capacity tối thiểu 4 hoặc 8 là chi tiết source 1.95.0 ([P10·09, mục 2](../10-rust-internals/09-collections-internals.md)).

</details>

6. `dealloc` của `GlobalAlloc` khác `free` của C ở điểm nào về tham số, và hợp đồng đi kèm?
<details><summary>Đáp án</summary>

`free(ptr)` không nhận kích thước nên allocator tự lưu[^ostep-freesize]; `dealloc` nhận `Layout` và phải là đúng layout đã dùng khi cấp, nếu không là UB[^ga-dealloc].

</details>

7. Vì sao vòng `Rc` leak, và `Weak` sửa thế nào?
<details><summary>Đáp án</summary>

Mỗi `Rc` trong vòng giữ `strong_count` của cái kia nên không về 0[^book-leak-rc]. `Weak` không biểu diễn sở hữu và không ảnh hưởng lúc `Rc` được dọn[^book-weak]; `upgrade()` trả `Option` vì giá trị có thể đã bị drop[^weak-upgrade].

</details>

8. `mem::forget` có `unsafe` không? Vì sao?
<details><summary>Đáp án</summary>

Không, vì bảo đảm an toàn của Rust không gồm việc destructor luôn chạy[^forget-safe]. Hệ quả: code `unsafe` không được dựa vào destructor để giữ soundness[^ref-forget-sound].

</details>

9. Vì sao `let r: &i32 = unsafe { MaybeUninit::zeroed().assume_init() };` là UB dù chưa đọc `*r`?
<details><summary>Đáp án</summary>

Compiler giả định tham chiếu luôn căn đúng và không null[^maybe-invariant]; zero-init một tham chiếu là UB tức thời, bất kể có dùng để truy cập hay không[^maybe-zeroed].

</details>

10. Lỗi C nào Rust *không* chặn được ở safe code, và vì sao điều đó vẫn "safe"?
<details><summary>Đáp án</summary>

Leak: không nằm trong bảo đảm của Rust[^book-leak]; vùng nhớ không bao giờ được free không thể bị dùng sai, chỉ bị lãng phí (suy luận của người soạn).

</details>

## Đọc thêm

- [The Rust Book, 4.1 What Is Ownership?](https://doc.rust-lang.org/1.95.0/book/ch04-01-what-is-ownership.html): bản gốc của ba quy tắc và minh họa move/stack/heap.
- [The Rust Reference, Destructors](https://doc.rust-lang.org/1.95.0/reference/destructors.html): quy tắc đầy đủ về drop scope, temporary và partial move.
- [`std::alloc`](https://doc.rust-lang.org/1.95.0/std/alloc/index.html) và [`GlobalAlloc`](https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html): hợp đồng an toàn khi tự viết allocator.
- [`MaybeUninit`](https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html): các ví dụ khởi tạo mảng và những cách gây UB.
- [Chương 06 — Heap allocator](06-heap-allocator.md): bên dưới `System` là `malloc`, free list và `brk`/`mmap`.
- [Chương 07 — Stack, alignment và padding, mục 4](07-stack-alignment-padding.md): guard page và cách std biến tràn stack thành `abort`.

[^ostep-leak]: OSTEP, ch.14 Interlude: Memory API — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-api.pdf> — "Another common error is known as a memory leak, and it occurs when you forget to free memory."
[^ostep-doublefree]: OSTEP, ch.14 Interlude: Memory API — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-api.pdf> — "Programs also sometimes free memory more than once; this is known as the double free. The result of doing so is undefined."
[^book-gc]: The Rust Programming Language, 4.1 What Is Ownership? — <https://doc.rust-lang.org/1.95.0/book/ch04-01-what-is-ownership.html> — "In languages with a garbage collector (GC), the GC keeps track of and cleans up memory that isn’t being used anymore, and we don’t need to think about it."
[^book-auto]: The Rust Programming Language, 4.1 What Is Ownership? — <https://doc.rust-lang.org/1.95.0/book/ch04-01-what-is-ownership.html> — "Rust takes a different path: The memory is automatically returned once the variable that owns it goes out of scope."
[^book-rules]: The Rust Programming Language, 4.1 What Is Ownership? — <https://doc.rust-lang.org/1.95.0/book/ch04-01-what-is-ownership.html> — "Each value in Rust has an owner. There can only be one owner at a time. When the owner goes out of scope, the value will be dropped."
[^book-doublefree]: The Rust Programming Language, 4.1 What Is Ownership? — <https://doc.rust-lang.org/1.95.0/book/ch04-01-what-is-ownership.html> — "When s2 and s1 go out of scope, they will both try to free the same memory. This is known as a double free error and is one of the memory safety bugs we mentioned previously."
[^book-move]: The Rust Programming Language, 4.1 What Is Ownership? — <https://doc.rust-lang.org/1.95.0/book/ch04-01-what-is-ownership.html> — "But because Rust also invalidates the first variable, instead of being called a shallow copy, it’s known as a move."
[^e0382]: Rust error index 1.95.0, E0382 — <https://doc.rust-lang.org/1.95.0/error_codes/E0382.html> — "A variable was used after its contents have been moved elsewhere."
[^book-string]: The Rust Programming Language, 4.1 What Is Ownership? — <https://doc.rust-lang.org/1.95.0/book/ch04-01-what-is-ownership.html> — "A String is made up of three parts, shown on the left: a pointer to the memory that holds the contents of the string, a length, and a capacity. This group of data is stored on the stack."
[^ref-move-intro]: The Rust Reference, Expressions (Moved and copied types) — <https://doc.rust-lang.org/1.95.0/reference/expressions.html> — "When a place expression is evaluated in a value expression context, or is bound by value in a pattern, it denotes the value held in that memory location."
[^ref-move-copy]: The Rust Reference, Expressions (Moved and copied types) — <https://doc.rust-lang.org/1.95.0/reference/expressions.html> — "If the type of that value implements Copy, then the value will be copied."
[^ref-move-sized]: The Rust Reference, Expressions (Moved and copied types) — <https://doc.rust-lang.org/1.95.0/reference/expressions.html> — "In the remaining situations, if that type is Sized, then it may be possible to move the value."
[^copy-bits]: Rust std 1.95.0, trait `Copy` — <https://doc.rust-lang.org/1.95.0/std/marker/trait.Copy.html> — "Under the hood, both a copy and a move can result in bits being copied in memory, although this is sometimes optimized away."
[^book-box-large]: The Rust Programming Language, 15.1 Using Box\<T> to Point to Data on the Heap — <https://doc.rust-lang.org/1.95.0/book/ch15-01-box.html> — "When you have a large amount of data, and you want to transfer ownership but ensure that the data won’t be copied when you do so"
[^copy-def]: Rust std 1.95.0, trait `Copy` — <https://doc.rust-lang.org/1.95.0/std/marker/trait.Copy.html> — "Types whose values can be duplicated simply by copying bits."
[^book-copy-drop]: The Rust Programming Language, 4.1 What Is Ownership? — <https://doc.rust-lang.org/1.95.0/book/ch04-01-what-is-ownership.html> — "Rust won’t let us annotate a type with Copy if the type, or any of its parts, has implemented the Drop trait."
[^copy-doc-drop]: Rust std 1.95.0, trait `Copy` (When can’t my type be Copy?) — <https://doc.rust-lang.org/1.95.0/std/marker/trait.Copy.html> — "any type implementing Drop can’t be Copy, because it’s managing some resource besides its own size_of::\<T> bytes."
[^e0184]: Rust error index 1.95.0, E0184 — <https://doc.rust-lang.org/1.95.0/error_codes/E0184.html> — "The Copy trait was implemented on a type with a Drop implementation."
[^book-drop-trait]: The Rust Programming Language, 15.3 Running Code on Cleanup with the Drop Trait — <https://doc.rust-lang.org/1.95.0/book/ch15-03-drop.html> — "The second trait important to the smart pointer pattern is Drop, which lets you customize what happens when a value is about to go out of scope."
[^book-drop-auto]: The Rust Programming Language, 15.3 Running Code on Cleanup with the Drop Trait — <https://doc.rust-lang.org/1.95.0/book/ch15-03-drop.html> — "and the compiler will insert this code automatically. As a result, you don’t need to be careful about placing cleanup code everywhere in a program"
[^book-drop-manual]: The Rust Programming Language, 15.3 Running Code on Cleanup with the Drop Trait — <https://doc.rust-lang.org/1.95.0/book/ch15-03-drop.html> — "Rust doesn’t let you call the Drop trait’s drop method manually; instead, you have to call the std::mem::drop function provided by the standard library if you want to force a value to be dropped before the end of its scope."
[^drop-two-parts]: Rust std 1.95.0, trait `Drop` — <https://doc.rust-lang.org/1.95.0/std/ops/trait.Drop.html> — "A call to Drop::drop for that value, if this special Drop trait is implemented for its type."
[^drop-glue]: Rust std 1.95.0, trait `Drop` — <https://doc.rust-lang.org/1.95.0/std/ops/trait.Drop.html> — "which recursively calls the destructors of all the fields of this value."
[^ref-destr-op]: The Rust Reference, Destructors — <https://doc.rust-lang.org/1.95.0/reference/destructors.html> — "Recursively running the destructor of all of its fields."
[^drop-resources]: Rust std 1.95.0, trait `Drop` — <https://doc.rust-lang.org/1.95.0/std/ops/trait.Drop.html> — "That resource may be memory, it may be a file descriptor, it may be a network socket."
[^nomicon-orm]: The Rustonomicon, Leaking — <https://doc.rust-lang.org/1.95.0/nomicon/leaking.html> — "You acquire resources when you create the object, and you release the resources when it gets destroyed."
[^ref-drop-scope]: The Rust Reference, Destructors — <https://doc.rust-lang.org/1.95.0/reference/destructors.html> — "all variables associated to that scope are dropped in reverse order of declaration (for variables) or creation (for temporaries)."
[^book-drop-order]: The Rust Programming Language, 15.3 Running Code on Cleanup with the Drop Trait — <https://doc.rust-lang.org/1.95.0/book/ch15-03-drop.html> — "Variables are dropped in the reverse order of their creation"
[^ref-drop-fields]: The Rust Reference, Destructors — <https://doc.rust-lang.org/1.95.0/reference/destructors.html> — "The fields of a struct are dropped in declaration order."
[^ref-drop-tuple]: The Rust Reference, Destructors — <https://doc.rust-lang.org/1.95.0/reference/destructors.html> — "The fields of a tuple are dropped in order."
[^ref-destr-array]: The Rust Reference, Destructors — <https://doc.rust-lang.org/1.95.0/reference/destructors.html> — "The elements of an array or owned slice are dropped from the first element to the last."
[^box-owns]: Rust std 1.95.0, `Box` — <https://doc.rust-lang.org/1.95.0/std/boxed/struct.Box.html> — "A pointer type that uniquely owns a heap allocation of type T."
[^box-zst]: Rust std 1.95.0, `Box::new` — <https://doc.rust-lang.org/1.95.0/std/boxed/struct.Box.html> — "This doesn’t actually allocate if T is zero-sized."
[^vec-triplet]: Rust std 1.95.0, `Vec` (Guarantees) — <https://doc.rust-lang.org/1.95.0/std/vec/struct.Vec.html> — "Most fundamentally, Vec is and always will be a (pointer, capacity, length) triplet."
[^vec-heap]: Rust std 1.95.0, `Vec` — <https://doc.rust-lang.org/1.95.0/std/vec/struct.Vec.html> — "If a Vec has allocated memory, then the memory it points to is on the heap (as defined by the allocator Rust is configured to use by default)"
[^string-repr]: Rust std 1.95.0, `String` (Representation) — <https://doc.rust-lang.org/1.95.0/std/string/struct.String.html> — "A String is made up of three components: a pointer to some bytes, a length, and a capacity."
[^string-doc]: Rust std 1.95.0, `String` — <https://doc.rust-lang.org/1.95.0/std/string/struct.String.html> — "It has ownership over the contents of the string, stored in a heap-allocated buffer"
[^dst-fat]: The Rust Reference, Dynamically Sized Types — <https://doc.rust-lang.org/1.95.0/reference/dynamically-sized-types.html> — "Pointer types to DSTs are sized but have twice the size of pointers to sized types"
[^book-ptr-on-stack]: The Rust Programming Language, 4.1 What Is Ownership? — <https://doc.rust-lang.org/1.95.0/book/ch04-01-what-is-ownership.html> — "Because the pointer to the heap is a known, fixed size, you can store the pointer on the stack, but when you want the actual data, you must follow the pointer."
[^book-box-unknown]: The Rust Programming Language, 15.1 Using Box\<T> to Point to Data on the Heap — <https://doc.rust-lang.org/1.95.0/book/ch15-01-box.html> — "When you have a type whose size can’t be known at compile time"
[^opt-npo]: Rust std 1.95.0, `std::option` (Representation) — <https://doc.rust-lang.org/1.95.0/std/option/index.html> — "Rust guarantees to optimize the following types T such that Option\<T> has the same size, alignment, and function call ABI as T."
[^opt-box]: Rust std 1.95.0, `std::option` (Representation) — <https://doc.rust-lang.org/1.95.0/std/option/index.html> — "Box\<U> (specifically, only Box\<U, Global>)"
[^vec-cap]: Rust std 1.95.0, `Vec` (Capacity and reallocation) — <https://doc.rust-lang.org/1.95.0/std/vec/struct.Vec.html> — "The capacity of a vector is the amount of space allocated for any future elements that will be added onto the vector."
[^vec-new]: Rust std 1.95.0, `Vec::new` — <https://doc.rust-lang.org/1.95.0/std/vec/struct.Vec.html> — "The vector will not allocate until elements are pushed onto it."
[^vec-zst]: Rust std 1.95.0, `Vec` (Guarantees) — <https://doc.rust-lang.org/1.95.0/std/vec/struct.Vec.html> — "Vec will allocate if and only if size_of::\<T>() * capacity() > 0."
[^nomicon-vec-empty]: The Rustonomicon, Implementing Vec: Allocating Memory — <https://doc.rust-lang.org/1.95.0/nomicon/vec/vec-alloc.html> — "creating an empty Vec doesn’t actually allocate at all. This is not the same as allocating a zero-sized memory block, which is not allowed by the global allocator (it results in undefined behavior!)."
[^vec-growth]: Rust std 1.95.0, `Vec` (Guarantees) — <https://doc.rust-lang.org/1.95.0/std/vec/struct.Vec.html> — "Vec does not guarantee any particular growth strategy when reallocating when full, nor when reserve is called."
[^vec-amort]: Rust std 1.95.0, `Vec` (Guarantees) — <https://doc.rust-lang.org/1.95.0/std/vec/struct.Vec.html> — "Whatever strategy is used will of course guarantee O(1) amortized push."
[^vec-exact]: Rust std 1.95.0, `Vec` (Guarantees) — <https://doc.rust-lang.org/1.95.0/std/vec/struct.Vec.html> — "Vec::with_capacity(n) produce a Vec that requests an allocation of the exact size needed for precisely n elements from the allocator"
[^book-vec-realloc]: The Rust Programming Language, 8.1 Storing Lists of Values with Vectors — <https://doc.rust-lang.org/1.95.0/book/ch08-01-vectors.html> — "adding a new element onto the end of the vector might require allocating new memory and copying the old elements to the new space, if there isn’t enough room to put all the elements next to each other where the vector is currently stored."
[^e0502]: Rust error index 1.95.0, E0502 — <https://doc.rust-lang.org/1.95.0/error_codes/E0502.html> — "A variable already borrowed with a certain mutability (either mutable or immutable) was borrowed again with a different mutability."
[^vec-recommend]: Rust std 1.95.0, `Vec` (Capacity and reallocation) — <https://doc.rust-lang.org/1.95.0/std/vec/struct.Vec.html> — "it is recommended to use Vec::with_capacity whenever possible to specify how big the vector is expected to get."
[^alloc-one]: Rust std 1.95.0, module `std::alloc` — <https://doc.rust-lang.org/1.95.0/std/alloc/index.html> — "that is used for example by Box\<T> and Vec\<T>."
[^alloc-global]: Rust std 1.95.0, module `std::alloc` — <https://doc.rust-lang.org/1.95.0/std/alloc/index.html> — "Currently the default global allocator is unspecified."
[^std-system]: Rust std 1.95.0, `std::alloc::System` — <https://doc.rust-lang.org/1.95.0/std/alloc/struct.System.html> — "The default memory allocator provided by the operating system. This is based on malloc on Unix platforms and HeapAlloc on Windows, plus related functions."
[^globalalloc]: Rust std 1.95.0, `GlobalAlloc` — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html> — "A memory allocator that can be registered as the standard library’s default through the #[global_allocator] attribute."
[^ref-global-alloc]: The Rust Reference, The global_allocator attribute — <https://doc.rust-lang.org/1.95.0/reference/runtime.html> — "The global_allocator attribute may only be applied to a static item whose type implements the GlobalAlloc trait."
[^layout]: Rust std 1.95.0, `Layout` — <https://doc.rust-lang.org/1.95.0/std/alloc/struct.Layout.html> — "All layouts have an associated size and a power-of-two alignment."
[^layout-isize]: Rust std 1.95.0, `Layout` — <https://doc.rust-lang.org/1.95.0/std/alloc/struct.Layout.html> — "The size, when rounded up to the nearest multiple of align, does not overflow isize (i.e., the rounded value will always be less than or equal to isize::MAX)."
[^ga-zero]: Rust std 1.95.0, `GlobalAlloc::alloc` (Safety) — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html> — "layout must have non-zero size. Attempting to allocate for a zero-sized layout will result in undefined behavior."
[^ga-unwind]: Rust std 1.95.0, `GlobalAlloc` (Safety) — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html> — "It’s undefined behavior if global allocators unwind."
[^ga-dealloc]: Rust std 1.95.0, `GlobalAlloc::dealloc` (Safety) — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html> — "layout is the same layout that was used to allocate that block of memory. Otherwise the behavior is undefined."
[^ostep-header]: OSTEP, ch.17 Free-Space Management — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-freespace.pdf> — "You might have noticed that the interface to free(void *ptr) does not take a size parameter"
[^ostep-freesize]: OSTEP, ch.14 Interlude: Memory API — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-api.pdf> — "the size of the allocated region is not passed in by the user, and must be tracked by the memory-allocation library itself."
[^rfc1398-why]: RFC 1398 (Kinds of allocators), "Why not just `free(ptr)` for deallocation?" — <https://raw.githubusercontent.com/rust-lang/rfcs/d2705ad64ba47e812a2b6b19d6f613fb4c14c5e0/text/1398-kinds-of-allocators.md> — "we want to require client code to provide the metadata like size and alignment to both the allocation and deallocation call sites."
[^allocator-doc]: Rust std 1.95.0, trait `Allocator` — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.Allocator.html> — "An implementation of Allocator can allocate, grow, shrink, and deallocate arbitrary blocks of data described via Layout."
[^allocator-unstable]: Rust std 1.95.0, trait `Allocator` — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.Allocator.html> — "This is a nightly-only experimental API. (allocator_api #32838)"
[^rc-doc]: Rust std 1.95.0, `Rc` — <https://doc.rust-lang.org/1.95.0/std/rc/struct.Rc.html> — "A single-threaded reference-counting pointer."
[^book-rc]: The Rust Programming Language, 15.4 Rc\<T>, the Reference Counted Smart Pointer — <https://doc.rust-lang.org/1.95.0/book/ch15-04-rc.html> — "The Rc\<T> type keeps track of the number of references to a value to determine whether or not the value is still in use."
[^rc-inner]: rust-lang/rust tag 1.95.0, library/alloc/src/rc.rs, `RcInner` — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/alloc/src/rc.rs> — "struct RcInner\<T: ?Sized> { strong: Cell\<usize>, weak: Cell\<usize>, value: T, }"
[^arc-inner]: rust-lang/rust tag 1.95.0, library/alloc/src/sync.rs, `ArcInner` — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/alloc/src/sync.rs> — "struct ArcInner\<T: ?Sized> { strong: Atomic\<usize>,"
[^book-rc-send]: The Rust Programming Language, 16.4 Extensible Concurrency with Send and Sync — <https://doc.rust-lang.org/1.95.0/book/ch16-04-extensible-concurrency-sync-and-send.html> — "This cannot implement Send because if you cloned an Rc\<T> value and tried to transfer ownership of the clone to another thread, both threads might update the reference count at the same time."
[^arc-doc]: Rust std 1.95.0, `Arc` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Arc.html> — "A thread-safe reference-counting pointer. ‘Arc’ stands for ‘Atomically Reference Counted’."
[^arc-last]: Rust std 1.95.0, `Arc` — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Arc.html> — "When the last Arc pointer to a given allocation is destroyed, the value stored in that allocation (often referred to as “inner value”) is also dropped."
[^book-arc-cost]: The Rust Programming Language, 16.3 Shared-State Concurrency — <https://doc.rust-lang.org/1.95.0/book/ch16-03-shared-state.html> — "The reason is that thread safety comes with a performance penalty that you only want to pay when you really need to."
[^book-leak-rc]: The Rust Programming Language, 15.6 Reference Cycles Can Leak Memory — <https://doc.rust-lang.org/1.95.0/book/ch15-06-reference-cycles.html> — "We can see that Rust allows memory leaks by using Rc\<T> and RefCell\<T>"
[^book-weak]: The Rust Programming Language, 15.6 Reference Cycles Can Leak Memory — <https://doc.rust-lang.org/1.95.0/book/ch15-06-reference-cycles.html> — "Weak references don’t express an ownership relationship, and their count doesn’t affect when an Rc\<T> instance is cleaned up."
[^book-weak-count]: The Rust Programming Language, 15.6 Reference Cycles Can Leak Memory — <https://doc.rust-lang.org/1.95.0/book/ch15-06-reference-cycles.html> — "The difference is the weak_count doesn’t need to be 0 for the Rc\<T> instance to be cleaned up."
[^weak-alloc]: Rust std 1.95.0, `rc::Weak` — <https://doc.rust-lang.org/1.95.0/std/rc/struct.Weak.html> — "Note however that a Weak reference does prevent the allocation itself (the backing store) from being deallocated."
[^weak-upgrade]: Rust std 1.95.0, `rc::Weak` — <https://doc.rust-lang.org/1.95.0/std/rc/struct.Weak.html> — "The allocation is accessed by calling upgrade on the Weak pointer, which returns an Option\<Rc\<T>>."
[^weak-cycle]: Rust std 1.95.0, `rc::Weak` — <https://doc.rust-lang.org/1.95.0/std/rc/struct.Weak.html> — "It is also used to prevent circular references between Rc pointers, since mutual owning references would never allow either Rc to be dropped."
[^forget]: Rust std 1.95.0, `mem::forget` — <https://doc.rust-lang.org/1.95.0/std/mem/fn.forget.html> — "Takes ownership and “forgets” about the value without running its destructor."
[^forget-linger]: Rust std 1.95.0, `mem::forget` — <https://doc.rust-lang.org/1.95.0/std/mem/fn.forget.html> — "Any resources the value manages, such as heap memory or a file handle, will linger forever in an unreachable state."
[^forget-safe]: Rust std 1.95.0, `mem::forget` (Safety) — <https://doc.rust-lang.org/1.95.0/std/mem/fn.forget.html> — "forget is not marked as unsafe, because Rust’s safety guarantees do not include a guarantee that destructors will always run."
[^ref-forget-sound]: The Rust Reference, Destructors (Not running destructors) — <https://doc.rust-lang.org/1.95.0/reference/destructors.html> — "types may not safely rely on a destructor being run for soundness."
[^box-leak]: Rust std 1.95.0, `Box::leak` — <https://doc.rust-lang.org/1.95.0/std/boxed/struct.Box.html> — "Consumes and leaks the Box, returning a mutable reference, &'a mut T."
[^box-leak-drop]: Rust std 1.95.0, `Box::leak` — <https://doc.rust-lang.org/1.95.0/std/boxed/struct.Box.html> — "Dropping the returned reference will cause a memory leak."
[^book-leak]: The Rust Programming Language, 15.6 Reference Cycles Can Leak Memory — <https://doc.rust-lang.org/1.95.0/book/ch15-06-reference-cycles.html> — "Preventing memory leaks entirely is not one of Rust’s guarantees, meaning memory leaks are memory safe in Rust."
[^nomicon-leak]: The Rustonomicon, Leaking — <https://doc.rust-lang.org/1.95.0/nomicon/leaking.html> — "Many people like to believe that Rust eliminates resource leaks. In practice, this is basically true. You would be surprised to see a Safe Rust program leak resources in an uncontrolled way. However from a theoretical perspective this is absolutely not the case, no matter how you look at it."
[^malloc-man]: malloc(3), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/malloc.3.en.html> — "The malloc() function allocates size bytes and returns a pointer to the allocated memory. The memory is not initialized."
[^ostep-uninit]: OSTEP, ch.14 Interlude: Memory API — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-api.pdf> — "your program will eventually encounter an uninitialized read, where it reads from the heap some data of unknown value."
[^maybe-doc]: Rust std 1.95.0, `MaybeUninit` — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "A wrapper type to construct uninitialized instances of T."
[^maybe-invariant]: Rust std 1.95.0, `MaybeUninit` (Initialization invariant) — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "The compiler, in general, assumes that a variable is properly initialized according to the requirements of the variable’s type. For example, a variable of reference type must be aligned and non-null."
[^maybe-zeroed]: Rust std 1.95.0, `MaybeUninit` (Initialization invariant) — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "zero-initializing a variable of reference type causes instantaneous undefined behavior, no matter whether that reference ever gets used to access memory"
[^maybe-exploit]: Rust std 1.95.0, `MaybeUninit` (Initialization invariant) — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "This is exploited by the compiler for various optimizations, such as eliding run-time checks and optimizing enum layout."
[^maybe-signal]: Rust std 1.95.0, `MaybeUninit` (Examples) — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "MaybeUninit\<T> serves to enable unsafe code to deal with uninitialized data. It is a signal to the compiler indicating that the data here might not be initialized:"
[^maybe-layout]: Rust std 1.95.0, `MaybeUninit` (Layout) — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "MaybeUninit\<T> is guaranteed to have the same size, alignment, and ABI as T:"
[^nomicon-nogc]: The Rustonomicon, Ownership and Lifetimes — <https://doc.rust-lang.org/1.95.0/nomicon/ownership.html> — "It allows Rust to be completely memory-safe and efficient, while avoiding garbage collection."
[^ostep-dangling]: OSTEP, ch.14 Interlude: Memory API — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-api.pdf> — "such a mistake is called a dangling pointer, and it, as you can guess, is also a bad thing."
[^book-dangling]: The Rust Programming Language, 4.2 References and Borrowing — <https://doc.rust-lang.org/1.95.0/book/ch04-02-references-and-borrowing.html> — "the compiler guarantees that references will never be dangling references"
[^book-lifetime-aim]: The Rust Programming Language, 10.3 Validating References with Lifetimes — <https://doc.rust-lang.org/1.95.0/book/ch10-03-lifetime-syntax.html> — "The main aim of lifetimes is to prevent dangling references"
[^e0597]: Rust error index 1.95.0, E0597 — <https://doc.rust-lang.org/1.95.0/error_codes/E0597.html> — "This error occurs because a value was dropped while it was still borrowed."
[^e0381]: Rust error index 1.95.0, E0381 — <https://doc.rust-lang.org/1.95.0/error_codes/E0381.html> — "It is not allowed to use or capture an uninitialized variable."
[^ostep-overflow]: OSTEP, ch.14 Interlude: Memory API — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-api.pdf> — "A related error is not allocating enough memory, sometimes called a buffer"
[^book-overread-c]: The Rust Programming Language, 9.1 Unrecoverable Errors with panic! — <https://doc.rust-lang.org/1.95.0/book/ch09-01-unrecoverable-errors-with-panic.html> — "In C, attempting to read beyond the end of a data structure is undefined behavior."
[^book-panic]: The Rust Programming Language, 9.1 Unrecoverable Errors with panic! — <https://doc.rust-lang.org/1.95.0/book/ch09-01-unrecoverable-errors-with-panic.html> — "In this situation, Rust will panic."
[^book-datarace]: The Rust Programming Language, 4.2 References and Borrowing — <https://doc.rust-lang.org/1.95.0/book/ch04-02-references-and-borrowing.html> — "Rust prevents this problem by refusing to compile code with data races!"
[^book-ref-rule]: The Rust Programming Language, 4.2 References and Borrowing — <https://doc.rust-lang.org/1.95.0/book/ch04-02-references-and-borrowing.html> — "At any given time, you can have either one mutable reference or any number of immutable references."
[^book-stack-fixed]: The Rust Programming Language, 4.1 What Is Ownership? — <https://doc.rust-lang.org/1.95.0/book/ch04-01-what-is-ownership.html> — "All data stored on the stack must have a known, fixed size. Data with an unknown size at compile time or a size that might change must be stored on the heap instead."
[^book-push-faster]: The Rust Programming Language, 4.1 What Is Ownership? — <https://doc.rust-lang.org/1.95.0/book/ch04-01-what-is-ownership.html> — "Pushing to the stack is faster than allocating on the heap because the allocator never has to search for a place to store new data"
[^book-heap-alloc]: The Rust Programming Language, 4.1 What Is Ownership? — <https://doc.rust-lang.org/1.95.0/book/ch04-01-what-is-ownership.html> — "The memory allocator finds an empty spot in the heap that is big enough, marks it as being in use, and returns a pointer, which is the address of that location."
[^std-guard]: rust-lang/rust tag 1.95.0, library/std/src/sys/pal/unix/stack_overflow.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/pal/unix/stack_overflow.rs> — "Signal handler for the SIGSEGV and SIGBUS handlers. We've got guard pages (unmapped pages) at the end of every thread's stack, so if a thread ends up running into the guard page it'll trigger this handler."
