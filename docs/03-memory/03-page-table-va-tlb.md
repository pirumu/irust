# P3 · Chương 03 — Page table nhiều cấp và TLB

## Bạn sẽ hiểu được gì

1. **Q1.** TLB là gì, hit/miss khác nhau thế nào, và vì sao thiếu TLB thì paging quá chậm? → mục 1
2. **Q2.** Khi TLB miss, ai đi tra page table — phần cứng hay phần mềm — và đánh đổi gì? → mục 2
3. **Q3.** Vì sao context switch làm TLB "mất tác dụng", ASID giải quyết ra sao, và TLB đầy thì đuổi entry nào? → mục 3
4. **Q4.** Vì sao page table tuyến tính quá lớn, và các cách thu nhỏ (page lớn, hybrid, inverted) được gì, mất gì? → mục 4
5. **Q5.** Page table nhiều cấp thu nhỏ bảng bằng cách nào, và trả giá gì? → mục 5
6. **Q6.** Một virtual address trên x86-64 và AArch64 được cắt qua bao nhiêu cấp bảng, Linux đặt tên các cấp thế nào? → mục 6
7. **Q7.** Huge page và THP là gì, vì sao giúp TLB, và giá phải trả là gì? → mục 7
8. **Q8.** Khi nào TLB miss thành vấn đề thật với workload backend? → mục 8

## Kiến thức cần trước

- [Chương 01 — Address space](./01-address-space.md) và [Chương 02 — Dịch địa chỉ](./02-address-translation.md): virtual address, VPN/offset, PFN, ý tưởng paging.
- Giúp ích: [Cache và NUMA](../02-cpu/06-cache-va-numa.md) (TLB cũng là một cache, dựa vào locality).

**Ví dụ chạy suốt chương.** Hãy hình dung `catalog`, một service viết bằng Rust, giữ trong RAM một chỉ mục sản phẩm cỡ 20 GB (một `HashMap` khổng lồ). Mỗi request, một worker thread trong thread pool tra chỉ mục này theo key. Cùng máy còn chạy database và vài process khác. Đây là ví dụ giả định của người soạn, không phải code trong repo, và mọi con số của `catalog` đều là số minh họa. Câu hỏi bám theo chương: mỗi lần một worker đọc một phần tử của chỉ mục, CPU tốn gì để dịch địa chỉ, và khi nào khoản tốn đó đủ lớn để làm p99 của `catalog` xấu đi?

---

## 1. TLB: cache của các bản dịch (Q1)

> **Trả lời ngắn.** Với paging, mỗi lần truy cập bộ nhớ đều phải tra page table trước. Làm không cẩn thận thì máy chậm đi vì thêm rất nhiều lần truy cập bộ nhớ[^ostep-paging-slow]. TLB là cách phần cứng tránh chuyện đó: một cache giữ các bản dịch địa chỉ hay dùng[^ostep-tlb]. Có sẵn bản dịch trong TLB thì gọi là hit[^ostep-tlbhit]. Không có thì gọi là miss, và phần cứng phải làm thêm việc[^ostep-tlbmiss-cost].

**Giải thích.**

*Với service `catalog`.* Khi worker chạy `index.get(&key)`, thứ CPU thấy là vài lệnh load trên virtual address. Trước khi RAM trả được dữ liệu, CPU phải biết virtual address đó ứng với physical address nào. Nếu lần tra này luôn phải đi vào RAM thì mọi lookup của `catalog` đều chậm gấp đôi. Mục này nói TLB chặn chuyện đó ra sao, và khi nó chặn hụt thì chuyện gì xảy ra.

Từ [chương 02](./02-address-translation.md) bạn cần nhớ ba điều. Page table là cấu trúc riêng của từng process. Virtual address tách thành VPN và offset. Mỗi PTE ngoài PFN còn có các bit như valid/present và protection, nên quyền truy cập kiểm được theo từng page (suy luận từ các bit đó).

### 1.1 Paging để lại hai vấn đề

Paging cho `catalog` một address space riêng, nhưng nó kéo theo hai vấn đề: **chậm** (phải đọc thêm page table cho mỗi truy cập) và **to** (bản thân page table chiếm bộ nhớ). Chương này lo chuyện chậm bằng TLB (mục 1–3), lo chuyện to bằng các cấu trúc bảng nhỏ hơn, chủ yếu là nhiều cấp (mục 4–6). Huge page (mục 7) giúp được cả hai.

### 1.2 TLB chặn lần đọc thêm bằng cách nào

Nếu mỗi load/store phải đọc PTE từ RAM trước, mỗi truy cập của `catalog` thành ít nhất hai lần đọc: paging đòi thêm một tham chiếu bộ nhớ chỉ để lấy bản dịch[^ostep-one-extra]. Vì vậy phần cứng đặt thêm TLB ngay trong MMU, bộ phận trên chip lo việc dịch địa chỉ. TLB chỉ đơn giản là một cache phần cứng giữ các bản dịch virtual-to-physical hay dùng[^ostep-tlb]. Ngoài TLB, MMU còn có thể dùng các *page walk cache* để tăng tốc việc dịch[^kdoc-mmu-tlb].

Với mỗi virtual address, phần cứng làm theo hai bước:

1. Rút VPN ra và tra TLB. Nếu có bản dịch thì đó là **TLB hit**[^ostep-tlbhit]: ghép PFN với offset, và chỉ tốn một lần truy cập RAM cho chính dữ liệu cần đọc.
2. Nếu không có thì đó là **TLB miss**. MMU phải *page walk*, tức đi từ đầu page table xuống tới PTE để xác định physical address[^kdoc-walk]. Nếu tham chiếu hợp lệ, phần cứng nạp bản dịch vừa tìm vào TLB[^ostep-tlbmiss-update]. Sau đó nó chạy lại lệnh đang dở[^ostep-tlbmiss-retry]. Cả quá trình này tốn kém vì phải thêm truy cập bộ nhớ[^ostep-tlbmiss-cost].

```mermaid
sequenceDiagram
  participant CPU
  participant TLB
  participant PT as Page table (RAM)
  participant OS as Kernel (fault handler)
  CPU->>TLB: VPN?
  alt TLB hit
    TLB-->>CPU: PFN + quyền
  else TLB miss
    TLB->>PT: tra VPN (nhiều cấp = nhiều lần đọc RAM)
    alt PTE hợp lệ & present
      PT-->>TLB: nạp bản dịch, thử lại lệnh
    else không hợp lệ / không present / sai quyền
      PT-->>OS: trap (page fault)
    end
  end
```

Có một điểm dễ nhầm: TLB miss **không** phải page fault. Miss chỉ có nghĩa là bản dịch chưa nằm trong cache. Nếu page đang present thì page walk lấy được bản dịch và không có fault nào cả. Fault là nhánh dưới của sơ đồ, nói ở [chương 04](./04-page-fault-va-swapping.md). Với `catalog`, TLB miss là chuyện thường ngày của từng lookup chứ chưa phải dấu hiệu có lỗi (nhận xét của người soạn).

<details><summary>Đi sâu: ví dụ hit rate của OSTEP, locality, TLB reach và Culler's Law</summary>

- **Ví dụ hit rate.** Duyệt tuần tự một mảng nhỏ cho chuỗi truy cập: miss, hit, hit, miss, hit, hit, hit, miss, hit, hit, tức hit rate 70%[^ostep-hitrate]. Lần đầu chạm mảng mà TLB vẫn có ích nhờ spatial locality: chỉ truy cập đầu tiên trên mỗi page mới miss[^ostep-tlb-spatial]. Như mọi cache, TLB cần cả spatial lẫn temporal locality, và đó là tính chất của chương trình chứ không phải của TLB[^ostep-tlb-cache-locality].
- **Culler's Law.** Truy cập ngẫu nhiên, nhất là khi số page chạm vượt *TLB coverage*, gây tổn thất hiệu năng nặng[^ostep-culler]. *TLB reach* (suy luận của người soạn) là số entry N nhân kích thước page S. Với N = 1536 (số minh họa, không phải CPU cụ thể): page 4 KB phủ 6144 KiB, page 2 MiB phủ 3072 MiB (phép nhân, đã chạy bằng rustc 1.95.0). Đem so với chỉ mục 20 GB của `catalog` thì thấy ngay vì sao lookup ngẫu nhiên là chỗ TLB hay hụt.

</details>

**Quay lại Q1:** TLB là cache các bản dịch. Hit tránh được lần đọc page table, miss thì phải page walk, và miss chưa phải page fault.

---

## 2. Phần cứng hay phần mềm xử lý miss? (Q2)

> **Trả lời ngắn.** Tùy kiến trúc. Trên x86, phần cứng tự đi tra page table nhiều cấp[^ostep-hw-tlb]. Ở kiểu software-managed, phần cứng chỉ phát exception và nhảy vào trap handler của kernel[^ostep-sw-tlb]. Bù lại, OS được dùng cấu trúc page table tùy ý[^ostep-sw-flex].

**Giải thích.**

