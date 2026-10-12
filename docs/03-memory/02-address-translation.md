# P3 · Chương 02 — Address translation: từ base/bounds, segmentation đến paging

## Bạn sẽ hiểu được gì

1. **Q1.** "Hardware-based address translation" là gì, MMU làm gì, OS phải làm gì để phần cứng dịch đúng? → mục 1
2. **Q2.** Base and bounds dịch (và từ chối) địa chỉ cụ thể ra sao; khi context switch OS lưu/khôi phục gì? → mục 2
3. **Q3.** Segmentation sửa lỗi gì của base/bounds; vì sao cần bit chiều tăng và protection bits; vì sao external fragmentation không chữa dứt điểm? → mục 3
4. **Q4.** Vì sao lỗi truy cập trái phép vẫn gọi là "segmentation fault" dù máy không còn segmentation? → mục 4
5. **Q5.** Paging tách virtual address thành VPN và offset, tra PTE, ghép PFN thế nào (tính tay một ví dụ)? → mục 5
6. **Q6.** PTE chứa những bit nào và mỗi bit để làm gì? → mục 6
7. **Q7.** Vì sao paging thắng segmentation, và nó tạo ra hai vấn đề nào cho [chương 03](03-page-table-va-tlb.md)? → mục 7

## Kiến thức cần trước

- [Chương 01 — Address space](01-address-space.md): process thấy bộ nhớ riêng, liên tục, bắt đầu từ 0 (virtual address); ba mục tiêu transparency / efficiency / protection.

## 1. Hardware-based address translation: ảo giác cần một cơ chế (Q1)

> **Trả lời ngắn.** Với mỗi truy cập bộ nhớ (lấy lệnh, load, store), phần cứng biến virtual address thành physical address nơi dữ liệu thực sự nằm[^ostep-translation]. Phần cứng chỉ cung cấp cơ chế mức thấp; OS phải cấu hình nó cho đúng và quản lý bộ nhớ nào đang rảnh, đang dùng[^m-track]. Khi mọi thứ bình thường, phần cứng tự dịch mà không cần OS chen vào[^m-nohw].

**Giải thích.**

Hình dung (nhận xét của người soạn): mỗi lá thư (truy cập bộ nhớ) đi qua một máy phân loại ở bưu điện, máy tra sổ và dán địa chỉ thật lên phong bì; con người (OS) chỉ lập và cập nhật cuốn sổ, không tự tay xử lý từng lá thư.

OSTEP nhắc lại *limited direct execution*: cho chương trình chạy thẳng trên phần cứng, chỉ chen vào ở vài thời điểm then chốt. Với bộ nhớ: hiệu quả đòi hỏi hỗ trợ phần cứng[^m-eff], còn kiểm soát nghĩa là không ứng dụng nào truy cập bộ nhớ ngoài phần của mình[^m-ctrl].

Cơ chế chạy theo ba bước. (1) Lúc boot, OS đang ở kernel mode cài các exception handler bằng lệnh đặc quyền, trong đó có handler cho truy cập ngoài giới hạn[^m-handlers]. (2) Khi process chạy, với *từng* lần lấy lệnh, load, store, phần cứng đổi virtual address do lệnh đưa ra thành physical address nơi dữ liệu thực sự nằm[^ostep-translation]; việc lặp ở mọi truy cập nên phải do phần cứng làm. (3) Phần cứng một mình không ảo hóa được bộ nhớ vì nó chỉ cung cấp cơ chế mức thấp; OS phải chen vào ở các thời điểm then chốt để cấu hình phần cứng cho đúng, đồng thời quản lý bộ nhớ nào đang rảnh, đang dùng[^m-os][^m-track].

Ba thế hệ cơ chế, mỗi thế hệ sửa nhược điểm của thế hệ trước: base/bounds → segmentation → paging.

Phần của bộ xử lý lo việc dịch này đôi khi được gọi là **memory management unit (MMU)**; OSTEP nói các kỹ thuật phức tạp hơn về sau sẽ thêm mạch vào MMU[^m-mmu]. Khi bình thường, phần cứng tự dịch mà không cần OS; OS chỉ chen vào khi process vi phạm hoặc khi timer buộc đổi process[^m-nohw][^m-lde].

<details><summary>Đi sâu: phần cứng cần có (theo OSTEP, trong bối cảnh base/bounds)</summary>

OSTEP tổng kết yêu cầu phần cứng: hai chế độ CPU (kernel mode toàn quyền, user mode bị giới hạn)[^m-modes]; cặp base/bounds và mạch dịch + kiểm tra giới hạn; lệnh *đặc quyền* để sửa base/bounds[^m-priv]; khả năng phát exception, kể cả khi process user thử đổi các thanh ghi đó[^m-exc].

</details>

**Quay lại Q1:** địa chỉ ảo được phần cứng dịch cho *từng* truy cập; OS không dịch từng lần mà cấu hình phần cứng và xử lý khi có vi phạm hoặc đổi process.

## 2. Base and bounds (dynamic relocation) (Q2)

> **Trả lời ngắn.** Mỗi CPU có hai thanh ghi base và bounds[^m-two-regs]; địa chỉ vật lý bằng địa chỉ ảo cộng base[^m-formula], sau khi kiểm tra địa chỉ ảo nằm trong giới hạn, nếu không CPU phát exception[^ostep-bounds-exc]. Vì chỉ có một cặp thanh ghi mà giá trị khác nhau theo process, OS phải lưu cặp này vào cấu trúc dữ liệu riêng của process (gọi là PCB) khi dừng process và nạp lại khi chạy tiếp[^m-ctx][^m-ctx2].

**Giải thích.**

Hình dung (nhận xét của người soạn): thuê một dãy phòng khách sạn; bạn tưởng phòng của mình đánh số từ 0, lễ tân cộng thêm số tầng bạn thuê (base), và số phòng vượt quá số phòng bạn thuê (bounds) bị từ chối.

*Ví dụ chạy xuyên suốt.* Hãy tưởng tượng một process có address space 16 KB, bắt đầu ở địa chỉ 0. Đoạn code C `x = x + 3` được compiler dịch thành ba lệnh x86 đặt tại địa chỉ 128, 132, 135: một lệnh load từ địa chỉ của biến `x` (ở 15 KB, trong stack, giá trị ban đầu 3000), một lệnh cộng, một lệnh store trở lại cùng địa chỉ. Theo quan điểm của process, các truy cập bộ nhớ là: lấy lệnh ở 128, load từ 15 KB, lấy lệnh ở 132, (lệnh cộng không chạm bộ nhớ), lấy lệnh ở 135, store vào 15 KB[^m-example-seq][^m-example-seq2]. Đây toàn là virtual address. Vấn đề: OS muốn đặt process ở chỗ khác trong RAM, không nhất thiết từ địa chỉ 0, mà process không được biết[^m-problem].

*Cơ chế.* Ý tưởng được giới thiệu từ các máy time-sharing đầu tiên cuối thập niên 1950, gọi là *base and bounds*, hay *dynamic relocation*[^ostep-bb]. Mỗi CPU có hai thanh ghi: *base* và *bounds* (đôi khi gọi là *limit*)[^m-two-regs]. Mỗi chương trình được viết và biên dịch như thể được nạp ở địa chỉ 0; khi chương trình bắt đầu chạy, OS quyết định đặt nó ở đâu trong RAM và nạp giá trị đó vào thanh ghi base[^m-load0]. Từ đó mọi tham chiếu được dịch theo công thức[^m-formula]:

