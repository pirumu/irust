# P5 · Chương 07 — RAID và copy-on-write file system

## Bạn sẽ hiểu được gì

1. **Q1.** RAID giải quyết ba vấn đề gì, vì sao nó giả dạng một "đĩa lớn", và mô hình lỗi **fail-stop** giả định/bỏ qua điều gì? → mục 1
2. **Q2.** RAID-0, RAID-1, RAID-4 bố trí block ra sao; parity XOR dựng lại block mất thế nào? → mục 2
3. **Q3.** Vì sao một ghi nhỏ vào RAID-4/5 tốn 4 I/O (**small-write problem**), RAID-5 xoay parity để làm gì, và các cấp so với nhau thế nào? → mục 3
4. **Q4.** **Consistent-update problem** / "write hole" là gì, và Linux md xử lý nó bằng cơ chế nào? → mục 4
5. **Q5.** Vì sao cần checksum: những lỗi nào nằm ngoài fail-stop (LSE, corruption, misdirected write, lost write)? → mục 5
6. **Q6.** Copy-on-write (COW) / shadow paging là gì; vì sao snapshot gần như miễn phí và vì sao snapshot **không phải** backup? → mục 6
7. **Q7.** Btrfs và ZFS dùng ý tưởng đó cho checksum và RAID thế nào, và cái giá của COW là gì (garbage, fragmentation, ghi nhỏ đắt)? → mục 7

## Kiến thức cần trước

- [Chương 01 — Thiết bị lưu trữ](./01-thiet-bi-luu-tru.md)
- [Chương 03 — Cài đặt file system](./03-cai-dat-file-system.md)
- [Chương 05 — Crash consistency và journaling](./05-crash-consistency-va-journaling.md): write-ahead log; chương này dùng lại cùng ý tưởng ở tầng RAID.
- [Chương 04 — File descriptor và VFS](../01-linux/04-file-descriptor-va-vfs.md)

Bạn đã chạy database trên server có đĩa, nhưng chưa cần biết bên dưới file system, controller hay phần mềm RAID làm gì khi một đĩa chết giữa lúc đang ghi. Chương này lấp chỗ đó.

## 1. RAID: nhiều đĩa trông như một đĩa (Q1)

> **Trả lời ngắn.** RAID ghép nhiều đĩa thành một hệ đĩa nhanh hơn, lớn hơn và đáng tin hơn[^raid-def]. Bên ngoài nó vẫn trông như một đĩa, nên database và OS phía trên không phải đổi gì[^raid-transparent]. Để so sánh các thiết kế, người ta tạm giả định mỗi đĩa chỉ ở một trong hai trạng thái, chạy hoặc hỏng hẳn[^failstop]. Giả định đó chỉ là bước đầu, vì các lỗi thực tế phức tạp hơn nhiều[^failstop-later].

**Giải thích.**

*Với database của bạn.* Hãy hình dung `orders-db`, một database lưu đơn hàng, chạy trên một server có bốn đĩa. Đĩa nào cũng có thể chết, mà đơn hàng thì không được mất; ta cũng muốn đọc ghi nhanh hơn mức một đĩa đơn lẻ cho phép. Ta sẽ bám theo `orders-db` suốt chương (nhận xét của người soạn: đây là ví dụ giả định, không phải code trong repo). Câu hỏi của mục này: ghép bốn đĩa lại bằng cách nào mà `orders-db` vẫn chỉ thấy "một đĩa"?

Một đĩa đơn lẻ bị giới hạn về cả tốc độ, dung lượng lẫn độ tin cậy. RAID (Redundant Array of Inexpensive Disks) cho nhiều đĩa phối hợp với nhau để dựng nên một hệ đĩa nhanh hơn, lớn hơn và đáng tin hơn[^raid-def]. Ba lợi ích đó đến theo đúng thứ tự: hiệu năng nhờ nhiều đĩa chạy song song, dung lượng, và độ tin cậy, vì dữ liệu trải trên nhiều đĩa mà không có kỹ thuật RAID thì dễ mất khi một đĩa hỏng[^raid-adv].

Điều khiến RAID dễ đưa vào hệ thống có sẵn là nó trong suốt. Từ bên ngoài, mảng RAID chỉ là một nhóm block đọc hoặc ghi được, y như một đĩa[^raid-looks]. Khi file system gửi một I/O logic, RAID tự tính xem phải đụng đĩa nào rồi phát một hay nhiều I/O vật lý. Chẳng hạn mảng giữ hai bản sao của mỗi block (mirror) thì mỗi lần ghi logic biến thành hai lần ghi vật lý[^raid-logical]. Nhờ vậy, thay một đĩa SCSI bằng một mảng RAID thì phần còn lại của hệ thống (máy chủ, OS…) không phải sửa gì để dùng được nó[^raid-transparent]. `orders-db` cũng vậy: nó mở một file như mọi khi, và không biết bên dưới có bao nhiêu đĩa.

### 1.1 Mô hình lỗi fail-stop

Muốn so sánh các thiết kế RAID, ta cần một mô hình lỗi đủ đơn giản. Mô hình đó là **fail-stop**: mỗi đĩa ở đúng một trong hai trạng thái, đang chạy, khi đó đọc ghi được mọi block, hoặc đã hỏng, khi đó coi như mất vĩnh viễn[^failstop]. Giả định thứ hai là đĩa hỏng thì phát hiện được dễ dàng[^failstop-detect]. Với `orders-db`, nghĩa là một đĩa chết thì hệ thống biết ngay và chỉ việc thay đĩa.

Nhưng mô hình này chỉ là bước đầu. Nó chưa xét các lỗi "im lặng", như corruption (đĩa trả dữ liệu sai mà không báo) hay latent sector error (một block đơn lẻ không đọc được trên một đĩa vẫn chạy bình thường). Những lỗi đó phức tạp hơn nhưng tiếc là thực tế hơn[^failstop-later]. Mục 5 quay lại chúng.

<details><summary>Đi sâu: ba trục đánh giá một thiết kế RAID</summary>

Mỗi thiết kế RAID được đánh giá theo ba trục: **capacity** (dung lượng hữu dụng), **reliability** (chịu được mấy lần hỏng đĩa) và **performance**. Các cấp dùng parity có dung lượng nằm giữa mức không dư thừa và mức mirror[^axes].

</details>

**Quay lại Q1:** RAID ghép bốn đĩa của `orders-db` thành một đĩa trông như bình thường; fail-stop là mô hình lỗi đơn giản hóa, chưa phản ánh các lỗi hỏng im lặng.

## 2. RAID-0, RAID-1 và parity của RAID-4 (Q2)

> **Trả lời ngắn.** RAID-0 rải block luân phiên qua các đĩa và không có dư thừa[^r0]. RAID-1 giữ nhiều bản sao của mỗi block trên các đĩa khác nhau, nên mỗi lần ghi phải cập nhật mọi bản[^r1-rw]. RAID-4 thêm cho mỗi stripe một block parity (block chứa thông tin dư thừa) tính bằng XOR, đủ để chịu mất một block bất kỳ trong stripe[^par-xor].

**Giải thích.**

*Với `orders-db`.* Có bốn đĩa thì ta có ba hướng. Hướng một là chia dữ liệu ra cả bốn đĩa cho nhanh, nhưng chết một đĩa là mất dữ liệu (RAID-0). Hướng hai là lưu mỗi thứ hai bản, an toàn hơn nhưng hết một nửa dung lượng (RAID-1). Hướng ba là dành một phần dung lượng cho thông tin dư thừa được tính bằng phép toán, rẻ hơn mirror nhưng phải trả giá ở chỗ khác (RAID-4). Mục này cho thấy mỗi hướng bố trí block ra sao.

### 2.1 RAID-0: striping

RAID-0 thật ra không phải một cấp RAID đúng nghĩa vì không có dư thừa gì; nó đóng vai cận trên về hiệu năng và dung lượng[^r0]. Các block logic của `orders-db` được rải luân phiên qua các đĩa. Với 4 đĩa và chunk một block:

| | Disk 0 | Disk 1 | Disk 2 | Disk 3 |
|---|---|---|---|---|
| stripe 0 | 0 | 1 | 2 | 3 |
| stripe 1 | 4 | 5 | 6 | 7 |
| stripe 2 | 8 | 9 | 10 | 11 |

Một **stripe** là một hàng các **chunk**, và chunk có thể lớn hơn một block. Ví dụ chunk 8 KB trên 4 đĩa thì một stripe dài 32 KB[^chunk-size-def].

Về độ tin cậy, striping "hoàn hảo nhưng theo nghĩa xấu": bất kỳ đĩa nào hỏng cũng dẫn tới mất dữ liệu, tức chịu mất 0 đĩa[^r0-rel]. Với `orders-db`, bốn đĩa chia nhau dữ liệu thì chết một đĩa là mất đơn hàng.

### 2.2 RAID-1: mirroring

RAID-1 giữ nhiều bản sao của mỗi block trên các đĩa khác nhau. Khi đọc, RAID tự do chọn một bản. Khi ghi thì không có lựa chọn: RAID phải cập nhật mọi bản để giữ độ tin cậy, và các lần ghi đó có thể chạy song song[^r1-rw]. Cái giá là dung lượng hữu dụng chỉ còn một nửa (với mức mirror 2)[^r1-cap].

