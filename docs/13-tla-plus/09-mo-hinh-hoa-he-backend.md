# P13 · Chương 09 — Mô hình hóa hệ backend thực tế

> **Cách đọc chương này.** Mọi đoạn ```` ```tla ```` là **spec minh họa do người soạn viết**; theo quy ước của dự án, chúng **chưa được chạy qua TLC hay Apalache**. Vì vậy mỗi câu dạng "spec này vi phạm invariant X" là *suy luận của người soạn kèm trace viết tay* (ghi nhãn "chưa chạy TLC"), không phải kết quả model checking. Kết quả model checking chỉ được nêu khi nguồn tự ghi (repo `tlaplus/Examples` ghim ở commit `6a7b26a`, bài báo AWS). PlusCal dùng cú pháp P (`begin … end process`) theo PlusCal manual. `learntla.com` và `apalache-mc.org/docs` là **trang sống**, không có số phiên bản: nội dung có thể đổi sau ngày soạn.

## Bạn sẽ hiểu được gì

- **Q1.** Khi mô hình hóa một hệ backend, giữ gì, bỏ gì, và đặt "ranh giới nguyên tử" ở đâu? Crash, pause, timeout, thời gian đi vào spec thế nào? → mục 1
- **Q2.** Distributed lock có lease và fencing token được viết thành spec ra sao; vì sao lease một mình hỏng, và vì sao thêm một bước "kiểm tra còn hạn" vẫn không cứu được? → mục 2
- **Q3.** Hàng đợi at-least-once cộng consumer idempotent được mô hình hóa thế nào; vì sao bản ghi dedup phải nằm *cùng một bước nguyên tử* với tác dụng phụ? → mục 3
- **Q4.** Cache-aside và cuộc đua với DB (stale cache) được viết thành spec ra sao; tính chất đúng cần phát biểu là gì (không phải "cache luôn bằng DB")? → mục 4
- **Q5.** Transactional outbox giải quyết gì so với dual write ở mức spec, và để lại gì (trùng lặp)? → mục 5
- **Q6.** Retry kèm timeout sinh ra tác dụng phụ lặp thế nào, idempotency key sửa ở đâu, và phần nào của vấn đề retry nằm *ngoài* tầm TLA+? → mục 6
- **Q7.** Làm sao giữ không gian trạng thái trong tầm TLC: mô hình nhỏ trước, symmetry, constraint, view, ngân sách lỗi — và làm sao biết một lần "không thấy lỗi" không phải kết quả rỗng? → mục 7
- **Q8.** Apalache (symbolic, SMT) khác TLC ở đâu, kiểm bất biến quy nạp thế nào, cần gì để chạy spec của bạn, và khi nào *không* nên đổi? → mục 8

## Kiến thức cần trước

- [Chương 03 — Trạng thái, hành vi và action](03-trang-thai-hanh-vi-action.md): `Init`, `Next`, `UNCHANGED`, grain of atomicity.
- [Chương 04 — Invariant và TLC](04-invariant-va-tlc.md): cách TLC kiểm invariant, file `.cfg`, đọc counterexample.
- [Chương 05 — Liveness và fairness](05-liveness-va-fairness.md): vì sao chương này gần như chỉ kiểm *safety* và chỉ nhắc liveness.
- [Chương 06 — PlusCal](06-pluscal.md): `process`, label, `await`, `either`, `with`. Mọi spec chương này viết bằng PlusCal (riêng mục 8 dùng TLA+ thuần).
- Phía hệ thống: [P12·08 — Sự cố phân tán](../12-he-thong-du-lieu/08-su-co-he-phan-tan.md) (lease, fencing, process pause), [P12·11 — Stream processing](../12-he-thong-du-lieu/11-stream-processing.md) mục 4 (at-least-once), [P12·12 — Đúng đắn và tích hợp dữ liệu](../12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md) (idempotency, outbox).

(Chương 10 — [Từ đặc tả đến code Rust](10-tu-dac-ta-den-code-rust.md) nói cách nối spec với code; chương 07 — [Refinement](07-refinement.md) nói cách xếp spec thô và spec chi tiết thành bậc thang.)

## 1. Chọn độ trừu tượng: giữ gì, bỏ gì, đặt ranh giới nguyên tử ở đâu (Q1)

> **Trả lời ngắn.** Một spec là bản mô tả *trừu tượng* của thiết kế, đi kèm bản trừu tượng của *môi trường* nó chạy trong đó (crash, mạng, pause)[^aws-abstract]. Giữ những gì quyết định đúng/sai: dữ liệu ở từng kho, thông điệp đang bay, từng bước mà hệ thật coi là nguyên tử, và các sự kiện lỗi. Bỏ phần còn lại, vì spec càng chi tiết thì càng nhiều state[^lt-detail]. Độ trừu tượng đúng là mức thô nhất mà vẫn tái hiện được lỗi bạn đang săn (nhận xét của người soạn).

**Giải thích.**

Hình dung (nhận xét của người soạn): sơ đồ tàu điện ngầm không vẽ khoảng cách thật hay độ cong của đường ray; nó chỉ giữ thứ tự các ga và chỗ đổi tuyến — vậy mà đủ để trả lời "đi từ A đến B phải đổi tàu ở đâu". Spec là sơ đồ tàu điện của hệ thống: bỏ payload, tải, độ trễ cụ thể; giữ thứ tự các bước và những chỗ hai tiến trình có thể chen vào nhau.

Sáu quyết định gặp ở mọi spec backend (danh sách là nhận xét của người soạn; từng ý có nguồn riêng):

1. **Biến = những nơi dữ liệu có thể bất đồng.** Mỗi kho (bảng DB, cache, broker) là một biến; thông điệp đang bay là một tập hoặc dãy. Dữ liệu bên trong thì co lại thật nhỏ: thay vì tải máy chủ 0..100, người có kinh nghiệm chỉ giữ 0..3 hoặc một cờ boolean[^lt-load]. Chỉ giữ chi tiết khi lỗi phụ thuộc vào nó.

2. **Một label = một bước nguyên tử.** Trong PlusCal, một bước chạy từ label này đến label kế tiếp[^pm-step]; với thuật toán đa tiến trình, người viết nên tự chọn grain of atomicity[^pm-grain]. Quy tắc thực hành (nhận xét của người soạn): thao tác mà hệ thật *bảo đảm* nguyên tử — một transaction DB, một lệnh compare-and-set, một `fetch_max` nguyên tử — là *một* label; hai thao tác mà giữa chúng tiến trình khác chen được (đọc DB rồi mới set cache; commit rồi mới publish) là *hai* label. Chọn grain quá thô có thể che mất lỗi thật; quá mịn thì model checking quá lâu[^lt-grain].

3. **Môi trường là một phần của spec.** AWS mô tả cách làm: nêu đặc tính môi trường mà hệ thống dựa vào ("what must go right"), rồi đặc tả tác động của từng sự kiện có thể xảy ra — lỗi mạng và hồi phục, lỗi đĩa, process crash và restart, thao tác của người vận hành[^aws-events]. Họ thấy cách này ít sai sót hơn cách liệt kê "cái gì có thể hỏng"[^aws-needs-right]. Với PlusCal: crash là một nhánh `either … or` bỏ qua bước (kèm ngân sách, mục 3 và 5); trùng/mất thông điệp là nhánh nondeterministic; **pause không cần mô hình riêng**, vì thuật toán đa tiến trình được thực thi bằng cách liên tục chọn một tiến trình *bất kỳ* và chạy một bước của nó[^pm-arbitrary] — nên một tiến trình đứng yên giữa hai label lâu tùy ý là hành vi hợp lệ (suy luận của người soạn từ câu trích).

4. **Thời gian: trừu tượng hóa tối đa.** Kleppmann: các thuật toán trong mô hình bất đồng bộ thường bảo đảm safety mà không cần giả định thời gian; chỉ liveness phụ thuộc timeout hay failure detector[^kb-safety-liveness]. Hệ quả cho spec (suy luận của người soạn): hết hạn lease, timeout RPC chỉ cần là hành động "có thể xảy ra bất cứ lúc nào", không cần đồng hồ. Khi bắt buộc phải có "thời gian", spec trong repo Examples dùng một *bộ đếm logic* (`TimeoutCounter`, tăng một đơn vị mỗi lần kích hoạt timeout)[^ex-cp-timeout]; còn Lamport, ở chương real-time của sách, dùng biến `now`[^ss-now] và nhận xét rằng đặc tả real-time không phổ biến: spec thường nói hệ *làm gì* chứ không nói *mất bao lâu*[^ss-realtime-rare].

5. **Tính chất: nói rõ cái gì "không bao giờ" và cái gì "rồi sẽ".** AWS chia hai loại: safety ("hệ được phép làm gì", ví dụ mọi dữ liệu đã commit luôn còn nguyên và đúng)[^aws-safety] và liveness ("hệ phải rồi sẽ làm gì")[^aws-liveness]. Chương này chủ yếu kiểm safety; liveness cần fairness ([chương 05](05-liveness-va-fairness.md)).

6. **Biên hữu hạn.** TLC duyệt state; spec phải có biên (số client, số message, ngân sách crash). Chi tiết ở mục 7.

```mermaid
flowchart LR
  R["Hệ thật: DB, cache, broker, client"] --> C["Chọn: biến, bước nguyên tử, sự kiện lỗi, cách thể hiện thời gian"]
  C --> S["Spec PlusCal/TLA+ + biên hữu hạn"]
  S --> M["TLC hoặc Apalache"]
  M -->|"counterexample"| T["Đọc trace: lỗi thiết kế hay lỗi spec?"]
  M -->|"không thấy lỗi"| V["Kiểm tra không rỗng: mục 7"]
  T -->|"sửa thiết kế/spec"| S
  M -->|"quá lớn"| G["Giảm chi tiết, đổi hằng, symmetry"]
  G --> S
```

Bảng ánh xạ thành phần backend sang cấu trúc spec (nhận xét của người soạn, dùng xuyên suốt chương):

| Thành phần hệ thật | Trong spec |
| --- | --- |
| Bảng/row trong DB | biến, thường là hàm `Key -> Value` hoặc tập |
| Một DB transaction | **một** label (không thể bị chen) |
| Thông điệp trên mạng | phần tử của tập/dãy `net`; gửi và nhận là hai label |
| Process crash/restart | nhánh `either … or` bỏ qua bước, kèm ngân sách `MaxCrashes` |
| GC pause, preempt, trễ mạng | không cần hành động riêng: interleaving đã cho phép |
| Lease TTL, RPC timeout | hành động có thể xảy ra bất cứ lúc nào (`Expire`, nhánh `skip`) |
| Retry | vòng `while` có trần `MaxRetries` |
| Tải, độ trễ, kích thước payload | bỏ |

*Một ví dụ có thật: Checkpoint Coordination trong `tlaplus/Examples`.* Tác giả viết spec này (2017) khi thiết kế tính năng điều phối checkpoint trên một cluster năm replica có primary được bầu[^ex-cp-intro]. Mô hình lỗi nêu thẳng trong README: replica crash rồi hồi phục, đường mạng giữa hai replica hỏng rồi hồi phục, và tốc độ trôi của thời gian khác nhau giữa các replica[^ex-cp-failure]. Trong spec, lỗi môi trường nằm trong những hành động riêng của `Next` (như `NodeFailure`, `NodeRecovery`, `NetworkFailure`)[^ex-cp-next], còn thời gian là bộ đếm `TimeoutCounter` tăng ở hành động `TriggerTimeout`[^ex-cp-timeout]. README ghi rằng TLC tìm ra một trace 12 bước cho một lỗi tinh vi do một "tối ưu hóa" gây ra[^ex-cp-12], và file `MCCheckpointCoordinationFailure.cfg` tái hiện trace đó[^ex-cp-failcfg]; manifest của repo ghi kết quả `safety failure` cho model Failure đó[^ex-cp-manifest]. (Đây là kết quả do repo ghi; chương này không chạy lại.)

<details><summary>Đi sâu: luật PlusCal cần nhớ, thứ TLA+ không làm được, và spec không phải code</summary>

- **Luật label.** Hai lệnh gán cho cùng một biến không được nằm trong cùng một bước: giữa chúng phải có label[^pm-assign-once]. Thân `with` không được chứa label[^pm-with-label]. `either` chọn nondeterministic một nhánh *thực thi được*[^pm-either]. Khi chạy bằng TLC, định danh process phải so sánh được với nhau[^pm-ids]; TLC coi model value so sánh được với mọi giá trị và luôn khác chúng[^ss-modelvalue] — nên một process tên chuỗi `"lock_service"` sống chung với một tập process là model value được.
- **Deadlock giả.** Khi mọi process đã xong, bộ dịch PlusCal tự thêm một hành động "mọi thứ đã hoàn tất, không có gì xảy ra" để TLC không coi là deadlock[^lt-terminating]. Nhưng một process kiểu `while TRUE do await … end while` luôn kẹt ở `await` khi hết việc, và TLC báo deadlock: sách mô tả tùy chọn `-deadlock` để tắt kiểm tra này[^ss-deadlock-opt]; trong file cấu hình, ví dụ cấu hình của spec Chameneos ở `tlaplus/Examples` dùng dòng `CHECK_DEADLOCK FALSE`[^ex-chameneos-cfg]. Các spec mục 2, 3, 5, 6 có process như vậy nên cần điều đó; spec mục 4 thì không, vì mọi process của nó đều kết thúc (suy luận của người soạn từ cấu trúc spec).
- **Thứ TLA+ không bắt được.** AWS nói rõ có hai loại vấn đề: lỗi logic (spec bắt được) và suy giảm hiệu năng "nổi lên" do vòng phản hồi, ví dụ một server chậm nhất thời làm client timeout, client retry, tải tăng, server càng chậm[^aws-emergent]. Họ viết rằng chưa biết cách khả thi để mô hình hóa một hệ thật nhằm dự đoán hành vi đó[^aws-emergent-no]. Vì vậy mục 6 chỉ mô hình hóa *tác dụng phụ lặp* của retry, không mô hình hóa *bão retry*.
- **Spec không phải code.** Câu hỏi thường gặp nhất ở AWS là làm sao biết code cài đặt đúng thiết kế đã kiểm; câu trả lời của họ là "không biết"[^aws-code]. Spec vẫn giúp: giúp làm đúng thiết kế, một bước cần thiết để làm đúng code[^aws-code-help]; và bất biến tìm được dùng để viết assertion trong code (suy luận của người soạn từ cùng đoạn). Chương 10 nói tiếp phần nối spec với code.
- **Bậc thang trừu tượng.** Cùng một hệ có thể được mô tả ở nhiều mức, mỗi mức phục vụ một mục đích như hiểu hệ quả của đồng thời mịn hơn[^aws-ladder]; học cách nối các mức ở [chương 07](07-refinement.md).

</details>

**Quay lại Q1:** giữ biến ở nơi dữ liệu có thể bất đồng, label ở ranh giới mà hệ thật bảo đảm nguyên tử, lỗi môi trường thành hành động, thời gian thành "có thể xảy ra bất cứ lúc nào" — rồi chỉ làm mịn khi trace bạn cần bị che mất.

## 2. Distributed lock với lease và fencing token (Q2)

> **Trả lời ngắn.** Lease là khóa có hạn: nó chỉ giải phóng khóa của node chết, không ngăn được chủ cũ vừa tỉnh dậy ghi tiếp[^kb-pause-lease]. Trong spec, "hết hạn" là một hành động xảy ra bất cứ lúc nào, và "pause" là việc một client không được chạy bước kế tiếp; hai thứ đó đủ để tạo ra trace ghi sai thứ tự. Fencing token sửa điều đó, nhưng chỉ khi *tài nguyên đích* tự kiểm tra token và từ chối ghi có token đi lùi[^kb-fencing-active]; trong spec dưới đây, việc kiểm và việc ghi nằm cùng một bước nguyên tử (lựa chọn mô hình của người soạn).

**Giải thích.**

Hình dung (nhận xét của người soạn): thẻ gửi xe có hạn, và barie nhớ số thẻ lớn nhất đã vào. Thẻ hết hạn là cách bãi thu hồi chỗ; barie nhớ số thẻ là cách chặn chiếc xe cầm thẻ cũ chen vào sau khi chỗ đã giao người khác. Cơ chế vận hành đầy đủ nằm ở [P12·08 mục 7–8](../12-he-thong-du-lieu/08-su-co-he-phan-tan.md); ở đây ta chỉ hỏi: *viết thành spec thì lỗi hiện ra thế nào?*

Kleppmann nhấn mạnh rằng ngay cả với một dịch vụ khóa hoàn hảo, đoạn code "lấy khóa → ghi → nhả khóa" vẫn hỏng nếu client bị pause quá hạn lease[^kb-perfect]. Ông cũng chỉ ra rằng thêm một lần kiểm tra hạn ngay trước khi ghi không sửa được, vì tiến trình có thể bị pause đúng giữa lần kiểm tra và lần ghi[^kb-nocheck] (lập luận đầy đủ ở [P12·08 mục 7](../12-he-thong-du-lieu/08-su-co-he-phan-tan.md)). Hai điều đó chính là thứ spec dưới đây sẽ làm lộ ra.

**Spec minh họa (PlusCal, chưa chạy TLC).** Ta mô hình hóa: một dịch vụ khóa hoàn hảo (một biến `holder`, một bộ đếm `next_token`), một tiến trình `lock_service` có thể làm hết hạn lease bất cứ lúc nào, nhiều client, và một kho lưu trữ ghi lại lịch sử các lần ghi được chấp nhận.

Nếu chưa quen ký hiệu trong spec: `/\` là "và", `\/` là "hoặc", `~` là phủ định, `/=` là "khác", `<<>>` là dãy rỗng, `\in`/`\notin`/`\union` là "thuộc"/"không thuộc"/"hợp"; chúng được giải nghĩa ở [chương 02](02-toan-nen-tang.md). Các từ khóa PlusCal (`process`, label, `await`, `either`, `with`, `while`, `self`, `pc`) ở [chương 06](06-pluscal.md).

```tla
---- MODULE LeaseFencing ----
EXTENDS Integers, Sequences, TLC

