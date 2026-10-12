# P13 · Chương 02 — Toán nền tảng: logic, tập hợp, hàm, record, sequence

> Mọi khẳng định về cú pháp và ngữ nghĩa là về **TLA+** theo sách *Specifying Systems* của Lamport (bản PDF miễn phí trên trang tác giả), bản tóm tắt cú pháp của Lamport, và mã nguồn các module chuẩn trong repo `tlaplus/tlaplus` (ghim commit `94d0c5070852b37b801bb8998c436383ab7ee37b`). Các khối ```` ```tla ```` là **minh họa cú pháp, chưa chạy qua SANY hay TLC**; giá trị ghi trong chú thích `\*` là kết quả *theo định nghĩa toán học*, không phải output đã chạy. Trang Learn TLA+ (learntla.com) là **trang sống**, nội dung có thể đổi sau ngày soạn (2026-10-12). Rust theo docs 1.95.0.

## Bạn sẽ hiểu được gì

- **Q1.** Các toán tử `/\`, `\/`, `~`, `=>`, `<=>`, `\A`, `\E` nghĩa là gì, và danh sách `/\`/`\/` xếp thẳng hàng (bullet) hoạt động theo luật nào? → mục 1
- **Q2.** Tập hợp có những toán tử và cách dựng nào: `\in`, `\cup`, `\cap`, `\subseteq`, `SUBSET`, `UNION`, lọc `{x \in S : P}`, ánh xạ `{e : x \in S}`? → mục 2
- **Q3.** Hàm `[x \in S |-> e]`, `DOMAIN`, `[S -> T]`, `EXCEPT` là gì, và hàm khác toán tử ở đâu? → mục 3
- **Q4.** Vì sao record, tuple, sequence thực chất đều là hàm; dùng `Seq`, `Len`, `Head`, `Tail`, `Append` thế nào? → mục 4
- **Q5.** `CHOOSE` làm gì, và vì sao nó không phải phép chọn ngẫu nhiên? → mục 5
- **Q6.** Định nghĩa `==`, tham số, `LET`/`IN`, `IF`, `CASE` hoạt động ra sao (kể cả phạm vi tên)? → mục 6
- **Q7.** Vì sao phải viết `EXTENDS Naturals`/`Integers`/`Sequences`, và số trong TLA+ khác số nguyên của máy ở đâu? → mục 7
- **Q8.** TLA+ không có kiểu thì "biểu thức ngớ ngẩn" là gì, và TLC đánh giá biểu thức theo luật nào? → mục 8

## Kiến thức cần trước

- [Chương 01 — Vì sao cần đặc tả hình thức](01-vi-sao-dac-ta-hinh-thuc.md): đặc tả là gì, vì sao một kỹ sư backend có thể cần nó. Chương này chỉ dạy phần *toán*; cách mô tả trạng thái thay đổi (biến, `Init`, `Next`, ký hiệu `'`) để dành cho [Chương 03](03-trang-thai-hanh-vi-action.md).
- Không cần biết trước logic hay lý thuyết tập hợp, và cũng không cần nhớ toán phổ thông. Bạn chỉ cần đọc được code Rust; mục 10 đặt từng khái niệm của chương cạnh thứ tương ứng trong Rust.

Vì sao đáng bỏ công học phần toán này? Vì nó không nặng như tên gọi. Đội Amazon Web Services mô tả TLA+ là dựa trên toán rời rạc đơn giản, gồm lý thuyết tập hợp cơ bản và vị từ[^aws-math]. Lamport cũng nói ý tưởng nền của TLA+ là cách tốt nhất để mô tả sự vật chính xác là dùng toán đơn giản[^tla-home].

## 1. Logic: `/\`, `\/`, `~`, `=>`, `<=>`, `\A`, `\E` và danh sách bullet (Q1)

> **Trả lời ngắn.** TLA+ dùng đúng năm toán tử của logic mệnh đề: và, hoặc, phủ định, kéo theo, tương đương[^bk-prop-ops]. Trên một tập, ta có thêm lượng từ "với mọi" `\A`, nói một điều đúng với mọi phần tử[^bk-forall]. Lượng từ "tồn tại" `\E` thì nói điều đó đúng với ít nhất một phần tử[^bk-exists]. Toán tử dễ nhầm nhất là `=>`: `F => G` chỉ sai khi `F` đúng mà `G` sai[^bk-imp-table]. Muốn nối nhiều điều kiện mà khỏi dùng ngoặc, ta xếp chúng thành danh sách `/\` hoặc `\/` thẳng cột; mỗi mục kết thúc ở đâu là do *cột* của ký tự `/\` hay `\/` quyết định[^bk-align-1].

**Giải thích.**

*Với service `lease-svc`.* Hãy hình dung `lease-svc`, một service cấp *lease* (quyền giữ một resource key, ví dụ một shard, trong một khoảng thời gian) cho các node worker, để mỗi key chỉ có một worker xử lý tại một thời điểm. Cluster có vài node, mỗi node đang up hoặc down và có một node là leader. Client gửi lên hai loại request: đọc xem ai đang giữ lease, hoặc xin lease. Ta sẽ bám theo `lease-svc` suốt chương (nhận xét của người soạn: đây là ví dụ giả định, không phải code trong repo). Ngay từ đầu nó đã đặt ra hai câu hỏi cần logic chính xác. Một: node này có được phục vụ request này không? Điều kiện sẽ có dạng "node đang up, *và* request là đọc *hoặc* node là leader". Hai: hệ thống phải luôn đúng điều gì? Chẳng hạn "*nếu* key đã có chủ *thì* epoch của lease phải lớn hơn 0". Câu đầu dùng `/\` và `\/`, câu sau dùng `=>`. Mục này dạy cách viết cả hai thành công thức mà máy kiểm tra được.

### 1.1 Năm toán tử và hai lượng từ

Ba toán tử đầu tương ứng `&&`, `||`, `!` của Rust, chỉ khác cách gõ: "và" viết `/\`, "hoặc" viết `\/`, phủ định viết `~`. Hai toán tử còn lại không có bản sao trực tiếp trong Rust: kéo theo viết `=>`, tương đương viết `<=>`. Trong file `.tla` ta gõ ASCII. `/\` còn viết được là `\land`, và `~` còn viết là `\lnot` hoặc `\neg`[^sum-ascii-and]. `\/` còn viết là `\lor`, và `<=>` còn viết là `\equiv`[^sum-ascii-or]. Hai giá trị Boolean viết là `TRUE` và `FALSE`. Vì sao viết hoa? Trong sách của Lamport, các từ khóa in bằng chữ hoa nhỏ (small caps), và khi gõ vào file thì chúng thành chữ HOA thông thường[^bk-ascii-kw]. Quy tắc đó cho ra `TRUE`, `FALSE`[^learn-bool] và tên `BOOLEAN` cho tập `{TRUE, FALSE}` (nhận xét của người soạn: tên `boolean` trong sách in thành `BOOLEAN` theo đúng quy tắc từ khóa đó).

| Toán học | Gõ ASCII | Đọc |
|---|---|---|
| `∧` | `/\` hoặc `\land` | và |
| `∨` | `\/` hoặc `\lor` | hoặc (cả hai cùng đúng cũng được) |
| `¬` | `~` hoặc `\lnot`, `\neg` | phủ định |
| `⇒` | `=>` | kéo theo |
| `≡` | `<=>` hoặc `\equiv` | tương đương |
| `∀ x ∈ S : P` | `\A x \in S : P` | với mọi `x` thuộc `S`, `P` |
| `∃ x ∈ S : P` | `\E x \in S : P` | tồn tại `x` thuộc `S` sao cho `P` |

Cách gõ `\A`, `\E`, `\in` xuất hiện ngay trong định nghĩa `IsFiniteSet` của module chuẩn `FiniteSets`[^repo-isfinite].

### 1.2 Phép kéo theo: không phải `if`

`=>` là toán tử đáng dừng lâu nhất, vì nó trông giống `if` nhưng không phải. `F => G` bằng `~F \/ G`; sách chứng minh điều này bằng bảng chân trị, cho thấy `(F ⇒ G) ≡ (¬F ∨ G)` là hằng đúng[^bk-imp-or]. Bảng chân trị của `=>` như sau.

| `F` | `G` | `F => G` |
|---|---|---|
| `TRUE` | `TRUE` | `TRUE` |
| `TRUE` | `FALSE` | `FALSE` |
| `FALSE` | `TRUE` | `TRUE` |
| `FALSE` | `FALSE` | `TRUE` |

Hai dòng cuối, vế trái sai thì cả câu đúng, hay làm người mới bối rối. Lamport giải thích bằng một ví dụ số: ta kỳ vọng nếu `n` lớn hơn 3 thì `n` lớn hơn 1, tức `n > 3` phải kéo theo `n > 1`; thay `n` bằng 4, 2, 0 sẽ ra lần lượt các dòng 1, 3, 4 của bảng[^bk-imp-why].

Quay lại `lease-svc`. Invariant "key đã có chủ thì epoch lớn hơn 0" có dạng `đã_có_chủ => epoch > 0`. Với key chưa có chủ, vế trái sai nên cả invariant đúng, bất kể epoch là bao nhiêu; đó chính là hai dòng cuối của bảng, và đó cũng là điều ta muốn: invariant không đòi gì ở key chưa ai giữ. Vì thế `=>` không phải `if-then` của lập trình: nó không có nhánh `else` và không chạy gì cả. Nó là một *phát biểu* về quan hệ giữa hai mệnh đề, và đó là lý do hầu hết invariant có dạng `điều_kiện => hệ_quả` (nhận xét của người soạn).

`<=>` là phép bằng cho giá trị Boolean. Ta thay được `≡` bằng `=`, nhưng không làm ngược lại được[^bk-equiv].

### 1.3 Thứ tự ưu tiên và lượng từ

Về độ ưu tiên, `¬` ràng chặt hơn `∧` và `∨`, hai toán tử này ràng chặt hơn `⇒` và `≡`. Vì vậy `¬F ∧ G ⇒ H` nghĩa là `((¬F) ∧ G) ⇒ H`[^bk-prec]. Dù hiểu đúng, bạn vẫn nên thêm ngoặc, và sách cũng khuyên thế: nếu có chút nghi ngờ về việc cần ngoặc hay không, cứ viết[^bk-parens]. Riêng chuyện trộn `/\` với `\/` trong một biểu thức thì đừng bao giờ để trần, hãy ngoặc hoặc xếp bullet (nhận xét của người soạn).

`\A x \in S : P` khẳng định `P` đúng với mọi phần tử `x` của `S`[^bk-forall]. `\E x \in S : P` khẳng định `P` đúng với ít nhất một phần tử[^bk-exists]. Trong `lease-svc`, `\E n \in Node : n.role = "leader"` nghĩa là cluster có ít nhất một leader, còn `\A n \in Node : n.up` nghĩa là mọi node đều đang up. Hai lượng từ này đối ngẫu: `P` đúng với một `x` nào đó khi và chỉ khi không phải "`P` sai với mọi `x`"[^bk-dual]. Hệ quả đáng nhớ nằm ở tập rỗng. Trên `{}`, `\E` luôn sai vì không có phần tử nào để thỏa[^bk-empty], nên theo đối ngẫu `\A` luôn đúng, kiểu "đúng rỗng". Với `lease-svc`, nếu chưa có node nào up thì `\A n \in UpNodes : ...` đúng một cách tầm thường (nhận xét của người soạn). Để nhớ, bạn cứ coi `\A` là AND và `\E` là OR chạy qua từng phần tử[^bk-conj-disj] (nhận xét của người soạn: gần nhất với `all` và `any` của iterator Rust, xem mục 10).

Có thể gom nhiều biến vào một lượng từ: `\A x \in S, y \in T : F` nghĩa là `\A x \in S : (\A y \in T : F)`[^bk-quant-nest]. Khi viết đặc tả, sách khuyên dùng lượng từ *có chặn* (bounded, tức `\A x \in S`) thay cho lượng từ không chặn (`\A x : ...`) bất cứ khi nào có thể[^bk-bounded]. Dạng có chặn dễ đọc hơn, cho cả người lẫn công cụ.

Vài ký hiệu trong các khối mã dưới đây sẽ học ở chương này: `Tên == biểu_thức` định nghĩa một tên (mục 6), `EXTENDS Naturals` nạp module số học (mục 7), và `\*` mở một comment đến hết dòng (mục 6, phần Đi sâu).

```tla
EXTENDS Naturals

\* (n > 3) kéo theo (n > 1): đúng với mọi n trong khoảng này (theo định nghĩa của =>)
Safe == \A n \in 0..10 : (n > 3) => (n > 1)   \* 0..10 là tập {0, 1, ..., 10}, xem mục 7

AllPositive(S) == \A n \in S : n > 0
HasEven(S)     == \E n \in S : n % 2 = 0       \* % là phép chia dư, xem mục 7
HasTwoDistinct(S) == \E x, y \in S : x # y     \* # đọc là "khác" (≠), xem bảng ở mục 2

\* Bullet list: mỗi dòng bắt đầu bằng /\ hoặc \/ ở cùng một cột
CanServe(req, node) ==
    /\ node.up
    /\ \/ req.kind = "read"
       \/ node.role = "leader"
    /\ ~ node.draining
\* nghĩa là: node.up /\ (req.kind = "read" \/ node.role = "leader") /\ ~node.draining
```

### 1.4 Danh sách bullet

`CanServe` ở trên chính là câu hỏi đầu tiên của `lease-svc`: node phải đang up, request phải là đọc hoặc node phải là leader, và node không được đang draining. Viết bằng bullet, mỗi `/\` mở một conjunct (một vế của phép "và"), và conjunct đó phải nằm hoàn toàn bên phải dấu `/\` của nó[^bk-bullet]. Các `/\` và `\/` phải thẳng cột tuyệt đối, kể cả khi gõ ASCII[^bk-bullet-align]. Nhờ vậy thụt lề mang nghĩa, ta khỏi cần ngoặc, và cách viết này đặc biệt tiện khi `/\` với `\/` lồng nhau[^bk-bullet-nest]. Đây là chỗ duy nhất thụt lề có nghĩa cú pháp; ở mọi nơi khác nó bị bỏ qua[^bk-indent-ignored].

Ví dụ sau cho thấy cột của `/\` quyết định conjunct nào chứa `D`:

```tla
Inner ==
    /\ A
    /\ \/ B
       \/ /\ C
          /\ D        \* D thuộc nhánh (C /\ D):  A /\ (B \/ (C /\ D))
Outer ==
    /\ A
    /\ \/ B
       \/ /\ C
    /\ D              \* D ở cột ngoài cùng:      A /\ (B \/ C) /\ D