*Với service `catalog`.* Mỗi lần một worker của `catalog` dính TLB miss, phải có ai đó đi tra bảng. Câu hỏi của mục này là ai tra, và việc đó ràng buộc OS vào phần cứng đến mức nào. Ở kiểu đơn giản nhất, phần cứng lo trọn việc đó mà không cần kernel chen vào; kiểu còn lại thì kernel phải chạy code để xử lý từng miss.

### 2.1 Hardware-managed: phần cứng tự tra

Muốn xử lý trọn một miss, phần cứng phải biết chính xác page table nằm ở đâu trong RAM (qua một thanh ghi) và định dạng của nó ra sao[^ostep-hw-know]. x86 thỏa điều kiện đó. Page table của nó nhiều cấp với định dạng cố định, địa chỉ của bảng hiện hành nằm trong thanh ghi CR3, và phần cứng tự page walk[^ostep-hw-tlb].

### 2.2 Software-managed: OS tra hộ

Kiểu còn lại đẩy việc cho OS. Khi miss, phần cứng chỉ phát một exception: nó tạm dừng luồng lệnh, nâng lên kernel mode rồi nhảy tới trap handler[^ostep-sw-tlb]. Handler tra page table, nạp TLB bằng các lệnh đặc quyền rồi return-from-trap để phần cứng chạy lại lệnh[^ostep-sw-priv].

Hai kiểu đánh đổi nhau như sau:

| | Hardware-managed | Software-managed |
|---|---|---|
| Khi miss | phần cứng tự tra | exception, OS tra |
| Được | OS không phải viết và chạy handler cho miss (suy luận của người soạn) | OS dùng cấu trúc page table tùy ý[^ostep-sw-flex]; phần cứng đơn giản, chỉ phát exception rồi để OS làm nốt[^ostep-sw-simple] |
| Mất | cấu trúc bảng do phần cứng quy định[^ostep-hw-know] | phải cẩn thận, ví dụ không để chính handler gây chuỗi miss vô hạn[^ostep-inf-chain] |

<details><summary>Đi sâu: handler của OS, ví dụ CPU, và page fault thì khác</summary>

- Handler ở kiểu software-managed là code OS viết riêng cho việc xử lý TLB miss[^ostep-sw-handler]. OSTEP nêu MIPS R10k và SPARC v9 là ví dụ kiến trúc software-managed[^ostep-sw-arch].
- Page fault thì khác TLB miss: gần như mọi hệ thống xử lý page fault bằng phần mềm, kể cả khi TLB do phần cứng quản lý[^ostep-pf-sw]. Lý do và luồng từng bước nằm ở [chương 04, mục 1](./04-page-fault-va-swapping.md).

</details>

**Quay lại Q2:** x86 để phần cứng tự tra. Software-managed đổi sự phức tạp lấy tính linh hoạt của OS. Page fault thì (hầu như) luôn do phần mềm xử lý.

---

## 3. Context switch, ASID và replacement (Q3)

> **Trả lời ngắn.** Bản dịch trong TLB chỉ đúng cho process đang chạy[^ostep-tlb-ctx]. Vì vậy khi đổi process có hai lựa chọn. Một là xóa hết entry (flush), việc này đắt vì process mới phải chịu miss lại[^ostep-flush-cost]. Hai là gắn thêm số nhận diện address space (ASID) vào mỗi entry để nhiều process cùng nằm trong TLB[^ostep-asid-field]. Khi TLB đầy thì đuổi một entry, thường theo LRU[^ostep-lru]. Cách khác là chọn ngẫu nhiên[^ostep-random].

**Giải thích.**

*Với service `catalog`.* Máy chạy `catalog` còn chạy database và vài process khác, nên một core liên tục chuyển qua lại giữa chúng. TLB chỉ giữ các cặp "trang ảo → frame", trong khi mỗi process có page table riêng. Vậy khi core vừa chạy worker của `catalog` xong rồi chuyển sang database, những bản dịch `catalog` đã nạp vào TLB phải xử lý ra sao?

### 3.1 Vì sao entry cũ gây hại

Bản dịch trong TLB chỉ đúng cho process đang chạy, với process khác chúng vô nghĩa[^ostep-tlb-ctx]. Ví dụ của OSTEP, đặt lại tên cho khớp `catalog`: trang ảo số 10 của `catalog` ánh xạ tới frame 100, còn trang ảo số 10 của database ánh xạ tới frame 170. Nếu TLB giữ cả hai entry VPN 10 thì phần cứng không phân biệt được entry nào dành cho process nào[^ostep-asid-example]. Có hai hướng xử lý.

### 3.2 Hướng 1: flush

Cách đơn giản nhất là flush TLB mỗi lần đổi process, tức làm rỗng nó trước khi chạy process kế tiếp[^ostep-flush]. Cụ thể, hệ thống đặt mọi entry thành invalid để process sắp chạy không vô tình dùng bản dịch của process trước[^ostep-asid]. Giá phải trả là khi process chạm lại dữ liệu và code của nó, mọi thứ lại miss. Nếu OS đổi process thường xuyên thì cái giá này cao[^ostep-flush-cost]. Với `catalog`, nghĩa là mỗi lần core quay lại chạy worker, các lookup đầu tiên phải nạp lại bản dịch từ page table (suy luận của người soạn).

### 3.3 Hướng 2: ASID

Cách thứ hai là phần cứng thêm một trường address-space identifier (ASID) vào mỗi entry của TLB[^ostep-asid-field]. Bạn có thể coi ASID như một PID ngắn hơn, ví dụ 8 bit so với 32 bit của PID[^ostep-asid-pid]. Nhờ vậy TLB giữ được bản dịch của nhiều process cùng lúc (suy luận từ việc mỗi entry mang ASID riêng). Khi chuyển process, OS đặt một thanh ghi đặc quyền thành ASID của process sắp chạy để phần cứng biết đang dịch cho ai[^ostep-asid-reg].

### 3.4 TLB đầy thì đuổi entry nào

TLB có số entry hữu hạn, nên khi cần chỗ cho bản dịch mới thì phải đuổi một entry cũ. Một cách thường gặp là đuổi entry ít được dùng gần đây nhất (LRU, least-recently-used)[^ostep-lru]. Cách khác là đuổi một entry bất kỳ (random)[^ostep-random].

<details><summary>Đi sâu: nội dung entry, ASID 8 bit, global bit, LRU với n + 1 page</summary>

- **Nội dung entry.** Một TLB điển hình (theo OSTEP) có 32, 64 hoặc 128 entry và fully associative: bản dịch nằm ở đâu cũng được, phần cứng tìm song song[^ostep-tlb-size]. Entry có VPN, PFN và các bit khác như valid, protection, ASID, dirty[^ostep-tlb-entry]. Lưu ý: valid bit của TLB **không** phải valid bit của page table, và OSTEP gọi nhầm lẫn này là "a common mistake"[^ostep-valid-mistake]. Con số của một CPU cụ thể phải tra tài liệu của vendor.
- **ASID 8 bit.** Có hơn 256 process thì sao? OSTEP nêu đây là câu hỏi để ngỏ[^ostep-asid-256].
- **Global bit.** Ví dụ MIPS có thêm *global bit* cho page dùng chung giữa các process. Khi bật, ASID bị bỏ qua[^ostep-global].
- **Góc chết của LRU.** Vòng lặp qua n + 1 page với TLB n entry khiến LRU miss ở **mọi** truy cập, còn random tốt hơn nhiều[^ostep-lru-bad].

</details>

**Quay lại Q3:** entry TLB chỉ đúng cho một address space, nên đổi process thì phải flush hoặc dùng ASID. TLB đầy thì đuổi entry theo LRU hay random.

---

## 4. Page table quá lớn và bốn hướng thu nhỏ (Q4)

> **Trả lời ngắn.** Bảng tuyến tính cho address space 32-bit, page 4 KB, PTE 4 byte nặng 4 MB cho **mỗi** process[^ostep-pt-size], dù phần lớn entry là invalid. Có bốn hướng thu nhỏ: page lớn hơn, hybrid paging + segmentation, inverted page table và nhiều cấp. Page lớn hơn bị internal fragmentation[^ostep-internal-frag]. Hybrid kế thừa nhược điểm của segmentation[^ostep-hybrid-1]. Inverted page table phải tìm kiếm[^ostep-inverted-hash]. Nhiều cấp là hướng thắng thế, nói ở mục 5.

**Giải thích.**

*Với service `catalog`.* Mục 1–3 lo cho `catalog` khỏi chậm. Còn chuyện to thì tự nó là một bài toán: page table cũng là bộ nhớ, mà process nào cũng phải có một bảng. Phép tính của người soạn cho thấy bảng tuyến tính ngây thơ vô lý đến mức nào với `catalog` chạy 64-bit: address space 48 bit với page 4 KB có 2³⁶ page, PTE 8 byte như ở mục 6, nên bảng tuyến tính nặng 2³⁶ × 8 byte = 512 GiB cho mỗi process, dù `catalog` chỉ dùng một phần rất nhỏ.

### 4.1 Con số 4 MB

