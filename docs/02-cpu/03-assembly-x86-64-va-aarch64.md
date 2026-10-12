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

Quy ước gọi hàm, stack frame, căn lề stack và tail call ở [Chương 04](04-calling-convention-va-stack-frame.md); pipeline và giá của nhánh đoán sai ở [Chương 05](05-pipeline-va-speculation.md); `opt-level` và bounds check được đào sâu hơn ở [Chương 07](07-rust-va-may.md) (mục 1 và 3).

## 1. Cú pháp AT&T và Intel (x86-64) (Q1)

> **Trả lời ngắn.** Cùng một lệnh x86-64 có hai cách viết: AT&T (nguồn trước, đích sau, `$imm`, `%reg`) và Intel (đích trước); GNU as nêu rõ hai cú pháp dùng thứ tự nguồn và đích ngược nhau[^gasorder]. `rustc --emit=asm` in AT&T (quan sát), còn `asm!` viết tay trong Rust mặc định dùng Intel[^rustasm]. Với A64, tài liệu và output trong chương này luôn viết đích trước[^armops].

**Giải thích.**

Hình dung (nhận xét của người soạn): ghi "chuyển từ A sang B" hay "chuyển tới B từ A": cùng một việc, chỉ khác thứ tự đọc.

Cùng một ISA, hai cách viết. Tài liệu GNU as liệt kê khác biệt chính[^gasdiff].

| Điểm | AT&T | Intel |
|---|---|---|
| Thứ tự toán hạng | nguồn, đích | đích, nguồn |
| Hằng (immediate) | `$4` | `4` |
| Thanh ghi | `%rax` | `rax` |
| Kích thước toán hạng bộ nhớ | hậu tố mnemonic `b/w/l/q` | tiền tố `byte/word/dword/qword ptr` |
| Tham chiếu bộ nhớ | `disp(base,index,scale)` | `[base + index*scale + disp]` |

Nguồn các hàng: thứ tự nguồn/đích[^gasorder]; `$` và `%`[^gasdiff]; hậu tố `b/w/l/q` so với tiền tố `… ptr`[^gassize][^gasptr]; dạng tham chiếu bộ nhớ[^gasscale].

Ví dụ thật — cùng một hàm `load_idx(p: &[i32], i: usize) -> i32 { p[i] }` ở hai cú pháp (đường nhanh, đã rút gọn bỏ nhánh panic):

```asm
# AT&T (mặc định)                 # Intel (-x86-asm-syntax=intel)
load_idx:                         load_idx:
    cmpq   %rsi, %rdx                 cmp    rdx, rsi
    jae    .LBB5_2                    jae    .LBB5_2
    movl   (%rdi,%rdx,4), %eax        mov    eax, dword ptr [rdi + 4*rdx]
    retq                              ret
```

Hai dòng `movl` và `mov … dword ptr` là một lệnh máy. Lưu ý `cmpq %rsi, %rdx` (AT&T) so `rdx` với `rsi`, tức `i` với `len`: thứ tự đảo so với cách đọc tự nhiên nên rất dễ nhầm khi đọc `jae` (nhận xét của người soạn; quy tắc của `cmp` ở mục 4). Chương này dùng AT&T cho phần x86-64 vì đó là thứ `rustc --emit=asm` in ra (quan sát) và cũng là mặc định của `gcc`[^gccmasm]; phần AArch64 viết đích trước như đã nói ở trên.

**Cú pháp mặc định.** Rust phát x86-64 ở cú pháp AT&T trong các file `.s` (quan sát ở mọi output dưới đây: `%rax`, `$1`); LLVM chọn AT&T làm mặc định cho x86 và cho đổi bằng tùy chọn `x86-asm-syntax`[^llvmasmsyntax]. Ngược lại, với `asm!` viết tay trong Rust, trên x86, chế độ `.intel_syntax noprefix` của GAS được dùng theo mặc định[^rustasm]; còn option `att_syntax` mới chuyển sang AT&T[^rustatt]. Bạn sẽ gặp cả hai. Để ép `--emit=asm` ra Intel syntax, tôi đã chạy thật `rustc -C llvm-args=-x86-asm-syntax=intel`; `-C llvm-args` chuyển thẳng tham số cho LLVM nên không thuộc bảo đảm ổn định của CLI rustc, có thể đổi[^llvmargs].

<details><summary>Đi sâu: trích GNU as về thứ tự, kích thước và scale</summary>

Tài liệu nói: cú pháp AT&T và Intel dùng thứ tự ngược nhau cho toán hạng nguồn và đích; ví dụ Intel `add eax, 4` là `addl $4, %eax` trong AT&T[^gasorder]. Về kích thước: AT&T xác định kích thước toán hạng bộ nhớ từ ký tự cuối của mnemonic lệnh[^gassize] (vì thế `movq`, `addl`, `cmpq` trong output), còn Intel đặt `qword ptr` (hay `byte`/`word`/`dword ptr`) trước toán hạng bộ nhớ[^gasptr]. Khi GAS dịch tham chiếu bộ nhớ Intel `section:[base + index*scale + disp]` sang AT&T `section:disp(base, index, scale)`, `scale` nhận giá trị 1, 2, 4 hoặc 8 và nhân với `index` để tính địa chỉ[^gasscale].

</details>

**Quay lại Q1:** AT&T là nguồn-đích với `$`/`%` và hậu tố kích thước, Intel là đích-nguồn; cùng một lệnh máy.

## 2. Thanh ghi: `rax`/`eax`/`ax`/`al`, `x0`/`w0`, `xzr`/`sp` (Q2)

> **Trả lời ngắn.** Trên x86-64, `rax`, `eax`, `ax`, `al` là các cỡ 64/32/16/8 bit của cùng một thanh ghi[^x86sub][^x86low8]; A64 có `x0`–`x30` 64 bit và dùng tên `w0`–`w30` cho ngữ cảnh 32 bit[^aapcsregs]. Ghi vào phần 32 bit trên x86-64 xoá nửa trên về 0, còn ghi 8/16 bit thì không[^sdm32][^sdm816]. `xzr` luôn đọc ra 0 và mọi lần ghi vào nó bị bỏ qua[^armzr]; nó dùng chung số hiệu thanh ghi 31 với `sp`, và lệnh nào hiểu số 31 là cái nào thì tuỳ lệnh[^armr31].

**Giải thích.**

Hình dung (nhận xét của người soạn): một ngăn kéo dài 64 ô: `eax`, `ax`, `al` là các đoạn cuối của ngăn kéo đó; ghi đủ 4 ô cuối thì máy tự dọn sạch các ô còn lại.

[Chương 01, mục 3](01-mo-hinh-may-va-isa.md) (phần Đi sâu) đã điểm qua số lượng thanh ghi, thanh ghi con và quy tắc ghi 32-bit; mục này đi vào chi tiết từng ISA kèm output thật.

**x86-64**

x86-64 có 16 thanh ghi 64-bit: `%rax, %rbx, %rcx, %rdx, %rdi, %rsi, %rsp, %rbp` và `%r8`–`%r15`[^x86regs]; Intel SDM đếm cả 16 là thanh ghi đa dụng ở 64-bit mode[^sdm16]. Theo Dive into Systems, mọi thanh ghi trừ `%rsp` và `%rbp` chứa dữ liệu 64-bit đa dụng[^x86gp], còn `%rsp`/`%rbp` là con trỏ stack và con trỏ frame[^x86sp]; ngoài ra có `%rip` (instruction pointer) mà chương trình không ghi trực tiếp được[^x86rip].

