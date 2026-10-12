# P10 · Chương 04 — Monomorphization, static vs dynamic dispatch, zero-cost abstraction

> Chương này chỉ đọc tài liệu và source Rust 1.95.0 (Book, Reference, rustc-dev-guide, Cargo book, std-dev-guide, source `library/`); **không có thí nghiệm mới**. Quan sát asm đã có ở [P10·03 mục 4](./03-dst-fat-pointer-vtable.md) và [P9·06 mục 5](../09-rust-language/06-iterator-va-closure.md) được nhắc lại bằng link, không lặp. Khẳng định về *cách rustc cài đặt* (collector, codegen unit) là chi tiết cài đặt của rustc-dev-guide, có thể đổi giữa các phiên bản.

## Bạn sẽ hiểu được gì

1. **Q1.** Monomorphization là gì, compiler làm nó ở đâu và khi nào? → mục 1
2. **Q2.** Static dispatch và dynamic dispatch khác nhau thế nào ở lời gọi hàm? → mục 2
3. **Q3.** Khi nào chọn generic, khi nào chọn `dyn Trait`; vì sao có trait không làm trait object được? → mục 3
4. **Q4.** Closure đi vào monomorphization ra sao; `impl Fn` khác `Box<dyn Fn>` ở đâu? → mục 4
5. **Q5.** Vì sao chuỗi iterator được gọi là "zero-cost", và "zero-cost" *không* có nghĩa là gì? → mục 5
6. **Q6.** Cái giá của monomorphization (code bloat, compile time) nằm ở đâu và cách giảm? → mục 6
7. **Q7.** `#[inline]`, codegen unit và LTO quyết định điều gì? → mục 7

## Kiến thức cần trước

- [P9 · Chương 02 — Trait nhìn sâu](../09-rust-language/02-trait.md) và [P9 · Chương 03 — Generics nâng cao](../09-rust-language/03-generics-nang-cao.md): trait bound, `impl Trait`.
- [P10 · Chương 01 — Pipeline của rustc](./01-pipeline-rustc.md): mục 5 nói monomorphization nằm ở đâu trong pipeline từ source đến binary. Chương này đi sâu vào dispatch và chi phí, không lặp lại phần vị trí đó.
- [P10 · Chương 03 — DST, fat pointer và vtable](./03-dst-fat-pointer-vtable.md): `&dyn Trait` là (data ptr, vtable ptr), vtable chứa gì, dyn compatibility. Chương này chỉ tóm lại để so sánh.
- [P9 · Chương 06 — Iterator và closure](../09-rust-language/06-iterator-va-closure.md): closure là struct ẩn danh, `Fn`/`FnMut`/`FnOnce`, adapter lười. Chương này chỉ nói phần monomorphization.

## 1. Monomorphization: một bản mã cho mỗi kiểu (Q1)

> **Trả lời ngắn.** Monomorphization là bước compiler biến code generic thành code cụ thể bằng cách điền các kiểu thật được dùng vào[^book-mono]. Compiler "đóng dấu" một bản riêng của hàm generic cho mỗi kiểu cụ thể cần dùng[^dev-mono]. Đây là bước đầu tiên của backend rustc[^dev-mono-first].

**Giải thích.**

Hình dung (nhận xét của người soạn): bạn có một khuôn bánh in được mọi hình. Máy nướng không biết nướng "hình bất kỳ"; nó chỉ nướng được cái bánh có hình cụ thể. Nên trước khi nướng, người thợ phải dập ra một khuôn riêng cho từng hình đơn hàng yêu cầu. Generic là bản vẽ khuôn tổng quát; monomorphization là việc dập khuôn cụ thể.

*Vì sao phải làm.* Assembly không có generic, nên mọi kiểu generic phải được biết cụ thể trước khi code chạy[^dev-mono-asm]. Ngôn ngữ khác xử lý bằng cách khác: rustc-dev-guide lấy Java làm ví dụ, nơi có thể không biết kiểu chính xác của một giá trị cho tới lúc chạy[^dev-mono-java]; Rust chọn monomorphize toàn bộ kiểu generic[^dev-mono-all].

*Hiệu ứng nhìn thấy được.* Ví dụ của rustc-dev-guide: dùng `Vec<u64>` và `Vec<String>` thì binary có hai bản mã sinh ra của `Vec`, một cho mỗi kiểu[^dev-mono-vec]. Book minh họa bằng `Option<T>`: compiler đọc các chỗ dùng, thấy hai loại `Option<T>` (với `i32` và `f64`) và bung định nghĩa generic thành hai định nghĩa chuyên biệt[^book-mono-expand]. Vì mã sinh ra đã gắn chặt với kiểu cụ thể, chạy lên nó như thể bạn tự viết tay mỗi định nghĩa[^book-mono-hand].

*Cơ chế từng bước ở mức khái niệm:*

1. **Collection (thu thập).** Compiler xác định những kiểu cụ thể nào cần dùng cho mọi thứ generic trong chương trình; việc này gọi là collection, và đoạn code làm nó là *monomorphization collector*[^dev-mono-collect-what]. Ví dụ trong rustc-dev-guide: `main` gọi `banana`, `banana` gọi `peach::<u64>`; collector trả `[main, banana, peach::<u64>]` — những hàm sẽ được sinh mã máy[^dev-mono-collect]; collector còn thêm cả `static` vào danh sách[^dev-mono-statics]. Hàm generic không ai dùng với kiểu nào thì không có trong danh sách (suy luận của người soạn từ định nghĩa "danh sách hàm sẽ được sinh mã").
2. **Partition (chia codegen unit).** Query `collect_and_partition_mono_items` làm thu thập rồi chia các mục thành *codegen unit* (CGU)[^dev-mono-query]. CGU được giải thích ở mục 6–7.
3. **Sinh mã.** Mỗi mục trong danh sách là một hàm sẽ có mã máy được sinh ra cho nó[^dev-mono-collect], với kiểu đã cụ thể.

Về *khi nào*: rustc-dev-guide nói collector chạy ngay trước *MIR lowering* và codegen[^dev-mono-before-codegen]. Vị trí của collection và của việc monomorphize thật sự trong cả pipeline có ở [P10·01 mục 5](./01-pipeline-rustc.md), chương này không lặp lại.

```mermaid
flowchart LR
  G["fn peach&lt;T&gt;() — một định nghĩa generic"] --> C["collector: ai gọi peach với T nào?"]
  C --> I1["peach::&lt;u64&gt; — bản mã 1"]
  C --> I2["peach::&lt;String&gt; — bản mã 2"]
  I1 --> CGU["chia vào codegen unit"]
  I2 --> CGU
```

(Sơ đồ là minh họa của người soạn dựa trên mô tả collector; `peach::<String>` là ví dụ thêm, không phải của nguồn.)

Hệ quả: lời gọi `peach::<u64>()` là lời gọi tới một hàm cụ thể mà đích biết lúc biên dịch. Đó là *static dispatch* (mục 2)[^book-dispatch].

<details><summary>Đi sâu: Book hứa gì, và ai sinh bản mã khi hàm generic nằm ở crate khác</summary>

- Book hứa rằng dùng generic không làm chương trình chạy chậm hơn dùng kiểu cụ thể[^book-generics-nocost]. Cơ chế của lời hứa là: compiler nhìn mọi chỗ gọi code generic và sinh code cho đúng các kiểu mà nó được gọi với[^book-mono-callsites].
- Bảng của rustc-dev-guide về hàm của Crate A được Crate B dùng nói rằng với hàm generic, mọi hàm đã monomorphize từ Crate A xuất hiện trong một codegen unit của Crate B, bất kể có inline hay không[^dev-cgu-generic]; còn hàm *không* generic và không `#[inline]` thì không xuất hiện trong CGU nào của Crate B[^dev-cgu-nongeneric]. (Suy luận của người soạn: vì vậy crate dùng phải nhận đủ thông tin từ crate thư viện để sinh mã cho kiểu của mình; hai crate cùng dùng `Vec<u64>` có thể mỗi bên có một bản — nguồn ở đây không nói chi tiết việc dùng chung bản, và bảng chỉ nói về hàm của A được B dùng.)
- `impl Trait` ở vị trí đối số là cú pháp tắt cho tham số kiểu generic, chỉ khác là kiểu ẩn danh và không nằm trong danh sách generic parameter[^ref-impl-arg]; nên (suy luận của người soạn) `fn f(x: impl Display)` cũng bị monomorphize như `fn f<T: Display>(x: T)`.

