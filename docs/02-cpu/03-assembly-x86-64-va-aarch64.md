# P2 · Chương 03 — Assembly x86-64 và AArch64

## Bạn sẽ hiểu được gì

1. **Q1.** Hai cú pháp AT&T / Intel khác nhau ở đâu? → mục 1
2. **Q2.** `rax`, `eax`, `ax`, `al` là gì của nhau; `x0` và `w0` là gì của nhau; `xzr`/`sp` đặc biệt ra sao? → mục 2
3. **Q3.** Lệnh x86-64 và A64 khác nhau thế nào về thứ tự toán hạng, truy cập bộ nhớ (load/store) và addressing mode? → mục 3
4. **Q4.** Cờ điều kiện (condition flags) là gì, `cmp` + nhảy có điều kiện hoạt động ra sao, và `cmov`/`csel` thay thế nhánh khi nào? → mục 4
5. **Q5.** `if`, `while`, `for`, `match` được compiler dịch thành khung nhánh nào? → mục 5
6. **Q6.** `call`/`bl`/`ret` làm gì với thanh ghi và stack ở mức tối thiểu? → mục 6
7. **Q7.** Cách tự sinh và đọc assembly do `rustc` tạo ra (`--emit=asm`), lọc nhiễu directive, nhận ra bounds check, overflow check, vector hoá? → mục 7

## Kiến thức cần trước

- [Chương 01 — Mô hình máy và ISA](01-mo-hinh-may-va-isa.md): ISA là gì, thanh ghi, chu trình fetch–decode–execute.
- [Chương 02 — Biểu diễn dữ liệu](02-bieu-dien-du-lieu.md): số nguyên có dấu/không dấu, two's complement, endianness, zero-/sign-extension.

Bạn đã viết service bằng Rust nhưng chưa cần đọc assembly bao giờ. Chương này bám theo một service giả định tên `orders` (nhận xét của người soạn: ví dụ giả định, không phải code trong repo). Giả sử `orders` đang chậm, flamegraph chỉ vào một hàm nóng, và bạn muốn biết compiler thực sự cho CPU chạy những lệnh gì. Muốn trả lời thì phải đọc được assembly, và chương này dạy đúng phần đó. Các hàm ví dụ trong chương có tên trung tính (`load_idx`, `max`, `sw`…) để asm ngắn gọn; ta chỉ tưởng tượng mỗi hàm là hình dạng của một việc quen thuộc trong `orders`.

Quy ước gọi hàm, stack frame, căn lề stack và tail call ở [Chương 04](04-calling-convention-va-stack-frame.md); pipeline và giá của nhánh đoán sai ở [Chương 05](05-pipeline-va-speculation.md); `opt-level` và bounds check được đào sâu hơn ở [Chương 07](07-rust-va-may.md) (mục 1 và 3).

## 1. Cú pháp AT&T và Intel (x86-64) (Q1)

> **Trả lời ngắn.** Cùng một lệnh x86-64 có hai cách viết: AT&T ghi nguồn trước, đích sau, kèm `$` cho hằng và `%` cho thanh ghi; Intel ghi đích trước. Hai cú pháp dùng thứ tự nguồn và đích ngược nhau[^gasorder]. `rustc --emit=asm` in AT&T (quan sát), còn `asm!` viết tay trong Rust mặc định dùng Intel[^rustasm]. Với A64, mọi tài liệu và output trong chương này đều viết đích trước[^armops].

**Giải thích.**

*Với service `orders`.* Bạn mở assembly của một hàm trong `orders` và dòng đầu tiên đã đặt ra một câu hỏi: lệnh này lấy từ đâu, ghi vào đâu? Câu trả lời tùy tool đang in ra cú pháp nào, và đọc ngược toán hạng sẽ khiến bạn hiểu ngược cả điều kiện của một nhánh. Mục này cho bạn bảng đối chiếu hai cú pháp để không nhầm.

### 1.1 Hai cách viết một lệnh

Hai cú pháp mô tả cùng một ISA, chỉ khác cách ghi. Bảng dưới gom các khác biệt chính:

| Điểm | AT&T | Intel |
|---|---|---|
| Thứ tự toán hạng | nguồn, đích[^gasorder] | đích, nguồn |
| Hằng (immediate) | `$4`[^gasdiff] | `4` |
| Thanh ghi | `%rax`[^gasdiff] | `rax` |
| Kích thước toán hạng bộ nhớ | hậu tố mnemonic `b/w/l/q`[^gassize] | tiền tố `byte/word/dword/qword ptr`[^gasptr] |
| Tham chiếu bộ nhớ | `disp(base,index,scale)` | `[base + index*scale + disp]`[^gasscale] |

Thử với `load_idx(p: &[i32], i: usize) -> i32 { p[i] }`, hàm lấy phần tử thứ `i` của một mảng, chẳng hạn giá của dòng hàng thứ `i` trong một đơn. Dưới đây là đường nhanh của nó ở hai cú pháp (đã rút gọn, bỏ nhánh panic):

```asm
# AT&T (mặc định)                 # Intel (-x86-asm-syntax=intel)
load_idx:                         load_idx:
    cmpq   %rsi, %rdx                 cmp    rdx, rsi
    jae    .LBB5_2                    jae    .LBB5_2
    movl   (%rdi,%rdx,4), %eax        mov    eax, dword ptr [rdi + 4*rdx]
    retq                              ret
```

Hai dòng `movl` và `mov … dword ptr` là cùng một lệnh máy. Chỗ dễ nhầm nhất là `cmpq %rsi, %rdx`: vì AT&T viết nguồn trước, lệnh này so `rdx` với `rsi`, tức `i` với `len`, ngược với thứ tự đọc tự nhiên. Quên điều này khi đọc `jae` ngay sau đó là hiểu sai điều kiện (nhận xét của người soạn; quy tắc của `cmp` nằm ở mục 4). Phần x86-64 của chương dùng AT&T, vì đó là thứ `rustc --emit=asm` in ra (quan sát) và cũng là mặc định của `gcc`[^gccmasm]. Phần AArch64 viết đích trước như đã nói ở đầu mục.

### 1.2 Cú pháp mặc định phụ thuộc tool

File `.s` do rustc sinh cho x86-64 là AT&T (quan sát ở mọi output dưới đây: `%rax`, `$1`). LLVM chọn AT&T làm mặc định cho x86, và cho đổi bằng tùy chọn `x86-asm-syntax`[^llvmasmsyntax]. Với `asm!` viết tay trong Rust thì ngược lại: trên x86, GAS chạy ở chế độ `.intel_syntax noprefix` theo mặc định[^rustasm]. Option `att_syntax` mới chuyển sang AT&T[^rustatt]. Nên bạn sẽ gặp cả hai trong thực tế.

Muốn ép `--emit=asm` ra Intel, tôi đã chạy thật `rustc -C llvm-args=-x86-asm-syntax=intel`. `-C llvm-args` chuyển thẳng tham số cho LLVM nên không thuộc bảo đảm ổn định của CLI rustc, có thể đổi[^llvmargs].

<details><summary>Đi sâu: trích GNU as về thứ tự, kích thước và scale</summary>

Ví dụ của chính tài liệu GNU as: Intel `add eax, 4` là `addl $4, %eax` trong AT&T[^gasorder]. Về kích thước toán hạng bộ nhớ, AT&T lấy từ ký tự cuối của mnemonic[^gassize], nên output có `movq`, `addl`, `cmpq`. Intel thì đặt `qword ptr` (hay `byte`/`word`/`dword ptr`) trước toán hạng bộ nhớ[^gasptr]. Khi GAS dịch tham chiếu bộ nhớ Intel `section:[base + index*scale + disp]` sang AT&T `section:disp(base, index, scale)`, `scale` nhận giá trị 1, 2, 4 hoặc 8 và nhân với `index` để ra địa chỉ[^gasscale].

</details>

**Quay lại Q1:** với `orders`, AT&T là nguồn-đích kèm `$`/`%` và hậu tố kích thước, Intel là đích-nguồn; cùng một lệnh máy, chỉ khác cách ghi.

## 2. Thanh ghi: `rax`/`eax`/`ax`/`al`, `x0`/`w0`, `xzr`/`sp` (Q2)

> **Trả lời ngắn.** Trên x86-64, `rax`, `eax`, `ax`, `al` là bốn cỡ 64, 32, 16 và 8 bit của cùng một thanh ghi[^x86sub]. A64 có `x0`–`x30` 64 bit, và `w0`–`w30` là tên dùng cho ngữ cảnh 32 bit[^aapcsregs]. Có một quy tắc dễ gây nhầm: ghi vào phần 32 bit trên x86-64 thì nửa trên bị xoá về 0[^sdm32]. Ghi 8 hoặc 16 bit thì không[^sdm816]. Còn `xzr` luôn đọc ra 0 và mọi lần ghi vào nó bị bỏ qua[^armzr]; nó dùng chung số hiệu 31 với `sp`, và lệnh nào hiểu số 31 là cái nào thì tùy lệnh[^armr31].

**Giải thích.**

*Với service `orders`.* Hàm nóng của `orders` thường làm việc với số nhỏ: một mã trạng thái 1 byte (`u8`), một chỉ số 32 bit, một số tiền `i64`. Mỗi cỡ có tên thanh ghi riêng, nên đọc output là phải biết `%dil` hay `%eax` đang nói về phần nào của thanh ghi nào, và ghi vào phần đó thì phần còn lại ra sao. [Chương 01](01-mo-hinh-may-va-isa.md) (mục 3, phần Đi sâu) đã điểm qua số lượng thanh ghi, thanh ghi con và quy tắc ghi 32 bit; mục này đi vào chi tiết từng ISA kèm output thật.

### 2.1 x86-64

x86-64 có 16 thanh ghi 64 bit: `%rax, %rbx, %rcx, %rdx, %rdi, %rsi, %rsp, %rbp` và `%r8`–`%r15`[^x86regs]. Intel SDM cũng đếm cả 16 là thanh ghi đa dụng ở 64-bit mode[^sdm16]. Trừ `%rsp` và `%rbp`, các thanh ghi còn lại chứa dữ liệu 64 bit đa dụng[^x86gp]; `%rsp` và `%rbp` là con trỏ stack và con trỏ frame[^x86sp]. Ngoài ra còn `%rip` (instruction pointer), chương trình không ghi trực tiếp vào nó được[^x86rip].

Mỗi thanh ghi có tên riêng cho từng phần thấp của nó (sub-register). Với tám thanh ghi đầu (`%rax`…`%rbp`), đổi `r` trong tên thành `e` để chọn 32 bit thấp (`%eax`), và lấy hai chữ cuối của tên để chọn 16 bit thấp (`%ax`)[^x86sub]. Byte thấp của bốn thanh ghi đầu có tên kết thúc bằng `l`, như `%al`[^x86low8]; các tên như `%dil`, `%sil` có trong bảng dưới. Với `%r8`–`%r15` thì hậu tố `d`, `w`, `b` chọn 32, 16, 8 bit thấp: `%r9d` là 32 bit thấp của `%r9`[^x86r8].

| 64-bit | 32-bit | 16-bit | 8-bit thấp |
|---|---|---|---|
| `%rax` | `%eax` | `%ax` | `%al` |
| `%rdi` | `%edi` | `%di` | `%dil` |
| `%r9` | `%r9d` | `%r9w` | `%r9b` |

Bây giờ đến quy tắc dễ gây nhầm: **ghi 32 bit xoá nửa trên, ghi 8 hay 16 bit thì không.** Ở 64-bit mode, toán hạng 8 và 16 bit cho kết quả 8 hay 16 bit, và 56 hay 48 bit cao của thanh ghi đích không bị phép toán sửa đổi[^sdm816]. Toán hạng 32 bit thì khác: kết quả 32 bit được zero-extend thành 64 bit trong thanh ghi đích[^sdm32]. Đổi mã trạng thái `u8` của một đơn thành `u32` cho thấy hệ quả ngay trong output:

```asm
byte32:                 # fn byte32(x: u8) -> u32 { x as u32 }
    movzbl %dil, %eax   # đọc %dil (8 bit), ghi %eax (32 bit) => %rax cao = 0
    retq
```

Ghi `%eax` là đủ để `rax` mang giá trị `u32` đã zero-extend, hệ quả trực tiếp của quy tắc 32 bit[^sdm32]. Cùng lý do, `xorl %eax, %eax` (gặp ở `sw`, mục 5) đặt `rax = 0` (suy luận của người soạn từ quy tắc 32 bit).

### 2.2 AArch64

AArch64 (ARMv8-A, tập lệnh A64) có 31 thanh ghi đa dụng 64 bit `x0`–`x30`[^armregs]. Khi cần làm việc với 32 bit thì dùng tên `w0`–`w30`[^aapcsregs]; đó là nửa thấp 32 bit của chính các thanh ghi ấy. Ghi vào như một word 32 bit thì nửa trên của thanh ghi bị xoá[^armzeroupper]. Dive into Systems nói tương tự khi dữ liệu 32 bit nằm trong `w0`[^armw]. Vậy A64 cùng hành vi với x86-64 khi ghi 32 bit.

Trong output dưới đây, chỗ x86-64 dùng `movzbl`, A64 dùng `and w0, w0, #0xff` trên `w0` (quan sát ở `byte32`). Tôi đã chạy thật trên máy `aarch64-apple-darwin`: sau `mov x0, #-1` rồi `mov w0, #1`, `x0` đọc ra `0x1`.