Ví dụ của OSTEP dùng address space 32-bit cho gọn, và con số 4 MB là phép nhân đã làm ở [chương 02, mục 7](./02-address-translation.md). Với page 4 KB, VPN dài 20 bit, tức 2²⁰ PTE, mỗi PTE 4 byte[^ostep-pt-size]. Phần lớn bảng này không dùng, toàn entry invalid[^ostep-mostly-invalid]. Cả bốn hướng dưới đây cùng giảm số entry hoặc phần bảng phải giữ (suy luận của người soạn), mỗi hướng bằng một cách khác và trả một giá khác.

### 4.2 Bốn hướng thu nhỏ

**Page lớn hơn.** Đây là cách đơn giản nhất[^ostep-bigger]. Giữ address space 32-bit nhưng tăng page lên 16 KB, VPN còn 18 bit cộng offset 14 bit[^ostep-bigger-18]. Bảng nhỏ đi bốn lần, đúng bằng hệ số tăng của page[^ostep-bigger-4x]. Giá phải trả: mỗi page cấp ra mà chỉ dùng một phần thì phần còn lại bị bỏ phí, gọi là internal fragmentation[^ostep-internal-frag].

**Hybrid paging + segmentation.** Thay vì một page table cho cả address space, mỗi segment logic (code, heap, stack) có một page table riêng[^ostep-hybrid]. MMU dùng lại cặp thanh ghi của segmentation nhưng đổi nghĩa: base giữ địa chỉ vật lý của page table của segment, còn bounds cho biết bảng có bao nhiêu page hợp lệ[^ostep-hybrid-regs]. Nhờ vậy khoảng trống giữa stack và heap không còn tốn entry[^ostep-hybrid-save]. Giá phải trả có hai. Một là vẫn dính segmentation vốn kém linh hoạt[^ostep-hybrid-1]. Hai là bảng giờ có kích thước tùy ý, nên tìm chỗ đặt chúng trong RAM khó hơn[^ostep-hybrid-free]. Hệ quả là external fragmentation quay lại[^ostep-hybrid-2].

**Inverted page table.** Hệ thống chỉ giữ một bảng duy nhất, mỗi *physical* page một entry[^ostep-inverted]. Entry ghi process nào đang dùng page đó và virtual page nào của process ánh xạ vào[^ostep-inverted-entry]. Khi đó tra bản dịch thành bài toán tìm kiếm. Quét tuyến tính thì đắt, nên người ta thường dựng thêm một hash table để tra nhanh[^ostep-inverted-hash]. Kích thước bảng tỉ lệ với RAM vật lý chứ không với số process (suy luận từ định nghĩa).

**Nhiều cấp.** Hướng thắng thế, nói ở mục 5.

Chốt lại, page table chỉ là cấu trúc dữ liệu: làm nhỏ hay lớn, nhanh hay chậm đều được[^ostep-just-ds].

<details><summary>Đi sâu: cỡ page thực tế</summary>

- Vì internal fragmentation, phần lớn hệ thống dùng page nhỏ: 4 KB (x86) hoặc 8 KB (SPARCv9)[^ostep-small-pages].
- OSTEP có một aside: nhiều kiến trúc (MIPS, SPARC, x86-64) hỗ trợ nhiều cỡ page, nhưng lý do chính không phải tiết kiệm chỗ cho bảng mà là giảm áp lực lên TLB[^ostep-multi-pagesize]. Đó là chủ đề của mục 7.

</details>

**Quay lại Q4:** bảng tuyến tính tốn 4 MB mỗi process trong ví dụ 32-bit. Mỗi cách thu nhỏ đổi một giá khác: fragmentation, segmentation, tìm kiếm, hoặc thêm lần đọc ở mục 5.

---

## 5. Page table nhiều cấp (Q5)

> **Trả lời ngắn.** Thay vì một mảng liền khối, bảng được chia thành các đoạn dài bằng một page và xếp thành thứ giống cái cây[^ostep-mlpt]. Đoạn nào toàn entry invalid thì không cấp phát, và một page directory (bảng chỉ mục) ghi nhớ đoạn nào còn tồn tại[^ostep-pdir]. Nhờ vậy bảng chỉ chiếm chỗ tỉ lệ với phần address space đang dùng[^ostep-ml-adv]. Giá phải trả là khi TLB miss phải đọc bộ nhớ hai lần thay vì một[^ostep-ml-cost].

**Giải thích.**

*Với service `catalog`.* Address space của `catalog` rộng, nhưng phần nó thật sự dùng (code, heap, stack, vài vùng mmap, trong đó có chỉ mục khổng lồ) gồm vài khối rải rác, giữa chúng là khoảng trống lớn (nhận xét của người soạn). Bảng liền khối buộc phải có entry cho cả khoảng trống đó. Nhiều cấp cho phép bỏ qua nó. Đổi lại, mỗi lần một worker của `catalog` dính TLB miss thì page walk dài hơn.

### 5.1 Cắt bảng

Bảng tuyến tính là một mảng PTE rất dài. Ta cắt nó thành các đoạn dài đúng bằng một page. Một *page directory* ghi lại đoạn nào đã được cấp phát và nằm ở đâu. Đoạn nào toàn PTE invalid thì bỏ luôn, không chiếm RAM[^ostep-pdir].

Entry của directory (PDE) có valid bit và PFN như PTE, nhưng valid ở đây mang nghĩa hơi khác. PDE valid nghĩa là ít nhất một PTE trong page bảng mà nó trỏ tới là valid, còn PDE invalid thì phần còn lại của nó không có nghĩa[^ostep-pde]. Directory cần một entry cho mỗi page của bảng, và ta lấy một số bit cao của VPN làm chỉ số vào directory[^ostep-index].

### 5.2 TLB miss đi hai bước

Khi `catalog` dính TLB miss trên bảng hai cấp, phần cứng đi hai bước:

1. Dùng các bit cao của VPN đọc PDE trong directory (lần đọc RAM thứ nhất). Nếu PDE invalid thì không có page bảng nào để đọc tiếp (suy ra từ định nghĩa trên).
2. Nếu PDE valid, PFN của nó cho biết page bảng nằm đâu. Dùng các bit còn lại của VPN chọn PTE trong page đó (lần đọc thứ hai). PTE cho PFN của page dữ liệu, rồi ghép với offset như ở bảng tuyến tính. Công thức đầy đủ và ví dụ làm tay nằm ở phần Đi sâu.

### 5.3 Lợi và giá

Bảng giờ chiếm chỗ tỉ lệ với phần address space đang dùng[^ostep-ml-adv]. Ngoài ra các page của bảng đặt đâu trong RAM cũng được, không cần một khối liền, vì directory thêm một tầng gián tiếp chỉ tới từng mảnh của bảng[^ostep-indirection]. Giá phải trả là mỗi miss cần hai lần đọc bộ nhớ thay vì một[^ostep-ml-cost]. Đó là đổi thời gian lấy không gian: TLB hit không khác gì trước, còn TLB miss đắt hơn[^ostep-timespace]. Với address space lớn như của `catalog`, hai cấp có thể không đủ[^ostep-more-levels], và ta cần thêm cấp (mục 6).

<details><summary>Đi sâu: ví dụ làm tay 0x3F80, tính tiết kiệm, page table bị swap</summary>

**Ví dụ làm tay (OSTEP).** Address space 16 KB, page 64 byte: địa chỉ 14 bit gồm 8 bit VPN và 6 bit offset, bảng tuyến tính có 256 entry[^ostep-example]. Page directory có 16 entry, nên dùng 4 bit cao của VPN làm chỉ số vào directory và 4 bit sau làm chỉ số vào page của bảng[^ostep-index]. Dịch `0x3F80` từng bước:

1. Viết 14 bit: `0x3F80` = `11 1111 1000 0000`.
2. 6 bit thấp `000000` là offset = 0. 8 bit cao `1111 1110` là VPN = 254. Kiểm lại: 254 × 64 = 16256 = `0x3F80`, nên đây đúng là byte đầu của page 254[^ostep-translate-start].
3. Lấy 4 bit đầu của VPN, `1111`, làm chỉ số vào directory: entry thứ 15 (đếm từ 0), trỏ tới page của bảng nằm ở PFN 101[^ostep-translate-pde].
4. Lấy 4 bit sau, `1110`, làm chỉ số vào page bảng đó: entry 14, cho biết page 254 nằm ở physical page 55 (`0x37`)[^ostep-translate-result].
5. Ghép PFN 55 với offset: 55 × 64 = 3520 = `0x0DC0`, tức `00 1101 1100 0000`[^ostep-translate-result].

Số 101 và 55 là nội dung do OSTEP chọn trong hình 20.5, không suy ra được từ địa chỉ. Công thức chung: `PTEAddr = (PDE.PFN << SHIFT) + (PTIndex * sizeof(PTE))`[^ostep-formulas].

**Tiết kiệm** (tính của người soạn từ hình 20.4: code VPN 0–1, heap 4–5, stack 254–255). VPN 0–5 nằm chung một page bảng, VPN 254–255 nằm ở một page khác. Cần 2 page bảng + 1 directory = 3 × 64 = 192 byte, so với 1 KB của bảng tuyến tính.

**Page table bị swap.** Bản thân page table cũng có thể bị swap nếu nằm trong kernel virtual memory[^ostep-pt-swap].

</details>