```

Ở `Outer`, dòng `/\ D` thẳng cột với `/\ A`, nên nó kết thúc luôn danh sách lồng bên trong và mở một conjunct mới của danh sách ngoài (luật kết thúc conjunct nằm ở phần Đi sâu). Learn TLA+ có ví dụ tương tự và nhấn mạnh đây là nơi duy nhất trong ngôn ngữ mà khoảng trắng có nghĩa[^learn-bullet].

<details><summary>Đi sâu: luật căn lề chính xác, lượng từ "ăn" tới cùng, và <code>=</code> với <code>&lt;=&gt;</code></summary>

- **Luật kết thúc một conjunct** (sách, mục 15.2.2). Gọi `c` là cột của ký tự `/` trong `/\` mở danh sách. Một conjunct kết thúc khi gặp một trong bốn thứ. (1) Một `/\` khác có ký tự `/` ở đúng cột `c` và là ký tự khác khoảng trắng đầu tiên của dòng[^bk-align-1]. (2) Bất kỳ ký tự khác khoảng trắng nào ở cột `c` hoặc bên trái cột `c`[^bk-align-2]. (3) Một ngoặc đóng mà ngoặc mở nằm trước danh sách. (4) Chỗ bắt đầu "đơn vị module" kế tiếp (một định nghĩa hay khai báo). Với luật (1), `/\` đó mở conjunct tiếp theo của *cùng* danh sách; với ba luật còn lại thì cả danh sách kết thúc. Danh sách `\/` có luật tương tự.
- **Thụt sai thì sao.** Sách nêu ví dụ: nếu dòng `= y` không thụt sang bên phải dấu `/\` (nằm đúng cột của `/\` hoặc bên trái nó), thì `/\ x'` trở thành danh sách chỉ có một conjunct, và `/\` thứ hai bị hiểu là toán tử infix thông thường[^bk-align-bad]. Vì vậy hãy luôn thụt mỗi conjunct hoàn toàn sang phải dấu `/\` của nó. Ngoặc đơn cũng không cứu được: sách nói không dùng ngoặc để lách luật căn lề được, và nêu ví dụ bất hợp pháp `/\ (x'` xuống dòng rồi `= y)` (mục 15.2.2).
- **Lượng từ, `IF`, `CASE`, `LET`, `CHOOSE` "ăn" tới cùng.** Các cấu trúc này không có từ khóa kết thúc[^bk-undelim]. Vì vậy chúng được coi như toán tử tiền tố có độ ưu tiên thấp nhất, và biểu thức làm bằng chúng kéo dài đến mức có thể[^bk-extends-far]. Sách đưa ví dụ ngay sau quy tắc này[^bk-rule-ex]: `\A x \in S : P(x) \/ Q` nghĩa là `\A x \in S : (P(x) \/ Q)`, không phải `(\A x \in S : P(x)) \/ Q` (mục 15.2.1). Ai quen `all(...) || q` của Rust cần chú ý điểm này (nhận xét của người soạn).
- **`=` và `<=>`.** Với Boolean, hai toán tử dùng thay nhau được[^bk-equiv]. Dù vậy sách vẫn nêu một lý do tinh tế để viết `<=>` khi hai vế là Boolean (mục 16.1.3)[^bk-equiv-subtle]. Lý do là `=` không phải toán tử của logic mệnh đề hay vị từ, nên một hằng đúng của logic chưa chắc còn đúng với giá trị phi-Boolean nếu thay `≡` bằng `=`[^bk-equiv-3]. Mục 8 nói thêm về chuyện này.
- **Kiểm bảng chân trị bằng máy.** Sách lưu ý máy làm bảng chân trị tốt hơn người, và TLC kiểm được các hằng đúng của logic mệnh đề[^bk-tlc-calc]. Learn TLA+ có một mẹo: hàm `[p, q \in BOOLEAN |-> p => q]` liệt kê trọn bảng chân trị[^learn-truth] (hàm xem mục 3).

</details>

**Quay lại Q1:** logic của TLA+ là logic mệnh đề cộng hai lượng từ trên tập; `=>` là phát biểu chứ không phải rẽ nhánh, và dấu `/\`/`\/` xếp cột thay cho ngoặc. Với `lease-svc`, "node nào được phục vụ" là một danh sách bullet, còn "key có chủ thì epoch lớn hơn 0" là một phép `=>`.

## 2. Tập hợp: toán tử, lọc, ánh xạ, `SUBSET`, `UNION` (Q2)

> **Trả lời ngắn.** Một tập hợp được xác định hoàn toàn bởi các phần tử của nó: không có thứ tự, không có trùng lặp[^bk-set-same]. TLA+ có các phép `\in`, `\cup`, `\cap`, `\subseteq` và `\` (hiệu). Để dựng tập mới từ tập cũ, một cách là lọc bằng `{x \in S : P}`[^bk-filter]. Cách kia là ánh xạ bằng `{e : x \in S}`[^bk-map]. Ngoài ra `SUBSET S` là tập mọi tập con của `S`[^sum-subset]. `UNION S` là hợp của mọi phần tử của `S`[^sum-union].

**Giải thích.**

*Với service `lease-svc`.* Gần như câu hỏi vận hành nào về `lease-svc` cũng là câu hỏi về tập: những node nào đang up, những key nào đang rảnh, node `n1` đang giữ những key nào, hiện có những chủ nào. Để trả lời, ta cần ba việc: so sánh và kết hợp các tập, chọn ra phần tử thỏa một điều kiện (lọc), và biến mỗi phần tử thành thứ khác (ánh xạ). Phần này đi qua cả ba; các ví dụ về `lease-svc` dùng ký hiệu `L[k]` của mục 3 và được ghép hoàn chỉnh ở mục 9.

### 2.1 Tập hợp và các phép toán

TLA+ lấy khái niệm tập và quan hệ `∈` làm khái niệm nguyên thủy, không định nghĩa thêm[^bk-set-undef]. Hai tập bằng nhau khi và chỉ khi có cùng phần tử, nên `{0, 1, 2}`, `{2, 1, 0}` và `{0, 0, 1, 2, 2}` là cùng một tập[^bk-set-same]. Tập rỗng viết `{}`.

| Phép | Gõ ASCII | Ví dụ (giá trị theo định nghĩa) |
|---|---|---|
| thuộc / không thuộc | `x \in S`, `x \notin S` | `2 \in {1, 2}` là `TRUE` |
| hợp | `S \cup T` hoặc `S \union T`[^sum-ascii-cup] | `{1, 2} \cup {2, 3}` = `{1, 2, 3}` |
| giao | `S \cap T` hoặc `S \intersect T`[^sum-ascii-cap] | `{1, 2} \cap {2, 3}` = `{2}` |
| hiệu | `S \ T` | `{1, 2, 3} \ {2}` = `{1, 3}` |
| tập con | `S \subseteq T` | `{1, 3} \subseteq {3, 2, 1}` là `TRUE` |
| bằng / khác | `=`, `#` (hoặc `/=`)[^bk-ascii-box] | |
| tập lũy thừa | `SUBSET S` | `SUBSET {1, 2}` = `{{}, {1}, {2}, {1, 2}}` |
| hợp các phần tử | `UNION S` | `UNION {{1, 2}, {2, 3}, {3, 4}}` = `{1, 2, 3, 4}` |
| tích Descartes | `S \X T` | tập các cặp `<<a, b>>` |

Các phép trong bảng định nghĩa như sau. Giao `S ∩ T` gồm các phần tử có ở cả hai tập[^bk-cap]. Hợp `S ∪ T` gồm các phần tử có ở một trong hai hoặc cả hai[^bk-cup]. `S ⊆ T` đúng khi mọi phần tử của `S` đều thuộc `T`[^bk-subseteq]. Hiệu `S \ T` gồm các phần tử của `S` không thuộc `T`[^bk-diff]. Ví dụ `UNION` trong bảng lấy từ sách[^bk-union-ex]. Ví dụ `SUBSET` cũng vậy[^bk-subset-ex].

### 2.2 Lọc và ánh xạ

`{x \in S : P}` là tập con của `S` gồm các `x` thỏa `P`. Ví dụ tập số tự nhiên lẻ viết `{n \in Nat : n % 2 = 1}`[^bk-filter]. Với `lease-svc`, tập các key đang rảnh là `{k \in Key : L[k].owner = NoOwner}`: bắt đầu từ tập `Key` có sẵn rồi giữ lại key nào chưa có chủ.

`{e : x \in S}` là tập các giá trị có dạng `e` khi `x` chạy trên `S`; tập số lẻ cũng viết được `{2 * n + 1 : n \in Nat}`[^bk-map]. Với `lease-svc`, tập các chủ hiện có là `{L[k].owner : k \in Key}`: mỗi key cho ra một chủ, các chủ trùng nhau tự gộp lại. Learn TLA+ có một mẹo để khỏi lẫn hai dạng: đọc dấu hai chấm là "mà" (*where*). "Bình phương của `x` mà `x` thuộc 1..4" là ánh xạ, còn "`x` thuộc 1..4 mà `x` chẵn" là lọc[^learn-colon].

Cả hai dạng tổng quát hóa giống lượng từ. `{e : x \in S, y \in T}` là tập các phần tử dạng `e` khi `x` chạy trên `S` và `y` chạy trên `T`[^bk-map-multi]. Trong dạng lọc, `x` còn có thể là một tuple, chẳng hạn `{<<y, z>> \in S : P}` là các cặp trong `S` thỏa `P`[^bk-set-tuples].

```tla
EXTENDS Naturals, FiniteSets

Small   == {1, 2, 3}
Evens   == {n \in 1..10 : n % 2 = 0}            \* lọc:      {2, 4, 6, 8, 10}
Squares == {n * n : n \in 1..4}                 \* ánh xạ:   {1, 4, 9, 16}
Pairs   == {<<a, b>> \in Small \X Small : a < b} \* {<<1, 2>>, <<1, 3>>, <<2, 3>>}
Subs    == SUBSET {1, 2}                        \* {{}, {1}, {2}, {1, 2}}
Flat    == UNION {{1, 2}, {2, 3}, {3, 4}}       \* {1, 2, 3, 4}
Count   == Cardinality(Evens)                   \* 5 (cần FiniteSets)
```

`Cardinality(S)` (số phần tử của một tập hữu hạn) và `IsFiniteSet(S)` không phải built-in mà nằm trong module `FiniteSets`[^bk-cardinality].

### 2.3 Vì sao không có `{x : P(x)}` trần

Có thể bạn muốn viết `{x : P(x)}` cho "mọi `x` thỏa `P`". TLA+ không cho. Lý do là nghịch lý Russell: nếu có "tập mọi tập `S` sao cho `S ∉ S`" thì ra mâu thuẫn. Nguồn gốc của mâu thuẫn là "tập" đó không phải tập, và không có cách nào viết nó trong TLA+[^bk-russell]. Hệ quả thực dụng (nhận xét của người soạn): muốn lấy "các `x` thỏa `P`", bạn luôn phải lọc từ một tập nguồn `S` có sẵn bằng `{x \in S : P}`, giống mệnh đề `FROM` của SQL.

### 2.4 Cái giá của tập lớn

Khi dùng TLC (chương 04), kích thước tập là chuyện sống còn. Learn TLA+ cảnh báo `SUBSET` của một tập 86400 phần tử có $2^{86400}$ phần tử, đừng làm vậy. Muốn kiểm tra "`S` là tập con của `T`" thì viết `S \subseteq T`, không phải `S \in SUBSET T`[^learn-subset-cost]. Cũng trang Learn TLA+ đó cho biết số phần tử của `[A -> B]` là $|B|^{|A|}$ (hàm xem mục 3)[^learn-fnset-size]. Thử tính cho `lease-svc` (phép tính của người soạn): nếu cluster có 10 node thì `SUBSET Node` có $2^{10} = 1024$ phần tử, nhưng 30 node đã là $2^{30} = 1\,073\,741\,824$. Còn `[Key -> Node]` với 10 key và 5 node là $5^{10} = 9\,765\,625$ hàm. Nhiều lỗi "TLC chạy mãi" xuất phát từ một `SUBSET` hay `[S -> T]` quá lớn như vậy (nhận xét của người soạn).

<details><summary>Đi sâu: ngữ nghĩa dựa trên <code>∈</code>, thứ tự duyệt, và phần tử phải "so sánh được" trong TLC</summary>

- **Nền tảng.** TLA+ dựa trên lý thuyết tập Zermelo–Fränkel, trong đó mọi giá trị đều là tập[^bk-zf]. Mọi toán tử tập khác đều định nghĩa được qua `∈`; chẳng hạn `S ∪ T` được định nghĩa bằng `∀ x : (x ∈ (S ∪ T)) ≡ (x ∈ S) ∨ (x ∈ T)`[^bk-set-sem]. Nhận xét của người soạn: nghĩa là hai tập "bằng nhau" khi có cùng phần tử (extensionality), và không có khái niệm danh tính (identity) như con trỏ.
- **Không có thứ tự duyệt.** Vì tập không có thứ tự nên đặc tả đúng không được phụ thuộc vào thứ tự duyệt. Sách mô tả TLC đánh giá `\E x \in S : p` bằng cách duyệt các phần tử của `S` "theo một thứ tự nào đó"[^tlc-order]; đừng viết đặc tả mà đúng hay sai tùy thứ tự (nhận xét của người soạn).
- **Phần tử phải so sánh được (khi chạy TLC).** TLC chỉ làm việc với "giá trị TLC": tập hữu hạn các giá trị *so sánh được* với nhau. Chuỗi và số không so sánh được, vì ngữ nghĩa TLA+ không nói `"abc"` có bằng 42 hay không. Do đó `{"abc", 42}` không phải giá trị TLC, trong khi `{{"abc"}, {4, 2}}` thì được, vì hai tập khác số phần tử thì chắc chắn khác nhau[^tlc-comparable]. Tuple và record có thành phần là giá trị TLC thì cũng là giá trị TLC[^tlc-tuple-value]. Trong thực hành, nên giữ cho các phần tử của một tập cùng loại (nhận xét của người soạn).
- **`\X` không kết hợp.** `A \X B \X C` là tập các bộ ba, khác với `A \X (B \X C)` là tập các cặp `<<a, <<b, c>>>>`[^bk-product]. Learn TLA+ nhắc cùng điều này[^learn-product].
- **Số trong tập.** Các tập số (`Nat`, `Int`, `a..b`) thuộc về module chuẩn, xem mục 7.

</details>

**Quay lại Q2:** tập là bộ sưu tập không thứ tự, không trùng; mọi tập mới đều dựng từ một tập nguồn bằng lọc, ánh xạ, `SUBSET`, `UNION` hoặc `\X`. Với `lease-svc`, "key nào đang rảnh" là một phép lọc trên `Key`, còn "hiện có những chủ nào" là một phép ánh xạ.

## 3. Hàm: `[x \in S |-> e]`, `DOMAIN`, `[S -> T]`, `EXCEPT` (Q3)

> **Trả lời ngắn.** Hàm trong TLA+ là hàm toán học: mỗi hàm có một miền xác định `DOMAIN f` (luôn là một tập), và mỗi phần tử của miền được gán một giá trị `f[x]`[^bk-fn-def]. Ta viết hàm bằng `[x \in S |-> e]`, và viết *tập các hàm* từ `S` vào `T` bằng `[S -> T]`[^bk-fn-set]. Muốn "cập nhật", ta viết `[f EXCEPT ![c] = e]`, biểu thức này tạo ra một hàm *mới* giống `f` trừ tại `c`[^bk-except-def]. Sách nói hàm tương ứng với mảng của ngôn ngữ lập trình[^bk-fn-array] (nhận xét của người soạn: cũng gần với map hay dictionary), và trạng thái của một hệ thống thường là một hàm (nhận xét của người soạn).

**Giải thích.**

*Với service `lease-svc`.* Trái tim của `lease-svc` là bảng lease: mỗi key ứng với một lease. Trong code Rust, đó sẽ là một `HashMap<Key, Lease>` mà mỗi lần cấp lease ta `insert` vào tại chỗ. Đặc tả thì khác: nó không mô tả vùng nhớ mà mô tả *giá trị* của bảng ở từng thời điểm. Bảng lease là một hàm từ `Key` sang lease, và mỗi bước của hệ thống cho ra một bảng mới thay vì sửa bảng cũ. Mục này dạy cách viết và "sửa" hàm theo kiểu đó.

### 3.1 Hàm và miền

Hàm `f` có miền `DOMAIN f` và gán cho mỗi `x` thuộc miền một giá trị `f[x]`. TLA+ dùng ngoặc vuông như ký hiệu mảng, thay cho `f(x)` của toán học[^bk-fn-def]. Hai hàm bằng nhau khi có cùng miền và cùng giá trị tại mọi điểm[^bk-fn-eq]. Còn `f[x]` chỉ có giá trị xác định khi `x` thuộc miền[^bk-fn-value].

`[x \in S |-> e]` là hàm có miền `S` mà `f[x] = e` tại mọi `x \in S`[^sum-fn]. Ví dụ của sách: `succ == [n \in Nat |-> n + 1]` là hàm kế tiếp trên số tự nhiên[^bk-succ]. Với `lease-svc`, bảng lease lúc khởi động, mọi key chưa ai giữ, là `[k \in Key |-> [owner |-> NoOwner, epoch |-> 0]]` (record xem mục 4).

### 3.2 Tập hàm

`[S -> T]` là tập mọi hàm có miền đúng bằng `S` và giá trị rơi vào `T`. Sách nói rõ miền giá trị có thể là một tập con *bất kỳ* của `T`[^bk-fn-set], nên `[S -> T]` *không* đòi hàm phủ hết `T`. Ta hay dùng nó làm "kiểu" của biến. Sách ví dụ `mem \in [Adr -> Val]` cho một bộ nhớ: trạng thái bộ nhớ là phép gán giá trị cho địa chỉ, gọi là "mảng" trong lập trình và "hàm từ `Adr` sang `Val`" trong toán học[^bk-fn-array2]. Với `lease-svc`, "bảng lease hợp lệ" nằm trong `[Key -> Lease]`. Số phần tử của `[A -> B]` là $|B|^{|A|}$[^learn-fnset-size].

### 3.3 Cập nhật bằng `EXCEPT`

`[f EXCEPT ![c] = e]` là hàm giống `f` ngoại trừ tại `c` có giá trị `e`; theo cách nhìn mảng, đó là mảng thu được từ `f` bằng cách gán `e` vào `f[c]`[^bk-except-array]. Trong `e` ta dùng `@` để chỉ giá trị cũ `f[c]`[^bk-except-at]. Nhiều điều khoản cách nhau dấu phẩy được áp dụng lần lượt, và `![c][d]` cập nhật phần tử lồng[^bk-except-nested]. Chẳng hạn bước "cấp lease cho node `n` trên key `k`" của `lease-svc` viết là `[L EXCEPT ![k].owner = n, ![k].epoch = @ + 1]`: chủ đổi thành `n`, epoch tăng thêm 1.

```tla
EXTENDS Naturals

Succ   == [n \in Nat |-> n + 1]
Square == [n \in 1..5 |-> n * n]           \* DOMAIN Square = 1..5;  Square[3] = 9

Cfg    == [h \in {"a", "b"} |-> 0]         \* hàm có miền là hai chuỗi => cũng là record [a |-> 0, b |-> 0]
Cfg2   == [Cfg EXCEPT !["a"] = @ + 1]      \* [a |-> 1, b |-> 0]

Grid   == [r \in 1..2 |-> [c \in 1..2 |-> 0]]
Grid2  == [Grid EXCEPT ![1][2] = 7]        \* chỉ ô (1,2) đổi thành 7

Status == [Server -> {"online", "booting", "offline"}]   \* tập mọi cách gán trạng thái cho từng server
```

`Server` ở đây là một CONSTANT (chương 03 và 04 sẽ dạy), và ví dụ `Status` theo Learn TLA+[^learn-status].

*`EXCEPT` không thêm khóa mới.* Nếu `c` không thuộc `DOMAIN f` thì `[f EXCEPT ![c] = e]` chính là `f`[^bk-except-nondomain]. Sách nói TLC cảnh báo biểu thức kiểu này vì nhiều khả năng đó là lỗi[^tlc-nowarning]. Với `lease-svc`, nếu `Release(L, k)` bị gọi với một `k` không có trong `Key` thì bảng trả về y nguyên và không báo lỗi gì, một bug rất dễ lọt. Khác `HashMap::insert` của Rust (mục 10): muốn "thêm khóa", ta phải dựng một hàm có miền lớn hơn.

```tla
EXTENDS TLC

PutPure(f, k, v) ==                               \* thuần TLA+
    [x \in (DOMAIN f) \cup {k} |-> IF x = k THEN v ELSE f[x]]

Put(f, k, v) == (k :> v) @@ f                     \* dùng toán tử của module TLC
```

Trong module `TLC`, `d :> e` là hàm một điểm `[x \in {d} |-> e]`, còn `f @@ g` ghép hai hàm và ưu tiên giá trị ở `f` khi trùng khóa[^repo-tlc-ops]. Learn TLA+ mô tả cùng điều và nói thêm: hai toán tử đó chỉ có khi `EXTENDS TLC`[^learn-tlc-ops]. Suy ra từ định nghĩa (nhận xét của người soạn): `(k :> v) @@ f` ghi đè khóa `k` nếu đã có, còn `f @@ (k :> v)` thì giữ giá trị cũ.

### 3.4 Nhiều đối số

Hàm nhiều đối số là hàm có miền là tập các tuple, nên `f[a, b]` viết tắt của `f[<<a, b>>]`[^bk-multi-arg]. Sách khuyên chuộng hàm một đối số, và nếu được thì thay `f[a, b]` bằng `f[a][b]` (hàm trả về hàm)[^bk-multi-prefer]. Learn TLA+ minh họa bằng `[x, y \in S |-> x * y]` rồi gọi `Prod[3, 5]`[^learn-multi].