Về độ tin cậy, mảng mirror mức 2 chắc chắn chịu được 1 đĩa hỏng, và tối đa N/2 đĩa tùy đĩa nào hỏng. Nhưng thực tế người ta không muốn phó mặc may rủi, nên coi mirror là để chịu **một** lần hỏng[^r1-rel].

Có hai biến thể ghép lớp: **RAID-10** (stripe trên các cặp mirror) và **RAID-01** (mirror của hai mảng stripe)[^r10].

### 2.3 RAID-4: parity

Mirror tốn một nửa dung lượng. **Parity** tốn ít chỗ hơn, nhưng trả giá bằng hiệu năng[^par-intro]. Cách làm: với mỗi stripe, một block parity chứa thông tin dư thừa tính từ các block dữ liệu của stripe đó. Ví dụ P1 tính từ block 4, 5, 6, 7[^par-p1]. Hàm tính là XOR, một hàm đơn giản mà làm việc này rất khéo, cho phép chịu mất bất kỳ một block nào trong stripe[^par-xor]. Muốn dựng lại block mất, ta XOR các block còn lại với parity (suy ra từ tính chất XOR, đã kiểm bằng code ở mục 9; nhận xét của người soạn).

Trong RAID-4, parity nằm trọn trên **một** đĩa riêng, nên dung lượng hữu dụng là (N − 1)·B, với B là số block mỗi đĩa[^r4-parity-disk]. Bốn đĩa của `orders-db` cho ba đĩa chứa dữ liệu và một đĩa chứa parity (tính từ công thức trên). Theo bảng tổng kết của OSTEP, dung lượng và độ chịu lỗi của RAID-4 và RAID-5 như nhau: cả hai chịu 1 lần hỏng đĩa[^table].

<details><summary>Đi sâu: chunk size và ví dụ XOR bằng số cụ thể</summary>

**Chunk size.** Chunk nhỏ thì nhiều file bị rải qua nhiều đĩa; chunk lớn thì giảm song song trong nội bộ một file, nên phải dựa vào nhiều request đồng thời mới đạt throughput cao, đổi lại giảm thời gian định vị. Chọn cỡ "tốt nhất" khó vì phụ thuộc workload[^chunk-perf].

**XOR bằng số.** Mô phỏng của người soạn, đã chạy bằng `rustc` (mục 9). Lấy ba block dữ liệu 4 byte `d0=11 22 33 44`, `d1=aa bb cc dd`, `d2=01 02 03 04`. Parity là `P = d0⊕d1⊕d2 = ba 9b fc 9d`. Mất `d1` thì dựng lại bằng `d0⊕d2⊕P = aa bb cc dd`, đúng bằng `d1`.

</details>

**Quay lại Q2:** RAID-0 không dư thừa, RAID-1 tốn một nửa dung lượng, RAID-4 dùng một đĩa parity XOR để chịu mất một đĩa.

## 3. Ghi nhỏ, RAID-5 xoay parity và bảng so sánh (Q3)

> **Trả lời ngắn.** Ghi đè một block làm parity cũ sai. Cách subtractive đọc dữ liệu cũ và parity cũ rồi ghi dữ liệu mới và parity mới, tức 4 I/O cho một lần ghi[^4io]. Ở RAID-4, mọi ghi nhỏ đều phải đi qua một đĩa parity nên bị xếp hàng lần lượt[^bottleneck]. RAID-5 xoay parity qua các đĩa để các ghi nhỏ chạy song song được[^r5-rotate]. Dù vậy mỗi ghi vẫn tốn 4 I/O, tức vẫn mất hệ số 4[^r5-four].

**Giải thích.**

*Với `orders-db`.* Giả sử mỗi lần `orders-db` đổi trạng thái một đơn hàng, nó chỉ ghi một block nhỏ. Trên RAID parity, một ghi nhỏ như vậy không dừng ở một block, vì parity của stripe chứa block đó cũng phải đổi theo. Mục này đếm xem một ghi nhỏ thật ra tốn bao nhiêu I/O, và vì sao để parity trên một đĩa riêng thì cả hệ thống phải xếp hàng chờ đĩa ấy.

### 3.1 Small-write problem

Ghi đè một block của stripe làm parity cũ sai, nên phải cập nhật parity. Có hai cách:

1. **Additive parity**: đọc *toàn bộ* các block dữ liệu khác trong stripe, XOR với block mới để ra parity mới, rồi ghi dữ liệu mới và parity mới. Số lần đọc tăng theo số đĩa nên cách này không hợp với mảng lớn[^additive].
2. **Subtractive parity**: chỉ đọc dữ liệu cũ và parity cũ, rồi so với dữ liệu mới: `P_new = (C_old ⊕ C_new) ⊕ P_old`. Bit dữ liệu không đổi thì parity bit không đổi; bit dữ liệu đổi thì parity bit đảo[^subtractive].

Ví dụ bằng số (đã chạy bằng `rustc`, mục 9): `d1_old=aa bb cc dd`, `d1_new=5a 5b 5c 5d`, `P_old=ba 9b fc 9d` cho parity mới `4a 7b 6c 1d` theo subtractive (`(d1_old ⊕ d1_new) ⊕ P_old`). Tính lại từ đầu bằng `d0 ⊕ d1_new ⊕ d2` cũng ra đúng giá trị đó.

Với cách subtractive, mỗi lần ghi RAID phải làm 4 I/O vật lý: hai đọc, hai ghi[^4io]. Chuyện lộ ra khi có nhiều ghi nhỏ đồng thời. Dữ liệu nằm ở các đĩa khác nhau thì ghi song song được, nhưng lần nào cũng phải cập nhật parity trên đĩa parity duy nhất, nên mọi lần ghi của hệ thống bị xếp hàng qua đĩa đó[^bottleneck]. Đó là small-write problem. Kết luận của OSTEP: throughput của RAID-4 khi ghi nhỏ ngẫu nhiên rất tệ và không khá hơn khi thêm đĩa[^r4-terrible].

### 3.2 RAID-5: parity xoay vòng

RAID-5 làm gần như y hệt RAID-4, chỉ khác là xoay block parity qua các đĩa, để bỏ nút cổ chai ở đĩa parity[^r5-rotate]. Bảng dưới là bốn stripe đầu của bố trí 5 đĩa trong hình 38.7 của OSTEP (bảng có block 0–15 và parity P0–P3; hình gốc còn stripe thứ năm với block 16–19 và P4)[^r5-layout].

| | Disk 0 | Disk 1 | Disk 2 | Disk 3 | Disk 4 |
|---|---|---|---|---|---|
| stripe 0 | 0 | 1 | 2 | 3 | **P0** |
| stripe 1 | 5 | 6 | 7 | **P1** | 4 |
| stripe 2 | 10 | 11 | **P2** | 8 | 9 |
| stripe 3 | 15 | **P3** | 12 | 13 | 14 |

Theo ví dụ của OSTEP, ghi block 1 chạm đĩa 1 và đĩa 4, còn ghi block 10 chạm đĩa 0 và đĩa 2[^r5-example]. Hai lần ghi dùng hai cặp đĩa không giao nhau nên chạy song song được. Ngoài ra, dung lượng, độ chịu lỗi, throughput tuần tự và latency của một request giống RAID-4. Đọc ngẫu nhiên tốt hơn chút vì dùng được mọi đĩa, và ghi ngẫu nhiên cải thiện rõ[^r5-analysis].

Nhưng mỗi ghi nhỏ vẫn sinh 4 I/O. Hệ số 4 bị mất là vì mỗi lần ghi RAID-5 vẫn tạo ra tổng cộng 4 thao tác I/O, và đó đơn giản là cái giá của RAID dựa trên parity[^r5-four].

Vì vậy RAID-5 gần như đã thay hẳn RAID-4 trên thị trường. RAID-4 chỉ còn ở những hệ thống biết chắc chỉ có ghi lớn, vì khi đó không dính small-write problem và RAID-4 hơi đơn giản hơn[^r5-replace].

### 3.3 Bảng so sánh (OSTEP hình 38.8)

Trong bảng dưới, N là số đĩa, B là số block mỗi đĩa, S là băng thông tuần tự của một đĩa, R là băng thông ngẫu nhiên của một đĩa, và T là latency của một request trên một đĩa[^table-legend].

| | RAID-0 | RAID-1 | RAID-4 | RAID-5 |
|---|---|---|---|---|
| Dung lượng | N·B | (N·B)/2 | (N−1)·B | (N−1)·B |
| Chịu mất | 0 | 1 chắc chắn; tối đa N/2 nếu may | 1 | 1 |
| Tuần tự đọc | N·S | (N/2)·S | (N−1)·S | (N−1)·S |
| Tuần tự ghi | N·S | (N/2)·S | (N−1)·S | (N−1)·S |
| Ngẫu nhiên đọc | N·R | N·R | (N−1)·R | N·R |
| Ngẫu nhiên ghi | N·R | (N/2)·R | R/2 | (N/4)·R |
| Latency đọc / ghi | T / T | T / T | T / 2T | T / 2T |