```
physical address = virtual address + base
```

Với process 16 KB được OS đặt tại 32 KB (32768): khi PC = 128, phần cứng cộng 128 + 32768 = 32896 và lấy lệnh từ địa chỉ vật lý đó[^m-trace128]. Khi lệnh load phát địa chỉ ảo 15 KB, phần cứng cộng base 32 KB và ra địa chỉ vật lý 47 KB[^m-trace15k].

Thanh ghi **bounds** phục vụ protection[^ostep-bounds] — trước khi cộng base, bộ xử lý kiểm tra tham chiếu có nằm trong giới hạn không. Nếu virtual address lớn hơn hoặc bằng bounds, hoặc âm, CPU phát exception và process nhiều khả năng bị kết thúc[^ostep-bounds-exc].

Bảng dưới là ví dụ của OSTEP (address space 4 KB nạp tại 16 KB: base = 16384, bounds = 4096); tôi đã chạy lại bằng công thức trên ở mục Liên hệ với Rust; các giá trị 16 KB, 17 KB, 19384 và fault nằm trong bảng của sách, còn 16384 và 17408 là đổi KB sang byte của người soạn[^m-example-table][^m-table-fault].

| Virtual address | Kiểm tra `va < 4096` | Physical address |
|---|---|---|
| 0 | đúng | 16384 (16 KB) |
| 1024 (1 KB) | đúng | 17408 (17 KB) |
| 3000 | đúng | 19384 |
| 4400 | sai | Fault (out of bounds) |

Cặp base/bounds là cấu trúc phần cứng nằm trên chip, mỗi CPU một cặp[^m-onchip].

*Việc của OS.* Về phía OS có bốn việc:

1. **Khi tạo process**: tìm chỗ cho address space. Với giả định mọi address space nhỏ hơn bộ nhớ vật lý và cùng kích thước, OS xem RAM là mảng các "khe" và theo dõi khe nào trống[^m-slots]; nó duyệt một *free list* để tìm khe trống rồi đánh dấu đã dùng[^m-free]. Khi process kết thúc, OS trả bộ nhớ của nó về free list[^m-reclaim].
2. **Khi context switch**: mỗi CPU chỉ có *một* cặp base/bounds mà giá trị khác nhau cho mỗi process[^m-ctx]. Chuyển từ process A sang B nghĩa là ghi giá trị hiện có của cặp thanh ghi vào PCB của A, rồi nạp giá trị đã lưu của B vào thanh ghi[^m-ctx2]; nếu bỏ bước này, B sẽ bị dịch bằng base của A (suy luận của người soạn).
3. **Có thể di chuyển address space** khi process đang bị dừng: deschedule, copy sang vị trí mới, cập nhật base đã lưu; khi chạy lại process không hay biết gì[^m-move].
4. **Cài exception handler** lúc boot qua lệnh đặc quyền[^m-handlers]; phản ứng thường gặp với truy cập ngoài bounds là kết thúc process vi phạm.

<details><summary>Đi sâu: hai cách định nghĩa bounds</summary>

Có hai cách định nghĩa bounds: giữ *kích thước* address space (kiểm tra trước, rồi mới cộng base) hoặc giữ *địa chỉ vật lý cuối* (cộng base trước rồi kiểm tra); OSTEP nói hai cách tương đương về logic[^m-bound-two].

</details>

**Quay lại Q2:** `PA = VA + base` với kiểm tra `VA < bounds`; cặp thanh ghi là per-CPU nên OS lưu/khôi phục khi context switch.

## 3. Segmentation (Q3)

> **Trả lời ngắn.** Base/bounds lãng phí vùng trống giữa stack và heap nằm trong khối đã cấp (internal fragmentation)[^ostep-bb-waste]; segmentation đặt một cặp base/bounds cho mỗi segment logic[^ostep-seg] nên chỉ phần dùng mới chiếm RAM[^ostep-seg-sparse]. Stack tăng ngược nên cần bit chiều tăng[^ostep-seg-bit], còn protection bits cho phép chia sẻ code[^ostep-seg-prot]. Đổi lại, RAM đầy lỗ hổng rải rác giữa các khối đã cấp (external fragmentation), mà dồn khối lại (compaction) hay thuật toán chọn chỗ trống khôn hơn chỉ giảm chứ không chữa dứt điểm[^ostep-seg-ext-always].

**Giải thích.**

Hình dung (nhận xét của người soạn): thay vì thuê nguyên một căn hộ lớn cho cả đồ đạc lẫn khoảng trống (base/bounds), bạn thuê ba phòng nhỏ đúng cỡ cho code, heap, stack; nhưng kho ngày càng nhiều phòng lẻ khó ghép lại.

*Lỗi của base/bounds.* Base/bounds nhanh (nhận xét của người soạn: một phép cộng, một phép so sánh), nhưng process 16 KB trong ví dụ chiếm trọn khe 32–48 KB dù stack và heap chưa lớn: khoảng trống giữa chúng bị lãng phí. OSTEP gọi đó là *internal fragmentation*, vì chỗ lãng phí nằm *bên trong* đơn vị đã cấp phát[^ostep-bb-waste]. Với address space 32-bit, chương trình thường chỉ dùng vài MB nhưng vẫn đòi cả address space nằm trong bộ nhớ[^s-32bit].

*Ý tưởng: generalized base/bounds.* Segmentation là ý tưởng cũ, ít nhất từ đầu thập niên 1960[^s-old]. Thay vì một cặp base/bounds cho cả address space, hãy có một cặp *cho mỗi segment logic*[^ostep-seg]. Segment là một phần liên tục của address space có độ dài xác định[^s-def]; trong address space chuẩn có ba segment khác nhau về mặt logic: code, stack, heap. OS đặt mỗi segment ở một chỗ riêng trong RAM vật lý, nhờ vậy không phải lấp đầy RAM bằng phần address space không dùng.

Phần cứng cần một tập ba cặp thanh ghi base/bounds[^s-regs]. Ví dụ của OSTEP (address space 16 KB, RAM 64 KB, 16 KB đầu dành cho OS; giá trị bảng lấy từ Hình 16.3 của sách, phần text của sách xác nhận code 32 KB/2 KB, heap 34 KB/3 KB và stack 28 KB, size 2 KB)[^s-table][^s-stack-base].

| Segment | Base | Size |
|---|---|---|
| Code | 32K | 2K |
| Heap | 34K | 3K |
| Stack | 28K | 2K |

Thanh ghi size chính là bounds đã gặp[^s-size]. Chỉ phần bộ nhớ thực sự dùng mới chiếm RAM, nên address space lớn mà thưa dùng được[^ostep-seg-sparse].

*Dịch địa chỉ: phải trừ đi điểm bắt đầu segment.*

Ví dụ 1 — virtual address 100 (lấy lệnh, thuộc code): cộng base với offset trong segment (100) ra 100 + 32 KB = 32868, rồi kiểm tra 100 < 2 KB (đúng) và phát truy cập tới 32868[^s-trans100].

