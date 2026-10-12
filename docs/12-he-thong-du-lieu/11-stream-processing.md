# P12 · Chương 11 — Stream processing: log, delivery semantics, CDC, window và watermark

> Chương này dùng khung chủ đề của sách *Designing Data-Intensive Applications* (DDIA) làm xương sống, nhưng **không trích và không viết lại nội dung sách** (sách không có bản đọc tự do). Mọi khẳng định dựa vào nguồn mở ghi ở chú thích: docs Apache Kafka 3.9.0 (ghim theo tag GitHub), Kafka Streams, docs Apache Flink 1.20, RabbitMQ, PostgreSQL 16, Debezium 2.7, bài *The Dataflow Model* (VLDB 2015), bài giảng *Distributed Systems* của Kleppmann (Cambridge), bài *End-to-End Arguments* (Saltzer–Reed–Clark), docs.rs `rdkafka` 0.39.0 và `tokio` 1.53.2. **Chương không có thí nghiệm**: không con số hiệu năng nào ở đây do người soạn đo.

## Bạn sẽ hiểu được gì

- **Q1.** Event, stream, producer và consumer là gì; "unbounded" khác "streaming" ra sao? → mục 1
- **Q2.** Message broker kiểu *queue* (xóa sau khi ack) khác kiểu *log* (giữ lại, consumer tự giữ vị trí) ở đâu, và mỗi kiểu hợp việc gì? → mục 2
- **Q3.** Trong một log kiểu Kafka, partition, key, offset, consumer group, retention và log compaction là gì và ăn khớp với nhau thế nào? → mục 3
- **Q4.** At-most-once, at-least-once, exactly-once khác nhau thế nào; idempotent producer và transaction của Kafka giải quyết đến đâu, và vì sao "exactly-once" với hệ ngoài vẫn cần thêm việc? → mục 4
- **Q5.** Change data capture (CDC) dựa vào PostgreSQL logical decoding hoạt động ra sao; event sourcing khác CDC thế nào; "stream–table duality" là gì? → mục 5
- **Q6.** Event time khác processing time thế nào; tumbling/hopping/session window là gì; watermark làm gì và vì sao không bao giờ đủ? → mục 6
- **Q7.** Stream join (stream–stream, stream–table, table–table) cần gì và dễ sai ở đâu? → mục 7
- **Q8.** Một job stream có state thì chịu lỗi bằng cách nào (checkpoint, changelog, replay)? → mục 8

## Kiến thức cần trước

- [Chương 05 — Replication](05-replication.md): replication log, leader/follower. Chương này dùng lại ý "log là nguồn sự thật" mà không giải thích lại cách replicate.
- [Chương 06 — Partitioning](06-partitioning.md): chia dữ liệu theo key; partition của Kafka là một ứng dụng của ý đó.
- [Chương 07 — Transaction](07-transaction.md): atomic commit; transaction của Kafka (mục 4) là một dạng "ghi nhiều nơi cùng thành hoặc cùng bại".
- [Chương 08 — Sự cố hệ phân tán](08-su-co-he-phan-tan.md): mạng mất/trễ/trùng gói, đồng hồ không đáng tin — gốc của mọi vấn đề *trùng lặp* và *sai thứ tự* ở chương này.
- [P5 · Chương 05 — Crash consistency và journaling](../05-storage/05-crash-consistency-va-journaling.md) (WAL) và [P5 · Chương 06 — Durability](../05-storage/06-durability-trong-thuc-te.md): "ghi vào log trước" và `fsync`; chương này không lặp lại.

Bạn đã quen với một service backend ghi vào database; chương này nói về chuyện xảy ra *sau* cú ghi đó, khi nhiều hệ khác cũng cần biết dữ liệu vừa đổi. Code Rust chỉ xuất hiện ở phần "Liên hệ với Rust".

(Batch processing đối chiếu ở [Chương 10](10-batch-processing.md); ghép stream vào kiến trúc tổng thể và dữ liệu dẫn xuất ở [Chương 12](12-dung-dan-va-tich-hop-du-lieu.md); warehouse/lake/ETL ở [Chương 13](13-kien-truc-he-du-lieu.md); xóa dữ liệu cá nhân và GDPR ở [Chương 14](14-trach-nhiem-voi-du-lieu.md). Các chủ đề đó chỉ được nhắc 1–2 câu ở đây.)

## 1. Event, stream, producer, consumer (Q1)

> **Trả lời ngắn.** *Event* là bản ghi rằng "một chuyện đã xảy ra" trong thế giới hoặc trong nghiệp vụ[^k-event]. Trong Kafka, event gồm key, value, timestamp và vài header tùy chọn[^k-event-fields]. *Stream* là chuỗi event cứ nối dài mãi, không biết trước điểm kết thúc: *producer* ghi event vào, *consumer* đọc ra, và hai bên không cần biết nhau[^k-decoupled]. Hai chữ hay bị lẫn là "unbounded" và "streaming": unbounded (không biên) là tính chất của *dữ liệu*, còn streaming chỉ là một loại *engine*, vì engine batch vẫn xử lý được dữ liệu không biên và engine streaming cũng xử lý được dữ liệu hữu hạn[^df-unbounded].

**Giải thích.**

*Với service của bạn.* Hãy hình dung `orders`, service Rust của một shop online. Một đơn hàng đi qua vài trạng thái: tạo, thanh toán, có thể hủy. Ngoài `orders` còn ba bên cần biết mỗi lần đơn đổi trạng thái: `billing` xuất hóa đơn, `search-index` cập nhật trang tìm đơn, `dashboard` đếm số đơn mỗi 5 phút. Ta sẽ bám theo `orders` suốt chương (nhận xét của người soạn: đây là ví dụ giả định, không phải code trong repo). Câu hỏi của mục này: `orders` báo cho ba bên kia bằng cách nào mà không phải biết từng bên là ai?

### 1.1 Event: "đã có chuyện gì"

Bảng `orders` trong database chỉ cho biết hiện giờ đơn #7 đang ở trạng thái PAID. Điều ba bên kia cần là câu chuyện dẫn tới trạng thái đó: đơn #7 được tạo, rồi thanh toán, rồi hủy. Câu chuyện ấy chính là một stream. Có stream thì dựng lại được bảng tại mọi thời điểm, còn có bảng thì không suy ngược ra stream được (nhận xét của người soạn).

Docs Kafka gọi mỗi mẩu của câu chuyện ấy là event, và cũng gọi là record hoặc message[^k-event]. Docs lấy ví dụ một event có key là "Alice", value là khoản thanh toán 200 đô cho Bob, kèm một mốc thời gian[^k-event-example]. Event "đơn #7 đã thanh toán" của `orders` có cùng dạng. Điều đáng nhớ là timestamp nằm *trong* event; mục 6 sẽ cho thấy chính nó là gốc của cả bài toán thời gian.

### 1.2 Producer và consumer

Trong ví dụ này `orders` là producer: nó ghi event "đơn #7 đã thanh toán" lên Kafka rồi xong việc. `billing`, `search-index`, `dashboard` là ba consumer, mỗi bên tự đọc theo nhịp của mình. Kafka coi hai phía là tách rời hoàn toàn, và docs nói đó là một yếu tố thiết kế chính để hệ thống mở rộng được; chẳng hạn producer không bao giờ phải chờ consumer[^k-decoupled].

```mermaid
flowchart LR
  P["orders (producer)"] -->|"ghi event: key, value, timestamp"| T["Topic orders"]
  T -->|"đọc"| C1["billing"]
  T -->|"đọc"| C2["search-index"]
  T -->|"đọc"| C3["dashboard"]
```

*Sơ đồ minh họa của người soạn: `orders` không chờ ba bên kia đọc, và ba bên kia không cần biết `orders` là ai.*

### 1.3 Unbounded và bounded

Dòng event của `orders` là dữ liệu không biên: đơn hàng cứ đến mãi. Bài Dataflow (Google, VLDB 2015) dùng cặp *unbounded/bounded* (vô hạn/hữu hạn) để nói về **dữ liệu**, và để cặp *streaming/batch* cho **engine thực thi**, vì cặp sau ngụ ý một loại engine cụ thể[^df-unbounded-def]. Lý do thực tế là dữ liệu vô hạn từ lâu đã được xử lý bằng cách chạy batch lặp đi lặp lại, và một engine streaming được thiết kế tốt xử lý được cả dữ liệu hữu hạn[^df-unbounded].

Dữ liệu không biên còn kéo theo một khó khăn. Phép gộp theo key (`GroupByKey`) phải gom hết dữ liệu của một key rồi mới đẩy xuống bước sau, mà với đầu vào không biên ta không biết bao giờ dữ liệu hết[^df-groupbykey]. Lối ra thông dụng là chia dữ liệu thành các *window* hữu hạn; đó là chuyện của mục 6.

<details><summary>Đi sâu: vì sao không chỉ "poll bảng database"; batch là trường hợp riêng của stream?</summary>

- Một topic có số producer và consumer tùy ý, kể cả không có[^k-topicmulti]. Vì vậy thêm một consumer mới, ví dụ job báo cáo, không đụng tới code của `orders`. Nếu mỗi consumer tự truy vấn bảng của service khác thì coupling nằm ở schema bảng (nhận xét của người soạn).
- Dataflow mô tả bài toán bằng bốn câu hỏi: *tính cái gì*, *ở đâu trong event time*, *khi nào trong processing time thì phát kết quả*, và *kết quả sớm liên hệ với kết quả hiệu chỉnh sau thế nào*[^df-four]. Ba câu sau là nội dung mục 6; câu đầu là phép biến đổi thông thường.
- Cũng bài đó nhận xét rằng với góc nhìn của mô hình, batch hay streaming "phần lớn không liên quan": chỉ là chọn engine theo độ trễ và chi phí[^df-unbounded-def]. [Chương 10](10-batch-processing.md) nói batch từ phía kia.

</details>

**Quay lại Q1:** event là bản ghi "đã có chuyện"; stream là chuỗi event không biên mà producer ghi, consumer đọc, tách rời nhau (`orders` ghi, ba bên kia đọc); "streaming" là kiểu engine, "unbounded" là kiểu dữ liệu.

## 2. Queue hay log: hai kiểu message broker (Q2)

> **Trả lời ngắn.** Broker kiểu *queue* (ví dụ RabbitMQ) chia việc cho các consumer, đợi ack rồi bỏ message khỏi hàng đợi[^rmq-ack-discard]. Broker kiểu *log* (Kafka) không xóa event sau khi tiêu thụ mà giữ theo thời hạn cấu hình[^k-notdeleted]. Mỗi consumer tự giữ vị trí đọc của mình[^k-offset-one]. Vì vậy queue hợp với việc "giao một lần cho một worker", còn log hợp khi nhiều bên cùng đọc, cần đọc lại, hoặc cần dựng lại state.

**Giải thích.**

*Với service `orders`.* Đơn #7 vừa thanh toán và `orders` có hai việc phải giao đi. Việc thứ nhất là gửi email xác nhận: chỉ cần *một* worker làm, làm xong là hết. Việc thứ hai là báo cho `billing`, `search-index` và `dashboard`: cả ba đều phải thấy đơn này, và tuần sau nếu có bên thứ tư thì nó còn muốn đọc lại cả lịch sử. Hai nhu cầu đó kéo về hai loại broker khác nhau (nhận xét của người soạn).

### 2.1 Kiểu queue

Gửi email là việc chậm, nên `orders` không làm ngay trong lúc xử lý request mà đóng nó thành một message, bỏ vào hàng đợi cho worker chạy nền. Đó là ý chính của work queue (RabbitMQ gọi là task queue)[^rmq-workq]. Mặc định RabbitMQ chuyển message cho consumer kế tiếp theo vòng, nên trung bình các worker nhận số message bằng nhau; cách chia này gọi là round-robin[^rmq-rr]. Khi worker xử lý xong và báo ack (xác nhận), broker ghi nhận message đã giao và có thể bỏ nó[^rmq-ack-discard].

Worker chết khi chưa ack thì sao? Message được đưa lại vào hàng đợi (requeue) khi kênh hoặc kết nối của worker đó đóng[^rmq-requeue]. Vì vậy docs yêu cầu consumer phải chịu được việc nhận lại cùng một message, và nên viết theo hướng idempotent[^rmq-idem].

### 2.2 Kiểu log

Với ba bên kia, queue không hợp: message bị bỏ sau ack thì `billing` lấy xong là `search-index` hết thấy. Kafka đi hướng khác. Nó lưu event trong topic, và khác với hệ messaging truyền thống, event không bị xóa sau khi tiêu thụ[^k-notdeleted]. Event chỉ bị bỏ khi quá thời hạn *retention*, cấu hình riêng cho từng topic[^k-retain]. Vì không xóa theo consumer, broker khỏi phải nhớ "message nào đã giao cho ai".

Docs Kafka giải thích vì sao điều đó đáng giá. Phần lớn hệ messaging giữ metadata "đã tiêu thụ" ngay ở broker[^k-broker-state]. Cách đó hỏng theo hai kiểu: nếu broker đánh dấu "đã tiêu thụ" ngay lúc gửi thì consumer chết giữa chừng làm mất message; còn nếu thêm ack thì consumer xử lý xong nhưng chết trước khi gửi ack sẽ khiến message bị tiêu thụ hai lần[^k-ack-problem]. Kafka tránh cả hai bằng cách khác: mỗi partition chỉ có một consumer trong mỗi consumer group tại một thời điểm[^k-group-excl] (partition và group nói kỹ ở mục 3). Nhờ vậy vị trí đọc chỉ cần là một số nguyên cho mỗi partition, tức offset của message kế tiếp[^k-offset-one].

### 2.3 Hai kiểu so với nhau

| | Queue (kiểu RabbitMQ) | Log (kiểu Kafka) |
|---|---|---|
| Sau khi tiêu thụ | message bị bỏ khi được ack[^rmq-ack-discard] | event vẫn còn tới hết retention[^k-notdeleted] |
| Ai nhớ vị trí | broker theo dõi ack | consumer giữ offset cho mỗi partition[^k-offset-one] |
| Đọc lại | không phải hợp đồng của work queue cổ điển (nhận xét của người soạn); RabbitMQ có thêm loại *stream*, là log append-only đọc lại được tới khi hết hạn[^rmq-streams] | consumer tua lại offset cũ và đọc lại được[^k-rewind] |
| Chia tải | message lần lượt tới các consumer[^rmq-rr] | mỗi partition một consumer trong group[^k-group-excl] |
| Consumer chậm | hàng đợi dài ra (nhận xét của người soạn) | consumer tụt lại rồi đuổi kịp khi có thể[^k-pull] |

Dòng cuối của bảng là chuyện *pull* so với *push*. Kafka chọn mô hình pull, tức consumer chủ động kéo dữ liệu, vì push dễ làm consumer quá tải khi tốc độ tiêu thụ thấp hơn tốc độ sinh; pull thì để consumer tụt lại và đuổi kịp khi có thể[^k-pull].

```mermaid
flowchart TB
  subgraph Q["Kiểu queue"]
    direction LR
    PQ["producer"] --> QQ["hàng đợi: m3 m2 m1"]
    QQ -->|"m1 (ack rồi xóa)"| W1["worker 1"]
    QQ -->|"m2"| W2["worker 2"]
  end
  subgraph L["Kiểu log"]
    direction LR
    PL["producer"] --> LL["log: m1 m2 m3 m4 ... (giữ tới hết retention)"]
    LL -.->|"offset 4"| R1["consumer A"]
    LL -.->|"offset 2"| R2["consumer B (tụt lại, hoặc tua lại)"]
  end
```

*Sơ đồ minh họa của người soạn.*

<details><summary>Đi sâu: vì sao log "rẻ" — pagecache, ghi tuần tự, sendfile; và cái giá</summary>

- Kafka ghi mọi thứ vào log trên filesystem mà không nhất thiết flush xuống đĩa, nghĩa là thực chất dữ liệu chỉ mới vào pagecache của kernel[^k-pagecache]. Cách làm này dựa vào page cache của OS: xem [P5 · Chương 04](../05-storage/04-page-cache-va-direct-io.md), còn `fsync` ở [P5 · Chương 06](../05-storage/06-durability-trong-thuc-te.md).
- Docs lập luận rằng log (đọc và append tuần tự) có mọi thao tác O(1), và đọc không chặn ghi cũng không chặn nhau[^k-O1]. Nhờ vậy Kafka giữ được message khá lâu, ví dụ một tuần, thay vì xóa ngay khi tiêu thụ[^k-retain-week].
- Khi gửi từ pagecache ra socket, Kafka dùng `sendfile` để khỏi chép dữ liệu qua lại: `sendfile` cho OS gửi thẳng từ pagecache ra mạng[^k-sendfile]. Cơ chế này giải thích ở [P6 · Chương 09](../06-networking/09-zero-copy-io.md).
- Cái giá (suy luận của người soạn từ "mỗi partition một consumer trong group"[^k-group-excl]): số consumer *hữu ích* trong một group bị chặn trên bởi số partition. Ngoài ra một message "độc" (xử lý luôn lỗi) sẽ chặn các message sau nó *trong cùng partition*, trừ khi consumer bỏ qua hoặc chuyển nó sang nơi khác (dead-letter).
- Broker log vẫn cần *ack ở phía producer*, tức mức độ bền khi ghi (mục 4).
- Docs RabbitMQ được dẫn ở mục này là bản ghi *Version 4.3* trên trang; URL không mang số phiên bản.

</details>

**Quay lại Q2:** queue = broker nhớ ai đã nhận gì và xóa sau ack; log = broker giữ một chuỗi append-only, consumer tự giữ offset. Với `orders`, email xác nhận hợp với queue, còn dòng event cho `billing`, `search-index`, `dashboard` hợp với log.

## 3. Bên trong một log kiểu Kafka (Q3)

> **Trả lời ngắn.** Một topic chia thành nhiều *partition*, mỗi partition là một log append-only. Event cùng key đi vào cùng partition[^k-partition]. Chúng được đọc đúng thứ tự ghi, nhưng thứ tự chỉ được bảo đảm *trong một partition*[^k-partition-order]. Vị trí đọc của consumer là *offset*, và các consumer chung một *consumer group* chia nhau các partition. Dữ liệu bị xóa khi quá *retention* (theo thời gian hoặc kích thước), hoặc được *compact* để giữ ít nhất giá trị mới nhất của mỗi key[^k-compaction].

**Giải thích.**

