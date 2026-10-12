# P12 · Chương 04 — Encoding và tiến hóa schema

> Chương này dùng *Designing Data-Intensive Applications* (DDIA) chỉ làm **khung chủ đề** (chương 4 của sách, "Encoding and Evolution"[^ddia-toc]); sách không có bản đọc tự do nên mọi khẳng định bên dưới lấy từ nguồn mở: bài giảng *Distributed Systems* của Kleppmann tại Cambridge, đặc tả/tài liệu chính thức của JSON, Protocol Buffers, Avro, gRPC, Confluent Schema Registry, PostgreSQL 16 và docs.rs của các crate Rust (phiên bản ghi trong từng chú thích). Không có thí nghiệm nào được chạy; ví dụ code chỉ để minh họa và không khẳng định output.

## Bạn sẽ hiểu được gì

- **Q1.** Vì sao dữ liệu trong bộ nhớ phải được *encode* trước khi lưu hay gửi đi, và JSON/CSV hạn chế ở đâu (số, binary, schema)? → mục 1
- **Q2.** Protocol Buffers biến một message thành byte ra sao (field number, wire type, varint), và vì sao thiết kế đó chịu được schema đổi? → mục 2
- **Q3.** Những thay đổi nào trên một `.proto` là an toàn, có điều kiện, hay phá hỏng dữ liệu — và vì sao không bao giờ tái dùng field number? → mục 3
- **Q4.** Avro làm khác Protobuf thế nào: writer schema, reader schema và *schema resolution* hoạt động ra sao? → mục 4
- **Q5.** Backward compatibility và forward compatibility khác nhau thế nào, và vì sao *rolling upgrade* buộc ta cần cả hai? → mục 5
- **Q6.** Khi dữ liệu đi qua database, "code mới đọc dữ liệu cũ" và "code cũ ghi đè dữ liệu mới" gây vấn đề gì? → mục 6
- **Q7.** Qua RPC/REST và qua message broker, ai nâng cấp trước, và schema registry giải quyết việc gì? → mục 7

## Kiến thức cần trước

- [P2 · Chương 02 — Biểu diễn dữ liệu](../02-cpu/02-bieu-dien-du-lieu.md): số nguyên có dấu/không dấu, endianness (mục 4), `f64` (mục 5), UTF-8 (mục 6). Chương này dùng lại các khái niệm đó ở mức "đặc tả định dạng chọn gì".
- [P6 · Chương 08 — DNS, HTTP, TLS](../06-networking/08-dns-http-tls.md), mục 5: HTTP/1.1 và HTTP/2, nền của REST và gRPC.
- [P7 · Chương 09 — Async trong production](../07-io-async/09-async-trong-production.md), mục 2: `timeout` chỉ ngừng chờ chứ không hủy việc phía xa; mục 7 dưới đây dùng lại ý này cho RPC.
- Chương P12 liên quan: [02 — Mô hình dữ liệu](02-mo-hinh-du-lieu.md) (schema-on-write / schema-on-read), [05 — Replication](05-replication.md), [10 — Batch processing](10-batch-processing.md), [11 — Stream processing](11-stream-processing.md). Tổng quan kiến trúc hệ dữ liệu và vận hành nằm ở [13 — Kiến trúc hệ dữ liệu](13-kien-truc-he-du-lieu.md).

## 1. Vì sao cần encoding; JSON, CSV hạn chế ở đâu (Q1)

> **Trả lời ngắn.** Chương trình giữ dữ liệu dưới dạng struct/object trong bộ nhớ, còn đĩa và mạng chỉ chở được dãy byte; việc đổi qua lại gọi là encode/decode (RPC gọi là *marshalling*[^cam-marshal]). Định dạng văn bản như JSON và CSV tiện đọc và phổ biến (nhận xét của người soạn), nhưng JSON không có kiểu "dãy byte"[^rfc8259-obj]; RFC 8259 chỉ nêu rằng với phần mềm double (binary64) thì số nguyên trong $[-(2^{53})+1,\ 2^{53}-1]$ là interoperable[^rfc8259-limits][^rfc8259-num], và không cấm key trùng[^rfc8259-dup]; CSV thậm chí không có đặc tả chính thức[^rfc4180-nospec]. Vì thế khi cần hợp đồng chặt và tiến hóa được, người ta dùng định dạng nhị phân có schema (Protobuf, Avro).

**Giải thích.**

Hình dung (nhận xét của người soạn): service Rust của bạn có một `struct Order` đang nằm trong heap, có con trỏ tới `String`, `Vec`. Gửi nguyên vùng nhớ đó cho service khác là vô nghĩa: địa chỉ con trỏ chỉ có ý nghĩa trong process của bạn, và layout của struct còn do compiler quyết. Bạn phải "dịch" nó ra một dãy byte có quy ước chung, bên kia dịch ngược lại. Quy ước chung đó là *encoding format*.

*Mức cơ bản — vì sao không gửi thẳng struct.* Bài giảng Cambridge mô tả RPC: stub ở phía gọi mã hóa các đối số vào một message gửi sang node kia; quá trình mã hóa đối số gọi là marshalling[^cam-marshal]. Với Rust, `repr(Rust)` — layout mặc định của struct — chỉ cam kết vài tính chất cơ bản, ngoài ra "không có bảo đảm nào khác về data layout"[^rust-layout]; nên không thể xem bộ nhớ của struct là một định dạng trao đổi (chi tiết layout ở [P10 · Chương 02](../10-rust-internals/02-type-layout.md)). Thứ tự byte của số nhiều byte cũng là một lựa chọn phải thống nhất — xem [P2 · Chương 02, mục 4](../02-cpu/02-bieu-dien-du-lieu.md).

*Định dạng văn bản: JSON.* Theo RFC 8259, một JSON object là tập *không có thứ tự* các cặp tên/giá trị, và giá trị chỉ có thể là string, number, boolean, null, object hoặc array[^rfc8259-obj] — không có kiểu "dãy byte". Hệ quả: ai muốn nhét dữ liệu nhị phân vào JSON phải tự mã hóa thành chuỗi; Protobuf JSON mapping chọn base64 chuẩn cho trường `bytes`[^pbjson-bytes], và base64 biến mỗi nhóm 3 byte đầu vào thành 4 ký tự[^rfc4648-b64] (nên văn bản dài hơn dữ liệu gốc — suy luận của người soạn từ tỷ lệ 3→4).

*Số trong JSON.* RFC 8259 cho phép implementation giới hạn độ chính xác và miền giá trị; vì phần mềm double (IEEE 754 binary64) phổ biến, muốn interoperable thì đừng mong nhiều hơn mức đó[^rfc8259-limits]. Với phần mềm như vậy, các số nguyên trong khoảng $[-(2^{53})+1,\ 2^{53}-1]$ là interoperable: các implementation đồng ý chính xác về giá trị[^rfc8259-num] (nhận xét của người soạn: RFC không đưa bảo đảm tương tự cho số nguyên ngoài khoảng này). Tài liệu ProtoJSON nói thẳng hệ quả thực tế: nhiều implementation hiểu mọi số là binary64 và âm thầm mất chính xác với số nguyên lớn hơn $2^{53}$[^pbjson-lossy], nên serializer của ProtoJSON xuất `int64` thành *chuỗi*[^pbjson-int64]. (Nhận xét của người soạn: ID 64-bit kiểu snowflake là ví dụ điển hình bị JavaScript làm tròn nếu đi qua JSON number.) Ngoài ra `NaN` và `Infinity` không được phép trong JSON[^rfc8259-nan]; còn về văn bản, JSON trao đổi giữa các hệ thống không thuộc cùng một hệ sinh thái đóng phải dùng UTF-8[^rfc8259-utf8].

*Key trùng và thứ tự.* Khi tên trong một object không duy nhất, hành vi của phần mềm nhận là không đoán trước được[^rfc8259-dup]. Một ví dụ cụ thể ở tầng DB: PostgreSQL `json` lưu bản sao đúng nguyên văn, giữ khoảng trắng, thứ tự key và mọi cặp key/value trùng[^pg-json-exact], còn `jsonb` bỏ khoảng trắng, không giữ thứ tự key và với key trùng chỉ giữ giá trị cuối[^pg-json].

*CSV.* RFC 4180 tự nhận rằng không tồn tại đặc tả chính thức cho CSV, nên có "nhiều cách hiểu" khác nhau về file CSV; tài liệu chỉ ghi lại định dạng mà đa số implementation có vẻ theo[^rfc4180-nospec]. Dòng header là tùy chọn[^rfc4180-header]. (Suy luận của người soạn: đặc tả không có khái niệm kiểu, nên "123" là số hay chuỗi, ngày tháng theo format nào, rỗng là NULL hay chuỗi rỗng đều do hai bên tự thỏa thuận ngoài file.)

*Nhị phân có schema.* Protobuf và Avro đi theo hướng ngược lại: schema là hợp đồng được mô tả bằng ngôn ngữ riêng; code ở cả hai phía được sinh ra hoặc cấu hình từ đó. Bài giảng Cambridge gọi đó là IDL — từ IDL, lập trình viên sinh mã marshalling/unmarshalling và stub cho từng ngôn ngữ của mỗi service[^cam-idl]. Hai mục tiếp theo xem chúng mã hóa ra sao.

| Tiêu chí | JSON | CSV | Protobuf | Avro |
|---|---|---|---|---|
| Có kiểu nhị phân (bytes) | không, phải tự mã hóa (vd. base64)[^pbjson-bytes] | không có khái niệm kiểu (suy luận của người soạn; RFC 4180 không có đặc tả chính thức[^rfc4180-nospec]) | có (`bytes`, cùng wire type LEN với `string`)[^pb-enc-wtmap][^pb-enc-bytes] | có (`bytes`)[^avro-bytes] |
| Tên field có trong dữ liệu | có[^rfc8259-obj] | tùy chọn (dòng header)[^rfc4180-header] | không, chỉ số field[^pb-enc-kv] | không[^avro-noname] |
| Cần schema để đọc | không[^serde-selfdesc] | không (suy luận của người soạn) | cần để biết tên và kiểu field (file `.proto`)[^pb-enc-kv] | có, schema của writer[^avro-noname] |

<details><summary>Đi sâu: JSON trong database, dữ liệu "self-describing" và cái giá</summary>

- JSON/CBOR được gọi là *self-describing*: người đọc nhìn dữ liệu là biết nó là gì. RFC 8949 (CBOR) đặt mục tiêu thiết kế là không cần schema kèm theo hay được giả định — decoder có thể giải mã một item mà không cần biết gì thêm[^cbor-schemaless]; tuy vậy RFC nói thêm rằng trong thực tế encoder và decoder vẫn có chung cách hiểu về nội dung mong đợi của một item[^cbor-shared]. Trong hệ sinh thái Rust, serde mô tả cùng sự phân biệt: format self-describing như JSON cho phép nhìn dữ liệu mà biết nó đại diện cho cái gì, còn format như Postcard phải được *báo trước* là trong input có gì[^serde-selfdesc][^serde-nonself].
- Cái giá của self-describing là tên field lặp lại ở mọi bản ghi (suy luận của người soạn; so với Protobuf/Avro không chứa tên[^pb-enc-kv][^avro-noname]). Cái lợi là đọc được mà không cần schema, và key lạ có thể bị bỏ qua tùy ngữ cảnh (mục 8).
- `jsonb` còn có ràng buộc riêng: nó từ chối số nằm ngoài miền của kiểu `numeric` của PostgreSQL, còn `json` thì không[^pg-jsonb-num]. Nghĩa là "JSON hợp lệ" chưa chắc là "jsonb hợp lệ".

</details>

**Quay lại Q1:** encoding là hợp đồng giữa hai chương trình về cách biến cấu trúc thành byte; JSON/CSV tiện nhưng lỏng (số chỉ chắc chắn đến $2^{53}$, không có bytes, key trùng không xác định, CSV không có chuẩn), nên hệ thống đòi hỏi chặt chẽ chuyển sang định dạng nhị phân có schema.

## 2. Protocol Buffers: field number và wire format (Q2)

> **Trả lời ngắn.** Một message Protobuf là chuỗi các cặp khóa–giá trị, trong đó khóa chỉ là *số hiệu field* — tên và kiểu khai báo chỉ có trong file `.proto`[^pb-enc-kv]. Mỗi bản ghi gồm field number, *wire type* và payload; wire type cho parser biết payload lớn bao nhiêu, nên parser cũ bỏ qua được field mới mà nó không hiểu[^pb-enc-skip]. Field vắng mặt thì đơn giản là không được ghi[^pb-enc-missing].

**Giải thích.**

Hình dung (nhận xét của người soạn): thay vì điền biểu mẫu "Họ tên: ..., Tuổi: ...", bạn gửi một dãy phong bì dán nhãn số: "phong bì #1 chứa 150", "phong bì #2 chứa 'testing'". Người nhận có sẵn bảng tra "#1 là tuổi, #2 là họ tên"; phong bì có nhãn số lạ thì đặt sang một bên mà không phải mở. Số nhãn đó chính là field number.

*Một ví dụ nhỏ.* Với `message Test1 { int32 a = 1; }` và `a = 150`, tài liệu cho biết bản mã hóa là ba byte `08 96 01`[^pb-enc-simple]. Hai thành phần cần hiểu: varint và tag.

*Varint.* Varint (số nguyên độ dài thay đổi) mã hóa số nguyên không dấu 64-bit trong 1 đến 10 byte, số nhỏ dùng ít byte hơn[^pb-enc-varint]. Mỗi byte dùng bit cao nhất làm *continuation bit* báo "còn byte tiếp theo"; 7 bit thấp là payload, ghép theo thứ tự little-endian các nhóm 7 bit[^pb-enc-varint2]. Với 150, hai byte `96 01` giải mã như sau: bỏ bit cao của từng byte được `0010110` và `0000001`; đảo thứ tự nhóm rồi nối lại được `0000001 0010110` = 150[^pb-enc-150].

