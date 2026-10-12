# P12 · Chương 02 — Mô hình dữ liệu và ngôn ngữ truy vấn

> Chương này dùng *Designing Data-Intensive Applications* (DDIA) chỉ làm **khung chủ đề**: tên chương và tên mục của DDIA lấy từ mục lục do ETH Library lưu[^ddia-toc]; sách không có bản đọc tự do nên chương không trích và không viết lại nội dung sách. Mọi khẳng định dựa trên: bài báo gốc của Codd (1970), tài liệu **PostgreSQL 16**, tài liệu Cypher/Neo4j (của nhà cung cấp — phần so sánh hiệu năng được ghi rõ là khẳng định của họ), RFC 8259, hướng dẫn Hibernate ORM 5.1, tài liệu Serde, giáo trình mở BCcampus về dạng chuẩn, tutorial Apache Jena cho RDF/SPARQL, và docs.rs của `sqlx 0.8.6`, `tokio-postgres 0.7.13`, `serde_json 1.0.140`, `diesel 2.2.10` (các số phiên bản là bản người soạn chọn để đọc docs, không khẳng định là bản mới nhất). Trang W3C của SPARQL/RDF trả HTTP 403 cho công cụ kiểm trích dẫn nên không dùng làm nguồn.

## Bạn sẽ hiểu được gì

- **Q1.** Mô hình quan hệ của Codd (1970) giải quyết vấn đề gì, và "phụ thuộc dữ liệu" mà nó muốn loại bỏ là gì? → mục 1
- **Q2.** Document model và JSON/`jsonb` của PostgreSQL khác bảng quan hệ thế nào, được và mất gì? → mục 2
- **Q3.** Impedance mismatch là gì, và document model hay ORM giải quyết được đến đâu? → mục 3
- **Q4.** Quan hệ nhiều-nhiều, join và chuẩn hóa (normalization) liên quan với nhau ra sao; khi nào chấp nhận lặp dữ liệu? → mục 4
- **Q5.** Schema-on-write và schema-on-read khác nhau chỗ nào, và "không schema" có thật không? → mục 5
- **Q6.** Truy vấn khai báo (declarative, SQL) khác mệnh lệnh (imperative) ở đâu, và vì sao người ta gọi đó là lợi thế? → mục 6
- **Q7.** Graph model (property graph, Cypher) là gì, và recursive CTE của PostgreSQL làm được những gì của nó? → mục 7

## Kiến thức cần trước

- [Chương 01 — Reliability, scalability, maintainability](01-reliability-scalability-maintainability.md): khung "hệ thống dữ liệu" và các tiêu chí chọn công cụ; chương này là câu hỏi đầu tiên của khung đó: *dữ liệu được biểu diễn thế nào*.
- Biết SQL cơ bản (`SELECT`, `JOIN`, `GROUP BY`) và JSON.

Chủ đề liền kề (chỉ nhắc, không giải thích lại): cách các engine lưu và tìm dữ liệu trên đĩa ở [Chương 03 — Storage engine](03-storage-engine.md); định dạng mã hóa và tiến hóa schema ở [Chương 04](04-encoding-va-tien-hoa-schema.md); giao dịch ở [Chương 07](07-transaction.md); OLTP so với phân tích ở [Chương 13](13-kien-truc-he-du-lieu.md).

## 1. Mô hình quan hệ của Codd: loại bỏ phụ thuộc dữ liệu (Q1)

> **Trả lời ngắn.** Năm 1970, Codd đề xuất cho người dùng thấy dữ liệu chỉ qua các *relation* (tập hợp các bộ giá trị, tức bảng) mà không kèm cấu trúc phục vụ máy[^codd-natural]. Mục tiêu là *data independence*: chương trình ứng dụng không phụ thuộc cách dữ liệu được tổ chức bên dưới[^codd-indep]. Ông chỉ ra ba loại phụ thuộc cần loại bỏ ở các hệ thống đương thời: phụ thuộc thứ tự, phụ thuộc index và phụ thuộc đường truy cập[^codd-3dep].

**Giải thích.**

Hình dung (nhận xét của người soạn): bạn có một service đọc dữ liệu bằng cách đi theo con trỏ — "lấy đơn hàng, theo con trỏ `first_item`, rồi `next_item`…". Nếu DBA đổi cách móc nối các bản ghi, service hỏng dù dữ liệu vẫn đủ. Codd đang muốn một thế giới mà service chỉ nói *cần dữ liệu nào*, còn việc dữ liệu nằm ở đâu, móc nối ra sao là việc của hệ thống.

*Bối cảnh.* Bài báo nói rõ phạm vi: các vấn đề của nó là *data independence* — chương trình và thao tác ở terminal không bị ảnh hưởng khi dữ liệu thêm loại mới hoặc khi cách biểu diễn thay đổi — cùng một số loại bất nhất dữ liệu (*data inconsistency*)[^codd-scope]. Codd viết rằng góc nhìn quan hệ "có vẻ vượt trội ở nhiều mặt" (*appears to be superior in several respects*) so với mô hình graph/network đang thịnh hành thời đó cho các hệ thống không suy luận (*noninferential*)[^codd-superior].

*Ba loại phụ thuộc.* Codd liệt kê ba thứ còn lẫn trong cách người dùng tương tác với dữ liệu: ordering dependence, indexing dependence, access path dependence[^codd-3dep].

- **Phụ thuộc thứ tự:** chương trình ngầm giả định bản ghi nằm theo một thứ tự lưu; Codd nói chương trình dựa vào thứ tự lưu của file có thể chạy sai khi thứ tự đó phải đổi[^codd-order-fail]. Ông nhận xét rằng mọi hệ thống thông tin phổ biến được bán trên thị trường thời đó đều không phân biệt rõ *thứ tự trình bày* với *thứ tự lưu*[^codd-order].
- **Phụ thuộc index:** index vốn chỉ là thành phần phục vụ hiệu năng[^codd-index], nhưng nếu chương trình phải gọi chuỗi index theo tên thì khi chuỗi bị gỡ, chương trình chạy sai[^codd-chain].
- **Phụ thuộc đường truy cập:** với dữ liệu dạng cây hay network, chương trình phải dùng các đường truy cập do người dùng định nghĩa, nên hễ đường đó bị bỏ thì chương trình và thao tác terminal bị ảnh hưởng, bất kể biểu diễn lưu trữ ra sao[^codd-accesspath]. Giải pháp "không bao giờ bỏ đường cũ" bị Codd gạt đi vì số đường truy cập sẽ lớn quá mức[^codd-pathpolicy].

*Mô hình quan hệ trả lời thế nào.* Một relation là tập các n-tuple (bộ n phần tử)[^codd-reldef]; biểu diễn dạng mảng chỉ để minh họa[^codd-array]. Thứ tự hàng không có ý nghĩa và các hàng đều khác nhau[^codd-rel-props]. Dữ liệu của ngân hàng dữ liệu là tập các relation thay đổi theo thời gian: thêm, xóa, sửa các tuple[^codd-timevarying]. Quan hệ giữa các relation được biểu diễn bằng *khóa ngoại* (foreign key) — một domain mà giá trị là giá trị khóa chính của một relation nào đó[^codd-fk] (nhận xét của người soạn: tức là liên kết bằng giá trị chứ không bằng con trỏ lưu trữ). Codd còn nhận xét rằng khi đã cho phép khóa ngoại ở bất kỳ relation nào thì khó duy trì ranh giới giữa "mô tả thực thể" và "quan hệ giữa các thực thể"[^codd-nodist].

*Ngôn ngữ truy vấn.* Từ góc nhìn này, Codd cho rằng mô hình quan hệ cho phép phát triển một "data sublanguage" phổ quát dựa trên phép tính vị từ áp dụng (*applied predicate calculus*)[^codd-sublang], và ông xem nó là ứng viên mạnh để nhúng (kèm chỉnh sửa cú pháp) vào nhiều ngôn ngữ chủ[^codd-embed]; "tính phổ quát" (*universality*) của ngôn ngữ này nằm ở khả năng *mô tả* tập dữ liệu cần lấy, không phải khả năng tính toán[^codd-descr]. Đây là mầm của ý tưởng truy vấn khai báo ở mục 6 (nhận xét của người soạn).

```mermaid
flowchart LR
  subgraph Cu["Truy cập kiểu con trỏ (network/tree)"]
    A1["Chương trình"] -->|"đi theo đường truy cập đã định danh"| P1["bản ghi"]
    P1 --> P2["bản ghi"]
    P2 --> P3["bản ghi"]
  end
  subgraph Qh["Truy cập kiểu quan hệ"]
    A2["Chương trình"] -->|"mô tả tập dữ liệu cần"| R["relation(s)"]
  end
```

(Sơ đồ minh họa ý của Codd, do người soạn vẽ.)

<details><summary>Đi sâu: relation, khóa, domain; "thời gian" trong bài báo; PostgreSQL ánh xạ ra sao</summary>

- Codd định nghĩa relation theo nghĩa toán học: với các tập S1…Sn, R là một relation nếu nó là tập các n-tuple, mỗi tuple lấy phần tử thứ j từ Sj[^codd-reldef]; bài báo nói rõ dạng mảng chỉ là cách minh họa chứ không phải bản chất của mô hình[^codd-array]. (Nhận xét của người soạn: "hàng không có thứ tự, không trùng" khác với bảng SQL thực tế — bảng SQL cho phép hàng trùng nếu không có ràng buộc duy nhất; chương này chỉ nêu điều bài báo nói, không bàn chi tiết lệch giữa lý thuyết và SQL.)
- Khi hai cột cùng domain nhưng khác nghĩa (ví dụ `part` làm thành phần và `part` làm cụm lắp ráp), Codd yêu cầu gắn *role name* để phân biệt vị trí cột khỏi ý nghĩa[^codd-role]. (Nhận xét của người soạn: trong SQL ngày nay, việc đó tương ứng với đặt tên cột riêng — `sub_part`, `part` như ví dụ của PostgreSQL ở mục 7.)
- PostgreSQL biểu diễn "ràng buộc để dữ liệu không thể sai" bằng các constraint: kiểu dữ liệu quá thô cho nhiều ứng dụng nên cần thêm ràng buộc[^pg-constraints]. `CHECK` là loại tổng quát nhất[^pg-check]; khóa ngoại bảo đảm giá trị cột phải khớp một hàng của bảng khác, duy trì *referential integrity*[^pg-fk].

</details>

**Quay lại Q1:** Codd muốn ứng dụng nói *cần gì* và để hệ thống lo *lấy thế nào*; để đạt được, ông bỏ ba phụ thuộc — thứ tự, index, đường truy cập — khỏi giao diện người dùng.

## 2. Document model và JSON: đổi gì, được gì, mất gì (Q2)

> **Trả lời ngắn.** Document model lưu một thực thể dưới dạng một giá trị JSON lồng nhau, thay vì rải ra nhiều bảng. PostgreSQL có hai kiểu JSON: `json` giữ nguyên văn bản, còn `jsonb` lưu dạng nhị phân đã phân rã, xử lý nhanh hơn và index được[^pg-jsonb-vs-json]. Docs khuyên đa số ứng dụng dùng `jsonb`[^pg-prefer-jsonb] và vẫn nên giữ cho tài liệu có cấu trúc khá cố định[^pg-fixed].

**Giải thích.**

Hình dung (nhận xét của người soạn): hồ sơ ứng viên trong hệ thống tuyển dụng — họ tên, vài chục công ty đã làm, mỗi công ty vài dòng mô tả, danh sách kỹ năng. Trong mô hình quan hệ đó là bốn năm bảng nối bằng khóa ngoại; trong document model đó là *một cục JSON* đọc ra một lần, in ra một lần.

*JSON là gì, ở mức đủ dùng.* RFC 8259 định nghĩa JSON là định dạng trao đổi dữ liệu dạng văn bản, nhẹ, độc lập ngôn ngữ[^rfc-json]. Một object là các cặp tên/giá trị; tên trong một object *nên* (SHOULD) duy nhất[^rfc-unique] — tức đặc tả không cấm trùng tên; khi tên trùng, RFC nói hành vi của phần mềm nhận là không đoán trước được[^rfc-unpredictable]. (`json` và `jsonb` của PostgreSQL xử lý key trùng khác nhau — xem bảng dưới; cạm bẫy mã hóa JSON nói chung ở [Chương 04](04-encoding-va-tien-hoa-schema.md).)

*PostgreSQL có hai kiểu.* Cả `json` lẫn `jsonb` đều bắt mỗi giá trị lưu vào phải là JSON hợp lệ — ưu điểm so với lưu thành `text`[^pg-json-valid]. Khác biệt:

| | `json` | `jsonb` |
|---|---|---|
| Lưu | bản sao nguyên văn của đầu vào[^pg-json-exact] | dạng nhị phân đã phân rã[^pg-jsonb-vs-json] |
| Khoảng trắng, thứ tự key, key trùng | giữ cả ba[^pg-json-exact] | không giữ khoảng trắng, không giữ thứ tự key, bỏ key trùng[^pg-jsonb-nopreserve] |
| Nhập / xử lý | nhập nhanh hơn nhưng phải parse lại mỗi lần xử lý[^pg-jsonb-vs-json] | nhập chậm hơn chút, xử lý nhanh hơn rõ[^pg-jsonb-vs-json] |
| Index và containment | không có bộ chức năng containment tương đương[^pg-contain]; docs chỉ nêu việc hỗ trợ index như một lợi thế của `jsonb`[^pg-jsonb-vs-json] | có index GIN[^pg-gin] |

*Truy vấn trong tài liệu.* `jsonb` hỗ trợ toán tử *containment* `@>`: kiểm tra một tài liệu có chứa tài liệu khác không[^pg-contain]; subscripting kiểu mảng để đọc và cập nhật phần tử[^pg-subscript]; và index GIN để tìm key hoặc cặp key/value trong lượng lớn tài liệu[^pg-gin]. Lớp toán tử GIN mặc định hỗ trợ các toán tử tồn tại key `?`, `?|`, `?&`, containment `@>` và các toán tử khớp jsonpath `@?`, `@@`[^pg-gin-default]. Lưu ý một cạm bẫy của docs: index trên cột `jdoc` *không* được dùng cho biểu thức `jdoc -> 'tags' ? 'qui'`, vì toán tử `?` tuy index được nhưng không áp trực tiếp lên cột đã index[^pg-expr-notused]; docs nêu hai hướng: dùng expression index[^pg-expr-index] hoặc viết lại truy vấn bằng containment `@>`[^pg-contain-alt].

```sql
-- Minh họa (dựa theo ví dụ của docs PostgreSQL 16; chưa chạy trong chương này)
CREATE TABLE api (jdoc jsonb);
CREATE INDEX idxgin ON api USING GIN (jdoc);

-- Dùng được index: containment áp lên chính cột đã index
SELECT jdoc->'guid', jdoc->'name'
FROM api
WHERE jdoc @> '{"company": "Magnafone"}';
```

*Được gì.* Docs nói biểu diễn bằng JSON có thể linh hoạt hơn mô hình quan hệ truyền thống nhiều, hấp dẫn khi yêu cầu còn thay đổi[^pg-flex]; hai cách có thể cùng tồn tại trong một ứng dụng[^pg-flex]. Với dữ liệu "một thực thể tự chứa", một lần đọc một hàng đủ lấy hết (suy luận của người soạn).

*Mất gì.* Ba ràng buộc đáng nhớ, đều từ docs:

1. **Khóa cả hàng.** Mọi lần update khóa mức hàng (row-level lock) trên *toàn* hàng, nên tài liệu lớn gây tranh chấp khóa giữa các giao dịch cập nhật; docs khuyên mỗi tài liệu nên là một "datum nguyên tử" không chia nhỏ hợp lý được thành phần sửa độc lập[^pg-lock][^pg-atomic].
2. **Cấu trúc không được ép.** Cấu trúc JSON thường không bị enforced (dù có thể ép một số quy tắc nghiệp vụ theo cách khai báo)[^pg-fixed]. Đây chính là chủ đề schema-on-read ở mục 5.
3. **Số.** `jsonb` từ chối số ngoài khoảng của kiểu `numeric` của PostgreSQL còn `json` thì không[^pg-numeric]; docs còn lưu ý nhiều hệ khác biểu diễn số JSON bằng double IEEE 754, nên khi dùng JSON làm định dạng trao đổi với chúng cần cân nhắc nguy cơ mất độ chính xác số so với dữ liệu gốc trong PostgreSQL[^pg-numeric-loss]. (Nhận xét của người soạn: ID 64-bit và tiền tệ là hai nơi cần chú ý khi truyền qua JSON tới client như trình duyệt; mặt mã hóa của JSON xem [Chương 04](04-encoding-va-tien-hoa-schema.md).)

<details><summary>Đi sâu: tài liệu lớn nằm ở đâu trong PostgreSQL; hệ quả cho thiết kế</summary>

- PostgreSQL dùng kích thước trang cố định (thường 8 kB) và không cho tuple nằm trên nhiều trang; giá trị trường lớn được nén và/hoặc cắt thành nhiều hàng vật lý — kỹ thuật TOAST[^pg-toast].
- Kích thước logic của một giá trị thuộc kiểu TOAST-able bị giới hạn ở 1 GB[^pg-toast-1gb]. (Suy luận của người soạn: `jsonb` cũng thuộc diện này; docs trang JSON không nhắc, nên đừng thiết kế gần ngưỡng đó. Về mặt vận hành, tài liệu hàng MB đã đủ để khiến mỗi lần update một field nhỏ trở thành việc ghi lại cả tài liệu — xem lại lời khuyên "datum nguyên tử" ở trên.)
- Cơ chế TOAST và page là chi tiết engine; cách engine tổ chức trang và index ở [Chương 03](03-storage-engine.md).

</details>

**Quay lại Q2:** document model gói một thực thể vào một giá trị lồng nhau; `jsonb` cho phép index và truy vấn bên trong, đổi lại cấu trúc không được ép, update khóa cả hàng và số JSON có nguy cơ mất chính xác.

## 3. Impedance mismatch: lệch pha giữa object và bảng (Q3)

> **Trả lời ngắn.** Mã ứng dụng thường nghĩ bằng đối tượng lồng nhau, còn cơ sở dữ liệu quan hệ lưu bằng bảng phẳng; việc dịch qua lại tốn công (nhận xét của người soạn). Hướng dẫn Hibernate (một ORM cho Java) viết rằng làm phần mềm hướng đối tượng với dữ liệu từ CSDL quan hệ có thể "cumbersome and resource consuming", và chi phí phát triển cao hơn đáng kể vì "paradigm mismatch" giữa cách dữ liệu được biểu diễn trong đối tượng và trong CSDL quan hệ[^hib-mismatch]. "Impedance mismatch" là cách gọi phổ biến của hiện tượng này (nhận xét của người soạn: chương không trích nguồn cho tên gọi đó; nguồn trích dùng "paradigm mismatch"). Document model giảm bớt phần lệch cho dữ liệu dạng cây nhưng không làm nó biến mất, còn ORM có thể sinh ra loại lỗi riêng như N+1 (nhận xét của người soạn; N+1 do Diesel nêu ở dưới[^diesel-n1]).

**Giải thích.**

Hình dung (nhận xét của người soạn): struct Rust `Profile { name, positions: Vec<Position>, skills: Vec<String> }`. Để lưu vào quan hệ, bạn tách nó thành `profiles`, `positions`, `skills` (hoặc bảng nối); để đọc lại, bạn phải gom các hàng về đúng struct. Mỗi chiều đều là code phải viết và test.

*Ba chỗ lệch cụ thể* (nhận xét của người soạn, minh họa bằng nguồn ở dưới):

1. **Lồng nhau so với phẳng.** Codd xử lý chuyện này từ 1970: relation có thể có domain *không đơn* (nonsimple), tức phần tử là relation khác — ví dụ lịch sử lương của nhân viên[^codd-nonsimple]; Codd nói thuật ngữ "repeating group" của thời đó tương đương gần đúng (*roughly analogous*) với nonsimple domain[^codd-repgroup]. Ông đề xuất loại bỏ chúng bằng *normalization* (mục 4). Tức là mô hình quan hệ chủ động ép dữ liệu lồng thành các bảng phẳng; document model đi ngược lại, cho phép lồng.
2. **NULL so với `Option`.** Một cột có thể NULL, nên field Rust tương ứng thường phải là `Option<T>` (nhận xét của người soạn). Về phía tham số truy vấn, docs của `sqlx` ghi: với kiểu mong đợi `T`, cả `T` lẫn `Option<T>` đều được phép khi bind, và `Option::None` được bind thành NULL[^sqlx-nullable].
3. **Quan hệ so với con trỏ/vector.** Trong code, `user.posts` là một `Vec`. Trong CSDL, đó là một truy vấn qua khóa ngoại. Diesel nhận xét thiết kế "has many" trong nhiều ORM khác dẫn tới nhiều vấn đề, gồm lỗi N+1 query và lỗi lúc chạy khi truy cập một association không tồn tại[^diesel-n1]; vì vậy Diesel tách dữ liệu và association thành hai thứ riêng[^diesel-separate].

*Document model giúp gì và không giúp gì.* Với dữ liệu có hình cây tự chứa (hồ sơ, giỏ hàng, cấu hình), lưu nguyên cây vào một cột `jsonb` loại bỏ bước tách/gom: serde (mục Rust) đã làm việc chuyển giữa struct và JSON. Nhưng:

- Khi dữ liệu có tham chiếu chéo (nhiều hồ sơ cùng nhắc tới một công ty), cây không còn đủ: cần tham chiếu hoặc lặp dữ liệu — vào mục 4.
- Lược đồ chuyển từ cơ sở dữ liệu sang *mã đọc*: mọi nơi đọc phải hiểu hình dạng cũ lẫn mới (mục 5).

<details><summary>Đi sâu: ORM không phải lỗi, mismatch cũng không phải của riêng quan hệ</summary>

- Hibernate mô tả ORM là kỹ thuật ánh xạ dữ liệu giữa biểu diễn mô hình đối tượng và biểu diễn mô hình quan hệ, cho môi trường Java[^hib-mismatch]. Không phải mọi hệ sinh thái làm giống nhau: Rust có `sqlx` (ánh xạ hàng → struct qua `FromRow`, xem mục Rust) và Diesel — Diesel tự mô tả là "ORM and query builder"[^diesel-desc] nhưng khác nhiều ORM ở chỗ không có khái niệm `has many`, association luôn từ con tới cha[^diesel-childparent].
- Mismatch còn ở các tầng khác: mỗi lần đổi biểu diễn (struct ↔ hàng ↔ JSON ↔ protobuf) là một điểm có thể sai. Chương 04 nói thêm về tầng mã hóa.

</details>

**Quay lại Q3:** mismatch là chi phí dịch giữa mô hình đối tượng lồng nhau trong code và bảng phẳng trong CSDL; JSON/document giảm chi phí cho dữ liệu dạng cây tự chứa nhưng không xóa nó khi có tham chiếu chéo.

## 4. Nhiều-nhiều, join và normalization (Q4)

> **Trả lời ngắn.** Khi hai loại thực thể liên hệ nhiều-nhiều (đơn hàng ↔ sản phẩm), mô hình quan hệ dùng một bảng nối có hai khóa ngoại[^pg-m2m]. Dữ liệu của một câu hỏi nằm ở nhiều bảng được ghép lại bằng *join*[^pg-join-def]. *Normalization* ban đầu của Codd là cách loại bỏ dữ liệu lồng bằng cách tách thành các relation phẳng, mỗi cái mang khóa của cha[^codd-norm]; lặp dữ liệu có chủ ý (denormalize) đổi tốn chỗ và công cập nhật lấy tốc độ đọc[^codd-redundancy] (Codd nói về dư thừa trong biểu diễn lưu trữ và chỉ nói *có thể* giảm thời gian truy vấn; gọi đó là denormalize là diễn giải của người soạn).

**Giải thích.**

Hình dung (nhận xét của người soạn): hóa đơn trên bàn thu ngân ghi tên hàng, giá, tên khách. Nếu mỗi dòng của hóa đơn chép lại *đủ* tên và địa chỉ khách, thì khi khách đổi địa chỉ bạn phải sửa hàng nghìn tờ. Chuẩn hóa nghĩa là "mỗi sự thật nằm ở đúng một chỗ, nơi khác chỉ trỏ tới".

*Bảng nối cho quan hệ nhiều-nhiều.* Ví dụ chính thức của PostgreSQL: `products`, `orders` và `order_items` có hai khóa ngoại và khóa chính ghép; docs nói một bảng có thể có nhiều khóa ngoại để biểu diễn quan hệ nhiều-nhiều[^pg-m2m]. Vì khóa ngoại cần xử lý khi xóa, ví dụ cho `order_items` chọn: không cho xóa sản phẩm còn được tham chiếu (`ON DELETE RESTRICT`)[^pg-restrict], xóa đơn thì xóa luôn các dòng của nó (`ON DELETE CASCADE`)[^pg-cascade]. Docs cũng nhắc: khai báo khóa ngoại *không* tự tạo index trên cột tham chiếu, trong khi xóa/sửa hàng bị tham chiếu phải quét bảng tham chiếu, nên thường nên tự index các cột đó[^pg-fk-index].

```sql
-- Dựa theo ví dụ trong docs PostgreSQL 16, mục 5.4.5 (minh họa, chưa chạy)
CREATE TABLE products (product_no integer PRIMARY KEY, name text, price numeric);
CREATE TABLE orders   (order_id integer PRIMARY KEY, shipping_address text);
CREATE TABLE order_items (
    product_no integer REFERENCES products ON DELETE RESTRICT,
    order_id   integer REFERENCES orders   ON DELETE CASCADE,
    quantity   integer,
    PRIMARY KEY (product_no, order_id)
);
```

