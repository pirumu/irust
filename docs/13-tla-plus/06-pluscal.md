# P13 · Chương 06 — PlusCal: viết thuật toán rồi dịch sang TLA+

> Nguồn chính là *PlusCal User's Manual* của Lamport, bản 1.8 (ngày ghi trên bìa: 11 March 2024), có hai phiên bản cú pháp: P-syntax và C-syntax. Mọi spec trong chương này là **minh họa của người soạn**: không chạy TLC, không chạy translator; chương không khẳng định kết quả model checking, trừ chỗ nguồn nói rõ. Bản dịch TLA+ của ví dụ do người soạn viết tay theo quy tắc trong manual (Phụ lục B) và đối chiếu với bản dịch thật trong repo `tlaplus/Examples`; translator thật có thể khác về định dạng. Trang Learn TLA+ là trang sống (truy cập 2026-10-12), không ghim phiên bản.

## Bạn sẽ hiểu được gì

- **Q1.** PlusCal là gì, nó khác TLA+ thuần ở đâu, và một file PlusCal (module, comment, vùng `BEGIN/END TRANSLATION`, P-syntax lẫn C-syntax) trông thế nào? → mục 1
- **Q2.** "Label" là gì, và vì sao "bước nguyên tử" của PlusCal là đoạn code giữa hai label? → mục 2
- **Q3.** Label bắt buộc phải đặt ở đâu, cấm đặt ở đâu, và vì sao chọn chỗ đặt label là một quyết định mô hình hóa? → mục 3
- **Q4.** `process` đơn và tập process khác nhau thế nào; `self`, biến local và `fair process` làm gì? → mục 4
- **Q5.** `await`/`when`, `either`/`or` và `with` biểu diễn việc chờ và việc chọn không tất định ra sao; deadlock xuất hiện thế nào? → mục 5
- **Q6.** `macro` khác `procedure` ở đâu; `call`/`return` và biến `stack` hoạt động thế nào? → mục 6
- **Q7.** PlusCal được dịch sang TLA+ như thế nào (`pc`, `Init`, một action cho mỗi label, `Next`, `Spec`, `stack`)? → mục 7
- **Q8.** Hai thread cùng tăng một counter không khóa gây lỗi lost update thế nào, và sửa bằng lock ra sao khi viết bằng PlusCal? → mục 8

## Kiến thức cần trước

- [Chương 02 — Toán nền tảng](02-toan-nen-tang.md): `/\`, `\/`, `\in`, hàm `[x \in S |-> e]`, `EXCEPT`, record, sequence (`Head`, `Tail`, `\o`). Mọi đoạn TLA+ trong chương này (nhất là bản dịch ở mục 6–8) dùng đúng những ký hiệu đó và chương này không giải thích lại.
- [Chương 03 — Trạng thái, hành vi, action](03-trang-thai-hanh-vi-action.md): state, behavior, action, `Init`/`Next`/`Spec`, `[][Next]_vars`; chương này dịch PlusCal sang đúng những thứ đó.
- [Chương 04 — Invariant và TLC](04-invariant-va-tlc.md): invariant, model, TLC, đọc counterexample; chương này dùng lại khi kiểm tra các ví dụ.
- [Chương 05 — Liveness và fairness](05-liveness-va-fairness.md): weak/strong fairness; mục 4 và mục 7 chỉ nêu phần PlusCal sinh ra, lý thuyết nằm ở đó.
- [Phase 4 · Chương 03 — Lock](../04-concurrency/03-lock.md), mục 1: race condition `counter += 1` ở mức máy; mục 8 của chương này mô hình hóa đúng kịch bản đó.
- [Phase 4 · Chương 05 — Bug concurrency và deadlock](../04-concurrency/05-concurrency-bug-va-deadlock.md): deadlock; mục 5 dùng lại khái niệm.

## 1. PlusCal là gì và một file PlusCal trông thế nào (Q1)

> **Trả lời ngắn.** PlusCal là một *ngôn ngữ thuật toán*: ngôn ngữ để viết thuật toán, không phải chương trình[^pm-algo]. Bạn viết thuật toán bằng cú pháp giống code mệnh lệnh; translator biên dịch nó thành một đặc tả TLA+[^pm-translator]; TLC duyệt mọi thực thi có thể của đặc tả đó[^pm-tlc]. Biểu thức bên trong thuật toán vẫn là TLA+ nguyên bản[^pm-expr].

**Giải thích.**

Hình dung (nhận xét của người soạn): TLA+ thuần như viết thẳng máy trạng thái — mỗi hành động là một công thức về "trạng thái trước / trạng thái sau". PlusCal cho bạn viết cùng máy trạng thái đó dưới dạng pseudo-code có `while`, `if`, nhiều thread; translator làm việc của một compiler, sinh ra bản TLA+ mà bạn vẫn đọc được.

*Vì sao cần một "ngôn ngữ thuật toán".* Manual nêu ba điểm khác giữa thuật toán và chương trình. Một: thuật toán thao tác trên đối tượng toán học tùy ý (manual lấy ví dụ đồ thị và không gian vector), còn chương trình chỉ thao tác trên đối tượng đơn giản như Boolean và số nguyên[^pm-differ-objects]. Hai: một thuật toán có thể mô tả cả một *lớp* phép tính, chương trình thì mô tả một cách tính[^pm-differ-class]. Ba: thuật toán gồm một dãy bước, và muốn định nghĩa thuật toán đồng thời phải nói rõ cái gì là *một bước nguyên tử*; manual nói chương trình không có khái niệm "một bước" xác định[^pm-steps]. PlusCal phản ánh ba điểm đó: biểu thức là TLA+, có cấu trúc diễn đạt nondeterminism[^pm-nondet], và dùng *label* để mô tả các bước (mục 2).

*Một file PlusCal gồm gì.* Thuật toán nằm trong một comment của module TLA+; translator ghi bản dịch vào giữa hai dòng comment đặc biệt `\* BEGIN TRANSLATION` và `\* END TRANSLATION`[^pm-in-comment]. Nếu hai dòng này đã có, translator xóa mọi thứ nằm giữa chúng và thay bằng bản dịch mới[^pm-begin-trans] — nên **không sửa tay trong vùng đó**. Dưới đây là khung của một file (minh họa của người soạn, cú pháp P-syntax):

```tla
---------------------------- MODULE Skeleton ----------------------------
EXTENDS Naturals, TLC

CONSTANT N

(* --algorithm Skeleton
variables x = 0, y \in 1..N;       \* biến toàn cục; y có N giá trị khởi đầu

define                              \* toán tử TLA+ dùng được biến toàn cục
  NonNegative == x >= 0
end define;

process worker \in 1..N             \* tập N process
variable local = 0;                 \* mỗi process một biến local riêng
begin
  Step:                             \* label: đầu một bước nguyên tử
    x := x + 1;
end process;
end algorithm; *)

\* BEGIN TRANSLATION
\* (translator ghi bản dịch TLA+ vào đây)
\* END TRANSLATION
=============================================================================
```

Các phần theo thứ tự trong ngữ pháp: tên thuật toán, khai báo biến, khối `define`, các `macro`, các `procedure`, rồi hoặc thân của thuật toán một process, hoặc một dãy `process`[^pm-grammar]. Module cần `EXTENDS TLC` nếu thuật toán dùng `print`[^pm-print-ext] hay `assert`[^pm-assert-ext], và `EXTENDS Sequences` nếu có `procedure`[^pm-proc-seq].

*Khai báo biến.* `variables x = 0` đặt giá trị khởi đầu; `y \in 1..N` cho phép *bất kỳ* giá trị nào trong tập làm giá trị khởi đầu (TLC kiểm tra tất cả các khả năng)[^pm-init-in]. Biến không khai báo giá trị khởi đầu nhận hằng chưa xác định `defaultInitValue`[^pm-defaultinit].

*Hai cú pháp.* PlusCal có hai cú pháp: P-syntax dài dòng và C-syntax gọn hơn[^pm-two-syn]. Cùng một đoạn code trong manual:

P-syntax:

```tla
while x > 0 do
  if y > 0 then y := y-1;
                x := x-1
  else x := x-2
  end if
end while;
print y;
```

C-syntax:

```tla
while (x > 0)
{ if (y > 0) { y := y-1;
               x := x-1 }
  else x := x-2 } ;
print y;
```

Bảng đối chiếu các cấu trúc chính, theo phụ lục A của hai manual:

| Cấu trúc | P-syntax | C-syntax |
|---|---|---|
| thuật toán | `--algorithm N` … `begin` … `end algorithm` | `--algorithm N {` … `}` |
| process | `process P \in S` … `begin` … `end process;` | `process (P \in S)` … `{` … `}` |
| procedure | `procedure F(a)` … `begin` … `end procedure;` | `procedure F(a)` … `{` … `}` |
| macro | `macro M(a) begin` … `end macro;` | `macro M(a) {` … `}` |
| define | `define` … `end define;` | `define {` … `}` |
| `if` | `if t then A else B end if;` | `if (t) A else B` |
| `while` | `L: while t do A end while;` | `L: while (t) A` |
| `either` | `either A or B end either;` | `either A or B` |
| `with` | `with x \in S do A end with;` | `with (x \in S) A` |
| nhóm lệnh | cả khối `then … end if`, `do … end while` | dấu ngoặc nhọn `{ … }` |

Trong C-syntax, một *compound statement* là dãy lệnh ngăn cách bằng dấu chấm phẩy và đặt trong dấu ngoặc nhọn[^pm-c-compound]; thân process cũng là một dãy lệnh trong ngoặc nhọn[^pm-c-procbody]. Danh sách các lệnh không nhãn (`UnlabeledStmt`) trong ngữ pháp C-syntax gồm gán, `if`, `while`, `either`, `with`, `await`/`when`, `print`, `assert`, `skip`, `return`, `goto`, `call` và gọi macro[^pm-c-grammar]; mục 3.2 của P-syntax manual liệt kê cùng các lệnh đó (đối chiếu mục lục hai manual). Cú pháp từng lệnh thì khác (xem bảng trên: `if (t) A else B` thay cho `if t then A else B end if`). Tiện lợi riêng của C-syntax: có thể bao một dãy lệnh vào `{ }` — nhờ đó bao được cả một bước nguyên tử — và label của lệnh đầu đặt trước hoặc sau dấu ngoặc mở đều được[^pm-c-braces].

Chương này viết chủ yếu bằng P-syntax; ví dụ thật từ repo `tlaplus/Examples` mà chương trích (mục 7) viết bằng C-syntax. Mục 8 đưa cả hai phiên bản của cùng một thuật toán.

*Kiểu dữ liệu.* PlusCal không có kiểu. "Đúng kiểu" là một *tính chất* của thuật toán: giá trị mỗi biến luôn thuộc đúng tập, và bạn kiểm tra bằng một invariant như `TypeOK`[^pm-typecorrect] (chương 04).

<details><summary>Đi sâu: cách chạy translator và TLC, <code>pc</code> trong invariant, hai chữ ký checksum, quy tắc đặt tên</summary>

- Manual giả định bạn dùng TLA+ Toolbox, nhưng translator và TLC là chương trình Java thường và có thể chạy từ dòng lệnh[^pm-java]. Khi chạy từ dòng lệnh, translator ghi thêm một file `.cfg` để chạy TLC[^pm-cfg]. (Nhận xét của người soạn: nếu bạn dùng VS Code hay `tla2tools.jar`, các bước vẫn là "dịch rồi kiểm tra"; tên lệnh cụ thể thuộc chương 04, ở đây không lặp.)
- Tùy chọn cho translator đặt trong file bằng dòng `PlusCal options (...)`, dấu `-` có thể bỏ[^pm-options-stmt]. Danh sách đầy đủ ở Phụ lục C của manual; chương này nhắc `-termination`, `-wf`, `-sf`, `-nof`, `-wfNext`, `-label`, `-noDoneDisjunct` ở những mục liên quan.
- Trong các file mẫu của repo `tlaplus/Examples`, dòng mở vùng dịch có thêm hai checksum, ví dụ `\* BEGIN TRANSLATION (chksum(pcal) = "1d547bc3" /\ chksum(tla) = "8de86c82")`[^ex-peterson-chksum]. Manual 1.8 không mô tả hai checksum này; chương không suy đoán mục đích.
- TLA+ có quy tắc chung: một định danh đã có nghĩa thì không được gán nghĩa mới[^pm-reuse]. Hệ quả cụ thể trong PlusCal: nếu bạn khai báo biến `j`, một định nghĩa viết sau `END TRANSLATION` không được dùng `j` làm biến buộc (`\A i, j \in ...`), nếu không parser báo "multiply-defined identifier"[^pm-redefine]. Các tên do translator khai báo hoặc định nghĩa gồm: mọi biến toàn cục và local, `pc`, `stack` (khi có procedure), tham số procedure, `ProcSet` (chỉ với thuật toán nhiều process), `vars`, `Init`, `Next`, `Spec`, và một action cho mỗi label[^pm-trans-ids].
- Từ khóa của PlusCal bị cấm làm tên: `assert await begin call define do either else elsif end goto if macro or print procedure process return skip then variable variables when while with`[^pm-c-reserved]. Ngoài ra `pc`, `stack`, `self` không được làm tên biến[^pm-c-varname], và `Done`, `Error` không được làm label[^pm-implicit].
- Translator không phân tích biểu thức mà để parser SANY làm việc đó[^pm-no-parse]; ví dụ với phép gán, nó chỉ coi mọi thứ đến ký hiệu dành riêng kế tiếp là biểu thức của phép gán[^pm-assume-expr]. Hệ quả: lỗi có thể được báo ở chỗ sau nguyên nhân thật[^pm-error-source].

</details>

**Quay lại Q1:** PlusCal là pseudo-code có ngữ nghĩa chính xác, nằm trong comment của module TLA+, được translator biên dịch thành TLA+ giữa hai dòng `BEGIN/END TRANSLATION`, rồi TLC kiểm tra bản dịch; hai cú pháp P và C chỉ khác nhau ở bề ngoài.

## 2. Label và bước nguyên tử (Q2)

> **Trả lời ngắn.** Một *bước* của PlusCal là phần thực thi từ một label đến label kế tiếp[^pm-step-label]. Cả bước chạy như một khối không thể chen ngang: process khác chỉ có thể chen vào *giữa hai bước*, tức là ở ranh giới label. Vì vậy label quyết định các interleaving mà model checker khám phá.

**Giải thích.**

Hình dung (nhận xét của người soạn): label là các điểm mà scheduler được phép đổi thread. Mọi dòng code nằm giữa hai label là "một lần chạy không bị ngắt"; muốn mô hình hóa việc `counter += 1` có thể bị ngắt giữa lúc đọc và lúc ghi (P4·03 mục 1), bạn phải đặt một label ở giữa.

*Định nghĩa chính xác.* Manual định nghĩa *control path* là một đường đi qua code theo cú pháp (bỏ qua việc câu lệnh có thực sự chạy được hay không), và *step* là control path bắt đầu ở một label, kết thúc ở một label và không đi qua label nào khác[^pm-step-cp]. Một lần thực thi thuật toán là một dãy các lần thực thi step; không bao giờ có chuyện chạy một phần của step (ngoại trừ `print`/`assert`, xem phần Đi sâu)[^pm-exec-steps]. Mỗi process kết thúc ở một label ngầm `Done`[^pm-pc-done].

*Nhiều process chạy thế nào.* Thuật toán nhiều process chạy bằng cách lặp lại: chọn một process bất kỳ, chạy một step của nó (nếu step đó chạy được)[^pm-multi-exec]. "Bất kỳ" nghĩa là model checker thử *mọi* thứ tự, đó là nguồn của việc tìm ra race. Một step không chạy được khi process đã kết thúc, hoặc khi step chứa `await` mà biểu thức sai (mục 5)[^pm-impossible].

```mermaid
stateDiagram-v2
  [*] --> Load
  Load --> Store: bước Load đọc counter vào tmp
  Store --> Done: bước Store ghi counter
  Done --> [*]
```

Sơ đồ là điều khiển của một thread trong ví dụ mục 8: mỗi mũi tên là một bước nguyên tử, tên trạng thái là giá trị của `pc` của thread đó (mục 7). Hai thread cho hai chuỗi như vậy, và scheduler (ở đây là TLC) chọn xen kẽ.

*Một vòng `while` là bao nhiêu bước?* Với `L: while test do body end while;`, mỗi lần thực thi cả thân vòng lặp từ `L` về lại `L` là **một** bước; khi `test` sai, bước thực thi phép thử và chuyển điều khiển ra khỏi vòng[^pm-while-exit]. Learn TLA+ nói cùng ý: vòng lặp không nguyên tử, mỗi vòng lặp là một action riêng[^lt-nonatomic]. Nếu muốn mỗi lần lặp tách thành nhiều bước, phải đặt label *bên trong* thân vòng lặp.

*Độ hạt nguyên tử là lựa chọn của người viết.* Manual nói rõ: thuật toán đồng thời cần xác định cái gì là một bước nguyên tử[^pm-step-def], và với thuật toán nhiều process bạn thường muốn tự chỉ định độ hạt này[^pm-grain]. Cùng một phép `counter := counter + 1` có thể mô hình hóa theo ba độ hạt:

```tla
\* (a) một label: tăng nguyên tử, như một lệnh fetch-and-add
Inc:    counter := counter + 1;

