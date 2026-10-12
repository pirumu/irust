# P3 · Chương 05 — Bộ nhớ của tiến trình trên Linux

Chương này trả lời câu hỏi thực dụng: **một process Linux trông thế nào trong bộ nhớ, nó đến từ đâu, và các con số "memory usage" trên `top`, `ps`, `/proc` nghĩa là gì?** Bạn cần nó khi debug một service "ăn" RAM hoặc một container bị OOM-kill.

## Bạn sẽ hiểu được gì

1. **Q1.** File ELF chứa gì; `text`, `rodata`, `data`, `bss` khác nhau thế nào và cái nào không chiếm chỗ trong file? → mục 1
2. **Q2.** Kernel và `ld.so` biến file ELF thành process ra sao? → mục 2
3. **Q3.** Đọc `/proc/<pid>/maps` và `smaps` thế nào; ASLR đổi gì? → mục 3
4. **Q4.** `mmap` anonymous khác file-backed, `MAP_SHARED` khác `MAP_PRIVATE` thế nào; copy-on-write xảy ra khi nào; `brk` là gì? → mục 4
5. **Q5.** Overcommit là gì; vì sao `mmap` 1 TiB có thể thất bại nhưng thêm `MAP_NORESERVE` thì không? → mục 5
6. **Q6.** RSS, VSZ, PSS khác nhau thế nào và đọc chúng ra sao? → mục 6
7. **Q7.** `mlock` bảo đảm gì và giới hạn ở đâu? → mục 7
8. **Q8.** Memory cgroup đếm gì, vì sao container bị OOM-kill dù `RssAnon` thấp? → mục 8

## Kiến thức cần trước

- [Chương 01 — Address space](./01-address-space.md), [Chương 03 — Page table và TLB](./03-page-table-va-tlb.md), [Chương 04 — Page fault và swapping](./04-page-fault-va-swapping.md): demand paging, demand zeroing, minor/major fault.
- Thuộc chương khác: allocator ở [Chương 06](./06-heap-allocator.md); stack, guard page, size/align/padding của kiểu ở [Chương 07](./07-stack-alignment-padding.md) (layout kiểu Rust chuyên sâu: [P10 Chương 02](../10-rust-internals/02-type-layout.md)); `Box`, `Vec`, global allocator, `Rc` và leak ở [Chương 08](./08-rust-va-bo-nho.md).

> Môi trường các ví dụ chạy thật: container Debian bookworm của `labs/linux.sh`, kernel `6.8.0-117-generic`, **aarch64**, rustc 1.95.0, vài chương trình C. Trong container: `randomize_va_space` = 2, `vm.overcommit_memory` = 0, `ulimit -l` = 8192 kB, RAM ≈ 5,8 GiB, không swap. Địa chỉ và kích thước trên máy bạn sẽ khác; cái ổn định là *hình dạng* kết quả.

## 1. Từ file đến process: ELF (Q1)

> **Trả lời ngắn.** ELF là định dạng file thực thi của Linux: nó cho hệ thống biết byte nào trong file là lệnh, byte nào là dữ liệu, và cần dành thêm bao nhiêu chỗ cho dữ liệu chưa khởi tạo. `.text`, `.rodata`, `.data` có nội dung nằm trong file, còn `.bss` (dữ liệu toàn số 0) thuộc kiểu `SHT_NOBITS` nên không chiếm chỗ trong file[^elf-nobits]. Lúc nạp, header mô tả một vùng nhớ lớn hơn phần có trong file (`p_memsz` lớn hơn `p_filesz`), và phần dư được định nghĩa là số 0[^elf-load-zero].

**Giải thích.**

*Với service của bạn.* Hãy hình dung `orders`, một service viết bằng Rust, chạy trong container trên Kubernetes. Ta sẽ bám theo nó suốt chương (nhận xét của người soạn: đây là ví dụ giả định, không phải code trong repo). Mỗi mục trả lời một câu bạn sẽ gặp khi vận hành `orders`: binary của nó chứa gì (mục 1), nó thành process ra sao (mục 2), đọc `/proc/<pid>/maps` thế nào (mục 3), RSS tăng lúc nào (mục 4 và 5), con số nào của `ps` đáng tin (mục 6), và vì sao container bị OOM-kill (mục 8). Ta bắt đầu từ file. Giả sử `orders` có một buffer `static mut` rộng 1 MiB, toàn số 0, mà file binary không phình thêm 1 MiB. Vậy 1 MiB đó nằm ở đâu?

CPU chỉ chạy được lệnh nằm trong bộ nhớ, trong khi thứ `orders` có trên đĩa là một file. Vì vậy cần một **định dạng file** nói rõ: byte nào là lệnh, byte nào là dữ liệu đã khởi tạo, cần bao nhiêu chỗ cho dữ liệu chưa khởi tạo, nạp vào đâu và với quyền gì. Trên Linux định dạng đó là **ELF** (Executable and Linkable Format), mô tả trong `elf(5)`.

ELF có hai "bảng mục lục" cho hai đối tượng đọc khác nhau:

- **Section** (section header table) dành cho *linker* và các công cụ: `.text`, `.rodata`, `.data`, `.bss`…
- **Segment** (program header table) dành cho *loader*, tức bên nạp chương trình lên bộ nhớ. Bảng này là một mảng cấu trúc, mỗi phần tử mô tả một segment hoặc một thông tin khác mà hệ thống cần để chuẩn bị chạy chương trình[^elf-phdr]. Một segment chứa một hoặc nhiều section[^elf-seg-sec].

Lúc chạy, kernel không quan tâm section tên gì mà chỉ đọc các segment `PT_LOAD`. Mỗi phần tử `PT_LOAD` mô tả một segment nạp được bằng hai con số: `p_filesz` (phần có trong file) và `p_memsz` (kích thước trong bộ nhớ). Các byte của file được map vào đầu segment[^elf-load]. Nếu `p_memsz` lớn hơn `p_filesz` thì phần dư được định nghĩa là số 0 và nằm ngay sau vùng đã khởi tạo[^elf-load-zero]. Đó chính là cơ chế của `.bss`, và cũng là câu trả lời cho buffer 1 MiB của `orders`: file không chứa byte nào của nó, chỉ ghi một `p_memsz` lớn hơn `p_filesz`; kernel tự điền số 0 khi nạp (thí nghiệm ở phần Đi sâu cho thấy đúng như vậy).

Mỗi segment còn có cờ quyền: segment text thường có `PF_X` và `PF_R`, segment data thường có `PF_W` và `PF_R`[^elf-flags]. Các quyền này sẽ hiện ra thành `r-xp` và `rw-p` trong `/proc/<pid>/maps` (mục 3).

| Section | Nội dung theo `elf(5)` | Trong file? | Quyền thường gặp |
|---|---|---|---|
| `.text` | các lệnh thực thi của chương trình[^elf-text] | có | đọc + thực thi |
| `.rodata` | dữ liệu chỉ đọc, thường thuộc một segment không ghi được[^elf-rodata] | có | chỉ đọc |
| `.data` | dữ liệu đã khởi tạo, đóng góp vào memory image[^elf-data] | có | đọc + ghi |
| `.bss` | dữ liệu chưa khởi tạo; hệ thống khởi tạo toàn số 0 khi chương trình bắt đầu chạy[^elf-bss] | **không** | đọc + ghi |

`.bss` có kiểu `SHT_NOBITS`[^elf-bss], và section kiểu này có thể có kích thước khác 0 nhưng không chiếm chỗ trong file[^elf-nobits]. Ba section còn lại đều có kiểu `SHT_PROGBITS`. `.text` là vậy[^elf-progbits-text]. `.rodata` cũng vậy[^elf-progbits-ro]. `.data` cũng vậy[^elf-progbits-data]. Từ đó ta suy ra chúng có byte thật trong file (suy luận của người soạn). Một mảng 1 MiB toàn số 0 vì vậy không tốn 1 MiB trên đĩa: file chỉ ghi kích thước (`p_memsz` lớn hơn `p_filesz`), còn phần dư được định nghĩa là số 0[^elf-load-zero]. `proc(5)` cũng nêu BSS làm ví dụ cho một mapping không có inode[^proc-inode].

<details><summary>Đi sâu: ví dụ chạy thật với <code>static mut</code>, static không <code>mut</code>, và <code>PT_GNU_STACK</code></summary>

```rust
static mut ZERO: [u8; 1 << 20] = [0; 1 << 20];          // 1 MiB số 0
// main: in địa chỉ ZERO, các dòng /proc/self/maps từ file thực thi tới [heap],
// rồi VmRSS/RssAnon/RssFile; ghi 1 byte vào mỗi page 4 KiB của ZERO; in lại.
```

Kết quả (đã chạy, rút gọn):

```text
c9e9e7520000-c9e9e756e000 r-xp 00000000 00:32 1337895   /tmp/sec3
c9e9e757d000-c9e9e7580000 r--p 0005d000 00:32 1337895   /tmp/sec3
c9e9e7580000-c9e9e7581000 rw-p 00060000 00:32 1337895   /tmp/sec3
c9e9e7581000-c9e9e7681000 rw-p 00000000 00:00 0
c9ea03628000-c9ea03649000 rw-p 00000000 00:00 0         [heap]
before touch: VmRSS: 1680 kB | RssAnon: 120 kB  | RssFile: 1560 kB
after touch : VmRSS: 2704 kB | RssAnon: 1144 kB | RssFile: 1560 kB
```

- Dòng `rw-p` có `offset 0x60000` và inode khác 0 là `.data` lấy từ file. Dòng ngay sau, dài đúng `0x100000` = 1 MiB, **không tên, inode 0**, khớp với phần dư `p_memsz > p_filesz` của `.bss`. Hệ thống dựng nó bằng bộ nhớ anonymous (suy luận của người soạn từ inode 0 và kích thước khớp).
- Trước khi chạm, `RssAnon` chỉ 120 kB dù `ZERO` rộng 1 MiB; sau khi chạm mỗi page, `RssAnon` tăng đúng 1024 kB. Đó là demand zeroing[^ostep-demand-zero] quan sát bằng số liệu.
- `readelf -lW` cho segment `LOAD RW` có `FileSiz 0x3c68`, `MemSiz 0x103d48`: chênh ≈ 1 MiB là `.bss`; file thực thi (≈ 4,3 MB) không phình ra.