### 3.5 Hàm khác toán tử

Đây là chỗ người mới hay nhầm. `f[x]` và `Op(x)` trông gần giống nhau, nhưng ví dụ `Tail(s)` và `fact[n \in Nat]` là hai loại đối tượng khác nhau[^bk-fn-vs-op0]. Bảng sau tóm tắt khác biệt.

| | Hàm `f[x]` | Toán tử `Op(x)` |
|---|---|---|
| Có miền? | **Phải** có miền, là một tập[^bk-fn-vs-op1] | Không có miền |
| Là biểu thức độc lập? | Có: `f \in S` hợp lệ[^bk-fn-vs-op-expr] | Không: `Op \in S` là vô nghĩa[^bk-def-opname] |
| Đệ quy | Được: `fact[n \in Nat] == ...`[^bk-recfn-ok] | Sách: không đệ quy được[^bk-fn-vs-op3] (xem Đi sâu về `RECURSIVE`) |
| Làm giá trị của biến? | Được | Không: nếu biến có thể nhận `V` làm giá trị thì `V` phải là hàm[^bk-fn-vs-op2] |
| Nhận toán tử làm đối số? | Không | Được[^bk-op-arg] |

Ví dụ rõ nhất: không thể định nghĩa `Tail` như một hàm, vì miền của nó phải gồm mọi dãy khác rỗng, và tập các dãy như vậy "quá lớn để là một tập"[^bk-tail-notfn]. Còn vì biến chỉ mang được giá trị hàm chứ không mang được toán tử, nên mọi cấu trúc ánh xạ nằm trong biến, như bảng lease của `lease-svc`, đều phải viết bằng hàm, record hoặc tuple. Phần "phép tính" phụ trợ như `Free(L)` hay `Held(L, n)` thì thường viết bằng toán tử (nhận xét của người soạn). Khi những khác biệt trên không quyết định được, sách cho đó là chuyện sở thích và tác giả thường chọn toán tử[^bk-prefer-op].

<details><summary>Đi sâu: định nghĩa đệ quy, <code>RECURSIVE</code>, "<code>f' = [...]</code>" so với "<code>\A i : f'[i] = ...</code>", và <code>@</code>/<code>:&gt;</code> trong TLC</summary>

- **Hàm đệ quy.** Viết `fact == [n \in Nat |-> IF n = 0 THEN 1 ELSE n * fact[n - 1]]` là *bất hợp pháp*, vì `fact` ở vế phải chưa được định nghĩa[^bk-fact-illegal]. Thay vào đó, TLA+ có dạng viết tắt `fact[n \in Nat] == IF n = 0 THEN 1 ELSE n * fact[n - 1]`, và sách nói rõ rằng TLA+ cho phép sự "vòng tròn bề ngoài" này riêng cho định nghĩa hàm đệ quy[^bk-recfn-ok]. Ngữ nghĩa đằng sau là `CHOOSE` (mục 5): sách coi dạng viết tắt đó chỉ là cách viết gọn của một định nghĩa dùng `CHOOSE`[^bk-recfn-abbrev]. Vì vậy nó "không nhất thiết định nghĩa ra một hàm"; nếu không tồn tại hàm thỏa phương trình thì kết quả là một giá trị không xác định[^bk-recfn-silly].
- **Đệ quy tương hỗ** (hai hàm gọi nhau) không được phép[^bk-mutual]. Thủ thuật của sách là dựng một hàm trả về record có các trường là hai hàm cần định nghĩa; cách này đưa được mọi đệ quy tương hỗ về một đệ quy đơn[^bk-mr-trick].
- **`RECURSIVE` cho toán tử.** Sách xuất bản năm 2002 nói toán tử không đệ quy được[^bk-fn-vs-op3]. Learn TLA+ cho biết TLA+ đã được cập nhật năm 2008 để có toán tử đệ quy, và phải khai báo trước (`RECURSIVE SumSeq(_)` rồi mới định nghĩa)[^learn-recursive]. Hai nguồn không mâu thuẫn: sách là mô tả gốc, Learn mô tả bản ngôn ngữ về sau.
- **Khi viết đặc tả: đặt giá trị mới của cả hàm, không phải từng điểm.** Xét `f' = [i \in Nat |-> i + 1]`, công thức xác định duy nhất hàm `f'`, và `\A i \in Nat : f'[i] = i + 1`, công thức chỉ ràng buộc từng điểm nên `f'` có thể là hàm có miền rộng hơn, thậm chí không phải hàm. Sách nói hai công thức không tương đương: công thức đầu kéo theo công thức sau chứ không ngược lại. Khi viết đặc tả, ta hầu như luôn muốn đặt giá trị mới của *cả biến*, nên hãy viết dạng đầu[^bk-using-fn]. Ký hiệu `f'` sẽ học ở chương 03.
- **Bảng chân trị bằng hàm.** `TruthTable == [p, q \in BOOLEAN |-> p => q]` liệt kê `=>` trên mọi cặp Boolean; Learn TLA+ cho biết kết quả hiển thị dưới dạng "mở rộng" dùng `:>` và `@@`[^learn-truth].

</details>

**Quay lại Q3:** hàm là ánh xạ có miền là một tập; `EXCEPT` sinh hàm mới cùng miền (không thêm khóa), và vì biến chỉ mang được giá trị chứ không mang được toán tử, thứ ánh xạ nằm trong biến, như bảng lease của `lease-svc`, được viết bằng hàm.

## 4. Record, tuple, sequence: tất cả đều là hàm (Q4)

> **Trả lời ngắn.** Record là hàm có miền là một tập hữu hạn các chuỗi, và `r.h` chỉ là cách viết khác của `r["h"]`[^bk-record-def]. Tuple `<<a, b, c>>` là hàm có miền `{1, 2, 3}`[^bk-tuple-fn]. Sequence (dãy hữu hạn) *chính là* tuple[^bk-seq-tuple]. Module `Sequences` cho các toán tử `Len`, `Head`, `Tail`, `Append`, `\o` và tập `Seq(S)`[^bk-seq-ops]. Ngoài ra có `SubSeq` để cắt một đoạn[^bk-subseq]. Có cả `SelectSeq` để lọc theo điều kiện[^bk-selectseq]. Chỉ số bắt đầu từ 1.

**Giải thích.**

*Với service `lease-svc`.* Dữ liệu của `lease-svc` có ba dạng quen thuộc. Một lease gồm chủ và epoch, ví dụ `[owner |-> "n1", epoch |-> 3]`. Một request xin lease gồm loại, key và node xin, ví dụ `[kind |-> "acquire", key |-> "shard-7", node |-> "n2"]`. Và các request chưa xử lý xếp thành một hàng chờ theo thứ tự đến. Trong Rust, bạn sẽ viết hai `struct` và một `VecDeque<Request>`. TLA+ không có ba loại đó: cả ba đều là hàm, nên mọi công cụ của hàm ở mục 3 (`DOMAIN`, `EXCEPT`, `[S -> T]`) dùng chung được. Learn TLA+ nói gọn: TLA+ chỉ có hai kiểu collection "thật" là tập và hàm; sequence và struct chỉ là những lớp hàm đặc biệt mà lập trình viên quen thuộc[^learn-two-collections].

### 4.1 Record

Vì sao có record khi đã có tuple? Lamport trả lời: toán học cho phép dùng bộ ba có thứ tự, nhưng lập trình viên đã học được rằng dùng tuple như thế gây lỗi, vì dễ quên một thành phần là thứ hai hay thứ ba[^bk-record-why]. Với request ở trên, nếu viết `<<"acquire", "shard-7", "n2">>` thì đọc rất khó biết thành phần nào là key, thành phần nào là node (nhận xét của người soạn).

Record dùng tên trường. Ta viết `[op |-> "Wr", adr |-> 7, val |-> 42]` để tạo record (ký hiệu `↦` gõ ASCII là `|->`[^bk-mapsto-ascii]), rồi truy cập bằng `r.adr`. Thứ tự các trường không có nghĩa[^bk-record-unordered]. *Tập* các record cùng dạng viết bằng dấu hai chấm: `[op : {"Rd", "Wr"}, adr : Adr]` là tập mọi record có `op` thuộc `{"Rd", "Wr"}` và `adr` thuộc `Adr`[^bk-record-set]. Tên trường không được lặp, chẳng hạn `[a : S, a : T]` là bất hợp pháp[^bk-record-illegal]. Với `EXCEPT`, cập nhật trường của record dùng `!.tên_trường`, ví dụ `[chan EXCEPT !.val = d, !.rdy = 1 - @]`[^bk-except-record].

### 4.2 Tuple và sequence

`<<a, b, c>>` (gõ `<<` và `>>`[^bk-angle]) thực ra là hàm có miền `{1, 2, 3}`, ánh xạ 1 tới `a`, 2 tới `b`, 3 tới `c`. Do đó `<<a, b, c>>[2] = b`[^bk-tuple-fn]. Module `Sequences` định nghĩa dãy hữu hạn là tuple, nên một dãy dài `n` là hàm có miền `1..n`[^bk-seq-tuple]. Dãy rỗng là tuple không phần tử, viết `<<>>`, và chỉ có đúng một dãy như vậy[^bk-empty-seq].

Các ví dụ sau dùng hàng chờ `q` của `lease-svc`, tạm thu gọn mỗi request thành một số. Năm toán tử đầu lấy theo ví dụ trong mục 4.1 của sách[^bk-seq-ops-ex]; `Len`, `q[1]`, `SubSeq` và `SelectSeq` là ví dụ của người soạn, giá trị tính tay theo định nghĩa.

```tla
EXTENDS Naturals, Sequences

q == <<3, 7>>
Ex1 == Head(q)               \* 3             phần tử đầu
Ex2 == Tail(q)               \* <<7>>         bỏ phần tử đầu
Ex3 == Append(q, 3)          \* <<3, 7, 3>>   thêm vào cuối
Ex4 == q \o <<3>>            \* <<3, 7, 3>>   nối dãy
Ex5 == Len(q)                \* 2
Ex6 == q[1]                  \* 3             chỉ số bắt đầu từ 1
Ex7 == SubSeq(<<1, 2, 3, 4>>, 2, 3)                      \* <<2, 3>>
Ex8 == SelectSeq(<<1, 2, 3, 4>>, LAMBDA n : n % 2 = 0)   \* <<2, 4>>
```

`Seq(S)` là *tập* mọi dãy hữu hạn có phần tử thuộc `S`; ví dụ `<<3, 7>>` thuộc `Seq(Nat)`[^bk-seq-set]. Toán tử `\o` là phép nối, gõ ASCII của `∘`[^bk-seq-concat]. `SelectSeq(s, Test)` giữ các phần tử `s[i]` mà `Test(s[i])` đúng[^bk-selectseq]. Đối số thứ hai của nó là một *toán tử*, có thể là toán tử vô danh viết bằng `LAMBDA`[^learn-lambda] hoặc một toán tử có tên mà bạn định nghĩa rồi truyền vào.

Về chỉ số: Learn TLA+ nhấn mạnh hai lần rằng sequence "1-indexed"[^learn-1idx]. Chọn 1 có lý do thực dụng, vì `1..Len(s)` là tập chỉ số của `s`, và nếu `s = <<>>` thì nó là `1..0 = {}`, đúng như ta kỳ vọng[^learn-empty-range].

### 4.3 Hàng chờ bằng sequence

Hàng đợi, stack hay buffer của một hệ thống thường mô hình bằng sequence (nhận xét của người soạn). Với `lease-svc`, request mới vào thì `q' = Append(q, m)`; muốn xử lý request đầu thì lấy `Head(q)` và đặt `q' = Tail(q)`. Ký hiệu `q'`, giá trị của `q` ở bước kế tiếp, học ở [Chương 03](03-trang-thai-hanh-vi-action.md). Bản thân sách cũng làm đúng thế khi đặc tả một hàng đợi FIFO có bộ đệm chuyển thông điệp[^bk-fifo].

### 4.4 Record làm được những việc struct không làm được

Vì record là hàm, ta có thể thao tác trên nó theo cách mà struct của ngôn ngữ lập trình không có. Sách ghi nhận điều này: định nghĩa record bằng hàm cho phép những thao tác "không có đối tác trong ngôn ngữ lập trình"[^bk-record-power]. Ví dụ, toán tử `R(r, s)` của sách thay giá trị mọi trường của `r` mà `s` cũng có[^bk-record-override]; dưới đây ta viết lại nó với tên `Override`. Với `lease-svc`, `Override(lease, [owner |-> "n2"])` cho ra lease đổi chủ mà giữ nguyên epoch.

```tla
Override(r, s) == [c \in DOMAIN r |-> IF c \in DOMAIN s THEN s[c] ELSE r[c]]
```

### 4.5 Chuỗi ký tự

Trong định nghĩa hình thức của sách, chuỗi là tuple của ký tự[^bk-string]. Nhưng trong thực hành, Learn TLA+ nói chuỗi chỉ có phép `=` và `#`, và ta dùng chúng như định danh mờ đục, giống kiểu `:symbol` của một số ngôn ngữ. Nếu cần thao tác trên chuỗi thì phải lưu vào sequence[^learn-string]. Vì vậy hãy viết `"Wr"`, `"online"`, `"acquire"` như những nhãn.

<details><summary>Đi sâu: <code>Tail(&lt;&lt;&gt;&gt;)</code> sách khác module hiện hành, <code>Len</code> bằng <code>CHOOSE</code>, và những lỗi cú pháp quen thuộc</summary>

- **Định nghĩa trong module chuẩn.** Mã nguồn module `Sequences` ở commit ghim định nghĩa `Seq(S) == UNION {[1..n -> S] : n \in Nat}`[^repo-seq-defs] và `Len(s) == CHOOSE n \in Nat : DOMAIN s = 1..n`[^repo-len]. Còn `Append(s, e) == s \o <<e>>`[^repo-append]. Và `Head(s) == s[1]`[^repo-head]. Nghĩa là `Len` được định nghĩa qua `CHOOSE` và `DOMAIN` (mục 5); suy ra từ định nghĩa (nhận xét của người soạn), áp `Len` vào thứ không phải dãy cho kết quả không xác định.
- **`Tail` đã đổi.** Sách định nghĩa `Tail(s) = [i ∈ 1..(Len(s) − 1) ↦ s[i + 1]]`[^bk-tail-def]. Với `s = <<>>`, miền `1..(-1)` là rỗng, và tuple không phần tử là hàm duy nhất có miền rỗng[^bk-0tuple], nên kết quả là dãy rỗng (suy ra từ định nghĩa trong sách, nhận xét của người soạn). Mã nguồn module hiện hành thì viết khác: `Tail(s) == CASE s # << >> -> [i \in 1..(Len(s)-1) |-> s[i+1]]`, kèm chú thích rằng định nghĩa `Tail` đã đổi ngày 4/6/2013 vì bản gốc cho `Tail(<<>>) = <<>>`[^repo-tail]. `CASE` không có nhánh `OTHER` (mục 6), nên `Tail(<<>>)` giờ là giá trị không xác định (suy ra từ luật `CASE` ở mục 6, nhận xét của người soạn). Hệ quả thực hành: luôn bảo vệ bằng điều kiện `q # <<>>` trước khi dùng `Head(q)` hay `Tail(q)`.
- **`Head(<<>>)` là biểu thức ngớ ngẩn.** `<<>>[1]` không xác định vì miền rỗng, và TLC không đánh giá được `x[1]` khi `x = <<>>`[^bk-seq-silly]. Luật thứ tự đánh giá nằm ở mục 8.
- **Lỗi quen của Learn TLA+.** Viết `["key" |-> val]` thay vì `[key |-> val]` gây lỗi cú pháp, và TLC báo `Encountered "|->"`[^learn-record-key]. Viết `[key: "val"]` thay vì `[key: {"val"}]` gây lỗi "Attempted to compute the number of elements in the string" khi chạy TLC[^learn-record-err]. Lý do: vế phải dấu `:` phải là *tập*.
- **`Seq(S)` vô hạn.** Theo định nghĩa trên, `Seq(S)` là hợp của vô hạn tập (mỗi độ dài một tập), nên nó là tập vô hạn khi `S` khác rỗng. Dùng nó để nói `q \in Seq(Msg)` thì tự nhiên về mặt toán học. Còn TLC xử lý phép kiểm này ra sao, và làm thế nào giới hạn độ dài dãy khi chạy (ràng buộc không gian trạng thái), là chủ đề của chương 04 (suy luận của người soạn; chưa kiểm chứng về hành vi của TLC với `Seq`).

</details>

**Quay lại Q4:** record, tuple, sequence đều là hàm (miền lần lượt là tập chuỗi, `1..n`, `1..n`), nên chung một bộ công cụ; `Sequences` thêm các toán tử dãy và nhớ chỉ số bắt đầu từ 1. Với `lease-svc`, lease và request là record, còn hàng chờ request là sequence.

## 5. `CHOOSE`: chọn một giá trị xác định (Q5)

> **Trả lời ngắn.** `CHOOSE x \in S : P` là *một* giá trị `x` thuộc `S` thỏa `P`[^bk-choose-def]. Nếu không có giá trị nào thỏa, biểu thức vẫn có giá trị nhưng hoàn toàn tùy ý[^bk-choose-bounded]. Nó **không** ngẫu nhiên và không phải "phi tất định": trong toán học không có hàm hay toán tử phi tất định, biểu thức bằng 42 hôm nay thì mai vẫn bằng 42[^bk-choose-nondet]. Ta dùng nó chủ yếu để đặt tên cho một giá trị được xác định duy nhất bởi một tính chất[^bk-choose-unique].

**Giải thích.**

*Với service `lease-svc`.* Bảng lease cần nói được "key này chưa có chủ". Trong Rust, bạn dùng `Option<NodeId>` và gán `None`. TLA+ không có `None`; ta phải tự có một giá trị chắc chắn không trùng với node nào, và `CHOOSE` làm đúng việc đó. Nó là công cụ hơi lạ, vì ở bề ngoài giống "chọn bừa một phần tử" nhưng thực chất không phải: mục này giải thích nó là gì và vì sao một câu như "leader là node được `CHOOSE`" không mô hình hóa được bầu cử.

