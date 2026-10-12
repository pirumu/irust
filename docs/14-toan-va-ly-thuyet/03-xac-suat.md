# P14 · Chương 03 — Xác suất cho hệ thống

> Phạm vi: chương này dạy xác suất rời rạc ở mức đủ để lập luận về hệ thống thật: không gian xác suất, kỳ vọng và phương sai, các bất đẳng thức đuôi (Markov, Chebyshev, Chernoff), balls-into-bins và birthday paradox, power of two choices, Bloom filter, và xác suất tách phiếu khi Raft bầu leader. Chứng minh được viết đủ từng bước; chỗ nào người soạn tự dựng thì ghi "(chứng minh của người soạn)". Các số ghi "quan sát" là kết quả chạy thật các chương trình Rust trong chương (`rustc 1.95.0`, macOS arm64, một lần chạy với seed cố định), không phải bảo đảm của tài liệu nào. Hệ thống `kvmeta` và các con số giả định quanh nó (xác suất hỏng, tải mỗi node…) là ví dụ của người soạn, không phải code trong repo.

Backend engineer quen nghĩ theo "trung bình" hoặc "trường hợp xấu nhất", nhưng phần lớn câu hỏi vận hành thật nằm ở giữa. Một node cache quá tải *hiếm đến mức nào*? Bloom filter báo oan *bao nhiêu phần trăm*? Hai request id có *đụng nhau* không? Bầu leader Raft có *kẹt* không? Cả bốn là câu hỏi xác suất, và trả lời được bằng con số thay vì cảm giác là một kỹ năng đáng học (nhận xét của người soạn).

Cả chương bám theo `kvmeta`, một kho key-value giả định giữ feature flag và khóa leader cho các service backend. Lõi của nó là cụm Raft 5 node đã gặp ở [P12 · Chương 18](../12-he-thong-du-lieu/18-thuat-toan-dong-thuan.md). Ta thêm hai thứ quanh lõi đó: một tầng cache `kvmeta-cache` gồm 100 node đứng trước (client băm khóa để chọn node), và mỗi node Raft lưu dữ liệu bằng LSM-tree với một Bloom filter cho mỗi SSTable. Mỗi request còn mang một id ngẫu nhiên để khử trùng lặp. Các mục 1–3 dựng công cụ; các mục 4–7 dùng công cụ ấy lần lượt cho tầng cache, request id, Bloom filter và bầu cử Raft.

## Bạn sẽ hiểu được gì

1. **Q1.** Không gian xác suất, xác suất có điều kiện, độc lập và union bound là gì, và vì sao giả thiết "các sự cố độc lập" là chỗ dễ sai nhất? → mục 1
2. **Q2.** Kỳ vọng, linearity of expectation và phương sai tính ra sao, và vì sao linearity không đòi các biến độc lập? → mục 2
3. **Q3.** Markov, Chebyshev và Chernoff nói gì về "đuôi" của phân phối, chứng minh thế nào, và dùng chúng để đặt capacity ra sao? → mục 3
4. **Q4.** Khi m request băm vào n node: bao nhiêu node rảnh, node nặng nhất nặng cỡ nào (ln n / ln ln n), và vì sao hash, UUID va chạm sớm hơn trực giác (birthday paradox)? → mục 4
5. **Q5.** Power of two choices: vì sao hỏi hai node thay vì một đưa tải nặng nhất từ cỡ ln n / ln ln n xuống cỡ ln ln n / ln 2? → mục 5
6. **Q6.** Tỉ lệ dương tính giả của Bloom filter, (1 − e^(−kn/m))^k, suy ra thế nào, k tối ưu là bao nhiêu, và công thức đó đúng đến đâu? → mục 6
7. **Q7.** Khi leader Raft chết, xác suất bầu cử tách phiếu với election timeout ngẫu nhiên là bao nhiêu (5 node, khoảng 150–300 ms), và mô phỏng Monte Carlo có khớp không? → mục 7

## Kiến thức cần trước

- [Chương 01 — Logic và chứng minh](01-logic-va-chung-minh.md): phản chứng, quy nạp. Các chứng minh dưới đây dùng chúng ở mức rất nhẹ.
- [Chương 02 — Toán rời rạc](02-toan-roi-rac.md): tập hợp, đếm tổ hợp C(n, k).
- Không cần học xác suất trước. Bạn chỉ cần đọc được tổng Σ, tích ∏, hàm mũ và logarit tự nhiên.
- Khi đọc mục 6 và 7, các chương sau là bối cảnh: [P12 · Chương 03, mục 4](../12-he-thong-du-lieu/03-storage-engine.md) (Bloom filter trong LSM-tree), [P12 · Chương 09, mục 7](../12-he-thong-du-lieu/09-consistency-va-consensus.md) và [P12 · Chương 18](../12-he-thong-du-lieu/18-thuat-toan-dong-thuan.md) (Raft, quorum). Mục 4 liên quan [P12 · Chương 06, mục 3](../12-he-thong-du-lieu/06-partitioning.md) (consistent hashing), còn mục 3 nối với [P11 · Chương 03](../11-do-dac-va-go-loi/03-hieu-nang-service-rust.md) (percentile).

---

## 1. Không gian xác suất, độc lập và union bound (Q1)

> **Trả lời ngắn.** Một không gian xác suất gồm tập mọi kết cục có thể xảy ra (gọi là không gian mẫu) cùng một hàm gán xác suất cho từng kết cục[^mcs-space]; một biến cố là một tập kết cục. Hai biến cố độc lập khi biết cái này xảy ra không làm đổi xác suất của cái kia[^mcs-indep], và xác suất có điều kiện cho phép chia bài toán thành các trường hợp rồi cộng lại. Khi không biết các biến cố liên quan nhau thế nào, union bound vẫn đúng: xác suất "ít nhất một thành phần hỏng" không vượt quá tổng xác suất hỏng của từng thành phần[^mcs-union].

**Giải thích.**

*Với `kvmeta`.* Tầng `kvmeta-cache` có 100 node và client băm khóa để chọn node. Trước khi tính được bất cứ điều gì, ta phải nói "một request rơi vào node nào" bằng ngôn ngữ toán. Mục này dựng bộ khung đó (không gian mẫu, biến cố, xác suất có điều kiện, độc lập), rồi dùng nó để chỉ ra giả thiết nào hay bị dùng sai nhất khi tính xác suất sự cố (nhận xét của người soạn).

### 1.1 Không gian mẫu, biến cố, xác suất

Không gian mẫu Ω là tập mọi kết cục. Với một request của `kvmeta-cache`, Ω = {1, 2, …, 100} là chỉ số node mà hàm băm chọn. Hàm xác suất Pr gán cho mỗi kết cục ω một số Pr[ω] ≥ 0, và các số này cộng lại bằng 1. Nếu hàm băm đủ tốt để coi là đều thì Pr[ω] = 1/100 với mọi ω (giả thiết, nhận xét của người soạn). Biến cố là một tập con của Ω[^mcs-event]. Xác suất của một biến cố là tổng xác suất các kết cục nằm trong nó. Biến cố "request rơi vào một node của rack A", với rack A chứa 20 node, có xác suất 20 × 1/100 = 0,2.

Hai biến cố rời nhau (không bao giờ cùng xảy ra) thì xác suất của hợp bằng tổng hai xác suất, vì không kết cục nào bị đếm hai lần. Quy tắc cộng này là nền cho mọi chứng minh trong chương.

### 1.2 Có điều kiện và độc lập

Xác suất có điều kiện Pr[A | B] = Pr[A ∩ B] / Pr[B] (khi Pr[B] > 0) là xác suất của A khi ta đã biết B xảy ra. Viết ngược lại, ta có quy tắc nhân Pr[A ∩ B] = Pr[B] · Pr[A | B], chính là việc nhân các xác suất dọc một nhánh của cây quyết định[^mcs-product]. Từ đó có công thức xác suất toàn phần: Pr[A] = Pr[A | B] · Pr[B] + Pr[A | không B] · Pr[không B][^mcs-total].

A và B độc lập khi Pr[A | B] = Pr[A], tức Pr[A ∩ B] = Pr[A] · Pr[B][^mcs-indep]. Có hai chỗ hay nhầm. Chỗ thứ nhất: hai biến cố rời nhau mà cả hai có xác suất dương thì *không* độc lập, vì biết một cái xảy ra là biết cái kia không xảy ra[^mcs-disjoint]. Chỗ thứ hai: độc lập từng đôi một (pairwise) là điều kiện yếu hơn độc lập toàn phần (mutual)[^mcs-pairwise]; phần Đi sâu có ví dụ hai đồng xu. Chương này cần cả hai mức: phương sai (mục 2) chỉ đòi độc lập từng đôi một, còn Chernoff (mục 3) đòi độc lập toàn phần.

### 1.3 Ví dụ: bỏ giả thiết độc lập thì con số nhân lên gấp bốn

Cụm Raft 5 node của `kvmeta` mất quorum (không ghi được nữa) khi từ 3 node trở lên hỏng; lý do nằm ở [P12 · Chương 18](../12-he-thong-du-lieu/18-thuat-toan-dong-thuan.md). Giả sử mỗi node hỏng trong một ngày với xác suất p = 1% và các node hỏng độc lập (cả hai đều là số giả định của người soạn). Số node hỏng khi ấy có phân phối nhị thức, nên

```text
Pr[≥ 3 node hỏng] = C(5,3)·p³·(1−p)² + C(5,4)·p⁴·(1−p) + p⁵
                  = 10·10⁻⁶·0,9801 + 5·10⁻⁸·0,99 + 10⁻¹⁰
                  ≈ 9,85·10⁻⁶ mỗi ngày.
```

Đó là khoảng một ngày trong 100 000 ngày, một con số nhỏ đến mức dễ tin. Bây giờ bỏ giả thiết độc lập: hai trong 5 node cắm chung một rack, và rack mất nguồn với xác suất q = 0,1% mỗi ngày (cũng là số giả định) làm cả hai node chết cùng lúc. Gọi R là biến cố "rack mất nguồn". Theo công thức xác suất toàn phần, Pr[mất quorum] = Pr[mất quorum | R]·Pr[R] + Pr[mất quorum | không R]·Pr[không R]. Khi R xảy ra, hai node đã chết, nên chỉ cần thêm ít nhất một trong ba node còn lại hỏng: 1 − 0,99³ ≈ 0,0297. Khi R không xảy ra, ta quay về tình huống độc lập, 9,85·10⁻⁶. Cộng lại: 0,001 × 0,0297 + 0,999 × 9,85·10⁻⁶ ≈ 3,95·10⁻⁵, gấp khoảng 4 lần con số độc lập (quan sát: chương trình `numbers.rs` in ra 9.851e-6, 3.954e-5 và tỉ số 4.01). Một nguyên nhân chung, rẻ như một bộ nguồn, đã đủ làm đổi kết luận. Từ đó rút ra một nhận xét của người soạn: sai số của phần lớn ước lượng độ sẵn sàng đến từ giả thiết độc lập chứ không đến từ phép tính.

### 1.4 Union bound

Với các biến cố E₁, …, Eₙ bất kỳ: Pr[E₁ ∪ … ∪ Eₙ] ≤ Pr[E₁] + … + Pr[Eₙ][^mcs-union].

**Chứng minh (chứng minh của người soạn).** Đặt F₁ = E₁ và Fᵢ = Eᵢ \ (E₁ ∪ … ∪ Eᵢ₋₁) với i ≥ 2: Fᵢ là phần của Eᵢ chưa nằm trong các biến cố trước nó. Có ba điều cần kiểm.

1. Các Fᵢ đôi một rời nhau. Nếu i < j và ω ∈ Fᵢ thì ω ∈ Eᵢ; mà Fⱼ đã loại mọi phần tử của Eᵢ, nên ω ∉ Fⱼ.
2. Hợp của các Fᵢ bằng hợp của các Eᵢ. Vì Fᵢ ⊆ Eᵢ nên hợp các Fᵢ nằm trong hợp các Eᵢ. Ngược lại, nếu ω thuộc hợp các Eᵢ, gọi i là chỉ số nhỏ nhất có ω ∈ Eᵢ; khi đó ω không thuộc Eⱼ nào với j < i, nên ω ∈ Fᵢ.
3. Vì Fᵢ ⊆ Eᵢ nên Pr[Fᵢ] ≤ Pr[Eᵢ] (xác suất là tổng các số không âm trên tập con).

Từ 1 và 2, quy tắc cộng cho biến cố rời nhau cho Pr[∪Eᵢ] = Pr[∪Fᵢ] = Σ Pr[Fᵢ]; theo 3, tổng này không vượt quá Σ Pr[Eᵢ]. ∎

Sức mạnh của union bound là nó không đòi độc lập: các Eᵢ có tương quan thế nào cũng được. Cái giá là cận lỏng khi các biến cố chồng lên nhau nhiều, và vô nghĩa khi tổng vượt 1. Nguồn minh họa bằng bài toán quen thuộc: nếu mỗi trong n bộ phận quan trọng hỏng với xác suất nhỏ thì tổng các xác suất đó cho một cận trên yên tâm cho biến cố "có bộ phận nào hỏng"[^mcs-union]. Với `kvmeta`, "có ít nhất một trong 100 node cache quá tải" không vượt quá 100 × Pr[một node quá tải]; mục 3 tính nốt con số này.

<details><summary>Đi sâu: độc lập từng đôi một mà không toàn phần, và nguồn gốc của sự tương quan</summary>

**Ví dụ hai đồng xu.** Tung hai đồng xu công bằng, độc lập. Gọi A là "đồng thứ nhất sấp", B là "đồng thứ hai sấp", C là "hai đồng cho kết quả khác nhau". Mỗi biến cố có xác suất 1/2. Từng đôi một độc lập: Pr[A ∩ B] = 1/4; Pr[A ∩ C] = Pr[đồng 1 sấp, đồng 2 ngửa] = 1/4; Pr[B ∩ C] = 1/4; cả ba đều bằng tích hai xác suất. Nhưng Pr[A ∩ B ∩ C] = 0, vì khi cả hai đồng cùng sấp thì chúng giống nhau, trong khi toàn phần đòi 1/8. Vậy ba biến cố độc lập từng đôi một nhưng không độc lập toàn phần (người soạn dựng ví dụ này; kết luận "đôi một yếu hơn toàn phần" có trong nguồn đã dẫn ở trên).

**Cùng cấu trúc xuất hiện trong birthday paradox.** Các biến cố "người i trùng sinh nhật với người j" độc lập từng đôi một, nhưng không độc lập ba-ba: nếu Alice trùng Bob và Alice trùng Carol thì chắc chắn Bob trùng Carol[^mcs-bday-notmutual]. Mục 4 dùng đúng tính chất đôi một để tính phương sai.

**Vì sao đời thực hay tương quan.** Các sự cố hiếm hầu như không bao giờ độc lập: chung nguồn điện, chung đợt deploy, chung lỗi kernel, chung đường mạng (nhận xét của người soạn). Ví dụ rack ở trên chỉ là phiên bản nhỏ nhất của chuyện đó. Khi không dám giả thiết độc lập, hãy dùng union bound (không cần độc lập) hoặc đo trực tiếp.

**Chương trình đã chạy.** Hai con số 9.851e-6 và 3.954e-5 ở §1.3 là phần D trong output của `numbers.rs` (hàm `binom_tail_exact` tính tổng nhị thức được trích ở Đi sâu của mục 3):

```text
== D. Hong hoc: 5 node kvmeta, p = 1% moi node/ngay, mat quorum khi >= 3 node hong ==
doc lap: P(>=3 trong 5 hong) = 9.851e-6
them su kien rack (q = 0.001): P(mat quorum) = q*P(>=1 trong 3 node con lai) + (1-q)*P_doc_lap = 1.000e-3*0.02970 + ... = 3.954e-5
ti so voi gia dinh doc lap = 4.01
```

</details>

**Quay lại Q1:** không gian xác suất cho ta ngôn ngữ để nói về "một request rơi vào đâu"; xác suất có điều kiện và độc lập cho phép tách và nhân các xác suất, nhưng độc lập là giả thiết đắt giá (một rack chung đã nhân con số lên 4 lần); union bound là công cụ cứu cánh vì đúng mà không cần độc lập.

---

## 2. Biến ngẫu nhiên, kỳ vọng, linearity và phương sai (Q2)

> **Trả lời ngắn.** Biến ngẫu nhiên gán một con số cho mỗi kết cục, ví dụ số request mà một node nhận trong một giây; kỳ vọng của nó là giá trị trung bình, trong đó mỗi kết cục được tính theo xác suất của nó. Kỳ vọng của một tổng luôn bằng tổng các kỳ vọng, dù các số hạng có phụ thuộc nhau hay không[^mcs-lin], nên nhiều bài toán đếm tưởng rối chỉ còn một dòng tính. Phương sai đo độ phân tán quanh kỳ vọng; phương sai của tổng bằng tổng phương sai khi các số hạng độc lập từng đôi một, nhưng nói chung không đúng khi chúng phụ thuộc nhau[^mcs-addvar].

**Giải thích.**

*Với `kvmeta`.* Một lệnh `multi_get` của service `orders` đọc 100 flag cùng lúc, mỗi flag nằm ở một trong 100 node cache (băm đều, độc lập). Câu hỏi thực dụng: lệnh đó phải liên lạc bao nhiêu node khác nhau? Con số này quyết định độ trễ đuôi, vì `multi_get` chỉ xong khi node chậm nhất trả lời (nhận xét của người soạn). Ta sẽ tính nó trong một dòng nhờ linearity, dù các sự kiện "node 1 bị chạm", "node 2 bị chạm"… phụ thuộc nhau.

### 2.1 Định nghĩa

Biến ngẫu nhiên R là một hàm từ Ω sang số thực: R(ω) là số ta đo được ở kết cục ω. Kỳ vọng là E[R] = Σ_ω R(ω) · Pr[ω]. Biến chỉ thị I_A của biến cố A bằng 1 nếu A xảy ra và 0 nếu không, nên E[I_A] = Pr[A]. Biến chỉ thị là phép đếm cơ bản nhất; đa số bài toán đếm trong chương là tổng của các biến chỉ thị. Phân phối nhị thức Bin(n, p), số lần thành công trong n phép thử độc lập mà mỗi lần thành công với xác suất p, chính là tổng của n biến chỉ thị như vậy.

### 2.2 Linearity of expectation

**Định lý.** Với mọi biến ngẫu nhiên R₁, R₂ và hằng số a₁, a₂: E[a₁R₁ + a₂R₂] = a₁E[R₁] + a₂E[R₂]; quy nạp theo k cho tổng k biến.

**Chứng minh (theo MIT 6.042, viết lại).** Đặt T = R₁ + R₂. Theo định nghĩa, E[T] = Σ_ω (R₁(ω) + R₂(ω)) · Pr[ω] = Σ_ω R₁(ω) · Pr[ω] + Σ_ω R₂(ω) · Pr[ω] = E[R₁] + E[R₂]. Hằng số a₁, a₂ đưa ra ngoài tổng theo cùng cách, và quy nạp theo k cho tổng k biến[^mcs-lin]. ∎

Trong chứng minh không có chỗ nào cần giả thiết độc lập; nguồn nhấn mạnh đúng điểm này vì độc lập rất tốn công để kiểm[^mcs-lin2].

**Áp vào `multi_get`.** Gọi n = 100 là số node, k = 100 là số key. Với mỗi node j, đặt Iⱼ = 1 nếu node j nhận ít nhất một key, và X = I₁ + … + Iₙ là số node bị chạm. Node j *không* bị chạm khi cả k key đều tránh nó; mỗi key tránh nó với xác suất 1 − 1/n và các key độc lập, nên Pr[Iⱼ = 0] = (1 − 1/n)^k. Linearity cho

```text
E[X] = n · (1 − (1 − 1/n)^k).
```