Cách chọn theo OSTEP: chỉ cần hiệu năng thì dùng striping; cần random I/O và độ tin cậy thì mirroring, trả bằng dung lượng; cần dung lượng và độ tin cậy thì RAID-5, trả bằng hiệu năng ghi nhỏ; còn nếu luôn là I/O tuần tự và muốn tối đa dung lượng thì RAID-5 cũng hợp[^choose]. Với `orders-db`, ghi ngẫu nhiên nhỏ là chỗ RAID-5 thua mirror trong bảng: (N/4)·R so với (N/2)·R (suy luận của người soạn từ bảng). Còn chọn cấp RAID và các tham số như chunk size hay số đĩa thì vẫn thiên về nghệ thuật hơn là khoa học[^art].

<details><summary>Đi sâu: đối chiếu từng ô của bảng với OSTEP</summary>

Bảng là bản chuyển thể của người soạn từ hình 38.8[^table]. PDF trích ra không giữ lưới, nên từng ô được đối chiếu với phần phân tích từng cấp trong cùng chương; không ô nào lệch.

- **RAID-0**
  - Dung lượng N·B[^axes].
  - Chịu mất 0 đĩa[^r0-rel].
  - Latency bằng một đĩa[^r0-lat].
  - Băng thông tuần tự N·S, ngẫu nhiên N·R[^r0-tp].
- **RAID-1**
  - Dung lượng (N·B)/2[^r1-cap].
  - Chịu 1 chắc chắn, tối đa N/2[^r1-rel].
  - Đọc tuần tự (N/2)·S[^r1-seqread].
  - Đọc ngẫu nhiên N·R, ghi ngẫu nhiên (N/2)·R[^r1-rand].
  - Ghi tuần tự (N/2)·S, theo "2× performance penalty"[^table-footnote].
  - Latency ghi T là giá trị đơn giản hóa; văn bản nói ghi mirror chậm hơn một chút[^r1-wlat].
- **RAID-4**
  - Chịu 1 đĩa hỏng, dung lượng (N−1)·B[^r4-cap].
  - Đọc tuần tự (N−1)·S, đọc ngẫu nhiên (N−1)·R[^r4-seq].
  - Ghi ngẫu nhiên R/2[^r4-terrible].
  - Latency ghi 2T[^r4-lat].
- **RAID-5**
  - Như RAID-4, trừ đọc ngẫu nhiên N·R[^r5-analysis].
  - Ghi ngẫu nhiên là (N/4)·R[^r5-four].

Chú thích 1 của OSTEP: hệ số 1/2 của RAID-1 tuần tự là với kiểu đọc/ghi ngây thơ, và phát I/O lớn đến các phần khác nhau của mỗi mirror có thể đạt băng thông đầy đủ[^table-note1]. OSTEP cũng nói mô hình đã bỏ qua nhiều chi tiết nhưng vẫn nắm được các khác biệt cốt yếu[^table-caveat].

</details>

**Quay lại Q3:** ghi nhỏ trên parity luôn tốn 4 I/O; RAID-5 chỉ bỏ cổ chai đĩa parity chứ không bỏ chi phí đó.

## 4. Write hole: consistent-update problem và Linux md (Q4)

> **Trả lời ngắn.** Một thao tác logic của RAID phải ghi nhiều đĩa, mà các lần ghi đó không nguyên tử: mất điện giữa chừng thì các bản lệch nhau[^cu-scenario]. Với RAID-5, OpenZFS gọi tình trạng dữ liệu và parity lệch nhau là "write hole"[^raidz-hole]. Lời giải chung là ghi log trước khi làm, tức write-ahead log[^cu-wal]. Linux md có hai chính sách `journal` và `ppl` cho việc này[^md-policy]. Ngoài ra md thường từ chối khởi động mảng raid5 vừa dirty vừa degraded nếu quản trị viên không ra lệnh rõ ràng[^md-dirty].

**Giải thích.**

*Với `orders-db`.* Giả sử máy mất điện đúng lúc `orders-db` ghi một đơn hàng: block dữ liệu đã xuống đĩa 2, nhưng block parity trên đĩa 4 thì chưa. Khi máy lên lại, không có gì cho biết stripe đó còn nhất quán hay không (nhận xét của người soạn). Mục này giải thích vì sao chuyện đó xảy ra, và RAID phần mềm của Linux đối phó ra sao.

### 4.1 Consistent-update problem

Bất kỳ RAID nào phải cập nhật nhiều đĩa trong một thao tác logic đều gặp vấn đề này. Kịch bản của OSTEP chạy trên mảng mirror: RAID ghi xong đĩa 0 thì mất điện, trước khi kịp phát lệnh ghi cho đĩa 1. Kết quả là hai bản sao không nhất quán: đĩa 0 giữ phiên bản mới, đĩa 1 giữ phiên bản cũ. Điều ta cần là hai đĩa đổi nguyên tử, cả hai thành mới hoặc không đĩa nào[^cu-scenario].

Lời giải chung là **write-ahead log**: ghi lại điều sắp làm *trước* khi làm. Sau crash, thủ tục recovery phát lại các giao dịch còn dở, nên không có hai bản sao nào lệch nhau[^cu-wal]. Đây cùng ý tưởng với journaling ở [Chương 05](./05-crash-consistency-va-journaling.md), đặt ở tầng RAID. Ghi log xuống đĩa mỗi lần thì quá đắt, nên phần cứng RAID thường có một ít NVRAM (ví dụ có pin) để ghi log[^cu-nvram]. RAID phần mềm thì rẻ hơn nhưng cũng chịu consistent-update problem[^cu-sw].

### 4.2 Linux md ở mức tổng quan

Linux có RAID phần mềm tên **md**. Điều kernel docs nói rõ và liên quan thẳng đến 4.1 là chuyện mảng raid5 hoặc raid6 vừa **dirty** vừa **degraded**. Dirty ở đây là trạng thái md đặt cho mảng: md đánh dấu mảng dirty trước khi một lệnh ghi bắt đầu, rồi đánh dấu clean lại sau một khoảng không có lệnh ghi[^md-safe]. Degraded là mảng đã mất một số đĩa.

Nếu mảng raid5 hoặc raid6 vừa dirty vừa degraded thì dữ liệu có thể bị hỏng mà không ai phát hiện được. Tài liệu giải thích: dirty nghĩa là parity không còn tin được, còn degraded nghĩa là một số block dữ liệu đang thiếu và không dựng lại đáng tin được vì không còn parity đúng. Vì vậy md thường từ chối khởi động mảng như thế, và buộc quản trị viên ra lệnh rõ ràng (tài liệu nêu `mdadm --assemble --force`)[^md-dirty].

Đó chính là hậu quả của consistent-update problem trên RAID-5: data và parity là hai lần ghi vật lý trên hai đĩa, và crash giữa chừng để lại một stripe không nhất quán (suy luận của người soạn từ mục 4.1 và đoạn trên).

<details><summary>Đi sâu: các chính sách <code>consistency_policy</code> của md</summary>

Các phát biểu về md trong chương này dựa vào `md.rst` ở tag kernel v6.8; trang docs.kernel.org không ghim phiên bản. Giá trị `level` của một mảng, tài liệu ví dụ là "raid0, raid1, raid5, linear, multipath, faulty"[^md-level].

Để giảm chi phí hoặc đóng write hole, md có các chế độ chọn qua thuộc tính sysfs `consistency_policy`[^md-policy]:

| Chính sách | Ý nghĩa theo tài liệu |
|---|---|
| `none` | Mảng không có thông tin dư thừa, ví dụ raid0 và linear.[^md-policy] |
| `resync` | Sau tắt không sạch, resync toàn mảng và tạo lại toàn bộ dư thừa.[^md-policy] |
| `bitmap` | Resync có hỗ trợ **write-intent bitmap**.[^md-policy] |
| `journal` | Chỉ cho raid4/5/6: dùng một thiết bị journal ghi lại giao dịch và phát lại sau tắt không sạch.[^md-policy] |
| `ppl` | Chỉ raid5: Partial Parity Log, nhằm đóng write hole và loại bỏ resync.[^md-policy] |

`journal` chính là ý tưởng write-ahead log ở mục 4.1, đặt trên một thiết bị journal riêng (nối nó với NVRAM của RAID phần cứng là nhận xét của người soạn).

</details>

**Quay lại Q4:** write hole nảy sinh vì cập nhật nhiều đĩa không nguyên tử; ghi log trước (NVRAM, journal, PPL) là cách chữa chung.

## 5. Vì sao cần checksum: các lỗi ngoài fail-stop (Q5)

> **Trả lời ngắn.** Ngoài đĩa hỏng hẳn còn hai loại lỗi nữa: latent sector error (một block không đọc được, ổ báo lỗi) và corruption im lặng (ổ trả dữ liệu sai mà không báo)[^lse-vs-corrupt]. Cách chính để phát hiện corruption là checksum, một bản tóm tắt nhỏ lưu cùng dữ liệu rồi so lại khi đọc[^checksum-def]. Với misdirected write, checksum chưa đủ mà phải thêm thông tin định danh vị trí[^misdirected-id]. Còn lost write thì cả checksum lẫn physical ID đều không bắt được[^lost-cksum].

**Giải thích.**

