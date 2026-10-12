# P11 · Chương 03 — Đo và cải thiện hiệu năng service Rust

> Chương này nói về **phương pháp** và **cấu hình**, không đưa ra con số hiệu năng nào của người soạn: người soạn không chạy benchmark (theo hợp đồng của dự án). Những con số như "10–20%" là lời của tài liệu được trích, đúng như tài liệu đó nói, và không hứa gì cho dự án của bạn. Một số nguồn là **trang sống**, không ghim phiên bản nên nội dung có thể đổi: *The Rust Performance Book* (`nnethercote.github.io/perf-book`), sách Criterion.rs và *Site Reliability Engineering* của Google. Các nguồn còn lại ghim phiên bản: Rust 1.95.0, `tracing` 0.1.44, `tokio` 1.53.2, `hdrhistogram` 7.5.4, `console-subscriber` 0.5.0, README `tokio-console-v0.1.13`.

## Bạn sẽ hiểu được gì

1. **Q1.** Vì sao một benchmark viết vội thường đo sai, và warm-up, nhiễu, `std::hint::black_box` giúp được gì (và không giúp được gì)? → mục 1
2. **Q2.** Vì sao phải nhìn phân phối/percentile thay vì trung bình, và ghi lại nó bằng cách nào? → mục 2
3. **Q3.** `opt-level`, `codegen-units`, LTO, `panic` đổi gì ở tốc độ chạy và thời gian build? → mục 3
4. **Q4.** PGO (profile-guided optimization) là gì, quy trình bốn bước ra sao, bẫy ở đâu? → mục 4
5. **Q5.** Tìm chỗ nóng bằng profiler nào, và bản build cần những cờ gì để profile đọc được? → mục 5
6. **Q6.** Cấp phát heap tốn gì, đếm allocation bằng `GlobalAlloc` thế nào, khi nào đáng đổi allocator? → mục 6
7. **Q7.** Crate `tracing` có span, event, subscriber là gì; vì sao `Span::enter` qua `.await` làm sai trace; `tokio-console` thêm gì? → mục 7
8. **Q8.** Latency đuôi (tail) của một service Rust/Tokio đến từ đâu và khoanh vùng ra sao? → mục 8

## Kiến thức cần trước

- [P11 · Chương 01 — perf và sampling profiler](01-perf-va-sampling-profiler.md): `perf stat/record`, flame graph, symbol và debuginfo. Chương này dùng lại các công cụ đó; ở đây chỉ nói phần riêng của Rust.
- [P3 · Chương 06 — Heap allocator](../03-memory/06-heap-allocator.md): free list, arena, `brk`/`mmap`, jemalloc/mimalloc và `#[global_allocator]` ở mức khái niệm.
- [P7 · Chương 08 — Tokio](../07-io-async/08-tokio.md): runtime, scheduler, vì sao không được chặn trong async, `spawn_blocking`.

Cả chương bám theo một service giả định tên `catalog`, giới thiệu ở đầu mục 1.

## 1. Benchmark đúng cách: warm-up, nhiễu, `black_box` (Q1)

> **Trả lời ngắn.** Benchmark viết vội thường sai vì hai lý do. Lý do thứ nhất: trình biên dịch tối ưu mất đoạn code bạn định đo[^bb-elim]. Lý do thứ hai: máy chưa "nóng", nên cache còn lạnh[^crit-warm]. Máy cũng phải yên tĩnh thì số đo mới đáng tin[^crit-quiet]. `black_box` là cách bảo trình biên dịch đừng giả định gì về một giá trị[^bb-def]; warm-up và máy yên tĩnh lo phần còn lại. Wall-time vốn dễ dao động mạnh[^pb-var]. Vì vậy đôi khi cần đo thêm một đại lượng ít nhiễu hơn như số lệnh[^pb-instr].

**Giải thích.**

*Với service `catalog`.* `catalog` là một service Rust chạy trên Tokio, có endpoint `GET /items/{id}`. Mỗi request nó parse đường dẫn, hỏi Postgres rồi serialize dòng dữ liệu thành JSON. Ta sẽ bám theo `catalog` suốt chương (nhận xét của người soạn: đây là ví dụ giả định, không phải code trong repo). Giả sử p99 của endpoint này đang cao và bạn nghi hàm `to_json` serialize chậm. Việc đầu tiên là viết benchmark cho hàm đó, và đây cũng là chỗ dễ đo sai nhất: bạn bấm giờ một vòng lặp gọi `to_json` rồi nhận về 0 ns. Con số ấy trông rất đẹp và hoàn toàn vô nghĩa. Mục này giải thích vì sao, và cần làm gì để số đo đáng tin.

### 1.1 Lỗi 1: đoạn code bị tối ưu mất

Bản release của Rust được tối ưu rất mạnh, và với benchmark thì điều đó gây hại. Docs của `black_box` minh họa bằng một hàm thuần `contains` được gọi trong vòng lặp mà không ai dùng kết quả. Về lý thuyết, trình biên dịch được phép làm bốn việc: kéo lời gọi ra khỏi vòng lặp, inline nó, thay bằng hằng `true` vì đầu vào đã biết lúc biên dịch, và xóa hẳn lời gọi vì kết quả không được dùng[^bb-nothing]. Docs không chắc cả bốn việc xảy ra cùng lúc. Nhưng chỉ cần trình biên dịch làm một vài trong số đó là benchmark đã "rất thiếu chính xác"[^bb-elim].

`to_json` của `catalog` rơi đúng vào tình huống này: nó là hàm thuần, và vòng lặp benchmark bỏ kết quả đi (nhận xét của người soạn). Trình biên dịch có đủ lý do để bỏ luôn việc serialize, và 0 ns là thời gian của một phép đo không còn đo gì.

### 1.2 Cách chữa: `std::hint::black_box`

`black_box` là hàm đồng nhất, tức trả đúng giá trị nó nhận. Điểm đặc biệt là trình biên dịch được dặn phải "bi quan tối đa" về việc hàm này có thể làm gì với giá trị đó[^bb-def]. Trong thực hành nó phục vụ hai việc: ngăn trình biên dịch tối ưu dựa trên giá trị `black_box` trả về, và buộc giá trị truyền vào phải được tính ra dù kết quả không dùng[^bb-purposes].

Từ đó có hai quy tắc dùng. Thứ nhất, bọc *đầu vào* của hàm cần đo để trình biên dịch không thể tối ưu theo cách phi thực tế, một cách mà production không bao giờ được hưởng[^bb-inputs]. Thứ hai, bọc cả *kết quả* để phép tính không bị xóa. Ví dụ trong docs bọc cả lời gọi lẫn hai đối số, nhờ vậy thân `benchmark` không thể bỏ lời gọi[^bb-example]. Với `catalog`, nghĩa là viết `black_box(to_json(black_box(&item)))`. Đoạn code dưới đây dùng hàm `contains` của docs để minh họa cùng khuôn đó:

```rust
use std::hint::black_box;

fn contains(haystack: &[&str], needle: &str) -> bool {
    haystack.iter().any(|x| x == &needle)
}

// Minh họa theo ví dụ trong docs của black_box; chưa chạy trong chương này.
pub fn benchmark() {
    let haystack = vec!["abc", "def", "ghi", "jkl", "mno"];
    let needle = "ghi";
    for _ in 0..10 {
        black_box(contains(black_box(&haystack), black_box(needle)));
    }
}
```

`black_box` chỉ được cung cấp theo kiểu "best-effort": mức nó chặn được tối ưu hóa tùy nền tảng và backend sinh code[^bb-bestef]. Chương trình cũng không được dựa vào nó cho tính đúng đắn, ngoài việc nó là hàm đồng nhất[^bb-correct]. Với benchmark thế là đủ dùng, nhưng đừng coi nó là bảo đảm tuyệt đối. Một quy tắc thực hành: số đo tốt đến mức khó tin thì nên nghi code đã bị tối ưu mất trước khi mừng (nhận xét của người soạn).

### 1.3 Lỗi 2: máy chưa nóng

Rust biên dịch trước nên không có pha JIT như Java. Dù vậy lần chạy đầu vẫn chậm hơn: cache CPU còn lạnh, một số page chưa được map, trạng thái allocator chưa ổn định (nhận xét của người soạn). Criterion xử lý bằng cách chạy hàm cần đo lặp lại trong một khoảng warm-up cấu hình được, mặc định ba giây. Với hàm Rust, mục đích là làm nóng cache của bộ xử lý và cache của file system nếu có[^crit-warm]. Cách làm khá đơn giản: chạy hàm một lần, rồi hai lần, bốn lần... cho đến khi tổng thời gian chạy vượt ngưỡng warm-up[^crit-warm-alg].

### 1.4 Lỗi 3: nhiễu

Criterion khuyên hai điều: chạy benchmark trên máy yên tĩnh, và thiết kế sao cho mỗi vòng lặp làm khối lượng việc xấp xỉ nhau[^crit-quiet]. Laptop đang mở IDE và trình duyệt chưa phải là máy yên tĩnh (nhận xét của người soạn). Nhưng máy yên tĩnh vẫn chưa hết nhiễu. Wall-time là thời gian thực trôi qua trên đồng hồ, thứ người dùng cảm nhận được, và nó dễ có phương sai cao[^pb-var]. Chỉ một thay đổi rất nhỏ về bố cục bộ nhớ cũng đủ gây dao động đáng kể, dù thoáng qua[^pb-layout]. Vì vậy các số đo ít nhiễu hơn, như số chu kỳ hoặc số lệnh, có thể là phương án thay thế hợp lý[^pb-instr]. Khi so hai lần chạy với nhau, Criterion còn có một ngưỡng nhiễu cấu hình được, vì những dao động không lường trước hiếm khi là thứ người đo quan tâm[^crit-noise].

Ghép các bước lại, một benchmark `to_json` đáng tin có dạng:

```mermaid
flowchart LR
  W["Workload đại diện<br/>(đầu vào thật nếu có)"] --> H["Harness: warm-up<br/>→ lấy mẫu nhiều lần"]
  H --> BB["black_box ở đầu vào<br/>và kết quả"]
  BB --> S["Thống kê: phân phối,<br/>outlier, so với lần trước"]
  S --> D{"Khác biệt vượt<br/>ngưỡng nhiễu?"}
  D -- "có" --> R["Kết luận về thay đổi"]
  D -- "không" --> N["Coi như chưa có bằng chứng"]
```

*Sơ đồ trên là tóm tắt của người soạn từ các đoạn vừa nêu (warm-up, `black_box`, ngưỡng nhiễu); không phải sơ đồ của tài liệu nào.*

<details><summary>Đi sâu: Criterion xử lý outlier thế nào, công cụ nào có sẵn, microbenchmark thôi chưa đủ</summary>

- Criterion không đo từng lần lặp mà chỉ đo cả mẫu (sample)[^crit-sample]. Thời gian mỗi lần lặp được ước lượng từ độ dốc của đường thẳng hồi quy vẽ số lần lặp theo thời gian của các mẫu[^crit-regress]. Vì thế nó không cho bạn phân phối latency *của từng request*; việc đó thuộc mục 2 (suy luận của người soạn từ việc Criterion chỉ đo cả mẫu).
- Criterion phân loại outlier (mẫu lệch xa phần còn lại) bằng một biến thể của phương pháp Tukey, dựa trên khoảng tứ phân vị IQR[^crit-tukey]. Nó in cảnh báo nhưng *không loại* outlier khỏi dữ liệu: chúng vẫn đi vào phân tích[^crit-outlier].
- Bộ benchmark có sẵn của Rust chỉ chạy trên nightly vì dùng tính năng chưa ổn định; Criterion và Divan tinh vi hơn[^pb-tools]. Performance Book cũng khuyên có nhiều workload đại diện cho cách dùng thật: input thật là tốt nhất, còn microbenchmark và stress test chỉ nên dùng có chừng mực[^pb-workload].
- `cargo bench` dùng profile `bench`, kế thừa profile `release`[^cargo-bench-inherits]. Nghĩa là benchmark chạy có tối ưu hóa; chạy bản dev (không tối ưu) thì chẳng có ý nghĩa gì, vì sách nói tăng tốc 10–100 lần so với bản dev là chuyện phổ biến[^pb-release].
- Thái độ thực dụng của sách: đừng quá lo phải có hệ thống benchmark hoàn hảo, vì benchmark tầm thường vẫn tốt hơn không có[^pb-mediocre].

</details>

**Quay lại Q1:** với `catalog`, benchmark `to_json` sai vì code bị tối ưu mất và vì máy lạnh, ồn; `black_box` bọc đầu vào và kết quả, warm-up cùng máy yên tĩnh là điều kiện tối thiểu, và `black_box` chỉ là "best-effort".

## 2. Phân phối và percentile thay vì trung bình (Q2)

> **Trả lời ngắn.** Trung bình che mất đuôi: một service có latency trung bình 100 ms ở 1.000 request/giây vẫn có thể có 1% request mất tới 5 giây[^sre-100ms]. Cách rẻ nhất để thấy đuôi là đếm số request rơi vào từng khoảng latency (histogram) thay vì lưu từng giá trị[^sre-bucket]. Trong Rust, crate `hdrhistogram` làm đúng việc này, với chi phí bộ nhớ cố định[^hdr-fixed]. Bạn truy vấn nó được theo quantile[^hdr-quantile].

**Giải thích.**

*Với service `catalog`.* Sau mục 1 bạn có benchmark đáng tin cho `to_json`, nhưng nó chỉ đo một hàm trong vòng lặp, chưa đo request thật. Muốn biết người dùng gặp gì, bạn phải đo latency của `GET /items/{id}` ngay lúc service đang chạy. Giả sử dashboard báo trung bình 40 ms (số giả định). Con số đó có ổn không? Chưa biết được, vì trung bình không cho biết bao nhiêu request chậm bất thường, và chính những request ấy là thứ người dùng phàn nàn. Mục này nói vì sao trung bình đánh lừa, và `catalog` nên ghi latency bằng gì.

Trước hết là mấy từ sẽ gặp (giải nghĩa của người soạn): *percentile thứ 99* (p99) là mức latency mà 99% số request nhanh hơn hoặc bằng nó; p50 là trung vị, một nửa số request nhanh hơn; p99,9 là mốc 99,9%.

### 2.1 Vì sao trung bình đánh lừa