**Quay lại Q5:** nhiều cấp chỉ cấp phát phần bảng đang dùng, đổi lại mỗi TLB miss ở bảng hai cấp phải đọc RAM hai lần thay vì một.

---

## 6. Page table của Linux, x86-64 và AArch64 (Q6)

> **Trả lời ngắn.** Linux mô hình hóa page table thành hệ phân cấp hiện cao **năm cấp**: `pgd` → `p4d` → `pud` → `pmd` → `pte`. Mỗi kiến trúc ánh xạ mô hình đó vào giới hạn phần cứng của mình[^kdoc-five]. Cấp nào kiến trúc không dùng thì bị bỏ qua lúc biên dịch (fold)[^kdoc-fold]. Trên x86-64 với địa chỉ 48 bit, virtual address cắt thành bốn chỉ số 9 bit cộng offset 12 bit (người soạn tính, bên dưới). AArch64 cho tối đa 4 cấp với page 4 KB[^arm-levels].

**Giải thích.**

*Với service `catalog`.* Mỗi lần worker của `catalog` dính TLB miss trên máy thật, phần cứng đi qua đúng các cấp bảng của mục này. Vì vậy mục này trả lời hai câu hỏi cụ thể: lần miss đó đọc RAM tối đa mấy lần, và khi đọc tài liệu kernel thì mỗi tên `pgd`, `pud`, `pmd` ứng với cấp nào.

### 6.1 Mô hình của Linux

Phân cấp tồn tại để tiết kiệm bộ nhớ cho page table và tăng tốc việc ánh xạ[^kdoc-why]. Một lỗ lớn trong address space chỉ cần đánh dấu unmapped ở cấp cao[^kdoc-holes]. Các cấp, từ trên xuống, là `pgd` → `p4d` → `pud` → `pmd` → `pte`. Có bốn chi tiết cần nhớ:

- Mỗi process có một `pgd` riêng, nằm trong `struct mm_struct`, và mỗi `task_struct` trỏ tới `mm_struct` của nó[^kdoc-pgd].
- `pte` là *bảng* thấp nhất chứ không phải một entry. Tên này gây nhầm vì lịch sử[^kdoc-pte-name].
- `p4d` chỉ dùng trên hệ 5 cấp thật, nếu không thì bị folded[^kdoc-p4d]. Fold nghĩa là cấp đó bị bỏ qua lúc biên dịch[^kdoc-fold].
- Một entry cấp cao có thể trỏ thẳng tới một vùng vật lý liền khối, ánh xạ vài MB hay thậm chí GB bằng một entry[^kdoc-huge-entry]. Đó là cơ sở của huge page ở mục 7.

### 6.2 x86-64: cắt địa chỉ 48 bit

Với page 4 KB, mỗi bảng chiếm đúng một page và mỗi entry dài 8 byte, nên một bảng có 512 entry và cần 9 bit để chọn entry[^x86-pte8]. Địa chỉ 48 bit vì vậy cắt thành bốn chỉ số 9 bit và offset 12 bit: 48 = 4 × 9 + 12 (phép tính của người soạn). Đã chạy bằng rustc 1.95.0 với `va = 0x0000_7f12_3456_789a`:

```rust
let (pgd, pud, pmd, pt) = ((va >> 39) & 0x1ff, (va >> 30) & 0x1ff, (va >> 21) & 0x1ff, (va >> 12) & 0x1ff);
let off = va & 0xfff;
```

Kết quả in ra là `pgd=254 pud=72 pmd=418 pte=359 off=0x89a`. Vậy một TLB miss của `catalog` có thể cần **bốn** lần đọc, mở rộng từ "hai lần cho hai cấp" ở mục 5 (suy luận của người soạn)[^ostep-ml-cost]. Đó là chặn trên chứ không phải thời gian đo, vì page walk cache và cache dữ liệu giúp giảm số lần đọc thật[^kdoc-mmu-tlb]. Hệ 5 cấp thêm 9 bit (57 = 5 × 9 + 12) và thêm một lần đọc, cũng là chặn trên.

### 6.3 AArch64

Kiến trúc cho tối đa 4 cấp với page 4 KB và tối đa 3 cấp với page 64 KB[^arm-levels]. Linux dùng 3–4 cấp với page 4 KB (địa chỉ 39 hoặc 48 bit), và 2 cấp với page 64 KB (42 bit)[^arm-vabits].

<details><summary>Đi sâu: 48 so với 57 bit, hố non-canonical, TTBR0/TTBR1, nguồn của PTE 8 byte và khác biệt giữa các phiên bản docs</summary>

- **x86-64: địa chỉ 48 hay 57 bit.** Kiến trúc định nghĩa địa chỉ 64 bit, hiện hỗ trợ 48 và 57 bit. Các bit cao được sign-extend, nên khi đọc như số không dấu thì có một hố giữa user và kernel[^x86-sign]. Hố non-canonical đó, cùng direct mapping của toàn bộ RAM vật lý trong nửa kernel, được trình bày ở [chương 01, mục 5](./01-address-space.md). Layout 5 cấp mở rộng user space lên 512 lần, từ 0.125 PB lên 64 PB[^x86-5lvl-user].
- **56 hay 57 bit?** Chỗ dễ nhầm: địa chỉ ảo 5 cấp là **57 bit** (kernel docs ghi 128 PiB không gian ảo[^x86-5lvl-total]). Nhưng vì bit cao được sign-extend nên mỗi nửa (user, kernel) chỉ có 56 bit = 64 PiB. Câu "5 cấp cho phép không gian địa chỉ ảo userspace 56 bit" trong kernel docs[^x86-5lvl-56] nói về nửa user, và dải `0` đến `00ffffffffffffff` trong ví dụ bản đồ "56-bit" khớp với điều đó. Tương tự với 4 cấp: 48 bit tổng cộng (256 TiB[^x86-4lvl-total]), mỗi nửa 47 bit = 128 TiB = 0.125 PiB (suy luận của người soạn từ phép chia đôi; khớp ranh giới `00007fffffffffff` trong bảng bản đồ).
- Mặc định kernel không cấp địa chỉ user vượt 47 bit, trừ khi `mmap` có hint cao hơn[^x86-5lvl-hint].
- **Nguồn của "PTE 8 byte".** Tài liệu giảng dạy của TU Graz nêu mỗi entry của các bảng x86-64 dài 8 byte[^x86-pte8]. Kernel docs chỉ nói `pteval_t` là giá trị 32 hoặc 64 bit tùy kiến trúc[^kdoc-pteval]. Chưa trích được Intel SDM vì trang tải về bị chặn.
- **AArch64: TTBR.** Khác x86-64, một bit cao của virtual address chọn TTBR0 hay TTBR1. `swapper_pg_dir` chỉ chứa ánh xạ kernel và được ghi vào TTBR1, còn `pgd` của user chứa ánh xạ user. Bit nào chọn thì tài liệu kernel ghi khác nhau theo phiên bản: docs hiện hành trên docs.kernel.org ghi bit 55[^arm-ttbr]. Bản trong source kernel v6.8 (kernel của máy lab) lại ghi bit 63[^arm-ttbr-v68]. Mặc định kernel trả địa chỉ userspace trong dải 48 bit; muốn 52 bit phải opt-in qua hint `mmap`[^arm-52].
- Chương này không khẳng định kích thước TLB hay ASID cụ thể của Intel, AMD hay Arm.

</details>

**Quay lại Q6:** Linux gọi các cấp là pgd, p4d, pud, pmd, pte. x86-64 48 bit dùng bốn cấp 9 bit + offset 12 bit, AArch64 dùng 3–4 cấp với page 4 KB.

---

## 7. Huge page và Transparent Huge Pages (Q7)

> **Trả lời ngắn.** Huge page là page lớn hơn 4 KB, trên x86 thường là 2M, và 1G nếu CPU hỗ trợ[^huge-sizes]. TLB có số entry hữu hạn, nên page lớn làm mỗi entry phủ được nhiều bộ nhớ hơn và ít miss hơn[^thp-f2b]. Giá phải trả có hai. THP có thể cấp page 2M cho vùng chỉ chạm vài byte, lãng phí bộ nhớ[^thp-waste]. Và ở cấu hình `defrag=always`, ứng dụng có thể bị stall khi kernel dồn bộ nhớ để cấp page lớn[^thp-defrag-always].

**Giải thích.**

*Với service `catalog`.* Chỉ mục 20 GB của `catalog` nằm trong hơn 5 triệu page 4 KB, và mỗi lookup ngẫu nhiên chạm một page khác (20 GiB / 4 KiB = 5 242 880; phép chia của người soạn). Nếu dùng page 2 MiB thì cùng vùng nhớ đó chỉ còn 10 240 page (phép chia của người soạn), tức TLB phải theo dõi ít page hơn 512 lần. Mục này nói huge page làm được điều đó ra sao, và `catalog` phải trả giá gì.

### 7.1 Vì sao phải có huge page