*Với `orders-db`.* Đến giờ ta giả định đĩa hỏng thì `orders-db` biết ngay. Giờ thử tình huống khác: đĩa im lặng trả về một block khác với block đã ghi. `orders-db` đọc ra một đơn hàng sai mà không có lỗi nào báo (nhận xét của người soạn). Mục này là về loại lỗi đó, và về việc checksum bắt được đến đâu.

Mô hình fail-stop coi lỗi luôn rõ ràng. Chương về data integrity của OSTEP thêm hai loại lỗi nữa. LSE dễ phát hiện vì lần đọc báo lỗi. **Corruption** thì khó hơn: đĩa trả dữ liệu sai mà không báo gì, nên gọi là lỗi "silent"[^lse-vs-corrupt].

Cách chính để phát hiện corruption là **checksum**. Đó là một hàm chạy trên một khối dữ liệu (ví dụ 4 KB) và cho ra một bản tóm tắt nhỏ (4–8 byte). Bản tóm tắt này được lưu cùng dữ liệu, rồi so lại mỗi lần đọc[^checksum-def].

Nhưng có hai kiểu lỗi mà checksum cơ bản không đủ để bắt:

- **Misdirected write**: controller ghi đúng dữ liệu nhưng sai vị trí, ví dụ block Dx bị ghi lên địa chỉ y, hoặc sang một đĩa khác[^misdirected]. Cách chữa của OSTEP là lưu kèm checksum một physical ID (số đĩa và số sector của block), rồi đối chiếu khi đọc[^misdirected-id].
- **Lost write**: thiết bị báo đã ghi xong nhưng thực chất chưa persist, nên nội dung vẫn là bản cũ[^lost-write]. Bản cũ này vẫn có checksum khớp và physical ID đúng, nên hai cơ chế trên không bắt được[^lost-cksum]. Một hướng chữa là đặt thêm checksum ở nơi khác; ZFS đặt checksum trong inode và indirect block[^lost-zfs].

<details><summary>Đi sâu: parity gặp LSE khi rebuild, RAID-DP và scrubbing</summary>

Parity có một điểm yếu khi gặp **latent sector error (LSE)**. Khi cả một đĩa hỏng, RAID dựng lại nó (ví dụ lên hot spare) bằng cách đọc mọi đĩa còn lại. Nếu trong lúc đó gặp một LSE trên bất kỳ đĩa nào thì việc dựng lại không thể hoàn tất[^lse-rebuild].

Cách đối phó là thêm một tầng dư thừa, như RAID-DP của NetApp có số đĩa parity tương đương hai đĩa thay vì một[^raiddp]. Ngoài ra, các nghiên cứu mà OSTEP tóm tắt cho thấy scrub đĩa hữu ích, vì phần lớn LSE được tìm ra theo cách này[^scrub]. **Scrubbing** là đọc định kỳ mọi block và kiểm tra checksum còn hợp lệ, thường theo lịch hằng đêm hoặc hằng tuần[^scrub-def].

</details>

**Quay lại Q5:** checksum bắt corruption im lặng; misdirected write cần thêm physical ID, lost write cần checksum ở nơi khác; LSE khi rebuild là điểm yếu của parity.

## 6. Copy-on-write: không bao giờ ghi đè (Q6)

> **Trả lời ngắn.** COW (trong database gọi là shadow paging) không ghi đè dữ liệu đang dùng: nó ghi bản mới sang chỗ trống rồi chuyển sang trỏ vào bản mới[^lfs-shadow]. Vì cây cũ còn nguyên nên snapshot chỉ là một tham chiếu ngăn các block cũ bị giải phóng, không copy gì[^zfs-snap]. Nhưng snapshot dùng chung block với bản gốc nên không thay được backup[^btrfs-notbackup].

**Giải thích.**

*Với `orders-db`.* Giả sử thư mục dữ liệu của `orders-db` nằm trên một file system COW như ZFS hoặc Btrfs. Mỗi lần database sửa một block, file system không đè lên block cũ mà ghi sang chỗ khác. Hệ quả là ba chuyện: mất điện giữa chừng không để lại cây ghi dở, chụp snapshot gần như không tốn gì, nhưng snapshot cũng không cứu được `orders-db` khi đĩa hỏng. Mục này đi từng chuyện một.

### 6.1 Ý tưởng: không ghi đè, ghi sang chỗ mới

Ở [Chương 05](./05-crash-consistency-va-journaling.md), journaling bảo vệ việc cập nhật tại chỗ bằng log. Hướng khác là **không bao giờ ghi đè**. LFS (log-structured file system) của OSTEP không ghi đè dữ liệu hiện có mà luôn ghi các segment vào chỗ trống[^lfs-never]. Nhưng điều đó kéo theo một rắc rối. Vì không ghi đè tại chỗ, phiên bản mới nhất của một inode, thứ ta cần tìm, cứ liên tục dịch chuyển[^lfs-move]. LFS giải quyết bằng một lớp gián tiếp là **inode map**, cộng với một vị trí cố định là **checkpoint region** trỏ tới các mảnh inode map mới nhất[^lfs-cr].

Kỹ thuật này có hai tên: trong database là shadow paging, còn trong cách nói của file system đôi khi là copy-on-write. Nó cho phép ghi rất hiệu quả, vì gom nhiều cập nhật vào một segment trong bộ nhớ rồi ghi tuần tự[^lfs-shadow].

Ghi lớn tốt cho đĩa quay (ít thời gian định vị), và trên RAID-4/5 thì tránh hẳn small-write problem[^lfs-raid]. Đây là chỗ nối với mục 3.1: LFS gom cập nhật vào segment lớn nên không dính small-write problem. Đó là mô tả về LFS; với ZFS, mục 7.3 cho thấy ghi một phần record lại đắt. WAFL của NetApp, ZFS của Sun và Btrfs của Linux đều dùng cách tiếp cận copy-on-write tương tự[^lfs-legacy].

### 6.2 Cơ chế COW trên cây: ví dụ OpenZFS

OpenZFS áp dụng quy tắc này triệt để: block đang dùng không bao giờ bị ghi đè, và đổi dữ liệu nghĩa là ghi nó ở chỗ khác rồi chuyển sang phiên bản mới[^zfs-rule]. Cả pool là một cây lớn. Lá chứa dữ liệu, nút trong chứa block pointer ghi vị trí, kích thước và checksum của nút con, còn gốc là **uberblock**[^zfs-tree].

Sửa một block ở giữa cây đi qua bốn bước: ghi dữ liệu mới vào chỗ trống; ghi phiên bản mới của nút cha trỏ vào block mới; lặp lên trên, vì checksum của cha giờ đã đổi; cuối cùng ghi uberblock mới[^zfs-write].

```mermaid
flowchart TD
    subgraph cu["Cây cũ (còn nguyên)"]
        U0["uberblock cũ"] --> R0["root cũ"]
        R0 --> I1["ind 1"]
        R0 --> I2["ind 2 cũ"]
        I1 --> D1["D1"]
        I1 --> D2["D2"]
        I2 --> D3["D3"]
        I2 --> D4["D4 cũ"]
    end
    subgraph moi["Cây mới (đường từ lá tới gốc)"]
        U1["uberblock mới"] --> R1["root mới"]
        R1 --> I1
        R1 --> I2n["ind 2 mới"]
        I2n --> D3
        I2n --> D4n["D4 mới"]
    end
```

(Sơ đồ của người soạn, vẽ lại ví dụ D4 của tài liệu OpenZFS ở dưới.)

Trong ví dụ đó chỉ D4 đổi, nên chỉ D4, cha của nó (ind 2), root và uberblock có bản sao mới. D1, D2, D3 và ind 1 được cây mới tham chiếu chứ không nhân đôi[^zfs-path]. Đến bước cuối, uberblock cũ vẫn mô tả một pool trọn vẹn, nên việc chuyển sang cây mới là một lần ghi nguyên tử. Từ đó tài liệu suy ra là không có "RAID-5 write hole" và không cần journal replay cho metadata[^zfs-atomic].

Cũng nên để ý chỗ checksum nằm. Checksum của một block được lưu ở **cha**, không cạnh block, nên một block hỏng không thể tự đứng ra bảo chứng cho chính nó[^zfs-tree]. Đối chiếu mục 5: ZFS đặt checksum trong inode và indirect block để bắt lost write[^lost-zfs]. Đó cùng là ý tưởng "checksum ở cha" (phép nối của người soạn).

### 6.3 Snapshot

Cây không bị ghi đè thì snapshot gần như không tốn gì. Tài liệu OpenZFS viết rằng snapshot chỉ là một tham chiếu ngăn các block đó bị giải phóng, và không có gì được copy[^zfs-snap]. OSTEP kể lại cách WAFL biến "garbage" của LFS thành tính năng: đưa ra các phiên bản cũ qua snapshot để người dùng lấy lại file lỡ xóa[^lfs-wafl].