```mermaid
erDiagram
  ORDERS ||--o{ ORDER_ITEMS : "có"
  PRODUCTS ||--o{ ORDER_ITEMS : "xuất hiện trong"
  ORDERS { int order_id PK }
  PRODUCTS { int product_no PK }
  ORDER_ITEMS { int order_id FK
                int product_no FK
                int quantity }
```

(Sơ đồ ER do người soạn vẽ từ ví dụ SQL ở trên.)

*Join.* Truy vấn đụng nhiều bảng (hoặc nhiều "bản" của cùng một bảng) gọi là join query[^pg-join-def]. Ví dụ `LEFT OUTER JOIN` giữ các hàng bên trái dù không có hàng khớp, điền giá trị rỗng cho cột bên phải[^pg-outer]. Codd định nghĩa *natural join* của hai quan hệ nhị phân R(a,b), S(b,c) là tập các bộ (a,b,c) mà R(a,b) và S(b,c) cùng đúng[^codd-join]. Join là phép *khai báo*: người viết nói "ghép theo điều kiện này", còn chọn thuật toán là việc của planner[^pg-planner-ways]. PostgreSQL có ba chiến lược join: nested loop (quét bảng phải một lần cho mỗi hàng bảng trái)[^pg-nl], merge join (sắp xếp hai bên theo khóa join rồi quét song song)[^pg-merge] và hash join (nạp bảng phải vào hash table rồi dò từng hàng bảng trái)[^pg-hash]. Nghĩa là join *nhiều bảng* không phải là một phép tính cố định mà là bài toán tối ưu: planner xét các thứ tự join khác nhau để tìm phương án rẻ nhất[^pg-order].

*Chuẩn hóa theo Codd.* Bài báo mô tả thủ tục cụ thể: bắt đầu từ relation gốc của cây, lấy khóa chính của nó chèn vào các relation con trực tiếp, khóa chính của mỗi relation con là khóa cũ cộng với khóa copy từ cha; sau đó gạch bỏ các domain không đơn khỏi relation cha, bỏ nút gốc và lặp lại cho từng cây con còn lại[^codd-normproc]. Ví dụ của ông: `employee(man#, name, birthdate, jobhistory, children)` với `jobhistory` và `children` lồng, trở thành `employee'`, `jobhistory'(man#, jobdate, title)`, `salaryhistory'(man#, jobdate, salarydate, salary)` và `children'(man#, childname, birthyear)`[^codd-fig3]. Kết quả là mọi relation đều có domain đơn, biểu diễn được bằng mảng hai chiều đồng nhất[^codd-simple]. Về tên gọi sau này: một giáo trình mở liệt kê các dạng chuẩn 1NF, 2NF, 3NF và BCNF[^bc-nf], trong đó 1NF là dạng chỉ cho phép giá trị đơn ở mỗi giao điểm hàng–cột, tức "không có repeating group"[^bc-1nf]; lý thuyết chuẩn hóa dựa nhiều vào lý thuyết phụ thuộc hàm[^bc-fd]. (Suy luận của người soạn: bước normalization của Codd 1970 tương ứng 1NF của giáo trình đó; chương này không đi vào 2NF/3NF/BCNF.)

*Dư thừa và nhất quán, theo Codd.* Codd cũng phân tích dư thừa (redundancy). Một tập relation là *strongly redundant* nếu chứa relation có một phép chiếu tính được từ phép chiếu của các relation khác[^codd-strong]. Với dư thừa mạnh được phản ánh xuống lưu trữ, nói chung tốn thêm chỗ và thời gian cập nhật, đổi lấy khả năng giảm thời gian truy vấn và tải CPU ở một số truy vấn[^codd-redundancy]. Khi tập relation dư thừa (theo một trong hai nghĩa của ông), Codd gắn với nó một tập câu lệnh định nghĩa mọi dư thừa luôn đúng giữa các relation[^codd-constraints]; trạng thái là nhất quán hay không tùy giá trị hiện tại có thỏa các ràng buộc đó không[^codd-consistent]. Đây là ngôn ngữ 1970 của chuyện mà thợ gọi là *update anomaly* (suy luận của người soạn: khi dư thừa không được hệ thống biết đến, mỗi nơi lặp là một chỗ có thể sửa lệch).

*Khi nào document model và normalization đối đầu.*

- **Document phù hợp** khi dữ liệu tự chứa, đọc theo từng cụm, ít tham chiếu chéo (nhận xét của người soạn).
- **Normalize phù hợp** khi cùng một thực thể được nhiều nơi tham chiếu và cần sửa một chỗ — ví dụ tên công ty trong hồ sơ của nhiều ứng viên.
- **Join phía ứng dụng.** Nếu CSDL không join, mã ứng dụng làm: Diesel ví dụ nạp dữ liệu liên quan bằng nhiều truy vấn, mỗi bảng một truy vấn[^diesel-multi], và nhận xét rằng với đa số tập dữ liệu, join một phát truyền quá nhiều dữ liệu lặp nên có thể đắt hơn một vòng round-trip thêm[^diesel-join-cost]. Đó là nhận xét của dự án Diesel, không phải quy luật chung — kết quả phụ thuộc kích thước và hình dạng dữ liệu.

<details><summary>Đi sâu: các số về chi phí join, và lời quảng cáo của graph database</summary>

- Docs Neo4j (tài liệu của nhà cung cấp graph database) nói join được tính lúc truy vấn bằng cách khớp khóa chính/khóa ngoại của mọi hàng trong các bảng liên quan, các phép này "compute-heavy and memory-intensive" và "have an exponential cost"; trang đó cũng nói quan hệ nhiều-nhiều cần một bảng nối chứa khóa của cả hai bảng, làm tăng thêm chi phí join[^neo-join-table]. Phần "chi phí hàm mũ" là khẳng định của nhà cung cấp: đoạn trích không nêu chi phí tăng theo biến nào (số bảng hay kích thước dữ liệu) và chương này không có nguồn trung lập để xác nhận; docs PostgreSQL cho thấy planner có hash join và merge join ngoài nested loop (mục trên). Nhận xét của người soạn: chi phí thực phụ thuộc kế hoạch và dữ liệu, nên đo bằng `EXPLAIN` trên dữ liệu của bạn.
- `EXPLAIN` hiển thị kế hoạch mà planner chọn; docs PostgreSQL nhấn mạnh chọn đúng kế hoạch là tối quan trọng cho hiệu năng, vì vậy hệ có một planner phức tạp[^pg-explain].
- Khi số bảng join vượt ngưỡng `geqo_threshold`, PostgreSQL chuyển sang tìm thứ tự join bằng phương pháp heuristic thay vì gần-vét-cạn[^pg-geqo].
- Khi dữ liệu chia qua nhiều máy, join và giao dịch xuyên partition trở thành bài toán phân tán: xem [Chương 06 — Partitioning](06-partitioning.md); giao dịch cập nhật nhiều hàng/bảng là chuyện của [Chương 07](07-transaction.md).

</details>

**Quay lại Q4:** nhiều-nhiều biểu diễn bằng bảng nối với hai khóa ngoại, join ghép lại lúc đọc, normalization giữ mỗi sự thật ở một chỗ; denormalize là đổi chỗ và công cập nhật lấy tốc độ đọc, nên chọn có chủ ý.

## 5. Schema-on-write và schema-on-read (Q5)

> **Trả lời ngắn.** (Định nghĩa làm việc của người soạn; hai thuật ngữ này không đến từ nguồn trích dẫn trong chương.) *Schema-on-write* là cơ sở dữ liệu kiểm tra và từ chối dữ liệu sai hình dạng ngay lúc ghi (kiểu, `NOT NULL`, `CHECK`, khóa ngoại). *Schema-on-read* là hệ thống chấp nhận mọi hình dạng lúc ghi, mã đọc tự diễn giải lúc đọc. Không có cách nào "không schema": schema chỉ chuyển từ cơ sở dữ liệu sang mã của bạn (nhận xét của người soạn).

**Giải thích.**

Hình dung (nhận xét của người soạn): schema-on-write giống kiểm tra vé ở cổng vào rạp, ai sai vé bị chặn ngay. Schema-on-read giống rạp mở cửa tự do, nhưng khi vào trong, mỗi nhân viên soát khán giả theo cách riêng — và nếu một người quên kiểm một loại vé lạ, lỗi lộ ra rất muộn.

*Schema-on-write trong PostgreSQL.* Kiểu dữ liệu giới hạn loại dữ liệu lưu được nhưng với nhiều ứng dụng thế là quá thô; các constraint bổ sung phần còn lại[^pg-constraints]. `CHECK` là loại tổng quát nhất[^pg-check]. Khóa ngoại bảo đảm tính toàn vẹn tham chiếu[^pg-fk]. Giá phải trả: đổi schema là một thao tác có chi phí. Chẳng hạn `ADD COLUMN` với giá trị mặc định không-volatile chỉ lưu giá trị đó vào metadata của bảng, không cần ghi lại bảng[^pg-addcol]; còn thêm cột với default volatile hay đổi kiểu cột thì nói chung phải ghi lại toàn bộ bảng cùng index (có ngoại lệ cho đổi kiểu, xem mục Đi sâu)[^pg-rewrite]; `SET NOT NULL` thường phải quét toàn bảng để kiểm tra[^pg-setnotnull].

*Schema-on-read với JSON.* Cột `jsonb` chấp nhận mọi JSON hợp lệ; docs nói cấu trúc thường không bị ép buộc, đồng thời vẫn khuyên giữ cấu trúc khá cố định để truy vấn tổng hợp dễ viết[^pg-fixed]. Neo4j cũng tự mô tả là "schema-flexible": dù docs nói có thể và nên ép schema từng phần bằng index và constraint[^neo-partial], người dùng không bắt buộc dùng một schema cố định và có thể thêm thuộc tính, quan hệ mới khi đồ thị tiến hóa[^neo-flex].

*Schema nằm ở mã.* Trong Rust, đọc JSON "lỏng" nghĩa là chọn một trong hai: giữ nguyên `serde_json::Value` (không có kiểu) hay giải mã vào struct có kiểu. `serde_json` nêu ba cách làm việc với JSON — văn bản thô[^sj-text], biểu diễn không kiểu hoặc kiểu lỏng, và cấu trúc dữ liệu Rust có kiểu cho trường hợp bạn kỳ vọng dữ liệu đa phần khớp một cấu trúc[^sj-three]. Mặc định, `serde` bỏ qua field lạ khi giải mã định dạng tự mô tả như JSON, trừ khi dùng `#[serde(deny_unknown_fields)]`[^serde-deny]; field thiếu có thể được điền bằng `Default` nhờ `#[serde(default)]`[^serde-default]; và `#[serde(alias = "…")]` cho phép đọc một field từ nhiều tên[^serde-alias]. Ba thuộc tính này chính là các công cụ viết bộ đọc chịu được nhiều "đời" của dữ liệu — schema-on-read thực hiện bằng code (nhận xét của người soạn).

```rust
// Minh họa (chưa biên dịch trong chương này): bộ đọc chịu được hai đời dữ liệu.
use serde::Deserialize;

#[derive(Deserialize)]
struct Profile {
    name: String,
    // Đời cũ lưu "tel", đời mới lưu "phone": đọc được cả hai.
    #[serde(alias = "tel")]
    phone: Option<String>,
    // Đời cũ không có field này: dùng Default (Vec rỗng).
    #[serde(default)]
    skills: Vec<String>,
}
```

*Khi nào chọn cái nào* (nhận xét của người soạn): schema-on-write hợp khi nhiều dịch vụ cùng ghi vào một chỗ và cần một nguồn sự thật về hình dạng; schema-on-read hợp khi dữ liệu đến từ nguồn ngoài hay hình dạng thay đổi nhanh, và bạn chấp nhận gánh việc xử lý nhiều phiên bản trong mã. Cách mã hóa và tương thích tiến/lùi giữa các phiên bản (Avro, Protobuf…) được bàn ở [Chương 04](04-encoding-va-tien-hoa-schema.md).

<details><summary>Đi sâu: chi phí ALTER TABLE và ràng buộc trên jsonb</summary>

- Docs `ALTER TABLE` bổ sung: đổi kiểu cột mà `USING` không đổi nội dung và kiểu cũ tương thích nhị phân với kiểu mới thì không cần ghi lại bảng, nhưng index vẫn phải dựng lại trừ khi hệ thống kiểm chứng được index mới tương đương logic với index cũ[^pg-rewrite]. Đây là chi tiết cần đọc trước khi migrate bảng lớn (nhận xét của người soạn).
- Nếu có `CHECK` đã được xác thực chứng minh không có NULL, `SET NOT NULL` bỏ qua bước quét bảng[^pg-setnotnull].
- Có thể ép một số quy tắc lên `jsonb` bằng `CHECK` (docs nói việc ép quy tắc nghiệp vụ theo cách khai báo là có thể[^pg-fixed]); chương này không trình bày cú pháp cụ thể vì docs trang JSON không đưa ví dụ.

</details>