TLB là tài nguyên rất khan hiếm trên bộ xử lý, và kernel docs nhấn mạnh việc dùng TLB hiệu quả càng quan trọng khi RAM ngày càng lớn[^huge-scarce]. Số entry hữu hạn, nên muốn tăng reach chỉ còn cách tăng kích thước page (suy luận của người soạn). Nhưng mục 4 đã cho thấy page lớn có giá, và OSTEP nói lý do chính để hỗ trợ nhiều cỡ page là giảm áp lực lên TLB chứ không phải tiết kiệm chỗ cho bảng[^ostep-multi-pagesize]. Huge page chấp nhận cái giá đó có chọn lọc.

Con số 2 MB và 1 GB đến từ mục 6. Với 9 bit mỗi cấp, một entry PMD phủ 2 MB (512 × 4 KB) và một entry PUD phủ 1 GB (suy luận của người soạn). Đó chính là cơ chế entry cấp cao trỏ thẳng vào vùng vật lý[^kdoc-huge-entry]. Page walk cũng ngắn đi một cấp (suy luận của người soạn).

### 7.2 hugetlbfs: huge page tường minh

Ứng dụng dùng hugetlbfs qua `mmap` hoặc qua shared memory SysV[^huge-use]. Huge page loại này bị reserve trong kernel, không dùng cho việc khác và **không bị swap**[^huge-reserved].

### 7.3 THP: huge page tự động

THP (Transparent Huge Pages) thì kernel tự nâng và hạ kích thước page, không có nhược điểm của hugetlbfs[^thp-obj]. Hiện nó chỉ áp dụng cho anonymous mapping và tmpfs/shmem[^thp-scope]. Ứng dụng không cần sửa gì để hưởng lợi[^thp-nomod]. Với `catalog` đó là điểm đáng giá: bật được mà không đụng code.

Về lợi ích, kernel docs nêu hai yếu tố. Yếu tố ít quan trọng là mỗi vùng 2M chỉ cần một page fault thay vì 512, và chỉ ở lần chạm đầu[^thp-f1]. Yếu tố quan trọng và kéo dài là TLB miss chạy nhanh hơn, đặc biệt với ảo hóa dùng nested page table và "almost always" cả trên bare metal[^thp-f2a]. Lý do là một TLB entry phủ lượng virtual memory lớn hơn nên ít miss hơn[^thp-f2b].

### 7.4 Giá phải trả

- **Lãng phí bộ nhớ.** Khi huge page bật toàn hệ thống, một process mmap vùng lớn rồi chỉ chạm 1 byte vẫn có thể được cấp page 2M thay vì 4k[^thp-waste].
- **Clear/copy page lớn hơn** trong lúc xử lý fault[^thp-clear].
- **Stall.** Với `defrag=always`, ứng dụng yêu cầu THP sẽ stall khi cấp phát thất bại: nó trực tiếp reclaim page và compact bộ nhớ để cố cấp ngay một THP[^thp-defrag-always]. Service nhạy latency như `catalog` nên cân nhắc điểm này (nhận xét của người soạn).

<details><summary>Đi sâu: reserve và boot với hugetlbfs, các nút điều khiển THP, quan sát trên máy soạn chương</summary>

- **hugetlbfs.** Reserve bảo đảm cấp được huge page lúc fault[^huge-rsvd]. Cấp lúc boot là cách đáng tin nhất vì bộ nhớ chưa bị phân mảnh[^huge-boot].
- **Điều khiển THP.** THP bật được toàn hệ thống hoặc giới hạn theo task hay vùng nhớ[^thp-control]. Có các chế độ `always`, `madvise`, `never`[^thp-modes]. Ứng dụng hưởng lợi nên gọi `madvise(MADV_HUGEPAGE)` cho các vùng quan trọng[^thp-madvise]. `khugepaged` gộp các page thường thành huge page nếu hệ thống cấu hình cho phép[^thp-khuge]. mTHP cấp khối nhỏ hơn cỡ PMD, để page fault bớt gây latency spike so với THP cỡ PMD[^thp-mthp].
- **Quan sát trên máy soạn chương** (một lần chạy, không phải bảo đảm): container Linux của repo (`./labs/linux.sh`, aarch64, page 4096) cho `enabled` = `always [madvise] never`, `defrag` = `always defer defer+madvise [madvise] never`, `AnonHugePages: 0 kB`, `HugePages_Total: 0`, `Hugepagesize: 2048 kB`. Máy của bạn có thể khác: hãy đọc các file đó trên chính máy cần xét.

</details>

**Quay lại Q7:** huge page tăng lượng bộ nhớ mỗi entry TLB phủ được. hugetlbfs dành riêng RAM, còn THP tự động nhưng có thể phình bộ nhớ và gây stall.

---

## 8. TLB miss với workload backend (Q8)

> **Trả lời ngắn.** Chỉ đáng nghi khi số page khác nhau mà chương trình chạm trong thời gian ngắn vượt TLB coverage, kịch bản mà OSTEP gọi là Culler's Law[^ostep-culler]. Vì vậy hãy đo bộ đếm TLB miss trước khi chỉnh THP hay hugetlbfs (nhận xét của người soạn).

**Giải thích.**

*Với service `catalog`.* Đây là mục trả lời câu hỏi thực tế của cả chương: p99 của `catalog` xấu đi khi chỉ mục lớn lên, vậy có phải do TLB không? Toàn bộ mục là nhận xét của người soạn; chỉ câu nào có chú thích mới là sự thật có nguồn.

### 8.1 Khi nào TLB thành vấn đề

Worker của `catalog` tra chỉ mục theo hash, nên hai key kế nhau rơi vào hai page khác nhau. Truy cập ngẫu nhiên kiểu này mất lợi thế "một miss cho mỗi page" của truy cập tuần tự[^ostep-tlb-spatial]. Khi số page chạm trong thời gian ngắn vượt TLB coverage thì ta rơi vào kịch bản Culler's Law[^ostep-culler]. Một scan tuần tự qua chính chỉ mục đó thì ổn. Các ứng viên dễ gặp ở backend cùng kiểu: cache in-process hàng GB truy cập theo hash, index trong bộ nhớ, buffer pool.

### 8.2 Miss đắt hơn vẻ ngoài

Mỗi miss là cả một page walk nhiều cấp, và các lần đọc của walk đi qua cache như mọi truy cập khác (suy luận của người soạn từ mục 6).

### 8.3 Đo trước khi chỉnh

Hãy dùng bộ đếm TLB miss phần cứng, ví dụ qua `perf stat`. Tên sự kiện phụ thuộc CPU, nên tra `perf list` trên máy bạn; chương này không chạy lệnh đó nên không có số liệu. Sau đó mới thử THP hay hugetlbfs và so p50/p99 của `catalog` trước và sau.

<details><summary>Đi sâu: ảo hóa và rủi ro vận hành</summary>

- **Ảo hóa.** Huge page đặc biệt có lợi với nested page table: chỉ một bên (KVM hoặc guest) dùng huge page đã tăng tốc đáng kể[^thp-f2a]. Chạy trong VM (đa số cloud) thì áp dụng.
- **Rủi ro vận hành.** Có ba thứ cần canh. Thứ nhất là stall khi compaction[^thp-defrag-always]. Thứ hai là phình bộ nhớ[^thp-waste]. Thứ ba là hugetlbfs dành riêng RAM nên không swap được[^huge-reserved]. Trong container, chi phí này tính vào giới hạn bộ nhớ (chưa kiểm chứng ở đây).

</details>

**Quay lại Q8:** nghi TLB khi working set truy cập ngẫu nhiên vượt coverage. Hãy đo bộ đếm trước, rồi mới thử huge page, và để ý rủi ro vận hành.

---

## Liên hệ với Rust

Rust không có "API TLB". Điểm chạm gián tiếp là nơi chỉ mục của `catalog` lấy bộ nhớ: nếu nó nằm trong `Vec` hay `Box` lớn thì bộ nhớ đó đến từ global allocator, mà allocator mặc định hiện không được đặc tả[^alloc-global]. Vùng nhớ đó có thành anonymous mapping, thứ THP áp dụng được[^thp-scope], hay không là việc của allocator thực tế; xem [chương 06](./06-heap-allocator.md). Suy luận của người soạn: với cấu trúc dữ liệu khổng lồ truy cập ngẫu nhiên, chỗ cải thiện nằm ở bố cục dữ liệu hoặc cấu hình hệ thống (THP, `madvise` qua crate như `libc`, chưa kiểm chứng ở đây) chứ không ở `std`.

---

## Tóm tắt

- TLB là cache phần cứng của bản dịch[^ostep-tlb]. TLB miss không phải page fault.
- Miss do phần cứng xử lý trên x86[^ostep-hw-tlb]. Kiểu software-managed (MIPS, SPARC v9) để phần mềm xử lý[^ostep-sw-arch]. Phần mềm đổi lấy sự linh hoạt[^ostep-sw-flex].
- Context switch: flush TLB thì tốn miss lại[^ostep-flush-cost]. Dùng ASID thì nhiều process cùng nằm trong TLB[^ostep-asid-field].
- LRU có tình huống góc (n + 1 page, n entry), random tránh được[^ostep-lru-bad].
- Bảng tuyến tính 32-bit/4 KB nặng 4 MB mỗi process[^ostep-pt-size]. Page lớn đổi lấy internal fragmentation, hybrid kế thừa segmentation, inverted phải tìm kiếm, nhiều cấp thêm lần đọc khi miss[^ostep-timespace].
- Linux: PGD, P4D, PUD, PMD, PTE, và fold cấp không dùng[^kdoc-five]. x86-64 là 48/57 bit, AArch64 là 39/48 bit (4 KB) hay 42 bit (64 KB)[^arm-vabits].
- Huge page: một entry phủ nhiều hơn, page walk ngắn hơn[^thp-f2b]. hugetlbfs dành riêng RAM[^huge-reserved]. THP tự động nhưng có thể phình bộ nhớ[^thp-waste].
- Với `catalog` và các backend giống nó: chỉ nghi TLB khi working set truy cập ngẫu nhiên vượt coverage[^ostep-culler]. Đo trước khi chỉnh.