```asm
byte32:                 // aarch64: fn byte32(x: u8) -> u32 { x as u32 }
    and  w0, w0, #0xff  // giữ 8 bit thấp; ghi w0 => x0 cao = 0
    ret
```

Ngoài `x0`–`x30` còn vài thanh ghi có vai trò riêng:

| Tên | Vai trò | Nguồn |
|---|---|---|
| `sp` | stack pointer; chỉ dùng được trong một số ít lệnh | [^aapcssp] |
| `pc` | program counter; chương trình không ghi trực tiếp | [^armpc] |
| `xzr` / `wzr` (DiS gọi "zr") | luôn đọc ra 0, ghi vào bị bỏ qua | [^armzr] |
| `x30` = `lr` | link register: chứa địa chỉ trả về của `bl` | [^armbl] |
| `x29` = `fp` | frame pointer | [^aapcsfp] |

Dive into Systems viết thanh ghi zero "chỉ hữu ích làm thanh ghi nguồn"[^armsp]. Nhưng Arm nói rõ ghi vào nó bị bỏ qua, và `cmp` được mã hoá thành `sub` có đích là thanh ghi zero[^armzr]. Vậy dùng nó làm đích để vứt kết quả cũng là cách dùng thật (mục 4 có ví dụ).

<details><summary>Đi sâu: tên <code>dil</code>, quy ước gọi hàm, và thí nghiệm <code>xzr</code>/<code>sp</code> trong LLVM</summary>

**Tên `%dil`.** Bảng tên thanh ghi ở 64-bit mode của SDM liệt kê `DIL`, `SIL`, `BPL`, `SPL`, `R8B`–`R15B` ở cột có tiền tố REX[^sdmbyte].

**Gắn với calling convention.** Sáu tham số đầu của một hàm được nạp vào các thanh ghi `%rdi`, `%rsi`, `%rdx`, `%rcx`, `%r8` và `%r9`[^x86args]. Giá trị trả về nằm ở `%rax` hoặc một thanh ghi con của nó như `%eax`[^x86ret]. Từ đó `fn add(a: i64, b: i64)` đọc `%rdi`, `%rsi` và trả về ở `%rax` (suy luận của người soạn từ hai câu trên; chi tiết ở [Chương 04](04-calling-convention-va-stack-frame.md)). Lưu ý: hàm Rust khai báo không có `extern` dùng ABI "Rust", calling convention gốc cho hàm và closure Rust, và ABI này không đưa ra bảo đảm ổn định nào[^rustabi]. Việc nó trùng với quy ước C trong các ví dụ ở đây là quan sát, không phải bảo đảm.

**`xzr` và `sp` chung số 31.** Trong phần mã hoá, `xzr` và `sp` dùng chung một số hiệu thanh ghi (31). Chỉ một số ít lệnh hiểu số đó là `sp`, còn lại coi nó là thanh ghi zero[^armr31]. Các lệnh hiểu là `sp` gồm: mọi lệnh load/store (làm thanh ghi base), `and`/`orr`/`eor` dạng immediate không đặt cờ (làm đích), và `add`/`sub` dạng immediate hoặc dạng extend (làm đích hoặc toán hạng nguồn đầu)[^armspinstr]. Phía LLVM, cả hai thanh ghi được khai báo với số hiệu 31[^llvmreg31]. LLVM chọn giữa chúng bằng lớp thanh ghi mà từng toán hạng của lệnh chấp nhận: `GPR64` là `x0`–`x30` cộng `XZR`[^llvmgpr64]. Còn `GPR64sp` là `x0`–`x30` cộng `SP`[^llvmgpr].

Tôi đã thử hệ quả bằng `asm!` (rustc 1.95.0, `aarch64-apple-darwin`): `add x8, sp, #0` được assembler chấp nhận (ở dạng immediate, số 31 của toán hạng nguồn là `sp`), `add x8, xzr, #7` bị từ chối "invalid operand", còn `add x8, xzr, x1` chạy và cho `7` khi `x1 = 7` (dạng thanh ghi, số 31 là `xzr`); `adds xzr, x8, #1` chạy được vì đích `xzr` bỏ kết quả. Kết quả khớp mô tả của Arm ở trên và việc `cmp reg, #0` thực chất là `subs xzr, reg, #0`[^llvmcmp] (mục 4). Đây là hành vi của assembler LLVM kết hợp tài liệu Arm; đặc tả đầy đủ nằm ở Arm ARM, trang developer.arm.com trả 403 lúc soạn.

</details>

**Quay lại Q2:** với `orders`, sub-register là các cỡ của cùng một thanh ghi (ghi 32 bit xoá nửa trên, ghi 8/16 bit thì không); `xzr` đọc ra 0 (ghi vào bị bỏ qua) và dùng chung số 31 với `sp`.

## 3. Lệnh cơ bản, load/store và addressing mode (Q3)

> **Trả lời ngắn.** x86-64 chủ yếu là lệnh hai toán hạng, và đích cũng là một nguồn[^x86sd]. Một lệnh không được dùng bộ nhớ vừa làm nguồn vừa làm đích[^x86mem]. A64 thì phần lớn là lệnh ba toán hạng (đích, nguồn 1, nguồn 2)[^armops], theo mô hình load/store: dữ liệu phải nằm trong thanh ghi, và chỉ các lệnh load/store chuyên dụng như `ldr`/`str` mới chạm bộ nhớ[^ldst]. Địa chỉ bộ nhớ x86-64 tính theo công thức Base + Index×Scale + Displacement[^sdmea].

**Giải thích.**

*Với service `orders`.* Hầu như hàm nóng nào của `orders` cũng làm hai việc: tính toán trên vài số, và đọc ghi mảng hay struct trong bộ nhớ. Hai ISA chia hai việc này rất khác nhau, nên cùng một hàm Rust cho hai chuỗi lệnh có hình dạng khác hẳn. Mục này đi lần lượt: cách ghi toán hạng của lệnh số học, cách chạm bộ nhớ, rồi các dạng địa chỉ.

### 3.1 Lệnh số học: hai toán hạng hay ba

Lệnh hai toán hạng của x86-64 thường theo định dạng nguồn, đích (S, D) ở AT&T[^x86sd]. Với lệnh số học thì đích cũng là một nguồn: `add S, D` tính `S + D → D`[^x86add]. A64 thì hầu hết lệnh có dạng `opcode D, O1, O2`, với `D` là thanh ghi đích. Ví dụ `add w0, w0, #0x2` có đích là `w0` và hai toán hạng là `w0` và `#0x2`[^armops].

Hệ quả (nhận xét của người soạn): x86-64 chủ yếu là lệnh **hai** toán hạng, đích cũng là nguồn (`addq %rsi, %rdi` nghĩa là `rdi += rsi`), còn A64 thường là **ba** toán hạng nên đích không bắt buộc trùng nguồn (như `add x8, x1, x0` ở [Chương 01, mục 6](01-mo-hinh-may-va-isa.md)). Hàm cộng hai khoản tiền `add(a: i64, b: i64)` cho thấy rõ:

```asm
# x86-64 (AT&T)               # AArch64
add:                          _add:
    leaq (%rdi,%rsi), %rax        add  x0, x1, x0
    retq                          ret
```

Ở A64, compiler chọn đích `x0` vì không cần giữ lại `a`, `b`. Ở x86-64, nó dùng `lea` thay `add` để có đích khác nguồn (suy luận của người soạn). `lea` không tra cứu bộ nhớ[^lea]: nó chỉ tính `rdi + rsi` rồi đặt kết quả vào thanh ghi.

### 3.2 Chạm bộ nhớ: load/store và `mov`

A64 theo mô hình load/store: dữ liệu không thể đọc hay ghi thẳng vào bộ nhớ mà phải xử lý trong thanh ghi[^ldst]. Vì vậy `add`, `cmp`… chỉ làm trên thanh ghi, còn việc chạm RAM do các lệnh load/store chuyên dụng lo: `ldr` (load), `str` (store), và `ldp`/`stp` nạp hay lưu một cặp thanh ghi 64 bit, thay cho load/store-multiple và push/pop của ARM cũ[^armpushpop].

x86-64 thì khác: `mov` vừa nạp vừa lưu, và nhiều lệnh số học nhận được một toán hạng bộ nhớ (`addq 8(%rdi), %rax`). Giới hạn duy nhất là bộ nhớ không thể vừa là nguồn vừa là đích trong cùng một lệnh[^x86mem]. Cặp hàm dưới đây cộng hai bộ đếm trong một mảng 4 phần tử `p: &[u64;4]` (rút gọn):

```asm
# fn ld(p: &[u64;4]) -> u64 { p[1] + p[3] }
# x86-64                          # AArch64
ld:                               _ld:
    movq 24(%rdi), %rax               ldr  x8, [x0, #8]
    addq 8(%rdi), %rax                ldr  x9, [x0, #24]
    retq                              add  x0, x9, x8
                                      ret
```

Cùng một việc, nhưng x86-64 gộp load vào `addq` còn A64 tách thành `ldr` rồi `add`. Offset `8` và `24` tính bằng byte (`1*8`, `3*8`). Một lệnh ghi tương ứng (`p[1] = v`) sẽ là `movq %rsi, 8(%rdi)` / `str x1, [x0, #8]` (ví dụ minh hoạ của người soạn, không phải output đã dán).

### 3.3 Addressing mode

Một toán hạng bộ nhớ x86-64 như `-0x4(%rbp)` nghĩa là cộng -0x4 vào giá trị trong `%rbp` rồi tra bộ nhớ[^x86memform]. Với A64, `[sp, #12]` nghĩa là cộng 12 vào giá trị trong `sp` rồi tra bộ nhớ[^armmemform]. Dạng tổng quát của x86-64 là `Offset = Base + (Index * Scale) + Displacement`[^sdmea]. Trong đó Scale nhận giá trị 1, 2, 4 hoặc 8[^gasscale].

Bảng dưới đối chiếu các dạng gặp thật trong output. Hai ô "cần lệnh riêng" và "(hai lệnh)" là nhận xét của người soạn, không phải khẳng định của nguồn.

| Ý nghĩa | x86-64 (AT&T) | AArch64 |
|---|---|---|
| `*(r)` | `(%rdi)` | `[x0]` |
| `*(r + 8)` | `8(%rdi)` | `[x0, #8]` |
| `*(r + i*4)` | `(%rdi,%rdx,4)` | `[x0, x2, lsl #2]` |
| `*(r + zext(w)*4)` | cần lệnh riêng | `[x8, w0, uxtw #2]` |
| `*r` rồi `r += 8` | (hai lệnh) | `ldr x11, [x10], #8` (post-index) |
| `push`/`pop`-kiểu | `pushq` / `popq` | `stp x29, x30, [sp, #-16]!` (pre-index), `ldp x29, x30, [sp], #16` (post-index) |

<details><summary>Đi sâu: thí nghiệm pre-/post-index và <code>uxtw</code>/<code>sxtw</code>, địa chỉ position-independent</summary>

Các dạng `[x0, x2, lsl #2]`, `[x0, w3, SXTW]` có trong bảng toán hạng của Dive into Systems cho ARM64[^armscaled]. Dạng `uxtw` là bản zero-extend của `sxtw` (suy luận của người soạn từ tên lệnh). Tôi đã chạy `ldr x, [base, w, uxtw #3]` với `w = 0x80000000`: nó đọc đúng phần tử ở `base + 0x4_0000_0000` (zero-extend, không phải âm), còn `sxtw` với cùng `w` cho `base − 0x4_0000_0000`.

Với pre-index (`[x, #imm]!`) và post-index (`[x], #imm`), LLVM mô tả chế độ chung của hai kiểu. Ở pre-index, địa chỉ truy cập là con trỏ base cộng hoặc trừ offset[^llvmpre]. Ở post-index, địa chỉ truy cập chính là base pointer, còn offset được cộng vào base sau khi truy cập bộ nhớ[^llvmpost]. Với `ldr` ở A64, cả hai biến thể có thêm một kết quả ghi lại (`$wback`) buộc vào thanh ghi base `$Rn`[^llvmwb]. Đọc vào output: `ldr x11, [x10], #8` đọc từ `x10` rồi tăng `x10` thêm 8, chính là bước `p++` của vòng cộng mảng `u64`; `stp x29, x30, [sp, #-16]!` trừ `sp` 16 rồi mới ghi cặp. Tôi đã chạy thật: `ldr x, [p], #8` đọc `10` và `p` tăng 8 byte; `ldr y, [p, #8]!` đọc `20` (phần tử kế) và `p` tăng 8 byte.

Cặp `adrp`+`add … @PAGEOFF` (macOS, A64) và `leaq sym(%rip)`/`@GOTPCREL(%rip)` (Linux, x86-64) là cách lấy địa chỉ symbol theo kiểu tương đối (quan sát trong output của chương). Chương này chỉ cần bạn nhận ra chúng; cơ chế GOT/relocation nằm ở [P11·06](../11-do-dac-va-go-loi/06-elf-linker-loader.md).

</details>

**Quay lại Q3:** với `orders`, x86-64 hai toán hạng và cho toán hạng bộ nhớ ngay trong lệnh số học; A64 thường ba toán hạng và chỉ các lệnh load/store (như `ldr`/`str`) chạm RAM.

## 4. Condition flags, nhảy có điều kiện và `cmov`/`csel` (Q4)