*Tag.* "Tag" của mỗi bản ghi là một varint được ghép từ field number và wire type theo công thức `(field_number << 3) | wire_type`[^pb-enc-tag]. Byte `08` = `0000 1000`: 3 bit thấp là `000` (wire type 0 = VARINT), phần còn lại dịch phải 3 bit ra field number 1; sau đó đọc varint `96 01` = 150[^pb-enc-simple] (phép tính từ công thức tag, suy luận của người soạn).

*Wire type.* Có sáu loại: VARINT, I64, LEN, SGROUP, EGROUP, I32[^pb-enc-wt]. VARINT dùng cho `int32`, `int64`, `uint32`, `uint64`, `sint32`, `sint64`, `bool`, `enum`; I64 cho `fixed64`, `sfixed64`, `double`; LEN cho `string`, `bytes`, message lồng nhau và repeated field dạng packed; I32 cho `fixed32`, `sfixed32`, `float`; SGROUP/EGROUP là group đã deprecated[^pb-enc-wtmap]. Với LEN, ngay sau tag là một varint chỉ độ dài, rồi đến payload[^pb-enc-lendef]. Ví dụ `string b = 2` bằng `"testing"` là `12 07 74 65 73 74 69 6e 67`: tag `12` = field 2 kiểu LEN, độ dài 7, rồi 7 byte UTF-8[^pb-enc-len].

```mermaid
flowchart LR
  subgraph rec["Một bản ghi (record)"]
    direction LR
    tag["tag (varint):<br/>field_number và wire_type ghép lại"] --> payload["payload<br/>VARINT: varint<br/>I32/I64: 4/8 byte<br/>LEN: độ dài (varint) + dữ liệu"]
  end
  msg["message = chuỗi các record,<br/>field vắng mặt thì không có record"] --> rec
```

(Sơ đồ là minh họa của người soạn từ công thức tag và quy tắc LEN ở trên, không phải hình trích từ nguồn.)

*Vì sao parser cũ bỏ qua được field mới.* Sơ đồ TLV (tag–length–value) cho phép parser biết payload dài bao nhiêu mà không cần hiểu field; tài liệu nói rõ đây là lý do parser cũ nhảy qua được field mới[^pb-enc-skip]. (Nhận xét của người soạn: nghĩa là khả năng bỏ qua field mới đã nằm sẵn trong định dạng, không phải phép màu của thư viện; tiến hóa schema ở mục 3 dựa vào đó.)

*Số âm.* Các kiểu `intN` mã hóa số âm bằng two's complement, nên khi coi là số không dấu 64-bit thì bit cao nhất được bật và cần đủ 10 byte[^pb-enc-neg]. Các kiểu `sintN` dùng ZigZag: số dương $p$ thành $2p$, số âm $n$ thành $2|n|-1$[^pb-enc-zigzag] (nhận xét của người soạn: nhờ vậy số có trị tuyệt đối nhỏ thì mã hóa ngắn; nếu trường hay mang số âm, chọn `sint32`/`sint64`, nhưng đổi kiểu giữa `int32` và `sint32` trên field đang dùng là phá dữ liệu — mục 3).

*Thứ tự, trùng lặp, không canonical.* Field number có thể khai báo theo thứ tự tùy ý trong `.proto` và thứ tự đó không ảnh hưởng việc serialize[^pb-enc-order]. Không có bảo đảm về thứ tự ghi các field; parser phải đọc được mọi thứ tự[^pb-enc-noorder]. Nếu một field không phải repeated xuất hiện nhiều lần, với số và chuỗi parser lấy giá trị cuối cùng[^pb-enc-last]. Hệ quả thực tế: **không dùng byte đã serialize làm khóa cache hay để so sánh**; tài liệu cảnh báo đừng giả định byte output của message serialize là ổn định[^pb-enc-stable], và best practices nhắc rằng tính ổn định không được bảo đảm giữa các binary hay các lần build[^pbdos-stable].

<details><summary>Đi sâu: kích thước tag, packed, field 19000–19999, giới hạn 29 bit</summary>

- Field number nằm trong khoảng 1 đến 536,870,911; dải 19,000–19,999 dành riêng cho implementation của Protobuf[^pb3-num][^pb3-19000]. Giới hạn 29 bit (thay vì 32) vì 3 bit dành cho wire type[^pb3-29bits].
- Field 1–15 chỉ tốn 1 byte cho tag, field 16–2047 tốn 2 byte[^pb3-1to15]. (Suy luận của người soạn từ công thức tag: field 16 → tag $16\cdot 8+0=128$, cần 2 byte varint vì 128 > 127.) Do đó nên dành số 1–15 cho field hay xuất hiện[^pb3-1to15].
- Repeated field kiểu số (scalar không phải string/bytes) mặc định *packed* trong proto3[^pb3-packed] và trong Edition 2023[^pb-enc-packed0], tức là cả danh sách là một record LEN chứa các phần tử nối nhau. Parser vẫn phải đọc được cả dạng không packed lẫn nhiều record packed rời nhau, nên thêm `[packed=true]` vào field có sẵn là thay đổi tương thích cả hai chiều[^pb-enc-packed].
- Với message lồng: trường message dùng wire type LEN, bên trong là các record như message thường[^pb-enc-sub]. Nếu cùng một field message xuất hiện nhiều lần, parser *merge* các lần xuất hiện; kết quả là parse chuỗi nối hai message sinh ra cùng kết quả như parse riêng rồi merge[^pb-enc-merge].
- `map<K,V>` được mã hóa gần như repeated message với hai field mỗi phần tử; thứ tự phần tử map không được bảo đảm khi serialize[^pb-enc-map].
- Message serialize phải nhỏ hơn 2 GiB[^pb-enc-limit].

</details>

**Quay lại Q2:** Protobuf ghi `(field_number, wire_type, payload)` cho từng field có mặt; số hiệu thay tên, wire type cho biết độ dài, nên định dạng gọn mà parser vẫn nhảy qua được field nó không biết.

## 3. Protobuf: đổi schema thế nào cho đúng (Q3)

> **Trả lời ngắn.** Thêm field mới và xóa field là an toàn *nếu* không bao giờ dùng lại số hiệu của field đã xóa[^pb3-safe-add][^pb3-safe-remove]; đổi số hiệu field tương đương xóa field cũ và thêm field mới[^pb3-change]. Một số đổi kiểu chỉ "tương thích có điều kiện" (có thể cắt mất bit) và đòi hỏi quản lý rollout[^pb3-compat-int][^pb3-compat-rollout]. Dùng lại số hiệu làm việc giải mã mơ hồ vì định dạng không có cách nhận ra field được mã hóa bằng định nghĩa này nhưng giải mã bằng định nghĩa khác[^pb3-lean].

**Giải thích.**

Hình dung (nhận xét của người soạn): số hiệu field giống số căn cước trong một sổ đăng ký đã in ra hàng triệu bản — log cũ, message trong queue, file đã lưu. Bạn đổi nghĩa của "số 4" thì mọi bản in cũ bị đọc sai mà không có báo lỗi nào.

*Những thay đổi "wire-safe".* Tài liệu chia thay đổi thành ba nhóm. Nhóm an toàn hoàn toàn gồm: thêm field mới — message do code cũ serialize vẫn parse được bằng code mới[^pb3-safe-add], và binary cũ bỏ qua field mới khi parse[^pb3-safe-add2]; xóa field, miễn là không dùng lại số hiệu đó (nên đổi tên thành `OBSOLETE_` hoặc `reserved`)[^pb3-safe-remove]; thêm giá trị vào enum[^pb3-safe-enum]. Lưu ý tài liệu cũng cảnh báo: thay đổi wire-safe vẫn có thể làm gãy *code ứng dụng* (ví dụ `switch` vét cạn trên enum không còn vét cạn)[^pb3-safe-app].

*Nhóm "wire-unsafe".* Đổi field number của field đang tồn tại là không an toàn; chuyển field vào một `oneof` đã có cũng vậy[^pb3-unsafe-num][^pb3-unsafe-oneof]. Số hiệu không thể đổi khi message đã được dùng vì nó định danh field trên wire[^pb3-immut]; "đổi" số hiệu thực chất là xóa field rồi thêm field cùng kiểu với số mới[^pb3-change].

*Nhóm "wire-compatible" có điều kiện.* Đây là các thay đổi mà dữ liệu parse được cả trước lẫn sau, nhưng có thể mất thông tin: ví dụ `int32` → `int64` tương thích, nhưng nếu ghi giá trị lớn hơn `INT32_MAX` thì client đọc bằng `int32` bỏ mất các bit cao[^pb3-compat-int]. `int32`, `uint32`, `int64`, `uint64`, `bool` tương thích với nhau; `sint32`/`sint64` tương thích với nhau nhưng *không* với các kiểu số nguyên còn lại[^pb3-compat-sint]. Những đổi này chỉ nên làm nếu bạn quản lý được rollout (ví dụ vẫn chỉ ghi giá trị hợp lệ cho `int32` cho đến khi schema mới ở mọi endpoint)[^pb3-compat-rollout], và với schema công bố ra ngoài tổ chức thì nhìn chung không nên[^pb3-compat-pub]. Best practices còn khuyên "gần như đừng bao giờ" đổi kiểu field, vì ngay cả khi schema mới parse được dữ liệu cũ, việc rollout vẫn khó an toàn[^pbdos-type].

*Xóa field cho đúng.* Khi xóa, phải `reserved` số hiệu[^pb3-reserve]. Nên reserve cả tên: dùng lại tên cũ nói chung an toàn, trừ khi dùng TextProto hoặc JSON — nơi tên field được serialize[^pb3-reserve-name]; lưu ý reserved name chỉ ảnh hưởng hành vi của compiler `protoc`, không ảnh hưởng runtime[^pb3-reserve-compiler]. Cú pháp ví dụ: `reserved 2, 15, 9 to 11;`, dải số tính cả hai đầu[^pb3-reserved-syntax]. Lý do thực tế để không dùng lại số: nếu thay đổi từng chạy trên production, có thể còn bản serialize của proto trong log nào đó, hay code cũ trong server khác sẽ hỏng[^pbdos-reuse].

*Hậu quả khi tái dùng số hiệu.* Tài liệu liệt kê: tốn thời gian debug, lỗi parse/merge (trường hợp tốt nhất), rò rỉ PII, hỏng dữ liệu[^pb3-reuse]. Nguyên nhân hay gặp: "đánh số lại cho đẹp" (renumbering thực chất xóa và thêm lại mọi field liên quan) và xóa field mà không reserve[^pb3-reuse-causes].

*Field vắng mặt và default.* Wire format chỉ chứa giá trị có mặt; với field implicit presence thì giá trị bằng default không được serialize[^pbpres-implicit-ser], nên sau khi parse không phân biệt được "đặt rõ là `false`" với "không đặt"[^pb3-implicit]. Do đó phải thận trọng khi đặt ý nghĩa cho giá trị mặc định; best practices nói gần như đừng đổi default của một field, vì gây lệch phiên bản giữa client và server[^pbdos-default] (cùng đoạn ghi rõ proto3 đã bỏ khả năng tự đặt default[^pbdos-default-proto3]). Với enum, giá trị khai báo đầu tiên nên là `..._UNSPECIFIED` với tag 0[^pbdos-enum-unspec], và nên tránh gán cho nó ý nghĩa nghiệp vụ[^pbdos-enum-nomean]. Giá trị enum lạ khi giải mã vẫn được giữ trong message (cách biểu diễn tùy ngôn ngữ)[^pb3-enum-open].

*Field `required`.* Best practices: đừng bao giờ thêm field required, vì có thể một ngày nó không còn đúng về nghiệp vụ nhưng proto vẫn bắt phải điền[^pbdos-required]; proto3 bỏ hẳn required[^pbdos-required2], nên lời khuyên này nhắm tới proto2/Editions, còn với proto3 nguồn ghi là không áp dụng[^pbdos-required3].

*Unknown field.* Khi binary cũ parse dữ liệu của binary mới, các field mới trở thành unknown fields trong binary cũ[^pb3-unknown]; proto3 giữ chúng và ghi lại khi serialize[^pb3-unknown-keep]. Nhưng có hành động làm mất chúng: serialize sang JSON, hoặc duyệt từng field để điền vào message mới[^pb3-unknown-lose]; nên dùng API theo message như `CopyFrom()`/`MergeFrom()` để sao chép[^pb3-unknown-copy]. (Nhận xét của người soạn: đây là bẫy "proxy/gateway đọc–sửa–ghi": service trung gian dùng schema cũ mà copy field-by-field thì âm thầm cắt mất field mới.)

*Đừng dùng text/JSON để trao đổi nếu cần tiến hóa.* Format văn bản biểu diễn field và enum bằng chuỗi nên code cũ parse sẽ lỗi khi một field/giá trị enum bị đổi tên hay thêm mới[^pbdos-text]; ProtoJSON không hỗ trợ unknown field và đưa tên field/enum vào message, nên khó đổi tên về sau, còn xóa field là breaking change gây lỗi parse[^pbjson-evol].

*Tách message cho RPC và cho lưu trữ.* Best practices khuyên dùng message khác nhau cho API RPC và cho lưu trữ dài hạn vì nhu cầu của hai thứ về sau sẽ phân kỳ[^pbdos-rpc-storage]. (Nhận xét của người soạn: thực tế là đặt lớp mapping giữa "DTO API" và "DTO lưu trữ", chấp nhận chi phí viết mapping để mỗi bên tự tiến hóa.)

<details><summary>Đi sâu: bảng quy tắc ngắn và ví dụ tiến hóa một message</summary>