Btrfs làm tương tự: snapshot là một subvolume có nội dung khởi tạo, mặc định tạo ở dạng read-write, và sửa file trong snapshot không ảnh hưởng subvolume gốc[^btrfs-snap]. Nhưng tài liệu Btrfs cảnh báo rõ: snapshot không phải là backup. Snapshot và bản gốc ban đầu dùng chung mọi block dữ liệu, nên nếu dữ liệu đó hỏng (bad sector, `dd` nhầm lên đĩa) thì cả hai cùng hỏng[^btrfs-notbackup]. Với `orders-db`, snapshot đỡ được chuyện lỡ tay xóa file, nhưng không đỡ được chuyện đĩa hỏng (nhận xét của người soạn).

<details><summary>Đi sâu: transaction group (txg) và ZIL của ZFS</summary>

ZFS gom các lần ghi thành **transaction group (txg)** rồi commit nguyên tử cả nhóm. Có ba txg cùng tồn tại (đang mở, đang quiescing, đang sync), và một nhóm đóng khi `zfs_txg_timeout` hết (mặc định 5 giây) hoặc khi có đủ dữ liệu bẩn[^zfs-txg].

Với ghi bất đồng bộ, hàm trả về khi dữ liệu đã nằm trong RAM. Nếu crash, các txg chưa sync bị mất và pool quay về txg đã commit gần nhất: nhất quán nhưng cũ hơn một chút[^zfs-txg-loss]. Ứng dụng gọi `fsync()` thì được ZFS Intent Log (ZIL) phục vụ: ZIL ghi ngay các ghi đồng bộ, để phát lại nếu hệ thống chết trước khi txg commit[^zfs-zil]. `fsync` ở tầng ứng dụng nằm ở [Chương 06](./06-durability-trong-thuc-te.md).

</details>

**Quay lại Q6:** COW ghi bản mới rồi đổi gốc nên cây cũ còn nguyên; snapshot rẻ nhưng dùng chung block với bản gốc nên không phải backup.

## 7. Btrfs và ZFS: checksum, RAID tích hợp và cái giá của COW (Q7)

> **Trả lời ngắn.** ZFS và Btrfs đều lưu checksum để phát hiện dữ liệu hỏng, và ZFS còn tự sửa khi có dư thừa[^zfs-cksum]. Cả hai cũng tự quản nhiều thiết bị theo kiểu RAID ngay trong file system: Btrfs có striping, mirroring và parity, ZFS có RAIDZ[^raidz]. Cái giá của COW là fragmentation, cần chỗ trống để ghi, và ghi từng phần record đắt[^zfs-costs].

**Giải thích.**

*Với `orders-db`.* Nếu thư mục dữ liệu nằm trên ZFS hay Btrfs thì checksum và RAID không còn là việc của controller hay của md nữa, mà do chính file system lo (nhận xét của người soạn). Mục này xem hai chuyện: file system giúp `orders-db` phát hiện và sửa block hỏng ra sao, và đổi lại `orders-db` phải trả giá gì.

### 7.1 Checksum end-to-end và tự sửa lỗi

ZFS kiểm checksum end-to-end: dữ liệu hỏng được phát hiện khi đọc từ phương tiện. Block hỏng được tự sửa nếu có thể, nhờ bảo vệ kiểu RAID của pool hoặc các bản sao dư thừa. Scrub định kỳ thì phát hiện và sửa các suy thoái tiềm ẩn[^zfs-cksum]. Lý do checksum đứng ở block cha đã nói ở mục 6.2.

Btrfs lưu checksum cho cả dữ liệu lẫn metadata. Block metadata có checksum nội tuyến trong header nút b-tree, còn mỗi block dữ liệu có checksum tách rời nằm trong **checksum tree**[^btrfs-ck-layout]. Tính năng "Self-healing" của Btrfs nằm trong danh sách đặc điểm chính trên trang giới thiệu[^btrfs-intro].

### 7.2 RAID tích hợp trong file system

Vì Btrfs và ZFS tự quản nhiều thiết bị, chúng có "RAID" riêng. Btrfs liệt kê striping (như RAID0), mirroring (như RAID1, tới 4 bản), stripe cộng mirror (như RAID10), và parity đơn hoặc kép (như RAID5/6). Với nhóm parity, tài liệu btrfs-progs ở tag v6.14 ghi rõ là thử nghiệm, chưa sẵn sàng cho production[^btrfs-raid].

ZFS có **RAIDZ**, một biến thể của RAID-5 cho phép phân bố parity tốt hơn, với một, hai hoặc ba parity (raidz1/2/3). Nhóm N đĩa kích thước X với P parity chứa khoảng (N−P)·X byte và chịu P đĩa hỏng[^raidz]. Tài liệu nói RAIDZ loại bỏ "write hole" của RAID-5, tức tình trạng dữ liệu và parity trở nên bất nhất sau khi mất điện[^raidz-hole].

Hãy đặt cạnh nhau cách hai bên xử lý write hole. md dùng journal hoặc PPL[^md-policy]. Còn tài liệu COW của OpenZFS gắn việc pool không có "RAID-5 write hole" với việc chuyển sang cây mới bằng một lần ghi nguyên tử (mục 6.2)[^zfs-atomic]. Tài liệu RAIDZ chỉ nêu kết quả, không nêu cơ chế riêng của RAIDZ, nên chương này không khẳng định RAIDZ tránh write hole bằng cách nào; việc đặt md cạnh ZFS là tổng hợp của người soạn.

### 7.3 Cái giá của COW

Ghi sang chỗ mới thì sinh rác. Trong LFS, các bản cũ của dữ liệu nằm rải rác khắp đĩa, nên phải dọn các segment cũ định kỳ. Việc dọn đó gọi là **cleaning**, một dạng garbage collection. OSTEP nói cleaning là điểm gây tranh cãi và có thể đã hạn chế ảnh hưởng ban đầu của LFS[^lfs-garbage].

OpenZFS liệt kê ba chi phí của COW trong tài liệu của mình[^zfs-costs]:

| Chi phí | Mô tả theo tài liệu |
|---|---|
| Fragmentation | Ghi lại file làm block mới nằm ở nơi nào còn chỗ trống, nên dữ liệu ghi tuần tự rồi sửa ngẫu nhiên không còn tuần tự; pool đầy khiến tìm chỗ trống liền khó hơn. |
| Cần chỗ trống để ghi | Ngay cả xóa dữ liệu cũng cần cấp phát, nên có "slop space" dành riêng. |
| Ghi từng phần đắt | Đổi một phần record nghĩa là đọc cả record, sửa, rồi ghi record mới; khớp `recordsize` với kích thước I/O ứng dụng là cách xử lý thông thường. |

Về chi phí ghi, `orders-db` nên nhớ rằng chi phí của một ghi nhỏ tỷ lệ với độ sâu của cây, không phải kích thước của nó[^zfs-depth]. Có thể gọi đó là một dạng **write amplification**, vì sửa một block kéo theo ghi lại cả đường từ block đó lên gốc (suy luận của người soạn: tài liệu OpenZFS không dùng thuật ngữ này, và chương không có số liệu đo nên không khẳng định mức khuếch đại).

### 7.4 Overlay / union file system

Overlay/union file system của container: xem [Chương 04 — Filesystem của container](../08-containers/04-filesystem-cua-container.md).

<details><summary>Đi sâu: thuật toán checksum của Btrfs, O_DIRECT và phiên bản tài liệu</summary>

**Thuật toán checksum.** Mặc định là crc32c. Từ kernel 5.5 có thêm ba thuật toán khác (xxhash, sha256, blake2b), đánh đổi khác nhau giữa tốc độ và độ mạnh[^btrfs-ck-algos].

**Tương tác với O_DIRECT (chương [04](./04-page-cache-va-direct-io.md)).** Checksum dữ liệu được tính ngay trước khi gửi xuống thiết bị, nên Btrfs đòi block không đổi cho tới khi writeback xong; bộ đệm `O_DIRECT` nằm ở user space nên không kiểm soát được. Theo tài liệu, từ kernel 6.14 direct write bị ép quay về buffered nếu inode cần checksum dữ liệu[^btrfs-odirect].

**Phiên bản.** Trạng thái "thử nghiệm" của parity ở §7.2 là của tag btrfs-progs v6.14, không suy ra cho bản khác. Kernel v6.8 `btrfs.rst` chỉ ghi chung là có nhiều thuật toán raid[^btrfs-k68].

</details>

**Quay lại Q7:** ZFS/Btrfs gắn checksum vào cây COW và tự quản RAID, đổi lại là fragmentation, nhu cầu chỗ trống và ghi từng phần đắt.

## 8. Storage trên cloud: nhận xét về backend

(Nhận xét của người soạn, không có nguồn trích; chương chưa đọc tài liệu của nhà cung cấp cloud nên không khẳng định gì về cách họ làm dư thừa.) Giả sử `orders-db` chạy trên đĩa cloud, trong một máy có md, file system ZFS hoặc Btrfs, và database lại có replication riêng. Lúc đó có nhiều lớp dư thừa chồng lên nhau. Với mỗi lớp, hãy hỏi: ai ghi checksum, và một ghi nhỏ tốn bao nhiêu I/O.

## 9. Liên hệ với Rust

Chương này không có API Rust riêng (nhận xét của người soạn). Với `orders-db` hay bất kỳ service nào viết bằng Rust, RAID và COW nằm dưới `std::fs`, và code Rust không thấy chúng. Phần Rust duy nhất ở đây là một đoạn code nhỏ kiểm chứng số học parity: nó XOR từng byte của các block để tính parity, dựng lại block mất, và so cách cập nhật subtractive với cách tính lại từ đầu.

