# P5 · Chương 01 — Thiết bị lưu trữ

## Bạn sẽ hiểu được gì

1. **Q1.** Kernel nói chuyện với một thiết bị I/O thế nào: thanh ghi, polling, interrupt, DMA, và vì sao cần device driver? → mục 1
2. **Q2.** Một lần đọc 4 KB trên ổ cứng quay (HDD) tốn thời gian ở đâu, và vì sao I/O tuần tự nhanh hơn ngẫu nhiên hàng trăm lần? Bạn tự tính được `T_I/O` và `R_I/O`. → mục 2
3. **Q3.** SSTF, elevator (SCAN), SPTF khác nhau thế nào, và vì sao SPTF nằm trong ổ chứ không nằm trong OS? → mục 3
4. **Q4.** Vì sao flash phải "erase trước, program sau", và FTL, garbage collection, wear leveling, write amplification, TRIM sinh ra từ ràng buộc đó ra sao? → mục 4
5. **Q5.** Block layer multi-queue (blk-mq) của Linux hoạt động thế nào; `none`, `mq-deadline`, `bfq` khác nhau gì; NVMe khớp vào đâu? → mục 5
6. **Q6.** Kernel báo những con số nào về kích thước sector (512 vs 4 KB), và chúng bảo đảm gì? → mục 6

## Kiến thức cần trước

- [Chương 02 — System call](../01-linux/02-system-call.md): ranh giới user/kernel.
- [Chương 04 — File descriptor và VFS](../01-linux/04-file-descriptor-va-vfs.md): lớp trên của stack mà chương này đi xuống.

Đây là tầng thấp nhất của phase P5. Các tầng trên: API file ([chương 02](02-file-va-directory-api.md)), cách file system dàn dữ liệu lên block ([chương 03](03-cai-dat-file-system.md)), page cache và `O_DIRECT` ([chương 04](04-page-cache-va-direct-io.md)), crash consistency ([chương 05](05-crash-consistency-va-journaling.md)), `fsync` và độ bền ([chương 06](06-durability-trong-thuc-te.md)), RAID ([chương 07](07-raid-va-copy-on-write-fs.md)).

Ví dụ xuyên suốt chương là `orders-db`, cơ sở dữ liệu phía sau service `orders` ở các chương Linux. Đây là ví dụ giả định của người soạn, không phải code trong repo; mỗi mục mở đầu bằng việc `orders-db` đụng vào thiết bị lưu trữ ở chỗ nào.

## 1. Từ CPU đến thiết bị (Q1)

> **Trả lời ngắn.** CPU điều khiển thiết bị qua một giao diện thanh ghi: OS ghi lệnh và dữ liệu vào đó, rồi chờ thiết bị làm xong[^dev-protocol]. Cách chờ thứ nhất là polling, tức hỏi thiết bị liên tục[^dev-poll]. Cách thứ hai là cho tiến trình ngủ và để thiết bị đánh thức bằng interrupt[^dev-irq]. Với khối dữ liệu lớn, DMA giao việc chép cho một engine riêng[^dev-dma]. Còn device driver giấu chi tiết từng thiết bị khỏi file system[^dev-driver].

**Giải thích.**

*Với `orders-db`.* Một request `GET /orders/42` làm database cần một page 4 KB đang nằm trên đĩa, nên một thread của nó gọi `read`. Kernel phải giao yêu cầu đó cho thiết bị rồi chờ. Chờ kiểu nào để CPU khỏi phí? Giao dữ liệu thế nào để CPU khỏi ngồi chép từng byte? Và vì sao `orders-db` chạy trên HDD, SSD hay đĩa ảo mà file system vẫn không phải sửa gì? Mục này trả lời ba câu đó.

### 1.1 Giao thức chuẩn: thanh ghi và polling

Để thấy cơ chế mà không vướng chi tiết của một con chip cụ thể, ta dùng "thiết bị chuẩn" của OSTEP (một thiết bị không có thật). Giao diện của nó gồm ba thanh ghi: *status* để đọc trạng thái, *command* để ghi lệnh, *data* để chuyển dữ liệu[^dev-regs]. Khi `orders-db` xin đọc, OS làm bốn bước theo đúng thứ tự này[^dev-protocol]:

1. Chờ thiết bị hết bận.
2. Ghi dữ liệu vào thanh ghi data.
3. Ghi lệnh vào thanh ghi command.
4. Chờ thiết bị làm xong.

Cách OS chờ ở bước 1 và 4, tức đọc thanh ghi status lặp đi lặp lại cho đến khi thiết bị sẵn sàng, gọi là **polling**[^dev-poll]. Còn khi chính CPU chép dữ liệu qua thanh ghi data thì gọi là **programmed I/O (PIO)**[^dev-pio]. Giao thức này đơn giản và chạy được, nhưng có những điểm kém hiệu quả[^dev-pollcost]. Điểm rõ nhất: polling đốt CPU để chờ một thiết bị có thể rất chậm. Thread của `orders-db` đứng quay vòng hỏi "xong chưa" trong lúc đĩa còn đang tìm dữ liệu thì CPU chẳng làm được việc gì khác.

### 1.2 Interrupt: ngủ thay vì hỏi

Lối ra là đừng hỏi nữa. OS gửi yêu cầu, cho tiến trình đang gọi đi ngủ, rồi chuyển CPU sang việc khác[^dev-irq]. Thiết bị làm xong thì phát một hardware interrupt, CPU nhảy vào interrupt handler của OS, handler hoàn tất yêu cầu và đánh thức tiến trình. Nhờ vậy CPU và đĩa chạy chồng lên nhau[^dev-irq2]. Với `orders-db`, trong lúc một thread chờ page 4 KB thì CPU vẫn xử lý được request khác.

```mermaid
sequenceDiagram
    participant P1 as Thread của orders-db
    participant K as Kernel
    participant D as Thiết bị
    participant P2 as Tiến trình khác
    P1->>K: read() (syscall)
    K->>D: ghi lệnh vào thanh ghi
    K->>P1: block (ngủ)
    K->>P2: context switch, chạy tiến trình khác
    D-->>K: interrupt khi xong
    K->>P1: handler đánh thức thread
```

(Sơ đồ minh họa của người soạn theo mô tả của OSTEP.)

Tuy vậy interrupt không phải lúc nào cũng thắng polling. Với thiết bị rất nhanh, lần poll đầu thường đã thấy việc xong, trong khi interrupt kéo theo chuyển sang tiến trình khác, xử lý ngắt rồi chuyển về, tức đắt hơn chính việc poll[^dev-notalways]. Vì thế có cách lai: poll một lúc, hết kiên nhẫn mới chuyển sang interrupt[^dev-hybrid]. Interrupt dồn dập cũng có thể gây hại: khi mỗi gói tin đến lại sinh một interrupt, OS có thể rơi vào livelock[^dev-livelock]. Một cách giảm nhẹ là *interrupt coalescing*: thiết bị đợi thêm một chút để gom nhiều hoàn tất vào một interrupt, đổi lấy độ trễ cao hơn[^dev-coalesce]. Mục 5 sẽ cho thấy NVMe trên Linux có cả queue chạy bằng interrupt lẫn queue chạy bằng polling.

### 1.3 DMA

PIO với khối lớn vẫn bắt CPU chép từng từ một. **Direct Memory Access (DMA)** bỏ việc đó đi bằng một engine chuyên dụng. OS lập trình engine bằng ba thông tin: dữ liệu nằm ở đâu trong RAM, chép bao nhiêu byte, gửi tới thiết bị nào. Engine chép xong thì phát interrupt[^dev-dma]. Khi `orders-db` đọc một khối lớn, CPU chỉ việc giao việc rồi đi làm chuyện khác.

### 1.4 Device driver và block layer

Mỗi loại thiết bị có giao diện riêng, nhưng file system thì không muốn biết điều đó, nếu không mỗi lần thêm một loại ổ lại phải sửa file system. Lời giải là trừu tượng hóa: một đoạn phần mềm trong OS biết rõ từng chi tiết của thiết bị, gọi là **device driver**[^dev-driver]. Mô hình "gần đúng" của OSTEP cho Linux là: file system chỉ gửi yêu cầu đọc/ghi block cho generic block layer, lớp này định tuyến yêu cầu tới driver phù hợp[^dev-stack].

```mermaid
flowchart TD
    A[Ứng dụng: open / read / write] --> B[VFS + file system]
    B --> C[Generic block layer]
    C --> D[Device driver: SCSI / ATA / NVMe / virtio]
    D --> E[Thiết bị]
```

Trừu tượng hóa có giá của nó: tính năng đặc biệt của từng thiết bị bị mất. SCSI báo lỗi rất phong phú, nhưng vì các block device khác báo lỗi đơn giản hơn nhiều, tầng trên chỉ nhận được một mã `EIO` chung[^dev-eio]. Nghĩa là khi `read` của `orders-db` thất bại, nguyên nhân chi tiết thường đã bị che đi trước khi lên tới ứng dụng (nhận xét của người soạn).

<details><summary>Đi sâu: CPU tìm thanh ghi thiết bị bằng cách nào</summary>

Có hai cách. Cách thứ nhất là lệnh I/O riêng; trên x86 đó là `in` và `out`, hai lệnh đặc quyền[^dev-inout]. Cách thứ hai là **memory-mapped I/O (MMIO)**: thanh ghi của thiết bị hiện ra như những ô nhớ, OS đọc hay ghi chúng bằng load/store thường, và phần cứng định tuyến các lệnh đó tới thiết bị thay vì tới RAM[^dev-mmio]. OSTEP kết luận không cách nào hơn hẳn cách nào[^dev-mmio2].

</details>

