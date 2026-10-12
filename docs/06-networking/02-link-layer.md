# P6 · Chương 02 — Link layer

## Bạn sẽ hiểu được gì

1. **Q1.** Vì sao link layer phải "đóng khung" (framing) bit thành frame, và có những cách đóng khung nào? → mục 1
2. **Q2.** CRC làm gì ở cuối frame Ethernet, và nó khác checksum của Internet ở điểm nào (mức khái niệm)? → mục 2
3. **Q3.** Một frame Ethernet gồm những trường nào, địa chỉ MAC thuộc về ai, broadcast hoạt động ra sao? → mục 3
4. **Q4.** Switch (learning bridge) học "MAC nào nằm sau cổng nào" bằng cách nào, và chuyện gì xảy ra khi chưa học được hoặc khi mạng có vòng lặp? VLAN giải quyết vấn đề gì? → mục 4
5. **Q5.** ARP biến địa chỉ IPv4 thành MAC bằng request/reply nào; ARP cache sống và chết ra sao; vì sao `ip neigh` đôi khi hiện `FAILED`? → mục 5
6. **Q6.** IPv6 thay ARP bằng gì? → mục 6
7. **Q7.** MTU 1500 và jumbo frame liên quan gì tới nhau? → mục 7
8. **Q8.** Đọc `ip link`, `ip neigh`, `tcpdump -e` trong một container Linux thế nào; loopback, veth, bridge là gì? → mục 9

(Mục phụ trợ: giới thiệu Wi-Fi → mục 8.)

## Kiến thức cần trước

- [Chương 01 — Layering](01-layering.md): encapsulation, "demux key", mô hình TCP/IP.

Bạn đã viết service gọi qua mạng (`TcpStream::connect`, một HTTP client) nhưng chưa cần biết gói tin đi qua dây ra sao; chương này lấp phần dưới cùng của đường đi đó. Ta bám theo một ví dụ xuyên chương: `orders`, một service Rust chạy trong container Docker, mỗi request phải gọi sang `payments`, một service khác trong cùng mạng container (ví dụ giả định của người soạn, không phải code trong repo). Mục nào cũng mở bằng một câu hỏi: tầng này làm gì cho cú gọi từ `orders` sang `payments`?

**Môi trường các quan sát** (mọi chỗ ghi "quan sát"): container Debian 12 (bookworm) từ `./labs/linux.sh` trên Docker của macOS aarch64, kernel VM `6.8.0-117-generic`, `iproute2-6.1.0`, capability `NET_ADMIN`/`NET_RAW`; `ip netns add` bị từ chối (`mount --make-shared /run/netns failed: Permission denied`). Mỗi lần chạy là một container mới nên IP và MAC khác nhau giữa các khối lệnh (`172.17.0.2` ở chỗ này, `172.17.0.3` ở chỗ kia); trong từng khối thì nhất quán.

## 1. Link layer và framing: dòng bit thành frame (Q1)

> **Trả lời ngắn.** Link layer chuyển datagram của IP tới hop kế tiếp trên cùng một mạng nối trực tiếp (nhận xét của người soạn). Để làm việc đó nó gom dòng bit thành frame[^sa-osi-frame]. Phần cứng chỉ đẩy bit đi, nên bên nhận phải tự tìm ra frame bắt đầu và kết thúc ở đâu[^sa-framing-challenge]. Có ba hướng giải quyết: theo byte, theo bit và theo đồng hồ (như SONET)[^sa-framing-three].

**Giải thích.**

*Với service `orders`.* Khi `orders` gọi `payments`, kernel đã bọc request thành TCP segment rồi thành IP datagram (chương [01](01-layering.md)). Từ đây IP chỉ cần tầng dưới làm đúng một việc: đưa datagram tới **hop kế tiếp**, tức máy nằm trên cùng mạng nối trực tiếp với `orders`. Tầng làm việc đó là link layer (nhận xét của người soạn). Nhưng dây chỉ chở bit, không cho biết datagram của `orders` bắt đầu và kết thúc ở bit nào. Mục này nói link layer giải quyết chuyện đó ra sao, và vì sao đơn vị nó dùng là frame.

### 1.1 Bên nhận phải tự tìm ranh giới frame

Adaptor (card mạng) bên nhận nhận về một dãy bit. Thách thức trung tâm của nó là xác định chính xác tập bit nào làm thành một frame, tức frame bắt đầu và kết thúc ở đâu[^sa-framing-challenge]. Bài toán này nền tảng đến mức mạng đa truy cập như Ethernet và Wi-Fi cũng phải giải[^sa-framing-general].

### 1.2 Ba cách đánh dấu ranh giới

Sách minh họa không gian thiết kế bằng ba giao thức[^sa-framing-three]. Hai cách đầu thuộc hướng byte (byte-oriented), ví dụ PPP:

- **Sentinel**: dùng ký tự đặc biệt đánh dấu đầu và cuối frame[^sa-sentinel]. Nếu chính ký tự đó xuất hiện trong dữ liệu thì phải "thoát" (escape) nó. Kỹ thuật này gọi là character stuffing vì chèn thêm ký tự vào phần dữ liệu[^sa-stuff].
- **Byte count**: ghi số byte của frame vào header[^sa-bytecount]. Rủi ro là lỗi truyền làm hỏng đúng trường đếm, và khi đó bên nhận không xác định được điểm kết thúc frame[^sa-bytecount-risk].

Cách thứ ba thuộc hướng bit, tiêu biểu là HDLC: nó đánh dấu đầu và cuối frame bằng chuỗi bit `01111110`[^sa-hdlc-flag]. Chuỗi này cũng có thể xuất hiện trong thân frame, nên bên gửi chèn một bit 0 sau mỗi năm bit 1 liên tiếp của thân (bit stuffing)[^sa-bitstuff]. Còn cách dựa trên đồng hồ như SONET thì sách gọi là clock-based framing[^sa-clockbased]; chi tiết SONET ngoài phạm vi chương.

<details><summary>Đi sâu: Ethernet đóng khung theo hướng bit</summary>

Ethernet đóng khung theo hướng bit (bit-oriented)[^sa-eth-bitoriented]; chi tiết tầng vật lý (preamble, tín hiệu) ngoài phạm vi chương. Khi frame lên tới kernel, nó đã là một mảng byte có độ dài xác định với header 14 byte như ở mục 3 (suy luận của người soạn từ góc nhìn host của sách).

</details>

**Quay lại Q1:** framing cho bên nhận biết ranh giới frame; có ba hướng là byte (sentinel hoặc byte count), bit (bit stuffing) và đồng hồ. Với `orders`, mọi việc này diễn ra ở adaptor, dưới tầm nhìn của code Rust (suy luận của người soạn).

## 2. Phát hiện lỗi: CRC ở mức khái niệm (Q2)

> **Trả lời ngắn.** CRC là mã phát hiện lỗi: bên gửi thêm các bit kiểm tra sao cho cả message chia hết cho một đa thức đã thỏa thuận, bên nhận chia lại và dư khác 0 nghĩa là có lỗi[^sa-crc-remainder]. Frame Ethernet mang một CRC 32 bit ở cuối[^sa-eth-crc]. CRC chỉ phát hiện lỗi; xử lý lỗi đã phát hiện (truyền lại hoặc mã sửa lỗi) là phần khác của bài toán[^sa-err-two]. Checksum của Internet là phép cộng 16 bit, yếu hơn CRC[^sa-checksum-weak].

**Giải thích.**

*Với service `orders`.* Request của `orders` đi qua ít nhất một đoạn dây hoặc cáp, và trên đoạn đó bit có thể bị lật. Nếu không ai kiểm tra thì `payments` nhận về một request sai mà không hề biết. Ở link layer, việc kiểm tra là của CRC. Mục này nói CRC làm gì, rồi so nó với checksum của Internet mà bạn đã gặp ở TCP và IP.

### 2.1 Bit bị lật và ý tưởng chung

Bit có thể bị lật vì nhiễu điện hoặc nhiễu nhiệt[^sa-err-source]. Muốn phát hiện, ý nền của mọi cách là thêm thông tin dư thừa (redundant) vào frame để bên nhận kiểm tra[^sa-err-redundant]. Cách ngây thơ nhất là gửi hai bản sao. Nó tốn n bit dư cho n bit dữ liệu[^sa-err-twocopies], và bỏ sót được lỗi trùng đúng vị trí ở cả hai bản (suy luận của người soạn). Mục tiêu thật sự là xác suất phát hiện lỗi cao với rất ít bit dư[^sa-err-goal].

### 2.2 CRC làm gì

CRC (cyclic redundancy check) là một trong những kỹ thuật phổ biến nhất, và gần như mọi giao thức link-level mà sách bàn tới đều dùng nó[^sa-err-crc-used]. Ý tưởng, không cần toán sâu:

1. Hai bên thỏa thuận trước một "đa thức chia" C(x). Ethernet dùng một đa thức bậc 32 nổi tiếng[^sa-crc-poly].
2. Bên gửi coi dữ liệu là một đa thức rồi thêm k bit kiểm tra, sao cho cả message chia hết cho C(x)[^sa-crc-divisible].
3. Bên nhận chia lại. Dư khác 0 nghĩa là có lỗi[^sa-crc-remainder].

Con số cho Ethernet: một frame chứa tới 12.000 bit (1500 byte) dữ liệu chỉ cần mã CRC 32 bit (CRC-32)[^sa-crc32]. Mã ngắn như vậy mà vẫn bảo vệ mạnh trước các lỗi bit thường gặp trong message dài hàng nghìn byte[^sa-crc-strong].

### 2.3 Phát hiện rồi, ai xử lý

Khi đã phát hiện lỗi, hướng thứ nhất là báo bên gửi truyền lại một bản sao[^sa-err-two]. Hướng thứ hai là dùng mã sửa lỗi để bên nhận tự khôi phục message[^sa-err-ecc]. Với CRC, khi dư khác 0 thì "có thể cần bỏ" message hỏng[^sa-crc-discard]. Còn truyền lại thuộc về ai? Độ tin cậy là chức năng link-level *có thể* có, nhưng nhiều công nghệ link hiện đại bỏ qua nó; thường nó được cung cấp ở tầng cao hơn[^sa-link-reliab].

Từ đó ta suy ra, với Ethernet, frame sai CRC bị bỏ, và nếu cần truyền lại thì đó là việc của tầng trên như TCP (chương [05](05-tcp-reliability-va-flow-control.md)). Đây là suy luận của người soạn: trong các trang Systems Approach đã đọc (§2.4, §2.5, §2.6) không có câu nào nói thẳng "Ethernet không truyền lại frame hỏng". Vậy với `orders`, một frame chứa request bị hỏng giữa đường sẽ bị bỏ, và nếu cần gửi lại thì TCP lo (cùng suy luận ấy).

Đừng nhầm chuyện này với việc adaptor Ethernet thử lại khi **va chạm** lúc phát (CSMA/CD): adaptor thử lại tới một số lần rồi báo lỗi truyền cho host[^sa-eth-giveup].

### 2.4 CRC khác checksum của Internet

Checksum của Internet là một loại mã khác hẳn: nó coi dữ liệu là dãy số nguyên 16 bit rồi cộng chúng lại[^sa-checksum-16]. Nó không dùng ở link level nhưng làm cùng loại việc với CRC[^sa-checksum-nolink]. Phép cộng này yếu hơn CRC. Một điểm yếu sách chỉ ra: hai lỗi bit, một cái tăng một từ và một cái giảm từ khác cùng lượng, sẽ lọt qua mà không bị phát hiện[^sa-checksum-weak]. Vậy sao nó vẫn đủ dùng? Lý do sách nêu là nó chỉ đóng vai tuyến phòng thủ cuối cùng của giao thức đầu-cuối, còn phần lớn lỗi đã bị CRC ở link level bắt trước[^sa-checksum-ok].