</details>

**Quay lại Q1:** monomorphization là dập một bản mã riêng cho mỗi kiểu cụ thể được dùng; collector tìm ra danh sách bản cần dập, rồi backend sinh mã cho từng bản.

## 2. Static dispatch và dynamic dispatch (Q2)

> **Trả lời ngắn.** Static dispatch là compiler biết *lúc biên dịch* sẽ gọi method nào; mã sinh ra từ monomorphization là như vậy[^book-dispatch]. Dynamic dispatch là compiler không biết, nên sinh mã tự tìm method lúc chạy[^book-dispatch-dyn]. Book nói khi dùng trait object thì Rust phải dùng dynamic dispatch[^book-dyn-must]; trình tối ưu đôi khi vẫn bỏ được vtable khi thấy rõ kiểu thật (quan sát ở P10·03, xem phần Đi sâu).

**Giải thích.**

Hình dung (nhận xét của người soạn): static dispatch như gọi điện thẳng số của một người cụ thể — số nằm sẵn trong danh bạ in từ trước. Dynamic dispatch như gọi tổng đài: bạn gọi một số cố định, tổng đài tra "hôm nay ai trực" rồi nối máy; thêm một bước tra cứu mỗi cuộc gọi.

*Static.* Với `fn f<T: Trait>(x: &T) { x.method() }`, mỗi bản `f::<A>`, `f::<B>` đã biết `T`, nên `x.method()` trỏ thẳng tới `A::method` hay `B::method`. Đích của lời gọi biết ở compile time[^book-dispatch].

*Dynamic.* Với `fn f(x: &dyn Trait) { x.method() }`, chỉ có một bản `f` (vì hàm không generic — suy luận của người soạn). `dyn Trait` là kiểu trait object: giá trị "đục" của một kiểu nào đó có implement trait[^ref-to-intro]; `&dyn Trait` là con trỏ tới giá trị đó. Mỗi con trỏ tới trait object chứa con trỏ tới instance và con trỏ tới vtable; vtable chứa, cho mỗi method của trait (và supertrait) mà kiểu đó implement, một con trỏ tới implementation[^ref-to-vtable]. Gọi method trên trait object là virtual dispatch: lúc chạy tải một function pointer từ vtable rồi gọi gián tiếp[^ref-to-virtual]. Book nói bước tra cứu này là một chi phí lúc chạy mà static dispatch không có[^book-dyn-cost].

```mermaid
flowchart LR
  subgraph Static["static: f::&lt;A&gt; (monomorphized)"]
    s1["x.method()"] --> s2["A::method — đích biết lúc biên dịch"]
  end
  subgraph Dynamic["dynamic: f(&dyn Trait)"]
    d1["x.method()"] --> d2["tải fn ptr từ vtable"] --> d3["gọi gián tiếp"]
  end
```

Chương 03 mục 4 có asm AArch64 (quan sát trên một compiler): lời gọi `dyn` là một lệnh load con trỏ từ vtable rồi một lệnh nhảy gián tiếp, còn lời gọi generic là `bl` tới hàm đã monomorphize (lệnh `bl` thấy ở `_use_generic` trong mục đó; [P10·03 mục 4](./03-dst-fat-pointer-vtable.md)). Cấu trúc vtable chi tiết, kể cả phần ngoài các con trỏ method, cũng ở mục đó; ở đây ta chỉ cần biết hai điều: một bản mã dùng chung cho mọi kiểu ở phía gọi `dyn`, và một bước tra bảng ở mỗi lời gọi.

Chọn dispatch nào là chọn *ở chỗ viết kiểu*, không phải một cờ biên dịch (nhận xét của người soạn): `T: Trait` hay `impl Trait` là static, `&dyn Trait` hay `Box<dyn Trait>` là dynamic.

<details><summary>Đi sâu: <code>dyn Trait</code> cũng là một kiểu để điền vào generic</summary>

- Trait object implement chính base trait, auto trait và các supertrait của nó[^ref-to-implements]. Nghĩa là `dyn Trait` có thể điền vào generic `T: Trait`... nhưng chỉ khi `T` được phép không có `Sized`: tham số kiểu mặc định là `Sized`[^ref-sized-implicit] và phải dùng bound đặc biệt `?Sized` để nới[^ref-sized-relax], vì trait object là DST (kiểu có kích thước chỉ biết lúc chạy)[^ref-to-unsized]. (`?Sized` và DST: [P10·03 mục 1](./03-dst-fat-pointer-vtable.md).)
- Hệ quả (suy luận của người soạn từ hai điểm trên): với `fn f<T: Trait + ?Sized>(x: &T)`, gọi `f::<dyn Trait>` là *một* instance monomorphize duy nhất mà bên trong dùng dynamic dispatch. Nên "generic" và "dyn" không loại trừ nhau: generic quyết định *số bản mã*, còn `T` là gì quyết định lời gọi bên trong là tĩnh hay động.
- Chương 03 quan sát rằng khi compiler thấy rõ kiểu cụ thể phía sau `&dyn`, nó vẫn có thể inline và bỏ vtable ([P10·03 mục 4](./03-dst-fat-pointer-vtable.md), phần Đi sâu). Đó là hành vi tối ưu quan sát được, không phải bảo đảm của ngôn ngữ.

</details>

**Quay lại Q2:** static = đích gọi biết lúc biên dịch (nhờ monomorphization); dynamic = tra vtable ở mỗi lời gọi và chỉ có một bản mã.

## 3. Generic hay `dyn`: trade-off và dyn compatibility (Q3)

> **Trả lời ngắn.** Generic chỉ nhận *một* kiểu cụ thể mỗi lần, còn trait object cho phép nhiều kiểu khác nhau lấp chỗ đó lúc chạy[^book-generic-vs-dyn]. Generic đổi lấy tốc độ chạy bằng binary lớn hơn và compile lâu hơn[^dev-mono-cost]; `dyn` thì mỗi lời gọi tốn một bước tra cứu và ngăn compiler inline[^book-dyn-cost][^book-inline], đổi lại chỉ cần một bản mã (suy luận của người soạn từ mục 2). Chỉ trait "dyn compatible" mới làm được trait object[^ref-to-base][^ref-dyncompat].

**Giải thích.**

Hình dung (nhận xét của người soạn): bạn có một giỏ chứa "các thứ biết `draw()`". Nếu giỏ là generic `Vec<T>` thì cả giỏ chỉ chứa toàn hình tròn *hoặc* toàn hình vuông — nhưng mỗi lần lấy ra thì máy biết ngay là hình gì. Nếu giỏ là `Vec<Box<dyn Draw>>` thì lẫn lộn được, nhưng mỗi lần lấy phải nhìn nhãn (vtable) mới biết.

Ví dụ trong Book: `Vec<Box<dyn Draw>>` cho phép vector chứa nhiều kiểu khác nhau cùng implement `Draw`; với generic `T: Draw` thì mọi phần tử phải cùng một kiểu `T`[^book-generic-vs-dyn]. Đây là thứ trait object mang lại: *tập kiểu mở lúc chạy* (diễn giải của người soạn).

Bảng tóm tắt (tổng hợp của người soạn từ các câu đã dẫn ở mục 1–2, không có nguồn riêng cho bảng):

| | Generic / `impl Trait` | `dyn Trait` |
|---|---|---|
| Số bản mã của hàm gọi | một bản mỗi kiểu[^dev-mono] | một bản chung (impl của từng kiểu vẫn có riêng; suy luận) |
| Đích lời gọi | biết lúc biên dịch[^book-dispatch] | tra vtable lúc chạy[^ref-to-virtual] |
| Inline | tạo điều kiện (mục 7) | Book nói dynamic dispatch ngăn inline[^book-inline]; chương 03 quan sát ngoại lệ khi compiler thấy rõ kiểu |
| Tập kiểu | cố định lúc biên dịch, một kiểu mỗi lần | mở, nhiều kiểu lẫn nhau[^book-generic-vs-dyn] |
| Chi phí chính | compile time + binary size[^dev-mono-cost] | tra bảng[^book-dyn-cost] + mất một số tối ưu[^book-inline] |