> **Trả lời ngắn.** `cmp` và `test` không ghi kết quả vào thanh ghi mà đặt các cờ điều kiện[^x86cmp]. Lệnh nhảy có điều kiện đọc các cờ đó[^sdmflags]. Hậu tố của lệnh nhảy quyết định so sánh có dấu hay không dấu[^jsuffix]. `cmov`/`csel` chọn giá trị mà không nhảy, nhờ vậy không làm gián đoạn pipeline như một nhánh[^brexp]. Nhưng compiler thận trọng khi hai nhánh có tác dụng phụ[^cmovcare], và `cmov` không phải lúc nào cũng nhanh hơn nhánh ([Chương 05, mục 7](05-pipeline-va-speculation.md)).

**Giải thích.**

*Với service `orders`.* Hàm nóng của `orders` đầy `if`: số dư đủ không, retry hết lượt chưa, id có nằm trong mảng không. Compiler dịch mỗi `if` thành hai bước: một lệnh so sánh để đặt cờ, rồi một lệnh rẽ hoặc chọn đọc cờ đó. Hiểu hai bước này là đọc được hầu hết nhánh trong asm.

### 4.1 Cờ điều kiện

Cả hai họ kiến trúc đều có thanh ghi cờ: lệnh so sánh hoặc số học ghi vào, lệnh nhảy hoặc chọn đọc ra. Trên x86-64, `cmp` và `test` không sửa thanh ghi đích; chúng sửa một loạt giá trị một bit gọi là condition code flags (cờ điều kiện)[^x86cmp]. Các cờ này nằm trong FLAGS register[^x86flagsreg]. Các lệnh Jcc dùng trạng thái của cờ trạng thái trong thanh ghi EFLAGS làm điều kiện để rẽ nhánh[^sdmflags]. Trên A64, cờ nằm trong PSTATE, trạng thái bộ xử lý ARM[^pstate]. Lệnh `cmp`, `tst` và biến thể `s` của lệnh số học (`adds`, `subs`) cập nhật chúng.

Về cấu tạo lệnh, `cmp` chỉ là tên khác của `subs`, còn `tst` là tên khác của `ands`, cả hai với đích là thanh ghi zero để bỏ kết quả. Arm nói `CMP` được mã hoá thành `SUB` có đích là thanh ghi zero[^armzr]. LLVM ghi cụ thể `cmp reg, #0` là `subs xzr, reg, #0`[^llvmcmp]. Còn `tst` thì alias của LLVM biến nó thành `ands` với đích `xzr`/`wzr`[^llvmtst].

Bốn cờ cần nhớ, với tên ở hai ISA:

| Cờ | x86-64[^x86flagtab] | AArch64[^armflagtab] | Dùng cho |
|---|---|---|---|
| Bằng/không | ZF | Z | `==`, `!=` |
| Âm (dấu) | SF | N | so sánh có dấu (cùng OF/V) |
| Tràn có dấu | OF | V | so sánh có dấu |
| Nhớ (carry) | CF (sau phép trừ: 1 khi *có* borrow) | C (sau phép trừ: 0 khi *có* borrow) | so sánh không dấu |

Cờ nào dùng cho so sánh nào? Trên x86, SF và OF dùng cho so sánh số nguyên có dấu, còn CF dùng cho so sánh không dấu[^x86flagsuse]. A64 tương tự: V hữu ích cho so sánh có dấu, C hữu ích cho so sánh không dấu[^armflags].

Thứ tự toán hạng của `cmp` là chỗ hay nhầm nhất. `cmp O1, O2` của A64 tính O1 - O2[^armcmp]. Còn `cmp R1, R2` của x86 trừ R1 khỏi R2, tức tính R2 - R1[^x86cmpord]. Vì vậy `cmpq %rsi, %rdx` ở mục 1 tính `rdx - rsi`, tức `i - len`.

**Carry khi trừ: hai ISA ngược nhau.** Trên x86-64, CF được đặt khi phép tính số học tạo một carry hoặc borrow ra khỏi bit cao nhất[^sdmcf]. Nên sau một phép trừ `a - b` (như `cmp`) thì CF=1 khi `a < b` không dấu; `jae` nhảy khi CF=0 và `jb` nhảy khi CF=1[^sdmjae] (suy luận của người soạn từ định nghĩa borrow). Trên A64 thì ngược lại: source LLVM ghi điều kiện `HS` ứng với C == 1 và `LO` ứng với C == 0[^llvmnzcv]. Nói cách khác, sau phép trừ không có borrow thì C = 1. `HS` là "higher or same", cao hơn hoặc bằng, không dấu[^llvmcc].

### 4.2 Hậu tố điều kiện

Lệnh nhảy x86 bắt đầu bằng `j`; hậu tố của mỗi lệnh cho biết điều kiện nhảy, và cũng quyết định so sánh được hiểu là có dấu hay không dấu[^jsuffix]. A64 tương tự với `b.` cộng hậu tố: hậu tố cũng quyết định có dấu hay không dấu, và cùng bộ hậu tố ấy dùng cho `csel`[^bsuffix].

| Điều kiện | x86[^jsufftab] | A64[^bsufftab] | Dấu |
|---|---|---|---|
| `==` | `je` | `b.eq` | — |
| `!=` | `jne` | `b.ne` | — |
| `<` có dấu | `jl` | `b.lt` | signed |
| `>` có dấu | `jg` | `b.gt` | signed |
| `>=` unsigned | `jae` | `b.hs` (còn gọi `b.cs`)[^llvmalias] | unsigned |
| `>` unsigned | `ja` | `b.hi` | unsigned |

### 4.3 Chọn thay vì nhảy: `cmov` và `csel`

Khi `if` chỉ chọn giữa hai giá trị và không có tác dụng phụ, LLVM sinh lệnh chọn thay vì nhảy (quan sát ở output dưới). Hàm lấy giá trị lớn hơn của hai số đo, `max(a: i64, b: i64)`, là ví dụ:

```asm
# fn max(a: i64, b: i64) -> i64 { if a > b { a } else { b } }
max:                          _max:                    # AArch64
    movq  %rsi, %rax              cmp   x0, x1
    cmpq  %rsi, %rdi              csel  x0, x0, x1, gt   // x0 = (gt) ? x0 : x1
    cmovgq %rdi, %rax             ret
    retq
```

`cmov` thực hiện một phép chuyển dữ liệu có điều kiện: cả then lẫn else đều được thực thi, và dữ liệu được đặt vào thanh ghi phù hợp[^cmov]. `csel` làm tương tự trên A64[^csel]. Lý do dùng chúng là nhánh thường rất tốn kém khi chạy, vì có thể làm gián đoạn instruction pipeline[^brexp]. Cơ chế đó được giải thích kỹ ở [Chương 05](05-pipeline-va-speculation.md), kể cả vì sao branchless không phải lúc nào cũng nhanh hơn (mục 7 của chương đó).

Ngược lại, compiler rất thận trọng khi tối ưu lệnh nhảy thành `cmov`, nhất là khi có tác dụng phụ hoặc giá trị con trỏ[^cmovcare]. Ví dụ `br` ở mục 5 gọi hai hàm khác nhau, có tác dụng phụ, nên giữ nhánh (nhận xét của người soạn).

<details><summary>Đi sâu: <code>collatz</code> với <code>csinc</code>, carry khi trừ, bảng điều kiện đầy đủ, <code>cbz</code> và tầm nhảy</summary>

**Chọn có điều kiện kèm phép cộng: `collatz`.** Với `n = if n % 2 == 0 { n / 2 } else { 3*n + 1 }`, cả hai ISA tính trước cả hai vế rồi chọn bằng lệnh có điều kiện (mã chạy lại với rustc 1.95.0 `-O`). Trên x86-64: `leaq (%rdi,%rdi,2), %rax; incq %rax` (`rax = 3n+1`), `movq %rdi, %rcx; shrq %rcx` (`rcx = n/2`), `testb $1, %dil`, rồi `cmoveq %rcx, %rax` (chẵn thì lấy `n/2`). Trên AArch64: `lsr x8, x0, #1` (`x8 = n/2`), `add x9, x0, x0, lsl #1` (`x9 = 3n`), `tst x0, #0x1`, rồi `csinc x0, x8, x9, eq`.

`csinc Rd, Rn, Rm, cond` cho `Rn` khi `cond` đúng, ngược lại cho `Rm + 1`. Arm gọi nhóm này là Conditional Select and Increment[^armcsinc]. LLVM thì định nghĩa `csinc` là phép chọn có điều kiện trên `Rm + 1` (`inc` là phép cộng 1)[^llvmcsinc]. Tôi đã chạy thật `cmp x, #5; csinc r, p, q, eq` với `p=100`, `q=200`: `x=5` cho `100`, `x=6` cho `201`. Vì vậy `eq` (số chẵn, bit 0 = 0) chọn `n/2`, còn lại chọn `3n + 1`: bước `+1` của vế lẻ được gộp vào chính lệnh chọn.

**Carry khi trừ, phần còn lại.** Tôi đã chạy thật `subs xzr, a, b` rồi `cset r, hs`: `a=5, b=3` cho `1`; `a=3, b=5` cho `0`. Vậy `b.hs` (C=1) và `jae` (CF=0) cùng nghĩa "a ≥ b không dấu" dù giá trị cờ ngược nhau. Kết luận này đến từ source LLVM[^llvmnzcv] và thí nghiệm, không phải từ Arm ARM (403 lúc soạn). Về phía borrow, `usub.with.overflow` được LLVM lower thành `subs` với điều kiện `LO`[^llvmusubo], nên borrow ứng với C = 0, hay C là NOT borrow. LLVM cũng ghi rõ SBCS dùng carry chứ không phải borrow[^llvmsbcs]. Còn với `adds`/`add`, carry-out của cả hai ISA cùng chiều: C/CF=1 khi tràn không dấu, và LLVM map `UADDO` sang `HS` (qua `ADDS`)[^llvmusubo].

**Bảng điều kiện đầy đủ.** Trong output, `jae`/`b.hs` ở hàm `load_idx` kiểm tra `i >= len` kiểu không dấu (chỉ số `usize`), còn `b.lt` ở `wh` là so sánh `i32` có dấu. Hai chỗ này khớp quy tắc: có dấu ứng với l/g/lt/gt/ge/le, không dấu ứng với a/b/hi/hs/lo/ls. Dive into Systems ghép cặp có dấu/không dấu của x86 là `jg`/`ja`, `jge`/`jae`, `jl`/`jb`, `jle`/`jbe`[^jsufftab]. Của A64 là `gt`/`hi`, `ge`/`hs`, `lt`/`lo`, `le`/`ls`[^bsufftab]. Bảng điều kiện x86 đầy đủ nằm ở SDM Vol. 1 Table 7-4[^sdmjae]. Phía A64, tên và nghĩa các điều kiện nằm trong `enum CondCode` của LLVM[^llvmcc]. Trong đó `HS` và `LO` có alias `cs` và `cc` ở assembler[^llvmalias]. Nghĩa của `hi`, `ls`, `ge`, `lt`, `gt` nằm ở phần còn lại của enum đó[^llvmcc2].

**Nhảy theo thanh ghi, không qua cờ.** A64 có thêm `cbz`/`cbnz`: nếu R bằng 0 thì nhánh được thực hiện và pc được đặt thành addr (`cbz`)[^cbz]. Ví dụ `cbz x1, LBB7_3` ở đầu `sum` ("len == 0"; dòng này không nằm trong đoạn trích ở mục 5).

**Tầm nhảy.** `b` đổi `pc` trong phạm vi 128 MB[^b128]. Nhánh điều kiện thì chỉ có tầm hạn chế hơn nhiều, 1 MB[^brange]. Hệ quả là các lệnh branch A64 không với tới mọi đích[^aapcsveneer]; linker có thể phải chèn một veneer (đoạn mã nối trung gian) giữa hàm gọi và hàm được gọi.

</details>

**Quay lại Q4:** với `orders`, cờ do `cmp`/`test`/lệnh `s` đặt và lệnh nhảy hoặc chọn đọc; hậu tố cho biết có dấu hay không; `cmov`/`csel` chọn giá trị mà không cần nhánh.

## 5. Dịch `if`, `while`, `for`, `match` (Q5)

> **Trả lời ngắn.** Compiler dịch điều kiện thành lệnh sửa con trỏ lệnh để nhảy tới một địa chỉ khác lệnh kế tiếp[^armcond]. Công cụ cụ thể là nhảy không điều kiện, so sánh đặt cờ rồi nhảy có điều kiện, hoặc chọn có điều kiện không nhảy (mục 4). `if` có tác dụng phụ giữ dạng nhánh[^cmovcare]. Trong ví dụ C của Dive into Systems, vòng `for` cho assembly giống hệt vòng `while`[^forwhile]. Còn Rust định nghĩa `for` qua `IntoIterator` và `loop`[^reffor]. `match` trả hằng được LLVM đổi thành bảng tra, và lời gọi ở vị trí đuôi có thể thành `jmp`/`b` (cả hai là quan sát).

**Giải thích.**

*Với service `orders`.* Code của `orders` có đủ bốn kiểu cấu trúc: `if` (hoàn tiền hay thu tiền), `while` (thử lại), `for` (duyệt các dòng hàng), `match` (đổi mã trạng thái thành một giá trị). Mục này xem compiler dịch mỗi kiểu thành khung nhánh nào. Một lưu ý trước: output là kết quả của LLVM chứ không dịch từng dòng, nên asm có thể thiếu hẳn một `call` hay một nhánh mà bạn đã viết.

