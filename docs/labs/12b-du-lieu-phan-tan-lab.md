# Lab P12b — Hệ thống dữ liệu: phân tán

> Đây là phần **thực hành** của [P12 · Hệ thống dữ liệu](../12-he-thong-du-lieu/00-tong-quan.md), nửa **phân tán**: replication, partitioning, sự cố của hệ phân tán, consistency và consensus, stream processing, tính đúng đắn khi tích hợp dữ liệu, và Redis làm case study. Hãy đọc lý thuyết trước rồi mới làm lab: lab chỉ minh họa và kiểm chứng những gì các chương đã nêu, và mỗi lab ghi rõ chương và mục cần đọc. Mọi fact có chú thích cuối trang, kiểm bằng `python3 tools/verify_citations.py docs/labs/12b-du-lieu-phan-tan-lab.md`. Output trong bài là output **thật** đã chạy trên máy người soạn (macOS aarch64, rustc 1.95.0). Mọi lab là **mô phỏng tất định** (thời gian ảo, seed cố định, không mạng, không thread, không đồng hồ thật): chúng cho bạn thấy *cơ chế* và *bất biến* mà chương nêu, **không** đo hiệu năng hay hành vi của hệ thật. Phần chỉ chạy được trên Redis thật (`redis-cli`) được viết thành lệnh cho bạn và đánh dấu **chưa chạy**.

## Chuẩn bị và các bài lab

Crate: `labs/crates/lab-distributed`, **không phụ thuộc crate ngoài** (chỉ std). Mười một binary trong `src/bin/`, mỗi binary một mô phỏng; mô hình nằm ở thư viện (`src/<module>.rs`) và binary chỉ gọi nó rồi in kết quả. Không có code chỉ-Linux, không cần container, Redis, Kafka hay PostgreSQL thật: `cargo build -p lab-distributed` và mọi lệnh bên dưới chạy ngay trên macOS. Vì sao mô phỏng thay vì dựng cụm thật (nhận xét của người soạn): trong một cụm thật, thứ tự các sự kiện (gói tin tới trước hay sau, tiến trình bị dừng đúng lúc nào) do may rủi quyết định nên khó lặp lại; trong mô phỏng ta chọn được thứ tự đó và dò hết các thứ tự có thể, nhờ vậy mới *phản chứng* được một thiết kế ngây thơ bằng một seed cụ thể. Cái giá là mô hình chỉ đúng tới mức bạn đọc ở phần "Mô hình" của từng lab.

Quy ước chung của mọi lab:

1. **Seed.** Tham số đầu tiên của mọi binary là seed (số nguyên, mặc định 42); cùng seed thì cùng output. Bộ sinh số giả ngẫu nhiên là SplitMix64 tự cài trong `src/rng.rs` (không dùng cho mật mã), nên output không phụ thuộc phiên bản crate ngoài nào.
2. **Thời gian ảo.** Thời gian là số nguyên (ms hoặc µs) do mô phỏng tự tiến; không bao giờ gọi `Instant`/`SystemTime`. Các hằng số độ trễ, chi phí lệnh, xác suất lỗi là **tham số của mô hình** — in ở đầu output hoặc đặt tên trong code — không phải số đo.
3. **Số thực.** Phần lớn logic dùng số nguyên; số thực chỉ dùng để lấy mẫu hoặc in phần trăm. Chữ số thập phân cuối có thể khác trên nền tảng khác (chưa kiểm; người soạn chỉ chạy trên macOS aarch64).
4. **Một seed không chứng minh gì.** Bảng trong output là kết quả của một lần quét. Điều được *khẳng định* nằm ở test (`cargo test -p lab-distributed`): mỗi module quét hàng trăm seed, kiểm bất biến, và có ví dụ phản chứng cho thiết kế ngây thơ (lab nào cũng nói rõ test kiểm gì).

| Lab | Binary | Mô phỏng | Lý thuyết |
| --- | --- | --- | --- |
| (a) | `replication_lag` | leader–follower với lag; read-your-writes, monotonic reads, mất ghi khi failover | [05 §1, §2, §4](../12-he-thong-du-lieu/05-replication.md) |
| (b) | `quorum` | kho leaderless `n/w/r`, read repair, ABD, sloppy quorum | [05 §6–7](../12-he-thong-du-lieu/05-replication.md), [09 §1](../12-he-thong-du-lieu/09-consistency-va-consensus.md) |
| (c) | `partitioning` | dời bao nhiêu khóa khi N → N+1, slot Redis, vnode, hot key | [06 §1–3, §5.1, §8](../12-he-thong-du-lieu/06-partitioning.md), [16 §6](../12-he-thong-du-lieu/16-redis-ben-trong.md) |
| (d) | `fencing` | lease, process pause, fencing token | [08 §5, §7, §8](../12-he-thong-du-lieu/08-su-co-he-phan-tan.md), [09 §8](../12-he-thong-du-lieu/09-consistency-va-consensus.md) |
| (e) | `lamport` | Lamport/vector clock, LWW, total order broadcast | [09 §3–4](../12-he-thong-du-lieu/09-consistency-va-consensus.md), [08 §6](../12-he-thong-du-lieu/08-su-co-he-phan-tan.md) |
| (f) | `raft_election` | Raft: bầu leader, sao chép log, năm bất biến, đột biến | [09 §6–7](../12-he-thong-du-lieu/09-consistency-va-consensus.md) |
| (g) | `delivery_dedup` | at-least-once, idempotency key, outbox dưới crash | [11 §4](../12-he-thong-du-lieu/11-stream-processing.md), [12 §1, §3–6](../12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md) |
| (h) | `watermark` | event time, tumbling window, watermark, dữ liệu trễ | [11 §6](../12-he-thong-du-lieu/11-stream-processing.md) |
| (i) | `redis_loop` | một luồng, ranh giới nguyên tử, replication bất đồng bộ và `WAIT` | [16 §1, §5, §7](../12-he-thong-du-lieu/16-redis-ben-trong.md) |
| (j) | `rate_limiter` | fixed window, sliding log, token bucket | [17 §2](../12-he-thong-du-lieu/17-redis-patterns.md) |
| (k) | `cache_stampede` | stampede, stale set, nhả khóa, negative caching | [17 §1, §3](../12-he-thong-du-lieu/17-redis-patterns.md) |

Mọi lệnh chạy từ thư mục `labs/`: `cargo run -q -p lab-distributed --bin <binary> -- [seed]`. Máy người soạn:

```bash
rustc -Vv | head -3
```

```text
rustc 1.95.0 (59807616e 2026-04-14)
binary: rustc
commit-hash: 59807616e1fa2540724bfbac14d7976d7e4a3860
```

Test nằm trong từng module (`quorum::tests::…`, `raft::tests::…`…). Số test mỗi module và kết quả tổng:

```bash
cd labs && cargo test -p lab-distributed -- --list 2>/dev/null | grep ': test$' | sed 's/::.*//' | sort | uniq -c
cd labs && cargo test -q -p lab-distributed 2>&1 | grep -E '^(running|test result)' | head -2    # exit 0
```

```text
  19 cache
  17 clocks
  21 delivery
  12 fencing
  18 limiter
  22 partition
  13 quorum
  36 raft
  23 redis_loop
  13 replication
   7 rng
  13 windows
running 214 tests
test result: ok. 213 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in <thời gian phụ thuộc máy>s
```

Một test của module `raft` (soak) bị `#[ignore]` để `cargo test` mặc định nhanh; lab (f) nói cách chạy nó.

### Lab (a) — `replication_lag`: lag, đọc từ follower và mất ghi khi failover

Lý thuyết: [chương 05 mục 1](../12-he-thong-du-lieu/05-replication.md) (async / sync / semi-sync, Đi sâu: `remote_apply`, điểm chờ của MariaDB), [mục 2](../12-he-thong-du-lieu/05-replication.md) (failover mất ghi chưa nhân bản), [mục 4](../12-he-thong-du-lieu/05-replication.md) (replication lag; read-your-writes, monotonic reads, consistent prefix; cách chữa) và [mục "Liên hệ với Rust"](../12-he-thong-du-lieu/05-replication.md) ý 3 (token LSN).

**Mô hình của lab** (thời gian ảo, đơn vị ms): một leader có log chỉ-thêm `(lsn, key)`; `F = 2` follower nhận *mọi* entry theo thứ tự log sau một độ trễ mạng, rồi entry mới *đọc được* sau thêm một độ trễ replay. Độ trễ của từng entry có jitter nhưng bị ép đơn điệu theo từng follower, nên follower luôn giữ một **prefix** của log leader (hàm `Cluster::check_prefix`, được `assert!` ở cuối mỗi `run`). Follower cuối cố ý chậm: tổng lag `lag` ms, gồm nhận `lag/4` rồi replay `3·lag/4`; follower nhanh bằng 1/4 con số đó. 400 session, mỗi session làm 3 vòng: ghi key của mình, đợi ack, nghỉ `think = 20` ms, rồi đọc 3 lần cách nhau 5 ms. Một key được 4 session ghi chung. Mô hình **không** có: LSN thật (chỉ là bộ đếm), follower sập, bầu leader hay timeline rẽ nhánh, hàng đợi/CPU, độ trễ client–leader, replay xung đột với query đọc; mọi giá trị là *mô hình của lab*, không phải số đo của database nào.

Chính sách đọc (`Policy`): `Leader`; `RandomFollower` (ngây thơ); `StickyFollower` (session ghim một follower — hướng "độ dính" của Vogels[^a-v-sticky]); `LeaderForWindow(w)` (đọc leader `w` ms sau ack của lần ghi của mình, sau đó đọc follower ngẫu nhiên); `Token` (session nhớ `max(LSN lần ghi cuối, version đã đọc)` và chỉ đọc follower đã apply tới đó, không có thì đọc leader — ý tưởng write-set của Terry[^a-t94-ws] rút thành một LSN — suy luận của người soạn). Mức ack (`Ack`): `Async`; `Received(k)` (k follower đã *nhận* entry và trả lời về — tương tự `on` của PostgreSQL, chờ standby nhận và flush[^a-pg-on]; mô hình coi flush là tức thì); `Applied(k)` (k follower đã *replay* — tương tự `remote_apply`[^a-pg-ra]). Câu trả lời của follower quay về leader mất đúng độ trễ nhận cơ sở của follower đó.

Đo: **RYW%** = tỉ lệ lần đọc trả version nhỏ hơn lần ghi gần nhất của chính session; **MR%** = tỉ lệ lần đọc trả version nhỏ hơn version session đã đọc trước đó; **lead%** = tỉ lệ lần đọc do leader phục vụ. **Failover**: ở 200 thời điểm ngẫu nhiên mỗi run, leader sập; follower có LSN *đã nhận* cao nhất lên làm leader; đếm số ghi *đã được ack cho client* trước lúc sập mà leader mới không có (entry còn "trên dây" coi như mất — cách đếm bảo thủ).

```bash
cd labs && cargo run -q -p lab-distributed --bin replication_lag    # exit 0
```

Tham số tùy chọn: `[seed=42] [runs=10]`; kết quả cộng dồn trên các seed `seed..seed+runs`. Toàn bộ là số nguyên (phần trăm làm tròn bằng số nguyên), nên không có chuyện lệch chữ số thập phân giữa nền tảng. Output (macOS aarch64, rustc 1.95.0; xác định theo seed, chạy hai lần `diff` giống hệt):

```text
Lab (a) replication_lag: seeds 42..51 (10 runs, summed)
F=2 followers (last = slow), 400 sessions x 3 rounds, 100 keys, think=20 ms, 3 reads/round 5 ms apart
lag = total lag of the slow follower: recv lag/4 then replay 3*lag/4; fast follower = 1/4 of that; jitter <= stage/4

[1] read policy, ack=Async: RYW% / MR% / leader-read%
policy                      lag=0 ms              lag=20 ms              lag=80 ms
                         RYW%    MR%  lead%     RYW%    MR%  lead%     RYW%    MR%  lead%
Leader                    0.0    0.0  100.0      0.0    0.0  100.0      0.0    0.0  100.0
RandomFollower            0.0    0.0    0.0     16.3    2.1    0.0     66.4   32.4    0.0
StickyFollower            0.0    0.0    0.0     16.0    0.0    0.0     65.9    0.0    0.0
LeaderForWindow(30)       0.0    0.0   66.7      0.0    2.3   66.7     16.8   19.1   66.7
LeaderForWindow(120)      0.0    0.0  100.0      0.0    0.0  100.0      0.0    0.0  100.0
Token                     0.0    0.0    0.0      0.0    0.0    0.0      0.0    0.0   40.4

[2] ack level at lag=40 ms, reads=RandomFollower; failover = 200 random leader crashes per run
ack            lat mean lat max    RYW%     MR%   lost/crash  crash w/loss%
Async               0.0       0    50.1    20.6          2.2           86.3
Received(1)         4.0       4    50.1    20.6          0.0            0.0
Received(all)      21.3      22    23.9     7.5          0.0            0.0
Applied(1)         11.6      12    49.8    20.4          0.0            0.0
Applied(all)       56.3      59     0.0     4.5          0.0            0.0
Async acked writes lost per crash by lag: lag=0: 0.0  lag=20: 1.1  lag=80: 6.1

[3] consistent prefix: 40 order+payment pairs, 500 probes per seed; probes showing 'payment without its order'
lag     2 logs (shards)       1 log
0                0/5000      0/5000
20             417/5000      0/5000
80            1173/5000      0/5000
```

Giải thích:

1. **Lag + think ⇒ mất read-your-writes.** Ở `lag=0` mọi chính sách cho 0.0% — follower áp dụng tức thì nên không có gì để vỡ. Khi `lag` tiến tới `think`, `RandomFollower` đọc phải follower chậm: 16.3% lần đọc không thấy lần ghi của chính mình (`lag=20`), 66.4% ở `lag=80`. Đây chính là tình huống "F5 vẫn thấy avatar cũ" của chương: read-your-writes nghĩa là đọc lại đúng giá trị vừa ghi hoặc mới hơn[^a-ryw][^a-ryw-strict]; còn người dùng thấy rất khó hiểu khi bản cập nhật như biến mất[^a-t94-ryw]. Cột MR% của cùng dòng (2.1% rồi 32.4%) là lần đọc *lùi*: thấy bản mới ở follower nhanh rồi bản cũ ở follower chậm — Terry mô tả monotonic reads là thấy kho dữ liệu ngày càng cập nhật hơn[^a-t94-mr]. Độ trễ replication của PostgreSQL cũng được docs mô tả như vậy: dữ liệu ở standby "eventually consistent" với primary[^a-pg-hot-ec].
2. **`StickyFollower`: MR = 0 nhưng RYW vẫn vỡ.** Một follower chỉ tiến lên, nên cùng session đọc cùng follower thì không bao giờ lùi (MR% = 0.0 ở cả ba mức lag). Nhưng *ghi* vẫn đi qua leader, còn follower được ghim có thể chưa replay xong: RYW% 16.0 và 65.9, gần như bằng `RandomFollower` vì khoảng một nửa session bị ghim vào follower chậm. Vogels nói nếu luôn là cùng một server thì tương đối dễ bảo đảm cả hai thuộc tính[^a-v-sticky] — "cùng server" ở đó gồm cả đường ghi; ở mô hình của lab ghi và đọc đi hai nơi, nên chỉ MR có được (suy luận của người soạn).
3. **`LeaderForWindow(w)`: cửa sổ phải dài hơn lag, và nó chỉ dời lỗi.** `w=30` bảo vệ ở `lag=20` (RYW 0.0) nhưng ở `lag=80` follower chậm cần ~80 ms nên 16.8% lần đọc vẫn trượt; hơn nữa MR% là 2.3 rồi 19.1 vì lần đọc leader (mới) được theo sau bởi lần đọc follower (cũ). `lead% = 66.7` cố định vì mỗi session đọc ba lần ở +20, +25 và +30 ms sau ack (think = 20 ms, cách nhau 5 ms) và cửa sổ là 30 ms: 2/3 lần đọc (+20, +25) nằm trong cửa sổ còn 1/3 (+30) nằm ngoài. `w=120` phủ hết các lần đọc nên thực chất là `Leader` (lead% = 100.0) — số 0.0 của nó chỉ chứng tỏ cửa sổ đủ dài, không phải một mẹo miễn phí: toàn bộ tải đọc dồn về leader.
4. **`Token`: 0.0% RYW và 0.0% MR ở mọi lag, trả giá bằng fallback.** Chính sách chỉ chọn follower đã apply tới LSN của session — đúng kiểu "ghi nhớ vị trí log, đọc từ replica đã đi qua vị trí đó" ở chương 05 mục "Liên hệ với Rust" ý 3; cơ chế gốc là write-set của Terry[^a-t94-ws]. Cái giá hiện ra ở `lag=80`: 40.4% lần đọc phải quay về leader vì không follower nào đủ mới; ở `lag=20` follower nhanh luôn đủ mới nên fallback 0.0%. Lab không thử phương án thay thế là chờ follower bắt kịp; đó là bài tập mở rộng của lab (a) về `Policy::Token` chờ tối đa `wait` ms.
5. **Ack level đổi chỗ lỗi, không xóa nó.** Bảng [2] (latency tính bằng ms): `Async` ack ngay (latency 0) nhưng mất ghi khi failover — mỗi thời điểm sập mất trung bình 2.2 ghi đã ack, và 86.3% thời điểm sập mất ít nhất một. Khớp mô tả của PostgreSQL: async thì ghi đã commit có thể chưa tới standby và lượng mất tỉ lệ với độ trễ replication[^a-pg-loss][^a-pg-async]; dòng cuối của [2] (0.0 → 1.1 → 6.1 khi lag 0 → 20 → 80) tăng theo lag *cùng hướng* với câu đó, lab không kiểm tỉ lệ chính xác. `Received(1)` và mọi mức `Received`/`Applied` đều cho `lost/crash = 0.0`: ack chỉ phát ra khi ít nhất một follower đã nhận entry, và follower có LSN nhận cao nhất được thăng cấp luôn có entry đó. Đây là đúng lời hứa mà PostgreSQL phát biểu — ứng dụng không nhận xác nhận trước khi WAL được biết là đã tới các standby đồng bộ[^a-pg-ack]; trong lab, follower không sập nên "còn một follower sống" luôn đúng.
6. **`Received` ≠ `Applied`.** Cột RYW%: `Received(1)` y hệt `Async` (50.1%) vì ack quay về sau vài ms trong khi follower chậm còn lag 40 ms; `Received(all)` giảm còn 23.9% nhưng chưa về 0 vì entry mới *nhận*, chưa *replay*; chỉ `Applied(all)` cho 0.0% với `RandomFollower`, đúng ý docs: `remote_apply` chờ standby replay xong để thay đổi nhìn thấy được, và trong trường hợp đơn giản cho phép cân bằng tải với causal consistency[^a-pg-ra]. Đó cũng là điểm MariaDB nhấn mạnh: xác nhận semisync nói về relay log của replica, chưa chắc đã được áp dụng[^a-m-relay]. Giá phải trả là cột latency: trung bình 4.0 → 21.3 → 56.3 ms; PostgreSQL docs nói chờ standby thì thời gian phản hồi tăng ít nhất bằng một round-trip[^a-pg-rtt] và `remote_apply` còn chậm hơn vì chờ replay WAL[^a-pg-ra-cost]. `Applied(1)` vẫn vỡ RYW (49.8%): chờ *một* follower, mà đọc lại có thể rơi vào follower kia.
7. **`Applied(all)` vẫn còn MR% = 4.5.** RYW là chuyện "bản của tôi", MR là chuyện "bản tôi đã thấy"; key có 4 người ghi nên một lần đọc ở follower nhanh có thể thấy bản của *người khác* mới hơn, rồi lần sau rơi vào follower chậm chưa có. Terry nói các bảo đảm này không cái nào mạnh hơn cái nào[^a-t11-indep]; muốn cả hai phải kết hợp, như `Token` ở bảng [1]. Mức ack không thay được chính sách định tuyến.
8. **Consistent prefix.** Bảng [3]: `đơn hàng` và `thanh toán` ở hai shard với hai log độc lập, người đọc một replica mỗi shard thấy "có thanh toán mà không có đơn" ở 417/5000 (lag 20) và 1173/5000 (lag 80) thời điểm; với một log duy nhất là 0/5000 ở mọi lag, và độc lập mà `lag=0` cũng 0. Một log nghĩa là replica chỉ thấy một prefix nên không thể thấy payment mà thiếu order được commit trước nó; Terry mô tả consistent prefix là thấy chuỗi ghi có thứ tự bắt đầu từ lần ghi đầu[^a-t11-cp], tức một phiên bản kho dữ liệu từng tồn tại ở master[^a-t11-cp-snap]. Mô hình này chỉ đếm; nó không nói hệ nào cho consistent prefix.
9. **Một seed không chứng minh.** Bảng chỉ là tổng trên 10 seed. Các bất biến được kiểm bằng `cargo test -p lab-distributed replication::` (13 test của module `replication`): mọi follower giữ prefix ở mọi thời điểm dò, với 30 seed × 3 mức lag × 1, 2, 3 follower; `Leader` và `Token` có **0** RYW và **0** MR trên 50 seed × 3 mức lag (hơn 10 000 lần đọc mỗi tổ hợp); `RandomFollower` có RYW và MR > 0 khi lag ≫ think và đúng 0 khi lag = 0; `StickyFollower` MR = 0 nhưng RYW > 0; `Received(1)`, `Received(all)`, `Applied(·)` mất 0 ghi đã ack qua 50 seed × 100 thời điểm sập × 3 lag, còn `Async` mất > 0 và tăng theo lag; `Applied(all)` ⇒ RYW = 0 với `RandomFollower` (phản ví dụ: `Applied(1)` và `Received(all)` vẫn có RYW > 0); độ trễ ack khớp công thức khi tắt jitter; hai log độc lập cho anomaly > 0, một log cho đúng 0; cùng seed cho cùng `Report`.

### Lab (b) — `quorum`: quorum `w + r > n`, read repair, ABD và sloppy quorum

Lý thuyết: [chương 05 mục 6](../12-he-thong-du-lieu/05-replication.md) (quorum, `w + r > n`, sloppy quorum, hinted handoff, "Quorum không phải linearizable"), [mục 7](../12-he-thong-du-lieu/05-replication.md) (read repair), [chương 09 mục 1](../12-he-thong-du-lieu/09-consistency-va-consensus.md) (bước 4–5: phản ví dụ quorum và ghi ngược kiểu ABD).

**Mô hình của lab** (`labs/crates/lab-distributed/src/quorum.rs`). `n` replica, mỗi replica giữ một version cho một khóa và chỉ đi tới: nhận version đến khi và chỉ khi nó lớn hơn version đang giữ. Version do *một* writer logic sinh ra (bộ đếm tăng dần). Quorum là số node tối thiểu phải phản hồi để yêu cầu thành công[^b-quorum-def]; ở đây ghi thành công khi ít nhất `w` replica áp dụng, đọc thành công khi ít nhất `r` replica trả lời và trả về version cao nhất trong các câu trả lời. Một lần đọc **stale** nếu trả version thấp hơn bản ghi *đã được xác nhận* gần nhất.

* Mục 1, 2 và 2b của output (bảng, read repair, trace tối thiểu): các thao tác chạy lần lượt, không chồng lấn; mỗi tin nhắn tới mỗi replica mất độc lập với xác suất `loss` (mặc định 0.2). Ghi *thất bại* (chạm chưa tới `w` replica) vẫn để lại bản sao ở những replica đã nhận. Bảng quét 200 seed × 200 thao tác (50% ghi) cho mỗi dòng.
* Mục 3 (kịch bản cố định, không ngẫu nhiên): phản ví dụ của bài giảng, có và không có write-back.
* Mục 4 (sloppy quorum): 3 replica nhà + 2 node standby, `w = r = 2`, `loss = 0.4`; mỗi vòng ghi → đọc lúc sự cố → giao hint → đọc lại. Hai lần đọc theo *cùng một luật*: hỏi các replica nhà, mỗi replica tới được với xác suất `1 − loss`, thành công khi có ít nhất `r` trả lời, trả version cao nhất. Strict và sloppy chạy trên *cùng* dãy lỗi; dòng thứ ba là phản chứng "standby không bao giờ giao hint" (`deliver_hints = false`).

**Mô hình không có**: đồng thời thật (trừ kịch bản mục 3), timeout, đồng hồ, vector clock/siblings, coordinator và preference list trên vòng consistent hashing, hint hết hạn hay standby chết, anti-entropy. Lỗi là mất tin nhắn độc lập theo từng thao tác, không phải sập node kéo dài. Đây **không** phải cài đặt của Dynamo hay Cassandra và các số bên dưới không phải số đo của hệ thật.

```bash
cd labs && cargo run -q -p lab-distributed --bin quorum            # exit 0 (seed 42, loss 0.2)
cd labs && cargo run -q -p lab-distributed --bin quorum -- 7 0.3   # seed 7, loss 0.3
```

Output (macOS aarch64, rustc 1.95.0; xác định theo seed; phần trăm in từ `f64`, chữ số thập phân cuối có thể khác trên nền tảng khác — chưa kiểm):

```text
Lab (b) quorum - seed 42, loss 0.2, 200 seeds x 200 ops (50% writes)

== 1. success rates and stale reads (ghost = reads that saw a FAILED write) ==
n  w  r  w+r  w+r>n  write%   read%   stale  stale%  ghost
3  1  1    2  false     99.2     99.1     633    3.19      0
3  1  2    3  false     99.2     89.7     268    1.49      0
3  1  3    4   true     99.2     51.5       0    0.00      0
3  2  1    3  false     89.7     99.1     258    1.30   1554
3  2  2    4   true     89.7     89.7       0    0.00   1487
3  2  3    5   true     89.7     51.5       0    0.00    979
3  3  1    4   true     51.5     99.1       0    0.00   8977
3  3  2    5   true     51.5     89.7       0    0.00   8316
3  3  3    6   true     51.5     51.5       0    0.00   4901
5  3  3    6   true     94.0     93.8       0    0.00   1041
5  2  2    4  false     99.3     99.2      62    0.31     99
5  1  5    6   true    100.0     33.0       0    0.00      0
5  5  1    6   true     32.6     99.9       0    0.00  13373
5  3  2    5  false     94.0     99.2      23    0.12   1082
5  4  1    5  false     74.0     99.9      16    0.08   5108

== 2. read repair, same seeds ==
n=3 w=1 r=2: stale   268 (1.49%) ->   149 (0.84%) with read repair
n=5 w=2 r=2: stale    62 (0.31%) ->    34 (0.17%) with read repair
n=5 w=2 r=3: stale    24 (0.13%) ->     8 (0.04%) with read repair

== 2b. minimal stale-read trace, n=3 w=1 r=1 ==
seed 176:
  write v1: applied by {B} -> ACK
  read: C:v0 -> returns v0 STALE (latest acked is v1)

== 3. scripted trace n=3 w=2 r=2: write v1 reached only A ==
  writer: write v1 sent to A,B,C; only A has applied it so far (B, C in flight)
  reader 1: reads {A,B}: A:v1 B:v0 -> highest is v1
  reader 1: returns v1
  reader 2: starts AFTER reader 1 returned; reads {B,C}
  reader 2: B:v0 C:v0 -> returns v0   <- older than what reader 1 returned!
  -- with ABD write-back, instead:
  reader 1: write-back v1 to {A,B}, waits for both acks -> B:v1
  reader 2: B:v1 C:v0 -> returns v1

== 4. sloppy quorum n=3 w=2 r=2 + 2 standbys, loss 0.4 ==
Strict: writes ok 12955 rejected 7045 | stale before handoff 0/12938 after 0/13067 | hints delivered 0
Sloppy: writes ok 18264 rejected 1736 | stale before handoff 995/12938 after 0/13067 | hints delivered 15762
Sloppy, no handoff: writes ok 18264 rejected 1736 | stale before handoff 1047/12938 after 1027/13067 | hints delivered 0
```

Giải thích:

1. **Mục 1: `stale = 0` đúng ở những dòng `w+r>n = true`.** Khi `w + r > n`, tập ghi và tập đọc luôn giao nhau nên quorum đọc chứa ít nhất một node đã xác nhận ghi[^b-intersect]; Dynamo gọi `R + W > N` là hệ "quorum-like"[^b-rw], Cassandra viết cùng ý là `W + R > RF`[^b-cass]. Các dòng `false` đều có `stale > 0` (ví dụ `n=3, w=1, r=1`: 633 lần). Vì mô hình là chuỗi thao tác tuần tự, `stale = 0` ở đây là hệ quả *chứng minh được* của giao nhau, không chỉ là may mắn của seed; test kiểm cả hai chiều (xem cuối mục).
2. **Giá phải trả là availability.** `write%` ở `n=3` là 99.2 / 89.7 / 51.5 cho `w = 1, 2, 3` — khớp xác suất "ít nhất `w` trong 3 replica tới được" với mỗi replica tới được 0.8 (`1 − 0.2³ = 0.992`, `0.896`, `0.8³ = 0.512`; tính của người soạn). `read%` giảm theo `r` y hệt. `(5,1,5)` cho `read% = 33.0` (≈ 0.8⁵): `w = 1, r = n` ghi dễ nhưng mỗi lần đọc cần cả 5 replica. Dynamo nêu cấu hình thường dùng là (N,R,W) = (3,2,2)[^b-config]; trong các dòng `n=3` có `w+r>n`, `(2,2)` cân bằng nhất: tỉ lệ thấp hơn trong hai tỉ lệ (ghi, đọc) cao nhất (89.7 / 89.7), còn `(1,3)` và `(3,1)` đẩy một phía lên 99% và phía kia xuống 51% (nhận xét của người soạn từ bảng này, không phải kết luận cho hệ thật).
3. **Cột `ghost` (mục 1): ghi "thất bại" không có nghĩa là không có dấu vết.** `ghost` đếm lần đọc thành công trả về version *cao hơn* bản ghi đã xác nhận, tức đọc thấy một bản ghi mà client đã được báo là thất bại. Nó xuất hiện cả khi `w + r > n` (ví dụ `n=3, w=3, r=1`: 8977 lần) và bằng 0 khi `w = 1` (ghi chỉ thất bại nếu chạm 0 replica). Cassandra nêu đúng hiện tượng này: khi chưa có monotonic quorum reads và một ghi không tới quorum, nó có thể hiện ra ở một lần đọc rồi biến mất ở lần đọc sau[^b-vanish]. Mô hình không tính `ghost` là vi phạm; nó chỉ cho thấy "thất bại" ở client và "không có tác dụng" ở replica là hai chuyện khác nhau.
4. **Read repair (mục 2).** Sau mỗi lần đọc thành công, replica trả lời mà đang cũ hơn được gửi lại version cao nhất (tin nhắn này cũng có thể mất). Stale giảm: 268 → 149 (`n=3, w=1, r=2`), 62 → 34, 24 → 8, trên đúng các seed như nhau. Ý tưởng là của bài giảng[^b-rr-notes] và của Dynamo, nơi coordinator cập nhật các node trả bản cũ sau khi đã trả lời caller[^b-rr-dynamo]. Nhưng stale **không về 0**: repair chỉ chạy *sau* lần đọc, chỉ sửa replica đã trả lời, và có thể mất tin nhắn; nó không biến `w + r ≤ n` thành an toàn. Bài giảng cũng nói trong cách tiếp cận quorum, tại một thời điểm một số cập nhật có thể thiếu ở vài replica[^b-missing] — nên cần các lớp bù (nhận xét của người soạn: read repair chỉ là một lớp).
5. **Trace tối thiểu (mục 2b, `n=3, w=1, r=1`).** Trong 1000 seed bắt đầu từ seed gốc, seed 176 cho trace ngắn nhất (2 sự kiện): ghi v1 chỉ chạm B và vẫn được xác nhận vì `w = 1`; đọc kế tiếp chỉ C trả lời → v0. Hai tập `{B}` và `{C}` không giao nhau, đúng điều `w + r = 2 ≤ 3` không loại trừ.
6. **Kịch bản mục 3: `w + r > n` vẫn không phải linearizability.** v1 mới chỉ tới A (các bản gửi tới B, C còn trên đường). Reader 1 đọc `{A,B}` thấy v1 và trả v1. *Sau khi* reader 1 đã trả, reader 2 đọc `{B,C}` và chỉ thấy v0. Cả hai quorum đều đủ `r = 2` và `w + r = 4 > 3`, nhưng reader 2 thấy cũ hơn reader 1 — đúng phản ví dụ trong bài giảng, nơi hành vi này "không được phép trong hệ linearizable"[^b-nonlin] (xem [chương 05 mục 6](../12-he-thong-du-lieu/05-replication.md), [chương 09 mục 1](../12-he-thong-du-lieu/09-consistency-va-consensus.md)). Với write-back kiểu ABD, reader 1 gửi lại v1 cho `{A,B}` và chờ đủ ack *trước khi trả*, nên B đã có v1 và `{B,C}` thấy v1: bài giảng mô tả ABD là ghi ngược giá trị mới nhất cho các replica chưa có, như read repair[^b-abd], và chỉ kết thúc khi giá trị mới nhất đã nằm trên một quorum[^b-abd-wait]. Điểm khác read repair của mục 2 chỉ là *chờ* trước khi trả (suy luận của người soạn). Test duyệt *mọi* cặp quorum đọc và mọi tập ghi dở dang cho `n = 3..6`, không chỉ cặp `{A,B}`/`{B,C}`.
7. **Sloppy quorum (mục 4).** Strict: ghi chỉ lên replica nhà nên bị từ chối khi chưa đủ `w` replica nhà tới được — 7045/20000 lần bị từ chối, nhưng stale = 0 cả trước lẫn sau. Sloppy: ghi lên `n` node *tới được* đầu tiên của danh sách `3 nhà + 2 standby`, standby giữ hint — Dynamo mô tả đúng như vậy: mọi đọc/ghi chạy trên `N` node khỏe đầu tiên của preference list, không nhất thiết `N` node đầu trên vòng[^b-sloppy], bản của node hỏng được gửi cho node khác kèm hint về người nhận dự kiến[^b-hint], và preference list chứa nhiều hơn `N` node để chịu lỗi[^b-pref]. Kết quả: chỉ 1736 ghi bị từ chối (Dynamo cũng nói quorum truyền thống sẽ không khả dụng khi server hỏng hay mạng bị chia[^b-strict]) nhưng 995/12938 lần đọc lúc sự cố là **stale dù `w + r = 4 > 3`**, vì đọc chỉ hỏi replica nhà, trong khi bản ghi mới còn nằm ở standby. Sau khi hint được giao (15762 lần), lần đọc thứ hai — *cũng* là đọc quorum `r = 2` trên replica nhà, cùng luật với lần đầu — cho stale = 0/13067. Dòng `no handoff` cho thấy đây là tác dụng của việc giao hint: cùng seed, cùng lỗi, cùng số ghi được nhận, nhưng nếu standby không giao lại thì lần đọc thứ hai vẫn stale 1027/13067 (và 1047/12938 ở lần đọc đầu, vì phần dư của các vòng trước không được sửa). Lý do 0 là chắc chắn chứ không do may: sau khi giao hint, ít nhất `w` replica nhà giữ version đã xác nhận, nên mọi quorum đọc `r` với `w + r > n` giao với chúng (suy luận của người soạn từ mô hình). Dynamo cũng nói hinted handoff chỉ hoạt động tốt khi membership ít biến động và lỗi node là tạm thời[^b-hint-limit]; mô hình của lab giao hint đúng một lần cuối mỗi vòng và không bao giờ mất hint, nên 0 sau handoff là cận *lạc quan* (nhận xét của người soạn). Sloppy quorum đổi bảo đảm giao nhau lấy availability; đó là lựa chọn thiết kế, không phải lỗi, và Dynamo mô tả mình là eventual consistency: cập nhật lan bất đồng bộ tới mọi replica[^b-eventual].

Một seed không chứng minh gì, nên test (`cargo test -p lab-distributed quorum::`, 13 test của module `quorum`) kiểm: (a) với mọi `n ≤ 7` và mọi `(w, r)`, bằng bitmask, mọi tập ≥ `w` giao mọi tập ≥ `r` **khi và chỉ khi** `w + r > n`; (b) `w + r > n` ⇒ `stale = 0` với `n = 1..5`, mọi `(w, r)`, `loss ∈ {0, 0.1, 0.3, 0.5}`, có và không read repair, 100 seed mỗi cấu hình; (c) `w + r ≤ n` (`n = 2..5`, `loss ≥ 0.2`) ⇒ `stale > 0` tính gộp trên 200 seed, và `stale = 0` khi `loss = 0`; (d) trace 2 sự kiện của `n=3, w=1, r=1` ở seed 176 và không có trace stale nào cho `w + r > n`; (e) tỉ lệ thành công giảm theo `w`, `r` và khớp 0.992 / 0.896 / 0.512 trong ±1.5%; (f) `ghost > 0` với `n=3, w=3, r=1`; (g) read repair giảm stale ngặt trên cùng seed; (h) kịch bản v1→v0 và v1→v1, cùng đếm chính xác 6 cặp quorum đảo ngược ở `n=3, w=r=2` (và 0 khi có write-back); (i) sloppy: stale trước handoff > 0, sau handoff = 0 *ở từng seed* (lần đọc thứ hai là đọc quorum `r` như lần đầu), còn khi `deliver_hints = false` thì stale sau đó > 0 trên cùng 200 seed; strict từ chối nhiều hơn, và không có standby thì sloppy ≡ strict.

### Lab (c) — `partitioning`: dời bao nhiêu khóa, slot Redis, vnode và hot key

Lý thuyết: [chương 06 mục 1](../12-he-thong-du-lieu/06-partitioning.md) (key range vs hash, khóa tăng đơn điệu), [mục 2](../12-he-thong-du-lieu/06-partitioning.md) (skew, hot key, hậu tố), [mục 3](../12-he-thong-du-lieu/06-partitioning.md) (vì sao `hash mod N` hỏng, consistent hashing, vnode, jump hash), [mục 5.1](../12-he-thong-du-lieu/06-partitioning.md) (số partition cố định), [mục 8](../12-he-thong-du-lieu/06-partitioning.md) (hàm băm có đặc tả, CRC16 + hash tag của Redis); [chương 16 mục 6](../12-he-thong-du-lieu/16-redis-ben-trong.md) (Redis Cluster) và [chương 17 mục 1.5](../12-he-thong-du-lieu/17-redis-patterns.md) (hot key, bản sao `product:42#0…#7`), [mục 8.2](../12-he-thong-du-lieu/17-redis-patterns.md) (hash tag khi đặt tên key).

**Mô hình của lab.** Module `partition` là code của người soạn, chạy trong một process, không có mạng và không có dữ liệu thật:

- `hash64` là hàm băm 64 bit **riêng của lab** (một vòng nhân/xor theo byte rồi một bước trộn bit cuối), có đặc tả cố định và test vector được ghim trong test; lab không dùng `Hash`/`DefaultHasher` của std vì docs std nói dữ liệu `Hash` đưa cho `Hasher` không được coi là portable giữa các nền tảng[^c-std-hash-portable] và test không nên so với giá trị băm cứng[^c-std-hash-hardcoded] — hàm chọn partition phải là một hợp đồng dữ liệu ([chương 06 mục 8](../12-he-thong-du-lieu/06-partitioning.md)). Đây không phải hàm băm của hệ thống nào; chất lượng của nó chỉ được kiểm bằng các test thống kê ở dưới.
- Bốn cách đặt khóa lên `N` node: `ModN` (`hash % N`); `FixedPartitions` (P = 1024 partition cố định, `partition = hash % P` không bao giờ đổi, chỉ bảng partition → node được sửa, mỗi node mới lấy `⌊P/(N+1)⌋` partition từ node đang giữ nhiều nhất — cùng ý với "Q/S token mỗi node" của Dynamo[^c-dyn-s3-def]); `Ring` (mỗi node `v` điểm trên vòng 64 bit, khóa đi theo chiều kim đồng hồ tới điểm đầu tiên[^c-dyn-ring], nhiều điểm mỗi node là virtual node[^c-dyn-vnodes]); `Jump` (thuật toán Figure 1 của paper Lamping–Veach, đầu vào là khóa 64 bit và số bucket, đầu ra là số bucket trong `[0, n)`[^c-jump-alg]).
- Redis Cluster: `crc16_xmodem`, `hash_tag`, `hash_slot` theo đặc tả (CRC-16/XMODEM[^c-redis-crc-spec], `HASH_SLOT = CRC16(key) mod 16384`[^c-redis-formula], quy tắc `{...}`[^c-redis-tag-rule]) và `SlotMap` (chia 16384 slot cho các master, thêm master thì lấy số slot tối thiểu).
- Skew: `Zipf` (xác suất hạng `r` tỉ lệ `1/r`, lấy mẫu bằng bảng CDF số nguyên), `hot_key_run` (200.000 request, tính tải từng node), hai cách giảm nhẹ: hậu tố `key#0..key#S-1` (ghi chọn một hậu tố, đọc phải hỏi cả `S`)[^c-ddb-suffix][^c-ddb-suffix-read] và bản sao (đọc chọn một bản, ghi cập nhật cả `S`).
- Range vs hash cho khóa tăng đơn điệu: `static_range_partition` (ranh giới cố định chia đều) và `SplitRangeTable` (tách partition tại khóa giữa khi quá 1000 khóa).

**Mô hình không có:** di chuyển dữ liệu thật (băng thông, `-MOVED`/`-ASK`[^c-redis-asks], giữ phục vụ trong lúc chuyển), replica, cache phía client, gossip/bảng định tuyến, node chết, chi phí duy trì vnode (Cassandra nêu mỗi token thêm tối đa `2 × (RF − 1)` hàng xóm[^c-cass-vnode-cost] — lab không đo cái này). Phân phối Zipf s = 1 là **giả định của người soạn**, không phải số đo từ workload thật. Kết quả mô phỏng không phải số đo của Redis, Cassandra hay Dynamo. Có dùng số thực (`f64`) ở Jump, bảng Zipf và định dạng in: chữ số thập phân cuối có thể khác trên nền tảng khác (chưa kiểm).

```bash
cd labs && cargo run -q -p lab-distributed --bin partitioning -- 42    # exit 0
```

Tham số tùy chọn: `-- <seed> <1..5>` chỉ in một mục. Output (macOS aarch64, rustc 1.95.0; xác định theo seed; một lần chạy, tách thành hai khối cho dễ đọc):

```text
== Lab (c) partitioning, seed 42 ==
-- (1) keys moved when growing N -> M nodes (100000 keys; ring: 100 vnodes; fixed: P=1024) --
  N->M     min%    modN%   fixed%    ring%    jump%
  2->3      33.3     66.6     33.5     32.5     33.2
  3->4      25.0     74.9     25.0     23.1     25.0
  5->6      16.7     83.2     16.6     17.3     16.5
  8->9      11.1     88.9     11.0     11.5     11.1
  9->10     10.0     89.8     10.0     12.3     10.0
 10->11      9.1     91.1      9.1      9.1      9.1
 10->12     16.7     83.2     16.7     16.3     16.6
10->12 keys moved between two OLD nodes: modN=66514 fixed=0 ring=0 jump=0
-- (2) Redis Cluster: CRC16/XMODEM, hash tags, slots --
CRC16("123456789") = 0x31C3   (Redis spec: 31C3)
key                    hashed        slot
somekey                somekey      11058
foo{hash_tag}          hash_tag      2515
bar{hash_tag}          hash_tag      2515
{user1000}.following   user1000      3443
{user1000}.followers   user1000      3443
foo{}{bar}             foo{}{bar}    8363
foo{{bar}}zap          {bar          4015
foo{bar}{zap}          bar           5061
3 masters: M0:0-5461 M1:5462-10922 M2:10923-16383
4 masters: M0:0-4095 M3:4096-5461 M1:5462-9557 M3:9558-10922 M2:10923-15018 M3:15019-16383
slots moved: 4096 (25.00% of 16384; all to M3: true); slot counts after: [4096, 4096, 4096, 4096]; keys moved: 25.1% (min 25.0%)
```

```text
-- (3) load balance, N=10 nodes, 100000 keys, mean over 20 rings (max/mean: 1.00 is perfect) --
scheme          max/mean     worst  std/mean%
ring v=1           2.756     4.352      86.22
ring v=10          1.453     1.723      26.84
ring v=100         1.161     1.228       8.99
ring v=1000        1.049     1.096       2.77
fixed P=1024       1.016         -       0.94
jump               1.007         -       0.50
-- (4) Zipf(s=1) over 10000 keys, 200000 requests; fixed partitions; hot key has P=10.2% --
 nodes hottest node  max/mean   (no mitigation, 50% writes)
     5        24.7%      1.23
    10        17.6%      1.76
    20        13.9%      2.78
    40        12.1%      4.86
N=40, protect the top H keys with S=16 variants/copies; cell = max/mean | node requests per client request
  H      90%w suffix      10%w copies     90%w copies(!)   10%w suffix(!)
  0     4.86 | 1.00x     4.86 | 1.00x       4.86 | 1.00x     4.86 | 1.00x
  1     2.80 | 1.15x     2.79 | 1.16x       3.33 | 2.37x     3.33 | 2.38x
  4     2.00 | 1.32x     2.01 | 1.33x       3.32 | 3.87x     3.31 | 3.86x
 10     1.97 | 1.45x     1.98 | 1.45x       2.80 | 5.05x     2.81 | 5.05x
H=10, 90%w, suffix, more variants -> S=16: 1.97 | 1.45x   S=64: 1.41 | 2.88x   S=256: 1.35 | 8.61x
-- (5) 100000 writes with increasing keys, 16 partitions --
static ranges: busiest partition takes 100.0% of writes; hash: 6.3% (even would be 6.2%)
auto-split ranges (cap 1000): 199 partitions at the end; in each window of 100 consecutive writes the busiest partition takes 99.8% (hash: 11.0%)
scan of 100 consecutive keys: range touches 1 partition(s), hash touches 16 of 16
```

Giải thích:

1. **`mod N` dời gần hết khóa (mục 1 của output).** Cột `modN%` là `N/(N+1)` (66,6% cho 2→3; 91,1% cho 10→11), còn mức tối thiểu là `1/(N+1)` cột `min%`; ví dụ 10 → 12 của paper Jump Consistent Hash — chỉ cần dời 1/6 dữ liệu[^c-jump-1-6] — cho 83,2% với `modN` so với 16,7%. Cassandra mô tả đúng hiện tượng: với sơ đồ ngây thơ thêm một node có thể làm hỏng gần như mọi ánh xạ[^c-cass-naive]. Dòng cuối cho thấy cái phí: 66.514 khóa chuyển giữa **hai node đã có từ trước**, trong khi `fixed`, `ring`, `jump` đều bằng 0 — tức là mọi khóa chỉ chuyển sang node mới, đúng tính chất *monotonicity* của Karger và cộng sự[^c-kar-mono]; tổng số khóa dời sát mức tối thiểu của tính chất *smoothness*[^c-kar-smooth]. Cột `ring%` dao động quanh mức tối thiểu (12,3% cho 9→10 thay vì 10,0%) vì phần vòng mà node mới chiếm là ngẫu nhiên với 100 vnode; Dynamo nêu đúng nhược điểm này của vị trí ngẫu nhiên[^c-dyn-challenges], phần 3 đo nó. `fixed%` sát mức tối thiểu vì chỉ dời nguyên partition và ranh giới khóa → partition không đổi, đúng tinh thần "chuyển Q/S token"[^c-dyn-s3-def] và Redis "thêm node = chuyển hash slot"[^c-redis-reconf].
2. **Redis slot (mục 2).** `CRC16("123456789") = 0x31C3` trùng test vector của đặc tả[^c-redis-crc-vec]. Ba slot đầu bảng (`somekey` → 11058, `foo{hash_tag}` và `bar{hash_tag}` → 2515) trùng đúng ví dụ `CLUSTER KEYSLOT` trong docs Redis[^c-keyslot-1][^c-keyslot-2] — test của lab khẳng định cả ba. Các slot còn lại (3443, 8363, 4015, 5061) là output của code lab, **chưa đối chiếu với server Redis thật** (xem mục chưa chạy). Cột `hashed` là phần được băm: `{user1000}.following` và `{user1000}.followers` cùng slot vì chỉ `user1000` được băm[^c-redis-tag-ex]; `foo{}{bar}` băm cả key vì `{` đứng ngay trước `}`[^c-redis-tag-empty]; `foo{{bar}}zap` băm `{bar`[^c-redis-tag-nested]; `foo{bar}{zap}` băm `bar`. Bảng slot: lab chia 16384 slot thành `5462/5461/5461` slot liền nhau cho 3 master (cách chia của lab; docs Redis chỉ đưa một ví dụ minh họa khác là 0–5500, 5501–11000, 11001–16383[^c-redis-tut-ranges]). Master thứ tư lấy 4096 = 16384/4 slot — đúng phần công bằng tối thiểu, từ cuối dải của mỗi master cũ — nên 25,0% slot đổi chủ, tất cả sang master mới; mẫu khóa của bảng này được băm bằng `hash64` của lab (không qua CRC16) rồi lấy `% 16384` nên 25,1% khóa mẫu đổi chủ chỉ minh họa phép chia slot, không phải phép tính `HASH_SLOT` của Redis. Điều này khớp với việc Redis coi thêm/bớt node là cùng một thao tác: chuyển hash slot[^c-redis-reconf], và chuyển slot là chuyển mọi khóa băm vào slot đó[^c-redis-slot-keys]. Docs còn nhấn mạnh Redis Cluster không dùng consistent hashing mà dùng hash slot[^c-redis-no-ch] — cùng nhóm "số partition cố định" với `FixedPartitions` ([chương 06 mục 5.1](../12-he-thong-du-lieu/06-partitioning.md)), không phải vòng băm.
3. **Vnode (mục 3).** Với 10 node và 100.000 khóa đồng đều, `max/mean` của vòng giảm khi tăng số vnode: 2,76 → 1,45 → 1,16 → 1,05 cho `v` = 1, 10, 100, 1000 (trung bình 20 vòng; cột `worst` là vòng xấu nhất). Độ lệch chuẩn tải chia trung bình (`std/mean%`) giảm gần như theo `1/√v` (86 → 27 → 9,0 → 2,8%; quan sát và suy luận của người soạn từ bốn số này). Paper Jump kể rằng thí nghiệm của Karger và cộng sự dùng 1000 điểm mỗi bucket để đạt độ lệch chuẩn 3,2% số khóa giữa các bucket[^c-kar-1000]; con số 2,8% của lab cùng bậc độ lớn nhưng **hai thí nghiệm khác nhau**, đừng xem là một lần tái hiện. Dynamo đo hiệu quả cân tải bằng tỉ lệ tải trung bình trên tải của node nóng nhất[^c-dyn-efficiency], tức nghịch đảo của cột `max/mean`. `fixed P=1024` và `jump` cân gần hoàn hảo (1,016 và 1,007) với khóa **đồng đều** — mục sau cho thấy khóa không đồng đều thì chuyện khác. Giới hạn của Jump: bucket phải đánh số tuần tự[^c-jump-seq], nên (suy luận của người soạn) không bỏ được node ở giữa dải.
4. **Hot key (mục 4).** Khóa hạng 1 có xác suất 10,2% (`1/H₁₀₀₀₀`). Node chứa nó nhận **ít nhất** phần đó, bất kể N: 24,7% → 17,6% → 13,9% → 12,1% khi N = 5 → 40; vì trung bình là `1/N` nên `max/mean` **tăng** 1,23 → 4,86 — thêm node không chữa được. Dynamo giả định có đủ khóa ở đầu "nổi tiếng" để tải nhiều khóa trải đều[^c-dyn-skew]; một khóa chiếm 10% là trường hợp giả định đó không còn đúng. Bảng giảm nhẹ (N = 40, `S` = 16, hàng `H` = số khóa nóng nhất được bảo vệ): bảo vệ riêng khóa hạng 1 chỉ kéo `max/mean` từ 4,86 xuống 2,80 vì khóa hạng 2, 3, … vẫn nóng và điểm nóng chuyển sang chúng; `H` = 4 và 10 cho 2,00 và 1,97. Phần dư không phải do hạng thấp: dòng cuối bảng cho thấy tăng số biến thể lên `S` = 64 và 256 hạ tiếp còn 1,41 và 1,35 — gợi ý (suy luận của người soạn) phần dư là do 160 biến thể được băm ngẫu nhiên lên 40 node, giống ném bóng vào hộp. Cái giá nằm ở ô sau dấu `|`: mỗi lần đọc khóa đã tách phải gọi cả `S` biến thể — DynamoDB nói rõ muốn đọc tất cả mục của một ngày phải truy vấn mọi hậu tố rồi gộp[^c-ddb-suffix-read] — nên hai cột `(!)` (dùng sai công cụ: hậu tố cho mix 10% ghi, bản sao cho mix 90% ghi) cho 3,87× số request ở `H` = 4 và 5,05× ở `H` = 10, còn hai cột đúng chỉ 1,32× và 1,45×. Lab không mô phỏng cache trong process, replica đọc (Redis nói replica dùng được để mở rộng đọc khi chấp nhận dữ liệu cũ[^c-cl-replicas-read]) hay tự động tách partition nóng — ba thứ nên thử trước khi tới bước nhân khóa (nhận xét của người soạn).
5. **Range vs hash (mục 5).** Ghi 100.000 khóa tăng dần (như timestamp): với 16 khoảng chia đều cố định, một partition nhận 100,0% — vì 16 khoảng chia không gian khóa 2^32 nên mỗi khoảng rộng 2^28 và 100.000 khóa tăng dần nằm gọn trong khoảng đầu tiên (con số 100,0% là hệ quả của tham số không gian khóa của lab, không phải số đo); cùng hiện tượng mà Bigtable cảnh báo: row key bắt đầu bằng timestamp đẩy ghi tuần tự lên một node[^c-bt-ts-hot]; hash chỉ 6,3% (đều sẽ là 6,2%). Tách tự động không cứu: sau 199 partition, trong mỗi cửa sổ 100 ghi liên tiếp partition bận nhất vẫn nhận 99,8% vì điểm ghi luôn nằm ở mép phải — đúng cơ chế MongoDB mô tả: chunk có cận trên `MaxKey` nhận phần lớn insert với khóa tăng đơn điệu[^c-mongo-mono-hot] (mô hình tách của lab là đơn giản hóa; chính sách tách của MongoDB/Bigtable khác). Hash trả giá ở scan: quét 100 khóa liền nhau chạm 1 partition với range nhưng cả 16 với hash — MongoDB nêu cùng đánh đổi: hashed sharding đều hơn nhưng giảm truy vấn targeted[^c-mongo-hash], còn Bigtable nói đọc dải ngắn chỉ cần liên lạc ít máy[^c-bt-short]. Hai cách chữa được nguồn nêu: hash khóa đơn điệu[^c-mongo-hash-mono] hoặc đặt giá trị phân biệt cao (như user ID) trước timestamp[^c-bt-ts-prefix].
6. **Test (`cargo test -p lab-distributed partition::`).** 22 test. Cố định: `CRC16("123456789") == 0x31C3`; `somekey`/`foo{hash_tag}`/`bar{hash_tag}` ra 11058/2515/2515; mọi ví dụ hash tag của đặc tả; mọi slot `< 16384`; vector `hash64`/partition được ghim (bản thân việc hash64 đổi giá trị sẽ làm test đỏ). Quét: với 100.000 khóa, `ModN` dời `N/(N+1)` ± 0,02 cho N = 2…10 và ≥ 0,8 cho 10→12; `FixedPartitions`, `Jump`, `Ring` dời `1/(N+1)` trong dung sai nêu trong test (Ring: 5σ với σ ≈ phần × `1/√v`) và số khóa chuyển giữa hai node cũ **bằng 0**, kể cả Ring với 30 `salt` khác nhau; `FixedPartitions` không bao giờ đổi partition của khóa và giữ số partition mỗi node chênh ≤ 1; `max/mean` giảm đơn điệu theo vnode; hot key: với 5 seed, `max_share ≥` xác suất khóa nóng và `max/mean` tăng theo N; hậu tố/bản sao hạ `max/mean` xuống dưới 0,6 lần với mix phù hợp, và chi phí fan-out đúng bằng `S` mỗi lần đọc khóa nóng; range tĩnh dồn ≥ 90% ghi vào một partition còn hash < 7%. Một seed trong output không chứng minh gì; các test là chỗ kiểm bất biến.

### Lab (d) — `fencing`: lease, process pause và fencing token

Lý thuyết: [chương 08 mục 5](../12-he-thong-du-lieu/08-su-co-he-phan-tan.md) (đồng hồ monotonic và time-of-day), [mục 7](../12-he-thong-du-lieu/08-su-co-he-phan-tan.md) (process pause, "kiểm tra rồi mới làm" vẫn sai), [mục 8](../12-he-thong-du-lieu/08-su-co-he-phan-tan.md) (lease, fencing token 33/34, điều kiện của fencing), [chương 09 mục 8](../12-he-thong-du-lieu/09-consistency-va-consensus.md) (etcd/ZooKeeper: token là revision/zxid), [chương 16 mục 8.1–8.3](../12-he-thong-du-lieu/16-redis-ben-trong.md) và [chương 17 mục 3.2](../12-he-thong-du-lieu/17-redis-patterns.md) (khóa Redis, counter token nằm trên node được sao chép).

Mô hình của lab (thời gian ảo, đơn vị ms; mọi thứ là hàm của seed):

- `LockService`: cấp tối đa một lease tại một thời điểm, `TTL_MS = 10_000`, mỗi lần cấp tăng bộ đếm token (bắt đầu từ 33 để khớp hình ở chương 08), hết hạn theo đồng hồ *của chính nó*. Bộ đếm có một bản sao chép bất đồng bộ; `failover` có thể làm bộ đếm lùi về bản cũ (kịch bản 3).
- `Storage`: một counter có version, ba chế độ: `Unfenced` (nhận mọi ghi); `Fenced` (từ chối ghi có token nhỏ hơn token cao nhất đã nhận); `FencedCas` (kiểm token *và* ghi phải mang đúng version đã đọc, giống `UPDATE … WHERE version = $v`, xem [chương 07 mục 5](../12-he-thong-du-lieu/07-transaction.md)).
- Client: `acquire → read → compute → check lease cục bộ → write (value + 1)`, rồi release. Deadline cục bộ tính từ lúc **gửi** yêu cầu acquire. Một pause (GC, `SIGSTOP`, bị throttle CPU…) được chèn vào: ở các kịch bản dựng tay, trước bất kỳ bước nào — kể cả giữa `check` và `write`; ở sweep ngẫu nhiên, trước `read`/`compute`/`check`/`write` với xác suất `p` (không pause trước `acquire`).
- "Overlap" = khoảng thời gian "tôi đang giữ khóa" của hai client (từ lúc được cấp tới lúc ghi hoặc bỏ cuộc) chồng nhau. "Lost update" = số ghi được storage nhận trừ số gia tăng thực sự có trong counter cuối.

Mô hình **không** có: mất/đảo thứ tự message, trễ một chiều (mỗi RPC là một hằng số 5 ms, hiệu lực áp dụng ngay lúc gửi), gia hạn lease, nhiều khóa/tài nguyên, partition. Các kịch bản 1–4 là phản ví dụ dựng tay; bảng sweep dùng tham số tùy chọn của người soạn (xác suất pause, độ dài work) nên **không** phải tần suất của hệ thật.

```bash
cd labs && cargo run -q -p lab-distributed --bin fencing    # exit 0
# tùy chọn: ... --bin fencing -- <seed> <số run mỗi mode>   (mặc định 42 2000)
```

Output (macOS aarch64, rustc 1.95.0; xác định theo seed, chạy hai lần `diff` giống hệt; logic mô phỏng dùng số nguyên, chỉ quyết định "có pause hay không" dùng `Rng::chance` trên `f64` — kết quả trên nền tảng khác chưa kiểm):

```text
== lab d: lease + pause + fencing token (seed 42, ttl 10000 ms, virtual time) ==

-- 0. check-then-act: same 15 s pause, Unfenced --
  pause before check             aborted_by_check=1 stale_accepted=0 lost=0
  pause between check and write  aborted_by_check=0 stale_accepted=1 lost=1

-- 1. A (token 33) pauses 15 s between check and write; B takes over --
[Unfenced]
  t=     0 A acquire -> token 33, lease until 10000
  t=     5 A read doc -> value 100 (v1)
  t=    10 A compute 2000 ms
  t=  2010 A check lease: ok (local deadline 10000)
  t=  2010 A PAUSE 15000 ms before write
  t=  9000 B acquire denied (lease held until 10000)
  t= 10500 B acquire -> token 34, lease until 20500
  t= 10505 B read doc -> value 100 (v1)
  t= 10510 B compute 3000 ms
  t= 13510 B check lease: ok (local deadline 20500)
  t= 13510 B write(token 34, read v1) -> ACCEPTED, value 101 (v2)
  t= 17010 A write(token 33, read v1) -> ACCEPTED, value 101 (v3)  [writer's lease already expired/superseded]
  => final value 101 (accepted 2, lost 1), holder intervals overlap: true
[Fenced] same events; only the last line differs:
  t= 17010 A write(token 33, read v1) -> REJECTED: token 33 < highest seen 34  [writer's lease already expired/superseded]
  => final value 101 (accepted 1, lost 0), holder intervals overlap: true

-- 2. residual window: A's late write (33) lands before B's write (34) --
  Fenced    accepted [A:t33 B:t34] rejected [] final 101 lost 1
  FencedCas accepted [A:t33] rejected [B:t34 (read v1, now v2)] final 101 lost 0

-- 3. token source: lock service fails over at t=5000, counter rolled back? --
  counter rolled back grants [A=33 B=33] accepted [B:t33 A:t33] rejected [] lost 1
  counter survives  grants [A=33 B=34] accepted [B:t34] rejected [A:t33] lost 0

-- 4. clock skew, no pause: client wall clock 8 s behind, 12 s after the request --
  truly valid: false   wall-clock check says valid: true   monotonic-since-send says valid: false
  A's write with token 33 after B (token 34) wrote: Unfenced accepts: true, Fenced accepts: false

-- sweep: 2000 runs per mode, 2 clients x 5 rounds, pause p=0.03 per step up to 30000 ms --
mode       overlap stale_acc  sup_acc stale_rej valid_rej lost_runs lost_upd tok_back check_abort
Unfenced       742       444      249         0         0       221      508      199        1325
Fenced         742       233       38       211         0        23       23        0        1325
FencedCas      742       233       38       211        23         0        0        0        1325
no pauses (p=0), all 3 modes: overlap 0, stale writes 0, lost updates 0, check aborts 0
```

Giải thích:

1. **"Kiểm tra rồi mới làm" (kịch bản 0).** Cùng một pause 15 s: đặt *trước* `check` thì client thấy lease hết hạn và bỏ round (`aborted_by_check=1`); đặt *giữa* `check` và `write` thì check đã qua ở t=2010 nhưng ghi xảy ra ở t=17010 (`stale_accepted=1`, `lost=1`). Đúng như Kleppmann: không sửa được bằng cách chèn một bước kiểm tra hạn ngay trước khi ghi[^d-kb-check]. Pause xảy ra mà process không hề biết[^d-pause-nonotice]; nguyên nhân gồm GC dừng mọi thread[^d-pause-gc] hay ai đó gửi `SIGSTOP`[^d-kb-sigstop].
2. **Hình 33/34 của chương 08 (kịch bản 1).** A nhận token 33 (lease tới 10000), check OK ở t=2010 rồi bị pause; B bị từ chối ở t=9000 vì lease còn sống, được cấp token 34 ở t=10500 và ghi ở t=13510. Khi A tỉnh (t=17010): `Unfenced` nhận ghi của A, dựa trên lần đọc v1, nên đè lên ghi của B — hai ghi được nhận nhưng counter chỉ tăng 1 (`lost 1`). `Fenced` từ chối vì 33 < 34, đúng cơ chế trong bài của Kleppmann[^d-kb-fencing]. Chú ý dòng cuối mỗi chế độ: `holder intervals overlap: true` ở **cả hai**. Fencing không ngăn hai bên cùng tin mình giữ khóa, nó làm sự chồng chéo đó vô hại — và chỉ hoạt động khi storage chủ động kiểm token[^d-kb-fencing-active]. Lease một mình không đủ khi pause dài hơn hạn[^d-kb-pause-lease]; etcd cũng tự cảnh báo rằng server có thể đã thu hồi lease mà client vẫn tưởng mình còn giữ[^d-etcd-lease-clock] và lease không tự bảo đảm mutual exclusion[^d-etcd-lease-nomx].
3. **Cửa sổ còn sót của fencing chỉ-bằng-token (kịch bản 2).** B (34) đã đọc nhưng chưa ghi; ghi muộn của A (33) tới *trước*: `Fenced` nhận nó (storage chưa thấy token 34), rồi ghi của B — dựa trên lần đọc cũ — cũng được nhận và đè lên (`lost 1`). `FencedCas` từ chối ghi của B (`read v1, now v2`): không mất update (`lost 0`) nhưng đó là một ghi *hợp lệ* của người đang giữ lease bị từ chối (`valid_rejected` = 1 trong test), B phải đọc lại và làm lại round — mô hình của lab không retry, nên counter cuối vẫn là 101 như ở `Fenced`. Suy luận của người soạn từ kết quả này: kiểm token chỉ chặn các ghi tới *sau* một ghi có token cao hơn; CAS theo version đóng nốt phần còn lại — ý "để chính DB làm fencing" ở [chương 17 mục 3.2](../12-he-thong-du-lieu/17-redis-patterns.md). Lưu ý: ghi của A ở `FencedCas` vẫn được nhận (nó là một CAS hợp lệ); với counter thì vô hại, còn với tác dụng phụ bên ngoài storage thì chưa chắc (nhận xét của người soạn).
4. **Nguồn token (kịch bản 3, storage `Fenced`).** Lock service failover lúc t=5000 và mất bảng lease; B xin lúc t=6000. Nếu bộ đếm lùi về bản sao cũ (`rolled back`), B nhận **cùng token 33** và storage nhận cả hai ghi (`33 < 33` là sai) — `lost 1`. Nếu bộ đếm sống sót, B nhận 34 và ghi của A bị chặn, dù overlap vẫn xảy ra. Đây là điều kiện "token phải tăng đơn điệu nghiêm ngặt"[^d-kb-fencing-monotonic]. Mô hình giả định bộ đếm kiểu Redis được sao chép bất đồng bộ (Redis dùng replication bất đồng bộ[^d-repl-async], và tài liệu Redis tự mô tả kịch bản failover làm hai client cùng giữ khóa[^d-lk-race-steps]); ta *không* đo Redis thật. Kleppmann nói giữ counter trên một node Redis là chưa đủ vì node đó có thể hỏng[^d-kb-counter] và có lẽ cần cả một thuật toán consensus chỉ để sinh token[^d-kb-fencing-consensus]; với ZooKeeper có thể dùng zxid hoặc version của znode[^d-kb-zk], với etcd là revision[^d-etcd-fence]. Storage không thể bù: nó chỉ so sánh số mình nhận, và số trùng là không phân biệt được (nhận xét của người soạn).
5. **Lệch đồng hồ, không có pause (kịch bản 4).** Đồng hồ client chậm 8 s so với lock service; 12 s sau khi gửi request, lease thật đã hết nhưng kiểm tra `client_wall_now < expiry_timestamp` vẫn trả "còn hạn"; kiểm tra "monotonic đo từ lúc gửi + TTL" trả đúng. Đồng hồ time-of-day có thể nhảy nên không dùng để đo thời gian trôi qua[^d-tod-jump]; monotonic không bị nhảy do chỉnh giờ[^d-cg-mono-nojump] nhưng chỉ *hiệu* hai giá trị trên cùng một node mới có nghĩa[^d-mono-local]. Chubby cũng để client giữ một local lease timeout bảo thủ[^d-ch-lease-local] và đòi đồng hồ server không chạy nhanh hơn client quá một hệ số đã biết[^d-ch-clockrate]. Gói `leaderelection` của client-go chỉ tin thời gian quan sát cục bộ (chịu skew tùy ý, không chịu skew *rate* tùy ý) và nói thẳng rằng nó không cung cấp fencing[^d-k8s-local][^d-k8s-nofence]. Redis dùng `gettimeofday` thay vì monotonic để tính hạn key theo bài của Kleppmann[^d-kb-redis-clock], và tài liệu Redis thừa nhận wall-clock shift có thể làm khóa bị nhiều process giữ[^d-lk-disclaimer-clock]. Bài học: kiểm tra theo thời gian giảm xác suất sai, nhưng chỉ fencing làm *safety* không phụ thuộc thời gian — thời gian chỉ còn cho liveness[^d-kb-safety-liveness]. Đồng hồ client *nhanh* (test `scenario4_...`) chỉ gây mất liveness (bỏ lease sớm), không gây sai.
6. **Bảng sweep (2000 run mỗi mode, cùng seed ⇒ cùng lịch trình).** `overlap` giống hệt ở cả ba mode (742 run) vì storage không ảnh hưởng lịch trình; `check_abort` (1325 round bị chính client chặn) cũng giống nhau — check cục bộ chỉ bắt được pause xảy ra *trước* nó. `stale_acc` = ghi được nhận dù lease của người ghi đã hết/bị thay; `sup_acc` = trong số đó, người khác đã được cấp lease. `Unfenced`: 444 ghi cũ được nhận, 221 run mất 508 update, và ở 199 run storage nhận token *lùi* (token 33 được nhận sau token 34). `Fenced`: 211 ghi cũ bị chặn, nhưng còn 38 ghi `sup_acc` lọt qua (storage chưa thấy token mới) và 23 run vẫn mất update — cửa sổ của kịch bản 2 xuất hiện trong sweep ngẫu nhiên. `FencedCas`: 0 mất update, **nhưng** có 23 ghi `valid_rej` bị từ chối của người *còn* giữ lease hợp lệ — đúng bằng 23 run mà `Fenced` mất update; CAS đổi "mất dữ liệu im lặng" thành "từ chối, client phải đọc lại và thử lại" (mô hình không retry nên chi phí này không được tính vào counter cuối). `stale_rej` = ghi bị từ chối dù writer đã hết/bị thay lease; `valid_rej` = ghi bị từ chối dù writer còn lease hợp lệ. Phần còn lại của `stale_acc` ở `Fenced` (233 − 38 = 195) chỉ là ghi sau khi lease hết mà *chưa ai* thay nên không có gì để đè. Dòng `no pauses` (tổng của ba mode, gồm cả `valid_rej`): 0 ở mọi cột — không có pause và không skew thì lease + khóa là đủ, mọi vi phạm đều do pause. Một seed không chứng minh gì; test quét 2000 seed kiểm các bất biến bên dưới.

Test (`cargo test -p lab-distributed fencing::`, 12 test trong module `fencing`): kịch bản 1 (unfenced: ghi cũ của A được nhận, overlap, mất 1 update; fenced: A bị từ chối đúng lý do `StaleToken { token: 33, highest: 34 }`); kịch bản 2 (token-only mất update, CAS thì không); kịch bản 3 (token trùng 33/33 bị nhận cả hai; bộ đếm sống sót thì từ chối); kịch bản 4 (wall-clock sai, monotonic đúng, và không skew thì cả hai đúng ở các mốc 0, 5000, 9999, 10000, 12000); biên của `LockService` (hết hạn là `expires <= now`, release muộn không giải phóng lease người khác) và của `Storage` (token bằng nhau được nhận); trên **2000 seed**: ở `Fenced`/`FencedCas` token được nhận không bao giờ giảm, `FencedCas` không bao giờ mất update (định nghĩa của `lost_updates` khiến điều này gần như hiển nhiên, nên test còn kiểm phần chi phí: với mọi seed mà `Fenced` mất update thì `FencedCas` từ chối nhiều ghi hơn, và tổng `valid_rejected` của `FencedCas` > 0 còn của `Fenced` = 0), `Unfenced` mất, `Fenced` mất ít hơn nhưng không bằng 0, mọi run `Fenced` có mất update đều có ghi `sup_acc`; `accepted + rejected` của `Fenced` bằng số ghi của `Unfenced` ở từng seed; mọi mode cùng lịch trình theo seed (200 seed, so cả ba mode); sweep không pause: 0 vi phạm ở cả ba mode (500 seed) và đúng 10 increment, token 33..42.

### Lab (e) — `lamport`: Lamport clock, vector clock, LWW và total order broadcast

Lý thuyết: [chương 09 mục 3](../12-he-thong-du-lieu/09-consistency-va-consensus.md) (happens-before, Lamport clock, thứ tự toàn phần `(t, node)`, vector clock, bảng A/B/C), [mục 4](../12-he-thong-du-lieu/09-consistency-va-consensus.md) (total order broadcast, state machine replication), [chương 08 mục 6](../12-he-thong-du-lieu/08-su-co-he-phan-tan.md) (timestamp vật lý, LWW, hình đồng hồ nhanh 30 ms), [chương 05 mục 8](../12-he-thong-du-lieu/05-replication.md) (vector clock và siblings kiểu Dynamo).

Mô hình của lab (module `clocks`, mọi thứ là hàm của seed, thời gian ảo tính bằng ms nguyên):

- **Execution ngẫu nhiên** với N = 3, 4, 5 process và 60 sự kiện: mỗi bước chọn một process rồi một sự kiện cục bộ, một lần gửi tới process khác, hoặc một lần nhận một tin *bất kỳ* đang bay tới nó (nên các tin bị đảo thứ tự). **Ground truth** của happens-before được dựng chỉ từ thứ tự trong từng process và cạnh gửi→nhận, rồi lấy bao đóng bắc cầu; đồng hồ không được dùng để dựng nó, nên đồng hồ có thể bị kiểm tra đối chiếu. Hai đồng hồ cài đúng luật của chương: Lamport `t := max(t, t') + 1` khi nhận[^e-lamport-alg] và vector clock lấy max từng phần tử rồi tăng ô của mình[^e-vc-alg].
- **Hai register**: LWW (giữ stamp lớn nhất) và multi-value register (giữ mọi bản chưa bị ghi đè về nhân quả, gọi là siblings[^e-siblings]).
- **Total order broadcast bằng Lamport timestamp**: 3 replica, mỗi replica phát 3 phép toán không giao hoán lên một chuỗi (`=a` là *set*, `+4` là *append*), trễ 1..=6 ms, kênh FIFO. Replica chỉ áp phép toán có `(t, node)` nhỏ nhất khi đã nghe từ *mọi* node khác một tin có `t' >= t`; mỗi replica trả lời mỗi phép toán nhận được bằng một tin ack mang timestamp mới.

Mô hình **không** có: mất hoặc nhân đôi tin, node hồi phục sau crash, đo thời gian thật. Số 30 ms của hình LWW là giá trị người soạn chọn theo hình ở chương 08, không phải số đo; bài giảng chỉ nói NTP thường chỉ còn lệch vài ms trong điều kiện tốt và có thể tệ hơn nhiều[^e-ntp-good]. Giả định kênh FIFO là giả định chính Lamport nêu cho thuật toán loại trừ tương hỗ của ông[^e-fifo]; lab giữ nó cho TOB. Điều kiện `t' >= t` (thay vì "timestamp muộn hơn" nghiêm ngặt như trong thuật toán của Lamport[^e-grant]) là suy luận của người soạn: với kênh FIFO và đồng hồ mỗi node tăng ngặt, mọi tin còn lại của node đó đều có `t` lớn hơn.

```bash
cd labs && cargo run -q -p lab-distributed --bin lamport -- 42    # exit 0
```

Output (macOS aarch64, rustc 1.95.0; xác định theo seed, không dùng số thực trừ một phần trăm in ra, nên chữ số thập phân cuối có thể khác trên nền tảng khác — chưa kiểm):

```text
== Lab (e) lamport — seed 42 ==

-- 1. chapter 09 section 3: A/B/C worked example --
event        Lamport  vector<A,B,C>
A: a1              1  <1,0,0>
A: send m1         2  <2,0,0>
B: b1              1  <0,1,0>
B: recv m1         3  <2,2,0>
B: send m2         4  <2,3,0>
C: c1              1  <0,0,1>
C: recv m2         5  <2,3,2>
equals the table in chapter 09: true
b1 L=1 < send m1 L=2 but concurrent: <0,1,0> vs <2,0,0> are incomparable

-- 2. 200 random executions (seeds 42..241), 60 events each --
 N execs    pairs     a->b concurrent clock-vio  converse   vc-bad
 3    67   118590    71842      46748         0     43614        0
 4    67   118590    53406      65184         0     60648        0
 5    66   116820    38836      77984         0     71981        0
 *   200   354000   164084     189916         0    176243        0
converse = concurrent pairs with L(a) != L(b): 92.8% of concurrent pairs look ordered to Lamport
(t,node) order: 0 violations (contradicts a->b, or duplicate stamp)

-- 3. LWW: client A clock +30 ms, B correct --
physical ts: A set x=1 @40 ; B read x=1, set x=2 @20  => LWW keeps x=1
Lamport:     A set x=1 @(1,A) ; B read x=1, set x=2 @(3,B)  => LWW keeps x=2
vector clk:  same causal pair                          => siblings [2]
concurrent:  A x=1 @(1,A), B x=2 @(1,B) (no read in between)
  LWW on (t,node)  => keeps x=2 (either arrival order), x=1 lost
  vector <1,0> vs <0,1> => siblings [1, 2]; after B reads both and writes x=3: [3]

-- 4. total order broadcast: 3 replicas x 3 ops (=set, +append), delay 1..=6 ms, seed 42 --
apply on arrival:
  A state="b42"      applied=9
  B state="c44612"   applied=9
  C state="b6214"    applied=9
apply in (t,node) order after hearing t' >= t from every other node:
  A state="b142"     applied=9
  B state="b142"     applied=9
  C state="b142"     applied=9
  delivery order on every replica: (1,A)=a (1,C)=c (2,A)+6 (4,B)+4 (4,C)+4 (5,B)=b (6,A)+1 (14,B)+4 (15,C)+2
node C crashes at 6 ms (7 ops issued in total):
  A applied=3 stuck-pending=4 state="c6"
  B applied=3 stuck-pending=4 state="c6"
over 200 seeds: arrival order diverged 188, (t,node) order agreed 200, stalled with C crashed 200
```

Giải thích:

1. **Bảng A/B/C** khớp từng ô với bảng ở [chương 09 mục 3](../12-he-thong-du-lieu/09-consistency-va-consensus.md); bin `assert` và test `worked_example_matches_chapter_table` so với hằng `WORKED_EXPECTED`. `b1` có `L = 1` nhỏ hơn "A gửi m1" có `L = 2`, nhưng hai sự kiện đồng thời: vector `<0,1,0>` và `<2,0,0>` không so sánh được. Bài giảng nói rõ chỉ từ hai Lamport timestamp thì nói chung không phân biệt được "xảy ra trước" với "đồng thời"[^e-lamport-nodist].
2. **Quét 200 execution** (seed 42..241, 118 590 + 118 590 + 116 820 = 354 000 cặp sự kiện). Cột `clock-vio` = 0: không cặp a → b nào có `L(a) >= L(b)`, tức Clock Condition đúng[^e-clockcond]. Cột `converse` = 176 243: số cặp *đồng thời* mà `L` vẫn khác nhau, tức Lamport clock trông như xếp thứ tự cho 92,8% cặp đồng thời. Đó là lý do chiều ngược lại không đúng[^e-converse]. Cột `vc-bad` = 0: so sánh vector khớp ground truth ở cả 354 000 cặp, đúng khẳng định thứ tự trên vector tương ứng chính xác với happens-before[^e-vc-hb] (quy tắc so sánh: mọi ô nhỏ hơn hoặc bằng thì là tổ tiên, ngược lại là xung đột[^e-vc-rule]). Thứ tự `(t, node)` không có vi phạm nào: không cặp a → b nào bị đảo và không hai sự kiện nào trùng stamp[^e-unique]. Đây là thứ tự toàn phần nhất quán với nhân quả[^e-total] nhưng với hai sự kiện đồng thời nó chỉ chọn tùy ý theo id node.
3. **LWW.** Với timestamp vật lý, replica thấy `(40, x=1)` rồi `(20, x=2)` và giữ `x=1`: ghi `x=2` của B xảy ra *sau khi B đọc* `x=1` nhưng bị bỏ, vì đồng hồ của A nhanh 30 ms. Đây đúng là nguy cơ bài giảng nêu: sắp thứ tự theo timestamp time-of-day có thể ra thứ tự sai[^e-ts-wrong]. Dynamo (theo bài báo) có một chế độ cấu hình hòa giải theo timestamp ("timestamp based reconciliation"): chọn đối tượng có timestamp vật lý lớn nhất[^e-dyn-lww], chỉ là một trong các cách hòa giải mà bài báo mô tả; Cassandra dùng mô hình last write wins với timestamp trên mỗi mutation[^e-cass-lww] (lab không mô phỏng hai hệ này, chỉ minh họa nguyên tắc). Với stamp Lamport, B đã quan sát ghi của A nên `(3,B) > (1,A)` và giữ `x=2`. Với hai ghi *đồng thời* thật, LWW theo `(t,node)` giữ `x=2` (do id node B lớn hơn, tùy ý) và mất `x=1` mà không ai ghi đè nó về nhân quả: bài giảng mô tả LWW là chấp nhận mất dữ liệu khi cập nhật đồng thời[^e-lww-loss]. Multi-value register với vector clock giữ cả hai (`[1, 2]`); sau khi B đọc cả hai và ghi `x=3` kèm context đã gộp thì chỉ còn `[3]`, đúng cơ chế context của Dynamo[^e-d-context].
4. **TOB.** Áp theo thứ tự tới nơi: ba replica nhận cùng 9 phép toán nhưng kết thúc ở ba trạng thái khác nhau (`b42`, `c44612`, `b6214`) vì `set` và `append` không giao hoán — bài giảng đòi các replica nhận cùng thứ tự, cùng với cập nhật tất định mới hội tụ[^e-tob-def][^e-smr]. Áp theo `(t,node)` sau khi nghe đủ mọi node thì cả ba ra `b142` với cùng một dãy giao. Khi C crash ở 6 ms, A và B chỉ giao được 3 phép toán (trạng thái `c6` là tiền tố của dãy trên) và 4 phép toán nằm kẹt mãi. Đây là điểm yếu Lamport tự nêu: một process phải biết mọi lệnh của process khác, nên một process hỏng làm cả hệ dừng[^e-halt]; và không có thời gian vật lý thì không phân biệt được process hỏng với process đang tạm dừng[^e-fail]. Bài giảng cũng nói cách TOB dựa trên Lamport timestamp và cách dùng leader đều không chịu lỗi[^e-tob-naive]; sửa việc này là việc của consensus (Paxos, Raft). Dòng cuối: trên 200 seed arrival diverged 188, `(t,node)` agreed 200, stalled 200 — trong cấu hình này (crash lúc 6 ms, 9 phép toán rải trong 20 ms); nếu C chỉ crash sau khi mọi thứ đã xong (test `tob_stalls_when_one_node_is_silent` thử crash ở 10 000 ms) thì không có gì kẹt.
5. **Một seed không chứng minh gì.** Các bảng trên chỉ minh họa; bất biến được kiểm bằng test (`cargo test -p lab-distributed clocks::`, 17 test): Clock Condition, "vector ⇔ happens-before" và thứ tự `(t,node)` trên 300 execution (N = 3..5, 60 sự kiện); ground truth bằng Floyd–Warshall trên 20 seed; sắp theo stamp là linear extension trên 50 seed; phản ví dụ nhỏ cho chiều ngược; hai luật nhận *sai* (`MaxOnly`, `IgnoreMessage`) cho vi phạm Clock Condition trên 100 seed (để chứng tỏ checker bắt được lỗi thật); LWW vật lý giữ `x=1`, Lamport giữ `x=2`; TOB hội tụ trên 200 seed, kẹt khi crash trên 100 seed, tất định.
6. **Mô hình bỏ qua gì** (nhận xét của người soạn): TOB ở đây an toàn nhưng không sống khi một node im lặng; nó cũng giả định không mất tin và không phân vùng, nên không phải một giao thức dùng được ngoài đời — chỉ để thấy vì sao cần consensus. Vector clock ở đây có một ô cho mỗi node cố định, không phải map như thực tế[^e-vc-map].

### Lab (f) — `raft_election`: bầu leader, chép log và năm tính chất an toàn của Raft

Lý thuyết: [chương 09 mục 7](../12-he-thong-du-lieu/09-consistency-va-consensus.md) (Raft: term, `RequestVote`, `AppendEntries`, election timeout ngẫu nhiên, luật "log đủ mới", chỉ đếm bản sao cho entry của term hiện tại, năm tính chất, leader cũ vẫn tồn tại), [mục 6](../12-he-thong-du-lieu/09-consistency-va-consensus.md) (FLP và tính sống), [chương 08 mục 8](../12-he-thong-du-lieu/08-su-co-he-phan-tan.md) (term cùng họ ý tưởng với fencing token) và [P13 chương 08 mục 7–8](../13-tla-plus/08-dac-ta-giao-thuc-phan-tan.md) (mô phỏng so với model checking).

**Mô hình của lab** — đây là bản cài *để học*, không phải Raft production. Một tiến trình, thời gian ảo tính bằng ms, hàng đợi sự kiện sắp theo `(time, seq)`; mọi ngẫu nhiên lấy từ `Rng` có seed nên một lần chạy là hàm thuần của `(Config, seed)`.

- Mỗi node là một server của Hình 2: ba trạng thái leader/follower/candidate[^f-states], term đánh số liên tiếp và mỗi term bắt đầu bằng một cuộc bầu cử[^f-terms], hai RPC `RequestVote`/`AppendEntries`[^f-rpcs]. Một node chỉ bỏ phiếu cho một ứng viên mỗi term[^f-vote], và chỉ cho ứng viên có log không cũ hơn log của mình[^f-restr]. `AppendEntries` mang chỉ số và term của entry đứng trước; follower không thấy entry khớp thì từ chối[^f-consistency] và leader lùi `nextIndex`[^f-nextindex]. Entry của term hiện tại được commit khi nằm trên đa số; entry term cũ chỉ được commit theo nó, không bằng cách đếm bản sao[^f-prior].
- Election timeout rút ngẫu nhiên trong `[150, 300)` ms, đúng khoảng ví dụ của paper[^f-range]. Heartbeat 50 ms và latency một chiều 5–15 ms là lựa chọn của lab (paper chỉ đòi broadcast time nhỏ hơn election timeout cỡ một bậc độ lớn[^f-broadcast]).
- `currentTerm`, `votedFor`, `log` sống qua crash; vai trò, `commitIndex`, `nextIndex`… mất. Paper đòi cập nhật stable storage *trước khi trả lời RPC*[^f-persist]; mô hình cập nhật tức thì (không có giá `fsync`). Candidate gửi lại `RequestVote` mỗi heartbeat cho node chưa trả lời (paper: server gửi lại RPC khi không thấy hồi đáp[^f-retry]); leader mới ghi một entry no-op của term mình[^f-noop] (tắt trong kịch bản Figure 8).
- Mạng mất, trùng, đảo thứ tự gói và có partition; crash/restart theo kế hoạch sinh từ seed. Client gửi một lệnh mỗi 100 ms tới node nó tin là leader, được chỉ lại leader nếu sai, quên leader sau 400 ms không hồi đáp; không retry, không dedup.
- Năm kịch bản: `no faults`; `drops` (mất 10–20% gói, mỗi seed một tỉ lệ); `crash+restart` (cứ 250–700 ms crash một node trong 100–500 ms, một nửa số lần nhắm vào leader hiện tại, tối đa 2 node cùng chết); `partitions` (kéo dài 300–700 ms, cách nhau 300–700 ms; 60% cô lập leader, 20% một node ngẫu nhiên, 20% tách thiểu số khỏi đa số); `combined` (mất 10%, trùng 3% và cả hai loại lỗi trên). Lỗi dừng ở 3000 ms, rồi chữa hết; client dừng ở 3300 ms; chạy tới 4000 ms.
- Sau **mỗi** sự kiện, một checker toàn cục kiểm năm tính chất của Hình 3: Election Safety, Leader Append-Only, Log Matching, Leader Completeness, State Machine Safety[^f-es][^f-append-only][^f-logmatch][^f-lc][^f-sms] — paper nói chúng đúng "ở mọi thời điểm"[^f-fig3]. Cờ `Mutant` bật các biến thể cố ý sai để chứng tỏ checker thật sự bắt được lỗi. Giới hạn của checker (nhận xét của người soạn từ đọc code, không phải từ paper): "đã commit" nghĩa là một node đã tăng `commitIndex`, hẹp hơn "đã nhân bản trên đa số" của paper, và Leader Completeness chỉ được kiểm với các leader đang tại vị lúc entry được commit — một leader term cao hơn đã mất vị trí trước lúc đó không bị kiểm.
- **Mô hình bỏ qua:** snapshot[^f-snapshot], đổi thành viên (joint consensus[^f-joint]), pre-vote hay leader stickiness[^f-disrupt], đọc linearizable[^f-read], giá của `fsync` (etcd nhấn mạnh độ trễ đĩa là một phần "sức sống" của leader[^f-etcd-disk]), lệch đồng hồ, session client, lỗi đĩa. Mọi con số dưới đây là của *mô hình*, không phải số đo hệ thật.

```bash
cd labs && cargo run -q -p lab-distributed --bin raft_election -- 42 300    # exit 0
```

Tham số: seed của timeline và của kịch bản Figure 8 (mặc định 42); số seed của mỗi sweep (mặc định 300, luôn là các seed `0..300`; đặt `0` để chỉ in timeline). Debug build chạy khoảng 4 giây. Output (macOS aarch64, rustc 1.95.0; xác định theo seed, chạy hai lần `diff` giống hệt) gồm ba đoạn liên tiếp của cùng một lần chạy. Logic mô phỏng dùng số nguyên; số thực chỉ có ở `Rng::chance` khi sinh kế hoạch crash và khi in các số trung bình (`{:.2}`), nên chữ số thập phân cuối có thể khác trên nền tảng khác (chưa kiểm).

```text
Lab (f) raft_election | seed 42 | sweeps over seeds 0..300
virtual ms; election timeout 150-300, heartbeat 50, latency 5-15, one client command per 100 ms
== 1. Election timeline (seed 42): leader crashes at 900 ms, next leader is cut off 1500-2000 ms ==
t=      195 ms  n3 election timeout: candidate for term 1
t= 200..209 ms  n2,n0,n4,n1 vote for n3 (term 1)
t=      214 ms  n3 wins term 1 with 3/5 votes: LEADER (log length 0)
t=      900 ms  n3 CRASHES (was leader)
t=     1028 ms  n4 election timeout: candidate for term 2
t=1038..1042 ms  n2,n0,n1 vote for n4 (term 2)
t=     1050 ms  n4 wins term 2 with 3/5 votes: LEADER (log length 6)
t=     1400 ms  n3 restarts as follower (term 1, log length 6)
t=     1500 ms  leader n4 is cut off from the other nodes
t=     1648 ms  n1 election timeout: candidate for term 3
t=1653..1659 ms  n2,n0,n3 vote for n1 (term 3)
t=     1663 ms  n1 wins term 3 with 3/5 votes: LEADER (log length 8)
t=     2000 ms  network healed
t=     2006 ms  n4 (leader) sees term 3 > 2: steps down to follower
trace hash 0x4f8d51db3c9c2dd0, re-run identical: true; invariants violated: 0
```

```text
== 2. Scenarios: 5 nodes x 300 seeds x 4000 ms (faults stop at 3000 ms, then all is healed) ==
scenario      1st leader ms elections  max committed overwrit.         recovery ms converged viol.
                  med / p95   per run term   per run   per run       med / p95 (n)                
no faults        189 /  238      1.00    2      30.8      0.00     - /   - (   0)   300/300     0
drops 10-20%     195 /  340      1.05    4      30.4      0.02     - /   - (   0)   300/300     0
crash+restart    189 /  238      4.08    8      23.6      0.00   186 / 302 ( 922)   300/300     0
partitions       189 /  238      3.11   13      23.5      6.38   176 / 383 ( 373)   300/300     0
combined         192 /  249      4.93   12      20.5      3.52   190 / 406 (1039)   300/300     0
== 3. Election timeout in [150, 150+w) ms: 5 nodes, constant 10 ms latency, no faults, deadline 2000 ms ==
w (ms)  leader<=2s 1st leader med/p95 mean term of 1st campaigns/run
     0       0/300        - /      -                -         65.00
     5     102/300     1381 /   1987             9.17         56.36
    10     277/300      779 /   1708             5.29         25.38
    25     300/300      175 /    493             1.50          4.51
    50     300/300      177 /    338             1.08          2.24
   150     300/300      190 /    235             1.01          1.38
```

```text
== 4. Mutants: the checker must bite; the correct algorithm runs on the same seeds ==
mutant                   faults used               first violation      first seed violating  correct
NoPersistVote            crash right after voting  Election Safety               5    25/300        0
NoUpToDateCheck          partitions                Leader Completeness           1   164/300        0
MinorityLeader           no faults                 Election Safety               7    21/300        0
CommitOldTermByCounting  Figure 8 script           Leader Completeness           0   300/300        0
NoPrevLogTermCheck       combined                  State Machine Safety          1   114/300        0
== 5. Figure 8 replay (seed 42), CommitOldTermByCounting: S1..S5 = n0..n4, elections forced by a script ==
t=      805 ms  n0 wins term 4 with 3/5 votes: LEADER (log length 2)
t=      840 ms  n0 commits up to index 2
t=      900 ms  n0 CRASHES (was leader)
t=     1039 ms  n4 wins term 5 with 3/5 votes: LEADER (log length 2)
Leader Completeness violated at t=1039 ms: n4 won term 5 but its log lacks index 2 (term 2, cmd 2) committed in term 4
State Machine Safety violated at t=1078 ms: n4 applies (term 3, cmd 3) at index 2, another node applied (term 2, cmd 2)
correct algorithm: 0 violations, 3 entries overwritten, converged: true, every log [(1,1) (3,3) (5,5)]
```

Giải thích:

1. **Timeline (mục 1)** — [chương 09 mục 7](../12-he-thong-du-lieu/09-consistency-va-consensus.md): vai trò và term, bầu leader, leader cũ.
   - Dòng `vote` ghi lúc *voter* cấp phiếu; candidate nhận trả lời 5–15 ms sau, nên n3 thắng lúc 214 ms khi có phiếu thứ hai (cộng phiếu của chính nó là 3/5, đa số[^f-majority]); hai phiếu còn lại đến sau. n3 hết giờ lúc 195 ms, không sớm hơn mốc 150 ms; ngẫu nhiên hóa để thường chỉ một node hết giờ trước[^f-random].
   - Leader n3 chết lúc 900 ms; n4 hết giờ lúc 1028 ms (timer của nó chạy từ heartbeat cuối) và thắng lúc 1050 ms: cụm không khả dụng chừng một election timeout[^f-timing-ineq]. n3 restart với `term 1, log length 6`: term và log là trạng thái bền.
   - Lúc 1500 ms leader n4 bị cô lập nhưng *vẫn là leader của term 2* tới 2006 ms, trong khi n1 đã làm leader term 3 từ 1663 ms: hai "leader" ở hai term khác nhau cùng tồn tại, như bài giảng nhấn mạnh[^f-stale-leader]. Election Safety chỉ cấm hai leader *cùng term*[^f-es]; term lớn hơn là cách node phát hiện thông tin cũ như leader cũ[^f-stale-term] (suy luận của người soạn: cùng họ ý tưởng với fencing token).
2. **Bảng kịch bản (mục 2)** — [chương 09 mục 7](../12-he-thong-du-lieu/09-consistency-va-consensus.md): bầu leader, chép log, sửa log lệch.
   - `1st leader` giống nhau ở `no faults`, `crash+restart` và `partitions` vì lỗi đầu tiên chỉ xảy ra sau ≥ 300 ms (cùng seed ⇒ cùng quá khứ); trung vị 189 ms ≈ nhỏ nhất của 5 timeout (kỳ vọng 150 + 150/6 = 175 ms, tính của người soạn) cộng một vòng RPC.
   - `elections` = 1,00 khi không lỗi: heartbeat 50 ms giữ mọi follower dưới ngưỡng ≥ 150 ms. Crash và partition đẩy lên 3–5 lần bầu mỗi lần chạy. `committed` ≈ 30,8 khi không lỗi: khoảng 30 lệnh client (mỗi 100 ms tới 3300 ms) cộng một no-op; lỗi làm số này giảm vì client phải dò lại leader.
   - `max term` 13 ở `partitions`: một node bị cô lập tự tăng term mỗi lần hết giờ, khi nối lại nó ép leader đang sống phải step down (suy luận của người soạn từ cơ chế). Paper nêu hiện tượng tương tự với server bị gỡ khỏi cụm và chữa bằng cách bỏ qua `RequestVote` đến trong vòng election timeout tối thiểu kể từ lần nghe leader[^f-disrupt]; lab chưa cài (là bài tập mở rộng của lab (f) về pre-vote).
   - `overwrit.` chỉ lớn ở `partitions` (6,38 entry mỗi lần chạy, ~0 ở crash thuần): đó là phần đuôi *chưa commit* của leader cũ bị cô lập, bị leader mới ghi đè[^f-force].
   - `recovery` (từ lúc mất leader tới lúc có leader mới) trung vị 176–190 ms, cỡ một election timeout[^f-timing-ineq]; p95 cao hơn do split vote. Test khẳng định, ở hai kịch bản không mất gói, không lần nào dưới 150 + 10 − 50 = 110 ms (timeout nhỏ nhất + một vòng RPC nhanh nhất − một heartbeat).
   - `converged` 300/300: 1 s sau khi chữa mọi thứ, mọi node sống, log giống hệt, mọi entry đã commit đã được áp ở mọi node (no-op giúp leader mới commit entry cũ mà không chờ lệnh client[^f-noop]). `viol.` = 0 theo checker ở trên.
3. **Timeout ngẫu nhiên và FLP (mục 3)** — [chương 09 mục 7](../12-he-thong-du-lieu/09-consistency-va-consensus.md) (bầu leader) và [mục 6](../12-he-thong-du-lieu/09-consistency-va-consensus.md) (FLP).
   - Với `w = 0` cả 5 node hết giờ *cùng lúc*, ai cũng bỏ phiếu cho chính mình rồi từ chối nhau: split vote lặp mãi[^f-split]. Đúng 65 campaign mỗi lần chạy = 13 vòng × 5 node, không lần nào có leader (test khẳng định con số 13 × 5).
   - Với `w = 5` ms, nhỏ hơn latency 10 ms, nhiều seed vẫn kẹt (102/300 có leader trong 2 s) vì hai ứng viên cách nhau dưới 10 ms không kịp thấy `RequestVote` của nhau (suy luận của người soạn). Từ `w = 25` ms mọi seed đều có leader; với `w = 150` (khoảng của paper) term trung bình của leader đầu tiên là 1,01.
   - Trên cụm 5 server của họ, paper đo: không ngẫu nhiên thì bầu leader "luôn lâu hơn 10 giây", thêm 5 ms ngẫu nhiên thì trung vị còn 287 ms[^f-eval-norandom][^f-eval-5ms]. Cùng chiều với bảng, nhưng thí nghiệm khác (crash leader, đo trên máy thật) nên **không so được con số**.
   - FLP: trong hệ bất đồng bộ, không có thuật toán consensus tất định nào bảo đảm kết thúc khi dù chỉ một node có thể hỏng[^f-flp]; Raft chỉ dùng đồng hồ cho timeout[^f-flp-clocks]. Hàng `w = 0` **không** phải một ví dụ của FLP (không có lỗi nào, độ trễ hằng số — một thiết lập đồng bộ): nó không kết thúc vì các timer *đối xứng hoàn toàn*; nối nó với "ngẫu nhiên giúp thoát livelock" chỉ là phép so sánh của người soạn. Các hàng `w ≥ 25` là *quan sát*, không phải chứng minh kết thúc.
4. **Mutant (mục 4)** — [chương 09 mục 7](../12-he-thong-du-lieu/09-consistency-va-consensus.md), phần "Hai luật an toàn" và "Năm tính chất". Cột `correct` = 0: thuật toán đúng chạy trên *đúng các seed và kế hoạch lỗi* đó không vi phạm.
   - `NoPersistVote` mất `votedFor` khi restart: hai ứng viên cùng term đều có đa số, vi phạm Election Safety (luật "mỗi term một phiếu"[^f-vote] phải bền; openraft ghi rõ vote phải nằm trên đĩa trước khi trả về[^f-save-vote]). Crash ngẫu nhiên thuần không bắt được (test: 0 trong 120 seed ở `crash+restart` và `combined`), nên lab chèn lỗi nhắm vào cửa sổ hẹp: 50% node crash ngay sau khi cấp phiếu và quay lại sau 1–5 ms (25/300 seed). Thời gian 1–5 ms rất ngắn, chỉ để dựng đúng tình huống hai `RequestVote` cùng term đến cách nhau (nhận xét của người soạn).
   - `NoUpToDateCheck` bỏ luật log đủ mới[^f-restr] — một follower tụt hậu sau partition thắng cử mà thiếu entry đã commit, vi phạm Leader Completeness[^f-lc].
   - `MinorityLeader` thắng với `N/2` phiếu: hai ứng viên cùng term cùng thắng, phá tính giao nhau của hai đa số[^f-majority].
   - `NoPrevLogTermCheck` chỉ kiểm độ dài chứ không kiểm term của entry đứng trước[^f-consistency] — follower lệch log vẫn nhận `AppendEntries` rồi áp entry khác (State Machine Safety). Khi cho chạy tiếp thay vì dừng ở vi phạm đầu, seed 1 còn vi phạm thêm Log Matching (test ghim); lab không đếm xem cả 114 seed có đều vi phạm Log Matching hay không, vì quét mutant dừng ở vi phạm đầu tiên.
5. **Figure 8 (mục 5)** — [chương 09 mục 7](../12-he-thong-du-lieu/09-consistency-va-consensus.md), phần "Chỉ đếm bản sao cho entry của term hiện tại".
   - Kịch bản dựng lại Hình 8 (election timer tắt, mỗi cuộc bầu cử do script ép bằng `ForceTimeout`; no-op tắt): S1 (n0) làm leader term 4 và đưa entry term 2 lên đa số[^f-fig8] (đơn giản hóa của lab so với Hình 8(c): S1 chưa giữ entry term 4 nào chưa nhân bản). Leader đúng *không* commit entry đó[^f-prior].
   - Mutant `CommitOldTermByCounting` commit nó ở 840 ms rồi S1 chết. S5 (n4) có log cuối term 3, "mới hơn" log của S2/S3 có term cuối 2[^f-uptodate], nên thắng term 5 lúc 1039 ms và ghi đè: Leader Completeness bị vi phạm ngay lúc thắng cử, State Machine Safety bị vi phạm lúc n4 áp entry term 3 ở chỉ số 2 trong khi S1 đã áp entry term 2.
   - Chạy đúng: không vi phạm, 3 entry bị ghi đè (S2, S3 và S1 sau khi restart), mọi log hội tụ. Trên 300 seed, bản đúng có 0 vi phạm còn mutant vi phạm 300/300; chi tiết "3 entry bị ghi đè, log hội tụ" được test kiểm trên 40 seed (chỉ khác latency).
6. **Test** (`cargo test -p lab-distributed raft::`; thời gian của module khoảng vài giây ở debug và phụ thuộc máy).
   - `all_five_invariants_hold_in_every_scenario_and_the_cluster_recovers`: 5 kịch bản × (200 seed × 5 node + 100 seed × 3 node) bằng `simulate_checked` (panic kèm seed nếu bất kỳ tính chất nào bị vi phạm) và đòi hội tụ sau khi chữa lỗi. `safety_does_not_depend_on_timing`: 250 cấu hình ngẫu nhiên (3–7 node, timeout 5–450 ms, latency tới 180 ms, mất tới 40%, trùng tới 20%) vì an toàn không được phụ thuộc thời gian[^f-timing-safety]. `invariants_hold_without_the_leader_noop_too`: no-op chỉ giúp tính sống, tắt đi vẫn an toàn. `sweeps_really_exercise_…` đòi sweep thật sự có bầu lại, ghi đè và term tăng, để "0 vi phạm" không phải do chưa đi vào chỗ nguy hiểm.
   - Mỗi mutant có seed *đầu tiên* bị bắt được ghim (5, 1, 7, 1; Figure 8: 40 seed) kèm tính chất cụ thể, và bản đúng sạch trên cùng seed. Có thêm test từng luật (một phiếu mỗi term, so sánh log đủ mới, `commitIndex` bị chặn bởi entry mới cuối cùng, chỉ đếm bản sao của term hiện tại), test checker trên trạng thái dựng sẵn cho đủ năm tính chất, một test đối chiếu checker online (chỉ xét lại log vừa đổi) với quét toàn bộ trạng thái mỗi 5 ms, quorum (`2` trên `5` node hỏng vẫn commit, `3` thì không) và băm vết sự kiện (cùng seed ⇒ cùng hash).
   - Test dài `soak_100k_random_configurations_and_20k_seeds_per_scenario` (`#[ignore]`, chạy bằng `cargo test --release -p lab-distributed -- --ignored raft`, khoảng 1 phút; người soạn đã chạy một lần, đạt): 100.000 cấu hình ngẫu nhiên như trên (horizon 4 s) và 20.000 seed × 5 kịch bản × {3, 5} node, đều 0 vi phạm và 20.000/20.000 hội tụ.
7. **Sweep không phải chứng minh** — [P13 chương 08 mục 7–8](../13-tla-plus/08-dac-ta-giao-thuc-phan-tan.md).
   - "Không phản ví dụ trong N seed" chỉ là bằng chứng lấy mẫu: `NoPersistVote` sống sót qua cả trăm seed crash ngẫu nhiên, nên *độ phủ phụ thuộc cách chọn lỗi*.
   - Model checker duyệt mọi lịch trong biên của một mô hình nhỏ, nhưng tác giả Raft nói Murphi có lẽ đã bỏ sót một lỗi luật commit vì mô hình bị giới hạn kích thước[^f-tlc-missed]. AWS kể một lỗi mất dữ liệu có vết ngắn nhất 35 bước đã lọt qua review, code review và test[^f-aws-dynamo][^f-aws-unnoticed], nhưng cũng nói TLA+ không cho biết code có cài đúng thiết kế đã kiểm hay không[^f-aws-gap].
   - Hai công cụ bổ sung nhau (nhận xét của người soạn): lab chạy *code* với lỗi ngẫu nhiên và mutant để thử cả checker; TLA+ ([P13](../13-tla-plus/08-dac-ta-giao-thuc-phan-tan.md)) duyệt *đặc tả*; chính paper có đặc tả TLA+ của Hình 2[^f-tla-spec].

Hai ứng viên của cùng một term xảy ra ở seed 38. Lệnh sau chỉ in timeline; đoạn dưới là 8 dòng đầu của output (rút gọn):

```bash
cd labs && cargo run -q -p lab-distributed --bin raft_election -- 38 0    # exit 0
```

```text
Lab (f) raft_election | seed 38 | timeline only
virtual ms; election timeout 150-300, heartbeat 50, latency 5-15, one client command per 100 ms
== 1. Election timeline (seed 38): leader crashes at 900 ms, next leader is cut off 1500-2000 ms ==
t=      179 ms  n0 election timeout: candidate for term 1
t=      181 ms  n4 election timeout: candidate for term 1
t= 184..191 ms  n1,n2,n3 vote for n0 (term 1)
t=      195 ms  n0 wins term 1 with 3/5 votes: LEADER (log length 0)
t=      206 ms  n4 hears leader n0 of term 1: back to follower
...
```

n0 và n4 hết giờ cách nhau 2 ms; ba node kia đã bỏ phiếu cho n0 trước khi `RequestVote` của n4 tới nên n4 chỉ có phiếu của chính nó. Khi nhận `AppendEntries` của leader term 1 (bằng term của nó), n4 coi leader là hợp lệ và quay về follower[^f-cand-recognize].

### Lab (g) — `delivery_dedup`: at-least-once, idempotency key và outbox dưới crash

Lý thuyết: [chương 11 mục 3](../12-he-thong-du-lieu/11-stream-processing.md) (log, offset, consumer group), [mục 4](../12-he-thong-du-lieu/11-stream-processing.md) (at-most/at-least/exactly-once, producer idempotent, offset lưu cùng output), [chương 12 mục 1](../12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md) (dual write), [mục 3](../12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md) (end-to-end), [mục 4](../12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md) (`Idempotency-Key`), [mục 5](../12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md) ("exactly-once" = at-least-once + dedup), [mục 6](../12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md) (outbox), [chương 08 mục 1](../12-he-thong-du-lieu/08-su-co-he-phan-tan.md) (RPC timeout mơ hồ) và [chương 17 mục 3.3](../12-he-thong-du-lieu/17-redis-patterns.md) (key lưu ngoài DB hỏng ở đâu).

**Mô hình của lab.** Bốn mô phỏng xác định theo seed, chỉ dùng `std`. "Kho bền" là một struct trong bộ nhớ; một *bước* (tick) chạm kho bền có thể bị crash ngay trước nó, và mọi thứ sau tick mà không có tick tiếp là **nguyên tử** — nó đại diện cho một transaction DB hoặc một transaction Kafka, không phải bản thân các hệ đó. Crash làm mất trạng thái trong bộ nhớ của tiến trình và khởi động lại từ trạng thái bền. Có ba chế độ crash (`CrashMode`): không crash, ngẫu nhiên (`CRASH_P` = 0,15 mỗi bước), và **liệt kê vét cạn**: mọi tổ hợp crash/không-crash trong `K` tick đầu (`2^14` = 16384 mẫu, kể cả crash lúc đang recovery). (1) Consumer một partition, `EVENTS` = 8 event, poll `BATCH` = 2 event; sink là sổ cái cộng `amount` của event; sáu thiết kế, từ `commit→process` tới "offset lưu nguyên tử cùng state". (2) Producer gửi `(producer_id, seq)`; request có thể mất, hoặc broker đã append mà ack mất; cửa sổ in-flight `window`. (3) Server `POST /pay` có/không có key, `LEASE_TTL` = 4; client retry tối đa `MAX_ATTEMPTS` = 8 lần. (4) Service vừa cập nhật DB vừa phát event: dual write hai chiều so với outbox + relay.

**Mô hình không có**: nhiều consumer đồng thời/rebalance, replication và ISR của broker, `fsync` (mọi commit coi là bền), độ trễ mạng thật (thời gian là số nguyên ảo), cô lập transaction của DB (suy luận của người soạn: với ràng buộc unique trong DB thật, retry khi bản gốc đang chạy có thể *chờ* ở unique index chứ không thấy `InProgress`; chi tiết và giới hạn ở [chương 12 mục 4](../12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md), phần Đi sâu). Ở phần 4, request bị crash giữa chừng **không** được client retry — đó là sự đơn giản hóa để lộ lỗi. Mọi con số dưới đây là đếm trong mô hình với xác suất do người soạn đặt, **không phải đo hệ thật**.

```bash
cd labs && cargo run -q -p lab-distributed --bin delivery_dedup    # exit 0; đối số tùy chọn: seed (mặc định 42)
```

Output (macOS aarch64, rustc 1.95.0; xác định theo seed; logic mô phỏng dùng số nguyên, `f64` chỉ dùng để rút mẫu "có crash không" bằng `Rng::chance`, các cột in ra đều là số đếm nguyên; chưa kiểm trên nền tảng khác):

```text
== Lab (g) delivery_dedup  seed=42 ==

-- 1. consumer: 8 events, batch 2, crash p=0.15/step, 1000 runs (8000 events) --
design                lost   dup  exact | exhaustive: 4 events, all 2^14 crash patterns -> runs with loss / dup
commit->process       1754     0   6246 |  15335 /      0
process->commit          0  1736   6264 |      0 /  15335
dedup apply,record       0  1143   6857 |      0 /  14728
dedup record,apply    1190     0   6810 |  14728 /      0
dedup atomic             0     0   8000 |      0 /      0
offset+state atomic      0     0   8000 |      0 /      0

-- 2. producer: 20 msgs/run, request lost 0.20, ack lost 0.20, 1000 runs --
producer                        log   dups misordered  rejected
plain retry window=1          24982   4982          0         0
plain retry window=3          24982   4982        914         0
idempotent (pid,seq) window=1    20000      0          0         0
idempotent (pid,seq) window=3    20000      0          0      3685

-- 3. POST /pay with Idempotency-Key (lease 4) --
t=1 key=7 amount=500 retry while original in flight   -> 409
t=2 key=7 amount=500 first request completes          -> 201 created charge #1
t=3 key=7 amount=500 retry after done                 -> 201 replay of charge #1
t=4 key=7 amount=900 same key, different amount       -> 422
charges after script: 1; stale handler after lease takeover: non-atomic 2 charges, atomic 1
1000 intentions, step/reply fail p=0.15, <= 8 attempts; exhaustive = 1 intention x 2^12 patterns
design            double   never confirmed  409s | exh double      of
no key               134       0      1000     0 |       1976    4096
key, non-atomic      147       0       998   429 |       1824    4096
key, atomic            0       0      1000   224 |          0    4096

-- 4. update DB + publish event: 20 ops/run, crash p=0.10/step, 500 runs --
design                 committed  lost  phantom   dup  effect dup   effect dup
                                                 msgs  (no dedup) (dedup by id)
dual write DB->broker       9017   899        0     0           0            0
dual write broker->DB       8118     0      899     0           0            0
outbox + relay              9012     0        0   929         929            0
```

Giải thích:

1. **Thứ tự commit/process (phần 1, hai dòng đầu).** `commit→process` mất 1754 trên 8000 event và không trùng event nào; `process→commit` ngược lại: 0 mất, 1736 trùng. Đúng bảng của docs Kafka: lưu offset trước rồi xử lý thì process thay thế bỏ sót message chưa xử lý (at-most-once)[^g-k-cons-amo]; xử lý trước rồi lưu offset thì process thay thế xử lý lại vài message đầu (at-least-once)[^g-k-cons-alo]. Trong mô hình, "cửa sổ nguy hiểm" là đúng một tick cho mỗi event (với batch `b` là `b` tick mỗi batch); một test pin điều đó: với 3 event và batch 1, trong 6 vị trí crash đơn lẻ có đúng 3 vị trí gây 1 lần mất (hoặc 1 lần trùng).
2. **Dedup không nguyên tử vẫn rò (hai dòng giữa).** Nếu sink *kiểm id → áp dụng → ghi id* thành các bước rời, crash giữa "áp dụng" và "ghi id" làm lần giao lại áp dụng lần nữa: 1143 trùng (ít hơn 1736 vì phần lớn lần giao lại bị chặn, nhưng vẫn rò). Đảo thứ tự *ghi id → áp dụng* thì crash ở giữa làm lần giao lại thấy id đã có và bỏ qua: 1190 mất. Đây là lý do Kafka nói cách đơn giản và tổng quát là lưu offset **cùng chỗ** với output vì nhiều hệ đích không có two-phase commit[^g-k-external], và Kleppmann đòi trạng thái dedup nằm ở stable storage để nhận ra bản trùng cả sau crash[^g-cam-dedup]. Chương 12 mục 5 vẽ đúng bản đúng: `INSERT processed_ids … ON CONFLICT DO NOTHING` và tác dụng nghiệp vụ **cùng một transaction**.
3. **Hai thiết kế nguyên tử (hai dòng cuối).** `dedup atomic` ghi id và áp dụng trong một commit; `offset+state atomic` ghi cả batch và offset mới trong một commit, không cần bảng id (consumer đọc offset từ chính sink — mô hình của lab cho ý "offset lưu cùng output"). Cả hai: 0 mất, 0 trùng ở cả 1000 run ngẫu nhiên lẫn **mọi** trong 16384 mẫu crash của cột vét cạn. Với Kafka → Kafka, docs nói offset được ghi trong cùng transaction với output topic[^g-k-offset-in-txn]; Kleppmann gọi tổng quát là "retry + idempotence hoặc deduplication"[^g-cam-eo]. Cột vét cạn đếm *số run* (trên 16384) có ít nhất một event mất/trùng, nên 15335 ≠ "xác suất"; nó chỉ cho thấy bốn thiết kế đầu có phản ví dụ còn hai thiết kế cuối thì không có. Vét cạn chỉ phủ `K` tick đầu của log 4 event; chứng minh thật cần lập luận về tính nguyên tử, không phải bảng này.
4. **Producer (phần 2).** Producer retry cho tới khi nhận ack nên không mất message nào, nhưng không có dedup thì mỗi lần ack mất sau khi broker đã append sinh một bản trùng: 4982 trùng trên 20000 message. Docs Kafka nói đúng điều này: lỗi mạng thì producer không biết lỗi xảy ra trước hay sau khi message được commit[^g-k-prod-unsure], và `enable.idempotence=false` thì retry có thể ghi trùng[^g-kp-enable-idem]. Với dedup theo `(producer_id, seq)` — cơ chế docs mô tả: broker gán producer ID và khử trùng bằng sequence number[^g-k-idem] — log đúng 20000 dòng. Cột `misordered`: với `window=3` và không dedup, 914/1000 run có message bị đảo thứ tự vì bản gửi lại rơi sau bản gửi sau; docs Kafka cảnh báo đúng kịch bản "batch đầu fail rồi retry, batch sau thành công thì batch sau xuất hiện trước"[^g-kp-retries-order]. Với idempotent + `window=3`, broker của mô hình **từ chối** (`rejected` = 3685) request có `seq` vượt trước `seq` kỳ vọng, nên thứ tự giữ nguyên; docs Kafka hứa thứ tự được giữ với mọi giá trị cho phép của `max.in.flight`[^g-kp-idem-order], nhưng *cơ chế từ chối* là cách mô hình của lab tự dựng, không phải mô tả bên trong Kafka (không có nguồn trong chương). Hai dòng `plain retry` có cùng 4982 trùng vì cùng seed cho cùng dãy mất/không mất; `window` chỉ đổi thứ tự.
5. **`Idempotency-Key` (phần 3, kịch bản).** Bốn tình huống của [chương 12 mục 4](../12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md), chạy tuần tự trên một server có key: retry khi bản gốc còn chạy → 409 (bản thảo IETF: "SHOULD reply with an HTTP 409"[^g-idem-409]); bản gốc xong → 201; retry sau khi xong → trả lại *đúng* kết quả đã lưu, cùng `charge #1`, không trừ thêm (bản thảo: trả kết quả của thao tác đã hoàn tất[^g-idem-retry]; Stripe API v1 lưu status code và body của request đầu rồi trả lại cho retry cùng key[^g-stripe-v1]); cùng key khác payload → 422 (bản thảo[^g-idem-422]). Lưu ý: bản thảo này **đã hết hạn**, datatracker ghi nó không còn hoạt động[^g-idem-expired] — mã 409/422 ở đây là thực hành được mô tả, không phải chuẩn. "Fingerprint" của mô hình là hash FNV của `amount`, thay cho checksum payload.
6. **Key store không nguyên tử chuyển cửa sổ lỗi chứ không đóng nó (phần 3, bảng).** `no key`: 134/1000 intention bị trừ tiền hai lần (reply mất → client retry → trừ lại). `key, non-atomic` (đánh dấu `InProgress` → trừ tiền → đánh dấu `Done` là ba bước rời): 147/1000 — **không tốt hơn**; cửa sổ "reply mất" đóng lại (retry sau khi `Done` nhận bản replay) nhưng mở cửa sổ "crash giữa trừ tiền và ghi `Done`": lease hết hạn, retry giành được key và trừ lần hai, đúng điểm hỏng của [chương 17 mục 3.3](../12-he-thong-du-lieu/17-redis-patterns.md). Chênh 134 vs 147 là nhiễu lấy mẫu (cột vét cạn đi ngược chiều: 1976 vs 1824 trên 4096 mẫu không có trọng số xác suất) — đừng đọc thành xếp hạng. `key, atomic` ("trừ tiền + `Done`" một commit, kèm kiểm tra key còn chưa `Done`): 0 lần trừ đôi trong 1000 intention **và** trong 4096 mẫu vét cạn; dòng "stale handler" cho thấy handler chậm, lease đã bị chiếm, vẫn không trừ lần hai ở bản atomic (2 so với 1 charge). Đây là cùng lời khuyên của Kleppmann: dedup request cần lưu ở stable storage[^g-kleppmann-dedup], và (nhận xét của người soạn) cùng chỗ, cùng transaction với tác dụng. Cột `409s` (224 và 429) là client thấy lease còn hạn sau khi server crash giữa chừng: bản gốc đã chết nhưng key vẫn `InProgress` tới khi lease hết — cái giá của TTL, và `never` = 0 vì client retry đủ lâu (`confirmed` 998 ở bản non-atomic: hai intention hết 8 lần thử).
7. **Dual write và outbox (phần 4).** `DB→broker`: 899 event mất (DB đã commit, crash trước khi publish), 0 phantom. `broker→DB`: 899 phantom (event đã publish, DB không có thay đổi), 0 mất. Đây là hai nhánh "gửi giữa transaction / gửi sau commit" mà microservices.io mô tả: không có gì bảo đảm transaction commit, hoặc tiến trình không chết trước khi gửi[^g-msio-fail]. Dedup **không sửa được** hai thứ này: cột `effect dup` bằng 0 nhưng event vẫn mất/phantom (tức nhất quán sai theo kiểu khác). `outbox + relay`: ghi dòng outbox trong cùng commit với thay đổi DB, relay publish rồi đánh dấu; 0 mất, 0 phantom — "gửi nếu và chỉ nếu transaction commit"[^g-msio-benefit] — nhưng relay crash giữa publish và đánh dấu nên 929 event bị publish hai lần; docs của pattern cũng nói relay có thể publish một message nhiều lần[^g-msio-dup]. Consumer không dedup áp dụng trùng 929 lần; dedup theo event id (Debezium nêu `id` của outbox dùng được để loại message trùng[^g-debezium-outbox-id]) đưa về 0. Tóm lại outbox đổi "mất/phantom" lấy "trùng", và trùng thì dedup xử được: đây là "exactly-once = at-least-once + dedup" của [chương 12 mục 5](../12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md); OLEP viết cùng ý: framework "exactly-once" vẫn at-least-once với kho ngoài và dựa vào idempotence[^g-olep-exact-ext].
8. **Nhận xét về phạm vi (nhận xét của người soạn).** Một seed không chứng minh gì; các bất biến nằm ở test (`cargo test -p lab-distributed delivery::`, chạy 21 test của module `delivery`, khoảng 0,3 s ở debug trên máy người soạn): vét cạn `2^14` mẫu crash × 4 cấu hình `(events, batch)` cho từng thiết kế consumer (`commit→process`: không bao giờ trùng, có mất; `process→commit`: không bao giờ mất, có trùng; hai dedup không nguyên tử: phản ví dụ; hai thiết kế nguyên tử: 0 mất 0 trùng), thêm 600 seed ngẫu nhiên với `n`, `batch`, `p` thay đổi; producer 300 run × `window` ∈ {1, 3, 5}; vét cạn `2^12` mẫu (kèm crash lúc client nhận reply) cho server `Idempotency-Key`, và 300 seed cho atomic = 0 trừ đôi; vét cạn `2^12` mẫu × 4 seed scheduler cho outbox (0 mất, 0 phantom, sau dedup mỗi op đã commit có đúng một tác dụng). Cái bảng không nói: tỉ lệ thật trong production, hành vi khi có nhiều consumer đồng thời (zombie sau rebalance cần fencing — xem lab (d)), hay độ bền thật của "commit nguyên tử".

### Lab (h) — `watermark`: event time, window và watermark

Lý thuyết: [chương 11 mục 6](../12-he-thong-du-lieu/11-stream-processing.md) (event time vs processing time, tumbling/hopping/session, watermark và hai kiểu sai của nó, allowed lateness/grace period, trade-off độ trễ–độ đủ, đoạn "Đi sâu" về nguồn nhàn rỗi), [mục 8](../12-he-thong-du-lieu/11-stream-processing.md) (replay cho kết quả event-time tái lập được — chỉ nhắc tới), [chương 08 mục 5](../12-he-thong-du-lieu/08-su-co-he-phan-tan.md) và [mục 6](../12-he-thong-du-lieu/08-su-co-he-phan-tan.md) (đồng hồ thiết bị không đáng tin).

**Mô hình của lab** (module `windows`, thuần hàm của seed, thời gian là số nguyên ms ảo): `E = 2000` event, mỗi ~7 ms một event, 4 key, 2 partition đầu vào (`key % 2`). Mỗi event có event time `ts` thật và bị trễ một khoảng `arrival - ts` theo phân phối đuôi nặng `DelayModel::HEAVY_TAIL` (85% trong `[0,40)` ms, 12% trong `[40,400)`, 3% trong `[1000,3000)` — "shipper đi qua hầm" của chương 11), nên thứ tự đến khác thứ tự `ts`. Processing time chính là `arrival`. Window là tumbling `[s, s+W)`, `W = 1000` ms. Hai pipeline đầu chạy trên cùng một dãy event theo thứ tự đến; thí nghiệm nguồn nhàn rỗi (pipeline 3, cuối bảng) dùng dãy riêng (độ trễ nhỏ + một sự cố ở partition 1, `D = 50`):

1. **Processing-time window:** đếm theo `arrival`. Chương 11 và Kafka Streams định nghĩa event time là lúc sự việc xảy ra "at the source" còn processing time là lúc record được xử lý[^h-ks-event-time][^h-ks-proc-time]; Dataflow nói độ lệch giữa hai miền này thay đổi động vì trễ truyền thông, lập lịch…[^h-df-skew]
2. **Event-time window + watermark `max_ts_đã_thấy − D`** (`run_event_time`): window `[s, s+W)` phát kết quả khi watermark `≥ s+W`, đúng tinh thần "watermark trigger fires when the watermark passes the end of the window"[^h-df-wm-trigger]. Flink có sẵn kiểu watermark này dưới tên bounded-out-of-orderness[^h-fl-bounded]. Chính sách cho event tới sau khi window đã phát: `LatePolicy::Drop` (allowed lateness 0 — event bị bỏ[^h-fl-late-drop]) hoặc `LatePolicy::AllowedLateness(L)` (khi `wm < end + L` event trễ vẫn được cộng vào window[^h-fl-lateness] và window có thể phát lại một kết quả cập nhật — "re-fire"[^h-fl-refire]; sau đó event bị bỏ). Cột `late` của lab đếm event tới khi **window của nó đã phát** (watermark đã vượt cuối window, cộng allowed lateness nếu có); đó là cách đếm theo window của lab, hẹp hơn định nghĩa "element trễ" của Flink — element tới sau khi watermark đã vượt timestamp của nó[^h-fl-late]. Hết input thì watermark nhảy lên vô cực và các window còn lại được phát (cột `flush`).
3. **Hai partition + min-watermark:** watermark của toán tử là min của các partition đang hoạt động[^h-fl-wm-min]; `idle_timeout` loại partition im lặng quá lâu khỏi phép min (Flink có `WatermarkStrategy` phát hiện nguồn nhàn rỗi và đánh dấu idle[^h-fl-idle2], ví dụ helper `withIdleness(Duration)`[^h-fl-idle3]).

Mô hình **không** có: đồng hồ lệch (event time và processing time dùng chung một trục, chỉ khác nhau ở độ trễ `arrival − ts`; đồng hồ thiết bị thật có thể sai nên event time cũng sai theo — suy luận của người soạn, xem chương 08 mục 6), độ trễ tương quan (một thiết bị trong hầm làm trễ *nhiều* event liền nhau; ở đây các độ trễ độc lập), window theo key, state/checkpoint, trigger sớm, và việc watermark thật thường không biết `D`. Định nghĩa watermark của lab là "không còn event nào có `ts` < wm" (biên mở); Flink dùng biên đóng `ts' <= t`[^h-fl-wm-def] — lệch một đơn vị ở biên, không ảnh hưởng kết luận.

```bash
cd labs && cargo run -q -p lab-distributed --bin watermark -- 42    # exit 0
```

Output (macOS aarch64, rustc 1.95.0; xác định theo seed; mô phỏng chỉ dùng số nguyên):

```text
== Lab h: event time vs processing time, watermark (seed 42) ==
E=2000 events (1 per ~7 ms, 4 keys), tumbling W=1000 ms, delay: 85% U[0,40) 12% U[40,400) 3% U[1000,3000) ms
observed max delay = 2987 ms; events whose arrival window != event window: 168
processing-time windows wrong: 15 of 17 windows (no late events, latency 0)

-- per-window counts (excerpt: windows starting at 2000..6000 ms) --
   start  truth   proc   ev D=0   ev D=100   ev D=max
    2000    142    142      133        136        142
    3000    144    144      137        137        144
    4000    142    135      130        136        142
    5000    143    147      133        135        143
    6000    143    140      131        136        143

-- trade-off: watermark delay D and allowed lateness L (ms; latency over windows ending <= 10000) --
    D      L |  late  drop wrong refire | 1st mean 1st max fin mean fin max flush
    0      - |   133   133    13      0 |       23      42       23      42     1
   50      - |   109   109    13      0 |       66      81       66      81     1
  200      - |    83    83    13      0 |      219     231      219     231     1
  500      - |    63    63    13      0 |      521     542      521     542     1
 1000      - |    39    39    10      0 |     1021    1042     1021    1042     2
 2987      - |     0     0     0      0 |     3003    3017     3003    3017     3
    0    500 |   133    63    13     70 |       23      42      227     385     1
    0   2987 |   133     0     0    133 |       23      42     2255    2941     1
  200   2987 |    83     0     0     83 |      219     231     2255    2941     1

-- idle source: 2 partitions, partition 1 silent for ts in [4000,9000) then drained --
no idle timeout    windows fired before t=9000:  3 | wm lag at t=8999:  5005 ms | drops   0 | wrong 0/14
idle timeout 500ms windows fired before t=9000:  8 | wm lag at t=8999:    50 ms | drops 263 | wrong 4/14

-- hopping windows (size 5000, slide 1000): one event, many windows --
ts=7300 -> window starts [3000, 4000, 5000, 6000, 7000]
2000 events -> 10000 window memberships (size/slide = 5 each); tumbling: 1 each
```

Giải thích:

1. **Processing-time window sai gần hết.** 168 trong 2000 event nằm ở window khác lúc đến so với lúc xảy ra; 15 trên 17 window sai (17 = 14 window theo event time + 3 window chỉ tồn tại vì event đuôi nặng đến sau 14 000 ms). Cột `proc` ở bảng đầu: window 4000 mất 7 event, window 5000 thừa 4 — event không mất, chỉ chuyển sang window khác (test kiểm tổng luôn bằng `E`). Đó là lý do chương 11 tách hai miền thời gian.
2. **D = 0 (watermark nhanh).** 133 event (6,7%) tới khi window của chúng đã phát nên bị bỏ; 13 trên 14 window sai nhưng độ trễ kết quả chỉ ~23 ms. Đây chính là kiểu sai "quá nhanh: có dữ liệu trễ tới sau watermark"[^h-df-fast].
3. **Quét D.** Số event bị bỏ giảm đơn điệu theo D (133 → 109 → 83 → 63 → 39 → 0) và độ trễ lần phát đầu tăng gần như đúng bằng D (23, 66, 219, 521, 1021, 3003 ms): đó là trade-off "trì hoãn watermark quá nhiều làm trễ kết quả window"[^h-fl-wm-delay] mà Kafka Streams gọi là cân đối giữa latency, cost và correctness[^h-ks-tradeoff]. `wrong` đứng yên ở 13 đến D = 500 dù drop giảm một nửa: ~3% × 2000 = 60 event thuộc đuôi 1–3 s (kỳ vọng, không in ra) và mỗi event đó đủ làm hỏng một window. Chỉ `D = 2987` (= độ trễ lớn nhất **quan sát được** ở seed này) cho 0 drop, 0 window sai — với giá ~3 s độ trễ, và 3 window cuối chỉ phát khi hết input. Cột độ trễ chỉ tính window kết thúc trước 10 000 ms để không bị phần flush làm nhiễu (`LATENCY_END_LIMIT`).
4. **Allowed lateness.** Hàng `D=0, L=2987`: lần phát đầu vẫn nhanh (23 ms) nhưng sai; 133 lần re-fire sửa dần cho tới khi *kết quả cuối* của mọi window bằng ground truth (`wrong 0`, `drop 0`); đổi lại thời điểm có kết quả cuối là ~2255 ms trung bình, tối đa 2941 ms sau cuối window. `L = 500` chỉ cứu được 70 event (63 vẫn bị bỏ, vẫn 13 window sai). Hàng `D=200, L=2987` cho thấy hai núm xoay bù cho nhau: ít re-fire hơn (83) vì watermark chậm hơn, nhưng lần phát đầu trễ 219 ms. Kafka Streams có khái niệm tương tự là grace period — thời gian chờ record sai thứ tự, record tới sau đó bị loại[^h-ks-grace][^h-ks-grace2]. Một consumer phía sau phải chịu được việc cùng một window được phát nhiều lần (nhận xét của người soạn).
5. **Nguồn nhàn rỗi.** Ở phần này dùng độ trễ nhỏ (`DelayModel::SMALL`) và `D = 50` để khi không có sự cố thì kết quả chính xác. Partition 1 "mất mạng" với `ts ∈ [4000,9000)` rồi xả backlog sau t = 9000. Không có idle timeout, watermark = min bị partition 1 giữ lại: chỉ 3 window phát trước t = 9000, watermark tụt sau event mới nhất 5005 ms — "watermark bị giữ lại vì là min của các watermark song song"[^h-fl-idle] — nhưng khi backlog đến thì còn nằm trong window mở nên không mất event nào. Với `idle_timeout = 500 ms` watermark bám sát (trễ 50 ms = D), 8 window phát đúng lúc, **nhưng** 263 event trong backlog bị coi là trễ và bỏ, 4 window sai (suy luận: watermark ≈ 8950 nên các window 4000–7000 đã phát xong trước khi backlog đến). Đánh dấu idle là một cược rằng nguồn đó thật sự im lặng, không phải đang tạm nghẽn (nhận xét của người soạn).
6. **Hopping.** `hopping_starts(7300, 5000, 1000)` trả 5 window; 2000 event cho đúng 10 000 phần tử thành viên = 5 mỗi event. Hopping có cỡ + bước tiến và một record có thể thuộc nhiều window[^h-ksdsl-hop2]; tumbling là trường hợp cỡ bằng bước, mỗi record đúng một window[^h-ksdsl-tumb][^h-df-fixed-special]; Dataflow gọi tumbling là "fixed" và hopping là "sliding"[^h-df-fixed][^h-df-sliding].

Điều output **không** chứng minh: một seed không chứng minh gì về bất biến; các con số (133, 263, 2987…) là của seed 42 và của phân phối trễ do lab tự chọn, không phải đo hệ thống thật. Watermark `max − D` chỉ chạy đúng vì lab *biết* trần độ trễ; hệ thống thật thường thiếu thông tin để dựng watermark đúng 100%[^h-df-heur], watermark có thể bị một datum chậm giữ lại[^h-df-slow], và chỉ watermark thì chưa đủ[^h-df-insufficient] — chương 11 nói "không bao giờ đủ". Mô phỏng không dùng số thực (trung bình là chia nguyên), nên output không phụ thuộc nền tảng — chưa kiểm trên máy khác.

**Test** (`cargo test -p lab-distributed windows::`, 13 test trong module, vài giây ở debug — phụ thuộc máy): với `D ≥` độ trễ lớn nhất, event-time cho kết quả chính xác, 0 late, 0 drop — quét 120 seed × 3 mô hình trễ (đuôi nặng, nhỏ, không trễ) × 2 giá trị D; processing-time sai ở mọi seed có event vượt biên window (120 seed; lập luận: event chỉ dịch về sau nên window sớm nhất mất event không ai bù); drop không tăng và độ trễ không giảm theo D trên 11 mốc D × 60 seed; không có allowed lateness thì mỗi window phát đúng một lần (80 seed); với `L ≥` độ trễ lớn nhất kết quả *cuối* bằng ground truth nhưng kết quả đầu thì không (80 seed), `L = 100` vẫn sai; watermark đơn điệu và (một partition) đúng bằng `max_ts − D` ở mọi event, kể cả với hai partition và idle timeout; khi một partition im lặng, số window phát trước lúc xả backlog ≤ 4 và watermark tụt sau event mới nhất > 4000 ms (không mất event nào); idle timeout nhả watermark (≥ 4 window phát thêm, tụt < 300 ms) nhưng làm rơi backlog (60 seed); tumbling phủ kín trục thời gian không chồng không hở; hopping gán đúng `size/slide` window; event time không phụ thuộc mô hình trễ (cùng seed, cùng `ts`); một test thủ công 3 event (`ts` 1, 12, 8) ghim đúng số: drop → count 1; `D=4` → 2; `L=5` → re-fire count 2; `L=2` → bị bỏ.

### Lab (i) — `redis_loop`: một luồng, ranh giới nguyên tử, replication bất đồng bộ và `WAIT`

Lý thuyết: [chương 16 mục 1](../12-he-thong-du-lieu/16-redis-ben-trong.md) (một event loop, lệnh chậm chặn tất cả; `KEYS`, `DEL` so với `UNLINK`, `SCAN`), [mục 5](../12-he-thong-du-lieu/16-redis-ben-trong.md) (replication bất đồng bộ, `WAIT` không phải CP, chọn replica của Sentinel), [mục 7](../12-he-thong-du-lieu/16-redis-ben-trong.md) (`MULTI`/`EXEC`, `WATCH`, script), [chương 17 mục 0](../12-he-thong-du-lieu/17-redis-patterns.md) (chọn ranh giới nguyên tử) và [mục 8.1](../12-he-thong-du-lieu/17-redis-patterns.md) (chọn công cụ nguyên tử).

**Mô hình** (`labs/crates/lab-distributed/src/redis_loop.rs`): một *mô hình* của các hành vi mà tài liệu Redis mô tả, **không phải** Redis và không cài giao thức RESP. Thời gian là số nguyên µs ảo. Server là một thread: lấy request có `arrival` sớm nhất, `start = max(finish trước, arrival)`, `finish = start + chi phí`. Mỗi lệnh chạy trọn vẹn rồi mới tới lệnh sau, đúng như tài liệu nói Redis phục vụ request tuần tự[^i-lat-seq]. Bảng chi phí là **tham số tự đặt** (in ở đầu output); riêng `KEYS` được hiệu chỉnh theo con số trong tài liệu: một keyspace 1 triệu key quét hết trong 40 ms trên laptop cấu hình thấp[^i-keys-40ms], tức 40 ns/key — hằng số này tái tạo con số đó, nó không phải phép đo của chúng ta. Ba kịch bản:

1. **Head-of-line blocking.** 50 000 `GET`/s (đến ngẫu nhiên theo seed, vòng hở: không ai chờ reply) trong 1 s ảo, keyspace 1 triệu key. Tại t = 300 ms chen vào một sự kiện chậm: `KEYS *`; hoặc đi hết keyspace bằng `SCAN COUNT 1000` (1000 lệnh, mỗi lệnh cách nhau một RTT ngẫu nhiên 100–300 µs, xen kẽ với các `GET`); hoặc `DEL big` / `UNLINK big` trên một collection 1 triệu phần tử. Mọi biến thể của một seed thấy đúng cùng một chuỗi `GET`.
2. **Ranh giới nguyên tử.** 8 client × 1000 lần cộng 1 vào một counter; giữa hai request liên tiếp của một client là một RTT ngẫu nhiên nên request của các client xen kẽ nhau. Bốn cách: `GET` rồi `SET v+1` (hai request); `INCR`; `WATCH x; GET x; MULTI; SET x v+1; EXEC` và thử lại khi `EXEC` bị hủy; một `EVAL` đọc-cộng-ghi trong một request.
3. **Replication bất đồng bộ + `WAIT`.** Master và hai replica: A (đường truyền nhanh, 100–500 µs) và B (chậm, 1–6 ms nhưng `replica-priority` tốt hơn: 10 so với 100). Luồng ghi có thứ tự theo offset nên replica không áp dụng ghi *i* trước ghi *i−1*. Master crash tại thời điểm ngẫu nhiên trong 20–80 ms, rồi một replica được thăng theo `ByOffset` (offset cao nhất) hoặc `ByPriorityThenOffset` (priority thấp trước, rồi offset cao — thứ tự mà tài liệu Sentinel mô tả[^i-sent-order]). Đếm các ghi mà client **đã được báo xong** nhưng master mới không có. "Báo xong" nghĩa là: không `WAIT` → ngay khi master trả lời `SET`; `WAIT k` → khi k replica đã báo đã áp dụng offset đó[^i-wait-impl] (client chỉ ghi tiếp sau khi được báo xong). Mỗi dòng bảng là 2000 lần crash từ cùng bộ trễ ngẫu nhiên.

**Mô hình không có**: mạng/TCP thật, RESP, persistence (AOF/RDB), expire, I/O thread, thread nền thật (chỉ *quy ước* `UNLINK` không tính vào vòng lặp khi value lớn), phát hiện lỗi và bầu Sentinel (crash → thăng ngay, không quorum, không epoch), việc Sentinel loại replica đã mất kết nối quá lâu[^i-sent-select], replica chết hay đứt kết nối ngẫu nhiên (trừ ca "A chết cùng master" ở cuối bảng 3), và `WAIT` có timeout. `SCAN` ở đây là con trỏ vị trí trên keyspace đóng băng; `SCAN` thật hứa hẹn khác và có thể trả trùng[^i-scan-dup]. Số µs là ảo; mô hình chỉ tin được ở *hình dạng* (thứ tự lớn, đơn điệu), không ở con số tuyệt đối.

```bash
cd labs && cargo run -q -p lab-distributed --bin redis_loop -- 42    # exit 0
```

Output (macOS aarch64, rustc 1.95.0; xác định theo seed; chỉ số nguyên, không dùng float; mất khoảng 2 s ở build debug):

```text
Lab (i) redis_loop - seed 42
== Cost model (invented parameters, virtual us on the one server thread) ==
GET/SET/INCR 2  EVAL 4  MULTI/WATCH/queued 1  EXEC 1+queued
KEYS 40 us/1000 keys  SCAN 5+40 us/1000 keys  DEL 2+100 us/1000 elems  UNLINK 2 (> 64 elems)

== 1. Head-of-line blocking: 50000 GET/s for 1000 ms, slow event at 300 ms, 1000000 keys, big=1000000 ==
slow event       longest us  p50 us   p99 us    max us   GET>1ms
no slow event             0       2        4         8         0
KEYS *                40000       2    31773     39994      2215
SCAN COUNT 1000          45       2       38        49         0
DEL big              100002       2    91775     99996      5468
UNLINK big                2       2        4         8         0

== 2. Atomic boundary: 8 clients x 1000 increments of one counter, RTT 100-300 us ==
variant                    expected   final   lost   aborts  requests
GET + SET (2 requests)         8000    1634   6366        0     16000
INCR                           8000    8000      0        0      8000
WATCH/MULTI/EXEC + retry       8000    8000      0    47660    278300
EVAL script (1 request)        8000    8000      0        0      8000

== 3. Async replication: acked writes missing on the new master, 2000 crashes per row ==
replica priority: A=100 (fast link), B=10 (lagging link; lower number wins)
policy                WAIT     acked    lost  runs with loss
ByOffset              none    199925    1231            1159
ByOffset              1       107754       0               0
ByOffset              2        23452       0               0
ByPriorityThenOffset  none    199925   17803            2000
ByPriorityThenOffset  1       107754    7997            1998
ByPriorityThenOffset  2        23452       0               0
same, but replica A dies together with the master (only B can be promoted):
only B                none    199925   17803            2000
only B                1       107754    7997            1998
only B                2        23452       0               0
```

Giải thích:

1. **Lệnh chậm chặn tất cả (bảng 1).** Với tải ≈ 2 µs mỗi `GET` ở 50 000/s, server rảnh ~90% thời gian nên baseline p99 chỉ 4 µs. `KEYS *` giữ thread 40 000 µs; trong 40 ms có ~2000 `GET` đến và phải chờ, cộng ~200 `GET` đến trong ~4 ms server cần để xả hàng đợi sau đó (~2000 × 2 µs), nên 2215 `GET` > 1 ms, max 39 994 µs ≈ trọn thời gian của lệnh. Đây chính là hệ quả mà tài liệu nêu: một request chậm thì mọi client khác chờ[^i-lat-slow], và tài liệu cảnh báo dùng `KEYS` ở production "cực kỳ cẩn thận"[^i-keys-warn]. `DEL big` (100 002 µs) tệ hơn vì giải phóng 1 triệu phần tử tốn thời gian theo số phần tử[^i-del-cx]. Cột p99 cho thấy tail: ~4,4% `GET` bị ảnh hưởng nên p50 vẫn 2 µs nhưng p99 là 31 773 µs — *trung vị không nói gì về lệnh chậm* (nhận xét của người soạn).
2. **`SCAN` và `UNLINK` giữ vòng lặp ngắn.** `SCAN COUNT 1000` chia cùng 1 triệu key thành 1000 lệnh ≤ 45 µs; `COUNT` là lượng việc mỗi lần gọi[^i-scan-count], và tài liệu nói `SCAN` dùng được ở production mà không có nhược điểm chặn của `KEYS`[^i-scan-prod]. Cái giá không nằm trong bảng: client phải gọi 1000 lần và tự xử lý kết quả có thể trùng. `UNLINK big` chỉ tốn 2 µs trong vòng lặp vì mô hình coi việc thu hồi là việc nền, đúng ý tài liệu: thu hồi bộ nhớ ở thread khác nên không chặn[^i-unlink-doc]; ngoại lệ đã ghi ở chương 16: object nhỏ vẫn giải phóng đồng bộ[^i-lazyfree] — mô hình dùng ngưỡng 64 phần tử và test kiểm cả hai phía. (Nhận xét của người soạn: tải 50 000/s tự đặt, nên "GET > 1 ms" chỉ cho biết *bao nhiêu* request bị kẹt, không phải SLO của ai.)
3. **Ranh giới nguyên tử (bảng 2).** `GET` + `SET`: counter cuối chỉ 1634 trên 8000, mất 6366 cập nhật — hai client cùng đọc *v* rồi cùng ghi *v+1*. Không có lệnh nào sai: mỗi lệnh nguyên tử, nhưng *cặp* lệnh thì không, vì giữa hai request của một client server còn phục vụ client khác. `INCR` đúng 8000 vì cả đọc-cộng-ghi là một lệnh trong thread duy nhất; tài liệu gọi `INCR` là phép tăng nguyên tử[^i-incr-atomic]. Script cũng đúng 8000: Redis bảo đảm script chạy nguyên tử, mọi hoạt động server bị chặn suốt lúc chạy[^i-sc-atomic].
4. **`WATCH` đúng nhưng đắt (bảng 2).** `WATCH`/`MULTI`/`EXEC` cũng đúng 8000, nhưng có 47 660 lần `EXEC` bị hủy: 278 300 request cho 8000 lần cộng (= 5 × (8000 + 47 660)). Mỗi lần thử gồm 5 round trip (`WATCH`, `GET`, `MULTI`, `SET`, `EXEC`), và một transaction chỉ chạy nếu key được watch chưa đổi; nếu đổi thì `EXEC` hủy và client thử lại — vòng lặp lạc quan[^i-tx-watch][^i-tx-watch-retry]. Với 8 client tranh một key, mỗi lần commit kéo theo trung bình ~6 lần hủy. Kết luận (nhận xét của người soạn, chỉ trong mô hình này): tranh chấp cao thì `INCR` hoặc script rẻ hơn rất nhiều — khớp với nhánh "tranh chấp cao → Lua" ở [chương 17 mục 8.1](../12-he-thong-du-lieu/17-redis-patterns.md).
5. **`MULTI`/`EXEC` thuần không thay được `GET`+`SET`.** Trong `MULTI`, lệnh chỉ được xếp hàng tới `EXEC`[^i-tx-usage] nên `GET` trả `QUEUED` chứ không trả giá trị, và `SET` không thể dùng *v+1* tính từ nó (suy luận của người soạn; test `a_get_whose_result_decides_the_set_cannot_be_queued_in_multi` ghim hành vi đó trong mô hình). `MULTI` đảm bảo các lệnh được tuần tự hóa, không request nào chen vào giữa[^i-tx-serial], nhưng không rollback: lệnh lỗi lúc chạy thì các lệnh khác vẫn chạy[^i-tx-runtime-err][^i-tx-no-rollback] — test `multi_queues_commands_and_exec_runs_them_back_to_back_without_rollback` cho `INCR` vào key sai kiểu giữa hai `INCR` hợp lệ.
6. **Replication không `WAIT` mất ghi đã báo xong (bảng 3).** Replication mặc định bất đồng bộ[^i-repl-async-default], replica chỉ báo lại bất đồng bộ lượng dữ liệu đã xử lý[^i-repl-async] — master không chờ replica trước khi trả lời client (mô hình của lab). Không `WAIT`, `ByOffset` vẫn mất 1231 ghi trong 1159/2000 lần crash — những ghi nằm trong 100–500 µs cuối chưa tới cả A. `ByPriorityThenOffset` mất 17 803 ghi và mất ở **cả 2000** lần crash vì B (priority tốt hơn) luôn chậm vài ghi.
7. **`WAIT 1` + priority vẫn mất; `WAIT 1` + offset thì không.** `WAIT 1` được thỏa chỉ bởi A, nhưng `ByPriorityThenOffset` vẫn thăng B → 7997 ghi mất ở 1998/2000 lần. `ByOffset` thăng replica có offset cao nhất, mà replica đã báo "có" ghi *i* có offset ≥ *i* nên luôn ≥ mọi ghi đã báo xong → 0 mất. `WAIT 2` bắt cả hai replica báo nên 0 mất với mọi policy. Cột `acked` giảm (199 925 → 107 754 → 23 452) vì client chờ replica: đó là giá của `WAIT` (độ trễ mỗi ghi), và các hàng khác `WAIT` không cùng khối lượng việc nên chỉ so được cột `lost`/`runs with loss` *trên mỗi lần crash*.
8. **Đừng đọc "0" thành "an toàn".** Ca "A chết cùng master" (dòng `only B`): replica đã báo ack lại chính là replica không còn nữa, nên `WAIT 1` mất 7997 ghi, chỉ `WAIT 2` còn 0. Đó là một phản ví dụ cho "`ByOffset` + `WAIT 1` không bao giờ mất" — nó chỉ đúng khi mọi replica sống sót và báo offset trung thực. Tài liệu replication nói thẳng về `WAIT`: nó không biến Redis thành CP, ghi đã ack vẫn có thể mất khi failover[^i-repl-wait-cp], và đây chỉ là nỗ lực hết sức nên vẫn có thể mất một ghi đã đồng bộ tới nhiều replica[^i-wait-besteffort]; Redis + Sentinel hợp nhất theo "last failover wins" nên luôn còn cửa sổ mất ghi đã ack[^i-sent-lfw]. Mô hình không cài những chỗ làm điều đó xảy ra với `WAIT 2` (replica đứt kết nối giữa chừng, offset báo trễ, partition) — kết quả "0" là của mô hình, **không** phải bằng chứng cho Redis thật. Ngoài ra `WAIT` chỉ đảm bảo cho các ghi *của cùng kết nối*[^i-wait-conn]; mô hình chỉ có một client ghi.

Test (`cargo test -p lab-distributed redis_loop::`; 23 test của module, mỗi test quét seed hoặc ghim phản ví dụ): `GET` đến trong lúc lệnh chậm chạy có latency ≥ phần thời gian còn lại (3 seed × `KEYS`/`DEL`, mỗi lần > 1000 `GET` bị kẹt); p99 của `KEYS` > 100 × p99 của `SCAN` và max `GET` ≤ một batch + vài lệnh (3 seed); max `GET` khi `UNLINK` ≈ baseline còn `DEL` ≥ 90 ms; `UNLINK` value ≤ 64 phần tử giải phóng đồng bộ; `SCAN` đi đủ 1000 trang và con trỏ về 0; `GET`+`SET` mất cập nhật ở **cả 20/20 seed** và mỗi cập nhật mất ứng với ít nhất một cửa sổ GET…SET bị chen trong log; `INCR`/`WATCH`+retry/script đúng 8000 ở 8 seed; `WATCH` hủy > 0 khi 8 client và đúng 0 khi 1 client, với số request = 5 × (commit + hủy); log thao tác của mỗi `EXEC`/script liền nhau và không hai request nào chồng thời gian; `WAIT 2` mất 0 (5 seed × 400 crash × 2 policy); `ByOffset` + `WAIT 1` mất 0 (5 seed × 400); `ByPriority` + `WAIT 1` mất trong > 300/400 lần (5 seed) và một crash được ghim (seed 42, run 0: B được thăng, mất > 0, trong khi `ByOffset` thăng A và mất 0); không `WAIT` mất ở cả hai policy; phản ví dụ "A chết cùng master"; `WAIT` chặt hơn không bao giờ báo xong *nhiều* ghi hơn trên cùng một crash; báo cáo xác định theo seed.

### Lab (j) — `rate_limiter`: fixed window, sliding log, token bucket trên mô hình Redis

Lý thuyết: [chương 17 mục 2.1](../12-he-thong-du-lieu/17-redis-patterns.md) (fixed window, biên cửa sổ), [mục 2.2](../12-he-thong-du-lieu/17-redis-patterns.md) (sliding log), [mục 2.3](../12-he-thong-du-lieu/17-redis-patterns.md) (token bucket), [mục 2.4](../12-he-thong-du-lieu/17-redis-patterns.md) và [mục 0 (F1–F3)](../12-he-thong-du-lieu/17-redis-patterns.md) (hỏng ở đâu), [mục 8.1](../12-he-thong-du-lieu/17-redis-patterns.md) (chọn công cụ nguyên tử), [chương 16 mục 8.5](../12-he-thong-du-lieu/16-redis-ben-trong.md) (mẫu rate limiter của tài liệu Redis).

**Mô hình của lab** (module `limiter`): `MiniRedis` là một kho in-memory với đồng hồ ảo tính bằng ms; có số nguyên kèm TTL, sorted set, hash và `TIME`. Mỗi lệnh đơn là nguyên tử; `MiniRedis::exec` chạy một danh sách lệnh đã xếp hàng liền nhau (vai `MULTI`/`EXEC`: danh sách không thể rẽ nhánh theo kết quả[^j-tx-usage]); một script là một closure chiếm kho trong đúng một bước lập lịch. Tranh chấp giữa các client chạy routine *không* nguyên tử được tạo bởi một scheduler có seed, mỗi bước chọn ngẫu nhiên client nào chạy lệnh kế tiếp. Ba limiter: `FixedWindow` (hai biến thể: window id lấy từ đồng hồ pod `FixedAligned`, và cửa sổ bắt đầu từ request đầu `FixedTtl`), `SlidingLog`, `TokenBucket`. Tham số mặc định: $L=100$ request / $W=60$ s; token bucket nạp $L$ token mỗi $W$ và thử hai dung lượng `cap=20` (burst nhỏ) và `cap=100`. Bucket lưu token *nhân với `window_ms`* để nạp `limit` token mỗi `window_ms` là số nguyên chính xác, không làm tròn.

**Mô hình không có**: mạng và độ trễ lệnh (mọi lệnh tốn 0 thời gian ảo), replication/failover (F1), persistence, eviction, bộ nhớ tính bằng byte (chỉ đếm *phần tử*), Cluster/`CROSSSLOT` (F4). Vì vậy các con số dưới đây là hành vi của *mô hình*, không phải đo một Redis thật. Thao tác Redis được mô hình hóa theo mô tả của tài liệu: rate limiter là một counter đặc biệt[^j-rl-pattern], `INCR` và `EXPIRE` đặt trong `MULTI`/`EXEC` để mỗi lần gọi đều vừa tăng vừa đặt hạn[^j-rl-multi]; `EXPIRE` có option `NX` chỉ đặt hạn khi key chưa có hạn[^j-expire-nx] (option ghi ở mốc 7.0.0 trong lịch sử lệnh[^j-expire-nx-since]).

```bash
cd labs && cargo run -q -p lab-distributed --bin rate_limiter -- 42    # exit 0
```

Output (macOS aarch64, rustc 1.95.0; xác định theo seed — chạy hai lần `diff` giống hệt; logic dùng số nguyên, số thực chỉ để in trung bình nên chữ số thập phân cuối có thể khác trên nền tảng khác, chưa kiểm):

```text
== Lab (j) rate limiter model, seed 42 ==
limit L=100 per W=60 s; token bucket refills L tokens per W (cap 20 or 100)

(a) boundary burst: admitted in the 0.2 s [59.9 s, 60.1 s) when L requests arrive at 59.900..59.999 s
    and L more at 60.000..60.099 s (opener = one extra request at t=0 that starts the first window)
algorithm               no opener   opener@0
fixed (aligned id)            200        199
fixed (TTL, NX)               100        199
sliding log                   100        100
token bucket cap=20            20         20
token bucket cap=100          100        100

(b) steady overload: 1 request / 333 ms (about 3/s, 180 per minute) for 600 s, admitted:
  fixed (aligned id)      1000
  fixed (TTL, NX)         1000
  sliding log             1000
  token bucket cap=20     1019
  token bucket cap=100    1099

(c) max admitted in ANY 60 s window, 200 seeded bursty traces of 600 s
algorithm               bound   max    mean   traces>L
fixed (aligned id)        200   200   194.9        196
fixed (TTL, NX)           200   150   109.9         77
sliding log               100   100   100.0          0
token bucket cap=20       120    95    58.0          0
token bucket cap=100      200   199   183.4        200

(d) race: 8 clients x 5 requests at the same ms, sliding log L=20, 200 seeds
variant                          min   mean   max   seeds>L
3 separate commands               20   22.6    27       190
one script (atomic)               20   20.0    20         0

(d2) TTL leak: 1000 keys, 80 requests each at 1 per 15 s, L=5 per 60 s (a healthy client sends 4 per window),
     crash probability 0.02 per request
variant                             leaked  blocked  crashes
INCR; EXPIRE if count==1               306      286     1594
MULTI INCR; EXPIRE NX; EXEC              0        0     1594
     1 request / 10 s for 600 s, L=5: admitted with EXPIRE NX = 50, with plain EXPIRE = 5

(e) elements stored under the key(s) after 2L back-to-back requests
algorithm                 L=100  L=10000
fixed (TTL, NX)               1        1
sliding log                 100    10000
token bucket cap=20           2        2
```

Giải thích:

1. **(a) Biên cửa sổ.** Với window id lấy từ đồng hồ pod (`FixedAligned`, kiểu `rl:{k}:{floor(now/60)}` ở [chương 17 mục 2.1](../12-he-thong-du-lieu/17-redis-patterns.md)), $L$ request ở 59,900–59,999 s và $L$ request ở 60,000–60,099 s đều được nhận: **200** trong 0,2 s, đúng $2L$. Bản `FixedTtl` (key không có window id, cửa sổ bắt đầu từ request đầu) cho kết quả khác một cách thú vị: *không có opener* thì request đầu của burst A mở cửa sổ và burst B rơi vào *cùng* cửa sổ nên chỉ 100; muốn lách, kẻ gửi phải mở cửa sổ sớm bằng một request ở $t=0$ — request đó ăn một đơn vị ngân sách nên chỉ được $(L-1)+L=$ **199**. Cả hai đều là kết quả của mô hình lab (suy luận của người soạn: biên $2L$ là *cận trên* cho cả hai biến thể, 199 hay 200 tùy cách cửa sổ được neo). Sliding log luôn 100: các request ở burst B bị từ chối cho tới khi các mục của burst A rời cửa sổ $(now-W, now]$. Token bucket chỉ nhận đúng `cap` (20 hoặc 100): trong 0,2 s nạp được $100\cdot 200/60000\approx 0{,}33$ token, chưa đủ một token nguyên.
2. **(b) Quá tải đều.** 3 request/s (180/phút) là gần gấp đôi giới hạn (1.8×). Fixed và sliding nhận 1000 = 10 phút × 100. Token bucket nhận $cap + rate\cdot T$: `cap=20` cho 1019 (= 20 + 999), `cap=100` cho 1099 — tách "kích thước burst" (`cap`) khỏi "tốc độ bền vững" (`rate`) như [mục 2.3](../12-he-thong-du-lieu/17-redis-patterns.md) mô tả.
3. **(c) Cận trên trong *mọi* cửa sổ $W$.** Đo từ các timestamp *đã được nhận*: sliding $\le L$ (cột `max`=100, 0 trace vượt); token bucket $\le cap+rate\cdot W$ (cận 120 và 200); fixed $\le 2L$ vì một khoảng dài $W$ chạm tối đa hai cửa sổ cố định (suy luận của người soạn) và *thật sự vượt $L$*: `FixedAligned` chạm đúng 200 và vượt $L$ ở 196/200 trace, `FixedTtl` vượt ở 77/200 trace với `max`=150 trên các trace này. Điều đáng chú ý (suy luận của người soạn): `cap=100` với nạp $L$/phút *không* chặt hơn fixed window trong một cửa sổ $W$ (cận 200, đo được 199); muốn cận 120 phải hạ `cap`. Một bộ 200 trace không chứng minh cận — chứng minh nằm ở lập luận, test chỉ quét thêm 200 seed để tìm phản ví dụ.
4. **(d) Race giữa các client.** Ba lệnh rời `ZREMRANGEBYSCORE`, `ZCARD`, rồi `ZADD` nếu *số đếm đã đọc* nhỏ hơn $L$: tám client cùng đọc số đếm cũ rồi cùng thêm, nên 190/200 seed nhận nhiều hơn $L=20$ (tối đa 27). Đưa cả khối vào một script thì luôn đúng $L$ — script chạy nguyên tử vì mọi hoạt động khác của server bị chặn lúc nó chạy[^j-sc-atomic], và dùng `TIME` trong script là được[^j-sc-time-free] (`TIME` trả giờ server[^j-time-cmd]; mô hình dùng đồng hồ server cho sliding và bucket, chỉ `FixedAligned` dùng đồng hồ pod như F3). `MULTI`/`EXEC` *không* chữa được chỗ này vì lệnh chỉ được xếp hàng tới `EXEC`[^j-tx-usage], không đọc `ZCARD` rồi mới quyết định (suy luận của người soạn từ tài liệu đó; vì thế [mục 2.2](../12-he-thong-du-lieu/17-redis-patterns.md) dùng Lua). Phần tử ZSET phải duy nhất: `ZADD` với member đã có chỉ cập nhật score[^j-zset-update], nên code nhúng `req_id` vào member, và `ZREMRANGEBYSCORE` xóa theo khoảng score[^j-zrem-doc].
5. **(d2) Rò rỉ TTL.** Với `INCR` rồi `EXPIRE` nếu count bằng 1, client chết giữa hai lệnh để lại key không có TTL — đúng cảnh báo của tài liệu Redis[^j-rl-race], tài liệu chữa bằng script Lua[^j-rl-lua]. Trong mô hình, 2% request crash: **306/1000** key rò rỉ và 286 key chặn client lành mạnh (chỉ 4 request/cửa sổ, dưới giới hạn 5) *mãi mãi* trong mô hình, vì `INCR` để TTL nguyên vẹn[^j-expire-untouched] nên không gì dọn counter (suy luận của người soạn: theo chính pseudo-code của tài liệu, lần gặp lại IP chỉ `INCR` tới giá trị ≥ 2 nên `EXPIRE` không bao giờ chạy lại; câu "until we'll see the same IP address again" của tài liệu lạc quan hơn mô hình). Bản `MULTI`/`EXEC` + `EXPIRE NX`: 0 key rò vì mất kết nối trước `EXEC` thì không lệnh nào chạy[^j-tx-exec-all] (mô hình đặt crash *trước* `EXEC`, không giữa hai lệnh — đây là điều lấy từ tài liệu, lab không kiểm chứng riêng cho Redis thật). Dòng cuối so `NX` với `EXPIRE` thường: `EXPIRE` lên key đã có hạn *cập nhật* TTL[^j-expire-refresh], nên mỗi request đẩy hạn đi xa hơn, cửa sổ không bao giờ kết thúc dưới tải liên tục: 5 request được nhận trong cả 10 phút so với 50 của `NX` (đúng lập luận ở [mục 2.1](../12-he-thong-du-lieu/17-redis-patterns.md)).
6. **(e) Bộ nhớ.** Số *phần tử* (không phải byte): sliding log giữ $L$ phần tử mỗi key (100 → 10 000 khi $L$ tăng 100 lần), fixed window 1, token bucket 2 field của hash[^j-hash-def] bất kể $L$ (key tự biến mất sau `ceil(cap·W/limit)+1000` ms; test kiểm hành vi không đổi khi key hết hạn).
7. **Token bucket và đồng hồ lùi.** Test ghim các con số: nạp 1 token mỗi 600 ms với 100 token/phút; `max(0, now−ts)` khiến đồng hồ lùi 50 s không thêm token. Một hạn chế (suy luận của người soạn, kiểm bằng test `token_bucket_backward_step_is_recredited_when_clock_recovers`): script ở [mục 2.3](../12-he-thong-du-lieu/17-redis-patterns.md) ghi `ts = now` kể cả khi `now < ts`, nên khi đồng hồ phục hồi, khoảng thời gian bị lùi được nạp *lần thứ hai* (mô hình: lùi 6 s ⇒ thêm 10 token). Chương lý thuyết nêu `max(0, …)` nhưng không nêu hệ quả này; giữ `ts = max(ts, now)` là một cách tránh (nhận xét của người soạn, chưa kiểm).

**Test** (`cargo test -p lab-distributed limiter::`, 18 test, khoảng 1–2 s ở debug — phụ thuộc máy): burst biên ghim đúng 200/100/199/100/20/100; 200 trace seed cho mọi thuật toán (sliding $\le L$ trong mọi cửa sổ, token bucket $\le cap+rate\cdot W$, fixed $\le 2L$ và có trace vượt $L$, max fixed > $1{,}5L$); 200 seed race (script luôn đúng $L$; ba lệnh rời vượt $L$ ở > 150 seed và có seed nhận ít nhất $L+5$); crash injection (20 seed × 100 key: `MULTI` không rò, bản rời rò); key rò chặn vĩnh viễn dù cách 10 phút; `NX` giữ TTL đầu còn `EXPIRE` thường kéo dài; token bucket với TTL và bản tham chiếu không TTL cho cùng dãy quyết định trên 50 seed; skew đồng hồ pod chia đôi key `FixedAligned`.

### Lab (k) — `cache_stampede`: stampede, stale set, nhả khóa và negative caching

Lý thuyết: [chương 17 mục 1.1](../12-he-thong-du-lieu/17-redis-patterns.md) (cache-aside đọc/ghi, thứ tự "DB trước, xóa cache sau", cuộc đua stale set và lease token), [mục 1.3](../12-he-thong-du-lieu/17-redis-patterns.md) (negative caching), [mục 1.4](../12-he-thong-du-lieu/17-redis-patterns.md) (stampede, single-flight, jitter, XFetch), [mục 3.1–3.2](../12-he-thong-du-lieu/17-redis-patterns.md) (khóa `SET NX PX`, nhả bằng so token, vì sao vẫn cần fencing) và [chương 16 mục 8.1 và 8.4](../12-he-thong-du-lieu/16-redis-ben-trong.md) (khóa trên một instance; cache đứng trước PostgreSQL).

**Mô hình của lab** (`labs/crates/lab-distributed/src/cache.rs`) là mô hình trong bộ nhớ, thời gian ảo tính bằng mili-giây, **không phải** Redis hay memcached. Bốn phần độc lập:

* Phần 1, một key nóng: request đến đều đặn mỗi 100 ms (10 req/s, pha ngẫu nhiên theo seed), TTL 60 s, mỗi lần tính lại (tải từ DB) mất đúng 3 s, chạy 20 phút ảo. Bảy chính sách: naive (mỗi request trượt tự tính lại), single-flight (người thắng `SET key:load token NX PX` mới được tính lại, người thua ngủ 20/40/80/160 ms rồi đọc lại) với `lock_ttl` 10 s và 1 s, XFetch với β = 0.5, 1, 2, và XFetch cộng khóa. Một *wave* là một đợt tính lại bắt đầu khi chưa có lần tính nào đang chạy. Phần 1b: 500 key nạp cùng lúc, TTL 60 s có hoặc không jitter 0–10%.
* Phần 2, cuộc đua stale set, **duyệt vét cạn**: reader `r1` GET (trượt), `r2` đọc DB, `r3` SET; writer có hai bước theo thứ tự A (`w1` ghi DB, `w2` DEL cache) hoặc B (`w1` DEL, `w2` ghi DB). Có đúng C(5,2) = 10 cách xen kẽ cho mỗi thứ tự; chương trình chạy cả 10. "Stale" nghĩa là trạng thái *cuối* có cache giữ giá trị cũ trong khi DB giữ giá trị mới. Ba cấu hình: thường, lease token, SET có phiên bản (writer *SET* bản mới thay vì DEL; mọi SET bị từ chối nếu cache đã có bản mới hơn).
* Phần 3, nhả khóa: một key khóa với TTL 30 s, ba client A, B, C; hai cách nhả: `DEL` trần và "xóa chỉ khi giá trị còn là token của tôi".
* Phần 4, negative caching: bot dò id không tồn tại; và cái bẫy quên xóa bản ghi âm khi tạo dòng.

**Mô hình không có**: độ trễ mạng, mất tin, eviction, replication/failover, lệch đồng hồ (mọi bên dùng chung một đồng hồ ảo chính xác), sức chứa DB (một lần tải luôn mất đúng 3 s, không xếp hàng), request đến theo phân phối ngẫu nhiên (ở đây đều tăm tắp nên mọi con số "30" là chính xác, thật thì dao động), và cơ chế lease thật của memcached ngoài tính chất "bị `delete` thì token mất hiệu lực". Các số dưới đây **không** phải số đo của Redis hay memcached thật.

```bash
cd labs && cargo run -q -p lab-distributed --bin cache_stampede            # exit 0 (seed 42)
cd labs && cargo run -q -p lab-distributed --bin cache_stampede -- 42 all  # exit 0, in thêm cả 10 interleaving mỗi thứ tự
```

Output (macOS aarch64, rustc 1.95.0; xác định theo seed; phần trăm in từ `f64`, quyết định XFetch dùng `ln` của `f64`, chữ số thập phân cuối và hiếm khi cả đếm có thể khác trên nền tảng khác — chưa kiểm). Một lần chạy duy nhất, cắt thành bốn khối theo phần `[n]` cho dễ đọc:

```text
Lab (k) cache_stampede  seed=42  (model in virtual ms, not Redis)

[1] hot key: 10 req/s, ttl 60 s, recompute 3 s, 20 min of virtual time
policy                      recomp waves per_wave peak_db  miss% waited% wait_max
naive                          540    18     30.0      30   4.50    4.50    3000ms
single-flight ttl=10s           19    19      1.0       1   4.75    4.75    3130ms
single-flight ttl=1s<comp       54    18      3.0       3   4.50    4.50    3800ms
xfetch beta=0.5                 96    19      5.1      13   0.00    0.80    3000ms
xfetch beta=1                   69    22      3.1       8   0.00    0.57    3000ms
xfetch beta=2                   58    32      1.8       5   0.00    0.48    3000ms
xfetch beta=1 + lock 10s        22    22      1.0       1   0.00    0.18    3000ms
(naive, one cycle: 10 req/s x 3 s = 30 recomputes)

[1b] 500 keys loaded together, ttl 60 s, reload 500 ms, each key read every 100 ms
jitter 0.. 0% of ttl: peak concurrent reloads 500, reload starts spread over 99 ms
jitter 0..10% of ttl: peak concurrent reloads  53, reload starts spread over 5971 ms
```

```text
[2] stale set: reader r1=GET(miss) r2=SELECT r3=SET; every interleaving with the writer's w1 w2
A: w1=UPDATE db, w2=DEL cache     B: w1=DEL cache, w2=UPDATE db   (stale = cache old, db new)
stale interleavings of 10            plain    lease   versioned SET
A: db then DEL                           1        0               0
B: DEL then db                           6        2               0
  stale in A plain: r1 r2 w1 w2 r3
  stale in B lease: w1 r1 r2 r3 w2
  stale in B lease: w1 r1 r2 w2 r3
stale lifetime of r1 r2 w1 w2 r3 (reader's SET at 2 s, horizon 300 s):
  no ttl   plain: 298000 ms   lease: 0 ms
  ttl 60 s plain:  60000 ms   lease: 0 ms
  ttl 5 s  plain:   5000 ms   lease: 0 ms
```

```text
[3] lock ttl 30 s; A works 45 s, B 25 s, C 20 s; B and C poll every 1 s
-- release by plain DEL
     0.000s A: SET lock tokA NX PX 30000 -> OK (expires  30.000s)
    30.500s B: SET lock tokB NX PX 30000 -> OK (expires  60.500s)
    45.000s A: work done; DEL lock -> deleted B's lock!
    45.750s C: SET lock tokC NX PX 30000 -> OK (expires  75.750s)
    55.500s B: work done; DEL lock -> deleted C's lock!
    65.750s C: work done; DEL lock -> key already gone
   mutual-exclusion violations: 2 [A&B 30.500s..45.000s, B&C 45.750s..55.500s]; foreign deletes: 2
-- release by token compare
     0.000s A: SET lock tokA NX PX 30000 -> OK (expires  30.000s)
    30.500s B: SET lock tokB NX PX 30000 -> OK (expires  60.500s)
    45.000s A: work done; unlock script -> token is B's, no-op
    55.500s B: work done; unlock script -> token matches, deleted
    55.750s C: SET lock tokC NX PX 30000 -> OK (expires  85.750s)
    75.750s C: work done; unlock script -> token matches, deleted
   mutual-exclusion violations: 1 [A&B 30.500s..45.000s]; foreign deletes: 0
```

```text
[4] negative caching: bot probes 12000 non-existent ids at 200 req/s (60 s), neg ttl 30 s
id space      1000: db lookups 12000 -> 1975  (peak negative entries 998)
id space 100000000: db lookups 12000 -> 12000  (peak negative entries 6000)
row created at 5 s while a negative entry lives to 30 s; reads every 100 ms still returning 404: no DEL on create = 249, DEL on create = 0
```

Với `all` (chỉ phần thêm vào, nằm trong mục `[2]`):

```text
order A, plain, all 10 (S = stale):
  r1 r2 r3 w1 w2  cache=None db=1 
  r1 r2 w1 r3 w2  cache=None db=1 
  r1 w1 r2 r3 w2  cache=None db=1 
  w1 r1 r2 r3 w2  cache=None db=1 
  r1 r2 w1 w2 r3  cache=Some(0) db=1 S
  r1 w1 r2 w2 r3  cache=Some(1) db=1 
  w1 r1 r2 w2 r3  cache=Some(1) db=1 
  r1 w1 w2 r2 r3  cache=Some(1) db=1 
  w1 r1 w2 r2 r3  cache=Some(1) db=1 
  w1 w2 r1 r2 r3  cache=Some(1) db=1 
order B, plain, all 10 (S = stale):
  r1 r2 r3 w1 w2  cache=None db=1 
  r1 r2 w1 r3 w2  cache=Some(0) db=1 S
  r1 w1 r2 r3 w2  cache=Some(0) db=1 S
  w1 r1 r2 r3 w2  cache=Some(0) db=1 S
  r1 r2 w1 w2 r3  cache=Some(0) db=1 S
  r1 w1 r2 w2 r3  cache=Some(0) db=1 S
  w1 r1 r2 w2 r3  cache=Some(0) db=1 S
  r1 w1 w2 r2 r3  cache=Some(1) db=1 
  w1 r1 w2 r2 r3  cache=Some(1) db=1 
  w1 w2 r1 r2 r3  cache=Some(1) db=1 
```

Giải thích:

1. **Stampede, naive.** 540 lần tính lại = 18 wave × 30: mỗi wave đúng 30, `peak_db` = 30 (cả 30 lần tải còn đang chạy khi request cuối của cửa sổ đến). Đây là ví dụ của paper VLDB 2015: mục được đọc 10 lần/giây, tính lại 3 giây thì 30 request cùng tính lại[^k-xf-size]; định nghĩa stampede ở đó là nhiều request cùng thấy trượt và cùng tính lại đúng mục vừa hết hạn[^k-xf-def]. Dòng `(naive, one cycle…)` chạy riêng đúng một chu kỳ và cũng ra 30. Vì mô hình có request đều đặn nên 30 là *chính xác* (10 req/s × 3 s), không phải trung bình.
2. **Single-flight.** 19 lần tính lại cho 19 wave: `per_wave` = 1,0, `peak_db` = 1 — DB được bảo vệ. Nhưng người dùng thì không: vẫn ~4,75% request trượt và đợi, tối đa 3130 ms = 3000 ms tính lại + tối đa một lần ngủ cuối (≤ 160 ms + tối đa 80 ms jitter) của người thua. Single-flight bảo vệ *DB*, không bảo vệ độ trễ. Khóa dùng chính primitive của khóa: lấy bằng `SET … NX PX`[^k-lk-set], nhả bằng script so token[^k-lk-unlock]; biến thể phía memcached của Facebook điều tiết lease, mặc định một token mỗi 10 giây cho mỗi key[^k-fb-herd-rate], còn request đến trong lúc đó được bảo chờ một chút[^k-fb-herd-wait]. Khóa kiểu này là khóa "vì hiệu quả": Kleppmann khuyên thuật toán một node và ghi rõ trong code rằng khóa chỉ gần đúng[^k-kb-efficiency] — làm trùng một lần tính lại chỉ tốn tài nguyên.
3. **`lock_ttl` = 1 s < 3 s tính lại.** 54 lần tính lại cho 18 wave (3 mỗi wave), `peak_db` = 3: khóa hết hạn giữa chừng nên một người thua kế tiếp thắng lại, rồi lần nữa. TTL của khóa là "thời gian tự nhả" đồng thời là thời gian client có để làm xong việc mà không vi phạm loại trừ tương hỗ[^k-lk-window] — nên đặt lớn hơn thời gian tải tệ nhất (nhận xét của người soạn; test dùng `compute + 500 ms` thì đúng 1 lần).
4. **XFetch.** Điều kiện `Time() − ∆β log(rand()) ≥ expiry` của paper[^k-xf-formula] cho phép mỗi request tự quyết định, ngẫu nhiên, tính lại *trước* hạn[^k-xf-early]; paper nhắm tới hệ cache chỉ có `get`/`set`, không khóa[^k-xf-nolock]. Kết quả: `miss%` = 0,00 (không request nào thấy mục hết hạn), nhưng **không phải một lần tính lại mỗi wave**: β = 1 cho 69/22 ≈ 3,1, `peak_db` = 8. Lý do (suy luận của người soạn, khớp số đếm): khi lần tính lại sớm đang chạy, khoảng 30 request nữa đến và *mỗi cái* vẫn tự thử điều kiện trên mục cũ còn hiệu lực, vài cái trúng. β lớn hơn thì ít trùng hơn (β = 2: 1,8) nhưng số wave trong 20 phút tăng (32 so với 19 của β = 0.5) vì mục bị làm mới sớm hơn — đúng đánh đổi paper nêu: tăng β chặn stampede tốt hơn nếu việc hết hạn sớm không quan trọng[^k-xf-beta]. Hàng cuối (XFetch + khóa, ghép của người soạn, không phải của paper): đúng 1/wave. Một lựa chọn của mô hình: request kích hoạt tính lại sớm *tự chờ* 3 s (tính lại inline như pseudo-code của paper), nên `wait_max` vẫn 3000 ms với 0,18–0,80% request; làm mới ở nền thì request đó không chờ (nhận xét của người soạn).
5. **Jitter (1b).** Cùng TTL: cả 500 key tải lại trong 99 ms nên `peak_db` = 500 (mọi lần tải 500 ms chồng lên nhau). Với TTL cộng thêm 0–10% (nhận xét của người soạn; con số 0–10% chưa có nguồn ghim, như ghi ở chương 17): các key rải trên ~6 s, `peak_db` = 53. Mô hình giả định mỗi key tải lại đúng một lần (single-flight theo từng key).
6. **Stale set, vét cạn (mục `[2]`).** Thứ tự A cho **đúng 1/10** interleaving stale: `r1 r2 w1 w2 r3` — reader đọc DB *trước* khi writer ghi, rồi SET giá trị cũ *sau* khi writer đã DEL. Đây là đúng cuộc đua chương 17 mô tả, và Facebook định nghĩa stale set là web server đặt vào cache giá trị không phản ánh giá trị mới nhất khi các cập nhật đồng thời bị đảo thứ tự[^k-fb-stale]. Ở danh sách `all`, `r1 r2 w1 r3 w2` có cache rỗng vì cuối cùng `w2` (DEL) xóa nốt bản cũ vừa SET: *có* một khoảng ngắn cache giữ giá trị cũ, nhưng mô hình chỉ đếm trạng thái cuối — Azure cũng ghi giữa lúc ghi và lần đọc kế tiếp reader có thể thấy dữ liệu cũ thoáng chốc[^k-az-stale]. Thứ tự B (DEL trước, ghi DB sau) cho **6/10** stale: đúng khi `r2 < w2` (reader đọc trước khi DB đổi) và `w1 < r3` (SET của reader nằm sau DEL). Đó là lý do tài liệu Azure dặn cập nhật data store *trước* khi xóa mục cache[^k-az-order]. Test đối chiếu cả hai con số với điều kiện vị trí tính tay độc lập với máy trạng thái.
7. **Lease token.** Lease là token 64-bit gắn với key; client trả nó khi SET, và memcached từ chối nếu đã vô hiệu hóa token do nhận `delete` cho mục đó[^k-fb-lease]. Mô hình: miss cấp token, `DEL` hủy token, SET với token hỏng bị từ chối, còn *ghi DB đơn thuần không đụng tới lease*. Kết quả: thứ tự A còn **0/10** stale — nhưng thứ tự B vẫn còn **2/10** (`w1 r1 r2 r3 w2` và `w1 r1 r2 w2 r3`): reader xin lease *sau* DEL, đọc DB cũ, rồi writer mới ghi DB và không còn `delete` nào hủy lease. Bài học (suy luận của người soạn từ mô hình này): lease vá cuộc đua ở thứ tự đúng, không cứu được thứ tự sai. **SET có phiên bản** (writer SET bản mới, SET cũ bị từ chối) cho 0/10 ở cả hai thứ tự — nhưng ở thứ tự B con số 0 này **không** phải một cách sửa thật: writer chạy bước SET vào cache *trước* khi ghi DB, nên có 3 trong 10 interleaving (`w1 r1 w2 r2 r3`, `w1 r1 r2 w2 r3`, `w1 r1 r2 r3 w2`) mà reader đọc được giá trị mới khi DB còn giá trị cũ, tức một giá trị chưa commit (nếu lần ghi DB thất bại, giá trị đó ở lại trong cache) — oracle của lab chỉ đếm trạng thái cache giữ giá trị cũ khi DB đã giá trị mới nên không thấy trạng thái "cache chạy trước DB" này (kiểm bằng cách đọc interleaving; lab chưa đếm nó); trong mô hình, giá trị chính là phiên bản, và phiên bản không bị mất vì cache không có expiry/eviction ở phần vét cạn — trên Redis thật phải giữ phiên bản trong key và phiên bản biến mất theo key (nhận xét của người soạn).
8. **TTL chặn đời sống của giá trị cũ.** Kịch bản `r1 r2 w1 w2 r3` với reader chậm SET lúc 2 s: không TTL → cũ tới hết horizon (298000 ms); TTL 60 s → đúng 60000 ms; TTL 5 s → 5000 ms; lease → 0. Cửa sổ bị chặn bởi TTL *của lần SET cũ* (đếm từ lúc SET, không từ lúc ghi DB). Azure dặn chọn chính sách hết hạn khớp với kiểu truy cập dữ liệu[^k-az-ttl]; TTL là lưới an toàn, không phải cách sửa (nhận xét của người soạn).
9. **Nhả khóa (mục `[3]`).** A giữ khóa 30 s nhưng làm 45 s: lúc 30,5 s B thắng khóa. Cả hai cách nhả đều có vi phạm đầu tiên **A&B** (30,5–45 s): đó là việc TTL hết hạn trước khi làm xong, chỉ chặn được bằng fencing token ở tài nguyên đích[^k-lk-disclaimer-fence] — xem lab (d) `fencing`. Cách nhả `DEL` trần thêm vi phạm thứ hai: lúc 45 s A `DEL` xóa mất khóa của B (client có thể xóa khóa của client khác[^k-lk-del-unsafe]), C vào lúc 45,75 s trong khi B còn làm tới 55,5 s ⇒ **B&C**; rồi `DEL` của B lại xóa khóa của C (`foreign deletes: 2`) — thiệt hại lan truyền. Nhả bằng script so token (xóa khi và chỉ khi giá trị còn đúng là giá trị client chờ[^k-lk-unlock]) làm lần nhả của A thành no-op và C chỉ vào sau khi B nhả (55,75 s): 1 vi phạm thay vì 2. Chú ý: "token-compare không vi phạm" là sai — nó chỉ loại vi phạm *do cách nhả*, không loại vi phạm do TTL.
10. **Negative caching (mục `[4]`).** RFC 2308 định nghĩa negative caching (cho DNS) là lưu tri thức rằng một thứ không tồn tại[^k-rfc2308-def] và nó có ích vì giảm thời gian phản hồi cho câu trả lời âm[^k-rfc2308-use]; áp dụng cho id sản phẩm là suy luận của người soạn. Bot dò 1000 id lặp lại: 12000 lần tra DB → 1975 (TTL 30 s). Nhưng nếu bot quét một không gian id khổng lồ (10^8) thì mỗi id chỉ hỏi một lần: vẫn 12000 lần tra DB, và cache còn phải giữ 6000 key âm — negative caching không giúp gì và ăn bộ nhớ; chương 17 dặn đi cùng giới hạn tần suất hoặc Bloom filter. Cái bẫy: tạo dòng lúc 5 s mà không xóa bản ghi âm thì 249 lần đọc (mỗi 100 ms, tới 29,9 s) vẫn trả 404; có `DEL` khi tạo thì 0.
11. **Test** (`cargo test -p lab-distributed cache::`, 19 test; thời gian < 1 s trên máy người soạn, phụ thuộc máy): naive đúng 30 lần tính lại/peak 30 trên 100 seed (và đúng `compute/interval` với ba cặp khác); single-flight đúng 1 lần/wave, mọi request hoàn tất, `wait_max ≤ 3240 ms` trên 100 seed; `lock_ttl` 1 s cho ≥ 2 lần tính lại trên 50 seed, `compute + 500 ms` cho đúng 1; XFetch trên 40 seed: 1–5 lần/wave, miss < 0,1%, XFetch + khóa đúng 1/wave; số wave tăng theo β chỉ được kiểm ở seed 3 và 1–5 lần/wave là con số *tổng* trên 40 seed, không phải từng seed; jitter hạ `peak_db` xuống dưới 1/5 trên 50 seed; 10 interleaving đúng thứ tự chương trình; đếm stale A=1, B=6, lease A=0/B=2, versioned 0/0, mỗi cấu hình được đối chiếu với vị từ tính tay; "một fix không bao giờ thêm outcome stale"; cửa sổ stale = TTL; nhả khóa: quét 500 seed, với token-compare mọi cặp chồng lấn đều có holder trước làm lâu hơn TTL, còn với `DEL` trần có trường hợp holder làm xong trong TTL vẫn bị chồng lấn; negative caching ≥ 4× ít lần tra DB khi id lặp lại, còn khi quét id không lặp thì ≥ 99% lượt tra vẫn còn. Một seed không chứng minh gì; các bất biến trên được quét nhiều seed.

### Trên Redis thật (chưa chạy)

Các lệnh dưới đây là phần **chưa chạy**: chúng cần một `redis-server` thật (và `redis-cli`), mà người soạn **không** chạy trong lab này nên không có output nào được dán ở đây. Mỗi đoạn "kỳ vọng" chỉ diễn đạt lại điều tài liệu đã cho và các số lab in ở trên; nếu Redis thật khác thì đó là thông tin để bạn đối chiếu lại mô hình, không phải lỗi của tài liệu hay của Redis (nhận xét của người soạn). Chỉ chạy trên instance thử nghiệm của riêng bạn, không chạy trên instance dùng chung.

#### Lab (c) — `CLUSTER KEYSLOT`

**Chưa chạy** (cần một `redis-server` thật; người soạn không chạy Redis). Docs Redis mô tả `CLUSTER KEYSLOT` là lệnh trả về hash slot của một key[^c-keyslot-desc]. Lệnh chỉ chạy trên instance bật chế độ cluster: source 7.2.5 trả lỗi "This instance has cluster support disabled" khi `cluster_enabled == 0`[^c-cluster-disabled], nên với instance thử nghiệm của riêng bạn hãy khởi động với `--cluster-enabled yes` (nhận xét của người soạn). Rồi so với các slot lab in ở output (2):

```bash
redis-cli CLUSTER KEYSLOT somekey
redis-cli CLUSTER KEYSLOT 'foo{hash_tag}'
redis-cli CLUSTER KEYSLOT 'bar{hash_tag}'
redis-cli CLUSTER KEYSLOT '{user1000}.following'
redis-cli CLUSTER KEYSLOT 'foo{}{bar}'
```

Kỳ vọng: khớp với số lab in — `11058`, `2515`, `2515` (ba số này lấy từ ví dụ trong docs[^c-keyslot-1][^c-keyslot-2]), `3443` và `8363` (hai số này chỉ là output của lab; chưa chạy, chưa kiểm chứng). Nếu khác, lỗi nằm ở code lab, không phải ở Redis (nhận xét của người soạn).

#### Lab (i) — lệnh chậm, `SLOWLOG` và `WAIT`

**Chưa chạy** (các lệnh dưới đây cần một `redis-server` thật; người soạn **không** chạy chúng, nên không có output nào ở đây; phần "kỳ vọng" chỉ diễn đạt lại tài liệu, không phải kết quả quan sát). Chỉ làm trên instance thử nghiệm của riêng bạn. Lưu ý về (1): `redis.conf` của Redis 7.2.5 ghi `enable-debug-command no`[^i-debug-default], và mô tả giá trị `local` là chỉ cho kết nối cục bộ[^i-debug-local]; trên server mặc định lệnh `DEBUG` bị từ chối nên (1) không chặn gì cả; hãy khởi động một instance thử nghiệm với `redis-server --port 6380 --enable-debug-command local` rồi thêm `-p 6380` vào mọi lệnh `redis-cli` dưới đây (nhận xét của người soạn).

```bash
# (1) Lệnh chậm chặn client khác. Terminal A rồi terminal B ngay sau đó:
redis-cli -p 6380 DEBUG SLEEP 1
time redis-cli -p 6380 PING

# (2) SLOWLOG: ghi lại lệnh vượt ngưỡng, cần một DB đủ lớn để KEYS đáng kể:
redis-cli -p 6380 CONFIG SET slowlog-log-slower-than 10000
redis-cli -p 6380 KEYS '*' > /dev/null
redis-cli -p 6380 SLOWLOG GET 1

# (3) Offset và WAIT: WAIT chỉ tính ghi của CÙNG kết nối nên dùng một kết nối duy nhất:
redis-cli -p 6380 INFO replication
printf 'SET k v\nWAIT 1 100\n' | redis-cli -p 6380
```

Kỳ vọng theo tài liệu (không phải output): `DEBUG SLEEP` là lệnh debug; tài liệu Sentinel dùng nó để mô phỏng một master bị treo[^i-debug-sleep] — nên ở (1) `PING` của terminal B phải chờ tới khi lệnh ngủ kết thúc (nhận xét của người soạn: đừng chạy trên instance dùng chung). Ở (2), ngưỡng `10000` là µs (10 ms)[^i-slowlog-unit] và `SLOWLOG` ghi thời gian thực thi lệnh *không* gồm I/O với client — "chặn" ở đây là khoảng thread chính không phục vụ ai khác[^i-slowlog-blocked][^i-slowlog]. Ở (3), `INFO replication` có trường `master_repl_offset`[^i-info-offset]; `WAIT 1 100` trả số replica đã nhận các ghi trước đó *của kết nối này*[^i-wait-conn], nên với một master không có replica kết nối bạn nên chờ số 0 (suy luận của người soạn từ định nghĩa trên). So kết quả thật của máy bạn với bảng 1 và 3 chỉ để thấy *cùng hình dạng*, không phải cùng con số.

#### Lab (j) — fixed window và token bucket

**Chưa chạy** (cần `redis-server`/`redis-cli` thật; người soạn không chạy chúng trong lab này): chỉ là điểm bắt đầu để bạn đối chiếu mô hình với Redis thật. Kỳ vọng được suy từ tài liệu, không phải từ quan sát.

Fixed window (một connection để `MULTI`/`EXEC` dùng chung):

```bash
redis-cli <<'EOF'
MULTI
INCR rl:{u1}
EXPIRE rl:{u1} 60 NX
EXEC
PTTL rl:{u1}
EOF
```

Kỳ vọng (chưa kiểm): lần đầu key chưa có hạn nên `NX` cho phép đặt TTL[^j-expire-nx], các lần sau `NX` không đặt lại nên `PTTL` giảm dần thay vì quay về 60 000 (so với `EXPIRE` không `NX`, vốn cập nhật TTL[^j-expire-refresh]). Nếu server cũ hơn 7.0, option `NX` có thể chưa tồn tại[^j-expire-nx-since].

Token bucket: lưu script ở [chương 17 mục 2.3](../12-he-thong-du-lieu/17-redis-patterns.md) vào `token_bucket.lua` rồi gọi (`cap=20`, nạp $100/60\approx1{,}6667$ token/s, `cost=1`):

```bash
for i in $(seq 21); do redis-cli EVAL "$(cat token_bucket.lua)" 1 'tb:{u1}' 20 1.6666667 1; done
```

Kỳ vọng (chưa kiểm; dùng key `tb:{u1}` khác `rl:{u1}` của đoạn fixed window vì key đó là một string đếm và script này đọc hash, nên dùng chung sẽ lỗi `WRONGTYPE`): nếu cả 21 lần gọi xong trong chưa đầy ~600 ms (mỗi token nạp lại sau 1/1.6667 s), vì bucket bắt đầu đầy, 20 lần đầu trả `allowed=1`, lần thứ 21 trả `allowed=0` kèm `retry` khoảng 600 ms — theo cách script ở chương 17 được viết; script đọc `TIME` bên trong, điều tài liệu cho phép khi effects replication được bật[^j-sc-time-free].

#### Lab (k) — khóa `SET NX PX` và nhả khóa

**Chưa chạy** (người soạn không chạy Redis trên máy này): cùng kịch bản khóa của mục `[3]` trong output trên một Redis thật, để đối chiếu với mô hình. Mỗi lệnh phải bám đúng hành vi mà tài liệu mô tả: `SET … NX PX` đặt khóa chỉ khi chưa tồn tại và có hạn[^k-lk-set]; script nhả chỉ xóa khi giá trị đúng là token mong đợi[^k-lk-unlock]; `PTTL` trả thời gian sống còn lại tính bằng mili-giây[^k-pttl].

```bash
# chưa chạy: client A lấy khóa 30 s
redis-cli SET lock:x tokA NX PX 30000
# chưa chạy: xem thời gian còn lại (ms)
redis-cli PTTL lock:x
# chưa chạy: sau 30 s khóa tự mất; client B lấy
redis-cli SET lock:x tokB NX PX 30000
# chưa chạy: A nhả bằng DEL trần (xóa khóa của B) — cách SAI
redis-cli DEL lock:x
# chưa chạy: A nhả bằng script so token (no-op vì giá trị là tokB)
redis-cli SET lock:x tokB NX PX 30000
redis-cli EVAL "if redis.call('get',KEYS[1]) == ARGV[1] then return redis.call('del',KEYS[1]) else return 0 end" 1 lock:x tokA
```

Đừng suy ra thời gian thật từ dãy này: các lệnh chạy tay nối tiếp, không tái hiện được khoảng chờ 30 s hay độ trễ giữa các client (nhận xét của người soạn).

### Phạm vi: những gì lab này không làm, và vì sao

Lab này phủ chương 05, 06, 08, 09, 11, 12, 16 và 17 ở mức "cơ chế có bất biến kiểm được bằng mô phỏng". Những mục dưới đây của các chương đó **cố ý không** có lab (nhận xét của người soạn về phạm vi, không phải thiếu sót ngẫu nhiên):

| Bỏ qua | Lý do |
| --- | --- |
| Multi-leader, CRDT, Merkle tree anti-entropy ([05 §5, §7](../12-he-thong-du-lieu/05-replication.md)) | Cần kiểu dữ liệu và cấu trúc băm riêng, ít bất biến nào ngoài "hội tụ"; lab (b) chỉ có read repair và hinted handoff, lab (e) chạm LWW và siblings. Để lại thành bài tập mở rộng (lab b, lab e). |
| Secondary index, request routing, partition động ([06 §4, §5.2, §6](../12-he-thong-du-lieu/06-partitioning.md)) | Là bài toán thiết kế và cấu hình hơn là cơ chế mô phỏng được gọn; lab (c) giữ phần có số đo rõ: dời khóa, vnode, hot key, slot Redis. |
| Two generals, failure detector, hai pha commit, Paxos ([08 §3–4](../12-he-thong-du-lieu/08-su-co-he-phan-tan.md), [09 §5–6](../12-he-thong-du-lieu/09-consistency-va-consensus.md)) | Raft (lab f) đại diện cho consensus; chương 08 §3 trình bày two generals như một thí nghiệm tư duy nên không có thuật toán để chạy; failure detector chỉ xuất hiện dưới dạng election timeout và TTL của lease. |
| Bộ kiểm tra linearizability tổng quát ([09 §1](../12-he-thong-du-lieu/09-consistency-va-consensus.md)) | Chỉ có phản ví dụ viết tay của quorum (lab b); kiểm lịch sử tùy ý là một bài riêng. |
| CDC, event sourcing, stream join, checkpoint/changelog ([11 §5, §7, §8](../12-he-thong-du-lieu/11-stream-processing.md)) | Lab (g) chạm outbox (cách thay dual write); phần còn lại cần một hệ stream để quan sát. |
| Checksum, kiểm tra ngoài luồng, audit ([12 §7](../12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md)) | Không có thuật toán phân tán để mô phỏng; là kỷ luật vận hành. |
| Cấu trúc dữ liệu, eviction, RDB/AOF, Cluster bus và failover, Pub/Sub, Streams ([16 §2–4, §6–7](../12-he-thong-du-lieu/16-redis-ben-trong.md)); queue, leaderboard, HyperLogLog/bitmap, session, feature flag ([17 §4–7](../12-he-thong-du-lieu/17-redis-patterns.md)) | Đó là cấu trúc dữ liệu hoặc persistence của một sản phẩm cụ thể; mô phỏng chúng nghĩa là dựng lại Redis và người soạn không chạy Redis thật (quy ước thực thi của dự án). Phần quan sát được trên Redis thật được viết thành lệnh ở mục "chưa chạy" phía trên. |
| Trong Raft: snapshot, thay đổi thành viên, pre-vote, đọc linearizable ([09 §7](../12-he-thong-du-lieu/09-consistency-va-consensus.md)) | Lab (f) là bản cài để học, không phải Raft cho production (chương 09 cũng dặn không tự viết Raft cho production); snapshot và thay đổi thành viên không có trong code, còn pre-vote và đọc linearizable chỉ xuất hiện như bài tập mở rộng. |

Hai giới hạn chung: mọi lab chạy trên **thời gian ảo và mạng mô phỏng**, nên không nói gì về hiệu năng thật (độ trễ, thông lượng) hay về hành vi của TCP, đĩa, bộ nhớ ([P5](../05-storage/00-tong-quan.md), [P6](../06-networking/00-tong-quan.md)); và mỗi mô hình chỉ có các lỗi nó liệt kê (mất tin nhắn độc lập, crash tại các điểm định sẵn, pause theo xác suất…), không có lỗi Byzantine, lỗi đĩa hay lệch đồng hồ ngoài các kịch bản được nêu.

### Bài tập mở rộng

1. (lab a) Đặt `Config.followers = 3` rồi so `Ack::Received(2)` với `Ack::Applied(2)` (dạng quorum, như `ANY 2` của PostgreSQL[^a-pg-any]): dự đoán RYW% và `lost/crash` trước khi chạy, rồi chạy `run_many`. Follower chậm nhất vẫn là follower cuối (`Config::delays`); đổi để hai follower chậm và xem `Received(2)` còn bảo vệ gì.
2. (lab a) Cài semi-sync "tụt về async sau timeout" như MariaDB mô tả[^a-m-timeout] — thêm biến thể `Ack::Received` có `timeout` vào `Cluster::ack_time` (nếu thời điểm ack vượt `t + timeout` thì ack ngay tại `t + timeout`). Đo `lost_acked` khi `timeout` nhỏ hơn lag của follower chậm và so với `Received(2)` thuần.
3. (lab a) Trong `run`, nhánh `Policy::Token` hiện fallback sang leader ngay. Thay bằng "chờ tối đa `wait` ms cho follower đủ mới rồi mới fallback" (đẩy lần đọc lùi `wait` ms bằng `push`); vẽ `lead%` theo `wait`. Quan sát bằng số: chờ có đắt hơn đọc leader không?
4. (lab b) Cho hint có thể mất: trong `run_sloppy`, với xác suất `hint_loss` standby "chết" trước khi giao hint. Dự đoán `stale_after` sẽ khác 0 ở mức nào theo `hint_loss`, rồi đo; liên hệ với anti-entropy ở [chương 05 mục 7](../12-he-thong-du-lieu/05-replication.md).
5. (lab b) Thêm anti-entropy vào `Sim`: cứ `K` thao tác thì so toàn bộ `versions` và chép version cao nhất cho mọi replica (một lần so Merkle-style có thể thay bằng so cả mảng). Vẽ `stale` theo `K` cho `(n, w, r) = (3, 1, 2)` và so với read repair của `Config::read_repair`.
6. (lab b) Đo *monotonic reads* (đọc sau không được cũ hơn đọc trước của cùng reader): thêm bộ đếm `regress` vào `Stats` (lần đọc trả version thấp hơn lần đọc thành công trước). Dự đoán xem `(3, 2, 2)` có `regress > 0` không (gợi ý: cột `ghost` của mục 1); rồi thêm write-back (ABD) vào `Sim::read` và kiểm `regress = 0` ([chương 09 mục 1](../12-he-thong-du-lieu/09-consistency-va-consensus.md)).
7. (lab c) Cài `Rendezvous` (highest random weight): node thắng là node có `hash64(node, key)` lớn nhất; paper Jump nhắc đây là thuật toán consistent hash duy nhất khác mà họ biết, cần thời gian tỉ lệ với số bucket[^c-jump-rdv]. Cài `Placement` cho nó rồi chạy lại `move_stats` và `LoadStats::of_keys` trong `section_moves`/`section_vnodes`; kiểm monotonicity và so `max/mean` với `Ring` có `v = 1000`.
8. (lab c) Vnode có trọng số: cho node 0 gấp đôi số điểm trên `Ring` (Dynamo nói số vnode có thể chọn theo năng lực[^c-dyn-vnode-adv]); đo tải của node 0 chia cho tải trung bình các node khác bằng `LoadStats` và xem nó có gần 2 không. Viết dự đoán trước khi chạy.
9. (lab c) Replication factor 3: thêm `Ring::replicas(hash, rf)` đi theo chiều kim đồng hồ cho tới khi gặp `rf` node khác nhau (Cassandra mô tả đúng quy tắc này[^c-cass-rf-walk]). Khi node thứ `N+1` vào, bao nhiêu khóa đổi **tập** replica so với đổi chủ chính? Đoán rồi đo.
10. (lab d) Thêm bước `Step::Renew`: một "watchdog" gia hạn lease mỗi `TTL_MS / 3` và *cũng bị pause* bởi `Pauses::Random`. Đo lại `runs_overlap` và `runs_lost` của `Unfenced` — gia hạn có giảm số run lỗi không, và có còn run `Fenced` mất update không? So với lời khuyên "đừng giả định khóa còn đó chỉ vì process còn sống"[^d-lk-no-assume-alive].
11. (lab d) Cài *lock-delay* kiểu Chubby[^d-ch-lockdelay] — sau khi lease hết hạn, `LockService::acquire` từ chối thêm `lock_delay` ms. Quét `lock_delay` từ 0 đến `TTL_MS` và vẽ `runs_lost` của `Fenced`/`Unfenced`; giải thích vì sao nó chỉ giảm chứ không loại bỏ lỗi (pause tối đa là `3 * TTL_MS`).
12. (lab d) Dùng *term* của Raft làm fencing token: viết `LockService` mà token là term của một leader-election (xem lab (f)), để một leader cũ (term nhỏ hơn) còn đang ghi sau khi bị thay bị storage từ chối — Raft bảo đảm tối đa một leader mỗi term nên thứ cần chặn là leader của term trước — và kiểm bằng sweep rằng `token_regressed` luôn `false` — liên hệ chương 09 mục 8 (nhận xét của người soạn: cùng họ ý tưởng "số thế hệ").
13. (lab e) Cài hybrid logical clock (`(physical, logical)`) cho `Lamport`: ý tưởng (nhận xét của người soạn) là lấy `max(physical_now, physical_prev)` và tăng phần logical khi hòa; **luật nhận** mới là chỗ quyết định: khi nhận tin mang `(p_msg, l_msg)`, đặt `p = max(p, p_msg, physical_now)` và tính `l` từ những giá trị đang hòa nhau (tăng `l` nếu hòa, về 0 nếu `p` mới lớn hơn tất cả), nên B sau khi đọc ghi của A (stamp vật lý 40) sẽ có stamp lớn hơn 40 dù đồng hồ của B chỉ 20 — áp luật nhận này ở bước `cb.recv(sa.t)` của `lww_demo`. Bài giảng chỉ nhắc tới đồng hồ lai kết hợp tính chất của đồng hồ logic và vật lý[^e-hybrid]; hãy chạy lại `lww_demo` với đồng hồ A nhanh 30 ms và kiểm xem ghi của B có còn thắng không (thử cả khi B không đọc A).
14. (lab e) Cài thuật toán loại trừ tương hỗ của Lamport trên `Net`: yêu cầu và giải phóng phát tới mọi process kèm timestamp; process vào critical section khi yêu cầu của nó đứng đầu hàng đợi theo `(t, node)` và đã nhận từ mọi process khác tin có timestamp muộn hơn[^e-grant]. Kiểm "không hai process cùng vào" trên nhiều seed.
15. (lab e) Bỏ `.max(self.last_arrival[from][to])` trong `Net::broadcast` (kênh không còn FIFO) và chạy `tob_arrival_order_diverges_lamport_order_agrees`: dự đoán test nào vỡ và vì sao, đối chiếu với giả định FIFO[^e-fifo].
16. (lab f) **Pre-vote / leader stickiness.** Trong `on_request_vote` (`src/raft.rs`), bỏ qua (không đổi term, không cấp phiếu) `RequestVote` đến trong vòng `election_timeout.0` kể từ `AppendEntries` hợp lệ cuối[^f-disrupt]. Chạy lại `sweep(Scenario::Partitions, …)`: dự đoán `max_term` giảm bao nhiêu và `recovery` có xấu đi không; vì sao mutant `NoUpToDateCheck` vẫn bị bắt?
17. (lab f) **Đọc linearizable.** Thêm `Action::Read(node)`: trước hết để leader trả lời từ state machine của nó và dùng cảnh cô lập trong test `leader_cut_off_keeps_accepting_commands_that_are_overwritten_after_the_heal` để chỉ ra một lần đọc cũ[^f-read]; rồi sửa bằng cách chỉ trả lời sau một vòng heartbeat với đa số[^f-readindex]. So với `ensure_linearizable` của openraft[^f-ensure-lin].
18. (lab f) **Dedup lệnh client.** Cho client gửi lại *cùng* id khi hết `client_timeout` (thay vì lệnh mới), đếm lệnh bị áp hai lần trong `Scenario::Combined`, rồi thêm bảng `(client, seq)` ở state machine[^f-dup].
19. (lab f) **Mutant của bạn.** Trong `on_append` đổi `leader_commit.min(last_new)` thành `leader_commit.min(n.log.len())`, hoặc trong `on_append_reply` bỏ cận `n.match_index[from] + 1`. Dự đoán tính chất nào bị vi phạm, thêm biến thể vào `Mutant`, rồi dùng `sweep_mutant` xem seed nào bắt được; so với test `follower_commits_only_up_to_the_last_entry_the_leader_vouched_for`.
20. (lab g) Đổi `LEASE_TTL` trong `delivery.rs` thành 2 rồi 20 (giá trị 1 làm kịch bản dựng tay của phần 3 panic vì lần retry ở t = 1 đã thấy lease hết hạn) và chạy lại phần 3. Dự đoán trước: với `KeyNonAtomic`, cột `409s` và `double` đổi thế nào (TTL là "con dao hai lưỡi" của [chương 17 mục 3.3](../12-he-thong-du-lieu/17-redis-patterns.md))? Rồi đọc `script_stale_handler`: handler chậm tiếp tục sau khi lease đã hết hạn (script luôn bắt đầu handler thứ hai ở `LEASE_TTL + 1`); đối chiếu `KeyAtomic` với bản trừ đôi của `KeyNonAtomic` ở hai giá trị TTL.
21. (lab g) Thêm "key hết hạn": cho `PayServer` xóa mọi `KeyEntry::Done` cũ hơn `retention` đơn vị thời gian ảo, đặt `retention` nhỏ hơn tổng thời gian retry của `run_intent` (1+2+…+7). Chứng minh bằng test rằng `KeyAtomic` cũng trừ đôi, rồi rút ra điều kiện cho cửa sổ nhớ ở [chương 12 mục 5](../12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md) ("dài hơn thời gian client còn có thể retry").
22. (lab g) Trong `run_producer`, cho producer *khởi động lại* sau vài vòng (đổi `PRODUCER_ID`, gửi lại mọi message chưa ack). Dự đoán số `duplicates` của chế độ `idempotent: true`, rồi đo; liên hệ với `transactional.id` ở [chương 11 mục 4](../12-he-thong-du-lieu/11-stream-processing.md) (phần Đi sâu).
23. (lab h) **Discarding vs accumulating.** `Firing.count` hiện là tổng tích luỹ (accumulating: kết quả sau là bản tinh chỉnh của kết quả trước[^h-df-trigger-modes]). Thêm chế độ discarding (mỗi lần phát chỉ mang phần delta kể từ lần phát trước[^h-df-discarding]) và chứng minh bằng test rằng tổng các delta bằng kết quả accumulating cuối cùng.
24. (lab h) **Replay.** Chạy `run_event_time` nhiều lần trên *cùng* `events` với các `D ≥ observed_max_delay`: `final_counts` luôn bằng nhau và bằng `truth_counts` (event time tái lập được khi chạy lại trên log cũ — suy luận của người soạn, chương 11 mục 8). Hai tính chất này đã có test (`event_time_with_d_at_least_max_delay_is_exact_for_all_seeds`, `event_times_do_not_depend_on_the_delay_model`: `generate` với `DelayModel::SMALL` và `HEAVY_TAIL` cho cùng `ts`, nên `truth_counts` giống hệt còn `processing_time_counts` khác). Hãy thêm một tính chất *mới*: xáo trộn thứ tự giữa các event có cùng `arrival` rồi chạy lại `run_event_time`; `final_counts` có đổi không, và firing nào đổi?
25. (lab h) **Watermark thích nghi.** Thay `D` cố định bằng "phân vị 99 của độ trễ đã quan sát đến lúc này" (nhận xét của người soạn: cách này không cần biết trần trước, nhưng đổi sai lầm "biết trần" thành sai lầm "ước lượng sai"). Đo drop và độ trễ ở `LATENCY_END_LIMIT` rồi so với hàng `D=500` và `D=1000` của bảng.
26. (lab i) Đổi `count` trong `SlowKind::Scan { count }` (và `COST_SCAN_BASE_US`) để tìm điểm gãy: `count` nhỏ làm `SCAN` cần rất nhiều lệnh, `count` rất lớn đưa p99 của `GET` trở lại mức `KEYS`. Vẽ p99 và số lệnh theo `count` rồi nhận xét (nhận xét của người soạn: bài toán chọn kích thước batch).
27. (lab i) Quét `NET_RTT_US` từ `(5, 10)` tới `(1000, 3000)` và số client trong `scenario2` cho `Rmw::GetSet`; vẽ `lost()` theo (RTT, số client). Giải thích vì sao một client duy nhất không bao giờ mất cập nhật còn nhiều client thì mất, và so với số `aborts` của `Rmw::WatchMulti` ở cùng tham số.
28. (lab i) Thêm replica C vào `failover_run` (và `WAIT 3`); thêm luật loại replica bị mất kết nối quá lâu như tài liệu Sentinel mô tả[^i-sent-select] (ví dụ B bị đứt link trong 10 ms cuối nên bị loại). Đo lại `ByPriorityThenOffset` + `WAIT 1`: luật loại có "cứu" được không, và khi nào lại làm tệ hơn?
29. (lab j) Thêm `Algo::SlidingCounter` (hai fixed window liền kề, cộng có trọng số theo phần cửa sổ đã trôi — nhắc ở [chương 17 mục 2.2](../12-he-thong-du-lieu/17-redis-patterns.md)). Chạy lại `window_stats` và `memory_elements`: nó có giữ $\le L$ trong mọi cửa sổ không? Dự đoán rồi đo — chương không chứng minh điều này.
30. (lab j) Dùng tham số `app_now` của `Algo::allow` để mô phỏng hai pod lệch đồng hồ $\delta$ ms với `FixedAligned` (F3, [chương 17 mục 0](../12-he-thong-du-lieu/17-redis-patterns.md)); đo số request được nhận quanh biên theo $\delta$ và so với `FixedTtl`, vốn không dùng đồng hồ pod.
31. (lab j) Sửa `token_bucket_script` thành `ts = max(ts, now)` và đổi test `token_bucket_backward_step_is_recredited_when_clock_recovers` cho khớp; kiểm cận $cap+rate\cdot W$ còn đúng khi đồng hồ server nhảy lùi ngẫu nhiên.
32. (lab k) Thêm tham số `stale_while_revalidate` vào `Policy`: khi mục vừa quá hạn logic, người thắng khóa tính lại còn mọi người khác nhận giá trị cũ ngay (độ trễ 0). So `waited%` và `wait_max` với dòng `single-flight ttl=10s` (xem [chương 17 mục 1.4](../12-he-thong-du-lieu/17-redis-patterns.md), biến thể stale-while-revalidate). Đo thêm tuổi tối đa của giá trị phục vụ.
33. (lab k) Mở rộng `enumerate_race` lên hai reader (`r1 r2 r3` × 2) và một writer: có bao nhiêu interleaving (đoán trước, rồi đếm: 8!/(3!·3!·2!)) và `Fix::Lease` còn bao nhiêu stale ở thứ tự A? Thử thêm một `Fix::LeaseAndVersion` kết hợp hai cách.
34. (lab k) Dùng `simulate_lock` với `random_lock_clients` thêm cơ chế gia hạn (watchdog mỗi TTL/3 như chương 17 mục 3.1) và đếm lại số cặp chồng lấn; rồi thêm một "pause" làm cả watchdog đứng và đối chiếu với lab (d) `fencing`.

## Câu hỏi tự kiểm tra (về output lab)

1. Vì sao ở bảng [2] `Received(all)` vẫn còn RYW 23.9% còn `Applied(all)` thì 0.0% (lab a)?
   <details><summary>Đáp án</summary>

   `Received` chờ follower *nhận* (và flush) entry, `Applied` chờ follower *replay* xong để thay đổi nhìn thấy được. PostgreSQL phân biệt rõ hai mức: `on` chờ standby nhận và flush commit record[^a-pg-on], còn `remote_apply` chờ replay[^a-pg-ra]; MariaDB cũng nói xác nhận semisync chỉ nói về relay log của replica chứ chưa chắc đã áp dụng[^a-m-relay]. Trong lab, follower chậm nhận sau `lag/4` nhưng replay xong sau `lag`, nên đọc ở follower đó giữa hai thời điểm vẫn thấy bản cũ.

   </details>

2. Vì sao `StickyFollower` có MR% = 0.0 nhưng vẫn có RYW% > 0 (lab a)?
   <details><summary>Đáp án</summary>

   Follower chỉ áp dụng log tiến lên nên cùng session đọc cùng follower thì không đọc lùi. Nhưng ghi đi qua leader còn follower được ghim có thể chưa replay lần ghi đó. Vogels nói cùng một server mỗi lần thì tương đối dễ bảo đảm read-your-writes và monotonic reads[^a-v-sticky]; ở lab, ghi và đọc không cùng server nên chỉ MR có được. Đọc leader hoặc dùng token là cách thêm RYW[^a-t94-ws].

   </details>

3. `Token` cho 0.0% RYW/MR nhưng "miễn phí" không? Cái giá ở output là gì (lab a)?
   <details><summary>Đáp án</summary>

   Không. Ở `lag=80` có 40.4% lần đọc phải quay về leader vì không follower nào đã apply tới LSN của session (cột `lead%`); ở `lag=20` chi phí là 0.0%. Token ghi nhớ vị trí log như write-set của Terry[^a-t94-ws], nên độ an toàn đến từ việc chỉ chọn replica đã đủ mới, và tải đọc dồn về leader khi replica lag.

   </details>

4. `Received(1)` cho `lost/crash = 0.0` nghĩa là gì và không nghĩa là gì (lab a)?
   <details><summary>Đáp án</summary>

   Trong mô hình (follower không sập), mọi ghi đã ack đều nằm ở ít nhất một follower đã nhận nên follower có LSN nhận cao nhất được thăng cấp luôn đủ. Lời hứa của PostgreSQL cũng chỉ là ứng dụng không nhận xác nhận trước khi WAL tới các standby đồng bộ; không có cách chắc chắn biết mọi standby đã nhận hết WAL lúc primary sập[^a-pg-ack]. Nếu follower duy nhất đã ack cũng sập (không mô phỏng ở lab), vẫn có thể mất.

   </details>

5. Vì sao hai log độc lập cho "payment mà thiếu order" còn một log thì không (lab a)?
   <details><summary>Đáp án</summary>

   Một replica áp dụng một log theo đúng thứ tự nên luôn giữ một prefix; order được commit trước payment thì không thể thiếu khi payment đã hiện. Hai shard replicate độc lập nên replica của shard `thanh toán` có thể đi trước replica của shard `đơn hàng`. Consistent prefix là thấy một chuỗi ghi có thứ tự[^a-t11-cp] tương ứng một phiên bản đã từng tồn tại ở master[^a-t11-cp-snap]. Lab chỉ đếm anomaly, không nói hệ thật nào cho gì.

   </details>

6. Vì sao ở bảng lab (b) `stale` bằng 0 ở mọi dòng `w+r>n = true` nhưng khác 0 ở mọi dòng `false`?
   <details><summary>Đáp án</summary>

   Nếu `w + r > n` thì quorum ghi và quorum đọc luôn giao nhau, nên quorum đọc chứa ít nhất một node đã xác nhận ghi[^b-intersect]. Replica chỉ tiến lên, nên node đó trả version không thấp hơn bản ghi đã xác nhận. Khi `w + r ≤ n`, tồn tại một tập đọc nằm hoàn toàn ngoài tập ghi (như `{C}` so với `{B}` ở trace tối thiểu) nên đọc có thể thấy bản cũ; bảng chỉ cho thấy điều này xảy ra với tần suất đo được ở `loss = 0.2`.

   </details>

7. Ở `n=3`, `write% = 51.5` với `w = 3` đến từ đâu?
   <details><summary>Đáp án</summary>

   Mô hình của lab cho mỗi replica tới được với xác suất 0.8, và ghi cần cả ba: `0.8³ = 0.512`. Với `w = 2` là `0.896`, với `w = 1` là `0.992`. Đây là tính toán trên mô hình; tỉ lệ thật phụ thuộc phân phối lỗi của hệ thật.

   </details>

8. Cột `ghost` nói gì về ý nghĩa của "ghi thất bại"?
   <details><summary>Đáp án</summary>

   Một ghi thất bại ở client (chưa tới `w` replica) vẫn có thể nằm trên vài replica và hiện ra ở lần đọc sau; Cassandra mô tả đúng điều này khi chưa có monotonic quorum reads[^b-vanish]. Do đó "thất bại" không phải rollback, và có thể làm các lần đọc liên tiếp thấy giá trị lúc có lúc không.

   </details>

9. Vì sao read repair giảm stale nhưng không đưa nó về 0 khi `w + r ≤ n`?
   <details><summary>Đáp án</summary>

   Read repair sửa replica đã trả lời *sau* khi đọc, nên lần đọc đang chạy vẫn có thể trả bản cũ; ngoài ra tin nhắn sửa có thể mất. Nó giúp các lần đọc sau, đúng như Dynamo mô tả: sửa replica bỏ lỡ cập nhật vào lúc thuận tiện và giúp anti-entropy khỏi phải làm[^b-rr-dynamo]. Muốn bảo đảm giao nhau thì cần `w + r > n`[^b-intersect].

   </details>

10. Trong kịch bản mục 3, `w + r > n` vậy mà reader 2 lại đọc v0. Điều gì còn thiếu và sửa thế nào?
   <details><summary>Đáp án</summary>

   `w + r > n` chỉ bảo đảm quorum đọc *thấy* bản ghi đã xác nhận, không bảo đảm thứ tự giữa hai lần đọc khi ghi còn dở dang; vì thế quorum không phải linearizable[^b-nonlin]. Sửa bằng write-back: đọc phải ghi ngược giá trị mới nhất cho replica chưa có và chờ giá trị mới nhất nằm trên một quorum trước khi trả[^b-abd][^b-abd-wait]; lab cho thấy khi đó reader 2 thấy v1.

   </details>

11. Vì sao sloppy quorum đọc stale dù `w + r > n`, và vì sao hết stale sau handoff?
   <details><summary>Đáp án</summary>

   Sloppy quorum cho phép ghi lên node ngoài `n` replica nhà[^b-sloppy], còn đọc ở lab chỉ hỏi replica nhà; tập ghi thực tế không còn nằm trong tập mà quorum đọc tính (suy luận của người soạn từ định nghĩa). Standby giữ hint và giao lại cho replica dự kiến khi nó hồi phục[^b-hint]; sau khi giao, ít nhất `w` replica nhà giữ bản ghi nên mọi quorum đọc `r` lại giao với chúng — lab đo được 0 stale ở lần đọc thứ hai, còn nếu hint không bao giờ được giao (dòng `no handoff`) thì lần đọc thứ hai vẫn stale 1027/13067. Con số 0 đúng vì mô hình không để mất hint.

   </details>

12. Vì sao ở lab (c) `modN` dời 83,2% khóa khi 10 → 12 node, còn `fixed`, `ring`, `jump` chỉ khoảng 16,7%, và 66514 khóa "chuyển giữa hai node cũ" nói lên điều gì?
   <details><summary>Đáp án</summary>

   Với `hash mod 10` rồi `mod 12`, một khóa chỉ đứng yên khi hai số dư bằng nhau — khoảng 1/6 số khóa — nên khoảng 5/6 đổi chỗ, trong khi chỉ cần dời 1/6 để cân bằng lại 12 shard[^c-jump-1-6][^c-cass-naive] (phép tính số học của người soạn, khớp với 83,2% của output). Phần lớn khóa dời là dời giữa hai node cũ, vốn không cần: consistent hashing có tính monotonicity — khi thêm bucket, đối tượng chỉ chuyển từ bucket cũ sang bucket mới[^c-kar-mono] — nên ba cách còn lại có đúng 0 khóa chuyển giữa node cũ.

   </details>

13. Vì sao `{user1000}.following` và `{user1000}.followers` ra cùng slot 3443 còn `foo{}{bar}` được băm nguyên key?
   <details><summary>Đáp án</summary>

   Nếu key có `{`, có `}` ở bên phải và có ít nhất một ký tự giữa `{` đầu tiên và `}` đầu tiên sau nó, chỉ phần giữa được băm[^c-redis-tag-rule]; hai key đầu cùng băm `user1000`[^c-redis-tag-ex]. Với `foo{}{bar}` thì `{` đầu tiên theo ngay sau bởi `}`, không có ký tự nào ở giữa, nên băm cả key[^c-redis-tag-empty]. Con số 3443 là output của code lab, chưa đối chiếu với server thật.

   </details>

14. Vì sao thêm node không làm `max/mean` của hot key giảm, và vì sao chỉ bảo vệ khóa hạng 1 (`H` = 1) vẫn còn 2,80 ở N = 40?
   <details><summary>Đáp án</summary>

   Một khóa chỉ nằm trên một node, nên node đó nhận ít nhất phần của khóa (10,2% ở mô hình Zipf s = 1 của lab) bất kể N; trung bình là `1/N` nên tỉ số tăng theo N (output: 1,23 → 4,86). Hậu tố rải ghi khóa đó qua nhiều khóa con[^c-ddb-suffix], nhưng ở phân phối Zipf khóa hạng 2, 3, … cũng nóng và trở thành điểm nóng mới; đó là kết quả của mô hình giả định, không phải số đo workload thật.

   </details>

15. Hậu tố và bản sao khác nhau ở đâu, và vì sao hai cột `(!)` đắt?
   <details><summary>Đáp án</summary>

   Hậu tố: ghi chọn một hậu tố nên rẻ, nhưng đọc phải truy vấn mọi hậu tố rồi gộp[^c-ddb-suffix-read]. Bản sao là hình ảnh phản chiếu: đọc chọn một bản, ghi phải cập nhật tất cả `S` bản (đó là cách làm trong output của lab, không phải mô tả của nguồn nào). Dùng sai công cụ cho mix đọc/ghi làm mỗi thao tác phổ biến tốn `S` request, nên chi phí lên tới 3,87× và 5,05× trong output.

   </details>

16. Vì sao range partition tự tách vẫn dồn ghi của khóa tăng đơn điệu vào một partition?
   <details><summary>Đáp án</summary>

   Khóa mới luôn lớn hơn mọi khóa cũ nên rơi vào khoảng ngoài cùng bên phải; tách một partition chỉ sinh thêm partition cũ ở bên trái, không có ghi mới nào quay lại đó. Output: 199 partition nhưng cửa sổ 100 ghi liên tiếp vẫn 99,8% vào một partition. MongoDB mô tả chunk chứa cận trên `MaxKey` nhận phần lớn insert với khóa đơn điệu[^c-mongo-mono-hot] và khuyên cân nhắc hashed sharding[^c-mongo-hash-mono]; Bigtable khuyên đặt giá trị phân biệt cao trước timestamp[^c-bt-ts-prefix].

   </details>

17. Vì sao thêm một bước "kiểm tra lease còn hạn" ngay trước `write` vẫn không an toàn (lab d, kịch bản 0 và 1)?
   <details><summary>Đáp án</summary>

   Pause có thể rơi đúng giữa lần kiểm tra và lần ghi: output cho thấy check OK ở t=2010 nhưng ghi ở t=17010, sau khi lease hết ở t=10000. Kleppmann nói rõ không sửa được bằng cách chèn kiểm tra hạn ngay trước khi ghi[^d-kb-check]. Cái cứu được là storage từ chối token cũ[^d-kb-fencing].

   </details>

18. Ở kịch bản 2, vì sao `Fenced` vẫn mất update còn `FencedCas` thì không? Token có đủ không?
   <details><summary>Đáp án</summary>

   Lúc ghi của A (token 33) tới, storage chưa thấy token 34 nên quy tắc `token < highest` không có gì để từ chối; ghi của B sau đó mang token 34 nhưng dựa trên lần đọc cũ, và `Fenced` không kiểm version. `FencedCas` từ chối ghi của B vì version đã đổi: không mất update, nhưng B là người giữ lease hợp lệ nên đó là một ghi hợp lệ bị từ chối (`valid_rejected`) — B phải đọc lại và thử lại, điều mô hình không làm. Token một mình chỉ bảo vệ khỏi ghi tới *sau* ghi có token cao hơn (suy luận của người soạn từ output); storage phải chủ động kiểm[^d-kb-fencing-active].

   </details>

19. Vì sao storage nhận token *bằng* token cao nhất, và hệ quả với kịch bản 3?
   <details><summary>Đáp án</summary>

   Chủ khóa hiện tại ghi nhiều lần với cùng một token, nên `token == highest` phải được nhận. Hệ quả: hai holder cùng token (bộ đếm lùi sau failover) không phân biệt được. Vì vậy dịch vụ khóa phải sinh token tăng đơn điệu nghiêm ngặt[^d-kb-fencing-monotonic]; một counter đơn trên một node là chưa đủ[^d-kb-counter], cần consensus hoặc revision/zxid[^d-kb-fencing-consensus][^d-kb-zk].

   </details>

20. Vì sao client nên đo hạn lease bằng đồng hồ monotonic tính từ lúc *gửi* request, và vì sao vậy vẫn chưa đủ?
   <details><summary>Đáp án</summary>

   Wall clock có thể nhảy hoặc lệch giữa các máy[^d-tod-jump], còn hiệu hai giá trị monotonic trên cùng node thì có nghĩa[^d-mono-local]; tính từ lúc gửi cho một xấp xỉ bảo thủ, như local lease timeout của Chubby[^d-ch-lease-local]. Nhưng đồng hồ đúng cũng không cứu được pause giữa check và write (kịch bản 1); chỉ fencing làm safety độc lập với thời gian[^d-kb-safety-liveness].

   </details>

21. Vì sao cột `overlap` giống nhau ở ba mode, và điều đó nói gì về fencing?
   <details><summary>Đáp án</summary>

   Lịch trình của client không phụ thuộc kết quả ghi (client bị từ chối vẫn release và đi tiếp), nên cùng seed ⇒ cùng pause ⇒ cùng overlap. Fencing không loại hai bên cùng tin mình giữ khóa; nó làm tài nguyên không bị hỏng. Khóa "vì hiệu quả" có thể chấp nhận lease thuần, khóa "vì đúng đắn" cần kiểm ở tài nguyên đích[^d-kb-two-reasons] (nhận xét của người soạn).

   </details>

22. Vì sao `b1` có Lamport `L = 1` nhỏ hơn "A gửi m1" có `L = 2` mà không suy ra được `b1` xảy ra trước (lab e)?
   <details><summary>Đáp án</summary>

   Clock Condition chỉ đảm bảo chiều "a xảy ra trước b ⇒ `L(a) < L(b)`"[^e-clockcond]; chiều ngược lại không đúng vì nó sẽ buộc hai sự kiện đồng thời phải cùng một lúc[^e-converse]. Trong bảng A/B/C, hai sự kiện đó không có chuỗi tin nào nối chúng: vector `<0,1,0>` và `<2,0,0>` không so sánh được. Trong quét 200 seed, 176 243 trong 189 916 cặp đồng thời có `L` khác nhau.

   </details>

23. Trong lab (e), vì sao LWW theo timestamp vật lý giữ `x=1` còn LWW theo stamp Lamport giữ `x=2`?
   <details><summary>Đáp án</summary>

   Timestamp vật lý của A là 40 (đồng hồ nhanh 30 ms) còn của B là 20, nên bản `x=2` thua dù B ghi sau khi đọc `x=1`: sau NTP vẫn còn độ bất định dư về độ lệch, nên thứ tự theo timestamp time-of-day có thể sai[^e-ts-order]. Stamp Lamport của B được tính sau khi nhận stamp của A (`recv` rồi `tick`), nên `(3,B) > (1,A)`, phù hợp tính chất Lamport timestamp nhất quán với nhân quả[^e-lamport-prop].

   </details>

24. Hai ghi thực sự đồng thời: LWW theo `(t,node)` và vector clock cho kết quả khác nhau thế nào (lab e)?
   <details><summary>Đáp án</summary>

   Hai stamp `(1,A)` và `(1,B)` chỉ được xếp theo id node (tùy ý), nên LWW giữ một bản và mất bản kia — cái giá đã nêu của LWW[^e-lww-loss]; với hai ghi đồng thời, Lamport cũng chỉ xếp chúng tùy ý[^e-arb]. Vector `<1,0>` và `<0,1>` không so sánh được nên multi-value register giữ cả hai như siblings; một ghi mang context đã gộp thay cả hai thành một bản, như Dynamo nói về cập nhật dùng context để gộp các nhánh[^e-d-context].

   </details>

25. Vì sao TOB bằng Lamport timestamp trong lab (e) kẹt khi một node im lặng, và điều đó nói gì về nhu cầu consensus?
   <details><summary>Đáp án</summary>

   Replica chỉ giao phép toán có `(t,node)` nhỏ nhất khi đã nghe từ *mọi* node khác một tin có `t' >= t`; với node im lặng điều kiện không bao giờ thỏa, nên các phép toán sau đó kẹt (output: `stuck-pending=4`). Bài giảng nêu cả hai cách làm TOB đơn giản (leader và Lamport timestamp) đều không chịu lỗi: một node crash có thể chặn mọi node khác giao tin[^e-tob-naive]. Lamport nhận xét tương tự cho thuật toán của ông[^e-halt] và vì không có thời gian vật lý nên không phân biệt được node hỏng với node tạm dừng[^e-fail]. Do đó cần consensus, tương đương với TOB về mặt hình thức[^e-equiv].

   </details>

26. Trong lab (e), vì sao áp phép toán theo thứ tự tới nơi làm các replica lệch nhau, còn áp theo `(t,node)` thì không?
   <details><summary>Đáp án</summary>

   `set` rồi `append` cho kết quả khác `append` rồi `set`, nên cùng tập phép toán nhưng khác thứ tự thì khác trạng thái. State machine replication cần cùng thứ tự và logic áp tất định[^e-smr-det]; thứ tự `(t,node)` cho mọi replica cùng dãy sau khi tin ổn định (xem output: cùng dãy giao trên A, B, C). Nếu muốn dùng thứ tự yếu hơn thì phải làm cho các cập nhật đồng thời giao hoán[^e-commute].

   </details>

27. Vì sao một node phải ghi `votedFor` xuống bộ nhớ bền trước khi trả lời `RequestVote` (lab f)?
   <details><summary>Đáp án</summary>

   Nếu `votedFor` mất sau restart, node có thể bỏ phiếu lần hai cho một ứng viên khác *trong cùng term*, nên hai ứng viên cùng đạt đa số và có hai leader một term: phá Election Safety, vốn dựa vào "mỗi term một phiếu" và tính giao nhau của hai đa số[^f-vote][^f-majority]. Paper đòi cập nhật stable storage trước khi trả lời[^f-persist]; openraft yêu cầu tương tự[^f-save-vote]. Lab: mutant `NoPersistVote` bị bắt ở seed 5 (25/300) khi crash ngay sau khi cấp phiếu, còn crash ngẫu nhiên thuần thì không.

   </details>

28. Ở mục 3 của output, vì sao timeout cố định `[150,150)` không bao giờ có leader còn `[150,300)` thì luôn có (lab f)?
   <details><summary>Đáp án</summary>

   Cùng lúc hết giờ thì mọi node thành candidate, bỏ phiếu cho chính mình và từ chối nhau; vòng sau lặp y hệt, 13 vòng × 5 candidate trong 2 s mà không có leader[^f-split]. Khoảng ngẫu nhiên phá đối xứng nên thường chỉ một node hết giờ trước[^f-random]. FLP nói trong hệ bất đồng bộ không thuật toán tất định nào bảo đảm kết thúc khi dù chỉ một node có thể hỏng[^f-flp]; Raft dùng đồng hồ chỉ cho timeout[^f-flp-clocks]; hàng `w = 0` là livelock do đối xứng chứ không phải ví dụ của FLP (phép so sánh của người soạn), và kết quả `w ≥ 25` chỉ là quan sát, không chứng minh.

   </details>

29. Leader bị cô lập vẫn nhận lệnh của client (timeline, `partitions`). Lệnh đó có bị mất không, và vì sao Raft vẫn an toàn (lab f)?
   <details><summary>Đáp án</summary>

   Entry chỉ nằm ở leader cũ nên không bao giờ đạt đa số, không được commit và không được áp. Khi nối lại, leader term cao hơn ép log của nó trùng log của mình và ghi đè phần chưa commit[^f-force] (cột `overwrit.`). An toàn vì Leader Completeness chỉ đòi leader mới có mọi entry *đã commit*[^f-lc]; leader cũ phát hiện mình lỗi thời qua term lớn hơn[^f-stale-term], và trong lúc bị cô lập nó không quyết được gì thêm[^f-leader-quorum]. Client không nhận hồi đáp nên (trong lab) quên leader sau 400 ms.

   </details>

30. Vì sao leader không commit entry của term cũ bằng cách đếm bản sao, và lab chứng minh điều đó thế nào (lab f)?
   <details><summary>Đáp án</summary>

   Entry term cũ có thể nằm trên đa số mà vẫn bị leader khác ghi đè (Hình 8)[^f-fig8], nên Raft chỉ commit bằng cách đếm entry của term hiện tại, còn entry trước đó được commit gián tiếp[^f-prior]. Lab dựng lại Hình 8 bằng script: mutant `CommitOldTermByCounting` commit entry term 2 ở term 4 rồi mất nó ở term 5 (Leader Completeness, sau đó State Machine Safety bị vi phạm, 300/300 seed); thuật toán đúng không vi phạm.

   </details>

31. "300 seed × 5 kịch bản, 0 vi phạm" có chứng minh Raft đúng không? Nên bổ sung bằng gì (lab f)?
   <details><summary>Đáp án</summary>

   Không: mô phỏng chỉ lấy mẫu lịch chạy, `NoPersistVote` lọt qua trăm seed crash ngẫu nhiên. Model checking duyệt mọi lịch trong biên nhưng cũng có thể bỏ sót khi mô hình quá nhỏ[^f-tlc-missed]; TLA+ từng tìm ra lỗi 35 bước mà test không thấy[^f-aws-dynamo][^f-aws-unnoticed], nhưng không kiểm code[^f-aws-gap]. Dùng cả hai là nhận xét của người soạn: sweep và mutant để thử *code* và checker, TLA+ (P13) để thử *thiết kế*.

   </details>

32. Vì sao `commit→process` chỉ mất event mà không bao giờ trùng, còn `process→commit` thì ngược lại (lab g, phần 1)?
   <details><summary>Đáp án</summary>

   Chỉ khác ở thứ tự hai bước và crash rơi vào giữa chúng. Lưu offset trước: process thay thế bắt đầu từ offset đã lưu nên bỏ sót message chưa xử lý (at-most-once)[^g-k-cons-amo]. Xử lý trước: process thay thế xử lý lại vài message đầu (at-least-once)[^g-k-cons-alo]. Output: 1754 mất/0 trùng so với 0 mất/1736 trùng.

   </details>

33. Vì sao dedup phía sink bằng "kiểm id, áp dụng, ghi id" vẫn không exactly-once, và phải sửa thế nào (lab g, phần 1)?
   <details><summary>Đáp án</summary>

   Ba bước rời nhau thì crash giữa "áp dụng" và "ghi id" cho lần giao lại áp dụng lần hai (1143 trùng); đảo thứ tự thì crash giữa "ghi id" và "áp dụng" làm mất (1190). Sửa: ghi id (hoặc offset) trong **cùng commit nguyên tử** với tác dụng. Kafka khuyên lưu offset cùng chỗ với output vì nhiều hệ đích không hỗ trợ two-phase commit[^g-k-external], và Kleppmann nhấn mạnh trạng thái dedup phải ở stable storage[^g-cam-dedup].

   </details>

34. Vì sao producer retry mà không có idempotence thì trùng, và với `window > 1` còn đảo thứ tự (lab g, phần 2)?
   <details><summary>Đáp án</summary>

   Mất ack sau khi broker đã append làm producer gửi lại; nó không biết lỗi xảy ra trước hay sau khi commit[^g-k-prod-unsure], và `enable.idempotence=false` nghĩa là retry có thể ghi trùng[^g-kp-enable-idem]. Với nhiều request in-flight, bản retry của batch đầu có thể rơi sau batch sau đã thành công, nên batch sau hiện trước[^g-kp-retries-order]. Broker khử trùng bằng producer ID và sequence number[^g-k-idem]; việc *từ chối* seq vượt trước là cách mô hình của lab giữ thứ tự (không phải mô tả nội bộ Kafka).

   </details>

35. Server nên trả gì cho bốn tình huống của `Idempotency-Key`, và tài liệu tham chiếu có phải chuẩn không (lab g, phần 3)?
   <details><summary>Đáp án</summary>

   Lần đầu: xử lý bình thường (201). Retry sau khi xong: trả lại kết quả đã lưu, không thực hiện lại[^g-idem-retry]. Retry khi bản gốc còn chạy: 409[^g-idem-409]. Cùng key khác payload: 422[^g-idem-422]. Nguồn là bản thảo IETF đã hết hạn[^g-idem-expired], nên đó là thực hành mô tả chứ không phải chuẩn; Stripe lưu và trả lại response của request đầu cho retry cùng key[^g-stripe-v1].

   </details>

36. Outbox còn trùng event; vì sao vẫn tốt hơn dual write, và trùng xử lý ở đâu (lab g, phần 4)?
   <details><summary>Đáp án</summary>

   Dual write có hai cửa sổ không nguyên tử: gửi giữa transaction thì chưa chắc commit, gửi sau commit thì có thể chết trước khi gửi[^g-msio-fail] — output: 899 mất hoặc 899 phantom. Outbox ghi event trong cùng transaction với dữ liệu nên message được gửi nếu và chỉ nếu commit[^g-msio-benefit]; relay có thể publish một message nhiều lần[^g-msio-dup] (929 trùng), nên consumer dedup theo event id[^g-debezium-outbox-id]; sau dedup số tác dụng trùng là 0. Dedup không sửa được mất/phantom của dual write.

   </details>

37. Vì sao ở lab (h) processing-time window sai ở 15/17 window còn event-time với `D = 2987` thì chính xác?
   <details><summary>Đáp án</summary>

   Processing time đếm theo lúc đến, mà 168 event bị trễ qua biên window nên chuyển sang window khác; hai miền thời gian lệch nhau một độ trễ biến động[^h-df-skew]. Event-time dùng `ts` thật nên đúng window; `D = 2987` ≥ độ trễ lớn nhất nên không event nào tới khi window của nó đã phát, tức không có late event (theo cách đếm của lab). Lưu ý: ở seed này lab biết trần 2987; hệ thống thật thường không dựng được watermark đúng 100%[^h-df-heur].

   </details>

38. Trong bảng quét `D` (lab h), vì sao số event bị bỏ giảm liên tục nhưng số window sai vẫn là 13 từ `D = 0` đến `D = 500`?
   <details><summary>Đáp án</summary>

   `drop` đếm event, `wrong` đếm window: chỉ cần *một* event bị bỏ là cả window sai. Khoảng 60 event thuộc đuôi 1–3 s trải khắp 14 window, và `D ≤ 500` không đủ che chúng (số bị bỏ ở `D = 500` là 63), nên hầu như window nào cũng mất ít nhất một event. Watermark còn có thể "quá nhanh" kể cả khi phần lớn dữ liệu đã đúng[^h-df-fast]. (Lập luận về số 60 là kỳ vọng 3% × 2000 chứ không phải số in ra.)

   </details>

39. Allowed lateness `L = 2987` với `D = 0` khác gì so với `D = 2987` với `Drop`? Khi nào chọn cái nào?
   <details><summary>Đáp án</summary>

   Cả hai cuối cùng đều chính xác (0 drop, 0 sai), nhưng `D = 2987, Drop` phát *một* kết quả đúng sau ~3003 ms trung bình, còn `D = 0, L = 2987` phát kết quả đầu sau ~23 ms (sai) rồi sửa 133 lần, kết quả cuối đến sau ~2255 ms trung bình. Flink cho phép event trễ vào window và có thể làm window phát lại[^h-fl-refire], trong khi việc làm watermark chậm đi để chờ làm trễ kết quả mọi window[^h-fl-wm-delay]. Chọn allowed lateness khi người dùng cần số sơ bộ nhanh và chịu được cập nhật; chọn `D` lớn khi chỉ chấp nhận một kết quả duy nhất (nhận xét của người soạn).

   </details>

40. Vì sao idle timeout làm watermark tiến tiếp nhưng lại làm mất event trong lab (h)?
   <details><summary>Đáp án</summary>

   Watermark là min các partition nên một partition im lặng giữ cả pipeline[^h-fl-idle]; đánh dấu nó idle loại nó khỏi phép min[^h-fl-idle2], nhờ đó 8 window phát đúng lúc (trễ 50 ms). Nhưng partition 1 không chết mà chỉ nghẽn; khi nó xả backlog thì event đã nằm sau watermark nên bị coi là trễ (263 event bị bỏ ở `Drop`). Không có timeout thì ngược lại: không mất gì nhưng chỉ 3 window phát trước t = 9000.

   </details>

41. Hopping window cỡ 5000 ms, bước 1000 ms gán một event vào mấy window, và tumbling liên hệ thế nào?
   <details><summary>Đáp án</summary>

   5 window (`size/slide`): `hopping_starts(7300, 5000, 1000)` = 3000…7000. Tumbling là hopping với cỡ bằng bước tiến nên mỗi record thuộc đúng một window[^h-ksdsl-tumb].

   </details>

42. Vì sao `KEYS *` làm p99 của `GET` vọt lên hàng chục ms còn `SCAN COUNT 1000` thì không, dù cả hai đi qua cùng 1 triệu key (lab i)?
   <details><summary>Đáp án</summary>

   Server chạy từng lệnh một nên mọi request đến trong lúc `KEYS` chạy phải chờ hết lệnh[^i-lat-seq][^i-lat-slow]; `KEYS` là một lệnh O(N) 40 ms trong mô hình. `SCAN` chia việc thành 1000 lệnh ≤ 45 µs và `COUNT` là lượng việc mỗi lần gọi[^i-scan-count], nên `GET` chỉ chờ tối đa một batch. Cái giá: 1000 lần gọi và kết quả có thể trùng[^i-scan-dup].

   </details>

43. Vì sao `GET` rồi `SET v+1` mất 6366/8000 cập nhật còn `INCR` thì không, và có thể sửa bằng `MULTI`/`EXEC` thuần không (lab i)?
   <details><summary>Đáp án</summary>

   Mỗi lệnh nguyên tử nhưng cặp lệnh thì không: giữa hai request của một client server còn phục vụ client khác (read-modify-write bị chen). `INCR` gói cả ba bước vào một lệnh[^i-incr-atomic]. `MULTI`/`EXEC` thuần không sửa được: lệnh chỉ được xếp hàng tới `EXEC`[^i-tx-usage] nên `SET` không dùng được kết quả `GET`; cần `WATCH` + thử lại (đúng nhưng ở lab tốn 5× số request vì 47 660 lần hủy[^i-tx-watch-retry]) hoặc script nguyên tử[^i-sc-atomic].

   </details>

44. Vì sao `WAIT 1` + `ByPriorityThenOffset` vẫn mất ghi đã báo xong còn `WAIT 1` + `ByOffset` thì không, trong mô hình (lab i)?
   <details><summary>Đáp án</summary>

   `WAIT 1` chỉ đảm bảo *một* replica nào đó (ở đây là A, nhanh) đã áp dụng ghi[^i-wait-doc]. Sentinel xếp theo `replica-priority` trước, offset sau[^i-sent-order]; B có priority tốt hơn nhưng chậm nên bị thăng và thiếu các ghi mà chỉ A có. `ByOffset` thăng replica có offset cao nhất, luôn ≥ offset của replica đã ack nên không mất — nhưng chỉ khi replica đó còn sống (phản ví dụ "only B").

   </details>

45. Kết quả "WAIT 2 mất 0" trong lab có chứng minh rằng `WAIT` làm Redis an toàn không (lab i)?
   <details><summary>Đáp án</summary>

   Không. Đó là kết quả của mô hình, không có replica đứt giữa chừng, partition hay offset báo trễ. Tài liệu nói `WAIT` không biến Redis thành CP, ghi đã ack vẫn có thể mất khi failover[^i-repl-wait-cp], và ngay cả ghi đã đồng bộ tới nhiều replica vẫn có thể mất vì chọn replica chỉ là best-effort[^i-wait-besteffort].

   </details>

46. Vì sao `UNLINK big` không làm `GET` chậm còn `DEL big` thì có, và mô hình đơn giản hóa điều gì (lab i)?
   <details><summary>Đáp án</summary>

   `DEL` giải phóng value trong vòng lặp, tốn theo số phần tử[^i-del-cx]; `UNLINK` thu hồi bộ nhớ ở thread khác[^i-unlink-doc], nhưng chỉ với value đủ lớn, object nhỏ vẫn giải phóng đồng bộ[^i-lazyfree]. Mô hình chỉ có ngưỡng 64 phần tử, không mô hình thread nền thật hay tranh chấp bộ nhớ giữa thread nền và thread chính.

   </details>

47. Vì sao với cùng $L=100$/phút, fixed window cho 200 request trong 0,2 s còn sliding log chỉ cho 100 (lab j)?
   <details><summary>Đáp án</summary>

   Fixed window đếm theo từng cửa sổ riêng và đặt lại về 0 ở biên, nên $L$ request cuối cửa sổ này và $L$ request đầu cửa sổ sau đều "không vượt" (mục (a) của output: 200). Sliding log lưu từng timestamp và xóa các mục cũ hơn $now-W$ rồi mới đếm[^j-zrem-doc], nên cửa sổ luôn là $(now-W, now]$ và không có biên để lách (đo: 100 ở cả hai kịch bản). Cái giá là bộ nhớ $O(L)$ mỗi key (mục (e): 10 000 phần tử khi $L=10\,000$).

   </details>

48. Vì sao `INCR` + `EXPIRE NX` trong `MULTI`/`EXEC` không rò key còn `INCR` rồi `EXPIRE` rời thì rò (lab j)?
   <details><summary>Đáp án</summary>

   Trong `MULTI`/`EXEC` các lệnh chạy liền nhau và không request nào chen giữa[^j-tx-serial]; client mất kết nối trước `EXEC` thì không lệnh nào chạy[^j-tx-exec-all]. Bản rời có cửa sổ "đã `INCR` chưa `EXPIRE`": tài liệu Redis nói key rò rỉ nếu client không chạy `EXPIRE`[^j-rl-race]. Output (d2): 306/1000 key rò, 286 key chặn client lành mạnh vĩnh viễn so với 0 của `MULTI`.

   </details>

49. Vì sao sliding log cần script chứ không dùng `MULTI`/`EXEC` (lab j)?
   <details><summary>Đáp án</summary>

   Quyết định "thêm hay không" phụ thuộc kết quả `ZCARD`, mà lệnh trong `MULTI` chỉ được xếp hàng và chạy khi `EXEC`[^j-tx-usage] nên không rẽ nhánh được; script thì đọc rồi ghi như một khối nguyên tử[^j-sc-atomic]. Output (d): ba lệnh rời nhận nhiều hơn $L$ ở 190/200 seed, script không bao giờ.

   </details>

50. Vì sao nên dùng `EXPIRE … NX` thay vì `EXPIRE` thường ở fixed window (lab j)?
   <details><summary>Đáp án</summary>

   `EXPIRE` lên key đã có hạn cập nhật TTL về giá trị mới[^j-expire-refresh], nên dưới tải liên tục key không bao giờ hết hạn và counter chỉ tăng; `NX` chỉ đặt hạn khi key chưa có[^j-expire-nx]. Output (d2): 50 request được nhận với `NX` so với 5 với `EXPIRE` thường trong 600 s.

   </details>

51. Token bucket `cap=100`, nạp 100/phút có chặt hơn fixed window trong một cửa sổ 60 s không (lab j)?
   <details><summary>Đáp án</summary>

   Không: cận là $cap+rate\cdot W=200$, bằng cận $2L$ của fixed window; output (c) đo được 199. Muốn chặt hơn phải hạ `cap` (với `cap=20` cận là 120, đo được 95). Đây là kết quả của mô hình lab và công thức của người soạn (suy luận của người soạn), không phải mô tả của tài liệu Redis.

   </details>

52. Vì sao naive cache-aside cho đúng 30 lần tính lại trong ví dụ 10 req/s × 3 s, và con số đó đổi thế nào nếu tính lại mất 1,5 s (lab k)?
   <details><summary>Đáp án</summary>

   Mọi request đến trong khoảng thời gian tính lại của lần đầu đều thấy trượt và cùng tính lại, nên số lần = tần suất × thời gian tính lại: 10 × 3 = 30, đúng ví dụ của paper VLDB 2015[^k-xf-size]. Với 1,5 s là 15 (test kiểm `compute/interval` với `(100, 700)` ra 7, `(50, 2000)` ra 40...). Trong mô hình đều đặn đây là số chính xác; với request ngẫu nhiên nó là kỳ vọng.

   </details>

53. Khi `lock_ttl` nhỏ hơn thời gian tính lại, điều gì xảy ra với single-flight, và sửa thế nào (lab k)?
   <details><summary>Đáp án</summary>

   Khóa hết hạn khi người thắng còn đang tải; người thua kế tiếp thắng `SET NX PX` và tải thêm — output cho 3 lần tính lại mỗi wave với TTL 1 s so với 1 lần khi TTL 10 s. TTL của khóa vừa là thời gian tự nhả vừa là khoảng client có để làm xong[^k-lk-window], nên phải lớn hơn thời gian tải tệ nhất. Hậu quả ở đây chỉ là tốn thêm tài nguyên — loại khóa "vì hiệu quả"[^k-kb-efficiency].

   </details>

54. Trong cuộc đua stale set của lab, vì sao chỉ đúng 1 trong 10 interleaving của thứ tự A là stale, và nó là cái nào (lab k)?
   <details><summary>Đáp án</summary>

   Stale cần `r2 < w1` (reader đọc DB cũ trước khi DB đổi) và `w2 < r3` (reader SET sau khi writer đã DEL), cộng thứ tự chương trình `r1 < r2` và `w1 < w2`. Chỉ có một cách xếp: `r1 r2 w1 w2 r3`. Đây là cuộc đua mà Facebook gọi là stale set[^k-fb-stale]; thứ tự A là thứ tự Azure khuyến nghị (cập nhật data store trước khi xóa mục cache[^k-az-order]) — nó *thu hẹp* cửa sổ (1/10 so với 6/10 của thứ tự B) chứ không đóng hẳn.

   </details>

55. Lease token có sửa được thứ tự B (DEL cache rồi mới ghi DB) không (lab k)?
   <details><summary>Đáp án</summary>

   Không hoàn toàn: trong mô hình còn 2/10 interleaving stale (`w1 r1 r2 r3 w2` và `w1 r1 r2 w2 r3`). Token chỉ bị vô hiệu khi mục nhận `delete`[^k-fb-lease]; ở thứ tự B, reader xin token *sau* DEL duy nhất rồi đọc DB cũ, nên token vẫn hợp lệ khi SET. Với thứ tự A, interleaving stale duy nhất có `r1` đứng trước `w2`, nên DEL hủy token trước `r3` và còn 0/10 stale. Đây là kết quả của mô hình lab (writer không gửi gì tới cache khi ghi DB), không phải mô tả mọi triển khai lease.

   </details>

56. Nhả khóa bằng script so token có đủ để hai client không bao giờ cùng ở trong vùng găng không (lab k)?
   <details><summary>Đáp án</summary>

   Không. Nó loại vi phạm do `DEL` trần xóa nhầm khóa người khác[^k-lk-del-unsafe][^k-lk-unlock] (cặp B&C và vụ xóa lan truyền), nhưng cặp A&B (A làm 45 s trên khóa 30 s) vẫn còn trong cả hai cách nhả vì khóa đã hết hạn dưới chân A. Cần fencing token kiểm ở tài nguyên đích[^k-lk-disclaimer-fence] (lab d).

   </details>

[^a-m-relay]: MariaDB Documentation, Semisynchronous Replication — <https://mariadb.com/kb/en/semisynchronous-replication/> — "The guarantee is about the replica's relay log, not about the replica's data: an acknowledged transaction has been written to a replica's relay log, but it has not necessarily been applied there yet."
[^a-pg-ack]: PostgreSQL 16, 27.2.8.4 Planning for High Availability — <https://www.postgresql.org/docs/16/warm-standby.html> — "There is no way to be certain that all standbys have received all outstanding WAL data at time of the crash of the primary. Some transactions may not show as committed on the standby, even though they show as committed on the primary. The guarantee we offer is that the application will not receive explicit acknowledgment of the successful commit of a transaction until the WAL data is known to be safely received by all the synchronous standbys."
[^a-pg-any]: PostgreSQL 16, 27.2.8.2 Multiple Synchronous Standbys — <https://www.postgresql.org/docs/16/warm-standby.html> — "The method ANY specifies a quorum-based synchronous replication and makes transaction commits wait until their WAL records are replicated to at least the requested number of synchronous standbys in the list."
[^a-pg-async]: PostgreSQL 16, 27.2.5 Streaming Replication — <https://www.postgresql.org/docs/16/warm-standby.html> — "Streaming replication is asynchronous by default (see Section 27.2.8), in which case there is a small delay between committing a transaction in the primary and the changes becoming visible in the standby."
[^a-pg-hot-ec]: PostgreSQL 16, 27.4.1 User's Overview — <https://www.postgresql.org/docs/16/hot-standby.html> — "The data on the standby takes some time to arrive from the primary server so there will be a measurable delay between primary and standby. Running the same query nearly simultaneously on both primary and standby might therefore return differing results. We say that data on the standby is eventually consistent with the primary."
[^a-pg-loss]: PostgreSQL 16, 27.2.8 Synchronous Replication — <https://www.postgresql.org/docs/16/warm-standby.html> — "If the primary server crashes then some transactions that were committed may not have been replicated to the standby server, causing data loss. The amount of data loss is proportional to the replication delay at the time of failover."
[^a-pg-on]: PostgreSQL 16, 20.5.1 Settings (synchronous_commit) — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "When set to on, commits wait until replies from the current synchronous standby(s) indicate they have received the commit record of the transaction and flushed it to durable storage."
[^a-pg-ra]: PostgreSQL 16, 27.2.8.1 Basic Configuration — <https://www.postgresql.org/docs/16/warm-standby.html> — "Setting synchronous_commit to remote_apply will cause each commit to wait until the current synchronous standbys report that they have replayed the transaction, making it visible to user queries. In simple cases, this allows for load balancing with causal consistency."
[^a-pg-ra-cost]: PostgreSQL 16, 20.5.1 Settings (synchronous_commit) — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "This will cause much larger commit delays than previous settings since it waits for WAL replay."
[^a-pg-rtt]: PostgreSQL 16, 27.2.8 Synchronous Replication — <https://www.postgresql.org/docs/16/warm-standby.html> — "Waiting for confirmation increases the user's confidence that the changes will not be lost in the event of server crashes but it also necessarily increases the response time for the requesting transaction. The minimum wait time is the round-trip time between primary and standby."
[^a-ryw]: Kleppmann, Distributed Systems (Cambridge 2021/22), mục 5.2 Quorums — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "after a client writes a value, the same client will be able to read back the value it has just written."
[^a-ryw-strict]: Kleppmann, Distributed Systems (Cambridge 2021/22), mục 5.2 Quorums — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "read-after-write consistency requires reading either the last value written, or a later value."
[^a-t11-cp]: Terry, Replicated Data Consistency Explained Through Baseball (MSR 2011), mục 2 — <https://www.microsoft.com/en-us/research/wp-content/uploads/2011/10/ConsistencyAndBaseballReport.pdf> — "By requesting a consistent prefix, a reader is guaranteed to observe an ordered sequence of writes starting with the first write to a data object."
[^a-t11-cp-snap]: Terry, Replicated Data Consistency Explained Through Baseball (MSR 2011), mục 2 — <https://www.microsoft.com/en-us/research/wp-content/uploads/2011/10/ConsistencyAndBaseballReport.pdf> — "In other words, the reader sees a version of the data store that existed at the master at some time in the past."
[^a-t11-indep]: Terry, Replicated Data Consistency Explained Through Baseball (MSR 2011), mục 2 — <https://www.microsoft.com/en-us/research/wp-content/uploads/2011/10/ConsistencyAndBaseballReport.pdf> — "These last four read guarantees are all a form of eventual consistency but stronger than the eventual consistency model that is typically provided in systems like Amazon. None of these four guarantees is stronger than any of the others"
[^a-t94-mr]: Terry và cộng sự, Session Guarantees for Weakly Consistent Replicated Data (PDIS 1994), mục 3.2 — <https://www.cs.cornell.edu/courses/cs734/2000FA/cached%20papers/SessionGuaranteesPDIS_1.html> — "The Monotonic Reads guarantee permits users to observe a database that is increasingly up-to-date over time."
[^a-t94-ryw]: Terry và cộng sự, Session Guarantees for Weakly Consistent Replicated Data (PDIS 1994), mục 3.1 — <https://www.cs.cornell.edu/courses/cs734/2000FA/cached%20papers/SessionGuaranteesPDIS_1.html> — "The Read Your Writes guarantee is motivated by the fact that users and applications find it particularly confusing if they update a database and then immediately read from the database only to discover that the update appears to be missing."
[^a-t94-ws]: Terry và cộng sự, Session Guarantees for Weakly Consistent Replicated Data (PDIS 1994), mục 4 (Providing the guarantees) — <https://www.cs.cornell.edu/courses/cs734/2000FA/cached%20papers/SessionGuaranteesPDIS_1.html> — "Whenever a Write is accepted by a server, its assigned WID is added to the session's write-set. Before each Read to server S at time t, the session manager must check that the write-set is a subset of DB(S,t)."
[^a-v-sticky]: Vogels, Eventually Consistent — Revisited (2008) — <https://www.allthingsdistributed.com/2008/12/eventually_consistent.html> — "If this is the same server every time, then it is relatively easy to guarantee read-your-writes and monotonic reads. This makes it slightly harder to manage load balancing and fault tolerance, but it is a simple solution."
[^a-m-timeout]: MariaDB Documentation, Semisynchronous Replication (Configuring the Primary Timeout) — <https://mariadb.com/kb/en/semisynchronous-replication/> — "If the replica does not acknowledge the transaction before a certain amount of time has passed, then a timeout occurs and the primary switches to asynchronous replication."
[^b-quorum-def]: Kleppmann, Distributed Systems (Cambridge 2021/22), mục 5.2 Quorums — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "In general, a quorum is a minimum set of nodes that must respond to some request for it to be successful."
[^b-intersect]: Kleppmann, Distributed Systems (Cambridge 2021/22), mục 5.2 Quorums — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "the quorum for the write and the quorum for the read must have a non-empty intersection: in other words, the read quorum must contain at least one node that has acknowledged the write."
[^b-rw]: DeCandia và cộng sự, Dynamo (SOSP 2007), mục 4.5 Execution of get() and put() — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "R is the minimum number of nodes that must participate in a successful read operation. W is the minimum number of nodes that must participate in a successful write operation. Setting R and W such that R + W > N yields a quorum-like system."
[^b-config]: DeCandia và cộng sự, Dynamo (SOSP 2007), mục 6.1 Balancing Performance and Durability — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "The common (N,R,W) configuration used by several instances of Dynamo is (3,2,2)."
[^b-cass]: Apache Cassandra 4.1, Architecture: Dynamo (Picking Consistency Levels) — <https://cassandra.apache.org/doc/4.1/cassandra/architecture/dynamo.html> — "This is typically expressed in the same terms Dynamo does, in that W + R > RF, where W is the write consistency level, R is the read consistency level, and RF is the replication factor. For example, if RF = 3, a QUORUM request will require responses from at least 2/3 replicas."
[^b-sloppy]: DeCandia và cộng sự, Dynamo (SOSP 2007), mục 4.6 Handling Failures: Hinted Handoff — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "it does not enforce strict quorum membership and instead it uses a “sloppy quorum”; all read and write operations are performed on the first N healthy nodes from the preference list, which may not always be the first N nodes encountered while walking the consistent hashing ring."
[^b-hint]: DeCandia và cộng sự, Dynamo (SOSP 2007), mục 4.6 Handling Failures: Hinted Handoff — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "The replica sent to D will have a hint in its metadata that suggests which node was the intended recipient of the replica (in this case A). Nodes that receive hinted replicas will keep them in a separate local database that is scanned periodically. Upon detecting that A has recovered, D will attempt to deliver the replica to A."
[^b-pref]: DeCandia và cộng sự, Dynamo (SOSP 2007), mục 4.3 Replication — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "The list of nodes that is responsible for storing a particular key is called the preference list. The system is designed, as will be explained in Section 4.8, so that every node in the system can determine which nodes should be in this list for any particular key. To account for node failures, preference list contains more than N nodes."
[^b-strict]: DeCandia và cộng sự, Dynamo (SOSP 2007), mục 4.6 Handling Failures: Hinted Handoff — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "If Dynamo used a traditional quorum approach it would be unavailable during server failures and network partitions, and would have reduced durability even under the simplest of failure conditions."
[^b-rr-notes]: Kleppmann, Distributed Systems (Cambridge 2021/22), mục 5.2 Quorums — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "This process is called read repair. The client can perform read repair on any read request it makes, regardless of whether it was the client that originally performed the update in question."
[^b-rr-dynamo]: DeCandia và cộng sự, Dynamo (SOSP 2007), mục 5 Implementation (đoạn cuối, trước mục 6; read repair; câu mở đầu "After the read response has been returned to the caller the state" bị ngắt sang cột sau trong PDF) — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "machine waits for a small period of time to receive any outstanding responses. If stale versions were returned in any of the responses, the coordinator updates those nodes with the latest version. This process is called read repair because it repairs replicas that have missed a recent update at an opportunistic time and relieves the anti-entropy protocol from having to do it."
[^b-nonlin]: Kleppmann, Distributed Systems (Cambridge 2021/22), mục 7.2 Linearizability — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "Thus, client 3 observes an older value than client 2, even though the real-time order of operations would require client 3’s read to return a value that is no older than client 2’s result. This behaviour is not allowed in a linearizable system."
[^b-abd]: Kleppmann, Distributed Systems (Cambridge 2021/22), mục 7.2 Linearizability — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "A client must first send the get request to replicas, and wait for responses from a quorum. If some responses include a more recent value than other responses, as indicated by their timestamps, then the client must write back the most recent value to all replicas that did not already respond with the most recent value, like in read repair"
[^b-abd-wait]: Kleppmann, Distributed Systems (Cambridge 2021/22), mục 7.2 Linearizability — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "The get operation finishes only after the client is sure that the most recent value is stored on a quorum of replicas: that is, after a quorum of replicas either responded ok to the read repair, or replied with the most recent value in the first place."
[^b-vanish]: Apache Cassandra 4.1, Operating: Read repair — <https://cassandra.apache.org/doc/4.1/cassandra/operating/read_repair.html> — "When monotonic quorum reads are not provided and a write fails to reach a quorum of replicas, it may be visible in one read, and then disappear in a subsequent read."
[^b-hint-limit]: DeCandia và cộng sự, Dynamo (SOSP 2007), mục 4.6 Handling Failures: Hinted Handoff — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "Hinted handoff works best if the system membership churn is low and node failures are transient."
[^b-missing]: Kleppmann, Distributed Systems (Cambridge 2021/22), mục 5.2 Quorums — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "In this quorum approach to replication, some updates may be missing from some replicas at any given moment"
[^b-eventual]: DeCandia và cộng sự, Dynamo (SOSP 2007), mục 4.4 Data Versioning — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "Dynamo provides eventual consistency, which allows for updates to be propagated to all replicas asynchronously."
[^c-cluster-disabled]: Redis 7.2.5, src/cluster.c, clusterCommand — <https://raw.githubusercontent.com/redis/redis/7.2.5/src/cluster.c> — "This instance has cluster support disabled"
[^c-bt-short]: Bigtable paper, mục 2 Rows — <https://research.google.com/archive/bigtable-osdi06.pdf> — "reads of short row ranges are efficient and typically require communication with only a small number of machines."
[^c-bt-ts-hot]: Google Cloud Bigtable, Schema design — <https://cloud.google.com/bigtable/docs/schema-design> — "Row keys that start with a timestamp. This pattern causes sequential writes to be pushed onto a single node, creating a hotspot."
[^c-bt-ts-prefix]: Google Cloud Bigtable, Schema design — <https://cloud.google.com/bigtable/docs/schema-design> — "If you put a timestamp in a row key, precede it with a high-cardinality value like a user ID to avoid hotspots."
[^c-cass-naive]: Cassandra 4.1, Architecture → Dynamo — <https://cassandra.apache.org/doc/4.1/cassandra/architecture/dynamo.html> — "In this naive scheme, however, adding a single node might invalidate almost all of the mappings."
[^c-cass-rf-walk]: Cassandra 4.1, Architecture → Dynamo — <https://cassandra.apache.org/doc/4.1/cassandra/architecture/dynamo.html> — "until we encounter three distinct nodes, at which point we have found all the replicas of that key."
[^c-cass-vnode-cost]: Cassandra 4.1, Architecture → Dynamo — <https://cassandra.apache.org/doc/4.1/cassandra/architecture/dynamo.html> — "Every token introduces up to 2 * (RF - 1) additional neighbors on the token ring, which means that there are more combinations of node failures where we lose availability for a portion of the token ring."
[^c-cl-replicas-read]: Redis Cluster specification, Key distribution model — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "and that can be used in order to scale read operations where reading stale data is acceptable"
[^c-ddb-suffix]: Amazon DynamoDB Developer Guide, Using write sharding — <https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/bp-partition-key-sharding.html> — "You can add a random number to the partition key values to distribute the items among partitions."
[^c-ddb-suffix-read]: Amazon DynamoDB Developer Guide, Using write sharding — <https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/bp-partition-key-sharding.html> — "to read all the items for a given day, you would have to query the items for all the suffixes and then merge the results."
[^c-dyn-challenges]: Dynamo paper, mục 4.2 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "First, the random position assignment of each node on the ring leads to non-uniform data and load distribution."
[^c-dyn-efficiency]: Dynamo paper, mục 6.2 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "Load balancing efficiency is defined as the ratio of average number of requests served by each node to the maximum number of requests served by the hottest node."
[^c-dyn-ring]: Dynamo paper, mục 4.2 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "walking the ring clockwise to find the first node with a position larger than the item’s position."
[^c-dyn-s3-def]: Dynamo paper, mục 6.2 Strategy 3 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "each node is assigned Q/S tokens where S is the number of nodes in the system."
[^c-dyn-skew]: Dynamo paper (SOSP 2007), mục 6.1 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "Dynamo’s design assumes that even where there is a significant skew in the access distribution there are enough keys in the popular end of the distribution so that the load of handling popular keys can be spread across the nodes uniformly through partitioning."
[^c-dyn-vnode-adv]: Dynamo paper, mục 4.2 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "The number of virtual nodes that a node is responsible can decided based on its capacity, accounting for heterogeneity in the physical infrastructure."
[^c-dyn-vnodes]: Dynamo paper, mục 4.2 — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "instead of mapping a node to a single point in the circle, each node gets assigned to multiple points in the ring."
[^c-jump-1-6]: Lamping & Veach, Jump Consistent Hash, Introduction — <https://arxiv.org/pdf/1406.2294> — "But it is only necessary to move 1/6 of the data stored in the 10 shards in order to end up with the data balanced among 12 shards."
[^c-jump-alg]: Lamping & Veach, Jump Consistent Hash, mục 2 (Figure 1) — <https://arxiv.org/pdf/1406.2294> — "Its inputs are a 64 bit key and the number of buckets. It outputs a bucket number in the range [0, num_buckets)."
[^c-jump-rdv]: Lamping & Veach, Jump Consistent Hash, Related work — <https://arxiv.org/pdf/1406.2294> — "The only other algorithm we are aware of that computes a consistent hash is the rendezvous algorithm by Thaler and Ravishankar [3]. Used as a consistent hash, the original version of their algorithm takes a key, and for each candidate bucket, computes a hash function value h(key, bucket). It then returns the bucket for which the hash yielded the highest value. This requires time proportional to the number of buckets."
[^c-jump-seq]: Lamping & Veach, Jump Consistent Hash, Abstract — <https://arxiv.org/pdf/1406.2294> — "Its main limitation is that the buckets must be numbered sequentially, which makes it more suitable for data storage applications than for distributed web caching."
[^c-kar-1000]: Lamping & Veach, Jump Consistent Hash, Related work — <https://arxiv.org/pdf/1406.2294> — "experiments used 1000 points per bucket to get to a standard deviation of 3.2% in the number of keys assigned to different buckets."
[^c-kar-mono]: Karger et al., mục 4.1 (Monotonicity) — <https://people.csail.mit.edu/karger/Papers/web.pdf> — "an item may move from an old bucket to a new bucket, but not from one old bucket to another."
[^c-kar-smooth]: Karger et al., mục 4 — <https://people.csail.mit.edu/karger/Papers/web.pdf> — "When a machine is added to or removed from the set of caches, the expected fraction of objects that must be moved to a new cache is the minimum needed to maintain a balanced load across the caches."
[^c-keyslot-1]: Redis docs, CLUSTER KEYSLOT, Example — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/cluster-keyslot.md> — "CLUSTER KEYSLOT somekey (integer) 11058"
[^c-keyslot-2]: Redis docs, CLUSTER KEYSLOT, Example — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/cluster-keyslot.md> — "CLUSTER KEYSLOT foo{hash_tag} (integer) 2515 > CLUSTER KEYSLOT bar{hash_tag} (integer) 2515"
[^c-keyslot-desc]: Redis docs, CLUSTER KEYSLOT — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/cluster-keyslot.md> — "Returns an integer identifying the hash slot the specified key hashes to."
[^c-mongo-hash]: MongoDB 7.0, Hashed Sharding — <https://www.mongodb.com/docs/v7.0/core/hashed-sharding/> — "Hashed sharding provides a more even data distribution across the sharded cluster at the cost of reducing Targeted Operations vs. Broadcast Operations."
[^c-mongo-hash-mono]: MongoDB 7.0, Choose a Shard Key — <https://www.mongodb.com/docs/v7.0/core/sharding-choose-a-shard-key/> — "If your data model requires sharding on a key that changes monotonically, consider using Hashed Sharding."
[^c-mongo-mono-hot]: MongoDB 7.0, Hashed Sharding, Hashed vs Ranged Sharding — <https://www.mongodb.com/docs/v7.0/core/hashed-sharding/> — "Since the value of X is always increasing, the chunk with an upper bound of MaxKey receives the majority incoming writes."
[^c-redis-asks]: Redis Cluster specification, MOVED Redirection — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "A client must be also able to handle -ASK redirections that are described later in this document, otherwise it is not a complete Redis Cluster client."
[^c-redis-crc-spec]: Redis Cluster specification, Key distribution model — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "Name: XMODEM (also known as ZMODEM or CRC-16/ACORN)"
[^c-redis-crc-vec]: Redis Cluster specification, Key distribution model — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "Xor constant to output CRC: 0000 * Output for "123456789": 31C3"
[^c-redis-formula]: Redis Cluster specification, Key distribution model — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "HASH_SLOT = CRC16(key) mod 16384"
[^c-redis-no-ch]: Redis docs, Scale with Redis Cluster, Redis Cluster data sharding — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/management/scaling.md> — "Redis Cluster does not use consistent hashing, but a different form of sharding where every key is conceptually part of what we call a hash slot."
[^c-redis-reconf]: Redis Cluster specification, Live reconfiguration — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "Adding or removing a node is abstracted into the same operation: moving a hash slot from one node to another."
[^c-redis-slot-keys]: Redis Cluster specification, Live reconfiguration — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "Moving a hash slot means moving all the keys that happen to hash into this hash slot."
[^c-redis-tag-empty]: Redis Cluster specification, Hash tags — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "For the key foo{}{bar} the whole key will be hashed as usually since the first occurrence of { is followed by } on the right without characters in the middle."
[^c-redis-tag-ex]: Redis Cluster specification, Hash tags — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "The two keys {user1000}.following and {user1000}.followers will hash to the same hash slot since only the substring user1000 will be hashed in order to compute the hash slot."
[^c-redis-tag-nested]: Redis Cluster specification, Hash tags — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "For the key foo{{bar}}zap the substring {bar will be hashed, because it is the substring between the first occurrence of { and the first occurrence of } on its right."
[^c-redis-tag-rule]: Redis Cluster specification, Hash tags — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/reference/cluster-spec.md> — "Then instead of hashing the key, only what is between the first occurrence of { and the following first occurrence of } is hashed."
[^c-redis-tut-ranges]: Redis docs, Scale with Redis Cluster, Redis Cluster data sharding — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/management/scaling.md> — "Node A contains hash slots from 0 to 5500. * Node B contains hash slots from 5501 to 11000. * Node C contains hash slots from 11001 to 16383."
[^c-std-hash-hardcoded]: Rust 1.95.0 std, trait Hash, Portability — <https://doc.rust-lang.org/1.95.0/std/hash/trait.Hash.html> — "This means tests shouldn’t probe hard-coded hash values or data fed to a Hasher and instead should check consistency with Eq."
[^c-std-hash-portable]: Rust 1.95.0 std, trait Hash, Portability — <https://doc.rust-lang.org/1.95.0/std/hash/trait.Hash.html> — "Due to differences in endianness and type sizes, data fed by Hash to a Hasher should not be considered portable across platforms."
[^d-kb-check]: Kleppmann, "How to do distributed locking" (2016-02-08) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "You cannot fix this problem by inserting a check on the lock expiry just before writing back to storage."
[^d-pause-nonotice]: Kleppmann, Concurrent and Distributed Systems, mục 2.3 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "When an executing process or thread is paused, it normally does not notice it has been paused, unless it regularly checks the system clock to measure elapsed time."
[^d-pause-gc]: Kleppmann, Concurrent and Distributed Systems, mục 2.3 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "A real problem in memory-managed languages such as Java is that when the garbage collector runs, it needs to pause all running threads from time to time"
[^d-kb-sigstop]: Kleppmann, "How to do distributed locking" (2016-02-08) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "Maybe someone accidentally sent SIGSTOP to the process."
[^d-kb-fencing]: Kleppmann, "How to do distributed locking" (2016-02-08) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "the storage server remembers that it has already processed a write with a higher token number (34), and so it rejects the request with token 33"
[^d-kb-fencing-active]: Kleppmann, "How to do distributed locking" (2016-02-08) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "Note this requires the storage server to take an active role in checking tokens, and rejecting any writes on which the token has gone backwards."
[^d-kb-pause-lease]: Kleppmann, "How to do distributed locking" (2016-02-08) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "However, if the GC pause lasts longer than the lease expiry period, and the client doesn’t realise that it has expired, it may go ahead and make some unsafe change."
[^d-etcd-lease-clock]: etcd v3.5, Why etcd — <https://etcd.io/docs/v3.5/learning/why/> — "It allows a situation that the server revokes the lease but the client still claims it owns the lease."
[^d-etcd-lease-nomx]: etcd v3.5, Why etcd — <https://etcd.io/docs/v3.5/learning/why/> — "Actually, the lease mechanism itself doesn’t guarantee mutual exclusion."
[^d-kb-fencing-monotonic]: Kleppmann, "How to do distributed locking" (2016-02-08) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "And provided that the lock service generates strictly monotonically increasing tokens, this makes the lock safe."
[^d-repl-async]: Redis docs, Replication, Important facts about Redis replication — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/management/replication.md> — "Redis uses asynchronous replication, with asynchronous replica-to-master acknowledges of the amount of data processed."
[^d-lk-race-steps]: Redis docs, Distributed locks with Redis, Why failover-based implementations are not enough — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/manual/patterns/distributed-locks.md> — "Client A acquires the lock in the master. 2. The master crashes before the write to the key is transmitted to the replica. 3. The replica gets promoted to master. 4. Client B acquires the lock to the same resource A already holds a lock for. SAFETY VIOLATION!"
[^d-kb-counter]: Kleppmann, "How to do distributed locking" (2016-02-08; trang blog, truy cập 2026-10-12) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "Simply keeping a counter on one Redis node would not be sufficient, because that node may fail."
[^d-kb-fencing-consensus]: Kleppmann, "How to do distributed locking" (2016-02-08) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "It’s likely that you would need a consensus algorithm just to generate the fencing tokens."
[^d-kb-zk]: Kleppmann, "How to do distributed locking" (2016-02-08) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "if you are using ZooKeeper as lock service, you can use the zxid or the znode version number as fencing token"
[^d-etcd-fence]: etcd v3.5, Why etcd — <https://etcd.io/docs/v3.5/learning/why/> — "The authors interpret that fencing token is revision number in the case of etcd."
[^d-tod-jump]: Kleppmann, Concurrent and Distributed Systems, mục 3.2 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "This type of clock is therefore not suitable for measuring elapsed time."
[^d-cg-mono-nojump]: clock_gettime(2), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/clock_gettime.2.en.html> — "The CLOCK_MONOTONIC clock is not affected by discontinuous jumps in the system time (e.g., if the system administrator manually changes the clock), but is affected by the incremental adjustments performed by adjtime(3) and NTP."
[^d-mono-local]: Kleppmann, Concurrent and Distributed Systems, mục 3.2 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "When using a monotonic clock, only the difference between two timestamps from the same node is meaningful."
[^d-ch-lease-local]: Burrows, Chubby (OSDI 2006), mục 2.8 Sessions and KeepAlives — <https://research.google.com/archive/chubby-osdi06.pdf> — "The client maintains a local lease timeout that is a conservative approximation of the master’s lease timeout."
[^d-ch-clockrate]: Burrows, Chubby (OSDI 2006), mục 2.8 Sessions and KeepAlives — <https://research.google.com/archive/chubby-osdi06.pdf> — "to maintain consistency, we require that the server’s clock advance no faster than a known constant factor faster than the client’s."
[^d-k8s-local]: kubernetes/client-go v0.31.0, tools/leaderelection/leaderelection.go, package comment — <https://raw.githubusercontent.com/kubernetes/client-go/v0.31.0/tools/leaderelection/leaderelection.go> — "Thus the implementation is tolerant to arbitrary clock skew, but is not tolerant to arbitrary clock skew rate."
[^d-k8s-nofence]: kubernetes/client-go v0.31.0, tools/leaderelection/leaderelection.go, package comment — <https://raw.githubusercontent.com/kubernetes/client-go/v0.31.0/tools/leaderelection/leaderelection.go> — "This implementation does not guarantee that only one client is acting as a leader (a.k.a. fencing)."
[^d-kb-redis-clock]: Kleppmann, "How to do distributed locking" (2016-02-08) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "Note that Redis uses gettimeofday, not a monotonic clock, to determine the expiry of keys."
[^d-lk-disclaimer-clock]: Redis docs, Distributed locks with Redis, Disclaimer about consistency — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/manual/patterns/distributed-locks.md> — "Redis is not using monotonic clock for TTL expiration mechanism. That means that a wall-clock shift may result in a lock being acquired by more than one process."
[^d-kb-safety-liveness]: Kleppmann, "How to do distributed locking" (2016-02-08) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "Only liveness properties depend on timeouts or some other failure detector."
[^d-lk-no-assume-alive]: Redis docs, Distributed locks with Redis, Disclaimer about consistency — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/manual/patterns/distributed-locks.md> — "Extending locks' lifetime is also an option, but don´t assume that a lock is retained as long as the process that had acquired it is alive."
[^d-ch-lockdelay]: Burrows, Chubby (OSDI 2006), mục 2.4 Locks and sequencers — <https://research.google.com/archive/chubby-osdi06.pdf> — "if a lock becomes free because the holder has failed or become inaccessible, the lock server will prevent other clients from claiming the lock for a period called the lock-delay."
[^d-kb-two-reasons]: Kleppmann, "How to do distributed locking" (2016-02-08) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "Both are valid cases for wanting a lock, but you need to be very clear about which one of the two you are dealing with."
[^e-lamport-alg]: Kleppmann, Distributed Systems, §4.1 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "When the recipient receives a message, it moves its local Lamport clock forward to the timestamp in the message plus one; if the recipient’s clock is already ahead of the timestamp in the message, it is only incremented."
[^e-vc-alg]: Kleppmann, Distributed Systems, §4.1 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "the recipient merges the vector timestamp in the message with its local timestamp by taking the element-wise maximum of the two vectors, and then the recipient increments its own entry."
[^e-vc-map]: Kleppmann, Distributed Systems, §4.1 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "In practice, this vector is often implemented as a map from node IDs to integers rather than an array of integers."
[^e-siblings]: Kleppmann, Distributed Systems (Cambridge 2021/22), §5.1 Manipulating remote state — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "These concurrently written values are called conflicts, or sometimes siblings. The application can later merge conflicts back into a single value, as discussed in Lecture 8."
[^e-ntp-good]: Kleppmann, Concurrent and Distributed Systems, §3.2, slide 54 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "Reduces clock skew to a few milliseconds in good network conditions, but can be much worse!"
[^e-fifo]: Lamport 1978, Time, Clocks, and the Ordering of Events — <https://lamport.azurewebsites.net/pubs/time-clocks.pdf> — "are received in the same order as they are sent"
[^e-grant]: Lamport 1978, Time, Clocks, and the Ordering of Events — <https://lamport.azurewebsites.net/pubs/time-clocks.pdf> — "has received a message from every other process timestamped later than"
[^e-lamport-nodist]: Kleppmann, Distributed Systems, §4.1 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "Given the Lamport timestamps of two events, it is in general not possible to tell whether those events are concurrent or whether one happened before the other."
[^e-clockcond]: Lamport 1978, Time, Clocks, and the Ordering of Events — <https://lamport.azurewebsites.net/pubs/time-clocks.pdf> — "The strongest reasonable condition is that if an event a occurs before another event b, then a should happen at an earlier time than b."
[^e-converse]: Lamport 1978, Time, Clocks, and the Ordering of Events — <https://lamport.azurewebsites.net/pubs/time-clocks.pdf> — "Note that we cannot expect the converse condition to hold as well, since that would imply that any two concurrent events must occur at the same time."
[^e-lamport-prop]: Kleppmann, Distributed Systems, §4.1 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "Lamport timestamps have the property that if a happened before b, then b always has a greater timestamp than a; in other words, the timestamps are consistent with causality."
[^e-vc-hb]: Kleppmann, Distributed Systems, §4.1 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "The partial order over vector timestamps corresponds exactly to the partial order defined by the happens-before relation. Thus, the vector clock algorithm provides us with a mechanism for computing the happens-before relation in practice."
[^e-vc-rule]: DeCandia et al., Dynamo (SOSP 2007), §4.4 Data Versioning — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "If the counters on the first object’s clock are less-than-or-equal to all of the nodes in the second clock, then the first is an ancestor of the second and can be forgotten. Otherwise, the two changes are considered to be in conflict and require reconciliation."
[^e-unique]: Kleppmann, Distributed Systems, §4.1 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "If we need a unique timestamp for every event, each timestamp can be extended with the name or identifier of the node on which that event occurred."
[^e-total]: Lamport 1978, Time, Clocks, and the Ordering of Events — <https://lamport.azurewebsites.net/pubs/time-clocks.pdf> — "We can use a system of clocks satisfying the Clock Condition to place a total ordering on the set of all system events."
[^e-ts-wrong]: Kleppmann, Concurrent and Distributed Systems, §3.3 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "Thus, if we order messages based on their timestamps from time-of-day clocks, we might again end up with the wrong order."
[^e-ts-order]: Kleppmann, Concurrent and Distributed Systems, §3.3 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "The clock synchronisation performed by NTP and similar protocols always leaves some residual uncertainty about the exact skew between two clocks, especially if the network latency in the two directions is asymmetric."
[^e-dyn-lww]: DeCandia et al., Dynamo (SOSP 2007), §6 Experiences and lessons learned — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "In case of divergent versions, Dynamo performs simple timestamp based reconciliation logic of “last write wins”; i.e., the object with the largest physical timestamp value is chosen as the correct version."
[^e-cass-lww]: Apache Cassandra 4.1, Architecture: Dynamo (Dataset Partitioning) — <https://cassandra.apache.org/doc/4.1/cassandra/architecture/dynamo.html> — "Unlike in the original Dynamo paper where deterministic versions and vector clocks were used to reconcile concurrent updates to a key, Cassandra uses a simpler last write wins model where every mutation is timestamped (including deletes) and then the latest version of data is the "winning" value."
[^e-lww-loss]: Kleppmann, Concurrent and Distributed Systems, §5.1 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "This approach is simple to work with, but it does imply data loss when multiple updates are performed concurrently."
[^e-arb]: Kleppmann, Distributed Systems (Cambridge 2021/22), §5.1 Manipulating remote state — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "If we use Lamport clocks (with the total order defined on Slide 68), two concurrent updates will be ordered arbitrarily, depending on how the timestamps happen to get assigned."
[^e-d-context]: DeCandia et al., Dynamo (SOSP 2007), §4.4 Data Versioning — <https://www.allthingsdistributed.com/files/amazon-dynamo-sosp2007.pdf> — "An update using this context is considered to have reconciled the divergent versions and the branches are collapsed into a single new version."
[^e-tob-def]: Kleppmann, Distributed Systems, §4.2 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "If m1 is delivered before m2 on one node, then m1 must be delivered before m2 on all nodes"
[^e-smr]: Kleppmann, Distributed Systems, §5.3 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "Replica is a state machine: starts in fixed initial state, goes through same sequence of state transitions in the same order =⇒ all replicas end up in the same state"
[^e-smr-det]: Kleppmann, Distributed Systems, §5.3 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "We only require that the update logic is deterministic: any two replicas that are in the same state, and are given the same input, must end up in the same next state."
[^e-tob-naive]: Kleppmann, Distributed Systems, §4.3 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "neither of these approaches is fault tolerant: in both cases, the crash of a single node can stop all other nodes from being able to deliver messages."
[^e-halt]: Lamport 1978, Time, Clocks, and the Ordering of Events — <https://lamport.azurewebsites.net/pubs/time-clocks.pdf> — "A process must know all the commands issued by other processes, so that the failure of a single process will make it impossible for any other process to execute State Machine commands, thereby halting the system."
[^e-fail]: Lamport 1978, Time, Clocks, and the Ordering of Events — <https://lamport.azurewebsites.net/pubs/time-clocks.pdf> — "Without physical time, there is no way to distinguish a failed process from one which is just pausing between events."
[^e-equiv]: Kleppmann, Distributed Systems, §6.1 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "It has been formally shown that consensus and total order broadcast are equivalent to each other"
[^e-commute]: Kleppmann, Distributed Systems, §5.3 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "we want to ensure that replicas end up in the same state, no matter in which order concurrent updates are delivered, we need to make those updates commutative"
[^e-hybrid]: Kleppmann, Distributed Systems, §4.1 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "there are hybrid clocks that combine some of the properties of logical and physical clocks"
[^f-states]: Ongaro & Ousterhout, Raft, §5.1 — <https://raft.github.io/raft.pdf> — "At any given time each server is in one of three states: leader, follower, or candidate. In normal operation there is exactly one leader and all of the other servers are followers."
[^f-terms]: Ongaro & Ousterhout, Raft, §5.1 — <https://raft.github.io/raft.pdf> — "Terms are numbered with consecutive integers. Each term begins with an election"
[^f-rpcs]: Ongaro & Ousterhout, Raft, §5 — <https://raft.github.io/raft.pdf> — "RequestVote RPCs are initiated by candidates during elections (Section 5.2), and AppendEntries RPCs are initiated by leaders to replicate log entries and to provide a form of heartbeat (Section 5.3)."
[^f-vote]: Ongaro & Ousterhout, Raft, §5.2 — <https://raft.github.io/raft.pdf> — "Each server will vote for at most one candidate in a given term, on a first-come-first-served basis"
[^f-restr]: Ongaro & Ousterhout, Raft, §5.4.1 — <https://raft.github.io/raft.pdf> — "Raft uses the voting process to prevent a candidate from winning an election unless its log contains all committed entries."
[^f-consistency]: Ongaro & Ousterhout, Raft, §5.3 — <https://raft.github.io/raft.pdf> — "If the follower does not find an entry in its log with the same index and term, then it refuses the new entries."
[^f-nextindex]: Ongaro & Ousterhout, Raft, §5.3 — <https://raft.github.io/raft.pdf> — "After a rejection, the leader decrements nextIndex and retries the AppendEntries RPC."
[^f-prior]: Ongaro & Ousterhout, Raft, §5.4.2 — <https://raft.github.io/raft.pdf> — "Raft never commits log entries from previous terms by counting replicas."
[^f-range]: Ongaro & Ousterhout, Raft, §5.2 — <https://raft.github.io/raft.pdf> — "election timeouts are chosen randomly from a fixed interval (e.g., 150–300ms)"
[^f-broadcast]: Ongaro & Ousterhout, Raft, §5.6 — <https://raft.github.io/raft.pdf> — "The broadcast time should be an order of magnitude less than the election timeout so that leaders can reliably send the heartbeat messages"
[^f-persist]: Ongaro & Ousterhout, Raft, Hình 2 — <https://raft.github.io/raft.pdf> — "(Updated on stable storage before responding to RPCs)"
[^f-retry]: Ongaro & Ousterhout, Raft, §5.1 — <https://raft.github.io/raft.pdf> — "Servers retry RPCs if they do not receive a response in a timely manner, and they issue RPCs in parallel for best performance."
[^f-noop]: Ongaro & Ousterhout, Raft, §8 — <https://raft.github.io/raft.pdf> — "Raft handles this by having each leader commit a blank no-op entry into the log at the start of its term."
[^f-es]: Ongaro & Ousterhout, Raft, Hình 3 — <https://raft.github.io/raft.pdf> — "Election Safety: at most one leader can be elected in a given term."
[^f-append-only]: Ongaro & Ousterhout, Raft, §5.3 — <https://raft.github.io/raft.pdf> — "A leader never overwrites or deletes entries in its own log"
[^f-logmatch]: Ongaro & Ousterhout, Raft, §5.3 — <https://raft.github.io/raft.pdf> — "If two entries in different logs have the same index and term, then the logs are identical in all preceding entries."
[^f-lc]: Ongaro & Ousterhout, Raft, Hình 3 — <https://raft.github.io/raft.pdf> — "Leader Completeness: if a log entry is committed in a given term, then that entry will be present in the logs of the leaders for all higher-numbered terms."
[^f-sms]: Ongaro & Ousterhout, Raft, Hình 3 — <https://raft.github.io/raft.pdf> — "State Machine Safety: if a server has applied a log entry at a given index to its state machine, no other server will ever apply a different log entry for the same index."
[^f-fig3]: Ongaro & Ousterhout, Raft, Hình 3 — <https://raft.github.io/raft.pdf> — "Raft guarantees that each of these properties is true at all times."
[^f-snapshot]: Ongaro & Ousterhout, Raft, §7 — <https://raft.github.io/raft.pdf> — "Snapshotting is the simplest approach to compaction."
[^f-joint]: Ongaro & Ousterhout, Raft, §6 — <https://raft.github.io/raft.pdf> — "In Raft the cluster first switches to a transitional configuration we call joint consensus"
[^f-disrupt]: Ongaro & Ousterhout, Raft, §6 — <https://raft.github.io/raft.pdf> — "if a server receives a RequestVote RPC within the minimum election timeout of hearing from a current leader, it does not update its term or grant its vote."
[^f-read]: Ongaro & Ousterhout, Raft, §8 — <https://raft.github.io/raft.pdf> — "Read-only operations can be handled without writing anything into the log. However, with no additional measures, this would run the risk of returning stale data, since the leader responding to the request might have been superseded by a newer leader of which it is unaware."
[^f-etcd-disk]: etcd v3.5, FAQ — <https://etcd.io/docs/v3.5/faq/> — "This is intentional; disk latency is part of leader liveness."
[^f-majority]: Ongaro & Ousterhout, Raft, §5.2 — <https://raft.github.io/raft.pdf> — "The majority rule ensures that at most one candidate can win the election for a particular term"
[^f-random]: Ongaro & Ousterhout, Raft, §5.2 — <https://raft.github.io/raft.pdf> — "Raft uses randomized election timeouts to ensure that split votes are rare and that they are resolved quickly."
[^f-timing-ineq]: Ongaro & Ousterhout, Raft, §5.6 — <https://raft.github.io/raft.pdf> — "When the leader crashes, the system will be unavailable for roughly the election timeout"
[^f-stale-leader]: Kleppmann, Distributed Systems, §6.1 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "Node 1 may not even know that a new leader has been elected!"
[^f-stale-term]: Ongaro & Ousterhout, Raft, §5.1 — <https://raft.github.io/raft.pdf> — "they allow servers to detect obsolete information such as stale leaders."
[^f-force]: Ongaro & Ousterhout, Raft, §5.3 — <https://raft.github.io/raft.pdf> — "the leader handles inconsistencies by forcing the followers’ logs to duplicate its own."
[^f-split]: Ongaro & Ousterhout, Raft, §5.2 — <https://raft.github.io/raft.pdf> — "However, without extra measures split votes could repeat indefinitely."
[^f-eval-norandom]: Ongaro & Ousterhout, Raft, §9.3 — <https://raft.github.io/raft.pdf> — "In the absence of randomness, leader election consistently took longer than 10 seconds in our tests due to many split votes."
[^f-eval-5ms]: Ongaro & Ousterhout, Raft, §9.3 — <https://raft.github.io/raft.pdf> — "Adding just 5ms of randomness helps significantly, resulting in a median downtime of 287ms."
[^f-flp]: Fischer, Lynch & Paterson 1985, Abstract — <https://groups.csail.mit.edu/tds/papers/Lynch/jacm85.pdf> — "it is shown that every protocol for this problem has the possibility of nontermination, even with only one faulty process."
[^f-flp-clocks]: Kleppmann, Distributed Systems, §6.1 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "Paxos, Raft, etc. use clocks only used for timeouts/failure detector to ensure progress."
[^f-save-vote]: openraft 0.9.21, RaftLogStorage — <https://docs.rs/openraft/0.9.21/openraft/storage/trait.RaftLogStorage.html> — "The vote must be persisted on disk before returning."
[^f-fig8]: Ongaro & Ousterhout, Raft, Hình 8 — <https://raft.github.io/raft.pdf> — "has been replicated on a majority of the servers, but it is not committed. If S1 crashes as in (d), S5 could be elected leader (with votes from S2, S3, and S4) and overwrite the entry with its own entry from term 3."
[^f-uptodate]: Ongaro & Ousterhout, Raft, §5.4.1 — <https://raft.github.io/raft.pdf> — "If the logs have last entries with different terms, then the log with the later term is more up-to-date. If the logs end with the same term, then whichever log is longer is more up-to-date."
[^f-timing-safety]: Ongaro & Ousterhout, Raft, §5.6 — <https://raft.github.io/raft.pdf> — "One of our requirements for Raft is that safety must not depend on timing"
[^f-tlc-missed]: Ongaro, dissertation, correctness.tex @ 0c10ea1 — <https://raw.githubusercontent.com/ongardie/dissertation/0c10ea1b53d73bf6d163bdde8e4141a1cbd8f36a/correctness/correctness.tex> — "Murphi most likely missed this bug because of the constrained model size (fortunately, David \mazieres{} found it)."
[^f-aws-dynamo]: Newcombe et al., Use of Formal Methods at AWS — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf> — "This time the model checker found a bug that could lead to losing data if a particular sequence of failures and recovery steps was interleaved with other processing. This was a very subtle bug; the shortest error trace exhibiting the bug contained 35 high level steps."
[^f-aws-unnoticed]: Newcombe et al., Use of Formal Methods at AWS — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf> — "The bug had passed unnoticed through extensive design reviews, code reviews, and testing, and T.R. is convinced that we would not have found it by doing more work in those conventional areas."
[^f-aws-gap]: Newcombe et al., Use of Formal Methods at AWS — <https://lamport.azurewebsites.net/tla/formal-methods-amazon.pdf> — "On learning about TLA+, engineers usually ask, “How do we know that the executable code correctly implements the verified design?” The answer is that we don’t."
[^f-tla-spec]: Ongaro & Ousterhout, Raft, §9.2 — <https://raft.github.io/raft.pdf> — "makes the information summarized in Figure 2 completely precise using the TLA+ specification language"
[^f-cand-recognize]: Ongaro & Ousterhout, Raft, §5.2 — <https://raft.github.io/raft.pdf> — "then the candidate recognizes the leader as legitimate and returns to follower state."
[^f-readindex]: Ongaro & Ousterhout, Raft, §8 — <https://raft.github.io/raft.pdf> — "Raft handles this by having the leader exchange heartbeat messages with a majority of the cluster before responding to read-only requests."
[^f-ensure-lin]: openraft 0.9.21, Raft — <https://docs.rs/openraft/0.9.21/openraft/raft/struct.Raft.html> — "This method confirms the node’s leadership at the time of invocation by sending heartbeats to a quorum of followers, and the state machine is up to date."
[^f-dup]: Ongaro & Ousterhout, Raft, §8 — <https://raft.github.io/raft.pdf> — "The solution is for clients to assign unique serial numbers to every command."
[^f-leader-quorum]: Kleppmann, Distributed Systems, §6.1 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "Therefore, even though multiple leaders may exist at the same time, the old leaders will no longer be able to decide on any further messages to deliver, making the algorithm safe."
[^g-k-cons-amo]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "This corresponds to "at-most-once" semantics as in the case of a consumer failure messages may not be processed."
[^g-k-cons-alo]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "This corresponds to the "at-least-once" semantics in the case of consumer failure."
[^g-k-external]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "this can be handled more simply and generally by letting the consumer store its offset in the same place as its output. This is better because many of the output systems a consumer might want to write to will not support a two-phase commit."
[^g-k-offset-in-txn]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "The consumer's position is stored as a message in a topic, so we can write the offset to Kafka in the same transaction as the output topics receiving the processed data."
[^g-k-prod-unsure]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "If a producer attempts to publish a message and experiences a network error it cannot be sure if this error happened before or after the message was committed."
[^g-k-idem]: Apache Kafka 3.9.0, design (Message Delivery Semantics) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/design.html> — "the broker assigns each producer an ID and deduplicates messages using a sequence number that is sent by the producer along with every message."
[^g-kp-enable-idem]: Apache Kafka 3.9.0, producer configs (enable.idempotence) — <https://kafka.apache.org/39/configuration/producer-configs/> — "If 'false', producer retries due to broker failures, etc., may write duplicates of the retried message in the stream."
[^g-kp-retries-order]: Apache Kafka 3.9.0, producer configs (retries) — <https://kafka.apache.org/39/configuration/producer-configs/> — "will potentially change the ordering of records because if two batches are sent to a single partition, and the first fails and is retried but the second succeeds, then the records in the second batch may appear first."
[^g-kp-idem-order]: Apache Kafka 3.9.0, producer configs (enable.idempotence) — <https://kafka.apache.org/39/configuration/producer-configs/> — "Note that enabling idempotence requires max.in.flight.requests.per.connection to be less than or equal to 5 (with message ordering preserved for any allowable value), retries to be greater than 0, and acks must be 'all'."
[^g-cam-eo]: Kleppmann, Distributed Systems lecture notes (Cambridge), slide 89 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "Exactly-once semantics: retry + idempotence or deduplication"
[^g-cam-dedup]: Kleppmann, Distributed Systems lecture notes (Cambridge), slide 89 — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "in a crash-recovery system model, this requires storing requests (or some metadata about requests, such as a vector clock) in stable storage, so that duplicates can be accurately detected even after a crash."
[^g-kleppmann-dedup]: Kleppmann, Distributed Systems lecture notes (Cambridge, 2021–22), mục "Retrying state updates" — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "One way of preventing an update from taking effect multiple times is to deduplicate requests. However, in a crash-recovery system model, this requires storing requests (or some metadata about requests, such as a vector clock) in stable storage, so that duplicates can be accurately detected even after a crash."
[^g-idem-409]: Jena, Dalal, draft-ietf-httpapi-idempotency-key-header-07, mục 2.7 Error Handling — <https://www.ietf.org/archive/id/draft-ietf-httpapi-idempotency-key-header-07.txt> — "If the request is retried, while the original request is still being processed, the resource SHOULD reply with an HTTP 409 status code with body containing problem description."
[^g-idem-422]: Jena, Dalal, draft-ietf-httpapi-idempotency-key-header-07, mục 2.7 — <https://www.ietf.org/archive/id/draft-ietf-httpapi-idempotency-key-header-07.txt> — "If there is an attempt to reuse an idempotency key with a different request payload, the resource SHOULD reply with a HTTP 422 status code with body containing a link pointing to relevant documentation."
[^g-idem-retry]: Jena, Dalal, draft-ietf-httpapi-idempotency-key-header-07, mục 2.6 — <https://www.ietf.org/archive/id/draft-ietf-httpapi-idempotency-key-header-07.txt> — "The request was retried after the original request completed. The resource SHOULD respond with the result of the previously completed operation, success or an error."
[^g-idem-expired]: IETF Datatracker, trang draft-ietf-httpapi-idempotency-key-header — <https://datatracker.ietf.org/doc/draft-ietf-httpapi-idempotency-key-header/> — "This Internet-Draft is no longer active."
[^g-stripe-v1]: Stripe API Reference, "Idempotent requests" — <https://docs.stripe.com/api/idempotent_requests> — "For API v1, Stripe saves the status code and body of the first request made with a given idempotency key after endpoint execution begins. Retries with the same key return the saved response, including 500 errors."
[^g-msio-fail]: Richardson, microservices.io, "Pattern: Transactional outbox" — <https://microservices.io/patterns/data/transactional-outbox.html> — "But without using 2PC, sending a message in the middle of a transaction is not reliable. There’s no guarantee that the transaction will commit. Similarly, if a service sends a message after committing the transaction there’s no guarantee that it won’t crash before sending the message."
[^g-msio-benefit]: Richardson, microservices.io, "Pattern: Transactional outbox" — <https://microservices.io/patterns/data/transactional-outbox.html> — "Messages are guaranteed to be sent if and only if the database transaction commits"
[^g-msio-dup]: Richardson, microservices.io, "Pattern: Transactional outbox" — <https://microservices.io/patterns/data/transactional-outbox.html> — "The Message relay might publish a message more than once."
[^g-debezium-outbox-id]: Debezium 2.7 documentation, "Outbox Event Router", Table 1 — <https://debezium.io/documentation/reference/2.7/transformations/outbox-event-router.html> — "Contains the unique ID of the event. In an outbox message, this value is a header. You can use this ID, for example, to remove duplicate messages."
[^g-olep-exact-ext]: Kleppmann, Beresford, Svingen, "Online Event Processing", khung "Aside: Exactly-once semantics" — <https://martin.kleppmann.com/papers/olep-cacm.pdf> — "Thus, frameworks with exactly-once semantics still exhibit at-least-once processing when interacting with external storage and rely on idempotence to eliminate the effects of duplicate processing."
[^h-df-discarding]: Akidau et al., The Dataflow Model (VLDB 2015), 2.3 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "Discarding: Upon triggering, window contents are discarded, and later results bear no relation to previous results."
[^h-df-fast]: Akidau et al., The Dataflow Model (VLDB 2015), 2.3 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "They are sometimes too fast, meaning there may be late data that arrives behind the watermark."
[^h-df-fixed]: Akidau et al., The Dataflow Model (VLDB 2015), 1.2 Windowing — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "Fixed windows (sometimes called tumbling windows) are defined by a static window size"
[^h-df-fixed-special]: Akidau et al., The Dataflow Model (VLDB 2015), 1.2 Windowing — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "Fixed windows are really a special case of sliding windows where size equals period."
[^h-df-heur]: Akidau et al., The Dataflow Model (VLDB 2015), 1.3 (footnote 6) — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "For most real-world distributed data sets, the system lacks sufficient knowledge to establish a 100% correct watermark."
[^h-df-insufficient]: Akidau et al., The Dataflow Model (VLDB 2015), 2.3 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "For these reasons, we postulate that watermarks alone are insufficient."
[^h-df-skew]: Akidau et al., The Dataflow Model (VLDB 2015), 1.3 Time Domains — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "During processing, the realities of the systems in use (communication delays, scheduling algorithms, time spent processing, pipeline serialization, etc.) result in an inherent and dynamically changing amount of skew between the two domains."
[^h-df-sliding]: Akidau et al., The Dataflow Model (VLDB 2015), 1.2 Windowing — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "Sliding windows are defined by a window size and slide period, e.g. hourly windows starting every minute."
[^h-df-slow]: Akidau et al., The Dataflow Model (VLDB 2015), 2.3 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "Because they are a global progress metric, the watermark can be held back for the entire pipeline by a single slow datum."
[^h-df-trigger-modes]: Akidau et al., The Dataflow Model (VLDB 2015), 2.3 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "Accumulating: Upon triggering, window contents are left intact in persistent state, and later results become a refinement of previous results."
[^h-df-wm-trigger]: Akidau et al., The Dataflow Model (VLDB 2015), 2.4 — <https://www.vldb.org/pvldb/vol8/p1792-Akidau.pdf> — "The watermark trigger fires when the watermark passes the end of the window in question."
[^h-fl-bounded]: Apache Flink 1.20, Generating Watermarks — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/dev/datastream/event-time/generating_watermarks/> — "to use bounded-out-of-orderness watermarks and a lambda function as a timestamp assigner you use this"
[^h-fl-idle]: Apache Flink 1.20, Generating Watermarks (Dealing With Idle Sources) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/dev/datastream/event-time/generating_watermarks/> — "In that case, the watermark will be held back, because it is computed as the minimum over all the different parallel watermarks."
[^h-fl-idle2]: Apache Flink 1.20, Generating Watermarks (Dealing With Idle Sources) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/dev/datastream/event-time/generating_watermarks/> — "To deal with this, you can use a WatermarkStrategy that will detect idleness and mark an input as idle."
[^h-fl-idle3]: Apache Flink 1.20, Generating Watermarks (Dealing With Idle Sources) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/dev/datastream/event-time/generating_watermarks/> — "WatermarkStrategy provides a convenience helper for this"
[^h-fl-late]: Apache Flink 1.20, Event Time and Watermarks (Late Elements) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/concepts/time/> — "Late elements are elements that arrive after the system's event time clock (as signaled by the watermarks) has already passed the time of the late element's timestamp."
[^h-fl-late-drop]: Apache Flink 1.20, Windows (Allowed Lateness) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/dev/datastream/operators/windows/> — "By default, the allowed lateness is set to 0. That is, elements that arrive behind the watermark will be dropped."
[^h-fl-refire]: Apache Flink 1.20, Windows (Allowed Lateness) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/dev/datastream/operators/windows/> — "Depending on the trigger used, a late but not dropped element may cause the window to fire again. This is the case for the EventTimeTrigger."
[^h-fl-lateness]: Apache Flink 1.20, Windows (Allowed Lateness) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/dev/datastream/operators/windows/> — "Elements that arrive after the watermark has passed the end of the window but before it passes the end of the window plus the allowed lateness, are still added to the window."
[^h-fl-wm-def]: Apache Flink 1.20, Event Time and Watermarks — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/concepts/time/> — "A Watermark(t) declares that event time has reached time t in that stream, meaning that there should be no more elements from the stream with a timestamp t' <= t"
[^h-fl-wm-delay]: Apache Flink 1.20, Event Time and Watermarks (Late Elements) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/concepts/time/> — "even if the lateness can be bounded, delaying the watermarks by too much is often not desirable, because it causes too much delay in the evaluation of event time windows."
[^h-fl-wm-min]: Apache Flink 1.20, Event Time and Watermarks (Watermarks in Parallel Streams) — <https://nightlies.apache.org/flink/flink-docs-release-1.20/docs/concepts/time/> — "Such an operator's current event time is the minimum of its input streams' event times."
[^h-ks-event-time]: Apache Kafka 3.9.0, Kafka Streams core concepts (Time) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "The point in time when an event or data record occurred, i.e. was originally created "at the source"."
[^h-ks-grace]: Apache Kafka 3.9.0, Kafka Streams core concepts (Windowing) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "This grace period controls how long Kafka Streams will wait for"
[^h-ks-grace2]: Apache Kafka 3.9.0, Kafka Streams core concepts (Windowing) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "data records for a given window. If a record arrives after the grace period of a window has passed, the record is discarded and will not be processed in that window."
[^h-ks-proc-time]: Apache Kafka 3.9.0, Kafka Streams core concepts (Time) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "The point in time when the event or data record happens to be processed by the stream processing application, i.e. when the record is being consumed."
[^h-ks-tradeoff]: Apache Kafka 3.9.0, Kafka Streams core concepts (Out-of-Order Handling) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/core-concepts.html> — "making trade-off decisions between latency, cost, and correctness"
[^h-ksdsl-hop2]: Apache Kafka 3.9.0, Kafka Streams DSL (Hopping time windows) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/developer-guide/dsl-api.html> — "you can configure a hopping window with a size 5 minutes and an advance interval of 1 minute"
[^h-ksdsl-tumb]: Apache Kafka 3.9.0, Kafka Streams DSL (Tumbling time windows) — <https://raw.githubusercontent.com/apache/kafka/3.9.0/docs/streams/developer-guide/dsl-api.html> — "A tumbling window is a hopping window whose window size is equal to its advance interval. Since tumbling windows never overlap, a data record will belong to one and only one window."
[^i-debug-default]: Redis 7.2.5, redis.conf, SECURITY — <https://raw.githubusercontent.com/redis/redis/7.2.5/redis.conf> — "enable-protected-configs no enable-debug-command no enable-module-command no"
[^i-debug-local]: Redis 7.2.5, redis.conf, SECURITY — <https://raw.githubusercontent.com/redis/redis/7.2.5/redis.conf> — "local - Allow only for local connections. Ones originating from the IPv4 address (127.0.0.1), IPv6 address (::1) or Unix domain sockets."
[^i-lat-seq]: Redis docs, Latency diagnosis (redis-doc commit 1044224), Single threaded nature of Redis — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/management/optimization/latency.md> — "This means that Redis can serve a single request in every given moment, so all the requests are served sequentially."
[^i-keys-40ms]: Redis docs, KEYS command — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/keys.md> — "While the time complexity for this operation is O(N), the constant times are fairly low. For example, Redis running on an entry level laptop can scan a 1 million key database in 40 milliseconds."
[^i-sent-order]: Redis docs, Sentinel, Replica selection and priority — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/management/sentinel.md> — "The replicas are sorted by replica-priority as configured in the redis.conf file of the Redis instance. A lower priority will be preferred. 2. If the priority is the same, the replication offset processed by the replica is checked, and the replica that received more data from the master is selected."
[^i-wait-impl]: Redis docs, WAIT command, Implementation details — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/wait.md> — "In the specific case of the implementation of WAIT, Redis remembers, for each client, the replication offset of the produced replication stream when a given write command was executed in the context of a given client. When WAIT is called Redis checks if the specified number of replicas already acknowledged this offset or a greater one."
[^i-sent-select]: Redis docs, Sentinel, Replica selection and priority — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/management/sentinel.md> — "A replica that is found to be disconnected from the master for more than ten times the configured master timeout (down-after-milliseconds option), plus the time the master is also not available from the point of view of the Sentinel doing the failover, is considered to be not suitable for the failover and is skipped."
[^i-scan-dup]: Redis docs, SCAN command, Scan guarantees — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/scan.md> — "A given element may be returned multiple times. It is up to the application to handle the case of duplicated elements, for example only using the returned elements in order to perform operations that are safe when re-applied multiple times."
[^i-lat-slow]: Redis docs, Latency diagnosis, Latency generated by slow commands — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/management/optimization/latency.md> — "A consequence of being single thread is that when a request is slow to serve all the other clients will wait for this request to be served."
[^i-keys-warn]: Redis docs, KEYS command — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/keys.md> — "Warning: consider KEYS as a command that should only be used in production environments with extreme care."
[^i-del-cx]: Redis 7.2.5, src/commands/del.json — <https://raw.githubusercontent.com/redis/redis/7.2.5/src/commands/del.json> — "When a key to remove holds a value other than a string, the individual complexity for this key is O(M) where M is the number of elements in the list, set, sorted set or hash."
[^i-scan-count]: Redis docs, SCAN command, The COUNT option — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/scan.md> — "Basically with COUNT the user specifies the amount of work that should be done at every call in order to retrieve elements from the collection."
[^i-scan-prod]: Redis docs, SCAN command — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/scan.md> — "they can be used in production without the downside of commands like KEYS or SMEMBERS that may block the server for a long time (even several seconds) when called against big collections of keys or elements."
[^i-unlink-doc]: Redis docs, UNLINK command — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/unlink.md> — "However the command performs the actual memory reclaiming in a different thread, so it is not blocking, while DEL is."
[^i-lazyfree]: Redis 7.2.5, src/lazyfree.c, lazyfreeGetFreeEffort — <https://raw.githubusercontent.com/redis/redis/7.2.5/src/lazyfree.c> — "So under a certain limit we just free the object synchronously."
[^i-incr-atomic]: Redis docs, INCR command, Pattern: Counter — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/incr.md> — "The counter pattern is the most obvious thing you can do with Redis atomic increment operations."
[^i-sc-atomic]: Redis docs, Scripting with Lua (eval-intro) — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/interact/programmability/eval-intro.md> — "Redis guarantees the script's atomic execution. While executing the script, all server activities are blocked during its entire runtime. These semantics mean that all of the script's effects either have yet to happen or had already happened."
[^i-tx-watch]: Redis docs, Transactions, Optimistic locking using check-and-set — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/interact/transactions.md> — "WATCH is used to provide a check-and-set (CAS) behavior to Redis transactions. WATCHed keys are monitored in order to detect changes against them. If at least one watched key is modified before the EXEC command, the whole transaction aborts"
[^i-tx-watch-retry]: Redis docs, Transactions, Optimistic locking using check-and-set — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/interact/transactions.md> — "We just have to repeat the operation hoping this time we'll not get a new race. This form of locking is called optimistic locking."
[^i-tx-usage]: Redis docs, Transactions, Usage — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/interact/transactions.md> — "Instead of executing these commands, Redis will queue them. All the commands are executed once EXEC is called."
[^i-tx-serial]: Redis docs, Transactions — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/interact/transactions.md> — "All the commands in a transaction are serialized and executed sequentially. A request sent by another client will never be served in the middle of the execution of a Redis Transaction."
[^i-tx-runtime-err]: Redis docs, Transactions, Errors inside a transaction — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/interact/transactions.md> — "Errors happening after EXEC instead are not handled in a special way: all the other commands will be executed even if some command fails during the transaction."
[^i-tx-no-rollback]: Redis docs, Transactions, What about rollbacks — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/interact/transactions.md> — "Redis does not support rollbacks of transactions since supporting rollbacks would have a significant impact on the simplicity and performance of Redis."
[^i-repl-async-default]: Redis docs, Replication, Important facts about Redis replication — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/management/replication.md> — "Redis uses by default asynchronous replication, which being low latency and high performance, is the natural replication mode for the vast majority of Redis use cases."
[^i-repl-async]: Redis docs, Replication, Important facts about Redis replication — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/management/replication.md> — "Redis uses asynchronous replication, with asynchronous replica-to-master acknowledges of the amount of data processed."
[^i-repl-wait-cp]: Redis docs, Replication — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/management/replication.md> — "it does not turn a set of Redis instances into a CP system with strong consistency: acknowledged writes can still be lost during a failover, depending on the exact configuration of the Redis persistence."
[^i-wait-besteffort]: Redis docs, WAIT command, Consistency and WAIT — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/wait.md> — "However this is just a best-effort attempt so it is possible to still lose a write synchronously replicated to multiple replicas."
[^i-sent-lfw]: Redis docs, Sentinel, Consistency under partitions — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/management/sentinel.md> — "In general Redis + Sentinel as a whole are an eventually consistent system where the merge function is last failover wins, and the data from old masters are discarded to replicate the data of the current master, so there is always a window for losing acknowledged writes."
[^i-wait-conn]: Redis docs, WAIT command — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/wait.md> — "When WAIT returns, all the previous write commands sent in the context of the current connection are guaranteed to be received by the number of replicas returned by WAIT."
[^i-wait-doc]: Redis docs, WAIT command — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/wait.md> — "This command blocks the current client until all the previous write commands are successfully transferred and acknowledged by at least the specified number of replicas."
[^i-debug-sleep]: Redis docs, Sentinel, Testing the failover — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/management/sentinel.md> — "This command will make our master no longer reachable, sleeping for 30 seconds. It basically simulates a master hanging for some reason."
[^i-slowlog-unit]: Redis 7.2.5, redis.conf, SLOWLOG — <https://raw.githubusercontent.com/redis/redis/7.2.5/redis.conf> — "The following time is expressed in microseconds, so 1000000 is equivalent to one second."
[^i-slowlog-blocked]: Redis docs, SLOWLOG GET command — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/slowlog-get.md> — "this is the only stage of command execution where the thread is blocked and can not serve other requests in the meantime"
[^i-slowlog]: Redis docs, SLOWLOG GET command — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/slowlog-get.md> — "The execution time does not include I/O operations like talking with the client, sending the reply and so forth"
[^i-info-offset]: Redis docs, INFO command, Replication — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/info.md> — "master_repl_offset: The server's current replication offset"
[^j-rl-pattern]: Redis docs, INCR command, Pattern: Rate limiter — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/incr.md> — "The rate limiter pattern is a special counter that is used to limit the rate at which an operation can be performed."
[^j-rl-multi]: Redis docs, INCR command, Pattern: Rate limiter 1 — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/incr.md> — "Note the used of MULTI and EXEC in order to make sure that we'll both increment and set the expire at every API call."
[^j-rl-race]: Redis docs, INCR command, Pattern: Rate limiter 2 — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/incr.md> — "If for some reason the client performs the INCR command but does not perform the EXPIRE the key will be leaked until we'll see the same IP address again."
[^j-rl-lua]: Redis docs, INCR command, Pattern: Rate limiter 2 — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/incr.md> — "This can be fixed easily turning the INCR with optional EXPIRE into a Lua script that is send using the EVAL command"
[^j-expire-nx]: Redis docs, EXPIRE command, Options — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/expire.md> — "NX -- Set expiry only when the key has no expiry"
[^j-expire-nx-since]: Redis 7.2.5, src/commands/expire.json — <https://raw.githubusercontent.com/redis/redis/7.2.5/src/commands/expire.json> — "Added options: `NX`, `XX`, `GT` and `LT`."
[^j-expire-untouched]: Redis docs, EXPIRE command — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/expire.md> — "For instance, incrementing the value of a key with INCR, pushing a new value into a list with LPUSH, or altering the field value of a hash with HSET are all operations that will leave the timeout untouched."
[^j-expire-refresh]: Redis docs, EXPIRE command, Refreshing expires — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/expire.md> — "It is possible to call EXPIRE using as argument a key that already has an existing expire set. In this case the time to live of a key is updated to the new value."
[^j-tx-serial]: Redis docs, Transactions — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/interact/transactions.md> — "All the commands in a transaction are serialized and executed sequentially. A request sent by another client will never be served in the middle of the execution of a Redis Transaction."
[^j-tx-exec-all]: Redis docs, Transactions — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/interact/transactions.md> — "if a client loses the connection to the server in the context of a transaction before calling the EXEC command none of the operations are performed, instead if the EXEC command is called, all the operations are performed."
[^j-tx-usage]: Redis docs, Transactions, Usage — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/interact/transactions.md> — "Instead of executing these commands, Redis will queue them. All the commands are executed once EXEC is called."
[^j-sc-atomic]: Redis docs, Scripting with Lua (eval-intro) — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/interact/programmability/eval-intro.md> — "Redis guarantees the script's atomic execution. While executing the script, all server activities are blocked during its entire runtime. These semantics mean that all of the script's effects either have yet to happen or had already happened."
[^j-sc-time-free]: Redis docs, Scripting with Lua (eval-intro), Replicating commands instead of scripts — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/interact/programmability/eval-intro.md> — "When script effects replication is enabled, the restrictions on non-deterministic functions are removed. You can, for example, use the TIME or SRANDMEMBER commands inside your scripts freely at any place."
[^j-time-cmd]: Redis docs, TIME command — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/time.md> — "The TIME command returns the current server time as a two items lists: a Unix timestamp and the amount of microseconds already elapsed in the current second."
[^j-zset-update]: Redis docs, Sorted sets, Basic commands — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/data-types/sorted-sets.md> — "ZADD adds a new member and associated score to a sorted set. If the member already exists, the score is updated."
[^j-zrem-doc]: Redis docs, ZREMRANGEBYSCORE command — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/zremrangebyscore.md> — "Removes all elements in the sorted set stored at key with a score between min and max (inclusive)."
[^j-hash-def]: Redis docs, Redis hashes — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/data-types/hashes.md> — "Redis hashes are record types structured as collections of field-value pairs."
[^k-az-order]: Microsoft Azure Architecture Center, Cache-Aside pattern, Example (ghi chú sau UpdateEntityAsync) — <https://learn.microsoft.com/en-us/azure/architecture/patterns/cache-aside> — "The order of the steps is important. Update the data store before removing the item from the cache."
[^k-az-stale]: Microsoft Azure Architecture Center, Cache-Aside pattern, Problems and considerations — <https://learn.microsoft.com/en-us/azure/architecture/patterns/cache-aside> — "Between the write and the next read, a reader can experience a cache miss or briefly see stale data."
[^k-az-ttl]: Microsoft Azure Architecture Center, Cache-Aside pattern, Problems and considerations — <https://learn.microsoft.com/en-us/azure/architecture/patterns/cache-aside> — "To make cache-aside effective, ensure that the expiration policy matches the pattern of access for applications that use the data."
[^k-fb-herd-rate]: Nishtala et al., Scaling Memcache at Facebook (NSDI 2013), mục 3.2.1 Leases — <https://www.usenix.org/system/files/conference/nsdi13/nsdi13-final170_update.pdf> — "Each memcached server regulates the rate at which it returns tokens. By default, we configure these servers to return a token only once every 10 seconds per key."
[^k-fb-herd-wait]: Nishtala et al., Scaling Memcache at Facebook (NSDI 2013), mục 3.2.1 Leases — <https://www.usenix.org/system/files/conference/nsdi13/nsdi13-final170_update.pdf> — "Requests for a key’s value within 10 seconds of a token being issued results in a special notification telling the client to wait a short amount of time. Typically, the client with the lease will have successfully set the data within a few milliseconds."
[^k-fb-lease]: Nishtala et al., Scaling Memcache at Facebook (NSDI 2013), mục 3.2.1 Leases — <https://www.usenix.org/system/files/conference/nsdi13/nsdi13-final170_update.pdf> — "The lease is a 64-bit token bound to the specific key the client originally requested. The client provides the lease token when setting the value in the cache. With the lease token, memcached can verify and determine whether the data should be stored and thus arbitrate concurrent writes. Verification can fail if memcached has invalidated the lease token due to receiving a delete request for that item."
[^k-fb-stale]: Nishtala et al., Scaling Memcache at Facebook (NSDI 2013), mục 3.2.1 Leases — <https://www.usenix.org/system/files/conference/nsdi13/nsdi13-final170_update.pdf> — "A stale set occurs when a web server sets a value in memcache that does not reflect the latest value that should be cached. This can occur when concurrent updates to memcache get reordered."
[^k-kb-efficiency]: Kleppmann, "How to do distributed locking" (2016-02-08; trang blog, truy cập 2026-10-12) — <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html> — "I would recommend sticking with the straightforward single-node locking algorithm for Redis (conditional set-if-not-exists to obtain a lock, atomic delete-if-value-matches to release a lock), and documenting very clearly in your code that the locks are only approximate and may occasionally fail."
[^k-lk-del-unsafe]: Redis docs, Distributed locks with Redis, Correct implementation with a single instance — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/manual/patterns/distributed-locks.md> — "Using just DEL is not safe as a client may remove another client's lock."
[^k-lk-disclaimer-fence]: Redis docs, Distributed locks with Redis, Disclaimer about consistency — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/manual/patterns/distributed-locks.md> — "You should implement fencing tokens. This is especially important for processes that can take significant time and applies to any distributed locking system."
[^k-lk-set]: Redis docs, Distributed locks with Redis, Correct implementation with a single instance — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/manual/patterns/distributed-locks.md> — "The command will set the key only if it does not already exist (NX option), with an expire of 30000 milliseconds (PX option). The key is set to a value “my\_random\_value”. This value must be unique across all clients and all lock requests."
[^k-lk-unlock]: Redis docs, Distributed locks with Redis, Correct implementation with a single instance — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/manual/patterns/distributed-locks.md> — "Basically the random value is used in order to release the lock in a safe way, with a script that tells Redis: remove the key only if it exists and the value stored at the key is exactly the one I expect to be."
[^k-lk-window]: Redis docs, Distributed locks with Redis, Correct implementation with a single instance — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/docs/manual/patterns/distributed-locks.md> — "It is both the auto release time, and the time the client has in order to perform the operation required before another client may be able to acquire the lock again, without technically violating the mutual exclusion guarantee"
[^k-pttl]: Redis docs, PTTL command — <https://raw.githubusercontent.com/redis/redis-doc/1044224477fceacc5af56a636b0a7495d19e533c/commands/pttl.md> — "TTL returns the amount of remaining time in seconds while PTTL returns it in milliseconds."
[^k-rfc2308-def]: RFC 2308, mục 1 — <https://www.rfc-editor.org/rfc/rfc2308.html> — "It is the storage of knowledge that something does not exist, cannot or does not give an answer that we call negative caching."
[^k-rfc2308-use]: RFC 2308, Abstract — <https://www.rfc-editor.org/rfc/rfc2308.html> — "Negative caching is useful as it reduces the response time for negative answers."
[^k-xf-beta]: Vattani, Chierichetti, Lowenstein, Optimal Probabilistic Cache Stampede Prevention (VLDB 2015), Figure 3 — <https://www.vldb.org/pvldb/vol8/p886-vattani.pdf> — "The parameter β defaults to 1 and already provides effective prevention against cache stampedes. It can be increased for even better guarantees against stampedes, if earlier expirations are not a concern."
[^k-xf-def]: Vattani, Chierichetti, Lowenstein, Optimal Probabilistic Cache Stampede Prevention (VLDB 2015), mục 1 — <https://www.vldb.org/pvldb/vol8/p886-vattani.pdf> — "a situation that occurs when a popular cache item expires, leading to multiple requests seeing a cache miss and regenerating that same item at the same time."
[^k-xf-early]: Vattani, Chierichetti, Lowenstein, Optimal Probabilistic Cache Stampede Prevention (VLDB 2015), mục 1 — <https://www.vldb.org/pvldb/vol8/p886-vattani.pdf> — "Probabilistic early expiration: Each individual request may regenerate the item in cache before its expiration by making an independent probabilistic decision."
[^k-xf-formula]: Vattani, Chierichetti, Lowenstein, Optimal Probabilistic Cache Stampede Prevention (VLDB 2015), Figure 3 — <https://www.vldb.org/pvldb/vol8/p886-vattani.pdf> — "if !value or Time() − ∆β log(rand()) ≥ expiry then"
[^k-xf-nolock]: Vattani, Chierichetti, Lowenstein, Optimal Probabilistic Cache Stampede Prevention (VLDB 2015), mục 1 — <https://www.vldb.org/pvldb/vol8/p886-vattani.pdf> — "primitive get/set operations, and which do not provide locking mechanisms, or protection against stampedes"
[^k-xf-size]: Vattani, Chierichetti, Lowenstein, Optimal Probabilistic Cache Stampede Prevention (VLDB 2015), mục 1 — <https://www.vldb.org/pvldb/vol8/p886-vattani.pdf> — "if the cache item is accessed 10 times per second, and the recomputation of the item takes 3 seconds, then 30 requests will recompute the item."