*Dyn compatibility.* Chỉ trait dyn compatible[^ref-dyncompat] (trước đây gọi là object safety[^ref-dyn-formerly]) mới làm được trait object. Riêng method generic thì không dispatch được: hàm "dispatchable" không được có type parameter[^ref-dyn-generic], vì generic không tương thích với vtable[^ref-dyn-why-generic]. Điều kiện đầy đủ, lý do với `Self`, `where Self: Sized`, supertrait và upcasting nằm ở [P10·03 mục 5](./03-dst-fat-pointer-vtable.md) và [mục 6](./03-dst-fat-pointer-vtable.md).

<details><summary>Đi sâu: chọn thế nào khi viết API backend</summary>

- (Nhận xét của người soạn.) `Vec<Box<dyn Handler>>` hợp với đăng ký handler/middleware/plugin lúc chạy: tập kiểu mở và mỗi lời gọi gắn với I/O hay công việc lớn hơn nhiều một bước tra bảng. Hot loop xử lý từng byte hay từng record thì generic thường hợp lý hơn vì inline được — nhưng hãy đo, đừng đoán.
- Book chỉ nói dynamic dispatch "ngăn compiler chọn inline" và "ngăn một số tối ưu"[^book-inline]; nó không đưa số đo chi phí, nên chương này cũng không.
- Không gì ép phải chọn một trong hai cho cả chương trình: kiểu *ở biên* có thể là `dyn`, còn bên trong là generic (mục 6).

</details>

**Quay lại Q3:** generic = một kiểu mỗi lần, nhanh và inline được nhưng nhiều bản mã; `dyn` = nhiều kiểu lúc chạy, một bản mã nhưng gián tiếp; chỉ trait dyn compatible mới làm `dyn` được.

## 4. Closure và monomorphization (Q4)

> **Trả lời ngắn.** Mỗi closure có một kiểu duy nhất, ẩn danh, không viết ra được[^ref-closure-struct]. Vì vậy hàm generic nhận closure được monomorphize một bản *cho mỗi closure* và gọi closure là static dispatch (suy luận của người soạn, nối từ mục 1–2). `Box<dyn Fn()>` thì ngược lại: là trait object, có vtable và gọi gián tiếp[^ref-to-virtual].

**Giải thích.**

Hình dung (nhận xét của người soạn): mỗi closure như một cây bút có khắc tên riêng. Một hàm `run<F>` nhận "bất kỳ cây bút nào" sẽ có một bản riêng cho mỗi cây bút bạn đưa; còn hộp bút `Box<dyn Fn()>` chấp nhận mọi cây bút nhưng phải mở nắp xem nhãn mỗi lần dùng.

*Kiểu duy nhất.* Reference: closure expression sinh một giá trị có kiểu duy nhất và ẩn danh, gần tương đương một struct chứa các giá trị capture[^ref-closure-struct]. Chi tiết capture, `Fn`/`FnMut`/`FnOnce` nằm ở [P9·06 mục 1–3](../09-rust-language/06-iterator-va-closure.md); ở đây chỉ cần "mỗi closure là một kiểu riêng".

*Hệ quả với generic.* `fn run<F: Fn()>(f: F)` có một instance cho mỗi kiểu `F` được truyền vào; hai closure khác nhau — dù thân giống hệt — là hai kiểu và cho hai instance ở mức monomorphization (suy luận của người soạn từ "kiểu duy nhất" + mục 1; chương này không khẳng định số hàm trong binary cuối cùng). Vì kiểu `F` cụ thể, `f()` bên trong là lời gọi tĩnh tới thân closure đó nên có thể inline (suy luận của người soạn từ mục 2); [P9·06 mục 5](../09-rust-language/06-iterator-va-closure.md) đã soi asm một chuỗi iterator + closure biến mất hoàn toàn (quan sát một trường hợp).

*Hệ quả với `impl Trait`.* Reference: `impl Trait` ở vị trí trả về cho phép trả một abstract type *không boxed*, đặc biệt hữu ích với closure và iterator[^ref-impl-ret-unboxed]. Trước khi có nó, cách duy nhất trả closure là `Box<dyn Fn>`, có thể kéo theo heap allocation và dynamic dispatch[^ref-impl-closure-cost]. Giá phải trả: mọi nhánh trả về phải cùng một kiểu cụ thể[^ref-impl-ret-same]. Nếu các nhánh trả closure khác nhau thì (suy luận từ điều kiện đó) phải dùng `Box<dyn Fn>`, tức quay về dynamic dispatch.

```mermaid
flowchart TD
  Q{"closure đi vào đâu?"}
  Q -->|"tham số F: Fn() / impl Fn()"| M["monomorphize một bản mỗi closure → static dispatch, inline được"]
  Q -->|"trả impl Fn() (một kiểu)"| M2["không heap, static dispatch"]
  Q -->|"Box&lt;dyn Fn()&gt;"| D["vtable + gọi gián tiếp; closure thường nằm trên heap"]
```

<details><summary>Đi sâu: kích thước closure và <code>dyn Fn</code> (quan sát)</summary>

- Reference chỉ nói kiểu closure xấp xỉ một struct chứa các giá trị capture[^ref-closure-struct] và nói rõ closure không có bảo đảm layout nào[^ref-closure-nolayout] (xem thêm [P10·02 mục 1](./02-type-layout.md), phần Đi sâu), nên mọi con số về kích thước là quan sát, không phải hợp đồng. Số đo aarch64 đã có ở [P9·06 mục 3](../09-rust-language/06-iterator-va-closure.md): closure không capture gì = 0 byte; closure `move` một `String` = 24 byte; closure chỉ mượn thì chứa tham chiếu (8 byte mỗi tham chiếu); closure `move` một mảng 100 byte = 100 byte. Chương này không đo thêm.
- `Box<dyn Fn()>` là fat pointer (data, vtable) giữ closure trong `Box`, thường kéo theo heap allocation[^ref-impl-closure-cost]; việc *gọi* closure đi qua vtable như mọi trait object ([P10·03 mục 4](./03-dst-fat-pointer-vtable.md)).
- Chọn trong thực tế (nhận xét của người soạn): callback truyền vào hàm tiện ích nên là generic (`F: Fn…` hoặc `impl Fn…`); bảng route/hook đăng ký lúc chạy với nhiều closure khác nhau trong cùng một `Vec` cần `Box<dyn Fn…>`.

</details>

**Quay lại Q4:** mỗi closure là một kiểu, nên generic nhận closure sinh một bản mã mỗi closure và gọi tĩnh; `Box<dyn Fn>` đổi điều đó lấy khả năng gom nhiều closure vào một kiểu.

## 5. Iterator và zero-cost abstraction (Q5)

> **Trả lời ngắn.** Book nói iterator tuy là abstraction mức cao nhưng được biên dịch xuống mã gần như tương đương code vòng lặp mức thấp viết tay[^book-iter-perf]. "Zero-cost" nghĩa là dùng abstraction không thêm overhead *lúc chạy*[^book-zero-cost]. Nó không có nghĩa là "không tốn compile time hay kích thước binary": monomorphization chính là khoản chi đó[^dev-mono-cost] (nối hai nguồn: suy luận của người soạn).

**Giải thích.**

Hình dung (nhận xét của người soạn): bạn thuê một đầu bếp chuyên nghiệp mà cho ra món y hệt khi bạn tự nấu theo công thức chi tiết, nhưng bạn khỏi phải đọc công thức. Cái "giá" không biến mất: nó chuyển sang người chuẩn bị (compiler) làm việc nhiều hơn trước giờ ăn.

*Iterator là gì ở tầng kiểu.* Iterator và adapter là lười: tạo ra chưa làm gì đáng kể, chỉ khi gọi `next` mới có việc[^std-iter-lazy]. Adapter như `map` là một struct generic: `Iterator::map` trong source 1.95.0 có chữ ký `fn map<B, F>(self, f: F) -> Map<Self, F>`[^src-iter-map], và `Map<I, F>` là struct giữ iterator con `I` và closure `F` làm field[^src-map-struct]. Nó implement `Iterator` với ràng buộc `F: FnMut(I::Item) -> B`[^src-map-impl].

*Từ kiểu tới mã.* Kết hợp mục 1 và mục 4 (suy luận của người soạn): `v.iter().map(|x| x * 2).sum()` có một kiểu cụ thể duy nhất, dạng `Map<slice::Iter<..>, {closure}>`, trong đó closure là kiểu riêng. Mọi tham số kiểu đều là kiểu cụ thể, nên không có chỗ nào cần vtable: `next` của `Map` gọi `next` của iterator con rồi áp closure[^src-map-next], tất cả là lời gọi tĩnh giữa các hàm đã biết, đủ điều kiện để inline và tối ưu như một vòng lặp duy nhất. Book cũng xác nhận chiều ngược lại: dynamic dispatch ngăn compiler chọn inline[^book-inline].