Ví dụ 2 — virtual address 4200 (heap): không thể cộng thẳng với base heap, vì ra 39016 là sai. Heap bắt đầu ở virtual address 4 KB (4096) nên offset là 4200 − 4096 = 104; cộng với base 34 KB ra 34920[^s-heap].

Ví dụ 3 — địa chỉ từ 7 KB trở lên nằm sau cuối heap: phần cứng thấy ngoài bounds, trap vào OS, process nhiều khả năng bị kết thúc[^s-oob].

*Chiều tăng của stack.* Stack khác code/heap ở một điểm: nó tăng ngược về phía địa chỉ thấp[^ostep-seg-stack]. Trong ví dụ, stack nằm ở 28 KB vật lý và tăng ngược về 26 KB, tương ứng virtual 16 KB về 14 KB; phép dịch phải làm khác đi. Phần cứng cần thêm thông tin về chiều tăng của segment, ví dụ một bit đặt 1 nếu tăng theo chiều dương, 0 nếu âm[^ostep-seg-bit].

Ví dụ: virtual address 15 KB (0x3C00 = `11 1100 0000 0000`). Hai bit trên `11` chọn stack, còn lại offset 3 KB. Để có offset âm đúng ta lấy 3 KB − 4 KB (kích thước tối đa của segment) = −1 KB, rồi cộng offset âm vào base 28 KB ra 27 KB[^s-negative]. Kiểm tra bounds lúc này là trị tuyệt đối của offset âm không vượt kích thước hiện tại của segment (2 KB ở ví dụ)[^s-negbound].

*Protection bits và chia sẻ.* Khi segmentation phát triển, các nhà thiết kế thấy có thể tiết kiệm bộ nhớ bằng cách chia sẻ một số segment giữa các address space. Để làm được, phần cứng cần *protection bits*: vài bit mỗi segment cho biết chương trình có được đọc, ghi hoặc thực thi code trong segment đó không[^s-protbits]. Đặt segment code là read-only thì cùng một đoạn code có thể được chia sẻ giữa nhiều process mà không phá cô lập[^ostep-seg-prot]. Phần cứng lúc này kiểm tra thêm quyền truy cập: ghi vào segment read-only hoặc thực thi từ segment không-executable sẽ phát exception cho OS xử lý[^s-protcheck].

*External fragmentation.* Mỗi process có nhiều segment kích thước khác nhau, nên RAM nhanh chóng đầy "lỗ hổng" nhỏ rải rác. Đó là **external fragmentation** — chỗ trống nằm *bên ngoài* các khối đã cấp[^ostep-seg-ext].

Ví dụ của OSTEP: xin segment 20 KB khi tổng cộng có 24 KB trống nhưng ở ba mảnh không liên tục thì OS không đáp ứng được[^s-20kb].

Hai hướng xử lý:

- **Compaction**: dừng các process, copy segment về một vùng liên tục, cập nhật thanh ghi segment. Tốn kém, vì copy segment là việc ngốn bộ nhớ và tốn nhiều thời gian CPU[^s-compact]; nó còn làm khó việc đáp ứng các yêu cầu nới segment về sau[^s-compact-grow].
- **Thuật toán free-list thông minh** (best-fit, worst-fit, first-fit, buddy…). Nhưng dù thuật toán khôn đến đâu, external fragmentation vẫn tồn tại; thuật toán tốt chỉ giảm nó[^ostep-seg-ext-always]. OSTEP kết luận: giải pháp thật sự là tránh vấn đề bằng cách không bao giờ cấp phát bộ nhớ theo khối kích thước thay đổi[^s-onlyreal].

Segmentation nhanh vì phép tính đơn giản[^s-fast] và cho chia sẻ code[^s-share]. Nhưng còn external fragmentation (xem ở trên), và chưa đủ linh hoạt cho address space thưa tổng quát: một heap lớn dùng thưa nằm trọn trong một segment thì cả heap vẫn phải nằm trong RAM[^s-notflex].

<details><summary>Đi sâu: cách phần cứng biết segment nào, và việc của OS với segmentation</summary>

*Chọn segment.* Cách *explicit* (dùng trong VAX/VMS): cắt address space theo vài bit cao của virtual address[^s-explicit]. Với địa chỉ 14 bit và ba segment, cần 2 bit: `00` = code, `01` = heap, `11` = stack; 12 bit còn lại là offset trong segment. Địa chỉ 4200 là `01 0000 0110 1000`: 2 bit trên là `01` (heap) và 12 bit dưới là 0x068 = 104, đúng kết quả ở trên. Có thể diễn tả bằng giả mã của sách:

```
Segment = (VirtualAddress & SEG_MASK) >> SEG_SHIFT   // SEG_MASK=0x3000, SEG_SHIFT=12
Offset  =  VirtualAddress & OFFSET_MASK              // OFFSET_MASK=0xFFF
if (Offset >= Bounds[Segment]) RaiseException(PROTECTION_FAULT)
else PhysAddr = Base[Segment] + Offset
```

Offset giúp việc kiểm tra bounds gọn: chỉ cần xem offset có nhỏ hơn bounds không[^s-bounds-off]. Nhược điểm của cách chia bit cao: mỗi segment bị giới hạn kích thước tối đa (ở ví dụ là 4 KB vì 16 KB chia làm 4 phần); chương trình muốn segment lớn hơn thì "hết cách"[^s-maxsize], và dùng 2 bit cho 3 segment thì một phần tư address space bỏ không[^s-unused]. Cách *implicit*: phần cứng suy ra segment từ *cách địa chỉ được tạo ra* (địa chỉ từ PC là code, từ stack/base pointer là stack, còn lại là heap)[^s-implicit].

*Việc của OS.* Context switch: các thanh ghi segment phải được lưu và khôi phục, vì mỗi process có address space riêng[^s-ctx]. Khi segment lớn lên: ví dụ `malloc()` thấy heap hiện tại hết chỗ, thư viện cấp phát thực hiện một system call để nới heap (kiểu `sbrk()` cổ điển); OS thường cấp thêm và cập nhật thanh ghi size, nhưng nó *có thể từ chối* nếu hết bộ nhớ vật lý hoặc nếu cho rằng process đã dùng quá nhiều[^s-sbrk][^s-reject]. (Xem tiếp: [chương 06](06-heap-allocator.md) đi sâu vào `brk`/`mmap` của allocator hiện đại.)

</details>

**Quay lại Q3:** segmentation sửa lãng phí giữa heap và stack bằng một cặp base/bounds mỗi segment (cần bit chiều tăng và protection bits), nhưng đẻ ra external fragmentation mà chỉ giảm được, không chữa dứt điểm.

## 4. Vì sao vẫn gọi là "segmentation fault" (Q4)

> **Trả lời ngắn.** Thuật ngữ sinh ra từ truy cập địa chỉ bất hợp pháp trên máy có segmentation, và vẫn tồn tại ngay cả trên máy không hỗ trợ segmentation[^ostep-segfault]. Trên máy dùng paging, lỗi tương ứng nằm ở mức page, nên cái tên chỉ còn là di sản (suy luận của người soạn).

**Giải thích.**