**Quay lại Q5:** schema-on-write đẩy kiểm tra về phía ghi (tốn chi phí khi đổi schema), schema-on-read đẩy về phía đọc (tốn code xử lý nhiều đời dữ liệu); schema luôn tồn tại, chỉ là nằm ở CSDL hay ở mã.

## 6. Truy vấn khai báo và mệnh lệnh (Q6)

> **Trả lời ngắn.** Truy vấn *khai báo* mô tả *kết quả cần có*; hệ thống tự chọn cách tính. Truy vấn *mệnh lệnh* ghi từng bước *làm thế nào* (hai định nghĩa là cách diễn đạt của người soạn). Docs Neo4j nói Cypher, giống SQL, cho người dùng tập trung vào *lấy gì* thay vì *lấy thế nào*[^cy-what]; ở PostgreSQL, một truy vấn SQL có thể thực thi theo rất nhiều cách cho cùng một kết quả và planner chọn kế hoạch được ước lượng nhanh nhất[^pg-planner-ways]. Cypher cũng tự mô tả là khai báo[^cy-declarative]. Lợi ích chính — cùng truy vấn vẫn chạy khi cách lưu và index đổi — là nhận xét của người soạn, nối từ mục tiêu data independence của Codd (mục 1).

**Giải thích.**

Hình dung (nhận xét của người soạn): gọi taxi. Khai báo: "đến sân bay" — tài xế tự chọn đường, tránh kẹt. Mệnh lệnh: "rẽ phải ở ngã tư 3, đi thẳng 2 km…" — nhanh khi bạn biết hết đường, nhưng đường thay đổi thì lệnh của bạn sai.

*Từ Codd tới SQL.* Cái Codd gọi là "tính phổ quát của ngôn ngữ nằm ở khả năng mô tả" (mục 1) là mầm của ý này (nhận xét của người soạn)[^codd-descr]. Codd còn mô tả cái giá của việc phụ thuộc vào đường truy cập: chương trình theo access path hỏng khi cấu trúc đổi[^codd-accesspath], và chương trình gọi chuỗi index theo tên hỏng khi chuỗi bị gỡ[^codd-chain]; việc nối hai điểm đó với "kiểu mệnh lệnh" là nhận xét của người soạn.

*Một truy vấn, nhiều kế hoạch.* Docs PostgreSQL nói rõ một truy vấn có thể được thực thi theo nhiều cách khác nhau, cho cùng tập kết quả; nếu khả thi về mặt tính toán, planner xét các kế hoạch đó và chọn kế hoạch được ước lượng là chạy nhanh nhất[^pg-planner-ways]. Kế hoạch gồm quét tuần tự hoặc quét index trên bảng gốc, các nút join (nested loop, merge, hash) và các bước phụ như sort hay aggregate[^pg-plan-tree]. Người dùng xem kế hoạch bằng `EXPLAIN`[^pg-explain]. Hệ quả thực tế (nhận xét của người soạn): thêm index hay đổi thống kê có thể làm cùng câu `SELECT` chạy theo kế hoạch khác mà không phải sửa code; còn nếu kế hoạch được chọn tệ, công cụ để can thiệp là index, thống kê, và viết lại truy vấn — không phải sửa vòng lặp.

*Mệnh lệnh ở đâu trong hệ thống backend?* Mọi nơi bạn tự viết vòng lặp để ghép dữ liệu. Ví dụ N+1: lấy danh sách rồi cho mỗi phần tử gọi một truy vấn riêng, tức là bạn tự làm nested loop ở tầng ứng dụng, mất cơ hội cho planner chọn hash join (suy luận của người soạn). Diesel đề cập đúng nguy cơ này ở các ORM có `has many`[^diesel-n1].

*Các ngôn ngữ khai báo khác.* Cypher mô tả kết quả bằng pattern; docs nói nó cho người dùng tập trung vào *lấy gì* thay vì *lấy thế nào*[^cy-what]. Chi tiết ở mục 7.

*Khai báo không phải miễn phí.* Planner có thể chọn sai khi thống kê lệch (nhận xét của người soạn). Docs PostgreSQL nhấn mạnh chọn đúng kế hoạch rất quan trọng cho hiệu năng, nên hệ thống mới có một planner phức tạp[^pg-explain]. Cái đổi được: bạn bớt quản lý đường truy cập, thêm việc đọc kế hoạch.

<details><summary>Đi sâu: nhận xét về thứ tự đánh giá và "khai báo" ở tầng khác</summary>

- Truy vấn đệ quy trong PostgreSQL *viết* đệ quy nhưng *chạy* bằng vòng lặp: docs ghi chú các truy vấn như vậy được đánh giá lặp bên trong[^pg-rec-iter]. Và docs cảnh báo thứ tự duyệt hàng là phụ thuộc cài đặt; muốn thứ tự phải tính cột sắp xếp rồi `ORDER BY`[^pg-search-order] — theo nhận xét của người soạn, đó là một minh họa rằng với ngôn ngữ khai báo, thứ tự thực thi không phải điều bạn được hứa.
- Không phải mọi cách xử lý dữ liệu đều khai báo (nhận xét của người soạn): Chương 10 trích Blanas et al. ghi nhận nhóm phê bình MapReduce chỉ ra việc nó thiếu ngôn ngữ truy vấn khai báo — xem [Chương 10](10-batch-processing.md); xử lý luồng ở [Chương 11](11-stream-processing.md).
- Ở Rust, builder như Diesel mô hình hóa SQL thành các method trùng tên mệnh đề (`filter` ứng với `WHERE`)[^diesel-builder], tức ngôn ngữ truy vấn được nhúng vào hệ kiểu của Rust (nhận xét của người soạn).

</details>

**Quay lại Q6:** truy vấn khai báo nói cần gì và để planner chọn cách; bạn đổi lại quyền kiểm soát từng bước lấy khả năng thay index, thống kê, thuật toán mà không sửa truy vấn.

## 7. Graph model, Cypher và recursive CTE (Q7)

> **Trả lời ngắn.** Graph model biểu diễn dữ liệu là *đỉnh* và *cạnh*. Trong property graph của Neo4j, đỉnh có nhãn và thuộc tính; cạnh có hướng, có đúng một kiểu và có thuộc tính[^neo-pg]. Cypher truy vấn bằng pattern. Với PostgreSQL, truy vấn đệ quy `WITH RECURSIVE` cho phép duyệt cấu trúc cây hoặc đồ thị trong SQL thuần[^pg-rec-def][^pg-rec-use].

**Giải thích.**

Hình dung (nhận xét của người soạn): mạng bạn bè, lộ trình tàu, cây thư mục, "ai là cấp trên của cấp trên của tôi". Điểm chung: số bước nhảy giữa hai thực thể *không biết trước*. Với bảng và join cố định bạn phải biết trước bao nhiêu join; đồ thị cho phép hỏi "đi bao xa cũng được".

*Property graph.* Neo4j dùng property graph database model gồm: *node* biểu diễn thực thể, có thể có không hoặc nhiều nhãn (label) phân loại; *relationship* nối một node nguồn tới một node đích, luôn có một hướng và đúng một kiểu; cả node lẫn relationship đều có thể có property dạng key-value[^neo-pg]. Ví dụ của docs tạo một diễn viên, một phim, và hai quan hệ `ACTED_IN`, `DIRECTED` chỉ bằng một lệnh `CREATE`[^neo-create].

```mermaid
graph LR
  T["(:Person:Actor)<br/>name: Tom Hanks"] -->|"ACTED_IN {roles}"| M["(:Movie)<br/>title: Forrest Gump"]
  R["(:Person)<br/>name: Robert Zemeckis"] -->|DIRECTED| M
```

(Sơ đồ dựa theo ví dụ `CREATE` của docs Neo4j[^neo-create]; người soạn vẽ lại và lược bớt thuộc tính.)

*Cypher.* Là ngôn ngữ truy vấn khai báo của Neo4j, tạo năm 2011 như "SQL cho graph database"[^cy-declarative][^cy-2011]. Cú pháp "vẽ" pattern: node trong ngoặc tròn, quan hệ là mũi tên kèm ngoặc vuông[^cy-syntax], nên viết truy vấn là "vẽ một đường đi qua dữ liệu"[^cy-ascii]. Khác SQL: Cypher kết thúc bằng `RETURN` thay vì bắt đầu bằng `SELECT`[^cy-order].

```cypher
// Minh họa (chưa chạy): bộ phim mà Tom Hanks đóng
MATCH (p:Person {name: 'Tom Hanks'})-[:ACTED_IN]->(m:Movie)
RETURN m.title
```

*Đường đi có độ dài biến thiên.* Cypher bản trước 5.9 chỉ có *variable-length relationship* để khớp đường đi dài không biết trước; từ 5.9 có *quantified path pattern* và quantified relationship, ví dụ `-[:NEXT]->{1,10}`[^cy-qpp]. Docs cũng ghi một quy tắc dễ bỏ sót: trong một pattern, mỗi relationship chỉ được khớp một lần[^cy-unique] — nên vòng lặp không làm duyệt lặp vô hạn trong một pattern (suy luận của người soạn từ quy tắc đó).

*Recursive CTE: duyệt đồ thị trong SQL.* Docs PostgreSQL: từ khóa `RECURSIVE` làm `WITH` thành tính năng làm được việc mà SQL chuẩn không làm được — truy vấn `WITH` có thể tham chiếu chính kết quả của nó[^pg-rec-def]. Dạng tổng quát: một *non-recursive term*, rồi `UNION` (hoặc `UNION ALL`), rồi một *recursive term* — chỉ term đệ quy được tham chiếu kết quả của chính truy vấn[^pg-rec-form]. Docs giải thích đây là cách xử lý dữ liệu phân cấp hoặc dạng cây[^pg-rec-use].

Cơ chế chạy (tóm tắt thủ tục của docs): tính term không đệ quy, đưa kết quả vào kết quả chung và vào bảng làm việc (working table). Trong khi bảng làm việc còn hàng, tính term đệ quy trên nội dung bảng làm việc hiện tại, thêm kết quả vào kết quả chung và vào bảng trung gian, rồi thay nội dung bảng làm việc bằng bảng trung gian và làm rỗng bảng trung gian[^pg-rec-algo]. Với `UNION` (khác `UNION ALL`), hàng trùng bị loại[^pg-rec-algo].

```mermaid
flowchart TD
  A["Tính term không đệ quy"] --> B["Đưa vào kết quả + working table"]
  B --> C{"working table còn hàng?"}
  C -- "có" --> D["Tính term đệ quy trên working table"]
  D --> E["Thêm vào kết quả + bảng trung gian"]
  E --> F["working table := bảng trung gian; làm rỗng bảng trung gian"]
  F --> C
  C -- "không" --> G["Kết thúc"]
```

(Sơ đồ tóm tắt thủ tục của docs ở trên, do người soạn vẽ.)

Ví dụ chính thức của docs: cho bảng `parts` chỉ ghi quan hệ "chứa trực tiếp", tìm mọi bộ phận con trực tiếp lẫn gián tiếp của một sản phẩm và tổng số lượng[^pg-rec-parts]. Đây cũng là bài toán "parts explosion" mà Codd đã dùng làm ví dụ cho relation `component` năm 1970[^codd-parts].

```sql
-- Dựa theo ví dụ của docs PostgreSQL 16, mục 7.8.2 (minh họa, chưa chạy)
WITH RECURSIVE included_parts(sub_part, part, quantity) AS (
    SELECT sub_part, part, quantity FROM parts WHERE part = 'our_product'
  UNION ALL
    SELECT p.sub_part, p.part, p.quantity * pr.quantity
    FROM included_parts pr, parts p
    WHERE p.part = pr.sub_part
)
SELECT sub_part, SUM(quantity) AS total_quantity
FROM included_parts
GROUP BY sub_part;
```

*Vòng lặp (cycle).* Nếu dữ liệu có chu trình, truy vấn đệ quy lặp mãi: docs nói truy vấn mẫu sẽ lặp nếu quan hệ `link` có vòng[^pg-cycle-loop]. Cách chuẩn là tính một mảng các giá trị đã thăm[^pg-cycle-array]; PostgreSQL 16 còn có cú pháp dựng sẵn, `CYCLE … SET … USING …`, được viết lại nội bộ thành dạng thủ công đó[^pg-cycle-builtin][^pg-cycle-rewrite].

*Khi nào chọn graph database, khi nào chỉ cần SQL* (nhận xét của người soạn): nếu chỉ cần một hai cấp duyệt hay cây phân cấp, join hoặc recursive CTE đủ và bạn giữ nguyên hệ sinh thái giao dịch, backup, quyền; nếu truy vấn trung tâm là "đường đi / mẫu con trong đồ thị lớn" với độ dài biến thiên và nhiều kiểu quan hệ, graph model gần với bài toán hơn. Docs của Neo4j mô tả lợi thế của họ là đồ thị cho phép "pre-materialize" quan hệ và có hiệu năng tốt hơn "so với các hệ khác", đặc biệt ở truy vấn nhiều join[^neo-premat] — đó là khẳng định của nhà cung cấp; muốn quyết định hãy đo trên dữ liệu của bạn. RDF/SPARQL là một họ mô hình graph khác; chương chỉ giới thiệu ở mức khái niệm trong mục Đi sâu bên dưới.