**Quan sát đáng nhớ (không phải bảo đảm).** Với `static ZERO: [u8; 1<<20] = [0; 1<<20];` (không `mut`), `readelf -SW` cho thấy `.rodata` phình lên `0x107390` byte còn `.bss` gần rỗng: rustc đặt static bất biến vào `.rodata` kể cả khi toàn số 0. Điều này phù hợp với Rust Reference: static không `mut`, kiểu không interior-mutable, *có thể* nằm ở bộ nhớ chỉ đọc[^ref-static-ro]. Muốn thấy `.data` hoặc `.bss` thì phải dùng `static mut` (cần `unsafe` để đọc hay ghi[^ref-static-mut]) hoặc một kiểu interior-mutable như atomic.

`PT_GNU_STACK` là extension của GNU, kernel Linux dùng nó để điều khiển trạng thái stack qua `p_flags`[^elf-gnustack]; trong ví dụ cờ là `RW` (không có `E`). Guard page và tràn stack nằm ở [Chương 07](./07-stack-alignment-padding.md); chương đó không bàn riêng `PT_GNU_STACK`.

</details>

**Quay lại Q1:** với `orders`, ELF mô tả file bằng section (cho linker) và segment (cho loader); buffer 1 MiB toàn số 0 chỉ là một con số trong header, vì phần dư `p_memsz > p_filesz` được định nghĩa là số 0 lúc chạy.

## 2. Loader và dynamic linker (`ld.so`) (Q2)

> **Trả lời ngắn.** `execve` thay chương trình đang chạy trong process bằng một chương trình mới, với stack, heap và data segment mới[^execve-replace]. Nếu chương trình dùng thư viện chia sẻ (liên kết động), file ELF ghi đường dẫn của một "người nạp thư viện" (interpreter, segment `PT_INTERP`), và `execve` nhờ nó nạp các thư viện cần thiết[^execve-interp]. Người nạp đó là `ld.so`: nó tìm và nạp thư viện rồi mới chạy chương trình[^ldso-desc].

**Giải thích.**

*Với service `orders`.* Khi bạn chạy `./orders`, trước khi dòng đầu tiên của `main` chạy đã có một chuỗi việc diễn ra, và chuỗi đó giải thích một chi tiết lạ trong `maps` (mục 3): `libc.so.6` nằm trong address space của `orders` dù không hề có trong file binary. Mục này đi qua chuỗi việc ấy, từ lúc shell gọi `execve` tới lúc `main` chạy. Có hai bên làm hai việc nối nhau.

1. **Kernel dọn chỗ.** `execve` thay chương trình đang chạy trong process bằng chương trình mới, với stack, heap và data segment mới[^execve-replace]. Không có process mới nào ra đời, PID giữ nguyên[^execve-pid], còn mọi memory mapping cũ đều bị bỏ[^execve-mappings]. Vì vậy phần page dùng chung kiểu COW mà con thừa hưởng từ cha qua `fork` (mục 4) không sống sót qua `exec` (suy luận của người soạn).
2. **Kernel đọc bảng segment.** Các segment `PT_LOAD` của file (mục 1) cho kernel biết vùng nhớ nào nạp từ byte nào của file[^elf-load]. Nếu ELF còn có segment `PT_INTERP`, segment đó chứa đường dẫn của một chương trình "thông dịch" cần gọi[^elf-interp]. Với binary liên kết động, `execve` dùng interpreter này để nạp các shared object cần thiết[^execve-interp]. Trong ví dụ ở mục 1, `readelf` báo `/lib/ld-linux-aarch64.so.1`.
3. **`ld.so` nạp thư viện.** Interpreter đó chính là dynamic linker `ld.so`: nó tìm và nạp các shared object chương trình cần, chuẩn bị chương trình rồi chạy nó[^ldso-desc]. Danh sách thư viện cần nạp nằm trong ELF dưới dạng các mục `DT_NEEDED`, mỗi mục là một offset trong string table, trỏ tới tên một thư viện[^elf-needed].

Chương này không đọc source kernel nên không khẳng định thứ tự chi tiết bên trong, chẳng hạn mapping nào được dựng trước hay stack được đặt lúc nào. Điều quan sát được nằm trong `maps` (mục 3): `libc.so.6` có mặt vì `ld.so` nạp nó lúc chạy, còn `ld-linux-aarch64.so.1` có mặt vì chính nó là interpreter của `PT_INTERP`.

<details><summary>Đi sâu: thứ tự tìm thư viện, secure-execution, resolve lười, static và dynamic</summary>

- **Tìm thư viện ở đâu.** Với dependency không chứa `/`, thứ tự là: `DT_RPATH` (nếu không có `DT_RUNPATH`), `LD_LIBRARY_PATH` (bỏ qua trong secure-execution mode), `DT_RUNPATH`, cache `/etc/ld.so.cache`, rồi `/lib`, `/usr/lib`[^ldso-search]. Cache là một "danh sách đã biên dịch các shared object ứng viên"[^ldso-cache], nên thêm thư viện mà không cập nhật cache (`ldconfig`) có thể khiến nó không được thấy (suy luận của người soạn).
- Trong **secure-execution mode** (ví dụ binary set-user-ID), một số biến môi trường bị vô hiệu hoặc sửa, rồi bị gỡ khỏi môi trường[^ldso-secure]. Vì vậy đừng dựa vào `LD_LIBRARY_PATH` hay `LD_PRELOAD` cho chương trình đặc quyền.
- **Resolve lười.** Đặt `LD_BIND_NOW` thì mọi symbol được phân giải ngay lúc khởi động thay vì hoãn đến lần tham chiếu đầu tiên[^ldso-bind].
- **Static và dynamic.** Binary Linux cần liên kết động trừ khi truyền `-static` cho `ld(1)` lúc biên dịch[^ldso-static]. Liên kết động cho nhiều process dùng chung một bản `libc.so.6` trong RAM; liên kết tĩnh cho file tự đủ, tiện cho container tối giản (nhận xét của người soạn).

</details>

**Quay lại Q2:** `execve` dựng address space mới từ các segment `PT_LOAD`; nếu có `PT_INTERP` thì `ld.so` nạp thư viện chia sẻ rồi mới chạy `main` của `orders`.

## 3. Bố cục address space, `maps`, `smaps` và ASLR (Q3)

> **Trả lời ngắn.** `/proc/<pid>/maps` liệt kê các vùng nhớ đang được map cùng quyền truy cập của chúng[^proc-maps], còn `smaps` cho biết mỗi vùng tiêu thụ bao nhiêu[^proc-smaps]. ASLR (ngẫu nhiên hóa bố cục address space) làm vị trí một số vùng đổi giữa các lần chạy, và kiểu ngẫu nhiên hóa chọn bằng `randomize_va_space`[^kdoc-aslr]. Vì vậy địa chỉ trong log chỉ có nghĩa khi kèm địa chỉ nền (base) của file hoặc thư viện lấy từ `maps` (nhận xét của người soạn).

**Giải thích.**

*Với service `orders`.* Giả sử `orders` vừa crash và on-call dán vào chat một địa chỉ lấy từ log, hỏi "địa chỉ này thuộc hàm nào?". Muốn trả lời, bạn phải biết địa chỉ đó rơi vào vùng nhớ nào của process, và vùng đó bắt đầu ở đâu trong lần chạy này. `maps` và `smaps` cho bạn thông tin ấy; ASLR giải thích vì sao lần chạy sau lại khác.

### Đọc một dòng của `maps`

Mỗi dòng của `maps` có dạng:

```text
address            perms offset   dev   inode    pathname
c9e9e7520000-c9e9e756e000 r-xp 00000000 00:32 1337895   /tmp/sec3
```

- `perms` gồm `r`, `w`, `x`, và ký tự thứ tư là `s` (shared) hoặc `p` (private, copy on write)[^proc-perms].
- `offset`, `dev`, `inode` cho biết vị trí trong file hoặc thiết bị. Inode 0 nghĩa là không có inode nào gắn với vùng đó[^proc-inode].
- `pathname` thường là file đang backing mapping. Với file ELF, bạn có thể đối chiếu `offset` với trường Offset trong program header (`readelf -l`)[^proc-pathname]. Đó chính là điều ta đã làm ở mục 1: dòng `rw-p ... 00060000` khớp với segment data.
- Một số pathname là tên giả. `[stack]` là stack của initial process, còn gọi là main thread[^proc-stack]. `[heap]` là heap của process[^proc-heap]. `[vdso]` là virtual dynamically linked shared object[^proc-vdso]. Pathname trống nghĩa là anonymous mapping.

Cùng một file có thể xuất hiện ở nhiều dòng với quyền khác nhau. File thực thi ở mục 1 có `r-xp`, `r--p` và `rw-p`; trong lần chạy quan sát, `libc.so.6` còn có thêm dòng `---p` (không truy cập được, không liệt kê ở đây). Lý do (suy luận của người soạn): quyền được thực thi theo từng page vì mỗi PTE có bit riêng, như [Chương 03](./03-page-table-va-tlb.md) đã nêu.

### Một process thật

Quan sát một lần chạy trên aarch64, theo thứ tự địa chỉ tăng dần: file thực thi (`r-xp` text, `r--p`, `rw-p` data), rồi anonymous `rw-p` là `.bss`, rồi `[heap]`, rồi `libc.so.6`, `libgcc_s.so.1`, `ld-linux`, rồi `[vvar]`, `[vdso]`, và `[stack]` ở địa chỉ rất cao. Cách xếp stack và heap như vậy chỉ là quy ước, có thể xếp khác[^ostep-convention], nên đừng viết code giả định `[heap]` nằm trước hay sau thư viện.

Có hai điều dễ hiểu sai khi nhìn vào danh sách này:

1. **`[heap]` không phải toàn bộ heap của bạn.** Nó thường là vùng do `brk` quản lý (mục 4; suy luận của người soạn từ chuyện `[heap]` là "heap của process" và định nghĩa program break). Cấp phát lớn thường đi qua `mmap` anonymous không tên ([Chương 06](./06-heap-allocator.md)), nên RSS anonymous nằm ở cả hai nơi.
2. **Stack của thread không còn tên riêng.** Dòng `[stack:tid]` chỉ có từ Linux 3.4 đến 4.4, rồi bị gỡ từ 4.5 vì quá tốn kém với process có nhiều thread[^proc-stacktid].

### `smaps` và PSS

`/proc/<pid>/smaps` cho biết mức tiêu thụ của *từng* mapping[^proc-smaps]: `Size`, `Rss` (phần đang nằm trong RAM), `Pss` (phần chia theo tỉ lệ cho process này), số page shared và private, clean và dirty[^proc-smaps-fields]. Kernel docs định nghĩa PSS là số page process có trong bộ nhớ, trong đó mỗi page được chia cho số process dùng chung nó[^kdoc-pss]. Một page do N process cùng map thì mỗi process chịu 1/N. Từ định nghĩa này ta thấy tổng PSS không đếm trùng page dùng chung, còn tổng RSS thì có (suy luận của người soạn).