**Sub-register.** Với tám thanh ghi đầu (`%rax`…`%rbp`), đổi `r` trong tên thành `e` để chọn 32 bit thấp (`%eax`) và lấy hai chữ cuối của tên để chọn 16 bit thấp (`%ax`)[^x86sub]; byte thấp của bốn thanh ghi đầu có tên kết thúc bằng `l` (`%al`)[^x86low8], còn các tên như `%dil`, `%sil` có trong bảng dưới. Với `%r8`–`%r15` thì hậu tố `d`, `w`, `b` chọn 32/16/8 bit thấp (`%r9d` truy cập 32 bit thấp của thanh ghi `%r9`)[^x86r8].

| 64-bit | 32-bit | 16-bit | 8-bit thấp |
|---|---|---|---|
| `%rax` | `%eax` | `%ax` | `%al` |
| `%rdi` | `%edi` | `%di` | `%dil` |
| `%r9` | `%r9d` | `%r9w` | `%r9b` |

**Quy tắc bẫy: ghi 32-bit xoá nửa trên, ghi 8/16-bit thì không.** Intel SDM, 64-bit mode: toán hạng 8-bit và 16-bit cho kết quả 8-bit hoặc 16-bit; 56 bit hoặc 48 bit cao (tương ứng) của thanh ghi đa dụng đích không bị phép toán sửa đổi[^sdm816], trong khi toán hạng 32-bit cho kết quả 32-bit, được zero-extend thành kết quả 64-bit trong thanh ghi đa dụng đích[^sdm32]. Hệ quả bạn thấy ngay trong output:

```asm
byte32:                 # fn byte32(x: u8) -> u32 { x as u32 }
    movzbl %dil, %eax   # đọc %dil (8 bit), ghi %eax (32 bit) => %rax cao = 0
    retq
```

Ghi `%eax` đủ để `rax` mang giá trị `u32` đã zero-extend (hệ quả của quy tắc 32-bit ở trên[^sdm32]). Cùng lý do, `xorl %eax, %eax` (ví dụ ở `sw`, mục 5) đặt `rax = 0` (suy luận của người soạn từ quy tắc 32-bit).

**AArch64**

AArch64 (ARMv8-A, tập lệnh A64) có 31 thanh ghi đa dụng 64-bit `x0`–`x30`[^armregs]. AAPCS64 viết cùng nội dung: ba mươi mốt thanh ghi đa dụng (số nguyên) 64-bit; trong ngữ cảnh 64-bit gọi bằng `x0`–`x30`, còn trong ngữ cảnh 32-bit thì dùng `w0`–`w30`[^aapcsregs]. `w0`–`w30` là nửa thấp 32 bit của cùng thanh ghi: tài liệu Arm nói nếu ghi như một word 32-bit thì nửa trên của thanh ghi bị xoá[^armzeroupper], và Dive into Systems nói tương tự khi dữ liệu 32-bit nằm trong `w0`[^armw] — cùng hành vi như x86-64 khi ghi 32-bit. Trong output dưới đây, nơi x86-64 dùng `movzbl`, A64 dùng `and w0, w0, #0xff` trên `w0` (quan sát ở `byte32`). Tôi đã chạy thật trên máy `aarch64-apple-darwin`: sau `mov x0, #-1` rồi `mov w0, #1`, `x0` đọc ra `0x1`.

```asm
byte32:                 // aarch64: fn byte32(x: u8) -> u32 { x as u32 }
    and  w0, w0, #0xff  // giữ 8 bit thấp; ghi w0 => x0 cao = 0
    ret
```

Các thanh ghi đặc biệt:

| Tên | Vai trò | Nguồn |
|---|---|---|
| `sp` | stack pointer; chỉ dùng được với một số lượng lệnh hạn chế | [^aapcssp] |
| `pc` | program counter; chương trình không ghi trực tiếp | [^armpc] |
| `xzr` / `wzr` (DiS gọi "zr") | luôn đọc ra 0, ghi vào bị bỏ qua | [^armzr] |
| `x30` = `lr` | link register: địa chỉ trả về của `bl` | [^armbl] |
| `x29` = `fp` | frame pointer | [^aapcsfp] |

Dive into Systems viết thanh ghi zero "chỉ hữu ích làm thanh ghi nguồn"[^armsp]; nhưng Arm nói rõ ghi vào nó bị bỏ qua và `cmp` được mã hoá thành `sub` có đích là thanh ghi zero[^armzr], nên dùng nó làm đích để bỏ kết quả cũng là cách dùng thật (xem mục 4).

<details><summary>Đi sâu: tên <code>dil</code>, quy ước gọi hàm, và thí nghiệm <code>xzr</code>/<code>sp</code> trong LLVM</summary>

(`%dil`: bảng tên thanh ghi ở 64-bit mode của SDM liệt kê `DIL`, `SIL`, `BPL`, `SPL`, `R8B`–`R15B` ở cột có tiền tố REX[^sdmbyte].)

Calling convention gắn với tên thanh ghi: theo Dive into Systems, sáu tham số đầu tiên của một hàm được nạp vào các thanh ghi `%rdi`, `%rsi`, `%rdx`, `%rcx`, `%r8` và `%r9`[^x86args], còn giá trị trả về nằm ở `%rax` hoặc một thanh ghi con của nó như `%eax`[^x86ret]. Do đó `fn add(a: i64, b: i64)` đọc `%rdi`, `%rsi` và trả `%rax` (suy luận của người soạn từ hai câu trên; chi tiết ở [Chương 04](04-calling-convention-va-stack-frame.md)). Lưu ý: hàm Rust khai báo không có `extern` dùng ABI "Rust" — calling convention gốc cho hàm và closure Rust, không đưa ra bảo đảm ổn định nào[^rustabi]; việc nó trùng với quy ước C trong các ví dụ ở đây là quan sát, không phải bảo đảm.

Trong phần mã hoá, `xzr` và `sp` dùng chung một số hiệu thanh ghi (31): Arm nói cả hai được mã hoá bằng core register 31 và chỉ một số ít lệnh hiểu số đó là `sp`, còn lại coi nó là thanh ghi zero[^armr31]. Các lệnh hiểu là `sp` gồm: mọi lệnh load/store (làm thanh ghi base), `and`/`orr`/`eor` dạng immediate không đặt cờ (làm đích), và `add`/`sub` dạng immediate hoặc dạng extend (làm đích hoặc toán hạng nguồn đầu)[^armspinstr]. LLVM khai báo cả hai thanh ghi với số hiệu 31[^llvmreg31] và chọn bằng lớp thanh ghi mà từng toán hạng của lệnh chấp nhận — `GPR64` là `x0`–`x30` cộng `XZR`[^llvmgpr64], còn `GPR64sp` là `x0`–`x30` cộng `SP`[^llvmgpr]. Hệ quả tôi đã thử bằng `asm!` (rustc 1.95.0, `aarch64-apple-darwin`): `add x8, sp, #0` được assembler chấp nhận (ở dạng immediate, số 31 của toán hạng nguồn là `sp`), `add x8, xzr, #7` bị từ chối "invalid operand", còn `add x8, xzr, x1` chạy và cho `7` khi `x1 = 7` (dạng thanh ghi, số 31 là `xzr`); `adds xzr, x8, #1` chạy được vì đích `xzr` bỏ kết quả. Kết quả khớp mô tả của Arm ở trên và việc `cmp reg, #0` thực chất là `subs xzr, reg, #0`[^llvmcmp] (mục 4). Đây là hành vi của assembler LLVM kết hợp tài liệu Arm; đặc tả đầy đủ nằm ở Arm ARM, trang developer.arm.com trả 403 lúc soạn.