Hình dung (nhận xét của người soạn): biển "cổng bảo vệ" vẫn treo ở tòa nhà dù cổng đã đổi sang thẻ từ; tên là di sản, cơ chế đã khác.

Đi theo từng thế hệ cơ chế. Trên máy có segmentation, phần cứng so offset với bounds của segment; ngoài bounds thì trap vào OS, và process nhiều khả năng bị kết thúc[^s-oob]. Truy cập bất hợp pháp kiểu này chính là nguồn gốc của tên *segmentation fault* (hay *violation*)[^ostep-segfault].

Sang paging, kiểm tra chuyển vào PTE: giả mã của OSTEP cho paging (mục 7, phần Đi sâu) vẫn gọi `RaiseException(SEGMENTATION_FAULT)` khi PTE không valid[^p-segv-pte]. Tên cũ được giữ cho lỗi mới; OSTEP nhận xét nó tồn tại ngay cả trên máy không hỗ trợ segmentation[^ostep-segfault].

<details><summary>Đi sâu: Linux báo hai nguyên nhân cho SIGSEGV</summary>

Linux báo hai nguyên nhân cho SIGSEGV là địa chỉ không ánh xạ tới đối tượng nào (`SEGV_MAPERR`) hoặc ánh xạ có nhưng sai quyền (`SEGV_ACCERR`)[^l-maperr][^l-accerr]. (Suy luận của người soạn: vì vậy ngày nay "segmentation fault" thường là lỗi mức page — thiếu mapping hoặc sai quyền — còn cái tên chỉ là di sản; man page không nói trực tiếp câu này.)

</details>

**Quay lại Q4:** "segmentation fault" là tên từ thời segmentation; ngày nay thường là lỗi mức page (thiếu mapping hoặc sai quyền).

## 5. Paging: VPN, offset, PTE và PFN (Q5)

> **Trả lời ngắn.** Paging chia address space thành các page cố định và RAM thành các page frame cùng cỡ[^ostep-paging][^ostep-frame]. Virtual address được tách thành số thứ tự page (VPN) và offset[^ostep-vpn]; page table tra VPN ra số thứ tự frame (PFN), PFN ghép với offset thành physical address, còn offset giữ nguyên vì nó chỉ cho biết byte nào bên trong page[^p-offset].

**Giải thích.**

Hình dung (nhận xét của người soạn): địa chỉ như "trang 1, dòng 5" của một cuốn sách; mục lục (page table) cho biết trang 1 thực sự nằm ở kệ số 7, còn "dòng 5" thì giữ nguyên.

Thay vì tách address space thành vài segment logic, ta chia nó thành các đơn vị cố định gọi là **page**[^ostep-paging]. Tương ứng, RAM vật lý được xem là một mảng các khe kích thước cố định gọi là **page frame**; mỗi frame chứa đúng một virtual page[^ostep-frame].

Ví dụ nhỏ trong sách: address space 64 byte với bốn page 16 byte (virtual page 0–3), và RAM vật lý 128 byte gồm tám frame[^p-example]. OS đặt: VP0 → frame 3, VP1 → frame 7, VP2 → frame 5, VP3 → frame 2; frame 1, 4, 6 còn trống[^p-placement].

Lợi ích: **linh hoạt** — không cần giả định stack/heap tăng hướng nào[^p-flex]; và **quản lý chỗ trống đơn giản** — đặt address space 64 byte chỉ cần lấy bốn frame trống từ free list[^ostep-page-simple][^p-freelist].

Để ghi virtual page nằm ở frame nào, OS thường giữ một cấu trúc *theo từng process* gọi là **page table**[^ostep-pt]. Vai trò chính của nó là lưu bản dịch cho từng virtual page của address space, nên nó cho ta biết mỗi page đang ở đâu trong RAM vật lý. Với ví dụ, page table có bốn entry: (VP0 → PF3), (VP1 → PF7), (VP2 → PF5), (VP3 → PF2)[^p-pt4]. Vì page table là cấu trúc per-process nên khi chạy process khác, OS dùng page table khác (các process có thể chia sẻ vài frame, nhưng ánh xạ của mỗi process vẫn riêng)[^p-perproc]. Cấu trúc cụ thể của page table hoặc do phần cứng quy định (hệ cũ) hoặc được OS quản lý linh hoạt hơn (hệ hiện đại)[^p-pt-variants].

Để dịch một virtual address, trước hết phải tách nó thành hai phần: **virtual page number (VPN)** và **offset trong page**[^ostep-vpn].

Với address space 64 byte, cần 6 bit cho virtual address (2⁶ = 64)[^p-bits]. Page size là 16 byte, nên cần chọn một trong bốn page — hai bit cao làm việc đó: đó là VPN 2 bit, 4 bit còn lại là offset[^p-vpn-bits].

Ví dụ `movl 21, %eax` (bỏ qua việc lấy lệnh). 21 = `010101`; tách: VPN = `01` (= 1), offset = `0101` (= 5), tức byte thứ 5 của virtual page 1. Tra page table: VPN 1 → physical frame number (PFN, đôi khi gọi là physical page number, PPN) = 7 = `111`[^p-pfn]. Thay VPN bằng PFN, giữ nguyên offset: `111` `0101` = `1110101` = 117[^p-result]. Offset không được dịch, vì nó chỉ cho biết byte nào bên trong page[^p-offset].

Thêm ví dụ do tôi tự tính từ page table này (đã kiểm bằng chương trình ở phần Liên hệ với Rust; không trích từ sách):

| VA (thập phân) | Nhị phân | VPN | offset | PFN | PA = PFN×16 + offset |
|---|---|---|---|---|---|
| 0 | 000000 | 0 | 0 | 3 | 48 |
| 21 | 010101 | 1 | 5 | 7 | 117 |
| 50 | 110010 | 3 | 2 | 2 | 34 |
| 63 | 111111 | 3 | 15 | 2 | 47 |

<details><summary>Đi sâu: vì sao paging có từ Atlas, và phép tách bằng mask/shift</summary>

Với hầu hết bài toán quản lý không gian, OS có hai cách: chia thành mảnh kích thước thay đổi (như segmentation) hoặc chia thành mảnh *kích thước cố định*[^p-two-ways]. Cách sau, trong virtual memory, gọi là *paging*, có từ hệ thống Atlas[^p-atlas].

Với address space 32-bit và page 4 KB, địa chỉ tách thành VPN 20 bit và offset 12 bit[^p-32bit]. Phép tách là mask và shift: `VPN = (VirtualAddress & VPN_MASK) >> SHIFT`, địa chỉ vật lý là `(PFN << SHIFT) | offset`[^p-mask].

</details>

**Quay lại Q5:** tách VA thành VPN + offset, tra page table lấy PFN, ghép lại với offset (ví dụ: 21 → VPN 1, offset 5, PFN 7 → 117).

## 6. Bên trong một PTE (Q6)

> **Trả lời ngắn.** Ngoài PFN (số thứ tự frame), mỗi PTE — một dòng của page table — mang các bit điều khiển: valid (bản dịch có hợp lệ không), protection (đọc/ghi/thực thi), present (page ở RAM hay đĩa), dirty (đã bị sửa chưa) và accessed (đã được truy cập chưa)[^p-valid][^p-prot][^p-present][^p-dirty][^p-ref].