<details><summary>Đi sâu: so checksum cộng 16 bit với CRC trên một ví dụ</summary>

Thí nghiệm nhỏ (`python3` trên host, tự tính): với hai chuỗi 4 byte `12 34 56 78` và `56 78 12 34` (đổi chỗ hai từ 16 bit), checksum cộng 16 bit kiểu Internet đều là `0x9753`, còn `zlib.crc32` (một CRC 32 bit) cho `0x4a090e98` và `0xcc8032ec`. Phép cộng không phụ thuộc thứ tự nên checksum không đổi, CRC thì đổi. Đây là minh họa trên một ví dụ, không phải đo xác suất phát hiện lỗi.

Về từ ngữ: chữ "checksum" hay bị dùng cho mọi loại mã phát hiện lỗi. Sách khuyên chỉ dùng nó cho những mã thực sự dựa trên phép cộng[^sa-checksum-word].

</details>

**Quay lại Q2:** CRC là mã phát hiện frame hỏng (sửa lỗi là phần khác của bài toán), mạnh hơn checksum cộng 16 bit của Internet; với `orders`, frame hỏng bị bỏ ở link layer và việc gửi lại thuộc tầng trên.

## 3. Ethernet: frame và địa chỉ MAC (Q3)

> **Trả lời ngắn.** Từ góc nhìn của host, header Ethernet dài 14 byte: MAC đích 6 byte, MAC nguồn 6 byte và trường type 2 byte[^sa-eth-header]. Payload tối đa 1500 byte[^sa-eth-1500]. Cuối frame là CRC 32 bit[^sa-eth-crc]. Địa chỉ MAC 48 bit thuộc về adaptor chứ không phải host[^sa-eth-addr]. Địa chỉ toàn bit 1 là broadcast: mọi adaptor đẩy frame lên host[^sa-eth-bcast].

**Giải thích.**

*Với service `orders`.* Các quan sát trong chương đều chạy trong một container có IP `172.17.0.3`, và gateway của nó là `172.17.0.1`. Đó là next hop mặc định của container (mục 9). Mỗi datagram `orders` gửi ra khỏi container nằm trong một frame Ethernet, và trong các quan sát frame ấy đi tới `172.17.0.1`. Mục này mở frame đó ra: nó có những trường nào, MAC là địa chỉ của ai, và broadcast nghĩa là gì.

### 3.1 Ethernet: từ đường truyền chung đến điểm-điểm

Ethernet bắt đầu như một đường truyền chia sẻ, dùng CSMA/CD (Carrier Sense, Multiple Access with Collision Detect)[^sa-csmacd]. Ngày nay link Ethernet phần lớn là điểm-điểm: nối host với switch hoặc nối các switch, còn biến thể của cơ chế multiple access sống tiếp ở Wi-Fi[^sa-eth-p2p]. Lịch sử này giải thích vài mô tả bên dưới, chẳng hạn "mọi adaptor nhận mọi frame", vốn xuất phát từ đường truyền chung.

### 3.2 Cấu trúc frame

Header Ethernet nhìn từ host dài 14 byte: hai địa chỉ 6 byte (đích, nguồn) và một trường type 2 byte. Adaptor bên gửi gắn thêm preamble và CRC, adaptor bên nhận gỡ chúng đi[^sa-eth-header]. Trường type là demux key, cho biết giao thức tầng trên nào nhận frame[^sa-eth-type]. Payload tối đa 1500 byte[^sa-eth-1500] và tối thiểu 46 byte; nếu ngắn hơn, host phải đệm (pad) trước khi gửi[^sa-eth-min]. Cuối frame là CRC 32 bit[^sa-eth-crc].

```mermaid
flowchart LR
  D["MAC đích<br/>6 byte"] --> S["MAC nguồn<br/>6 byte"] --> T["Type<br/>2 byte"] --> P["Payload<br/>46..1500 byte"] --> C["CRC<br/>4 byte"]
```

(Sơ đồ minh họa của người soạn; preamble và CRC do adaptor gắn và gỡ[^sa-eth-header].) Header cộng payload đầy là 14 + 1500 = 1514 byte (số học của người soạn). CRC bị adaptor bên nhận gỡ nên không tới được ứng dụng (suy luận của người soạn).

**Quan sát (container).** `tcpdump -e` in header link-level của mỗi frame[^tcpdump-e]. Một gói ping trong LAN của container:

```text
9a:d3:64:fe:4f:18 > 9a:d5:fb:4b:f6:79, ethertype IPv4 (0x0800), length 98: 172.17.0.3 > 172.17.0.1: ICMP echo request, id 1, seq 1, length 64
```

Dòng này đọc từ trái sang: MAC nguồn, MAC đích, rồi `ethertype IPv4 (0x0800)` chính là trường type. `length 98` là 14 byte header Ethernet cộng 84 byte datagram IP (20 byte header IP, 8 byte header ICMP và 56 byte dữ liệu mặc định của `ping`). Tổng 98 do chính dòng quan sát in ra; phần phân rã là phép cộng của người soạn.

### 3.3 Địa chỉ MAC và broadcast

Địa chỉ Ethernet 48 bit thuộc về **adaptor**, không phải host, và thường được ghi sẵn trong ROM[^sa-eth-addr]. Trên Ethernet dùng chung đường truyền, mỗi frame phát ra tới mọi adaptor trên cùng Ethernet đó, và adaptor chỉ chuyển lên host những frame gửi tới địa chỉ của nó[^sa-eth-everyframe] (ngày nay phần lớn link là điểm-điểm, như ở 3.1). Chế độ **promiscuous** thì chuyển lên host mọi frame, và đó không phải chế độ bình thường[^sa-eth-promisc]. `tcpdump` cần chế độ này khi bắt gói không dành cho máy mình (nhận xét của người soạn; cách nó bắt không thuộc chương này).

Ngoài unicast (gửi tới một MAC), có hai loại địa chỉ đặc biệt:

- **Broadcast**: địa chỉ toàn bit 1 (`ff:ff:ff:ff:ff:ff`); mọi adaptor đẩy frame lên host[^sa-eth-bcast].
- **Multicast**: địa chỉ có bit đầu bằng 1 nhưng không phải broadcast; host cấu hình adaptor nhận một tập địa chỉ multicast[^sa-eth-multicast].

Còn một điều cần nhớ: địa chỉ MAC chỉ có nghĩa trong **một** mạng link. Nếu `payments` nằm ở mạng khác thì frame đi qua router, và ở mỗi hop frame được dựng lại với MAC mới trong khi IP đầu-cuối giữ nguyên (nhận định của người soạn; chương [03](03-ip.md)).

<details><summary>Đi sâu: loopback không có MAC thật</summary>

Địa chỉ dạng `{127, <any>}` (viết thành `127.0.0.0/8`, cách viết của người soạn) là địa chỉ loopback nội bộ host và không được xuất hiện ngoài host[^rfc1122-loop]. `ip(7)` nói `INADDR_LOOPBACK` (127.0.0.1) luôn dẫn tới chính máy qua loopback device[^ip7-loop].

**Quan sát:** `tcpdump -e -i lo` thấy MAC nguồn lẫn đích đều là `00:00:00:00:00:00` (cùng giá trị `ip -br link` in cho `lo`), `length 98`, và `lo` có `mtu 65536` thay vì 1500. Vậy nếu `orders` gọi một service cùng máy qua `127.0.0.1` thì không có adaptor thật nào dính vào, nên không có ARP và không gặp MTU 1500 (suy luận của người soạn).

</details>

**Quay lại Q3:** frame Ethernet = header 14 byte + payload 46..1500 byte + CRC 32 bit; MAC thuộc adaptor, `ff:ff:ff:ff:ff:ff` là broadcast. Mỗi request của `orders` ra khỏi container trong một frame như thế.

## 4. Switch và learning bridge (Q4)

> **Trả lời ngắn.** Switch (learning bridge) học "MAC nào ở cổng nào" bằng cách xem địa chỉ nguồn của mọi frame nhận được[^sa-learn-src]. Khi đích chưa có trong bảng, nó flood frame ra mọi cổng khác[^sa-learn-flood]. Mạng có vòng lặp thì frame có thể bị chuyển mãi mãi[^sa-loop]. Giải pháp là chạy spanning tree để loại chu trình[^sa-stp]. Broadcast thì không scale, nên có VLAN chia một mạng L2 thành nhiều mạng "có vẻ riêng biệt"[^sa-vlan-def].

**Giải thích.**

*Với service `orders`.* Frame của `orders` thường không nối thẳng dây tới `payments` mà đi qua ít nhất một switch (nhận xét của người soạn). Trên mạng container, vai switch có thể do một bridge phần mềm đảm nhiệm (mục 9). Câu hỏi của mục này: làm sao switch biết `payments` nằm sau cổng nào, và nó làm gì khi chưa biết?

### 4.1 Nối nhiều đoạn Ethernet

Khi có nhiều đoạn Ethernet thì cần thiết bị nối chúng. Repeater không đủ vì nó chỉ sao chép bit từ cổng này sang cổng khác[^sa-repeater]. **Bridge** thì nhận *cả frame* và chuyển từ Ethernet này sang Ethernet kia (trong ví dụ của sách, nó có một cặp adaptor Ethernet). Để làm được vậy, nó chạy promiscuous mode, nhận mọi frame trên cả hai đoạn[^sa-bridge-promisc]. Bản đơn giản nhất chuyển mọi frame ra mọi cổng khác, và có hạn chế khá nghiêm trọng[^sa-bridge-simple]. "Switch" trong ngữ cảnh Ethernet là bridge hiện đại; hai từ gần như thay thế nhau (nhận xét của người soạn).

### 4.2 Learning bridge: học từ địa chỉ nguồn

Cải tiến đầu tiên: bridge không cần chuyển mọi frame, vì nếu nguồn và đích cùng một phía thì chuyển sang phía kia là thừa[^sa-learn-q]. Còn lại là chuyện bridge biết host nào ở cổng nào. Thay vì nhập bảng bằng tay, ta cho bridge tự học: nó xem địa chỉ nguồn của mọi frame nhận được và ghi "địa chỉ này vừa xuất hiện ở cổng này"[^sa-learn-src]. Bảng lúc khởi động trống. Mỗi mục có timeout và bị bỏ sau một thời gian, để chịu được trường hợp host (và địa chỉ LAN của nó) chuyển sang mạng khác[^sa-learn-timeout]. Khi đích chưa có trong bảng, bridge chuyển frame ra *mọi cổng khác*, gọi là flood[^sa-learn-flood]. Vì vậy bảng chỉ là một tối ưu để lọc bớt frame, **không cần cho tính đúng đắn**[^sa-learn-opt].

Ví dụ: `orders` ở cổng 1 gửi frame tới `payments` ở cổng 2 khi switch còn trống bảng.

```mermaid
sequenceDiagram
  participant A as orders (cổng 1)
  participant SW as Switch
  participant B as payments (cổng 2)
  participant C as Host khác (cổng 3)
  A->>SW: frame orders→payments (payments chưa có trong bảng)
  Note over SW: học: orders ở cổng 1
  SW->>B: flood ra mọi cổng khác
  SW->>C: flood ra mọi cổng khác
  B->>SW: frame payments→orders
  Note over SW: học: payments ở cổng 2
  SW->>A: chỉ ra cổng 1 (orders đã biết)
```