**Quay lại Q1:** thiết bị được điều khiển qua thanh ghi; polling hay interrupt quyết định cách chờ, DMA lo việc chép dữ liệu, và driver giấu khác biệt giữa các thiết bị. Với `orders-db`, nhờ vậy một lần `read` ngủ chờ thiết bị mà không chiếm CPU và chạy được trên bất kỳ loại ổ nào.

## 2. Ổ cứng quay (HDD) (Q2)

> **Trả lời ngắn.** Một lần I/O trên HDD gồm seek (dời arm tới đúng track), rotational delay (chờ sector quay tới dưới head) rồi transfer (truyền dữ liệu)[^hdd-time]. Với khối 4 KB, gần như toàn bộ thời gian là định vị chứ không phải truyền. Thời gian và tốc độ của một I/O tính bằng `T_I/O = T_seek + T_rotation + T_transfer` và `R_I/O = Size_transfer / T_I/O`[^hdd-formula]. Chênh lệch giữa I/O ngẫu nhiên và tuần tự là "gần 200 lần" đến "hơn 300 lần" tùy ổ[^hdd-gap].

**Giải thích.**

*Với `orders-db`.* Giả sử `orders-db` nằm trên một HDD. Mỗi `GET /orders/42` phải đọc một page 4 KB ở chỗ nào đó trên đĩa, còn việc ghi log của database thì nối đuôi liên tục (nhận xét của người soạn: đây là cách ta giả định database dùng đĩa). Hai kiểu truy cập ấy chênh nhau hàng trăm lần về tốc độ. Mục này tự tính con số đó từ cấu tạo của ổ.

### 2.1 Giao diện: một mảng sector

Với OS, ổ chỉ là một mảng sector 512 byte đánh số từ 0 đến n−1[^hdd-iface]. Nhà sản xuất chỉ bảo đảm đúng một điều: ghi **một sector 512 byte là atomic**[^hdd-atomic]. Nếu mất điện giữa một lần ghi lớn hơn thì chỉ một phần dữ liệu kịp xuống đĩa (*torn write*); chuyện này quay lại ở [chương 05](05-crash-consistency-va-journaling.md). Ngoài ra client còn ngầm giả định rằng truy cập liên tục (tuần tự) là chế độ nhanh nhất[^hdd-contract]. Giả định ngầm này chính là thứ mục 2.3 sẽ đo.

### 2.2 Hình học và ba thành phần của một I/O

Dữ liệu nằm trên các **platter** quay quanh một **spindle**, thường ở tốc độ 7.200 đến 15.000 RPM (vòng/phút)[^hdd-rpm0]. Trên mỗi mặt đĩa, dữ liệu xếp thành các vòng tròn đồng tâm gọi là **track**[^hdd-geo]. Mỗi mặt có một **head** gắn vào **arm**, và arm di chuyển để đặt head lên đúng track cần đọc[^hdd-head]. Một vòng quay của đĩa 10.000 RPM mất khoảng 6 ms (60.000 ms/phút chia 10.000)[^hdd-rpm].

Một lần I/O đi qua ba giai đoạn: **seek** (dời arm tới track), **rotational delay** (chờ sector quay tới dưới head), rồi **transfer**[^hdd-time]. Riêng seek cũng có nhiều pha: tăng tốc, chạy đều, giảm tốc, rồi *settling* để head ổn định trên track. Settling thường khá lớn, cỡ 0,5 đến 2 ms[^hdd-seek].

```mermaid
flowchart LR
    R[Yêu cầu sector S] --> SK[Seek] --> RO[Rotational delay] --> TR[Transfer]
```

Ổ còn có một cache nhỏ, thường khoảng 8 hoặc 16 MB[^hdd-cache]. Khi ghi, ổ có thể báo "xong" ngay khi dữ liệu mới vào bộ nhớ của ổ (*write back*), hoặc chỉ báo sau khi dữ liệu đã lên đĩa (*write through*)[^hdd-wb2]. Write back làm ổ trông nhanh hơn, nhưng nguy hiểm nếu file system cần dữ liệu xuống đĩa theo đúng thứ tự[^hdd-wb]. Với `orders-db`, đây là gốc của nỗi lo "database báo commit rồi mà dữ liệu chưa bền" (nhận xét của người soạn); lệnh flush ở [chương 06](06-durability-trong-thuc-te.md) sinh ra để xử lý chuyện này.

### 2.3 Tính I/O time và rate

Hai công thức ở đầu mục cho ta tính được cả thời gian lẫn tốc độ của một I/O[^hdd-formula]:

```
T_I/O = T_seek + T_rotation + T_transfer
R_I/O = Size_transfer / T_I/O
```

Ta áp vào hai ổ mà OSTEP dùng, lấy từ datasheet Seagate khoảng năm 2009: Cheetah 15K.5 (SCSI, 15.000 RPM, seek trung bình 4 ms, 125 MB/s) và Barracuda (SATA, 7.200 RPM, seek 9 ms, 105 MB/s)[^hdd-specs].

**Đọc ngẫu nhiên 4 KB trên Cheetah** (đúng kiểu `GET /orders/42` của `orders-db`). 15.000 RPM là 250 vòng/giây nên một vòng mất 4 ms, trung bình đợi nửa vòng, tức 2 ms[^hdd-cheetah]. Truyền 4 KB ở 125 MB/s mất khoảng 30 µs. Tổng cộng khoảng 6 ms, tức rate khoảng 0,66 MB/s[^hdd-6ms].

**Đọc ngẫu nhiên 4 KB trên Barracuda.** Một vòng mất 8,33 ms nên nửa vòng khoảng 4,17 ms; seek 9 ms; truyền khoảng 0,04 ms. Tổng khoảng 13,2 ms, rate khoảng 0,31 MB/s[^hdd-barra].

**Đọc tuần tự 100 MB** (chỉ một lần seek và quay, sau đó truyền liền một mạch, giống `orders-db` quét log). Cheetah mất khoảng 800 ms, Barracuda khoảng 950 ms, và rate gần bằng tốc độ truyền tối đa của ổ[^hdd-seq].

| | Cheetah (15K) | Barracuda (7,2K) |
|---|---|---|
| Random 4 KB, T_I/O | ≈ 6 ms | ≈ 13,2 ms |
| Random 4 KB, R_I/O | ≈ 0,66 MB/s | ≈ 0,31 MB/s |
| Sequential, R_I/O | ≈ 125 MB/s | ≈ 105 MB/s |

Với `orders-db`, hệ quả thực tế là khoảng cách giữa hai dòng random và sequential của bảng: một query cần một lần đọc ngẫu nhiên thì ổ chỉ phục vụ được vài trăm query mỗi giây (phép tính ở phần Đi sâu), còn ghi log liên tục thì ổ chạy gần hết tốc độ truyền.

<details><summary>Đi sâu: kiểm lại tỉ lệ và các số suy ra thêm</summary>

Sách gọi chênh lệch là "gần 200 lần" (Cheetah) và "hơn 300 lần" (Barracuda)[^hdd-gap]. Kiểm lại bằng phép chia các số đã làm tròn trong bảng: 125 / 0,66 ≈ 189 và 105 / 0,31 ≈ 339 (tính của người soạn). Số thập phân phụ thuộc cách làm tròn 4 KB (4.000 hay 4.096 byte) và thời gian truyền, nên chỉ tin bậc độ lớn, không tin chữ số cuối.

**Suy ra thêm (tính của người soạn, không phải số đo):**

- IOPS ngẫu nhiên xấp xỉ 1 / T_I/O: Cheetah ≈ 166, Barracuda ≈ 76.
- Với 4 KB, thời gian truyền chỉ khoảng 0,5% của 6 ms; 99,5% là định vị. Muốn hiệu suất 50% thì thời gian truyền phải bằng thời gian định vị, tức khối cỡ 125 MB/s × 6 ms ≈ 750 KB (Cheetah) hoặc 105 MB/s × 13,2 ms ≈ 1,4 MB (Barracuda). Đây là lý do định lượng cho lời khuyên của sách: truyền tuần tự, và nếu không thể thì theo khối lớn[^hdd-tip].

</details>

**Quay lại Q2:** đọc ngẫu nhiên 4 KB tốn cỡ 6–13 ms vì định vị, còn đọc tuần tự gần tốc độ truyền tối đa; nên truyền tuần tự, hoặc theo khối lớn. Với `orders-db`, đó là lý do log nối đuôi nhanh hơn hẳn việc đọc rải rác từng page.

## 3. Disk scheduling (Q3)

> **Trả lời ngắn.** SSTF chọn yêu cầu ở track gần nhất trước, nên có thể làm các track khác bị bỏ đói[^sched-starv]. Elevator (SCAN) quét qua lại theo thứ tự track nên không bị như vậy[^sched-scan]. SPTF tính cả thời gian quay[^sched-sptf]. Nhưng OS không biết ranh giới track và vị trí quay của head, nên SPTF thường chạy bên trong ổ[^sched-inside].

**Giải thích.**

*Với `orders-db`.* Nhiều thread của database cùng lúc gửi yêu cầu đọc, mỗi yêu cầu trúng một track khác nhau. Khi arm vừa phục vụ xong một yêu cầu thì hàng đợi đã có sẵn nhiều ứng viên. Chọn ai làm kế tiếp quyết định tổng quãng đường arm phải chạy, và cả việc có yêu cầu nào bị bỏ quên mãi hay không. Đó là bài toán của disk scheduling.

Bài toán này dễ hơn job scheduling thông thường. Ở job scheduling ta thường không biết một job chạy bao lâu, còn ở đĩa ta đoán khá tốt một yêu cầu sẽ tốn bao nhiêu thời gian. Vì vậy scheduler của đĩa áp dụng thẳng ý tưởng SJF (làm việc ngắn nhất trước)[^sched-sjf].

