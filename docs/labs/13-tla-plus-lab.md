# Lab P13 — TLA+

> Đây là phần **thực hành** của [P13 · Đặc tả hình thức với TLA+](../13-tla-plus/00-tong-quan.md). Hãy đọc xong lý thuyết trước rồi mới làm lab: lab chỉ kiểm chứng và minh họa những gì các chương P13 đã nêu, và mỗi lab ghi rõ chương và mục cần đọc. Mọi fact có chú thích cuối trang, kiểm bằng `python3 tools/verify_citations.py docs/labs/13-tla-plus-lab.md`. Ba quy ước trung thực của bài này. **(1)** Output trong các khối `text` là output **thật** của chương trình Rust, chạy trên máy người soạn (macOS aarch64, rustc 1.95.0); trừ thời gian và hash trong output của `cargo test` (có ghi chú), không giá trị nào trong output phụ thuộc máy. **(2)** Các file TLA+ trong `labs/crates/lab-tla/specs/` **chưa được chạy** qua `pcal.trans`, SANY hay TLC (quy ước dự án: không chạy các công cụ Java). Mọi chỗ viết TLC "nên" báo gì là **kỳ vọng**, suy ra từ nguồn và từ kết quả của bản Rust, gắn nhãn *chưa chạy TLC*; không dòng nào trong bài là output của TLC. **(3)** Bản Rust là một bộ kiểm tra mô hình tự viết theo cách TLC làm; bài (a) hiệu chuẩn nó với các con số mà repo `tlaplus/Examples` ghi sẵn cho TLC, trước khi bạn tin những con số khác.

## Chuẩn bị và các bài lab

Crate: `labs/crates/lab-tla`, hai nửa tách bạch.

**Nửa Rust, chạy được trên macOS** (không có code chỉ-Linux). Phụ thuộc: `proptest` (bản `~1.12`, chỉ bật feature `std`) cho bài (f); hai crate chỉ dùng khi test, `tree-sitter` và `tree-sitter-tlaplus`, cho bài kiểm cú pháp spec ở `tests/specs.rs`.

| Module | Nội dung | Bài |
|---|---|---|
| `mc` | bộ kiểm tra mô hình explicit-state: BFS, invariant, counterexample ngắn nhất, thống kê giống TLC, kiểm refinement | a–d |
| `live` | `P ~> Q` dưới weak/strong fairness, counterexample dạng lasso | e |
| `simple`, `counter`, `commit`, `queue`, `request` | các mô hình, viết lại từ spec trong `specs/` | a–e |
| `sut`, `conform` | cài đặt 2PC "như service thật" có tiêm lỗi, và bộ so khớp code với model | f |

Sáu binary, chạy từ `labs/` bằng `cargo run -q -p lab-tla --bin <tên>`: `calibrate`, `lost_update`, `commit`, `queue`, `liveness`, `conformance`. (Binary của bài (b) tên `lost_update` chứ không phải `counter`, để không trùng tên file output với binary `counter` của lab P4 khi build cả workspace.)

**Nửa TLA+, chưa chạy.** Thư mục `labs/crates/lab-tla/specs/` có tám module và mười bốn file cấu hình `.cfg`. Mỗi module có một mô hình Rust tương ứng, và mỗi cfg ứng với một lần chạy của bản Rust:

| Module (ngôn ngữ) | File `.cfg` | Bài |
|---|---|---|
| `CounterLost` (PlusCal) | `CounterLost.cfg` | b |
| `CounterLock` (PlusCal) | `CounterLock.cfg` | b |
| `CounterLockSplit` (PlusCal) | `CounterLockSplit.cfg` | b |
| `TCommit` (TLA+) | `TCommit.cfg`, `TCommitBuggy.cfg` | a, c |
| `TwoPhase` (TLA+) | `TwoPhase.cfg`, `TwoPhaseBuggy.cfg` | a, c |
| `AtLeastOnce` (PlusCal) | `AtLeastOnce.cfg`, `AtLeastOnceNoDedup.cfg`, `AtLeastOnceNoDedupCrash.cfg` | d |
| `AtLeastOnceSplit` (PlusCal) | `AtLeastOnceSplit.cfg` | d |
| `RequestService` (TLA+) | `RequestServiceNoFair.cfg`, `RequestServiceWF.cfg`, `RequestServiceSF.cfg` | e |

Build và test:

```bash
cd labs
cargo build -p lab-tla --all-targets
cargo clippy -p lab-tla --all-targets
cargo test -p lab-tla
```

Output của `cargo test` (rút gọn: bỏ các dòng của sáu binary, mỗi binary có 0 test):

```text
running 41 tests
test result: ok. 41 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.03s
     Running tests/specs.rs (target/debug/deps/specs-3edc457c6c60d9cc)
running 8 tests
test the_counter_models_have_the_labels_of_their_specs ... ok
test the_queue_models_have_the_labels_of_their_specs ... ok
test the_models_print_the_variables_of_their_specs ... ok
test the_other_models_have_the_actions_of_their_specs ... ok
test pluscal_files_ship_an_empty_translation_for_pcal_trans_to_fill ... ok
test every_cfg_names_only_things_its_module_defines ... ok
test the_parser_does_reject_broken_specs ... ok
test every_tla_file_parses_without_syntax_errors ... ok
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
```

(Thời gian `finished in` và hash trong tên file test phụ thuộc máy và lần build.) Hai chỗ cần nói rõ về `tests/specs.rs`, vì nó là thứ duy nhất kiểm các file TLA+ khi TLC không chạy:

1. Nó parse mọi `.tla` bằng grammar tree-sitter cho TLA⁺ và PlusCal và đòi không có nút ERROR hay MISSING. Grammar đó cố ý dễ dãi: README của nó nói nếu spec không hợp lệ thì cây cú pháp chỉ "xấp xỉ đúng", có thể chấp nhận cú pháp sai[^ts-lenient]. Vì vậy đây là kiểm cú pháp thô, **không phải SANY**: nó không bắt tên chưa định nghĩa hay lỗi mức (level). Test `the_parser_does_reject_broken_specs` chỉ bảo đảm bộ parse không "xanh rỗng": nó phải từ chối lỗi gõ trong cả TLA⁺ lẫn PlusCal.
2. Nó kiểm mỗi `.cfg` chỉ nhắc tới tên mà module của nó định nghĩa và gán đúng các hằng số module khai báo, và kiểm mô hình Rust dùng đúng nhãn, tên action, tên biến của spec; nhờ đó "cùng một mô hình" không chỉ là lời nói.

Các file PlusCal được giao **chưa dịch**: vùng giữa hai dòng marker của bản dịch để trống, và test xác nhận điều đó; bạn chạy `pcal.trans` để điền vào.

### Cài công cụ TLA+ và lệnh chạy (chưa chạy)

Các công cụ TLA⁺ cần Java 11 trở lên[^readme-java]. Lấy `tla2tools.jar` từ trang Releases của kho `tlaplus/tlaplus` để dùng các công cụ từ dòng lệnh[^readme-releases]; README liệt kê, sau khi đưa file vào `CLASSPATH`, hai lệnh dùng ở đây là `java tlc2.TLC` (model checker) và `java pcal.trans` (bộ dịch PlusCal sang TLA⁺)[^readme-tlc][^readme-pcal]. Người soạn **chưa chạy** các lệnh dưới đây (theo quy ước dự án, không chạy công cụ Java); chúng được viết ra để bạn chạy. Dạng `-cp` thay cho biến `CLASSPATH`:

```bash
export TLA2TOOLS="$HOME/tla2tools.jar"      # đường dẫn tới file bạn tải về
cd labs/crates/lab-tla/specs

# 1. Chỉ với module PlusCal: dịch, điền vào giữa hai dòng marker. -nocfg: đừng ghi file .cfg.
java -cp "$TLA2TOOLS" pcal.trans -nocfg CounterLost.tla

# 2. Kiểm. Một module có nhiều cfg nên luôn truyền -config; -workers 1 cho trace ngắn nhất.
java -cp "$TLA2TOOLS" tlc2.TLC -workers 1 -config CounterLost.cfg CounterLost.tla
```

Ba chi tiết, mỗi cái có lý do:

- **`-nocfg`.** Khi chạy từ dòng lệnh, `pcal.trans` còn tạo một file `.cfg` để chạy TLC[^pm-cfg]; tùy chọn `-nocfg` tắt việc ghi file đó[^pm-nocfg]. Các cfg trong `specs/` do người soạn viết tay. Người soạn không biết `pcal.trans` có ghi đè một `.cfg` đã tồn tại hay không; `-nocfg` loại bỏ câu hỏi đó.
- **`-workers 1`.** Không có `-workers` thì CLI dùng một worker[^cli-workers]; ghi tường minh để thấy lý do: TLC duyệt BFS nên trace báo lỗi là ngắn nhất[^bfs-shortest], nhưng với nhiều worker, về lý thuyết có thể tồn tại hành vi ngắn hơn[^workers-shorter].
- **`-config`.** Nếu thiếu, TLC tìm `<spec>.cfg`[^cli-default-cfg]. Ở đây `TCommit.tla` có hai cfg (`TCommit.cfg`, `TCommitBuggy.cfg`), nên cfg nào cũng được truyền tường minh.

### Đọc con số: Rust và TLC

Mọi bài dưới đây in các con số cùng loại với con số TLC in, nên cần biết chúng ứng với nhau thế nào:

| Rust (`mc::Stats`) | TLC |
|---|---|
| `distinct`: số trạng thái khác nhau tìm được | số nút của đồ thị `G` mà TLC dựng, mỗi nút một trạng thái[^tlc-g-u]; thông báo tiến độ của TLC báo cả số trạng thái sinh ra lẫn số trạng thái phân biệt, ví dụ 2846 và 984 trong sách[^progress] |
| `generated`: trạng thái đầu cộng mọi successor sinh ra, kể cả trùng | số trạng thái sinh ra và xét, nằm trong cùng thông báo[^progress]; trạng thái đầu cũng được đếm là sinh ra[^init-progress] |
| `depth`: số trạng thái trên đường đi ngắn nhất từ trạng thái đầu tới trạng thái xa nhất | đường kính (diameter) của `G`[^diameter] |
| counterexample ngắn nhất | BFS cho hành vi ngắn nhất tới vi phạm safety[^bfs-shortest] |

Hai điểm khác cần nhớ khi so sánh:

- **TLC mặc định dừng ở vi phạm đầu tiên**[^step-check]; có cờ `-continue` để chạy tiếp[^cli-continue]. Bản Rust thì luôn duyệt hết đồ thị rồi mới đánh giá invariant, nên với model *có lỗi*, con số "toàn đồ thị" của Rust là của cả đồ thị, còn con số TLC in lúc dừng chỉ là phần đã duyệt tới đó. Người soạn chưa kiểm xem TLC có mở rộng tiếp từ trạng thái vi phạm khi dùng `-continue` hay không, nên bài này **chỉ đòi so con số trạng thái với những lần chạy không có lỗi** (các cfg "đúng"); với cfg có lỗi, thứ để so là *độ dài trace ngắn nhất*.
- **Một bước cho ra đúng trạng thái hiện tại vẫn là một trạng thái sinh ra.** Rust đếm `Terminating` của bản dịch PlusCal hay việc nhận lại một message đã nhận là một successor trùng. Quy ước này không tự hiển nhiên; bài (a) kiểm nó bằng số liệu TLC có sẵn.

### Lab (a) — `calibrate`: bộ đếm trạng thái này có đếm như TLC không?

