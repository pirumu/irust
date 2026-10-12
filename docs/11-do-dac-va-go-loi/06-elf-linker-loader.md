# P11 · Chương 06 — ELF, linker và loader

> Phạm vi: mọi khẳng định về kernel/syscall là về **Linux** (man page Debian bookworm, `man-pages 6.03`; mã nguồn kernel `v6.8`). Phần `ld.so` mô tả **glibc**; phần musl lấy từ tài liệu của dự án musl. Chương này **không chạy thí nghiệm**: lệnh `readelf`/`objdump` trong chương là lệnh minh họa để bạn tự chạy, không kèm kết quả. Ví dụ mô tả PLT lấy từ tài liệu của Drepper viết cho IA-32 (x86 32-bit); mục 5 đối chiếu thêm với đặc tả x86-64 psABI. Cơ chế hai bên tương tự nhưng chi tiết khác nhau theo kiến trúc.

## Bạn sẽ hiểu được gì

1. **Q1.** Trong một file ELF có những gì; *section* khác *segment* ra sao, và vì sao file cần hai bảng mục lục? → mục 1
2. **Q2.** Symbol và relocation là gì; linker biến nhiều file `.o` thành một file chạy được bằng cách nào? → mục 2
3. **Q3.** `execve` nạp một ELF ra sao; `PT_INTERP` và `ld.so` vào cuộc lúc nào? → mục 3
4. **Q4.** `ld.so` tìm thư viện ở đâu (`LD_LIBRARY_PATH`, `RPATH`/`RUNPATH`, `ld.so.cache`)? → mục 4
5. **Q5.** PLT/GOT và lazy binding chạy thế nào; RELRO bảo vệ cái gì? → mục 5
6. **Q6.** PIE và ASLR là gì; kernel đặt chương trình ở địa chỉ nào? → mục 6
7. **Q7.** Static hay dynamic, glibc hay musl, symbol versioning: binary chạy trong container nên chọn gì? → mục 7
8. **Q8.** Rust tương ứng với các khái niệm trên thế nào: crate type, target `*-musl`, `-C relocation-model`, `strip`, kích thước binary? → mục 8

## Kiến thức cần trước

Chương này đi theo một service Rust từ lúc nó còn là các file `.o` cho tới lúc thành process đang chạy trong container. Để theo được, bạn cần ba thứ từ các chương trước:

- [P2·03 — Assembly x86-64 và AArch64](../02-cpu/03-assembly-x86-64-va-aarch64.md): đọc được `call`, `jmp` và cách lấy địa chỉ kiểu `@GOTPCREL(%rip)`; chương này dùng lại khi nói về GOT/PLT.
- [P3·05 — Bộ nhớ của tiến trình trên Linux](../03-memory/05-bo-nho-tien-trinh-linux.md): mục 1–3 đã giới thiệu ELF, `.bss`, `PT_LOAD`, `/proc/<pid>/maps` và ASLR ở mức nhập môn. Chương này đi sâu vào *file* ELF, linker và loader, không lặp lại thí nghiệm `.bss`.
- [P8·08 — Rust trong container](../08-containers/08-rust-trong-container.md): mục 5–6 đo kích thước image musl/glibc và tác dụng của `strip`. Chương này giải thích *vì sao* những con số đó khác nhau.

## 1. ELF: header, section và segment (Q1)

> **Trả lời ngắn.** ELF là định dạng file của executable, thư viện và object trên Linux. File bắt đầu bằng ELF header ở offset 0, trỏ tới hai bảng mục lục: *section header table* (dành cho linker và công cụ) và *program header table* (dành cho loader/kernel)[^elf-hdr-tables]. Một segment chứa một hoặc nhiều section[^elf-phdr]. File dùng để dựng process bắt buộc phải có program header table[^gabi-pht].

**Giải thích.**

*Với service `orders`.* Giả sử `orders` là một service Rust (ví dụ giả định của người soạn, không phải code trong repo), được build trên CI, đóng vào container image rồi chạy trên Kubernetes. Thứ nằm trên đĩa của image là một file ELF tên `orders`. Mỗi lần container khởi động, kernel phải đọc file này, quyết định nạp phần nào vào bộ nhớ, rồi mới chuyển điều khiển cho code của service. Ta sẽ bám theo `orders` suốt chương, từ các file `.o` lúc build tới process đang chạy. Câu hỏi của mục này: trong file `orders` có những gì, và kernel cần đọc phần nào?

### 1.1 Ba loại file ELF

Trước khi thành file chạy được, `orders` đi qua nhiều file ELF, nên cần phân biệt ba loại. *Relocatable* (đầu ra của compiler, thường là `.o`) chứa code và data sẵn sàng để linker ghép với các object khác, còn *executable* chứa một chương trình sẵn sàng chạy[^gabi-3types]. *Shared object* (`.so`) dùng được ở hai ngữ cảnh: linker ghép nó với các object khác, rồi dynamic linker kết hợp nó với executable lúc chạy để tạo process image[^gabi-so]. Loại file nằm ở trường `e_type` của ELF header, với ba giá trị `ET_REL`, `ET_EXEC`, `ET_DYN`[^elf-etype]. Có một ca dễ nhầm: PIE (mục 6) mang `e_type` là `ET_DYN` chứ không phải `ET_EXEC`, nhưng khác shared object thường ở chỗ nó có *interpreter*, tức chương trình mà `PT_INTERP` chỉ tới (chính là `ld.so`, mục 3)[^binfmt-pie].

### 1.2 Hai bảng mục lục

Cùng một file mà lúc build và lúc chạy cần thông tin khác nhau, nên ELF cho hai "khung nhìn" song song lên nó[^gabi-views]. Hình dưới là bố cục của hai khung nhìn đó.

```mermaid
flowchart LR
  subgraph L["Linking view (cho linker)"]
    l1["ELF header"]
    l2["Program header table — tùy chọn"]
    l3["Section 1 … Section n"]
    l4["Section header table — bắt buộc"]
  end
  subgraph E["Execution view (cho loader)"]
    e1["ELF header"]
    e2["Program header table — bắt buộc"]
    e3["Segment 1 … Segment n"]
    e4["Section header table — tùy chọn"]
  end
```

(Sơ đồ theo Hình 4-1 của gABI. Thứ tự thực tế trong file có thể khác hình, chỉ ELF header có vị trí cố định[^gabi-order].)

Từ hình suy ra hai điều. File dùng để dựng process phải có program header table, còn relocatable thì không cần[^gabi-pht]. Ngược lại, file dùng lúc linking phải có section header table, còn các file khác có hay không đều được[^gabi-sht]. Ghép hai điều ấy lại, kernel nạp chương trình dựa vào segment, còn section là thông tin cho linker và công cụ như `readelf` (suy luận của người soạn từ hai trích dẫn trên). Với `orders`, khi container khởi động kernel đọc program header table chứ không cần tới section.

### 1.3 ELF header

ELF header nằm ở offset 0[^elf-hdr-tables]. Các trường đáng nhớ:

| Trường | Nói gì |
|---|---|
| `e_ident` | 16 byte nhận dạng; byte đầu là magic `0x7f`, theo sau là `'E'`, `'L'`, `'F'`[^elf-magic] |
| `e_type` | loại file: `ET_REL`, `ET_EXEC` hoặc `ET_DYN`[^elf-etype] |
| `e_machine` | kiến trúc mà file yêu cầu, ví dụ `EM_X86_64`[^elf-machine] |
| `e_entry` | địa chỉ ảo mà hệ thống chuyển điều khiển tới đầu tiên[^elf-entry] |
| `e_phoff` | vị trí (offset trong file) của program header table[^elf-phoff] |
| `e_phnum` | số phần tử của program header table[^elf-phnum] |
| `e_shoff` | vị trí của section header table; bằng 0 nếu file không có bảng này[^elf-shoff] |
| `e_shstrndx` | chỉ số của section chứa tên các section[^elf-shstrndx] |

### 1.4 Section: đơn vị của linker

Section header table cho phép định vị mọi section trong file[^elf-shdr]. Mỗi section có tên (là chỉ số vào bảng chuỗi tên section), kiểu, cờ, địa chỉ, offset và kích thước. Hai điểm trong đó ảnh hưởng tới chuyện chạy chương trình:

- Cờ `SHF_ALLOC` đánh dấu section chiếm bộ nhớ khi chương trình chạy, và một số section điều khiển không có mặt trong memory image[^elf-shf-alloc]. Ví dụ section `.debug` chứa thông tin cho debugger và nội dung của nó không được đặc tả[^elf-debug]; loader không cần nó để chạy chương trình (suy luận của người soạn).
- Kiểu `SHT_NOBITS` là section không chiếm chỗ trong file nhưng ngoài ra giống `SHT_PROGBITS`[^elf-nobits]. `.bss` là ví dụ quen thuộc[^elf-bss]; thí nghiệm quan sát nó nằm ở [P3·05 mục 1](../03-memory/05-bo-nho-tien-trinh-linux.md).

Các section sẽ gặp lại ở những mục sau (tên là quy ước theo `elf(5)`):

| Section | Chứa gì |
|---|---|
| `.interp` | pathname của program interpreter[^elf-interp-sec] |
| `.dynamic` | thông tin liên kết động[^elf-dynamic-sec] |
| `.dynsym` | bảng symbol dành cho liên kết động, ở dạng tối thiểu[^elf-dynsym] |
| `.symtab` | bảng symbol đầy đủ, điển hình dùng cho link editing[^elf-symtab] |
| `.relaNAME` | các relocation áp lên section `NAME`[^elf-relaname] |
| `.got` | global offset table (mục 5)[^elf-got] |
| `.plt` | procedure linkage table (mục 5)[^elf-plt] |
| `.gnu.version_r` | các version symbol mà file *cần* (mục 7)[^elf-verneed] |

### 1.5 Segment: đơn vị của loader

Program header table là một mảng cấu trúc. Mỗi phần tử mô tả một segment, hoặc một thông tin khác mà hệ thống cần để chuẩn bị chạy chương trình[^elf-phdr]. Với `orders`, ba loại `p_type` quan trọng nhất là:

| `p_type` | Ý nghĩa |
|---|---|
| `PT_LOAD` | segment nạp được, mô tả bằng `p_filesz` và `p_memsz`; byte của file được map vào đầu segment[^elf-load] |
| `PT_INTERP` | đường dẫn (kết thúc bằng null) của interpreter cần gọi; xuất hiện tối đa một lần, và nếu có thì phải đứng trước mọi `PT_LOAD`[^elf-interp] |
| `PT_DYNAMIC` | thông tin liên kết động[^elf-ptdynamic] |

Với `PT_LOAD`, phần `p_memsz` dư so với `p_filesz` được định nghĩa là số 0[^elf-load-zero]. Từ đó suy ra cách một segment có `.bss` mà file không tốn chỗ cho nó (suy luận của người soạn từ `SHT_NOBITS` và `PT_LOAD`). Quyền truy cập của segment nằm ở `p_flags`: segment text (chứa mã lệnh) thường có `PF_X` và `PF_R`, segment data thường có `PF_W` và `PF_R`[^elf-pflags].

<details><summary>Đi sâu: <code>e_phnum</code> tràn, căn chỉnh <code>p_vaddr</code>/<code>p_offset</code>, <code>PT_PHDR</code>, <code>PT_GNU_STACK</code>, lệnh để tự xem</summary>

- **Số phần tử vượt giới hạn trường.** Nếu số phần tử program header ≥ `PN_XNUM` (0xffff) thì `e_phnum` giữ giá trị `PN_XNUM` và số thật nằm ở `sh_info` của phần tử đầu section header table[^elf-phnum-xnum]. Hai trường kia tràn theo cơ chế riêng, với ngưỡng `SHN_LORESERVE` (0xff00). Khi vượt ngưỡng, `e_shnum` bằng 0 và số thật nằm ở `sh_size` của phần tử đầu section header table[^elf-shnum-xindex]. Còn `e_shstrndx` giữ `SHN_XINDEX` (0xffff) và chỉ số thật nằm ở `sh_link` của phần tử đầu[^elf-shstrndx-xindex].
- **Căn chỉnh.** Segment nạp được phải có `p_vaddr` và `p_offset` đồng dư theo kích thước page[^elf-malign]. Suy luận của người soạn: đó là điều kiện để kernel map trực tiếp page của file vào địa chỉ ảo.
- **Thêm hai loại `p_type`.** `PT_PHDR` cho biết vị trí của chính program header table trong memory image, và nếu có thì cũng phải đứng trước mọi `PT_LOAD`[^elf-phdr-self]. `PT_GNU_STACK` là extension GNU, kernel Linux dùng nó để điều khiển trạng thái stack qua `p_flags`[^elf-gnustack].
- **Đọc ELF header bằng tay.** Đoạn mã minh họa (không phải kết quả đã chạy) đọc 16 byte đầu rồi so với `7f 45 4c 46`. Byte thứ 5 là class, trong đó `ELFCLASS64` định nghĩa kiến trúc 64-bit[^elf-class].
- **Lệnh để tự xem** (chưa chạy trong chương này): `readelf -h` (ELF header), `readelf -lW` (program header, tức segment), `readelf -SW` (section header), `readelf -d` (`.dynamic`), `objdump -p file | grep NEEDED` (các `DT_NEEDED`).
- **Core file** cũng là ELF (`ET_CORE`). Phần ghi chú (note) cho phép đính kèm thông tin tùy ý cho hệ thống dùng, và chủ yếu thấy trong core file[^elf-notes].

</details>

**Quay lại Q1:** ELF có header ở offset 0 trỏ tới hai bảng: section (cho linker/công cụ) và segment (cho loader). Lúc chạy, kernel chỉ cần các segment, đặc biệt `PT_LOAD` và `PT_INTERP`.

## 2. Symbol và relocation: linker làm gì (Q2)

> **Trả lời ngắn.** Mỗi file `.o` định nghĩa một số *symbol* (tên hàm, tên biến) và tham chiếu tới những symbol định nghĩa ở nơi khác. Địa chỉ cuối cùng của chúng chưa biết lúc compile, nên compiler để lại chỗ trống kèm một *relocation entry* ghi "chỗ này cần điền địa chỉ của symbol X". Relocation là quá trình nối tham chiếu symbol với định nghĩa symbol[^gabi-reloc]. Linker gom các object lại, tìm định nghĩa cho từng tham chiếu rồi điền vào chỗ trống.

**Giải thích.**

*Với service `orders`.* Code của `orders` gọi những hàm nó không tự định nghĩa. Giả sử một đoạn gọi `SSL_read` của OpenSSL để đọc dữ liệu TLS từ client (ví dụ giả định). Khi compiler biên dịch file chứa lời gọi đó thành `.o`, nó chưa thể biết `SSL_read` nằm ở địa chỉ nào. Mục này trả lời hai câu hỏi: compiler ghi lại điều chưa biết ấy ra sao, và ai điền vào sau.

### 2.1 Hai loại linker

Có hai linker cần phân biệt. *Static linker* (`ld`) chạy lúc build và ghép các object thành file đầu ra. *Dynamic linker* (`ld.so`) chạy lúc chương trình khởi động: nó tìm và nạp các shared object rồi chuẩn bị chương trình để chạy[^ldso-desc]. Chữ "linker" đứng một mình trong mục này là static linker (giải nghĩa thuật ngữ của người soạn).

### 2.2 Symbol và relocation entry

Mỗi object mang một bảng symbol, giữ thông tin cần để định vị và relocate các định nghĩa symbol và tham chiếu symbol của chương trình[^elf-symtab-def]. Có hai bảng, đã gặp ở bảng section của mục 1: `.symtab` đầy đủ dùng khi link, `.dynsym` tối thiểu dùng cho liên kết động.

Trong `.o` của `orders`, `SSL_read` có chỉ số section `st_shndx` là `SHN_UNDEF`, tức *chưa định nghĩa* trong file này. Khi linker ghép file đó với file định nghĩa `SSL_read`, tham chiếu sẽ được nối với định nghĩa thật[^gabi-undef]. Chỗ trong code cần địa chỉ của `SSL_read` được compiler để trống, kèm một relocation entry. Đến lúc điền, cách áp dụng điển hình là: xác định giá trị của symbol được tham chiếu, lấy addend (hằng số cộng thêm), áp công thức của loại relocation lên cặp (symbol, addend), rồi đặt kết quả vào chỗ cần sửa[^gabi-reloc-apply]. Các trường của symbol và của relocation entry nằm ở phần Đi sâu.

### 2.3 Ba binding quyết định cách xử lý tên trùng

Khi hai object cùng có symbol trùng tên, kết quả phụ thuộc vào *binding* của symbol:

- `STB_LOCAL`: không nhìn thấy ngoài object chứa định nghĩa, nên hai file có thể có local symbol cùng tên mà không đụng nhau[^elf-bind-local].
- `STB_GLOBAL`: nhìn thấy với mọi object đang được ghép; định nghĩa ở file này thỏa mãn tham chiếu chưa định nghĩa ở file khác[^elf-bind-global]. Linker không cho phép nhiều định nghĩa `STB_GLOBAL` cùng tên[^gabi-global-weak].
- `STB_WEAK`: giống global nhưng định nghĩa có độ ưu tiên thấp hơn[^elf-bind-weak]. Nếu đã có định nghĩa global thì weak cùng tên không gây lỗi và linker giữ bản global[^gabi-global-weak].

### 2.4 Linker ghép các mảnh

Hình dưới là quy trình khái quát (của người soạn; các điểm chi tiết có chú thích bên dưới):

```mermaid
flowchart LR
  A["foo.o<br/>(ET_REL)"] --> L["linker"]
  B["bar.o<br/>(ET_REL)"] --> L
  C["libx.a<br/>(archive)"] --> L
  S["liby.so<br/>(shared object)"] --> L
  L --> X["file thực thi / .so<br/>(ET_EXEC / ET_DYN)"]
```

Linker xử lý ba nguồn đầu vào theo ba cách khác nhau.

**Archive `.a`.** Linker *rút* (extract) những member chứa định nghĩa cho symbol global còn chưa định nghĩa[^gabi-archive-extract]. Những member đó được chép vào file đầu ra[^gabi-archive-copy]. Với tham chiếu weak chưa định nghĩa thì linker không rút member, và weak chưa giải quyết có giá trị 0[^gabi-weak-undef]. Vì vậy một binary liên kết tĩnh chỉ chứa các member archive được rút để lấp symbol còn thiếu chứ không chứa cả thư viện (suy luận của người soạn từ các trích dẫn đó). Đơn vị rút là member, nên phần không dùng *bên trong* một member đã rút vẫn có mặt.

**Shared object.** Linker chỉ xử lý shared object cùng các object khác; việc kết hợp nó với executable để tạo process image thuộc về dynamic linker lúc chạy[^gabi-so]. Nói cách khác, định nghĩa nằm lại trong `.so` và tham chiếu được ghi lại cho dynamic linker (suy luận của người soạn; mục 3–5 đi tiếp). Với `orders`, nếu `SSL_read` nằm trong `libssl.so` thì `orders` không chứa hàm đó, nó chỉ ghi lại rằng cần nó.

**Executable truyền thống.** Địa chỉ nạp đã biết ngay lúc link, nên static linker tự thực hiện relocation, và ứng dụng không có relative relocation[^dr-no-relative]. Câu này của Drepper nói về executable cố định địa chỉ; PIE ở mục 6 khác.

<details><summary>Đi sâu: các trường symbol và relocation, visibility, relocation tĩnh vs động, tên symbol, lệnh tự xem</summary>

- **Một entry trong bảng symbol** có các trường sau:

  | Trường | Nói gì |
  |---|---|
  | `st_name` | chỉ số vào bảng chuỗi symbol, nơi giữ tên symbol[^elf-st-name] |
  | `st_value` | giá trị của symbol[^elf-st-value] |
  | `st_size` | kích thước; bằng 0 nếu symbol không có kích thước hoặc chưa biết[^elf-st-size] |
  | `st_info` | loại và binding của symbol[^elf-st-info] |
  | `st_other` | visibility của symbol[^elf-st-other] |
  | `st_shndx` | chỉ số section header mà symbol thuộc về[^elf-st-shndx] |