Khi dựng monitoring từ đầu, rất dễ chọn trung bình của một đại lượng nào đó: latency trung bình, CPU trung bình của các node, hay độ đầy trung bình của database[^sre-mean]. Với latency, hậu quả cụ thể là thế này: một web service trung bình 100 ms ở 1.000 request/giây vẫn có thể có 1% request mất tới 5 giây[^sre-100ms]. Cùng chỗ đó có một phép tính đáng nhớ: nếu 1% request chậm gấp 50 lần trung bình thì 99% còn lại nhanh hơn trung bình chỉ khoảng hai lần[^sre-fn23]. Nói cách khác, khi không đo phân phối thì câu "đa số request gần mức trung bình" chỉ là hy vọng (ý của chú thích đó, người soạn diễn đạt lại).

### 2.2 Đuôi bị khuếch đại khi gọi nhiều backend

Chuyện này nặng hơn khi `catalog` chỉ là một mắt xích. Giả sử trang sản phẩm phải gọi `catalog` cùng vài service khác rồi mới dựng xong. Khi người dùng phụ thuộc vào nhiều service như vậy, percentile thứ 99 của một backend có thể dễ dàng trở thành phản hồi trung vị của frontend[^sre-fan]. Vì thế p99 của service nội bộ quan trọng hơn con số trung bình, nhất là khi service của bạn nằm giữa một chuỗi gọi (nhận xét của người soạn).

### 2.3 Ghi lại phân phối thế nào

Cách đơn giản nhất mà sách SRE gợi ý: đếm số request theo từng khoảng latency (0–10 ms, 10–30 ms, 30–100 ms, …) thay vì lưu từng latency[^sre-bucket]. Chia ranh giới các khoảng theo cấp số nhân, với hệ số khoảng 3, là một cách dễ để nhìn ra phân phối[^sre-expo]. Ngoài ra, percentile thứ 99 tính trên cửa sổ ngắn, ví dụ một phút, còn là tín hiệu sớm của việc bão hòa[^sre-p99-sat].

### 2.4 Trong Rust: `hdrhistogram`

Crate `hdrhistogram` mô tả "HDR histogram" là cấu trúc cho phép phân tích nhanh và chính xác các vùng cực trị của dữ liệu không có phân phối chuẩn, như latency[^hdr-desc]. Nó giữ chi phí cố định cả về không gian lẫn thời gian[^hdr-fixed]. Bạn hỏi được giá trị tại một quantile bất kỳ[^hdr-quantile]. Với `catalog`, bạn ghi latency của mỗi request vào một histogram theo endpoint, rồi đọc p50, p99 và p99,9 từ đó:

```rust
use hdrhistogram::Histogram;

// Minh họa theo ví dụ trong docs của hdrhistogram 7.5.4; chưa chạy trong chương này.
fn main() {
    // giá trị trong [1, 3.600.000], giữ 2 chữ số có nghĩa (ví dụ: micro/mili-giây tùy bạn quy ước)
    let mut hist = Histogram::<u64>::new_with_bounds(1, 60 * 60 * 1000, 2).unwrap();
    for sample in [12_u64, 15, 14, 13, 4_900] {
        hist.record(sample).unwrap();
    }
    println!("p50  = {}", hist.value_at_quantile(0.50));
    println!("p99  = {}", hist.value_at_quantile(0.99));
    println!("p999 = {}", hist.value_at_quantile(0.999));
}
```

Đối số cuối của `new_with_bounds` là độ chính xác tính bằng số chữ số có nghĩa (significant digits)[^hdr-precision]. Docs mô tả đúng ví dụ này là đếm giá trị trong khoảng [1..3600000] với độ chính xác 1%, dùng để theo dõi latency từ 1 ms đến 1 giờ[^hdr-bounds].

<details><summary>Đi sâu: quantile hay percentile, monitoring ≠ profiling, và các cách ghi ở lớp tracing</summary>

- `hdrhistogram` ưu tiên làm việc với *quantile* (số trong [0, 1]) hơn *percentile* (số trong [0, 100]) để giảm mất chính xác do số dấu phẩy động[^hdr-quant-pct]. Vì vậy p99,9 là `value_at_quantile(0.999)`.
- Kiểu đếm của histogram (ví dụ `u64`) có thể đổi để tiết kiệm bộ nhớ, đổi lại nguy cơ bão hòa nếu quá nhiều mẫu rơi vào cùng một ô[^hdr-u64].
- `tracing-timing` là crate bên thứ ba mà docs của `tracing` liệt kê. Nó cung cấp một subscriber đo thời gian giữa các cặp event và tạo histogram[^tr-timing]; nghĩa là có thể gắn đo phân phối vào instrumentation sẵn có (mục 7).
- Sách SRE nhấn mạnh: quan sát CPU trung bình theo phút sẽ không lộ các đỉnh ngắn nhưng gây latency đuôi cao[^sre-cpu]. Cửa sổ lấy mẫu của metric phải ngắn hơn đuôi bạn muốn bắt (nhận xét của người soạn).

</details>

**Quay lại Q2:** với `catalog`, hãy báo cáo p50/p99/p99,9 từ histogram, vì trung bình có thể đẹp trong khi 1% request chậm gấp hàng chục lần, và đuôi đó gây hại nhất khi nhiều service gọi nối tiếp nhau.

## 3. Cấu hình build: opt-level, codegen-units, LTO, panic (Q3)

> **Trả lời ngắn.** Profile `release` mặc định đã tối ưu (`opt-level = 3`) và đặt `lto = false`, `codegen-units = 16`, `panic = 'unwind'`[^cargo-release-defaults]. Đặt `codegen-units = 1` có thể cho code nhanh hơn nhưng build chậm hơn[^rustc-cgu]. LTO (tối ưu lúc link) tối ưu xuyên crate, đổi lại thời gian link dài hơn[^rustc-lto]. `panic = "abort"` có thể giảm kích thước và tăng tốc nhẹ[^pb-panic]. Đổi lại bạn mất unwind, tức việc tháo ngược stack khi panic[^book-unwind]. Mọi thay đổi đều phải đo bằng workload của bạn.

**Giải thích.**

*Với service `catalog`.* `catalog` được build bằng `cargo build --release` trong CI rồi đóng vào container image. Profile release mặc định đã tối ưu khá mạnh, nhưng vẫn còn vài núm vặn cho phép đổi thời gian build lấy tốc độ chạy. Mục này nói mỗi núm đổi gì, và núm nào đổi cả hành vi của `catalog` lúc panic. Nhớ rằng mọi thay đổi ở đây chỉ có giá trị nếu bạn đo lại bằng cách của mục 1 và 2.

### 3.1 `opt-level`

Mức tối ưu cao hơn có thể cho code chạy nhanh hơn, với giá thời gian biên dịch dài hơn; nó cũng có thể sắp xếp lại code khiến debug khó hơn[^cargo-opt]. Các mức là 0–3, `"s"` và `"z"`. Kết quả có thể bất ngờ, chẳng hạn mức 3 chậm hơn mức 2, nên cần thử và đánh giá lại mỗi khi rustc đổi[^cargo-opt-surprise]. Còn cờ `-O` của rustc chỉ là bí danh của `-C opt-level=3`[^rustc-O].

### 3.2 `codegen-units`

Khi crate bị chia thành nhiều codegen unit, LLVM xử lý chúng song song. Song song nhiều hơn có thể rút thời gian biên dịch nhưng có thể sinh code chậm hơn; đặt bằng 1 thì code sinh ra có thể nhanh hơn, đổi lại biên dịch chậm hơn[^rustc-cgu]. Mặc định là 16 cho build không incremental và 256 cho incremental[^rustc-cgu-default]. Profile `release` dùng 16 (build không incremental)[^cargo-release-defaults].

### 3.3 LTO (link-time optimization)

Cờ này cho LLVM tối ưu lúc link, dựa trên phân tích toàn chương trình, để code tốt hơn với giá thời gian link lâu hơn[^rustc-lto]. Trong Cargo, khóa `lto` có bốn giá trị đáng phân biệt:

- `"fat"` (hoặc `true`): tối ưu xuyên mọi crate trong đồ thị phụ thuộc[^cargo-lto-fat].
- `"thin"`: tương tự `fat` nhưng tốn ít thời gian hơn đáng kể mà vẫn đạt lợi ích gần bằng[^cargo-lto-thin].
- `false`: "thin local LTO", chỉ tối ưu trong crate hiện tại, xuyên các codegen unit của nó[^cargo-lto-false].
- `"off"`: tắt hẳn LTO[^cargo-lto-false].

Có một cái bẫy: nếu bạn không chỉ định `lto`, rustc vẫn có thể làm thin local LTO, nhưng sẽ tắt khi `codegen-units` bằng 1 hoặc `opt-level = 0`[^rustc-lto-cgu1]. Vì thế `lto = false` *khác* `lto = "off"`[^pb-lto-off].

### 3.4 `panic`: đổi tốc độ lấy hành vi

Khi panic, `unwind` đi ngược lên stack và dọn dữ liệu của từng hàm đi qua[^book-unwind]. `abort` thì kết thúc chương trình ngay, không dọn dẹp[^rustc-panic]. Nếu không cần unwind (ví dụ không dùng `catch_unwind`), `panic = "abort"` có thể giảm kích thước binary, tăng tốc nhẹ và rút ngắn thời gian biên dịch một chút[^pb-panic].

Nhưng với một server Tokio, **đánh đổi này thực chất là đổi hành vi**. Khi unwind, panic trong một task được `JoinHandle` báo ra qua `JoinError`[^tokio-jh-panic]. Với `abort`, cả tiến trình chết thay vì chỉ một task (suy luận của người soạn từ hai nguồn trên: nguồn chỉ nói `abort` kết thúc tiến trình khi panic, không nói gì riêng về Tokio). Cụ thể, nếu một handler của `catalog` panic vì `unwrap()` trên giá trị không ngờ tới, thì dưới `unwind` chỉ request đó hỏng; dưới `abort`, `catalog` chết cùng mọi request đang xử lý (suy luận của người soạn). Chọn `abort` cho service nghĩa là chấp nhận crash cả process, nên cần một supervisor khởi động lại, chẳng hạn Kubernetes (nhận xét của người soạn).

### 3.5 Tổ hợp tham khảo và `target-cpu`

Performance Book nêu: để tối đa tốc độ chạy, hãy cân nhắc `codegen-units = 1`, `lto = "fat"`, allocator thay thế và `panic = "abort"`[^pb-max]. Một cờ khác, `-C target-cpu=native`, bảo rustc dùng bộ xử lý của máy host[^rustc-cpu-native] để sinh lệnh riêng cho CPU đó. Cách này chỉ hợp lý nếu bạn không cần tương thích với CPU cũ hoặc khác[^pb-cpu].

Với `catalog` có một bẫy cụ thể (suy luận của người soạn): nếu image được build trên CI rồi chạy trên node khác loại CPU, `native` có thể sinh lệnh mà CPU đích không có. Khi đó tiến trình có thể nhận SIGILL, tức "Illegal Instruction"[^sig-ill]. Hãy chỉ định `target-cpu` cụ thể và kiểm tra trước khi triển khai.

### 3.6 Một profile riêng để so sánh

Thay vì sửa thẳng `release`, bạn có thể tạo một profile riêng để đo song song với bản mặc định:

```toml
# Cargo.toml — minh họa một profile riêng để so sánh với release thường (chưa chạy)
[profile.release-tuned]
inherits = "release"
codegen-units = 1
lto = "fat"
# panic = "abort"   # chỉ khi chấp nhận crash cả process (xem mục này)
```

Cargo bắt buộc profile tùy biến phải có khóa `inherits`[^cargo-custom]. Cargo cũng chỉ đọc cấu hình profile ở `Cargo.toml` gốc của workspace; profile khai báo trong dependency bị bỏ qua[^cargo-ws-profiles].

<details><summary>Đi sâu: bảng mặc định, các ngoại lệ, và build-override</summary>

- Mặc định `release`: `opt-level = 3`, `debug = false`, `strip = "none"`, `debug-assertions = false`, `overflow-checks = false`, `lto = false`, `panic = 'unwind'`, `incremental = false`, `codegen-units = 16`[^cargo-release-defaults]. Mặc định `dev`: `opt-level = 0`, `debug-assertions = true`, `overflow-checks = true`, `incremental = true`, `codegen-units = 256`[^cargo-dev-defaults].
- Tổ hợp `lto` và `codegen-units` của rustc khi chỉ định `-C lto`: một mình `lto` là 16 codegen unit rồi fat LTO xuyên crate; `codegen-units=1` cộng `lto` là 1 unit với fat LTO[^rustc-lto-matrix].
- Performance Book viết LTO "có thể cải thiện tốc độ chạy 10–20% hoặc hơn" và giảm kích thước nhị phân, với giá thời gian biên dịch tệ hơn[^pb-lto]. Còn đặt `codegen-units = 1` có thể cải thiện tốc độ và kích thước với giá thời gian biên dịch[^pb-cgu]. Đó là lời của sách, không phải bảo đảm: Cargo nhấn mạnh nên tự thử, vì có thể có kết quả bất ngờ[^cargo-opt-surprise].
- Build dependency (build script, proc macro) mặc định không được tối ưu (`opt-level = 0`, `codegen-units = 256`) để build nhanh[^cargo-build-override]. Muốn đổi thì khai báo `[profile.release.build-override]`, vì Cargo cho phép ghi đè cấu hình profile cho từng package và cho crate chạy lúc build[^cargo-overrides].
- Với `cargo test`, `cargo bench`, build script và proc macro, Cargo bỏ qua `panic`; test harness hiện đòi hành vi unwind[^cargo-panic-ignored].

</details>

**Quay lại Q3:** `release` mặc định cân bằng giữa thời gian build và tốc độ chạy; `codegen-units = 1`, LTO, `panic = "abort"` và `target-cpu` đẩy `catalog` về phía tốc độ chạy, với giá là thời gian build hoặc hành vi khi panic, và chỉ nên chọn sau khi đo.

## 4. PGO: tối ưu theo hồ sơ chạy thật (Q4)

> **Trả lời ngắn.** PGO thu dữ liệu về cách chương trình chạy điển hình (nhánh nào hay đi…), rồi dùng nó để quyết định inlining, bố trí code máy, cấp phát thanh ghi…[^pgo-concept]. Quy trình có bốn bước: build bản có instrument, chạy nó để sinh file `.profraw`, gộp thành `.profdata` bằng `llvm-profdata`, rồi build lại với `-Cprofile-use`[^pgo-steps]. Chất lượng kết quả phụ thuộc vào workload bạn chạy ở bước hai.

**Giải thích.**