---

## Câu hỏi tự kiểm tra

1. TLB miss khác page fault thế nào?

<details><summary>Đáp án</summary>

Miss là chưa có bản dịch trong cache: phần cứng page walk rồi nạp TLB[^ostep-tlbmiss-update]. Sau đó nó thử lại lệnh[^ostep-tlbmiss-retry]. Fault là page không present hoặc sai quyền, và nó vào handler của OS[^ostep-pf-sw].

</details>

2. Hardware- vs software-managed TLB? Ưu điểm của kiểu phần mềm?

<details><summary>Đáp án</summary>

Hardware-managed: phần cứng tự page walk, ví dụ x86 với CR3[^ostep-hw-tlb]. Software-managed: khi miss phần cứng chỉ phát exception và OS xử lý[^ostep-sw-tlb]. Ưu điểm của kiểu phần mềm là linh hoạt: OS dùng được cấu trúc page table tùy ý[^ostep-sw-flex].

</details>

3. Hai cách xử lý TLB khi context switch, và giá của mỗi cách?

<details><summary>Đáp án</summary>

Cách một là flush[^ostep-asid]. Giá của nó là process mới miss lại từ đầu[^ostep-flush-cost]. Cách hai là ASID[^ostep-asid-field]. Giá của nó là ASID có ít bit hơn PID[^ostep-asid-pid].

</details>

4. Vòng lặp qua n + 1 page với TLB n entry: LRU và random?

<details><summary>Đáp án</summary>

LRU miss mọi truy cập, random tốt hơn nhiều[^ostep-lru-bad].

</details>

5. Bảng tuyến tính 32-bit, page 4 KB, PTE 4 byte nặng bao nhiêu? Page 16 KB thì sao, giá là gì?

<details><summary>Đáp án</summary>

Nặng 4 MB mỗi process[^ostep-pt-size]. Với page 16 KB thì bảng còn 1 MB, vì page lớn gấp bốn lần[^ostep-bigger]. Giá là internal fragmentation[^ostep-internal-frag].

</details>

6. Vì sao bảng hai cấp nhỏ hơn nhưng miss đắt hơn?

<details><summary>Đáp án</summary>

Page bảng toàn invalid thì không cấp phát[^ostep-pdir]. Nhờ vậy bảng chỉ chiếm chỗ tỉ lệ với phần address space đang dùng[^ostep-ml-adv]. Nhưng miss cần hai lần đọc bộ nhớ[^ostep-ml-cost]. Hit thì không khác gì trước[^ostep-timespace].

</details>

7. Dịch `0x3F80` trong ví dụ OSTEP.

<details><summary>Đáp án</summary>

PD index 15, PT index 14, PFN 55[^ostep-index]. Physical address là `0x0DC0`[^ostep-translate-result].

</details>

8. Các cấp page table của Linux và việc fold?

<details><summary>Đáp án</summary>

Các cấp là PGD, P4D, PUD, PMD, PTE[^kdoc-five]. Cấp nào không dùng thì bị fold[^kdoc-fold]. P4D chỉ dùng trên hệ 5 cấp thật[^kdoc-p4d].

</details>

9. Vì sao huge page giúp TLB? Giá phải trả?

<details><summary>Đáp án</summary>

Miss chạy nhanh hơn[^thp-f2a]. Một entry cũng phủ nhiều bộ nhớ hơn[^thp-f2b]. Giá phải trả là lãng phí bộ nhớ[^thp-waste]. Ngoài ra có thể stall khi compaction[^thp-defrag-always]. hugetlbfs còn có thêm nhược điểm là không swap được[^huge-reserved].

</details>

10. `catalog` giữ cache 20 GB truy cập theo hash, p99 xấu dù CPU không bão hòa. Chẩn đoán TLB thế nào?

<details><summary>Đáp án</summary>

Theo mục 8 (nhận xét của người soạn): đây đúng là kịch bản Culler's Law[^ostep-culler]. Hãy đo bộ đếm TLB miss trước. Sau đó thử `madvise(MADV_HUGEPAGE)` cho vùng cache[^thp-madvise] hoặc hugetlbfs, và chú ý rủi ro stall và phình bộ nhớ.

</details>

## Đọc thêm

