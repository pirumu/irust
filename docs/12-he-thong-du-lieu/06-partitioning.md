# P12 · Chương 06 — Partitioning (sharding)

> Chương này mượn *khung chủ đề* từ sách *Designing Data-Intensive Applications* (DDIA, ấn bản 1, chương 6 "Partitioning"[^ddia-toc-ch]) nhưng **không trích sách**. Mọi khẳng định dựa trên paper gốc (Dynamo, Bigtable, Karger và cộng sự 1997, Jump Consistent Hash) và tài liệu có phiên bản của hệ thống thật (Cassandra 4.1/5.0, Elasticsearch 8.15, MongoDB 7.0, Kafka 3.7.0, PostgreSQL 16, Redis Cluster spec, DynamoDB, Bigtable). Chỗ nào ghi "(nhận xét của người soạn)" hoặc "(suy luận của người soạn)" thì đó không phải fact từ nguồn. Phần Rust theo **Rust 1.95.0** và các crate có ghi số phiên bản. Chương không chạy thí nghiệm nào; code chỉ để minh họa, không kèm khẳng định về output.

## Bạn sẽ hiểu được gì

- **Q1.** Partition theo *khoảng khóa* (key range) khác partition theo *băm* (hash) thế nào, và mỗi cách được/mất gì? → mục 1
- **Q2.** Skew và hot spot là gì; vì sao băm đều khóa vẫn có thể có node nóng, và có những cách giảm nhẹ nào? → mục 2
- **Q3.** Vì sao `hash(key) mod N` hỏng khi đổi N, consistent hashing sửa việc đó ra sao, và virtual node (vnode) để làm gì? → mục 3
- **Q4.** Secondary index khi dữ liệu bị chia: index *local* (theo document) khác index *global* (theo term) thế nào? → mục 4
- **Q5.** Rebalancing có mấy chiến lược (số partition cố định, partition động, tỷ lệ theo số node) và hệ thống thật chọn gì? → mục 5
- **Q6.** Client làm sao biết gửi request tới node nào (request routing): coordination service hay gossip, và khi bản đồ cũ thì chuyện gì xảy ra? → mục 6
- **Q7.** Chia dữ liệu bắt bạn trả những giá nào ở truy vấn xuyên partition, ràng buộc unique, thứ tự và giao dịch? → mục 7
- **Q8.** (Rust) Vì sao `std::hash::Hash`/`DefaultHasher` không dùng được để chọn shard, và nên dùng gì? → mục 8

## Kiến thức cần trước

- [Chương 01 — Reliability, scalability, maintainability](01-reliability-scalability-maintainability.md): *scalability* và tham số tải; chương này là cách *scale-out* dữ liệu.
- [Chương 02 — Mô hình dữ liệu](02-mo-hinh-du-lieu.md) và [Chương 03 — Storage engine](03-storage-engine.md): SSTable/LSM và B-tree, vì mỗi partition bên trong vẫn là một storage engine như thế.
- [Chương 05 — Replication](05-replication.md): partition và replication thường đi cùng nhau; chương này chỉ nói *chia*, còn mỗi partition được sao chép thế nào là chuyện của chương 05.
- [P10 · Chương 09 — Collection của std](../10-rust-internals/09-collections-internals.md): `HashMap`, SipHash, `RandomState` (mục 5) và `BTreeMap::range` (mục 6); mục 8 dùng lại.

Các chương sau dùng chương này: [Chương 07 — Transaction](07-transaction.md) (giao dịch xuyên partition), [Chương 08 — Sự cố hệ phân tán](08-su-co-he-phan-tan.md) (phát hiện node chết), [Chương 09 — Consistency và consensus](09-consistency-va-consensus.md) (bản đồ partition cần đồng thuận), [Chương 11 — Stream processing](11-stream-processing.md) (partition của Kafka). Khi nào *không* nên chia dữ liệu: [Chương 13 — Kiến trúc hệ dữ liệu](13-kien-truc-he-du-lieu.md).

## 0. Từ vựng: partition, shard, tablet, vnode… cùng là một ý

Cả chương bám theo một tình huống: `orders`, kho đơn hàng của một sàn thương mại điện tử có nhiều người bán. Mỗi đơn có `order_id`, `customer_id`, `seller_id`, `created_at` và email người mua. Service `orders-api` viết bằng Rust nhận request rồi đọc ghi vào kho này. Ta giả định kho đã to đến mức một node không chứa nổi hay không phục vụ nổi, nên phải chia ra nhiều node. Bốn truy vấn của `orders-api` sẽ quay lại ở mọi mục: lấy một đơn theo `order_id`; liệt kê đơn của một khách theo thời gian; báo cáo mọi đơn trong một khoảng ngày; tìm đơn theo email. Mỗi quyết định chia dữ liệu làm vài truy vấn rẻ đi và vài truy vấn đắt lên, và chương này đi qua từng quyết định như vậy. (Nhận xét của người soạn: `orders` là ví dụ giả định, không phải code trong repo.)

Trước hết là chuyện tên gọi. Mỗi hệ thống đặt một tên riêng cho "một mảnh dữ liệu", nhưng ý thì giống nhau:

| Hệ thống | Gọi là | Mảnh đó là gì |
|---|---|---|
| Bigtable | *tablet* | một khoảng hàng (row range), vừa là đơn vị phân phối vừa là đơn vị cân tải[^bt-tablet] |
| MongoDB | *chunk* | một khoảng dữ liệu đã shard, tức một khoảng giá trị shard key[^mongo-chunk] |
| Cassandra | *partition* | tập các hàng có cùng giá trị partition key[^cass-partition-def] |
| Elasticsearch | *shard* | mỗi index chia thành một hoặc nhiều shard, mỗi shard có thể sao chép ra nhiều node[^es-shard-hit] |
| Kafka | *partition* của topic | mỗi event mới được ghi vào một partition của topic[^kafka-partitioned] |
| Redis Cluster | *hash slot* | mỗi master giữ một tập con của 16384 slot[^redis-slots] |

Chương này dùng "partition" cho khái niệm chung, và dùng "shard" khi nói về hệ cụ thể gọi như vậy. Mỗi mảnh nằm trên một node (hoặc một nhóm replica). Mục tiêu là *scale-out*: mỗi node chỉ giữ và phục vụ một phần dữ liệu (định nghĩa làm việc của người soạn).

Hai chỗ dễ nhầm:

- **Partition của PostgreSQL ≠ sharding.** *Declarative partitioning* của PostgreSQL 16 chia một bảng thành các bảng con *trong cùng một server*. Bảng cha là bảng "ảo" không có storage riêng, còn dữ liệu nằm ở các partition là bảng thường[^pg-virtual]. Lợi ích chính là *partition pruning*: planner chứng minh được partition nào không thể chứa dòng thỏa `WHERE` rồi bỏ qua nó[^pg-prune]. Lợi ích thứ hai là xóa dữ liệu cũ theo từng partition. Không có chuyện scale-out qua nhiều máy ở đây. Cách chia (range/list/hash) thì tương tự khung dưới đây (nhận xét của người soạn).
- **Partition mạng ≠ partition dữ liệu.** "Network partition" (đứt mạng giữa các node) là chuyện của [Chương 08](08-su-co-he-phan-tan.md). Chương này chỉ nói chia *dữ liệu*.

## 1. Chia theo khoảng khóa hay theo băm (Q1)

> **Trả lời ngắn.** *Key range*: sắp khóa theo thứ tự rồi cắt thành các khoảng liên tiếp, mỗi khoảng một partition. Quét theo dải thì rẻ, nhưng khóa tăng dần dễ dồn hết ghi vào một partition. *Hash*: băm khóa rồi chia theo giá trị băm. Tải rải đều, nhưng các khóa kề nhau bị tách ra, nên truy vấn dải phải hỏi mọi partition. Hệ thống thật dùng cả hai, và có cách lai (khóa ghép) để lấy một phần lợi ích của cả hai.

**Giải thích.**

*Với `orders`.* Việc đầu tiên khi chia `orders` là chọn cột để cắt. Hai ứng viên tự nhiên là `created_at`, cắt thành các khoảng ngày liên tiếp, và `order_id`, băm rồi chia. Hai cách này làm truy vấn "báo cáo đơn tuần trước" và "lấy đơn 42" rẻ hay đắt rất khác nhau, nên ta xét từng cách.

### 1.1 Chia theo khoảng khóa

Nếu cắt `orders` theo `created_at`, mỗi partition giữ một khoảng ngày liền nhau. Bigtable thiết kế đúng như vậy: dữ liệu xếp theo thứ tự từ điển (lexicographic) của row key, và *tablet* là một khoảng hàng liên tiếp[^bt-tablet]. Vì thế đọc một dải hàng ngắn thường chỉ cần liên lạc vài máy, và client tận dụng điều đó bằng cách chọn row key sao cho dữ liệu hay truy cập cùng nhau nằm kề nhau[^bt-short]. Paper lấy ví dụ đảo các thành phần của hostname trong URL, để các trang cùng domain nằm liền nhau[^bt-reverse]. MongoDB ranged sharding nói cùng ý: các document có shard key gần nhau nhiều khả năng ở cùng chunk, cùng shard, nên truy vấn đọc dải liên tục hiệu quả[^mongo-range]. Với `orders`, báo cáo "đơn tuần trước" chỉ chạm một hai partition.

Cái giá hiện ra ngay ở đường ghi. Đơn mới luôn có `created_at` lớn nhất, nên luôn rơi vào partition cuối. MongoDB mô tả đúng hiện tượng này: mọi cụm range-sharded có một chunk chứa cận trên `MaxKey`, và với shard key tăng đơn điệu như ObjectId hay timestamp, chunk đó nhận hầu hết lượng insert. Ghi bị bó vào một shard và mất lợi thế ghi phân tán[^mongo-mono-hot]. Vì vậy MongoDB khuyên dùng hashed sharding cho khóa đơn điệu[^mongo-hash-mono]. Bigtable cảnh báo tương tự: row key bắt đầu bằng timestamp đẩy ghi tuần tự lên một node, tạo hotspot[^bt-ts-hot].

### 1.2 Chia theo băm

Cách còn lại là băm `order_id` rồi chia theo giá trị băm. Cassandra làm vậy: nó tính hash từ các cột partition key, và giá trị hash quyết định vị trí partition[^cass-hash]. Producer Java của Kafka cũng thế với bản ghi có khóa: mã nguồn `partitionForKey` là `murmur2(key)`, lấy dương rồi `% numPartitions`[^kafka-bip]. Đơn mới giờ rải đều qua mọi partition, hết dồn một chỗ.

Đổi lại, các khóa kề nhau bị xáo ra khắp nơi. MongoDB nói thẳng cả hai mặt: hashed sharding cho phân bố đều hơn trên cụm, nhưng giảm số truy vấn *targeted* (chỉ tới một shard) và tăng số truy vấn *broadcast* (hỏi mọi shard)[^mongo-hash]. Với `orders`, báo cáo "đơn tuần trước" bây giờ phải hỏi mọi partition.

### 1.3 Khóa ghép: băm trước, sắp sau

Hai cách trên không loại trừ nhau. Cassandra tách khóa chính thành *partition key*, được băm để chọn nơi lưu, và *clustering columns*, xếp thứ tự các hàng *bên trong* partition[^cass-clustering]. Mọi hàng cùng partition key nằm trên cùng tập replica[^cass-colocate]. Bigtable cũng khuyên: nếu đặt timestamp vào row key thì đặt trước nó một giá trị có độ phân biệt cao như user ID[^bt-ts-prefix]. Mẫu chung (nhận xét của người soạn) là băm ở thành phần đầu để rải tải, giữ thứ tự ở thành phần sau để quét dải trong một partition.

Với `orders`, khóa `(customer_id, created_at)` làm truy vấn "đơn của khách C theo thời gian" nằm gọn trong một partition, còn đơn của những khách khác nhau vẫn rải khắp cụm. Báo cáo theo ngày cho toàn sàn thì vẫn phải hỏi mọi partition (nhận xét của người soạn).

```mermaid
flowchart LR
  subgraph R["Key range: khoảng liên tiếp"]
    direction TB
    r1["tablet 1: a … f"]
    r2["tablet 2: f … m"]
    r3["tablet 3: m … z"]
  end
  subgraph H["Hash: khóa bị xáo trộn"]
    direction TB
    h1["partition 0: khóa rải rác"]
    h2["partition 1: khóa rải rác"]
    h3["partition 2: khóa rải rác"]
  end
  Q1["quét dải f..k"] --> r2
  Q2["quét dải f..k"] -.->|"phải hỏi tất cả"| h1
  Q2 -.-> h2
  Q2 -.-> h3
```

(Sơ đồ minh họa của người soạn; không phải số liệu từ nguồn.)

<details><summary>Đi sâu: tên khóa, điều kiện unique, và những chi tiết PostgreSQL/MongoDB hay quên</summary>

- PostgreSQL 16 định nghĩa range partitioning là các khoảng của cột khóa, không chồng lấn, cận dưới bao gồm và cận trên loại trừ[^pg-range]. Hash partitioning thì chỉ định một modulus và một remainder cho từng partition[^pg-hash]. Declarative chỉ hỗ trợ ba kiểu (range, list, hash); kiểu khác phải dùng inheritance hoặc UNION ALL view và mất một số lợi ích hiệu năng[^pg-other].
- Với chia theo thứ tự, *thứ tự phải ổn định giữa các máy*. `pg_dump --load-via-partition-root` có thể hữu ích khi khôi phục sang server mà dòng không luôn rơi vào cùng partition như ở server gốc; ví dụ trong docs là cột partition kiểu text và hai hệ có định nghĩa collation khác nhau[^pg-collation]. (Nhận xét của người soạn: bài học chung là hàm xác định partition, thứ tự hay hash, là một phần của *định dạng dữ liệu*; đổi nó là migration. Mục 8 nói điều này cho hash.)
- MongoDB: nếu shard collection đã có dữ liệu thì ban đầu chỉ có một chunk, balancer tách và di chuyển sau đó[^mongo-initial-chunk]. Collection rỗng với hashed key thì mặc định tạo 2 chunk mỗi shard, đổi được bằng `numInitialChunks`[^mongo-init-hashed].
- Hashed index của MongoDB cắt số thực về số nguyên 64-bit trước khi băm, nên 2.3, 2.2, 2.9 cho cùng giá trị băm. Docs khuyên không dùng hashed index cho số thực không chuyển về nguyên được[^mongo-hash-float].

</details>

**Quay lại Q1:** key range giữ thứ tự (báo cáo theo ngày của `orders` rẻ, nhưng đơn mới dồn vào partition cuối); hash rải đều (đơn mới phân tán, nhưng báo cáo phải hỏi mọi partition); khóa ghép "băm trước, sắp sau" là thỏa hiệp phổ biến.

## 2. Skew và hot spot (Q2)

> **Trả lời ngắn.** *Skew* là dữ liệu hoặc tải phân bố lệch giữa các partition; partition nhận nhiều hơn phần của mình là *hot spot*. Băm chỉ rải đều các khóa *khác nhau*. Nếu một khóa nhận quá nhiều request (người nổi tiếng, một tenant khổng lồ) thì partition chứa nó vẫn nóng. Cách giảm nhẹ nằm ở thiết kế khóa, và cách nào cũng có giá riêng.

**Giải thích.**

*Với `orders`.* Giả sử `orders` băm theo `seller_id` để mọi đơn của một người bán nằm chung chỗ. Đến ngày sale, một seller lớn nhận cả triệu đơn trong khi hàng chục nghìn seller nhỏ gần như im lặng. Hash không cứu được: mọi đơn của seller đó có cùng `seller_id`, nên cùng giá trị băm, nên cùng partition. Partition ấy bị quá tải dù cả cụm còn rảnh (giả định minh họa của người soạn).

### 2.1 Hai loại lệch

Loại thứ nhất là lệch do *thiết kế khóa*: khóa tăng đơn điệu (mục 1), hoặc khóa có ít giá trị phân biệt. MongoDB gọi hai yếu tố này là *cardinality* (số giá trị khác nhau) và *frequency* (mức độ lặp của từng giá trị). Cardinality thấp giới hạn số chunk tối đa và làm giảm hiệu quả scale-out[^mongo-card]. Nếu phần lớn document chỉ mang vài giá trị khóa thì các chunk chứa chúng thành nút cổ chai, và có thể thành chunk không chia được nữa[^mongo-freq]. DynamoDB đưa ví dụ cụ thể: User ID khi có nhiều user là khóa tốt, còn status code chỉ có vài giá trị hay ngày tạo làm tròn theo giờ là khóa xấu[^ddb-uniform]. Theo cách đánh giá đó, `customer_id` là khóa tốt còn `status` của đơn là khóa xấu (suy luận của người soạn).

Loại thứ hai là lệch do *tải trên một khóa*: phân phối khóa có đều thì tải truy cập vẫn có thể không đều. Dynamo thừa nhận giả định của họ là dù truy cập lệch nhiều, vẫn có đủ khóa ở đầu "nổi tiếng" để tải rải đều qua partitioning[^dyn-skew-assume]. Một khóa quá nóng, như seller lớn ở trên, phá đúng giả định đó.

### 2.2 Nhận biết