Lý thuyết: [chương 04 mục 3](../13-tla-plus/04-invariant-va-tlc.md) (TLC duyệt BFS, `distinct` khác `generated`, diameter, ví dụ tính tay `TCommit`), [mục 8](../13-tla-plus/04-invariant-va-tlc.md) (số liệu `manifest.json`), [chương 08 mục 2 và 3](../13-tla-plus/08-dac-ta-giao-thuc-phan-tan.md) (`TCommit`, `TwoPhase`), [chương 06 mục 7](../13-tla-plus/06-pluscal.md) (bản dịch PlusCal và action `Terminating`).

Trước khi tin bất kỳ con số nào của một bộ kiểm tra tự viết cần một phép thử *độc lập*: tìm những lần chạy TLC mà người khác đã ghi lại con số, rồi xem bản Rust có tái tạo được không. Repo `tlaplus/Examples` ghi `distinctStates`, `totalStates`, `stateDepth` cho từng cfg trong `manifest.json`. `calibrate` so bốn mô hình:

- `TCommit.cfg`, `RM = {r1, r2, r3}`: 34 trạng thái phân biệt, 94 tổng, độ sâu 7[^manifest-tcommit];
- `TwoPhase.cfg`, `RM = {r1, r2, r3}`: 288 / 1146 / 11[^manifest-twophase];
- `TwoPhase` với sáu RM: comment cuối file spec nói cấu hình đó có 50816 trạng thái khả đạt[^tp-checked6] (nguồn không ghi số sinh ra hay độ sâu);
- `Simple.cfg`, `N = 5`: 723 / 1842 / 11[^manifest-simple][^ex-simple-cfg]. Đây là spec PlusCal, nên bản dịch có thêm một nhánh cho phép thuật toán đã kết thúc "làm một bước không làm gì", vì TLC không phân biệt được deadlock với kết thúc[^pm-done-disj][^pm-done-loop]. Đây là phép thử cho quy ước đếm self-loop ở trên.

Người soạn hiểu `totalStates` là số `generated` (chương 04 mục 3 cũng đọc thế). Việc cả ba con số khớp đồng thời ở ba mô hình có đủ ba số (`TCommit`, `TwoPhase`, `Simple`) là bằng chứng cho cách đọc đó, không phải chứng minh.

```bash
cd labs && cargo run -q -p lab-tla --bin calibrate    # exit 0
```

```text
model             config           distinct  generated  depth   recorded (d / g / depth) agree
TCommit           RM = {r1,r2,r3}        34         94      7   34 / 94 / 7            yes
TwoPhase          RM = {r1,r2,r3}       288       1146     11   288 / 1146 / 11        yes
TwoPhase          six RMs             50816     402306     20   50816 / - / -          yes
Simple (PlusCal)  N = 5                 723       1842     11   723 / 1842 / 11        yes
every recorded number is reproduced: true
the invariants of the cfg files hold, with each cfg's deadlock setting: true
Simple: 31 of 723 states have every pc = Done and each has a Terminating step to itself; without those self-loops `generated` would be 1811, not 1842
```

Giải thích:

1. **Cả bốn dòng khớp**, ở cả ba loại số. Với `TCommit`, `TwoPhase` và `Simple`, `generated` và `depth` đều khớp, không chỉ `distinct`. Bản Rust cho cùng kết luận với `TCommit.cfg` về deadlock: cfg đó đặt `CHECK_DEADLOCK FALSE`[^tc-cfg] còn `TwoPhase.cfg` thì không[^tp-cfg], và bản Rust chạy với đúng từng thiết lập đó mà không báo lỗi.
2. **Dòng cuối chứng minh quy ước self-loop.** Trong 723 trạng thái của `Simple`, 31 trạng thái có mọi `pc = Done`; mỗi trạng thái có một bước `Terminating` về chính nó. Nếu không tính 31 bước đó, `generated` là 1811; con số mà nguồn ghi là 1842. Vậy TLC đếm các successor trùng với trạng thái hiện tại (điều chương 06 mục 7 chỉ mô tả bằng lời).
3. **Con số sáu RM**: 50816 trạng thái phân biệt khớp comment của spec; `402306` (sinh ra) và `20` (độ sâu) là số của Rust, không nguồn nào ghi lại để đối chiếu.
4. **Công thức đóng cho `TCommit`** (tính tay, người soạn tổng quát hóa phép đếm `27 + 7 = 34` ở chương 04 mục 3): `3ⁿ + 2ⁿ − 1` trạng thái phân biệt, `1 + n(3ⁿ + 2ⁿ⁻¹)` trạng thái sinh ra, độ sâu `2n + 1`. Test `tcommit_counts_match_the_hand_derivation` kiểm công thức cho `n = 1…6`; với `n = 3` ra đúng `34`, `94`, `7`.
5. **Không chứng minh gì về các mô hình khác.** Bài (a) chỉ cho thấy luật đếm của bộ duyệt (phân biệt, sinh ra kể cả self-loop, độ sâu) trùng TLC trên bốn spec. Nó không chứng minh rằng bản Rust của `CounterLost`, `AtLeastOnce`... mô tả đúng điều spec TLA+ trong `specs/` nói; phần đó mới chỉ được `tests/specs.rs` kiểm ở mức tên (nhãn, action, biến) và bằng tính tay ở các bài sau.

### Lab (b) — `lost_update`: lost update, lock, và lock sai phạm vi

Lý thuyết: [chương 06 mục 2](../13-tla-plus/06-pluscal.md) (label là ranh giới bước nguyên tử), [mục 7](../13-tla-plus/06-pluscal.md) (bản dịch sang TLA+), [mục 8](../13-tla-plus/06-pluscal.md) (hai thread tăng counter, lost update, sửa bằng lock, và biến thể lock sai phạm vi trong phần Đi sâu), [chương 04 mục 5](../13-tla-plus/04-invariant-va-tlc.md) (đọc error trace; nghi ngờ khi "xanh").

Ba spec PlusCal, cùng thuật toán với chương 06 mục 8: `CounterLost.tla` (`Load: tmp := counter; Store: counter := tmp + 1`), `CounterLock.tla` (thêm `Acquire` và `Release` quanh cả hai bước), `CounterLockSplit.tla` (nhả lock giữa `Load` và `Store`). Ba điều về dạng file:

- Một bước PlusCal là phần thực thi từ một label đến label kế[^pm-step-label]; thuật toán nhiều process chạy bằng cách lặp lại việc chọn một process bất kỳ rồi chạy một bước của nó[^pm-multi-exec]. Đó là lý do TLC thử mọi xen kẽ và tìm ra lost update.
- Bản dịch định nghĩa một action cho mỗi thao tác nguyên tử[^pm-action-per-label] và thêm nhánh cho thuật toán đã kết thúc làm một bước không làm gì[^pm-done-loop]; bản Rust `counter.rs` có đúng hai thứ đó (`Load(1)`, `Store(2)`… và `Terminating`).
- `TypeOK` dùng biến cục bộ `tmp` nên phải nằm sau dòng `END TRANSLATION`[^pm-defs-after]; `Correct` và `MutualExclusion` chỉ dùng biến toàn cục và `pc` nên nằm được trong khối `define`[^pm-define-vars].

```bash
cd labs && cargo run -q -p lab-tla --bin lost_update    # exit 0; tham số tùy chọn: số thread, mặc định 2
```

Output (rút gọn: bỏ các dòng biến của `State 2`–`State 12` ở mô hình thứ ba; mô hình thứ hai chỉ giữ phần thống kê):

```text
== CounterLost: Load, Store (no lock), NumThreads = 2 ==
whole state graph: 13 distinct states, 18 states generated, depth 5
invariant TypeOK: holds in all 13 states
invariant Correct: VIOLATED in 1 of 13 states; shortest counterexample has 5 states (4 steps):
State 1: <Initial predicate>
/\ counter = 0
/\ pc = <<"Load", "Load">>
/\ tmp = <<0, 0>>
State 2: <Load(1)>
/\ counter = 0
/\ pc = <<"Store", "Load">>
/\ tmp = <<0, 0>>
State 3: <Load(2)>
/\ counter = 0
/\ pc = <<"Store", "Store">>
/\ tmp = <<0, 0>>
State 4: <Store(1)>
/\ counter = 1
/\ pc = <<"Done", "Store">>
/\ tmp = <<0, 0>>
State 5: <Store(2)>
/\ counter = 1
/\ pc = <<"Done", "Done">>
/\ tmp = <<0, 0>>
deadlock check on: no state without a successor
AllDone holds in 3 of 13 states, so Correct is not vacuous

== CounterLock: Acquire, Load, Store, Release, NumThreads = 2 ==
whole state graph: 17 distinct states, 19 states generated, depth 9
invariant TypeOK: holds in all 17 states
invariant Correct: holds in all 17 states
invariant MutualExclusion: holds in all 17 states
deadlock check on: no state without a successor
AllDone holds in 2 of 17 states, so Correct is not vacuous

== CounterLockSplit: lock released between Load and Store, NumThreads = 2 ==
whole state graph: 41 distinct states, 46 states generated, depth 13
invariant TypeOK: holds in all 41 states
invariant MutualExclusion: holds in all 41 states
invariant Correct: VIOLATED in 1 of 41 states; shortest counterexample has 13 states (12 steps):
State 1: <Initial predicate>
/\ counter = 0
/\ lock = 0
/\ pc = <<"AcquireR", "AcquireR">>
/\ tmp = <<0, 0>>
State 2: <AcquireR(1)>
State 3: <Load(1)>
State 4: <ReleaseR(1)>
State 5: <AcquireR(2)>
State 6: <Load(2)>
State 7: <ReleaseR(2)>
State 8: <AcquireW(1)>
State 9: <Store(1)>
State 10: <ReleaseW(1)>
State 11: <AcquireW(2)>
State 12: <Store(2)>
State 13: <ReleaseW(2)>
/\ counter = 1
/\ lock = 0
/\ pc = <<"Done", "Done">>
/\ tmp = <<0, 0>>
deadlock check on: no state without a successor
AllDone holds in 1 of 41 states, so Correct is not vacuous
```

Layout của trace gần giống error trace của TLC để dễ đối chiếu, nhưng **không phải output của TLC**.

Giải thích:

1. **Lost update trong đúng bốn bước.** Phản ví dụ ngắn nhất là `Load(1)`, `Load(2)`, `Store(1)`, `Store(2)`: cả hai thread đọc `counter = 0`, rồi cùng ghi `0 + 1`. Đó chính là kịch bản mà Learn TLA+ mô tả cho spec tương tự[^lt-threads-fail] và mà chương 06 mục 8 dẫn ra bằng tay. BFS trả về *một* trace ngắn nhất; có thể có trace khác cùng độ dài (thứ tự `Load` đổi chỗ), và TLC có thể in cái khác. Độ dài là thứ đáng so, không phải từng bước[^bfs-shortest].
2. **Tính tay khớp.** Với hai thread, số trạng thái theo độ sâu là 1, 2, 3, 4, 3 (tổng 13); số successor là 17, gồm 14 bước thật (2 từ trạng thái đầu, rồi 4 + 4 + 4 từ ba tầng kế) và 3 bước `Terminating` ở ba trạng thái cuối; cộng 1 trạng thái đầu bằng 18. Test `state_counts_are_stable` ghi lại các số này.
3. **`TypeOK` đúng, `Correct` sai.** Lost update không làm sai kiểu: `counter` luôn nằm trong `0..2`. Chương 04 cũng dặn type invariant chỉ bảo đảm giá trị *có thể*, không phải *đúng*[^typeinv-principle]. Chỉ invariant nghiệp vụ mới thấy lỗi.
4. **`Correct` không rỗng.** `Correct` là `AllDone => counter = NumThreads`: nó chỉ nói *nếu* mọi thread xong thì kết quả đúng; Learn TLA+ lưu ý chính điều đó[^lt-alldone-weak]. Một invariant dạng này dễ "xanh" chỉ vì không bao giờ tới trạng thái xong: sách dặn rất dễ thỏa một safety property bằng cách không làm gì[^safety-nothing]. Dòng cuối mỗi mô hình in số trạng thái mà `AllDone` đúng (3, 2, 1) để chứng tỏ `Correct` thật sự được kiểm ở những trạng thái đó.
5. **Lock sửa được.** Một lock bao quanh cả đọc lẫn ghi: 17 trạng thái, cả ba invariant đúng. Learn TLA+ báo spec có lock của họ cũng qua kiểm tra[^lt-threads-pass]. Con số 17 tính tay được: sau `Acquire` đầu tiên, hai thread chỉ có thể chạy *trọn* critical section lần lượt, nên đồ thị gần như hai chuỗi 8 trạng thái chia nhau trạng thái đầu (1 + 8 + 8); với ba thread là 61 (test).
6. **Lock sai phạm vi: bài học lớn nhất của bài.** `CounterLockSplit` giữ lock riêng quanh `Load` và quanh `Store`. `MutualExclusion` vẫn đúng ở cả 41 trạng thái, nhưng `Correct` sai: lỗi nằm ở khoảng *giữa* `ReleaseR` và `AcquireW`, nơi thread kia chen vào. Phản ví dụ dài 12 bước chứ không ngắn hơn vì `Correct` chỉ có thể sai khi `AllDone`, tức mỗi thread phải đi hết sáu label. Điều rút ra (nhận xét của người soạn, theo chương 06 mục 8): tính chất bạn kiểm phải là tính chất bạn thật sự cần; "không hai thread cùng trong critical section" khác "phép đọc-sửa-ghi đúng".

**TLC (chưa chạy TLC).** Kỳ vọng, suy từ kết quả Rust:

| Lệnh | Kỳ vọng |
|---|---|
| `pcal.trans -nocfg CounterLost.tla`, rồi TLC với `CounterLost.cfg` | invariant `Correct` bị vi phạm; trace ngắn nhất 5 trạng thái |
| tương tự với `CounterLock.cfg` | không lỗi; nếu TLC chạy hết, kỳ vọng 17 trạng thái phân biệt, 19 sinh ra, độ sâu 9 |
| tương tự với `CounterLockSplit.cfg` | `Correct` bị vi phạm; trace ngắn nhất 13 trạng thái; `MutualExclusion` đúng |

Hai điều kiện cho con số 17/19/9: bản dịch của `pcal.trans` phải có cùng cấu trúc như `Simple.tla` ở bài (a) (một action mỗi label và nhánh `Terminating`), và TLC phải chạy hết không dừng vì lỗi. Với cfg có lỗi, TLC dừng ở vi phạm[^step-check] nên con số trạng thái nó in là của phần đồ thị đã duyệt tới lúc đó, không phải của toàn đồ thị như con số Rust ở trên.

### Lab (c) — `commit`: TCommit, TwoPhase, và refinement

Lý thuyết: [chương 08 mục 2](../13-tla-plus/08-dac-ta-giao-thuc-phan-tan.md) (`TCommit`: đặc tả *yêu cầu*), [mục 3](../13-tla-plus/08-dac-ta-giao-thuc-phan-tan.md) (`TwoPhase`: giao thức, và cái nó không nói), [mục 6](../13-tla-plus/08-dac-ta-giao-thuc-phan-tan.md) (viết invariant), [mục 7](../13-tla-plus/08-dac-ta-giao-thuc-phan-tan.md) (cỡ model, "xanh" nghĩa là gì), [chương 04 mục 5](../13-tla-plus/04-invariant-va-tlc.md) (biến thể lỗi của `TCommit` tính tay) và [mục 6](../13-tla-plus/04-invariant-va-tlc.md) (deadlock check), [chương 07](../13-tla-plus/07-refinement.md) (refinement).

`TCommit.tla` là yêu cầu: một biến `rmState`, hai action; một RM chỉ được commit sau khi mọi RM đã prepared[^tc-gl-order] và không bao giờ có RM committed cùng lúc với RM aborted[^tc-gl-cons]. `TwoPhase.tla` thêm một transaction manager và tập `msgs` chứa mọi message từng gửi: message không bao giờ bị xóa, và action nhận được bật khi message *tồn tại* trong `msgs`[^net-set-never-removed]; nhờ vậy nhận hai lần được phép[^net-dup]. Cả hai file thêm một hằng `Buggy`; khi bật, mỗi file bỏ đúng một guard: ở `TCommit`, nhánh abort không còn `notCommitted`; ở `TwoPhase`, `TMCommit` không còn đòi `tmPrepared = RM`. `TwoPhase.tla` còn thêm hai định nghĩa cho TLC: `TPConsistent` (invariant của `TCommit` áp lên trạng thái của `TwoPhase`) và `RefinesTCommit == TC!TCSpec`, khai bằng `PROPERTY`. Bản gốc của Examples chỉ kiểm `TPTypeOK` và ghi định lý "`TPSpec` cài đặt `TCSpec`" dưới dạng `THEOREM`[^tp-refine].

```bash
cd labs && cargo run -q -p lab-tla --bin commit    # exit 0; tham số tùy chọn: số RM, mặc định 3
```

Output (rút gọn: hai trace dài của `TCommit` lỗi và `TwoPhase` lỗi chỉ giữ dòng thống kê và các action):

```text
== TCommit, RM = 3 ==
whole state graph: 34 distinct states, 94 states generated, depth 7
invariant TCConsistent: holds in all 34 states
deadlock check off (CHECK_DEADLOCK FALSE): 2 states without a successor

== TCommit with Buggy = TRUE (abort without notCommitted), RM = 3 ==
whole state graph: 46 distinct states, 109 states generated, depth 7
invariant TCConsistent: VIOLATED in 12 of 46 states; shortest counterexample has 6 states (5 steps):
State 1: <Initial predicate>
/\ rmState = [r1 |-> "working", r2 |-> "working", r3 |-> "working"]
State 2: <Prepare(r1)>
/\ rmState = [r1 |-> "prepared", r2 |-> "working", r3 |-> "working"]
State 3: <Prepare(r2)>
/\ rmState = [r1 |-> "prepared", r2 |-> "prepared", r3 |-> "working"]
State 4: <Prepare(r3)>
/\ rmState = [r1 |-> "prepared", r2 |-> "prepared", r3 |-> "prepared"]
State 5: <Decide(r1) [commit]>
/\ rmState = [r1 |-> "committed", r2 |-> "prepared", r3 |-> "prepared"]
State 6: <Decide(r2) [abort]>
/\ rmState = [r1 |-> "committed", r2 |-> "aborted", r3 |-> "prepared"]
deadlock check off (CHECK_DEADLOCK FALSE): 8 states without a successor

== TwoPhase, RM = 3 ==
whole state graph: 288 distinct states, 1146 states generated, depth 11
invariant TCConsistent: holds in all 288 states
deadlock check on: no state without a successor
refinement TwoPhase => TCommit: every step of the 288 states maps to a TCommit step or a stutter

== TwoPhase with Buggy = TRUE (TMCommit without tmPrepared = RM), RM = 3 ==
whole state graph: 623 distinct states, 2508 states generated, depth 11
invariant TCConsistent: VIOLATED in 90 of 623 states; shortest counterexample has 4 states (3 steps):
State 1: <Initial predicate>
/\ rmState = [r1 |-> "working", r2 |-> "working", r3 |-> "working"]
/\ tmState = "init"
/\ tmPrepared = {}
/\ msgs = {}
State 2: <TMCommit>
/\ rmState = [r1 |-> "working", r2 |-> "working", r3 |-> "working"]
/\ tmState = "committed"
/\ tmPrepared = {}
/\ msgs = {[type |-> "Commit"]}
State 3: <RMChooseToAbort(r1)>
/\ rmState = [r1 |-> "aborted", r2 |-> "working", r3 |-> "working"]
/\ tmState = "committed"
/\ tmPrepared = {}
/\ msgs = {[type |-> "Commit"]}
State 4: <RMRcvCommitMsg(r2)>
/\ rmState = [r1 |-> "aborted", r2 |-> "committed", r3 |-> "working"]
/\ tmState = "committed"
/\ tmPrepared = {}
/\ msgs = {[type |-> "Commit"]}
deadlock check on: no state without a successor
refinement TwoPhase => TCommit: FAILS; shortest behaviour ending in a step TCommit does not have (3 states):
State 1: <Initial predicate>
/\ rmState = [r1 |-> "working", r2 |-> "working", r3 |-> "working"]
/\ tmState = "init"
/\ tmPrepared = {}
/\ msgs = {}
State 2: <TMCommit>
/\ rmState = [r1 |-> "working", r2 |-> "working", r3 |-> "working"]
/\ tmState = "committed"
/\ tmPrepared = {}
/\ msgs = {[type |-> "Commit"]}
State 3: <RMRcvCommitMsg(r1)>
/\ rmState = [r1 |-> "committed", r2 |-> "working", r3 |-> "working"]
/\ tmState = "committed"
/\ tmPrepared = {}
/\ msgs = {[type |-> "Commit"]}
```

Giải thích:

1. **`TCommit` đúng khớp số của Examples** (34 / 94 / 7, bài (a)), và bản Rust xác nhận điều chương 04 mục 6 mới chỉ lập luận bằng tay: vì sao cfg đặt `CHECK_DEADLOCK FALSE`[^tc-cfg]. Có đúng **hai** trạng thái không còn successor, "mọi RM committed" và "mọi RM aborted"; theo sách, TLC mặc định coi trạng thái không còn bước là deadlock và có thể tắt vì với nhiều hệ thống đó chỉ là kết thúc thành công[^deadlock-term]. Đây là điều bản Rust quan sát được trên mô hình của nó, không phải điều TLC báo.
2. **Bỏ `notCommitted` là đủ để vỡ, trong `n + 2` bước.** Trace ngắn nhất có 6 trạng thái: ba `Prepare`, một `Decide` commit, một `Decide` abort, đúng bằng trace tính tay ở chương 04 mục 5. Test `dropping_not_committed_breaks_consistency_in_n_plus_2_steps` kiểm độ dài `n + 3` trạng thái cho `n = 2…4`. Không gian trạng thái phình từ 34 lên 46 vì giờ có thêm các trạng thái "committed và aborted chung".
3. **`TwoPhase` đúng**: 288 / 1146 / 11 như manifest[^manifest-twophase]; invariant `TCommit` áp lên `rmState` đúng ở mọi trạng thái (bản Rust in tên `TCConsistent`; trong `TwoPhase.tla` cùng định nghĩa đó gọi là `TPConsistent`); và **không có deadlock** với deadlock check bật, khớp việc `TwoPhase.cfg` không tắt nó[^tp-cfg]. `generated` (1146) lớn hơn nhiều so với `distinct` (288) vì nhiều đường xen kẽ dẫn tới cùng một trạng thái và vì các self-loop như `TMRcvPrepared` lặp lại hay `RMRcvCommitMsg` với RM đã committed (người soạn không tách riêng số của từng nguyên nhân).
4. **Refinement bằng step simulation.** `check_refinement` duyệt đồ thị của `TwoPhase` và đòi mỗi bước, chiếu xuống `rmState`, hoặc là một bước của `TCommit` hoặc không đổi (stuttering). Đây là phần safety của điều TLC kiểm khi bạn khai `PROPERTY RefinesTCommit`: TLC kiểm *từng bước* với ImpliedAction của property dạng `[][A]_v`[^implied-action]. Kết quả: 288 trạng thái, mọi bước hợp lệ; `TwoPhase` cài đặt `TCommit` trong phạm vi model này. Phát biểu chung là một `THEOREM` của spec gốc[^tp-refine]; một lần chạy trên ba RM không phải chứng minh.
5. **`TwoPhase` lỗi.** Bỏ điều kiện "mọi RM đã gửi `Prepared`" khỏi `TMCommit`: trace ngắn nhất của invariant dài 4 trạng thái. BFS của Rust chọn `TMCommit`, `RMChooseToAbort(r1)`, `RMRcvCommitMsg(r2)`; một thứ tự khác cùng độ dài cũng có thể. Spec gốc nói rõ `TMCommit` chỉ bật khi TM ở trạng thái đầu và *mọi* RM đã gửi `Prepared`[^tp-tmcommit]; bỏ vế sau, `Commit` có thể được phát khi RM còn `working`, và vì `RMRcvCommitMsg` không có guard trên `rmState`, một RM `working` chuyển thẳng sang `committed`. Refinement vỡ *sớm hơn* một bước (3 trạng thái, so với 4 của invariant): chỉ cần một RM `working` nhận `Commit` là đã có bước không thuộc `TCommit` (`working → committed`; nhánh commit của `TCommit` đòi `prepared`), chưa cần có RM nào aborted. Vì thế kiểm refinement cho biết *bước nào* sai so với yêu cầu, còn invariant chỉ cho biết trạng thái xấu.