*Với service `orders`.* Giả sử topic `orders` nhận nhiều event đến mức một máy không gánh nổi, nên ta chia nó thành 3 partition. Hai câu hỏi nảy ra ngay. Thứ nhất, các event của cùng một đơn (tạo, rồi thanh toán, rồi hủy) có còn đúng thứ tự không? Thứ hai, nếu `billing` chạy hai instance thì ai đọc partition nào? Mục này trả lời cả hai, rồi nói thêm event nằm lại trong log bao lâu.

### 3.1 Partition và key

Topic được trải ra nhiều partition đặt trên các broker khác nhau, để client đọc ghi song song với nhiều broker. Khi một event được publish, nó được append vào *một* partition của topic[^k-partition]. Event cùng key (ví dụ ID khách hàng) vào cùng partition, và consumer của một topic-partition luôn đọc event của partition đó đúng thứ tự đã ghi[^k-partition-order].

Ai chọn partition? Client chọn: ngẫu nhiên, hoặc theo một hàm phân vùng băm key. Docs lấy ví dụ key là user id, để mọi dữ liệu của một user vào cùng một partition[^k-keypartition]. Với `orders`, nếu lấy ID đơn hàng làm key thì ba event của đơn #7 luôn nằm trong cùng partition và được đọc theo thứ tự tạo, thanh toán, hủy (nhận xét của người soạn).

Chia theo key là ý của [Chương 06](06-partitioning.md). Điều chương này thêm là: *thứ tự theo key được bảo toàn, thứ tự toàn cục thì không* (suy luận của người soạn từ câu "đúng thứ tự trong partition" ở nguồn). Event của đơn #7 và đơn #8 nằm ở hai partition khác nhau thì không có thứ tự chung nào giữa chúng.

### 3.2 Offset

Offset là số thứ tự của message trong một partition, và Kafka dùng nó làm id của message. Docs giải thích Kafka đã chọn một bộ đếm nguyên tử theo partition thay vì GUID do producer sinh, để khỏi phải dựng cấu trúc chỉ mục nặng[^k-offset-id]. Consumer gửi yêu cầu *fetch* tới broker đang là leader của partition, kèm offset muốn đọc[^k-fetch]. Vì vậy consumer tự kiểm soát vị trí đọc của mình và tua lại được khi cần[^k-rewind]. Chẳng hạn `dashboard` phát hiện hôm qua mình đếm sai: sửa code xong, nó tua offset về đầu hôm qua và đọc lại (nhận xét của người soạn).

### 3.3 Consumer group

Mỗi consumer khai `group.id` để cho biết nó thuộc group nào[^kc-group-id]. Trong một group, mỗi partition do đúng một consumer đọc tại một thời điểm[^k-group-excl]. Với `orders`, group `billing` có hai consumer và group `search-index` có một. Mỗi group giữ offset riêng và đọc toàn bộ topic, độc lập với group kia (suy luận của người soạn từ "mỗi partition một consumer trong mỗi group đăng ký").

Kafka Streams, thư viện xử lý stream của Kafka, lấy chính quy tắc này làm mô hình song song. Nó tạo một số task cố định, tính theo số partition đầu vào, và giao mỗi task một danh sách partition[^ks-tasks]. Vì vậy số instance chạy song song tối đa bị chặn bởi số partition của topic đầu vào; ví dụ topic 5 partition thì chạy tối đa 5 instance[^ks-maxpar].

```mermaid
flowchart LR
  subgraph T["topic orders (3 partition)"]
    P0["P0: o1 o4 o7 ..."]
    P1["P1: o2 o5 ..."]
    P2["P2: o3 o6 ..."]
  end
  subgraph G1["group 'billing' (2 consumer)"]
    A["consumer A"]
    B["consumer B"]
  end
  subgraph G2["group 'search-index' (1 consumer)"]
    C["consumer C"]
  end
  P0 --> A
  P1 --> A
  P2 --> B
  P0 -.-> C
  P1 -.-> C
  P2 -.-> C
```

*Sơ đồ minh họa của người soạn: mỗi group có offset riêng và đọc toàn bộ topic độc lập với group khác (suy luận của người soạn từ "mỗi partition một consumer trong mỗi group đăng ký").*

### 3.4 Rebalance

Khi thành viên của group đổi (một consumer chết, hoặc ta thêm instance), các partition được chia lại; việc đó gọi là *rebalance*. Một consumer bị coi là hỏng nếu không gọi `poll()` trước khi hết `max.poll.interval.ms`, và khi đó group rebalance để giao partition của nó cho thành viên khác[^kc-poll-interval].

Rebalance đắt nhất với ứng dụng có state lớn. Khi deploy hay restart, các task bị chuyển sang instance khác phải mất thời gian phục hồi state cục bộ trước khi xử lý tiếp, nên ứng dụng có thể bị gián đoạn một phần hoặc toàn phần[^k-static]. Để tránh rebalance không cần thiết, Kafka có *static membership*: đặt `group.instance.id` cố định cho từng instance.

### 3.5 Retention và compaction

Dữ liệu không bị xóa theo consumer mà theo chính sách của topic. Việc xóa làm theo từng *segment*, tức từng file log một[^k-impl-delete]. Ta chọn xóa theo thời gian hoặc theo kích thước. Chính sách theo thời gian dựa vào timestamp lớn nhất trong segment để quyết định cả segment hết hạn chưa[^k-impl-retention]. Chính sách theo kích thước (mặc định tắt) xóa segment cũ nhất cho tới khi partition nằm trong giới hạn[^k-impl-size].

Có một loại log cần cách giữ khác: dòng thay đổi của dữ liệu có key, ví dụ bảng email người dùng. Với log đó, ta thay retention theo thời gian bằng *log compaction*, bảo đảm giữ ít nhất giá trị cập nhật cuối của mỗi key[^k-compaction]. Bản ghi có key và payload null gọi là *tombstone*, đánh dấu xóa[^k-tombstone]. Compaction không đổi thứ tự message, chỉ bỏ bớt một số[^k-compact-order]. Offset cũng giữ nguyên, vì nó là định danh vĩnh viễn của một vị trí trong log[^k-compact-offsets].

Docs nêu ba cách dùng compaction, và cả ba quay lại ở mục 5 và 8:

- đăng ký theo dõi thay đổi của database, để mỗi thay đổi được phản ánh sang cache, search cluster và nơi khác[^k-compact-cdc];
- event sourcing, tức dùng log thay đổi làm nơi lưu chính của ứng dụng[^k-compact-es];
- journaling để phục hồi state: một tiến trình ghi lại mọi thay đổi của state cục bộ, để tiến trình khác nạp lại và chạy tiếp nếu nó chết[^k-compact-uses].

### 3.6 Replication và "committed"

Đơn vị replicate là partition: mỗi partition có một leader và các follower[^k-repl-unit]. Một message được coi là *committed* khi mọi replica trong ISR (tập replica đang đồng bộ) đã áp dụng nó vào log[^k-committed]. Consumer chỉ được giao message đã committed[^k-committed-only]. Message committed không mất miễn là còn ít nhất một replica đồng bộ còn sống[^k-committed-lost]. Giao thức replicate log và bầu leader giải thích ở [Chương 05](05-replication.md) và [Chương 09](09-consistency-va-consensus.md).

<details><summary>Đi sâu: ISR không phải "mọi replica", static membership, timestamp, chọn số partition</summary>

- **ISR không phải "mọi replica được gán".** Với `acks=all`, ack được trả ngay khi mọi replica *hiện đang* trong ISR đã nhận message[^k-acks-all-isr]. Nếu producer đòi ack yếu hơn thì message vẫn có thể được commit và consume kể cả khi số replica đồng bộ thấp hơn mức tối thiểu của topic, thấp nhất là chỉ còn leader[^k-min-isr].
- **Static membership:** docs yêu cầu broker và client từ 2.3 trở lên, và mỗi instance một `group.instance.id` riêng. Nếu hai instance trùng id, broker "fence" instance trùng bằng `FencedInstanceIdException`[^k-static-detail].
- **Timestamp của record:** từ Kafka 0.10, timestamp được nhúng sẵn vào message; tùy cấu hình broker, nó đại diện *event time* hoặc *ingestion time*, và chọn ở cấu hình Kafka chứ không phải ở Kafka Streams[^ks-ts-choice]. Điều này nối sang mục 6.
- **Thứ tự và timestamp không đồng nhất:** trong một partition, timestamp của record có thể không tăng đơn điệu theo offset[^ks-oo-sources]. Nên "thứ tự theo offset" và "thứ tự theo thời gian" là hai thứ khác nhau.
- **Chọn số partition** (nhận xét của người soạn): số partition cố định mức song song tối đa của một group (ở trên), nên chọn dư một chút ngay từ đầu. Đổi số partition sau này làm đổi ánh xạ key → partition, một chủ đề của [Chương 06](06-partitioning.md).

</details>

**Quay lại Q3:** topic = N log append-only (partition); key quyết định partition và bảo toàn thứ tự theo key (với `orders`: mọi event của một đơn nằm chung partition); offset là vị trí đọc; group chia partition cho consumer; retention hoặc compaction quyết định phần nào của log còn lại.

## 4. At-most-once, at-least-once, exactly-once (Q4)

> **Trả lời ngắn.** *At-most-once*: message có thể mất nhưng không bao giờ giao lại[^k-three]. *At-least-once*: không mất nhưng có thể giao lại, tức bị trùng[^k-three-2]. *Exactly-once*: mỗi message giao đúng một lần, điều mà mọi người thực sự muốn[^k-three-3]. Mất hay trùng đều bắt nguồn từ chỗ: sau khi mạng lỗi, ta *không biết* yêu cầu trước đã thành công hay chưa[^k-prod-unsure]. Kafka có producer idempotent và transaction để đạt exactly-once *bên trong Kafka*; với hệ bên ngoài thì hệ đó phải cộng tác, bằng cách ghi idempotent hoặc ghi offset cùng chỗ với kết quả[^k-external-note].

**Giải thích.**

*Với service `orders`.* `billing` đọc event "đơn #7 đã thanh toán" rồi ghi hóa đơn vào database của nó. Giả sử `billing` ghi xong hóa đơn rồi chết trước khi kịp lưu offset: khi khởi động lại nó đọc lại event của đơn #7 và xuất hóa đơn lần nữa. Ngược lại, nếu nó lưu offset *trước* rồi mới ghi hóa đơn, và chết ở giữa, thì đơn #7 vĩnh viễn không có hóa đơn. Mục này đi từ hai kịch bản đó tới câu hỏi: làm sao để hóa đơn có đúng một bản?

### 4.1 Ba mức bảo đảm, và vì sao khó

Docs Kafka tách bài toán thành hai: bảo đảm bền khi *publish* một message, và bảo đảm khi *tiêu thụ* nó[^k-two-problems]. Docs cũng cảnh báo rằng nhiều hệ tuyên bố "exactly once", nhưng phần lớn lời tuyên bố ấy gây hiểu nhầm: nó không còn đúng khi consumer hoặc producer có thể lỗi, khi có nhiều consumer process, hay khi dữ liệu ghi xuống đĩa có thể mất[^k-exactly-fine].

### 4.2 Phía producer

`orders` gửi event lên Kafka và gặp lỗi mạng. Nó không thể biết lỗi xảy ra trước hay sau khi message được committed[^k-prod-unsure]. Trước Kafka 0.11.0.0, producer chỉ còn cách gửi lại, và đó là at-least-once vì lần gửi trước có thể đã thành công, khiến message bị ghi vào log lần nữa[^k-prod-resend].

Từ 0.11.0.0 có *idempotent producer*. Broker gán cho mỗi producer một ID và khử trùng bằng số thứ tự (sequence number) mà producer gửi kèm mỗi message, nên gửi lại không tạo bản ghi trùng trong log[^k-idem]. Muốn bật, đặt `enable.idempotence=true`; cấu hình này đòi `max.in.flight.requests.per.connection` ≤ 5, `retries` > 0 và `acks=all`[^kp-idem]. `acks=all` nghĩa là leader chờ toàn bộ ISR xác nhận, nên record không mất miễn là còn một replica đồng bộ sống[^kp-acks-all].

### 4.3 Phía consumer

Với `billing`, mức bảo đảm do thứ tự của hai việc "xử lý" và "lưu offset" quyết định. Nếu consumer chết giữa chừng thì:

| Thứ tự | Nếu chết ở giữa | Mức bảo đảm |
|---|---|---|
| đọc → **lưu offset** → xử lý | process thay thế bắt đầu từ offset đã lưu, bỏ sót message chưa xử lý | at-most-once[^k-cons-1] |
| đọc → xử lý → **lưu offset** | process thay thế xử lý lại vài message đầu | at-least-once[^k-cons-alo] |

Cách cứu quen thuộc là làm cho việc xử lý *idempotent*. Docs Kafka nhận xét rằng nhiều message có primary key, nên cập nhật là idempotent: nhận cùng message hai lần thì chỉ ghi đè một bản ghi bằng chính nó[^k-cons-2]. Hóa đơn của `billing` ghi theo ID đơn hàng thì nằm đúng trường hợp này (nhận xét của người soạn).

Mặc định Kafka bảo đảm at-least-once. Muốn at-most-once, ta tắt retry ở producer và commit offset trước khi xử lý batch[^k-default-alo].

### 4.4 Exactly-once trong Kafka: transaction

Nếu vòng lặp của ta là *đọc từ topic A, xử lý, ghi vào topic B* thì dùng được producer transaction: producer ghi nhiều partition theo kiểu "hoặc tất cả thành công, hoặc không cái nào"[^k-txn]. Mấu chốt nằm ở chỗ vị trí đọc của consumer cũng được lưu như một message trong một topic, nên offset được ghi trong *cùng transaction* với dữ liệu ra. Transaction abort thì offset trở về giá trị cũ và dữ liệu ra không hiện với consumer khác[^k-offset-in-txn].

Consumer chọn thấy gì bằng `isolation.level`. Ở `read_uncommitted` (mặc định của Kafka consumer) nó thấy cả message của transaction đã abort; ở `read_committed` nó chỉ thấy message của transaction đã commit và message không thuộc transaction nào[^k-readcommitted].

```mermaid
sequenceDiagram
  participant C as consumer
  participant App as ứng dụng
  participant P as producer (transactional.id)
  participant K as Kafka
  App->>P: init_transactions
  loop mỗi batch
    C->>App: poll (topic A)
    App->>P: begin_transaction
    App->>P: send kết quả (topic B)
    App->>P: send_offsets_to_transaction (offset A + 1)
    App->>P: commit_transaction
    P->>K: ghi kết quả + offset trong cùng transaction
  end
```

*Sơ đồ minh họa vòng consume–transform–produce; tên hàm theo API của `rdkafka` (mục "Liên hệ với Rust").*

### 4.5 Khi đích là hệ ngoài Kafka

`billing` ghi hóa đơn vào database chứ không vào một topic Kafka, và đó là chỗ transaction của Kafka hết với tới. Cái khó là phải phối hợp *vị trí consumer* với *thứ thực sự được lưu*. Cách cổ điển là two-phase commit giữa nơi lưu offset và nơi lưu output. Docs Kafka bảo có cách đơn giản và tổng quát hơn: lưu offset *cùng chỗ* với output, vì nhiều hệ đích không hỗ trợ 2PC. Ví dụ docs nêu là một connector ghi dữ liệu vào HDFS kèm offset, để dữ liệu và offset hoặc cùng được cập nhật hoặc cùng không[^k-external].

Kết luận của docs: Kafka hỗ trợ exactly-once trong Kafka Streams, và producer/consumer transactional dùng được khi chuyển và xử lý dữ liệu *giữa các topic Kafka*; với hệ đích khác thì cần cộng tác của hệ đó[^k-external-note]. Với `billing`, đó nghĩa là hoặc ghi offset vào cùng database trong cùng transaction với hóa đơn, hoặc để hóa đơn ghi idempotent theo ID đơn (nhận xét của người soạn).

Vì sao "chỉ cần idempotent" thường là đủ? (Tóm tắt; phân tích đầy đủ ở [Chương 12, mục 4–5](12-dung-dan-va-tich-hop-du-lieu.md).) Bài giảng của Kleppmann ở Cambridge gói gọn: nếu retry không tránh được thì exactly-once = retry + idempotence hoặc khử trùng[^cam-eo]. Một cập nhật idempotent có thể áp dụng nhiều lần mà hiệu quả vẫn như đúng một lần[^cam-idem]. Đếm "like" bằng `+1` thì không idempotent, còn thêm một user vào *tập* thì idempotent[^cam-counter]. Khử trùng thì phải lưu thông tin về các request đã thấy ở nơi bền, để nhận ra trùng cả sau khi crash[^cam-dedup]. Theo lập luận end-to-end của Saltzer–Reed–Clark, việc khử trùng phải do chính ứng dụng làm, vì chỉ ứng dụng biết cách nhận ra bản trùng của mình[^e2e-dup].

<details><summary>Đi sâu: LSO, fencing, Kafka Streams, và "exactly-once" của Flink thực sự nghĩa là gì</summary>