Paper Dynamo coi một node là "in balance" nếu tải request của nó lệch khỏi trung bình dưới một ngưỡng (15%), còn lại là "out-of-balance"[^dyn-imbalance]. Tỷ lệ node mất cân bằng giảm khi tải hệ thống tăng, vì tải cao thì nhiều khóa phổ biến được truy cập nên tải rải đều hơn (lý giải của paper)[^dyn-imbalance2]. Trong vận hành, hãy theo dõi tải *theo từng partition*, không chỉ trung bình toàn cụm. Chỉ số trung bình che mất một partition nóng, vốn là loại sự cố "cả cụm rảnh mà một node 100% CPU" (nhận xét của người soạn).

### 2.3 Giảm nhẹ khi một khóa quá nóng

- **Khóa phụ có hậu tố (write sharding).** DynamoDB thêm một số ngẫu nhiên vào cuối partition key để rải ghi qua không gian khóa lớn hơn[^ddb-suffix]. Với `orders`, đơn của seller lớn sẽ ghi vào `seller_id#0` đến `seller_id#9` (nhận xét của người soạn). Cái giá là muốn đọc tất cả mục của một ngày thì phải truy vấn mọi hậu tố rồi gộp kết quả ở ứng dụng[^ddb-suffix-read]. Biến thể "hậu tố tính được", tức hash của một thuộc tính hay tìm, cho phép đọc một mục cụ thể mà vẫn rải được ghi[^ddb-calc-suffix].
- **Chia khóa nóng sang một tập shard nhỏ.** Với `index.routing_partition_size` của Elasticsearch, giá trị routing chỉ chọn một *tập* shard, rồi `_id` chọn shard trong tập đó. Tăng kích thước tập thì dữ liệu đều hơn, nhưng mỗi request phải tìm trên nhiều shard hơn[^es-routing-partition].
- **Đổi khóa chính để thêm độ phân biệt.** MongoDB gợi ý dùng compound key kết hợp giá trị duy nhất hoặc tần suất thấp khi khóa có giá trị tần suất cao[^mongo-freq-fix]. Bigtable gợi ý đặt định danh có độ phân biệt cao (user ID) trước timestamp[^bt-ts-prefix].

Không công cụ nào trong số này *tự động* biết khóa nào nóng, nên ứng dụng phải chọn khóa nào cần chia nhỏ (nhận xét của người soạn: các tài liệu trên mô tả cơ chế chứ không hứa tự phát hiện).

<details><summary>Đi sâu: partition quá lớn cũng là skew, và Cassandra "hotspot đọc/ghi"</summary>

- Cassandra nêu rõ mặt trái của việc "mọi hàng cùng partition key nằm trên cùng node": nó tạo hotspot cho cả đọc lẫn ghi[^cass-hotspot]. Partition phải "vừa phải, không quá lớn không quá nhỏ". Thiết kế khóa luôn là cân bằng giữa *đồng-nơi*, tức ít node phải hỏi và batch nguyên tử trong partition[^cass-atomic], với *phân tán*, tức tải đều (nhận xét của người soạn).
- Bigtable lưu ý hotspot ở một bảng ảnh hưởng độ trễ của các bảng khác trong cùng instance, vì mọi bảng trong một instance được lưu trên cùng các tablet[^bt-hotspot-shared]. (Nguồn là tài liệu vendor không đánh số phiên bản.)
- Dynamo đánh giá hiệu quả cân tải bằng "mean load / max load", tức tải trung bình chia tải của node nóng nhất[^dyn-efficiency]. Đây là thước đo đáng đưa lên dashboard của chính bạn (nhận xét của người soạn).

</details>

**Quay lại Q2:** hot spot đến từ khóa tăng đơn điệu, khóa ít giá trị phân biệt, hoặc một khóa quá nóng như seller lớn của `orders`; hash không cứu được trường hợp cuối, nên phải đổi khóa hoặc chia khóa nóng bằng hậu tố/tập shard và chấp nhận chi phí đọc nhiều nơi.

## 3. Consistent hashing và virtual node (Q3)

> **Trả lời ngắn.** Với `hash(key) mod N`, đổi N có thể làm gần như mọi ánh xạ khóa → node mất hiệu lực[^cass-naive]. Consistent hashing đặt cả node và khóa lên một vòng băm; khóa thuộc về node đầu tiên gặp khi đi theo chiều kim đồng hồ, nên thêm hay bớt một node chỉ ảnh hưởng hàng xóm kề nó[^dyn-ring-neighbors]. Mỗi node giữ nhiều điểm trên vòng (virtual node) để cân tải tốt hơn và tận dụng máy mạnh yếu khác nhau[^dyn-vnodes].

**Giải thích.**

*Với `orders`.* Giả sử cụm đang có 10 node và `orders-api` chọn node bằng `hash(order_id) mod 10`. Khi đĩa đầy bạn thêm hai node, công thức thành `mod 12`. Câu hỏi là bao nhiêu đơn hàng phải chuyển sang node khác sau thay đổi đó, và liệu có cách nào để chỉ chuyển phần tối thiểu.

### 3.1 Vì sao `mod N` hỏng

Câu trả lời của `mod N` là gần hết. Cassandra docs nói gọn: với cách chia modulo, thêm một node có thể làm hỏng gần như mọi ánh xạ[^cass-naive]. Paper Jump Consistent Hash lấy đúng ví dụ 10 shard lên 12. `h(key) mod 12` thường khác `h(key) mod 10`, nên dữ liệu phải sắp xếp lại gần hết, trong khi chỉ cần chuyển 1/6 dữ liệu là đủ cân bằng lại[^jump-1-6]. Số 5/6 ở đây là phép tính của người soạn: với `mod 10` rồi `mod 12`, một khóa chỉ không đổi chỗ khi hai số dư bằng nhau; trong mỗi chu kỳ 60 giá trị băm chỉ 10 giá trị thỏa, tức khoảng 5/6 khóa đổi chỗ so với mức tối thiểu 1/6.

### 3.2 Vòng băm

Consistent hashing sinh ra để tránh chuyện đó. Paper Karger và cộng sự (1997) định nghĩa hàm băm "consistent" là hàm thay đổi tối thiểu khi miền giá trị của nó thay đổi[^kar-def]. Bài toán gốc là cache web, nơi các client có thể có những "góc nhìn" (view) không nhất quán về việc máy nào đang sẵn sàng[^kar-views]. Dùng cho chia dữ liệu là ứng dụng đến sau.

Dynamo biến ý tưởng đó thành vòng băm. Miền giá trị của hàm băm được coi là một không gian tròn cố định, và mỗi node nhận một giá trị ngẫu nhiên làm "vị trí" trên vòng. Muốn tìm node cho một khóa, ta băm khóa ra một vị trí rồi đi theo chiều kim đồng hồ đến node đầu tiên có vị trí lớn hơn[^dyn-ring]. Mỗi node chịu trách nhiệm vùng giữa nó và node đứng trước, nên node đến hay đi chỉ ảnh hưởng hàng xóm gần nhất[^dyn-ring-neighbors]. Cassandra mô tả cùng cơ chế. Nó băm khóa ra token rồi đi theo chiều kim đồng hồ cho tới khi gặp đủ số node khác nhau bằng replication factor (RF), và đó là các replica của khóa[^cass-rf-walk].

```mermaid
flowchart LR
  A(("A<br/>vị trí 10")) --> B(("B<br/>vị trí 90"))
  B --> C(("C<br/>vị trí 170"))
  C --> D(("D<br/>vị trí 250"))
  D --> A
  K["khóa k<br/>hash = 120"] -. "đi theo chiều kim đồng hồ" .-> C
```

(Con số vị trí chỉ để minh họa; khóa có hash 120 rơi vào khoảng (90, 170] nên thuộc node C, theo quy tắc "node đầu tiên có vị trí lớn hơn".)

Quay lại `orders`: thêm node E vào vị trí 130 thì chỉ những đơn có hash trong (90, 130] chuyển từ C sang E. Đơn của A, B, D không động đến (suy luận của người soạn từ quy tắc ở trên).

### 3.3 Virtual node

Vòng đơn giản có hai khuyết điểm. Gán vị trí ngẫu nhiên khiến phân bố dữ liệu và tải không đều, và thuật toán cơ bản không biết gì về việc các node có hiệu năng khác nhau[^dyn-challenges]. Cách khắc phục là cho mỗi node *nhiều* điểm trên vòng, gọi là virtual node (vnode)[^dyn-vnodes]. Paper nêu ba lợi ích: node ngừng hoạt động thì tải của nó rải đều lên các node còn lại; node mới hay node trở lại nhận lượng tải xấp xỉ bằng nhau từ mọi node khác; và có thể chọn số vnode theo năng lực từng máy để tính đến phần cứng không đồng nhất[^dyn-vnode-adv]. Ngay paper Karger gốc cũng đã cần mỗi bucket có nhiều hơn một điểm trên đoạn đơn vị[^kar-multipoint]. Trong Cassandra, ý này nằm ở `num_tokens`: node có nhiều token hơn thì chứa tỷ lệ dữ liệu lớn hơn, và nên cho các node cùng phần cứng cùng số token[^cass-num-tokens].

Vnode có giá. Cassandra liệt kê rằng mỗi token thêm tối đa `2 × (RF − 1)` hàng xóm trên vòng, nên có nhiều tổ hợp lỗi node hơn làm mất khả dụng một phần vòng, và càng nhiều token thì xác suất sự cố càng cao[^cass-vnode-cost]. Các thao tác bảo trì toàn cụm như repair cũng chậm hơn khi số token mỗi node tăng[^cass-vnode-repair]. Ở Cassandra 2.x chỉ có thuật toán chọn token ngẫu nhiên nên mặc định phải 256 token để cân bằng; từ 3.x có bộ cấp phát xác định cho phép dùng ít token hơn nhiều[^cass-256].

<details><summary>Đi sâu: tính chất của Karger, Jump Consistent Hash, và vì sao Dynamo bỏ "mỗi node T token ngẫu nhiên"</summary>

- **Hai tính chất của Karger.** *Smoothness*: khi thêm hay bớt một máy, tỷ lệ kỳ vọng các đối tượng phải chuyển sang cache mới là mức tối thiểu cần để giữ tải cân bằng[^kar-smooth]. *Monotonicity*: nếu thêm bucket, một đối tượng chỉ có thể chuyển từ bucket cũ sang bucket *mới*, không chuyển giữa hai bucket cũ[^kar-mono].
- **Jump Consistent Hash** (Lamping & Veach, Google) không cần lưu trữ, nhanh hơn thuật toán Karger và chia đều hơn. Hạn chế chính là bucket phải đánh số tuần tự, nên hợp với lưu trữ dữ liệu hơn là web cache phân tán[^jump-abstract]. Hệ quả (suy luận của người soạn): không biểu diễn được việc bỏ bucket ở *giữa* dải số, nên chỉ hợp khi thêm hay bớt ở cuối (số shard tăng dần), không hợp khi node chết bất kỳ. Crate Rust `jump-consistent-hash` 0.1.0 cài thuật toán này: hàm `hash` nhận khóa 64 bit và số bucket, trả số bucket trong `0..buckets`[^jump-crate].
- **Dynamo đổi chiến lược ba lần.** Chiến lược 1: mỗi node T token ngẫu nhiên, phân vùng theo giá trị token, và kích thước bộ nhớ lưu membership tăng tuyến tính theo số node[^dyn-s1]. Chiến lược 2: T token ngẫu nhiên cộng Q phân vùng bằng nhau, tức tách phân vùng khỏi vị trí đặt. Nhưng khi node mới "đánh cắp" khoảng khóa, node cũ phải quét kho dữ liệu cục bộ (tốn tài nguyên, chạy ưu tiên thấp), có lúc mùa mua sắm bootstrap mất gần một ngày, Merkle tree phải tính lại, và không có cách dễ để snapshot toàn bộ không gian khóa[^dyn-s2]. Chiến lược 3: Q/S token mỗi node (S là số node) với phân vùng bằng nhau. So với chiến lược 1, hiệu quả cân tải tốt hơn và kích thước membership giảm ba bậc độ lớn[^dyn-s3-better]. Vì ranh giới cố định nên mỗi partition có thể là một file riêng và chuyển như một đơn vị bằng cách chép file[^dyn-s3-files]. Nhược điểm là đổi membership đòi hỏi phối hợp để giữ các tính chất của phép gán[^dyn-s3-cost]. Đây chính là chiến lược "số partition cố định" của mục 5.
- **Elasticsearch** ghi rõ cách chọn shard: `shard_num = (hash(_routing) % num_routing_shards) / routing_factor`, với `routing_factor = num_routing_shards / num_primary_shards`, và routing mặc định là `_id`[^es-formula]. Docs mô tả không gian `num_routing_shards` này là không gian băm dùng consistent hashing để chia document[^es-split]. Nhìn công thức thì cơ chế là `mod` trên không gian cố định rồi chia gộp theo `routing_factor` (suy luận của người soạn); mục 5 nói cách nó cho phép *split*.

</details>

**Quay lại Q3:** `mod N` làm gần hết khóa phải đổi chỗ khi N đổi (ví dụ 10 → 12 ở trên); consistent hashing chỉ di chuyển phần tối thiểu quanh node vào/ra; vnode rải phần di chuyển đó ra nhiều node và cho phép trọng số theo năng lực, với giá là nhiều hàng xóm hơn.

## 4. Secondary index: local và global (Q4)

> **Trả lời ngắn.** Khi dữ liệu đã chia theo khóa chính, tìm theo cột khác (secondary index) có hai cách. *Local index* (*document-partitioned*): mỗi partition giữ index chỉ cho dữ liệu *của chính nó*. Ghi rẻ vì chỉ chạm một partition, nhưng đọc theo cột phụ *không kèm khóa partition* phải hỏi *mọi* partition rồi gộp. *Global index* (*term-partitioned*): index riêng được chia theo *giá trị được index*. Đọc theo cột phụ trúng ít partition, nhưng một lần ghi có thể phải cập nhật index nằm ở partition khác, thường bất đồng bộ nên index có thể cũ. (Phần tổng quát hóa này là của người soạn; bằng chứng từng hệ nằm ở dưới.)

(Thuật ngữ *document-partitioned* và *term-partitioned* là cách gọi quen trong khung DDIA: mục lục sách có hai mục "Partitioning Secondary Indexes by Document" và "by Term"[^ddia-toc-index]. Các tài liệu vendor dưới đây dùng tên riêng; nhận xét của người soạn.)

**Giải thích.**

*Với `orders`.* Giả sử `orders` chia theo `order_id` (hoặc `customer_id`). Truy vấn "lấy đơn 42" và "đơn của khách C" đi thẳng tới một partition, nhưng truy vấn thứ tư, "tìm đơn theo email", thì không: email nằm trong document của bất kỳ partition nào. Muốn tìm nhanh phải có một index trên cột `email`, tức một secondary index, và câu hỏi là index đó nằm ở đâu khi dữ liệu đã bị chia.

### 4.1 Hai cách đặt index

Cách thứ nhất là để mỗi partition tự giữ index cho dữ liệu của nó: đó là *local index*. Ghi một đơn mới chỉ cần cập nhật partition chứa đơn đó, nhưng tìm theo email thì không biết đơn nằm ở partition nào, nên `orders-api` phải hỏi tất cả rồi gộp kết quả.

Cách thứ hai là dựng một index toàn cục, tự nó cũng được chia ra, nhưng chia theo *giá trị được index* (email) chứ không theo khóa chính: đó là *global index*. Tìm theo email chỉ trúng một partition của index. Đổi lại, đơn mới nằm ở partition này mà entry email của nó nằm ở partition index khác, nên một lần ghi đụng tới hai nơi.

```mermaid
flowchart TB
  subgraph LOC["Local (document-partitioned)"]
    direction LR
    lw["ghi 1 document"] --> lp["partition chứa document<br/>+ index của nó"]
    lr["đọc theo cột phụ"] --> lp1["partition 1 + index"]
    lr --> lp2["partition 2 + index"]
    lr --> lp3["partition 3 + index"]
  end
  subgraph GLO["Global (term-partitioned)"]
    direction LR
    gw["ghi 1 document"] --> gp["partition chứa document"]
    gw -.->|"cập nhật, thường bất đồng bộ"| gi["partition của index<br/>(chia theo giá trị)"]
    gr["đọc theo cột phụ"] --> gi
  end
```

(Sơ đồ minh họa của người soạn, tóm tắt lập luận ở trên.)

### 4.2 Local index trong hệ thống thật

**Cassandra SAI** (Storage-Attached Index) gắn index vào memtable và SSTable: index được tạo lúc chúng được ghi, và chênh lệch giữa các index được xử lý lúc đọc[^sai-attach]. Khi bootstrap hay decommission node, index đi theo SSTable sang node nhận chứ không phải dựng lại[^sai-stream]. Lúc đọc, một "SAI Coordinator" gửi *token range read* tới các replica, và mỗi replica chạy truy vấn index cục bộ[^sai-local]. Coordinator chọn số range cần liên lạc theo lượng hàng ước tính và `LIMIT`[^sai-concurrency]. Đây là mô hình local: index nằm cùng dữ liệu, nên truy vấn theo cột phụ không kèm khóa partition phải gửi tới nhiều range của vòng (suy luận của người soạn từ các mô tả trên).