<details><summary>Đi sâu: thứ tự duyệt, tìm đường ngắn nhất, và so sánh cú pháp</summary>

- Thứ tự kết quả: docs PostgreSQL nói thuật toán đánh giá đệ quy sinh kết quả theo thứ tự duyệt theo chiều rộng nhưng đó là chi tiết cài đặt và có lẽ không nên dựa vào[^pg-bfs-tip]; để có thứ tự duyệt sâu hay rộng, tính thêm cột `path` hoặc `depth` rồi `ORDER BY`[^pg-search-order], hoặc dùng cú pháp dựng sẵn `SEARCH DEPTH FIRST` / `SEARCH BREADTH FIRST`[^pg-search-builtin].
- Cypher có cú pháp cho đường đi ngắn nhất (`SHORTEST`, `ALL SHORTEST`) và docs dùng ví dụ mạng đường sắt để trình bày; docs so sánh đường ngắn nhất theo *số chặng* (từ từ khóa `SHORTEST`) với đường ngắn nhất theo *khoảng cách vật lý*[^cy-distance] — và ví dụ cho thấy có hai đường hòa nhau về số chặng, nên cần chọn tiêu chí rõ[^cy-shortest].
- So sánh của Neo4j: một truy vấn kiểu SQL phải `JOIN` hai lần qua bảng nối, còn Cypher viết một pattern `(actor)-[:ACTED_IN]->(movie)`; đây là ví dụ do nhà cung cấp chọn, cho thấy độ gọn của cú pháp chứ không chứng minh hiệu năng[^cy-concise].
- Dữ liệu đồ thị rất lớn phải phân vùng (partition) qua nhiều máy; cách phân vùng nói chung ở [Chương 06](06-partitioning.md). (Suy luận của người soạn: đồ thị khó cắt hơn bảng vì cạnh nối các đỉnh nằm ở nhiều phân vùng; chương này không có nguồn riêng cho điểm đó.)
- RDF/SPARQL (họ graph khác, chỉ ở mức khái niệm): một RDF model là tập các *statement*, mỗi statement có subject, predicate, object và đôi khi được gọi là *triple*[^jena-triple]; SPARQL là ngôn ngữ truy vấn và giao thức truy cập RDF do W3C RDF Data Access Working Group thiết kế[^jena-sparql]. Nguồn ở đây là tutorial của Apache Jena (một bản cài đặt), không phải đặc tả W3C (trang W3C trả HTTP 403); chương không trình bày cú pháp SPARQL. Mục lục DDIA xếp "Triple-Stores and SPARQL" ngay sau "Graph Queries in SQL" trong chương 2[^ddia-toc-graph].

</details>

**Quay lại Q7:** graph model biểu diễn đỉnh/cạnh có thuộc tính và truy vấn bằng pattern; PostgreSQL làm được phần duyệt cây/đồ thị bằng `WITH RECURSIVE` (nhớ xử lý chu trình), còn graph database gọn hơn khi bài toán xoay quanh đường đi độ dài biến thiên.

## Liên hệ với Rust

Chương này không bàn async runtime (xem [P7·09](../07-io-async/09-async-trong-production.md)); phần dưới chỉ trả lời "kiểu/API nào của Rust đứng trên các mô hình dữ liệu ở trên".

| Khái niệm trong chương | Công cụ Rust | Bảo đảm theo docs |
|---|---|---|
| Hàng ↔ struct (impedance mismatch) | `sqlx::FromRow`, `diesel::Queryable` | derive sinh mã đọc từng cột theo tên field[^sqlx-fromrow-derive] |
| Cột `jsonb` ↔ dữ liệu có kiểu | `sqlx::types::Json<T>` | `T: Deserialize`, dùng cho dữ liệu có cấu trúc[^sqlx-json][^sqlx-pgjson] |
| `jsonb` không có kiểu | `serde_json::Value` | enum đệ quy biểu diễn mọi JSON hợp lệ[^sj-value] |
| Schema-on-read | `#[serde(default)]`, `alias`, `deny_unknown_fields` | xem mục 5 |
| Truy vấn kiểm tra lúc biên dịch | `sqlx::query!`, `diesel::table!` | kiểm tra với CSDL thật hoặc schema khai báo[^sqlx-query-macro][^diesel-schema] |
| Quan hệ 1-n / n-n | `belonging_to` + `grouped_by` của Diesel | nạp bằng nhiều truy vấn[^diesel-multi] |

**`sqlx`.** `FromRow` là trait cho "bản ghi dựng từ một hàng CSDL"; muốn dùng `query_as` thì kiểu đầu ra phải implement `FromRow`[^sqlx-fromrow]. Có thể derive: mã sinh ra là chuỗi lời gọi `Row::try_get` theo tên từng field của struct[^sqlx-fromrow-derive]. Với cột JSON, thuộc tính `#[sqlx(json)]` tự tích hợp giải mã JSON bằng `serde` vào `FromRow`[^sqlx-json-attr]. Kiểu `Json<T>` đại diện cho cột `json` và `jsonb`[^sqlx-json]; theo bảng kiểu của Postgres, `serde_json::Value` dùng cho JSON không cấu trúc và `Json<T>` cho JSON có cấu trúc[^sqlx-pgjson]. Macro `query!` kiểm tra câu SQL tĩnh; yêu cầu `DATABASE_URL` trỏ tới một server có schema tương ứng lúc build, hoặc dữ liệu offline `.sqlx`[^sqlx-query-macro]. (Nhận xét của người soạn: nghĩa là *schema của CSDL trở thành đầu vào của bước biên dịch* — một biến thể schema-on-write chạy ở build time.)

```rust
// Minh họa (chưa biên dịch trong chương này), dựa theo ví dụ trong docs sqlx 0.8.6.
use sqlx::types::Json;

#[derive(serde::Deserialize)]
struct Prefs { theme: String }

#[derive(sqlx::FromRow)]
struct Account {
    id: i32,
    name: String,
    prefs: Json<Prefs>, // Json<T> giải mã jsonb qua serde; có thể dùng #[sqlx(json)] với kiểu thường, xem docs
}
```

**`tokio-postgres`.** Là client PostgreSQL bất đồng bộ có pipelining[^tp-desc]. Một điểm quan trọng với người đọc đã đọc P7: gọi `Client::query` mà chưa poll future thì chưa gửi gì tới server; yêu cầu chạy theo thứ tự được poll lần đầu, không phải thứ tự tạo future[^tp-lazy]. Hỗ trợ kiểu JSON qua feature `with-serde_json-1`[^tp-json-feature]. Vì future bị hủy khi bị drop, tính an toàn khi hủy giữa chừng là chủ đề của [P7·09](../07-io-async/09-async-trong-production.md) (không bàn lại ở đây).

**`serde_json`.** `Value` là enum đệ quy với các biến thể `Null`, `Bool`, `Number`, `String`, `Array`, `Object`[^sj-value]. Truy cập bằng chỉ mục `v["name"]` trả `&Value`; nếu kiểu không khớp, key không có hay chỉ mục vượt biên thì kết quả là `Value::Null`[^sj-index]. (Nhận xét của người soạn: đây là "lỏng lẻo" có chủ ý — thiếu key không là lỗi; cẩn thận khi dùng cho dữ liệu cần xác thực, nên chuyển sang struct có kiểu.)

**`diesel`.** Là ORM và query builder nhằm giảm boilerplate khi làm việc với CSDL[^diesel-desc]. Để kiểm tra truy vấn lúc biên dịch, Diesel yêu cầu khai báo schema trong code bằng macro `table!`, và `diesel print-schema` có thể sinh sẵn các lời gọi này[^diesel-schema]. Association luôn là từ con tới cha (`#[diesel(belongs_to)]`)[^diesel-childparent], dữ liệu và association tách riêng; muốn truyền user cùng các post thì kiểu là `(User, Vec<Post>)`[^diesel-separate].

**Chọn gì.** (Nhận xét của người soạn.) Dịch vụ cần kiểm tra SQL lúc build và thích viết SQL thuần: `sqlx::query!`. Dịch vụ cần query builder có kiểu mạnh: Diesel. Dữ liệu dạng document: `jsonb` + `Json<T>` cho phần cấu trúc ổn định và `serde_json::Value` cho phần thật sự tự do. Mỗi lựa chọn đổi một chỗ kiểm tra lúc build lấy một chỗ phụ thuộc lúc chạy.

## Tóm tắt

- Codd (1970) đề xuất mô hình quan hệ để loại ba phụ thuộc dữ liệu: thứ tự, index, đường truy cập[^codd-3dep]; giá trị cốt lõi là *data independence*[^codd-indep].
- Document model lưu thực thể thành giá trị lồng nhau; `jsonb` của PostgreSQL cho phép index GIN, containment và subscripting[^pg-gin][^pg-contain][^pg-subscript].
- Giá của document: update khóa cả hàng, cấu trúc không được ép, số JSON có nguy cơ mất chính xác[^pg-lock][^pg-fixed][^pg-numeric].
- Impedance mismatch là chi phí dịch giữa đối tượng lồng nhau và bảng phẳng[^hib-mismatch]; ORM và JSON giảm nhưng không xóa.
- Quan hệ nhiều-nhiều dùng bảng nối với hai khóa ngoại[^pg-m2m]; normalization giữ mỗi sự thật một chỗ, denormalize đổi chỗ/công cập nhật lấy tốc độ đọc[^codd-redundancy].
- Schema-on-write kiểm ở lúc ghi (và đổi schema có giá[^pg-rewrite]); schema-on-read kiểm ở lúc đọc bằng mã (`serde` default/alias/deny_unknown_fields).
- SQL và Cypher cùng được mô tả là khai báo[^cy-what][^cy-declarative]; một truy vấn SQL có nhiều kế hoạch, planner chọn kế hoạch được ước lượng nhanh nhất[^pg-planner-ways].
- Property graph gồm node có nhãn, relationship có hướng và kiểu, cả hai có property[^neo-pg]; `WITH RECURSIVE` duyệt cây/đồ thị bằng vòng lặp bảng làm việc[^pg-rec-algo] và cần xử lý chu trình[^pg-cycle-loop].
- Rust: `FromRow`/`Json<T>`/`query!` (sqlx), `table!`/`belonging_to` (Diesel), `serde_json::Value` cho JSON không kiểu.

## Câu hỏi tự kiểm tra

1. Ba loại phụ thuộc dữ liệu mà Codd muốn loại bỏ là gì? Cho một ví dụ cho phụ thuộc index.
2. Nêu ba khác biệt giữa `json` và `jsonb` trong PostgreSQL.
3. Vì sao docs PostgreSQL khuyên giữ tài liệu JSON nhỏ và "nguyên tử"?
4. Vì sao `jdoc -> 'tags' ? 'qui'` không dùng được index GIN trên `jdoc` trong ví dụ docs, và có hai cách xử lý nào?
5. Impedance mismatch là gì; lấy hai ví dụ cụ thể khi ánh xạ struct Rust ↔ bảng.
6. Mô tả bảng nối cho quan hệ đơn hàng–sản phẩm; `ON DELETE RESTRICT` và `ON DELETE CASCADE` ở ví dụ docs làm gì?
7. Normalization theo Codd 1970 là gì (một câu) và `employee` trong ví dụ của ông bị tách thành những relation nào?
8. Phân biệt schema-on-write và schema-on-read; thuộc tính `serde` nào giúp viết bộ đọc chịu được dữ liệu cũ?
9. "Truy vấn khai báo" nghĩa là gì; PostgreSQL có những chiến lược join nào và vì sao người viết SQL không phải chọn?
10. Viết (bằng lời) thủ tục tính của `WITH RECURSIVE`; điều gì xảy ra nếu dữ liệu có chu trình và sửa thế nào?
11. Property graph gồm những thành phần nào; một quy tắc của Cypher về việc khớp relationship trong một pattern?
12. `sqlx::query!` yêu cầu gì lúc build, và điều đó nói gì về "schema" trong hệ Rust?

<details><summary>Đáp án</summary>