\* (b) hai label: đọc, rồi ghi lại giá trị đã cộng
Load:   tmp := counter;
Store:  counter := tmp + 1;

\* (c) ba label: đọc, cộng, ghi như một chuỗi lệnh máy rời nhau
Load:   reg := counter;
Add:    reg := reg + 1;
Store:  counter := reg;
```

(Minh họa của người soạn. Suy luận của người soạn: với bài toán này (b) và (c) cho cùng tập lỗi quan trọng, vì `Add` chỉ chạm biến local; đó là lý do thường gộp thao tác trên biến local vào label lân cận, xem mục 3.) Càng nhiều label, càng nhiều interleaving: Learn TLA+ lưu ý concurrency là một trong các nguồn chính của bùng nổ không gian trạng thái[^lt-explosion].

<details><summary>Đi sâu: step chứa <code>await</code>, <code>print</code>/<code>assert</code>, <code>goto</code>, và <code>if</code> trong một step</summary>

- `if` và phép thử của nó nằm trong *cùng* một step. Trong ví dụ FastMutex của manual, phép thử `y # 0` ở label `l2` là nguyên tử vì step bắt đầu ở `l2` kết thúc khi điều khiển tới `l3` hoặc `l4`[^pm-fastmutex-l2].
- `goto lab` kết thúc step hiện tại và chuyển điều khiển tới câu lệnh có label `lab`[^pm-goto]. Vì vậy sau `goto` luôn phải có label (mục 3).
- `await` có thể nằm *ở bất kỳ chỗ nào* trong step, không nhất thiết ở đầu[^pm-await-pos]. Manual cho ví dụ hai đoạn tương đương: `a: x := y + 1; await x > 0; b: ...` và `a: await y + 1 > 0; x := y + 1; b: ...`: step từ `a` đến `b` chỉ chạy được khi giá trị hiện tại của `y+1` dương[^pm-await-equiv]. Nghĩa là `await` được đánh giá với giá trị *đã cập nhật* trong cùng step; và nếu `await` sai thì *cả step* không chạy, mọi phép gán trước nó cũng không có hiệu lực. Learn TLA+ cảnh báo kiểu: `await` tương tác hơi lạ với việc cập nhật biến và khuyên không đặt biến vừa gán vào `await`[^lt-await-odd]. (Nhận xét của người soạn: đặt `await` ở *đầu* label cho khỏi nhầm.)
- `print` và `assert` là ngoại lệ của quy tắc "không chạy một phần step": TLC có thể in giá trị, hoặc báo `assert` sai, ngay cả khi step không chạy được vì một `await` đứng sau chúng trong step[^pm-print][^pm-assert]. (Nhận xét của người soạn: vì vậy nên đặt `assert` sau mọi `await` của step, hoặc trong step không có `await`.)
- Biểu thức trong `define` luôn dùng giá trị biến *ở đầu step*, kể cả khi nó xuất hiện sau một phép gán trong cùng step[^pm-define-start]. Đây là khác biệt tinh vi giữa dùng `define` và viết biểu thức trực tiếp.

</details>

**Quay lại Q2:** label đánh dấu ranh giới giữa các bước; mỗi bước chạy trọn vẹn không bị chen, và model checker thử mọi cách xen kẽ các bước của các process — nên vị trí label chính là độ hạt nguyên tử của mô hình.

## 3. Quy tắc đặt label (Q3)

> **Trả lời ngắn.** Label *bắt buộc* ở đầu mỗi process/procedure/thuật toán một process, ở mỗi `while`, sau `goto`/`return`, sau `call` (trừ khi câu kế là `return` hoặc `goto`), và sau mỗi `if`/`either` có chứa label hay `goto`/`call`/`return` bên trong[^pm-lab-first][^pm-lab-while][^pm-lab-after]. Label *bị cấm* trong thân `macro` và trong phần `do` của `with`[^pm-lab-macro]. Và giữa hai phép gán cùng một biến phải có label[^pm-lab-assign]. Với thuật toán một process mà bạn không gõ label nào, translator tự thêm số label tối thiểu để thỏa các quy tắc[^pm-fewest]; với thuật toán nhiều process thì tùy chọn `-label` không phải mặc định[^pm-label-default] và manual nhắc bạn thường sẽ muốn tự chỉ định độ hạt nguyên tử[^pm-grain].

**Giải thích.**

Hình dung (nhận xét của người soạn): label là cái "thẻ chấm công" của dòng code. Translator cần thẻ ở những chỗ nó phải biết "đã xong bước này, bước kế là đâu", và cấm thẻ ở chỗ bước không thể cắt đôi (trong `macro`, trong `with`).

Bảng quy tắc (từ mục 3.7 và 2.7 của manual), kèm ví dụ sai và cách sửa — các đoạn dưới là minh họa của người soạn:

| Quy tắc | Ví dụ vi phạm | Cách sửa |
|---|---|---|
| Câu đầu của process/procedure/thuật toán phải có label[^pm-lab-first] | `begin x := 1; ...` | `begin Init: x := 1; ...` |
| `while` phải có label; câu *sau* `while` không cần[^pm-lab-while] | `while i < 3 do ... end while;` | `Loop: while i < 3 do ... end while;` |
| Hai phép gán cùng một biến phải cách nhau một label[^pm-lab-assign] | `a: x := x + 1; x := x * 2;` | `a: x := x + 1; b: x := x * 2;` hoặc gộp `x := (x + 1) * 2;` |
| Câu đứng sau `goto` hoặc `return` phải có label; câu đứng sau `call` cũng vậy, trừ khi nó là `return` hoặc `goto`[^pm-lab-after] | `goto Top; x := 0;` | `goto Top; Next: x := 0;` |
| `if`/`either` có label/`goto`/`call`/`return` bên trong thì câu sau nó phải có label[^pm-lab-if] | `if c then L: ... end if; y := 1;` | `if c then L: ... end if; M: y := 1;` |
| Thân `macro` và phần `do` của `with` không được có label[^pm-lab-macro][^pm-with-nolabel] | `with m \in S do L: ... end with;` | bỏ label trong `with`, đưa phần cần label ra ngoài `with` |
| `Done` và `Error` là label ngầm, không dùng làm label thật[^pm-implicit] | `Done: skip;` | đổi tên |

Quy tắc "hai phép gán cùng biến" áp dụng cho *biến*, không phải thành phần của biến: nhiều phép gán vào các thành phần của cùng một biến có thể đi chung trong một phép gán nhiều vế nối bằng `||`[^pm-multiassign-ok]. Phép gán nhiều vế tính vế phải của mọi phép gán trong nó trước, rồi mới thực hiện các phép gán từ trái sang phải[^pm-multiassign], nên `u := v || v := u` hoán đổi hai giá trị (manual dùng đúng cách này để hoán đổi trong thuật toán Euclid)[^pm-swap]. Ngoài ra `call` và `return` cũng được tính là phép gán vào tham số và biến local của procedure, nên cũng nằm trong quy tắc trên (mục 6)[^pm-callret-assign].

Ví dụ dương trong repo tlaplus, một `either` có label bên trong, nên phải có label `d` ngay sau nó:

```tla
--algorithm Either
  variables x = 0 ; y = 0 ;
  begin a: either x := 1 ; b: x := x + 1;
               or y := 1 ; c: y := y + 1;
           end either ;
        d: assert x+y = 2 ;
end algorithm
```

(Nguồn: file test của translator, `test-model/pcal/Either1.tla` trong `tlaplus/tlaplus` ghim SHA `94d0c50…`[^tt-either-src].)

*Khi thiếu label.* Translator báo "Missing label at the following location". Manual nhắc nguyên nhân hay bị quên: `call` và `return` cũng là phép gán nên đòi hỏi label[^pm-missing-label]. Muốn translator tự thêm label, dùng tùy chọn `-label`; tùy chọn này là mặc định cho thuật toán một process mà bạn không gõ label nào[^pm-label-default]. `-reportLabels` giống `-label` nhưng cho biết nó đã thêm label ở đâu[^pm-reportlabels] (Phụ lục C xếp nó vào nhóm tùy chọn chỉ dùng ở dòng lệnh).

*Chọn chỗ đặt label.* Quy tắc cơ học ở trên chỉ nói "phải có chỗ nào"; còn "đặt thêm label ở đâu" là quyết định mô hình hóa. Hai thái cực: chỉ giữ số label tối thiểu mà quy tắc đòi hỏi cho ra ít step nhất và model checking nhanh nhất[^pm-fast], nhưng có thể che mất lỗi vì process khác không có cơ hội chen vào; đặt label trước *mỗi* phép đọc/ghi biến chung cho ra nhiều interleaving nhất, bắt được mọi race ở mức đó nhưng không gian trạng thái lớn. (Nhận xét của người soạn: cách làm ổn là đặt ranh giới label đúng ở ranh giới nguyên tử của hệ thật — một lệnh atomic, một lần acquire lock, một RPC, một lần commit transaction là một label; thao tác chỉ chạm biến local thì gộp vào label lân cận.)

<details><summary>Đi sâu: tên label, tùy chọn label, fairness trên label</summary>

- Label trở thành *tên action* trong bản dịch (mục 7). Vì vậy hai process không dùng chung tên label (Learn TLA+ nêu điều này)[^lt-labels-share]. Tên label có thể trùng tên biến (hoặc hai biến local ở hai procedure trùng tên); khi đó translator đổi tên một trong hai, phát cảnh báo và ghi các phép đổi tên trong comment ngay sau dòng `BEGIN TRANSLATION`[^pm-rename].
- `-labelRoot name` đổi tiền tố của các label do translator thêm (mặc định `Lbl`, ra `Lbl1`, `Lbl2`, …)[^pm-labelroot].
- Hậu tố `+`/`-` sau label (như `l4:+`, `ncs:-`) chỉnh fairness của action tương ứng (mục 4).
- Với thuật toán một process không có label nào, mọi label đều do translator thêm và số step ít nhất có thể[^pm-fewest]. Với nhiều process, manual nói bạn thường sẽ muốn chỉ định độ hạt nguyên tử một cách tường minh[^pm-grain] — tức là tự đặt label.

</details>

**Quay lại Q3:** bắt buộc: đầu process, mỗi `while`, sau `goto`/`call`/`return`, sau `if`/`either` có label bên trong, và giữa hai phép gán cùng biến; cấm: trong `macro` và trong `do` của `with`; còn chỗ đặt thêm là quyết định về độ hạt nguyên tử, nên đặt đúng ranh giới nguyên tử của hệ thật.

## 4. `process`: process đơn, tập process, `self`, `fair` (Q4)

> **Trả lời ngắn.** `process P = e` khai báo một process có định danh `e`; `process P \in S` khai báo một *tập* process, mỗi phần tử của `S` là một process chạy cùng đoạn code, trong đó `self` là định danh của process đang chạy[^pm-proc-set][^pm-proc-self]. Biến `variable` khai báo ngay dưới `process` là biến local: mỗi process một bản[^pm-proc-local]. Mặc định process không có giả định fairness nào — có thể "dừng mãi" ở bất kỳ bước nào; `fair process` thêm giả định weak fairness cho mọi action của nó[^pm-fair-proc][^pm-unfair].

**Giải thích.**

Hình dung (nhận xét của người soạn): `process worker \in 1..N` là thread pool N worker cùng chạy một hàm; `self` là worker id; biến local là biến trên stack của mỗi thread, còn biến toàn cục là bộ nhớ dùng chung.

*Cú pháp và quy tắc.* Hai dạng khai báo là `process ProcName \in IdSet` (tập process) và `process ProcName = Id` (một process)[^pm-proc-set]. Các định danh của những process khác nhau phải khác nhau[^pm-proc-ids], và nên cùng "kiểu" (đều là số, hoặc đều là chuỗi…), vì ngữ nghĩa TLA+ không nói một chuỗi có bằng một số hay không[^pm-proc-ids-type]; Learn TLA+ diễn đạt: mọi process phải có kiểu so sánh được với nhau[^lt-types]. Tên `ProcName` không có ý nghĩa gì: đổi tên không đổi nghĩa của câu `process`; tên này xuất hiện trong bản dịch TLA+ và nên khác nhau giữa các câu `process`[^pm-proc-name] (ví dụ `Simple.tla` ở mục 7 cho thấy nó thành tên action `proc(self)`). Thân process bắt đầu bằng `begin`, kết thúc bằng `end process` (P-syntax) và câu đầu phải có label[^pm-lab-first]. Trong thân của một tập process, `self` bằng định danh của process hiện hành[^pm-proc-self] — và trong thân procedure được gọi từ process đó, `self` cũng là định danh của process đang thực thi procedure[^pm-proc-self-proc].

*Ví dụ: producer/consumer.* Minh họa của người soạn, P-syntax:

```tla
---------------------------- MODULE ProdCons ----------------------------
EXTENDS Naturals, Sequences

CONSTANTS NumProducers, MaxQueue
Producers == 1..NumProducers

(* --algorithm ProdCons
variables queue = <<>>;

fair process producer \in Producers
begin
  Produce:
    await Len(queue) < MaxQueue;
    queue := Append(queue, self);
    goto Produce;
end process;

fair process consumer = 0
variable item = 0;
begin
  Consume:
    await queue /= <<>>;
    item := Head(queue);
    queue := Tail(queue);
    goto Consume;
end process;
end algorithm; *)
```

`producer` là tập process với định danh `1..NumProducers`, còn `consumer` là một process có định danh `0` — cùng kiểu số, và khác mọi định danh của producer. Mỗi label (`Produce`, `Consume`) là một bước nguyên tử gồm cả phần `await` và phần cập nhật `queue`, tức là việc kiểm tra hàng đợi và thao tác trên nó là một thể (một giả định mô hình hóa: ví dụ hàng đợi bảo vệ bằng mutex, hoặc kênh có sẵn thao tác nguyên tử). `goto Produce` ở cuối là cách lặp mãi, tương đương bọc thân trong `while TRUE`[^lt-goto-loop].

*Biến local.* Trong bản dịch, biến local của process trở thành *hàm* có miền là tập định danh process (`item` thành `item[self]`)[^pm-localvar]. Learn TLA+ lưu ý hệ quả: biến local không tham chiếu được từ process khác hoặc từ khối `define`[^lt-define-local], vì `define` chỉ được dùng biến toàn cục, `pc` và `stack`[^pm-define-vars]. Cũng vì thế, các invariant dùng biến local phải viết *sau* `END TRANSLATION` (mục 7).

*Fairness.* Mỗi label ứng với một action (mục 7), và "action *enabled* khi nó có thể chạy"[^pm-enabled]. Action như `a: y := 42;` chỉ enabled khi điều khiển đang ở `a` gọi là *non-blocking*; action có `await` như `b: await x > self;` gọi là *blocking*[^pm-nonblock]. Với action non-blocking, fairness nghĩa là process không được dừng mãi tại action đó[^pm-fair-nonblock]. Với action blocking có hai mức: *weak fairness* (process không được dừng nếu action enabled *liên tục mãi mãi*) và *strong fairness* (cũng không được dừng nếu action cứ bị tắt rồi bật lại vô hạn lần)[^pm-wf-sf].

| Khai báo | Fairness mặc định của các action |
|---|---|
| `process` | không có giả định nào[^pm-unfair] |
| `fair process` | weak fairness[^pm-fair-proc] |
| `fair+ process` | strong fairness[^pm-fairplus] |
| nhãn `lab:+` trong process `fair` | action đó strong fair[^pm-fair-label] |
| nhãn `lab:-` | action đó không có giả định fairness[^pm-fair-label] |

Tùy chọn của translator: `-wf` biến process "unfair" thành `fair`, `-sf` thành `fair+`, `-nof` làm mọi process unfair[^pm-fair-opts]. Fairness của *toàn thuật toán* ("không dừng khi còn action enabled") bật bằng `--fair algorithm` hoặc `-wfNext`[^pm-fair-algorithm]. Vì sao phải quan tâm: tác giả một spec Dining Philosophers trong repo `tlaplus/Examples` ghi chú rằng nếu bỏ `fair` khỏi `fair process`, mỗi process có thể dừng bất cứ lúc nào và không bao giờ chạy lại[^ex-dp-fair-comment]. Suy luận của người soạn: khi đó các tính chất liveness kiểu "triết gia nào cũng cuối cùng lại được ăn" (spec đó viết là `NobodyStarves == \A p \in 1..NP: []<>(~hungry[p])`[^ex-dp-nobody-starves]) có thể sai trên một hành vi "đứng im". Chương 05 giải thích lý thuyết.

<details><summary>Đi sâu: weak/strong fairness qua ví dụ của manual, <code>-termination</code>, quy tắc giữa các dạng</summary>