### ASLR

Nếu bố cục cố định thì kẻ tấn công biết trước địa chỉ các hàm của `libc`, của stack… (nhận xét của người soạn). **ASLR** (address space layout randomization) ngẫu nhiên hóa vị trí các vùng, nên lần chạy sau thường khác lần trước (xem hai lần chạy ở phần Đi sâu). `randomize_va_space` chọn kiểu ngẫu nhiên hóa, trên các kiến trúc hỗ trợ[^kdoc-aslr]:

| Giá trị | Ý nghĩa (theo kernel docs) |
|---|---|
| 0 | tắt ngẫu nhiên hóa; mặc định với kiến trúc không hỗ trợ và kernel boot với `norandmaps`[^kdoc-aslr0] |
| 1 | ngẫu nhiên hóa địa chỉ mmap base, stack và trang VDSO, nên thư viện chia sẻ được nạp ở địa chỉ ngẫu nhiên; với binary liên kết PIE, địa chỉ bắt đầu code cũng ngẫu nhiên[^kdoc-aslr1] |
| 2 | thêm ngẫu nhiên hóa heap[^kdoc-aslr2] |

Quay lại địa chỉ trong log của `orders` (nhận xét của người soạn): nó chỉ có nghĩa khi bạn biết base của module chứa nó, và base lấy từ `maps` của đúng lần chạy đó. Trong core dump cũng vậy. ASLR làm khai thác khó hơn chứ không thay được memory safety. [Chương 01](./01-address-space.md) mục 2 đã in địa chỉ của hai lần chạy; ở đây là cơ chế đứng sau.

<details><summary>Đi sâu: VmFlags, vì sao heap tách riêng, quan sát hai lần chạy</summary>

- Mỗi mapping trong `smaps` còn có dòng `VmFlags` với các cờ như `rd`, `wr`, `ex`, `sh`, `lo` (pages are locked in memory), `ac` (area is accountable), `nr` (swap space is not reserved)[^proc-vmflags].
- Heap được ngẫu nhiên hóa riêng vì có chương trình rất cũ giả định vùng `brk` bắt đầu ngay sau code và bss (kernel docs nêu libc.so.5 từ 1996), nhưng không biết ứng dụng không-legacy nào bị hỏng vì điều này[^kdoc-compat].
- Chạy hai lần cùng một binary (`randomize_va_space = 2`):

```text
lần 1: main = 0xc3affe0e86e8   heap Box = 0xc3b0260dcd60   stack = 0xffffebb959dc
lần 2: main = 0xbb813b1086e8   heap Box = 0xbb816aa31d60   stack = 0xffffc2e18ddc
```

Phần thấp (`6e8`, `d60`) giữ nguyên, phần cao đổi: base của text, heap và stack đều dời. Binary là PIE (`readelf`: `DYN (Position-Independent Executable file)`).

</details>

**Quay lại Q3:** `maps` cho biết vùng nào có quyền gì và đến từ file nào, `smaps` cho biết mỗi vùng tốn bao nhiêu, còn ASLR dời vị trí các vùng (mmap base, stack, vdso, text của PIE, heap ở mức 2) nên địa chỉ trong log của `orders` phải đi kèm base.

## 4. `mmap`, copy-on-write và `brk` (Q4)

> **Trả lời ngắn.** `mmap` tạo một vùng nhớ mới trong address space của process[^mmap-new]. Vùng *anonymous* (không gắn file) được khởi tạo bằng 0[^mmap-anon]. Vùng *file-backed* (gắn file) được khởi tạo từ nội dung file[^mmap-file]. Với `MAP_SHARED`, ghi vào vùng thì process khác map cùng vùng thấy được[^mmap-shared]. Với `MAP_PRIVATE`, ghi chỉ mình thấy và không xuống file[^mmap-private], nhờ copy-on-write (COW): chỉ khi một bên ghi thì page mới được chép thật. Còn `brk` dời *program break*, mép cuối vùng dữ liệu của process, để cấp hoặc trả bộ nhớ[^brk-def].

**Giải thích.**

*Với service `orders`.* Giả sử `orders` nạp bảng giá 16 MiB từ file bằng `mmap` thay vì `read`, và còn tạo snapshot bằng `fork`. Có hai chuyện bạn cần nắm: RSS của `orders` tăng lúc nào, và khi `orders` (hoặc process con của nó) ghi vào một vùng nhớ thì file và bộ nhớ của bên kia bị ảnh hưởng ra sao.

Mọi vùng bạn thấy trong `maps` đều do một trong số ít syscall tạo ra, và hai syscall quan trọng nhất là `mmap` và `brk`.

### `mmap`: anonymous và file-backed

`mmap` tạo mapping mới trong virtual address space của process gọi nó[^mmap-new]. Nếu `addr` là NULL thì kernel tự chọn địa chỉ, căn theo page[^mmap-null]. Có hai loại mapping:

| | **Anonymous** (`MAP_ANONYMOUS`) | **File-backed** |
|---|---|---|
| Nội dung ban đầu | không gắn file nào, khởi tạo bằng 0[^mmap-anon] | khởi tạo từ `length` byte của file bắt đầu tại `offset`[^mmap-file] |
| Khi hết RAM | cần swap (không có file để nạp lại) | page chưa bị sửa bỏ đi được, nạp lại từ file |
| `RssAnon` / `RssFile` | `RssAnon` | `RssFile` |

Hàng "khi hết RAM" có lý do này: frame của page code dùng lại được vì nạp lại được từ file thực thi trên đĩa ([Chương 04](./04-page-fault-va-swapping.md) mục 3), còn anonymous không có bản gốc trên đĩa nên cần swap (suy luận của người soạn từ ý đó).

### `MAP_SHARED` và `MAP_PRIVATE`

- `MAP_SHARED`: cập nhật hiển thị với process khác map cùng vùng, và với file-backed thì còn được ghi xuống file[^mmap-shared].
- `MAP_PRIVATE`: mapping copy-on-write riêng; cập nhật không hiển thị với process khác và không ghi xuống file[^mmap-private].

Với bảng giá của `orders`, nếu map `MAP_PRIVATE` thì dù `orders` sửa vào vùng đó, file trên đĩa vẫn nguyên. Nếu map `MAP_SHARED` thì sửa là ghi xuống file.

Một hiểu lầm phổ biến là "RSS tăng nghĩa là process vừa `mmap` thêm". Thí nghiệm ở phần Đi sâu cho thấy `mmap` chỉ làm `VmSize` tăng; RSS tăng khi *chạm* page (anonymous) hoặc *đọc* page (file-backed).

### Copy-on-write và `fork`

Khi cần sao chép page giữa hai address space, OS map page đó vào đích rồi đánh dấu chỉ đọc ở cả hai bên; chỉ khi một bên ghi thì mới chép thật[^ostep-cow]. Điều này đặc biệt hữu ích với `fork()`, vì `fork()` tạo bản sao chính xác của address space của caller, và chép thật thì vừa chậm vừa tốn dữ liệu[^ostep-cow-fork]. Linux cài `fork` bằng COW, nên cái giá chỉ là thời gian và bộ nhớ để nhân đôi page table của cha và tạo task structure cho con[^fork-cow].

### `brk` và program break

`brk` và `sbrk` đổi vị trí **program break**, tức cuối data segment (vị trí đầu tiên sau uninitialized data segment). Tăng break là cấp bộ nhớ cho process, giảm break là trả lại[^brk-def]. Man page khuyên tránh gọi trực tiếp, vì `malloc(3)` mới là cách di động để cấp phát[^brk-avoid]. Hàm này còn bị đánh dấu LEGACY trong SUSv2 và bị gỡ khỏi POSIX.1-2001[^brk-std]. Người gọi `brk` thực tế là allocator ([Chương 06](./06-heap-allocator.md)). Ở `randomize_va_space = 2`, điểm bắt đầu của `[heap]` cũng bị dời (mục 3): trong lần chạy ở mục 1, `[heap]` cách `.bss` một khoảng lớn.

<details><summary>Đi sâu: thí nghiệm <code>mmap</code>/COW, thí nghiệm <code>fork</code> (RSS vs PSS), <code>SIGBUS</code>, đóng fd</summary>

- Sau khi `mmap` trả về, bạn có thể đóng ngay file descriptor mà mapping vẫn hợp lệ[^mmap-fdclose]. Ngược lại, đóng fd không làm vùng bị unmap; mapping tự biến mất khi process kết thúc[^munmap].
- Truy cập một page nằm hoàn toàn sau cuối file đã map gây `SIGBUS`[^mmap-sigbus]. Man page không nói rõ trường hợp file bị cắt ngắn, nhưng chạy thử trên lab (C: `mmap` file 8 KiB `MAP_SHARED`, `ftruncate` xuống 0, ghi byte 5000) thì process chết với `Bus error`, exit 135 = 128 + SIGBUS (một lần chạy). Nhận xét của người soạn: `mmap` file không an toàn như `read` nếu process khác có thể cắt ngắn file.

**Thí nghiệm `mmap`** (C, đã chạy; file 16 MiB tạo bằng `ftruncate`; trước đó `VmSize=2196kB VmRSS=1216kB`):

```text
mmap anon 256MiB          VmSize=264340kB  VmRSS=1216kB     RssAnon=88kB      RssFile=1128kB
touched 128MiB            VmSize=264340kB  VmRSS=132288kB   RssAnon=131160kB  RssFile=1128kB
mmap file private 16MiB   VmSize=280724kB  VmRSS=132288kB   RssAnon=131160kB  RssFile=1128kB
read all 16MiB            VmSize=280724kB  VmRSS=148672kB   RssAnon=131160kB  RssFile=17512kB
wrote 1 byte (COW)        VmSize=280724kB  VmRSS=148672kB   RssAnon=131164kB  RssFile=17508kB
file byte 0 after private write: 0
file byte 0 after shared write: Y
```

1. `mmap` 256 MiB anonymous: `VmSize` +262 144 kB nhưng `VmRSS` **không đổi**. Chạm 128 MiB thì `RssAnon` +128 MiB.
2. `mmap` file private 16 MiB: RSS chưa đổi; đọc hết file thì `RssFile` +16 384 kB.
3. Ghi **một byte** vào mapping private: `RssAnon` +4 kB, `RssFile` −4 kB. Page file-backed được thay bằng một bản sao anonymous riêng (COW), còn `pread` byte 0 của file vẫn là 0.
4. Ghi qua `MAP_SHARED` thì đọc lại file thấy `Y`.