CONSTANTS Clients, NULL, Fencing
ASSUME NULL \notin Clients
ASSUME Fencing \in BOOLEAN

(*--algorithm lease_fencing
variables
  holder      = NULL,   \* người giữ lease theo dịch vụ khóa
  next_token  = 1,      \* token sẽ cấp cho lần acquire kế tiếp
  storage_max = 0,      \* token lớn nhất kho đã chấp nhận (chỉ để fencing)
  log         = <<>>;   \* lịch sử các lần ghi được chấp nhận (biến lịch sử)

define
  \* Không có ghi nào của token cũ xuất hiện SAU ghi của token mới hơn.
  NoStaleWrite ==
    \A i, j \in 1..Len(log) : i < j => log[i].tok <= log[j].tok
end define;

process lock_service = "lock_service"
begin
  Expire:                     \* lease có thể hết hạn bất cứ lúc nào
    while TRUE do
      await holder /= NULL;
      holder := NULL;
    end while;
end process;

process client \in Clients
variable my_tok = 0;
begin
  Acquire:                    \* một bước nguyên tử tại dịch vụ khóa
    await holder = NULL;
    holder := self;
    my_tok := next_token;
    next_token := next_token + 1;
  Write:                      \* một bước nguyên tử tại kho: kiểm token VÀ ghi
    if ~Fencing \/ my_tok >= storage_max then
      storage_max := my_tok;
      log := Append(log, [client |-> self, tok |-> my_tok]);
    end if;
end process;
end algorithm; *)
====
```

Cách đọc từng phần:

- *Process `lock_service`.* Nhánh `Expire` luôn bật khi có người giữ lease. Đây là sự trừu tượng hóa thời gian của mục 1: ta không đếm giờ, ta cho phép hết hạn xảy ra *bất cứ lúc nào*. Nhờ đó mọi lịch hết hạn thực tế đều là một lịch của spec (suy luận của người soạn: spec bao trùm thực tế, nên không bỏ sót hành vi thật).
- *Process `client`.* Hai label = hai bước nguyên tử. Giữa `Acquire` và `Write`, TLC được phép chạy bất kỳ process nào khác bao nhiêu bước tùy ý; đó chính là "pause" (GC, preempt, page fault) hay "gói ghi bị trễ trên mạng" — Kleppmann liệt kê cả hai làm nguyên nhân[^kb-network-delay]. Trong spec, hai nguyên nhân vật lý khác nhau co về *một* cơ chế: client không được chạy bước kế (nhận xét của người soạn).
- *Hằng `Fencing`.* `FALSE` là kho ngây thơ, ghi mọi thứ. `TRUE` là kho kiểm token: chỉ chấp nhận nếu token không nhỏ hơn token lớn nhất đã thấy, đúng như Kleppmann mô tả: kho phải đóng vai chủ động kiểm token và từ chối ghi có token đi lùi[^kb-fencing-active].
- *Biến lịch sử `log` và invariant `NoStaleWrite`.* Tính chất cần bảo vệ không phải "chỉ một client giữ lease" (dịch vụ khóa luôn bảo đảm điều đó vì `holder` là một biến), mà là "kho không áp dụng một ghi của lease cũ sau một ghi của lease mới hơn". Muốn phát biểu điều đó cần nhớ thứ tự ghi, nên ta thêm biến `log`. Biến chỉ phục vụ kiểm tra như vậy gọi là biến phụ/lịch sử (xem mục 7 về cái giá của nó).
- Khi chạy, tắt kiểm tra deadlock (mục 1) vì `lock_service` không bao giờ kết thúc, và chạy hai lần: `Fencing = FALSE` (kỳ vọng thấy vi phạm) rồi `Fencing = TRUE`.

**Trace tay khi `Fencing = FALSE` (suy luận của người soạn, chưa chạy TLC).** Với `Clients = {c1, c2}`:

1. `c1.Acquire`: `holder = c1`, `my_tok[c1] = 1`, `next_token = 2`.
2. `lock_service.Expire`: `holder = NULL`. Trong lúc đó `c1` đơn giản là không được chạy bước nào (pause).
3. `c2.Acquire`: `holder = c2`, `my_tok[c2] = 2`, `next_token = 3`.
4. `c2.Write`: kho chấp nhận; `log = <<[client |-> c2, tok |-> 2]>>`.
5. `c1.Write`: vì `~Fencing` đúng nên ghi vẫn được chấp nhận; `log = <<(c2, 2), (c1, 1)>>`.

Ở bước 5, với `i = 1`, `j = 2`: `log[1].tok = 2` không `<=` `log[2].tok = 1`, nên `NoStaleWrite` sai. Đây là đúng kịch bản mà Kleppmann vẽ: client thứ nhất tỉnh dậy sau khi lease hết hạn và vẫn ghi[^kb-pause-lease]. Hai client và một lần acquire mỗi client là đủ: bản vi phạm nhỏ nhất (suy luận của người soạn).

**Cùng trace khi `Fencing = TRUE` (suy luận, chưa chạy TLC).** Bước 4 cho `storage_max = 2`. Bước 5: `my_tok[c1] = 1 < 2` và `Fencing` đúng, nên điều kiện `~Fencing \/ my_tok >= storage_max` sai, `log` không đổi. Đây chính là cơ chế "kho nhớ đã xử lý token cao hơn nên từ chối token thấp hơn"[^kb-fencing].

```mermaid
sequenceDiagram
  participant C1 as client c1
  participant L as lock_service
  participant C2 as client c2
  participant S as kho storage
  C1->>L: Acquire, nhận token 1
  Note over C1: pause: c1 không có bước nào
  L->>L: Expire (holder = NULL)
  C2->>L: Acquire, nhận token 2
  C2->>S: Write token 2, được chấp nhận
  Note over S: storage_max = 2
  C1->>S: Write token 1 (tỉnh dậy)
  alt Fencing = FALSE
    S-->>C1: chấp nhận, NoStaleWrite sai
  else Fencing = TRUE
    S-->>C1: từ chối vì 1 nhỏ hơn 2
  end
```

*Vì sao fencing đúng, lập luận quy nạp (suy luận của người soạn).* Khi `Fencing = TRUE`, ta có bất biến phụ: `storage_max` bằng token của phần tử cuối trong `log` (hoặc 0 nếu `log` rỗng) và `log` không giảm. Bước `Acquire` và `Expire` không đụng tới `log` hay `storage_max`. Bước `Write` chỉ thêm phần tử khi `my_tok >= storage_max`, tức không nhỏ hơn token cuối; sau đó `storage_max = my_tok` bằng token cuối mới. Nếu bị từ chối thì không có gì đổi. Khởi đầu (`log` rỗng, `storage_max = 0`) thỏa bất biến. Vậy `NoStaleWrite` đúng ở mọi state — với điều kiện duy nhất là *kiểm token và ghi là một bước nguyên tử* (đi sâu, bên dưới). Lưu ý: lập luận này không dùng tới tính đơn điệu của `next_token`; nó chỉ chứng minh `log` không giảm theo token. Chính tính đơn điệu của `next_token` (giả định về dịch vụ khóa, xem (c)) nối "token nhỏ hơn" với "lease cũ hơn", nhờ đó `NoStaleWrite` mới có nghĩa "không áp dụng ghi của lease cũ sau ghi của lease mới".

<details><summary>Đi sâu: thêm bước kiểm tra, tách kiểm tra khỏi ghi, và giả định ẩn của spec</summary>

**(a) Thêm một label `Check` giữa `Acquire` và `Write` không sửa được.** Giả sử client tự kiểm tra còn giữ lease trước khi ghi (spec `Fencing = FALSE`):

```tla
  Check:                      \* client tự kiểm tra: "mình còn giữ lease không?"
    await holder = self;
  Write:
    \* như cũ
```

Trace (suy luận của người soạn, chưa chạy TLC): `c1.Acquire`; `c1.Check` thành công vì `holder = c1`; rồi `c1` bị pause; `Expire`; `c2.Acquire`, `c2.Check`, `c2.Write` (`log = <<(c2, 2)>>`); `c1.Write` chạy tiếp và ghi `(c1, 1)` sau `(c2, 2)`, `NoStaleWrite` sai. Spec tái hiện đúng nhận xét của Kleppmann: kiểm tra hạn ngay trước khi ghi không sửa được vì pause có thể rơi vào giữa kiểm tra và ghi[^kb-nocheck]. Ý nghĩa tổng quát (nhận xét của người soạn): "check rồi act" qua hai label luôn là một cửa sổ cho interleaving; chỉ có một bước nguyên tử *ở nơi giữ dữ liệu* mới đóng được cửa sổ đó.

**(b) Tách "kiểm token" khỏi "ghi" ở kho cũng hỏng.** Nếu kho cài đặt thành hai label:

```tla
process client \in Clients
variables my_tok = 0, ok = FALSE;
begin
  Acquire:
    \* như cũ
  CheckToken:
    ok := my_tok >= storage_max;
  Apply:
    if ok then
      storage_max := my_tok;
      log := Append(log, [client |-> self, tok |-> my_tok]);
    end if;
end process;
```

thì trace (suy luận, chưa chạy TLC) là: `c1.Acquire` (token 1); `c1.CheckToken` đặt `ok = TRUE` vì `storage_max = 0`; `c1` pause; `Expire`; `c2.Acquire` (token 2); `c2.CheckToken`, `c2.Apply` → `storage_max = 2`, `log = <<(c2, 2)>>`; `c1.Apply` chạy với `ok = TRUE` cũ và ghi `(c1, 1)`, đồng thời hạ `storage_max` về 1. `NoStaleWrite` sai. Bài học nối với code (xem "Liên hệ với Rust"): token check và ghi phải cùng một critical section hoặc cùng một câu lệnh có điều kiện ở kho.

**(c) Giả định ẩn: `next_token` tăng đơn điệu qua mọi lần failover của dịch vụ khóa.** Trong spec đó là một biến duy nhất, không bao giờ hỏng. Khi bàn về việc thêm fencing token cho Redlock, Kleppmann nhận xét nhiều khả năng cần cả một thuật toán consensus chỉ để sinh token[^kb-fencing-consensus]; giả định tăng đơn điệu qua failover trong spec của ta là phần chưa được kiểm chứng (nhận xét của người soạn). Cách làm đúng tinh thần AWS: viết giả định này thành *đặc tính môi trường* tường minh trong spec[^aws-env-props], rồi nếu cần thì tinh chỉnh bằng một spec mức thấp hơn ([chương 07](07-refinement.md)).

**(d) Thời gian tường minh.** Có thể thêm biến `now` đếm tới một `MaxTime` hữu hạn và chỉ cho `Expire` bật khi `now` đã qua hạn, theo tinh thần biến `now` của Lamport[^ss-now]. Thêm điều đó không làm lỗi biến mất (pause vẫn là "không bước"), nhưng làm nhân state lên với số giá trị của `now` (suy luận của người soạn). Với lý do ở mục 1 — safety không dựa vào thời gian — bản nondeterministic là lựa chọn mặc định tốt hơn.

**(e) Liên hệ rộng.** Các ví dụ thực tế về lease có hoặc không có fencing (Kubernetes `leaderelection`, Chubby, etcd) đã được dẫn nguồn ở [P12·08 mục 8](../12-he-thong-du-lieu/08-su-co-he-phan-tan.md). Số thế hệ (term) trong Raft là cùng họ ý tưởng với fencing token; xem [P12·09](../12-he-thong-du-lieu/09-consistency-va-consensus.md) (nhận xét của người soạn).

</details>

**Quay lại Q2:** hết hạn = hành động tự do, pause = client không được chạy; hai điều đó đủ để lease một mình ghi sai thứ tự, và chỉ kiểm token *ở kho, trong cùng bước nguyên tử với việc ghi* mới đóng được lỗi.

## 3. Hàng đợi at-least-once và consumer idempotent (Q3)

> **Trả lời ngắn.** Hàng đợi at-least-once chỉ hứa không mất tin, không hứa không trùng: nếu consumer xử lý xong nhưng chưa kịp ack thì tin sẽ được giao lại[^k-ack-problem]. Trong spec, "mất ack" là một nhánh nondeterministic và "hết hạn visibility" là một hành động của broker; kết quả là tác dụng phụ có thể chạy hai lần. Consumer idempotent sửa được, miễn là bản ghi "đã xử lý" nằm trong *cùng một bước nguyên tử* với tác dụng phụ.

**Giải thích.**

Hình dung (nhận xét của người soạn): thợ giao hàng gõ cửa đưa gói hàng; bạn ký nhận nhưng tờ biên nhận bị gió thổi bay trước khi tới tay hãng; hãng không thấy biên nhận nên cử người giao lần nữa. Muốn không nhận hai lần, nhà bạn phải có sổ ghi "đã nhận gói số X", và việc *mở gói* với việc *ghi sổ* không được tách rời.

Cơ chế thực tế: khi dùng manual acknowledgement, RabbitMQ requeue mọi delivery chưa ack khi channel hoặc connection đóng[^rmq-requeue], nên consumer phải chuẩn bị xử lý redelivery và thiết kế idempotent[^rmq-idem]. Phần lý thuyết (idempotency, dedup cần bộ nhớ bền) ở [P12·11 mục 4](../12-he-thong-du-lieu/11-stream-processing.md) và [P12·12 mục 4–5](../12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md); dưới đây là spec.

*Chọn cách biểu diễn hàng đợi.* Learntla nêu hai cách. Hàng đợi "phá hủy" (đọc xong là mất khỏi dãy) dễ viết, nhưng không phát biểu được tính chất dựa vào lịch sử, như "một tin chưa từng được xếp hàng hai lần"[^lt-queue-mutable]. Hàng đợi bất biến kèm con trỏ đọc `i` giữ nguyên lịch sử nhưng khó viết hơn[^lt-queue-immutable]. Mỗi thông điệp nên có `id` để phân biệt các tin cùng nội dung[^lt-queue-id]. Ở đây ta chỉ cần *tập* các id và một *trạng thái* cho từng id: `ready`, `inflight`, `acked`; thứ tự giao bị bỏ có chủ ý (mục "Đi sâu").

**Spec minh họa (PlusCal, chưa chạy TLC).**

```tla
---- MODULE AtLeastOnce ----
EXTENDS Integers, TLC

CONSTANTS Msgs, Consumers, NULL, Dedup, MaxCrashes
ASSUME NULL \notin Msgs
ASSUME Dedup \in BOOLEAN
ASSUME MaxCrashes \in Nat

(*--algorithm at_least_once
variables
  status  = [m \in Msgs |-> "ready"],   \* ở broker: ready | inflight | acked
  effects = [m \in Msgs |-> 0],         \* tác dụng phụ đã áp dụng cho từng tin
  seen    = {},                         \* bảng dedup của consumer, cùng DB với effects
  crashes = 0;                          \* ngân sách crash đã dùng

define
  AtMostOneEffect == \A m \in Msgs : effects[m] <= 1
end define;

process broker = "broker"
begin
  Redeliver:                            \* visibility timeout: tin inflight quay lại ready
    while TRUE do
      with m \in {x \in Msgs : status[x] = "inflight"} do
        status[m] := "ready";
      end with;
    end while;
end process;

process consumer \in Consumers
variable cur = NULL;
begin
  Receive:
    while TRUE do
      with m \in {x \in Msgs : status[x] = "ready"} do
        status[m] := "inflight";
        cur := m;
      end with;
  Handle:                               \* một transaction: tác dụng phụ + bản ghi dedup
      if ~(Dedup /\ cur \in seen) then
        effects[cur] := effects[cur] + 1;
        seen := seen \union {cur};
      end if;
  Ack:
      either
        status[cur] := "acked";
      or
        await crashes < MaxCrashes;     \* crash/mất ack: broker sẽ giao lại
        crashes := crashes + 1;
      end either;
    end while;
end process;
end algorithm; *)
====
```

Chú giải:

- `broker.Redeliver` không có điều kiện thời gian: tin `inflight` có thể quay lại `ready` bất cứ lúc nào. Nó gộp hai chuyện thật — consumer crash và consumer chỉ chậm — vào một hành động (nhận xét của người soạn). Mỗi lần `with` chọn một phần tử của tập rỗng thì bước đó không thực thi được[^lt-with-empty].
- `Handle` là *một* label: kiểm `seen`, tăng `effects`, ghi `seen` cùng lúc. Nó mô hình hóa một transaction DB bao quanh "áp dụng tác dụng phụ + ghi bản ghi đã xử lý".
- Nhánh `or` của `Ack` là mất ack hoặc consumer chết sau khi làm xong. Nó có ngân sách `MaxCrashes`, để ta nói "tối đa K lần crash" thay vì crash vô hạn (mục 7).
- `AtMostOneEffect` là tính chất "tác dụng phụ không chạy quá một lần cho mỗi tin" — ta *muốn* nó đúng khi `Dedup = TRUE` và *kỳ vọng* nó sai khi `Dedup = FALSE`.

**Trace tay khi `Dedup = FALSE`, một consumer, `MaxCrashes >= 1` (suy luận của người soạn, chưa chạy TLC).** Với `Msgs = {m1}`, `Consumers = {c1}`:

1. `c1.Receive`: `status[m1] = "inflight"`, `cur = m1`.
2. `c1.Handle`: `effects[m1] = 1`.
3. `c1.Ack`, nhánh `or`: `crashes = 1`, `status[m1]` vẫn `"inflight"`; vòng lặp quay lại `Receive`.
4. `broker.Redeliver`: `status[m1] = "ready"`.
5. `c1.Receive` rồi `c1.Handle`: `effects[m1] = 2`, `AtMostOneEffect` sai.

Một tin, một consumer, một lần crash là đủ — đúng tình huống mà tài liệu thiết kế Kafka nêu cho các hệ thống nhắn tin dùng xác nhận (ack): consumer xử lý xong nhưng hỏng trước khi gửi xác nhận thì tin bị tiêu thụ hai lần (Kafka tự xử lý khác, bằng vị trí đọc của consumer)[^k-ack-problem].

**Khi `Dedup = TRUE` (suy luận, chưa chạy TLC).** `Handle` chỉ tăng `effects[cur]` khi `cur \notin seen` và đồng thời thêm `cur` vào `seen`; vậy bất biến quy nạp là: với mọi `m`, `effects[m] = 1` nếu `m \in seen` và `effects[m] = 0` nếu không. Bất biến này kéo theo `AtMostOneEffect`, và mọi bước khác không đụng `effects`/`seen`. Nó đúng *vì* hai biến được cập nhật trong cùng một label.

**Tách dedup khỏi tác dụng phụ thì hỏng.** Thay `Handle` bằng hai label (chưa chạy TLC):

```tla
  Effect:
    if ~(Dedup /\ cur \in seen) then
      effects[cur] := effects[cur] + 1;
    end if;
  Record:
    seen := seen \union {cur};