- Ví dụ blocking của manual: `b: await x > self; x := x-1;`. Action `b` enabled khi điều khiển ở `b` *và* `x > self`. Weak fairness của `b` nghĩa là nếu `x > self` đúng mãi khi process ở `b`, thì `b` cuối cùng chạy; strong fairness của `b` nghĩa là process không dừng mãi ở `b` chừng nào `x > self` còn đúng mãi *hoặc* cứ liên tục đúng trở lại, dù có lúc sai[^pm-wf-sf].
- `+`/`-` sau label trong `fair+ process` hoặc process thường: thêm `+` sau label trong `fair+ process` không có tác dụng; thêm `+` hoặc `-` sau label trong process không fair cũng không có tác dụng[^pm-fair-plus-effect][^pm-unfair].
- Ví dụ trong manual: với FastMutex, để kiểm tra tính chất "nếu có process đang cố vào critical section thì cuối cùng có process (không nhất thiết là chính nó) ở trong", người ta đặt `fair` trước process và thêm `-` sau hai label `ncs:` và `cs:` để process được phép ở yên mãi trong noncritical/critical section[^pm-fastmutex-fair].
- `-termination`: nếu không có tùy chọn fairness nào thì nó tương đương `-wf`[^pm-termination-opt]. (Nhận xét của người soạn: hợp lý vì muốn kiểm tra "luôn kết thúc" thì phải giả định các process không dừng giữa chừng.)
- Manual cảnh báo: chậm hơn kiểm tra invariant, kiểm tra liveness (kể cả termination) không chạy được trên model lớn bằng kiểm tra invariant[^pm-liveness-slow].
- Dạng dịch của fairness: với tập process `P`, `Spec` có thêm conjunct `\A self \in S : WF_vars(P(self))`. Ví dụ thật (spec Dining Philosophers có `fair process Philosopher \in 1..NP`): `Spec == /\ Init /\ [][Next]_vars /\ \A self \in 1..NP : WF_vars(Philosopher(self))`[^ex-dp-spec].

</details>

**Quay lại Q4:** `process P = e` là một process, `process P \in S` là một tập process dùng `self` làm định danh, biến local là hàm theo `self` sau khi dịch, và `fair process` thêm giả định weak fairness (không có nó, process được phép dừng vĩnh viễn).

## 5. `await`, `either`, `with`: chờ và chọn không tất định (Q5)

> **Trả lời ngắn.** `await expr` (đồng nghĩa `when expr`) làm bước chỉ chạy được khi `expr` đúng[^pm-await][^pm-when]; nếu sai, bước không chạy và không có gì thay đổi, process thử lại sau[^pm-await-fail]. `either … or …` chọn không tất định *một nhánh chạy được* để thực thi[^pm-either]. `with x \in S do …` chọn không tất định một phần tử của `S` rồi chạy thân; nếu `S` rỗng thì không chạy được[^pm-with]. TLC thử mọi lựa chọn.

**Giải thích.**

Hình dung (nhận xét của người soạn): `await` là "chờ mutex / chờ queue có việc / chờ điều kiện của condvar"; `either` là "môi trường có thể làm một trong các việc này" (mạng có thể mất gói hoặc giao được, node có thể chết hoặc sống); `with` là "chọn bất kỳ phần tử nào" (bất kỳ message trong inbox, bất kỳ replica).

*`await`/`when`.* Một bước chứa `await expr` chỉ thực hiện được khi `expr` bằng TRUE[^pm-await]. Hãy nghĩ "bộ xử lý thử chạy cả bước": thử thành công thì bước chạy thật và điều khiển sang bước kế; thử thất bại (biểu thức sai) thì không có gì đổi và sẽ thử lại sau[^pm-await-fail]. Đó là cách PlusCal biểu diễn việc *chờ*: không có "spin" hay "ngủ", chỉ có một bước chưa được phép.

*`either`.* Cú pháp `either A or B or … end either;`[^pm-either]. Bước chạy được khi ít nhất một nhánh chạy được[^pm-either-enabled]. Hệ quả: `if t then A else B end if` tương đương `either await t; A or await ~t; B end either` — manual nêu đúng sự tương đương đó[^pm-if-either]. Điểm khác là `either` cho phép các điều kiện *không loại trừ nhau* (hoặc không có điều kiện), tức nondeterminism thật.

*`with`.* `with id \in S do body end with;` chạy `body` với `id` bằng một phần tử được chọn không tất định của `S`; nếu `S` rỗng thì không chạy được[^pm-with]. `with id = expr` chỉ gán tên cho giá trị của `expr`, tương đương `with id \in {expr}`[^pm-with-eq]. Thân `with` không chứa label[^pm-with-nolabel]; hệ quả là trong thân không có `while`, không có hai phép gán cùng biến (trừ phép gán nhiều vế vào các thành phần khác nhau)[^pm-with-consequence].

*Ví dụ: kênh có thể mất gói.* Minh họa của người soạn (P-syntax):

```tla
---------------------------- MODULE Channel ----------------------------
EXTENDS Naturals

CONSTANT MaxMsg

(* --algorithm Channel
variables inflight = {}, delivered = {};

process sender = 1
variable seq = 0;
begin
  Send:
    while seq < MaxMsg do
      seq := seq + 1;
      either
        inflight := inflight \union {seq};   \* gói được gửi đi
      or
        skip;                                 \* gói bị mất
      end either;
    end while;
end process;

process receiver = 2
begin
  Receive:
    while TRUE do
      with m \in inflight do
        inflight := inflight \ {m};
        delivered := delivered \union {m};
      end with;
    end while;
end process;
end algorithm; *)
```

Đọc ví dụ theo ngữ nghĩa trên: mỗi vòng của `Send` là một bước, trong đó `either` chọn giữa gửi và bỏ (cả hai chạy được, nên TLC thử cả hai); mỗi vòng của `Receive` là một bước, trong đó `with` chọn một gói bất kỳ đang bay; khi `inflight` rỗng, `with` không chạy được nên bước `Receive` bị chặn. Nhận xét của người soạn: model này cố ý đơn giản — nó không mô hình hóa thứ tự (tập chứ không phải hàng đợi) hay nhân bản.

*Deadlock.* Manual định nghĩa: thuật toán nhiều process bị deadlock nếu không process nào đi tiếp được mà còn process chưa kết thúc; thường xảy ra khi mọi process đang chờ ở `await` sai hoặc ở `with x \in S` mà `S` rỗng[^pm-deadlock]. Ví dụ `Channel` ở trên có đúng tình huống đó: khi `sender` đã `Done` và `inflight` rỗng, `receiver` kẹt ở `Receive` mãi. Theo manual, deadlock thường là lỗi và TLC báo; nếu thuật toán *được phép* dừng khi chưa mọi process kết thúc, tắt kiểm tra deadlock trong model[^pm-deadlock-normal]. (Suy luận của người soạn từ định nghĩa trên: với `Channel`, TLC sẽ báo deadlock trừ khi tắt kiểm tra đó, vì `receiver` là vòng lặp vô hạn nên không bao giờ tới `Done`.) Learn TLA+ có ví dụ tương tự: sau khi mọi writer chạy xong, reader kẹt mãi ở `await`[^lt-deadlock].

Còn trường hợp tất cả process đều tới `Done`: TLC không coi là deadlock vì translator thêm một action "đứng im" (mục 7).

<details><summary>Đi sâu: nhiều biến trong <code>with</code>, <code>either</code> chứa label, khởi tạo không tất định</summary>

- `with` có thể liệt kê nhiều biến: `with id1 \in S1; id2 = e2 do …`; mỗi biến có thể gán bằng `=` hoặc `\in`, và dạng nhiều biến tương đương lồng nhiều `with`[^pm-with-multi].
- `either` có thể chứa label bên trong nhánh; khi đó câu sau `either` phải có label (mục 3). Bản dịch thật của ví dụ `Either1` ở mục 3 (trong repo tlaplus): action `a` thành một hội gồm `pc = "a"` và một *tuyển* hai nhánh, mỗi nhánh gán `pc'` một giá trị khác nhau và giữ nguyên biến kia:

```tla
a == /\ pc = "a"
     /\ \/ /\ x' = 1
           /\ pc' = "b"
           /\ y' = y
        \/ /\ y' = 1
           /\ pc' = "c"
           /\ x' = x
```

Đây là cách `either` được dịch: một tuyển các nhánh[^tt-either-trans].

- Khởi tạo không tất định: `variables v \in 1..N` cho ra nhiều trạng thái đầu. Manual dùng ví dụ `EuclidAlg` với `v \in 1..N`: đặt `N = 4` thì có 4 thực thi[^pm-bfs-n4], và TLC không chạy riêng từng thực thi mà tìm mọi trạng thái đạt tới bằng tìm kiếm theo chiều rộng[^pm-bfs-search].
- `print` in giá trị của một biểu thức; trong chế độ model-checking, vì TLC duyệt theo chiều rộng nên output của `print` lẫn lộn, và manual khuyên chỉ để một `print` ở cuối thuật toán[^pm-bfs]. Muốn kiểm tra một điều kiện thì dùng `assert` thay vì tự đọc output[^pm-assert-check].

</details>

**Quay lại Q5:** `await` chặn cả bước tới khi điều kiện đúng, `either` và `with` là hai cách chọn không tất định mà TLC thử hết, và khi mọi process đều bị chặn mà chưa kết thúc thì đó là deadlock mà TLC báo.

## 6. `macro` và `procedure`: tái sử dụng code (Q6)

> **Trả lời ngắn.** `macro` giống procedure nhưng được *thay thế văn bản* tại thời điểm dịch: nó chạy bên trong bước của chỗ gọi, nên không được chứa label, `while`, `call`, `return`, `goto`[^pm-macro][^pm-macro-body]. `procedure` là hàm thật: có label (nhiều bước), tham số, biến local, được gọi bằng `call` và trả về bằng `return`; translator thêm biến `stack` để mô hình hóa call stack[^pm-stack-add][^pm-stack-model]. Procedure không trả giá trị — kết quả đi qua biến chung[^pm-proc-noreturn].

**Giải thích.**

Hình dung (nhận xét của người soạn): `macro` giống `#define` hoặc hàm `#[inline(always)]` mà nội dung được dán thẳng vào chỗ gọi, nên nó nằm trọn trong một bước; `procedure` giống hàm bình thường: chỗ gọi và thân hàm có thể bị chen giữa bởi thread khác, và nơi trả về được ghi trên stack.

*Macro.* Ví dụ của manual (P-syntax):

```tla
macro P(s, i) begin await s >= i;
                    s := s - i;
end macro;
```

Lời gọi `P(sem, y + 17);` được thay bằng thân macro với tham số thay bằng đối số, tức là `await sem >= (y + 17); sem := sem - (y + 17);`[^pm-macro-expand]. Macro được định nghĩa ngay sau khai báo biến toàn cục và khối `define`, và có thể gọi một macro đã định nghĩa trước đó[^pm-macro-pos]. Macro dùng `self` được: Learn TLA+ cho ví dụ macro `write()` gọi `Append(queue, self)` bên trong[^lt-macro-self] — hợp lý vì phép thay thế là *văn bản* tại chỗ gọi, nên ý nghĩa của mọi ký hiệu không phải tham số là ý nghĩa ở ngữ cảnh chỗ gọi[^pm-macro-syntactic]. Ví dụ thật: spec `Lock` trong `tlaplus/Examples` định nghĩa

```tla
macro Lock(l){ await l = 1; l := 0; }
macro Unlock(l){ l := 1; }
```

và label `l1: Lock(lock);` được dịch thành một action có đúng điều kiện `lock = 1` và hiệu ứng `lock' = 0`: `l1(self) == /\ pc[self] = "l1" /\ lock = 1 /\ lock' = 0 /\ pc' = [pc EXCEPT ![self] = "cs"]`[^ex-lock-macro][^ex-lock-l1]. Đó chính là minh họa "macro chạy bên trong bước của chỗ gọi"[^pm-macro].

*Procedure.* Cú pháp: `procedure Name(p1, p2) variables v = 0; begin L: ...; return; end procedure;`. Procedure nằm sau khối `define`/macro và trước `begin` của thuật toán hoặc process đầu tiên[^pm-proc-pos]; Learn TLA+ nhắc: procedure phải định nghĩa sau macro và trước process[^lt-proc-order]. Thân phải bắt đầu bằng câu có label[^pm-lab-first]. Các tham số được xử lý như biến và gán được; biến local của procedure khởi tạo lại mỗi lần vào procedure[^pm-proc-params]. Ví dụ đệ quy giai thừa lấy từ file test của translator (đã bỏ các comment):

```tla
--algorithm Factorial
  variable result = 1;
  procedure FactProc(arg1 = 0)
    variable u = 1 ;
    begin p1 : if arg1 = 0
                 then return;
                 else result := result * arg1;
                      call FactProc ( arg1 - 1 ) ;
                      return;
               end if;
    end procedure
  begin
    a1 : call FactProc( 5 ) ;
    a2 : assert result = 120 ;
  end algorithm
```

Nguồn: `test-model/pcal/Factorial.tla` ghim SHA `94d0c50…`[^tt-fact-src1][^tt-fact-src2]. Lưu ý nhánh `else` của `p1` kết thúc bằng `call FactProc(arg1 - 1); return;`. Manual nói khi một `call` đứng ngay trước `return` thì hai lần trả về được thực hiện trong cùng một bước, tức là tối ưu hóa gọi đuôi (thay đệ quy đuôi bằng vòng lặp)[^pm-tailcall]; đúng như vậy, trong bản dịch bên dưới nhánh `ELSE` của `p1` không đẩy bản ghi nào lên `stack` mà đặt `pc' = "p1"` và `stack' = stack`[^tt-fact-else].

*Dịch procedure.* Biến `stack` chứa call stack; với thuật toán nhiều process, `stack` là mảng các stack theo định danh process[^pm-stack-model]. Với `FactProc`, bản dịch trong file test (đã bỏ phần tiêu đề comment) có hai chỗ then chốt (đọc ký hiệu: `\o` nối hai sequence, `Head`/`Tail` là phần tử đầu / phần còn lại, `[ procedure |-> ... ]` là record; xem [chương 02](02-toan-nen-tang.md), mục 4):

```tla
\* a1: call FactProc(5) → đẩy một bản ghi lên stack, đặt tham số, nhảy vào p1
a1 == /\ pc = "a1"
      /\ /\ arg1' = 5
         /\ stack' = << [ procedure |->  "FactProc",
                          pc        |->  "a2",
                          u         |->  u,
                          arg1      |->  arg1 ] >>
                      \o stack
      /\ u' = 1
      /\ pc' = "p1"
      /\ UNCHANGED result

\* p1, nhánh return: lấy lại pc và các biến từ đỉnh stack rồi bỏ đỉnh
p1 == /\ pc = "p1"
      /\ IF arg1 = 0
            THEN /\ pc' = Head(stack).pc
                 /\ u' = Head(stack).u
                 /\ arg1' = Head(stack).arg1
                 /\ stack' = Tail(stack)
                 /\ UNCHANGED result
            ELSE /\ result' = result * arg1
                 /\ arg1' = arg1 - 1
                 /\ u' = 1
                 /\ pc' = "p1"
                 /\ stack' = stack
```

Các dòng trên rút gọn từ bản dịch của `Factorial.tla`[^tt-fact-call][^tt-fact-ret]. Ý nghĩa khớp với manual: một `call` gán đối số cho tham số, khởi tạo biến local và đặt điều khiển ở đầu thân procedure; `return` gán lại cho tham số và biến local *giá trị trước lần gọi* rồi trả điều khiển về câu ngay sau `call`[^pm-proc-return].

| | `macro` | `procedure` |
|---|---|---|
| mở rộng | lúc dịch, thay văn bản | gọi thật lúc chạy (`call`) |
| số bước | nằm trong bước của chỗ gọi | có thể gồm nhiều bước (có label) |
| label, `while`, `goto`, `call`, `return` | cấm[^pm-macro-body] | được (thân phải bắt đầu bằng label) |
| trả giá trị | không áp dụng | không có giá trị trả về trực tiếp; dùng biến chung[^pm-proc-noreturn] |
| thêm biến vào bản dịch | không (suy luận của người soạn: macro được mở rộng lúc dịch) | `stack`, tham số, biến local[^pm-stack-add] |
| module cần `EXTENDS Sequences` | không | có[^pm-proc-seq] |

<details><summary>Đi sâu: đọc <code>stack</code>, label ngầm <code>Error</code>, giá trị trả về qua biến chung, bẫy của macro</summary>