### 5.1 `CHOOSE` khác `\E` và khác `find`

Nếu quen Rust, bạn có thể thấy `CHOOSE` giống `iter().find(P)`. Nó khác ở hai điểm: kết quả không phụ thuộc thứ tự duyệt hay thời điểm tính (nhận xét của người soạn), và khi không có phần tử nào thỏa thì không panic mà cho một giá trị "không biết là gì". TLC thì coi trường hợp sau là lỗi, xem phần cuối của mục này. Còn so với `\E`: `\E` là một mệnh đề đúng hoặc sai ("có phần tử thỏa không?"), `CHOOSE` là một *giá trị* (nhận xét của người soạn).

Giá trị đó ràng buộc bởi hai quy tắc. Thứ nhất, nếu có `x` thỏa `P(x)` thì `P(CHOOSE x : P(x))` đúng. Thứ hai, nếu hai vị từ tương đương với mọi `x` thì hai `CHOOSE` tương ứng bằng nhau. Ngoài những gì suy ra được từ hai luật này, ta không biết gì về giá trị được chọn[^bk-choose-rules]. Hệ quả kỳ lạ là nếu không tồn tại `x` thỏa thì `CHOOSE x : P(x)` bằng `CHOOSE x : FALSE`. Vì phép chia `a/b` được định nghĩa bằng `CHOOSE`, suy ra `1/0` bằng `2/0`[^bk-choose-div].

### 5.2 Dùng để đặt tên: `NoOwner`

Ví dụ hay nhất của sách là cần một giá trị `NoVal` khác mọi giá trị bộ nhớ mà không muốn thêm tham số. Với `lease-svc`, đó là `NoOwner`, giá trị "chưa có chủ" khác mọi node.

```tla
EXTENDS Naturals

CONSTANTS Node, Val

NoOwner == CHOOSE v : v \notin Node               \* một giá trị nào đó không phải Node
NoVal   == CHOOSE v : v \notin Val                \* dạng trong sách: NoVal == CHOOSE v : v \notin Val
MaxOf(S) == CHOOSE x \in S : \A y \in S : y <= x  \* S là tập số hữu hạn khác rỗng
```

Về `NoVal`, sách giải thích rằng ta không biết nó bằng gì, chỉ biết nó *không phải* phần tử của `Val`[^bk-noval]. Đó là mọi thứ ta cần. `MaxOf` là thành ngữ "phần tử lớn nhất" do người soạn viết; nó hợp lệ khi `S` là tập hữu hạn khác rỗng gồm các số, còn nếu `S` rỗng thì kết quả không xác định.

### 5.3 Không phải phi tất định

Sách đối chiếu hai đặc tả để thấy khác biệt. Đặc tả `(x = CHOOSE n : n \in Nat) /\ [][x' = CHOOSE n : n \in Nat]_x` chỉ cho phép *một* hành vi, trong đó `x` luôn bằng cùng một số tự nhiên không xác định. Đặc tả `(x \in Nat) /\ [][x' \in Nat]_x` cho phép vô số hành vi, vì `x` có thể đổi ở mỗi bước[^bk-choose-vs-in]. (Ký hiệu chưa học, tạm đọc: `x'` là giá trị của `x` ở bước kế tiếp; `[][A]_x` là "mọi bước đều thỏa `A` hoặc giữ nguyên `x`"; chi tiết ở [Chương 03](03-trang-thai-hanh-vi-action.md).)

Bài học cho `lease-svc`: nếu ta viết "leader là `CHOOSE n \in UpNodes : TRUE`" thì leader sẽ là một node cố định, không bao giờ đổi; ta mô hình hóa được "một node nào đó là leader" nhưng không mô hình hóa được "leader có thể đổi sang node khác" (suy ra từ luật của `CHOOSE`, nhận xét của người soạn). Phi tất định, tức nhiều lựa chọn khả dĩ của hệ thống, trong TLA+ viết bằng `\in` hoặc `\E` trên biến ở trạng thái kế tiếp, **không** bằng `CHOOSE`.

### 5.4 `CHOOSE` khi chạy TLC

Ngữ nghĩa TLA+ cho `CHOOSE x \in S : P` một giá trị tùy ý nếu không có `x` thỏa. TLC thì coi trường hợp đó là lỗi, vì nó hầu như luôn do nhầm lẫn[^tlc-choose-err]. TLC chỉ đánh giá `CHOOSE` khi nó thật sự cần: `IF n > 5 THEN CHOOSE i \in 1..n : i > 5 ELSE 42` không báo lỗi khi `n <= 5`, vì nhánh `CHOOSE` không được đánh giá[^tlc-choose-lazy].

Còn `CHOOSE x : P` không chặn, tức không có `\in S`, thì TLC không đánh giá được[^tlc-unbounded]. Vì thế thành ngữ `NoOwner == CHOOSE v : v \notin Node` ở trên đặc tả đúng toán học nhưng TLC không tính nổi. Sách hướng dẫn ghi đè một định nghĩa như vậy (ví dụ `NotAnS`) bằng một giá trị mô hình trong phần `CONSTANT` của file cấu hình[^tlc-notans], chi tiết ở chương 04. Cuối cùng, khi nhiều phần tử cùng thỏa, Learn TLA+ nói yêu cầu duy nhất là kết quả phải *xác định*, tức luôn cùng một giá trị[^learn-choose-det]. Nhận xét của người soạn: đừng viết đặc tả phụ thuộc vào việc TLC chọn phần tử nào, vì đó là chi tiết cài đặt chứ không phải bảo đảm của ngôn ngữ.

<details><summary>Đi sâu: nền tảng ε của Hilbert, <code>IF</code>/<code>CASE</code> là <code>CHOOSE</code> trá hình, và vì sao phép chia cho 0 không "nổ"</summary>

- **Hilbert's ε.** `CHOOSE x : p` (dạng không chặn) là cấu trúc nguyên thủy của TLA+; các nhà toán học gọi nó là ε của Hilbert, và ý nghĩa của nó được định nghĩa bằng hai luật nói trên[^bk-hilbert]. Dạng chặn `CHOOSE x \in S : p` được định nghĩa là `CHOOSE x : (x \in S) /\ p`[^bk-choose-bounded].
- **`IF` và `CASE` định nghĩa qua `CHOOSE`.** Sách viết rõ rằng hai cấu trúc này được định nghĩa dùng `CHOOSE`[^bk-if-via-choose]; ví dụ `IF p THEN e1 ELSE e2` là `CHOOSE v : (p => (v = e1)) /\ (~p => (v = e2))`. Điều này giải thích vì sao `CASE` không có nhánh nào đúng cho kết quả không xác định (mục 6).
- **Tránh `1/0 = 2/0`.** Nếu bạn thấy khó chịu, sách đề nghị một toán tử `Choice(v, P)` cho giá trị phụ thuộc `v` khi không tồn tại `x`, để không suy ra được quan hệ giữa `1/0` và `2/0`. Nhưng sách cũng nói hiếm khi có lý do thực tế để lo chuyện đó[^bk-choice-op].
- **Ví dụ "ngớ ngẩn".** `circ[n \in Nat] == CHOOSE y : y # circ[n]` không định nghĩa được hàm nào, và định nghĩa này cho `circ` một giá trị không xác định[^bk-recfn-silly].

</details>

**Quay lại Q5:** `CHOOSE` là một giá trị cố định nhưng không biết là gì; nó đặt tên cho thứ được xác định bởi tính chất, như `NoOwner` của `lease-svc`, còn lựa chọn của hệ thống phải viết bằng `\in`/`\E`.

## 6. Định nghĩa, `LET`/`IN`, `IF`, `CASE` (Q6)

> **Trả lời ngắn.** `Id == biểu_thức` định nghĩa `Id` là *đồng nghĩa* với biểu thức đó[^bk-def-synonym]. `Op(p1, p2) == ...` định nghĩa toán tử có tham số[^bk-def-op]. `LET d == ... IN e` cho biểu thức `e` trong bối cảnh các định nghĩa cục bộ `d`[^sum-let]. `IF p THEN a ELSE b` bắt buộc có cả hai nhánh[^learn-if]. `CASE` có nhiều nhánh ngăn bởi `[]`.

**Giải thích.**

*Với service `lease-svc`.* Đặc tả `lease-svc` thực chất là một chồng định nghĩa: `Free`, `Held`, `Acquire`, `Release`, `Quota`, mỗi cái dựa trên các cái đứng trước. Không có câu lệnh nào chạy cả. Vì vậy `==` không giống phép gán `let` của Rust: nó khai báo rằng một tên *thay thế* cho một biểu thức. Mục này nói thay thế đó xảy ra thế nào, tên có hiệu lực ở đâu, và cách rẽ nhánh bằng `IF` và `CASE` (ví dụ `Backoff` ở dưới: client xin lease thất bại thì chờ bao lâu).

### 6.1 Định nghĩa là đồng nghĩa

Nếu `Id` là một tên đơn giản như `Init` hay `Spec`, thì `Id == exp` làm `Id` đồng nghĩa với `exp`[^bk-def-synonym]. Việc thay thế làm *sau khi phân tích cú pháp*, không phải trên văn bản thô: `x == a + b` làm `x * c` bằng `(a + b) * c`, không phải `a + b * c`[^bk-def-after-parse]. Với `Id(p) == exp`, `Id(e)` là biểu thức thu được bằng cách thay `p` bằng `e` trong `exp`[^bk-def-op]. Tên toán tử như `Send` đứng một mình, không kèm đối số, thì không phải một biểu thức[^bk-def-opname].

### 6.2 Phạm vi và đặt tên

Phạm vi của một khai báo hay định nghĩa là phần module đứng *sau* nó[^bk-scope], nên phải định nghĩa trước khi dùng. TLA+ cũng không cho định nghĩa lại một ký hiệu đã có nghĩa[^bk-redecl]. Vì thế `(\E v \in S : (e1 /\ \E v \in T : e2))` là bất hợp pháp, vì `v` thứ hai nằm trong phạm vi của `v` thứ nhất. Toán học thông thường và các ngôn ngữ lập trình đều cho phép khai báo lại như vậy (che bóng, *shadowing*), nhưng TLA+ cấm vì dễ gây nhầm lẫn và lỗi[^bk-redecl2].

### 6.3 `LET`/`IN`

Dạng tổng quát là `LET ∆1 ... ∆n IN e`, trong đó các định nghĩa đứng trước dùng được trong các định nghĩa sau[^learn-let-seq]. Ví dụ của sách: `LET sq(i) == i * i IN sq(1) + sq(2) + sq(3)` bằng `1 * 1 + 2 * 2 + 3 * 3`, tức 14[^bk-let-ex]. `LET` định nghĩa được cả tên không tham số (hằng cục bộ) lẫn toán tử có tham số, và chúng chỉ có hiệu lực trong phần `IN`. Với `lease-svc`, `LET` hợp để đặt tên cho một bước trung gian như "bảng sau khi thu hồi lease hết hạn" rồi dùng nó vài lần trong phần `IN` (nhận xét của người soạn).

### 6.4 `IF`

`IF p THEN e1 ELSE e2` bằng `e1` nếu `p` đúng và `e2` nếu `p` sai[^bk-if]. Vì là biểu thức nên nó luôn phải có `ELSE`[^learn-if]. `Acquire` ở mục 9 dùng đúng dạng này: nếu key chưa có chủ thì cấp, còn không thì trả bảng cũ. Ví dụ trong sách là quan hệ trạng thái kế tiếp của đồng hồ giờ: `HCnxt == hr' = IF hr # 12 THEN hr + 1 ELSE 1`[^bk-hcnxt] (`hr'` là giá trị của `hr` ở trạng thái kế tiếp, xem [Chương 03](03-trang-thai-hanh-vi-action.md)).

### 6.5 `CASE`

`CASE` có dạng `CASE p1 -> e1 [] p2 -> e2 [] ... [] OTHER -> e`. Ký hiệu ngăn cách là `□` của bản in, gõ `[]`[^bk-ascii-box]. **Cẩn thận**: `CASE` khác `match` của Rust. Nếu nhiều `p_i` cùng đúng, giá trị là *một trong các* `e_i` tương ứng và ngữ nghĩa không nói là cái nào; sách nói cấu trúc này thường dùng khi các `p_i` rời nhau[^bk-case-disjoint]. Nếu không có `p_i` nào đúng và không có `OTHER`, giá trị không xác định; còn có `OTHER` thì giá trị bằng biểu thức của `OTHER`[^bk-case-none].

```tla
EXTENDS Naturals

Max2(a, b) == IF a >= b THEN a ELSE b

Clamp(x, lo, hi) ==
    LET below == x < lo
        above == x > hi
    IN  IF below THEN lo ELSE IF above THEN hi ELSE x

Backoff(attempt) ==               \* các điều kiện rời nhau, có OTHER
    CASE attempt = 0 -> 0
      [] attempt = 1 -> 100
      [] OTHER       -> 400

\* Toán tử nhận toán tử làm đối số: R(_, _) khai báo "R là toán tử hai ngôi"
IsPartialOrder(R(_, _), S) ==
    /\ \A x, y, z \in S : R(x, y) /\ R(y, z) => R(x, z)
    /\ \A x \in S : ~R(x, x)
```

`IsPartialOrder` là ví dụ trong sách, có tham số đầu là một toán tử hai ngôi[^bk-ispo]. Ví dụ `CASE` ở trên cố ý chọn các nhánh rời nhau. Ngược lại, một `CASE score >= 90 -> "A" [] score >= 80 -> "B" ...` với `score = 95` sẽ làm *cả hai* nhánh đầu đúng, và kết quả không xác định theo ngữ nghĩa (nhận xét của người soạn, theo luật của sách ở trên).

<details><summary>Đi sâu: các cấu trúc "ăn" tới cùng, tham số toán tử, và vị trí của <code>==</code></summary>

- **`LET`/`IF`/`CASE` không có từ khóa kết thúc**, nên biểu thức kéo dài hết mức; đôi khi điều này buộc phải thêm ngoặc[^bk-undelim]. Ví dụ `IF x > 0 THEN y + 1 ELSE y - 1 + 2` hiểu là `ELSE (y - 1 + 2)` (mục 15.2.1).
- **Dấu `[]` của `CASE` trùng ký hiệu với toán tử thời gian "luôn luôn"** `[]`. Sách nói ký hiệu ngăn cách của `CASE` không phải toán tử thời gian gõ giống nó, và nó kết thúc mọi cấu trúc trừ `CASE` không có `OTHER`[^bk-case-sep]. Toán tử `[]` thời gian học ở chương 05.
- **Toán tử bậc cao.** Sách cho ví dụ `IsPartialOrder(R(_, _), S)` và nói `IsPartialOrder(>, Nat)` hợp cú pháp, bằng `TRUE` nếu `>` là toán tử thông thường. `IsPartialOrder(+, 3)` thì hợp cú pháp nhưng "ngớ ngẩn"[^bk-ispo-ex]. Nghĩa là TLA+ bắt lỗi *cú pháp* (số đối số) nhưng không bắt lỗi *ngữ nghĩa* (kiểu của đối số); xem mục 8.
- **`==` là ký hiệu của `≜`** trong bản in. Sách nói ngoại lệ chính của quy ước gõ "theo hình" là `==`[^bk-ascii-def]. Lỗi quen của Learn TLA+ là viết `=` một dấu thay cho `==` khi định nghĩa, hoặc `==` thay cho `=` khi so sánh[^learn-eq-err].
- **`Cardinality` dùng `LET` với hàm đệ quy cục bộ.** Ngay trong sách, `Cardinality(S)` được định nghĩa bằng `LET CS[T \in SUBSET S] == IF T = {} THEN 0 ELSE 1 + CS[T \ {CHOOSE x : x \in T}] IN CS[S]`. Cách này dùng hàm đệ quy cục bộ để vượt luật "toán tử không đệ quy"[^bk-cardinality-let]. Module chuẩn `FiniteSets` ở commit ghim giữ đúng định nghĩa này[^repo-card].
- **Chú thích.** Chú thích khối là `(* ... *)` (lồng được), chú thích cuối dòng là `\*`[^bk-comments].

</details>

**Quay lại Q6:** `==` là đồng nghĩa (không phải gán), `LET` thu hẹp phạm vi định nghĩa, `IF` luôn có `ELSE`, và `CASE` chỉ an toàn khi các nhánh rời nhau hoặc có `OTHER`. Với `lease-svc`, `Acquire` là một `IF`, còn `Backoff` là một `CASE` có `OTHER`.

## 7. Số và các module chuẩn: `Naturals`, `Integers`, `Sequences`, `FiniteSets` (Q7)

> **Trả lời ngắn.** Các *chữ số* như `63` có sẵn trong TLA+, nhưng *tập số* như `Nat` và các phép như `+` thì không: chúng nằm trong module, và ta phải nạp bằng `EXTENDS`[^bk-nums-predef]. `Naturals` cho `+ - * ^ < > <= >= .. \div %` và `Nat`; `Integers` thêm `Int` và dấu trừ một ngôi[^bk-int]. Số của TLA+ là số toán học nguyên nghĩa, không phải chuỗi bit, nên không có tràn số[^bk-no-overflow].

**Giải thích.**

*Với service `lease-svc`.* Epoch của lease là một số tự nhiên tăng dần: mỗi lần cấp lease, `Acquire` cộng thêm 1. Hàng chờ request cũng cần phép cộng, như `Len(q) + 1`. Cả hai đều dùng `+`, vậy mà `+` không có sẵn trong ngôn ngữ lõi. Mục này trả lời hai câu hỏi: cần nạp gì để có `+`, và "số" của TLA+ giống hay khác `u64` của Rust ở đâu.

### 7.1 Nạp module bằng `EXTENDS`

Toán tử số học như `+` không built-in trong TLA+ mà được định nghĩa trong module. Các toán tử phổ biến trên số tự nhiên nằm trong module `Naturals`, và ta nạp chúng bằng `EXTENDS Naturals`. Lý do hợp lý của thiết kế này là trong một ví dụ của sách, `+` có thể được định nghĩa là cộng ma trận thay vì cộng số[^bk-extends]. Mỗi spec chỉ có một dòng `EXTENDS`, và nhiều module ngăn cách bằng dấu phẩy: `EXTENDS Integers, Sequences`[^learn-one-extends].