*Với service `catalog`.* Giả sử `GET /items/{id}` chiếm gần hết lưu lượng của `catalog`. Nếu compiler biết điều đó, nó có thể đặt code của đường đi này gần nhau và inline đúng chỗ, thay vì đoán mò như khi chỉ nhìn source. PGO là cách cho compiler biết điều ấy: bạn chạy `catalog` với lưu lượng thật, ghi lại hàm nào nóng, nhánh nào hầu như luôn đi, rồi build lại dựa trên bản ghi đó. Mục này nói quy trình, và câu hỏi khó nhất của nó: lưu lượng nào là "đại diện" cho `catalog`.

Ba thứ PGO giúp quyết định, giải nghĩa của người soạn: *inlining* là chép thân hàm được gọi vào ngay chỗ gọi; *machine-code layout* là thứ tự đặt các khối lệnh máy trong binary; *register allocation* là chọn giá trị nào được giữ trong thanh ghi CPU. Cả ba đều nằm trong danh sách mà rustc book nêu[^pgo-concept].

### 4.1 Hai cách thu dữ liệu

Có thể chạy chương trình dưới một profiler như `perf`, hoặc tạo bản instrumented, tức bản có sẵn mã thu dữ liệu. Cách sau thường cho dữ liệu chính xác hơn và là cách rustc hỗ trợ[^pgo-instr].

### 4.2 Bốn bước

```mermaid
flowchart LR
  A["1. Build có instrument<br/>-Cprofile-generate=DIR"] --> B["2. Chạy bản đó với<br/>workload điển hình<br/>→ *.profraw"]
  B --> C["3. llvm-profdata merge<br/>→ merged.profdata"]
  C --> D["4. Build lại<br/>-Cprofile-use=merged.profdata"]
```

Cụ thể: (1) biên dịch với instrumentation, ví dụ `rustc -Cprofile-generate=/tmp/pgo-data main.rs`; (2) chạy chương trình đã instrument để sinh `.profraw`; (3) chuyển `.profraw` thành `.profdata` bằng công cụ `llvm-profdata` của LLVM; (4) biên dịch lại và dùng dữ liệu đó, ví dụ `rustc -Cprofile-use=merged.profdata main.rs`[^pgo-steps]. Chạy bản instrument nhiều lần sẽ dùng lại và cập nhật tại chỗ các file `.profraw` tương ứng[^pgo-multi]. Công cụ `llvm-profdata` lấy dễ nhất qua `rustup component add llvm-tools-preview`, nhưng thành phần này không đưa nó vào `PATH`[^pgo-llvmtools].

### 4.3 Với Cargo

Tài liệu rustc dùng biến `RUSTFLAGS` để truyền cờ PGO cho mọi crate, kèm vài chú ý:

- Truyền `--target` để cờ không áp lên build script, vì ta không muốn build script sinh hàng đống `.profraw`[^pgo-cargo-target].
- Dùng `--release`, vì đó là nơi PGO có ý nghĩa nhất[^pgo-release].
- Dùng đường dẫn tuyệt đối cho `-Cprofile-generate` và `-Cprofile-use`, vì Cargo có thể gọi rustc từ các thư mục làm việc khác nhau[^pgo-abs].
- Xóa dữ liệu cũ trước khi build bản instrument[^pgo-leftover].
- Bước cuối: ví dụ của sách ghi chú "mọi cờ rustc phải giống nhau"; trong ví dụ đó bước 1 và bước 4 đều có `-O`, chỉ khác cờ PGO[^pgo-same-flags].

### 4.4 Với một service: workload nào là "điển hình"?

Bước 2 là chỗ khó nhất. Tài liệu chỉ nói PGO dựa trên "quá trình chạy điển hình"[^pgo-concept], nên với `catalog`, lưu lượng bạn chạy ở bước này quyết định compiler tối ưu cho cái gì. Nên chạy bản instrument với lưu lượng đại diện, chẳng hạn replay lưu lượng thật hoặc load test có tỉ lệ endpoint giống production. Profile lệch với thực tế có thể khiến compiler tối ưu sai chỗ. Đây là suy luận của người soạn, không phải lời của nguồn.

<details><summary>Đi sâu: cảnh báo thiếu profile, cargo-pgo, giới hạn phân phối</summary>

- Nên thêm `-Cllvm-args=-pgo-warn-missing-function` ở pha `-Cprofile-use`: mặc định LLVM không cảnh báo khi thiếu dữ liệu profile cho một hàm, bật cờ này giúp phát hiện cấu hình sai[^pgo-warn].
- Hỗ trợ PGO của rustc dựa hoàn toàn trên LLVM, tương đương `-fprofile-generate`/`-fprofile-use` của Clang[^pgo-llvm].
- Công cụ cộng đồng `cargo-pgo` có API dòng lệnh dễ dùng, làm thay phần việc thủ công[^pgo-cargo-pgo].
- Performance Book: PGO có thể tăng tốc 10% hoặc hơn[^pb-pgo] (lời của sách, không phải bảo đảm). Đây là kỹ thuật nâng cao, tốn công cài đặt, đáng làm trong một số trường hợp[^pb-pgo-effort]. Nó cũng không hỗ trợ binary phát hành trên crates.io qua `cargo install`[^pb-pgo-cargo-install].
- Chương này không dạy `cargo-pgo` hay BOLT chi tiết, vì ngoài các đoạn trên chúng không nằm trong phạm vi nguồn đã ghim.

</details>

**Quay lại Q4:** PGO là build hai lần quanh một lần chạy thu dữ liệu; cú pháp rõ ràng trong rustc book, còn chất lượng phụ thuộc vào việc lưu lượng bạn replay cho `catalog` có đại diện cho production hay không.

## 5. Tìm chỗ nóng: profiler và cờ build để đọc được kết quả (Q5)

> **Trả lời ngắn.** Đo trước, tối ưu sau: chương trình có "chỗ nóng" (thực thi đủ thường xuyên để ảnh hưởng thời gian chạy) và bạn nên tìm chúng bằng profiler[^pp-hot]. Với Rust, bản build release có thể thiếu thông tin để profiler đọc. Bạn có thể cần `debug = "line-tables-only"` để có số dòng[^pp-debug]. Có thể cần thêm `-C force-frame-pointers=yes` để stack trace tốt hơn[^pp-fp]. Symbol Rust cũng bị mangle nên có thể phải demangle[^pp-mangle].

**Giải thích.**

*Với service `catalog`.* Bạn đã có p99 xấu (mục 2) nhưng chưa biết thời gian nằm ở đâu: parse, serialize, hay chờ DB. Nếu đoán rồi viết lại một đoạn bằng `unsafe` cho "nhanh", rất có thể bạn sửa nhầm chỗ. Profiler cho biết chỗ nóng thật. Nhưng bạn chạy `perf record` trên `catalog` bản release và nhận về flame graph toàn địa chỉ hex hoặc tên bắt đầu bằng `_ZN`, không đọc được. Mục này nói chọn công cụ nào, và build `catalog` ra sao để kết quả đọc được.

### 5.1 Chọn công cụ

Performance Book liệt kê các profiler đã được dùng thành công với Rust:

- `perf` dùng bộ đếm phần cứng và chạy trên Linux[^pp-perf].
- Instruments đi kèm Xcode trên macOS[^pp-instruments].
- `samply` là profiler lấy mẫu, xem kết quả trong Firefox Profiler, chạy trên Mac, Linux và Windows[^pp-samply].
- `flamegraph` là lệnh Cargo dùng perf hoặc DTrace để vẽ flame graph[^pp-flame].
- Cachegrind và Callgrind cho số lệnh và dữ liệu mô phỏng cache, dự đoán nhánh theo hàm và theo dòng[^pp-cachegrind].

Cách dùng `perf` và đọc flame graph xem [P11 · Chương 01](01-perf-va-sampling-profiler.md).

### 5.2 Build để profile

Để profile một bản release cho hiệu quả, bạn có thể cần bật debug info mức dòng: thêm `debug = "line-tables-only"` vào `[profile.release]`[^pp-debug]. Cargo mô tả mức này là tạo lượng thông tin tối thiểu cho backtrace, gồm tên file và số dòng, không có thông tin biến hay tham số[^cargo-line-tables]. Lưu ý rằng dù vậy bạn vẫn không có thông tin chi tiết cho code thư viện chuẩn, vì bản std phát hành không được build kèm debug info[^pp-std].

### 5.3 Frame pointer

Frame pointer là thanh ghi trỏ vào khung stack của hàm đang chạy. Chuỗi các khung này là thứ profiler đi ngược để dựng call stack (đã giải thích ở [P11 · Chương 01 mục 5](01-perf-va-sampling-profiler.md)). Trình biên dịch có thể bỏ frame pointer, làm giảm chất lượng stack trace trong profile[^pp-fp]. Bạn ép nó giữ lại bằng `RUSTFLAGS="-C force-frame-pointers=yes"`.

### 5.4 Symbol bị mangle và đừng strip

Rust mã hóa tên hàm (name mangling), nên profiler không biết cách đọc sẽ hiện tên bắt đầu bằng `_ZN` hoặc `_R`. Có thể dùng `rustfilt`, hoặc đổi định dạng sang `v0` bằng `-C symbol-mangling-version=v0`[^pp-mangle].

Còn `strip = "symbols"` có thể làm chương trình khó debug và khó profile hơn[^pb-strip]. Vì vậy ở bản dùng để profile, nên giữ symbol (nhận xét của người soạn, suy ra từ câu trên).

<details><summary>Đi sâu: các profiler khác, và hai mức debuginfo</summary>

- DHAT dùng để tìm phần code gây nhiều allocation và hiểu đỉnh bộ nhớ. `dhat-rs` là bản thay thế thử nghiệm, kém mạnh hơn một chút, cần sửa chương trình nhẹ nhưng chạy trên mọi nền tảng[^pp-dhat]. `heaptrack` và `bytehound` là profiler heap trên Linux[^pp-heaptrack]. Phần allocation xem mục 6.
- Coz làm *causal profiling* để ước lượng tiềm năng tối ưu, có hỗ trợ Rust qua `coz-rs`, chạy trên Linux[^pp-coz].
- Cargo liệt kê các mức `debug`: `"line-tables-only"`, `1`/`"limited"` (thông tin mức module chi tiết hơn line-tables-only) và `2`/`"full"`, mặc định của dev là full[^cargo-debug-levels]. Mặc định của release là `debug = false`, tức không có debug info[^cargo-release-defaults].
- Cargo cũng nói tối ưu cao có thể sắp xếp lại code làm khó dùng với debugger[^cargo-opt]. Với profiler lấy mẫu, stack có thể "không giống source"; đó là lý do inline làm flame graph khó đọc (nhận xét của người soạn).

</details>

**Quay lại Q5:** profile trước khi tối ưu; build release của `catalog` cần `line-tables-only`, tốt nhất thêm frame pointer, giữ symbol và demangle, nếu không flame graph sẽ toàn địa chỉ và tên `_ZN…`.

## 6. Heap allocation: đếm, giảm, đổi allocator (Q6)

> **Trả lời ngắn.** Mỗi lần cấp phát (và giải phóng) heap thường khá tốn: tùy allocator, nó có thể phải lấy khóa toàn cục, thao tác cấu trúc dữ liệu không đơn giản, và có khi gọi system call[^pb-heap-cost]. Nếu profiler thấy `malloc`/`free` nóng thì nên giảm tốc độ cấp phát hoặc thử allocator khác[^pb-heap-prof]. Công cụ chuyên dụng cho việc này là DHAT[^pb-dhat]. Trong Rust bạn cũng có thể tự đếm bằng cách đặt một `GlobalAlloc` bọc ngoài `System`, nhưng không được kỳ vọng mọi allocation trong source thật sự xảy ra[^ga-notrely].

**Giải thích.**

*Với service `catalog`.* Giả sử flame graph của `catalog` (mục 5) cho thấy `malloc` và `free` chiếm một phần đáng kể. Thường thủ phạm là một đoạn như `Vec::new()` hoặc `String::new()` trong vòng lặp mà `to_json` chạy cho mỗi dòng dữ liệu của mỗi request, nên mỗi request cấp phát lại hàng chục lần cùng một thứ. Mục này nói allocation tốn gì, đếm chúng ra sao, giảm bằng cách nào, và khi nào mới đáng đổi allocator.

### 6.1 Chi phí

Performance Book nói cấp phát heap "vừa phải đắt", chi tiết tùy allocator, và cấp phát nhỏ không nhất thiết rẻ hơn cấp phát lớn[^pb-heap-small]. Cơ chế allocator (header, free list, arena, thread cache, `brk`/`mmap`) đã giải thích ở [P3 · Chương 06](../03-memory/06-heap-allocator.md); chương này chỉ bàn cách đo và đổi.

### 6.2 Tìm ra allocation nóng

Nếu profiler tổng quát thấy `malloc`, `free` và các hàm liên quan nóng, thì đáng giảm allocation rate hoặc dùng allocator khác[^pb-heap-prof]. DHAT xác định chính xác điểm cấp phát nóng và tốc độ cấp phát ở từng điểm[^pb-dhat]. Về mức đáng kể: kinh nghiệm với rustc cho thấy giảm 10 allocation trên mỗi triệu lệnh có thể cho cải thiện đo được, khoảng 1%[^pb-dhat-rustc]. Đó là kinh nghiệm của dự án rustc, không phải quy luật cho `catalog`.

### 6.3 Giảm allocation

Khi biết kích thước tối thiểu hoặc chính xác của vector, hãy đặt trước bằng `Vec::with_capacity`, `Vec::reserve` hoặc `Vec::reserve_exact`[^pb-withcap]. Trong `catalog`, nếu `to_json` biết trước số cột của một dòng, một lần `with_capacity` thay được hàng chục lần mở rộng vector (nhận xét của người soạn). `SmallVec` giảm số allocation khi dùng đúng chỗ nhưng không bảo đảm nhanh hơn, nên cần đo để xác nhận[^pb-smallvec]. Bạn cũng có thể viết test kiểm tra một snippet cấp phát đúng lượng heap mong đợi bằng tính năng heap usage testing của `dhat-rs`[^pb-dhat-test].

### 6.4 Tự đếm bằng `GlobalAlloc`

`GlobalAlloc` là trait cho allocator có thể đăng ký làm mặc định của thư viện chuẩn qua thuộc tính `#[global_allocator]`[^ga-desc]. Ý tưởng: bọc `System`, và tăng một bộ đếm nguyên tử trong `alloc`:

```rust
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

pub static ALLOCS: AtomicUsize = AtomicUsize::new(0);
pub static BYTES: AtomicUsize = AtomicUsize::new(0);

struct Counting;

// Minh họa; chưa chạy trong chương này. Không dùng Mutex/Vec/println! trong alloc.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(layout.size(), Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;
```