- Manual dùng thuật toán `Procedures` (hình 4) để giải thích cách đọc `stack`: `stack` là một *chuỗi các bản ghi*, mỗi bản ghi tương ứng một lần gọi procedure đang thực thi; bản ghi của lần gọi trong cùng ở đầu chuỗi[^pm-stack-rec][^pm-stack-innermost]. Mỗi bản ghi có trường `procedure` (tên procedure), `pc` (nhãn sẽ quay lại) và các tham số/biến local lúc *gọi*, là những giá trị sẽ được khôi phục khi `return` từ procedure đó[^pm-stack-restore].
- Label ngầm `Error` nằm ngay sau thân procedure: nếu điều khiển rơi tới đó (procedure chạy hết mà chưa `return`), process hay cả thuật toán dừng lại[^pm-proc-error]. Manual nói cách dễ gây deadlock nhất của thuật toán một process chính là "rơi khỏi cuối" procedure không `return`[^pm-deadlock-error].
- Trả giá trị: để một procedure `P` trả giá trị, cho nó gán một biến toàn cục (ví dụ `rVal[self] := ...;`) trước khi `return`, và chỗ gọi đọc biến đó ở câu có label ngay sau `call`[^pm-proc-rval]. Một `call` phải được theo sau bởi một label, trừ khi nó đứng ngay trước một `return`[^pm-call-label].
- Cũng vì `call` và `return` là phép gán vào tham số và biến local, trong cùng một step, một `call` đệ quy không thể đồng thời gán biến local của procedure đó[^pm-callret-assign].
- Bẫy macro: phép thay thế là *văn bản*; nếu tham số macro dùng làm biến buộc trong một biểu thức như `[id \in 1..N |-> FALSE]`, việc thay `id` bằng một biểu thức như `y + 17` gây lỗi khó hiểu lúc parse[^pm-macro-boundvar]. Tuân theo quy tắc TLA+: đừng dùng lại định danh đã có nghĩa[^pm-reuse].
- `define` khác `macro`: `define` định nghĩa *toán tử TLA+* dùng được biến toàn cục (không gán được), còn `macro` chứa câu lệnh. Định nghĩa trong `define` không được mở rộng trong bản dịch mà xuất hiện nguyên xi[^pm-define-notexpanded]; ví dụ thật: bản dịch spec Dining Philosophers có đoạn `(* define statement *)` giữa hai khai báo `VARIABLES`[^ex-dp-define].

</details>

**Quay lại Q6:** macro là phép thay văn bản chạy trong một bước của chỗ gọi; procedure là hàm nhiều bước có `call`/`return`, và translator mô hình hóa call stack bằng biến `stack` (chuỗi bản ghi chứa nơi quay lại và giá trị biến cần khôi phục).

## 7. Bản dịch sang TLA+ (Q7)

> **Trả lời ngắn.** Translator thêm biến `pc` (điều khiển) — và `stack` nếu có procedure[^pm-stack-add] — rồi định nghĩa `vars`, `Init`, một action cho mỗi label, `Next` và `Spec`[^pm-trans-ids]; với thuật toán nhiều process thì `Next` là tuyển các action tổng của từng process, lượng tử hóa `\E self \in ...` (ví dụ `Simple.tla` bên dưới)[^pm-next]. Action của label `L` có dạng "`pc[self] = "L"` và hiệu ứng của các câu lệnh trong bước, và `pc'` trỏ tới label kế, và `UNCHANGED` phần còn lại". Biến local thành hàm theo `self`[^pm-localvar].

**Giải thích.**

Hình dung (nhận xét của người soạn): bản dịch là *máy trạng thái viết tường minh*: `pc` là program counter của từng process; mỗi label thành một "lệnh máy" kiểu `if pc == L { làm gì đó; pc = kế }`. TLC chỉ làm việc với máy trạng thái đó; PlusCal chỉ là cách viết thân thiện.

```mermaid
flowchart LR
  A["Thuật toán PlusCal<br/>trong comment"] -->|translator| B["TLA+ giữa BEGIN và END TRANSLATION<br/>pc, vars, Init, một action mỗi label, Next, Spec"]
  B --> C["TLC duyệt các trạng thái"]
  D["Invariant, property viết tay<br/>(sau END TRANSLATION)"] --> C
```

*Ví dụ thật, C-syntax.* File `Simple.tla` trong `tlaplus/Examples` (ghim SHA `6a7b26a…`) có một thuật toán nhỏ, N process xếp vòng tròn:

```tla
--algorithm Simple {
    variables x = [i \in 0..(N-1) |-> 0], y = [i \in 0..(N-1) |-> 0] ;
    process (proc \in 0..N-1) {
      a: x[self] := 1 ;
      b: y[self] := x[(self-1) % N]
    }
}
```

[^ex-simple-src] và bản dịch (đã bỏ comment tiêu đề của file):

```tla
VARIABLES x, y, pc

vars == << x, y, pc >>

ProcSet == (0..N-1)

Init == (* Global variables *)
        /\ x = [i \in 0..(N-1) |-> 0]
        /\ y = [i \in 0..(N-1) |-> 0]
        /\ pc = [self \in ProcSet |-> "a"]

a(self) == /\ pc[self] = "a"
           /\ x' = [x EXCEPT ![self] = 1]
           /\ pc' = [pc EXCEPT ![self] = "b"]
           /\ y' = y

b(self) == /\ pc[self] = "b"
           /\ y' = [y EXCEPT ![self] = x[(self-1) % N]]
           /\ pc' = [pc EXCEPT ![self] = "Done"]
           /\ x' = x

proc(self) == a(self) \/ b(self)

(* Allow infinite stuttering to prevent deadlock on termination. *)
Terminating == /\ \A self \in ProcSet: pc[self] = "Done"
               /\ UNCHANGED vars

Next == (\E self \in 0..N-1: proc(self))
           \/ Terminating

Spec == Init /\ [][Next]_vars

Termination == <>(\A self \in ProcSet: pc[self] = "Done")
```

Các đoạn Init, `a(self)`, và `Next`/`Spec` đối chiếu với file gốc[^ex-simple-init][^ex-simple-a][^ex-simple-next]. Đọc từng mảnh, theo mô tả của manual:

1. **`VARIABLES`, `vars`, `ProcSet`.** `pc` được thêm để mô tả trạng thái điều khiển; `vars` là tuple mọi biến[^pm-trans-pc]; với thuật toán nhiều process, `ProcSet` là tập mọi định danh process[^pm-trans-ids].
2. **`Init`.** Có một conjunct cho mỗi biến; conjunct của biến toàn cục đứng trước biến local của process/procedure, và các biến khai báo trong cùng một câu `variables` xuất hiện theo thứ tự khai báo[^pm-trans-init]. Biến local của process và `pc` là hàm có miền là tập định danh[^pm-init-fn]; ở đây `pc` ban đầu là `"a"` cho mọi process — label đầu của process. Với thuật toán nhiều process, `pc` là hàm từ định danh sang chuỗi label[^pm-pc-fn]; với thuật toán một process, `pc` là một chuỗi đơn[^pm-pc-uni].
3. **Một action mỗi label.** Translator định nghĩa một action cho mỗi thao tác nguyên tử của thuật toán, được đặt tên theo label bắt đầu thao tác[^pm-action-per-label]; action là `Lab(self)` khi câu nằm trong process set hoặc procedure, ngược lại là `Lab`[^pm-action-lab]. Mỗi action là hội của: điều kiện bật (*enabling condition*, ở đây `pc[self] = "a"`, và với `await` thì thêm biểu thức của `await`)[^pm-enable][^pm-await-enable]; hiệu ứng lên biến (`x' = [x EXCEPT ![self] = 1]`: gán phần tử `self` của hàm `x`, đó là cách dịch phép gán thành phần)[^pm-except]; `pc'` trỏ tới label kế; và `UNCHANGED` hoặc `y' = y` cho các biến không đổi[^pm-unchanged].
4. **Action tổng của process.** `proc(self) == a(self) \/ b(self)` là tuyển các action của một process; `proc` là *tên* process set, chỉ dùng để đặt tên như vậy[^pm-proc-next][^pm-proc-name].
5. **`Terminating` và `Next`.** `Next` là tuyển của các action tổng của mọi process[^pm-next]; translator thêm một nhánh cho phép thuật toán đã kết thúc làm một bước không làm gì, vì TLC không phân biệt được deadlock với kết thúc (kết thúc chỉ là một dạng deadlock mong muốn)[^pm-done-disj]; nhánh này biến "kết thúc" thành vòng lặp vô hạn không làm gì nên không bị báo deadlock[^pm-done-loop]. Tùy chọn `-noDoneDisjunct` bỏ nhánh này đi[^pm-nodone].
6. **`Spec`.** Khi không chọn tùy chọn liveness hay termination, `Spec` là `Init /\ [][Next]_vars`[^pm-spec-default] — "điều kiện đầu, rồi mỗi bước hoặc là một bước của `Next` hoặc không đổi `vars`" (chương 03). Khi dùng `fair process` hoặc `-wf`, `Spec` có thêm `\A self \in 1..N : WF_vars(Proc(self))`[^pm-spec-fair]. Hình thức này khớp bản dịch thật của spec Dining Philosophers ở mục 4[^ex-dp-spec].
7. **`Termination`.** Bản dịch còn định nghĩa `Termination == <>(\A self \in ProcSet: pc[self] = "Done")`, tức là "cuối cùng mọi process đều ở `Done`"[^pm-termination]. Muốn TLC kiểm tra nó, dùng `-termination`; Toolbox sẽ thêm `Termination` vào danh sách property[^pm-termination-toolbox].

Từ đây bạn viết invariant và property bằng cách dùng `pc`: process `i` đang ở label `L` khi và chỉ khi `pc[i] = "L"`[^pm-pc-label]. Ví dụ mutual exclusion của manual: không có hai process cùng ở câu `cs`, tức invariant `\A i, k \in 1..N : i /= k => ~(pc[i] = "cs" /\ pc[k] = "cs")`[^pm-mutex-def]. Quy tắc vị trí: định nghĩa dùng biến do bản dịch khai báo phải nằm *sau* `END TRANSLATION`[^pm-defs-after]; muốn đặt chúng trong module trước đó thì viết vào khối `define`, nơi `pc` được phép xuất hiện[^pm-define-vars].

<details><summary>Đi sâu: <code>define</code> và hai khai báo <code>VARIABLES</code>, <code>defaultInitValue</code>, dạng của fairness, <code>if</code>/<code>while</code> trong bản dịch</summary>

- Nếu thuật toán có `define`, bản dịch có *hai* câu `VARIABLES`: câu đầu khai báo biến toàn cục và `pc`; các định nghĩa của `define` nằm giữa; câu sau khai báo biến local của process[^pm-define-twovars]. Spec Dining Philosophers thật cho thấy đúng bố cục: `VARIABLES forks, pc`, rồi `(* define statement *)` với các định nghĩa, rồi `VARIABLE hungry`[^ex-dp-define].
- Bản dịch bắt đầu bằng `CONSTANT defaultInitValue` nếu có biến không có giá trị khởi đầu (câu khai báo này bị bỏ nếu mọi biến đều có giá trị khởi đầu)[^pm-defaultinit-decl].
- Dạng đầy đủ của fairness theo manual B.1 với tập process `Proc`: `fair process` cho conjunct `\A self \in 1..N : WF_vars(Proc(self))`; thêm `-` sau label `ncs:` và `cs:` đổi nó thành `WF_vars((pc[self] \notin {"ncs","cs"}) /\ Proc(self))`[^pm-spec-minus]; `fair+` thay `WF` bằng `SF`[^pm-spec-sf]; nếu có procedure `P` thì thêm `\A self \in 1..N : WF_vars(P(self))` cho action của procedure đó[^pm-spec-proc].
- `if` được dịch thành `IF … THEN … ELSE` trong action; manual cho ví dụ action `l2` dùng `IF y # 0 THEN pc' = ... "l3" ELSE pc' = ... "l5"`[^pm-if-trans]. `while` có phép thử không đồng nhất đúng: manual cho ví dụ action `l8`, một `IF` trong action của label đầu vòng lặp; nhánh `THEN` chạy thân vòng lặp rồi đặt `pc'` về chính `l8`, nhánh `ELSE` đặt `pc'` sang `l9`, label sau vòng lặp[^pm-while-trans] (thân vòng lặp của `l8` không chứa label khác). `goto` dịch thành việc đặt `pc'`[^pm-goto-trans].
- Kiểm tra bản dịch: manual nói lệnh Toolbox "Goto PCal Source" nhảy tới vùng PlusCal tương ứng với vùng bản dịch đang chọn[^pm-goto-jump] và gợi ý bạn có thể thử nó trên bất kỳ vùng nào của bản dịch[^pm-goto-pcal]. Manual cũng nói bản dịch sao chép biểu thức của bạn gần như nguyên xi, trừ một số thay đổi (bốn gạch đầu dòng ngay sau câu đó: thêm dấu `'` vào một số biến; biến local của process thành hàm, ví dụ `j[self]`; phép gán vào thành phần viết bằng `EXCEPT`; biến có thể bị đổi tên)[^pm-translation-copies].
- Ví dụ `Peterson` thật trong `tlaplus/Examples`: `await ~c[Other(self)] \/ turn = self` được dịch thành một conjunct trong điều kiện bật của action `a3`[^ex-peterson-await]. Bản dịch thật cũng có một conjunct `TRUE` cho `skip` ở label `a0` (`while(TRUE)`, `skip`)[^ex-peterson-a0]. Manual cho biết bản dịch FastMutex trình bày ở Phụ lục B đã được "simplified a bit to make it easier to read" mà không đổi nghĩa[^pm-simplified]; nên bản do translator sinh ra có thể dài hơn bản trong manual (nhận xét của người soạn, từ so sánh hai nguồn trên).

</details>

**Quay lại Q7:** bản dịch là một máy trạng thái TLA+ chuẩn: `pc` ghi label mỗi process đang ở, mỗi label thành một action `pc[self] = "L" /\ hiệu ứng /\ pc' = ...`, `Next` là tuyển các action (cộng nhánh đứng im khi kết thúc), `Spec == Init /\ [][Next]_vars` (thêm `WF` khi `fair`), và `stack` là chuỗi bản ghi cho `call`/`return`.

## 8. Ví dụ: hai thread tăng counter — lost update và cách sửa bằng lock (Q8)

> **Trả lời ngắn.** Nếu mô hình hóa `counter += 1` thành hai bước (đọc vào biến local `tmp`, rồi ghi `tmp + 1`), TLC có thể thử thứ tự "cả hai đọc 0, rồi cả hai ghi 1": cả hai thread xong nhưng `counter = 1` thay vì 2 — lost update. Thêm biến `lock` cùng một label `Acquire` (`await lock = 0; lock := self`) trước đoạn đọc–ghi và một label `Release` (`lock := 0`) sau nó thì hai thread không thể cùng ở giữa đoạn đọc–ghi, và tính chất `Correct` được giữ (lập luận ở bước 2; chưa chạy TLC).

**Giải thích.**

Hình dung (nhận xét của người soạn): hai nhân viên cùng sửa một con số trên bảng — đọc ra giấy nháp, tính, chép lại. Ca tệ là cả hai đọc cùng một giá trị rồi chép đè nhau. P4·03 mục 1 mô tả kịch bản này ở mức máy (nạp–cộng–ghi); ở đây ta mô hình hóa nó bằng label.

Learn TLA+ trình bày đúng bài này: hai thread cùng tăng một counter; ban đầu cho cập nhật nguyên tử và thấy kết quả đúng, rồi làm cập nhật không nguyên tử và cho thấy có race condition, rồi thêm lock[^lt-threads-intro]. Dưới đây là phiên bản viết riêng cho chương (người soạn tự viết, lấy ý tưởng cấu trúc từ Learn TLA+; chưa chạy TLC).

**Bước 1 — spec có lost update.**

```tla
---------------------------- MODULE Counter ----------------------------
EXTENDS Naturals, TLC

CONSTANT NumThreads
Threads == 1..NumThreads

(* --algorithm Counter
variables counter = 0;

define
  AllDone == \A t \in Threads : pc[t] = "Done"
  Correct == AllDone => counter = NumThreads
end define;

process thread \in Threads
variable tmp = 0;                    \* thanh ghi/biến local của mỗi thread
begin
  Load:  tmp := counter;             \* bước 1: đọc
  Store: counter := tmp + 1;         \* bước 2: ghi
end process;
end algorithm; *)

\* BEGIN TRANSLATION
\* END TRANSLATION

TypeOK == /\ counter \in 0..NumThreads
          /\ tmp \in [Threads -> 0..NumThreads]
          /\ pc \in [Threads -> {"Load", "Store", "Done"}]
=============================================================================
```

Cả hai thread dùng chung `counter` (biến toàn cục) và có `tmp` riêng (biến local). `Correct` nói: *khi mọi thread đã Done*, `counter` phải bằng `NumThreads`. Với model đặt `NumThreads = 2` và `Correct` là invariant, TLC sẽ duyệt mọi thứ tự xen kẽ các bước `Load`/`Store` của hai thread.

Learn TLA+ kể kết quả của spec tương tự: cả hai thread đọc `counter` khi nó bằng 0, nên đều đặt `tmp` bằng 0, nên đều gán `counter := 0 + 1`[^lt-threads-fail]. Ta tự dẫn ra đúng thứ tự đó từ ngữ nghĩa ở mục 2 (suy luận của người soạn; không phải output TLC): bắt đầu `counter = 0`, `tmp = [1 |-> 0, 2 |-> 0]`, `pc = [1 |-> "Load", 2 |-> "Load"]`:

| # | Bước | `tmp` | `counter` | `pc` | `Correct` |
|---|---|---|---|---|---|
| 0 | khởi đầu | `<<0, 0>>` | 0 | `<<Load, Load>>` | đúng (chưa Done) |
| 1 | `Load(1)` | `<<0, 0>>` | 0 | `<<Store, Load>>` | đúng |
| 2 | `Load(2)` | `<<0, 0>>` | 0 | `<<Store, Store>>` | đúng |
| 3 | `Store(1)` | `<<0, 0>>` | 1 | `<<Done, Store>>` | đúng |
| 4 | `Store(2)` | `<<0, 0>>` | **1** | `<<Done, Done>>` | **sai**: `AllDone` đúng, `counter = 1 ≠ 2` |