(Sơ đồ minh họa của người soạn theo thuật toán trên; không phải một lần chạy thật vì container lab không tạo được network namespace.)

### 4.3 Vòng lặp và spanning tree

Chiến lược trên chạy tốt cho tới khi mạng có **vòng lặp**. Lúc đó nó thất bại thảm hại: frame có thể bị chuyển mãi mãi[^sa-loop]. Ví dụ trong sách: khi đích chưa có trong bảng của switch nào, S4 flood frame ra hai cổng tới S1 và S6; hai switch này chuyển cho nhau rồi trả về S4, lặp vô hạn theo cả hai chiều[^sa-loop-ex]. Chống vòng lặp phải làm ở cấp topology: các switch chạy thuật toán **spanning tree** phân tán để chọn một tập con của mạng không có chu trình[^sa-stp]. Chi tiết thuật toán ngoài phạm vi chương; xem Đọc thêm.

### 4.4 Broadcast đi khắp nơi, và VLAN

Switch còn phải hỗ trợ broadcast: mỗi switch chuyển frame có đích broadcast ra mọi cổng đang hoạt động, trừ cổng nhận[^sa-bcast-switch]. Hệ quả là host nào trong cùng mạng L2 cũng thấy broadcast của nhau. Ở phạm vi nhỏ như một phòng ban thì chuyện đó còn chấp nhận được, nhưng ở quy mô công ty hay trường đại học khó ai muốn bị làm phiền như vậy. Kết luận của sách: broadcast không scale, nên mạng L2 không scale[^sa-vlan-scale].

**VLAN** (virtual LAN) chia một mạng L2 thành nhiều mạng L2 "có vẻ riêng biệt", mỗi mạng có một định danh[^sa-vlan-def]. Ví dụ của sách: đoạn mạng nối tới host W, X thuộc VLAN 100, đoạn nối tới Y, Z thuộc VLAN 200. Switch nhận frame ở cổng thuộc VLAN 100 sẽ chèn VLAN header vào giữa header Ethernet và payload (vị trí chính xác của trường VID xem phần Đi sâu), và frame, kể cả broadcast, không bao giờ ra cổng không thuộc VLAN 100[^sa-vlan-ex]. Đổi topology logic chỉ cần đổi cấu hình, không phải dời dây hay đổi địa chỉ[^sa-vlan-nowires]. Với `orders`: nếu nó và `payments` ở hai VLAN khác nhau thì broadcast của bên này không tới bên kia (suy luận của người soạn từ ví dụ trên).

<details><summary>Đi sâu: bridge trong kernel Linux, định dạng VLAN tag, <code>ip link</code> kiểu <code>vlan</code></summary>

**Bridge của Linux.** Kernel Linux có cùng ý tưởng. `bridge(8)` ghi `learning on|off` điều khiển việc một cổng có học MAC từ lưu lượng nhận được không; nếu tắt thì bridge flood mọi lưu lượng không có mục trong FDB[^bridge-learning]. Đối tượng `fdb` (forwarding database) của `bridge(8)` chứa các địa chỉ Ethernet đã biết trên một link[^bridge-fdb].

**Định dạng VLAN tag.** Sách nói VLAN ID 12 bit được chèn giữa địa chỉ nguồn và Type[^sa-vlan-tag]. Đó là cách nói cụ thể hơn "giữa header Ethernet và payload" ở trên; cả hai là cách sách mô tả cùng một cơ chế (nhận xét của người soạn), và khi cần vị trí chính xác thì theo câu này. Type `0x8100` báo frame có phần mở rộng VLAN[^sa-vlan-8100]. Trường VID 12 bit có 2^12 = 4096 giá trị (số học của người soạn; đoạn sách đã đọc không nói giá trị nào dành riêng). Sách còn nói có "32 bit" được chèn vào header[^sa-vlan-32], tức header dài hơn (suy luận của người soạn). Hệ quả lên MTU phụ thuộc thiết bị, ngoài nguồn của chương.

**`ip link` kiểu `vlan`.** Trên Linux, `ip link` có kiểu thiết bị `vlan` ("802.1q tagged virtual LAN interface")[^iplink-types]. Kiểu này nhận tham số `id VLANID`[^iplink-vlan-id]. **Quan sát:** `ip link add link br0 name br0.100 type vlan id 100` thành công, và `ip -d link show br0.100` in `vlan protocol 802.1Q id 100`.

</details>

**Quay lại Q4:** switch học từ địa chỉ nguồn và flood khi chưa biết; vòng lặp cần spanning tree; VLAN giới hạn phạm vi broadcast. Với `orders`, frame tới `payments` có thể bị flood ở lần đầu, rồi đi thẳng khi switch đã học.

## 5. ARP: từ địa chỉ IPv4 sang MAC (Q5)

> **Trả lời ngắn.** ARP hỏi "ai có địa chỉ IPv4 này?" bằng một request broadcast tới mọi trạm trên mạng link[^rfc826-bcast]. Trạm có địa chỉ đó trả reply unicast kèm MAC của mình[^rfc826-reply]. Kernel lưu kết quả trong cache và coi mục là stale khi lâu không có phản hồi tích cực[^arp7-positive]. Hỏi mà không ai trả lời thì sau số lần thử tối đa, mục thành `FAILED`[^ipneigh-failed].

**Giải thích.**

*Với service `orders`.* Code của `orders` chỉ biết IP, chẳng hạn `TcpStream::connect("172.17.0.77:80")`. Nhưng frame Ethernet cần MAC đích, nên kernel phải tự đổi IP thành MAC trước khi gửi được byte đầu tiên. Đó là việc của ARP. Và nếu IP kia không còn ai dùng (giả sử instance `payments` đã bị dừng) thì ARP là chỗ đầu tiên cú gọi của `orders` chết; mục 5.4 có quan sát về chuyện đó.

### 5.1 Vì sao cần ARP

Giao thức P (ví dụ IP) quyết định gửi tới host T trên cùng một đoạn Ethernet, và cần địa chỉ Ethernet 48 bit của T. Nhưng địa chỉ của host trong giao thức P không phải lúc nào cũng tương thích với địa chỉ Ethernet tương ứng (khác độ dài hoặc giá trị)[^rfc826-abs]. ARP (RFC 826) là giao thức phân phối động thông tin để dựng bảng dịch địa chỉ đó[^rfc826-dyn]. Trên Linux, `arp(7)` mô tả module kernel này là dùng để chuyển đổi giữa địa chỉ phần cứng Layer 2 và địa chỉ IPv4 **trên các mạng nối trực tiếp**[^arp7-ipv4]. Với `orders`, P là IP, còn T là `payments` (hoặc gateway nếu `payments` ở ngoài mạng link).

### 5.2 Request và reply

Khi gói đi xuống, routing xác định địa chỉ giao thức của hop kế tiếp, rồi module Address Resolution tìm cặp đó trong một bảng[^rfc826-table]. Nếu không thấy, RFC mô tả rằng nó có lẽ báo cho bên gọi biết là đang bỏ gói đó, với giả định tầng mạng cao hơn sẽ gửi lại[^rfc826-throw]. Sau đó nó sinh một gói ARP REQUEST: sender là chính mình, target protocol address là máy cần tìm. Gói này được **broadcast** tới mọi trạm trên cáp[^rfc826-bcast]. Trạm nào thấy `ar$tpa` là địa chỉ của mình thì hoán đổi trường, đặt opcode REPLY và gửi thẳng (unicast) tới phần cứng của bên hỏi[^rfc826-reply].

```mermaid
sequenceDiagram
  participant A as orders (172.17.0.3)
  participant LAN as Mạng link
  participant G as 172.17.0.1
  A->>A: next hop = 172.17.0.1; bảng ARP chưa có
  A->>LAN: ARP request ff:ff:ff:ff:ff:ff — who-has 172.17.0.1 tell 172.17.0.3
  LAN->>G: tới mọi trạm
  G-->>A: ARP reply (unicast): 172.17.0.1 is-at 9a:d5:fb:4b:f6:79
  A->>G: frame IPv4 tới MAC đó (ICMP echo request)
```

**Quan sát (container, `tcpdump -n -e -i eth0 "arp or icmp"` trong khi `ping -c 2 172.17.0.1`).** Các dòng thật, rút gọn thời gian:

```text
9a:d3:64:fe:4f:18 > ff:ff:ff:ff:ff:ff, ethertype ARP (0x0806), length 42: Request who-has 172.17.0.1 tell 172.17.0.3, length 28
9a:d5:fb:4b:f6:79 > 9a:d3:64:fe:4f:18, ethertype ARP (0x0806), length 42: Reply 172.17.0.1 is-at 9a:d5:fb:4b:f6:79, length 28
9a:d3:64:fe:4f:18 > 9a:d5:fb:4b:f6:79, ethertype IPv4 (0x0800), length 98: 172.17.0.3 > 172.17.0.1: ICMP echo request, id 1, seq 1, length 64
9a:d5:fb:4b:f6:79 > 9a:d3:64:fe:4f:18, ethertype IPv4 (0x0800), length 98: 172.17.0.1 > 172.17.0.3: ICMP echo reply, id 1, seq 1, length 64
```

Request đi tới `ff:ff:ff:ff:ff:ff`, reply đi thẳng về MAC bên hỏi, đúng như RFC. Gói ICMP đầu tiên chỉ được gửi *sau* khi reply đến (quan sát: dòng `echo request` đứng sau dòng `Reply`; ở lần chạy này request và reply cách nhau chưa tới 1 ms theo timestamp).

### 5.3 Mục ARP cache sống và chết ra sao

Cache ARP không giữ mục mãi. Khi lâu không có "phản hồi tích cực" về một ánh xạ, mục bị coi là *stale*. Phản hồi tích cực có thể đến từ tầng cao hơn, ví dụ một TCP ACK thành công[^arp7-positive]. Nghĩa là một kết nối TCP đang sống từ `orders` tới `payments` tự giữ cho mục ARP của đường đó còn tươi (suy luận của người soạn từ câu trên).

Để xác minh lại một mục stale, ARP thử hỏi daemon user space trước, rồi probe unicast tới MAC cũ, rồi mới broadcast request mới; và nó chỉ gửi khi có dữ liệu đang chờ[^arp7-reprobe]. Các mặc định liên quan:

- `mcast_solicit`: tối đa 3 lần broadcast trước khi coi là không tới được[^arp7-mcast].
- `retrans_time_ms`: 1000 ms giữa hai lần gửi[^arp7-retrans].
- `base_reachable_time_ms`: 30000 ms[^arp7-reach]. Man page nói mục hợp lệ ít nhất một giá trị ngẫu nhiên trong khoảng từ nửa đến 1,5 lần giá trị này[^arp7-reach-rand].

**Quan sát:** `/proc/sys/net/ipv4/neigh/eth0/` trong container cho `base_reachable_time_ms=30000`, `retrans_time_ms=1000`, `mcast_solicit=3`, `ucast_solicit=3`, `app_solicit=0`, khớp các mặc định trong man page.

### 5.4 `ip neigh`: đọc ARP cache

`ip neigh` thao tác bảng "neighbour", liên kết giữa địa chỉ giao thức và địa chỉ link layer của các host cùng link. Bảng IPv4 còn gọi là ARP table[^ipneigh-arp]. Mỗi mục có một trạng thái; tên trạng thái viết tắt theo NUD, Neighbour Unreachability Detection[^ipneigh-nud]. Bốn trạng thái ta sẽ gặp:

- `reachable`: hợp lệ đến khi hết timeout reachability[^ipneigh-reachable].
- `stale`: hợp lệ nhưng đáng ngờ[^ipneigh-stale].
- `incomplete`: chưa được xác nhận hay giải quyết[^ipneigh-incomplete].
- `failed`: vượt số probe tối đa mà không thành công[^ipneigh-failed].

Man page còn liệt kê `permanent`, `noarp`, `none`, `delay`, `probe`[^ipneigh-states]; các trạng thái đó không xuất hiện trong thí nghiệm dưới đây.

**Quan sát (container).** Ban đầu `ip neigh` trống. Sau `ping -c1 172.17.0.1`:

```text
172.17.0.1 dev eth0 lladdr 9a:d5:fb:4b:f6:79 REACHABLE
```

Rồi ping `172.17.0.77` (không ai dùng) với `tcpdump -n -e -tt -i eth0 arp` chạy song song: **ba** request broadcast cách nhau khoảng 1 giây, không có reply, và `ip neigh show 172.17.0.77` sau đó in `FAILED`:

```text
1791739210.491028 ... Request who-has 172.17.0.77 tell 172.17.0.3, length 28
1791739211.516199 ... Request who-has 172.17.0.77 tell 172.17.0.3, length 28
1791739212.543682 ... Request who-has 172.17.0.77 tell 172.17.0.3, length 28
172.17.0.77 dev eth0 FAILED
```

Ba lần cách nhau ~1 s khớp `mcast_solicit=3` và `retrans_time_ms=1000`. Ở một lần chạy trước, ngay sau `ping -c1 -W1 172.17.0.77`, mục này là `INCOMPLETE`; tôi không theo dõi liên tục nên không đo thời điểm chuyển trạng thái.

### 5.5 Khi `payments` không còn ai trả lời ARP

Đây là chỗ ARP chạm vào backend. Giả sử IP của `payments` không còn ai dùng. **Quan sát** (một lần chạy Python `socket.connect(('172.17.0.77', 80))` song song với `tcpdump arp`): `connect()` không nhận RST mà thất bại với `OSError(113, 'No route to host')` (EHOSTUNREACH) sau khoảng 3,06 s, trong lúc `ip neigh` chuyển sang `FAILED` sau ba ARP request cách nhau ~1 s. Gói TCP SYN có lẽ không được phát vì chưa có MAC đích (suy luận của người soạn: `tcpdump` chỉ lọc `arp` nên không thấy SYN). Con số ~3 s khớp 3 × 1 s nhưng chỉ là một lần đo, không phải bảo đảm của docs.

Vậy với `orders`, lỗi "No route to host" sau vài giây không nhất thiết là chuyện bảng định tuyến; nó có thể là ARP không tìm được máy (nhận xét của người soạn, từ quan sát trên).

<details><summary>Đi sâu: quy tắc ghi bảng của RFC 826, định dạng gói ARP và cấu hình module ARP của Linux</summary>

**Quy tắc ghi bảng của RFC 826.** Bên nhận **ghi bên gửi vào bảng trước khi xét opcode**, vì RFC giả định giao tiếp hai chiều: nếu A có lý do nói với B thì nhiều khả năng B cũng có lý do nói lại với A[^rfc826-merge-first]. Theo thuật toán của RFC, nếu cặp đã có trong bảng thì cập nhật; nếu chưa có thì chỉ thêm mới khi bên nhận chính là target protocol address[^rfc826-add]. Ngoài ra, nếu cặp `<protocol type, sender protocol address>` đã có trong bảng thì địa chỉ phần cứng mới **thay** địa chỉ cũ[^rfc826-supersede]. RFC cũng giải thích vì sao không quảng bá định kỳ: việc đó hoàn toàn không được mong muốn[^rfc826-noperiodic]. Giao thức phân phối thông tin khi cần, và (nhiều khả năng) chỉ một lần mỗi lần máy khởi động[^rfc826-needed].

**Gói ARP.** RFC 826 định nghĩa phần dữ liệu của gói gồm loại phần cứng, loại giao thức, độ dài hai loại địa chỉ và opcode (REQUEST = 1, REPLY = 2)[^rfc826-defs]. Tiếp theo là bốn địa chỉ: sender hardware/protocol và target hardware/protocol[^rfc826-format]. Với Ethernet 10 Mbit (loại RFC nói tới), `<ar$hrd, ar$hln>` là `<1, 6>`[^rfc826-gen-hrd]. Gói ARP đi trong frame Ethernet có type riêng[^rfc826-etype]. `tcpdump` hiện `ethertype ARP (0x0806)` và `length 42`, tức 14 byte header cộng 28 byte gói ARP (quan sát).

**Cấu hình module ARP của Linux.** Module ARP giữ một cache ánh xạ địa chỉ phần cứng ↔ địa chỉ giao thức. Cache có kích thước giới hạn nên mục cũ, ít dùng bị garbage-collect, trừ mục permanent[^arp7-cache]. Nếu số mục vượt `gc_thresh3` (mặc định 1024) thì GC luôn chạy[^arp7-gc3]. Người dùng thường chỉ cấu hình module này, vì nó phục vụ các giao thức khác trong kernel[^arp7-user]. Bảng cũng điều khiển được bằng `ioctl(2)` trên socket `AF_INET` bất kỳ[^arp7-ioctl] (đường của công cụ quản trị, không phải đường dữ liệu). Với `connect` và `bind` ở chương [07](07-socket-va-kernel.md), bạn chỉ đưa IP và port; việc dịch sang MAC do kernel làm (suy luận của người soạn).

</details>

**Quay lại Q5:** request broadcast, reply unicast; cache có vòng đời stale/reachable; không ai trả lời thì sau ba request mục thành `FAILED`, và `connect` của `orders` tới IP đó thất bại sau khoảng 3 s (quan sát một lần).

## 6. IPv6: Neighbor Discovery thay ARP (Q6)

> **Trả lời ngắn.** IPv6 dùng Neighbor Discovery (RFC 4861), tương ứng với tổ hợp ARP, ICMP Router Discovery và ICMP Redirect của IPv4[^rfc4861-comb]. Phân giải địa chỉ làm bằng Neighbor Solicitation multicast tới đích, và đích trả Neighbor Advertisement unicast[^rfc4861-ns].

**Giải thích.**

*Với service `orders`.* Nếu `orders` và `payments` nói chuyện qua IPv6 thì ARP không còn tham gia: `arp(7)` chỉ nói về IPv4[^arp7-ipv4]. Việc đổi địa chỉ lớp mạng sang MAC khi đó thuộc về Neighbor Discovery.

IPv6 dùng **Neighbor Discovery** (RFC 4861), tương ứng với tổ hợp ARP, ICMP Router Discovery và ICMP Redirect của IPv4[^rfc4861-comb]. Để phân giải địa chỉ, nó gửi Neighbor Solicitation multicast tới địa chỉ solicited-node của đích, và đích trả địa chỉ link-layer của mình bằng Neighbor Advertisement unicast[^rfc4861-ns]. So với broadcast của ARP, multicast ở đây chỉ gõ cửa nhóm máy có thể là đích (nhận xét của người soạn). Khác ARP, ND còn phát hiện lỗi nửa-liên-kết (half-link) bằng Neighbor Unreachability Detection[^rfc4861-nud]. Chi tiết (Router Advertisement, DAD, các option) ngoài phạm vi chương.

Container lab không có địa chỉ IPv6 ngoài `::1` (quan sát: `ip -6 neigh` trống, `ip -6 addr` chỉ có `::1/128` trên `lo`), nên không có gì để bắt ở đây.

<details><summary>Đi sâu: <code>ip neigh</code> dùng chung cho hai họ địa chỉ</summary>

Cùng một lệnh `ip neigh` phục vụ cả hai họ địa chỉ[^ipneigh-addr].

</details>

**Quay lại Q6:** IPv6 thay ARP bằng Neighbor Discovery (Solicitation multicast, Advertisement unicast).

## 7. MTU và jumbo frame (Q7)

> **Trả lời ngắn.** MTU là datagram IP lớn nhất mà một loại mạng chở được trong một frame[^sa-mtu]. Với Ethernet chuẩn đó là 1500 byte, trùng giới hạn payload của frame[^sa-eth-1500]. Ethernet cũng cho jumbo packet tới 9000 byte dữ liệu[^sa-eth-jumbo], nhưng cần mọi thiết bị trên đường cùng nới MTU (suy luận của người soạn).

**Giải thích.**

*Với service `orders`.* Giả sử `payments` trả về một response khá lớn. Response đó không đi hết trong một frame: mỗi frame chỉ chở được tối đa MTU byte datagram, nên nó phải được cắt thành nhiều gói (chia ở tầng nào là chuyện của chương [03](03-ip.md) và [04](04-udp-va-tcp-co-ban.md)). Mục này chỉ nói con số giới hạn ấy là bao nhiêu và vì sao nó khó đổi.

**MTU** của một loại mạng là datagram IP lớn nhất nó chở được trong một frame[^sa-mtu]. Với Ethernet chuẩn là 1500 byte, trùng giới hạn payload ở mục 3[^sa-eth-1500]. Ethernet cũng cho phép jumbo packet tới 9000 byte dữ liệu thay vì 1500[^sa-eth-jumbo]. Phía TCP, RFC 9293 nêu một mục tiêu khi gửi segment nhỏ hơn: tránh segment khiến IP datagram lớn hơn MTU nhỏ nhất trên đường đi, vì điều đó gây mất gói hoặc phân mảnh[^rfc9293-segsmall] (chương [03](03-ip.md)).

Vì sao jumbo không bật tùy tiện được? Gói 9000 byte chỉ đi hết đường khi mọi thiết bị trên đường chịu MTU đó (suy luận của người soạn); một chặng còn ở 1500 là đủ để gói lớn bị chặn.

<details><summary>Đi sâu: quan sát <code>ping -M do</code> và đổi MTU trong container</summary>

**Quan sát (container).** `eth0` có MTU 1500 (`ip link show eth0`: `mtu 1500`; `/sys/class/net/eth0/mtu` = `1500`). Giới hạn thể hiện rõ với `ping -M do` (cấm phân mảnh):

```text
$ ping -c1 -M do -s 1472 172.17.0.1   -> 1480 bytes from 172.17.0.1: icmp_seq=1 ttl=64 ...
$ ping -c1 -M do -s 1473 172.17.0.1   -> ping: local error: message too long, mtu=1500
```

`-s N` là N byte dữ liệu ICMP. Cộng 8 byte header ICMP và 20 byte header IP ra 1500 với N = 1472 và 1501 với N = 1473 (số học của người soạn, khớp "1473(1501) bytes" mà `ping` in). MTU là thuộc tính của *từng interface*; `ip link` có tham số `mtu NUMBER`[^iplink-mtu].

Trong container, `ip link set br0 mtu 9000` được chấp nhận và `ip link show br0` in `mtu 9000`. Điều đó chỉ chứng tỏ kernel nhận cấu hình, không chứng minh cả đường chạy được khung 9000 (suy luận của người soạn: mọi thiết bị trên đường đều phải nới MTU). Nhận xét của người soạn: MTU lệch giữa hai đầu overlay hoặc VPN là nguồn kinh điển của chuyện "request nhỏ của `orders` chạy, response lớn của `payments` treo".

</details>

**Quay lại Q7:** MTU Ethernet là 1500 byte; jumbo ~9000 chỉ chạy được khi cả đường cấu hình.

## 8. Mục phụ trợ: Wi-Fi (giới thiệu)

> **Trả lời ngắn.** Wi-Fi (802.11) giống Ethernet ở chỗ dùng trong phạm vi hạn chế, nhưng sóng vô tuyến là môi trường chia sẻ nên thách thức chính là điều phối truy cập[^sa-wifi]. Nó dùng CSMA/CA thay vì collision detection[^sa-csma-ca].

