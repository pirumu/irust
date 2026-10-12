# Lab P12a — Hệ thống dữ liệu: một node

> Đây là phần thực hành của [P12 · Hệ thống dữ liệu](../12-he-thong-du-lieu/00-tong-quan.md) cho các chương xoay quanh **một máy**: [độ tin cậy, khả năng mở rộng, bảo trì](../12-he-thong-du-lieu/01-reliability-scalability-maintainability.md), [mô hình dữ liệu](../12-he-thong-du-lieu/02-mo-hinh-du-lieu.md), [storage engine](../12-he-thong-du-lieu/03-storage-engine.md), [encoding và tiến hóa schema](../12-he-thong-du-lieu/04-encoding-va-tien-hoa-schema.md), [transaction](../12-he-thong-du-lieu/07-transaction.md), [batch processing](../12-he-thong-du-lieu/10-batch-processing.md) và [PostgreSQL bên trong](../12-he-thong-du-lieu/15-postgresql-ben-trong.md). Hãy đọc lý thuyết trước. Mọi khẳng định về hệ thống thật đều có footnote và được `tools/verify_citations.py` kiểm.
>
> Các block output bên dưới là output thật của `cargo run` trên **macOS aarch64, rustc 1.95.0**; giá trị phụ thuộc máy (thời gian, GB/s, đường dẫn thư mục tạm) được ghi rõ. **Chưa chạy**: toàn bộ phần PostgreSQL thật (Lab (h), cần Docker và `psql`) và mọi so sánh với Linux. Các mục đó chỉ là lệnh cho người đọc cùng kỳ vọng suy ra từ tài liệu đã dẫn, không phải output đã quan sát. Con số của lab là kết quả của **mô hình nhỏ tự viết**, không phải số đo của RocksDB hay PostgreSQL.

## Chuẩn bị và các bài lab

Crate `labs/crates/lab-data` (Rust edition 2024) **không có dependency ngoài**, chỉ dùng `std`: CRC-32C, Bloom filter, SSTable, LSM, MVCC, external sort… đều tự viết để mọi con số truy được tới dòng code. Các module dùng `pread` và `fsync` được bọc bằng `#[cfg(unix)]`; trên nền không phải Unix các bin in cách chạy rồi thoát.

```bash
cd labs
cargo build -p lab-data --all-targets
cargo test  -p lab-data            # 120 test, gồm cả test phản ví dụ (đuôi hỏng, tombstone rơi sớm, schedule không tuần tự hóa được)
```

| Lab | Bin | Lệnh (từ thư mục `labs/`) | Thực hành cho |
|---|---|---|---|
| (a) | `kvlog` | `cargo run -q -p lab-data --bin kvlog` | log nối đuôi + chỉ mục băm, đuôi bị xé, compaction (chương 03) |
| (b) | `lsm` | `cargo run -q --release -p lab-data --bin lsm` | LSM-tree, Bloom filter, chính sách compaction, so với page store (chương 03) |
| (c) | `layout_scan` | `cargo run -q --release -p lab-data --bin layout_scan` | row store và column store (chương 03) |
| (d) | `encoding` | `cargo run -q -p lab-data --bin encoding` | varint, tag, tiến hóa schema (chương 04) |
| (e) | `mvcc` | `cargo run -q --release -p lab-data --bin mvcc [số trial]` | xmin/xmax, snapshot, ma trận bất thường, fuzz (chương 07, 15) |
| (f) | `batch` | `cargo run -q --release -p lab-data --bin batch` | external sort, shuffle, combiner, join (chương 10) |
| (g) | `percentile` | `cargo run -q --release -p lab-data --bin percentile` | percentile, fan-out, hedging, coordinated omission (chương 01) |
| (h) | `sql/*.sql` | `docker exec -i lab-pg psql …` (chưa chạy) | cùng hiện tượng trên PostgreSQL 16 (chương 07, 15) |

**Chương 02 (mô hình dữ liệu) không có bin riêng.** Lý do: quan hệ, document, đồ thị là các hình dạng *logic* của dữ liệu, không có đại lượng nào để đo; thứ đo được là chi phí vật lý của chúng, và phần đó nằm ở Lab (c) (layout), Lab (d) (schema trên dây) và Lab (h) (PostgreSQL). Chương 02 vẫn là nền cho Lab (d), nơi tên trường biến mất khỏi byte và chỉ còn số tag.

**Cách đọc output.** Các giá trị có đánh dấu `[machine-dependent]` hoặc là thời gian/GB/s sẽ đổi theo máy. Các con số còn lại (byte, số bản ghi, tỉ lệ) được sinh từ bộ sinh số ngẫu nhiên có hạt giống cố định nên lặp lại được; chữ số cuối của vài phép tính dấu phẩy động (log-normal trong Lab (g)) có thể khác trên nền tảng có `libm` khác. Dòng `scratch dir: …` đầu một số bin (đường dẫn thư mục tạm) đã được bỏ khỏi các block bên dưới vì mỗi lần chạy một khác.

**Những gì lab cố ý không làm** (nhận xét của người soạn, để đặt kỳ vọng đúng): không hỗ trợ Windows; `pagestore` không có full-page write; `kvlog` chỉ có một file dữ liệu và không có hint file; SSI trong `mvcc` là thuật toán Cahill cơ bản chứ chưa có tối ưu theo thứ tự commit của PostgreSQL; không có B-tree đầy đủ, chỉ có page store một tầng để đếm write amplification. Những việc này được đưa vào bài tập mở rộng.

---

### Lab (a) — `kvlog`: log nối đuôi, chỉ mục trong RAM, đuôi bị xé, compaction

Lý thuyết: [Chương 03 — Storage engine](../12-he-thong-du-lieu/03-storage-engine.md), phần Bitcask (log nối đuôi + hash index trong RAM) và phần độ bền ghi (`fsync`).

Code: `labs/crates/lab-data/src/kvlog.rs`, `record.rs`, `crc32c.rs`. Mỗi bản ghi là `crc32c(u32) key_len(u32) val_len(u32) key value`; xóa là bản ghi có `val_len = u32::MAX` (tombstone). Chỉ mục (`keydir`) ánh xạ key tới offset trong file, nên `get` là đúng một `pread`. Khi mở lại, store quét cả file để dựng lại chỉ mục, kiểm CRC từng bản ghi và xử lý phần đuôi theo chính sách `Recovery`. Compaction ghi file mới `data.log.compact`, `fsync`, `rename` đè lên file cũ rồi `fsync` thư mục.

```bash
cd labs && cargo run -q -p lab-data --bin kvlog
```

Output (macOS aarch64, rustc 1.95.0; các giá trị `ms` là `[machine-dependent]`):

```text
== 1. append-only log + keydir ==
open empty dir: records=0 live_keys=0
after 10000 puts (new keys)         : file=1,210,000 B  live_keys=10000  garbage=        0 B  space_amp=1.00
after 15000 overwrites, 1000 deletes: file=3,046,000 B  live_keys= 9000  garbage=1,957,000 B  space_amp=2.80
1000 random get(): hits=903 preads=903   1000 get() of absent keys: preads=0
reopen: scanned 26,000 records (1000 tombstones), rebuilt keydir with 9000 keys, stop=Clean  [67.3 ms, machine-dependent]

== 2. torn tail: cut the file inside the last record ==
5 records of 44 B each (header 12 B = crc32c + key_len + val_len); last record starts at offset 176
  cut  stop                 truncated  keys   file after  k4
    0  Clean                        0     4          176  absent
    1  Incomplete                   1     4          176  absent
   11  Incomplete                  11     4          176  absent
   12  Incomplete                  12     4          176  absent
   13  Incomplete                  13     4          176  absent
   20  Incomplete                  20     4          176  absent
   43  Incomplete                  43     4          176  absent
(cut = bytes of the last record that reached the disk; every cut < 44 is repaired by truncation)
4096 zero bytes appended: stop=ZeroTail truncated=4096 keys=5
appending after an unrepaired torn tail, then reopening:
  short record (17 B): opened, stop=Incomplete, truncated=30 B, get(new)=None
  long record (51 B) : recovery refused: damaged record at offset 176 (BadChecksumInMiddle)

== 3. damage in the middle: three recovery policies ==
Strict        recovery refused: damaged record at offset 88 (BadChecksumInMiddle); file untouched (220 B)
TolerateTail  recovery refused: damaged record at offset 88 (BadChecksumInMiddle); file untouched (220 B)
PointInTime   opened: kept 2 of 5 records, dropped 132 B = 3 records (2 of them intact); keys=2

== 4. compaction ==
before: file=736,500 B live=181,500 B garbage=555,000 B  space_amp=4.06  live_keys=1500
after : file=181,500 B live=181,500 B garbage=0 B  space_amp=1.00  live_keys=1500  (reclaimed 555,000 B)
deleted key user01700 after compaction: None
leftover data.log.compact from an interrupted compaction: removed=true live_keys=1500

== 5. durability knob: SyncPolicy ==
Never       200 puts: fsyncs=  0      0.007 ms/put  [machine-dependent]
EveryWrite  200 puts: fsyncs=200      2.604 ms/put  [machine-dependent]
```

**Đọc output.**

- *Mục 1.* Ghi 10.000 key mới cho `space_amp=1.00` (file đúng bằng dữ liệu sống). Sau 15.000 lần ghi đè và 1.000 lần xóa, file là 3.046.000 B nhưng chỉ 9.000 key còn sống, 1.957.000 B là rác: `space_amp=2.80`. Đây là hệ quả trực tiếp của việc chỉ nối đuôi: ghi đè hay xóa đều là một bản ghi *mới* mà không chạm bản cũ, nên file phình dần cho tới khi merge.[^bc-merge] Xóa là ghi một tombstone, và tombstone chỉ biến mất ở lần merge kế tiếp.[^bc-tomb] Chỉ mục nằm trong RAM nên đọc một key có mặt chỉ tốn một `pread` (903 hit, 903 `pread`) và đọc key vắng mặt tốn **0** `pread`: keydir là hash table ánh xạ key tới file, offset và kích thước bản ghi mới nhất,[^bc-keydir] và Bitcask tuyên bố một lần đọc không bao giờ cần hơn một lần seek đĩa.[^bc-read] `pread` ở đây là `read_at` của Rust, không đụng tới con trỏ file chung.[^std-readat] Cái giá: mở lại phải quét 26.000 bản ghi (67,3 ms, `[machine-dependent]`), tỉ lệ thuận với kích thước file. Bitcask dùng hint file để khởi động nhanh hơn;[^bc-scan] lab này chưa có (bài tập 1).
- *Mục 2 (đuôi bị xé).* Mô phỏng "máy sập giữa lúc ghi bản ghi cuối": cắt file ở mọi vị trí bên trong bản ghi thứ 5 (44 B, trong đó header 12 B). Mọi điểm cắt `1..43` cho `Incomplete`; recovery cắt bỏ phần đuôi, file về 176 B, `k4` vắng mặt (ghi chưa hoàn tất thì coi như chưa xảy ra). Điểm cắt `0` là `Clean`. Quy tắc này, "bản ghi dở hoặc sai CRC ở *cuối* file thì bỏ", là một heuristic: wiki RocksDB tự mô tả chế độ tương ứng (`kTolerateCorruptedTailRecords`) là heuristic vì hệ thống không phân biệt được hỏng ở đuôi log với ghi dở.[^rdb-wal-tolerate] CRC dùng ở đây là CRC-32C, bản cài đặt được kiểm bằng các vector trong phụ lục B.4 của RFC 3720[^rfc3720-b4] và RFC này mô tả mục đích của digest là kiểm tính toàn vẹn đầu-cuối không dùng mật mã;[^rfc3720-digest] LevelDB cũng ghi checksum `crc32c` trong từng bản ghi log.[^ldb-crc] Dòng `4096 zero bytes appended: stop=ZeroTail` là một ca lab tự chọn (đuôi toàn số 0); tôi không dẫn nguồn rằng hệ thống tệp nào để lại đuôi như vậy (suy luận của người soạn: đây là một dạng đuôi mà bộ nhận diện phải xử lý được).
- *Ghi tiếp sau đuôi chưa được sửa.* Hai dòng cuối mục 2 là bài học "recovery phải cắt đuôi **trước khi** nhận ghi mới". Nếu cứ nối bản ghi mới vào sau phần đuôi dở: với bản ghi ngắn (17 B) lần mở sau thấy lại `Incomplete`, cắt 30 B và `get(new)=None`, tức một bản ghi *đã được báo thành công* bị nuốt mất mà không có lỗi nào; với bản ghi dài (51 B) phần hỏng lọt vào *giữa* file nên recovery từ chối mở (`BadChecksumInMiddle`). Cả hai đều tệ hơn việc cắt đuôi ngay lúc mở.
- *Mục 3 (hỏng giữa file).* Ba chính sách đặt tên theo các chế độ WAL của RocksDB. `Strict` và `TolerateTail` đều từ chối khi hỏng nằm *giữa* file (offset 88) và để file nguyên vẹn; chế độ `kAbsoluteConsistency` của RocksDB coi mọi lỗi IO khi replay WAL là hỏng dữ liệu.[^rdb-wal-abs] `PointInTime` giữ tiền tố hợp lệ (2/5 bản ghi) và bỏ 132 B phía sau, trong đó có **2 bản ghi còn nguyên vẹn**: đó là cái giá của việc không phân biệt được hỏng với mất, và RocksDB mô tả chế độ này phù hợp cho hệ có replica để phát lại phần sau điểm khôi phục.[^rdb-wal-pit] Log của LevelDB tránh bài toán này bằng cấu trúc khối: gặp bản ghi hỏng thì nhảy tới ranh giới khối kế tiếp và quét tiếp, không cần heuristic để đồng bộ lại.[^ldb-resync] Định dạng một-file của lab không có ranh giới khối nên không làm được việc đó (nhận xét của người soạn: đó là lý do định dạng thật chia log thành khối).
- *Mục 4 (compaction).* Rác 555.000 B bị thu hồi (736.500 B về 181.500 B, `space_amp` 4,06 về 1,00); một key đã xóa không "sống lại" sau compaction. File `data.log.compact` bỏ dở từ một lần compaction bị ngắt được xóa khi mở. Mô hình "ghi file tạm rồi `rename`" là mẫu quen thuộc: MapReduce dựa vào phép đổi tên nguyên tử của hệ thống tệp để chốt đầu ra của một tác vụ.[^mr-rename] Việc bỏ tombstone ở đây an toàn vì lab chỉ có **một** file, nên mọi phiên bản cũ của key đều nằm trong đầu vào của lần compaction. Với nhiều segment thì không còn đúng nữa: xem tombstone rơi sớm ở Lab (b) mục 2.
- *Mục 5 (độ bền).* `SyncPolicy::Never` mất 0,007 ms mỗi `put`; `EveryWrite` (một `fsync` mỗi lần ghi) mất 2,604 ms (`[machine-dependent]`, đo trên macOS). Bitcask cũng có tùy chọn sync mỗi lần ghi,[^bc-sync] và LevelDB nêu rằng ghi bất đồng bộ thường nhanh hơn ghi đồng bộ hơn một nghìn lần,[^ldb-1000] nghĩa là bậc độ lớn của khoảng cách này không có gì lạ. Đừng đem con số 2,604 ms so với Linux: tài liệu của Rust nói rõ một số nền tảng có thể cài `sync_data` bằng `sync_all`,[^std-syncdata] nên cái giá của một lần "đồng bộ" khác nhau theo nền tảng (so sánh trên Linux/ext4: chưa chạy). Lưu ý phạm vi: các test mô phỏng đuôi hỏng bằng cách cắt file, không phải bằng cách làm sập hệ điều hành, nên chúng kiểm logic recovery chứ không kiểm tính bền thật của phần cứng.

---

### Lab (b) — `lsm`: LSM-tree, tombstone, Bloom filter, chính sách compaction, và page store để so

Lý thuyết: [Chương 03 — Storage engine](../12-he-thong-du-lieu/03-storage-engine.md), phần LSM-tree (memtable, SSTable, compaction), phần Bloom filter và phần write/read/space amplification.

Code: `lsm.rs`, `sstable.rs`, `bloom.rs`, `pagestore.rs`. Ghi đi qua WAL rồi vào memtable (`BTreeMap`); khi đầy, memtable được flush thành một SSTable (file có block dữ liệu, index, Bloom filter, footer có CRC); một file `MANIFEST` (thay nguyên tử) liệt kê các run. `Policy` chọn compaction: `None`, `Tiered { fanout }` (gộp `fanout` run cùng tầng) hoặc `Leveled { fanout, base_bytes }` (một run mỗi tầng, gộp khi hai run đụng tầng). `TombstonePolicy::AlwaysDrop` là một **lỗi cố ý** để quan sát tombstone rơi sớm. `pagestore.rs` là cửa hàng trang 4 KiB ghi tại chỗ có WAL và checkpoint, chỉ để so write amplification.

```bash
cd labs && cargo run -q --release -p lab-data --bin lsm
```

Output (macOS aarch64, rustc 1.95.0, `--release`):