**Thí nghiệm `fork`** (C, đã chạy): cha chạm ~192 MiB anonymous, `fork`; con đọc `/proc/self/smaps_rollup`, ghi 32 MiB, đọc lại.

```text
parent before fork        Rss=214212kB Pss=213170kB Shared_Dirty=0kB      Private_Dirty=196712kB
child after fork          Rss=213792kB Pss=106612kB Shared_Dirty=196664kB Private_Dirty=44kB
child wrote 32MiB         Rss=213920kB Pss=123004kB Shared_Dirty=163896kB Private_Dirty=32812kB
parent after child exit   Rss=214276kB Pss=213174kB Shared_Dirty=0kB      Private_Dirty=196712kB
```

Đây là bài học **RSS vs PSS**: ngay sau `fork`, `Rss` của con ≈ cha nhưng `Pss` chỉ còn một nửa, vì gần như mọi page đều dùng chung và mỗi bên chịu 1/2. Khi con ghi 32 MiB, `Private_Dirty` ≈ 32 812 kB (đã COW), `Shared_Dirty` giảm 32 768 kB, và `Pss` tăng ≈ 16 MiB (nửa của 32 MiB chuyển từ chia sẻ sang riêng). Khi con thoát, PSS của cha trở lại như cũ. (Một lần chạy; phần giải thích là suy luận từ định nghĩa PSS.)

Liên hệ backend (nhận xét của người soạn): snapshot kiểu "fork rồi con ghi ra đĩa" dựa vào COW. Nếu cha ghi nhanh trong lúc con chạy, page bị chép dần và bộ nhớ thật có thể tiến gần gấp đôi, nên hãy theo dõi `Private_Dirty` hoặc PSS chứ đừng chỉ nhìn RSS.

</details>

**Quay lại Q4:** `mmap` chỉ tạo mapping và RAM chỉ được cấp khi chạm; với bảng giá của `orders`, `MAP_PRIVATE` ghi thì COW còn `MAP_SHARED` ghi xuống file; `fork` cũng dựa vào COW.

## 5. Overcommit (Q5)

> **Trả lời ngắn.** `mmap` chỉ giữ chỗ trong address space chứ không đặt trước RAM thật, nên kernel phải chọn giữa từ chối sớm và chấp nhận rồi chịu rủi ro hết RAM về sau. Ở chế độ 0 (mặc định), kernel chỉ từ chối các yêu cầu *hiển nhiên* quá lớn[^kdoc-oc0]. Lời gọi `mmap` có cờ `MAP_NORESERVE` thì không bị kiểm tra[^proc-oc0]. Trong thí nghiệm ở phần Đi sâu, `mmap` 1 TiB trên máy 5,8 GiB RAM thất bại, nhưng thêm `MAP_NORESERVE` thì thành công.

**Giải thích.**

*Với service `orders`.* Giả sử một thư viện trong `orders` xin trước một vùng address space rất lớn để dùng dần, và `ps` hiện VSZ lên tới hàng chục GB, trong khi máy chỉ có vài GB RAM. Câu hỏi là kernel có chịu cấp vùng đó không, và dựa vào đâu để quyết định.

`mmap` 256 MiB ở mục 4 thành công mà RSS không đổi: *không có RAM nào được đặt trước*. Vì vậy kernel phải chọn một trong hai. Nó có thể từ chối sớm ngay lúc `mmap` (trả `ENOMEM` rõ ràng), hoặc chấp nhận rồi chịu rủi ro hết RAM về sau, kết thúc bằng OOM kill. Linux cho bạn chọn bằng sysctl `vm.overcommit_memory`, với các chế độ sau theo kernel docs[^kdoc-oc-modes]:

| Mode | Hành vi |
|---|---|
| 0 | heuristic: từ chối các overcommit *hiển nhiên* về address space; dùng cho hệ thống điển hình; bảo đảm cấp phát "hoang dại" thất bại nhưng vẫn cho overcommit để giảm swap; **mặc định**[^kdoc-oc0] |
| 1 | luôn overcommit; hợp với một số ứng dụng khoa học dùng mảng thưa, dựa vào việc bộ nhớ ảo gần như toàn page số 0[^kdoc-oc1] |
| 2 | không overcommit: tổng commit toàn hệ thống không được vượt swap + một tỉ lệ cấu hình được (mặc định 50%) RAM vật lý; thường process sẽ không bị giết lúc truy cập page mà nhận lỗi ngay lúc cấp phát[^kdoc-oc2] |

`proc(5)` bổ sung hai điều cho mode 0: `mmap` có `MAP_NORESERVE` không bị kiểm tra, và phép kiểm tra mặc định "rất yếu", nên vẫn có rủi ro bị OOM-kill[^proc-oc0]. Hai lời gọi trong thí nghiệm chỉ khác nhau đúng cờ `MAP_NORESERVE`, tức là không dành chỗ swap cho mapping[^mmap-noreserve]. Cái giá là khi swap không được dành, một lần ghi vào page có thể nhận `SIGSEGV` nếu không còn bộ nhớ vật lý[^mmap-noreserve2].

Hệ quả cho `orders` (nhận xét của người soạn): một process có `VmSize` hàng chục GB vì reserve address space chưa chắc đã có vấn đề. Chỉ số đáng theo dõi là RSS, PSS và memory cgroup (mục 8).

<details><summary>Đi sâu: cái gì tính vào commit, <code>CommitLimit</code>, thí nghiệm 1 TiB</summary>

- `CommitLimit` và `Committed_AS` xem được trong `/proc/meminfo`[^kdoc-oc-view]; giới hạn chỉ có hiệu lực khi bật strict accounting (mode 2)[^proc-commitlimit]. Ở mode 2, cờ `MAP_NORESERVE` bị bỏ qua[^kdoc-oc-gotcha].
- Theo kernel docs, mapping file-backed SHARED hoặc READ-only tốn 0, còn PRIVATE WRITABLE tốn bằng kích thước mapping cho mỗi instance[^kdoc-oc-rules]. Anonymous SHARED hoặc PRIVATE WRITABLE thì tốn bằng kích thước mapping[^kdoc-oc-rules2]. `Committed_AS` trong `/proc/meminfo` tính cả phần chưa "dùng": xin 1 GB nhưng chỉ chạm 300 MB thì vẫn được cấp address space cho cả 1 GB[^proc-committed].
- **Thí nghiệm.** Máy lab: mode 0, RAM ≈ 5,8 GiB, không swap. Hai lời gọi (C, đã chạy):

```text
mmap 1TiB:             Cannot allocate memory        (ENOMEM)
mmap 1TiB NORESERVE:   ok
```

Sau lệnh thứ hai, `VmSize` là 1 074 104 468 kB (≈ 1 TiB) trong khi `VmRSS` vẫn ≈ 214 MB.

</details>

**Quay lại Q5:** mode 0 chỉ chặn yêu cầu hiển nhiên quá lớn và bỏ qua kiểm tra khi có `MAP_NORESERVE`; đổi lại, nếu hết RAM thật thì rủi ro là OOM-kill hoặc `SIGSEGV`.

## 6. Đọc memory usage: RSS, VSZ và họ hàng (Q6)

> **Trả lời ngắn.** `VSZ` (cột của `ps`) là kích thước virtual memory[^ps-vsz], tức phần address space đã map chứ chưa chắc có RAM. Trong `/proc/<pid>/status` trường tương ứng là `VmSize`[^proc-vmsize]. `RSS` là phần đang nằm trong RAM: `ps` gọi nó là `rss`[^ps-rss], còn `/proc` gọi là `VmRSS`, bằng `RssAnon + RssFile + RssShmem`[^proc-vmrss]. `PSS` chia mỗi page cho số process dùng chung nó[^kdoc-pss], nên cộng PSS giữa các process không đếm trùng (suy luận của người soạn từ định nghĩa). VSZ lớn mà RSS nhỏ thường vô hại (nhận xét của người soạn).

**Giải thích.**

*Với service `orders`.* Dashboard của bạn có thể hiện `orders` với VSZ vài chục GB nhưng RSS vài trăm MB, và `ps` của năm replica cộng lại vượt RAM của node. Hai câu hỏi tự nhiên là con số nào mới phản ánh mức dùng thật, và cộng chúng giữa các process có hợp lệ không. Các trường dưới đây trả lời cả hai.

| Trường | Ý nghĩa theo `proc(5)` |
|---|---|
| `VmSize` | virtual memory size[^proc-vmsize] (cùng giá trị với `size` trong `statm`[^proc-statm]) |
| `VmLck` | locked memory size (xem `mlock(2)`) |
| `VmRSS` | resident set size; là **tổng** của `RssAnon`, `RssFile`, `RssShmem`[^proc-vmrss] |
| `RssAnon` | resident anonymous memory (từ Linux 4.5)[^proc-rssanon] |
| `RssFile` | resident file mappings[^proc-rssfile] |
| `RssShmem` | resident shared memory, gồm System V shared memory, mapping từ tmpfs và shared anonymous mapping[^proc-rssshmem] |
| `VmSwap` | bộ nhớ anonymous private đã bị swap ra; không tính shmem swap[^proc-vmswap] |
| `VmHWM` | đỉnh của RSS[^proc-vmhwm] |

Cách đọc các con số này khi nhìn vào `orders`:

- **VSZ lớn, RSS nhỏ**: process reserve nhiều address space nhưng chưa chạm tới (mục 4 và 5). Thường vô hại.
- **`RssFile` lớn**: phần lớn là code, thư viện hoặc file được `mmap`. Page sạch bỏ được rồi nạp lại từ file, và lần chạm sau là một major fault ([Chương 04](./04-page-fault-va-swapping.md) mục 6).
- **`RssAnon` lớn**: heap, stack, `.bss` đã chạm và các buffer. Đây là phần "thật sự của ứng dụng", chỉ giảm khi chương trình trả lại hoặc bị swap.
- **Tổng RSS vượt RAM** giữa nhiều process: bình thường nếu có chia sẻ (`libc.so.6`, COW sau `fork`). Muốn cộng cho đúng thì dùng PSS (mục 4).
- **"Tôi `free` mà RSS không giảm"**: `free` không phải lúc nào cũng trả bộ nhớ cho OS (chi tiết và nguồn ở [Chương 06](./06-heap-allocator.md)).

