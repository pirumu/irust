# P10 · Chương 08 — Panic, unwinding, ABI và FFI

> Mọi khẳng định về Rust theo tài liệu bản **1.95.0** (Reference, Nomicon, rustc book, std, Cargo book; một chú thích từ rustc-dev-guide). Chương này **không có thí nghiệm chạy thật**: các đoạn code chỉ để minh họa, không kèm output; chỗ nào là suy luận của người soạn đều ghi rõ.

## Bạn sẽ hiểu được gì

1. **Q1.** `panic!` thực sự làm gì với thread và stack: vì sao có "unwind" và destructor chạy ra sao? → mục 1
2. **Q2.** `panic = "unwind"` khác `panic = "abort"` thế nào, chọn ở đâu, và đổi lại được gì/mất gì? → mục 2
3. **Q3.** `catch_unwind`, ranh giới thread, `UnwindSafe` và mutex poisoning: panic được "chặn" ở đâu và dùng đúng chỗ nào? → mục 3
4. **Q4.** ABI là gì; `"Rust"`, `"C"`, `"system"` khác nhau ra sao và khối `extern`/`extern fn` làm gì? → mục 4
5. **Q5.** Kiểu dữ liệu nào đi qua ranh giới FFI an toàn: `repr(C)`, con trỏ null, opaque struct (và hàm variadic, nói gọn ở phần Đi sâu của mục 4)? → mục 5
6. **Q6.** Điều gì xảy ra khi panic (hoặc exception C++) băng qua ranh giới FFI; `"C-unwind"` giải quyết gì? → mục 6
7. **Q7.** Mở API Rust cho C (hoặc ngôn ngữ khác) thì đặt "tường chắn panic" và quy tắc build ở đâu? → mục 7

## Kiến thức cần trước

- [P2 · Chương 04 — Calling convention và stack frame](../02-cpu/04-calling-convention-va-stack-frame.md): ABI là hợp đồng về thanh ghi và stack (mục 1), frame và bảng unwind (mục 6), `extern "Rust"` so với `extern "C"` ở mức asm (mục 8). Chương này dùng lại các ý đó.
- [P9 · Chương 05 — Error handling](../09-rust-language/05-error-handling.md): khi nào dùng `Result`, khi nào `panic!` (mục 1 và 7). Chương này đào vào *cơ chế bên dưới* panic chứ không nhắc lại tiêu chí chọn.
- [Chương 07 — unsafe, undefined behavior và Miri](07-unsafe-ub-miri.md): UB là gì và vì sao UB ở một phía FFI lan sang phía kia (mục 2). Khi chương này nói panic băng qua ranh giới FFI thì abort hay UB (mục 6–7), nó dùng lại khái niệm đó.

Bạn đã viết service Rust bằng `std` và `Cargo`, dùng `unwrap`, `Result`, `Mutex`; chưa cần từng viết FFI.

---

## 1. Panic: dừng đường chạy hiện tại mà vẫn trả tài nguyên (Q1)

> **Trả lời ngắn.** Panic là cách Rust nói "chỗ này gặp lỗi mà không thể xử lý hợp lý": hàm thôi không return bình thường nữa[^ref-panic-intro]. Mặc định ở đa số target là chiến lược unwind[^ref-panic-default]. Với chiến lược này, thread dừng chạy bình thường rồi tháo stack từng frame và gọi destructor như thể mọi hàm đều return ngay lập tức[^nomicon-unwind]. Nhờ vậy khi chương trình chạy tiếp, những object đang giữ tài nguyên đã được dọn[^ref-panic-cleanup].

**Giải thích.**

*Với service `billing`.* Hãy hình dung `billing`, một service Rust nhận request tính hóa đơn (ví dụ giả định của người soạn, không phải code trong repo). Mỗi request chạy trên một worker thread của thread pool và đi qua vài lớp hàm: parse JSON, tra giá trong một `Mutex<PriceCache>` dùng chung, mở transaction DB, ghi file tạm. Giả sử ở lớp thứ tư có một `unwrap()` trên `None`. Lớp đó không thể tự chạy ngược lên để rollback transaction, xóa file tạm và nhả lock. Mục này cho thấy Rust làm hộ việc đó bằng cách nào và tính phí ra sao. Ba mục tiếp theo hỏi tiếp về `billing`: nên để cả service chết hay chỉ request đó (mục 2), request sau có dùng nhầm dữ liệu dở dang không (mục 3). Từ mục 4, `billing` gọi sang thư viện C và để C gọi ngược lại.

### 1.1 Panic xuất phát từ đâu

Bạn có thể gọi `panic!` tường minh[^ref-panic-macro]. Cũng có những cấu trúc ngôn ngữ tự panic, ví dụ truy cập mảng ngoài biên[^ref-panic-lang]. Nomicon kể thêm `unwrap` trên `None`, chia cho 0, và ở debug build là mọi phép số học có thể tràn. Từ đó nó kết luận: trừ khi bạn rất cẩn thận kiểm soát code nào chạy, gần như dòng nào cũng có thể unwind, và bạn phải sẵn sàng cho điều đó[^nomicon-excsafe-everything]. Với `billing`, `prices[idx]` hay `total / quantity` đều là chỗ có thể panic mà không cần dòng `panic!` nào (nhận xét của người soạn).

Trong bốn cách Rust xử lý một tình huống bất thường, panic đứng ở bậc thứ ba. Nomicon xếp như sau: `Option` cho cái có thể vắng; `Result` cho lỗi xử lý được; panic cho lỗi không thể xử lý hợp lý; abort cho thảm họa[^nomicon-tiers].

### 1.2 Trình tự một lần panic

Dưới đây là những gì xảy ra trên worker thread của `billing` khi chiến lược là unwind:

1. **Panic hook chạy trước.** Hook được gọi khi thread panic, nhưng trước khi panic runtime được gọi, nên nó chạy với cả runtime unwind lẫn runtime abort[^hook-before]. Ngoại lệ là `-C panic=immediate-abort` của rustc: chế độ này không gọi hook[^rustc-panic-flag] (xem mục 2). Hook mặc định in thông điệp ra stderr, và dựng backtrace nếu bạn yêu cầu[^hook-default].
2. **Unwind.** Runtime đi qua các frame Rust giống như `throw` của C++ đi qua frame C++, cho tới điểm phục hồi, ví dụ ranh giới thread. Trên đường đi, object nào còn sống trong frame và có `Drop` thì bị gọi `drop`[^ref-panic-unwind]. Đây là lúc transaction được rollback và file tạm bị xóa, nếu chúng được bọc trong struct có `Drop` (nhận xét của người soạn).
3. **Dừng ở điểm phục hồi.** Thư viện chuẩn có hai cơ chế phục hồi: `catch_unwind` phục hồi ngay trong thread đang panic, còn `thread::spawn` tự dựng điểm phục hồi cho thread con để các thread khác chạy tiếp[^ref-panic-recover]. Nếu không ai bắt, thread thoát và thread khác có thể thấy panic qua `join`; riêng main thread panic mà không bắt thì cả ứng dụng thoát với exit code khác 0[^thread-panic].

```mermaid
flowchart TD
  P["panic! / unwrap None / index ngoài biên"] --> H["panic hook (mặc định: in thông điệp ra stderr)"]
  H --> R{"panic runtime"}
  R -- "unwind" --> U["tháo từng frame, gọi Drop của object còn sống"]
  U --> C{"có điểm phục hồi?"}
  C -- "catch_unwind / ranh giới thread" --> OK["thread hoặc closure kết thúc bằng Err(payload)"]
  C -- "không" --> T["thread thoát; main thread ⇒ process thoát mã khác 0"]
  R -- "abort" --> A["process bị kết thúc ngay, không chạy destructor"]
```

(Sơ đồ là tóm tắt của người soạn từ các chú thích trong mục này và mục 2.)

### 1.3 Cái giá của unwind

Cài đặt unwinding hiện tại được tối ưu cho trường hợp *không* unwind: chương trình không unwind thì không tốn chi phí runtime nào chỉ vì sẵn sàng unwind. Đổi lại, unwind thật sẽ đắt hơn so với, chẳng hạn, Java. Vì vậy Nomicon dặn đừng xây chương trình dựa vào unwind trong điều kiện bình thường[^nomicon-unwind-cost]. Điều này khớp với thang ở §1.1: lỗi xảy ra thường xuyên, như đơn hàng sai định dạng, nên đi bằng `Result`, còn panic để dành cho bug (nhận xét của người soạn).

<details><summary>Đi sâu: lịch sử, payload, điểm tiếp giáp với MIR, "unwind" không bắt buộc dùng cơ chế của C++</summary>

- **Lịch sử.** Rust thuở đầu giống Erlang: có task nhẹ, và task tự kết liễu bằng panic khi gặp trạng thái không chịu nổi[^nomicon-unwind-erlang]. Về sau task nhẹ nhường chỗ cho thread OS nặng. Theo Nomicon, trên Rust stable tính đến 1.0 chỉ thread cha bắt được panic, tức bắt panic đòi dựng cả một thread OS; `catch_unwind` là API cho phép bắt panic mà không cần dựng thread[^nomicon-unwind-hist]. Đây là mô tả lịch sử trong Nomicon, không phải điều cần nhớ khi viết code.
- **Unwind không nhất thiết là cơ chế của C++.** Reference nhấn vào hành vi *dọn dẹp*: miễn là bảo đảm dọn tài nguyên vẫn đúng (object không còn truy cập được đã được dọn như thể ra khỏi scope bình thường), unwinding có thể được cài đặt mà không dùng đúng cơ chế mà C++ dùng trên target đó[^ref-unwind-impl]. Vậy bảng unwind ở [P2 · Chương 04, mục 6](../02-cpu/04-calling-convention-va-stack-frame.md) chỉ là *một* cách cài đặt, không phải điều Reference bắt buộc (suy luận của người soạn từ câu trích).
- **Payload.** `panic_any` cho phép panic với thông điệp thuộc *bất kỳ* kiểu `Any + Send`, không chỉ chuỗi[^panic-any]. `catch_unwind` trả `Err(cause)`, trong đó `cause` chính là object mà panic được khởi phát với nó[^catch-ret].
- **MIR.** rustc-dev-guide nói lời gọi hàm luôn là *terminator* vì có khả năng unwind[^dev-mir-call]. Suy ra đồ thị luồng điều khiển phải có thêm đường dọn dẹp (suy luận của người soạn từ câu trích; về MIR xem [Chương 01, mục 4](01-pipeline-rustc.md)).
- **`std::thread::panicking()`** cho biết thread hiện tại có đang unwind vì panic không. Docs nêu cách dùng điển hình là *đầu độc* tài nguyên chung khi viết unsafe code, bằng cách kiểm tra nó trong `drop`; code safe thường không cần vì mutex đã tự đầu độc khi thread panic lúc đang giữ lock[^thread-panicking].

</details>

**Quay lại Q1:** panic dừng đường chạy hiện tại; mặc định nó tháo stack và gọi `Drop`, và hook chạy trước cả hai. Với `billing`, request panic được dọn sạch những gì nó đang giữ, nhưng phải trả giá bằng một lần unwind đắt.

---

## 2. `unwind` hay `abort`: chọn chiến lược panic (Q2)

> **Trả lời ngắn.** Chiến lược panic là một lựa chọn lúc build: `unwind` tháo stack và có thể phục hồi, `abort` kết thúc process và không phục hồi[^ref-panic-kinds]. Với `abort` thì không thể `catch_unwind`[^catch-notall]. Process bị `process::abort` kết thúc không chạy destructor nào[^abort-doc]. Dưới `panic=abort`, `panic!` "tương tự" `abort`[^abort-similar]. Nomicon nói thẳng hơn: panic lúc đó abort process ngay lập tức[^nomicon-panic-abort-note]. Ghép các điều trên lại thì panic dưới `abort` cũng không chạy destructor (suy luận của người soạn; có quan sát chạy thật ở [P9 · Chương 05, mục 7](../09-rust-language/05-error-handling.md)). Đổi lại, trình tối ưu được phép giả định unwind không xảy ra, nên code có thể nhỏ hơn và nhanh hơn[^ref-panic-abort-opt].

**Giải thích.**

*Với service `billing`.* Một request của `billing` panic. Service có nên chết theo không? Nếu `billing` chạy trên Kubernetes thì có hai chính sách. Chính sách thứ nhất là để worker tự dọn rồi phục vụ tiếp: đó là `unwind`. Chính sách thứ hai là cho cả container chết ngay để orchestrator dựng lại: đó là `abort`. Cả hai đều hợp lý; điều cần là biết mình đã chọn cái nào, và biết cái giá, ví dụ log đang nằm trong buffer có mất không (nhận xét của người soạn).