**TLC (chưa chạy TLC).**

| Cfg | Kỳ vọng |
|---|---|
| `TCommit.cfg` | không lỗi; vì cfg gốc ghi 34 / 94 / 7[^manifest-tcommit] và cfg này chỉ thêm hằng `Buggy = FALSE`, kỳ vọng cùng con số |
| `TCommitBuggy.cfg` | `TCConsistent` bị vi phạm; trace ngắn nhất 6 trạng thái |
| `TwoPhase.cfg` | không lỗi, kể cả `PROPERTY RefinesTCommit`; kỳ vọng 288 / 1146 / 11 (cfg gốc ghi các số này cho cfg chỉ kiểm `TPTypeOK`[^manifest-twophase]; kiểm thêm invariant và property không đổi tập trạng thái) |
| `TwoPhaseBuggy.cfg` | `TPConsistent` bị vi phạm; trace ngắn nhất 4 trạng thái; `RefinesTCommit` cũng bị vi phạm |

### Lab (d) — `queue`: at-least-once, idempotent consumer, và dedup tách bước

Lý thuyết: [chương 09 mục 3](../13-tla-plus/09-mo-hinh-hoa-he-backend.md) (hàng đợi at-least-once và consumer idempotent), [mục 7](../13-tla-plus/09-mo-hinh-hoa-he-backend.md) (giữ không gian trạng thái trong tầm TLC; "đừng tin thành công"), [chương 06 mục 5](../13-tla-plus/06-pluscal.md) (`await`, `either`, `with`), [chương 04 mục 8](../13-tla-plus/04-invariant-va-tlc.md) (state-space explosion).

Mô hình: một broker có thể giao lại bất kỳ tin nào đang `inflight` bất cứ lúc nào (visibility timeout co thành một action); mỗi consumer nhận một tin, áp tác dụng phụ, rồi ack hoặc "crash" (mất ack, tin sẽ được giao lại). Cơ chế thật là điều chương 09 dẫn từ tài liệu thiết kế của Kafka và của RabbitMQ: consumer xử lý xong nhưng hỏng trước khi ack thì tin bị tiêu thụ hai lần[^k-ack-problem]; delivery chưa ack bị requeue khi channel đóng[^rmq-requeue], nên consumer phải được thiết kế idempotent[^rmq-idem]. `Handle` trong `AtLeastOnce.tla` là *một* label: kiểm `seen`, tăng `effects`, ghi `seen` là một transaction; `AtLeastOnceSplit.tla` tách thành `Effect` rồi `Record`. Trong cfg, `NULL = NULL` gán hằng `NULL` cho model value tên `NULL`: theo sách, một chuỗi ký tự như `a1` hay `foo` mà không phải số, chuỗi trong nháy kép, `TRUE` hay `FALSE` thì được hiểu là model value[^mv-syntax].

```bash
cd labs && cargo run -q -p lab-tla --bin queue    # exit 0
```

Output (rút gọn: chỉ giữ trace của cấu hình đầu tiên và thứ tư, mỗi trace chỉ các dòng `State` và biến `effects`, `crashes`; `invariant EffectsIffSeen` ... "(trace not shown)" là nguyên văn của bin):

```text
== AtLeastOnce (Dedup = FALSE): one message, one consumer, one lost ack ==
whole state graph: 13 distinct states, 18 states generated, depth 8
invariant CrashBudget: holds in all 13 states
invariant AtMostOneEffect: VIOLATED in 3 of 13 states; shortest counterexample has 7 states (6 steps):
State 1: <Initial predicate>
State 2: <Receive(c1, m1)>
State 3: <Redeliver(broker, m1)>
State 4: <Handle(c1)>
State 5: <Ack(c1) [crash]>
State 6: <Receive(c1, m1)>
State 7: <Handle(c1)>
/\ effects = [m1 |-> 2]
/\ crashes = 1
invariant EffectsIffSeen: VIOLATED in 3 of 13 states; shortest counterexample has 7 states (6 steps) (trace not shown)
deadlock check off (CHECK_DEADLOCK FALSE): 2 states without a successor

== AtLeastOnce (Dedup = FALSE): one message, two consumers, no crash at all ==
whole state graph: 24 distinct states, 43 states generated, depth 8
invariant CrashBudget: holds in all 24 states
invariant AtMostOneEffect: VIOLATED in 5 of 24 states; shortest counterexample has 6 states (5 steps):
State 1: <Initial predicate>
State 2: <Receive(c1, m1)>
State 3: <Redeliver(broker, m1)>
State 4: <Handle(c1)>
State 5: <Receive(c2, m1)>
State 6: <Handle(c2)>
/\ effects = [m1 |-> 2]
/\ crashes = 0
invariant EffectsIffSeen: VIOLATED in 5 of 24 states; shortest counterexample has 6 states (5 steps) (trace not shown)
deadlock check off (CHECK_DEADLOCK FALSE): 3 states without a successor

== AtLeastOnce (Dedup = TRUE): dedup in the same atomic step as the effect ==
whole state graph: 96 distinct states, 202 states generated, depth 15
invariant CrashBudget: holds in all 96 states
invariant AtMostOneEffect: holds in all 96 states
invariant EffectsIffSeen: holds in all 96 states
deadlock check off (CHECK_DEADLOCK FALSE): 9 states without a successor

== AtLeastOnceSplit (Dedup = TRUE): dedup recorded in a separate step from the effect ==
whole state graph: 51 distinct states, 97 states generated, depth 10
invariant CrashBudget: holds in all 51 states
invariant AtMostOneEffect: VIOLATED in 13 of 51 states; shortest counterexample has 6 states (5 steps):
State 1: <Initial predicate>
State 2: <Receive(c1, m1)>
State 3: <Redeliver(broker, m1)>
State 4: <Effect(c1)>
State 5: <Receive(c2, m1)>
State 6: <Effect(c2)>
/\ effects = [m1 |-> 2]
/\ crashes = 0
invariant EffectsIffSeen: VIOLATED in 21 of 51 states; shortest counterexample has 3 states (2 steps) (trace not shown)
deadlock check off (CHECK_DEADLOCK FALSE): 4 states without a successor

== growth of the idempotent model with its parameters ==
 msgs consumers  crashes  distinct  generated  depth
    1         1        1        13         18      8
    1         2        1        59        119     11
    2         1        1        73        117     11
    2         2        1       621       1617     14
    2         2        2      1171       3269     18
    3         2        1      3571      10594     17
```

Giải thích:

1. **Không dedup, một tin, một consumer, một lần mất ack là đủ.** Trace ngắn nhất 7 trạng thái (6 bước) cho `effects = 2`, cùng độ dài với trace tính tay ở chương 09 mục 3 nhưng thứ tự khác: BFS ở đây chọn `Redeliver` *trước* `Handle`, ack mất rồi consumer nhận lại. Hai trace là hai cách dựng cùng một vi phạm; bộ duyệt không phân biệt cái nào "đúng". Đó là cơ chế "xử lý xong, chưa ack, giao lại" mà Kafka mô tả[^k-ack-problem].
2. **Consumer chậm là đủ, không cần crash.** Với hai consumer và `MaxCrashes = 0`, trace ngắn nhất dài 6 trạng thái (5 bước): `Receive(c1)`, `Redeliver`, `Handle(c1)`, `Receive(c2)`, `Handle(c2)`; `crashes = 0` ở cuối. Điều đó khớp nhận xét ở chương 09 rằng action `Redeliver` gộp hai chuyện thật, consumer crash và consumer chỉ chậm, vào một action. Hệ quả (suy luận của người soạn): nếu chỉ mô hình hóa "crash" mà không có action giao lại độc lập với crash, bạn sẽ bỏ sót kịch bản consumer chậm.
3. **Dedup cùng bước nguyên tử với tác dụng phụ thì đúng.** 96 trạng thái, `AtMostOneEffect` đúng, và cả bất biến quy nạp `EffectsIffSeen` mà chương 09 dùng để lập luận ("`effects[m] = 1` đúng khi và chỉ khi `m` thuộc `seen`") cũng đúng ở mọi trạng thái của cấu hình này. Đây là kiểm tra trên một cấu hình hữu hạn, không phải chứng minh quy nạp.
4. **Tách `Effect` khỏi `Record` thì trùng quay lại, dù `Dedup = TRUE` và không crash.** Trace ngắn nhất 6 trạng thái: consumer thứ hai kiểm `seen` khi consumer thứ nhất đã `Effect` nhưng chưa `Record`. `EffectsIffSeen` vỡ sớm hơn nhiều, chỉ sau 2 bước (`Receive`, `Effect`): ngay giữa hai bước đã có `effects = 1` mà `seen` còn rỗng. Đây là lý do thực tế để đặt bản ghi dedup và thay đổi dữ liệu trong cùng transaction DB (chương 09 mục 3).
5. **Con số trạng thái tăng rất nhanh theo tham số**, đúng như sách nói về cỡ model: số trạng thái thường là hàm mũ của tham số[^ss-exp]. Bảng cuối đi từ 13 trạng thái (1 tin, 1 consumer) đến 3571 (3 tin, 2 consumer) khi chỉ thêm hai tin và một consumer; sáu điểm không đủ để suy ra quy luật, nhưng đủ để thấy vì sao sách dặn bắt đầu bằng model tí hon[^start-small], mà các trace ở trên (một tin) vẫn đủ tái hiện lỗi.
6. **`CHECK_DEADLOCK FALSE` là bắt buộc ở các cfg này.** Cả bốn cấu hình ở trên báo có trạng thái không còn successor (2, 3, 9, 4): tin đã `acked` rồi thì không ai làm gì thêm nữa, và vòng lặp của broker không bao giờ kết thúc. Nếu để deadlock check bật, TLC sẽ dừng ở trạng thái xong đó[^deadlock-term]; cfg trong `specs/` đều ghi `CHECK_DEADLOCK FALSE`[^cli-checkdeadlock].