- [OSTEP ch.19 — TLBs](https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf): thuật toán TLB, ASID, replacement, entry MIPS thật.
- [OSTEP ch.20 — Smaller Tables](https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf): ví dụ nhiều cấp làm tay, hybrid, inverted.
- [Kernel docs — Page Tables](https://docs.kernel.org/mm/page_tables.html): mô hình 5 cấp, folding.
- [Kernel docs — x86_64 mm](https://docs.kernel.org/arch/x86/x86_64/mm.html) và [AArch64 memory](https://docs.kernel.org/arch/arm64/memory.html): bản đồ địa chỉ thật.
- [Kernel docs — THP](https://docs.kernel.org/admin-guide/mm/transhuge.html) và [HugeTLB](https://docs.kernel.org/admin-guide/mm/hugetlbpage.html): chế độ vận hành.
- [Chương 04 — Page fault](./04-page-fault-va-swapping.md): nhánh fault của sơ đồ mục 1.

[^ostep-paging-slow]: OSTEP, ch.18 Paging: Introduction (Summary) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "implementing paging support without care will lead to a slower machine (with many extra memory accesses to access the page table) as well as memory waste"
[^ostep-tlb]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "A TLB is part of the chip’s memory-management unit (MMU), and is simply a hardware cache of popular virtual-to-physical address translations"
[^ostep-tlbhit]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "If it does, we have a TLB hit, which means the TLB holds the translation."
[^ostep-tlbmiss-cost]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "If the CPU does not find the translation in the TLB (a TLB miss), we have some more work to do. In this example, the hardware accesses the page table to find the translation"
[^ostep-tlbmiss-update]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "assuming that the virtual memory reference generated by the process is valid and accessible (Lines 13, 15), updates the TLB with the translation"
[^ostep-tlbmiss-retry]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "once the TLB is updated, the hardware retries the instruction"
[^kdoc-mmu-tlb]: Linux kernel docs, Page Tables (MMU, TLB, and Page Faults) — <https://docs.kernel.org/mm/page_tables.html> — "It may use relatively small caches in hardware called Translation Lookaside Buffers (TLBs) and Page Walk Caches to speed up these translations."
[^kdoc-walk]: Linux kernel docs, Page Tables (MMU, TLB, and Page Faults) — <https://docs.kernel.org/mm/page_tables.html> — "If no translation is found, MMU uses the page walks to determine the physical address and create the map."
[^ostep-hitrate]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "miss, hit, hit, miss, hit, hit, hit, miss, hit, hit. Thus, our TLB hit rate, which is the number of hits divided by the total number of accesses, is 70%."
[^ostep-tlb-spatial]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "The elements of the array are packed tightly into pages (i.e., they are close to one another in space), and thus only the first access to an element on a page yields a TLB miss."
[^ostep-tlb-cache-locality]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "Like any cache, TLBs rely upon both spatial and temporal locality for success, which are program properties."
[^ostep-hw-tlb]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "which uses a fixed multi-level page table (see the next chapter for details); the current page table is pointed to by the CR3 register"
[^ostep-sw-tlb]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "On a TLB miss, the hardware simply raises an exception (line 11 in Figure 19.3), which pauses the current instruction stream, raises the privilege level to kernel mode, and jumps to a trap handler."
[^ostep-sw-handler]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "this trap handler is code within the OS that is written with the express purpose of handling TLB misses."
[^ostep-sw-flex]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "The primary advantage of the software-managed approach is flexibility: the OS can use any data structure it wants to implement the page"
[^ostep-inf-chain]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "the OS needs to be extra careful not to cause an infinite chain of TLB misses to occur."
[^ostep-pf-sw]: OSTEP, ch.21 Beyond Physical Memory: Mechanisms — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-beyondphys.pdf> — "Virtually all systems handle page faults in software; even with a hardware-managed TLB, the hardware trusts the OS to manage this important duty."
[^ostep-tlb-size]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "A typical TLB might have 32, 64, or 128 entries and be what is called fully associative."
[^ostep-valid-mistake]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "A common mistake is to confuse the valid bits found in a TLB with those found in a page table."
[^ostep-tlb-ctx]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "the TLB contains virtual-to-physical translations that are only valid for the currently running process; these translations are not meaningful for other processes."
[^ostep-flush]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "One approach is to simply flush the TLB on context switches, thus emptying it before running the next process."
[^ostep-asid]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "By setting all TLB entries to invalid, the system can ensure that the about-to-be-run process does not accidentally use a virtual-to-physical translation from a previous process."
[^ostep-flush-cost]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "If the OS switches between processes frequently, this cost may be high."
[^ostep-asid-field]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "some hardware systems provide an address space identifier (ASID) field in the TLB."
[^ostep-asid-pid]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "You can think of the ASID as a process identifier (PID), but usually it has fewer bits (e.g., 8 bits for the ASID versus 32 bits for a PID)."
[^ostep-asid-256]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "One question for you: what should the OS do if there are more than 256 (28 ) processes running at a time?"
[^ostep-global]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "We see a global bit (G), which is used for pages that are globally-shared among processes. Thus, if the global bit is set, the ASID is ignored."
[^ostep-lru]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "One common approach is to evict the least-recently-used or LRU entry."
[^ostep-random]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "Another typical approach is to use a random policy, which evicts a TLB mapping at random."
[^ostep-lru-bad]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "behaves quite unreasonably when a program loops over n + 1 pages with a TLB of size n; in this case, LRU misses upon every access, whereas random does much better."
[^ostep-culler]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "Sometimes randomly accessing your address space, particularly if the number of pages accessed exceeds the TLB coverage, can lead to severe performance penalties."
[^ostep-pt-size]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "assuming we need 4 bytes per page table entry (PTE) to hold the physical translation plus any other useful stuff, we get an immense 4MB of memory needed for each page table!"
[^ostep-bigger]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "We could reduce the size of the page table in one simple way: use bigger pages."
[^ostep-internal-frag]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "The major problem with this approach, however, is that big pages lead to waste within each page, a problem known as internal fragmentation (as the waste is internal to the unit of allocation)."
[^ostep-small-pages]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "Thus, most systems use relatively small page sizes in the common case: 4KB (as in x86) or 8KB (as in SPARCv9)."
[^ostep-hybrid]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "instead of having a single page table for the entire address space of the process, why not have one per logical segment?"
[^ostep-hybrid-1]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "First, it still requires us to use segmentation; as we discussed before, segmentation is not quite as flexible as we would like"
[^ostep-hybrid-2]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "Second, this hybrid causes external fragmentation to arise again."
[^ostep-inverted]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "we keep a single page table that has an entry for each physical page of the system."
[^ostep-inverted-hash]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "A linear scan would be expensive, and thus a hash table is often built over the base structure to speed up lookups."
[^ostep-just-ds]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "page tables are just data structures."
[^ostep-mlpt]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "We call this approach a multi-level page table, as it turns the linear page table into something like a tree. This approach is so effective that many modern systems employ it (e.g., x86"
[^ostep-pdir]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "if an entire page of page-table entries (PTEs) is invalid, don’t allocate that page of the page table at all. To track whether a page of the page table is valid (and if valid, where it is in memory), use a new structure, called the page directory."
[^ostep-ml-adv]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "the multi-level table only allocates page-table space in proportion to the amount of address space you are using; thus it is generally compact and supports sparse address spaces."
[^ostep-indirection]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "With a multi-level structure, we add a level of indirection through use of the page directory, which points to pieces of the page table; that indirection allows us to place page-table pages wherever we would like in physical memory."
[^ostep-example]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "Imagine a small address space of size 16KB, with 64-byte pages. Thus, we have a 14-bit virtual address space, with 8 bits for the VPN and 6 bits for the offset."
[^ostep-index]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "The page directory needs one entry per page of the page table; thus, it has 16 entries. As a result, we need four bits of the VPN to index into the directory; we use the top four bits of the VPN"
[^ostep-translate-result]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "PhysAddr = (PTE.PFN << SHIFT) + offset = 00 1101 1100 0000 = 0x0DC0."
[^ostep-formulas]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "PTEAddr = (PDE.PFN << SHIFT) + (PTIndex * sizeof(PTE))"
[^ostep-ml-cost]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "on a TLB miss, two loads from memory will be required to get the right translation information from the page table"
[^ostep-timespace]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "We wanted smaller tables (and got them), but not for free; although in the common case (TLB hit), performance is obviously identical, a TLB miss suffers from a higher cost with this smaller table."
[^ostep-pt-swap]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "Thus, some systems place such page tables in kernel virtual memory, thereby allowing the system to swap some of these page tables to disk when memory pressure gets a little tight."
[^kdoc-five]: Linux kernel docs, Page Tables — <https://docs.kernel.org/mm/page_tables.html> — "Linux defines page tables as a hierarchy which is currently five levels in height. The architecture code for each supported architecture will then map this to the restrictions of the hardware."
[^kdoc-why]: Linux kernel docs, Page Tables — <https://docs.kernel.org/mm/page_tables.html> — "The software page table hierarchy reflects the fact that page table hardware has become hierarchical and that in turn is done to save page table memory and speed up mapping."
[^kdoc-holes]: Linux kernel docs, Page Tables — <https://docs.kernel.org/mm/page_tables.html> — "By using hierarchical page tables large holes in the virtual address space does not waste valuable page table memory, because it will suffice to mark large areas as unmapped at a higher level in the page table hierarchy."
[^kdoc-pgd]: Linux kernel docs, Page Tables — <https://docs.kernel.org/mm/page_tables.html> — "each userspace process in the system also has its own memory context and thus its own pgd, found in struct mm_struct which in turn is referenced to in each struct task_struct."
[^kdoc-pte-name]: Linux kernel docs, Page Tables — <https://docs.kernel.org/mm/page_tables.html> — "so the pte is the lowermost page table, not a page table entry."
[^kdoc-p4d]: Linux kernel docs, Page Tables — <https://docs.kernel.org/mm/page_tables.html> — "This is only used on systems which actually have 5 levels of page tables, otherwise it is folded."
[^kdoc-fold]: Linux kernel docs, Page Tables (Page Table Folding) — <https://docs.kernel.org/mm/page_tables.html> — "If the architecture does not use all the page table levels, they can be folded which means skipped"
[^kdoc-huge-entry]: Linux kernel docs, Page Tables — <https://docs.kernel.org/mm/page_tables.html> — "a higher level page table entry can point directly to a physical memory range, which allows mapping a contiguous range of several megabytes or even gigabytes in a single high-level page table entry"
[^kdoc-pteval]: Linux kernel docs, Page Tables — <https://docs.kernel.org/mm/page_tables.html> — "A typical example is that the pteval_t is a 32- or 64-bit value with the upper bits being a pfn (page frame number)"
[^x86-sign]: Linux kernel docs, x86_64 Memory Management — <https://docs.kernel.org/arch/x86/x86_64/mm.html> — "Currently supported are 48- and 57-bit virtual addresses. Bits 63 through to the most-significant implemented bit are sign extended."
[^x86-5lvl-user]: Linux kernel docs, x86_64 Memory Management (5-level) — <https://docs.kernel.org/arch/x86/x86_64/mm.html> — "With 56-bit addresses, user-space memory gets expanded by a factor of 512x, from 0.125 PB to 64 PB."
[^arm-levels]: Linux kernel docs, Memory Layout on AArch64 Linux — <https://docs.kernel.org/arch/arm64/memory.html> — "The architecture allows up to 4 levels of translation tables with a 4KB page size and up to 3 levels with a 64KB page size."
[^arm-vabits]: Linux kernel docs, Memory Layout on AArch64 Linux — <https://docs.kernel.org/arch/arm64/memory.html> — "allowing 39-bit (512GB) or 48-bit (256TB) virtual addresses, respectively, for both user and kernel. With 64KB pages, only 2 levels of translation tables, allowing 42-bit (4TB) virtual address, are used"
[^arm-ttbr]: Linux kernel docs, Memory Layout on AArch64 Linux — <https://docs.kernel.org/arch/arm64/memory.html> — "TTBRx selection is given by bit 55 of the virtual address. The swapper_pg_dir contains only kernel (global) mappings while the user pgd contains only user (non-global) mappings."
[^arm-ttbr-v68]: Linux kernel docs, Memory Layout on AArch64 Linux (source tag v6.8) — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/arch/arm64/memory.rst> — "TTBRx selection is given by bit 63 of the virtual address."
[^arm-52]: Linux kernel docs, Memory Layout on AArch64 Linux (52-bit userspace VAs) — <https://docs.kernel.org/arch/arm64/memory.html> — "the kernel will, by default, return virtual addresses to userspace from a 48-bit range."
[^huge-scarce]: Linux kernel docs, HugeTLB Pages — <https://docs.kernel.org/admin-guide/mm/hugetlbpage.html> — "Typically this is a very scarce resource on processor. Operating systems try to make best use of limited number of TLB resources. This optimization is more critical now as bigger and bigger physical memories (several GBs) are more readily available."
[^huge-sizes]: Linux kernel docs, HugeTLB Pages — <https://docs.kernel.org/admin-guide/mm/hugetlbpage.html> — "x86 CPUs normally support 4K and 2M (1G if architecturally supported) page sizes"
[^huge-use]: Linux kernel docs, HugeTLB Pages — <https://docs.kernel.org/admin-guide/mm/hugetlbpage.html> — "Users can use the huge page support in Linux kernel by either using the mmap system call or standard SYSV shared memory system calls (shmget, shmat)."
[^huge-reserved]: Linux kernel docs, HugeTLB Pages — <https://docs.kernel.org/admin-guide/mm/hugetlbpage.html> — "Pages that are used as huge pages are reserved inside the kernel and cannot be used for other purposes. Huge pages cannot be swapped out under memory pressure."
[^huge-rsvd]: Linux kernel docs, HugeTLB Pages — <https://docs.kernel.org/admin-guide/mm/hugetlbpage.html> — "Reserved huge pages guarantee that an application will be able to allocate a huge page from the pool of huge pages at fault time."
[^huge-boot]: Linux kernel docs, HugeTLB Pages — <https://docs.kernel.org/admin-guide/mm/hugetlbpage.html> — "This is the most reliable method of allocating huge pages as memory has not yet become fragmented."
[^thp-obj]: Linux kernel docs, Transparent Hugepage Support — <https://docs.kernel.org/admin-guide/mm/transhuge.html> — "that supports the automatic promotion and demotion of page sizes and without the shortcomings of hugetlbfs."
[^thp-scope]: Linux kernel docs, Transparent Hugepage Support — <https://docs.kernel.org/admin-guide/mm/transhuge.html> — "Currently THP only works for anonymous memory mappings and tmpfs/shmem."
[^thp-nomod]: Linux kernel docs, Transparent Hugepage Support — <https://docs.kernel.org/admin-guide/mm/transhuge.html> — "It requires no modifications for applications to take advantage of it."
[^thp-f1]: Linux kernel docs, Transparent Hugepage Support — <https://docs.kernel.org/admin-guide/mm/transhuge.html> — "taking a single page fault for each 2M virtual region touched by userland (so reducing the enter/exit kernel frequency by a 512 times factor)."
[^thp-f2a]: Linux kernel docs, Transparent Hugepage Support — <https://docs.kernel.org/admin-guide/mm/transhuge.html> — "the TLB miss will run faster (especially with virtualization using nested pagetables but almost always also on bare metal without virtualization)"
[^thp-f2b]: Linux kernel docs, Transparent Hugepage Support — <https://docs.kernel.org/admin-guide/mm/transhuge.html> — "a single TLB entry will be mapping a much larger amount of virtual memory in turn reducing the number of TLB misses."
[^thp-waste]: Linux kernel docs, Transparent Hugepage Support — <https://docs.kernel.org/admin-guide/mm/transhuge.html> — "An application may mmap a large region but only touch 1 byte of it, in that case a 2M page might be allocated instead of a 4k page for no good."
[^thp-clear]: Linux kernel docs, Transparent Hugepage Support — <https://docs.kernel.org/admin-guide/mm/transhuge.html> — "requiring larger clear-page copy-page in page faults which is a potentially negative effect."
[^thp-defrag-always]: Linux kernel docs, Transparent Hugepage Support (defrag) — <https://docs.kernel.org/admin-guide/mm/transhuge.html> — "means that an application requesting THP will stall on allocation failure and directly reclaim pages and compact memory in an effort to allocate a THP immediately."
[^thp-control]: Linux kernel docs, Transparent Hugepage Support — <https://docs.kernel.org/admin-guide/mm/transhuge.html> — "THP can be enabled system wide or restricted to certain tasks or even memory ranges inside task’s address space."
[^thp-modes]: Linux kernel docs, Transparent Hugepage Support (Global THP controls) — <https://docs.kernel.org/admin-guide/mm/transhuge.html> — "or only enabled inside MADV_HUGEPAGE regions (to avoid the risk of consuming more memory resources) or enabled system wide."
[^thp-madvise]: Linux kernel docs, Transparent Hugepage Support — <https://docs.kernel.org/admin-guide/mm/transhuge.html> — "should use madvise(MADV_HUGEPAGE) on their critical mmapped regions."
[^thp-khuge]: Linux kernel docs, Transparent Hugepage Support — <https://docs.kernel.org/admin-guide/mm/transhuge.html> — "there is khugepaged daemon that scans memory and collapses sequences of basic pages into huge pages of either PMD size or mTHP sizes, if the system is configured to do so."
[^thp-mthp]: Linux kernel docs, Transparent Hugepage Support — <https://docs.kernel.org/admin-guide/mm/transhuge.html> — "Page faults are significantly reduced (by a factor of e.g. 4, 8, 16, etc), but latency spikes are much less prominent"
[^alloc-global]: Rust std 1.95.0, module `std::alloc` — <https://doc.rust-lang.org/1.95.0/std/alloc/index.html> — "Currently the default global allocator is unspecified."
[^x86-5lvl-total]: Linux kernel docs, 5-level paging (Overview) — <https://docs.kernel.org/arch/x86/x86_64/5level-paging.html> — "It bumps the limits to 128 PiB of virtual address space and 4 PiB of physical address space."
[^x86-4lvl-total]: Linux kernel docs, 5-level paging (Overview) — <https://docs.kernel.org/arch/x86/x86_64/5level-paging.html> — "Original x86-64 was limited by 4-level paging to 256 TiB of virtual address"
[^x86-5lvl-56]: Linux kernel docs, 5-level paging (User-space and large virtual address space) — <https://docs.kernel.org/arch/x86/x86_64/5level-paging.html> — "On x86, 5-level paging enables 56-bit userspace virtual address space."
[^x86-5lvl-hint]: Linux kernel docs, 5-level paging (User-space and large virtual address space) — <https://docs.kernel.org/arch/x86/x86_64/5level-paging.html> — "But userspace can ask for allocation from full address space by specifying hint address (with or without MAP_FIXED) above 47-bits."
[^ostep-translate-start]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "Here is an address that refers to the 0th byte of VPN 254: 0x3F80, or 11 1111 1000 0000 in binary."
[^ostep-translate-pde]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "Thus, 1111 will choose the last (15th, if you start at the 0th) entry of the page directory above. This points us to a valid page of the page table located at address 101."
[^x86-pte8]: TU Graz ISEC, Paging on Intel x86-64 (teaching tutorial) — <https://www.isec.tugraz.at/teaching/materials/os/tutorials/paging-on-intel-x86-64/> — "every entry has 8 bytes and each table has a size of 4kB. This means each table has 512 entries which is why we need to have 9 bits to be able to address each entry"
[^ostep-more-levels]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "sometimes two levels of page table is not enough!"
[^ostep-one-extra]: OSTEP, ch.18 Paging: Introduction — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-paging.pdf> — "For every memory reference (whether an instruction fetch or an explicit load or store), paging requires us to perform one extra memory reference"
[^ostep-hw-know]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "To do this, the hardware has to know exactly where the page tables are located in memory (via a pagetable base register, used in Line 11 in Figure 19.1), as well as their exact format"
[^ostep-sw-priv]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "use special “privileged” instructions to update the TLB, and return from the trap; at this point, the hardware retries the instruction (resulting in a TLB hit)."
[^ostep-sw-simple]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "The hardware doesn’t do much on a miss: just raise an exception and let the OS TLB miss handler do the rest."
[^ostep-sw-arch]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "More modern architectures (e.g., MIPS R10k [H93] or Sun’s SPARC v9 [WG00], both RISC or reduced-instruction set computers) have what is known as a software-managed TLB."
[^ostep-asid-example]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "VPN 10 translates to either PFN 100 (P1) or PFN 170 (P2), but the hardware can’t distinguish which entry is meant for which process."
[^ostep-asid-reg]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "the OS must, on a context switch, set some privileged register to the ASID of the current process."
[^ostep-tlb-entry]: OSTEP, ch.19 Paging: Faster Translations (TLBs) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-tlbs.pdf> — "For example, the TLB commonly has a valid bit, which says whether the entry has a valid translation or not. Also common are protection bits, which determine how a page can be accessed (as in the page table)."
[^ostep-mostly-invalid]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "most of the page table is unused, full of invalid entries."
[^ostep-bigger-18]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "Take our 32-bit address space again, but this time assume 16KB pages. We would thus have an 18-bit VPN plus a 14-bit offset."
[^ostep-bigger-4x]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "of four reduction in size of the page table (not surprisingly, the reduction exactly mirrors the factor of four increase in page size)."
[^ostep-hybrid-regs]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "we use the base not to point to the segment itself but rather to hold the physical address of the page table of that segment. The bounds register is used to indicate the end of the page table"
[^ostep-hybrid-save]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "unallocated pages between the stack and the heap no longer take up space in a page table (just to mark them as not valid)."
[^ostep-hybrid-free]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "page tables now can be of arbitrary size (in multiples of PTEs). Thus, finding free space for them in memory is more complicated."
[^ostep-inverted-entry]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "The entry tells us which process is using this page, and which virtual page of that process maps to this physical page."
[^ostep-multi-pagesize]: OSTEP, ch.20 Paging: Smaller Tables (aside: multiple page sizes) — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "The main reason for multiple page sizes is not to save page table space, however; it is to reduce pressure on the TLB"
[^ostep-pde]: OSTEP, ch.20 Paging: Smaller Tables — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-smalltables.pdf> — "if the PDE is valid, it means that at least one of the pages of the page table that the entry points to (via the PFN) is valid"