| Thay đổi | Mức | Ghi chú |
|---|---|---|
| Thêm field mới | wire-safe | binary cũ bỏ qua[^pb3-safe-add2] |
| Xóa field (reserve số hiệu) | wire-safe | không tái dùng số[^pb3-safe-remove] |
| Thêm giá trị enum | wire-safe | có thể làm gãy code `switch` vét cạn[^pb3-safe-app] |
| `int32` ↔ `int64` ↔ `uint*` ↔ `bool` | compatible | có thể cắt bit[^pb3-compat-int] |
| `int32` ↔ `sint32` | không tương thích | `sintN` chỉ tương thích với nhau[^pb3-compat-sint] |
| Đổi field number | unsafe | = xóa + thêm[^pb3-change] |
| Chuyển field vào `oneof` đã có | unsafe | [^pb3-unsafe-oneof] |

Ví dụ tiến hóa (minh họa, không phải output thật):

```proto
// v1
message Order {
  int64 id = 1;
  string customer = 2;
  string legacy_note = 3;
}

// v2: bỏ legacy_note, thêm currency; không bao giờ dùng lại số 3.
message Order {
  reserved 3;
  reserved "legacy_note";
  int64 id = 1;
  string customer = 2;
  string currency = 4;
}
```

Nếu v1 đọc bản ghi do v2 viết: field 4 là unknown, được bỏ qua (hoặc giữ lại)[^pb3-unknown]; field 3 không còn trong dữ liệu mới nên v1 thấy giá trị mặc định (suy luận của người soạn từ "field vắng mặt thì không có record"[^pb-enc-missing]). Nếu v2 đọc bản ghi cũ do v1 viết: field 3 là unknown với v2; `currency` vắng mặt nên mang giá trị mặc định[^pb3-safe-add].

</details>

**Quay lại Q3:** thêm/xóa field an toàn nhờ parser bỏ qua field lạ; điều kiện sống còn là số hiệu đã dùng thì không bao giờ được dùng lại với nghĩa khác, và mọi đổi kiểu phải coi là phải quản lý rollout.

## 4. Avro: writer schema, reader schema và schema resolution (Q4)

> **Trả lời ngắn.** Dữ liệu Avro nhị phân không chứa tên field hay thông tin kiểu, nên phải có schema mới đọc được[^avro-noname]; vì vậy nơi lưu dữ liệu Avro nên kèm *writer's schema*[^avro-store]. Người đọc có thể cung cấp thêm *reader's schema* khác; khi đó "schema resolution" so khớp field theo *tên* và điền default cho field mới[^avro-resolve-names][^avro-resolve-default].

**Giải thích.**

Hình dung (nhận xét của người soạn): Protobuf giống phiếu có số nhãn nên ai cũng đọc được nhãn, còn Avro giống một dãy giá trị trần "27, foo" kèm một *bản mô tả cột* đi theo. Người đọc có bản mô tả cột của mình; hai bản mô tả (của người ghi và người đọc) được đặt cạnh nhau và đối chiếu theo tên cột.

*Encoding gọn nhất có thể.* Record Avro được mã hóa bằng cách nối các giá trị field theo đúng thứ tự khai báo[^avro-record]. Với record `{a: long, b: string}`, `a = 27` (hex `36`) và `b = "foo"` (hex `06 66 6f 6f`) thì kết quả đơn giản là `36 06 66 6f 6f`[^avro-example]. `int`/`long` dùng zig-zag có độ dài thay đổi[^avro-zigzag]; chuỗi là một `long` độ dài rồi các byte UTF-8[^avro-string]. Không có tag, không có tên: người đọc "wholly reliant" vào schema đã dùng khi ghi[^avro-reliant].

*Vì vậy writer's schema phải đi cùng dữ liệu.* Đặc tả: file hay hệ thống lưu Avro nên luôn gồm writer's schema; hệ RPC dựa trên Avro phải bảo đảm bên nhận có bản sao schema mà bên gửi đã dùng[^avro-store]. Ba cách thực tế:

- *File:* object container file có header chứa schema, mọi object trong file ghi theo schema đó[^avro-container].
- *RPC:* bước handshake bảo đảm client và server có định nghĩa protocol của nhau để giải mã đúng[^avro-handshake].
- *Bản ghi lẻ lưu lâu* (ví dụ vài tuần trong topic Kafka[^avro-single-kafka]): định dạng single-object gồm marker hai byte `C3 01`, theo sau là fingerprint CRC-64-AVRO 8 byte của schema rồi dữ liệu Avro[^avro-single]. Đặc tả nêu rõ lý do: sau khi schema đổi, hệ lưu trữ chứa bản ghi viết bằng nhiều schema khác nhau, và schema thường quá lớn để nhúng vào từng message[^avro-single-why].

*Schema resolution.* Gọi schema dùng để ghi là *writer's schema*, schema mà ứng dụng mong đợi là *reader's schema*[^avro-resolve-defs]. Với hai record cùng tên, thứ tự field có thể khác vì field được so khớp theo tên[^avro-resolve-names]. Quy tắc cho record:

1. Field có trong writer nhưng không có trong reader: giá trị của writer bị bỏ qua[^avro-resolve-ignore].
2. Field có trong reader, không có trong writer, và reader có default: dùng default của reader[^avro-resolve-default].
3. Field có trong reader, không có trong writer, reader *không* có default: báo lỗi[^avro-resolve-err].

Một số kiểu cũng được "thăng hạng" khi đọc, ví dụ `int` → `long`, `float`, `double`[^avro-promote]. Enum cũng tiến hóa được: nếu reader gặp symbol của writer mà mình không có, và reader khai báo `default` cho enum thì dùng default đó, nếu không thì lỗi[^avro-enum-default].

*Default chỉ phục vụ lúc đọc.* Đặc tả nhấn mạnh default của field "chỉ dùng khi đọc bản ghi thiếu field, cho mục đích schema evolution" và sự hiện diện của default không làm field thành tùy chọn lúc mã hóa[^avro-default-def]. Trường tùy chọn thường khai báo bằng union có `null` (nhận xét của người soạn; xem ví dụ union `["null","string"]` ở phần đi sâu); default của field union ứng với schema đầu tiên khớp trong union[^avro-union-default]. Ví dụ, thêm một field vào reader schema, kèm default, thì đọc được dữ liệu cũ (backward) — đây chính là mô hình dùng ở mục 5.

*Đổi tên.* Field/kiểu có thể có `aliases`; implementation có thể (tùy chọn) dùng alias để ánh xạ writer's schema vào reader's schema, phục vụ cả schema evolution lẫn xử lý bộ dữ liệu khác nguồn[^avro-alias].

*So với Protobuf.* (Tổng hợp của người soạn từ mục 3 và 4.) Protobuf "chống chịu" bằng *số hiệu* nên writer không cần gửi schema nhưng mọi bên phải giữ discipline số hiệu (mục 3); Avro "chống chịu" bằng *so khớp tên + default* nên không cần số hiệu, đổi lại phải biết schema của writer lúc đọc. (Nhận xét của người soạn: vì không có số hiệu, Avro hợp với dữ liệu sinh ra tự động từ schema động, như dump bảng DB, và với batch/stream nơi có file hoặc registry giữ schema; Protobuf hợp với RPC nơi hai bên đã build chung IDL.)

<details><summary>Đi sâu: block array/map, sort order, canonical form, zig-zag của <code>64</code></summary>

- Array/map mã hóa thành các *block*: mỗi block có `long` đếm rồi từng phần tử, block đếm 0 là kết thúc; nếu đếm âm thì theo sau có kích thước block tính bằng byte để nhảy qua nhanh, ví dụ khi chiếu (project) record về tập con field[^avro-blocks].
- Union mã hóa bằng `int` chỉ vị trí nhánh trong union, rồi giá trị theo nhánh đó; `["null","string"]` mã hóa `null` thành `00` và chuỗi `"a"` thành `02 02 61`[^avro-union].
- Hai schema "giống nhau" theo nghĩa đọc dữ liệu được định nghĩa bằng *Parsing Canonical Form*[^avro-canon], dạng này chỉ giữ các thuộc tính liên quan đến parse và bỏ phần còn lại như `doc`, `aliases`[^avro-canon-strip]. Fingerprint trong single-object encoding là fingerprint của schema[^avro-single].
- Với `long` zig-zag: ví dụ bảng của đặc tả cho `64` là `80 01` và `-64` là `7f`[^avro-zigzag-tbl].
- JSON encoding của Avro khác binary ở union: giá trị union (không null) viết thành object một cặp {tên kiểu: giá trị}[^avro-json-union]; và JSON encoding vẫn cần schema gốc để xử lý, vì nó không phân biệt `int`/`long`, `float`/`double`, record/map...[^avro-json]

</details>

**Quay lại Q4:** Avro mã hóa không tên, không tag; đọc đúng nhờ có writer's schema và tiến hóa nhờ resolution theo tên với default của reader.

## 5. Backward/forward compatibility và rolling upgrade (Q5)

> **Trả lời ngắn.** *Backward compatible*: code/schema mới đọc được dữ liệu do bản cũ ghi. *Forward compatible*: code/schema cũ đọc được dữ liệu do bản mới ghi, dù không dùng hết khả năng của bản mới[^conf-backward][^conf-forward]. Khi nâng cấp từng node một[^cam-rolling], nhiều phiên bản của ứng dụng chạy cùng lúc[^k8s-multi] (suy luận của người soạn: trên cùng dữ liệu), và cả client lẫn server có thể bị rollback[^pbdos-skew], nên cần cả hai chiều.

**Giải thích.**

Hình dung (nhận xét của người soạn): bạn thay đổi cách ghi sổ ở cả chi nhánh. Trong vài giờ chuyển giao, một số nhân viên đã dùng mẫu sổ mới, số khác vẫn dùng mẫu cũ; sổ cũ và mới nằm lẫn trong cùng một tủ. Nhân viên mới phải đọc được sổ cũ (backward), nhân viên cũ không được "nổ" khi gặp trang sổ mới (forward).

*Định nghĩa.* Theo tài liệu Confluent Schema Registry: BACKWARD nghĩa là consumer dùng schema mới đọc được dữ liệu do schema trước đó tạo ra[^conf-backward]; FORWARD nghĩa là dữ liệu tạo bởi schema mới đọc được bởi consumer dùng schema trước, dù consumer có thể không dùng được toàn bộ khả năng của schema mới[^conf-forward]; FULL là cả hai[^conf-full]. (Nhận xét của người soạn: chương này dùng đúng các định nghĩa ấy; cần kiểm tra lại khi đọc tài liệu khác, vì cách gọi tên có thể không thống nhất giữa các nguồn.) Bản "transitive" kiểm tra với mọi phiên bản trước, không chỉ bản liền trước[^conf-trans].

*Rolling upgrade.* Bài giảng Cambridge: nếu một service chịu được 1 trong 3 node không khả dụng thì có thể nâng cấp bằng cách cài và restart từng node một trong khi hai node còn lại tiếp tục phục vụ; mục đích là nâng cấp mà client không thấy gián đoạn[^cam-rolling]. Kubernetes Deployment với chiến lược `RollingUpdate` cập nhật Pod theo kiểu rolling update, dần giảm ReplicaSet cũ và tăng ReplicaSet mới[^k8s-rolling], và nhiều phiên bản của ứng dụng có thể chạy cùng lúc trong lúc đó[^k8s-multi]. (Suy luận của người soạn: trong khoảng thời gian đó, tồn tại đồng thời instance cũ và mới đọc/ghi cùng database, cùng topic, gọi nhau qua RPC.)

```mermaid
flowchart LR
  subgraph tg["Trong lúc rolling upgrade"]
    old["instance v1"]
    new["instance v2"]
  end
  store[("DB / topic / file")]
  old -- "ghi dữ liệu v1" --> store
  new -- "ghi dữ liệu v2" --> store
  store -- "v2 đọc dữ liệu v1: BACKWARD" --> new
  store -- "v1 đọc dữ liệu v2: FORWARD" --> old
```

(Sơ đồ là minh họa của người soạn, dựa trên định nghĩa BACKWARD/FORWARD của Confluent ở trên.)

*Vì sao cần cả hai chiều.* Có hai lý do. Thứ nhất, trong lúc rollout, v1 và v2 cùng sống nên v1 có thể đọc dữ liệu của v2 và ngược lại. Thứ hai, Protobuf best practices: client và server không bao giờ được cập nhật chính xác cùng lúc, kể cả khi cố, và một trong hai có thể bị rollback, nên không giả định có thể phá tương thích chỉ vì hai phía "đồng bộ"[^pbdos-skew]. Rollback biến một chiều tương thích thành bắt buộc cả hai (suy luận của người soạn).

*Thứ tự nâng cấp.* Confluent nêu hệ quả của chế độ tương thích lên thứ tự nâng cấp: với BACKWARD, không có bảo đảm consumer cũ đọc được dữ liệu mới, nên nâng cấp hết consumer trước khi bắt đầu produce sự kiện mới[^conf-order]; với FORWARD, nâng cấp producer trước và bảo đảm dữ liệu cũ không còn cho consumer, rồi mới nâng cấp consumer[^conf-order-fwd]; với FULL, producer và consumer nâng cấp độc lập[^conf-order-full]. Khi cần đổi không tương thích (ví dụ Number → String), phải nâng cấp cùng lúc mọi producer và consumer, hoặc "nhiều khả năng hơn" là tạo topic mới và chuyển dần ứng dụng sang[^conf-newtopic].

*Mẫu "mở rộng rồi thu hẹp".* (Nhận xét của người soạn, rút từ các quy tắc trên.) Đổi schema không phá vỡ thành các bước: (1) triển khai code *đọc* được field mới (nhưng chưa ghi) — backward/forward cho dữ liệu hiện hữu; (2) khi mọi bản đã đọc được, triển khai code *ghi* field mới; (3) rất lâu sau, khi chắc chắn không còn dữ liệu hay instance cũ, mới xóa field (và reserve số hiệu). Tài liệu Protobuf nói cùng tinh thần cho đổi kiểu: chỉ ghi giá trị hợp lệ cho schema cũ cho đến khi schema mới ở mọi endpoint, rồi mới ghi giá trị lớn hơn[^pb3-compat-rollout]; Confluent nói tương tự cho thứ tự producer/consumer[^conf-order].