```rust
fn xor_all(blocks: &[&[u8; 4]]) -> [u8; 4] {
    let mut out = [0u8; 4];
    for b in blocks {
        for i in 0..4 {
            out[i] ^= b[i];
        }
    }
    out
}

fn main() {
    let d0 = [0x11, 0x22, 0x33, 0x44];
    let d1 = [0xaa, 0xbb, 0xcc, 0xdd];
    let d2 = [0x01, 0x02, 0x03, 0x04];

    // Parity của stripe: XOR từng byte của mọi block dữ liệu.
    let p = xor_all(&[&d0, &d1, &d2]);
    println!("parity P      = {:02x?}", p);

    // Mất d1: dựng lại từ các block còn lại và parity.
    let rebuilt = xor_all(&[&d0, &d2, &p]);
    println!("dựng lại d1   = {:02x?} (đúng: {})", rebuilt, rebuilt == d1);

    // Ghi đè d1: cập nhật parity theo kiểu subtractive.
    let d1_new = [0x5a, 0x5b, 0x5c, 0x5d];
    let p_sub = xor_all(&[&d1, &d1_new, &p]);
    println!("P mới (subtractive) = {:02x?}", p_sub);

    // Đối chiếu: tính lại parity từ đầu với d1 mới.
    let p_full = xor_all(&[&d0, &d1_new, &d2]);
    println!("P mới (tính lại)    = {:02x?} (khớp: {})", p_full, p_sub == p_full);
}
```

Kết quả chạy thật trên **macOS arm64** (`rustc -O` 1.95.0, một lần chạy; file tạm nằm trong `/tmp` và đã xóa):

```text
parity P      = [ba, 9b, fc, 9d]
dựng lại d1   = [aa, bb, cc, dd] (đúng: true)
P mới (subtractive) = [4a, 7b, 6c, 1d]
P mới (tính lại)    = [4a, 7b, 6c, 1d] (khớp: true)
```

Parity là `[ba, 9b, fc, 9d]`, dựng lại `d1` đúng, và parity mới theo hai cách đều là `[4a, 7b, 6c, 1d]`. Đây chỉ là kiểm chứng số học, không mô phỏng crash.

## Tóm tắt

- RAID ghép nhiều đĩa thành một "đĩa" nhanh, lớn, đáng tin hơn[^raid-def]. Nó trong suốt với phần phía trên, nên `orders-db` vẫn thấy một đĩa[^raid-looks].
- Fail-stop là giả định đơn giản hóa: đĩa chạy hoặc hỏng hẳn[^failstop]. Corruption im lặng và LSE thực tế hơn[^failstop-later].
- RAID-0 không dư thừa[^r0]. RAID-1 tốn nửa dung lượng[^r1-cap]. RAID-4/5 dùng XOR parity, dung lượng (N−1)·B và chịu 1 đĩa hỏng[^table].
- Ghi nhỏ trên RAID parity tốn 4 I/O[^4io]. RAID-4 bị nghẽn ở đĩa parity[^bottleneck]. RAID-5 xoay parity để bỏ nút thắt đó nhưng vẫn mất hệ số 4[^r5-four].
- Consistent-update problem: cập nhật nhiều đĩa không nguyên tử, và lời giải là write-ahead log[^cu-wal]. Phần cứng dùng NVRAM[^cu-nvram]. Còn md dùng journal hoặc PPL[^md-policy].
- Checksum bắt corruption im lặng[^checksum-def]. Misdirected write cần thêm physical ID[^misdirected-id]. Lost write cần checksum ở nơi khác[^lost-zfs].
- COW/shadow paging không ghi đè: ghi bản mới rồi đổi gốc[^lfs-shadow]. Nhờ vậy pool nhất quán sau crash[^zfs-atomic]. Cây cũ còn nguyên nên snapshot rẻ[^zfs-snap]. Checksum nằm ở block cha[^zfs-tree].
- Giá của COW: phải dọn rác (cleaning)[^lfs-garbage]. Ngoài ra còn fragmentation và ghi từng phần đắt[^zfs-costs]. Snapshot không phải backup[^btrfs-notbackup].

## Câu hỏi tự kiểm tra

1. Fail-stop giả định gì? Nêu hai loại lỗi nó chưa xét.
<details><summary>Đáp án</summary>

Đĩa hoặc chạy hoặc hỏng hẳn[^failstop]. Khi đĩa hỏng thì lỗi dễ phát hiện[^failstop-detect]. Mô hình chưa xét corruption im lặng và latent sector error[^failstop-later].

</details>

2. RAID-1 mức 2 chịu được mấy đĩa hỏng, vì sao thực tế chỉ tin một?
<details><summary>Đáp án</summary>

Chắc chắn 1, tối đa N/2 tùy đĩa nào hỏng; không nên phó mặc may rủi[^r1-rel].

</details>

3. Tính parity của (0,0,1,1) và nêu cách dựng lại một bit mất.
<details><summary>Đáp án</summary>

XOR của bốn bit bằng 0[^par-xor]. Bit mất bằng XOR của các bit còn lại với parity (suy ra từ tính chất XOR, đã kiểm bằng code ở mục 9).

</details>

4. Vì sao ghi một block vào RAID-5 (subtractive) tốn 4 I/O?
<details><summary>Đáp án</summary>

Phải đọc dữ liệu cũ, đọc parity cũ, ghi dữ liệu mới và ghi parity mới[^4io]. Parity mới tính từ ba thứ: dữ liệu cũ, dữ liệu mới và parity cũ[^subtractive].

</details>

5. Vì sao RAID-4 không scale ghi nhỏ, và RAID-5 sửa gì?
<details><summary>Đáp án</summary>

Mọi ghi nhỏ đi qua đĩa parity duy nhất nên bị xếp hàng lần lượt[^bottleneck]. Throughput ghi nhỏ ngẫu nhiên vì thế tệ và không khá hơn khi thêm đĩa[^r4-terrible]. RAID-5 xoay parity qua các đĩa để ghi chạy song song[^r5-rotate]. Nhưng ghi ngẫu nhiên vẫn chỉ đạt (N/4)·R vì mỗi ghi vẫn sinh 4 I/O[^r5-four].

</details>

6. Mô tả consistent-update problem và lời giải.
<details><summary>Đáp án</summary>

Ghi xong đĩa 0 rồi mất điện trước khi ghi đĩa 1 thì hai bản lệch nhau[^cu-scenario]. Lời giải là write-ahead log[^cu-wal]. Ở phần cứng, log nằm trong NVRAM[^cu-nvram]. Ở md, log là journal hoặc PPL[^md-policy].

</details>

7. Vì sao md thường từ chối khởi động mảng raid5 vừa dirty vừa degraded?
<details><summary>Đáp án</summary>

Parity không còn tin được và một số block dữ liệu thì thiếu, nên dữ liệu có thể hỏng mà không phát hiện được[^md-dirty].

</details>

8. Bốn bước ZFS sửa một block là gì, và điều gì giữ pool nhất quán sau mất điện?
<details><summary>Đáp án</summary>

Ghi dữ liệu mới; ghi cha mới; lặp lên gốc; ghi uberblock mới[^zfs-write]. Uberblock cũ vẫn mô tả một cây trọn vẹn, và chuyển sang cây mới là một lần ghi nguyên tử[^zfs-atomic].

</details>

9. Vì sao snapshot COW rẻ nhưng không thay được backup?
<details><summary>Đáp án</summary>

Cây cũ còn nguyên, nên snapshot chỉ là tham chiếu ngăn giải phóng block[^zfs-snap]. Nhưng snapshot dùng chung block với bản gốc nên hỏng cùng nhau[^btrfs-notbackup].

</details>

## Đọc thêm