Với n = k = 100: 0,99¹⁰⁰ ≈ 0,366, nên E[X] ≈ 63,4. Một `multi_get` 100 key chạm khoảng 63 trong 100 node và để khoảng 37 node rảnh. Các Iⱼ phụ thuộc nhau (nếu một node nhận cả 100 key thì 99 node kia đều rảnh), nhưng điều đó không làm hỏng phép tính; còn muốn có phân phối đầy đủ của X thì phải làm việc nhiều hơn hẳn (nhận xét của người soạn). Quan sát: mô phỏng 100 000 lần cho tỉ lệ node rảnh 0.36598, tức 36,6 node, khớp với 100 × 0,99¹⁰⁰ = 36,60.

**Hệ quả dùng ở mục 7.** Nếu X nhận giá trị nguyên không âm thì E[X] = Σ_{k≥1} Pr[X ≥ k] (chứng minh của người soạn). Thật vậy, X = Σ_{k≥1} I[X ≥ k], vì khi X = x có đúng x chỉ thị đầu bằng 1; lấy kỳ vọng hai vế và dùng linearity.

### 2.3 Phương sai

Kỳ vọng nói "trung bình", không nói "dao động". Hai node cache cùng nhận trung bình 1000 request mỗi giây, một node luôn quanh 1000 còn node kia lúc 0 lúc 2000, là hai thế giới khác nhau. Phương sai định lượng sự khác biệt đó: Var[R] = E[(R − E[R])²], trung bình của bình phương độ lệch khỏi kỳ vọng. Nó nhỏ khi R luôn gần kỳ vọng và lớn khi R hay lệch xa[^mcs-varmeaning]. Độ lệch chuẩn σ = √Var[R] cùng đơn vị với R.

Ba tính chất cần dùng, chứng minh ngắn (chứng minh của người soạn; đây là các kết quả chuẩn của giáo trình MIT 6.042, §20.3):

1. **Var[R] = E[R²] − E[R]².** Đặt μ = E[R]. Khai triển (R − μ)² = R² − 2μR + μ², lấy kỳ vọng và dùng linearity: E[R²] − 2μ·μ + μ² = E[R²] − μ².
2. **Var[aR + b] = a² · Var[R].** Vì aR + b − E[aR + b] = a(R − μ); bình phương rồi lấy kỳ vọng.
3. **Cộng phương sai.** Nếu R và S độc lập thì E[RS] = Σₓ Σᵧ xy · Pr[R = x] · Pr[S = y] = (Σₓ x · Pr[R = x]) · (Σᵧ y · Pr[S = y]) = E[R] · E[S]. Khi đó Var[R + S] = E[(R + S)²] − (E[R] + E[S])² = Var[R] + Var[S] + 2(E[RS] − E[R]E[S]) = Var[R] + Var[S]. Lập luận chỉ dùng E[RS] = E[R]E[S] cho *từng cặp*. Vì vậy với n biến độc lập từng đôi một, mọi số hạng chéo triệt tiêu và Var[ΣRᵢ] = ΣVar[Rᵢ][^mcs-addvar]. Khi các biến phụ thuộc thì đẳng thức mất: lấy S = R thì Var[2R] = 4Var[R], khác 2Var[R] trừ khi Var[R] = 0.

**Ví dụ.** Node cache `c₇` nhận request từ 10 000 client; mỗi client gửi một request trong cửa sổ với xác suất 0,1, độc lập (giả định của người soạn). Số request X là Bin(10⁴; 0,1), tổng của 10⁴ biến chỉ thị Iₖ. Vì Iₖ² = Iₖ nên Var[Iₖ] = E[Iₖ²] − p² = p − p² = p(1 − p). Suy ra E[X] = 10⁴ × 0,1 = 1000, Var[X] = 10⁴ × 0,1 × 0,9 = 900 và σ = 30: dao động điển hình khoảng ±3% quanh mức trung bình. Ba con số 1000, 900, 30 được dùng lại ở mục 3.

Một hệ quả cho benchmark: trung bình của n mẫu độc lập cùng phương sai σ² có phương sai σ²/n (từ tính chất 2 và 3), nên độ lệch chuẩn của trung bình giảm như 1/√n. Đó là lý do chạy lặp nhiều lần cho ước lượng chắc hơn; [Chương 06](06-thong-ke-cho-benchmark.md) khai thác điều này (nhận xét của người soạn).

<details><summary>Đi sâu: kỳ vọng không giao hoán với hàm phi tuyến</summary>

E[f(X)] nói chung khác f(E[X]). Ví dụ của người soạn: hai request có latency 1 ms và 3 ms, trung bình 2 ms. Nếu đổi sang "throughput một connection" bằng 1/latency, ta có 1000 và 333 request mỗi giây, trung bình 667; trong khi 1/(latency trung bình) = 500. Đảo thứ tự lấy trung bình và đổi đơn vị cho hai kết quả khác nhau, nên khi báo cáo số liệu phải nói rõ lấy trung bình của đại lượng nào.

Bất đẳng thức Jensen phát biểu chính xác hiện tượng đó: nếu f lồi thì E[f(X)] ≥ f(E[X]). Mục 6 dùng nó để chứng minh công thức Bloom filter là một cận dưới.

</details>

**Quay lại Q2:** kỳ vọng của tổng bằng tổng kỳ vọng mà không cần độc lập, nên một `multi_get` 100 key trên 100 node chạm trung bình 100 × (1 − 0,99¹⁰⁰) ≈ 63,4 node; phương sai thì chỉ cộng được khi các số hạng độc lập từng đôi một, và với 10 000 client tải một node có kỳ vọng 1000, phương sai 900.

---

## 3. Markov, Chebyshev và Chernoff: chặn đuôi phân phối (Q3)

> **Trả lời ngắn.** Ba bất đẳng thức này cùng trả lời một câu hỏi: biến ngẫu nhiên có thể vượt xa mức trung bình với xác suất tối đa là bao nhiêu? Markov chỉ cần biết biến không âm và kỳ vọng của nó; Chebyshev dùng thêm phương sai; Chernoff đòi các thành phần độc lập, đổi lại cho cận giảm theo hàm mũ chứ không theo đa thức[^mcs-chernoff-exp]. Với một node cache có kỳ vọng 1000 request mỗi cửa sổ, ba cận cho xác suất vượt 1200 request lần lượt là 0,83; 0,0225 và 6,9·10⁻⁹, trong khi giá trị đúng là 4,8·10⁻¹¹ (quan sát).

**Giải thích.**

*Với `kvmeta`.* Giữ lại node cache `c₇` ở mục 2: số request mỗi cửa sổ X có kỳ vọng 1000, phương sai 900, và node chỉ chịu được 1200 request trước khi hàng đợi phình ra (con số giả định). Ta không muốn biết trung bình mà muốn biết chuyện xấu hiếm đến đâu, tức Pr[X ≥ 1200]. Phân phối đầy đủ thường không biết hoặc quá rắc rối; ba bất đẳng thức dưới đây chặn xác suất đó chỉ bằng một ít thông tin, và càng biết nhiều thì cận càng chặt.

### 3.1 Markov

**Phát biểu.** Nếu R ≥ 0 và x > 0 thì Pr[R ≥ x] ≤ E[R] / x[^mcs-markov].

**Chứng minh (theo MIT 6.042, viết lại).** Với mọi kết cục ta có x · I[R ≥ x] ≤ R. Nếu R ≥ x thì vế trái bằng x ≤ R; nếu R < x thì vế trái bằng 0 ≤ R, và chỗ này cần R không âm. Lấy kỳ vọng hai vế, dùng linearity và E[I] = Pr: x · Pr[R ≥ x] ≤ E[R]. Chia cho x. ∎

Cận Markov không thể cải thiện nếu chỉ biết "không âm" và kỳ vọng: lấy R bằng x với xác suất μ/x và bằng 0 nếu không; khi đó E[R] = μ và Pr[R ≥ x] = μ/x, đúng bằng cận[^mcs-markov-tight]. Ví dụ: nếu latency trung bình của một lệnh `get` là 2 ms thì không quá 10% lệnh chậm hơn 20 ms, chỉ cần biết latency không âm. Với `c₇`: Pr[X ≥ 1200] ≤ 1000/1200 ≈ 0,83, một cận gần như vô dụng, và đó là mức ta đạt được khi chỉ biết kỳ vọng.

### 3.2 Chebyshev

**Phát biểu.** Với mọi biến ngẫu nhiên R có phương sai hữu hạn và t > 0: Pr[|R − E[R]| ≥ t] ≤ Var[R] / t².

**Chứng minh.** Mẹo là áp Markov cho một hàm của R thay vì chính R[^mcs-cheb-trick]. Biến (R − μ)² không âm, và |R − μ| ≥ t khi và chỉ khi (R − μ)² ≥ t². Do đó Pr[|R − μ| ≥ t] = Pr[(R − μ)² ≥ t²] ≤ E[(R − μ)²] / t² = Var[R] / t². ∎

Chebyshev không cần độc lập, chỉ cần biết phương sai (mà ta tính được bằng quy tắc ở mục 2). Đặt t = kσ ta được cận 1/k² cho độ lệch từ k độ lệch chuẩn trở lên. Với `c₇`: biến cố X ≥ 1200 nằm trong biến cố |X − 1000| ≥ 200, nên Pr[X ≥ 1200] ≤ 900 / 200² = 0,0225. Cận này tốt hơn Markov 37 lần, đổi lại ta phải biết phương sai.

Một hệ quả quen thuộc: trung bình X̄ của n mẫu độc lập, mỗi mẫu có kỳ vọng μ và phương sai σ², có Var[X̄] = σ²/n; nên Pr[|X̄ − μ| ≥ ε] ≤ σ² / (nε²), tiến về 0 khi n tăng. Đó là luật số lớn dạng yếu, một cách nói "lấy trung bình nhiều mẫu thì chắc" (nhận xét của người soạn).

### 3.3 Chernoff

**Phát biểu.** Cho T₁, …, Tₙ độc lập toàn phần, 0 ≤ Tᵢ ≤ 1, và T = T₁ + … + Tₙ. Với mọi c ≥ 1: Pr[T ≥ c · E[T]] ≤ e^(−β(c) · E[T]), trong đó β(c) = c ln c − c + 1[^mcs-chernoff]. Các Tᵢ không cần cùng phân phối; nhị thức là trường hợp quen nhất[^mcs-chernoff-bin].

**Chứng minh (theo MIT 6.042 §20.5.6, viết lại).** Ý tưởng cốt lõi là *mũ hóa* rồi áp Markov: mũ hóa làm Markov mạnh lên rất nhiều, dù không phải lúc nào cũng vậy[^mcs-chernoff-proof]. Với c > 1 (nếu c = 1 thì cận bằng 1, hiển nhiên đúng), đặt μ = E[T].

1. **Mũ hóa và Markov.** Hàm x ↦ cˣ tăng nghiêm ngặt, nên T ≥ cμ khi và chỉ khi c^T ≥ c^(cμ). Áp Markov cho biến không âm c^T: Pr[T ≥ cμ] ≤ E[c^T] / c^(cμ).
2. **Độc lập biến tổng thành tích.** c^T = c^T₁ · … · c^Tₙ, và hàm của các biến độc lập cũng độc lập, nên E[c^T] = E[c^T₁] · … · E[c^Tₙ].
3. **Chặn từng thừa số.** Với v ∈ [0, 1] và c ≥ 1, hàm lồi c^v nằm dưới dây cung nối (0, 1) với (1, c), tức c^v ≤ 1 + (c − 1)v. Lấy kỳ vọng với v = Tᵢ: E[c^Tᵢ] ≤ 1 + (c − 1)E[Tᵢ] ≤ e^((c−1)E[Tᵢ]), bước cuối nhờ 1 + z ≤ e^z. Đây là chỗ dùng giả thiết 0 ≤ Tᵢ ≤ 1.
4. **Nhân lại.** E[c^T] ≤ e^((c−1)(E[T₁] + … + E[Tₙ])) = e^((c−1)μ), nhờ linearity.
5. **Ghép.** Pr[T ≥ cμ] ≤ e^((c−1)μ) / c^(cμ) = exp((c − 1 − c ln c) · μ) = e^(−β(c)μ). ∎

Nhận xét của người soạn: cơ số c = e^λ với λ = ln c chính là lựa chọn λ tối ưu cho cận E[e^(λT)] ≤ exp((e^λ − 1)μ), vì đạo hàm của (e^λ − 1)μ − λcμ theo λ triệt tiêu đúng khi e^λ = c.

**Số cho `c₇`.** Lấy c = 1,2: β(1,2) = 1,2 · ln 1,2 − 1,2 + 1 ≈ 0,018786, nhân với E[T] = 1000 cho số mũ −18,79, tức cận 6,9·10⁻⁹. Bảng dưới so bốn con số (quan sát: tính bằng chương trình Rust; giá trị "chính xác" là tổng chính xác của phân phối nhị thức, tính bằng log của hàm khối).

| Cận | Cần biết gì | Pr[X ≥ 1200] |
|---|---|---|
| Markov | X ≥ 0, E[X] = 1000 | ≤ 0,833 |
| Chebyshev | thêm Var[X] = 900 | ≤ 0,0225 |
| Chernoff | thêm X là tổng của các thành phần độc lập trong [0, 1] | ≤ 6,9·10⁻⁹ |
| Chính xác | toàn bộ phân phối Bin(10⁴; 0,1) | 4,8·10⁻¹¹ |

Chernoff vẫn cao hơn giá trị đúng khoảng 140 lần, nhưng cùng bậc "mũ" với nó; Markov và Chebyshev thì thua hẳn một phạm trù, vì chúng giảm theo đa thức còn Chernoff giảm theo hàm mũ[^mcs-chernoff-exp].

### 3.4 Đặt capacity: Chernoff cộng union bound

Tầng `kvmeta-cache` có 100 node. Ta muốn biết xác suất *có node nào* vượt 1200 trong một cửa sổ. Biến cố đó là hợp của 100 biến cố "node j vượt 1200", nên union bound (mục 1) cho cận trên bằng 100 lần xác suất của một node, mà không cần các node độc lập với nhau[^mcs-overload]. Với Chernoff: 100 × 6,9·10⁻⁹ = 6,9·10⁻⁷ mỗi cửa sổ. Nếu mỗi cửa sổ dài một giây, một ngày có 86 400 cửa sổ, và theo linearity số cửa sổ xấu kỳ vọng không quá 86 400 × 6,9·10⁻⁷ ≈ 0,06 mỗi ngày, tức cỡ một lần mỗi 17 ngày (suy luận của người soạn từ các con số trên).

Làm ngược lại để chọn capacity: c nhỏ nhất thỏa 100 · e^(−β(c)·1000) ≤ 10⁻⁶ là c ≈ 1,198 (quan sát: chương trình tìm số này bằng vòng lặp tăng dần). Nói cách khác, dự phòng khoảng 20% trên mức trung bình đủ cho mục tiêu này, *với điều kiện* các client độc lập. Chebyshev ở đây vô dụng: 100 × 0,0225 = 2,25 lớn hơn 1, một cận vô nghĩa. Đó là lý do khi phải union bound qua nhiều đối tượng, cận mũ của Chernoff mới đáng giá.

Cảnh báo (nhận xét của người soạn): giả thiết độc lập giữa các client là điều kiện của Chernoff. Retry storm, cache stampede hay một key quá nóng làm các request tương quan với nhau, và khi đó 20% dự phòng ở trên là ảo tưởng; hãy kiểm lại bằng phân phối đo thật ([P11 · Chương 03](../11-do-dac-va-go-loi/03-hieu-nang-service-rust.md)).

<details><summary>Đi sâu: chương trình tính bốn cận, và Markov với đuôi nặng</summary>

**Chương trình.** Phần tính "chính xác" cộng hàm khối của nhị thức, mỗi số hạng tính qua logarit để khỏi tràn số:

```rust
fn ln_fact_table(n: usize) -> Vec<f64> {
    let mut t = vec![0.0f64; n + 1];
    for i in 1..=n {
        t[i] = t[i - 1] + (i as f64).ln();
    }
    t
}

fn binom_tail_exact(n: usize, p: f64, k0: usize) -> f64 {
    let lf = ln_fact_table(n);
    let mut tot = 0.0;
    for k in k0..=n {
        let lp = lf[n] - lf[k] - lf[n - k] + k as f64 * p.ln() + (n - k) as f64 * (1.0 - p).ln();
        tot += lp.exp();
    }
    tot
}
```

Output (quan sát, phần A của `numbers.rs`, macOS arm64, `rustc 1.95.0 -O`):

```text
== A. Markov / Chebyshev / Chernoff cho X ~ Bin(10000, 0.1) (mu = 1000, Var = 900), P(X >= 1200) ==
Markov:    mu/1200            = 0.8333
Chebyshev: Var/200^2          = 0.0225
Chernoff:  exp(-beta(c) mu)   = 6.941e-9 (beta(1.2) = 0.018786)
Chinh xac: P(Bin >= 1200)     = 4.803e-11
Union bound 100 node: 100 * Chernoff = 6.941e-7
c nho nhat de 100*exp(-beta(c)*1000) <= 1e-6: c = 1.1980
```

**Đuôi nặng.** Markov và Chebyshev vẫn đúng với phân phối đuôi nặng (latency thật thường như vậy) miễn là kỳ vọng, và phương sai với Chebyshev, hữu hạn; Chernoff thì không áp dụng được khi các số hạng không bị chặn trong [0, 1] hay không độc lập. Hệ quả cho vận hành (nhận xét của người soạn): biết latency trung bình không đủ để suy ra p99. Markov chỉ cho Pr[latency ≥ 100 × trung bình] ≤ 1%, và cận đó đạt được ở một số phân phối[^mcs-markov-tight]. Muốn nói về p99 thì phải đo p99, không suy từ trung bình.

</details>

**Quay lại Q3:** Markov (chỉ cần kỳ vọng) cho Pr[X ≥ 1200] ≤ 0,83, Chebyshev (thêm phương sai) cho ≤ 0,0225, Chernoff (thêm độc lập) cho ≤ 6,9·10⁻⁹ so với giá trị đúng 4,8·10⁻¹¹; ghép Chernoff với union bound qua 100 node cho thấy dự phòng khoảng 20% trên trung bình là đủ, miễn là các client thật sự độc lập.

---

## 4. Balls into bins và birthday paradox: tải, hash và va chạm (Q4)

> **Trả lời ngắn.** Ném m quả bóng độc lập, đều vào n cái thùng là mô hình chuẩn của việc băm m request hay m khóa vào n node. Khi m = n, khoảng 1/e ≈ 36,8% thùng rỗng, nhưng thùng nặng nhất không chứa 1 quả mà cỡ ln n / ln ln n quả với xác suất cao[^p2c-one]; ta chứng minh được một cận trên 3 ln n / ln ln n. Còn nếu "thùng" là các giá trị băm hay UUID thì hai món cùng rơi vào một thùng (va chạm) xảy ra sớm: với d giá trị khả dĩ, chỉ cần cỡ √d món là xác suất va chạm đã xấp xỉ một nửa. Đó là birthday paradox, một mô hình tốt cho va chạm của các món được chèn ngẫu nhiên vào hash table[^mcs-bday-hash].

**Giải thích.**

*Với `kvmeta`.* Tầng `kvmeta-cache` có 100 node. Vào một khoảnh khắc có 100 request đang bay, mỗi request đã được băm vào một node. Tải trung bình là một request mỗi node, và ta hay hình dung chúng "rải đều". Mục này cho thấy vì sao đó là ảo giác, và cùng một bộ toán ấy trả lời luôn câu hỏi hai request id có đụng nhau không (nhận xét của người soạn).

### 4.1 Thùng rỗng

Mục 2 đã tính số thùng bị chạm; phần bù của nó là số thùng rỗng. Với m quả bóng vào n thùng, số thùng rỗng kỳ vọng là n · (1 − 1/n)^m. Để thấy nó gần n · e^(−m/n), dùng bất đẳng thức 1 − x ≤ e^(−x). Chứng minh bất đẳng thức này: hàm f(x) = e^(−x) − 1 + x có f(0) = 0 và f'(x) = 1 − e^(−x), âm khi x < 0 và dương khi x > 0, nên f đạt cực tiểu 0 tại x = 0. Từ đó (1 − 1/n)^m ≤ e^(−m/n), và cận này sát khi n lớn vì (1 − 1/n)ⁿ tiến về e⁻¹.