**SSTF** chọn yêu cầu ở track gần nhất làm trước[^sched-sstf]. Có hai vấn đề. Thứ nhất, OS không thấy hình học của ổ nên chỉ làm được nearest-block-first, tức chọn theo khoảng cách địa chỉ block[^sched-nbf]. Thứ hai là **starvation**: nếu yêu cầu cứ liên tục đổ vào track đang đứng, các yêu cầu ở track khác không bao giờ tới lượt[^sched-starv]. Trong `orders-db`, một nhóm query dồn vào vùng dữ liệu nóng có thể làm query vào vùng nguội chờ mãi (nhận xét của người soạn).

**Elevator (SCAN)** quét qua lại trên đĩa, phục vụ yêu cầu theo thứ tự track nên mọi track đều tới lượt[^sched-scan]. Cái giá là yêu cầu đến sau khi arm đã đi qua track của nó phải đợi lượt quét sau. **C-SCAN** là biến thể chỉ quét một chiều rồi quay về đầu[^sched-cscan].

SSTF lẫn SCAN còn một thiếu sót: cả hai bỏ qua *quay*[^sched-sptf]. **SPTF** (còn gọi SATF) sửa điều đó, vì nếu seek rẻ hơn quay thì nên seek xa hơn để tới sector sắp quay ngang head. Trên ổ hiện đại, seek và quay cỡ ngang nhau nên SPTF có ích[^sched-sptf2]. Nhưng OS không biết ranh giới track và vị trí quay của head, nên SPTF thường chạy *bên trong ổ*[^sched-inside]. OS vì thế chỉ chọn ra vài yêu cầu tốt nhất (sách lấy ví dụ 16) và gửi hết cho ổ, ổ tự sắp xếp[^sched-16].

Phần việc OS vẫn giữ lại là **I/O merging**, ví dụ gộp yêu cầu block 33 và 34 thành một yêu cầu hai block[^sched-merge]. OS còn quyết định có nên để ổ rảnh để đợi một yêu cầu tốt hơn hay không. Scheduler không bao giờ để ổ rảnh khi còn yêu cầu gọi là *work-conserving*[^sched-wc]; ngược lại là non-work-conserving. Mục 5 sẽ cho thấy các ý này sống tiếp trong scheduler của Linux.

<details><summary>Đi sâu: ví dụ tính lại FIFO, SSTF, elevator</summary>

**Ví dụ tính lại (số của người soạn, đơn vị = track).** Đầu ở track 50; hàng đợi theo thứ tự đến: 10, 55, 12, 60, 11.

| Thuật toán | Thứ tự | Tổng seek |
|---|---|---|
| FIFO | 10, 55, 12, 60, 11 | 40+45+43+48+49 = 225 |
| SSTF | 55, 60, 12, 11, 10 | 5+5+48+1+1 = 60 |
| Elevator, lên trước | 55, 60, rồi 12, 11, 10 | 5+5+48+1+1 = 60 |
| Elevator, xuống trước | 12, 11, 10, rồi 55, 60 | 38+1+1+45+5 = 90 |

Bảng tính elevator theo kiểu "đi tới yêu cầu xa nhất rồi đảo chiều" (không chạy tới mép đĩa như SCAN nguyên thủy). SSTF và elevator-lên-trước cùng 60, tốt hơn FIFO nhiều. Khác biệt chỉ lộ khi yêu cầu mới liên tục đến gần track 50: SSTF thuần túy không bao giờ tới 10–12, còn elevator chắc chắn tới trong một sweep.

</details>

**Quay lại Q3:** SSTF nhanh nhưng bỏ đói, elevator công bằng hơn, SPTF chính xác nhất nhưng chỉ ổ mới có đủ thông tin để chạy nó. Với `orders-db` trên HDD, OS gộp và lọc, ổ tự quyết thứ tự cuối cùng.

## 4. SSD và flash (Q4)

> **Trả lời ngắn.** Flash chỉ ghi được vào page đã erase, mà erase phải làm trên cả một block lớn hơn, và ghi quá nhiều vào một chỗ thì chỗ đó mòn[^ssd-erase]. FTL che giấu ràng buộc này: nó nhận đọc/ghi trên khối logic rồi biến thành read/erase/program trên block/page vật lý[^ssd-ftl]. FTL thường làm theo kiểu log-structured, tức append rồi cập nhật bảng ánh xạ[^ssd-log]. Cái giá là garbage collection, wear leveling và write amplification[^ssd-wa]. TRIM cho thiết bị biết khối nào đã bỏ[^ssd-trim].

**Giải thích.**

*Với `orders-db`.* Giả sử `orders-db` chuyển sang SSD. Khi một đơn hàng đổi trạng thái, database ghi đè đúng page 4 KB cũ ở cùng địa chỉ logic. Với đĩa quay, ghi đè một sector là chuyện bình thường. Flash thì không ghi đè tại chỗ được, nên bên trong SSD phải làm khá nhiều việc để giữ cho giao diện "ghi đè địa chỉ này" vẫn chạy đúng. Mục này đi từ ràng buộc của chip flash đến những việc ấy.

### 4.1 Ràng buộc của NAND flash

Muốn ghi vào một **page**, trước hết phải **erase** cả một **block** lớn hơn, và erase khá đắt. Thêm nữa, ghi quá nhiều vào một chỗ thì chỗ đó mòn[^ssd-erase]. Đừng nhầm hai chữ này với thứ cùng tên ở nơi khác: "block" và "page" của flash không phải block của đĩa hay page của bộ nhớ ảo[^ssd-term]. Một block (còn gọi erase block) thường 128 KB hoặc 256 KB, còn page chỉ vài KB, ví dụ 4 KB[^ssd-sizes].

Chip có ba thao tác. **Read** đọc một page; **erase** và **program** đi cùng nhau để ghi[^ssd-ops]. Erase phá nội dung cả block (đặt mọi bit về 1), nên dữ liệu còn cần giữ phải được chép đi trước[^ssd-erase2]. Còn page đã program rồi thì cách duy nhất để đổi nội dung là erase cả block chứa nó[^ssd-program]. Hệ quả cho `orders-db`: ghi đè page 4 KB thật ra không thể là "sửa 4 KB tại chỗ" như trên đĩa quay (suy luận của người soạn từ các ràng buộc trên).

```mermaid
stateDiagram-v2
    [*] --> INVALID
    INVALID --> ERASED: erase(block)
    ERASED --> VALID: program(page)
    VALID --> ERASED: erase(block)
```

### 4.2 Hiệu năng và độ bền (có năm)

Hình 44.2 của OSTEP tóm tắt độ trễ thô của từng loại chip, bằng số lấy từ "báo chí chuyên ngành" mà sách ghi là tài liệu [V12][^ssd-fig442]. Bảng dưới chép lại các số đó[^ssd-fig442b]. Mục tham khảo ghi [V12] là bài của Kristian Vatto đăng trên AnandTech tháng 9/2012[^ssd-v12]. Vậy "2012" là ngày của bài báo mà sách trích số liệu, không phải ngày đo của sách.

| Loại | Read | Program | Erase |
|---|---|---|---|
| SLC | 25 µs | 200–300 µs | 1500–2000 µs |
| MLC | 50 µs | 600–900 µs | ≈ 3000 µs |
| TLC | ≈ 75 µs | ≈ 900–1350 µs | ≈ 4500 µs |

Sách tóm lại: erase tốn "vài ms"[^ssd-lat]. Đây là số của chip đời 2012, **không** phải độ trễ của một SSD hôm nay. Người soạn không tìm được nguồn ổn định trong phạm vi chương cho độ trễ NVMe hiện đại nên không nêu; hãy đo trên máy đích.

Về độ bền, nhà sản xuất định mức block MLC khoảng 10.000 chu kỳ P/E (program/erase), và nghiên cứu cho thấy tuổi thọ thực tế dài hơn kỳ vọng[^ssd-wear]. Ngoài ra còn *disturbance*: truy cập một page có thể làm lật bit ở page lân cận[^ssd-disturb].

### 4.3 FTL: biến flash thành "ổ đĩa"

**Flash Translation Layer (FTL)** là lớp phần mềm trong SSD, nhận đọc/ghi trên khối logic rồi biến thành read/erase/program trên block/page vật lý[^ssd-ftl]. Nó nhắm hai mục tiêu. Thứ nhất là giảm **write amplification**, tức tổng byte FTL ghi vào chip chia cho tổng byte client ghi[^ssd-wa]. Thứ hai là **wear leveling**: rải việc ghi đều để mọi block mòn gần cùng lúc[^ssd-wl].

Cách ngây thơ nhất là **FTL ánh xạ trực tiếp**: logical page nào nằm đúng page vật lý đó. Mỗi lần ghi một logical page, FTL phải đọc cả block, erase, rồi program lại tất cả. Write amplification khi đó tỉ lệ với số page trong block, và ghi chậm hơn cả HDD[^ssd-direct]. Với block 256 KB, page 4 KB, mỗi lần `orders-db` ghi đè 4 KB sẽ làm flash program 64 page, tức WA ≈ 64 (tính của người soạn).

Vì thế FTL phổ biến là **log-structured**. Mỗi lần ghi, thiết bị append dữ liệu vào chỗ trống kế tiếp rồi cập nhật **bảng ánh xạ** logical → physical. Bảng này giữ trong bộ nhớ của thiết bị và cũng được lưu bền ở đâu đó trên thiết bị[^ssd-log]. Nhờ vậy erase chỉ cần làm lúc này lúc khác, không còn read-modify-write đắt đỏ của cách trực tiếp[^ssd-log2]. Với `orders-db`, ghi đè page 4 KB giờ chỉ là append một page mới và đổi một mục trong bảng ánh xạ; page cũ nằm lại đó thành rác.

### 4.4 Garbage collection, TRIM, over-provisioning