- **LSO.** Ở `read_committed`, `poll()` chỉ trả message tới *last stable offset* (LSO), nhỏ hơn offset của transaction đang mở đầu tiên một đơn vị[^kc-iso]. Mọi message nằm sau message của transaction đang chạy bị giữ lại tới khi transaction đó xong, nên consumer `read_committed` không đọc tới high watermark được khi còn transaction đang bay.
- **`transactional.id`.** Cấu hình này cho bảo đảm kéo dài qua nhiều phiên producer: client bảo đảm các transaction cùng id đã hoàn tất trước khi bắt đầu transaction mới[^kp-txid2]. Có `transactional.id` thì `enable.idempotence` được ngầm bật, và mặc định transaction cần cụm tối thiểu ba broker[^kp-txid]. Hàm `init_transactions` của `rdkafka` ghi: nó bảo đảm mọi transaction của producer trước dùng cùng id được hoàn tất, và transaction còn mở sẽ bị abort[^rk-init-tx].
- **Kafka Streams.** Điểm khác biệt mà docs nêu: Streams tích hợp chặt với Kafka nên việc commit offset đầu vào, cập nhật state store và ghi topic đầu ra hoàn tất nguyên tử, không coi Kafka như hệ ngoài có tác dụng phụ[^ks-eos]. Bật chế độ exactly-once bằng cấu hình `processing.guarantee`[^ks-pg]. Mặc định của nó là `at_least_once`[^ks-pg-default]. Từ bản 3.0.0, phiên bản đầu của exactly-once bị đánh dấu deprecated và docs khuyến khích dùng "exactly-once v2" (`EXACTLY_ONCE_V2`)[^ks-eos-v2].
- **Flink.** Flink nói rõ "exactly once" *không* có nghĩa mỗi event được xử lý đúng một lần, mà là mỗi event *ảnh hưởng đến state do Flink quản lý* đúng một lần, vì Flink khôi phục bằng cách tua lại và phát lại nguồn[^flink-eo]. Muốn exactly-once đầu-cuối thì nguồn phải phát lại được và sink phải transactional hoặc idempotent[^flink-e2e].
- **Lưu ý cho consumer Rust** (nhận xét của người soạn): `rdkafka` 0.39.0 đi kèm librdkafka 2.12.1[^rk-sys-ver]. Phiên bản này suy ra từ số hiệu của crate `rdkafka-sys`, theo quy ước `RX.RY.RZ` là bản librdkafka đi kèm[^rk-sys-readme]. `CONFIGURATION.md` của librdkafka ở đúng tag đó ghi mặc định `isolation.level` là `read_committed`[^lib-iso]. Docs `rdkafka` cũng nói consumer nhận biết transaction dùng mặc định `read_committed`[^rk-iso]. Trong khi đó docs consumer của client Java Kafka 3.9.0 ghi mặc định là `read_uncommitted`[^kc-iso-default]. Hai mặc định khác nhau vì đó là hai thư viện khác nhau (C và Java), nên **đặt `isolation.level` tường minh** thay vì dựa vào mặc định, nhất là khi bạn đổi qua lại giữa hai client hoặc dùng `dynamic-linking` với bản librdkafka khác.

</details>

**Quay lại Q4:** mất/trùng sinh ra từ việc không biết một lần gửi đã thành công chưa; "xử lý rồi lưu offset" cho at-least-once; transaction của Kafka cho exactly-once *trong* Kafka; với hệ ngoài (như database của `billing`) phải ghi idempotent hoặc ghi offset cùng chỗ với kết quả.

## 5. CDC, event sourcing và stream–table duality (Q5)

> **Trả lời ngắn.** *Change data capture* (CDC) biến mọi thay đổi đã commit của database thành một stream event. Với PostgreSQL, cơ chế là *logical decoding*: đọc WAL và giải mã thành dạng dễ hiểu[^pg-ld-wal]. Kết quả được giao qua *replication slot* theo đúng thứ tự thực hiện trên server gốc[^pg-slot]. *Event sourcing* làm ngược lại: chính log event là nơi lưu sự thật, và trạng thái hiện tại được dựng bằng cách phát lại event[^az-es]. Cả hai nối vào ý "stream là changelog của một bảng, bảng là ảnh chụp của một stream"[^ks-stream-table].

**Giải thích.**

*Với service `orders`.* `orders` lưu đơn vào PostgreSQL, còn `search-index` và một lớp cache cần theo kịp mỗi thay đổi. Cách ngây thơ là code của `orders` ghi database *rồi* ghi message lên Kafka. Hai bước ghi đó không nguyên tử: `orders` chết giữa hai bước thì database và search index lệch nhau vĩnh viễn. CDC đảo cách làm: `orders` chỉ ghi database, còn thay đổi của database *tự* trở thành stream. Mục này giải thích cơ chế, những chỗ vận hành dễ vỡ, và event sourcing khác CDC ra sao.

### 5.1 Logical decoding của PostgreSQL

Docs định nghĩa logical decoding là quá trình trích mọi thay đổi bền của các bảng thành một định dạng mạch lạc, dễ hiểu, diễn giải được mà không cần biết nội bộ database[^pg-ld-def]. Trong PostgreSQL, nó được cài bằng cách giải mã nội dung WAL, vốn chỉ mô tả thay đổi ở mức lưu trữ, thành dạng ứng dụng dùng được như dòng tuple hay câu SQL[^pg-ld-wal]. (WAL và nguyên tắc "ghi log trước" xem [P5 · Chương 05](../05-storage/05-crash-consistency-va-journaling.md).) Điều kiện là đặt `wal_level = logical` để WAL chứa thêm thông tin cần cho logical decoding[^pg-wal-level].

```mermaid
flowchart LR
  APP["ứng dụng"] -->|"INSERT/UPDATE/DELETE + COMMIT"| DB["PostgreSQL (bảng + WAL)"]
  DB -->|"WAL"| SLOT["logical replication slot + output plugin"]
  SLOT -->|"thay đổi theo thứ tự commit"| CDC["connector CDC (ví dụ Debezium)"]
  CDC --> TOPIC["topic Kafka"]
  TOPIC --> S1["search index"]
  TOPIC --> S2["cache"]
  TOPIC --> S3["warehouse (xem Chương 13)"]
```

*Sơ đồ minh họa của người soạn.*

### 5.2 Replication slot

Slot là một dòng thay đổi có thể phát lại cho client theo đúng thứ tự thực hiện trên server gốc; mỗi slot phát thay đổi của một database[^pg-slot]. Slot tồn tại độc lập với kết nối đang dùng nó và an toàn khi crash[^pg-slot-persist]. Các thay đổi được giải mã theo *thứ tự commit*; giữa begin và commit chỉ có thay đổi của đúng transaction đó, và transaction bị rollback không bao giờ được giải mã[^pg-commit-order]. Chỉ transaction đã được flush an toàn xuống đĩa mới được giải mã[^pg-flushed].

*Output plugin* biến biểu diễn nội bộ của WAL thành định dạng mà consumer của slot muốn[^pg-plugin]. Consumer lấy thay đổi từ slot qua giao thức streaming replication hoặc qua hàm SQL[^pg-consume].

CDC cần trạng thái đầu rồi mới theo dõi thay đổi. Khi tạo slot qua giao thức streaming replication, PostgreSQL xuất một snapshot cho biết đúng trạng thái database mà sau đó mọi thay đổi sẽ được đưa vào stream. Nhờ đó ta dựng được bản sao rồi áp tiếp nội dung slot mà không mất thay đổi nào[^pg-snapshot]. Debezium (một connector CDC phổ biến) làm tương tự: lần đầu kết nối, connector chụp một snapshot nhất quán, rồi liên tục bắt thay đổi mức hàng đã commit[^deb-snap]. Nó ghi vị trí WAL cho mỗi event, và khi khởi động lại thì đọc tiếp từ chỗ dừng[^deb-resume].

### 5.3 Hai điểm vận hành dễ vỡ

Cả hai đều có trong docs.

**Trùng lặp sau crash.** Trong điều kiện bình thường slot phát mỗi thay đổi một lần. Nhưng vị trí slot chỉ được lưu tại checkpoint, nên sau crash slot có thể lùi về LSN cũ và gửi lại các thay đổi gần nhất; client có trách nhiệm tránh tác hại khi nhận cùng message nhiều lần[^pg-slot-once]. Nói cách khác, CDC là *at-least-once* (mục 4) ở phía consumer. Debezium nói đúng điều đó: khi hệ thống chạy bình thường, nó giao exactly-once mỗi bản ghi thay đổi[^deb-eo]. Trong các tình huống bất thường thì nó, giống Kafka, chỉ bảo đảm at-least-once[^deb-alo].

**Slot giữ WAL.** Slot sống qua crash và không biết gì về trạng thái consumer của nó. Nó ngăn việc xóa tài nguyên cần thiết ngay cả khi không còn kết nối nào dùng: theo docs, cả WAL cần thiết lẫn các hàng catalog hệ thống cần thiết đều không được dọn chừng nào slot còn cần, nên tốn dung lượng[^pg-slot-caution]. Trong trường hợp cực đoan, database có thể tự tắt để tránh wraparound ID transaction, và slot không còn dùng thì phải xóa[^pg-slot-wrap]. Cấu hình `max_slot_wal_keep_size` mặc định là `-1`, nghĩa là slot có thể giữ lượng WAL không giới hạn[^pg-maxslot]. Hệ quả cho `orders` (nhận xét của người soạn): consumer CDC chết lâu thì thành sự cố đĩa của chính database nguồn, nên phải theo dõi độ trễ của slot.

### 5.4 Outbox: tránh ghi hai nơi

Quay lại bài toán ghi hai nơi ở đầu mục. Khi service cần vừa đổi state trong database vừa phát event, mẫu *outbox* ghi event vào một bảng ngay trong transaction đổi nghiệp vụ, rồi để CDC chuyển bảng đó lên broker. Cách này tránh được sự không nhất quán giữa state nội bộ của service và state trong các event mà service khác nhận[^deb-outbox]. (Tóm tắt; mẫu này và cách vận hành ở [Chương 12, mục 6](12-dung-dan-va-tich-hop-du-lieu.md).) Phần chuyển lên broker là at-least-once, nên consumer vẫn phải idempotent (suy luận của người soạn).

### 5.5 Event sourcing

Thay vì lưu trạng thái hiện tại rồi cập nhật tại chỗ, event sourcing lưu toàn bộ chuỗi hành động trên một đối tượng vào kho append-only; kho đó là system of record để dựng lại đối tượng[^az-es]. Mỗi entity có một event stream riêng; trạng thái hiện tại có được bằng cách phát lại các event của nó, gọi là rehydration[^az-replay]. Vì phát lại tốn kém nên hệ thống thường duy trì *materialized view*, một phép chiếu chỉ-đọc của event store, tối ưu cho truy vấn[^az-mv]. View cập nhật sau event, nên hệ thống chỉ *nhất quán cuối cùng* (eventually consistent)[^az-ec].

Khác biệt cốt lõi với CDC (nhận xét của người soạn): CDC *suy ra* event từ thay đổi của một database vốn là nơi giữ state; event sourcing đặt chính event làm nơi giữ state, còn bảng chỉ là dẫn xuất. Nếu `orders` theo hướng event sourcing, kho event của đơn #7 là sự thật, còn bảng `orders` chỉ là một view dựng từ đó.

### 5.6 Stream–table duality

Docs Kafka Streams gọi tên mối quan hệ giữa hai thứ trên. Một stream có thể xem như changelog của một bảng, mỗi record là một thay đổi trạng thái[^ks-stream-table]. Ngược lại, một bảng có thể xem như ảnh chụp, tại một thời điểm, của giá trị mới nhất mỗi key trong một stream[^ks-table-snapshot]. Nói gọn: stream xem như bảng, bảng xem như stream, và log compaction của Kafka khai thác chính tính đối ngẫu này[^ks-duality]. Đây là nền cho join stream–table ở mục 7 và cho changelog của state ở mục 8.

<details><summary>Đi sâu: replica identity, event sourcing trả giá gì, xóa dữ liệu cá nhân, thử logical decoding bằng SQL</summary>

- **Replica identity.** Output plugin luôn thấy hàng mới của INSERT và phiên bản mới của UPDATE, còn hàng *cũ* của UPDATE/DELETE có hay không là tùy cấu hình replica identity[^pg-ri]. Mặc định chỉ ghi giá trị cũ của các cột khóa chính[^pg-ri-default]. Với `FULL` thì ghi giá trị cũ của mọi cột[^pg-ri-full].
- **Giá phải trả của event sourcing.** Docs Azure Architecture Center cảnh báo đây là mẫu phức tạp với nhiều đánh đổi: nó thay đổi cách lưu dữ liệu, xử lý đồng thời, tiến hóa schema và truy vấn trạng thái[^az-complex]. Event đã lưu là bất biến, nên muốn sửa hay hoàn tác chỉ có cách thêm một *compensating event*[^az-comp]. Đổi schema event thì có thể dùng *upcasting*, tức hàm chuyển schema cũ sang schema hiện tại lúc giải tuần tự hóa[^az-upcast]. Snapshot chỉ là tối ưu: event stream vẫn là nguồn sự thật[^az-snap]. Event thường được giao at-least-once nên handler phải idempotent[^az-idem].
- **Xóa dữ liệu cá nhân.** Cũng nguồn đó nêu rằng tính append-only và bất biến của event store xung đột với quy định yêu cầu xóa dữ liệu cá nhân, như quyền "được quên"[^az-gdpr]. Hướng xử lý (tách dữ liệu cá nhân, crypto-shredding…) bàn ở [Chương 14](14-trach-nhiem-voi-du-lieu.md), không lặp lại ở đây.
- **Thử bằng SQL.** Docs PostgreSQL có ví dụ tạo slot bằng `pg_create_logical_replication_slot` rồi đọc thay đổi bằng `pg_logical_slot_get_changes`[^pg-ex]. Ví dụ đó ghi chú rằng thay đổi đã đọc bằng `get_changes` thì bị tiêu thụ và không phát lại[^pg-ex-consumed]. Muốn xem trước mà không tiêu thụ thì dùng `pg_logical_slot_peek_changes`[^pg-ex-peek].
- **CDC so với replication** (nhận xét của người soạn): CDC là *replication ở mức logic* (hàng/bảng) nên đi được sang hệ khác kiểu như search hay warehouse; replication vật lý (byte WAL) chỉ nối PostgreSQL với PostgreSQL. Phân tích đầy đủ ở [Chương 05](05-replication.md).
- **Ghép lên kiến trúc lớn.** Dòng thay đổi này là một trong các cách đưa dữ liệu từ hệ vận hành sang hệ phân tích (xem [Chương 13](13-kien-truc-he-du-lieu.md)), và là chất liệu của "dữ liệu dẫn xuất" ở [Chương 12](12-dung-dan-va-tich-hop-du-lieu.md).

</details>

**Quay lại Q5:** CDC = đọc WAL → slot → stream thay đổi theo thứ tự commit (at-least-once; nhớ theo dõi slot giữ WAL), nên `orders` chỉ cần ghi database; event sourcing = lưu event làm sự thật, state là dẫn xuất; hai thứ gặp nhau ở ý stream và bảng là hai mặt của cùng dữ liệu.

## 6. Event time, processing time, window và watermark (Q6)

> **Trả lời ngắn.** *Event time* là lúc sự việc thực sự xảy ra; *processing time* là lúc hệ thống xử lý nó, và hai mốc đó có thể cách nhau từ vài mili-giây tới vài ngày[^ks-proc-time]. Muốn gộp theo thời gian thì phải chia stream thành *window* (tumbling, hopping, session). Vì event tới trễ và sai thứ tự nên cần thêm *watermark*, tín hiệu "event time đã tới mốc t"; watermark thường chỉ là ước lượng nên có thể sai[^df-fast], và event trễ vẫn cần chính sách riêng (grace period, allowed lateness).

**Giải thích.**

*Với service `orders`.* `dashboard` đếm "số đơn mỗi 5 phút". Giả sử một khách đặt đơn trên app mobile lúc 12:03 khi đang đi qua đoạn hầm mất sóng; điện thoại giữ đơn lại và chỉ gửi lên được lúc 12:09. Nếu `dashboard` đếm theo *lúc server nhận* (processing time) thì đơn này rơi vào ô 12:05–12:10, sai so với thực tế. Nếu đếm theo *lúc xảy ra* (event time) thì nó thuộc ô 12:00–12:05, nhưng ô đó có thể đã được chốt và gửi báo cáo từ lâu. Cả mục này xoay quanh khoảng lệch đó.

### 6.1 Hai miền thời gian

Event time là thời điểm event hoặc data record xảy ra, tức lúc nó được tạo ra "ở nguồn"[^ks-event-time]. Dataflow nói chặt hơn một chút: đó là đồng hồ của hệ sinh ra event, đọc tại lúc xảy ra[^df-evdef]. Processing time là thời điểm event được quan sát tại một điểm trong pipeline theo đồng hồ hệ thống, và bài này không giả định các đồng hồ phân tán đã đồng bộ[^df-procdef].

Hai miền này khác nhau ở điểm sau: event time của một sự kiện gần như không bao giờ đổi, còn processing time cứ trôi tới khi event đi qua pipeline[^df-time]. Độ chênh (skew) giữa hai miền cũng không cố định, vì nó phụ thuộc độ trễ truyền thông, thuật toán lập lịch, thời gian xử lý…[^df-skew] Kafka Streams tách thêm *ingestion time*: lúc broker lưu record vào partition[^ks-ingest]. Đồng hồ trên thiết bị sinh event không đáng tin tới đâu thì event time cũng sai tới đó (suy luận của người soạn; xem [Chương 08](08-su-co-he-phan-tan.md)).

### 6.2 Window

Windowing chia stream thành các khối hữu hạn để xử lý theo nhóm. Với dữ liệu không biên nó cần cho nhiều phép, như gộp hay outer join[^df-windowing]. Về thực chất window luôn dựa trên thời gian: nhiều hệ hỗ trợ window theo số tuple, nhưng đó chỉ là window theo thời gian logic[^df-window-time]. Kafka Streams dùng window để nhóm các record cùng key[^ks-win-def]. Nó theo dõi window *theo từng key*[^ks-win-key]. Với `dashboard`, "mỗi 5 phút" là một window. Các kiểu chính:

| Kiểu | Định nghĩa | Một record thuộc |
|---|---|---|
| **Tumbling** (Dataflow gọi là "fixed"[^df-fixed]) | cửa sổ cỡ cố định, không chồng lấn, không hở[^ksdsl-tumb] | đúng một window |
| **Hopping** (Dataflow gọi là "sliding"[^df-sliding]) | cỡ cố định cộng một bước tiến (advance), nên có thể chồng lấn[^ksdsl-hop] | có thể nhiều window |
| **Session** | chuỗi hoạt động ngăn cách bởi khoảng im lặng (gap), nên cỡ thay đổi[^ksdsl-sess2] | window của session đang có, nếu rơi trong gap[^ksdsl-sess] |

"Đơn mỗi 5 phút" của `dashboard` là tumbling. Nếu muốn "đơn trong 5 phút gần nhất, cập nhật mỗi phút" thì dùng hopping cỡ 5 phút, bước 1 phút[^ksdsl-hop2]. Window session hợp với "phiên mua hàng của một khách", vì window của mỗi key được theo dõi độc lập[^ksdsl-sess-keys] (nhận xét của người soạn).

Tumbling là trường hợp riêng của hopping có cỡ bằng bước tiến[^ksdsl-tumb]. Dataflow nói tương tự: fixed window là trường hợp riêng của sliding window khi cỡ bằng chu kỳ[^df-fixed-special].