- **Một relocation entry** ghi chỗ cần sửa và cách sửa:

  | Trường | Nói gì |
  |---|---|
  | `r_offset` | trong relocatable file là offset từ đầu một section; trong executable/shared object là địa chỉ ảo của chỗ cần sửa[^elf-roffset] |
  | `r_info` | chỉ số symbol cần dùng và loại relocation[^elf-relocs] |
  | `r_addend` | hằng số cộng thêm, chỉ có ở dạng `Rela`[^elf-raddend] |

  Loại relocation phụ thuộc bộ xử lý và được mô tả trong processor supplement[^gabi-reloc-type]. Dạng `Rel` không có trường addend, mà để giá trị cộng thêm ngay tại chỗ cần sửa[^gabi-rel-addend].
- **Visibility.** `STV_DEFAULT`: symbol global/weak có sẵn cho module khác, và tham chiếu trong module có thể bị định nghĩa ở module khác *chèn vào thay thế* (interposition)[^elf-vis-default]. `STV_HIDDEN`: không thấy ngoài module, và tham chiếu trong module luôn trỏ vào bản cục bộ[^elf-vis-hidden]. Điều này liên quan tới việc lời gọi có cần đi qua PLT hay không (mục 5; suy luận của người soạn).
- **Relocation còn lại cho dynamic linker.** Trong executable/shared object, các relocation này nằm ở các section `.rela*`. `DT_JMPREL` trỏ vào nhóm relocation gắn riêng với PLT, để dynamic linker bỏ qua chúng lúc khởi tạo nếu bật lazy binding[^gabi-jmprel].
- **Hai loại relocation động** theo Drepper. *Relative*: đích nằm trong chính object, linker biết sẵn offset, dynamic linker chỉ cộng địa chỉ nạp rồi ghi kết quả[^dr-relative]. *Theo symbol*: phải tìm định nghĩa, và đó là việc tốn kém, càng nhiều DSO và symbol thì tra cứu càng lâu[^dr-symreloc-cost].
- **Text relocation.** Nếu relocation phải sửa vào segment chỉ-đọc, page bị sửa không còn chia sẻ được với process khác dùng cùng object[^dr-textrel]. Đó là một lý do thư viện được biên dịch position-independent (mục 5).
- **Tên symbol** có thể khác tên trong mã nguồn: Rust "mangle" tên symbol (mục 8). Liên hệ với assembly: `call foo@PLT` hay `@GOTPCREL(%rip)` trong [P2·03](../02-cpu/03-assembly-x86-64-va-aarch64.md) là những chỗ trong mã mà địa chỉ phải được điền lúc link hoặc lúc chạy (suy luận của người soạn; mục 5 giải thích PLT/GOT).
- **Lệnh tự xem** (chưa chạy): `readelf -sW file.o` (symbol), `readelf -rW file.o` (relocation trong `.o`), `readelf -rW exe` (relocation động còn lại).

</details>

**Quay lại Q2:** `.o` của `orders` chỉ chứa tên và những "lỗ" cần điền. Linker nối các tham chiếu symbol với định nghĩa và điền địa chỉ (relocation); phần nào thuộc shared object thì để dynamic linker làm lúc chạy.

## 3. `execve` nạp ELF: `PT_INTERP`, auxv, `ld.so` (Q3)

> **Trả lời ngắn.** `execve` thay chương trình đang chạy của process bằng một chương trình mới[^execve-replace]. Kernel đọc program header; nếu có `PT_INTERP` thì nó mở file interpreter đó, map cả chương trình lẫn interpreter, dựng auxiliary vector trên stack rồi chuyển điều khiển cho *interpreter* chứ không phải cho chương trình[^dr-entry]. Interpreter (`ld.so`) nạp các thư viện cần, relocate, rồi mới chạy chương trình[^ldso-desc].

**Giải thích.**

*Với service `orders`.* Khi ai đó chạy `orders` (từ shell, hay từ entrypoint của container), kernel xử lý việc đó bằng `execve`. Nếu `orders` liên kết tĩnh thì file đã đủ, kernel chỉ việc đặt nó vào bộ nhớ rồi nhảy vào entry. Nếu `orders` liên kết động, và đó là mặc định của target `*-gnu` (mục 8), thì file còn thiếu một phần: địa chỉ của `SSL_read` và mọi hàm lấy từ thư viện khác vẫn là những chỗ trống (mục 2). Kernel không tự lấp được chúng, nên nó nhờ `ld.so` làm việc đó trước khi code của `orders` được chạy. Mục này đi qua từng bước của chuyện đó.

### 3.1 Từng bước với binary liên kết động

**Bước 1: thay chương trình.** `execve()` thay chương trình hiện tại bằng chương trình mới, với stack, heap và data segment mới khởi tạo[^execve-replace]. Các memory mapping cũ không được giữ lại[^execve-mappings].

**Bước 2: tìm `PT_INTERP`.** Binary tự ghi interpreter của nó trong program header, kernel không hard-code `ld.so`[^dr-ptinterp]. Trong source kernel `v6.8`, hàm `load_elf_binary` duyệt program header và bỏ qua mọi phần tử không phải `PT_INTERP`[^binfmt-interp]. Với phần tử `PT_INTERP`, nó đọc chuỗi đường dẫn rồi mở file đó[^binfmt-open]. Vòng lặp dừng ở `PT_INTERP` đầu tiên[^binfmt-interp-break].

**Bước 3: map hai ELF.** Kernel map các `PT_LOAD` của chương trình, rồi gọi `load_elf_interp` để map interpreter và nhận lại địa chỉ nạp của nó[^binfmt-interp-load]. Interpreter phải hoàn chỉnh và có thể relocate tự do[^dr-interp-complete]. Lý do (suy luận của người soạn) là chính nó không có ai nạp hộ.

**Bước 4: dựng auxiliary vector (auxv).** Kernel đặt một mảng cặp (tag, value) lên stack của process mới. Mảng này giúp dynamic linker tránh được nhiều syscall[^dr-auxv]. Source kernel đặt vào đó, trong số các mục khác, `AT_PHDR`, `AT_PHENT`, `AT_PHNUM`, `AT_BASE` và `AT_ENTRY`[^binfmt-auxv]. Ba tag `ld.so` cần nhất mang ý nghĩa sau, theo `getauxval(3)`:

| Tag | Ý nghĩa |
|---|---|
| `AT_PHDR` | địa chỉ program header của executable[^auxv-phdr] |
| `AT_BASE` | base address của program interpreter, thường là dynamic linker[^auxv-base] |
| `AT_ENTRY` | entry address của executable[^auxv-entry] |

**Bước 5: chuyển điều khiển cho interpreter.** Entry mà kernel nhảy tới là `e_entry` của *interpreter* cộng địa chỉ nạp[^binfmt-entry]. Đó không phải entry của chương trình[^dr-entry].

**Bước 6: `ld.so` làm việc.** Nhiệm vụ của nó gồm ba việc: xác định và nạp các dependency, relocate chương trình cùng mọi dependency, rồi khởi tạo chúng theo đúng thứ tự[^dr-steps]. Man page tóm gọn là "tìm và nạp các shared object cần, chuẩn bị chương trình để chạy, rồi chạy nó"[^ldso-desc]. Khi mọi việc xong, điều khiển mới đến chương trình[^dr-dynlinker-tasks], và đích nhảy là `AT_ENTRY` (suy luận của người soạn từ định nghĩa `AT_ENTRY`).

Toàn bộ chuỗi trên, với `orders` là chương trình ở cột bên phải:

```mermaid
sequenceDiagram
  participant P as process (sau fork)
  participant K as kernel (binfmt_elf)
  participant L as ld.so (interpreter)
  participant A as chương trình
  P->>K: execve(path, argv, envp)
  K->>K: đọc program header, tìm PT_INTERP
  K->>K: map PT_LOAD của chương trình và của interpreter
  K->>K: dựng auxv (AT_PHDR, AT_BASE, AT_ENTRY, ...)
  K->>L: nhảy tới entry của interpreter
  L->>L: nạp DT_NEEDED, relocate, chạy init
  L->>A: nhảy tới AT_ENTRY
```

### 3.2 Binary tĩnh: không cần interpreter

Binary tĩnh cổ điển (địa chỉ nạp cố định, không PIE) không có `PT_INTERP`. Tải trọng của nó đã hoàn chỉnh, nên kernel chỉ cần đưa binary vào address space và chuyển điều khiển tới entry của ứng dụng, mọi việc còn lại static linker đã làm xong[^dr-static]. Static-PIE (mục 6) cũng không có `PT_INTERP` nhưng không có địa chỉ nạp cố định, nên câu của Drepper, viết cho trường hợp "fixed load address", không áp dụng nguyên văn. Suy luận của người soạn: nó cần thêm một bước relocate lúc chạy; chương này không đi sâu.

<details><summary>Đi sâu: ca nhiều <code>PT_INTERP</code>, PIE và interpreter nạp ở đâu, chạy <code>ld.so</code> trực tiếp, <code>ldd</code>, vDSO, script <code>#!</code></summary>

- **Interpreter sai định dạng.** Nếu interpreter không phải một định dạng nhận ra được, `execve` trả `ELIBBAD`[^execve-elibbad].
- **Tài liệu và code lệch nhau ở ca nhiều `PT_INTERP`.** `execve(2)` ghi rằng ELF có nhiều hơn một `PT_INTERP` thì lỗi `EINVAL`[^execve-einval]. `elf(5)` cũng nói `PT_INTERP` không được xuất hiện quá một lần[^elf-interp]. Nhưng trong đoạn `v6.8` đã đọc, người soạn không thấy kiểm tra tương ứng (nhận xét của người soạn; đây là chi tiết cài đặt, có thể khác ở bản kernel khác).
- **PIE và loader trong source kernel.** Source kernel phân biệt hai loại `ET_DYN`: *chương trình* (PIE: `ET_DYN` có `INTERP`) và *loader* (`ET_DYN` không có `INTERP`, vì chúng *chính là* interpreter)[^binfmt-pie]. Hai loại này phải nạp ở hai chỗ khác nhau, vì chương trình có thể đụng vào loader, đặc biệt khi `ET_EXEC` không có vị trí ngẫu nhiên[^binfmt-loader-away]. Chương trình PIE được đặt lệch từ `ELF_ET_DYN_BASE`, còn loader vào vùng mmap ngẫu nhiên độc lập[^binfmt-pie-base]. Suy luận của người soạn: static-PIE (`ET_DYN` không có `INTERP`) rơi vào nhóm thứ hai theo phân loại này. Mục 6 quay lại.
- **Chạy interpreter trực tiếp.** Cú pháp là `/lib/ld-linux.so.* [OPTIONS] [PROGRAM [ARGUMENTS]]`. Chạy gián tiếp qua một chương trình động thì không truyền được option cho dynamic linker, và với ELF thì interpreter ghi ở `.interp` được thực thi[^ldso-interp-sec]. Option hữu ích là `--list`, liệt kê mọi dependency và cách chúng được giải quyết[^ldso-list].
- **`ldd`** gọi dynamic linker chuẩn với biến môi trường `LD_TRACE_LOADED_OBJECTS` bằng 1[^ldd-trace]. Cảnh báo an toàn: với chương trình khai báo interpreter khác `ld-linux.so`, một số phiên bản `ldd` có thể thực thi chương trình, nên không bao giờ chạy `ldd` trên executable không tin cậy[^ldd-sec]. Cách an toàn hơn là `objdump -p /path/to/program | grep NEEDED`, nhưng nó chỉ liệt kê dependency trực tiếp[^ldd-objdump].
- **`linux-vdso.so.1`.** Dòng này trong đầu ra `ldd` là dependency đặc biệt; `ldd` ghi chú vdso và ld-linux là đặc biệt[^ldd-special]. Trong auxv, `AT_SYSINFO_EHDR` là địa chỉ page chứa vDSO *ảo* mà kernel tạo ra để cung cấp bản cài đặt nhanh cho một số syscall[^auxv-vdso].
- **Script `#!`.** `execve` cũng chạy được script bắt đầu bằng `#!interpreter [optional-arg]`[^execve-script]. Đó là cơ chế khác `PT_INTERP`: interpreter của script là chương trình đọc file script, còn `PT_INTERP` của ELF là bộ nạp.
- **`AT_SECURE`.** Giá trị khác 0 nghĩa là chương trình cần được xử lý "an toàn", thường do set-user-ID/set-group-ID[^auxv-secure]. `ld.so` dùng nó để chọn secure-execution mode (mục 4).

</details>

**Quay lại Q3:** kernel đọc `PT_INTERP`, map chương trình và `ld.so`, đưa thông tin qua auxv rồi nhảy vào `ld.so`. Với `orders` động, `ld.so` nạp thư viện, relocate và chỉ sau đó mới chuyển điều khiển sang code của service.

## 4. `ld.so` tìm thư viện ở đâu (Q4)

> **Trả lời ngắn.** Nếu tên dependency có chứa dấu `/`, `ld.so` coi đó là đường dẫn và nạp thẳng[^ldso-slash]. Nếu không, theo `ld.so(8)` nó thử lần lượt năm nơi. Đầu tiên là `DT_RPATH` của binary, nhưng chỉ khi binary không có `DT_RUNPATH`[^ldso-order-rpath]. Kế đến là biến môi trường `LD_LIBRARY_PATH`[^ldso-order-ldlp]. Rồi đến `DT_RUNPATH`[^ldso-order-runpath]. Sau đó là cache `/etc/ld.so.cache`[^ldso-order-cache]. Cuối cùng là thư mục mặc định `/lib` rồi `/usr/lib`[^ldso-order-default].

**Giải thích.**

*Với service `orders`.* Giả sử `orders` build trên máy CI, còn image chạy thật dùng một image nền khác, và `libssl` ở đó nằm chỗ khác hoặc không có. Container khởi động rồi thoát ngay, vì `ld.so` không tìm ra thư viện mà `orders` cần. Muốn sửa đúng chỗ, bạn phải biết `ld.so` tìm ở đâu và theo thứ tự nào. Mục này trả lời hai câu hỏi đó.

### 4.1 Binary ghi tên thư viện thế nào

`DT_SONAME` là tên định danh do chính shared object khai báo[^gabi-soname]. Tên trong `DT_NEEDED` của `orders` là bản sao của chuỗi `DT_SONAME`, hoặc của đường dẫn shared object dùng lúc build[^gabi-needed-names]. Vì thế bạn thường thấy `libc.so.6` (một SONAME) chứ không phải đường dẫn tuyệt đối. Tên không có `/` như vậy chính là loại phải đi qua quy trình tìm kiếm dưới đây.

### 4.2 Thứ tự tìm

Với tên không có `/`, `ld.so(8)` tìm theo bảng sau:

| Bước | Nguồn | Ghi chú |
|---|---|---|
| 1 | `DT_RPATH` của binary | chỉ dùng khi **không** có `DT_RUNPATH`; `DT_RPATH` bị đánh dấu deprecated[^ldso-order-rpath] |
| 2 | `LD_LIBRARY_PATH` | bị bỏ qua trong secure-execution mode[^ldso-order-ldlp] |
| 3 | `DT_RUNPATH` | chỉ dùng để tìm dependency trực tiếp (`DT_NEEDED`) của object chứa nó, *không* áp xuống con cháu[^ldso-order-runpath] |
| 4 | `/etc/ld.so.cache` | danh sách đã biên dịch các shared object ứng viên[^ldso-order-cache] |
| 5 | `/lib`, rồi `/usr/lib` | trên một số kiến trúc 64-bit là `/lib64`, `/usr/lib64`; bước này bị bỏ qua nếu binary được link với `-z nodeflib`[^ldso-order-default] |

Hai bước 1 và 3 khác nhau ở phạm vi. `DT_RPATH` áp cho việc tìm mọi object con trong cây dependency[^ldso-order-runpath]. `DT_RUNPATH` của một object thì không ảnh hưởng việc tìm dependency của object khác[^gabi-runpath-scope]. Ví dụ áp dụng bước 3: nếu `orders` có `DT_RUNPATH` trỏ vào thư mục `lib/` của nó, thì `libssl` được tìm ở đó, nhưng một thư viện mà `libssl` kéo theo phải tự có `DT_RUNPATH` riêng.

### 4.3 `$ORIGIN`: đóng gói thư viện cạnh binary

`$ORIGIN` được mở rộng thành thư mục chứa chương trình hoặc shared object[^ldso-tokens]. Việc mở rộng áp dụng trong giá trị các tag `DT_NEEDED`, `DT_RPATH`, `DT_RUNPATH` và vài tag khác của binary ELF[^ldso-tokens-where]. Ví dụ trong man page: build với `-Wl,-rpath,'$ORIGIN/../lib'` thì chương trình tìm được shared object đi kèm trong `somedir/lib`, bất kể `somedir` nằm ở đâu trong cây thư mục[^ldso-origin]. Nhờ vậy ứng dụng kiểu "turn-key" có thể giải nén vào thư mục bất kỳ mà vẫn chạy[^ldso-origin-turnkey]. Nhận xét của người soạn: cách này hợp khi `orders` phải mang theo `.so` riêng trong image thay vì cài vào hệ thống.

### 4.4 Bẫy của `LD_LIBRARY_PATH`

`LD_LIBRARY_PATH` là danh sách thư mục phân tách bằng dấu hai chấm hoặc chấm phẩy, không có cơ chế escape, và một tên thư mục độ dài 0 nghĩa là thư mục hiện tại[^ldso-ldlp]. Đó là chỗ dễ ra chuyện: một dấu hai chấm thừa ở cuối biến có thể kéo thư mục làm việc vào đường tìm thư viện (suy luận của người soạn từ quy tắc trên).

Secure-execution mode được kích hoạt khi `AT_SECURE` trong auxv khác 0[^ldso-secure]; ví dụ điển hình là chương trình set-user-ID, tức file có bit cho process chạy với quyền của chủ file. Ở chế độ này `ld.so` bỏ qua `LD_LIBRARY_PATH`[^ldso-order-ldlp]. `LD_PRELOAD` là biến để nạp thêm shared object *trước* mọi object khác nhằm ghi đè hàm[^ldso-preload]. Ở chế độ này nó cũng hạn chế các đường dẫn có dấu `/`[^ldso-preload-secure]. Nhận xét của người soạn: đừng dựa vào hai biến này cho chương trình đặc quyền. Còn trong container, hãy cài `.so` đúng chỗ hoặc dùng `RUNPATH` có `$ORIGIN`, thay vì để `LD_LIBRARY_PATH` nằm sẵn trong image.

### 4.5 `dlopen` dùng cùng quy tắc

Khi chương trình gọi `dlopen` với tên không có `/`, dynamic linker tìm theo trình tự tương tự: `DT_RPATH` (nếu không có `DT_RUNPATH`) của *đối tượng gọi*, `LD_LIBRARY_PATH`, `DT_RUNPATH`, cache, rồi `/lib`, `/usr/lib`[^dlopen-search].

<details><summary>Đi sâu: thư viện sai kiến trúc, <code>LD_DEBUG</code>, <code>--list</code>, <code>ld.so.cache</code>, hardware capabilities</summary>

- **Thư viện sai kiến trúc.** Khi tìm, gặp một file ELF có thuộc tính không hợp (sai class, sai `e_machine`...) không phải lỗi nghiêm trọng: dynamic linker vét hết các đường dẫn rồi mới kết luận không tìm thấy[^gabi-wrong-attrs].
- **Chẩn đoán.** `LD_DEBUG` bật log chi tiết hoạt động của dynamic linker[^ldso-debug]. Hạng mục `libs` hiển thị các đường dẫn tìm thư viện[^ldso-debug-libs]; `LD_DEBUG=help` liệt kê các hạng mục. `ld.so --list` liệt kê dependency và cách giải quyết[^ldso-list].
- **`ld.so.cache`** do `ldconfig(8)` duy trì. Theo `ld.so(8)`, đó là file chứa danh sách đã biên dịch các thư mục và danh sách có thứ tự các shared object ứng viên[^ldso-cache-file]. Suy luận của người soạn: thêm `.so` vào một thư mục mà chưa cập nhật cache thì bước 4 không thấy nó.
- **Thời điểm đọc `LD_LIBRARY_PATH`.** `ld.so` đọc biến này khi chương trình *khởi động*; `dlopen` dùng giá trị tại thời điểm chương trình bắt đầu chạy[^dlopen-ldlp-start].
- **`DT_RPATH` và `DT_RUNPATH` trong `.dynamic`.** `DT_RPATH` được ghi là deprecated[^elf-dt-rpath]. `DT_RUNPATH` là chuỗi chứa đường dẫn tìm thư viện[^elf-dt-runpath].
- **Hardware capabilities.** `ld.so` còn ưu tiên các thư mục "hardware capabilities" (ví dụ thư viện tối ưu cho tập lệnh mở rộng) nếu có và phù hợp với CPU[^ldso-hwcap].