### 2.1 Hai panic handler của `std`

`std` cung cấp hai panic handler: `unwind` (tháo stack, có khả năng phục hồi) và `abort` (kết thúc process, không phục hồi)[^ref-panic-kinds]. Đa số target mặc định dùng `unwind`[^ref-panic-default]. Tuy vậy không phải target nào cũng có handler `unwind`[^ref-panic-notall].

### 2.2 Chọn bằng cờ rustc hay bằng Cargo

Cờ `-C panic` của rustc nhận `abort`, `immediate-abort` (kết thúc process và *không* gọi panic hook) hoặc `unwind`. Nếu bạn không chỉ định thì mặc định tùy target[^rustc-panic-flag]. Cargo bọc cờ này bằng key `panic` trong profile, với hai giá trị `"unwind"` và `"abort"`[^cargo-panic]. Profile `dev` mặc định là `'unwind'`[^cargo-defaults]. Profile `release` cũng vậy[^cargo-defaults-release] (bảng mặc định đầy đủ của các profile ở [P9·08 mục 7](../09-rust-language/08-module-crate-cargo.md)). Muốn `billing` abort ở bản release thì ghi:

```toml
[profile.release]
panic = "abort"
```

### 2.3 Ba ràng buộc dễ vấp

- **Đồ thị crate.** Nếu *bất kỳ* crate nào trong đồ thị dùng `abort` thì binary cuối (bin, dylib, cdylib, staticlib) cũng phải dùng `abort`[^rustc-panic-flag]. Chiều ngược lại thì Reference nêu hệ quả: crate build với `unwind` dùng được handler `abort`, còn crate build với `abort` không dùng được handler `unwind`[^ref-panic-implication].
- **Test.** Cargo bỏ qua `panic` cho test, benchmark, build script và proc macro, vì test harness hiện đòi hành vi unwind[^cargo-test-ignore]. Nghĩa là test của `billing` vẫn chạy dưới `unwind` dù `release` đặt `abort` (suy luận của người soạn từ câu trích).
- **Hook vẫn chạy.** Dưới `panic=abort`, `panic!` và `process::abort` "tương tự" nhau, nhưng `panic!` vẫn gọi panic hook còn `abort` thì không[^abort-similar]. Đó là chỗ để `billing` ghi lại lý do chết trước khi process bị kết thúc (nhận xét của người soạn).

### 2.4 Hệ quả cho code

Dưới `abort`, `catch_unwind` không còn gì để bắt: nó chỉ bắt panic dạng unwind, không bắt panic làm process abort[^catch-notall]. Vì vậy cách ly kiểu "một request panic không giết cả server" trong mô hình thread-per-request phụ thuộc vào `unwind` (nhận xét của người soạn). Hành vi của từng runtime async cụ thể, ví dụ Tokio, nằm ngoài các nguồn của chương này.

<details><summary>Đi sâu: <code>process::abort</code>, mất gì khi abort, <code>no_std</code>, test với abort</summary>

- **`process::abort` kết thúc process theo kiểu "bất thường".** Không destructor nào chạy, dù trên stack hiện tại hay stack của thread khác. Buffer IO của Rust như `BufWriter` không được flush, và buffer C stdio thường cũng không[^abort-doc]. Trên Unix, process kết thúc bằng signal `SIGABRT`[^abort-sigabrt]. Với service, hệ quả thực tế là mất log hoặc dữ liệu đang nằm trong buffer (nhận xét của người soạn). Nếu cần thoát sạch thì chỉ gọi `abort` ở điểm không còn destructor nào cần chạy; docs khuyến nghị đúng như vậy[^abort-clean].
- **Lợi ích với trình tối ưu.** Reference nói khi biên dịch với chiến lược `abort`, trình tối ưu có thể giả định unwinding qua frame Rust là không thể, và nhờ đó cải thiện cả cỡ code lẫn tốc độ[^ref-panic-abort-opt]. Tài liệu không đưa con số, và chương này **không đo** nên cũng không nêu số.
- **Unwind handler không bảo đảm mọi panic phục hồi được.** Reference chỉ bảo đảm panic qua `panic!` và cơ chế tương tự của thư viện chuẩn là phục hồi được[^ref-panic-notrecover].
- **`no_std`.** Với binary, dylib, cdylib hay staticlib kiểu `no_std`, bạn phải tự cung cấp panic handler[^ref-panic-nostd]. Cách làm là gắn attribute `panic_handler` lên một hàm[^ref-panic-handler-attr]. Trong đồ thị phụ thuộc chỉ được có đúng một hàm như vậy[^ref-panic-handler-unique].
- **Test với abort.** Khi chạy test với chiến lược abort (cờ `panic-abort-tests`, đang ở dạng unstable), Cargo ép mọi dependency build theo `unwind`[^cargo-abort-test].

</details>

**Quay lại Q2:** `unwind` giữ dọn dẹp và khả năng cô lập panic; `abort` bỏ cả hai để lấy code đơn giản hơn cho trình tối ưu. Chọn bằng `-C panic` hoặc `panic` trong profile; với `billing` đó là quyết định "request panic thì cả container có chết theo không".

---

## 3. `catch_unwind`, ranh giới thread, `UnwindSafe`, poisoning (Q3)

> **Trả lời ngắn.** Panic dừng ở hai loại điểm: `thread::spawn`, cô lập cả thread con, và `catch_unwind`, bắt ngay trong thread[^ref-panic-recover]. `catch_unwind` trả `Ok` nếu closure không panic và `Err(nguyên nhân)` nếu có[^catch-ret]. Docs không khuyên dùng nó làm cơ chế try/catch thông thường[^catch-notgeneral]. Sau khi bắt, dữ liệu có thể đang ở trạng thái dở dang, nên std đặt hai lớp cảnh báo. Một là bound `UnwindSafe`, mà docs gọi là "speed bump"[^unwindsafe-speedbump]. Hai là mutex poisoning, mà docs nói chỉ mang tính khuyến cáo[^mutex-advisory].

**Giải thích.**

*Với service `billing`.* Nhiều worker thread của `billing` cùng dùng `Mutex<PriceCache>`. Giả sử một request panic đúng lúc đang giữ lock để cập nhật cache, và cache mới sửa được nửa chừng. Request tiếp theo lấy lock thì sao: nó có dùng nhầm dữ liệu dở dang không? Mục này trả lời hai việc: panic của một request dừng ở đâu, và std giúp request sau tránh dữ liệu hỏng đến mức nào.

### 3.1 Đặt `catch_unwind` ở đâu

Nên đặt `catch_unwind` ở *biên* của một đơn vị công việc độc lập (một request, một task), giống `try/catch` bao quanh một handler của web framework, chứ đừng rải khắp code để điều khiển luồng (nhận xét của người soạn). Luồng lỗi bình thường vẫn là `Result`.

### 3.2 Nó làm gì, và không làm gì

`catch_unwind` gọi closure. Nếu closure không panic, bạn nhận `Ok(kết quả)`; nếu panic, bạn nhận `Err(cause)`, trong đó `cause` là object mà panic được khởi phát với nó[^catch-ret]. Docs nói rõ không nên dùng nó làm try/catch tổng quát, vì `Result` hợp hơn cho hàm có thể lỗi thường xuyên[^catch-notgeneral]. Nó cũng có thể không bắt được mọi panic, vì panic có thể được cài bằng abort[^catch-notall]. Hook, nếu bạn đã đặt, chạy *trước* khi panic bị bắt, tức trước khi unwind[^catch-hook].

### 3.3 Vì sao có `UnwindSafe`

Vấn đề cần tránh nằm ở hai bước. Khi thread panic, một cấu trúc dữ liệu đang ở trạng thái tạm thời không hợp lệ; sau đó có người quan sát bất biến bị phá ấy[^unwindsafe-two]. Trong `billing`, đó là `PriceCache` mới ghi một nửa bản cập nhật. Điều cần nhớ là nếu không có `unsafe`, panic không thể dẫn tới mất an toàn bộ nhớ[^unwindsafe-nomem]. Thứ có thể hỏng là bất biến *logic*, và nó gây ra lỗi hành vi chứ không phải lỗi bộ nhớ[^unwindsafe-logic].

Để đưa khái niệm này vào hệ kiểu, `catch_unwind` đòi closure phải là `UnwindSafe`[^catch-bound]. Trait này không phải `unsafe trait`, nên không có hợp đồng cứng; docs gọi nó là "speed bump" để nhắc người dùng `catch_unwind` rằng họ có thể thấy bất biến đã hỏng[^unwindsafe-speedbump]. Ví dụ kiểu không unwind safe là `&mut T` và `&RefCell<T>`: trạng thái khả biến chia sẻ ngang qua `catch_unwind` rất dễ bị nhìn thấy khi đã hỏng, vì dữ liệu cứ được truy cập như bình thường[^unwindsafe-mut]. Nếu bạn biết chắc mình đang làm gì và muốn tắt gờ này, hãy bọc closure bằng `AssertUnwindSafe`[^assertunwindsafe].

### 3.4 Mutex poisoning

Mutex của std dùng chiến lược gọi là poisoning (đầu độc): mutex bị "đầu độc" nếu nhận ra thread đang giữ nó đã panic[^mutex-poison-def]. Từ đó `lock` và `try_lock` trả về `Result` để báo mutex có bị đầu độc không, và phần lớn code chỉ `unwrap()` kết quả này. Nhờ vậy panic lan giữa các thread, và không ai thấy một bất biến có thể đã sai[^mutex-poison]. Với `billing`: worker panic khi đang giữ lock của `PriceCache` thì các worker sau gọi `lock().unwrap()` cũng panic, thay vì đọc cache dở dang (nhận xét của người soạn).

Poisoning chỉ là khuyến cáo: `PoisonError::into_inner` vẫn trả về guard như thể lock thành công[^mutex-advisory]. Nomicon giải thích rằng mutex không đầu độc vì an toàn bộ nhớ[^nomicon-poison-mem]. Nó đầu độc để bạn không dùng mù quáng dữ liệu của một mutex đã chứng kiến panic lúc đang khóa[^nomicon-poison-why]. Chi tiết `RwLock`, `Condvar` và kết quả chạy thật nằm ở [P4 · Chương 08, mục 6](../04-concurrency/08-rust-concurrency.md).

### 3.5 Hai tầng exception safety

Nomicon chia exception safety thành hai tầng. Tầng *minimal*: trong unsafe code, phải đủ exception safe để không phá an toàn bộ nhớ. Tầng *maximal*: trong safe code, nên đủ exception safe để chương trình làm đúng[^nomicon-excsafe-levels].

<details><summary>Đi sâu: ví dụ <code>Vec::push_all</code>, <code>resume_unwind</code>, hook toàn cục, bẫy khi drop payload</summary>

- **Ví dụ kinh điển của Nomicon.** `push_all` đặt `len` trước rồi mới `clone` từng phần tử. `clone` là "quả bom panic" bị bỏ sót: nếu nó panic thì `Vec` có `len` lớn hơn số phần tử đã khởi tạo, và việc nhìn hay drop `Vec` sẽ đọc bộ nhớ chưa khởi tạo[^nomicon-pushall]. Bài học (nhận xét của người soạn): code unsafe không được để trạng thái bất hợp lệ tồn tại qua một lời gọi tới code lạ có thể panic.
- **`resume_unwind`** kích hoạt panic mà *không* gọi hook. Docs nói nó dành để dùng cùng `catch_unwind`, ví dụ để mang một panic qua một lớp code C[^resume]. Nếu panic đang được cài bằng abort thì hàm này abort chứ không unwind[^resume-abort].
- **Hook là tài nguyên toàn cục**[^hook-global]. `set_hook` thay hook cũ[^hook-replace]. `take_hook` lấy hook hiện tại và khôi phục hook mặc định[^hook-take]. `set_hook` sẽ panic nếu gọi từ một thread đang panic[^hook-panics].
- **Bẫy khi drop kết quả `catch_unwind`.** Nếu kết quả là `Err`, nó chứa payload, và drop payload có thể lại panic[^catch-drop].

</details>

**Quay lại Q3:** panic dừng ở ranh giới thread hoặc `catch_unwind`; và vì dữ liệu có thể dở dang, std chặn bằng `UnwindSafe` và poisoning, hai gờ giảm tốc chứ không phải bảo đảm tuyệt đối. Với `billing`, request panic không kéo theo request khác trừ khi chúng đụng cùng một mutex bị đầu độc.

---

## 4. ABI là gì; `extern` khai báo và định nghĩa (Q4)