**Cẩn thận tên gọi.** Kafka Streams có thêm một kiểu riêng tên "sliding", là cửa sổ cỡ cố định chồng lấn dựa trên *chênh lệch giữa timestamp các record*[^ksdsl-sliding]. Nó khác "sliding" của Dataflow, vốn tương đương "hopping" của Kafka Streams. Flink thì dùng tên tumbling, sliding, session cho các window assigner[^fl-tumbling]. Session window của Flink đóng lại khi không nhận element nào trong một khoảng thời gian[^fl-session].

### 6.3 Watermark

Flink định nghĩa: `Watermark(t)` khai báo rằng event time đã tới t trong stream đó, nghĩa là không còn element nào có timestamp ≤ t sẽ tới nữa[^fl-wm-def]. Operator có nhiều đầu vào thì event time của nó là *min* của các đầu vào[^fl-wm-min]. Với `dashboard`, watermark là cách nó biết "ô 12:00–12:05 chắc đã đủ đơn, chốt được rồi".

Chữ "chắc" ở đó là điểm yếu. Bài Dataflow nêu hai nhược điểm, đều về tính đúng đắn. Watermark đôi khi *quá nhanh*: vẫn có dữ liệu trễ tới sau watermark[^df-fast]. Và đôi khi *quá chậm*: nó là thước đo tiến độ toàn cục, nên một datum chậm có thể giữ cả pipeline lại[^df-slow]. Gốc rễ là với đa số tập dữ liệu phân tán thực tế, hệ thống không đủ thông tin để dựng watermark đúng 100%[^df-heur]. Kết luận của bài: chỉ watermark thì chưa đủ[^df-insufficient].

### 6.4 Dữ liệu trễ

Đơn lúc 12:03 của khách trong hầm là một element trễ. Theo Flink, element trễ là element tới sau khi đồng hồ event time (theo watermark) đã đi quá timestamp của nó[^fl-late]. Ta có thể làm watermark chậm đi để chờ, nhưng như vậy kết quả window bị trễ theo[^fl-wm-delay]. Vì vậy cần một tham số riêng cho phép trễ.

Ở Flink, `allowed lateness` mặc định là 0, tức element tới sau watermark bị bỏ[^fl-late-drop]. Đặt lớn hơn 0 thì element trễ nhưng chưa bị bỏ vẫn được thêm vào window, và window có thể phát kết quả lần nữa[^fl-lateness]. Kafka Streams có *grace period*, thời gian chờ record sai thứ tự của một window[^ks-grace]. Record tới sau grace period thì bị bỏ[^ks-grace2]. Docs gọi thẳng đây là đánh đổi giữa độ trễ, chi phí và độ đúng[^ks-tradeoff].

### 6.5 Phát kết quả khi nào, và sửa ra sao

Dataflow tách câu hỏi "khi nào phát" thành *trigger*, quyết định lúc nào trong processing time thì kết quả của phép gộp được phát ra[^df-trigger]. Trigger theo watermark phát khi watermark vượt cuối window[^df-wm-trigger]. Còn câu hỏi "kết quả sau liên hệ với kết quả trước thế nào" thành chế độ tinh chỉnh. *Discarding* bỏ nội dung window sau mỗi lần phát, kết quả sau không liên quan tới kết quả trước[^df-discarding]. *Accumulating* giữ nội dung lại, và kết quả sau là bản tinh chỉnh của kết quả trước[^df-trigger-modes].

Bài Dataflow cũng nhận xét về Lambda Architecture: nó né bài toán đầy đủ-dữ-liệu bằng cách chỉ cho ước lượng độ trễ thấp từ pipeline stream, kèm hứa hẹn kết quả đúng về sau khi batch chạy[^df-lambda]. Phần này nối sang [Chương 10](10-batch-processing.md) và [Chương 12](12-dung-dan-va-tich-hop-du-lieu.md).

<details><summary>Đi sâu: stream time của Kafka Streams, nguồn nhàn rỗi, watermark theo partition</summary>

- **Stream time.** Kafka Streams gán timestamp cho mỗi record qua `TimestampExtractor`. Thời gian do dữ liệu dẫn dắt (*stream time*) chỉ tiến khi có record mới tới processor[^ks-streamtime]. Nó khác wall-clock time lúc ứng dụng thực sự chạy[^ks-streamtime2].
- **Nguồn nhàn rỗi (idle source).** Nếu một partition lâu không có event, bộ sinh watermark không có thông tin mới, và watermark bị giữ lại vì nó là *min* của các watermark song song[^fl-idle]. Flink có `WatermarkStrategy` phát hiện nguồn nhàn rỗi và đánh dấu nó idle[^fl-idle2]. Tình huống này hay gặp khi một topic Kafka có partition thưa (nhận xét của người soạn).
- **Watermark theo partition Kafka.** Flink nêu rằng khi đọc Kafka song song nhiều partition, các event xen kẽ nhau và phá mẫu timestamp của từng partition. Vì vậy Flink sinh watermark *bên trong* consumer Kafka theo từng partition rồi gộp lại[^fl-kafka-wm].
- **Event time và tính tái lập** (suy luận của người soạn): kết quả theo event time không phụ thuộc lúc chạy, nên chạy lại trên log cũ cho kết quả như cũ; đó là lý do replay (mục 8) quan trọng. Kết quả theo processing time thì không.

</details>

**Quay lại Q6:** event time là lúc xảy ra, processing time là lúc xử lý; window chia stream thành khối hữu hạn (tumbling/hopping/session); watermark ước lượng tiến độ event time nhưng có thể sai, nên `dashboard` cần thêm allowed lateness/grace và trigger cho những đơn tới muộn.

## 7. Stream join (Q7)

> **Trả lời ngắn.** Join trên stream khác join trong database ở chỗ dữ liệu chưa đầy đủ và còn đang tới tiếp. Có ba dạng: *stream–stream* (ghép hai dòng event theo key trong một khoảng thời gian), *stream–table* (làm giàu event bằng giá trị mới nhất của một bảng được duy trì từ changelog) và *table–table*. Hai chỗ dễ sai là *co-partitioning* (hai đầu vào phải chia partition cùng cách) và *thứ tự* (xử lý theo offset hay theo timestamp).

**Giải thích.**

*Với service `orders`.* Giả sử ngoài topic `orders`, một service thanh toán khác phát topic `payments`. Ta muốn ghép để biết đơn nào đã được thanh toán. Hai stream này không đồng bộ: event thanh toán có thể tới Kafka *trước* event tạo đơn, vì độ trễ của hai đường khác nhau. Vì vậy join phải *nhớ* cùng lúc cả hai phía, nghĩa là có state, và phải có giới hạn thời gian để state không phình mãi (nhận xét của người soạn). Ta lần lượt xem các dạng join, rồi hai chỗ dễ sai.

### 7.1 Stream–stream: ghép `orders` với `payments`

Ở Flink, window join ghép các element của hai stream có cùng key và cùng nằm trong một window; window xác định bằng window assigner[^fl-win-join]. Các cặp được tạo ra như inner join: element không có đối tác ở stream kia thì không được phát ra[^fl-inner]. Flink còn có *interval join*: element của stream B có timestamp nằm trong một khoảng tương đối so với timestamp của element stream A cùng key[^fl-interval]. Hiện nay interval join chỉ hỗ trợ event time[^fl-interval-event]. Kafka Streams thì coi join stream–stream là join có cửa sổ, và theo docs cả ba loại (inner, outer, left) đều xử lý đúng dữ liệu sai thứ tự[^ks-join-ss].

### 7.2 Stream–table: làm giàu event

Giả sử `orders` muốn gắn vào mỗi event hồ sơ mới nhất của khách hàng. Đây là join KStream–KTable. Nó không dùng window: mỗi khi có record mới trên stream, nó tra một KTable (một changelog stream)[^ksj-streamtable]. Theo cách hiểu của người soạn (từ stream–table duality ở mục 5), đó là tra giá trị mới nhất của mỗi key. Ví dụ của docs rất giống: làm giàu luồng hoạt động của user bằng thông tin hồ sơ user mới nhất[^ksj-enrich].

### 7.3 Co-partitioning

Với equi-join, hai đầu vào phải *co-partitioned*: record cùng key của hai bên phải tới cùng một task xử lý[^ksj-copart]. Điều kiện gồm hai topic có *cùng số partition* và cùng cách phân vùng[^ksj-same-parts]. Docs nhấn mạnh việc bảo đảm điều đó là trách nhiệm của người dùng[^ksj-copart-user]. Đây là hệ quả trực tiếp của partition theo key ở [Chương 06](06-partitioning.md): muốn ghép hai dòng theo key mà không chuyển dữ liệu liên máy thì cả hai phải chia theo cùng một cách (suy luận của người soạn). Với `orders` và `payments`, nghĩa là cả hai topic phải lấy ID đơn hàng làm key và có cùng số partition.

### 7.4 Thứ tự

Nếu stream–table join không dùng *versioned store*, Kafka Streams không kiểm tra record sai thứ tự mà chỉ xử lý theo thứ tự offset, nên kết quả có thể không đoán trước được[^ks-join-ooo]. Với versioned store, record phía stream sẽ join với bản ghi bảng có timestamp mới nhất mà không vượt quá timestamp của record stream[^ks-versioned]. Phía bảng mà sai thứ tự thì vẫn chưa được xử lý[^ks-join-ooo2].

<details><summary>Đi sâu: các biến thể khác</summary>

- Kafka Streams có `GlobalKTable` và *foreign-key join* của KTable, những kiểu không đòi co-partitioning (docs ghi rõ ở phần co-partitioning)[^ksj-copart-exempt]. Chi tiết xem developer guide.
- Phát biểu tổng quát (suy luận của người soạn): mọi join stream cần state có giới hạn thời gian (window hoặc interval), hoặc cần một bảng materialize. Chọn kích thước window hay interval là quyết định đánh đổi, giống `grace period` ở mục 6.

</details>

**Quay lại Q7:** join stream = ghép hai nguồn đang tới tiếp (như `orders` với `payments`), nên cần state; hai đầu vào phải chia partition cùng cách; và kết quả phụ thuộc vào chính sách xử lý sai thứ tự.

## 8. Chịu lỗi cho xử lý có state (Q8)

> **Trả lời ngắn.** Job có state (đếm, window, join) mất state khi máy chết, nên phải lưu state một cách có hệ thống và biết đọc lại nguồn từ đúng điểm. Có hai cách phổ biến. *Checkpoint* chụp state của mọi operator cùng với offset nguồn (Flink)[^flink-barrier]. *Changelog* thì cho mỗi state store ghi mọi cập nhật vào một topic Kafka để dựng lại sau lỗi (Kafka Streams)[^ks-changelog]. Cả hai đều dựa vào nguồn *phát lại được*.

**Giải thích.**

*Với service `orders`.* Job `dashboard` giữ số đơn đang đếm của từng window trong RAM. Máy chạy nó chết thì số đếm mất. Job thay thế cần hai thứ: bản ghi gần nhất của các số đếm, và biết số đếm đó ứng với offset nào trong topic `orders`. Hai thứ này phải khớp nhau. Giả sử bản ghi ứng với offset 1000: nếu job mới đọc tiếp từ offset 1200 thì sót 200 đơn, còn nếu đọc từ 800 thì đếm đôi 200 đơn (nhận xét của người soạn).

### 8.1 Checkpoint kiểu Flink

Flink dùng một biến thể của thuật toán Chandy–Lamport, gọi là *asynchronous barrier snapshotting*[^flink-abs]. Khi checkpoint coordinator ra lệnh, mọi nguồn ghi lại offset của mình rồi chèn các *barrier* đánh số vào stream. Barrier trôi qua đồ thị job, đánh dấu phần stream trước và sau mỗi checkpoint[^flink-barrier]. Checkpoint n chứa state của mỗi operator có được sau khi đã tiêu thụ mọi event *trước* barrier n và không event nào *sau* nó[^flink-cp-n].

Operator có hai đầu vào thực hiện *barrier alignment*, để snapshot phản ánh state sau khi tiêu thụ cả hai đầu vào tới (nhưng không quá) hai barrier[^flink-align]. Khi có lỗi, Flink khôi phục bằng cách tua lại và phát lại các stream nguồn[^flink-replay].

```mermaid
flowchart LR
  SRC["nguồn (Kafka): ... e5 e6 | B(n) | e7 e8"] --> OP1["operator đếm"]
  OP1 --> OP2["operator window"]
  OP1 -. "khi nhận B(n): ghi state + offset" .-> CK[("checkpoint n")]
  OP2 -. "khi nhận B(n): ghi state" .-> CK
```

*Sơ đồ minh họa của người soạn: barrier B(n) chia stream thành "trước" và "sau" checkpoint n.*

Flink có ba kết quả khả dĩ sau lỗi: không cố khôi phục (at-most-once); không mất nhưng có thể trùng kết quả (at-least-once); không mất, không trùng (exactly-once)[^flink-three]. Barrier alignment chỉ cần cho exactly-once, nên nếu không cần thì ta cấu hình `AT_LEAST_ONCE` để bỏ alignment cho nhanh hơn[^flink-alo]. Như ở mục 4, exactly-once ở đây là về *state của Flink*[^flink-eo]. Ra tới hệ ngoài thì cần sink transactional hoặc idempotent[^flink-e2e].

### 8.2 Changelog kiểu Kafka Streams

Mỗi state store cục bộ có một *changelog topic* Kafka, được replicate, ghi mọi cập nhật của store[^ks-changelog]. Topic này được partition để mỗi store/task có phần changelog riêng, và bật log compaction để dữ liệu cũ dọn được mà topic không phình mãi[^ks-changelog2]. Nếu task chạy trên máy hỏng và khởi động lại ở máy khác, Kafka Streams bảo đảm khôi phục state như trước lỗi bằng cách phát lại changelog trước khi tiếp tục xử lý[^ks-restore].

Chi phí khởi tạo lại chủ yếu là thời gian dựng lại state. Để giảm, ta cấu hình *standby replica*[^ks-standby]. Đó là bản sao đầy đủ của state[^ks-standby2]. Và theo mục 4, trong chế độ exactly-once, việc commit offset đầu vào, cập nhật state store và ghi topic đầu ra được thực hiện nguyên tử[^ks-eos].

### 8.3 So sánh hai cách

Hai cách đánh đổi khác nhau (nhận xét của người soạn). Checkpoint chụp *toàn bộ job tại một điểm cắt thống nhất*; ở dạng đầy đủ mỗi lần chụp phải ghi lượng state tương ứng, còn checkpoint tăng dần giảm bớt phần này[^flink-ckstorage]. Changelog ghi *từng cập nhật* liên tục, nên phục hồi tốn thời gian phát lại, và standby giúp giảm khoản đó. Cả hai chỉ đúng nếu nguồn đọc lại được từ một offset, và đó là lý do log kiểu Kafka (mục 2–3) hợp với stream processing. Với `dashboard`, nguồn là topic `orders` giữ trong retention, nên đọc lại từ offset đã chụp là việc làm được.

<details><summary>Đi sâu: nơi lưu checkpoint, backpressure, và liên hệ với batch</summary>

- Flink có hai loại snapshot. *Checkpoint* là snapshot tự động để phục hồi lỗi, có thể tăng dần, tối ưu cho việc khôi phục nhanh[^flink-ckstorage]. *Savepoint* là snapshot do người dùng kích hoạt cho mục đích vận hành như redeploy, nâng cấp hay đổi cỡ[^flink-savepoint]. Vị trí lưu snapshot cấu hình qua checkpoint storage[^flink-ckstore2].
- Một task Kafka Streams là đơn vị song song cố định, gắn với danh sách partition đầu vào, và phép gán partition–task không đổi[^ks-tasks]. (Suy luận của người soạn: nên mỗi task phục hồi state độc lập với task khác.)
- Backpressure trong stream engine là bài toán cùng họ với channel bounded ở [P7 · Chương 09, mục 3](../07-io-async/09-async-trong-production.md): consumer chậm thì phải làm producer chậm theo, không để hàng đợi phình vô hạn.
- Kafka docs có ví dụ nạp dữ liệu vào Hadoop, nơi task thất bại khởi động lại từ vị trí gốc mà không có dữ liệu trùng[^k-hadoop]. Đó là ví dụ cổ điển cho batch tải offline từ log (nối sang [Chương 10](10-batch-processing.md)).

</details>

**Quay lại Q8:** state phải lưu cùng vị trí nguồn tương ứng (checkpoint, hoặc changelog + offset commit nguyên tử) và nguồn phải phát lại được; chịu lỗi khi đó là "tua lại và chạy lại", như `dashboard` đọc lại `orders` từ offset đã chụp.

## Liên hệ với Rust

> Các API dưới đây theo docs.rs **`rdkafka` 0.39.0** (binding an toàn tới `librdkafka`; docs cảnh báo thư viện đang phát triển và API có thể đổi[^rk-warn]) và **`tokio` 1.53.2**. Đoạn code chỉ để minh họa cách ghép API, **chưa được biên dịch hay chạy**; không khẳng định kết quả chạy.

Giả sử `billing` cũng viết bằng Rust và đọc topic `orders` bằng `rdkafka`. Phần này nói `rdkafka` cho `billing` những công cụ gì cho các khái niệm ở trên, và hai chỗ dễ vấp khi ghép với async Rust.

**Các kiểu của `rdkafka` ứng với khái niệm trong chương.**

| Khái niệm | Kiểu / hàm | Ghi chú (nguồn) |
|---|---|---|
| producer async | `FutureProducer` | trả một `Future` hoàn tất khi message được giao tới Kafka (hoặc lỗi)[^rk-clients]. Có thread poll nội bộ nên không cần poll tay[^rk-fp-poll] |
| consumer async | `StreamConsumer` | một `Stream` message, tự poll consumer[^rk-clients]; `recv().await` lấy message kế |
| thông tin message | trait `Message` | `key()`, `payload()`, `topic()`, `partition()`, `offset()`, `timestamp()`[^rk-msg] |
| commit offset | `Consumer::commit_message`, `CommitMode::{Sync, Async}` | tương đương `commit` với offset + 1[^rk-commit-plus1] |
| transaction | `init_transactions`, `begin_transaction`, `send_offsets_to_transaction`, `commit_transaction`, `abort_transaction` | trait `Producer`[^rk-tx-section] |