**Elasticsearch** cũng fan-out. Một request tìm kiếm thường chạm nhiều shard, và mỗi shard chạy tìm kiếm trên một thread[^es-multi-shard]. Thống kê tần suất term để tính điểm mặc định được tính cục bộ trên từng shard: nhanh hơn nhưng điểm có thể kém chính xác. `dfs_query_then_fetch` tính toàn cục bằng thông tin từ mọi shard, chính xác hơn nhưng thêm một vòng đi về tới mỗi shard[^es-dfs]. Phân trang sâu đắt vì mỗi shard phải nạp hit của cả các trang trước vào bộ nhớ[^es-deep-page]. Nếu biết trước khóa routing thì tìm kiếm có thể chỉ gửi tới shard tương ứng thay vì fan-out toàn bộ[^es-custom-routing].

**DynamoDB local secondary index (LSI)** tổ chức theo *cùng partition key* với bảng nhưng khác sort key[^lsi-def]. Vì vậy index đi cùng partition của dữ liệu (suy luận của người soạn). Khác SAI và Elasticsearch, truy vấn LSI chỉ định một giá trị partition key nên không fan-out qua mọi partition (suy luận của người soạn; docs dẫn chỉ nói LSI dùng cùng partition key với bảng).

**PostgreSQL** (partition trong một server) tạo index của bảng partition thành các index riêng trên từng partition. Mỗi index chỉ bảo đảm unique *trong partition của nó*, nên ràng buộc unique hay primary key trên bảng partition phải gồm mọi cột partition key (và không dùng biểu thức hay hàm), để cấu trúc partition tự bảo đảm không trùng giữa các partition[^pg-unique].

### 4.3 Global index trong hệ thống thật

**DynamoDB global secondary index (GSI)** tổ chức theo khóa chính *khác* khóa của bảng. Khóa index không cần có thuộc tính nào của khóa bảng, thậm chí không cần cùng lược đồ khóa[^gsi-def]. GSI được cập nhật *bất đồng bộ* theo mô hình eventually consistent mỗi khi ghi hoặc xóa trên bảng[^gsi-async]. Thay đổi thường lan tới GSI trong một phần giây, nhưng có kịch bản lỗi hiếm làm trễ lâu hơn, nên ứng dụng phải chấp nhận truy vấn GSI trả kết quả cũ[^gsi-stale]. GSI còn có công suất ghi riêng: nếu GSI thiếu write capacity thì ghi vào bảng chính cũng bị throttle[^gsi-throttle].

### 4.4 Vì sao đổi lại như vậy

Local: ghi chạm một partition nên đơn giản hơn (nhận xét của người soạn). Đổi lại, đọc theo cột phụ không kèm khóa partition là fan-out tới mọi partition. MongoDB mô tả điều tương tự cho truy vấn không có shard key: mongos phải hỏi mọi shard, đợi phản hồi rồi mới trả về[^mongo-broadcast]. Global: đọc nhanh, nhưng một lần ghi trở thành thao tác xuyên partition. Nếu muốn index luôn khớp dữ liệu thì cần giao dịch phân tán ([Chương 07](07-transaction.md)); nếu không thì chấp nhận index trễ như GSI (suy luận của người soạn từ hai mô tả trên).

Với `orders`, nếu tìm theo email là truy vấn hiếm thì local index với fan-out là đủ; nếu đó là đường đăng nhập chạy cả ngày thì global index (hoặc một bảng tra email → `order_id` tự duy trì) đáng giá, kèm việc chấp nhận index có thể trễ (nhận xét của người soạn).

<details><summary>Đi sâu: unique constraint là secondary index đặc biệt</summary>

- MongoDB: nếu `_id` không phải shard key thì ràng buộc unique của `_id` chỉ áp trong phạm vi shard lưu document. Hai document cùng `_id` có thể tồn tại ở hai shard khác nhau, và ứng dụng phải tự bảo đảm `_id` duy nhất toàn cụm[^mongo-unique-id]. Với collection đã shard, không thể tạo unique index trên trường khác trừ khi shard key là tiền tố của index[^mongo-unique-prefix].
- Đây chính là bài toán "index cục bộ không thấy được dữ liệu partition khác". Chỉ khi giá trị cần unique *quyết định* partition (là khóa partition hoặc tiền tố) thì mọi bản trùng mới gặp nhau ở cùng một nơi (suy luận của người soạn, khớp với điều kiện của PostgreSQL ở trên).
- Cassandra chia khóa chính thành partition key và clustering; index SAI là thứ dùng khi cần truy vấn ngoài partition key, và chi phí của nó là fan-out như trên (nhận xét của người soạn; nguồn không đưa ra con số).

</details>

**Quay lại Q4:** local index = ghi một nơi, đọc cột phụ (như email của `orders`) hỏi mọi partition; global index = đọc cột phụ trúng ít nơi, ghi phải cập nhật partition khác và thường để index hơi cũ.

## 5. Rebalancing: thêm node thì dữ liệu chuyển thế nào (Q5)

> **Trả lời ngắn.** Rebalancing là chuyển partition giữa các node khi thêm hoặc bớt node, hoặc khi tải lệch. Có ba chiến lược quen thuộc (cách phân loại của người soạn theo khung DDIA; mục lục sách chỉ ghi tên mục "Strategies for Rebalancing"[^ddia-toc-rebal]). Một là *số partition cố định*: tạo sẵn rất nhiều partition, node mới nhận một số partition nguyên vẹn. Hai là *partition động*: tách partition khi quá lớn, gộp khi quá nhỏ. Ba là *tỷ lệ theo số node*: mỗi node giữ một số partition cố định nên số partition tăng theo số node. Điểm chung là *không* dùng `hash mod N` để chọn node: partition là lớp trung gian giữa khóa và node.

**Giải thích.**

*Với `orders`.* Cụm đang 10 node, đĩa sắp đầy nên bạn thêm node thứ 11. Câu hỏi là node mới nhận dữ liệu gì, và `orders-api` có phải xếp lại cả kho không. Cả ba chiến lược trả lời bằng cùng một ý: chèn một lớp partition vào giữa khóa và node. Khóa → partition có ranh giới ổn định, còn partition → node là bảng có thể sửa. Thêm node chỉ đổi vế sau, nên chỉ cần chuyển vài partition nguyên vẹn, không ai bị băm lại từ đầu như với `mod N`.

Một rebalancing tốt phải thỏa ba điều (nhận xét của người soạn): cụm vẫn phục vụ đọc ghi trong lúc di chuyển, chỉ di chuyển phần cần thiết, và băng thông di chuyển được kiểm soát. Về điều cuối, Kafka cho đặt throttle cho lưu lượng replication khi rebalance, bootstrap hay thêm bớt broker, để chặn trên băng thông dùng cho di chuyển[^kafka-throttle]. MongoDB balancer thì chỉ cho mỗi shard tham gia tối đa một di chuyển tại một thời điểm[^mongo-one-migration].

### 5.1. Số partition cố định

Cách đơn giản nhất là tạo trước nhiều partition hơn số node và giữ nguyên số đó. Thêm node thì chuyển một số partition nguyên vẹn sang. Ranh giới khóa → partition không đổi khi thêm node, chỉ ánh xạ partition → node đổi (nhận xét của người soạn; Elasticsearch có ngoại lệ qua API split, xem dưới).

- **Dynamo chiến lược 3**: Q phân vùng bằng nhau, mỗi node Q/S token. Khi node vào, nó "đánh cắp" token từ các node khác để giữ tính chất đó; khi node ra, token của nó được chia ngẫu nhiên cho node còn lại[^dyn-s3-def]. Lợi ích đã nêu ở mục 3: partition là file riêng, chuyển bằng cách chép file[^dyn-s3-files].
- **Redis Cluster**: không gian khóa chia thành 16384 hash slot, với `HASH_SLOT = CRC16(key) mod 16384`[^redis-formula]. Mỗi master giữ một tập slot[^redis-slots]. Thêm node là chuyển một số slot từ các node hiện có sang node trống, bớt node là chuyển slot của nó đi, cân lại là chuyển slot giữa các node: mọi thao tác quy về "di chuyển một hash slot"[^redis-reconf]. Slot vẫn chỉ là tập khóa, nên di chuyển slot là di chuyển các khóa băm vào slot đó[^redis-slot-keys].
- **Kafka**: topic chia thành P partition xác định trước, đánh số 0…P−1[^kafka-pdef]. Có thể *thêm* partition nhưng không giảm[^kafka-no-reduce]. Thêm partition không đổi vị trí của dữ liệu cũ, nên nếu khóa được chia theo `hash(key) % number_of_partitions` thì ánh xạ bị xáo, mà Kafka không tự phân phối lại dữ liệu cũ[^kafka-add-part].
- **Elasticsearch**: `index.number_of_shards` chỉ đặt được lúc tạo index[^es-shards-create]. Để vẫn tách được, API *split* biến mỗi shard thành hai hoặc nhiều shard của index mới, theo các thừa số mà `number_of_routing_shards` cho phép; số này là kích thước "không gian băm" nội bộ[^es-split]. Ví dụ trong docs: 5 shard với routing shards 30 (= 5×2×3) tách được theo hệ số 2 hoặc 3[^es-split-example]. Index phải read-only[^es-split-req]. Cluster health phải green[^es-split-green].

Số partition ban đầu là quyết định khó đảo ngược. Chọn quá ít thì partition sau này quá lớn, khó di chuyển và khó khôi phục (nhận xét của người soạn; Elasticsearch nêu riêng việc shard rất lớn làm phục hồi lâu hơn, ngay bên dưới). Chọn quá nhiều cũng không miễn phí: mỗi shard của Elasticsearch đều có overhead cho quản lý cluster và tìm kiếm, nên cụm có quá nhiều shard ("oversharding") chậm hơn khi tìm kiếm và nặng hơn có thể mất ổn định[^es-oversharding]. Tìm trên một nghìn shard 50MB đắt hơn đáng kể một shard 50GB cùng dữ liệu, nhưng shard rất lớn cũng làm tìm kiếm chậm và phục hồi lâu hơn[^es-shard-size]. Docs gợi ý khoảng 10–50 GB mỗi shard trong nhiều trường hợp, kèm dưới 200 triệu document[^es-10-50].

### 5.2. Partition động

Chiến lược thứ hai hợp với key range, nơi khó chia sẵn ranh giới có nghĩa trước khi biết phân bố khóa: bắt đầu với ít partition, tách khi lớn, gộp khi nhỏ (nhận xét của người soạn).

- **Bigtable**: ban đầu mỗi bảng chỉ có một tablet. Khi bảng lớn lên, nó tự động tách thành nhiều tablet, mỗi cái xấp xỉ 100–200 MB mặc định[^bt-split-size]. Mỗi tablet server quản lý khoảng 10 đến 1000 tablet và tự tách tablet quá lớn[^bt-per-server]. Master gán tablet cho server và cân tải[^bt-master].
- **MongoDB**: chunk mặc định 128 MB. Range nhỏ cho phân bố đều hơn nhưng di chuyển thường xuyên hơn, và thêm overhead ở tầng định tuyến mongos[^mongo-small-range]. Docs nhận xét với phần lớn triển khai, phân bố hơi lệch còn tốt hơn di chuyển liên tục[^mongo-range-size]. Balancer là tiến trình nền theo dõi lượng dữ liệu mỗi shard và di chuyển khi chênh lệch chạm ngưỡng[^mongo-balancer]. Shard mới thêm vào còn trống nên cụm mất cân bằng; MongoDB bắt đầu di chuyển ngay nhưng cần thời gian mới cân xong[^mongo-add-shard].
- Rủi ro (nhận xét của người soạn): với *một khóa duy nhất* quá nóng, chia range không giúp, vì chunk nhỏ nhất cũng chỉ bằng một giá trị khóa[^mongo-smallest-chunk].

### 5.3. Tỷ lệ theo số node

Chiến lược thứ ba giữ kích thước *mỗi* partition ổn định và để số partition tăng theo số node, thay vì cố định số partition (5.1) hay chỉ phụ thuộc kích thước dữ liệu (5.2). Hệ thống gần nhất trong nguồn đã tải là **Cassandra vnode**: mỗi node nhận `num_tokens` token, nên số vnode của cả cụm tăng theo số node (suy luận của người soạn). Khi dùng bộ cấp phát token tự động, tải gán cho node gần tỷ lệ với số vnode của nó[^cass-alloc]. Node mới nhận xấp xỉ bằng nhau từ các node khác[^cass-vnode-benefit].

### 5.4. Tự động hay thủ công?

Dynamo cố ý *không* rebalance khi node chỉ tạm ngừng: một lần ngừng hiếm khi nghĩa là node rời hẳn, nên không nên kích hoạt rebalance hay sửa replica. Việc thêm bớt node trong vòng do quản trị viên ra lệnh tường minh, và thay đổi membership được lan bằng gossip[^dyn-explicit]. Lý do còn thấy rõ ở phía vận hành (nhận xét của người soạn): rebalance tự động kết hợp phát hiện lỗi sai, chẳng hạn một node chậm bị tưởng là chết, có thể chuyển cả đống dữ liệu đúng lúc cụm đang chịu tải và làm sự cố trầm trọng hơn. Để một người trong vòng quyết định, hay ít nhất xác nhận, là phương án an toàn hơn.

<details><summary>Đi sâu: di chuyển một partition mà không dừng ghi</summary>

- MongoDB range migration: shard nguồn nhận lệnh `moveRange`. Trong khi di chuyển, thao tác trên range vẫn gửi tới shard nguồn và shard nguồn chịu trách nhiệm ghi[^mongo-migration-source]. Shard đích yêu cầu document rồi nhận bản sao; sau document cuối, đích đồng bộ các thay đổi xảy ra trong lúc di chuyển[^mongo-migration-sync]. Đọc từ secondary trong lúc di chuyển có thể bỏ sót document nếu truy vấn chạy lâu[^mongo-secondary-miss].
- Redis Cluster: khi cấu hình ổn định, một slot do một node phục vụ[^redis-stable]. Node có thể trả `-MOVED` (slot thuộc node khác) hoặc `-ASK` (slot đang di chuyển), và client phải xử lý cả hai[^redis-asks].
- Bigtable tách tablet: tablet server commit việc tách bằng cách ghi thông tin tablet mới vào bảng METADATA rồi báo master. Nếu thông báo mất, master phát hiện tablet đã tách khi yêu cầu tải tablet[^bt-split-commit].
- Dynamo: node X vào vòng giữa A và B thì các node B, C, D đề nghị và chuyển khóa cho X khi X xác nhận. Vòng xác nhận đó nhằm tránh chuyển trùng[^dyn-bootstrap].

</details>

**Quay lại Q5:** đặt lớp partition giữa khóa và node rồi di chuyển *partition nguyên vẹn*; chọn số cố định (Dynamo, Redis, Kafka, ES), động theo kích thước (Bigtable, MongoDB), hoặc tỷ lệ số node (Cassandra vnode); tốc độ di chuyển nên bị chặn để không làm sập cụm (nhận xét của người soạn; Kafka có throttle, MongoDB giới hạn số migration song song, xem đầu mục 5).

## 6. Request routing: gửi tới đúng node (Q6)

> **Trả lời ngắn.** Cần một *bản đồ* "khóa → partition → node" và ai đó giữ nó. Có ba kiểu: client gửi tới node bất kỳ rồi node đó chuyển tiếp hoặc chỉ đường; một tầng router riêng; hoặc client tự giữ bản đồ và gửi thẳng. Bản đồ gốc nằm ở một dịch vụ phối hợp (ZooKeeper, etcd, Chubby, config server) hoặc tự lan giữa các node bằng gossip. Bản đồ ở client có thể cũ, nên hệ thống tốt phải nhận ra lỗi "sai node" và làm tươi bản đồ.

**Giải thích.**

*Với `orders`.* Request `GET /orders/42` tới `orders-api`. Service tính được khóa, nhưng để gửi query đi nó còn phải biết đơn 42 nằm ở partition nào và partition đó hiện ở node nào. Partition lại di chuyển mỗi khi cụm rebalance (mục 5), nên bản đồ này không đứng yên. Hai câu hỏi của mục này: ai giữ bản đồ, và `orders-api` lấy câu trả lời từ đâu.

### 6.1 Ba kiểu định tuyến

```mermaid
flowchart TB
  subgraph O1["(1) Node bất kỳ rồi chuyển tiếp/chỉ đường"]
    c1["Client"] --> n1["Node bất kỳ"] --> o1["Node sở hữu partition"]
  end
  subgraph O2["(2) Tầng router riêng"]
    c2["Client"] --> r2["Router"] --> o2["Node sở hữu partition"]
  end
  subgraph O3["(3) Client giữ bản đồ"]
    c3["Client"] --> o3["Node sở hữu partition"]
  end
```

(Sơ đồ minh họa của người soạn cho ba kiểu định tuyến mô tả bên dưới.)

*Kiểu 1: node bất kỳ.* Client chưa cần biết gì, cứ gửi tới một node và node đó lo phần còn lại. Ở Cassandra, khi có mutation, coordinator băm partition key để xác định token range rồi sao chép mutation tới các replica theo replication strategy[^cass-coordinator]. Redis Cluster thì để client tự do gửi truy vấn tới bất kỳ node nào; nếu node không phục vụ slot đó, nó trả lỗi `-MOVED` kèm slot và địa chỉ node đúng[^redis-moved]. Node *không* proxy yêu cầu mà chỉ chuyển hướng bằng `-MOVED` hoặc `-ASK`[^redis-no-proxy]. Dynamo hỗ trợ client đi qua một load balancer chung, rồi node nhận chuyển tiếp tới node đúng[^dyn-lb-or-client].