> **Trả lời ngắn.** ABI quy định hàm gọi hàm ra sao ở mức máy: tham số vào thanh ghi hay stack, giá trị trả về đi đường nào, ai dọn stack[^ref-ext-abi-intro]. ABI `"Rust"` là mặc định và không có bảo đảm ổn định[^ref-abi-rust]. ABI `"C"` khớp ABI mặc định của trình biên dịch C chủ đạo trên target[^ref-abi-c]. Vì vậy nó là điểm chung để nói chuyện với code ngoài: Nomicon gọi C là ngôn ngữ chung (lingua franca) của thế giới lập trình[^nomicon-ffi-reprc].

**Giải thích.**

*Với service `billing`.* Từ mục này `billing` ra khỏi thế giới Rust thuần ở hai chiều. Chiều thứ nhất, `billing` gọi `libtax`, một thư viện C tính thuế. Chiều thứ hai, `billing` đóng gói phần tính giá thành `libpricing`, một thư viện chung để gateway C++ của đồng nghiệp gọi vào. Cả hai chuyện đều là một lời gọi hàm, nhưng hai bên phải thống nhất tham số nằm ở thanh ghi nào và ai dọn stack. Nếu hai bên hiểu khác nhau thì chương trình vẫn link được, chỉ là đọc sai giá trị (nhận xét của người soạn). Mục này nói ABI là gì và `extern` khai báo hai chiều ấy ra sao.

### 4.1 Nền: calling convention

[P2 · 04](../02-cpu/04-calling-convention-va-stack-frame.md) (mục 1) đã nói ABI là hợp đồng về thanh ghi và stack. Reference nói lại đúng ý đó cho `extern`: calling convention là giao diện mức thấp của hàm, gồm tham số được đặt ở thanh ghi hay stack, giá trị trả về được truyền thế nào, và ai chịu trách nhiệm dọn stack[^ref-ext-abi-intro].

### 4.2 ABI `"Rust"` không ổn định

Hàm không ghi `extern` dùng ABI này, và Reference nói rõ ABI Rust *không cam kết ổn định*[^ref-abi-rust]. Hệ quả (suy luận của người soạn): đừng bao giờ đặt hàm Rust thường vào thư viện chung rồi cho code ngoài gọi, vì cách gọi có thể đổi giữa các bản compiler. [P2 · Chương 04, mục 8](../02-cpu/04-calling-convention-va-stack-frame.md) so sánh asm của cùng một hàm biên dịch theo hai ABI.

### 4.3 Các ABI chuẩn

`"C"` khớp ABI mặc định của trình biên dịch C chủ đạo trên target[^ref-abi-c]. `"system"` tương đương `"C"` trừ Windows x86_32, nơi nó là `"stdcall"` cho hàm không variadic và `"C"` cho hàm variadic[^ref-ext-system]. Reference khuyên dùng `"system"` khi gọi API Windows không định nghĩa ABI tường minh[^ref-ext-system-win]. Ngoài ra còn các ABI riêng nền tảng như `"win64"`, `"sysv64"`, `"aapcs"`; nhiều cái trong số đó cũng có biến thể `-unwind`[^ref-ext-platform-unwind]. Cái nào là "C" thật trên máy bạn tùy target: `"sysv64"` là System V cho x86_64 và trùng `"C"` trên x86_64 không phải Windows[^ref-ext-sysv64].

### 4.4 Hai hướng khai báo

**`billing` gọi vào `libtax`.** Bạn khai báo hàm ngoài trong khối `unsafe extern "C" { fn f(...); }`. Từ khóa `unsafe` đứng trước `extern` là bắt buộc về mặt ngữ nghĩa[^ref-ext-unsafe-required]. Trước edition 2024, từ khóa này còn tùy chọn[^ref-ext-edition2024]. Mỗi hàm trong khối ngầm là `unsafe`, trừ khi bạn ghi qualifier `safe`[^ref-ext-unsafe]. Khi gọi, compiler tự dịch giữa ABI Rust và ABI ngoài[^ref-ext-foreign-abi]. Lý do cần `unsafe` thì Nomicon nói: thư viện C hay phơi giao diện không thread-safe, và hầu như hàm nào nhận con trỏ cũng không hợp lệ với mọi đầu vào, vì con trỏ có thể dangling[^nomicon-ffi-unsafe] (ý chung đã nêu ở [Chương 07, mục 1](07-unsafe-ub-miri.md)). Compiler không kiểm được khai báo của bạn có khớp với phía C hay không, nên việc khai báo đúng là một phần của việc giữ binding đúng lúc chạy[^nomicon-ffi-decl]. Gọi hàm với ABI sai là một mục trong danh sách UB của Reference[^ref-ub-call] (xem [Chương 07, mục 2](07-unsafe-ub-miri.md)).

**Gateway C++ gọi vào `libpricing`.** Bạn *định nghĩa* trong Rust một hàm `extern "C" fn` để code ngoài, ví dụ C, gọi được[^ref-fn-foreign-call]. Muốn phía ngoài thấy symbol đúng tên, hãy thêm `#[unsafe(no_mangle)]`: nó tắt mangling để symbol mang đúng tên định danh của item[^ref-no-mangle]. Attribute này là `unsafe` vì một symbol không bị mangle có thể đụng tên với symbol khác[^ref-no-mangle-unsafe].

```rust
// minh họa; chưa chạy
use std::ffi::c_int;

unsafe extern "C" {
    fn abs(x: c_int) -> c_int;          // gọi vào libc (hàm unsafe ngầm)
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_add(a: c_int, b: c_int) -> c_int {
    a.wrapping_add(b)                   // gọi ra từ C
}

fn main() {
    let v = unsafe { abs(-3) };
    let _ = (v, rust_add(1, 2));
}
```

<details><summary>Đi sâu: <code>link</code>, variadic, static ngoại, ABI mặc định khi bỏ chuỗi</summary>

- **`link`.** Attribute `link` cho compiler biết tên thư viện native cần liên kết với các item trong khối `extern`[^ref-link-attr]. Nomicon gọi nó là viên gạch cơ bản để chỉ cho rustc cách link thư viện native[^nomicon-link].
- **Bỏ chuỗi ABI** thì mặc định là `"C"`[^ref-ext-default]. Tuy vậy Reference nói cú pháp bỏ ABI đang bị loại dần, nên bạn nên viết rõ[^ref-ext-default-phase].
- **Hàm variadic.** Khối `extern` khai báo được hàm variadic bằng `...` ở tham số cuối[^ref-ext-variadic]. Hàm Rust thường thì không variadic được[^nomicon-no-variadic]. Truyền sai số hoặc sai kiểu đối số cho hàm variadic có thể gây UB, nên qualifier `safe` chỉ hợp khi hàm bảo đảm không đụng vào đối số variadic[^ref-ext-variadic-ub].
- **`static` ngoại.** `static` khai báo trong khối `extern` mặc định là `unsafe` để truy cập, vì không gì bảo đảm bit pattern ở đó hợp lệ với kiểu đã khai báo[^ref-ext-static].
- **`export_name`** mang cùng cảnh báo đụng tên như `no_mangle`[^ref-export-name-unsafe]. Từ edition 2024, `no_mangle` phải viết dạng `unsafe(...)`, trước đó thì không bắt buộc[^ref-no-mangle-ed]. `export_name` cũng vậy[^ref-export-name-ed].

</details>

**Quay lại Q4:** ABI là hợp đồng gọi hàm ở mức máy; `"Rust"` không ổn định, `"C"` là mẫu số chung, và `extern` khai báo hai chiều (gọi vào, bị gọi) đều không được compiler kiểm hộ. Với `billing`, cả `libtax` lẫn `libpricing` đi qua `"C"`.

---

## 5. Kiểu dữ liệu qua ranh giới FFI (Q5)

> **Trả lời ngắn.** Kiểu đi qua FFI nên là `repr(C)`, vì C là ngôn ngữ chung của thế giới lập trình[^nomicon-ffi-reprc]. Con trỏ có thể null thì dùng `Option` quanh kiểu không-null, ví dụ `Option<extern "C" fn(c_int) -> c_int>`[^nomicon-ffi-nullable]. Struct mà C giấu nội dung thì biểu diễn bằng kiểu *opaque*[^nomicon-ffi-opaque].

**Giải thích.**

*Với service `billing`.* Mỗi lần `billing` gọi `libtax`, nó gửi qua ranh giới một dãy thứ: số tiền, con trỏ tới buffer, một callback ghi log mà C có thể không truyền, và nhận lại một handle `TaxContext*` mà C không cho nhìn bên trong (các ví dụ này là giả định của người soạn). Mỗi thứ rơi vào một mục dưới đây.

### 5.1 Layout

Nomicon nói: kiểu bạn định đưa qua FFI nên có `repr(C)`[^nomicon-ffi-reprc]. Với `billing`, struct hóa đơn gửi cho `libtax` vì vậy nên là `repr(C)` (nhận xét của người soạn). Chi tiết `repr(C)`, `repr(transparent)` và enum có `repr` kiểu nguyên nằm ở [Chương 02, mục 2–4](02-type-layout.md), không nhắc lại ở đây.

### 5.2 Con trỏ có thể null

Rust định nghĩa vài kiểu là không bao giờ null: `&T`, `&mut T`, `Box<T>` và con trỏ hàm `extern "abi" fn()`[^nomicon-ffi-nonnull]. Tạo hoặc làm việc với giá trị không hợp lệ đó là UB, nên khi con trỏ đến từ C có thể null, bạn phải dùng `Option` thay vì ép (transmute)[^nomicon-ffi-nonnull-ub]. Ví dụ trong `billing`: một con trỏ hàm log mà phía C có thể để null (nhận xét của người soạn).

Cách này gọi là "nullable pointer optimization": một enum đủ điều kiện khi nó có đúng hai biến thể, một biến thể không chứa dữ liệu và biến thể kia chứa một trường thuộc các kiểu không-null vừa kể[^nomicon-ffi-nullable-def]. Enum như vậy không tốn thêm chỗ cho discriminant, vì biến thể rỗng được biểu diễn bằng giá trị null đặt vào trường không-null[^nomicon-ffi-nullable-nodisc]. Dù tên là "optimization", tối ưu này *được bảo đảm* áp dụng cho kiểu đủ điều kiện[^nomicon-ffi-nullable-guar]. Vì vậy `Option<extern "C" fn(c_int) -> c_int>` là cách biểu diễn đúng cho con trỏ hàm C có thể null[^nomicon-ffi-nullable]. Danh sách kiểu mà std hứa `Option<T>` cùng size/ABI với `T`, và khái niệm niche đứng sau, nằm ở [Chương 02, mục 5–6](02-type-layout.md).

### 5.3 Opaque

Thỉnh thoảng thư viện C đưa bạn một con trỏ tới một thứ mà nó không cho biết cấu trúc bên trong[^nomicon-ffi-opaque]. Handle `TaxContext*` của `libtax` thuộc loại này. Nomicon nêu hai cách xử lý. Cách thứ nhất là `void *`, biểu diễn trong Rust bằng `c_void`[^nomicon-ffi-cvoid]. Cách này hợp lệ, nhưng Nomicon nói còn làm tốt hơn được, vì struct opaque cho một mức an toàn kiểu nhất định[^nomicon-ffi-opaque-struct]. Cách thứ hai là tạo một kiểu `repr(C)` riêng cho mỗi struct opaque, để mỗi cái có kiểu riêng trong Rust[^nomicon-ffi-opaque-rust].

### 5.4 Ai sở hữu gì

Thư viện ngoài thường *giao quyền sở hữu* tài nguyên cho code gọi. Khi đó bạn phải dùng destructor của Rust để bảo đảm tài nguyên được giải phóng, nhất là khi có panic[^nomicon-ffi-destructors]. Với `TaxContext*`, nghĩa là bọc nó trong một struct có `Drop` gọi hàm giải phóng của C (nhận xét của người soạn). Ngoài ra, wrapper nên bọc API thô thành giao diện an toàn và giấu phần unsafe bên trong[^nomicon-ffi-wrap].

<details><summary>Đi sâu: vì sao UB ở một phía lây sang phía kia; <code>longjmp</code>, callback, C++</summary>

- **UB không dừng ở biên ngôn ngữ.** Reference nói UB của C làm cả chương trình có UB, kể cả phần Rust; ngược lại, UB trong Rust có thể ảnh hưởng code ngôn ngữ khác qua FFI[^ref-ub-ffi]. Chi tiết ở [Chương 07, mục 2](07-unsafe-ub-miri.md). Đây là lý do mọi hàm `extern` đều là biên cần kiểm (nhận xét của người soạn).
- **`longjmp`.** Runtime Rust giả định frame Rust không bị giải phóng mà không chạy destructor của biến cục bộ; C như `longjmp` có thể vi phạm giả định đó[^ref-ub-runtime]. Nhận xét của người soạn: đừng để thư viện C `longjmp` qua frame Rust.
- **Callback.** Rust có thể đưa một hàm do mình định nghĩa cho thư viện ngoài, miễn hàm đó được đánh dấu `extern` đúng calling convention. Hàm callback được đăng ký qua một lời gọi vào thư viện C, rồi sau đó được gọi lại từ chính thư viện đó[^nomicon-ffi-callback].
- **C++.** Rust hiện không gọi thẳng được vào thư viện C++, nên ví dụ của Nomicon dùng thư viện có giao diện C[^nomicon-ffi-cpp].