1. Phụ thuộc thứ tự, phụ thuộc index, phụ thuộc đường truy cập[^codd-3dep]. Ví dụ index: chương trình phải gọi chuỗi index theo tên nên khi chuỗi bị gỡ thì chạy sai[^codd-chain].
2. `json` giữ nguyên văn bản (khoảng trắng, thứ tự key, key trùng), `jsonb` không giữ cả ba[^pg-json-exact][^pg-jsonb-nopreserve]; `jsonb` nhập chậm hơn chút nhưng xử lý nhanh hơn rõ[^pg-jsonb-vs-json]; `jsonb` có containment và index GIN[^pg-contain][^pg-gin].
3. Update khóa mức hàng trên toàn hàng nên tài liệu lớn gây tranh chấp khóa; lý tưởng mỗi tài liệu là datum không chia nhỏ hợp lý được[^pg-lock][^pg-atomic].
4. Toán tử `?` index được nhưng không áp trực tiếp lên cột đã index[^pg-expr-notused]; cách xử lý theo docs: expression index[^pg-expr-index] hoặc viết lại truy vấn bằng containment `@>`[^pg-contain-alt].
5. Chi phí dịch giữa mô hình đối tượng và quan hệ[^hib-mismatch]. Ví dụ: NULL ↔ `Option<T>`[^sqlx-nullable]; `Vec<Post>` trong code ↔ truy vấn qua khóa ngoại; ORM có `has many` có thể gây N+1[^diesel-n1].
6. Bảng `order_items` có khóa ngoại tới `products` và `orders`, khóa chính ghép[^pg-m2m]. `RESTRICT` ngăn xóa sản phẩm còn được tham chiếu, `CASCADE` xóa dòng đơn khi đơn bị xóa[^pg-restrict].
7. Là thủ tục loại bỏ domain không đơn bằng cách tách thành relation phẳng, mỗi relation con mang khóa của cha[^codd-norm][^codd-normproc]. `employee` → `employee'`, `jobhistory'`, `salaryhistory'`, `children'`[^codd-fig3].
8. Schema-on-write: CSDL kiểm tra lúc ghi[^pg-constraints]; schema-on-read: cấu trúc không ép, mã đọc diễn giải[^pg-fixed]. `#[serde(default)]`[^serde-default], `alias`[^serde-alias], `deny_unknown_fields` (để *nghiêm* hơn mặc định)[^serde-deny].
9. Khai báo: mô tả kết quả, hệ thống chọn cách; nested loop, merge, hash join[^pg-nl][^pg-merge][^pg-hash]; planner xét các thứ tự và phương án để chọn cái ước lượng rẻ nhất[^pg-order].
10. Tính term không đệ quy → vào kết quả và working table; lặp: tính term đệ quy trên working table, thêm vào kết quả, thay working table bằng kết quả mới tới khi rỗng[^pg-rec-algo]. Có chu trình thì lặp mãi[^pg-cycle-loop]; sửa bằng mảng đã thăm[^pg-cycle-array] hoặc `CYCLE`[^pg-cycle-builtin].
11. Node (nhãn, property), relationship (hướng, đúng một kiểu, property)[^neo-pg]. Trong một pattern, mỗi relationship chỉ được khớp một lần[^cy-unique].
12. Cần `DATABASE_URL` trỏ tới server có schema tương ứng lúc build, hoặc `.sqlx` offline[^sqlx-query-macro]: schema của CSDL thành đầu vào của biên dịch.

</details>

## Đọc thêm