*Kiểu 2: router riêng.* Giữa ứng dụng và cụm có một tầng chuyên định tuyến. MongoDB `mongos` là query router; nó không có trạng thái bền vững mà cache metadata lấy từ config server để biết dữ liệu nào ở shard nào[^mongo-mongos]. Config server là một replica set lưu metadata và cấu hình cụm[^mongo-config]. Nếu đặt proxy hay load balancer trước mongos, phải bật client affinity để mọi kết nối của một client tới cùng một mongos[^mongo-affinity]. Elasticsearch có node điều phối: với search, nó chọn node chứa bản sao của dữ liệu rồi chuyển yêu cầu tới shard của node đó, mặc định bằng adaptive replica selection[^es-search-routing].

*Kiểu 3: client giữ bản đồ.* Đây là kiểu gửi thẳng, ít bước nhất. Ở Kafka, producer gửi thẳng tới broker là leader của partition, không qua tầng định tuyến trung gian, và mọi broker đều trả lời được metadata request cho biết broker nào đang sống và leader của từng partition ở đâu[^kafka-direct]. Client bootstrap bằng danh sách hai ba URL broker[^kafka-bootstrap-list]. Sau đó nó lấy metadata và cache cho tới khi gặp lỗi báo metadata cũ, tức lỗi socket hoặc mã lỗi cho biết broker không còn giữ partition[^kafka-bootstrap]. Dynamo hỗ trợ cả "thư viện client nhận biết partition", định tuyến thẳng tới coordinator với độ trễ thấp hơn vì bỏ một bước chuyển tiếp[^dyn-client-lib]. Client đó định kỳ (10 giây trong paper) hỏi một node ngẫu nhiên để tải trạng thái membership, và paper chọn kéo thay vì đẩy vì kéo scale tốt hơn với số client lớn[^dyn-client-poll]. Bigtable cũng cho client cache vị trí tablet: cache rỗng cần ba round-trip, gồm một lần đọc Chubby, còn cache cũ có thể tới sáu round-trip vì chỉ phát hiện cũ khi trượt[^bt-location-cache].

### 6.2 Ai giữ bản đồ gốc

*Dịch vụ phối hợp (ưu tiên nhất quán).* Bigtable giữ vị trí tablet trong ba tầng giống B+ tree[^bt-3level]. Tầng đầu là một file Chubby chứa vị trí root tablet; root tablet trỏ tới các tablet METADATA, và các tablet này trỏ tới tablet của người dùng[^bt-root]. Bigtable cũng dùng Chubby để theo dõi tablet server[^bt-chubby]. Chubby dùng thuật toán Paxos để giữ các bản sao nhất quán khi có lỗi[^bt-paxos]. ZooKeeper tự mô tả là dịch vụ phối hợp cho ứng dụng phân tán, cung cấp primitive để cài đặt đồng bộ, duy trì cấu hình, nhóm và đặt tên[^zk-overview]. Client có thể đặt *watch* lên znode để được báo khi nó đổi[^zk-watch]. Cập nhật từ một client được áp theo đúng thứ tự gửi[^zk-seq]. Kafka 3.7.0 với cụm dùng ZooKeeper nhận biết broker chết qua node ephemeral của phiên ZooKeeper, và controller phát hiện node bị xóa qua ZooKeeper watch[^kafka-zk-watch]. Với cụm KRaft, phiên được duy trì bằng heartbeat gửi tới controller[^kafka-kraft-hb]. etcd được mô tả là kho key-value nhất quán cho quản lý cấu hình, service discovery và điều phối công việc phân tán[^etcd-use]. Nó dành cho hệ không chấp nhận split-brain và chịu hy sinh availability để đạt điều đó[^etcd-cap]. Cơ chế đồng thuận bên dưới ở [Chương 09](09-consistency-va-consensus.md).

*Gossip (ưu tiên luôn sẵn sàng).* Không có node trung tâm; các node tự kể cho nhau nghe. Ở Dynamo, thay đổi membership được lan bằng giao thức gossip để duy trì một view membership *eventually consistent*; mỗi giây mỗi node liên lạc một peer ngẫu nhiên và hai bên đối chiếu lịch sử thay đổi[^dyn-gossip]. Thông tin token cũng lan theo đường gossip, nên mỗi node biết peer giữ khoảng token nào và chuyển tiếp trực tiếp được[^dyn-gossip-token]. Đó là lý do paper gọi Dynamo là "DHT zero-hop": mỗi node giữ đủ thông tin định tuyến cục bộ để gửi thẳng tới node đúng[^dyn-zero-hop]. Cassandra cũng dùng gossip để lan thông tin token và schema, "dạy" các node biết endpoint nào sở hữu dữ liệu nào[^cass-gossip-token]. Redis Cluster nối các node với nhau bằng cluster bus và dùng gossip để phát hiện node mới và kiểm tra sức sống[^redis-gossip].

Gossip toàn phần có giá. Dynamo thừa nhận mô hình "mỗi node biết dữ liệu của mọi peer" chạy tốt với vài trăm node nhưng khó scale lên hàng chục nghìn, vì chi phí duy trì bảng định tuyến tăng theo kích thước hệ thống[^dyn-full-membership]. Chọn coordination service hay gossip thực chất là chọn *nhất quán* hay *luôn sẵn sàng* cho metadata (nhận xét của người soạn; xem thêm [Chương 09](09-consistency-va-consensus.md)).

### 6.3 Bản đồ cũ thì sao?

Không hệ thống nào đòi client luôn đúng. Chúng *phát hiện sai và sửa*. Redis trả `-MOVED` mỗi khi slot thuộc node khác, kể cả khi client đợi lâu rồi gửi lại sau khi cấu hình đổi[^redis-moved-again]. Khi gặp redirect, nhiều khả năng nhiều slot đã đổi, nên cách tốt thường là làm tươi cả bản đồ chứ không chỉ một slot[^redis-refresh]. Cụm ổn định thì cuối cùng mọi client có bản đồ và gửi thẳng, không cần redirect hay proxy[^redis-stable-map]. Kafka cũng dùng mã lỗi rồi lấy metadata mới[^kafka-bootstrap]. Với `orders-api`, hệ quả là: đừng coi một lần định tuyến sai là lỗi nghiệp vụ, mà hãy xử lý như tín hiệu làm tươi bản đồ rồi gửi lại (nhận xét của người soạn).

<details><summary>Đi sâu: partition quyết định ai ghi — và vì sao bản đồ sai nguy hiểm hơn "chậm"</summary>

- Kafka: các request ghi/đọc phải gửi tới broker đang là leader; broker kiểm tra điều này và trả lỗi `NotLeaderForPartition` nếu gửi sai[^kafka-leader-enforced]. Tức là *sự thật* về ai là leader do broker bảo vệ, còn bản đồ ở client chỉ là gợi ý (nhận xét của người soạn).
- Kafka cũng nói rõ rằng *client* quyết định ánh xạ khóa → partition, broker không áp đặt ngữ nghĩa nào. Hai client muốn dùng cùng cách chia phải dùng cùng phương pháp tính ánh xạ[^kafka-client-controls]. Đây là chỗ mục 8 (Rust) quan trọng: producer Rust và producer Java phải dùng cùng hàm băm.
- Bigtable master không nằm trên đường dữ liệu: client nói chuyện trực tiếp với tablet server, nên hầu hết client không bao giờ liên lạc master và master ít tải[^bt-master-light].
- Ở Cassandra, vì mỗi node đều biết token map qua gossip, bất kỳ node nào cũng làm được coordinator (nhận xét của người soạn, suy ra từ hai mô tả ở mục 6.1 và 6.2).

</details>

**Quay lại Q6:** bản đồ khóa → node do coordination service (nhất quán) hoặc gossip (luôn sẵn sàng, cuối cùng nhất quán) giữ; client dùng node bất kỳ, router, hoặc cache bản đồ, và phải xử lý lỗi "sai node" bằng cách làm tươi bản đồ.

## 7. Giá phải trả của việc chia dữ liệu (Q7)

> **Trả lời ngắn.** Chia dữ liệu cho phép scale-out, nhưng mọi thứ từng "miễn phí" trong một node đều thành bài toán phân tán. Truy vấn không kèm khóa phải hỏi mọi partition; ràng buộc unique chỉ rẻ khi khóa unique chứa khóa chia; thứ tự (như ở Kafka) chỉ được bảo đảm trong một partition; giao dịch qua nhiều partition cần cơ chế riêng. Vì vậy chọn khóa chia là quyết định thiết kế chính.

**Giải thích.**

*Với `orders`.* Khi kho còn một node, `orders-api` làm gì cũng được: join, `COUNT(*)`, chuyển tiền giữa hai đơn trong một transaction. Sau khi chia, mỗi thao tác đó đều phải hỏi "dữ liệu cần dùng có nằm cùng một partition không?". Các mục dưới đi qua từng chi phí và quay lại với bốn truy vấn của `orders`.

### 7.1 Truy vấn không kèm khóa chia

Truy vấn nhanh nhất ở MongoDB thường là truy vấn mà mongos định tuyến được tới một shard nhờ shard key và metadata từ config server (targeted)[^mongo-targeted]. Truy vấn không có shard key phải hỏi mọi shard và chờ phản hồi. Loại này có thể chạy lâu, tùy tải cụm, độ trễ mạng, tải từng shard và số document mỗi shard trả về[^mongo-broadcast]. `updateMany` và `deleteMany` là broadcast trừ khi truy vấn chỉ định đủ shard key[^mongo-multi]. Elasticsearch gặp chuyện tương tự ở phân trang sâu, vì mỗi shard phải nạp hit của các trang trước[^es-deep-page]. Với `orders` chia theo `customer_id`, "đơn của khách C" là targeted, còn báo cáo theo ngày toàn sàn là broadcast (nhận xét của người soạn).

### 7.2 Thao tác nhiều khóa

Redis Cluster hiện thực các lệnh nhiều khóa phức tạp, như hợp và giao tập, chỉ khi mọi khóa băm vào cùng một slot[^redis-multikey]. *Hash tag* `{...}` ép nhiều khóa vào cùng slot: chỉ phần giữa cặp ngoặc đầu tiên được băm[^redis-hashtag]. Dù vậy, khi resharding thủ công, thao tác nhiều khóa có thể tạm thời không dùng được trong khi thao tác một khóa vẫn luôn sẵn sàng[^redis-reshard-multikey]. Hash tag là cách chủ động *đồng-nơi* dữ liệu, đổi lấy nguy cơ hot spot vì mọi khóa cùng tag nằm một slot (nhận xét của người soạn).

### 7.3 Thứ tự và atomic

Kafka chia topic thành các partition có thứ tự toàn phần, và mỗi partition chỉ được đúng một consumer trong mỗi consumer group đọc tại một thời điểm[^kafka-order2]. Các sự kiện cùng khóa được ghi vào cùng partition[^kafka-order]. Nguồn này không bảo đảm thứ tự *giữa các partition*. Vì vậy muốn thứ tự theo thực thể, chẳng hạn mọi sự kiện của một đơn hàng, phải chọn khóa chính là thực thể đó (suy luận của người soạn).

Cassandra nói cùng ý cho atomic: chọn khóa chính nhóm các hàng giúp batch update và có thể bảo đảm các cập nhật atomic và cô lập[^cass-atomic]. Phạm vi của bảo đảm đó là *một partition*. Giao dịch xuyên partition là chủ đề của [Chương 07](07-transaction.md).

### 7.4 Unique, index, và việc chọn khóa

Ràng buộc unique chỉ rẻ khi khóa unique chứa khóa chia (xem mục 4): PostgreSQL đòi vậy[^pg-unique]. MongoDB đòi shard key là tiền tố của unique index[^mongo-unique-prefix].

Chọn khóa chia cũng là chọn truy vấn nào rẻ. PostgreSQL khuyên cột partition nên là cột hay xuất hiện nhất trong `WHERE`, vì chỉ mệnh đề tương thích với cận partition mới cắt được partition thừa. Yêu cầu PRIMARY KEY/UNIQUE và nhu cầu xóa dữ liệu cũ cũng ảnh hưởng quyết định[^pg-choose].

### 7.5 Đổi hàm băm hoặc khóa chia

Đổi hàm băm hay khóa chia thường kéo theo di chuyển dữ liệu. Ở Cassandra, *partitioner*, tức hàm băm gán partition key vào vòng, không thể đổi nếu không nạp lại toàn bộ dữ liệu[^cass-partitioner-fixed]. Mặc định là Murmur3Partitioner[^cass-murmur3]. MongoDB cho phép *sửa giá trị* shard key của document, trừ `_id` bất biến[^mongo-update-key]. Từ MongoDB 5.0 còn đổi được chính shard key và phân phối lại dữ liệu bằng `reshardCollection`[^mongo-reshard]. Việc phân phối lại vẫn xảy ra, chỉ khác là hệ thống làm giúp (suy luận của người soạn).

Vì cả chuỗi chi phí này, quyết định "có cần chia không" nên đến sau khi đã thử một node lớn hơn, replication để đọc, và partition *trong* một server (PostgreSQL, mục 0); xem [Chương 13](13-kien-truc-he-du-lieu.md) (nhận xét của người soạn).

<details><summary>Đi sâu: vì sao "shard theo tenant" thường là điểm bắt đầu hợp lý</summary>

- (Suy luận của người soạn từ các điểm trên.) Nếu truy vấn nghiệp vụ gần như luôn nằm trong một tenant, khách hàng hay tổ chức, thì chọn tenant làm khóa chia biến hầu hết truy vấn thành *targeted*, unique theo tenant dễ giữ trong một partition, và giao dịch thường nằm gọn một partition. Cái giá là tenant lớn thành hot spot (mục 2), và tenant quá lớn thành partition không chia được nếu khóa chỉ là tenant. Vì thế thường cần khóa ghép hoặc tách riêng tenant lớn. Với `orders`, tenant chính là `seller_id`, nên seller lớn ở mục 2 là đúng ví dụ này.
- DynamoDB nói về cách phân tán khóa: khóa phân vùng quyết định các partition logic nơi dữ liệu bảng được lưu, từ đó ảnh hưởng partition vật lý bên dưới[^ddb-pk].

</details>

**Quay lại Q7:** chia dữ liệu biến truy vấn không kèm khóa thành fan-out, biến unique/giao dịch xuyên partition thành bài toán phân tán, và (như Kafka) chỉ bảo đảm thứ tự trong một partition; vì vậy khóa chia phải khớp với cách truy cập phổ biến nhất.

## Liên hệ với Rust (mục 8, Q8)

> **Trả lời ngắn.** Đừng dùng `std::hash::Hash` cùng `DefaultHasher`/`RandomState` để chọn shard hay partition. Docs std nói dữ liệu `Hash` đưa cho `Hasher` không portable giữa nền tảng và không ổn định giữa phiên bản compiler, còn `RandomState` cho kết quả khác nhau giữa các instance (seed ngẫu nhiên, xem [P10 · Chương 09, mục 5](../10-rust-internals/09-collections-internals.md)). Hãy mã hóa khóa thành *chuỗi byte do bạn định nghĩa* rồi băm bằng thuật toán có đặc tả và kết quả ổn định (xxHash, MurmurHash3, SipHash với khóa cố định, CRC16…). Khi nói chuyện với hệ thống khác (Kafka, Redis) phải dùng đúng hàm của hệ đó.

*Với `orders-api`.* Service Rust phải tự tính `partition_of(customer_id)` mỗi khi nhận request, rồi dùng kết quả để chọn connection pool, hoặc để gắn khóa khi ghi lên Kafka. Hàm đó chạy trong nhiều instance và qua nhiều lần deploy, nên mọi instance phải cho cùng một đáp án với cùng một khóa. Điều kiện này loại hầu hết hàm băm "tiện tay" của Rust, như mục 8.1 giải thích.

### 8.1. Vì sao `Hash` của std không dùng được để chia shard

Dữ liệu một giá trị đưa vào `Hasher` qua `Hash` không được coi là portable giữa các nền tảng, do endianness và kích thước kiểu. Với hầu hết kiểu của thư viện chuẩn, nó cũng không được coi là ổn định giữa các phiên bản compiler[^std-hash-portable]. Docs khuyên test không nên so với giá trị băm cứng[^std-hash-hardcoded]. Định dạng tuần tự hóa muốn portable thì tránh mã hóa hash, hoặc chỉ dựa vào các cài đặt `Hash`/`Hasher` có thêm bảo đảm[^std-hash-tests]. Trait `Hasher` cũng không bảo đảm các hàm `write_*` được định nghĩa thế nào: không thể giả định `write_u32` tương đương bốn lần `write_u8`[^std-hasher-write].

`HashMap` mặc định dùng `RandomState`. Một instance tạo ra các hasher giống nhau, nhưng hasher của hai instance khác nhau khó có khả năng cho cùng kết quả với cùng giá trị[^std-randomstate]. `DefaultHasher::new` và `default` giống nhau với nhau nhưng không bảo đảm giống mọi `DefaultHasher` khác[^std-defaulthasher]. SipHash có seed ngẫu nhiên là để chống HashDoS, lý do đã nói ở [P10 · Chương 09, mục 5](../10-rust-internals/09-collections-internals.md). Nhưng *chính* thuộc tính đó khiến nó vô dụng cho sharding: hai tiến trình, hoặc một tiến trình sau khi khởi động lại, sẽ đặt cùng khóa vào hai shard khác nhau (suy luận của người soạn từ docs trên).