(`tmp` viết dưới dạng tuple cho gọn; trong TLA+ nó là hàm trên `Threads`.)

```mermaid
sequenceDiagram
  participant T1 as thread 1
  participant C as counter
  participant T2 as thread 2
  Note over C: counter = 0
  T1->>C: Load - tmp[1] nhận 0
  T2->>C: Load - tmp[2] nhận 0
  T1->>C: Store - counter nhận tmp[1] + 1 = 1
  T2->>C: Store - counter nhận tmp[2] + 1 = 1
  Note over C: cả hai Done, counter = 1 thay vì 2
```

Đây là cùng kịch bản của P4·03 mục 1: hai lần tăng, kết quả chỉ tăng một.

*Bản dịch (viết tay theo manual B.1 và mẫu `Simple.tla` ở mục 7; không phải output của translator).* Vì có `define` nên bản dịch tách hai câu `VARIABLES`:

```tla
VARIABLES counter, pc

(* define statement *)
AllDone == \A t \in Threads : pc[t] = "Done"
Correct == AllDone => counter = NumThreads

VARIABLE tmp

vars == << counter, pc, tmp >>

ProcSet == (Threads)

Init == (* Global variables *)
        /\ counter = 0
        (* Process thread *)
        /\ tmp = [self \in Threads |-> 0]
        /\ pc = [self \in ProcSet |-> "Load"]

Load(self) == /\ pc[self] = "Load"
              /\ tmp' = [tmp EXCEPT ![self] = counter]
              /\ pc' = [pc EXCEPT ![self] = "Store"]
              /\ UNCHANGED counter

Store(self) == /\ pc[self] = "Store"
               /\ counter' = tmp[self] + 1
               /\ pc' = [pc EXCEPT ![self] = "Done"]
               /\ tmp' = tmp

thread(self) == Load(self) \/ Store(self)

(* Allow infinite stuttering to prevent deadlock on termination. *)
Terminating == /\ \A self \in ProcSet: pc[self] = "Done"
               /\ UNCHANGED vars

Next == (\E self \in Threads: thread(self))
           \/ Terminating

Spec == Init /\ [][Next]_vars

Termination == <>(\A self \in ProcSet: pc[self] = "Done")
```

Điều đáng nhìn: `Load(self)` đọc `counter` *ở trạng thái trước* và ghi vào `tmp[self]`; `Store(self)` dùng `tmp[self]` (đã cũ) chứ không đọc lại `counter`. Lost update nằm trong chính sự tách này: giữa `Load(1)` và `Store(1)`, `Next` có quyền chọn `Load(2)` (và sau đó cả `Store(2)`), nên `Store(1)` ghi đè bằng giá trị tính từ `counter` đã cũ.

Vì `TypeOK` dùng `tmp` — biến local mà `define` không dùng được[^pm-define-vars] — nó phải viết sau `END TRANSLATION`; Learn TLA+ cũng nêu đúng điểm này khi nói về TypeInvariant của ví dụ threads[^lt-typeinv-placement].

**Bước 2 — sửa bằng lock.**

```tla
(* --algorithm CounterLock
variables counter = 0, lock = 0;     \* lock = 0: tự do; lock = t: thread t đang giữ
                                      \* (0 không thuộc Threads nên không nhầm)

define
  AllDone == \A t \in Threads : pc[t] = "Done"
  Correct == AllDone => counter = NumThreads
  InCS(t) == pc[t] \in {"Load", "Store", "Release"}
  MutualExclusion ==
    \A t1, t2 \in Threads : t1 /= t2 => ~(InCS(t1) /\ InCS(t2))
end define;

process thread \in Threads
variable tmp = 0;
begin
  Acquire: await lock = 0;
           lock := self;
  Load:    tmp := counter;
  Store:   counter := tmp + 1;
  Release: lock := 0;
end process;
end algorithm; *)
```

Điều thay đổi: `Acquire` chỉ chạy được khi `lock = 0` (enabling condition, mục 5/7), và khi chạy nó đặt `lock` bằng `self` trong *cùng một bước nguyên tử* — kiểm tra và chiếm là một thể, như một lệnh compare-and-swap (nhận xét của người soạn). `Load` và `Store` vẫn là hai bước tách, nên thread khác vẫn có thể được lên lịch giữa chúng, nhưng thread khác không thể *qua Acquire* nữa. Bản dịch của `Acquire` có cùng hình dạng với `l1(self)` của `Lock.tla` (mục 6):

```tla
Acquire(self) == /\ pc[self] = "Acquire"
                 /\ lock = 0
                 /\ lock' = self
                 /\ pc' = [pc EXCEPT ![self] = "Load"]
                 /\ UNCHANGED << counter, tmp >>
```

(Minh họa viết tay.) Lập luận (suy luận của người soạn): người giữ lock là thread có `pc` ở một trong `Load`, `Store`, `Release` — từ lúc `Acquire` chạy xong đến lúc `Release` chạy xong. `Acquire` của thread khác bị chặn cho đến khi `Release` đặt `lock` về 0. Do đó không bao giờ có hai thread cùng ở `{Load, Store, Release}` — đó là `MutualExclusion` — và mỗi thread tăng `counter` đúng một lần trong đoạn đó, không ai chen vào, nên khi cả hai Done thì `counter = NumThreads`. Learn TLA+ báo rằng spec có lock của họ vượt qua kiểm tra sau khi thêm lock[^lt-threads-pass]; spec của chương này chưa được TLC chạy, nên kết luận ở đây là lập luận, chưa phải kết quả đo.

`MutualExclusion` ở dạng này trực tiếp là "không có hai process cùng ở một nhóm label", cùng cách manual biểu diễn mutual exclusion của FastMutex bằng `pc`[^pm-mutex-def]. Spec `Lock.tla` trong `tlaplus/Examples` (mục 6) cũng gom các label giữ lock vào một định nghĩa — `lockcs(i) == pc[i] \in {"cs", "l2"}`, trong đó `l2` là label nhả lock[^ex-lock-inv] — rồi đòi `\A i, j \in ProcSet: (i # j) => ~(lockcs(i) /\ lockcs(j))`[^ex-lock-inv2]; `InCS` ở trên gồm `Release` vì cùng lý do. Learn TLA+ còn gợi ý thêm `assert lock = self` ở `Release` để chắc rằng thread đang nhả lock thật sự giữ nó[^lt-assert-release] (cần `EXTENDS TLC`, đã có ở trên[^pm-assert-ext]).

**Hai điều spec này *không* chứng minh.** Một: `Correct` chỉ nói *nếu* mọi thread kết thúc thì kết quả đúng; Learn TLA+ lưu ý điều này — nếu spec cho một thread lặp vô ích mãi thì invariant được thỏa tầm thường[^lt-alldone-weak]. Muốn nói "cuối cùng chắc chắn đúng" cần `Termination` và fairness (`fair process`, mục 4; chương 05). Hai: lock ở đây chỉ bảo đảm loại trừ lẫn nhau, không bảo đảm công bằng giữa các thread — đó cũng là chuyện của liveness.

<details><summary>Đi sâu: cùng thuật toán bằng C-syntax; lock sai phạm vi; symmetry; kết quả TLC ở các nguồn</summary>

Cùng bước 2 viết bằng C-syntax (dấu ngoặc nhọn thay cho `begin`/`end process`, theo ngữ pháp ở Phụ lục A của manual C):

```tla
(* --algorithm CounterLock {
variables counter = 0, lock = 0;

define {
  AllDone == \A t \in Threads : pc[t] = "Done"
  Correct == AllDone => counter = NumThreads
  InCS(t) == pc[t] \in {"Load", "Store", "Release"}
  MutualExclusion ==
    \A t1, t2 \in Threads : t1 /= t2 => ~(InCS(t1) /\ InCS(t2))
}

process (thread \in Threads)
variable tmp = 0;
{
  Acquire: await lock = 0;
           lock := self;
  Load:    tmp := counter;
  Store:   counter := tmp + 1;
  Release: lock := 0
}
} *)
```

Ngữ pháp C-syntax: `define { defs }`[^pm-c-grammar-defs] và `process ( Name \in Expr ) VarDecls CompoundStmt`, dấu chấm phẩy cuối là tùy chọn[^pm-c-grammar-define]. Phần dịch của process giống hệt bản P-syntax vì cú pháp chỉ là bề ngoài (suy luận của người soạn: cùng ngữ nghĩa theo hai manual).

*Lock sai phạm vi: lost update dù có lock.* Biến thể này giữ lock riêng quanh `Load` và quanh `Store`:

```tla
process thread \in Threads
variable tmp = 0;
begin
  AcquireR: await lock = 0;
            lock := self;
  Load:     tmp := counter;
  ReleaseR: lock := 0;
  AcquireW: await lock = 0;
            lock := self;
  Store:    counter := tmp + 1;
  ReleaseW: lock := 0;
end process;
```

Từng thao tác `Load`/`Store` vẫn được bảo vệ, nhưng khoảng giữa `ReleaseR` và `AcquireW` để lại đúng cửa sổ như bước 1: `Load(1)`, `Load(2)` (mỗi thread lần lượt acquire–load–release), rồi `Store(1)`, `Store(2)` (mỗi thread acquire–store–release) cho `counter = 1` khi cả hai Done (suy luận của người soạn từ ngữ nghĩa bước). Mỗi đoạn có lock riêng lẻ vẫn loại trừ lẫn nhau, nhưng `Correct` sai. Bài học (nhận xét của người soạn): tính "không có hai thread cùng trong một critical section" không đồng nghĩa với tính đúng đắn của *cả thao tác đọc–sửa–ghi*; invariant bạn chọn phải là tính chất bạn thực sự cần (ở đây là `Correct`).

*Symmetry.* Manual nói TLC chỉ dùng được symmetry cho một tập *model value*; để tận dụng symmetry của FastMutex cần viết lại thuật toán dùng tập định danh tùy ý thay cho `1..N`[^pm-symmetry]. `Threads == 1..NumThreads` ở trên là tập số, nên không dùng symmetry được như hiện tại (suy luận của người soạn từ câu trên). Manual cũng dặn không bật symmetry cùng lúc với kiểm tra liveness[^pm-symmetry-liveness].

</details>

**Quay lại Q8:** khi tách `counter += 1` thành hai label (`Load`, `Store`), TLC được quyền xen bước của thread kia vào giữa nên có thứ tự cả hai đọc 0 và cùng ghi 1 (lost update); thêm `await lock = 0; lock := self` ở bước `Acquire` và `lock := 0` ở `Release` ngăn hai thread cùng ở đoạn đọc–ghi.

## 9. Liên hệ với Rust và backend

PlusCal không sinh code Rust; nó giúp bạn nghĩ về *thiết kế* trước khi viết code. Mối liên hệ cụ thể:

**Process ↔ thread/task; label ↔ độ hạt nguyên tử.** `process thread \in Threads` ứng với `N` lần `std::thread::spawn` (hoặc `tokio::spawn`) chạy cùng hàm; `self` là chỉ số worker; biến local ứng với biến trên stack; biến toàn cục ứng với dữ liệu dùng chung. Label là câu hỏi quan trọng nhất: trong code thật, đoạn nào *thật sự* không thể bị chen? Một lệnh `AtomicUsize::fetch_add` là một thao tác "cộng vào giá trị hiện tại và trả về giá trị trước"[^rust-fetch-add], nên mô hình hóa bằng *một* label (bước (a) ở mục 2). Còn `load` rồi `store` là *hai* thao tác atomic riêng — hai label — và có thể mất cập nhật *dù cả hai đều atomic*. Nomicon nói chính điều này: Safe Rust bảo đảm không có data race, nhưng Rust không ngăn được race condition nói chung[^rust-race]. PlusCal bắt đúng loại lỗi đó, loại mà borrow checker không bắt.

**`await lock = 0; lock := self` ↔ `Mutex::lock`.** `Mutex::lock` chặn thread hiện tại cho đến khi lấy được mutex[^rust-mutex-lock], tương ứng với việc `await` làm bước không chạy được cho đến khi `lock = 0`. Lock thả khi guard ra khỏi scope[^rust-mutex-drop], tương ứng với label `Release`. Ví dụ lock sai phạm vi ở mục 8 có hình dạng thật trong Rust (minh họa của người soạn, không chạy):

```rust
use std::sync::{Arc, Mutex};
use std::thread;

fn main() {
    let counter = Arc::new(Mutex::new(0u32));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let c = Arc::clone(&counter);
            thread::spawn(move || {
                // AcquireR; Load; ReleaseR — guard hết scope ở cuối khối
                let tmp = {
                    let g = c.lock().unwrap();
                    *g
                };
                // AcquireW; Store; ReleaseW — một critical section khác
                let mut g = c.lock().unwrap();
                *g = tmp + 1;
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    println!("{}", *counter.lock().unwrap());
}
```

Mỗi truy cập vào `counter` đều qua mutex, nên không có data race — Safe Rust bảo đảm không có data race[^rust-safe-race]. Nhưng đọc và ghi nằm ở hai critical section riêng — cùng cấu trúc bốn bước như biến thể sai phạm vi ở mục 8; theo suy luận ở bước đó, có interleaving cho kết quả 1. Cách sửa trong Rust: giữ một guard qua cả đọc và ghi (`let mut g = c.lock().unwrap(); *g += 1;`), tức một critical section duy nhất, ứng với phiên bản đúng ở bước 2 mục 8.

**`.await` ↔ ranh giới label (suy luận của người soạn).** Với async Rust, `poll` cố giải quyết future thành giá trị cuối cùng và không chặn nếu giá trị chưa sẵn sàng; thay vào đó task được lên lịch đánh thức lại khi có thể tiến tiếp[^rust-poll]. Điểm có thể bị đổi sang task khác là chỗ `poll` trả về chưa xong (nơi bạn viết `.await`); đoạn code giữa hai `.await` chạy liền mạch trên một task. Nên khi mô hình hóa một hệ async, đặt label ở các `.await` — mỗi đoạn giữa hai `.await` là một bước nguyên tử *đối với các task khác trên cùng executor đơn luồng*; với executor đa luồng, các đoạn trên thread khác vẫn song song, nên phải đặt label chặt hơn. Đây là mô hình hóa của người soạn, không phải lời của tài liệu.

**Backend.** (Nhận xét của người soạn.) Các cấu trúc PlusCal khớp thẳng với thiết kế backend: `await` ↔ chờ lock/semaphore/queue; `either` ↔ môi trường có thể làm nhiều việc (mạng mất gói, replica chết); `with m \in inbox` ↔ nhận một message bất kỳ trong hàng đợi không có thứ tự; `process \in Workers` ↔ thread pool, consumer group, replica. Chương 08 và 09 áp dụng cấu trúc này cho giao thức phân tán và hệ backend; chương 10 (từ đặc tả đến code Rust) nói tiếp việc nối spec với code.

## Tóm tắt

- PlusCal là ngôn ngữ thuật toán viết trong comment của module TLA+; translator dịch nó thành TLA+ giữa `BEGIN/END TRANSLATION`, và TLC kiểm tra bản dịch; biểu thức bên trong là TLA+.
- Hai cú pháp (P-syntax dài, C-syntax gọn) cùng ngữ nghĩa, khác ở dấu ngoặc nhọn thay cho `begin … end`.
- **Bước nguyên tử = đoạn thực thi từ một label đến label kế**; process khác chỉ chen được giữa hai bước. Vị trí label là độ hạt nguyên tử của mô hình.
- Label bắt buộc ở đầu process/procedure/thuật toán, ở `while`, sau `goto`/`call`/`return`, sau `if`/`either` có label bên trong; cấm trong `macro` và `do` của `with`; giữa hai phép gán cùng biến phải có label.
- `process P \in S` tạo một tập process với `self`; biến local thành hàm theo `self`; mặc định process không fair, `fair process` thêm weak fairness, `fair+` thêm strong fairness.
- `await`/`when` chặn bước; `either` và `with` là hai cách chọn không tất định; không process nào chạy được mà chưa kết thúc là deadlock (TLC báo).
- `macro` là thay văn bản trong bước của chỗ gọi; `procedure` là hàm nhiều bước có `call`/`return`, dịch bằng biến `stack`; procedure không trả giá trị trực tiếp.
- Bản dịch: `pc` (hàm theo process khi nhiều process), `vars`, `Init`, một action mỗi label (`pc[self] = "L" /\ … /\ pc' = …`), `Next` (cộng nhánh đứng im khi kết thúc), `Spec`, `Termination`.
- Lost update với hai thread không khóa: tách `counter += 1` thành hai label (`Load`, `Store`) rồi kiểm tra `AllDone => counter = NumThreads`; sửa bằng `lock` với `await lock = 0` ở một label riêng; invariant viết sau `END TRANSLATION` hoặc trong `define`.
- Invariant chỉ chứng minh tính đúng *nếu* kết thúc; muốn "cuối cùng chắc chắn đúng" cần liveness và fairness (chương 05).

## Câu hỏi tự kiểm tra