</details>

**Quay lại Q2:** sub-register là các cỡ của cùng một thanh ghi (ghi 32 bit xoá nửa trên, ghi 8/16 bit thì không); `xzr` đọc ra 0 (ghi vào bị bỏ qua) và dùng chung số 31 với `sp`.

## 3. Lệnh cơ bản, load/store và addressing mode (Q3)

> **Trả lời ngắn.** x86-64 chủ yếu là lệnh hai toán hạng, đích cũng là nguồn[^x86sd][^x86add], và không cho phép bộ nhớ vừa là nguồn vừa là đích trong một lệnh[^x86mem]; hầu hết lệnh A64 có dạng ba toán hạng (đích, nguồn 1, nguồn 2)[^armops] và theo mô hình load/store: dữ liệu phải ở thanh ghi, chỉ các lệnh load/store chuyên dụng (như `ldr`/`str`) mới chạm bộ nhớ[^ldst]. Toán hạng bộ nhớ x86-64 theo công thức Base + Index×Scale + Displacement[^sdmea].

**Giải thích.**

Hình dung (nhận xét của người soạn): x86 như thợ làm thẳng trên bàn đầy đồ (làm việc ngay trên bộ nhớ); A64 như bếp phải lấy nguyên liệu ra mặt bếp (thanh ghi) rồi mới nấu.

Với x86-64, lệnh hai toán hạng thường theo định dạng nguồn, đích (S, D) (AT&T)[^x86sd], và với lệnh số học đích cũng là một nguồn: `add S, D` tính `S + D → D`[^x86add]. Với A64, hầu hết lệnh có dạng `opcode D, O1, O2` với `D` là thanh ghi đích; ví dụ `add w0, w0, #0x2` có thanh ghi đích là w0 và hai toán hạng là w0 và #0x2[^armops]. Một hệ quả thiết kế (nhận xét của người soạn): x86-64 chủ yếu là lệnh **hai** toán hạng, đích cũng là nguồn (`addq %rsi, %rdi`: `rdi += rsi`), còn A64 thường là **ba** toán hạng nên đích không bắt buộc trùng nguồn (như `add x8, x1, x0` ở [Chương 01, mục 6](01-mo-hinh-may-va-isa.md); trong `add(a,b)` dưới đây compiler chọn đích `x0` vì không cần giữ lại `a`/`b`). Output hàm `add(a,b)` làm rõ:

```asm
# x86-64 (AT&T)               # AArch64
add:                          _add:
    leaq (%rdi,%rsi), %rax        add  x0, x1, x0
    retq                          ret
```

x86-64 dùng `lea` thay `add` để có đích khác nguồn (suy luận của người soạn). DiS: `lea` không bao gồm việc tra cứu bộ nhớ[^lea] — tức tính `rdi+rsi` mà không truy cập bộ nhớ.

**Truy cập bộ nhớ: load/store và mov**

A64 theo mô hình load/store: dữ liệu không thể được đọc hoặc ghi vào bộ nhớ trực tiếp mà phải được xử lý trong thanh ghi[^ldst]. Vì vậy `add`, `cmp`… chỉ làm trên thanh ghi, còn việc chạm RAM do các lệnh load/store chuyên dụng đảm nhiệm: `ldr` (load), `str` (store) và `ldp`/`stp` (nạp/lưu cặp thanh ghi 64-bit, thay cho load/store-multiple và push/pop của ARM cũ[^armpushpop]). x86-64 thì `mov` vừa nạp vừa lưu, và nhiều lệnh số học nhận một toán hạng bộ nhớ (`addq 8(%rdi), %rax`), nhưng dạng bộ nhớ không thể vừa là toán hạng nguồn vừa là toán hạng đích trong một lệnh[^x86mem]. Cặp hàm thật dưới đây (rút gọn, `p: &[u64;4]`):

```asm
# fn ld(p: &[u64;4]) -> u64 { p[1] + p[3] }
# x86-64                          # AArch64
ld:                               _ld:
    movq 24(%rdi), %rax               ldr  x8, [x0, #8]
    addq 8(%rdi), %rax                ldr  x9, [x0, #24]
    retq                              add  x0, x9, x8
                                      ret
```

Cùng một công việc: x86-64 gộp load vào `addq`, A64 tách `ldr` + `add`. Offset `8` và `24` là byte (`1*8`, `3*8`). Một lệnh ghi tương ứng (`p[1] = v`) sẽ là `movq %rsi, 8(%rdi)` / `str x1, [x0, #8]` (ví dụ minh hoạ của người soạn, không phải output đã dán).

**Addressing mode**

Dive into Systems mô tả toán hạng bộ nhớ x86-64 như `-0x4(%rbp)`: cộng -0x4 vào giá trị trong thanh ghi `%rbp`, rồi tra cứu bộ nhớ[^x86memform]; và A64 như `[sp, #12]`: cộng 12 vào giá trị trong thanh ghi `sp`, rồi tra cứu bộ nhớ[^armmemform]. Công thức tổng quát x86-64 trong Intel SDM: `Offset = Base + (Index * Scale) + Displacement`[^sdmea], với Scale ∈ {1,2,4,8}[^gasscale]. Bảng đối chiếu các dạng gặp thật trong output (các ô "cần lệnh riêng" và "(hai lệnh)" là nhận xét của người soạn, không phải khẳng định của nguồn):

| Ý nghĩa | x86-64 (AT&T) | AArch64 |
|---|---|---|
| `*(r)` | `(%rdi)` | `[x0]` |
| `*(r + 8)` | `8(%rdi)` | `[x0, #8]` |
| `*(r + i*4)` | `(%rdi,%rdx,4)` | `[x0, x2, lsl #2]` |
| `*(r + zext(w)*4)` | cần lệnh riêng | `[x8, w0, uxtw #2]` |
| `*r` rồi `r += 8` | (hai lệnh) | `ldr x11, [x10], #8` (post-index) |
| `push`/`pop`-kiểu | `pushq` / `popq` | `stp x29, x30, [sp, #-16]!` (pre-index), `ldp x29, x30, [sp], #16` (post-index) |

<details><summary>Đi sâu: thí nghiệm pre-/post-index và <code>uxtw</code>/<code>sxtw</code>, địa chỉ position-independent</summary>