**TLC (chưa chạy TLC).**

| Cfg | Kỳ vọng |
|---|---|
| `AtLeastOnceNoDedupCrash.cfg` (1 tin, 1 consumer, 1 crash, `Dedup = FALSE`) | `AtMostOneEffect` bị vi phạm; trace ngắn nhất 7 trạng thái |
| `AtLeastOnceNoDedup.cfg` (1 tin, 2 consumer, 0 crash, `Dedup = FALSE`) | `AtMostOneEffect` bị vi phạm; trace ngắn nhất 6 trạng thái |
| `AtLeastOnce.cfg` (1 tin, 2 consumer, 2 crash, `Dedup = TRUE`) | không lỗi; nếu chạy hết, kỳ vọng 96 / 202 / 15 |
| `AtLeastOnceSplit.cfg` (1 tin, 2 consumer, 0 crash, `Dedup = TRUE`) | `AtMostOneEffect` bị vi phạm; trace ngắn nhất 6 trạng thái; `EffectsIffSeen` vỡ ở trạng thái thứ 3 |

Với bốn dòng này, con số 96 / 202 / 15 có cùng điều kiện như ở bài (b): bản dịch của `pcal.trans` phải khớp mô hình, kể cả việc biến `cur` giữ giá trị cũ sau mỗi vòng (spec không reset nó, và bản Rust cũng không).

### Lab (e) — `liveness`: "request rồi sẽ được xử lý" dưới ba mức fairness

Lý thuyết: [chương 05 mục 3](../13-tla-plus/05-liveness-va-fairness.md) (stuttering mãi và vì sao cần fairness), [mục 5](../13-tla-plus/05-liveness-va-fairness.md) (weak và strong fairness), [mục 6](../13-tla-plus/05-liveness-va-fairness.md) (chính ví dụ này, và lập luận tay), [mục 8](../13-tla-plus/05-liveness-va-fairness.md) (TLC kiểm liveness thế nào, lasso).

Chương 05 *lập luận tay* rằng `Spec0` (không fairness) và `Spec1` (`WF` cho `Restart` và `Process`) vi phạm `AllServed`, còn `Spec2` (`SF` cho `Process`) thỏa. Lab này kiểm các lập luận đó bằng một thuật toán, cùng nguyên tắc với TLC: TLC xét các behavior là đường đi vô hạn trong đồ thị trạng thái[^ss-tlc-checkalg], mà đồ thị của nó có sẵn cạnh từ mỗi trạng thái về chính nó[^ss-tlc-selfloop] (suy luận của người soạn: đó là chỗ biểu diễn bước stuttering); bản cài đặt thực tế tìm các thành phần liên thông mạnh, tức chu trình, trong đồ thị[^tlc-scc]. `live.rs` làm đúng thế cho thuộc tính `P ~> Q`: lấy các trạng thái "`P` và không `Q`" làm điểm bắt đầu, giới hạn đồ thị vào các trạng thái không `Q`, chia thành SCC, và với mỗi SCC kiểm các điều kiện fairness:

- weak fairness `WF_v(A)` buộc `A` xảy ra nếu nó bật liên tục từ một lúc nào đó[^ss-wf-def]; một chu trình thỏa nó khi trong chu trình có bước `A` *hoặc* có trạng thái mà `A` bị tắt[^ss-wf-88];
- strong fairness `SF_v(A)` buộc `A` xảy ra nếu nó bật vô hạn lần, dù bị tắt xen kẽ[^ss-sf-def]; một chu trình thỏa nó khi trong chu trình có bước `A` *hoặc* `A` tắt trên mọi trạng thái của nó. Nếu SCC vi phạm, cách duy nhất để cứu là loại các trạng thái mà `A` bật rồi tìm lại chu trình nhỏ hơn.

Counterexample có dạng lasso: một phần đầu hữu hạn rồi một chu trình lặp mãi; mã nguồn TLC có sẵn các chuỗi `Back to state` và `Stuttering` để chỉ ra chỗ chu trình quay lại hay lặp một trạng thái[^tlc-backto][^tlc-stuttering]; bản Rust in theo cách tương tự.

```bash
cd labs && cargo run -q -p lab-tla --bin liveness    # exit 0
```

```text
state graph of RequestService with Reqs = {r1, r2}: 18 distinct states, 37 generated, depth 6

== Spec0: no fairness ==
(req[r1] = "pending") ~> (req[r1] = "done"): VIOLATED; lasso with a prefix of 2 states and a cycle of 1 steps
State 1: <Initial predicate>
/\ req = [r1 |-> "idle", r2 |-> "idle"]
/\ up = TRUE
State 2: <Submit(r1)>
/\ req = [r1 |-> "pending", r2 |-> "idle"]
/\ up = TRUE
Stuttering: state 2 repeats forever
(req[r2] = "pending") ~> (req[r2] = "done"): VIOLATED; lasso with a prefix of 2 states and a cycle of 1 steps

== Spec1: WF_vars(Restart), WF_vars(Process(r)) ==
fairness: WF_vars(Restart)
fairness: WF_vars(Process(r1))
fairness: WF_vars(Process(r2))
(req[r1] = "pending") ~> (req[r1] = "done"): VIOLATED; lasso with a prefix of 2 states and a cycle of 2 steps
State 1: <Initial predicate>
/\ req = [r1 |-> "idle", r2 |-> "idle"]
/\ up = TRUE
State 2: <Submit(r1)>
/\ req = [r1 |-> "pending", r2 |-> "idle"]
/\ up = TRUE
State 3: <Crash>
/\ req = [r1 |-> "pending", r2 |-> "idle"]
/\ up = FALSE
Back to state 2: <Restart>
(req[r2] = "pending") ~> (req[r2] = "done"): VIOLATED; lasso with a prefix of 2 states and a cycle of 2 steps

== Spec2: WF_vars(Restart), SF_vars(Process(r)) ==
fairness: WF_vars(Restart)
fairness: SF_vars(Process(r1))
fairness: SF_vars(Process(r2))
(req[r1] = "pending") ~> (req[r1] = "done"): holds
(req[r2] = "pending") ~> (req[r2] = "done"): holds
```

Giải thích:

1. **`Spec0`: stuttering sau `Submit`.** Behavior `Submit(r1)` rồi đứng yên mãi thỏa `Init /\ [][Next]_vars` (spec cho phép stuttering[^ss-hc-stutter]) nên request kẹt `pending`. Với `RequestServiceNoFair.cfg`, mã nguồn TLC có sẵn một thông báo gợi ý rằng counterexample stuttering có thể do thiếu fairness[^tlc-hint-nofair]; người soạn chưa chạy TLC nên chưa thấy nó in ra.
2. **`Spec1`: weak fairness thỏa suông.** Lasso là `Submit(r1)` rồi lặp `Crash`, `Restart`. `WF_vars(Restart)` được thỏa vì `Restart` chạy vô hạn lần; `WF_vars(Process(r1))` cũng được thỏa, nhưng *suông*: `Process(r1)` bị tắt (`up = FALSE`) vô hạn lần, và weak fairness chỉ buộc nó chạy nếu nó bật *liên tục*[^ss-wf-88]. Cả hai lập luận khớp chính xác mục 6 của chương 05, bước 3.
3. **`Spec2`: strong fairness chặn được.** `Process(r)` bật vô hạn lần (mỗi lần worker sống lại) nên `SF` buộc nó chạy; trong đồ thị này không còn chu trình fair nào giữ request ở `pending`. Cả hai request đều `holds`, khớp lập luận theo ba trường hợp của chương 05.
4. **Độ tin cậy của thuật toán.** Phép thử độc lập nằm trong test của `live.rs`: nó dựng lại ví dụ pool một connection của chương 05 mục 5 (`ConnPool1`) và kiểm đúng lập luận tay ở đó. Với weak fairness cho `Acquire`, client 2 có thể chờ mãi trong lasso gồm `Want`, `Acquire`, `Release` của client 1; với strong fairness cho `Acquire`, không ai bị bỏ đói. Test khác dựng lại bước 2 của mục 6: nếu worker không thể crash thì `WF` đã đủ. Sách dặn nghi ngờ khi TLC không tìm thấy vi phạm liveness vì mô hình hữu hạn có thể che lỗi[^ss-tlc-suspicious], và khuyên kiểm một thuộc tính liveness mà spec *không* thỏa để chắc việc kiểm thật sự chạy[^ss-tlc-sanity]. Ở đây `Spec0` và `Spec1` đóng đúng vai đó: chúng bị vi phạm đúng như lập luận tay, nên kết quả `holds` của `Spec2` không phải "xanh rỗng".

**TLC (chưa chạy TLC).**

| Cfg | Kỳ vọng |
|---|---|
| `RequestServiceNoFair.cfg` (`Spec0`) | `AllServed` bị vi phạm bởi một behavior stuttering sau `Submit` |
| `RequestServiceWF.cfg` (`Spec1`) | `AllServed` bị vi phạm bởi lasso mà worker crash/restart mãi |
| `RequestServiceSF.cfg` (`Spec2`) | không lỗi |

### Lab (f) — `conformance`: code có làm đúng điều model cho phép không?

Lý thuyết: [chương 10 mục 1](../13-tla-plus/10-tu-dac-ta-den-code-rust.md) (khoảng cách spec–code; ví dụ "code đi tắt": đếm RM thay vì nhớ danh tính), [mục 2](../13-tla-plus/10-tu-dac-ta-den-code-rust.md) (model-based testing: sinh test từ model, và kiểm vết với model), [mục 3](../13-tla-plus/10-tu-dac-ta-den-code-rust.md) (trace validation), [mục 6](../13-tla-plus/10-tu-dac-ta-den-code-rust.md) (property-based testing kiểu state machine với proptest), [mục 7](../13-tla-plus/10-tu-dac-ta-den-code-rust.md) (mỗi công cụ trả lời câu hỏi nào).

**Đối tượng kiểm.** `sut.rs` là một cài đặt 2PC viết như service thật: participant và coordinator là các state machine Rust có guard, và một "wire" nhớ mọi gói tin mạng từng chở (gửi lại một gói là được phép, giống tập `msgs` của spec). Code chỉ dùng enum action của model làm *từ vựng* để điều khiển (như các dòng log của một vết); mọi thứ khác là của nó. Coordinator có thể nhớ phiếu bầu theo danh tính (đúng) hoặc chỉ đếm (lỗi). Cùng `Mutant` chọn một lỗi thực tế mỗi lần:

- `CountVotes`: coordinator *đếm* gói `Prepared` thay vì nhớ RM nào đã gửi; đúng lối "đi tắt" mà Cirstea và cộng sự gặp: nếu một RM gửi lại do timeout, TM đếm hai lần và commit sớm[^tv-shortcut];
- `AbortAfterVote`: một RM vẫn tự abort được sau khi đã bỏ phiếu `Prepared`;
- `StrictAbort`: coordinator từ chối abort khi đã nhận một phiếu. An toàn, nhưng chặt hơn spec.

**Hai hướng kiểm tra, hai câu hỏi khác nhau** (`conform.rs`):