### 8.2. Quy trình đúng: mã hóa khóa → băm có đặc tả → ánh xạ vào partition

1. **Mã hóa khóa bằng byte do bạn kiểm soát.** Với `u64` thì dùng `to_be_bytes()` (độ rộng và thứ tự byte cố định), với chuỗi thì UTF-8. Đừng `#[derive(Hash)]` rồi tin kết quả (nhận xét của người soạn, từ 8.1). Crate `twox-hash` nói thẳng: các thuật toán xxHash cho kết quả nhất quán với đầu vào nhất quán và không phụ thuộc nền tảng (32/64-bit, little/big-endian giống nhau)[^twox-portable]. Nhưng trait `Hash` không bảo đảm đưa dữ liệu vào `Hasher` theo kiểu độc lập nền tảng: chẳng hạn `Vec<T>`, `&[T]` hay `BTreeMap` băm độ dài theo cách phụ thuộc nền tảng, nên cho kết quả khác nhau giữa hệ 32 và 64-bit[^twox-hash-trait].
2. **Chọn hàm băm có đặc tả.** `siphasher` 1.0.4 cài SipHash-2-4 và SipHash-1-3, và cho đặt khóa 16 byte tường minh qua `new_with_key`[^siphasher-key]. Crate cảnh báo các phương thức của trait `Hasher` chuẩn cho kết quả không portable, và chỉ `write()` và `finish()` là an toàn[^siphasher-note]. `murmur3` 0.5.2 là bản Rust thuần của MurmurHash3, cung cấp biến thể 32 bit và 128 bit đọc từ một `Read`[^murmur3-crate]. `twox-hash` 2.1.5 có `XxHash64::oneshot(seed, bytes)`[^twox-oneshot].
3. **Ánh xạ hash → partition.** `hash % n_partitions` dùng cho *số partition cố định* (mục 5.1). Nếu `n_partitions` có thể đổi thì dùng lớp partition trung gian hoặc consistent hashing (mục 3), đừng `% số_node`.

```rust
// Minh họa, không biên dịch ở đây; cần bật feature `xxhash64` của twox-hash 2.1.5.
use twox_hash::XxHash64;

/// Số partition cố định, ghi vào "hợp đồng dữ liệu" cùng SEED và cách mã hóa khóa.
const N_PARTITIONS: u64 = 1024;
const SEED: u64 = 0;

fn partition_of(customer_id: u64) -> u32 {
    // Mã hóa tường minh: 8 byte big-endian, không phụ thuộc nền tảng.
    let h = XxHash64::oneshot(SEED, &customer_id.to_be_bytes());
    (h % N_PARTITIONS) as u32
}
```

`SEED`, hàm băm, cách mã hóa khóa và `N_PARTITIONS` hợp thành một "hợp đồng dữ liệu" (nhận xét của người soạn): cả bốn đều là một phần *định dạng lưu trữ*, và đổi bất kỳ thứ nào là di chuyển dữ liệu. Nên viết test với vài cặp (khóa → partition) cố định để bắt thay đổi vô ý khi nâng cấp crate. Docs std khuyên *không* làm vậy với `Hash` của std vì nó không ổn định[^std-hash-hardcoded], nhưng với thuật toán có đặc tả như trên thì test vector là hợp lý.

### 8.3. Khi phải khớp với hệ khác

**Kafka.** Kafka protocol để client điều khiển phép gán khóa → partition, và broker không áp ngữ nghĩa nào. Hai client muốn dùng chung cách chia phải dùng cùng phương pháp tính ánh xạ[^kafka-client-controls]. Producer Java dùng `murmur2` trên byte của khóa đã tuần tự hóa[^kafka-bip]. Phía Rust, crate `rdkafka` 0.39.0 là client dựa trên librdkafka[^rdkafka-crate]. Nó phụ thuộc `rdkafka-sys` 4.10.0[^rdkafka-sys-dep]. Changelog của `rdkafka-sys` ghi bản 4.10.0+2.12.1 nâng librdkafka lên v2.12.1[^rdkafka-sys-ver]. Mặc định `rdkafka-sys` build và liên kết tĩnh bản librdkafka đi kèm[^rdkafka-sys-readme]. Với feature `dynamic-linking` thì bản cài trong hệ thống phải khớp đúng bản đi kèm[^rdkafka-sys-dyn]. (Nhận xét của người soạn: ràng buộc `4.10.0` là semver, nên bản `rdkafka-sys`, và librdkafka kèm theo, phụ thuộc `Cargo.lock` của bạn; hãy đọc lại changelog khi nâng.)

Điểm cần cẩn thận là partitioner mặc định. Trong tài liệu cấu hình librdkafka v2.12.1, `partitioner` mặc định là `consistent_random` (CRC32 của khóa)[^librdkafka-default]. Còn `murmur2_random` được mô tả là "tương thích với Java Producer" và tương đương partitioner mặc định của Java producer[^librdkafka-partitioner]. Suy luận của người soạn: nếu cùng một topic có producer Java và producer Rust đều dùng mặc định, cùng một khóa có thể vào hai partition khác nhau và phá thứ tự theo khóa, nên hãy đặt `partitioner=murmur2_random` ở phía Rust. Cũng có thể chỉ định partition tường minh: `FutureRecord` có trường `partition: Option<i32>`[^rdkafka-record].

**Redis Cluster.** Slot là `CRC16(key) mod 16384`, và nếu khóa chứa `{...}` có nội dung thì chỉ phần đó được băm[^redis-formula]. Quy tắc hash tag này nằm trong spec[^redis-hashtag]. Client Rust tự tính slot (hoặc dùng crate client hỗ trợ cluster) phải dùng đúng biến thể CRC16 mà spec mô tả; spec có cài đặt tham chiếu ở Phụ lục A[^redis-crc-appendix].

**Elasticsearch** chọn shard theo công thức của docs dựa trên `hash(_routing)`[^es-formula] (mục 3, Đi sâu). Cách dùng thông thường là gửi document qua API và để cụm tự tính shard, nên client không cần tự băm (nhận xét của người soạn; docs dẫn chỉ nêu công thức).

### 8.4. Partition theo khoảng khóa: `BTreeMap` làm bảng định tuyến

Với key range, bản đồ "cận dưới của khoảng → partition" là một `BTreeMap<Key, PartitionId>`, và tìm partition của khóa k là tìm khóa lớn nhất ≤ k. `BTreeMap::range` dựng một iterator hai chiều trên một dải con của map, nhận cú pháp `min..max` hoặc cặp `Bound`[^std-btree-range]. Theo bảng trong docs `std::collections`, `range` là O(log n) và `HashMap` thì không có `range`[^coll-btree-range] (xem [P10 · Chương 09, mục 6](../10-rust-internals/09-collections-internals.md)). Vì vậy bảng này cần `BTreeMap` chứ không phải `HashMap`.

```rust
// Minh họa, không biên dịch ở đây.
use std::collections::BTreeMap;
use std::ops::Bound::{Included, Unbounded};

struct RangeRouter { starts: BTreeMap<Vec<u8>, u32> } // cận dưới (bao gồm) -> partition id

impl RangeRouter {
    fn partition_of(&self, key: &[u8]) -> Option<u32> {
        self.starts
            .range::<[u8], _>((Unbounded, Included(key)))
            .next_back()
            .map(|(_, &p)| p)
    }
}
```

(Nhận xét của người soạn: `next_back` lấy phần tử cuối của dải "mọi cận dưới ≤ key", tức cận dưới lớn nhất ≤ key. Vì `Vec<u8>: Borrow<[u8]>` nên tra bằng `&[u8]` không phải cấp phát khóa mới.)

### 8.5. Consistent hashing và client: crate có sẵn

`hashring` 0.3.6 cài consistent hashing theo paper Karger với API `add`, `remove`, `get`. Tác giả chủ ý để API tối giản và để client tự dựng virtual node phía trên; ví dụ trong docs bọc một `VNode { id, addr }` có `Hash`[^hashring-doc]. `HashRing<T, S>` nhận hash builder tường minh qua `with_hasher`[^hashring-hasher]. Hasher mặc định của nó là `DefaultHashBuilder` với `Hasher = SipHasher`[^hashring-default]. Mã nguồn 0.3.6 cho `build_hasher` trả `SipHasher::new()`[^hashring-src]. Crate phụ thuộc `siphasher` `0.3.1`[^hashring-dep]. Docs `siphasher` 0.3.11 ghi `SipHasher::new` đặt hai khóa ban đầu bằng 0[^siphasher-new]. Từ đó suy ra (suy luận của người soạn từ mã nguồn trên) hasher mặc định không có seed ngẫu nhiên như `RandomState`, và cho kết quả lặp lại giữa các tiến trình. Khóa 0 cố định thì không chống được kẻ đoán hash (xem Đi sâu bên dưới), và kết quả vẫn phụ thuộc cách kiểu node/khóa bạn đưa vào cài `Hash` (mục 8.1). Với sharding đánh số tuần tự, `jump-consistent-hash` nhỏ gọn hơn (mục 3, Đi sâu).

(Kết nối chương khác, nhận xét của người soạn: mỗi shard thường là một connection pool riêng tới một DB/node; timeout và backpressure theo từng shard xem [P7 · Chương 09](../07-io-async/09-async-trong-production.md). Một shard chậm không nên kéo cả pool.)

<details><summary>Đi sâu: vì sao std vẫn dùng SipHash seed ngẫu nhiên và điều đó mâu thuẫn gì ở đây</summary>

- Không mâu thuẫn với P10: seed ngẫu nhiên phục vụ `HashMap` *trong bộ nhớ một tiến trình*, chống va chạm do kẻ tấn công điều khiển (nguồn ở [P10 · Chương 09, mục 5](../10-rust-internals/09-collections-internals.md)). Chia shard cần thuộc tính ngược lại: mọi tiến trình và mọi lần khởi động đồng ý hàm băm. Hai mục tiêu cần hai công cụ khác nhau (nhận xét của người soạn).
- Nếu khóa shard do bên ngoài kiểm soát và hash đoán được, kẻ tấn công có thể cố ý đẩy khóa vào một shard: một dạng "hash flooding" ở tầng shard (suy luận của người soạn). Khi đó cân nhắc hash có khóa bí mật (SipHash với khóa riêng) thay vì hash không khóa, đổi lại khóa phải đồng bộ giữa mọi tiến trình và đổi khóa là di chuyển dữ liệu.

</details>

**Quay lại Q8:** hàm băm chọn shard là một phần định dạng dữ liệu; `Hash`/`DefaultHasher` của std không được bảo đảm ổn định, nên mã hóa khóa bằng byte tường minh và băm bằng thuật toán có đặc tả, rồi khớp đúng hàm của Kafka/Redis khi tương tác.

## Tóm tắt

- Partition là chia dữ liệu để scale-out; mỗi hệ thống đặt tên khác nhau (tablet, chunk, shard, slot) nhưng cùng ý. Partition trong một server của PostgreSQL là tối ưu truy vấn và vòng đời dữ liệu, không phải scale-out.
- Key range giữ thứ tự (quét dải rẻ) nhưng dễ hot spot với khóa tăng đơn điệu; hash rải đều nhưng mất quét dải; khóa ghép (băm trước, sắp sau) là thỏa hiệp.
- Hot spot đến từ khóa lệch, cardinality thấp, hoặc một khóa quá nóng; giảm nhẹ bằng hậu tố khóa, chia khóa nóng sang tập shard, đổi khóa; mỗi cách đều tăng chi phí đọc.
- `hash mod N` di chuyển gần hết dữ liệu khi N đổi; consistent hashing cộng vnode chỉ di chuyển phần tối thiểu và cho phép trọng số theo năng lực, với giá là nhiều hàng xóm hơn.
- Secondary index: local là ghi một nơi, đọc cột phụ fan-out; global là đọc trúng ít nơi, ghi xuyên partition và thường bất đồng bộ.
- Rebalancing: số partition cố định (Dynamo, Redis, Kafka, ES), động (Bigtable, MongoDB), tỷ lệ số node (Cassandra vnode). Băng thông di chuyển thường cần được chặn (Kafka throttle, MongoDB giới hạn migration song song), và Dynamo cố ý không rebalance tự động khi node chỉ tạm ngừng.
- Request routing: node bất kỳ cộng chuyển hướng, router riêng, hoặc client giữ bản đồ; bản đồ nằm ở coordination service hoặc gossip; client phải xử lý "sai node".
- Chia dữ liệu làm truy vấn không kèm khóa thành fan-out, còn unique, atomic và thứ tự chỉ bảo đảm trong một partition; chọn khóa chia là quyết định thiết kế chính.
- Rust: `Hash`/`DefaultHasher`/`RandomState` không được bảo đảm ổn định; dùng byte tường minh cộng hàm băm có đặc tả (xxHash, MurmurHash3, SipHash khóa cố định) và khớp hàm của Kafka/Redis khi tương tác.

## Câu hỏi tự kiểm tra

1. Một bảng sự kiện shard theo `created_at` (tăng dần) bằng range sharding; mọi ghi mới rơi vào đâu và vì sao? Đổi sang khóa nào để khắc phục, và bạn mất gì?

   <details><summary>Đáp án</summary>

   Ghi mới rơi vào partition chứa cận trên cao nhất (MongoDB: chunk có cận `MaxKey`), nên ghi bị bó vào một shard[^mongo-mono-hot]. Khắc phục bằng hashed sharding[^mongo-hash-mono]. Cách khác là khóa ghép đặt giá trị phân biệt cao (user ID) trước timestamp[^bt-ts-prefix]. Cái mất là truy vấn dải theo thời gian không còn rẻ ở mức toàn cục: với hash, truy vấn dải nhiều khả năng phải broadcast tới nhiều shard[^mongo-hash].

   </details>

2. Vì sao băm đều khóa vẫn có thể có hot spot? Nêu một cách giảm nhẹ và giá của nó.

   <details><summary>Đáp án</summary>

   Hash chỉ rải đều các khóa khác nhau; một khóa nhận tải quá lớn thì partition chứa nó vẫn nóng. Dynamo giả định có đủ khóa nổi tiếng để rải đều, và giả định này vỡ trong trường hợp đó[^dyn-skew-assume]. Ví dụ: hậu tố ngẫu nhiên vào partition key rải được ghi[^ddb-suffix]. Nhưng đọc phải truy vấn mọi hậu tố rồi gộp kết quả[^ddb-suffix-read].

   </details>

3. Đổi từ 10 lên 12 shard với `h(key) mod N`: khoảng bao nhiêu khóa phải đổi chỗ so với tối thiểu?

   <details><summary>Đáp án</summary>

   Paper Jump Consistent Hash: chỉ cần di chuyển 1/6 dữ liệu, trong khi `mod` làm dữ liệu phải sắp xếp lại gần như hoàn toàn[^jump-1-6]. Theo tính toán của người soạn, khoảng 5/6 khóa đổi chỗ (mục 3).

   </details>

4. Trong consistent hashing kiểu Dynamo, khi node mới vào vòng thì node nào bị ảnh hưởng và vì sao thêm virtual node giúp?

   <details><summary>Đáp án</summary>

   Chỉ hàng xóm gần nhất bị ảnh hưởng[^dyn-ring-neighbors]. Vnode cho node mới nhận lượng tải xấp xỉ bằng nhau từ mọi node khác, và cho phép chọn số vnode theo năng lực[^dyn-vnode-adv].

   </details>

5. Cassandra vnode làm tăng rủi ro nào?

   <details><summary>Đáp án</summary>

   Mỗi token thêm tối đa `2×(RF−1)` hàng xóm, tức nhiều tổ hợp lỗi hơn làm mất khả dụng một phần vòng[^cass-vnode-cost]. Các thao tác bảo trì như repair cũng chậm hơn[^cass-vnode-repair].

   </details>

6. Phân biệt local và global secondary index về đường đọc và đường ghi; kể một hệ thống cho mỗi loại.

   <details><summary>Đáp án</summary>

   Local: index nằm cùng dữ liệu. Với Cassandra SAI, coordinator gửi token range read tới các replica và mỗi replica chạy truy vấn index cục bộ[^sai-local]. DynamoDB LSI dùng cùng partition key với bảng, khác sort key[^lsi-def]. (Suy luận của người soạn: ghi chỉ chạm partition chứa dữ liệu; đọc theo cột phụ không kèm khóa partition như ở SAI phải đi qua nhiều range, còn LSI truy vấn trong một partition key.) Global (DynamoDB GSI): khóa index khác khóa bảng[^gsi-def]. Việc cập nhật bất đồng bộ nên truy vấn có thể trả kết quả cũ[^gsi-async]. Chính độ trễ đó là điều ứng dụng phải chấp nhận[^gsi-stale].

   </details>

7. Vì sao `number_of_shards` cố định của Elasticsearch vẫn tách (split) được index?

   <details><summary>Đáp án</summary>

   Vì định tuyến dùng không gian băm `num_routing_shards` cố định rồi chia gộp theo `routing_factor`[^es-formula]. Split tăng số shard theo thừa số của `number_of_routing_shards`[^es-split]. Split cần index read-only[^es-split-req].

   </details>