Các dạng `[x0, x2, lsl #2]`, `[x0, w3, SXTW]` có trong bảng toán hạng của DiS cho ARM64[^armscaled]; dạng `uxtw` là bản zero-extend của `sxtw` (suy luận của người soạn từ tên lệnh): tôi đã chạy `ldr x, [base, w, uxtw #3]` với `w = 0x80000000` và nó đọc đúng phần tử ở `base + 0x4_0000_0000` (zero-extend, không phải âm), còn `sxtw` với cùng `w` cho `base − 0x4_0000_0000`. Với pre-index (`[x, #imm]!`) và post-index (`[x], #imm`), LLVM mô tả chế độ chung của hai kiểu: ở pre-index địa chỉ truy cập là con trỏ base cộng/trừ offset[^llvmpre], ở post-index địa chỉ truy cập chính là base pointer, còn offset được cộng vào base sau khi truy cập bộ nhớ[^llvmpost]; ở A64, cả hai biến thể của `ldr` có thêm kết quả ghi lại (`$wback`) buộc vào thanh ghi base `$Rn`[^llvmwb]. Đọc vào output: `ldr x11, [x10], #8` đọc từ `x10` rồi tăng `x10` thêm 8, chính là bước `p++` của vòng cộng mảng `u64`; `stp x29, x30, [sp, #-16]!` trừ `sp` 16 rồi mới ghi cặp. Tôi đã chạy thật: `ldr x, [p], #8` đọc `10` và `p` tăng 8 byte; `ldr y, [p, #8]!` đọc `20` (phần tử kế) và `p` tăng 8 byte.

Cặp `adrp`+`add … @PAGEOFF` (macOS, A64) và `leaq sym(%rip)`/`@GOTPCREL(%rip)` (Linux, x86-64) là cách lấy địa chỉ symbol theo kiểu tương đối (quan sát trong output của chương); chương này chỉ cần nhận ra chúng, cơ chế GOT/relocation nằm ở [P11·06](../11-do-dac-va-go-loi/06-elf-linker-loader.md).

</details>

**Quay lại Q3:** x86-64 hai toán hạng và cho toán hạng bộ nhớ; A64 thường ba toán hạng và chỉ các lệnh load/store (như `ldr`/`str`) chạm RAM.

## 4. Condition flags, nhảy có điều kiện và `cmov`/`csel` (Q4)

> **Trả lời ngắn.** `cmp`/`test` không ghi kết quả vào thanh ghi đích mà đặt các cờ điều kiện[^x86cmp]; lệnh nhảy có điều kiện đọc các cờ đó[^sdmflags]. Hậu tố điều kiện của lệnh nhảy quyết định so sánh có dấu hay không dấu[^jsuffix]. `cmov`/`csel` chọn giá trị mà không nhảy, tránh gián đoạn pipeline do nhánh[^cmov][^brexp], nhưng compiler thận trọng khi có tác dụng phụ[^cmovcare] và chúng không phải lúc nào cũng nhanh hơn nhánh ([Chương 05, mục 7](05-pipeline-va-speculation.md)).

**Giải thích.**

Hình dung (nhận xét của người soạn): `cmp` như bật một đèn hiệu; lệnh nhảy kế tiếp nhìn đèn rồi mới rẽ. `cmov` như chuẩn bị sẵn cả hai đáp án rồi chọn một, không rẽ hướng.

Hai họ kiến trúc đều có **thanh ghi cờ** do lệnh so sánh/số học ghi và lệnh nhảy/chọn đọc. DiS cho x86-64: `cmp` và `test` không sửa thanh ghi đích; chúng sửa một loạt giá trị một bit gọi là condition code flags (cờ điều kiện)[^x86cmp]; các cờ nằm trong FLAGS register[^x86flagsreg]. Intel SDM: các lệnh Jcc dùng trạng thái của các cờ trạng thái trong thanh ghi EFLAGS làm điều kiện để rẽ nhánh[^sdmflags]. Với A64 cờ nằm trong PSTATE (trạng thái bộ xử lý ARM)[^pstate], cập nhật bởi `cmp`, `tst` và biến thể `s` của lệnh số học (`adds`, `subs`; LLVM dùng chính `ADDS`/`SUBS` để lấy cờ tràn và carry, như `UADDO` → `ADDS` rồi điều kiện `HS`[^llvmusubo]). Về cấu tạo lệnh, `cmp` là tên khác của `subs` còn `tst` là tên khác của `ands`, cả hai với đích là thanh ghi zero để bỏ kết quả: Arm nói `CMP` được mã hoá thành `SUB` có đích là thanh ghi zero[^armzr], LLVM ghi cụ thể `cmp reg, #0` là `subs xzr, reg, #0`[^llvmcmp], và định nghĩa alias của LLVM biến `tst` thành `ands` với đích `xzr`/`wzr`[^llvmtst].

| Cờ | x86-64 | AArch64 | Dùng cho |
|---|---|---|---|
| Bằng/không | ZF | Z | `==`, `!=` |
| Âm (dấu) | SF | N | so sánh có dấu (cùng OF/V) |
| Tràn có dấu | OF | V | so sánh có dấu |
| Nhớ (carry) | CF (sau phép trừ: 1 khi *có* borrow) | C (sau phép trừ: 0 khi *có* borrow) | so sánh không dấu |

Nguồn cho bảng cờ: tên và nghĩa các cờ x86-64[^x86flagtab] và A64[^armflagtab]; vai trò có dấu/không dấu ở đoạn dưới, và hàng carry khi trừ được giải thích ngay sau đó.

Phân vai theo DiS: x86 cờ SF và OF dùng cho phép so sánh trên số nguyên có dấu, còn cờ CF dùng cho so sánh trên số nguyên không dấu[^x86flagsuse]; A64: V hữu ích cho so sánh có dấu, C hữu ích cho so sánh không dấu[^armflags]. Thứ tự toán hạng của `cmp`: DiS viết `cmp O1, O2` của A64 là tính O1 - O2[^armcmp], còn `cmp R1, R2` của x86 là trừ R1 khỏi R2, tức tính R2 - R1[^x86cmpord]; vì vậy `cmpq %rsi, %rdx` (AT&T, mục 1) tính `rdx - rsi`. **Carry khi trừ: hai ISA ngược nhau.** Trên x86-64, CF được đặt khi phép tính số học tạo một carry hoặc borrow ra khỏi bit cao nhất[^sdmcf], nên sau một phép trừ `a - b` (như `cmp`) thì CF=1 khi `a < b` không dấu, và `jae` nhảy khi CF=0, `jb` khi CF=1[^sdmjae] (suy luận của người soạn từ định nghĩa borrow). Trên A64, source LLVM ghi điều kiện `HS` ứng với C == 1 và `LO` ứng với C == 0[^llvmnzcv], và `usub.with.overflow` (borrow) được lower thành `subs` với điều kiện `LO`[^llvmusubo], tức borrow ứng với C = 0, hay C là NOT borrow; LLVM cũng ghi rõ SBCS dùng carry chứ không phải borrow[^llvmsbcs]. `HS` là cao hơn hoặc bằng, không dấu[^llvmcc].

**Hậu tố điều kiện.** Lệnh nhảy x86 bắt đầu bằng `j`, hậu tố của mỗi lệnh cho biết điều kiện của lệnh nhảy, và cũng quyết định việc diễn giải so sánh số là có dấu hay không dấu[^jsuffix]. A64 tương tự với `b.` + hậu tố: hậu tố cũng quyết định có dấu hay không dấu, và cùng bộ hậu tố dùng cho `csel`[^bsuffix].

| Điều kiện | x86[^jsufftab] | A64[^bsufftab] | Dấu |
|---|---|---|---|
| `==` | `je` | `b.eq` | — |
| `!=` | `jne` | `b.ne` | — |
| `<` có dấu | `jl` | `b.lt` | signed |
| `>` có dấu | `jg` | `b.gt` | signed |
| `>=` unsigned | `jae` | `b.hs` (còn gọi `b.cs`)[^llvmalias] | unsigned |
| `>` unsigned | `ja` | `b.hi` | unsigned |