*Book hứa gì.* Book dùng định nghĩa zero-overhead của Stroustrup: cái bạn không dùng thì không trả tiền, và cái bạn dùng thì bạn không thể tự viết tay tốt hơn[^book-stroustrup]. Book nêu thêm rằng các tối ưu như loop unrolling và loại bỏ bounds check trên truy cập mảng áp dụng được ở đây[^book-iter-opts]. Asm một trường hợp cụ thể (quan sát, không phải bảo đảm) ở [P9·06 mục 5](../09-rust-language/06-iterator-va-closure.md), phần Đi sâu.

*Hiểu lầm cần tránh.*

- "Zero-cost" là cam kết về *runtime so với code tay tương đương*[^book-zero-cost]. Compile time và kích thước binary vẫn tăng khi có nhiều instance (mục 6)[^dev-mono-cost].
- Book chỉ nói mã sinh ra *gần như* tương đương mã viết tay[^book-iter-perf], không phải luôn giống hệt trên mọi cấu hình. [P9·06 mục 5](../09-rust-language/06-iterator-va-closure.md) đã lưu ý build debug (không `-O`) khác (suy luận của người soạn; P9·06 không đo).
- `Box<dyn Iterator>` là lựa chọn dynamic dispatch, vì Book nói trait object luôn dùng dynamic dispatch[^book-dyn-must]. `Box<I>` implement `Iterator` (cho cả `I: ?Sized`) bằng cách chuyển tiếp `(**self).next()`[^src-box-iter-next]; với `I = dyn Iterator`, lời gọi này đi qua vtable (suy luận của người soạn từ mục 2). Mỗi lần gọi `next` thường là một lời gọi gián tiếp — đánh đổi hợp lý khi các nhánh trả iterator khác kiểu ([P9·06 mục 4](../09-rust-language/06-iterator-va-closure.md), phần trait object cho iterator), nhưng theo định nghĩa "zero-cost" ở trên thì bạn đã rời con đường đó (suy luận của người soạn).

<details><summary>Đi sâu: <code>#[inline]</code> trong chính adapter của std</summary>

- Trong source `core` 1.95.0, `next` của `Map` mang `#[inline]`: dòng `#[inline]` đứng ngay trước `fn next(&mut self) -> Option<B>`[^src-map-next-sig]. Hãy hiểu đây chỉ là *gợi ý* (mục 7): thuộc tính `inline` chỉ đề nghị đặt bản copy vào nơi gọi[^ref-inline], không bắt buộc.
- Phần `fold` của `Map` được viết riêng để chuyển tiếp vào `fold` của iterator con[^src-map-fold]. (Nhận xét của người soạn: nhờ vậy nhiều consumer không đi qua từng `next`, nhưng đó là tối ưu của từng adapter — nguồn ở đây không hứa mọi chuỗi đều như vậy.)
- Đo ở [P9·06 mục 5](../09-rust-language/06-iterator-va-closure.md): hai hàm cộng gấp đôi các phần tử `u32`, vòng `for` với chỉ số và iterator, cho cùng khung lệnh trên aarch64 `-O` (quan sát một trường hợp). Không suy ra mọi chuỗi iterator đều free.

</details>

**Quay lại Q5:** chuỗi adapter là một kiểu cụ thể, được monomorphize thành mã tĩnh nên compiler gộp được như vòng lặp viết tay; "zero-cost" chỉ nói về runtime, còn compile time và binary size là cái giá của monomorphization.

## 6. Cái giá: code bloat, compile time và cách giảm (Q6)

> **Trả lời ngắn.** Monomorphization cho chương trình nhanh nhưng đổi bằng thời gian biên dịch (tạo hết các bản sao tốn thời gian) và kích thước binary (các bản sao chiếm chỗ)[^dev-mono-cost]. Hai cách giảm được nêu ở mục này: đẩy phần thân không phụ thuộc kiểu ra hàm không generic (mẫu `inner` có thật trong std[^src-fs-read-inner]), hoặc dùng `dyn` ở những chỗ không nóng (nhận xét của người soạn).

**Giải thích.**

Hình dung (nhận xét của người soạn): in sách giáo khoa bằng khuôn riêng cho mỗi lớp thì mỗi cuốn nhanh, nhưng xưởng in tốn thời gian làm khuôn và kho chất đầy bản gần giống nhau. Giải pháp xưởng: phần chung (bìa, mục lục) in một lần, chỉ phần khác lớp mới làm khuôn riêng.

*Vấn đề.* Compiler đóng dấu một bản mã cho mỗi kiểu cụ thể cần dùng[^dev-mono], nên số instance ≈ số cặp (hàm generic, bộ tham số kiểu) được dùng; hàm generic thân lớn gọi với 20 kiểu cho 20 bản thân lớn (cả hai là suy luận của người soạn từ "một bản mỗi kiểu"). Hệ quả đã nêu trong rustc-dev-guide là compile time và binary size[^dev-mono-cost]; nguồn không đưa con số, nên chương này cũng không.

*Cách giảm 1 — hàm `inner` không generic.* Source std 1.95.0 cho một ví dụ: `fs::read` là `pub fn read<P: AsRef<Path>>(path: P) -> io::Result<Vec<u8>>`[^src-fs-read-sig], còn toàn bộ thân việc đọc file nằm trong hàm lồng `fn inner(path: &Path) -> io::Result<Vec<u8>>`[^src-fs-read-inner], và hàm ngoài chỉ gọi `inner(path.as_ref())`[^src-fs-read-call]. Mẫu cũng xuất hiện ở `read_to_string`[^src-fs-rts-inner] và `write`[^src-fs-write-inner]. Suy luận của người soạn (source không giải thích lý do): phần generic mỏng chỉ chuyển `P` thành `&Path`, nên số bản mã nhân bản theo `P` chỉ còn phần mỏng đó; thân lớn có đúng một bản.

```rust
// Minh họa (không chạy): mẫu "generic mỏng bọc hàm thật không generic".
pub fn load<P: AsRef<std::path::Path>>(path: P) -> std::io::Result<Vec<u8>> {
    fn inner(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
        std::fs::read(path) // thân nặng, chỉ một bản
    }
    inner(path.as_ref())
}
```

*Cách giảm 2 — `dyn` ở biên.* Với chỗ không nằm trên hot path (đăng ký plugin, handler cấu hình lúc khởi động), `dyn Trait` cho một bản mã duy nhất (mục 3). Đây là cách chọn có chủ đích giữa hai chi phí, không phải "tối ưu miễn phí" (nhận xét của người soạn).

*Tác động tới CI/backend (nhận xét của người soạn).* Với service lớn, compile time và binary size là chi phí vận hành thật: CI chậm, image container to hơn. Hãy đo bằng công cụ build của bạn trước khi tối ưu; chương này không đưa số.

<details><summary>Đi sâu: vì sao generic ảnh hưởng incremental build</summary>

- Để build incremental nhanh hơn, CGU partitioner tạo hai CGU cho mỗi module cấp source: một cho code "stable" (không generic), một cho code hay thay đổi hơn gồm các instance đã monomorphize/chuyên biệt[^dev-cgu-two].
- Hàm generic từ crate khác được đưa vào CGU của crate dùng[^dev-cgu-generic], nên sửa crate thư viện generic có thể buộc crate dùng sinh lại mã (suy luận của người soạn từ các câu trên).
- Mọi quan sát này là mô tả của rustc-dev-guide về cài đặt hiện tại; không phải bảo đảm của ngôn ngữ.

</details>

**Quay lại Q6:** cái giá là một bản mã mỗi kiểu (compile time + binary size); giảm bằng cách tách phần không phụ thuộc kiểu ra hàm thường (mẫu `inner` trong std) hoặc dùng `dyn` ở nơi không nóng.

## 7. `#[inline]`, codegen unit, LTO (Q7)