Nguyên lý chung (tổng hợp của người soạn từ mục 4): compiler có bốn công cụ, gồm nhảy không điều kiện (`jmp`/`b`), so sánh đặt cờ, nhảy có điều kiện (`jcc`/`b.cond`), và chọn có điều kiện không nhảy (`cmov`/`csel`). Mọi cấu trúc điều khiển đều là tổ hợp của bốn thứ này.

### 5.1 `if` có tác dụng phụ: giữ nhánh

Khi hai nhánh gọi hai hàm khác nhau (có tác dụng phụ), compiler không thể "chạy cả hai" như `cmov`, nên phải giữ nhánh (nhận xét của người soạn; Dive into Systems cũng lưu ý compiler thận trọng với tác dụng phụ[^cmovcare]). Hàm dưới đây giả sử `f` hoàn tiền còn `g` thu tiền:

```rust
pub unsafe fn br(x: i32) { if x < 0 { f() } else { g() } }
```

```asm
# x86-64                         # AArch64
br:                              _br:
    testl %edi, %edi                 tbnz w0, #31, LBB0_2   // bit 31 (dấu) = 1 ? -> f
    js    .LBB0_2                    b     _g
    jmpq  *g@GOTPCREL(%rip)      LBB0_2:
.LBB0_2:                             b     _f
    jmpq  *f@GOTPCREL(%rip)
```

Mỗi bên kiểm tra dấu của `x` theo cách riêng. x86 dùng `test` rồi `js`, nhảy khi cờ dấu SF=1[^sdmjs]. A64 dùng `tbnz w0, #31` (test bit and branch if non-zero): nhảy khi bit 31, tức bit dấu của `w0`, bằng 1.

Điều đáng chú ý hơn là **không có `call`/`bl`/`ret`**. Lời gọi `f()`/`g()` nằm ở vị trí đuôi (tail position) nên compiler thay bằng nhảy `jmp`/`b`, và hàm đích sẽ `ret` thẳng về caller của `br`. Đây là tối ưu của compiler, không phải bảo đảm ngôn ngữ (nhận xét của người soạn); tail call được nói riêng ở [Chương 04, mục 7](04-calling-convention-va-stack-frame.md).

### 5.2 `while` và `for`

Trong ví dụ C của Dive into Systems (`sumUp`/`sumUp2`), một vòng `for` cho ra assembly giống hệt vòng `while`[^forwhile]. Với ví dụ đó chỉ còn một khuôn: *khởi tạo → (kiểm tra → thân → bước nhảy) → thoát*. Rust thì định nghĩa `for` là tương đương một `loop` chứa `match` trên `Iterator::next(&mut iter)`, với `iter` lấy từ `IntoIterator::into_iter(iter_expr)`[^reffor]. Assembly cuối cùng của `for` và `while` có giống nhau hay không là việc của optimizer, nên chương này chỉ nói về các output `-O` đã dán (nhận xét của người soạn).

Hàm `wh(mut n: i32) { while n > 0 { f(); n -= 1; } }` (với `f` là hàm ngoài) giống một vòng retry gọi `f` tối đa `n` lần. Trên AArch64 nó cho (đã bỏ `stp`/`ldp`):

```asm
    cmp   w0, #1
    b.lt  LBB3_4         // n<1: thoát
    add   w19, w0, #1
LBB3_2:
    bl    _f
    sub   w19, w19, #1
    cmp   w19, #1
    b.hi  LBB3_2
LBB3_4: ret
```

x86-64 có cùng khung (`testl; jle`, `callq *%r14`, `decl %ebx; cmpl $1; ja`).

### 5.3 `match` và `switch`

Giả sử `orders` đổi mã trạng thái thành thời gian chờ retry:

```rust
pub fn sw(x: u32) -> u32 { match x { 0 => 10, 1 => 20, 2 => 35, 3 => 7, 4 => 99, _ => 0 } }
```

Hai kiến trúc cho cùng một chiến lược: **bảng tra (lookup table)** chứ không phải chuỗi so sánh (quan sát ở output dưới):

```asm
# x86-64                                  # AArch64
sw:                                       _sw:
    xorl %eax, %eax                           cmp  w0, #4
    cmpl $4, %edi                             b.hi LBB8_2
    ja   .LBB8_2         # x>4 -> 0           adrp x8, l_switch.table.sw@PAGE
    movl %edi, %eax                           add  x8, x8, l_switch.table.sw@PAGEOFF
    leaq .Lswitch.table.sw(%rip), %rcx        ldr  w0, [x8, w0, uxtw #2]
    movl (%rcx,%rax,4), %eax                  ret
.LBB8_2: retq                             LBB8_2: mov w0, #0 ; ret
.Lswitch.table.sw: .long 10,20,35,7,99    l_switch.table.sw: .long 10,20,35,7,99
```

Nhánh `_ =>` trở thành một lần so sánh miền (`x > 4` unsigned) rồi nhảy. Phần còn lại là một lệnh load đánh chỉ số vào bảng 5 phần tử `.long` (4 byte). Khi mỗi nhánh *trả về một hằng số*, LLVM biến cả `match` thành bảng dữ liệu (quan sát). Với thân phức tạp hơn tôi không chạy ví dụ nên không khẳng định hình dạng.

<details><summary>Đi sâu: <code>tbnz</code> trong LLVM, hình dạng vòng <code>while</code> và vector hoá</summary>

**`tbnz` từ phía LLVM.** Đây là chi tiết cài đặt của LLVM 22.1.0, có thể đổi. Khi lower phép so sánh `x < 0` (`SETLT` với 0), LLVM sinh `TBNZ` trên vị trí bit dấu[^llvmtbnz]. Và `tbnz` tương đương `tst` bit đó rồi rẽ nhánh khi kết quả khác 0 (`ands xzr, reg, #1<<bit` với điều kiện `NE`)[^llvmtbnzsem]. Tôi cũng chạy thật `tbnz w, #31, label`: nhảy với `w = -3`, không nhảy với `w = 3`.

**Hình dạng vòng `while`.** Ở `wh`, có một **guard** trước vòng lặp (`n <= 0` thì không vào), rồi thân vòng ở nhãn `LBB3_2` nằm *trước* phép kiểm tra cuối (bottom-tested loop), nên mỗi lượt chỉ cần một nhảy có điều kiện ngược. Khác với khuôn "nhảy xuống kiểm tra trước, rồi quay lại" mà Dive into Systems minh hoạ cho `sumUp` (`jmp <sumUp+33>` rồi `jle` ngược)[^loopjmp]. Biến của ví dụ đó nằm trên stack (`-0x4(%rbp)`), nên trông là mã chưa tối ưu (suy luận của người soạn). Bộ đếm của `wh` được biến đổi thành đếm xuống (`n+1`, giảm, so với 1); `ja`/`b.hi` là nhảy *unsigned* vì LLVM đã chứng minh giá trị không âm (suy luận của người soạn).

**Vòng lặp có truy cập mảng và vector hoá.** `sum(p: &[i64]) -> i64` với `for i in 0..p.len() { s += p[i] }` (tổng các khoản trong một đơn) ở `-O` được **vector hoá**: thân scalar vẫn còn để xử lý phần dư, nhưng có thêm vòng chính dùng thanh ghi SIMD. A64 có 32 thanh ghi SIMD/FP `v0`–`v31`[^aapcssimd]. Trích phần cốt lõi:

```asm
# AArch64 (đã rút gọn): vòng vector, bộ đếm giảm 8 mỗi lượt, rồi vòng scalar cho phần dư
LBB7_5:
    ldp   q4, q5, [x8, #-32]
    add.2d v0, v4, v0     // cộng 2 lane i64
    subs  x10, x10, #8
    b.ne  LBB7_5
LBB7_8:
    ldr   x11, [x10], #8
    add   x8, x11, x8
    subs  x9, x9, #1
    b.ne  LBB7_8
```

Trên x86-64, vòng chính dùng `movdqu`/`paddq` với `%xmm` (128 bit; output chỉ chứa thanh ghi `%xmm`, quan sát). Trong vòng không còn bounds check: compiler đã chứng minh `i < len` (suy luận của người soạn; [Chương 07, mục 3](07-rust-va-may.md) phân tích kỹ việc compiler bỏ hoặc đưa bounds check ra ngoài vòng). Còn `subs … ; b.ne` là mẫu "đếm xuống đến 0" của A64.

</details>

**Quay lại Q5:** với `orders`, `if` thành nhánh khi có tác dụng phụ; `for` và `while` chung khung trong ví dụ C của Dive into Systems (Rust định nghĩa `for` qua iterator); `match` trả hằng thành bảng tra; output không dịch từng dòng.

## 6. Gọi hàm: `call`/`bl` và `ret` (Q6)

> **Trả lời ngắn.** `callq` cất địa chỉ trả về (giá trị `%rip` kế tiếp) lên stack[^callx86]. `bl` ghi `pc+4` vào thanh ghi `x30` (LR)[^blarm]. `ret` trên x86-64 nạp lại `%rip` từ stack[^retx86]. Trên A64, `ret` đặt `pc = x30`[^retarm].

**Giải thích.**

*Với service `orders`.* Một request đi qua `orders` là một chuỗi lời gọi hàm lồng nhau: handler gọi parse, parse gọi validate, và cứ thế. Mỗi hàm chạy xong, CPU phải biết quay lại đúng chỗ đã gọi. Hai ISA lưu "địa chỉ quay lại" ở hai nơi khác nhau, và khác biệt đó giải thích vì sao hàm A64 hay mở đầu bằng `stp x29, x30` còn hàm x86-64 thì không.

Hai ISA khác nhau ở **nơi cất địa chỉ trả về**:

| | x86-64 | AArch64 |
|---|---|---|
| Gọi | `callq addr`: cất `%rip` kế tiếp lên stack[^callx86] | `bl addr`: ghi `pc+4` vào `x30` (LR)[^blarm] |
| Về | `retq`: nạp `%rip` từ stack[^retx86] | `ret`: `pc = x30`[^retarm] |
| Địa chỉ trả về nằm ở | bộ nhớ (stack) | thanh ghi |

Tài liệu ABI của Arm diễn đạt cùng ý: `BL` chuyển giá trị kế tiếp của program counter, tức địa chỉ trả về, vào link register (LR)[^aapcsbl].

Ví dụ: `caller(x) = callee(x) + 1`, với `callee(x) = x.wrapping_mul(31)`, kiểu một bước băm id trong `orders` (cả hai `#[inline(never)]`/`no_mangle`):

```asm
# x86-64                          # AArch64
caller:                           _caller:
    pushq %rax          # căn lề     stp  x29, x30, [sp, #-16]!   // cất FP và LR (trước khi bl ghi đè x30)
    callq *callee@GOTPCREL(%rip)     mov  x29, sp
    incq  %rax                       bl   _callee
    popq  %rcx                       add  x0, x0, #1
    retq                             ldp  x29, x30, [sp], #16
                                     ret
callee:                           _callee:
    movq %rdi, %rax                   lsl x8, x0, #5      // x*32
    shlq $5, %rax                     sub x0, x8, x0      // x*32 - x
    subq %rdi, %rax                   ret
    retq
```

`callee` là `x.wrapping_mul(31)`: cả hai ISA thay phép nhân bằng dịch bit rồi trừ (`x*32 - x`). Chỗ đáng xem là `caller`, một hàm *non-leaf* (gọi hàm khác). Trên A64, `bl` ghi đè `x30` còn `ret` lấy địa chỉ từ `x30`, nên prologue phải cất `x30` lên stack bằng `stp` rồi khôi phục bằng `ldp` trước `ret` (suy luận của người soạn từ hai định nghĩa ở bảng trên). Trên x86, `call` tự đẩy địa chỉ trả về lên stack, nên `pushq %rax` ở đây chỉ để căn lề stack (suy luận của người soạn; xem Đi sâu). Hàm *leaf* như `callee` không cần cất gì. Bố cục stack frame và danh sách thanh ghi caller-/callee-saved nằm ở [Chương 04](04-calling-convention-va-stack-frame.md).

<details><summary>Đi sâu: căn lề stack ở x86-64 và AArch64</summary>

Ở x86-64, System V psABI nói stack cần căn 16 (32 hoặc 64) byte ngay trước khi lệnh `call` được thực thi, và ngay khi vào hàm (địa chỉ trả về vừa được đẩy) thì `%rsp + 8` là bội của 16[^sysvalign]. Vì vậy khi vừa vào `caller`, `%rsp` lệch 8 so với bội của 16, và `pushq` 8 byte đưa nó về bội của 16 trước `callq` (suy luận của người soạn từ câu trích). Phía AArch64, AAPCS64 đòi `SP` chia hết cho 16 mỗi khi truy cập bộ nhớ qua SP (phần cứng yêu cầu)[^aapcsalign]. `stp x29, x30, [sp, #-16]!` ở trên trừ SP đúng 16 nên giữ được điều đó (suy luận của người soạn). Quy tắc này, red zone và prologue/epilogue được nói kỹ ở [Chương 04, mục 4 và 6](04-calling-convention-va-stack-frame.md).

</details>

**Quay lại Q6:** với `orders`, x86-64 cất địa chỉ trả về lên stack, A64 cất vào `x30`.

## 7. Sinh và đọc assembly do `rustc` tạo ra (Q7)

> **Trả lời ngắn.** Sinh asm bằng `rustc -O --emit=asm`[^emit]; `-O` là `opt-level=3`[^opt]. `--target` chọn target tuple, tức kiến trúc và hệ điều hành đích[^target]. Output là kết quả của LLVM chứ không dịch từng dòng. Khi đọc, lọc directive trước, rồi tìm nhánh tới `core::panicking`: đó là bounds check[^bounds]. So `opt-level=0` với `-O` thì thấy overflow check[^ovf]. Thanh ghi SIMD trong thân vòng cho biết vòng đã được vector hoá (Đi sâu ở mục 5).