```

Trace với hai consumer `c1`, `c2` (suy luận): `c1.Receive` (m1 inflight); `c1.Effect` (`effects[m1] = 1`, `seen` vẫn rỗng); `c1` pause; `broker.Redeliver` (m1 ready); `c2.Receive`; `c2.Effect` thấy `m1 \notin seen` nên `effects[m1] = 2` — `AtMostOneEffect` sai, dù `Dedup = TRUE` và không có crash nào. Đây là lý do ở thực tế bản ghi dedup và thay đổi dữ liệu phải chung transaction DB.

<details><summary>Đi sâu: thứ tự, at-most-once, liveness, ack muộn</summary>

- **Thứ tự.** Dùng tập bỏ thông tin về thứ tự giao. Nếu tính chất cần thứ tự (ví dụ "sự kiện của cùng một khóa được xử lý theo thứ tự gửi"), hàng đợi phải là dãy hoặc hàng đợi bất biến kèm chỉ số, và nhớ rằng hàng đợi bất biến có thể làm mô hình không còn hữu hạn nếu đọc mãi[^lt-queue-warning].
- **At-most-once.** Learntla gợi ý mô hình giao-tối-đa-một-lần bằng cách giao tin cho một *tập con* người nhận hợp lệ: phần nằm ngoài tập con là không nhận được[^lt-amo]. Trong spec trên, đảo thứ tự để `Ack` chạy trước `Handle` cho ngữ nghĩa tương ứng: nếu consumer chết giữa hai bước thì tác dụng phụ mất; tài liệu Kafka gọi đó là at-most-once ở phía consumer[^kafka-consumer-amo]. Muốn thấy điều đó trong spec cần thêm một nhánh crash giữa hai label, và tính chất bị vi phạm lúc đó là "tin đã `acked` thì `effects >= 1`" (suy luận của người soạn, chưa chạy TLC).
- **Liveness.** "Mọi tin rồi sẽ được ack" cần `fair process` hoặc khai fairness, cộng ngân sách crash hữu hạn; nếu không, nhánh crash lặp mãi là một hành vi hợp lệ không bao giờ ack. Chi tiết ở [chương 05](05-liveness-va-fairness.md). Chương này không khẳng định liveness của spec trên.
- **Ack của consumer chậm.** Spec cho phép một consumer chậm ack một tin mà consumer khác đang xử lý; đó là hành vi của hàng đợi dùng visibility timeout (nhận xét của người soạn). Nếu hệ của bạn có quy tắc khác (ví dụ ack kèm token của lần giao), viết lại `Ack` theo quy tắc đó.
- **Outbox ở đầu kia.** Nếu *producer* cũng cần bảo đảm "ghi DB và phát tin cùng sống cùng chết", đó là mục 5.

</details>

**Quay lại Q3:** at-least-once nghĩa là spec của bạn *phải* cho phép giao lặp; consumer idempotent chỉ đúng khi bản ghi dedup và tác dụng phụ là một bước nguyên tử — tách hai label ra là trace trùng lặp xuất hiện.

## 4. Cache-aside và cuộc đua stale cache (Q4)

> **Trả lời ngắn.** Với cache-aside, ứng dụng đọc cache, nếu trượt thì đọc DB rồi đặt vào cache; khi ghi thì ghi DB rồi xóa mục trong cache[^az-order]. Cuộc đua: một reader đọc DB giá trị cũ, writer cập nhật DB và xóa cache, rồi reader mới đặt giá trị cũ vào cache — cache giữ giá trị cũ cho tới khi hết hạn. Facebook gọi đó là *stale set*[^fb-stale] và sửa bằng lease token gắn với khóa cache[^fb-lease].

**Giải thích.**

Hình dung (nhận xét của người soạn): bạn đi xem bảng giá ở quầy, chép giá cũ vào giấy nhớ, đang đi ra quầy dán thì quầy trưởng đổi giá và xé bảng cũ; bạn không biết chuyện đó và vẫn dán tờ giấy giá cũ lên. Từ đó mọi người tin tờ giấy của bạn.

Tài liệu Azure nói rõ giữa lần ghi và lần đọc kế tiếp, reader có thể trượt cache hoặc thoáng thấy dữ liệu cũ[^az-stale-window] — nghĩa là *một số* độ cũ là chủ ý của mẫu thiết kế. Vì vậy tính chất đúng không thể là "cache luôn bằng DB" (state nào cũng thế), mà là: **khi hệ đã yên (quiescent), cache hoặc trống hoặc bằng DB**. Cache chứa giá trị cũ *mãi mãi* mới là lỗi, và "mãi mãi" của cache thật bị chặn bởi TTL (không mô hình hóa ở đây) (nhận xét của người soạn).

**Spec minh họa (PlusCal, chưa chạy TLC).** Một writer ghi DB rồi xóa cache; mỗi reader đọc một lần theo cache-aside. `epoch` mô phỏng lease token: tăng mỗi lần cache bị xóa.

```tla
---- MODULE CacheAside ----
EXTENDS Integers, TLC

CONSTANTS Readers, NULL, UseLease
ASSUME UseLease \in BOOLEAN

(*--algorithm cache_aside
variables
  db    = 0,
  cache = NULL,   \* NULL = trống; ngược lại là giá trị đang được cache
  epoch = 0;      \* số lần cache bị xóa (đóng vai lease token)

define
  Quiescent    == pc["writer"] = "Done" /\ \A r \in Readers : pc[r] = "Done"
  CacheNotStale == Quiescent => (cache = NULL \/ cache = db)
end define;

process writer = "writer"
begin
  WriteDB:
    db := 1;
  Invalidate:
    cache := NULL;
    epoch := epoch + 1;
end process;

process reader \in Readers
variables v = NULL, tok = 0, hit = FALSE;
begin
  ReadCache:                       \* một thao tác tại cache; nhận token nếu trượt
    if cache /= NULL then
      hit := TRUE;
    else
      tok := epoch;
    end if;
  ReadDB:
    if ~hit then
      v := db;
    end if;
  FillCache:                       \* với lease: chỉ đặt nếu token còn hiệu lực
    if ~hit /\ (~UseLease \/ epoch = tok) then
      cache := v;
    end if;
end process;
end algorithm; *)
====
```

Chú giải: `ReadCache` là *một* label vì nó là một thao tác tại cache (kèm token); `ReadDB` và `FillCache` là hai label tách rời vì giữa "đọc DB" và "đặt cache" tiến trình khác chen được — đó là cửa sổ lỗi. Writer có hai label: ghi DB trước, xóa cache sau, đúng thứ tự khuyến nghị[^az-order].

**Trace tay khi `UseLease = FALSE`, `Readers = {r1}` (suy luận của người soạn, chưa chạy TLC).**

1. `r1.ReadCache`: `cache = NULL` nên trượt; `tok = epoch = 0`.
2. `r1.ReadDB`: `v = db = 0`.
3. `writer.WriteDB`: `db = 1`.
4. `writer.Invalidate`: `cache = NULL` (vốn đã trống), `epoch = 1`.
5. `r1.FillCache`: `~hit` đúng và `~UseLease` đúng nên `cache = 0`.

Mọi process đã `Done` (state yên); `cache = 0` khác `NULL`, và khác `db = 1`; `CacheNotStale` sai. Đây là stale set: một cập nhật cache bị sắp xếp lại sau lần xóa, đúng như Facebook mô tả (các cập nhật đồng thời vào memcache bị đảo thứ tự)[^fb-stale].

```mermaid
sequenceDiagram
  participant R as reader r1
  participant K as cache
  participant D as DB
  participant W as writer
  R->>K: ReadCache, trượt (tok = 0)
  R->>D: ReadDB, được 0
  W->>D: WriteDB, db = 1
  W->>K: Invalidate, cache trống, epoch = 1
  R->>K: FillCache, đặt 0 (cache cũ)
  Note over K,D: yên rồi: cache = 0, db = 1
```

**Khi `UseLease = TRUE` (suy luận, chưa chạy TLC).** Ở bước 5, `epoch = 1` khác `tok = 0` nên `FillCache` không đặt gì; cache vẫn trống. Facebook mô tả đúng ý đó: memcached cấp token ràng buộc với khóa khi miss; client trình token khi set; memcached có thể từ chối nếu token bị vô hiệu bởi một lệnh delete[^fb-lease-verify], tương tự load-link/store-conditional[^fb-ll-sc]. Lập luận tổng quát cho `CacheNotStale` khi có lease (suy luận): nếu `FillCache` đặt giá trị khi `epoch = tok` thì `Invalidate` chưa chạy giữa `ReadCache` và `FillCache`. Hoặc `Invalidate` đã chạy *trước* `ReadCache` — khi đó `WriteDB` đã chạy trước nó, nên `ReadDB` thấy `db = 1` — hoặc nó chưa chạy và sẽ chạy sau, xóa cache trước khi writer `Done`. Cả hai trường hợp state yên đều có `cache = NULL` hoặc `cache = 1 = db`.

**Đảo thứ tự của writer: lease không cứu được (suy luận, chưa chạy TLC).** Nếu writer xóa cache *trước* rồi mới ghi DB (hoán đổi hai label), trace là: `writer.Invalidate` (`epoch = 1`); `r1.ReadCache` trượt, `tok = 1`; `r1.ReadDB`, `v = 0`; `writer.WriteDB` (`db = 1`); `r1.FillCache` thấy `epoch = tok = 1` nên đặt `cache = 0`. State yên, `cache = 0 ≠ db = 1`. Tài liệu Azure nói đúng điều này bằng lời: thứ tự các bước quan trọng; xóa cache trước khi cập nhật kho dữ liệu mở ra một cửa sổ nhỏ để client đọc bản cũ và đặt lại vào cache[^az-bad-order]. Một lệnh sửa mẫu hình (lease) không bù được một lỗi về thứ tự thao tác (nhận xét của người soạn).

<details><summary>Đi sâu: những gì spec này bỏ, thundering herd, và nhiều writer</summary>

- **Thundering herd.** Lease của Facebook giải hai vấn đề: stale set và thundering herd (một khóa nóng liên tục bị ghi làm các lần đọc đổ xuống đường tốn kém)[^fb-herd]. Spec chỉ bắt stale set; thundering herd là vấn đề tải, ngoài tầm safety/liveness đơn giản (xem lại giới hạn ở mục 1).
- **Nhiều writer.** Spec có một writer. Với hai writer, DB và cache có thể bị hai lần ghi xen kẽ theo thứ tự khác nhau ở hai nơi; đó là bài toán khác (bất biến "cache theo kịp DB" cần một số phiên bản). Muốn kiểm, thêm một writer và biến phiên bản — chưa viết ở đây vì chưa có nguồn để đối chiếu.
- **TTL.** Azure khuyên chính sách hết hạn phải khớp mẫu truy cập; hết hạn quá ngắn thì ứng dụng cứ phải nạp lại[^az-lifetime]. TTL là yếu tố thời gian: trong spec nó là một hành động "xóa mục cache bất cứ lúc nào"; thêm hành động đó làm stale set *tự lành*, nhưng không làm trace trên biến mất — chỉ đổi "mãi mãi" thành "đến khi TTL chạy" (nhận xét của người soạn).
- **Write-through.** Cập nhật cả DB lẫn cache trong cùng một thao tác ghi để reader thấy giá trị mới ngay sau khi ghi thành công, khác với cache-aside[^az-writethrough]. Cái giá (nhận xét của người soạn): hai nơi phải ghi cùng lúc, nên nó thừa hưởng bài toán dual write ở mục 5.
- **Liên hệ P12.** Cache là *dữ liệu dẫn xuất*; xem [P12·12 mục 1](../12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md).

</details>

**Quay lại Q4:** vì "cache luôn bằng DB" không phải mục tiêu của cache-aside, tính chất đúng là "khi yên, cache trống hoặc bằng DB"; cuộc đua là reader giữ giá trị cũ qua một lần ghi + xóa, và lease token (hoặc sửa đúng thứ tự thao tác) là thứ chặn nó.

## 5. Transactional outbox (Q5)

> **Trả lời ngắn.** Dual write — ghi DB rồi gửi tin, hai việc riêng — luôn có một khoảng hở: sập giữa hai việc thì DB đổi mà tin biến mất[^msio-fail]. Outbox ghi tin vào một bảng *cùng transaction* với dữ liệu nghiệp vụ; một tiến trình riêng gửi tin đi[^msio-solution], nên tin được gửi nếu và chỉ nếu transaction commit[^msio-benefit]. Cái giá: relay có thể gửi một tin nhiều lần[^msio-dup], nên phía nhận phải idempotent (mục 3).

**Giải thích.**

Hình dung (nhận xét của người soạn): bạn vừa ký hợp đồng (ghi DB) và cần báo cho kế toán (gửi tin). Nếu *sau khi ký* bạn mới đi báo, mà bạn ngất giữa đường thì kế toán không bao giờ biết. Outbox là: *trong cùng lúc ký*, bạn kẹp một tờ "cần báo kế toán" vào hồ sơ; một nhân viên khác đọc tờ đó và đi báo — nếu họ ngất rồi đi báo lại thì kế toán có thể nghe hai lần, nhưng không bao giờ không nghe.

Mô hình thứ nhất: dual write (chưa chạy TLC).

```tla
---- MODULE DualWrite ----
CONSTANT Orders

(*--algorithm dual_write
variables
  db_orders = {},    \* các order đã commit trong DB
  published = {};    \* các sự kiện broker đã nhận

define
  Quiescent   == \A o \in Orders : pc[o] = "Done"
  NoLostEvent == Quiescent => db_orders \subseteq published
end define;

process svc \in Orders
begin
  Commit:
    db_orders := db_orders \union {self};
  Publish:
    either
      published := published \union {self};
    or
      skip;          \* service chết giữa commit và publish
    end either;
end process;
end algorithm; *)
====
```

Trace tay (suy luận của người soạn, chưa chạy TLC): `svc[o1].Commit` (`db_orders = {o1}`); `svc[o1].Publish` chọn nhánh `skip`; `svc[o1]` xong. State yên, `db_orders = {o1}` còn `published = {}`, nên `NoLostEvent` sai: một order đã commit mà không có sự kiện nào. Với một order là đủ. Đảo thứ tự (publish trước, commit sau, với một nhánh transaction không commit) cho lỗi ngược lại: sự kiện về một order chưa từng được commit — vi phạm một tính chất kiểu `published \subseteq db_orders` (không phải `NoLostEvent`; suy luận của người soạn). Hai lỗi này đúng là hai kịch bản mà tài liệu microservices.io nêu: gửi tin giữa transaction thì không có gì bảo đảm transaction sẽ commit; gửi sau commit thì không bảo đảm khỏi sập trước khi gửi[^msio-fail].

Mô hình thứ hai: outbox (chưa chạy TLC).

```tla
---- MODULE Outbox ----
EXTENDS Integers, TLC
CONSTANTS Orders, NULL, MaxCrashes
ASSUME NULL \notin Orders
ASSUME MaxCrashes \in Nat