- DDIA (Kleppmann, O'Reilly, ấn bản 1), Phần I *Foundations of Data Systems*[^ddia-toc-part], chương 2 *Data Models and Query Languages*[^ddia-toc] — khung chủ đề của chương này. Theo mục lục ETH, chương 2 gồm các mục về mô hình quan hệ so với document, ngôn ngữ truy vấn và graph-like data models (property graph, Cypher, graph queries in SQL, triple-stores và SPARQL)[^ddia-toc-graph]: <https://www.oreilly.com/library/view/designing-data-intensive-applications/9781491903063/ch02.html> (sách trả phí; chương này chỉ dùng tên chương/mục, không trích nội dung sách).
- Codd 1970, "A Relational Model of Data for Large Shared Data Banks" — đọc mục 1.2–1.5 để thấy động cơ và định nghĩa gốc: <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf>.
- PostgreSQL 16, mục 8.14 JSON Types — chi tiết `json`/`jsonb`, containment, index, subscripting, jsonpath: <https://www.postgresql.org/docs/16/datatype-json.html>.
- PostgreSQL 16, mục 7.8 WITH Queries — recursive query, search order, cycle detection: <https://www.postgresql.org/docs/16/queries-with.html>.
- PostgreSQL 16, mục 5.4 Constraints — ràng buộc phía schema-on-write: <https://www.postgresql.org/docs/16/ddl-constraints.html>.
- PostgreSQL 16, mục 52.5 Planner/Optimizer — vì sao một truy vấn có nhiều kế hoạch: <https://www.postgresql.org/docs/16/planner-optimizer.html>.
- RFC 8259 — đặc tả JSON: <https://www.rfc-editor.org/rfc/rfc8259.html>.
- Neo4j Cypher Manual 5, mục Patterns — pattern, đường đi độ dài biến thiên, shortest path: <https://neo4j.com/docs/cypher-manual/5/patterns/>.
- W3C SPARQL 1.1 Query Language (đặc tả RDF query, graph model họ khác): <https://www.w3.org/TR/2013/REC-sparql11-query-20130321/> — trang W3C trả HTTP 403 cho công cụ kiểm trích dẫn nên chương không dùng làm nguồn; phần RDF/SPARQL ở mục 7 dựa trên [tutorial SPARQL của Apache Jena](https://jena.apache.org/tutorials/sparql.html).
- Watt & Eng, *Database Design – 2nd Edition* (BCcampus), chương 12 Normalization — các dạng chuẩn 1NF/2NF/3NF/BCNF: <https://opentextbc.ca/dbdesign01/chapter/chapter-12-normalization/>.
- Docs.rs: [`sqlx` 0.8.6](https://docs.rs/sqlx/0.8.6/sqlx/), [`tokio-postgres` 0.7.13](https://docs.rs/tokio-postgres/0.7.13/tokio_postgres/), [`serde_json` 1.0.140](https://docs.rs/serde_json/1.0.140/serde_json/), [`diesel` 2.2.10](https://docs.rs/diesel/2.2.10/diesel/).
- Tiếp theo: [Chương 03 — Storage engine](03-storage-engine.md) (dữ liệu và index nằm thế nào trên đĩa) và [Chương 04 — Encoding và tiến hóa schema](04-encoding-va-tien-hoa-schema.md).

[^codd-natural]: Codd 1970, A Relational Model of Data for Large Shared Data Banks, mục 1.1 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "It provides a means of describing data with its natural structure only-that is, without superimposing any additional structure for machine representation purposes."
[^codd-indep]: Codd 1970, mục 1.1 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "Accordingly, it provides a basis for a high level data language which will yield maximal independence between programs on the one hand and machine representation and organization of data on the other."
[^codd-scope]: Codd 1970, mục 1.1 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "In contrast, the problems treated here are those of data independence-the independence of application programs and terminal activities from growth in data types and changesin data representation and certain kinds of data inconsistency which are expected to become troublesome even in nondeductive systems"
[^codd-3dep]: Codd 1970, mục 1.2 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "Three of the principal kinds of data dependencies which still need to be removed are: ordering dependence, indexing dependence, and accesspath dependence."
[^codd-order]: Codd 1970, mục 1.2.1 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "It is unnecessary to single out any system as an example, because all the well-known information systems that are marketed today fail to make a clear distinction between order of presentation on the one hand and stored ordering on the other."
[^codd-index]: Codd 1970, mục 1.2.2 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "In the context of formatted data, an index is usually thought of as a purely performance-oriented component of the data representation."
[^codd-chain]: Codd 1970, mục 1.2.2 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "Such programs do not operate correctly if these chains are later removed."
[^codd-accesspath]: Codd 1970, mục 1.2.3 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "The consequence, regardless of the stored representation, is that terminal activities and programs become dependent on the continued existence of the user access paths."
[^codd-pathpolicy]: Codd 1970, mục 1.2.3 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "Such a policy is not practical, because the number of access paths in the total model for the community of users of a data bank would eventually become excessively large."
[^codd-rel-props]: Codd 1970, mục 1.3 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "(2) The ordering of rows is immaterial. (3) All rows are distinct."
[^codd-timevarying]: Codd 1970, mục 1.3 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "The totality of data in a data bank may be viewed as a collection of time-varying relations."
[^codd-nodist]: Codd 1970, mục 1.3 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "This distinction is difficult to maintain when one may have foreign keys in any relation whatsoever."
[^codd-descr]: Codd 1970, mục 1.5 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "The universality of the data sublanguage lies in its descriptive ability (not its computing ability)."
[^codd-role]: Codd 1970, mục 1.3 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "we require in each case that the domain name be qualified by a distinctive role name, which serves to identify the role played by that domain in the given relation."
[^codd-nonsimple]: Codd 1970, mục 1.3 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "Thus, some domains may have relations as elements."
[^codd-norm]: Codd 1970, mục 1.4 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "There is, in fact, a very simple elimination procedure, which we shall call normalization."
[^codd-normproc]: Codd 1970, mục 1.4 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "Now, strike out from the parent relation all nonsimple domains, remove the top node of the tree, and repeat the same sequence of operations on each remaining subtree."
[^codd-fig3]: Codd 1970, mục 1.4, hình 3(b) — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "employee' (man#, name, birthdate) jobhistory' (man#, jobdate, title)"
[^codd-simple]: Codd 1970, mục 1.4 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "A relation whose domains are all simple can be represented in storage by a two-dimensional column-homogeneous array"
[^codd-join]: Codd 1970, mục 2.1.3 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "One join that always exists in such a case is the natural join of R with S defined by"
[^codd-strong]: Codd 1970, mục 2.2.1 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "A set of relations is strongly redundant if it contains at least one relation that possesses a projection which is derivable from other projections of relations in the set."
[^codd-redundancy]: Codd 1970, mục 2.2.1 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "extra storage space and update time are consumed with a potential drop in query time for some queries and in load on the central processing units."
[^codd-consistent]: Codd 1970, mục 2.3 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "we shall call the state (C, 2, V) consistent or inconsistent according as V does or does not satisfy 2."
[^codd-parts]: Codd 1970, mục 1.3 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "It is a relation which plays a critical role in the parts explosion problem."
[^pg-constraints]: PostgreSQL 16, mục 5.4 Constraints — <https://www.postgresql.org/docs/16/ddl-constraints.html> — "Data types are a way to limit the kind of data that can be stored in a table. For many applications, however, the constraint they provide is too coarse."
[^pg-check]: PostgreSQL 16, mục 5.4.1 Check Constraints — <https://www.postgresql.org/docs/16/ddl-constraints.html> — "A check constraint is the most generic constraint type."
[^pg-fk]: PostgreSQL 16, mục 5.4.5 Foreign Keys — <https://www.postgresql.org/docs/16/ddl-constraints.html> — "A foreign key constraint specifies that the values in a column (or a group of columns) must match the values appearing in some row of another table."
[^pg-m2m]: PostgreSQL 16, mục 5.4.5 Foreign Keys — <https://www.postgresql.org/docs/16/ddl-constraints.html> — "This is used to implement many-to-many relationships between tables."
[^pg-restrict]: PostgreSQL 16, mục 5.4.5 Foreign Keys — <https://www.postgresql.org/docs/16/ddl-constraints.html> — "Restricting and cascading deletes are the two most common options. RESTRICT prevents deletion of a referenced row."
[^pg-fk-index]: PostgreSQL 16, mục 5.4.5 Foreign Keys — <https://www.postgresql.org/docs/16/ddl-constraints.html> — "it is often a good idea to index the referencing columns too. Because this is not always needed, and there are many choices available on how to index, the declaration of a foreign key constraint does not automatically create an index on the referencing columns."
[^pg-json-valid]: PostgreSQL 16, mục 8.14 JSON Types — <https://www.postgresql.org/docs/16/datatype-json.html> — "the JSON data types have the advantage of enforcing that each stored value is valid according to the JSON rules."
[^pg-json-exact]: PostgreSQL 16, mục 8.14 JSON Types — <https://www.postgresql.org/docs/16/datatype-json.html> — "Because the json type stores an exact copy of the input text, it will preserve semantically-insignificant white space between tokens, as well as the order of keys within JSON objects."
[^pg-jsonb-vs-json]: PostgreSQL 16, mục 8.14 JSON Types — <https://www.postgresql.org/docs/16/datatype-json.html> — "jsonb data is stored in a decomposed binary format that makes it slightly slower to input due to added conversion overhead, but significantly faster to process, since no reparsing is needed. jsonb also supports indexing, which can be a significant advantage."
[^pg-jsonb-nopreserve]: PostgreSQL 16, mục 8.14 JSON Types — <https://www.postgresql.org/docs/16/datatype-json.html> — "jsonb does not preserve white space, does not preserve the order of object keys, and does not keep duplicate object keys."
[^pg-prefer-jsonb]: PostgreSQL 16, mục 8.14 JSON Types — <https://www.postgresql.org/docs/16/datatype-json.html> — "In general, most applications should prefer to store JSON data as jsonb, unless there are quite specialized needs"
[^pg-flex]: PostgreSQL 16, mục 8.14.2 Designing JSON Documents — <https://www.postgresql.org/docs/16/datatype-json.html> — "Representing data as JSON can be considerably more flexible than the traditional relational data model, which is compelling in environments where requirements are fluid. It is quite possible for both approaches to co-exist and complement each other within the same application."
[^pg-fixed]: PostgreSQL 16, mục 8.14.2 Designing JSON Documents — <https://www.postgresql.org/docs/16/datatype-json.html> — "it is still recommended that JSON documents have a somewhat fixed structure. The structure is typically unenforced (though enforcing some business rules declaratively is possible)"
[^pg-lock]: PostgreSQL 16, mục 8.14.2 Designing JSON Documents — <https://www.postgresql.org/docs/16/datatype-json.html> — "keep in mind that any update acquires a row-level lock on the whole row."
[^pg-atomic]: PostgreSQL 16, mục 8.14.2 Designing JSON Documents — <https://www.postgresql.org/docs/16/datatype-json.html> — "Ideally, JSON documents should each represent an atomic datum that business rules dictate cannot reasonably be further subdivided into smaller datums that could be modified independently."
[^pg-contain]: PostgreSQL 16, mục 8.14.3 jsonb Containment and Existence — <https://www.postgresql.org/docs/16/datatype-json.html> — "Containment tests whether one jsonb document has contained within it another one."
[^pg-gin]: PostgreSQL 16, mục 8.14.4 jsonb Indexing — <https://www.postgresql.org/docs/16/datatype-json.html> — "GIN indexes can be used to efficiently search for keys or key/value pairs occurring within a large number of jsonb documents (datums)."
[^pg-gin-default]: PostgreSQL 16, mục 8.14.4 jsonb Indexing — <https://www.postgresql.org/docs/16/datatype-json.html> — "The default GIN operator class for jsonb supports queries with the key-exists operators ?, ?| and ?&, the containment operator @>, and the jsonpath match operators @? and @@."
[^pg-expr-notused]: PostgreSQL 16, mục 8.14.4 jsonb Indexing — <https://www.postgresql.org/docs/16/datatype-json.html> — "the index could not be used for queries like the following, because though the operator ? is indexable, it is not applied directly to the indexed column jdoc"
[^pg-subscript]: PostgreSQL 16, mục 8.14.5 jsonb Subscripting — <https://www.postgresql.org/docs/16/datatype-json.html> — "The jsonb data type supports array-style subscripting expressions to extract and modify elements."
[^pg-numeric]: PostgreSQL 16, mục 8.14.1 JSON Input and Output Syntax — <https://www.postgresql.org/docs/16/datatype-json.html> — "Notably, jsonb will reject numbers that are outside the range of the PostgreSQL numeric data type, while json will not."
[^pg-toast]: PostgreSQL 16, mục 73.2 TOAST — <https://www.postgresql.org/docs/16/storage-toast.html> — "PostgreSQL uses a fixed page size (commonly 8 kB), and does not allow tuples to span multiple pages."
[^pg-toast-1gb]: PostgreSQL 16, mục 73.2 TOAST — <https://www.postgresql.org/docs/16/storage-toast.html> — "thereby limiting the logical size of any value of a TOAST-able data type to 1 GB"
[^pg-join-def]: PostgreSQL 16, mục 2.6 Joins Between Tables — <https://www.postgresql.org/docs/16/tutorial-join.html> — "Queries that access multiple tables (or multiple instances of the same table) at one time are called join queries."
[^pg-outer]: PostgreSQL 16, mục 2.6 Joins Between Tables — <https://www.postgresql.org/docs/16/tutorial-join.html> — "If no matching row is found we want some “empty values” to be substituted for the cities table's columns. This kind of query is called an outer join."
[^pg-nl]: PostgreSQL 16, mục 52.5.1 Generating Possible Plans — <https://www.postgresql.org/docs/16/planner-optimizer.html> — "nested loop join: The right relation is scanned once for every row found in the left relation."
[^pg-merge]: PostgreSQL 16, mục 52.5.1 Generating Possible Plans — <https://www.postgresql.org/docs/16/planner-optimizer.html> — "merge join: Each relation is sorted on the join attributes before the join starts."
[^pg-hash]: PostgreSQL 16, mục 52.5.1 Generating Possible Plans — <https://www.postgresql.org/docs/16/planner-optimizer.html> — "hash join: the right relation is first scanned and loaded into a hash table, using its join attributes as hash keys."
[^pg-order]: PostgreSQL 16, mục 52.5.1 Generating Possible Plans — <https://www.postgresql.org/docs/16/planner-optimizer.html> — "The planner examines different possible join sequences to find the cheapest one."
[^pg-geqo]: PostgreSQL 16, mục 52.5.1 Generating Possible Plans — <https://www.postgresql.org/docs/16/planner-optimizer.html> — "When geqo_threshold is exceeded, the join sequences considered are determined by heuristics, as described in Chapter 62."
[^pg-planner-ways]: PostgreSQL 16, mục 52.5 Planner/Optimizer — <https://www.postgresql.org/docs/16/planner-optimizer.html> — "A given SQL query (and hence, a query tree) can be actually executed in a wide variety of different ways, each of which will produce the same set of results."
[^pg-plan-tree]: PostgreSQL 16, mục 52.5.1 Generating Possible Plans — <https://www.postgresql.org/docs/16/planner-optimizer.html> — "The finished plan tree consists of sequential or index scans of the base relations, plus nested-loop, merge, or hash join nodes as needed"
[^pg-explain]: PostgreSQL 16, mục 14.1 Using EXPLAIN — <https://www.postgresql.org/docs/16/using-explain.html> — "Choosing the right plan to match the query structure and the properties of the data is absolutely critical for good performance, so the system includes a complex planner that tries to choose good plans."
[^pg-addcol]: PostgreSQL 16, ALTER TABLE, mục Notes — <https://www.postgresql.org/docs/16/sql-altertable.html> — "When a column is added with ADD COLUMN and a non-volatile DEFAULT is specified, the default is evaluated at the time of the statement and the result stored in the table's metadata."
[^pg-rewrite]: PostgreSQL 16, ALTER TABLE, mục Notes — <https://www.postgresql.org/docs/16/sql-altertable.html> — "Adding a column with a volatile DEFAULT or changing the type of an existing column will require the entire table and its indexes to be rewritten."
[^pg-setnotnull]: PostgreSQL 16, ALTER TABLE, mục SET/DROP NOT NULL — <https://www.postgresql.org/docs/16/sql-altertable.html> — "Ordinarily this is checked during the ALTER TABLE by scanning the entire table"
[^pg-rec-def]: PostgreSQL 16, mục 7.8.2 Recursive Queries — <https://www.postgresql.org/docs/16/queries-with.html> — "Using RECURSIVE, a WITH query can refer to its own output."
[^pg-rec-form]: PostgreSQL 16, mục 7.8.2 Recursive Queries — <https://www.postgresql.org/docs/16/queries-with.html> — "The general form of a recursive WITH query is always a non-recursive term, then UNION (or UNION ALL), then a recursive term, where only the recursive term can contain a reference to the query's own output."
[^pg-rec-use]: PostgreSQL 16, mục 7.8.2 Recursive Queries — <https://www.postgresql.org/docs/16/queries-with.html> — "Recursive queries are typically used to deal with hierarchical or tree-structured data."
[^pg-rec-algo]: PostgreSQL 16, mục 7.8.2 Recursive Queries — <https://www.postgresql.org/docs/16/queries-with.html> — "Replace the contents of the working table with the contents of the intermediate table, then empty the intermediate table."
[^pg-rec-iter]: PostgreSQL 16, mục 7.8.2 Recursive Queries — <https://www.postgresql.org/docs/16/queries-with.html> — "While RECURSIVE allows queries to be specified recursively, internally such queries are evaluated iteratively."
[^pg-rec-parts]: PostgreSQL 16, mục 7.8.2 Recursive Queries — <https://www.postgresql.org/docs/16/queries-with.html> — "A useful example is this query to find all the direct and indirect sub-parts of a product, given only a table that shows immediate inclusions:"
[^pg-search-order]: PostgreSQL 16, mục 7.8.2.1 Search Order — <https://www.postgresql.org/docs/16/queries-with.html> — "Note that this does not actually control in which order the query evaluation visits the rows; that is as always in SQL implementation-dependent."
[^pg-bfs-tip]: PostgreSQL 16, mục 7.8.2.1 Search Order — <https://www.postgresql.org/docs/16/queries-with.html> — "The recursive query evaluation algorithm produces its output in breadth-first search order. However, this is an implementation detail and it is perhaps unsound to rely on it."
[^pg-cycle-loop]: PostgreSQL 16, mục 7.8.2.2 Cycle Detection — <https://www.postgresql.org/docs/16/queries-with.html> — "This query will loop if the link relationships contain cycles."
[^pg-cycle-array]: PostgreSQL 16, mục 7.8.2.2 Cycle Detection — <https://www.postgresql.org/docs/16/queries-with.html> — "The standard method for handling such situations is to compute an array of the already-visited values."
[^pg-cycle-builtin]: PostgreSQL 16, mục 7.8.2.2 Cycle Detection — <https://www.postgresql.org/docs/16/queries-with.html> — "There is built-in syntax to simplify cycle detection."
[^rfc-json]: RFC 8259, Abstract — <https://www.rfc-editor.org/rfc/rfc8259.html> — "JavaScript Object Notation (JSON) is a lightweight, text-based, language-independent data interchange format."
[^rfc-unique]: RFC 8259, mục 4 Objects — <https://www.rfc-editor.org/rfc/rfc8259.html> — "The names within an object SHOULD be unique."
[^hib-mismatch]: Hibernate ORM 5.1 User Guide, Preface — <https://docs.jboss.org/hibernate/orm/5.1/userguide/html_single/Hibernate_User_Guide.html> — "Developing Object-Oriented software that deals with data from Relational Databases can be cumbersome and resource consuming. Development costs are significantly higher due to a paradigm mismatch between how data is represented in objects versus relational databases. Hibernate is an Object/Relational Mapping (ORM) solution for Java environments. ORM refers to the technique of mapping data between an object model representation to a relational data model representation."
[^neo-pg]: Neo4j Getting Started, Graph database concepts — <https://neo4j.com/docs/getting-started/appendix/graphdb-concepts/> — "Nodes can have zero or more labels to define (classify) what kind of nodes they are. Relationships describe a connection between a source node and a target node. Relationships always have a direction (one direction). Relationships must have a type (one type) to define (classify) what type of relationship they are. Nodes and relationships can have properties (key-value pairs), which further describe them."
[^neo-create]: Neo4j Getting Started, Graph database concepts — <https://neo4j.com/docs/getting-started/appendix/graphdb-concepts/> — "To create the example graph, use the Cypher® clause CREATE."
[^neo-join-table]: Neo4j Getting Started, Comparing relational to graph database — <https://neo4j.com/docs/getting-started/appendix/graphdb-concepts/graphdb-vs-rdbms/> — "JOIN s are computed at query time by matching primary and foreign keys of all rows in the connected tables. These operations are compute-heavy and memory-intensive, and have an exponential cost. When many-to-many relationships occur in the model, you must introduce a JOIN table (or associative entity table) that holds foreign keys of both the participating tables, further increasing join operation costs"
[^neo-premat]: Neo4j Getting Started, Comparing relational to graph database — <https://neo4j.com/docs/getting-started/appendix/graphdb-concepts/graphdb-vs-rdbms/> — "This ability to pre-materialize relationships into the database structure allows Neo4j to provide improved performance compared to others, especially for join-heavy queries."
[^neo-flex]: Neo4j Cypher Manual 5, Overview — <https://neo4j.com/docs/cypher-manual/5/introduction/cypher-overview/> — "users are not required to use a fixed schema to represent data and that they can add new attributes and relationships as their graphs evolve."
[^cy-declarative]: Neo4j Cypher Manual 5, Overview — <https://neo4j.com/docs/cypher-manual/5/introduction/cypher-overview/> — "Cypher is Neo4j’s declarative graph query language."
[^cy-2011]: Neo4j Cypher Manual 5, Overview — <https://neo4j.com/docs/cypher-manual/5/introduction/cypher-overview/> — "It was created in 2011 by Neo4j engineers as an SQL-equivalent language for graph databases."
[^cy-what]: Neo4j Cypher Manual 5, Overview — <https://neo4j.com/docs/cypher-manual/5/introduction/cypher-overview/> — "Similar to SQL, Cypher lets users focus on what to retrieve from graph, rather than how to retrieve it."
[^cy-ascii]: Neo4j Cypher Manual 5, Overview — <https://neo4j.com/docs/cypher-manual/5/introduction/cypher-overview/> — "Writing a query is effectively like drawing a pattern through the data in the graph."
[^cy-order]: Neo4j Cypher Manual 5, Overview — <https://neo4j.com/docs/cypher-manual/5/introduction/cypher-overview/> — "SQL queries begin with what a user wants to return, whereas Cypher queries end with the return clause."
[^cy-concise]: Neo4j Cypher Manual 5, Overview — <https://neo4j.com/docs/cypher-manual/5/introduction/cypher-overview/> — "Due to its intuitive, whiteboard-like method of constructing clauses, Cypher queries are often more concise than their equivalent SQL queries."
[^cy-qpp]: Neo4j Cypher Manual 5, Variable-length paths — <https://neo4j.com/docs/cypher-manual/5/patterns/variable-length-patterns/> — "Prior to the introduction of quantified path patterns and quantified relationships in Neo4j 5.9, the only method in Cypher to match paths of a variable length was through variable-length relationships."
[^cy-unique]: Neo4j Cypher Manual 5, MATCH — <https://neo4j.com/docs/cypher-manual/5/clauses/match/> — "Relationships will only be matched once inside a single pattern."
[^cy-shortest]: Neo4j Cypher Manual 5, Variable-length paths — <https://neo4j.com/docs/cypher-manual/5/patterns/variable-length-patterns/> — "ALL SHORTEST finds all shortest paths by number of hops, and as the result shows, there are two paths in the graph tied for the shortest path."
[^sqlx-fromrow]: sqlx 0.8.6, trait FromRow — <https://docs.rs/sqlx/0.8.6/sqlx/trait.FromRow.html> — "In order to use query_as the output type must implement FromRow."
[^sqlx-fromrow-derive]: sqlx 0.8.6, trait FromRow, mục Derivable — <https://docs.rs/sqlx/0.8.6/sqlx/trait.FromRow.html> — "The generated implementation will consist of a sequence of calls to Row::try_get using the name from each struct field."
[^sqlx-json-attr]: sqlx 0.8.6, trait FromRow, mục json — <https://docs.rs/sqlx/0.8.6/sqlx/trait.FromRow.html> — "If your database supports a JSON type, you can leverage #[sqlx(json)] to automatically integrate JSON deserialization in your FromRow implementation using serde."
[^sqlx-json]: sqlx 0.8.6, struct types::Json — <https://docs.rs/sqlx/0.8.6/sqlx/types/struct.Json.html> — "Json for json and jsonb fields"
[^sqlx-pgjson]: sqlx 0.8.6, module postgres::types — <https://docs.rs/sqlx/0.8.6/sqlx/postgres/types/index.html> — "Value and RawValue from serde_json can be used for unstructured JSON data with Postgres."
[^sqlx-query-macro]: sqlx 0.8.6, macro query — <https://docs.rs/sqlx/0.8.6/sqlx/macro.query.html> — "The DATABASE_URL environment variable must be set at build-time to point to a database server with the schema that the query string will be checked against."
[^sqlx-nullable]: sqlx 0.8.6, macro query, mục Nullability: Bind Parameters — <https://docs.rs/sqlx/0.8.6/sqlx/macro.query.html> — "For a given expected type T, both T and Option\<T> are allowed (as well as either behind references). Option::None will be bound as NULL"
[^tp-desc]: tokio-postgres 0.7.13, crate docs — <https://docs.rs/tokio-postgres/0.7.13/tokio_postgres/> — "An asynchronous, pipelined, PostgreSQL client."
[^tp-lazy]: tokio-postgres 0.7.13, crate docs, mục Behavior — <https://docs.rs/tokio-postgres/0.7.13/tokio_postgres/> — "Calling a method like Client::query on its own does nothing. The associated request is not sent to the database until the future returned by the method is first polled. Requests are executed in the order that they are first polled, not in the order that their futures are created."
[^tp-json-feature]: tokio-postgres 0.7.13, crate docs, mục Features — <https://docs.rs/tokio-postgres/0.7.13/tokio_postgres/> — "Enable support for the serde_json crate."
[^sj-three]: serde_json 1.0.140, crate docs — <https://docs.rs/serde_json/1.0.140/serde_json/> — "As an untyped or loosely typed representation. Maybe you want to check that some JSON data is valid before passing it on, but without knowing the structure of what it contains. Or you want to do very basic manipulations like insert a key in a particular spot. As a strongly typed Rust data structure. When you expect all or most of your data to conform to a particular structure"
[^sj-value]: serde_json 1.0.140, crate docs — <https://docs.rs/serde_json/1.0.140/serde_json/> — "Any valid JSON data can be manipulated in the following recursive enum representation."
[^sj-index]: serde_json 1.0.140, crate docs — <https://docs.rs/serde_json/1.0.140/serde_json/> — "If the type of the data is not right for the type with which it is being indexed, or if a map does not contain the key being indexed, or if the index into a vector is out of bounds, the returned element is Value::Null."
[^serde-deny]: Serde, Container attributes — <https://serde.rs/container-attrs.html> — "When this attribute is not present, by default unknown fields are ignored for self-describing formats like JSON."
[^serde-default]: Serde, Field attributes — <https://serde.rs/field-attrs.html> — "If the value is not present when deserializing, use the Default::default()."
[^serde-alias]: Serde, Field attributes — <https://serde.rs/field-attrs.html> — "Deserialize this field from the given name or from its Rust name."
[^diesel-desc]: diesel 2.2.10, crate docs — <https://docs.rs/diesel/2.2.10/diesel/> — "Diesel is an ORM and query builder designed to reduce the boilerplate for database interactions."
[^diesel-schema]: diesel 2.2.10, crate docs, mục Declaring your schema — <https://docs.rs/diesel/2.2.10/diesel/> — "For Diesel to validate your queries at compile time it requires you to specify your schema in your code, which you can do with the table! macro."
[^diesel-builder]: diesel 2.2.10, crate docs, mục Constructing a query — <https://docs.rs/diesel/2.2.10/diesel/> — "These methods usually have the same name as the SQL they map to, except for WHERE which is called filter in Diesel"
[^diesel-multi]: diesel 2.2.10, module associations — <https://docs.rs/diesel/2.2.10/diesel/associations/index.html> — "Associated data is typically loaded in multiple queries (one query per table)."
[^diesel-join-cost]: diesel 2.2.10, module associations — <https://docs.rs/diesel/2.2.10/diesel/associations/index.html> — "For most datasets, using a join to load in a single query transmits so much duplicate data that it costs more time than the extra round trip would have."
[^diesel-n1]: diesel 2.2.10, module associations — <https://docs.rs/diesel/2.2.10/diesel/associations/index.html> — "This design leads to many problems, including N+1 query bugs, and runtime errors when accessing an association that isn’t there."
[^diesel-separate]: diesel 2.2.10, module associations — <https://docs.rs/diesel/2.2.10/diesel/associations/index.html> — "In Diesel, data and its associations are considered to be separate."
[^ddia-toc]: DDIA (ấn bản 1), mục lục do ETH Library lưu, chương 2 — <https://toc.library.ethz.ch/objects/pdf03/z01_978-1-4493-7332-0_01.pdf> — "2. Data Models and Query Languages Relational Model Versus Document Model"
[^ddia-toc-part]: DDIA (ấn bản 1), mục lục do ETH Library lưu, Phần I — <https://toc.library.ethz.ch/objects/pdf03/z01_978-1-4493-7332-0_01.pdf> — "Part I. 1. Foundations of Data Systems Reliable, Scalable, and Maintainable Applications"
[^ddia-toc-graph]: DDIA (ấn bản 1), mục lục do ETH Library lưu, chương 2 — <https://toc.library.ethz.ch/objects/pdf03/z01_978-1-4493-7332-0_01.pdf> — "Graph-Like Data Models Property Graphs The Cypher Query Language Graph Queries in SQL Triple-Stores and SPARQL"
[^codd-superior]: Codd 1970, mục 1.1 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "appears to be superior in several respects to the graph or network model [3, 4] presently in vogue for noninferential systems"
[^codd-order-fail]: Codd 1970, mục 1.2.1 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "Application programs which take advantage of the stored ordering of a file are likely to fail to operate correctly if for some reason it becomes necessary to replace that ordering by a different one."
[^codd-reldef]: Codd 1970, mục 1.3 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "R is a relation on these n sets if it is a set of ntuples each of which has its first element from S1, its"
[^codd-array]: Codd 1970, mục 1.3 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "this particular representation is not an essential part of the relational view being expounded"
[^codd-fk]: Codd 1970, mục 1.3 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "a foreign key if it is not the primary key of R but its elements are values of the primary key of some relation S"
[^codd-sublang]: Codd 1970, mục 1.5 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "permits the development of a universal data sublanguage based on an applied predicate calculus"
[^codd-embed]: Codd 1970, mục 1.5 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "would itself be a strong candidate for embedding (with appropriate syntactic modification) in a variety of host"
[^codd-repgroup]: Codd 1970, mục 1.3 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "The terms attribute and repeating group in present data base terminology are roughly analogous to simple domain"
[^codd-constraints]: Codd 1970, mục 2.3 — <https://www.seas.upenn.edu/~zives/03f/cis550/codd.pdf> — "a collection of statements which define all of the redundancies which hold independent of time between the member relations"
[^bc-nf]: Watt & Eng, Database Design – 2nd Edition (BCcampus), chương 12 Normalization — <https://opentextbc.ca/dbdesign01/chapter/chapter-12-normalization/> — "There are six normal forms, but we will only look at the first four, which are: First normal form (1NF) Second normal form (2NF) Third normal form (3NF) Boyce-Codd normal form (BCNF)"
[^bc-1nf]: Watt & Eng, Database Design – 2nd Edition (BCcampus), chương 12 Normalization — <https://opentextbc.ca/dbdesign01/chapter/chapter-12-normalization/> — "In the first normal form, only single values are permitted at the intersection of each row and column; hence, there are no repeating groups."
[^bc-fd]: Watt & Eng, Database Design – 2nd Edition (BCcampus), chương 12 Normalization — <https://opentextbc.ca/dbdesign01/chapter/chapter-12-normalization/> — "Normalization theory draws heavily on the theory of functional dependencies."
[^pg-cascade]: PostgreSQL 16, mục 5.4.5 Foreign Keys — <https://www.postgresql.org/docs/16/ddl-constraints.html> — "CASCADE specifies that when a referenced row is deleted, row(s) referencing it should be automatically deleted as well."
[^pg-expr-index]: PostgreSQL 16, mục 8.14.4 jsonb Indexing — <https://www.postgresql.org/docs/16/datatype-json.html> — "Still, with appropriate use of expression indexes, the above query can use an index"
[^pg-contain-alt]: PostgreSQL 16, mục 8.14.4 jsonb Indexing — <https://www.postgresql.org/docs/16/datatype-json.html> — "Another approach to querying is to exploit containment, for example:"
[^pg-numeric-loss]: PostgreSQL 16, mục 8.14.1 JSON Input and Output Syntax — <https://www.postgresql.org/docs/16/datatype-json.html> — "When using JSON as an interchange format with such systems, the danger of losing numeric precision compared to data originally stored by PostgreSQL should be considered."
[^pg-search-builtin]: PostgreSQL 16, mục 7.8.2.1 Search Order — <https://www.postgresql.org/docs/16/queries-with.html> — "There is built-in syntax to compute a depth- or breadth-first sort column."
[^pg-cycle-rewrite]: PostgreSQL 16, mục 7.8.2.2 Cycle Detection — <https://www.postgresql.org/docs/16/queries-with.html> — "and it will be internally rewritten to the above form"
[^rfc-unpredictable]: RFC 8259, mục 4 Objects — <https://www.rfc-editor.org/rfc/rfc8259.html> — "When the names within an object are not unique, the behavior of software that receives such an object is unpredictable."
[^neo-partial]: Neo4j Cypher Manual 5, Overview — <https://neo4j.com/docs/cypher-manual/5/introduction/cypher-overview/> — "While it is both possible and advised to enforce partial schemas using indexes and constraints, Cypher and Neo4j offers a greater degree of schema-flexibility than SQL and a relational database."
[^cy-syntax]: Neo4j Cypher Manual 5, Overview — <https://neo4j.com/docs/cypher-manual/5/introduction/cypher-overview/> — "Rounded brackets are used for circular nodes, and -[:ARROWS]-> for relationships."
[^cy-distance]: Neo4j Cypher Manual 5, Variable-length paths — <https://neo4j.com/docs/cypher-manual/5/patterns/variable-length-patterns/> — "finding the shortest path by physical distance and compares that to the results yielded by using the SHORTEST keyword"
[^sj-text]: serde_json 1.0.140, crate docs — <https://docs.rs/serde_json/1.0.140/serde_json/> — "As text data. An unprocessed string of JSON data that you receive on an HTTP endpoint, read from a file, or prepare to send to a remote server."
[^diesel-childparent]: diesel 2.2.10, module associations — <https://docs.rs/diesel/2.2.10/diesel/associations/index.html> — "Associations in Diesel are always child-to-parent. You can declare an association between two records with #[diesel(belongs_to)]. Unlike other ORMs, Diesel has no concept of has many"
[^jena-triple]: Apache Jena, RDF API tutorial, mục Statements — <https://jena.apache.org/tutorials/rdf_api.html> — "A statement has three parts: the subject is the resource from which the arc leaves the predicate is the property that labels the arc the object is the resource or literal pointed to by the arc A statement is sometimes called a triple, because of its three parts."
[^jena-sparql]: Apache Jena, SPARQL tutorial — <https://jena.apache.org/tutorials/sparql.html> — "SPARQL is a query language and a protocol for accessing RDF designed by the W3C RDF Data Access Working Group."