<details><summary>Đi sâu: độ chính xác của các con số và quy trình chẩn đoán</summary>

- `proc(5)` cảnh báo nhiều giá trị trên **không chính xác** vì một tối ưu hóa scalability trong kernel; cần chính xác thì dùng `smaps` hoặc `smaps_rollup`, chậm hơn nhiều[^proc-statm-inacc]. Dùng `VmRSS` để xem xu hướng, `smaps_rollup` để đối chiếu.
- Quy trình chẩn đoán gợi ý (nhận xét của người soạn): `VmRSS`, rồi tách `RssAnon/RssFile/RssShmem`, rồi `smaps_rollup` hoặc `smaps` để tìm mapping lớn nhất, cuối cùng đối chiếu `maps` xem đó là `[heap]`, một vùng anonymous giữa các thư viện hay một file cụ thể.

</details>

**Quay lại Q6:** VSZ là address space, RSS là phần resident (Anon + File + Shmem), PSS chia phần dùng chung; với `orders`, hãy nhìn xu hướng bằng `VmRSS` và đối chiếu bằng `smaps_rollup`.

## 7. `mlock`: ghim page trong RAM (Q7)

> **Trả lời ngắn.** `mlock`, `mlock2` và `mlockall` khóa một phần hoặc toàn bộ address space vào RAM, không cho bộ nhớ đó bị đẩy ra swap[^mlock-desc]. Khi `mlock` thành công, các page trong dải địa chỉ được bảo đảm nằm trong RAM cho tới khi mở khóa[^mlock-guarantee]. Process không đặc quyền bị giới hạn bởi `RLIMIT_MEMLOCK`, mức tối đa được khóa[^mlock-limit]. Vượt mức thì lỗi `ENOMEM`[^mlock-enomem].

**Giải thích.**

*Với service `orders`.* Giả sử `orders` giữ khóa ký token trong bộ nhớ và bạn không muốn khóa đó bị chép ra swap, hoặc có một đường xử lý nhạy cảm về latency mà bạn không muốn bị page fault làm chậm. `mlock` cho phép ghim vùng nhớ đó vào RAM, nhưng ghim bao nhiêu thì còn tùy hạn mức, và ghim cũng không miễn phí.

Cơ chế đi theo từng bước:

1. **Khóa theo page, kèm bảo đảm.** `mlock(addr, len)` khóa mọi page chứa một phần của dải địa chỉ; việc khóa và mở khóa tính theo page nguyên[^mlock-page]. Khi lời gọi trả về thành công, các page đó đã nằm trong RAM và ở lại cho tới khi mở khóa[^mlock-guarantee]. `mlock2` với cờ `MLOCK_ONFAULT` nới lỏng hơn: nó chỉ khóa các page đang có trong RAM, rồi đánh dấu cả dải để page chưa có sẽ bị khóa khi được nạp qua page fault[^mlock-onfault].
2. **Hạn mức.** `RLIMIT_MEMLOCK` chỉ áp cho process không đặc quyền[^mlock-limit]. Vượt hạn mức thì lỗi `ENOMEM`, trừ khi process có `CAP_IPC_LOCK`[^mlock-enomem]. Trong lab `ulimit -l` là 8192 kB nên khóa 16 MiB chạm hạn mức (thí nghiệm ở phần Đi sâu).
3. **Trạng thái và vòng đời.** `VmLck` trong `/proc/<pid>/status` là kích thước bộ nhớ đã khóa[^proc-vmsize]. Khóa không xếp chồng: một page đã khóa nhiều lần vẫn được mở bằng một lần `munlock`[^mlock-nostack], và sau đó kernel được phép đẩy page ra swap trở lại[^mlock-munlock]. Khóa tự gỡ khi dải địa chỉ bị unmap[^mlock-munmap]. Process con tạo bởi `fork` không thừa hưởng khóa, còn `execve` hoặc việc process kết thúc cũng gỡ khóa[^mlock-inherit].
4. **Dùng để làm gì.** Hai ứng dụng chính là real-time, vì paging là một nguyên nhân chính gây trễ bất ngờ[^mlock-rt], và bảo mật, vì bí mật không nên bị chép ra swap[^mlock-sec].

<details><summary>Đi sâu: <code>MCL_FUTURE</code> và thí nghiệm trong lab</summary>

- `mlockall(MCL_FUTURE)` còn khiến các syscall về sau (`mmap`, `sbrk`, `malloc`) có thể thất bại nếu vượt giới hạn[^mlock-future].
- Thí nghiệm (C, đã chạy, `ulimit -l` = 8192 kB): `mlock` 16 MiB trả `ENOMEM`; `mlock` 4 MiB thành công và `VmLck=4096kB`. Người soạn không kiểm tra `CAP_IPC_LOCK`; kết quả phù hợp với trường hợp không đặc quyền. Cái giá (suy luận của người soạn): bộ nhớ đã khóa không thu hồi được khi thiếu RAM, nên khóa nhiều làm hệ thống hoặc cgroup dễ hết bộ nhớ hơn.

</details>

**Quay lại Q7:** `mlock` bảo đảm page ở lại RAM (hợp với real-time và bảo mật), nhưng với process không đặc quyền như `orders` thì bị `RLIMIT_MEMLOCK` chặn.

## 8. Nối sang memory cgroup (Q8)

> **Trả lời ngắn.** Hạn mức bộ nhớ của container (cgroup v2) tính nhiều thứ hơn bộ nhớ anonymous của process: cả page cache, cấu trúc dữ liệu kernel như dentry và inode, và socket buffer TCP[^cg-tracked]. Chạm `memory.max` mà mức dùng không giảm được thì OOM killer chạy trong cgroup[^cg-max]. Vì vậy container có thể bị OOM-kill dù `RssAnon` còn thấp, khi phần không thu hồi được (như tmpfs hay bộ nhớ kernel) chiếm chỗ (suy luận của người soạn).

**Giải thích.**

*Với service `orders`.* Đây là tình huống thực tế nhất của cả chương. Pod `orders` bị Kubernetes báo OOMKilled, bạn mở `/proc/<pid>/status` và thấy `RssAnon` chỉ ở mức vừa phải. Vậy ai đã chiếm bộ nhớ? Câu trả lời nằm ở chỗ mọi con số trên là góc nhìn **per-process**, trong khi giới hạn của container được áp ở mức **cgroup**.

Memory controller của cgroup v2 đếm bộ nhớ userland (page cache và anonymous), cấu trúc dữ liệu kernel như dentry và inode, và socket buffer TCP[^cg-tracked]. Đi từng bước:

1. **Tính cho ai.** Một vùng nhớ được tính cho cgroup đã tạo ra nó. Từ quy tắc đó ta suy ra page cache sinh ra khi process trong container đọc file cũng được tính vào container (suy luận của người soạn). Quy tắc "charge" và thí nghiệm ghi file lớn vào cgroup có giới hạn nằm ở [Cgroups v2, mục 4](../08-containers/03-cgroups-v2.md).
2. **Cộng lại thành `memory.current`.** Đây là tổng bộ nhớ mà cgroup và con cháu của nó đang dùng[^cg-current], nên nó **không** bằng tổng `RssAnon` của các process: nó gồm cả page cache và bộ nhớ kernel.
3. **Thu hồi trước, giết sau.** Page có thể giải phóng bất cứ lúc nào, vì dữ liệu có bản ở nơi khác (như page cache của file) hoặc có thể swap ra, được gọi là *reclaimable*[^kdoc-reclaimable]. Page chứa dữ liệu nội bộ của kernel thường không dùng lại được, gọi là *unreclaimable*[^kdoc-unreclaimable]. `memory.max` là hard limit: khi mức dùng chạm giới hạn và không giảm được thì OOM killer được gọi *trong cgroup*[^cg-max]. `memory.high` (chỉ throttle) và các cờ khác nằm ở [Chương 04](./04-page-fault-va-swapping.md) mục 7; danh sách file và thí nghiệm ở [Cgroups v2, mục 4](../08-containers/03-cgroups-v2.md).
4. **Vì sao `RssAnon` thấp vẫn bị giết (suy luận của người soạn).** Page cache sạch được thu hồi dễ nên hiếm khi một mình gây OOM. Thủ phạm thường là phần không thu hồi được: bộ nhớ kernel, socket buffer, và `shmem`, tức dữ liệu filesystem dạng swap-backed như tmpfs, shm segment, shared anonymous mmap[^cg-shmem]. Phần `shmem` đang được map hiện ở `RssShmem` chứ không phải `RssAnon` (mục 6), và nếu máy không có swap thì nó không có chỗ nào để đi.

<details><summary>Đi sâu: <code>memory.stat</code> tách theo loại</summary>

- `memory.stat` tách bộ nhớ theo loại. `anon` là bộ nhớ trong anonymous mapping như `brk()`, `sbrk()` và `mmap(MAP_ANONYMOUS)`[^cg-anon]. `file` là bộ nhớ cache dữ liệu filesystem, gồm tmpfs và shared memory[^cg-file]. Hai loại này gần với `RssAnon` và `RssFile/RssShmem`, nhưng tính trên cả cgroup và gồm cả page cache chưa được map (suy luận của người soạn).
- Các trường để tìm phần không thu hồi được: `kernel` là tổng bộ nhớ kernel (kernel_stack, pagetables, percpu, vmalloc, slab…)[^cg-kernel]; `slab_unreclaimable` là phần slab không thu hồi được khi thiếu bộ nhớ[^cg-slab-unrecl]; `sock` là bộ nhớ cho buffer truyền mạng[^cg-sock].

</details>

**Quay lại Q8:** cgroup tính cả page cache và bộ nhớ kernel vào hạn mức nên `memory.current` của pod `orders` lớn hơn tổng `RssAnon`; OOM-kill xảy ra khi phần không thu hồi được chạm `memory.max`, dù `RssAnon` thấp.

## Liên hệ với Rust

Với `orders` viết bằng Rust, bốn điểm sau nối các mục ở trên với code bạn viết.