(*--algorithm outbox
variables
  db_orders = {},                       \* bảng nghiệp vụ
  outbox    = {},                       \* bảng outbox, CÙNG database với db_orders
  sent      = {},                       \* các dòng outbox đã đánh dấu đã gửi
  published = [o \in Orders |-> 0],     \* broker đã nhận sự kiện của o bao nhiêu lần
  crashes   = 0;

define
  Quiescent   == \A o \in Orders : pc[o] = "Done"
  Atomic      == db_orders = outbox
  NoPhantom   == \A o \in Orders : published[o] >= 1 => o \in db_orders
  NoLostEvent == (Quiescent /\ outbox = sent) => \A o \in db_orders : published[o] >= 1
  AtMostOnce  == \A o \in Orders : published[o] <= 1      \* KỲ VỌNG SAI
end define;

process svc \in Orders
begin
  Txn:                                   \* một transaction: dòng nghiệp vụ + dòng outbox
    db_orders := db_orders \union {self};
    outbox    := outbox \union {self};
end process;

process relay = "relay"
variable cur = NULL;
begin
  Poll:
    while TRUE do
      with o \in outbox \ sent do
        cur := o;
      end with;
  Publish:
      published[cur] := published[cur] + 1;
  MarkSent:
      either
        sent := sent \union {cur};
      or
        await crashes < MaxCrashes;      \* relay chết trước khi đánh dấu
        crashes := crashes + 1;
      end either;
    end while;
end process;
end algorithm; *)
====
```

Các tính chất và vì sao chúng là loại tính chất đó (suy luận của người soạn, chưa chạy TLC):

- `Atomic` (`db_orders = outbox`) đúng ở mọi state: hai biến chỉ đổi trong cùng label `Txn`. Đây là "bất biến lõi" của outbox; mọi tính chất còn lại dựa vào nó.
- `NoPhantom` đúng: `published[o]` chỉ tăng khi `cur = o` với `o \in outbox`, và `outbox = db_orders`.
- `NoLostEvent` đúng: `sent` chỉ thêm `cur` *sau* `Publish`, nên `o \in sent` kéo theo `published[o] >= 1`; khi mọi `svc` xong và `outbox = sent`, mọi order trong `db_orders = outbox` đã thuộc `sent`. Lưu ý vế trái `outbox = sent`: đây là tính chất safety "nếu relay đã gửi hết thì không mất", chứ không phải "relay rồi sẽ gửi hết" — cái sau là liveness, cần `relay` được chạy công bằng và ngân sách crash hữu hạn ([chương 05](05-liveness-va-fairness.md)); chương này không khẳng định nó.
- `AtMostOnce` **sai**. Trace (cần `MaxCrashes >= 1`): `svc[o1].Txn`; `relay.Poll` (`cur = o1`); `relay.Publish` (`published[o1] = 1`); `relay.MarkSent` nhánh `or` (`crashes = 1`, `o1` chưa vào `sent`); vòng lặp: `Poll` chọn lại `o1`; `Publish` (`published[o1] = 2`). Đó chính là nhược điểm microservices.io ghi: message relay có thể phát một tin nhiều lần[^msio-dup]; AWS cũng khuyên làm consumer idempotent bằng cách theo dõi các tin đã xử lý[^aws-dup].

Ngân sách `MaxCrashes` làm mô hình hữu hạn. Nếu bỏ nó, relay có thể chết và phát lại mãi: `published[o]` không có trần, TLC sẽ không bao giờ kết thúc (xem "unbound model" ở mục 7). Ngoài ra `relay` kẹt ở `with` khi không còn dòng nào chưa gửi, nên cũng như mục 1, cần tắt kiểm deadlock.

<details><summary>Đi sâu: thứ tự, CDC, và ghép với consumer idempotent</summary>

- **Thứ tự gửi.** AWS lưu ý outbox nên gửi tin theo cùng thứ tự mà service cập nhật DB[^aws-order]. `outbox` là tập nên không biểu diễn thứ tự; nếu thứ tự là một yêu cầu, thêm một số thứ tự cho mỗi dòng và một tính chất "thứ tự publish không đảo so với thứ tự commit" (chưa viết ở đây).
- **Relay bằng CDC.** Debezium có transform riêng cho outbox: connector chỉ cần bắt thay đổi trong bảng outbox[^debezium-outbox-capture]. Trong spec, "relay đọc outbox rồi publish" và "CDC bắt outbox rồi publish" cùng một khuôn; khác nhau ở chỗ đánh dấu `sent`. Xem [P12·12 mục 6](../12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md).
- **Ghép với mục 3.** Hai spec ghép được: `published[o]` của outbox là nguồn của hàng đợi at-least-once ở mục 3, `Handle` có dedup theo `o`. Kết quả mong đợi (suy luận của người soạn) là hiệu ứng "tác dụng phụ đúng một lần" ở cuối chuỗi dù có hai nơi trùng lặp.
- **Trừu tượng hóa broker.** Spec coi broker là một bộ đếm: đủ cho tính chất về mất/trùng; không đủ để bàn về thứ tự hay phân vùng.

</details>

**Quay lại Q5:** outbox đổi "hai việc rời nhau" thành "một transaction + một relay lặp lại được"; bất biến lõi là `db_orders = outbox`, và trùng lặp là cái giá cố ý phải trả — bắt bằng idempotent ở phía nhận.

## 6. Retry và timeout (Q6)

> **Trả lời ngắn.** Timeout không cho client biết yêu cầu đã chạy hay chưa: một lần timeout hay lỗi không nhất thiết có nghĩa là tác dụng phụ chưa xảy ra[^abl-sideeffects]. Vì vậy retry một thao tác có tác dụng phụ mà không idempotent sẽ làm tác dụng phụ lặp[^abl-idem]. Spec cho thấy lỗi ở mức logic và chỉ ra chỗ sửa (idempotency key phía server); còn *bão retry* làm quá tải nằm ngoài tầm spec[^aws-emergent-no].

**Giải thích.**

Hình dung (nhận xét của người soạn): bạn bấm nút "thanh toán", màn hình quay mãi; bạn bấm lại. Nếu hệ thống không có cách nhận ra "đây vẫn là lần bấm đó", bạn bị trừ tiền hai lần.

Kleppmann mô tả đúng tình huống: client gửi RPC mà không nhận hồi đáp thì không biết server đã nhận và xử lý chưa; gửi lại có thể làm thao tác chạy nhiều lần (như trừ thẻ hai lần)[^kleppmann-rpc]. Phía API, một idempotency key là khóa duy nhất mà client sinh ra để server nhận ra các lần retry của cùng yêu cầu[^stripe-key]; cơ chế và cách server lưu key ở [P12·12 mục 4](../12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md), phía Rust ở [P7·09 mục 2](../07-io-async/09-async-trong-production.md). Chương này chỉ dựng lại cơ chế đó thành spec.

**Spec minh họa (PlusCal, chưa chạy TLC).** Một client gửi một yêu cầu logic, tối đa `MaxRetries` lần; mỗi lần gửi là một *attempt* mang số thứ tự. Mạng là hai tập: yêu cầu đang bay và hồi đáp đang bay. Timeout là nhánh `skip` — client bỏ cuộc đợi và gửi lại.

```tla
---- MODULE RetryTimeout ----
EXTENDS Integers, TLC
CONSTANTS MaxRetries, Idem
ASSUME MaxRetries \in Nat
ASSUME Idem \in BOOLEAN

(*--algorithm retry_timeout
variables
  req_net   = {},      \* các attempt đang bay tới server
  resp_net  = {},      \* hồi đáp đang bay về client
  applied   = 0,       \* số lần server THỰC SỰ áp dụng thao tác (ví dụ trừ tiền)
  done      = FALSE,   \* server nhớ "request id này đã chạy" (idempotency key)
  client_ok = FALSE;   \* client tin rằng thao tác thành công

define
  AppliedAtMostOnce    == applied <= 1
  OkImpliesApplied     == client_ok => applied >= 1
end define;

process client = "client"
variable attempt = 0;
begin
  Send:
    while ~client_ok /\ attempt < MaxRetries do
      attempt := attempt + 1;
      req_net := req_net \union {attempt};
  Wait:
      either
        await resp_net /= {};     \* nhận được hồi đáp
        client_ok := TRUE;
      or
        skip;                     \* timeout: không thấy hồi đáp, gửi lại
      end either;
    end while;
end process;

process server = "server"
begin
  Serve:
    while TRUE do
      with a \in req_net do
        req_net := req_net \ {a};
        if Idem /\ done then
          skip;                   \* đã chạy rồi: chỉ trả lời lại
        else
          applied := applied + 1;
          done := TRUE;
        end if;
        resp_net := resp_net \union {a};
      end with;
    end while;
end process;
end algorithm; *)
====
```

Chú giải: timeout là `skip` không có điều kiện, nên client có thể bỏ cuộc đợi *ngay lập tức*, kể cả khi server chuẩn bị trả lời — đó là mô hình bất đồng bộ không giả định thời gian, nhất quán với mục 1. Hồi đáp "bị mất" không cần hành động riêng: client timeout rồi bỏ qua hồi đáp cũ, hoặc nhận nó ở vòng sau (nhận xét của người soạn). `MaxRetries` giữ mô hình hữu hạn. Process `server` không bao giờ kết thúc nên cần tắt kiểm deadlock (mục 1). Server và client đều không có "crash" riêng ở đây; thêm được bằng cách giống ngân sách ở mục 3.

**Trace tay khi `Idem = FALSE`, `MaxRetries >= 2` (suy luận của người soạn, chưa chạy TLC).**

1. `client.Send`: `attempt = 1`, `req_net = {1}`.
2. `client.Wait`, nhánh `skip` (timeout): vòng lặp tiếp.
3. `client.Send`: `attempt = 2`, `req_net = {1, 2}`.
4. `server.Serve` chọn `a = 1`: `applied = 1`.
5. `server.Serve` chọn `a = 2`: `Idem` sai nên `applied = 2`.

`AppliedAtMostOnce` sai. Chỉ cần hai attempt và một yêu cầu logic. Đây chính là tác dụng phụ lặp mà Kleppmann và AWS cảnh báo[^kleppmann-rpc][^abl-idem].

**Khi `Idem = TRUE` (suy luận, chưa chạy TLC).** Bất biến quy nạp: `applied = 1` nếu `done` và `applied = 0` nếu không. Bước 5 giờ thấy `done` đúng nên chỉ trả lời lại: `applied` vẫn là 1. `OkImpliesApplied` đúng ở cả hai chế độ: `client_ok` chỉ đặt khi nhận được hồi đáp, và hồi đáp chỉ sinh ra sau một lần `Serve`, mà `Serve` luôn dẫn tới `applied >= 1` (hoặc đã có `done`, tức `applied = 1` từ trước). Tính chất "client tin thành công thì server thật sự đã làm" không phụ thuộc idempotency; idempotency chỉ bảo vệ khỏi *làm quá*.

<details><summary>Đi sâu: backoff, bão retry, lưu key, và timeout như failure detector</summary>

- **Backoff không có trong spec — có chủ ý.** Retry làm tăng tải lên hệ đang quá tải; vì vậy AWS dùng backoff để tải lên backend đều hơn[^abl-backoff]. Backoff chỉ có ý nghĩa với *tải và thời gian*, thứ mà spec đã bỏ ở mục 1. Spec chỉ bắt được hệ quả logic (tác dụng phụ lặp), không bắt được hiện tượng quá tải tự khuếch đại; AWS tự nhận chưa có cách khả thi mô hình hóa hiện tượng đó[^aws-emergent-no].
- **Server phải nhớ key ở bộ nhớ bền.** Spec dùng biến `done` bền vĩnh viễn. Thật ra, dedup trong mô hình crash-recovery đòi hỏi lưu request hoặc metadata ở stable storage để phát hiện trùng được cả sau crash[^kleppmann-dedup]; và key thường có hạn dùng (xem [P12·12 mục 4](../12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md)). Hết hạn key là một hành động mới trong spec mà ta nên mô hình hóa nếu lo "retry đến sau khi key đã bị xóa" (nhận xét của người soạn).
- **Timeout là failure detector không hoàn hảo.** Nhánh `skip` nói đúng điều đó: client không thể phân biệt "server chậm" với "server chết" từ một lần timeout. Xem [P12·08 mục 4](../12-he-thong-du-lieu/08-su-co-he-phan-tan.md).
- **Liveness.** "Cuối cùng client có `client_ok`" cần giả định fairness và không thể bảo đảm khi `MaxRetries` hữu hạn mà mọi attempt đều timeout. Muốn phát biểu đúng, viết thêm giả định "mạng rồi sẽ giao được" ở [chương 05](05-liveness-va-fairness.md).

</details>

**Quay lại Q6:** timeout không cho biết tác dụng phụ đã xảy ra chưa, nên retry chỉ an toàn khi server nhận ra yêu cầu lặp; spec bắt được lỗi logic đó (hai attempt, một lần `applied` thừa) nhưng không bắt được hiệu ứng tải của bão retry.

## 7. Giữ không gian trạng thái trong tầm TLC (Q7)

> **Trả lời ngắn.** TLC duyệt mọi state đạt được, và số state thường tăng *mũ* theo tham số mô hình[^ss-exp]. Cách kiểm soát: luôn bắt đầu bằng mô hình thật nhỏ[^ss-tiny], dùng ít hằng[^lt-small] và ít chi tiết nhất có thể[^lt-detail], tận dụng symmetry, ngân sách lỗi và constraint khi cần, và tách mô hình kiểm safety khỏi mô hình kiểm liveness[^lt-safety-liveness]. Và hãy nghi ngờ mọi lần "không thấy lỗi": rất dễ thỏa một tính chất safety chỉ bằng cách không làm gì[^ss-suspicious].

**Giải thích.**

Hình dung (nhận xét của người soạn): bạn muốn thử mọi cách xếp ghế cho một đám cưới. Với 4 khách là chuyện nhỏ; với 12 khách thì không máy nào đếm hết. Nhưng phần lớn "ghế lệch" (xếp người ngồi cạnh kẻ thù) đã lộ ra từ bàn 4 người — bạn không cần bàn 12 để tìm ra nó.

*Mô hình nhỏ trước.* Sách của Lamport: luôn bắt đầu thử spec với mô hình *nhỏ xíu*: tập process và tập giá trị chỉ một phần tử, hàng đợi độ dài một[^ss-tiny]; một spec chưa được thử thường có nhiều lỗi, và mô hình nhỏ bắt nhanh các lỗi đơn giản; khi mô hình rất nhỏ hết lỗi thì tăng dần[^ss-small-first]. Learntla nói cùng ý: phần lớn lỗi xuất hiện trong không gian nhỏ, nên không có nhiều lợi ích khi dùng hằng khổng lồ; nên giữ cấu hình nhỏ để lặp nhanh và chỉ chạy cấu hình lớn khi cấu hình nhỏ đã qua[^lt-small].

Điều đó khớp với các spec chương này (suy luận của người soạn từ trace tay): lỗi lease cần hai client; lỗi hàng đợi cần một tin, một consumer, một lần crash; lỗi cache cần một reader, một writer; lỗi outbox cần một order và một lần crash relay; lỗi retry cần một yêu cầu và hai attempt. Mọi con số "hai", "một" đó là tham số của mô hình mà TLC chạy được trong tích tắc.

*Ước lượng cỡ.* Learntla gợi ý ước lượng trước: mô hình tốn thời gian theo thời gian sinh state và số state, và số state thường là yếu tố lấn át[^lt-two-factors]. Kích cỡ các kiểu tập xếp chồng lên nhau, và một cấu trúc như `[A \X B -> SUBSET C]` thường là nguồn của rắc rối[^lt-stack]. Ví dụ thô cho spec hàng đợi ở mục 3 (suy luận của người soạn; cận trên của tích miền giá trị các biến toàn cục, chưa tính `pc`, `cur`, `crashes`; *không phải* số state đạt được): với `M` tin, `C` consumer, `K = MaxCrashes`: `status` có 3^M giá trị, `seen` có 2^M, `effects[m]` tối đa C + K + 1 giá trị (mỗi lần một consumer nhận tin `m` kết thúc bằng một crash — tốn ngân sách, tổng cộng K — hoặc bằng một ack; sau ack, `m` không còn `ready` nên không ai nhận được nữa, và mỗi consumer chỉ giữ một lượt nhận tại một thời điểm, nên số lần nhận không quá C + K), nên tích cỡ (6·(C + K + 1))^M. Với C = 2, K = 2: 30^3 = 27 000 khi M = 3 nhưng 30^4 = 810 000 khi M = 4 — thêm một tin nhân lên 30 lần. Điều này khớp lời Lamport rằng số state đạt được thường là hàm mũ của tham số[^ss-exp].

*Symmetry.* Nếu spec không phân biệt các client/tin/consumer với nhau, đổi chỗ chúng không đổi việc một hành vi có thỏa spec hay không. Lamport: ghi `SYMMETRY Perms` với `Perms == Permutations(Proc)` trong file cấu hình làm TLC không thêm state `s` nếu đã có state tương đương qua một hoán vị; với n process, số state phải duyệt giảm theo hệ số n!, tức n giai thừa[^ss-sym]. Trong spec mục 3 sẽ là:

```tla
Sym == Permutations(Msgs) \union Permutations(Consumers)   \* đặt trong module sau END TRANSLATION; Permutations thuộc module TLC
```

```text
SPECIFICATION Spec
CONSTANTS
    Msgs = {m1, m2}
    Consumers = {c1, c2}
    NULL = NULL
    Dedup = TRUE
    MaxCrashes = 2