Rác chính là hệ quả của cách làm trên: ghi đè sinh ra *garbage*, vì bản cũ vẫn chiếm page. Thiết bị phải định kỳ **garbage collect (GC)** để dọn, và GC quá nhiều thì đẩy write amplification lên[^ssd-oob2]. Một lượt GC gồm bốn bước: chọn block có page rác; đọc các page còn sống của block đó; ghi chúng vào cuối log; rồi erase cả block[^ssd-gc]. Block toàn page chết là ứng viên lý tưởng, vì erase ngay mà không cần di chuyển gì[^ssd-gc2].

Ví dụ của sách: ghi logical 100, 101, 2000, 2001 vào block 0, rồi ghi đè 100, 101 vào block 1. Lúc này block 0 có 2 page rác và 2 page sống, nên GC chép 2 page sống ra cuối log rồi erase block 0[^ssd-gcex]. Tính write amplification của kịch bản này (tính của người soạn): client ghi 6 page, flash program 6 + 2 = 8 page, nên WA ≈ 1,33. Tổng quát hơn (tính của người soạn): với block 4 page mà 3 page còn sống, GC phải chép 3 page để thu về đúng 1 page trống, nên block càng nhiều page sống thì GC càng đắt.

**TRIM** là lệnh báo cho thiết bị biết các block đã bị xóa, để nó khỏi phải giữ thông tin về chúng nữa[^ssd-trim]. Không có TRIM thì SSD không biết đơn hàng cũ mà `orders-db` đã xóa là rác, và GC có thể vẫn chép chúng đi (suy luận của người soạn). Phía Linux gọi khả năng tương ứng là *discard*: thuộc tính sysfs `queue/discard_granularity` bằng 0 nghĩa là thiết bị không hỗ trợ discard[^sys-discard]. Tài liệu kernel không dùng chữ "TRIM" ở mục này, nên việc coi discard là cách Linux nói TRIM là suy luận của người soạn. Cuối cùng, **over-provisioning** (thêm dung lượng flash ẩn) giúp trì hoãn GC[^ssd-op].

<details><summary>Đi sâu: bảng ánh xạ, wear leveling và so sánh với HDD</summary>

**Bảng ánh xạ.** Với SSD 1 TB, bảng ánh xạ cấp page cần một mục 4 byte cho mỗi page 4 KB, tức 1 GB bộ nhớ chỉ để chứa bảng[^ssd-map] (2⁴⁰/2¹² = 2²⁸ mục × 4 B = 1 GiB). Ánh xạ theo block thì rẻ hơn nhiều, nhưng ghi nhỏ buộc phải chép nhiều dữ liệu sống nên WA tăng mạnh[^ssd-blockmap]. OSTEP nêu hai cách giảm bộ nhớ. Cách thứ nhất là *hybrid mapping*: giữ ít block log ánh xạ theo page, phần còn lại ánh xạ theo block[^ssd-hybrid]. Cách thứ hai là chỉ cache phần bảng đang dùng[^ssd-cache].

**Wear leveling.** Nếu một block chứa dữ liệu sống lâu, không bao giờ bị ghi đè, thì GC sẽ không thu hồi nó, và nó không nhận phần mòn công bằng của mình[^ssd-wl3]. FTL vì vậy phải định kỳ chuyển dữ liệu đó đi chỗ khác, và việc này làm write amplification tăng thêm[^ssd-wl2].

**So sánh với HDD.** Bảng dưới là Bảng 44.4 của OSTEP, đơn vị MB/s, số liệu khớp nguyên văn bảng gốc[^ssd-fig444]. Sách ghi số liệu "lấy từ vài nguồn trực tuyến" [S13, T15][^ssd-src]. Mục tham khảo cho biết đó là bài AnandTech ngày 7/5/2013[^ssd-s13] và biểu đồ Tom's Hardware tháng 1/2015[^ssd-t15].

| Thiết bị | Rand read | Rand write | Seq read | Seq write |
|---|---|---|---|---|
| Samsung 840 Pro SSD | 103 | 287 | 421 | 384 |
| Seagate 600 SSD | 84 | 252 | 424 | 374 |
| Intel SSD 335 SSD | 39 | 222 | 344 | 354 |
| Seagate Savvio 15K.3 HDD | 2 | 2 | 223 | 223 |

Khác biệt lớn nhất nằm ở I/O ngẫu nhiên; ở tuần tự, HDD vẫn là lựa chọn tốt nếu chỉ cần tuần tự[^ssd-cmp]. Ghi ngẫu nhiên nhanh hơn đọc ngẫu nhiên vì thiết kế log-structured biến ghi ngẫu nhiên thành tuần tự[^ssd-rw]. Đây là số của các thiết bị đời 2013–2015, không phải số NVMe hôm nay.

</details>

**Quay lại Q4:** flash không ghi đè tại chỗ nên FTL phải ánh xạ lại và dọn rác, kéo theo write amplification và wear leveling. Với `orders-db`, mỗi lần ghi đè một page 4 KB thực chất là một lần append cộng với một khoản GC trả sau.

## 5. NVMe và block layer multi-queue của Linux (Q5)

> **Trả lời ngắn.** NVMe thường gắn trên PCIe[^dev-pcie]. blk-mq có nhiều hàng đợi, mỗi hàng có điểm vào cục bộ theo CPU, nên không cần một lock chung[^mq-intro]. Request đi đường ngắn nhất tới hardware queue, và chỉ qua software queue khi có scheduler hoặc cần merge[^mq-short]. Scheduler chọn được lúc chạy. `none` không sắp xếp lại[^mq-none]. `mq-deadline` gom batch theo sector kèm deadline[^dl-batch]. `bfq` chia băng thông theo tỉ lệ[^bfq-prop].

**Giải thích.**

*Với `orders-db`.* Giả sử máy chạy `orders-db` có nhiều core và một SSD NVMe. Vài chục thread xử lý query cùng lúc gửi I/O xuống một thiết bị duy nhất. Nếu mọi thread phải xếp vào một hàng đợi, giữ bằng một lock chung, thì lock đó trở thành điểm nghẽn trước khi thiết bị kịp nhanh (suy luận của người soạn từ lý do ở mục 5.1). Mục này xem Linux và NVMe tránh điều đó thế nào.

NVMe là giao thức cho thiết bị lưu trữ hiệu năng cao, thường gắn trên PCIe[^dev-pcie]. Mỗi thiết bị có ít nhất hai queue: một cho lệnh admin, một cho lệnh I/O[^nvme-q]. Driver Linux yêu cầu số queue I/O tối đa cỡ một queue mỗi CPU (cộng thêm queue ghi riêng và queue polled nếu cấu hình)[^nvme-max]. Chi tiết driver ở phần Đi sâu.

### 5.1 Vì sao cần multi-queue

Block layer cũ được tối ưu cho HDD, nơi các bộ phận cơ khí là nút cổ chai[^mq-bg]. Với SSD và bộ nhớ không bay hơi, nút cổ chai chuyển sang chính OS: một hàng đợi với một lock không scale trên máy SMP[^mq-bg2]. **blk-mq** giải quyết bằng cách có nhiều hàng đợi, mỗi hàng có điểm vào cục bộ theo CPU, nên bỏ được lock chung[^mq-intro].

### 5.2 Hai tầng queue

blk-mq có hai nhóm hàng đợi: **software staging queue** và **hardware dispatch queue**[^mq-two]. Một request mới được gửi theo đường ngắn nhất, thẳng tới hardware queue. Chỉ khi có I/O scheduler gắn vào hoặc cần merge thì nó mới đi qua software queue[^mq-short].

```mermaid
flowchart LR
    BIO[bio từ file system] --> RQ[struct request]
    RQ -->|không scheduler, không merge| HW[Hardware dispatch queue]
    RQ -->|có scheduler hoặc merge| SW[Software staging queue]
    SW --> HW
    HW --> DEV[Driver / thiết bị]
    DEV -->|tag| DONE[Hoàn tất]
```

- Software queue (theo CPU hoặc node, lock riêng) có thể gộp request cho sector kề nhau, ví dụ 3–6, 6–7, 7–9 thành 3–9[^mq-sw].
- Hardware queue ánh xạ tới submission queue của thiết bị; số lượng không vượt số core[^mq-hw].
- Mỗi request có một **tag**. Khi xong, driver trả lại đúng tag đó nên block layer không phải tìm tuyến tính[^mq-tag].
- Block layer lẫn giao thức thiết bị đều **không bảo đảm thứ tự hoàn tất** của các request; tầng trên, tức file system, phải tự xử lý[^mq-order]. Vì vậy file system cần cơ chế riêng để ép thứ tự, như barrier hoặc flush (suy luận của người soạn; xem [chương 05](05-crash-consistency-va-journaling.md)). Với `orders-db`: hai `write` gửi liên tiếp có thể xong theo thứ tự ngược lại.

<details><summary>Đi sâu: driver NVMe và các I/O scheduler none, mq-deadline, bfq</summary>

**Driver NVMe (`drivers/nvme/host/pci.c`, tag v6.8: chi tiết cài đặt, có thể đổi).** Hàm `nvme_write_sq_db` ghi `sq_tail` vào thanh ghi doorbell `q_db` của queue[^nvme-doorbell]. Đây là một ví dụ của MMIO ở mục 1. Lệnh `writel` đó nằm sau điều kiện `nvme_dbbuf_update_and_check_event`, tức khi thiết bị dùng shadow doorbell buffer thì không phải lần cập nhật nào cũng chạm thanh ghi thật. Việc các lệnh nằm trong *submission queue* ở RAM là cách NVMe hoạt động nói chung (suy luận của người soạn; chương không trích đặc tả NVMe vì không tải được bản đặc tả).