```text
== 1. write path, flush, merge on read, tombstones, major compaction ==
after 3 flushes (puts 0..50, puts 25..75, deletes 30..40) and 10 puts left in the memtable:
  memtable: 10 entries
  run 000006  level 0     10 entries      389 B
  run 000004  level 0     50 entries    6,427 B
  run 000002  level 0     50 entries    6,427 B
  lookup user00000010: value "a:10:..." from run 000002
  lookup user00000028: value "b:28:..." from run 000004
  lookup user00000035: tombstone in run 000006 (deleted)
  lookup user00000060: value "b:60:..." from run 000004
  lookup user00000072: value "c:72:..." from memtable
  lookup user00000099: absent everywhere
  scan of the whole key space: 70 live entries (merged from 4 sources)
after compact_all (merge every run, drop tombstones and shadowed versions):
  memtable: 0 entries
  run 000010  level 0     70 entries    8,960 B
  table bytes 13,243 -> 8,960
  lookup user00000035: absent everywhere

== 2. dropping a tombstone too early ==
size-tiered, fanout 3: flush k1="the-value", k100, k101 -> merged into the oldest run;
then flush the delete of k1, k200, k201 -> these three merge, but the oldest run is not part of that merge
DropAtBottom (correct): runs (newest first) = [tier 1 / 3 entries, tier 1 / 3 entries]  get(user00000001) -> None
AlwaysDrop   (the bug): runs (newest first) = [tier 1 / 2 entries, tier 1 / 3 entries]  get(user00000001) -> Some("the-value")

== 3. Bloom filter: reads per lookup of keys that do not exist ==
8 runs of 4000 keys each (no compaction); 40000 lookups of keys that were never written
 bits per key   filter B  blocks/lookup  bloom negatives false positives   FP rate
            0          0          7.999                0              0         -
            5     20,000          0.748          290,007         29,937     9.36%
           10     40,000          0.067          317,256          2,688     0.84%
           15     60,000          0.007          319,681            263     0.08%
(filter B = approximate bytes of filter per run times 8 runs; FP rate = false positives / lookups that passed the range check)

== 4. compaction policies: write, read and space amplification ==
200,000 puts of 112-byte entries over 50,000 distinct keys (uniform random), memtable 256 KiB, 10 bits/key Bloom filter
engine                         WA     log MB   flush MB compact MB  runs space amp blk/get hit blk/get miss
no compaction                2.20       24.8       24.5        0.0    83      4.03       1.12        0.67
tiered, fanout 4             4.20       24.8       24.5       44.9     5      1.69       0.99        0.04
tiered, fanout 10            3.06       24.8       24.5       19.2    11      3.30       0.99        0.09
leveled, fanout 10           8.80       24.8       24.5      147.7     2      1.15       0.97        0.01
leveled, fanout 4            7.32       24.8       24.5      114.7     3      1.47       0.97        0.02
(user data written: 22.4 MB)

== 5. the same workload on an in-place page store (4 KiB pages + WAL + checkpoints) ==
engine                         WA     WAL MB     pages MB   pages
pages, checkpoint/1000      26.68       24.8        572.8    1995
pages, checkpoint/10000      6.86       24.8        128.9    1995
pages, checkpoint/100000     1.79       24.8         15.3    1995
pages, ascending keys        2.16       24.8         23.5    5715
("pages MB" = bytes written in place at checkpoint; every row writes 22.4 MB of user data)

== 6. crash recovery ==
reopen after a crash: 1 run, replayed 501 log records, memtable has 501 entries, orphans removed 0
  user00001400 -> value "b:1400:." from memtable   user00000005 (deleted in the log) -> tombstone in memtable (deleted)
log cut 20 bytes short (torn last record): replayed 500 records, cut 4 B, stop=Incomplete
  the delete of user00000005 was the last record: value "a:5:...." from run 000002
```

**Đọc output.**

- *Mục 1 (đường ghi và đường đọc).* Memtable bị đóng băng rồi thành SSTable khi đủ lớn,[^bt-minor] và một lần đọc chạy trên *góc nhìn gộp* của memtable và các SSTable, mới nhất thắng.[^bt-merged] Output cho thấy đúng thứ tự đó: `user00000072` chỉ có trong memtable, `user00000028` ra từ run `000004` (bản `b:`, che bản `a:` cũ ở run `000002`), và `user00000035` ra `tombstone in run 000006`: xóa là một dấu *che* các giá trị cũ ở bảng cũ hơn, không phải xóa thật.[^ldb-sorted2] `compact_all` gộp mọi run, bỏ tombstone và các phiên bản bị che: từ 13.243 B còn 8.960 B, 70 entry, và `user00000035` thành "absent everywhere".
- *Mục 2 (tombstone rơi sớm).* Đây là bẫy kinh điển. LevelDB chỉ bỏ dấu xóa khi không còn tầng nào *cao hơn* có file chứa key đó: "…they also drop deletion markers if there are no higher numbered levels that contain a file whose range overlaps the current key".[^ldb-drop] Kịch bản: lần flush đầu đã được gộp vào run *cũ nhất* (chứa `k1 = "the-value"`); sau đó lần flush xóa `k1` chỉ gộp ba run mới với nhau, run cũ nhất **không** nằm trong lần gộp đó. `DropAtBottom` (đúng) giữ tombstone nên `get(user00000001)` là `None`; `AlwaysDrop` (lỗi) bỏ tombstone ngay (run mới chỉ còn 2 entry thay vì 3), và `get` trả về `Some("the-value")`: dữ liệu đã xóa **sống lại**. Không có lỗi hay cảnh báo nào, chỉ là một đáp án sai.
- *Mục 3 (Bloom filter).* Không có filter (0 bit/key) thì một lần tra key *không tồn tại* đọc gần hết số run (7,999 block trên 8 run). Mỗi file SST mang theo một Bloom filter để biết file có thể chứa key hay không,[^rdb-bloom-perfile] và bộ lọc chắc chắn đúng khi nói "không có": key chắc chắn vắng nếu ít nhất một phép thăm dò trả về 0.[^rdb-bloom-how] Với 10 bit/key chỉ còn 0,067 block mỗi lần tra (2.688 false positive trên 40.000 lần tra, đúng `2.688 / 40.000 = 0,067`) và tỉ lệ dương giả 0,84%. Hướng dẫn tuning của RocksDB cho cặp (bit/key, dương giả) là 4,9 bit ↔ 10%, 9,9 bit ↔ 1%, 15,5 bit ↔ 0,1%;[^rdb-bloom-bits] các hàng 5/10/15 bit của lab (9,36%, 0,84%, 0,08%) bám sát bảng đó, dù bộ băm là FNV-1a tự viết chứ không phải của RocksDB. Cột `filter B` cho thấy cái giá: 20.000, 40.000, 60.000 B cho 32.000 key (8 run × 4.000), tức đúng 5, 10, 15 bit mỗi key. Lưu ý: Bloom filter không giúp gì cho quét theo dải.[^rdb-tune-range]
- *Mục 4 (ba loại khuếch đại).* `WA` là tổng byte ghi xuống đĩa (WAL + flush + compaction) chia cho byte dữ liệu người dùng; ví dụ hàng `tiered, fanout 4`: (24,8 + 24,5 + 44,9) / 22,4 = 4,20. Định nghĩa gốc: write amplification là tỉ số byte ghi xuống lưu trữ trên byte ghi vào database;[^rdb-wa-def] read amplification là số lần đọc đĩa mỗi truy vấn;[^rdb-ra-def] space amplification là tỉ số kích thước file trên đĩa so với kích thước dữ liệu.[^rdb-sa-def] Hướng của kết quả khớp với lý thuyết: tiered "giảm write amplification, đổi lại read và space amplification"[^rdb-comp-tier] (WA 3,06 đến 4,20 so với 7,32 đến 8,80 của leveled; nhưng 5 đến 11 run và `space amp` 1,69 đến 3,30), còn leveled cổ điển "giảm space amplification, đổi lại read và write amplification"[^rdb-comp-lvl] (`space amp` 1,15 đến 1,47 với chỉ 2 đến 3 run). Cột `blk/get` cho thấy vì sao 83 run (không compaction) vẫn chỉ tốn 1,12 block cho một lần đọc trúng: Bloom filter làm giảm mạnh read amplification của tra cứu điểm,[^rdb-tune-ra] còn lần đọc *trượt* đắt hơn rõ rệt khi chưa compaction (0,67 so với 0,01 đến 0,09). **Cảnh báo phạm vi** (nhận xét của người soạn): lý thuyết nói write amplification mỗi tầng của leveled xấp xỉ fanout còn của tiered là 1;[^rdb-comp-lvl-wa][^rdb-comp-tier-wa] số đo ở đây (8,80 với fanout 10) là của engine *đơn giản hóa* này (mỗi tầng một run, gộp khi đụng tầng), không phải của RocksDB.
- *Mục 5 (page store để so).* Cùng 200.000 `put` (22,4 MB dữ liệu) trên cửa hàng trang ghi tại chỗ: WAL luôn là 24,8 MB, còn phần ghi trang tại checkpoint phụ thuộc cực mạnh vào tần suất checkpoint: 26,68× khi checkpoint mỗi 1.000 thao tác (cùng một trang nóng bị ghi lại nhiều lần), 6,86× mỗi 10.000, 1,79× mỗi 100.000; khóa tăng dần chỉ tốn 2,16× vì các lần ghi dồn vào trang cuối. Nguyên tắc nền: thay đổi trang dữ liệu chỉ được ghi *sau khi* bản ghi WAL tương ứng đã xuống đĩa,[^pg-wal-concept] nhờ đó lúc commit chỉ cần flush WAL (ghi tuần tự, rẻ) chứ không phải mọi trang đã đổi.[^pg-wal-fewer] Lab **chưa** cài full-page write, thứ PostgreSQL bật để chống trang bị ghi dở: một trang đang ghi lúc hệ điều hành sập có thể chỉ hoàn tất một phần, để lại trang trộn dữ liệu cũ và mới.[^pg-fpw] Vì vậy WA của page store ở đây là cận dưới (bài tập 2). Đừng kết luận "LSM thắng B-tree" hay ngược lại từ một workload (ghi ngẫu nhiên đều, 50.000 key): kết quả đổi chiều theo thứ tự khóa và tần suất checkpoint, và đó chính là điều bảng này cho thấy (nhận xét của người soạn).
- *Mục 6 (phục hồi sau sập).* Mở lại sau khi sập: replay 501 bản ghi WAL vào memtable; WAL bị cắt ngắn 20 B (bản ghi cuối bị xé) thì chỉ replay 500, cắt 4 B còn lại, `stop=Incomplete`. Bản ghi cuối là lệnh xóa `user00000005`, nên key này **sống lại** với giá trị cũ từ run `000002`: một lệnh xóa chưa kịp xuống đĩa nguyên vẹn thì mất, và hệ chỉ được phép báo "đã xóa" cho client sau khi WAL đã bền (Lab (a) mục 5 cho thấy giá của việc đó). Danh sách file còn sống nằm trong `MANIFEST`, một file được ghi như một log;[^ldb-manifest] file mồ côi không có trong MANIFEST bị dọn khi mở (`orphans removed 0` ở lần chạy này).

---

### Lab (c) — `layout_scan`: row store và column store

Lý thuyết: [Chương 03 — Storage engine](../12-he-thong-du-lieu/03-storage-engine.md), phần row store, column store, nén cột và OLAP với OLTP.

Code: `layout.rs`. Bảng `orders` 2.000.000 dòng × 14 cột, mỗi dòng đúng 64 B. Hai bố cục: **row store** (một mảng các dòng 64 B) và **column store** (14 mảng riêng theo kiểu `u64`, `u32`, `u16`). Truy vấn OLAP: `SELECT SUM(price_cents), COUNT(*) FROM orders WHERE country = 7`. Truy vấn OLTP: lấy cả dòng theo `id` ngẫu nhiên. Bố cục thứ ba: cột `country` được sắp theo `country` rồi mã hóa theo run (RLE).

```bash
cd labs && cargo run -q --release -p lab-data --bin layout_scan
```

Output (macOS aarch64, rustc 1.95.0, `--release`; mọi thời gian và GB/s là `[machine-dependent]`):

```text
== table `orders`: 2,000,000 rows x 14 columns, 64 bytes per row ==
row store    : 128 MB in one array of 64-byte rows
column store : 128 MB in 14 arrays (u64 x3, u32 x9, u16 x2)

== OLAP: SELECT SUM(price_cents), COUNT(*) FROM orders WHERE country = 7 ==
result (identical in both layouts): sum_price_cents=494,935,518 matches=10,010

layout                              bytes streamed   best ms   median ms  GB/s (best)
row store (scan all 64 B rows)         128,000,000     1.929       2.070         66.3
column store (country + price)          20,000,000     0.354       0.358         56.6
sorted by country + RLE (slice)             80,080     0.001       0.001        113.1

sorted + run-length encoded `country` column: 200 runs, 2,000 B encoded vs 4,000,000 B as a plain u16 array
(the third line streams only the 10,010 matching prices; "bytes streamed" there is matches x 8)

== OLTP: fetch whole rows by id (1,000,000 random ids) ==
layout                              arrays touched   best ms    ns / row
row store (1 contiguous 64 B row)                1      8.69         8.7
column store (rebuild from 14 arrays)              14     75.32        75.3
```

**Đọc output.**

- Cả hai bố cục trả cùng một kết quả (`sum_price_cents=494.935.518`, 10.010 dòng khớp); chỉ lượng byte phải đi qua bộ nhớ khác nhau. Column store chỉ đọc hai cột cần cho truy vấn (`country`: `u16`, `price_cents`: `u64`, tức 10 B mỗi dòng, 20.000.000 B) thay vì cả 128.000.000 B của row store: đúng ý của C-Store, "một DBMS column store chỉ cần đọc giá trị của các cột mà truy vấn cần và tránh đưa các thuộc tính không liên quan vào bộ nhớ".[^cs-col] Thời gian tốt nhất giảm từ 1,929 ms xuống 0,354 ms (5,4 lần, trong khi lượng byte giảm 6,4 lần); cả hai đều bị chặn bởi băng thông bộ nhớ chứ không phải tính toán. Cột GB/s chỉ là `bytes streamed / best ms`, phụ thuộc máy và có thể chịu ảnh hưởng của prefetcher và cache hệ thống, nên không nên đọc nó như băng thông DRAM đo chuẩn (suy luận của người soạn).
- Dòng thứ ba: sau khi sắp theo `country`, cột chỉ còn **200 run**, 2.000 B thay vì 4.000.000 B, và truy vấn chỉ cần đọc đúng 10.010 giá cả khớp (80.080 B) trong khoảng 0,001 ms. Đây là điều Abadi và cộng sự mô tả: nếu dữ liệu được sắp theo một cột thì cột đó nén cực tốt, ví dụ các run giá trị lặp lại được mã hóa theo run.[^ab-compress] Cái giá là chỉ một thứ tự sắp xếp: các cột khác không được lợi tương tự.
- OLTP đảo ngược cán cân: lấy một dòng nguyên vẹn mất 8,7 ns ở row store (một vùng liền 64 B) nhưng 75,3 ns ở column store, vì phải ghép dòng lại từ 14 mảng. Bài báo của Abadi nêu đúng chi phí này: ở một điểm nào đó của hầu hết kế hoạch truy vấn phải ghép dữ liệu từ nhiều cột lại với nhau.[^ab-late2] Ngược lại C-Store lưu ý một thiết kế row store chỉ cần *một* lần ghi đĩa để đẩy mọi trường của một bản ghi, nên ghi nhanh.[^cs-row]
- **Giới hạn** (nhận xét của người soạn): lab đo trong bộ nhớ; các luận điểm gốc về column store chủ yếu nói về I/O đĩa. Kết quả ở đây chỉ minh họa phần "lượng byte đi qua bộ nhớ" và "chi phí ghép dòng", không thay cho đo trên đĩa.

---

### Lab (d) — `encoding`: varint, tag và tiến hóa schema

Lý thuyết: [Chương 04 — Encoding và tiến hóa schema](../12-he-thong-du-lieu/04-encoding-va-tien-hoa-schema.md), phần định dạng nhị phân có schema (Protocol Buffers) và phần tương thích tiến/lùi.

Code: `codec.rs`. Bộ mã hóa theo kiểu protobuf, viết tay: varint, zigzag, tag `(field << 3) | wire_type`, các wire type `VARINT` và `LEN`. Hai phiên bản schema: `OrderV1` (`id`, `customer`, `legacy_note` ở tag 3) và `OrderV2` (`reserved 3`, thêm `currency` ở tag 4 và `amount_cents` ở tag 5), có **giữ trường lạ** (unknown fields). Hai biến thể sai: `OrderV3RetypedTag3` (dùng lại tag 3 với kiểu khác wire type) và `OrderV3ReusedTag3` (dùng lại tag 3 cùng wire type).

```bash
cd labs && cargo run -q -p lab-data --bin encoding
```

Output (macOS aarch64, rustc 1.95.0). Rút gọn: ở mục 4 bỏ các dòng `n=4..11`, `n=13..16`, `n=18`, `n=19`, tất cả đều là `error: buffer ends inside a record`:

```text
== 1. varint: 7 payload bits per byte, least significant group first, high bit = "more bytes follow" ==
value                  bytes                            len
0                      00                                 1
1                      01                                 1
127                    7f                                 1
128                    80 01                              2
150                    96 01                              2
300                    ac 02                              2
16384 (2^14)           80 80 01                           3
u32::MAX               ff ff ff ff 0f                     5
u64::MAX               ff ff ff ff ff ff ff ff ff 01     10
-1 written as int64    ff ff ff ff ff ff ff ff ff 01     10
zigzag(-1) = sint64 -1 01                                 1
zigzag: n -> 2n for n >= 0, -n -> 2n - 1:
             0 ->          0
            -1 ->          1
             1 ->          2
            -2 ->          3
             2 ->          4
           -64 ->        127
            63 ->        126
   -2147483648 -> 4294967295

== 2. tag = (field_number << 3) | wire_type, itself a varint ==
field    1 Varint  tag=     8  bytes: 08
field    2 Len     tag=    18  bytes: 12
field   15 Varint  tag=   120  bytes: 78
field   16 Varint  tag=   128  bytes: 80 01
field 2047 Varint  tag= 16376  bytes: f8 7f
field 2048 Varint  tag= 16384  bytes: 80 80 01

== 3. the two worked examples of the Protocol Buffers encoding guide ==
{ a = 150 }        field 1, VARINT: 08 96 01
{ b = "testing" }  field 2, LEN    : 12 07 74 65 73 74 69 6e 67
decode_fields(second) = ["2:LEN(\"testing\")"]

== 4. rolling upgrade: v1 and v2 code reading each other's bytes ==
v1: Order { uint64 id = 1; string customer = 2; string legacy_note = 3; }
v2: Order { reserved 3; uint64 id = 1; string customer = 2; string currency = 4; int64 amount_cents = 5; }

written by v1 (23 bytes): 08 96 01 12 07 74 65 73 74 69 6e 67 1a 09 67 69 66 74 20 77 72 61 70
  read by v2 (backward compatible): id=150 customer="testing" currency="" amount_cents=0 unknown=[3:LEN("gift wrap")]

written by v2 (20 bytes): 08 96 01 12 07 74 65 73 74 69 6e 67 22 03 55 53 44 28 c6 0f
  read by v1 (forward compatible):  id=150 customer="testing" legacy_note="" unknown=[4:LEN("USD"), 5:VARINT(1990)]

the v1 service reads a v2 message, edits `customer`, writes it back:
  unknown fields preserved : v2 then reads customer="renamed" currency="USD" amount_cents=1990
  copied field by field    : v2 then reads customer="renamed" currency="" amount_cents=0   <- new fields silently gone

truncated v2 message (cut after n bytes):
  n= 0: decodes, id=0 customer="" currency="" amount_cents=0
  n= 1: error: buffer ends inside a record
  n= 2: error: buffer ends inside a record
  n= 3: decodes, id=150 customer="" currency="" amount_cents=0
  n=12: decodes, id=150 customer="testing" currency="" amount_cents=0
  n=17: decodes, id=150 customer="testing" currency="USD" amount_cents=0
  n=20: decodes, id=150 customer="testing" currency="USD" amount_cents=1990

== 5. changes that break ==
(a) number 3 reused as `uint32 retries` (another wire type), reading v1 bytes:
    error: field 3: schema says Varint but the record is Len
(b) number 3 reused as `string coupon_code` (same wire type), reading v1 bytes:
    no error: coupon_code = "gift wrap"   <- the old free-text note, now treated as a coupon
(c) number 5 declared int64 by the writer, read under other declarations:
     written value     as int64            as sint64     as int32
               150          150                   75          150
                -1           -1 -9223372036854775808           -1
                -5           -5 -9223372036854775806           -5
        5000000000   5000000000           2500000000    705032704

== 6. names are not on the wire ==
JSON  (68 bytes): {"id":150,"customer":"testing","currency":"USD","amount_cents":1990}
v2 binary (20 bytes): 08 96 01 12 07 74 65 73 74 69 6e 67 22 03 55 53 44 28 c6 0f
```

**Đọc output.**

- *Mục 1 và 2 (varint, tag).* Số 150 được mã hóa thành `96 01`, đúng ví dụ của hướng dẫn mã hóa protobuf (bỏ bit cao nhất của mỗi byte, đảo thứ tự nhóm 7 bit);[^pb-enc-150] varint dùng 1 đến 10 byte, số nhỏ dùng ít byte,[^pb-enc-varint] và mỗi byte có một bit tiếp-tục cho biết byte sau còn thuộc về varint hay không.[^pb-enc-varint2] Một số âm viết như `int64` phải dùng cả 10 byte vì bit cao nhất được đặt (`-1` ra `ff … 01`),[^pb-enc-neg] còn `sint64` dùng zigzag, ánh xạ số dương `p` thành `2p` và số âm `n` thành `2|n| - 1`:[^pb-enc-zigzag] `-1` thành `1`, chỉ còn 1 byte. Tag là varint của `(field_number << 3) | wire_type`.[^pb-enc-tag] Vì vậy trường 1 đến 15 tốn 1 byte tag, 16 đến 2047 tốn 2 byte:[^pb-field-range] bảng mục 2 cho đúng ranh giới này (`15` ra `78`, `16` ra `80 01`, `2047` ra `f8 7f`, `2048` cần 3 byte). Hệ quả thiết kế (nhận xét của người soạn): dành số 1 đến 15 cho các trường xuất hiện nhiều.
- *Mục 3.* Hai ví dụ trong hướng dẫn: `{a = 150}` ra ba byte `08 96 01`;[^pb-enc-simple] chuỗi `"testing"` ra tag `12`, độ dài `07` rồi bảy byte UTF-8.[^pb-enc-len] Output của lab trùng từng byte.
- *Mục 4 (nâng cấp cuốn chiếu).* Khi triển khai cuốn chiếu, hai phiên bản code cùng chạy và đọc dữ liệu của nhau, nên schema phải tương thích hai chiều.[^cam-rolling] Theo định nghĩa của Confluent, *backward compatible* là consumer dùng schema mới đọc được dữ liệu sinh bởi schema cũ,[^conf-backward] còn *forward compatible* là dữ liệu sinh bởi schema mới vẫn đọc được bởi consumer dùng schema cũ (dù có thể không dùng hết khả năng của schema mới).[^conf-forward] Output thể hiện cả hai: v2 đọc byte của v1 (`currency=""`, `amount_cents=0` mặc định, và tag 3 thành trường lạ `unknown=[3:LEN("gift wrap")]`); v1 đọc byte của v2 và bỏ qua tag 4 và 5. Việc bỏ qua được là nhờ wire type cho parser biết payload dài bao nhiêu để nhảy qua.[^pb-enc-skip] Mục đọc-sửa-ghi là bài học dễ bỏ sót: nếu v1 *giữ* trường lạ rồi ghi lại thì v2 đọc tiếp được `currency="USD"`, `amount_cents=1990`; protobuf 3 cũng giữ trường lạ khi parse và khi serialize lại.[^pb3-unknown-keep] Nếu v1 sao chép từng trường sang một message mới thì trường lạ biến mất âm thầm (`currency=""`, `amount_cents=0`), và tài liệu protobuf khuyên dùng `CopyFrom()`/`MergeFrom()` thay vì sao chép từng trường vì lý do này.[^pb3-unknown-copy] Bảng cắt cụt cho thấy một nhược điểm khác: định dạng không có khung độ dài hay checksum, nên một message bị cắt ở ranh giới bản ghi (n = 0, 3, 12, 17, 20) vẫn **giải mã thành công** thành một message "có vẻ hợp lệ" nhưng thiếu trường; các điểm cắt khác mới báo lỗi. Độ dài và tính toàn vẹn phải do tầng ngoài đảm nhiệm (suy luận của người soạn).
- *Mục 5 (thay đổi gây hỏng).* Wire format mỏng và không có cách phát hiện một trường được mã hóa theo định nghĩa này nhưng giải mã theo định nghĩa khác.[^pb3-lean] (a) Dùng lại tag 3 làm `uint32`: decoder của lab báo lỗi vì wire type lệch (`schema says Varint but the record is Len`); lab chọn báo lỗi, còn thư viện protobuf thật có thể xử lý khác, tôi chưa kiểm ở đây. (b) Dùng lại tag 3 làm `string`, **cùng wire type**: không có lỗi nào, nội dung ghi chú cũ `"gift wrap"` được hiểu thành mã giảm giá. Tài liệu protobuf liệt kê hậu quả của việc đó: tốn thời gian gỡ lỗi, lỗi parse/merge ở trường hợp tốt nhất, lộ dữ liệu nhạy cảm, hỏng dữ liệu;[^pb3-reuse] cách phòng là xóa trường nhưng **phải `reserved` số của nó**,[^pb3-reserve] đúng như `reserved 3;` trong schema v2. (c) Đổi kiểu cho cùng số tag: `int32` và `int64` tương thích nhưng giá trị lớn hơn `INT32_MAX` bị cắt cụt ở bên đọc `int32`;[^pb3-compat-int] bảng cho `5000000000` ra `705032704` khi đọc như `int32` (cắt 32 bit cao). `sint32`/`sint64` tương thích với nhau nhưng **không** tương thích với các kiểu số nguyên khác:[^pb3-compat-sint] đọc 150 như `sint64` ra 75 và `-1` ra `-9223372036854775808`. Đó là lý do "đổi kiểu số nguyên" không bao giờ là thay đổi vô hại.
- *Mục 6 (tên không có trên dây).* JSON 68 B chứa cả tên trường; dạng nhị phân 20 B chỉ có số tag. Trong protobuf, tên và kiểu khai báo của trường chỉ biết được ở phía giải mã qua định nghĩa message;[^pb-enc-kv] Avro còn đi xa hơn: dữ liệu nhị phân không chứa thông tin kiểu hay tên trường nên luôn phải có schema mới đọc được,[^avro-noname] và khi writer và reader dùng schema khác nhau thì các trường được ghép theo **tên**.[^avro-resolve-names] Đó là sự khác biệt thiết kế mà bài tập 6 yêu cầu hiện thực.

---

### Lab (e) — `mvcc`: phiên bản dòng, snapshot, ma trận bất thường, fuzz

Lý thuyết: [Chương 07 — Transaction](../12-he-thong-du-lieu/07-transaction.md), phần isolation level, snapshot isolation, write skew và SSI; [Chương 15 — PostgreSQL bên trong](../12-he-thong-du-lieu/15-postgresql-ben-trong.md), phần MVCC, VACUUM và `xmin`/`xmax`.

Code: `mvcc.rs`. Một engine MVCC xác định (một luồng, các interleaving do kịch bản quyết định): mỗi phiên bản dòng có `xmin`/`xmax`; snapshot theo ngữ nghĩa `xmin:xmax:xip` của PostgreSQL; ba mức `ReadCommitted`, `SnapshotIsolation`, `Serializable` (SSI bản đơn giản: cạnh rw, pivot, cờ "doom"). Không có luồng thật: nơi PostgreSQL *chờ* khóa thì engine trả `WouldBlock` để kịch bản tự quyết định cách thử lại. Quy ước của lab: txid đầu tiên là 100, `xmin = 2` đóng vai xid đã đóng băng. Có module `scenarios` (dirty read/write, read skew, lost update, write skew, double booking cùng các cách sửa) và module `fuzz` (chương trình ngẫu nhiên, kiểm tuần tự hóa được bằng vét cạn).

```bash
cd labs && cargo run -q --release -p lab-data --bin mvcc        # mặc định 20000 trial mỗi mức
```

Output (macOS aarch64, rustc 1.95.0, `--release`). Rút gọn: mục "details" chỉ giữ read skew, lost update, write skew và double booking; các nhóm còn lại có cùng khuôn:

```text
== 1. one row, several versions: xmin, xmax and snapshots ==
row loaded before any transaction (xmin = 2 plays the role of a frozen xid):
    xmin   xmax  value   xmin state     xmax state
       2      0    100   committed      -

transaction 100 UPDATEs acct to 80 and has not committed:
    xmin   xmax  value   xmin state     xmax state
       2    100    100   committed      in progress
     100      0     80   in progress    -

transaction 101 (snapshot isolation) reads acct = Some(100); its snapshot is 100:100: (xmin:xmax:xip)

transaction 100 commits. The reader's snapshot is fixed, so it still reads Some(100);
a transaction that starts now reads Some(80), snapshot 101:101:

transaction 103 updates acct to 60 and rolls back; its version stays in the table, invisible:
    xmin   xmax  value   xmin state     xmax state
       2    100    100   committed      committed
     100    103     80   committed      aborted
     103      0     60   aborted        -
(3 versions of one logical row are stored; vacuum is what removes the dead ones, section 3)

== 2. which anomalies does each isolation level allow? ==
scenario                                       read committed   snapshot isolation   serializable (SSI)
dirty read                                     prevented        prevented            prevented
dirty write                                    prevented        prevented            prevented
read skew                                      ANOMALY          prevented            prevented
lost update (read in app, then write)          ANOMALY          prevented            prevented
lost update with UPDATE SET v = v + 1          prevented        prevented            prevented
write skew: doctors on call                    ANOMALY          ANOMALY              prevented
  fix 1: both update a shared row first        prevented        prevented            prevented
  fix 2: SELECT ... FOR UPDATE the rows read   prevented        prevented            prevented
phantom write skew: double booking             ANOMALY          ANOMALY              prevented
  fix: both update a shared slot row first     prevented        prevented            prevented

details (what happened in each run):
read skew
  [read committed] T1 read x = 50, then y = 90: sum 140
  [snapshot isolation] T1 read x = 50, then y = 50: sum 100
  [serializable (SSI)] T1 read x = 50, then y = 50: sum 100
lost update (read in app, then write)
  [read committed] two increments leave c = 1
  [snapshot isolation] T2 failed (could not serialize access due to concurrent update (row "c")); retried as T3 -> two increments leave c = 2
write skew: doctors on call
  [read committed] alice: saw 2 on call, went off; bob: saw 2 on call, went off; doctors on call at the end: 0
  [snapshot isolation] alice: saw 2 on call, went off; bob: saw 2 on call, went off; doctors on call at the end: 0
  [serializable (SSI)] alice: saw 2 on call, went off; bob: could not serialize access due to read/write dependencies among transactions; retry of bob: saw 1 on call, stayed; doctors on call at the end: 1
fix 2: SELECT ... FOR UPDATE the rows read
  [snapshot isolation] alice: saw 2 on call, went off; bob: would block: row "oncall/alice" is being modified by transaction 100; retry of bob: saw 1 on call, stayed; doctors on call at the end: 1
phantom write skew: double booking
  [snapshot isolation] alice: room free, booked; bob: room free, booked; committed bookings for 9am: 2
  [serializable (SSI)] alice: room free, booked; bob: could not serialize access due to read/write dependencies among transactions; committed bookings for 9am: 1

== 3. VACUUM analogue: an open snapshot holds dead versions back ==
transaction 100 (snapshot isolation) reads x and stays open; snapshot 100:100:
5 committed updates and 1 rolled-back update later: 7 versions stored
vacuum while 100 is open : 1 removed, 6 remain, 5 are dead but not yet removable
vacuum after 100 commits : 5 removed, 1 remain, 0 are dead but not yet removable

== 4. random transactions, random interleavings, brute-force serializability check ==
20000 trials per level; each trial: 3-4 transactions of 2-4 operations on keys k0..k3 (k0, k1 exist initially)
a transaction that gets any error is rolled back and not retried; "serializable" = some serial order of the
committed transactions reproduces everything they read and the final state

isolation                trials    committed      rolled back       NOT serializable
read committed            20000        57495            12568           8423 (42.12%)
snapshot isolation        20000        53625            16530           5448 (27.24%)
serializable (SSI)        20000        45171            24851              0 (0.00%)

the smallest non-serializable outcome found under snapshot isolation:
  T0: read k2; write k3 = (sum of values read) + 2
  T1: scan k*; write k0 = (sum of values read) + 3
  T2: scan k*; write k2 = (sum of values read) + 4
  steps in this order (a transaction's last step is its commit): T0 T2 T2 T2 T1 T0 T1 T0 T1
  T2 committed having read [] and scanned [[("k0", 10), ("k1", 20)]]
  T0 committed having read [None] and scanned []
  T1 committed having read [] and scanned [[("k0", 10), ("k1", 20), ("k2", 34)]]
  final state {"k0": 67, "k1": 20, "k2": 34, "k3": 2}
```

**Đọc output.**