8. Kafka thêm partition cho topic: dữ liệu cũ và ánh xạ khóa thay đổi thế nào?

   <details><summary>Đáp án</summary>

   Dữ liệu cũ không được phân phối lại, và nếu dùng `hash(key) % số_partition` thì phép chia khóa thay đổi với khóa mới[^kafka-add-part]. Số partition không thể giảm[^kafka-no-reduce].

   </details>

9. Kể ba kiểu request routing và một ví dụ thật cho mỗi kiểu.

   <details><summary>Đáp án</summary>

   Node bất kỳ cộng chuyển hướng: Redis `-MOVED`[^redis-moved]. Router: MongoDB mongos[^mongo-mongos]. Client giữ bản đồ: Kafka metadata[^kafka-direct]. Client thư viện của Dynamo cũng thuộc kiểu này[^dyn-client-lib].

   </details>

10. Redis Cluster trả `-MOVED` nghĩa là gì và client nên làm gì?

    <details><summary>Đáp án</summary>

    Slot thuộc node khác; lỗi kèm slot và địa chỉ node đúng[^redis-moved]. Client gửi lại tới node đó và nên nhớ ánh xạ[^redis-moved-again]. Thường nên làm tươi toàn bản đồ[^redis-refresh].

    </details>

11. Vì sao không dùng `std::collections::hash_map::DefaultHasher` để chọn shard? Dùng gì thay thế?

    <details><summary>Đáp án</summary>

    Docs std: dữ liệu `Hash` không portable giữa nền tảng và không ổn định giữa phiên bản compiler[^std-hash-portable]. `RandomState` cho kết quả khác nhau giữa các instance[^std-randomstate]. Thay vào đó, dùng byte tường minh rồi băm bằng thuật toán có đặc tả. xxHash cho kết quả nhất quán giữa các nền tảng[^twox-portable]. SipHash cho đặt khóa tường minh[^siphasher-key]. MurmurHash3 có bản Rust thuần[^murmur3-crate].

    </details>

12. Hai producer Kafka, một Java một Rust (`rdkafka` mặc định), gửi cùng khóa. Rủi ro gì?

    <details><summary>Đáp án</summary>

    Java dùng murmur2[^kafka-bip]. Bản librdkafka đi kèm `rdkafka` 0.39.0 theo changelog `rdkafka-sys` là v2.12.1[^rdkafka-sys-ver]. Docs của v2.12.1 ghi partitioner mặc định là `consistent_random` CRC32[^librdkafka-default]. Chỉ `murmur2_random` mới tương thích Java[^librdkafka-partitioner]. Hai bên có thể chọn partition khác nhau cho cùng khóa nếu không cấu hình lại, và broker không áp đặt phép chia[^kafka-client-controls] (suy luận của người soạn về hậu quả).

    </details>

## Đọc thêm

- DDIA chương 6 "Partitioning" (ấn bản 1, Phần II *Distributed Data*): khung chủ đề của chương này; tên chương và các mục con (key range, hash, skewed workloads, secondary index theo document/term, rebalancing, request routing) theo mục lục do ETH Library lưu: <https://toc.library.ethz.ch/objects/pdf03/z01_978-1-4493-7332-0_01.pdf>. Trang chủ sách: <https://dataintensive.net/>.
- Karger và cộng sự, *Consistent Hashing and Random Trees* (STOC 1997): <https://people.csail.mit.edu/karger/Papers/web.pdf>. Mục 4 định nghĩa smoothness/spread/load/balance/monotonicity.
- DeCandia và cộng sự, *Dynamo* (SOSP 2007), mục 4.2 (vòng băm, vnode), 4.8 (membership, gossip), 6.2 (ba chiến lược phân vùng), 6.4 (client-driven coordination): <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf>.
- Chang và cộng sự, *Bigtable* (OSDI 2006), mục 2 (row, tablet) và 5 (tablet location/assignment): <https://research.google.com/archive/bigtable-osdi06.pdf>.
- Lamping & Veach, *A Fast, Minimal Memory, Consistent Hash Algorithm*: <https://arxiv.org/pdf/1406.2294>.
- Cassandra 4.1, Architecture → Dynamo (token ring, vnode, gossip): <https://cassandra.apache.org/doc/4.1/cassandra/architecture/dynamo.html>.
- Redis Cluster specification (hash slot, MOVED/ASK, resharding): <https://github.com/redis/redis-doc/blob/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md>.
- MongoDB 7.0, Sharding: shard key, chunk, balancer: <https://www.mongodb.com/docs/v7.0/core/sharding-data-partitioning/>.
- Kafka 3.7 docs (design, protocol "Partitioning and bootstrapping"): <https://github.com/apache/kafka/tree/3.7.0/docs>.
- Tiếp theo trong phase: [Chương 07 — Transaction](07-transaction.md) (giao dịch xuyên partition), [Chương 08 — Sự cố hệ phân tán](08-su-co-he-phan-tan.md), [Chương 09 — Consistency và consensus](09-consistency-va-consensus.md).