<details><summary>Đi sâu: vì sao Kafka chọn BACKWARD làm mặc định; ai "đúng" cho từng định dạng</summary>

- Schema Registry mặc định BACKWARD: lý do được nêu là để có thể *tua lại* consumer về đầu topic; với FORWARD không bảo đảm đọc được thông điệp cũ, và FORWARD khó dùng vì phải đoán trước mọi thay đổi tương lai[^conf-rewind].
- Với Protobuf, Confluent khuyên dùng BACKWARD_TRANSITIVE vì thêm kiểu message mới không forward compatible[^conf-pb-trans].
- Confluent mô tả quy tắc mỗi định dạng khác nhau: trang tài liệu viết cho Avro, vốn có đặc tả quy tắc backward rõ ràng; riêng JSON Schema và Protobuf thì quy tắc "tinh tế hơn"[^conf-nuance].
- Ở Protobuf JSON: thêm field là tương thích *nhưng* parser JSON thường không giữ unknown field, nên client cũ gặp field mới có thể lỗi parse, trừ khi client đã bật cờ ignore-unknown[^pbjson-unknown][^pbjson-ignore]. Điều này liên hệ với `deny_unknown_fields` ở mục 8.

</details>

**Quay lại Q5:** backward = mới đọc cũ, forward = cũ đọc mới; rolling upgrade và rollback khiến hai phiên bản cùng tồn tại trên cùng dữ liệu, nên phải đảm bảo cả hai chiều hoặc nâng cấp theo thứ tự chặt.

## 6. Dataflow qua database (Q6)

> **Trả lời ngắn.** Ghi vào database là gửi thông điệp cho chính "bạn của tương lai": bản ghi có thể sống lâu hơn nhiều phiên bản code, nên code mới phải đọc được dữ liệu cũ (backward) và code cũ đang chạy trong lúc rollout không được làm mất dữ liệu mới (forward) (nhận xét của người soạn). PostgreSQL hỗ trợ phần backward rẻ — thêm cột với default không volatile không cần viết lại bảng[^pg-addcol] — còn phần forward phụ thuộc vào việc code của bạn có giữ nguyên field lạ khi đọc–sửa–ghi hay không (suy luận của người soạn từ nguyên lý unknown field của Protobuf[^pb3-unknown-lose]).

**Giải thích.**

Hình dung (nhận xét của người soạn): một dòng trong bảng `orders` tạo từ ba năm trước bởi phiên bản code mà không ai còn nhớ; hôm nay đọc nó lên bằng bản mới nhất. Khác với RPC (thông điệp sống vài mili-giây) và message broker (sống vài ngày hoặc đến hết retention), dữ liệu trong DB có thể "sống" mãi.

*Schema DB là một dạng schema evolution có sẵn.* PostgreSQL: khi `ADD COLUMN` với `DEFAULT` không volatile, giá trị default được tính một lần lúc chạy lệnh và lưu trong metadata của bảng; giá trị đó dùng cho mọi hàng hiện có, nếu không có `DEFAULT` thì dùng NULL, và cả hai trường hợp đều không cần viết lại bảng[^pg-addcol]. (Nhận xét của người soạn: cơ chế này gần với ý "reader's default" của Avro — hàng cũ không bị sửa, người đọc điền default khi thiếu.) Ngược lại, thêm cột với default *volatile* hay đổi kiểu cột đòi viết lại toàn bộ bảng và index[^pg-rewrite], trừ ngoại lệ khi `USING` không đổi nội dung cột và kiểu cũ binary coercible sang kiểu mới (khi đó không cần viết lại bảng)[^pg-rewrite-exc]. `DROP COLUMN` không xóa vật lý cột mà chỉ làm nó vô hình với SQL[^pg-dropcol], nên nhanh nhưng không giảm ngay dung lượng trên đĩa[^pg-dropcol-fast].

*Rủi ro forward: ghi đè field lạ.* Dữ liệu mới (có cột/field mới) có thể được code cũ đọc lên, sửa, rồi ghi lại. Nếu code cũ chuyển bản ghi sang một cấu trúc chỉ có các field nó biết rồi ghi ngược, field mới bị mất. Tài liệu Protobuf nêu đúng nguy cơ này ở mức message: serialize sang JSON hoặc copy từng field vào message mới làm mất unknown fields[^pb3-unknown-lose], và khuyên dùng API kiểu `CopyFrom`/`MergeFrom`[^pb3-unknown-copy]. (Suy luận của người soạn: cùng nguyên lý áp dụng cho ORM hay cho cột JSON — nếu bạn deserialize cột JSON vào struct Rust không giữ field lạ rồi ghi ngược cả cột, field lạ biến mất; `UPDATE` chỉ đặt cột bạn biết thì an toàn hơn.)

*Cột JSON.* `json` giữ nguyên văn bản[^pg-json-exact], `jsonb` bỏ khoảng trắng, không giữ thứ tự key, chỉ giữ giá trị cuối của key trùng[^pg-json]. Chọn kiểu nào là chọn bạn có "giữ nguyên bản gốc" hay không (liên hệ mô hình dữ liệu: xem [Chương 02 — Mô hình dữ liệu](02-mo-hinh-du-lieu.md)).

*Lưu trữ lưu lâu theo file.* Với dump/batch, dùng định dạng file chứa schema: Avro object container file có header chứa schema, mọi object ghi theo schema đó[^avro-container], dữ liệu chia block có sync marker để chia nhỏ file cho MapReduce[^avro-sync]. Chi tiết xử lý batch xem [Chương 10](10-batch-processing.md); chuyện dữ liệu rơi vào data lake/warehouse xem [Chương 13](13-kien-truc-he-du-lieu.md).

<details><summary>Đi sâu: bẫy "data outlives code" và migration schema</summary>

- Hậu quả thực tế (nhận xét của người soạn): migration kiểu "đổi kiểu cột" hay "backfill hàng triệu hàng" nên được thiết kế như rollout hai pha giống mục 5, vì trong lúc migration code cũ và mới cùng chạy. Quy tắc PostgreSQL ở trên cho biết lệnh nào rẻ (metadata) và lệnh nào đắt (rewrite)[^pg-addcol][^pg-rewrite].
- Thêm ràng buộc `CHECK`/`NOT NULL` đòi hỏi quét bảng để xác minh các hàng hiện có nhưng không cần viết lại bảng[^pg-check].
- Replication giữa các node chạy phiên bản khác nhau cũng là một dạng dataflow qua DB (nhận xét của người soạn); nội dung replication nằm ở [Chương 05 — Replication](05-replication.md), không lặp lại ở đây.

</details>

**Quay lại Q6:** trong database, dữ liệu sống lâu hơn code, nên cần backward cho dữ liệu cũ và cẩn thận không làm mất field mới khi code cũ đọc–sửa–ghi.

## 7. Dataflow qua RPC/REST và message broker (Q7)

> **Trả lời ngắn.** Với RPC/REST, client và server là hai chương trình được triển khai độc lập nên schema giữa chúng phải tiến hóa được — gRPC mặc định dùng Protocol Buffers làm IDL[^grpc-idl]. Với message broker như Kafka, message sống lâu trong topic và consumer có thể đọc lại từ đầu (nhận xét của người soạn), nên Confluent đặt chế độ BACKWARD làm mặc định để có thể tua consumer về đầu topic[^conf-rewind]; mỗi message gắn schema ID qua một wire format riêng[^conf-wire].

**Giải thích.**

Hình dung (nhận xét của người soạn): RPC như gọi điện — hai đầu dây đang cùng "online" nhưng có thể là hai phiên bản code; broker như gửi thư qua bưu điện — người nhận có thể mở thư sau nhiều ngày, bằng phần mềm đã khác.

*RPC/REST.* Cambridge ghi nhận hình thức RPC phổ biến nhất ngày nay là JSON gửi qua HTTP[^cam-json-http]. Để nhiều ngôn ngữ cùng gọi nhau, dùng IDL để sinh code marshalling và stub; gRPC dùng Protocol Buffers làm IDL[^cam-idl]. gRPC mặc định dùng Protobuf để mô tả cả giao diện service lẫn cấu trúc payload, và cũng cho phép lựa chọn khác nếu muốn[^grpc-idl]. Vì client và server là chương trình khác nhau được phát hành riêng, mọi quy tắc ở mục 3 áp dụng trực tiếp cho message RPC; và cũng cần đến cả hai chiều tương thích ở mục 5 do thời điểm deploy không đồng bộ[^pbdos-skew].

*RPC không giống gọi hàm.* Cambridge nhấn mạnh: nếu client gửi request mà không nhận phản hồi thì không biết server đã nhận/xử lý hay chưa[^cam-rpc-fail]. gRPC có deadline: client chỉ định thời gian sẵn sàng chờ, quá hạn RPC kết thúc với `DEADLINE_EXCEEDED`[^grpc-deadline]; client và server tự xác định "thành công" cục bộ và hai kết luận có thể không khớp[^grpc-indep]; hủy RPC không rollback những thay đổi đã làm[^grpc-cancel]. Đây là cùng một ý với `timeout` ở [P7 · Chương 09, mục 2](../07-io-async/09-async-trong-production.md): ngừng chờ không phải là hủy việc phía xa. Tức là tiến hóa schema và idempotency là hai chuyện riêng nhưng cùng xuất hiện khi retry sau khi nâng cấp (nhận xét của người soạn).

*REST/JSON.* Dùng JSON thì tính tương thích trở thành quy ước ngầm: bên nhận bỏ qua field lạ (forward) và cho giá trị mặc định cho field thiếu (backward) hay báo lỗi? (Nhận xét của người soạn: quy ước này nằm ở thư viện parse — mục 8 với serde — chứ không nằm trong định dạng.) Số ID 64-bit và dữ liệu nhị phân đòi hỏi quy ước riêng ở mục 1.

*Message broker.* Với Kafka, ứng dụng dùng `Serializer<T>` — giao diện "chuyển đối tượng thành byte"[^kafka-ser]. Vì Avro nhị phân không tự mô tả[^avro-noname], hệ thống cần cách cho consumer biết schema. Confluent đặt vào đầu mỗi message một byte magic (hiện luôn là 0), tiếp theo schema ID 4 byte lấy từ Schema Registry, rồi dữ liệu đã serialize[^conf-wire]; với Protobuf, giữa schema ID và payload còn có danh sách message index[^conf-wire-pb]. Avro single-object encoding là cách chuẩn của Avro cho cùng nhu cầu — dùng fingerprint thay vì ID registry[^avro-single]. Registry kiểm tra tương thích trước khi nhận phiên bản schema mới theo chế độ cấu hình[^conf-check].

*Vì sao message "sống lâu" làm tiến hóa khó hơn.* Kafka giữ log theo thời gian/kích thước[^kafka-retention] hoặc theo compaction; compaction bảo đảm log chứa ít nhất trạng thái cuối cùng của mỗi key[^kafka-compact] — nghĩa là record viết bằng schema rất cũ có thể còn nằm trong topic (suy luận của người soạn). Confluent nêu lý do đặt BACKWARD làm mặc định là để tua consumer về đầu topic[^conf-rewind]; khi tua như vậy, consumer sẽ gặp lại cả những record cũ (suy luận của người soạn). Khi không thể tương thích: tạo topic mới[^conf-newtopic]. Chi tiết stream processing xem [Chương 11](11-stream-processing.md).

<details><summary>Đi sâu: HTTP/2 và gRPC, <code>Any</code>, giới hạn kích thước</summary>

- gRPC có bốn kiểu method: unary, server streaming, client streaming, bidirectional streaming[^grpc-kinds]. Nền HTTP/2 xem [P6 · Chương 08, mục 5](../06-networking/08-dns-http-tls.md).
- Protobuf có kiểu `Any` chứa message tùy ý kèm URL định danh kiểu[^pb3-any]; best practices khuyên ưu tiên *extensions* thay vì `Any` khi có thể, và dành `Any` cho trường hợp hạ tầng ngang phải chuyển message hoàn toàn tùy ý[^pbdos-any].
- Kafka Streams cần chế độ BACKWARD[^conf-streams]; FULL, FULL_TRANSITIVE và BACKWARD_TRANSITIVE cũng luôn được hỗ trợ vì đã bao gồm backward compatibility[^conf-streams2].

</details>

**Quay lại Q7:** RPC cần tương thích cả hai chiều vì deploy không đồng bộ; broker thêm bài toán "thông điệp sống lâu và có thể đọc lại", nên cần schema ID/registry và thường chọn BACKWARD.

## 8. Liên hệ với Rust

Phần này gắn các quy tắc ở trên với crate Rust (phiên bản docs.rs ghi trong chú thích). Ví dụ code chỉ minh họa API; không có kết quả chạy.

**serde: tiến hóa schema ở lớp ứng dụng.** serde tách *kiểu dữ liệu* khỏi *format*; thuộc tính `#[serde(...)]` điều khiển hành vi khi field thiếu hoặc thừa.