**Giải thích.**

*Với service `orders`.* Trong container `orders` ít khi đi qua Wi-Fi, nên mục này chỉ để bạn biết nửa còn lại của link layer. Nếu bạn chạy `orders` trên laptop lúc phát triển và gọi service khác qua Wi-Fi thì frame của bạn đi bằng 802.11 (nhận xét của người soạn).

Wi-Fi (IEEE 802.11) giống Ethernet ở chỗ dùng trong phạm vi địa lý hạn chế. Thách thức chính của nó là điều phối truy cập một môi trường chia sẻ, vì sóng truyền trong không gian[^sa-wifi]. 802.11 dùng CSMA/CA (collision avoidance), đối lập với collision detection của Ethernet[^sa-csma-ca]. Một phần của CSMA/CA là ACK tường minh từ bên nhận[^sa-wifi-ack]. Hầu hết mạng 802.11 dùng topology dựa trên trạm gốc[^sa-wifi-base]. 802.11 gọi các trạm gốc này là access point (AP)[^sa-wifi-ap]. PHY, bảo mật WPA và roaming ngoài phạm vi chương.

## 9. Network interface trên Linux (Q8)

> **Trả lời ngắn.** Trên Linux mỗi điểm gắn mạng là một network interface, quản lý bằng `ip`[^ip8-desc]: `ip link` cho thiết bị, `ip neigh` cho bảng neighbour/ARP. `lo` là loopback. veth là cặp Ethernet ảo, gói gửi vào một đầu được nhận ngay trên đầu kia[^veth-imm]. Bridge là thiết bị kiểu Ethernet Bridge[^iplink-bridge], đóng vai switch như ở mục 4 (nhận xét của người soạn).

**Giải thích.**

*Với service `orders`.* Chạy `ip link` trong container của `orders` bạn chỉ thấy vài interface, trong đó `eth0` là chỗ mọi frame của `orders` ra khỏi container. Mục này đọc từng dòng đó, và cho thấy `eth0` thật ra là một đầu của cặp veth nối ra mạng của host (suy luận của người soạn, xem dưới).

`ip(8)` là công cụ để xem và thao tác định tuyến, thiết bị mạng, interface và tunnel[^ip8-desc]. Trong chương này ta dùng `ip link` (thiết bị) và `ip neigh` (bảng neighbour/ARP).

### 9.1 Đọc `ip -br link` trong container

**Quan sát:**

```text
lo               UNKNOWN        00:00:00:00:00:00 <LOOPBACK,UP,LOWER_UP>
eth0@if108       UP             b2:17:cb:01:40:b5 <BROADCAST,MULTICAST,UP,LOWER_UP>
```

`lo` là loopback (mục 3). Hậu tố `@if108` của `eth0` cho biết nó là một đầu của cặp ảo có đầu kia mang chỉ số 108 ngoài namespace của container (`ip link show eth0` còn in `link-netnsid 0`; `ip -d link show eth0` in dòng `veth`). `ip route` trong container cho `default via 172.17.0.1 dev eth0` và `172.17.0.0/16 dev eth0 proto kernel scope link`. Nghĩa là next hop mặc định `172.17.0.1` chính là địa chỉ mục 5 đã ARP.

### 9.2 veth, bridge, vlan: các thiết bị ảo

`veth(4)` là thiết bị Ethernet ảo luôn tạo thành **cặp**[^veth-pair]. Gói truyền trên một đầu được nhận ngay trên đầu kia[^veth-imm]. Nhờ vậy cặp này dùng được như đường hầm giữa các network namespace, hoặc như thiết bị độc lập[^veth-desc]. Cùng với `bridge` (switch phần mềm, mục 4) và `vlan` (cũng ở mục 4), đó là nguyên liệu của mạng container: một đầu veth nằm trong container của `orders`, đầu kia gắn vào bridge của host (suy luận của người soạn từ `eth0@if108` và gateway `172.17.0.1`; tôi không kiểm tra phía host). Xem [chương mạng container](../08-containers/07-mang-container.md).

<details><summary>Đi sâu: tự dựng bridge, veth, vlan trong container</summary>

**Quan sát: tự dựng trong container.**

```text
ip link add br0 type bridge
ip link add va type veth peer name pa
ip link set va master br0 ; ip link set pa master br0
ip link add link br0 name br0.100 type vlan id 100
```

`ip -br link` sau đó liệt kê `br0`, `pa@va`, `va@pa`, `br0.100@br0`; `bridge link` cho `pa@va` và `va@pa` với `master br0 state disabled` (mới tạo, chưa up). Tôi không chạy được thí nghiệm "hai host qua bridge rồi đọc `bridge fdb show`" vì cần `ip netns add`, bị từ chối trong container; sơ đồ ở mục 4 vì thế chỉ là minh họa lý thuyết. `ip link` cũng ghi `loopback`, `bridge`, `wireless` là thiết bị *network namespace local*, không chuyển sang namespace khác được[^iplink-local].

</details>

**Quay lại Q8:** `ip link` liệt kê thiết bị (lo, eth0 là đầu của một cặp veth), `ip neigh` liệt kê bảng ARP; bridge, veth, vlan là nguyên liệu mạng container.

## 10. Liên hệ với Rust

Với `orders` viết bằng Rust, chương này không bắt bạn đổi dòng code nào: mọi thứ ở đây diễn ra dưới `std::net`. Cái bạn nhận được là hiểu vì sao Rust không hề nhắc tới MAC, ARP hay MTU, và biết dùng công cụ nào khi cần nhìn xuống tầng 2 (nhận xét của người soạn).

**`std::net` chỉ biết IP và cổng.** `SocketAddr` gồm một địa chỉ IP, số hiệu cổng 16 bit và có thể thêm vài thông tin phụ thuộc phiên bản IP[^std-socketaddr]. Nó **không có địa chỉ MAC**. Hệ quả là chương trình Rust dùng `TcpStream` hay `UdpSocket` không thấy và không điều khiển ARP, MTU hay VLAN; kernel xử lý hết (nhận xét của người soạn, dựa trên `SocketAddr` ở trên). Một điểm chạm nhỏ: `Ipv4Addr::is_loopback` kiểm tra `127.0.0.0/8` và trả `true` ở đó[^std-loopback], đúng khoảng loopback ở mục 3.

**Muốn đụng tới frame thì cần packet socket.** Đây là loại socket của Linux để nhận hoặc gửi gói thô ở tầng device driver (OSI Layer 2)[^packet7-l2]. `SOCK_RAW` cho gói gồm cả header link-level, còn `SOCK_DGRAM` thì gỡ header đi[^packet7-types]. Tạo socket cần `CAP_NET_RAW`[^packet7-cap]. Tôi không kiểm tra `std` 1.95.0 có API riêng cho nó không; nếu không thì cần `libc` hoặc crate ngoài (suy luận chưa kiểm chứng), và việc đó ngoài phạm vi chương.

Còn khi debug một service Rust gặp lỗi mạng, `tcpdump -e` và `ip neigh` thường đã đủ cho tầng 2 (nhận xét của người soạn).

## Tóm tắt

- Link layer chuyển datagram tới hop kế bên: đóng khung bit thành frame và phát hiện frame hỏng bằng CRC; sửa hay gửi lại là chuyện của tầng trên (suy luận của người soạn, mục 2).
- Frame Ethernet: header 14 byte (MAC đích, MAC nguồn, Type) + payload 46..1500 byte + CRC 32 bit; Type là demux key (`0x0800` IPv4, `0x0806` ARP).
- MAC thuộc adaptor và chỉ có nghĩa trong một mạng link; `ff:ff:ff:ff:ff:ff` là broadcast; loopback hiện MAC toàn 0.
- Learning bridge học địa chỉ nguồn → cổng và flood khi chưa biết (bảng chỉ là tối ưu); vòng lặp cần spanning tree; broadcast không scale nên có VLAN.
- ARP: request broadcast, reply unicast. Cache có trạng thái `REACHABLE/STALE/INCOMPLETE/FAILED`; không ai trả lời thì 3 request cách ~1 s rồi `FAILED`, và `connect` của `orders` tới IP đó thất bại sau khoảng 3 s (quan sát một lần).
- IPv6 dùng Neighbor Discovery thay ARP.
- MTU Ethernet 1500 (jumbo ~9000 nếu cả đường cấu hình); `ping -M do -s 1473` bị chặn tại máy gửi.
- Linux: `ip link`, `ip neigh`; veth theo cặp, bridge là switch phần mềm; `std::net` của Rust không có khái niệm MAC.

## Câu hỏi tự kiểm tra

1. Framing giải quyết vấn đề gì; character stuffing để làm gì?
<details><summary>Đáp án</summary>

Bên nhận phải biết frame bắt đầu và kết thúc ở đâu. Với cách dùng ký tự sentinel, nếu sentinel xuất hiện trong dữ liệu thì phải thoát nó bằng cách chèn thêm ký tự, gọi là character stuffing[^sa-stuff]. Cách đếm byte thì rủi ro khi trường đếm bị hỏng[^sa-bytecount-risk].

</details>

2. CRC có sửa được lỗi không? Bên nhận biết lỗi thế nào?
<details><summary>Đáp án</summary>

Trong sách CRC dùng để phát hiện, không dùng để sửa: dư khác 0 khi chia cho đa thức đã thỏa thuận là có lỗi[^sa-crc-remainder]. Xử lý lỗi đã phát hiện là truyền lại[^sa-err-two]. Cách khác là dùng mã sửa lỗi[^sa-err-ecc].

</details>

3. Vì sao đảo hai từ 16 bit không đổi checksum Internet nhưng đổi CRC (mục 2)?
<details><summary>Đáp án</summary>

Checksum là phép cộng nên không phụ thuộc thứ tự (cùng `0x9753`), còn CRC là chia đa thức (`0x4a090e98` so với `0xcc8032ec`). Sách coi checksum yếu hơn CRC[^sa-checksum-weak].

</details>

4. Một frame Ethernet đầy gồm những phần nào, bao nhiêu byte?
<details><summary>Đáp án</summary>

Header 14 byte[^sa-eth-header]. Payload tối đa 1500 byte[^sa-eth-1500]. Rồi CRC 32 bit ở cuối[^sa-eth-crc].

</details>

5. Switch gặp đích chưa có trong bảng thì làm gì; vì sao bảng không cần cho tính đúng?
<details><summary>Đáp án</summary>

Nó flood frame ra mọi cổng khác[^sa-learn-flood]. Bảng chỉ là tối ưu để lọc bớt frame[^sa-learn-opt].

</details>

6. Mạng có vòng lặp thì sao, giải pháp gì?
<details><summary>Đáp án</summary>

Frame bị chuyển lặp mãi[^sa-loop-ex]. Giải pháp là các switch chạy spanning tree để loại chu trình[^sa-stp].

</details>

7. VLAN giải quyết gì và thêm gì vào frame?
<details><summary>Đáp án</summary>

VLAN giới hạn phạm vi broadcast bằng cách chia mạng L2 thành nhiều mạng "có vẻ riêng biệt"[^sa-vlan-def]. Về định dạng, sách nói nó chèn VLAN ID 12 bit giữa địa chỉ nguồn và Type[^sa-vlan-tag].

</details>

8. Mô tả ARP request/reply và hai quy tắc ghi bảng của RFC 826.
<details><summary>Đáp án</summary>