**Giải thích.**

Hình dung (nhận xét của người soạn): mỗi trang trong mục lục có thêm vài ô đánh dấu — "trang này có thật không", "ai được đọc/ghi", "đang ở kệ hay trong kho", "có bị sửa chưa", "có ai dùng gần đây không".

Page table đơn giản nhất là *linear page table*, chỉ là một mảng: OS lập chỉ mục theo VPN và đọc **page-table entry (PTE)** tại chỉ mục đó để lấy PFN[^p-linear]. Ngoài PFN, PTE mang các bit sau (theo OSTEP):

| Bit | Ý nghĩa |
|---|---|
| Valid | Bản dịch có hợp lệ không. Vùng chưa dùng bị đánh dấu invalid; truy cập vào đó sinh trap tới OS. Vì vậy valid then chốt cho address space thưa[^p-valid]. |
| Protection | Đọc / ghi / thực thi được không; sai thì trap[^p-prot]. |
| Present | Page ở RAM hay trên đĩa[^p-present]. |
| Dirty | Page đã bị sửa từ khi nạp vào RAM chưa[^p-dirty]. |
| Accessed | Page đã được truy cập chưa; hữu ích khi chọn page để thay thế[^p-ref]. |

<details><summary>Đi sâu: PTE x86 và bit present/valid</summary>

PTE x86 trong sách (Hình 18.5, sơ đồ 32 bit) có: present bit (P), read/write (R/W) quyết định có được ghi vào page không, user/supervisor (U/S) quyết định process ở user mode có truy cập được không, một vài bit (PWT, PCD, PAT, G) điều khiển cách hardware caching hoạt động cho page, bit accessed (A), bit dirty (D) và cuối cùng là PFN[^ostep-pte-bits].

Chi tiết hay gây nhầm: trong ví dụ PTE x86 của sách chỉ có present, không có valid riêng. P = 1: vừa present vừa valid; P = 0: có thể hợp lệ nhưng không ở RAM, hoặc không hợp lệ, và OS dùng cấu trúc phụ để phân biệt[^p-nobit]. Xem tiếp ở [chương 04](04-page-fault-va-swapping.md).

</details>

**Quay lại Q6:** PTE = PFN + các bit valid, protection, present, dirty, accessed (x86 gộp valid vào present).

## 7. Vì sao paging thắng segmentation, và hai vấn đề cho chương 03 (Q7)

> **Trả lời ngắn.** Paging không gây external fragmentation vì đơn vị cố định, và cho dùng thưa address space[^ostep-page-noext]; quản lý chỗ trống thành việc lấy frame từ free list[^ostep-page-simple]. Cái giá là hai vấn đề: page table quá lớn (ví dụ 4 MB mỗi process ở 32-bit/4 KB)[^ostep-pt-size] và mỗi truy cập tốn thêm một lần đọc RAM[^p-extra].

**Giải thích.**

Hình dung (nhận xét của người soạn): ghép tủ bằng ô vuông cùng cỡ thì ô nào trống cũng dùng được (không còn "lỗ hổng không vừa"), nhưng cuốn sổ ghi ô nào ở đâu có thể dày bằng cả kho, và mỗi lần lấy đồ phải mở sổ trước.

Page table có thể lớn hơn nhiều so với bảng segment hoặc cặp base/bounds[^p-big]. Tính với address space 32-bit, page 4 KB: VPN 20 bit nghĩa là 2²⁰ (khoảng một triệu) bản dịch cho mỗi process; với 4 byte mỗi PTE ta cần 4 MB cho *mỗi* page table[^ostep-pt-size]. Với 100 process, OS cần 400 MB chỉ để chứa các bản dịch[^p-100]. Và chưa tính address space 64-bit.

Vì quá lớn, MMU không có phần cứng đặc biệt trên chip để chứa page table của process đang chạy; thay vào đó page table của từng process nằm trong bộ nhớ[^p-inmem].

Hệ quả: với *mỗi* tham chiếu bộ nhớ (lấy lệnh, load, store), paging đòi thêm một lần tham chiếu RAM để lấy bản dịch từ page table trước[^p-extra]. Như OSTEP nói, điều đó nhiều khả năng làm chậm process "một hệ số hai hoặc hơn"[^p-slow2].

OSTEP tóm lại: không thiết kế cẩn thận cả phần cứng lẫn phần mềm thì page table làm hệ thống chạy quá chậm và tốn quá nhiều bộ nhớ[^p-two-prob]. Hai vấn đề: (1) **bảng quá lớn**; (2) **mỗi truy cập tốn thêm một lần đọc RAM**.
[Chương 03](03-page-table-va-tlb.md) giải cả hai: page table nhiều cấp cho vấn đề kích thước, TLB (cache phần cứng các bản dịch gần đây) cho vấn đề tốc độ.

| Tiêu chí | Base and bounds | Segmentation | Paging |
|---|---|---|---|
| Đơn vị dịch | cả address space | segment, kích thước thay đổi | page, kích thước cố định |
| Phần cứng | 1 cặp thanh ghi mỗi CPU | một cặp mỗi segment (+ bit chiều tăng, protection) | page table trong RAM + thanh ghi trỏ tới nó (PTBR) |
| Quản lý RAM vật lý | đặt cả khối liên tục | phải tìm khối liên tục theo kích thước từng segment | chỉ cần tìm các frame trống (free list) |
| Vấn đề chính | lãng phí vùng giữa heap và stack (internal fragmentation)[^ostep-bb-waste] | external fragmentation[^ostep-seg-ext] | bảng lớn và chậm nếu làm cẩu thả[^ostep-paging-slow] |
| Address space thưa | không: vùng trống giữa heap và stack vẫn chiếm RAM[^ostep-bb-waste] | một phần; segment lớn thưa vẫn phải nằm trọn trong RAM[^s-notflex] | có, nhờ bit valid[^p-valid] |
| Giả định về stack/heap | không áp dụng (cả address space là một khối) | phần cứng phải biết chiều tăng của từng segment[^ostep-seg-bit] | không giả định hướng tăng[^p-flex] |

Khớp với kết luận ở mục 3: cách thật sự tránh external fragmentation là không cấp phát khối kích thước thay đổi[^s-onlyreal]; paging làm đúng điều đó vì chia bộ nhớ thành đơn vị cố định[^ostep-page-noext].

<details><summary>Đi sâu: công thức tra PTE, dấu vết bộ nhớ, và internal fragmentation trong page</summary>

Giả sử có một **page-table base register (PTBR)** chứa địa chỉ vật lý của đầu page table của process đang chạy[^p-ptbr]. Để tìm PTE cần đọc, phần cứng tính: `VPN = (VirtualAddress & VPN_MASK) >> SHIFT`, rồi `PTEAddr = PageTableBaseRegister + (VPN * sizeof(PTE))`[^p-formula]. Sau đó nó đọc PTE từ RAM, kiểm tra valid và protection (invalid thì phát exception; sai quyền thì phát protection fault, theo giả mã Hình 18.6 của sách[^p-fig186]), ghép PFN với offset rồi mới truy cập dữ liệu thật.