- *Mục 1 (phiên bản dòng).* Mỗi `UPDATE` không sửa dòng mà thêm một *phiên bản* mới; PostgreSQL định nghĩa "row version" là một trạng thái riêng của dòng và mỗi lần cập nhật tạo một phiên bản mới cho cùng một dòng logic.[^sys-rowversion] `xmin` là transaction đã tạo phiên bản,[^sys-xmin] `xmax` là transaction đã xóa nó hoặc 0 nếu chưa xóa.[^sys-xmax] Bảng thứ hai cho thấy một phiên bản *nhìn thấy được* vẫn có `xmax = 100`: tài liệu nói rõ điều này xảy ra khi transaction xóa chưa commit hoặc đã bị rollback,[^sys-xmax-nonzero] và output có đủ hai ca (100 đang chạy: `in progress`; 103 rollback: `aborted`). Snapshot in ra theo dạng `xmin:xmax:xip`: `xmin` là transaction nhỏ nhất còn hoạt động, `xmax` là "một đơn vị sau transaction đã hoàn tất lớn nhất", mọi id từ `xmax` trở lên coi như chưa hoàn tất và vô hình, còn id trong `[xmin, xmax)` không có trong danh sách `xip` đã hoàn tất tại thời điểm chụp.[^snap-text][^snap-xmax][^snap-xip] Vì vậy snapshot `100:100:` của transaction 101 (lúc 100 đang chạy) coi mọi thứ của 100 là vô hình: nó đọc `Some(100)` dù 100 đã ghi 80; sau khi 100 commit, snapshot đã chụp *không đổi* (vẫn `Some(100)`), còn transaction mới (`101:101:`) thấy `Some(80)`. Mức Repeatable Read của PostgreSQL có hành vi đó: chỉ thấy dữ liệu đã commit trước khi transaction bắt đầu.[^rr-snapshot] Phiên bản của transaction 103 đã rollback vẫn nằm trong bảng (3 phiên bản cho một dòng logic) cho tới khi VACUUM dọn.
- *Mục 2 (ma trận).* Đây là điểm mấu chốt của chương 07: **mức cô lập càng cao thì càng ít bất thường, nhưng chỉ `Serializable` chặn được write skew**. Trên engine này: `dirty read` và `dirty write` bị chặn ở mọi mức; `read skew` (T1 đọc `x`, T2 cập nhật `x` và `y`, T1 đọc `y`) xảy ra ở Read Committed (tổng 140 thay vì 100) và bị chặn khi có snapshot, khớp với định nghĩa A5A của Berenson;[^ber-a5a] lost update kiểu "đọc ở ứng dụng rồi ghi" xảy ra ở Read Committed (`c = 1` sau hai lần tăng)[^ber-p4] và bị chặn ở Snapshot Isolation bởi first-committer-wins,[^ber-fcw] khi T2 bị hủy với `could not serialize access due to concurrent update` và phải thử lại, đúng như PostgreSQL hướng dẫn: ứng dụng nhận lỗi này phải hủy transaction và thử lại **toàn bộ** transaction;[^rr-retry] còn câu lệnh nguyên tử `UPDATE … SET v = v + 1` không bị lost update ở mọi mức, vì transaction thứ hai chờ rồi bắt đầu từ phiên bản dòng đã cập nhật.[^rc-bank] **Write skew** (hai bác sĩ cùng thấy "còn 2 người trực" rồi cùng xin nghỉ) lọt qua cả Read Committed lẫn Snapshot Isolation: cuối cùng không còn ai trực. Ports và Grittner mô tả đúng ví dụ này và chỉ ra khóa ghi lúc update không cứu được vì hai transaction sửa hai dòng khác nhau nên không xung đột;[^ports-doctors][^ports-wl] chỉ `Serializable` chặn, bằng cách hủy `bob` với `could not serialize access due to read/write dependencies among transactions`, đúng thông điệp lỗi trong tài liệu PostgreSQL (nếu chạy ở Repeatable Read thì cả hai được commit, còn Serializable cho một bên commit và hủy bên kia).[^ser-example] Hai cách sửa ở tầng ứng dụng đều hoạt động ở mọi mức: (1) cả hai cùng cập nhật một dòng dùng chung trước (biến xung đột ẩn thành xung đột ghi-ghi), (2) `SELECT … FOR UPDATE` các dòng đã đọc, thứ khiến transaction khác "bị chặn" cho tới khi transaction hiện tại kết thúc.[^forupdate-block] Double booking là biến thể có *phantom*: hai transaction cùng kiểm "phòng còn trống" rồi cùng chèn một dòng *mới*, nên không có dòng cũ nào để khóa; Berenson đã chỉ ra kịch bản kiểu này lọt qua Snapshot Isolation.[^ber-jobs] Cách sửa dùng một dòng "slot" dùng chung để có thứ mà khóa. (Đây là *engine mô hình*: `WouldBlock` đại diện cho việc chờ khóa, còn tài liệu PostgreSQL ghi rõ Repeatable Read được cài bằng Snapshot Isolation.)[^rr-si]
- *Mục 3 (VACUUM).* Sau 5 lần cập nhật đã commit và 1 lần rollback có 7 phiên bản. Trong lúc transaction 100 còn mở (snapshot cũ), VACUUM chỉ gỡ được 1 phiên bản, còn "5 are dead but not yet removable": phiên bản cũ chưa được xóa khi nó còn có thể nhìn thấy được bởi transaction khác.[^vac-mustnot] Khi 100 commit, 5 phiên bản bị gỡ, còn 1. Dòng thông báo của lab bắt chước dòng log của PostgreSQL (`tuples: N removed, M remain, K are dead but not yet removable`).[^vl-log] Ở PostgreSQL, một transaction để mở quá lâu cũng làm bảng phình vì lý do đó: cấu hình `idle_in_transaction_session_timeout` tồn tại để ngắt các phiên rảnh giữ transaction.[^iit]
- *Mục 4 (fuzz).* 20.000 chương trình ngẫu nhiên mỗi mức; một transaction gặp lỗi thì bị rollback và không thử lại; "tuần tự hóa được" nghĩa là tồn tại một thứ tự chạy tuần tự của các transaction đã commit tái tạo mọi thứ chúng đã đọc cùng trạng thái cuối, đúng định nghĩa Serializable của PostgreSQL ("kết quả giống như chạy lần lượt theo một thứ tự nào đó").[^iso-serial-def] Kết quả: Read Committed có 8.423/20.000 (42,12%) lần chạy không tuần tự hóa được, Snapshot Isolation 5.448 (27,24%), **Serializable 0**. Con số 0 không miễn phí: SSI hủy 24.851 transaction (so với 16.530 ở SI), tức nhiều *dương giả* hơn. Ports và Grittner nói SSI có thể có dương giả vì không phải cấu trúc nguy hiểm nào cũng nằm trong một chu trình;[^ports-fp] bản của lab hủy mọi "pivot" (transaction có cả cạnh rw đến và đi),[^ports-dangerous] còn PostgreSQL có thêm tối ưu theo thứ tự commit để giảm dương giả[^ports-commitorder] (bài tập 4). Đừng đọc 42% hay 27% như tần suất ngoài đời: đó là tỉ lệ trên *chương trình ngẫu nhiên dày xung đột* (4 key, vài transaction), không phải đo đạc workload thật. Ví dụ nhỏ nhất dưới SI là một **chu trình rw**: `T0` đọc `k2` khi chưa có (vậy `T0` phải đứng *trước* `T2`, người sau ghi `k2`), trong khi lần quét của `T2` không thấy `k3` mà `T0` ghi (vậy `T2` phải đứng trước `T0`): không tồn tại thứ tự tuần tự nào. Đó là rw-antidependency: T1 ghi một phiên bản mà T2 đã đọc phiên bản trước nên T1 xuất hiện *sau* T2,[^ports-rw] và các phụ thuộc cũng có thể sinh từ đọc theo điều kiện (quét), không chỉ đọc từng dòng.[^ports-predicate] (Cạnh tranh cùng một dòng chưa phải điều kiện cần: write skew là hai transaction ghi hai dòng khác nhau.)[^ber-a5b]

---

### Lab (f) — `batch`: external sort, shuffle, combiner, join

Lý thuyết: [Chương 10 — Batch processing](../12-he-thong-du-lieu/10-batch-processing.md), phần Unix pipeline và sort, phần MapReduce (map, shuffle, reduce, combiner) và phần join.

Code: `batch.rs`. `external_sort` sắp xếp nhiều dòng hơn bộ nhớ bằng cách tạo các run đã sắp rồi gộp nhiều pha với fan-in cho trước. `word_count` chạy mô hình map → partition (băm FNV) → shuffle → sort → reduce, bật/tắt combiner. `sort_merge_join` là reduce-side join với bản ghi gắn nhãn và hai cách reduce (`SecondarySort`, `BufferWholeKey`); `hash_join` là join phía map (broadcast). Dữ liệu sinh ra có phân phối Zipf và một user "nóng" chiếm 25% click.

```bash
cd labs && cargo run -q --release -p lab-data --bin batch
```

Output (macOS aarch64, rustc 1.95.0, `--release`):

```text
== 1. external merge sort: more lines than memory ==
input: 300,000 lines of 21 bytes (6.6 MB); at most 10,000 lines held in memory at a time
  fan-in   runs  merge passes     spilled MB  output == in-memory sort
       2     30             5           32.6  true
       4     30             3           19.8  true
      32     30             1            6.6  true
("spilled" counts runs and intermediate merge files; the final output is not included)

== 2. word count: map -> partition -> shuffle -> sort -> reduce ==
input: 100,000 lines x 10 words = 1,000,000 words (7.0 MB), Zipf weights 1/rank over 5,000 distinct words; M = 8 map tasks, R = 4 reduce tasks
combiner   shuffled recs   shuffle MB  sort runs  records per reduce partition        distinct == in-memory count
off            1,000,000          9.0         22  [225395, 249260, 310137, 215208]       5,000 true
on                39,377          0.4          4  [9825, 9822, 9860, 9870]               5,000 true
most frequent: w00000=110,099  w00001=54,830  w00002=36,561  w00003=27,115  w00004=22,086
w00000 is 11.0% of all words; the partition function sends it (and nothing can split it) to reduce partition 2

== 3. join: clicks x users ==
users: 20,000 rows (id, country); clicks: 400,000 rows (id, url); 25% of the clicks belong to user 0, 1% have an id with no user row
reduce-side sort-merge join, R = 8 reducers, records tagged (0 = user, 1 = click), sorted by (id, tag):
reduce function      joined  orphans  peak records  shuffle MB  records per reduce partition
secondary sort      396,977    3,023             1         9.5  [40008, 39993, 39845, 40074, 40204, 39700, 40112, 140064]
  (clicks of the hottest key: 99,795; biggest partition / mean partition = 2.67; "peak records" = most records the reduce function kept in memory)
buffer whole key    396,977    3,023        99,796         9.5  [40008, 39993, 39845, 40074, 40204, 39700, 40112, 140064]
hash join (map side, no shuffle): 20,000 users held in memory, 396,977 rows joined, same rows as the sort-merge join: true

== 4. a chain of three jobs: clicks per country ==
job 1 (join) -> 396,977 rows -> job 2 (map-only: keep column `country`) -> 396,977 lines -> job 3 (word count with a combiner)
job 3 shuffled 160 records for 40 distinct countries
top countries by clicks: c22=107,307  c10=7,986  c12=7,984  c25=7,921
```

**Đọc output.**

- *Mục 1 (external sort).* 300.000 dòng, chỉ giữ tối đa 10.000 dòng trong bộ nhớ: sinh ra 30 run đã sắp. Gộp 2 run một lần cần 5 pha và ghi tạm 32,6 MB; fan-in 4 cần 3 pha (19,8 MB); fan-in 32 gộp một phát (1 pha, 6,6 MB, chính là kích thước các run ban đầu). Mọi cấu hình cho kết quả giống sắp trong bộ nhớ (`true`). Công cụ `sort` của GNU làm đúng việc này: khi phải gộp nhiều hơn `nmerge` đầu vào, nó gộp theo nhóm `nmerge`, ghi kết quả vào file tạm rồi dùng làm đầu vào của lần gộp sau.[^sort-merge] MapReduce cũng dùng external sort ở phía reduce khi dữ liệu trung gian không vừa bộ nhớ.[^mr-extsort] Khi đó số pha là `ceil(log_fan-in(số run))`: 30 run với fan-in 2 cho 5 pha, đúng với bảng (suy luận của người soạn từ output).
- *Mục 2 (word count).* Bản đồ đi qua hàm phân vùng `hash(key) mod R`,[^mr-part] nên **mọi** lần xuất hiện của một từ về cùng một reducer. Từ `w00000` chiếm 11,0% toàn bộ từ (phân phối Zipf) và nằm trọn trong partition 2 (310.137 bản ghi so với 215.208 đến 249.260 ở các partition khác): không có gì chia nhỏ được một key nóng. Combiner "gộp từng phần trên dữ liệu trước khi gửi qua mạng":[^mr-combiner] shuffle giảm từ 1.000.000 xuống 39.377 bản ghi (9,0 MB xuống 0,4 MB, giảm hơn 20 lần) và các partition đều lại (khoảng 9.8k mỗi cái). Động cơ giống bài báo gốc: tần suất từ theo phân phối Zipf nên mỗi map task sinh ra hàng trăm hay nghìn bản ghi dạng (từ phổ biến, 1);[^mr-comb-the] điều kiện để dùng combiner là hàm reduce giao hoán và kết hợp.[^mr-comb-why] Cả hai chế độ cho cùng 5.000 từ đúng với đếm trong bộ nhớ (`true`).
- *Mục 3 (join).* Reduce-side join: mỗi map task gắn nhãn bản ghi theo bảng nguồn rồi xuất (khóa join, bản ghi gắn nhãn); framework tự phân vùng, sắp xếp, gộp.[^join-std] Trong cách `BufferWholeKey`, reducer phải giữ **mọi** bản ghi của một khóa trong bộ nhớ, và đây đúng là điểm yếu bài báo cảnh báo: mọi bản ghi của một khóa join từ cả hai bảng phải được đệm.[^join-buf] Ở khóa nóng đó là 99.796 bản ghi (so với 1 ở `SecondarySort`); trong các trường hợp cực đoan, join chuẩn không chạy xong vì hết bộ nhớ.[^join-oom] Cải tiến là sắp sao cho bản ghi bảng nhỏ đứng trước, rồi chỉ đệm bảng nhỏ và *stream* bảng lớn,[^join-impr] đó chính là `SecondarySort` (peak = 1, vì mỗi user chỉ có một dòng ở bảng nhỏ). Dù bộ nhớ đã được cứu, **tải vẫn lệch**: partition cuối nhận 140.064 bản ghi, gấp 2,67 lần trung bình, và dữ liệu lệch là nguyên nhân khiến join đắt.[^join-skew] Thời gian chạy toàn bộ bị chặn bởi đơn vị không chia nhỏ được lớn nhất.[^sre-skew] Cột `shuffle MB` bằng nhau (9,5) ở hai cách vì chiến lược reduce không đổi lượng dữ liệu phải shuffle. Cuối cùng, vì bảng `users` (20.000 dòng) nhỏ, **hash join phía map** không cần shuffle nào: broadcast join chạy như một job chỉ có map, mỗi map task dùng bảng băm trong bộ nhớ để join một split của bảng lớn,[^join-bc-job] và cho đúng 396.977 dòng như sort-merge join (`true`).
- *Mục 4 (chuỗi job).* Job 1 (join) rồi job 2 (map-only, giữ cột `country`) rồi job 3 (đếm với combiner): job 3 chỉ shuffle 160 bản ghi cho 40 quốc gia, và `c22` (quốc gia của user nóng) áp đảo với 107.307 click. Đầu ra của job này là đầu vào của job kia, giống các file đầu ra của MapReduce thường được dùng làm đầu vào của lần MapReduce khác.[^mr-outfiles]

---

### Lab (g) — `percentile`: đuôi độ trễ, fan-out, hedging, coordinated omission

Lý thuyết: [Chương 01 — Độ tin cậy, khả năng mở rộng, bảo trì](../12-he-thong-du-lieu/01-reliability-scalability-maintainability.md), phần mô tả tải và hiệu năng (percentile, đuôi độ trễ, fan-out, hedged requests).

Code: `latency.rs`. Định nghĩa percentile của lab là *nearest-rank* (một định nghĩa của lab, không phải chuẩn của thư viện nào). `Histogram` có thể cộng gộp (bucket 1%), `fan_out` mô phỏng một request chờ N yêu cầu con, `hedged_fan_out` gửi lại sau ngưỡng p95 của một yêu cầu con, và `coordinated_omission` chạy mô hình thời gian ảo.

```bash
cd labs && cargo run -q --release -p lab-data --bin percentile
```

Output (macOS aarch64, rustc 1.95.0, `--release`; số liệu sinh bằng RNG có hạt giống cố định, chữ số cuối có thể lệch trên nền tảng khác):

```text
== 1. the mean hides the tail ==
workload A: 95% of requests take 50 ms, 5% take 1000 ms (1000 requests)
  mean 97.5 ms   p50 50 ms   p95 50 ms   p99 1000 ms
workload B: lognormal body (median 10 ms, sigma 0.35) plus a 1% chance of a ~1000 ms stall (200,000 requests)
  mean 20.6 ms   p50 10.0   p95 18.5   p99 808.2   p99.9 1164.3   max 1219.6

== 2. percentiles cannot be averaged ==
10 machines: #0 serves 40% of the traffic and stalls on 3% of its requests; #1-#9 share 60% and stall on 0.1%
  mean of the 10 per-machine p99          :    129.2 ms
  per-machine p99 weighted by traffic     :    447.6 ms
  p99 of all 100,000 requests pooled      :    907.9 ms   <- the number users experience
  p99 from the sum of the 10 histograms   :    914.0 ms   (1% buckets, so within 1% of the pooled value)

== 3. fan-out: a request waits for the slowest of N parallel sub-requests ==
one sub-request is "slow" (> 500 ms) with probability p = 0.0100 (measured on 1,000,000 draws); 20,000 requests per row
    N      requests slow      1 - (1-p)^N     p50 ms     p99 ms
    1               0.9%             1.0%       10.0       27.7
    2               2.1%             2.0%       12.2     1002.1
    5               5.2%             4.9%       15.1     1134.7
   10               9.7%             9.5%       17.3     1166.8
   50              39.9%            39.3%       25.5     1204.1
  100              63.8%            63.2%      937.4     1206.0
  200              86.6%            86.5%     1073.8     1209.4
  500              99.3%            99.3%     1154.4     1212.7

== 4. hedged requests, fan-out 100 ==
a sub-request that has not answered after 18.4 ms (the p95 of one sub-request) is sent again to another replica; first answer wins
            requests slow    p50 ms    p99 ms   p99.9 ms   extra load
no hedge            63.7%     936.6    1206.8          -           0%
hedged               1.0%      27.7      49.8     1073.9         5.0%
(the model assumes the second copy's latency is independent of the first; a correlated cause, such as an overloaded rack, breaks that)

== 5. coordinated omission (virtual time) ==
the server answers in 1 ms for 100 s, then stalls for 100 s; the client intends one request every 10 ms
samples (ms)                           count     <= 1 ms       p50        p99     p99.99        max
closed loop (waits for each reply)    10,001      99.99%         1          1          1     100000
closed loop + correction              20,000      50.00%         1      98000      99980     100000
open loop (FIFO queue model)          20,000      50.00%         1      98201      99983     100001
```