Với m = n, có khoảng n/e, tức 36,8% thùng rỗng. Với 100 request trên 100 node, 100 × 0,99¹⁰⁰ ≈ 36,6 node không nhận request nào (quan sát: tỉ lệ rỗng 0.36598 trong 100 000 lần mô phỏng; với n = 10⁶ là 0.36786, so với 1/e = 0.36788).

### 4.2 Thùng nặng nhất

Nếu 37 node rảnh thì các node còn lại gánh nhiều hơn mức trung bình. Node nặng nhất nhận bao nhiêu? Lý thuyết cho biết tải lớn nhất xấp xỉ log n / log log n với xác suất cao[^p2c-one]. Dưới đây là chứng minh một cận trên tường minh.

**Định lý (chứng minh của người soạn).** Với n ≥ 3, đặt k = ⌈3 ln n / ln ln n⌉. Ném n quả bóng độc lập, đều vào n thùng. Khi đó Pr[có thùng nhận ít nhất k quả] ≤ 1/n.

**Chứng minh.**

1. *Một thùng cố định.* Thùng 1 nhận ít nhất k quả khi và chỉ khi tồn tại một tập S gồm k quả cùng rơi vào nó. Với S cố định, xác suất đó là n^(−k). Có C(n, k) tập S, nên theo union bound, Pr[thùng 1 có ≥ k quả] ≤ C(n, k) · n^(−k).
2. *Chặn hệ số nhị thức.* C(n, k) = n(n−1)…(n−k+1) / k! ≤ nᵏ / k!, nên Pr[thùng 1 có ≥ k quả] ≤ 1/k!.
3. *Chặn giai thừa.* Chuỗi eᵏ = Σⱼ kʲ/j! có số hạng j = k bằng kᵏ/k!, và mọi số hạng đều dương, nên eᵏ ≥ kᵏ/k!. Vậy k! ≥ (k/e)ᵏ và 1/k! ≤ (e/k)ᵏ.
4. *Mọi thùng.* Union bound lần nữa, trên n thùng: Pr[có thùng ≥ k quả] ≤ n · (e/k)ᵏ.
5. *Quy về một bất đẳng thức một biến.* Ta cần (e/k)ᵏ ≤ n⁻², tức k(ln k − 1) ≥ 2 ln n. Đặt L = ln n và u = ln L; n ≥ 3 cho L > 1 nên u > 0. Với k₀ = 3L/u thì ln k₀ = ln 3 + u − ln u, nên k₀(ln k₀ − 1) = 3L · (1 + (ln 3 − 1 − ln u)/u). Bất đẳng thức k₀(ln k₀ − 1) ≥ 2L tương đương (ln 3 − 1 − ln u)/u ≥ −1/3, tức h(u) = u/3 − ln u + ln 3 − 1 ≥ 0. Hàm h có h'(u) = 1/3 − 1/u, triệt tiêu tại u = 3, và h''(u) = 1/u² > 0 nên đó là cực tiểu toàn cục; mà h(3) = 1 − ln 3 + ln 3 − 1 = 0. Vậy h(u) ≥ 0 với mọi u > 0.
6. *Làm tròn lên.* k ≥ k₀. Hàm g(k) = k(ln k − 1) tăng khi k ≥ 1 (g'(k) = ln k ≥ 0), và k₀ = 3L/ln L ≥ 3e vì L/ln L đạt cực tiểu e tại L = e. Do đó g(k) ≥ g(k₀) ≥ 2 ln n.
7. *Ghép.* Pr[có thùng ≥ k quả] ≤ n · (e/k)ᵏ ≤ n · n⁻² = 1/n. ∎

Chứng minh dùng union bound hai lần và không cần biết gì về quan hệ giữa các thùng. Chiều ngược lại (tải lớn nhất cũng *không nhỏ hơn* cỡ ln n / ln ln n) có trong nguồn đã dẫn ở trên; chương này không chứng minh nó.

**Số liệu.** Cận chứng minh được và con số quan sát (mỗi quả bóng chọn một thùng):

| n | k chứng minh được (cận 1/n) | ln n / ln ln n | Tải lớn nhất quan sát |
|---|---|---|---|
| 100 | 10 | 3,0 | 3–10; phổ biến nhất là 4 (57% trong 100 000 lần) |
| 10 000 | 13 | 4,1 | 5–10; 6 hoặc 7 chiếm 90% (2 000 lần) |
| 1 000 000 | 16 | 5,3 | 8–11; 9 (51 lần) và 8 (38 lần) trong 100 lần |

Cận 3 ln n / ln ln n đúng nhưng lỏng khoảng hai lần. Điều đáng chú ý hơn: số hạng chính ln n / ln ln n *thấp hơn* con số quan sát ở các n thực dụng, nên không nên dùng nó làm ước lượng điểm cho n cỡ trăm đến triệu (quan sát). Một ước lượng thực dụng hơn (suy luận của người soạn, rồi đối chiếu quan sát): xác suất một thùng có ≥ k quả cỡ e⁻¹/k!, nên tải lớn nhất điển hình là k lớn nhất mà n/(e · k!) ≥ 1. Với n = 100 đó là k = 4 (100/(e · 24) ≈ 1,5), với n = 10⁴ là k = 6 và với n = 10⁶ là k = 9 (10⁶/(e · 9!) ≈ 1,01); cả ba rơi vào vùng quan sát (với n = 10⁴, tải 6 và 7 gần ngang nhau). Cũng từ đây ra cỡ ln n / ln ln n: giải k! ≈ n cho k ln k ≈ ln n.

Với `kvmeta`: 100 request đồng thời trên 100 node cho node bận nhất nhận từ 4 request trở lên trong 87% số lần, và từ 6 trở lên trong 5% số lần (quan sát), trong khi tải trung bình chỉ là 1. Nếu node xử lý tuần tự mỗi request 1 ms thì request cuối của node bận nhất chờ từ 4 ms thay vì 1 ms (nhận xét của người soạn).

### 4.3 Birthday paradox: va chạm của hash và id

Cùng bộ toán nhưng câu hỏi khác: n món (request id, khóa, giá trị băm) mỗi món nhận một giá trị đều và độc lập trong d giá trị khả dĩ. Khi nào có hai món trùng? Gọi A là biến cố "có ít nhất một cặp trùng", M là số cặp (i < j) trùng giá trị, và μ = E[M].

**Mệnh đề (chứng minh của người soạn; (c) đi theo hướng của nguồn).**
(a) μ = n(n − 1)/(2d), và Pr[A] ≤ μ.
(b) Pr[A] ≥ 1 − e^(−n(n−1)/(2d)).
(c) Pr[A] ≥ 1 − 1/μ.

**Chứng minh (a).** Với mỗi cặp (i, j), đặt Iᵢⱼ = 1 nếu hai món trùng nhau. Giá trị của món j phải bằng giá trị của món i, nên Pr[Iᵢⱼ = 1] = 1/d. M = ΣIᵢⱼ có C(n, 2) số hạng, và linearity cho μ = C(n, 2)/d mà không cần độc lập giữa các cặp. Vì A = [M ≥ 1], bất đẳng thức Markov cho Pr[A] ≤ E[M] = μ.

**Chứng minh (b).** Món thứ i (đánh số từ 0) phải tránh i giá trị đã bị chiếm, nên xác suất không trùng của nó là (d − i)/d = 1 − i/d; các món độc lập, vậy Pr[không trùng] = ∏_{i=0}^{n−1} (1 − i/d). Dùng 1 − x ≤ e^(−x) cho từng thừa số: tích này không vượt quá ∏ e^(−i/d) = e^(−n(n−1)/(2d)). Lấy phần bù. ∎

**Chứng minh (c).** Các Iᵢⱼ độc lập từng đôi một. Hai cặp không chung chỉ số là hàm của các món rời nhau nên độc lập. Hai cặp chung đúng một chỉ số, chẳng hạn (i, j) và (i, l): Pr[Iᵢⱼ = 1 và Iᵢₗ = 1] = Pr[ba món i, j, l cùng giá trị] = 1/d², đúng bằng Pr[Iᵢⱼ = 1] · Pr[Iᵢₗ = 1]; các tổ hợp còn lại của hai biến chỉ thị suy ra từ đó bằng phép trừ. Nguồn nói điều này bằng lời: biết Alice trùng Bob không cho biết gì về Carol[^mcs-bday-pairwise]. Theo tính chất cộng phương sai ở mục 2, Var[M] = ΣVar[Iᵢⱼ] = C(n, 2) · (1/d)(1 − 1/d) ≤ μ. Nếu M = 0 thì |M − μ| = μ, nên theo Chebyshev, Pr[M = 0] ≤ Var[M]/μ² ≤ 1/μ. ∎

Gộp lại: khi n²/(2d) nhỏ thì Pr[A] xấp xỉ n²/(2d); khi nó lớn thì Pr[A] gần 1. Va chạm chuyển từ hiếm sang gần như chắc chắn quanh n ≈ √d. Giải 1 − e^(−n²/(2d)) = 1/2 cho n ≈ √(2 ln 2 · d) ≈ 1,177 · √d (người soạn tính). Nguồn nêu hệ quả thực dụng đúng như vậy: hash n món vào bảng cỡ d, ta có thể chờ nhiều va chạm nếu n² vượt một phần nhỏ của d[^mcs-bday-many].

**Số liệu** (quan sát: phần B của `numbers.rs`; xác suất tính bằng 1 − e^(−μ), μ = n(n − 1)/(2d)):

| Không gian giá trị d | n | μ | Pr[có va chạm] |
|---|---|---|---|
| 2³² (hash 32 bit) | 10⁴ | 0,0116 | 1,16% |
| 2³² | 77 163 | 0,693 | 50% |
| 2³² | 10⁶ | 116 | ≈ 100% |
| 2⁶⁴ (hash hoặc id 64 bit) | 10⁸ | 2,7·10⁻⁴ | 0,027% |
| 2⁶⁴ | 10⁹ | 0,027 | 2,7% |
| 2⁶⁴ | 10¹⁰ | 2,71 | 93% |
| 2¹²² (UUIDv4) | 10¹² | 9,4·10⁻¹⁴ | 9,4·10⁻¹⁴ |
| 2¹²² | 10¹⁵ | 9,4·10⁻⁸ | 9,4·10⁻⁸ |

UUIDv4 có thể được sinh bằng đúng 122 bit ngẫu nhiên, rồi ghép thêm các bit version và variant vào vị trí quy định[^uuid-122]; vì thế d = 2¹²². Mốc 50% là n ≈ 2,7·10¹⁸. Nếu muốn xác suất va chạm không quá 10⁻⁹ (từ n ≈ √(2dp), người soạn tính) thì UUIDv4 cho phép n cỡ 10¹⁴, còn id ngẫu nhiên 64 bit chỉ cho phép n cỡ 1,9·10⁵.

Quan sát bằng mô phỏng, với d = 2³² và 300 lần chạy: số giá trị sinh ra tới lúc có va chạm đầu tiên có trung bình 80 723 và trung vị 75 150 (nhỏ nhất 3 909, lớn nhất 247 006). Trong 77 163 giá trị đầu, 53,0% số lần đã có va chạm; công thức dự đoán 50%, và với 300 lần chạy sai số mẫu cỡ ±3 điểm phần trăm, nên hai số khớp nhau.

Ba hệ quả cho người làm backend (nhận xét của người soạn):

1. Trong hash table, va chạm là chuyện bình thường; hàm băm chỉ chọn bucket, còn so sánh khóa thật mới quyết định hai khóa có bằng nhau không.
2. Id ngẫu nhiên 64 bit dùng làm khóa duy nhất toàn cục chỉ ổn cho khoảng 10⁵ đối tượng nếu muốn xác suất trùng ≤ 10⁻⁹; với 10⁹ đối tượng, xác suất đã là 2,7%. Cần nhiều bit hơn, hoặc cấp phát có phối hợp.
3. UUIDv4 an toàn về va chạm *nếu* nguồn ngẫu nhiên tốt. Giả thiết "đều và độc lập" là điều kiện của toàn bộ tính toán: hai máy cùng seed RNG là đủ làm nó sụp.

<details><summary>Đi sâu: bài toán gốc 365 ngày, vì sao ra ln n / ln ln n, và liên hệ với consistent hashing</summary>

**Bài toán gốc.** Với d = 365 và n = 95 sinh viên, xác suất có hai người trùng sinh nhật là hơn 0,9999, dù cảm giác ban đầu thường đoán thấp hơn nhiều[^mcs-bday-95]. Với n = 23, tích chính xác cho 0,5073 còn công thức 1 − e^(−n(n−1)/(2d)) cho 0,5000 (quan sát: `numbers.rs`); hai số chênh nhau khoảng 0,7 điểm phần trăm.

**Ước lượng k! ≈ n/e.** Số thùng có tải đúng k xấp xỉ n · e⁻¹/k! (phân phối Poisson trung bình 1, cho m = n; đây là ước lượng của người soạn, không phải định lý ở trên). Tải lớn nhất điển hình là k lớn nhất mà biểu thức đó còn ≥ 1:

| n | n/e | k = 4 | k = 5 | k = 6 | k = 7 | k = 8 | k = 9 | k = 10 |
|---|---|---|---|---|---|---|---|---|
| 100 | 36,8 | 1,5 | 0,31 | | | | | |
| 10 000 | 3 679 | | | 5,1 | 0,73 | | | |
| 1 000 000 | 367 879 | | | | | 9,1 | 1,01 | 0,10 |

(Mỗi ô là n/(e · k!), ô trống không cần cho kết luận. Tải điển hình là k lớn nhất có giá trị ≥ 1: lần lượt 4, 6 và 9.)

**Liên hệ với consistent hashing.** Việc đặt node ngẫu nhiên lên vòng băm cũng là một bài toán ném bóng, và mất cân bằng của nó là lý do virtual node ra đời; xem [P12 · Chương 06, mục 3](../12-he-thong-du-lieu/06-partitioning.md).

**Chương trình đã chạy.** Output phần B và C của `numbers.rs` (macOS arm64, `rustc 1.95.0 -O`, một lần chạy; phần C dùng bộ sinh splitmix64 với seed cố định):

```text
== B. Birthday: xac suat co it nhat mot va cham khi n gia tri ngau nhien deu trong d gia tri ==
                        tinh huong          n      n(n-1)/2d 1-e^{-n(n-1)/2d}
             hash 32 bit, n = 10^4    1.000e4      1.1640e-2        1.1573e-2
            hash 32 bit, n = 77163    7.716e4      6.9314e-1        5.0000e-1
             hash 32 bit, n = 10^6    1.000e6      1.1642e2         1.0000e0
             hash 64 bit, n = 10^8    1.000e8      2.7105e-4        2.7101e-4
             hash 64 bit, n = 10^9    1.000e9      2.7105e-2        2.6741e-2
            hash 64 bit, n = 10^10   1.000e10      2.7105e0        9.3350e-1
       UUIDv4 (122 bit), n = 10^12   1.000e12     9.4040e-14       9.4040e-14
       UUIDv4 (122 bit), n = 10^15   1.000e15     9.4040e-8        9.4040e-8
n cho P = 50% (n ~ sqrt(2 ln2 d)): d=2^32: 77163; d=2^64: 5.0569e9; d=2^122: 2.7149e18
n cho P = 1e-9 (n ~ sqrt(2 d p)): d=2^64: 1.9208e5; d=2^122: 1.0312e14
23 nguoi, d=365: P(trung) chinh xac = 0.5073; 1 - e^(-n(n-1)/2d) = 0.5000

== C. Mo phong: lan va cham dau tien khi sinh gia tri 32 bit ngau nhien (d = 2^32) ==
300 lan chay: trung binh so gia tri sinh ra den khi co va cham dau tien = 80723; trung vi = 75150; nho nhat = 3909; lon nhat = 247006
tham chieu: sqrt(2 ln2 * 2^32) = 77163; sqrt(2^32) = 65536
ti le lan chay co va cham trong <= 77163 gia tri dau: 0.530
```

</details>

**Quay lại Q4:** ném n quả bóng vào n thùng thì khoảng 36,8% thùng rỗng, thùng nặng nhất có tải cỡ ln n / ln ln n (ta chứng minh được cận trên 3 ln n / ln ln n, quan sát 8–11 ở n = 10⁶); còn va chạm hash hay UUID đến sớm vì số cặp tăng như n²: xác suất cỡ n²/(2d), nên 32 bit va chạm ở cỡ 77 000 món, 64 bit ở cỡ 5·10⁹, còn UUIDv4 thì ở cỡ 3·10¹⁸.

---

## 5. Power of two choices: hỏi hai node thay vì một (Q5)

> **Trả lời ngắn.** Nếu mỗi request chọn ngẫu nhiên hai node rồi gửi tới node đang ít việc hơn, tải của node nặng nhất giảm từ cỡ ln n / ln ln n xuống cỡ ln ln n / ln 2 cộng một hằng số, một con số gần như không tăng theo n[^p2c-abku]. Từ một lên hai lựa chọn là bước nhảy lớn; thêm lựa chọn thứ ba, thứ tư chỉ giảm thêm một hằng số[^p2c-const]. Vì vậy load balancer thật như NGINX có chế độ `random two`: chọn ngẫu nhiên hai backend rồi chọn một trong hai theo phương pháp đã cấu hình[^nginx-two]. Phương pháp mặc định là chọn backend có ít kết nối đang hoạt động hơn[^nginx-def].

**Giải thích.**

*Với `kvmeta`.* Client của `kvmeta-cache` phải chọn node cho từng request. Băm ngẫu nhiên thuần túy cho tải nặng nhất cỡ ln n / ln ln n như mục 4. Chọn node ít việc nhất toàn cụm thì đòi biết tải của mọi node, mà hỏi tải một server có thể tốn một message khứ hồi và một lần xử lý interrupt ở phía server[^p2c-query]. Power of two choices (P2C) nằm giữa hai cực đó: hỏi đúng hai node.

### 5.1 Phát biểu

Mô hình: n request lần lượt đến; mỗi request chọn d node độc lập, đều, ngẫu nhiên, rồi đi tới node ít việc nhất trong d node đó. Với d ≥ 2, tải lớn nhất là ln ln n / ln d + Θ(1) với xác suất cao[^p2c-abku]. Cận này tối ưu, sai khác một hằng số cộng, trong mọi chiến lược đặt dạng chọn-nhiều-ứng-viên đều[^p2c-opt]. Với d = 1 ta quay về mục 4.

### 5.2 Vì sao hai lựa chọn khác một lựa chọn

Gọi xᵢ là tỉ lệ thùng có ít nhất i quả. Một quả bóng chỉ đạt "độ cao" i + 1 trở lên khi cả d lần chọn đều gặp thùng đã có ít nhất i quả[^p2c-height]. Xác suất đó xấp xỉ xᵢᵈ. Với d = 2, số thùng có ≥ i + 1 quả không vượt quá cỡ n · xᵢ²: tỉ lệ này giảm *bình phương* sau mỗi tầng, nên chỉ sau O(ln ln n) tầng đã dưới 1/n[^p2c-quad]. Nguồn làm chặt lập luận này bằng quy nạp từng tầng, điều kiện hóa trên số thùng ở tầng trước[^p2c-layers]. Phần khó nằm ở chỗ xử lý điều kiện hóa cho đúng[^p2c-challenge]. Phần dưới là phiên bản trực giác, đủ để thấy hình dạng của kết quả; bản chặt phải giữ lại các hằng số mà ta bỏ qua.

Chạy số (người soạn tính): x₂ ≤ 1/2, vì n quả bóng làm đầy tối đa n/2 thùng tới ≥ 2 quả. Sau đó xᵢ₊₁ ≈ xᵢ²:

| i | 2 | 3 | 4 | 5 | 6 | 7 |
|---|---|---|---|---|---|---|
| xᵢ ≲ | 1/2 | 1/4 | 1/16 | 1/256 | 1,5·10⁻⁵ | 2,3·10⁻¹⁰ |

Tức xᵢ ≈ 2^(−2^(i−2)): *số mũ nhân đôi* sau mỗi tầng. Với n = 10⁶, x₆ ≈ 1,5·10⁻⁵ còn lớn hơn 1/n, nhưng x₇ ≈ 2,3·10⁻¹⁰ đã nhỏ hơn 1/n nhiều: kỳ vọng số thùng có tải ≥ 7 là 10⁶ × 2,3·10⁻¹⁰ ≈ 2,3·10⁻⁴, tức tải lớn nhất không quá 6. Với d = 1, tỉ lệ thùng có ≥ i quả cỡ 1/i! (mục 4.2): giảm kiểu giai thừa. Từ 1/i! xuống dưới 1/n cần i cỡ ln n / ln ln n; từ phép bình phương liên tiếp thì chỉ cần i cỡ ln ln n / ln 2. Toàn bộ "phép màu" của P2C là đổi từ giảm theo giai thừa sang giảm theo bình phương (nhận xét của người soạn).

### 5.3 Quan sát

Mô phỏng: n request, mỗi request chọn d node (có hoàn lại, hòa thì lấy node chọn trước) và đi vào node ít request nhất.

| n (số lần chạy) | d = 1: tải lớn nhất | d = 2: tải lớn nhất | d = 3: tải lớn nhất |
|---|---|---|---|
| 100 (100 000) | 3–10; phổ biến nhất 4 (57%) | 2 (41%), 3 (58%), 4 (0,2%) | 2 (95%), 3 (5%) |
| 10⁴ (2 000) | 5–10; 6 hoặc 7 chiếm 90% | 3 (93%), 4 (7%) | 2 (0,7%), 3 (99,3%) |
| 10⁶ (100) | 8–11; 9 (51 lần), 8 (38 lần) | 4 (cả 100 lần) | 3 (cả 100 lần) |

Với n = 10⁶, chỉ nhờ hỏi thêm một node, tải lớn nhất từ 8–11 xuống 4; so với ln ln n / ln 2 ≈ 3,8. Đồng thời P2C làm giảm số node rảnh: tỉ lệ thùng rỗng là 0,368 (d = 1), 0,238 (d = 2) rồi 0,177 (d = 3), vì request tránh các node đang có việc nên lấp bớt node rảnh.

### 5.4 Trong thực tế

NGINX có chỉ thị `random two` trong `upstream`: nó chọn ngẫu nhiên hai server rồi chọn một trong hai bằng phương pháp được chỉ định[^nginx-two]. Phương pháp mặc định là `least_conn`, tức server có ít kết nối đang hoạt động nhất[^nginx-def].

Vì sao không cứ chọn node ít việc nhất toàn cụm? Ngoài chi phí lấy thông tin còn một lý do toán học: khi thông tin tải cũ (chỉ cập nhật định kỳ), mọi request lao cùng lúc vào vài node trông có vẻ rảnh, và nguồn gọi hiện tượng này là herd behavior[^p2c-herd]. Điều đáng ngạc nhiên mà nguồn ghi lại: với độ trễ cập nhật vừa phải, chọn ngắn nhất trong hai node ngẫu nhiên là chiến lược toàn cục tốt hơn chọn ngắn nhất trong ba[^p2c-three]. P2C chỉ cần tải của hai node, và client có thể dùng số request đang bay do *chính nó* gửi, không cần thêm message nào (nhận xét của người soạn).

Hai giới hạn (nhận xét của người soạn). Thứ nhất, P2C đếm số request chứ không đếm chi phí của chúng; khi request nặng nhẹ khác nhau nhiều, "số request đang bay" không còn đo đúng tải. Thứ hai, mô hình ở đây là n quả bóng đến rồi ở yên; khi request đến và đi liên tục thì bài toán chuyển sang lý thuyết hàng đợi ([Chương 05](05-queueing-theory.md)). Nguồn cũng khảo sát cả các tình huống động đó[^p2c-dyn].

<details><summary>Đi sâu: chương trình mô phỏng balls-into-bins và output đầy đủ</summary>

Hàm lõi mô phỏng một lần ném m quả bóng vào n thùng với d lựa chọn:

```rust
/// Tra ve (so thung rong, max load).
fn run(n: usize, m: usize, d: usize, r: &mut Rng, load: &mut Vec<u32>) -> (usize, u32) {
    load.clear();
    load.resize(n, 0);
    for _ in 0..m {
        let mut best = r.below(n);
        for _ in 1..d {
            let c = r.below(n);
            if load[c] < load[best] {
                best = c;
            }
        }
        load[best] += 1;
    }
    let empty = load.iter().filter(|&&x| x == 0).count();
    let mx = *load.iter().max().unwrap();
    (empty, mx)
}
```

`Rng` là splitmix64 (seed cố định `0xBA11_5B1B_0000_0001`); `below(n)` đổi 53 bit cao của đầu ra thành số nguyên trong [0, n). Hàm `main` chạy `run(n, n, d, …)` với d = 1, 2, 3 và n = 10², 10⁴, 10⁶, rồi in tỉ lệ thùng rỗng và histogram của tải lớn nhất. Output (quan sát, macOS arm64, `rustc 1.95.0 -O`, một lần chạy):

```text
== A. m = n qua bong vao n thung, d lua chon (chon thung it bong nhat trong d thung ngau nhien) ==
        n   d  so_lan    ti_le_rong  ln n/ln ln n                 phan_bo max load (gia_tri:so_lan)
      100   1  100000       0.36598         3.015 3:12721 4:57340 5:24755 6:4506 7:603 8:69 9:4 10:2
      100   2  100000       0.23682         3.015              2:41434 3:58390 4:176
      100   3  100000       0.17545         3.015                     2:94982 3:5018
    10000   1    2000       0.36781         4.148    5:3 6:862 7:948 8:164 9:22 10:1
    10000   2    2000       0.23840         4.148                       3:1855 4:145
    10000   3    2000       0.17694         4.148                        2:14 3:1986
  1000000   1     100       0.36786         5.261               8:38 9:51 10:10 11:1
  1000000   2     100       0.23841         5.261                              4:100
  1000000   3     100       0.17698         5.261                              3:100
tham chieu: (1 - 1/n)^n voi n=100 la 0.36603; 1/e = 0.36788
n =       100: 3 ln n/ln ln n =   9.05; ln ln n/ln 2 =  2.20
n =     10000: 3 ln n/ln ln n =  12.44; ln ln n/ln 2 =  3.20
n =   1000000: 3 ln n/ln ln n =  15.78; ln ln n/ln 2 =  3.79
```

(Các cột của output là tải lớn nhất kèm số lần xuất hiện; bảng ở §5.3 chỉ đổi chúng sang phần trăm.)

</details>

**Quay lại Q5:** hỏi hai node ngẫu nhiên rồi chọn node ít việc hơn đổi quy luật giảm của tỉ lệ node nặng từ giai thừa sang bình phương, nên tải lớn nhất giảm từ cỡ ln n / ln ln n xuống ln ln n / ln 2 + Θ(1); quan sát với 10⁶ node là từ 8–11 xuống 4, và NGINX dùng đúng ý đó trong chế độ `random two`.

---

## 6. Bloom filter: suy ra tỉ lệ dương tính giả và k tối ưu (Q6)

> **Trả lời ngắn.** Bloom filter là một mảng m bit và k hàm băm; sau khi chèn n key, mỗi bit vẫn bằng 0 với xác suất xấp xỉ e^(−kn/m), nên một key *không* có trong tập bị báo "có thể có" oan với xác suất xấp xỉ (1 − e^(−kn/m))^k[^bloom-fp]. Với m và n cho trước, k = ln 2 · (m/n) làm xác suất đó nhỏ nhất; lúc ấy đúng một nửa số bit bằng 1 và xác suất báo oan là (1/2)^k ≈ 0,6185^(m/n)[^bloom-opt]. Công thức dựa trên xấp xỉ "các bit độc lập", và nó thực ra là một cận dưới: một ví dụ nhỏ cho thấy công thức đánh giá thấp xác suất đúng[^bose-under], nhưng khoảng hở rất bé khi m lớn.

**Giải thích.**

*Với `kvmeta`.* Mỗi node Raft của `kvmeta` lưu dữ liệu bằng LSM-tree. Một lần tra key *không tồn tại* (chẳng hạn kiểm tra idempotency key mới) phải xét nhiều SSTable, và mỗi SSTable có một Bloom filter trong RAM để loại nhanh file không chứa key; cơ chế nằm ở [P12 · Chương 03, mục 4](../12-he-thong-du-lieu/03-storage-engine.md). Giả sử một SSTable có n = 100 000 key và ta dành m = 1 000 000 bit (10 bit mỗi key, tức 125 000 byte). Dùng bao nhiêu hàm băm k, và filter sẽ báo oan bao nhiêu phần trăm?

### 6.1 Suy ra công thức

Cách tính dựa trên một giả thiết: các hàm băm h₁, …, h_k độc lập với nhau và đều trên {1, …, m}[^bloom-assume]. Mỗi key chèn vào được băm k lần, nên n key cho kn lần băm.

1. **Một bit cố định vẫn bằng 0.** Bit đó phải bị cả kn lần băm tránh; mỗi lần tránh với xác suất 1 − 1/m, độc lập. Vậy xác suất là p′ = (1 − 1/m)^(kn).
2. **Xấp xỉ.** Từ 1 − x ≤ e^(−x) (mục 4.1) ta có p′ ≤ e^(−kn/m); và p = e^(−kn/m) là một xấp xỉ rất gần của p′, sai khác cỡ O(1/m)[^bloom-approx].
3. **Điều kiện hóa trên trạng thái filter.** Gọi ρ là tỉ lệ bit 0 sau khi chèn xong; ρ là một biến ngẫu nhiên với E[ρ] = p′. Với key y không thuộc tập, k lần băm của y độc lập với trạng thái filter, nên khi đã biết ρ, xác suất cả k bit được kiểm đều bằng 1 đúng bằng (1 − ρ)^k. Vậy xác suất báo oan thật là E[(1 − ρ)^k].
4. **Thay ρ bằng kỳ vọng của nó.** ρ tập trung rất chặt quanh E[ρ][^bloom-conc], nên E[(1 − ρ)^k] ≈ (1 − p′)^k ≈ (1 − p)^k.

Kết quả: f(k) = (1 − e^(−kn/m))^k[^bloom-fp]. Cảnh báo của chính nguồn: nếu số bit 0 thực tế thấp hơn đáng kể so với kỳ vọng thì xác suất báo oan cao hơn con số f vừa tính[^bloom-cond].

Với n = 10⁵ và m = 10⁶ thì kn/m = k/10. Lấy k = 7: e^(−0,7) = 0,4966, nên 1 − e^(−0,7) = 0,5034 và f = 0,5034⁷ ≈ 0,00819, tức 0,82%.

Vì sao có một k tối ưu? Dùng nhiều hàm băm cho một key không thuộc tập nhiều cơ hội gặp bit 0 hơn, nhưng cũng làm mảng đầy bit 1 nhanh hơn; dùng ít hàm băm thì ngược lại[^bloom-force]. Hai lực đó giằng nhau, và đường cong f(k) có dạng chữ U.

### 6.2 Chọn k

Đặt p = e^(−kn/m), tức k = −(m/n) · ln p. Khi đó

```text
ln f = k · ln(1 − p) = −(m/n) · ln p · ln(1 − p).
```

Vì ln p < 0 và ln(1 − p) < 0, tích ln p · ln(1 − p) dương, nên f nhỏ nhất khi tích đó lớn nhất. Tích này đối xứng qua phép đổi p ↔ 1 − p, nên p = 1/2 là một điểm dừng; phần Đi sâu chứng minh nó là cực đại toàn cục. Nguồn cũng đi bằng lập luận đối xứng này[^bloom-opt]. Với p = 1/2: e^(−kn/m) = 1/2, tức **k = (m/n) · ln 2**. Lúc đó ln f = −(m/n)(ln 2)², nên f = e^(−(m/n)(ln 2)²) = (1/2)^k ≈ 0,6185^(m/n), vì (1/2)^(ln 2) = e^(−(ln 2)²) ≈ 0,6185.

k phải là số nguyên, và chọn k nhỏ hơn mức tối ưu một chút cũng chấp nhận được vì tính ít hàm băm hơn[^bloom-int]. Với `kvmeta`, m/n = 10 cho k* = 6,93, nên chọn k = 7 và f ≈ 0,82%.

Đảo chiều thành quy tắc chọn kích thước: muốn tỉ lệ báo oan f thì cần m/n = ln(1/f)/(ln 2)² bit mỗi key và k = log₂(1/f) hàm băm (người soạn rút từ f = (1/2)^k và công thức trên):

| f mục tiêu | bit mỗi key (m/n) | k = log₂(1/f) |
|---|---|---|
| 10% | 4,79 | 3,3 |
| 1% | 9,59 | 6,6 |
| 0,1% | 14,38 | 10,0 |
| 0,01% | 19,17 | 13,3 |

Bảng RocksDB ở [P12 · Chương 03, mục 4.2](../12-he-thong-du-lieu/03-storage-engine.md) ghi 4,9; 9,9 và 15,5 bit mỗi key cho 10%, 1% và 0,1%, hơi cao hơn các giá trị lý thuyết ở đây; chương đó không giải thích phần chênh, và chương này cũng không. Về mặt không gian, Bloom filter nằm trong phạm vi hệ số log₂ e ≈ 1,44 của cận dưới lý thuyết thông tin cho mọi cách biểu diễn tập hợp[^bloom-space]. Một mốc kiểm tra: với m = 8n, xác suất báo oan chỉ hơn 0,02 một chút[^bloom-8n], khớp với (1 − e^(−6/8))⁶ ≈ 0,0216 khi k = 6.

Với `kvmeta`, nếu một lần tra key không tồn tại phải qua L = 8 SSTable, mỗi filter báo oan 0,82%, thì kỳ vọng số lần đọc đĩa thừa là 8 × 0,0082 ≈ 0,066 mỗi lần tra, nhờ linearity (không cần các filter độc lập nhau); không có filter thì có thể là cả 8 lần (nhận xét của người soạn).

### 6.3 Thử bằng chương trình

Chương trình dựng Bloom filter thật với n = 100 000 key, m = 10⁶ bit, k từ 1 đến 12, rồi hỏi 4 triệu key chắc chắn không có trong tập và đếm số lần filter báo "có" (quan sát; hash là splitmix64 áp lên cặp (key, chỉ số hàm băm)):

| k | f theo công thức | f đo được |
|---|---|---|
| 1 | 9,516% | 9,487% |
| 2 | 3,286% | 3,289% |
| 3 | 1,741% | 1,742% |
| 4 | 1,181% | 1,180% |
| 5 | 0,943% | 0,948% |
| 6 | 0,844% | 0,853% |
| 7 | 0,819% | 0,824% |
| 8 | 0,846% | 0,852% |
| 9 | 0,913% | 0,915% |
| 10 | 1,019% | 1,013% |
| 11 | 1,165% | 1,166% |
| 12 | 1,356% | 1,349% |

Đường cong có dạng chữ U và đáy nằm ở k = 7, đúng với k* = 6,93. Ở k = 7, tỉ lệ bit 1 đo được là 0,50361, rất gần 1/2 mà phân tích dự đoán (công thức 0,50341). Số đo nằm trong khoảng 1% so với công thức. Ở k = 6, 7, 8 số đo cao hơn công thức 1,1%, 0,6% và 0,8%; điều đó phù hợp với việc công thức là cận dưới (mục 6.4), nhưng sai số lấy mẫu của 4 triệu truy vấn ở f ≈ 0,8% đã là khoảng 0,55%, nên phép đo này không đủ để khẳng định chiều lệch.

### 6.4 Công thức đúng đến đâu

**Công thức là cận dưới (chứng minh của người soạn).** Xác suất báo oan thật là E[φ(ρ)] với φ(x) = (1 − x)^k, còn công thức Bloom là φ(E[ρ]) = (1 − p′)^k. Hàm φ lồi trên [0, 1] vì φ″(x) = k(k − 1)(1 − x)^(k−2) ≥ 0. Một hàm lồi nằm trên tiếp tuyến của nó: φ(x) ≥ φ(μ) + φ′(μ)(x − μ) với μ = E[ρ]. Lấy kỳ vọng hai vế, số hạng tuyến tính triệt tiêu vì E[ρ − μ] = 0, và còn E[φ(ρ)] ≥ φ(μ). Bất đẳng thức là chặt khi k ≥ 2 và ρ không phải hằng số, vì khi đó φ lồi chặt. Vậy công thức (1 − (1 − 1/m)^(kn))^k luôn nhỏ hơn xác suất thật; đây là bất đẳng thức Jensen cho hàm lồi (đã gặp ở mục 2, Đi sâu). Kết luận này trùng với kết luận của Bose và cộng sự, những người chỉ ra rằng phân tích gốc của Bloom là sai[^bose-wrong]. Chỗ sai nằm ở một giả định ngầm rằng các biến cố "bit được kiểm tra bằng 1" độc lập với nhau[^bose-indep].

**Phản ví dụ nhỏ nhất.** Lấy n = 1, k = 2, m = 2: key x đặt các bit x₁, x₂ ∈ {1, 2}; key y không thuộc tập bị báo oan khi cả y₁ và y₂ rơi vào tập bit mà x đã đặt. Liệt kê 16 tổ hợp (x₁, x₂, y₁, y₂): khi x₁ = x₂ (2 trên 4 tổ hợp của x) chỉ có một bit bị đặt, nên y phải có y₁ = y₂ bằng bit đó (1 trên 4 tổ hợp của y); khi x₁ ≠ x₂ (2 trên 4) cả hai bit bị đặt và mọi y đều bị báo oan (4 trên 4). Tổng cộng 2 × 1 + 2 × 4 = 10 trên 16, tức 5/8. Công thức lại cho (1 − (1 − 1/2)²)² = 9/16[^bose-58]. Quan sát: chương trình liệt kê đúng 16 tổ hợp và đếm 10 trường hợp báo oan.

**Khoảng hở lớn cỡ nào?** Vì ρ tập trung quanh kỳ vọng với phương sai cỡ 1/m, khai triển bậc hai của φ quanh E[ρ] cho khoảng hở tương đối cỡ k²/m (suy luận của người soạn). Với m = 10⁶ nó nhỏ hơn nhiễu lấy mẫu ở mục 6.3 nhiều bậc; với một filter chỉ vài chục bit thì nó đáng kể. Giả thiết "hàm băm hoàn toàn ngẫu nhiên" cũng chỉ là xấp xỉ khi cài đặt: không có k hàm băm độc lập và lý tưởng (nhận xét của người soạn). Hệ quả thực dụng: công thức chọn tham số rất tốt cho filter cỡ lớn như của SSTable, nhưng đừng dùng nó để chứng minh điều gì cho filter nhỏ.

<details><summary>Đi sâu: chứng minh p = 1/2 là cực đại toàn cục, biến thể chia bit, và chương trình</summary>

**p = 1/2 là cực đại toàn cục của G(p) = ln p · ln(1 − p) trên (0, 1)** (chứng minh của người soạn; nguồn chỉ nói "đối xứng" và "đạo hàm"). G đối xứng: G(1 − p) = G(p). Đạo hàm là G′(p) = ln(1 − p)/p − ln p/(1 − p) = H(p) / (p(1 − p)), với H(p) = (1 − p) ln(1 − p) − p ln p. Cần biết dấu của H.

- H(p) → 0 khi p → 0 và khi p → 1 (vì x ln x → 0 khi x → 0), H(1/2) = 0, và H(1 − p) = −H(p).
- H′(p) = −ln(1 − p) − 1 − ln p − 1 = −ln(p(1 − p)) − 2. Nó bằng 0 khi p(1 − p) = e⁻² ≈ 0,135; vì p(1 − p) ≤ 1/4, phương trình bậc hai này có đúng hai nghiệm p₁ < 1/2 < p₂ = 1 − p₁ trong (0, 1). Do đó H′ > 0 trên (0, p₁) và trên (p₂, 1), còn H′ < 0 trên (p₁, p₂).
- Vậy H tăng từ 0 trên (0, p₁) nên dương ở đó; giảm trên (p₁, p₂) từ giá trị dương H(p₁) xuống giá trị âm H(p₂) = −H(p₁), cắt 0 đúng một lần, và vì H(1/2) = 0 nên chỗ cắt là p = 1/2; rồi tăng trên (p₂, 1) lên tới 0 nên âm suốt đoạn đó.
- Kết luận: H > 0 trên (0, 1/2) và H < 0 trên (1/2, 1). Do đó G tăng rồi giảm, và cực đại toàn cục là G(1/2) = (ln 2)².

**Biến thể chia bit.** Có cách cài đặt dành cho mỗi hàm băm một đoạn m/k bit riêng thay vì dùng chung cả mảng. Về mặt tiệm cận cho cùng hiệu năng, nhưng xác suất báo oan luôn không nhỏ hơn cách dùng chung[^bloom-split]; đổi lại cách chia có thể dễ song song hóa truy cập mảng.

**Chương trình đã chạy.** Cấu trúc lõi:

```rust
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

struct Bloom {
    bits: Vec<u64>,
    m: usize,
    k: usize,
}

impl Bloom {
    fn new(m: usize, k: usize) -> Self {
        Bloom { bits: vec![0; (m + 63) / 64], m, k }
    }
    fn pos(&self, key: u64, i: usize) -> usize {
        (mix(key ^ mix(i as u64 + 1)) % self.m as u64) as usize
    }
    fn insert(&mut self, key: u64) {
        for i in 0..self.k {
            let p = self.pos(key, i);
            self.bits[p / 64] |= 1u64 << (p % 64);
        }
    }
    fn contains(&self, key: u64) -> bool {
        (0..self.k).all(|i| {
            let p = self.pos(key, i);
            self.bits[p / 64] >> (p % 64) & 1 == 1
        })
    }
}
```

`main` chèn các key 0…n−1, rồi hỏi các key 10⁹ + q cho q = 0…4·10⁶−1 (đều không thuộc tập), và với mỗi k in ra tỉ lệ bit 1 theo lý thuyết (1 − e^(−kn/m)) và đo được, cùng xác suất báo oan theo công thức và đo được. Phần B liệt kê 16 tổ hợp của phản ví dụ Bose. Output (quan sát, macOS arm64, `rustc 1.95.0 -O`, một lần chạy):

```text
== A. n = 100000 key, m = 1000000 bit (10 bit/key), 4000000 truy van key khong co ==
  k bit1_ly_thuyet bit1_do_duoc FP_cong_thuc   FP_do_duoc
  1        0.09516      0.09509     0.095163     0.094866
  2        0.18127      0.18133     0.032859     0.032889
  3        0.25918      0.25940     0.017411     0.017418
  4        0.32968      0.33003     0.011813     0.011796
  5        0.39347      0.39373     0.009431     0.009476
  6        0.45119      0.45151     0.008436     0.008526
  7        0.50341      0.50361     0.008194     0.008241
  8        0.55067      0.55120     0.008455     0.008521
  9        0.59343      0.59391     0.009127     0.009152
 10        0.63212      0.63229     0.010186     0.010128
 11        0.66713      0.66720     0.011650     0.011658
 12        0.69881      0.69881     0.013561     0.013489
k toi uu ly thuyet = ln2 * m/n = 6.931; FP toi thieu (1/2)^k voi k=ln2*m/n: 0.008193

== B. Liet ke 16 truong hop cua phan vi du Bose et al. (n=1, k=2, m=2) ==
so truong hop = 16, false positive = 10 (10/16), cong thuc (1-(1-1/m)^(kn))^k = 0.5625 = 9/16
```

</details>

**Quay lại Q6:** một bit vẫn bằng 0 sau khi chèn n key với xác suất (1 − 1/m)^(kn) ≈ e^(−kn/m), nên key ngoài tập bị báo oan với xác suất ≈ (1 − e^(−kn/m))^k, nhỏ nhất ở k = (m/n) ln 2 (một nửa số bit bằng 1), khi đó f = (1/2)^k ≈ 0,6185^(m/n); với `kvmeta` 10 bit mỗi key thì k = 7 và f ≈ 0,82%, mô phỏng cho 0,824%, và công thức là cận dưới (Jensen) với khoảng hở rất bé khi m lớn.

---

## 7. Raft: xác suất tách phiếu với election timeout ngẫu nhiên (Q7)

> **Trả lời ngắn.** Khi leader chết, mỗi follower hết giờ ở một thời điểm ngẫu nhiên trong khoảng timeout (ví dụ 150–300 ms), nên thường chỉ một follower hết giờ trước, xin phiếu và thắng trước khi follower khác kịp hết giờ[^raft-random]. Tách phiếu, tức không ai đủ đa số, chỉ xảy ra khi nhiều follower hết giờ sát nhau, trong vòng thời gian một RequestVote đi tới nơi. Với cụm 5 node còn 4 follower và độ trễ một chiều δ cố định, xác suất mỗi vòng là 6ε² − 8ε³ + 3ε⁴ với ε = δ / 150 ms: khoảng 0,03% khi δ = 1 ms, 0,64% khi 5 ms, 2,4% khi 10 ms và 8,9% khi 20 ms. Số vòng bầu là biến hình học[^th-geom2], nên trung bình chỉ 1/(1 − p) vòng.

**Giải thích.**

*Với `kvmeta`.* Cụm Raft 5 node `N1…N5` của `kvmeta` đang chạy với `N1` là leader, rồi `N1` chết (kernel panic chẳng hạn). Bốn follower `N2…N5` đều đang đếm ngược election timeout, mỗi người một số ngẫu nhiên đều trong [150, 300) ms kể từ heartbeat cuối cùng[^th-uniform]. Follower nào về 0 trước sẽ tăng term, thành ứng viên, tự bỏ phiếu cho mình và gửi RequestVote cho những server còn lại. Server nhận được lời xin phiếu khi chưa hết giờ thì bỏ phiếu cho ứng viên; mỗi term nó chỉ có một phiếu, ai đến trước được phục vụ[^raft-fcfs]. Trao phiếu xong, nó đặt lại đồng hồ[^raft-fig2]. Ứng viên thắng khi nhận phiếu của đa số *cả cụm*, tức 3 trên 5[^raft-majority]. Câu hỏi của mục này: xác suất không ai gom đủ 3 phiếu là bao nhiêu, và việc đó làm cuộc bầu kéo dài thêm bao lâu?

### 7.1 Mô hình

Mô hình dưới đây là mô hình "độ trễ cố định" của luận án tác giả Raft[^th-model], với các giả thiết sau.

1. Cụm có n = 5 server; leader vừa chết nên còn s = 4 follower. Ứng viên thắng khi có 3 phiếu.
2. Cả bốn follower nhận heartbeat cuối cùng cùng một lúc. Từ đó mỗi follower đợi một timeout ngẫu nhiên đều trong khoảng [150, 300) ms, độc lập với nhau[^th-uniform]. Chỉ *hiệu* giữa các timeout quyết định chúng có va chạm không, nên ta gọi W = 150 ms là độ rộng khoảng; cận dưới 150 ms chỉ cộng thêm vào thời gian chờ.
3. Độ trễ một chiều giữa hai server là hằng số δ, tính từ lúc ứng viên hết giờ đến lúc server kia xử lý xong lời xin phiếu và đặt lại đồng hồ (cách định nghĩa δ là lựa chọn của người soạn). RequestVote của ứng viên tới mọi server khác đúng sau δ.
4. Server chưa hết giờ khi nhận lời xin phiếu thì bỏ phiếu cho người gửi và đặt lại đồng hồ. Server hết giờ trước đó thì thành ứng viên của cùng term, tự bỏ phiếu cho mình và từ chối mọi lời xin phiếu khác.
5. Log của mọi server đủ mới (không ai từ chối vì log cũ), không mất gói tin, không thêm lỗi nào.

### 7.2 Khi nào tách phiếu

Sắp bốn timeout tăng dần T₍₁₎ ≤ T₍₂₎ ≤ T₍₃₎ ≤ T₍₄₎. Ứng viên đầu tiên C₁ hết giờ lúc T₍₁₎, và lời xin phiếu của nó tới mọi server khác lúc T₍₁₎ + δ. Server j trở thành ứng viên cùng term khi và chỉ khi T_j < T₍₁₎ + δ; mọi server còn lại bỏ phiếu cho C₁. Vì độ trễ cố định, C₁ chắc chắn nhận phiếu của mọi server không hết giờ trong vòng δ sau nó, và không nhận phiếu nào của các ứng viên khác, những người bỏ phiếu cho chính mình[^th-model]. Gọi K là số ứng viên, kể cả C₁. C₁ nhận 1 + (4 − K) phiếu.

| K (số ứng viên cùng term) | Phiếu của C₁ | Kết quả |
|---|---|---|
| 1 | 1 + 3 = 4 | C₁ thắng |
| 2 | 1 + 2 = 3 | C₁ thắng (đủ 3 trên 5) |
| 3 | 1 + 1 = 2 | tách phiếu |
| 4 | 1 | tách phiếu |

Khi K ≥ 3, mỗi ứng viên khác chỉ có phiếu của chính nó và C₁ chỉ gom được tối đa 2 phiếu, nên không ai đạt 3: đây đúng là tình huống ba server hết giờ trong vòng δ thì ứng viên đầu tiên chỉ còn tới được một server khác kịp nhận phiếu[^th-three]. Vậy

```text
P_split = Pr[K ≥ 3] = Pr[T₍₃₎ − T₍₁₎ < δ]
```

tức xác suất để ba timeout nhỏ nhất rơi vào cùng một cửa sổ rộng δ. Tổng quát hơn: xác suất tách phiếu là xác suất để quá nhiều ứng viên hết giờ trong vòng δ của nhau[^th-cond].

Sơ đồ dưới minh họa một lần tách phiếu với δ = 10 ms (con số thời gian là giả định của người soạn, tính từ heartbeat cuối cùng):

```mermaid
sequenceDiagram
  participant N2
  participant N3
  participant N4
  participant N5
  Note over N2: t=168 ms: hết giờ, thành ứng viên, tự bỏ phiếu
  N2->>N3: RequestVote (tới lúc t=178)
  N2->>N4: RequestVote (tới lúc t=178)
  N2->>N5: RequestVote (tới lúc t=178)
  Note over N3: t=172: hết giờ khi chưa nhận được gì, thành ứng viên
  Note over N4: t=177: hết giờ khi chưa nhận được gì, thành ứng viên
  Note over N5: t=178: nhận RequestVote của N2 khi chưa hết giờ, bỏ phiếu cho N2
  Note over N2,N5: N2 có 2 phiếu, N3 có 1, N4 có 1; cần 3: tách phiếu
```

### 7.3 Tính xác suất

**Mệnh đề (chứng minh của người soạn).** Với n = 5, s = 4 và ε = δ/W ≤ 1: P_split = 6ε² − 8ε³ + 3ε⁴.

**Chứng minh.** Chuẩn hóa W = 1, nên bốn timeout T₁, …, T₄ độc lập, đều trên [0, 1) và δ = ε.

1. *Phân phối của timeout nhỏ nhất.* T₍₁₎ > t khi và chỉ khi cả bốn timeout lớn hơn t, xác suất (1 − t)⁴; đây cũng chính là cách luận án tính phân phối của timeout nhỏ nhất[^th-min-cdf]. Mật độ của T₍₁₎ là đạo hàm của 1 − (1 − t)⁴, tức 4(1 − t)³ trên [0, 1].
2. *Ba timeout còn lại, khi biết T₍₁₎ = t.* Chúng độc lập và đều trên [t, 1]: mật độ chung của chúng, giới hạn trong vùng "cả ba lớn hơn t", là một hằng số.
3. *Số timeout rơi vào cửa sổ.* Mỗi timeout trong ba cái đó rơi vào [t, t + ε) với xác suất π(t) = min(1, ε/(1 − t)). Số timeout rơi vào cửa sổ có phân phối Bin(3, π). K ≥ 3 khi và chỉ khi ít nhất 2 trong 3 timeout rơi vào, với xác suất 3π²(1 − π) + π³ = 3π² − 2π³.
4. *Lấy trung bình theo T₍₁₎.* Đổi biến u = 1 − t (mật độ 4u³ trên [0, 1]). Khi u < ε thì π = 1, và khi u ≥ ε thì π = ε/u:

```text
P_split = ∫₀^ε 4u³ · 1 du + ∫_ε^1 4u³ · (3ε²/u² − 2ε³/u³) du
        = ε⁴ + ∫_ε^1 (12ε²u − 8ε³) du
        = ε⁴ + 6ε²(1 − ε²) − 8ε³(1 − ε)
        = 6ε² − 8ε³ + 3ε⁴.   ∎
```

Kiểm tra nhanh: ε = 1 cho 6 − 8 + 3 = 1, đúng với trực giác "mọi timeout đều nằm trong cửa sổ thì chắc chắn tách phiếu". Với ε nhỏ, P_split ≈ 6ε²: cần *ba* timeout cùng rơi vào một cửa sổ hẹp, nên xác suất tỉ lệ với bình phương độ rộng tương đối của cửa sổ chứ không tỉ lệ thuận.

**Đối chiếu với luận án.** Luận án cho ba con số cho khoảng timeout 100 ms và cụm 5 node còn 4 server: khoảng 0,06%, 5,2% và 18,1% các cuộc bầu tách phiếu khi độ trễ một chiều lần lượt là 1, 10 và 20 ms[^th-numbers]. Công thức trên với ε = 0,01; 0,1; 0,2 cho 0,0592%; 5,23%; 18,08%. Quan sát: Monte Carlo của người soạn (2·10⁶ vòng mỗi ô) cho 0,0618%; 5,209%; 18,076%. Ba con số cùng khớp.

**Cho khoảng 150–300 ms (W = 150 ms)** (quan sát: Monte Carlo, độ trễ cố định, 2·10⁶ vòng mỗi ô, so với công thức):

| δ (ms) | ε = δ/150 | P_split theo công thức | P_split đo được |
|---|---|---|---|
| 1 | 0,0067 | 0,0264% | 0,0276% |
| 2 | 0,0133 | 0,105% | 0,103% |
| 5 | 0,0333 | 0,637% | 0,633% |
| 10 | 0,0667 | 2,44% | 2,43% |
| 20 | 0,1333 | 8,87% | 8,86% |

Khoảng hở giữa công thức và số đo luôn nhỏ hơn hai lần sai số lấy mẫu (ví dụ ở δ = 5 ms, sai số chuẩn khoảng 0,006 điểm phần trăm). Nhân đôi độ rộng khoảng timeout, từ 150 lên 300 ms ở δ = 10 ms, đưa ε từ 0,0667 xuống 0,0333 và P_split từ 2,44% xuống 0,637%, giảm 3,8 lần: gần đúng với "tỉ lệ thuận với ε²" (suy ra của người soạn từ bảng).

### 7.4 Số vòng bầu và thời gian đến khi có leader

Khi một vòng tách phiếu, các server đã đặt lại đồng hồ lúc trao phiếu, nên vòng sau có cùng xác suất thành công như một cuộc bầu hoàn toàn mới[^th-vote-reset]. Do đó số vòng R là biến hình học với xác suất thành công 1 − p, trong đó p = P_split[^th-geom2]: Pr[R ≥ r] = p^(r−1). Theo hệ quả ở mục 2 (E[R] = Σ_{r≥1} Pr[R ≥ r]):

```text
E[R] = 1 + p + p² + … = 1/(1 − p).
```

Với δ = 10 ms, p = 0,0244, nên E[R] ≈ 1,025; Pr[R ≥ 2] = 2,4%, Pr[R ≥ 3] = p² ≈ 0,06%.

Về thời gian: timeout nhỏ nhất của s server đều trên [0, W) có kỳ vọng W/(s + 1), vì E[T₍₁₎] = ∫₀^W Pr[T₍₁₎ > t] dt = ∫₀^W (1 − t/W)^s dt = W/(s + 1) (người soạn tính; luận án cũng chỉ ra rằng càng nhiều server thì server đầu tiên hết giờ càng sớm[^th-min]). Với s = 4 và W = 150 ms, đó là 30 ms, nên ứng viên đầu tiên xuất hiện sau khoảng 150 + 30 = 180 ms kể từ heartbeat cuối. Vòng thành công tốn thêm 2δ (lời xin phiếu đi δ, phiếu về δ), còn mỗi vòng thất bại tốn khoảng 180 ms + δ trước khi vòng sau bắt đầu. Công thức xấp xỉ (người soạn):

```text
E[thời gian đến khi có leader] ≈ 180 + 2δ + (p / (1 − p)) · (180 + δ)   (ms, tính từ heartbeat cuối)
```

Quan sát (Monte Carlo cho cả chuỗi bầu cử, độ trễ cố định, 10⁶ cuộc bầu mỗi hàng):

| δ (ms) | p đo được | E[R] đo được | 1/(1 − p) | Pr[R ≥ 2] | Pr[R ≥ 3] | Thời gian TB (ms) | Công thức xấp xỉ (ms) |
|---|---|---|---|---|---|---|---|
| 1 | 0,000274 | 1,00027 | 1,00027 | 0,000274 | 0 | 182,06 | 182,05 |
| 5 | 0,006369 | 1,00641 | 1,00641 | 0,006371 | 0,000038 | 191,16 | 191,19 |
| 10 | 0,024239 | 1,02484 | 1,02484 | 0,024285 | 0,000541 | 204,73 | 204,72 |
| 20 | 0,088769 | 1,09742 | 1,09742 | 0,088702 | 0,007959 | 239,48 | 239,48 |

Cột E[R] đo được khớp 1/(1 − p) đến chữ số cuối, và công thức xấp xỉ khớp thời gian đo được trong vòng 0,03 ms. Hai điều rút ra. Thứ nhất, thời gian đến khi có leader bị chi phối bởi election timeout chứ không bởi tách phiếu: với độ trễ đến 20 ms, tách phiếu chỉ cộng thêm kỳ vọng dưới 20 ms trên nền hơn 200 ms. Thứ hai, thời gian ở đây tính từ heartbeat cuối cùng; nếu đo từ lúc leader sập thì con số nhỏ hơn bằng đoạn từ heartbeat cuối đến lúc sập, tối đa một chu kỳ heartbeat (nhận xét của người soạn).

### 7.5 Độ trễ không cố định, và một cận trên

Mô hình độ trễ cố định có một điểm yếu: nó giả định lời xin phiếu của ứng viên hết giờ sớm hơn luôn tới trước. Khi độ trễ dao động, một lời xin phiếu có thể vượt lời xin phiếu gửi trước nó[^th-overtake], nên ứng viên đầu tiên không còn chắc chắn gom được mọi phiếu còn lại. Vì vậy ta kỳ vọng tỉ lệ tách phiếu cao hơn một chút, do có nhiều cạnh tranh hơn[^th-var]. Luận án không mô hình hóa trường hợp này bằng toán mà dùng mô phỏng[^th-sim]. Ta làm tương tự: mỗi lời xin phiếu có độ trễ độc lập, đều trong [δ/2, 3δ/2] (trung bình vẫn là δ).

**Cận trên không cần giả thiết gì về thứ tự tới.** Nếu mọi độ trễ một chiều đều ≤ δ_max thì tách phiếu chỉ xảy ra khi có ít nhất hai ứng viên, tức khi T₍₂₎ − T₍₁₎ < δ_max: nếu C₁ là ứng viên duy nhất thì nó nhận phiếu của cả ba server còn lại và thắng. Còn Pr[T₍₂₎ − T₍₁₎ ≥ x] = (1 − x/W)⁴ với 0 ≤ x ≤ W (người soạn): điều kiện hóa trên T₍₁₎ = t (chuẩn hóa W = 1), ba timeout còn lại đều phải lớn hơn t + x, xác suất ((1 − t − x)/(1 − t))³, nên Pr = ∫₀^(1−x) 4(1 − t)³ · ((1 − t − x)/(1 − t))³ dt = ∫₀^(1−x) 4(1 − t − x)³ dt = (1 − x)⁴. Vậy

```text
P_split ≤ 1 − (1 − δ_max/W)⁴ ≤ 4 · δ_max/W,
```

bất đẳng thức cuối là bất đẳng thức Bernoulli (1 − ε)⁴ ≥ 1 − 4ε. Bảng dưới (quan sát; độ trễ đều trong [δ/2, 3δ/2], nên δ_max = 1,5δ):

| δ (ms) | Công thức, độ trễ cố định δ | Đo, độ trễ cố định δ | Đo, độ trễ đều [δ/2, 3δ/2] | Cận trên với δ_max = 1,5δ |
|---|---|---|---|---|
| 1 | 0,026% | 0,028% | 0,614% | 3,94% |
| 2 | 0,105% | 0,103% | 1,26% | 7,76% |
| 5 | 0,637% | 0,633% | 3,35% | 18,5% |
| 10 | 2,44% | 2,43% | 7,24% | 34,4% |
| 20 | 8,87% | 8,86% | 16,3% | 59,0% |

Cột "độ trễ đều" cao hơn cột độ trễ cố định (từ 1,8 lần ở δ = 20 ms đến hơn 20 lần ở δ = 1 ms), và vẫn thấp hơn cận trên. Lý do (suy luận của người soạn): khi có đúng hai ứng viên A và B hết giờ sát nhau, hai follower còn lại bỏ phiếu cho ứng viên nào có lời xin tới *trước* ở chỗ mình, và nếu độ lệch giữa hai thời điểm hết giờ nhỏ so với dao động độ trễ thì thứ tự đó gần như ngẫu nhiên; nếu hai follower còn lại chia 1–1 thì mỗi ứng viên chỉ có 2 phiếu, thiếu 1 phiếu để đủ đa số. Ở δ nhỏ, dao động ±δ/2 so với độ lệch hết giờ cỡ δ làm trường hợp K = 2 đóng góp đáng kể; ở δ lớn, trường hợp K ≥ 3 vốn đã lấn át nên khoảng cách thu hẹp. Cận trên 4δ_max/W lỏng vì nó đếm "hai timeout va chạm" (xác suất cỡ 4ε) trong khi tách phiếu thật cần *ba* timeout va chạm hoặc hai timeout cộng thêm một thứ tự tới bất lợi.

### 7.6 Cỡ cụm và số node đang hỏng

Với độ trễ cố định, công thức cho mọi n, s (người soạn). Nó là dạng tổng quát của điều kiện trong luận án: ứng viên đầu tiên nhận phiếu của chính nó cộng phiếu của các server hết giờ muộn hơn nó ít nhất δ[^th-votes]. Chứng minh ở phần Đi sâu:

```text
P_split = Pr[Bin(s, ε) ≥ s − ⌊n/2⌋],  ε = δ/W.
```

Bảng (W = 150 ms, δ = 10 ms, độ trễ cố định; quan sát 2·10⁶ vòng mỗi hàng):

| n (cụm) | s (server sống) | P_split theo công thức | P_split đo được |
|---|---|---|---|
| 3 | 2 | 12,9% | 12,9% |
| 5 | 4 | 2,44% | 2,45% |
| 5 | 3 (đa số tối thiểu) | 18,7% | 18,7% |
| 7 | 6 | 0,51% | 0,51% |
| 7 | 4 (đa số tối thiểu) | 24,1% | 24,1% |
| 9 | 8 | 0,11% | 0,11% |

Ba nhận xét, khớp với những gì luận án nêu. Một: số server hỏng ảnh hưởng rất mạnh đến tỉ lệ tách phiếu, nhất là khi cụm chỉ còn đúng đa số[^th-failures]; cụm 5 node mất 1 server cho 2,44%, mất 2 server cho 18,7%. Hai: với cùng một server hỏng, cụm lớn hơn tách phiếu ít hơn[^th-fewer]: 12,9% (n = 3), 2,44% (n = 5), 0,51% (n = 7), 0,11% (n = 9). Ba: với cùng số server *sống*, cụm lớn hơn lại tách phiếu nhiều hơn[^th-more]: s = 4 cho 2,44% ở n = 5 nhưng 24,1% ở n = 7, vì cần nhiều phiếu hơn nên ít ứng viên hơn đã đủ gây tách. Vì vậy luận án khuyên chọn khoảng timeout sao cho giá trị chấp nhận được khi cụm chỉ còn đa số tối thiểu[^th-worst].

Với `kvmeta`: nếu một follower đang bảo trì rồi đúng lúc đó leader chết, cụm còn s = 3 và P_split = 1 − (1 − ε)³ ≈ 18,7% ở δ = 10 ms. Số vòng kỳ vọng khi đó là 1/(1 − 0,187) ≈ 1,23; đắt thêm khoảng 0,23 × 190 ms ≈ 44 ms trung bình, chứ không phải thảm họa (nhận xét của người soạn).

### 7.7 Không ngẫu nhiên thì sao, và chọn khoảng timeout

Nếu W → 0 (mọi follower dùng cùng một timeout) thì ε ≥ 1 và P_split = 1: vòng nào cũng tách phiếu (quan sát: mô phỏng với W ≈ 10⁻⁹ ms và δ = 1 ms cho P_split = 1,0000). Bài báo gốc cũng ghi nhận như vậy: khi không có ngẫu nhiên, bầu leader luôn mất hơn 10 giây vì tách phiếu liên tục; chỉ thêm 5 ms ngẫu nhiên đã cho thời gian ngừng trung vị 287 ms[^raft-eval]. Thí nghiệm đó chạy trên cụm 5 server với broadcast time cỡ 15 ms[^raft-cluster]. Nó còn được dựng cố ý bất lợi: log các server dài ngắn khác nhau, heartbeat đồng bộ để kích thích tách phiếu[^raft-eval-setup]. Vì thế ta không đem số của nó so với mô hình ở trên; chỉ lấy điều định tính (W càng hẹp càng tệ).

Luận án gắn hiện tượng này với FLP: không giao thức đồng thuận chịu lỗi nào kết thúc một cách tất định trong mô hình bất đồng bộ, và trong Raft điều đó hiện ra dưới dạng tách phiếu có thể lặp lại[^th-flp]. Ngẫu nhiên hóa không thay được bảo đảm tất định đó bằng một cận cứng, nhưng đổi nó thành bảo đảm xác suất: xác suất một cuộc bầu cần ít nhất r vòng giảm theo cấp số nhân, p^(r−1) (suy luận của người soạn từ phân phối hình học). Xem thêm [P15 · Chương 01](../15-thuat-toan-phan-tan/01-flp-va-cac-chan-duoi.md) về FLP.

Hướng dẫn chọn khoảng (nhận xét của người soạn, dựa trên các công thức trên). Luận án khuyến nghị khoảng timeout rộng 10–20 lần độ trễ một chiều[^th-rec]. Theo công thức, W = 10δ (ε = 0,1) cho P_split = 5,2% và W = 20δ (ε = 0,05) cho 1,4% ở cụm 5 node còn 4 server. Muốn P_split ≤ 10⁻³ ở cụm đó cần ε ≤ 0,0130, tức W ≥ 77δ; với δ = 5 ms đó là W ≥ 385 ms. Bài báo khuyên khoảng 150–300 ms và cho biết broadcast time có thể từ 0,5 ms đến 20 ms tùy công nghệ lưu trữ[^raft-bcast-range]; nếu độ trễ một chiều bằng nửa broadcast time thì δ nằm trong 0,25–10 ms, ε trong 0,0017–0,067, và P_split từ 1,7·10⁻⁵ đến 2,4% (tính của người soạn). Nói cách khác, khoảng 150–300 ms giữ P_split dưới vài phần trăm trong cả dải môi trường đó.

<details><summary>Đi sâu: công thức nhị thức tổng quát và chứng minh, mã mô phỏng, output đầy đủ, và những gì mô hình bỏ qua</summary>

**Công thức tổng quát (chứng minh của người soạn).** Luận án dẫn ra xác suất tách phiếu bằng một tổng nhị thức dài và nhận xét rằng "CDF trông giống một phân phối nhị thức, gợi ý có cách suy dễ hơn"[^th-binom]. Mệnh đề sau cho cách đó.

*Mệnh đề.* Cho s timeout độc lập, đều trên [0, 1), ε ∈ [0, 1] và 2 ≤ c ≤ s. Khi đó Pr[T₍c₎ − T₍₁₎ < ε] = Pr[Bin(s, ε) ≥ c − 1].

*Chứng minh.* Đặt D₀ = T₍₁₎, Dᵢ = T₍ᵢ₊₁₎ − T₍ᵢ₎ với 1 ≤ i ≤ s − 1, và D_s = 1 − T₍s₎; các D cộng lại bằng 1. Mật độ chung của (T₍₁₎, …, T₍s₎) là hằng số s! trên miền 0 < t₁ < … < t_s < 1. Phép đổi biến sang (D₀, …, D_{s−1}) là một phép biến đổi tuyến tính có định thức 1, nên (D₀, …, D_{s−1}) có mật độ s! trên đơn hình {dᵢ ≥ 0, d₀ + … + d_{s−1} ≤ 1}. Đơn hình này viết theo s + 1 tọa độ (d₀, …, d_s), với d_s = 1 − Σ, đối xứng khi hoán vị tọa độ nào cũng được (hoán vị d_s với một d_j là một phép biến đổi affine của đơn hình lên chính nó, giữ độ đo). Do đó (D₀, …, D_s) *hoán đổi được* (exchangeable). Hệ quả: tổng c − 1 khoảng liên tiếp D₁ + … + D_{c−1} = T₍c₎ − T₍₁₎ có cùng phân phối với D₀ + … + D_{c−2} = T₍c−1₎. Cuối cùng T₍c−1₎ < ε khi và chỉ khi ít nhất c − 1 trong s timeout rơi vào [0, ε), và số timeout rơi vào [0, ε) là Bin(s, ε). ∎

Với s = 4 và c = 3 ta được Pr[Bin(4, ε) ≥ 2] = 6ε²(1 − ε)² + 4ε³(1 − ε) + ε⁴ = 6ε² − 8ε³ + 3ε⁴, khớp tích phân ở §7.3. Với điều kiện tách phiếu c > s − ⌊n/2⌋, tức c ≥ s − ⌊n/2⌋ + 1, ta được công thức tổng quát P_split = Pr[Bin(s, ε) ≥ s − ⌊n/2⌋] đã dùng ở §7.6; với n = 3, s = 2 nó cho 1 − (1 − ε)².

**Mã mô phỏng.** Hàm `round` mô phỏng một vòng bầu bằng hàng đợi sự kiện nhỏ: mỗi server có một sự kiện hết giờ; server chưa bỏ phiếu khi hết giờ thì thành ứng viên, tự bỏ phiếu và gửi lời xin phiếu cho các server khác (mỗi lời xin có độ trễ lấy từ `Lat`); server chưa bỏ phiếu khi nhận lời xin đầu tiên thì bỏ phiếu cho người gửi.

```rust
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
    fn unif(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Timeout(usize),
    Msg { from: usize, to: usize },
}

#[derive(Clone, Copy)]
enum Lat {
    Fixed(f64),
    Uniform(f64, f64),
}

fn lat(l: Lat, r: &mut Rng) -> f64 {
    match l {
        Lat::Fixed(d) => d,
        Lat::Uniform(lo, hi) => lo + (hi - lo) * r.unif(),
    }
}

/// Mot vong bau cu. Tra ve (thanh_cong, thoi_diem_timeout_dau_tien, thoi_diem_thang_neu_co).
fn round(n: usize, s: usize, a: f64, w: f64, l: Lat, r: &mut Rng) -> (bool, f64, f64) {
    let q = n / 2 + 1;
    let mut ev: Vec<(f64, Kind)> = Vec::with_capacity(s * s);
    let mut first = f64::INFINITY;
    for i in 0..s {
        let t = a + w * r.unif();
        if t < first {
            first = t;
        }
        ev.push((t, Kind::Timeout(i)));
    }
    let mut voted = vec![false; s];
    let mut votes = vec![0usize; s];
    let mut win_time = f64::INFINITY;
    while !ev.is_empty() {
        let mut bi = 0;
        for k in 1..ev.len() {
            if ev[k].0 < ev[bi].0 {
                bi = k;
            }
        }
        let (t, kind) = ev.swap_remove(bi);
        match kind {
            Kind::Timeout(i) => {
                if !voted[i] {
                    voted[i] = true;
                    votes[i] += 1;
                    for j in 0..s {
                        if j != i {
                            let d = lat(l, r);
                            ev.push((t + d, Kind::Msg { from: i, to: j }));
                        }
                    }
                }
            }
            Kind::Msg { from, to } => {
                if !voted[to] {
                    voted[to] = true;
                    votes[from] += 1;
                    if votes[from] >= q && win_time.is_infinite() {
                        win_time = t + lat(l, r);
                    }
                }
            }
        }
    }
    let ok = votes.iter().any(|&v| v >= q);
    (ok, first, win_time)
}

fn binom_tail(s: usize, p: f64, k: usize) -> f64 {
    // P(Bin(s,p) >= k)
    let mut c = vec![1.0f64; s + 1];
    for i in 1..=s {
        c[i] = c[i - 1] * (s - i + 1) as f64 / i as f64;
    }
    let mut tot = 0.0;
    for j in k..=s {
        tot += c[j] * p.powi(j as i32) * (1.0 - p).powi((s - j) as i32);
    }
    tot
}

fn closed_fixed(n: usize, s: usize, eps: f64) -> f64 {
    // P(split | do tre co dinh) = P(Bin(s, eps) >= s - floor(n/2))
    let k = s - n / 2;
    binom_tail(s, eps.min(1.0), k)
}

fn mc_split(n: usize, s: usize, w: f64, l: Lat, trials: usize, r: &mut Rng) -> f64 {
    let mut bad = 0usize;
    for _ in 0..trials {
        let (ok, _, _) = round(n, s, 0.0, w, l, r);
        if !ok {
            bad += 1;
        }
    }
    bad as f64 / trials as f64
}
```

`main` (không đăng lại) khởi tạo `Rng(0x5EED_1234_ABCD_0001)`, chạy `mc_split` với 2·10⁶ vòng mỗi ô cho các bảng A, B, C và 10⁶ cuộc bầu mỗi hàng cho bảng D, trong đó mỗi cuộc bầu lặp `round(5, 4, 150.0, 150.0, …)` đến khi có người thắng và cộng thời gian: vòng thành công cộng `first + 2δ`, vòng thất bại cộng `first + δ`. Output nguyên văn (quan sát, macOS arm64, `rustc 1.95.0 -O`, một lần chạy, tổng cộng khoảng 8 giây):

```text
== A. Kiem tra voi luan an Ongaro (n=5, s=4, khoang timeout W=100 ms, do tre co dinh) ==
 delta_ms      eps      cong_thuc    monte_carlo
      1.0   0.0100       0.000592       0.000618
     10.0   0.1000       0.052300       0.052090
     20.0   0.2000       0.180800       0.180755

== B. n=5, s=4, khoang timeout W=150 ms (150-300 ms) ==
 delta_ms      eps   ct_co_dinh   MC_co_dinh      MC_unif    ub(delta) ub(1.5delta)
      1.0   0.0067     0.000264     0.000276     0.006141     0.026401     0.039404
      2.0   0.0133     0.001048     0.001032     0.012649     0.052276     0.077632
      5.0   0.0333     0.006374     0.006326     0.033467     0.126814     0.185494
     10.0   0.0667     0.024356     0.024321     0.072361     0.241165     0.343900
     20.0   0.1333     0.088652     0.088645     0.162809     0.435832     0.590400

== C. Anh huong cua cum va so server song (W=150 ms, delta=10 ms, do tre co dinh) ==
  n   s      cong_thuc    monte_carlo
  3   2       0.128889       0.129268
  5   4       0.024356       0.024505
  5   3       0.186963       0.187045
  7   6       0.005084       0.005091
  7   4       0.241165       0.241132
  9   8       0.001111       0.001136

== D. Toan bo cuoc bau (n=5, s=4, timeout 150-300 ms): so vong va thoi gian den khi co leader ==
(thoi gian do tu luc cac follower nhan heartbeat cuoi; 1 vong that bai ton them timeout dau tien + delta)
 delta_ms  p_split_MC  TB_vong_MC     1/(1-p) P(>=2 vong) P(>=3 vong)       TB_ms
      1.0    0.000274     1.00027     1.00027    0.000274    0.000000      182.06
      5.0    0.006369     1.00641     1.00641    0.006371    0.000038      191.16
     10.0    0.024239     1.02484     1.02484    0.024285    0.000541      204.73
     20.0    0.088769     1.09742     1.09742    0.088702    0.007959      239.48

== E. Khong ngau nhien (W=0): moi vong tach phieu chac chan khi delta > 0 ==
W ~ 0 ms, delta = 1 ms: P(split) = 1.0000
```

Cột `ub(delta)` trong bảng B là cận 1 − (1 − δ/W)⁴; cột `ub(1.5delta)` là cận với δ_max = 1,5δ, cột duy nhất hợp lệ cho `MC_unif`.

**Mô hình bỏ qua những gì.** (1) Heartbeat cuối đến các follower không đúng cùng lúc, và sai lệch đó dịch các timeout tương đối với nhau (nhận xét của người soạn). (2) Log của các server có thể khác nhau; khi đó có server không đủ điều kiện làm leader và từ chối phiếu, nên các server không nhất thiết đặt lại đồng hồ ở vòng sau[^th-logs]. Thí nghiệm của bài báo cố ý dựng điều kiện này[^raft-eval-setup]. (3) Server ghi `currentTerm` và `votedFor` xuống đĩa khi trao phiếu ([P12 · Chương 18](../12-he-thong-du-lieu/18-thuat-toan-dong-thuan.md), mục 4), thời gian ghi đĩa cộng vào δ; ta đã gộp nó vào định nghĩa của δ. (4) PreVote và CheckQuorum ([P12 · Chương 18](../12-he-thong-du-lieu/18-thuat-toan-dong-thuan.md), mục 6) đổi cách một server được phép trở thành ứng viên; mô hình ở đây không có chúng. (5) Luận án nhấn mạnh rằng các kết luận về bầu cử chỉ đúng với một số giả định: server chọn timeout từ phân phối ngẫu nhiên thật sự (không đồng bộ ngầm với nhau), đồng hồ chạy cùng tốc độ, server và mạng đáp ứng kịp thời hoặc đã dừng hẳn[^th-assume]. Vi phạm một trong các giả định đó thì chương này không còn nói được gì về cuộc bầu.

</details>

**Quay lại Q7:** với 5 node còn 4 follower, tách phiếu cần ít nhất 3 timeout rơi vào cùng một cửa sổ δ (độ trễ cố định), nên P_split = 6ε² − 8ε³ + 3ε⁴ với ε = δ/150 ms: 0,64% ở δ = 5 ms, 2,4% ở 10 ms, 8,9% ở 20 ms, khớp cả công thức của luận án lẫn Monte Carlo 2·10⁶ vòng; số vòng là biến hình học (E = 1/(1 − p)), nên bầu leader mất khoảng 180 ms + 2δ cộng thêm vài chục ms trong trường hợp xấu, và độ trễ dao động hoặc mất một node nữa đẩy P_split lên rõ rệt (7,2% và 18,7% ở δ = 10 ms).

---

## Liên hệ với Rust

Chương này nặng toán hơn Rust, nhưng có ba điểm chạm cụ thể.

**1. Tính xác suất rất nhỏ cho đúng: `exp_m1` và `ln_1p`.** Xác suất va chạm UUID ở mục 4 cỡ 10⁻¹³. Nếu viết `1.0 - (-x).exp()` với x rất nhỏ, ta trừ hai số gần 1 nên mất bớt chữ số có nghĩa. `f64::exp_m1` trả về e^x − 1 chính xác ngay cả khi x gần 0[^rust-expm1]. Còn `f64::ln_1p` tính ln(1 + x) chính xác hơn việc cộng rồi lấy log riêng[^rust-ln1p]. Hàm thứ hai là bạn đồng hành tự nhiên của tích ∏(1 − i/d) ở birthday paradox: lấy log thành tổng Σ ln_1p(−i/d). Chương trình dưới là phép tính ở mục 4, kèm hàm chọn node theo P2C của mục 5:

```rust
use std::sync::atomic::{AtomicUsize, Ordering};

/// Chọn node cho một request: lấy ngẫu nhiên 2 node, chọn node có ít request đang bay hơn.
fn pick_p2c(in_flight: &[AtomicUsize], rng: &mut u64) -> usize {
    let n = in_flight.len();
    let a = next(rng) as usize % n;
    let b = next(rng) as usize % n;
    let la = in_flight[a].load(Ordering::Relaxed);
    let lb = in_flight[b].load(Ordering::Relaxed);
    if la <= lb { a } else { b }
}

/// xorshift64: đủ cho minh họa, không dùng cho mật mã.
fn next(s: &mut u64) -> u64 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    *s
}

fn main() {
    // xác suất va chạm của 10^12 UUIDv4: n(n-1)/(2d) với d = 2^122
    let x = 1e12_f64 * (1e12_f64 - 1.0) / (2.0 * 2f64.powi(122));
    let naive = 1.0 - (-x).exp();
    let stable = -(-x).exp_m1();
    println!("x = {x:e}; naive 1 - e^-x = {naive:e}; exp_m1 = {stable:e}");

    let loads: Vec<AtomicUsize> = (0..100).map(|_| AtomicUsize::new(0)).collect();
    let mut rng = 0x9E3779B97F4A7C15u64;
    for _ in 0..100 {
        let i = pick_p2c(&loads, &mut rng);
        loads[i].fetch_add(1, Ordering::Relaxed);
    }
    let max = loads.iter().map(|l| l.load(Ordering::Relaxed)).max().unwrap();
    println!("100 request vao 100 node, P2C: tai max = {max}");
}
```

Kết quả chạy thật (quan sát, macOS arm64, `rustc 1.95.0 -O`, một lần chạy):

```text
x = 9.403954806568897e-14; naive 1 - e^-x = 9.403589018575076e-14; exp_m1 = 9.403954806568455e-14
100 request vao 100 node, P2C: tai max = 2
```

Cách tính ngây thơ lệch ở chữ số có nghĩa thứ năm so với `exp_m1`; với x nhỏ hơn nữa nó sẽ tệ hơn (suy luận của người soạn: sai số tuyệt đối của phép trừ cỡ 10⁻¹⁶ không đổi, nên sai số tương đối tăng khi x giảm). Dòng thứ hai chỉ là một lần chạy với seed cố định: tải lớn nhất 2, nằm trong các giá trị 2–3 của bảng ở §5.3. Hàm `pick_p2c` chỉ dùng số request đang bay do chính client này gửi, nên không cần thêm message nào để biết tải; đây là điểm lợi của P2C đã nêu ở mục 5 (nhận xét của người soạn).

**2. `HashMap` và giả thiết "băm đều, độc lập".** Toàn bộ mục 4 giả thiết các giá trị băm đều và độc lập. Giả thiết đó sụp khi kẻ tấn công chọn được khóa để gây va chạm có chủ đích. Vì vậy `HashMap` của thư viện chuẩn mặc định dùng thuật toán băm được chọn để chống tấn công HashDoS, và thuật toán đó được seed ngẫu nhiên[^rust-hashmap]. Hệ quả cho `kvmeta` (nhận xét của người soạn): đừng dùng hasher mặc định của `HashMap` để chia khóa ra node cache, vì nó được seed ngẫu nhiên; cần một hàm băm có đặc tả ổn định, xem [P12 · Chương 06](../12-he-thong-du-lieu/06-partitioning.md). Cấu trúc bên trong của `HashMap` nằm ở [P10 · Chương 09](../10-rust-internals/09-collections-internals.md).

**3. Số ngẫu nhiên trong ví dụ.** Các chương trình của chương dùng splitmix64 hoặc xorshift64 viết tay để không cần crate ngoài và để có kết quả lặp lại được với seed cố định. Đó là lựa chọn cho mô phỏng; bộ sinh như vậy không dùng cho mật mã hay sinh UUID thật (nhận xét của người soạn).

## Tóm tắt

- Xác suất cần một không gian mẫu, các biến cố và giả thiết về độc lập; giả thiết độc lập là chỗ dễ sai nhất. Một rack chung làm xác suất mất quorum của cụm 5 node tăng gấp 4 lần so với phép tính độc lập (9,85·10⁻⁶ lên 3,95·10⁻⁵ mỗi ngày, số giả định).
- Union bound, Pr[∪Eᵢ] ≤ ΣPr[Eᵢ], đúng mà không cần độc lập; chứng minh chỉ cần tách các Eᵢ thành các phần rời nhau.
- Linearity of expectation cũng không cần độc lập: `multi_get` 100 key trên 100 node chạm trung bình 100 × (1 − 0,99¹⁰⁰) ≈ 63,4 node. Phương sai chỉ cộng được khi các số hạng độc lập từng đôi một.
- Markov (cần kỳ vọng), Chebyshev (cần phương sai) và Chernoff (cần độc lập) cho Pr[X ≥ 1200] của một node cache lần lượt ≤ 0,83; ≤ 0,0225; ≤ 6,9·10⁻⁹ (giá trị đúng 4,8·10⁻¹¹). Chernoff cộng union bound cho thấy dự phòng cỡ 20% là đủ nếu các client độc lập.
- Balls into bins: với m = n khoảng 36,8% thùng rỗng và thùng nặng nhất cỡ ln n / ln ln n; ta chứng minh được cận 3 ln n / ln ln n (quan sát 8–11 ở n = 10⁶).
- Birthday paradox: va chạm khi n ≈ √d, với xác suất ≈ n²/(2d). Hash 32 bit va chạm ở cỡ 77 000 giá trị, id 64 bit ở cỡ 5·10⁹ (đã 2,7% ở 10⁹), UUIDv4 ở cỡ 3·10¹⁸.
- Power of two choices: hỏi hai node và chọn node ít việc hơn đổi quy luật giảm của tỉ lệ node nặng từ giai thừa sang bình phương, đưa tải lớn nhất xuống ln ln n / ln 2 + Θ(1); quan sát 4 so với 8–11 ở 10⁶ node.
- Bloom filter: f ≈ (1 − e^(−kn/m))^k, k tối ưu = (m/n) ln 2, f_min = (1/2)^k ≈ 0,6185^(m/n); 1% cần 9,59 bit mỗi key. Công thức là cận dưới của xác suất thật (Jensen), nhưng khoảng hở rất bé khi m lớn.
- Raft với 5 node còn 4 follower, độ trễ cố định δ, khoảng timeout rộng W: P_split = 6ε² − 8ε³ + 3ε⁴ (ε = δ/W), khớp luận án và Monte Carlo; số vòng là biến hình học với trung bình 1/(1 − p), và bầu leader mất khoảng 180 ms + 2δ. Một node nữa hỏng đẩy P_split lên 18,7% ở δ = 10 ms.
- Mọi con số xác suất đi kèm giả thiết (độc lập, băm đều, độ trễ cố định…); nên kiểm bằng mô phỏng nhỏ khi có thể, như các chương trình Rust trong chương.

## Câu hỏi tự kiểm tra

1. Hai biến cố rời nhau, cả hai có xác suất dương, có độc lập không? Vì sao?

   <details><summary>Đáp án</summary>

   Không. Nếu A xảy ra thì ta biết chắc B không xảy ra, nên Pr[B | A] = 0 ≠ Pr[B]. Chỉ khi một trong hai biến cố có xác suất 0 thì chúng mới độc lập[^mcs-disjoint].

   </details>

2. Vì sao union bound hữu ích khi tính xác suất "có node nào quá tải" trên 100 node cache, và khi nào nó cho kết quả vô nghĩa?

   <details><summary>Đáp án</summary>

   Nó chặn xác suất của hợp bằng tổng xác suất từng biến cố mà không cần các node độc lập[^mcs-union]. Nó vô nghĩa khi tổng vượt 1, ví dụ khi mỗi node có cận Chebyshev 0,0225 thì 100 × 0,0225 = 2,25 > 1 (§3.4).

   </details>

3. Một `multi_get` đọc 50 key, mỗi key băm đều vào một trong 100 node. Kỳ vọng số node bị chạm là bao nhiêu, và bạn có cần giả thiết gì ngoài "các key độc lập"?

   <details><summary>Đáp án</summary>

   E[X] = 100 × (1 − 0,99⁵⁰) ≈ 100 × (1 − 0,605) ≈ 39,5 node. Không cần gì thêm: các biến chỉ thị "node j bị chạm" phụ thuộc nhau nhưng linearity không cần độc lập[^mcs-lin2].

   </details>

4. Var[Bin(n, p)] bằng bao nhiêu, và vì sao chỉ cần các lần thử độc lập từng đôi một?

   <details><summary>Đáp án</summary>

   np(1 − p). Mỗi biến chỉ thị có phương sai p(1 − p); các số hạng chéo Cov(Iᵢ, Iⱼ) = E[IᵢIⱼ] − E[Iᵢ]E[Iⱼ] bằng 0 khi từng cặp độc lập, nên phương sai của tổng là tổng phương sai[^mcs-addvar].

   </details>

5. Latency trung bình của lệnh `get` là 5 ms. Nếu chỉ biết điều đó (và latency không âm), tối đa bao nhiêu phần trăm lệnh chậm hơn 50 ms?

   <details><summary>Đáp án</summary>

   Theo Markov, không quá 5/50 = 10%[^mcs-markov]. Cận này đạt được ở một số phân phối, nên không thể cải thiện nếu chỉ biết trung bình[^mcs-markov-tight].

   </details>

6. Vì sao khi union bound qua 100 node, cận Chernoff dùng được còn cận Chebyshev thì không?

   <details><summary>Đáp án</summary>

   Chebyshev cho Pr[X ≥ 1200] ≤ 0,0225 mỗi node, nhân 100 được 2,25, một cận vô nghĩa. Chernoff cho 6,9·10⁻⁹ mỗi node, nhân 100 được 6,9·10⁻⁷; cận mũ giảm nhanh hơn cận đa thức[^mcs-chernoff-exp].

   </details>

7. Băm 10⁶ request vào 10⁶ node. Khoảng bao nhiêu node không nhận gì, và tải lớn nhất cỡ nào?

   <details><summary>Đáp án</summary>

   Khoảng 10⁶/e ≈ 368 000 node (36,8%). Tải lớn nhất quan sát được là 8–11; cận chứng minh được là 16, còn số hạng chính ln n / ln ln n ≈ 5,3 thấp hơn quan sát[^p2c-one].

   </details>

8. Dùng id ngẫu nhiên 64 bit cho 10⁹ đối tượng thì xác suất có hai đối tượng trùng id cỡ bao nhiêu? Để xác suất ≤ 10⁻⁹ thì được phép có tối đa bao nhiêu đối tượng?

   <details><summary>Đáp án</summary>

   Cỡ n²/(2d) = 10¹⁸/(2 · 2⁶⁴) ≈ 2,7%. Để ≤ 10⁻⁹ cần n ≲ √(2 · 2⁶⁴ · 10⁻⁹) ≈ 1,9·10⁵. Công thức xuất phát từ birthday paradox[^mcs-bday-many]. UUIDv4 có 122 bit ngẫu nhiên nên dư dả hơn nhiều[^uuid-122].

   </details>

9. Vì sao power of two choices thường được ưa hơn "luôn chọn node ít việc nhất toàn cụm" khi thông tin tải chỉ cập nhật định kỳ?

   <details><summary>Đáp án</summary>

   Thông tin cũ làm mọi request lao cùng lúc vào vài node trông có vẻ rảnh (herd behavior)[^p2c-herd]. Nguồn còn ghi nhận rằng với độ trễ cập nhật vừa phải, chọn ngắn nhất trong hai node ngẫu nhiên là chiến lược toàn cục tốt hơn chọn ngắn nhất trong ba[^p2c-three].

   </details>

10. Cần bao nhiêu bit mỗi key và bao nhiêu hàm băm để Bloom filter báo oan 1%?

    <details><summary>Đáp án</summary>

    m/n = ln(100)/(ln 2)² ≈ 9,59 bit mỗi key và k = log₂(100) ≈ 6,6, làm tròn 7. Công thức k = (m/n) ln 2 và f = (1/2)^k ≈ 0,6185^(m/n) nằm ở nguồn[^bloom-opt].

    </details>

11. Vì sao công thức (1 − (1 − 1/m)^(kn))^k luôn không lớn hơn xác suất báo oan thật?

    <details><summary>Đáp án</summary>

    Xác suất thật là E[(1 − ρ)^k] với ρ là tỉ lệ bit 0 (ngẫu nhiên); công thức là (1 − E[ρ])^k. Hàm (1 − x)^k lồi nên theo Jensen E[(1 − ρ)^k] ≥ (1 − E[ρ])^k (§6.4). Bose và cộng sự cũng chỉ ra phân tích gốc sai, với phản ví dụ n = 1, k = 2, m = 2: thật là 5/8, công thức cho 9/16[^bose-58].

    </details>

12. Cụm Raft 5 node, leader chết, khoảng timeout 150–300 ms, δ = 10 ms. Tính P_split và số vòng kỳ vọng; làm lại khi một follower khác đang bảo trì. Nhân đôi độ rộng khoảng timeout thì P_split đổi thế nào?

    <details><summary>Đáp án</summary>

    ε = 10/150 = 0,0667: P_split = 6ε² − 8ε³ + 3ε⁴ ≈ 2,44% và E[R] = 1/(1 − p) ≈ 1,025. Khi còn 3 follower: P_split = Pr[Bin(3, ε) ≥ 1] = 1 − (1 − ε)³ ≈ 18,7% và E[R] ≈ 1,23. Nhân đôi độ rộng (W = 300 ms) thì ε giảm một nửa, P_split ≈ 6ε² giảm gần 4 lần (2,44% xuống 0,64%). Mô hình và số khớp luận án[^th-numbers] và Monte Carlo của chương. Số vòng là biến hình học[^th-geom2].

    </details>

## Đọc thêm

- MIT 6.042J, *Mathematics for Computer Science* (Lehman, Leighton, Meyer), các chương 17–20: <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf>. Đọc §17.4 (birthday principle), §20.1–20.2 (Markov, Chebyshev), §20.3.5 (đếm cặp trùng bằng phương sai) và §20.5 (Chernoff, cân bằng tải ngẫu nhiên) để có lời giải thích đầy đủ và bài tập.
- Broder & Mitzenmacher, *Network Applications of Bloom Filters: A Survey*, §2: <https://www.eecs.harvard.edu/~michaelm/postscripts/im2005b.pdf>. Phân tích tỉ lệ dương tính giả, k tối ưu, cận dưới không gian, và các ứng dụng mạng.
- Bose và cộng sự, *On the false-positive rate of Bloom filters*: <https://cglab.ca/~morin/publications/ds/bloom-submitted.pdf>. Công thức chính xác và vì sao công thức quen thuộc chỉ là cận dưới.
- Mitzenmacher, Richa, Sitaraman, *The Power of Two Random Choices: A Survey of Techniques and Results*, §1–2 và §4.2.2: <https://www.eecs.harvard.edu/~michaelm/postscripts/handbook2001.pdf>. Chứng minh quy nạp từng tầng và tác động của thông tin tải cũ.
- Ongaro, *Consensus: Bridging Theory and Practice*, chương 9: <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf>. Phân tích đầy đủ xác suất tách phiếu, cụm nhiều kích thước, mô phỏng độ trễ biến thiên và thời gian bầu leader.
- Ongaro & Ousterhout, *In Search of an Understandable Consensus Algorithm (Extended Version)*, §5.2, §5.6, §9.3: <https://raft.github.io/raft.pdf>. Nguồn của election timeout ngẫu nhiên và các số đo ban đầu.
- Trong giáo trình: [P12 · Chương 03, mục 4](../12-he-thong-du-lieu/03-storage-engine.md) (Bloom filter trong LSM-tree), [P12 · Chương 06, mục 3](../12-he-thong-du-lieu/06-partitioning.md) (consistent hashing và virtual node), [P12 · Chương 18](../12-he-thong-du-lieu/18-thuat-toan-dong-thuan.md) (Raft, quorum), [P11 · Chương 03](../11-do-dac-va-go-loi/03-hieu-nang-service-rust.md) (percentile), [Chương 05](05-queueing-theory.md) (hàng đợi) và [Chương 06](06-thong-ke-cho-benchmark.md) (thống kê cho benchmark).

[^mcs-space]: MIT 6.042J, Mathematics for Computer Science (2018), §17.5.1 Probability Spaces — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "A sample space together with a probability function is called a probability space."
[^mcs-event]: MIT 6.042J, Mathematics for Computer Science (2018), §17.2.2 Step 2: Define Events of Interest — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "A set of outcomes is called an event."
[^mcs-indep]: MIT 6.042J, Mathematics for Computer Science (2018), §18.7 Independence — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "In other words, A and B are independent if knowing that B happens does not alter the probability that A happens, as is the case with flipping two coins on opposite sides of a room."
[^mcs-total]: MIT 6.042J, Mathematics for Computer Science (2018), §18.5 The Law of Total Probability — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "The idea is to calculate the probability of an event A by splitting into two cases based on whether or not another event E occurs."
[^mcs-union]: MIT 6.042J, Mathematics for Computer Science (2018), §17.5.2 (Rule 17.5.4, Union Bound) — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "the Union Bound can provide a reassuringly small upper bound on this overall probability of critical failure"
[^mcs-product]: MIT 6.042J, Mathematics for Computer Science (2018), §18.4 Why Tree Diagrams Work — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "So the Conditional Probability Product Rule is the formal justification for multiplying edge probabilities to get outcome probabilities."
[^mcs-disjoint]: MIT 6.042J, Mathematics for Computer Science (2018), §18.7 Independence — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "Disjoint events are never independent—unless one of them has probability zero."
[^mcs-pairwise]: MIT 6.042J, Mathematics for Computer Science (2018), §18.8 Mutual Independence — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "Pairwise independence is a much weaker property than mutual independence."
[^mcs-bday-notmutual]: MIT 6.042J, Mathematics for Computer Science (2018), §20.3.5 Matching Birthdays — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "If Alice matches Bob and Alice matches Carol, it’s certain that Bob and Carol match as well!"
[^mcs-bday-pairwise]: MIT 6.042J, Mathematics for Computer Science (2018), §20.3.5 Matching Birthdays — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "But knowing that Alice’s birthday matches Bob’s tells us nothing about who Carol matches. This means that the events that a pair of people have matching birthdays are pairwise independent"
[^mcs-lin]: MIT 6.042J, Mathematics for Computer Science (2018), §19.5 Linearity of Expectation — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "Its simplest form says that the expected value of a sum of random variables is the sum of the expected values of the variables."
[^mcs-lin2]: MIT 6.042J, Mathematics for Computer Science (2018), §19.5 Linearity of Expectation — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "The great thing about linearity of expectation is that no independence is required. This is really useful, because dealing with independence is a pain, and we often need to work with random variables that are not independent."
[^mcs-varmeaning]: MIT 6.042J, Mathematics for Computer Science (2018), §20.2 Chebyshev’s Theorem — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "So if R is always close to the mean, then the variance will be small. If R is often far from the mean, then the variance will be large."
[^mcs-addvar]: MIT 6.042J, Mathematics for Computer Science (2018), §20.3 Properties of Variance — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "It’s easy to see that additivity of variance does not generally hold for variables that are not independent."
[^mcs-markov]: MIT 6.042J, Mathematics for Computer Science (2018), §20.1 Markov’s Theorem — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "Markov’s theorem gives a generally coarse estimate of the probability that a random variable takes a value much larger than its mean."
[^mcs-markov-tight]: MIT 6.042J, Mathematics for Computer Science (2018), §20.1 Markov’s Theorem — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "Using only these facts, we can’t derive smaller fractions, because there are nonnegative random variables with mean 100 that achieve these fractions."
[^mcs-cheb-trick]: MIT 6.042J, Mathematics for Computer Science (2018), §20.2 Chebyshev’s Theorem — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "a good trick for getting stronger bounds on a random variable R out of Markov’s Theorem is to apply the theorem to some cleverly chosen function of R."
[^mcs-chernoff]: MIT 6.042J, Mathematics for Computer Science (2018), §20.5.1 The Chernoff Bound — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "More precisely, the Chernoff Bound says that the sum of lots of little, independent, random variables is unlikely to significantly exceed the mean of the sum."
[^mcs-chernoff-exp]: MIT 6.042J, Mathematics for Computer Science (2018), §20.5.1 The Chernoff Bound — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "In particular, the Markov and Chebyshev bounds are polynomial, while the Chernoff bound is exponential."
[^mcs-chernoff-bin]: MIT 6.042J, Mathematics for Computer Science (2018), §20.5.1 The Chernoff Bound — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "The binomial distribution is the most well-known distribution that fits these criteria, but many others are possible, because the Chernoff bound allows the variables in the sum to have differing, arbitrary, or even unknown distributions"
[^mcs-chernoff-proof]: MIT 6.042J, Mathematics for Computer Science (2018), §20.5.6 Proof of the Chernoff Bound — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "there is a brilliant idea in this proof: in this context, exponentiating somehow supercharges the Markov bound. This is not true in general!"
[^mcs-overload]: MIT 6.042J, Mathematics for Computer Science (2018), §20.5.5 Randomized Load Balancing — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "The probability that some server is overloaded is at most m times the probability that the first server is overloaded, by the Union Bound"
[^mcs-bday-hash]: MIT 6.042J, Mathematics for Computer Science (2018), §17.4 The Birthday Principle — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "birthday matching is a good model for collisions between items randomly inserted into a hash table"
[^mcs-bday-many]: MIT 6.042J, Mathematics for Computer Science (2018), §17.4 The Birthday Principle — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "you can expect many collisions if n2 is more than a small fraction of d"
[^mcs-bday-95]: MIT 6.042J, Mathematics for Computer Science (2018), §17.4 The Birthday Principle — <https://courses.csail.mit.edu/6.042/spring18/mcs.pdf> — "the probability that there will be two people in the class with matching birthdays is actually more than 0.9999."
[^uuid-122]: RFC 9562, §5.4 UUID Version 4 — <https://www.rfc-editor.org/rfc/rfc9562.html> — "randomly generate the exact required number of bits for random_a, random_b, and random_c (122 bits total) and then concatenate the version and variant in the required position."
[^p2c-one]: Mitzenmacher, Richa, Sitaraman, The Power of Two Random Choices: A Survey of Techniques and Results, §1 — <https://www.eecs.harvard.edu/~michaelm/postscripts/handbook2001.pdf> — "Then the maximum load, or the largest number of balls in any bin, is approximately log n / log log n with high probability."
[^p2c-const]: Mitzenmacher, Richa, Sitaraman, The Power of Two Random Choices: A Survey of Techniques and Results, §1 — <https://www.eecs.harvard.edu/~michaelm/postscripts/handbook2001.pdf> — "having just two random choices (i.e., d = 2) yields a large reduction in the maximum load over having one choice, while each additional choice beyond two decreases the maximum load by just a constant factor"
[^p2c-abku]: Mitzenmacher, Richa, Sitaraman, The Power of Two Random Choices: A Survey of Techniques and Results, §1 — <https://www.eecs.harvard.edu/~michaelm/postscripts/handbook2001.pdf> — "Azar, Broder, Karlin, and Upfal showed that in this case, the maximum load is log log n / log d + Θ(1) with high probability"
[^p2c-opt]: Mitzenmacher, Richa, Sitaraman, The Power of Two Random Choices: A Survey of Techniques and Results, §2 — <https://www.eecs.harvard.edu/~michaelm/postscripts/handbook2001.pdf> — "They also show that this bound is optimal (up to an additive constant term) among all the uniform multiple-choice placement strategies."
[^p2c-query]: Mitzenmacher, Richa, Sitaraman, The Power of Two Random Choices: A Survey of Techniques and Results, §1.1.3 — <https://www.eecs.harvard.edu/~michaelm/postscripts/handbook2001.pdf> — "querying a server for its load may involve sending a message and waiting for a response, processing an interrupt at the server, etc."
[^p2c-height]: Mitzenmacher, Richa, Sitaraman, The Power of Two Random Choices: A Survey of Techniques and Results, §2.1 — <https://www.eecs.harvard.edu/~michaelm/postscripts/handbook2001.pdf> — "A ball has height at least i + 1 only if, for each of the d times it chooses a random bin, it chooses one with load at least i."
[^p2c-quad]: Mitzenmacher, Richa, Sitaraman, The Power of Two Random Choices: A Survey of Techniques and Results, §2.1 — <https://www.eecs.harvard.edu/~michaelm/postscripts/handbook2001.pdf> — "drops at least quadratically at each step, so that after only j = O(log log n) steps the fraction drops below 1/n"
[^p2c-layers]: Mitzenmacher, Richa, Sitaraman, The Power of Two Random Choices: A Survey of Techniques and Results, §2 — <https://www.eecs.harvard.edu/~michaelm/postscripts/handbook2001.pdf> — "we inductively bound the number of bins that contain at least j balls conditioned on the number of bins that contain at least j − 1 balls"
[^p2c-challenge]: Mitzenmacher, Richa, Sitaraman, The Power of Two Random Choices: A Survey of Techniques and Results, §2.1 — <https://www.eecs.harvard.edu/~michaelm/postscripts/handbook2001.pdf> — "The proof is technically challenging primarily because one must handle the conditioning appropriately."
[^p2c-herd]: Mitzenmacher, Richa, Sitaraman, The Power of Two Random Choices: A Survey of Techniques and Results, §4.2.2 Dealing with stale information — <https://www.eecs.harvard.edu/~michaelm/postscripts/handbook2001.pdf> — "The tasks essentially exhibit a herd behavior, moving together in the same fashion, due to the unfortunate feedback given by the system."
[^p2c-three]: Mitzenmacher, Richa, Sitaraman, The Power of Two Random Choices: A Survey of Techniques and Results, §4.2.2 Dealing with stale information — <https://www.eecs.harvard.edu/~michaelm/postscripts/handbook2001.pdf> — "choosing the shortest of two randomly selected processors is a better global strategy than having all tasks choose the shortest from three!"
[^p2c-dyn]: Mitzenmacher, Richa, Sitaraman, The Power of Two Random Choices: A Survey of Techniques and Results, §2 — <https://www.eecs.harvard.edu/~michaelm/postscripts/handbook2001.pdf> — "The layered induction approach in fact proved to be also useful in dynamic scenarios."
[^nginx-two]: nginx, ngx_http_upstream_module, directive random — <https://nginx.org/en/docs/http/ngx_http_upstream_module.html> — "The optional two parameter instructs nginx to randomly select two servers and then choose a server using the specified method."
[^nginx-def]: nginx, ngx_http_upstream_module, directive random — <https://nginx.org/en/docs/http/ngx_http_upstream_module.html> — "The default method is least_conn which passes a request to a server with the least number of active connections."
[^bloom-assume]: Broder & Mitzenmacher, Network Applications of Bloom Filters: A Survey (Internet Mathematics 1(4)), §2.1 Standard Bloom Filters — <https://www.eecs.harvard.edu/~michaelm/postscripts/im2005b.pdf> — "these hash functions map each item in the universe to a random number uniform over the range"
[^bloom-fp]: Broder & Mitzenmacher, Network Applications of Bloom Filters: A Survey (Internet Mathematics 1(4)), §2.1 Standard Bloom Filters — <https://www.eecs.harvard.edu/~michaelm/postscripts/im2005b.pdf> — "The probability of a false positive for an element not in the set, or the false positive rate, can be estimated in a straightforward fashion, given our assumption that hash functions are perfectly random."
[^bloom-approx]: Broder & Mitzenmacher, Network Applications of Bloom Filters: A Survey (Internet Mathematics 1(4)), §2.1 Standard Bloom Filters — <https://www.eecs.harvard.edu/~michaelm/postscripts/im2005b.pdf> — "note that p is a convenient and very close (within O(1/m)) approximation for p"
[^bloom-conc]: Broder & Mitzenmacher, Network Applications of Bloom Filters: A Survey (Internet Mathematics 1(4)), §2.1 Standard Bloom Filters — <https://www.eecs.harvard.edu/~michaelm/postscripts/im2005b.pdf> — "Mitzenmacher shows that, in fact, the fraction of 0 bits is extremely concentrated around its expectation, using a simple martingale argument"
[^bloom-cond]: Broder & Mitzenmacher, Network Applications of Bloom Filters: A Survey (Internet Mathematics 1(4)), §2.1 Standard Bloom Filters — <https://www.eecs.harvard.edu/~michaelm/postscripts/im2005b.pdf> — "simply represents the expected fraction of 0 bits in the array. If the number of 0 bits in the array is substantially less than expected, then the probability of a false positive will be higher than the quantity f that we computed."
[^bloom-force]: Broder & Mitzenmacher, Network Applications of Bloom Filters: A Survey (Internet Mathematics 1(4)), §2.1 Standard Bloom Filters — <https://www.eecs.harvard.edu/~michaelm/postscripts/im2005b.pdf> — "There are two competing forces: using more hash functions gives us more chances to find a 0 bit for an element that is not a member of S, but using fewer hash functions increases the fraction of 0 bits in the array."
[^bloom-opt]: Broder & Mitzenmacher, Network Applications of Bloom Filters: A Survey (Internet Mathematics 1(4)), §2.1 Standard Bloom Filters — <https://www.eecs.harvard.edu/~michaelm/postscripts/im2005b.pdf> — "from which symmetry reveals that the minimum value for g occurs when p = 1/2, or equivalently k = ln 2 · (m/n). In this case the false positive rate f is (1/2)^k ≈ (0.6185)^(m/n)."
[^bloom-int]: Broder & Mitzenmacher, Network Applications of Bloom Filters: A Survey (Internet Mathematics 1(4)), §2.1 Standard Bloom Filters — <https://www.eecs.harvard.edu/~michaelm/postscripts/im2005b.pdf> — "In practice, of course, k must be an integer, and a smaller, suboptimal k might be preferred since this reduces the number of hash functions that have to be computed."
[^bloom-split]: Broder & Mitzenmacher, Network Applications of Bloom Filters: A Survey (Internet Mathematics 1(4)), §2.1 Standard Bloom Filters — <https://www.eecs.harvard.edu/~michaelm/postscripts/im2005b.pdf> — "the probability of a false positive is actually always at least as large with this division."
[^bloom-space]: Broder & Mitzenmacher, Network Applications of Bloom Filters: A Survey (Internet Mathematics 1(4)), §2.2 A Lower Bound — <https://www.eecs.harvard.edu/~michaelm/postscripts/im2005b.pdf> — "Thus, space-wise Bloom filters are within a factor of log2 e ≈ 1.44 of the (asymptotic) lower bound."
[^bloom-8n]: Broder & Mitzenmacher, Network Applications of Bloom Filters: A Survey (Internet Mathematics 1(4)), §2.3 Hashing vs. Bloom Filters — <https://www.eecs.harvard.edu/~michaelm/postscripts/im2005b.pdf> — "For example, when m = 8n, the false positive probability is just over 0.02."
[^bose-under]: Bose et al., On the false-positive rate of Bloom filters, §1 — <https://cglab.ca/~morin/publications/ds/bloom-submitted.pdf> — "Thus, Bloom’s bound underestimates the false-positive rate in this case."
[^bose-wrong]: Bose et al., On the false-positive rate of Bloom filters, Abstract — <https://cglab.ca/~morin/publications/ds/bloom-submitted.pdf> — "We show that Bloom’s analysis is incorrect and give a correct analysis."
[^bose-indep]: Bose et al., On the false-positive rate of Bloom filters, §1 — <https://cglab.ca/~morin/publications/ds/bloom-submitted.pdf> — "The error occurs in deriving (1), where there is an implicit assumption that the event “Byi = 1” and the event “By1 = By2 = · · · = Byi−1 = 1” are independent."
[^bose-58]: Bose et al., On the false-positive rate of Bloom filters, §1 — <https://cglab.ca/~morin/publications/ds/bloom-submitted.pdf> — "one finds that the false-positive rate is 5/8, whereas pk evaluates to 9/16 = 4.5/8."
[^raft-random]: Ongaro & Ousterhout, In Search of an Understandable Consensus Algorithm (Extended Version), §5.2 Leader election — <https://raft.github.io/raft.pdf> — "Raft uses randomized election timeouts to ensure that split votes are rare and that they are resolved quickly. To prevent split votes in the first place, election timeouts are chosen randomly from a fixed interval (e.g., 150–300ms). This spreads out the servers so that in most cases only a single server will time out; it wins the election and sends heartbeats before any other servers time out."
[^raft-fcfs]: Ongaro & Ousterhout, In Search of an Understandable Consensus Algorithm (Extended Version), §5.2 Leader election — <https://raft.github.io/raft.pdf> — "Each server will vote for at most one candidate in a given term, on a first-come-first-served basis"
[^raft-majority]: Ongaro & Ousterhout, In Search of an Understandable Consensus Algorithm (Extended Version), §5.2 Leader election — <https://raft.github.io/raft.pdf> — "A candidate wins an election if it receives votes from a majority of the servers in the full cluster for the same term."
[^raft-fig2]: Ongaro & Ousterhout, In Search of an Understandable Consensus Algorithm (Extended Version), Figure 2, Rules for Servers (Followers) — <https://raft.github.io/raft.pdf> — "If election timeout elapses without receiving AppendEntries RPC from current leader or granting vote to candidate: convert to candidate"
[^raft-bcast-range]: Ongaro & Ousterhout, In Search of an Understandable Consensus Algorithm (Extended Version), §5.6 Timing and availability — <https://raft.github.io/raft.pdf> — "Raft’s RPCs typically require the recipient to persist information to stable storage, so the broadcast time may range from 0.5ms to 20ms, depending on storage technology."
[^raft-eval]: Ongaro & Ousterhout, In Search of an Understandable Consensus Algorithm (Extended Version), §9.3 — <https://raft.github.io/raft.pdf> — "In the absence of randomness, leader election consistently took longer than 10 seconds in our tests due to many split votes. Adding just 5ms of randomness helps significantly, resulting in a median downtime of 287ms."
[^raft-cluster]: Ongaro & Ousterhout, In Search of an Understandable Consensus Algorithm (Extended Version), §9.3 (Figure 16) — <https://raft.github.io/raft.pdf> — "The measurements were taken on a cluster of five servers with a broadcast time of roughly 15ms."
[^raft-eval-setup]: Ongaro & Ousterhout, In Search of an Understandable Consensus Algorithm (Extended Version), §9.3 — <https://raft.github.io/raft.pdf> — "To generate a worst-case scenario, the servers in each trial had different log lengths, so some candidates were not eligible to become leader. Furthermore, to encourage split votes, our test script triggered a synchronized broadcast of heartbeat RPCs from the leader before terminating its process"
[^th-uniform]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), §9.1 — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "Each server waits for a uniform random timeout after the last time it received a heartbeat."
[^th-min]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), §9.1 — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "Intuitively, any individual server is expected to time out halfway through the election timeout range, but with more servers it becomes more likely that the first server will time out sooner."
[^th-min-cdf]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), §9.1 — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "Let Ms be the minimum of T1 . . . Ts , a random variable representing the time the first server times out. Its cumulative distribution function (CDF) defines the probability that Ms is no greater than a particular time, t. This is equivalent to one minus the probability that all servers times out after t"
[^th-model]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), §9.2 Split vote rate with fixed latency — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "Because of the fixed network latency, the first server to time out is guaranteed to get the votes of all servers that don’t time out within l of it, and it will receive none of the votes of the other servers, who will each vote for themselves."
[^th-three]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), §9.2 Split vote rate with fixed latency — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "However, if three servers time out within l , then the earliest server will only be able to reach one other server in time to receive its vote, so the vote is split."
[^th-cond]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), §9.2 Split vote rate with fixed latency — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "The probability of a split vote is thus the probability that too many candidates time out within l of each other."
[^th-votes]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), §9.2 Split vote rate with fixed latency — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "The first server will get its own vote plus votes from the s − c servers that time out at least l time after the first."
[^th-numbers]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), §9.2 Split vote rate with fixed latency — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "In about 0.06% of elections, if the one-way network latency is 1 ms, Pr(D3,4 ≤ .01); In about 5.2% of elections, if the one-way network latency is 10 ms, Pr(D3,4 ≤ .1); and In about 18.1% of elections, if the one-way network latency is 20 ms, Pr(D3,4 ≤ .2)."
[^th-binom]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), §9.2 Split vote rate with fixed latency — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "The CDF somewhat resembles a binomial distribution, which hints that there may exist an easier derivation."
[^th-failures]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), §9.2 Split vote rate with fixed latency — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "The first thing to observe is that failures have a very large effect on split vote rates, especially if the cluster is down to a bare majority of its original members."
[^th-worst]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), §9.2 Split vote rate with fixed latency — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "To prepare for worst-case scenarios, the election timeout range should be set to produce tolerable values when a bare majority of the cluster is available."
[^th-fewer]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), §9.2 Split vote rate with fixed latency — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "larger clusters experience fewer split votes with the same number of failures"
[^th-more]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), §9.2 Split vote rate with fixed latency — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "Third, keeping the number of available servers constant, larger full cluster sizes will have more split votes."
[^th-overtake]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), §9.2 Split vote rate with variable latency — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "The problem is that a RequestVote message sent by one server can overtake a RequestVote message sent earlier by a different server."
[^th-var]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), §9.2 Split vote rate with variable latency — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "Thus, with variable latency, we intuitively expect somewhat higher rates of split votes (there is more competition)."
[^th-sim]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), §9.2 Split vote rate with variable latency — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "Rather than model this mathematically, we used a small simulation."
[^th-vote-reset]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), §9.3 — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "Thus, the next election term has the same probability of success as an entirely new election and will take just as long."
[^th-geom2]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), §9.3 — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "each election term is essentially memoryless, and the number of election terms required in an election can be modeled as a geometric distribution"
[^th-logs]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), §9.3 — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "(this isn’t quite true when logs differ; see Section 9.5)"
[^th-rec]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), chương 9 (tóm tắt các kết luận) — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "Split vote rates are low when the election timeout range is sufficiently broad. We recommend a range that is 10–20 times the one-way network latency"
[^th-flp]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), chương 9 (mở đầu) — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "no fault-tolerant consensus protocol can deterministically terminate in a purely asynchronous model. This manifests itself in split votes in Raft, which can potentially impede progress repeatedly during leader election."
[^th-assume]: Ongaro, Consensus: Bridging Theory and Practice (Stanford PhD thesis, 2014), chương 9 (mở đầu) — <https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> — "For example, servers must choose timeouts from a random distribution (they are not somehow synchronized), clocks must proceed at about the same rates, and servers and networks must be timely (or stopped)."
[^rust-expm1]: Rust std 1.95.0, f64::exp_m1 — <https://doc.rust-lang.org/1.95.0/std/primitive.f64.html> — "Returns e^(self) - 1 in a way that is accurate even if the number is close to zero."
[^rust-ln1p]: Rust std 1.95.0, f64::ln_1p — <https://doc.rust-lang.org/1.95.0/std/primitive.f64.html> — "Returns ln(1+n) (natural logarithm) more accurately than if the operations were performed separately."
[^rust-hashmap]: Rust std 1.95.0, std::collections::HashMap — <https://doc.rust-lang.org/1.95.0/std/collections/struct.HashMap.html> — "By default, HashMap uses a hashing algorithm selected to provide resistance against HashDoS attacks. The algorithm is randomly seeded"