- **Static đi đâu.** `static` là một allocation trong chương trình, và mọi tham chiếu hay raw pointer tới nó đều trỏ cùng một allocation[^ref-static-intro]. Nó được khởi tạo bằng một constant expression tính lúc biên dịch[^ref-static-init]. Static không `mut` *có thể* nằm ở bộ nhớ chỉ đọc[^ref-static-ro], đúng như quan sát ở mục 1: mảng 1 MiB toàn số 0 đi vào `.rodata`.
- **Heap Rust trên Unix.** `std::alloc::System` dựa trên `malloc` trên Unix[^sys-alloc]. Còn global allocator mặc định thì "chưa được đặc tả", và chỉ cdylib hay staticlib được bảo đảm dùng `System`[^sys-default]. Vì vậy những gì mục 4, 5 và 6 nói về `brk`, `mmap` và RSS không giảm sau `free` áp dụng cho `Vec` và `Box` trong cấu hình mặc định hiện nay (suy luận của người soạn; docs không hứa điều này cho binary thông thường).
- **`Box::leak`, `mem::forget`** không làm bộ nhớ rời khỏi RSS: bộ nhớ bị leak vẫn là page anonymous resident cho tới khi process kết thúc (suy luận của người soạn; về `forget` và `leak` xem [Chương 08](./08-rust-va-bo-nho.md) mục 6).
- **Đọc memory trong Rust.** Bạn đọc `std::fs::read_to_string("/proc/self/status")` rồi tách `VmRSS` và `RssAnon`. Theo người soạn, danh sách mục của `std` 1.95.0 không có API `mmap` hay `mlock`, nên cần crate `libc`.

## Tóm tắt

- ELF có hai bảng: section cho linker, segment (`PT_LOAD`…) cho loader. `.bss` là `SHT_NOBITS`: chỉ có kích thước; phần dư `p_memsz > p_filesz` được định nghĩa là số 0 khi chạy.
- `execve` dựng lại address space; `PT_INTERP` trỏ tới `ld.so`, nạp thư viện chia sẻ rồi chạy chương trình.
- `maps` liệt kê mapping với quyền, file, offset; `smaps` thêm Rss/Pss/Private/Shared. `[heap]` chỉ là phần `brk`.
- ASLR (`randomize_va_space` 0/1/2) dời mmap base, stack, vdso, text của PIE, và ở mức 2 cả heap.
- `mmap` chỉ tạo mapping; RAM được cấp khi chạm. `MAP_PRIVATE` ghi thì COW, `MAP_SHARED` ghi xuống file; trong thí nghiệm `fork`, ngay sau `fork` PSS của con chỉ xấp xỉ nửa RSS.
- Overcommit mode 0 chỉ từ chối yêu cầu hiển nhiên quá lớn, `MAP_NORESERVE` bỏ qua kiểm tra; VSZ lớn không phải vấn đề.
- `VmRSS = RssAnon + RssFile + RssShmem`, có thể kém chính xác; cần chính xác thì dùng `smaps_rollup`.
- `mlock` ghim page trong RAM, bị `RLIMIT_MEMLOCK` giới hạn với process không đặc quyền; memory cgroup tính cả page cache và bộ nhớ kernel, `memory.max` gọi OOM killer trong cgroup.

## Câu hỏi tự kiểm tra

1. Binary có `.bss` 1 MiB nhưng file chỉ vài trăm KB. Vì sao, và trường nào của `readelf -l` cho thấy?
<details><summary>Đáp án</summary>

`.bss` thuộc kiểu `SHT_NOBITS` nên không chiếm chỗ trong file[^elf-nobits]. Ở `PT_LOAD` ghi được, `p_memsz` lớn hơn `p_filesz` và phần dư là số 0[^elf-load-zero]. Thí nghiệm: `FileSiz 0x3c68` so với `MemSiz 0x103d48`.

</details>

2. Ai đọc `PT_INTERP` và việc gì xảy ra tiếp?
<details><summary>Đáp án</summary>

`execve` dùng interpreter ghi trong `PT_INTERP` để nạp shared object[^execve-interp]. Sau đó `ld.so` tìm và nạp thư viện, chuẩn bị rồi chạy chương trình[^ldso-desc].

</details>

3. Dòng `maps` `rw-p`, inode 0, không pathname, ngay sau segment data của file thực thi thường là gì?
<details><summary>Đáp án</summary>

Đó là phần `.bss` vượt quá file: inode 0 là trường hợp của BSS[^proc-inode], và phần dư được điền số 0[^elf-load-zero]. Ở mục 1 (thí nghiệm `static mut`) vùng này dài đúng 0x100000.

</details>

4. `randomize_va_space` = 1 và = 2 khác nhau thế nào? Vì sao heap tách riêng?
<details><summary>Đáp án</summary>

Mức 1 ngẫu nhiên hóa mmap base, stack, VDSO và code của PIE[^kdoc-aslr1]; mức 2 thêm heap[^kdoc-aslr2]. Heap tách riêng vì có chương trình rất cũ giả định vùng brk nằm ngay sau code và bss[^kdoc-compat].

</details>

5. Sau `mmap` 256 MiB anonymous, `VmSize` và `VmRSS` đổi thế nào? Khi nào RSS tăng?
<details><summary>Đáp án</summary>

`VmSize` tăng ≈ 256 MiB còn `VmRSS` không đổi. RSS tăng khi chạm page (demand zeroing[^ostep-demand-zero]), vì anonymous được khởi tạo bằng 0[^mmap-anon].

</details>

6. Ghi một byte vào mapping `MAP_PRIVATE` của file thì `RssAnon`, `RssFile`, file đổi thế nào? `MAP_SHARED` thì sao?
<details><summary>Đáp án</summary>

Private: COW và không ghi xuống file[^mmap-private] (thí nghiệm: `RssAnon` +4 kB, `RssFile` −4 kB, file không đổi). Shared: ghi xuống file[^mmap-shared] (đọc lại thấy `Y`).

</details>

7. Sau `fork`, RSS con ≈ RSS cha nhưng PSS chỉ một nửa. Vì sao?
<details><summary>Đáp án</summary>

`fork` dùng COW[^fork-cow]. Nhờ đó page được dùng chung[^ostep-cow]. Rss đếm mọi page resident, còn Pss chỉ là phần chia theo tỉ lệ[^proc-pss]. Thí nghiệm: Pss 106 612 kB so với Rss 213 792 kB.

</details>

8. Vì sao `mmap` 1 TiB thất bại nhưng thêm `MAP_NORESERVE` lại thành công ở mode 0? Rủi ro? Mode 2 thì sao?
<details><summary>Đáp án</summary>

Mode 0 từ chối overcommit hiển nhiên[^kdoc-oc0], còn `MAP_NORESERVE` không bị kiểm tra ở mode 0[^proc-oc0]. Rủi ro là ghi vào page khi hết bộ nhớ vật lý có thể nhận `SIGSEGV`[^mmap-noreserve2]. Mode 2 bỏ qua cờ này[^kdoc-oc-gotcha].

</details>

9. `VmRSS` gồm những phần nào; vì sao không nên tin tuyệt đối?
<details><summary>Đáp án</summary>

`VmRSS` là tổng `RssAnon + RssFile + RssShmem`[^proc-vmrss]. Giá trị có thể kém chính xác do một tối ưu hóa scalability, và cần chính xác thì dùng `smaps` hoặc `smaps_rollup`[^proc-statm-inacc].

</details>

10. `mlock` 16 MiB có thể thất bại trong container vì sao; và `memory.current` của cgroup vì sao khác tổng `RssAnon`?
<details><summary>Đáp án</summary>

Process không đặc quyền bị `RLIMIT_MEMLOCK` giới hạn, vượt thì `ENOMEM`[^mlock-enomem] (`ulimit -l` = 8192 kB). cgroup tính cả page cache, dentry/inode và socket buffer TCP[^cg-tracked]. OOM-kill xảy ra khi mức dùng chạm `memory.max` mà không giảm được[^cg-max], và phần thu hồi được như page cache sạch bị thu hồi trước[^kdoc-reclaimable].

</details>

## Đọc thêm