1. PlusCal khác một ngôn ngữ lập trình ở ba điểm nào theo manual, và biểu thức trong PlusCal là ngôn ngữ nào?
2. Vùng giữa `BEGIN TRANSLATION` và `END TRANSLATION` có nên sửa tay không? Vì sao?
3. Định nghĩa "bước" (step) của PlusCal là gì? Một vòng lặp `while` có label ở đầu là mấy bước cho mỗi lần lặp?
4. Cho đoạn `a: x := x + 1; x := x * 2;`. Vì sao sai, và hai cách sửa?
5. Nêu năm chỗ bắt buộc phải có label và hai chỗ cấm có label.
6. `process worker \in 1..3` và `process worker = 1` khác nhau thế nào? `self` có ý nghĩa gì trong từng trường hợp?
7. `fair process` thêm gì vào `Spec`? Nếu bỏ `fair`, điều gì có thể xảy ra với một tính chất liveness?
8. `await FALSE;` trong một bước làm gì? Nếu mọi process cùng kẹt ở `await` sai thì TLC báo gì?
9. `either A or B end either` và `with x \in S do ... end with` khác nhau chỗ nào; `with x \in {}` làm gì?
10. `macro` và `procedure` khác nhau ở những điểm nào về số bước, label, và biến thêm vào bản dịch?
11. Bản dịch của label `Store` trong ví dụ `Counter` gồm những conjunct nào?
12. Mô tả thứ tự các bước dẫn tới lost update trong ví dụ hai thread, và nêu một cách sửa dùng `await`.

<details><summary>Đáp án</summary>

1. Thuật toán thao tác trên đối tượng toán học tùy ý; một thuật toán có thể mô tả một lớp phép tính; và phải xác định cái gì là một bước nguyên tử, điều chương trình thiếu[^pm-steps]. Biểu thức là TLA+[^pm-expr]. (Mục 1.)
2. Không: translator xóa mọi thứ giữa hai dòng đó và thay bằng bản dịch mới[^pm-begin-trans]. Sửa thuật toán trong comment rồi dịch lại; định nghĩa thêm của bạn đặt sau `END TRANSLATION` hoặc trong `define`[^pm-defs-after]. (Mục 1, 7.)
3. Một bước là control path bắt đầu ở một label, kết thúc ở một label và không qua label nào khác[^pm-step-cp]. Mỗi lần thực thi cả thân vòng lặp từ label đầu về lại label đầu là một bước[^pm-while-exit]; muốn nhiều bước mỗi vòng thì đặt label trong thân. (Mục 2.)
4. Hai phép gán vào `x` trong cùng đường đi từ label không có label giữa vi phạm quy tắc[^pm-lab-assign]. Sửa: `a: x := x + 1; b: x := x * 2;` (hai bước) hoặc `a: x := (x + 1) * 2;` (một bước). (Mục 3.)
5. Bắt buộc: câu đầu của process, của procedure, của thuật toán một process; mỗi `while`; câu sau `goto`/`return`, và câu sau `call` (trừ khi câu kế là `return` hoặc `goto`); câu sau `if`/`either` chứa label/`goto`/`call`/`return` bên trong[^pm-lab-first][^pm-lab-while][^pm-lab-after][^pm-lab-if]. Cấm: thân `macro` và phần `do` của `with`[^pm-lab-macro]. (Mục 3.)
6. `\in 1..3` tạo ba process có định danh 1, 2, 3, mỗi process dùng `self` để biết mình là ai; `= 1` tạo đúng một process có định danh 1[^pm-proc-set]. Manual nói `self` bằng định danh của process hiện hành *trong thân của process set*[^pm-proc-self]. Vì vậy nếu cần `self` (hoặc macro dùng `self`) mà chỉ có một process, viết `process P \in {val}` (suy luận của người soạn từ câu trên của manual). (Mục 4.)
7. Thêm conjunct weak fairness `\A self \in S : WF_vars(P(self))` vào `Spec`[^pm-spec-fair][^ex-dp-spec]. Bỏ `fair`, process được phép dừng mãi ở bất kỳ bước nào[^ex-dp-fair-comment], nên tính chất liveness kiểu "cuối cùng sẽ xảy ra" có thể sai trên hành vi đứng im. (Mục 4, 7.)
8. Bước không chạy được, không gì thay đổi, process thử lại sau[^pm-await-fail]. Nếu không process nào đi tiếp được mà còn process chưa kết thúc thì đó là deadlock, TLC báo[^pm-deadlock]. (Mục 5.)
9. `either` chọn một *nhánh* chạy được trong số các nhánh[^pm-either]; `with` chọn một *phần tử* của tập rồi chạy thân với phần tử đó; `with x \in {}` không chạy được[^pm-with]. (Mục 5.)
10. Macro: thay văn bản lúc dịch, nằm trong bước của chỗ gọi, không label/`while`/`call`/`return`/`goto`[^pm-macro][^pm-macro-body], và (suy luận từ việc mở rộng lúc dịch) không có biến riêng trong bản dịch. Procedure: nhiều bước, có label, `call`/`return`, thêm `stack` cùng tham số và biến local vào bản dịch, module cần `EXTENDS Sequences`[^pm-stack-add][^pm-proc-seq]. (Mục 6.)
11. `pc[self] = "Store"`; `counter' = tmp[self] + 1`; `pc' = [pc EXCEPT ![self] = "Done"]`; `tmp' = tmp` (người soạn viết tay theo mẫu `Simple.tla`)[^ex-simple-a]. (Mục 8.)
12. Cả hai thread `Load` (đều đọc 0), rồi `Store(1)` ghi 1, rồi `Store(2)` ghi `tmp[2] + 1 = 1`; khi cả hai Done, `counter = 1 ≠ 2`[^lt-threads-fail]. Sửa: thêm biến `lock = 0`, bước `Acquire: await lock = 0; lock := self;` trước `Load`, và `Release: lock := 0;` sau `Store`. (Mục 8.)

</details>

## Đọc thêm