Về số queue: driver *yêu cầu* tối đa `num_possible_cpus() + nr_write_queues + nr_poll_queues` queue I/O[^nvme-max]. Số queue thực tế còn phụ thuộc số queue thiết bị cấp phát (`nvme_set_queue_count`) và số vector interrupt, và có một quirk của Apple ép về 1 (đã đối chiếu với `nvme_max_io_queues` và `nvme_setup_io_queues` ở v6.8). Driver cần interrupt cho queue admin và cho mỗi queue I/O không polled[^nvme-irq]. Với queue polled có tham số module `poll_queues`[^nvme-poll]. Block layer cũng có loại hardware queue tương ứng là `HCTX_TYPE_POLL`[^mq-poll].

**I/O scheduler.** Các trích dẫn từ docs.kernel.org dưới đây là bản tài liệu đang phát hành, không ghim phiên bản; đã đối chiếu từng câu trích nguyên văn với `Documentation/block/*.rst` ở tag v6.8 và chúng có mặt ở đó. Scheduler cắm được, chọn lúc chạy, và việc sắp xếp chỉ xảy ra giữa các request trong cùng một queue[^mq-sched]. Đổi scheduler bằng `echo TÊN > /sys/block/DEV/queue/scheduler`; khi đọc file này, scheduler đang chạy nằm trong ngoặc vuông[^sw-sched]. Docs liệt kê `mq-deadline`, `none`, `bfq`, `kyber`[^sw-list].

**`none`** đặt request vào software queue của CPU đang chạy mà không sắp xếp lại[^mq-none]. Về nguồn: các trang `blk-mq`, `switching-sched`, `deadline-iosched`, `bfq-iosched` (và bản .rst v6.8) không có câu nào nói `none` hợp với NVMe, nên lý do "hợp với NVMe" dưới đây là suy luận. Điều có nguồn là *mặc định* của kernel v6.8: thiết bị một hardware queue được gán `mq-deadline`, thiết bị nhiều queue (hoặc không có `mq-deadline`) mặc định `none`[^elv-default]. Suy luận của người soạn: `none` hợp với thiết bị nhiều queue song song như NVMe vì thiết bị tự sắp xếp lại bên trong (như SPTF ở mục 3), còn sắp xếp ở host phải qua lock chung mà blk-mq cố tránh (xem overhead mỗi request ở `bfq` bên dưới).

**`mq-deadline`** cố bảo đảm thời điểm *bắt đầu phục vụ* của một request: mỗi request vào scheduler nhận deadline bằng hiện tại cộng `read_expire`[^dl-goal]. Các request được gom thành batch cùng hướng (read hoặc write), phục vụ theo sector tăng dần, gần với elevator ở mục 3. Deadline chỉ được kiểm tra giữa các batch, và `fifo_batch` bằng 1 thì thành FIFO[^dl-batch]. Read được ưu tiên, nhưng `writes_starved` giới hạn để write không bị bỏ đói vô hạn[^dl-pref].

**`bfq`** là scheduler proportional-share có thêm khả năng giữ độ trễ thấp[^bfq-prop]. Cấu hình mặc định ưu tiên độ trễ hơn throughput[^bfq-lat]. Về chi phí, tài liệu ghi mq-deadline tốn 0,7 µs mỗi request, BFQ 1,9 µs, đo trên một Core i7-2760QM đã cũ[^bfq-over]. Scheduler phức tạp hơn thì overhead mỗi request lớn hơn (nhận xét của người soạn).

</details>

**Quay lại Q5:** NVMe có queue riêng ở RAM và doorbell qua MMIO; blk-mq nhân hàng đợi theo CPU, và scheduler (`none`, `mq-deadline`, `bfq`) chỉ sắp xếp giữa các request trong cùng một queue. Nhờ vậy các thread của `orders-db` trên nhiều core không giành nhau một lock chung khi gửi I/O.

## 6. Sector 512 vs 4 KB; atomic write unit (Q6)

> **Trả lời ngắn.** Kernel báo hai kích thước. `logical_block_size` là đơn vị nhỏ nhất mà thiết bị địa chỉ hóa được, thường 512 byte[^sys-lbs]. `physical_block_size` là đơn vị nhỏ nhất mà thiết bị vật lý ghi atomic, thường bằng logical nhưng có thể lớn hơn; ví dụ sector SATA 4 KB hiển thị logical 512[^sys-pbs]. "Atomic" ở đây chỉ nói về đơn vị vật lý, không phải bảo đảm cho cả một lần `write()` của ứng dụng (suy luận của người soạn).

**Giải thích.**

*Với `orders-db`.* Database ghi page 4 KB hoặc ghi một record vài trăm byte vào log. Nếu ổ nhận địa chỉ theo 512 byte nhưng bên trong ghi theo 4 KB, thì hai kiểu ghi đó có thể tốn công rất khác nhau. Kernel cho `orders-db` và file system biết điều này qua hai file trong sysfs:

| sysfs | Ý nghĩa theo tài liệu kernel |
|---|---|
| `queue/logical_block_size` | đơn vị nhỏ nhất thiết bị địa chỉ hóa được, thường 512 byte[^sys-lbs] |
| `queue/physical_block_size` | đơn vị nhỏ nhất thiết bị vật lý ghi *atomic*; thường bằng logical nhưng có thể lớn hơn, ví dụ SATA sector 4 KB hiển thị logical 512[^sys-pbs] |
| `queue/rotational` | cờ rotational / non-rotational[^sys-rot] |

Suy luận của người soạn: nếu ổ báo logical 512 và physical 4096, ghi nhỏ hơn 4096 hoặc lệch 4096 có thể buộc ổ đọc-sửa-ghi cả sector vật lý, nên file system và partition nên căn theo `physical_block_size`. Ghi lớn hơn một đơn vị atomic thì có thể dở dang (*torn write*, mục 2.1); hệ quả cho file system ở [chương 05](05-crash-consistency-va-journaling.md).

<details><summary>Đi sâu: atomic write không có trên kernel 6.8 của máy lab</summary>

**Atomic write không có trên kernel 6.8 của máy lab.** Thuộc tính sysfs `atomic_write_unit_max_bytes` (cùng `atomic_write_unit_min_bytes`, `atomic_write_max_bytes`) không có trong `Documentation/ABI/stable/sysfs-block` ở tag v6.8, v6.9, v6.10; mục này xuất hiện ở v6.11 (đã kiểm bằng grep trên từng tag; `February 2024` là trường `Date:` của mục ABI, không phải ngày phát hành kernel). Theo mô tả ở v6.11, `atomic_write_unit_max_bytes` là khối lớn nhất có thể ghi atomic bằng thao tác atomic write[^sys-atomic]. Đã kiểm trên VM: kernel 6.8.0-117-generic, `/sys/block/vda/` và `/sys/block/vda/queue/` không có thuộc tính nào tên `atomic_write*`. Chương không khẳng định ổ nào hỗ trợ.

</details>

**Quay lại Q6:** `logical_block_size` là đơn vị địa chỉ, `physical_block_size` là đơn vị ghi atomic của thiết bị; hãy căn dữ liệu theo cái sau.

## 7. Quan sát trên một VM

Máy người soạn là macOS, nên Linux ở đây là **VM Colima** (kernel 6.8.0-117-generic) chạy qua `./labs/linux.sh`. Một vòng lặp trong shell đọc `/sys/block/*/queue/{scheduler,rotational,logical_block_size,physical_block_size}`, kèm `lsblk`, cho kết quả:

```text
loop0 sched=[none] mq-deadline  rot=1 lbs=512 pbs=512
vda   sched=none [mq-deadline]  rot=1 lbs=512 pbs=512
vdb   sched=none [mq-deadline]  rot=1 lbs=512 pbs=512
vdc   sched=none [mq-deadline]  rot=1 lbs=512 pbs=512
vda 20G disk; vdb 40G disk (mount /usr/local/cargo/registry); vdc 18.9M RO
```

Đây là quan sát một lần chạy trên VM này, không suy rộng:

- Đĩa ảo `vda`–`vdc` dùng `mq-deadline`; `loop*` dùng `none`. Danh sách chỉ có `none` và `mq-deadline`, không thấy `bfq` hay `kyber` (có lẽ không được build hoặc nạp vào kernel này; suy luận).
- `rotational = 1` trên đĩa ảo. Giá trị này do thiết bị ảo tự khai báo, không chứng minh có đĩa quay; người soạn không kiểm tra phần cứng phía host[^sys-rot].
- Logical và physical đều 512 byte.

Cùng thông tin này đọc bằng Rust (chạy trong container, `rustc 1.95.0`): `std::fs::read_dir("/sys/block")` rồi `std::fs::read_to_string`, hàm đọc toàn bộ nội dung file vào một chuỗi[^rust-rts]. Chương trình in đúng `vda: scheduler=none [mq-deadline] logical=512 physical=512` (và tương tự cho `vdb`, `vdc`). Thông tin thiết bị khối không có API riêng trong `std`; nó chỉ là file văn bản dưới `/sys`. `std::fs` và `File::sync_all` nằm ở [chương 08](08-rust-va-luu-tru.md).

Kết nối với backend (nhận xét của người soạn): "disk" của `orders-db` trong production thường là thiết bị ảo, nên cờ `rotational` và scheduler có thể không phản ánh phần cứng thật. Hãy đo trên đúng môi trường production. Cache trong ổ (mục 2) là một nguồn của tình huống "đã báo xong nhưng chưa bền"; xem [chương 06](06-durability-trong-thuc-te.md).

## Tóm tắt