INVARIANT AtMostOneEffect
SYMMETRY Sym
CHECK_DEADLOCK FALSE
```

(File cấu hình minh họa do người soạn viết, theo khuôn các file `.cfg` trong `tlaplus/Examples`: ví dụ model KeyValueStore đặt `SYMMETRY TxIdSymmetric`[^ex-kvs-sym]. Chưa chạy TLC. Tên module khai `Msgs`, `Consumers` là model value.) Hai điều cần cẩn thận. Một: symmetry chỉ đúng khi *spec và tính chất* thật sự đối xứng; nếu không, TLC có thể không in nổi trace khi gặp lỗi[^ss-sym-trace]; spec dùng `CHOOSE` trên model value hay so sánh thứ tự các id là dấu hiệu phá đối xứng (nhận xét của người soạn). Hai: với kiểm *liveness*, sách cảnh báo TLC có thể kiểm sai: bỏ sót lỗi, báo lỗi không có thật, hoặc đưa counterexample sai[^ss-sym-liveness]. Learntla cũng nói liveness chậm hơn nhiều và không dùng được symmetry[^lt-safety-liveness]. Vậy: giữ hai file cấu hình, một cho safety (có symmetry), một cho liveness (hằng nhỏ hơn).

*Constraint và ngân sách.* Có hai cách để chặn mô hình vô hạn: đặt biên *trong spec* (ngân sách `MaxCrashes`, trần `MaxRetries`, như mục 3, 5, 6) hoặc đặt `CONSTRAINT` ở cấu hình. Learntla khuyên: phần lớn trường hợp mô hình vô hạn là lỗi spec (quên một kiểm tra), nên sửa spec trước[^lt-unbound-err]; khi không sửa được thì thêm state constraint, hoặc dùng `TLCGet("level")` để giới hạn độ sâu[^lt-tlcget]. Có hai điểm tinh vi:

- Ràng buộc không phải ở "kiểm tra", mà ở "dừng mở rộng". Theo thuật toán của sách, TLC kiểm invariant trên state mới `t` *trước*, rồi mới xét `Constraint` để quyết định có thêm `t` vào hàng đợi không[^ss-alg-inv]. Hệ quả (suy luận của người soạn từ thuật toán): state ngay sát biên vẫn được kiểm invariant, nhưng các state *sau* nó không được sinh.
- Constraint cắt hành vi: learntla cảnh báo đừng dùng khi kiểm temporal property, vì cắt đi một phần hành vi thì model checker không còn đủ hành vi để đánh giá liveness[^lt-bound-warning].

*Biến phụ và `VIEW`.* Biến lịch sử như `log` ở mục 2 làm số state nở ra: các hành vi trước đây dẫn tới cùng một state giờ khác nhau ở biến phụ. Learntla: `VIEW` cho TLC so sánh state *không tính* biến phụ, giúp khử cái giá đó, nhưng dễ vô tình loại state hợp lệ, nên dùng rất cẩn thận[^lt-views].

*Giảm chi tiết, gộp action, bỏ "loader".* Heuristic quan trọng nhất của learntla: mô hình càng chi tiết thì càng nhiều state[^lt-detail]; nếu cần mô hình chi tiết, hãy viết spec đơn giản ở mức cao rồi đặt mô hình chi tiết thành một refinement[^lt-refine]. Gộp hai label thành một giảm số chỗ chen, nhưng chỉ khi hệ thật *thật sự* nguyên tử ở đó; learntla nhắc rằng đặt grain quá thô có thể che mất lỗi thật[^lt-grain]. Tiến trình "loader" chỉ để dựng trạng thái đầu có thể thay bằng một `Init` nondeterministic[^lt-loader]. Với hệ ở chương này (nhận xét của người soạn), `Handle` ở mục 3 gộp "tác dụng phụ + dedup" vì DB bảo đảm nguyên tử — gộp *đúng*; còn gộp `ReadDB` và `FillCache` ở mục 4 sẽ xóa luôn lỗi cần săn — gộp *sai*.

*Chạy bao lâu, bao nhiêu worker.* Chạy từ dòng lệnh, TLC mặc định dùng một worker và 25% RAM, nên cần truyền `-workers`[^lt-workers]. Số liệu từ repo Examples cho thấy chi phí tăng mạnh theo cỡ: ba cấu hình cho cùng spec KeyValueStore (snapshot isolation). `MCKVSSafetySmall.cfg` có 2 key, 2 giá trị, 3 transaction[^ex-kvs-small-cfg] và manifest ghi 40 giây[^ex-kvs-small]. `MCKVSSafetyMedium.cfg` có 3 key, 3 giá trị, 2 transaction, kèm symmetry[^ex-kvs-medium-cfg], ghi 4 phút 30 giây[^ex-kvs-medium]. `MCKVSSafetyLarge.cfg` có 3 key, 2 giá trị, 3 transaction, kèm symmetry[^ex-kvs-large-cfg], ghi 7 giờ 16 phút[^ex-kvs-large]. Cả ba đều `success`. Từ Small lên Large là chừng 650 lần (26 160 giây chia 40 giây; số của manifest repo, không rõ phần cứng). Trong lần kiểm chứng phần sao chép/chịu lỗi của DynamoDB, AWS chạy phiên bản phân tán của TLC trên cụm mười máy EC2[^aws-distributed] để kiểm một thể hiện "đủ lớn" cho độ tin cậy cao[^aws-sufficient].

*Mô phỏng ngẫu nhiên.* Khi mô hình đã vượt tầm duyệt hết, Lamport: chạy mô hình lớn nhất mà kiên nhẫn cho phép, rồi chạy chế độ simulation trên mô hình lớn hơn; mô phỏng ngẫu nhiên không hiệu quả để tìm lỗi tinh vi, nhưng đáng thử[^ss-sim]. Con số "35 bước" của lỗi DynamoDB gợi ý vì sao tìm ngẫu nhiên khó (suy luận của người soạn: một trace dài như vậy khó gặp bằng bước chọn ngẫu nhiên; AWS không nói về simulation): AWS ghi trace lỗi ngắn nhất dài 35 bước ở mức cao[^aws-35], và cho rằng sự hiếm gặp của chuỗi sự kiện kép không phải lý do để bỏ qua chúng[^aws-improbable].

*Không rỗng: "đừng tin thành công".* Hai kiểu kết quả rỗng thường gặp (nhận xét của người soạn):

1. *Tính chất rỗng.* `CacheNotStale` ở mục 4 chỉ ràng buộc state "yên"; nếu không state yên nào đạt được (ví dụ lỡ `await` luôn sai), TLC báo "không có lỗi" mà chẳng kiểm gì. Cách thử rẻ: thêm một invariant mà bạn *biết* phải sai khi trạng thái mục tiêu đạt được, ví dụ `NotQuiescent == ~Quiescent`, rồi chạy và kỳ vọng TLC báo vi phạm. Nếu không vi phạm, spec chưa bao giờ đến state yên.
2. *Hành động không bao giờ bật.* Lamport: một lỗi phổ biến là một action không bật khi lẽ ra phải bật, ngăn một số state được đạt tới; dùng tùy chọn coverage để phát hiện action không bao giờ bật[^ss-coverage]. Cùng tinh thần, learntla cảnh báo: nếu đổi spec để một thread lặp mãi không làm gì thì invariant về kết quả cuối cùng *trivially* qua[^lt-trivial]; sách nói thẳng safety dễ thỏa bằng cách không làm gì[^ss-suspicious].

<details><summary>Đi sâu: quy trình đề xuất cho một spec backend, và các cờ thực tế</summary>

Quy trình (nhận xét của người soạn, tổng hợp từ các nguồn trên):

1. Viết spec ở mức thô nhất; chạy với `Msgs`, `Clients`, … đều một phần tử và `Fencing`/`Dedup` ở chế độ *sai* — bạn *phải* thấy counterexample. Không thấy: tính chất hoặc spec đang rỗng.
2. Đọc counterexample: đây là lỗi thiết kế, hay lỗi do mô hình môi trường quá hào phóng (ví dụ lease hết hạn *ngay lập tức* khi client chưa kịp làm gì — vẫn là hành vi hợp lệ trong mô hình bất đồng bộ)?
3. Bật cơ chế sửa (`Fencing = TRUE`); tăng dần hằng; thêm symmetry khi spec đối xứng.
4. Tách riêng cấu hình liveness với hằng nhỏ hơn.
5. Khi cấu hình lớn nhất còn chạy được đã xong: thử simulation với cấu hình lớn hơn; thử Apalache (mục 8) nếu tính chất có dạng bất biến quy nạp.
6. Ghi lại *chính xác* cấu hình đã kiểm (hằng, symmetry, constraint); đó là phạm vi khẳng định của bạn — "đã kiểm với hai client" chứ không phải "đúng với mọi số client".

Cờ thực tế: `-deadlock` hoặc `CHECK_DEADLOCK FALSE` (mục 1), `-workers`, `-simulate` (chế độ mô phỏng)[^ss-simulate-opt]. Learntla còn dùng ràng buộc `TLCGet("level") < 12` (trong lời văn của trang; code mẫu trên trang ghi `< 11`) như một mẹo lặp nhanh khi thử các tối ưu hóa[^lt-level-iter].

</details>

**Quay lại Q7:** bắt đầu từ mô hình nhỏ nhất vẫn tái hiện lỗi; mỗi tăng tham số nhân mũ số state; symmetry, ngân sách, constraint, view là công cụ chỉnh cỡ — mỗi cái có cái giá về độ tin cậy cần ghi lại; và một lần "không thấy lỗi" chỉ đáng tin sau khi bạn đã thấy mô hình *bắt được* lỗi ở chế độ sai.

## 8. Apalache: symbolic, SMT, khi TLC không đủ (Q8)

> **Trả lời ngắn.** TLC liệt kê từng state; Apalache dịch bài toán kiểm tra thành các ràng buộc logic và nhờ bộ giải SMT (Z3 mặc định, CVC5 thử nghiệm) tìm lời giải[^ap-symbolic][^ap-smt]. Nhờ đó nó kiểm được bất biến quy nạp cho thực thi dài tùy ý và xử lý miền số lớn mà không liệt kê. Đổi lại: spec phải gắn kiểu, không dùng được toàn bộ ngôn ngữ, mô hình vẫn phải hữu hạn, và tài liệu của chính Apalache nói hiệu quả hơn hay kém hơn TLC tùy spec[^ap-depends].

**Giải thích.**

Hình dung (nhận xét của người soạn): TLC giống một thanh tra đi qua từng căn phòng của tòa nhà để xem có gì sai; Apalache giống một luật sư hỏi: "có tồn tại một chuỗi sự kiện nào thỏa các điều kiện này mà dẫn tới vi phạm không?" rồi nhờ một bộ giải logic trả lời — mà không phải đi qua từng phòng.

*Cách hoạt động.* Apalache trích các *symbolic transition* từ spec — phân `Next` thành tuyển của các action đơn giản hơn — rồi dịch `Init` và các action sang công thức SMT[^ap-theory]. Một chuỗi `k` bước được mã hóa thành `Init(s0) /\ A(s0, s1) /\ ... /\ A(s_{k-1}, s_k)`, và để tìm vi phạm bất biến `Inv` người ta thêm `~Inv(s0) \/ ... \/ ~Inv(sk)`; điều quan trọng là giá trị của `s0, …, sk` *không* được liệt kê như TLC mà do bộ giải SMT tìm[^ap-theory-enum]. Hệ quả (suy luận của người soạn): việc "duyệt" nằm trong bộ giải; chi phí phụ thuộc cấu trúc công thức, không đơn thuần là số state.

*Hai chế độ kiểm.*

- **Bounded model checking (mặc định).** Kiểm các thực thi có độ dài tối đa `--length` (mặc định 10 bước `Next`)[^ap-length]. Tài liệu nói thẳng đây là kỹ thuật *không đầy đủ*: nếu một lỗi cần thực thi dài hơn để lộ ra thì có thể bị bỏ sót[^ap-bmc-incomplete].
- **Bất biến quy nạp.** Để kiểm thực thi dài tùy ý, người ta tìm một công thức `IndInv` thỏa hai điều kiện: trạng thái đầu thỏa `TypeOK /\ IndInv`, và bước `Next` từ một state thỏa `TypeOK /\ IndInv` dẫn tới state cũng thỏa[^ap-ind]. Khi đó chạy hai lệnh `check` ngắn (`--init=Init --inv=IndInv --length=0` và `--init=IndInv --inv=IndInv --length=1`), rồi lệnh thứ ba kiểm `IndInv` kéo theo tính chất an toàn đang quan tâm[^ap-indcmds]. Tài liệu lưu ý: nếu `IndInv` quá yếu, bước ba sẽ cho counterexample và bạn phải mạnh hóa nó[^ap-ind-weak].

*Spec tương ứng, viết bằng TLA+ thuần có chú thích kiểu (chưa chạy Apalache).* Dưới đây là bản TLA+ của kho có fencing ở mục 2, viết để Apalache kiểm được. Mỗi `CONSTANT`/`VARIABLE` phải có chú thích kiểu `\* @type: …;`[^ap-types]; mỗi biến phải có *đúng một* phép gán `x' = e` trong mỗi symbolic transition, nếu không Apalache báo lỗi[^ap-assign]; bởi vậy các hành động bên dưới liệt kê mọi biến, kể cả bằng `UNCHANGED`. Hằng được khởi tạo qua một toán tử `ConstInit` hoặc khai như toán tử (như `N == 3`)[^ap-constops]. Hai toán tử `ConstFenced`/`ConstUnfenced` bên dưới viết theo khuôn `ConstInit` của tài liệu (như `BIRTH_YEAR \in 0..99 /\ …`[^ap-constinit]) nhưng dùng `=` thay `\in` (suy luận của người soạn, chưa chạy). Dãy `log` được thay bằng hai biến lịch sử (`applied_max`, `stale_applied`) cho gọn (nhận xét của người soạn); nhớ rằng Apalache không làm gì được với tập `Seq(S)` vô hạn[^ap-seq].

```tla
---------------------------- MODULE LeaseFencingApa ----------------------------
EXTENDS Integers

CONSTANT
  \* @type: Bool;
  Fencing

N == 3
Clients == 1..N

VARIABLES
  \* @type: Int;
  holder,          \* 0 = không ai giữ lease
  \* @type: Int;
  next_token,
  \* @type: Int;
  storage_max,     \* token lớn nhất kho đã chấp nhận
  \* @type: Int;
  applied_max,     \* biến lịch sử: token lớn nhất từng được áp dụng
  \* @type: Bool;
  stale_applied,   \* biến lịch sử: từng áp dụng ghi có token nhỏ hơn applied_max
  \* @type: Int -> Int;
  my_tok,
  \* @type: Int -> Str;
  pc

vars == <<holder, next_token, storage_max, applied_max, stale_applied, my_tok, pc>>

ConstFenced   == Fencing = TRUE
ConstUnfenced == Fencing = FALSE

TypeOK ==
  /\ holder \in 0..N
  /\ next_token \in Int
  /\ storage_max \in Int
  /\ applied_max \in Int
  /\ stale_applied \in BOOLEAN
  /\ my_tok \in [Clients -> Int]
  /\ pc \in [Clients -> {"idle", "leased", "done"}]

Init ==
  /\ holder = 0
  /\ next_token = 1
  /\ storage_max = 0
  /\ applied_max = 0
  /\ stale_applied = FALSE
  /\ my_tok = [c \in Clients |-> 0]
  /\ pc = [c \in Clients |-> "idle"]

Acquire(c) ==
  /\ pc[c] = "idle"
  /\ holder = 0
  /\ holder' = c
  /\ my_tok' = [my_tok EXCEPT ![c] = next_token]
  /\ next_token' = next_token + 1
  /\ pc' = [pc EXCEPT ![c] = "leased"]
  /\ UNCHANGED <<storage_max, applied_max, stale_applied>>

Expire ==
  /\ holder /= 0
  /\ holder' = 0
  /\ UNCHANGED <<next_token, storage_max, applied_max, stale_applied, my_tok, pc>>

\* Kho chấp nhận ghi: kiểm token VÀ ghi trong cùng một bước.
WriteOk(c) ==
  /\ pc[c] = "leased"
  /\ (~Fencing \/ my_tok[c] >= storage_max)
  /\ pc' = [pc EXCEPT ![c] = "done"]
  /\ storage_max' = my_tok[c]
  /\ applied_max' = IF my_tok[c] > applied_max THEN my_tok[c] ELSE applied_max
  /\ stale_applied' = (stale_applied \/ my_tok[c] < applied_max)
  /\ UNCHANGED <<holder, next_token, my_tok>>

WriteReject(c) ==
  /\ pc[c] = "leased"
  /\ Fencing
  /\ my_tok[c] < storage_max
  /\ pc' = [pc EXCEPT ![c] = "done"]
  /\ UNCHANGED <<holder, next_token, storage_max, applied_max, stale_applied, my_tok>>

Terminated ==
  /\ \A c \in Clients : pc[c] = "done"
  /\ UNCHANGED vars

Next ==
  \/ \E c \in Clients : Acquire(c)
  \/ Expire
  \/ \E c \in Clients : WriteOk(c)
  \/ \E c \in Clients : WriteReject(c)
  \/ Terminated

Spec == Init /\ [][Next]_vars

NoStaleWrite == ~stale_applied

\* Bất biến quy nạp cho Fencing = TRUE
IndInv ==
  /\ TypeOK
  /\ (Fencing => storage_max = applied_max)
  /\ ~stale_applied
=============================================================================
```

*Các lệnh định chạy (cú pháp lấy từ tài liệu Apalache; chưa chạy).*

```text
# Tìm lỗi khi KHÔNG có fencing (BMC, mặc định tối đa 10 bước)
apalache-mc check --cinit=ConstUnfenced --inv=NoStaleWrite LeaseFencingApa.tla

# Chứng minh bằng quy nạp khi CÓ fencing, ba lệnh
apalache-mc check --cinit=ConstFenced --init=Init     --inv=IndInv       --length=0 LeaseFencingApa.tla
apalache-mc check --cinit=ConstFenced --init=IndInv   --inv=IndInv       --length=1 LeaseFencingApa.tla
apalache-mc check --cinit=ConstFenced --init=IndInv   --inv=NoStaleWrite --length=0 LeaseFencingApa.tla
```

*Hai kết quả mong đợi (suy luận của người soạn, chưa chạy Apalache).*

- Với `Fencing = FALSE`: có counterexample dài 5 bước — `Acquire(1)`, `Expire`, `Acquire(2)`, `WriteOk(2)` (`applied_max = 2`), `WriteOk(1)` (`stale_applied = (FALSE \/ 1 < 2) = TRUE`). Dài 5 nằm trong trần mặc định 10[^ap-length].
- Với `Fencing = TRUE`: `IndInv` là bất biến quy nạp. `Init` thỏa (cả `storage_max` và `applied_max` bằng 0, `stale_applied` sai). `WriteOk(c)` khi `Fencing` đúng: `my_tok[c] >= storage_max = applied_max`, nên `stale_applied'` vẫn sai, và `storage_max' = applied_max' = my_tok[c]` giữ đẳng thức. `WriteReject` chỉ đổi `pc`; các hành động còn lại không đụng bốn biến đó. Vì vậy ba lệnh trên đều nên báo không có vi phạm. Đây là lập luận tay của mục 2 viết lại cho bộ giải.

*Giới hạn Apalache cần biết.*

- Mọi tham số cố định và hữu hạn, mọi cấu trúc dữ liệu hữu hạn[^ap-assume], và chỉ phân tích thực thi hữu hạn có trần độ dài[^ap-assume-exec]. Với "N client tùy ý", Apalache một mình không trả lời; ta chỉ kiểm với N cụ thể (ở trên là 3).
- Có những cấu trúc không hỗ trợ: tập `Seq(S)` vô hạn (tạm thời có `Gen(n)` để sinh dãy tối đa `n` phần tử)[^ap-gen]; khoảng số nguyên `a..b` với cận không hằng không mã hóa được[^ap-range]; `CHOOSE` không dùng trực tiếp được, từ bản 0.16.1 tài liệu khuyên dùng `Some`, `ApaFoldSet` hoặc `ApaFoldSeqLeft`[^ap-features].
- Với PlusCal: bộ dịch PlusCal xóa comment nên không gắn được chú thích kiểu trực tiếp trong code PlusCal; tutorial của Apalache đề xuất tạo một module kiểu riêng, dùng `INSTANCE` để các `CONSTANT`/`VARIABLE` có kiểu thay thế cho bản gốc, và module này không bị ảnh hưởng khi PlusCal đổi[^ap-pluscal][^ap-instance]. Vậy spec mục 2–6 không chạy thẳng trên Apalache mà cần phiên bản TLA+ đã dịch cộng module kiểu.
- Apalache tự mô tả là còn thử nghiệm, nên hãy chuẩn bị dùng dòng lệnh và giúp tìm lỗi[^ap-experimental].
- Liveness: Apalache dùng phép biến đổi liveness-to-safety[^ap-liveness]; vì vậy với liveness hãy đọc lại tài liệu hiện hành trước khi dựa vào (nhận xét của người soạn).