**Đọc output.**

- *Mục 1.* Workload A bắt chước ví dụ trong cuốn SRE của Google ("một request điển hình mất khoảng 50 ms nhưng 5% chậm gấp 20 lần"):[^sre-fig41] trung bình 97,5 ms không phải con số của bất kỳ request nào (một nửa mất 50 ms, một phần trăm cuối mất 1000 ms), còn p50, p95, p99 kể đúng câu chuyện. SRE book khuyên coi hầu hết các metric như phân phối chứ không phải trung bình,[^sre-dist] và dùng percentile cao (p99, p99.9) để thấy trường hợp xấu hợp lý, còn p50 nhấn vào trường hợp điển hình.[^sre-pct] Lưu ý p95 = 50 ms ở A là do định nghĩa nearest-rank với chính xác 95% mẫu ≤ 50 ms: kết quả ở đúng ranh giới nhạy với định nghĩa.
- *Mục 2 (không lấy trung bình percentile).* Máy #0 nhận 40% lưu lượng và treo 3% request. Trung bình các p99 theo từng máy là 129,2 ms, p99 gia quyền theo lưu lượng là 447,6 ms, p99 của **toàn bộ** 100.000 request gộp lại là 907,9 ms: trung bình cộng và cả gia quyền đều sai, vì percentile không phải một đại lượng cộng được. Cách đúng là gộp *histogram* (cộng từng bucket) rồi lấy percentile của histogram gộp: 914,0 ms, lệch dưới 1% so với giá trị gộp thật vì bucket rộng 1%. Thư viện HdrHistogram cũng cung cấp phép `add` để cộng nội dung histogram này vào histogram khác.[^hdr-add]
- *Mục 3 (fan-out).* Nếu mỗi yêu cầu con chậm với xác suất `p = 1%` thì một request chờ N yêu cầu con chậm với xác suất `1 - (1-p)^N`. Cột đo (từ mô phỏng 20.000 request) bám sát cột công thức: N = 100 cho 63,8% đo được so với 63,2% theo công thức. Con số 63% chính là con số trong bài "The Tail at Scale": nếu một request phải thu thập phản hồi từ 100 server như vậy song song thì 63% request mất hơn một giây.[^tail-100] p50 cũng đổi chế độ: từ 25,5 ms ở N = 50 nhảy lên 937,4 ms ở N = 100; "lúc bình thường của đuôi" trở thành lúc bình thường của người dùng.
- *Mục 4 (hedging).* Gửi lại một yêu cầu con sau 18,4 ms (p95 của yêu cầu con đơn) ở fan-out 100 kéo tỉ lệ request chậm từ 63,7% xuống 1,0% và p50 từ 936,6 xuống 27,7 ms, với giá 5,0% tải thêm. "The Tail at Scale" cũng nêu cách làm này giới hạn tải thêm ở khoảng 5% mà rút ngắn đáng kể đuôi độ trễ.[^tail-hedge] **Giới hạn** do chính bin ghi trong output: mô hình giả định độ trễ của bản gửi lại *độc lập* với bản đầu; một nguyên nhân tương quan (cả rack quá tải) phá vỡ giả định đó và làm hedging kém hiệu quả hơn nhiều (nhận xét của người soạn, chưa đo; bài tập 8).
- *Mục 5 (coordinated omission).* Máy chủ trả lời 1 ms trong 100 s rồi treo 100 s; client muốn gửi mỗi 10 ms. Vòng lặp *đóng* (đợi từng phản hồi rồi mới gửi tiếp) chỉ có 10.001 mẫu, 99,99% ≤ 1 ms và p99 = 1 ms: nó **bỏ sót** mọi request lẽ ra đã được gửi trong lúc máy chủ treo. Với tài liệu HdrHistogram, đó đúng là kịch bản: lấy mẫu mỗi 10 ms và máy chủ đứng 100 s.[^hdrj-setup] Sau khi bù, chỉ khoảng 50% kết quả ≤ 1 ms, phần còn lại là các bản ghi tự sinh cho các khoảng bị thiếu.[^hdrj-co] Output của lab trùng khớp: 20.000 mẫu, đúng 50,00% ≤ 1 ms, p99 = 98.000 ms. Thư viện `hdrhistogram` thực hiện bù bằng cách tự sinh thêm chuỗi bản ghi nhỏ dần cho tới khoảng cách mong đợi.[^hdr-rc] Mô hình hàng đợi FIFO (vòng lặp *mở*, không đợi) cho p99 = 98.201 ms, cùng bậc với bản đã bù: lời đáp đúng cho "người dùng thật thấy gì".

---

### Lab (h) — `sql/*.sql`: cùng hiện tượng trên PostgreSQL 16 (chưa chạy)

Lý thuyết: [Chương 07 — Transaction](../12-he-thong-du-lieu/07-transaction.md) và [Chương 15 — PostgreSQL bên trong](../12-he-thong-du-lieu/15-postgresql-ben-trong.md) (MVCC, VACUUM, khóa, EXPLAIN).

> **Chưa chạy.** Tác giả chỉ chạy `cargo` trên macOS và không dùng Docker hay `psql`. Mọi mục "Kỳ vọng" dưới đây là **dự đoán** suy ra từ tài liệu PostgreSQL 16 đã dẫn, **không phải output quan sát được**. Hãy chạy rồi so với output thật của bạn. Số liệu cụ thể (kích thước, thời gian, kế hoạch mà planner chọn) phụ thuộc máy, cấu hình và thống kê của bảng, nên chỉ so *trong cùng một lần chạy*.

Năm script nằm ở `labs/crates/lab-data/sql/`. Khởi động server một lần và chạy từng script từ thư mục gốc của repo:

```bash
docker run --name lab-pg -e POSTGRES_PASSWORD=lab -d postgres:16
docker exec -i lab-pg psql -U postgres -v ON_ERROR_STOP=1 -f - < labs/crates/lab-data/sql/01_mvcc_xmin_xmax.sql
# ... 02 đến 05 tương tự; dọn dẹp:
docker rm -f lab-pg
```

Image `postgres` bắt buộc có `POSTGRES_PASSWORD` không rỗng,[^docker-pw] và người dùng `postgres` cùng database mặc định được tạo bằng `initdb` trong entrypoint.[^docker-initdb] `-v ON_ERROR_STOP=1` làm `psql` dừng ngay khi gặp lỗi (mặc định nó chạy tiếp sau lỗi),[^psql-onerror] còn `-f -` đọc script từ đầu vào chuẩn.[^psql-stdin] Phần nhiều bước **hai phiên** (A, B, C, D) được viết dưới dạng chú thích `-- [A] …`: dán từng dòng vào hai hoặc ba cửa sổ `docker exec -it lab-pg psql -U postgres` theo đúng thứ tự.

| Script | Mô phỏng ở đâu trong lab | Việc cần quan sát |
|---|---|---|
| `01_mvcc_xmin_xmax.sql` | Lab (e) mục 1 | `ctid`, `xmin`, `xmax`, `pageinspect`, `pg_current_snapshot()`, đọc cũ ở Repeatable Read |
| `02_write_skew.sql` | Lab (e) mục 2 | lost update, write skew ở Repeatable Read so với Serializable, sửa bằng `FOR UPDATE` |
| `03_vacuum_dead_tuples.sql` | Lab (e) mục 3 | phiên bản chết, `VACUUM VERBOSE`, transaction dài chặn dọn dẹp, `VACUUM FULL` |
| `04_locks.sql` | (không có trong mô hình) | `pg_locks`, `pg_blocking_pids`, hàng đợi khóa của DDL, deadlock |
| `05_explain.sql` | (không có trong mô hình) | `EXPLAIN (ANALYZE, BUFFERS)`, ước lượng so với thực tế, index-only scan |

**01: phiên bản dòng.** Kỳ vọng: sau `INSERT`, `SELECT ctid, xmin, xmax, *` cho `ctid = (0,1)` và `xmax = 0`; sau `UPDATE` trong một transaction, dòng mới có `ctid` khác, vì tài liệu nói `ctid` đổi khi dòng bị cập nhật.[^ddl-ctid] `pg_current_xact_id()` gán cho transaction một id.[^pg-curxid] `heap_page_items(get_raw_page(…))` của module `pageinspect` hiện *mọi* tuple trong trang dù nhìn thấy được hay không,[^pageinspect-hpi] nên bạn phải thấy hai tuple: bản cũ với `t_xmax` bằng id của transaction vừa cập nhật và `t_ctid` trỏ tới bản mới; mã nguồn heap ghi đúng hai việc đó (lưu transaction cập nhật vào tuple cũ và ghi địa chỉ tuple mới vào `t_ctid` của tuple cũ).[^hu-xmax][^hu-ctid] `pg_current_snapshot()` in dạng `xmin:xmax:xip_list`.[^snap-text] Ở hai phiên: phiên A mở Repeatable Read và đọc, phiên B cập nhật và commit; A đọc lại phải vẫn thấy dữ liệu cũ,[^rr-snapshot] và `UPDATE` của A phải lỗi `40001` (`could not serialize access due to concurrent update`);[^rr-concurrent-update][^err-codes] cùng chuỗi lệnh ở Read Committed thì `UPDATE` của A không lỗi vì WHERE được đánh giá lại trên phiên bản mới.[^rc-reeval] Chú ý: `xmin` trong `SELECT` có kiểu `xid` (32 bit) còn `pg_current_xact_id()` trả `xid8`; trên cụm mới hai con số trùng nhau (suy luận của người soạn).

**02: write skew.** Kỳ vọng: (1) lost update ở Read Committed, hai phiên cùng đọc `n = 0` rồi cùng ghi `n = 1`, cho kết quả cuối `1`;[^ber-p4] ở Repeatable Read lệnh `UPDATE` thứ hai lỗi `40001`, còn `UPDATE counter SET n = n + 1` nguyên tử không mất cập nhật vì transaction thứ hai bắt đầu từ phiên bản mới của dòng.[^rc-bank] (2) Write skew với `doctors`: ở Repeatable Read cả hai `COMMIT` thành công và số bác sĩ trực là 0, đúng ví dụ Ports và Grittner mô tả;[^ports-doctors] ở Serializable một trong hai transaction bị hủy với `could not serialize access due to read/write dependencies among transactions`[^ser-example] (lỗi xảy ra ở `UPDATE` hay `COMMIT` tùy thời điểm, tôi không khẳng định bên nào) và ứng dụng phải thử lại cả transaction.[^ser-retry] (3) Sửa bằng `SELECT … FOR UPDATE` ở Repeatable Read: phiên B bị chặn cho tới khi A kết thúc,[^forupdate-block] rồi lỗi `40001` vì dòng đã đổi sau snapshot của B.[^forupdate-rr-error]

**03: VACUUM.** Bảng `churn` được tạo với `autovacuum_enabled = false` để bạn tự quyết khi nào VACUUM chạy.[^av-enabled] Kỳ vọng: sau ba lần `UPDATE` toàn bảng, `pg_relation_size` lớn rõ rệt so với ban đầu (cận trên khoảng 4 lần; dọn dẹp trong trang có thể làm số thật thấp hơn: tài liệu về HOT nói phiên bản cũ có thể được gỡ trong hoạt động bình thường, kể cả khi `SELECT`, không cần VACUUM định kỳ[^hot-prune]) và `n_dead_tup` lớn,[^pgstat-dead] lưu ý bộ đếm thống kê được cache tới hết transaction hiện tại.[^pgstat-cache] `VACUUM (VERBOSE)` in một dòng dạng `tuples: N removed, M remain, K are dead but not yet removable`;[^vl-log] kích thước file **không** giảm vì VACUUM thường không trả không gian cho hệ điều hành (trừ trang trống ở cuối bảng), chỉ đánh dấu chỗ trống để tái sử dụng,[^vac-space] nên một `UPDATE` tiếp theo không làm file phình thêm đáng kể. `VACUUM FULL` viết lại bảng và cần khóa `ACCESS EXCLUSIVE`, tức không chạy song song với bất kỳ truy cập nào khác vào bảng.[^vac-full] Phần hai phiên: một transaction `REPEATABLE READ` để mở khiến `K` (dead but not yet removable) khoảng 100.000 vì phiên bản cũ có thể còn được nó nhìn thấy; `pg_stat_activity.backend_xmin` là "xmin horizon" của backend đó,[^bxmin] và các phiên rảnh giữa transaction ngăn dọn các tuple "mới chết" chỉ nhìn thấy được bởi chính chúng.[^iit] Sau `COMMIT` ở A, VACUUM kế tiếp gỡ được chúng. Đây là bản thật của Lab (e) mục 3.

**04: khóa.** Kỳ vọng: khi A giữ khóa dòng (UPDATE chưa commit) và B cập nhật cùng dòng, B bị treo. Trong `pg_locks` B có một dòng `locktype = transactionid` ở trạng thái `granted = f` chờ transaction của A, đúng như tài liệu: một tiến trình chờ transaction khác kết thúc sẽ cố lấy khóa share trên id của transaction đó, còn transaction đã có id thì giữ khóa exclusive trên id của chính nó;[^pglocks-xid] khóa dòng không nằm trong view mà ghi trên đĩa, nên tiến trình đang chờ khóa dòng thường hiện ra như đang chờ id transaction của người đang giữ.[^pglocks-row] `granted` là `true` cho khóa đang giữ và `false` cho khóa đang chờ.[^pglocks-granted] Trong `pg_stat_activity`, B có `wait_event_type = Lock` và `wait_event = transactionid`,[^we-xid] và `pg_blocking_pids(pid_B)` chứa pid của A. Phần hai: A giữ `ACCESS SHARE` (một `SELECT` trong transaction mở),[^lk-as] `ALTER TABLE … ADD COLUMN` của B cần `ACCESS EXCLUSIVE`[^lk-ae] nên chờ A; rồi một `SELECT` thường của C, vốn **không** xung đột với A, vẫn bị treo vì xếp sau yêu cầu `ACCESS EXCLUSIVE` đang chờ: yêu cầu khóa chỉ được cấp ngay nếu không xung đột với khóa hiện có *hoặc đang chờ*,[^lock-rule1] và các yêu cầu xung đột được cấp theo thứ tự đến.[^lmgr-fifo] `pg_blocking_pids(pid_C)` vì vậy chứa pid của B (một "soft block").[^blockpids] Biện pháp: `SET lock_timeout = '2s'` để DDL thất bại nhanh thay vì xếp hàng,[^lt-def] ứng dụng thử lại sau. Phần ba: hai phiên cập nhật hai dòng theo thứ tự ngược nhau tạo vòng chờ; sau `deadlock_timeout` (mặc định 1 giây)[^dl-timeout-def] một phiên nhận `ERROR: deadlock detected` với SQLSTATE `40P01`,[^err-codes] và ứng dụng phải thử lại transaction;[^lock-deadlock-retry] phòng thủ tốt nhất là luôn lấy khóa theo cùng một thứ tự.[^dl-order]

**05: EXPLAIN.** Bảng `events` 1.000.000 dòng; script tắt parallel query bằng `SET max_parallel_workers_per_gather = 0`[^par-off] để kế hoạch không có nút `Gather`. Kỳ vọng và cách đọc: (1) tìm một dòng theo khóa chính cho `Index Scan using events_pkey` và chỉ vài buffer; kế hoạch này thường thấy với truy vấn lấy đúng một dòng;[^indexscan-use] (2) lọc theo cột không có chỉ mục cho `Seq Scan`; một sequential scan luôn là một kế hoạch khả dĩ;[^plan-seq] dòng `Rows Removed by Filter` chỉ xuất hiện khi có dòng bị bộ lọc loại[^rows-removed] và `Buffers` xấp xỉ số trang của bảng, vì `BUFFERS` giúp nhận ra phần nào của truy vấn tốn I/O nhất;[^explain-buffers] (3) sau `CREATE INDEX`, cùng truy vấn nên chuyển sang `Index Scan` hoặc `Bitmap Heap Scan` (bitmap sắp vị trí dòng theo thứ tự vật lý trước khi đọc[^bitmap-why]) với thời gian nhỏ hơn hẳn; (4) chỉ mục trên cột `kind` với 99,9% dòng có cùng một giá trị: truy vấn `kind = 'view'` vẫn nên là `Seq Scan` vì đọc gần hết bảng qua chỉ mục đắt hơn; planner chọn theo chi phí, không phải "có chỉ mục là dùng"; (5) hai cột tương quan: planner mặc định coi các điều kiện độc lập,[^ext-indep] nên `WHERE a = 1 AND b = 1` được ước lượng `rows = 10` (100.000 × 1/100 × 1/100) trong khi thực tế là 1000; tạo extended statistics để bảo server thu thống kê trên nhóm cột[^ext-create] rồi `ANALYZE` thì ước lượng phải gần 1000; thống kê luôn chỉ là xấp xỉ;[^stats-approx] (6) `EXPLAIN ANALYZE` **thực sự chạy** câu lệnh,[^explain-analyze] mọi tác dụng phụ vẫn xảy ra, nên bọc lệnh ghi trong `BEGIN … ROLLBACK`;[^explain-sideeffect] script kiểm lại số dòng sau rollback; (7) index-only scan cần visibility map: sau `VACUUM`, `Heap Fetches` nên bằng 0 hoặc rất nhỏ,[^vm-only] còn sau một `UPDATE` ngay trên các trang đó thì `Heap Fetches` lớn hơn 0 cho tới lần VACUUM sau (nhãn `Heap Fetches` là đếm lần truy cập heap, xuất hiện trong output của `EXPLAIN ANALYZE` cho index-only scan theo mã nguồn[^explain-heapfetch]). Mỗi giá trị `actual time` là mili giây thời gian thực, còn chi phí kế hoạch dùng đơn vị tùy ý nên hai thứ không so trực tiếp được; điều quan trọng nhất cần nhìn là ước lượng số dòng có gần thực tế không.[^actual-ms]