- Thiết bị nói chuyện qua thanh ghi (lệnh I/O hoặc MMIO). Polling tốn CPU; interrupt cho phép CPU và thiết bị chạy chồng lên nhau nhưng không luôn tốt hơn với thiết bị nhanh; DMA bỏ việc CPU chép dữ liệu; driver che chi tiết thiết bị dưới block layer.
- HDD: `T_I/O = T_seek + T_rotation + T_transfer`. Đọc ngẫu nhiên 4 KB mất khoảng 6 ms (0,66 MB/s) trên Cheetah và khoảng 13,2 ms (0,31 MB/s) trên Barracuda; đọc tuần tự gần tốc độ truyền tối đa.
- SSTF gây starvation, elevator tránh được, SPTF tính cả quay và nằm trong ổ; OS vẫn merge và gửi nhiều yêu cầu cùng lúc.
- Flash chỉ ghi vào page đã erase, erase theo block, và block có tuổi thọ P/E hữu hạn.
- FTL log-structured cùng bảng ánh xạ tạo ra block device bình thường, với cái giá là GC, wear leveling, write amplification và bảng ánh xạ lớn; TRIM cho biết khối nào đã chết.
- blk-mq có software staging queue và hardware dispatch queue, request gắn tag, và không bảo đảm thứ tự hoàn tất.
- `none` không sắp xếp; `mq-deadline` batch theo sector kèm deadline; `bfq` chia băng thông theo tỉ lệ và thiên về latency. Kích thước sector kernel báo qua `logical_block_size` và `physical_block_size`.

## Câu hỏi tự kiểm tra

1. Vì sao polling có thể tốt hơn interrupt với thiết bị rất nhanh? Một lý do khác để dùng polling?
<details><summary>Đáp án</summary>

Lần poll đầu thường đã thấy việc xong, còn interrupt kéo theo chuyển tiến trình đắt hơn[^dev-notalways]. Lý do khác là tránh livelock khi interrupt dồn dập[^dev-livelock]. Linux NVMe cũng có queue polled riêng[^nvme-poll].

</details>

2. DMA khác PIO ở điểm nào, và ai báo OS khi DMA xong?
<details><summary>Đáp án</summary>

Với PIO, CPU tự chép dữ liệu[^dev-pio]. Với DMA, một engine chép giữa RAM và thiết bị, xong thì phát interrupt[^dev-dma].

</details>

3. Tính T_I/O và R_I/O cho đọc ngẫu nhiên 4 KB trên ổ 7.200 RPM, seek 9 ms, 105 MB/s.
<details><summary>Đáp án</summary>

Quay trung bình ≈ 4,17 ms; seek 9 ms; transfer ≈ 0,04 ms. Tổng ≈ 13,2 ms, rate ≈ 0,31 MB/s[^hdd-barra].

</details>

4. Cùng ổ đó, đọc tuần tự 100 MB mất bao lâu?
<details><summary>Đáp án</summary>

Khoảng 950 ms, rate gần 105 MB/s[^hdd-seq].

</details>

5. Vì sao SSTF có thể bỏ đói yêu cầu còn elevator thì không?
<details><summary>Đáp án</summary>

SSTF luôn chọn track gần nhất, nên dòng yêu cầu ở track hiện tại chặn mọi yêu cầu ở xa[^sched-starv]. Elevator thì quét theo sweep, và yêu cầu bị lỡ chỉ phải chờ sweep kế tiếp[^sched-scan].

</details>

6. Vì sao SPTF thường chạy trong ổ?
<details><summary>Đáp án</summary>

Vì OS không biết ranh giới track và vị trí quay của head[^sched-inside].

</details>

7. Vì sao không ghi đè page flash tại chỗ, và FTL log-structured giải quyết thế nào?
<details><summary>Đáp án</summary>

Page đã program chỉ đổi được bằng cách erase cả block[^ssd-program]. FTL log-structured thì append vào chỗ trống kế tiếp và cập nhật bảng ánh xạ[^ssd-log]. Bản cũ thành garbage chờ GC[^ssd-oob2].

</details>

8. Write amplification là gì và cơ chế nào làm nó tăng?
<details><summary>Đáp án</summary>

Đó là tổng byte FTL ghi vào flash chia cho tổng byte client ghi[^ssd-wa]. GC làm nó tăng vì phải ghi lại các page sống[^ssd-oob2]. Wear leveling cũng làm nó tăng vì phải chuyển dữ liệu đi chỗ khác[^ssd-wl2].

</details>

9. Khi nào request đi qua software staging queue trong blk-mq?
<details><summary>Đáp án</summary>

Chỉ khi có I/O scheduler hoặc cần merge; còn lại đi thẳng tới hardware queue[^mq-short].

</details>

10. `none`, `mq-deadline`, `bfq` khác nhau thế nào?
<details><summary>Đáp án</summary>

`none` không sắp xếp lại[^mq-none]. `mq-deadline` gom batch theo sector, có deadline[^dl-batch]. Nó cũng ưu tiên read[^dl-pref]. `bfq` chia băng thông theo tỉ lệ[^bfq-prop]. Nó cũng ưu tiên latency[^bfq-lat].

</details>

## Đọc thêm

- OSTEP, chương 36 *I/O Devices* — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — mục 36.8 có driver IDE của xv6 để thấy polling/interrupt/PIO bằng code thật.
- OSTEP, chương 37 *Hard Disk Drives* — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — bài tập mô phỏng `disk.py` để tự thử seek, rotation, SSTF, SATF.
- OSTEP, chương 44 *Flash-based SSDs* — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — mục 44.7–44.9 về FTL, GC, hybrid mapping.
- Kernel docs, *blk-mq* — <https://docs.kernel.org/block/blk-mq.html> — API driver và cấu trúc hàng đợi.
- Kernel docs, *Deadline IO scheduler tunables* — <https://docs.kernel.org/block/deadline-iosched.html> — `read_expire`, `fifo_batch`, `writes_starved`.
- Chương tiếp theo: [Chương 02 — File và directory API](02-file-va-directory-api.md).