- [PlusCal User's Manual, P-syntax](https://lamport.azurewebsites.net/tla/p-manual.pdf) — mục 2 (khởi động), 3.7 (quy tắc label), 3.8 (tên do bản dịch định nghĩa), phụ lục B (bản dịch FastMutex và procedure).
- [PlusCal User's Manual, C-syntax](https://lamport.azurewebsites.net/tla/c-manual.pdf) — cùng nội dung với cú pháp ngoặc nhọn; phụ lục A có ngữ pháp C-syntax.
- [Learn TLA+ — Concurrency](https://learntla.com/core/concurrency.html) (trang sống) — process, `await`, procedure và ví dụ threads từng bước.
- [tlaplus/Examples](https://github.com/tlaplus/Examples/tree/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications) — `TeachingConcurrency/Simple.tla`, `locks_auxiliary_vars/Lock.tla` và `Peterson.tla`, `DiningPhilosophers/DiningPhilosophers.tla`: PlusCal kèm bản dịch thật.
- [tlaplus/tlaplus — `test-model/pcal`](https://github.com/tlaplus/tlaplus/tree/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/test-model/pcal) — file test của translator, gồm `Factorial.tla` (procedure), `Either1.tla`.
- [Chương 03](03-trang-thai-hanh-vi-action.md), [04](04-invariant-va-tlc.md), [05](05-liveness-va-fairness.md) — các khái niệm TLA+ mà bản dịch PlusCal dựa vào; [07](07-refinement.md) — một thuật toán thực thi một thuật toán khác; [08](08-dac-ta-giao-thuc-phan-tan.md), [09](09-mo-hinh-hoa-he-backend.md), [10](10-tu-dac-ta-den-code-rust.md) — dùng PlusCal cho giao thức, hệ backend, và nối sang code.
- [Phase 4 · Chương 03 — Lock](../04-concurrency/03-lock.md), [Chương 05 — Deadlock](../04-concurrency/05-concurrency-bug-va-deadlock.md) — hai kết quả ở mức hệ điều hành mà mục 8 và mục 5 mô hình hóa.

[^pm-algo]: PlusCal User's Manual (P-syntax), mục 1 Introduction — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "PlusCal is an algorithm language. An algorithm language is meant for writing algorithms, not programs."
[^pm-translator]: PlusCal User's Manual (P-syntax), mục 1 Introduction — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The PlusCal translator compiles a PlusCal algorithm into a TLA+ specification."
[^pm-tlc]: PlusCal User's Manual (P-syntax), mục 1 Introduction — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "When used in model-checking mode, TLC will check all possible executions of the algorithm."
[^pm-expr]: PlusCal User's Manual (P-syntax), mục 1 Introduction — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The language of PlusCal expressions is TLA+, a high-level specification language based on set theory and first-order logic"
[^pm-steps]: PlusCal User's Manual (P-syntax), mục 1 Introduction — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Execution of an algorithm consists of a sequence of steps. An algorithm's computational complexity is the number of steps it takes to compute the result; defining a concurrent algorithm requires specifying what constitutes a single (atomic) step. There is no well-defined notion of a step of a program."
[^pm-differ-objects]: PlusCal User's Manual (P-syntax), mục 1 Introduction — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Algorithms perform operations on arbitrary mathematical objects, such as graphs and vector spaces. Programs perform operations on simple objects such as Booleans and integers;"
[^pm-differ-class]: PlusCal User's Manual (P-syntax), mục 1 Introduction — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A program describes one method of computing a result; an algorithm may describe a class of possible computations."
[^pm-nondet]: PlusCal User's Manual (P-syntax), mục 1 Introduction — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "PlusCal provides simple constructs for expressing nondeterminism."
[^pm-in-comment]: PlusCal User's Manual (P-syntax), mục 2.2 The TLA+ Module — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The algorithm itself should appear in the module within a single comment:"
[^pm-begin-trans]: PlusCal User's Manual (P-syntax), mục 2.2 The TLA+ Module — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "If those lines are already present, the translator will delete everything between them and replace it with the TLA+ translation of the algorithm."
[^pm-grammar]: PlusCal User's Manual (P-syntax), Phụ lục A The Grammar — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "hAlgorithm i ::= [ --algorithm | --fair algorithm ] hname i hVarDecls i0,1 hDefinitions i0,1 hMacro i∗ hProcedure i∗ [hAlgorithmBody i | hProcess i+ ] end algorithm"
[^pm-assert-ext]: PlusCal User's Manual (P-syntax), mục 3.2.9 Assert — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "An algorithm containing an assert statement must be in a module that extends the TLC module."
[^pm-print-ext]: PlusCal User's Manual (P-syntax), mục 3.2.8 Print — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "An algorithm containing a print statement must be in a module that extends the TLC module."
[^pm-proc-seq]: PlusCal User's Manual (P-syntax), mục 3.4 Procedures — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "If it does, the algorithm must be in a TLA+ module that extends the Sequences module."
[^pm-init-in]: PlusCal User's Manual (P-syntax), mục 2.1 Typing the Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The declaration of v asserts that its initial value is an element of the set 1 .. N of integers from 1 through N."
[^pm-defaultinit]: PlusCal User's Manual (P-syntax), mục 4.5 Invariance Checking — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "If no initial value is specified for a variable, its default initial value is an unspecified constant named defaultInitValue."
[^pm-two-syn]: PlusCal User's Manual (P-syntax), The Two Syntaxes — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "PlusCal has two separate syntaxes, the prolix p-syntax and the more compact c-syntax."
[^pm-c-compound]: PlusCal User's Manual (C-syntax), mục 3.2 The Statements — <https://lamport.azurewebsites.net/tla/c-manual.pdf> — "A compound statement consists of a sequence of statements separated by semicolons, with an optional semicolon at the end, that is enclosed in braces"
[^pm-c-procbody]: PlusCal User's Manual (C-syntax), mục 3.3 Processes — <https://lamport.azurewebsites.net/tla/c-manual.pdf> — "The process body is a sequence of statements enclosed by braces"
[^pm-c-grammar]: PlusCal User's Manual (C-syntax), Phụ lục A The Grammar — <https://lamport.azurewebsites.net/tla/c-manual.pdf> — "hUnlabeledStmt i ::= hAssign i | hIf i | hWhile i | hEither i | hWith i | hAwait i | hPrint i | hAssert i | hSkip i | hReturn i | hGoto i | hCall i | hMacroCall i"
[^pm-c-braces]: PlusCal User's Manual (C-syntax), mục 2.6 A Multiprocess Algorithm — <https://lamport.azurewebsites.net/tla/c-manual.pdf> — "In the c-syntax, any sequence of statements can be enclosed in { } braces, and the label of the first statement can appear either before or after the opening brace."
[^pm-typecorrect]: PlusCal User's Manual (P-syntax), mục 4.5 Invariance Checking — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Because PlusCal is typeless, type correctness is a property of the algorithm, asserting that the value of each variable is an element of the proper set."
[^pm-java]: PlusCal User's Manual (P-syntax), mục 1 Introduction — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The translator and TLC are ordinary Java programs and can be run from a command line."
[^pm-cfg]: PlusCal User's Manual (P-syntax), Phụ lục C Translator Options — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "It then produces a configuration (.cfg) file that can be used to run TLC from the command line."
[^pm-options-stmt]: PlusCal User's Manual (P-syntax), Phụ lục C Translator Options — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "In the options statement, the \"-\" in the option name may be omitted."
[^ex-peterson-chksum]: tlaplus/Examples @ 6a7b26a, specifications/locks_auxiliary_vars/Peterson.tla — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/locks_auxiliary_vars/Peterson.tla> — "\* BEGIN TRANSLATION (chksum(pcal) = "1d547bc3" /\ chksum(tla) = "8de86c82")"
[^pm-reuse]: PlusCal User's Manual (P-syntax), mục 3.1 Expressions — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "TLA+ has the general rule that an identifier cannot be assigned a new meaning if it already has a meaning."
[^pm-redefine]: PlusCal User's Manual (P-syntax), mục 3.8 The Translation's Definitions and Declarations — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Redefining an identifier declared or defined by the translation, or using it as a bound variable, will cause a"
[^pm-trans-ids]: PlusCal User's Manual (P-syntax), mục 3.8 The Translation's Definitions and Declarations — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Each variable declared either globally or locally within a process or a procedure."
[^pm-c-reserved]: PlusCal User's Manual (P-syntax), Phụ lục A The Grammar — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The PlusCal reserved words are assert await begin call define do either else elsif end goto if macro or print procedure process return skip then variable variables when while with"
[^pm-error-source]: PlusCal User's Manual (P-syntax), mục 4.1 Running the Translator — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The lesson to be learned from this example is that the source of an error can come well before the location where the error is reported."
[^pm-no-parse]: PlusCal User's Manual (P-syntax), mục 4.1 Running the Translator — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "the translator does not try to parse expressions. It leaves that task to the SANY parser, which is called by the Toolbox."
[^pm-assume-expr]: PlusCal User's Manual (P-syntax), mục 4.1 Running the Translator — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "the translator just assumes that everything until the next reserved symbol is part of the assignment statement's expression."
[^pm-step-label]: PlusCal User's Manual (P-syntax), mục 2.6 A Multiprocess Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A single step (atomic action) of a PlusCal algorithm consists of the execution from one label to the next."
[^pm-step-cp]: PlusCal User's Manual (P-syntax), mục 3 The Language — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A step is a control path that starts at a label, ends at a label, and passes through no other labels."
[^pm-exec-steps]: PlusCal User's Manual (P-syntax), mục 3 The Language — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "An execution of a PlusCal algorithm consists of a sequence of executions of steps. Part of a step can never be executed by itself"
[^pm-pc-done]: PlusCal User's Manual (P-syntax), mục 2.6 A Multiprocess Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "There is an implicit label Done at the end of every process, and at the end of a uniprocess algorithm."
[^pm-multi-exec]: PlusCal User's Manual (P-syntax), mục 3.3 Processes — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A multiprocess algorithm is executed by repeatedly choosing an arbitrary process and executing one step of that process"
[^pm-impossible]: PlusCal User's Manual (P-syntax), mục 3.3 Processes — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "if the process has terminated, if its next step contains an await statement whose expression equals false"
[^pm-while-exit]: PlusCal User's Manual (P-syntax), mục 2.6 A Multiprocess Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "that is, a complete execution of the body of the while loop. If j > N , a step that begins at l8 performs the while test and ends with control at l9."
[^lt-nonatomic]: Learn TLA+, Concurrency (trang sống, truy cập 2026-10-12) — <https://learntla.com/core/concurrency.html> — "The while loop is nonatomic, and every iteration counts as a separate Write action."
[^pm-step-def]: PlusCal User's Manual (P-syntax), mục 1 Introduction — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "defining a concurrent algorithm requires specifying what constitutes a single (atomic) step."
[^pm-grain]: PlusCal User's Manual (P-syntax), mục 1 Introduction — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "For multiprocess (concurrent) algorithms, you will probably want to specify the grain of atomicity explicitly."
[^lt-explosion]: Learn TLA+, Concurrency (trang sống, truy cập 2026-10-12) — <https://learntla.com/core/concurrency.html> — "Concurrency is one of the main sources of state space explosion."
[^pm-fastmutex-l2]: PlusCal User's Manual (P-syntax), mục 2.6 A Multiprocess Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "the execution of the test y 6= 0 at label l2 is atomic because a single step that begins at l2 ends when control reaches either l3 or l4."
[^pm-goto]: PlusCal User's Manual (P-syntax), mục 3.2.11 Goto — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "ends the execution of the current step and causes control to go to the statement labeled lab."
[^pm-await-pos]: PlusCal User's Manual (P-syntax), mục 3.2.5 Await (When) — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Although it usually appears at the beginning of a step, an await statement can appear anywhere within the step."
[^pm-await-equiv]: PlusCal User's Manual (P-syntax), mục 3.2.5 Await (When) — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The step from a to b can be executed only when the current value of y+1 is positive."
[^lt-await-odd]: Learn TLA+, Concurrency (trang sống, truy cập 2026-10-12) — <https://learntla.com/core/concurrency.html> — "await interacts a little oddly with variable updates"
[^pm-print]: PlusCal User's Manual (P-syntax), mục 3.2.8 Print — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "TLC may print the value even if the step containing the print statement is not executed because of an await statement that appears later in the step."
[^pm-assert]: PlusCal User's Manual (P-syntax), mục 3.2.9 Assert — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "TLC may report a failed assertion even if the step containing the assert statement is not executed because of an await statement that appears later in the step."
[^pm-define-start]: PlusCal User's Manual (P-syntax), mục 3.6 Definitions — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "evaluation of a defined symbol in the body of the algorithm uses the value that a variable appearing in the definition has at the beginning of the current step"
[^pm-lab-first]: PlusCal User's Manual (P-syntax), mục 3.7 Labels — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The first statement in the body of a procedure, of a process, or of a uniprocess algorithm must be labeled."
[^pm-lab-while]: PlusCal User's Manual (P-syntax), mục 3.2.4 While — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A while statement must be labeled. However, the statement following a while statement need not be labeled, even if there is a label in body."
[^pm-lab-after]: PlusCal User's Manual (P-syntax), mục 3.7 Labels — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A statement S in a statement sequence must be labeled if it is preceded in that sequence by any of the following:"
[^pm-lab-if]: PlusCal User's Manual (P-syntax), mục 3.7 Labels — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "An if or either statement that contains a labeled statement, a goto, a call, or a return anywhere within it."
[^pm-lab-macro]: PlusCal User's Manual (P-syntax), mục 3.7 Labels — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A macro body and the do clause of a with statement cannot contain any labeled statements."
[^pm-lab-assign]: PlusCal User's Manual (P-syntax), mục 3.7 Labels — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "In any control path, a label must come between an assignment to a variable x and any other statement that assigns a value to x ."
[^pm-with-nolabel]: PlusCal User's Manual (P-syntax), mục 3.2.6 With — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The body of a with statement may not contain a label."
[^pm-implicit]: PlusCal User's Manual (P-syntax), mục 3.7 Labels — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The implicit labels Done and Error cannot be used as actual labels."
[^pm-multiassign-ok]: PlusCal User's Manual (P-syntax), mục 3.2.1 Assignment — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "However, assignments to components of the same variable may appear in a single multiple assignment"
[^pm-multiassign]: PlusCal User's Manual (P-syntax), mục 3.2.1 Assignment — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A multiple assignment is executed by first evaluating the right-hand sides of all its assignments, and then performing those assignments from left to right."
[^pm-swap]: PlusCal User's Manual (P-syntax), mục 2.1 Typing the Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Assignments separated by \"||\"s (rather than by semicolons) form a single multiple assignment statement that is executed by first evaluating all the right-hand sides, then doing the assignments."
[^pm-callret-assign]: PlusCal User's Manual (P-syntax), mục 3.4 Procedures — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The call and return statements are considered to be assignments to the procedure's parameters and local variables."
[^tt-either-src]: tlaplus/tlaplus @ 94d0c50, test-model/pcal/Either1.tla — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/test-model/pcal/Either1.tla> — "a: either x := 1 ; b: x := x + 1; or y := 1 ; c: y := y + 1; end either ; d: assert x+y = 2 ;"
[^pm-missing-label]: PlusCal User's Manual (P-syntax), mục 4.1 Running the Translator — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "If you are mystified by this message, it may be because you've forgotten that call and return statements assign values to a procedure's parameters and local variables."
[^pm-fewest]: PlusCal User's Manual (P-syntax), mục 2.7 Where Labels Must and Can't Go — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "If the algorithm contains no labels, then the translator will add as few labels as possible, resulting in an algorithm with the fewest possible steps."
[^pm-label-default]: PlusCal User's Manual (P-syntax), mục 2.7 Where Labels Must and Can't Go — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The -label option is the default for a uniprocess algorithm if (and only if) you don't type any labels yourself."
[^pm-reportlabels]: PlusCal User's Manual (P-syntax), Phụ lục C Translator Options — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "-reportLabels Tells the translator to print the names and locations of all labels it adds. Like -label, it causes the translator to add missing labels."
[^pm-fast]: PlusCal User's Manual (P-syntax), mục 2.7 Where Labels Must and Can't Go — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Using the fewest steps makes model checking as fast as possible."
[^lt-labels-share]: Learn TLA+, Concurrency (trang sống, truy cập 2026-10-12) — <https://learntla.com/core/concurrency.html> — "Different processes cannot share label names."
[^pm-rename]: PlusCal User's Manual (P-syntax), mục 3.8 The Translation's Definitions and Declarations — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "if the PlusCal code declares a variable x and also uses x as a label, or if it declares x as a local variable in two different procedures, then one of the two x's must be renamed. If the translator renames identifiers, then it issues a warning and indicates, in comments placed right after the \"BEGIN TRANSLATION\" line, what renamings have been done."
[^pm-labelroot]: PlusCal User's Manual (P-syntax), Phụ lục C Translator Options — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The default prefix is Lbl ."
[^pm-proc-set]: PlusCal User's Manual (P-syntax), mục 3.3 Processes — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The first form begins a process set, the second an individual process."
[^pm-proc-self]: PlusCal User's Manual (P-syntax), mục 3.3 Processes — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Within the body of a process set, self equals the current process's identifier."
[^pm-proc-local]: PlusCal User's Manual (P-syntax), mục 2.6 A Multiprocess Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "declares j to be a local variable of these processes, meaning that each of the N processes has its own separate variable j."
[^pm-fair-proc]: PlusCal User's Manual (P-syntax), mục 4.6 Termination, Liveness, and Fairness — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Writing fair process instead of just process asserts that all actions of the process are, by default, weakly fair."
[^pm-unfair]: PlusCal User's Manual (P-syntax), mục 4.6 Termination, Liveness, and Fairness — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A process that is not a fair or fair+ process is called an unfair process and has no fairness assumptions on its actions."
[^pm-proc-ids]: PlusCal User's Manual (P-syntax), mục 3.3 Processes — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "identifiers of different processes in the same algorithm must all be different."
[^pm-proc-ids-type]: PlusCal User's Manual (P-syntax), mục 3.3 Processes — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "For example, the semantics of TLA+ does not specify whether or not a string may equal a number."
[^lt-types]: Learn TLA+, Concurrency (trang sống, truy cập 2026-10-12) — <https://learntla.com/core/concurrency.html> — "All processes must have comparable types: all integers, all strings, all sequences, etc."
[^pm-proc-name]: PlusCal User's Manual (P-syntax), mục 3.3 Processes — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The name ProcName has no significance; changing it does not change the meaning of the process statement in any way. The name appears in the TLA+ translation, and it should be different for different process statements"
[^pm-proc-self-proc]: PlusCal User's Manual (P-syntax), mục 3.4 Procedures — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "For a multiprocess algorithm, the identifier self in the body of a procedure equals the process identifier of the process within which the procedure is executing."
[^lt-goto-loop]: Learn TLA+, Concurrency (trang sống, truy cập 2026-10-12) — <https://learntla.com/core/concurrency.html> — "This is equivalent to putting the label in a while TRUE loop."
[^pm-localvar]: PlusCal User's Manual (P-syntax), mục 4.1 Running the Translator — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Variables local to a process are turned into functions (arrays) that take an additional argument."
[^lt-define-local]: Learn TLA+, Concurrency (trang sống, truy cập 2026-10-12) — <https://learntla.com/core/concurrency.html> — "Local variables cannot be referenced in other processes, or in a define."
[^pm-define-vars]: PlusCal User's Manual (P-syntax), mục 3.6 Definitions — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The variables that may appear within the define statement are the ones declared in the variable statement that immediately precedes it and that follows the algorithm name, as well as the variable pc and, if there is a procedure, the variable stack ."
[^pm-enabled]: PlusCal User's Manual (P-syntax), mục 4.6 Termination, Liveness, and Fairness — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "An action is enabled iff it can be executed."
[^pm-nonblock]: PlusCal User's Manual (P-syntax), mục 4.6 Termination, Liveness, and Fairness — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "An action like a that is enabled iff control is at that label is said to be non-blocking. An action like b that is not non-blocking is said to be blocking."
[^pm-fair-nonblock]: PlusCal User's Manual (P-syntax), mục 4.6 Termination, Liveness, and Fairness — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Fairness for a non-blocking action means that the process cannot stop at that action."
[^pm-wf-sf]: PlusCal User's Manual (P-syntax), mục 4.6 Termination, Liveness, and Fairness — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "For example, weak fairness of action b means that if x > self remains true while the process is at b, then action b is eventually executed."
[^pm-fairplus]: PlusCal User's Manual (P-syntax), mục 4.6 Termination, Liveness, and Fairness — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Writing fair+ process process assserts that all actions of the process are strongly fair by default."
[^pm-fair-label]: PlusCal User's Manual (P-syntax), mục 4.6 Termination, Liveness, and Fairness — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Writing a:+ asserts that action a is strongly fair. Writing a:- asserts that action a satisfies no fairness condition."
[^pm-fair-opts]: PlusCal User's Manual (P-syntax), mục 4.6 Termination, Liveness, and Fairness — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "-wf Makes any unfair process (one not preceded by fair or fair+) a fair process."
[^pm-fair-algorithm]: PlusCal User's Manual (P-syntax), mục 4.6 Termination, Liveness, and Fairness — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "This property asserts that the algorithm cannot halt if it has at least one enabled action."
[^ex-dp-fair-comment]: tlaplus/Examples @ 6a7b26a, specifications/DiningPhilosophers/DiningPhilosophers.tla — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/DiningPhilosophers/DiningPhilosophers.tla> — "If you remove the 'fair' in 'fair process', each process can stop at any time and will never run again."
[^ex-dp-nobody-starves]: tlaplus/Examples @ 6a7b26a, specifications/DiningPhilosophers/DiningPhilosophers.tla — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/DiningPhilosophers/DiningPhilosophers.tla> — "NobodyStarves == \A p \in 1..NP: []<>(~hungry[p])"
[^pm-fair-plus-effect]: PlusCal User's Manual (P-syntax), mục 4.6 Termination, Liveness, and Fairness — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Adding + after a label in a fair+ process has no effect."
[^pm-fastmutex-fair]: PlusCal User's Manual (P-syntax), mục 4.6 Termination, Liveness, and Fairness — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "We allow a process to remain forever inside its noncritical section or its critical section."
[^pm-termination-opt]: PlusCal User's Manual (P-syntax), Phụ lục C Translator Options — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "-termination If no fairness option is selected, it is equivalent to selecting the -wf option."
[^pm-liveness-slow]: PlusCal User's Manual (P-syntax), mục 4.6 Termination, Liveness, and Fairness — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Liveness checking (including termination) is slower than invariance checking, and TLC cannot check liveness on as large a model as it can check invariance."
[^ex-dp-spec]: tlaplus/Examples @ 6a7b26a, specifications/DiningPhilosophers/DiningPhilosophers.tla — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/DiningPhilosophers/DiningPhilosophers.tla> — "Spec == /\ Init /\ [][Next]_vars /\ \A self \in 1..NP : WF_vars(Philosopher(self))"
[^pm-await]: PlusCal User's Manual (P-syntax), mục 3.2.5 Await (When) — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A step containing the statement await expr can be executed only when the value of the Boolean expression expr is TRUE."
[^pm-when]: PlusCal User's Manual (P-syntax), mục 3.2.5 Await (When) — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The keyword when can be used instead of await."
[^pm-await-fail]: PlusCal User's Manual (P-syntax), mục 2.6 A Multiprocess Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "If exp equals false, then the attempted execution fails and nothing is changed; the processor will try to execute the step again later."
[^pm-either]: PlusCal User's Manual (P-syntax), mục 3.2.3 Either — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "It is executed by nondeterministically choosing any clause i that is executable and executing it."
[^pm-with]: PlusCal User's Manual (P-syntax), mục 3.2.6 With — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Execution is impossible if S is empty."
[^pm-either-enabled]: PlusCal User's Manual (P-syntax), mục 3.2.3 Either — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The either statement can be executed iff at least one of those clauses can be executed."
[^pm-if-either]: PlusCal User's Manual (P-syntax), mục 3.2.3 Either — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "is equivalent to the following, where the await statement is explained in Section 3.2.5 on the next page."
[^pm-with-eq]: PlusCal User's Manual (P-syntax), mục 3.2.6 With — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "(The expression {expr } equals the set containing a single element equal to expr .)"
[^pm-with-consequence]: PlusCal User's Manual (P-syntax), mục 3.2.6 With — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "implies that the following may not appear within the body of a with statement."
[^pm-deadlock]: PlusCal User's Manual (P-syntax), mục 4.7.1 Deadlock Checking — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A multiprocess algorithm is deadlocked if no process can take a step, but some process has not terminated."
[^pm-deadlock-normal]: PlusCal User's Manual (P-syntax), mục 4.7.1 Deadlock Checking — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "However, sometimes an algorithm is supposed to halt in a state in which not all processes have reached the end of their code."
[^lt-deadlock]: Learn TLA+, Concurrency (trang sống, truy cập 2026-10-12) — <https://learntla.com/core/concurrency.html> — "Once every writer has finished running, the reader is stuck at ReadFromQueue forever"
[^pm-with-multi]: PlusCal User's Manual (P-syntax), mục 3.2.6 With — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "where each ? may be either = or ∈ . (Commas may be used instead of semicolons between the id i ? expr i items.) This statement is equivalent to"
[^tt-either-trans]: tlaplus/tlaplus @ 94d0c50, test-model/pcal/Either1.tla — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/test-model/pcal/Either1.tla> — "a == /\ pc = "a" /\ \/ /\ x' = 1 /\ pc' = "b" /\ y' = y \/ /\ y' = 1 /\ pc' = "c" /\ x' = x"
[^pm-bfs]: PlusCal User's Manual (P-syntax), mục 2.3 Translating and Executing the Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "If you want sensible output from running TLC in model-checking mode, you should have the algorithm execute only a single print statement at the end."
[^pm-bfs-n4]: PlusCal User's Manual (P-syntax), mục 2.3 Translating and Executing the Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "let's change the model to have it set N to 4, so there are only 4 possible executions of the algorithm."
[^pm-bfs-search]: PlusCal User's Manual (P-syntax), mục 2.3 Translating and Executing the Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Instead, TLC found all reachable states using a breadthfirst search."
[^pm-assert-check]: PlusCal User's Manual (P-syntax), mục 2.4 Checking the Results — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "We don't have to print the results and examine them by hand to check them."
[^pm-macro]: PlusCal User's Manual (P-syntax), mục 3.5 Macros — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A macro is like a procedure, except that a call of a macro is expanded at translation time. You can think of a macro as a procedure that is executed within the step from which it is called."
[^pm-macro-body]: PlusCal User's Manual (P-syntax), mục 3.5 Macros — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The difference is that the body of the macro may contain no labels, no while, call, return, or goto statement."
[^pm-macro-pos]: PlusCal User's Manual (P-syntax), mục 3.5 Macros — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "It may contain a call of a previously defined macro. Macro definitions come right after any global variable declarations and define section."
[^pm-c-varname]: PlusCal User's Manual (C-syntax), Phụ lục A The Grammar — <https://lamport.azurewebsites.net/tla/c-manual.pdf> — "hVariable i ::= A TLA+ identifier that is not a PlusCal reserved word and is not pc, stack , or self ."
[^tt-fact-else]: tlaplus/tlaplus @ 94d0c50, test-model/pcal/Factorial.tla — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/test-model/pcal/Factorial.tla> — "ELSE /\ result' = result * arg1 /\ arg1' = arg1 - 1 /\ u' = 1 /\ pc' = "p1" /\ stack' = stack"
[^pm-pc-uni]: PlusCal User's Manual (P-syntax), mục 2.6 A Multiprocess Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "For a uniprocess algorithm, the value of pc is a single string equal to the label of the next statement to be executed."
[^pm-spec-default]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "If no liveness or termination option is specified, the definition of Spec is"
[^ex-peterson-a0]: tlaplus/Examples @ 6a7b26a, specifications/locks_auxiliary_vars/Peterson.tla — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/locks_auxiliary_vars/Peterson.tla> — "a0(self) == /\ pc[self] = "a0" /\ TRUE /\ pc' = [pc EXCEPT ![self] = "a1"] /\ UNCHANGED << c, turn >>"
[^pm-simplified]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "I have simplified the translation a bit to make it easier to read, but I have not altered its meaning."
[^rust-safe-race]: Rust Nomicon (1.95.0), Data Races and Race Conditions — <https://doc.rust-lang.org/1.95.0/nomicon/races.html> — "Safe Rust guarantees an absence of data races, which are defined as:"
[^pm-stack-add]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The variable pc is added to describe the control state. If an algorithm contains one or more procedures, a variable stack is added to hold the calling stack. In addition, each formal parameter and local variable of a procedure is declared to be a variable."
[^pm-stack-model]: PlusCal User's Manual (P-syntax), Phụ lục B.2 Procedures — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Their translation models a standard implementation using a call stack that is represented by the variable stack ."
[^pm-proc-noreturn]: PlusCal User's Manual (P-syntax), mục 3.4 Procedures — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A PlusCal procedure does not explicitly return a value."
[^pm-macro-expand]: PlusCal User's Manual (P-syntax), mục 3.5 Macros — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The translation replaces the macro call with the sequence of statements obtained from the body of the macro definition by substituting the arguments of the call"
[^lt-macro-self]: Learn TLA+, Concurrency (trang sống, truy cập 2026-10-12) — <https://learntla.com/core/concurrency.html> — "Macros can use the value of self inside of them."
[^pm-macro-syntactic]: PlusCal User's Manual (P-syntax), mục 3.5 Macros — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "substitution is syntactic in the sense that the meaning of any symbol in the macro definition other than a parameter is the meaning it has in the context of the call."
[^ex-lock-macro]: tlaplus/Examples @ 6a7b26a, specifications/locks_auxiliary_vars/Lock.tla — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/locks_auxiliary_vars/Lock.tla> — "macro Lock(l){ await l = 1; l := 0; } macro Unlock(l){ l := 1; }"
[^ex-lock-l1]: tlaplus/Examples @ 6a7b26a, specifications/locks_auxiliary_vars/Lock.tla — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/locks_auxiliary_vars/Lock.tla> — "l1(self) == /\ pc[self] = "l1" /\ lock = 1 /\ lock' = 0 /\ pc' = [pc EXCEPT ![self] = "cs"]"
[^ex-lock-inv]: tlaplus/Examples @ 6a7b26a, specifications/locks_auxiliary_vars/Lock.tla — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/locks_auxiliary_vars/Lock.tla> — "lockcs(i) == pc[i] \in {"cs", "l2"}"
[^ex-lock-inv2]: tlaplus/Examples @ 6a7b26a, specifications/locks_auxiliary_vars/Lock.tla — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/locks_auxiliary_vars/Lock.tla> — "/\ \A i, j \in ProcSet: (i # j) => ~(lockcs(i) /\ lockcs(j))"
[^pm-proc-pos]: PlusCal User's Manual (P-syntax), mục 3.4 Procedures — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The algorithm's procedures follow its global variable declarations and define section (if any) and precede the begin of a uniprocess algorithm or the first process of a multiprocess algorithm."
[^lt-proc-order]: Learn TLA+, Concurrency (trang sống, truy cập 2026-10-12) — <https://learntla.com/core/concurrency.html> — "Procedures must be defined after any macros and before any processes."
[^pm-proc-params]: PlusCal User's Manual (P-syntax), mục 3.4 Procedures — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The procedure's local variables are initialized on each entry to the procedure."
[^tt-fact-src1]: tlaplus/tlaplus @ 94d0c50, test-model/pcal/Factorial.tla — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/test-model/pcal/Factorial.tla> — "variable u = 1 ; begin p1 : if arg1 = 0 then return;"
[^tt-fact-src2]: tlaplus/tlaplus @ 94d0c50, test-model/pcal/Factorial.tla — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/test-model/pcal/Factorial.tla> — "else result := result * arg1; call FactProc ( arg1 - 1 ) ; return; end if; end procedure"
[^pm-tailcall]: PlusCal User's Manual (P-syntax), mục 3.4 Procedures — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "When a call P statement is followed immediately by a return, the return from procedure P and the return performed by the return statement are both executed as part of a single execution step."
[^tt-fact-call]: tlaplus/tlaplus @ 94d0c50, test-model/pcal/Factorial.tla — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/test-model/pcal/Factorial.tla> — "a1 == /\ pc = "a1" /\ /\ arg1' = 5 /\ stack' = << [ procedure |-> "FactProc", pc |-> "a2", u |-> u, arg1 |-> arg1 ] >> \o stack /\ u' = 1 /\ pc' = "p1" /\ UNCHANGED result"
[^tt-fact-ret]: tlaplus/tlaplus @ 94d0c50, test-model/pcal/Factorial.tla — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/test-model/pcal/Factorial.tla> — "IF arg1 = 0 THEN /\ pc' = Head(stack).pc /\ u' = Head(stack).u /\ arg1' = Head(stack).arg1 /\ stack' = Tail(stack) /\ UNCHANGED result"
[^pm-proc-return]: PlusCal User's Manual (P-syntax), mục 3.4 Procedures — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Executing this call assigns the current values of the expressions expr i to the corresponding parameters param i , initializes the procedure's local variables, and puts control at the beginning of the procedure body. A return statement assigns to the parameters and local procedure variables their previous values—that is, the values they had before the procedure was last called—and returns control to the point immediately following the call statement."
[^pm-stack-rec]: PlusCal User's Manual (P-syntax), Phụ lục B.2 Procedures — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The value is a sequence of three records, one for each procedure being executed."
[^pm-stack-innermost]: PlusCal User's Manual (P-syntax), Phụ lục B.2 Procedures — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The innermost procedure execution produced the first of these records."
[^pm-stack-restore]: PlusCal User's Manual (P-syntax), Phụ lục B.2 Procedures — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The corresponding variables will be restored to those values upon the next return from procedure Q"
[^pm-proc-error]: PlusCal User's Manual (P-syntax), mục 3.4 Procedures — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "There is an implicit label Error immediately after the body."
[^pm-deadlock-error]: PlusCal User's Manual (P-syntax), mục 4.7.1 Deadlock Checking — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The most likely way for a uniprocess algorithm to deadlock is for a procedure call to fall off the end without executing a return statement"
[^pm-proc-rval]: PlusCal User's Manual (P-syntax), mục 3.4 Procedures — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A value can be returned by having the procedure set a global variable and having the code immediately following the call read that variable."
[^pm-call-label]: PlusCal User's Manual (P-syntax), mục 3.4 Procedures — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A call statement must either be followed in the control path by a label or else it must appear immediately before a return statement in a statement sequence."
[^pm-macro-boundvar]: PlusCal User's Manual (P-syntax), mục 3.5 Macros — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The substitution of an expression like y + 17 for id here will cause a mysterious error when the translation is parsed."
[^pm-define-notexpanded]: PlusCal User's Manual (P-syntax), mục 3.6 Definitions — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Definitions, including ones in a define statement, are not expanded in the PlusCal to TLA+ translation."
[^ex-dp-define]: tlaplus/Examples @ 6a7b26a, specifications/DiningPhilosophers/DiningPhilosophers.tla — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/DiningPhilosophers/DiningPhilosophers.tla> — "VARIABLES forks, pc (* define statement *) LeftFork(p) == p"
[^ex-simple-src]: tlaplus/Examples @ 6a7b26a, specifications/TeachingConcurrency/Simple.tla — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/TeachingConcurrency/Simple.tla> — "process (proc \in 0..N-1) { a: x[self] := 1 ; b: y[self] := x[(self-1) % N] }"
[^ex-simple-init]: tlaplus/Examples @ 6a7b26a, specifications/TeachingConcurrency/Simple.tla — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/TeachingConcurrency/Simple.tla> — "Init == (* Global variables *) /\ x = [i \in 0..(N-1) |-> 0] /\ y = [i \in 0..(N-1) |-> 0] /\ pc = [self \in ProcSet |-> "a"]"
[^ex-simple-a]: tlaplus/Examples @ 6a7b26a, specifications/TeachingConcurrency/Simple.tla — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/TeachingConcurrency/Simple.tla> — "a(self) == /\ pc[self] = "a" /\ x' = [x EXCEPT ![self] = 1] /\ pc' = [pc EXCEPT ![self] = "b"] /\ y' = y"
[^ex-simple-next]: tlaplus/Examples @ 6a7b26a, specifications/TeachingConcurrency/Simple.tla — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/TeachingConcurrency/Simple.tla> — "Terminating == /\ \A self \in ProcSet: pc[self] = "Done" /\ UNCHANGED vars Next == (\E self \in 0..N-1: proc(self)) \/ Terminating Spec == Init /\ [][Next]_vars"
[^pm-trans-pc]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The variable pc is added to describe the control state. If an algorithm contains one or more procedures, a variable stack is added to hold the calling stack."
[^pm-trans-init]: PlusCal User's Manual (P-syntax), mục 3.8 The Translation's Definitions and Declarations — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The initial predicate Init. It contains a conjunct for each variable. The conjuncts for global variables precede those for local procedure and process variables. The conjuncts for the variables declared in a single variable statement appear in the order in which they are declared."
[^pm-init-fn]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Observe that the process-local variables and the variable pc are made functions with domain equal to the appropriate set of process identifiers."
[^pm-pc-fn]: PlusCal User's Manual (P-syntax), mục 2.6 A Multiprocess Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "For any multiprocess algorithm, the value of the variable pc is a function whose domain is the set of process identifiers."
[^pm-action-per-label]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The translation defines an action for each atomic operation of the algorithm."
[^pm-action-lab]: PlusCal User's Manual (P-syntax), mục 3.8 The Translation's Definitions and Declarations — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "This action is the TLA+ representation of the atomic operation beginning at that label."
[^pm-enable]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "is an enabling condition, meaning that the action can be executed only when it is true."
[^pm-await-enable]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The expression y = 0 of the await statement is an enabling condition of action l 4."
[^pm-except]: PlusCal User's Manual (P-syntax), mục 4.1 Running the Translator — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "An assignment to an element of a function or record variable is rewritten as an assignment to the variable using the TLA+ except construct"
[^pm-unchanged]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "the unchanged conjunct asserts that the values of all other variables are not changed."
[^pm-proc-next]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A step of the process is one that satisfies formula Proc(self )."
[^pm-next]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The action Next is defined to be the next-state action of the entire algorithm. It is the disjunction of the next-state actions of all the processes."
[^pm-done-disj]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "TLC has no way of distinguishing deadlock from termination, which is simply a desired form of deadlock."
[^pm-done-loop]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The translation therefore adds a disjunction to Next that allows a terminated algorithm to perform a step that does nothing."
[^pm-nodone]: PlusCal User's Manual (P-syntax), Phụ lục C Translator Options — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "-noDoneDisjunct Suppress the extra disjunct that the translation normally adds to the next-state action Next to prevent TLC from thinking that the algorithm has deadlocked when it terminates."
[^pm-spec-fair]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Had we used a fair process statement or the -wf option to specify weak fairness of the processes"
[^pm-termination]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Finally, the translation defines the temporal formula Termination that asserts the property that the algorithm eventually terminates:"
[^pm-termination-toolbox]: PlusCal User's Manual (P-syntax), mục 2.5 Checking Termination — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "If we then create a new model, it will add Termination to the Properties part of the What to check? section of the Model Overview page."
[^pm-pc-label]: PlusCal User's Manual (P-syntax), mục 2.6 A Multiprocess Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The TLA+ translation introduces a new variable pc whose value is the label of the next statement to be executed."
[^pm-mutex-def]: PlusCal User's Manual (P-syntax), mục 2.6 A Multiprocess Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The safety property that algorithm FastMutex should satisfy is mutual exclusion, meaning that at most one process can be in its critical section at any one time."
[^pm-defs-after]: PlusCal User's Manual (P-syntax), mục 5.11 TLA+ Definitions — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "definitions that use the identifiers declared or defined in the translation must come after the END TRANSLATION line."
[^pm-define-twovars]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "the translation would have contained two variables statements, the first declaring the variables x , y, b, and pc, and the second declaring the remaining variable j ."
[^pm-defaultinit-decl]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The declaration of defaultInitValue is omitted if every variable is explicitly initialized."
[^pm-spec-minus]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Had we added - after the labels ncs: and cs:, this conjunct would have become"
[^pm-spec-sf]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Had we specified strong fairness of the process, either with a fair+ process statement or the -sf translator option, then the translation would have been the same except with WF replaced everywhere by SF"
[^pm-spec-proc]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "For each procedure P called by the process, the translation would have defined action P (self ) to be the disjunction of the atomic actions in the body of the procedure"
[^pm-if-trans]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Action l 2 performs the if statement's test and the subsequent transfer of control."
[^pm-while-trans]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Action l 8 shows how a while statement whose test is not identically true is translated."
[^pm-goto-trans]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Actions l 10 and l 12 show the translation of an explicit goto and the transfer of control at the end of the while loop."
[^pm-goto-pcal]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "To help you understand the translation of your own algorithm, you can try executing the Toolbox's Goto PCal Source command on any region of the translation."
[^pm-goto-jump]: PlusCal User's Manual (P-syntax), mục 4.1 Running the Translator — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Executing the Toolbox's Goto PCal Source command jumps to the corresponding region of PlusCal."
[^pm-translation-copies]: PlusCal User's Manual (P-syntax), mục 4.1 Running the Translator — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "the translation copies your expressions pretty much the way you typed them, except for the following changes."
[^ex-peterson-await]: tlaplus/Examples @ 6a7b26a, specifications/locks_auxiliary_vars/Peterson.tla — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/locks_auxiliary_vars/Peterson.tla> — "a3(self) == /\ pc[self] = "a3" /\ ~c[Other(self)] \/ turn = self /\ pc' = [pc EXCEPT ![self] = "cs"] /\ UNCHANGED << c, turn >>"
[^lt-threads-intro]: Learn TLA+, Concurrency (trang sống, truy cập 2026-10-12) — <https://learntla.com/core/concurrency.html> — "We have two threads incrementing a single counter. At first, we'll have them do this atomically, and show that we get the expected value."
[^lt-threads-fail]: Learn TLA+, Concurrency (trang sống, truy cập 2026-10-12) — <https://learntla.com/core/concurrency.html> — "Both threads read the value of counter when it's 0, meaning they both set tmp to 0, meaning they both assign counter := 0 + 1."
[^lt-typeinv-placement]: Learn TLA+, Concurrency (trang sống, truy cập 2026-10-12) — <https://learntla.com/core/concurrency.html> — "Look for the \* END TRANSLATION line and put it after that (and before the ====)."
[^lt-threads-pass]: Learn TLA+, Concurrency (trang sống, truy cập 2026-10-12) — <https://learntla.com/core/concurrency.html> — "Now the spec passes again."
[^lt-assert-release]: Learn TLA+, Concurrency (trang sống, truy cập 2026-10-12) — <https://learntla.com/core/concurrency.html> — "I'd also add a quick error-check in the form of an assert, just to make sure that a thread actually has the lock before releasing it."
[^lt-alldone-weak]: Learn TLA+, Concurrency (trang sống, truy cập 2026-10-12) — <https://learntla.com/core/concurrency.html> — "AllDone only says that if the processes finish, we have the correct result."
[^pm-c-grammar-define]: PlusCal User's Manual (C-syntax), Phụ lục A The Grammar — <https://lamport.azurewebsites.net/tla/c-manual.pdf> — "hProcess i ::= [ fair [+]0,1 ]0,1 process ( hName i [= | \in] hExpr i ) hVarDecls i0,1 hCompoundStmt i [;]0,1"
[^pm-c-grammar-defs]: PlusCal User's Manual (C-syntax), Phụ lục A The Grammar — <https://lamport.azurewebsites.net/tla/c-manual.pdf> — "hDefinitions i ::= define { hDefs i } [;]0,1"
[^pm-symmetry]: PlusCal User's Manual (P-syntax), mục 4.7.3 Symmetry — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "TLC can use symmetry only for a set of model values. For TLC to take advantage of the symmetry of algorithm FastMutex , that algorithm would have to be rewritten to use an arbitrary set of process identifiers instead of the set of numbers 1 . . N ."
[^pm-symmetry-liveness]: PlusCal User's Manual (P-syntax), mục 4.7.3 Symmetry — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "Do not tell TLC both to assume symmetry and to check liveness."
[^rust-fetch-add]: Rust 1.95.0, std::sync::atomic::AtomicUsize::fetch_add — <https://doc.rust-lang.org/1.95.0/std/sync/atomic/struct.AtomicUsize.html> — "Adds to the current value, returning the previous value."
[^rust-race]: Rust Nomicon (1.95.0), Data Races and Race Conditions — <https://doc.rust-lang.org/1.95.0/nomicon/races.html> — "However Rust does not prevent general race conditions."
[^rust-mutex-lock]: Rust 1.95.0, std::sync::Mutex::lock — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html> — "Acquires a mutex, blocking the current thread until it is able to do so."
[^rust-mutex-drop]: Rust 1.95.0, std::sync::Mutex::lock — <https://doc.rust-lang.org/1.95.0/std/sync/struct.Mutex.html> — "When the guard goes out of scope, the mutex will be unlocked."
[^rust-poll]: Rust 1.95.0, std::future::Future — <https://doc.rust-lang.org/1.95.0/std/future/trait.Future.html> — "attempts to resolve the future into a final value. This method does not block if the value is not ready."