</details>

**Quay lại Q5:** qua FFI chỉ gửi kiểu có layout cố định (`repr(C)`), dùng `Option<kiểu không-null>` cho con trỏ có thể null, opaque cho thứ C giấu, và nhớ ai sở hữu và giải phóng. Với `billing`, đó là struct hóa đơn, callback log và handle `TaxContext*`.

---

## 6. Panic băng qua ranh giới FFI: `-unwind` ABI (Q6)

> **Trả lời ngắn.** Mỗi ABI thuộc một trong hai nhóm: cho phép unwind hoặc không[^ref-fn-categories]. Nếu panic của Rust chạm ranh giới không cho unwind thì process bị abort an toàn; còn exception ngoại lai, ví dụ của C++, đi vào Rust qua ranh giới như vậy là UB[^nomicon-ffi-boundary]. Muốn panic hoặc exception xuyên qua ranh giới thì phải dùng biến thể `-unwind` của ABI, ví dụ `"C-unwind"`[^nomicon-ffi-unwind].

**Giải thích.**

*Với service `billing`.* Gateway C++ gọi `price_order` của `libpricing`, và hàm đó panic giữa chừng. Panic có chạy ngược vào frame C++ của gateway không? Ngược lại, nếu `billing` gọi `libtax` và `libtax` gọi lại một callback Rust rồi callback đó panic thì sao? Cơ chế unwind của Rust không được đặc tả để tương thích với cơ chế unwind của ngôn ngữ nào khác[^nomicon-unwinding-ffi], nên đến ranh giới hai bên phải có luật. Mục này là bộ luật đó; biến thể `-unwind` của ABI chỉ nên dùng khi cả hai bên cố ý thiết kế để unwind xuyên qua nhau (nhận xét của người soạn).

### 6.1 Ba quy tắc nền

1. Hầu hết ABI có hai biến thể, có và không có hậu tố `-unwind`; ABI Rust *luôn* cho phép unwind nên không có "Rust-unwind"[^ref-fn-unwind-intro]. `"C-unwind"` giống `"C"` nhưng khác ở hành vi khi callee unwind, bằng panic hoặc bằng exception kiểu C++[^ref-abi-unwind].
2. ABI cộng với panic runtime quyết định hành vi khi unwind chạm ranh giới[^ref-fn-unwind-intro]. Reference tóm tắt trong một bảng về hành vi khi một thao tác unwind chạm từng loại ranh giới ABI[^ref-fn-table-intro]; bảng được chép lại ở §6.2. Cột "panic" gồm cả `panic!` lẫn các thao tác Rust khác gây panic, như chỉ số mảng ngoài biên hay tràn số nguyên[^ref-fn-panic-col]. Cột cuối là unwind "native" theo từng target: trên target hỗ trợ exception C++ thì đó là cơ chế cài đặt chúng. "Forced unwinding" như `longjmp` trên Windows hay `pthread_exit` của glibc bị loại khỏi cột này[^ref-fn-native].
3. Unwind vào Rust từ một hàm ngoại được gọi qua ABI không-unwind là UB[^ref-panic-ffi-undef1]. Gọi một hàm Rust có thể unwind từ code không hỗ trợ unwind, như C biên dịch với `-fno-exceptions`, cũng là UB[^ref-panic-ffi-undef2].

### 6.2 Bảng hành vi

| Panic runtime | ABI của ranh giới | Panic Rust tới đây | Exception ngoại lai (native, không cưỡng bức) tới đây | Nguồn |
|---|---|---|---|---|
| `panic=unwind` | unwinding (`"Rust"`, `"C-unwind"`, …) | unwind tiếp | unwind tiếp | [^ref-fn-row1] |
| `panic=unwind` | không unwind (`"C"`, `"stdcall"`, …) | abort (xem ghi chú dưới bảng) | UB | [^ref-fn-row2] |
| `panic=abort` | unwinding | abort, không unwind | abort | [^ref-fn-row3] |
| `panic=abort` | không unwind | abort, không unwind | UB | [^ref-fn-row4] |

(Bảng chép lại từ Reference, mỗi dòng có chú thích riêng ở cột cuối. Cách phân nhóm ABI "unwinding" và "non-unwinding" theo định nghĩa của Reference[^ref-fn-categories].)

Hai điều cần nhớ khi đọc bảng. Thứ nhất, với `panic=unwind`, khi panic bị ranh giới không-unwind đổi thành abort thì hoặc *không* destructor nào chạy, hoặc *tất cả* destructor tới ranh giới sẽ chạy; Reference nói *không xác định* trường hợp nào[^ref-fn-abort]. Đừng viết code phụ thuộc vào cái nào (nhận xét của người soạn). Thứ hai, với `panic=abort`, `panic!` abort ngay bất kể ABI của hàm đang panic[^nomicon-panic-abort-note].

### 6.3 Hàm `extern "C"` panic thì abort an toàn, không phải UB

Ví dụ của Nomicon: `extern "C" fn assert_nonzero(input: u32)` chứa `assert!(input != 0)`. Gọi với 0 thì runtime *bảo đảm* abort process một cách an toàn, dù có biên dịch với `panic=abort` hay không[^nomicon-ffi-abort]. Dòng 2 của bảng cho cùng kết luận[^ref-fn-row2]. std cũng nói như vậy: hàm Rust sẽ bị code không hỗ trợ unwind gọi thì nên định nghĩa bằng `extern "C"`, vì khi đó panic tự động bị bắt và process bị abort; muốn xử lý nhẹ nhàng hơn thì mới cần `catch_unwind`[^catch-extern-c]. Với `billing`: `price_order` khai báo `extern "C"` mà panic thì process của gateway bị abort, chứ không phải runtime C++ của họ bị hỏng ngầm (nhận xét của người soạn).

### 6.4 Đối chiếu với danh sách UB

Danh sách UB của Reference có một mục "unwind qua frame không cho phép unwind", với ví dụ gọi hàm `"C-unwind"` bị import hoặc transmute thành `"C"`[^ref-ub-call]. Reference cũng xếp vào UB việc unwind từ code ngoại vào Rust qua ABI không-unwind, tức quy tắc 3 ở trên[^ref-panic-ffi-undef1]. Hai điều này không mâu thuẫn với "panic ⇒ abort an toàn" nếu phân biệt hai trường hợp. UB xảy ra khi *ABI khai báo không khớp với hàm thật*, hoặc khi *exception ngoại lai* đi vào Rust. Còn panic của một hàm Rust *định nghĩa* bằng `extern "C"` là trường hợp mà bảng và Nomicon mô tả là abort (suy luận của người soạn: Reference không nối các đoạn này với nhau). Nguyên tắc thực hành: khai báo ABI phải khớp hàm thật ở cả hai phía; xem thêm [Chương 07, mục 2](07-unsafe-ub-miri.md).

### 6.5 Dùng `"C-unwind"` đúng chỗ

Nomicon cho hai ví dụ. Ví dụ thứ nhất: hàm Rust `extern "C-unwind"` khi panic (biên dịch với `panic=unwind`) được phép unwind qua frame C++[^nomicon-ffi-cunwind]. Nếu frame C++ có object thì destructor của chúng sẽ được gọi[^nomicon-ffi-cpp-dtor]. Ví dụ thứ hai: hàm Rust `"C-unwind"` gọi một hàm C++ có thể throw. Nếu throw xảy ra thì `Box` cục bộ được drop, nếu không thì in giá trị bình thường[^nomicon-ffi-throw].

<details><summary>Đi sâu: một trang Nomicon nói khác, bắt exception ngoại lai, "bản Rust khác" cũng là ngoại lai, hạn chế khi rethrow</summary>

- **Một trang Nomicon nói khác.** Trang "Unwinding" của Nomicon (cùng bản 1.95.0) viết rằng unwind từ ngôn ngữ khác vào Rust, hoặc từ Rust sang ngôn ngữ khác, là UB và bạn phải bắt mọi panic ở ranh giới FFI[^nomicon-unwinding-ffi]. Đó là phát biểu chung, không phân biệt chuỗi ABI. Trang FFI của cùng Nomicon thì phân biệt theo ABI: panic Rust chạm ranh giới không-unwind ⇒ abort an toàn[^nomicon-ffi-boundary]. Cũng trang đó nói `-unwind` cho phép panic xuyên ranh giới[^nomicon-ffi-cunwind]. Chương này theo Reference và trang FFI vì chúng chi tiết hơn và có bảng (nhận xét của người soạn). Điểm chung của mọi nguồn là đừng để panic băng qua FFI một cách vô ý: hoặc `catch_unwind`, hoặc chấp nhận abort, hoặc chủ động dùng `-unwind` ở cả hai phía.
- **Bắt exception ngoại lai.** Nếu bạn bắt bằng `catch_unwind`, bằng `JoinHandle::join`, hoặc để nó lan ra ngoài `main` hay gốc thread, kết quả là một trong hai và *không xác định* là cái nào: process abort, hoặc hàm trả `Err` chứa một kiểu opaque[^ref-panic-catch-foreign]. Docs `catch_unwind` nói cùng ý, và thêm rằng nếu abort thì abort *sau khi* chạy mọi destructor của `f` và các hàm nó gọi[^catch-foreign].
- **Tương tác với cơ chế bắt của ngôn ngữ khác.** Nomicon nhấn: tương tác của `catch_unwind` với exception ngoại lai là không xác định, cũng như tương tác của panic với cơ chế bắt exception của ngôn ngữ khác, đáng chú ý là `try/catch` của C++[^nomicon-ffi-undef-catch].
- **Một bản runtime Rust khác cũng là "ngoại lai".** Một thư viện dùng `panic!` và link với một bản `std`, nếu được gọi từ ứng dụng dùng bản `std` khác, có thể làm cả ứng dụng abort dù chỉ dùng trong thread con[^ref-panic-other-rt].
- **Hủy hoặc rethrow payload.** Chưa có bảo đảm nào về việc runtime ngoại lai hủy hoặc rethrow payload panic của Rust. Nói cách khác, unwind sinh từ runtime Rust phải dẫn tới kết thúc process hoặc bị chính runtime đó bắt[^ref-panic-dispose].
- **Lint.** Với `-unwind` ABI, lint `ffi_unwind_calls` đánh dấu mọi lời gọi tới hàm hay con trỏ ngoại `-unwind`, hữu ích nếu bạn muốn thư viện sound với mọi panic runtime lúc link[^ref-link-lint].

</details>

**Quay lại Q6:** ranh giới không cho unwind thì panic ⇒ abort, exception ngoại lai ⇒ UB; muốn xuyên ranh giới phải chọn `-unwind` ở cả hai phía. Với `billing`, `price_order` khai báo `"C"` sẽ abort process thay vì để panic chạy vào gateway C++.

---

## 7. Mở API Rust cho C: tường chắn panic và quy tắc build (Q7)

> **Trả lời ngắn.** Hàm `extern "C"` mà panic sẽ abort process an toàn[^nomicon-ffi-abort]. Nếu bạn không muốn chết thì phải `catch_unwind` *trước* ranh giới và đổi thành mã lỗi[^nomicon-ffi-catch]. Ngoài ra, nếu artifact có khả năng unwind thì mọi crate trong nó phải build theo `unwind`, nếu không unwind có thể là UB[^ref-link-unwind-prohibited].

**Giải thích.**

*Với service `billing`.* Gateway C++ gọi `price_order` của `libpricing` cho mỗi đơn hàng. Với gateway, `price_order` là hộp đen: lỗi phải về bằng mã lỗi, chứ không phải bằng một panic lan vào runtime của họ (nhận xét của người soạn). Mục này là ba bước để `billing` làm được điều đó: xuất symbol, chắn panic, và build đúng.

### 7.1 Bước 1: xuất symbol

Nomicon nói bạn cần `extern "C"` để hàm theo calling convention C, và `no_mangle` để hàm có symbol với tên xác định mà bên kia link được[^nomicon-ffi-callrust]. Muốn biên dịch thành thư viện chung, đặt `crate-type = ["cdylib"]` trong `Cargo.toml`[^nomicon-ffi-cdylib].

### 7.2 Bước 2: chắn panic

Bạn chọn một trong hai.