- **code ⇒ model** (`check_trace`, `find_divergence`): điều khiển code bằng chuỗi action *tùy ý*, kể cả action mà model cấm; mỗi bước code *chấp nhận* phải là bước của model (hoặc stuttering) và trạng thái quan sát được phải khớp; code từ chối thì cả hai giữ nguyên. Đây là ý của trace validation: Cirstea và cộng sự chỉ đòi giao của tập behavior của spec và tập behavior mà vết biểu diễn khác rỗng[^tv-problem], không đòi bao hàm, vì biến không ghi trong vết có thể nhận giá trị bất định[^tv-notrefine]; ở đây mọi thứ quan sát được đều có giá trị đầy đủ nên bài đòi chặt hơn, mỗi bước code nhận phải là bước model. Chuỗi action do proptest sinh rồi thu nhỏ: state machine test kiểm hệ thống so với một định nghĩa trạng thái trừu tượng và rút gọn phản ví dụ về chuỗi transition tối thiểu[^pt-sm-shrink].
- **model ⇒ code** (`transition_cover`): với *mỗi* trạng thái của đồ thị model, chạy lại đường đi ngắn nhất tới nó trên code mới, rồi thử *mọi* bước model cho phép từ đó; code phải chấp nhận (trừ bước giữ nguyên trạng thái) và đi tới cùng trạng thái quan sát. Đó là model-based test-case generation theo hướng MongoDB: họ dùng không gian trạng thái mà TLC khám phá để sinh test[^mdb-mbtcg-use], và test sinh tự động chạy lại được sau mỗi lần đổi model[^mdb-nice].

Về trạng thái quan sát: nó gồm trạng thái các RM, quyết định của coordinator và tập gói đã gửi, nhưng **không** gồm sổ phiếu của coordinator, nên cái sai của `CountVotes` không hiện ra trong log; chỉ bước mà nó cho phép (commit sớm) mới lộ.

```bash
cd labs && cargo run -q -p lab-tla --bin conformance    # exit 0; seed cố định nên output lặp lại được
```

```text
== model => code: transition cover of the TwoPhase graph, RM = 3 ==
intended code                  ok: 288 states, 1145 transitions replayed on the code
coordinator counts votes       ok: 288 states, 1145 transitions replayed on the code
RM may abort after voting      ok: 288 states, 1145 transitions replayed on the code
TM never aborts after a vote   DIVERGES: the code refused TMAbort, which the model enables in
                                  /\ rmState = [r1 |-> "prepared", r2 |-> "working", r3 |-> "working"]
                                  /\ tmState = "init"
                                  /\ tmPrepared = {r1}
                                  /\ msgs = {[type |-> "Prepared", rm |-> r1]}

== code => model: 10000 proptest walks of 1..=40 actions, RM = 2, seed 7 ==
intended code                  passed: 10000 walks, 33482 of 207992 attempted steps accepted; 132 walks ended committed, 7317 aborted
coordinator counts votes       DIVERGES; shrunk to 4 actions: [RMPrepare(r2), TMRcvPrepared(r2), TMRcvPrepared(r2), TMCommit]
                               action 4 (TMCommit): the code took it, the model forbids it in
                                  /\ rmState = [r1 |-> "working", r2 |-> "prepared"]
                                  /\ tmState = "init"
                                  /\ tmPrepared = {r2}
                                  /\ msgs = {[type |-> "Prepared", rm |-> r2]}
RM may abort after voting      DIVERGES; shrunk to 2 actions: [RMPrepare(r1), RMChooseToAbort(r1)]
                               action 2 (RMChooseToAbort(r1)): the code took it, the model forbids it in
                                  /\ rmState = [r1 |-> "prepared", r2 |-> "working"]
                                  /\ tmState = "init"
                                  /\ tmPrepared = {}
                                  /\ msgs = {[type |-> "Prepared", rm |-> r1]}
TM never aborts after a vote   passed: 10000 walks, 33121 of 207992 attempted steps accepted; 336 walks ended committed, 5907 aborted
```

(1145 là số bước của đồ thị `TwoPhase` với ba RM: `generated` 1146 trừ trạng thái đầu.)

Giải thích:

1. **Mã đúng qua cả hai hướng.** Chạy lại mọi trạng thái và 1145 bước của model trên code, và 10000 chuỗi ngẫu nhiên, không có bước nào code làm mà model cấm.
2. **Bảng hai chiều: hướng nào bắt lỗi nào.**

   | Lỗi | model ⇒ code (cover) | code ⇒ model (proptest) |
   |---|---|---|
   | `CountVotes` | **không thấy** | bắt, chuỗi 4 action |
   | `AbortAfterVote` | **không thấy** | bắt, chuỗi 2 action |
   | `StrictAbort` | bắt: code từ chối `TMAbort` | **không thấy** |

   Hai lỗi đầu là code làm *nhiều hơn* spec cho phép: cover chỉ thử bước model *bật*, nên không bao giờ thử chạm tới chỗ code quá dễ dãi. `StrictAbort` ngược lại: code làm *ít hơn*, vẫn an toàn, nên hướng "mọi bước code nhận phải là bước model" không có gì để phàn nàn; chỉ cover thấy rằng spec cho phép `TMAbort` ở đó mà code từ chối. Một đội chỉ chạy một hướng sẽ tin nhầm rằng mình đã kiểm xong.
3. **`CountVotes` lộ ra qua một chuỗi retransmit, và thu nhỏ về bốn action**: một RM bỏ phiếu, gói `Prepared` của nó được giao hai lần, coordinator commit. Đây chính là kịch bản Cirstea mô tả[^tv-shortcut]. Cover không thấy vì nó không bao giờ gửi lại một gói: đường đi ngắn nhất tới mỗi trạng thái model đi qua `TMRcvPrepared` đúng một lần cho mỗi RM. Kết quả thu nhỏ là *một* chuỗi tối thiểu, không duy nhất: chạy với số ca khác, bộ shrinker có thể đổi `r2` thành `r1`.
4. **`AbortAfterVote` thu nhỏ về hai action** (`RMPrepare` rồi `RMChooseToAbort` cùng một RM), khớp việc `RMChooseToAbort` chỉ bật ở `working` trong spec.
5. **Phép thử không rỗng, và vì sao cần cover.** Với mã đúng, 10000 chuỗi chỉ có 132 chuỗi kết thúc với coordinator đã commit (33482 / 207992 ≈ 16% bước được chấp nhận): lấy mẫu đều trong cả bảng chữ cái action hiếm khi dựng được cả một đường commit. Sách dặn nghi ngờ thành công vì rất dễ thỏa safety bằng cách không làm gì[^safety-nothing], và dùng coverage để thấy phần `Next` chưa bao giờ chạy[^coverage]; ShardStore cũng thừa nhận test ngẫu nhiên có thể bỏ sót lỗi[^ss21-miss] và chỉ kiểm được các trạng thái mà harness chạm tới[^ss21-reach]. Số 132 là coverage thô của người soạn; cover phủ *mọi* trạng thái của model nên bù cho chỗ mỏng này.
6. **Seed cố định, kết quả lặp lại được trong một phiên bản proptest.** `find_divergence` dựng `TestRng` từ `seed = 7`; tài liệu proptest hứa cùng thuật toán và seed cho cùng dãy giá trị của bộ sinh số[^pt-rng-seed]. Điều đó không hứa gì về việc chiến lược sinh chuỗi giữ nguyên qua phiên bản: các test chỉ khẳng định *hình dạng* của lỗi (ví dụ chuỗi shrunk của `CountVotes` kết thúc bằng `TMCommit` và có hai lần nhận cùng một `Prepared`), không khẳng định từng action.

**Giới hạn của bài (nhận xét của người soạn).** `Cluster` là đồ chơi: một process, "mạng" là tập gói, quan sát được lấy thẳng từ biến trong process thay vì từ log sinh ra bởi instrument; ba lỗi là do người soạn tiêm chứ không phải lỗi tự nhiên. Đây không phải trace validation bằng TLC (không có file vết, không có `TraceSpec`), chỉ là cùng ý tưởng, dựng bằng Rust. Nó cho thấy *cách* hai hướng bổ sung nhau, không cho thấy chúng bắt được bao nhiêu lỗi trong code thật; chính Cirstea và cộng sự nói phương pháp trace validation không cho bảo đảm đúng đắn hình thức[^tv-nofg].

### Bài tập mở rộng

1. **Đối chiếu với TLC.** Cài công cụ, chạy `pcal.trans -nocfg` cho năm module PlusCal rồi chạy TLC với cả mười bốn cfg, và điền bảng: cfg, verdict, độ dài trace ngắn nhất (nếu lỗi), số trạng thái phân biệt (nếu chạy hết). So với các cột "kỳ vọng" trong bài. Chỗ khác nhau là chỗ đáng học nhất: hoặc spec trong `specs/` khác mô hình Rust (hãy tìm chỗ khác bằng cách đọc bản dịch mà `pcal.trans` sinh ra), hoặc điều kiện của con số (cấu trúc bản dịch, TLC dừng ở lỗi) không đúng như người soạn giả định. Thử thêm `-coverage` để xem action nào chưa bao giờ chạy[^coverage].
2. **Symmetry.** Viết hàm chuẩn hóa trạng thái cho mô hình `queue` (hoán vị `Consumers` và `Msgs`) và đếm lại. Sách nói với `n` process số trạng thái TLC xét giảm một hệ số `n!`[^ss-sym]; đo xem số thật giảm bao nhiêu và vì sao nhỏ hơn `n!`. Lưu ý TLC không kiểm tính đối xứng thay bạn.
3. **Thêm một lỗi thứ tư vào `sut.rs`**, ví dụ coordinator quên `tm_on_prepared` kiểm `decision`, hoặc RM chấp nhận `Commit` khi đang `Working`. Dự đoán trước hướng nào bắt được, rồi chạy `conformance`.
4. **Liveness cho `queue`.** Dùng `live::check_leads_to` để kiểm "mọi tin `ready` rồi sẽ `acked`" cho `Queue::idempotent`: chọn `Fairness` cho `Receive`, `Handle`, `AckOk`, và thử ngân sách crash hữu hạn. Hãy tự hỏi action nào cần `SF` (nhiều consumer tranh cùng một tin).
5. **`CacheAside`.** Viết mô hình thứ năm từ [chương 09 mục 4](../13-tla-plus/09-mo-hinh-hoa-he-backend.md) (cache-aside và cuộc đua stale cache), kèm cặp `UseLease` bật/tắt và trace tay trong chương. Nhớ kiểm tính không rỗng: `CacheNotStale` chỉ ràng buộc các trạng thái "yên", nên cần chứng tỏ có trạng thái yên (chương 09 mục 7 gợi ý invariant `NotQuiescent`).
6. **Chuyển bài (f) sang `proptest-state-machine`.** Dùng `ReferenceStateMachine` với `TwoPhase` làm reference. Lưu ý docs của sách Proptest nói hiện chỉ hỗ trợ chiến lược tuần tự[^pt-sm-seq]; vì thế bài (f) cũng chỉ kiểm logic tuần tự, không kiểm xen kẽ giữa các thread (việc của loom hay shuttle, [chương 10 mục 5](../13-tla-plus/10-tu-dac-ta-den-code-rust.md)).
7. **Đổi tham số bài (f).** Chạy `conformance` với bốn RM (`RMS = 4`, sửa hằng trong `bin/conformance.rs`). Tỉ lệ chuỗi kết thúc bằng commit thay đổi thế nào, và `CountVotes` có còn được bắt trong cùng ngân sách ca không? Đây là cách tự tay thấy vì sao sách khuyên bắt đầu bằng model tí hon[^start-small].

## Câu hỏi tự kiểm tra (về output lab)