[^hdd-formula]
[^dev-protocol]: OSTEP, chương 36, mục 36.3 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — "The protocol has four steps."
[^dev-poll]: OSTEP, chương 36, mục 36.3 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — "the OS waits until the device is ready to receive a command by repeatedly reading the status register; we call this polling the device"
[^dev-pio]: OSTEP, chương 36, mục 36.3 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — "When the main CPU is involved with the data movement (as in this example protocol), we refer to it as programmed I/O (PIO)."
[^dev-irq]: OSTEP, chương 36, mục 36.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — "the OS can issue a request, put the calling process to sleep, and context switch to another task."
[^dev-irq2]: OSTEP, chương 36, mục 36.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — "Interrupts thus allow for overlap of computation and I/O, which is key for improved utilization."
[^dev-notalways]: OSTEP, chương 36, mục 36.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — "the first poll usually finds the device to be done with task. Using an interrupt in this case will actually slow down the system: switching to another process, handling the interrupt, and switching back to the issuing process is expensive."
[^dev-hybrid]: OSTEP, chương 36, mục 36.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — "it may be best to use a hybrid that polls for a little while and then"
[^dev-livelock]: OSTEP, chương 36, mục 36.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — "When a huge stream of incoming packets each generate an interrupt, it is possible for the OS to livelock"
[^dev-coalesce]: OSTEP, chương 36, mục 36.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — "While waiting, other requests may soon complete, and thus multiple interrupts can be coalesced into a single interrupt delivery"
[^dev-dma]: OSTEP, chương 36, mục 36.5 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — "the OS would program the DMA engine by telling it where the data lives in memory, how much data to copy, and which device to send it to."
[^dev-inout]: OSTEP, chương 36, mục 36.6 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — "on x86, the in and out instructions can be used to communicate with devices."
[^dev-mmio]: OSTEP, chương 36, mục 36.6 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — "To access a particular register, the OS issues a load (to read) or store (to write) the address; the hardware then routes the load/store to the device instead of main memory."
[^dev-mmio2]: OSTEP, chương 36, mục 36.6 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — "There is not some great advantage to one approach or the other."
[^dev-driver]: OSTEP, chương 36, mục 36.7 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — "At the lowest level, a piece of software in the OS must know in detail how a device works. We call this piece of software a device driver"
[^dev-stack]: OSTEP, chương 36, mục 36.7 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — "it simply issues block read and write requests to the generic block layer, which routes them to the appropriate device driver"
[^dev-eio]: OSTEP, chương 36, mục 36.7 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — "in Linux with SCSI devices, which have very rich error reporting; because other block devices (e.g., ATA/IDE) have much simpler error handling, all that higher levels of software ever receive is a generic EIO (generic IO error) error code"
[^hdd-iface]: OSTEP, chương 37 Hard Disk Drives, mục 37.1 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "The drive consists of a large number of sectors (512-byte blocks), each of which can be read or written."
[^hdd-atomic]: OSTEP, chương 37, mục 37.1 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "the only guarantee drive manufacturers make is that a single 512-byte write is atomic"
[^hdd-contract]: OSTEP, chương 37, mục 37.1 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "One can also usually assume that accessing blocks in a contiguous chunk (i.e., a sequential read or write) is the fastest access mode"
[^hdd-rpm0]: OSTEP, chương 37, mục 37.2 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "The platters are all bound together around the spindle, which is connected to a motor that spins the platters around (while the drive is powered on) at a constant (fixed) rate. The rate of rotation is often measured in rotations per minute (RPM), and typical modern values are in the 7,200 RPM to 15,000 RPM range."
[^hdd-geo]: OSTEP, chương 37, mục 37.2 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "Data is encoded on each surface in concentric circles of sectors; we call one such concentric circle a track."
[^hdd-head]: OSTEP, chương 37, mục 37.2 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "there is one such head per surface of the drive. The disk head is attached to a single disk arm, which moves across the surface to position the head over the desired track."
[^hdd-rpm]: OSTEP, chương 37, mục 37.2 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "a drive that rotates at 10,000 RPM means that a single rotation takes about 6 milliseconds (6 ms)."
[^hdd-time]: OSTEP, chương 37, mục 37.3 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "first a seek, then waiting for the rotational delay, and finally the transfer."
[^hdd-seek]: OSTEP, chương 37, mục 37.3 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "The settling time is often quite significant, e.g., 0.5 to 2 ms, as the drive must be certain to find the right track"
[^hdd-wb]: OSTEP, chương 37, mục 37.3 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "if the file system or applications require that data be written to disk in a certain order for correctness, write-back caching can lead to problems"
[^hdd-formula]: OSTEP, chương 37, mục 37.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "Simply divide the size of the transfer by the time it took"
[^hdd-specs]: OSTEP, chương 37, mục 37.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "The first, known as the Cheetah 15K.5 [S09b], is a high-performance SCSI drive. The second, the Barracuda [S09a], is a drive built for capacity."
[^hdd-cheetah]: OSTEP, chương 37, mục 37.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "15000 RPM is equal to 250 RPS (rotations per second); thus, each rotation takes 4 ms. On average, the disk will encounter a half rotation and thus 2 ms is the average time."
[^hdd-6ms]: OSTEP, chương 37, mục 37.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "Thus, from our equation above, TI/O for the Cheetah roughly equals 6 ms."
[^hdd-barra]: OSTEP, chương 37, mục 37.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "The same calculation for the Barracuda yields a TI/O of about 13.2 ms, more than twice as slow, and thus a rate of about 0.31 MB/s."
[^hdd-seq]: OSTEP, chương 37, mục 37.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "Thus, TI/O for the Cheetah and Barracuda is about 800 ms and 950 ms, respectively."
[^hdd-gap]: OSTEP, chương 37, mục 37.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "there is a huge gap in drive performance between random and sequential workloads, almost a factor of 200 or so for the Cheetah and more than a factor 300 difference for the Barracuda."
[^hdd-tip]: OSTEP, chương 37, mục 37.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "When at all possible, transfer data to and from disks in a sequential manner. If sequential is not possible, at least think about transferring data in large chunks: the bigger, the better."
[^sched-sjf]: OSTEP, chương 37, mục 37.5 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "Unlike job scheduling, where the length of each job is usually unknown, with disk scheduling, we can make a good guess at how long a “job” (i.e., disk request) will take."
[^sched-sstf]: OSTEP, chương 37, mục 37.5 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "SSTF orders the queue of I/O requests by track, picking requests on the nearest track to complete first."
[^sched-nbf]: OSTEP, chương 37, mục 37.5 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "an OS can simply implement nearest-block-first (NBF), which schedules the request with the nearest block address next."
[^sched-starv]: OSTEP, chương 37, mục 37.5 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "Requests to any other tracks would then be ignored completely by a pure SSTF approach."
[^sched-scan]: OSTEP, chương 37, mục 37.5 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "The algorithm, originally called SCAN, simply moves back and forth across the disk servicing requests in order across the tracks."
[^sched-cscan]: OSTEP, chương 37, mục 37.5 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "C-SCAN is another common variant, short for Circular SCAN. Instead of sweeping in both directions across the disk, the algorithm only sweeps from outer-to-inner, and then resets at the outer track to begin again."
[^sched-sptf]: OSTEP, chương 37, mục 37.5 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "In particular, SCAN (or SSTF even) does not actually adhere as closely to the principle of SJF as they could. In particular, they ignore rotation."
[^sched-sptf2]: OSTEP, chương 37, mục 37.5 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "On modern drives, as we saw above, both seek and rotation are roughly equivalent (depending, of course, on the exact requests), and thus SPTF is useful and improves performance."
[^sched-inside]: OSTEP, chương 37, mục 37.5 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "Thus, SPTF is usually performed inside a drive, described below."
[^sched-16]: OSTEP, chương 37, mục 37.5 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "the OS scheduler usually picks what it thinks the best few requests are (say 16) and issues them all to disk"
[^sched-merge]: OSTEP, chương 37, mục 37.5 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "the scheduler should merge the requests for blocks 33 and 34 into a single two-block request"
[^sched-wc]: OSTEP, chương 37, mục 37.5 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "this approach is called work-conserving, as the disk will never be idle if there are requests to serve."
[^ssd-erase]: OSTEP, chương 44 Flash-based SSDs, phần mở đầu — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "to write to a given chunk of it (i.e., a flash page), you first have to erase a bigger chunk (i.e., a flash block), which can be quite expensive."
[^ssd-term]: OSTEP, chương 44, tip về thuật ngữ — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "What is a block to you may be a page to someone else, and vice versa, depending on the context."
[^ssd-sizes]: OSTEP, chương 44, mục 44.2 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "blocks (sometimes called erase blocks), which are typically of size 128 KB or 256 KB, and pages, which are a few KB in size (e.g., 4KB)."
[^ssd-ops]: OSTEP, chương 44, mục 44.3 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "The read command is used to read a page from the flash; erase and program are used in tandem to write."
[^ssd-erase2]: OSTEP, chương 44, mục 44.3 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "Erase, importantly, destroys the contents of the block (by setting each bit to the value 1)"
[^ssd-program]: OSTEP, chương 44, mục 44.3 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "Once a page has been programmed, the only way to change its contents is to erase the entire block within which the page resides."
[^ssd-fig442]: OSTEP, chương 44, mục 44.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "Figure 44.2 presents a rough summary of some numbers found in the popular press [V12]."
[^ssd-lat]: OSTEP, chương 44, mục 44.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "Finally, erases are quite expensive, taking a few milliseconds typically."
[^ssd-wear]: OSTEP, chương 44, mục 44.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "Manufacturers rate MLC-based blocks as having a 10,000 P/E (Program/Erase) cycle lifetime"
[^ssd-disturb]: OSTEP, chương 44, mục 44.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "it is possible that some bits get flipped in neighboring pages; such bit flips are known as read disturbs or program disturbs"
[^ssd-ftl]: OSTEP, chương 44, mục 44.5 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "The FTL takes read and write requests on logical blocks (that comprise the device interface) and turns them into low-level read, erase, and program commands on the underlying physical blocks and physical pages"
[^ssd-wa]: OSTEP, chương 44, mục 44.5 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "Another performance goal will be to reduce write amplification, which is defined as the total write traffic (in bytes) issued to the flash chips by the FTL divided by the total write traffic (in bytes)"
[^ssd-wl]: OSTEP, chương 44, mục 44.5 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "ensuring that all of the blocks of the device wear out at roughly the same time; doing so is called wear leveling"
[^ssd-direct]: OSTEP, chương 44, mục 44.6 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "The end result is severe write amplification (proportional to the number of pages in a block) and as a result, terrible write performance, even slower than typical hard drives"
[^ssd-log]: OSTEP, chương 44, mục 44.7 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "the device keeps a mapping table (in its memory, and persistent, in some form, on the device); this table stores the physical address of each logical block in the system."
[^ssd-log2]: OSTEP, chương 44, mục 44.7 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "The log-based approach by its nature improves performance (erases only being required once in a while, and the costly read-modify-write of the direct-mapped approach avoided altogether), and greatly enhances reliability."
[^ssd-oob2]: OSTEP, chương 44, mục 44.7 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "The device has to periodically perform garbage collection (GC) to find said blocks and free space for future writes; excessive garbage collection drives up write amplification and lowers performance."
[^ssd-gc]: OSTEP, chương 44, mục 44.8 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "find a block that contains one or more garbage pages, read in the live (non-garbage) pages from that block, write out those live pages to the log, and (finally) reclaim the entire block for use in writing."
[^ssd-gc2]: OSTEP, chương 44, mục 44.8 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "The ideal candidate for reclamation is a block that consists of only dead pages; in this case, the block can immediately be erased and used for new data, without expensive data migration."
[^ssd-gcex]: OSTEP, chương 44, mục 44.8 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "Block 0 has two dead blocks (pages 0 and 1) and two live blocks (pages 2 and 3, which contain blocks 2000 and 2001, respectively)."
[^ssd-trim]: OSTEP, chương 44, aside về TRIM — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "informs the device that the block(s) specified by the address (and length) have been deleted; the device thus no longer has to track any information about the given address range."
[^sys-discard]: Linux v6.8, Documentation/ABI/stable/sysfs-block, queue/discard_granularity — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/ABI/stable/sysfs-block> — "A discard_granularity of 0 means that the device does not support discard functionality."
[^ssd-op]: OSTEP, chương 44, mục 44.8 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "To reduce GC costs, some SSDs overprovision the device [A+08]; by adding extra flash capacity, cleaning can be delayed and pushed to the background"
[^ssd-map]: OSTEP, chương 44, mục 44.9 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "With a large 1-TB SSD, for example, a single 4-byte entry per 4-KB page results in 1 GB of memory needed by the device, just for these mappings!"
[^ssd-blockmap]: OSTEP, chương 44, mục 44.9 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "while block level mappings greatly reduce the amount of memory needed for translations, they cause significant performance problems when writes are smaller than the physical block size of the device"
[^ssd-wl2]: OSTEP, chương 44, mục 44.10 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "This process of wear leveling increases the write amplification of the SSD, and thus decreases performance as extra I/O is required to ensure that all blocks wear at roughly the same rate."
[^ssd-fig444]: OSTEP, chương 44, mục 44.11 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "Samsung 840 Pro SSD 103 287 421 384 Seagate 600 SSD 84 252 424 374 Intel SSD 335 SSD 39 222 344 354 Seagate Savvio 15K.3 HDD 2 2 223 223"
[^ssd-cmp]: OSTEP, chương 44, mục 44.11 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "in terms of sequential performance, there is much less of a difference; while the SSDs perform better, a hard drive is still a good choice if sequential performance is all you need."
[^ssd-rw]: OSTEP, chương 44, mục 44.11 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "The reason for such unexpectedly good random-write performance is due to the log-structured design of many SSDs, which transforms random writes into sequential ones and improves performance."
[^dev-pcie]: OSTEP, chương 36, mục 36.1 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — "higher performance storage devices (such as NVMe persistent storage devices) are often connected here."
[^nvme-doorbell]: Linux v6.8, drivers/nvme/host/pci.c, hàm nvme_write_sq_db — <https://raw.githubusercontent.com/torvalds/linux/v6.8/drivers/nvme/host/pci.c> — "writel(nvmeq->sq_tail, nvmeq->q_db);"
[^nvme-q]: Linux v6.8, drivers/nvme/host/pci.c, comment trên struct nvme_queue — <https://raw.githubusercontent.com/torvalds/linux/v6.8/drivers/nvme/host/pci.c> — "An NVM Express queue. Each device has at least two (one for admin commands and one for I/O commands)."
[^nvme-max]: Linux v6.8, drivers/nvme/host/pci.c, hàm nvme_max_io_queues — <https://raw.githubusercontent.com/torvalds/linux/v6.8/drivers/nvme/host/pci.c> — "return num_possible_cpus() + dev->nr_write_queues + dev->nr_poll_queues;"
[^nvme-irq]: Linux v6.8, drivers/nvme/host/pci.c, hàm nvme_setup_irqs — <https://raw.githubusercontent.com/torvalds/linux/v6.8/drivers/nvme/host/pci.c> — "We need interrupts for the admin queue and each non-polled I/O queue"
[^nvme-poll]: Linux v6.8, drivers/nvme/host/pci.c, tham số module poll_queues — <https://raw.githubusercontent.com/torvalds/linux/v6.8/drivers/nvme/host/pci.c> — "Number of queues to use for polled IO."
[^mq-poll]: Kernel docs, blk-mq, enum hctx_type — <https://docs.kernel.org/block/blk-mq.html> — "HCTX_TYPE_POLL Polled I/O of any kind."
[^mq-bg]: Kernel docs, blk-mq, Background — <https://docs.kernel.org/block/blk-mq.html> — "The Block IO subsystem aimed to achieve the best performance possible for those devices with a high penalty when doing random access, and the bottleneck was the mechanical moving parts"
[^mq-bg2]: Kernel docs, blk-mq, Background — <https://docs.kernel.org/block/blk-mq.html> — "The former design had a single queue to store block IO requests with a single lock. That did not scale well in SMP systems"
[^mq-intro]: Kernel docs, blk-mq, Background — <https://docs.kernel.org/block/blk-mq.html> — "the blk-mq API spawns multiple queues with individual entry points local to the CPU, removing the need for a lock."
[^mq-two]: Kernel docs, blk-mq, Operation — <https://docs.kernel.org/block/blk-mq.html> — "blk-mq has two group of queues: software staging queues and hardware dispatch queues."
[^mq-short]: Kernel docs, blk-mq, Operation — <https://docs.kernel.org/block/blk-mq.html> — "it will try the shortest path possible: send it directly to the hardware queue. However, there are two cases that it might not do that: if there’s an IO scheduler attached at the layer or if we want to try to merge requests."
[^mq-sw]: Kernel docs, blk-mq, Software staging queues — <https://docs.kernel.org/block/blk-mq.html> — "The staging queue can be used to merge requests for adjacent sectors. For instance, requests for sector 3-6, 6-7, 7-9 can become one request for 3-9."
[^mq-hw]: Kernel docs, blk-mq, Hardware dispatch queues — <https://docs.kernel.org/block/blk-mq.html> — "The number of hardware queues depends on the number of hardware contexts supported by the hardware and its device driver, but it will not be more than the number of cores of the system."
[^mq-tag]: Kernel docs, blk-mq, Tag-based completion — <https://docs.kernel.org/block/blk-mq.html> — "every request is identified by an integer, ranging from 0 to the dispatch queue size."
[^mq-order]: Kernel docs, blk-mq, Hardware dispatch queues (Note) — <https://docs.kernel.org/block/blk-mq.html> — "Neither the block layer nor the device protocols guarantee the order of completion of requests. This must be handled by higher layers, like the filesystem."
[^mq-sched]: Kernel docs, blk-mq, IO Schedulers — <https://docs.kernel.org/block/blk-mq.html> — "The scheduling happens only between requests in the same queue, so it is not possible to merge requests from different queues"
[^sw-sched]: Kernel docs, Switching Scheduler — <https://docs.kernel.org/block/switching-sched.html> — "the list of valid names will be displayed, with the currently selected scheduler in brackets"
[^sw-list]: Kernel docs, Switching Scheduler — <https://docs.kernel.org/block/switching-sched.html> — "It is possible to change the IO scheduler for a given block device on the fly to select one of mq-deadline, none, bfq, or kyber schedulers"
[^mq-none]: Kernel docs, blk-mq, IO Schedulers — <https://docs.kernel.org/block/blk-mq.html> — "It will just place requests on whatever software queue the process is running on, without any reordering."
[^dl-goal]: Kernel docs, Deadline IO scheduler tunables — <https://docs.kernel.org/block/deadline-iosched.html> — "The goal of the deadline io scheduler is to attempt to guarantee a start service time for a request."
[^dl-batch]: Kernel docs, Deadline IO scheduler tunables, fifo_batch — <https://docs.kernel.org/block/deadline-iosched.html> — "Requests are grouped into batches of a particular data direction (read or write) which are serviced in increasing sector order."
[^dl-pref]: Kernel docs, Deadline IO scheduler tunables, writes_starved — <https://docs.kernel.org/block/deadline-iosched.html> — "we always give a preference to reads. However, we don’t want to starve writes indefinitely either."
[^bfq-prop]: Kernel docs, BFQ — <https://docs.kernel.org/block/bfq-iosched.html> — "BFQ is a proportional-share I/O scheduler, with some extra low-latency capabilities."
[^bfq-lat]: Kernel docs, BFQ — <https://docs.kernel.org/block/bfq-iosched.html> — "In its default configuration, BFQ privileges latency over throughput."
[^bfq-over]: Kernel docs, BFQ — <https://docs.kernel.org/block/bfq-iosched.html> — "the total, single-lock-protected, per-request execution time of the lightest I/O scheduler available in blk-mq, mq-deadline, is 0.7 us"
[^sys-lbs]: Linux v6.8, Documentation/ABI/stable/sysfs-block, queue/logical_block_size — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/ABI/stable/sysfs-block> — "This is the smallest unit the storage device can address. It is typically 512 bytes."
[^sys-pbs]: Linux v6.8, Documentation/ABI/stable/sysfs-block, queue/physical_block_size — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/ABI/stable/sysfs-block> — "This is the smallest unit a physical storage device can write atomically. It is usually the same as the logical block size but may be bigger."
[^sys-rot]: Linux v6.8, Documentation/ABI/stable/sysfs-block, queue/rotational — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/ABI/stable/sysfs-block> — "This file is used to stat if the device is of rotational type or non-rotational type."
[^sys-atomic]: Linux v6.11, Documentation/ABI/stable/sysfs-block, atomic_write_unit_max_bytes — <https://raw.githubusercontent.com/torvalds/linux/v6.11/Documentation/ABI/stable/sysfs-block> — "This parameter defines the largest block which can be written atomically with an atomic write operation."
[^rust-rts]: Rust std 1.95.0, std::fs::read_to_string — <https://doc.rust-lang.org/1.95.0/std/fs/fn.read_to_string.html> — "Reads the entire contents of a file into a string."
[^hdd-cache]: OSTEP, chương 37, mục 37.3 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "This cache is just some small amount of memory (usually around 8 or 16 MB) which the drive can use to hold data read from or written to the disk."
[^ssd-v12]: OSTEP, chương 44, phần tham khảo [V12] — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "by Kristian Vatto. AnandTech, September, 2012."
[^ssd-hybrid]: OSTEP, chương 44, mục 44.9 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "To enable flexible writing but also reduce mapping costs, many modern FTLs employ a hybrid mapping technique."
[^ssd-cache]: OSTEP, chương 44, mục 44.9 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "Probably the simplest is just to cache only the active parts of the FTL in memory, thus reducing the amount of memory needed"
[^ssd-src]: OSTEP, chương 44, mục 44.11 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "the data was taken from a few different online sources [S13, T15]."
[^ssd-s13]: OSTEP, chương 44, phần tham khảo [S13] — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "by Anand Lal Shimpi. AnandTech, May 7, 2013."
[^ssd-t15]: OSTEP, chương 44, phần tham khảo [T15] — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "Hard Drives” by Tom’s Hardware. January 2015."
[^elv-default]: Linux v6.8, block/elevator.c, comment trên elevator_get_default — <https://raw.githubusercontent.com/torvalds/linux/v6.8/block/elevator.c> — "For single queue devices, default to using mq-deadline. If we have multiple queues or mq-deadline is not available, default to"
[^dev-regs]: OSTEP, chương 36, mục 36.3 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — "a status register, which can be read to see the current status of the device; a command register, to tell the device to perform a certain task; and a data register to pass data to the device, or get data from the device."
[^dev-pollcost]: OSTEP, chương 36, mục 36.3 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-devices.pdf> — "This basic protocol has the positive aspect of being simple and working. However, there are some inefficiencies and inconveniences involved."
[^hdd-wb2]: OSTEP, chương 37, mục 37.3 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-disks.pdf> — "The former is called write back caching (or sometimes immediate reporting), and the latter write through."
[^ssd-fig442b]: OSTEP, chương 44, Hình 44.2 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "SLC 25 200-300 1500-2000 MLC 50 600-900 ˜3000 TLC ˜75 ˜900-1350 ˜4500"
[^ssd-wl3]: OSTEP, chương 44, mục 44.10 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-ssd.pdf> — "a block will be filled with long-lived data that does not get over-written; in this case, garbage collection will never reclaim the block, and thus it does not receive its fair share of the write load."