> **Trả lời ngắn.** `#[inline]` chỉ *gợi ý* compiler đặt bản copy mã hàm vào nơi gọi thay vì sinh lời gọi[^ref-inline]; rustc vẫn tự inline khi thấy đáng[^ref-inline-auto]. Codegen unit là đơn vị chia crate để sinh mã song song; nhiều CGU hơn có thể giảm compile time nhưng có thể sinh mã chậm hơn[^cargo-cgu], và LTO tối ưu bằng phân tích toàn chương trình, kể cả ở dạng cục bộ chỉ qua các CGU của crate[^cargo-lto][^cargo-lto-local].

**Giải thích.**

Hình dung (nhận xét của người soạn): inline giống việc chép nguyên đoạn hướng dẫn vào tờ giấy của bạn thay vì ghi "xem trang 57". Nhanh hơn khi dùng, nhưng tờ giấy dày lên, và chép sai chỗ làm tờ giấy phình mà chẳng nhanh hơn.

*Inline là gì và ai quyết định.* Reference: thuộc tính `inline` *gợi ý* có nên đặt bản copy của mã hàm vào caller thay vì sinh một lời gọi hay không[^ref-inline]; rustc tự inline khi thấy đáng, và quyết định inline tồi có thể làm chương trình chậm[^ref-inline-auto]. Ba dạng `#[inline]`/`always`/`never` (đều chỉ là gợi ý) và cách kiểm chứng bằng asm: [P2·07 mục 2](../02-cpu/07-rust-va-may.md).

*Liên hệ với monomorphization.* Inline cần compiler biết đích lời gọi và có thân hàm trong tay (suy luận của người soạn). Static dispatch cho biết đích; còn có thân hàm hay không thì tùy loại hàm (xem bảng CGU ở đoạn sau). Trait object thường không biết đích, nên Book nói dynamic dispatch ngăn inline[^book-inline]. Đây là chỗ hai nửa của chương gặp nhau: monomorphization *tạo điều kiện* cho inline (suy luận của người soạn).

*Codegen unit (CGU).* `codegen-units` của Cargo điều khiển số "code generation units" mà một crate được chia ra; nhiều CGU hơn cho phép xử lý song song nhiều hơn, có thể giảm compile time, nhưng có thể sinh mã chậm hơn[^cargo-cgu]. Mặc định là 256 cho build incremental và 16 cho build không incremental[^cargo-cgu-default] (bảng mặc định của các profile: [P9·08 mục 7](../09-rust-language/08-module-crate-cargo.md)). (Suy luận của người soạn: ranh giới CGU hạn chế tối ưu qua lại — đó là lý do "có thể chậm hơn"; khớp với việc Cargo có chế độ LTO cục bộ để tối ưu thêm *qua các CGU* của crate[^cargo-lto-local].)

*Qua ranh giới crate.* Bảng CGU của rustc-dev-guide: hàm không generic *có* `#[inline]` từ Crate A xuất hiện trong một CGU của Crate B[^dev-cgu-inline]; hàm không generic không `#[inline]` thì không xuất hiện trong CGU nào của Crate B[^dev-cgu-nongeneric]. Hướng dẫn của đội thư viện chuẩn (std-dev-guide) mô tả thêm: ở release mode compiler mặc định sinh một hàm `#[inline]` vào *mỗi* CGU có tham chiếu tới nó và thêm gợi ý inline, còn ở debug mode rustc gần như phớt lờ `#[inline]`[^std-inline-cgu]. Đó là lời một người đóng góp được trích trong hướng dẫn, mô tả cài đặt hiện tại nên có thể đổi. Hai mô tả nghe lệch nhau về số CGU (bảng nói "một CGU", lời trích nói "mỗi CGU có tham chiếu tới nó"); nguồn không giải thích chỗ lệch, nên chương này không suy ra con số cụ thể. Cũng theo hướng dẫn, không cần `#[inline]` cho method có generic trong phạm vi[^std-inline-generic]. (Suy luận của người soạn: đó là cách `#[inline]` cho phép inline *giữa* các crate, vì thân hàm có mặt ở crate dùng; hàm generic thì vốn đã được sinh ở crate dùng.)

*LTO.* Link-time optimization tối ưu bằng phân tích toàn chương trình với giá là link lâu hơn[^cargo-lto]; `lto = false` vẫn là LTO cục bộ qua các CGU của crate[^cargo-lto-local]. Phân biệt `true`/`"fat"`, `"thin"`, `false` và `"off"` cùng điều kiện không có LTO: [P11·03 mục 3](../11-do-dac-va-go-loi/03-hieu-nang-service-rust.md).

<details><summary>Đi sâu: vị trí hợp lệ của <code>#[inline]</code></summary>

- Thuộc tính chỉ áp dụng cho hàm có thân: closure, async block, hàm tự do, hàm trong `impl` (inherent hoặc trait impl), và hàm trong định nghĩa trait khi có thân mặc định[^ref-inline-positions].
- `#[inline]` cũng có thể gắn lên closure, nhưng tính hữu dụng hạn chế vì chưa hỗ trợ attribute trên expression[^ref-inline-closure].
- Nhận xét của người soạn: không nên rắc `#[inline(always)]` bừa; Reference đã cảnh báo quyết định inline tồi làm chậm[^ref-inline-auto]. Hãy đo.

</details>

**Quay lại Q7:** `#[inline]` là gợi ý; static dispatch cho compiler cơ hội inline mà `dyn` thường không có; CGU và LTO quyết định phạm vi mà tối ưu nhìn thấy nhau.

## Liên hệ với Rust

Đây là phần "lái xe" của chương: các quyết định thiết kế hằng ngày khi viết Rust (nhận xét của người soạn, dựa trên các mục trên).

1. **`dyn` vs generic.** Mặc định dùng generic/`impl Trait` cho API nhận callback hay kiểu nội bộ (nhanh, inline được, mục 2–4). Dùng `dyn` khi cần tập kiểu mở lúc chạy (plugin, handler, collection hỗn hợp, mục 3). Khi nghi ngờ, viết bản generic trước rồi đổi sang `dyn` ở chỗ binary phình hay compile chậm — và đo.
2. **Code bloat.** Instance nhân theo số kiểu; tách phần thân không phụ thuộc kiểu ra hàm thường (mẫu `inner` của std, mục 6) hoặc nhận `&dyn`/`&[u8]`/`&Path` ở biên thay vì generic toàn bộ.
3. **Inline.** `#[inline]` là gợi ý; để compiler tự quyết trừ khi đã đo (mục 7). Trả `impl Iterator`/`impl Fn` thay vì `Box<dyn …>` để giữ khả năng inline và tránh heap (mục 4–5), trừ khi các nhánh trả kiểu khác nhau.
4. **API công khai.** Đổi `impl Trait` thành generic (hay ngược lại) trong tham số có thể là breaking change vì thay đổi số generic argument[^ref-impl-arg-break]; đổi giữa generic và `dyn` thì còn rộng hơn (suy luận của người soạn).
5. **Build profile.** `codegen-units` và `lto` là cần gạt cân bằng compile time và chất lượng mã[^cargo-cgu][^cargo-lto]; đừng bật bừa trên CI nếu chưa đo.

## Tóm tắt

- Monomorphization: compiler dập một bản mã riêng cho mỗi kiểu cụ thể của hàm generic[^dev-mono]; là bước đầu tiên của backend[^dev-mono-first].
- Collector tạo danh sách hàm cần sinh mã (ví dụ `main`, `banana`, `peach::<u64>`)[^dev-mono-collect], sau đó chia vào CGU[^dev-mono-query].
- Mã monomorphize là static dispatch[^book-dispatch]; trait object luôn dynamic dispatch[^book-dyn-must] qua vtable[^ref-to-virtual].
- Generic: một kiểu mỗi lần; `dyn`: nhiều kiểu lúc chạy[^book-generic-vs-dyn]. Dynamic dispatch ngăn inline và một số tối ưu[^book-inline].
- Trait phải dyn compatible mới làm trait object[^ref-dyncompat]; method generic không dispatch được[^ref-dyn-why-generic].
- Mỗi closure một kiểu[^ref-closure-struct], nên `impl Fn`/`F: Fn` là static, `Box<dyn Fn>` là dynamic.
- Iterator "zero-cost": biên dịch xuống mã gần như viết tay[^book-iter-perf], nhưng chỉ nói về runtime[^book-zero-cost].
- Cái giá của monomorphization: compile time và binary size[^dev-mono-cost]; giảm bằng hàm `inner` không generic (ví dụ `fs::read`)[^src-fs-read-inner] hoặc `dyn` ở chỗ không nóng.
- `#[inline]` là gợi ý[^ref-inline]; `codegen-units` đánh đổi tốc độ biên dịch và chất lượng mã[^cargo-cgu]; LTO tối ưu toàn chương trình[^cargo-lto].