- **Chấp nhận abort.** Như ở mục 6, hàm `extern "C"` panic thì process abort an toàn[^nomicon-ffi-abort]. Cách này đơn giản và an toàn, nhưng một bug trong `libpricing` sẽ giết cả process của gateway.
- **Chuyển thành mã lỗi.** Nomicon nói nếu code Rust có thể panic và bạn không muốn abort khi nó panic thì *phải* dùng `catch_unwind`[^nomicon-ffi-catch]. Ví dụ của Nomicon dùng `catch_unwind` rồi trả `0` nếu thành công và `1` nếu `Err`[^nomicon-ffi-catch-ex]. Với `billing`, mã lỗi đó chính là thứ gateway đọc được.

```rust
// minh họa; chưa chạy
use std::panic::catch_unwind;

#[unsafe(no_mangle)]
pub extern "C" fn do_work(x: u32) -> i32 {
    match catch_unwind(|| risky(x)) {
        Ok(v) => v,
        Err(_) => -1,           // báo lỗi cho phía C bằng mã lỗi
    }
}

fn risky(x: u32) -> i32 {
    assert!(x != 0);
    100 / x as i32
}
```

Cần nhớ giới hạn của cách này: `catch_unwind` chỉ bắt panic dạng unwind, không bắt panic làm abort[^nomicon-catch-only]. Vì vậy nếu bạn build `panic=abort` thì tường chắn này không hoạt động, vì process đã abort ngay từ `panic!` (hệ quả của câu trích; nhận xét của người soạn).

### 7.3 Bước 3: quy tắc build

Một artifact Rust là "có khả năng unwind" nếu thỏa một trong ba điều kiện: nó dùng panic handler `unwind`; nó chứa một crate build `unwind` có gọi hàm qua ABI `-unwind`; hoặc nó gọi qua ABI `"Rust"` vào một artifact Rust khác, có bản runtime riêng, mà artifact kia có khả năng unwind[^ref-link-potential]. Nếu artifact có khả năng unwind thì *mọi* crate của nó phải build với chiến lược `unwind`, nếu không unwind có thể gây UB[^ref-link-unwind-prohibited]. Khi `rustc` làm bước link, quy tắc này được thực thi tự động; nếu link không qua rustc, kể cả `dlopen`, bạn phải tự bảo đảm[^ref-link-note].

<details><summary>Đi sâu: khi nào dùng <code>"C-unwind"</code> thay vì chắn; thiết kế API; checklist</summary>

- **Khi nào dùng `"C-unwind"`.** Nếu hàm Rust *cố ý* để panic xuyên sang C++ (để C++ `catch`) thì dùng `"C-unwind"` và `panic=unwind`[^nomicon-ffi-cunwind]. Chỉ làm vậy khi hai bên cùng hỗ trợ unwind: nếu phía gọi biên dịch kiểu `-fno-exceptions` thì việc gọi vào hàm Rust có thể unwind là UB[^ref-panic-ffi-undef2].
- **`resume_unwind`** hữu ích để mang một panic đã bắt qua một lớp code C rồi ném tiếp (docs gợi ý đúng ý này), vì nó không gọi lại hook[^resume].
- **Giữ ranh giới hẹp.** Nhận và trả kiểu `repr(C)`, con trỏ opaque, mã lỗi (mục 5). Nhận xét của người soạn: đặt wrapper an toàn phía Rust để phần `unsafe` nằm gọn một chỗ. Nomicon cũng nói một thư viện có thể chỉ phơi giao diện an toàn, cấp cao và giấu phần unsafe bên trong[^nomicon-ffi-wrap].
- **Đừng dùng panic hook làm cơ chế báo lỗi cho phía C** (nhận xét của người soạn): hook là tài nguyên toàn cục của process[^hook-global], và nó chạy trước mọi bước (mục 1).
- **Checklist** (nhận xét của người soạn, tổng hợp từ các mục trên): (1) mọi hàm xuất đều `extern "C"` hoặc `-unwind`, viết rõ; (2) đã quyết định abort hay chuyển lỗi; (3) nếu artifact có khả năng unwind thì mọi crate trong nó build `unwind`; (4) kiểu qua ranh giới là `repr(C)`, opaque hoặc `Option<kiểu không-null>`.

</details>

**Quay lại Q7:** hàm `extern "C"` panic thì mặc định abort; muốn thành mã lỗi thì `catch_unwind` trong từng hàm xuất, và artifact có khả năng unwind phải build mọi crate theo `unwind`. Với `billing`, `price_order` bọc `catch_unwind` và trả mã lỗi cho gateway.

---

## Liên hệ với Rust

Với `billing`, chương này không đổi dòng code nào của bạn; nó cho bạn biết năm điều. ABI là thứ bạn ghi tường minh trên từng hàm. Panic không thay `Result`. Ba lớp "phanh" của panic đều là cảnh báo chứ không phải rào chắn cứng. Danh sách UB của Reference có đúng mục "gọi sai ABI". Và `panic = "abort"` là một quyết định vận hành chứ không chỉ là cờ build (nhận xét của người soạn).

**Hợp đồng ABI nằm trong ngôn ngữ.** Rust cho bạn ghi ABI tường minh trên từng hàm (`extern "C"`, `"C-unwind"`, `"system"`). Khai báo ABI không khớp hàm thật là UB[^ref-ub-call]. Compiler thì không kiểm được khai báo ngoại[^nomicon-ffi-decl].

**Panic không phải một phần của `Result`.** Panic dành cho lỗi lập trình hoặc trạng thái không phục hồi, `Result` dành cho lỗi thường xuyên. Docs `catch_unwind` nhắc lại đúng điều này[^catch-notgeneral]; tiêu chí chi tiết ở [P9 · 05](../09-rust-language/05-error-handling.md).

**Ba lớp "phanh" của panic (cách gọi của người soạn).** Lớp thứ nhất là `Drop`, dọn dẹp khi unwind, do ngôn ngữ bảo đảm[^ref-panic-unwind]. Lớp thứ hai là poisoning, để đừng dùng mù quáng dữ liệu dở dang. Lớp thứ ba là `UnwindSafe`, cảnh báo ở `catch_unwind`. Hai lớp sau là cơ chế *thư viện và kiểu* chỉ mang tính cảnh báo. Poisoning chỉ khuyến cáo[^mutex-advisory]. `UnwindSafe` không phải `unsafe trait`[^unwindsafe-speedbump].

**Danh sách UB của Reference có đúng mục này.** Gọi sai ABI hoặc unwind qua frame không cho unwind đều nằm trong danh sách[^ref-ub-call]. Mục đó không đồng nghĩa với "mọi panic chạm `extern "C"` là UB" (xem mục 6 và [Chương 07, mục 2](07-unsafe-ub-miri.md)).

**Chọn bằng Cargo.** `panic = "abort"` trong profile đổi chiến lược panic của build đó, còn test, bench, build script và proc macro bỏ qua cài đặt này và vẫn unwind[^cargo-test-ignore]. Override theo từng package cũng không được đặt `panic`[^cargo-no-override]. Với service backend như `billing`, quyết định này nên đi kèm câu hỏi "ai khởi động lại tiến trình, và có mất log đang buffer không" (nhận xét của người soạn; xem mục 2).

---

## Tóm tắt

- Panic: hook chạy trước[^hook-before]. Rồi với `unwind`, runtime tháo stack và gọi `Drop`[^ref-panic-unwind]. Quá trình dừng ở điểm phục hồi: ranh giới thread hoặc `catch_unwind`[^ref-panic-recover].
- `-C panic` hoặc `panic` trong profile chọn `unwind` hay `abort`[^cargo-panic]. Mặc định là `unwind` ở đa số target[^ref-panic-default]. Với `abort`, `catch_unwind` vô tác dụng[^catch-notall], và theo suy luận ở mục 2 thì destructor cũng không chạy. Đổi lại, trình tối ưu có thể làm code nhỏ và nhanh hơn[^ref-panic-abort-opt].
- Một crate `abort` trong đồ thị kéo cả binary thành `abort`[^rustc-panic-flag]. Test thì vẫn unwind[^cargo-test-ignore].
- `catch_unwind` dành cho biên của đơn vị công việc hoặc FFI, không phải try/catch[^catch-notgeneral]. `UnwindSafe` là "speed bump"[^unwindsafe-speedbump]. Poisoning chỉ mang tính khuyến cáo[^mutex-advisory].
- ABI `"Rust"` không ổn định[^ref-abi-rust]. `"C"` khớp C compiler chủ đạo của target[^ref-abi-c]. `extern` hai chiều đều không được compiler kiểm hộ[^nomicon-ffi-decl].
- Qua FFI, dùng `repr(C)`[^nomicon-ffi-reprc]. Con trỏ có thể null thì dùng `Option<kiểu không-null>`[^nomicon-ffi-nullable]. Thứ bị giấu thì dùng opaque[^nomicon-ffi-opaque].
- Ranh giới không-unwind: panic ⇒ abort an toàn, exception ngoại lai ⇒ UB[^nomicon-ffi-boundary]. Muốn xuyên qua thì dùng `-unwind` ở cả hai phía[^nomicon-ffi-unwind].
- Mục UB "unwind qua frame không cho unwind" trong danh sách của Reference có ví dụ là khai báo ABI không khớp hàm thật, tức gọi `"C-unwind"` như `"C"`[^ref-ub-call].
- `extern "C"` panic ⇒ abort[^nomicon-ffi-abort]. Muốn mã lỗi thì `catch_unwind` trước ranh giới[^nomicon-ffi-catch]. Cách này chỉ có tác dụng với `panic=unwind`[^nomicon-catch-only].
- Artifact "có khả năng unwind" phải build mọi crate với `unwind`[^ref-link-unwind-prohibited].

---

## Câu hỏi tự kiểm tra

1. Trong `billing`, panic hook chạy trước hay sau khi stack được unwind? Nó có chạy dưới `panic=abort` không?
2. `catch_unwind` có bắt được panic khi build `panic=abort` không? Vì sao?
3. Đặt `panic = "abort"` trong `[profile.release]` thì `cargo test` hành xử thế nào với panic?
4. Nếu một crate trong đồ thị dùng `abort` thì binary cuối phải thế nào?
5. Vì sao `UnwindSafe` không phải `unsafe trait`, và trait đó làm gì?
6. Mutex poisoning bảo vệ điều gì; có bảo vệ an toàn bộ nhớ không? Có vượt qua được không?
7. ABI `"Rust"` khác `"C"` ở điểm cốt lõi nào, và vì sao hàm Rust thường không nên đưa cho code ngoài gọi?
8. Vì sao `Option<extern "C" fn(c_int) -> c_int>` là cách biểu diễn con trỏ hàm C có thể null?
9. Panic Rust chạm ranh giới `"C"` (với `panic=unwind`) sẽ ra sao? Exception C++ đi vào hàm Rust khai báo `"C"` thì sao?
10. Hàm `extern "C"` của bạn gọi code có thể panic; muốn trả mã lỗi thay vì abort, bạn làm gì, và nó vô tác dụng khi nào?
11. Liệt kê ba điều kiện khiến một artifact Rust được gọi là "có khả năng unwind", và hệ quả của nó.
12. Danh sách UB của Reference có mục "unwind qua frame không cho phép unwind", còn Nomicon nói panic chạm hàm `extern "C"` thì abort an toàn. Hai điều đó khớp nhau thế nào?

<details><summary>Đáp án</summary>