**At-least-once với `StreamConsumer`.** Quy tắc ở mục 4 viết lại bằng lời Rust: để at-least-once, ứng dụng phải commit offset *chỉ sau khi* xử lý xong message. Commit quá sớm có thể mất message, vì khi phục hồi, consumer bắt đầu từ message *kế tiếp* và bỏ qua message đang lỗi[^rk-alo]. Có hai điểm API cần biết. `commit` coi mọi offset trước cũng đã commit, nên chỉ dùng khi xử lý *đúng thứ tự*[^rk-commit]. Còn `commit_message` tự cộng 1 vào offset của message[^rk-commit-plus1]. Muốn tự điều khiển, đặt `enable.auto.offset.store=false` rồi gọi `store_offset`[^rk-store].

```rust
// Minh họa (chưa biên dịch): đọc → xử lý idempotent → commit.
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use rdkafka::Message;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", "localhost:9092")
        .set("group.id", "billing")
        .set("enable.auto.commit", "false") // commit tay, sau khi xử lý
        .set("isolation.level", "read_committed") // đặt tường minh (mục 4)
        .create()?;
    consumer.subscribe(&["orders"])?;

    loop {
        let msg = consumer.recv().await?; // phải gọi đủ thường xuyên (xem dưới)
        if let Some(bytes) = msg.payload() {
            apply_idempotently(msg.partition(), msg.offset(), bytes).await?; // upsert theo khóa
        }
        consumer.commit_message(&msg, CommitMode::Async)?; // SAU khi xử lý
    }
}
// apply_idempotently là hàm của ứng dụng (ví dụ upsert theo khóa), không thuộc rdkafka.
```

Hai bẫy sát với async Rust (đều ghi trong docs):

- `StreamConsumer` phải được lấy message ra ít nhất mỗi `max.poll.interval.ms`, nếu không `librdkafka` coi luồng xử lý bị treo và rời group[^rk-sc-poll]. (Nhận xét của người soạn: handler `await` một lời gọi chậm rồi mới quay lại `recv` là cách dễ vi phạm; kết hợp với timeout và backpressure ở [P7 · Chương 09](../07-io-async/09-async-trong-production.md).)
- `CommitMode::Async` *không* "async theo nghĩa Rust". Mọi API commit chỉ đặt yêu cầu vào hàng đợi cục bộ, rồi một thread riêng gửi tới broker[^rk-commitmode]. Khác biệt Sync/Async là caller có chờ broker trả lời hay không; docs nói API commit không phải `async` vì `librdkafka` không có giao diện callback[^rk-commitmode2].

**Transaction (exactly-once trong Kafka).** Docs `rdkafka` nói EOS đạt được bằng producer transactional, cho phép record và offset consumer được commit hoặc abort nguyên tử; consumer cần `isolation.level = read_committed` mới chỉ thấy message đã commit[^rk-eos]. Thứ tự gọi: `init_transactions` một lần[^rk-init-tx]. Mỗi batch thì `begin_transaction` → gửi record → `send_offsets_to_transaction` → `commit_transaction`[^rk-tx-section]. Hàm `send_offsets_to_transaction` đánh dấu các offset là một phần transaction hiện tại và chỉ coi là đã commit nếu transaction commit thành công[^rk-send-offsets]. Consumer không được bật auto commit[^rk-send-offsets-hint]. Sau khi `init_transactions`, producer không được ghi ngoài transaction[^rk-tx-nooutside]. Các hàm này có thể *chặn* tới hết timeout được truyền vào[^rk-tx-block]; trong runtime Tokio nên đặt chúng vào `spawn_blocking` hoặc thread riêng, theo [P7 · Chương 09, mục 5](../07-io-async/09-async-trong-production.md) (nhận xét của người soạn).

```rust
// Minh họa (chưa biên dịch): một vòng consume–transform–produce.
use std::time::Duration;
use rdkafka::consumer::Consumer;
use rdkafka::producer::{FutureProducer, FutureRecord, Producer};
use rdkafka::topic_partition_list::{Offset, TopicPartitionList};
use rdkafka::util::Timeout;

// producer đã tạo với "transactional.id"; consumer với enable.auto.commit=false
async fn step<C: Consumer>(
    producer: &FutureProducer,
    consumer: &C,
    topic: &str,
    partition: i32,
    next_offset: i64, // offset của message tiếp theo = offset đã xử lý + 1
    key: &str,
    out: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    producer.begin_transaction()?;
    producer
        .send(FutureRecord::to("orders-enriched").key(key).payload(out), Timeout::Never)
        .await
        .map_err(|(e, _)| e)?;
    let mut offsets = TopicPartitionList::new();
    offsets.add_partition_offset(topic, partition, Offset::Offset(next_offset))?;
    let meta = consumer.group_metadata().ok_or("consumer chưa vào group")?;
    producer.send_offsets_to_transaction(&offsets, &meta, Duration::from_secs(30))?;
    producer.commit_transaction(Duration::from_secs(30))?; // lỗi "abortable" thì gọi abort_transaction
    Ok(())
}
```

`commit_transaction` flush các message đang chờ trước khi commit. Nếu một message lỗi vĩnh viễn, transaction vào trạng thái lỗi có thể abort, và phải gọi `abort_transaction` trước khi bắt đầu transaction mới[^rk-commit-tx]. Hàm nhận `offsets` là offset *của message kế tiếp sẽ đọc*, tức offset đã xử lý cộng một[^rk-send-offsets-next].

**`tokio::sync::mpsc`: queue *trong* process.** Module `mpsc` là hàng đợi nhiều-producer một-consumer để gửi giá trị giữa các task async[^tk-mpsc]. Biến thể bounded giới hạn số message, và `send` chờ khi đầy[^tk-bounded]; đó chính là backpressure đã học ở [P7 · Chương 09, mục 3](../07-io-async/09-async-trong-production.md). So với log của Kafka (nhận xét của người soạn):

| | `tokio::sync::mpsc` | Log kiểu Kafka |
|---|---|---|
| Phạm vi | trong một process | giữa nhiều process/máy |
| Consumer | một consumer[^tk-mpsc] | mỗi partition một consumer trong mỗi group[^k-group-excl] |
| Giữ sau khi nhận | không (message chuyển cho receiver) | giữ tới hết retention[^k-notdeleted] |
| Đọc lại | không | được, bằng cách tua lại offset[^k-rewind] |
| Bền qua crash | message nằm trong bộ nhớ process nên mất khi process chết (suy luận của người soạn) | log trên đĩa broker, replicate theo partition[^k-repl-unit] |

Từ đó suy ra cách ghép thường gặp trong một service như `billing` (suy luận của người soạn): consumer Kafka đọc message rồi đẩy vào `mpsc` bounded cho một pool worker, và offset chỉ được commit *sau* khi worker xử lý xong. Nếu commit sớm hơn thì channel trở thành chỗ mất message: hàng đợi trong RAM đã "ack" lên Kafka mà chưa xử lý.

**Stream processing viết bằng Rust.** Docs `rdkafka` liệt kê một số dự án dùng nó, trong đó có `timely-dataflow`, một compute engine song song dữ liệu phân tán[^rk-users]. Bài Dataflow và docs Flink ở trên là nguồn khái niệm chung cho các engine đó.

## Tóm tắt

- Event là bản ghi "đã có chuyện" (key, value, timestamp, header); stream là chuỗi event không biên mà producer ghi và consumer đọc, tách rời nhau. Unbounded là tính chất dữ liệu, streaming là kiểu engine.
- Queue: broker theo dõi ack rồi bỏ message, chia việc cho worker. Log: broker giữ theo retention, consumer giữ offset, nên đọc lại và nhiều bên cùng đọc là chuyện bình thường.
- Kafka: topic = N partition; key → partition; thứ tự chỉ có trong partition; mỗi partition một consumer trong mỗi group; retention theo segment, hoặc compaction giữ giá trị mới nhất mỗi key.
- Mất/trùng sinh ra từ "không biết đã thành công chưa". "Xử lý rồi lưu offset" → at-least-once; "lưu offset rồi xử lý" → at-most-once. Idempotent producer + transaction cho exactly-once *trong Kafka*; với hệ ngoài thì ghi idempotent hoặc ghi offset cùng chỗ với kết quả.
- CDC của PostgreSQL = logical decoding đọc WAL, slot phát theo thứ tự commit, at-least-once sau crash, và slot giữ WAL nên phải theo dõi. Outbox tránh ghi hai nơi. Event sourcing lưu event làm sự thật và trả giá bằng độ phức tạp.
- Stream và bảng là hai mặt của cùng dữ liệu: stream là changelog, bảng là ảnh chụp giá trị mới nhất mỗi key.
- Event time là lúc xảy ra, processing time là lúc xử lý; window có tumbling, hopping, session; watermark là ước lượng tiến độ event time và có thể quá nhanh hoặc quá chậm, nên cần allowed lateness/grace và trigger.
- Stream join cần state có giới hạn thời gian, co-partitioning, và một chính sách cho dữ liệu sai thứ tự.
- Chịu lỗi cho state: checkpoint (barrier + offset nguồn) hoặc changelog (+ standby); cả hai cần nguồn phát lại được; exactly-once của engine là về state nội bộ của nó.
- Rust: `rdkafka` (`StreamConsumer`, `FutureProducer`, transaction API; commit offset *sau* xử lý; các hàm transaction có thể chặn); `tokio::sync::mpsc` là queue trong process, có backpressure nhưng không bền.

## Câu hỏi tự kiểm tra

1. Vì sao docs Dataflow ưa cặp "unbounded/bounded" hơn "streaming/batch"?
2. Nêu ba khác biệt giữa broker kiểu queue và kiểu log. Vì sao Kafka chỉ cần lưu *một số nguyên* mỗi partition cho mỗi consumer group?
3. Một topic có 4 partition, một group có 6 consumer. Bao nhiêu consumer thực sự nhận dữ liệu, và vì sao? Đâu là phần suy luận, đâu là phần có trong docs?
4. Thứ tự "lưu offset rồi xử lý" cho mức bảo đảm nào? Còn thứ tự ngược lại? Khi nào at-least-once vẫn cho kết quả đúng?
5. Idempotent producer khử trùng bằng cách nào, và nó không bảo vệ khỏi tình huống nào (gợi ý: consumer xử lý và ghi ra database ngoài)?
6. Trong transaction consume–transform–produce của Kafka, offset đầu vào nằm ở đâu và vì sao điều đó cho exactly-once? `read_committed` và LSO làm consumer thấy gì?
7. Nêu hai điều có thể làm hỏng một pipeline CDC dựa trên logical decoding của PostgreSQL ở phía *vận hành*, và cách phòng.
8. Event sourcing khác CDC ở đâu? Vì sao nó xung đột với yêu cầu xóa dữ liệu cá nhân (chỉ cần nêu, chi tiết ở Chương 14)?
9. Event time và processing time khác nhau thế nào? Tumbling, hopping, session window khác nhau ở đâu? Vì sao watermark "quá nhanh" và "quá chậm" đều là vấn đề?
10. Vì sao join stream–stream cần co-partitioning và window? Chuyện gì xảy ra với stream–table join khi bảng cập nhật sai thứ tự và không dùng versioned store?
11. So checkpoint của Flink với changelog của Kafka Streams. Cả hai dựa trên điều kiện nào của nguồn dữ liệu? "Exactly-once" của Flink thực sự bảo đảm điều gì?
12. Với `rdkafka`, vì sao nên commit offset *sau* khi xử lý, và vì sao `commit_message` cộng 1 vào offset? Vì sao các hàm transaction không nên gọi trực tiếp trên worker thread của Tokio?

<details><summary>Đáp án</summary>

1. "Streaming/batch" ngụ ý một loại engine cụ thể[^df-unbounded-def]. Thực tế dữ liệu vô hạn từng được xử lý bằng batch lặp, và engine streaming tốt xử lý được dữ liệu hữu hạn[^df-unbounded].
2. Queue: message bị bỏ sau ack[^rmq-ack-discard]. Log giữ tới hết retention[^k-notdeleted]. Queue chia round-robin[^rmq-rr]. Log chia theo partition[^k-group-excl]. Log còn cho tua lại offset[^k-rewind]. Kafka chỉ cần một số nguyên vì mỗi partition được đúng một consumer trong group đọc, nên vị trí chỉ là offset của message kế tiếp[^k-offset-one].
3. Docs: mỗi partition do đúng một consumer trong group đọc tại một thời điểm[^k-group-excl]. Số instance chạy song song tối đa bị chặn bởi số partition đầu vào, ví dụ 5 partition thì tối đa 5 instance[^ks-maxpar]. Suy luận: chỉ 4 consumer nhận dữ liệu, 2 consumer nhàn rỗi.
4. Lưu offset rồi xử lý → at-most-once[^k-cons-1]. Xử lý rồi lưu offset → at-least-once[^k-cons-alo]. At-least-once vẫn cho kết quả đúng khi cập nhật idempotent (ví dụ upsert theo khóa chính): nhận lại hai lần chỉ ghi đè bằng chính nó[^k-cons-2].
5. Broker gán ID cho producer và khử trùng bằng sequence number[^k-idem]. Nó chỉ bảo vệ việc ghi *vào Kafka*. Phần xử lý rồi ghi ra hệ ngoài cần idempotent hoặc ghi offset cùng chỗ với output[^k-external]. Docs tóm lại rằng với hệ đích khác thì cần cộng tác của hệ đó[^k-external-note].
6. Offset được lưu như một message trong một topic và ghi trong cùng transaction với dữ liệu ra: abort thì offset về giá trị cũ và dữ liệu ra không hiện[^k-offset-in-txn]. `read_committed` chỉ trả message tới LSO, các message sau transaction đang mở bị giữ lại[^kc-iso].
7. Thứ nhất, trùng lặp sau crash: client phải chịu được việc nhận lại cùng thay đổi[^pg-slot-once]. Thứ hai, slot giữ WAL và catalog nên đĩa đầy hoặc có nguy cơ wraparound nếu consumer chết lâu[^pg-slot-caution]. Cách phòng: xóa slot không dùng, và đặt giới hạn `max_slot_wal_keep_size` (mặc định không giới hạn)[^pg-maxslot].
8. CDC suy ra event từ thay đổi của database đang giữ state; event sourcing dùng event store làm system of record[^az-es]. Event bất biến và append-only xung đột với quy định đòi xóa dữ liệu cá nhân[^az-gdpr]; hướng xử lý ở Chương 14.
9. Event time là lúc xảy ra[^df-evdef]. Processing time là lúc xử lý[^df-procdef]. Tumbling: cỡ cố định không chồng lấn[^ksdsl-tumb]. Hopping: cỡ cố định cộng bước tiến, chồng lấn được[^ksdsl-hop]. Session: ngăn cách bởi gap im lặng, cỡ thay đổi[^ksdsl-sess2]. Watermark quá nhanh thì dữ liệu trễ nằm sau watermark[^df-fast]. Quá chậm thì một datum chậm giữ cả pipeline[^df-slow].
10. Stream–stream: hai dòng không đồng bộ nên phải nhớ cả hai phía trong một khoảng thời gian (window hoặc interval)[^fl-win-join]. Interval join cũng như vậy, theo khoảng tương đối giữa hai timestamp[^fl-interval]. Co-partitioning để record cùng key tới cùng task[^ksj-copart]. Stream–table không versioned: chỉ xử lý theo offset, có thể cho kết quả không đoán trước[^ks-join-ooo].
11. Checkpoint chụp state mọi operator tại barrier cùng offset nguồn[^flink-barrier]. Changelog thì mỗi store ghi cập nhật vào topic để phát lại[^ks-changelog]. Cả hai cần nguồn phát lại được[^flink-replay]. "Exactly-once" của Flink nghĩa là mỗi event ảnh hưởng *state do Flink quản lý* đúng một lần, chứ không phải được xử lý đúng một lần[^flink-eo].
12. Commit sớm rồi chết thì lúc phục hồi consumer bắt đầu từ message kế tiếp và bỏ qua message lỗi[^rk-alo]. Offset commit là *vị trí message kế sẽ đọc* nên cộng 1[^rk-commit-plus1]. Các hàm transaction có thể chặn tới hết timeout[^rk-tx-block]; chặn trên worker thread async làm đói các task khác (nhận xét của người soạn, xem P7·09 mục 5).

</details>

## Đọc thêm