1. Ở bài (a), vì sao `generated` của `Simple` là 1842 chứ không phải 1811, và điều đó cho biết gì về cách TLC đếm?
   <details><summary>Đáp án</summary>

   31 trong 723 trạng thái có mọi `pc = Done`, và mỗi trạng thái có một bước `Terminating` về chính nó; bản dịch PlusCal thêm nhánh đó để trạng thái kết thúc không bị coi là deadlock[^pm-done-loop]. Bỏ 31 bước đó, `generated` là 1811; nguồn ghi 1842[^manifest-simple], nên TLC đếm cả successor trùng với trạng thái hiện tại.

   </details>

2. Ở bài (b), `CounterLockSplit` giữ `MutualExclusion` nhưng vi phạm `Correct`. Vì sao phản ví dụ phải dài 12 bước?
   <details><summary>Đáp án</summary>

   Lock chỉ bảo vệ từng bước `Load` và từng bước `Store` riêng; khoảng giữa `ReleaseR` và `AcquireW` để thread kia đọc cùng giá trị cũ. `Correct` là `AllDone => counter = NumThreads`, chỉ có thể sai khi cả hai thread `Done`, nên mỗi thread phải chạy hết sáu label: 12 bước, 13 trạng thái. `AllDone` có thật trong đồ thị (1 trong 41 trạng thái) nên invariant không rỗng[^safety-nothing].

   </details>

3. Ở bài (c), `TCommit` có hai trạng thái không còn successor. Chúng là gì, và cfg của spec phải làm gì với chúng?
   <details><summary>Đáp án</summary>

   "Mọi RM committed" và "mọi RM aborted": cả `Prepare` lẫn `Decide` đều không còn bật. TLC coi trạng thái không còn bước là deadlock theo mặc định và cho phép tắt vì với nhiều hệ thống đó là kết thúc thành công[^deadlock-term], nên `TCommit.cfg` đặt `CHECK_DEADLOCK FALSE`[^tc-cfg]. `TwoPhase` không có trạng thái như vậy (message trong `msgs` không bao giờ bị xóa nên luôn còn bước nhận), và `TwoPhase.cfg` giữ deadlock check[^tp-cfg].

   </details>

4. Ở bài (c), bản `TwoPhase` lỗi vi phạm cả `TPConsistent` (4 trạng thái) lẫn refinement (3 trạng thái). Kiểm tra refinement cho thêm thông tin gì?
   <details><summary>Đáp án</summary>

   Invariant chỉ báo trạng thái xấu (`r1` aborted, `r2` committed). Refinement chỉ ra *bước* nào của cài đặt không tương ứng bước nào của yêu cầu: `RMRcvCommitMsg(r1)` đưa RM `working` sang `committed`, mà `TCommit` chỉ cho commit từ `prepared`. Kiểm theo từng bước dạng `[][A]_v`[^implied-action] nên TLC có thể báo đúng bước đó khi bạn khai `PROPERTY RefinesTCommit`.

   </details>

5. Ở bài (d), `Dedup = TRUE` mà `AtMostOneEffect` vẫn vỡ trong bản `AtLeastOnceSplit`, không cần crash. Cơ chế là gì?
   <details><summary>Đáp án</summary>

   `Effect` và `Record` là hai bước riêng. Sau `Receive(c1)` và `Redeliver`, consumer 2 nhận cùng tin; nếu `Effect(c1)` đã chạy mà `Record(c1)` chưa, `seen` còn rỗng nên `Effect(c2)` không bị chặn và `effects` thành 2. Bất biến `EffectsIffSeen` vỡ sớm hơn, sau hai bước (`Receive`, `Effect`), vì có `effects = 1` mà `seen` rỗng. Chương 09 mục 3 nói điều tương tự bằng lời: dedup phải ở cùng transaction với tác dụng phụ.

   </details>

6. Ở bài (e), `Spec1` có `WF_vars(Process(r))` mà request vẫn kẹt. Vì sao `Spec2` thì không?
   <details><summary>Đáp án</summary>

   Trong lasso `Crash`, `Restart`, `Process(r)` bị tắt vô hạn lần, nên weak fairness thỏa suông: nó chỉ buộc một action chạy nếu action đó bật *liên tục*[^ss-wf-88]. Strong fairness buộc action chạy nếu nó bật *vô hạn lần*, dù bị tắt xen kẽ[^ss-sf-def]; mỗi lần worker sống lại `Process(r)` bật lại, nên `SF` loại lasso đó. Đây cũng là lý do `RequestServiceWF.cfg` được kỳ vọng báo lỗi còn `RequestServiceSF.cfg` thì không (chưa chạy TLC).

   </details>

7. Ở bài (f), `StrictAbort` qua 10000 chuỗi ngẫu nhiên nhưng bị cover bắt, còn `CountVotes` thì ngược lại. Vì sao một đội nên chạy cả hai hướng?
   <details><summary>Đáp án</summary>

   Hướng code ⇒ model chỉ đòi mọi bước code *chấp nhận* là bước model, nên code chặt hơn spec không vi phạm gì, nhưng nó bắt code làm quá nhiều, như `CountVotes` hay `AbortAfterVote`. Hướng model ⇒ code chỉ thử bước model *bật*, nên bắt code làm thiếu (`StrictAbort`) mà không bao giờ chạm tới các bước model cấm. `CountVotes` còn cần một lần *gửi lại* gói mà đường đi ngắn nhất của cover không có[^tv-shortcut]. Cả hai đều là kiểm chứng trong phạm vi chúng chạm tới, không phải chứng minh[^tv-nofg].

   </details>