*Nhanh hơn TLC không phải mặc định.* Chính tutorial PlusCal của Apalache cho một ví dụ: kiểm tính loại trừ lẫn nhau (mutual exclusion) của thuật toán Bakery với bốn process, tối đa 10 bước, ghi `It took me 0 days 0 hours 32 min 2 sec` — kèm lời rằng phân tích này chưa đầy đủ vì trần 10 bước[^ap-bakery-time]. (Số liệu từ trang tài liệu, không rõ phần cứng; không so với TLC trên cùng máy.) Cũng chính tài liệu này nêu: tùy spec, TLC hoặc Apalache có thể hiệu quả hơn[^ap-depends].

```mermaid
flowchart TD
  A["Spec đã có, hằng nhỏ"] --> B{"TLC chạy xong trong thời gian chấp nhận được?"}
  B -->|"có"| TLC["Dùng TLC: đầy đủ ngôn ngữ, liveness trưởng thành"]
  B -->|"không"| C{"Cần thực thi dài hoặc miền số lớn?"}
  C -->|"có: tìm được bất biến quy nạp"| AP1["Apalache: kiểm IndInv bằng 3 lệnh"]
  C -->|"có: chỉ cần tìm lỗi"| AP2["Apalache BMC hoặc simulate"]
  C -->|"không"| D["Giảm chi tiết, symmetry, hằng nhỏ hơn (mục 7)"]
  AP1 --> E["Tham số vẫn cố định hữu hạn"]
```

(Sơ đồ là nhận xét của người soạn, không phải quy trình chính thức của Apalache.)

<details><summary>Đi sâu: một ví dụ miền số lớn, simulate, các loại invariant</summary>

- **Miền hằng lớn.** Tài liệu Apalache có ví dụ khởi tạo hằng bằng khoảng: `ConstInit == BIRTH_YEAR \in 0..99 /\ LICENSE_AGE \in 10..99`; với ràng buộc đó Apalache tìm thấy vi phạm an toàn, ví dụ `BIRTH_YEAR = 89` và `LICENSE_AGE = 10`[^ap-constinit]. Ý nghĩa cho spec backend (suy luận của người soạn): thay vì chạy TLC cho từng tổ hợp (ví dụ `MaxRetries`, `TTL`, kích thước batch), một lần chạy symbolic có thể quét cả khoảng. Lưu ý giới hạn khoảng với cận không hằng ở trên.
- **`simulate`.** Chế độ này chọn ngẫu nhiên một dãy action rồi kiểm invariant trên tập con các thực thi chỉ dùng thứ tự action đó; tài liệu nói thường nhanh hơn nhiều so với `check`[^ap-simulate].
- **Ba loại invariant.** Ngoài invariant trạng thái, Apalache kiểm *action invariant* (dùng biến có dấu phẩy `'`, tức tính chất trên cặp state liền nhau) và *trace invariant* (nhận lịch sử thực thi)[^ap-inv-kinds]; tài liệu khuyên dùng trace invariant cho kiểm thử, còn để kiểm chứng thì nên dùng temporal property[^ap-trace-warning]. Action invariant là lựa chọn tự nhiên cho tính chất "token không đi lùi" thay cho biến lịch sử `stale_applied`: nhận xét của người soạn, chưa chạy.
- **Trạng thái dự án.** Tài liệu Apalache là trang sống; tên lệnh, cờ và danh sách tính năng hỗ trợ có thể đổi. Kiểm lại trang gốc trước khi dựa vào chi tiết trong chương này.

</details>

**Quay lại Q8:** Apalache là bộ kiểm *bổ sung*, không thay thế TLC: nó mạnh ở bất biến quy nạp cho thực thi dài tùy ý và miền số lớn, nhưng đòi kiểu, cần phiên bản TLA+ của spec PlusCal, vẫn bị giới hạn hữu hạn, và không bảo đảm nhanh hơn.

## Liên hệ với Rust và backend (nhận xét của người soạn)

*Chỗ Rust và công cụ backend đứng trên các khái niệm trên.* Chương này nhấn mạnh: spec chỉ bảo đảm những gì grain of atomicity của nó bảo đảm, nên câu hỏi khi viết code là "bước nguyên tử của spec ứng với cái gì trong chương trình?".

1. **Một label ↔ một critical section hay một câu lệnh có điều kiện.** Ở mục 2, `Write` kiểm token và ghi trong *một* label. Trong Rust, `AtomicU64::fetch_max` cập nhật giá trị bằng max của giá trị hiện tại và đối số, rồi trả về giá trị cũ[^rust-fetch-max]. Nhưng dùng `fetch_max` để "kiểm token" rồi *ghi dữ liệu thật* ở một lệnh sau là *hai* label — đúng lỗi ở trace (b) mục 2: client token cũ qua phép kiểm, bị chen bởi client token mới, rồi ghi. Fencing đúng cần kiểm và áp dụng trong cùng một mutex, hoặc một câu lệnh có điều kiện ở kho (ví dụ `UPDATE … WHERE fence <= $token`), tức nơi *giữ dữ liệu* làm phép so sánh (nhận xét của người soạn). Memory model của Rust nói thêm về thứ tự các thao tác nguyên tử ở [P4·06](../04-concurrency/06-memory-model.md); lỗi "kiểm rồi mới làm" thuộc họ atomicity violation ở [P4·05 mục 2](../04-concurrency/05-concurrency-bug-va-deadlock.md).
2. **Timeout ↔ hủy phía client, không hủy việc phía server.** `tokio::time::timeout` trả về `Elapsed` và *hủy* future nếu quá hạn[^tokio-timeout]; việc hủy chỉ xảy ra ở phía bạn. Yêu cầu đã gửi có thể đã chạy ở server: đúng chỗ hở mục 6 mô tả. Hệ quả: gọi API có tác dụng phụ trong `timeout` rồi retry cần idempotency key ở phía server; xem [P7·09 mục 2](../07-io-async/09-async-trong-production.md) (nhận xét của người soạn).
3. **Pause ↔ điểm chen interleaving.** Spec không cần hành động "pause" vì interleaving bao trùm nó (mục 1). Trong code Rust bất đồng bộ, mỗi `.await` là một chỗ mà task khác chạy xen vào; với thread OS, preempt có thể xảy ra ở bất kỳ chỗ nào. Vì thế mỗi "check rồi act" qua `.await` hay qua hai lệnh nên được coi là hai label (nhận xét của người soạn).
4. **Outbox, dedup ↔ transaction DB.** Một transaction DB ứng với *một* label ở spec; kiểu của Rust không diễn tả được "DB và broker cùng nguyên tử", đó là lý do outbox ghi tin vào cùng transaction. Phần cài đặt cụ thể (thư viện DB, relay) thuộc chương 10 và [P12·12](../12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md).
5. **Spec ↔ code.** Spec kiểm *thiết kế*, không kiểm code: AWS thẳng thắn nói họ không có cách để biết chắc code cài đặt đúng thiết kế đã kiểm[^aws-code]. Cách làm cầu nối (sinh test từ trace, `loom`/`shuttle`/`stateright`, assertion lấy từ bất biến) là nội dung [chương 10](10-tu-dac-ta-den-code-rust.md).

## Tóm tắt

- Mô hình hóa backend là chọn: biến ở nơi dữ liệu có thể bất đồng, label ở ranh giới nguyên tử của hệ thật, lỗi môi trường thành hành động, thời gian thành "có thể xảy ra bất cứ lúc nào" (mục 1).
- Pause không cần mô hình riêng: interleaving đã bao trùm; "check rồi act" qua hai label luôn là một cửa sổ lỗi.
- Lease một mình hỏng khi chủ cũ bị pause; fencing token chỉ an toàn khi kho kiểm token *và* ghi trong cùng bước nguyên tử và token tăng đơn điệu (mục 2).
- At-least-once nghĩa là spec phải cho phép giao lặp; idempotent đúng khi bản ghi dedup cùng bước nguyên tử với tác dụng phụ (mục 3).
- Cache-aside: tính chất đúng là "khi yên, cache trống hoặc bằng DB"; stale set là reader giữ giá trị cũ qua một lần ghi + xóa; lease token hay thứ tự thao tác đúng chặn nó (mục 4).
- Outbox biến dual write thành một transaction cộng một relay lặp lại được; trùng lặp là cái giá; bất biến lõi là `db_orders = outbox` (mục 5).
- Retry với timeout cần idempotency phía server; bão retry (hiệu năng nổi lên) nằm ngoài tầm TLA+ theo chính AWS (mục 6).
- Không gian trạng thái: mô hình nhỏ trước, mũ theo tham số, symmetry/ngân sách/constraint/view có cái giá, tách safety khỏi liveness, và chỉ tin "không lỗi" sau khi thấy mô hình bắt được lỗi ở chế độ sai (mục 7).
- Apalache kiểm bất biến quy nạp và miền số lớn nhờ SMT, nhưng đòi kiểu, giới hạn hữu hạn và không đảm bảo nhanh hơn TLC (mục 8).
- Mọi kết luận "vi phạm" trong chương là suy luận có nhãn kèm trace tay, chưa chạy TLC/Apalache.

## Câu hỏi tự kiểm tra

1. Vì sao hết hạn lease được mô hình hóa là hành động có thể xảy ra bất cứ lúc nào thay vì một đồng hồ?
2. Viết trace ngắn nhất (theo spec mục 2, `Fencing = FALSE`) làm `NoStaleWrite` sai. Cần tối thiểu mấy client?
3. Vì sao thêm label `Check` ("await holder = self") trước `Write` không sửa được lỗi?
4. Nếu kho kiểm token ở một label và ghi ở label kế, lỗi nào xuất hiện? Nó liên quan gì đến `fetch_max`?
5. Ở spec at-least-once, vì sao `effects` và `seen` phải được cập nhật trong cùng một label? Cho trace khi tách đôi.
6. Vì sao `CacheNotStale` được viết dưới dạng `Quiescent => …` chứ không phải `cache = NULL \/ cache = db` trong mọi state?
7. Nếu writer xóa cache *trước* khi ghi DB và reader dùng lease token, lỗi còn không? Cho trace.
8. Với outbox, bất biến `Atomic` bảo vệ điều gì? Vì sao `AtMostOnce` được kỳ vọng sai?
9. Vì sao spec retry cần `MaxRetries`, và vì sao nó không bắt được bão retry?
10. Theo thuật toán TLC của sách, một state vi phạm `CONSTRAINT` có được kiểm invariant không? Vì sao constraint nguy hiểm khi kiểm liveness?
11. Symmetry giảm bao nhiêu state, và cảnh báo nào khi kết hợp với liveness?
12. Hai điều kiện của bất biến quy nạp trong Apalache là gì; vì sao spec PlusCal cần module `INSTANCE` riêng để chạy Apalache?

<details><summary>Đáp án</summary>

1. Vì các thuật toán trong mô hình bất đồng bộ thường bảo đảm safety mà không cần giả định thời gian; chỉ liveness phụ thuộc timeout hay failure detector[^kb-safety-liveness]. Cho `Expire` bật bất cứ lúc nào bao trùm mọi lịch hết hạn thật; bản có đồng hồ chỉ nhân state lên mà không bỏ lỗi (suy luận của người soạn).
2. `c1.Acquire`; `Expire`; `c2.Acquire`; `c2.Write`; `c1.Write` — `log = <<(c2,2), (c1,1)>>`, `NoStaleWrite` sai (mục 2; suy luận, chưa chạy TLC). Cần hai client: một người bị pause, một người lấy lease mới.
3. `Check` và `Write` là hai label; giữa chúng `Expire` + client khác chạy được. Kiểm hạn ngay trước ghi không đóng được cửa sổ giữa kiểm và ghi[^kb-nocheck].
4. Client token cũ qua phép kiểm với `storage_max` cũ, bị chen bởi client token mới ghi trước, rồi ghi tiếp và còn hạ `storage_max`. `fetch_max` chỉ nguyên tử cho giá trị bộ đếm[^rust-fetch-max]; ghi dữ liệu thật ở lệnh sau thành label thứ hai, nên token check và ghi phải chung critical section hoặc chung câu lệnh có điều kiện (mục 2, đi sâu b; mục "Liên hệ").
5. Nếu tách, `c1.Effect` tăng `effects` khi `seen` còn rỗng; sau `Redeliver`, `c2.Effect` cũng thấy `seen` rỗng và tăng lần nữa — `effects[m1] = 2` dù `Dedup = TRUE`. Vì vậy bản ghi dedup phải nằm cùng một bước nguyên tử (cùng transaction) với tác dụng phụ (suy luận của người soạn từ spec); Kleppmann chỉ nêu thêm điều kiện rằng bản ghi dedup phải ở bộ nhớ bền để phát hiện trùng được cả sau crash[^kleppmann-dedup].
6. Vì cache-aside cho phép độ cũ tạm thời sau một lần ghi: reader có thể trượt cache hoặc thoáng thấy bản cũ[^az-stale-window]. Lỗi thật là cache giữ giá trị cũ *khi hệ đã yên*.
7. Còn. `Invalidate` (`epoch = 1`); `ReadCache` trượt (`tok = 1`); `ReadDB` (`v = 0`); `WriteDB` (`db = 1`); `FillCache` thấy `epoch = tok` nên đặt `cache = 0`: state yên có `cache ≠ db`. Thứ tự thao tác sai không được lease bù[^az-bad-order] (suy luận, chưa chạy TLC).
8. `Atomic` (`db_orders = outbox`) nói dòng nghiệp vụ và dòng outbox chỉ đổi cùng nhau trong `Txn`; mọi tính chất khác (không mất, không "ma") dựa vào nó[^msio-benefit]. `AtMostOnce` sai vì relay có thể chết sau `Publish` trước `MarkSent` và phát lại[^msio-dup].
9. Không có trần thì số attempt và nội dung `req_net` vô hạn: mô hình không hữu hạn. Spec chỉ biểu diễn logic tác dụng phụ; bão retry là hiệu ứng tải/thời gian nổi lên, AWS nói chưa biết cách khả thi để mô hình hóa[^aws-emergent-no].
10. Có: TLC kiểm invariant trên state mới trước, rồi mới xét `Constraint`[^ss-alg-inv]. Nhưng constraint cắt hành vi nên liveness có thể đánh giá sai[^lt-bound-warning].
11. Khoảng n! lần với n process hoán vị được[^ss-sym]. Với liveness, TLC có thể bỏ sót lỗi, báo lỗi không có thật hoặc in counterexample sai[^ss-sym-liveness].
12. (i) Trạng thái đầu thỏa `TypeOK /\ IndInv`; (ii) từ state thỏa `TypeOK /\ IndInv`, bước `Next` dẫn tới state thỏa lại[^ap-ind]. PlusCal translator xóa comment nên chú thích kiểu không đặt trực tiếp trong PlusCal; module kiểu dùng `INSTANCE` thay các `CONSTANT`/`VARIABLE` có kiểu và không bị ghi đè khi PlusCal đổi[^ap-pluscal][^ap-instance].

</details>

## Đọc thêm

- [P12·08 — Sự cố phân tán, mục 7–8](../12-he-thong-du-lieu/08-su-co-he-phan-tan.md): process pause, lease và fencing ở mức hệ thống, nền cho mục 2.
- [P12·11 — Stream processing, mục 4](../12-he-thong-du-lieu/11-stream-processing.md) và [P12·12, mục 4–6](../12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md): at-least-once, idempotency, outbox — phần "hệ thật" của mục 3, 5, 6.
- [P12·05 — Replication](../12-he-thong-du-lieu/05-replication.md), [P12·07 — Transaction](../12-he-thong-du-lieu/07-transaction.md), [P12·09 — Consistency và consensus](../12-he-thong-du-lieu/09-consistency-va-consensus.md): các lược đồ bạn sẽ muốn mô hình hóa tiếp theo.
- [P4·05 — Concurrency bug và deadlock](../04-concurrency/05-concurrency-bug-va-deadlock.md) và [P4·06 — Memory model](../04-concurrency/06-memory-model.md): cùng khuôn "check rồi act" ở mức thread.
- [P7·09 — Async trong production, mục 2](../07-io-async/09-async-trong-production.md): `timeout` không hủy việc phía xa.
- Kleppmann, *How to do distributed locking* — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html>: đọc phần "Making the lock safe with fencing" để đối chiếu mục 2.
- Newcombe et al., *How Amazon Web Services Uses Formal Methods* — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf>: đọc phần "What Formal Specification Is Not Good For" và "Most Frequently Asked Question" cho giới hạn của spec.
- Lamport, *Specifying Systems*, chương 14 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf>: TLC, symmetry, constraint, và mục "Be Suspicious of Success".
- Learn TLA+ (trang sống), *Optimizing Model Checking*, *Handling Unbound Models*, *Modelling Message Queues* — <https://learntla.com/topics/index.html>.
- Apalache (trang sống), *Running the Tool*, *Five minutes of theory*, *Tutorial on checking PlusCal specifications* — <https://apalache-mc.org/docs/>.
- `tlaplus/Examples` (commit `6a7b26a`), `specifications/CheckpointCoordination` và `specifications/KeyValueStore` — <https://github.com/tlaplus/Examples/tree/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications>: một spec backend có thật cùng cấu hình model nhỏ/lớn.
- [Chương 07 — Refinement](07-refinement.md) và [Chương 10 — Từ đặc tả đến code Rust](10-tu-dac-ta-den-code-rust.md): bậc thang trừu tượng và cầu nối sang code.