*Dấu vết bộ nhớ.* OSTEP minh họa bằng vòng lặp khởi tạo mảng 1000 số nguyên, với giả định: address space 64 KB, page 1 KB, page table tuyến tính ở địa chỉ vật lý 1 KB.

Kết quả của sách: mỗi lần lặp có **10** truy cập bộ nhớ — bốn lần lấy lệnh, một lần cập nhật mảng, và năm lần truy cập page table để dịch chúng[^p-trace]. Một nửa số truy cập RAM chỉ để tra bản dịch (tính toán của sách cho cấu hình giả định, không phải đo trên máy thật).

Cái giá: page table to và truy cập chậm (như trên). Ngoài ra internal fragmentation vẫn còn *trong page*: OSTEP (ch.20) nói page lớn gây lãng phí bên trong mỗi page[^p-internal], nên hầu hết hệ thống dùng page nhỏ, thường 4 KB[^p-smallpg]. Mức lãng phí cụ thể (trung bình bao nhiêu) sách không đo; chương này không đưa số.

</details>

**Quay lại Q7:** paging thắng vì hết external fragmentation và cho address space thưa; nó để lại hai việc cho chương 03 — bảng quá lớn và mỗi truy cập chậm hơn.

## Liên hệ với Rust: mô phỏng ba cơ chế bằng vài dòng

Rust không có API "dịch địa chỉ" (việc của phần cứng và kernel), nhưng viết lại các phép toán trên là cách rẻ nhất để kiểm chứng số học. Chương trình dưới được biên dịch bằng `rustc -O` (1.95.0) và chạy; nó chỉ mô phỏng các ví dụ số ở trên.

```rust
fn bb(va: u32, base: u32, bounds: u32) -> Option<u32> {
    if va < bounds { Some(base + va) } else { None }
}

// Địa chỉ 14 bit: 2 bit cao chọn segment; mỗi segment là (base, size, grows_positive).
fn seg(va: u32) -> Option<u32> {
    let t = [(32768u32, 2048u32, true), (34816, 3072, true), (0, 0, true), (28672, 2048, false)];
    let s = (va >> 12) as usize;
    let off = va & 0xFFF;
    let (b, sz, pos) = t[s];
    if pos {
        if off < sz { Some(b + off) } else { None }
    } else {
        let neg = 4096 - off; // độ lớn của offset âm
        if neg <= sz { Some(b - neg) } else { None }
    }
}

// Address space 64 B, page 16 B: VPN = 2 bit cao, offset = 4 bit thấp.
fn page(va: u32, pt: &[u32]) -> u32 {
    let vpn = (va & 0x30) >> 4;
    let off = va & 0xF;
    (pt[vpn as usize] << 4) | off
}

fn main() {
    for va in [0u32, 1024, 3000, 4400] {
        println!("bb {va} -> {:?}", bb(va, 16384, 4096));
    }
    for va in [100u32, 4200, 15360, 7168] {
        println!("seg {va} -> {:?}", seg(va));
    }
    let pt = [3u32, 7, 5, 2];
    for va in [0u32, 21, 50, 63] {
        println!("page {va} -> {}", page(va, &pt));
    }
    println!("{} bytes", (1u64 << 20) * 4);
}
```

Kết quả khi chạy:

```
bb 0 -> Some(16384)
bb 1024 -> Some(17408)
bb 3000 -> Some(19384)
bb 4400 -> None
seg 100 -> Some(32868)
seg 4200 -> Some(34920)
seg 15360 -> Some(27648)
seg 7168 -> None
page 0 -> 48
page 21 -> 117
page 50 -> 34
page 63 -> 47
4194304 bytes
```

Đối chiếu: 19384 và fault cho 4400 khớp bảng base/bounds[^m-example-table]; 32868, 34920 khớp ví dụ segmentation[^s-trans100][^s-heap]; 27648 = 27 KB khớp ví dụ stack[^s-negative]; 7168 bị từ chối như sách mô tả[^s-oob]; 117 khớp ví dụ paging[^p-result]; 4194304 byte = 4 MB khớp phép tính page table[^ostep-pt-size]. Đây là mô hình tối giản của sách, không phải một CPU cụ thể.

## Tóm tắt

- Hardware-based address translation: phần cứng đổi virtual address thành physical address cho *mỗi* truy cập; OS chỉ cấu hình phần cứng và can thiệp ở các thời điểm then chốt[^ostep-translation][^m-os].
- Base and bounds: `PA = VA + base`, kiểm tra `VA < bounds`; cặp thanh ghi là per-CPU nên OS lưu/khôi phục khi context switch; lệnh sửa chúng là đặc quyền[^m-formula][^m-ctx][^m-priv].
- Base/bounds lãng phí vùng trống giữa stack và heap (internal fragmentation)[^ostep-bb-waste].
- Segmentation: một cặp base/bounds mỗi segment, stack cần bit chiều tăng, protection bits cho phép chia sẻ code; đổi lại là external fragmentation mà compaction và thuật toán free-list chỉ giảm chứ không chữa dứt điểm[^ostep-seg][^ostep-seg-bit][^ostep-seg-prot][^ostep-seg-ext-always].
- Paging: page/frame cố định, page table per-process ánh xạ VPN → PFN, offset giữ nguyên; PTE mang thêm bit valid/present, protection, dirty, accessed[^ostep-paging][^ostep-pt][^p-offset][^ostep-pte-bits].
- Paging không gây external fragmentation và cho phép address space thưa, nhưng bảng lớn (4 MB mỗi process ở 32-bit/4 KB) và mỗi truy cập tốn thêm một lần đọc RAM — đề bài của [chương 03](03-page-table-va-tlb.md)[^ostep-page-noext][^ostep-pt-size][^p-extra].

## Câu hỏi tự kiểm tra

1. Process 4 KB nạp tại 16 KB: địa chỉ ảo 3000 và 4400 cho kết quả gì?
<details><summary>Đáp án</summary>

3000 → 19384; 4400 ≥ 4096 nên fault[^m-formula][^m-example-table].

</details>

2. Vì sao OS phải lưu/khôi phục base/bounds khi context switch?
<details><summary>Đáp án</summary>

Mỗi CPU chỉ có một cặp thanh ghi mà giá trị khác nhau cho mỗi process; OS lưu vào PCB khi dừng và nạp lại khi chạy tiếp[^m-ctx][^m-ctx2].

</details>

3. Dịch virtual address 4200 trong ví dụ segmentation; vì sao không cộng thẳng với base?
<details><summary>Đáp án</summary>

Offset = 4200 − 4096 = 104, cộng base 34 KB ra 34920; cộng thẳng ra 39016 là sai[^s-heap].

</details>

4. Vì sao stack cần bit chiều tăng, và 15 KB dịch ra đâu?
<details><summary>Đáp án</summary>

Stack tăng về địa chỉ thấp nên dùng offset âm: 3 KB − 4 KB = −1 KB, cộng base 28 KB ra 27 KB[^ostep-seg-stack][^ostep-seg-bit][^s-negative].

</details>

5. External fragmentation là gì và vì sao compaction không phải lời giải tốt?
<details><summary>Đáp án</summary>

RAM đầy lỗ hổng nhỏ nên khó cấp/nới segment (24 KB trống ở ba mảnh không thỏa yêu cầu 20 KB); compaction tốn nhiều CPU và bộ nhớ để copy[^ostep-seg-ext][^s-20kb][^s-compact].