</details>

**Quay lại Q4:** `ld.so` tìm theo thứ tự RPATH (nếu không có RUNPATH) → `LD_LIBRARY_PATH` → RUNPATH → `ld.so.cache` → `/lib`, `/usr/lib`. `$ORIGIN` cho phép đóng gói thư viện cạnh binary của `orders`, và `LD_LIBRARY_PATH` bị bỏ qua với chương trình đặc quyền.

## 5. PLT, GOT, lazy binding và RELRO (Q5)

> **Trả lời ngắn.** Mã trong shared object không được chứa địa chỉ tuyệt đối của hàm hay biến bên ngoài, vì như thế phải sửa vào text lúc chạy[^dr-got-why]. Thay vào đó, mã đọc địa chỉ từ một bảng dữ liệu gọi là GOT, và gọi hàm ngoài qua một đoạn "bắc cầu" gọi là PLT[^dr-plt-stub]. Với *lazy binding*, lần gọi đầu đi qua dynamic linker để tìm địa chỉ thật rồi ghi vào GOT, các lần sau nhảy thẳng[^dr-plt-resolve]. RELRO chuyển các section chỉ bị relocation sửa sang trang riêng và gỡ quyền ghi sau khi relocate xong[^dr-relro]. Với Full RELRO (kèm bind-now), cả GOT thành chỉ-đọc[^rs-relro-full].

**Giải thích.**

*Với service `orders`.* Mỗi request, `orders` gọi `SSL_read` nhiều lần. Hàm này nằm trong `libssl.so`, một object khác `orders`, và địa chỉ nạp của nó chỉ `ld.so` biết lúc chạy. Vậy lệnh `call` trong code của `orders` nhảy tới đâu? Mục này trả lời bằng GOT và PLT, rồi nói địa chỉ được điền vào lúc nào (lazy binding), và cuối cùng là cách khóa bảng địa chỉ lại để kẻ tấn công không sửa được (RELRO).

### 5.1 Vì sao cần GOT

Drepper dùng một ví dụ C: một hàm dùng `extern int foo;` và gọi `extern int bar(int);`. Hàm này cần hai relocation, một để nạp giá trị `foo` và một để tìm địa chỉ `bar`[^dr-two-relocs]. Giả sử mã mã hóa thẳng địa chỉ vào lệnh, trong khi địa chỉ đó chỉ dynamic linker biết. Khi đó text segment (vùng chứa mã lệnh) phải bị sửa lúc chạy, điều cần tránh[^dr-got-why].

Vì vậy mã sinh cho shared object lấy địa chỉ `foo` từ GOT. (Drepper gọi shared object là DSO, *dynamic shared object*; compiler sinh mã kiểu này với cờ `-fpic`/`-fPIC`, tức position-independent code.) Vị trí của ô GOT so với một thanh ghi PIC (`%ebx` trong ví dụ IA-32 của Drepper) đã biết từ lúc link, nên chỉ GOT bị sửa còn text giữ nguyên[^dr-got-load].

Trên x86-64 ý tưởng giống vậy. psABI mô tả GOT là bảng giữ địa chỉ tuyệt đối trong dữ liệu riêng, nhờ đó địa chỉ có sẵn mà text vẫn position-independent[^psabi-got]. Mã truy cập GOT bằng địa chỉ tương đối `%rip`, ví dụ `jmp *name1@GOTPCREL(%rip)` trong entry PLT[^psabi-plt-entry] (xem `@GOTPCREL` ở [P2·03](../02-cpu/03-assembly-x86-64-va-aarch64.md)).

### 5.2 Vì sao cần PLT

Khi hàm không chắc được định nghĩa trong chính object gọi nó, mỗi lần gọi phải đi qua một đoạn stub trong PLT[^dr-plt-stub]. Với `orders`, `SSL_read` nằm ở object khác nên chắc chắn vào trường hợp này. Nhưng ngay cả khi có định nghĩa cùng file, vẫn chưa đủ để tránh PLT[^dr-plt-always]. Lý do là định nghĩa đó có thể bị object khác chèn thay thế (interposition)[^dr-interposition].

### 5.3 Lazy binding từng bước

Ý của lazy binding là lúc `orders` khởi động, ô GOT của `SSL_read` chưa cần chứa địa chỉ thật. Cơ chế dưới đây là mô hình IA-32 của Drepper; kiến trúc khác chi tiết khác[^dr-plt-arch]. Ở sơ đồ, `SSL_read` đứng ở chỗ Drepper dùng `bar`.

```mermaid
sequenceDiagram
  participant C as mã gọi (orders)
  participant S as PLT entry của SSL_read
  participant G as GOT slot của SSL_read
  participant R as PLT0 + ld.so
  participant F as SSL_read thật
  Note over G: lúc khởi động slot trỏ vào lệnh thứ 2 của PLT entry
  C->>S: call SSL_read@PLT (lần đầu)
  S->>G: jmp *GOT[SSL_read]
  G-->>S: quay lại lệnh pushl trong PLT entry
  S->>R: push offset (IA-32) hoặc index (x86-64) của relocation, nhảy PLT0 rồi vào ld.so
  R->>R: tìm địa chỉ SSL_read, ghi vào GOT slot
  R->>F: nhảy tới SSL_read
  C->>S: call SSL_read@PLT (lần sau)
  S->>G: jmp *GOT[SSL_read]
  G->>F: nhảy thẳng tới SSL_read
```

Đọc từ trên xuống:

1. Mỗi entry PLT ứng với đúng một symbol hàm, và lệnh đầu của nó là một lệnh nhảy gián tiếp qua một slot GOT[^dr-plt-slot].
2. Lúc khởi động, dynamic linker điền slot ấy bằng địa chỉ trỏ vào lệnh thứ hai của chính entry PLT. Vì thế lần gọi đầu, lệnh nhảy rơi xuống lệnh `pushl` kế tiếp, và lệnh này đẩy offset relocation của hàm cần gọi lên stack[^dr-plt-first].
3. Sau đó điều khiển chuyển tới entry PLT đặc biệt đầu tiên (PLT0), đẩy thêm giá trị rồi nhảy vào dynamic linker[^dr-plt0].
4. Khi đã xác định được địa chỉ hàm, dynamic linker ghi nó vào đúng slot GOT mà lệnh `jmp` đầu PLT dùng, rồi nhảy tới hàm. Mọi lần gọi sau không qua dynamic linker nữa mà nhảy thẳng tới hàm[^dr-plt-resolve]. Nhận xét của người soạn: lời gọi vẫn đi qua entry PLT và một lệnh nhảy gián tiếp qua slot GOT.

Trên x86-64 (psABI) cùng khung đó, khác ở chỗ lệnh `pushq` đẩy một relocation *index* chứ không phải *offset* như ví dụ IA-32 của Drepper[^psabi-index]. Các chi tiết còn lại của psABI nằm ở phần Đi sâu.

Lazy binding là *tùy chọn* của dynamic linker: gABI cho phép nó đánh giá entry PLT một cách lười, để tránh chi phí phân giải symbol và relocation cho những hàm không bao giờ được gọi[^gabi-lazy]. Chỉ tham chiếu *hàm* được bind lười, còn tham chiếu biến luôn được bind ngay khi nạp[^dlopen-lazy-var]. Mục `DT_JMPREL` tách riêng các relocation của PLT để dynamic linker bỏ qua chúng lúc khởi tạo khi lazy binding bật[^gabi-jmprel].

### 5.4 Tắt lazy binding (bind now)

Có ba cách tắt. Cách thứ nhất là đặt biến môi trường `LD_BIND_NOW` khác rỗng, khiến dynamic linker phân giải mọi symbol ngay lúc khởi động[^ldso-bindnow]. Cách thứ hai là cờ nằm trong chính object: gABI ghi mục `DT_BIND_NOW` (nay đã được thay bằng cờ `DF_BIND_NOW`) khiến dynamic linker xử lý mọi relocation của object đó trước khi chuyển điều khiển, và mục này ưu tiên hơn chỉ thị lazy từ môi trường hay từ `dlopen`[^gabi-dtbindnow]. Cách thứ ba là truyền `-z now` lúc link, khi đó linker đặt cờ `DF_BIND_NOW` trong entry `DT_FLAGS` của DSO[^dr-bindnow]. Riêng với `dlopen`, cờ `RTLD_NOW` phân giải hết trước khi `dlopen` trả về[^dlopen-now].

### 5.5 RELRO: khóa GOT lại

Nếu kẻ tấn công ghi được một word tùy ý vào address space của `orders`, thì một ô GOT là mục tiêu đáng giá: chỉ cần đổi địa chỉ hàm gọi qua PLT sang chỗ khác[^dr-got-attack]. Chặn ghi GOT mà vẫn để dynamic linker ghi được là việc khó, vì gọi `mprotect` mỗi lần đổi giá trị thì quá đắt[^dr-mprotect].

Giải pháp là `-z relro`. Linker chuyển những section mà chỉ relocation mới sửa sang trang bộ nhớ riêng, và phát ra program header `PT_GNU_RELRO` trỏ tới các trang đó. Lúc chạy, dynamic linker gỡ quyền ghi sau khi xong việc[^dr-relro]. Đây mới là giải pháp một phần. Kết hợp thêm `-z now` thì mọi relocation đều đủ điều kiện được bảo vệ, đổi lại chi phí khởi động tăng[^dr-now].

rustc gọi các mức bảo vệ này là `relro-level`:

| Mức | Hành vi theo rustc book |
|---|---|
| `off` | hàm động được phân giải lười và GOT ghi được[^rs-relro-off] |
| `partial` | hàm động phân giải lười, ghi vào phần GOT của PLT (`.got.plt`); phần GOT còn lại (`.got`) thành chỉ-đọc[^rs-relro-partial] |
| `full` | hàm động được phân giải ngay lúc bắt đầu chạy, GOT được điền sớm rồi chuyển thành chỉ-đọc; tốn thêm bộ nhớ và làm tăng thời gian khởi động process[^rs-relro-full] |

rustc bật Full RELRO mặc định trên nền tảng hỗ trợ[^rs-relro-default]. Trong source target spec của Rust 1.95.0, các nền tảng Linux đặt `relro_level: RelroLevel::Full`[^rs-base-linux]. Về chi phí, Drepper nhận xét rằng với ứng dụng chạy lâu và nhạy cảm bảo mật, chi phí khởi động không nặng bằng lợi ích an ninh[^dr-now-longrun]. Nhận xét của người soạn: với một service backend như `orders`, chạy hàng ngày, Full RELRO gần như không đáng kể. Với công cụ CLI khởi động hàng nghìn lần mỗi giờ, hay chương trình cần cold start cực ngắn, nên cân nhắc đo.

<details><summary>Đi sâu: x86-64 vs IA-32, quan sát bind, <code>STV_HIDDEN</code>, <code>RTLD_*</code></summary>

- **Cấu trúc GOT/PLT là chi tiết riêng của từng kiến trúc.** Ví dụ ở IA-32 PLT chỉ-đọc, còn một số kiến trúc khác (Drepper không nêu tên) thì PLT phải ghi được, vì entry PLT bị sửa trực tiếp thay vì nhảy gián tiếp qua GOT[^dr-plt-arch]. Entry PLT x86-64 thì nhảy gián tiếp qua GOT như đã thấy ở mục 5.1[^psabi-plt-entry]. Vì vậy mô hình IA-32 chỉ là *ví dụ minh họa* cho cơ chế.
- **Chi tiết psABI x86-64.** Entry PLT gồm `jmp *name@GOTPCREL(%rip)`, `pushq $index`, `jmp .PLT0`[^psabi-plt-entry]. Lúc tạo memory image, dynamic linker đặt giá trị đặc biệt vào ô GOT thứ hai và thứ ba[^psabi-got-special]. Lúc đầu, slot của hàm chứa địa chỉ lệnh `pushq` kế tiếp[^psabi-got-init]. `.PLT0` đẩy giá trị ở `GOT+8` lên stack rồi nhảy tới địa chỉ ở `GOT+16`, tức vào dynamic linker[^psabi-plt0]. Sau khi bind, `jmp` đầu entry nhảy thẳng tới hàm thay vì rơi xuống `pushq`[^psabi-subseq].
- **Quan sát bind.** `LD_BIND_NOT` (nếu đặt) khiến dynamic linker **không** cập nhật GOT/PLT sau khi phân giải một hàm. Kết hợp biến này với `LD_DEBUG` ở các hạng mục `bindings` và `symbols`, bạn có thể xem mọi lần bind lúc chạy[^ldso-bindnot].
- **`STV_HIDDEN`.** Symbol loại này không bị chèn thay thế nên tham chiếu trong module luôn trỏ vào bản cục bộ[^elf-vis-hidden]. Suy luận của người soạn: đó là lý do thư viện nên ẩn symbol không xuất khẩu, để lời gọi nội bộ khỏi qua PLT. Drepper hứa giải thích "khi nào và cách tránh PLT" ở phần sau của tài liệu[^dr-avoid-plt].
- **`dlopen` và lazy.** `RTLD_LAZY` nghĩa là "chỉ phân giải symbol khi mã tham chiếu được thực thi; symbol không bao giờ được tham chiếu thì không bao giờ phân giải"[^dlopen-lazy]. Nếu `RTLD_NOW` hoặc `LD_BIND_NOW` được đặt thì phải phân giải hết symbol chưa định nghĩa trước khi `dlopen` trả về[^dlopen-now]. `LD_BIND_NOW` thắng `RTLD_LAZY`[^dlopen-lazy-var].
- **musl** xử lý lazy binding khác glibc, xem bảng so sánh ở mục 7.

</details>

**Quay lại Q5:** GOT là bảng địa chỉ được ghi lúc chạy, PLT là đoạn stub dùng GOT, và lazy binding bind hàm ở lần gọi đầu. Trong `orders`, RELRO khóa phần GOT chỉ bị relocation sửa sau khi relocate xong (Full RELRO kèm bind-now: cả GOT chỉ-đọc).

## 6. PIE và ASLR (Q6)

> **Trả lời ngắn.** *PIE* (Position Independent Executable) là executable được build để chạy đúng ở địa chỉ nạp bất kỳ, nên kernel có thể đặt nó lệch ngẫu nhiên. *ASLR* là cơ chế của kernel nhằm ngẫu nhiên hóa vị trí các vùng của process, điều khiển bằng `kernel.randomize_va_space`. Ở mức 1, nó ngẫu nhiên hóa mmap base, stack và trang vDSO, và với binary PIE thì cả điểm bắt đầu của code[^aslr-1]. Binary không PIE (`ET_EXEC`) nằm ở địa chỉ cố định ghi trong file[^gabi-exec-abs].

**Giải thích.**

*Với service `orders`.* Giả sử `orders` crash trong production và log ghi một địa chỉ trong code. Chạy lại lần sau, cùng lỗi ấy lại cho một địa chỉ khác. Nguyên nhân là `orders` được build thành PIE, nên kernel đặt nó lệch một khoảng khác nhau mỗi lần chạy. Mục này nói PIE khác executable truyền thống ở đâu, kernel đặt địa chỉ thế nào, và PIE tốn thêm gì.

### 6.1 Mã tuyệt đối và mã position-independent

Executable truyền thống thường chứa mã tuyệt đối: các segment phải nằm đúng ở địa chỉ ảo đã dùng khi build[^gabi-exec-abs]. Segment của shared object thì thường chứa mã position-independent, nên địa chỉ ảo của nó có thể đổi từ process này sang process khác mà việc thực thi vẫn đúng[^gabi-base]. Chênh lệch giữa địa chỉ ảo thật trong bộ nhớ và địa chỉ ảo ghi trong file là một hằng số cho mỗi object trong một process. Hằng số đó là *base address*[^gabi-baseaddr].

Drepper tóm lại rằng DSO "gần như chỉ là một loại binary đặc biệt", khác ở chỗ không có địa chỉ nạp cố định nên cần dynamic linker mới chạy được[^dr-dso]. Với PIE, sự khác biệt đó còn thu hẹp hơn[^dr-pie].

### 6.2 Kernel đặt PIE ở đâu

Trong source kernel `v6.8`, PIE là `ET_DYN` có `INTERP`[^binfmt-pie] (ở đây gọi *PIE động* để phân biệt với static-PIE). Với loại này, kernel đặt `load_bias = ELF_ET_DYN_BASE`, rồi khi cờ `PF_RANDOMIZE` bật thì cộng thêm `arch_mmap_rnd()`[^binfmt-load-bias]. Executable cố định (`ET_EXEC`) thì khác: `load_bias` khởi tạo bằng 0[^binfmt-load-bias-init]. Mỗi `PT_LOAD` của nó được map tại `load_bias + vaddr`, tức đúng tại `p_vaddr` ghi trong file[^binfmt-elf-load]. Hai đoạn code này là chi tiết cài đặt của `v6.8`, không phải bảo đảm của `execve(2)`. Chúng khớp với gABI: executable chứa mã tuyệt đối nên segment phải nằm ở địa chỉ đã dùng khi build[^gabi-exec-abs]. Phần Đi sâu có thêm các chi tiết còn lại của đoạn code đó.

### 6.3 ASLR là một sysctl

`randomize_va_space` có ba mức 0/1/2. Ý nghĩa từng mức, kèm trích tài liệu kernel và cách quan sát qua `maps`, nằm ở [P3·05 mục 3](../03-memory/05-bo-nho-tien-trinh-linux.md). Phần riêng của chương này là phía kernel khi nạp ELF. Ở mức 1 trở lên, với binary PIE cả điểm bắt đầu của code cũng được ngẫu nhiên hóa[^aslr-1]. Trong `load_elf_binary`, process được đặt cờ `PF_RANDOMIZE` trừ khi personality có `ADDR_NO_RANDOMIZE` hoặc sysctl bằng 0[^binfmt-pfrandomize].

### 6.4 Chi phí của PIE: relative relocation

Mọi con trỏ tuyệt đối nằm trong *dữ liệu* và trỏ tới đích trong chính file (ví dụ mảng con trỏ hàm, hay vtable) cần được cộng địa chỉ nạp lúc chạy. Đó là relative relocation: dynamic linker chỉ cộng địa chỉ nạp vào offset đã tính sẵn rồi ghi vào chỗ chỉ định[^dr-relative]. Con trỏ tới symbol ở object khác thì cần relocation theo symbol, xem phần Đi sâu của mục 2. Drepper lưu ý rằng executable không PIE không có relative relocation, vì địa chỉ nạp đã biết lúc link[^dr-no-relative]. Suy luận của người soạn từ hai câu đó: PIE phải có chúng.

<details><summary>Đi sâu: chi tiết code kernel, PIE trong Rust, static-PIE, quan sát backtrace</summary>