- [`elf(5)`](https://manpages.debian.org/bookworm/manpages/elf.5.en.html): loại segment/section và cờ đầy đủ.
- [`ld.so(8)`](https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html): thứ tự tìm thư viện, biến `LD_*`, secure-execution mode.
- [`proc(5)`](https://manpages.debian.org/bookworm/manpages/proc.5.en.html): `maps`, `smaps`, `status`, `meminfo`.
- [`mmap(2)`](https://manpages.debian.org/bookworm/manpages-dev/mmap.2.en.html) và [Overcommit Accounting](https://docs.kernel.org/mm/overcommit-accounting.html): cờ, lỗi, bảng tính commit.
- [cgroup v2, mục Memory](https://docs.kernel.org/admin-guide/cgroup-v2.html) và [cgroups v2](../08-containers/03-cgroups-v2.md): các tệp `memory.*`.
- [P11 · Chương 06 — ELF, linker, loader](../11-do-dac-va-go-loi/06-elf-linker-loader.md): đi sâu hơn mục 1–3 (section/segment, `PT_LOAD`, `PT_INTERP`, `ld.so`, `load_bias`) và ASLR ở góc loader.
- [P6 · Chương 09 — Zero-copy I/O, mục 6](../06-networking/09-zero-copy-io.md): dùng `mmap` cho I/O, dựa trên ngữ nghĩa `mmap`/`SIGBUS` ở mục 4 của chương này.
- Tiếp theo: [Chương 06 — Heap và allocator](./06-heap-allocator.md).

[^elf-phdr]: elf(5), Program header (Phdr), Debian bookworm — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "An executable or shared object file's program header table is an array of structures, each describing a segment or other information the system needs to prepare the program for execution."
[^elf-seg-sec]: elf(5), Program header (Phdr), Debian bookworm — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "An object file segment contains one or more sections."
[^elf-load]: elf(5), PT_LOAD, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "The array element specifies a loadable segment, described by p_filesz and p_memsz. The bytes from the file are mapped to the beginning of the memory segment."
[^elf-load-zero]: elf(5), PT_LOAD, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "bytes are defined to hold the value 0 and to follow the segment's initialized area. The file size may not be larger than the memory size."
[^elf-flags]: elf(5), p_flags, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "A text segment commonly has the flags PF_X and PF_R . A data segment commonly has PF_W and PF_R."
[^elf-text]: elf(5), .text, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "or executable instructions, of a program."
[^elf-rodata]: elf(5), .rodata, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This section holds read-only data that typically contributes to a nonwritable segment in the process image."
[^elf-data]: elf(5), .data, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This section holds initialized data that contribute to the program's memory image."
[^elf-bss]: elf(5), .bss, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "By definition, the system initializes the data with zeros when the program begins to run. This section is of type SHT_NOBITS."
[^elf-nobits]: elf(5), Section header, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "A section of type SHT_NOBITS may have a nonzero size, but it occupies no space in the file."
[^elf-interp]: elf(5), PT_INTERP, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "The array element specifies the location and size of a null-terminated pathname to invoke as an interpreter."
[^elf-gnustack]: elf(5), PT_GNU_STACK, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "GNU extension which is used by the Linux kernel to control the state of the stack via the flags set in the p_flags member."
[^execve-replace]: execve(2), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/execve.2.en.html> — "calling process to be replaced with a new program, with newly initialized stack, heap, and (initialized and uninitialized) data segments."
[^execve-interp]: execve(2), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/execve.2.en.html> — "the interpreter named in the PT_INTERP segment is used to load the needed shared objects."
[^ldso-desc]: ld.so(8), Debian bookworm — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "The programs ld.so and ld-linux.so* find and load the shared objects (shared libraries) needed by a program, prepare the program to run, and then run it."
[^ldso-search]: ld.so(8), Debian bookworm — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "Using the environment variable LD_LIBRARY_PATH, unless the executable is being run in secure-execution mode"
[^ldso-cache]: ld.so(8), Debian bookworm — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "From the cache file /etc/ld.so.cache, which contains a compiled list of candidate shared objects previously found in the augmented library path."
[^ldso-secure]: ld.so(8), Secure-execution mode, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "the effects of some environment variables are voided or modified, and furthermore those environment variables are stripped from the environment"
[^ldso-bind]: ld.so(8), LD_BIND_NOW, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "causes the dynamic linker to resolve all symbols at program startup instead of deferring function call resolution to the point when they are first referenced."
[^ldso-static]: ld.so(8), Debian bookworm — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "Linux binaries require dynamic linking (linking at run time) unless the -static option was given to ld(1) during compilation."
[^proc-maps]: proc(5), /proc/pid/maps, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "A file containing the currently mapped memory regions and their access permissions."
[^proc-perms]: proc(5), /proc/pid/maps, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "s = shared p = private (copy on write)"
[^proc-inode]: proc(5), /proc/pid/maps, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "0 indicates that no inode is associated with the memory region, as would be the case with BSS (uninitialized data)."
[^proc-pathname]: proc(5), /proc/pid/maps, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "The pathname field will usually be the file that is backing the mapping. For ELF files, you can easily coordinate with the offset field by looking at the Offset field in the ELF program headers (readelf -l)."
[^proc-stack]: proc(5), /proc/pid/maps, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "The initial process's (also known as the main thread's) stack."
[^proc-heap]: proc(5), /proc/pid/maps, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "The process's heap. [anon:name] (since Linux 5.17) A named private anonymous mapping."
[^proc-vdso]: proc(5), /proc/pid/maps, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "The virtual dynamically linked shared object."
[^proc-stacktid]: proc(5), /proc/pid/maps, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "This field was removed in Linux 4.5, since providing this information for a process with large numbers of threads is expensive."
[^proc-smaps]: proc(5), /proc/pid/smaps, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "This file shows memory consumption for each of the process's mappings."
[^proc-smaps-fields]: proc(5), /proc/pid/smaps, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "the number of clean and dirty shared pages in the mapping, and the number of clean and dirty private pages in the mapping."
[^proc-pss]: proc(5), /proc/pid/smaps, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "the process's proportional share of this mapping"
[^proc-vmflags]: proc(5), /proc/pid/smaps (VmFlags), Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "The "VmFlags" line (available since Linux 3.8) represents the kernel flags associated with the virtual memory area"
[^kdoc-aslr]: Linux kernel docs, sysctl/kernel, randomize_va_space — <https://docs.kernel.org/admin-guide/sysctl/kernel.html> — "This option can be used to select the type of process address space randomization that is used in the system, for architectures that support this feature."
[^kdoc-aslr0]: Linux kernel docs, sysctl/kernel, randomize_va_space — <https://docs.kernel.org/admin-guide/sysctl/kernel.html> — "Turn the process address space randomization off. This is the default for architectures that do not support this feature anyways, and kernels that are booted with the “norandmaps” parameter."
[^kdoc-aslr1]: Linux kernel docs, sysctl/kernel, randomize_va_space — <https://docs.kernel.org/admin-guide/sysctl/kernel.html> — "Make the addresses of mmap base, stack and VDSO page randomized. This, among other things, implies that shared libraries will be loaded to random addresses. Also for PIE-linked binaries, the location of code start is randomized."
[^kdoc-aslr2]: Linux kernel docs, sysctl/kernel, randomize_va_space — <https://docs.kernel.org/admin-guide/sysctl/kernel.html> — "Additionally enable heap randomization. This is the default if CONFIG_COMPAT_BRK is disabled."
[^kdoc-compat]: Linux kernel docs, sysctl/kernel, randomize_va_space — <https://docs.kernel.org/admin-guide/sysctl/kernel.html> — "There are however no known non-legacy applications that would be broken this way, so for most systems it is safe to choose full randomization."
[^mmap-new]: mmap(2), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mmap.2.en.html> — "mmap() creates a new mapping in the virtual address space of the calling process."
[^mmap-null]: mmap(2), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mmap.2.en.html> — "If addr is NULL, then the kernel chooses the (page-aligned) address at which to create the mapping"
[^mmap-anon]: mmap(2), MAP_ANONYMOUS, Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mmap.2.en.html> — "The mapping is not backed by any file; its contents are initialized to zero."
[^mmap-file]: mmap(2), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mmap.2.en.html> — "are initialized using length bytes starting at offset offset in the file (or other object) referred to by the file descriptor fd."
[^mmap-fdclose]: mmap(2), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mmap.2.en.html> — "After the mmap() call has returned, the file descriptor, fd, can be closed immediately without invalidating the mapping."
[^munmap]: mmap(2), munmap(), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mmap.2.en.html> — "The region is also automatically unmapped when the process is terminated. On the other hand, closing the file descriptor does not unmap the region."
[^mmap-shared]: mmap(2), MAP_SHARED, Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mmap.2.en.html> — "Share this mapping. Updates to the mapping are visible to other processes mapping the same region, and (in the case of file-backed mappings) are carried through to the underlying file."
[^mmap-private]: mmap(2), MAP_PRIVATE, Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mmap.2.en.html> — "Create a private copy-on-write mapping. Updates to the mapping are not visible to other processes mapping the same file, and are not carried through to the underlying file."
[^mmap-sigbus]: mmap(2), ERRORS/signals, Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mmap.2.en.html> — "Attempted access to a page of the buffer that lies beyond the end of the mapped file."
[^mmap-noreserve]: mmap(2), MAP_NORESERVE, Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mmap.2.en.html> — "Do not reserve swap space for this mapping."
[^mmap-noreserve2]: mmap(2), MAP_NORESERVE, Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mmap.2.en.html> — "When swap space is not reserved one might get SIGSEGV upon a write if no physical memory is available."
[^ostep-demand-zero]: OSTEP, ch.23 Complete VM Systems — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-complete.pdf> — "If the process never accesses the page, all such work is avoided, and thus the virtue of demand zeroing."
[^ostep-cow]: OSTEP, ch.23 Complete VM Systems — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-complete.pdf> — "when the OS needs to copy a page from one address space to another, instead of copying it, it can map it into the target address space and mark it readonly in both address spaces."
[^ostep-cow-fork]: OSTEP, ch.23 Complete VM Systems — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-complete.pdf> — "fork() creates an exact copy of the address space of the caller; with a large address space, making such a copy is slow and data intensive."
[^fork-cow]: fork(2), NOTES, Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/fork.2.en.html> — "Under Linux, fork() is implemented using copy-on-write pages, so the only penalty that it incurs is the time and memory required to duplicate the parent's page tables, and to create a unique task structure for the child."
[^brk-def]: brk(2), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/brk.2.en.html> — "brk() and sbrk() change the location of the program break, which defines the end of the process's data segment (i.e., the program break is the first location after the end of the uninitialized data segment). Increasing the program break has the effect of allocating memory to the process; decreasing the break deallocates memory."
[^brk-avoid]: brk(2), NOTES, Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/brk.2.en.html> — "Avoid using brk() and sbrk(): the malloc(3) memory allocation package is the portable and comfortable way of allocating memory."
[^brk-std]: brk(2), STANDARDS, Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/brk.2.en.html> — "4.3BSD; SUSv1, marked LEGACY in SUSv2, removed in POSIX.1-2001."
[^kdoc-oc-modes]: Linux kernel docs, Overcommit Accounting — <https://docs.kernel.org/mm/overcommit-accounting.html> — "The Linux kernel supports the following overcommit handling modes"
[^kdoc-oc0]: Linux kernel docs, Overcommit Accounting — <https://docs.kernel.org/mm/overcommit-accounting.html> — "Heuristic overcommit handling. Obvious overcommits of address space are refused. Used for a typical system. It ensures a seriously wild allocation fails while allowing overcommit to reduce swap usage. This is the default."
[^kdoc-oc1]: Linux kernel docs, Overcommit Accounting — <https://docs.kernel.org/mm/overcommit-accounting.html> — "Always overcommit. Appropriate for some scientific applications. Classic example is code using sparse arrays and just relying on the virtual memory consisting almost entirely of zero pages."
[^kdoc-oc2]: Linux kernel docs, Overcommit Accounting — <https://docs.kernel.org/mm/overcommit-accounting.html> — "The total address space commit for the system is not permitted to exceed swap + a configurable amount (default is 50%) of physical RAM."
[^proc-oc0]: proc(5), /proc/sys/vm/overcommit_memory, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "In mode 0, calls of mmap(2) with MAP_NORESERVE are not checked, and the default check is very weak, leading to the risk of getting a process"
[^kdoc-oc-view]: Linux kernel docs, Overcommit Accounting — <https://docs.kernel.org/mm/overcommit-accounting.html> — "The current overcommit limit and amount committed are viewable in /proc/meminfo as CommitLimit and Committed_AS respectively."
[^proc-commitlimit]: proc(5), /proc/meminfo, CommitLimit, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "This limit is adhered to only if strict overcommit accounting is enabled"
[^kdoc-oc-gotcha]: Linux kernel docs, Overcommit Accounting — <https://docs.kernel.org/mm/overcommit-accounting.html> — "In mode 2 the MAP_NORESERVE flag is ignored."
[^kdoc-oc-rules]: Linux kernel docs, Overcommit Accounting — <https://docs.kernel.org/mm/overcommit-accounting.html> — "SHARED or READ-only - 0 cost (the file is the map not swap) PRIVATE WRITABLE - size of mapping per instance"
[^kdoc-oc-rules2]: Linux kernel docs, Overcommit Accounting — <https://docs.kernel.org/mm/overcommit-accounting.html> — "For an anonymous or /dev/zero map SHARED - size of mapping PRIVATE READ-only - 0 cost (but of little use) PRIVATE WRITABLE - size of mapping per instance"
[^proc-committed]: proc(5), /proc/meminfo, Committed_AS, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "but touches only 300 MB of that memory will show up as using only 300 MB of memory even if it has the address space allocated for the entire 1 GB."
[^proc-vmsize]: proc(5), /proc/pid/status, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "VmSize Virtual memory size. VmLck Locked memory size (see mlock(2))."
[^proc-statm]: proc(5), /proc/pid/statm, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "size (1) total program size (same as VmSize in /proc/pid/status)"
[^proc-vmrss]: proc(5), /proc/pid/status, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "Resident set size. Note that the value here is the sum of RssAnon, RssFile, and RssShmem."
[^proc-rssanon]: proc(5), /proc/pid/status, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "RssAnon Size of resident anonymous memory. (since Linux 4.5)."
[^proc-rssfile]: proc(5), /proc/pid/status, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "RssFile Size of resident file mappings. (since Linux 4.5)."
[^proc-rssshmem]: proc(5), /proc/pid/status, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "Size of resident shared memory (includes System V shared memory, mappings from tmpfs(5), and shared anonymous mappings)."
[^proc-vmswap]: proc(5), /proc/pid/status, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "Swapped-out virtual memory size by anonymous private pages; shmem swap usage is not included"
[^proc-vmhwm]: proc(5), /proc/pid/status, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "VmHWM Peak resident set size"
[^proc-statm-inacc]: proc(5), /proc/pid/statm, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "Some of these values are inaccurate because of a kernel-internal scalability optimization. If accurate values are required, use /proc/pid/smaps or /proc/pid/smaps_rollup instead, which are much slower but provide accurate, detailed information."
[^mlock-desc]: mlock(2), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mlock.2.en.html> — "lock part or all of the calling process's virtual address space into RAM, preventing that memory from being paged to the swap area."
[^mlock-page]: mlock(2), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mlock.2.en.html> — "Memory locking and unlocking are performed in units of whole pages."
[^mlock-guarantee]: mlock(2), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mlock.2.en.html> — "All pages that contain a part of the specified address range are guaranteed to be resident in RAM when the call returns successfully; the pages are guaranteed to stay in RAM until later unlocked."
[^mlock-rt]: mlock(2), NOTES, Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mlock.2.en.html> — "Real-time applications require deterministic timing, and, like scheduling, paging is one major cause of unexpected program execution delays."
[^mlock-sec]: mlock(2), NOTES, Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mlock.2.en.html> — "As a result of paging, these secrets could be transferred onto a persistent swap store medium"
[^mlock-limit]: mlock(2), Limits and permissions, Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mlock.2.en.html> — "Since Linux 2.6.9, no limits are placed on the amount of memory that a privileged process can lock and the RLIMIT_MEMLOCK soft resource limit instead defines a limit on how much memory an unprivileged process may lock."
[^mlock-enomem]: mlock(2), ERRORS, Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mlock.2.en.html> — "the caller had a nonzero RLIMIT_MEMLOCK soft resource limit, but tried to lock more memory than the limit permitted. This limit is not enforced if the process is privileged (CAP_IPC_LOCK)."
[^mlock-future]: mlock(2), mlockall(), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mlock.2.en.html> — "may fail if it would cause the number of locked bytes to exceed the permitted maximum"
[^cg-tracked]: Linux kernel docs, cgroup v2, Memory — <https://docs.kernel.org/admin-guide/cgroup-v2.html> — "Userland memory - page cache and anonymous memory. Kernel data structures such as dentries and inodes. TCP socket buffers."
[^cg-current]: Linux kernel docs, cgroup v2, memory.current — <https://docs.kernel.org/admin-guide/cgroup-v2.html> — "The total amount of memory currently being used by the cgroup and its descendants."
[^cg-anon]: Linux kernel docs, cgroup v2, memory.stat — <https://docs.kernel.org/admin-guide/cgroup-v2.html> — "Amount of memory used in anonymous mappings such as brk(), sbrk(), and mmap(MAP_ANONYMOUS)."
[^cg-file]: Linux kernel docs, cgroup v2, memory.stat — <https://docs.kernel.org/admin-guide/cgroup-v2.html> — "Amount of memory used to cache filesystem data, including tmpfs and shared memory."
[^cg-max]: Linux kernel docs, cgroup v2, memory.max — <https://docs.kernel.org/admin-guide/cgroup-v2.html> — "Memory usage hard limit. This is the main mechanism to limit memory usage of a cgroup. If a cgroup’s memory usage reaches this limit and can’t be reduced, the OOM killer is invoked in the cgroup."
[^ref-static-intro]: The Rust Reference, Static items — <https://doc.rust-lang.org/1.95.0/reference/items/static-items.html> — "A static item is similar to a constant, except that it represents an allocation in the program that is initialized with the initializer expression. All references and raw pointers to the static refer to the same allocation."
[^ref-static-ro]: The Rust Reference, Static items — <https://doc.rust-lang.org/1.95.0/reference/items/static-items.html> — "Non-mut static items that contain a type that is not interior mutable may be placed in read-only memory."
[^ref-static-init]: The Rust Reference, Static items — <https://doc.rust-lang.org/1.95.0/reference/items/static-items.html> — "The static initializer is a constant expression evaluated at compile time."
[^ref-static-mut]: The Rust Reference, Static items — <https://doc.rust-lang.org/1.95.0/reference/items/static-items.html> — "an unsafe block is required when either reading or writing a mutable static variable."
[^sys-alloc]: Rust std 1.95.0, `std::alloc::System` — <https://doc.rust-lang.org/1.95.0/std/alloc/struct.System.html> — "This is based on malloc on Unix platforms and HeapAlloc on Windows, plus related functions."
[^sys-default]: Rust std 1.95.0, `std::alloc::System` — <https://doc.rust-lang.org/1.95.0/std/alloc/struct.System.html> — "Currently the default global allocator is unspecified. Libraries, however, like cdylibs and staticlibs are guaranteed to use the System by default"
[^ostep-convention]: OSTEP, ch.13 The Abstraction: Address Spaces — <https://pages.cs.wisc.edu/~remzi/OSTEP/vm-intro.pdf> — "However, this placement of stack and heap is just a convention; you could arrange the address space in a different way if you’d like"
[^execve-mappings]: execve(2), Effect on process attributes, Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/execve.2.en.html> — "Memory mappings are not preserved (mmap(2))."
[^execve-pid]: execve(2), NOTES, Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/execve.2.en.html> — "there is no new process; many attributes of the calling process remain unchanged (in particular, its PID)."
[^kdoc-pss]: Linux kernel docs, The /proc Filesystem, smaps — <https://docs.kernel.org/filesystems/proc.html> — "The “proportional set size” (PSS) of a process is the count of pages it has in memory, where each page is divided by the number of processes sharing it. So if a process has 1000 pages all to itself, and 1000 shared with one other process, its PSS will be 1500."
[^elf-needed]: elf(5), Dynamic section, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "String table offset to name of a needed library"
[^elf-progbits-data]: elf(5), .data, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This section holds initialized data that contribute to the program's memory image. This section is of type SHT_PROGBITS."
[^elf-progbits-ro]: elf(5), .rodata, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This section holds read-only data that typically contributes to a nonwritable segment in the process image. This section is of type SHT_PROGBITS."
[^elf-progbits-text]: elf(5), .text, Debian bookworm — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "or executable instructions, of a program. This section is of type SHT_PROGBITS."
[^ps-vsz]: ps(1), Debian bookworm — <https://manpages.debian.org/bookworm/procps/ps.1.en.html> — "virtual memory size of the process in KiB (1024-byte units)."
[^ps-rss]: ps(1), Debian bookworm — <https://manpages.debian.org/bookworm/procps/ps.1.en.html> — "resident set size, the non-swapped physical memory that a task has used"
[^mlock-onfault]: mlock(2), mlock2(), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mlock.2.en.html> — "Lock pages that are currently resident and mark the entire range so that the remaining nonresident pages are locked when they are populated by a page fault."
[^mlock-nostack]: mlock(2), NOTES, Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mlock.2.en.html> — "Memory locks do not stack, that is, pages which have been locked several times by calls to mlock(), mlock2(), or mlockall() will be unlocked by a single call to munlock() for the corresponding range or by munlockall()."
[^mlock-munlock]: mlock(2), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mlock.2.en.html> — "After this call, all pages that contain a part of the specified memory range can be moved to external swap space again by the kernel."
[^mlock-munmap]: mlock(2), NOTES, Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mlock.2.en.html> — "The memory lock on an address range is automatically removed if the address range is unmapped via munmap(2)."
[^mlock-inherit]: mlock(2), NOTES, Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/mlock.2.en.html> — "Memory locks are not inherited by a child created via fork(2) and are automatically removed (unlocked) during an execve(2) or when the process terminates."
[^kdoc-reclaimable]: Linux kernel docs, Concepts overview, Reclaim — <https://docs.kernel.org/admin-guide/mm/concepts.html> — "The pages that can be freed at any time, either because they cache the data available elsewhere, for instance, on a hard disk, or because they can be swapped out, again, to the hard disk, are called reclaimable."
[^kdoc-unreclaimable]: Linux kernel docs, Concepts overview, Reclaim — <https://docs.kernel.org/admin-guide/mm/concepts.html> — "In most cases, the pages holding internal kernel data and used as DMA buffers cannot be repurposed, and they remain pinned until freed by their user. Such pages are called unreclaimable."
[^cg-shmem]: Linux kernel docs, cgroup v2, memory.stat (shmem) — <https://docs.kernel.org/admin-guide/cgroup-v2.html> — "Amount of cached filesystem data that is swap-backed, such as tmpfs, shm segments, shared anonymous mmap()s"
[^cg-kernel]: Linux kernel docs, cgroup v2, memory.stat (kernel) — <https://docs.kernel.org/admin-guide/cgroup-v2.html> — "Amount of total kernel memory, including (kernel_stack, pagetables, percpu, vmalloc, slab) in addition to other kernel memory use cases."
[^cg-slab-unrecl]: Linux kernel docs, cgroup v2, memory.stat (slab_unreclaimable) — <https://docs.kernel.org/admin-guide/cgroup-v2.html> — "Part of “slab” that cannot be reclaimed on memory pressure."
[^cg-sock]: Linux kernel docs, cgroup v2, memory.stat (sock) — <https://docs.kernel.org/admin-guide/cgroup-v2.html> — "Amount of memory used in network transmission buffers"