- DDIA (ấn bản 1) chương 11 *Stream Processing* là chương tương ứng của chương này; mục lục chương liệt kê "Transmitting Event Streams", "Databases and Streams" và "Processing Streams"[^toc-ddia11]. Sách chỉ dùng làm khung chủ đề, không trích; giới thiệu sách ở <https://dataintensive.net/>. Nên đọc sau khi đã nắm nguồn mở ở chương này để so cách trình bày. Chương 5 *Replication* nói về log[^toc-ddia5]. Chương 10 *Batch Processing* là phía đối chiếu[^toc-ddia10]. Chương 12 *The Future of Data Systems* nói về dữ liệu dẫn xuất[^toc-ddia12].
- [Kafka 3.9.0 — Design (docs/design.html)](https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html): mục 4.5 (Consumer, vị trí consumer), 4.6 (Message Delivery Semantics), 4.7 (Replication), Log Compaction — nguồn chính của mục 2–4.
- [Kafka 3.9.0 — Kafka Streams core concepts](https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html) và [architecture](https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/architecture.html): time, windowing, stream–table duality, exactly-once, out-of-order, state/changelog/standby.
- [The Dataflow Model (VLDB 2015)](https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf): mục 1.2–2.4 (window, trigger, watermark, accumulating/discarding) — khung tư duy cho event time.
- [Flink 1.20 — Event Time and Watermarks](https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/concepts/time/) và [Fault Tolerance (Learn Flink)](https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/learn-flink/fault_tolerance/): watermark, barrier snapshot, exactly-once đầu-cuối.
- [PostgreSQL 16 — Chương 49 Logical Decoding](https://www.postgresql.org/docs/16/logicaldecoding.html): 49.2 (khái niệm, slot), 49.6 (output plugin); cộng [`ALTER TABLE … REPLICA IDENTITY`](https://www.postgresql.org/docs/16/sql-altertable.html).
- [Debezium 2.7 — PostgreSQL connector](https://debezium.io/documentation/reference/2.7/connectors/postgresql.html) và [Outbox Event Router](https://debezium.io/documentation/reference/2.7/transformations/outbox-event-router.html): vận hành CDC thực tế.
- [Azure Architecture Center — Event Sourcing pattern](https://learn.microsoft.com/en-us/azure/architecture/patterns/event-sourcing): đánh đổi, snapshot, upcasting, dữ liệu cá nhân.
- [Cambridge — Distributed Systems lecture notes (Kleppmann)](https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf): phần RPC, retry, idempotence, at-most/at-least/exactly-once.
- [docs.rs `rdkafka` 0.39.0](https://docs.rs/rdkafka/0.39.0/rdkafka/): mục "At-least-once delivery", "Exactly-once semantics", module `producer` (Transactions) và `consumer`.
- [P7 · Chương 09 — Async trong production](../07-io-async/09-async-trong-production.md): backpressure (`mpsc` bounded), blocking trong async — nền cho phần Rust.

[^k-event]: Apache Kafka 3.9.0, introduction (Main Concepts and Terminology) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/introduction.html> — "records the fact that "something happened" in the world or in your business. It is also called record or message in the documentation."
[^k-event-fields]: Apache Kafka 3.9.0, introduction — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/introduction.html> — "Conceptually, an event has a key, value, timestamp, and optional metadata headers."
[^k-event-example]: Apache Kafka 3.9.0, introduction — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/introduction.html> — "Event value: "Made a payment of $200 to Bob""
[^k-decoupled]: Apache Kafka 3.9.0, introduction — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/introduction.html> — "In Kafka, producers and consumers are fully decoupled and agnostic of each other, which is a key design element to achieve the high scalability that Kafka is known for."
[^k-topicmulti]: Apache Kafka 3.9.0, introduction — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/introduction.html> — "Topics in Kafka are always multi-producer and multi-subscriber: a topic can have zero, one, or many producers that write events to it, as well as zero, one, or many consumers that subscribe to these events."
[^k-notdeleted]: Apache Kafka 3.9.0, introduction — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/introduction.html> — "unlike traditional messaging systems, events are not deleted after consumption"
[^k-retain]: Apache Kafka 3.9.0, introduction — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/introduction.html> — "Instead, you define for how long Kafka should retain your events through a per-topic configuration setting, after which old events will be discarded."
[^k-partition]: Apache Kafka 3.9.0, introduction — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/introduction.html> — "When a new event is published to a topic, it is actually appended to one of the topic's partitions. Events with the same event key (e.g., a customer or vehicle ID) are written to the same partition"
[^k-partition-order]: Apache Kafka 3.9.0, introduction — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/introduction.html> — "that any consumer of a given topic-partition will always read that partition's events in exactly the same order as they were written."
[^df-unbounded]: Akidau et al., The Dataflow Model (VLDB 2015), 1.1 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "unbounded datasets have been processed using repeated runs of batch systems since their conception, and well-designed streaming systems are perfectly capable of processing bounded data."
[^df-unbounded-def]: Akidau et al., The Dataflow Model (VLDB 2015), 1.1 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "When describing infinite/finite data sets, we prefer the terms unbounded/bounded over streaming/batch"
[^df-groupbykey]: Akidau et al., The Dataflow Model (VLDB 2015), 2.1 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "The GroupByKey operation, on the other hand, collects all data for a given key before sending them downstream for reduction. If the input source is unbounded, we have no way of knowing when it will end. The common solution to this problem is to window the data."
[^df-four]: Akidau et al., The Dataflow Model (VLDB 2015), 1 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "What results are being computed. – Where in event time they are being computed. – When in processing time they are materialized. – How earlier results relate to later refinements."
[^k-broker-state]: Apache Kafka 3.9.0, design (Consumer Position) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "Most messaging systems keep metadata about what messages have been consumed on the broker."
[^k-ack-problem]: Apache Kafka 3.9.0, design (Consumer Position) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "if the consumer processes the message but fails before it can send an acknowledgement then the message will be consumed twice."
[^k-group-excl]: Apache Kafka 3.9.0, design (Consumer Position) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "each of which is consumed by exactly one consumer within each subscribing consumer group at any given time."
[^k-offset-one]: Apache Kafka 3.9.0, design (Consumer Position) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "the position of a consumer in each partition is just a single integer, the offset of the next message to consume."
[^k-rewind]: Apache Kafka 3.9.0, design (Consumer Position) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "back to an old offset and re-consume data. This violates the common contract of a queue, but turns out to be an essential feature for many consumers."
[^k-pull]: Apache Kafka 3.9.0, design (Push vs. pull) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "A pull-based system has the nicer property that the consumer simply falls behind and catches up when it can."
[^rmq-workq]: RabbitMQ tutorial two, Work Queues — <https://www.rabbitmq.com/tutorials/tutorial-two-python> — "The main idea behind Work Queues (aka: Task Queues) is to avoid doing a resource-intensive task immediately and having to wait for it to complete."
[^rmq-rr]: RabbitMQ tutorial two, Round-robin dispatching — <https://www.rabbitmq.com/tutorials/tutorial-two-python> — "By default, RabbitMQ will send each message to the next consumer, in sequence. On average every consumer will get the same number of messages. This way of distributing messages is called round-robin."
[^rmq-ack-discard]: RabbitMQ, Consumer Acknowledgements and Publisher Confirms — <https://www.rabbitmq.com/docs/confirms> — "Positive acknowledgements simply instruct RabbitMQ to record a message as delivered and can be discarded."
[^rmq-requeue]: RabbitMQ, Consumer Acknowledgements and Publisher Confirms — <https://www.rabbitmq.com/docs/confirms> — "any delivery (message) that was not acked is automatically requeued when the channel (or connection) on which the delivery happened is closed."
[^rmq-idem]: RabbitMQ, Consumer Acknowledgements and Publisher Confirms — <https://www.rabbitmq.com/docs/confirms> — "Due to this behavior, consumers must be prepared to handle redeliveries and otherwise be implemented with idempotence in mind."
[^k-pagecache]: Apache Kafka 3.9.0, design (Persistence) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "All data is immediately written to a persistent log on the filesystem without necessarily flushing to disk. In effect this just means that it is transferred into the kernel's pagecache."
[^k-O1]: Apache Kafka 3.9.0, design (Constant Time Suffices) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "This structure has the advantage that all operations are O(1) and reads do not block writes or each other."
[^k-retain-week]: Apache Kafka 3.9.0, design (Constant Time Suffices) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "instead of attempting to delete messages as soon as they are consumed, we can retain messages for a relatively long period (say a week)."
[^k-sendfile]: Apache Kafka 3.9.0, design (Efficiency) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "Using sendfile, this re-copying is avoided by allowing the OS to send the data from pagecache to the network directly."
[^k-keypartition]: Apache Kafka 3.9.0, design (Load balancing) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "if the key chosen was a user id then all data for a given user would be sent to the same partition."
[^k-offset-id]: Apache Kafka 3.9.0, implementation (Log) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/implementation.html> — "we decided to use a simple per-partition atomic counter which could be coupled with the partition id and node id to uniquely identify a message"
[^k-fetch]: Apache Kafka 3.9.0, design (The Consumer) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "The consumer specifies its offset in the log with each request and receives back a chunk of log beginning from that position."
[^kc-group-id]: Apache Kafka 3.9.0, consumer configs (group.id) — <https://kafka.apache.org/39/configuration/consumer-configs/> — "A unique string that identifies the consumer group this consumer belongs to."
[^ks-tasks]: Apache Kafka 3.9.0, Kafka Streams architecture (Stream Partitions and Tasks) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/architecture.html> — "Kafka Streams creates a fixed number of tasks based on the input stream partitions for the application, with each task assigned a list of partitions from the input streams"
[^ks-maxpar]: Apache Kafka 3.9.0, Kafka Streams architecture — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/architecture.html> — "if your input topic has 5 partitions, then you can run up to 5 applications instances."
[^kc-poll-interval]: Apache Kafka 3.9.0, consumer configs (max.poll.interval.ms) — <https://kafka.apache.org/39/configuration/consumer-configs/> — "If poll() is not called before expiration of this timeout, then the consumer is considered failed and the group will rebalance in order to reassign the partitions to another member."
[^k-static]: Apache Kafka 3.9.0, design (Static Membership) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "For large state applications, shuffled tasks need a long time to recover their local states before processing and cause applications to be partially or entirely unavailable."
[^k-static-detail]: Apache Kafka 3.9.0, design (Static Membership) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "a fencing mechanism on broker side will inform your duplicate client to shutdown immediately by triggering a"
[^k-impl-delete]: Apache Kafka 3.9.0, implementation (Deletes) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/implementation.html> — "Data is deleted one log segment at a time."
[^k-impl-retention]: Apache Kafka 3.9.0, implementation (Deletes) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/implementation.html> — "For time-based policies, the record timestamps are considered, with the largest timestamp in a segment file (order of records is not relevant) defining the retention time for the entire segment."
[^k-impl-size]: Apache Kafka 3.9.0, implementation (Deletes) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/implementation.html> — "Size-based retention is disabled by default. When enabled the log manager keeps deleting the oldest segment file until the overall size of the partition is within the configured limit again."
[^k-compaction]: Apache Kafka 3.9.0, design (Log Compaction) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "Log compaction gives us a more granular retention mechanism so that we are guaranteed to retain at least the last update for each primary key"
[^k-tombstone]: Apache Kafka 3.9.0, design (Log Compaction Basics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "A message with a key and a null payload will be treated as a delete from the log."
[^k-compact-order]: Apache Kafka 3.9.0, design (Log Compaction Details) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "Ordering of messages is always maintained. Compaction will never re-order messages, just remove some."
[^k-compact-offsets]: Apache Kafka 3.9.0, design (Log Compaction Details) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "The offset for a message never changes. It is the permanent identifier for a position in the log."
[^k-compact-uses]: Apache Kafka 3.9.0, design (Log Compaction) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "A process that does local computation can be made fault-tolerant by logging out changes that it makes to its local state so another process can reload these changes and carry on if it should fail."
[^k-compact-cdc]: Apache Kafka 3.9.0, design (Log Compaction) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "Each change to the database will need to be reflected in the cache, the search cluster, and eventually in Hadoop."
[^k-compact-es]: Apache Kafka 3.9.0, design (Log Compaction) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "This is a style of application design which co-locates query processing with application design and uses a log of changes as the primary store for the application."
[^k-repl-unit]: Apache Kafka 3.9.0, design (Replication) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "The unit of replication is the topic partition. Under non-failure conditions, each partition in Kafka has a single leader and zero or more followers."
[^k-committed]: Apache Kafka 3.9.0, design (Replicated Logs) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "We can now more precisely define that a message is considered committed when all replicas in the ISR for that partition have applied it to their log."
[^k-committed-only]: Apache Kafka 3.9.0, design (Replicated Logs) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "Only committed messages are ever given out to the consumer."
[^k-committed-lost]: Apache Kafka 3.9.0, design (Replicated Logs) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "The guarantee that Kafka offers is that a committed message will not be lost, as long as there is at least one in sync replica alive, at all times."
[^ks-ts-choice]: Apache Kafka 3.9.0, Kafka Streams core concepts (Time) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "The choice between event-time and ingestion-time is actually done through the configuration of Kafka (not Kafka Streams)"
[^ks-oo-sources]: Apache Kafka 3.9.0, Kafka Streams core concepts (Out-of-Order Handling) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "Within a topic-partition, a record's timestamp may not be monotonically increasing along with their offsets."
[^k-three]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "Messages may be lost but are never redelivered."
[^k-three-2]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "Messages are never lost but may be redelivered."
[^k-three-3]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "this is what people actually want, each message is delivered once and only once."
[^k-two-problems]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "this breaks down into two problems: the durability guarantees for publishing a message and the guarantees when consuming a message."
[^k-exactly-fine]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "it is important to read the fine print, most of these claims are misleading (i.e. they don't translate to the case where consumers or producers can fail, cases where there are multiple consumer processes, or cases where data written to disk can be lost)"
[^k-prod-unsure]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "If a producer attempts to publish a message and experiences a network error it cannot be sure if this error happened before or after the message was committed."
[^k-prod-resend]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "This provides at-least-once delivery semantics since the message may be written to the log again during resending if the original request had in fact succeeded."
[^k-idem]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "the broker assigns each producer an ID and deduplicates messages using a sequence number that is sent by the producer along with every message."
[^kp-idem]: Apache Kafka 3.9.0, producer configs (enable.idempotence) — <https://kafka.apache.org/39/configuration/producer-configs/> — "Note that enabling idempotence requires max.in.flight.requests.per.connection to be less than or equal to 5 (with message ordering preserved for any allowable value), retries to be greater than 0, and acks must be 'all'."
[^kp-acks-all]: Apache Kafka 3.9.0, producer configs (acks) — <https://kafka.apache.org/39/configuration/producer-configs/> — "This guarantees that the record will not be lost as long as at least one in-sync replica remains alive."
[^k-cons-1]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "This corresponds to "at-most-once" semantics as in the case of a consumer failure messages may not be processed."
[^k-cons-2]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "In many cases messages have a primary key and so the updates are idempotent (receiving the same message twice just overwrites a record with another copy of itself)."
[^k-default-alo]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "Otherwise, Kafka guarantees at-least-once delivery by default, and allows the user to implement at-most-once delivery by disabling retries on the producer and committing offsets in the consumer prior to processing a batch of messages."
[^k-txn]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "the producer supports the ability to send messages to multiple topic partitions using transaction-like semantics: i.e. either all messages are successfully written or none of them are."
[^k-offset-in-txn]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "The consumer's position is stored as a message in a topic, so we can write the offset to Kafka in the same transaction as the output topics receiving the processed data."
[^k-readcommitted]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "In the default "read_uncommitted" isolation level, all messages are visible to consumers even if they were part of an aborted transaction, but in "read_committed," the consumer will only return messages from transactions which were committed (and any messages which were not part of a transaction)."
[^k-external]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "this can be handled more simply and generally by letting the consumer store its offset in the same place as its output. This is better because many of the output systems a consumer might want to write to will not support a two-phase commit."
[^k-external-note]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "Exactly-once delivery for other destination systems generally requires cooperation with such systems, but Kafka provides the offset which makes implementing this feasible"
[^cam-eo]: Kleppmann, Distributed Systems lecture notes (Cambridge), slide 89 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "Exactly-once semantics: retry + idempotence or deduplication"
[^cam-idem]: Kleppmann, Distributed Systems lecture notes (Cambridge), slide 90 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "Idempotence allows an update to have exactly-once semantics: that is, the update may actually be applied multiple times, but the effect is the same as if it had been applied exactly once."
[^cam-counter]: Kleppmann, Distributed Systems lecture notes (Cambridge), slide 90 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "Incrementing a counter is not idempotent, but adding an element to a set is."
[^cam-dedup]: Kleppmann, Distributed Systems lecture notes (Cambridge), slide 89 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "in a crash-recovery system model, this requires storing requests (or some metadata about requests, such as a vector clock) in stable storage, so that duplicates can be accurately detected even after a crash."
[^e2e-dup]: Saltzer, Reed, Clark, End-to-End Arguments in System Design, Duplicate message suppression — <https://web.mit.edu/Saltzer/www/publications/endtoend/endtoend.pdf> — "These application level duplications look like different messages to the communication system, so it cannot suppress them; suppression must be accomplished by the application itself with knowledge of how to detect its own duplicates."
[^kc-iso]: Apache Kafka 3.9.0, consumer configs (isolation.level) — <https://kafka.apache.org/39/configuration/consumer-configs/> — "in read_committed mode, consumer.poll() will only return messages up to the last stable offset (LSO), which is the one less than the offset of the first open transaction."
[^kc-iso-default]: Apache Kafka 3.9.0, consumer configs (isolation.level) — <https://kafka.apache.org/39/configuration/consumer-configs/> — "If set to read_uncommitted (the default), consumer.poll() will return all messages, even transactional messages which have been aborted."
[^kp-txid]: Apache Kafka 3.9.0, producer configs (transactional.id) — <https://kafka.apache.org/39/configuration/producer-configs/> — "If a TransactionalId is configured, enable.idempotence is implied. By default the TransactionId is not configured, which means transactions cannot be used. Note that, by default, transactions require a cluster of at least three brokers"
[^kp-txid2]: Apache Kafka 3.9.0, producer configs (transactional.id) — <https://kafka.apache.org/39/configuration/producer-configs/> — "This enables reliability semantics which span multiple producer sessions since it allows the client to guarantee that transactions using the same TransactionalId have been completed prior to starting any new transactions."
[^ks-eos]: Apache Kafka 3.9.0, Kafka Streams core concepts (Processing Guarantees) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "Kafka Streams tightly integrates with the underlying Kafka storage system and ensure that commits on the input topic offsets, updates on the state stores, and writes to the output topics will be completed atomically instead of treating Kafka as an external system that may have side-effects."
[^ks-pg]: Apache Kafka 3.9.0, Kafka Streams core concepts (Processing Guarantees) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "To enable exactly-once semantics when running Kafka Streams applications,"
[^ks-pg-default]: Apache Kafka 3.9.0, Kafka Streams core concepts (Processing Guarantees) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "config value (default value is"
[^ks-eos-v2]: Apache Kafka 3.9.0, Kafka Streams core concepts (Processing Guarantees) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "As of the 3.0.0 release, the first version of exactly-once has been deprecated. Users are encouraged to use exactly-once v2 for exactly-once processing from now on"
[^flink-eo]: Apache Flink 1.20, Learn Flink: Fault Tolerance (Exactly Once Guarantees) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/learn-flink/fault_tolerance/> — "when the ideal situation is described as exactly once this does not mean that every event will be processed exactly once. Instead, it means that every event will affect the state being managed by Flink exactly once."
[^flink-e2e]: Apache Flink 1.20, Learn Flink: Fault Tolerance (Exactly Once End-to-end) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/learn-flink/fault_tolerance/> — "your sources must be replayable, and your sinks must be transactional (or idempotent)"
[^flink-three]: Apache Flink 1.20, Learn Flink: Fault Tolerance (Exactly Once Guarantees) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/learn-flink/fault_tolerance/> — "Nothing is lost, but you may experience duplicated results (at least once)"
[^flink-abs]: Apache Flink 1.20, Learn Flink: Fault Tolerance (How does State Snapshotting Work) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/learn-flink/fault_tolerance/> — "Flink uses a variant of the Chandy-Lamport algorithm known as asynchronous barrier snapshotting."
[^flink-barrier]: Apache Flink 1.20, Learn Flink: Fault Tolerance — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/learn-flink/fault_tolerance/> — "it has all of the sources record their offsets and insert numbered checkpoint barriers into their streams. These barriers flow through the job graph, indicating the part of the stream before and after each checkpoint."
[^flink-cp-n]: Apache Flink 1.20, Learn Flink: Fault Tolerance — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/learn-flink/fault_tolerance/> — "Checkpoint n will contain the state of each operator that resulted from having consumed every event before checkpoint barrier n, and none of the events after it."
[^flink-align]: Apache Flink 1.20, Learn Flink: Fault Tolerance — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/learn-flink/fault_tolerance/> — "Operators with two input streams (such as a CoProcessFunction) perform barrier alignment so that the snapshot will reflect the state resulting from consuming events from both input streams up to (but not past) both barriers."
[^flink-replay]: Apache Flink 1.20, Learn Flink: Fault Tolerance — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/learn-flink/fault_tolerance/> — "Given that Flink recovers from faults by rewinding and replaying the source data streams"
[^flink-alo]: Apache Flink 1.20, Learn Flink: Fault Tolerance — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/learn-flink/fault_tolerance/> — "Barrier alignment is only needed for providing exactly once guarantees."
[^flink-ckstorage]: Apache Flink 1.20, Learn Flink: Fault Tolerance (Glossary) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/learn-flink/fault_tolerance/> — "Checkpoint – a snapshot taken automatically by Flink for the purpose of being able to recover from faults. Checkpoints can be incremental, and are optimized for being restored quickly."
[^flink-savepoint]: Apache Flink 1.20, Learn Flink: Fault Tolerance (Glossary) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/learn-flink/fault_tolerance/> — "Savepoint – a snapshot triggered manually by a user (or an API call) for some operational purpose, such as a stateful redeploy/upgrade/rescaling operation."
[^flink-ckstore2]: Apache Flink 1.20, Learn Flink: Fault Tolerance (Checkpoint Storage) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/learn-flink/fault_tolerance/> — "The location where these snapshots are stored is defined via the jobs checkpoint storage."
[^pg-ld-def]: PostgreSQL 16, 49.2.1 Logical Decoding — <https://www.postgresql.org/docs/16/logicaldecoding-explanation.html> — "Logical decoding is the process of extracting all persistent changes to a database's tables into a coherent, easy to understand format which can be interpreted without detailed knowledge of the database's internal state."
[^pg-ld-wal]: PostgreSQL 16, 49.2.1 Logical Decoding — <https://www.postgresql.org/docs/16/logicaldecoding-explanation.html> — "In PostgreSQL, logical decoding is implemented by decoding the contents of the write-ahead log, which describe changes on a storage level, into an application-specific form such as a stream of tuples or SQL statements."
[^pg-wal-level]: PostgreSQL 16, wal_level — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "logical adds information necessary to support logical decoding."
[^pg-slot]: PostgreSQL 16, 49.2.2 Replication Slots — <https://www.postgresql.org/docs/16/logicaldecoding-explanation.html> — "In the context of logical replication, a slot represents a stream of changes that can be replayed to a client in the order they were made on the origin server. Each slot streams a sequence of changes from a single database."
[^pg-slot-persist]: PostgreSQL 16, 49.2.2 Replication Slots — <https://www.postgresql.org/docs/16/logicaldecoding-explanation.html> — "Slots persist independently of the connection using them and are crash-safe."
[^pg-slot-once]: PostgreSQL 16, 49.2.2 Replication Slots — <https://www.postgresql.org/docs/16/logicaldecoding-explanation.html> — "A logical slot will emit each change just once in normal operation. The current position of each slot is persisted only at checkpoint, so in the case of a crash the slot may return to an earlier LSN, which will then cause recent changes to be sent again when the server restarts. Logical decoding clients are responsible for avoiding ill effects from handling the same message more than once."
[^pg-slot-caution]: PostgreSQL 16, 49.2.2 Replication Slots — <https://www.postgresql.org/docs/16/logicaldecoding-explanation.html> — "Replication slots persist across crashes and know nothing about the state of their consumer(s). They will prevent removal of required resources even when there is no connection using them. This consumes storage because neither required WAL nor required rows from the system catalogs can be removed by VACUUM as long as they are required by a replication slot."
[^pg-maxslot]: PostgreSQL 16, max_slot_wal_keep_size — <https://www.postgresql.org/docs/16/runtime-config-replication.html> — "If max_slot_wal_keep_size is -1 (the default), replication slots may retain an unlimited amount of WAL files."
[^pg-commit-order]: PostgreSQL 16, 49.6.1 Initialization / Logical Decoding Output Plugins — <https://www.postgresql.org/docs/16/logicaldecoding-output-plugin.html> — "Concurrent transactions are decoded in commit order, and only changes belonging to a specific transaction are decoded between the begin and commit callbacks. Transactions that were rolled back explicitly or implicitly never get decoded."
[^pg-flushed]: PostgreSQL 16, Logical Decoding Output Plugins — <https://www.postgresql.org/docs/16/logicaldecoding-output-plugin.html> — "Only transactions that have already safely been flushed to disk will be decoded."
[^pg-plugin]: PostgreSQL 16, 49.2.3 Output Plugins — <https://www.postgresql.org/docs/16/logicaldecoding-explanation.html> — "Output plugins transform the data from the write-ahead log's internal representation into the format the consumer of a replication slot desires."
[^pg-consume]: PostgreSQL 16, Chapter 49 Logical Decoding — <https://www.postgresql.org/docs/16/logicaldecoding.html> — "Changes can be consumed either using the streaming replication protocol (see Section 55.4 and Section 49.3), or by calling functions via SQL (see Section 49.4)."
[^pg-ri]: PostgreSQL 16, Chapter 49 Logical Decoding — <https://www.postgresql.org/docs/16/logicaldecoding.html> — "Availability of old row versions for UPDATE and DELETE depends on the configured replica identity"
[^pg-ri-default]: PostgreSQL 16, ALTER TABLE (REPLICA IDENTITY) — <https://www.postgresql.org/docs/16/sql-altertable.html> — "Records the old values of the columns of the primary key, if any. This is the default for non-system tables."
[^pg-ri-full]: PostgreSQL 16, ALTER TABLE (REPLICA IDENTITY) — <https://www.postgresql.org/docs/16/sql-altertable.html> — "Records the old values of all columns in the row."
[^pg-snapshot]: PostgreSQL 16, 49.2.4 Exported Snapshots — <https://www.postgresql.org/docs/16/logicaldecoding-explanation.html> — "a snapshot is exported (see Section 9.27.5), which will show exactly the state of the database after which all changes will be included in the change stream."
[^pg-ex]: PostgreSQL 16, 49.1 Logical Decoding Examples — <https://www.postgresql.org/docs/16/logicaldecoding-example.html> — "SELECT * FROM pg_create_logical_replication_slot('regression_slot', 'test_decoding', false, true);"
[^pg-ex-consumed]: PostgreSQL 16, 49.1 Logical Decoding Examples — <https://www.postgresql.org/docs/16/logicaldecoding-example.html> — "Once changes are read, they're consumed and not emitted"
[^pg-ex-peek]: PostgreSQL 16, 49.1 Logical Decoding Examples — <https://www.postgresql.org/docs/16/logicaldecoding-example.html> — "You can also peek ahead in the change stream without consuming changes"
[^deb-snap]: Debezium 2.7, PostgreSQL connector — <https://debezium.io/documentation/reference/2.7/connectors/postgresql.html> — "The first time it connects to a PostgreSQL server or cluster, the connector takes a consistent snapshot of all schemas. After that snapshot is complete, the connector continuously captures row-level changes that insert, update, and delete database content and that were committed to a PostgreSQL database."
[^deb-resume]: Debezium 2.7, PostgreSQL connector — <https://debezium.io/documentation/reference/2.7/connectors/postgresql.html> — "As the connector reads changes and produces events, it records the WAL position for each event. If the connector stops for any reason (including communication failures, network problems, or crashes), upon restart the connector continues reading the WAL where it last left off."
[^deb-eo]: Debezium 2.7, PostgreSQL connector — <https://debezium.io/documentation/reference/2.7/connectors/postgresql.html> — "When the system is operating normally or being managed carefully then Debezium provides exactly once delivery of every change event record."
[^deb-alo]: Debezium 2.7, PostgreSQL connector (Kafka Connect process stops / crashes) — <https://debezium.io/documentation/reference/2.7/connectors/postgresql.html> — "In these abnormal situations, Debezium, like Kafka, provides at least once delivery of change events."
[^deb-outbox]: Debezium 2.7, Outbox Event Router — <https://debezium.io/documentation/reference/2.7/transformations/outbox-event-router.html> — "An outbox pattern implementation avoids inconsistencies between a service's internal state (as typically persisted in its database) and state in events consumed by services that need the same data."
[^az-es]: Azure Architecture Center, Event Sourcing pattern — <https://learn.microsoft.com/en-us/azure/architecture/patterns/event-sourcing> — "store the full series of actions taken on an object in an append-only store. The store acts as the system of record that you can use to materialize the domain objects."
[^az-replay]: Azure Architecture Center, Event Sourcing pattern — <https://learn.microsoft.com/en-us/azure/architecture/patterns/event-sourcing> — "Each entity in an event-sourced system has its own eventstream, which is the ordered sequence of events that records every change to that entity. At any point, applications can read the history of events. Applications derive the current state of an entity by replaying all the events in its stream. This process is known as rehydration."
[^az-mv]: Azure Architecture Center, Event Sourcing pattern — <https://learn.microsoft.com/en-us/azure/architecture/patterns/event-sourcing> — "Applications typically implement materialized views because it's costly to read and replay events. Materialized views are read-only projections of the event store that are optimized for querying."
[^az-ec]: Azure Architecture Center, Event Sourcing pattern — <https://learn.microsoft.com/en-us/azure/architecture/patterns/event-sourcing> — "The system is only eventually consistent when it creates materialized views or generates projections of data by replaying events."
[^az-complex]: Azure Architecture Center, Event Sourcing pattern — <https://learn.microsoft.com/en-us/azure/architecture/patterns/event-sourcing> — "Event sourcing is a complex pattern that introduces significant trade-offs. It changes how you store data, handle concurrency, evolve schemas, and query state."
[^az-comp]: Azure Architecture Center, Event Sourcing pattern — <https://learn.microsoft.com/en-us/azure/architecture/patterns/event-sourcing> — "The only way to update an entity or undo a change is to add a compensating event to the event store."
[^az-upcast]: Azure Architecture Center, Event Sourcing pattern — <https://learn.microsoft.com/en-us/azure/architecture/patterns/event-sourcing> — "Upcasting: Register transformation functions that convert older event schemas to the current schema during deserialization."
[^az-snap]: Azure Architecture Center, Event Sourcing pattern — <https://learn.microsoft.com/en-us/azure/architecture/patterns/event-sourcing> — "Snapshots are an optimization, not a replacement for the eventstream. The eventstream remains the source of truth, and you can regenerate snapshots from it at any time."
[^az-idem]: Azure Architecture Center, Event Sourcing pattern — <https://learn.microsoft.com/en-us/azure/architecture/patterns/event-sourcing> — "Event delivery to consumers is typically at least once, so consumers can receive the same event more than once."
[^az-gdpr]: Azure Architecture Center, Event Sourcing pattern — <https://learn.microsoft.com/en-us/azure/architecture/patterns/event-sourcing> — "The append-only, immutable nature of an event store conflicts with data protection regulations that require deletion of personal data, such as the right to be forgotten laws."
[^ks-stream-table]: Apache Kafka 3.9.0, Kafka Streams core concepts (Duality of Streams and Tables) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "A stream can be considered a changelog of a table, where each data record in the stream captures a state change of the table."
[^ks-duality]: Apache Kafka 3.9.0, Kafka Streams core concepts (Duality of Streams and Tables) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "this duality means that a stream can be viewed as a table, and a table can be viewed as a stream. Kafka's log compaction feature, for example, exploits this duality."
[^ks-event-time]: Apache Kafka 3.9.0, Kafka Streams core concepts (Time) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "The point in time when an event or data record occurred, i.e. was originally created "at the source"."
[^ks-proc-time]: Apache Kafka 3.9.0, Kafka Streams core concepts (Time) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "The point in time when the event or data record happens to be processed by the stream processing application, i.e. when the record is being consumed."
[^ks-ingest]: Apache Kafka 3.9.0, Kafka Streams core concepts (Time) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "The point in time when an event or data record is stored in a topic partition by a Kafka broker."
[^ks-streamtime]: Apache Kafka 3.9.0, Kafka Streams core concepts (Time) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "As a result, this time will only advance when a new record arrives at the processor."
[^ks-streamtime2]: Apache Kafka 3.9.0, Kafka Streams core concepts (Time) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "when this application is actually executing."
[^df-evdef]: Akidau et al., The Dataflow Model (VLDB 2015), 1.2 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "Event Time, which is the time at which the event itself actually occurred, i.e. a record of system clock time (for whatever system generated the event) at the time of occurrence."
[^df-procdef]: Akidau et al., The Dataflow Model (VLDB 2015), 1.2 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "Processing Time, which is the time at which an event is observed at any given point during processing within the pipeline, i.e. the current time according to the system clock. Note that we make no assumptions about clock synchronization within a distributed system."
[^df-time]: Akidau et al., The Dataflow Model (VLDB 2015), 1.3 Time Domains — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "Event time for a given event essentially never changes, but processing time changes constantly for each event as it flows through the pipeline and time marches ever forward."
[^df-skew]: Akidau et al., The Dataflow Model (VLDB 2015), 1.3 Time Domains — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "During processing, the realities of the systems in use (communication delays, scheduling algorithms, time spent processing, pipeline serialization, etc.) result in an inherent and dynamically changing amount of skew between the two domains."
[^df-fixed]: Akidau et al., The Dataflow Model (VLDB 2015), 1.2 Windowing — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "Fixed windows (sometimes called tumbling windows) are defined by a static window size"
[^df-sliding]: Akidau et al., The Dataflow Model (VLDB 2015), 1.2 Windowing — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "Sliding windows are defined by a window size and slide period, e.g. hourly windows starting every minute."
[^df-fixed-special]: Akidau et al., The Dataflow Model (VLDB 2015), 1.2 Windowing — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "Fixed windows are really a special case of sliding windows where size equals period."
[^df-heur]: Akidau et al., The Dataflow Model (VLDB 2015), 1.3 (footnote 6) — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "For most real-world distributed data sets, the system lacks sufficient knowledge to establish a 100% correct watermark."
[^df-fast]: Akidau et al., The Dataflow Model (VLDB 2015), 2.3 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "They are sometimes too fast, meaning there may be late data that arrives behind the watermark."
[^df-slow]: Akidau et al., The Dataflow Model (VLDB 2015), 2.3 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "Because they are a global progress metric, the watermark can be held back for the entire pipeline by a single slow datum."
[^df-insufficient]: Akidau et al., The Dataflow Model (VLDB 2015), 2.3 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "For these reasons, we postulate that watermarks alone are insufficient."
[^df-trigger-modes]: Akidau et al., The Dataflow Model (VLDB 2015), 2.3 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "Accumulating: Upon triggering, window contents are left intact in persistent state, and later results become a refinement of previous results."
[^df-discarding]: Akidau et al., The Dataflow Model (VLDB 2015), 2.3 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "Discarding: Upon triggering, window contents are discarded, and later results bear no relation to previous results."
[^df-trigger]: Akidau et al., The Dataflow Model (VLDB 2015), 2.3 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "Triggering determines when in processing time the results of groupings are emitted as"
[^df-wm-trigger]: Akidau et al., The Dataflow Model (VLDB 2015), 2.4 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "The watermark trigger fires when the watermark passes the end of the window in question."
[^df-lambda]: Akidau et al., The Dataflow Model (VLDB 2015), 2.3 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "it does not solve the completeness problem by somehow providing correct answers faster; it simply provides the best low-latency estimate of a result that the streaming pipeline can provide, with the promise of eventual consistency and correctness once the batch pipeline"
[^ks-win-def]: Apache Kafka 3.9.0, Kafka Streams core concepts (Windowing) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "group records that have the same key"
[^ks-win-key]: Apache Kafka 3.9.0, Kafka Streams core concepts (Windowing) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "Windows are tracked per record key."
[^ksdsl-tumb]: Apache Kafka 3.9.0, Kafka Streams DSL (Tumbling time windows) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/developer-guide/dsl-api.html> — "A tumbling window is a hopping window whose window size is equal to its advance interval. Since tumbling windows never overlap, a data record will belong to one and only one window."
[^ksdsl-hop]: Apache Kafka 3.9.0, Kafka Streams DSL (Hopping time windows) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/developer-guide/dsl-api.html> — "A hopping window is defined by two properties"
[^ksdsl-hop2]: Apache Kafka 3.9.0, Kafka Streams DSL (Hopping time windows) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/developer-guide/dsl-api.html> — "you can configure a hopping window with a size 5 minutes and an advance interval of 1 minute"
[^ksdsl-sess]: Apache Kafka 3.9.0, Kafka Streams DSL (Session Windows) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/developer-guide/dsl-api.html> — "Any events processed that fall within the inactivity gap of any existing sessions are merged into the existing sessions."
[^ksdsl-sess2]: Apache Kafka 3.9.0, Kafka Streams DSL (Session Windows) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/developer-guide/dsl-api.html> — "their window sizes sizes vary"
[^ksdsl-sliding]: Apache Kafka 3.9.0, Kafka Streams DSL (Windowing) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/developer-guide/dsl-api.html> — "Fixed-size, overlapping windows that work on differences between record timestamps"
[^fl-tumbling]: Apache Flink 1.20, Windows (Tumbling Windows) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/dev/datastream/operators/windows/> — "A tumbling windows assigner assigns each element to a window of a specified window size."
[^fl-session]: Apache Flink 1.20, Windows (Session Windows) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/dev/datastream/operators/windows/> — "Instead a session window closes when it does not receive elements for a certain period of time, i.e., when a gap of inactivity occurred."
[^fl-wm-def]: Apache Flink 1.20, Event Time and Watermarks — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/concepts/time/> — "A Watermark(t) declares that event time has reached time t in that stream, meaning that there should be no more elements from the stream with a timestamp t' <= t"
[^fl-wm-min]: Apache Flink 1.20, Event Time and Watermarks (Watermarks in Parallel Streams) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/concepts/time/> — "Such an operator's current event time is the minimum of its input streams' event times."
[^fl-late]: Apache Flink 1.20, Event Time and Watermarks (Late Elements) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/concepts/time/> — "Late elements are elements that arrive after the system's event time clock (as signaled by the watermarks) has already passed the time of the late element's timestamp."
[^fl-wm-delay]: Apache Flink 1.20, Event Time and Watermarks (Late Elements) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/concepts/time/> — "even if the lateness can be bounded, delaying the watermarks by too much is often not desirable, because it causes too much delay in the evaluation of event time windows."
[^fl-late-drop]: Apache Flink 1.20, Windows (Allowed Lateness) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/dev/datastream/operators/windows/> — "By default, the allowed lateness is set to 0. That is, elements that arrive behind the watermark will be dropped."
[^fl-lateness]: Apache Flink 1.20, Windows (Allowed Lateness) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/dev/datastream/operators/windows/> — "Elements that arrive after the watermark has passed the end of the window but before it passes the end of the window plus the allowed lateness, are still added to the window."
[^fl-idle]: Apache Flink 1.20, Generating Watermarks (Dealing With Idle Sources) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/dev/datastream/event-time/generating_watermarks/> — "In that case, the watermark will be held back, because it is computed as the minimum over all the different parallel watermarks."
[^fl-idle2]: Apache Flink 1.20, Generating Watermarks (Dealing With Idle Sources) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/dev/datastream/event-time/generating_watermarks/> — "To deal with this, you can use a WatermarkStrategy that will detect idleness and mark an input as idle."
[^fl-kafka-wm]: Apache Flink 1.20, Generating Watermarks (Watermark Strategies and the Kafka Connector) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/dev/datastream/event-time/generating_watermarks/> — "when consuming streams from Kafka, multiple partitions often get consumed in parallel, interleaving the events from the partitions and destroying the per-partition patterns"
[^ks-grace]: Apache Kafka 3.9.0, Kafka Streams core concepts (Windowing) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "This grace period controls how long Kafka Streams will wait for"
[^ks-grace2]: Apache Kafka 3.9.0, Kafka Streams core concepts (Windowing) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "data records for a given window. If a record arrives after the grace period of a window has passed, the record is discarded and will not be processed in that window."
[^ks-tradeoff]: Apache Kafka 3.9.0, Kafka Streams core concepts (Out-of-Order Handling) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "making trade-off decisions between latency, cost, and correctness"
[^fl-win-join]: Apache Flink 1.20, Joining (Window Join) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/dev/datastream/operators/joining/> — "A window join joins the elements of two streams that share a common key and lie in the same window."
[^fl-inner]: Apache Flink 1.20, Joining (Window Join) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/dev/datastream/operators/joining/> — "The creation of pairwise combinations of elements of the two streams behaves like an inner-join, meaning elements from one stream will not be emitted if they don't have a corresponding element from the other stream to be joined with."
[^fl-interval]: Apache Flink 1.20, Joining (Interval Join) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/dev/datastream/operators/joining/> — "The interval join joins elements of two streams (we'll call them A & B for now) with a common key and where elements of stream B have timestamps that lie in a relative time interval to timestamps of elements in stream A."
[^fl-interval-event]: Apache Flink 1.20, Joining (Interval Join) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/dev/datastream/operators/joining/> — "The interval join currently only supports event time."
[^ks-join-ss]: Apache Kafka 3.9.0, Kafka Streams core concepts (Out-of-Order Handling) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "For Stream-Stream joins, all three types (inner, outer, left) handle out-of-order records correctly."
[^ksj-streamtable]: Apache Kafka 3.9.0, Kafka Streams DSL (KStream-KTable Join) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/developer-guide/dsl-api.html> — "against a KTable (changelog stream) upon receiving a new record from the KStream (record stream)."
[^ksj-enrich]: Apache Kafka 3.9.0, Kafka Streams DSL (KStream-KTable Join) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/developer-guide/dsl-api.html> — "An example use case would be to enrich a stream of user activities (KStream) with the latest user profile information (KTable)."
[^ksj-copart]: Apache Kafka 3.9.0, Kafka Streams DSL (Join co-partitioning requirements) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/developer-guide/dsl-api.html> — "For equi-joins, input data must be co-partitioned when joining. This ensures that input records with the same key from both sides of the join, are delivered to the same stream task during processing."
[^ksj-copart-user]: Apache Kafka 3.9.0, Kafka Streams DSL (Join co-partitioning requirements) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/developer-guide/dsl-api.html> — "It is your responsibility to ensure data co-partitioning when joining."
[^ksj-same-parts]: Apache Kafka 3.9.0, Kafka Streams DSL (Join co-partitioning requirements) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/developer-guide/dsl-api.html> — "The input topics of the join (left side and right side) must have the"
[^ks-join-ooo]: Apache Kafka 3.9.0, Kafka Streams core concepts (Out-of-Order Handling) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "For Stream-Table joins, if not using versioned stores, then out-of-order records are not handled (i.e., Streams applications don't check for out-of-order records and just process all records in offset order), and hence it may produce unpredictable results."
[^ks-join-ooo2]: Apache Kafka 3.9.0, Kafka Streams core concepts (Out-of-Order Handling) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "With versioned stores, stream-side out-of-order data will be properly handled by performing a timestamp-based lookup in the table. Table-side out-of-order data is still not handled."
[^ks-versioned]: Apache Kafka 3.9.0, Kafka Streams DSL (versioned state stores) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/developer-guide/dsl-api.html> — "When performing a stream-table join, stream-side records will join with the latest-by-timestamp table record which has a timestamp less than or equal to the stream record's timestamp."
[^ks-changelog]: Apache Kafka 3.9.0, Kafka Streams architecture (Fault Tolerance) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/architecture.html> — "For each state store, it maintains a replicated changelog Kafka topic in which it tracks any state updates."
[^ks-changelog2]: Apache Kafka 3.9.0, Kafka Streams architecture (Fault Tolerance) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/architecture.html> — "is enabled on the changelog topics so that old data can be purged safely to prevent the topics from growing indefinitely."
[^ks-restore]: Apache Kafka 3.9.0, Kafka Streams architecture (Fault Tolerance) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/architecture.html> — "If tasks run on a machine that fails and are restarted on another machine, Kafka Streams guarantees to restore their associated state stores to the content before the failure by replaying the corresponding changelog topics prior to resuming the processing on the newly started tasks."
[^ks-standby]: Apache Kafka 3.9.0, Kafka Streams architecture (Fault Tolerance) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/architecture.html> — "To minimize this restoration time, users can configure their applications to have"
[^ks-standby2]: Apache Kafka 3.9.0, Kafka Streams architecture (Fault Tolerance) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/architecture.html> — "i.e. fully replicated copies of the state"
[^k-hadoop]: Apache Kafka 3.9.0, design (Offline Data Load) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "tasks which fail can restart without danger of duplicate data"
[^rk-warn]: docs.rs rdkafka 0.39.0, crate root — <https://docs.rs/rdkafka/0.39.0/rdkafka/> — "Warning: the library is under active development and the APIs are likely to change."
[^rk-clients]: docs.rs rdkafka 0.39.0, crate root (Client types) — <https://docs.rs/rdkafka/0.39.0/rdkafka/> — "StreamConsumer: a Stream of messages that takes care of polling the consumer automatically."
[^rk-fp-poll]: docs.rs rdkafka 0.39.0, FutureProducer — <https://docs.rs/rdkafka/0.39.0/rdkafka/producer/future_producer/struct.FutureProducer.html> — "This producer has an internal polling thread and as such it doesn't need to be polled."
[^rk-msg]: docs.rs rdkafka 0.39.0, trait Message — <https://docs.rs/rdkafka/0.39.0/rdkafka/message/trait.Message.html> — "Returns the offset of the message within the partition."
[^rk-alo]: docs.rs rdkafka 0.39.0, crate root (At-least-once delivery) — <https://docs.rs/rdkafka/0.39.0/rdkafka/> — "In order to implement at-least-once delivery the stream processing application has to carefully commit the offset only once the message has been processed. Committing the offset too early, instead, might cause message loss, since upon recovery the consumer will start from the next message, skipping the one where the failure occurred."
[^rk-eos]: docs.rs rdkafka 0.39.0, crate root (Exactly-once semantics) — <https://docs.rs/rdkafka/0.39.0/rdkafka/> — "Exactly-once semantics (EOS) can be achieved using transactional producers, which allow produced records and consumer offsets to be committed or aborted atomically. Consumers that set their isolation.level to read_committed will only observe committed messages."
[^rk-iso]: docs.rs rdkafka 0.39.0, module producer (Transactions) — <https://docs.rs/rdkafka/0.39.0/rdkafka/producer/index.html> — "Transactional producers work together with transaction-aware consumers configured with the default isolation.level of read_committed."
[^rk-commit]: docs.rs rdkafka 0.39.0, trait Consumer (commit) — <https://docs.rs/rdkafka/0.39.0/rdkafka/consumer/trait.Consumer.html> — "Notice that when a specific offset is committed, all the previous offsets are considered committed as well. Use this method only if you are processing messages in order."
[^rk-commit-plus1]: docs.rs rdkafka 0.39.0, trait Consumer (commit_message) — <https://docs.rs/rdkafka/0.39.0/rdkafka/consumer/trait.Consumer.html> — "This method is exactly equivalent to invoking Consumer::commit with a TopicPartitionList which copies the topic and partition from the message and adds 1 to the offset of the message."
[^rk-store]: docs.rs rdkafka 0.39.0, trait Consumer (store_offset) — <https://docs.rs/rdkafka/0.39.0/rdkafka/consumer/trait.Consumer.html> — "Stores offset to be used on the next (auto)commit. When using this enable.auto.offset.store should be set to false in the config."
[^rk-commitmode]: docs.rs rdkafka 0.39.0, enum CommitMode — <https://docs.rs/rdkafka/0.39.0/rdkafka/consumer/enum.CommitMode.html> — "Regardless of the CommitMode, the commit APIs enqueue the commit request in a local work queue. A separate worker thread picks up this commit request and forwards it to the Kafka broker over the network."
[^rk-commitmode2]: docs.rs rdkafka 0.39.0, enum CommitMode — <https://docs.rs/rdkafka/0.39.0/rdkafka/consumer/enum.CommitMode.html> — "Note that the commit APIs are not async in the Rust sense due to the lack of a callback-based interface exposed by librdkafka."
[^rk-sc-poll]: docs.rs rdkafka 0.39.0, StreamConsumer — <https://docs.rs/rdkafka/0.39.0/rdkafka/consumer/stream_consumer/struct.StreamConsumer.html> — "You must be sure that you attempt to extract a message from the stream consumer at least every max.poll.interval.ms milliseconds, or librdkafka will assume that the processing thread is wedged and leave the consumer groups."
[^rk-init-tx]: docs.rs rdkafka 0.39.0, trait Producer (init_transactions) — <https://docs.rs/rdkafka/0.39.0/rdkafka/producer/trait.Producer.html> — "This function ensures any transactions initiated by previous producers with the same transactional.id are completed. Any transactions left open by any such previous producers will be aborted."
[^rk-tx-section]: docs.rs rdkafka 0.39.0, module producer (Transactions) — <https://docs.rs/rdkafka/0.39.0/rdkafka/producer/index.html> — "Consumer offsets can be sent as part of the ongoing transaction using send_offsets_to_transaction and will be committed atomically with the other records sent in the transaction."
[^rk-send-offsets]: docs.rs rdkafka 0.39.0, trait Producer (send_offsets_to_transaction) — <https://docs.rs/rdkafka/0.39.0/rdkafka/producer/trait.Producer.html> — "Sends a list of topic partition offsets to the consumer group coordinator for cgm, and marks the offsets as part of the current transaction. These offsets will be considered committed only if the transaction is committed successfully."
[^rk-send-offsets-hint]: docs.rs rdkafka 0.39.0, trait Producer (send_offsets_to_transaction) — <https://docs.rs/rdkafka/0.39.0/rdkafka/producer/trait.Producer.html> — "The consumer must not have automatic commits enabled."
[^rk-send-offsets-next]: docs.rs rdkafka 0.39.0, trait Producer (send_offsets_to_transaction) — <https://docs.rs/rdkafka/0.39.0/rdkafka/producer/trait.Producer.html> — "The offsets should be the next message your application will consume, i.e., one greater than the the last processed message's offset for each partition."
[^rk-tx-nooutside]: docs.rs rdkafka 0.39.0, module producer (Transactions) — <https://docs.rs/rdkafka/0.39.0/rdkafka/producer/index.html> — "Once you have initialized transactions on a producer, you are not permitted to produce messages outside of a transaction."
[^rk-tx-block]: docs.rs rdkafka 0.39.0, trait Producer (commit_transaction) — <https://docs.rs/rdkafka/0.39.0/rdkafka/producer/trait.Producer.html> — "This function may block for the specified timeout."
[^rk-commit-tx]: docs.rs rdkafka 0.39.0, trait Producer (commit_transaction) — <https://docs.rs/rdkafka/0.39.0/rdkafka/producer/trait.Producer.html> — "If any of the outstanding messages fail permanently, the current transaction will enter an abortable error state and this function will return an abortable error."
[^rk-users]: docs.rs rdkafka 0.39.0, crate root (Users) — <https://docs.rs/rdkafka/0.39.0/rdkafka/> — "timely-dataflow: a distributed data-parallel compute engine."
[^tk-mpsc]: docs.rs tokio 1.53.2, module sync::mpsc — <https://docs.rs/tokio/1.53.2/tokio/sync/mpsc/index.html> — "A multi-producer, single-consumer queue for sending values between asynchronous tasks."
[^tk-bounded]: docs.rs tokio 1.53.2, module sync::mpsc — <https://docs.rs/tokio/1.53.2/tokio/sync/mpsc/index.html> — "The bounded variant has a limit on the number of messages that the channel can store, and if this limit is reached, trying to send another message will wait until a message is received from the channel."
[^toc-ddia11]: Designing Data-Intensive Applications (O'Reilly, ấn bản 1), mục lục do ETH Library lưu, chương 11 — <https://toc.library.ethz.ch/objects/pdf03/z01_978-1-4493-7332-0_01.pdf> — "11. Stream Processing 439 Transmitting Event Streams Messaging Systems 440 441 Partitioned Logs 446 Databases and Streams 451 Keeping Systems in Sync 452 Change Data Capture 454 Event Sourcing 457 State, Streams, and Immutability 459 Processing Streams 464"
[^toc-ddia5]: Designing Data-Intensive Applications (O'Reilly, ấn bản 1), mục lục do ETH Library lưu, chương 5 — <https://toc.library.ethz.ch/objects/pdf03/z01_978-1-4493-7332-0_01.pdf> — "Part II. 5. Distributed Data Replication 151 Leaders and Followers"
[^toc-ddia10]: Designing Data-Intensive Applications (O'Reilly, ấn bản 1), mục lục do ETH Library lưu, chương 10 — <https://toc.library.ethz.ch/objects/pdf03/z01_978-1-4493-7332-0_01.pdf> — "10. Batch Processing 389 Batch Processing with Unix Tools"
[^toc-ddia12]: Designing Data-Intensive Applications (O'Reilly, ấn bản 1), mục lục do ETH Library lưu, chương 12 — <https://toc.library.ethz.ch/objects/pdf03/z01_978-1-4493-7332-0_01.pdf> — "12. The Future of Data Systems Data Integration Combining Specialized Tools by Deriving Data Batch and Stream Processing Unbundling Databases"
[^k-acks-all-isr]: Apache Kafka 3.9.0, design (Availability and Durability Guarantees) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "By default, when acks=all, acknowledgement happens as soon as all the current in-sync replicas have received the message."
[^k-min-isr]: Apache Kafka 3.9.0, design (Replicated Logs) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "If a less stringent acknowledgement is requested by the producer, then the message can be committed, and consumed, even if the number of in-sync replicas is lower than the minimum (e.g. it can be as low as just the leader)."
[^k-cons-alo]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "This corresponds to the "at-least-once" semantics in the case of consumer failure."
[^df-windowing]: Akidau et al., The Dataflow Model (VLDB 2015), 1.2 Windowing — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "windowing is required for some operations (to delineate finite boundaries in most forms of grouping: aggregation, outer joins, time-bounded operations, etc.)"
[^df-window-time]: Akidau et al., The Dataflow Model (VLDB 2015), 1.2 Windowing — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "Windowing is effectively always time based; while many systems support tuple-based windowing, this is essentially time-based windowing over a logical time domain"
[^ks-table-snapshot]: Apache Kafka 3.9.0, Kafka Streams core concepts (Duality of Streams and Tables) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "A table can be considered a snapshot, at a point in time, of the latest value for each key in a stream"
[^rmq-streams]: RabbitMQ docs, Streams and Superstreams (Partitioned Streams) — <https://www.rabbitmq.com/docs/streams> — "Streams model an append-only log of messages that can be repeatedly read until they expire."
[^rk-sys-ver]: rust-rdkafka tag v0.39.0, rdkafka-sys/Cargo.toml — <https://raw.githubusercontent.com/fede1024/rust-rdkafka/v0.39.0/rdkafka-sys/Cargo.toml> — "name = "rdkafka-sys" version = "4.10.0+2.12.1""
[^rk-sys-readme]: rust-rdkafka tag v0.39.0, rdkafka-sys/README.md — <https://raw.githubusercontent.com/fede1024/rust-rdkafka/v0.39.0/rdkafka-sys/README.md> — "`RX.RY.RZ` is the version of the bundled librdkafka."
[^lib-iso]: librdkafka tag v2.12.1, CONFIGURATION.md (isolation.level) — <https://raw.githubusercontent.com/confluentinc/librdkafka/v2.12.1/CONFIGURATION.md> — "Controls how to read messages written transactionally: `read_committed` - only return transactional messages which have been committed. `read_uncommitted` - return all messages, even transactional messages which have been aborted."
[^ksdsl-sess-keys]: Apache Kafka 3.9.0, Kafka Streams DSL (Session Windows) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/developer-guide/dsl-api.html> — "all windows are tracked independently across keys"
[^ksj-copart-exempt]: Apache Kafka 3.9.0, Kafka Streams DSL (Join co-partitioning requirements) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/developer-guide/dsl-api.html> — "Co-partitioning is not required when performing"
[^pg-slot-wrap]: PostgreSQL 16, 49.2.2 Replication Slots — <https://www.postgresql.org/docs/16/logicaldecoding-explanation.html> — "In extreme cases this could cause the database to shut down to prevent transaction ID wraparound (see Section 25.1.5). So if a slot is no longer required it should be dropped."