[^aws-abstract]: Newcombe et al., How Amazon Web Services Uses Formal Methods, mục "Side Benefit: A Better Way to Design Systems" — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf> — "we then precisely describe an abstract version of the design along with an abstract version of its operating environment."
[^aws-events]: Newcombe et al., How Amazon Web Services Uses Formal Methods, mục "Side Benefit: A Better Way to Design Systems" — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf> — "Next, with the goal of confirming that our design correctly handles all of the dynamic events in the environment, we specify the effects of each of those possible events; e.g. network errors and repairs, disk errors, process crashes and restarts, data center failures and repairs, and actions by human operators."
[^aws-needs-right]: Newcombe et al., How Amazon Web Services Uses Formal Methods, mục "Side Benefit: A Better Way to Design Systems" — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf> — "We have found this rigorous “what needs to go right?” approach to be significantly less error prone than the ad hoc “what might go wrong?” approach."
[^aws-env-props]: Newcombe et al., How Amazon Web Services Uses Formal Methods, mục "Side Benefit: A Better Way to Design Systems" — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf> — "We express “what must go right” by explicitly specifying all of the properties of the environment on which the system relies."
[^aws-safety]: Newcombe et al., How Amazon Web Services Uses Formal Methods, mục "Side Benefit: A Better Way to Design Systems" — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf> — "Safety properties: “what the system is allowed to do” Example: at all times, all committed data is present and correct."
[^aws-liveness]: Newcombe et al., How Amazon Web Services Uses Formal Methods, mục "Side Benefit: A Better Way to Design Systems" — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf> — "Liveness properties: “what the system must eventually do”"
[^aws-emergent]: Newcombe et al., How Amazon Web Services Uses Formal Methods, mục "What Formal Specification Is Not Good For" — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf> — "A common example is when a momentary slowdown in a server (perhaps due to Java garbage collection) causes timeouts to be breached on clients, which causes the clients to retry requests, which adds more load to the server, which causes further slowdown."
[^aws-emergent-no]: Newcombe et al., How Amazon Web Services Uses Formal Methods, mục "What Formal Specification Is Not Good For" — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf> — "We don’t yet know of a feasible way to model a real system that would enable tools to predict such emergent behavior."
[^aws-code]: Newcombe et al., How Amazon Web Services Uses Formal Methods, mục "The Most Frequently Asked Question" — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf> — "On learning about TLA+, engineers usually ask, “How do we know that the executable code correctly implements the verified design?” The answer is that we don’t."
[^aws-ladder]: Newcombe et al., How Amazon Web Services Uses Formal Methods, mục "Precise Designs" — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf> — "The ladder of abstraction also helps designers to manage the complexity of real-world systems; the designer may choose to describe the system at several ‘middle’ levels of abstraction, with each lower level serving a different purpose, such as understanding the consequences of finer-grain concurrency"
[^aws-distributed]: Newcombe et al., How Amazon Web Services Uses Formal Methods, mục "First Big Success at Amazon" — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf> — "we used the distributed version of the TLC model checker, running on a cluster of ten cc1.4xlarge EC2 instances"
[^aws-sufficient]: Newcombe et al., How Amazon Web Services Uses Formal Methods, mục "First Big Success at Amazon" — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf> — "for a sufficiently large instance of the system to give very high confidence that it is correct."
[^aws-35]: Newcombe et al., How Amazon Web Services Uses Formal Methods, mục "First Big Success at Amazon" — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf> — "the shortest error trace exhibiting the bug contained 35 high level steps."
[^aws-improbable]: Newcombe et al., How Amazon Web Services Uses Formal Methods, mục "First Big Success at Amazon" — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf> — "The improbability of such compound events is not a defense against such bugs"
[^pm-step]: Lamport, PlusCal manual (P-syntax), mục 2.6 — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A single step (atomic action) of a PlusCal algorithm consists of the execution from one label to the next."
[^pm-grain]: Lamport, PlusCal manual (P-syntax), mục 1 — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "For multiprocess (concurrent) algorithms, you will probably want to specify the grain of atomicity explicitly."
[^pm-arbitrary]: Lamport, PlusCal manual (P-syntax), mục 3.3 — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A multiprocess algorithm is executed by repeatedly choosing an arbitrary process and executing one step of that process, if that step’s execution is possible."
[^pm-assign-once]: Lamport, PlusCal manual (P-syntax), mục 3.2.1 — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Assignments to the same variable cannot be made in two different assignment statements within the same step."
[^pm-with-label]: Lamport, PlusCal manual (P-syntax), mục 3.2.6 — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The body of a with statement may not contain a label."
[^pm-either]: Lamport, PlusCal manual (P-syntax), mục 3.2.3 — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "It is executed by nondeterministically choosing any clause i that is executable and executing it."
[^pm-ids]: Lamport, PlusCal manual (P-syntax), mục 3.3 — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "For execution by TLC, this means that all process identifiers must be comparable values"
[^ss-modelvalue]: Lamport, Specifying Systems, mục 14.2.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "TLC considers a model value to be comparable to, and unequal to, any other value."
[^ss-now]: Lamport, Specifying Systems, mục 9.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "In TLA+ specifications, I prefer to use the variable now rather than t."
[^ss-realtime-rare]: Lamport, Specifying Systems, chương 9 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Formal specifications are most often used to describe what a system does rather than how long it takes to do it."
[^ss-deadlock-opt]: Lamport, Specifying Systems, mục 14.5 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Tells TLC not to check for deadlock. Unless this option is specified, TLC will stop if it finds a deadlock"
[^ss-exp]: Lamport, Specifying Systems, mục 14.5.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The number of reachable states is typically an exponential function of the model’s parameters"
[^ss-tiny]: Lamport, Specifying Systems, mục 14.5.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "You should always begin testing a specification with a tiny model, which TLC can check quickly."
[^ss-small-first]: Lamport, Specifying Systems, mục 14.5.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "When a very small model reveals no more errors, you can then run TLC with larger models to try to catch more subtle errors."
[^ss-suspicious]: Lamport, Specifying Systems, mục 14.5.3 "Be Suspicious of Success" — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "It’s very easy to satisfy a safety property by simply doing nothing."
[^ss-sym]: Lamport, Specifying Systems, mục 14.3.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "If there are n processes, this reduces the number of states that TLC examines by a factor of n !."
[^ss-sym-trace]: Lamport, Specifying Systems, mục 14.3.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "If the specification and properties are not symmetric with respect to all permutations in the symmetry set, then TLC may be unable to print an error trace if it does find an error."
[^ss-sym-liveness]: Lamport, Specifying Systems, mục 14.3.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "However, TLC may perform ImpliedTemporal checking incorrectly—it may miss errors, report an error that doesn’t exist, or report a real error with an incorrect counterexample."
[^ss-alg-inv]: Lamport, Specifying Systems, mục 14.3.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "If Invariant is false in state t or ImpliedAction is false for the step s → t, then report an error and stop."
[^ss-coverage]: Lamport, Specifying Systems, mục 14.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "You can discover if an action is never enabled by using the coverage option, described on page 252."
[^ss-sim]: Lamport, Specifying Systems, mục 14.5.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Random simulation is not an effective way to catch subtle errors, but it’s worth trying; you might get lucky."
[^ss-simulate-opt]: Lamport, Specifying Systems, mục 14.5 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Tells TLC to run in simulation mode, generating randomly chosen behaviors, instead of generating all reachable states."
[^lt-detail]: Learn TLA+ [trang sống], Optimizing Model Checking, mục "Reduce Spec Detail" — <https://learntla.com/topics/optimization.html> — "This is the most important heuristic about state space optimization: the more detailed your model, the more states you will have."
[^lt-load]: Learn TLA+ [trang sống], Optimizing Model Checking, mục "Reduce Spec Detail" — <https://learntla.com/topics/optimization.html> — "I often see beginners model things like load \in [Server -> 0..100]. A more experienced modeller would instead write something like load \in [Server -> 0..3]."
[^lt-grain]: Learn TLA+ [trang sống], Optimizing Model Checking, mục "Do more in each action" — <https://learntla.com/topics/optimization.html> — "Choosing too coarse a grain might hide legitimate errors, while choosing too fine a grain might make model checking take too long."
[^lt-small]: Learn TLA+ [trang sống], Optimizing Model Checking, mục "Use smaller constants" — <https://learntla.com/topics/optimization.html> — "Most bugs appear with small state spaces anyway, so there’s not much benefit to using enormous constant inputs."
[^lt-safety-liveness]: Learn TLA+ [trang sống], Optimizing Model Checking, mục "Separate safety and liveness" — <https://learntla.com/topics/optimization.html> — "Liveness checking is a lot slower than safety and it prevents symmetry set optimizations."
[^lt-two-factors]: Learn TLA+ [trang sống], Optimizing Model Checking, mục "The Basics" — <https://learntla.com/topics/optimization.html> — "In my experience, (2) dominates and deserves the most attention."
[^lt-stack]: Learn TLA+ [trang sống], Optimizing Model Checking, mục "Set Sizes" — <https://learntla.com/topics/optimization.html> — "These stack; if you have something like [A \X B -> SUBSET C] in your spec that’s probably the source of your woes."
[^lt-views]: Learn TLA+ [trang sống], Optimizing Model Checking, mục "Use Views" — <https://learntla.com/topics/optimization.html> — "Views can nullify the state-space impact of adding an aux variable, but it’s also really easy to remove valid states, too, so be very careful when you use them."
[^lt-refine]: Learn TLA+ [trang sống], Optimizing Model Checking, mục "Reduce Spec Detail" — <https://learntla.com/topics/optimization.html> — "If you do need to model a detailed system, it’s often best to first write a simple, high-level specification and then put the detailed system in a refinement."
[^lt-loader]: Learn TLA+ [trang sống], Optimizing Model Checking, mục "Remove “loader” processes" — <https://learntla.com/topics/optimization.html> — "The writer is what I call a “loader” process: all it does is manually set up to_process."
[^lt-workers]: Learn TLA+ [trang sống], Optimizing Model Checking, mục "Before you begin" — <https://learntla.com/topics/optimization.html> — "If you instead run TLA+ from the command line, it will default to using 25% RAM and exactly one worker."
[^lt-level-iter]: Learn TLA+ [trang sống], Optimizing Model Checking, mục "The Spec" — <https://learntla.com/topics/optimization.html> — "I’ll add the constraint TLCGet("level") < 12 to my spec."
[^lt-unbound-err]: Learn TLA+ [trang sống], Handling Unbound Models, mục "Bounding Models" — <https://learntla.com/topics/unbound-models.html> — "Most of the time, an unbound model is a spec error: you left a check off somewhere and the model checker exploited that."
[^lt-tlcget]: Learn TLA+ [trang sống], Handling Unbound Models, mục "TLCGet" — <https://learntla.com/topics/unbound-models.html> — "This is also my favorite way to limit the state space of an unbound model: ModelConstraint == TLCGet("level") < 9"
[^lt-bound-warning]: Learn TLA+ [trang sống], Handling Unbound Models, mục "Bounding Models" — <https://learntla.com/topics/unbound-models.html> — "Don’t do this if you’re also checking temporal properties! You’re effectively “cutting off” part of the behavior, so model checker doesn’t have the whole behavior to evaluate liveness with."
[^lt-terminating]: Learn TLA+ [trang sống], Concurrency, mục "await" — <https://learntla.com/core/concurrency.html> — "All processes finishing (reaching the “Done” state) doesn’t cause a deadlock because the PlusCal translator inserts an extra “everything’s complete and nothing happens” action."
[^lt-with-empty]: Learn TLA+ [trang sống], Concurrency, mục "await" — <https://learntla.com/core/concurrency.html> — "with x \in set also blocks if the set is empty."
[^lt-trivial]: Learn TLA+ [trang sống], Concurrency, mục "Finding More Invariants" — <https://learntla.com/core/concurrency.html> — "If we change the spec in a way that lets a thread noop-loop forever, then the invariant trivially passes."
[^lt-queue-mutable]: Learn TLA+ [trang sống], Modelling Message Queues, mục "The Queue" — <https://learntla.com/topics/message-queues.html> — "The one downside is that you can’t write properties that rely on the queue’s history, like “the same message is never enqueued twice”."
[^lt-queue-immutable]: Learn TLA+ [trang sống], Modelling Message Queues, mục "The Queue" — <https://learntla.com/topics/message-queues.html> — "This is harder to work with, adds variable bloat, and makes a lot of simple queue checks (like getting the length) more awkward. The one upside is that you preserve the whole queue history, making generalized properties easier."
[^lt-queue-id]: Learn TLA+ [trang sống], Modelling Message Queues, mục "The Messages" — <https://learntla.com/topics/message-queues.html> — "Having an id field is good practice because it lets you distinguish difference messages with the same content (make sure to have a MaxId constant!)."
[^lt-queue-warning]: Learn TLA+ [trang sống], Modelling Message Queues, mục "The Queue" — <https://learntla.com/topics/message-queues.html> — "Be careful of unbound models with immutable queues! Even if you bound the maximum number of unread messages, the queue can still grow without, as long as you keep reading old messages."
[^lt-amo]: Learn TLA+ [trang sống], Modelling Message Queues, mục "Multiple Reader Queues" — <https://learntla.com/topics/message-queues.html> — "At-most-once delivery can be modeled as delivering to a subset of a valid recipients."
[^kb-safety-liveness]: Kleppmann, "How to do distributed locking" (2016-02-08) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "Only liveness properties depend on timeouts or some other failure detector."
[^kb-pause-lease]: Kleppmann, "How to do distributed locking" (2016-02-08) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "However, if the GC pause lasts longer than the lease expiry period, and the client doesn’t realise that it has expired, it may go ahead and make some unsafe change."
[^kb-perfect]: Kleppmann, "How to do distributed locking" (2016-02-08) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "Unfortunately, even if you have a perfect lock service, the code above is broken."
[^kb-nocheck]: Kleppmann, "How to do distributed locking" (2016-02-08) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "You cannot fix this problem by inserting a check on the lock expiry just before writing back to storage."
[^kb-network-delay]: Kleppmann, "How to do distributed locking" (2016-02-08) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "This means that an application process may send a write request, and it may reach the storage server a minute later when the lease has already expired."
[^kb-fencing]: Kleppmann, "How to do distributed locking" (2016-02-08) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "the storage server remembers that it has already processed a write with a higher token number (34), and so it rejects the request with token 33"
[^kb-fencing-active]: Kleppmann, "How to do distributed locking" (2016-02-08) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "Note this requires the storage server to take an active role in checking tokens, and rejecting any writes on which the token has gone backwards."
[^kb-fencing-consensus]: Kleppmann, "How to do distributed locking" (2016-02-08) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "It’s likely that you would need a consensus algorithm just to generate the fencing tokens."
[^k-ack-problem]: Apache Kafka 3.9.0, design (Consumer Position) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "if the consumer processes the message but fails before it can send an acknowledgement then the message will be consumed twice."
[^rmq-requeue]: RabbitMQ, Consumer Acknowledgements and Publisher Confirms — <https://www.rabbitmq.com/docs/confirms> — "any delivery (message) that was not acked is automatically requeued when the channel (or connection) on which the delivery happened is closed."
[^rmq-idem]: RabbitMQ, Consumer Acknowledgements and Publisher Confirms — <https://www.rabbitmq.com/docs/confirms> — "Due to this behavior, consumers must be prepared to handle redeliveries and otherwise be implemented with idempotence in mind."
[^kleppmann-dedup]: Kleppmann, Distributed Systems lecture notes (Cambridge, 2021–22), mục 5.1 "Manipulating remote state" (tiểu mục "Retrying state updates") — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "However, in a crash-recovery system model, this requires storing requests (or some metadata about requests, such as a vector clock) in stable storage, so that duplicates can be accurately detected even after a crash."
[^kleppmann-rpc]: Kleppmann, Distributed Systems lecture notes (Cambridge, 2021–22), mục 1.3 RPC — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "If the client sends an RPC request but receives no response, it doesn’t know whether or not the server received and processed the request. It could resend the request if it doesn’t hear back for a while, but that might cause the request to be performed more than once (e.g. charging a credit card twice)."
[^kafka-consumer-amo]: Apache Kafka 3.9 documentation, Design, mục Message Delivery Semantics — <https://kafka.apache.org/39/design/design/> — "This corresponds to “at-most-once” semantics as in the case of a consumer failure messages may not be processed."
[^az-order]: Microsoft Learn, Azure Architecture Center, "Cache-Aside pattern" — <https://learn.microsoft.com/en-us/azure/architecture/patterns/cache-aside> — "When an application updates information, it writes the change to the data store and then invalidates the corresponding item in the cache."
[^az-stale-window]: Microsoft Learn, Azure Architecture Center, "Cache-Aside pattern", mục "Staleness after writes" — <https://learn.microsoft.com/en-us/azure/architecture/patterns/cache-aside> — "Between the write and the next read, a reader can experience a cache miss or briefly see stale data."
[^az-bad-order]: Microsoft Learn, Azure Architecture Center, "Cache-Aside pattern", mục ví dụ code — <https://learn.microsoft.com/en-us/azure/architecture/patterns/cache-aside> — "The cache miss causes the application to retrieve the outdated item from the data store and add it back to the cache. This sequence leads to stale data in the cache."
[^az-lifetime]: Microsoft Learn, Azure Architecture Center, "Cache-Aside pattern", mục "Lifetime of cached data" — <https://learn.microsoft.com/en-us/azure/architecture/patterns/cache-aside> — "To make cache-aside effective, ensure that the expiration policy matches the pattern of access for applications that use the data. Don't make the expiration period too short because premature expiration can cause applications to continually retrieve data from the data store and add it to the cache."
[^az-writethrough]: Microsoft Learn, Azure Architecture Center, "Cache-Aside pattern", mục "Staleness after writes" — <https://learn.microsoft.com/en-us/azure/architecture/patterns/cache-aside> — "This behavior distinguishes the Cache-Aside pattern from write-through caching, which updates the data store and the cache in the same write operation so that readers see the new value immediately after a successful write."
[^fb-stale]: Nishtala et al., Scaling Memcache at Facebook (NSDI 2013), mục 3.2.1 Leases — <https://www.usenix.org/system/files/conference/nsdi13/nsdi13-final170_update.pdf> — "A stale set occurs when a web server sets a value in memcache that does not reflect the latest value that should be cached. This can occur when concurrent updates to memcache get reordered."
[^fb-lease]: Nishtala et al., Scaling Memcache at Facebook (NSDI 2013), mục 3.2.1 Leases — <https://www.usenix.org/system/files/conference/nsdi13/nsdi13-final170_update.pdf> — "With the lease token, memcached can verify and determine whether the data should be stored and thus arbitrate concurrent writes."
[^fb-lease-verify]: Nishtala et al., Scaling Memcache at Facebook (NSDI 2013), mục 3.2.1 Leases — <https://www.usenix.org/system/files/conference/nsdi13/nsdi13-final170_update.pdf> — "Verification can fail if memcached has invalidated the lease token due to receiving a delete request for that item."
[^fb-ll-sc]: Nishtala et al., Scaling Memcache at Facebook (NSDI 2013), mục 3.2.1 Leases — <https://www.usenix.org/system/files/conference/nsdi13/nsdi13-final170_update.pdf> — "Leases prevent stale sets in a manner similar to how load-link/storeconditional operates"
[^fb-herd]: Nishtala et al., Scaling Memcache at Facebook (NSDI 2013), mục 3.2.1 Leases — <https://www.usenix.org/system/files/conference/nsdi13/nsdi13-final170_update.pdf> — "A thundering herd happens when a specific key undergoes heavy read and write activity."
[^msio-fail]: Richardson, microservices.io, "Pattern: Transactional outbox" — <https://microservices.io/patterns/data/transactional-outbox.html> — "But without using 2PC, sending a message in the middle of a transaction is not reliable. There’s no guarantee that the transaction will commit. Similarly, if a service sends a message after committing the transaction there’s no guarantee that it won’t crash before sending the message."
[^msio-solution]: Richardson, microservices.io, "Pattern: Transactional outbox" — <https://microservices.io/patterns/data/transactional-outbox.html> — "The solution is for the service that sends the message to first store the message in the database as part of the transaction that updates the business entities. A separate process then sends the messages to the message broker."
[^msio-benefit]: Richardson, microservices.io, "Pattern: Transactional outbox" — <https://microservices.io/patterns/data/transactional-outbox.html> — "Messages are guaranteed to be sent if and only if the database transaction commits"
[^msio-dup]: Richardson, microservices.io, "Pattern: Transactional outbox" — <https://microservices.io/patterns/data/transactional-outbox.html> — "The Message relay might publish a message more than once."
[^aws-dup]: AWS Prescriptive Guidance, Cloud design patterns, "Transactional outbox pattern" — <https://docs.aws.amazon.com/prescriptive-guidance/latest/cloud-design-patterns/transactional-outbox.html> — "The events processing service might send out duplicate messages or events, so we recommend that you make the consuming service idempotent by tracking the processed messages."
[^aws-order]: AWS Prescriptive Guidance, "Transactional outbox pattern" — <https://docs.aws.amazon.com/prescriptive-guidance/latest/cloud-design-patterns/transactional-outbox.html> — "Send messages or events in the same order in which the service updates the database."
[^debezium-outbox-capture]: Debezium 2.7 documentation, "Outbox Event Router" — <https://debezium.io/documentation/reference/2.7/transformations/outbox-event-router.html> — "A Debezium connector that is configured to apply the outbox SMT should capture changes that occur in an outbox table only."
[^abl-sideeffects]: Amazon Builders' Library, "Timeouts, retries and backoff with jitter" — <https://aws.amazon.com/builders-library/timeouts-retries-and-backoff-with-jitter/> — "A timeout or failure doesn't necessarily mean that side effects haven't happened."
[^abl-idem]: Amazon Builders' Library, "Timeouts, retries and backoff with jitter", mục "Deciding when to retry" — <https://aws.amazon.com/builders-library/timeouts-retries-and-backoff-with-jitter/> — "In general, our view is that APIs with side effects aren't safe to retry unless they provide idempotency."
[^abl-backoff]: Amazon Builders' Library, "Timeouts, retries and backoff with jitter" — <https://aws.amazon.com/builders-library/timeouts-retries-and-backoff-with-jitter/> — "To avoid this problem, we implement our clients to use backoff. This increases the time between subsequent retries, which keeps the load on the backend even."
[^stripe-key]: Stripe API Reference, "Idempotent requests" — <https://docs.stripe.com/api/idempotent_requests> — "A client generates an idempotency key, which is a unique key that the server uses to recognize subsequent retries of the same request."
[^ex-cp-intro]: tlaplus/Examples @ 6a7b26a, CheckpointCoordination/README.md — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/CheckpointCoordination/README.md> — "There are five replicas in a Ring Master cluster, one of which is elected primary at any given time."
[^ex-cp-failure]: tlaplus/Examples @ 6a7b26a, CheckpointCoordination/README.md, mục Failure Model — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/CheckpointCoordination/README.md> — "Replicas crashing, then later recovering * Network links failing between any two replicas in the cluster, then recovering * The rate of passage of time differing between replicas"
[^ex-cp-12]: tlaplus/Examples @ 6a7b26a, CheckpointCoordination/README.md — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/CheckpointCoordination/README.md> — "The TLC model checker found a 12-step error trace for a very subtle bug introduced by an attempted optimization"
[^ex-cp-failcfg]: tlaplus/Examples @ 6a7b26a, CheckpointCoordination/README.md — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/CheckpointCoordination/README.md> — "The `MCCheckpointCoordinationFailure.cfg` model reproduces the above error trace."
[^ex-cp-manifest]: tlaplus/Examples @ 6a7b26a, CheckpointCoordination/manifest.json — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/CheckpointCoordination/manifest.json> — "path": "specifications/CheckpointCoordination/MCCheckpointCoordinationFailure.cfg", "runtime": "00:00:10", "mode": "exhaustive search", "result": "safety failure"
[^ex-cp-timeout]: tlaplus/Examples @ 6a7b26a, CheckpointCoordination/CheckpointCoordination.tla, hành động TriggerTimeout — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/CheckpointCoordination/CheckpointCoordination.tla> — "TriggerTimeout == /\ \E n \in Node : ReadLog(n, TimeoutCounter) /= NoNode /\ TimeoutCounter' = TimeoutCounter + 1"
[^ex-cp-next]: tlaplus/Examples @ 6a7b26a, CheckpointCoordination/CheckpointCoordination.tla, định nghĩa Next — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/CheckpointCoordination/CheckpointCoordination.tla> — "Next == \/ \E n \in Node : NodeFailure(n) \/ \E n \in Node : NodeRecovery(n) \/ \E src, dst \in Node : NetworkFailure(src, dst)"
[^ex-chameneos-cfg]: tlaplus/Examples @ 6a7b26a, Chameneos/Chameneos.cfg — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/Chameneos/Chameneos.cfg> — "INVARIANTS TypeOK SumMet SPECIFICATION Spec CHECK_DEADLOCK FALSE"
[^ex-kvs-sym]: tlaplus/Examples @ 6a7b26a, KeyValueStore/MCKVSSafetyMedium.cfg — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/KeyValueStore/MCKVSSafetyMedium.cfg> — "SYMMETRY TxIdSymmetric SPECIFICATION Spec INVARIANTS TypeInvariant TxLifecycle"
[^ex-kvs-small]: tlaplus/Examples @ 6a7b26a, KeyValueStore/manifest.json — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/KeyValueStore/manifest.json> — "path": "specifications/KeyValueStore/MCKVSSafetySmall.cfg", "runtime": "00:00:40", "mode": "exhaustive search", "result": "success"
[^ex-kvs-medium]: tlaplus/Examples @ 6a7b26a, KeyValueStore/manifest.json — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/KeyValueStore/manifest.json> — "path": "specifications/KeyValueStore/MCKVSSafetyMedium.cfg", "runtime": "00:04:30", "mode": "exhaustive search", "result": "success"
[^ex-kvs-large]: tlaplus/Examples @ 6a7b26a, KeyValueStore/manifest.json — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/KeyValueStore/manifest.json> — "path": "specifications/KeyValueStore/MCKVSSafetyLarge.cfg", "runtime": "07:16:00", "mode": "exhaustive search", "result": "success"
[^ex-kvs-small-cfg]: tlaplus/Examples @ 6a7b26a, KeyValueStore/MCKVSSafetySmall.cfg — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/KeyValueStore/MCKVSSafetySmall.cfg> — "Key = {k1, k2} Val = {v1, v2} TxId = {t1, t2, t3}"
[^ex-kvs-medium-cfg]: tlaplus/Examples @ 6a7b26a, KeyValueStore/MCKVSSafetyMedium.cfg — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/KeyValueStore/MCKVSSafetyMedium.cfg> — "Key = {k1, k2, k3} Val = {v1, v2, v3} TxId = {t1, t2}"
[^ex-kvs-large-cfg]: tlaplus/Examples @ 6a7b26a, KeyValueStore/MCKVSSafetyLarge.cfg — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/KeyValueStore/MCKVSSafetyLarge.cfg> — "Key = {k1, k2, k3} Val = {v1, v2} TxId = {t1, t2, t3}"
[^ap-symbolic]: Apalache documentation [trang sống], Getting Started — <https://apalache-mc.org/docs/apalache/index.html> — "Whereas TLC enumerates the states produced by the behaviors of a TLA+ specification, Apalache translates the verification problem to a set of logical constraints."
[^ap-smt]: Apalache documentation [trang sống], Five minutes of theory — <https://apalache-mc.org/docs/apalache/theory.html> — "This allows us to explore bounded executions with an SMT solver. Z3 is the default backend, and CVC5 is also available as an experimental backend."
[^ap-depends]: Apalache documentation [trang sống], Getting Started — <https://apalache-mc.org/docs/apalache/index.html> — "Depending on the specification you wrote, either TLC or Apalache may be more efficient in checking it."
[^ap-experimental]: Apalache documentation [trang sống], Getting Started — <https://apalache-mc.org/docs/apalache/index.html> — "While TLC is a mature tool, Apalache is still experimental, so be prepared to use the command-line and to help us discover bugs."
[^ap-assume]: Apalache documentation [trang sống], Getting Started, mục Assumptions — <https://apalache-mc.org/docs/apalache/index.html> — "As in TLC, all specification parameters are fixed and finite, i.e., the system state is initialized with integers, finite sets, and functions of finite domains and co-domains."
[^ap-assume-exec]: Apalache documentation [trang sống], Getting Started, mục Assumptions — <https://apalache-mc.org/docs/apalache/index.html> — "Only finite executions of bounded length are analyzed."
[^ap-theory]: Apalache documentation [trang sống], Five minutes of theory — <https://apalache-mc.org/docs/apalache/theory.html> — "It automatically extracts symbolic transitions from the specification. This allows us to partition the action Next into a disjunction of simpler actions A_1, ..., A_n."
[^ap-theory-enum]: Apalache documentation [trang sống], Five minutes of theory — <https://apalache-mc.org/docs/apalache/theory.html> — "Importantly, the values for the states s_0, …, s_k are not enumerated as in TLC, but have to be found by the SMT solver."
[^ap-length]: Apalache documentation [trang sống], Running the Tool — <https://apalache-mc.org/docs/apalache/running.html> — "--length specifies the maximal number of Next steps, 10 by default"
[^ap-bmc-incomplete]: Apalache documentation [trang sống], Running the Tool, mục 1.4 Bounded model checking — <https://apalache-mc.org/docs/apalache/running.html> — "If a bug needs a long execution to get revealed, bounded model checking may miss it!"
[^ap-ind]: Apalache documentation [trang sống], Running the Tool, mục 1.5 — <https://apalache-mc.org/docs/apalache/running.html> — "(1) The initial states satisfy the constraint TypeOK /\ IndInv, and (2) whenever the specification makes a step when starting in a state that satisfies TypeOK /\ IndInv, it ends up in a state that again satisfies TypeOK /\ IndInv."
[^ap-indcmds]: Apalache documentation [trang sống], Running the Tool, mục 1.5 — <https://apalache-mc.org/docs/apalache/running.html> — "IndProp: Check that all states captured with IndInv satisfy the predicate Safety:"
[^ap-ind-weak]: Apalache documentation [trang sống], Running the Tool, mục 1.5 — <https://apalache-mc.org/docs/apalache/running.html> — "It may happen that your inductive invariant IndInv is too weak and it violates Safety. In this case, you would have to add additional constraints to IndInv."
[^ap-types]: Apalache documentation [trang sống], How to write type annotations — <https://apalache-mc.org/docs/HOWTOs/howto-write-type-annotations.html> — "You have to write type annotations for CONSTANTS and VARIABLES."
[^ap-assign]: Apalache documentation [trang sống], Assignments and symbolic transitions — <https://apalache-mc.org/docs/apalache/principles/assignments.html> — "For every variable x declared with VARIABLE, there is exactly one assignment of the form x' := e in every symbolic transition A_n."
[^ap-constops]: Apalache documentation [trang sống], Supported features — <https://apalache-mc.org/docs/apalache/features.html> — "Either define a ConstInit operator to initialize the constants, use a .cfg file, or declare operators instead of constants, e.g., C1 == 111"
[^ap-seq]: Apalache documentation [trang sống], Known issues, mục "Using Seq(S)" — <https://apalache-mc.org/docs/apalache/known-issues.html> — "The operator Seq(S) produces an infinite set of unbounded sequences."
[^ap-gen]: Apalache documentation [trang sống], Known issues, mục "Using Seq(S)" — <https://apalache-mc.org/docs/apalache/known-issues.html> — "In the above example, we instruct Apalache to introduce an unrestricted sequence that contains up to 10 elements; this is done with Gen."
[^ap-range]: Apalache documentation [trang sống], Known issues — <https://apalache-mc.org/docs/apalache/known-issues.html> — "When using an integer range a..b, where a or b aren’t constant (or cannot be simplified to a constant), the current encoding fails"
[^ap-features]: Apalache documentation [trang sống], Supported features, mục Logic — <https://apalache-mc.org/docs/apalache/features.html> — "Partial support prior to version 0.16.1. From 0.16.1 and later, use Some, ApaFoldSet, or ApaFoldSeqLeft."
[^ap-pluscal]: Apalache documentation [trang sống], Tutorial on checking PlusCal specifications — <https://apalache-mc.org/docs/tutorials/pluscal-tutorial.html> — "The most straightforward approach would be to add type annotations directly in the PlusCal code. As reported in Issue 1412, this does not work as expected, as the PlusCal translator erases the comments."
[^ap-instance]: Apalache documentation [trang sống], Tutorial on checking PlusCal specifications — <https://apalache-mc.org/docs/tutorials/pluscal-tutorial.html> — "Due to the semantics of INSTANCE, the constants and variables declared in BakeryTyped.tla substitute the constants and variables of BakeryWoTlaps.tla. By doing so, we effectively introduce type annotations. Since we introduce a separate module, any changes in the PlusCal code do not affect our type annotations."
[^ap-bakery-time]: Apalache documentation [trang sống], Tutorial on checking PlusCal specifications, Step 3 — <https://apalache-mc.org/docs/tutorials/pluscal-tutorial.html> — "It took me 0 days 0 hours 32 min 2 sec Apalache reports no violation of MutualExclusion. This is a good start. However, since Apalache only analyzes executions that make up to 10 transitions by default, this analysis is incomplete."
[^ap-liveness]: Apalache documentation [trang sống], Supported features — <https://apalache-mc.org/docs/apalache/features.html> — "To check liveness/temporal properties, we employ a liveness-to-safety transformation."
[^ap-constinit]: Apalache documentation [trang sống], Running the Tool, mục 2.3 — <https://apalache-mc.org/docs/apalache/running.html> — "ConstInit == BIRTH_YEAR \in 0..99 /\ LICENSE_AGE \in 10..99 In this case, Apalache finds a safety violation, e.g., for BIRTH_YEAR=89 and LICENSE_AGE=10."
[^ap-simulate]: Apalache documentation [trang sống], Running the Tool — <https://apalache-mc.org/docs/apalache/running.html> — "This command usually checks randomized symbolic runs much faster than the check command."
[^ap-inv-kinds]: Apalache documentation [trang sống], Invariants: state, action, and trace — <https://apalache-mc.org/docs/apalache/principles/invariants.html> — "Recently, we have added support for action invariants and trace invariants."
[^ap-trace-warning]: Apalache documentation [trang sống], Invariants: state, action, and trace — <https://apalache-mc.org/docs/apalache/principles/invariants.html> — "However, we recommend using trace invariants for testing, as they are too powerful. For verification, you should use temporal properties."
[^rust-fetch-max]: Rust 1.95.0, std::sync::atomic::AtomicU64::fetch_max — <https://doc.rust-lang.org/1.95.0/std/sync/atomic/struct.AtomicU64.html> — "Finds the maximum of the current value and the argument val, and sets the new value to the result. Returns the previous value."
[^tokio-timeout]: docs.rs tokio 1.53.2, tokio::time::timeout — <https://docs.rs/tokio/1.53.2/tokio/time/fn.timeout.html> — "If the future completes before the duration has elapsed, then the completed value is returned. Otherwise, an error is returned and the future is canceled."
[^aws-code-help]: Newcombe et al., How Amazon Web Services Uses Formal Methods, mục "The Most Frequently Asked Question" — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf> — "Formal methods help engineers to get the design right, which is a necessary first step toward getting the code right."