Đọc `ALLOCS` trước và sau một đoạn code, bạn có số allocation giữa hai thời điểm. Lưu ý (nhận xét của người soạn): nếu tiến trình có thread khác đang chạy, số đếm là *toàn tiến trình*, không riêng đoạn code của bạn; hãy dùng test đơn luồng hoặc `dhat-rs` để cô lập.

Docs của `GlobalAlloc` nêu ba cạm bẫy:

- **Allocator không được unwind (panic).** Đó là hành vi không xác định[^ga-unwind].
- **Không được dựa vào việc allocation thật sự xảy ra.** Optimizer có thể bỏ nó hoặc chuyển xuống stack, kể cả khi trong source có heap allocation tường minh[^ga-notrely]. "Có allocation hay không" không thuộc hành vi chương trình, dù một allocator có thể phát hiện qua việc in ra hay có side effect khác[^ga-notbehavior]. Vì thế số đếm của bạn là *quan sát trên một bản build cụ thể*, không phải thuộc tính của ngôn ngữ. Cách dùng (suy luận của người soạn): so sánh trước/sau trên cùng cấu hình build, và đừng assert cứng số đếm trong test chạy qua nhiều phiên bản.
- **Tái nhập.** Nhiều cấu trúc của thư viện chuẩn có thể tự cấp phát, nên rất dễ vô tình tạo ra allocator đệ quy vô hạn[^ga-reent]. Ví dụ `std::sync::Mutex` trên một số nền tảng có thể cấp phát, nên dùng nó trong global allocator rất rắc rối[^ga-mutex]. Vì vậy nên bám vào các tính năng của `core`[^ga-core].

### 6.5 Đổi allocator

Có thể thay allocator hệ thống của chương trình Rust bằng allocator khác. Hiệu quả tùy chương trình và allocator được chọn; thực tế đã thấy cải thiện lớn về tốc độ và bộ nhớ[^pb-altalloc]. Đổi lại, binary có thể lớn hơn và thời gian biên dịch lâu hơn[^pb-altalloc-cost]. Sách nêu hai lựa chọn. Một là jemalloc, cho Linux và Mac, qua crate `tikv-jemallocator`[^pb-jemalloc]. Hai là mimalloc, chạy trên nhiều nền tảng, qua crate `mimalloc`[^pb-mimalloc]. Việc gắn allocator chỉ là một dòng `#[global_allocator] static GLOBAL: ... = ...;` như đã nêu ở [P3 · Chương 06 mục 10](../03-memory/06-heap-allocator.md). Với `catalog`, hãy chỉ đổi sau khi DHAT hoặc bộ đếm cho thấy allocation thật sự là vấn đề, rồi đo lại bằng mục 1 và 2 (nhận xét của người soạn).

<details><summary>Đi sâu: yêu cầu hợp đồng của <code>realloc</code> và hạn chế của số đếm</summary>

- `realloc` thu nhỏ hoặc mở rộng khối đến `new_size`. Nếu nó trả con trỏ khác null, quyền sở hữu khối chuyển sang allocator và con trỏ cũ không được truy cập nữa dù khối giữ nguyên chỗ. Vùng `0..min(layout.size(), new_size)` của khối mới được bảo đảm giống khối cũ[^ga-realloc]. Nếu bạn viết allocator đếm và override `realloc`, nhớ giữ hợp đồng này.
- Ví dụ `Counting` ở trên không override `realloc`, nên một `Vec` mở rộng đi qua bản mặc định của trait. Trong source 1.95.0, bản mặc định gọi `self.alloc` cho khối mới, rồi sao chép và gọi `self.dealloc` cho khối cũ[^ga-realloc-default]. Nghĩa là lần mở rộng đó được đếm như một lần `alloc` (chi tiết cài đặt, có thể đổi; docs không hứa điều này). Vì không dùng `System.realloc`, allocator đếm có thể làm mất khả năng mở rộng tại chỗ của allocator thật, nên thời gian đo qua nó có thể khác bản không bọc (suy luận của người soạn).
- Performance Book cũng nhắc: allocator hệ thống của mỗi nền tảng có điểm mạnh riêng, nên hiệu quả khác nhau theo nền tảng[^pb-altalloc].
- Trên Linux, jemalloc còn có thể cấu hình dùng transparent huge pages để tăng tốc, có thể đổi lấy nhiều bộ nhớ hơn[^pb-jemalloc-thp].

</details>

**Quay lại Q6:** allocation là chi phí đo được; thấy `malloc`/`free` nóng trong `catalog` thì dùng DHAT hoặc bộ đếm `GlobalAlloc` để biết ở đâu, giảm bằng `with_capacity`/tái dùng buffer, và chỉ đổi allocator sau khi đo; docs không hứa mọi allocation trong source thực sự xảy ra.

## 7. Quan sát runtime: `tracing` và `tokio-console` (Q7)

> **Trả lời ngắn.** `tracing` là framework để gắn instrumentation có cấu trúc vào chương trình Rust[^tr-what]. Ba khái niệm lõi: *span* là một khoảng thời gian có đầu có cuối[^tr-span]. *Event* là một thời điểm[^tr-event]. *Subscriber* là thứ ghi hoặc tổng hợp chúng[^tr-sub]. Trong async, đừng giữ guard của `Span::enter` qua `.await`, vì docs nói làm vậy cho trace sai[^tr-enter-warn]; hãy dùng `#[instrument]`, `Instrument` hoặc `in_scope`. `tokio-console` là công cụ chẩn đoán task Tokio, cần `tokio_unstable`[^con-unstable].

**Giải thích.**

*Với service `catalog`.* Giả sử một request `GET /items/42` chậm bất thường. Log của `catalog` có dòng "xử lý xong request", nhưng nó chỉ cho bạn một khoảnh khắc: không biết thời gian nằm ở parse, ở DB hay ở serialize. Tệ hơn, Tokio chạy xen kẽ nhiều request trên cùng một thread, nên các dòng log của những request khác chen vào giữa. Mục này nói `tracing` giải quyết chuyện đó thế nào, cái bẫy lớn nhất của nó trong async, và `tokio-console` cho thấy thêm gì.

### 7.1 Vì sao log thường không đủ trong async

Trong hệ thống async như Tokio, đọc log truyền thống khá khó: nhiều task bị ghép trên cùng một thread nên log của các tác vụ xen lẫn nhau, và khó lần theo luồng logic[^tr-async]. `tracing` mở rộng log bằng cách ghi sự kiện có cấu trúc, kèm thông tin về thời gian và quan hệ nhân quả. Khác với một dòng log, một span có thời điểm bắt đầu và kết thúc, có thể được vào và ra theo luồng thực thi, và lồng nhau thành cây[^tr-span-nest]. Với `catalog`, một span `handle_request` cho bạn thấy request vào lúc nào, ra lúc nào, và các span con bên trong (parse, DB, serialize) chiếm bao lâu (nhận xét của người soạn).

### 7.2 Span, event, subscriber

Span biểu diễn một khoảng thời gian: chương trình "vào" span khi bắt đầu làm việc trong ngữ cảnh đó và "ra" khi dừng[^tr-span]. Event là một điểm thời gian, thường đặt trong một span, ví dụ "request trả về với status code nào"[^tr-event]. Subscriber là thứ nhận thông báo khi event xảy ra và khi span được vào hoặc ra, để ghi hay tổng hợp lại[^tr-sub].

Thư viện `tracing` không có sẵn subscriber; đó là việc của các crate khác[^tr-nosub]. Ví dụ `tracing-subscriber` có `FmtSubscriber` in ra stdout[^tr-ts]. Tài liệu Tokio hướng dẫn đăng ký subscriber sớm nhất có thể trong `main`[^tokio-tracing-early]. Event nằm ngoài ngữ cảnh của mọi subscriber thì sẽ không được thu[^tr-global].

```mermaid
flowchart LR
  subgraph APP["Ứng dụng / thư viện"]
    S["span!, #[instrument]"]
    E["event!, info!, error!"]
  end
  S --> SUB["Subscriber<br/>(vd. FmtSubscriber,<br/>Layer ghép)"]
  E --> SUB
  SUB --> O1["stdout / file"]
  SUB --> O2["histogram thời gian"]
  SUB --> O3["OpenTelemetry..."]
```

*Sơ đồ là của người soạn. Docs của `tracing` liệt kê `tracing-timing` (tạo histogram) là một crate bên thứ ba[^tr-timing]. Crate `tracing-opentelemetry` xuất trace sang các hệ OpenTelemetry[^tr-otel].*

### 7.3 Chi phí của instrumentation

Subscriber có thể lọc theo metadata. Theo docs, nếu không subscriber nào đang hoạt động quan tâm đến metadata của một span hay event thì span, event đó không bao giờ được dựng[^tr-enabled]. Do đó instrumentation ở mức TRACE trên đường nóng có thể rẻ khi bị lọc, nhưng bạn vẫn nên đo chi phí khi bật (suy luận của người soạn). Attribute `#[instrument]` ghi tham số của hàm thành field bằng `fmt::Debug`[^tr-instrument]. Muốn tránh ghi những thứ lớn hoặc không cần, hãy dùng `skip(...)`[^tr-skip]. Với `catalog`, nếu `to_json` nhận cả một dòng dữ liệu lớn làm tham số thì đó là thứ nên `skip`: tham số lớn bị format trên đường nóng là một nguồn chậm kín đáo (nhận xét của người soạn).

### 7.4 Bẫy async: `Span::enter`

Docs của `Span::enter` cảnh báo: trong code async, nên dùng rất cẩn thận hoặc tránh hẳn; giữ guard trả về qua điểm `.await` sẽ cho trace sai[^tr-enter-warn]. Lý do: khi async fn dừng ở `.await`, scope hiện tại bị thoát nhưng các giá trị trong scope *không bị drop*. Kết quả là một task khác bắt đầu chạy trong khi vẫn "ở trong" span đó[^tr-enter-why]. Với `catalog`, nghĩa là thời gian và field của request khác có thể bị gán nhầm cho request này.

Có ba cách thay thế:

- `Span::in_scope` cho đoạn đồng bộ: closure thoát span trước điểm await kế tiếp[^tr-in-scope].
- `Future::instrument`: gắn span vào future, vào span mỗi lần future được poll và ra khi future yield[^tr-instrument-combinator].
- `#[instrument]`: sinh code đúng khi dùng trên `async fn`[^tr-instrument-async].

```rust
use tracing::{info_span, Instrument};

// Minh họa theo docs của Span/Instrument (tracing 0.1.44); chưa chạy trong chương này.
async fn handle(req_id: u64) {
    let span = info_span!("handle", req_id);
    async move {
        // nếu yield ở đây, span được thoát và vào lại khi tiếp tục
        // db_query().await;
    }
    .instrument(span)
    .await
}
```

### 7.5 `tokio-console`

`tokio-console` là công cụ debug và profile cho ứng dụng async, thu và hiển thị dữ liệu chẩn đoán chi tiết về task, resource và operation[^con-what]. Nó gồm phần instrumentation nhúng trong ứng dụng và phần consumer, như ứng dụng dòng lệnh `tokio-console`. Crate `console-subscriber` cài phần instrumentation dưới dạng một `Layer` của `tracing-subscriber`, kèm một server gRPC xuất telemetry[^con-impl].

Yêu cầu là bật tính năng `tracing` của Tokio và cờ cfg `tokio_unstable`[^con-unstable]. Lý do là support `tracing` trong runtime Tokio hiện được coi là thử nghiệm[^con-experimental]. Bản README ghim cũng dặn target `tokio` và `runtime` phải bật ở mức TRACE; dùng `console_subscriber::init()` thì chúng tự được bật[^con-targets].

Tool còn có một số cảnh báo (lint) về các mẫu task đáng ngờ:

- `lost-waker`: task bị drop mà chưa từng được đánh thức[^con-lost].
- `never-yielded`: task chưa từng yield[^con-never].
- `self-wakes`: task tự đánh thức mình quá nhiều so với tổng số lần đánh thức[^con-self].

README không nói các cảnh báo này liên quan latency; riêng `never-yielded` liên hệ trực tiếp với mục 8, vì task không yield giữ chặt worker (nhận xét của người soạn).

<details><summary>Đi sâu: Layer, tracing-log, và cấu hình của console-subscriber</summary>

- `tracing-subscriber` cung cấp trait `Layer` để ghép nhiều thành phần thành một subscriber có nhiều hành vi[^tokio-tracing-layer]; `console-subscriber` là một `Layer` như vậy[^con-impl].
- Cầu nối với crate `log`: feature `log` làm span/event phát log record khi không có subscriber `tracing` nào hoạt động. `log-always` phát log cả khi đã có subscriber, dành cho ứng dụng thu log và trace riêng[^tr-log].
- `tracing-timing` dựng histogram từ khoảng thời gian giữa các cặp event[^tr-timing], kết nối trực tiếp với mục 2.
- `console-subscriber` có `retention(Duration)` để chỉnh thời gian giữ dữ liệu task đã hoàn thành; theo ví dụ trong docs đặt là 60 giây[^con-retention].
- Console chỉ dùng với Tokio 1.7.0 trở lên, vì hiện chỉ runtime đó cài instrumentation cần thiết[^con-tokio-only].
- Overhead của `tokio-console`/`tracing` trong service production: nguồn đã ghim không nêu số, cần tự đo (nhận xét của người soạn).

</details>

**Quay lại Q7:** span là đoạn thời gian, event là điểm thời gian, subscriber ghi lại chúng; trong `catalog` hãy dùng `#[instrument]`/`Instrument`/`in_scope`, đừng giữ `enter` qua `.await`, và `tokio-console` cho thấy trạng thái task Tokio khi bật `tokio_unstable`.

## 8. Latency đuôi trong service Rust/Tokio (Q8)

> **Trả lời ngắn.** Docs Tokio chỉ ra hai thứ làm *một* worker không chạy được các task khác kịp lúc. Thứ nhất là code chặn thread trong task[^tokio-noblock]. Thứ hai là task chạy lâu mà không yield[^tokio-coop]. Cả hai đều có khả năng kéo đuôi latency của service Tokio (suy luận của người soạn: docs không dùng từ "tail latency"). Cấp phát heap, vốn có thể phải lấy khóa toàn cục hoặc gọi system call[^pb-heap-cost], là ứng viên thứ ba. Runtime chỉ bảo đảm công bằng khi không task nào chặn thread[^tokio-fair]. Nó cũng không bảo đảm công bằng giữa các task với nhau[^tokio-fair-neg]. Cách làm việc: đo phân phối (mục 2), chia thời gian request thành span (mục 7), rồi profile (mục 5).