**Dạng chọn.** Với `if a > b { a } else { b }` không có tác dụng phụ, LLVM sinh lệnh chọn (quan sát ở output dưới):

```asm
# fn max(a: i64, b: i64) -> i64 { if a > b { a } else { b } }
max:                          _max:                    # AArch64
    movq  %rsi, %rax              cmp   x0, x1
    cmpq  %rsi, %rdi              csel  x0, x0, x1, gt   // x0 = (gt) ? x0 : x1
    cmovgq %rdi, %rax             ret
    retq
```

DiS giải thích: `cmov` thực hiện một phép chuyển dữ liệu có điều kiện: cả then_statement lẫn else_statement của điều kiện đều được thực thi, và dữ liệu được đặt vào thanh ghi phù hợp[^cmov]; `csel` tương tự cho A64[^csel]. Động cơ là nhánh thường rất tốn kém để thực thi, do sự gián đoạn chúng có thể gây ra cho instruction pipeline (DiS nói câu này ở phần A64)[^brexp] — cơ chế đó giải thích kỹ ở [Chương 05](05-pipeline-va-speculation.md), kể cả vì sao branchless không phải lúc nào cũng nhanh hơn (mục 7 của chương đó). Cũng theo DiS, compiler rất thận trọng khi tối ưu lệnh nhảy thành lệnh cmov, đặc biệt trong trường hợp có tác dụng phụ và giá trị con trỏ[^cmovcare]; ví dụ `br` ở mục 5 là trường hợp có tác dụng phụ nên giữ nhánh (nhận xét của người soạn).

**Ví dụ `collatz`.** Với `n = if n % 2 == 0 { n / 2 } else { 3*n + 1 }`, cả hai ISA tính trước cả hai vế rồi chọn bằng lệnh có điều kiện (mã chạy lại với rustc 1.95.0 `-O`). x86-64: `leaq (%rdi,%rdi,2), %rax; incq %rax` (`rax = 3n+1`), `movq %rdi, %rcx; shrq %rcx` (`rcx = n/2`), `testb $1, %dil`, rồi `cmoveq %rcx, %rax` (chẵn thì lấy `n/2`). AArch64: `lsr x8, x0, #1` (`x8 = n/2`), `add x9, x0, x0, lsl #1` (`x9 = 3n`), `tst x0, #0x1`, rồi `csinc x0, x8, x9, eq`. `csinc Rd, Rn, Rm, cond` cho `Rn` khi `cond` đúng, ngược lại `Rm + 1`: Arm gọi nhóm này là Conditional Select and Increment[^armcsinc], và LLVM định nghĩa `csinc` là phép chọn có điều kiện trên `Rm + 1` (`inc` là phép cộng 1)[^llvmcsinc]; tôi đã chạy thật `cmp x, #5; csinc r, p, q, eq` với `p=100`, `q=200`: `x=5` cho `100`, `x=6` cho `201`. Vì vậy `eq` (số chẵn, bit 0 = 0) chọn `n/2`, còn lại `3n + 1`: bước `+1` của vế lẻ được gộp vào chính lệnh chọn.

<details><summary>Đi sâu: carry khi trừ, bảng điều kiện đầy đủ, <code>cbz</code> và tầm nhảy</summary>

Tôi đã chạy thật `subs xzr, a, b` rồi `cset r, hs`: `a=5, b=3` cho `1`; `a=3, b=5` cho `0`. Vậy `b.hs` (C=1) và `jae` (CF=0) cùng nghĩa "a ≥ b không dấu" dù giá trị cờ ngược nhau. Đây là kết luận từ source LLVM[^llvmnzcv] và thí nghiệm, không phải từ Arm ARM (403 lúc soạn). Với `adds`/`add`, carry-out của cả hai ISA cùng chiều (C/CF=1 khi tràn không dấu; `UADDO` được LLVM map sang `HS`[^llvmusubo]).

Trong output: `jae`/`b.hs` ở hàm `load_idx` cho kiểm tra `i >= len` kiểu không dấu (chỉ số `usize`), còn `b.lt` ở `wh` là so sánh `i32` có dấu — khớp quy tắc "có dấu ↔ l/g/lt/gt/ge/le, không dấu ↔ a/b/hi/hs/lo/ls" (đã thấy trong các output; DiS ghép cặp có dấu/không dấu của x86 là `jg`/`ja`, `jge`/`jae`, `jl`/`jb`, `jle`/`jbe`[^jsufftab] và của A64 là `gt`/`hi`, `ge`/`hs`, `lt`/`lo`, `le`/`ls`[^bsufftab]). Bảng điều kiện x86 đầy đủ ở SDM Vol. 1 Table 7-4[^sdmjae]; phía A64, tên và nghĩa các điều kiện nằm trong `enum CondCode` của LLVM, nơi `HS` và `LO` có alias `cs` và `cc` ở assembler[^llvmcc][^llvmalias]; nghĩa của `hi`, `ls`, `ge`, `lt`, `gt` ở phần còn lại của enum đó[^llvmcc2].

**Nhảy theo thanh ghi, không qua cờ.** A64 có thêm `cbz`/`cbnz`: nếu R bằng 0, nhánh được thực hiện và pc được đặt thành addr (`cbz`)[^cbz], như `cbz x1, LBB7_3` ở đầu `sum` ("len == 0"; dòng này không nằm trong đoạn trích ở mục 5). **Tầm nhảy**: `b` đổi `pc` trong phạm vi 128 MB[^b128], còn nhánh điều kiện chỉ có tầm hạn chế hơn nhiều (1 MB)[^brange]. Hệ quả: AAPCS64 nói các lệnh branch A64 không với tới mọi đích, nên linker có thể phải chèn một veneer (đoạn mã nối trung gian) giữa hàm gọi và hàm được gọi[^aapcsveneer].

</details>

**Quay lại Q4:** cờ do `cmp`/`test`/lệnh `s` đặt và lệnh nhảy/chọn đọc; hậu tố cho biết có dấu hay không; `cmov`/`csel` chọn không cần nhánh.

## 5. Dịch `if`, `while`, `for`, `match` (Q5)

> **Trả lời ngắn.** Compiler dịch điều kiện thành lệnh sửa con trỏ lệnh để nhảy tới một địa chỉ không phải lệnh kế tiếp[^armcond]; công cụ cụ thể là nhảy không điều kiện, so sánh đặt cờ rồi nhảy có điều kiện, hoặc chọn có điều kiện không nhảy (mục 4). `if` có tác dụng phụ giữ dạng nhánh[^cmovcare]; trong ví dụ C của DiS, vòng `for` cho assembly giống hệt vòng `while`[^forwhile], còn Rust định nghĩa `for` qua `IntoIterator` và `loop`[^reffor]. `match` trả hằng được LLVM đổi thành bảng tra, và lời gọi ở vị trí đuôi có thể thành `jmp`/`b` (cả hai là quan sát).

**Giải thích.**

Hình dung (nhận xét của người soạn): đường rẽ trong khu dân cư: `if` là ngã ba, vòng lặp là vòng xuyến, `match` là bảng chỉ dẫn tra số để đến thẳng nhà.

Nguyên lý chung (tổng hợp của người soạn từ mục 4): compiler dùng bốn công cụ — nhảy không điều kiện (`jmp`/`b`), so sánh đặt cờ, nhảy có điều kiện (`jcc`/`b.cond`), và chọn có điều kiện không nhảy (`cmov`/`csel`). DiS: compiler dịch các điều kiện thành lệnh assembly sửa instruction pointer (pc)[^armcond].