**Giải thích.**

*Với service `orders`.* Khi một hàm nóng của `orders` chậm, bạn mở assembly của nó để trả lời ba câu hỏi: bounds check có nằm trong vòng lặp không, còn phép tính nào mang overflow check không, vòng lặp có được vector hoá không (nhận xét của người soạn). Mục này chỉ cách sinh asm, lọc nhiễu và đọc ba dấu hiệu đó.

### 7.1 Sinh asm

Mọi ví dụ trong chương được sinh bằng `rustc 1.95.0` trên máy `aarch64-apple-darwin`. rustc 1.95.0 dùng LLVM 22[^rustllvm], nên các trích dẫn mã LLVM trong chương ghim ở tag 22.1.0 (bản LLVM đi kèm rustc có thể khác vài chi tiết). Phần x86-64 sinh bằng cross-target `x86_64-unknown-linux-gnu`. Cách này chạy được vì loại output `link` chỉ là mặc định khi không có `--emit`[^emitlink], nên `--emit=asm` không có bước liên kết và không cần linker Linux (suy luận của người soạn từ câu đó). Lệnh:

```sh
rustc -O --emit=asm a.rs -o arm.s --target aarch64-apple-darwin
rustc -O --emit=asm a.rs -o x86.s --target x86_64-unknown-linux-gnu
```

`--emit=asm` tạo ra một file chứa assembly của crate[^emit]; `-O` là bí danh của `-C opt-level=3`[^opt]. File nguồn bắt đầu bằng `#![crate_type="lib"]`, và mỗi hàm có `#[no_mangle] pub fn` để tên symbol giữ nguyên (`add` thay vì `_RNv…`)[^nomangle]. Tôi xoá các directive nhiễu (`.cfi_*`, `.p2align`, `.size`, `.section`…) khỏi bản dán bên dưới và ghi chú mỗi đoạn "rút gọn"; các lệnh còn lại là nguyên văn. Trong file có hai kiểu nhãn: `LBBn_m` (macOS, có thể có thêm tiền tố `_` ở tên hàm) và `.LBBn_m` (ELF/Linux). Đó là chuyện của object format chứ không phải của ISA (quan sát).

Output là **kết quả của LLVM ở `-O`**, không phải dịch từng dòng; `opt-level=0` cho code khác hẳn (xem bước 3 bên dưới). Phiên bản rustc/LLVM khác có thể sinh khác (nhận xét của người soạn).

### 7.2 Bốn bước đọc

1. **Lọc nhiễu.** Directive là chỉ thị cho assembler, tên bắt đầu bằng `.`[^gasdirective]. Ví dụ `.cfi_*` mở và đóng mục unwind của hàm trong `.eh_frame`[^gascfi], cùng với `.p2align`, `.section`, `.size`, `.loh` (chỉ thấy ở output macOS, quan sát). Nhãn là một symbol kết thúc bằng `:`[^gaslabel]; nhãn cục bộ như `.LBB5_2:` cũng bắt đầu bằng `.` nhưng có `:` (quan sát). Phần lệnh là những dòng còn lại.
2. **Tìm "dấu vân tay" của Rust.** Một nhánh dẫn tới `…core9panicking…` (tên symbol của `core::panicking`, quan sát) là một *panic*. Ví dụ `load_idx` có nhánh `cmp x2, x1 ; b.hs LBB5_2` rồi `bl …panic_bounds_check`: đó chính là kiểm tra chỉ số lúc chạy. Reference nói một kiểm tra sẽ được thực hiện lúc chạy và đưa thread vào trạng thái panic nếu nó thất bại[^bounds]. Đường nhanh (`ldr w0, [x0, x2, lsl #2]`) chỉ có `cmp` và một nhánh không được lấy.
3. **So debug với release.** Toán tử số nguyên sẽ panic khi tràn nếu biên dịch ở chế độ debug[^ovf]. Nếu không chỉ định, debug assertions tự bật chỉ khi `opt-level` là 0[^debugassert]. Còn overflow checks bật khi debug assertions bật[^ovfchecks]. Vì vậy so asm của cùng một hàm ở `opt-level=0` và `-O` (`-O` là `opt-level=3`) sẽ cho thấy overflow check: ở `-O` kiểm tra không được phát ra, chứ không phải optimizer "chứng minh" không tràn (suy luận của người soạn từ hai câu trên). Listing nằm ở Đi sâu bên dưới; [Chương 07, mục 1](07-rust-va-may.md) đối chiếu cùng cặp `-O0`/`-O3` cho một hàm `u64`.
4. **Nhận ra vector hoá.** Vòng lặp mà thân xử lý nhiều phần tử mỗi lượt bằng thanh ghi SIMD là vòng đã được vector hoá. Trên A64 đó là các thanh ghi SIMD `v0`–`v31`[^aapcssimd], xuất hiện trong output dưới tên `q4`, `q5`, `v0`… (quan sát); trên x86-64 là `%xmm…`. Ví dụ `sum` ở Đi sâu của mục 5 có `ldp q4, q5` và `add.2d` ở A64, `movdqu`/`paddq` ở x86-64 (quan sát).

**Hiểu lầm phổ biến**

- "Assembly dịch một-một từ Rust": không; `br` mất cả `call`, `max` mất cả nhánh (quan sát ở mục 4 và 5).
- "`mov eax, …` chỉ đụng 32 bit": sai trên x86-64, nó xoá nửa trên `rax`[^sdm32].

<details><summary>Đi sâu: overflow check ở <code>opt-level=0</code></summary>

Thật vậy, `a + b` cho `i64` ở `opt-level=0` (AArch64):

```asm
# AArch64, opt-level=0 (đã bỏ phần hằng panic)
_add:
    sub  sp, sp, #32
    stp  x29, x30, [sp, #16]
    add  x29, sp, #16
    adds x8, x0, x1         // cộng + đặt cờ
    str  x8, [sp, #8]       // spill ra stack
    cset w8, vs             // w8 = (V set) ? 1 : 0
    tbnz w8, #0, LBB0_2     // tràn -> panic
    b    LBB0_1
LBB0_1: ldr x0, [sp, #8] ; ldp x29, x30, [sp, #16] ; add sp, sp, #32 ; ret
LBB0_2: ... bl ...panic_const_add_overflow
```

Ở `-O` cùng hàm chỉ còn một lệnh (`add x0, x1, x0` / `leaq`) vì kiểm tra không còn được phát ra. Cờ `V`/`OF` ở đây là "tràn có dấu" của bảng mục 4: `adds … cset vs` đọc cờ V ngay sau lệnh cộng. Ở x86-64, lệnh `jo` nhảy khi OF = 1[^sdmjs]. Chương này không dán output x86-64 ở `opt-level=0` nên không khẳng định dạng lệnh cụ thể ở đó (suy luận của người soạn: tương tự, đọc cờ OF sau `addq`).

</details>

**Quay lại Q7:** với `orders`, sinh bằng `--emit=asm`, lọc directive, tìm nhánh panic, so debug với release để thấy overflow check, và nhận ra vector hoá qua thanh ghi SIMD.

## Liên hệ với Rust

- Lệnh sinh asm là `rustc --emit=asm`[^emit]. `--target` chọn target tuple[^target]. Với Cargo: `cargo rustc --release -- --emit=asm` (nhận xét của người soạn, chưa chạy).
- `-O` là `opt-level=3`[^opt]. Việc kiểm tra tràn số có thể điều khiển trực tiếp bằng `-C overflow-checks` / `-C debug-assertions`[^ovfflags], và mặc định của chúng phụ thuộc `opt-level` như ở mục 7.
- Rust ABI không ổn định[^rustabi], nên nếu viết `asm!` hoặc gọi vào code ngoài thì dùng `extern "C"`. Khi đó bạn bám vào ABI mặc định của compiler C cho target[^rustabic] (chi tiết ở [Chương 04, mục 8](04-calling-convention-va-stack-frame.md)).
- `asm!` trong Rust dùng Intel syntax mặc định trên x86[^rustasm]; `att_syntax` để đổi[^rustatt]. Cùng câu trong Reference còn nhắc chế độ `.syntax unified` cho "ARM" nói chung; chương không dùng nó để khẳng định gì về AArch64 (nhận xét của người soạn).

## Tóm tắt

- x86-64: 16 thanh ghi (`rax/eax/ax/al`); A64: `x0–x30` với nửa thấp `w0–w30`, thêm `sp`, `pc`, `xzr`.
- Ghi 32-bit trên x86-64 zero-extend lên 64-bit; ghi 8/16-bit giữ phần cao. `xzr` đọc ra 0 và ghi vào bị bỏ qua; nó dùng chung số hiệu 31 với `sp`.
- AT&T: nguồn trước, `$imm`, `%reg`, hậu tố kích thước; Intel: đích trước. `rustc` in AT&T trong `.s`; `asm!` mặc định Intel.
- Hầu hết lệnh A64 có ba toán hạng và theo mô hình load/store (chỉ lệnh load/store chạm RAM); x86-64 cho một toán hạng bộ nhớ trong lệnh số học.
- Cờ do `cmp`/`test`/`tst`/lệnh `s` đặt; `jcc`/`b.cond`/`cmov`/`csel` đọc; hậu tố quyết định có dấu hay không dấu. Sau phép trừ không dấu, x86 đặt CF=1 khi có borrow còn A64 đặt C=0.
- `if` có tác dụng phụ → nhánh; chỉ chọn giá trị → `cmov`/`csel` (không phải lúc nào cũng nhanh hơn nhánh); `for` và `while` chung khung trong ví dụ C của DiS; `match` trả hằng → bảng tra.
- `call` cất địa chỉ trả về lên stack, `bl` cất vào `x30`.
- Đọc asm: lọc directive, tìm nhánh tới `core::panicking`, so `-O` với `opt-level=0`; thanh ghi SIMD trong thân vòng là dấu hiệu vector hoá.

## Câu hỏi tự kiểm tra

1. `movl %edi, %eax` thay đổi những bit nào của `%rax`? Còn `movb %dil, %al`?
<details><summary>Đáp án</summary>

`movl` ghi 32 bit, kết quả được zero-extend lên 64 bit nên bit 63–32 thành 0[^sdm32]. `movb` ghi 8 bit và 56 bit cao của đích không đổi[^sdm816].

</details>

2. Chuyển `addl $4, %eax` sang Intel syntax.
<details><summary>Đáp án</summary>

`add eax, 4` (ví dụ của chính tài liệu GNU as)[^gasorder].

</details>

3. Vì sao `cmpq %rsi, %rdx` rồi `jae` tương ứng với `rdx >= rsi` (`i >= len`) mà không phải `rsi >= rdx`?
<details><summary>Đáp án</summary>

AT&T có nguồn trước đích[^gasorder]. `cmp` trừ toán hạng đứng trước khỏi toán hạng đứng sau: `cmp R1, R2` của x86 tính R2 - R1[^x86cmpord]. Còn `cmp O1, O2` của A64 tính O1 - O2[^armcmp]. Với `cmpq %rsi, %rdx` đó là `rdx - rsi`, và `jae` nhảy khi CF = 0, tức không borrow: `rdx >= rsi` không dấu[^sdmjae].

</details>

4. `ldr w0, [x8, w0, uxtw #2]` đọc từ địa chỉ nào?
<details><summary>Đáp án</summary>

`x8 + (zext(w0) << 2)`: chỉ số 32 bit được zero-extend rồi nhân 4, là dạng "scaled/extended" trong bảng addressing mode (đã chạy thật, xem mục 3; dạng `sxtw` tương tự có trong Dive into Systems)[^armscaled].

</details>

5. Khi nào compiler dùng `cmov`/`csel` thay vì nhánh, và vì sao?
<details><summary>Đáp án</summary>

Khi hai nhánh chỉ chọn giá trị và không có tác dụng phụ. `cmov` thực hiện "chuyển dữ liệu có điều kiện" thay vì "chuyển điều khiển có điều kiện"[^cmov]. Mục đích là tránh phí nhánh[^brexp]. Compiler ngại làm vậy khi có tác dụng phụ[^cmovcare]. Và `cmov` không phải lúc nào cũng nhanh hơn nhánh: xem [Chương 05, mục 7](05-pipeline-va-speculation.md).

</details>

6. Phân biệt `jl` và `jb` (x86) / `b.lt` và `b.lo` (A64).
<details><summary>Đáp án</summary>

Hậu tố quyết định so sánh có dấu hay không dấu[^jsuffix]. Trên x86, so sánh có dấu dùng SF/OF, không dấu dùng CF[^x86flagsuse]. A64 tương tự: V cho so sánh có dấu, C cho so sánh không dấu[^armflags].

</details>

7. Vì sao `caller` trên AArch64 phải `stp x29, x30, …` còn `callee` thì không?
<details><summary>Đáp án</summary>

`bl` ghi địa chỉ trả về vào `x30`[^blarm]. Còn `ret` lấy lại `pc = x30`[^retarm]. Hàm non-leaf gọi `bl` sẽ ghi đè `x30` của chính nó, nên phải cất rồi khôi phục trước `ret` (suy luận của người soạn từ hai định nghĩa đó). `callee` là leaf, không gọi ai.

</details>

8. Vì sao `br(x)` không có `call`/`bl` hay `ret`?
<details><summary>Đáp án</summary>