[^ts-lenient]: tree-sitter-tlaplus 1.5.0, README — <https://docs.rs/crate/tree-sitter-tlaplus/1.5.0/source/README.md> — "If the spec is not valid TLA⁺, the parse tree will be approximately correct - perhaps permissively allowing illegal syntax, or interpreting erroneous syntax in strange ways."
[^readme-java]: tlaplus/tlaplus, README (commit 94d0c507) — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/README.md> — "The TLA⁺ tools require Java 11+ to run."
[^readme-releases]: tlaplus/tlaplus, README (commit 94d0c507) — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/README.md> — "Get `tla2tools.jar` from the \[releases\]\(https://github.com/tlaplus/tlaplus/releases\) to use the tools from the command line."
[^readme-tlc]: tlaplus/tlaplus, README (commit 94d0c507) — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/README.md> — "java tlc2.TLC -help # The TLA⁺ model checker"
[^readme-pcal]: tlaplus/tlaplus, README (commit 94d0c507) — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/README.md> — "java pcal.trans -help # The PlusCal-to-TLA⁺ translator"
[^pm-cfg]: PlusCal User's Manual (P-syntax), Phụ lục C Translator Options — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "It then produces a configuration (.cfg) file that can be used to run TLC from the command line."
[^pm-nocfg]: PlusCal User's Manual (P-syntax), tùy chọn của pcal.trans — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "-nocfg Suppress writing of the .cfg file."
[^cli-workers]: Learn TLA+, Beyond the Toolbox — <https://learntla.com/topics/cli.html> — "Specifies the number of worker threads to use for model checking. This is very important. Without this, the CLI defaults to a single worker."
[^bfs-shortest]: tlaplus/tlaplus, Toolbox doc "TLC Options Page" (commit 94d0c50) — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/toolbox/org.lamport.tla.toolbox.doc/html/model/tlc-options-page.html> — "This has the advantage that if TLC finds a violation of a safety property, then it will produce a shortest possible behavior that exhibits the error."
[^workers-shorter]: Specifying Systems, §14.5.2 (chú thích 3) — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "When using multiple threads, it is possible, though unlikely, for there to be a shorter behavior that also violates the invariant."
[^cli-default-cfg]: Learn TLA+, Beyond the Toolbox — <https://learntla.com/topics/cli.html> — "If -config isn’t provided, TLC will by default look for specfile.cfg."
[^tlc-g-u]: Specifying Systems, §14.3.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The graph G is the part of the state reachability graph that TLC has found so far, and U contains all states in G whose successors TLC has not yet computed."
[^progress]: Specifying Systems, §14.5.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "that it has generated and examined 2846 states, finding 984 distinct ones, and that the queue U of unexplored states contains 856 states."
[^init-progress]: Specifying Systems, §14.5.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "This indicates that, when evaluating the initial predicate, TLC generated 4 states, among which there were 2 distinct ones."
[^diameter]: Specifying Systems, §14.5.2 (chú thích 2) — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The diameter of G is the smallest number d such that every state in G can be reached from an initial state by a path containing at most d states."
[^step-check]: Specifying Systems, §14.3.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "If Invariant is false in state t or ImpliedAction is false for the step s → t, then report an error and stop."
[^cli-continue]: Learn TLA+, Beyond the Toolbox — <https://learntla.com/topics/cli.html> — "Will continue model checking even after a violation is found."
[^manifest-tcommit]: tlaplus/Examples, specifications/transaction_commit/manifest.json (commit 6a7b26a) — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/transaction_commit/manifest.json> — ""distinctStates": 34, "totalStates": 94, "stateDepth": 7"
[^manifest-twophase]: tlaplus/Examples, transaction_commit/manifest.json (commit 6a7b26a) — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/transaction_commit/manifest.json> — ""distinctStates": 288, "totalStates": 1146, "stateDepth": 11"
[^tp-checked6]: TwoPhase.tla (transaction_commit), tlaplus/Examples @ 6a7b26a — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/transaction_commit/TwoPhase.tla> — "The two theorems in this module have been checked with TLC for six RMs, a configuration with 50816 reachable states, in a little over a minute on a 1 GHz PC."
[^manifest-simple]: tlaplus/Examples, specifications/TeachingConcurrency/manifest.json (commit 6a7b26a) — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/TeachingConcurrency/manifest.json> — ""distinctStates": 723, "totalStates": 1842, "stateDepth": 11"
[^ex-simple-cfg]: tlaplus/Examples, specifications/TeachingConcurrency/Simple.cfg (commit 6a7b26a) — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/TeachingConcurrency/Simple.cfg> — "CONSTANT N = 5 SPECIFICATION Spec INVARIANTS PCorrect TypeOK Inv"
[^pm-done-disj]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "TLC has no way of distinguishing deadlock from termination, which is simply a desired form of deadlock."
[^pm-done-loop]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The translation therefore adds a disjunction to Next that allows a terminated algorithm to perform a step that does nothing."
[^tc-cfg]: tlaplus/Examples, specifications/transaction_commit/TCommit.cfg (commit 6a7b26a) — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/transaction_commit/TCommit.cfg> — "CONSTANT RM = {r1, r2, r3} INVARIANTS TCTypeOK TCConsistent SPECIFICATION TCSpec CHECK_DEADLOCK FALSE"
[^tp-cfg]: TwoPhase.cfg (transaction_commit), tlaplus/Examples @ 6a7b26a — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/transaction_commit/TwoPhase.cfg> — "CONSTANT RM = {r1, r2, r3} INVARIANT TPTypeOK SPECIFICATION TPSpec"
[^pm-step-label]: PlusCal User's Manual (P-syntax), mục 2.6 A Multiprocess Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A single step (atomic action) of a PlusCal algorithm consists of the execution from one label to the next."
[^pm-multi-exec]: PlusCal User's Manual (P-syntax), mục 3.3 Processes — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "A multiprocess algorithm is executed by repeatedly choosing an arbitrary process and executing one step of that process"
[^pm-action-per-label]: PlusCal User's Manual (P-syntax), Phụ lục B.1 The FastMutex Algorithm — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The translation defines an action for each atomic operation of the algorithm."
[^pm-defs-after]: PlusCal User's Manual (P-syntax), mục 5.11 TLA+ Definitions — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "definitions that use the identifiers declared or defined in the translation must come after the END TRANSLATION line."
[^pm-define-vars]: PlusCal User's Manual (P-syntax), mục 3.6 Definitions — <https://lamport.azurewebsites.net/tla/p-manual.pdf> — "The variables that may appear within the define statement are the ones declared in the variable statement that immediately precedes it and that follows the algorithm name, as well as the variable pc and, if there is a procedure, the variable stack ."
[^lt-threads-fail]: Learn TLA+, Concurrency (trang sống, truy cập 2026-10-12) — <https://learntla.com/core/concurrency.html> — "Both threads read the value of counter when it's 0, meaning they both set tmp to 0, meaning they both assign counter := 0 + 1."
[^typeinv-principle]: Learn TLA+, General Tips — <https://learntla.com/topics/tips.html> — "As a principle, I like the TypeInvariant to only cover the possible values of a variable, as opposed to the legitimate values."
[^lt-alldone-weak]: Learn TLA+, Concurrency (trang sống, truy cập 2026-10-12) — <https://learntla.com/core/concurrency.html> — "AllDone only says that if the processes finish, we have the correct result."
[^safety-nothing]: Specifying Systems, §14.5.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "It’s very easy to satisfy a safety property by simply doing nothing."
[^lt-threads-pass]: Learn TLA+, Concurrency (trang sống, truy cập 2026-10-12) — <https://learntla.com/core/concurrency.html> — "Now the spec passes again."
[^tc-gl-order]: Gray & Lamport, Consensus on Transaction Commit, mục 2 — <https://www.microsoft.com/en-us/research/uploads/prod/2004/01/consensus-on-transaction-commit.pdf> — "An RM can enter the committed state only after all RMs have been in the prepared state."
[^tc-gl-cons]: Gray & Lamport, Consensus on Transaction Commit, mục 2 — <https://www.microsoft.com/en-us/research/uploads/prod/2004/01/consensus-on-transaction-commit.pdf> — "Consistency It is impossible for one RM to be in the committed state and another to be in the aborted state."
[^net-set-never-removed]: TwoPhase.tla (transaction_commit), tlaplus/Examples @ 6a7b26a — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/transaction_commit/TwoPhase.tla> — "Messages are never removed from $msgs$. An action that, in an implementation, would be enabled by the receipt of a certain message is here enabled by the existence of that message in $msgs$."
[^net-dup]: TwoPhase.tla (transaction_commit), tlaplus/Examples @ 6a7b26a — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/transaction_commit/TwoPhase.tla> — "(Receipt of the same message twice is therefore allowed; but in this particular protocol, receiving a message for the second time has no effect.)"
[^tp-refine]: TwoPhase.tla (transaction_commit), tlaplus/Examples @ 6a7b26a — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/transaction_commit/TwoPhase.tla> — "This theorem asserts that the specification TPSpec of the Two-Phase Commit protocol implements the specification TCSpec of the Transaction Commit protocol."
[^deadlock-term]: Specifying Systems, §14.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "TLC normally checks for deadlock, but this checking can be disabled since, for some systems, deadlock may just indicate successful termination."
[^implied-action]: Specifying Systems, §14.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "ImpliedAction The conjunction of every action [A]v such that some property conjunct equals 2[A]v ."
[^tp-tmcommit]: TwoPhase.tla (transaction_commit), tlaplus/Examples @ 6a7b26a — <https://raw.githubusercontent.com/tlaplus/Examples/6a7b26a31c1a6aeec789b455e9bbfa3b3ad2728f/specifications/transaction_commit/TwoPhase.tla> — "The TM commits the transaction; enabled iff the TM is in its initial state and every RM has sent a $"Prepared"$ message."
[^k-ack-problem]: Apache Kafka 3.9.0, design (Consumer Position) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "if the consumer processes the message but fails before it can send an acknowledgement then the message will be consumed twice."
[^rmq-requeue]: RabbitMQ, Consumer Acknowledgements and Publisher Confirms — <https://www.rabbitmq.com/docs/confirms> — "any delivery (message) that was not acked is automatically requeued when the channel (or connection) on which the delivery happened is closed."
[^rmq-idem]: RabbitMQ, Consumer Acknowledgements and Publisher Confirms — <https://www.rabbitmq.com/docs/confirms> — "Due to this behavior, consumers must be prepared to handle redeliveries and otherwise be implemented with idempotence in mind."
[^mv-syntax]: Specifying Systems, §14.2.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "In v , any sequence of characters like a1 or foo that is not a number, a quoted string, or TRUE or FALSE is taken to be a model value."
[^ss-exp]: Lamport, Specifying Systems, mục 14.5.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "The number of reachable states is typically an exponential function of the model’s parameters"
[^start-small]: Specifying Systems, §14.5.3 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "You should always begin testing a specification with a tiny model, which TLC can check quickly."
[^cli-checkdeadlock]: Learn TLA+, Beyond the Toolbox — <https://learntla.com/topics/cli.html> — "You can also disable deadlock checking in the cfg with CHECK_DEADLOCK FALSE."
[^ss-tlc-checkalg]: Lamport, Specifying Systems, §14.3.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Let T be the set consisting of every behavior τ that is the sequence of states in an infinite path in G starting with an initial state."
[^ss-tlc-selfloop]: Lamport, Specifying Systems, §14.3.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "For every state s in G, the edge from s to s is in G."
[^tlc-scc]: tlaplus/tlaplus @94d0c50, tlc2/tool/liveness/LiveWorker.java (Javadoc) — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/src/tlc2/tool/liveness/LiveWorker.java> — "Searches for strongly connected components (SCC) a.k.a. cycles in the liveness/behavior graph."
[^ss-wf-def]: Lamport, Specifying Systems, §8.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "This formula asserts that, if A ever becomes forever enabled, then an A step must eventually occur."
[^ss-wf-88]: Lamport, Specifying Systems, §8.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "(8.8) A is infinitely often disabled, or infinitely many A steps occur."
[^ss-sf-def]: Lamport, Specifying Systems, §8.6 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "(8.32) A is eventually disabled forever, or infinitely many A steps occur. (8.33) If A is infinitely often enabled, then infinitely many A steps occur."
[^tlc-backto]: tlaplus/tlaplus @94d0c50, util/TLAConstants.java — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/src/util/TLAConstants.java> — "public static final String BACK_TO_STATE = "Back to state";"
[^tlc-stuttering]: tlaplus/tlaplus @94d0c50, util/TLAConstants.java — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/src/util/TLAConstants.java> — "public static final String STUTTERING = " Stuttering";"
[^ss-hc-stutter]: Lamport, Specifying Systems, §2.2 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "This behavior describes a clock whose display attains the value 11 and then keeps that value forever"
[^tlc-hint-nofair]: tlaplus/tlaplus @94d0c50, tlc2/output/MP.java (thông báo cấu hình) — <https://raw.githubusercontent.com/tlaplus/tlaplus/94d0c5070852b37b801bb8998c436383ab7ee37b/tlatools/org.lamport.tlatools/src/tlc2/output/MP.java> — "The stuttering counterexample above may be caused by the absence of a fairness constraint in the behavior specification"
[^ss-tlc-suspicious]: Lamport, Specifying Systems, §14.5.3, Be Suspicious of Success — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "you should be suspicious if TLC does not find a violation of a liveness property; the finite model may mask errors."
[^ss-tlc-sanity]: Lamport, Specifying Systems, §14.3.5 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "Have it check a liveness property that the specification does not satisfy and make sure it reports an error."
[^tv-shortcut]: Cirstea et al., Validating Traces of Distributed Programs Against TLA+ Specifications (arXiv 2404.16075v2), mục 4.3 — <https://arxiv.org/pdf/2404.16075v2> — "However, if some RM resends the Prepared message due to a timeout, the TM might count it twice and then commit prematurely."
[^tv-problem]: Cirstea et al., Validating Traces of Distributed Programs Against TLA+ Specifications (arXiv 2404.16075v2), mục 4.1 — <https://arxiv.org/pdf/2404.16075v2> — "Let S be the set of finite behaviors that satisfy the specification Spec, and let T be the set of finite behaviors represented by the trace, with arbitrary values assigned to variables whose values are not recorded."
[^tv-notrefine]: Cirstea et al., Validating Traces of Distributed Programs Against TLA+ Specifications (arXiv 2404.16075v2), mục 4.1 — <https://arxiv.org/pdf/2404.16075v2> — "this condition would be too strong in the presence of non-deterministically chosen values for variables not recorded in the trace."
[^pt-sm-shrink]: Proptest book, State Machine testing (trang sống), When to use State Machine testing? — <https://proptest-rs.github.io/proptest/proptest/state-machine.html> — "It does this by trying to discover a counter-example that breaks the defined properties of the system and attempts to shrink it to a minimal sequence of transitions that still reproduce the issue."
[^mdb-mbtcg-use]: Davis et al., eXtreme Modelling in Practice (arXiv 2006.00915v1), mục 5 — <https://arxiv.org/pdf/2006.00915v1> — "We wrote a new TLA+ specification for this purpose and used the state space explored by the TLC model checker to generate C++ and Golang test cases for aspects of the OT algorithm."
[^mdb-nice]: Davis et al., eXtreme Modelling in Practice (arXiv 2006.00915v1), mục 5 — <https://arxiv.org/pdf/2006.00915v1> — "These test cases can be automatically generated and run after any change to the model, and ensure that both the C++ and Golang implementations make appropriate changes to maintain equivalence."
[^coverage]: Specifying Systems, §14.5.1 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "In particular, a value of 0 indicates part of the next-state action that was never “executed”."
[^ss21-miss]: Bornholt et al., ShardStore (SOSP 2021), mục 4.2 — <https://www.cs.utexas.edu/~bornholt/papers/shardstore-sosp21.pdf> — "Property-based testing selects random sequences of operations to test, and so can miss bugs."
[^ss21-reach]: Bornholt et al., ShardStore (SOSP 2021), mục 4.2 — <https://www.cs.utexas.edu/~bornholt/papers/shardstore-sosp21.pdf> — "but these tests can only ever check system states that the test harness is able to reach."
[^pt-rng-seed]: docs.rs, proptest 1.12.0, `TestRng` — <https://docs.rs/proptest/1.12.0/proptest/test_runner/struct.TestRng.html> — "Any RNG created with the same algorithm-seed pair will produce the same sequence of values on all systems and all supporting versions of proptest."
[^tv-nofg]: Cirstea et al., Validating Traces of Distributed Programs Against TLA+ Specifications (arXiv 2404.16075v2), mục 1 — <https://arxiv.org/pdf/2404.16075v2> — "Although this approach does not provide formal correctness guarantees, even when the TLA+ specification has been extensively verified, we have found it very useful for discovering and analyzing discrepancies between the runs of distributed programs and their high-level specifications."
[^ss-sym]: Lamport, Specifying Systems, mục 14.3.4 — <https://lamport.azurewebsites.net/tla/book-02-08-08.pdf> — "If there are n processes, this reduces the number of states that TLC examines by a factor of n !."
[^pt-sm-seq]: Proptest book, State Machine testing (trang sống), Similar technologies — <https://proptest-rs.github.io/proptest/proptest/state-machine.html> — "Currently, only sequential strategy is supported, but a concurrent strategy is planned to be added at later point."