**Giải thích.**

*Với service `catalog`.* Giả sử `catalog` có thêm endpoint `GET /reports/{id}`, và handler của nó gọi thẳng một hàm đồng bộ `build_report()` chạy vài trăm mili-giây (ví dụ giả định). Hàm này không có `.await` nào, nên worker thread chạy nó không poll được task nào khác trong suốt thời gian đó, kể cả những request `GET /items/{id}` đang chờ trên chính worker ấy. Dashboard khi đó có dạng quen thuộc: p50 đẹp vì phần lớn request rơi vào các worker khác, còn p99 xấu vì nhóm request xui xẻo bị kẹt sau `build_report()` (nhận xét của người soạn). Mục này đi qua bốn nguồn có thể gây ra đuôi như vậy, rồi một quy trình khoanh vùng.

Trước hết, vì sao đuôi đáng lo hơn trung bình: như ở mục 2, 1% request chậm là chuyện bình thường với service trung bình nhanh[^sre-100ms]. Khi nhiều backend nối tiếp, p99 của một backend lại dễ trở thành trung vị phía frontend[^sre-fan].

### 8.1 Nguồn 1: chặn thread

Tokio chạy nhiều task xen kẽ trên cùng một thread. Vì vậy task không nên gọi system call hay thao tác nào có thể chặn thread, vì làm thế sẽ ngăn các task khác cùng thread chạy[^tokio-noblock]. `build_report()` ở trên là một ví dụ: nó chặn thread bằng CPU thay vì bằng system call, nhưng hậu quả với các task cùng thread thì như nhau (nhận xét của người soạn).

Docs liệt kê hai cách thoát: `task::spawn_blocking` và `task::block_in_place`[^tokio-blocking-apis]. Docs còn nhắc rằng gọi hàm không-async từ code async vẫn nằm trong ngữ cảnh async, nên cũng phải tránh chặn ở đó, kể cả trong destructor của object bị hủy trong code async[^tokio-destructors]. Chi tiết từng cơ chế xem [P7 · Chương 08 mục 4](../07-io-async/08-tokio.md).

### 8.2 Nguồn 2: task chạy lâu mà không yield

Đa nhiệm của Tokio là hợp tác: một task được chạy cho đến khi nó yield[^tokio-coop]. Vì thế một vòng lặp CPU dài giữa hai điểm `.await` chiếm trọn worker. `tokio-console` có cảnh báo `never-yielded` cho đúng kiểu task này[^con-never].

### 8.3 Nguồn 3: công bằng chỉ có điều kiện

Docs Tokio chỉ bảo đảm công bằng nếu số task không tăng không giới hạn và không task nào chặn thread[^tokio-fair]. Nói chính xác hơn: nếu mọi lần `poll` đều quay về trong một thời gian hữu hạn thì task được đánh thức sẽ được lập lịch trong một độ trễ tối đa hữu hạn[^tokio-fair-formal]. Một lần `poll` chạy lâu làm tăng giả định về thời gian `poll`, nên cận trễ này cũng tăng theo, vì docs nói cận đó phụ thuộc vào các giả định (suy luận của người soạn).

Ngoài ra không có bảo đảm về thứ tự hay mức công bằng giữa các task: runtime có thể lập lịch A năm lần trước B dù cả hai đều sẵn sàng, chỉ bảo đảm B rồi sẽ được lập lịch[^tokio-fair-neg]. Tokio cũng đưa ra bảo đảm kiểu "tối đa bao nhiêu đơn vị thời gian" cho việc đánh thức task có sự kiện IO hoặc timer, với cùng các giả định đó[^tokio-io-timer].

### 8.4 Nguồn 4: cấp phát

Cấp phát heap có thể phải lấy khóa toàn cục hoặc gọi system call, tùy allocator[^pb-heap-cost]. Hiệu ứng thực tế phụ thuộc allocator và workload (mục 6, và [P3·06](../03-memory/06-heap-allocator.md)).

### 8.5 Quy trình khoanh vùng

Đây là quy trình của người soạn, ghép từ các mục trên:

1. Ghi latency vào histogram theo endpoint; so p50 với p99 và p99,9 (mục 2).
2. Bọc từng pha của request trong span (parse, DB, serialize…) và xem pha nào kéo đuôi (mục 7).
3. Mở `tokio-console` xem task nào có poll time dài hoặc dính `never-yielded` (mục 7).
4. Profile CPU bằng flame graph để tìm đoạn nóng hoặc đoạn chặn (mục 5, [P11·01](01-perf-va-sampling-profiler.md)); heap profile nếu `malloc` nóng (mục 6).
5. Sửa một thứ, benchmark lại theo mục 1, rồi so sánh *phân phối* chứ không chỉ trung bình.

<details><summary>Đi sâu: lập lịch của Tokio ảnh hưởng đuôi thế nào</summary>

- Trong runtime đa luồng (docs ghi rõ hành vi chi tiết này "có thể đổi" ở phiên bản sau[^tokio-detail-warn]): hàng đợi toàn cục được kiểm tra khi hàng đợi local rỗng, hoặc khi worker đã lấy `global_queue_interval` task liên tiếp từ hàng đợi local. Nếu không đặt, runtime tự tính theo heuristic nhắm đến khoảng 10 ms giữa các lần kiểm tra, dựa trên metric `worker_mean_poll_time`[^tokio-gqi]. Nghĩa là poll time trung bình của task ảnh hưởng trực tiếp tần suất task ở hàng đợi chung được xét tới (nhận xét của người soạn).
- Runtime kiểm tra sự kiện IO/timer khi không còn task sẵn sàng hoặc đã lập lịch 61 task liên tiếp; hằng 61 đổi được bằng `event_interval`[^tokio-event-interval].
- Docs của `spawn_blocking` nói nó chạy hàm chặn trên một thread pool riêng cho các tác vụ chặn[^tokio-sb-pool]. Task `spawn_blocking` đã chạy thì không `abort` được[^tokio-sb-abort]. Pool này có giới hạn thread, mặc định 512 theo docs của `max_blocking_threads`[^tokio-blocking-limit]. Khi hết thread rảnh và đã đạt giới hạn, task chờ trong hàng đợi[^tokio-blocking-queue]. Đẩy việc CPU nặng sang pool này giữ worker async rảnh, nhưng tải nặng vẫn có thể xếp hàng ở đó (nhận xét của người soạn).
- `tokio::task::yield_now` cho task nhường runtime một lần để task khác chạy[^tokio-yield].

</details>

**Quay lại Q8:** latency đuôi trong service Tokio thường do một worker bị chiếm (chặn, hoặc vòng lặp dài không yield), hoặc do công bằng và cấp phát không đều; với `catalog`, hãy khoanh vùng bằng histogram, span, `tokio-console` và profile thay vì đoán.

## Tóm tắt

- Benchmark `to_json` phải bọc đầu vào bằng `black_box`[^bb-inputs]. Hãy nhớ `black_box` chỉ là best-effort[^bb-bestef].
- Warm-up làm nóng cache trước khi đo[^crit-warm]. Máy cũng phải yên tĩnh[^crit-quiet].
- Wall-time dễ nhiễu, kể cả chỉ vì bố cục bộ nhớ đổi nhẹ[^pb-layout]. Số lệnh hay chu kỳ là phương án ít nhiễu hơn[^pb-instr].
- Trung bình che đuôi: 100 ms trung bình vẫn có thể có 1% request mất 5 giây[^sre-100ms]. Hãy ghi histogram[^sre-bucket]. Trong Rust, `hdrhistogram` truy vấn được theo quantile[^hdr-quantile].
- `release` mặc định có `opt-level = 3`, `lto = false`, `codegen-units = 16`, `panic = 'unwind'`[^cargo-release-defaults].
- `codegen-units = 1` và LTO đổi thời gian build lấy code có thể nhanh hơn[^rustc-cgu].
- `panic = "abort"` kết thúc tiến trình thay vì tháo stack[^rustc-panic]. Nó có thể tăng tốc nhẹ[^pb-panic].
- PGO là instrument, chạy, `llvm-profdata merge`, rồi build lại với `-Cprofile-use`[^pgo-steps]. Bước cuối cần giữ mọi cờ rustc giống nhau[^pgo-same-flags].
- Để profile build release, cần `debug = "line-tables-only"`[^pp-debug]. Nên thêm frame pointer[^pp-fp]. Symbol cũng cần được demangle[^pp-mangle]. Std phát hành không có debug info[^pp-std].
- Cấp phát heap tốn vì khóa toàn cục và system call[^pb-heap-cost]. Hãy đếm bằng DHAT hoặc `GlobalAlloc`, và nhớ docs không hứa mọi allocation trong source đều xảy ra[^ga-notrely].
- `tracing` có span, event và subscriber[^tr-sub]. Đừng giữ `Span::enter` qua `.await`[^tr-enter-warn].
- `tokio-console` cần `tokio_unstable`[^con-unstable].
- Đuôi latency của service Tokio đến từ chặn thread[^tokio-noblock].
- Task không yield cũng chiếm worker[^tokio-coop].
- Công bằng của Tokio chỉ có điều kiện[^tokio-fair].

## Câu hỏi tự kiểm tra

1. Vì sao một benchmark trả về 0 ns cho một hàm thuần có thể là lỗi của phép đo, và `black_box` sửa thế nào?
<details><summary>Đáp án</summary>

Trình biên dịch có thể xóa lời gọi hàm thuần mà kết quả không được dùng[^bb-nothing]. Nó cũng có thể thay lời gọi bằng hằng vì đầu vào biết lúc biên dịch, và những việc này làm benchmark rất thiếu chính xác[^bb-elim]. `black_box` bọc đầu vào và kết quả để trình biên dịch phải coi giá trị là không dự đoán được và phải tính ra[^bb-purposes]. Với đầu vào, mục đích cụ thể là tránh các tối ưu phi thực tế[^bb-inputs].

</details>

2. `black_box` có bảo đảm tuyệt đối chặn mọi tối ưu hóa không?
<details><summary>Đáp án</summary>

Không. Docs nói nó chỉ ở mức best-effort, tùy nền tảng và backend sinh code[^bb-bestef]. Chương trình cũng không được dựa vào nó cho tính đúng đắn, ngoài việc nó là hàm đồng nhất[^bb-correct].

</details>

3. Warm-up làm gì cho benchmark hàm Rust (không có JIT)?
<details><summary>Đáp án</summary>

Criterion nói với hàm Rust, warm-up nhằm làm nóng cache bộ xử lý và cache file system nếu có[^crit-warm]. Thuật toán là chạy 1, 2, 4… lần cho đến khi vượt ngưỡng thời gian[^crit-warm-alg].

</details>

4. Vì sao SRE book khuyên đếm request theo khoảng latency thay vì lưu trung bình?
<details><summary>Đáp án</summary>

Vì trung bình che đuôi: 100 ms trung bình vẫn có 1% request mất 5 giây[^sre-100ms]. Histogram thì phân biệt được trung bình chậm với đuôi rất chậm[^sre-bucket].

</details>

5. `hdrhistogram` tính p99,9 bằng lời gọi nào, và vì sao dùng đối số 0.999?
<details><summary>Đáp án</summary>

Bằng `value_at_quantile(0.999)`[^hdr-quantile]. Thư viện làm việc với quantile trong [0, 1] thay vì percentile trong [0, 100] để giảm mất chính xác do số dấu phẩy động[^hdr-quant-pct].

</details>

6. `lto = false` và `lto = "off"` khác nhau thế nào trong Cargo?
<details><summary>Đáp án</summary>

`false` là thin local LTO chỉ trong crate hiện tại, và không có LTO nếu codegen units là 1 hoặc opt-level là 0; `"off"` tắt hẳn[^cargo-lto-false]. Performance Book nhấn mạnh sự khác nhau này[^pb-lto-off].

</details>

7. Đặt `codegen-units = 1` đánh đổi gì?
<details><summary>Đáp án</summary>

Code sinh ra có thể nhanh hơn nhưng biên dịch chậm hơn[^rustc-cgu]. Mặc định là 16 cho build không incremental và 256 cho incremental[^rustc-cgu-default].

</details>

8. Bốn bước của PGO với rustc là gì, và vì sao "mọi cờ phải giống nhau"?
<details><summary>Đáp án</summary>

Build bản instrument, chạy để sinh `.profraw`, dùng `llvm-profdata` tạo `.profdata`, rồi build lại với `-Cprofile-use`[^pgo-steps]. Rustc book dặn mọi cờ rustc phải giống nhau ở bước cuối[^pgo-same-flags]; lý do kỹ thuật không được nêu trong nguồn.

</details>

9. Cần cấu hình gì để flame graph của build release có số dòng và stack tốt?
<details><summary>Đáp án</summary>

Cần `debug = "line-tables-only"`[^pp-debug]. Nên thêm `-C force-frame-pointers=yes`[^pp-fp]. Giữ symbol, tức không `strip`[^pb-strip]. Cuối cùng là demangle, hoặc dùng mangling `v0`[^pp-mangle]. Lưu ý std không có debug info[^pp-std].

</details>

10. Vì sao không nên assert cứng số allocation đếm được bằng `GlobalAlloc` làm bất biến của chương trình?
<details><summary>Đáp án</summary>

Docs cấm dựa vào việc allocation thật sự xảy ra, vì optimizer có thể loại bỏ chúng hoặc chuyển xuống stack[^ga-notrely]. Việc có allocation hay không cũng không thuộc hành vi chương trình[^ga-notbehavior].

</details>

11. Vì sao giữ guard của `Span::enter` qua `.await` làm sai trace, và ba cách thay thế?
<details><summary>Đáp án</summary>

Khi yield, scope bị thoát nhưng guard không bị drop, nên task khác chạy trong lúc vẫn ở trong span[^tr-enter-why]. Cách thứ nhất là `Span::in_scope`[^tr-in-scope]. Cách thứ hai là `Future::instrument`[^tr-instrument-combinator]. Cách thứ ba là `#[instrument]` trên `async fn`[^tr-instrument-async].

</details>

12. Vì sao một task chặn thread làm tăng p99 của các request không liên quan?
<details><summary>Đáp án</summary>

Các task chạy xen kẽ trên cùng thread, nên task chặn ngăn các task khác trên thread đó chạy[^tokio-noblock]. Bảo đảm công bằng của Tokio cũng chỉ có điều kiện là không task nào chặn thread[^tokio-fair]. Phần "p99 tăng" là suy luận từ mục 2 và 8.

</details>

## Đọc thêm