- [OSTEP — RAID (file-raid)](https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf): phân tích từng cấp RAID.
- [OSTEP — Log-structured file systems (file-lfs)](https://pages.cs.wisc.edu/~remzi/OSTEP/file-lfs.pdf): nền của COW.
- [OSTEP — Data integrity (file-integrity)](https://pages.cs.wisc.edu/~remzi/OSTEP/file-integrity.pdf): LSE, checksum, lost write.
- [OpenZFS — Copy-on-write](https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Copy-on-write.html): diễn giải cây, txg, ZIL và chi phí của COW.
- [Linux kernel v6.8 — md.rst](https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/md.rst): thuộc tính `consistency_policy`, boot-time assembly, mảng dirty/degraded.
- [Btrfs — Introduction](https://raw.githubusercontent.com/kdave/btrfs-progs/v6.14/Documentation/Introduction.rst), [Subvolumes](https://raw.githubusercontent.com/kdave/btrfs-progs/v6.14/Documentation/ch-subvolume-intro.rst), [Checksumming](https://raw.githubusercontent.com/kdave/btrfs-progs/v6.14/Documentation/ch-checksumming.rst): tính năng, snapshot, checksum (nguồn rst của btrfs-progs tag v6.14; trang readthedocs `latest` không ghim phiên bản).
- [Chương 05](./05-crash-consistency-va-journaling.md) và [Chương 06](./06-durability-trong-thuc-te.md): log và `fsync` ở tầng file system/ứng dụng.

[^raid-def]: OSTEP, "Redundant Arrays of Inexpensive Disks" (file-raid), mục giới thiệu — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "a technique to use multiple disks in concert to build a faster, bigger, and more reliable disk system"
[^raid-adv]: OSTEP, file-raid, mục giới thiệu — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "RAIDs offer a number of advantages over a single disk. One advantage is performance."
[^raid-looks]: OSTEP, file-raid, mục giới thiệu — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "Externally, a RAID looks like a disk: a group of blocks one can read or write."
[^raid-logical]: OSTEP, file-raid, mục 38.1 Interface And RAID Internals — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "the RAID will have to perform two physical I/Os for every one logical I/O it is issued."
[^raid-transparent]: OSTEP, file-raid, hộp TIP: Transparency Enables Deployment — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "administrators could install a SCSI-based RAID storage array instead of a SCSI disk, and the rest of the system (host computer, OS, etc.) did not have to change one bit to start using it."
[^failstop]: OSTEP, file-raid, mục 38.2 Fault Model — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "In this model, a disk can be in exactly one of two states: working or failed."
[^failstop-detect]: OSTEP, file-raid, mục 38.2 Fault Model — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "when a disk has failed, we assume that this is easily detected."
[^failstop-later]: OSTEP, file-raid, mục 38.2 Fault Model — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "We will consider these more complex (and unfortunately, more realistic) disk faults later."
[^axes]: OSTEP, file-raid, mục 38.3 How To Evaluate A RAID — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "Different schemes (e.g., parity-based ones) tend to fall in between."
[^r0]: OSTEP, file-raid, mục 38.4 RAID Level 0: Striping — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "The first RAID level is actually not a RAID level at all, in that there is no redundancy."
[^r0-rel]: OSTEP, file-raid, mục 38.4, Back To RAID-0 Analysis — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "striping is also perfect, but in the bad way: any disk failure will lead to data loss."
[^chunk-size-def]: OSTEP, file-raid, mục 38.4 (Figure 38.2) — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "a stripe thus consists of 4 chunks or 32KB of data."
[^chunk-perf]: OSTEP, file-raid, mục 38.4, hộp Chunk Sizes — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "Chunk size mostly affects performance of the array."
[^r1-rw]: OSTEP, file-raid, mục 38.5 RAID Level 1: Mirroring — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "the RAID must update both copies of the data, in order to preserve reliability."
[^r1-cap]: OSTEP, file-raid, mục 38.5, RAID-1 Analysis — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "From a capacity standpoint, RAID-1 is expensive;"
[^r1-rel]: OSTEP, file-raid, mục 38.5, RAID-1 Analysis — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "a mirrored system (with mirroring level of 2) can tolerate 1 disk failure for certain, and up to N/2 failures depending on which disks fail."
[^r10]: OSTEP, file-raid, mục 38.5 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "RAID-10 (or RAID 1+0, stripe of mirrors) because it uses mirrored pairs (RAID-1) and then stripes (RAID-0) on top of them; another common arrangement is RAID-01 (or RAID 0+1, mirror of stripes)"
[^cu-scenario]: OSTEP, file-raid, hộp The RAID Consistent-Update Problem — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "the copy on disk 0 is the new version, and the copy on disk 1 is the old."
[^cu-wal]: OSTEP, file-raid, hộp The RAID Consistent-Update Problem — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "The general way to solve this problem is to use a write-ahead log of some kind to first record what the RAID is about to do"
[^cu-nvram]: OSTEP, file-raid, hộp The RAID Consistent-Update Problem — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "most RAID hardware includes a small amount of non-volatile RAM (e.g., battery-backed) where it performs this type of logging."
[^cu-sw]: OSTEP, file-raid, mục 38.9 Other Interesting RAID Issues — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "such software RAID systems are cheaper but have other problems, including the consistent-update problem [DAA05]."
[^par-intro]: OSTEP, file-raid, mục 38.6 RAID Levels 4 and 5 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "They do so at a cost, however: performance."
[^par-p1]: OSTEP, file-raid, mục 38.6 (Figure 38.4) — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "For example, parity block P1 has redundant information that it calculated from blocks 4, 5, 6, and 7."
[^par-xor]: OSTEP, file-raid, mục 38.6 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "It turns out the simple function XOR does the trick quite nicely."
[^table]: OSTEP, file-raid, mục 38.7 RAID-5 Analysis và Figure 38.8 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "the effective capacity and failure tolerance of the two levels are identical."
[^additive]: OSTEP, file-raid, mục 38.6 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "read in all of the other data blocks in the stripe in parallel"
[^subtractive]: OSTEP, file-raid, mục 38.6 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "we read in the old data at C2 (C2old = 1) and the old parity (Pold = 0)."
[^4io]: OSTEP, file-raid, mục 38.6 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "for each write, the RAID has to perform 4 physical I/Os (two reads and two writes)."
[^bottleneck]: OSTEP, file-raid, mục 38.6 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "all writes to the system will be serialized because of the parity disk."
[^r4-terrible]: OSTEP, file-raid, mục 38.6 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "RAID-4 throughput under random small writes is terrible; it does not improve as you add disks to the system."
[^r5-rotate]: OSTEP, file-raid, mục 38.7 RAID Level 5: Rotating Parity — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "RAID-5 works almost identically to RAID-4, except that it rotates the parity block across drives"
[^r5-example]: OSTEP, file-raid, mục 38.7 RAID-5 Analysis — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "Imagine a write to block 1 and a write to block 10; this will turn into requests to disk 1 and disk 4"
[^r5-layout]: OSTEP, file-raid, mục 38.7, Figure 38.7 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "the parity block for each stripe is now rotated across the disks, in order to remove the parity-disk bottleneck for RAID-4."
[^r5-analysis]: OSTEP, file-raid, mục 38.7 RAID-5 Analysis — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "Random read performance is a little better, because we can now utilize all disks."
[^r5-four]: OSTEP, file-raid, mục 38.7 RAID-5 Analysis — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "The factor of four loss is due to the fact that each RAID-5 write still generates 4 total I/O operations, which is simply the cost of using parity-based RAID."
[^r5-replace]: OSTEP, file-raid, mục 38.7 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "it has almost completely replaced RAID-4 in the marketplace."
[^r0-lat]: OSTEP, file-raid, mục 38.4, Back To RAID-0 Analysis (performance) — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "the latency of a single-block request should be just about identical to that of a single disk"
[^r0-tp]: OSTEP, file-raid, mục 38.4, Back To RAID-0 Analysis, Again — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "From the perspective of steady-state sequential throughput, we’d expect to get the full bandwidth of the system."
[^r1-seqread]: OSTEP, file-raid, mục 38.5, RAID-1 Analysis — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "Unfortunately, we obtain the exact same performance during a sequential read."
[^r1-rand]: OSTEP, file-raid, mục 38.5, RAID-1 Analysis — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "Random reads are the best case for a mirrored RAID."
[^r1-wlat]: OSTEP, file-raid, mục 38.5, RAID-1 Analysis — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "because the logical write must wait for both physical writes to complete"
[^r4-cap]: OSTEP, file-raid, mục 38.6, RAID-4 Analysis — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "RAID-4 tolerates 1 disk failure and no more."
[^r4-seq]: OSTEP, file-raid, mục 38.6, RAID-4 Analysis — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "Sequential read performance can utilize all of the disks except for the parity disk"
[^r4-lat]: OSTEP, file-raid, mục 38.6, RAID-4 Analysis — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "The latency of a single write requires two reads and then two writes;"
[^table-note1]: OSTEP, file-raid, mục 38.8, chú thích 1 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "The 1/2 penalty assumes a naive read/write pattern for mirroring;"
[^table-legend]: OSTEP, file-raid, mục 38.8 RAID Comparison: A Summary — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "For the latency analysis, we simply use T to represent the time that a request to a single disk would take."
[^table-footnote]: OSTEP, file-raid, mục 38.8 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "Finally, sequential I/O to mirrored RAIDs pay a 2× performance penalty"
[^table-caveat]: OSTEP, file-raid, mục 38.8 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "However, the comparison in Figure 38.8 does capture the essential differences, and is useful for understanding tradeoffs across RAID levels."
[^choose]: OSTEP, file-raid, mục 38.8 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "If capacity and reliability are your main goals, then RAID5 is the winner; the cost you pay is in small-write performance."
[^art]: OSTEP, file-raid, mục 38.10 Summary — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "remains more of an art than a science."
[^lse-rebuild]: OSTEP, file-integrity, mục 45.2 Handling Latent Sector Errors — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-integrity.pdf> — "the reconstruction cannot successfully complete."
[^raiddp]: OSTEP, file-integrity, mục 45.2 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-integrity.pdf> — "NetApp’s RAID-DP has the equivalent of two parity disks instead of one"
[^scrub]: OSTEP, file-integrity, mục 45.1 (các phát hiện về LSE) — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-integrity.pdf> — "Disk scrubbing is useful (most LSEs were found this way)"
[^md-level]: Linux kernel v6.8, Documentation/admin-guide/md.rst, thuộc tính `level` — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/md.rst> — "e.g. raid0, raid1, raid5, linear, multipath, faulty."
[^md-dirty]: Linux kernel v6.8, Documentation/admin-guide/md.rst, Boot time assembly of degraded/dirty arrays — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/md.rst> — "If a raid5 or raid6 array is both dirty and degraded, it could have undetectable data corruption."
[^md-policy]: Linux kernel v6.8, Documentation/admin-guide/md.rst, thuộc tính `consistency_policy` — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/md.rst> — "For raid5 only, Partial Parity Log is used to close the write hole and eliminate resync."
[^lse-vs-corrupt]: OSTEP, file-integrity, mục 45.1 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-integrity.pdf> — "silent faults; the disk gives no indication of the problem when returning"
[^checksum-def]: OSTEP, file-integrity, mục 45.3 Detecting Corruption: The Checksum — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-integrity.pdf> — "A checksum is simply the result of a function that takes a chunk of data (say a 4KB block) as input"
[^misdirected]: OSTEP, file-integrity, mục 45.5 A New Problem: Misdirected Writes — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-integrity.pdf> — "This arises in disk and RAID controllers which write the data to disk correctly, except in the wrong location."
[^lost-write]: OSTEP, file-integrity, mục 45.6 One Last Problem: Lost Writes — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-integrity.pdf> — "which occurs when the device informs the upper layer that a write has completed but in fact it never is persisted"
[^lfs-never]: OSTEP, file-lfs, mục giới thiệu — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-lfs.pdf> — "LFS never overwrites existing data, but rather always writes segments to free locations."
[^lfs-move]: OSTEP, file-lfs, mục 43.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-lfs.pdf> — "Worse, we never overwrite in place, and thus the latest version of an inode (i.e., the one we want) keeps moving."
[^lfs-shadow]: OSTEP, file-lfs, mục 43.13 Summary — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-lfs.pdf> — "which in database systems is called shadow paging [L77] and in file-system-speak is sometimes called copy-on-write"
[^lfs-raid]: OSTEP, file-lfs, mục 43.13 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-lfs.pdf> — "on parity-based RAIDs, such as RAID-4 and RAID-5, they avoid the small-write problem entirely."
[^lfs-legacy]: OSTEP, file-lfs, mục 43.13 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-lfs.pdf> — "Sun’s ZFS [B07], and Linux btrfs [R+13]"
[^lfs-wafl]: OSTEP, file-lfs, mục 43.13 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-lfs.pdf> — "users could access old files whenever they deleted current ones accidentally."
[^lfs-garbage]: OSTEP, file-lfs, mục 43.13 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-lfs.pdf> — "one must clean old segments periodically."
[^zfs-rule]: OpenZFS docs, Basic Concepts, Copy-on-Write — <https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Copy-on-write.html> — "a block that is in use is never overwritten."
[^zfs-tree]: OpenZFS docs, Copy-on-Write, mục The tree — <https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Copy-on-write.html> — "A block’s checksum is therefore stored in its parent, not next to the block itself, so a damaged block cannot vouch for itself."
[^zfs-write]: OpenZFS docs, Copy-on-Write, mục Writing something — <https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Copy-on-write.html> — "writes the new data to free space;"
[^zfs-path]: OpenZFS docs, Copy-on-Write, mục Writing something — <https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Copy-on-write.html> — "the path from the changed block to the root, and nothing else."
[^zfs-atomic]: OpenZFS docs, Copy-on-Write, mục Writing something — <https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Copy-on-write.html> — "so no equivalent of the RAID-5 write hole and no need for a journal replay of metadata."
[^zfs-txg]: OpenZFS docs, Copy-on-Write, mục Transaction groups — <https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Copy-on-write.html> — "Three transaction groups are in flight at once"
[^zfs-txg-loss]: OpenZFS docs, Copy-on-Write, mục Transaction groups — <https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Copy-on-write.html> — "It is consistent, just slightly older."
[^zfs-zil]: OpenZFS docs, Copy-on-Write, mục Transaction groups — <https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Copy-on-write.html> — "it records synchronous writes immediately so they can be replayed if the system dies before the txg commits."
[^zfs-snap]: OpenZFS docs, Copy-on-Write, mục What follows from it — <https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Copy-on-write.html> — "a snapshot is just a reference that stops those blocks being freed. Nothing is copied."
[^zfs-costs]: OpenZFS docs, Copy-on-Write, mục The costs — <https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Copy-on-write.html> — "Rewriting a file scatters its new blocks wherever there was free space, so data that was written sequentially and then modified randomly does not stay sequential."
[^zfs-depth]: OpenZFS docs, Copy-on-Write, mục Writing something — <https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Copy-on-write.html> — "The cost of a small write is therefore proportional to the depth of the tree, not its size"
[^zfs-cksum]: OpenZFS docs, Checksums and Their Use in ZFS — <https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Data%20Storage/Checksums.html> — "detects data corruption upon reading from media"
[^raidz]: OpenZFS docs, Pool Structure, RAIDZ — <https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Pool%20Structure/RAIDZ.html> — "RAIDZ is a variation on RAID-5 that allows for better distribution of parity"
[^raidz-hole]: OpenZFS docs, Pool Structure, RAIDZ — <https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Pool%20Structure/RAIDZ.html> — "in which data and parity become inconsistent after a power loss"
[^btrfs-snap]: btrfs-progs v6.14, Documentation/ch-subvolume-intro.rst (nội dung trang Subvolumes) — <https://raw.githubusercontent.com/kdave/btrfs-progs/v6.14/Documentation/ch-subvolume-intro.rst> — "File modifications in a snapshot do not affect the files in the original subvolume."
[^btrfs-notbackup]: btrfs-progs v6.14, Documentation/ch-subvolume-intro.rst — <https://raw.githubusercontent.com/kdave/btrfs-progs/v6.14/Documentation/ch-subvolume-intro.rst> — "A snapshot is not a backup: snapshots work by use of BTRFS' copy-on-write behaviour."
[^btrfs-ck-layout]: btrfs-progs v6.14, Documentation/ch-checksumming.rst — <https://raw.githubusercontent.com/kdave/btrfs-progs/v6.14/Documentation/ch-checksumming.rst> — "Each data block has a detached checksum stored in the checksum tree."
[^btrfs-ck-algos]: btrfs-progs v6.14, Documentation/ch-checksumming.rst — <https://raw.githubusercontent.com/kdave/btrfs-progs/v6.14/Documentation/ch-checksumming.rst> — "The default and backward compatible algorithm is crc32c. Since kernel 5.5 there are three more with different characteristics and trade-offs regarding speed and strength."
[^btrfs-intro]: btrfs-progs v6.14, Documentation/Introduction.rst — <https://raw.githubusercontent.com/kdave/btrfs-progs/v6.14/Documentation/Introduction.rst> — "Self-healing - checksums for data and metadata, automatic detection of silent data corruptions"
[^btrfs-odirect]: btrfs-progs v6.14, Documentation/ch-checksumming.rst — <https://raw.githubusercontent.com/kdave/btrfs-progs/v6.14/Documentation/ch-checksumming.rst> — "kernel starting with version 6.14 will force a direct write to fall back to buffered, if the inode requires a data checksum."
[^btrfs-raid]: btrfs-progs v6.14, Documentation/Introduction.rst, Feature overview — <https://raw.githubusercontent.com/kdave/btrfs-progs/v6.14/Documentation/Introduction.rst> — "Single and Dual Parity implementations (like RAID5/6, experimental, not production-ready)"
[^r4-parity-disk]: OSTEP, file-raid, mục 38.6, RAID-4 Analysis — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-raid.pdf> — "RAID-4 uses 1 disk for parity information for every group of disks it is protecting. Thus, our useful capacity for a RAID group is (N − 1) · B."
[^md-safe]: Linux kernel v6.8, Documentation/admin-guide/md.rst, thuộc tính `safe_mode_delay` — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/md.rst> — "When another write request arrives, the array is marked as ``dirty`` before the write commences."
[^misdirected-id]: OSTEP, file-integrity, mục 45.5 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-integrity.pdf> — "if the stored information now contains the checksum C(D) and both the disk and sector numbers of the block, it is easy for the client to determine whether the correct information resides within a particular locale."
[^lost-cksum]: OSTEP, file-integrity, mục 45.6 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-integrity.pdf> — "the old block likely has a matching checksum, and the physical ID used above (disk number and block offset) will also be correct."
[^lost-zfs]: OSTEP, file-integrity, mục 45.6 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-integrity.pdf> — "Sun’s Zettabyte File System (ZFS) includes a checksum in each file system inode and indirect block for every block included within a file."
[^scrub-def]: OSTEP, file-integrity, mục 45.7 Scrubbing — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-integrity.pdf> — "By periodically reading through every block of the system, and checking whether checksums are still valid"
[^lfs-cr]: OSTEP, file-lfs, mục 43.6 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-lfs.pdf> — "The checkpoint region contains pointers to (i.e., addresses of) the latest pieces of the inode map"
[^btrfs-k68]: Linux kernel v6.8, Documentation/filesystems/btrfs.rst — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/filesystems/btrfs.rst> — "Integrated multiple device support, with several raid algorithms"