Có một cái bẫy dễ vấp. `Sequences` dùng phép tính trên số tự nhiên, nhưng nó dùng `LOCAL INSTANCE Naturals`, nên *không* xuất các định nghĩa đó cho module nạp `Sequences`[^bk-local-inst]. Sách giải thích là để một module chỉ muốn dùng dãy không bị kéo theo cả `Naturals`[^bk-extends-seq]. Hệ quả cho `lease-svc`: muốn viết `Len(q) + 1`, bạn phải `EXTENDS Naturals, Sequences`.

### 7.2 Các module số

`Naturals` định nghĩa `+`, `-` hai ngôi, `*`, `^`, `<`, `>`, `<=`, `>=`, `Nat`, `..`, `\div`, `%`[^bk-nat-ops]. `Integers` mở rộng `Naturals` với `Int` và dấu trừ một ngôi[^bk-int]. `Reals` thêm `Real`, `Infinity` và phép chia `/`[^bk-real-div]. `Nat` là tập con của `Int`, và `Int` là tập con của `Real`[^bk-nat-subset]. Sách còn nói chi tiết các module số "không có ý nghĩa thực tế": khi viết đặc tả, cứ giả định các toán tử có nghĩa quen thuộc[^bk-numbers-practical].

`i..j` là tập các số nguyên từ `i` tới `j`, và rỗng nếu `j < i`[^bk-dotdot]. Với `lease-svc`, `1..Len(q)` là tập chỉ số của các request đang chờ.

`\div` là chia nguyên[^learn-div]. `%` là phép lấy dư. Hai toán tử được định nghĩa sao cho với số nguyên `a` và số nguyên dương `b` thì `a % b` nằm trong `0..(b-1)` và `a = b * (a \div b) + (a % b)`[^bk-div-mod]. Tức là phần dư luôn *không âm*.

```tla
EXTENDS Naturals, Sequences, FiniteSets    \* một dòng EXTENDS duy nhất

AddU32(a, b) == (a + b) % 2^32             \* muốn mô hình tràn số u32 phải viết tường minh
Idx(s)       == 1..Len(s)                  \* tập chỉ số của dãy s
R            == 17 % 5                     \* 2
D            == 17 \div 5                  \* 3
```

### 7.3 Không có tràn số

Chú thích đầu sách nhắc rằng số không phải chuỗi bit; chẳng hạn `2^33 * 2^33` bằng `2^66`, không phải lỗi tràn[^bk-no-overflow]. Hệ quả là đặc tả mô hình số nguyên *toán học*. Với `lease-svc`, điều này có nghĩa epoch trong đặc tả không bao giờ tràn, trong khi nếu code Rust lưu epoch bằng `u32` thì nó có giới hạn. Rust thì khác (mục 10): nếu hành vi tràn hay wrap quanh là điều bạn muốn kiểm, bạn phải mô hình nó rõ ràng như `AddU32` ở trên (nhận xét của người soạn).

<details><summary>Đi sâu: module chuẩn trong repo chỉ là "giả", số học hữu hạn của TLC, và <code>Int</code> với <code>-</code></summary>

- **Module `Naturals` trong repo là định nghĩa giả.** Tệp `Naturals.tla` ở commit ghim nói rõ nó chỉ chứa định nghĩa "giả" (dummy), và mọi công cụ được kỳ vọng tự cài các toán tử đó; sách mới là nơi có module thật[^repo-naturals]. TLC cài chúng bằng Java, và các comment trong `Sequences.tla`, `FiniteSets.tla` nói tương tự[^repo-seq-override]. Sách mô tả cùng cơ chế ở phía TLC: TLC không thể tính `2 + 2` từ định nghĩa của `+` trong module `Naturals` chuẩn, mà nạp một lớp Java thay thế module đó khi gặp `EXTENDS Naturals`[^tlc-override].
- **Tập số vô hạn và TLC.** `Nat` và `Int` vô hạn nên TLC không duyệt được: `{i \in Nat : i < 4}` không đánh giá được còn `{i \in 0..5 : i < 4}` thì được[^tlc-enum]. Khi viết đặc tả để chạy TLC, hãy giới hạn bằng khoảng `a..b` với hằng số cấu hình, ví dụ `1..MaxRetries` (nhận xét của người soạn; cách đặt hằng ở chương 04).
- **`Integers` thêm gì.** Trong repo, `Integers.tla` mở rộng `Naturals` bằng `Int` và định nghĩa dấu trừ một ngôi là `-. a == 0 - a`[^repo-integers]. `Naturals` thì chỉ có dấu trừ hai ngôi[^sum-nat-minus]. Nhận xét của người soạn: cần dấu trừ một ngôi như `-1` thì phải `EXTENDS Integers`.
- **Số thực.** `Reals` thêm `/` và `Infinity`[^bk-real-div]. Learn TLA+ nói TLA+ không có kiểu số thực dấu phẩy động vì ngữ nghĩa của nó quá phức tạp, và khuyên trừu tượng hóa nó đi[^learn-no-float]. Chương này không đi sâu vào số thực.
- **Peano.** Sách định nghĩa `Nat` bằng tiên đề Peano trong module `Peano`, tách riêng vì tuple và chuỗi được định nghĩa qua số tự nhiên[^bk-peano].

</details>

**Quay lại Q7:** tập số và phép tính nằm trong module (`Naturals`, `Integers`) phải `EXTENDS`; số là số toán học không tràn, và `Nat` vô hạn nên khi chạy TLC phải thay bằng khoảng hữu hạn. Với `lease-svc`, epoch và `Len(q) + 1` chỉ chạy được sau khi nạp `Naturals`.

## 8. Không có kiểu: "biểu thức ngớ ngẩn" và cách TLC đánh giá (Q8)

> **Trả lời ngắn.** TLA+ không có hệ thống kiểu: mọi biểu thức đúng cú pháp đều có một nghĩa, kể cả `3/"abc"`, mà sách gọi là *biểu thức ngớ ngẩn* (silly)[^bk-silly]. Một công thức đúng có thể chứa biểu thức ngớ ngẩn, nhưng chân trị của nó không được phụ thuộc vào nghĩa của biểu thức ấy[^bk-silly2]. "Kiểu" chỉ là một invariant bình thường (`x \in S`) mà bạn tự phát biểu và để công cụ kiểm tra[^bk-type-meaning]. TLC đánh giá `/\`, `\/` từ trái sang phải[^tlc-left-right]. Nó cũng báo lỗi khi đụng biểu thức ngớ ngẩn[^bk-silly-tlc].

**Giải thích.**

*Với service `lease-svc`.* Trong Rust, trình biên dịch từ chối `epoch + "abc"` trước khi chương trình chạy (nhận xét của người soạn). TLA+ thì không có bước đó: `epoch + "abc"` đúng cú pháp, nên nó có một nghĩa nào đó, chỉ là ta không biết nghĩa gì. Vậy ai bắt lỗi trong đặc tả `lease-svc`? Câu trả lời gồm hai phần: bạn tự viết một invariant nói "mọi biến có dạng gì" (thường tên là `TypeOK`), và TLC kiểm nó trên mọi trạng thái, đồng thời báo lỗi khi đánh giá phải một biểu thức ngớ ngẩn. Phần còn lại của mục này giải thích hai cơ chế đó.

### 8.1 Vì sao không có kiểu

Sách trả lời thẳng. Ngôn ngữ lập trình có kiểu vì kiểu giúp trình biên dịch sinh code hiệu quả và bắt lỗi, và lợi ích thắng chi phí. Với việc viết đặc tả, tác giả thấy chi phí lớn hơn lợi ích[^bk-types-cost]. Toán học thông thường cũng không có kiểu. Biểu thức `3/0`, thứ các nhà toán học vẫn ngầm viết ra mỗi ngày, thực ra không "ngớ ngẩn" hơn `3/"abc"`[^bk-silly]. Vấn đề là một quy tắc cú pháp không thể cấm `3/0` mà không cấm luôn cả những biểu thức hoàn toàn hợp lý.

### 8.2 Kiểu là một invariant

Sách định nghĩa: biến `v` có kiểu `T` trong đặc tả `Spec` khi `v \in T` là một invariant của `Spec`[^bk-type-def]. "Kiểu" chỉ là tên của một tính chất bất biến chứ không phải địa vị đặc biệt của ngôn ngữ[^bk-type-meaning]. Ví dụ đồng hồ giờ trong sách có invariant tên `TypeInvariant` (và `TypeOK`); vì sao nó là *tính chất* chứ không phải giả định được giải thích trọn ở [Chương 03, mục 6](03-trang-thai-hanh-vi-action.md), còn cách kiểm bằng TLC ở [chương 04](04-invariant-va-tlc.md). Với `lease-svc`, `TypeOK` sẽ như sau.

```tla
TypeOK ==
    /\ queue  \in Seq(Request)
    /\ leases \in [Key -> Lease]
    /\ status \in [Node -> {"up", "down"}]