**Dạng nhánh.** Khi hai nhánh gọi hàm khác nhau (có tác dụng phụ), compiler không thể "chạy cả hai", nên nó giữ nhánh (nhận xét của người soạn; DiS cũng lưu ý compiler thận trọng với tác dụng phụ[^cmovcare]). Hàm thật:

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

Mỗi bên kiểm tra dấu của `x` theo cách riêng: x86 `test` + `js` (nhảy khi cờ dấu SF=1[^sdmjs]), A64 `tbnz w0, #31` (test bit and branch if non-zero: nhảy khi bit 31, tức bit dấu của `w0`, bằng 1). Nguồn (chi tiết cài đặt của LLVM 22.1.0, có thể đổi): khi lower phép so sánh `x < 0` (`SETLT` với 0), LLVM sinh `TBNZ` trên vị trí bit dấu[^llvmtbnz], và `tbnz` tương đương `tst` bit đó rồi rẽ nhánh khi kết quả khác 0 (`ands xzr, reg, #1<<bit` với điều kiện `NE`)[^llvmtbnzsem]. Tôi cũng chạy thật `tbnz w, #31, label`: nhảy với `w = -3`, không nhảy với `w = 3`. Điều đáng nhớ hơn: **không có `call`/`bl`/`ret`**. Lời gọi `f()`/`g()` nằm ở vị trí đuôi (tail position) nên compiler thay bằng nhảy `jmp`/`b` — hàm đích sẽ `ret` thẳng về caller của `br`. Đây là tối ưu của compiler, không phải bảo đảm ngôn ngữ (nhận xét của người soạn); tail call được nói riêng ở [Chương 04, mục 7](04-calling-convention-va-stack-frame.md).

**`while`, `for`**

DiS: một vòng `for` cho ra assembly giống hệt ví dụ vòng while của họ[^forwhile] (ví dụ C `sumUp`/`sumUp2` của DiS) — nên với ví dụ đó chỉ còn một khuôn: *khởi tạo → (kiểm tra → thân → bước nhảy) → thoát*. Trong Rust, Reference định nghĩa `for` là tương đương một `loop` chứa `match` trên `Iterator::next(&mut iter)`, với `iter` lấy từ `IntoIterator::into_iter(iter_expr)`[^reffor]; assembly cuối cùng của `for` và `while` có giống nhau hay không là việc của optimizer, nên chương này chỉ nói về các output `-O` đã dán (nhận xét của người soạn).

Hàm `wh(mut n: i32) { while n > 0 { f(); n -= 1; } }` (với `f` là hàm ngoài) cho, trên AArch64 (đã bỏ `stp`/`ldp`):

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

**`match` / `switch`**

```rust
pub fn sw(x: u32) -> u32 { match x { 0 => 10, 1 => 20, 2 => 35, 3 => 7, 4 => 99, _ => 0 } }
```

Hai kiến trúc cho cùng một chiến lược — **bảng tra (lookup table)** chứ không phải chuỗi so sánh (quan sát ở output dưới):

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

Nhánh `_ =>` trở thành một lần so sánh miền (`x > 4` unsigned) rồi nhảy; phần còn lại là một lệnh load đánh chỉ số vào bảng 5 phần tử `.long` (4 byte). Khi mỗi nhánh *trả về một hằng số*, LLVM biến cả `match` thành bảng dữ liệu (quan sát). Với thân phức tạp hơn tôi không chạy ví dụ nên không khẳng định hình dạng.

<details><summary>Đi sâu: hình dạng vòng <code>while</code> và vector hoá</summary>

Hình dạng: một **guard** trước vòng lặp (`n <= 0` thì không vào), rồi thân vòng ở nhãn `LBB3_2` nằm *trước* phép kiểm tra cuối (bottom-tested loop), nên mỗi lượt chỉ cần một nhảy có điều kiện ngược. Đây là khác với khuôn "nhảy xuống kiểm tra trước, rồi quay lại" mà DiS minh hoạ cho `sumUp` (`jmp <sumUp+33>` rồi `jle` ngược)[^loopjmp]; biến của ví dụ DiS nằm trên stack (`-0x4(%rbp)`), nên trông là mã chưa tối ưu (suy luận của người soạn). Bộ đếm được biến đổi thành đếm xuống (`n+1`, giảm, so với 1); `ja`/`b.hi` là nhảy *unsigned* vì LLVM đã chứng minh giá trị không âm (suy luận của người soạn).

**Vòng lặp có truy cập mảng và vector hoá**

`sum(p: &[i64]) -> i64` với `for i in 0..p.len() { s += p[i] }` ở `-O` được **vector hoá**: thân scalar vẫn còn (xử lý phần dư), nhưng có thêm vòng chính dùng thanh ghi SIMD (A64 có 32 thanh ghi SIMD/FP `v0`–`v31`[^aapcssimd]). Trích phần cốt lõi:

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

Trên x86-64, vòng chính dùng `movdqu`/`paddq` với `%xmm` (128-bit; output chỉ chứa thanh ghi `%xmm`, quan sát). Trong vòng không còn bounds check — compiler đã chứng minh `i < len` (suy luận của người soạn; [Chương 07, mục 3](07-rust-va-may.md) phân tích kỹ việc compiler bỏ hoặc đưa bounds check ra ngoài vòng) — và `subs … ; b.ne` là mẫu "đếm xuống đến 0" của A64.

</details>

**Quay lại Q5:** `if` thành nhánh khi có tác dụng phụ; `for` và `while` chung khung trong ví dụ C của DiS (Rust định nghĩa `for` qua iterator); `match` trả hằng thành bảng tra; output không dịch từng dòng.

## 6. Gọi hàm: `call`/`bl` và `ret` (Q6)

> **Trả lời ngắn.** `callq` cất địa chỉ trả về (`%rip` kế tiếp) lên stack[^callx86]; `bl` ghi `pc+4` vào thanh ghi `x30` (LR)[^blarm]. `ret` trên x86-64 nạp `%rip` từ stack[^retx86], còn trên A64 lấy `pc = x30`[^retarm].

**Giải thích.**

Hình dung (nhận xét của người soạn): x86 để mảnh giấy ghi đường về vào ngăn kéo chung (stack); A64 giữ mảnh giấy trong túi áo (`x30`), nên gọi tiếp lần nữa thì phải cất giấy cũ đi.

Hai ISA khác nhau ở **nơi cất địa chỉ trả về**:

| | x86-64 | AArch64 |
|---|---|---|
| Gọi | `callq addr`: cất `%rip` kế tiếp lên stack[^callx86] | `bl addr`: ghi `pc+4` vào `x30` (LR)[^blarm] |
| Về | `retq`: nạp `%rip` từ stack[^retx86] | `ret`: `pc = x30`[^retarm] |
| Địa chỉ trả về nằm ở | bộ nhớ (stack) | thanh ghi |

DiS: khi hàm gọi thực thi lệnh callq, giá trị hiện tại của %rip được lưu trên stack để biểu diễn địa chỉ trả về[^callx86]; khi hàm gọi thực thi lệnh bl, giá trị pc + 4 được lưu trong thanh ghi x30[^blarm]; AAPCS64 diễn đạt cùng ý: BL chuyển giá trị kế tiếp theo thứ tự của program counter — tức địa chỉ trả về — vào link register (LR)[^aapcsbl].