Request được broadcast[^rfc826-bcast]. Reply được gửi unicast[^rfc826-reply]. Hai quy tắc ghi bảng nằm ở phần Đi sâu của mục 5. Một là bên nhận ghi bên gửi vào bảng trước khi xét opcode[^rfc826-merge-first]. Hai là địa chỉ phần cứng mới thay địa chỉ cũ của cùng cặp[^rfc826-supersede].

</details>

9. `orders` gọi một IP cùng subnet nhưng không ai dùng: tcpdump và `ip neigh` cho thấy gì?
<details><summary>Đáp án</summary>

Mục 5.4: ba request broadcast cách nhau ~1 s, không có reply, rồi mục thành `FAILED`. Số lần khớp mặc định `mcast_solicit=3`[^arp7-mcast]. Khoảng cách khớp mặc định `retrans_time_ms=1000`[^arp7-retrans]. `failed` nghĩa là vượt số probe tối đa mà không thành công[^ipneigh-failed]. Ở mục 5.5, `connect()` của `orders` thất bại với EHOSTUNREACH sau khoảng 3 s (quan sát một lần).

</details>

10. IPv6 thay ARP bằng gì?
<details><summary>Đáp án</summary>

Bằng Neighbor Discovery: Solicitation multicast, Advertisement unicast[^rfc4861-ns]. ND còn có thêm NUD để phát hiện lỗi nửa-liên-kết[^rfc4861-nud].

</details>

11. `ping -M do -s 1473` trên link MTU 1500 bị gì?
<details><summary>Đáp án</summary>

1473 + 8 + 20 = 1501 > 1500 nên gói bị chặn ngay tại máy gửi ("message too long, mtu=1500", mục 7). MTU là datagram IP lớn nhất mà một frame chở được[^sa-mtu].

</details>

12. Vì sao `TcpStream` không thấy MAC, và muốn thấy thì dùng gì?
<details><summary>Đáp án</summary>

`SocketAddr` chỉ có IP, cổng và vài thông tin phụ thuộc phiên bản IP[^std-socketaddr]. Muốn thấy frame thì cần packet socket tầng L2[^packet7-l2]. Tạo nó cần quyền `CAP_NET_RAW`[^packet7-cap].

</details>

## Đọc thêm