[^bt-tablet]: Bigtable paper (OSDI 2006), mục 2 Rows — <https://research.google.com/archive/bigtable-osdi06.pdf> — "Each row range is called a tablet, which is the unit of distribution and load balancing."
[^mongo-chunk]: MongoDB 7.0, Data Partitioning with Chunks — <https://www.mongodb.com/docs/v7.0/core/sharding-data-partitioning/> — "A chunk consists of a range of sharded data."
[^cass-partition-def]: Cassandra 4.1, Data definition (DDL), Partition key — <https://cassandra.apache.org/doc/4.1/cassandra/cql/ddl.html> — "A partition is the set of rows that share the same value for their partition key."
[^es-shard-hit]: Elasticsearch 8.15, Size your shards — <https://www.elastic.co/guide/en/elasticsearch/reference/8.15/size-your-shards.html> — "Each index in Elasticsearch is divided into one or more shards, each of which may be replicated across multiple nodes to protect against hardware failures."
[^kafka-partitioned]: Kafka 3.7.0 docs, introduction — <https://raw.githubusercontent.com/apache/kafka/3.7.0/docs/introduction.html> — "When a new event is published to a topic, it is actually appended to one of the topic's partitions."
[^redis-slots]: Redis Cluster specification (redis-doc commit 1044224), Key distribution model — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "Each master node in a cluster handles a subset of the 16384 hash slots."
[^pg-virtual]: PostgreSQL 16, Table Partitioning — <https://www.postgresql.org/docs/16/ddl-partitioning.html> — "The partitioned table itself is a “virtual” table having no storage of its own."
[^pg-prune]: PostgreSQL 16, Partition Pruning — <https://www.postgresql.org/docs/16/ddl-partitioning.html> — "the planner will examine the definition of each partition and prove that the partition need not be scanned because it could not contain any rows meeting the query's WHERE clause."
[^bt-short]: Bigtable paper, mục 2 Rows — <https://research.google.com/archive/bigtable-osdi06.pdf> — "reads of short row ranges are efficient and typically require communication with only a small number of machines."
[^bt-reverse]: Bigtable paper, mục 2 Rows — <https://research.google.com/archive/bigtable-osdi06.pdf> — "pages in the same domain are grouped together into contiguous rows by reversing the hostname components of the URLs."
[^mongo-range]: MongoDB 7.0, Ranged Sharding — <https://www.mongodb.com/docs/v7.0/core/ranged-sharding/> — "This allows for efficient queries where reads target documents within a contiguous range."
[^cass-hash]: Cassandra 4.1, Data definition (DDL), Partition key — <https://cassandra.apache.org/doc/4.1/cassandra/cql/ddl.html> — "A hash is computed from the partition key columns and that hash value defines the partition location."
[^mongo-hash]: MongoDB 7.0, Hashed Sharding — <https://www.mongodb.com/docs/v7.0/core/hashed-sharding/> — "Hashed sharding provides a more even data distribution across the sharded cluster at the cost of reducing Targeted Operations vs. Broadcast Operations."
[^kafka-bip]: Apache Kafka 3.7.0, BuiltInPartitioner.java — <https://raw.githubusercontent.com/apache/kafka/3.7.0/clients/src/main/java/org/apache/kafka/clients/producer/internals/BuiltInPartitioner.java> — "return Utils.toPositive(Utils.murmur2(serializedKey)) % numPartitions;"
[^mongo-mono-hot]: MongoDB 7.0, Hashed Sharding, Hashed vs Ranged Sharding — <https://www.mongodb.com/docs/v7.0/core/hashed-sharding/> — "Since the value of X is always increasing, the chunk with an upper bound of MaxKey receives the majority incoming writes."
[^mongo-hash-mono]: MongoDB 7.0, Choose a Shard Key — <https://www.mongodb.com/docs/v7.0/core/sharding-choose-a-shard-key/> — "If your data model requires sharding on a key that changes monotonically, consider using Hashed Sharding."
[^bt-ts-hot]: Google Cloud Bigtable, Schema design — <https://cloud.google.com/bigtable/docs/schema-design> — "Row keys that start with a timestamp. This pattern causes sequential writes to be pushed onto a single node, creating a hotspot."
[^cass-clustering]: Cassandra 4.1, Data definition (DDL), Clustering columns — <https://cassandra.apache.org/doc/4.1/cassandra/cql/ddl.html> — "The clustering columns of a table define the clustering order for the partition of that table."
[^cass-colocate]: Cassandra 4.1, Data definition (DDL), Partition key — <https://cassandra.apache.org/doc/4.1/cassandra/cql/ddl.html> — "all the rows belonging to the same partition are guaranteed to be stored on the same set of replica nodes."
[^bt-ts-prefix]: Google Cloud Bigtable, Schema design — <https://cloud.google.com/bigtable/docs/schema-design> — "If you put a timestamp in a row key, precede it with a high-cardinality value like a user ID to avoid hotspots."
[^pg-range]: PostgreSQL 16, Range Partitioning — <https://www.postgresql.org/docs/16/ddl-partitioning.html> — "with no overlap between the ranges of values assigned to different partitions."
[^pg-hash]: PostgreSQL 16, Hash Partitioning — <https://www.postgresql.org/docs/16/ddl-partitioning.html> — "The table is partitioned by specifying a modulus and a remainder for each partition."
[^pg-other]: PostgreSQL 16, Table Partitioning — <https://www.postgresql.org/docs/16/ddl-partitioning.html> — "Such methods offer flexibility but do not have some of the performance benefits of built-in declarative partitioning."
[^pg-collation]: PostgreSQL 16, pg_dump — <https://www.postgresql.org/docs/16/app-pgdump.html> — "This may be useful when restoring data on a server where rows do not always fall into the same partitions as they did on the original server. That could happen, for example, if the partitioning column is of type text and the two systems have different definitions of the collation used to sort the partitioning column."
[^mongo-initial-chunk]: MongoDB 7.0, Data Partitioning with Chunks — <https://www.mongodb.com/docs/v7.0/core/sharding-data-partitioning/> — "The sharding operation creates one large initial chunk to cover all of the shard key values."
[^mongo-init-hashed]: MongoDB 7.0, Data Partitioning with Chunks — <https://www.mongodb.com/docs/v7.0/core/sharding-data-partitioning/> — "By default, the operation creates 2 chunks per shard and migrates across the cluster."
[^mongo-hash-float]: MongoDB 7.0, Hashed Sharding — <https://www.mongodb.com/docs/v7.0/core/hashed-sharding/> — "MongoDB hashed indexes truncate floating point numbers to 64-bit integers before hashing."
[^mongo-card]: MongoDB 7.0, Choose a Shard Key — <https://www.mongodb.com/docs/v7.0/core/sharding-choose-a-shard-key/> — "A shard key with low cardinality reduces the effectiveness of horizontal scaling in the cluster."
[^mongo-freq]: MongoDB 7.0, Choose a Shard Key — <https://www.mongodb.com/docs/v7.0/core/sharding-choose-a-shard-key/> — "If the majority of documents contain only a subset of the possible shard key values, then the chunks storing the documents with those values can become a bottleneck within the cluster."
[^ddb-uniform]: Amazon DynamoDB Developer Guide, Designing partition keys to distribute your workload — <https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/bp-partition-key-uniform-load.html> — "Status code, where there are only a few possible status codes."
[^dyn-skew-assume]: Dynamo paper (SOSP 2007), mục 6.1 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "Dynamo’s design assumes that even where there is a significant skew in the access distribution there are enough keys in the popular end of the distribution so that the load of handling popular keys can be spread across the nodes uniformly through partitioning."
[^dyn-imbalance2]: Dynamo paper, mục 6.1 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "As seen in the figure, the imbalance ratio decreases with increasing load."
[^dyn-imbalance]: Dynamo paper, mục 6.1 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "In a given time window, a node is considered to be “inbalance”, if the node’s request load deviates from the average load by a value a less than a certain threshold (here 15%)."
[^ddb-suffix]: Amazon DynamoDB Developer Guide, Using write sharding — <https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/bp-partition-key-sharding.html> — "You can add a random number to the partition key values to distribute the items among partitions."
[^ddb-suffix-read]: Amazon DynamoDB Developer Guide, Using write sharding — <https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/bp-partition-key-sharding.html> — "to read all the items for a given day, you would have to query the items for all the suffixes and then merge the results."
[^ddb-calc-suffix]: Amazon DynamoDB Developer Guide, Using write sharding — <https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/bp-partition-key-sharding.html> — "use a number that you can calculate based upon something that you want to query on."
[^es-routing-partition]: Elasticsearch 8.15, _routing field — <https://www.elastic.co/guide/en/elasticsearch/reference/8.15/mapping-routing-field.html> — "As the partition size increases, the more evenly distributed the data will become at the expense of having to search more shards per request."
[^mongo-freq-fix]: MongoDB 7.0, Choose a Shard Key — <https://www.mongodb.com/docs/v7.0/core/sharding-choose-a-shard-key/> — "If your data model requires sharding on a key that has high frequency values, consider using a compound index using a unique or low frequency value."
[^cass-hotspot]: Cassandra 4.1, Data definition (DDL), Partition key — <https://cassandra.apache.org/doc/4.1/cassandra/cql/ddl.html> — "all rows sharing a partition key will be stored on the same node, creating a hotspot for both reading and writing."
[^cass-atomic]: Cassandra 4.1, Data definition (DDL), Partition key — <https://cassandra.apache.org/doc/4.1/cassandra/cql/ddl.html> — "While selecting a primary key that groups table rows assists batch updates and can ensure that the updates are atomic and done in isolation"
[^bt-hotspot-shared]: Google Cloud Bigtable, Schema design — <https://cloud.google.com/bigtable/docs/schema-design> — "Because all tables in an instance are stored on the same tablets, a schema design that results in hotspots in one table can affect the latency of other tables in the same instance."
[^dyn-efficiency]: Dynamo paper, mục 6.2 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "Load balancing efficiency is defined as the ratio of average number of requests served by each node to the maximum number of requests served by the hottest node."
[^cass-naive]: Cassandra 4.1, Architecture → Dynamo — <https://cassandra.apache.org/doc/4.1/cassandra/architecture/dynamo.html> — "In this naive scheme, however, adding a single node might invalidate almost all of the mappings."
[^dyn-ring-neighbors]: Dynamo paper, mục 4.2 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "The principle advantage of consistent hashing is that departure or arrival of a node only affects its immediate neighbors and other nodes remain unaffected."
[^dyn-vnodes]: Dynamo paper, mục 4.2 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "instead of mapping a node to a single point in the circle, each node gets assigned to multiple points in the ring."
[^jump-1-6]: Lamping & Veach, Jump Consistent Hash, Introduction — <https://arxiv.org/pdf/1406.2294> — "But it is only necessary to move 1/6 of the data stored in the 10 shards in order to end up with the data balanced among 12 shards."
[^kar-def]: Karger et al., Consistent Hashing and Random Trees (STOC 1997), Abstract — <https://people.csail.mit.edu/karger/Papers/web.pdf> — "Roughly speaking, a consistent hash function is one which changes minimally as the range of the function changes."
[^kar-views]: Karger et al., mục 1 — <https://people.csail.mit.edu/karger/Papers/web.pdf> — "so that clients may have incompatible “views” of which machines are available to replicate data."
[^kar-smooth]: Karger et al., mục 4 — <https://people.csail.mit.edu/karger/Papers/web.pdf> — "When a machine is added to or removed from the set of caches, the expected fraction of objects that must be moved to a new cache is the minimum needed to maintain a balanced load across the caches."
[^kar-mono]: Karger et al., mục 4.1 (Monotonicity) — <https://people.csail.mit.edu/karger/Papers/web.pdf> — "an item may move from an old bucket to a new bucket, but not from one old bucket to another."
[^dyn-ring]: Dynamo paper, mục 4.2 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "walking the ring clockwise to find the first node with a position larger than the item’s position."
[^cass-rf-walk]: Cassandra 4.1, Architecture → Dynamo — <https://cassandra.apache.org/doc/4.1/cassandra/architecture/dynamo.html> — "until we encounter three distinct nodes, at which point we have found all the replicas of that key."
[^dyn-challenges]: Dynamo paper, mục 4.2 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "First, the random position assignment of each node on the ring leads to non-uniform data and load distribution."
[^dyn-vnode-adv]: Dynamo paper, mục 4.2 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "The number of virtual nodes that a node is responsible can decided based on its capacity, accounting for heterogeneity in the physical infrastructure."
[^kar-multipoint]: Karger et al., mục 4.2 — <https://people.csail.mit.edu/karger/Papers/web.pdf> — "we actually need to have more than one point in the unit interval associated with each bucket."
[^cass-num-tokens]: Cassandra 4.1, cassandra.yaml, num_tokens — <https://cassandra.apache.org/doc/4.1/cassandra/configuration/cass_yaml_file.html> — "The more tokens, relative to other nodes, the larger the proportion of data that this node will store."
[^cass-vnode-cost]: Cassandra 4.1, Architecture → Dynamo — <https://cassandra.apache.org/doc/4.1/cassandra/architecture/dynamo.html> — "Every token introduces up to 2 * (RF - 1) additional neighbors on the token ring, which means that there are more combinations of node failures where we lose availability for a portion of the token ring."
[^cass-vnode-repair]: Cassandra 4.1, Architecture → Dynamo — <https://cassandra.apache.org/doc/4.1/cassandra/architecture/dynamo.html> — "as the number of tokens per node is increased, the number of discrete repair operations the cluster must do also increases."
[^cass-256]: Cassandra 4.1, Architecture → Dynamo — <https://cassandra.apache.org/doc/4.1/cassandra/architecture/dynamo.html> — "the only token allocation algorithm available was picking random tokens, which meant that to keep balance the default number of tokens per node had to be quite high, at 256."
[^jump-abstract]: Lamping & Veach, Jump Consistent Hash, Abstract — <https://arxiv.org/pdf/1406.2294> — "Its main limitation is that the buckets must be numbered sequentially, which makes it more suitable for data storage applications than for distributed web caching."
[^jump-crate]: docs.rs, jump-consistent-hash 0.1.0 — <https://docs.rs/jump-consistent-hash/0.1.0/jump_consistent_hash/> — "Takes a 64 bit key and the number of buckets, outputs a bucket number 0..buckets."
[^dyn-s1]: Dynamo paper, mục 6.2 Strategy 1 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "Note that the space needed to maintain the membership at each node increases linearly with the number of nodes in the system."
[^dyn-s2]: Dynamo paper, mục 6.2 Strategy 2 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "the bootstrapping has taken almost a day to complete."
[^dyn-s3-better]: Dynamo paper, mục 6.2 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "Compared to Strategy 1, Strategy 3 achieves better efficiency and reduces the size of membership information maintained at each node by three orders of magnitude."
[^dyn-s3-files]: Dynamo paper, mục 6.2 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "Since partition ranges are fixed, they can be stored in separate files, meaning a partition can be relocated as a unit by simply transferring the file"
[^dyn-s3-cost]: Dynamo paper, mục 6.2 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "The disadvantage of strategy 3 is that changing the node membership requires coordination in order to preserve the properties required of the assignment."
[^es-formula]: Elasticsearch 8.15, _routing field — <https://www.elastic.co/guide/en/elasticsearch/reference/8.15/mapping-routing-field.html> — "shard_num = (hash(_routing) % num_routing_shards) / routing_factor"
[^sai-attach]: Cassandra 5.0, Storage-Attached Indexing concepts — <https://cassandra.apache.org/doc/5.0/cassandra/developing/cql/indexing/sai/sai-concepts.html> — "SAI indexes Memtables and Sorted String Tables (SSTables) as they are written, resolving the differences between those indexes at read time."
[^sai-stream]: Cassandra 5.0, SAI concepts — <https://cassandra.apache.org/doc/5.0/cassandra/developing/cql/indexing/sai/sai-concepts.html> — "when you bootstrap or decommission nodes in a cluster, the indexes are fully streamed with the SSTables and not serialized or rebuilt on the receiving node’s end."
[^sai-local]: Cassandra 5.0, SAI concepts — <https://cassandra.apache.org/doc/5.0/cassandra/developing/cql/indexing/sai/sai-concepts.html> — "Once a replica receives a token range read request from the SAI Coordinator, the local index query begins."
[^sai-concurrency]: Cassandra 5.0, SAI concepts — <https://cassandra.apache.org/doc/5.0/cassandra/developing/cql/indexing/sai/sai-concepts.html> — "The SAI Coordinator estimates the concurrency factor, the number of rows per range based on local data and the query limit to determine the number of ranges to contact."
[^es-multi-shard]: Elasticsearch 8.15, Size your shards — <https://www.elastic.co/guide/en/elasticsearch/reference/8.15/size-your-shards.html> — "Most searches hit multiple shards. Each shard runs the search on a single CPU thread."
[^es-dfs]: Elasticsearch 8.15, Search API — <https://www.elastic.co/guide/en/elasticsearch/reference/8.15/search-search.html> — "While this option increases the accuracy of scoring, it adds a round-trip to each shard, which can result in slower searches."
[^es-deep-page]: Elasticsearch 8.15, Paginate search results — <https://www.elastic.co/guide/en/elasticsearch/reference/8.15/paginate-search-results.html> — "Each shard must load its requested hits and the hits for any previous pages into memory."
[^es-custom-routing]: Elasticsearch 8.15, _routing field — <https://www.elastic.co/guide/en/elasticsearch/reference/8.15/mapping-routing-field.html> — "Instead of having to fan out a search request to all the shards in an index, the request can be sent to just the shard that matches the specific routing value (or values)"
[^lsi-def]: Amazon DynamoDB Developer Guide, Local secondary indexes — <https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/LSI.html> — "The data in a local secondary index is organized by the same partition key as the base table, but with a different sort key."
[^pg-unique]: PostgreSQL 16, Table Partitioning, Limitations — <https://www.postgresql.org/docs/16/ddl-partitioning.html> — "the constraint's columns must include all of the partition key columns."
[^gsi-def]: Amazon DynamoDB Developer Guide, Global secondary indexes — <https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/GSI.html> — "A global secondary index contains a selection of attributes from the base table, but they are organized by a primary key that is different from that of the table."
[^gsi-async]: Amazon DynamoDB Developer Guide, Global secondary indexes — <https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/GSI.html> — "any global secondary indexes on that table are updated asynchronously, using an eventually consistent model."
[^gsi-stale]: Amazon DynamoDB Developer Guide, Global secondary indexes — <https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/GSI.html> — "your applications need to anticipate and handle situations where a query on a global secondary index returns results that are not up to date."
[^gsi-throttle]: Amazon DynamoDB Developer Guide, Global secondary indexes — <https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/GSI.html> — "a global secondary index on that table has insufficient write capacity, the write activity on the table will be throttled."
[^mongo-broadcast]: MongoDB 7.0, Sharded cluster query router — <https://www.mongodb.com/docs/v7.0/core/sharded-cluster-query-router/> — "For queries that don't include the shard key, mongos must query all shards, wait for their responses and then return the result to the application."
[^mongo-unique-id]: MongoDB 7.0, Unique indexes — <https://www.mongodb.com/docs/v7.0/core/index-unique/> — "If the _id field is not the shard key, the uniqueness constraint only applies to the shard that stores the document."
[^mongo-unique-prefix]: MongoDB 7.0, Unique indexes — <https://www.mongodb.com/docs/v7.0/core/index-unique/> — "For an already-sharded collection, you cannot create unique indexes on other fields unless the shard key is included as the prefix."
[^kafka-throttle]: Kafka 3.7.0 docs, Operations — <https://raw.githubusercontent.com/apache/kafka/3.7.0/docs/ops.html> — "Kafka lets you apply a throttle to replication traffic, setting an upper bound on the bandwidth used to move replicas from machine to machine."
[^mongo-one-migration]: MongoDB 7.0, Sharded cluster balancer — <https://www.mongodb.com/docs/v7.0/core/sharding-balancer-administration/> — "Restricting a shard to at most one migration at any given time."
[^dyn-s3-def]: Dynamo paper, mục 6.2 Strategy 3 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "each node is assigned Q/S tokens where S is the number of nodes in the system."
[^redis-formula]: Redis Cluster specification, Key distribution model — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "HASH_SLOT = CRC16(key) mod 16384"
[^redis-reconf]: Redis Cluster specification, Live reconfiguration — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "Adding or removing a node is abstracted into the same operation: moving a hash slot from one node to another."
[^redis-slot-keys]: Redis Cluster specification, Live reconfiguration — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "Moving a hash slot means moving all the keys that happen to hash into this hash slot."
[^kafka-pdef]: Kafka 3.7.0 docs, protocol — <https://raw.githubusercontent.com/apache/kafka/3.7.0/docs/protocol.html> — "topics are split into a pre-defined number of partitions, P, and each partition is replicated with some replication factor, N."
[^kafka-no-reduce]: Kafka 3.7.0 docs, Operations — <https://raw.githubusercontent.com/apache/kafka/3.7.0/docs/ops.html> — "Kafka does not currently support reducing the number of partitions for a topic."
[^kafka-add-part]: Kafka 3.7.0 docs, Operations — <https://raw.githubusercontent.com/apache/kafka/3.7.0/docs/ops.html> — "adding partitions doesn't change the partitioning of existing data so this may disturb consumers if they rely on that partition."
[^es-shards-create]: Elasticsearch 8.15, Index modules — <https://www.elastic.co/guide/en/elasticsearch/reference/8.15/index-modules.html> — "The number of primary shards that an index should have. Defaults to 1. This setting can only be set at index creation time."
[^es-split]: Elasticsearch 8.15, Split index API — <https://www.elastic.co/guide/en/elasticsearch/reference/8.15/indices-split-index.html> — "The number of routing shards specifies the hashing space that is used internally to distribute documents across shards with consistent hashing."
[^es-split-req]: Elasticsearch 8.15, Split index API — <https://www.elastic.co/guide/en/elasticsearch/reference/8.15/indices-split-index.html> — "The index must be read-only."
[^es-oversharding]: Elasticsearch 8.15, Size your shards — <https://www.elastic.co/guide/en/elasticsearch/reference/8.15/size-your-shards.html> — "A cluster with too many indices or shards is said to suffer from oversharding."
[^es-shard-size]: Elasticsearch 8.15, Size your shards — <https://www.elastic.co/guide/en/elasticsearch/reference/8.15/size-your-shards.html> — "Searching a thousand 50MB shards will be substantially more expensive than searching a single 50GB shard containing the same data."
[^es-10-50]: Elasticsearch 8.15, Size your shards — <https://www.elastic.co/guide/en/elasticsearch/reference/8.15/size-your-shards.html> — "experience shows that shards between 10GB and 50GB typically work well for many use cases, as long as the per-shard document count is kept below 200 million."
[^bt-split-size]: Bigtable paper, mục 5 — <https://research.google.com/archive/bigtable-osdi06.pdf> — "As a table grows, it is automatically split into multiple tablets, each approximately 100-200 MB in size by default."
[^bt-per-server]: Bigtable paper, mục 5 — <https://research.google.com/archive/bigtable-osdi06.pdf> — "Each tablet server manages a set of tablets (typically we have somewhere between ten to a thousand tablets per tablet server)."
[^bt-master]: Bigtable paper, mục 5 — <https://research.google.com/archive/bigtable-osdi06.pdf> — "The master is responsible for assigning tablets to tablet servers, detecting the addition and expiration of tablet servers, balancing tablet-server load"
[^mongo-small-range]: MongoDB 7.0, Data Partitioning with Chunks — <https://www.mongodb.com/docs/v7.0/core/sharding-data-partitioning/> — "Small ranges lead to a more even distribution of data at the expense of more frequent migrations, which adds overhead at the query routing (mongos) layer."
[^mongo-range-size]: MongoDB 7.0, Data Partitioning with Chunks — <https://www.mongodb.com/docs/v7.0/core/sharding-data-partitioning/> — "For most deployments, a slightly uneven data distribution is preferable to frequent migrations."
[^mongo-balancer]: MongoDB 7.0, Sharded cluster balancer — <https://www.mongodb.com/docs/v7.0/core/sharding-balancer-administration/> — "The MongoDB balancer is a background process that monitors the amount of data on each shard for each sharded collection."
[^mongo-add-shard]: MongoDB 7.0, Sharded cluster balancer — <https://www.mongodb.com/docs/v7.0/core/sharding-balancer-administration/> — "Adding a shard to a cluster creates an imbalance, since the new shard has no data."
[^mongo-smallest-chunk]: MongoDB 7.0, Data Partitioning with Chunks — <https://www.mongodb.com/docs/v7.0/core/sharding-data-partitioning/> — "The smallest unit of data a chunk can represent is a single unique shard key value."
[^cass-alloc]: Cassandra 4.1, cassandra.yaml, allocate_tokens — <https://cassandra.apache.org/doc/4.1/cassandra/configuration/cass_yaml_file.html> — "The load assigned to each node will be close to proportional to its number of vnodes."
[^cass-vnode-benefit]: Cassandra 4.1, Architecture → Dynamo — <https://cassandra.apache.org/doc/4.1/cassandra/architecture/dynamo.html> — "When a new node is added it accepts approximately equal amounts of data from other nodes in the ring, resulting in equal distribution of data across the cluster."
[^dyn-explicit]: Dynamo paper, mục 4.8.1 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "A node outage rarely signifies a permanent departure and therefore should not result in rebalancing of the partition assignment or repair of the unreachable replicas."
[^mongo-migration-source]: MongoDB 7.0, Sharded cluster balancer, Range Migration Procedure — <https://www.mongodb.com/docs/v7.0/core/sharding-balancer-administration/> — "During the migration process, operations to the range are sent to the source shard."
[^mongo-migration-sync]: MongoDB 7.0, Sharded cluster balancer, Range Migration Procedure — <https://www.mongodb.com/docs/v7.0/core/sharding-balancer-administration/> — "After receiving the final document in the range, the destination shard starts a synchronization process to ensure that it has the changes to the migrated documents that occurred during the migration."
[^mongo-secondary-miss]: MongoDB 7.0, Sharded cluster balancer — <https://www.mongodb.com/docs/v7.0/core/sharding-balancer-administration/> — "Long-running secondary reads in a sharded cluster can miss documents if migrations are occurring."
[^redis-stable]: Redis Cluster specification, Key distribution model — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "When the cluster is stable, a single hash slot will be served by a single node"
[^redis-asks]: Redis Cluster specification, MOVED Redirection — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "A client must be also able to handle -ASK redirections that are described later in this document, otherwise it is not a complete Redis Cluster client."
[^bt-split-commit]: Bigtable paper, mục 5.2 — <https://research.google.com/archive/bigtable-osdi06.pdf> — "The tablet server commits the split by recording information for the new tablet in the METADATA table. When the split has committed, it notifies the master."
[^dyn-bootstrap]: Dynamo paper, mục 4.9 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "by adding a confirmation round between the source and the destination, it is made sure that the destination node does not receive any duplicate transfers for a given key range."
[^cass-coordinator]: Cassandra 4.1, Architecture → Dynamo — <https://cassandra.apache.org/doc/4.1/cassandra/architecture/dynamo.html> — "the coordinator hashes the partition key to determine the token range the data belongs to and then replicates the mutation to the replicas of that data according to the Replication Strategy."
[^redis-moved]: Redis Cluster specification, MOVED Redirection — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "the node will check its internal hash slot to node map, and will reply to the client with a MOVED error"
[^redis-no-proxy]: Redis Cluster specification, Client and Server roles — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "Since cluster nodes are not able to proxy requests, clients may be redirected to other nodes using redirection errors -MOVED and -ASK."
[^dyn-lb-or-client]: Dynamo paper, mục 4.1 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "route its request through a generic load balancer that will select a node based on load information"
[^mongo-mongos]: MongoDB 7.0, Sharded cluster query router — <https://www.mongodb.com/docs/v7.0/core/sharded-cluster-query-router/> — "The mongos tracks what data is on which shard by caching the metadata from the config servers."
[^mongo-config]: MongoDB 7.0, Sharded Cluster Components — <https://www.mongodb.com/docs/v7.0/core/sharded-cluster-components/> — "Config servers store metadata and configuration settings for the cluster."
[^mongo-affinity]: MongoDB 7.0, Sharded Cluster Components — <https://www.mongodb.com/docs/v7.0/core/sharded-cluster-components/> — "Client affinity allows every connection from a single client to reach the same mongos."
[^es-search-routing]: Elasticsearch 8.15, Search shard routing — <https://www.elastic.co/guide/en/elasticsearch/reference/8.15/search-shard-routing.html> — "Elasticsearch selects a node containing a copy of the index’s data and forwards the search request to that node’s shards."
[^kafka-direct]: Kafka 3.7.0 docs, design — <https://raw.githubusercontent.com/apache/kafka/3.7.0/docs/design.html> — "The producer sends data directly to the broker that is the leader for the partition without any intervening routing tier."
[^kafka-bootstrap-list]: Kafka 3.7.0 docs, protocol — <https://raw.githubusercontent.com/apache/kafka/3.7.0/docs/protocol.html> — "the best practice for a client implementation is to take a list of two or three URLs to bootstrap from."
[^kafka-bootstrap]: Kafka 3.7.0 docs, protocol — <https://raw.githubusercontent.com/apache/kafka/3.7.0/docs/protocol.html> — "it can fetch metadata once when it is instantiated cache that metadata until it receives an error indicating that the metadata is out of date."
[^dyn-client-lib]: Dynamo paper, mục 4.1 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "the second strategy can achieve lower latency because it skips a potential forwarding step."
[^dyn-client-poll]: Dynamo paper, mục 6.4 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "Currently clients poll a random Dynamo node every 10 seconds for membership updates. A pull based approach was chosen over a push based one as the former scales better with large number of clients"
[^bt-location-cache]: Bigtable paper, mục 5.1 — <https://research.google.com/archive/bigtable-osdi06.pdf> — "If the client’s cache is stale, the location algorithm could take up to six round-trips, because stale cache entries are only discovered upon misses"
[^bt-root]: Bigtable paper, mục 5.1 — <https://research.google.com/archive/bigtable-osdi06.pdf> — "The first level is a file stored in Chubby that contains the location of the root tablet."
[^bt-3level]: Bigtable paper, mục 5.1 — <https://research.google.com/archive/bigtable-osdi06.pdf> — "We use a three-level hierarchy analogous to that of a B+ tree"
[^bt-chubby]: Bigtable paper, mục 5.2 — <https://research.google.com/archive/bigtable-osdi06.pdf> — "Bigtable uses Chubby to keep track of tablet servers."
[^zk-overview]: ZooKeeper 3.9.2, Overview — <https://zookeeper.apache.org/doc/r3.9.2/zookeeperOver.html> — "It exposes a simple set of primitives that distributed applications can build upon to implement higher level services for synchronization, configuration maintenance, and groups and naming."
[^zk-watch]: ZooKeeper 3.9.2, Overview — <https://zookeeper.apache.org/doc/r3.9.2/zookeeperOver.html> — "Clients can set a watch on a znode. A watch will be triggered and removed when the znode changes."
[^kafka-zk-watch]: Kafka 3.7.0 docs, design — <https://raw.githubusercontent.com/apache/kafka/3.7.0/docs/design.html> — "The controller would then notice the node deletion through a Zookeeper watch and mark the broker offline."
[^kafka-kraft-hb]: Kafka 3.7.0 docs, design — <https://raw.githubusercontent.com/apache/kafka/3.7.0/docs/design.html> — "For KRaft clusters, an active session is maintained by sending periodic heartbeats to the controller."
[^etcd-use]: etcd v3.5, Why etcd — <https://etcd.io/docs/v3.5/learning/why/> — "Distributed systems use etcd as a consistent key-value store for configuration management, service discovery, and coordinating distributed work."
[^etcd-cap]: etcd v3.5, Why etcd — <https://etcd.io/docs/v3.5/learning/why/> — "These are systems that will never tolerate split-brain operation and are willing to sacrifice availability to achieve this end."
[^dyn-gossip]: Dynamo paper, mục 4.8.1 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "A gossip-based protocol propagates membership changes and maintains an eventually consistent view of membership."
[^dyn-gossip-token]: Dynamo paper, mục 4.8.1 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "This allows each node to forward a key’s read/write operations to the right set of nodes directly."
[^dyn-zero-hop]: Dynamo paper, mục 4 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "Dynamo can be characterized as a zero-hop DHT, where each node maintains enough routing information locally to route a request to the appropriate node directly."
[^cass-gossip-token]: Cassandra 4.1, Architecture → Dynamo, Gossip — <https://cassandra.apache.org/doc/4.1/cassandra/architecture/dynamo.html> — "As token information propagates via gossip it is also the control plane for teaching nodes which endpoints own what data."
[^redis-gossip]: Redis Cluster specification, Client and Server roles — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "Nodes use a gossip protocol to propagate information about the cluster in order to discover new nodes, to send ping packets to make sure all the other nodes are working properly"
[^dyn-full-membership]: Dynamo paper, mục 6.6 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "This model works well for a system that contains couple of hundreds of nodes."
[^redis-moved-again]: Redis Cluster specification, MOVED Redirection — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "The client is not required to, but should try to memorize that hash slot 3999 is served by 127.0.0.1:6381."
[^redis-refresh]: Redis Cluster specification, MOVED Redirection — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "When a redirection is encountered, it is likely multiple slots were reconfigured rather than just one, so updating the client configuration as soon as possible is often the best strategy."
[^redis-stable-map]: Redis Cluster specification, MOVED Redirection — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "with clients directly addressing the right nodes without redirections, proxies or other single point of failure entities."
[^kafka-leader-enforced]: Kafka 3.7.0 docs, protocol — <https://raw.githubusercontent.com/apache/kafka/3.7.0/docs/protocol.html> — "These requests to publish or fetch data must be sent to the broker that is currently acting as the leader for a given partition."
[^kafka-client-controls]: Kafka 3.7.0 docs, protocol — <https://raw.githubusercontent.com/apache/kafka/3.7.0/docs/protocol.html> — "If two clients want to use the same partitioning scheme they must use the same method to compute the mapping of key to partition."
[^bt-master-light]: Bigtable paper, mục 5 — <https://research.google.com/archive/bigtable-osdi06.pdf> — "Because Bigtable clients do not rely on the master for tablet location information, most clients never communicate with the master."
[^mongo-targeted]: MongoDB 7.0, Sharded cluster query router — <https://www.mongodb.com/docs/v7.0/core/sharded-cluster-query-router/> — "Generally, the fastest queries in a sharded environment are those that mongos route to a single shard, using the shard key and the cluster meta data from the config server."
[^mongo-multi]: MongoDB 7.0, Sharded cluster query router — <https://www.mongodb.com/docs/v7.0/core/sharded-cluster-query-router/> — "The updateMany() and deleteMany() methods are broadcast operations, unless the query document specifies the shard key in full."
[^redis-multikey]: Redis Cluster specification, Implemented subset — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "Commands performing complex multi-key operations like set unions and intersections are implemented for cases where all of the keys involved in the operation hash to the same slot."
[^redis-hashtag]: Redis Cluster specification, Hash tags — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "Hash tags are a way to ensure that multiple keys are allocated in the same hash slot."
[^redis-reshard-multikey]: Redis Cluster specification, Implemented subset — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "during manual resharding, multi-key operations may become unavailable for some time while single-key operations are always available."
[^kafka-order2]: Kafka 3.7.0 docs, design — <https://raw.githubusercontent.com/apache/kafka/3.7.0/docs/design.html> — "Our topic is divided into a set of totally ordered partitions, each of which is consumed by exactly one consumer within each subscribing consumer group at any given time."
[^kafka-order]: Kafka 3.7.0 docs, introduction — <https://raw.githubusercontent.com/apache/kafka/3.7.0/docs/introduction.html> — "Events with the same event key (e.g., a customer or vehicle ID) are written to the same partition"
[^pg-choose]: PostgreSQL 16, Partitioning Best Practices — <https://www.postgresql.org/docs/16/ddl-partitioning.html> — "Often the best choice will be to partition by the column or set of columns which most commonly appear in WHERE clauses of queries being executed on the partitioned table."
[^cass-partitioner-fixed]: Cassandra 4.1, cassandra.yaml, partitioner — <https://cassandra.apache.org/doc/4.1/cassandra/configuration/cass_yaml_file.html> — "The partitioner can NOT be changed without reloading all data."
[^cass-murmur3]: Cassandra 4.1, cassandra.yaml, partitioner — <https://cassandra.apache.org/doc/4.1/cassandra/configuration/cass_yaml_file.html> — "The default partitioner is the Murmur3Partitioner."
[^mongo-update-key]: MongoDB 7.0, Choose a Shard Key — <https://www.mongodb.com/docs/v7.0/core/sharding-choose-a-shard-key/> — "You can update a document's shard key value unless the shard key field is the immutable _id field."
[^ddb-pk]: Amazon DynamoDB Developer Guide, Designing partition keys — <https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/bp-partition-key-uniform-load.html> — "The partition key portion of a table's primary key determines the logical partitions in which a table's data is stored."
[^std-hash-portable]: Rust 1.95.0 std, trait Hash, Portability — <https://doc.rust-lang.org/1.95.0/std/hash/trait.Hash.html> — "Due to differences in endianness and type sizes, data fed by Hash to a Hasher should not be considered portable across platforms."
[^std-hash-hardcoded]: Rust 1.95.0 std, trait Hash, Portability — <https://doc.rust-lang.org/1.95.0/std/hash/trait.Hash.html> — "This means tests shouldn’t probe hard-coded hash values or data fed to a Hasher and instead should check consistency with Eq."
[^std-hash-tests]: Rust 1.95.0 std, trait Hash, Portability — <https://doc.rust-lang.org/1.95.0/std/hash/trait.Hash.html> — "Serialization formats intended to be portable between platforms or compiler versions should either avoid encoding hashes or only rely on Hash and Hasher implementations that provide additional guarantees."
[^std-hasher-write]: Rust 1.95.0 std, trait Hasher — <https://doc.rust-lang.org/1.95.0/std/hash/trait.Hasher.html> — "This trait provides no guarantees about how the various write_* methods are defined and implementations of Hash should not assume that they work one way or another."
[^std-randomstate]: Rust 1.95.0 std, struct RandomState — <https://doc.rust-lang.org/1.95.0/std/hash/struct.RandomState.html> — "the hashers created by two different RandomState instances are unlikely to produce the same result for the same values."
[^std-defaulthasher]: Rust 1.95.0 std, struct DefaultHasher — <https://doc.rust-lang.org/1.95.0/std/hash/struct.DefaultHasher.html> — "This hasher is not guaranteed to be the same as all other DefaultHasher instances, but is the same as all other DefaultHasher instances created through new or default."
[^twox-portable]: docs.rs, twox-hash 2.1.5, Portability — <https://docs.rs/twox-hash/2.1.5/twox_hash/> — "The output does not depend on the platform; 32- and 64-bit systems produce the same output, as do little- and big-endian systems."
[^twox-hash-trait]: docs.rs, twox-hash 2.1.5, Portability — <https://docs.rs/twox-hash/2.1.5/twox_hash/> — "The Hash trait does not guarantee that implementors feed data into the Hasher in a platform-independent way."
[^siphasher-key]: docs.rs, siphasher 1.0.4 — <https://docs.rs/siphasher/1.0.4/siphasher/> — "let hasher = SipHasher13::new_with_key(key);"
[^siphasher-note]: docs.rs, siphasher 1.0.4, Note — <https://docs.rs/siphasher/1.0.4/siphasher/> — "The only safe methods in that trait are write() and finish()."
[^murmur3-crate]: docs.rs, murmur3 0.5.2 — <https://docs.rs/murmur3/0.5.2/murmur3/> — "A pure rust implementation of the fast, non-cryptographic hash murmur3"
[^twox-oneshot]: docs.rs, twox-hash 2.1.5 — <https://docs.rs/twox-hash/2.1.5/twox_hash/> — "let hash = XxHash64::oneshot(seed, b"some bytes");"
[^rdkafka-crate]: docs.rs, rdkafka 0.39.0 — <https://docs.rs/rdkafka/0.39.0/rdkafka/> — "A fully asynchronous, futures-enabled Apache Kafka client library for Rust based on librdkafka."
[^librdkafka-partitioner]: librdkafka v2.12.1, CONFIGURATION.md — <https://raw.githubusercontent.com/confluentinc/librdkafka/v2.12.1/CONFIGURATION.md> — "murmur2_random - Java Producer compatible Murmur2 hash of key (NULL keys are randomly partitioned. This is functionally equivalent to the default partitioner in the Java Producer.)"
[^rdkafka-record]: docs.rs, rdkafka 0.39.0, FutureRecord — <https://docs.rs/rdkafka/0.39.0/rdkafka/producer/struct.FutureRecord.html> — "partition: Option\<i32> Optional destination partition."
[^redis-crc-appendix]: Redis Cluster specification, Key distribution model — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "A reference implementation of the CRC16 algorithm used is available in the Appendix A of this document."
[^std-btree-range]: Rust 1.95.0 std, BTreeMap::range — <https://doc.rust-lang.org/1.95.0/std/collections/struct.BTreeMap.html> — "Constructs a double-ended iterator over a sub-range of elements in the map."
[^coll-btree-range]: Rust 1.95.0 std, module collections — <https://doc.rust-lang.org/1.95.0/std/collections/index.html> — "BTreeMap O(log(n)) O(log(n)) O(log(n)) N/A N/A O(log(n)) O(n+m)"
[^hashring-doc]: docs.rs, hashring 0.3.6 — <https://docs.rs/hashring/0.3.6/hashring/> — "HashRing’s API consists of three methods: add, remove, and get for adding a node to the ring, removing a node from the ring, and getting the node responsible for the provided key."
[^hashring-hasher]: docs.rs, hashring 0.3.6, HashRing — <https://docs.rs/hashring/0.3.6/hashring/struct.HashRing.html> — "Creates an empty HashRing which will use the given hash builder."
[^hashring-default]: docs.rs, hashring 0.3.6, DefaultHashBuilder — <https://docs.rs/hashring/0.3.6/hashring/struct.DefaultHashBuilder.html> — "impl BuildHasher for DefaultHashBuilder Source§ type Hasher = SipHasher"
[^ddia-toc-ch]: DDIA ấn bản 1, mục lục do ETH Library lưu, Part II Distributed Data — <https://toc.library.ethz.ch/objects/pdf03/z01_978-1-4493-7332-0_01.pdf> — "Partitioning and Replication Partitioning of Key-Value Data Partitioning by Key Range Partitioning by Hash of Key Skewed Workloads and Relieving Hot Spots"
[^ddia-toc-index]: DDIA ấn bản 1, mục lục do ETH Library lưu — <https://toc.library.ethz.ch/objects/pdf03/z01_978-1-4493-7332-0_01.pdf> — "Partitioning Secondary Indexes by Document Partitioning Secondary Indexes by Term"
[^ddia-toc-rebal]: DDIA ấn bản 1, mục lục do ETH Library lưu — <https://toc.library.ethz.ch/objects/pdf03/z01_978-1-4493-7332-0_01.pdf> — "Rebalancing Partitions Strategies for Rebalancing"
[^es-split-example]: Elasticsearch 8.15, Split index API — <https://www.elastic.co/guide/en/elasticsearch/reference/8.15/indices-split-index.html> — "number_of_routing_shards set to 30 (5 x 2 x 3) could be split by a factor of 2 or 3."
[^es-split-green]: Elasticsearch 8.15, Split index API — <https://www.elastic.co/guide/en/elasticsearch/reference/8.15/indices-split-index.html> — "The cluster health status must be green."
[^bt-paxos]: Bigtable paper (OSDI 2006), mục 4 Building Blocks — <https://research.google.com/archive/bigtable-osdi06.pdf> — "Chubby uses the Paxos algorithm [9, 23] to keep its replicas consistent in the face of failure."
[^zk-seq]: ZooKeeper 3.9.2, Overview — <https://zookeeper.apache.org/doc/r3.9.2/zookeeperOver.html> — "Sequential Consistency - Updates from a client will be applied in the order that they were sent."
[^mongo-reshard]: MongoDB 7.0, Choose a Shard Key — <https://www.mongodb.com/docs/v7.0/core/sharding-choose-a-shard-key/> — "Starting in MongoDB 5.0, you can change your shard key and redistribute your data using the reshardCollection command."
[^rdkafka-sys-dep]: rust-rdkafka v0.39.0, Cargo.toml — <https://raw.githubusercontent.com/fede1024/rust-rdkafka/v0.39.0/Cargo.toml> — "rdkafka-sys = { path = "rdkafka-sys", version = "4.10.0", default-features = false }"
[^rdkafka-sys-ver]: rust-rdkafka v0.39.0, rdkafka-sys/changelog.md — <https://raw.githubusercontent.com/fede1024/rust-rdkafka/v0.39.0/rdkafka-sys/changelog.md> — "v4.10.0+2.12.1 (2026-01-25) * Bump librdkafka to v2.12.1."
[^rdkafka-sys-readme]: rust-rdkafka v0.39.0, rdkafka-sys/README.md — <https://raw.githubusercontent.com/fede1024/rust-rdkafka/v0.39.0/rdkafka-sys/README.md> — "By default a submodule with the librdkafka sources will be used to compile and statically link the library."
[^rdkafka-sys-dyn]: rust-rdkafka v0.39.0, rdkafka-sys/README.md — <https://raw.githubusercontent.com/fede1024/rust-rdkafka/v0.39.0/rdkafka-sys/README.md> — "The system version of librdkafka must exactly match the version of librdkafka bundled with this crate."
[^librdkafka-default]: librdkafka v2.12.1, CONFIGURATION.md — <https://raw.githubusercontent.com/confluentinc/librdkafka/v2.12.1/CONFIGURATION.md> — "consistent_random - CRC32 hash of key (Empty and NULL keys are randomly partitioned)"
[^hashring-src]: docs.rs, hashring 0.3.6, src/lib.rs — <https://docs.rs/crate/hashring/0.3.6/source/src/lib.rs> — "impl BuildHasher for DefaultHashBuilder { type Hasher = SipHasher; fn build_hasher(&self) -> Self::Hasher { SipHasher::new() } }"
[^hashring-dep]: docs.rs, hashring 0.3.6, Cargo.toml — <https://docs.rs/crate/hashring/0.3.6/source/Cargo.toml> — "[dependencies.siphasher] version = "0.3.1""
[^siphasher-new]: docs.rs, siphasher 0.3.11, SipHasher::new — <https://docs.rs/siphasher/0.3.11/siphasher/sip/struct.SipHasher.html> — "Creates a new SipHasher with the two initial keys set to 0."