- **Thêm về `v6.8`.** Chương trình PIE được nạp lệch từ `ELF_ET_DYN_BASE`, còn loader vào vùng mmap ngẫu nhiên độc lập[^binfmt-pie-base]. Với `ET_EXEC`, dòng chú thích trong code nói không cần xử lý đặc biệt cho `PT_LOAD` đầu tiên[^binfmt-etexec]. Segment đầu đó được map với cờ `MAP_FIXED_NOREPLACE`[^binfmt-etexec-fixed]. Suy luận của người soạn: static-PIE (`ET_DYN` không `INTERP`) rơi vào nhóm "loader" của phân loại ở trên, tức vào vùng mmap ngẫu nhiên chứ không lệch từ `ELF_ET_DYN_BASE`.
- **Stack.** Khi dựng stack, `load_elf_binary` truyền `randomize_stack_top(STACK_TOP)`[^binfmt-stack-rnd]. Thân hàm đó nằm ngoài đoạn source đã đọc nên chương này không khẳng định nó phụ thuộc điều kiện nào.
- **`relocation-model` của rustc.** Cờ `-C relocation-model` điều khiển việc sinh position-independent code[^rs-reloc-intro]. Giá trị `pic` là relocatable hoàn toàn, lệnh máy phải dùng chế độ địa chỉ tương đối[^rs-reloc-pic]. Nó là mặc định cho đa số target[^rs-reloc-default]. Giá trị `pie` là relocatable nhưng không hỗ trợ interposition (thay symbol theo tên bằng `LD_PRELOAD`...)[^rs-reloc-pie]. Giá trị `static` là non-relocatable, lệnh có thể dùng địa chỉ tuyệt đối[^rs-reloc-static].
- **Khi nào ra PIE.** Nếu relocation model là `pic` và target hỗ trợ PIE, linker được yêu cầu (`-pie`) sinh executable PIE[^rs-reloc-link]. Target Linux của Rust đặt `position_independent_executables: true`[^rs-base-linux].
- **`crt-static` và static-PIE.** Nếu target không hỗ trợ đồng thời position-independent và statically linked executable, thì `-C target-feature=+crt-static` "thắng" `-C relocation-model=pic`, và linker sinh binary tĩnh không PIE[^rs-reloc-crt]. Target `x86_64-unknown-linux-musl` đặt `static_position_independent_executables = true`[^rs-x64-musl]. Định nghĩa trong spec nói executable vừa tĩnh vừa position-independent *được hỗ trợ*[^rs-spec-static-pie]. Hai nguồn này chỉ cho thấy target spec khai báo khả năng đó, không mô tả cơ chế tạo static-PIE. Chương này không khẳng định binary musl cụ thể của bạn có phải static-PIE hay không; hãy tự kiểm bằng `readelf -h` (`e_type`) và `readelf -lW` (có hay không `PT_INTERP`), là lệnh minh họa, chưa chạy.
- **Backtrace của service PIE.** Suy luận của người soạn (backend): địa chỉ trong crash log/backtrace của binary PIE đổi mỗi lần chạy, nên khi symbolize cần địa chỉ nạp (đọc từ `/proc/<pid>/maps`, hoặc `AT_BASE`/`AT_PHDR` trong auxv) để quy về offset trong file. Xem [P3·05 mục 3](../03-memory/05-bo-nho-tien-trinh-linux.md).
- **Tắt ASLR theo từng process.** Dùng personality `ADDR_NO_RANDOMIZE`: source kernel kiểm tra nó khi đặt `PF_RANDOMIZE`[^binfmt-pfrandomize].
- **`LD_PREFER_MAP_32BIT_EXEC`** (chỉ x86-64) *giảm* không gian ngẫu nhiên hóa, vì `MAP_32BIT` chỉ ánh xạ 2 GB thấp; nên biến này luôn bị tắt ở secure-execution mode[^ldso-map32].

</details>

**Quay lại Q6:** PIE động là executable `ET_DYN` có interpreter, kernel đặt lệch ngẫu nhiên khi ASLR bật, nên địa chỉ trong crash log của `orders` đổi mỗi lần chạy. ASLR (`randomize_va_space`) ngẫu nhiên hóa mmap base/stack/vDSO/PIE và, ở mức 2, cả heap[^aslr-2].

## 7. Static vs dynamic, glibc vs musl, symbol versioning (Q7)

> **Trả lời ngắn.** Binary *tĩnh* chép thư viện vào file nên tự đủ; binary *động* chỉ ghi tên thư viện (`DT_NEEDED`) và nhờ `ld.so` nạp lúc chạy. Với glibc, symbol versioning khiến binary build trên distro mới có thể từ chối chạy trên distro cũ hơn khi thiếu version cần (suy luận của người soạn từ cơ chế mô tả bên dưới). Còn musl kết hợp liên kết tĩnh cho một binary không phụ thuộc gì bên ngoài[^musl-no-deps], đổi lại có vài khác biệt hành vi (DNS, dynamic loader, stack thread). Trong container, binary tĩnh không cần `ld.so` hay `libc.so` trong image, nên có thể chạy trên image `scratch` chỉ chứa binary (suy luận của người soạn; số đo ở P8·08).

**Giải thích.**

*Với service `orders`.* Giả sử CI build `orders` trên một image mới, trong khi cluster chạy nó trên image nền cũ hơn. `orders` là binary động, nên nó hứa với môi trường chạy rằng sẽ có những thư viện, và cả những *phiên bản* của chúng, đúng như lúc build. Image cũ có thể không giữ được lời hứa đó. Cách khác là liên kết tĩnh với musl, để `orders` tự mang đủ mọi thứ, nhưng khi đó hành vi của libc đổi theo. Mục này cho thấy cả hai phía của lựa chọn.

### 7.1 Hai chế độ: tĩnh và động

Theo `ld.so(8)`, binary Linux cần liên kết động trừ khi truyền `-static` cho `ld(1)` lúc build[^ldso-static]. Binary tĩnh cổ điển (không PIE) được static linker hoàn thiện sẵn, kernel chỉ việc nạp rồi nhảy vào entry[^dr-static]. Binary động thì khác: khi còn nằm trên đĩa nó chưa "hoàn chỉnh", nên kernel không chuyển điều khiển ngay cho ứng dụng mà cần dynamic linker bổ sung phần còn lại[^dr-dynamic-incomplete]. Mỗi dependency được ghi bằng một entry `DT_NEEDED` trong `.dynamic`[^elf-needed]; mục 3–4 đã cho thấy `ld.so` dùng các entry đó ra sao.

### 7.2 libc làm gì

libc là thư viện cài đặt chuẩn ISO C/POSIX cùng các extension thông dụng; ví dụ musl được xây trên API syscall của Linux[^musl-libc-def]. Nó cung cấp lớp C cho các giao diện OS, stdio có đệm, quản lý bộ nhớ, tạo thread và đồng bộ, nạp shared library...[^musl-libc-does]. Vì vậy chọn libc cũng là chọn nền tảng thực thi của binary (nhận xét của người soạn).

### 7.3 Symbol versioning

Bài toán là thư viện cần đổi API/ABI mà không muốn đổi tên file. Drepper mô tả ba lớp giải pháp, từ thô tới tinh:

1. *Đổi tên file* (đổi `DT_SONAME`) là cách thô nhất: mỗi lần có thay đổi không tương thích lại phải đổi tên[^dr-ver-filename]. Cách này dựa vào chỗ `DT_NEEDED` ghi tên DSO, thường lấy từ `DT_SONAME`[^dr-soname].
2. *Versioning nội bộ kiểu Sun*: mỗi symbol gắn một version. Khi link, linker ghi lại phiên bản symbol đã dùng và lưu danh sách version cần vào binary[^dr-ver-record]. Lúc khởi động, dynamic linker kiểm tra mọi interface cần có đủ version. **Nếu không khớp thì DSO ở runtime không tương thích và chương trình không khởi động**[^dr-ver-fail].
3. *Cơ chế của Linux (và GNU Hurd)* mở rộng cách (2). Nó cho phép **nhiều định nghĩa của cùng một symbol** (version phải khác nhau), và binary ghi lại *từng symbol* đã dùng version nào từ DSO nào[^dr-linux-ver-two]. Nhờ đó ABI *thường* có thể giữ ổn định lâu tùy ý[^dr-linux-ver].

Trong file ELF, ba section mang thông tin này. `.gnu.version_d` chứa định nghĩa version[^elf-verdef]. `.gnu.version_r` chứa version cần[^elf-verneed]. `.gnu.version` là bảng version symbol[^elf-versym].

### 7.4 Hệ quả với glibc

Dynamic linker của glibc kiểm tra từng version mà object cần (đọc `elf/dl-version.c` tag `glibc-2.36`, bản của Debian bookworm; đây là chi tiết cài đặt). Nếu không tìm thấy mà tham chiếu là weak thì không gây lỗi nghiêm trọng[^glibc-ver-weak]. Các trường hợp còn lại báo lỗi dạng `version ... not found (required by ...)`[^glibc-ver-msg].

Suy luận của người soạn: binary build trên distro có glibc mới có thể ghi nhận một version symbol mà glibc cũ không định nghĩa, vì Drepper nói linker ghi lại version của symbol đã dùng[^dr-ver-record]. Khi đó nó dừng theo cơ chế trên. Vì vậy, muốn `orders` chạy rộng, hãy build trên môi trường glibc *cũ nhất* bạn cần hỗ trợ (nhận xét của người soạn). Bảng nền tảng của rustc ghi `x86_64-unknown-linux-gnu` kèm "kernel 3.2+, glibc 2.17+"[^rs-glibc217]. Binary bạn tự build còn phụ thuộc thêm glibc của máy build.

### 7.5 musl và binary tĩnh

musl cho phép triển khai ứng dụng thành *một* file duy nhất, chạy trên máy có kiến trúc và Linux kernel (hoặc lớp tương thích syscall ABI) phù hợp[^musl-single]. Binary liên kết tĩnh với musl không có dependency ngoài, kể cả với những tính năng như DNS lookup hay chuyển charset mà glibc cài đặt bằng dynamic loading[^musl-no-deps]. Với `orders`, nghĩa là image không cần chứa `ld.so` hay `libc.so`.

### 7.6 Hai khác biệt chạm thẳng vào vận hành backend

Đổi từ glibc sang musl không chỉ đổi cách liên kết. Nhận xét của người soạn, dựa trên các trích dẫn bên dưới: có hai khác biệt mà một service Rust trong container dễ chạm vào.

**Resolver.** Khi `/etc/resolv.conf` có nhiều nameserver, glibc thử lần lượt từng cái[^musl-dns-seq]. Resolver của musl thì hỏi tất cả song song và nhận phản hồi đến trước[^musl-dns-parallel]. Với tên miền, `search` dài hơn 256 ký tự bị musl bỏ qua[^musl-dns-256]. Tên có số dấu chấm ≥ `ndots` thì musl chỉ thử ở global namespace, không quay lại `search` như glibc[^musl-dns-ndots]. Giờ nhìn vào Pod của `orders`: Kubernetes cấu hình `/etc/resolv.conf` của Pod để mở rộng truy vấn bằng `search`[^k8s-resolv]. Ví dụ trong docs có `options ndots:5`[^k8s-ndots]. Vì vậy DNS nội bộ Kubernetes có thể cư xử khác giữa hai libc (suy luận của người soạn).

**Thread stack.** Với thread tạo bằng pthread mà không đặt kích thước stack, glibc lấy theo `RLIMIT_STACK` của main thread, thường 2–10 MB[^glibc-stack]. musl thì chỉ cấp 128k[^musl-stack]. Rust `std::thread` mặc định 2 MiB trên các nền tảng Tier-1[^rs-thread-stack]. Riêng với musl, source std 1.95.0 (đã đọc ở [P3·07 mục 9](../03-memory/07-stack-alignment-padding.md), Đi sâu) đặt `DEFAULT_MIN_STACK_SIZE = 2 * 1024 * 1024` cho các target Unix[^std-default-stack]. Khi tạo thread, std gọi `pthread_attr_setstacksize` với `max(stack, min_stack_size)`[^std-setstacksize]. Vì thế mặc định 128k của musl không áp lên `std::thread`, và chỉ thread do thư viện C tự tạo mới nhận 128k (suy luận của người soạn từ source; chưa đo trên musl).

<details><summary>Đi sâu: static trong container, <code>crt-static</code>, allocator, <code>dlclose</code>, các khác biệt glibc/musl còn lại</summary>

- **Image `scratch`.** Image chỉ có binary cần binary tĩnh. Số đo kích thước image/binary ở [P8·08 mục 5](../08-containers/08-rust-trong-container.md), còn arena của glibc ở [P8·08 mục 2](../08-containers/08-rust-trong-container.md) (malloc của musl chưa được đo ở đó). Chương này giải thích vì sao tĩnh được: không có `PT_INTERP` (mục 3) nên không cần `ld.so` và không cần `libc.so` trong image.
- **Mặc định tĩnh của musl.** Tài liệu Rust cho biết một số target có chế độ C runtime tĩnh *mặc định*, trong đó có `x86_64-unknown-linux-musl`; phần lớn target khác link động mặc định[^rs-crt-default]. Trong source target spec, đặc tả x86_64 musl có chú thích FIXME của team compiler "target musl nên link động theo mặc định", cùng với `crt_static_default = true`[^rs-x64-musl-crt]. Đặc tả aarch64 musl (target của máy lab) có đúng chú thích và giá trị đó[^rs-a64-musl-crt]. Nghĩa là musl tĩnh là mặc định *hiện tại*, không phải khẳng định thiết kế lâu dài (suy luận của người soạn).
- **Cờ `crt-static`** thuộc loại feature mà rustc đọc qua `-C target-feature=+crt-static` hoặc `-crt-static`[^rs-crt-flag]. Target không hỗ trợ chuyển đổi sẽ bỏ qua cờ này, và Rust khuyên kiểm tra lại binary sau khi build[^rs-crt-ineffective].
- **Allocator.** Docs Rust không chỉ định allocator toàn cục mặc định. Source std 1.95.0 hiện dùng `System`, tức `malloc` của libc, khi chương trình không đăng ký `#[global_allocator]`; đây là chi tiết cài đặt có thể đổi, không phải lời hứa (xem [P3·06 mục 10](../03-memory/06-heap-allocator.md) và [P8·08 mục 2](../08-containers/08-rust-trong-container.md)). Vì vậy *hiện nay* allocator mặc định đi theo libc; chi tiết glibc (arena) ở [P3·06 mục 7](../03-memory/06-heap-allocator.md). Chương này không có nguồn riêng về malloc của musl (P8·08 cũng nói không đo hành vi của nó), nên không khẳng định gì về malloc của musl hay chiều nhanh/chậm.
- **Nhánh stack riêng.** Theo source std 1.95.0 đã đọc, với `DEFAULT_MIN_STACK_SIZE` ở trên, nhánh riêng trong source std chỉ có cho l4re, vxworks, espidf và nuttx; các target Unix còn lại đi chung một giá trị.
- **Lazy binding.** glibc có, trì hoãn việc phân giải đến lần gọi hàm đầu tiên[^musl-glibc-lazy]. musl không hỗ trợ, vì lý do tin cậy (báo lỗi của lazy binding là bất khả thi) và vì giảm đáng kể mã dễ lỗi phụ thuộc kiến trúc[^musl-lazy].
- **`dlclose`.** glibc đếm tham chiếu thư viện và gỡ khi đếm về 0[^musl-dlclose-glibc]. musl nạp thư viện vĩnh viễn tới khi process thoát hoặc `exec`, và `dlclose` là no-op[^musl-dlclose]. Với constructor và destructor: glibc chạy destructor khi `dlclose` (nếu không còn tham chiếu) và lần mở lại sẽ chạy lại constructor; musl chỉ chạy constructor lần đầu và destructor khi thoát[^musl-dlclose-ctor]. Nhận xét của người soạn: plugin nạp bằng `dlopen` (`libloading`...) nên cân nhắc điều này, vì với musl hành vi quan sát được khi hot-reload plugin khác đi.
- **Symbol versioning.** glibc hỗ trợ versioned symbol qua version table trong các section ELF riêng của GNU. musl chỉ hỗ trợ tập con cho phép chọn *version mặc định*[^musl-ver].
- **DNS qua TCP.** resolver của musl chưa hỗ trợ DNS qua TCP trước phiên bản 1.2.4[^musl-dns-tcp].
- **Kích thước.** musl nhắm tới độ nhỏ: binary tĩnh tối thiểu có thể dưới 10 kB mã, ngay cả khi có thread, và chương trình hữu ích có thể dưới 50 kB[^musl-small].
- **Thứ tự và số lần nạp.** Shared object được nối vào process đúng một lần dù xuất hiện nhiều lần trong danh sách dependency[^gabi-once]. Dynamic linker tìm symbol theo thứ tự breadth-first, từ executable rồi tới các `DT_NEEDED` theo thứ tự[^gabi-bfs].
- **Danh sách đầy đủ** các khác biệt glibc/musl nằm ở trang "Functional differences from glibc" của musl, xem phần Đọc thêm.

</details>

**Quay lại Q7:** tĩnh là thư viện chép vào file, không cần `ld.so`; động là `DT_NEEDED` cộng `ld.so`. Symbol versioning khiến binary động của `orders` có thể từ chối khởi động nếu image chạy thiếu version cần. musl kết hợp liên kết tĩnh cho binary tự đủ, nhưng DNS, dynamic loader và stack thread khác glibc.

## 8. Rust: crate type, target `*-musl`, `relocation-model`, `strip` (Q8)

> **Trả lời ngắn.** rustc biên dịch crate thành object rồi gọi linker hệ thống, thường qua C compiler, để ra file ELF[^rs-linker-cc]. *Crate type* quyết định loại artifact: `bin` ra executable, `rlib` ra file trung gian rustc đọc lại, `staticlib` ra `.a` cho C, `cdylib` ra `.so` cho ngôn ngữ khác, `dylib` ra `.so` Rust, `proc-macro` ra macro cho compiler. Target `x86_64-unknown-linux-musl` link libc tĩnh mặc định, còn target `*-gnu` link động mặc định. Ba cờ `-C relocation-model`, `-C relro-level` và `-C strip` điều khiển PIE, RELRO và việc gỡ symbol.

**Giải thích.**

*Với service `orders`.* Lệnh `cargo build --release` biến mã nguồn `orders` thành file ELF của mục 1. Những cờ nào của Cargo và rustc quyết định file đó là PIE hay không, tĩnh hay động, còn symbol hay đã gỡ? Mục này nối các mục trước vào ngôn ngữ của Rust.

### 8.1 Các crate type

Một lần biên dịch có thể sinh nhiều artifact, bằng cờ dòng lệnh hoặc thuộc tính `crate_type`. Nếu có cờ dòng lệnh thì mọi thuộc tính `crate_type` bị bỏ qua[^rs-link-type]. Cargo đặt tương ứng qua trường `crate-type` của target, và trường này chỉ chỉ định được cho library và example[^cargo-crate-type]. `orders` là một `bin`; các loại còn lại xuất hiện khi service đưa code ra cho C hoặc ngôn ngữ khác.

| Crate type | Sinh ra | Ghi chú (theo Reference) |
|---|---|---|
| `bin` | executable chạy được | cần hàm `main`; liên kết mọi dependency Rust và native, ra một binary phân phối được; là mặc định[^rs-link-bin] |
| `lib` | thư viện Rust "kiểu compiler khuyến nghị" | khái niệm mơ hồ; rustc luôn dùng lại được, nhưng loại thực tế có thể đổi theo thời gian[^rs-link-lib] |
| `rlib` | file "Rust library" | artifact trung gian, có thể coi là "static Rust library"; rustc đọc lại ở lần link sau, tìm metadata trong rlib như trong dynamic library[^rs-link-rlib] |
| `dylib` | `.so` (Linux), `.dylib`, `.dll` | *ép* sinh dynamic library; dùng làm dependency cho library/executable khác[^rs-link-dylib] |
| `cdylib` | `.so`... để nạp từ ngôn ngữ khác | thư viện hệ thống động cho ngôn ngữ khác nạp[^rs-link-cdylib] |
| `staticlib` | `.a` trên Linux | chứa mã crate cục bộ + mọi upstream; rustc không bao giờ link tới `staticlib`[^rs-link-staticlib] |
| `proc-macro` | macro cho compiler | chỉ được export procedural macro; luôn biên dịch cho cùng target với compiler[^rs-link-procmacro] |

### 8.2 Dependency của `orders` được link tĩnh hay động

Khi tạo executable mà không có `-C prefer-dynamic`, dependency được tìm trước ở dạng rlib[^rs-dep-prefer]. Rustc book xác nhận điều này: mặc định rustc ưu tiên link tĩnh các dependency[^rs-prefer-dynamic]. Hệ quả là một binary Rust thường chứa luôn code của `std` và các crate khác (nhận xét của người soạn). Với `staticlib`, mọi upstream phải có dạng rlib, vì dynamic library không chuyển thành tĩnh được[^rs-dep-staticlib]. Mục tiêu của compiler là một thư viện không bao giờ xuất hiện hơn một lần trong artifact[^rs-single-copy].

### 8.3 Libc: `gnu` hay `musl`

Std cố gắng hỗ trợ cả C runtime liên kết tĩnh lẫn động tùy target[^rs-crt-intro]. Mỗi target có một chế độ mặc định, thường là động, với vài ngoại lệ tĩnh như `x86_64-unknown-linux-musl`[^rs-crt-default]. Bạn chọn bằng `-C target-feature=+crt-static` hoặc `-crt-static`[^rs-crt-flag]. Các target Linux Tier 1/2 đi kèm libc cụ thể. `x86_64-unknown-linux-gnu` cần kernel 3.2+ và glibc 2.17+[^rs-glibc217]. `x86_64-unknown-linux-musl` thì đi với musl 1.2.5[^rs-musl-125]. Chi tiết đo kích thước image nằm ở [P8·08 mục 5](../08-containers/08-rust-trong-container.md).

### 8.4 PIE, RELRO, rpath