1. Trước. Hook chạy khi thread panic, trước khi panic runtime được gọi, nên chạy với cả runtime unwind và abort[^hook-before]. `process::abort` thuần thì không gọi hook, còn `panic!` dưới `panic=abort` thì có[^abort-similar]. Ngoại lệ: `immediate-abort` không gọi hook[^rustc-panic-flag].
2. Không: `catch_unwind` chỉ bắt panic dạng unwind, không bắt panic làm process abort[^catch-notall]. Nomicon nhắc lại điều đó ở trang FFI[^nomicon-catch-only].
3. Cargo bỏ qua `panic` cho test, bench, build script và proc macro; test harness đòi unwind[^cargo-test-ignore].
4. Phải `abort`[^rustc-panic-flag].
5. Vì nó không có hợp đồng "chặt", chỉ là cảnh báo; nó mã hóa khái niệm exception safety vào kiểu, như một "speed bump" cho người dùng `catch_unwind`[^unwindsafe-speedbump]. Việc đưa khái niệm vào kiểu là mục đích của bound `UnwindSafe` trên `catch_unwind`[^catch-bound].
6. Nó chống việc dùng mù quáng dữ liệu có thể đang sửa dở[^nomicon-poison-why]. Nó không liên quan an toàn bộ nhớ: kiểu viết đúng thì vẫn không vi phạm bộ nhớ[^nomicon-poison-mem]. Bạn vượt qua được bằng `into_inner`[^mutex-advisory].
7. ABI Rust không cam kết ổn định[^ref-abi-rust]. `"C"` khớp C compiler chủ đạo của target[^ref-abi-c]. Vì thế hàm Rust thường không nên đưa cho code ngoài gọi: cách gọi của nó có thể đổi giữa các bản compiler (suy luận của người soạn từ việc ABI Rust không ổn định).
8. Vì kiểu hàm `extern "abi" fn()` là không-null[^nomicon-ffi-nonnull]. Enum hai biến thể, một rỗng và một chứa kiểu không-null, biểu diễn `None` bằng null và không thêm discriminant[^nomicon-ffi-nullable-def]. Biến thể rỗng nằm ngay trong trường không-null[^nomicon-ffi-nullable-nodisc]. Tối ưu này được bảo đảm cho kiểu đủ điều kiện[^nomicon-ffi-nullable-guar]. Nomicon nêu đúng `Option<extern "C" fn(c_int) -> c_int>` là cách biểu diễn con trỏ hàm C có thể null[^nomicon-ffi-nullable].
9. Panic ⇒ process abort an toàn; exception ngoại lai vào Rust ⇒ UB[^nomicon-ffi-boundary]. Với panic, destructor chạy hay không là không xác định[^ref-fn-abort].
10. Bọc phần có thể panic trong `catch_unwind` và đổi `Err` thành mã lỗi[^nomicon-ffi-catch]. Cách này vô tác dụng khi build `panic=abort`[^nomicon-catch-only].
11. Dùng handler `unwind`; hoặc có crate build `unwind` gọi hàm `-unwind`; hoặc gọi ABI `"Rust"` vào artifact Rust khác (runtime riêng) mà artifact đó cũng có khả năng unwind[^ref-link-potential]. Hệ quả: mọi crate của artifact phải build `unwind`, nếu không có thể UB[^ref-link-unwind-prohibited].
12. UB là khi ABI khai báo không khớp với hàm thật, ví dụ gọi một hàm `"C-unwind"` được import hoặc transmute thành `"C"`[^ref-ub-call]. UB cũng là khi exception ngoại lai đi vào Rust qua ABI không-unwind[^ref-panic-ffi-undef1]. Panic của hàm Rust định nghĩa bằng `extern "C"` chạm ranh giới không-unwind là abort an toàn[^nomicon-ffi-abort]. Reference không nối các đoạn này với nhau; cách đọc trên là suy luận của người soạn (mục 6).

</details>

---

## Đọc thêm