- [The Rust Performance Book](https://nnethercote.github.io/perf-book/) (trang sống): các chương Benchmarking, Build Configuration, Profiling, Heap Allocations — gần như toàn bộ danh mục kỹ thuật của chương này, ngắn gọn.
- [Rustc book — Profile-guided Optimization](https://doc.rust-lang.org/1.95.0/rustc/profile-guided-optimization.html): quy trình PGO đầy đủ cho `rustc` và Cargo.
- [Cargo reference — Profiles](https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html): mọi khóa `opt-level`, `lto`, `codegen-units`, `panic`, `debug`, `strip` và mặc định.
- [`std::hint::black_box`](https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html): ví dụ đầy đủ về lý do cần `black_box`.
- [`std::alloc::GlobalAlloc`](https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html): hợp đồng an toàn khi tự viết allocator, kể cả mục Re-entrance.
- [`tracing` 0.1.44](https://docs.rs/tracing/0.1.44/tracing/) và [`Span::enter`](https://docs.rs/tracing/0.1.44/tracing/struct.Span.html): span/event/subscriber, cảnh báo async.
- [SRE book — Monitoring Distributed Systems](https://sre.google/sre-book/monitoring-distributed-systems/) (trang sống), mục "Worrying About Your Tail": vì sao dùng histogram thay trung bình.
- [P11 · Chương 01 — perf](01-perf-va-sampling-profiler.md), [P3 · Chương 06 — Heap allocator](../03-memory/06-heap-allocator.md), [P7 · Chương 08 — Tokio](../07-io-async/08-tokio.md).


[^bb-def]: std::hint::black_box, Description — <https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html> — "An identity function that hints to the compiler to be maximally pessimistic about what black_box could do."
[^bb-bestef]: std::hint::black_box, Description — <https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html> — "The extent to which it can block optimisations may vary depending upon the platform and code-gen backend used."
[^bb-correct]: std::hint::black_box, Description — <https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html> — "Programs cannot rely on black_box for correctness, beyond it behaving as the identity function."
[^bb-elim]: std::hint::black_box, When is this useful? — <https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html> — "It is not likely that all of the above happens, but the compiler is definitely able to make some optimizations that could result in a very inaccurate benchmark."
[^bb-nothing]: std::hint::black_box, When is this useful? — <https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html> — "Nothing is done with the result of contains: delete this function call entirely"
[^bb-purposes]: std::hint::black_box, How to use this — <https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html> — "It forces the value passed to black_box to be calculated, even if the return value of black_box is unused"
[^bb-inputs]: std::hint::black_box, How to use this — <https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html> — "When benchmarking a function, you usually want to wrap its inputs in black_box so the compiler cannot make optimizations that would be unrealistic in real-life use."
[^crit-warm]: Criterion.rs Book, Command-Line Output (trang sống) — <https://bheisler.github.io/criterion.rs/book/user_guide/command_line_output.html> — "Every Criterion.rs benchmark iterates the benchmarked function automatically for a configurable warmup period (by default, for three seconds). For Rust function benchmarks, this is to warm up the processor caches and (if applicable) file system caches."
[^crit-warm-alg]: Criterion.rs Book, Analysis Process (trang sống) — <https://bheisler.github.io/criterion.rs/book/analysis.html> — "The warmup period is quite simple. The routine is executed once, then twice, four times and so on until the total accumulated execution time is greater than the configured warm up time."
[^crit-quiet]: Criterion.rs Book, Command-Line Output (trang sống) — <https://bheisler.github.io/criterion.rs/book/user_guide/command_line_output.html> — "In order to ensure reliable results, benchmarks should be run on a quiet computer and should be designed to do approximately the same amount of work for each iteration."
[^crit-noise]: Criterion.rs Book, Analysis Process, Comparison (trang sống) — <https://bheisler.github.io/criterion.rs/book/analysis.html> — "Since these sorts of unpredictable fluctuations are rarely of interest while benchmarking, there is also a configurable noise threshold."
[^crit-sample]: Criterion.rs Book, Analysis Process, Measurement (trang sống) — <https://bheisler.github.io/criterion.rs/book/analysis.html> — "Note that Criterion.rs does not measure each individual iteration, only the complete sample."
[^crit-outlier]: Criterion.rs Book, Analysis Process, Outlier Classification (trang sống) — <https://bheisler.github.io/criterion.rs/book/analysis.html> — "Note, however, that outlier samples are not dropped from the data, and are used in the following analysis steps along with all other samples."
[^pb-var]: The Rust Performance Book, Benchmarking (trang sống) — <https://nnethercote.github.io/perf-book/benchmarking.html> — "Wall-time is an obvious choice in many cases because it corresponds to what users perceive. However, it can suffer from high variance."
[^pb-layout]: The Rust Performance Book, Benchmarking (trang sống) — <https://nnethercote.github.io/perf-book/benchmarking.html> — "In particular, tiny changes in memory layout can cause significant but ephemeral performance fluctuations."
[^pb-instr]: The Rust Performance Book, Benchmarking (trang sống) — <https://nnethercote.github.io/perf-book/benchmarking.html> — "Therefore, other metrics with lower variance (such as cycles or instruction counts) may be a reasonable alternative."
[^pb-tools]: The Rust Performance Book, Benchmarking (trang sống) — <https://nnethercote.github.io/perf-book/benchmarking.html> — "Rust's built-in benchmark tests are a simple starting point, but they use unstable features and therefore only work on nightly Rust."
[^pb-workload]: The Rust Performance Book, Benchmarking (trang sống) — <https://nnethercote.github.io/perf-book/benchmarking.html> — "Ideally, you would have a variety of workloads that represent realistic usage of your program. Workloads using real-world inputs are best, but microbenchmarks and stress tests can be useful in moderation."
[^pb-mediocre]: The Rust Performance Book, Benchmarking (trang sống) — <https://nnethercote.github.io/perf-book/benchmarking.html> — "Mediocre benchmarking is far better than no benchmarking."
[^pb-release]: The Rust Performance Book, Build Configuration (trang sống) — <https://nnethercote.github.io/perf-book/build-configuration.html> — "In comparison, release builds are much more optimized, omit debug assertions and integer overflow checks, and omit debug info. 10-100x speedups over dev builds are common!"
[^cargo-bench-inherits]: Cargo Book, Profiles, bench — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "The bench profile inherits the settings from the release profile."
[^sre-mean]: Google SRE book, Monitoring Distributed Systems, Worrying About Your Tail (trang sống) — <https://sre.google/sre-book/monitoring-distributed-systems/> — "When building a monitoring system from scratch, it's tempting to design a system based upon the mean of some quantity: the mean latency, the mean CPU usage of your nodes, or the mean fullness of your databases."
[^sre-100ms]: Google SRE book, Monitoring Distributed Systems, Worrying About Your Tail (trang sống) — <https://sre.google/sre-book/monitoring-distributed-systems/> — "If you run a web service with an average latency of 100 ms at 1,000 requests per second, 1% of requests might easily take 5"
[^sre-fan]: Google SRE book, Monitoring Distributed Systems, Worrying About Your Tail (trang sống) — <https://sre.google/sre-book/monitoring-distributed-systems/> — "If your users depend on several such web services to render their page, the 99th percentile of one backend can easily become the median response of your frontend."
[^sre-bucket]: Google SRE book, Monitoring Distributed Systems, Worrying About Your Tail (trang sống) — <https://sre.google/sre-book/monitoring-distributed-systems/> — "collect request counts bucketed by latencies (suitable for rendering a histogram), rather than actual latencies"
[^sre-expo]: Google SRE book, Monitoring Distributed Systems, Worrying About Your Tail (trang sống) — <https://sre.google/sre-book/monitoring-distributed-systems/> — "Distributing the histogram boundaries approximately exponentially (in this case by factors of roughly 3) is often an easy way to visualize the distribution of your requests."
[^sre-fn23]: Google SRE book, Monitoring Distributed Systems, chú thích 23 (trang sống) — <https://sre.google/sre-book/monitoring-distributed-systems/> — "1% of your requests are 50x the average, it means that the rest of your requests are about twice as fast as the average."
[^sre-p99-sat]: Google SRE book, Monitoring Distributed Systems, Saturation (trang sống) — <https://sre.google/sre-book/monitoring-distributed-systems/> — "Measuring your 99th percentile response time over some small window (e.g., one minute) can give a very early signal of saturation."
[^sre-cpu]: Google SRE book, Monitoring Distributed Systems, Choosing an Appropriate Resolution for Measurements (trang sống) — <https://sre.google/sre-book/monitoring-distributed-systems/> — "Observing CPU load over the time span of a minute won't reveal even quite long-lived spikes that drive high tail latencies."
[^hdr-desc]: hdrhistogram 7.5.4, crate docs — <https://docs.rs/hdrhistogram/7.5.4/hdrhistogram/> — "The resulting HDR histogram allows for fast and accurate analysis of the extreme ranges of data with non-normal distributions, like latency."
[^hdr-fixed]: hdrhistogram 7.5.4, crate docs — <https://docs.rs/hdrhistogram/7.5.4/hdrhistogram/> — "The HDR Histogram maintains a fixed cost in both space and time."
[^hdr-quantile]: hdrhistogram 7.5.4, crate docs, Querying — <https://docs.rs/hdrhistogram/7.5.4/hdrhistogram/> — "At any time, the histogram can be queried to return interesting statistical measurements, such as the total number of recorded samples, or the value at a given quantile:"
[^hdr-quant-pct]: hdrhistogram 7.5.4, crate docs, Floating point issues — <https://docs.rs/hdrhistogram/7.5.4/hdrhistogram/> — "To minimize FP precision losses, we favor working with quantiles rather than percentiles. A quantile represents a portion of a set with a number in [0, 1]. A percentile is the same concept, except it uses the range [0, 100]."
[^hdr-u64]: hdrhistogram 7.5.4, crate docs — <https://docs.rs/hdrhistogram/7.5.4/hdrhistogram/> — "at the cost of a risk of saturating if a large number of samples end up in the same bin."
[^tr-timing]: tracing 0.1.44, crate docs, Related Crates — <https://docs.rs/tracing/0.1.44/tracing/> — "Additionally, there are also several third-party crates which are not maintained by the tokio project. These include: tracing-timing implements inter-event timing metrics on top of tracing. It provides a subscriber that records the time elapsed between pairs of tracing events and generates histograms."
[^cargo-release-defaults]: Cargo Book, Profiles, release — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "The default settings for the release profile are: [profile.release] opt-level = 3 debug = false split-debuginfo = '...' # Platform-specific. strip = "none" debug-assertions = false overflow-checks = false lto = false panic = 'unwind' incremental = false codegen-units = 16 rpath = false"
[^cargo-dev-defaults]: Cargo Book, Profiles, dev — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "The default settings for the dev profile are: [profile.dev] opt-level = 0 debug = true split-debuginfo = '...' # Platform-specific. strip = "none" debug-assertions = true overflow-checks = true lto = false panic = 'unwind' incremental = true codegen-units = 256 rpath = false"
[^cargo-opt]: Cargo Book, Profiles, opt-level — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "Higher optimization levels may produce faster runtime code at the expense of longer compiler times. Higher levels may also change and rearrange the compiled code which may make it harder to use with a debugger."
[^cargo-opt-surprise]: Cargo Book, Profiles, opt-level — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "There may be surprising results, such as level 3 being slower than 2,"
[^cargo-lto-thin]: Cargo Book, Profiles, lto — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "Performs thin LTO. This is similar to fat, but takes substantially less time to run while still achieving performance gains similar to fat."
[^cargo-lto-false]: Cargo Book, Profiles, lto — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "Performs thin local LTO which performs thin LTO on the local crate only across its codegen units. No LTO is performed if codegen units is 1 or opt-level is 0. "off": Disables LTO."
[^cargo-custom]: Cargo Book, Profiles, Custom profiles — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "When defining a custom profile, you must specify the inherits key to specify which profile the custom profile inherits settings from when the setting is not specified."
[^cargo-ws-profiles]: Cargo Book, Profiles — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "Cargo only looks at the profile settings in the Cargo.toml manifest at the root of the workspace. Profile settings defined in dependencies will be ignored."
[^cargo-build-override]: Cargo Book, Profiles, Build Dependencies — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "To compile quickly, all profiles, by default, do not optimize build dependencies (build scripts, proc macros, and their dependencies), and avoid computing debug info when a build dependency is not used as a runtime dependency. The default settings for build overrides are: [profile.dev.build-override] opt-level = 0 codegen-units = 256"
[^cargo-panic-ignored]: Cargo Book, Profiles, panic — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "Tests, benchmarks, build scripts, and proc macros ignore the panic setting. The rustc test harness currently requires unwind behavior."
[^cargo-line-tables]: Cargo Book, Profiles, debug — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "Generates the minimal amount of debug info for backtraces with filename/line number info, but not anything else, i.e. no variable or function parameter info."
[^cargo-debug-levels]: Cargo Book, Profiles, debug — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "1 or "limited": debug info without type or variable-level information. Generates more detailed module-level info than line-tables-only. 2, true, or "full": full debug info, default for dev"
[^rustc-O]: rustc book, Codegen options, opt-level — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "Note: The -O flag is an alias for -C opt-level=3."
[^rustc-cgu]: rustc book, Codegen options, codegen-units — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "When a crate is split into multiple codegen units, LLVM is able to process them in parallel. Increasing parallelism may speed up compile times, but may also produce slower code. Setting this to 1 may improve the performance of generated code, but may be slower to compile."
[^rustc-cgu-default]: rustc book, Codegen options, codegen-units — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "The default value, if not specified, is 16 for non-incremental builds. For incremental builds the default is 256 which allows caching to be more granular."
[^rustc-lto]: rustc book, Codegen options, lto — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "This flag controls whether LLVM uses link time optimizations to produce better optimized code, using whole-program analysis, at the cost of longer linking time."
[^rustc-lto-cgu1]: rustc book, Codegen options, lto — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "When -C lto is not specified, LTO is disabled if codegen units is 1 or optimizations are disabled (-C opt-level=0)."
[^rustc-lto-matrix]: rustc book, Codegen options, lto — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "codegen-units=1 + lto: 1 codegen unit, fat LTO across crates."
[^rustc-panic]: rustc book, Codegen options, panic — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "abort: terminate the process upon panic"
[^pb-cgu]: The Rust Performance Book, Build Configuration, Codegen Units (trang sống) — <https://nnethercote.github.io/perf-book/build-configuration.html> — "You may be able to improve runtime speed and reduce binary size, at the cost of increased compile times, by setting the number of units to one."
[^pb-lto]: The Rust Performance Book, Build Configuration, Link-time Optimization (trang sống) — <https://nnethercote.github.io/perf-book/build-configuration.html> — "Link-time optimization (LTO) is a whole-program optimization technique that can improve runtime speed by 10-20% or more, and also reduce binary size, at the cost of worse compile times."
[^pb-lto-off]: The Rust Performance Book, Build Configuration, Link-time Optimization (trang sống) — <https://nnethercote.github.io/perf-book/build-configuration.html> — "Note that this is different to the lto = false option, which, as mentioned above, leaves thin local LTO enabled."
[^pb-panic]: The Rust Performance Book, Build Configuration, Abort on panic! (trang sống) — <https://nnethercote.github.io/perf-book/build-configuration.html> — "This might reduce binary size and increase runtime speed slightly, and may even reduce compile times slightly."
[^pb-max]: The Rust Performance Book, Build Configuration, Summary (trang sống) — <https://nnethercote.github.io/perf-book/build-configuration.html> — "If you want to maximize runtime speed, consider all of the following: codegen-units = 1, lto = "fat", an alternative allocator, and panic = "abort"."
[^pb-cpu]: The Rust Performance Book, Build Configuration, CPU Specific Instructions (trang sống) — <https://nnethercote.github.io/perf-book/build-configuration.html> — "If you do not care about the compatibility of your binary on older (or other types of) processors, you can tell the compiler to generate the newest (and potentially fastest) instructions specific to a certain CPU architecture"
[^pb-strip]: The Rust Performance Book, Build Configuration, Strip Symbols (trang sống) — <https://nnethercote.github.io/perf-book/build-configuration.html> — "However, stripping symbols may make your compiled program more difficult to debug and profile."
[^tokio-jh-panic]: tokio 1.53.2, module task — <https://docs.rs/tokio/1.53.2/tokio/task/index.html> — "if the spawned task panics, awaiting its JoinHandle will return a JoinError."
[^pgo-concept]: rustc book, Profile-guided Optimization — <https://doc.rust-lang.org/1.95.0/rustc/profile-guided-optimization.html> — "The basic concept of PGO is to collect data about the typical execution of a program (e.g. which branches it is likely to take) and then use this data to inform optimizations such as inlining, machine-code layout, register allocation, etc."
[^pgo-instr]: rustc book, Profile-guided Optimization — <https://doc.rust-lang.org/1.95.0/rustc/profile-guided-optimization.html> — "The latter usually provides more accurate data and it is also what is supported by rustc."
[^pgo-steps]: rustc book, Profile-guided Optimization, Usage — <https://doc.rust-lang.org/1.95.0/rustc/profile-guided-optimization.html> — "Convert the .profraw file into a .profdata file using LLVM's llvm-profdata tool"
[^pgo-multi]: rustc book, Profile-guided Optimization, Usage — <https://doc.rust-lang.org/1.95.0/rustc/profile-guided-optimization.html> — "Running an instrumented binary multiple times, on the other hand, will re-use the respective .profraw files, updating them in place."
[^pgo-llvmtools]: rustc book, Profile-guided Optimization, Usage — <https://doc.rust-lang.org/1.95.0/rustc/profile-guided-optimization.html> — "Note that installing the llvm-tools-preview component won't add llvm-profdata to the PATH."
[^pgo-same-flags]: rustc book, Profile-guided Optimization, Usage — <https://doc.rust-lang.org/1.95.0/rustc/profile-guided-optimization.html> — "STEP 4: Use the merged .profdata file during optimization. All rustc flags have to be the same."
[^pgo-cargo-target]: rustc book, Profile-guided Optimization, A Complete Cargo Workflow — <https://doc.rust-lang.org/1.95.0/rustc/profile-guided-optimization.html> — "We pass the --target flag to Cargo, which prevents the RUSTFLAGS arguments to be passed to Cargo build scripts. We don't want the build scripts to generate a bunch of .profraw files."
[^pgo-release]: rustc book, Profile-guided Optimization, A Complete Cargo Workflow — <https://doc.rust-lang.org/1.95.0/rustc/profile-guided-optimization.html> — "We pass --release to Cargo because that's where PGO makes the most sense."
[^pgo-abs]: rustc book, Profile-guided Optimization, A Complete Cargo Workflow — <https://doc.rust-lang.org/1.95.0/rustc/profile-guided-optimization.html> — "It is recommended to use absolute paths for the argument of -Cprofile-generate and -Cprofile-use."
[^pgo-leftover]: rustc book, Profile-guided Optimization, A Complete Cargo Workflow — <https://doc.rust-lang.org/1.95.0/rustc/profile-guided-optimization.html> — "It is good practice to make sure that there is no left-over profiling data from previous compilation sessions."
[^pgo-warn]: rustc book, Profile-guided Optimization, Troubleshooting — <https://doc.rust-lang.org/1.95.0/rustc/profile-guided-optimization.html> — "It is recommended to pass -Cllvm-args=-pgo-warn-missing-function during the -Cprofile-use phase."
[^pgo-llvm]: rustc book, Profile-guided Optimization, Further Reading — <https://doc.rust-lang.org/1.95.0/rustc/profile-guided-optimization.html> — "rustc's PGO support relies entirely on LLVM's implementation of the feature and is equivalent to what Clang offers via the -fprofile-generate / -fprofile-use flags."
[^pgo-cargo-pgo]: rustc book, Profile-guided Optimization, Community Maintained Tools — <https://doc.rust-lang.org/1.95.0/rustc/profile-guided-optimization.html> — "you may choose to go with cargo-pgo, which has an intuitive command-line API and saves you the trouble of doing all the manual work."
[^pb-pgo]: The Rust Performance Book, Build Configuration, Profile-guided Optimization (trang sống) — <https://nnethercote.github.io/perf-book/build-configuration.html> — "Profile-guided optimization (PGO) is a compilation model where you compile your program, run it on sample data while collecting profiling data, and then use that profiling data to guide a second compilation of the program. This can improve runtime speed by 10% or more."
[^pb-pgo-effort]: The Rust Performance Book, Build Configuration, Profile-guided Optimization (trang sống) — <https://nnethercote.github.io/perf-book/build-configuration.html> — "It is an advanced technique that takes some effort to set up, but is worthwhile in some cases."
[^pb-pgo-cargo-install]: The Rust Performance Book, Build Configuration, Profile-guided Optimization (trang sống) — <https://nnethercote.github.io/perf-book/build-configuration.html> — "Unfortunately, PGO is not supported for binaries hosted on crates.io and distributed via cargo install, which limits its usability."
[^pp-hot]: The Rust Performance Book, Profiling (trang sống) — <https://nnethercote.github.io/perf-book/profiling.html> — "When optimizing a program, you also need a way to determine which parts of the program are "hot" (executed frequently enough to affect runtime) and worth modifying. This is best done via profiling."
[^pp-perf]: The Rust Performance Book, Profiling (trang sống) — <https://nnethercote.github.io/perf-book/profiling.html> — "perf is a general-purpose profiler that uses hardware performance counters. Hotspot and Firefox Profiler are good for viewing data recorded by perf. It works on Linux."
[^pp-instruments]: The Rust Performance Book, Profiling (trang sống) — <https://nnethercote.github.io/perf-book/profiling.html> — "Instruments is a general-purpose profiler that comes with Xcode on macOS."
[^pp-samply]: The Rust Performance Book, Profiling (trang sống) — <https://nnethercote.github.io/perf-book/profiling.html> — "samply is a sampling profiler that produces profiles that can be viewed in the Firefox Profiler. It works on Mac, Linux, and Windows."
[^pp-flame]: The Rust Performance Book, Profiling (trang sống) — <https://nnethercote.github.io/perf-book/profiling.html> — "flamegraph is a Cargo command that uses perf/DTrace to profile your code and then displays the results in a flame graph."
[^pp-cachegrind]: The Rust Performance Book, Profiling (trang sống) — <https://nnethercote.github.io/perf-book/profiling.html> — "Cachegrind & Callgrind give global, per-function, and per-source-line instruction counts and simulated cache and branch prediction data."
[^pp-dhat]: The Rust Performance Book, Profiling (trang sống) — <https://nnethercote.github.io/perf-book/profiling.html> — "DHAT is good for finding which parts of the code are causing a lot of allocations, and for giving insight into peak memory usage. It can also be used to identify hot calls to memcpy. It works on Linux and some other Unixes. dhat-rs is an experimental alternative that is a little less powerful and requires minor changes to your Rust program, but works on all platforms."
[^pp-heaptrack]: The Rust Performance Book, Profiling (trang sống) — <https://nnethercote.github.io/perf-book/profiling.html> — "heaptrack and bytehound are heap profiling tools. They work on Linux."
[^pp-coz]: The Rust Performance Book, Profiling (trang sống) — <https://nnethercote.github.io/perf-book/profiling.html> — "Coz performs causal profiling to measure optimization potential, and has Rust support via coz-rs. It works on Linux."
[^pp-debug]: The Rust Performance Book, Profiling, Debug Info (trang sống) — <https://nnethercote.github.io/perf-book/profiling.html> — "To profile a release build effectively you might need to enable source line debug info."
[^pp-std]: The Rust Performance Book, Profiling, Debug Info (trang sống) — <https://nnethercote.github.io/perf-book/profiling.html> — "This is because shipped versions of the Rust standard library are not built with debug info."
[^pp-fp]: The Rust Performance Book, Profiling, Frame pointers (trang sống) — <https://nnethercote.github.io/perf-book/profiling.html> — "The Rust compiler may optimize away frame pointers, which can hurt the quality of profiling information such as stack traces."
[^pp-mangle]: The Rust Performance Book, Profiling, Symbol Demangling (trang sống) — <https://nnethercote.github.io/perf-book/profiling.html> — "If you are having trouble with symbol demangling while profiling, it may be worth changing the mangling format from the default legacy format to the newer v0 format."
[^pb-heap-cost]: The Rust Performance Book, Heap Allocations (trang sống) — <https://nnethercote.github.io/perf-book/heap-allocations.html> — "Heap allocations are moderately expensive. The exact details depend on which allocator is in use, but each allocation (and deallocation) typically involves acquiring a global lock, doing some non-trivial data structure manipulation, and possibly executing a system call."
[^pb-heap-small]: The Rust Performance Book, Heap Allocations (trang sống) — <https://nnethercote.github.io/perf-book/heap-allocations.html> — "Small allocations are not necessarily cheaper than large allocations."
[^pb-heap-prof]: The Rust Performance Book, Heap Allocations, Profiling (trang sống) — <https://nnethercote.github.io/perf-book/heap-allocations.html> — "If a general-purpose profiler shows malloc, free, and related functions as hot, then it is likely worth trying to reduce the allocation rate and/or using an alternative allocator."
[^pb-dhat]: The Rust Performance Book, Heap Allocations, Profiling (trang sống) — <https://nnethercote.github.io/perf-book/heap-allocations.html> — "DHAT is an excellent profiler to use when reducing allocation rates. It works on Linux and some other Unixes. It precisely identifies hot allocation sites and their allocation rates."
[^pb-dhat-rustc]: The Rust Performance Book, Heap Allocations, Profiling (trang sống) — <https://nnethercote.github.io/perf-book/heap-allocations.html> — "experience with rustc has shown that reducing allocation rates by 10 allocations per million instructions executed can have measurable performance improvements (e.g. ~1%)."
[^pb-withcap]: The Rust Performance Book, Heap Allocations, Vec (trang sống) — <https://nnethercote.github.io/perf-book/heap-allocations.html> — "If you know the minimum or exact size of a vector, you can reserve a specific capacity with Vec::with_capacity, Vec::reserve, or Vec::reserve_exact."
[^pb-smallvec]: The Rust Performance Book, Heap Allocations, SmallVec (trang sống) — <https://nnethercote.github.io/perf-book/heap-allocations.html> — "SmallVec reliably reduces the allocation rate when used appropriately, but its use does not guarantee improved performance."
[^pb-dhat-test]: The Rust Performance Book, Heap Allocations (trang sống) — <https://nnethercote.github.io/perf-book/heap-allocations.html> — "you can use the heap usage testing feature of dhat-rs to write tests that check particular code snippets allocate the expected amount of heap memory."
[^pb-altalloc]: The Rust Performance Book, Build Configuration, Alternative Allocators (trang sống) — <https://nnethercote.github.io/perf-book/build-configuration.html> — "It is possible to replace the default (system) heap allocator used by a Rust program with an alternative allocator. The exact effect will depend on the individual program and the alternative allocator chosen, but large improvements in runtime speed and large reductions in memory usage have been seen in practice."
[^pb-altalloc-cost]: The Rust Performance Book, Build Configuration, Alternative Allocators (trang sống) — <https://nnethercote.github.io/perf-book/build-configuration.html> — "The use of an alternative allocator is also likely to increase binary size and compile times."
[^pb-jemalloc]: The Rust Performance Book, Build Configuration, jemalloc (trang sống) — <https://nnethercote.github.io/perf-book/build-configuration.html> — "One popular alternative allocator for Linux and Mac is jemalloc, usable via the tikv-jemallocator crate."
[^pb-jemalloc-thp]: The Rust Performance Book, Build Configuration, jemalloc (trang sống) — <https://nnethercote.github.io/perf-book/build-configuration.html> — "Furthermore, on Linux, jemalloc can be configured to use transparent huge pages (THP). This can further speed up programs, possibly at the cost of higher memory usage."
[^pb-mimalloc]: The Rust Performance Book, Build Configuration, mimalloc (trang sống) — <https://nnethercote.github.io/perf-book/build-configuration.html> — "Another alternative allocator that works on many platforms is mimalloc, usable via the mimalloc crate."
[^ga-desc]: std::alloc::GlobalAlloc, Description — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html> — "A memory allocator that can be registered as the standard library's default through the #[global_allocator] attribute."
[^ga-unwind]: std::alloc::GlobalAlloc, Safety — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html> — "It's undefined behavior if global allocators unwind."
[^ga-notrely]: std::alloc::GlobalAlloc, Safety — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html> — "You must not rely on allocations actually happening, even if there are explicit heap allocations in the source."
[^ga-notbehavior]: std::alloc::GlobalAlloc, Safety — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html> — "Whether allocations happen or not is not part of the program behavior, even if it could be detected via an allocator that tracks allocations by printing or otherwise having side effects."
[^ga-reent]: std::alloc::GlobalAlloc, Re-entrance — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html> — "When implementing a global allocator, one has to be careful not to create an infinitely recursive implementation by accident, as many constructs in the Rust standard library may allocate in their implementation."
[^ga-mutex]: std::alloc::GlobalAlloc, Re-entrance — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html> — "For example, on some platforms, std::sync::Mutex may allocate, so using it is highly problematic in a global allocator."
[^ga-core]: std::alloc::GlobalAlloc, Re-entrance — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html> — "For this reason, one should generally stick to library features available through core, and avoid using std in a global allocator."
[^ga-realloc]: std::alloc::GlobalAlloc::realloc — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html> — "The range 0..min(layout.size(), new_size) of the new memory block is guaranteed to have the same values as the original block."
[^tr-what]: tracing 0.1.44, crate docs — <https://docs.rs/tracing/0.1.44/tracing/> — "tracing is a framework for instrumenting Rust programs to collect structured, event-based diagnostic information."
[^tr-async]: tracing 0.1.44, crate docs — <https://docs.rs/tracing/0.1.44/tracing/> — "In asynchronous systems like Tokio, interpreting traditional log messages can often be quite challenging. Since individual tasks are multiplexed on the same thread, associated events and log lines are intermixed making it difficult to trace the logic flow."
[^tr-span-nest]: tracing 0.1.44, crate docs — <https://docs.rs/tracing/0.1.44/tracing/> — "unlike a log message, a span in tracing has a beginning and end time, may be entered and exited by the flow of execution, and may exist within a nested tree of similar spans."
[^tr-span]: tracing 0.1.44, crate docs, Spans — <https://docs.rs/tracing/0.1.44/tracing/> — "Unlike a log line that represents a moment in time, a span represents a period of time with a beginning and an end."
[^tr-event]: tracing 0.1.44, crate docs, Events — <https://docs.rs/tracing/0.1.44/tracing/> — "In general, events should be used to represent points in time within a span — a request returned with a given status code, n new items were taken from a queue, and so on."
[^tr-sub]: tracing 0.1.44, crate docs, Subscribers — <https://docs.rs/tracing/0.1.44/tracing/> — "As Spans and Events occur, they are recorded or aggregated by implementations of the Subscriber trait."
[^tr-enabled]: tracing 0.1.44, crate docs, Subscribers — <https://docs.rs/tracing/0.1.44/tracing/> — "For performance reasons, if no currently active subscribers express interest in a given set of metadata by returning true, then the corresponding Span or Event will never be constructed."
[^tr-nosub]: tracing 0.1.44, crate docs, In executables — <https://docs.rs/tracing/0.1.44/tracing/> — "This library does not contain any Subscriber implementations; these are provided by other crates."
[^tr-global]: tracing 0.1.44, crate docs, In executables — <https://docs.rs/tracing/0.1.44/tracing/> — "Any trace events generated outside the context of a subscriber will not be collected."
[^tr-ts]: tracing 0.1.44, crate docs, Related Crates — <https://docs.rs/tracing/0.1.44/tracing/> — "tracing-subscriber provides Subscriber implementations and utilities for working with Subscribers."
[^tr-instrument]: tracing 0.1.44, crate docs, Using the Macros — <https://docs.rs/tracing/0.1.44/tracing/> — "A function annotated with #[instrument] will create and enter a span with that function's name every time the function is called, with arguments to that function will be recorded as fields using fmt::Debug."
[^tr-skip]: Tokio topics, Getting started with Tracing — <https://tokio.rs/tokio/topics/tracing> — "skip(self) indicates that emitted span should not record Handler's debug representation."
[^tr-enter-warn]: tracing 0.1.44, Span::enter — <https://docs.rs/tracing/0.1.44/tracing/struct.Span.html> — "Warning: in asynchronous code that uses async/await syntax, Span::enter should be used very carefully or avoided entirely. Holding the drop guard returned by Span::enter across .await points will result in incorrect traces."
[^tr-enter-why]: tracing 0.1.44, Span::enter — <https://docs.rs/tracing/0.1.44/tracing/struct.Span.html> — "This means that another task will begin executing while remaining in the entered span. This results in an incorrect trace."
[^tr-in-scope]: tracing 0.1.44, Span::enter — <https://docs.rs/tracing/0.1.44/tracing/struct.Span.html> — "the span will always be exited before the next await point."
[^tr-instrument-combinator]: tracing 0.1.44, Span::enter — <https://docs.rs/tracing/0.1.44/tracing/struct.Span.html> — "This will enter the span every time the future is polled, and exit it whenever the future yields."
[^tr-instrument-async]: tracing 0.1.44, Span::enter — <https://docs.rs/tracing/0.1.44/tracing/struct.Span.html> — "The #[instrument] attribute macro can automatically generate correct code when used on an async function:"
[^tr-log]: tracing 0.1.44, crate docs, Emitting Log Records — <https://docs.rs/tracing/0.1.44/tracing/> — "Enabling the log-always feature will cause log records to be emitted even if a tracing Subscriber is set."
[^tokio-tracing-early]: Tokio topics, Getting started with Tracing — <https://tokio.rs/tokio/topics/tracing> — "In most circumstances, you should register your tracing subscriber as early as possible in your main function."
[^tokio-tracing-layer]: Tokio topics, Getting started with Tracing — <https://tokio.rs/tokio/topics/tracing> — "the tracing-subscriber crate provides a Layer trait that represents a component that may be composed together with other Layers to form a Subscriber."
[^con-what]: console-subscriber 0.5.0, crate docs, Overview — <https://docs.rs/console-subscriber/0.5.0/console_subscriber/> — "tokio-console is a debugging and profiling tool for asynchronous Rust applications, which collects and displays in-depth diagnostic data on the asynchronous tasks, resources, and operations in an application."
[^con-impl]: console-subscriber 0.5.0, crate docs, Overview — <https://docs.rs/console-subscriber/0.5.0/console_subscriber/> — "It provides a type implementing the Layer trait from tracing-subscriber, for collecting and aggregating the runtime's tracing data, and a gRPC server that exports telemetry to clients."
[^con-experimental]: console-subscriber 0.5.0, crate docs, Enabling Tokio Instrumentation — <https://docs.rs/console-subscriber/0.5.0/console_subscriber/> — "Currently, the tracing support in the tokio runtime is considered experimental."
[^con-unstable]: console-subscriber 0.5.0, crate docs, Enabling Tokio Instrumentation — <https://docs.rs/console-subscriber/0.5.0/console_subscriber/> — "The tokio_unstable cfg flag, which enables experimental APIs in Tokio, must be enabled."
[^con-tokio-only]: console-subscriber 0.5.0, crate docs, Getting Started — <https://docs.rs/console-subscriber/0.5.0/console_subscriber/> — "Currently, the only runtime that implements this instrumentation is Tokio version 1.7.0 and newer."
[^con-retention]: console-subscriber 0.5.0, crate docs, Configuration — <https://docs.rs/console-subscriber/0.5.0/console_subscriber/> — "set how long the console will retain data from completed tasks"
[^con-targets]: tokio-console v0.1.13, README — <https://raw.githubusercontent.com/tokio-rs/console/tokio-console-v0.1.13/README.md> — "these targets are enabled automatically."
[^con-lost]: tokio-console v0.1.13, README, --warn — <https://raw.githubusercontent.com/tokio-rs/console/tokio-console-v0.1.13/README.md> — "Warns when a task is dropped without being woken."
[^con-never]: tokio-console v0.1.13, README, --warn — <https://raw.githubusercontent.com/tokio-rs/console/tokio-console-v0.1.13/README.md> — "Warns when a task has never yielded."
[^con-self]: tokio-console v0.1.13, README, --warn — <https://raw.githubusercontent.com/tokio-rs/console/tokio-console-v0.1.13/README.md> — "Warns when a task wakes itself more than a certain percentage of its total wakeups."
[^tokio-noblock]: tokio 1.53.2, module task — <https://docs.rs/tokio/1.53.2/tokio/task/index.html> — "Tasks should generally not perform system calls or other operations that could block a thread, as this would prevent other tasks running on the same thread from executing as well."
[^tokio-coop]: tokio 1.53.2, module task — <https://docs.rs/tokio/1.53.2/tokio/task/index.html> — "a task is allowed to run until it yields, indicating to the Tokio runtime's scheduler that it cannot currently continue executing."
[^tokio-blocking-apis]: tokio 1.53.2, module task — <https://docs.rs/tokio/1.53.2/tokio/task/index.html> — "Instead, Tokio provides two APIs for running blocking operations in an asynchronous context: task::spawn_blocking and task::block_in_place."
[^tokio-destructors]: tokio 1.53.2, module task — <https://docs.rs/tokio/1.53.2/tokio/task/index.html> — "This includes destructors of objects destroyed in async code."
[^tokio-sb-abort]: tokio 1.53.2, module task — <https://docs.rs/tokio/1.53.2/tokio/task/index.html> — "Be aware that tasks spawned using spawn_blocking cannot be aborted because they are not async."
[^tokio-yield]: tokio 1.53.2, module task — <https://docs.rs/tokio/1.53.2/tokio/task/index.html> — "Calling and awaiting this function will cause the current task to yield to the Tokio runtime's scheduler, allowing other tasks to be scheduled."
[^tokio-fair]: tokio 1.53.2, module runtime, Fairness — <https://docs.rs/tokio/1.53.2/tokio/runtime/index.html> — "If the total number of tasks does not grow without bound, and no task is blocking the thread, then it is guaranteed that tasks are scheduled fairly."
[^tokio-fair-neg]: tokio 1.53.2, module runtime, Fairness — <https://docs.rs/tokio/1.53.2/tokio/runtime/index.html> — "There is also no guarantee that the runtime is equally fair to all tasks. For example, if the runtime has two tasks A and B that are both ready, then the runtime may schedule A five times before it schedules B."
[^tokio-io-timer]: tokio 1.53.2, module runtime, Fairness — <https://docs.rs/tokio/1.53.2/tokio/runtime/index.html> — "Tokio guarantees that it will wake tasks with an IO or timer event within some maximum number of time units."
[^tokio-gqi]: tokio 1.53.2, module runtime, Detailed runtime behavior — <https://docs.rs/tokio/1.53.2/tokio/runtime/index.html> — "then the runtime will dynamically compute it using a heuristic that targets 10ms intervals between each check of the global queue (based on the worker_mean_poll_time metric)."
[^tokio-event-interval]: tokio 1.53.2, module runtime, Detailed runtime behavior — <https://docs.rs/tokio/1.53.2/tokio/runtime/index.html> — "The runtime will check for new IO or timer events whenever there are no tasks ready to be scheduled, or when it has scheduled 61 tasks in a row. The number 61 may be changed using the event_interval setting."
[^bb-example]: std::hint::black_box, When is this useful? — <https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html> — "Treats the call to contains and its result as volatile: the body of benchmark cannot optimize this away"
[^crit-regress]: Criterion.rs Book, Analysis Process, Linear Regression (trang sống) — <https://bheisler.github.io/criterion.rs/book/analysis.html> — "The slope of that line gives an estimate of the time per iteration."
[^crit-tukey]: Criterion.rs Book, Analysis Process, Outlier Classification (trang sống) — <https://bheisler.github.io/criterion.rs/book/analysis.html> — "Each sample is classified using a modified version of Tukey's Method, which will be summarized here. First, the interquartile range (IQR) is calculated from the difference between the 25th and 75th percentile."
[^hdr-precision]: hdrhistogram 7.5.4, crate docs, HdrHistogram — <https://docs.rs/hdrhistogram/7.5.4/hdrhistogram/> — "Value precision is expressed as the number of significant digits in the value recording, and provides control over value quantization behavior across the value range and the subsequent value resolution at any given level."
[^hdr-bounds]: hdrhistogram 7.5.4, crate docs, Recording samples — <https://docs.rs/hdrhistogram/7.5.4/hdrhistogram/> — "create a Histogram that can count values in the [1..3600000] range with 1% precision, which could be used to track latencies in the range [1 msec..1 hour]"
[^cargo-lto-fat]: Cargo Book, Profiles, lto — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "Performs fat LTO which attempts to perform optimizations across all crates within the dependency graph."
[^cargo-overrides]: Cargo Book, Profiles, Overrides — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "Profile settings can be overridden for specific packages and build-time crates."
[^book-unwind]: The Rust Programming Language, ch. 9.1, Unwinding the Stack or Aborting in Response to a Panic — <https://doc.rust-lang.org/1.95.0/book/ch09-01-unrecoverable-errors-with-panic.html> — "By default, when a panic occurs, the program starts unwinding, which means Rust walks back up the stack and cleans up the data from each function it encounters. However, walking back and cleaning up is a lot of work. Rust therefore allows you to choose the alternative of immediately aborting, which ends the program without cleaning up."
[^rustc-cpu-native]: rustc book, Codegen options, target-cpu — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "Special values include: native can be passed to use the processor of the host machine."
[^sig-ill]: signal(7), Standard signals — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "SIGILL P1990 Core Illegal Instruction"
[^tr-otel]: tracing 0.1.44, crate docs, Related Crates — <https://docs.rs/tracing/0.1.44/tracing/> — "tracing-opentelemetry provides a subscriber for emitting traces to OpenTelemetry-compatible distributed tracing systems."
[^ga-realloc-default]: core::alloc::GlobalAlloc, phương thức realloc mặc định, source Rust 1.95.0 (chi tiết cài đặt) — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/core/src/alloc/global.rs> — "let new_ptr = unsafe { self.alloc(new_layout) };"
[^tokio-sb-pool]: tokio 1.53.2, module task, spawn_blocking — <https://docs.rs/tokio/1.53.2/tokio/task/index.html> — "it instead spawns a blocking function on a dedicated thread pool for blocking tasks."
[^tokio-blocking-limit]: tokio 1.53.2, runtime::Builder::max_blocking_threads — <https://docs.rs/tokio/1.53.2/tokio/runtime/struct.Builder.html> — "It's recommended to not set this limit too low in order to avoid hanging on operations requiring spawn_blocking. The default value is 512."
[^tokio-blocking-queue]: tokio 1.53.2, runtime::Builder::max_blocking_threads, Queue Behavior — <https://docs.rs/tokio/1.53.2/tokio/runtime/struct.Builder.html> — "If no idle thread is available and no more threads are allowed to be spawned, the task will remain in the queue until one of the busy threads pick it up."
[^tokio-fair-formal]: tokio 1.53.2, module runtime, Fairness — <https://docs.rs/tokio/1.53.2/tokio/runtime/index.html> — "Then, there is some number MAX_DELAY such that when a task is woken, it will be scheduled by the runtime within MAX_DELAY time units."
[^tokio-detail-warn]: tokio 1.53.2, module runtime, Multi threaded runtime (behavior at the time of writing) — <https://docs.rs/tokio/1.53.2/tokio/runtime/index.html> — "This section describes how the multi thread runtime behaves today. This behavior may change in future versions of Tokio."