PIE và RELRO đã nói ở mục 5–6. `relocation-model=pic` là mặc định của đa số target[^rs-reloc-default]. Target Linux dựa trên `base/linux.rs` đặt `position_independent_executables: true` và `relro_level: Full`[^rs-base-linux]. Mỗi target có default riêng[^rs-relro-default]. Cờ `-C rpath` mặc định tắt[^rs-rpath-off]. Nếu bật, rustc thêm rpath tương đối vị trí đầu ra (`$ORIGIN` trên ELF) cho mọi dependency dạng dylib[^rs-rpath-on]. Nhận xét của người soạn: `-C rpath` ảnh hưởng `DT_RUNPATH`/`DT_RPATH` ở mục 4.

### 8.5 `strip` và kích thước

`-C strip` điều khiển việc gỡ debuginfo và dữ liệu phụ trợ tương tự khỏi binary lúc link[^rs-strip-intro]. Có ba giá trị. `none` không đụng gì. `debuginfo` gỡ section debuginfo và symbol debuginfo khỏi bảng symbol, nên backtrace gần như còn nguyên nhưng debugger như gdb/lldb kém tác dụng. `symbols` làm như `debuginfo` và gỡ cả phần còn lại của bảng symbol, tùy nền tảng[^rs-strip-values]. rustc cảnh báo `-C strip=symbols` có thể làm trace khó đọc ở nơi dựa vào bảng symbol, và chương trình cần crash-reporting thường nên tránh nó[^rs-strip-warn]. Cũng không thể dựa vào strip như một biện pháp bảo mật hay làm rối có ý nghĩa, vì disassembler và decompiler vẫn trích được nhiều thông tin dù không có symbol[^rs-strip-security]. Trong Cargo, `strip` ở profile điều khiển cờ `-C strip` của rustc[^cargo-strip]. Kết quả đo giảm kích thước (4,8 MB → 595 kB) xem [P8·08 mục 6](../08-containers/08-rust-trong-container.md).

Vì sao strip không làm `orders` chạy hỏng? Suy luận của người soạn dựa trên mục 1–2: "strip symbols" nhắm tới bảng symbol và debug (`.symtab`, `.debug*`), là các section không cần để chạy vì không có `SHF_ALLOC`[^elf-shf-alloc]. Symbol động nằm trong `.dynsym` và cần cho liên kết động[^elf-dynsym], nên binary vẫn chạy.

### 8.6 Symbol và `#[no_mangle]`

`no_mangle` tắt mangling tên chuẩn, nên symbol là đúng tên identifier[^rs-no-mangle]. Item đó cũng được export công khai khỏi library hay object file[^rs-no-mangle-export]. Thuộc tính này `unsafe`, vì một symbol không mangle có thể đụng tên khác, hoặc đụng một tên nổi tiếng và gây UB[^rs-no-mangle-unsafe]. Đây là cách xuất hàm C ABI từ `cdylib`/`staticlib`. Định dạng mangling mặc định "do compiler chọn, có thể đổi trong tương lai"[^rs-mangling-default], nên đừng dựa vào dạng tên `_ZN...`.

### 8.7 Trộn Rust với C/C++

Reference đưa hai hướng. Hướng thứ nhất là dùng rustc để link, truyền thư viện non-Rust bằng `-L`, `-l` và/hoặc `#[link]`. Hướng thứ hai là dùng linker bên ngoài với một `staticlib` Rust[^rs-foreign-linkers]. Với hướng thứ hai, mỗi binary chỉ nên có một `staticlib`, vì nhiều staticlib Rust có khả năng đụng nhau, và việc truyền rlib thẳng cho linker ngoài *chưa được hỗ trợ*[^rs-foreign-conflict]. Staticlib chứa mã của mọi dependency (kể cả `std`) và xuất mọi public symbol của chúng, nên khi link vào executable hay shared library có thể cần xử lý đặc biệt[^rs-staticlib-care]. Với shared library, danh sách symbol xuất phải được giới hạn (ví dụ bằng linker hoặc symbol version script), và có thể bỏ phần không dùng bằng `--gc-sections`[^rs-staticlib-limit]. Dependency động của staticlib phải chỉ định tay, và cờ `--print=native-static-libs` có thể giúp[^rs-staticlib-native]. Thuộc tính `#[link(name = "...", kind = "static")]` chọn thư viện native tĩnh[^rs-link-attr-static]. Kind mặc định là `dylib`, tức thư viện động[^rs-link-attr-dylib].

<details><summary>Đi sâu: <code>Cargo.toml</code> mẫu, unwinding qua ranh giới, <code>dlopen</code>, quy ước đặt cờ</summary>

Ví dụ cấu hình (minh họa cú pháp; không kèm kết quả chạy):

```toml
# Cargo.toml — thư viện C-ABI nạp từ ngôn ngữ khác
[lib]
crate-type = ["cdylib"]       # hoặc ["staticlib"] để link vào ứng dụng C
```

```rust
// lib.rs — xuất một hàm C ABI với tên symbol không bị mangle
#[unsafe(no_mangle)]
pub extern "C" fn add(a: i32, b: i32) -> i32 { a + b }
```

Một số điểm bổ sung:

- **Build tĩnh bằng musl.** Chạy `rustup target add x86_64-unknown-linux-musl` rồi `cargo build --release --target x86_64-unknown-linux-musl` (lệnh minh họa). Sau đó dùng `readelf -lW` để kiểm tra *không có* `PT_INTERP`, và `readelf -d` không có `NEEDED`. Rust khuyên tự kiểm tra binary sau khi build, vì target không hỗ trợ có thể bỏ qua cờ[^rs-crt-ineffective].
- **Ranh giới unwinding.** Nếu artifact "có thể unwind" thì mọi crate trong đó phải build với panic strategy `unwind`; nếu không, unwinding có thể gây UB[^rs-unwind-prohibited]. Khi *không* link bằng rustc (ví dụ `dlopen`), bạn tự bảo đảm điều này[^rs-unwind-dlopen].
- **Plugin `dlopen` viết bằng Rust.** Hai `cdylib` mỗi cái chứa bản sao `std` riêng (suy luận của người soạn: mỗi cdylib là artifact độc lập chứa đủ upstream, theo định nghĩa `staticlib`/`cdylib` ở trên). Đừng truyền kiểu có layout Rust qua ranh giới; dùng `extern "C"` và kiểu `#[repr(C)]` (nhận xét của người soạn).
- **Cờ chuyển cho linker** là `-C link-arg=...`, ví dụ `-Clink-arg=file.o` để link một `.o`[^rs-foreign-linkers]. Một ví dụ liên quan: musl hỗ trợ tăng stack thread mặc định qua program header `PT_GNU_STACK`, đặt lúc link bằng `-Wl,-z,stack-size=N`[^musl-stack-ptgnu]. Chuyển nó qua `-C link-arg` là cách dùng (nhận xét của người soạn: nên đo trước khi áp dụng).

</details>

**Quay lại Q8:** crate type chọn *loại ELF* rustc giao (executable, rlib, staticlib, cdylib...). Target quyết định libc và tĩnh/động. `relocation-model`, `relro-level`, `strip` là ba cờ điều khiển PIE, RELRO và lượng symbol trong file `orders`.

## Liên hệ với Rust (tổng hợp)

Cả chương gắn với Rust ở mục 8. Phần này gom lại thành checklist cho một service Rust như `orders` triển khai trên Linux:

1. *Libc*: chọn có chủ đích `*-gnu` (động, glibc; build ở môi trường glibc cũ nhất cần hỗ trợ) hay `*-musl` (tĩnh; chấp nhận khác biệt DNS và allocator; kiểm tra `PT_INTERP`/`NEEDED` sau khi build).
2. *Hardening*: mặc định đã là PIE và Full RELRO; đừng hạ `relro-level` hay `relocation-model` nếu chưa có lý do đo được.
3. *Kích thước và debug*: `strip` hợp cho image sản xuất nhưng giữ symbol cho crash-reporting; địa chỉ PIE đổi theo lần chạy nên cần base address khi symbolize.
4. *Ranh giới C/Rust*: `cdylib`/`staticlib` + `#[unsafe(no_mangle)]` + `extern "C"`; một `staticlib` mỗi binary; không dựa vào định dạng mangling.
5. *Vận hành*: khi service không khởi động được vì thiếu thư viện hay version, hãy nghĩ tới `ld.so` (mục 3–4). Dùng `ld.so --list` hoặc `LD_DEBUG=libs` để chẩn đoán; không dùng `ldd` với binary không tin cậy (nhận xét của người soạn).

## Tóm tắt

- ELF có header ở offset 0 và hai bảng mục lục: section (linker/công cụ) và segment (loader). File dùng để dựng process bắt buộc có program header table[^gabi-pht]. Section header table thì bắt buộc với file dùng lúc linking[^gabi-sht].
- Relocation là bước nối tham chiếu symbol với định nghĩa[^gabi-reloc]. Shared object để lại relocation động cho `ld.so` xử lý lúc chạy[^dr-relative].
- `execve` đọc `PT_INTERP`, map chương trình và interpreter, dựng auxv rồi nhảy vào interpreter[^dr-ptinterp]. `ld.so` nạp thư viện, relocate, khởi tạo rồi chạy chương trình[^dr-steps].
- Thứ tự tìm thư viện: `DT_RPATH` (nếu không có `DT_RUNPATH`) → `LD_LIBRARY_PATH` → `DT_RUNPATH` → `ld.so.cache` → `/lib`, `/usr/lib`[^ldso-order-rpath]. `LD_LIBRARY_PATH` bị bỏ qua ở secure-execution[^ldso-order-ldlp].
- GOT là bảng địa chỉ ghi lúc chạy, PLT là stub, và lazy binding bind hàm ở lần gọi đầu[^dr-plt-resolve]. RELRO kèm bind-now khóa GOT chỉ-đọc[^dr-relro].
- PIE động là `ET_DYN` có interpreter, được kernel đặt lệch từ `ELF_ET_DYN_BASE` và ngẫu nhiên thêm khi `PF_RANDOMIZE` bật[^binfmt-load-bias]. `randomize_va_space` 0/1/2 điều khiển mức ngẫu nhiên hóa[^aslr-1].
- Binary tĩnh cổ điển không cần `ld.so`[^dr-static]. Symbol versioning khiến binary động có thể không khởi động nếu thiếu version cần[^dr-ver-fail]. musl không có lazy binding[^musl-lazy]. `dlclose` của musl là no-op[^musl-dlclose].
- Rust: `bin` mặc định *ưu tiên* liên kết tĩnh các dependency Rust (thiếu rlib thì rơi sang động)[^rs-prefer-dynamic]. `relocation-model=pic` là mặc định của đa số target[^rs-reloc-default]. `x86_64-unknown-linux-musl` tĩnh theo mặc định[^rs-crt-default]. Không thể dựa vào `strip` như biện pháp bảo mật có ý nghĩa[^rs-strip-security].

## Câu hỏi tự kiểm tra

1. Section và segment khác nhau thế nào, và bảng nào *bắt buộc* với file để chạy?
<details><summary>Đáp án</summary>

Section (section header table) là đơn vị cho linker; segment (program header table) là đơn vị cho loader; một segment chứa một hoặc nhiều section[^elf-phdr]. File dùng để dựng process phải có program header table[^gabi-pht]. Section header table thì file dùng lúc linking mới bắt buộc[^gabi-sht].

</details>

2. `.bss` có chiếm chỗ trong file không? Làm sao có đủ bộ nhớ số 0 lúc chạy?
<details><summary>Đáp án</summary>

Không: `SHT_NOBITS` không chiếm chỗ trong file[^elf-nobits]. Segment `PT_LOAD` có `p_memsz` lớn hơn `p_filesz` thì phần dư được định nghĩa là số 0[^elf-load-zero].

</details>

3. `SHN_UNDEF` nghĩa là gì và ai giải quyết?
<details><summary>Đáp án</summary>

Symbol chưa định nghĩa trong file này; khi linker ghép với file định nghĩa nó thì tham chiếu được nối với định nghĩa thật[^gabi-undef]. Nếu định nghĩa nằm trong shared object thì phần nối được hoãn cho dynamic linker (mục 2, 3).

</details>

4. Nêu hai khác biệt giữa symbol global và weak khi link.
<details><summary>Đáp án</summary>

Linker không cho phép nhiều định nghĩa `STB_GLOBAL` cùng tên; weak cùng tên với một định nghĩa global không gây lỗi và bị bỏ qua[^gabi-global-weak]. Linker cũng không rút archive member chỉ để giải quyết tham chiếu weak chưa định nghĩa, và weak chưa giải quyết có giá trị 0[^gabi-weak-undef].

</details>

5. Kernel làm gì khi gặp `PT_INTERP`, và đưa cho `ld.so` thông tin bằng cách nào?
<details><summary>Đáp án</summary>

Kernel đọc đường dẫn interpreter rồi mở file[^binfmt-open]. Sau đó nó map file này cùng chương trình[^binfmt-interp-load]. Nó đặt auxiliary vector (có `AT_PHDR`, `AT_BASE`, `AT_ENTRY`) lên stack[^binfmt-auxv]. Cuối cùng nó nhảy vào entry của interpreter[^dr-entry].

</details>

6. Binary có `DT_RUNPATH`, chạy với `LD_LIBRARY_PATH=/x`. Thư mục nào được thử trước? Và nếu binary là set-user-ID?
<details><summary>Đáp án</summary>

`LD_LIBRARY_PATH` được thử trước `DT_RUNPATH`, theo thứ tự ở bảng mục 4.2[^ldso-order-ldlp]. Với set-user-ID (secure-execution mode) biến này bị bỏ qua[^ldso-secure]. `DT_RUNPATH` chỉ dùng cho dependency trực tiếp của object chứa nó[^ldso-order-runpath].

</details>

7. Mô tả bước lazy binding ở lần gọi hàm ngoài đầu tiên.
<details><summary>Đáp án</summary>

Lời gọi vào entry PLT, lệnh đầu nhảy gián tiếp qua slot GOT, lúc đầu trỏ vào lệnh sau trong chính entry PLT[^dr-plt-first]. Lệnh `push` đẩy offset relocation (IA-32; x86-64 đẩy index)[^psabi-index]. Sau đó nó nhảy PLT0 vào dynamic linker[^dr-plt0]. Dynamic linker tìm địa chỉ, ghi vào slot GOT rồi nhảy tới hàm; lần sau slot đã chứa địa chỉ thật[^dr-plt-resolve].

</details>

8. Full RELRO khác partial RELRO thế nào, và cái giá của Full?
<details><summary>Đáp án</summary>

Partial: hàm vẫn phân giải lười, phần `.got.plt` ghi được còn `.got` chỉ-đọc[^rs-relro-partial]. Full: phân giải ngay lúc bắt đầu, GOT điền sớm rồi chỉ-đọc; tốn thêm bộ nhớ và tăng thời gian khởi động[^rs-relro-full]. Rust bật Full theo mặc định trên nền tảng hỗ trợ[^rs-relro-default].

</details>

9. Vì sao binary PIE phải có relative relocation còn executable truyền thống thì không?
<details><summary>Đáp án</summary>

Executable truyền thống có địa chỉ nạp biết lúc link nên static linker tự relocate[^dr-no-relative]. PIE động được kernel đặt ở địa chỉ nạp không biết trước lúc link (lệch từ `ELF_ET_DYN_BASE`, và ngẫu nhiên thêm khi ASLR bật)[^binfmt-load-bias]. Vì vậy con trỏ tuyệt đối trong dữ liệu trỏ tới địa chỉ trong chính file phải được cộng địa chỉ nạp lúc chạy, và đó là việc của relative relocation[^dr-relative]. (Phần suy ra này là suy luận từ hai nguồn đó.)

</details>

10. Vì sao binary build trên distro mới có thể không chạy trên distro cũ dù cùng kiến trúc, và musl tĩnh tránh điều đó thế nào?
<details><summary>Đáp án</summary>

Linker ghi vào binary danh sách version symbol cần[^dr-ver-record]. Dynamic linker kiểm tra lúc khởi động và nếu không khớp thì không khởi động[^dr-ver-fail]. glibc 2.36 cũng báo lỗi khi không tìm thấy version cần, trừ tham chiếu weak[^glibc-ver-weak]. Việc binary build trên glibc mới ghi version mới hơn là suy luận của người soạn. musl tĩnh không có dependency ngoài[^musl-no-deps].

</details>

11. Kể ba khác biệt hành vi giữa glibc và musl có thể ảnh hưởng một service Rust.
<details><summary>Đáp án</summary>

Resolver: musl hỏi song song mọi nameserver[^musl-dns-parallel]. Musl cũng xử lý `ndots`/`search` khác glibc[^musl-dns-ndots]. Dynamic loader: musl không có lazy binding[^musl-lazy]. Ở musl, `dlclose` là no-op[^musl-dlclose]. Thread stack mặc định: musl 128k[^musl-stack]. glibc thì theo `RLIMIT_STACK`, thường 2–10 MB[^glibc-stack].

</details>

12. `strip = "symbols"` có phải biện pháp bảo vệ mã nguồn không? Có làm hỏng backtrace không?
<details><summary>Đáp án</summary>

Không nên coi là biện pháp bảo vệ: nguồn nói không thể dựa vào nó như biện pháp bảo mật hay làm rối có ý nghĩa, vì disassembler và decompiler vẫn trích được nhiều thông tin[^rs-strip-security]. `symbols` có thể làm trace khó đọc ở nơi dựa vào bảng symbol; chương trình cần crash-reporting thường nên tránh[^rs-strip-warn].

</details>

## Đọc thêm