Hệ quả thấy ngay trong code của `caller(x) = callee(x) + 1` (cả hai `#[inline(never)]`/`no_mangle`):

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

`callee` là `x.wrapping_mul(31)`: cả hai ISA thay phép nhân bằng dịch bit rồi trừ (`x*32 - x`). `caller` là *non-leaf* (gọi hàm khác): trên A64 `bl` ghi đè `x30` còn `ret` lấy địa chỉ từ `x30`, nên prologue phải cất `x30` lên stack bằng `stp` rồi `ldp` khôi phục trước `ret` (suy luận của người soạn từ hai định nghĩa ở bảng trên); còn trên x86 `call` tự đẩy địa chỉ trả về lên stack nên `pushq %rax` chỉ để căn lề stack (suy luận của người soạn; xem Đi sâu). Hàm *leaf* như `callee` không cần cất gì. Bố cục stack frame và danh sách thanh ghi caller-/callee-saved nằm ở [Chương 04](04-calling-convention-va-stack-frame.md).

<details><summary>Đi sâu: căn lề stack ở x86-64 và AArch64</summary>

Căn lề stack ở x86-64: System V psABI nói stack cần căn 16 (32 hoặc 64) byte ngay trước khi lệnh `call` được thực thi, và ngay khi vào hàm (địa chỉ trả về vừa được đẩy) thì `%rsp + 8` là bội của 16[^sysvalign]. Vì vậy khi vừa vào `caller`, `%rsp` lệch 8 so với bội của 16, và `pushq` 8 byte đưa nó về bội của 16 trước `callq` (suy luận của người soạn từ câu trích). Phía AArch64, AAPCS64 đòi `SP` chia hết cho 16 mỗi khi truy cập bộ nhớ qua SP (phần cứng yêu cầu)[^aapcsalign]; `stp x29, x30, [sp, #-16]!` ở trên trừ SP đúng 16 nên giữ được điều đó (suy luận của người soạn). Quy tắc này, red zone và prologue/epilogue được nói kỹ ở [Chương 04, mục 4 và 6](04-calling-convention-va-stack-frame.md).

</details>

**Quay lại Q6:** x86-64 cất địa chỉ trả về lên stack, A64 cất vào `x30`.

## 7. Sinh và đọc assembly do `rustc` tạo ra (Q7)

> **Trả lời ngắn.** Sinh asm bằng `rustc -O --emit=asm`[^emit] (`-O` là `opt-level=3`[^opt]); `--target` chọn target tuple, tức kiến trúc và hệ điều hành đích[^target]. Output là kết quả của LLVM chứ không dịch từng dòng. Khi đọc: lọc directive, tìm nhánh tới `core::panicking` (bounds check[^bounds]), so `opt-level=0` với `-O` để thấy overflow check[^ovf], và để ý thanh ghi SIMD trong thân vòng để nhận ra vector hoá (Đi sâu ở mục 5).

**Giải thích.**

Hình dung (nhận xét của người soạn): đọc asm như đọc phim X-quang: không thấy tên biến, nhưng thấy hình dáng xương (nhánh, vòng lặp, bảng).

Mọi ví dụ trong chương được sinh bằng `rustc 1.95.0` trên máy `aarch64-apple-darwin`; rustc 1.95.0 dùng LLVM 22[^rustllvm], nên các trích dẫn mã LLVM trong chương ghim ở tag 22.1.0 (bản LLVM đi kèm rustc có thể khác vài chi tiết). Phần x86-64 sinh bằng cross-target `x86_64-unknown-linux-gnu`: `--emit=asm` không yêu cầu loại output `link`, vốn chỉ là mặc định khi không có `--emit`[^emitlink], nên không có bước liên kết và không cần linker Linux (suy luận của người soạn từ câu đó). Lệnh:

```sh
rustc -O --emit=asm a.rs -o arm.s --target aarch64-apple-darwin
rustc -O --emit=asm a.rs -o x86.s --target x86_64-unknown-linux-gnu
```

`--emit=asm` tạo ra một file chứa assembly của crate[^emit]; `-O` là bí danh của `-C opt-level=3`[^opt]. File nguồn bắt đầu bằng `#![crate_type="lib"]` và mỗi hàm có `#[no_mangle] pub fn` để tên symbol giữ nguyên (`add` thay vì `_RNv…`)[^nomangle]. Tôi cũng xoá các directive nhiễu (`.cfi_*`, `.p2align`, `.size`, `.section`…) khỏi bản dán bên dưới và đã ghi chú mỗi đoạn "rút gọn" — các lệnh còn lại là nguyên văn. Trong file có hai kiểu nhãn: `LBBn_m` (macOS, có thể có thêm tiền tố `_` ở tên hàm) và `.LBBn_m` (ELF/Linux); đó là chuyện của object format chứ không phải của ISA (quan sát).

Output là **kết quả của LLVM ở `-O`**, không phải dịch từng dòng; `opt-level=0` cho code khác hẳn (xem bước 3 bên dưới). Phiên bản rustc/LLVM khác có thể sinh khác (nhận xét của người soạn).

1. **Lọc nhiễu.** Directive là chỉ thị cho assembler, tên bắt đầu bằng `.`[^gasdirective]; ví dụ `.cfi_*` (mở và đóng mục unwind của hàm trong `.eh_frame`[^gascfi]), `.p2align`, `.section`, `.size`, `.loh` (chỉ thấy ở output macOS, quan sát). Nhãn là một symbol kết thúc bằng `:`[^gaslabel] (nhãn cục bộ như `.LBB5_2:` cũng bắt đầu bằng `.` nhưng có `:`; quan sát), và phần lệnh là những dòng còn lại.
2. **Tìm các "dấu vân tay" của Rust.** Một nhánh dẫn tới `…core9panicking…` (tên symbol của `core::panicking`, quan sát) là *panic*. Ví dụ `load_idx` có nhánh `cmp x2, x1 ; b.hs LBB5_2` rồi `bl …panic_bounds_check`: đây chính là kiểm tra chỉ số lúc chạy mà Reference nói: một kiểm tra sẽ được thực hiện lúc chạy và đưa thread vào trạng thái panic nếu nó thất bại[^bounds]. Đường nhanh (`ldr w0, [x0, x2, lsl #2]`) chỉ có `cmp` + một nhánh chưa lấy.
3. **Đọc debug và release khác nhau.** Reference: toán tử số nguyên sẽ panic khi tràn nếu biên dịch ở chế độ debug[^ovf]; rustc book: nếu không chỉ định, debug assertions tự bật chỉ khi `opt-level` là 0[^debugassert], và overflow checks bật khi debug assertions bật[^ovfchecks]. Vì vậy so asm của cùng một hàm ở `opt-level=0` và `-O` (`-O` là `opt-level=3`) cho thấy overflow check: ở `-O` kiểm tra không được phát ra, chứ không phải optimizer "chứng minh" không tràn (suy luận của người soạn từ hai câu trên). Listing nằm ở Đi sâu bên dưới; [Chương 07, mục 1](07-rust-va-may.md) đối chiếu cùng cặp `-O0`/`-O3` cho một hàm `u64`.
4. **Nhận ra vector hoá.** Vòng lặp mà thân xử lý nhiều phần tử mỗi lượt bằng thanh ghi SIMD đã được vector hoá: trên A64 là các thanh ghi SIMD `v0`–`v31`[^aapcssimd] (trong output xuất hiện dưới tên `q4`, `q5`, `v0`…; quan sát), trên x86-64 là `%xmm…`. Ví dụ `sum` ở Đi sâu của mục 5 có `ldp q4, q5` và `add.2d` ở A64, `movdqu`/`paddq` ở x86-64 (quan sát).

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