</details>

6. Dịch VA = 50 với page 16 byte và page table VP0→3, VP1→7, VP2→5, VP3→2.
<details><summary>Đáp án</summary>

50 = `110010`: VPN = 3, offset = 2, VP3 → frame 2, nên PA = 2×16 + 2 = 34 (cách tính theo ví dụ 21 → 117[^p-result]; đã chạy trong mô phỏng ở phần Liên hệ với Rust).

</details>

7. Page table 32-bit, page 4 KB, PTE 4 byte lớn bao nhiêu và hai vấn đề nào dẫn tới chương 03?
<details><summary>Đáp án</summary>

2²⁰ PTE × 4 byte = 4 MB mỗi process; bảng quá lớn và mỗi tham chiếu tốn thêm một lần đọc PTE từ RAM[^p-32bit][^ostep-pt-size][^p-extra].

</details>

8. Vì sao "segmentation fault" còn tên đó?
<details><summary>Đáp án</summary>

Thuật ngữ sinh ra trên máy có segmentation và vẫn tồn tại trên máy không có[^ostep-segfault].

</details>

## Đọc thêm

- [OSTEP ch.15 — Address Translation](https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf): hình timeline "@ boot"/"@ runtime"; bài tập `relocation.py`.
- [OSTEP ch.16 — Segmentation](https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf): thêm ví dụ số, bài tập `segmentation.py`.
- [OSTEP ch.18 — Paging: Introduction](https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf): trace bộ nhớ của vòng lặp mảng (Hình 18.7).
- [Chương 03](03-page-table-va-tlb.md): lời giải cho hai vấn đề cuối chương; [chương 04](04-page-fault-va-swapping.md): bit present khi page không ở RAM.