Hai lời gọi nằm ở vị trí đuôi nên compiler thay bằng nhảy; hàm đích `ret` thẳng về caller của `br` (quan sát từ output; tối ưu của compiler, không phải bảo đảm của Rust).

</details>

9. Hai dấu hiệu nào trong output `opt-level=0` của `a + b` cho thấy overflow check?
<details><summary>Đáp án</summary>

AArch64: `adds` (cộng và đặt cờ) rồi `cset w8, vs` đọc cờ tràn và `tbnz` tới `panic_const_add_overflow`. Reference nói debug build panic khi tràn[^ovf]. Debug assertions mặc định bật khi `opt-level` là 0[^debugassert]. Overflow checks bật theo debug assertions[^ovfchecks]. Với x86-64, chương không dán output ở `opt-level=0`; việc đọc cờ OF bằng `jo`[^sdmjs] là suy luận của người soạn.

</details>

10. Sau `cmp a, b` với `a >= b` không dấu, cờ carry bằng bao nhiêu trên x86-64 và trên AArch64?
<details><summary>Đáp án</summary>

x86-64: CF = 0 (không borrow), nên `jae` (CF = 0) nhảy[^sdmjae]. AArch64: C = 1, nên `b.hs` (ứng với C == 1) nhảy[^llvmnzcv]. Hai cờ ngược giá trị nhưng cùng nghĩa "a ≥ b không dấu" (mục 4).

</details>

11. `cmp x0, #0` trên AArch64 thực chất là lệnh gì, và vì sao dùng `xzr`?
<details><summary>Đáp án</summary>

Là `subs xzr, x0, #0`[^llvmcmp]. Thanh ghi zero luôn đọc ra 0, ghi vào bị bỏ qua, và `CMP` được mã hoá thành `SUB` với đích là thanh ghi zero[^armzr]. Nên chỉ cờ được cập nhật, kết quả bị bỏ.

</details>

## Đọc thêm