- [Reference — Panic](https://doc.rust-lang.org/1.95.0/reference/panic.html): panic handler, chiến lược panic, unwinding qua ranh giới FFI.
- [Reference — Functions, mục Unwinding](https://doc.rust-lang.org/1.95.0/reference/items/functions.html): bảng hành vi ABI × panic runtime.
- [Reference — External blocks](https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html): danh sách ABI, `safe`/`unsafe`, variadic, `link`.
- [Nomicon — Unwinding](https://doc.rust-lang.org/1.95.0/nomicon/unwinding.html) và [Exception Safety](https://doc.rust-lang.org/1.95.0/nomicon/exception-safety.html): lịch sử, chi phí, và cách code unsafe sống chung với panic. Lưu ý: đoạn cuối trang Unwinding nói chung chung rằng unwind qua FFI là UB; mục 6 chương này đối chiếu nó với Reference và trang FFI.
- [Nomicon — FFI](https://doc.rust-lang.org/1.95.0/nomicon/ffi.html): ví dụ gọi vào/ra, callback, opaque, `C-unwind`.
- [`std::panic`](https://doc.rust-lang.org/1.95.0/std/panic/index.html): `catch_unwind`, `set_hook`, `resume_unwind`, `UnwindSafe`.
- [Cargo — Profiles, `panic`](https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html): cấu hình chiến lược panic theo profile.
- [P2 · Chương 04, mục 1, 6 và 8](../02-cpu/04-calling-convention-va-stack-frame.md): ABI là gì, bảng unwind, so sánh asm của `extern "Rust"` và `extern "C"`.
- [P9 · Chương 05, mục 1 và 7](../09-rust-language/05-error-handling.md): tiêu chí panic vs `Result`; quan sát chạy thật `drop` dưới `panic=abort`.
- [Chương 07 — unsafe, undefined behavior và Miri, mục 2](07-unsafe-ub-miri.md): định nghĩa UB và danh sách UB mà mục 6 đối chiếu.
- [P4 · Chương 08, mục 6](../04-concurrency/08-rust-concurrency.md): poisoning ở mức concurrency (`RwLock`, `Condvar`).

[^ref-panic-intro]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "Rust provides a mechanism to prevent a function from returning normally, and instead “panic,” which is a response to an error condition that is typically not expected to be recoverable within the context in which the error is encountered."
[^ref-panic-lang]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "Some language constructs, such as out-of-bounds array indexing, panic automatically."
[^nomicon-excsafe-everything]: The Rustonomicon, Exception Safety — <https://doc.rust-lang.org/1.95.0/nomicon/exception-safety.html> — "If you unwrap a None, index out of bounds, or divide by 0, your program will panic. On debug builds, every arithmetic operation can panic if it overflows. Unless you are very careful and tightly control what code runs, pretty much everything can unwind, and you need to be ready for it."
[^nomicon-tiers]: The Rustonomicon, Unwinding — <https://doc.rust-lang.org/1.95.0/nomicon/unwinding.html> — "If something might reasonably be absent, Option is used. If something goes wrong and can reasonably be handled, Result is used. If something goes wrong and cannot reasonably be handled, the thread panics. If something catastrophic happens, the program aborts."
[^nomicon-unwind]: The Rustonomicon, Unwinding — <https://doc.rust-lang.org/1.95.0/nomicon/unwinding.html> — "Panics cause the thread to halt normal execution and unwind its stack, calling destructors as if every function instantly returned."
[^hook-before]: Rust std, panic::set_hook — <https://doc.rust-lang.org/1.95.0/std/panic/fn.set_hook.html> — "The panic hook is invoked when a thread panics, but before the panic runtime is invoked. As such, the hook will run with both the aborting and unwinding runtimes."
[^hook-default]: Rust std, panic::set_hook — <https://doc.rust-lang.org/1.95.0/std/panic/fn.set_hook.html> — "The default hook, which is registered at startup, prints a message to standard error and generates a backtrace if requested."
[^ref-panic-unwind]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "When a panic occurs, the unwind handler “unwinds” Rust frames, just as C++’s throw unwinds C++ frames, until the panic reaches the point of recovery (for instance at a thread boundary). This means that as the panic traverses Rust frames, live objects in those frames that implement Drop will have their drop methods called."
[^ref-panic-recover]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "The standard library provides two mechanisms for recovering from a panic, std::panic::catch_unwind (which enables recovery within the panicking thread) and std::thread::spawn (which automatically sets up panic recovery for the spawned thread so that other threads may continue running)."
[^thread-panic]: Rust std, thread — <https://doc.rust-lang.org/1.95.0/std/thread/index.html> — "If the panic is not caught the thread will exit, but the panic may optionally be detected from a different thread with join. If the main thread panics without the panic being caught, the application will exit with a non-zero exit code."
[^nomicon-unwind-cost]: The Rustonomicon, Unwinding — <https://doc.rust-lang.org/1.95.0/nomicon/unwinding.html> — "In particular, Rust’s current unwinding implementation is heavily optimized for the “doesn’t unwind” case. If a program doesn’t unwind, there should be no runtime cost for the program being ready to unwind. As a consequence, actually unwinding will be more expensive than in e.g. Java. Don’t build your programs to unwind under normal circumstances."
[^nomicon-unwind-hist]: The Rustonomicon, Unwinding — <https://doc.rust-lang.org/1.95.0/nomicon/unwinding.html> — "Light-weight tasks were killed in the name of heavy-weight OS threads. Still, on stable Rust as of 1.0 panics can only be caught by the parent thread. This means catching a panic requires spinning up an entire OS thread! This unfortunately stands in conflict to Rust’s philosophy of zero-cost abstractions. There is an API called catch_unwind that enables catching a panic without spawning a thread."
[^ref-unwind-impl]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "As long as this guarantee of resource-cleanup is preserved, “unwinding” may be implemented without actually using the mechanism used by C++ for the target platform."
[^panic-any]: Rust std, panic::panic_any — <https://doc.rust-lang.org/1.95.0/std/panic/fn.panic_any.html> — "The message can be of any (Any + Send) type, not just strings."
[^catch-ret]: Rust std, panic::catch_unwind — <https://doc.rust-lang.org/1.95.0/std/panic/fn.catch_unwind.html> — "This function will return Ok with the closure’s result if the closure does not panic, and will return Err(cause) if the closure panics. The cause returned is the object with which panic was originally invoked."
[^dev-mir-call]: rustc-dev-guide, The MIR — <https://rustc-dev-guide.rust-lang.org/mir/index.html> — "Function calls like the call to Vec::new are always terminators because of the possibility of unwinding"
[^thread-panicking]: Rust std, thread::panicking — <https://doc.rust-lang.org/1.95.0/std/thread/fn.panicking.html> — "Determines whether the current thread is unwinding because of panic. A common use of this feature is to poison shared resources when writing unsafe code, by checking panicking when the drop is called. This is usually not needed when writing safe code, as Mutexes already poison themselves when a thread panics while holding the lock."
[^ref-panic-kinds]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "unwind — unwinds the stack and is potentially recoverable. abort — aborts the process and is non-recoverable."
[^abort-doc]: Rust std, process::abort — <https://doc.rust-lang.org/1.95.0/std/process/fn.abort.html> — "no destructors on the current stack or any other thread’s stack will be run, Rust IO buffers (eg, from BufWriter) will not be flushed, and C stdio buffers will (on most platforms) not be flushed."
[^catch-notall]: Rust std, panic::catch_unwind — <https://doc.rust-lang.org/1.95.0/std/panic/fn.catch_unwind.html> — "This function only catches unwinding panics, not those that abort the process."
[^ref-panic-abort-opt]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "When compiling code with the abort panic strategy, the optimizer may assume that unwinding across Rust frames is impossible, which can result in both code-size and runtime speed improvements."
[^ref-panic-default]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "The default for most targets is unwind."
[^ref-panic-notall]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "Not all targets may provide the unwind handler."
[^rustc-panic-flag]: The rustc book, Codegen options — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "abort: terminate the process upon panic immediate-abort: terminate the process upon panic, and do not call any panic hooks unwind: unwind the stack upon panic If not specified, the default depends on the target. If any crate in the crate graph uses abort, the final binary (bin, dylib, cdylib, staticlib) must also use abort."
[^cargo-panic]: The Cargo Book, Profiles — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "The panic setting controls the -C panic flag which controls which panic strategy to use. The valid options are: "unwind": Unwind the stack upon panic. "abort": Terminate the process upon panic."
[^cargo-defaults]: The Cargo Book, Profiles — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "overflow-checks = true lto = false panic = 'unwind' incremental = true codegen-units = 256 rpath = false"
[^cargo-defaults-release]: The Cargo Book, Profiles — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "debug-assertions = false overflow-checks = false lto = false panic = 'unwind' incremental = false codegen-units = 16 rpath = false"
[^ref-panic-implication]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "An implication is that crates built with the unwind strategy can use the abort panic handler, but the abort strategy cannot use the unwind panic handler."
[^cargo-test-ignore]: The Cargo Book, Profiles — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "Tests, benchmarks, build scripts, and proc macros ignore the panic setting. The rustc test harness currently requires unwind behavior."
[^abort-similar]: Rust std, process::abort — <https://doc.rust-lang.org/1.95.0/std/process/fn.abort.html> — "When panic="abort" is set, either as an argument to rustc or in a crate’s Cargo.toml, panic! and abort are similar. However, panic! will still call the panic hook while abort will not."
[^abort-sigabrt]: Rust std, process::abort — <https://doc.rust-lang.org/1.95.0/std/process/fn.abort.html> — "On Unix, the process will terminate with signal SIGABRT, which typically means that the shell prints “Aborted”."
[^ref-panic-notrecover]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "the unwind handler does not guarantee that all panics are recoverable, only that panicking via the panic! macro and similar standard library mechanisms is recoverable."
[^ref-panic-nostd]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "Linking a no_std binary, dylib, cdylib, or staticlib will require specifying your own panic handler."
[^ref-panic-handler-unique]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "There must be a single panic_handler function in the dependency graph."
[^cargo-abort-test]: The Cargo Book, Profiles — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "See the panic-abort-tests unstable flag which enables abort behavior. Additionally, when using the abort strategy and building a test, all of the dependencies will also be forced to build with the unwind strategy."
[^catch-notgeneral]: Rust std, panic::catch_unwind — <https://doc.rust-lang.org/1.95.0/std/panic/fn.catch_unwind.html> — "It is not recommended to use this function for a general try/catch mechanism. The Result type is more appropriate to use for functions that can fail on a regular basis."
[^catch-hook]: Rust std, panic::catch_unwind — <https://doc.rust-lang.org/1.95.0/std/panic/fn.catch_unwind.html> — "If a custom panic hook has been set, it will be invoked before the panic is caught, before unwinding."
[^unwindsafe-two]: Rust std, panic::UnwindSafe — <https://doc.rust-lang.org/1.95.0/std/panic/trait.UnwindSafe.html> — "A data structure is in a temporarily invalid state when the thread panics. This broken invariant is then later observed."
[^unwindsafe-nomem]: Rust std, panic::UnwindSafe — <https://doc.rust-lang.org/1.95.0/std/panic/trait.UnwindSafe.html> — "Another key aspect of unwind safety in Rust is that, in the absence of unsafe code, a panic cannot lead to memory unsafety."
[^unwindsafe-logic]: Rust std, panic::UnwindSafe — <https://doc.rust-lang.org/1.95.0/std/panic/trait.UnwindSafe.html> — "It is possible, however, for logical invariants to be broken in Rust, which can end up causing behavioral bugs."
[^catch-bound]: Rust std, panic::catch_unwind — <https://doc.rust-lang.org/1.95.0/std/panic/fn.catch_unwind.html> — "The closure provided is required to adhere to the UnwindSafe trait to ensure that all captured variables are safe to cross this boundary. The purpose of this bound is to encode the concept of exception safety in the type system."
[^unwindsafe-speedbump]: Rust std, panic::UnwindSafe — <https://doc.rust-lang.org/1.95.0/std/panic/trait.UnwindSafe.html> — "Note, however, that this is not an unsafe trait, so there is not a succinct contract that this trait is providing. Instead it is intended as more of a “speed bump” to alert users of catch_unwind that broken invariants may be witnessed and may need to be accounted for."
[^unwindsafe-mut]: Rust std, panic::UnwindSafe — <https://doc.rust-lang.org/1.95.0/std/panic/trait.UnwindSafe.html> — "Types such as &mut T and &RefCell\<T> are examples which are not unwind safe. The general idea is that any mutable state which can be shared across catch_unwind is not unwind safe by default. This is because it is very easy to witness a broken invariant outside of catch_unwind as the data is simply accessed as usual."
[^assertunwindsafe]: Rust std, panic::UnwindSafe — <https://doc.rust-lang.org/1.95.0/std/panic/trait.UnwindSafe.html> — "The AssertUnwindSafe wrapper struct can be used to force this trait to be implemented for any closed over variables passed to catch_unwind."
[^mutex-poison]: Rust std, sync::Mutex — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html> — "For a mutex, this means that the lock and try_lock methods return a Result which indicates whether a mutex has been poisoned or not. Most usage of a mutex will simply unwrap() these results, propagating panics among threads to ensure that a possibly invalid invariant is not witnessed."
[^mutex-poison-def]: Rust std, sync::Mutex — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html> — "The mutexes in this module implement a strategy called “poisoning” where a mutex becomes poisoned if it recognizes that the thread holding it has panicked."
[^mutex-advisory]: Rust std, sync::Mutex — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html> — "Poisoning is only advisory: the PoisonError type has an into_inner method which will return the guard that would have otherwise been returned on a successful lock."
[^nomicon-poison-why]: The Rustonomicon, Poisoning — <https://doc.rust-lang.org/1.95.0/nomicon/poisoning.html> — "Mutex poisons not for true safety in the sense that Rust normally cares about. It poisons as a safety-guard against blindly using the data that comes out of a Mutex that has witnessed a panic while locked."
[^nomicon-excsafe-levels]: The Rustonomicon, Exception Safety — <https://doc.rust-lang.org/1.95.0/nomicon/exception-safety.html> — "In unsafe code, we must be exception safe to the point of not violating memory safety. We’ll call this minimal exception safety. In safe code, it is good to be exception safe to the point of your program doing the right thing. We’ll call this maximal exception safety."
[^nomicon-pushall]: The Rustonomicon, Exception Safety — <https://doc.rust-lang.org/1.95.0/nomicon/exception-safety.html> — "set_len, add, and write are all fine; clone is the panic bomb we over-looked. Clone is completely out of our control, and is totally free to panic. If it does, our function will exit early with the length of the Vec set too large. If the Vec is looked at or dropped, uninitialized memory will be read!"
[^resume]: Rust std, panic::resume_unwind — <https://doc.rust-lang.org/1.95.0/std/panic/fn.resume_unwind.html> — "Triggers a panic without invoking the panic hook. This is designed to be used in conjunction with catch_unwind to, for example, carry a panic across a layer of C code."
[^resume-abort]: Rust std, panic::resume_unwind — <https://doc.rust-lang.org/1.95.0/std/panic/fn.resume_unwind.html> — "If this function is called when panics are implemented this way then this function will abort the process, not trigger an unwind."
[^hook-global]: Rust std, panic::set_hook — <https://doc.rust-lang.org/1.95.0/std/panic/fn.set_hook.html> — "The panic hook is a global resource."
[^hook-take]: Rust std, panic::set_hook — <https://doc.rust-lang.org/1.95.0/std/panic/fn.set_hook.html> — "The current hook can be retrieved while reinstating the default hook with the take_hook function."
[^hook-panics]: Rust std, panic::set_hook — <https://doc.rust-lang.org/1.95.0/std/panic/fn.set_hook.html> — "Panics if called from a panicking thread."
[^catch-drop]: Rust std, panic::catch_unwind — <https://doc.rust-lang.org/1.95.0/std/panic/fn.catch_unwind.html> — "Finally, be careful in how you drop the result of this function. If it is Err, it contains the panic payload, and dropping that may in turn panic!"
[^ref-ext-abi-intro]: The Rust Reference, External blocks — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "The ABI specifies the calling convention of the functions in the block. The calling convention defines a low-level interface for functions, such as how arguments are placed in registers or on the stack, how return values are passed, and who is responsible for cleaning up the stack."
[^ref-abi-rust]: The Rust Reference, External blocks — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "The native calling convention for Rust functions and closures. This is the default when a function is declared without using extern fn. The Rust ABI offers no stability guarantees."
[^ref-abi-c]: The Rust Reference, External blocks — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "The “C” ABI matches the default ABI chosen by the dominant C compiler for the target."
[^ref-ext-system]: The Rust Reference, External blocks — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "This is equivalent to extern "C" except on Windows x86_32 where it is equivalent to "stdcall" for non-variadic functions, and equivalent to "C" for variadic functions."
[^ref-ext-system-win]: The Rust Reference, External blocks — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "As the correct underlying ABI on Windows is target-specific, it’s best to use extern "system" when attempting to link Windows API functions that don’t use an explicitly defined ABI."
[^ref-ext-platform-unwind]: The Rust Reference, External blocks — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "Like "C" and "system", most platform-specific ABI strings also have a corresponding -unwind variant; specifically, these are:"
[^ref-ext-sysv64]: The Rust Reference, External blocks — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "“sysv64” is the same as the “C” ABI on non-Windows x86_64 targets."
[^ref-ext-unsafe-required]: The Rust Reference, External blocks — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "The unsafe keyword is semantically required to appear before the extern keyword on external blocks."
[^ref-ext-unsafe]: The Rust Reference, External blocks — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "A function declared in an extern block is implicitly unsafe unless the safe function qualifier is present."
[^ref-ext-foreign-abi]: The Rust Reference, External blocks — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "The Rust compiler automatically translates between the Rust ABI and the foreign ABI."
[^nomicon-ffi-unsafe]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "C libraries often expose interfaces that aren’t thread-safe, and almost any function that takes a pointer argument isn’t valid for all possible inputs since the pointer could be dangling, and raw pointers fall outside of Rust’s safe memory model."
[^nomicon-ffi-decl]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "When declaring the argument types to a foreign function, the Rust compiler cannot check if the declaration is correct, so specifying it correctly is part of keeping the binding correct at runtime."
[^ref-fn-foreign-call]: The Rust Reference, Functions — <https://doc.rust-lang.org/1.95.0/reference/items/functions.html> — "Functions can be called by foreign code, and using an ABI that differs from Rust allows, for example, to provide functions that can be called from other programming languages like C:"
[^ref-no-mangle]: The Rust Reference, Application Binary Interface — <https://doc.rust-lang.org/1.95.0/reference/abi.html> — "The no_mangle attribute may be used on any item to disable standard symbol name mangling. The symbol for the item will be the identifier of the item’s name."
[^ref-no-mangle-unsafe]: The Rust Reference, Application Binary Interface — <https://doc.rust-lang.org/1.95.0/reference/abi.html> — "This attribute is unsafe as an unmangled symbol may collide with another symbol with the same name (or with a well-known symbol), leading to undefined behavior."
[^ref-link-attr]: The Rust Reference, External blocks — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "The link attribute specifies the name of a native library that the compiler should link with for the items within an extern block."
[^nomicon-link]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "The link attribute on extern blocks provides the basic building block for instructing rustc how it will link to native libraries."
[^ref-ext-default]: The Rust Reference, External blocks — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "If the ABI string is not specified, it defaults to "C"."
[^ref-ext-default-phase]: The Rust Reference, External blocks — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "The extern syntax without an explicit ABI is being phased out, so it’s better to always write the ABI explicitly."
[^ref-ext-variadic]: The Rust Reference, External blocks — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "Functions within external blocks may be variadic by specifying ... as the last argument."
[^nomicon-no-variadic]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "Normal Rust functions can not be variadic:"
[^ref-ext-variadic-ub]: The Rust Reference, External blocks — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "The safe qualifier should not be used on a function in an extern block unless that function guarantees that it will not access the variadic arguments at all. Passing an unexpected number of arguments or arguments of unexpected type to a variadic function may lead to undefined behavior."
[^ref-ext-static]: The Rust Reference, External blocks — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "Unless a static item declared in an extern block is qualified as safe, it is unsafe to access that item, whether or not it’s mutable, because there is nothing guaranteeing that the bit pattern at the static’s memory is valid for the type it is declared with, since some arbitrary (e.g. C) code is in charge of initializing the static."
[^ref-no-mangle-ed]: The Rust Reference, Application Binary Interface — <https://doc.rust-lang.org/1.95.0/reference/abi.html> — "Before the 2024 edition it is allowed to use the no_mangle attribute without the unsafe qualification."
[^nomicon-ffi-reprc]: The Rustonomicon, Alternative representations — <https://doc.rust-lang.org/1.95.0/nomicon/other-reprs.html> — "Any type you expect to pass through an FFI boundary should have repr(C), as C is the lingua-franca of the programming world."
[^nomicon-ffi-nullable]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "So Option\<extern "C" fn(c_int) -> c_int> is a correct way to represent a nullable function pointer using the C ABI (corresponding to the C type int (*)(int))."
[^nomicon-ffi-nonnull]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "Certain Rust types are defined to never be null. This includes references (&T, &mut T), boxes (Box\<T>), and function pointers (extern "abi" fn())."
[^nomicon-ffi-nonnull-ub]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "However, trying to construct/work with these invalid values is undefined behavior, so you should use the following workaround instead."
[^nomicon-ffi-nullable-guar]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "This is called an “optimization”, but unlike other optimizations it is guaranteed to apply to eligible types."
[^nomicon-ffi-opaque]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "Sometimes, a C library wants to provide a pointer to something, but not let you know the internal details of the thing it wants."
[^nomicon-ffi-destructors]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "Foreign libraries often hand off ownership of resources to the calling code. When this occurs, we must use Rust’s destructors to provide safety and guarantee the release of these resources (especially in the case of a panic)."
[^nomicon-ffi-wrap]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "A library can choose to expose only the safe, high-level interface and hide the unsafe internal details."
[^ref-ub-ffi]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "Undefined behavior affects the entire program. For example, calling a function in C that exhibits undefined behavior of C means your entire program contains undefined behaviour that can also affect the Rust code. And vice versa, undefined behavior in Rust can cause adverse affects on code executed by any FFI calls to other languages."
[^ref-ub-runtime]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "The runtime assumes that a Rust stack frame is not deallocated without executing destructors for local variables owned by the stack frame. This assumption can be violated by C functions like longjmp."
[^ref-ub-call]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "Calling a function with the wrong call ABI, or unwinding past a stack frame that does not allow unwinding (e.g. by calling a "C-unwind" function imported or transmuted as a "C" function or function pointer)."
[^nomicon-ffi-callback]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "It is possible to pass functions defined in Rust to an external library. The requirement for this is that the callback function is marked as extern with the correct calling convention to make it callable from C code. The callback function can then be sent through a registration call to the C library and afterwards be invoked from there."
[^nomicon-ffi-cpp]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "Rust is currently unable to call directly into a C++ library, but snappy includes a C interface"
[^nomicon-ffi-boundary]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "If an unwinding operation does encounter an ABI boundary that is not permitted to unwind, the behavior depends on the source of the unwinding (Rust panic or a foreign exception): panic will cause the process to safely abort. A foreign exception entering Rust will cause undefined behavior."
[^nomicon-ffi-unwind]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "If you expect Rust panics or foreign (e.g. C++) exceptions to cross an FFI boundary, that boundary must use the appropriate -unwind ABI string."
[^ref-fn-unwind-intro]: The Rust Reference, Functions — <https://doc.rust-lang.org/1.95.0/reference/items/functions.html> — "Most ABI strings come in two variants, one with an -unwind suffix and one without. The Rust ABI always permits unwinding, so there is no Rust-unwind ABI. The choice of ABI, together with the runtime panic handler, determines the behavior when unwinding out of a function."
[^ref-abi-unwind]: The Rust Reference, External blocks — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "Identical to "C" and "system", respectively, but with different behavior when the callee unwinds (by panicking or throwing a C++ style exception)."
[^ref-fn-table-intro]: The Rust Reference, Functions — <https://doc.rust-lang.org/1.95.0/reference/items/functions.html> — "The table below indicates the behavior of an unwinding operation reaching each type of ABI boundary (function declaration or definition using the corresponding ABI string)."
[^ref-fn-categories]: The Rust Reference, Functions — <https://doc.rust-lang.org/1.95.0/reference/items/functions.html> — "The “unwinding” ABI category refers to "Rust" (the implicit ABI of Rust functions not marked extern), "C-unwind", and any other ABI with -unwind in its name. The “non-unwinding” ABI category refers to all other ABI strings, including "C" and "stdcall"."
[^ref-panic-ffi-undef1]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "Causing an unwind into Rust code from a foreign function that was called via a function declaration or pointer declared with a non-unwinding ABI, such as "C", "system", etc."
[^ref-panic-ffi-undef2]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "Calling a Rust extern function that unwinds (with extern "C-unwind" or another ABI that permits unwinding) from code that does not support unwinding, such as code compiled with GCC or Clang using -fno-exceptions"
[^ref-fn-abort]: The Rust Reference, Functions — <https://doc.rust-lang.org/1.95.0/reference/items/functions.html> — "With panic=unwind, when a panic is turned into an abort by a non-unwinding ABI boundary, either no destructors (Drop calls) will run, or all destructors up until the ABI boundary will run. It is unspecified which of those two behaviors will happen."
[^nomicon-panic-abort-note]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "Compiling with panic=abort will still cause panic! to immediately abort the process, regardless of which ABI is specified by the function that panics."
[^nomicon-ffi-abort]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "If assert_nonzero is called with the argument 0, the runtime is guaranteed to (safely) abort the process, whether or not compiled with panic=abort."
[^catch-extern-c]: Rust std, panic::catch_unwind — <https://doc.rust-lang.org/1.95.0/std/panic/fn.catch_unwind.html> — "Rust functions that are expected to be called from foreign code that does not support unwinding (such as C compiled with -fno-exceptions) should be defined using extern "C", which ensures that if the Rust code panics, it is automatically caught and the process is aborted."
[^nomicon-ffi-cunwind]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "This function (when compiled with panic=unwind) is permitted to unwind C++ stack frames."
[^nomicon-ffi-throw]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "If may_throw does throw an exception, b will be dropped. Otherwise, 5 will be printed."
[^ref-panic-catch-foreign]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "Catching a foreign unwinding operation (such as a C++ exception) using std::panic::catch_unwind, std::thread::JoinHandle::join, or by letting it propagate beyond the Rust main() function or thread root will have one of two behaviors, and it is unspecified which will occur: The process aborts. The function returns a Result::Err containing an opaque type."
[^catch-foreign]: Rust std, panic::catch_unwind — <https://doc.rust-lang.org/1.95.0/std/panic/fn.catch_unwind.html> — "The process aborts, after executing all destructors of f and the functions it called."
[^nomicon-ffi-undef-catch]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "Note that the interaction of catch_unwind with foreign exceptions is undefined, as is the interaction of panic with foreign exception-catching mechanisms (notably C++’s try/catch)."
[^ref-panic-other-rt]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "Rust code compiled or linked with a different instance of the Rust standard library counts as a “foreign exception” for the purpose of this guarantee. Thus, a library that uses panic! and is linked against one version of the Rust standard library, invoked from an application that uses a different version of the standard library, may cause the entire application to abort even if the library is only used within a child thread."
[^ref-panic-dispose]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "There are currently no guarantees about the behavior that occurs when a foreign runtime attempts to dispose of, or rethrow, a Rust panic payload. In other words, an unwind originated from a Rust runtime must either lead to termination of the process or be caught by the same runtime."
[^ref-link-lint]: The Rust Reference, Linkage — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "To guarantee that a library will be sound (and linkable with rustc) regardless of the panic runtime used at link-time, the ffi_unwind_calls lint may be used. The lint flags any calls to -unwind foreign functions or function pointers."
[^nomicon-ffi-catch]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "If you are writing Rust code that may panic, and you don’t wish to abort the process if it panics, you must use catch_unwind"
[^nomicon-catch-only]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "Please note that catch_unwind will only catch unwinding panics, not those that abort the process."
[^ref-link-unwind-prohibited]: The Rust Reference, Linkage — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "If a Rust artifact is potentially unwinding, then all its crates must be built with the unwind panic strategy. Otherwise, unwinding can cause undefined behavior."
[^ref-link-potential]: The Rust Reference, Linkage — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "A Rust artifact is called potentially unwinding if any of the following conditions is met: The artifact uses the unwind panic handler. The artifact contains a crate built with the unwind panic strategy that makes a call to a function using a -unwind ABI. The artifact makes a "Rust" ABI call to code running in another Rust artifact that has a separate copy of the Rust runtime, and that other artifact is potentially unwinding."
[^ref-link-note]: The Rust Reference, Linkage — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "If you are using rustc to link, these rules are enforced automatically. If you are not using rustc to link, you must take care to ensure that unwinding is handled consistently across the entire binary. Linking without rustc includes using dlopen or similar facilities where linking is done by the system runtime without rustc being involved."
[^nomicon-ffi-callrust]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "The extern "C" makes this function adhere to the C calling convention, as discussed below in “Foreign Calling Conventions”. The no_mangle attribute turns off Rust’s name mangling, so that it has a well defined symbol to link to."
[^nomicon-ffi-cdylib]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "Then, to compile Rust code as a shared library that can be called from C, add the following to your Cargo.toml: [lib] crate-type = ["cdylib"]"
[^nomicon-ffi-catch-ex]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "let result = catch_unwind(|| { panic!("Oops!"); }); match result { Ok(_) => 0, Err(_) => 1, }"
[^ref-panic-cleanup]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "Thus, when normal execution resumes, no-longer-accessible objects will have been “cleaned up” just as if they had gone out of scope normally."
[^nomicon-unwind-erlang]: The Rustonomicon, Unwinding — <https://doc.rust-lang.org/1.95.0/nomicon/unwinding.html> — "In the long-long-ago, Rust was much more like Erlang. Like Erlang, Rust had lightweight tasks, and tasks were intended to kill themselves with a panic when they reached an untenable state."
[^abort-clean]: Rust std, process::abort — <https://doc.rust-lang.org/1.95.0/std/process/fn.abort.html> — "If a clean shutdown is needed, it is recommended to only call this function at a known point where there are no more destructors left to run."
[^ref-panic-handler-attr]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "The panic_handler attribute can be applied to a function to define the behavior of panics."
[^hook-replace]: Rust std, panic::set_hook — <https://doc.rust-lang.org/1.95.0/std/panic/fn.set_hook.html> — "Registers a custom panic hook, replacing the previously registered hook."
[^ref-ext-edition2024]: The Rust Reference, External blocks — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "Prior to the 2024 edition, the unsafe keyword is optional. The safe and unsafe item qualifiers are only allowed if the external block itself is marked as unsafe."
[^nomicon-ffi-opaque-struct]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "This is a perfectly valid way of handling the situation. However, we can do a bit better. To solve this, some C libraries will instead create a struct, where the details and memory layout of the struct are private. This gives some amount of type safety. These structures are called ‘opaque’."
[^nomicon-ffi-opaque-rust]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "To do this in Rust, let’s create our own opaque types: #[repr(C)] pub struct Foo { _data: (), _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>, }"
[^ref-fn-panic-col]: The Rust Reference, Functions — <https://doc.rust-lang.org/1.95.0/reference/items/functions.html> — "The panic-unwind column refers to panicking via the panic! macro and similar standard library mechanisms, as well as to any other Rust operations that cause a panic, such as out-of-bounds array indexing or integer overflow."
[^ref-fn-native]: The Rust Reference, Functions — <https://doc.rust-lang.org/1.95.0/reference/items/functions.html> — "Native unwinding is defined per-target. On targets that support throwing and catching C++ exceptions, it refers to the mechanism used to implement this feature. Some platforms implement a form of unwinding referred to as “forced unwinding”; longjmp on Windows and pthread_exit in glibc are implemented this way. Forced unwinding is explicitly excluded from the “Native unwind” column in the table."
[^ref-fn-row1]: The Rust Reference, Functions (bảng Unwinding, tiêu đề cột và dòng 1) — <https://doc.rust-lang.org/1.95.0/reference/items/functions.html> — "panic runtime ABI panic-unwind Native unwind (unforced) panic=unwind unwinding unwind unwind"
[^ref-fn-row2]: The Rust Reference, Functions (bảng Unwinding, dòng 2) — <https://doc.rust-lang.org/1.95.0/reference/items/functions.html> — "panic=unwind non-unwinding abort (see notes below) undefined behavior"
[^ref-fn-row3]: The Rust Reference, Functions (bảng Unwinding, dòng 3) — <https://doc.rust-lang.org/1.95.0/reference/items/functions.html> — "panic=abort unwinding panic aborts without unwinding abort"
[^ref-fn-row4]: The Rust Reference, Functions (bảng Unwinding, dòng 4) — <https://doc.rust-lang.org/1.95.0/reference/items/functions.html> — "panic=abort non-unwinding panic aborts without unwinding undefined behavior"
[^nomicon-poison-mem]: The Rustonomicon, Poisoning — <https://doc.rust-lang.org/1.95.0/nomicon/poisoning.html> — "It is important to note that one cannot violate memory safety with such a type if it is correctly written. After all, it must be minimally exception-safe!"
[^ref-panic-macro]: The Rust Reference, Panic — <https://doc.rust-lang.org/1.95.0/reference/panic.html> — "The standard library provides the capability to explicitly panic via the panic! macro."
[^nomicon-ffi-nullable-def]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "As a special case, an enum is eligible for the “nullable pointer optimization” if it contains exactly two variants, one of which contains no data and the other contains a field of one of the non-nullable types listed above."
[^nomicon-ffi-nullable-nodisc]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "This means no extra space is required for a discriminant; rather, the empty variant is represented by putting a null value into the non-nullable field."
[^ref-export-name-unsafe]: The Rust Reference, Application Binary Interface — <https://doc.rust-lang.org/1.95.0/reference/abi.html> — "This attribute is unsafe as a symbol with a custom name may collide with another symbol with the same name (or with a well-known symbol), leading to undefined behavior."
[^ref-export-name-ed]: The Rust Reference, Application Binary Interface — <https://doc.rust-lang.org/1.95.0/reference/abi.html> — "Before the 2024 edition it is allowed to use the export_name attribute without the unsafe qualification."
[^nomicon-ffi-cpp-dtor]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "If the C++ frames have objects, their destructors will be called."
[^nomicon-unwinding-ffi]: The Rustonomicon, Unwinding — <https://doc.rust-lang.org/1.95.0/nomicon/unwinding.html> — "Rust’s unwinding strategy is not specified to be fundamentally compatible with any other language’s unwinding. As such, unwinding into Rust from another language, or unwinding into another language from Rust is Undefined Behavior. You must absolutely catch any panics at the FFI boundary!"
[^cargo-no-override]: The Cargo Book, Profiles — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "Overrides cannot specify the panic, lto, or rpath settings."
[^nomicon-ffi-cvoid]: The Rustonomicon, FFI — <https://doc.rust-lang.org/1.95.0/nomicon/ffi.html> — "We can represent this in Rust with the c_void type:"