[^m-eff]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "Efficiency dictates that we make use of hardware support"
[^m-ctrl]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "Control implies that the OS ensures that no application is allowed to access any memory but its own"
[^m-os]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "The OS must get involved at key points to set up the hardware so that the correct translations take place"
[^m-example-seq]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "Execute this instruction (load from address 15 KB)"
[^m-problem]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "the OS wants to place the process somewhere else in physical memory, not necessarily at address 0."
[^ostep-translation]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "the hardware transforms each memory access (e.g., an instruction fetch, load, or store), changing the virtual address provided by the instruction to a physical address where the desired information is actually located."
[^ostep-bb]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "Introduced in the first time-sharing machines of the late 1950’s is a simple idea referred to as base and bounds; the technique is also referred to as dynamic relocation"
[^m-two-regs]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "we’ll need two hardware registers within each CPU: one is called the base register, and the other the bounds (sometimes called a limit register)."
[^m-load0]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "each program is written and compiled as if it is loaded at address zero."
[^m-formula]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "physical address = virtual address + base"
[^m-trace128]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "it first adds the value to the base register value of 32 KB (32768) to get a physical address of 32896"
[^m-trace15k]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "adds to the base register (32 KB), getting the final physical address of 47 KB"
[^ostep-bounds]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "the bounds register is there to help with protection."
[^ostep-bounds-exc]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "If a process generates a virtual address that is greater than (or equal to) the bounds, or one that is negative, the CPU will raise an exception, and the process will likely be terminated."
[^m-bound-two]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "Both methods are logically equivalent"
[^m-example-table]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "Imagine a process with an address space of size 4 KB (yes, unrealistically small) has been loaded at physical address 16 KB."
[^m-mmu]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "Sometimes people call the part of the processor that helps with address translation the memory management unit (MMU)"
[^m-onchip]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "the base and bounds registers are hardware structures kept on the chip (one pair per CPU)."
[^m-modes]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "The OS runs in privileged mode (or kernel mode), where it has access to the entire machine; applications run in user mode, where they are limited in what they can do."
[^m-priv]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "These instructions are privileged; only in kernel (or privileged) mode can the registers be modified."
[^m-exc]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "if a user program tries to change the values of the (privileged) base and bounds registers, the CPU should raise an exception"
[^m-slots]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "given our assumptions that each address space is (a) smaller than the size of physical memory and (b) the same size, this is quite easy for the OS; it can simply view physical memory as an array of slots"
[^m-free]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "the OS will have to search a data structure (often called a free list) to find room for the new address space and then mark it used."
[^m-reclaim]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "Upon termination of a process, the OS thus puts its memory back on the free list"
[^m-ctx]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "the OS must save and restore the base-and-bounds pair when it switches between processes."
[^m-ctx2]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "it must save the values of the base and bounds registers to memory, in some per-process structure such as the process structure or process control block (PCB)."
[^m-move]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "it is possible for the OS to move an address space from one location in memory to another rather easily."
[^m-handlers]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "the OS installs these handlers at boot time (via privileged instructions)."
[^m-nohw]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "note how its memory translations are handled by the hardware with no OS intervention."
[^m-lde]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "only when the process misbehaves does the OS have to become involved."
[^ostep-bb-waste]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "because the process stack and heap are not too big, all of the space between the two is simply wasted. This type of waste is usually called internal fragmentation"
[^s-32bit]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "a typical program will only use megabytes of memory, but still would demand that the entire address space be resident in memory."
[^s-old]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "going at least as far back as the very early 1960’s"
[^ostep-seg]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "why not have a base and bounds pair per logical segment of the address space?"
[^s-def]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "A segment is just a contiguous portion of the address space of a particular length"
[^s-regs]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "a set of three base and bounds register pairs"
[^s-size]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "it tells the hardware exactly how many bytes are valid in this segment"
[^ostep-seg-sparse]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "allowing us to fit more address spaces into physical memory and support a large and sparse virtual address space per process."
[^s-trans100]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "to arrive at the desired physical address: 100 + 32KB, or 32868"
[^s-heap]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "the offset of 4200 is actually 4200 minus 4096, or 104"
[^s-oob]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "the hardware detects that the address is out of bounds, traps into the OS, likely leading to the termination of the offending process."
[^s-explicit]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "chop up the address space into segments based on the top few bits of the virtual address"
[^s-bounds-off]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "we can simply check if the offset is less than the bounds"
[^s-maxsize]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "each segment is limited to a maximum size, which in our example is 4KB"
[^s-implicit]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "If, for example, the address was generated from the program counter"
[^ostep-seg-stack]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "it grows backwards (i.e., towards lower addresses)."
[^ostep-seg-bit]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "the hardware also needs to know which way the segment grows"
[^s-negative]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "We simply add the negative offset (-1KB) to the base (28KB) to arrive at the correct physical address: 27KB."
[^s-negbound]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "ensuring the absolute value of the negative offset is less than or equal to"
[^s-protbits]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "Basic support adds a few bits per segment, indicating whether or not a program can read or write a segment, or perhaps execute code that lies within the segment."
[^ostep-seg-prot]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "By setting a code segment to read-only, the same code can be shared across multiple processes, without worry of harming isolation"
[^s-protcheck]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "If a user process tries to write to a read-only segment, or execute from a non-executable segment, the hardware should raise an exception"
[^s-ctx]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "the segment registers must be saved and restored."
[^s-sbrk]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "the memory-allocation library will perform a system call to grow the heap"
[^s-reject]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "the OS could reject the request, if no more physical memory is available, or if it decides that the calling process already has too much."
[^ostep-seg-ext]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "physical memory quickly becomes full of little holes of free space, making it difficult to allocate new segments, or to grow existing ones. We call this problem external fragmentation"
[^s-20kb]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "there is 24KB free, but not in one contiguous segment (rather, in three non-contiguous chunks). Thus, the OS cannot satisfy the 20KB request."
[^s-compact]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "compaction is expensive, as copying segments is memory-intensive and generally uses a fair amount of processor time"
[^ostep-seg-ext-always]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "no matter how smart the algorithm, external fragmentation will still exist; a good algorithm attempts to minimize it."
[^s-onlyreal]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "The only real solution (as we will see in forthcoming chapters) is to avoid the problem altogether, by never allocating memory in variable-sized chunks."
[^s-fast]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "It is also fast, as doing the arithmetic segmentation requires is easy and well-suited to hardware"
[^s-share]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "A fringe benefit arises too: code sharing."
[^s-notflex]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "if we have a large but sparsely-used heap all in one logical segment, the entire heap must still reside in memory in order to be accessed."
[^ostep-segfault]: OSTEP, ch.16 Segmentation, aside — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "The term segmentation fault or violation arises from a memory access on a segmented machine to an illegal address. Humorously, the term persists, even on machines with no support for segmentation at all."
[^p-two-ways]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "to chop up space into fixed-sized pieces."
[^p-atlas]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "it goes back to an early and important system, the Atlas"
[^ostep-paging]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "we divide it into fixed-sized units, each of which we call a page."
[^ostep-frame]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "we view physical memory as an array of fixed-sized slots called page frames; each of these frames can contain a single virtual-memory page."
[^p-example]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "only 64 bytes total in size, with four 16-byte pages"
[^p-placement]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "the OS has placed virtual page 0 of the address space (AS) in physical frame 3, virtual page 1 of the AS in physical frame 7, page 2 in frame 5, and page 3 in frame 2."
[^p-flex]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "make assumptions about the direction the heap and stack grow and how they are used"
[^ostep-page-simple]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "Another advantage is the simplicity of free-space management that paging affords."
[^p-freelist]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "perhaps the OS keeps a free list of all free pages for this"
[^ostep-pt]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "the operating system usually keeps a per-process data structure known as a page table."
[^p-pt4]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "the page table would thus have the following four entries"
[^p-perproc]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "most page table structures we discuss are per-process structures"
[^p-pt-variants]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "The exact structure of the page table is either determined by the hardware (older systems) or can be more flexibly managed by the OS (modern systems)."
[^ostep-vpn]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "we have to first split it into two components: the virtual page number (VPN), and the offset within the page."
[^p-bits]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "because the virtual address space of the process is 64 bytes, we need 6 bits total for our virtual address"
[^p-vpn-bits]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "we need to be able to select 4 pages, and the top 2 bits of the address do just that."
[^p-pfn]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "the physical frame number (PFN) (also sometimes called the physical page number or PPN) is 7"
[^p-result]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "Our final physical address is 1110101 (117 in decimal)"
[^p-offset]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "the offset stays the same (i.e., it is not translated)"
[^p-32bit]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "splits into a 20-bit VPN and 12-bit offset"
[^p-mask]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "PhysAddr = (PTE.PFN << PFN_SHIFT) | offset"
[^p-linear]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "The simplest form is called a linear page table, which is just an array."
[^p-valid]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "the valid bit is crucial for supporting a sparse address space"
[^p-prot]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "We also might have protection bits, indicating whether the page could be read from, written to, or executed from."
[^p-present]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "A present bit indicates whether this page is in physical memory or on disk"
[^p-dirty]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "A dirty bit is also common, indicating whether the page has been modified since it was brought into memory."
[^p-ref]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "A reference bit (a.k.a. accessed bit) is sometimes used to track whether a page has been accessed"
[^ostep-pte-bits]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "It contains a present bit (P); a read/write bit (R/W) which determines if writes are allowed to this page; a user/supervisor bit (U/S) which determines if user-mode processes can access the page"
[^p-nobit]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "If that bit is set (P=1), it means the page is both present and valid."
[^p-big]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "Page tables can get terribly large, much bigger than the small segment table or base/bounds pair we have discussed previously."
[^ostep-pt-size]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "assuming we need 4 bytes per page table entry (PTE) to hold the physical translation plus any other useful stuff, we get an immense 4MB of memory needed for each page table!"
[^p-100]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "Now imagine there are 100 processes running: this means the OS would need 400MB of memory just for all those address translations!"
[^p-inmem]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "Instead, we store the page table for each process in memory somewhere."
[^p-ptbr]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "a single page-table base register contains the physical address of the starting location of the page table."
[^p-formula]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "PTEAddr = PageTableBaseRegister + (VPN * sizeof(PTE))"
[^p-extra]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "paging requires us to perform one extra memory reference in order to first fetch the translation from the page table."
[^p-slow2]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "will likely slow down the process by a factor of two or more."
[^p-trace]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "there are 10 memory accesses per loop, which includes four instruction fetches, one explicit update of memory, and five page table accesses"
[^p-two-prob]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "Without careful design of both hardware and software, page tables will cause the system to run too slowly, as well as take up too much memory."
[^ostep-page-noext]: OSTEP, ch.18 Paging: Introduction (Summary) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "First, it does not lead to external fragmentation, as paging (by design) divides memory into fixed-sized units. Second, it is quite flexible, enabling the sparse use of virtual address spaces."
[^ostep-paging-slow]: OSTEP, ch.18 Paging: Introduction (Summary) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "implementing paging support without care will lead to a slower machine (with many extra memory accesses to access the page table) as well as memory waste"
[^m-track]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "it must thus manage memory, keeping track of which locations are free and which are in use"
[^m-example-seq2]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "Fetch the instruction at address 135"
[^m-table-fault]: OSTEP, ch.15 Mechanism: Address Translation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-mechanism.pdf> — "Only if the virtual address is “too big” or negative will the result be a fault, causing an exception to be raised."
[^s-table]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "the code segment is placed at physical address 32KB and has a size of 2KB and the heap segment is placed at 34KB and has a size of 3KB."
[^s-stack-base]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "The stack has been relocated to physical address 28KB in the diagram above"
[^s-unused]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "one segment of the address space goes unused."
[^s-compact-grow]: OSTEP, ch.16 Segmentation — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-segmentation.pdf> — "Compaction also (ironically) makes requests to grow existing segments hard to serve"
[^p-fig186]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "else if (CanAccess(PTE.ProtectBits) == False) RaiseException(PROTECTION_FAULT)"
[^p-segv-pte]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "if (PTE.Valid == False) RaiseException(SEGMENTATION_FAULT)"
[^l-maperr]: sigaction(2), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/sigaction.2.en.html> — "Address not mapped to object."
[^l-accerr]: sigaction(2), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/sigaction.2.en.html> — "Invalid permissions for mapped object."
[^p-internal]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "big pages lead to waste within each page, a problem known as internal fragmentation"
[^p-smallpg]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "most systems use relatively small page sizes in the common case: 4KB (as in x86)"