Ở `-O` cùng hàm chỉ còn một lệnh (`add x0, x1, x0` / `leaq`) vì kiểm tra không còn được phát ra. Cờ `V`/`OF` ở đây là "tràn có dấu" của bảng mục 4: `adds … cset vs` đọc cờ V ngay sau lệnh cộng; ở x86-64 lệnh `jo` nhảy khi OF = 1[^sdmjs]. Chương này không dán output x86-64 ở `opt-level=0` nên không khẳng định dạng lệnh cụ thể ở đó (suy luận của người soạn: tương tự, đọc cờ OF sau `addq`).

</details>

**Quay lại Q7:** sinh bằng `--emit=asm`, lọc directive, tìm nhánh panic, so debug với release để thấy overflow check, và nhận ra vector hoá qua thanh ghi SIMD.

## Liên hệ với Rust

- Lệnh sinh asm: `rustc --emit=asm`[^emit]; `--target` chọn target tuple[^target]. Cargo: `cargo rustc --release -- --emit=asm` (nhận xét của người soạn, chưa chạy).
- `-O` = `opt-level=3`[^opt]; việc kiểm tra tràn số có thể điều khiển trực tiếp bằng `-C overflow-checks` / `-C debug-assertions`[^ovfflags], mặc định của chúng phụ thuộc `opt-level` như ở mục 7.
- Rust ABI không ổn định[^rustabi], nên nếu viết `asm!` hoặc gọi vào code ngoài, dùng `extern "C"` để bám vào ABI mặc định của compiler C cho target[^rustabic] (chi tiết ở [Chương 04, mục 8](04-calling-convention-va-stack-frame.md)).
- `asm!` trong Rust dùng Intel syntax mặc định trên x86, `att_syntax` để đổi[^rustasm][^rustatt]. Cùng câu trong Reference còn nhắc chế độ `.syntax unified` cho "ARM" nói chung; chương không dùng nó để khẳng định gì về AArch64 (nhận xét của người soạn).

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

`movl` ghi 32-bit, SDM: kết quả được zero-extend lên 64-bit nên bit 63–32 thành 0[^sdm32]; `movb` ghi 8-bit và 56 bit cao của đích không đổi[^sdm816].

</details>

2. Chuyển `addl $4, %eax` sang Intel syntax.
<details><summary>Đáp án</summary>

`add eax, 4` (ví dụ của chính tài liệu GNU as)[^gasorder].

</details>

3. Vì sao `cmpq %rsi, %rdx` rồi `jae` tương ứng với `rdx >= rsi` (`i >= len`) mà không phải `rsi >= rdx`?
<details><summary>Đáp án</summary>

AT&T có nguồn trước đích[^gasorder]; `cmp` trừ toán hạng đứng trước khỏi toán hạng đứng sau: DiS viết `cmp R1, R2` của x86 là tính R2 - R1[^x86cmpord] (còn `cmp O1, O2` của A64 tính O1 - O2[^armcmp]). Với `cmpq %rsi, %rdx` đó là `rdx - rsi`, và `jae` nhảy khi CF = 0, tức không borrow: `rdx >= rsi` không dấu[^sdmjae].

</details>

4. `ldr w0, [x8, w0, uxtw #2]` đọc từ địa chỉ nào?
<details><summary>Đáp án</summary>

`x8 + (zext(w0) << 2)`: chỉ số 32-bit zero-extend rồi nhân 4 — là dạng "scaled/extended" trong bảng addressing mode (đã chạy thật, xem mục 3; dạng `sxtw` tương tự có trong DiS)[^armscaled].

</details>

5. Khi nào compiler dùng `cmov`/`csel` thay vì nhánh, và vì sao?
<details><summary>Đáp án</summary>

Khi hai nhánh chỉ chọn giá trị, không tác dụng phụ; `cmov` thực hiện "chuyển dữ liệu có điều kiện" thay vì "chuyển điều khiển có điều kiện" để tránh phí nhánh[^cmov][^brexp][^cmovcare]. Không phải lúc nào cũng nhanh hơn nhánh: xem [Chương 05, mục 7](05-pipeline-va-speculation.md).

</details>

6. Phân biệt `jl` và `jb` (x86) / `b.lt` và `b.lo` (A64).
<details><summary>Đáp án</summary>

Hậu tố quyết định so sánh có dấu hay không dấu[^jsuffix]; có dấu dùng SF/OF (N/V), không dấu dùng CF (C)[^x86flagsuse][^armflags].

</details>

7. Vì sao `caller` trên AArch64 phải `stp x29, x30, …` còn `callee` thì không?
<details><summary>Đáp án</summary>

`bl` ghi địa chỉ trả về vào `x30`[^blarm] và `ret` lấy lại `pc = x30`[^retarm]; hàm non-leaf gọi `bl` sẽ ghi đè `x30` của chính nó, nên phải cất rồi khôi phục trước `ret` (suy luận của người soạn từ hai định nghĩa đó). `callee` là leaf, không gọi ai.

</details>

8. Vì sao `br(x)` không có `call`/`bl` hay `ret`?
<details><summary>Đáp án</summary>

Hai lời gọi nằm ở vị trí đuôi nên compiler thay bằng nhảy; hàm đích `ret` thẳng về caller của `br` (quan sát từ output; tối ưu của compiler, không phải bảo đảm của Rust).

</details>

9. Hai dấu hiệu nào trong output `opt-level=0` của `a + b` cho thấy overflow check?
<details><summary>Đáp án</summary>

AArch64: `adds` (cộng và đặt cờ) rồi `cset w8, vs` đọc cờ tràn và `tbnz` tới `panic_const_add_overflow`. Reference: debug build panic khi tràn[^ovf]; rustc book: debug assertions và overflow checks mặc định bật ở `opt-level=0`[^debugassert][^ovfchecks]. Với x86-64 chương không dán output ở `opt-level=0`; việc đọc cờ OF bằng `jo`[^sdmjs] là suy luận của người soạn.

</details>

10. Sau `cmp a, b` với `a >= b` không dấu, cờ carry bằng bao nhiêu trên x86-64 và trên AArch64?
<details><summary>Đáp án</summary>

x86-64: CF = 0 (không borrow), nên `jae` (CF = 0) nhảy[^sdmjae]. AArch64: C = 1, nên `b.hs` (ứng với C == 1) nhảy[^llvmnzcv]. Hai cờ ngược giá trị nhưng cùng nghĩa "a ≥ b không dấu" (mục 4).

</details>

11. `cmp x0, #0` trên AArch64 thực chất là lệnh gì, và vì sao dùng `xzr`?
<details><summary>Đáp án</summary>

Là `subs xzr, x0, #0`[^llvmcmp]. Arm nói thanh ghi zero luôn đọc ra 0, ghi vào bị bỏ qua, và `CMP` được mã hoá thành `SUB` với đích là thanh ghi zero[^armzr] — chỉ cờ được cập nhật, kết quả bị bỏ.

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