- `#[serde(default)]` (ở field) dùng `Default::default()` khi giá trị không có lúc deserialize; `default = "path"` gọi hàm `fn() -> T`[^serde-default]. Đây là "reader's default" của Avro (mục 4): đọc dữ liệu cũ thiếu field mới là thao tác *backward*.
- Field kiểu `Option<T>` vắng mặt được xử lý thành `None` (nếu không có default thì field không phải `Option` là lỗi): đó là hành vi của hàm `missing_field` trong `serde::private::de`[^serde-missing]; derive macro gọi hàm đó cho field thiếu không có default[^serde-derive-missing]. (Nhận xét của người soạn từ đọc source serde_derive: nếu field có `deserialize_with` thì field thiếu luôn là lỗi, kể cả `Option`.)
- Mặc định, field lạ bị bỏ qua với format self-describing như JSON; `#[serde(deny_unknown_fields)]` làm deserialize *lỗi* khi gặp field lạ[^serde-deny]. Dùng `deny_unknown_fields` biến forward compatibility thành lỗi parse — giống hành vi mặc định của parser ProtoJSON, vốn từ chối field lạ trừ khi có tùy chọn bỏ qua[^pbjson-ignore-default] (nhận xét của người soạn: `deny_unknown_fields` chỉ nên dùng cho cấu hình do người dùng gõ tay, nơi typo cần bị bắt; tránh cho message trao đổi giữa service). Cũng lưu ý `#[serde(flatten)]` không dùng chung với `deny_unknown_fields`[^serde-flatten].
- `#[serde(skip_serializing_if = "path")]` bỏ field khỏi output khi hàm trả `true`; ví dụ `Option::is_none` bỏ `None`[^serde-skipif]. (Nhận xét của người soạn: giúp bên đọc cũ chưa biết field không phải thấy `null` mà nó có thể không chấp nhận; nhưng nếu schema bên kia coi vắng mặt khác `null`, hãy kiểm tra hợp đồng.)
- `#[serde(alias = "name")]` cho phép field được đọc từ nhiều tên[^serde-alias]; (nhận xét của người soạn) dùng để đổi tên mà vẫn đọc dữ liệu cũ, tương tự `aliases` của Avro[^avro-alias].
- `#[serde(other)]` trên một variant unit: nhận mọi tag enum lạ — dùng cho enum tagged nội bộ/liền kề để chịu được giá trị mới[^serde-other]; (nhận xét của người soạn) vai trò gần với `default` của enum trong Avro[^avro-enum-default].
- 64-bit và JSON: `serde_json::Number` chỉ biểu diễn được số nhỏ hơn `i64::MIN` hoặc lớn hơn `u64::MAX` khi bật feature `arbitrary_precision`[^sjson-num]. Nếu client JavaScript đọc ID, hãy theo cách ProtoJSON: gửi dạng chuỗi[^pbjson-int64]; serde cho phép tự viết bằng `serialize_with`/`deserialize_with`[^serde-with].

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Order {
    id: u64,
    customer: String,
    // v2: field mới. Dữ liệu v1 không có -> dùng Default (backward).
    #[serde(default)]
    currency: String,
    // Không ghi key ra khi None: bên đọc nghiêm ngặt không gặp key `note: null`.
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}
```

**Format không self-describing: thứ tự field là schema.** Doc của serde: Postcard và các format tương tự phải được *báo* input có gì, không tự suy ra được kiểu[^serde-nonself]. Đặc tả wire format Postcard: struct được mã hóa là các phần tử theo thứ tự định nghĩa (trên xuống dưới) và tên field không nằm trong dữ liệu[^pc-struct]; từ v1.0.0 Postcard có wire format được tài liệu hóa và ổn định[^pc-stable]. (Suy luận của người soạn: do đó với Postcard, đổi thứ tự field hoặc thêm field giữa struct thay đổi byte; không có chỗ cho tiến hóa kiểu Protobuf/Avro — chỉ dùng cho dữ liệu "tạm", hoặc hai bên luôn build cùng phiên bản.)

**prost/tonic: Protobuf cho Rust.**

- prost sinh struct Rust từ `.proto` bằng `derive`, ví dụ `#[prost(string, tag="1")] pub name: ::prost::alloc::string::String`[^prost-gen]; field number nằm ở thuộc tính `tag`. Quy tắc đổi số hiệu ở mục 3 vẫn áp dụng: đổi `tag` là đổi nghĩa trên wire (nhận xét của người soạn).
- Enum `.proto` được biểu diễn bằng `i32` trong struct[^prost-i32], vì enum của Protobuf là "open" và giải mã giá trị lạ phải làm được[^prost-enum]; getter trả về giá trị mặc định của enum Rust nếu số không hợp lệ[^prost-getter]. Đây là cách prost hiện thực "enum lạ được giữ lại" ở mục 3[^pb3-enum-open]. Dùng `TryFrom<i32>` để chuyển `i32` về enum[^prost-tryfrom] (nhận xét của người soạn: nên kiểm tra giá trị lạ bằng cách này thay vì tin getter).
- Với scalar proto3 không có `optional`, kiểu Rust là `T` (thiếu thì `T::default()`); muốn biết field có mặt hay không, dùng `optional` để ra `Option<T>`[^prost-opt]. Khớp với vấn đề implicit presence ở mục 3[^pb3-implicit].
- **Unknown field.** Trong prost-derive 0.14.1, `merge_field` sinh ra có nhánh `_ => skip_field(...)` cho tag lạ[^prost-skip-derive], và `skip_field` chỉ đọc qua payload (tiến con trỏ buffer `len` byte) mà không lưu lại[^prost-skip-fn]; còn `encode_raw` sinh ra chỉ chạy qua các phần tử `#encode` ứng với field đã khai báo trong struct[^prost-encode-derive]. Suy luận của người soạn từ hai đoạn code này (chưa chạy): một service Rust dùng prost mặc định mà *decode → sửa → encode lại* một message sẽ không giữ field lạ của phiên bản mới, trái với việc proto3 chuẩn giữ unknown fields[^pb3-unknown-keep]. README prost 0.14.1 nêu việc giữ *giá trị enum* lạ nhưng không có mục nào về field lạ[^prost-readme] (nhận xét của người soạn: vắng mặt trong danh sách tính năng không phải bằng chứng tuyệt đối). Nếu service đóng vai gateway cần giữ field lạ, hãy kiểm tra lại với phiên bản prost bạn dùng.
- prost không hỗ trợ reflection hay message descriptor lúc runtime[^prost-noreflect].
- tonic là gRPC over HTTP/2 viết cho `async/await`, tập trung vào hiệu năng, interoperability và linh hoạt[^tonic-intro]; mục Getting Started của tonic chỉ sang tài liệu crate `tonic-build` để sinh code[^tonic-build], và macro `include_proto` nhúng code đã sinh[^tonic-include]. Mặc định giới hạn decode một message là 4MB còn encode là `usize::MAX`[^tonic-limit], và cả server lẫn client có thể cấu hình giới hạn này[^tonic-config] — một giới hạn vận hành riêng, độc lập với giới hạn 2 GiB của định dạng[^pb-enc-limit] (nhận xét của người soạn).

**apache-avro.** Crate `apache-avro` (docs 0.21.0) đọc dữ liệu Avro với schema được mã hóa kèm dữ liệu[^avro-rs-reader]; muốn đọc bằng reader schema khác (nhưng tương thích) thì dùng `Reader::with_schema`, và thư viện tự thực hiện schema resolution[^avro-rs-resolve]. Crate có `SchemaCompatibility::can_read` để kiểm tra tương thích[^avro-rs-compat] — (nhận xét của người soạn) hữu ích để đưa vào CI. Crate tự nhận còn ở giai đoạn beta nên API có thể đổi giữa các bản minor[^avro-rs-beta].

**Đừng dùng layout struct làm định dạng.** `repr(Rust)` không có bảo đảm layout ngoài vài tính chất cơ bản[^rust-layout]; dùng thư viện encoding thay vì `transmute`/ép con trỏ struct sang byte.

## Tóm tắt

- Dữ liệu trong bộ nhớ phải được encode thành byte để lưu/gửi; RPC gọi là marshalling[^cam-marshal]. Layout struct Rust mặc định không phải là định dạng[^rust-layout].
- JSON không có kiểu bytes; số nguyên đến $2^{53}-1$ là mức interoperable theo RFC 8259; key trùng không xác định[^rfc8259-obj][^rfc8259-num][^rfc8259-dup]; CSV không có đặc tả chính thức[^rfc4180-nospec].
- Protobuf ghi `(field number << 3 | wire type)` + payload; field vắng mặt không được ghi; parser cũ nhảy qua field lạ nhờ wire type[^pb-enc-tag][^pb-enc-skip][^pb-enc-missing].
- Số hiệu field không đổi và không tái dùng; xóa thì `reserved`; đổi kiểu chỉ trong nhóm tương thích và phải quản lý rollout[^pb3-immut][^pb3-reserve][^pb3-compat-rollout].
- Avro không mã hóa tên/tag; cần writer's schema, resolution theo tên và default của reader[^avro-noname][^avro-resolve-names][^avro-resolve-default].
- Backward: mới đọc cũ; forward: cũ đọc mới; rolling upgrade và rollback đòi cả hai[^conf-backward][^conf-forward][^pbdos-skew].
- Trong DB dữ liệu sống lâu hơn code; cẩn thận read–modify–write làm mất field lạ (suy luận từ nguyên lý unknown field của Protobuf[^pb3-unknown-lose]); `ADD COLUMN` với default không volatile không cần viết lại bảng[^pg-addcol].
- Qua broker, message sống lâu và đọc lại được; Schema Registry gắn schema ID, mặc định BACKWARD, thứ tự nâng cấp phụ thuộc chế độ[^conf-wire][^conf-rewind][^conf-order].
- Rust: `serde(default)`, `deny_unknown_fields`, `skip_serializing_if`, `alias`, `other`; prost dùng `i32` cho enum và theo code đọc được thì bỏ unknown field; `apache-avro` làm schema resolution[^serde-default][^serde-deny][^serde-skipif][^prost-enum][^avro-rs-resolve].

## Câu hỏi tự kiểm tra

1. Vì sao không thể gửi thẳng vùng nhớ của một `struct` Rust qua mạng? Nguồn nào cho biết `repr(Rust)` bảo đảm gì?
2. Một ID `u64` lớn hơn $2^{53}$ đi qua JSON tới client JavaScript. Điều gì có thể sai, và ProtoJSON xử lý thế nào?
3. Giải mã tay các byte `12 07 74 65 73 74 69 6e 67` của một message Protobuf. Field số mấy, kiểu gì, giá trị gì?
4. Byte `08` là tag; field number và wire type là gì? Tại sao 150 cần hai byte `96 01`?
5. Vì sao "đánh số lại field cho đẹp" là nguy hiểm? Làm gì khi xóa một field?
6. Đổi `int32` thành `int64` tương thích đến đâu? Đổi `int32` thành `sint32` thì sao?
7. Avro đọc `36 06 66 6f 6f` bằng cách nào khi không có tag/tên? Điều gì xảy ra nếu reader schema có thêm field có default / không có default?
8. Phân biệt backward và forward compatibility; vì sao rolling upgrade đòi cả hai?
9. Với Schema Registry, nếu chế độ là BACKWARD, nâng cấp producer hay consumer trước? Vì sao BACKWARD là mặc định cho Kafka?
10. Vì sao `#[serde(deny_unknown_fields)]` có thể làm hỏng rolling upgrade? Khi nào dùng nó là hợp lý?
11. prost xử lý enum `.proto` thế nào, và vì sao struct dùng `i32` thay vì enum Rust?
12. Service Rust dùng prost đóng vai gateway decode–encode lại message. Rủi ro nào theo code prost-derive 0.14.1, và vì sao đó chỉ là suy luận?

<details><summary>Đáp án</summary>

1. Con trỏ chỉ có nghĩa trong process; layout `repr(Rust)` chỉ có vài bảo đảm cơ bản, "không có bảo đảm nào khác"[^rust-layout]; việc đổi sang byte có quy ước là marshalling[^cam-marshal].
2. RFC 8259 chỉ nêu mức interoperable cho số nguyên đến $2^{53}-1$[^rfc8259-num]; nhiều bên coi số là binary64 và mất chính xác với số lớn hơn $2^{53}$[^pbjson-lossy]; ProtoJSON xuất `int64` thành chuỗi[^pbjson-int64].
3. `12` = `00010 010`: field 2, wire type 2 (LEN); `07` độ dài 7; 7 byte là UTF-8 `testing`[^pb-enc-len]. Kiểu `string`.
4. `08` = `00001 000`: 3 bit thấp = 0 (VARINT), phần còn lại `00001` = field 1[^pb-enc-simple][^pb-enc-tag]. 150 > 127 nên cần byte tiếp theo: `96 01` = 150[^pb-enc-150].
5. Số hiệu định danh field trên wire; đổi số = xóa + thêm field mới[^pb3-change]; tái dùng gây giải mã mơ hồ, có thể dẫn đến hỏng dữ liệu, rò PII[^pb3-reuse]. Khi xóa, `reserved` số (và tên)[^pb3-reserve].
6. Tương thích, nhưng giá trị lớn hơn `INT32_MAX` bị cắt bit khi đọc bằng `int32`[^pb3-compat-int]; `sint32` không tương thích với `int32`[^pb3-compat-sint].
7. Đọc theo schema: `long` zig-zag (36 → 27), chuỗi `long` độ dài 3 + `foo`[^avro-example]. Reader có field thêm có default thì dùng default; không có default thì lỗi[^avro-resolve-default][^avro-resolve-err].
8. Backward: schema/code mới đọc dữ liệu cũ; forward: cũ đọc mới[^conf-backward][^conf-forward]. Rolling upgrade: nâng cấp từng node một[^cam-rolling], nhiều phiên bản cùng chạy[^k8s-multi], và rollback có thể xảy ra[^pbdos-skew].
9. Nâng cấp consumer trước[^conf-order]; mặc định BACKWARD để tua consumer về đầu topic[^conf-rewind].
10. Gặp field mới từ bản mới ghi, bản cũ lỗi parse thay vì bỏ qua[^serde-deny]; hợp lý cho config gõ tay (nhận xét của người soạn).
11. Enum → `i32` vì enum Protobuf "open" và giá trị lạ phải giải mã được; getter trả default nếu giá trị không hợp lệ[^prost-enum].
12. `merge_field` bỏ tag lạ qua `skip_field` không lưu lại[^prost-skip-derive][^prost-skip-fn], còn `encode_raw` chỉ chạy qua các field đã khai báo[^prost-encode-derive]; nên field mới bị mất khi encode lại. Đây là suy luận từ đọc code derive v0.14.1, không chạy thử; cần kiểm lại với phiên bản prost đang dùng.

</details>

## Đọc thêm