```

Các biến `queue`, `leases`, `status` và các tập `Request`, `Key`, `Lease`, `Node` ở đây là giả định; chương 03 dạy khai báo, còn `Lease` được định nghĩa ở mục 9.

### 8.3 Lỗi cú pháp và lỗi ngữ nghĩa

Một lỗi là lỗi cú pháp hay lỗi ngữ nghĩa quyết định công cụ nào bắt nó. Bộ phân tích cú pháp bắt lỗi cú pháp, ví dụ `Tail(s, t)` sai số đối số, nhưng không bắt lỗi "ngớ ngẩn"[^bk-parser]. TLC mới là nơi báo lỗi khi cố đánh giá một biểu thức ngớ ngẩn[^bk-silly-tlc].

### 8.4 Thứ tự đánh giá của TLC

TLC đánh giá từ trái sang phải. Với `p /\ q`, nó tính `p` trước và chỉ tính `q` nếu `p` đúng. Với `p \/ q`, nó tính `p` trước và chỉ tính `q` nếu `p` sai. Còn `p => q` được tính như `~p \/ q`[^tlc-left-right]. Hệ quả thực hành: **đặt điều kiện bảo vệ (guard) lên trước**. Với `lease-svc`, "request đầu hàng chờ là xin lease" phải viết là "hàng chờ khác rỗng `/\` `Head(q)` là xin lease", chứ không đảo lại (nhận xét của người soạn). Hai dòng sau cho thấy khác biệt trên một ví dụ tối giản.

```tla
HeadIsZero(x) == (x # <<>>) /\ (x[1] = 0)   \* an toàn: vế trái chặn vế phải
Bad(x)        == (x[1] = 0) /\ (x # <<>>)   \* logic tương đương, nhưng TLC lỗi khi x = <<>>
```

Sách đưa chính ví dụ này: TLC đánh giá được công thức thứ nhất nhưng không đánh giá được công thức thứ hai khi `x = <<>>`, vì `<<>>[1]` là ngớ ngẩn[^bk-seq-silly]. Sách cũng ghi nhận rằng ta tự nhiên viết công thức thứ nhất vì nó dễ hiểu hơn[^tlc-natural-order].

### 8.5 TLC chỉ duyệt được tập hữu hạn

TLC đánh giá `\E x \in S : p` bằng cách duyệt từng phần tử của `S`, và báo lỗi nếu tập không "hiển nhiên hữu hạn"[^tlc-giveup]. Các dạng chỉ đánh giá được khi duyệt được `S` gồm `\E`, `\A`, `CHOOSE`, `{x \in S : p}`, `{e : x \in S}`, `SUBSET S`, `UNION S` và `[x \in S |-> e]`[^tlc-forms]. Một biểu thức tập chỉ đánh giá được nếu nó bằng một tập hữu hạn, và biểu thức hàm chỉ đánh giá được nếu miền của nó hữu hạn[^tlc-value]. Dạng không chặn `\E x : p`, `\A x : p`, `CHOOSE x : p` thì TLC không đánh giá được[^tlc-unbounded]. Dù vậy TLC vẫn tính được một số biểu thức dù không tính nổi mọi biểu thức con của nó, ví dụ `[n \in Nat |-> n * (n + 1)][3]`[^tlc-partial].

<details><summary>Đi sâu: ba cách diễn giải toán tử Boolean cho giá trị phi-Boolean</summary>

Vì không có kiểu, `2 /\ <<5>>` là biểu thức hợp lệ. Sách phân biệt ba cách diễn giải:

- *Bảo thủ*: giá trị hoàn toàn không xác định, nên các luật logic như giao hoán chỉ đúng với Boolean.
- *Tự do*: giá trị là một Boolean nào đó, nên mọi luật logic đều đúng[^bk-liberal].
- *Vừa phải*: chỉ giả định các biểu thức có `FALSE`/`TRUE` cho giá trị mong đợi, như `FALSE => 2` bằng `TRUE`.

Ngữ nghĩa của TLA+ khẳng định các luật của cách diễn giải vừa phải là đúng. Cách tự do không bắt buộc cũng không bị cấm, và bạn nên viết đặc tả có nghĩa dưới cách vừa phải[^bk-moderate]. Một ví dụ cho thấy khác biệt: `\A n \in Nat : tnat[n]` với `tnat == [n \in Nat |-> TRUE]` bằng `TRUE` theo cách tự do nhưng không bằng theo cách bảo thủ[^bk-tnat]. Theo cách vừa phải thì nó bằng `TRUE`[^bk-tnat-mod]. Với cách bảo thủ, sách đề nghị viết `tnat[n] = TRUE` (mục 16.1.3)[^bk-tnat-safe]. Nhận xét của người soạn: quy tắc thực hành là *đừng dùng giá trị không phải Boolean làm Boolean*.

</details>

**Quay lại Q8:** không có kiểu nghĩa là mọi lỗi "dùng sai kiểu" chỉ lộ ra khi TLC đánh giá; hãy phát biểu kiểu thành invariant `TypeOK`, đặt guard trước các biểu thức có thể ngớ ngẩn, và chỉ đưa tập hữu hạn cho TLC. Với `lease-svc`, đó là `TypeOK` cho `queue`, `leases`, `status` và guard `q # <<>>` trước `Head(q)`.

## 9. Ghép lại: một bảng lease nhỏ (mục phụ trợ)

Ví dụ này không dạy khái niệm mới; nó ghép mọi mảnh của chương vào một module (minh họa cú pháp, chưa chạy). Đây chính là lõi của `lease-svc` (nhận xét của người soạn): mỗi khóa tài nguyên có một chủ sở hữu hiện tại và một số epoch tăng mỗi lần cấp lại.

```tla
---- MODULE LeaseMath ----
EXTENDS Naturals, FiniteSets

CONSTANTS Node, Key

NoOwner == CHOOSE v : v \notin Node            \* "chưa có chủ": một giá trị không thuộc Node

Lease  == [owner : Node \cup {NoOwner}, epoch : Nat]   \* tập các record
Leases == [Key -> Lease]                               \* tập các hàm: bảng lease

InitLeases == [k \in Key |-> [owner |-> NoOwner, epoch |-> 0]]   \* hàm trả về record

Free(L)    == {k \in Key : L[k].owner = NoOwner}       \* lọc
Held(L, n) == {k \in Key : L[k].owner = n}

Acquire(L, k, n) ==                                    \* trả về bảng mới, không sửa L
    IF L[k].owner = NoOwner
       THEN [L EXCEPT ![k].owner = n, ![k].epoch = @ + 1]
       ELSE L

Release(L, k) == [L EXCEPT ![k].owner = NoOwner]

\* Một phát biểu ở dạng "với mọi": sẽ trở thành INVARIANT ở chương 04
Quota(L) == \A n \in Node : Cardinality(Held(L, n)) <= 2
====
```

Đọc từng phần như sau. `Lease` và `Leases` là *tập* (record và hàm), còn `InitLeases` là *giá trị* thuộc `Leases`. `Acquire` dùng `IF` và `EXCEPT` với đường dẫn lồng `![k].owner` và `@`. `Quota` dùng `\A`, lọc `{k \in Key : ...}` và `Cardinality` từ `FiniteSets`. Chú ý `Acquire` trên khóa đang có chủ trả lại `L` nguyên vẹn: đây là *toán tử trên giá trị*, không phải hành động. Việc "bước này được phép xảy ra khi nào" (guard, `Next`) là chuyện của chương 03.

Khi chạy TLC có một lưu ý: `NoOwner == CHOOSE v : v \notin Node` là `CHOOSE` không chặn, mà TLC không đánh giá được[^tlc-unbounded]. Cách xử lý thường là ghi đè `NoOwner` bằng một giá trị mô hình trong file cấu hình, như sách làm với `NotAnS`[^tlc-notans] (chương 04).

## 10. Liên hệ với Rust và backend

Đây là nơi bạn sẽ nối đặc tả với code Rust sau này ([chương 10](10-tu-dac-ta-den-code-rust.md)). Bảng dưới là *gần đúng có chủ đích*; cột cuối ghi chỗ sai lệch.

| TLA+ | Rust gần nhất | Khác biệt cần nhớ |
|---|---|---|
| Tập hữu hạn `{...}` | `HashSet<T>` / `BTreeSet<T>` | TLA+ so sánh tập theo phần tử; tập lồng được; không có "bộ nhớ" |
| `\cup`, `\cap`, `\`, `\subseteq` | `union`, `intersection`, `difference` của `HashSet`[^rs-hs-union] | Rust trả iterator (trừ `is_subset`[^rs-hs-subset] trả `bool`); TLA+ trả tập |
| `{x \in S : P}` / `{e : x \in S}` | `iter().filter(..).collect()` / `.map(..).collect::<HashSet<_>>()` | TLA+ tự loại trùng; không có thứ tự duyệt |
| `\A` / `\E` trên tập | `Iterator::all` / `Iterator::any` | Trên rỗng `all` trả `true`[^rs-all-empty]. `any` trả `false`[^rs-any-empty]. Cả hai khớp `\A`/`\E` trên `{}` |
| `CHOOSE x \in S : P` | `find(P)` trả `Option`[^rs-find] | Rust có `None`; TLA+ cho giá trị không xác định và TLC báo lỗi |
| Hàm hữu hạn `[S -> T]` | `HashMap<K,V>` / `BTreeMap<K,V>` | Xem bên dưới |
| `f[x]` ngoài `DOMAIN f` | `map[&k]` panic nếu thiếu khóa[^rs-hm-index]; `get` trả `Option` | "ngớ ngẩn" chứ không panic; TLC lỗi |
| `[f EXCEPT ![k] = v]` | `HashMap::insert` sửa tại chỗ, trả giá trị cũ[^rs-hm-insert] | `EXCEPT` là *giá trị mới*, không thêm khóa |
| Sequence `<<a, b>>` | `Vec<T>` / `VecDeque<T>` | Chỉ số từ 1; `Vec` chỉ số từ 0 và ra ngoài biên thì panic[^rs-vec-index]; `Head(<<>>)` ngớ ngẩn |
| Record `[a \|-> 1]` | `struct` | Xem bên dưới |
| Tuple `<<a, b>>` | tuple `(A, B)` — dãy hữu hạn, không đồng nhất[^rs-tuple] | TLA+ tuple chính là sequence, chỉ số từ 1 |
| `CASE` | `match` | `match` chọn nhánh đầu tiên khớp[^rs-match]; `CASE` chọn nhánh bất kỳ trong các nhánh đúng |
| `Nat`, `Int` | `u64`, `i64` | Không tràn số; với `b > 0`, `%` của TLA+ cho dư không âm trong `0..(b-1)`, cùng hệ thức với `rem_euclid`[^rs-rem-euclid] |
| `TypeOK` | hệ thống kiểu | Kiểu của TLA+ là *tập*, kiểm bằng TLC |

**`HashMap` ≈ hàm, `struct` ≈ record (nhận xét của người soạn).** Cả hai tương ứng đủ dùng để dịch đặc tả `lease-svc` sang code, nhưng có bốn chỗ lệch.

1. *Giá trị so với vùng nhớ.* `HashMap::insert` sửa map tại chỗ và trả về giá trị cũ nếu khóa đã có[^rs-hm-insert]. Hàm TLA+ thì không bao giờ bị sửa, `EXCEPT` dựng một hàm khác. Khi dịch bước `Next` thành code, mỗi bước thường tương ứng một lần ghi vào nhiều chỗ, và thứ cần đối chiếu là trạng thái *sau* bước.
2. *Khóa thiếu.* Truy cập `HashMap` bằng `[]` với khóa thiếu thì panic[^rs-hm-index]. Trong TLA+, `f[x]` ngoài miền là biểu thức ngớ ngẩn (không xác định, TLC lỗi). Nếu đặc tả của bạn "vô tình" dựa vào miền của hàm, code Rust sẽ cần `get`/`Option` tường minh.
3. *Struct có kiểu cố định.* Sách Rust nói struct đặt tên cho từng phần dữ liệu để rõ nghĩa, khác với tuple[^rs-struct]. Đó đúng là lý do record tồn tại trong TLA+[^bk-record-why]. Nhưng record TLA+ không có kiểu và còn là hàm: bạn duyệt được `DOMAIN r` và ghép record bằng toán tử như `Override` ở mục 4, còn struct Rust thì không.
4. *Cập nhật một phần.* Cú pháp cập nhật struct của Rust (tạo instance mới từ instance khác, đổi vài trường)[^rs-struct-update] là họ hàng gần nhất của `EXCEPT` cho record.

Ngoài ra, filter và map của tập tương ứng iterator adapter ([P9·06 — iterator và closure](../09-rust-language/06-iterator-va-closure.md)). `HashMap` bên trong xem [P10·09 — collections internals](../10-rust-internals/09-collections-internals.md). Số học Rust tràn thì panic ở debug và wrap ở release[^rs-overflow]. Điều này đối lập với số toán học của TLA+[^bk-no-overflow]. Trong hệ thống dữ liệu, "bảng ≈ tập các record" và "trạng thái replica ≈ hàm từ node sang trạng thái" (nhận xét của người soạn); xem ví dụ ở [P12·05 — replication](../12-he-thong-du-lieu/05-replication.md) và [P12·07 — transaction](../12-he-thong-du-lieu/07-transaction.md), những cấu trúc mà [Chương 08](08-dac-ta-giao-thuc-phan-tan.md) và [Chương 09](09-mo-hinh-hoa-he-backend.md) của phase này sẽ đặc tả.

## Tóm tắt

- Logic: `/\`, `\/`, `~`, `=>`, `<=>`; `\A`/`\E` chạy trên tập; `=>` chỉ sai khi vế trái đúng mà vế phải sai; bullet `/\`/`\/` thẳng cột thay ngoặc.
- Tập: `\in`, `\cup`, `\cap`, `\`, `\subseteq`, `SUBSET`, `UNION`, `\X`; dựng tập bằng lọc `{x \in S : P}` và ánh xạ `{e : x \in S}`, luôn từ một tập nguồn.
- Hàm: `[x \in S |-> e]`, `DOMAIN f`, `[S -> T]`, `[f EXCEPT ![c] = e]` sinh hàm mới (không thêm khóa); thứ ánh xạ nằm trong biến (như bảng lease của `lease-svc`) được viết bằng hàm.
- Record, tuple, sequence đều là hàm (miền: tập chuỗi, `1..n`, `1..n`); `Seq`, `Len`, `Head`, `Tail`, `Append`, `\o`; chỉ số từ 1.
- `CHOOSE` là một giá trị xác định nhưng không biết là gì, không phải phi tất định; phi tất định viết bằng `\in`/`\E`.
- `==` là đồng nghĩa, `LET`/`IN` định nghĩa cục bộ, `IF` luôn có `ELSE`, `CASE` chọn nhánh bất kỳ trong các nhánh đúng nên các nhánh nên rời nhau.
- Số và tập số nằm trong module `Naturals`/`Integers`; số là số toán học (không tràn), `Nat` vô hạn nên TLC cần khoảng hữu hạn.
- TLA+ không có kiểu: `TypeOK` là invariant; biểu thức ngớ ngẩn chỉ lộ khi TLC đánh giá, nên đặt guard lên trước.
- TLC đánh giá trái sang phải và chỉ duyệt được tập hữu hạn; các dạng `\E x : p`, `CHOOSE x : p` không chặn không đánh giá được.
- Rust: `HashSet`/`HashMap`/`struct`/`Vec` là bản "có vùng nhớ" của tập/hàm/record/sequence; khác biệt chính là cập nhật tại chỗ, khóa thiếu, chỉ số 0/1 và tràn số.

## Câu hỏi tự kiểm tra

1. `~A /\ B => C` được hiểu có ngoặc thế nào? Vì sao nên thêm ngoặc dù hiểu đúng?
2. Giá trị của `\E x \in {} : TRUE` và `\A x \in {} : FALSE` là gì, vì sao?
3. `{1, 2} = {2, 1, 1}` đúng hay sai? `{n \in 1..6 : n % 2 = 0}` và `{2 * n : n \in 1..3}` có bằng nhau không?
4. `SUBSET {1, 2}` gồm những phần tử nào? `UNION {{1}, {1, 2}}` bằng gì?
5. Với `f == [x \in {1, 2} |-> x * 10]`, `DOMAIN f` là gì? `[f EXCEPT ![3] = 0]` bằng gì và TLC làm gì?
6. Vì sao `[S -> T]` không đòi hàm phủ hết `T`? Có bao nhiêu hàm trong `[1..2 -> {"a", "b", "c"}]`?
7. Record `[a |-> 1]` và hàm `[x \in {"a"} |-> 1]` có bằng nhau không? `r.a` viết tắt của gì?
8. `Head(<<>>)` và `Tail(<<>>)` cho kết quả gì theo sách, theo module hiện hành, và theo TLC?
9. `CHOOSE n : n \in Nat` có "ngẫu nhiên" không? Hai đặc tả `x = CHOOSE...` và `x \in Nat` khác nhau thế nào?
10. Cho `CASE score >= 90 -> "A" [] score >= 80 -> "B" [] OTHER -> "C"`, với `score = 95` kết quả thế nào, và vì sao?
11. Vì sao `(x # <<>>) /\ (x[1] = 0)` chạy được trong TLC nhưng chiều ngược lại thì không?
12. Vì sao TLC không đánh giá được `{i \in Nat : i < 4}`, và sửa thế nào?

<details><summary>Đáp án</summary>

1. `((~A) /\ B) => C`: `~` ràng chặt hơn `/\`, và `/\` ràng chặt hơn `=>`[^bk-prec]. Nên thêm ngoặc vì sách khuyên cứ ngoặc khi nghi ngờ[^bk-parens].
2. `FALSE` và `TRUE`. Trên tập rỗng không có phần tử nào nên `\E` luôn sai[^bk-empty]. Theo đối ngẫu thì `\A` luôn đúng[^bk-dual].
3. Đúng: tập chỉ phụ thuộc phần tử, không phụ thuộc thứ tự hay số lần lặp[^bk-set-same]. Cả hai tập đều là `{2, 4, 6}` (tính theo định nghĩa lọc và ánh xạ[^bk-filter]) nên bằng nhau.
4. `{{}, {1}, {2}, {1, 2}}`[^bk-subset-ex]. `UNION {{1}, {1, 2}}` = `{1, 2}` (hợp mọi phần tử, theo định nghĩa[^sum-union]).
5. `{1, 2}`[^bk-fn-def]. `[f EXCEPT ![3] = 0]` chính là `f` vì 3 không thuộc miền[^bk-except-nondomain]. TLC cảnh báo biểu thức kiểu này[^tlc-nowarning].
6. Vì `[S -> T]` là tập hàm có miền bằng `S` và miền giá trị là tập con *bất kỳ* của `T`[^bk-fn-set]. Có $3^2 = 9$ hàm (công thức $|B|^{|A|}$[^learn-fnset-size]).
7. Bằng nhau: record là hàm có miền là tập hữu hạn các chuỗi, và `r.a` là viết tắt của `r["a"]`[^bk-record-def].
8. Theo sách, `Tail(<<>>)` là dãy rỗng (suy ra từ định nghĩa), còn `Head(<<>>)` là `<<>>[1]`, một biểu thức ngớ ngẩn. Theo module hiện hành, `Tail` dùng `CASE s # << >>` nên `Tail(<<>>)` không xác định[^repo-tail]. TLC báo lỗi khi đánh giá `x[1]` với `x = <<>>`[^bk-seq-silly].
9. Không: trong toán học không có toán tử phi tất định, biểu thức luôn bằng cùng một giá trị[^bk-choose-nondet]. Đặc tả `x = CHOOSE...` cho đúng một hành vi (x luôn bằng một số cố định), còn `x \in Nat` cho vô số hành vi[^bk-choose-vs-in].
10. Nhánh 1 và 2 cùng đúng, nên kết quả là `"A"` hoặc `"B"` mà ngữ nghĩa không nói là cái nào[^bk-case-disjoint]. Muốn xác định, các nhánh phải rời nhau (ví dụ nhánh thứ hai thêm `score < 90`).
11. TLC đánh giá `/\` từ trái sang phải, chỉ tính vế phải nếu vế trái đúng[^tlc-left-right]. Chiều ngược lại tính `<<>>[1]`, một biểu thức ngớ ngẩn, trước[^bk-seq-silly].
12. Vì `Nat` vô hạn: TLC đánh giá `{x \in S : p}` bằng cách duyệt `S` trước, nên không duyệt được `Nat`[^tlc-enum]. Sửa: dùng tập hữu hạn như `0..5` (`{i \in 0..5 : i < 4}`).

</details>

## Đọc thêm

- Lamport, [*Specifying Systems*](https://lamport.azurewebsites.net/tla/book-02-08-08.pdf), **chương 1** (logic mệnh đề, tập hợp, lượng từ): đọc để nắm nền toán, mục 1.1–1.3.
- *Specifying Systems*, **mục 5.2 và chương 6** ("Some More Math": hàm, `CHOOSE`, đệ quy, hàm so với toán tử): đọc để hiểu sâu mục 3 và 5 của chương này.
- *Specifying Systems*, **chương 16** ("The Operators of TLA+", mục 16.1): ngữ nghĩa hình thức của mọi toán tử hằng — nguồn chuẩn khi cần tra hành vi chính xác.
- *Specifying Systems*, **mục 15.2.2** ("Alignment") và **mục 14.2.2** ("How TLC Evaluates Expressions"): luật căn lề bullet và thứ tự đánh giá của TLC.
- *Specifying Systems*, **mục 18.1 và 18.4**: module `Sequences` và các module số.
- Lamport, [bản tóm tắt TLA+](https://lamport.azurewebsites.net/tla/summary-standalone.pdf): bảng một trang mọi toán tử và cách gõ ASCII.
- [Learn TLA+ — Operators and Values](https://learntla.com/core/operators.html) và [Structured Data](https://learntla.com/core/functions.html) (trang sống): giải thích thân thiện, ví dụ chạy được trong "scratchfile"; dùng cho cú pháp, nhưng đối chiếu với sách khi cần độ chính xác.
- [`tlaplus/tlaplus`, thư mục `StandardModules`](https://github.com/tlaplus/tlaplus/tree/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/src/tla2sany/StandardModules) (ghim commit): định nghĩa module chuẩn hiện hành, đặc biệt `Sequences.tla`.
- Chương kế: [Chương 03 — Trạng thái, hành vi, action](03-trang-thai-hanh-vi-action.md) (dùng hàm và record làm trạng thái); [Chương 04 — Invariant và TLC](04-invariant-va-tlc.md) (kiểm `TypeOK`, giới hạn tập khi chạy TLC); [Chương 06 — PlusCal](06-pluscal.md).

[^aws-math]: Newcombe et al., Use of Formal Methods at Amazon Web Services (2014), phần "Precise Designs" — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf> — "TLA+ is based on simple discrete math, i.e. basic set theory and predicates, with which all engineers are familiar."
[^tla-home]: Lamport, The TLA+ Home Page — <https://lamport.azurewebsites.net/tla/tla.html> — "It's based on the idea that the best way to describe things precisely is with simple mathematics."
[^bk-prop-ops]: Lamport, Specifying Systems, mục 1.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Propositional logic is the mathematics of the two Boolean values true and false and the five operators"
[^bk-forall]: Lamport, Specifying Systems, mục 1.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The formula ∀ x ∈ S : F asserts that formula F is true for every element x in the set S ."
[^bk-exists]: Lamport, Specifying Systems, mục 1.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The formula ∃ x ∈ S : F asserts that formula F is true for at least one element x in S ."
[^bk-imp-table]: Lamport, Specifying Systems, mục 1.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "F ⇒ G equals true iff F equals false or G equals true (or both)."
[^bk-imp-why]: Lamport, Specifying Systems, mục 1.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "We expect that if n is greater than 3, then it should be greater than 1, so n > 3 should imply n > 1."
[^bk-imp-or]: Lamport, Specifying Systems, mục 1.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "For example, here is the truth table showing that (F ⇒ G) ≡ (¬F ∨ G) is a tautology."
[^bk-equiv]: Lamport, Specifying Systems, mục 1.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The equivalence operator ≡ is equality for Booleans. We can replace ≡ by =, but not vice versa."
[^bk-prec]: Lamport, Specifying Systems, mục 1.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "has higher precedence than ∧ and ∨, which have higher precedence than ⇒ and ≡, so ¬F ∧ G ⇒ H means ((¬F ) ∧ G) ⇒ H"
[^bk-parens]: Lamport, Specifying Systems, mục 1.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "If you have the slightest doubt about whether parentheses are needed, use them."
[^bk-dual]: Lamport, Specifying Systems, mục 1.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Formula F is true for some x in S iff F is not false for all x in S"
[^bk-empty]: Lamport, Specifying Systems, mục 1.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Since there exists no element in the empty set, the formula ∃ x ∈ {} : F is false for every formula F ."
[^bk-conj-disj]: Lamport, Specifying Systems, mục 1.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Thinking of ∀ as conjunction and ∃ as disjunction can help."
[^bk-quant-nest]: Lamport, Specifying Systems, mục 1.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "∀ x ∈ S , y ∈ T : F means ∀ x ∈ S : (∀ y ∈ T : F )"
[^bk-bounded]: Lamport, Specifying Systems, mục 1.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Whenever possible, it is better to use bounded than unbounded quantification in a specification."
[^bk-ascii-kw]: Lamport, Specifying Systems, mục 2.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Reserved words that appear in small upper-case letters (like extends) are written in ascii with ordinary upper-case letters."
[^learn-bool]: Hillel Wayne, Learn TLA+, Operators and Values (trang sống) — <https://learntla.com/core/operators.html> — "The booleans are TRUE and FALSE."
[^sum-ascii-and]: Lamport, Summary of TLA+, ASCII Representation of Typeset Symbols — <https://lamport.azurewebsites.net/tla/summary-standalone.pdf> — "/\ or \land ~ or \lnot or \neg \in"
[^sum-ascii-or]: Lamport, Summary of TLA+, ASCII Representation of Typeset Symbols — <https://lamport.azurewebsites.net/tla/summary-standalone.pdf> — "\/ or \lor <=> or \equiv \notin"
[^repo-isfinite]: tlaplus/tlaplus @94d0c50, StandardModules/FiniteSets.tla — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/src/tla2sany/StandardModules/FiniteSets.tla> — "\E seq \in Seq(S) : \A s \in S : \E n \in 1..Len(seq) : seq[n] = s"
[^bk-bullet]: Lamport, Specifying Systems, mục 3.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Each conjunct begins with a ∧ and must lie completely to the right of that ∧."
[^bk-bullet-align]: Lamport, Specifying Systems, mục 3.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "When using this bulleted-list notation, the ∧’s or ∨’s must line up precisely (even in the ascii input)."
[^bk-bullet-nest]: Lamport, Specifying Systems, mục 3.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Because the indentation is significant, we can eliminate parentheses, making this notation especially useful when conjunctions and disjunctions are nested."
[^bk-indent-ignored]: Lamport, Specifying Systems, mục 15.2.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "As these examples show, indentation is ignored—except in conjunction and disjunction lists, discussed below."
[^learn-bullet]: Hillel Wayne, Learn TLA+, Operators and Values (trang sống) — <https://learntla.com/core/operators.html> — "This is also the only place in the language where whitespace matters."
[^bk-align-1]: Lamport, Specifying Systems, mục 15.2.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Another /\ whose / character is in column c and is the first nonspace character on the line."
[^bk-align-2]: Lamport, Specifying Systems, mục 15.2.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Any nonspace character in column c or a column to the left of column c."
[^bk-align-bad]: Lamport, Specifying Systems, mục 15.2.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "In the second example, ∧ x 0 is interpreted as a conjunction list containing only one conjunct, and the second /\ is interpreted as an infix operator."
[^bk-undelim]: Lamport, Specifying Systems, mục 15.2.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "TLA+ has several expression-making constructs with no explicit right-hand terminator. They are: choose, if/then/else, case, let/in, and quantifier constructs."
[^bk-tlc-calc]: Lamport, Specifying Systems, mục 1.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Chapter 14 explains, on page 261, how to use the TLC model checker to verify propositional logic tautologies and to perform other TLA+ calculations."
[^learn-truth]: Hillel Wayne, Learn TLA+, Structured Data (trang sống) — <https://learntla.com/core/functions.html> — "TruthTable == [p, q \in BOOLEAN |-> p => q]"
[^bk-set-undef]: Lamport, Specifying Systems, mục 1.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "We take as undefined concepts the notion of a set and the relation ∈"
[^bk-set-same]: Lamport, Specifying Systems, mục 1.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Thus, {0, 1, 2} and {2, 1, 0} and {0, 0, 1, 2, 2} are all the same set—the unique set containing the three elements 0, 1, and 2."
[^bk-cap]: Lamport, Specifying Systems, mục 1.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "S ∩ T The set of elements in both S and T ."
[^bk-cup]: Lamport, Specifying Systems, mục 1.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "S ∪ T The set of elements in S or T (or both)."
[^bk-subseteq]: Lamport, Specifying Systems, mục 1.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "S ⊆ T True iff every element of S is an element of T ."
[^bk-diff]: Lamport, Specifying Systems, mục 1.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The set of elements in S that are not in T ."
[^sum-subset]: Lamport, Summary of TLA+, The Constant Operators (Sets) — <https://lamport.azurewebsites.net/tla/summary-standalone.pdf> — "[Set of subsets of S ]"
[^sum-union]: Lamport, Summary of TLA+, The Constant Operators (Sets) — <https://lamport.azurewebsites.net/tla/summary-standalone.pdf> — "[Union of all elements of S ]"
[^sum-ascii-cup]: Lamport, Summary of TLA+, ASCII Representation of Typeset Symbols — <https://lamport.azurewebsites.net/tla/summary-standalone.pdf> — "\cup or \union t \sqcup ] \uplus"
[^sum-ascii-cap]: Lamport, Summary of TLA+, ASCII Representation of Typeset Symbols — <https://lamport.azurewebsites.net/tla/summary-standalone.pdf> — "\cap or \intersect \sqcap (+) or \oplus"
[^bk-union-ex]: Lamport, Specifying Systems, mục 6.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "union {{1, 2}, {2, 3}, {3, 4}} = {1, 2, 3, 4}"
[^bk-subset-ex]: Lamport, Specifying Systems, mục 6.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "subset {1, 2} = {{}, {1}, {2}, {1, 2}}"
[^bk-filter]: Lamport, Specifying Systems, mục 6.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The subset of S consisting of all elements x satisfying property p."
[^bk-map]: Lamport, Specifying Systems, mục 6.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The set of elements of the form e, for all x in the set S ."
[^learn-colon]: Hillel Wayne, Learn TLA+, Operators and Values (trang sống) — <https://learntla.com/core/operators.html> — "I’ve found that the best way to remember which is which is by reading the colon as a “where”."
[^bk-set-tuples]: Lamport, Specifying Systems, mục 6.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "In the construct {x ∈ S : P }, we can let x be a tuple."
[^bk-cardinality]: Lamport, Specifying Systems, mục 6.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "All the set operators we’ve seen so far are built-in operators of TLA+. There is also a standard module FiniteSets that defines two operators:"
[^bk-russell]: Lamport, Specifying Systems, mục 6.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The source of the paradox is that R isn’t a set. There’s no way to write it in TLA+."
[^learn-subset-cost]: Hillel Wayne, Learn TLA+, Operators and Values (trang sống) — <https://learntla.com/core/operators.html> — "I often see beginners try to test if “S is a subset of T” by writing S \in SUBSET T. This works but is very inefficient. Write S \subseteq T instead."
[^bk-zf]: Lamport, Specifying Systems, mục 16.1.6 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "TLA+ is based on Zermelo-Fränkel set theory, in which every value is a set."
[^bk-set-sem]: Lamport, Specifying Systems, mục 16.1.6 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "is defined by ∀ x : (x ∈ (S ∪ T )) ≡ (x ∈ S ) ∨ (x ∈ T )."
[^tlc-giveup]: Lamport, Specifying Systems, mục 14.2.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "It enumerates the elements of a set S in a very straightforward way, and it gives up and declares an error if the set is not obviously finite."
[^tlc-order]: Lamport, Specifying Systems, mục 14.2.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "by enumerating the elements s 1 , . . . , s n of S in some order and then evaluating p"
[^tlc-comparable]: Lamport, Specifying Systems, mục 14.2.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "For example, strings and numbers are not comparable because the semantics of TLA+ doesn’t tell us whether or not “abc” equals 42."
[^tlc-tuple-value]: Lamport, Specifying Systems, mục 14.2.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Since tuples and records are functions, rule 3 implies that a record or tuple whose components are TLC values is a TLC value."
[^bk-product]: Lamport, Specifying Systems, mục 5.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Note that A × B × C is different from A × (B × C )"
[^learn-product]: Hillel Wayne, Learn TLA+, Operators and Values (trang sống) — <https://learntla.com/core/operators.html> — "\X is not associative: S == 1..3"
[^bk-fn-def]: Lamport, Specifying Systems, mục 5.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "A function f has a domain, written domain f , and it assigns to each element x of its domain the value f [x ]."
[^bk-fn-set]: Lamport, Specifying Systems, mục 5.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "For any sets S and T , the set of all functions whose domain equals S and whose range is any subset of T is written [S → T ]."
[^bk-except-def]: Lamport, Specifying Systems, mục 16.1.7 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The except construct describes a function that is “almost the same as” another function."
[^bk-fn-array]: Lamport, Specifying Systems, mục 5.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Functions correspond to the arrays of programming languages. The domain of a function corresponds to the index set of an array."
[^bk-fn-eq]: Lamport, Specifying Systems, mục 5.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Two functions f and g are equal iff they have the same domain and f [x ] = g[x ] for all x in their domain."
[^bk-fn-value]: Lamport, Specifying Systems, mục 16.1.7 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "and the value of f [v ] is specified only if v is an element of domain f ."
[^sum-fn]: Lamport, Summary of TLA+, The Constant Operators (Functions) — <https://lamport.azurewebsites.net/tla/summary-standalone.pdf> — "[Function f such that f [x ] = e for x ∈ S ]"
[^bk-succ]: Lamport, Specifying Systems, mục 5.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "defines succ to be the successor function on the natural numbers—the function with domain Nat such that succ[n] = n + 1 for all n ∈ Nat."
[^bk-fn-array2]: Lamport, Specifying Systems, mục 5.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "In a programming language, such an assignment is called an array of type Val indexed by Adr . In mathematics, it’s called a function from Adr to Val ."
[^learn-fnset-size]: Hillel Wayne, Learn TLA+, Structured Data (trang sống) — <https://learntla.com/core/functions.html> — "A function set of form [A -> B] will have \(\#B^{\#A}\) elements in it."
[^bk-except-array]: Lamport, Specifying Systems, mục 5.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Function [f except ![c] = e] corresponds to the array obtained from f by assigning e to f [c]."
[^bk-except-at]: Lamport, Specifying Systems, mục 5.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "As in the except construct for records, the expression e in [f except ![c] = e] can contain the symbol @, where it means f [c]."
[^bk-except-nested]: Lamport, Specifying Systems, mục 5.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "TLA+ defines [f except ![c][d ] = e] to be the function corresponding to the array obtained by assigning e to f [c][d ]."
[^learn-status]: Hillel Wayne, Learn TLA+, Structured Data (trang sống) — <https://learntla.com/core/functions.html> — "We have a set of servers, which can have one of three states."
[^bk-except-nondomain]: Lamport, Specifying Systems, mục 16.1.7 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Hence, if neither u nor v is in the domain of f , then (16.7) equals f ."
[^tlc-nowarning]: Lamport, Specifying Systems, mục 14.5.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "the expression [f except ![v ] = e] is probably incorrect if v is not an element of the domain of f . (In this case, the expression just equals f .) TLC normally issues a warning when it encounters such an unlikely expression"
[^repo-tlc-ops]: tlaplus/tlaplus @94d0c50, StandardModules/TLC.tla — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/src/tla2sany/StandardModules/TLC.tla> — "[x \in (DOMAIN f) \cup (DOMAIN g) |-> IF x \in DOMAIN f THEN f[x] ELSE g[x]]"
[^learn-tlc-ops]: Hillel Wayne, Learn TLA+, Structured Data (trang sống) — <https://learntla.com/core/functions.html> — "@@ and :> are only available in your spec if you extend TLC."
[^bk-multi-arg]: Lamport, Specifying Systems, mục 5.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "In TLA+, as in ordinary mathematics, a function of multiple arguments is one whose domain is a set of tuples."
[^bk-multi-prefer]: Lamport, Specifying Systems, mục 5.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "You can almost always replace a function of multiple arguments with a function-valued function—for example, writing f [a][b] instead of f [a, b]."
[^learn-multi]: Hillel Wayne, Learn TLA+, Structured Data (trang sống) — <https://learntla.com/core/functions.html> — "You can also call the function with Prod[3, 5] and leave out the angle brackets."
[^bk-fn-vs-op0]: Lamport, Specifying Systems, mục 6.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "They define two very different kinds of objects: fact is a function, and Tail is an operator."
[^bk-fn-vs-op1]: Lamport, Specifying Systems, mục 6.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Unlike an operator, a function must have a domain, which is a set."
[^bk-def-opname]: Lamport, Specifying Systems, mục 3.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The identifier Send by itself is not an expression"
[^bk-fn-vs-op3]: Lamport, Specifying Systems, mục 6.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Unlike a function, an operator cannot be defined recursively in TLA+."
[^bk-fn-vs-op2]: Lamport, Specifying Systems, mục 6.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "For example, if a variable may have V as its value, then V must be a function."
[^bk-op-arg]: Lamport, Specifying Systems, mục 6.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Operators also differ from functions in that an operator can take an operator as an argument."
[^bk-tail-notfn]: Lamport, Specifying Systems, mục 6.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "the domain of such a function would have to include all nonempty sequences, and the collection of all such sequences is too big to be a set."
[^bk-fact-illegal]: Lamport, Specifying Systems, mục 5.5 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "This definition is illegal because the occurrence of fact to the right of the = is undefined—fact is defined only after its definition."
[^bk-recfn-ok]: Lamport, Specifying Systems, mục 5.5 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "TLA+ does allow the apparent circularity of recursive function definitions."
[^bk-recfn-silly]: Lamport, Specifying Systems, mục 6.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "A recursive function definition doesn’t necessarily define a function."
[^bk-mutual]: Lamport, Specifying Systems, mục 6.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Although TLA+ allows the apparent circularity of a recursive function definition, it does not allow circular definitions in which two or more functions are defined in terms of one another."
[^bk-mr-trick]: Lamport, Specifying Systems, mục 6.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "This trick can be used to convert any mutually recursive definitions into a single recursive definition of a record-valued function whose fields are the desired functions."
[^learn-recursive]: Hillel Wayne, Learn TLA+, More Operators (trang sống) — <https://learntla.com/core/advanced-operators.html> — "in 2008 TLA+ was updated to include recursive operators."
[^bk-using-fn]: Lamport, Specifying Systems, mục 6.5 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "When writing specifications, we almost always want to specify the new value of a variable f rather than the new values of f [i ] for all i in some set."
[^learn-two-collections]: Hillel Wayne, Learn TLA+, Structured Data (trang sống) — <https://learntla.com/core/functions.html> — "TLA+ has only two “real” collections: sets and functions."
[^bk-record-def]: Lamport, Specifying Systems, mục 5.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "A record is a function whose domain is a finite set of strings."
[^bk-tuple-fn]: Lamport, Specifying Systems, mục 5.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "In TLA+, this 3-tuple is actually the function with domain {1, 2, 3} that maps 1 to a, 2 to b, and 3 to c."
[^bk-seq-tuple]: Lamport, Specifying Systems, mục 5.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The Sequences module defines finite sequences to be tuples."
[^bk-seq-ops]: Lamport, Specifying Systems, mục 4.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The Sequences module defines the following operators on sequences:"
[^bk-record-why]: Lamport, Specifying Systems, mục 3.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "But programmers have learned that using tuples like this leads to mistakes; it’s easy to forget if the ack line is represented by the second or third component."
[^bk-record-unordered]: Lamport, Specifying Systems, mục 3.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The fields of a record are not ordered, so it doesn’t matter in what order we write them."
[^bk-record-set]: Lamport, Specifying Systems, mục 3.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The type invariant asserts that the value of chan is an element of the set of all such records r in which r .val is an element of the set Data and r .rdy and r .ack are elements of the set {0, 1}."
[^bk-record-illegal]: Lamport, Specifying Systems, mục 16.1.8 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "These expressions are legal only if the h i are all different. For example, [a : S , a : T ] is illegal."
[^bk-except-record]: Lamport, Specifying Systems, mục 3.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Think of the ! as standing for the new record that the except expression forms by modifying chan."
[^bk-angle]: Lamport, Specifying Systems, mục 3.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "(Angle brackets are typed in ascii as << and >>.)"
[^bk-empty-seq]: Lamport, Specifying Systems, mục 4.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The empty sequence is the 0-tuple (there’s only one, and it’s written h i)"
[^bk-seq-ops-ex]: Lamport, Specifying Systems, mục 4.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "For example, Append (h3, 7i, 3) equals h3, 7, 3i."
[^bk-seq-set]: Lamport, Specifying Systems, mục 4.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The set of all sequences of elements of the set S . For example, h3, 7i is an element of Seq(Nat)."
[^bk-seq-concat]: Lamport, Specifying Systems, mục 4.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "(We type ◦ in ascii as \o.)"
[^bk-selectseq]: Lamport, Specifying Systems, mục 18.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The subsequence of s consisting of the elements s[i ] such that Test(s[i ]) equals true."
[^bk-subseq]: Lamport, Specifying Systems, mục 18.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "consisting of the m th through n th elements of s"
[^learn-1idx]: Hillel Wayne, Learn TLA+, Operators and Values (trang sống) — <https://learntla.com/core/operators.html> — "Did I mention they’re 1-indexed? Because they’re 1-indexed."
[^learn-empty-range]: Hillel Wayne, Learn TLA+, Operators and Values (trang sống) — <https://learntla.com/core/operators.html> — "If seq = <<>>, you get 1..0 = {}, which is what you’d expect."
[^bk-fifo]: Lamport, Specifying Systems, mục 4.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The value of q is the sequence of messages that have been sent by the sender but not yet received by the receiver."
[^bk-record-power]: Lamport, Specifying Systems, mục 5.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The TLA+ definition of records as functions makes it possible to manipulate them in ways that have no counterparts in programming languages."
[^bk-record-override]: Lamport, Specifying Systems, mục 5.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "we can define an operator R such that R(r , s) is the record obtained from r by replacing the value of each field c that is also a field of the record s with s.c."
[^bk-string]: Lamport, Specifying Systems, mục 16.1.10 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "TLA+ defines a string to be a tuple of characters."
[^learn-string]: Hillel Wayne, Learn TLA+, Operators and Values (trang sống) — <https://learntla.com/core/operators.html> — "There are no operators for strings except = and #. In practice, they are used as opaque identifiers, similar to how some languages have a :symbol type."
[^repo-seq-defs]: tlaplus/tlaplus @94d0c50, StandardModules/Sequences.tla — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/src/tla2sany/StandardModules/Sequences.tla> — "Seq(S) == UNION {[1..n -> S] : n \in Nat}"
[^repo-len]: tlaplus/tlaplus @94d0c50, StandardModules/Sequences.tla — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/src/tla2sany/StandardModules/Sequences.tla> — "Len(s) == CHOOSE n \in Nat : DOMAIN s = 1..n"
[^repo-append]: tlaplus/tlaplus @94d0c50, StandardModules/Sequences.tla — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/src/tla2sany/StandardModules/Sequences.tla> — "Append(s, e) == s \o <\<e>>"
[^repo-head]: tlaplus/tlaplus @94d0c50, StandardModules/Sequences.tla — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/src/tla2sany/StandardModules/Sequences.tla> — "Head(s) == s[1] Tail(s) == CASE s # << >> -> [i \in 1..(Len(s)-1) |-> s[i+1]]"
[^repo-tail]: tlaplus/tlaplus @94d0c50, StandardModules/Sequences.tla — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/src/tla2sany/StandardModules/Sequences.tla> — "Definition of Tail changed on 4 Jun 2013 because original defined Tail(<< >>) = << >> ."
[^bk-tail-def]: Lamport, Specifying Systems, mục 18.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "[i ∈ 1 . . (Len(s) − 1) 7→ s[i + 1]] The usual head (first) and tail (rest) operators."
[^bk-seq-silly]: Lamport, Specifying Systems, mục 14.2.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "TLC cannot evaluate the expression x [1] if x equals h i, since h i[1] is silly."
[^learn-record-err]: Hillel Wayne, Learn TLA+, Structured Data (trang sống) — <https://learntla.com/core/functions.html> — "it’s because you wrote [key: "val"] instead of [key: {"val"}]."
[^learn-record-key]: Hillel Wayne, Learn TLA+, Structured Data (trang sống) — <https://learntla.com/core/functions.html> — "It’s because you wrote ["key" |-> val] instead of [key |-> val]."
[^bk-choose-def]: Lamport, Specifying Systems, mục 5.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The expression choose x : F equals an arbitrarily chosen value x that satisfies the formula F . (If no such x exists, the expression has a completely arbitrary value.)"
[^bk-choose-bounded]: Lamport, Specifying Systems, mục 6.6 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "(The expression choose x ∈ S : p means choose x : (x ∈ S ) ∧ p.)"
[^bk-choose-nondet]: Lamport, Specifying Systems, mục 6.6 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "In mathematics, there is no such thing as a nondeterministic operator or a nondeterministic function."
[^bk-choose-unique]: Lamport, Specifying Systems, mục 6.6 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The most common use for the choose operator is to “name” a uniquely specified value."
[^bk-choose-rules]: Lamport, Specifying Systems, mục 16.1.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "We know nothing about the value chosen by choose except what we can deduce from these rules."
[^bk-choose-div]: Lamport, Specifying Systems, mục 16.1.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "We can therefore deduce that 1/0 equals 2/0."
[^bk-noval]: Lamport, Specifying Systems, mục 5.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "We have no idea what the value of NoVal is; we just know what it isn’t—namely, that it isn’t an element of Val ."
[^bk-choose-vs-in]: Lamport, Specifying Systems, mục 6.6 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "allows only a single behavior—one in which x always equals choose n : n ∈ Nat, which is some particular, unspecified natural number."
[^tlc-choose-err]: Lamport, Specifying Systems, mục 14.2.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "However, this case almost always arises because of a mistake, so TLC treats it as an error."
[^tlc-choose-lazy]: Lamport, Specifying Systems, mục 14.2.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "will not produce an error because TLC will not evaluate the choose expression if n ≤ 5."
[^tlc-unbounded]: Lamport, Specifying Systems, mục 14.2.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "TLC cannot evaluate “unbounded” quantifiers or choose expressions—that is, expressions having one of the forms"
[^learn-choose-det]: Hillel Wayne, Learn TLA+, Operators and Values (trang sống) — <https://learntla.com/core/operators.html> — "In this case the only requirement is that the result is deterministic: the engine must always return the same value, no matter what."
[^bk-hilbert]: Lamport, Specifying Systems, mục 16.1.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "This form of the choose operator is known to mathematicians as Hilbert’s ε."
[^bk-if-via-choose]: Lamport, Specifying Systems, mục 16.1.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The if/then/else and case constructs are defined as follows in terms of choose:"
[^bk-choice-op]: Lamport, Specifying Systems, mục 16.1.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "But there is seldom any practical reason for worrying about it."
[^bk-def-synonym]: Lamport, Specifying Systems, mục 3.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "defines Id to be synonymous with the expression exp."
[^bk-def-op]: Lamport, Specifying Systems, mục 3.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "For any expression e, this defines Id (e) to be the expression obtained by substituting e for p in exp."
[^bk-def-after-parse]: Lamport, Specifying Systems, mục 3.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "This replacement must be done after the expression is parsed, not in the “raw input”."
[^bk-scope]: Lamport, Specifying Systems, mục 3.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The scope of a variable or constant declaration, and of a definition, is the part of the module that follows it."
[^bk-redecl]: Lamport, Specifying Systems, mục 3.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "A symbol cannot be declared or defined if it already has a meaning."
[^sum-let]: Lamport, Summary of TLA+, Miscellaneous Constructs — <https://lamport.azurewebsites.net/tla/summary-standalone.pdf> — "[e in the context of the definitions]"
[^learn-if]: Hillel Wayne, Learn TLA+, Operators and Values (trang sống) — <https://learntla.com/core/operators.html> — "Expressions always equal something, so there must always be an ELSE branch."
[^learn-let-seq]: Hillel Wayne, Learn TLA+, Operators and Values (trang sống) — <https://learntla.com/core/operators.html> — "Each operator in the LET can refer to previously defined operators in that scope."
[^bk-let-ex]: Lamport, Specifying Systems, mục 16.1.5 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "equals 1 ∗ 1 + 2 ∗ 2 + 3 ∗ 3, which equals 14."
[^bk-if]: Lamport, Specifying Systems, mục 16.1.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "It equals e 1 if p is true, and e 2 if p is false."
[^bk-ascii-box]: Lamport, Specifying Systems, mục 2.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "When possible, symbols are represented pictorially in ascii—for example, 2 is typed as [ ] and 6= as #. (You can also type 6= as /=.)"
[^bk-case-sep]: Lamport, Specifying Systems, mục 15.2.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The case separator 2 (not the prefix temporal operator that is typed the same) ends all of these constructs except a case statement without an other clause."
[^bk-case-disjoint]: Lamport, Specifying Systems, mục 16.1.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The case expressions (16.4) are generally used when the p i are mutually disjoint, so at most one p i can be true."
[^bk-case-none]: Lamport, Specifying Systems, mục 16.1.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "In that case, the value of the first is unspecified, while the value of the second is e, the other expression."
[^bk-ispo]: Lamport, Specifying Systems, mục 6.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The first argument of IsPartialOrder is an operator that takes two arguments; its second argument is an expression."
[^bk-ispo-ex]: Lamport, Specifying Systems, mục 6.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The expression IsPartialOrder (+, 3) is also syntactically correct, but it’s silly and we have no idea whether or not it equals true."
[^bk-ascii-def]: Lamport, Specifying Systems, mục 2.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The major exception is =, which is typed as =="
[^learn-eq-err]: Hillel Wayne, Learn TLA+, Operators and Values (trang sống) — <https://learntla.com/core/operators.html> — "It’s likely because you used a double == instead of a single = when testing for equality."
[^bk-cardinality-let]: Lamport, Specifying Systems, mục 6.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "However, we can usually transform an illegal recursive operator definition into a nonrecursive one using a recursive function definition."
[^repo-card]: tlaplus/tlaplus @94d0c50, StandardModules/FiniteSets.tla — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/src/tla2sany/StandardModules/FiniteSets.tla> — "LET CS[T \in SUBSET S] == IF T = {} THEN 0 ELSE 1 + CS[T \ {CHOOSE x : x \in T}] IN CS[S]"
[^bk-comments]: Lamport, Specifying Systems, mục 3.5 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "A comment may appear anywhere enclosed between (* and *). An end-of-line comment is preceded by \*. Comments may be nested"
[^bk-nums-predef]: Lamport, Specifying Systems, mục 16.1.11 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "However, sets of numbers like Nat and arithmetic operators like + are not."
[^bk-int]: Lamport, Specifying Systems, mục 18.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The Integers module extends the Naturals module and also defines the set Int of integers and unary minus (−)."
[^bk-no-overflow]: Lamport, Specifying Systems, chú thích 1 trong lời dẫn Phần I (trang 7, trước Chương 1) — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "numbers are not strings of bits, and 233 ∗ 233 equals 266 , not overflow error."
[^bk-extends]: Lamport, Specifying Systems, mục 2.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Arithmetic operators like + are not built into TLA+, but are themselves defined in modules."
[^learn-one-extends]: Hillel Wayne, Learn TLA+, Operators and Values (trang sống) — <https://learntla.com/core/operators.html> — "TLA+ can only have one EXTENDS line per spec, but you can have multiple modules (separated by commas) on it."
[^bk-extends-seq]: Lamport, Specifying Systems, mục 18.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "However, this would mean that any module that extends Sequences would then also extend Naturals."
[^bk-local-inst]: Lamport, Specifying Systems, mục 18.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "but it does not export those definitions to another module that extends or instantiates the Sequences module."
[^bk-nat-ops]: Lamport, Specifying Systems, mục 18.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The Naturals module defines the following operators:"
[^bk-real-div]: Lamport, Specifying Systems, mục 18.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The Reals module extends Integers and introduces the set Real of real numbers and ordinary division (/)."
[^bk-nat-subset]: Lamport, Specifying Systems, mục 18.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Hence, Nat is a subset of Int, which is a subset of Real ."
[^bk-numbers-practical]: Lamport, Specifying Systems, mục 18.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "When writing specifications, you can just assume that the operators they define have their usual meanings."
[^bk-dotdot]: Lamport, Specifying Systems, mục 2.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "In general i . . j is the set of integers from i through j , for any integers i and j . (It equals the empty set if j < i .)"
[^bk-div-mod]: Lamport, Specifying Systems, mục 18.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Integer division (÷) and modulus (%) are defined so that the following two conditions hold, for any integer a and positive integer b:"
[^repo-naturals]: tlaplus/tlaplus @94d0c50, StandardModules/Naturals.tla — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/src/tla2sany/StandardModules/Naturals.tla> — "This module provides dummy definitions of the operators that are defined by the real Naturals module. It is expected that any tool will provide its own implementations of these operators."
[^repo-seq-override]: tlaplus/tlaplus @94d0c50, StandardModules/Sequences.tla — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/src/tla2sany/StandardModules/Sequences.tla> — "These definitions are all overridden by TLC in the Java class tlc2.module.Sequences."
[^tlc-enum]: Lamport, Specifying Systems, mục 14.2.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "It enumerates a set of the form {x ∈ S : p} by first enumerating S , so it can enumerate {i ∈ 0 . . 5 : i < 4} but not {i ∈ Nat : i < 4}."
[^repo-integers]: tlaplus/tlaplus @94d0c50, StandardModules/Integers.tla — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/src/tla2sany/StandardModules/Integers.tla> — "This defines the prefix - operator."
[^learn-no-float]: Hillel Wayne, Learn TLA+, Operators and Values (trang sống) — <https://learntla.com/core/operators.html> — "Note there is not a float type. Floats have complex semantics that are extremely hard to represent. Usually you can abstract them out, but if you absolutely need floats then TLA+ is the wrong tool for the job."
[^bk-peano]: Lamport, Specifying Systems, mục 18.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "It simply defines the naturals to be a set satisfying Peano’s axioms."
[^bk-silly]: Lamport, Specifying Systems, mục 6.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "In an untyped formalism, every syntactically well-formed expression has a meaning—even a silly expression like 3/“abc”."
[^bk-silly2]: Lamport, Specifying Systems, mục 6.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "However, the truth of a correct formula cannot depend on the meaning of a silly expression."
[^bk-type-meaning]: Lamport, Specifying Systems, mục 3.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "For now, remember that TLA+ is an untyped language. Type correctness is just a name for a certain invariance property."
[^bk-silly-tlc]: Lamport, Specifying Systems, mục 6.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The TLC model checker, described in Chapter 14, will report an error if it tries to evaluate a semantically silly expression."
[^bk-types-cost]: Lamport, Specifying Systems, mục 6.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "For writing specifications, I have found that the costs outweigh the benefits."
[^bk-type-def]: Lamport, Specifying Systems, mục 3.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "A variable v has type T in a specification Spec iff v ∈ T is an invariant of Spec."
[^tlc-left-right]: Lamport, Specifying Systems, mục 14.2.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "It evaluates p ∧ q by first evaluating p and, if it equals true, then evaluating q."
[^tlc-natural-order]: Lamport, Specifying Systems, mục 14.2.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Fortunately, we naturally write the first formula rather than the second because it’s easier to understand."
[^tlc-forms]: Lamport, Specifying Systems, mục 14.2.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "TLC will evaluate expressions of the following forms only if it can enumerate the set S :"
[^tlc-value]: Lamport, Specifying Systems, mục 14.2.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "TLC can evaluate a set-valued expression only if that expression equals a finite set, and it can evaluate a function-valued expression only if that expression equals a function whose domain is a finite set."
[^tlc-partial]: Lamport, Specifying Systems, mục 14.2.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "TLC can often evaluate an expression even when it can’t evaluate all subexpressions."
[^bk-moderate]: Lamport, Specifying Systems, mục 16.1.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The semantics of TLA+ asserts that the rules of the moderate interpretation are valid."
[^rs-hs-union]: Rust 1.95.0, std::collections::HashSet — <https://doc.rust-lang.org/1.95.0/std/collections/struct.HashSet.html> — "Visits the values representing the union, i.e., all the values in self or other, without duplicates."
[^rs-hs-subset]: Rust 1.95.0, std::collections::HashSet — <https://doc.rust-lang.org/1.95.0/std/collections/struct.HashSet.html> — "Returns true if the set is a subset of another, i.e., other contains at least all the values in self."
[^rs-all-empty]: Rust 1.95.0, std::iter::Iterator — <https://doc.rust-lang.org/1.95.0/std/iter/trait.Iterator.html> — "An empty iterator returns true."
[^rs-any-empty]: Rust 1.95.0, std::iter::Iterator — <https://doc.rust-lang.org/1.95.0/std/iter/trait.Iterator.html> — "An empty iterator returns false."
[^rs-find]: Rust 1.95.0, std::iter::Iterator — <https://doc.rust-lang.org/1.95.0/std/iter/trait.Iterator.html> — "find() takes a closure that returns true or false. It applies this closure to each element of the iterator, and if any of them return true, then find() returns Some(element). If they all return false, it returns None."
[^rs-hm-index]: Rust 1.95.0, std::collections::HashMap — <https://doc.rust-lang.org/1.95.0/std/collections/struct.HashMap.html> — "Panics if the key is not present in the HashMap."
[^rs-hm-insert]: Rust 1.95.0, std::collections::HashMap — <https://doc.rust-lang.org/1.95.0/std/collections/struct.HashMap.html> — "If the map did have this key present, the value is updated, and the old value is returned."
[^rs-vec-index]: Rust 1.95.0, std::vec::Vec — <https://doc.rust-lang.org/1.95.0/std/vec/struct.Vec.html> — "if you try to access an index which isn’t in the Vec, your software will panic!"
[^rs-struct]: The Rust Programming Language 1.95.0, ch. 5.1 — <https://doc.rust-lang.org/1.95.0/book/ch05-01-defining-structs.html> — "Unlike with tuples, in a struct you’ll name each piece of data so it’s clear what the values mean."
[^rs-struct-update]: The Rust Programming Language 1.95.0, ch. 5.1 — <https://doc.rust-lang.org/1.95.0/book/ch05-01-defining-structs.html> — "It’s often useful to create a new instance of a struct that includes most of the values from another instance of the same type, but changes some of them."
[^rs-tuple]: Rust 1.95.0, primitive type tuple — <https://doc.rust-lang.org/1.95.0/std/primitive.tuple.html> — "A finite heterogeneous sequence, (T, U, ..)."
[^rs-match]: Rust Reference 1.95.0, match expressions — <https://doc.rust-lang.org/1.95.0/reference/expressions/match-expr.html> — "The first arm with a matching pattern is chosen as the branch target of the match"
[^rs-rem-euclid]: Rust 1.95.0, primitive type i32 — <https://doc.rust-lang.org/1.95.0/std/primitive.i32.html> — "the result satisfies self = rhs * self.div_euclid(rhs) + r and 0 <= r < abs(rhs)."
[^rs-overflow]: The Rust Programming Language 1.95.0, ch. 3.2 — <https://doc.rust-lang.org/1.95.0/book/ch03-02-data-types.html> — "When you’re compiling in release mode with the --release flag, Rust does not include checks for integer overflow that cause panics."
[^tlc-override]: Lamport, Specifying Systems, mục 14.2.5 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "TLC cannot compute 2 + 2 from the definition of + contained in the standard Naturals module."
[^bk-extends-far]: Lamport, Specifying Systems, mục 15.2.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "precedence, so an expression made with one of them extends as far as possible."
[^bk-rule-ex]: Lamport, Specifying Systems, mục 15.2.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Here is how some expressions are interpreted under this rule:"
[^bk-equiv-subtle]: Lamport, Specifying Systems, chú thích 1 của chương 1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Section 16.1.3 on page 296 explains a more subtle reason for using ≡ instead of = for equality of Boolean values."
[^bk-equiv-3]: Lamport, Specifying Systems, mục 16.1.3, chú thích 3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Equality (=) is not an operator of propositional or predicate logic; this tautology need not be valid for non-Boolean values if ≡ is replaced by =."
[^bk-map-multi]: Lamport, Specifying Systems, mục 6.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "For example, {e : x ∈ S , y ∈ T } is the set of all elements of the form e, for x in S and y in T ."
[^bk-fn-vs-op-expr]: Lamport, Specifying Systems, mục 6.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Both fact[n] ∈ S and fact ∈ S are syntactically correct expressions."
[^bk-prefer-op]: Lamport, Specifying Systems, mục 6.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "If these differences don’t determine whether to use an operator or a function, then the choice is a matter of taste. I usually prefer operators."
[^bk-recfn-abbrev]: Lamport, Specifying Systems, mục 6.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "is simply an abbreviation for (6.2)."
[^bk-mapsto-ascii]: Lamport, Specifying Systems, mục 3.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "(The symbol 7→ is typed in ascii as |-> .)"
[^learn-lambda]: Hillel Wayne, Learn TLA+, Advanced Operators (trang sống) — <https://learntla.com/core/advanced-operators.html> — "You can define anonymous operators with LAMBDA:"
[^bk-0tuple]: Lamport, Specifying Systems, mục 16.1.9 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The 0-tuple h i is the unique function having an empty domain."
[^tlc-notans]: Lamport, Specifying Systems, mục 14.2.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "because it cannot evaluate the unbounded choose expression. You can override this definition by assigning NotAnS a value in the CONSTANT statement of the configuration file."
[^bk-redecl2]: Lamport, Specifying Systems, mục 3.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Although conventional mathematics and programming languages allow such redeclarations, TLA+ forbids them because they can lead to confusion and errors."
[^bk-hcnxt]: Lamport, Specifying Systems, hình 2.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "HCnxt == hr’ = IF hr # 12 THEN hr + 1 ELSE 1"
[^learn-div]: Hillel Wayne, Learn TLA+, Operators and Values (trang sống) — <https://learntla.com/core/operators.html> — "h == seconds \div 3600 h_left == seconds % 3600"
[^sum-nat-minus]: Lamport, Summary of TLA+, Operators Defined in Standard Modules — <https://lamport.azurewebsites.net/tla/summary-standalone.pdf> — "(1) Only infix − is defined in Naturals."
[^bk-parser]: Lamport, Specifying Systems, mục 6.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "the parser described in Chapter 12 catches syntactic errors, but not semantic silliness."
[^bk-liberal]: Lamport, Specifying Systems, mục 16.1.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "In the liberal interpretation, the value of 2 ∧ h5i is specified to be a Boolean."
[^bk-tnat]: Lamport, Specifying Systems, mục 16.1.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "equals true in the liberal interpretation, but not in the conservative interpretation."
[^bk-tnat-mod]: Lamport, Specifying Systems, mục 16.1.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "In the moderate interpretation, (16.3) equals true, but the value of h5i ∧ 2 is still completely unspecified."
[^bk-tnat-safe]: Lamport, Specifying Systems, mục 16.1.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "If we are using the conservative interpretation, instead of (16.3), we should write"