- [P3·05 — Bộ nhớ của tiến trình trên Linux](../03-memory/05-bo-nho-tien-trinh-linux.md), mục 1–3: `.bss` quan sát bằng `/proc/<pid>/maps`, thí nghiệm ASLR.
- [P2·03 — Assembly x86-64 và AArch64](../02-cpu/03-assembly-x86-64-va-aarch64.md): đọc `@GOTPCREL(%rip)`, `call foo@PLT` — nơi relocation GOT/PLT xuất hiện trong mã.
- [P8·08 — Rust trong container](../08-containers/08-rust-trong-container.md), mục 5–6: kích thước image scratch+musl vs distroless vs debian-slim, hiệu ứng `strip`/`lto`/`panic=abort`.
- Ulrich Drepper, *How To Write Shared Libraries* (<https://akkadia.org/drepper/dsohowto.pdf>): mục 1.5 (relocation, GOT/PLT), mục 2.6 (bảo mật, RELRO), mục 3.3 (ABI versioning) — đọc để hiểu sâu cơ chế và chi phí.
- System V gABI, chương 4–5 (<https://refspecs.linuxfoundation.org/elf/gabi4+/ch4.intro.html>, `ch5.dynamic.html`): đặc tả chính thức của section, symbol, relocation, dynamic section.
- `ld.so(8)` (<https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html>): mọi biến `LD_*`; `elf(5)` (<https://manpages.debian.org/bookworm/manpages/elf.5.en.html>) cho struct.
- Rust Reference 1.95.0, Linkage (<https://doc.rust-lang.org/1.95.0/reference/linkage.html>) và rustc book, Codegen options (<https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html>): crate type, `crt-static`, `relocation-model`, `relro-level`, `strip`.
- musl wiki, "Functional differences from glibc" (<https://wiki.musl-libc.org/functional-differences-from-glibc.html>): danh sách đầy đủ khác biệt hành vi.

[^elf-hdr-tables]: elf(5), Debian bookworm (man-pages 6.03), mô tả chung — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "An executable file using the ELF file format consists of an ELF header, followed by a program header table or a section header table, or both. The ELF header is always at offset zero of the file."
[^elf-phdr]: elf(5), Program header — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "An executable or shared object file's program header table is an array of structures, each describing a segment or other information the system needs to prepare the program for execution. An object file segment contains one or more sections."
[^gabi-3types]: System V gABI, ch.4 Object Files, Introduction — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch4.intro.html> — "A relocatable file holds code and data suitable for linking with other object files to create an executable or a shared object file. An executable file holds a program suitable for execution;"
[^gabi-so]: System V gABI, ch.4 Object Files, Introduction — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch4.intro.html> — "First, the link editor [see ld(BA_OS)] processes the shared object file with other relocatable and shared object files to create another object file. Second, the dynamic linker combines it with an executable file and other shared objects to create a process image."
[^elf-etype]: elf(5), e_type — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "ET_REL A relocatable file. ET_EXEC An executable file. ET_DYN A shared object."
[^gabi-views]: System V gABI, ch.4, File Format — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch4.intro.html> — "For convenience and efficiency, the object file format provides parallel views of a file's contents, reflecting the differing needs of those activities."
[^gabi-order]: System V gABI, ch.4, File Format — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch4.intro.html> — "Moreover, sections and segments have no specified order. Only the ELF header has a fixed position in the file."
[^gabi-pht]: System V gABI, ch.4, File Format — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch4.intro.html> — "Files used to build a process image (execute a program) must have a program header table; relocatable files do not need one."
[^gabi-sht]: System V gABI, ch.4, File Format — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch4.intro.html> — "Files used during linking must have a section header table; other object files may or may not have one."
[^elf-magic]: elf(5), e_ident — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "The first byte of the magic number. It must be filled with ELFMAG0. (0: 0x7f)"
[^elf-class]: elf(5), e_ident EI_CLASS — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "ELFCLASS64 This defines the 64-bit architecture."
[^elf-machine]: elf(5), e_machine — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "EM_X86_64 AMD x86-64"
[^elf-entry]: elf(5), e_entry — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This member gives the virtual address to which the system first transfers control, thus starting the process."
[^elf-phoff]: elf(5), e_phoff — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This member holds the program header table's file offset in bytes."
[^elf-phnum]: elf(5), e_phnum — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This member holds the number of entries in the program header table."
[^elf-shoff]: elf(5), e_shoff — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This member holds the section header table's file offset in bytes. If the file has no section header table, this member holds zero."
[^elf-shstrndx]: elf(5), e_shstrndx — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This member holds the section header table index of the entry associated with the section name string table."
[^elf-shdr]: elf(5), Section header — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "A file's section header table lets one locate all the file's sections."
[^elf-shf-alloc]: elf(5), sh_flags SHF_ALLOC — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This section occupies memory during process execution. Some control sections do not reside in the memory image of an object file."
[^elf-debug]: elf(5), section .debug — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This section holds information for symbolic debugging. The contents are unspecified."
[^elf-nobits]: elf(5), sh_type SHT_NOBITS — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "A section of this type occupies no space in the file but otherwise resembles SHT_PROGBITS."
[^elf-bss]: elf(5), section .bss — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This section holds uninitialized data that contributes to the program's memory image. By definition, the system initializes the data with zeros when the program begins to run. This section is of type SHT_NOBITS."
[^elf-load]: elf(5), PT_LOAD — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "The array element specifies a loadable segment, described by p_filesz and p_memsz. The bytes from the file are mapped to the beginning of the memory segment."
[^elf-load-zero]: elf(5), PT_LOAD — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "bytes are defined to hold the value 0 and to follow the segment's initialized area. The file size may not be larger than the memory size."
[^elf-interp]: elf(5), PT_INTERP — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "The array element specifies the location and size of a null-terminated pathname to invoke as an interpreter. This segment type is meaningful only for executable files (though it may occur for shared objects). However it may not occur more than once in a file. If it is present, it must precede any loadable segment entry."
[^elf-ptdynamic]: elf(5), PT_DYNAMIC — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "The array element specifies dynamic linking information."
[^elf-phdr-self]: elf(5), PT_PHDR — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "The array element, if present, specifies the location and size of the program header table itself, both in the file and in the memory image of the program. This segment type may not occur more than once in a file. Moreover, it may occur only if the program header table is part of the memory image of the program. If it is present, it must precede any loadable segment entry."
[^elf-gnustack]: elf(5), PT_GNU_STACK — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "GNU extension which is used by the Linux kernel to control the state of the stack via the flags set in the p_flags member."
[^elf-pflags]: elf(5), p_flags — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "A text segment commonly has the flags PF_X and PF_R . A data segment commonly has PF_W and PF_R."
[^elf-interp-sec]: elf(5), section .interp — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This section holds the pathname of a program interpreter."
[^elf-dynamic-sec]: elf(5), section .dynamic — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This section holds dynamic linking information."
[^elf-dynsym]: elf(5), sh_type SHT_DYNSYM — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This section holds a minimal set of dynamic linking symbols."
[^elf-symtab]: elf(5), sh_type SHT_SYMTAB — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This section holds a symbol table. Typically, SHT_SYMTAB provides symbols for link editing, though it may also be used for dynamic linking."
[^elf-relaname]: elf(5), section .relaNAME — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This section holds relocation information as described below. If the file has a loadable segment that includes relocation, the section's attributes will include the SHF_ALLOC bit."
[^elf-got]: elf(5), section .got — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This section holds the global offset table."
[^elf-plt]: elf(5), section .plt — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This section holds the procedure linkage table."
[^elf-verneed]: elf(5), section .gnu.version_r — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This section holds the version symbol needed elements, a table of ElfN_Verneed structures."
[^elf-verdef]: elf(5), section .gnu.version_d — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This section holds the version symbol definitions, a table of ElfN_Verdef structures."
[^elf-versym]: elf(5), section .gnu.version — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This section holds the version symbol table, an array of ElfN_Half elements."
[^elf-phnum-xnum]: elf(5), e_phnum — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "If the number of entries in the program header table is larger than or equal to PN_XNUM (0xffff), this member holds PN_XNUM (0xffff) and the real number of entries in the program header table is held in the sh_info member of the initial entry in section header table."
[^elf-malign]: elf(5), p_align — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "Loadable process segments must have congruent values for p_vaddr and p_offset, modulo the page size."
[^elf-notes]: elf(5), Notes — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "ELF notes allow for appending arbitrary information for the system to use. They are largely used by core files (e_type of ET_CORE), but many projects define their own set of extensions."
[^elf-symtab-def]: elf(5), String and symbol tables — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "An object file's symbol table holds information needed to locate and relocate a program's symbolic definitions and references."
[^elf-bind-local]: elf(5), STB_LOCAL — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "Local symbols are not visible outside the object file containing their definition. Local symbols of the same name may exist in multiple files without interfering with each other."
[^elf-bind-global]: elf(5), STB_GLOBAL — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "Global symbols are visible to all object files being combined. One file's definition of a global symbol will satisfy another file's undefined reference to the same symbol."
[^elf-bind-weak]: elf(5), STB_WEAK — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "Weak symbols resemble global symbols, but their definitions have lower precedence."
[^gabi-global-weak]: System V gABI, ch.4, Symbol Table — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch4.symtab.html> — "When the link editor combines several relocatable object files, it does not allow multiple definitions of STB_GLOBAL symbols with the same name. On the other hand, if a defined global symbol exists, the appearance of a weak symbol with the same name will not cause an error. The link editor honors the global definition and ignores the weak ones."
[^gabi-undef]: System V gABI, ch.4, Symbol Table, SHN_UNDEF — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch4.symtab.html> — "This section table index means the symbol is undefined. When the link editor combines this object file with another that defines the indicated symbol, this file's references to the symbol will be linked to the actual definition."
[^elf-relocs]: elf(5), Relocation entries — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This member gives both the symbol table index with respect to which the relocation must be made and the type of relocation to apply."
[^gabi-reloc]: System V gABI, ch.4, Relocation — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch4.reloc.html> — "Relocation is the process of connecting symbolic references with symbolic definitions. For example, when a program calls a function, the associated call instruction must transfer control to the proper destination address at execution."
[^gabi-reloc-type]: System V gABI, ch.4, Relocation — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch4.reloc.html> — "Relocation types are processor-specific; descriptions of their behavior appear in the processor supplement."
[^gabi-rel-addend]: System V gABI, ch.4, Relocation — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch4.reloc.html> — "Entries of type Elf32_Rel and Elf64_Rel store an implicit addend in the location to be modified."
[^elf-roffset]: elf(5), r_offset — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "For a relocatable file, the value is the byte offset from the beginning of the section to the storage unit affected by the relocation. For an executable file or shared object, the value is the virtual address of the storage unit affected by the relocation."
[^gabi-reloc-apply]: System V gABI, ch.4, Relocation — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch4.reloc.html> — "The typical application of an ELF relocation is to determine the referenced symbol value, extract the addend (either from the field to be relocated or from the addend field contained in the relocation record, as appropriate for the type of relocation record), apply the expression implied by the relocation type to the symbol and addend, extract the desired part of the expression result, and place it in the field to be relocated."
[^gabi-archive-extract]: System V gABI, ch.4, Symbol Table, weak symbols — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch4.symtab.html> — "it extracts archive members that contain definitions of undefined global symbols."
[^gabi-archive-copy]: System V gABI, ch.5, Shared Object Dependencies — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch5.dynamic.html> — "When the link editor processes an archive library, it extracts library members and copies them into the output object file. These statically linked services are available during execution without involving the dynamic linker."
[^gabi-weak-undef]: System V gABI, ch.4, Symbol Table, weak symbols — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch4.symtab.html> — "The link editor does not extract archive members to resolve undefined weak symbols. Unresolved weak symbols have a zero value."
[^dr-static]: Drepper, How To Write Shared Libraries v4.1.2, 1.2 — <https://akkadia.org/drepper/dsohowto.pdf> — "Such an application has a fixed load address which the kernel knows. The load process consists simply of making the binary available in the appropriate address space of a newly created process and transferring control to the entry point of the application. Everything else was done by the static linker when creating the executable."
[^dr-no-relative]: Drepper, How To Write Shared Libraries v4.1.2, 1.5.1 — <https://akkadia.org/drepper/dsohowto.pdf> — "Note that applications do not have relative relocations since the load address of the code is known at link-time and therefore the static linker is able to perform the relocation."
[^elf-vis-default]: elf(5), STV_DEFAULT — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "Default symbol visibility rules. Global and weak symbols are available to other modules; references in the local module can be interposed by definitions in other modules."
[^elf-vis-hidden]: elf(5), STV_HIDDEN — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "Symbol is unavailable to other modules; references in the local module always resolve to the local symbol (i.e., the symbol can't be interposed by definitions in other modules)."
[^gabi-jmprel]: System V gABI, ch.5, Dynamic Section, DT_JMPREL — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch5.dynamic.html> — "Separating these relocation entries lets the dynamic linker ignore them during process initialization, if lazy binding is enabled."
[^dr-relative]: Drepper, How To Write Shared Libraries v4.1.2, 1.5.1 — <https://akkadia.org/drepper/dsohowto.pdf> — "The implementation of relative relocations is easy. The linker can compute the offset of the target destination in the object file at link-time. To this value the dynamic linker only has to add the load address of the object and store the result in the place indicated by the relocation."
[^dr-symreloc-cost]: Drepper, How To Write Shared Libraries v4.1.2, 1.5.1 — <https://akkadia.org/drepper/dsohowto.pdf> — "Fact is that symbol relocation is a costly process and the more DSOs participate or the more symbols are defined in the DSOs, the longer the symbol lookup takes."
[^dr-textrel]: Drepper, How To Write Shared Libraries v4.1.2, 1.5.1 — <https://akkadia.org/drepper/dsohowto.pdf> — "But the result is that the modified page cannot be shared with other processes using the same object."
[^execve-replace]: execve(2), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/execve.2.en.html> — "This causes the program that is currently being run by the calling process to be replaced with a new program, with newly initialized stack, heap, and (initialized and uninitialized) data segments."
[^execve-mappings]: execve(2), Effect on process attributes — <https://manpages.debian.org/bookworm/manpages-dev/execve.2.en.html> — "Memory mappings are not preserved (mmap(2))."
[^dr-entry]: Drepper, How To Write Shared Libraries v4.1.2, 1.4 — <https://akkadia.org/drepper/dsohowto.pdf> — "After setting up the auxiliary vector the kernel is finally ready to transfer control to the dynamic linker in user mode. The entry point is defined in e entry field of the ELF header of the dynamic linker."
[^ldso-desc]: ld.so(8), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "The programs ld.so and ld-linux.so* find and load the shared objects (shared libraries) needed by a program, prepare the program to run, and then run it."
[^dr-ptinterp]: Drepper, How To Write Shared Libraries v4.1.2, 1.4 — <https://akkadia.org/drepper/dsohowto.pdf> — "Which binary implements the dynamic linker is not hardcoded in the kernel. Instead the program header of the application contains an entry with the tag PT INTERP."
[^binfmt-interp]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "if (elf_ppnt->p_type != PT_INTERP) continue; /* * This is the program interpreter used for shared libraries - * for now assume that this is an a.out format binary. */"
[^binfmt-open]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "retval = elf_read(bprm->file, elf_interpreter, elf_ppnt->p_filesz, elf_ppnt->p_offset); if (retval < 0) goto out_free_interp; /* make sure path is NULL terminated */ retval = -ENOEXEC; if (elf_interpreter[elf_ppnt->p_filesz - 1] != '\0') goto out_free_interp; interpreter = open_exec(elf_interpreter); kfree(elf_interpreter); retval = PTR_ERR(interpreter);"
[^execve-einval]: execve(2), ERRORS — <https://manpages.debian.org/bookworm/manpages-dev/execve.2.en.html> — "An ELF executable had more than one PT_INTERP segment (i.e., tried to name more than one interpreter)."
[^execve-elibbad]: execve(2), ERRORS — <https://manpages.debian.org/bookworm/manpages-dev/execve.2.en.html> — "ELIBBAD An ELF interpreter was not in a recognized format."
[^binfmt-interp-load]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "elf_entry = load_elf_interp(interp_elf_ex, interpreter, load_bias, interp_elf_phdata, &arch_state);"
[^dr-interp-complete]: Drepper, How To Write Shared Libraries v4.1.2, 1.4 — <https://akkadia.org/drepper/dsohowto.pdf> — "The difference is that the dynamic linker itself must be complete and should be freely relocatable."
[^dr-auxv]: Drepper, How To Write Shared Libraries v4.1.2, 1.4 — <https://akkadia.org/drepper/dsohowto.pdf> — "The kernel puts an array of tag-value pairs on the stack of the new process. This auxiliary vector contains beside the two aforementioned values several more values which allow the dynamic linker to avoid several system calls."
[^binfmt-auxv]: Linux v6.8, fs/binfmt_elf.c, create_elf_tables — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "NEW_AUX_ENT(AT_PHDR, phdr_addr); NEW_AUX_ENT(AT_PHENT, sizeof(struct elf_phdr)); NEW_AUX_ENT(AT_PHNUM, exec->e_phnum); NEW_AUX_ENT(AT_BASE, interp_load_addr); if (bprm->interp_flags & BINPRM_FLAGS_PRESERVE_ARGV0) flags |= AT_FLAGS_PRESERVE_ARGV0; NEW_AUX_ENT(AT_FLAGS, flags); NEW_AUX_ENT(AT_ENTRY, e_entry);"
[^auxv-base]: getauxval(3), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/getauxval.3.en.html> — "AT_BASE The base address of the program interpreter (usually, the dynamic linker)."
[^auxv-entry]: getauxval(3) — <https://manpages.debian.org/bookworm/manpages-dev/getauxval.3.en.html> — "AT_ENTRY The entry address of the executable."
[^auxv-phdr]: getauxval(3) — <https://manpages.debian.org/bookworm/manpages-dev/getauxval.3.en.html> — "AT_PHDR The address of the program headers of the executable."
[^binfmt-entry]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "interp_load_addr = elf_entry; elf_entry += interp_elf_ex->e_entry;"
[^dr-steps]: Drepper, How To Write Shared Libraries v4.1.2, 1.5 — <https://akkadia.org/drepper/dsohowto.pdf> — "Determine and load dependencies; Relocate the application and all dependencies; Initialize the application and dependencies in the correct order."
[^dr-dynlinker-tasks]: Drepper, How To Write Shared Libraries v4.1.2, 1.2 — <https://akkadia.org/drepper/dsohowto.pdf> — "The task of the dynamic linker is it, to complete the dynamically linked application by loading the DSOs it needs (the dependencies) and to perform the relocations. Then finally control can be transferred to the program."
[^binfmt-pie]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "There are effectively two types of ET_DYN binaries: programs (i.e. PIE: ET_DYN with INTERP) and loaders (ET_DYN without INTERP, since they _are_ the ELF interpreter)."
[^binfmt-loader-away]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "The loaders must be loaded away from programs since the program may otherwise collide with the loader (especially for ET_EXEC which does not have a randomized position)."
[^binfmt-pie-base]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "Therefore, programs are loaded offset from ELF_ET_DYN_BASE and loaders are loaded into the independently randomized mmap region (0 load_bias without MAP_FIXED nor MAP_FIXED_NOREPLACE)."
[^ldso-interp-sec]: ld.so(8), SYNOPSIS — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "in the ELF case, the dynamic linker which is stored in the .interp section of the program is executed"
[^ldso-list]: ld.so(8), OPTIONS — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "--list List all dependencies and how they are resolved."
[^ldd-trace]: ldd(1), Debian bookworm — <https://manpages.debian.org/bookworm/manpages/ldd.1.en.html> — "In the usual case, ldd invokes the standard dynamic linker (see ld.so(8)) with the LD_TRACE_LOADED_OBJECTS environment variable set to 1."
[^ldd-sec]: ldd(1), Security — <https://manpages.debian.org/bookworm/manpages/ldd.1.en.html> — "Thus, you should never employ ldd on an untrusted executable, since this may result in the execution of arbitrary code."
[^ldd-objdump]: ldd(1), Security — <https://manpages.debian.org/bookworm/manpages/ldd.1.en.html> — "Note, however, that this alternative shows only the direct dependencies of the executable, while ldd shows the entire dependency"
[^ldd-special]: ldd(1) — <https://manpages.debian.org/bookworm/manpages/ldd.1.en.html> — "(The linux-vdso and ld-linux shared dependencies are special; see vdso(7) and ld.so(8).)"
[^auxv-vdso]: getauxval(3) — <https://manpages.debian.org/bookworm/manpages-dev/getauxval.3.en.html> — "The address of a page containing the virtual Dynamic Shared Object (vDSO) that the kernel creates in order to provide fast implementations of certain system calls."
[^execve-script]: execve(2), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/execve.2.en.html> — "pathname must be either a binary executable, or a script starting with a line of the form: #!interpreter [optional-arg]"
[^auxv-secure]: getauxval(3) — <https://manpages.debian.org/bookworm/manpages-dev/getauxval.3.en.html> — "AT_SECURE Has a nonzero value if this executable should be treated securely."
[^ldso-slash]: ld.so(8), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "If a slash is found, then the dependency string is interpreted as a (relative or absolute) pathname, and the shared object is loaded using that pathname."
[^ldso-order-rpath]: ld.so(8), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "Using the directories specified in the DT_RPATH dynamic section attribute of the binary if present and DT_RUNPATH attribute does not exist. Use of DT_RPATH is deprecated."
[^ldso-order-ldlp]: ld.so(8), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "Using the environment variable LD_LIBRARY_PATH, unless the executable is being run in secure-execution mode (see below), in which case this variable is ignored."
[^ldso-order-runpath]: ld.so(8), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "Such directories are searched only to find those objects required by DT_NEEDED (direct dependencies) entries and do not apply to those objects' children, which must themselves have their own DT_RUNPATH entries. This is unlike DT_RPATH, which is applied to searches for all children in the dependency tree."
[^ldso-order-cache]: ld.so(8), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "From the cache file /etc/ld.so.cache, which contains a compiled list of candidate shared objects previously found in the augmented library path."
[^ldso-order-default]: ld.so(8), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "In the default path /lib, and then /usr/lib. (On some 64-bit architectures, the default paths for 64-bit shared objects are /lib64, and then /usr/lib64.) If the binary was linked with the -z nodeflib linker option, this step is skipped."
[^gabi-needed-names]: System V gABI, ch.5, Shared Object Dependencies — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch5.dynamic.html> — "Names in the dependency list are copies either of the DT_SONAME strings or the path names of the shared objects used to build the object file."
[^gabi-runpath-scope]: System V gABI, ch.5, Shared Object Dependencies — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch5.dynamic.html> — "One object's DT_RUNPATH entry does not affect the search for any other object's dependencies."
[^ldso-tokens]: ld.so(8), Dynamic string tokens — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "This expands to the directory containing the program or shared object."
[^ldso-origin]: ld.so(8), Dynamic string tokens — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "so that it finds an associated shared object in somedir/lib no matter where somedir is located in the directory hierarchy."
[^ldso-origin-turnkey]: ld.so(8), Dynamic string tokens — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "This facilitates the creation of turn-key applications that do not need to be installed into special directories, but can instead be unpacked into any directory and still find their own shared objects."
[^ldso-ldlp]: ld.so(8), LD_LIBRARY_PATH — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "A list of directories in which to search for ELF libraries at execution time. The items in the list are separated by either colons or semicolons, and there is no support for escaping either separator. A zero-length directory name indicates the current working directory."
[^ldso-secure]: ld.so(8), Secure-execution mode — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "A binary is executed in secure-execution mode if the AT_SECURE entry in the auxiliary vector (see getauxval(3)) has a nonzero value."
[^ldso-preload]: ld.so(8), LD_PRELOAD — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "A list of additional, user-specified, ELF shared objects to be loaded before all others. This feature can be used to selectively override functions in other shared objects."
[^ldso-preload-secure]: ld.so(8), LD_PRELOAD — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "In secure-execution mode, preload pathnames containing slashes are ignored."
[^dlopen-search]: dlopen(3), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/dlopen.3.en.html> — "(ELF only) If the calling object (i.e., the shared library or executable from which dlopen() is called) contains a DT_RPATH tag, and does not contain a DT_RUNPATH tag, then the directories listed in the DT_RPATH tag are searched."
[^gabi-wrong-attrs]: System V gABI, ch.5, Shared Object Dependencies — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch5.dynamic.html> — "When the dynamic linker is searching for shared objects, it is not a fatal error if an ELF file with the wrong attributes is encountered in the search."
[^ldso-debug]: ld.so(8), LD_DEBUG — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "Output verbose debugging information about operation of the dynamic linker."
[^ldso-debug-libs]: ld.so(8), LD_DEBUG — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "libs Display library search paths."
[^ldso-cache-file]: ld.so(8), FILES — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "File containing a compiled list of directories in which to search for shared objects and an ordered list of candidate shared objects."
[^dlopen-ldlp-start]: dlopen(3) — <https://manpages.debian.org/bookworm/manpages-dev/dlopen.3.en.html> — "If, at the time that the program was started, the environment variable LD_LIBRARY_PATH was defined to contain a colon-separated list of directories, then these are searched."
[^elf-dt-rpath]: elf(5), Dynamic tags — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "DT_RPATH String table offset to library search path (deprecated)"
[^elf-dt-runpath]: elf(5), Dynamic tags — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "DT_RUNPATH String table offset to library search path"
[^ldso-hwcap]: ld.so(8), Hardware capabilities — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "The dynamic linker checks these directories against the hardware of the machine and selects the most suitable version of a given shared object."
[^dr-got-why]: Drepper, How To Write Shared Libraries v4.1.2, 1.5.5 — <https://akkadia.org/drepper/dsohowto.pdf> — "If the address is only known to the dynamic linker the text segment would have to be modified at run-time."
[^dr-plt-stub]: Drepper, How To Write Shared Libraries v4.1.2, 1.5.5 — <https://akkadia.org/drepper/dsohowto.pdf> — "Instead control is transferred to a stub for bar in the PLT (indicated by bar@PLT)."
[^dr-plt-resolve]: Drepper, How To Write Shared Libraries v4.1.2, 1.5.5 — <https://akkadia.org/drepper/dsohowto.pdf> — "Once the dynamic linker has determined the address of the function it stores the result in the GOT entry which was used in the jmp instruction at the beginning of the PLT entry before jumping to the found function. This has the effect that all future uses of the PLT entry will not go through the dynamic linker, but will instead directly transfer to the function."
[^dr-relro]: Drepper, How To Write Shared Libraries v4.1.2, 2.6 — <https://akkadia.org/drepper/dsohowto.pdf> — "Creating objects this way is enabled by the -z relro linker option. The linker is instructed to move the sections, which are only modified by relocations, onto separate memory page and emit a new program header entry PT GNU RELRO to point the dynamic linker to these special pages. At runtime the dynamic linker can then remove the write access to these pages after it is done."
[^dr-two-relocs]: Drepper, How To Write Shared Libraries v4.1.2, 1.5.5 — <https://akkadia.org/drepper/dsohowto.pdf> — "The call to bar requires two relocations: one to load the value of foo and another one to find the address of bar."
[^dr-got-load]: Drepper, How To Write Shared Libraries v4.1.2, 1.5.5 — <https://akkadia.org/drepper/dsohowto.pdf> — "The address of the variable foo is now not part of the instruction. Instead it is loaded from the GOT. The address of the location in the GOT relative to the PIC register value (%ebx) is known at link-time. Therefore the text segment does not have to be changed, only the GOT."
[^dr-plt-always]: Drepper, How To Write Shared Libraries v4.1.2, 1.5.5 — <https://akkadia.org/drepper/dsohowto.pdf> — "The PLT stub is always used if the function is not guaranteed to be defined in the object which references it. Please note that a simple definition in the object with the reference is not enough to avoid the PLT entry."
[^dr-interposition]: Drepper, How To Write Shared Libraries v4.1.2, 1.5.5 — <https://akkadia.org/drepper/dsohowto.pdf> — "Looking at the symbol lookup process it should be clear that the definition could be found in another object (interposition) in which case the PLT is needed."
[^dr-plt-arch]: Drepper, How To Write Shared Libraries v4.1.2, 1.5.5 — <https://akkadia.org/drepper/dsohowto.pdf> — "For instance, while the PLT on IA-32 is read-only it must be writable for other architectures since instead of indirect jumps using GOT values the PLT entries are modified directly."
[^dr-plt-slot]: Drepper, How To Write Shared Libraries v4.1.2, 1.5.5 — <https://akkadia.org/drepper/dsohowto.pdf> — "All the following entries belong to exactly one function symbol. The first instruction is an indirect jump where the address is taken from a slot in the GOT. Each PLT entry has one GOT slot."
[^dr-plt-first]: Drepper, How To Write Shared Libraries v4.1.2, 1.5.5 — <https://akkadia.org/drepper/dsohowto.pdf> — "At startup time the dynamic linker fills the GOT slot with the address pointing to the second instruction of the appropriate PLT entry. I.e., when the PLT entry is used for the first time the jump ends at the following pushl instruction. The value pushed on the stack is also specific to the PLT slot and it is the offset of the relocation entry for the function which should be called."
[^dr-plt0]: Drepper, How To Write Shared Libraries v4.1.2, 1.5.5 — <https://akkadia.org/drepper/dsohowto.pdf> — "Then control is transferred to the special first PLT entry which pushes some more values on the stack and finally jumps into the dynamic linker."
[^gabi-lazy]: System V gABI, ch.5, Dynamic Linking — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch5.dynamic.html> — "The dynamic linker is permitted to evaluate procedure linkage table entries lazily, thus avoiding symbol resolution and relocation overhead for functions that are not called."
[^dlopen-lazy-var]: dlopen(3), RTLD_LAZY — <https://manpages.debian.org/bookworm/manpages-dev/dlopen.3.en.html> — "(Lazy binding is performed only for function references; references to variables are always immediately bound when the shared object is loaded.) Since glibc 2.1.1, this flag is overridden by the effect of the LD_BIND_NOW environment variable."
[^ldso-bindnow]: ld.so(8), LD_BIND_NOW — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "If set to a nonempty string, causes the dynamic linker to resolve all symbols at program startup instead of deferring function call resolution to the point when they are first referenced."
[^gabi-dtbindnow]: System V gABI, ch.5, Dynamic Section, DT_BIND_NOW — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch5.dynamic.html> — "If present in a shared object or executable, this entry instructs the dynamic linker to process all relocations for the object containing this entry before transferring control to the program. The presence of this entry takes precedence over a directive to use lazy binding for this object when specified through the environment or via dlopen(BA_LIB). This entry is at level 2. Its use has been superseded by the DF_BIND_NOW flag."
[^dr-bindnow]: Drepper, How To Write Shared Libraries v4.1.2, 1.5.1 — <https://akkadia.org/drepper/dsohowto.pdf> — "Lazy relocation can be disabled for an individual object by adding the -z now option to the linker command line. The linker will set the DF BIND NOW flag in the DT FLAGS entry of the dynamic section to mark the DSO."
[^dlopen-now]: dlopen(3), RTLD_NOW — <https://manpages.debian.org/bookworm/manpages-dev/dlopen.3.en.html> — "all undefined symbols in the shared object are resolved before dlopen() returns."
[^dr-got-attack]: Drepper, How To Write Shared Libraries v4.1.2, 2.6 (security) — <https://akkadia.org/drepper/dsohowto.pdf> — "This kind of attack would not be possible if the data GOT and PLT could not be modified by the user program."
[^dr-mprotect]: Drepper, How To Write Shared Libraries v4.1.2, 2.6 (security) — <https://akkadia.org/drepper/dsohowto.pdf> — "The required calls to mprotect are prohibitively expensive, ruling out this solution for any system which aims to be performing well."
[^dr-now]: Drepper, How To Write Shared Libraries v4.1.2, 2.6 (security) — <https://akkadia.org/drepper/dsohowto.pdf> — "By using the -z now linker option it is possible to disable all lazy relocation at the expense of increased startup costs and make all relocations eligible for this special treatment."
[^dr-now-longrun]: Drepper, How To Write Shared Libraries v4.1.2, 2.6 (security) — <https://akkadia.org/drepper/dsohowto.pdf> — "For long-running applications which are security relevant this is definitely attractive: the startup costs should not weigh in as much as the gained security."
[^rs-relro-off]: rustc book 1.95.0, Codegen options, relro-level — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "off: Dynamically linked functions are resolved lazily and the GOT is writable."
[^rs-relro-partial]: rustc book 1.95.0, Codegen options, relro-level — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "partial: Dynamically linked functions are resolved lazily and written into the Procedure Linking Table (PLT) part of the GOT (.got.plt). The non-PLT part of the GOT (.got) is made read-only and both are moved to prevent writing from buffer overflows."
[^rs-relro-full]: rustc book 1.95.0, Codegen options, relro-level — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "full: Dynamically linked functions are resolved at the start of program execution and the Global Offset Table (.got/.got.plt) is populated eagerly and then made read-only. The GOT is also moved to prevent writing from buffer overflows. Full RELRO uses more memory and increases process startup time."
[^rs-relro-default]: rustc book 1.95.0, Codegen options, relro-level — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "Each rustc target has its own default for RELRO. rustc enables Full RELRO by default on platforms where it is supported."
[^rs-base-linux]: rust-lang/rust tag 1.95.0, compiler/rustc_target/src/spec/base/linux.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/compiler/rustc_target/src/spec/base/linux.rs> — "position_independent_executables: true, relro_level: RelroLevel::Full, has_thread_local: true, crt_static_respected: true,"
[^dr-avoid-plt]: Drepper, How To Write Shared Libraries v4.1.2, 1.5.5 — <https://akkadia.org/drepper/dsohowto.pdf> — "We will later explain exactly when and how to avoid PLT entries."
[^ldso-bindnot]: ld.so(8), LD_BIND_NOT — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "do not update the GOT (global offset table) and PLT (procedure linkage table) after resolving a function symbol. By combining the use of this variable with LD_DEBUG (with the categories bindings and symbols), one can observe all run-time function bindings."
[^musl-lazy]: musl wiki, Functional differences from glibc, Lazy bindings — <https://wiki.musl-libc.org/functional-differences-from-glibc.html> — "In any case, musl does not support lazy binding. This is both for robustness reasons (reporting failure of lazy binding is impossible) and because it greatly reduces the amount of fragile, arch-dependent code needed in the dynamic linker."
[^musl-glibc-lazy]: musl wiki, Functional differences from glibc, Lazy bindings — <https://wiki.musl-libc.org/functional-differences-from-glibc.html> — "glibc's dynamic linker supports lazy binding: deferring resolution of symbol references to the first time the function is called."
[^dlopen-lazy]: dlopen(3), RTLD_LAZY — <https://manpages.debian.org/bookworm/manpages-dev/dlopen.3.en.html> — "Perform lazy binding. Resolve symbols only as the code that references them is executed. If the symbol is never referenced, then it is never resolved."
[^aslr-1]: Linux kernel docs, admin-guide/sysctl/kernel, randomize_va_space — <https://docs.kernel.org/admin-guide/sysctl/kernel.html> — "Make the addresses of mmap base, stack and VDSO page randomized. This, among other things, implies that shared libraries will be loaded to random addresses. Also for PIE-linked binaries, the location of code start is randomized."
[^gabi-exec-abs]: System V gABI, ch.5, Base Address — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch5.pheader.html> — "Executable files typically contain absolute code. To let the process execute correctly, the segments must reside at the virtual addresses used to build the executable file."
[^gabi-base]: System V gABI, ch.5, Base Address — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch5.pheader.html> — "On the other hand, shared object segments typically contain position-independent code. This lets a segment's virtual address change from one process to another, without invalidating execution behavior."
[^gabi-baseaddr]: System V gABI, ch.5, Base Address — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch5.pheader.html> — "The differences between the virtual address of any segment in memory and the corresponding virtual address in the file is thus a single constant value for any one executable or shared object in a given process. This difference is the base address."
[^dr-dso]: Drepper, How To Write Shared Libraries v4.1.2, 1.2 — <https://akkadia.org/drepper/dsohowto.pdf> — "In fact, DSOs are little more than a special kind of binary; the difference is that they have no fixed load address and hence require the dynamic linker to actually become executable."
[^dr-pie]: Drepper, How To Write Shared Libraries v4.1.2, 1.2 — <https://akkadia.org/drepper/dsohowto.pdf> — "With Position Independent Executable (PIEs) the difference shrinks even more."
[^binfmt-load-bias]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "if (interpreter) { load_bias = ELF_ET_DYN_BASE; if (current->flags & PF_RANDOMIZE) load_bias += arch_mmap_rnd();"
[^binfmt-load-bias-init]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "unsigned long load_bias = 0, phdr_addr = 0;"
[^binfmt-etexec]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "This logic is run once for the first LOAD Program Header for ET_EXEC binaries. No special handling is needed."
[^aslr-2]: Linux kernel docs, admin-guide/sysctl/kernel, randomize_va_space — <https://docs.kernel.org/admin-guide/sysctl/kernel.html> — "Additionally enable heap randomization. This is the default if CONFIG_COMPAT_BRK is disabled."
[^binfmt-pfrandomize]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "if (!(current->personality & ADDR_NO_RANDOMIZE) && randomize_va_space) current->flags |= PF_RANDOMIZE;"
[^binfmt-stack-rnd]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "retval = setup_arg_pages(bprm, randomize_stack_top(STACK_TOP),"
[^rs-reloc-intro]: rustc book 1.95.0, Codegen options, relocation-model — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "This option controls generation of position-independent code (PIC)."
[^rs-reloc-pic]: rustc book 1.95.0, Codegen options, relocation-model — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "pic - fully relocatable position independent code, machine instructions need to use relative addressing modes."
[^rs-reloc-default]: rustc book 1.95.0, Codegen options, relocation-model — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "depending on the produced crate types. This is the default model for majority of supported targets."
[^rs-reloc-pie]: rustc book 1.95.0, Codegen options, relocation-model — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "pie - position independent executable, relocatable code but without support for symbol interpositioning (replacing symbols by name using LD_PRELOAD and similar)."
[^rs-reloc-static]: rustc book 1.95.0, Codegen options, relocation-model — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "static - non-relocatable code, machine instructions may use absolute addressing modes."
[^rs-reloc-link]: rustc book 1.95.0, Codegen options, relocation-model (Linking effects) — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "If the relocation model is pic and the current target supports position-independent executables (PIE), the linker will be instructed (-pie) to produce one."
[^rs-reloc-crt]: rustc book 1.95.0, Codegen options, relocation-model (Linking effects) — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "If the target doesn’t support both position-independent and statically linked executables, then -C target-feature=+crt-static “wins” over -C relocation-model=pic, and the linker is instructed (-static) to produce a statically linked but not position-independent executable."
[^rs-x64-musl]: rust-lang/rust tag 1.95.0, compiler/rustc_target/src/spec/targets/x86_64_unknown_linux_musl.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/compiler/rustc_target/src/spec/targets/x86_64_unknown_linux_musl.rs> — "base.stack_probes = StackProbeType::Inline; base.static_position_independent_executables = true;"
[^rs-spec-static-pie]: rust-lang/rust tag 1.95.0, compiler/rustc_target/src/spec/mod.rs, TargetOptions — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/compiler/rustc_target/src/spec/mod.rs> — "Executables that are both statically linked and position-independent are supported."
[^ldso-map32]: ld.so(8), LD_PREFER_MAP_32BIT_EXEC — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "Because MAP_32BIT reduces the address range available for address space layout randomization (ASLR), LD_PREFER_MAP_32BIT_EXEC is always disabled in secure-execution mode."
[^ldso-static]: ld.so(8), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "Linux binaries require dynamic linking (linking at run time) unless the -static option was given to ld(1) during compilation."
[^dr-dynamic-incomplete]: Drepper, How To Write Shared Libraries v4.1.2, 1.2 — <https://akkadia.org/drepper/dsohowto.pdf> — "Dynamically linked binaries, in contrast, are not complete when they are loaded from disk. It is therefore not possible for the kernel to immediately transfer control to the application."
[^elf-needed]: elf(5), Dynamic tags — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "DT_NEEDED String table offset to name of a needed library"
[^gabi-once]: System V gABI, ch.5, Shared Object Dependencies — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch5.dynamic.html> — "Even when a shared object is referenced multiple times in the dependency list, the dynamic linker will connect the object only once to the process."
[^gabi-bfs]: System V gABI, ch.5, Shared Object Dependencies — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch5.dynamic.html> — "When resolving symbolic references, the dynamic linker examines the symbol tables with a breadth-first search. That is, it first looks at the symbol table of the executable program itself, then at the symbol tables of the DT_NEEDED entries (in order), and then at the second level DT_NEEDED entries, and so on."
[^musl-libc-def]: musl, About musl — <https://musl.libc.org/about.html> — "an implementation of the standard library functionality described in the ISO C and POSIX standards, plus common extensions, built on top of the Linux system calls API."
[^musl-libc-does]: musl, About musl — <https://musl.libc.org/about.html> — "providing C bindings for the OS interfaces constructing higher-level buffered stdio, memory allocation management, thread creation and synchronization operations, shared library loading, and so on using the lower-level interfaces the kernel provides"
[^dr-ver-filename]: Drepper, How To Write Shared Libraries v4.1.2, 3.3 — <https://akkadia.org/drepper/dsohowto.pdf> — "The first method is the oldest and coarsest grained one. It is implemented by using a different filename for each incompatible DSO."
[^dr-soname]: Drepper, How To Write Shared Libraries v4.1.2, 3.3 — <https://akkadia.org/drepper/dsohowto.pdf> — "In ELF binaries dependencies on DSOs are recorded using DT NEEDED entries in the dynamic segment. The string associated with the entry has to be the name of an existing DSO. It is usually taken from the string associated with the DT SONAME entry in the DSO."
[^dr-ver-record]: Drepper, How To Write Shared Libraries v4.1.2, 3.3 — <https://akkadia.org/drepper/dsohowto.pdf> — "When the linker uses the versioned DSO to satisfy a dependency it also records the version of the used symbol. This way it gets for each DSO a list of required versions. This list is recorded in the binary which is produced."
[^dr-ver-fail]: Drepper, How To Write Shared Libraries v4.1.2, 3.3 — <https://akkadia.org/drepper/dsohowto.pdf> — "If no matching entry is found the DSO used at runtime is incompatible and the program does not start up."
[^dr-linux-ver-two]: Drepper, How To Write Shared Libraries v4.1.2, 3.3 — <https://akkadia.org/drepper/dsohowto.pdf> — "it is possible to have more than one definition of a given symbol (the associated version must differ) and the application or DSO linked with the versioned DSO contains not only a list of the required version, but also records for each symbol which symbol version was used and from which DSO the definition came."
[^dr-linux-ver]: Drepper, How To Write Shared Libraries v4.1.2, 3.3 — <https://akkadia.org/drepper/dsohowto.pdf> — "With Linux's symbol versioning mechanism this is not necessary. ABIs can normally be kept stable for as long as wanted."
[^rs-glibc217]: rustc book 1.95.0, Platform Support — <https://doc.rust-lang.org/1.95.0/rustc/platform-support.html> — "x86_64-unknown-linux-gnu 64-bit Linux (kernel 3.2+, glibc 2.17+)"
[^musl-single]: musl, About musl, Ease of deployment — <https://musl.libc.org/about.html> — "An application can really be deployed as a single binary file and run on any machine with the appropriate instruction set architecture and Linux kernel or Linux syscall ABI emulation layer."
[^musl-no-deps]: musl, About musl, Ease of deployment — <https://musl.libc.org/about.html> — "Binaries statically linked with musl have no external dependencies, even for features like DNS lookups or character set conversions that are implemented with dynamic loading on glibc."
[^musl-small]: musl, About musl, Resource efficiency — <https://musl.libc.org/about.html> — "Minimal static-linked binaries can be under 10 kB of code, even with threads, and even useful programs can be under 50 kB."
[^musl-dlclose]: musl wiki, Functional differences from glibc, Unloading libraries — <https://wiki.musl-libc.org/functional-differences-from-glibc.html> — "musl's dynamic loader loads libraries permanently for the lifetime of the process, until it exits or calls exec. dlclose is a no-op."
[^musl-ver]: musl wiki, Functional differences from glibc, Symbol versioning — <https://wiki.musl-libc.org/functional-differences-from-glibc.html> — "the musl dynlinker only supports the subset of symbol versioning that allows to pick the default symbol version, instead of the oldest version, which comes first in the regular symbol tables."
[^glibc-stack]: musl wiki, Functional differences from glibc, Thread stack size — <https://wiki.musl-libc.org/functional-differences-from-glibc.html> — "The default stack size for new threads on glibc is determined based on the resource limit governing the main thread’s stack (RLIMIT_STACK). It generally ends up being 2-10 MB."
[^musl-stack]: musl wiki, Functional differences from glibc, Thread stack size — <https://wiki.musl-libc.org/functional-differences-from-glibc.html> — "musl provides a default thread stack size of 128k (80k prior to 1.1.21)."
[^musl-dns-parallel]: musl wiki, Functional differences from glibc, Name Resolver/DNS — <https://wiki.musl-libc.org/functional-differences-from-glibc.html> — "musl’s resolver queries them all in parallel and accepts whichever response arrives first."
[^musl-dns-tcp]: musl wiki, Functional differences from glibc, Name Resolver/DNS — <https://wiki.musl-libc.org/functional-differences-from-glibc.html> — "musl’s resolver did not support the use of DNS over TCP until version 1.2.4."
[^musl-dns-256]: musl wiki, Functional differences from glibc, Name Resolver/DNS — <https://wiki.musl-libc.org/functional-differences-from-glibc.html> — "search lines in musl are ignored if they exceed 256 chars."
[^musl-dns-ndots]: musl wiki, Functional differences from glibc, Name Resolver/DNS — <https://wiki.musl-libc.org/functional-differences-from-glibc.html> — "but those with at least as many dots as ndots are only tried in the global namespace (never falling back to search, which glibc would do if the name is not found in the global DNS namespace)."
[^rs-thread-stack]: Rust std 1.95.0, std::thread, Stack size — <https://doc.rust-lang.org/1.95.0/std/thread/index.html> — "The default stack size is platform-dependent and subject to change. Currently, it is 2 MiB on all Tier-1 platforms."
[^rs-crt-default]: Rust Reference 1.95.0, Linkage, Static and dynamic C runtimes — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "Typically targets are linked dynamically by default, but there are exceptions which are static by default such as:"
[^rs-x64-musl-crt]: rust-lang/rust tag 1.95.0, compiler/rustc_target/src/spec/targets/x86_64_unknown_linux_musl.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/compiler/rustc_target/src/spec/targets/x86_64_unknown_linux_musl.rs> — "FIXME(compiler-team#422): musl targets should be dynamically linked by default. base.crt_static_default = true;"
[^rs-a64-musl-crt]: rust-lang/rust tag 1.95.0, compiler/rustc_target/src/spec/targets/aarch64_unknown_linux_musl.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/compiler/rustc_target/src/spec/targets/aarch64_unknown_linux_musl.rs> — "FIXME(compiler-team#422): musl targets should be dynamically linked by default. base.crt_static_default = true;"
[^std-default-stack]: rust-lang/rust tag 1.95.0, library/std/src/sys/thread/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/thread/unix.rs> — "pub const DEFAULT_MIN_STACK_SIZE: usize = 2 * 1024 * 1024;"
[^std-setstacksize]: rust-lang/rust tag 1.95.0, library/std/src/sys/thread/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/thread/unix.rs> — "cmp::max(stack, min_stack_size(attr.as_ptr()))"
[^rs-crt-flag]: Rust Reference 1.95.0, Linkage, crt-static — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "The linkage of the C runtime is configured to respect the crt-static target feature."
[^rs-crt-ineffective]: Rust Reference 1.95.0, Linkage, Static and dynamic C runtimes — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "Targets which do not support switching between linkage of the C runtime will ignore this flag. It’s recommended to inspect the resulting binary to ensure that it’s linked as you would expect after the compiler succeeds."
[^musl-dlclose-ctor]: musl wiki, Functional differences from glibc, Unloading libraries — <https://wiki.musl-libc.org/functional-differences-from-glibc.html> — "Destructors will run on dlclose (as long as there are no remaining references) under glibc, and subsequent opens of the same library will run their constructors again. Under musl, constructors only run the first time a library is run, and destructors only run on exit."
[^rs-linker-cc]: rust-lang/rust tag 1.95.0, compiler/rustc_target/src/spec/mod.rs, LinkerFeatures — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/compiler/rustc_target/src/spec/mod.rs> — "Invoke the linker via a C/C++ compiler (e.g. on most unix targets)."
[^rs-link-type]: Rust Reference 1.95.0, Linkage — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "In one session of compilation, the compiler can generate multiple artifacts through the usage of either command line flags or the crate_type attribute. If one or more command line flags are specified, all crate_type attributes will be ignored in favor of only building the artifacts specified by command line."
[^cargo-crate-type]: Cargo Book 1.95.0, Cargo Targets, The crate-type field — <https://doc.rust-lang.org/1.95.0/cargo/reference/cargo-targets.html> — "The crate-type field defines the crate types that will be generated by the target. It is an array of strings, allowing you to specify multiple crate types for a single target. This can only be specified for libraries and examples."
[^rs-link-bin]: Rust Reference 1.95.0, Linkage — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "A runnable executable will be produced. This requires that there is a main function in the crate which will be run when the program begins executing. This will link in all Rust and native dependencies, producing a single distributable binary. This is the default crate type."
[^rs-link-lib]: Rust Reference 1.95.0, Linkage — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "The output library will always be usable by rustc, but the actual type of library may change from time-to-time."
[^rs-link-rlib]: Rust Reference 1.95.0, Linkage — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "These rlib files, unlike staticlib files, are interpreted by the compiler in future linkage. This essentially means that rustc will look for metadata in rlib files like it looks for metadata in dynamic libraries."
[^rs-link-dylib]: Rust Reference 1.95.0, Linkage — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "A dynamic Rust library will be produced. This is different from the lib output type in that this forces dynamic library generation."
[^rs-link-cdylib]: Rust Reference 1.95.0, Linkage — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "A dynamic system library will be produced. This is used when compiling a dynamic library to be loaded from another language."
[^rs-link-staticlib]: Rust Reference 1.95.0, Linkage — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "A static system library will be produced. This is different from other library outputs in that the compiler will never attempt to link to staticlib outputs."
[^rs-link-procmacro]: Rust Reference 1.95.0, Linkage — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "Crates compiled with this crate type must only export procedural macros. The compiler will automatically set the proc_macro configuration option. The crates are always compiled with the same target that the compiler itself was built with."
[^rs-dep-prefer]: Rust Reference 1.95.0, Linkage — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "If an executable is being produced and the -C prefer-dynamic flag is not specified, then dependencies are first attempted to be found in the rlib format."
[^rs-prefer-dynamic]: rustc book 1.95.0, Codegen options, prefer-dynamic — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "By default, rustc prefers to statically link dependencies."
[^rs-dep-staticlib]: Rust Reference 1.95.0, Linkage — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "If a static library is being produced, all upstream dependencies are required to be available in rlib formats. This requirement stems from the reason that a dynamic library cannot be converted into a static format."
[^rs-single-copy]: Rust Reference 1.95.0, Linkage — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "A major goal of the compiler is to ensure that a library never appears more than once in any artifact."
[^rs-crt-intro]: Rust Reference 1.95.0, Linkage, Static and dynamic C runtimes — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "The standard library in general strives to support both statically linked and dynamically linked C runtimes for targets as appropriate."
[^rs-musl-125]: rustc book 1.95.0, Platform Support — <https://doc.rust-lang.org/1.95.0/rustc/platform-support.html> — "x86_64-unknown-linux-musl 64-bit Linux with musl 1.2.5"
[^rs-rpath-off]: rustc book 1.95.0, Codegen options, rpath — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "n, no, off or false: disable rpath (the default)."
[^rs-rpath-on]: rustc book 1.95.0, Codegen options, rpath — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "If enabled, rustc will add output-relative (using @load_path on Mach-O and $ORIGIN on ELF respectively) rpaths to all dylib dependencies."
[^rs-strip-intro]: rustc book 1.95.0, Codegen options, strip — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "The option -C strip=val controls stripping of debuginfo and similar auxiliary data from binaries during linking."
[^rs-strip-values]: rustc book 1.95.0, Codegen options, strip — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "debuginfo - debuginfo sections and debuginfo symbols from the symbol table section are stripped at link time and are not copied to the produced binary. This should leave backtraces mostly-intact but may make using a debugger like gdb or lldb ineffectual."
[^rs-strip-warn]: rustc book 1.95.0, Codegen options, strip — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "Programs which may be combined with others, such as CLI pipelines and developer tooling, or even anything which wants crash-reporting, should usually avoid -Cstrip=symbols."
[^rs-strip-security]: rustc book 1.95.0, Codegen options, strip — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "-Cstrip cannot be relied on as a meaningful security or obfuscation measure, as disassemblers and decompilers can extract considerable information even in the absence of symbols."
[^cargo-strip]: Cargo Book 1.95.0, Profiles, strip — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "The strip option controls the -C strip flag, which directs rustc to strip either symbols or debuginfo from a binary."
[^rs-no-mangle]: Rust Reference 1.95.0, ABI, no_mangle — <https://doc.rust-lang.org/1.95.0/reference/abi.html> — "The no_mangle attribute may be used on any item to disable standard symbol name mangling. The symbol for the item will be the identifier of the item’s name."
[^rs-no-mangle-export]: Rust Reference 1.95.0, ABI, no_mangle — <https://doc.rust-lang.org/1.95.0/reference/abi.html> — "Additionally, the item will be publicly exported from the produced library or object file, similar to the used attribute."
[^rs-no-mangle-unsafe]: Rust Reference 1.95.0, ABI, no_mangle — <https://doc.rust-lang.org/1.95.0/reference/abi.html> — "This attribute is unsafe as an unmangled symbol may collide with another symbol with the same name (or with a well-known symbol), leading to undefined behavior."
[^rs-mangling-default]: rustc book 1.95.0, Codegen options, symbol-mangling-version — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "The default, if not specified, will use a compiler-chosen default which may change in the future."
[^rs-foreign-linkers]: Rust Reference 1.95.0, Linkage, Mixed Rust and foreign codebases — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "Use rustc. Pass any non-Rust libraries using -L \<directory> and -l\<library> rustc arguments, and/or #[link] directives in your Rust code. If you need to link against .o files you can use -Clink-arg=file.o."
[^rs-foreign-conflict]: Rust Reference 1.95.0, Linkage, Mixed Rust and foreign codebases — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "Multiple Rust staticlib files are likely to conflict. Passing rlibs directly into your foreign linker is currently unsupported."
[^rs-staticlib-care]: Rust Reference 1.95.0, Linkage, staticlib — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "Note that, because the resulting static library contains the code of all the dependencies, including the standard library, and also exports all public symbols of them, linking the static library into an executable or shared library may need special care."
[^rs-staticlib-native]: Rust Reference 1.95.0, Linkage, staticlib — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "will have to be specified manually when linking that static library from somewhere. The --print=native-static-libs flag may help with this."
[^rs-link-attr-dylib]: Rust Reference 1.95.0, External blocks, the link attribute — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "dylib — Indicates a dynamic library. This is the default if kind is not specified."
[^rs-link-attr-static]: Rust Reference 1.95.0, External blocks, the link attribute — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "static — Indicates a static library."
[^rs-unwind-prohibited]: Rust Reference 1.95.0, Linkage, Prohibited linkage and unwinding — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "If a Rust artifact is potentially unwinding, then all its crates must be built with the unwind panic strategy. Otherwise, unwinding can cause undefined behavior."
[^rs-unwind-dlopen]: Rust Reference 1.95.0, Linkage, Prohibited linkage and unwinding — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "Linking without rustc includes using dlopen or similar facilities where linking is done by the system runtime without rustc being involved."
[^musl-stack-ptgnu]: musl wiki, Functional differences from glibc, Thread stack size — <https://wiki.musl-libc.org/functional-differences-from-glibc.html> — "Since 1.1.21, musl supports increasing the default thread stack size via the PT_GNU_STACK program header, which can be set at link time via -Wl,-z,stack-size=N."
[^ldso-tokens-where]: ld.so(8), Dynamic string tokens — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "inside the values of the dynamic section tags DT_NEEDED, DT_RPATH, DT_RUNPATH, DT_AUDIT, and DT_DEPAUDIT of ELF binaries"
[^elf-raddend]: elf(5), r_addend — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This member specifies a constant addend used to compute the value to be stored into the relocatable field."
[^elf-shnum-xindex]: elf(5), e_shnum — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "If the number of entries in the section header table is larger than or equal to SHN_LORESERVE (0xff00), e_shnum holds the value zero and the real number of entries in the section header table is held in the sh_size member of the initial entry in section header table."
[^elf-shstrndx-xindex]: elf(5), e_shstrndx — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "If the index of section name string table section is larger than or equal to SHN_LORESERVE (0xff00), this member holds SHN_XINDEX (0xffff) and the real index of the section name string table section is held in the sh_link member of the initial entry in section header table."
[^elf-st-name]: elf(5), st_name — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This member holds an index into the object file's symbol string table, which holds character representations of the symbol names."
[^elf-st-value]: elf(5), st_value — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This member gives the value of the associated symbol."
[^elf-st-size]: elf(5), st_size — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "Many symbols have associated sizes. This member holds zero if the symbol has no size or an unknown size."
[^elf-st-info]: elf(5), st_info — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This member specifies the symbol's type and binding attributes:"
[^elf-st-other]: elf(5), st_other — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This member defines the symbol visibility."
[^elf-st-shndx]: elf(5), st_shndx — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This member holds the relevant section header table index."
[^gabi-soname]: System V gABI, ch.5, Dynamic Section, DT_SONAME — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch5.dynamic.html> — "This element holds the string table offset of a null-terminated string, giving the name of the shared object."
[^binfmt-interp-break]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "retval = elf_read(interpreter, interp_elf_ex, sizeof(*interp_elf_ex), 0); if (retval < 0) goto out_free_dentry; break;"
[^binfmt-elf-load]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "error = elf_load(bprm->file, load_bias + vaddr, elf_ppnt, elf_prot, elf_flags, total_size);"
[^binfmt-etexec-fixed]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "This logic is run once for the first LOAD Program Header for ET_EXEC binaries. No special handling is needed. */ elf_flags |= MAP_FIXED_NOREPLACE;"
[^psabi-plt-entry]: System V x86-64 psABI (gitlab commit e1ce0983, 2025-03-12), dl.tex, Procedure Linkage Table — <https://gitlab.com/x86-psABIs/x86-64-ABI/-/raw/e1ce098331da5dbd66e1ffc74162380bcc213236/x86-64-ABI/dl.tex> — ".PLT1: jmp *name1@GOTPCREL(%rip) # 16 bytes from .PLT0 pushq $index1 jmp .PLT0"
[^psabi-got]: System V x86-64 psABI (gitlab commit e1ce0983, 2025-03-12), dl.tex, Global Offset Table — <https://gitlab.com/x86-psABIs/x86-64-ABI/-/raw/e1ce098331da5dbd66e1ffc74162380bcc213236/x86-64-ABI/dl.tex> — "Global offset tables hold absolute addresses in private data, thus making the addresses available without compromising the position-independence and shareability of a program's text."
[^psabi-got-special]: System V x86-64 psABI (gitlab commit e1ce0983, 2025-03-12), dl.tex, Procedure Linkage Table — <https://gitlab.com/x86-psABIs/x86-64-ABI/-/raw/e1ce098331da5dbd66e1ffc74162380bcc213236/x86-64-ABI/dl.tex> — "When first creating the memory image of the program, the dynamic linker sets the second and the third entries in the global offset table to special values."
[^psabi-got-init]: System V x86-64 psABI (gitlab commit e1ce0983, 2025-03-12), dl.tex, Procedure Linkage Table — <https://gitlab.com/x86-psABIs/x86-64-ABI/-/raw/e1ce098331da5dbd66e1ffc74162380bcc213236/x86-64-ABI/dl.tex> — "Initially the global offset table holds the address of the following"
[^psabi-index]: System V x86-64 psABI (gitlab commit e1ce0983, 2025-03-12), dl.tex, Procedure Linkage Table — <https://gitlab.com/x86-psABIs/x86-64-ABI/-/raw/e1ce098331da5dbd66e1ffc74162380bcc213236/x86-64-ABI/dl.tex> — "Now the program pushes a relocation index"
[^psabi-plt0]: System V x86-64 psABI (gitlab commit e1ce0983, 2025-03-12), dl.tex, Procedure Linkage Table — <https://gitlab.com/x86-psABIs/x86-64-ABI/-/raw/e1ce098331da5dbd66e1ffc74162380bcc213236/x86-64-ABI/dl.tex> — "places the value of the second global offset table entry (GOT+8) on the stack, thus giving the dynamic linker one word of identifying information. The program then jumps to the address in the third global offset table entry (GOT+16), which transfers control to the dynamic linker."
[^psabi-subseq]: System V x86-64 psABI (gitlab commit e1ce0983, 2025-03-12), dl.tex, Procedure Linkage Table — <https://gitlab.com/x86-psABIs/x86-64-ABI/-/raw/e1ce098331da5dbd66e1ffc74162380bcc213236/x86-64-ABI/dl.tex> — "Subsequent executions of the procedure linkage table entry will transfer directly to"
[^glibc-ver-weak]: glibc 2.36, elf/dl-version.c (match_symbol) — <https://raw.githubusercontent.com/bminor/glibc/glibc-2.36/elf/dl-version.c> — "Symbol not found. If it was a weak reference it is not fatal."
[^glibc-ver-msg]: glibc 2.36, elf/dl-version.c (match_symbol) — <https://raw.githubusercontent.com/bminor/glibc/glibc-2.36/elf/dl-version.c> — "version `%s' not found (required by %s)"
[^musl-dlclose-glibc]: musl wiki, Functional differences from glibc, Unloading libraries — <https://wiki.musl-libc.org/functional-differences-from-glibc.html> — "This is very different from glibc's approach of reference counting libraries and unloading them when the count reaches zero."
[^musl-dns-seq]: musl wiki, Functional differences from glibc, Name Resolver/DNS — <https://wiki.musl-libc.org/functional-differences-from-glibc.html> — "Traditional resolvers, including glibc's, make use of multiple nameserver lines in resolv.conf by trying each one in sequence and falling to the next after one times out."
[^k8s-resolv]: Kubernetes docs, DNS for Services and Pods, Namespaces of Services — <https://kubernetes.io/docs/concepts/services-networking/dns-pod-service/> — "DNS queries may be expanded using the Pod's /etc/resolv.conf. kubelet configures this file for each Pod. For example, a query for just data may be expanded to data.test.svc.cluster.local. The values of the search option are used to expand queries."
[^k8s-ndots]: Kubernetes docs, DNS for Services and Pods, Namespaces of Services (ví dụ resolv.conf) — <https://kubernetes.io/docs/concepts/services-networking/dns-pod-service/> — "nameserver 10.32.0.10 search \<namespace>.svc.cluster.local svc.cluster.local cluster.local options ndots:5"
[^rs-staticlib-limit]: Rust Reference 1.95.0, Linkage, staticlib — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "In case of a shared library the list of exported symbols will have to be limited via e.g. a linker or symbol version script, exported symbols list (macOS), or module definition file (Windows). Additionally, unused sections can be removed to remove all code of dependencies that is not actually used (e.g. --gc-sections or -dead_strip for macOS)."