- [Dive into Systems, §7 x86-64 Assembly](https://diveintosystems.org/book/C7-x86_64/index.html): từ `mov` đến hàm, mảng, struct.
- [Dive into Systems, §9 ARM64 Assembly](https://diveintosystems.org/book/C9-ARM64/index.html): bản song song cho A64.
- Intel SDM tập 1 (Basic Architecture) §3.4 và §3.7 (thanh ghi và địa chỉ), Table 7-4 (lệnh Jcc): <https://cdrdv2.intel.com/v1/dl/getContent/671436> — bản hiện hành, URL không ghim phiên bản; tập 2 (từng lệnh) là một file riêng của Intel.
- [GNU as, i386 Syntactical Considerations](https://sourceware.org/binutils/docs-2.42/as/i386_002dVariations.html): bảng khác biệt AT&T/Intel đầy đủ.
- Rust Reference, [Inline assembly](https://doc.rust-lang.org/1.95.0/reference/inline-assembly.html): khi nào tự viết `asm!`.
- Arm, [Porting to 64-bit ARM](https://community.arm.com/cfs-file/__key/telligent-evolution-components-attachments/01-2142-00-00-00-00-52-01/Porting-to-ARM-64_2D00_bit.pdf) (white paper 2014, không phải Arm Architecture Reference Manual): `sp` và thanh ghi zero dùng chung số 31, `cmp` là `sub` đích zero, `stp`/`ldp` thay push/pop.
- [Chương 04, mục 4, 6 và 7](04-calling-convention-va-stack-frame.md): căn lề stack, prologue/epilogue và tail call, phần Q5–Q6 ở đây chỉ nhắc.
- [Chương 07, mục 1 và 3](07-rust-va-may.md): `opt-level`, overflow check và bounds check đào sâu hơn.

[^emit]: rustc book, Command-line arguments, --emit — <https://doc.rust-lang.org/1.95.0/rustc/command-line-arguments.html> — "asm — Generates a file with the crate’s assembly code. The default output filename is CRATE_NAME.s."
[^opt]: rustc book, Command-line arguments, -O — <https://doc.rust-lang.org/1.95.0/rustc/command-line-arguments.html> — "A synonym for -C opt-level=3."
[^target]: rustc book, Command-line arguments, --target — <https://doc.rust-lang.org/1.95.0/rustc/command-line-arguments.html> — "--target: select a target tuple to build"
[^rustasm]: Rust Reference, Inline assembly, directives — <https://doc.rust-lang.org/1.95.0/reference/inline-assembly.html> — "On x86, the .intel_syntax noprefix mode of GAS is used by default. On ARM, the .syntax unified mode is used."
[^rustatt]: Rust Reference, Inline assembly, options — <https://doc.rust-lang.org/1.95.0/reference/inline-assembly.html> — "att_syntax: This option is only valid on x86, and causes the assembler to use the .att_syntax prefix mode of the GNU assembler."
[^rustabi]: Rust Reference, External blocks, ABI — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "The native calling convention for Rust functions and closures. This is the default when a function is declared without using extern fn. The Rust ABI offers no stability guarantees."
[^gasdiff]: GNU as manual 2.42, AT&T Syntax versus Intel Syntax — <https://sourceware.org/binutils/docs-2.42/as/i386_002dVariations.html> — "AT&T immediate operands are preceded by ‘$’; Intel immediate operands are undelimited (Intel ‘push 4’ is AT&T ‘pushl $4’). AT&T register operands are preceded by ‘%’; Intel register operands are undelimited."
[^gasorder]: GNU as manual 2.42, AT&T Syntax versus Intel Syntax — <https://sourceware.org/binutils/docs-2.42/as/i386_002dVariations.html> — "AT&T and Intel syntax use the opposite order for source and destination operands. Intel ‘add eax, 4’ is ‘addl $4, %eax’."
[^gassize]: GNU as manual 2.42, AT&T Syntax versus Intel Syntax — <https://sourceware.org/binutils/docs-2.42/as/i386_002dVariations.html> — "In AT&T syntax the size of memory operands is determined from the last character of the instruction mnemonic."
[^gasscale]: GNU as manual 2.42, Memory References — <https://sourceware.org/binutils/docs-2.42/as/i386_002dMemory.html> — "scale, taking the values 1, 2, 4, and 8, multiplies index to calculate the address of the operand."
[^x86regs]: Dive into Systems, 7.1.1 Registers — <https://diveintosystems.org/book/C7-x86_64/basics.html> — "%rax, %rbx, %rcx, %rdx, %rdi, %rsi, %rsp, %rbp, and %r8-%r15."
[^x86gp]: Dive into Systems, 7.1.1 Registers — <https://diveintosystems.org/book/C7-x86_64/basics.html> — "All the registers save for %rsp and %rbp hold general-purpose 64-bit data."
[^x86rip]: Dive into Systems, 7.1.1 Registers — <https://diveintosystems.org/book/C7-x86_64/basics.html> — "Unlike the 16 registers mentioned previously, programs cannot write directly to register %rip."
[^x86sub]: Dive into Systems, 7.1.2 Advanced Register Notation — <https://diveintosystems.org/book/C7-x86_64/basics.html> — "To access the lower 32 bits of the first eight registers, simply replace the r in the register name with e. Thus, the register corresponding to the lower 32 bits of register %rax is register %eax. To access the lower 16 bits of each of these eight registers, reference the last two letters of the register’s name. So, the mechanism to access the lower two bytes of register %rax is %ax."
[^x86r8]: Dive into Systems, 7.1.2 Advanced Register Notation — <https://diveintosystems.org/book/C7-x86_64/basics.html> — "Thus, %r9d accesses the lower 32 bits of register %r9, whereas %r9w accesses the lower 16 bits, and %r9b accesses the lowest byte of register %r9."
[^sdmbyte]: Intel SDM Vol. 1 (253665-093US, 09/2026; URL không ghim phiên bản), Table 3-2 Addressable General Purpose Registers — <https://cdrdv2.intel.com/v1/dl/getContent/671436> — "AL, BL, CL, DL, DIL, SIL, BPL, SPL, R8B - R15B"
[^sdm816]: Intel SDM Vol. 1 (253665-093US, 09/2026; URL không ghim phiên bản), 3.4.1.1 General-Purpose Registers in 64-Bit Mode — <https://cdrdv2.intel.com/v1/dl/getContent/671436> — "8-bit and 16-bit operands generate an 8-bit or 16-bit result. The upper 56 bits or 48 bits (respectively) of the destination general-purpose register are not modified by the operation."
[^sdm32]: Intel SDM Vol. 1 (253665-093US, 09/2026; URL không ghim phiên bản), 3.4.1.1 General-Purpose Registers in 64-Bit Mode — <https://cdrdv2.intel.com/v1/dl/getContent/671436> — "32-bit operands generate a 32-bit result, zero-extended to a 64-bit result in the destination general-purpose register."
[^x86args]: Dive into Systems, 7.5.1 Function Parameters — <https://diveintosystems.org/book/C7-x86_64/functions.html> — "The first six parameters to a function are loaded into registers %rdi, %rsi, %rdx, %rcx, %r8, and %r9, respectively."
[^armregs]: Dive into Systems, 9.1.1 Registers — <https://diveintosystems.org/book/C9-ARM64/basics.html> — "The ARMv8 CPU has a total of 31 registers for storing general-purpose 64-bit data: x0 to x30."
[^aapcsregs]: AAPCS64 (Arm ABI), General-purpose registers — <https://github.com/ARM-software/abi-aa/blob/2025Q4/aapcs64/aapcs64.rst> — "There are thirty-one, 64-bit, general-purpose (integer) registers visible to the A64 instruction set; these are labeled r0-r30. In a 64-bit context these registers are normally referred to using the names x0-x30; in a 32-bit context the registers are specified by using w0-w30."
[^armw]: Dive into Systems, 9.1.2 Advanced Register Notation — <https://diveintosystems.org/book/C9-ARM64/basics.html> — "If 32-bit data is stored in component register w0, then the upper 32 bits of the register become inaccessible, and are zeroed out."
[^armsp]: Dive into Systems, 9.1.1 Registers — <https://diveintosystems.org/book/C9-ARM64/basics.html> — "Next, the zero register zr permanently stores the value 0, and is only useful as a source register."
[^armbl]: Dive into Systems, 9.5 Functions — <https://diveintosystems.org/book/C9-ARM64/functions.html> — "In A64 systems, the return address is stored in register x30 (also known as LR)."
[^aapcsfp]: AAPCS64 (Arm ABI), Special roles — <https://github.com/ARM-software/abi-aa/blob/2025Q4/aapcs64/aapcs64.rst> — "In all variants of the procedure call standard, registers r16, r17, r29 and r30 have special roles. In these roles they are labeled IP0, IP1, FP and LR when being used for holding addresses"
[^x86sd]: Dive into Systems, 7.1.3 Instruction Structure — <https://diveintosystems.org/book/C7-x86_64/basics.html> — "Two operand instructions typically follow the source, destination (S, D) format"
[^armops]: Dive into Systems, 9.1.3 Instruction Structure — <https://diveintosystems.org/book/C9-ARM64/basics.html> — "For most A64 instructions, the following format is typically used: opcode D, O1, O2 Where opcode is the operation code, D is the destination register, O1 is the first operand, and O2 the second operand. For example, the instruction add w0, w0, #0x2 has the opcode add, a destination register of w0, and the two operands w0 and #0x2."
[^lea]: Dive into Systems, 7.3.3 The Load Effective Address Instruction — <https://diveintosystems.org/book/C7-x86_64/arithmetic.html> — "The lea instruction operates on the same operand structure that we’ve seen thus far but does not include a memory lookup."
[^ldst]: Dive into Systems, 9.1.4 An Example with Operands — <https://diveintosystems.org/book/C9-ARM64/basics.html> — "Data cannot be read or written to memory directly; instead, ARM follows a load/store model, which requires data to be operated on in registers."
[^x86mem]: Dive into Systems, 7.1.4 An Example with Operands — <https://diveintosystems.org/book/C7-x86_64/basics.html> — "Memory forms cannot serve as both the source and destination operand in a single instruction."
[^x86memform]: Dive into Systems, 7.1.3 Instruction Structure — <https://diveintosystems.org/book/C7-x86_64/basics.html> — "It loosely translates to \"add -0x4 to the value in register %rbp (i.e., subtract 0x4 from %rbp), and then perform a memory lookup.\""
[^armmemform]: Dive into Systems, 9.1.3 Instruction Structure — <https://diveintosystems.org/book/C9-ARM64/basics.html> — "It loosely translates to \"add 12 to the value in register sp, and then perform a memory lookup on the corresponding address.\""
[^sdmea]: Intel SDM Vol. 1 (253665-093US, 09/2026; URL không ghim phiên bản), Figure 3-11 Offset (or Effective Address) Computation — <https://cdrdv2.intel.com/v1/dl/getContent/671436> — "Offset = Base + (Index * Scale) + Displacement"
[^armscaled]: Dive into Systems, 9.1.4 An Example with Operands (Table 1) — <https://diveintosystems.org/book/C9-ARM64/basics.html> — "So, [x0, w3, SXTW] sign extends w3 into a 64-bit value before adding it to x0 and performing a memory lookup. Lastly, scaled memory types enable the calculation of offsets through the use of a left shift."
[^armcond]: Dive into Systems, 9.4 Conditional Control and Loops — <https://diveintosystems.org/book/C9-ARM64/conditional_control_loops.html> — "The compiler translates conditionals into assembly instructions that modify the instruction pointer (pc) to point to an address that is not the next one in the program sequence."
[^cmov]: Dive into Systems, 7.4.2 The cmov Instructions — <https://diveintosystems.org/book/C7-x86_64/if_statements.html> — "In contrast, the cmov instruction implements a conditional transfer of data. In other words, both the then_statement and else_statement of the conditional are executed, and the data is placed in the appropriate register based on the result of the condition."
[^csel]: Dive into Systems, 9.4.2 The Conditional Select Instruction — <https://diveintosystems.org/book/C9-ARM64/if_statements.html> — "In other words, the CPU executes both the then_statement and else_statement, and places the data in the appropriate register based on the result of the condition."
[^brexp]: Dive into Systems, 9.4.2 The Conditional Select Instruction — <https://diveintosystems.org/book/C9-ARM64/if_statements.html> — "because branch instructions are typically very expensive to execute, due to the disruption they can cause to the instruction pipeline"
[^cmovcare]: Dive into Systems, 7.4.2 The cmov Instructions — <https://diveintosystems.org/book/C7-x86_64/if_statements.html> — "In general, the compiler is very cautious about optimizing jump instructions into cmov instructions, especially in cases where side effects and pointer values are involved."
[^forwhile]: Dive into Systems, 7.4.3 for Loops in Assembly — <https://diveintosystems.org/book/C7-x86_64/loops.html> — "This version yields assembly code identical to our while loop example."
[^loopjmp]: Dive into Systems, 7.4.3 while Loops in Assembly — <https://diveintosystems.org/book/C7-x86_64/loops.html> — "The first instruction is a direct jump to \<sumUp+33>, which sets the instruction pointer (%rip) to address 0x400547."
[^x86cmp]: Dive into Systems, 7.4.1 Preliminaries — <https://diveintosystems.org/book/C7-x86_64/preliminaries.html> — "Unlike the arithmetic instructions covered thus far, cmp and test do not modify the destination register. Instead, both instructions modify a series of single-bit values known as condition code flags."
[^sdmflags]: Intel SDM Vol. 1 (253665-093US, 09/2026; URL không ghim phiên bản), 3.4.3 EFLAGS Register — <https://cdrdv2.intel.com/v1/dl/getContent/671436> — "Other instructions (such as the Jcc instructions) use the state of the status flags in the EFLAGS register as condition codes for branching or other decision making operations."
[^pstate]: Dive into Systems, 9.4.1 Preliminaries — <https://diveintosystems.org/book/C9-ARM64/preliminaries.html> — "The condition code flags are part of the ARM processor state (PSTATE),"
[^x86flagsuse]: Dive into Systems, 7.4.1 Preliminaries — <https://diveintosystems.org/book/C7-x86_64/preliminaries.html> — "The SF and OF flags are used for comparison operations on signed integers, whereas the CF flag is used for comparisons on unsigned integers."
[^armflags]: Dive into Systems, 9.4.1 Preliminaries — <https://diveintosystems.org/book/C9-ARM64/preliminaries.html> — "The V flag is set to 1 if the operation O1 - O2 results in overflow (useful for signed comparisons). The C flag is set to 1 if the operation O1 - O2 results in an arithmetic carry operation (useful for unsigned comparisons)."
[^armcmp]: Dive into Systems, 9.4.1 Preliminaries — <https://diveintosystems.org/book/C9-ARM64/preliminaries.html> — "Compares O1 with O2 (computes O1 - O2)"
[^jsuffix]: Dive into Systems, 7.4.1 Preliminaries — <https://diveintosystems.org/book/C7-x86_64/preliminaries.html> — "Each instruction starts with the letter j denoting that it is a jump instruction. The suffix of each instruction indicates the condition for the jump. The jump instruction suffixes also determine whether to interpret numerical comparisons as signed or unsigned."
[^bsuffix]: Dive into Systems, 9.4.1 Preliminaries — <https://diveintosystems.org/book/C9-ARM64/preliminaries.html> — "The suffix of each instruction (c) indicates the condition for the branch. The branch instruction suffixes also determine whether to interpret numerical comparisons as signed or unsigned. Note that conditional branch instructions have a much more limited range (1 MB) than the b instruction. These suffixes are also used for the conditional select instruction (csel), which is covered in the next section."
[^cbz]: Dive into Systems, 9.4.1 Preliminaries — <https://diveintosystems.org/book/C9-ARM64/preliminaries.html> — "In the case of cbz, if R is zero, the branch is taken and pc is set to addr."
[^brange]: Dive into Systems, 9.4.1 Preliminaries — <https://diveintosystems.org/book/C9-ARM64/preliminaries.html> — "Note that conditional branch instructions have a much more limited range (1 MB) than the b instruction."
[^callx86]: Dive into Systems, 7.5 Functions — <https://diveintosystems.org/book/C7-x86_64/functions.html> — "When the caller function executes the callq instruction, the current value of %rip is saved on the stack to represent the return address"
[^blarm]: Dive into Systems, 9.5 Functions — <https://diveintosystems.org/book/C9-ARM64/functions.html> — "When the caller function executes the bl instruction, the value of pc + 4 is saved in register x30 to represent the return address"
[^aapcsbl]: AAPCS64 (Arm ABI), Subroutine calls — <https://github.com/ARM-software/abi-aa/blob/2025Q4/aapcs64/aapcs64.rst> — "The effect of executing BL is to transfer the sequentially next value of the program counter—the return address—into the link register (LR)"
[^aapcsalign]: AAPCS64 (Arm ABI), Universal stack constraints — <https://github.com/ARM-software/abi-aa/blob/2025Q4/aapcs64/aapcs64.rst> — "Additionally, at any point at which memory is accessed via SP, the hardware requires that SP mod 16 = 0. The stack must be quad-word aligned."
[^bounds]: Rust Reference, Array and slice indexing expressions — <https://doc.rust-lang.org/1.95.0/reference/expressions/array-expr.html> — "Otherwise a check will be performed at run-time that will put the thread in a panicked state if it fails."
[^ovf]: Rust Reference, Overflow — <https://doc.rust-lang.org/1.95.0/reference/expressions/operator-expr.html> — "Integer operators will panic when they overflow when compiled in debug mode."
[^gasptr]: GNU as manual 2.42, AT&T Syntax versus Intel Syntax — <https://sourceware.org/binutils/docs-2.42/as/i386_002dVariations.html> — "Intel syntax accomplishes this by prefixing memory operands (not the instruction mnemonics) with ‘byte ptr’, ‘word ptr’, ‘dword ptr’, ‘qword ptr’"
[^x86sp]: Dive into Systems, 7.1.1 Registers — <https://diveintosystems.org/book/C7-x86_64/basics.html> — "The registers %rsp and %rbp are known as the stack pointer and the frame pointer"
[^aapcssp]: AAPCS64 (Arm ABI), General-purpose registers — <https://github.com/ARM-software/abi-aa/blob/2025Q4/aapcs64/aapcs64.rst> — "Additionally, a stack-pointer register, SP, can be used with a restricted number of instructions."
[^armpc]: Dive into Systems, 9.1.1 Registers — <https://diveintosystems.org/book/C9-ARM64/basics.html> — "The pc register points to the next instruction to be executed by the CPU; unlike the other registers, programs cannot write directly to the pc register."
[^llvmreg31]: LLVM 22.1.0, AArch64RegisterInfo.td — <https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-22.1.0/llvm/lib/Target/AArch64/AArch64RegisterInfo.td> — "def SP : AArch64Reg<31, "sp", [WSP, WSP_HI]>, DwarfRegAlias\<WSP>; def XZR : AArch64Reg<31, "xzr", [WZR, WZR_HI]>, DwarfRegAlias\<WSP> { let isConstant = true; }"
[^llvmgpr]: LLVM 22.1.0, AArch64RegisterInfo.td — <https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-22.1.0/llvm/lib/Target/AArch64/AArch64RegisterInfo.td> — "def GPR64sp : RegisterClass<"AArch64", [i64], 64, (add GPR64common, SP)> {"
[^llvmcmp]: LLVM 22.1.0, AArch64InstrInfo.cpp (insertSelect/branch condition) — <https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-22.1.0/llvm/lib/Target/AArch64/AArch64InstrInfo.cpp> — "cmp reg, #0 is actually subs xzr, reg, #0."
[^llvmtbnzsem]: LLVM 22.1.0, AArch64InstrInfo.cpp (insertSelect, nhánh tbz/tbnz) — <https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-22.1.0/llvm/lib/Target/AArch64/AArch64InstrInfo.cpp> — "cmp reg, #foo is actually ands xzr, reg, #1<\<foo."
[^llvmtbnz]: LLVM 22.1.0, AArch64ISelLowering.cpp (LowerBR_CC) — <https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-22.1.0/llvm/lib/Target/AArch64/AArch64ISelLowering.cpp> — "uint64_t SignBitPos; std::tie(LHS, SignBitPos) = lookThroughSignExtension(LHS); return DAG.getNode(AArch64ISD::TBNZ, DL, MVT::Other, Chain, LHS, DAG.getConstant(SignBitPos, DL, MVT::i64), Dest);"
[^llvmpre]: LLVM 22.1.0, ISDOpcodes.h, MemIndexedMode — <https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-22.1.0/llvm/include/llvm/CodeGen/ISDOpcodes.h> — "the value of the base pointer add / subtract the offset. It considers the computation as being folded into the load / store operation"
[^llvmpost]: LLVM 22.1.0, ISDOpcodes.h, MemIndexedMode — <https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-22.1.0/llvm/include/llvm/CodeGen/ISDOpcodes.h> — "value of the offset operand is then added to / subtracted from the base after memory transaction."
[^llvmwb]: LLVM 22.1.0, AArch64InstrFormats.td, LoadPreIdx và LoadPostIdx — <https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-22.1.0/llvm/lib/Target/AArch64/AArch64InstrFormats.td> — "(outs GPR64sp:$wback, regtype:$Rt), (ins GPR64sp:$Rn, simm9:$offset), asm, "$Rn = $wback,@earlyclobber $wback", []>"
[^sdmjs]: Intel SDM Vol. 1 (253665-093US, 09/2026; URL không ghim phiên bản), Table 7-4 Conditional Jump Instructions — <https://cdrdv2.intel.com/v1/dl/getContent/671436> — "JO | OF = 1 | Overflow | JS | SF = 1 | Sign (negative)"
[^x86flagsreg]: Dive into Systems, 7.4.1 Preliminaries — <https://diveintosystems.org/book/C7-x86_64/preliminaries.html> — "The condition code flags are part of the FLAGS register on x86 systems."
[^sdmcf]: Intel SDM Vol. 1 (253665-093US, 09/2026; URL không ghim phiên bản), 3.4.3.1 Status Flags — <https://cdrdv2.intel.com/v1/dl/getContent/671436> — "Carry flag — Set if an arithmetic operation generates a carry or a borrow out of the"
[^sdmjae]: Intel SDM Vol. 1 (253665-093US, 09/2026; URL không ghim phiên bản), Table 7-4 Conditional Jump Instructions — <https://cdrdv2.intel.com/v1/dl/getContent/671436> — "JAE/JNB | CF = 0 | Above or equal/not below | JB/JNAE | CF = 1 | Below/not above or equal"
[^llvmsbcs]: LLVM 22.1.0, AArch64ISelLowering.cpp (LowerSETCCCARRY) — <https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-22.1.0/llvm/lib/Target/AArch64/AArch64ISelLowering.cpp> — "SBCS uses a carry not a borrow so the carry flag should be inverted first."
[^llvmusubo]: LLVM 22.1.0, AArch64ISelLowering.cpp (getAArch64XALUOOp) — <https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-22.1.0/llvm/lib/Target/AArch64/AArch64ISelLowering.cpp> — "case ISD::UADDO: Opc = AArch64ISD::ADDS; CC = AArch64CC::HS; break; case ISD::SSUBO: Opc = AArch64ISD::SUBS; CC = AArch64CC::VS; break; case ISD::USUBO: Opc = AArch64ISD::SUBS; CC = AArch64CC::LO; break;"
[^llvmcc]: LLVM 22.1.0, Utils/AArch64BaseInfo.h, enum CondCode — <https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-22.1.0/llvm/lib/Target/AArch64/Utils/AArch64BaseInfo.h> — "EQ = 0x0, // Equal Equal NE = 0x1, // Not equal Not equal, or unordered HS = 0x2, // Unsigned higher or same >, ==, or unordered LO = 0x3, // Unsigned lower Less than"
[^llvmalias]: LLVM 22.1.0, AArch64AsmParser.cpp — <https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-22.1.0/llvm/lib/Target/AArch64/AsmParser/AArch64AsmParser.cpp> — ".Case("cs", AArch64CC::HS) .Case("hs", AArch64CC::HS) .Case("cc", AArch64CC::LO) .Case("lo", AArch64CC::LO)"
[^b128]: Dive into Systems, 9.4.1 Preliminaries — <https://diveintosystems.org/book/C9-ARM64/preliminaries.html> — "The b instruction enables the program counter to change within 128 MB of its current location"
[^aapcsveneer]: AAPCS64 (Arm ABI), Use of IP0 and IP1 by the linker — <https://github.com/ARM-software/abi-aa/blob/2025Q4/aapcs64/aapcs64.rst> — "The A64 branch instructions are unable to reach every destination in the address space, so it may be necessary for the linker to insert a veneer between a calling routine and a called subroutine."
[^llvmcsinc]: LLVM 22.1.0, AArch64InstrInfo.td (CSINC và PatFrag inc) — <https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-22.1.0/llvm/lib/Target/AArch64/AArch64InstrInfo.td> — "def inc : PatFrag<(ops node:$in), (add_and_or_is_add node:$in, 1)>; defm CSINC : CondSelectOp<0, 0b01, "csinc", inc>;"
[^sysvalign]: System V AMD64 psABI, 3.2.2 The Stack Frame — <https://gitlab.com/x86-psABIs/x86-64-ABI/-/raw/e1ce098331da5dbd66e1ffc74162380bcc213236/x86-64-ABI/low-level-sys-info.tex> — "In other words, the stack needs to be 16 (32 or 64) byte aligned immediately before the call instruction is executed. Once control has been transferred to the function entry point, i.e. immediately after the return address has been pushed, %rsp points to the return address, and the value of (%rsp + 8) is a multiple of 16 (32 or 64)."
[^retx86]: Dive into Systems, 7.5 Functions — <https://diveintosystems.org/book/C7-x86_64/functions.html> — "The retq instruction restores the value of %rip to the value saved on the stack, ensuring that the program resumes execution at the program address specified in the caller function."
[^retarm]: Dive into Systems, 9.5 Functions — <https://diveintosystems.org/book/C9-ARM64/functions.html> — "ret Returns value in x0 and sets pc = x30"
[^ovfflags]: Rust Reference, Overflow — <https://doc.rust-lang.org/1.95.0/reference/expressions/operator-expr.html> — "The -C debug-assertions and -C overflow-checks compiler flags can be used to control this more directly."
[^x86low8]: Dive into Systems, 7.1.2 Advanced Register Notation — <https://diveintosystems.org/book/C7-x86_64/basics.html> — "For example, %al references the lower eight-bits of register %ax, whereas %ah references the higher eight-bits of register %ax."
[^sdm16]: Intel SDM Vol. 1 (253665-093US, 09/2026; URL không ghim phiên bản), 3.4.1.1 General-Purpose Registers in 64-Bit Mode — <https://cdrdv2.intel.com/v1/dl/getContent/671436> — "In 64-bit mode, there are 16 general purpose registers and the default operand size is 32 bits."
[^x86ret]: Dive into Systems, 7.5 Functions — <https://diveintosystems.org/book/C7-x86_64/functions.html> — "Any value returned by the callee is stored in %rax or one of its component registers (e.g., %eax)."
[^x86add]: Dive into Systems, 7.3 Arithmetic Instructions, Table 1 — <https://diveintosystems.org/book/C7-x86_64/arithmetic.html> — "add S, D S + D → D sub S, D D - S → D"
[^x86cmpord]: Dive into Systems, 7.4.1 Preliminaries — <https://diveintosystems.org/book/C7-x86_64/preliminaries.html> — "The cmp instruction compares the values of two registers, R2 and R1. Specifically, it subtracts R1 from R2."
[^llvmnzcv]: LLVM 22.1.0, Utils/AArch64BaseInfo.h, getNZCVToSatisfyCondCode — <https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-22.1.0/llvm/lib/Target/AArch64/Utils/AArch64BaseInfo.h> — "case EQ: return Z; // Z == 1 case NE: return 0; // Z == 0 case HS: return C; // C == 1 case LO: return 0; // C == 0 case MI: return N; // N == 1 case PL: return 0; // N == 0 case VS: return V; // V == 1 case VC: return 0; // V == 0"
[^llvmcc2]: LLVM 22.1.0, Utils/AArch64BaseInfo.h, enum CondCode — <https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-22.1.0/llvm/lib/Target/AArch64/Utils/AArch64BaseInfo.h> — "HI = 0x8, // Unsigned higher Greater than, or unordered LS = 0x9, // Unsigned lower or same Less than or equal GE = 0xa, // Greater than or equal Greater than or equal LT = 0xb, // Less than Less than, or unordered GT = 0xc, // Greater than Greater than"
[^jsufftab]: Dive into Systems, 7.4.1 Preliminaries, Table 4 Conditional Jump Instructions — <https://diveintosystems.org/book/C7-x86_64/preliminaries.html> — "je (jz) jump if equal (==) or jump if zero jne (jnz) jump if not equal (!=) js jump if negative jns jump if non-negative jg (jnle) ja (jnbe) jump if greater (>) jge (jnl) jae (jnb) jump if greater than or equal (>=) jl (jnge) jb (jnae) jump if less (<) jle (jng) jbe (jna) jump if less than or equal (<=)"
[^bsufftab]: Dive into Systems, 9.4.1 Preliminaries, Table 4 Conditional Branch Instruction Suffixes — <https://diveintosystems.org/book/C9-ARM64/preliminaries.html> — "eq eq branch if equal (==) or branch if zero ne ne branch if not equal (!=) mi mi branch if minus (negative) pl pl branch if non-negative (>= 0) gt hi branch if greater than (higher) (>) ge cs (hs) branch if greater than or equal (>=) lt lo (cc) branch if less than (<) le ls branch if less than or equal (<=)"
[^llvmgpr64]: LLVM 22.1.0, AArch64RegisterInfo.td — <https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-22.1.0/llvm/lib/Target/AArch64/AArch64RegisterInfo.td> — "def GPR64 : RegisterClass<"AArch64", [i64], 64, (add GPR64common, XZR)> {"
[^llvmtst]: LLVM 22.1.0, AArch64InstrInfo.td (alias của tst) — <https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-22.1.0/llvm/lib/Target/AArch64/AArch64InstrInfo.td> — "def : InstAlias<"tst $src1, $src2", (ANDSXri XZR, GPR64:$src1, logical_imm64:$src2), 2>;"
[^armzr]: Arm, Porting to 64-bit ARM (white paper của Chris Shore, ARM, 07/2014, mô tả ARMv8-A thời kỳ đầu; không phải Arm Architecture Reference Manual), The Zero Register — <https://community.arm.com/cfs-file/__key/telligent-evolution-components-attachments/01-2142-00-00-00-00-52-01/Porting-to-ARM-64_2D00_bit.pdf> — "Specifically, it always reads as zero and writes to it are ignored. This means that the CMP instruction can be encoded as a SUB with the Zero Register as the destination."
[^armr31]: Arm, Porting to 64-bit ARM (white paper của Chris Shore, ARM, 07/2014, mô tả ARMv8-A thời kỳ đầu; không phải Arm Architecture Reference Manual), Stack Pointer — <https://community.arm.com/cfs-file/__key/telligent-evolution-components-attachments/01-2142-00-00-00-00-52-01/Porting-to-ARM-64_2D00_bit.pdf> — "The Stack Pointer and the Zero Register are both encoded via ARM core register 31. Only a few instructions recognize this coding as the Stack Pointer (the remainder see it as the Zero Register)."
[^armspinstr]: Arm, Porting to 64-bit ARM (white paper của Chris Shore, ARM, 07/2014, mô tả ARMv8-A thời kỳ đầu; không phải Arm Architecture Reference Manual), Stack Pointer — <https://community.arm.com/cfs-file/__key/telligent-evolution-components-attachments/01-2142-00-00-00-00-52-01/Porting-to-ARM-64_2D00_bit.pdf> — "The following instructions can access the Stack Pointer: All loads and stores can use SP as the base register AND, ORR and EOR (with immediate and without flag setting) can use SP as the destination ADD/SUB with immediate can use SP as destination or first operand ADD/SUB extend can use SP as destination or first operand"
[^armzeroupper]: Arm, Porting to 64-bit ARM (white paper của Chris Shore, ARM, 07/2014, mô tả ARMv8-A thời kỳ đầu; không phải Arm Architecture Reference Manual), Overview of AArch64 in ARMv8-A — <https://community.arm.com/cfs-file/__key/telligent-evolution-components-attachments/01-2142-00-00-00-00-52-01/Porting-to-ARM-64_2D00_bit.pdf> — "General-purpose registers are all 64-bit and can be accessed by most instructions either as 64-bit doublewords or as 32-bit words. If written as a 32-bit word, then the top half of the register is cleared."
[^armpushpop]: Arm, Porting to 64-bit ARM (white paper của Chris Shore, ARM, 07/2014, mô tả ARMv8-A thời kỳ đầu; không phải Arm Architecture Reference Manual), Overview of AArch64 in ARMv8-A — <https://community.arm.com/cfs-file/__key/telligent-evolution-components-attachments/01-2142-00-00-00-00-52-01/Porting-to-ARM-64_2D00_bit.pdf> — "The load and store multiple instructions have been replaced with instructions which load and store pairs of 64-bit registers. These are used for stack operations as well, in place of the earlier PUSH and POP."
[^armcsinc]: Arm, Porting to 64-bit ARM (white paper của Chris Shore, ARM, 07/2014, mô tả ARMv8-A thời kỳ đầu; không phải Arm Architecture Reference Manual), Instruction set — <https://community.arm.com/cfs-file/__key/telligent-evolution-components-attachments/01-2142-00-00-00-00-52-01/Porting-to-ARM-64_2D00_bit.pdf> — "Instead, it supports a range of instructions (like CSINC – Conditional Select and Increment) whose behavior is modified by the current state of the condition code flags."
[^reffor]: Rust Reference, Iterator loops — <https://doc.rust-lang.org/1.95.0/reference/expressions/loop-expr.html> — "A for loop is equivalent to a loop expression containing a match expression as follows:"
[^nomangle]: Rust Reference, Application Binary Interface, The no_mangle attribute — <https://doc.rust-lang.org/1.95.0/reference/abi.html> — "The no_mangle attribute may be used on any item to disable standard symbol name mangling. The symbol for the item will be the identifier of the item’s name."
[^rustabic]: Rust Reference, External blocks, ABI — <https://doc.rust-lang.org/1.95.0/reference/items/external-blocks.html> — "The “C” ABI matches the default ABI chosen by the dominant C compiler for the target."
[^gasdirective]: GNU as manual 2.42, Assembler Directives — <https://sourceware.org/binutils/docs-2.42/as/Pseudo-Ops.html> — "All assembler directives have names that begin with a period (‘.’)."
[^gaslabel]: GNU as manual 2.42, Labels — <https://sourceware.org/binutils/docs-2.42/as/Labels.html> — "A label is written as a symbol immediately followed by a colon ‘:’."
[^gascfi]: GNU as manual 2.42, CFI directives — <https://sourceware.org/binutils/docs-2.42/as/CFI-directives.html> — ".cfi_endproc is used at the end of a function where it closes its unwind entry previously opened by .cfi_startproc, and emits it to .eh_frame."
[^gccmasm]: GCC 14.2.0 manual, x86 Options, -masm=dialect — <https://gcc.gnu.org/onlinedocs/gcc-14.2.0/gcc/x86-Options.html> — "Supported choices (in dialect order) are ‘att’ or ‘intel’. The default is ‘att’."
[^llvmargs]: rustc book, Codegen options, llvm-args — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "This flag can be used to pass a list of arguments directly to LLVM. The list must be separated by spaces. Pass --help to see a list of options. Because this flag directly talks to LLVM, it is not subject to the usual stability guarantees of rustc’s CLI interface."
[^llvmasmsyntax]: LLVM 22.1.0, X86MCAsmInfo.cpp — <https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-22.1.0/llvm/lib/Target/X86/MCTargetDesc/X86MCAsmInfo.cpp> — "X86AsmSyntax( "x86-asm-syntax", cl::init(ATT), cl::Hidden, cl::desc("Select the assembly style for input"), cl::values(clEnumValN(ATT, "att", "Emit AT&T-style assembly"), clEnumValN(Intel, "intel", "Emit Intel-style assembly")));"
[^debugassert]: rustc book, Codegen options, debug-assertions — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "If not specified, debug assertions are automatically enabled only if the opt-level is 0."
[^ovfchecks]: rustc book, Codegen options, overflow-checks — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "If not specified, overflow checks are enabled if debug-assertions are enabled, disabled otherwise."
[^emitlink]: rustc book, Command-line arguments, --emit — <https://doc.rust-lang.org/1.95.0/rustc/command-line-arguments.html> — "link — Generates the crates specified by --crate-type. The default output filenames depend on the crate type and platform. This is the default if --emit is not specified."
[^rustllvm]: Rust 1.95.0 RELEASES.md, Version 1.95 (2026-04-16), Internal Changes — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/RELEASES.md> — "significant improvements to the performance or internals of rustc and related tools. - Update to LLVM 22"
[^aapcssimd]: AAPCS64 (Arm ABI), SIMD and Floating-Point registers — <https://github.com/ARM-software/abi-aa/blob/2025Q4/aapcs64/aapcs64.rst> — "The Arm 64-bit architecture also has a further thirty-two registers, v0-v31, which can be used by SIMD and Floating-Point operations."
[^x86flagtab]: Dive into Systems, 7.4.1 Preliminaries, Table 2 Common Condition Code Flags — <https://diveintosystems.org/book/C7-x86_64/preliminaries.html> — "ZF Is equal to zero (1: yes; 0: no) SF Is negative (1: yes; 0: no) OF Overflow has occurred (1:yes; 0: no) CF Arithmetic carry has occurred (1: yes; 0:no)"
[^armflagtab]: Dive into Systems, 9.4.1 Preliminaries, Table 2 Common Condition Code Flags — <https://diveintosystems.org/book/C9-ARM64/preliminaries.html> — "Z Is equal to zero (1: yes, 0: no) N Is negative (1: yes, 0: no) V Signed overflow has occurred (1: yes, 0: no) C Arithmetic carry/unsigned overflow has occurred (1: yes, 0: no)"