- DDIA (ấn bản 1), chương 4 — "Encoding and Evolution"; tên chương và mục đầu "Formats for Encoding Data" lấy từ mục lục do ETH Library lưu[^ddia-toc]. Mục lục cũng liệt kê ba mục về luồng dữ liệu: "Dataflow Through Databases", "Dataflow Through Services: REST and RPC", "Message-Passing Dataflow"[^ddia-toc-flow] — tương ứng với mục 6–7 của chương này, còn mục 1–4 nói về định dạng (đối chiếu của người soạn). Sách không có bản miễn phí nên chương này dùng nguồn mở thay thế.
- [Protocol Buffers — Encoding](https://protobuf.dev/programming-guides/encoding/): đặc tả wire format, ví dụ `08 96 01`, varint, ZigZag.
- [Protocol Buffers — Language Guide (proto 3), "Updating A Message Type"](https://protobuf.dev/programming-guides/proto3/): ba nhóm thay đổi (safe/compatible/unsafe), reserved, unknown fields.
- [Proto Best Practices](https://protobuf.dev/best-practices/dos-donts/): danh sách "nên/không nên" khi tiến hóa schema.
- [Apache Avro 1.12.0 — Specification](https://avro.apache.org/docs/1.12.0/specification/): binary encoding, Schema Resolution, single-object encoding.
- [Confluent Schema Registry — Schema Evolution and Compatibility (7.9)](https://docs.confluent.io/platform/7.9/schema-registry/fundamentals/schema-evolution.html): chế độ tương thích và thứ tự nâng cấp client.
- [Cambridge, Distributed Systems lecture notes — mục 1.3 RPC](https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf): marshalling, IDL, lỗi RPC.
- [RFC 8259 — JSON](https://www.rfc-editor.org/rfc/rfc8259.html), mục 4 và 6: object và số, vì sao nên cẩn thận với độ chính xác.
- [serde.rs — Container/Field/Variant attributes](https://serde.rs/container-attrs.html): đầy đủ các thuộc tính trong mục 8.
- [prost 0.14.1](https://docs.rs/prost/0.14.1/prost/) và [tonic 0.14.2](https://docs.rs/tonic/0.14.2/tonic/): ánh xạ `.proto` sang Rust và gRPC trên tokio.
- [Chương 05 — Replication](05-replication.md), [Chương 10 — Batch processing](10-batch-processing.md), [Chương 11 — Stream processing](11-stream-processing.md): nơi dữ liệu mã hóa theo các định dạng này đi qua các hệ thống khác.

[^ddia-toc]: ETH Library, mục lục *Designing Data-Intensive Applications* (ISBN 978-1-4493-7332-0) — <https://toc.library.ethz.ch/objects/pdf03/z01_978-1-4493-7332-0_01.pdf> — "Encoding and Evolution 111 Formats for Encoding Data 112"
[^ddia-toc-flow]: ETH Library, mục lục *Designing Data-Intensive Applications* (ISBN 978-1-4493-7332-0) — <https://toc.library.ethz.ch/objects/pdf03/z01_978-1-4493-7332-0_01.pdf> — "Dataflow Through Databases 129 Dataflow Through Services: REST and RPC 131 Message-Passing Dataflow 136"
[^cam-marshal]: Cambridge, Distributed Systems lecture notes (2021/22), §1.3 RPC — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "The process of encoding the function arguments is known as marshalling."
[^cam-rpc-fail]: Cambridge, Distributed Systems lecture notes (2021/22), §1.3 RPC — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "If the client sends an RPC request but receives no response, it doesn’t know whether or not the server received and processed the request."
[^cam-idl]: Cambridge, Distributed Systems lecture notes (2021/22), §1.3 RPC — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "software developers can then automatically generate marshalling/unmarshalling code and RPC stubs for the respective programming languages of each service and its clients."
[^cam-json-http]: Cambridge, Distributed Systems lecture notes (2021/22), §1.3 RPC — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "Today, the most common form of RPC is implemented using JSON data sent over HTTP."
[^cam-rolling]: Cambridge, Distributed Systems lecture notes (2021/22), §2.4 Fault tolerance and high availability — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "if a service can tolerate one out of three nodes being unavailable, then a software upgrade can be rolled out by installing it and restarting one node at a time, while the remaining two nodes continue running the service."
[^rfc8259-obj]: RFC 8259 (JSON) — <https://www.rfc-editor.org/rfc/rfc8259.html> — "An object is an unordered collection of zero or more name/value pairs, where a name is a string and a value is a string, number, boolean, null, object, or array."
[^rfc8259-num]: RFC 8259 (JSON) — <https://www.rfc-editor.org/rfc/rfc8259.html> — "numbers that are integers and are in the range [-(2**53)+1, (2**53)-1] are interoperable in the sense that implementations will agree exactly on their numeric values."
[^rfc8259-dup]: RFC 8259 (JSON) — <https://www.rfc-editor.org/rfc/rfc8259.html> — "When the names within an object are not unique, the behavior of software that receives such an object is unpredictable."
[^rfc8259-nan]: RFC 8259 (JSON) — <https://www.rfc-editor.org/rfc/rfc8259.html> — "Numeric values that cannot be represented in the grammar below (such as Infinity and NaN) are not permitted."
[^rfc8259-utf8]: RFC 8259 (JSON) — <https://www.rfc-editor.org/rfc/rfc8259.html> — "JSON text exchanged between systems that are not part of a closed ecosystem MUST be encoded using UTF-8"
[^rfc4180-nospec]: RFC 4180 (CSV) — <https://www.rfc-editor.org/rfc/rfc4180.html> — "there is no formal specification in existence, which allows for a wide variety of interpretations of CSV files."
[^rfc4180-header]: RFC 4180 (CSV) — <https://www.rfc-editor.org/rfc/rfc4180.html> — "There maybe an optional header line appearing as the first line of the file with the same format as normal record lines."
[^rfc4648-b64]: RFC 4648, mục 4 — <https://www.rfc-editor.org/rfc/rfc4648.html> — "The encoding process represents 24-bit groups of input bits as output strings of 4 encoded characters."
[^pbjson-bytes]: Protobuf, ProtoJSON Format — <https://protobuf.dev/programming-guides/json/> — "JSON value will be the data encoded as a string using standard base64 encoding with paddings."
[^pbjson-lossy]: Protobuf, ProtoJSON Format — <https://protobuf.dev/programming-guides/json/> — "Many implementations follow the original JS behavior that JSON was derived from and interpret all numbers as binary64 (double precision) and are silently lossy if a number is an integer larger than 2**53."
[^pbjson-int64]: Protobuf, ProtoJSON Format — <https://protobuf.dev/programming-guides/json/> — "To avoid these problems, ProtoJSON serializers emit int64s as strings to ensure no precision loss will occur on large int64s by any implementation."
[^pbjson-evol]: Protobuf, ProtoJSON Format — <https://protobuf.dev/programming-guides/json/> — "ProtoJSON format does not support unknown fields, and it puts field and enum value names into encoded messages which makes it much harder to change those names later. Removing fields is a breaking change that will trigger a parsing error."
[^pbjson-unknown]: Protobuf, ProtoJSON Format — <https://protobuf.dev/programming-guides/json/> — "Unlike the binary wire format, ProtoJSON implementations generally do not propagate unknown fields. This means that adding to schemas is generally compatible but will result in parse failures if a client using the old schema observes the new content."
[^pg-json]: PostgreSQL 16, JSON Types — <https://www.postgresql.org/docs/16/datatype-json.html> — "By contrast, jsonb does not preserve white space, does not preserve the order of object keys, and does not keep duplicate object keys. If duplicate keys are specified in the input, only the last value is kept."
[^cbor-schemaless]: RFC 8949 (CBOR), mục 1.1 và 5 — <https://www.rfc-editor.org/rfc/rfc8949.html> — "A specific design goal of CBOR is to not need any included or assumed schema: a decoder can take a CBOR item and decode it with no other knowledge."
[^serde-selfdesc]: docs.rs serde 1.0.228, trait Deserializer — <https://docs.rs/serde/1.0.228/serde/de/trait.Deserializer.html> — "Self-describing data formats like JSON are able to look at the serialized data and tell what it represents."
[^serde-nonself]: docs.rs serde 1.0.228, trait Deserializer — <https://docs.rs/serde/1.0.228/serde/de/trait.Deserializer.html> — "Non-self-describing formats like Postcard need to be told what is in the input in order to deserialize it."
[^pb-enc-kv]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "The binary version of a message just uses the field’s number as the key – the name and declared type for each field can only be determined on the decoding end by referencing the message type’s definition"
[^pb-enc-simple]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "If you were able to examine the encoded message, you’d see three bytes: 08 96 01"
[^pb-enc-varint]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "Variable-width integers, or varints, are at the core of the wire format. They allow encoding unsigned 64-bit integers using anywhere between one and ten bytes, with small values using fewer bytes."
[^pb-enc-varint2]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "Each byte in the varint has a continuation bit that indicates if the byte that follows it is part of the varint."
[^pb-enc-150]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "First you drop the MSB from each byte, as this is just there to tell us whether we’ve reached the end of the number"
[^pb-enc-tag]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "The “tag” of a record is encoded as a varint formed from the field number and the wire type via the formula (field_number << 3) | wire_type."
[^pb-enc-wt]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "There are six wire types: VARINT, I64, LEN, SGROUP, EGROUP, and I32"
[^pb-enc-wtmap]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "VARINT int32, int64, uint32, uint64, sint32, sint64, bool, enum 1 I64 fixed64, sfixed64, double 2 LEN string, bytes, embedded messages, packed repeated fields 3 SGROUP group start (deprecated) 4 EGROUP group end (deprecated) 5 I32 fixed32, sfixed32, float"
[^pb-enc-lendef]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "The LEN wire type has a dynamic length, specified by a varint immediately after the tag, which is followed by the payload as usual."
[^pb-enc-len]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "we see that the tag, 12, is 00010 010, or 2:LEN. The byte that follows is the int32 varint 7, and the next seven bytes are the UTF-8 encoding of "testing"."
[^pb-enc-bytes]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "bytes fields are encoded in the same way."
[^pb-enc-sub]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "Submessage fields also use the LEN wire type."
[^pb-enc-skip]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "The wire type tells the parser how big the payload after it is. This allows old parsers to skip over new fields they don’t understand."
[^pb-enc-missing]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "Missing fields are easy to encode: we just leave out the record if it’s not present."
[^pb-enc-neg]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "The intN types encode negative numbers as two’s complement, which means that, as unsigned, 64-bit integers, they have their highest bit set. As a result, this means that all ten bytes must be used."
[^pb-enc-zigzag]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "Positive integers p are encoded as 2 * p (the even numbers), while negative integers n are encoded as 2 * |n| - 1 (the odd numbers)."
[^pb-enc-order]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "Field numbers may be declared in any order in a .proto file. The order chosen has no effect on how the messages are serialized."
[^pb-enc-noorder]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "Therefore, Protobuf parsers must be able to parse fields in any order."
[^pb-enc-last]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "For numeric types and strings, if the same field appears multiple times, the parser accepts the last value it sees."
[^pb-enc-merge]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "The effect of these rules is that parsing the concatenation of two encoded messages produces exactly the same result as if you had parsed the two messages separately and merged the resulting objects."
[^pb-enc-stable]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "Do not assume the byte output of a serialized message is stable."
[^pb-enc-packed0]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "Starting in Edition 2023, repeated fields of a primitive type (any scalar type that is not string or bytes) are “packed” by default."
[^pb-enc-packed]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "Protobuf parsers must be able to parse repeated fields that were compiled as packed as if they were not packed, and vice versa. This permits adding [packed=true] to existing fields in a forward- and backward-compatible way."
[^pb-enc-map]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "Thus, maps are encoded almost exactly like a repeated message field: as a sequence of LEN-typed records, with two fields each. The exception is that order is not guaranteed to be preserved with maps during serialization."
[^pb-enc-limit]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "Protos must be smaller than 2 GiB when serialized."
[^pb3-num]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "You must give each field in your message definition a number between 1 and 536,870,911 with the following restrictions:"
[^pb3-19000]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Field numbers 19,000 to 19,999 are reserved for the Protobuf implementation."
[^pb3-29bits]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "The field number is limited to 29 bits rather than 32 bits because three bits are used to specify the field’s wire format."
[^pb3-1to15]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "For example, field numbers in the range 1 through 15 take one byte to encode. Field numbers in the range 16 through 2047 take two bytes."
[^pb3-immut]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "This number cannot be changed once your message type is in use because it identifies the field in the message wire format."
[^pb3-change]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "“Changing” a field number is equivalent to deleting that field and creating a new field with the same type but a new number."
[^pb3-lean]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "The Protobuf wire format is lean and doesn’t provide a way to detect fields encoded using one definition and decoded using another."
[^pb3-reuse]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Encoding a field using one definition and then decoding that same field with a different definition can lead to: Developer time lost to debugging A parse/merge error (best case scenario) Leaked PII/SPII Data corruption"
[^pb3-reuse-causes]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Renumbering effectively deletes and re-adds all the fields involved in the renumbering, resulting in incompatible wire-format changes."
[^pb3-reserve]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "you may delete the field definition from the message. However, you must reserve the deleted field number."
[^pb3-reserve-name]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Reusing an old field name later is generally safe, except when using TextProto or JSON encodings where the field name is serialized."
[^pb3-reserved-syntax]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Reserved field number ranges are inclusive (9 to 11 is the same as 9, 10, 11)."
[^pb3-safe-add]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "If you add new fields, any messages serialized by code using your “old” message format can still be parsed by your new generated code."
[^pb3-safe-add2]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Similarly, messages created by your new code can be parsed by your old code: old binaries simply ignore the new field when parsing."
[^pb3-safe-remove]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Removing fields is safe. The same field number must not used again in your updated message type."
[^pb3-safe-enum]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Adding additional values to an enum is safe."
[^pb3-safe-app]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Note that any wire-safe changes may be a breaking change to application code in a given language."
[^pb3-unsafe-num]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Changing field numbers for any existing field is not safe."
[^pb3-unsafe-oneof]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Moving fields into an existing oneof is not safe."
[^pb3-compat-int]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "changing an int32 to an int64 is a compatible change, but if a value larger than INT32_MAX is written, a client that reads it as an int32 will discard the high order bits of the number."
[^pb3-compat-sint]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "sint32 and sint64 are compatible with each other but are not compatible with the other integer types."
[^pb3-compat-rollout]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "You can make compatible changes to your schema only if you manage the roll out to your system carefully. For example, you may change an int32 to an int64 but ensure you continue to only write legal int32 values until the new schema is deployed to all endpoints"
[^pb3-compat-pub]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "If your schema is published outside of your organization, you should generally not make wire-compatible changes, as you cannot manage the deployment of the new schema to know when the different range of values may be safe to use."
[^pb3-implicit]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Note that for implicit-presence scalar fields, once a message is parsed there’s no way of telling whether that field was explicitly set to the default value (for example whether a boolean was set to false) or just not set at all"
[^pb3-enum-open]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "During deserialization, unrecognized enum values will be preserved in the message, though how this is represented when the message is deserialized is language-dependent."
[^pb3-unknown]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Unknown fields are well-formed Protobuf serialized data representing fields that the parser does not recognize. For example, when an old binary parses data sent by a new binary with new fields, those new fields become unknown fields in the old binary."
[^pb3-unknown-keep]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Proto3 messages preserve unknown fields and include them during parsing and in the serialized output, which matches proto2 behavior."
[^pb3-unknown-lose]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Some actions can cause unknown fields to be lost. For example, if you do one of the following, unknown fields are lost: Serialize a proto to JSON. Iterate over all of the fields in a message to populate a new message."
[^pb3-unknown-copy]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Use message-oriented APIs, such as CopyFrom() and MergeFrom(), to copy data rather than copying field-by-field"
[^pb3-any]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "The Any message type lets you use messages as embedded types without having their .proto definition."
[^pbpres-implicit-ser]: Protobuf, Field Presence — <https://protobuf.dev/programming-guides/field_presence/> — "When serializing, fields with implicit presence are not serialized if they contain their default value."
[^pbdos-skew]: Protobuf, Proto Best Practices — <https://protobuf.dev/best-practices/dos-donts/> — "Clients and servers are never updated at exactly the same time - even when you try to update them at the same time. One or the other may get rolled back."
[^pbdos-reuse]: Protobuf, Proto Best Practices — <https://protobuf.dev/best-practices/dos-donts/> — "If the change was live ever, there could be serialized versions of your proto in a log somewhere. Or there could be old code in another server that will break."
[^pbdos-type]: Protobuf, Proto Best Practices — <https://protobuf.dev/best-practices/dos-donts/> — "However, changing a field’s type can be difficult to roll out safely even when the new schema can successfully parse old data."
[^pbdos-default]: Protobuf, Proto Best Practices — <https://protobuf.dev/best-practices/dos-donts/> — "Almost never change the default value of a proto field. This causes version skew between clients and servers."
[^pbdos-enum-unspec]: Protobuf, Proto Best Practices — <https://protobuf.dev/best-practices/dos-donts/> — "the first declared enum value should be a default FOO_UNSPECIFIED value and should use tag 0."
[^pbdos-enum-nomean]: Protobuf, Proto Best Practices — <https://protobuf.dev/best-practices/dos-donts/> — "It may be tempting to declare this default as a semantically meaningful value but as a general rule, do not, to aid in the evolution of your protocol as new enum values are added over time."
[^pbdos-required]: Protobuf, Proto Best Practices — <https://protobuf.dev/best-practices/dos-donts/> — "You never know how long a message type is going to last and whether someone will be forced to fill in your required field with an empty string or zero in four years when it’s no longer logically required but the proto still says it is."
[^pbdos-required2]: Protobuf, Proto Best Practices — <https://protobuf.dev/best-practices/dos-donts/> — "Required fields are considered harmful by so many they were removed from proto3 completely."
[^pbdos-text]: Protobuf, Proto Best Practices — <https://protobuf.dev/best-practices/dos-donts/> — "Text-based serialization formats like text format and JSON represent fields and enum values as strings. As a result, deserialization of Protobuf messages in these formats using old code will fail when a field or enum value is renamed, or when a new field or enum value or extension is added."
[^pbdos-stable]: Protobuf, Proto Best Practices — <https://protobuf.dev/best-practices/dos-donts/> — "The stability of proto serialization is not guaranteed across binaries or across builds of the same binary."
[^pbdos-rpc-storage]: Protobuf, Proto Best Practices — <https://protobuf.dev/best-practices/dos-donts/> — "However, the needs of long-term storage and live RPC services tend to later diverge."
[^pbdos-any]: Protobuf, Proto Best Practices — <https://protobuf.dev/best-practices/dos-donts/> — "The main use case that Any fulfills is the uncommon scenario in which horizontal infrastructure needs to propagate completely arbitrary messages"
[^avro-bytes]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "bytes: sequence of 8-bit unsigned bytes"
[^avro-noname]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "Binary encoded Avro data does not include type information or field names. The benefit is that the serialized data is small, but as a result a schema must always be used in order to read Avro data correctly."
[^avro-store]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "Therefore, files or systems that store Avro data should always include the writer’s schema for that data. Avro-based remote procedure call (RPC) systems must also guarantee that remote recipients of data have a copy of the schema used to write that data."
[^avro-record]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "A record is encoded by encoding the values of its fields in the order that they are declared."
[^avro-example]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "An instance of this record whose a field has value 27 (encoded as hex 36) and whose b field has value “foo” (encoded as hex bytes 06 66 6f 6f), would be encoded simply as the concatenation of these, namely the hex byte sequence:"
[^avro-zigzag]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "int and long values are written using variable-length zig-zag coding."
[^avro-zigzag-tbl]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "-64 7f 64 80 01"
[^avro-string]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "a string is encoded as a long followed by that many bytes of UTF-8 encoded character data."
[^avro-reliant]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "Therefore readers are wholly reliant on the schema used when the data was encoded."
[^avro-container]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "A file has a schema, and all objects stored in the file must be written according to that schema, using binary encoding."
[^avro-sync]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "Syncronization markers are used between blocks to permit efficient splitting of files for MapReduce processing."
[^avro-handshake]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "The purpose of the handshake is to ensure that the client and the server have each other’s protocol definition, so that the client can correctly deserialize responses, and the server can correctly deserialize requests."
[^avro-single]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "A two-byte marker, C3 01, to show that the message is Avro and uses this single-record format (version 1). The 8-byte little-endian CRC-64-AVRO fingerprint of the object’s schema. The Avro object encoded using Avro’s binary encoding."
[^avro-single-kafka]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "One very common example is storing Avro records for several weeks in an Apache Kafka topic."
[^avro-single-why]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "So the need arises to know which schema was used to write a record to support schema evolution correctly. In most cases the schema itself is too large to include in the message, so this binary wrapper format supports the use case more effectively."
[^avro-resolve-defs]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "We refer to the schema used to write the data as the writer’s schema, and the schema that the application expects the reader’s schema."
[^avro-resolve-names]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "the ordering of fields may be different: fields are matched by name."
[^avro-resolve-ignore]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "if the writer’s record contains a field with a name not present in the reader’s record, the writer’s value for that field is ignored."
[^avro-resolve-default]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "if the reader’s record schema has a field that contains a default value, and writer’s schema does not have a field with the same name, then the reader should use the default value from its field."
[^avro-resolve-err]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "if the reader’s record schema has a field with no default value, and writer’s schema does not have a field with the same name, an error is signalled."
[^avro-promote]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "int is promotable to long, float, or double"
[^avro-enum-default]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "if the writer’s symbol is not present in the reader’s enum and the reader has a default value, then that value is used, otherwise an error is signalled."
[^avro-default-def]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "default: A default value for this field, only used when reading instances that lack the field for schema evolution purposes. The presence of a default value does not make the field optional at encoding time."
[^avro-union-default]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "Default values for union fields correspond to the first schema that matches in the union."
[^avro-alias]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "Named types and fields may have aliases. An implementation may optionally use aliases to map a writer’s schema to the reader’s. This facilitates both schema evolution as well as processing disparate datasets."
[^avro-blocks]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "If a block’s count is negative, its absolute value is used, and the count is followed immediately by a long block size indicating the number of bytes in the block. This block size permits fast skipping through data, e.g., when projecting a record to a subset of its fields."
[^avro-union]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "For example, the union schema ["null","string"] would encode: null as zero (the index of “null” in the union): 00 the string “a” as one (the index of “string” in the union, 1, encoded as hex 02), followed by the serialized string: 02 02 61"
[^avro-canon]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "Parsing Canonical Form is a transformation of a writer’s schema that let’s us define what it means for two schemas to be “the same” for the purpose of reading data written against the schema."
[^avro-canon-strip]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "Keep only attributes that are relevant to parsing data, which are: type, name, fields, symbols, items, values, size. Strip all others (e.g., doc and aliases)."
[^avro-json-union]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "otherwise it is encoded as a JSON object with one name/value pair whose name is the type’s name and whose value is the recursively encoded value."
[^avro-json]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "Note that the original schema is still required to correctly process JSON-encoded data. For example, the JSON encoding does not distinguish between int and long, float and double, records and maps, enums and strings, etc."
[^conf-backward]: Confluent Platform 7.9, Schema Evolution and Compatibility — <https://docs.confluent.io/platform/7.9/schema-registry/fundamentals/schema-evolution.html> — "BACKWARD compatibility means that consumers using the new schema can read data produced with the last schema."
[^conf-forward]: Confluent Platform 7.9, Schema Evolution and Compatibility — <https://docs.confluent.io/platform/7.9/schema-registry/fundamentals/schema-evolution.html> — "FORWARD compatibility means that data produced with a new schema can be read by consumers using the last schema, even though they may not be able to use the full capabilities of the new schema."
[^conf-full]: Confluent Platform 7.9, Schema Evolution and Compatibility — <https://docs.confluent.io/platform/7.9/schema-registry/fundamentals/schema-evolution.html> — "FULL compatibility means schemas are both backward and forward compatible."
[^conf-trans]: Confluent Platform 7.9, Schema Evolution and Compatibility — <https://docs.confluent.io/platform/7.9/schema-registry/fundamentals/schema-evolution.html> — "Transitive - compatibility checked against all previous versions, not just the last one"
[^conf-rewind]: Confluent Platform 7.9, Schema Evolution and Compatibility — <https://docs.confluent.io/platform/7.9/schema-registry/fundamentals/schema-evolution.html> — "BACKWARD compatibility mode is the default, and preferred for Kafka, is so that you can rewind consumers to the beginning of the topic."
[^conf-order]: Confluent Platform 7.9, Schema Evolution and Compatibility — <https://docs.confluent.io/platform/7.9/schema-registry/fundamentals/schema-evolution.html> — "BACKWARD or BACKWARD_TRANSITIVE: there is no assurance that consumers using older schemas can read data produced using the new schema. Therefore, upgrade all consumers before you start producing new events."
[^conf-order-fwd]: Confluent Platform 7.9, Schema Evolution and Compatibility — <https://docs.confluent.io/platform/7.9/schema-registry/fundamentals/schema-evolution.html> — "FORWARD or FORWARD_TRANSITIVE: there is no assurance that consumers using the new schema can read data produced using older schemas. Therefore, first upgrade all producers to using the new schema and make sure the data already produced using the older schemas are not available to consumers, then upgrade the consumers."
[^conf-order-full]: Confluent Platform 7.9, Schema Evolution and Compatibility — <https://docs.confluent.io/platform/7.9/schema-registry/fundamentals/schema-evolution.html> — "FULL or FULL_TRANSITIVE: there are assurances that consumers using older schemas can read data produced using the new schema and that consumers using the new schema can read data produced using older schemas. Therefore, you can upgrade the producers and consumers independently."
[^conf-newtopic]: Confluent Platform 7.9, Schema Evolution and Compatibility — <https://docs.confluent.io/platform/7.9/schema-registry/fundamentals/schema-evolution.html> — "you need to either upgrade all producers and consumers to the new schema version at the same time or, more likely, create a brand-new topic and start migrating applications to use the new topic and new schema"
[^conf-pb-trans]: Confluent Platform 7.9, Schema Evolution and Compatibility — <https://docs.confluent.io/platform/7.9/schema-registry/fundamentals/schema-evolution.html> — "Note that best practice for Protobuf is to use BACKWARD_TRANSITIVE, as adding new message types is not forward compatible."
[^conf-nuance]: Confluent Platform 7.9, Schema Evolution and Compatibility — <https://docs.confluent.io/platform/7.9/schema-registry/fundamentals/schema-evolution.html> — "The rules and grammar for JSON Schema and Protobuf can be more nuanced."
[^conf-check]: Confluent Platform 7.9, Schema Evolution and Compatibility — <https://docs.confluent.io/platform/7.9/schema-registry/fundamentals/schema-evolution.html> — "When a schema is updated, Schema Registry checks compatibility before accepting the new version."
[^conf-streams]: Confluent Platform 7.9, Schema Evolution and Compatibility — <https://docs.confluent.io/platform/7.9/schema-registry/fundamentals/schema-evolution.html> — "For Kafka Streams, only BACKWARD compatibility is supported."
[^conf-wire]: Confluent Platform 7.9, Wire format — <https://docs.confluent.io/platform/7.9/schema-registry/fundamentals/serdes-develop/index.html> — "Confluent serialization format version number; currently always 0. 1-4 Schema ID 4-byte schema ID as returned by Schema Registry. 5-… Data Serialized data for the specified schema format"
[^kafka-ser]: Kafka 4.0.2 API, Serializer — <https://kafka.apache.org/40/javadoc/org/apache/kafka/common/serialization/Serializer.html> — "An interface for converting objects to bytes."
[^kafka-retention]: Apache Kafka 4.0, Design — <https://kafka.apache.org/40/design/design/> — "old log data is discarded after a fixed period of time or when the log reaches some predetermined size."
[^kafka-compact]: Apache Kafka 4.0, Design — <https://kafka.apache.org/40/design/design/> — "The idea is to selectively remove records where we have a more recent update with the same primary key. This way the log is guaranteed to have at least the last state for each key."
[^grpc-idl]: gRPC, Core concepts — <https://grpc.io/docs/what-is-grpc/core-concepts/> — "By default, gRPC uses protocol buffers as the Interface Definition Language (IDL) for describing both the service interface and the structure of the payload messages. It is possible to use other alternatives if desired."
[^grpc-kinds]: gRPC, Core concepts — <https://grpc.io/docs/what-is-grpc/core-concepts/> — "gRPC lets you define four kinds of service method"
[^grpc-deadline]: gRPC, Core concepts — <https://grpc.io/docs/what-is-grpc/core-concepts/> — "gRPC allows clients to specify how long they are willing to wait for an RPC to complete before the RPC is terminated with a DEADLINE_EXCEEDED error."
[^grpc-indep]: gRPC, Core concepts — <https://grpc.io/docs/what-is-grpc/core-concepts/> — "In gRPC, both the client and server make independent and local determinations of the success of the call, and their conclusions may not match."
[^grpc-cancel]: gRPC, Core concepts — <https://grpc.io/docs/what-is-grpc/core-concepts/> — "Changes made before a cancellation are not rolled back."
[^k8s-rolling]: Kubernetes v1.33, Deployments — <https://v1-33.docs.kubernetes.io/docs/concepts/workloads/controllers/deployment/> — "The Deployment updates Pods in a rolling update fashion (gradually scale down the old ReplicaSets and scale up the new one) when .spec.strategy.type==RollingUpdate."
[^pg-addcol]: PostgreSQL 16, ALTER TABLE — <https://www.postgresql.org/docs/16/sql-altertable.html> — "When a column is added with ADD COLUMN and a non-volatile DEFAULT is specified, the default is evaluated at the time of the statement and the result stored in the table's metadata. That value will be used for the column for all existing rows. If no DEFAULT is specified, NULL is used. In neither case is a rewrite of the table required."
[^pg-rewrite]: PostgreSQL 16, ALTER TABLE — <https://www.postgresql.org/docs/16/sql-altertable.html> — "Adding a column with a volatile DEFAULT or changing the type of an existing column will require the entire table and its indexes to be rewritten."
[^pg-dropcol]: PostgreSQL 16, ALTER TABLE — <https://www.postgresql.org/docs/16/sql-altertable.html> — "The DROP COLUMN form does not physically remove the column, but simply makes it invisible to SQL operations."
[^pg-check]: PostgreSQL 16, ALTER TABLE — <https://www.postgresql.org/docs/16/sql-altertable.html> — "Adding a CHECK or NOT NULL constraint requires scanning the table to verify that existing rows meet the constraint, but does not require a table rewrite."
[^rust-layout]: The Rust Reference 1.95.0, Type layout — <https://doc.rust-lang.org/1.95.0/reference/type-layout.html> — "There are no other guarantees of data layout made by this representation."
[^serde-default]: serde.rs, Field attributes — <https://serde.rs/field-attrs.html> — "If the value is not present when deserializing, use the Default::default(). #[serde(default = "path")] If the value is not present when deserializing, call a function to get a default value. The given function must be callable as fn() -> T."
[^serde-missing]: serde v1.0.228, serde/src/private/de.rs — <https://raw.githubusercontent.com/serde-rs/serde/v1.0.228/serde/src/private/de.rs> — "If the missing field is of type `Option<T>` then treat is as `None`, otherwise it is an error."
[^serde-derive-missing]: serde_derive v1.0.228, src/de.rs — <https://raw.githubusercontent.com/serde-rs/serde/v1.0.228/serde_derive/src/de.rs> — "let func = quote_spanned!(span=> _serde::#private::de::missing_field);"
[^serde-deny]: serde.rs, Container attributes — <https://serde.rs/container-attrs.html> — "Always error during deserialization when encountering unknown fields. When this attribute is not present, by default unknown fields are ignored for self-describing formats like JSON."
[^serde-flatten]: serde.rs, Field attributes — <https://serde.rs/field-attrs.html> — "this attribute is not supported in combination with structs that use deny_unknown_fields."
[^serde-skipif]: serde.rs, Field attributes — <https://serde.rs/field-attrs.html> — "Call a function to determine whether to skip serializing this field. The given function must be callable as fn(&T) -> bool, although it may be generic over T. For example skip_serializing_if = "Option::is_none" would skip an Option that is None."
[^serde-alias]: serde.rs, Field attributes — <https://serde.rs/field-attrs.html> — "Deserialize this field from the given name or from its Rust name. May be repeated to specify multiple possible names for the same field."
[^serde-other]: serde.rs, Variant attributes — <https://serde.rs/variant-attrs.html> — "Deserialize this variant if the enum tag is anything other than the tag of one of the other variants in this enum. Only allowed on a unit variant inside of an internally tagged or adjacently tagged enum."
[^serde-with]: serde.rs, Field attributes — <https://serde.rs/field-attrs.html> — "Serialize this field using a function that is different from its implementation of Serialize."
[^sjson-num]: docs.rs serde_json 1.0.145, struct Number — <https://docs.rs/serde_json/1.0.145/serde_json/struct.Number.html> — "Numbers smaller than i64::MIN or larger than u64::MAX can only be represented in Number if serde_json’s “arbitrary_precision” feature is enabled."
[^pc-struct]: Postcard wire format specification — <https://postcard.jamesmunns.com/wire-format> — "A struct is encoded as the elements that comprise it, in their order of definition (top to bottom). As structs have a known number of elements with known names, their length and field names are not encoded on the wire."
[^pc-stable]: docs.rs postcard 1.1.3 — <https://docs.rs/postcard/1.1.3/postcard/> — "As of v1.0.0, postcard has a documented and stable wire format."
[^prost-gen]: docs.rs prost 0.14.1 — <https://docs.rs/prost/0.14.1/prost/> — "#[derive(Clone, PartialEq, ::prost::Message)] pub struct Person { #[prost(string, tag="1")] pub name: ::prost::alloc::string::String,"
[^prost-i32]: docs.rs prost 0.14.1 — <https://docs.rs/prost/0.14.1/prost/> — "All .proto enumeration types convert to the Rust i32 type."
[^prost-enum]: docs.rs prost 0.14.1 — <https://docs.rs/prost/0.14.1/prost/> — "The enum type isn’t used directly as a field, because the Protobuf spec mandates that enumerations values are ‘open’, and decoding unrecognized enumeration values must be possible."
[^prost-getter]: docs.rs prost 0.14.1 — <https://docs.rs/prost/0.14.1/prost/> — "Note that the getter methods will return the Rust enum’s default value if the field has an invalid i32 value."
[^prost-tryfrom]: docs.rs prost 0.14.1 — <https://docs.rs/prost/0.14.1/prost/> — "It also adds an impl TryFrom\<i32> for PhoneType, so you can convert an i32 to its corresponding PhoneType value"
[^prost-opt]: docs.rs prost 0.14.1 — <https://docs.rs/prost/0.14.1/prost/> — "Note that in proto3 the default representation for all user-defined message types is Option\<T>, and for scalar types just T (during decoding, a missing value is populated by T::default()). If you need a witness of the presence of a scalar type T, use the optional modifier to enforce an Option\<T> representation in the generated Rust struct."
[^prost-skip-derive]: prost-derive v0.14.1, src/lib.rs — <https://raw.githubusercontent.com/tokio-rs/prost/v0.14.1/prost-derive/src/lib.rs> — "_ => ::prost::encoding::skip_field(wire_type, tag, buf, ctx),"
[^prost-skip-fn]: prost v0.14.1, src/encoding.rs — <https://raw.githubusercontent.com/tokio-rs/prost/v0.14.1/prost/src/encoding.rs> — "if len > buf.remaining() as u64 { return Err(DecodeError::new("buffer underflow")); } buf.advance(len as usize); Ok(())"
[^prost-noreflect]: docs.rs prost 0.14.1 — <https://docs.rs/prost/0.14.1/prost/> — "Does not include support for runtime reflection or message descriptors."
[^tonic-intro]: docs.rs tonic 0.14.2 — <https://docs.rs/tonic/0.14.2/tonic/> — "tonic is a gRPC over HTTP/2 implementation focused on high performance, interoperability, and flexibility."
[^tonic-build]: docs.rs tonic 0.14.2 — <https://docs.rs/tonic/0.14.2/tonic/> — "Getting Started Follow the instructions in the tonic-build crate documentation."
[^tonic-include]: docs.rs tonic 0.14.2 — <https://docs.rs/tonic/0.14.2/tonic/> — "include_proto Include generated proto server and client items."
[^tonic-limit]: docs.rs tonic 0.14.2 — <https://docs.rs/tonic/0.14.2/tonic/> — "By default, the decoding message limit is 4MB and the encoding limit is usize::MAX."
[^avro-rs-reader]: docs.rs apache-avro 0.21.0 — <https://docs.rs/apache-avro/0.21.0/apache_avro/> — "As far as reading Avro encoded data goes, we can just use the schema encoded with the data to read them."
[^avro-rs-resolve]: docs.rs apache-avro 0.21.0 — <https://docs.rs/apache-avro/0.21.0/apache_avro/> — "The library will also automatically perform schema resolution while reading the data."
[^avro-rs-compat]: docs.rs apache-avro 0.21.0 — <https://docs.rs/apache-avro/0.21.0/apache_avro/> — "assert!(SchemaCompatibility::can_read(&writers_schema, &readers_schema).is_ok());"
[^avro-rs-beta]: docs.rs apache-avro 0.21.0 — <https://docs.rs/apache-avro/0.21.0/apache_avro/> — "The library is still in beta, so there might be backward-incompatible changes between minor versions."
[^pg-jsonb-num]: PostgreSQL 16, JSON Types — <https://www.postgresql.org/docs/16/datatype-json.html> — "Notably, jsonb will reject numbers that are outside the range of the PostgreSQL numeric data type, while json will not."
[^pg-json-exact]: PostgreSQL 16, JSON Types — <https://www.postgresql.org/docs/16/datatype-json.html> — "Because the json type stores an exact copy of the input text, it will preserve semantically-insignificant white space between tokens, as well as the order of keys within JSON objects. Also, if a JSON object within the value contains the same key more than once, all the key/value pairs are kept."
[^pg-rewrite-exc]: PostgreSQL 16, ALTER TABLE — <https://www.postgresql.org/docs/16/sql-altertable.html> — "As an exception, when changing the type of an existing column, if the USING clause does not change the column contents and the old type is either binary coercible to the new type or an unconstrained domain over the new type, a table rewrite is not needed."
[^pg-dropcol-fast]: PostgreSQL 16, ALTER TABLE — <https://www.postgresql.org/docs/16/sql-altertable.html> — "Thus, dropping a column is quick but it will not immediately reduce the on-disk size of your table"
[^rfc8259-limits]: RFC 8259 (JSON) — <https://www.rfc-editor.org/rfc/rfc8259.html> — "This specification allows implementations to set limits on the range and precision of numbers accepted."
[^prost-encode-derive]: prost-derive v0.14.1, src/lib.rs — <https://raw.githubusercontent.com/tokio-rs/prost/v0.14.1/prost-derive/src/lib.rs> — "fn encode_raw(&self, buf: &mut impl ::prost::bytes::BufMut) { #(#encode)* }"
[^prost-readme]: prost v0.14.1, README — <https://raw.githubusercontent.com/tokio-rs/prost/v0.14.1/README.md> — "Preserves unknown enum values during deserialization. Does not include support for runtime reflection or message descriptors."
[^pbjson-ignore]: Protobuf, ProtoJSON Format — <https://protobuf.dev/programming-guides/json/> — "or that the relevant clients set an Ignore Unknown Fields flag, discussed below"
[^pbjson-ignore-default]: Protobuf, ProtoJSON Format — <https://protobuf.dev/programming-guides/json/> — "The Protobuf JSON parser should reject unknown fields by default but may provide an option to ignore unknown fields in parsing."
[^conf-streams2]: Confluent Platform 7.9, Schema Evolution and Compatibility — <https://docs.confluent.io/platform/7.9/schema-registry/fundamentals/schema-evolution.html> — "FULL, FULL_TRANSITIVE, and BACKWARD_TRANSITIVE compatibilities are always supported for Kafka Streams, as they include backward compatibility"
[^conf-wire-pb]: Confluent Platform 7.9, Wire format — <https://docs.confluent.io/platform/7.9/schema-registry/fundamentals/serdes-develop/index.html> — "The Protobuf serialization format appends a list of message indexes after the magic-byte and schema-id."
[^k8s-multi]: Kubernetes v1.33, Deployments — <https://v1-33.docs.kubernetes.io/docs/concepts/workloads/controllers/deployment/> — "RollingUpdate Deployments support running multiple versions of an application at the same time."
[^pb3-packed]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "In proto3, repeated fields of scalar numeric types use packed encoding by default."
[^cbor-shared]: RFC 8949 (CBOR), mục 5 — <https://www.rfc-editor.org/rfc/rfc8949.html> — "in real-world implementations, the encoder and the decoder will have a shared view of what should be in a CBOR data item."
[^pbdos-default-proto3]: Protobuf, Proto Best Practices — <https://protobuf.dev/best-practices/dos-donts/> — "Proto3 removed the ability to set default values."
[^pbdos-required3]: Protobuf, Proto Best Practices — <https://protobuf.dev/best-practices/dos-donts/> — "For proto3 there are no required fields, so this advice does not apply."
[^pb3-reserve-compiler]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Reserved names affect only the protoc compiler behavior and not runtime behavior"
[^tonic-config]: docs.rs tonic 0.14.2 — <https://docs.rs/tonic/0.14.2/tonic/> — "both servers and clients can be configured to set the max message encoding and decoding size"