## Câu hỏi tự kiểm tra

1. Monomorphization xảy ra ở đâu trong pipeline rustc, và collector trả về gì cho ví dụ `main → banana → peach::<u64>`?
   <details><summary>Đáp án</summary>

   Là bước đầu của backend[^dev-mono-first]; collector trả `[main, banana, peach::<u64>]`, những hàm sẽ được sinh mã máy[^dev-mono-collect]. Query `collect_and_partition_mono_items` làm thu thập rồi chia CGU[^dev-mono-query].

   </details>

2. Vì sao `Vec<u64>` và `Vec<String>` có hai bản mã khác nhau trong binary?
   <details><summary>Đáp án</summary>

   Compiler đóng dấu một bản riêng cho mỗi kiểu cụ thể cần dùng[^dev-mono]; rustc-dev-guide nêu đúng ví dụ này[^dev-mono-vec].

   </details>

3. Phân biệt static dispatch và dynamic dispatch; trait object thuộc loại nào?
   <details><summary>Đáp án</summary>

   Static: compiler biết lúc biên dịch method nào được gọi[^book-dispatch]; dynamic: không biết nên sinh mã tự tìm lúc chạy[^book-dispatch-dyn]. Trait object luôn dynamic[^book-dyn-must].

   </details>

4. Gọi method qua `&dyn Trait` làm gì lúc chạy?
   <details><summary>Đáp án</summary>

   Tải function pointer từ vtable của trait object rồi gọi gián tiếp[^ref-to-virtual].

   </details>

5. Nêu các điểm đối lập chính giữa generic và `dyn`.
   <details><summary>Đáp án</summary>

   Số bản mã (một mỗi kiểu[^dev-mono] vs một chung); tập kiểu (một kiểu mỗi lần vs nhiều kiểu lúc chạy[^book-generic-vs-dyn]); inline (được vs bị ngăn trong nhiều trường hợp[^book-inline]); chi phí compile/binary[^dev-mono-cost] vs tra bảng[^book-dyn-cost].

   </details>

6. Vì sao trait có method generic không dùng làm trait object?
   <details><summary>Đáp án</summary>

   Reference: hàm dispatchable không được có type parameter[^ref-dyn-generic], ví dụ trong Reference chú thích rằng generic không tương thích với vtable[^ref-dyn-why-generic]. Giải thích vì sao (suy luận): vtable là bảng cố định nên không có ô cho mọi `T` có thể truyền.

   </details>

7. Hai closure có thân giống hệt có cùng kiểu không, và hàm `run<F: Fn()>` được monomorphize mấy bản nếu gọi với cả hai?
   <details><summary>Đáp án</summary>

   Mỗi closure expression sinh kiểu duy nhất[^ref-closure-struct], nên khác kiểu; do đó (suy luận) hai instance của `run` ở mức monomorphization — nguồn không nói gì về số hàm còn lại trong binary cuối.

   </details>

8. Vì sao trả `impl Fn(i32) -> i32` thường tốt hơn `Box<dyn Fn(i32) -> i32>`, và khi nào buộc phải dùng `Box<dyn …>`?
   <details><summary>Đáp án</summary>

   `impl Trait` trả abstract type không boxed[^ref-impl-ret-unboxed], tránh heap và dynamic dispatch[^ref-impl-closure-cost]. Nhưng mọi giá trị trả về phải cùng một kiểu cụ thể[^ref-impl-ret-same]; nhiều kiểu khác nhau thì phải dùng trait object.

   </details>

9. "Zero-cost abstraction" cam kết gì và không cam kết gì?
   <details><summary>Đáp án</summary>

   Dùng abstraction không thêm overhead lúc chạy[^book-zero-cost]; không cam kết compile time hay binary size, vốn là cái giá của monomorphization[^dev-mono-cost].

   </details>

10. Mẫu `fn inner` trong `fs::read` có tác dụng gì (nêu rõ phần nào là nguồn, phần nào là suy luận)?
    <details><summary>Đáp án</summary>

    Nguồn cho thấy hàm ngoài generic theo `P: AsRef<Path>` chỉ gọi `inner(path.as_ref())`[^src-fs-read-call] với `inner` nhận `&Path`[^src-fs-read-inner]. Việc nó giảm số bản mã được nhân bản là suy luận của người soạn: source không ghi lý do.

    </details>

11. `#[inline]` bảo đảm hàm được inline không? Nên dùng thế nào?
   <details><summary>Đáp án</summary>

   Không; nó chỉ gợi ý[^ref-inline] — kể cả `inline(always)`, compiler vẫn có thể bỏ qua[^ref-inline-hint]; rustc tự inline khi thấy đáng và inline tồi có thể làm chậm[^ref-inline-auto]. Chỉ dùng sau khi đo (nhận xét của người soạn).

   </details>

12. `codegen-units` lớn làm gì cho compile time và chất lượng mã?
    <details><summary>Đáp án</summary>

    Nhiều CGU cho phép xử lý song song nhiều hơn, có thể giảm compile time nhưng có thể sinh mã chậm hơn[^cargo-cgu].

    </details>

## Đọc thêm