- [Systems Approach §2.3–2.4 (Framing, Error Detection)](https://book.systemsapproach.org/direct/framing.html) — framing và cách CRC hoạt động chi tiết hơn (đa thức, phép chia).
- [Systems Approach §2.6 (Ethernet)](https://book.systemsapproach.org/direct/ethernet.html) — CSMA/CD, kích thước frame tối thiểu và vì sao.
- [Systems Approach §3.2 (Switched Ethernet)](https://book.systemsapproach.org/internetworking/ethernet.html) — learning bridge, spanning tree algorithm, VLAN.
- [Systems Approach §2.7.2 (Wi-Fi)](https://book.systemsapproach.org/direct/wireless.html) — hidden node, CSMA/CA, RTS/CTS, association.
- [RFC 826](https://www.rfc-editor.org/rfc/rfc826.html) — bản gốc ARP, ngắn; có ví dụ X–Y đi từng bước.
- [arp(7)](https://manpages.debian.org/bookworm/manpages/arp.7.en.html) — các tham số sysctl của ARP cache trên Linux.
- [RFC 4861](https://www.rfc-editor.org/rfc/rfc4861.html) — Neighbor Discovery cho IPv6.
- [ip-neighbour(8)](https://manpages.debian.org/bookworm/iproute2/ip-neighbour.8.en.html), [ip-link(8)](https://manpages.debian.org/bookworm/iproute2/ip-link.8.en.html), [bridge(8)](https://manpages.debian.org/bookworm/iproute2/bridge.8.en.html) — tra cứu lệnh dùng trong chương.
- [Chương 03 — IP](03-ip.md) và [mạng container](../08-containers/07-mang-container.md) — chương kế tiếp và nơi veth/bridge được dùng thật.

[^sa-osi-frame]: Computer Networks: A Systems Approach, §1.3.4 OSI Model — <https://book.systemsapproach.org/foundation/architecture.html> — "The data link layer then collects a stream of bits into a larger aggregate called a frame."
[^sa-framing-general]: Computer Networks: A Systems Approach, §2.3 Framing — <https://book.systemsapproach.org/direct/framing.html> — "the problem is a fundamental one that must also be addressed in multiple-access networks like Ethernet and Wi-Fi."
[^sa-sentinel]: Computer Networks: A Systems Approach, §2.3.1 Byte-Oriented Protocols (PPP) — <https://book.systemsapproach.org/direct/framing.html> — "The first is to use special characters known as sentinel characters to indicate where frames start and end."
[^sa-stuff]: Computer Networks: A Systems Approach, §2.3.1 Byte-Oriented Protocols (PPP) — <https://book.systemsapproach.org/direct/framing.html> — "This approach is often called character stuffing because extra characters are inserted in the data portion of the frame."
[^sa-bytecount]: Computer Networks: A Systems Approach, §2.3.1 Byte-Oriented Protocols (PPP) — <https://book.systemsapproach.org/direct/framing.html> — "The alternative to detecting the end of a frame with a sentinel value is to include the number of bytes in the frame at the beginning of the frame, in the frame header."
[^sa-bytecount-risk]: Computer Networks: A Systems Approach, §2.3.1 Byte-Oriented Protocols (PPP) — <https://book.systemsapproach.org/direct/framing.html> — "One danger with this approach is that a transmission error could corrupt the count field, in which case the end of the frame would not be correctly detected."
[^sa-eth-bitoriented]: Computer Networks: A Systems Approach, §2.6.2 Ethernet (Frame Format) — <https://book.systemsapproach.org/direct/ethernet.html> — "Like the HDLC protocol described in an earlier section, the Ethernet is a bit-oriented framing protocol."
[^sa-err-source]: Computer Networks: A Systems Approach, §2.4 Error Detection — <https://book.systemsapproach.org/direct/error.html> — "bit errors are sometimes introduced into frames. This happens, for example, because of electrical interference or thermal noise."
[^sa-err-redundant]: Computer Networks: A Systems Approach, §2.4 Error Detection — <https://book.systemsapproach.org/direct/error.html> — "The basic idea behind any error detection scheme is to add redundant information to a frame that can be used to determine if errors have been introduced."
[^sa-err-goal]: Computer Networks: A Systems Approach, §2.4 Error Detection — <https://book.systemsapproach.org/direct/error.html> — "the goal of error detecting codes is to provide a high probability of detecting errors combined with a relatively low number of redundant bits."
[^sa-err-crc-used]: Computer Networks: A Systems Approach, §2.4 Error Detection — <https://book.systemsapproach.org/direct/error.html> — "One of the most common techniques for detecting transmission errors is a technique known as the cyclic redundancy check (CRC). It is used in nearly all the link-level protocols discussed in this chapter."
[^sa-err-two]: Computer Networks: A Systems Approach, §2.4 Error Detection — <https://book.systemsapproach.org/direct/error.html> — "One is to notify the sender that the message was corrupted so that the sender can retransmit a copy of the message."
[^sa-crc-poly]: Computer Networks: A Systems Approach, §2.4.2 Cyclic Redundancy Check — <https://book.systemsapproach.org/direct/error.html> — "the Ethernet standard uses a well-known polynomial of degree 32."
[^sa-crc-remainder]: Computer Networks: A Systems Approach, §2.4.2 Cyclic Redundancy Check — <https://book.systemsapproach.org/direct/error.html> — "the receiver will obtain a nonzero remainder implying that an error has occurred."
[^sa-crc32]: Computer Networks: A Systems Approach, §2.4 Error Detection — <https://book.systemsapproach.org/direct/error.html> — "a frame carrying up to 12,000 bits (1500 bytes) of data requires only a 32-bit CRC code, or as it is commonly expressed, uses CRC-32."
[^sa-crc-strong]: Computer Networks: A Systems Approach, §2.4.2 Cyclic Redundancy Check — <https://book.systemsapproach.org/direct/error.html> — "a 32-bit CRC gives strong protection against common bit errors in messages that are thousands of bytes long."
[^sa-checksum-word]: Computer Networks: A Systems Approach, §2.4 Error Detection — <https://book.systemsapproach.org/direct/error.html> — "we urge you to use the word checksum only to apply to codes that actually do use addition"
[^sa-checksum-weak]: Computer Networks: A Systems Approach, §2.4.1 Internet Checksum Algorithm — <https://book.systemsapproach.org/direct/error.html> — "a pair of single-bit errors, one of which increments a word and one of which decrements another word by the same amount, will go undetected."
[^sa-checksum-ok]: Computer Networks: A Systems Approach, §2.4.1 Internet Checksum Algorithm — <https://book.systemsapproach.org/direct/error.html> — "one reason it is adequate is that this checksum is the last line of defense in an end-to-end protocol. The majority of errors are picked up by stronger error detection algorithms, such as CRCs, at the link level."
[^sa-crc-discard]: Computer Networks: A Systems Approach, §2.4.2 Cyclic Redundancy Check — <https://book.systemsapproach.org/direct/error.html> — "If the result is nonzero, it may be necessary to discard the corrupted message;"
[^sa-link-reliab]: Computer Networks: A Systems Approach, §2.5 Reliable Transmission — <https://book.systemsapproach.org/direct/reliable.html> — "reliability is a function that may be provided at the link level, but many modern link technologies omit this function. Furthermore, reliable delivery is frequently provided at higher levels, including both transport and sometimes, the application layer."
[^sa-eth-giveup]: Computer Networks: A Systems Approach, §2.6.2 Transmitter Algorithm — <https://book.systemsapproach.org/direct/ethernet.html> — "The adaptor gives up after a given number of tries and reports a transmit error to the host."
[^sa-csmacd]: Computer Networks: A Systems Approach, §2.6 Ethernet and Multiple Access Networks — <https://book.systemsapproach.org/direct/ethernet.html> — "The more general name for the technology behind the Ethernet is Carrier Sense, Multiple Access with Collision Detect (CSMA/CD)."
[^sa-eth-p2p]: Computer Networks: A Systems Approach, §2.6 Ethernet and Multiple Access Networks — <https://book.systemsapproach.org/direct/ethernet.html> — "Modern Ethernet links are now largely point to point; that is, they connect one host to an Ethernet switch, or they interconnect switches."
[^sa-eth-header]: Computer Networks: A Systems Approach, §2.6.2 Ethernet (Frame Format) — <https://book.systemsapproach.org/direct/ethernet.html> — "Ethernet frame has a 14-byte header: two 6-byte addresses and a 2-byte type field. The sending adaptor attaches the preamble and CRC before transmitting, and the receiving adaptor removes them."
[^sa-eth-type]: Computer Networks: A Systems Approach, §2.6.2 Ethernet (Frame Format) — <https://book.systemsapproach.org/direct/ethernet.html> — "The packet type field serves as the demultiplexing key; it identifies to which of possibly many higher-level protocols this frame should be delivered."
[^sa-eth-1500]: Computer Networks: A Systems Approach, §2.6.2 Ethernet (Frame Format) — <https://book.systemsapproach.org/direct/ethernet.html> — "Each frame contains up to 1500 bytes of data."
[^sa-eth-min]: Computer Networks: A Systems Approach, §2.6.2 Ethernet (Frame Format) — <https://book.systemsapproach.org/direct/ethernet.html> — "Minimally, a frame must contain at least 46 bytes of data, even if this means the host has to pad the frame before transmitting it."
[^sa-eth-crc]: Computer Networks: A Systems Approach, §2.6.2 Ethernet (Frame Format) — <https://book.systemsapproach.org/direct/ethernet.html> — "Finally, each frame includes a 32-bit CRC."
[^sa-eth-addr]: Computer Networks: A Systems Approach, §2.6.2 Ethernet (Addresses) — <https://book.systemsapproach.org/direct/ethernet.html> — "Technically, the address belongs to the adaptor, not the host; it is usually burned into ROM."
[^sa-eth-everyframe]: Computer Networks: A Systems Approach, §2.6.2 Ethernet (Addresses) — <https://book.systemsapproach.org/direct/ethernet.html> — "Each frame transmitted on an Ethernet is received by every adaptor connected to that Ethernet. Each adaptor recognizes those frames addressed to its address and passes only those frames on to the host."
[^sa-eth-promisc]: Computer Networks: A Systems Approach, §2.6.2 Ethernet (Addresses) — <https://book.systemsapproach.org/direct/ethernet.html> — "in which case it delivers all received frames to the host, but this is not the normal mode."
[^sa-eth-bcast]: Computer Networks: A Systems Approach, §2.6.2 Ethernet (Addresses) — <https://book.systemsapproach.org/direct/ethernet.html> — "an Ethernet address consisting of all 1s is treated as a broadcast address; all adaptors pass frames addressed to the broadcast address up to the host."
[^sa-eth-multicast]: Computer Networks: A Systems Approach, §2.6.2 Ethernet (Addresses) — <https://book.systemsapproach.org/direct/ethernet.html> — "an address that has the first bit set to 1 but is not the broadcast address is called a multicast address."
[^sa-eth-jumbo]: Computer Networks: A Systems Approach, §2.6.3 Longevity of Ethernet — <https://book.systemsapproach.org/direct/ethernet.html> — "it allows jumbo packets with up to 9000 bytes of data rather than 1500 bytes."
[^rfc1122-loop]: RFC 1122, §3.2.1.3 Addressing — <https://www.rfc-editor.org/rfc/rfc1122.html> — "Internal host loopback address. Addresses of this form MUST NOT appear outside a host."
[^ip7-loop]: ip(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/ip.7.en.html> — "INADDR_LOOPBACK (127.0.0.1) always refers to the local host via the loopback device;"
[^tcpdump-e]: tcpdump(8), Debian bookworm — <https://manpages.debian.org/bookworm/tcpdump/tcpdump.8.en.html> — "Print the link-level header on each dump line. This can be used, for example, to print MAC layer addresses for protocols such as Ethernet and IEEE 802.11."
[^sa-bridge-promisc]: Computer Networks: A Systems Approach, §3.2 Switched Ethernet — <https://book.systemsapproach.org/internetworking/ethernet.html> — "This device operates in promiscuous mode, accepting all frames transmitted on either of the Ethernets, and forwarding them to the other."
[^sa-bridge-simple]: Computer Networks: A Systems Approach, §3.2 Switched Ethernet — <https://book.systemsapproach.org/internetworking/ethernet.html> — "In their simplest variants, bridges simply accept LAN frames on their inputs and forward them out on all other outputs."
[^sa-learn-q]: Computer Networks: A Systems Approach, §3.2.1 Learning Bridges — <https://book.systemsapproach.org/internetworking/ethernet.html> — "The first optimization we can make to a bridge is to observe that it need not forward all frames that it receives."
[^sa-learn-src]: Computer Networks: A Systems Approach, §3.2.1 Learning Bridges — <https://book.systemsapproach.org/internetworking/ethernet.html> — "The idea is for each bridge to inspect the source address in all the frames it receives."
[^sa-learn-timeout]: Computer Networks: A Systems Approach, §3.2.1 Learning Bridges — <https://book.systemsapproach.org/internetworking/ethernet.html> — "a timeout is associated with each entry, and the bridge discards the entry after a specified period of time."
[^sa-learn-flood]: Computer Networks: A Systems Approach, §3.2.1 Learning Bridges — <https://book.systemsapproach.org/internetworking/ethernet.html> — "Should the bridge receive a frame that is addressed to a host not currently in the table, it goes ahead and forwards the frame out on all the other ports."
[^sa-learn-opt]: Computer Networks: A Systems Approach, §3.2.1 Learning Bridges — <https://book.systemsapproach.org/internetworking/ethernet.html> — "this table is simply an optimization that filters out some frames; it is not required for correctness."
[^bridge-learning]: bridge(8), iproute2 (Debian bookworm) — <https://manpages.debian.org/bookworm/iproute2/bridge.8.en.html> — "If learning if off, the bridge will end up flooding any traffic for which it has no FDB entry."
[^bridge-fdb]: bridge(8), iproute2 (Debian bookworm) — <https://manpages.debian.org/bookworm/iproute2/bridge.8.en.html> — "fdb objects contain known Ethernet addresses on a link."
[^sa-loop]: Computer Networks: A Systems Approach, §3.2.3 Spanning Tree Algorithm — <https://book.systemsapproach.org/internetworking/ethernet.html> — "it fails in a horrible way—frames potentially get forwarded forever."
[^sa-loop-ex]: Computer Networks: A Systems Approach, §3.2.3 Spanning Tree Algorithm — <https://book.systemsapproach.org/internetworking/ethernet.html> — "There is nothing to stop this cycle from repeating endlessly, with packets looping in both directions among S1, S4, and S6."
[^sa-stp]: Computer Networks: A Systems Approach, §3.2.3 Spanning Tree Algorithm — <https://book.systemsapproach.org/internetworking/ethernet.html> — "a spanning tree is a subgraph of this graph that covers (spans) all the vertices but contains no cycles."
[^sa-bcast-switch]: Computer Networks: A Systems Approach, §3.2.4 Broadcast and Multicast — <https://book.systemsapproach.org/internetworking/ethernet.html> — "each switch forwards a frame with a destination broadcast address out on each active (selected) port other than the one on which the frame was received."
[^sa-vlan-scale]: Computer Networks: A Systems Approach, §3.2.5 Virtual LANs (VLANs) — <https://book.systemsapproach.org/internetworking/ethernet.html> — "broadcast does not scale, and as a consequence L2-based networks do not scale."
[^sa-vlan-def]: Computer Networks: A Systems Approach, §3.2.5 Virtual LANs (VLANs) — <https://book.systemsapproach.org/internetworking/ethernet.html> — "VLANs allow a single extended LAN to be partitioned into several seemingly separate LANs."
[^sa-vlan-ex]: Computer Networks: A Systems Approach, §3.2.5 Virtual LANs (VLANs) — <https://book.systemsapproach.org/internetworking/ethernet.html> — "It inserts a VLAN header between the Ethernet header and its payload."
[^sa-vlan-nowires]: Computer Networks: A Systems Approach, §3.2.5 Virtual LANs (VLANs) — <https://book.systemsapproach.org/internetworking/ethernet.html> — "it is possible to change the logical topology without moving any wires or changing any addresses."
[^sa-vlan-tag]: Computer Networks: A Systems Approach, §3.2.5 Virtual LANs (VLANs) — <https://book.systemsapproach.org/internetworking/ethernet.html> — "inserting a 12-bit VLAN ID (VID) field between the SrcAddr and Type fields"
[^sa-vlan-8100]: Computer Networks: A Systems Approach, §3.2.5 Virtual LANs (VLANs) — <https://book.systemsapproach.org/internetworking/ethernet.html> — "Type = 0x8100 to indicate that this frame includes the VLAN extension"
[^sa-vlan-32]: Computer Networks: A Systems Approach, §3.2.5 Virtual LANs (VLANs) — <https://book.systemsapproach.org/internetworking/ethernet.html> — "There are actually 32-bits inserted in the middle of the header"
[^iplink-types]: ip-link(8), iproute2 (Debian bookworm) — <https://manpages.debian.org/bookworm/iproute2/ip-link.8.en.html> — "vlan - 802.1q tagged virtual LAN interface"
[^iplink-vlan-id]: ip-link(8), iproute2 (Debian bookworm) — <https://manpages.debian.org/bookworm/iproute2/ip-link.8.en.html> — "id VLANID - specifies the VLAN Identifier to use."
[^iplink-mtu]: ip-link(8), iproute2 (Debian bookworm) — <https://manpages.debian.org/bookworm/iproute2/ip-link.8.en.html> — "change the MTU of the device."
[^iplink-local]: ip-link(8), iproute2 (Debian bookworm) — <https://manpages.debian.org/bookworm/iproute2/ip-link.8.en.html> — "Some devices are not allowed to change network namespace: loopback, bridge, wireless. These are network namespace local devices."
[^rfc826-abs]: RFC 826, Abstract — <https://www.rfc-editor.org/rfc/rfc826.html> — "The addresses of hosts within protocol P are not always compatible with the corresponding Ethernet address"
[^arp7-ipv4]: arp(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/arp.7.en.html> — "It is used to convert between Layer2 hardware addresses and IPv4 protocol addresses on directly connected networks."
[^rfc826-defs]: RFC 826, Definitions — <https://www.rfc-editor.org/rfc/rfc826.html> — "ares_op$REQUEST (= 1, high byte transmitted first) and ares_op$REPLY (= 2),"
[^rfc826-format]: RFC 826, Packet format — <https://www.rfc-editor.org/rfc/rfc826.html> — "(ar$tha) Hardware address of target of this packet (if known)."
[^rfc826-gen-hrd]: RFC 826, Packet Reception (Generalization) — <https://www.rfc-editor.org/rfc/rfc826.html> — "For the 10Mbit Ethernet \<ar$hrd, ar$hln> takes on the value <1, 6>."
[^rfc826-etype]: RFC 826, Definitions — <https://www.rfc-editor.org/rfc/rfc826.html> — "and a new one: ether_type$ADDRESS_RESOLUTION."
[^rfc826-table]: RFC 826, Packet Generation — <https://www.rfc-editor.org/rfc/rfc826.html> — "The Address Resolution module tries to find this pair in a table."
[^rfc826-throw]: RFC 826, Packet Generation — <https://www.rfc-editor.org/rfc/rfc826.html> — "it probably informs the caller that it is throwing the packet away (on the assumption the packet will be retransmitted by a higher network layer)"
[^rfc826-bcast]: RFC 826, Packet Generation — <https://www.rfc-editor.org/rfc/rfc826.html> — "It then causes this packet to be broadcast to all stations on the Ethernet cable originally determined by the routing mechanism."
[^rfc826-reply]: RFC 826, Packet Reception — <https://www.rfc-editor.org/rfc/rfc826.html> — "Send the packet to the (new) target hardware address on the same hardware on which the request was received."
[^rfc826-merge-first]: RFC 826, Packet Reception — <https://www.rfc-editor.org/rfc/rfc826.html> — "Notice that the \<protocol type, sender protocol address, sender hardware address> triplet is merged into the table before the opcode is looked at. This is on the assumption that communcation is bidirectional; if A has some reason to talk to B, then B will probably have some reason to talk to A."
[^rfc826-supersede]: RFC 826, Packet Reception — <https://www.rfc-editor.org/rfc/rfc826.html> — "if an entry already exists for the \<protocol type, sender protocol address> pair, then the new hardware address supersedes the old one."
[^rfc826-noperiodic]: RFC 826, Why is it done this way?? — <https://www.rfc-editor.org/rfc/rfc826.html> — "Periodic broadcasting is definitely not desired."
[^arp7-user]: arp(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/arp.7.en.html> — "The user normally doesn't interact directly with this module except to configure it; instead it provides a service for other protocols in the kernel."
[^arp7-ioctl]: arp(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/arp.7.en.html> — "The ARP table can also be controlled via ioctl(2) on any AF_INET socket."
[^arp7-cache]: arp(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/arp.7.en.html> — "The cache has a limited size so old and less frequently used entries are garbage-collected."
[^arp7-gc3]: arp(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/arp.7.en.html> — "The garbage collector will always run if there are more than this number of entries in the cache. Defaults to 1024."
[^arp7-positive]: arp(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/arp.7.en.html> — "Positive feedback can be gotten from a higher layer; for example from a successful TCP ACK."
[^arp7-reprobe]: arp(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/arp.7.en.html> — "It first tries to ask a local arp daemon app_solicit times for an updated MAC address. If that fails and an old MAC address is known, a unicast probe is sent ucast_solicit times. If that fails too, it will broadcast a new ARP request to the network. Requests are sent only when there is data queued for sending."
[^arp7-mcast]: arp(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/arp.7.en.html> — "The maximum number of attempts to resolve an address by multicast/broadcast before marking the entry as unreachable. Defaults to 3."
[^arp7-retrans]: arp(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/arp.7.en.html> — "The number of milliseconds to delay before retransmitting a request. Defaults to 1000 milliseconds."
[^arp7-reach]: arp(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/arp.7.en.html> — "As for base_reachable_time, but measures time in milliseconds. Defaults to 30000 milliseconds."
[^arp7-reach-rand]: arp(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/arp.7.en.html> — "the entry is considered to be valid for at least a random value between base_reachable_time/2 and 3*base_reachable_time/2."
[^ipneigh-arp]: ip-neighbour(8), iproute2 (Debian bookworm) — <https://manpages.debian.org/bookworm/iproute2/ip-neighbour.8.en.html> — "The IPv4 neighbour table is also known by another name - the ARP table."
[^ipneigh-reachable]: ip-neighbour(8), iproute2 (Debian bookworm) — <https://manpages.debian.org/bookworm/iproute2/ip-neighbour.8.en.html> — "the neighbour entry is valid until the reachability timeout expires."
[^ipneigh-stale]: ip-neighbour(8), iproute2 (Debian bookworm) — <https://manpages.debian.org/bookworm/iproute2/ip-neighbour.8.en.html> — "the neighbour entry is valid but suspicious."
[^ipneigh-incomplete]: ip-neighbour(8), iproute2 (Debian bookworm) — <https://manpages.debian.org/bookworm/iproute2/ip-neighbour.8.en.html> — "the neighbour entry has not (yet) been validated/resolved."
[^ipneigh-failed]: ip-neighbour(8), iproute2 (Debian bookworm) — <https://manpages.debian.org/bookworm/iproute2/ip-neighbour.8.en.html> — "max number of probes exceeded without success, neighbor validation has ultimately failed."
[^ipneigh-addr]: ip-neighbour(8), iproute2 (Debian bookworm) — <https://manpages.debian.org/bookworm/iproute2/ip-neighbour.8.en.html> — "the protocol address of the neighbour. It is either an IPv4 or IPv6 address."
[^rfc4861-comb]: RFC 4861, §3.1 Comparison with IPv4 — <https://www.rfc-editor.org/rfc/rfc4861.html> — "The IPv6 Neighbor Discovery protocol corresponds to a combination of the IPv4 protocols Address Resolution Protocol [ARP], ICMP Router Discovery [RDISC], and ICMP Redirect [ICMPv4]."
[^rfc4861-ns]: RFC 4861, §3 Protocol Overview — <https://www.rfc-editor.org/rfc/rfc4861.html> — "Neighbor Solicitation messages are multicast to the solicited-node multicast address of the target address. The target returns its link-layer address in a unicast Neighbor Advertisement"
[^rfc4861-nud]: RFC 4861, §3.1 Comparison with IPv4 — <https://www.rfc-editor.org/rfc/rfc4861.html> — "Unlike ARP, Neighbor Discovery detects half-link failures (using Neighbor Unreachability Detection)"
[^sa-mtu]: Computer Networks: A Systems Approach, §3.3.2 Service Model (Fragmentation and Reassembly) — <https://book.systemsapproach.org/internetworking/basic-ip.html> — "every network type has a maximum transmission unit (MTU), which is the largest IP datagram that it can carry in a frame."
[^rfc9293-segsmall]: RFC 9293, §3.7 Segmentation — <https://www.rfc-editor.org/rfc/rfc9293.html> — "Avoiding sending a TCP segment that would result in an IP datagram larger than the smallest MTU along an IP network path because this results in either packet loss or packet fragmentation."
[^sa-wifi]: Computer Networks: A Systems Approach, §2.7.2 Wi-Fi (802.11) — <https://book.systemsapproach.org/direct/wireless.html> — "its primary challenge is to mediate access to a shared communication medium"
[^sa-csma-ca]: Computer Networks: A Systems Approach, §2.7.2 Wi-Fi (802.11) — <https://book.systemsapproach.org/direct/wireless.html> — "802.11 addresses these problems by using CSMA/CA, where the CA stands for collision avoidance, in contrast to the collision detection of CSMA/CD used on Ethernets."
[^sa-wifi-ack]: Computer Networks: A Systems Approach, §2.7.2 Wi-Fi (802.11) — <https://book.systemsapproach.org/direct/wireless.html> — "one part of CSMA/CA is an explicit ACK from the receiver to the sender."
[^sa-wifi-ap]: Computer Networks: A Systems Approach, §2.7.2 Wi-Fi (802.11) — <https://book.systemsapproach.org/direct/wireless.html> — "802.11 calls these base stations access points (APs)"
[^ip8-desc]: ip(8), iproute2 (Debian bookworm) — <https://manpages.debian.org/bookworm/iproute2/ip.8.en.html> — "ip - show / manipulate routing, network devices, interfaces and tunnels"
[^veth-pair]: veth(4), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/veth.4.en.html> — "veth devices are always created in interconnected pairs."
[^veth-imm]: veth(4), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/veth.4.en.html> — "Packets transmitted on one device in the pair are immediately received on the other device."
[^veth-desc]: veth(4), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/veth.4.en.html> — "They can act as tunnels between network namespaces to create a bridge to a physical network device in another namespace, but can also be used as standalone network devices."
[^std-socketaddr]: Rust core 1.95.0 source, library/core/src/net/socket_addr.rs (SocketAddr) — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/core/src/net/socket_addr.rs> — "Internet socket addresses consist of an IP address, a 16-bit port number, as well as possibly some version-dependent additional information."
[^std-loopback]: Rust std 1.95.0, Ipv4Addr::is_loopback — <https://doc.rust-lang.org/1.95.0/std/net/struct.Ipv4Addr.html> — "Returns true if this is a loopback address (127.0.0.0/8)."
[^packet7-l2]: packet(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/packet.7.en.html> — "Packet sockets are used to receive or send raw packets at the device driver (OSI Layer 2) level."
[^packet7-types]: packet(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/packet.7.en.html> — "The socket_type is either SOCK_RAW for raw packets including the link-level header or SOCK_DGRAM for cooked packets with the link-level header removed."
[^packet7-cap]: packet(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/packet.7.en.html> — "In order to create a packet socket, a process must have the CAP_NET_RAW capability in the user namespace that governs its network namespace."
[^sa-framing-challenge]: Computer Networks: A Systems Approach, §2.3 Framing — <https://book.systemsapproach.org/direct/framing.html> — "Recognizing exactly what set of bits constitutes a frame—that is, determining where the frame begins and ends—is the central challenge faced by the adaptor."
[^sa-framing-three]: Computer Networks: A Systems Approach, §2.3 Framing — <https://book.systemsapproach.org/direct/framing.html> — "This section uses three different protocols to illustrate the various points in the design space."
[^sa-hdlc-flag]: Computer Networks: A Systems Approach, §2.3.2 Bit-Oriented Protocols (HDLC) — <https://book.systemsapproach.org/direct/framing.html> — "HDLC denotes both the beginning and the end of a frame with the distinguished bit sequence 01111110."
[^sa-bitstuff]: Computer Networks: A Systems Approach, §2.3.2 Bit-Oriented Protocols (HDLC) — <https://book.systemsapproach.org/direct/framing.html> — "any time five consecutive 1s have been transmitted from the body of the message (i.e., excluding when the sender is trying to transmit the distinguished 01111110 sequence), the sender inserts a 0 before transmitting the next bit."
[^sa-clockbased]: Computer Networks: A Systems Approach, §2.3.3 Clock-Based Framing (SONET) — <https://book.systemsapproach.org/direct/framing.html> — "For lack of a widely accepted generic term, we refer to this approach simply as clock-based framing."
[^sa-err-ecc]: Computer Networks: A Systems Approach, §2.4 Error Detection — <https://book.systemsapproach.org/direct/error.html> — "Alternatively, some types of error detection algorithms allow the recipient to reconstruct the correct message even after it has been corrupted"
[^sa-err-twocopies]: Computer Networks: A Systems Approach, §2.4 Error Detection — <https://book.systemsapproach.org/direct/error.html> — "First, it sends \(n\) redundant bits for an \(n\)-bit message."
[^sa-crc-divisible]: Computer Networks: A Systems Approach, §2.4.2 Cyclic Redundancy Check — <https://book.systemsapproach.org/direct/error.html> — "contrive to make the polynomial representing \(P(x)\) exactly divisible by \(C(x)\)"
[^sa-checksum-16]: Computer Networks: A Systems Approach, §2.4.1 Internet Checksum Algorithm — <https://book.systemsapproach.org/direct/error.html> — "Consider the data being checksummed as a sequence of 16-bit integers."
[^sa-checksum-nolink]: Computer Networks: A Systems Approach, §2.4.1 Internet Checksum Algorithm — <https://book.systemsapproach.org/direct/error.html> — "Although it is not used at the link level, it nevertheless provides the same sort of functionality as CRCs, so we discuss it here."
[^sa-repeater]: Computer Networks: A Systems Approach, §3.2 Switched Ethernet — <https://book.systemsapproach.org/internetworking/ethernet.html> — "This node would differ from a repeater, which operates on bits, not frames, and just blindly copies the bits received on one interface to another."
[^sa-wifi-base]: Computer Networks: A Systems Approach, §2.7.2 Wi-Fi (802.11) — <https://book.systemsapproach.org/direct/wireless.html> — "nearly all 802.11 networks use a base-station-oriented topology."
[^rfc826-dyn]: RFC 826, Abstract — <https://www.rfc-editor.org/rfc/rfc826.html> — "Presented here is a protocol that allows dynamic distribution of the information needed to build tables to translate an address A in protocol P's address space into a 48.bit Ethernet address."
[^rfc826-add]: RFC 826, Packet Reception — <https://www.rfc-editor.org/rfc/rfc826.html> — "If Merge_flag is false, add the triplet \<protocol type, sender protocol address, sender hardware address> to the translation table."
[^rfc826-needed]: RFC 826, Why is it done this way?? — <https://www.rfc-editor.org/rfc/rfc826.html> — "The protocol described in this paper distributes information as it is needed, and only once (probably) per boot of a machine."
[^ipneigh-nud]: ip-neighbour(8), iproute2 (Debian bookworm) — <https://manpages.debian.org/bookworm/iproute2/ip-neighbour.8.en.html> — "nud is an abbreviation for 'Neighbour Unreachability Detection'."
[^ipneigh-states]: ip-neighbour(8), iproute2 (Debian bookworm) — <https://manpages.debian.org/bookworm/iproute2/ip-neighbour.8.en.html> — "STATE := { permanent | noarp | stale | reachable | none | incomplete | delay | probe | failed }"
[^iplink-bridge]: ip-link(8), iproute2 (Debian bookworm) — <https://manpages.debian.org/bookworm/iproute2/ip-link.8.en.html> — "bond - Bonding device bridge - Ethernet Bridge device"