---

### Bài tập mở rộng

1. **Kvlog nhiều segment và hint file.** Chia `kvlog` thành các segment bất biến cộng một segment đang ghi, thêm hint file cho khởi động nhanh[^bc-scan] và merge các segment cũ.[^bc-merge2] Thử lại bẫy tombstone: nếu merge chỉ một phần các segment mà bỏ tombstone thì key cũ ở segment không được merge sống lại (so với Lab (b) mục 2).
2. **Full-page write cho `pagestore`.** Thêm full-page write sau mỗi checkpoint (ghi nguyên trang vào WAL lần đầu trang bị sửa), thêm bước phát hiện trang bị xé khi mở và đo lại WA ở các mức checkpoint khác nhau: số của Lab (b) mục 5 sẽ tăng bao nhiêu?
3. **Compaction theo phân vùng.** Trong `lsm`, chia mỗi tầng leveled thành nhiều file theo dải khóa và chỉ gộp một file cộng các file chồng lấn ở tầng dưới (kiểu LevelDB[^ldb-drop]); thêm "trivial move". WA có tiến gần lý thuyết "bằng fanout" không?
4. **SSI theo thứ tự commit.** Bản `mvcc` hủy mọi pivot; hãy cài thêm điều kiện "T3 là transaction commit đầu tiên của chu trình"[^ports-t3first] và đo lại số transaction bị hủy ở mục 4 (24.851 ở SSI). Số dương giả giảm bao nhiêu mà vẫn giữ 0 kết quả không tuần tự hóa được?
5. **Map song song.** Chạy các map task của `batch` bằng nhiều luồng và đo thời gian; shuffle vẫn phải xác định (cùng đầu ra với bản một luồng).
6. **Giải quyết schema theo tên (kiểu Avro).** Viết bộ đọc ghép trường theo tên giữa schema của writer và reader (mặc định cho trường mới, lỗi nếu thiếu trường không có mặc định)[^avro-resolve-names] và so với cách ghép theo tag của protobuf trong Lab (d).
7. **Truy xuất hai pha cho column store.** Ở Lab (c), đọc `country` trước, lấy danh sách vị trí khớp rồi mới đọc các cột còn lại (position list);[^ab-late2] đo khi số dòng khớp là 0,5%, 5% và 50%.
8. **Hedging với độ trễ tương quan.** Sửa `hedged_fan_out` để độ trễ của bản gửi lại tương quan với bản đầu (ví dụ chung một rack chậm) và vẽ lại hiệu quả hedging theo mức tương quan.
9. **Chạy các script SQL của Lab (h) rồi so với mô hình.** Điền vào bảng "kỳ vọng so với quan sát", đặc biệt: kích thước bảng sau VACUUM, giá trị `K` ở bước transaction dài, và liệu planner có chọn đúng các kế hoạch đã dự đoán. Báo lại chỗ nào kỳ vọng sai.

---

## Câu hỏi tự kiểm tra (về output lab)

1. Ở Lab (a) mục 1, file `kvlog` là 3.046.000 B nhưng chỉ 9.000 key sống, `space_amp=2.80`. Vì sao file lớn như vậy, và compaction ở mục 4 đưa `space_amp` về 1,00 bằng cách nào?

   <details><summary>Đáp án</summary>

   Mọi ghi đè và xóa đều là một bản ghi *mới* nối vào đuôi file mà không chạm bản cũ, nên 1.957.000 B là các phiên bản đã bị che hoặc bị xóa.[^bc-merge] Compaction ghi một file mới chỉ chứa phiên bản mới nhất của từng key còn sống (và bỏ tombstone, an toàn vì chỉ có một file), `fsync`, `rename` đè lên file cũ rồi `fsync` thư mục: 736.500 B về 181.500 B.

   </details>

2. Ở Lab (a) mục 2, vì sao "ghi tiếp sau đuôi chưa sửa" nguy hiểm hơn cắt đuôi ngay lúc mở?

   <details><summary>Đáp án</summary>

   Với bản ghi ngắn (17 B), lần mở sau thấy `Incomplete`, cắt 30 B và `get(new)=None`: một bản ghi đã được báo thành công bị nuốt mất âm thầm. Với bản ghi dài (51 B) phần hỏng nằm *giữa* file nên recovery từ chối mở. Cách đúng là cắt phần đuôi dở trước khi nhận ghi mới.

   </details>

3. Ở Lab (b) mục 3, 10 bit/key cho `0,067` block/lần tra key không tồn tại và FP rate 0,84%. Hai con số này liên hệ với nhau thế nào, và chúng khớp với bảng của RocksDB ra sao?

   <details><summary>Đáp án</summary>

   2.688 false positive trên 40.000 lần tra là `2.688 / 40.000 = 0,0672` block mỗi lần tra: mỗi false positive đọc đúng một block. Hướng dẫn RocksDB cho 9,9 bit/key tương ứng 1% dương giả,[^rdb-bloom-bits] và 0,84% ở 10 bit/key là cùng bậc (khác một chút vì băm FNV-1a và số bit không đúng bằng 9,9).

   </details>

4. Ở Lab (b) mục 4, tiered fanout 4 có WA 4,20 còn leveled fanout 10 có WA 8,80, nhưng leveled chỉ có 2 run. Đánh đổi nào đang diễn ra?

   <details><summary>Đáp án</summary>

   Leveled gộp mạnh tay hơn: ghi lại dữ liệu nhiều lần (147,7 MB compaction so với 44,9 MB) để giữ ít run (2 so với 5) và ít chỗ thừa (`space amp` 1,15 so với 1,69). Tiered tiết kiệm ghi nhưng để nhiều run hơn, tốn đọc và không gian hơn.[^rdb-comp-tier][^rdb-comp-lvl] Số cụ thể là của engine đơn giản hóa này, không phải RocksDB.

   </details>

5. Ở Lab (b) mục 2, `AlwaysDrop` làm key đã xóa "sống lại". Tombstone được phép bỏ khi nào?

   <details><summary>Đáp án</summary>

   Chỉ khi không còn run nào *cũ hơn* (tầng cao hơn) có thể chứa key đó: LevelDB bỏ dấu xóa chỉ khi không có tầng cao hơn nào chứa file có dải khóa chồng lấn với key.[^ldb-drop] Ở kịch bản của lab, lần gộp chỉ gồm các run mới và bỏ sót run cũ nhất chứa `k1 = "the-value"`, nên bỏ tombstone làm giá trị cũ lộ ra.

   </details>

6. Ở Lab (e) mục 2, vì sao write skew lọt qua Snapshot Isolation nhưng bị `Serializable` chặn, và vì sao hai cách sửa ở tầng ứng dụng lại hiệu quả ở mọi mức?

   <details><summary>Đáp án</summary>

   Hai transaction đọc cùng một tập dòng rồi ghi hai dòng *khác nhau* nên không có xung đột ghi-ghi để first-committer-wins bắt.[^ports-wl] `Serializable` theo dõi các phụ thuộc đọc-ghi (rw) và hủy khi thấy cấu trúc nguy hiểm.[^ports-dangerous] Cách sửa 1 (cập nhật một dòng dùng chung) biến xung đột thành ghi-ghi; cách sửa 2 (`FOR UPDATE`) làm transaction thứ hai bị chặn tới khi transaction thứ nhất kết thúc,[^forupdate-block] và khi thử lại nó thấy trạng thái mới.

   </details>

7. Ở Lab (e) mục 4, `Serializable` cho 0 kết quả không tuần tự hóa được nhưng hủy 24.851 transaction (so với 16.530 ở SI). Con số 24.851 nói gì?

   <details><summary>Đáp án</summary>

   SSI chấp nhận *dương giả*: nó hủy cả những transaction không thật sự nằm trong chu trình, vì không phải cấu trúc nguy hiểm nào cũng thành chu trình.[^ports-fp] Bản của lab hủy mọi pivot; PostgreSQL có tối ưu theo thứ tự commit để giảm bớt.[^ports-commitorder] Ứng dụng phải sẵn sàng thử lại.[^ser-retry]

   </details>

8. Ở Lab (d) mục 5, vì sao dùng lại số tag 3 làm `uint32` báo lỗi, còn làm `string` thì chạy êm nhưng sai?

   <details><summary>Đáp án</summary>

   Dạng `uint32` cần wire type `VARINT` còn byte cũ là `LEN`, nên decoder của lab phát hiện lệch. Dạng `string` cùng wire type `LEN` nên không có gì để phát hiện: wire format không cho cách nào biết byte được mã hóa theo định nghĩa khác.[^pb3-lean] Vì thế phải `reserved` số tag đã xóa.[^pb3-reserve]

   </details>

9. Ở Lab (f) mục 3, `SecondarySort` có peak 1 còn `BufferWholeKey` có peak 99.796, nhưng cả hai cùng 140.064 bản ghi ở partition cuối. Vì sao bộ nhớ được cứu mà tải không cân?

   <details><summary>Đáp án</summary>

   `SecondarySort` sắp để bản ghi user (bảng nhỏ) đứng trước rồi chỉ đệm bảng nhỏ và stream bảng lớn,[^join-impr] nên peak là 1. Nhưng **phân vùng** vẫn theo hash của khóa join, và khóa nóng (25% số click) luôn rơi vào một partition: 140.064 bản ghi, gấp 2,67 lần trung bình, và thời gian chạy bị chặn bởi partition lớn nhất.[^sre-skew]

   </details>

10. Ở Lab (g) mục 2, vì sao `mean of the 10 per-machine p99` (129,2 ms) sai so với p99 gộp (907,9 ms), và cách nào cho số đúng?

    <details><summary>Đáp án</summary>

    Percentile không cộng được; máy chậm nhất chiếm 40% lưu lượng, nên p99 toàn hệ thống bị nó kéo lên. Cách đúng: gộp dữ liệu thô, hoặc cộng các histogram (bucket với bucket) rồi lấy percentile của histogram gộp: 914,0 ms.[^hdr-add]

    </details>

11. Ở Lab (g) mục 5, vòng lặp đóng báo p99 = 1 ms còn bản bù báo 98.000 ms. Con số nào đúng với người dùng, và vì sao vòng lặp đóng "quên" mất?

    <details><summary>Đáp án</summary>

    Con số lớn đúng với người dùng thật: những request lẽ ra được gửi trong 100 s máy chủ treo đều phải chờ. Vòng lặp đóng đợi phản hồi trước khi gửi tiếp nên không bao giờ tạo ra các request đó (coordinated omission); bù hoặc dùng vòng lặp mở (gửi theo lịch) mới ghi nhận chúng.[^hdrj-co]

    </details>

---