- [rustc-dev-guide — Monomorphization](https://rustc-dev-guide.rust-lang.org/backend/monomorph.html): collector, CGU partitioning và bảng hành vi qua ranh giới crate.
- [Book 1.95.0 — Generic Data Types (Performance of Code Using Generics)](https://doc.rust-lang.org/1.95.0/book/ch10-01-syntax.html): ví dụ `Option<T>` → `Option_i32`/`Option_f64`.
- [Book 1.95.0 — Trait Objects (Performing Dynamic Dispatch)](https://doc.rust-lang.org/1.95.0/book/ch18-02-trait-objects.html): static vs dynamic dispatch, trade-off, inline.
- [Reference 1.95.0 — Impl trait](https://doc.rust-lang.org/1.95.0/reference/types/impl-trait.html): `impl Trait` ở đối số và trả về; closure/iterator.
- [Reference 1.95.0 — Codegen attributes (inline)](https://doc.rust-lang.org/1.95.0/reference/attributes/codegen.html): `#[inline]`, `always`, `never`.
- [Cargo book 1.95.0 — Profiles](https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html): `codegen-units`, `lto`.
- [P10·01 — Pipeline của rustc](./01-pipeline-rustc.md): mục 5 đặt monomorphization và codegen unit trong pipeline từ source đến binary.
- [P10·03 — DST, fat pointer và vtable](./03-dst-fat-pointer-vtable.md): vtable, asm `dyn` vs generic, dyn compatibility đầy đủ.
- [P9·06 — Iterator và closure](../09-rust-language/06-iterator-va-closure.md): closure struct, `Fn` traits, asm zero-cost.
- [P10·09 — Bên trong collection của std](./09-collections-internals.md): mục 1 (Đi sâu) có `RawVecInner`, một ví dụ trong std của việc tách phần không generic để giảm code phải monomorphize.

[^book-mono]: The Rust Programming Language, Generic Data Types — <https://doc.rust-lang.org/1.95.0/book/ch10-01-syntax.html> — "Monomorphization is the process of turning generic code into specific code by filling in the concrete types that are used when compiled."
[^book-mono-expand]: The Rust Programming Language, Generic Data Types — <https://doc.rust-lang.org/1.95.0/book/ch10-01-syntax.html> — "it expands the generic definition of Option\<T> into two definitions specialized to i32 and f64"
[^book-mono-hand]: The Rust Programming Language, Generic Data Types — <https://doc.rust-lang.org/1.95.0/book/ch10-01-syntax.html> — "When the code runs, it performs just as it would if we had duplicated each definition by hand."
[^book-mono-callsites]: The Rust Programming Language, Generic Data Types — <https://doc.rust-lang.org/1.95.0/book/ch10-01-syntax.html> — "The compiler looks at all the places where generic code is called and generates code for the concrete types the generic code is called with."
[^book-generics-nocost]: The Rust Programming Language, Generic Data Types — <https://doc.rust-lang.org/1.95.0/book/ch10-01-syntax.html> — "using generic types won’t make your program run any slower than it would with concrete types."
[^dev-mono]: rustc-dev-guide, Monomorphization — <https://rustc-dev-guide.rust-lang.org/backend/monomorph.html> — "This means that compiler stamps out a different copy of the code of a generic function for each concrete type needed."
[^dev-mono-first]: rustc-dev-guide, Monomorphization — <https://rustc-dev-guide.rust-lang.org/backend/monomorph.html> — "Monomorphization is the first step in the backend of the Rust compiler."
[^dev-mono-asm]: rustc-dev-guide, Monomorphization — <https://rustc-dev-guide.rust-lang.org/backend/monomorph.html> — "But of course, assembly is not generic, so we need to figure out the concrete types of all the generics before the code can execute."
[^dev-mono-all]: rustc-dev-guide, Monomorphization — <https://rustc-dev-guide.rust-lang.org/backend/monomorph.html> — "Rust takes a different approach: it monomorphizes all generic types."
[^dev-mono-vec]: rustc-dev-guide, Monomorphization — <https://rustc-dev-guide.rust-lang.org/backend/monomorph.html> — "then the generated binary will have two copies of the generated code for Vec: one for Vec\<u64> and another for Vec\<String>."
[^dev-mono-cost]: rustc-dev-guide, Monomorphization — <https://rustc-dev-guide.rust-lang.org/backend/monomorph.html> — "The result is fast programs, but it comes at the cost of compile time (creating all those copies can take a while) and binary size (all those copies might take a lot of space)."
[^dev-mono-collect]: rustc-dev-guide, Monomorphization — <https://rustc-dev-guide.rust-lang.org/backend/monomorph.html> — "The monomorphization collector will give you a list of [main, banana, peach::\<u64>]. These are the functions that will have machine code generated for them."
[^dev-mono-query]: rustc-dev-guide, Monomorphization — <https://rustc-dev-guide.rust-lang.org/backend/monomorph.html> — "query, which does monomorphization collection and then partitions them into codegen units."
[^dev-cgu-two]: rustc-dev-guide, Monomorphization — <https://rustc-dev-guide.rust-lang.org/backend/monomorph.html> — "For better incremental build times, the CGU partitioner creates two CGU for each source level modules. One is for “stable” i.e. non-generic code and the other is more volatile code i.e. monomorphized/specialized instances."
[^dev-cgu-generic]: rustc-dev-guide, Monomorphization — <https://rustc-dev-guide.rust-lang.org/backend/monomorph.html> — "Regardless of inlining, all monomorphized (specialized) functions from Crate A appear within a single codegen unit for Crate B."
[^dev-cgu-nongeneric]: rustc-dev-guide, Monomorphization — <https://rustc-dev-guide.rust-lang.org/backend/monomorph.html> — "Non-generic function Crate A function doesn’t appear in any codegen units of Crate B."
[^dev-cgu-inline]: rustc-dev-guide, Monomorphization — <https://rustc-dev-guide.rust-lang.org/backend/monomorph.html> — "Non-generic #[inline] function Crate A function appears within a single CGU of Crate B."
[^book-dispatch]: The Rust Programming Language, Using Trait Objects That Allow for Values of Different Types — <https://doc.rust-lang.org/1.95.0/book/ch18-02-trait-objects.html> — "The code that results from monomorphization is doing static dispatch, which is when the compiler knows what method you’re calling at compile time."
[^book-dispatch-dyn]: The Rust Programming Language, Using Trait Objects That Allow for Values of Different Types — <https://doc.rust-lang.org/1.95.0/book/ch18-02-trait-objects.html> — "In dynamic dispatch cases, the compiler emits code that at runtime will know which method to call."
[^book-dyn-must]: The Rust Programming Language, Using Trait Objects That Allow for Values of Different Types — <https://doc.rust-lang.org/1.95.0/book/ch18-02-trait-objects.html> — "When we use trait objects, Rust must use dynamic dispatch."
[^book-dyn-cost]: The Rust Programming Language, Using Trait Objects That Allow for Values of Different Types — <https://doc.rust-lang.org/1.95.0/book/ch18-02-trait-objects.html> — "This lookup incurs a runtime cost that doesn’t occur with static dispatch."
[^book-inline]: The Rust Programming Language, Using Trait Objects That Allow for Values of Different Types — <https://doc.rust-lang.org/1.95.0/book/ch18-02-trait-objects.html> — "Dynamic dispatch also prevents the compiler from choosing to inline a method’s code, which in turn prevents some optimizations"
[^book-generic-vs-dyn]: The Rust Programming Language, Using Trait Objects That Allow for Values of Different Types — <https://doc.rust-lang.org/1.95.0/book/ch18-02-trait-objects.html> — "A generic type parameter can be substituted with only one concrete type at a time, whereas trait objects allow for multiple concrete types to fill in for the trait object at runtime."
[^ref-to-intro]: The Rust Reference, Trait object types — <https://doc.rust-lang.org/1.95.0/reference/types/trait-object.html> — "A trait object is an opaque value of another type that implements a set of traits."
[^ref-to-vtable]: The Rust Reference, Trait object types — <https://doc.rust-lang.org/1.95.0/reference/types/trait-object.html> — "a virtual method table, often just called a vtable, which contains, for each method of SomeTrait and its supertraits that T implements, a pointer to T’s implementation (i.e. a function pointer)."
[^ref-to-virtual]: The Rust Reference, Trait object types — <https://doc.rust-lang.org/1.95.0/reference/types/trait-object.html> — "Calling a method on a trait object results in virtual dispatch at runtime: that is, a function pointer is loaded from the trait object vtable and invoked indirectly."
[^ref-to-implements]: The Rust Reference, Trait object types — <https://doc.rust-lang.org/1.95.0/reference/types/trait-object.html> — "Trait objects implement the base trait, its auto traits, and any supertraits of the base trait."
[^ref-to-unsized]: The Rust Reference, Trait object types — <https://doc.rust-lang.org/1.95.0/reference/types/trait-object.html> — "Due to the opaqueness of which concrete type the value is of, trait objects are dynamically sized types."
[^ref-sized-implicit]: The Rust Reference, Special types and traits — <https://doc.rust-lang.org/1.95.0/reference/special-types-and-traits.html> — "Type parameters (except Self in traits) are Sized by default, as are associated types."
[^ref-sized-relax]: The Rust Reference, Special types and traits — <https://doc.rust-lang.org/1.95.0/reference/special-types-and-traits.html> — "These implicit Sized bounds may be relaxed by using the special ?Sized bound."
[^ref-dyncompat]: The Rust Reference, Traits (dyn compatibility) — <https://doc.rust-lang.org/1.95.0/reference/items/traits.html> — "A dyn-compatible trait can be the base trait of a trait object."
[^ref-dyn-generic]: The Rust Reference, Traits (dyn compatibility) — <https://doc.rust-lang.org/1.95.0/reference/items/traits.html> — "Not have any type parameters (although lifetime parameters are allowed)."
[^ref-dyn-why-generic]: The Rust Reference, Traits (dyn compatibility) — <https://doc.rust-lang.org/1.95.0/reference/items/traits.html> — "Generics are not compatible with vtables."
[^ref-closure-struct]: The Rust Reference, Closure types — <https://doc.rust-lang.org/1.95.0/reference/types/closure.html> — "A closure expression produces a closure value with a unique, anonymous type that cannot be written out. A closure type is approximately equivalent to a struct which contains the captured values."
[^ref-impl-arg]: The Rust Reference, Impl trait — <https://doc.rust-lang.org/1.95.0/reference/types/impl-trait.html> — "impl Trait in argument position is syntactic sugar for a generic type parameter like \<T: Trait>, except that the type is anonymous and doesn’t appear in the GenericParams list."
[^ref-impl-arg-break]: The Rust Reference, Impl trait — <https://doc.rust-lang.org/1.95.0/reference/types/impl-trait.html> — "Changing a parameter from either one to the other can constitute a breaking change for the callers of a function, since this changes the number of generic arguments."
[^ref-impl-ret-unboxed]: The Rust Reference, Impl trait — <https://doc.rust-lang.org/1.95.0/reference/types/impl-trait.html> — "impl Trait in return position allows a function to return an unboxed abstract type."
[^ref-impl-closure-cost]: The Rust Reference, Impl trait — <https://doc.rust-lang.org/1.95.0/reference/types/impl-trait.html> — "This could incur performance penalties from heap allocation and dynamic dispatch."
[^ref-impl-ret-same]: The Rust Reference, Impl trait — <https://doc.rust-lang.org/1.95.0/reference/types/impl-trait.html> — "Each possible return value from the function must resolve to the same concrete type."
[^std-iter-lazy]: Rust std, iter, Laziness — <https://doc.rust-lang.org/1.95.0/std/iter/index.html> — "Iterators (and iterator adapters) are lazy. This means that just creating an iterator doesn’t do a whole lot. Nothing really happens until you call next."
[^book-iter-perf]: The Rust Programming Language, Performance in Loops vs. Iterators — <https://doc.rust-lang.org/1.95.0/book/ch13-04-performance.html> — "Iterators, although a high-level abstraction, get compiled down to roughly the same code as if you’d written the lower-level code yourself."
[^book-zero-cost]: The Rust Programming Language, Performance in Loops vs. Iterators — <https://doc.rust-lang.org/1.95.0/book/ch13-04-performance.html> — "Iterators are one of Rust’s zero-cost abstractions, by which we mean that using the abstraction imposes no additional runtime overhead."
[^book-stroustrup]: The Rust Programming Language, Performance in Loops vs. Iterators — <https://doc.rust-lang.org/1.95.0/book/ch13-04-performance.html> — "What you don’t use, you don’t pay for. And further: What you do use, you couldn’t hand code any better."
[^book-iter-opts]: The Rust Programming Language, Performance in Loops vs. Iterators — <https://doc.rust-lang.org/1.95.0/book/ch13-04-performance.html> — "Optimizations such as loop unrolling and eliminating bounds checking on array access apply and make the resultant code extremely efficient."
[^src-iter-map]: rust-lang/rust tag 1.95.0, library/core/src/iter/traits/iterator.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/core/src/iter/traits/iterator.rs> — "fn map\<B, F>(self, f: F) -> Map\<Self, F>"
[^src-map-struct]: rust-lang/rust tag 1.95.0, library/core/src/iter/adapters/map.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/core/src/iter/adapters/map.rs> — "pub struct Map\<I, F> {"
[^src-map-impl]: rust-lang/rust tag 1.95.0, library/core/src/iter/adapters/map.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/core/src/iter/adapters/map.rs> — "impl\<B, I: Iterator, F> Iterator for Map\<I, F> where F: FnMut(I::Item) -> B,"
[^src-map-next]: rust-lang/rust tag 1.95.0, library/core/src/iter/adapters/map.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/core/src/iter/adapters/map.rs> — "self.iter.next().map(&mut self.f)"
[^src-map-next-sig]: rust-lang/rust tag 1.95.0, library/core/src/iter/adapters/map.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/core/src/iter/adapters/map.rs> — "#[inline] fn next(&mut self) -> Option\<B> {"
[^src-map-fold]: rust-lang/rust tag 1.95.0, library/core/src/iter/adapters/map.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/core/src/iter/adapters/map.rs> — "self.iter.fold(init, map_fold(self.f, g))"
[^src-box-iter-next]: rust-lang/rust tag 1.95.0, library/alloc/src/boxed/iter.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/alloc/src/boxed/iter.rs> — "impl\<I: Iterator + ?Sized, A: Allocator> Iterator for Box\<I, A> { type Item = I::Item; fn next(&mut self) -> Option\<I::Item> { (**self).next() }"
[^src-fs-read-sig]: rust-lang/rust tag 1.95.0, library/std/src/fs.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/fs.rs> — "pub fn read\<P: AsRef\<Path>>(path: P) -> io::Result\<Vec\<u8>> {"
[^src-fs-read-inner]: rust-lang/rust tag 1.95.0, library/std/src/fs.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/fs.rs> — "fn inner(path: &Path) -> io::Result\<Vec\<u8>> {"
[^src-fs-write-inner]: rust-lang/rust tag 1.95.0, library/std/src/fs.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/fs.rs> — "fn inner(path: &Path, contents: &[u8]) -> io::Result<()> {"
[^ref-inline]: The Rust Reference, Code generation attributes (inline) — <https://doc.rust-lang.org/1.95.0/reference/attributes/codegen.html> — "The inline attribute suggests whether a copy of the attributed function’s code should be placed in the caller rather than generating a call to the function."
[^ref-inline-auto]: The Rust Reference, Code generation attributes (inline) — <https://doc.rust-lang.org/1.95.0/reference/attributes/codegen.html> — "rustc automatically inlines functions when doing so seems worthwhile. Use this attribute carefully as poor decisions about what to inline can slow down programs."
[^ref-inline-positions]: The Rust Reference, Code generation attributes (inline) — <https://doc.rust-lang.org/1.95.0/reference/attributes/codegen.html> — "The inline attribute may only be applied to functions with bodies — closures, async blocks, free functions, associated functions in an inherent impl or trait impl, and associated functions in a trait definition when those functions have a default definition"
[^ref-inline-closure]: The Rust Reference, Code generation attributes (inline) — <https://doc.rust-lang.org/1.95.0/reference/attributes/codegen.html> — "Though the attribute can be applied to closures and async blocks, the usefulness of this is limited as we do not yet support attributes on expressions."
[^cargo-cgu]: The Cargo Book, Profiles (codegen-units) — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "More code generation units allows more of a crate to be processed in parallel possibly reducing compile time, but may produce slower code."
[^cargo-cgu-default]: The Cargo Book, Profiles (codegen-units) — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "The default is 256 for incremental builds, and 16 for non-incremental builds."
[^cargo-lto]: The Cargo Book, Profiles (lto) — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "LTO can produce better optimized code, using whole-program analysis, at the cost of longer linking time."
[^dev-mono-java]: rustc-dev-guide, Monomorphization — <https://rustc-dev-guide.rust-lang.org/backend/monomorph.html> — "in some languages, such as Java, we may not know the most precise type of value until runtime."
[^dev-mono-collect-what]: rustc-dev-guide, Monomorphization — <https://rustc-dev-guide.rust-lang.org/backend/monomorph.html> — "First, we need to figure out what concrete types we need for all the generic things in our program. This is called collection, and the code that does this is called the monomorphization collector."
[^ref-to-base]: The Rust Reference, Trait object types — <https://doc.rust-lang.org/1.95.0/reference/types/trait-object.html> — "The set of traits is made up of a dyn compatible base trait plus any number of auto traits."
[^ref-dyn-formerly]: The Rust Reference, Traits (dyn compatibility) — <https://doc.rust-lang.org/1.95.0/reference/items/traits.html> — "This concept was formerly known as object safety."
[^src-fs-read-call]: rust-lang/rust tag 1.95.0, library/std/src/fs.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/fs.rs> — "Ok(bytes) } inner(path.as_ref())"
[^src-fs-rts-inner]: rust-lang/rust tag 1.95.0, library/std/src/fs.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/fs.rs> — "fn inner(path: &Path) -> io::Result\<String> {"
[^ref-inline-hint]: The Rust Reference, Code generation attributes (inline) — <https://doc.rust-lang.org/1.95.0/reference/attributes/codegen.html> — "In every form the attribute is a hint. The compiler may ignore it."
[^cargo-lto-local]: The Cargo Book, Profiles (lto) — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "Performs “thin local LTO” which performs “thin” LTO on the local crate only across its codegen units. No LTO is performed if codegen units is 1 or opt-level is 0."
[^std-inline-cgu]: Standard library developers Guide, When to #[inline] (trích một thread) — <https://std-dev-guide.rust-lang.org/policy/inline.html> — "In debug mode rustc basically ignores #[inline], pretending you didn't even write it. In release mode the compiler will, by default, codegen an #[inline] function into every single referencing codegen unit, and then it will also add inlinehint."
[^std-inline-generic]: Standard library developers Guide, When to #[inline] — <https://std-dev-guide.rust-lang.org/policy/inline.html> — "You shouldn't need #[inline]: On methods that have any generics in scope."
[^dev-mono-before-codegen]: rustc-dev-guide, Monomorphization — <https://rustc-dev-guide.rust-lang.org/backend/monomorph.html> — "The monomorphization collector is run just before MIR lowering and codegen."
[^dev-mono-statics]: rustc-dev-guide, Monomorphization — <https://rustc-dev-guide.rust-lang.org/backend/monomorph.html> — "Collector will also add things like statics to that list."
[^ref-closure-nolayout]: The Rust Reference, Type layout (closure layout) — <https://doc.rust-lang.org/1.95.0/reference/type-layout.html> — "Closures have no layout guarantees."