[^bc-keydir]: Bitcask, mục keydir — <https://riak.com/assets/bitcask-intro.pdf> — "A keydir is simply a hash table that maps every key in a Bitcask to a fixed-size structure giving the file, offset, and size of the most recently written entry for that key."
[^bc-read]: Bitcask, mục đọc — <https://riak.com/assets/bitcask-intro.pdf> — "Reading a value is simple, and doesn't ever require more than a single disk seek."
[^bc-tomb]: Bitcask, mục mô hình — <https://riak.com/assets/bitcask-intro.pdf> — "Note that deletion is simply a write of a special tombstone value, which will be removed on the next merge."
[^bc-merge]: Bitcask, mục merge — <https://riak.com/assets/bitcask-intro.pdf> — "This simple model may use up a lot of space over time, since we just write out new values without touching the old ones."
[^bc-merge2]: Bitcask, mục merge và hint file — <https://riak.com/assets/bitcask-intro.pdf> — "The merge process iterates over all non-active (i.e. immutable) files in a Bitcask and produces as output a set of data files containing only the “live” or latest versions of each present key."
[^bc-scan]: Bitcask, mục khởi động — <https://riak.com/assets/bitcask-intro.pdf> — "For any data file that has a hint file, that will be scanned instead for a much quicker startup time."
[^bc-sync]: Bitcask, mục API — <https://riak.com/assets/bitcask-intro.pdf> — "sync on put (if this writer would prefer to sync the write file after every write operation)"
[^std-readat]: Rust 1.95.0, std::os::unix::fs::FileExt — <https://doc.rust-lang.org/1.95.0/std/os/unix/fs/trait.FileExt.html> — "The current file cursor is not affected by this function."
[^std-syncdata]: Rust 1.95.0, std::fs::File::sync_data — <https://doc.rust-lang.org/1.95.0/std/fs/struct.File.html> — "Note that some platforms may simply implement this in terms of sync_all."
[^rfc3720-b4]: RFC 3720 (iSCSI), phụ lục B.4 CRC Examples — <https://www.rfc-editor.org/rfc/rfc3720.txt> — "N.B. all Values are Hexadecimal 32 bytes of zeroes"
[^rfc3720-digest]: RFC 3720 (iSCSI), mục về HeaderDigest/DataDigest — <https://www.rfc-editor.org/rfc/rfc3720.txt> — "Digests enable the checking of end-to-end, non-cryptographic data integrity beyond the integrity checks provided by the link layers"
[^ldb-crc]: LevelDB 1.23, doc/log_format.md — <https://raw.githubusercontent.com/google/leveldb/1.23/doc/log_format.md> — "checksum: uint32 // crc32c of type and data[] ; little-endian"
[^ldb-resync]: LevelDB 1.23, doc/log_format.md — <https://raw.githubusercontent.com/google/leveldb/1.23/doc/log_format.md> — "We do not need any heuristics for resyncing - just go to next block boundary and scan."
[^rdb-wal-tolerate]: RocksDB wiki, WAL Recovery Modes — <https://raw.githubusercontent.com/wiki/facebook/rocksdb/WAL-Recovery-Modes.md> — "This is a heuristic mode, the system cannot differentiate between corruption at the tail of the log and incomplete write."
[^rdb-wal-abs]: RocksDB wiki, WAL Recovery Modes — <https://raw.githubusercontent.com/wiki/facebook/rocksdb/WAL-Recovery-Modes.md> — "In this mode, any IO error during WAL replay is considered as data corruption."
[^rdb-wal-pit]: RocksDB wiki, WAL Recovery Modes — <https://raw.githubusercontent.com/wiki/facebook/rocksdb/WAL-Recovery-Modes.md> — "In this mode, the WAL replay is stopped after encountering an IO error. The system is recovered to a point-in-time where it is consistent."
[^mr-rename]: Dean & Ghemawat, MapReduce (OSDI'04), mục 3.3 — <https://static.googleusercontent.com/media/research.google.com/en//archive/mapreduce-osdi04.pdf> — "We rely on the atomic rename operation provided by the underlying file system"
[^ldb-1000]: LevelDB 1.23, doc/index.md — <https://raw.githubusercontent.com/google/leveldb/1.23/doc/index.md> — "Asynchronous writes are often more than a thousand times as fast as synchronous writes."
[^bt-minor]: Bigtable (Chang et al., OSDI 2006), mục 5.4 — <https://static.googleusercontent.com/media/research.google.com/en//archive/bigtable-osdi06.pdf> — "When the memtable size reaches a threshold, the memtable is frozen, a new memtable is created, and the frozen memtable is converted to an SSTable and written to GFS."
[^bt-merged]: Bigtable, mục 5.3 — <https://static.googleusercontent.com/media/research.google.com/en//archive/bigtable-osdi06.pdf> — "A valid read operation is executed on a merged view of the sequence of SSTables and the memtable."
[^ldb-sorted2]: LevelDB 1.23, doc/impl.md — <https://raw.githubusercontent.com/google/leveldb/1.23/doc/impl.md> — "(Deletion markers are kept around to hide obsolete values present in older sorted tables)."
[^ldb-drop]: LevelDB 1.23, doc/impl.md — <https://raw.githubusercontent.com/google/leveldb/1.23/doc/impl.md> — "Compactions drop overwritten values. They also drop deletion markers if there are no higher numbered levels that contain a file whose range overlaps the current key."
[^ldb-manifest]: LevelDB 1.23, doc/impl.md, mục Manifest — <https://raw.githubusercontent.com/google/leveldb/1.23/doc/impl.md> — "The MANIFEST file is formatted as a log, and changes made to the serving state (as files are added or removed) are appended to this log."
[^rdb-bloom-perfile]: RocksDB wiki, Bloom Filter — <https://raw.githubusercontent.com/wiki/facebook/rocksdb/RocksDB-Bloom-Filter.md> — "every newly created SST file will contain a Bloom filter, which is used to determine if the file may contain the key we're looking for."
[^rdb-bloom-how]: RocksDB wiki, Bloom Filter — <https://raw.githubusercontent.com/wiki/facebook/rocksdb/RocksDB-Bloom-Filter.md> — "the key definitely does not exist if at least one of the probes return 0."
[^rdb-bloom-bits]: RocksDB wiki, Bloom Filter — <https://raw.githubusercontent.com/wiki/facebook/rocksdb/RocksDB-Bloom-Filter.md> — "4.9 bits per key (10% false positive rate) is 90% as effective as 100 bits per key 9.9 bits per key (1% false positive rate) is 99% as effective as 100 bits per key 15.5 bits per key (0.1% false positive rate) is 99.9% as effective as 100 bits per key"
[^rdb-tune-range]: RocksDB wiki, Tuning Guide — <https://raw.githubusercontent.com/wiki/facebook/rocksdb/RocksDB-Tuning-Guide.md> — "Bloom filters are not useful for range scans, so the read amplification is `number_of_level0_files + number_of_non_empty_levels`."
[^rdb-tune-ra]: RocksDB wiki, Tuning Guide — <https://raw.githubusercontent.com/wiki/facebook/rocksdb/RocksDB-Tuning-Guide.md> — "Point lookups must consult all files in level 0 and at most one file from each other levels. However, bloom filters help by greatly reducing read amplification."
[^rdb-wa-def]: RocksDB wiki, Tuning Guide — <https://raw.githubusercontent.com/wiki/facebook/rocksdb/RocksDB-Tuning-Guide.md> — "Write amplification is the ratio of bytes written to storage versus bytes written to the database."
[^rdb-ra-def]: RocksDB wiki, Tuning Guide — <https://raw.githubusercontent.com/wiki/facebook/rocksdb/RocksDB-Tuning-Guide.md> — "Read amplification is the number of disk reads per query."
[^rdb-sa-def]: RocksDB wiki, Tuning Guide — <https://raw.githubusercontent.com/wiki/facebook/rocksdb/RocksDB-Tuning-Guide.md> — "Space amplification is the ratio of the size of database files on disk to data size."
[^rdb-comp-tier]: RocksDB wiki, Compaction — <https://raw.githubusercontent.com/wiki/facebook/rocksdb/Compaction.md> — "Tiered compaction minimizes write amplification at the cost of read and space amplification."
[^rdb-comp-lvl]: RocksDB wiki, Compaction — <https://raw.githubusercontent.com/wiki/facebook/rocksdb/Compaction.md> — "Classic Leveled compaction, introduced by LSM-tree paper by O'Neil et al, minimizes space amplification at the cost of read and write amplification."
[^rdb-comp-lvl-wa]: RocksDB wiki, Compaction — <https://raw.githubusercontent.com/wiki/facebook/rocksdb/Compaction.md> — "The per-level write amplification is equal to the fanout in the worst case"
[^rdb-comp-tier-wa]: RocksDB wiki, Compaction — <https://raw.githubusercontent.com/wiki/facebook/rocksdb/Compaction.md> — "The per-level write amplification is 1 which is much less than for leveled where it was fanout."
[^pg-wal-concept]: PostgreSQL 16, mục 30.3 — <https://www.postgresql.org/docs/16/wal-intro.html> — "changes to data files (where tables and indexes reside) must be written only after those changes have been logged, that is, after WAL records describing the changes have been flushed to permanent storage."
[^pg-wal-fewer]: PostgreSQL 16, mục 30.3 — <https://www.postgresql.org/docs/16/wal-intro.html> — "only the WAL file needs to be flushed to disk to guarantee that a transaction is committed, rather than every data file changed by the transaction. The WAL file is written sequentially, and so the cost of syncing the WAL is much less than the cost of flushing the data pages."
[^pg-fpw]: PostgreSQL 16, mục 19.5 `full_page_writes` — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "This is needed because a page write that is in process during an operating system crash might be only partially completed, leading to an on-disk page that contains a mix of old and new data."
[^cs-col]: C-Store (Stonebraker et al., VLDB 2005), mục 1 — <https://www.vldb.org/archives/website/2005/program/paper/thu/p553-stonebraker.pdf> — "With a column store architecture, a DBMS need only read the values of columns required for processing a given query, and can avoid bringing into memory irrelevant attributes."
[^cs-row]: C-Store, mục 1 — <https://www.vldb.org/archives/website/2005/program/paper/thu/p553-stonebraker.pdf> — "With this row store architecture, a single disk write suffices to push all of the fields of a single record out to disk."
[^ab-compress]: Abadi, Madden, Hachem, "Column-Stores vs. Row-Stores", SIGMOD 2008, mục 5.1 — <https://www.cs.umd.edu/~abadi/papers/abadi-sigmod08.pdf> — "if the data is sorted by one of the columns, that column will be super-compressible (for example, runs of the same value can be run-length encoded)."
[^ab-late2]: Abadi et al. 2008, mục 5.2 — <https://www.cs.umd.edu/~abadi/papers/abadi-sigmod08.pdf> — "at some point in most query plans, data from multiple columns must be combined together into"
[^pb-enc-150]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "First you drop the MSB from each byte, as this is just there to tell us whether we’ve reached the end of the number"
[^pb-enc-varint]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "They allow encoding unsigned 64-bit integers using anywhere between one and ten bytes, with small values using fewer bytes."
[^pb-enc-varint2]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "Each byte in the varint has a continuation bit that indicates if the byte that follows it is part of the varint."
[^pb-enc-neg]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "As a result, this means that all ten bytes must be used."
[^pb-enc-zigzag]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "Positive integers p are encoded as 2 * p (the even numbers), while negative integers n are encoded as 2 * |n| - 1 (the odd numbers)."
[^pb-enc-tag]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "The “tag” of a record is encoded as a varint formed from the field number and the wire type via the formula (field_number << 3) | wire_type."
[^pb-field-range]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "For example, field numbers in the range 1 through 15 take one byte to encode. Field numbers in the range 16 through 2047 take two bytes."
[^pb-enc-simple]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "If you were able to examine the encoded message, you’d see three bytes: 08 96 01"
[^pb-enc-len]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "we see that the tag, 12, is 00010 010, or 2:LEN. The byte that follows is the int32 varint 7, and the next seven bytes are the UTF-8 encoding of "testing"."
[^pb-enc-kv]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "The binary version of a message just uses the field’s number as the key – the name and declared type for each field can only be determined on the decoding end by referencing the message type’s definition"
[^pb-enc-skip]: Protobuf, Encoding — <https://protobuf.dev/programming-guides/encoding/> — "The wire type tells the parser how big the payload after it is. This allows old parsers to skip over new fields they don’t understand."
[^cam-rolling]: Cambridge, Distributed Systems lecture notes (2021/22), §2.4 Fault tolerance and high availability — <https://www.cl.cam.ac.uk/teaching/2122/ConcDisSys/dist-sys-notes.pdf> — "if a service can tolerate one out of three nodes being unavailable, then a software upgrade can be rolled out by installing it and restarting one node at a time, while the remaining two nodes continue running the service."
[^conf-backward]: Confluent Platform 7.9, Schema Evolution and Compatibility — <https://docs.confluent.io/platform/7.9/schema-registry/fundamentals/schema-evolution.html> — "BACKWARD compatibility means that consumers using the new schema can read data produced with the last schema."
[^conf-forward]: Confluent Platform 7.9, Schema Evolution and Compatibility — <https://docs.confluent.io/platform/7.9/schema-registry/fundamentals/schema-evolution.html> — "FORWARD compatibility means that data produced with a new schema can be read by consumers using the last schema, even though they may not be able to use the full capabilities of the new schema."
[^pb3-unknown-keep]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Proto3 messages preserve unknown fields and include them during parsing and in the serialized output, which matches proto2 behavior."
[^pb3-unknown-copy]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Use message-oriented APIs, such as CopyFrom() and MergeFrom(), to copy data rather than copying field-by-field"
[^pb3-lean]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "The Protobuf wire format is lean and doesn’t provide a way to detect fields encoded using one definition and decoded using another."
[^pb3-reuse]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "Encoding a field using one definition and then decoding that same field with a different definition can lead to: Developer time lost to debugging A parse/merge error (best case scenario) Leaked PII/SPII Data corruption"
[^pb3-reserve]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "you may delete the field definition from the message. However, you must reserve the deleted field number."
[^pb3-compat-int]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "changing an int32 to an int64 is a compatible change, but if a value larger than INT32_MAX is written, a client that reads it as an int32 will discard the high order bits of the number."
[^pb3-compat-sint]: Protobuf, Language Guide (proto 3) — <https://protobuf.dev/programming-guides/proto3/> — "sint32 and sint64 are compatible with each other but are not compatible with the other integer types."
[^avro-noname]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "Binary encoded Avro data does not include type information or field names. The benefit is that the serialized data is small, but as a result a schema must always be used in order to read Avro data correctly."
[^avro-resolve-names]: Apache Avro 1.12.0, Specification — <https://avro.apache.org/docs/1.12.0/specification/> — "the ordering of fields may be different: fields are matched by name."
[^sys-rowversion]: PostgreSQL 16, 5.5 System Columns — <https://www.postgresql.org/docs/16/ddl-system-columns.html> — "A row version is an individual state of a row; each update of a row creates a new row version for the same logical row."
[^sys-xmin]: PostgreSQL 16, 5.5 System Columns — <https://www.postgresql.org/docs/16/ddl-system-columns.html> — "The identity (transaction ID) of the inserting transaction for this row version."
[^sys-xmax]: PostgreSQL 16, 5.5 System Columns — <https://www.postgresql.org/docs/16/ddl-system-columns.html> — "The identity (transaction ID) of the deleting transaction, or zero for an undeleted row version."
[^sys-xmax-nonzero]: PostgreSQL 16, 5.5 System Columns — <https://www.postgresql.org/docs/16/ddl-system-columns.html> — "It is possible for this column to be nonzero in a visible row version. That usually indicates that the deleting transaction hasn't committed yet, or that an attempted deletion was rolled back."
[^snap-xmax]: PostgreSQL 16, 9.27 System Information Functions, Table 9.81 — <https://www.postgresql.org/docs/16/functions-info.html> — "One past the highest completed transaction ID. All transaction IDs greater than or equal to xmax had not yet completed as of the time of the snapshot, and thus are invisible."
[^snap-xip]: PostgreSQL 16, 9.27 System Information Functions, Table 9.81 — <https://www.postgresql.org/docs/16/functions-info.html> — "A transaction ID that is xmin <= X < xmax and not in this list was already completed at the time of the snapshot, and thus is either visible or dead according to its commit status."
[^snap-text]: PostgreSQL 16, 9.27 System Information Functions — <https://www.postgresql.org/docs/16/functions-info.html> — "pg_snapshot's textual representation is xmin:xmax:xip_list. For example 10:20:10,14,15 means xmin=10, xmax=20, xip_list=10, 14, 15."
[^rr-snapshot]: PostgreSQL 16, 13.2.2 Repeatable Read — <https://www.postgresql.org/docs/16/transaction-iso.html> — "The Repeatable Read isolation level only sees data committed before the transaction began; it never sees either uncommitted data or changes committed by concurrent transactions during the transaction's execution."
[^rr-si]: PostgreSQL 16, 13.2.2 Repeatable Read — <https://www.postgresql.org/docs/16/transaction-iso.html> — "The Repeatable Read isolation level is implemented using a technique known in academic database literature and in some other database products as Snapshot Isolation."
[^rr-concurrent-update]: PostgreSQL 16, 13.2.2 Repeatable Read — <https://www.postgresql.org/docs/16/transaction-iso.html> — "because a repeatable read transaction cannot modify or lock rows changed by other transactions after the repeatable read transaction began."
[^rr-retry]: PostgreSQL 16, 13.2.2 Repeatable Read — <https://www.postgresql.org/docs/16/transaction-iso.html> — "When an application receives this error message, it should abort the current transaction and retry the whole transaction from the beginning."
[^rc-reeval]: PostgreSQL 16, 13.2.1 Read Committed — <https://www.postgresql.org/docs/16/transaction-iso.html> — "The search condition of the command (the WHERE clause) is re-evaluated to see if the updated version of the row still matches the search condition."
[^rc-bank]: PostgreSQL 16, 13.2.1 Read Committed — <https://www.postgresql.org/docs/16/transaction-iso.html> — "If two such transactions concurrently try to change the balance of account 12345, we clearly want the second transaction to start with the updated version of the account's row."
[^ber-p4]: Berenson et al., A Critique of ANSI SQL Isolation Levels (SIGMOD 1995), mục 4.1 — <https://arxiv.org/pdf/cs/0701157> — "P4 (Lost Update): The lost update anomaly occurs when transaction T1 reads a data item and then T2 updates the data item (possibly based on a previous read), then T1 (based on its earlier read value) updates the data item and commits."
[^ber-fcw]: Berenson et al., SIGMOD 1995, mục 4.2 — <https://arxiv.org/pdf/cs/0701157> — "This feature, called Firstcommitter-wins prevents lost updates (phenomenon P4)."
[^ber-a5a]: Berenson et al., SIGMOD 1995, mục 4.3 — <https://arxiv.org/pdf/cs/0701157> — "A5A Read Skew Suppose transaction T1 reads x, and then a second transaction T2 updates x and y to new values and commits. If now T1 reads y, it may see an inconsistent state, and therefore produce an inconsistent state as output."
[^ber-a5b]: Berenson et al., SIGMOD 1995, mục 4.3 — <https://arxiv.org/pdf/cs/0701157> — "A5B Write Skew Suppose T1 reads x and y, which are consistent with C(), and then a T2 reads x and y, writes x, and commits. Then T1 writes y."
[^ber-jobs]: Berenson et al., SIGMOD 1995, mục 4.2 — <https://arxiv.org/pdf/cs/0701157> — "Since the two transactions are inserting different data items (and different index entries as well, if any), this scenario is not precluded by First-Committer-Wins and can occur in Snapshot Isolation."
[^ports-doctors]: Ports & Grittner, Serializable Snapshot Isolation in PostgreSQL (VLDB 2012), mục 2.1.1 — <https://vldb.org/pvldb/vol5/p1850_danrkports_vldb2012.pdf> — "Each checks whether there are at least two doctors on call, and if so takes one doctor off call."
[^ports-wl]: Ports & Grittner, VLDB 2012, mục 2.1.1 — <https://vldb.org/pvldb/vol5/p1850_danrkports_vldb2012.pdf> — "The write locks taken on update don't solve this problem, because the two transactions modify different rows and thus do not conflict."
[^ports-rw]: Ports & Grittner, VLDB 2012, mục 3.1 — <https://vldb.org/pvldb/vol5/p1850_danrkports_vldb2012.pdf> — "rw-antidependencies: if T1 writes a version of some object, and T2 reads the previous version of that object, then T1 appears to have executed after T2, because T2 did not see its update."
[^ports-predicate]: Ports & Grittner, VLDB 2012, mục 3.1 — <https://vldb.org/pvldb/vol5/p1850_danrkports_vldb2012.pdf> — "because dependencies can also be caused by predicate reads."
[^ports-dangerous]: Ports & Grittner, VLDB 2012, mục 3.3 — <https://vldb.org/pvldb/vol5/p1850_danrkports_vldb2012.pdf> — "If any transaction has both an incoming rw-antidependency and an outgoing one, SSI aborts one of the transactions involved."
[^ports-fp]: Ports & Grittner, VLDB 2012, mục 3.3 — <https://vldb.org/pvldb/vol5/p1850_danrkports_vldb2012.pdf> — "but it may have false positives because not every dangerous structure is part of a cycle."
[^ports-commitorder]: Ports & Grittner, VLDB 2012, mục 3.3 — <https://vldb.org/pvldb/vol5/p1850_danrkports_vldb2012.pdf> — "Cahill's thesis [6] suggests a commit ordering optimization that can reduce false positives."
[^ports-t3first]: Ports & Grittner, VLDB 2012, mục 3.1, Theorem 1 — <https://vldb.org/pvldb/vol5/p1850_danrkports_vldb2012.pdf> — "Furthermore, T3 must be the first transaction in the cycle to commit."
[^ser-example]: PostgreSQL 16, 13.2.3 Serializable — <https://www.postgresql.org/docs/16/transaction-iso.html> — "If either transaction were running at the Repeatable Read isolation level, both would be allowed to commit; but since there is no serial order of execution consistent with the result, using Serializable transactions will allow one transaction to commit and will roll the other back with this message:"
[^ser-retry]: PostgreSQL 16, 13.2.3 Serializable — <https://www.postgresql.org/docs/16/transaction-iso.html> — "In all other cases applications must not depend on results read during a transaction that later aborted; instead, they should retry the transaction until it succeeds."
[^iso-serial-def]: PostgreSQL 16, 13.2 Transaction Isolation — <https://www.postgresql.org/docs/16/transaction-iso.html> — "any concurrent execution of a set of Serializable transactions is guaranteed to produce the same effect as running them one at a time in some order."
[^forupdate-block]: PostgreSQL 16, 13.3.2 Row-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "other transactions that attempt UPDATE, DELETE, SELECT FOR UPDATE, SELECT FOR NO KEY UPDATE, SELECT FOR SHARE or SELECT FOR KEY SHARE of these rows will be blocked until the current transaction ends"
[^forupdate-rr-error]: PostgreSQL 16, 13.3.2 Row-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "Within a REPEATABLE READ or SERIALIZABLE transaction, however, an error will be thrown if a row to be locked has changed since the transaction started."
[^err-codes]: PostgreSQL 16, Appendix A PostgreSQL Error Codes — <https://www.postgresql.org/docs/16/errcodes-appendix.html> — "Class 40 — Transaction Rollback 40000 transaction_rollback 40002 transaction_integrity_constraint_violation 40001 serialization_failure 40003 statement_completion_unknown 40P01 deadlock_detected"
[^vac-mustnot]: PostgreSQL 16, 25.1 Routine Vacuuming — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "the row version must not be deleted while it is still potentially visible to other transactions."
[^vl-log]: PostgreSQL REL_16_4, src/backend/access/heap/vacuumlazy.c, log của VACUUM VERBOSE/autovacuum — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/vacuumlazy.c> — "tuples: %lld removed, %lld remain, %lld are dead but not yet removable"
[^iit]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.11 idle_in_transaction_session_timeout — <https://www.postgresql.org/docs/16/runtime-config-client.html> — "Even when no significant locks are held, an open transaction prevents vacuuming away recently-dead tuples that may be visible only to this transaction; so remaining idle for a long time can contribute to table bloat."
[^sort-merge]: GNU coreutils manual, sort invocation, --batch-size — <https://www.gnu.org/software/coreutils/manual/html_node/sort-invocation.html> — "When sort has to merge more than nmerge inputs, it merges them in groups of nmerge, saving the result in a temporary file, which is then used as an input in a subsequent merge."
[^mr-extsort]: Dean & Ghemawat, MapReduce (OSDI'04), mục 3.1 — <https://static.googleusercontent.com/media/research.google.com/en//archive/mapreduce-osdi04.pdf> — "If the amount of intermediate data is too large to fit in memory, an external sort is used."
[^mr-part]: Dean & Ghemawat, MapReduce (OSDI'04), mục 3.1 — <https://static.googleusercontent.com/media/research.google.com/en//archive/mapreduce-osdi04.pdf> — "Reduce invocations are distributed by partitioning the intermediate key space into R pieces using a partitioning function (e.g., hash(key) mod R)."
[^mr-combiner]: Dean & Ghemawat, MapReduce (OSDI'04), mục 4.3 — <https://static.googleusercontent.com/media/research.google.com/en//archive/mapreduce-osdi04.pdf> — "We allow the user to specify an optional Combiner function that does partial merging of this data before it is sent over the network."
[^mr-comb-the]: Dean & Ghemawat, MapReduce (OSDI'04), mục 4.3 — <https://static.googleusercontent.com/media/research.google.com/en//archive/mapreduce-osdi04.pdf> — "Since word frequencies tend to follow a Zipf distribution, each map task will produce hundreds or thousands of records of the form"
[^mr-comb-why]: Dean & Ghemawat, MapReduce (OSDI'04), mục 4.3 — <https://static.googleusercontent.com/media/research.google.com/en//archive/mapreduce-osdi04.pdf> — "Reduce function is commutative and associative. A good example of this is the word counting example in Section 2.1."
[^mr-outfiles]: Dean & Ghemawat, MapReduce (OSDI'04), mục 3.1 — <https://static.googleusercontent.com/media/research.google.com/en//archive/mapreduce-osdi04.pdf> — "Typically, users do not need to combine these R output files into one file – they often pass these files as input to another MapReduce call"
[^join-std]: Blanas et al., A Comparison of Join Algorithms for Log Processing in MapReduce (SIGMOD'10), mục 3.1 — <https://pages.cs.wisc.edu/~jignesh/publ/hadoopjoin.pdf> — "To identify which table an input record is from, each map task tags the record with its originating table, and outputs the extracted join key and the tagged record as a (key, value) pair. The outputs are then partitioned, sorted and merged by the framework."
[^join-buf]: Blanas et al., SIGMOD'10, mục 3.1 — <https://pages.cs.wisc.edu/~jignesh/publ/hadoopjoin.pdf> — "One potential problem with the standard repartition join is that all the records for a given join key from both L and R have to be buffered."
[^join-oom]: Blanas et al., SIGMOD'10, mục 4 — <https://pages.cs.wisc.edu/~jignesh/publ/hadoopjoin.pdf> — "In some extreme cases, the standard repartition join did not finish because it ran out of memory."
[^join-impr]: Blanas et al., SIGMOD'10, mục 3.1 — <https://pages.cs.wisc.edu/~jignesh/publ/hadoopjoin.pdf> — "Finally, as records from the smaller table R are guaranteed to be ahead of those from L for a given join key, only R records are buffered and L records are streamed to generate the join output."
[^join-skew]: Blanas et al., SIGMOD'10, mục 3.1 — <https://pages.cs.wisc.edu/~jignesh/publ/hadoopjoin.pdf> — "When the key cardinality is small or when the data is highly skewed, all the records for a given join key may not fit in memory."
[^join-bc-job]: Blanas et al., SIGMOD'10, mục 3.2 — <https://pages.cs.wisc.edu/~jignesh/publ/hadoopjoin.pdf> — "Broadcast join is run as a map-only job. On each node, all of R is retrieved from the DFS to avoid sending L over the network. Each map task uses a main-memory hash table to join a split of L with R."
[^sre-skew]: Google SRE Book, Data Processing Pipelines — <https://sre.google/sre-book/data-processing-pipelines/> — "Because the customer is the point of indivisibility, end-to-end runtime is thus capped to the runtime of the largest customer."
[^sre-fig41]: Beyer et al., SRE book, Service Level Objectives — <https://sre.google/sre-book/service-level-objectives/> — "although a typical request is served in about 50 ms, 5% of requests are 20 times slower!"
[^sre-dist]: Beyer et al., SRE book, Service Level Objectives — <https://sre.google/sre-book/service-level-objectives/> — "Most metrics are better thought of as distributions rather than averages."
[^sre-pct]: Beyer et al., SRE book, Service Level Objectives — <https://sre.google/sre-book/service-level-objectives/> — "a high-order percentile, such as the 99th or 99.9th, shows you a plausible worst-case value, while using the 50th percentile (also known as the median) emphasizes the typical case."
[^hdr-add]: hdrhistogram 7.5.4, Histogram::add — <https://docs.rs/hdrhistogram/7.5.4/hdrhistogram/struct.Histogram.html> — "Add the contents of another histogram to this one."
[^tail-100]: Dean và Barroso, The Tail at Scale, CACM 2013 — <https://www.barroso.org/publications/TheTailAtScale.pdf> — "If a user request must collect responses from 100 such servers in parallel, then 63% of user requests will take more than one second"
[^tail-hedge]: Dean và Barroso, The Tail at Scale — <https://www.barroso.org/publications/TheTailAtScale.pdf> — "This approach limits the additional load to approximately 5% while substantially shortening the latency tail."
[^hdrj-setup]: HdrHistogram (Java), README tag HdrHistogram-2.1.12, Corrected vs. Raw value recording calls — <https://raw.githubusercontent.com/HdrHistogram/HdrHistogram/HdrHistogram-2.1.12/README.md> — "imagine a system for which response times samples are taken once every 10 msec to characterize response time distribution."
[^hdrj-co]: HdrHistogram (Java), README tag HdrHistogram-2.1.12 — <https://raw.githubusercontent.com/HdrHistogram/HdrHistogram/HdrHistogram-2.1.12/README.md> — "Only ~50% of results will be at 1 msec or below, with the remaining 50% coming from the auto-generated value records covering the missing increments spread between 10msec and 100 sec."
[^hdr-rc]: hdrhistogram 7.5.4, Histogram::record_n_correct — <https://docs.rs/hdrhistogram/7.5.4/hdrhistogram/struct.Histogram.html> — "To compensate for the loss of sampled values when a recorded value is larger than the expected interval between value samples, this method will auto-generate and record an additional series of decreasingly-smaller (down to interval) value records."
[^docker-pw]: Docker Official Image postgres, README (docker-library/docs, commit 376d507) — <https://raw.githubusercontent.com/docker-library/docs/376d5078669d8c56397d4cefc0212b9bb44735cc/postgres/README.md> — "This environment variable is required for you to use the PostgreSQL image. It must not be empty or undefined."
[^docker-initdb]: Docker Official Image postgres, README (docker-library/docs, commit 376d507) — <https://raw.githubusercontent.com/docker-library/docs/376d5078669d8c56397d4cefc0212b9bb44735cc/postgres/README.md> — "The default `postgres` user and database are created in the entrypoint with `initdb`."
[^psql-onerror]: PostgreSQL 16, psql, biến ON_ERROR_STOP — <https://www.postgresql.org/docs/16/app-psql.html> — "By default, command processing continues after an error. When this variable is set to on, processing will instead stop immediately."
[^psql-stdin]: PostgreSQL 16, psql, tùy chọn -f — <https://www.postgresql.org/docs/16/app-psql.html> — "If filename is - (hyphen), then standard input is read until an EOF indication or \q meta-command."
[^ddl-ctid]: PostgreSQL 16, 5.5 System Columns — <https://www.postgresql.org/docs/16/ddl-system-columns.html> — "The physical location of the row version within its table. Note that although the ctid can be used to locate the row version very quickly, a row's ctid will change if it is updated or moved by VACUUM FULL."
[^pg-curxid]: PostgreSQL 16, 9.27 System Information Functions — <https://www.postgresql.org/docs/16/functions-info.html> — "Returns the current transaction's ID. It will assign a new one if the current transaction does not have one already"
[^pageinspect-hpi]: PostgreSQL 16, pageinspect, Heap Functions — <https://www.postgresql.org/docs/16/pageinspect.html> — "heap_page_items shows all line pointers on a heap page. For those line pointers that are in use, tuple headers as well as tuple raw data are also shown. All tuples are shown, whether or not the tuples were visible to an MVCC snapshot at the time the raw page was copied."
[^hu-xmax]: PostgreSQL REL_16_4, heapam.c, heap_update — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/heapam.c> — "and store info about transaction updating this tuple"
[^hu-ctid]: PostgreSQL REL_16_4, heapam.c, heap_update — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/heapam.c> — "record address of new tuple in t_ctid of old one"
[^av-enabled]: PostgreSQL 16, CREATE TABLE, storage parameters — <https://www.postgresql.org/docs/16/sql-createtable.html> — "Enables or disables the autovacuum daemon for a particular table."
[^hot-prune]: PostgreSQL 16, 73.7 Heap-Only Tuples (HOT) — <https://www.postgresql.org/docs/16/storage-hot.html> — "Old versions of updated rows can be completely removed during normal operation, including SELECTs, instead of requiring periodic vacuum operations."
[^pgstat-dead]: PostgreSQL 16, 28.2.18 pg_stat_all_tables, cột n_dead_tup — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "n_dead_tup bigint Estimated number of dead rows"
[^pgstat-cache]: PostgreSQL 16, 28.2 The Cumulative Statistics System — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "accessed values are cached until the end of its current transaction in the default configuration."
[^vac-space]: PostgreSQL 16, 25.1.2 Recovering Disk Space — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "The standard form of VACUUM removes dead row versions in tables and indexes and marks the space available for future reuse. However, it will not return the space to the operating system, except in the special case where one or more pages at the end of a table become entirely free and an exclusive table lock can be easily obtained."
[^vac-full]: PostgreSQL 16, 25.1.1 Vacuuming Basics — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "VACUUM FULL requires an ACCESS EXCLUSIVE lock on the table it is working on, and therefore cannot be done in parallel with other use of the table."
[^bxmin]: PostgreSQL 16, 28.2 pg_stat_activity, cột backend_xmin — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "The current backend's xmin horizon."
[^pglocks-xid]: PostgreSQL 16, 54.12 pg_locks — <https://www.postgresql.org/docs/16/view-pg-locks.html> — "If a permanent ID is assigned to the transaction (which normally happens only if the transaction changes the state of the database), it also holds an exclusive lock on the transaction's permanent transaction ID until it ends."
[^pglocks-row]: PostgreSQL 16, 54.12 pg_locks — <https://www.postgresql.org/docs/16/view-pg-locks.html> — "If a process is waiting for a row-level lock, it will usually appear in the view as waiting for the permanent transaction ID of the current holder of that row lock."
[^pglocks-granted]: PostgreSQL 16, 54.12 pg_locks — <https://www.postgresql.org/docs/16/view-pg-locks.html> — "granted is true in a row representing a lock held by the indicated process. False indicates that this process is currently waiting to acquire this lock"
[^we-xid]: PostgreSQL 16, 28.2 Lock Wait Events — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "transactionid Waiting for a transaction to finish."
[^lk-as]: PostgreSQL 16, 13.3.1 Table-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "ACCESS SHARE (AccessShareLock) Conflicts with the ACCESS EXCLUSIVE lock mode only. The SELECT command acquires a lock of this mode on referenced tables."
[^lk-ae]: PostgreSQL 16, 13.3.1 Table-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "This mode guarantees that the holder is the only transaction accessing the table in any way. Acquired by the DROP TABLE, TRUNCATE, REINDEX, CLUSTER, VACUUM FULL, and REFRESH MATERIALIZED VIEW (without CONCURRENTLY) commands. Many forms of ALTER INDEX and ALTER TABLE also acquire a lock at this level."
[^lock-rule1]: PostgreSQL REL_16_4, src/backend/storage/lmgr/README, Lock acquisition — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/lmgr/README> — "A lock request is granted immediately if it does not conflict with any existing or waiting lock request, or if the process already holds an instance of the same lock type"
[^lmgr-fifo]: PostgreSQL REL_16_4, src/backend/storage/lmgr/README — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/lmgr/README> — "Rule (b) ensures that conflicting requests are granted in order of arrival."
[^blockpids]: PostgreSQL 16, 9.26.1 Session Information Functions, pg_blocking_pids — <https://www.postgresql.org/docs/16/functions-info.html> — "One server process blocks another if it either holds a lock that conflicts with the blocked process's lock request (hard block), or is waiting for a lock that would conflict with the blocked process's lock request and is ahead of it in the wait queue (soft block)."
[^lt-def]: PostgreSQL 16, 20.11 lock_timeout — <https://www.postgresql.org/docs/16/runtime-config-client.html> — "Abort any statement that waits longer than the specified amount of time while attempting to acquire a lock on a table, index, row, or other database object. The time limit applies separately to each lock acquisition attempt."
[^dl-timeout-def]: PostgreSQL 16, 20.12 deadlock_timeout — <https://www.postgresql.org/docs/16/runtime-config-locks.html> — "The default is one second (1s), which is probably about the smallest value you would want in practice. On a heavily loaded server you might want to raise it."
[^lock-deadlock-retry]: PostgreSQL 16, 13.3.4 Deadlocks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "deadlocks can be handled on-the-fly by retrying transactions that abort due to deadlocks."
[^dl-order]: PostgreSQL 16, 13.3.4 Deadlocks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "The best defense against deadlocks is generally to avoid them by being certain that all applications using a database acquire locks on multiple objects in a consistent order."
[^par-off]: PostgreSQL 16, 20.4.6 Asynchronous Behavior, max_parallel_workers_per_gather — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "Setting this value to 0 disables parallel query execution"
[^indexscan-use]: PostgreSQL 16, 14.1.1 EXPLAIN Basics — <https://www.postgresql.org/docs/16/using-explain.html> — "You'll most often see this plan type for queries that fetch just a single row."
[^plan-seq]: PostgreSQL 16, 52.5.1 Generating Possible Plans — <https://www.postgresql.org/docs/16/planner-optimizer.html> — "There is always the possibility of performing a sequential scan on a relation, so a sequential scan plan is always created."
[^rows-removed]: PostgreSQL 16, 14.1.2 EXPLAIN ANALYZE — <https://www.postgresql.org/docs/16/using-explain.html> — "The “Rows Removed” line only appears when at least one scanned row, or potential join pair in the case of a join node, is rejected by the filter condition."
[^explain-buffers]: PostgreSQL 16, 14.1.2 EXPLAIN ANALYZE — <https://www.postgresql.org/docs/16/using-explain.html> — "The numbers provided by BUFFERS help to identify which parts of the query are the most I/O-intensive."
[^bitmap-why]: PostgreSQL 16, 14.1.1 EXPLAIN Basics — <https://www.postgresql.org/docs/16/using-explain.html> — "The reason for using two plan levels is that the upper plan node sorts the row locations identified by the index into physical order before reading them, to minimize the cost of separate fetches."
[^ext-indep]: PostgreSQL 16, 14.2.2 Extended Statistics — <https://www.postgresql.org/docs/16/planner-stats.html> — "The planner normally assumes that multiple conditions are independent of each other, an assumption that does not hold when column values are correlated."
[^ext-create]: PostgreSQL 16, 14.2.2 Extended Statistics — <https://www.postgresql.org/docs/16/planner-stats.html> — "extended statistics objects, more often called just statistics objects, can be created to instruct the server to obtain statistics across interesting sets of columns."
[^stats-approx]: PostgreSQL 16, 14.2.1 Single-Column Statistics — <https://www.postgresql.org/docs/16/planner-stats.html> — "Entries in pg_statistic are updated by the ANALYZE and VACUUM ANALYZE commands, and are always approximate even when freshly updated."
[^explain-analyze]: PostgreSQL 16, 14.1.2 EXPLAIN ANALYZE — <https://www.postgresql.org/docs/16/using-explain.html> — "With this option, EXPLAIN actually executes the query, and then displays the true row counts and true run time accumulated within each plan node, along with the same estimates that a plain EXPLAIN shows."
[^explain-sideeffect]: PostgreSQL 16, 14.1.2 EXPLAIN ANALYZE — <https://www.postgresql.org/docs/16/using-explain.html> — "If you want to analyze a data-modifying query without changing your tables, you can roll the command back afterwards"
[^vm-only]: PostgreSQL 16, 25.1.4 Updating the Visibility Map — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "An index-only scan, on the other hand, checks the visibility map first. If it's known that all tuples on the page are visible, the heap fetch can be skipped."
[^explain-heapfetch]: PostgreSQL REL_16_4, src/backend/commands/explain.c, show_plan_tt cho T_IndexOnlyScan — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/commands/explain.c> — "ExplainPropertyFloat("Heap Fetches", NULL, planstate->instrument->ntuples2, 0, es);"
[^actual-ms]: PostgreSQL 16, 14.1.2 EXPLAIN ANALYZE — <https://www.postgresql.org/docs/16/using-explain.html> — "The thing that's usually most important to look for is whether the estimated row counts are reasonably close to reality."
