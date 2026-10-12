# P12 · Chương 15 — PostgreSQL bên trong: process, bộ nhớ, VACUUM, lock, planner

> Chương này là **case study**: các khái niệm đã có chỗ khác (MVCC, isolation, WAL, B-tree, replication, deadlock lý thuyết, page cache) chỉ được tóm 1–3 câu kèm link; phần viết sâu là thứ chưa có ở đâu — PostgreSQL *thực sự chạy* thế nào khi bạn vận hành nó. Mọi khẳng định về hành vi cụ thể là về **PostgreSQL 16** trừ khi ghi khác. Nguồn: docs PostgreSQL 16 (trang sống, truy cập 2026-10-12 — nội dung có thể được sửa theo bản vá nhỏ), mã nguồn PostgreSQL ghim tag `REL_16_4` trên `raw.githubusercontent.com`, docs và changelog PgBouncer (trang sống, truy cập 2026-10-12; trang chủ lúc đó liệt kê PgBouncer 1.26.0 là bản mới nhất, nên chi tiết — nhất là mặc định `max_prepared_statements` — có thể khác ở bản cũ hơn), docs.rs ghim phiên bản (`sqlx` 0.9.0, `sqlx-core` 0.9.0, `tokio-postgres` 0.7.18, `deadpool-postgres` 0.14.2, `deadpool` 0.13.1) và một đoạn source `sqlx-postgres` ghim tag `v0.9.0`. Chương **không có số đo** nào do người soạn chạy; mọi câu SQL/Rust chỉ để minh họa, chưa chạy/biên dịch; hình trích từ docs được ghi rõ nguồn. Các con số mặc định (default) là của docs PostgreSQL 16 / PgBouncer tại thời điểm truy cập.

## Bạn sẽ hiểu được gì

- **Q1.** Một connection tới PostgreSQL thực chất là gì ở mức process: postmaster, backend, và các process nền (checkpointer, background writer, WAL writer, autovacuum) làm gì? → mục 1
- **Q2.** Vì sao connection "đắt", vì sao cần pooler, và PgBouncer ở chế độ session / transaction / statement mỗi cái cho phép và cấm gì (prepared statement, `SET`, advisory lock)? → mục 2
- **Q3.** `shared_buffers`, `work_mem`, `maintenance_work_mem`, `effective_cache_size` mỗi cái là bộ nhớ của ai, tính theo đơn vị nào, và *double buffering* với page cache của OS là gì? → mục 3
- **Q4.** Một câu `UPDATE` đi qua những bước nào từ backend tới WAL, checkpoint và data file (tóm lược, kèm chỗ chương khác đã nói)? → mục 4
- **Q5.** VACUUM và autovacuum làm gì, khi nào kích hoạt, vì sao một transaction mở lâu chặn được nó, và XID wraparound / freezing là gì? → mục 5
- **Q6.** Table lock, row lock, bảng xung đột, hàng đợi lock, deadlock detection và vì sao một `ALTER TABLE` có thể làm cả bảng "đứng"? → mục 6
- **Q7.** `EXPLAIN` / `EXPLAIN ANALYZE` đọc thế nào; cost, các kiểu scan, các kiểu join và statistics quyết định plan ra sao? → mục 7
- **Q8.** Nhìn vào một PostgreSQL đang chạy bằng gì (`pg_stat_activity`, `pg_stat_statements`, `pg_locks`, wait event), và những cấu hình nào hay gây sự cố? → mục 8 và mục 9

## Kiến thức cần trước

- [P1 · Chương 03 — Process](../01-linux/03-process.md): `fork` nhân bản một process; chương này dùng lại khi nói "mỗi connection là một process".
- [P3 · Chương 05 — Bộ nhớ của tiến trình trên Linux](../03-memory/05-bo-nho-tien-trinh-linux.md): RSS/VSZ, overcommit (mục 5, 6) — nền cho phần OOM killer và đo bộ nhớ của backend.
- [P4 · Chương 05 — Bug concurrency và deadlock](../04-concurrency/05-concurrency-bug-va-deadlock.md): lý thuyết deadlock (mục 3–5) và deadlock ở tầng database/pool (mục 7); chương này chỉ nói *cài đặt* phát hiện deadlock của PostgreSQL.
- [P5 · Chương 04 — Page cache, writeback và direct I/O](../05-storage/04-page-cache-va-direct-io.md): page cache của OS, `fsync`; nền cho *double buffering* ở mục 3.
- [P5 · Chương 05](../05-storage/05-crash-consistency-va-journaling.md) và [Chương 06](../05-storage/06-durability-trong-thuc-te.md): write-ahead logging và `fsync` đúng chỗ; mục 4 không lặp lại.
- [P12 · Chương 03 — Storage engine](03-storage-engine.md): B-tree, page, WAL, full-page write, HOT (mục 5).
- [P12 · Chương 05 — Replication](05-replication.md): replication slot, standby; chỉ nhắc khi chúng ảnh hưởng VACUUM/WAL.
- [P12 · Chương 07 — Transaction và isolation](07-transaction.md): MVCC, `xmin`/`xmax`, snapshot (mục 4), `SELECT ... FOR UPDATE` (mục 5), lỗi `40001`/`40P01` và vòng retry (mục 8). Chương này **không** giải thích lại các thứ đó.

## 1. Một connection là một process: postmaster, backend và các process nền (Q1)

> **Trả lời ngắn.** PostgreSQL dùng mô hình "một process cho mỗi client": mỗi connection ứng với đúng một *backend process* riêng[^pm-process]. Một process giám sát tên *postmaster* lắng nghe cổng TCP và tạo backend mới (bằng `fork`) mỗi khi có connection tới[^pm-spawn]. Ngoài các backend phục vụ client còn có các process nền — checkpointer, background writer, WAL writer, autovacuum launcher (chạy thường trực) và các autovacuum worker do launcher khởi động — mỗi cái lo một việc bảo trì[^backend-types][^av-launcher].

**Giải thích.**

Hình dung (nhận xét của người soạn): một nhà hàng có một lễ tân (postmaster). Mỗi khách bước vào được lễ tân giao cho một nhân viên phục vụ riêng (backend) đứng cạnh bàn cho tới khi khách ra về. Nhà bếp, kho và sổ sách là chung (shared memory). Phía sau còn có nhân viên chuyên trách không phục vụ khách nào: người dọn bàn định kỳ (autovacuum), người ghi sổ (WAL writer), người chốt sổ cuối giờ (checkpointer).

*Thành phần 1 — postmaster.* Docs mô tả: vì không biết trước sẽ có bao nhiêu connection, phải có một *supervisor process* (process giám sát) sinh backend mới mỗi lần có yêu cầu; supervisor này tên là postmaster, lắng nghe ở một cổng TCP/IP chỉ định[^pm-spawn]. Docs hướng dẫn nhập môn nói cùng ý theo cách khác: server "fork" một process mới cho mỗi connection; từ lúc đó client và process mới nói chuyện với nhau mà process gốc không can thiệp; process giám sát luôn chạy, còn client và server process đi kèm thì đến rồi đi[^tut-fork]. Comment đầu file `postmaster.c` (REL_16_4) nói rõ thứ tự: khi nhận được yêu cầu, postmaster `fork()` ngay; process con tự làm bước xác thực và chỉ trở thành backend nếu xác thực thành công[^pm-forkimm].

*Thành phần 2 — backend.* Sau `fork`, process con (trong `BackendStartup`) đóng các socket của postmaster, thu startup packet, rồi tạo một cấu trúc `PGPROC` riêng của nó trong shared memory trước khi chạy vòng lặp phục vụ query[^pm-bestart]. Backend nhận query dạng text, parse, lập plan, thực thi rồi trả hàng về client trên chính connection đó[^pm-query]. Các backend phối hợp với nhau và với các process khác của instance qua semaphore và shared memory để giữ dữ liệu nhất quán khi truy cập đồng thời[^pm-ipc]. Mỗi backend là một process OS bình thường: PID của nó chính là cột `pid` trong `pg_stat_activity`, và có thể thấy bằng `ps`[^pid-ps]. Cơ chế `fork` (nhân bản bảng trang theo kiểu copy-on-write, thừa hưởng fd) là kiến thức của [P1 · Chương 03](../01-linux/03-process.md).

*Thành phần 3 — các process nền.* Danh sách `backend_type` của `pg_stat_activity` liệt kê: autovacuum launcher, autovacuum worker, logical replication launcher/worker, parallel worker, background writer, client backend, checkpointer, archiver, standalone backend, startup, walreceiver, walsender, walwriter[^backend-types]. Trong mã `REL_16_4`, postmaster khởi động một số process phụ trợ — background writer, checkpointer, WAL writer — bằng cùng hàm tạo process con `StartChildProcess`[^pm-aux]; đây là chi tiết cài đặt có thể đổi giữa các bản, và không phải loại nào trong danh sách trên cũng đi đường này (client backend được tạo ở `BackendStartup`, như trên). Vai trò (theo docs):

| Process | Làm gì | Nguồn |
|---|---|---|
| checkpointer | tự động chạy checkpoint theo chu kỳ (`checkpoint_timeout`) hoặc khi WAL sắp vượt `max_wal_size` | [^ckpt-proc][^ckpt-trigger] |
| background writer | chủ động ghi bớt các shared buffer "dirty" ra file system để backend khỏi phải tự ghi | [^bgw] |
| WAL writer | định kỳ flush WAL theo `wal_writer_delay` | [^walw] |
| autovacuum launcher / worker | launcher khởi động các worker; worker chạy VACUUM/ANALYZE | [^av-launcher] |
| startup, walreceiver, walsender, archiver | có trong danh sách `backend_type`; theo tên, liên quan tới khôi phục và replication/lưu trữ WAL (suy luận của người soạn từ tên; xem [P12 · Chương 05](05-replication.md)) | [^backend-types] |

Phần checkpoint/WAL ở mục 4, autovacuum ở mục 5.

```mermaid
flowchart TB
  C1["client A"] --> PM
  C2["client B"] --> PM
  PM["postmaster<br/>lắng nghe cổng TCP, fork"]
  PM -->|"fork + xác thực"| B1["backend A"]
  PM -->|"fork + xác thực"| B2["backend B"]
  PM --> CK["checkpointer"]
  PM --> BG["background writer"]
  PM --> WW["WAL writer"]
  PM --> AL["autovacuum launcher"]
  AL --> AW["autovacuum worker"]
  SHM[("shared memory: shared buffers, WAL buffers, lock table, mảng PGPROC…")]
  B1 --- SHM
  B2 --- SHM
  CK --- SHM
  BG --- SHM
  WW --- SHM
  AW --- SHM
```

*Sơ đồ: minh họa của người soạn, tổng hợp từ các nguồn trên; không liệt kê hết process.*

*Postmaster tránh đụng vào shared memory.* Comment của `postmaster.c`: postmaster tạo pool shared memory và semaphore lúc khởi động nhưng không tự dùng chúng; nó không nằm trong mảng `PGPROC` nên không tham gia lock manager; lý do là giữ postmaster xa shared memory làm nó đơn giản và đáng tin hơn, nên thường hồi phục được sau khi một backend crash bằng cách reset shared memory[^pm-noshm].

*Hệ quả vận hành.* (1) Khi một child process crash, `HandleChildCrash` ghi log `terminating any other active server processes` rồi postmaster dừng các process còn lại và khởi tạo lại; log tương ứng là `all server processes terminated; reinitializing`[^pm-crash]. Nghĩa là một backend bị kill (ví dụ do OOM killer) có thể kéo theo *toàn bộ connection* của instance bị ngắt trong lúc server chạy crash recovery (suy luận của người soạn từ hai thông điệp log trên, không từ lời của docs). (2) Docs còn cảnh báo kernel Linux có thể kill chính postmaster khi hết bộ nhớ ảo; khi đó các connection hiện có vẫn chạy bình thường nhưng không nhận connection mới[^oom-pm][^oom-conn]. Nền tảng OOM killer: [P3 · Chương 04, mục 5](../03-memory/04-page-fault-va-swapping.md).

<details><summary>Đi sâu: <code>fork</code> vs <code>fork</code>+<code>exec</code>, worker song song, và vì sao autovacuum worker không tính vào <code>max_connections</code></summary>

- Trong `BackendStartup`, nhánh chuẩn gọi `fork_process()` rồi chạy tiếp trong process con; nhánh `EXEC_BACKEND` (dành cho nền tảng không có `fork`, như Windows) dùng `backend_forkexec`[^pm-bestart]. Ở Linux, backend là con `fork` của postmaster, không `exec` lại (suy luận của người soạn từ việc nhánh `!EXEC_BACKEND` không gọi exec).
- *Parallel query* sinh thêm process: docs nói mỗi parallel worker là một process hoàn toàn riêng và có tác động lên hệ thống gần như một user session thêm vào[^par-sep]. Vì vậy một truy vấn song song 4 worker có thể dùng tới 5 lần CPU/bộ nhớ/I/O của truy vấn không song song[^par-five].
- Autovacuum worker không bị tính vào giới hạn `max_connections` hay `superuser_reserved_connections`[^av-nocount]; semaphore thì được cấp theo tổng `max_connections` + `autovacuum_max_workers` + `max_wal_senders` + `max_worker_processes` (với System V semaphore)[^kr-sem].
- Chuyện gì xảy ra khi số connection chạm `max_connections` được bàn ở mục 2 và mục 9.

</details>

**Quay lại Q1:** một connection = một process OS riêng do postmaster `fork`; cạnh các backend còn có checkpointer, background writer, WAL writer, autovacuum, tất cả chia sẻ shared memory.

## 2. Vì sao connection đắt, và PgBouncer: session, transaction, statement (Q2)

> **Trả lời ngắn.** Mỗi connection giữ một process OS, một chỗ trong các bảng dùng chung của server (kích cỡ theo `max_connections`) và bộ nhớ riêng của session[^kr-sem][^work-mem]; khi chính PostgreSQL làm hệ thống hết bộ nhớ, docs khuyên trong nhiều trường hợp nên giảm `max_connections` và dùng một *connection pooler* bên ngoài[^kr-pool]. PgBouncer là pooler như vậy: nhiều client nối vào PgBouncer, còn số connection thật tới PostgreSQL thì ít và được dùng lại. Nó có ba chế độ — session, transaction, statement — và chế độ càng "hung hăng" thì càng ép được nhiều client lên ít connection thật, nhưng càng phá nhiều tính năng gắn với session như `SET`, `LISTEN`, `PREPARE` và advisory lock mức session[^pgb-modes][^pgb-table][^pgb-table-prep].

**Giải thích.**

Hình dung (nhận xét của người soạn): quầy gửi xe có 100 chỗ (`max_connections`). Nếu mỗi khách giữ một chỗ cả ngày dù chỉ ghé vào 2 phút, chỗ hết sớm trong khi quầy gần như trống. Pooler là người trông xe: chỉ cấp chỗ khi khách thật sự đang có việc ở trong, xong việc là thu chỗ lại cho người khác.

**2a. Vì sao connection "đắt".** Phân tách theo từng nguồn, tránh gộp thành một câu mơ hồ:

1. *Một process mỗi connection.* Mô hình ở mục 1: mỗi connection là một process do postmaster `fork`, và process con tự lo xác thực[^pm-forkimm]. Mỗi lần mở connection phải `fork` một process, đọc startup packet, tạo `PGPROC` trong shared memory[^pm-bestart] và xác thực; chuỗi việc đó tốn thời gian, nên mở connection mới cho từng request là ý tồi (nhận xét của người soạn — docs không đưa con số).
2. *Chỗ trong bảng dùng chung theo `max_connections`.* `max_connections` quyết định số connection đồng thời tối đa[^maxconn]; semaphore (với System V semaphore) được cấp theo tổng connection, autovacuum worker, WAL sender và background process[^kr-sem]. Nghĩa là con số này là *cấu hình lúc khởi động* — đổi phải restart[^maxconn].
3. *Bộ nhớ riêng của session.* `work_mem` áp dụng cho từng thao tác sort/hash, và nhiều session có thể làm việc đó cùng lúc, nên tổng bộ nhớ có thể là nhiều lần `work_mem`[^work-mem] (chi tiết mục 3). Mỗi session còn có `temp_buffers` riêng cho bảng tạm, cấp phát dần tới giới hạn[^temp-buf].
4. *Process song song.* Mỗi parallel worker có tác động như thêm một user session[^par-sep].
5. *Hậu quả khi hết bộ nhớ.* Với trường hợp PostgreSQL tự làm hệ thống hết bộ nhớ, docs gợi ý hạ `shared_buffers`, `work_mem`, `hash_mem_multiplier`, hoặc — vì nguyên nhân có thể là cho phép quá nhiều connection — giảm `max_connections` và dùng connection pooler ngoài (docs dùng cách nói "trong nhiều trường hợp")[^kr-pool].

Một điểm tinh tế (suy luận của người soạn từ hai nguồn): docs về `idle_session_timeout` nói một session *idle không nằm trong transaction* không gây chi phí lớn cho server[^idle-cheap]. Tức là cái đắt không phải "có nhiều connection mở" mà là "nhiều connection *đang chạy* cùng lúc tranh CPU, I/O, bộ nhớ", và "connection treo trong transaction" (giữ snapshot, giữ lock — mục 5 và 6). Pooler giúp vì nó cho phép *giới hạn số truy vấn thực sự chạy đồng thời* ở một con số do bạn chọn, dù có hàng nghìn client.

**2b. PgBouncer đứng ở đâu.** PgBouncer nằm giữa ứng dụng và PostgreSQL, nói giao thức PostgreSQL ở cả hai phía. Docs của nó nêu đặc điểm nhẹ: mặc định 2 kB bộ nhớ mỗi connection, vì không cần nhìn thấy trọn gói tin một lần[^pgb-2kb]. Số connection client tối đa là `max_client_conn` (mặc định 100)[^pgb-maxclient]; số connection server tối đa mỗi cặp user/database mặc định là `default_pool_size` = 20, có thể bị ghi đè bằng `pool_size` theo từng database hoặc user[^pgb-poolsize].

```mermaid
flowchart LR
  subgraph APP["Ứng dụng: nhiều pod, mỗi pod có pool riêng"]
    A1["pod 1: pool"]
    A2["pod 2: pool"]
    A3["pod N: pool"]
  end
  A1 --> PB
  A2 --> PB
  A3 --> PB
  PB["PgBouncer<br/>max_client_conn (client)<br/>default_pool_size (server)"]
  PB --> S1["backend 1"]
  PB --> S2["backend 2"]
  PB --> S3["backend K"]
  S1 --> PG[("PostgreSQL")]
  S2 --> PG
  S3 --> PG
```

*Sơ đồ: minh họa của người soạn; số lượng tượng trưng.*

**2c. Ba chế độ** (`pool_mode`, mặc định là `session`[^pgb-mode-default]):

- **Session pooling**: khi client kết nối, một server connection được gán cho nó trong *suốt thời gian* nó còn kết nối; client ngắt thì server connection mới trả lại pool. Chế độ này hỗ trợ mọi tính năng của PostgreSQL[^pgb-session].
- **Transaction pooling**: server connection chỉ gán cho client *trong một transaction*; khi PgBouncer thấy transaction kết thúc, server connection quay về pool. Docs nói chế độ này "phá" vài tính năng dựa vào session và chỉ dùng được khi ứng dụng hợp tác, không dùng các tính năng hỏng[^pgb-trans][^pgb-compat].
- **Statement pooling**: giống transaction pooling nhưng *cấm transaction nhiều câu lệnh*, nhằm ép chế độ "autocommit" ở phía client[^pgb-stmt].

**2d. Bảng tương thích (theo docs PgBouncer).** Với transaction pooling, các mục sau được docs ghi là "Never" (không bao giờ dùng được): `SET`/`RESET`, `LISTEN`, `WITH HOLD CURSOR`, `PREPARE`/`DEALLOCATE` (mức SQL), temp table kiểu `PRESERVE`/`DELETE ROWS`, `LOAD`, và advisory lock mức session; còn `NOTIFY`, cursor `WITHOUT HOLD`, temp table `ON COMMIT DROP`, reset cached plan thì dùng được[^pgb-table][^pgb-table-prep]. Vì sao mỗi cái hỏng (suy luận của người soạn, dựa trên việc mỗi transaction có thể rơi vào một server connection khác — điều chính docs PgBouncer cũng nêu khi giải thích vì sao `server_reset_query` không dùng ở chế độ này[^pgb-reset]):

| Tính năng | Vì sao hỏng ở transaction pooling | Cách đi đường khác |
|---|---|---|
| `SET x = …` (mức session) | giá trị gắn vào *session của server connection*, transaction kế tiếp của bạn có thể chạy ở connection khác[^set-session] | `SET LOCAL` — hiệu lực chỉ đến hết transaction hiện tại[^set-local]; hoặc `ALTER ROLE … SET …` đặt giá trị mặc định cho session mới của role[^alter-role] |
| `LISTEN` | đăng ký nghe nằm ở session server; connection khác sẽ không nhận thông báo (suy luận) | dùng session pooling riêng cho consumer `LISTEN`, hoặc connection trực tiếp |
| `PREPARE` (SQL) / prepared statement tên | prepared statement chỉ sống trong session tạo ra nó[^prep-session][^prep-proto]; chuyển sang connection khác là mất | `PREPARE` mức SQL: không có lối thoát trong PgBouncer, lệnh chuyển thẳng tới Postgres[^pgb-maxprep-sql]; prepared statement *mức giao thức*: dựa vào `max_prepared_statements` khác 0 (xem đoạn *Về prepared statement* dưới đây), hoặc không dùng prepared statement có tên |
| advisory lock mức session | lock giữ đến khi nhả tường minh hoặc session kết thúc, *không theo ngữ nghĩa transaction*[^adv-session] — lock có thể mắc lại trên một connection mà bạn không còn dùng | `pg_advisory_xact_lock`: nhả tự động cuối transaction[^adv-xact] |
| temp table `PRESERVE ROWS` | bảng tạm gắn với session | `ON COMMIT DROP` hoặc bảng thường |

*Về prepared statement.* Theo giao thức PostgreSQL, một prepared statement có tên tồn tại đến hết session hiện tại trừ khi bị hủy tường minh, còn prepared statement *không tên* chỉ sống đến lần `Parse` không tên kế tiếp[^prep-proto][^prep-unnamed]. PgBouncer cung cấp lối thoát: nếu `max_prepared_statements` khác 0, nó theo dõi các lệnh prepared statement *có tên ở mức giao thức* trong chế độ transaction/statement, và bảo đảm statement mà client đã prepare luôn có mặt trên server connection đang phục vụ nó — kể cả khi statement được prepare ở một connection khác[^pgb-maxprep]. Cơ chế theo docs: PgBouncer đặt cho mỗi chuỗi query một tên nội bộ dạng `PGBOUNCER_<số>`, viết lại tên do client đặt thành tên này trước khi chuyển lệnh đi, và tự prepare statement trên server connection được gán nếu chỗ đó chưa có[^pgb-maxprep-how][^pgb-maxprep-auto]. Giới hạn: cơ chế theo dõi và viết lại tên *không* áp dụng cho `PREPARE`/`EXECUTE`/`DEALLOCATE` mức SQL — các lệnh này chuyển thẳng tới Postgres[^pgb-maxprep-sql]; riêng `DEALLOCATE ALL` và `DISCARD ALL` vẫn chạy đúng như kỳ vọng và xóa các statement PgBouncer đang theo dõi cho client gửi lệnh[^pgb-maxprep-all].

*Giá trị mặc định của `max_prepared_statements` phụ thuộc phiên bản PgBouncer.* Docs Configuration (trang sống; trang chủ lúc truy cập liệt kê PgBouncer 1.26.0) ghi rằng đặt 0 thì hỗ trợ prepared statement cho transaction/statement pooling bị tắt[^pgb-maxprep-zero]. Changelog: bản 1.21.0 thêm tính năng này và yêu cầu đặt giá trị khác 0 để dùng[^pgb-prep-1-21]; bản 1.24.0 đổi mặc định thành 200[^pgb-prep-default]. Suy ra (suy luận của người soạn từ hai dòng changelog, chưa kiểm từng bản): từ 1.24.0 tính năng bật sẵn; với 1.21–1.23 phải tự đặt giá trị khác 0 — nên kiểm tra cấu hình thực của bản PgBouncer bạn đang chạy thay vì giả định. Bảng tương thích của docs Features ghi mục *Protocol-level prepared plans* chạy được ở transaction pooling kèm chú thích phải đặt `max_prepared_statements` khác 0[^pgb-table-prep] (chú thích này nhất quán với giá trị 0 là tắt).

*Hệ quả khi migration DDL.* Vì mọi client dùng chung một tên nội bộ cho cùng chuỗi query, nếu kiểu trả về hoặc kiểu tham số của statement đổi giữa các lần chạy thì PostgreSQL có thể báo lỗi `cached plan must not change result type`[^pgb-maxprep-err]; docs PgBouncer nêu tình huống hay gặp nhất là migration thêm cột hoặc đổi kiểu cột, và gợi ý chạy `RECONNECT` trên admin console sau migration để ép prepare lại[^pgb-maxprep-ddl] (liên hệ migration ở mục 6d).

*Về tham số khởi động.* Chỉ một số tham số trong startup packet được PgBouncer theo dõi; tham số khác gây lỗi trừ khi liệt kê trong `ignore_startup_parameters`[^pgb-ignore]. Ví dụ `options` (nơi client có thể gửi `-c statement_timeout=…`) cũng là một *startup parameter* kiểu này; docs PgBouncer nói giao thức cho phép đặt tham số cả trực tiếp trong startup packet lẫn bên trong `options`, và cả hai cách đều được `ignore_startup_parameters` hỗ trợ[^pgb-ignore-ex]. Ý nghĩa với Rust: xem mục "Liên hệ với Rust".

*Hàng đợi khi pool cạn.* Khi tất cả server connection đang bận, truy vấn phải chờ: `query_wait_timeout` (mặc định 120 giây) là thời gian tối đa truy vấn được chờ được gán server; hết hạn mà chưa được gán thì client bị ngắt, còn nếu đặt 0 thì client xếp hàng vô hạn[^pgb-qwait]. Nghĩa là *latency của bạn bây giờ có thêm một thành phần: thời gian chờ pool* (nhận xét của người soạn) — và đó là thứ cần giám sát ở cả pool ứng dụng lẫn PgBouncer.

<details><summary>Đi sâu: tính ngân sách connection, fd của PgBouncer, và reset query</summary>

- *Ngân sách connection phía PostgreSQL* (suy luận của người soạn, số liệu tự đặt): giả sử `max_connections = 100` (mặc định của docs[^maxconn]) và `superuser_reserved_connections = 3` (mặc định[^super-res]). Còn lại tối đa 97 chỗ cho mọi nguồn: tất cả PgBouncer, các tool admin/monitoring, job migration kết nối trực tiếp. Nếu có 20 pod ứng dụng, mỗi pod một pool tối đa 10 connection, nối *thẳng* PostgreSQL thì cần 200 connection — vượt hạn mức ngay cả khi mỗi pod chỉ dùng một phần. Nối qua PgBouncer thì số connection thật mỗi PgBouncer bị chặn bởi `pool_size` của từng cặp (user, database) — mặc định `default_pool_size`[^pgb-poolsize] — cộng `reserve_pool_size` nếu bật (mặc định tắt)[^pgb-reserve], không phụ thuộc số pod.
- *File descriptor.* Docs PgBouncer: tăng `max_client_conn` có thể cần tăng giới hạn fd của OS; số fd có thể dùng lớn hơn `max_client_conn`, lý thuyết tối đa là `max_client_conn + (max pool_size × số database × số user)` khi mỗi user kết nối bằng tên riêng[^pgb-fd]. Cách xem và nâng giới hạn fd (`RLIMIT_NOFILE`): [P1 · Chương 06, mục 6](../01-linux/06-user-quyen-proc-sys.md).
- *`server_reset_query`.* Ở session pooling, khi server connection được trả lại, PgBouncer gửi một câu để dọn trạng thái session cho client sau; mặc định là `DISCARD ALL`, có thể nhẹ hơn như `DEALLOCATE ALL`[^pgb-reset-q]; ở transaction pooling thì câu này không dùng[^pgb-reset]. `DISCARD ALL` được docs PostgreSQL mô tả là giải phóng mọi tài nguyên tạm của session và đưa session về trạng thái ban đầu[^discard].
- *Kết hợp hai tầng pool.* Pool trong ứng dụng (sqlx, deadpool…) và PgBouncer có thể cùng tồn tại; pool ứng dụng tiết kiệm chi phí mở connection tới PgBouncer, PgBouncer gom lại thành ít connection thật. Chi tiết cấu hình phía Rust ở mục "Liên hệ với Rust".

</details>

**Quay lại Q2:** connection đắt vì mỗi cái là một process + chỗ trong bảng dùng chung + bộ nhớ riêng, và vì chỉ ít truy vấn chạy đồng thời mới có ích; PgBouncer ép nhiều client lên ít connection thật, đổi lại ở transaction/statement mode bạn mất mọi thứ gắn với session.

## 3. Bộ nhớ: `shared_buffers`, `work_mem`, `maintenance_work_mem`, `effective_cache_size` (Q3)

> **Trả lời ngắn.** `shared_buffers` là một vùng bộ nhớ *dùng chung* cho mọi backend, mặc định 128MB[^sb-def], và chỉ đổi được khi khởi động lại server[^sb-restart]. `work_mem` là hạn mức *cho mỗi thao tác sort/hash trong một truy vấn*, không phải cho mỗi connection hay mỗi truy vấn, nên tổng có thể gấp nhiều lần[^wm-multi][^work-mem]. `maintenance_work_mem` dành cho VACUUM, `CREATE INDEX`… [^mwm]; còn `effective_cache_size` *không cấp phát gì cả* — chỉ là con số giả định để planner ước lượng chi phí[^ecs][^ecs-noalloc]. Vì PostgreSQL đọc file qua page cache của OS, một page có thể nằm ở cả hai nơi (*double buffering*)[^ecs-both].

**Giải thích.**

Hình dung (nhận xét của người soạn): thư viện có một *kệ sách chung* ở sảnh (shared buffers) mà ai cũng lấy được, và mỗi độc giả có *bàn làm việc riêng* (work_mem) để trải sách ra sắp xếp. Số bàn làm việc tăng theo số độc giả *và* số việc mỗi người làm cùng lúc, còn kệ sách chung thì cố định kích thước. `effective_cache_size` giống tờ ghi chú "chắc kho phụ của thành phố cũng giữ sẵn khoảng ngần này cuốn" mà người lập kế hoạch dùng để ước lượng — chẳng có cuốn nào được mang về.

**3a. Đường đọc một page.** (Phần này theo README của buffer manager trong `REL_16_4` — chi tiết cài đặt, có thể đổi giữa các bản.) Muốn chạm vào một page của bảng/index, backend phải "ghim" (pin) buffer chứa nó — buffer chưa pin có thể bị tái sử dụng cho page khác bất cứ lúc nào[^bufpin]. Nếu page chưa có trong shared buffers, backend phải chọn một buffer nạn nhân để tái sử dụng bằng thuật toán *clock sweep*: mỗi buffer có bộ đếm sử dụng, tăng (tới một ngưỡng nhỏ) mỗi lần bị pin[^clock]; "kim đồng hồ" quét vòng tròn: buffer đang bị pin hoặc có bộ đếm khác 0 thì không dùng được, bộ đếm của nó bị giảm một (nếu khác 0) và kim chuyển sang buffer kế, cho tới khi gặp buffer chưa pin có bộ đếm 0[^clock-sweep]. Nếu buffer được chọn đang dirty thì phải ghi nó ra trước khi dùng lại[^dirty-first]. Background writer tồn tại để giảm khả năng backend phải tự đi ghi buffer dirty[^bgw-reduce]. Đọc dữ liệu từ file đi qua page cache của kernel ([P5 · Chương 04, mục 1](../05-storage/04-page-cache-va-direct-io.md)); PostgreSQL mặc định không dùng direct I/O ([mục 7 chương đó](../05-storage/04-page-cache-va-direct-io.md)).

*Double buffering.* Hệ quả: dữ liệu nóng có thể có hai bản — một ở shared buffers, một ở page cache. Docs tự nhận điều này khi hướng dẫn đặt `effective_cache_size`: phải tính cả shared buffers lẫn phần disk cache của kernel dùng cho file dữ liệu PostgreSQL, "dù một số dữ liệu có thể tồn tại ở cả hai nơi"[^ecs-both]. Và đó là lý do docs chỉ gợi ý bắt đầu `shared_buffers` ở khoảng 25% RAM trên máy chuyên dụng từ 1GB RAM, và nói rất khó có chuyện đặt trên 40% RAM chạy tốt hơn vì PostgreSQL *cũng dựa vào cache của OS*[^sb-25]. Chỉ số `blks_hit` trong `pg_stat_database` chỉ đếm hit ở cache của PostgreSQL, không tính cache file system của OS[^blks-hit] — nên *hit ratio* 99% của PostgreSQL không cho biết `read()` phần còn lại có chạm đĩa hay chỉ chạm page cache (suy luận của người soạn từ định nghĩa trên).

**3b. `work_mem`: mỗi thao tác một hạn mức.** `work_mem` là mức tối đa cho *một thao tác của truy vấn* (như một sort hay một hash table) trước khi dữ liệu tràn sang file tạm; mặc định 4MB[^wm-default]. Một truy vấn phức tạp có thể chạy nhiều sort/hash cùng lúc, mỗi cái được dùng tới mức này, và nhiều session có thể cùng làm vậy[^wm-multi]. Sort dùng cho `ORDER BY`, `DISTINCT`, merge join[^wm-sorts]. Với hash, giới hạn là `work_mem × hash_mem_multiplier` (mặc định 2.0)[^hash-mem][^hash-mem-default]; docs nói thao tác hash thường nhạy với bộ nhớ hơn sort[^hash-sens]. Với parallel query, `work_mem` áp dụng riêng cho từng worker[^par-wm].

Công thức *cận trên lý thuyết* (suy luận của người soạn, số liệu tự đặt): nếu có 100 session đồng thời, mỗi session chạy một truy vấn có 3 thao tác sort/hash, mỗi cái dùng đủ `work_mem` = 4MB, thì tổng bộ nhớ cho các thao tác đó là 100 × 3 × 4MB = 1200MB; tăng `work_mem` lên 64MB cho "một báo cáo chậm" ở mức toàn server thì cận trên thành 100 × 3 × 64MB = 19200MB. Đây là lý do `work_mem` nên đặt thấp ở mức server và nâng cục bộ cho vai trò/truy vấn cần (xem `SET LOCAL` ở mục 2); và là lý do pooler (giảm số session *thực sự chạy*) giảm cận trên này.

Khi vượt hạn mức, thao tác tràn ra đĩa. Cách *nhìn thấy* điều đó: `EXPLAIN ANALYZE` hiển thị cách sort (in-memory hay on-disk) và dung lượng, node Hash hiển thị số batch (số batch > 1 nghĩa là có dùng đĩa)[^explain-sort]; `pg_stat_database.temp_files` đếm số file tạm của mọi truy vấn trong database[^db-temp]; `log_temp_files` bật log từng file tạm (mặc định tắt)[^log-temp]; `temp_file_limit` đặt trần dung lượng file tạm mỗi process, vượt thì transaction bị hủy (mặc định không giới hạn)[^temp-limit].

**3c. `maintenance_work_mem`.** Dành cho thao tác bảo trì như VACUUM, `CREATE INDEX`, `ALTER TABLE ADD FOREIGN KEY`; mặc định 64MB[^mwm]. Vì một session chỉ chạy một thao tác như vậy tại một thời điểm và thường ít cái chạy đồng thời, docs nói đặt lớn hơn `work_mem` khá nhiều là an toàn — nhưng khi autovacuum chạy, bộ nhớ này có thể được cấp tới `autovacuum_max_workers` lần; có thể tách bằng `autovacuum_work_mem`[^mwm-av]. Đối với VACUUM, bộ nhớ này chứa mảng *địa chỉ dòng chết* (dead TID) cần xóa khỏi index; mã nguồn: VACUUM chịu dùng tối đa `maintenance_work_mem` (hoặc `autovacuum_work_mem`) cho mảng đó[^vl-header], và docs nói phần này bị chặn ở 1GB[^mwm-1gb]. Hết chỗ giữa chừng thì phải dừng để vacuum index rồi quay lại; mã nguồn ghi chú rằng index được xử lý *toàn bộ* mỗi lần như vậy, "rất kém hiệu quả khi bộ nhớ eo hẹp"[^vl-inefficient]. Nghĩa là `maintenance_work_mem` quá nhỏ trên bảng nhiều dòng chết làm VACUUM đi qua index nhiều lần (nhận xét của người soạn từ chú thích trên).

**3d. `effective_cache_size`.** Là giả định của planner về kích thước cache đĩa dành cho *một truy vấn*; tham số này vào công thức chi phí của index scan — giá trị cao làm index scan dễ được chọn, giá trị thấp làm seq scan dễ được chọn[^ecs]. Nó không đổi kích thước shared memory cũng không dự trữ cache của kernel, chỉ dùng để ước lượng[^ecs-noalloc]. (Nhận xét của người soạn: vì thế sai số ở tham số này không gây OOM, chỉ gây *plan sai*; kiểm tra nó cùng với `random_page_cost` ở mục 7.)

<details><summary>Đi sâu: ring buffer, đọc RSS của backend, huge pages, WAL buffers</summary>

- *Buffer ring cho scan lớn.* Truy vấn đọc nhiều page *đúng một lần* như VACUUM hay seq scan lớn không dùng clock sweep thông thường mà dùng một vòng buffer nhỏ tái sử dụng suốt lượt scan, để khỏi quét sạch cả cache[^ring]. Với seq scan vòng là 256KB (README `REL_16_4`, chi tiết cài đặt)[^ring-256]; một hệ quả là phần lớn lượng ghi do câu lệnh đó sinh ra do chính backend đảm nhiệm[^ring-write]. VACUUM/ANALYZE dùng *Buffer Access Strategy* cỡ `vacuum_buffer_usage_limit` (mặc định 256kB)[^vbul].
- *Đọc RSS của backend.* Trên Linux, `shared_memory_type` mặc định là `mmap` — shared memory anonymous cấp bằng `mmap`[^shm-type]. Theo định nghĩa ở [P3 · Chương 05, mục 6](../03-memory/05-bo-nho-tien-trinh-linux.md), `RssShmem` gồm cả shared anonymous mapping; vì vậy cộng `RSS` của các backend sẽ đếm trùng vùng shared buffers; muốn cộng cho đúng dùng PSS (suy luận của người soạn từ hai nguồn).
- *Huge pages.* `huge_pages` mặc định `try`: thử xin huge page, thất bại thì quay về mặc định[^huge-try]. Docs lưu ý transparent huge pages (THP) của Linux "đã được biết gây giảm hiệu năng với PostgreSQL ở một số người dùng, một số phiên bản Linux nên hiện không khuyến khích", khác với `huge_pages` tường minh[^thp].
- *WAL buffers.* `wal_buffers` mặc định -1 chọn 1/32 `shared_buffers`, tối thiểu 64kB và tối đa một WAL segment (thường 16MB)[^wal-buf]. Phần WAL của mục 4.
- *`shared_buffers` và `max_wal_size` đi theo cặp.* Docs nói `shared_buffers` lớn thường đòi `max_wal_size` tăng tương ứng, để rải việc ghi lượng lớn dữ liệu mới/thay đổi trên một khoảng thời gian dài hơn[^sb-wal].

</details>

**Quay lại Q3:** `shared_buffers` là cache chung có kích thước cố định; `work_mem` là hạn mức cho *mỗi thao tác sort/hash* nên tổng bộ nhớ là tích số session × thao tác; `maintenance_work_mem` cho VACUUM/`CREATE INDEX`; `effective_cache_size` chỉ là giả định của planner; và page có thể nằm ở cả shared buffers lẫn page cache.

## 4. Đường đi của một `UPDATE`: row version mới, WAL trước, checkpoint (Q4)

> **Trả lời ngắn.** `UPDATE` trong PostgreSQL không sửa tại chỗ: backend thêm một *row version* mới vào page (cùng page nếu còn chỗ — có thể là HOT — hoặc page khác), đánh dấu bản cũ là đã bị thay, và các thay đổi này nằm trong shared buffers (page bị "dirty") cùng một WAL record trong bộ nhớ. Tới `COMMIT` (với `synchronous_commit` mặc định `on`), WAL phải được flush xuống đĩa *trước khi* báo thành công; data page được ghi sau bởi background writer/checkpointer. Checkpoint là mốc từ đó crash recovery phát lại WAL, và lần sửa đầu tiên của mỗi page sau checkpoint kéo theo ghi nguyên page vào WAL (`full_page_writes`).

**Giải thích.** Mục này *ghép* các mảnh đã học thành một đường đi duy nhất; từng mảnh được giải thích kỹ ở chương khác: MVCC, `xmin`/`xmax` và snapshot ở [P12 · Chương 07, mục 4](07-transaction.md); B-tree, torn page, WAL và HOT ở [P12 · Chương 03, mục 5](03-storage-engine.md); giao thức WAL và `fsync` ở [P5 · Chương 05](../05-storage/05-crash-consistency-va-journaling.md) và [P5 · Chương 06](../05-storage/06-durability-trong-thuc-te.md). Cái *mới* ở đây là thứ tự các bước trong mã nguồn `heap_update` và ai (process nào) làm bước nào.

Hình dung (nhận xét của người soạn): bạn sửa một dòng trong sổ cái chung. Bạn không tẩy chữ cũ: bạn viết dòng mới ở cuối trang, gạch chéo dòng cũ và ghi "xem dòng mới". Trước khi nói "xong" với khách, bạn ghi một dòng vào *nhật ký sửa đổi* và chắc chắn nhật ký đã nằm an toàn trong két. Việc chép lại cho đẹp vào sổ cái chính có thể làm sau, vì nhật ký đủ để làm lại nếu mất điện.

**Các bước** (theo mã `heap_update` ở `REL_16_4` và docs; chi tiết nhỏ có thể đổi giữa các bản):

1. *Lock mức bảng và mức hàng.* `UPDATE` lấy lock `ROW EXCLUSIVE` trên bảng đích[^lk-rowexcl]; trên hàng bị sửa nó lấy lock hàng chế độ `FOR NO KEY UPDATE`[^lk-nokey], hoặc `FOR UPDATE` nếu update đổi một số cột nhất định (docs: các cột thuộc unique index dùng được cho foreign key)[^lk-forupd]. Chi tiết ở mục 6.
2. *Ghim buffer, tìm chỗ cho bản mới.* `heap_update` giữ pin và khóa nội dung trên buffer của bản cũ; buffer cho bản mới có thể là *chính buffer đó* (nếu page còn chỗ) hoặc buffer khác; ở điểm này cả hai đều đã pin và khóa, và buffer mới đủ chỗ cho tuple mới[^hu-bufs].
3. *Quyết định HOT.* Nếu bản mới nằm cùng page, kiểm tra xem có cột nào được index tham chiếu bị đổi không; nếu không có thì dùng HOT[^hu-hot]. Điều kiện HOT theo docs (đã nói ở [P12 · Chương 03, mục 5](03-storage-engine.md), phần Đi sâu): update không đổi cột nào được index tham chiếu (không tính index kiểu tóm tắt như BRIN) và page chứa hàng cũ còn đủ chỗ[^hot-doc]. Không phải HOT (đổi cột thuộc index không phải kiểu tóm tắt) thì tuple mới được chèn vào mọi index của bảng[^hot-readme-all]; docs nêu thêm rằng MVCC "có thể đòi index entry mới cho mỗi hàng được update"[^hot-cost].
4. *Sửa page trong bộ nhớ.* Chèn tuple mới vào buffer[^hu-put]; ghi vào bản cũ thông tin transaction đang update (trường `xmax`)[^hu-xmax] và đặt `t_ctid` của bản cũ trỏ tới bản mới[^hu-ctid]; xóa cờ all-visible của page và các bit tương ứng trong visibility map[^hu-vm] (suy luận của người soạn: page vừa có bản mới mà các transaction cũ chưa chắc nhìn thấy nên không còn "mọi tuple thấy được với mọi transaction"); đánh dấu buffer dirty.
5. *Sinh WAL record.* Nếu bảng cần WAL, mã gọi `log_heap_update(...)` để đưa WAL record vào *WAL buffers* trong shared memory rồi ghi LSN của record vào page[^hu-wal]. Docs WAL nhấn mạnh việc đưa record vào buffer phải thật nhanh vì nó diễn ra khi đang giữ lock độc quyền trên page dữ liệu[^wal-insert].
6. *COMMIT.* Với `synchronous_commit` bật (mặc định[^sc-default]), backend ghi commit record rồi gọi `XLogFlush(XactLastRecEnd)` để đẩy WAL tới vị trí cần thiết trước khi báo thành công (chi tiết cài đặt `REL_16_4`: điều kiện gọi có vế `synchronous_commit > SYNCHRONOUS_COMMIT_OFF`)[^xact-flush]. Docs: yêu cầu flush này "phần lớn" được đưa ra lúc commit để bảo đảm các record của transaction nằm trên bộ nhớ bền[^wal-flush-commit]. Nguyên lý WAL — thay đổi data file chỉ được ghi *sau khi* WAL mô tả chúng đã flush[^wal-intro], và một lần `fsync` WAL có thể phục vụ nhiều transaction[^wal-group] — đã được giải thích ở [P5 · Chương 05, mục 8](../05-storage/05-crash-consistency-va-journaling.md) và [P12 · Chương 03, mục 5](03-storage-engine.md); khác biệt `fsync`/`fdatasync` ở [P5 · Chương 06, mục 2](../05-storage/06-durability-trong-thuc-te.md). Mặc định trên Linux, phương thức flush là `fdatasync`[^wal-sync-default].
7. *Sau đó, không nằm trên đường chờ của client.* Data page dirty được ghi dần bởi background writer[^bgw], và *tất cả* dirty page được ghi hết ở mỗi checkpoint, sau đó một checkpoint record được ghi vào WAL[^ckpt-def].

```mermaid
sequenceDiagram
  participant C as Client
  participant B as Backend
  participant SB as Shared buffers
  participant WB as WAL buffers
  participant WF as File WAL
  participant BG as bgwriter / checkpointer
  participant DF as Data file
  C->>B: UPDATE …
  B->>SB: pin buffer, thêm row version mới, sửa xmax/t_ctid bản cũ (page dirty)
  B->>WB: WAL record (log_heap_update)
  C->>B: COMMIT
  B->>WB: commit record
  B->>WF: write + flush (synchronous_commit = on; mặc định Linux: fdatasync)
  B-->>C: COMMIT thành công
  BG->>SB: tìm buffer dirty
  BG->>DF: ghi data page (checkpoint: ghi hết, rồi ghi checkpoint record)
```

*Sơ đồ: minh họa của người soạn từ các nguồn trên; thứ tự giữa bước 5 và 6 là thứ tự logic, không phải mô tả từng lệnh gọi hệ thống.*

**Vì sao crash không làm mất commit.** Nếu server chết sau bước 6 nhưng trước khi data page được ghi, thì WAL đã có đủ: sau crash, recovery đọc checkpoint record gần nhất, lấy từ đó vị trí *redo record* và phát lại (REDO) mọi thay đổi từ điểm đó[^ckpt-redo]. Nghĩa là *tần suất checkpoint quyết định thời gian recovery*: docs nói giảm `checkpoint_timeout`/`max_wal_size` làm checkpoint dày hơn, recovery nhanh hơn (ít phải làm lại), đổi lại chi phí flush data page dày hơn[^ckpt-tradeoff].

**Checkpoint kích hoạt khi nào, và tốn gì.** Checkpoint bắt đầu mỗi `checkpoint_timeout` giây (mặc định 5 phút) hoặc khi WAL sắp vượt `max_wal_size` (mặc định 1GB), cái nào tới trước[^ckpt-trigger][^ckpt-defaults]. Vì ghi hết dirty page là tải I/O lớn nên checkpoint được "dàn trải": I/O bắt đầu từ lúc checkpoint khởi động và xong trước khi checkpoint kế tiếp tới hạn[^ckpt-spread]; `checkpoint_completion_target` là mục tiêu hoàn thành tính theo tỷ lệ của khoảng cách giữa hai checkpoint, mặc định 0.9[^ckpt-target]. Docs nói checkpoint khá đắt, vì phải ghi hết buffer dirty và vì sinh thêm WAL (xem `full_page_writes` ngay dưới)[^ckpt-expensive].

**`full_page_writes`: vì sao WAL phình sau checkpoint.** Cơ chế torn page, và việc ghi nguyên nội dung page vào WAL ở lần sửa *đầu tiên* của mỗi page sau mỗi checkpoint khi `full_page_writes` bật (mặc định[^fpw-default]), đã được giải thích ở [P12 · Chương 03, mục 5](03-storage-engine.md)[^fpw-def]. Phần *vận hành* thêm ở đây: docs nêu khoảng cách checkpoint nhỏ hơn làm tăng lượng WAL, phần nào triệt tiêu mục tiêu của việc rút ngắn khoảng cách[^ckpt-fpw]. `wal_compression` (mặc định tắt[^wal-compress-def]) giảm lượng WAL, đổi lại tốn CPU lúc ghi và phát lại[^wal-compress]. Tắt `full_page_writes` làm nhanh hơn nhưng có thể dẫn tới hỏng dữ liệu im lặng sau sự cố hệ thống, rủi ro tương tự tắt `fsync`[^fpw-off].

**Hệ quả với bảng nhiều update (nhận xét của người soạn, có nền từ docs).** (1) Muốn update thành HOT hơn, tránh index lên cột thay đổi liên tục (ví dụ `updated_at`) và có thể hạ `fillfactor` của bảng để page chừa chỗ — docs nói điều này làm tăng khả năng đủ chỗ cho HOT update và có thể theo dõi tỷ lệ HOT bằng `pg_stat_all_tables`[^hot-fill]. (2) Mỗi update không-HOT làm index phình và tăng việc cho VACUUM (mục 5). (3) Tốc độ sinh WAL ảnh hưởng trực tiếp tới replication ([P12 · Chương 05, mục 3](05-replication.md)) và tới việc `max_wal_size` có bị chạm không.

<details><summary>Đi sâu: HOT và pruning, group commit, <code>synchronous_commit</code></summary>

- *HOT chain và pruning.* README của HOT: phiên bản mới không có index entry nên chỉ có một index entry cho cả chuỗi update trên page[^hot-chain]; từ entry đó, tìm kiếm đi tiếp tới các tuple con chừng nào còn thấy cờ `HEAP_HOT_UPDATED`[^hot-chain2]. Vì chuỗi nằm gọn trong một page nên có thể dọn "tại chỗ" (pruning) mà không cần vacuum cả bảng: docs HOT nói phiên bản cũ có thể bị xóa hoàn toàn *trong hoạt động bình thường, kể cả `SELECT`*, thay vì phải đợi VACUUM định kỳ, vì index không tham chiếu tới item identifier của chúng[^hot-prune]. HOT chain kết thúc khi update đổi cột nằm trong một index không phải kiểu tóm tắt (BRIN không tính) hoặc khi page không còn chỗ cho bản mới[^hot-end].
- *Group commit.* `commit_delay` (mặc định 0) cho một *group commit leader* ngủ một chút sau khi giữ lock trong `XLogFlush` để các transaction khác kịp đưa commit record vào buffer và được flush chung; kể cả khi bằng 0, vẫn có dạng group commit gồm các session chạm tới điểm cần flush trong lúc lần flush trước đang diễn ra[^commit-delay].
- *`synchronous_commit`.* `on` (mặc định) chờ flush WAL cục bộ; ở chế độ `off`, không chờ, nên có thể có độ trễ giữa lúc báo thành công và lúc transaction thật sự an toàn trước crash; docs nói độ trễ tối đa là ba lần `wal_writer_delay` và khác với `fsync`, tắt `synchronous_commit` *không* gây rủi ro hỏng nhất quán database — chỉ có thể mất vài transaction vừa "commit"[^sync-commit-off]. Các giá trị `remote_write`/`remote_apply` liên quan tới standby: [P12 · Chương 05, mục 1](05-replication.md).
- *WAL writer và asynchronous commit.* WAL writer flush theo `wal_writer_delay` (mặc định 200ms[^walw-delay-def]) hoặc khi có đủ lượng WAL (`wal_writer_flush_after`, mặc định 1MB[^walw-flush-def]).

</details>

**Quay lại Q4:** `UPDATE` thêm row version mới và đánh dấu bản cũ trong shared buffers, sinh WAL record; `COMMIT` flush WAL (không phải data page) rồi mới báo thành công; data page được ghi sau bởi bgwriter/checkpointer, và checkpoint + `full_page_writes` quyết định thời gian recovery và lượng WAL.

## 5. VACUUM, autovacuum, bloat, XID wraparound và transaction mở lâu (Q5)

> **Trả lời ngắn.** Vì `UPDATE`/`DELETE` không xóa ngay phiên bản cũ của hàng[^vac-nodelete], bảng và index tích tụ "dòng chết"; VACUUM dọn chúng, đánh dấu chỗ trống để tái sử dụng, cập nhật visibility map, và "đóng băng" (freeze) hàng cũ để chống XID wraparound[^vac-reasons]. Autovacuum chạy việc này tự động khi số dòng thay đổi vượt ngưỡng[^av-trig]. Nhưng VACUUM chỉ xóa được phiên bản mà *không transaction nào còn có thể thấy*: một transaction mở lâu (kể cả "idle in transaction") giữ các phiên bản đó lại và làm bảng phình[^iit]; còn nếu autovacuum không freeze kịp, server sẽ cảnh báo rồi *từ chối thao tác ghi* khi quá gần wraparound[^wrap-refuse].

**Giải thích.**

Hình dung (nhận xét của người soạn): tờ giấy cũ trong ngăn kéo (mục 4 của [P12 · Chương 07](07-transaction.md)) chỉ vứt được khi không ai còn cầm tờ đó. VACUUM là người đi dọn ngăn kéo. Nếu một người cầm tờ giấy cũ rồi đi ăn trưa ba tiếng, người dọn phải giữ lại *mọi* tờ giấy bị thay thế *sau* thời điểm người đó bắt đầu cầm (vì anh ta có thể vẫn đang đọc chúng) — không chỉ tờ trong tay anh ta. Còn "wraparound" là chuyện đồng hồ số thứ tự chỉ có 32 bit: phải định kỳ đóng dấu "đã cũ chắc chắn" lên các tờ giấy rất xưa, nếu không đồng hồ quay vòng và tờ giấy xưa bỗng trông như từ tương lai.

**5a. Dòng chết và VACUUM làm gì.**

Vì sao có dòng chết đã nói ở [P12 · Chương 07, mục 4](07-transaction.md): `UPDATE`/`DELETE` không loại bỏ phiên bản cũ ngay vì transaction khác có thể còn nhìn thấy nó; VACUUM thu hồi không gian về sau[^vac-nodelete]. VACUUM (dạng chuẩn) xóa các phiên bản chết khỏi bảng *và index* rồi đánh dấu chỗ trống cho tái sử dụng — nhưng *không trả không gian cho hệ điều hành*, trừ trường hợp đặc biệt khi các page ở cuối bảng trống hoàn toàn và lấy được lock độc quyền dễ dàng[^vac-space]. Dạng `VACUUM FULL` viết lại cả bảng thành file mới không có chỗ chết, nhưng cần `ACCESS EXCLUSIVE` nên không chạy song song với mọi truy cập khác[^vac-full]; docs đặt mục tiêu là chạy VACUUM chuẩn đủ thường để khỏi cần `VACUUM FULL`, giữ mức dùng đĩa ở trạng thái ổn định: mỗi bảng chiếm cỡ kích thước tối thiểu cộng phần phát sinh giữa hai lần vacuum[^vac-steady]. Dạng chuẩn chạy song song với `SELECT`/`INSERT`/`UPDATE`/`DELETE` bình thường, nhưng trong lúc đó bạn không đổi được định nghĩa bảng bằng `ALTER TABLE`[^vac-concurrent].

*Bloat* (phình) — thuật ngữ của người soạn, không phải của docs: bảng hoặc index to hơn mức dữ liệu sống cần, do dòng chết hoặc chỗ trống chưa tái sử dụng được. Hậu quả (nhận xét của người soạn): scan phải đọc nhiều page hơn; index to hơn thì ít đoạn index vừa trong cache. Docs dùng chính chữ "bloat" khi nói về hậu quả của transaction mở lâu[^iit] và của `hot_standby_feedback`[^hsf].

Bên trong một lần VACUUM (comment của `lazy_scan_heap` trong `REL_16_4`): quét heap, *prune* từng page và cân nhắc freeze các tuple còn lại (bỏ qua các page nhờ visibility map); gọi `lazy_vacuum_all_indexes` để xóa index tuple trỏ tới các item `LP_DEAD`; cuối cùng `lazy_vacuum_heap_rel` chuyển `LP_DEAD` thành `LP_UNUSED` ở lượt thứ hai[^vl-phases]. Phải xóa index trước rồi mới tái sử dụng line pointer, vì không được phép còn index tuple nào trỏ tới một line pointer đã thành `LP_UNUSED`[^vl-invariant]. Đây là lý do mảng TID phải ở trong bộ nhớ (mục 3c) và lý do HOT tiết kiệm việc: HOT dọn được tại chỗ mà không cần động tới index (mục 4) (suy luận của người soạn từ các chú thích trên).

*Visibility map* (VM): mỗi page của heap có hai bit — bit thứ nhất "all-visible" (page không chứa tuple nào cần vacuum), bit thứ hai "all-frozen" (mọi tuple trong page đã freeze nên ngay cả anti-wraparound vacuum cũng không cần ghé lại)[^vm-bits][^vm-frozen]. VM có hai công dụng: VACUUM bỏ qua các page đó lần sau, và *index-only scan* trả lời chỉ bằng index vì không cần ghé heap kiểm tra visibility khi page được đánh dấu all-visible[^vm-uses] (xem mục 7).

**5b. Autovacuum: ai chạy, khi nào chạy.**

Autovacuum gồm một *launcher* thường trực và nhiều *worker*: launcher cố gắng khởi động một worker trong mỗi database mỗi `autovacuum_naptime` (mặc định 1 phút)[^av-naptime]; tối đa `autovacuum_max_workers` worker chạy cùng lúc (mặc định 3, đổi phải restart)[^av-workers]. Với mỗi bảng, worker quyết định chạy VACUUM khi số tuple bị lỗi thời từ lần vacuum trước vượt *ngưỡng* (công thức của docs):

```text
ngưỡng vacuum = ngưỡng cơ sở + scale factor × số tuple
```

trong đó base threshold là `autovacuum_vacuum_threshold` (mặc định 50), scale factor là `autovacuum_vacuum_scale_factor` (mặc định 0,2) và số tuple là `pg_class.reltuples`[^av-formula][^av-thr-def][^av-scale-def]. Bảng cũng được vacuum khi số tuple *chèn* vượt ngưỡng chèn tương tự (mặc định 1000 + 0,2 × số tuple)[^av-insert][^av-insert-def][^av-insert-scale]; ANALYZE theo công thức song song với mặc định 50 và 0,1[^av-analyze][^av-analyze-thr][^av-analyze-scale]. Bảng có `relfrozenxid` quá `autovacuum_freeze_max_age` luôn bị vacuum, kể cả khi autovacuum "tắt"[^freeze-max].

Hệ quả cần tính ra (suy luận của người soạn, phép nhân với số mặc định của docs): bảng 100 triệu dòng, ngưỡng mặc định là 50 + 0,2 × 100 000 000 = 20 000 050 tuple lỗi thời mới kích hoạt autovacuum; với ANALYZE là 50 + 0,1 × 100 000 000 = 10 000 050 dòng thay đổi. Tức là *bảng càng lớn thì autovacuum càng "lười"* nếu để nguyên scale factor — nên bảng rất lớn thường được hạ scale factor riêng (nhận xét của người soạn). Docs xác nhận có thể ghi đè các ngưỡng này, và nhiều tham số autovacuum khác, theo từng bảng bằng storage parameter[^av-pertable].

*Autovacuum chạy chậm có chủ đích.* VACUUM tạo nhiều I/O nên có cơ chế *cost-based delay*: một bộ đếm chi phí I/O ước lượng; khi đạt `vacuum_cost_limit` thì tiến trình ngủ `vacuum_cost_delay` rồi tiếp tục[^cost-delay]. Với autovacuum, `autovacuum_vacuum_cost_delay` mặc định 2ms[^av-cost-def]; khi có nhiều worker, các tham số này được "cân bằng" để tổng tác động I/O không đổi dù có bao nhiêu worker đang chạy[^av-balance]. Nghĩa là thêm worker *không* làm vacuum nhanh gấp đôi mà chia chung một ngân sách I/O (suy luận của người soạn từ câu trên); muốn nhanh hơn phải nâng ngân sách (`autovacuum_vacuum_cost_limit`) hoặc hạ delay.

*Autovacuum nhường chỗ.* Autovacuum giữ lock `SHARE UPDATE EXCLUSIVE`; nếu một tiến trình xin lock xung đột thì việc xin lock sẽ *ngắt* autovacuum[^av-interrupt]. Ngoại lệ: autovacuum chạy để chống wraparound (tên truy vấn trong `pg_stat_activity` kết thúc bằng `(to prevent wraparound)`) thì không tự bị ngắt[^av-wrap-nointr]. Docs cảnh báo lệnh thường xuyên xin lock xung đột với `SHARE UPDATE EXCLUSIVE` (ví dụ `ANALYZE`) có thể khiến autovacuum không bao giờ hoàn thành[^av-warn]. Cũng cần biết: autovacuum *không* chạy ANALYZE cho bảng partitioned (cha), nên cần ANALYZE thủ công — hệ quả với planner ở mục 7[^av-partitioned].

**5c. Transaction mở lâu chặn VACUUM.** Mã nguồn VACUUM tính một *cutoff* `OldestXmin` để quyết định tuple đã xóa nào là DEAD (xóa được) chứ không chỉ `RECENTLY_DEAD` (mới chết)[^vl-cutoff]; và kỳ vọng pruning xóa mọi tuple đã xóa có `xmax` nhỏ hơn `OldestXmin`[^vl-vistest]. `OldestXmin` được tính từ `xmin` của các backend, có xét cả `xid` của chúng vì một transaction có thể có `xmin` mà chưa có `xid` hoặc ngược lại (hàm `ComputeXidHorizons`)[^ph-xid][^ph-backend-xmin]; backend ở database khác thường bị bỏ qua, trừ horizon của bảng dùng chung[^ph-otherdb]; horizon còn bị kéo lùi thêm bởi `xmin` của replication slot[^ph-slot-xmin]; kết quả là "xid cũ nhất mà tuple đã xóa còn phải giữ trong bảng người dùng"[^ph-data-oldest]. Cột `backend_xmin` của `pg_stat_activity` chính là *xmin horizon* của backend đó[^bxmin]. VACUUM báo số tuple bị giữ lại trong log/`VERBOSE` bằng dòng `dead but not yet removable`[^vl-log]. Docs nói thẳng điều này ở phần `idle_in_transaction_session_timeout`: ngay cả khi không giữ lock đáng kể, một transaction mở ngăn việc vacuum các tuple "recently-dead" có thể chỉ transaction đó nhìn thấy, nên nằm idle lâu có thể góp phần gây bloat[^iit].

Nguồn của "horizon cũ" không chỉ là connection người dùng quên đóng — docs liệt kê khi xử lý wraparound ba nơi cần tìm: transaction đã `PREPARE` còn treo (`pg_prepared_xacts`)[^wrap-prepared], transaction mở lâu (`pg_stat_activity`, xem `age(backend_xid)`/`age(backend_xmin)`)[^wrap-longtx], và replication slot cũ (`pg_replication_slots`)[^wrap-slots]. Một standby bật `hot_standby_feedback` cũng gửi thông tin về truy vấn đang chạy trên standby và "có thể gây bloat ở primary với một số workload"[^hsf] (liên hệ [P12 · Chương 05](05-replication.md)).

```mermaid
flowchart LR
  L["row version<br/>còn sống"] -->|"UPDATE/DELETE<br/>commit"| R["dead nhưng còn<br/>thấy được bởi snapshot cũ<br/>(RECENTLY_DEAD)"]
  R -->|"không còn snapshot/slot/<br/>prepared xact nào cần"| D["DEAD: xóa được"]
  D -->|"pruning / VACUUM<br/>(xóa index entry trước)"| U["LP_UNUSED:<br/>chỗ trống tái sử dụng"]
  T["transaction mở lâu,<br/>replication slot, prepared xact"] -.->|"kéo lùi OldestXmin,<br/>giữ ở trạng thái RECENTLY_DEAD"| R
```

*Sơ đồ: minh họa của người soạn từ `vacuumlazy.c`, `procarray.c` và docs.*

**5d. XID wraparound và freezing.**

*Vấn đề.* XID chỉ có 32 bit, nên một cụm chạy lâu (hơn 4 tỷ transaction) sẽ gặp wraparound: bộ đếm quay về 0, transaction cũ bỗng trông như ở tương lai và dữ liệu của chúng trở nên vô hình; docs gọi đó là mất dữ liệu thảm họa dù dữ liệu thực ra vẫn còn đó nhưng không truy cập được[^xid-32] (cái giá này đã được nêu ở [P12 · Chương 07, mục 4](07-transaction.md)). XID thường được so sánh theo số học modulo 2³²: với mọi XID bình thường, có hai tỷ XID "cũ hơn" và hai tỷ "mới hơn", tức không gian XID là một vòng tròn không điểm cuối[^xid-modulo].

*Giải pháp: freeze.* VACUUM đánh dấu hàng là *frozen*: đã được chèn bởi transaction commit đủ lâu trong quá khứ để chắc chắn mọi transaction hiện tại và tương lai đều thấy hiệu ứng của nó[^xid-freeze]. Từ 9.4, freeze chỉ đặt một cờ chứ không thay `xmin` gốc[^freeze-flag]. Các ngưỡng (mặc định theo docs 16):

| Tham số | Mặc định | Ý nghĩa |
|---|---|---|
| `vacuum_freeze_min_age` | 50 triệu | mốc tuổi XID để VACUUM quyết định có freeze các page chứa XID cũ hơn hay không[^freeze-min] |
| `vacuum_freeze_table_age` | 150 triệu | `relfrozenxid` đạt tuổi này thì VACUUM thành *aggressive* — quét mọi page có thể chứa XID chưa freeze, không chỉ page có dòng chết[^freeze-table] |
| `autovacuum_freeze_max_age` | 200 triệu | `relfrozenxid` đạt tuổi này thì bảng *bị ép* vacuum chống wraparound, ngay cả khi autovacuum bị tắt[^freeze-max][^freeze-max-def] |
| `vacuum_failsafe_age` | 1,6 tỷ | VACUUM bước vào chế độ "cuối cùng": bỏ cost-based delay, bỏ các việc không thiết yếu như vacuum index[^failsafe][^failsafe-def] |

VACUUM thường bỏ qua page không có dòng chết kể cả khi chúng còn XID cũ, nên VACUUM thường không phải lúc nào cũng freeze hết; vì vậy cần aggressive vacuum định kỳ, mà phần lớn bảng đều cần[^aggressive]. VM bit "all-frozen" cho phép aggressive vacuum bỏ qua page đã freeze hết[^vm-aggr].

*Theo dõi tuổi.* VACUUM lưu thông tin ở `pg_class.relfrozenxid` (XID chưa freeze cũ nhất còn lại ở cuối lần VACUUM gần nhất đã đẩy được nó tiến lên) và `pg_database.datfrozenxid` (cận dưới của các XID chưa freeze trong database; là giá trị nhỏ nhất trong các `relfrozenxid`)[^frozen-cols]. Docs đưa sẵn truy vấn mẫu dùng `age(c.relfrozenxid)` và `age(datfrozenxid)`; cột `age` đo số transaction từ XID cắt tới XID hiện tại[^age-col]. Dưới đây là hai truy vấn theo mẫu của docs (viết lại, chưa chạy):

```sql
-- tuổi XID chưa freeze của từng bảng (gộp cả bảng TOAST đi kèm)
SELECT c.oid::regclass AS table_name,
       greatest(age(c.relfrozenxid), age(t.relfrozenxid)) AS age
FROM pg_class c
LEFT JOIN pg_class t ON c.reltoastrelid = t.oid
WHERE c.relkind IN ('r', 'm');

-- tuổi XID chưa freeze của từng database
SELECT datname, age(datfrozenxid) FROM pg_database;
```

*Khi sự cố xảy ra.* Nếu autovacuum không dọn được XID cũ, hệ thống bắt đầu in cảnh báo khi XID cũ nhất của database còn bốn mươi triệu transaction nữa là wraparound[^wrap-warn]; nếu bỏ qua, server từ chối cấp XID mới khi còn dưới ba triệu transaction[^wrap-threshold], khi đó các transaction đang chạy tiếp được nhưng chỉ transaction chỉ-đọc mới bắt đầu được, thao tác ghi thất bại, trong khi `VACUUM` vẫn chạy bình thường[^wrap-refuse]. Docs hướng dẫn khắc phục *không* cần dừng postmaster hay vào chế độ single-user (trái với gợi ý trong thông báo lỗi) mà làm theo thứ tự: xử lý prepared transaction cũ, kết thúc transaction mở lâu, xóa replication slot cũ, rồi chạy VACUUM trên database đích; **không** dùng `VACUUM FULL` vì nó cần XID và sẽ thất bại[^wrap-novacfull][^wrap-nosingle]. Nhận xét của người soạn: danh sách này chính là bảng kiểm "ai đang giữ horizon" ở 5c — wraparound và bloat có cùng nguyên nhân gốc.

<details><summary>Đi sâu: <code>INDEX_CLEANUP</code>, truncate, multixact, tiến độ VACUUM</summary>

- *Bỏ qua vacuum index.* `VACUUM` mặc định (`INDEX_CLEANUP AUTO`) bỏ qua vacuum index khi bảng có rất ít dòng chết vì chi phí xử lý mọi index được coi là vượt xa lợi ích[^idx-cleanup]. Trong mã nguồn, VACUUM bỏ qua khi số page có item `LP_DEAD` ít hơn ngưỡng tính từ hằng số `BYPASS_THRESHOLD_PAGES` = 0,02, tức 2% số page của bảng[^vl-bypass], và tổng số item `LP_DEAD` còn dưới một giới hạn cứng[^vl-bypass2] (chi tiết cài đặt trong `vacuumlazy.c`, có thể đổi).
- *Cắt đuôi bảng cần `ACCESS EXCLUSIVE`.* Tùy chọn `TRUNCATE` (mặc định bật) cắt các page trống ở cuối bảng để trả đĩa cho OS; docs nói tắt nó có ích để tránh lock `ACCESS EXCLUSIVE` mà việc cắt đòi hỏi[^vac-truncate].
- *Parallel vacuum.* VACUUM chuẩn có thể xử lý index song song bằng nhiều worker (tùy `PARALLEL`, `max_parallel_maintenance_workers`)[^vac-parallel].
- *Multixact.* Khi nhiều transaction cùng khóa một hàng, thông tin được mã hóa thành *multixact ID*[^multixact] — cũng 32 bit, cũng có wraparound và cũng cần aging/freeze riêng[^multixact32]. Cạn XID chặn mọi transaction ghi, còn cạn multixact chỉ chặn tập con thao tác cần multixact (khóa hàng nhiều bên)[^mxid-subset].
- *Theo dõi.* Mỗi backend chạy VACUUM (không `FULL`) báo tiến độ ở `pg_stat_progress_vacuum`[^vac-progress]; autovacuum ghi log nếu chạy từ `log_autovacuum_min_duration` trở lên — mặc định 10 phút (mục 9)[^av-log].

</details>

**Quay lại Q5:** VACUUM dọn dòng chết, cập nhật visibility map và freeze hàng cũ; autovacuum kích hoạt theo ngưỡng cộng với scale factor (nên bảng lớn cần chỉnh riêng); mọi thứ giữ *xmin horizon* cũ — transaction mở lâu, prepared transaction, replication slot — chặn dọn dẹp và đẩy cụm về phía wraparound.

## 6. Lock: table lock, row lock, hàng đợi, deadlock và migration (Q6)

> **Trả lời ngắn.** PostgreSQL có tám chế độ lock mức bảng; điểm khác nhau duy nhất giữa chúng là tập chế độ mà mỗi cái xung đột[^lk-intro2]. Câu `SELECT` thường chỉ lấy `ACCESS SHARE`, và chỉ `ACCESS EXCLUSIVE` mới chặn được nó[^lk-tip]; nhiều dạng `ALTER TABLE` lại đòi đúng `ACCESS EXCLUSIVE`[^at-lock]. Cái bẫy vận hành: lock xin *xếp hàng*, nên một `ALTER TABLE` đang chờ sau một transaction dài sẽ khiến *mọi truy vấn đến sau* cũng phải chờ — cả bảng "đứng" (suy luận từ luật hàng đợi của lock manager, xem mục 6c)[^lock-rule1]. Phòng bằng `lock_timeout` ngắn, thiết kế migration tránh lock mạnh, và giữ transaction ngắn.

**Giải thích.**

Hình dung (nhận xét của người soạn): một phòng họp có ba kiểu người: người *dự thính* (đọc), người *ghi biên bản* (ghi hàng), và người *dọn phòng* (đổi cấu trúc: dọn hết ghế rồi lắp lại). Dự thính và ghi biên bản ngồi chung được; người dọn phòng đòi cả phòng trống. Quy tắc xếp hàng ở cửa là: nếu người dọn phòng đang đứng chờ ở cửa, *ai đến sau cũng phải đứng sau anh ta*, dù người đến sau chỉ muốn dự thính. Vậy nên một người dự thính ngồi quá lâu bên trong có thể làm cả hàng dài tắc ngoài cửa.

**6a. Lock mức bảng.** Docs nhấn mạnh: *mọi* chế độ dưới đây đều là lock mức bảng, kể cả tên có chữ "row" — tên là di sản lịch sử[^lk-intro]; khác biệt thật duy nhất là tập chế độ xung đột; hai transaction không thể cùng giữ lock xung đột trên cùng một bảng[^lk-intro2]. Lock được giữ đến hết transaction (hoặc nhả ngay khi rollback về savepoint đã lấy lock sau nó)[^lk-hold].

| Chế độ | Ví dụ lệnh lấy (theo docs) | Xung đột với |
|---|---|---|
| `ACCESS SHARE` | `SELECT` thường[^lk-as] | chỉ `ACCESS EXCLUSIVE` |
| `ROW SHARE` | `SELECT … FOR UPDATE/SHARE/…`[^lk-rs] | `EXCLUSIVE`, `ACCESS EXCLUSIVE` |
| `ROW EXCLUSIVE` | `UPDATE`, `DELETE`, `INSERT`, `MERGE`[^lk-rowexcl] | `SHARE`, `SHARE ROW EXCLUSIVE`, `EXCLUSIVE`, `ACCESS EXCLUSIVE`[^lk-re] |
| `SHARE UPDATE EXCLUSIVE` | `VACUUM` (không `FULL`), `ANALYZE`, `CREATE INDEX CONCURRENTLY`, vài dạng `ALTER TABLE`[^lk-sue] | chính nó và mọi chế độ từ `SHARE` trở lên |
| `SHARE` | `CREATE INDEX` (không `CONCURRENTLY`)[^lk-s] | `ROW EXCLUSIVE` và mọi chế độ mạnh hơn (trừ chính `SHARE`) |
| `SHARE ROW EXCLUSIVE` | `CREATE TRIGGER`, vài dạng `ALTER TABLE` (như `ADD FOREIGN KEY`)[^lk-sre][^at-fk] | `ROW EXCLUSIVE` trở lên, kể cả chính nó |
| `EXCLUSIVE` | `REFRESH MATERIALIZED VIEW CONCURRENTLY`[^lk-e] | mọi chế độ trừ `ACCESS SHARE` |
| `ACCESS EXCLUSIVE` | `DROP TABLE`, `TRUNCATE`, `REINDEX`, `CLUSTER`, `VACUUM FULL`, nhiều dạng `ALTER TABLE`/`ALTER INDEX`[^lk-ae] | mọi chế độ |

Bảng xung đột đầy đủ (✗ = hai chế độ *không* cùng tồn tại được trên một bảng; người soạn dựng lại từ cột "Xung đột với" ở bảng trên — mỗi hàng ở đó có nguồn riêng — rồi đối chiếu từng ô với Table 13.2 của docs[^lk-table]; ma trận đối xứng):

| yêu cầu ↓ \ đang giữ → | AS | RS | RE | SUE | S | SRE | E | AE |
|---|---|---|---|---|---|---|---|---|
| **ACCESS SHARE** (AS) |  |  |  |  |  |  |  | ✗ |
| **ROW SHARE** (RS) |  |  |  |  |  |  | ✗ | ✗ |
| **ROW EXCLUSIVE** (RE) |  |  |  |  | ✗ | ✗ | ✗ | ✗ |
| **SHARE UPDATE EXCLUSIVE** (SUE) |  |  |  | ✗ | ✗ | ✗ | ✗ | ✗ |
| **SHARE** (S) |  |  | ✗ | ✗ |  | ✗ | ✗ | ✗ |
| **SHARE ROW EXCLUSIVE** (SRE) |  |  | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **EXCLUSIVE** (E) |  | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **ACCESS EXCLUSIVE** (AE) | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |

Cách đọc nhanh: `UPDATE` (RE) không xung đột với `SELECT` (AS) hay với `UPDATE` khác (RE) — nên ghi và đọc chạy đồng thời; `UPDATE` xung đột với `CREATE INDEX` thường (S) — nên tạo index kiểu thường chặn ghi (nhưng không chặn đọc, vì AS không xung đột với S)[^ci-conc]; `VACUUM` (SUE) không chặn đọc/ghi nhưng xung đột với chính nó và với `ALTER TABLE`/`CREATE INDEX` thường (suy luận của người soạn từ bảng).

**6b. Lock mức hàng và vì sao nó khó thấy.** Ngoài lock bảng còn lock hàng (`FOR UPDATE`, `FOR NO KEY UPDATE`, `FOR SHARE`, `FOR KEY SHARE`); chúng không ảnh hưởng truy vấn đọc thường, chỉ chặn bên ghi và bên khóa cùng hàng, và nhả cuối transaction hoặc khi rollback tới savepoint[^lk-row]. Cách dùng và ngữ nghĩa các chế độ này: [P12 · Chương 07, mục 5](07-transaction.md). Điều *vận hành* cần biết thêm: PostgreSQL không nhớ hàng bị khóa trong bộ nhớ nên không có giới hạn số hàng khóa, đổi lại việc khóa hàng có thể gây ghi đĩa vì phải đánh dấu trên chính hàng[^lk-row-disk]. Hệ quả: thông tin lock hàng nằm trên đĩa nên *thường không hiện trong `pg_locks`*; tiến trình đang chờ lock hàng thường hiện là đang chờ *transaction ID thường trực của bên giữ lock*[^pglocks-row] — vì transaction nào đã được cấp transaction ID thường trực (thường chỉ khi nó thay đổi trạng thái database, như sửa hàng) thì giữ lock độc quyền trên ID đó đến khi kết thúc, và muốn chờ transaction khác kết thúc thì xin lock chia sẻ trên ID đó[^pglocks-xid]. Đó là lý do wait event của kiểu chờ này là `Lock: transactionid` (docs mô tả: chờ một transaction kết thúc)[^we-xid].

**6c. Hàng đợi lock: vì sao `ALTER TABLE` làm cả bảng đứng.** README của lock manager (REL_16_4) mô tả luật cấp lock: một yêu cầu lock được cấp ngay nếu *không xung đột với bất kỳ lock đang giữ hay yêu cầu đang chờ nào*, hoặc nếu chính process đó đã giữ một lock cùng loại[^lock-rule1]. Ngược lại nó vào hàng đợi chờ; khi lock được nhả, các waiter được đánh thức theo thứ tự, và luật này bảo đảm các yêu cầu xung đột được cấp theo thứ tự đến[^lock-order]. Cũng vì vậy `pg_blocking_pids()` định nghĩa "chặn" gồm cả *hard block* (giữ lock xung đột) lẫn *soft block* (đang chờ một lock sẽ xung đột và nằm trước trong hàng đợi)[^blockpids].

Ghép lại (suy luận của người soạn từ các luật trên, không phải trích nguyên văn):

```mermaid
sequenceDiagram
  participant L as Session L (transaction dài, đã SELECT, chưa commit)
  participant M as Session M (ALTER TABLE, cần ACCESS EXCLUSIVE)
  participant R as Request mới (SELECT hoặc UPDATE)
  participant LM as Lock manager
  L->>LM: ACCESS SHARE
  LM-->>L: cấp (giữ đến hết transaction)
  M->>LM: ACCESS EXCLUSIVE
  LM-->>M: xung đột với L, M vào hàng đợi
  R->>LM: ACCESS SHARE / ROW EXCLUSIVE
  LM-->>R: xung đột với yêu cầu đang chờ của M, R xếp sau M
  Note over L,R: cả bảng đứng cho tới khi L kết thúc rồi M xong, hoặc M bị lock_timeout
```

Trình tự: (1) L đọc bảng và *chưa kết thúc transaction* (có thể chỉ vì ứng dụng quên commit — mục 5); (2) migration `ALTER TABLE` cần `ACCESS EXCLUSIVE` nên phải chờ L; (3) mọi truy vấn mới tới bảng — kể cả `SELECT` vốn xung đột với `ACCESS EXCLUSIVE` đang chờ — xếp sau M. Triệu chứng nhìn từ ngoài: không phải bảng chậm mà là *mọi* truy vấn tới bảng cùng treo, pool connection của ứng dụng cạn dần (mục 2). Lối thoát: chấm dứt L, hoặc để M tự bỏ cuộc nhờ `lock_timeout`.

**6d. Migration an toàn hơn** (nhận xét của người soạn, mỗi mệnh đề kỹ thuật kèm nguồn):

1. *Đặt `lock_timeout` ngắn cho phiên chạy migration, rồi retry.* Docs: `lock_timeout` hủy câu lệnh chờ lock quá thời gian chỉ định, áp dụng riêng cho từng lần xin lock, cả khóa tường minh lẫn lock tự động[^lt-def]; mặc định 0 là tắt[^lt-default]; docs khuyến cáo *không* đặt nó trong `postgresql.conf` vì sẽ ảnh hưởng mọi session[^lt-conf] — đặt trong session/transaction chạy migration (`SET`/`SET LOCAL`, xem mục 2 để biết khác biệt khi có PgBouncer). Đặt `lock_timeout` bằng hoặc lớn hơn `statement_timeout` là vô nghĩa vì `statement_timeout` luôn đến trước[^lt-vs-st].
2. *Biết lệnh nào lấy lock gì.* Docs: `ALTER TABLE` lấy `ACCESS EXCLUSIVE` trừ khi ghi khác, và khi có nhiều subcommand thì lấy chế độ mạnh nhất trong số đó[^at-lock].
3. *`ADD COLUMN`:* nếu `DEFAULT` không volatile thì giá trị mặc định được lưu trong metadata và không cần viết lại bảng; `DEFAULT` volatile hoặc đổi kiểu cột thì phải viết lại cả bảng và index[^at-addcol][^at-rewrite]; ngoại lệ khi đổi kiểu: nếu `USING` không đổi nội dung cột và kiểu cũ binary coercible sang kiểu mới thì không cần viết lại bảng[^at-rewrite-exc].
4. *Thêm ràng buộc:* với foreign key hoặc `CHECK` mới, mặc định phải quét bảng để kiểm tra, và các update khác bị khóa cho tới khi `ALTER TABLE ADD CONSTRAINT` commit; tùy chọn `NOT VALID` (PG16 chỉ cho phép với foreign key và `CHECK`[^at-notvalid-scope]) bỏ bước quét, commit ngay, rồi `VALIDATE CONSTRAINT` chạy sau để kiểm các hàng sẵn có mà không khóa update[^at-notvalid]; `VALIDATE CONSTRAINT` lấy `SHARE UPDATE EXCLUSIVE`[^at-validate]. `ADD FOREIGN KEY` lấy `SHARE ROW EXCLUSIVE` (cả trên bảng được tham chiếu)[^at-fk]. `SET NOT NULL` thường quét cả bảng, trừ khi đã có `CHECK` hợp lệ chứng minh không có `NULL` thì bỏ qua bước quét[^at-notnull].
5. *Tạo index bằng `CREATE INDEX CONCURRENTLY`:* không khóa chặn insert/update/delete, trong khi index thường khóa chặn ghi (không chặn đọc) tới khi xong[^ci-conc]; đổi lại phải quét bảng hai lần và chờ mọi transaction hiện có mà có thể sửa hoặc dùng index đó kết thúc, nên tốn tổng công việc hơn và lâu hơn (suy luận của người soạn từ hai điều đó)[^ci-cost]; nếu thất bại (deadlock, vi phạm unique…) nó để lại một index "invalid" phải dọn tay[^ci-invalid].
6. *Trước khi chạy:* xem `pg_stat_activity` tìm transaction mở lâu có thể chặn migration (mục 8).

**6e. Deadlock: phát hiện thế nào.** Lý thuyết deadlock (bốn điều kiện Coffman, phòng tránh) ở [P4 · Chương 05, mục 3–5](../04-concurrency/05-concurrency-bug-va-deadlock.md), deadlock ở tầng database ở [mục 7 chương đó](../04-concurrency/05-concurrency-bug-va-deadlock.md), lỗi `40P01` và retry ở [P12 · Chương 07, mục 8](07-transaction.md). Ở đây chỉ phần *cài đặt* của PostgreSQL:

- *Deadlock xảy ra cả khi không dùng lock tường minh*, vì lock hàng đủ để tạo vòng chờ — ví dụ hai transaction `UPDATE` hai hàng theo thứ tự ngược nhau[^dl-row]. Việc PostgreSQL tự phát hiện, hủy một transaction (khó dự đoán là transaction nào) và việc ứng dụng phải retry đã ở [P4 · Chương 05, mục 5 và 7](../04-concurrency/05-concurrency-bug-va-deadlock.md); chi tiết cài đặt ở dưới.
- *Chờ lạc quan (optimistic waiting).* README: nếu không lấy được lock ngay thì process đi ngủ *không kiểm tra deadlock*, nhưng đặt đồng hồ `DeadlockTimeout` ms (thường một giây); hết hạn mà chưa được cấp lock thì mới chạy mã phát hiện/phá deadlock[^lock-optimistic]. Lý do (kiểm tra deadlock đắt, deadlock hiếm) và mặc định 1 giây đã ở [P4 · Chương 05, mục 5](../04-concurrency/05-concurrency-bug-va-deadlock.md); docs nói thêm 1 giây "có lẽ là giá trị nhỏ nhất bạn muốn trong thực tế"[^dl-timeout-def].
- *Thuật toán.* Các process là các đỉnh của đồ thị chờ (waits-for graph)[^lock-wfg]; chu trình dẫn về chính process đang kiểm tra thì là deadlock và thường được giải bằng cách hủy transaction của chính process phát hiện[^lock-resolve]. PostgreSQL còn có cạnh "mềm" (soft edge) — chờ vì xếp sau trong hàng đợi — và đôi khi *sắp xếp lại hàng đợi* thay vì hủy transaction[^lock-soft].
- *Phòng tránh.* Thứ tự lock nhất quán và retry đã ở [P4 · Chương 05, mục 5 và 7](../04-concurrency/05-concurrency-bug-va-deadlock.md); docs PostgreSQL thêm lời khuyên lấy luôn lock mạnh nhất cần dùng ngay lần đầu[^dl-order].
- *Quan sát.* `log_lock_waits` (mặc định tắt) ghi log khi một session chờ lock quá `deadlock_timeout`[^log-lock-waits]; `pg_stat_database.deadlocks` đếm deadlock phát hiện được[^db-deadlocks]. Nhận xét của người soạn: hạ `deadlock_timeout` chỉ khi đang điều tra, vì docs nói khi `log_lock_waits` bật thì giá trị này cũng quyết định thời gian chờ trước khi ghi log[^dl-logwait].

**6f. Các timeout liên quan.** Tổng hợp (mặc định theo docs 16):

| Tham số | Đo cái gì | Mặc định |
|---|---|---|
| `statement_timeout` | từ lúc lệnh tới server đến lúc xong; với giao thức extended, tính từ khi message Parse/Bind/Execute/Describe đầu tiên tới đến khi `Execute`/`Sync` hoàn tất[^st-def] | 0 (tắt)[^st-default] |
| `lock_timeout` | chỉ thời gian *chờ lock*, mỗi lần xin lock[^lt-def] | 0 (tắt)[^lt-default] |
| `idle_in_transaction_session_timeout` | session idle *trong transaction* quá hạn thì bị ngắt; để tránh giữ lock quá lâu[^iit-def] | 0 (tắt)[^iit-def] |
| `idle_session_timeout` | session idle *ngoài transaction*; docs cảnh báo cẩn thận khi áp dụng với connection qua pooler vì pooler có thể không chịu được connection bị đóng bất ngờ[^idle-sess-warn] | 0 (tắt)[^idle-sess-def] |
| `deadlock_timeout` | chờ lock bao lâu thì mới kiểm tra deadlock | 1 giây[^dl-timeout-def] |

<details><summary>Đi sâu: <code>max_locks_per_transaction</code>, page lock, fast-path và lock bảng partition</summary>

- *Bảng lock dùng chung có kích thước cố định:* chứa `max_locks_per_transaction` đối tượng (ví dụ bảng) cho mỗi server process hoặc prepared transaction; đây là *số đối tượng khác nhau* bị khóa cùng lúc, không phải số hàng[^max-locks]. Mặc định 64; docs nói có thể phải nâng nếu truy vấn chạm nhiều bảng trong một transaction, ví dụ truy vấn bảng cha có nhiều bảng con[^max-locks-def]. Advisory lock cũng dùng cùng vùng bộ nhớ này, và "cạn" thì server không cấp thêm được lock nào[^adv-pool].
- *Lock mức page* chỉ để điều khiển truy cập đọc/ghi vào page trong buffer pool và nhả ngay sau khi lấy/sửa xong một hàng; docs nói nhà phát triển ứng dụng thường không cần quan tâm[^lk-page]. Còn *lightweight lock* (LWLock) bảo vệ cấu trúc trong shared memory và hiện ở wait event kiểu `LWLock`[^we-lwlock].
- *Prepared transaction* không bao giờ ở trạng thái chờ lock nhưng vẫn giữ các lock đã lấy khi còn chạy[^pglocks-prep]; vì vậy một prepared transaction bị bỏ quên là nguồn giữ lock (và giữ horizon của VACUUM — mục 5) khó thấy, và `pid` trong `pg_locks` của lock do nó giữ là null[^pglocks-pid].

</details>

**Quay lại Q6:** lock bảng có tám chế độ phân biệt nhau chỉ bằng ma trận xung đột; hàng đợi lock khiến một yêu cầu `ACCESS EXCLUSIVE` đang chờ chặn cả những truy vấn đến sau; vì vậy migration cần `lock_timeout` ngắn, chọn lệnh ít lock, và chạy khi không có transaction dài; deadlock được phát hiện lạc quan sau `deadlock_timeout` rồi hủy một transaction.

## 7. Planner: đọc `EXPLAIN`, cost, scan, join và statistics (Q7)

> **Trả lời ngắn.** Với mỗi câu SQL, planner liệt kê nhiều cách thực thi cho cùng một kết quả, ước lượng chi phí của từng cách dựa trên *statistics* về dữ liệu và các hằng số chi phí (như `seq_page_cost`, `random_page_cost`)[^cost-unit][^stats-approx], rồi — nếu tính được trong thời gian hợp lý — chọn cách được kỳ vọng chạy nhanh nhất[^planner-intro]. `EXPLAIN` in cây plan đã chọn kèm con số *ước lượng*; `EXPLAIN ANALYZE` chạy thật câu lệnh và thêm số liệu *thực tế*[^explain-analyze]. Kỹ năng quan trọng nhất là so số dòng ước lượng với số dòng thực tế ở từng node[^actual-ms] — lệch lớn thường nghĩa là statistics sai hoặc planner thiếu thông tin, và đó là chỗ plan tệ bắt đầu.

**Giải thích.**

Hình dung (nhận xét của người soạn): bạn hỏi bản đồ chỉ đường từ A đến B. Bản đồ không đo đường thật mà *ước lượng* thời gian dựa trên dữ liệu giao thông đã thu thập (statistics). Nếu dữ liệu giao thông cũ hoặc sai (ví dụ không biết hai con đường luôn kẹt cùng lúc), nó vẫn tự tin chỉ một tuyến tệ. `EXPLAIN` là xem bản đồ chọn tuyến nào; `EXPLAIN ANALYZE` là đi thử rồi xem thời gian thật có khớp dự đoán không.

**7a. Cây plan và con số trong ngoặc.** Plan là một *cây các node*; node ở đáy là node scan trả về hàng thô từ bảng; trên đó là node join, aggregate, sort…[^explain-tree] Mỗi node hiển thị `(cost=A..B rows=N width=W)`: A là chi phí khởi động (thời gian trước khi node bắt đầu xuất hàng, ví dụ sắp xếp trong node Sort), B là chi phí tổng giả định chạy hết, `rows` là số hàng ước lượng *node xuất ra* và `width` là độ rộng trung bình (byte)[^explain-numbers][^explain-numbers2]. Chi phí đo bằng *đơn vị tùy ý* quy ước theo số lần đọc page đĩa, trong đó `seq_page_cost = 1.0` là điểm quy chiếu[^cost-unit]; chi phí của node cha *bao gồm* chi phí các node con, và không tính thời gian gửi kết quả về client[^cost-child]. `rows` không phải số hàng node phải quét mà là số hàng nó xuất ra sau khi lọc[^explain-rows]. Với seq scan, docs cho công thức: (số page đọc × `seq_page_cost`) + (số hàng quét × `cpu_tuple_cost`); ví dụ trong docs có 358 page và 10000 hàng nên chi phí là 358 × 1,0 + 10000 × 0,01 = 458[^cost-formula].

Ví dụ dưới đây *trích từ docs* PostgreSQL 16 (mục 14.1; docs ghi các ví dụ lấy từ CSDL regression sau `VACUUM ANALYZE` bằng mã nguồn 9.3 development nên số trên máy bạn sẽ khác[^explain-9-3]):

```text
EXPLAIN SELECT * FROM tenk1 WHERE unique1 < 100;
 Bitmap Heap Scan on tenk1  (cost=5.07..229.20 rows=101 width=244)
   Recheck Cond: (unique1 < 100)
   ->  Bitmap Index Scan on tenk1_unique1  (cost=0.00..5.04 rows=101 width=0)
         Index Cond: (unique1 < 100)
```

Cách đọc từ *dưới lên*: node thụt vào sâu nhất chạy trước và cung cấp hàng cho node cha (nhận xét của người soạn; docs mô tả cấu trúc cây qua thụt lề[^explain-indent]). Ở đây: `Bitmap Index Scan` tìm vị trí các hàng thỏa `unique1 < 100` trong index, `Bitmap Heap Scan` ở trên mới đi lấy các hàng từ bảng.

**7b. Các kiểu scan.** Planner luôn tạo plan seq scan, và tạo thêm plan dùng index nếu có index khớp điều kiện[^plan-seq].

- *Seq scan*: đọc toàn bộ bảng. Rẻ khi đọc phần lớn bảng, hoặc bảng nhỏ — docs: bảng chỉ chiếm một page thì gần như luôn chọn seq scan dù có index, vì phải đọc một page dù sao[^toy-seq].
- *Index scan*: lấy hàng *theo thứ tự index*, mỗi hàng là một lần đọc page rời nên đắt mỗi hàng; hợp khi chỉ vài hàng, hoặc khi `ORDER BY` khớp thứ tự index nên không cần bước sort[^indexscan-use].
- *Bitmap scan* (hai tầng): tầng dưới (`Bitmap Index Scan`) tìm vị trí hàng khớp, tầng trên (`Bitmap Heap Scan`) sắp vị trí theo thứ tự vật lý rồi mới đọc để giảm chi phí đọc rời[^bitmap-why]; docs nói đọc rời đắt hơn đọc tuần tự nhiều nhưng vì không phải đọc hết page nên vẫn rẻ hơn seq scan[^bitmap-cheaper]. Bitmap cũng cho phép *kết hợp nhiều index* bằng AND/OR; đổi lại thứ tự gốc của index bị mất nên có thể cần thêm bước sort nếu có `ORDER BY`[^bitmap-combine].
- *Index-only scan*: trả lời chỉ bằng index mà khỏi ghé heap *nếu* visibility map cho biết mọi tuple trên page đều thấy được[^vm-only]. Vì vậy bảng chưa được vacuum thì index-only scan mất lợi thế (suy luận của người soạn từ câu trên và mục 5).

Chọn kiểu nào do *chi phí*: `random_page_cost` mặc định 4,0 so với `seq_page_cost` 1,0[^rpc-def][^spc-def]. Docs nói truy cập ngẫu nhiên tới bộ nhớ bền thường đắt hơn nhiều so với bốn lần truy cập tuần tự, nhưng giá trị mặc định thấp hơn vì phần lớn truy cập ngẫu nhiên như đọc qua index được *giả định là đã nằm trong cache*[^rpc-why]; và `effective_cache_size` cũng đi vào ước lượng chi phí index scan (mục 3d)[^ecs]. Docs đưa hướng dẫn điều chỉnh: nếu dữ liệu nhiều khả năng nằm trọn trong cache thì hạ `random_page_cost` có thể hợp lý[^rpc-tip]. (Nhận xét của người soạn: con số đúng phải rút ra từ workload và phần cứng của bạn, không từ lời khuyên chung.)

**7c. Ba kiểu join.** Planner xét ba chiến lược join[^join-3], tóm tắt như sau.

| Kiểu | Cách chạy (theo docs) | Hợp khi (nhận xét của người soạn, nối từ ví dụ docs) |
|---|---|---|
| *nested loop* | quét bên phải một lần cho *mỗi* hàng bên trái; đơn giản nhưng có thể rất tốn[^join-nl] | số hàng bên ngoài ít, và bên trong có index để mỗi lần lặp rẻ[^join-nl-index] |
| *hash join* | nạp bên phải vào hash table theo khóa join, rồi quét bên trái và tra hash table[^join-hash] | không có thứ tự sẵn; cần đủ bộ nhớ (`work_mem × hash_mem_multiplier`), nếu không chia batch ra đĩa (mục 3b)[^hash-mem] |
| *merge join* | sort hai bên theo khóa join rồi quét song song, ghép hàng khớp; mỗi bên chỉ quét một lần[^join-merge] | đầu vào đã sắp (qua index) hoặc sort rẻ; docs lưu ý merge join *đòi* input được sắp theo khóa join[^explain-merge] |

Với truy vấn nhiều bảng, planner phải chọn *thứ tự join*: dưới ngưỡng `geqo_threshold` nó tìm gần như vét cạn[^geqo]; từ ngưỡng đó trở lên (đếm theo số mục `FROM`; mặc định 12) thì dùng heuristic của GEQO (genetic) để có kết quả "hợp lý" thay vì tối ưu trong thời gian hợp lý[^geqo-thr].

**7d. `EXPLAIN ANALYZE`: đọc số thực tế.** Khác với `EXPLAIN` thường, `ANALYZE` *thực sự chạy* câu lệnh rồi hiển thị số hàng và thời gian thật của từng node cùng các ước lượng[^explain-analyze]. Vì chạy thật, mọi tác dụng phụ vẫn xảy ra; nếu muốn phân tích câu lệnh ghi dữ liệu mà không đổi bảng, docs gợi ý bọc trong `BEGIN … ROLLBACK`[^explain-sideeffect]. (Nhận xét của người soạn: trên primary đang chạy thật, câu `UPDATE` bị `EXPLAIN ANALYZE` vẫn lấy lock và sinh WAL như thường — mục 4 và 6 — nên đừng chạy tùy tiện trên bảng nóng.)

Các điểm cần soi, theo thứ tự (nhận xét của người soạn, mỗi điểm kèm nguồn):

1. *Ước lượng so với thực tế:* `actual time` tính bằng mili-giây thật, còn `cost` là đơn vị tùy ý nên không so được; thứ quan trọng nhất cần nhìn là số hàng ước lượng có gần thực tế không[^actual-ms].
2. *`loops`:* với node chạy nhiều lần (ví dụ phía trong nested loop), `actual time` và `rows` là *trung bình mỗi lần*; phải nhân với `loops` để ra tổng[^loops].
3. *`Rows Removed by Filter`:* số hàng bị lọc bỏ; chỉ hiện khi có ít nhất một hàng bị loại[^rows-removed]. Nhiều hàng bị loại ở node scan nghĩa là scan đã đọc nhiều hơn cần (có thể thiếu index phù hợp — suy luận của người soạn).
4. *`Sort Method`, `Hash` batches:* cho biết sort có tràn ra đĩa không, hash có chia batch không[^explain-sort] (liên hệ `work_mem`, mục 3b).
5. *`BUFFERS`* (`EXPLAIN (ANALYZE, BUFFERS)`): giúp xác định phần nào tốn I/O nhất[^explain-buffers].
6. *`Planning time` và `Execution time`:* thời gian lập plan không gồm parse/rewrite; thời gian thực thi gồm khởi động/kết thúc executor và trigger nhưng không gồm parse/rewrite/plan[^explain-times].

Những chỗ `EXPLAIN ANALYZE` dễ gây hiểu lầm (docs): thời gian đo không gồm chi phí gửi dòng kết quả tới client, và bản thân việc đo có thể tốn đáng kể trên máy có `gettimeofday()` chậm[^explain-caveat]; kết quả trên bảng nhỏ không ngoại suy được sang bảng lớn[^explain-toy]; số ước lượng và thực tế có thể lệch mà chẳng có gì sai, ví dụ khi node bị dừng sớm bởi `LIMIT`[^explain-limit].

**7e. Statistics: nguyên liệu của ước lượng.** Planner dùng hai nhóm thông tin: (1) tổng số hàng và số page mỗi bảng/index trong `pg_class.reltuples`/`relpages` — không cập nhật tức thời nên thường hơi cũ; do VACUUM, ANALYZE và vài lệnh DDL như `CREATE INDEX` cập nhật, và planner co giãn theo kích thước vật lý hiện tại của bảng[^stats-pgclass]; (2) *selectivity* của điều kiện `WHERE` (tỷ lệ hàng khớp) từ `pg_statistic` (xem qua view `pg_stats`), do ANALYZE cập nhật và *luôn chỉ gần đúng*, kể cả vừa mới cập nhật[^stats-approx]. ANALYZE lấy *mẫu ngẫu nhiên* các hàng chứ không đọc hết[^analyze-sample]; độ chi tiết (số phần tử tối đa của danh sách giá trị phổ biến và histogram) theo `default_statistics_target`, mặc định 100, đổi theo từng cột bằng `ALTER TABLE … SET STATISTICS`; nâng lên có thể cho ước lượng chính xác hơn với phân phối bất thường, đổi lại tốn thêm chỗ chứa và thời gian[^stats-target]. Autovacuum chạy ANALYZE theo công thức ngưỡng ở mục 5b, và *không* chạy cho bảng partitioned cha[^av-partitioned] hay foreign table[^av-foreign].

*Cột tương quan.* Mặc định planner coi các điều kiện trên nhiều cột là *độc lập*; giả định này sai khi giá trị các cột tương quan, và statistics theo từng cột không nắm được tương quan chéo[^ext-indep]. Giải pháp của PostgreSQL là *extended statistics* (`CREATE STATISTICS`): vì số tổ hợp cột quá nhiều nên không thể tính tự động, người dùng tạo *statistics object* cho các tập cột đáng quan tâm[^ext-create]; việc tạo chỉ ghi một mục trong catalog, dữ liệu thật được ANALYZE thu thập[^ext-analyze]. Các loại được hỗ trợ: `ndistinct` (thống kê số giá trị phân biệt theo nhóm cột), `dependencies` (phụ thuộc hàm) và `mcv` (danh sách giá trị phổ biến nhiều cột)[^ext-kinds]; docs khuyên chỉ tạo cho nhóm cột thật sự tương quan, tránh tốn công ANALYZE và lập plan[^ext-advice]. Với biểu thức/hàm: nếu bạn tạo statistics object hoặc *expression index* dùng lời gọi hàm thì statistics về hàm sẽ được thu thập, cải thiện plan dùng expression index[^func-stats].

*Prepared statement và plan.* Prepared statement có tham số có thể chạy bằng *generic plan* (giống nhau cho mọi lần chạy, tiết kiệm thời gian lập plan) hoặc *custom plan* (lập cho giá trị tham số cụ thể)[^prep-plan]. Mặc định (`plan_cache_mode = auto`) server chạy năm lần đầu bằng custom plan, tính chi phí trung bình, rồi so với chi phí generic plan để quyết định[^prep-auto]. (Liên hệ Rust: `sqlx`/`tokio-postgres` dùng prepared statement ngầm; xem mục "Liên hệ với Rust".)

**7f. Quy trình chẩn đoán gợi ý** (nhận xét của người soạn):

```mermaid
flowchart TB
  S["Truy vấn chậm"] --> E["EXPLAIN (ANALYZE, BUFFERS)<br/>trong BEGIN … ROLLBACK nếu là ghi"]
  E --> F["Tìm node đầu tiên (từ dưới lên)<br/>mà rows ước lượng lệch xa thực tế"]
  F --> A["Stats cũ?<br/>chạy ANALYZE, xem last_autoanalyze"]
  F --> B["Cột tương quan?<br/>CREATE STATISTICS"]
  F --> C["Phân phối lệch/khó?<br/>nâng statistics target cột đó"]
  E --> G["Số ước lượng đúng nhưng plan vẫn chậm"]
  G --> H["Sort/Hash tràn đĩa?<br/>xem work_mem, hash_mem_multiplier"]
  G --> I["Cost constants lệch?<br/>random_page_cost, effective_cache_size"]
  G --> J["Thiếu index phù hợp?<br/>nhiều 'Rows Removed by Filter'"]
```

*Công cụ thô nhưng hữu ích:* các cờ `enable_seqscan`, `enable_nestloop`, `enable_sort`… "làm planner không thích" kiểu scan/join đó chứ không cấm hẳn (không thể loại bỏ hoàn toàn seq scan hay sort tường minh)[^enable-flags]; docs gọi đây là công cụ thô nhưng hữu ích để xem plan biến thể[^enable-crude]. Dùng để *thử nghiệm trong session*, không đặt toàn cục (nhận xét của người soạn).

<details><summary>Đi sâu: ví dụ <code>EXPLAIN ANALYZE</code> của docs và cách đọc <code>loops</code></summary>

Ví dụ *trích từ docs* PostgreSQL 16 (mục 14.1.2; số liệu là của docs, không phải do người soạn chạy):

```text
EXPLAIN ANALYZE SELECT *
FROM tenk1 t1, tenk2 t2
WHERE t1.unique1 < 10 AND t1.unique2 = t2.unique2;

 Nested Loop  (cost=4.65..118.62 rows=10 width=488) (actual time=0.128..0.377 rows=10 loops=1)
   ->  Bitmap Heap Scan on tenk1 t1  (cost=4.36..39.47 rows=10 width=244) (actual time=0.057..0.121 rows=10 loops=1)
         Recheck Cond: (unique1 < 10)
         ->  Bitmap Index Scan on tenk1_unique1  (cost=0.00..4.36 rows=10 width=0) (actual time=0.024..0.024 rows=10 loops=1)
               Index Cond: (unique1 < 10)
   ->  Index Scan using tenk2_unique2 on tenk2 t2  (cost=0.29..7.91 rows=1 width=244) (actual time=0.021..0.022 rows=1 loops=10)
         Index Cond: (unique2 = t1.unique2)
 Planning time: 0.181 ms
 Execution time: 0.501 ms
```

Lưu ý nhãn: ví dụ giữ nguyên định dạng của docs (`Planning time`, `Execution time`), còn mã nguồn `explain.c` ở `REL_16_4` in các nhãn này là `Planning Time` và `Execution Time` viết hoa[^explain-label][^explain-label2] — dùng chữ hoa khi grep output PG16 thật.

Đọc: node `Index Scan` ở dưới cùng chạy `loops=10` lần (một lần cho mỗi trong 10 hàng bên ngoài); mỗi lần trung bình `0.022` ms, nên tổng khoảng 0,220 ms — docs tự tính con số này: "tổng cộng 0,220 mili-giây cho các index scan trên tenk2"[^loops]. Ước lượng `rows=10` khớp `rows=10` thực tế ở node cha — docs nhận xét ở ví dụ này mọi ước lượng đều "chính xác tuyệt đối", nhưng điều đó hiếm trong thực tế[^actual-ms].

*Các node chỉ hiện ở `ANALYZE`:* `Sort Method: quicksort Memory: 77kB` và `Buckets: 1024 Batches: 1 Memory Usage: 28kB` ở node Hash trong ví dụ khác của docs; `BitmapAnd`/`BitmapOr` luôn báo số hàng thực tế bằng 0 do giới hạn cài đặt, nên đừng coi đó là lỗi[^explain-bitmapzero].

</details>

**Quay lại Q7:** planner liệt kê plan, ước lượng chi phí từ statistics rồi chọn rẻ nhất; đọc `EXPLAIN ANALYZE` là so số hàng ước lượng với thực tế từ dưới lên và tìm node đầu tiên lệch; plan tệ phần lớn xuất phát từ statistics cũ hoặc thiếu, cột tương quan, hoặc hằng số chi phí không hợp phần cứng.

## 8. Quan sát PostgreSQL đang chạy: `pg_stat_activity`, wait event, `pg_locks`, `pg_stat_statements` (Q8, phần 1)

> **Trả lời ngắn.** Bốn nguồn đáng nhìn đầu tiên: `pg_stat_activity` (mỗi process đang làm gì, ở trạng thái nào, chờ gì)[^psa-row]; *wait event* (cột `wait_event_type`/`wait_event` của chính view đó, cho biết process đang chờ loại tài nguyên nào)[^psa-wait]; `pg_locks` (ai giữ, ai chờ lock nào) — kèm hàm `pg_blocking_pids()` để tìm "ai chặn ai"[^pglocks-blocking]; và extension `pg_stat_statements` (thống kê *cộng dồn theo từng dạng câu lệnh*: số lần chạy, tổng thời gian…)[^pgss-intro]. Phần "những cấu hình nào hay gây sự cố" nằm ở mục 9.

**Giải thích.**

Hình dung (nhận xét của người soạn): `pg_stat_activity` là ảnh chụp "bây giờ mỗi nhân viên đang làm gì", `pg_locks` là bảng ai đang đứng chờ ai, còn `pg_stat_statements` là sổ thống kê cuối ngày "việc nào tốn tổng thời gian nhiều nhất". Ảnh chụp tức thời trả lời "đang kẹt ở đâu", sổ thống kê trả lời "nên tối ưu cái gì".

Các câu SQL bên dưới chỉ để minh họa cách ghép (chưa chạy); tên cột lấy từ docs PostgreSQL 16.

**8a. `pg_stat_activity`: một dòng mỗi process.** View có một dòng cho mỗi server process, cho biết trạng thái và truy vấn hiện tại[^psa-row]. Các cột đáng nhớ:

| Cột | Ý nghĩa (docs) |
|---|---|
| `pid` | PID của process OS (mục 1)[^pid-ps] |
| `backend_type` | `client backend`, `autovacuum worker`, `checkpointer`, `walwriter`…[^backend-types] |
| `state` | `active` (đang chạy query), `idle` (chờ lệnh), `idle in transaction` (trong transaction nhưng không đang chạy query), `idle in transaction (aborted)` (như trên nhưng một câu lệnh đã lỗi)[^psa-state] |
| `xact_start` | lúc transaction hiện tại bắt đầu, null nếu không có transaction; nếu query hiện tại là câu đầu của transaction thì bằng `query_start`[^psa-xact] |
| `backend_xid`, `backend_xmin` | XID của transaction cấp cao nhất (top-level) của backend nếu có[^bxid], và *xmin horizon* của nó (mục 5c)[^bxmin] |
| `wait_event_type`, `wait_event` | chờ cái gì, null nếu không chờ[^psa-wait] |
| `query_id` | định danh câu lệnh gần nhất; null nếu không bật `compute_query_id` hoặc module bên thứ ba[^psa-queryid] |

Câu hỏi hay hỏi nhất: *transaction nào mở lâu?* (mục 5, 6).

```sql
-- Minh họa, chưa chạy: transaction mở lâu nhất, kèm horizon mà nó giữ
SELECT pid, usename, state,
       now() - xact_start AS xact_age,
       backend_xmin, wait_event_type, wait_event,
       left(query, 100) AS last_query
FROM pg_stat_activity
WHERE xact_start IS NOT NULL
ORDER BY xact_start
LIMIT 20;
```

Kết quả cần đọc (nhận xét của người soạn): dòng có `state = 'idle in transaction'` và `xact_age` lớn là ứng viên số một chặn VACUUM (mục 5c) và giữ lock (mục 6); `query` ở đó là câu *cuối cùng* nó chạy, không phải thứ nó đang làm — vì nó không đang làm gì cả.

Muốn dừng: `pg_cancel_backend(pid)` gửi `SIGINT` (hủy câu lệnh hiện tại) còn `pg_terminate_backend(pid)` gửi `SIGTERM` (kết thúc cả session)[^cancel-term]. (Mục 6 và mục 9 nói khi nào nên đặt timeout tự động thay vì dọn tay.)

**8b. Wait event: process đang chờ gì.** `wait_event_type` cho biết *loại* thứ đang chờ, `wait_event` là tên điểm chờ cụ thể; cả hai là null nếu không chờ gì[^psa-wait]. Các loại, theo docs:

- `Activity`: process rảnh, chờ việc ở vòng lặp chính của nó[^we-activity].
- `BufferPin`: chờ quyền độc quyền trên một buffer dữ liệu; docs nói việc chờ này có thể kéo dài nếu process khác giữ một cursor mở đã đọc buffer đó[^we-type-bufferpin].
- `Client`: chờ hoạt động từ phía socket nối với ứng dụng, tức chờ thứ không phụ thuộc process nội bộ[^we-type-client].
- `Extension`: chờ điều kiện do một extension định nghĩa[^we-extension].
- `IO`: chờ một thao tác I/O hoàn tất[^we-io].
- `IPC`: chờ tương tác với process server khác[^we-ipc].
- `Lock`: chờ lock "nặng" (heavyweight, còn gọi lock manager lock hay đơn giản là lock), vốn chủ yếu bảo vệ đối tượng nhìn thấy được từ SQL như bảng[^we-type-lock].
- `LWLock`: chờ lock nhẹ, thường bảo vệ một cấu trúc dữ liệu trong shared memory[^we-lwlock].
- `Timeout`: chờ một timeout hết hạn[^we-timeout].

Docs liệt kê từng `wait_event` cụ thể trong các bảng riêng. Một số hay gặp:

| `wait_event_type` / `wait_event` | Docs mô tả | Gợi ý đi tiếp (nhận xét của người soạn) |
|---|---|---|
| `Lock` / `relation` | chờ lock trên một relation[^we-relation] | `pg_blocking_pids`, xem mục 6c |
| `Lock` / `transactionid` | chờ một transaction kết thúc[^we-xid] | thường là chờ lock hàng; tìm transaction giữ |
| `Client` / `ClientRead` | chờ đọc dữ liệu từ client[^we-clientread] | với `state = idle in transaction`: ứng dụng đang giữ transaction mà không gửi lệnh |
| `IO` / `DataFileRead` | chờ đọc một file dữ liệu của relation[^we-datafileread] | cache miss; xem `shared_buffers`, `EXPLAIN (BUFFERS)` (mục 3, 7) |
| `IO` / `WALWrite`, `WALSync` | chờ ghi / chờ đạt bộ nhớ bền cho file WAL[^we-walwrite][^we-walsync] | độ trễ commit; xem mục 4 và P5·06 |
| `BufferPin` | chờ quyền độc quyền (exclusive pin) trên buffer[^we-bufferpin] | xem cursor mở lâu trong `pg_stat_activity` (suy luận từ mô tả của loại `BufferPin` ở trên) |
| `LWLock` | chờ lock nhẹ bảo vệ cấu trúc trong shared memory[^we-lwlock] | tranh chấp nội bộ; xem tên cụ thể (ví dụ `LockManager`: chờ đọc/cập nhật thông tin lock nặng[^we-lockmanager]) |

Cách dùng (nhận xét của người soạn): `wait_event` là *ảnh chụp tức thời*; để thấy xu hướng cần lấy mẫu lặp lại nhiều lần (script chạy mỗi giây chẳng hạn) và đếm theo `wait_event`. Một process `active` mà `wait_event` null thì nó đang không chờ cái gì docs liệt kê (suy luận của người soạn: thường nghĩa là đang chạy CPU).

Docs đưa sẵn một truy vấn mẫu để xem wait event[^we-example], như sau.

```sql
SELECT pid, wait_event_type, wait_event FROM pg_stat_activity WHERE wait_event is NOT NULL;
```

**8c. `pg_locks` và `pg_blocking_pids()`.** `pg_locks` có một dòng cho mỗi (đối tượng khóa, chế độ, process) — đối tượng khóa có thể là cả relation, một page, một tuple, một transaction ID, hoặc đối tượng database, hay advisory lock[^pglocks-objs]. Cột `granted` = true là đang giữ, false là đang chờ; cột `pid` nối được sang `pg_stat_activity`[^pglocks-granted][^pglocks-join]. Nhưng docs nói thẳng: tuy có thể tự join `pg_locks` với chính nó để biết ai chặn ai, làm đúng rất khó — phải mã hóa kiến thức về chế độ nào xung đột chế độ nào, và view không cho biết process nào đứng trước process nào trong hàng đợi lock, cũng không cho biết parallel worker thuộc session nào; *nên dùng `pg_blocking_pids()`*[^pglocks-blocking]. Hàm này trả mảng PID đang chặn process đã cho, tính cả hard block lẫn soft block[^blockpids].

```sql
-- Minh họa, chưa chạy: ai đang bị chặn và bởi ai
SELECT pid,
       pg_blocking_pids(pid) AS blocked_by,
       wait_event_type, wait_event,
       now() - query_start AS waiting_for,
       left(query, 100) AS query
FROM pg_stat_activity
WHERE cardinality(pg_blocking_pids(pid)) > 0;
```

Lần ngược chuỗi `blocked_by` đến một PID *không* nằm trong danh sách bị chặn — đó là "đầu chuỗi" (nhận xét của người soạn). Đầu chuỗi hay là một session `idle in transaction`, hoặc một `ALTER TABLE` (mục 6c).

**8d. `pg_stat_statements`: thống kê theo dạng câu lệnh.** Module theo dõi thống kê *lập plan và thực thi* của mọi câu SQL trong server[^pgss-intro]. Để bật: thêm `pg_stat_statements` vào `shared_preload_libraries` (cần restart vì module cần thêm shared memory), bật tính query identifier (tự động nếu `compute_query_id` là `auto` hoặc `on`), rồi `CREATE EXTENSION pg_stat_statements` ở database cần xem[^pgss-setup][^pgss-ext]. View có một dòng cho mỗi tổ hợp (database, user, query ID, top-level hay không), tối đa số câu lệnh mà module theo dõi được[^pgss-row]. Các câu chỉ khác nhau ở hằng số được gộp vào một dòng và hằng số được thay bằng `$1`…[^pgss-normalize] — nên các lần chạy cùng một câu có tham số `$1` (như qua sqlx/tokio-postgres) với giá trị khác nhau dồn vào một dòng. Docs không nói câu viết sẵn literal và câu tương đương viết bằng `$1` có chung `queryid` hay không, nên đừng giả định chúng gộp chung (nhận xét của người soạn).

Các cột hay dùng: `calls`, `total_exec_time`, `mean_exec_time`, `rows`, `shared_blks_hit`/`shared_blks_read` (hit và read ở cache của PostgreSQL), `temp_blks_read/written`, `wal_bytes`[^pgss-cols]. Docs đưa truy vấn mẫu sắp theo `total_exec_time`[^pgss-sample]; bản dưới viết lại theo mẫu đó.

```sql
-- Minh họa, theo mẫu trong docs: câu nào tốn tổng thời gian thực thi nhiều nhất
SELECT query, calls, total_exec_time, rows,
       100.0 * shared_blks_hit / nullif(shared_blks_hit + shared_blks_read, 0) AS hit_percent
FROM pg_stat_statements
ORDER BY total_exec_time DESC
LIMIT 5;
```

Lưu ý do docs nêu: (1) chỉ superuser và role có đặc quyền `pg_read_all_stats` thấy được text và `queryid` của câu lệnh người dùng khác[^pgss-privacy]; (2) thống kê *thời gian lập plan* chỉ có khi bật `pg_stat_statements.track_planning`, mặc định tắt vì có thể gây giảm hiệu năng đáng chú ý khi nhiều connection cùng cập nhật ít entry[^pgss-planning]; (3) `queryid` chỉ có bảo đảm ổn định hạn chế: vì tính từ cây sau parse-analysis nên nó phụ thuộc cả vào OID đối tượng — hai câu giống hệt nhau nhưng tham chiếu một bảng đã bị drop rồi tạo lại sẽ bị coi là khác nhau[^pgss-queryid-stability]; (4) khi số câu phân biệt vượt `pg_stat_statements.max` (mặc định 5000) thì thông tin câu ít chạy nhất bị loại, số lần loại xem ở `pg_stat_statements_info`[^pgss-max].

**8e. Các view thống kê khác và log.**

- *Bảng:* `pg_stat_all_tables` (và `pg_stat_user_tables`, cùng thông tin nhưng chỉ bảng người dùng[^pgstat-user]) có `n_dead_tup` (ước lượng số dòng chết)[^pgstat-dead], `n_tup_hot_upd` (số hàng HOT update — các update không cần phiên bản kế tiếp trong index)[^pgstat-hot], `last_autovacuum` (lần cuối autovacuum dọn bảng này)[^pgstat-lastav]. Dùng để kiểm tra tại sao bảng phình (mục 5) và update có HOT không (mục 4).
- *Database:* `pg_stat_database` có `deadlocks`[^db-deadlocks], `temp_files`[^db-temp], `blks_hit` (chỉ cache PostgreSQL[^blks-hit]).
- *Checkpoint:* `checkpoints_timed` (checkpoint theo lịch) và `checkpoints_req` (checkpoint được yêu cầu) trong `pg_stat_bgwriter`[^pgstat-ckpt]; nếu checkpoint do WAL đầy xảy ra dày hơn `checkpoint_warning`, server ghi log gợi ý tăng `max_wal_size`[^ckpt-warning].
- *Tiến độ VACUUM:* `pg_stat_progress_vacuum`[^vac-progress].
- *Log:* `log_min_duration_statement` ghi thời lượng câu lệnh chạy từ ngưỡng chỉ định trở lên[^log-min-dur] (mặc định -1: tắt[^log-min-dur-def]); `log_lock_waits` (mặc định tắt)[^log-lock-waits]; `log_checkpoints` (mặc định bật)[^log-ckpt]; `log_autovacuum_min_duration` (mặc định 10 phút)[^av-log]; `log_temp_files` (mặc định tắt)[^log-temp].

**8f. Bảng chẩn đoán nhanh** (nhận xét của người soạn; mỗi mục nối về nơi giải thích):

| Triệu chứng | Nhìn đâu trước | Hiểu là gì | Mục |
|---|---|---|---|
| Mọi truy vấn tới một bảng cùng treo | `pg_stat_activity` + `pg_blocking_pids`, `wait_event_type = Lock` | có một lock mạnh đang chờ hoặc đang giữ; tìm đầu chuỗi | 6c, 8c |
| Bảng/index phình dù autovacuum chạy | `n_dead_tup`, `last_autovacuum`, `backend_xmin` cũ nhất, `pg_prepared_xacts`, `pg_replication_slots`, log VACUUM `dead but not yet removable` | có thứ giữ horizon | 5c |
| Pool ứng dụng cạn, nhiều session `idle in transaction` | `pg_stat_activity` nhóm theo `state` | ứng dụng giữ transaction qua I/O chậm | 2, 5c, 9 |
| Latency tăng theo chu kỳ vài phút | `checkpoints_req` so với `checkpoints_timed`, log checkpoint, wait event `WALWrite`/`WALSync`/`DataFileRead` | tải checkpoint hoặc fsync WAL | 4 |
| Truy vấn bỗng chậm sau thay đổi dữ liệu | `pg_stat_statements` (`mean_exec_time`), `EXPLAIN (ANALYZE, BUFFERS)`, ngày `last_autoanalyze` | statistics lệch hoặc plan đổi | 7 |
| Sort/hash chậm bất thường | `pg_stat_database.temp_files`, `log_temp_files`, `Sort Method`/`Batches` trong `EXPLAIN ANALYZE` | tràn `work_mem` | 3b |
| Lỗi xin lock / `lock_timeout` | log (với `log_lock_waits`), `pg_locks WHERE NOT granted` | xung đột lock, thường do migration | 6 |

<details><summary>Đi sâu: <code>pg_stat_statements</code> và bộ nhớ; wait event theo ngữ cảnh; đọc <code>pg_locks</code> cho lock hàng</summary>

- *Bộ nhớ của `pg_stat_statements`.* Module cần shared memory tỷ lệ với `pg_stat_statements.max`, và nó chiếm bộ nhớ này ngay khi được nạp, kể cả khi `pg_stat_statements.track` là `none`[^pgss-mem]. Text câu lệnh đại diện nằm trong file ngoài, không chiếm shared memory[^pgss-extfile]; nếu file phình quá cỡ, module có thể bỏ hết text — view sẽ cho `query` null nhưng thống kê theo `queryid` vẫn còn[^pgss-text].
- *`plans` và `calls` có thể lệch nhau* vì thống kê plan và thực thi cập nhật ở cuối mỗi pha tương ứng và chỉ cho thao tác thành công; nếu dùng cached plan thì chỉ thống kê thực thi được cập nhật[^pgss-plans-calls].
- *Lock hàng trong `pg_locks`.* Vì lock hàng nằm trên đĩa nên chờ lock hàng hiện là chờ `transactionid` của bên giữ (mục 6b); để tìm bên giữ thì `pg_blocking_pids` là lối ngắn nhất.
- *Chuỗi lock kéo dài (lock convoy)* — nhiều session xếp sau một `ACCESS EXCLUSIVE` đang chờ (mục 6c) — hiện ra trong `pg_stat_activity` thành nhiều dòng `Lock`/`relation` cùng chung một `blocked_by` (suy luận của người soạn).

</details>

**Quay lại Q8 (phần 1):** `pg_stat_activity` cho biết ai đang làm/chờ gì; `wait_event` phân loại chờ theo loại tài nguyên; `pg_blocking_pids()` thay cho việc tự join `pg_locks`; `pg_stat_statements` cho bức tranh tích lũy theo dạng câu lệnh; còn các view bảng/database/checkpoint và log hoàn thiện bức tranh.

## 9. Những cấu hình hay gây sự cố (Q8, phần 2)

> **Trả lời ngắn.** Phần lớn sự cố do cấu hình không đến từ giá trị "sai" mà từ giá trị *mặc định không hợp với quy mô của bạn*, hoặc từ *không có giới hạn nào*: mặc định `statement_timeout`, `lock_timeout` và `idle_in_transaction_session_timeout` đều là 0 (tắt), nên một câu lệnh, một lần chờ lock hay một transaction treo có thể kéo dài vô hạn[^st-default][^lt-default][^iit-def]. Các nhóm hay vỡ: số connection so với `max_connections`, `work_mem` nhân với số thao tác và session, ngưỡng autovacuum theo tỷ lệ phần trăm, kích thước WAL/checkpoint, và hành vi của OS khi hết bộ nhớ. Dưới đây là bảng *nhận xét của người soạn*; mỗi giá trị mặc định và mỗi cơ chế kèm nguồn docs, còn cột "vì sao hay vỡ" là suy luận từ các cơ chế đó.

**Giải thích.**

Hình dung (nhận xét của người soạn): chiếc xe mới chạy tốt ở số lẻ trong gara, nhưng sự cố thường đến từ những thứ "chưa ai cài giới hạn": phanh chưa chỉnh, đèn chưa bật. Các giá trị mặc định của PostgreSQL là điểm khởi đầu chứ không phải cấu hình tối ưu cho máy production của bạn (nhận xét của người soạn). Ví dụ docs nói `shared_buffers` mặc định thường là 128MB[^sb-def], còn với máy chuyên dụng có từ 1GB RAM trở lên thì điểm khởi đầu hợp lý là 25% RAM[^sb-min].

Bảng dưới không đưa "giá trị khuyến nghị": con số phù hợp phụ thuộc workload và phần cứng (nhận xét của người soạn), và chương này không có số đo nào để làm căn cứ.

**9a. Connection và bộ nhớ**

| Tham số | Mặc định (docs 16) | Vì sao hay vỡ (suy luận của người soạn) | Xem lại ở |
|---|---|---|---|
| `max_connections` | 100, đổi cần restart[^maxconn] | tổng pool của mọi pod + job + tool vượt hạn mức; còn 3 chỗ dành riêng cho superuser[^super-res] | mục 2 |
| `shared_buffers` | 128MB[^sb-def] | quá thấp so với dữ liệu; nhưng nâng lên quá nhiều (trên ~40% RAM) khó có lợi vì PostgreSQL cũng dựa vào cache OS[^sb-25] | mục 3 |
| `work_mem` | 4MB, *mỗi thao tác* sort/hash[^wm-default][^wm-multi] | tăng toàn cục vì một báo cáo chậm: cận trên bộ nhớ nhân với session × thao tác; hash còn nhân `hash_mem_multiplier`[^hash-mem-default] | mục 3b |
| `maintenance_work_mem` | 64MB; autovacuum có thể dùng tới số worker lần[^mwm][^mwm-av] | vừa quá nhỏ (VACUUM đi nhiều lượt index) vừa quá lớn (nhân với worker) | mục 3c |
| `max_locks_per_transaction` | 64[^max-locks-def] | truy vấn chạm nhiều bảng/partition trong một transaction | mục 6 |
| `max_prepared_transactions` | 0; docs khuyên để 0 nếu không dùng, tránh vô tình tạo prepared transaction[^max-prep] | nếu đặt cao, prepared transaction bị bỏ quên giữ lock và horizon của VACUUM | mục 5c |

**9b. VACUUM và XID**

| Tham số | Mặc định (docs 16) | Vì sao hay vỡ (suy luận của người soạn) | Xem lại ở |
|---|---|---|---|
| `autovacuum_vacuum_scale_factor` | 0,2 (20% bảng)[^av-scale-def] | bảng lớn phải đợi lượng dòng chết khổng lồ mới được dọn | mục 5b |
| `autovacuum_max_workers`, `autovacuum_vacuum_cost_delay` | 3 worker[^av-workers]; delay 2ms[^av-cost-def] | nhiều bảng lớn cùng đến hạn, mọi worker bận (docs cảnh báo điều này: các bảng và database khác phải chờ)[^av-busy] | mục 5b |
| `autovacuum_freeze_max_age` | 200 triệu[^freeze-max-def] | bảng lớn tĩnh bị anti-wraparound autovacuum bất ngờ, và nó không tự bị ngắt khi có lock xung đột[^av-wrap-nointr] | mục 5d |
| `idle_in_transaction_session_timeout` | 0 (tắt)[^iit-def] | connection kẹt trong transaction giữ lock và chặn dọn dòng chết[^iit] | mục 5c, 6f |
| `hot_standby_feedback` | off; bật có thể gây bloat ở primary[^hsf] | standby chạy truy vấn dài giữ horizon trên primary | mục 5c |

**9c. WAL, checkpoint, durability**

| Tham số | Mặc định (docs 16) | Vì sao hay vỡ (suy luận của người soạn) | Xem lại ở |
|---|---|---|---|
| `max_wal_size`, `checkpoint_timeout` | 1GB, 5 phút[^ckpt-defaults] | tải ghi lớn làm checkpoint do WAL đầy xảy ra dày; log `checkpoint_warning` gợi ý nâng `max_wal_size`[^ckpt-warning] | mục 4, 8e |
| WAL tích tụ trong `pg_wal` | — | docs: nếu archive WAL không theo kịp hoặc `archive_command` liên tục thất bại, WAL cũ tích lũy cho đến khi giải quyết; standby chậm/hỏng dùng replication slot cũng gây hiệu ứng tương tự[^wal-accum] — nguy cơ đầy đĩa (suy luận) | [P12 · Chương 05](05-replication.md) |
| `full_page_writes` | on[^fpw-default] | tắt để "nhanh hơn" có thể gây hỏng dữ liệu thầm lặng sau sự cố[^fpw-off] | mục 4 |
| `fsync` | (docs không nêu mặc định ở mục này; khi bật, server cố bảo đảm update được ghi thật xuống đĩa[^fsync-on]) | docs: tắt thường có lợi hiệu năng nhưng có thể gây hỏng dữ liệu không phục hồi được khi mất điện hoặc crash[^fsync-off]; chỉ chấp nhận khi dựng lại được toàn bộ dữ liệu từ nguồn ngoài | [P5 · Chương 06](../05-storage/06-durability-trong-thuc-te.md) |
| `synchronous_commit` | on[^sc-default] | đặt `off` để giảm độ trễ đồng nghĩa chấp nhận mất vài transaction vừa "commit" khi crash, không gây hỏng nhất quán[^sync-commit-off]; docs lưu ý có thể đặt riêng cho từng transaction bằng `SET LOCAL synchronous_commit TO OFF`[^sc-local], và nói tắt nó cho transaction không quan trọng thường cho phần lớn lợi ích hiệu năng của tắt `fsync` mà không có rủi ro hỏng dữ liệu[^fsync-sc] | mục 4 |

**9d. Timeout và lock**

| Tham số | Mặc định (docs 16) | Vì sao hay vỡ (suy luận của người soạn) | Xem lại ở |
|---|---|---|---|
| `statement_timeout` | 0[^st-default] | truy vấn mất kiểm soát chiếm connection mãi; nhưng đặt trong `postgresql.conf` ảnh hưởng mọi session — docs không khuyến nghị[^st-conf] | mục 6f, Rust |
| `lock_timeout` | 0[^lt-default] | migration chờ lock vô hạn và kéo cả bảng đứng (mục 6c) | mục 6c–6d |
| `deadlock_timeout` | 1s[^dl-timeout-def] | quá nhỏ làm kiểm tra deadlock tốn kém dồn dập; quá lớn làm lỗi deadlock thật được báo chậm[^dl-timeout][^dl-tradeoff] | mục 6e |

**9e. Planner**

| Tham số | Mặc định (docs 16) | Vì sao hay vỡ (suy luận của người soạn) | Xem lại ở |
|---|---|---|---|
| `random_page_cost` | 4,0[^rpc-def] | không khớp phần cứng/cache thực tế; docs chỉ rõ điều kiện nên hạ khi dữ liệu nằm trọn trong cache[^rpc-tip] | mục 7b |
| `effective_cache_size` | (docs: giả định của planner)[^ecs] | đặt quá thấp so với RAM thực làm planner né index | mục 3d |
| `default_statistics_target` | 100[^stats-target] | cột phân phối lệch cần histogram chi tiết hơn | mục 7e |

**9f. Hệ điều hành và quan sát**

| Chủ đề | Sự thật theo docs | Vì sao hay vỡ (suy luận của người soạn) | Xem lại ở |
|---|---|---|---|
| Overcommit/OOM killer của Linux | docs: hành vi bộ nhớ ảo mặc định trên Linux "không tối ưu" cho PostgreSQL vì kernel có thể kill postmaster[^oom-default]; có thể đặt strict overcommit để giảm khả năng bị kill[^oom-strict] hoặc đặt OOM score của postmaster là -1000[^oom-score] | hết RAM (do `work_mem`, nhiều connection) rồi một process quan trọng bị kill | [P3 · Chương 05, mục 5](../03-memory/05-bo-nho-tien-trinh-linux.md); container: [mục 8 chương đó](../03-memory/05-bo-nho-tien-trinh-linux.md) |
| Transparent huge pages | docs: THP "có thể gây giảm hiệu năng ở một số người dùng", không khuyến khích; `huge_pages` tường minh mặc định `try`[^thp][^huge-try] | bật THP mặc định của distro mà không biết | mục 3, phần Đi sâu |
| Log mặc định | `log_min_duration_statement` -1 (tắt)[^log-min-dur-def]; `log_lock_waits` off[^log-lock-waits]; `log_temp_files` -1 (tắt)[^log-temp]; `log_autovacuum_min_duration` 10 phút[^av-log] | sự cố xảy ra mà không có dấu vết để điều tra: không biết câu nào chậm, ai chờ lock, sort nào tràn đĩa | mục 8e |
| `pg_stat_statements` | không có sẵn: phải nạp qua `shared_preload_libraries` và restart[^pgss-setup] | chưa bật trước khi cần thì không có lịch sử | mục 8d |

<details><summary>Đi sâu: các tổ hợp nguy hiểm (nhận xét của người soạn)</summary>

- *Pool lớn + transaction giữ qua I/O chậm + không có `idle_in_transaction_session_timeout`:* mỗi request giữ connection (mục 2) và giữ snapshot (mục 5c) trong lúc chờ HTTP bên ngoài; khi dịch vụ ngoài chậm, connection cạn trước khi CPU database bận. Kết hợp với `statement_timeout = 0` thì không có lớp nào tự cắt.
- *Migration `ALTER TABLE` + transaction dài + `lock_timeout = 0`:* chính kịch bản ở mục 6c.
- *Bảng cập nhật nhiều + scale factor mặc định + một replication slot cũ:* autovacuum chạy nhưng không dọn được (horizon bị slot giữ — mục 5c), bảng phình, đồng thời WAL tích tụ[^wal-accum]. Hai triệu chứng khác nhau cùng một nguyên nhân.
- *Tăng `shared_buffers` mà không tăng `max_wal_size`:* docs nói hai tham số này thường đi cùng nhau[^sb-wal]; nếu không, checkpoint do WAL đầy xảy ra sớm hơn dự định (suy luận).

</details>

**Quay lại Q8 (phần 2):** các sự cố cấu hình hay gặp nằm ở năm chỗ — connection và bộ nhớ nhân lên theo số session, autovacuum tỷ lệ phần trăm trên bảng lớn, WAL/checkpoint và slot cũ, các timeout mặc định bằng 0, và OOM của OS — và đều có thể được nhìn thấy sớm bằng các view ở mục 8 nếu đã bật log và `pg_stat_statements` từ trước.

## Liên hệ với Rust

Phần này nối các cơ chế ở trên với ba thư viện Rust hay dùng để nói chuyện với PostgreSQL. Phiên bản được ghim (docs.rs): `sqlx` 0.9.0 (phần `PoolOptions` nằm ở `sqlx-core` 0.9.0), `tokio-postgres` 0.7.18, `deadpool-postgres` 0.14.2 và `deadpool` 0.13.1. Mọi đoạn code là *minh họa*, chưa biên dịch và chưa chạy; chương này không khẳng định kết quả chạy. Cách quản lý transaction, rollback khi drop và vòng retry `40001`/`40P01` đã ở [P12 · Chương 07, mục "Liên hệ với Rust"](07-transaction.md) — không lặp lại.

**R1. Kích thước pool so với `max_connections`.** Mỗi pool trong ứng dụng là một *nguồn* connection tới PostgreSQL (hoặc tới PgBouncer). `sqlx` đặt trần bằng `PoolOptions::max_connections`; docs của nó nhắc phải để ý hạn mức connection của database *và* của các ứng dụng khác dùng chung database, kể cả nhiều bản sao của cùng ứng dụng trong triển khai HA[^sqlx-maxconn]. `deadpool` có `PoolConfig::max_size`, mặc định `cpu_core_count * 2`[^dp-maxsize], và `Timeouts` mặc định là *không có timeout nào*[^dp-timeouts] — tức một tác vụ xin connection khi pool cạn có thể chờ vô hạn (suy luận của người soạn từ "No timeouts"). `sqlx` có `acquire_timeout`: giới hạn tổng thời gian `Pool::acquire()` được chờ qua các pha — chờ giấy phép từ semaphore, kiểm tra sống của connection idle (mặc định có: docs `sqlx-core` 0.9.0 nói connection idle được kiểm tra sống và toàn vẹn trước khi trả về, có thể phải chạy một lệnh trên connection[^sqlx-tba]), rồi nếu cần thì mở connection mới[^sqlx-acquire].

Phép tính (suy luận của người soạn; mục 2): *tổng* `max_connections` của mọi pool ở mọi pod ≤ `max_connections` của server − chỗ dành riêng cho superuser − chỗ cho tool/migration. Ví dụ tự đặt: `max_connections = 100`, 3 chỗ dành riêng[^super-res]; 12 pod × pool 10 = 120 > 97 — vượt hạn mức dù mỗi pod chỉ dùng một phần. Khi HPA (autoscaler) thêm pod, số connection *tự tăng theo*; đó là lý do thực tế để đặt PgBouncer ở giữa. Qua PgBouncer, giới hạn thật là `default_pool_size` mỗi cặp user/database[^pgb-poolsize], còn phía client là `max_client_conn`[^pgb-maxclient]; pool trong ứng dụng khi đó chỉ quyết định bao nhiêu truy vấn của pod này được *đang chờ trên* PgBouncer.

**R2. Statement cache và PgBouncer transaction mode.** Hai thế giới va nhau ở đây: thư viện Rust thích dùng prepared statement có tên và cache chúng theo connection, còn transaction pooling có thể đổi server connection giữa các transaction (mục 2).

- `sqlx`: mỗi connection có statement cache LRU, mặc định 100 câu, đổi bằng `PgConnectOptions::statement_cache_capacity`[^sqlx-cache]. `Query::persistent(bool)` (mặc định `true`) quyết định câu có được prepare một lần và giữ trong cache hay bị đóng sau khi chạy[^sqlx-persistent]. Theo source `sqlx-postgres` 0.9.0 (chi tiết cài đặt, có thể đổi), câu không persistent được prepare bằng statement *không tên* (`StatementId::UNNAMED`)[^sqlx-unnamed] — loại statement chỉ sống đến lần `Parse` không tên kế tiếp[^prep-unnamed]; còn nếu cùng chuỗi SQL đã nằm trong cache do một lần chạy `persistent(true)` trước đó thì vẫn dùng statement cache, như docs nêu[^sqlx-persistent].
- `tokio-postgres`: prepared statement "chỉ dùng được với connection đã tạo ra chúng"[^tp-prepare]. Truyền chuỗi thô vào `query` thì docs khuyên nên `prepare` trước nếu chạy lặp lại[^tp-query]; còn `query_typed` cho phép chạy mà không cần ba round trip (prepare, execute, close) bằng cách khai báo kiểu tham số — docs mô tả nó là phù hợp với môi trường không hỗ trợ prepared statement (ví dụ trong docs: Cloudflare Workers với Hyperdrive; docs không nhắc PgBouncer)[^tp-typed].
- `deadpool-postgres`: bọc `tokio_postgres::Client` kèm một `StatementCache`; `prepare_cached` dùng lại `Statement` có sẵn nếu có[^dp-cache]. Cách recycle mặc định là `Fast`, không chạy câu dọn dẹp nào[^dp-fast] — nó chỉ chạy `Client::is_closed()`[^dp-fast2]; `Clean` chạy chuỗi dọn trạng thái gần giống `DISCARD ALL` nhưng *không* gọi `DEALLOCATE ALL`, để cache statement không bị vô hiệu[^dp-clean].

Tổ hợp an toàn hay không (suy luận của người soạn từ các nguồn trên và docs PgBouncer ở mục 2; chưa chạy thử). Điều kiện chung cho cột *transaction* mode: `max_prepared_statements` ở PgBouncer phải khác 0 — mặc định 200 từ 1.24.0, nhưng ở 1.21–1.23 phải tự đặt (mục 2d); hãy kiểm tra giá trị thực của bản đang chạy.

| Cấu hình | Với PgBouncer *session* mode | Với PgBouncer *transaction* mode |
|---|---|---|
| `sqlx`, `persistent(true)` (mặc định) | khớp: server connection gắn với client suốt thời gian kết nối[^pgb-session], và prepared statement có tên sống đến hết session[^prep-proto] | an toàn *nếu* `max_prepared_statements` khác 0: PgBouncer theo dõi, viết lại tên và prepare lại khi server connection đổi[^pgb-maxprep]; nếu bằng 0 thì hỗ trợ bị tắt[^pgb-maxprep-zero] và statement đã prepare có thể không có trên server connection kế tiếp, vì prepared statement chỉ sống trong session tạo ra nó[^prep-session] |
| `sqlx`, `persistent(false)` | khớp | dùng statement không tên (source 0.9.0, chi tiết cài đặt)[^sqlx-unnamed], nên không để lại statement có tên trên server connection; docs PgBouncer chỉ nói về statement *có tên*, và việc statement không tên đi trọn trong một transaction là suy luận của người soạn; nếu SQL đã nằm trong cache do lần `persistent(true)` trước thì vẫn dùng statement cache[^sqlx-persistent] |
| `tokio-postgres` / `deadpool-postgres` với `prepare_cached` | khớp | `Statement` chỉ dùng được với connection tạo ra nó[^tp-prepare]; cần `max_prepared_statements` khác 0 như hàng đầu, hoặc tránh cache |
| `query_typed` | khớp | docs `tokio-postgres`: hợp với môi trường không hỗ trợ prepared statement[^tp-typed] (docs không nhắc PgBouncer) |
| `PREPARE`/`EXECUTE` mức SQL | khớp | không dùng được: lệnh chuyển thẳng tới Postgres, không được PgBouncer theo dõi[^pgb-maxprep-sql] |

Sau migration DDL, statement đã cache có thể báo `cached plan must not change result type` — xem đoạn *Hệ quả khi migration DDL* ở mục 2.

Ví dụ cấu hình pool tới PgBouncer, tắt giữ statement có tên cho truy vấn này (minh họa):

```rust
use std::time::Duration;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;

pub async fn make_pool() -> Result<PgPool, sqlx::Error> {
    let opts = PgConnectOptions::new()
        .host("pgbouncer.internal")
        .port(6432)
        .username("app")
        .database("app")
        // hiện trong cột application_name của pg_stat_activity (mục 8)
        .application_name("orders-api");

    PgPoolOptions::new()
        // phần của MỖI pod trong ngân sách connection (R1)
        .max_connections(10)
        // pool cạn thì trả lỗi sau 3 giây thay vì chờ vô hạn
        .acquire_timeout(Duration::from_secs(3))
        .connect_with(opts)
        .await
}

pub async fn find_name(pool: &PgPool, id: i64) -> Result<Option<String>, sqlx::Error> {
    use sqlx::Row;
    let row = sqlx::query("SELECT name FROM accounts WHERE id = $1")
        .bind(id)
        // không giữ prepared statement có tên trên connection (R2)
        .persistent(false)
        .fetch_optional(pool)
        .await?;
    row.map(|r| r.try_get::<String, _>("name")).transpose()
}
```

Ghi chú: `application_name` là cột có thật trong `pg_stat_activity` ("tên ứng dụng đang kết nối tới backend này")[^psa-appname], và `sqlx` có `PgConnectOptions::application_name`[^sqlx-appname] — nên đặt tên dịch vụ ở đây để mục 8 cho thấy *service nào* đang giữ transaction dài.

**R3. `statement_timeout` từ Rust.** `statement_timeout` là hạn mức *phía server*: tính từ khi lệnh tới server tới khi xong; trong giao thức extended thì từ message Parse/Bind/Execute/Describe đầu tiên tới khi `Execute`/`Sync` hoàn tất[^st-def]; mặc định 0 (tắt)[^st-default]. Có bốn chỗ đặt, mỗi chỗ có hệ quả với pooler (suy luận của người soạn ghép từ docs):

1. *Mặc định của role/database:* `ALTER ROLE app SET statement_timeout = '5s'` — giá trị trở thành mặc định cho *session mới* của role[^alter-role]. Qua PgBouncer, nó áp dụng cho các server connection mà PgBouncer mở *sau* lệnh này bằng role đó; connection đã nằm sẵn trong pool giữ giá trị cũ cho tới khi được mở lại (suy luận của người soạn từ chữ "session mới" của docs) — vì không phụ thuộc `SET` của client nên đây là cách ít vướng nhất với transaction pooling. Docs khuyên *không* đặt `statement_timeout` trong `postgresql.conf` vì ảnh hưởng mọi session[^st-conf].
2. *Tham số khởi động qua `options`:* `sqlx` có `PgConnectOptions::options` (cặp khóa-giá trị gửi lúc khởi động)[^sqlx-options]; `tokio-postgres::Config` có `options` (tùy chọn dòng lệnh dùng để cấu hình server)[^tp-options]. Nhưng PgBouncer mặc định chỉ chấp nhận tham số khởi động nó theo dõi được, các tham số khác gây lỗi — trừ khi liệt kê trong `ignore_startup_parameters`[^pgb-ignore] (cả tham số nằm trong `options` cũng được hỗ trợ ở đó[^pgb-ignore-ex]).
3. *`SET` ở `after_connect` của sqlx:* docs `sqlx` có ví dụ chạy `SET application_name…; SET search_path…` sau khi kết nối[^sqlx-after]. Đó là `SET` mức session; với transaction pooling nó chỉ chắc chắn đúng cho server connection ở thời điểm đó (mục 2d, bảng `SET/RESET` ghi "Never"[^pgb-table]).
4. *Mỗi transaction:* `SET LOCAL` chỉ có hiệu lực đến hết transaction hiện tại[^set-local]; hàm `set_config(name, value, true)` làm điều tương tự với tham số bind được[^set-config]. Dưới đây là hình dạng (minh họa):

```rust
pub async fn debit(pool: &PgPool, id: i64, amount: i64) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    // hiệu lực đến hết transaction này; an toàn với transaction pooling
    sqlx::query("SELECT set_config('statement_timeout', $1, true)")
        .bind("5s")
        .execute(&mut *tx)
        .await?;
    let done = sqlx::query("UPDATE accounts SET balance = balance - $1 WHERE id = $2")
        .bind(amount)
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    tx.commit().await?;
    Ok(done)
}
```

PgBouncer có `query_timeout` riêng, nhưng docs nói chỉ nên dùng với một `statement_timeout` phía server *nhỏ hơn một chút*, để áp dụng cho sự cố mạng[^pgb-qtimeout]. Còn timeout *phía client* của Rust (ví dụ bọc future bằng `tokio::time::timeout`) chỉ ngừng *chờ*; muốn yêu cầu server hủy truy vấn thì `tokio-postgres` có `Client::cancel_token()` — "token hủy dùng để yêu cầu hủy một truy vấn đang chạy trên connection của client này"[^tp-cancel]. Việc hủy đã gửi đi có đến kịp hay không là chuyện khác (không có trong nguồn đọc ở đây).

**R4. Hai thói quen nên có.** (1) Đặt `lock_timeout` ngắn trong *công cụ migration* của bạn (mục 6d); (2) không giữ `Transaction`/connection qua lời gọi mạng chậm — mỗi giây giữ là một connection và một snapshot bị khóa (mục 2, 5c); mối liên hệ đã được [P4 · Chương 05, mục 7](../04-concurrency/05-concurrency-bug-va-deadlock.md) phân tích ở tầng pool.
**R5. Cache và hàng đợi đứng cạnh PostgreSQL (nối sang Redis).** Với mẫu cache-aside — đọc cache trước, trượt thì đọc PostgreSQL rồi nạp lại cache — Redis giảm số truy vấn *đọc* tới PostgreSQL (nhận xét của người soạn). Phần đúng đắn của mẫu này (cuộc đua stale set, lease, stampede) được phân tích ở [P12 · Chương 17, mục 1](17-redis-patterns.md) và [P13 · Chương 09, mục 4](../13-tla-plus/09-mo-hinh-hoa-he-backend.md); các nút vặn phía Redis (`maxmemory`, TTL, single-flight) ở [P12 · Chương 16, mục 8.4](16-redis-ben-trong.md); chương này không lặp lại. Điều thuộc về phía PostgreSQL (suy luận của người soạn từ các mục trước): khi cache trượt hàng loạt, tải dồn xuống PostgreSQL chỉ bị chặn bởi những gì bạn đã đặt — kích thước pool và `max_connections` (R1, mục 2), `statement_timeout` (R3, mục 6f) và hàng đợi `query_wait_timeout` của PgBouncer (mục 2); còn thao tác ghi vẫn đi trọn qua WAL và lock (mục 4, 6). Đừng giữ transaction PostgreSQL mở trong lúc chờ Redis hay HTTP (R4). Khi một thay đổi trong PostgreSQL phải kéo theo thông điệp sang Redis hoặc queue, mẫu *transactional outbox* và mô hình retry/timeout được bàn ở [P13 · Chương 09, mục 5 và 6](../13-tla-plus/09-mo-hinh-hoa-he-backend.md); khóa phân tán bằng Redis (so với advisory lock ở mục 2d) ở [P12 · Chương 16, mục 8.1–8.3](16-redis-ben-trong.md) và cách ghép thành pattern ở [P12 · Chương 17, mục 3](17-redis-patterns.md).

## Tóm tắt

- Một connection = một process backend do postmaster `fork`; ngoài ra có checkpointer, background writer, WAL writer, autovacuum; tất cả chia sẻ shared memory[^pm-process][^backend-types][^pm-ipc].
- Khi PostgreSQL làm hệ thống hết bộ nhớ, docs khuyên trong nhiều trường hợp giảm `max_connections` và dùng pooler ngoài[^kr-pool]; PgBouncer có ba mode, và transaction mode phá `SET`, `LISTEN`, `PREPARE` mức SQL, advisory lock mức session[^pgb-table][^pgb-table-prep].
- `work_mem` là hạn mức *mỗi thao tác sort/hash*; tổng bộ nhớ tỷ lệ với session × thao tác[^wm-multi]; `effective_cache_size` chỉ là giả định của planner[^ecs-noalloc]; page có thể nằm cả ở shared buffers và page cache[^ecs-both].
- `UPDATE` thêm row version, sinh WAL; `COMMIT` flush WAL; data page được ghi sau; checkpoint là điểm bắt đầu REDO và `full_page_writes` làm WAL phình sau mỗi checkpoint[^wal-intro][^ckpt-redo][^fpw-def].
- VACUUM chỉ xóa được dòng chết mà không snapshot nào còn thấy; transaction mở lâu, prepared transaction, replication slot kéo lùi horizon và gây bloat lẫn nguy cơ wraparound[^iit][^wrap-longtx][^wrap-slots].
- Ngưỡng autovacuum là ngưỡng cơ sở + tỷ lệ × số dòng, nên bảng lớn cần chỉnh riêng; XID 32 bit cần freeze định kỳ, và gần wraparound server từ chối ghi[^av-formula][^xid-32][^wrap-refuse].
- Lock xin xếp hàng: `ACCESS EXCLUSIVE` đang chờ chặn cả truy vấn đến sau; migration cần `lock_timeout` ngắn và lệnh ít lock (`CONCURRENTLY`, `NOT VALID`)[^lock-rule1][^lt-def][^ci-conc][^at-notvalid].
- Deadlock được phát hiện lạc quan sau `deadlock_timeout` (1s); lock bảng có tám chế độ phân biệt bằng ma trận xung đột[^lock-optimistic][^dl-timeout-def][^lk-intro2].
- Đọc `EXPLAIN ANALYZE` là so số hàng ước lượng với thực tế từ dưới lên; plan tệ thường do statistics cũ/thiếu, cột tương quan, hoặc hằng số chi phí lệch[^actual-ms][^ext-indep][^rpc-why].
- Quan sát bằng `pg_stat_activity`, wait event, `pg_blocking_pids()` và `pg_stat_statements`[^psa-row][^pglocks-blocking][^pgss-intro]; `statement_timeout`, `lock_timeout` và `idle_in_transaction_session_timeout` mặc định 0 (tắt) nên không có giới hạn nào[^st-default][^lt-default][^iit-def] (nhận xét của người soạn: đó là nguồn sự cố phổ biến).
- Từ Rust: tổng pool của mọi pod phải nằm trong ngân sách `max_connections`; với PgBouncer transaction mode phải chú ý statement cache và cách đặt `statement_timeout` (`SET LOCAL`/`set_config` hoặc mặc định theo role)[^sqlx-maxconn][^sqlx-persistent][^set-local].

## Câu hỏi tự kiểm tra

1. Docs nói một session idle ngoài transaction "không gây chi phí lớn". Vậy vì sao vẫn nên dùng pooler?

<details><summary>Đáp án</summary>

Chi phí đáng kể nằm ở số connection *đang chạy* (tranh CPU/I/O, bộ nhớ riêng như `work_mem` và `temp_buffers`) và ở connection kẹt trong transaction (giữ snapshot, lock); pooler cho phép giới hạn số truy vấn chạy đồng thời ở con số bạn chọn[^idle-cheap][^work-mem][^kr-pool]. Mỗi connection còn là một process và một chỗ trong bảng dùng chung cỡ `max_connections`[^pm-process][^kr-sem].

</details>

2. Kể ba thứ không dùng được ở PgBouncer transaction mode và một cách thay thế cho mỗi thứ.

<details><summary>Đáp án</summary>

`SET` mức session → `SET LOCAL` hoặc `ALTER ROLE … SET`; `PREPARE`/`DEALLOCATE` mức SQL → prepared statement mức giao thức với `max_prepared_statements` khác 0 (mặc định 200 từ PgBouncer 1.24.0, phải tự đặt ở 1.21–1.23), hoặc không dùng statement có tên; advisory lock mức session → `pg_advisory_xact_lock` (nhả cuối transaction); (cũng "Never": `LISTEN`, `LOAD`, temp table `PRESERVE`/`DELETE ROWS`)[^pgb-table][^pgb-table-prep][^set-local][^alter-role][^pgb-maxprep][^pgb-prep-default][^pgb-prep-1-21][^adv-xact].

</details>

3. 100 session đồng thời, mỗi session chạy một truy vấn có 3 thao tác sort, `work_mem = 4MB`. Cận trên bộ nhớ cho các thao tác này là bao nhiêu và vì sao đó chỉ là cận trên *lý thuyết*?

<details><summary>Đáp án</summary>

100 × 3 × 4MB = 1200MB (phép nhân của người soạn). Docs: mỗi thao tác "thường" được dùng tới `work_mem` trước khi tràn ra đĩa, và nhiều session có thể làm vậy cùng lúc; thực tế tùy thao tác có chạm hạn mức không (và hash còn nhân `hash_mem_multiplier`)[^wm-multi][^work-mem][^hash-mem].

</details>

4. Thứ tự đúng giữa "ghi WAL", "báo commit thành công" và "ghi data page" là gì, và vì sao crash không làm mất commit?

<details><summary>Đáp án</summary>

Sinh WAL record → flush WAL ở commit (với `synchronous_commit` mặc định `on`) → báo thành công → data page ghi sau (bgwriter/checkpointer). Sau crash, recovery lấy *redo record* từ checkpoint record gần nhất và phát lại WAL[^wal-intro][^wal-flush-commit][^ckpt-def][^ckpt-redo].

</details>

5. Bảng 50 triệu dòng, `autovacuum_vacuum_threshold` = 50 và `scale_factor` = 0,2 (mặc định). Cần bao nhiêu tuple lỗi thời để autovacuum kích hoạt? Bạn đổi gì cho bảng này?

<details><summary>Đáp án</summary>

50 + 0,2 × 50 000 000 = 10 000 050 (phép tính của người soạn theo công thức của docs, với `reltuples` ≈ 50 triệu). Có thể hạ `autovacuum_vacuum_scale_factor` riêng cho bảng bằng storage parameter[^av-formula][^av-thr-def][^av-scale-def][^av-pertable].

</details>

6. Autovacuum chạy liên tục trên một bảng nhưng `n_dead_tup` không giảm và bảng vẫn phình. Kể ba chỗ cần kiểm tra.

<details><summary>Đáp án</summary>

Transaction mở lâu (`backend_xmin` cũ ở `pg_stat_activity`), prepared transaction treo (`pg_prepared_xacts`), replication slot cũ (`pg_replication_slots`); cũng có thể `hot_standby_feedback` từ standby. VACUUM báo `dead but not yet removable` cho số tuple bị giữ[^wrap-longtx][^wrap-prepared][^wrap-slots][^hsf][^vl-log][^iit].

</details>

7. Khi còn dưới ba triệu transaction tới wraparound thì server làm gì, và vì sao `VACUUM FULL` không phải lối thoát?

<details><summary>Đáp án</summary>

Từ chối cấp XID mới: transaction đang chạy tiếp được, chỉ transaction chỉ-đọc mới bắt đầu được, thao tác ghi thất bại, `VACUUM` thường vẫn chạy. `VACUUM FULL` cần một XID nên sẽ thất bại (hoặc, ở chế độ superuser, tiêu thêm XID); không cần vào single-user mode[^wrap-threshold][^wrap-refuse][^wrap-novacfull][^wrap-nosingle].

</details>

8. Một `ALTER TABLE` đang chờ sau một `SELECT` chạy lâu. Vì sao các `SELECT` mới tới bảng đó cũng treo, và hai cách giảm thiệt hại?

<details><summary>Đáp án</summary>

Yêu cầu lock chỉ được cấp ngay nếu không xung đột với lock đang giữ *hoặc yêu cầu đang chờ*; `SELECT` mới (`ACCESS SHARE`) xung đột với `ACCESS EXCLUSIVE` đang chờ nên xếp sau nó. Giảm thiệt hại: `lock_timeout` ngắn cho phiên migration rồi retry; dùng lệnh ít lock (`NOT VALID` + `VALIDATE`, `CREATE INDEX CONCURRENTLY`) và không chạy khi có transaction dài[^lock-rule1][^lock-order][^lk-tip][^lt-def][^at-notvalid][^ci-conc].

</details>

9. Vì sao `deadlock_timeout` mặc định là 1 giây thay vì 0, và nó còn ảnh hưởng gì khác?

<details><summary>Đáp án</summary>

Kiểm tra deadlock khá đắt nên PostgreSQL chờ lạc quan một lúc rồi mới kiểm tra, vì giả định deadlock hiếm; khi bật `log_lock_waits` nó cũng là thời gian chờ trước khi ghi log về việc chờ lock[^dl-timeout][^lock-optimistic][^dl-logwait].

</details>

10. `EXPLAIN ANALYZE` cho một node có `rows=10` ước lượng nhưng thực tế `rows=1000000` ngay ở scan. Nêu bước điều tra, và hai điều *không* nên làm với `EXPLAIN ANALYZE` trên bảng nóng.

<details><summary>Đáp án</summary>

Nghi statistics cũ/thiếu (chạy `ANALYZE`, xem autovacuum analyze), cột tương quan (`CREATE STATISTICS`), phân phối lệch (nâng statistics target). Đừng chạy câu ghi bằng `EXPLAIN ANALYZE` mà không bọc `BEGIN … ROLLBACK` (nó thật sự chạy và có tác dụng phụ), và đừng suy luận từ bảng nhỏ ra bảng lớn[^actual-ms][^stats-pgclass][^ext-indep][^stats-target][^explain-sideeffect][^explain-toy].

</details>

11. Vì sao docs khuyên dùng `pg_blocking_pids()` thay vì tự join `pg_locks` với chính nó?

<details><summary>Đáp án</summary>

Tự join phải mã hóa chế độ nào xung đột chế độ nào, và `pg_locks` không cho biết process nào đứng trước process nào trong hàng đợi, cũng như parallel worker thuộc session nào; `pg_blocking_pids()` xử lý cả hard block lẫn soft block[^pglocks-blocking][^blockpids].

</details>

12. Có 12 pod, mỗi pod `max_connections(10)` trên sqlx, nối thẳng PostgreSQL với `max_connections = 100`. Chuyện gì xảy ra, và chuyển qua PgBouncer transaction mode thì cần chú ý gì với statement cache?

<details><summary>Đáp án</summary>

120 connection tiềm năng so với trần 100 trừ 3 chỗ dành riêng cho superuser — vượt hạn mức (phép tính của người soạn); docs `sqlx` nhắc để ý hạn mức của database và nhiều bản sao ứng dụng. Với transaction mode: `persistent` mặc định `true` giữ statement có tên trong cache theo connection; cần `max_prepared_statements` khác 0 ở PgBouncer (mặc định 200 từ 1.24.0) hoặc dùng `.persistent(false)` (statement không tên theo source 0.9.0)[^maxconn][^super-res][^sqlx-maxconn][^sqlx-persistent][^pgb-maxprep][^pgb-prep-default][^sqlx-unnamed].

</details>

## Đọc thêm

- [PostgreSQL 16 docs — 20.4 Resource Consumption](https://www.postgresql.org/docs/16/runtime-config-resource.html): `shared_buffers`, `work_mem`, `maintenance_work_mem`, cost-based vacuum delay, background writer; đọc để biết từng tham số bộ nhớ chính xác nói gì.
- [PostgreSQL 16 docs — 25.1 Routine Vacuuming](https://www.postgresql.org/docs/16/routine-vacuuming.html): toàn bộ VACUUM, visibility map, wraparound, autovacuum; đọc để nắm các công thức và quy trình khắc phục.
- [PostgreSQL 16 docs — 13.3 Explicit Locking](https://www.postgresql.org/docs/16/explicit-locking.html): bảng chế độ lock và ma trận xung đột gốc (Table 13.2), deadlock, advisory lock.
- [PostgreSQL 16 docs — 14.1 Using EXPLAIN](https://www.postgresql.org/docs/16/using-explain.html) và [14.2 Statistics Used by the Planner](https://www.postgresql.org/docs/16/planner-stats.html): các ví dụ plan đầy đủ và statistics mở rộng.
- [PostgreSQL 16 docs — 28.2 The Cumulative Statistics System](https://www.postgresql.org/docs/16/monitoring-stats.html): mọi cột của `pg_stat_activity` và bảng wait event đầy đủ.
- [PostgreSQL 16 docs — F.32 pg_stat_statements](https://www.postgresql.org/docs/16/pgstatstatements.html): thiết lập, cột, và các lưu ý về `queryid`.
- [PgBouncer — Features](https://www.pgbouncer.org/features.html) và [Configuration](https://www.pgbouncer.org/config.html): bảng tương thích theo mode, `max_prepared_statements`, `ignore_startup_parameters`, `query_wait_timeout`.
- [P12 · Chương 03 — Storage engine](03-storage-engine.md), mục 5: B-tree, WAL, full-page write, HOT ở mức khái niệm.
- [P12 · Chương 07 — Transaction và isolation](07-transaction.md): MVCC, `FOR UPDATE`, lỗi `40001`/`40P01` và retry trong Rust.
- [P4 · Chương 05 — Bug concurrency và deadlock](../04-concurrency/05-concurrency-bug-va-deadlock.md), mục 7: deadlock và pool-locking ở tầng database/pool.
- [P5 · Chương 04](../05-storage/04-page-cache-va-direct-io.md) và [Chương 06](../05-storage/06-durability-trong-thuc-te.md): page cache, `fsync`, durability nền cho mục 3 và 4.
- [P12 · Chương 17 — Redis trong thực chiến](17-redis-patterns.md), mục 1: cache-aside, stale và stampede khi Redis đứng trước PostgreSQL (xem R5); [Chương 16](16-redis-ben-trong.md), mục 8.4: các nút vặn phía Redis.
- [P13 · Chương 09 — Mô hình hóa hệ backend](../13-tla-plus/09-mo-hinh-hoa-he-backend.md): cache-aside, outbox và retry/timeout bằng TLA+.
- [docs.rs — `sqlx` 0.9.0, `PgConnectOptions`](https://docs.rs/sqlx/0.9.0/sqlx/postgres/struct.PgConnectOptions.html) và [`tokio-postgres` 0.7.18, `Client`](https://docs.rs/tokio-postgres/0.7.18/tokio_postgres/struct.Client.html): statement cache, `options`, `cancel_token`.
- [docs.rs — `deadpool-postgres` 0.14.2](https://docs.rs/deadpool-postgres/0.14.2/deadpool_postgres/): `ManagerConfig`, `RecyclingMethod`, `prepare_cached`.

[^pm-process]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 52.2 How Connections Are Established — <https://www.postgresql.org/docs/16/connect-estab.html> — "PostgreSQL implements a “process per user” client/server model. In this model, every client process connects to exactly one backend process."
[^pm-spawn]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 52.2 How Connections Are Established — <https://www.postgresql.org/docs/16/connect-estab.html> — "we have to use a “supervisor process” that spawns a new backend process every time a connection is requested. This supervisor process is called postmaster and listens at a specified TCP/IP port for incoming connections."
[^pm-ipc]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 52.2 How Connections Are Established — <https://www.postgresql.org/docs/16/connect-estab.html> — "Those backend processes communicate with each other and with other processes of the instance using semaphores and shared memory to ensure data integrity throughout concurrent data access."
[^pm-query]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 52.2 How Connections Are Established — <https://www.postgresql.org/docs/16/connect-estab.html> — "The backend process parses the query, creates an execution plan, executes the plan, and returns the retrieved rows to the client by transmitting them over the established connection."
[^tut-fork]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 1.2 Architectural Fundamentals — <https://www.postgresql.org/docs/16/tutorial-arch.html> — "To achieve this it starts (“forks”) a new process for each connection. From that point on, the client and the new server process communicate without intervention by the original postgres process."
[^pm-forkimm]: PostgreSQL REL_16_4, src/backend/postmaster/postmaster.c, comment đầu file — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/postmaster/postmaster.c> — "When a request message is received, we now fork() immediately. The child process performs authentication of the request, and then becomes a backend if successful."
[^pm-bestart]: PostgreSQL REL_16_4, postmaster.c, BackendStartup — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/postmaster/postmaster.c> — "Close the postmaster's sockets */ ClosePostmasterPorts(false); /* Perform additional initialization and collect startup packet */ BackendInitialize(port); /* * Create a per-backend PGPROC struct in shared memory. We must do * this before we can use LWLocks."
[^pm-noshm]: PostgreSQL REL_16_4, postmaster.c, comment đầu file — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/postmaster/postmaster.c> — "Keeping the postmaster away from shared memory operations makes it simpler and more reliable. The postmaster is almost always able to recover from crashes of individual backends by resetting shared memory"
[^pm-aux]: PostgreSQL REL_16_4, postmaster.c, các macro khởi động process nền — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/postmaster/postmaster.c> — "#define StartBackgroundWriter() StartChildProcess(BgWriterProcess) #define StartCheckpointer() StartChildProcess(CheckpointerProcess) #define StartWalWriter() StartChildProcess(WalWriterProcess)"
[^pm-crash]: PostgreSQL REL_16_4, postmaster.c, HandleChildCrash — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/postmaster/postmaster.c> — "errmsg("terminating any other active server processes")"
[^backend-types]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 pg_stat_activity, cột backend_type — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "Possible types are autovacuum launcher, autovacuum worker, logical replication launcher, logical replication worker, parallel worker, background writer, client backend, checkpointer, archiver, standalone backend, startup, walreceiver, walsender and walwriter."
[^ckpt-proc]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 30.5 WAL Configuration — <https://www.postgresql.org/docs/16/wal-configuration.html> — "The server's checkpointer process automatically performs a checkpoint every so often."
[^ckpt-trigger]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 30.5 WAL Configuration — <https://www.postgresql.org/docs/16/wal-configuration.html> — "A checkpoint is begun every checkpoint_timeout seconds, or if max_wal_size is about to be exceeded, whichever comes first."
[^bgw]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.5 Background Writer — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "There is a separate server process called the background writer, whose function is to issue writes of “dirty” (new or modified) shared buffers."
[^walw]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.5 wal_writer_delay — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "Specifies how often the WAL writer flushes WAL, in time terms."
[^av-launcher]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.6 The Autovacuum Daemon — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "There is a persistent daemon process, called the autovacuum launcher, which is in charge of starting autovacuum worker processes for all databases."
[^av-nocount]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.6 The Autovacuum Daemon — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "Note that the number of running workers does not count towards max_connections or superuser_reserved_connections limits."
[^par-sep]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.6 max_parallel_workers_per_gather — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "each worker process is a completely separate process which has roughly the same impact on the system as an additional user session."
[^par-five]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.6 max_parallel_workers_per_gather — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "For example, a parallel query using 4 workers may use up to 5 times as much CPU time, memory, I/O bandwidth, and so forth as a query which uses no workers at all."
[^kr-sem]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 19.4.1 Shared Memory and Semaphores — <https://www.postgresql.org/docs/16/kernel-resources.html> — "When using System V semaphores, PostgreSQL uses one semaphore per allowed connection (max_connections), allowed autovacuum worker process (autovacuum_max_workers), allowed WAL sender process (max_wal_senders), and allowed background process (max_worker_processes), in sets of 16."
[^pid-ps]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 9.27.2 Server Signaling Functions — <https://www.postgresql.org/docs/16/functions-admin.html> — "The process ID of an active backend can be found from the pid column of the pg_stat_activity view, or by listing the postgres processes on the server (using ps on Unix or the Task Manager on Windows)."
[^oom-pm]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 19.4.4 Linux Memory Overcommit — <https://www.postgresql.org/docs/16/kernel-resources.html> — "the kernel might terminate the PostgreSQL postmaster (the supervisor server process) if the memory demands of either PostgreSQL or another process cause the system to run out of virtual memory."
[^oom-conn]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 19.4.4 Linux Memory Overcommit — <https://www.postgresql.org/docs/16/kernel-resources.html> — "Although existing database connections will continue to function normally, no new connections will be accepted."
[^kr-pool]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 19.4.4 Linux Memory Overcommit — <https://www.postgresql.org/docs/16/kernel-resources.html> — "In many cases, it may be better to reduce max_connections and instead make use of external connection-pooling software."
[^maxconn]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.3 max_connections — <https://www.postgresql.org/docs/16/runtime-config-connection.html> — "Determines the maximum number of concurrent connections to the database server. The default is typically 100 connections, but might be less if your kernel settings will not support it (as determined during initdb). This parameter can only be set at server start."
[^super-res]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.3 superuser_reserved_connections — <https://www.postgresql.org/docs/16/runtime-config-connection.html> — "The default value is three connections. The value must be less than max_connections minus reserved_connections."
[^work-mem]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 work_mem — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "Also, several running sessions could be doing such operations concurrently. Therefore, the total memory used could be many times the value of work_mem; it is necessary to keep this fact in mind when choosing the value."
[^temp-buf]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 temp_buffers — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "A session will allocate temporary buffers as needed up to the limit given by temp_buffers."
[^idle-cheap]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.11 idle_session_timeout — <https://www.postgresql.org/docs/16/runtime-config-client.html> — "Unlike the case with an open transaction, an idle session without a transaction imposes no large costs on the server, so there is less need to enable this timeout than idle_in_transaction_session_timeout."
[^pgb-modes]: PgBouncer docs (trang sống, truy cập 2026-10-12), Features — <https://www.pgbouncer.org/features.html> — "Several levels of brutality when rotating connections: Session pooling Most polite method."
[^pgb-session]: PgBouncer docs (trang sống, truy cập 2026-10-12), Features — <https://www.pgbouncer.org/features.html> — "When a client connects, a server connection will be assigned to it for the whole duration it stays connected. When the client disconnects, the server connection will be put back into pool. This mode supports all PostgreSQL features."
[^pgb-trans]: PgBouncer docs (trang sống, truy cập 2026-10-12), Features — <https://www.pgbouncer.org/features.html> — "A server connection is assigned to a client only during a transaction. When PgBouncer notices that the transaction is over, the server will be put back into the pool."
[^pgb-compat]: PgBouncer docs (trang sống, truy cập 2026-10-12), Features — <https://www.pgbouncer.org/features.html> — "This mode breaks a few session-based features of PostgreSQL. You can use it only when the application cooperates by not using features that break."
[^pgb-stmt]: PgBouncer docs (trang sống, truy cập 2026-10-12), Features — <https://www.pgbouncer.org/features.html> — "Multi-statement transactions are disallowed. This is meant to enforce “autocommit” mode on the client, mostly targeted at PL/Proxy."
[^pgb-table]: PgBouncer docs (trang sống, truy cập 2026-10-12), Features, bảng SQL feature map — <https://www.pgbouncer.org/features.html> — "SET/RESET Yes Never LISTEN Yes Never NOTIFY Yes Yes WITHOUT HOLD CURSOR Yes Yes WITH HOLD CURSOR Yes Never"
[^pgb-table-prep]: PgBouncer docs (trang sống, truy cập 2026-10-12), Features, bảng SQL feature map — <https://www.pgbouncer.org/features.html> — "Protocol-level prepared plans Yes Yes 2 PREPARE / DEALLOCATE Yes Never ON COMMIT DROP temp tables Yes Yes PRESERVE/DELETE ROWS temp tables Yes Never Cached plan reset Yes Yes LOAD statement Yes Never Session-level advisory locks Yes Never"
[^pgb-2kb]: PgBouncer docs (trang sống, truy cập 2026-10-12), Features — <https://www.pgbouncer.org/features.html> — "Low memory requirements (2 kB per connection by default). This is because PgBouncer does not need to see full packets at once."
[^pgb-maxclient]: PgBouncer docs (trang sống, truy cập 2026-10-12), Configuration, max_client_conn — <https://www.pgbouncer.org/config.html> — "Note: ulimit does not apply in a Windows environment. Default: 100"
[^pgb-poolsize]: PgBouncer docs (trang sống, truy cập 2026-10-12), Configuration, default_pool_size — <https://www.pgbouncer.org/config.html> — "The maximum number of server connections to allow per user/database pair. Can be overridden by pool_size in the per-database and per-user configuration; this is the default used if no specific pool_size is specified for a given database or user. Default: 20"
[^pgb-mode-default]: PgBouncer docs (trang sống, truy cập 2026-10-12), Configuration, pool_mode — <https://www.pgbouncer.org/config.html> — "Server is released back to pool after client disconnects. Default."
[^pgb-reset]: PgBouncer docs (trang sống, truy cập 2026-10-12), Configuration, server_reset_query — <https://www.pgbouncer.org/config.html> — "When transaction pooling is used, the server_reset_query is not used, because in that mode, clients must not use any session-based features, since each transaction ends up in a different connection and thus gets a different session state."
[^pgb-reset-q]: PgBouncer docs (trang sống, truy cập 2026-10-12), Configuration, server_reset_query — <https://www.pgbouncer.org/config.html> — "The default is DISCARD ALL, which cleans everything, but that leaves the next client no pre-cached state. It can be made lighter, e.g. DEALLOCATE ALL to just drop prepared statements, if the application does not break when some state is kept around."
[^pgb-maxprep]: PgBouncer docs (trang sống, truy cập 2026-10-12), Configuration, max_prepared_statements — <https://www.pgbouncer.org/config.html> — "When this is set to a non-zero value PgBouncer tracks protocol-level named prepared statements related commands sent by the client in transaction and statement pooling mode. PgBouncer makes sure that any statement prepared by a client is available on the backing server connection. Even when the statement was originally prepared on another server connection."
[^pgb-maxprep-sql]: PgBouncer docs (trang sống, truy cập 2026-10-12), Configuration, max_prepared_statements — <https://www.pgbouncer.org/config.html> — "This tracking and rewriting of prepared statement commands does not work for SQL-level prepared statement commands, so PREPARE, EXECUTE and DEALLOCATE are forwarded straight to Postgres."
[^pgb-ignore]: PgBouncer docs (trang sống, truy cập 2026-10-12), Configuration, ignore_startup_parameters — <https://www.pgbouncer.org/config.html> — "By default, PgBouncer allows only parameters it can keep track of in startup packets (see track_extra_parameters for the list). All other parameters will raise an error."
[^pgb-ignore-ex]: PgBouncer docs (trang sống, truy cập 2026-10-12), Configuration, ignore_startup_parameters — <https://www.pgbouncer.org/config.html> — "The Postgres protocol allows specifying parameters settings, both directly as a parameter in the startup packet, or inside the options startup packet. Parameters specified using both of these methods are supported by ignore_startup_parameters."
[^pgb-qwait]: PgBouncer docs (trang sống, truy cập 2026-10-12), Configuration, query_wait_timeout — <https://www.pgbouncer.org/config.html> — "Maximum time queries are allowed to spend waiting for execution. If the query is not assigned to a server during that time, the client is disconnected. 0 disables. If this is disabled, clients will be queued indefinitely."
[^pgb-fd]: PgBouncer docs (trang sống, truy cập 2026-10-12), Configuration, max_client_conn — <https://www.pgbouncer.org/config.html> — "If each user connects under its own user name to the server, the theoretical maximum used is: max_client_conn + (max pool_size * total databases * total users)"
[^prep-session]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), PREPARE — <https://www.postgresql.org/docs/16/sql-prepare.html> — "Prepared statements only last for the duration of the current database session. When the session ends, the prepared statement is forgotten, so it must be recreated before being used again."
[^prep-proto]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 55.2.3 Extended Query — <https://www.postgresql.org/docs/16/protocol-flow.html> — "If successfully created, a named prepared-statement object lasts till the end of the current session, unless explicitly destroyed."
[^set-local]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), SET — <https://www.postgresql.org/docs/16/sql-set.html> — "The effects of SET LOCAL last only till the end of the current transaction, whether committed or not."
[^alter-role]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), ALTER ROLE — <https://www.postgresql.org/docs/16/sql-alterrole.html> — "Whenever the role subsequently starts a new session, the specified value becomes the session default, overriding whatever setting is present in postgresql.conf or has been received from the postgres command line."
[^adv-session]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.5 Advisory Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "Once acquired at session level, an advisory lock is held until explicitly released or the session ends. Unlike standard lock requests, session-level advisory lock requests do not honor transaction semantics"
[^adv-xact]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.5 Advisory Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "Transaction-level lock requests, on the other hand, behave more like regular lock requests: they are automatically released at the end of the transaction, and there is no explicit unlock operation."
[^discard]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), DISCARD ALL — <https://www.postgresql.org/docs/16/sql-discard.html> — "Releases all temporary resources associated with the current session and resets the session to its initial state."
[^sb-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 shared_buffers — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "Sets the amount of memory the database server uses for shared memory buffers. The default is typically 128 megabytes (128MB), but might be less if your kernel settings will not support it (as determined during initdb)."
[^sb-restart]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 shared_buffers — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "(Non-default values of BLCKSZ change the minimum value.) This parameter can only be set at server start."
[^sb-25]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 shared_buffers — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "If you have a dedicated database server with 1GB or more of RAM, a reasonable starting value for shared_buffers is 25% of the memory in your system."
[^sb-wal]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 shared_buffers — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "Larger settings for shared_buffers usually require a corresponding increase in max_wal_size, in order to spread out the process of writing large quantities of new or changed data over a longer period of time."
[^wm-default]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 work_mem — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "Sets the base maximum amount of memory to be used by a query operation (such as a sort or hash table) before writing to temporary disk files. If this value is specified without units, it is taken as kilobytes. The default value is four megabytes (4MB)."
[^wm-multi]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 work_mem — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "Note that a complex query might perform several sort and hash operations at the same time, with each operation generally being allowed to use as much memory as this value specifies before it starts to write data into temporary files."
[^wm-sorts]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 work_mem — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "Sort operations are used for ORDER BY, DISTINCT, and merge joins."
[^hash-mem]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 work_mem — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "The memory limit for a hash table is computed by multiplying work_mem by hash_mem_multiplier. This makes it possible for hash-based operations to use an amount of memory that exceeds the usual work_mem base amount."
[^hash-mem-default]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 hash_mem_multiplier — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "The default value is 2.0, which makes hash-based operations use twice the usual work_mem base amount."
[^hash-sens]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 work_mem — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "Hash-based operations are generally more sensitive to memory availability than equivalent sort-based operations."
[^par-wm]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.6 max_parallel_workers_per_gather — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "Resource limits such as work_mem are applied individually to each worker, which means the total utilization may be much higher across all processes than it would normally be for any single process."
[^mwm]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 maintenance_work_mem — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "Specifies the maximum amount of memory to be used by maintenance operations, such as VACUUM, CREATE INDEX, and ALTER TABLE ADD FOREIGN KEY. If this value is specified without units, it is taken as kilobytes. It defaults to 64 megabytes (64MB)."
[^mwm-av]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 maintenance_work_mem — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "Note that when autovacuum runs, up to autovacuum_max_workers times this memory may be allocated, so be careful not to set the default value too high. It may be useful to control for this by separately setting autovacuum_work_mem."
[^mwm-1gb]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 maintenance_work_mem — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "Note that for the collection of dead tuple identifiers, VACUUM is only able to utilize up to a maximum of 1GB of memory."
[^vl-header]: PostgreSQL REL_16_4, src/backend/access/heap/vacuumlazy.c, comment đầu file — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/vacuumlazy.c> — "We are willing to use at most maintenance_work_mem (or perhaps autovacuum_work_mem) memory space to keep track of dead TIDs."
[^vl-inefficient]: PostgreSQL REL_16_4, vacuumlazy.c, lazy_scan_heap — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/vacuumlazy.c> — "However, we process indexes in full every time lazy_vacuum is called, which makes index processing very inefficient when memory is in short supply."
[^ecs]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.7.2 effective_cache_size — <https://www.postgresql.org/docs/16/runtime-config-query.html> — "Sets the planner's assumption about the effective size of the disk cache that is available to a single query. This is factored into estimates of the cost of using an index; a higher value makes it more likely index scans will be used, a lower value makes it more likely sequential scans will be used."
[^ecs-both]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.7.2 effective_cache_size — <https://www.postgresql.org/docs/16/runtime-config-query.html> — "When setting this parameter you should consider both PostgreSQL's shared buffers and the portion of the kernel's disk cache that will be used for PostgreSQL data files, though some data might exist in both places."
[^ecs-noalloc]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.7.2 effective_cache_size — <https://www.postgresql.org/docs/16/runtime-config-query.html> — "This parameter has no effect on the size of shared memory allocated by PostgreSQL, nor does it reserve kernel disk cache; it is used only for estimation purposes."
[^blks-hit]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 pg_stat_database, cột blks_hit — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "Number of times disk blocks were found already in the buffer cache, so that a read was not necessary (this only includes hits in the PostgreSQL buffer cache, not the operating system's file system cache)"
[^bufpin]: PostgreSQL REL_16_4, src/backend/storage/buffer/README — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/buffer/README> — "An unpinned buffer is subject to being reclaimed and reused for a different page at any instant, so touching it is unsafe."
[^clock]: PostgreSQL REL_16_4, src/backend/storage/buffer/README — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/buffer/README> — "Each buffer header contains a usage counter, which is incremented (up to a small limit value) whenever the buffer is pinned."
[^dirty-first]: PostgreSQL REL_16_4, src/backend/storage/buffer/README — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/buffer/README> — "if the selected buffer is dirty, we will have to write it out before we can recycle it"
[^bgw-reduce]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.5 Background Writer — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "This reduces the likelihood that server processes handling user queries will be unable to find clean buffers and have to write dirty buffers themselves."
[^explain-sort]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.2 EXPLAIN ANALYZE — <https://www.postgresql.org/docs/16/using-explain.html> — "The Sort node shows the sort method used (in particular, whether the sort was in-memory or on-disk) and the amount of memory or disk space needed. The Hash node shows the number of hash buckets and batches as well as the peak amount of memory used for the hash table. (If the number of batches exceeds one, there will also be disk space usage involved, but that is not shown.)"
[^db-temp]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 pg_stat_database, cột temp_files — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "Number of temporary files created by queries in this database. All temporary files are counted, regardless of why the temporary file was created (e.g., sorting or hashing), and regardless of the log_temp_files setting."
[^log-temp]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.8.3 log_temp_files — <https://www.postgresql.org/docs/16/runtime-config-logging.html> — "Temporary files can be created for sorts, hashes, and temporary query results. If enabled by this setting, a log entry is emitted for each temporary file, with the file size specified in bytes, when it is deleted."
[^temp-limit]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.2 temp_file_limit — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "Specifies the maximum amount of disk space that a process can use for temporary files, such as sort and hash temporary files, or the storage file for a held cursor. A transaction attempting to exceed this limit will be canceled."
[^ring]: PostgreSQL REL_16_4, src/backend/storage/buffer/README, Buffer Ring Replacement Strategy — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/buffer/README> — "When running a query that needs to access a large number of pages just once, such as VACUUM or a large sequential scan, a different strategy is used."
[^ring-256]: PostgreSQL REL_16_4, src/backend/storage/buffer/README, Buffer Ring Replacement Strategy — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/buffer/README> — "For sequential scans, a 256KB ring is used. That's small enough to fit in L2 cache, which makes transferring pages from OS cache to shared buffer cache efficient."
[^ring-write]: PostgreSQL REL_16_4, src/backend/storage/buffer/README, Buffer Ring Replacement Strategy — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/buffer/README> — "This also implies that much of the write traffic caused by such a statement will be done by the backend itself and not pushed off onto other processes."
[^vbul]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 vacuum_buffer_usage_limit — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "Specifies the size of the Buffer Access Strategy used by the VACUUM and ANALYZE commands. A setting of 0 will allow the operation to use any number of shared_buffers."
[^shm-type]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 huge_pages — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "On Linux, it is only supported when shared_memory_type is set to mmap (the default)."
[^huge-try]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 huge_pages — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "With huge_pages set to try, the server will try to request huge pages, but fall back to the default if that fails."
[^thp]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 huge_pages — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "That feature has been known to cause performance degradation with PostgreSQL for some users on some Linux versions, so its use is currently discouraged (unlike explicit use of huge_pages)."
[^wal-buf]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.5.1 wal_buffers — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "The default setting of -1 selects a size equal to 1/32nd (about 3%) of shared_buffers, but not less than 64kB nor more than the size of one WAL segment, typically 16MB."
[^lk-rowexcl]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.1 Table-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "The commands UPDATE, DELETE, INSERT, and MERGE acquire this lock mode on the target table (in addition to ACCESS SHARE locks on any other referenced tables)."
[^lk-nokey]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.2 Row-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "Behaves similarly to FOR UPDATE, except that the lock acquired is weaker: this lock will not block SELECT FOR KEY SHARE commands that attempt to acquire a lock on the same rows. This lock mode is also acquired by any UPDATE that does not acquire a FOR UPDATE lock."
[^lk-forupd]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.2 Row-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "The FOR UPDATE lock mode is also acquired by any DELETE on a row, and also by an UPDATE that modifies the values of certain columns."
[^hu-bufs]: PostgreSQL REL_16_4, src/backend/access/heap/heapam.c, heap_update — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/heapam.c> — "At this point newbuf and buffer are both pinned and locked, and newbuf has enough space for the new tuple. If they are the same buffer, only one pin is held."
[^hu-hot]: PostgreSQL REL_16_4, heapam.c, heap_update — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/heapam.c> — "Since the new tuple is going into the same page, we might be able to do a HOT update. Check if any of the index columns have been changed."
[^hu-put]: PostgreSQL REL_16_4, heapam.c, heap_update — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/heapam.c> — "RelationPutHeapTuple(relation, newbuf, heaptup, false); /* insert new tuple */"
[^hu-xmax]: PostgreSQL REL_16_4, heapam.c, heap_update — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/heapam.c> — "and store info about transaction updating this tuple"
[^hu-ctid]: PostgreSQL REL_16_4, heapam.c, heap_update — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/heapam.c> — "record address of new tuple in t_ctid of old one"
[^hu-vm]: PostgreSQL REL_16_4, heapam.c, heap_update — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/heapam.c> — "clear PD_ALL_VISIBLE flags, reset all visibilitymap bits"
[^hu-wal]: PostgreSQL REL_16_4, heapam.c, heap_update — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/heapam.c> — "recptr = log_heap_update(relation, buffer, newbuf, &oldtup, heaptup, old_key_tuple, all_visible_cleared, all_visible_cleared_new);"
[^hot-doc]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 73.7 Heap-Only Tuples (HOT) — <https://www.postgresql.org/docs/16/storage-hot.html> — "The update does not modify any columns referenced by the table's indexes, not including summarizing indexes. The only summarizing index method in the core PostgreSQL distribution is BRIN. There is sufficient free space on the page containing the old row for the updated row."
[^hot-cost]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 73.7 Heap-Only Tuples (HOT) — <https://www.postgresql.org/docs/16/storage-hot.html> — "Specifically, updates require new versions of rows to be added to tables. This can also require new index entries for each updated row, and removal of old versions of rows and their index entries can be expensive."
[^hot-prune]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 73.7 Heap-Only Tuples (HOT) — <https://www.postgresql.org/docs/16/storage-hot.html> — "Old versions of updated rows can be completely removed during normal operation, including SELECTs, instead of requiring periodic vacuum operations. (This is possible because indexes do not reference their page item identifiers.)"
[^hot-fill]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 73.7 Heap-Only Tuples (HOT) — <https://www.postgresql.org/docs/16/storage-hot.html> — "You can increase the likelihood of sufficient page space for HOT updates by decreasing a table's fillfactor. If you don't, HOT updates will still happen because new rows will naturally migrate to new pages and existing pages with sufficient free space for new row versions. The system view pg_stat_all_tables allows monitoring of the occurrence of HOT and non-HOT updates."
[^hot-chain]: PostgreSQL REL_16_4, src/backend/access/heap/README.HOT — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/README.HOT> — "With HOT, a new tuple placed on the same page and with all indexed columns the same as its parent row version does not get new index entries. This means there is only one index entry for the entire update chain on the heap page."
[^hot-end]: PostgreSQL REL_16_4, src/backend/access/heap/README.HOT — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/README.HOT> — "If an update changes any column indexed by a non-summarizing indexes, or if there is not room on the same page for the new tuple, then the HOT chain ends"
[^wal-insert]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 30.5 WAL Configuration — <https://www.postgresql.org/docs/16/wal-configuration.html> — "XLogInsertRecord is used on every database low level modification (for example, row insertion) at a time when an exclusive lock is held on affected data pages, so the operation needs to be as fast as possible."
[^wal-flush-commit]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 30.5 WAL Configuration — <https://www.postgresql.org/docs/16/wal-configuration.html> — "Normally, WAL buffers should be written and flushed by an XLogFlush request, which is made, for the most part, at transaction commit time to ensure that transaction records are flushed to permanent storage."
[^xact-flush]: PostgreSQL REL_16_4, src/backend/access/transam/xact.c, RecordTransactionCommit — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/transam/xact.c> — "synchronous_commit > SYNCHRONOUS_COMMIT_OFF) || forceSyncCommit || nrels > 0) { XLogFlush(XactLastRecEnd);"
[^wal-intro]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 30.3 Write-Ahead Logging (WAL) — <https://www.postgresql.org/docs/16/wal-intro.html> — "changes to data files (where tables and indexes reside) must be written only after those changes have been logged, that is, after WAL records describing the changes have been flushed to permanent storage."
[^wal-group]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 30.3 Write-Ahead Logging (WAL) — <https://www.postgresql.org/docs/16/wal-intro.html> — "Furthermore, when the server is processing many small concurrent transactions, one fsync of the WAL file may suffice to commit many transactions."
[^wal-sync-default]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.5.1 wal_sync_method — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "The default is the first method in the above list that is supported by the platform, except that fdatasync is the default on Linux and FreeBSD."
[^ckpt-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 30.5 WAL Configuration — <https://www.postgresql.org/docs/16/wal-configuration.html> — "At checkpoint time, all dirty data pages are flushed to disk and a special checkpoint record is written to the WAL file. (The change records were previously flushed to the WAL files.)"
[^ckpt-redo]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 30.5 WAL Configuration — <https://www.postgresql.org/docs/16/wal-configuration.html> — "In the event of a crash, the crash recovery procedure looks at the latest checkpoint record to determine the point in the WAL (known as the redo record) from which it should start the REDO operation."
[^ckpt-tradeoff]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 30.5 WAL Configuration — <https://www.postgresql.org/docs/16/wal-configuration.html> — "Reducing checkpoint_timeout and/or max_wal_size causes checkpoints to occur more often. This allows faster after-crash recovery, since less work will need to be redone. However, one must balance this against the increased cost of flushing dirty data pages more often."
[^ckpt-defaults]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 30.5 WAL Configuration — <https://www.postgresql.org/docs/16/wal-configuration.html> — "The default settings are 5 minutes and 1 GB, respectively."
[^ckpt-spread]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 30.5 WAL Configuration — <https://www.postgresql.org/docs/16/wal-configuration.html> — "checkpoint activity is throttled so that I/O begins at checkpoint start and completes before the next checkpoint is due to start; this minimizes performance degradation during checkpoints."
[^ckpt-target]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.5.2 checkpoint_completion_target — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "Specifies the target of checkpoint completion, as a fraction of total time between checkpoints. The default is 0.9"
[^ckpt-expensive]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 30.5 WAL Configuration — <https://www.postgresql.org/docs/16/wal-configuration.html> — "Checkpoints are fairly expensive, first because they require writing out all currently dirty buffers, and second because they result in extra subsequent WAL traffic as discussed above."
[^ckpt-fpw]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 30.5 WAL Configuration — <https://www.postgresql.org/docs/16/wal-configuration.html> — "In that case, a smaller checkpoint interval increases the volume of output to the WAL, partially negating the goal of using a smaller interval, and in any case causing more disk I/O."
[^fpw-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.5.1 full_page_writes — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "When this parameter is on, the PostgreSQL server writes the entire content of each disk page to WAL during the first modification of that page after a checkpoint."
[^fpw-off]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.5.1 full_page_writes — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "Turning this parameter off speeds normal operation, but might lead to either unrecoverable data corruption, or silent data corruption, after a system failure. The risks are similar to turning off fsync, though smaller"
[^wal-compress]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.5.1 wal_compression — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "Enabling compression can reduce the WAL volume without increasing the risk of unrecoverable data corruption, but at the cost of some extra CPU spent on the compression during WAL logging and on the decompression during WAL replay."
[^wal-compress-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.5.1 wal_compression — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "The value on is a historical spelling of pglz. The default value is off."
[^commit-delay]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 30.5 WAL Configuration — <https://www.postgresql.org/docs/16/wal-configuration.html> — "When commit_delay is set to zero (the default), it is still possible for a form of group commit to occur, but each group will consist only of sessions that reach the point where they need to flush their commit records during the window in which the previous flush operation (if any) is occurring."
[^sync-commit-off]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.5.1 synchronous_commit — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "In off mode, there is no waiting, so there can be a delay between when success is reported to the client and when the transaction is later guaranteed to be safe against a server crash. (The maximum delay is three times wal_writer_delay.) Unlike fsync, setting this parameter to off does not create any risk of database inconsistency"
[^walw-delay-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.5.1 wal_writer_delay — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "If this value is specified without units, it is taken as milliseconds. The default value is 200 milliseconds (200ms)."
[^walw-flush-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.5.1 wal_writer_flush_after — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "If this value is specified without units, it is taken as WAL blocks, that is XLOG_BLCKSZ bytes, typically 8kB. The default is 1MB."
[^vac-reasons]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.1 Vacuuming Basics — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "To recover or reuse disk space occupied by updated or deleted rows. To update data statistics used by the PostgreSQL query planner. To update the visibility map, which speeds up index-only scans. To protect against loss of very old data due to transaction ID wraparound or multixact ID wraparound."
[^vac-nodelete]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.2 Recovering Disk Space — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "In PostgreSQL, an UPDATE or DELETE of a row does not immediately remove the old version of the row. This approach is necessary to gain the benefits of multiversion concurrency control (MVCC, see Chapter 13): the row version must not be deleted while it is still potentially visible to other transactions."
[^vac-space]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.2 Recovering Disk Space — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "The standard form of VACUUM removes dead row versions in tables and indexes and marks the space available for future reuse. However, it will not return the space to the operating system, except in the special case where one or more pages at the end of a table become entirely free and an exclusive table lock can be easily obtained."
[^vac-full]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.1 Vacuuming Basics — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "VACUUM FULL requires an ACCESS EXCLUSIVE lock on the table it is working on, and therefore cannot be done in parallel with other use of the table."
[^vac-steady]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.2 Recovering Disk Space — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "In this approach, the idea is not to keep tables at their minimum size, but to maintain steady-state usage of disk space: each table occupies space equivalent to its minimum size plus however much space gets used up between vacuum runs."
[^vac-concurrent]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.1 Vacuuming Basics — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "Also, the standard form of VACUUM can run in parallel with production database operations. (Commands such as SELECT, INSERT, UPDATE, and DELETE will continue to function normally, though you will not be able to modify the definition of a table with commands such as ALTER TABLE while it is being vacuumed.)"
[^vl-phases]: PostgreSQL REL_16_4, src/backend/access/heap/vacuumlazy.c, lazy_scan_heap — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/vacuumlazy.c> — "Also invokes lazy_vacuum_all_indexes to vacuum indexes, which largely consists of deleting index tuples that point to LP_DEAD items left in heap pages following pruning. Earlier initial pass over the heap will have collected the TIDs whose index tuples need to be removed."
[^vl-invariant]: PostgreSQL REL_16_4, vacuumlazy.c, lazy_scan_heap — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/vacuumlazy.c> — "no extant index tuple can ever be allowed to contain a TID that points to an LP_UNUSED line pointer in the heap."
[^vm-bits]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 73.4 Visibility Map — <https://www.postgresql.org/docs/16/storage-vm.html> — "The visibility map stores two bits per heap page. The first bit, if set, indicates that the page is all-visible, or in other words that the page does not contain any tuples that need to be vacuumed."
[^vm-uses]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.4 Updating the Visibility Map — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "First, vacuum itself can skip such pages on the next run, since there is nothing to clean up. Second, it allows PostgreSQL to answer some queries using only the index, without reference to the underlying table."
[^av-trig]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.6 The Autovacuum Daemon — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "When enabled, autovacuum checks for tables that have had a large number of inserted, updated or deleted tuples."
[^av-naptime]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.10 autovacuum_naptime — <https://www.postgresql.org/docs/16/runtime-config-autovacuum.html> — "In each round the daemon examines the database and issues VACUUM and ANALYZE commands as needed for tables in that database. If this value is specified without units, it is taken as seconds. The default is one minute (1min)."
[^av-workers]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.10 autovacuum_max_workers — <https://www.postgresql.org/docs/16/runtime-config-autovacuum.html> — "Specifies the maximum number of autovacuum processes (other than the autovacuum launcher) that may be running at any one time. The default is three. This parameter can only be set at server start."
[^av-formula]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.6 The Autovacuum Daemon — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "vacuum threshold = vacuum base threshold + vacuum scale factor * number of tuples where the vacuum base threshold is autovacuum_vacuum_threshold, the vacuum scale factor is autovacuum_vacuum_scale_factor, and the number of tuples is pg_class.reltuples."
[^av-thr-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.10 autovacuum_vacuum_threshold — <https://www.postgresql.org/docs/16/runtime-config-autovacuum.html> — "Specifies the minimum number of updated or deleted tuples needed to trigger a VACUUM in any one table. The default is 50 tuples."
[^av-scale-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.10 autovacuum_vacuum_scale_factor — <https://www.postgresql.org/docs/16/runtime-config-autovacuum.html> — "Specifies a fraction of the table size to add to autovacuum_vacuum_threshold when deciding whether to trigger a VACUUM. The default is 0.2 (20% of table size)."
[^av-insert]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.6 The Autovacuum Daemon — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "vacuum insert threshold = vacuum base insert threshold + vacuum insert scale factor * number of tuples"
[^av-insert-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.10 autovacuum_vacuum_insert_threshold — <https://www.postgresql.org/docs/16/runtime-config-autovacuum.html> — "Specifies the number of inserted tuples needed to trigger a VACUUM in any one table. The default is 1000 tuples."
[^av-insert-scale]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.10 autovacuum_vacuum_insert_scale_factor — <https://www.postgresql.org/docs/16/runtime-config-autovacuum.html> — "Specifies a fraction of the table size to add to autovacuum_vacuum_insert_threshold when deciding whether to trigger a VACUUM. The default is 0.2 (20% of table size)."
[^av-analyze]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.6 The Autovacuum Daemon — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "analyze threshold = analyze base threshold + analyze scale factor * number of tuples"
[^av-analyze-thr]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.10 autovacuum_analyze_threshold — <https://www.postgresql.org/docs/16/runtime-config-autovacuum.html> — "Specifies the minimum number of inserted, updated or deleted tuples needed to trigger an ANALYZE in any one table. The default is 50 tuples."
[^av-analyze-scale]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.10 autovacuum_analyze_scale_factor — <https://www.postgresql.org/docs/16/runtime-config-autovacuum.html> — "Specifies a fraction of the table size to add to autovacuum_analyze_threshold when deciding whether to trigger an ANALYZE. The default is 0.1 (10% of table size)."
[^av-pertable]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.6 The Autovacuum Daemon — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "The default thresholds and scale factors are taken from postgresql.conf, but it is possible to override them (and many other autovacuum control parameters) on a per-table basis"
[^cost-delay]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.4 Cost-based Vacuum Delay — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "When the accumulated cost reaches a limit (specified by vacuum_cost_limit), the process performing the operation will sleep for a short period of time, as specified by vacuum_cost_delay."
[^av-cost-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.10 autovacuum_vacuum_cost_delay — <https://www.postgresql.org/docs/16/runtime-config-autovacuum.html> — "If this value is specified without units, it is taken as milliseconds. The default value is 2 milliseconds."
[^av-balance]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.6 The Autovacuum Daemon — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "When multiple workers are running, the autovacuum cost delay parameters (see Section 20.4.4) are “balanced” among all the running workers, so that the total I/O impact on the system is the same regardless of the number of workers actually running."
[^av-interrupt]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.6 The Autovacuum Daemon — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "Autovacuum workers generally don't block other commands. If a process attempts to acquire a lock that conflicts with the SHARE UPDATE EXCLUSIVE lock held by autovacuum, lock acquisition will interrupt the autovacuum."
[^av-wrap-nointr]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.6 The Autovacuum Daemon — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "However, if the autovacuum is running to prevent transaction ID wraparound (i.e., the autovacuum query name in the pg_stat_activity view ends with (to prevent wraparound)), the autovacuum is not automatically interrupted."
[^av-warn]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.6 The Autovacuum Daemon — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "Regularly running commands that acquire locks conflicting with a SHARE UPDATE EXCLUSIVE lock (e.g., ANALYZE) can effectively prevent autovacuums from ever completing."
[^av-partitioned]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.6 The Autovacuum Daemon — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "Unfortunately, this means that autovacuum does not run ANALYZE on partitioned tables, and this can cause suboptimal plans for queries that reference partitioned table statistics."
[^vl-cutoff]: PostgreSQL REL_16_4, vacuumlazy.c, heap_vacuum_rel — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/vacuumlazy.c> — "Get cutoffs that determine which deleted tuples are considered DEAD, not just RECENTLY_DEAD, and which XIDs/MXIDs to freeze."
[^vl-vistest]: PostgreSQL REL_16_4, vacuumlazy.c, heap_vacuum_rel — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/vacuumlazy.c> — "We expect vistest will always make heap_page_prune remove any deleted tuple whose xmax is < OldestXmin."
[^ph-backend-xmin]: PostgreSQL REL_16_4, src/backend/storage/ipc/procarray.c, ComputeXidHorizons — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/ipc/procarray.c> — "h->data_oldest_nonremovable = TransactionIdOlder(h->data_oldest_nonremovable, xmin);"
[^ph-slot-xmin]: PostgreSQL REL_16_4, procarray.c, ComputeXidHorizons — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/ipc/procarray.c> — "h->data_oldest_nonremovable = TransactionIdOlder(h->data_oldest_nonremovable, h->slot_xmin);"
[^ph-data-oldest]: PostgreSQL REL_16_4, procarray.c, ComputeXidHorizonsResult — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/ipc/procarray.c> — "Oldest xid for which deleted tuples need to be retained in normal user defined tables."
[^bxmin]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 pg_stat_activity, cột backend_xmin — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "The current backend's xmin horizon."
[^vl-log]: PostgreSQL REL_16_4, vacuumlazy.c, log của VACUUM VERBOSE/autovacuum — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/vacuumlazy.c> — "tuples: %lld removed, %lld remain, %lld are dead but not yet removable"
[^iit]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.11 idle_in_transaction_session_timeout — <https://www.postgresql.org/docs/16/runtime-config-client.html> — "Even when no significant locks are held, an open transaction prevents vacuuming away recently-dead tuples that may be visible only to this transaction; so remaining idle for a long time can contribute to table bloat."
[^hsf]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.6.3 hot_standby_feedback — <https://www.postgresql.org/docs/16/runtime-config-replication.html> — "This parameter can be used to eliminate query cancels caused by cleanup records, but can cause database bloat on the primary for some workloads. Feedback messages will not be sent more frequently than once per wal_receiver_status_interval. The default value is off."
[^wrap-prepared]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.5 Preventing Transaction ID Wraparound Failures — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "Resolve old prepared transactions. You can find these by checking pg_prepared_xacts for rows where age(transactionid) is large. Such transactions should be committed or rolled back."
[^wrap-longtx]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.5 Preventing Transaction ID Wraparound Failures — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "End long-running open transactions. You can find these by checking pg_stat_activity for rows where age(backend_xid) or age(backend_xmin) is large."
[^wrap-slots]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.5 Preventing Transaction ID Wraparound Failures — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "Drop any old replication slots. Use pg_replication_slots to find slots where age(xmin) or age(catalog_xmin) is large."
[^xid-32]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.5 Preventing Transaction ID Wraparound Failures — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "But since transaction IDs have limited size (32 bits) a cluster that runs for a long time (more than 4 billion transactions) would suffer transaction ID wraparound: the XID counter wraps around to zero, and all of a sudden transactions that were in the past appear to be in the future"
[^xid-modulo]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.5 Preventing Transaction ID Wraparound Failures — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "This means that for every normal XID, there are two billion XIDs that are “older” and two billion that are “newer”; another way to say it is that the normal XID space is circular with no endpoint."
[^xid-freeze]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.5 Preventing Transaction ID Wraparound Failures — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "VACUUM will mark rows as frozen, indicating that they were inserted by a transaction that committed sufficiently far in the past that the effects of the inserting transaction are certain to be visible to all current and future transactions."
[^freeze-flag]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.5 Preventing Transaction ID Wraparound Failures — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "Newer versions just set a flag bit, preserving the row's original xmin for possible forensic use."
[^freeze-min]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.11 vacuum_freeze_min_age — <https://www.postgresql.org/docs/16/runtime-config-client.html> — "Specifies the cutoff age (in transactions) that VACUUM should use to decide whether to trigger freezing of pages that have an older XID. The default is 50 million transactions."
[^freeze-table]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.11 vacuum_freeze_table_age — <https://www.postgresql.org/docs/16/runtime-config-client.html> — "VACUUM performs an aggressive scan if the table's pg_class.relfrozenxid field has reached the age specified by this setting. An aggressive scan differs from a regular VACUUM in that it visits every page that might contain unfrozen XIDs or MXIDs, not just those that might contain dead tuples. The default is 150 million transactions."
[^freeze-max]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.10 autovacuum_freeze_max_age — <https://www.postgresql.org/docs/16/runtime-config-autovacuum.html> — "Specifies the maximum age (in transactions) that a table's pg_class.relfrozenxid field can attain before a VACUUM operation is forced to prevent transaction ID wraparound within the table. Note that the system will launch autovacuum processes to prevent wraparound even when autovacuum is otherwise disabled."
[^freeze-max-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.10 autovacuum_freeze_max_age — <https://www.postgresql.org/docs/16/runtime-config-autovacuum.html> — "Vacuum also allows removal of old files from the pg_xact subdirectory, which is why the default is a relatively low 200 million transactions."
[^failsafe]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.11 vacuum_failsafe_age — <https://www.postgresql.org/docs/16/runtime-config-client.html> — "When the failsafe is triggered, any cost-based delay that is in effect will no longer be applied, further non-essential maintenance tasks (such as index vacuuming) are bypassed"
[^failsafe-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.11 vacuum_failsafe_age — <https://www.postgresql.org/docs/16/runtime-config-client.html> — "The default is 1.6 billion transactions."
[^aggressive]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.5 Preventing Transaction ID Wraparound Failures — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "Therefore, normal VACUUMs won't always freeze every old row version in the table. When that happens, VACUUM will eventually need to perform an aggressive vacuum, which will freeze all eligible unfrozen XID and MXID values, including those from all-visible but not all-frozen pages. In practice most tables require periodic aggressive vacuuming."
[^frozen-cols]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.5 Preventing Transaction ID Wraparound Failures — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "Similarly, the datfrozenxid column of a database's pg_database row is a lower bound on the unfrozen XIDs appearing in that database — it is just the minimum of the per-table relfrozenxid values within the database."
[^age-col]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.5 Preventing Transaction ID Wraparound Failures — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "The age column measures the number of transactions from the cutoff XID to the current transaction's XID."
[^wrap-warn]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.5 Preventing Transaction ID Wraparound Failures — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "If for some reason autovacuum fails to clear old XIDs from a table, the system will begin to emit warning messages like this when the database's oldest XIDs reach forty million transactions from the wraparound point"
[^wrap-threshold]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.5 Preventing Transaction ID Wraparound Failures — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "If these warnings are ignored, the system will refuse to assign new XIDs once there are fewer than three million transactions left until wraparound"
[^wrap-refuse]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.5 Preventing Transaction ID Wraparound Failures — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "In this condition any transactions already in progress can continue, but only read-only transactions can be started. Operations that modify database records or truncate relations will fail. The VACUUM command can still be run normally."
[^wrap-novacfull]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.5 Preventing Transaction ID Wraparound Failures — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "Do not use VACUUM FULL in this scenario, because it requires an XID and will therefore fail, except in super-user mode, where it will instead consume an XID and thus increase the risk of transaction ID wraparound."
[^wrap-nosingle]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.5 Preventing Transaction ID Wraparound Failures — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "Contrary to what the hint states, it is not necessary or desirable to stop the postmaster or enter single user-mode in order to restore normal operation."
[^idx-cleanup]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), VACUUM, tùy chọn INDEX_CLEANUP — <https://www.postgresql.org/docs/16/sql-vacuum.html> — "Normally, VACUUM will skip index vacuuming when there are very few dead tuples in the table. The cost of processing all of the table's indexes is expected to greatly exceed the benefit of removing dead index tuples when this happens."
[^vl-bypass]: PostgreSQL REL_16_4, vacuumlazy.c — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/vacuumlazy.c> — "Threshold that controls whether we bypass index vacuuming and heap vacuuming as an optimization #define BYPASS_THRESHOLD_PAGES 0.02 /* i.e. 2% of rel_pages */"
[^vac-truncate]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), VACUUM, tùy chọn TRUNCATE — <https://www.postgresql.org/docs/16/sql-vacuum.html> — "Setting this option to false may be useful to avoid ACCESS EXCLUSIVE lock on the table that the truncation requires."
[^vac-parallel]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), VACUUM, tùy chọn PARALLEL — <https://www.postgresql.org/docs/16/sql-vacuum.html> — "The number of workers used to perform the operation is equal to the number of indexes on the relation that support parallel vacuum which is limited by the number of workers specified with PARALLEL option if any which is further limited by max_parallel_maintenance_workers."
[^multixact]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.5.1 Multixacts and Wraparound — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "Multixact IDs are used to support row locking by multiple transactions."
[^mxid-subset]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.5.1 Multixacts and Wraparound — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "XID exhaustion will block all write transactions, but MXID exhaustion will only block a subset of write transactions, specifically those that involve row locks that require an MXID."
[^vac-progress]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), VACUUM, mục Notes — <https://www.postgresql.org/docs/16/sql-vacuum.html> — "Each backend running VACUUM without the FULL option will report its progress in the pg_stat_progress_vacuum view."
[^av-log]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.8.2 log_autovacuum_min_duration — <https://www.postgresql.org/docs/16/runtime-config-logging.html> — "The default is 10min. Enabling this parameter can be helpful in tracking autovacuum activity."
[^lk-intro]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.1 Table-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "Remember that all of these lock modes are table-level locks, even if the name contains the word “row”; the names of the lock modes are historical."
[^lk-intro2]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.1 Table-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "The only real difference between one lock mode and another is the set of lock modes with which each conflicts (see Table 13.2). Two transactions cannot hold locks of conflicting modes on the same table at the same time."
[^lk-hold]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.1 Table-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "Once acquired, a lock is normally held until the end of the transaction. But if a lock is acquired after establishing a savepoint, the lock is released immediately if the savepoint is rolled back to."
[^lk-tip]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.1 Table-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "Only an ACCESS EXCLUSIVE lock blocks a SELECT (without FOR UPDATE/SHARE) statement."
[^lk-as]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.1 Table-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "ACCESS SHARE (AccessShareLock) Conflicts with the ACCESS EXCLUSIVE lock mode only. The SELECT command acquires a lock of this mode on referenced tables."
[^lk-rs]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.1 Table-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "ROW SHARE (RowShareLock) Conflicts with the EXCLUSIVE and ACCESS EXCLUSIVE lock modes. The SELECT command acquires a lock of this mode on all tables on which one of the FOR UPDATE, FOR NO KEY UPDATE, FOR SHARE, or FOR KEY SHARE options is specified"
[^lk-re]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.1 Table-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "ROW EXCLUSIVE (RowExclusiveLock) Conflicts with the SHARE, SHARE ROW EXCLUSIVE, EXCLUSIVE, and ACCESS EXCLUSIVE lock modes."
[^lk-sue]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.1 Table-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "Conflicts with the SHARE UPDATE EXCLUSIVE, SHARE, SHARE ROW EXCLUSIVE, EXCLUSIVE, and ACCESS EXCLUSIVE lock modes. This mode protects a table against concurrent schema changes and VACUUM runs. Acquired by VACUUM (without FULL), ANALYZE, CREATE INDEX CONCURRENTLY, CREATE STATISTICS, COMMENT ON, REINDEX CONCURRENTLY, and certain ALTER INDEX and ALTER TABLE variants"
[^lk-s]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.1 Table-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "Conflicts with the ROW EXCLUSIVE, SHARE UPDATE EXCLUSIVE, SHARE ROW EXCLUSIVE, EXCLUSIVE, and ACCESS EXCLUSIVE lock modes. This mode protects a table against concurrent data changes. Acquired by CREATE INDEX (without CONCURRENTLY)."
[^lk-sre]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.1 Table-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "Conflicts with the ROW EXCLUSIVE, SHARE UPDATE EXCLUSIVE, SHARE, SHARE ROW EXCLUSIVE, EXCLUSIVE, and ACCESS EXCLUSIVE lock modes. This mode protects a table against concurrent data changes, and is self-exclusive so that only one session can hold it at a time. Acquired by CREATE TRIGGER and some forms of ALTER TABLE."
[^lk-e]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.1 Table-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "This mode allows only concurrent ACCESS SHARE locks, i.e., only reads from the table can proceed in parallel with a transaction holding this lock mode. Acquired by REFRESH MATERIALIZED VIEW CONCURRENTLY."
[^lk-ae]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.1 Table-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "This mode guarantees that the holder is the only transaction accessing the table in any way. Acquired by the DROP TABLE, TRUNCATE, REINDEX, CLUSTER, VACUUM FULL, and REFRESH MATERIALIZED VIEW (without CONCURRENTLY) commands. Many forms of ALTER INDEX and ALTER TABLE also acquire a lock at this level."
[^lk-row]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.2 Row-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "Row-level locks do not affect data querying; they block only writers and lockers to the same row. Row-level locks are released at transaction end or during savepoint rollback, just like table-level locks."
[^lk-row-disk]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.2 Row-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "PostgreSQL doesn't remember any information about modified rows in memory, so there is no limit on the number of rows locked at one time. However, locking a row might cause a disk write, e.g., SELECT FOR UPDATE modifies selected rows to mark them locked, and so will result in disk writes."
[^lk-page]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.3 Page-Level Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "page-level share/exclusive locks are used to control read/write access to table pages in the shared buffer pool. These locks are released immediately after a row is fetched or updated. Application developers normally need not be concerned with page-level locks"
[^pglocks-row]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 54.12 pg_locks — <https://www.postgresql.org/docs/16/view-pg-locks.html> — "Although tuples are a lockable type of object, information about row-level locks is stored on disk, not in memory, and therefore row-level locks normally do not appear in this view. If a process is waiting for a row-level lock, it will usually appear in the view as waiting for the permanent transaction ID of the current holder of that row lock."
[^pglocks-xid]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 54.12 pg_locks — <https://www.postgresql.org/docs/16/view-pg-locks.html> — "If a permanent ID is assigned to the transaction (which normally happens only if the transaction changes the state of the database), it also holds an exclusive lock on the transaction's permanent transaction ID until it ends. When a process finds it necessary to wait specifically for another transaction to end, it does so by attempting to acquire share lock on the other transaction's ID"
[^pglocks-prep]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 54.12 pg_locks — <https://www.postgresql.org/docs/16/view-pg-locks.html> — "A prepared transaction can never be waiting for a lock, but it continues to hold the locks it acquired while running."
[^pglocks-pid]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 54.12 pg_locks, cột pid — <https://www.postgresql.org/docs/16/view-pg-locks.html> — "Process ID of the server process holding or awaiting this lock, or null if the lock is held by a prepared transaction"
[^we-xid]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 Lock Wait Events — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "transactionid Waiting for a transaction to finish."
[^we-lwlock]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 pg_stat_activity, wait_event_type LWLock — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "The server process is waiting for a lightweight lock. Most such locks protect a particular data structure in shared memory."
[^lock-rule1]: PostgreSQL REL_16_4, src/backend/storage/lmgr/README, Lock acquisition — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/lmgr/README> — "A lock request is granted immediately if it does not conflict with any existing or waiting lock request, or if the process already holds an instance of the same lock type"
[^lock-order]: PostgreSQL REL_16_4, src/backend/storage/lmgr/README — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/lmgr/README> — "Each waiter is awoken if (a) its request does not conflict with already-granted locks, and (b) its request does not conflict with the requests of prior un-wakable waiters. Rule (b) ensures that conflicting requests are granted in order of arrival."
[^blockpids]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 9.26.1 Session Information Functions, pg_blocking_pids — <https://www.postgresql.org/docs/16/functions-info.html> — "One server process blocks another if it either holds a lock that conflicts with the blocked process's lock request (hard block), or is waiting for a lock that would conflict with the blocked process's lock request and is ahead of it in the wait queue (soft block)."
[^lock-optimistic]: PostgreSQL REL_16_4, src/backend/storage/lmgr/README, The Deadlock Detection Algorithm — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/lmgr/README> — "We do this using an "optimistic waiting" approach: if a process cannot acquire the lock it wants immediately, it goes to sleep without any deadlock check. But it also sets a delay timer, with a delay of DeadlockTimeout milliseconds (typically set to one second). If the delay expires before the process is granted the lock it wants, it runs the deadlock detection/breaking code."
[^lock-wfg]: PostgreSQL REL_16_4, src/backend/storage/lmgr/README — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/lmgr/README> — "To perform deadlock checking, we use the standard method of viewing the various processes as nodes in a directed graph (the waits-for graph or WFG)."
[^lock-resolve]: PostgreSQL REL_16_4, src/backend/storage/lmgr/README — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/lmgr/README> — "But if a deadlock condition does exist, it will be resolved, usually by aborting the detecting process' transaction."
[^lock-soft]: PostgreSQL REL_16_4, src/backend/storage/lmgr/README — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/lmgr/README> — "A "soft" block, or wait-priority block, has the same potential for inducing deadlock as a hard block. However, we may be able to resolve a soft block without aborting the transactions involved: we can instead rearrange the order of the wait queue."
[^dl-row]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.4 Deadlocks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "Note that deadlocks can also occur as the result of row-level locks (and thus, they can occur even if explicit locking is not used)."
[^dl-order]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.4 Deadlocks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "The best defense against deadlocks is generally to avoid them by being certain that all applications using a database acquire locks on multiple objects in a consistent order."
[^dl-timeout]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.12 deadlock_timeout — <https://www.postgresql.org/docs/16/runtime-config-locks.html> — "The check for deadlock is relatively expensive, so the server doesn't run it every time it waits for a lock. We optimistically assume that deadlocks are not common in production applications and just wait on the lock for a while before checking for a deadlock."
[^dl-timeout-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.12 deadlock_timeout — <https://www.postgresql.org/docs/16/runtime-config-locks.html> — "The default is one second (1s), which is probably about the smallest value you would want in practice. On a heavily loaded server you might want to raise it."
[^dl-logwait]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.12 deadlock_timeout — <https://www.postgresql.org/docs/16/runtime-config-locks.html> — "When log_lock_waits is set, this parameter also determines the amount of time to wait before a log message is issued about the lock wait. If you are trying to investigate locking delays you might want to set a shorter than normal deadlock_timeout."
[^log-lock-waits]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.8.3 log_lock_waits — <https://www.postgresql.org/docs/16/runtime-config-logging.html> — "Controls whether a log message is produced when a session waits longer than deadlock_timeout to acquire a lock. This is useful in determining if lock waits are causing poor performance. The default is off."
[^db-deadlocks]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 pg_stat_database, cột deadlocks — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "Number of deadlocks detected in this database"
[^max-locks]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.12 max_locks_per_transaction — <https://www.postgresql.org/docs/16/runtime-config-locks.html> — "The shared lock table has space for max_locks_per_transaction objects (e.g., tables) per server process or prepared transaction; hence, no more than this many distinct objects can be locked at any one time."
[^max-locks-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.12 max_locks_per_transaction — <https://www.postgresql.org/docs/16/runtime-config-locks.html> — "The default, 64, has historically proven sufficient, but you might need to raise this value if you have queries that touch many different tables in a single transaction, e.g., query of a parent table with many children."
[^adv-pool]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.5 Advisory Locks — <https://www.postgresql.org/docs/16/explicit-locking.html> — "Both advisory locks and regular locks are stored in a shared memory pool whose size is defined by the configuration variables max_locks_per_transaction and max_connections. Care must be taken not to exhaust this memory or the server will be unable to grant any locks at all."
[^at-lock]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), ALTER TABLE — <https://www.postgresql.org/docs/16/sql-altertable.html> — "Note that the lock level required may differ for each subform. An ACCESS EXCLUSIVE lock is acquired unless explicitly noted. When multiple subcommands are given, the lock acquired will be the strictest one required by any subcommand."
[^at-fk]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), ALTER TABLE, ADD table_constraint — <https://www.postgresql.org/docs/16/sql-altertable.html> — "Although most forms of ADD table_constraint require an ACCESS EXCLUSIVE lock, ADD FOREIGN KEY requires only a SHARE ROW EXCLUSIVE lock. Note that ADD FOREIGN KEY also acquires a SHARE ROW EXCLUSIVE lock on the referenced table, in addition to the lock on the table on which the constraint is declared."
[^at-addcol]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), ALTER TABLE, Notes — <https://www.postgresql.org/docs/16/sql-altertable.html> — "When a column is added with ADD COLUMN and a non-volatile DEFAULT is specified, the default is evaluated at the time of the statement and the result stored in the table's metadata. That value will be used for the column for all existing rows. If no DEFAULT is specified, NULL is used. In neither case is a rewrite of the table required."
[^at-rewrite]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), ALTER TABLE, Notes — <https://www.postgresql.org/docs/16/sql-altertable.html> — "Adding a column with a volatile DEFAULT or changing the type of an existing column will require the entire table and its indexes to be rewritten."
[^at-notvalid]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), ALTER TABLE, Notes — <https://www.postgresql.org/docs/16/sql-altertable.html> — "The main purpose of the NOT VALID constraint option is to reduce the impact of adding a constraint on concurrent updates. With NOT VALID, the ADD CONSTRAINT command does not scan the table and can be committed immediately. After that, a VALIDATE CONSTRAINT command can be issued to verify that existing rows satisfy the constraint."
[^at-validate]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), ALTER TABLE, VALIDATE CONSTRAINT — <https://www.postgresql.org/docs/16/sql-altertable.html> — "Nothing happens if the constraint is already marked valid. (See Notes below for an explanation of the usefulness of this command.) This command acquires a SHARE UPDATE EXCLUSIVE lock."
[^at-notnull]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), ALTER TABLE, SET NOT NULL — <https://www.postgresql.org/docs/16/sql-altertable.html> — "Ordinarily this is checked during the ALTER TABLE by scanning the entire table; however, if a valid CHECK constraint exists (and is not dropped in the same command) which proves no NULL can exist, then the table scan is skipped."
[^ci-conc]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), CREATE INDEX, CONCURRENTLY — <https://www.postgresql.org/docs/16/sql-createindex.html> — "When this option is used, PostgreSQL will build the index without taking any locks that prevent concurrent inserts, updates, or deletes on the table; whereas a standard index build locks out writes (but not reads) on the table until it's done."
[^ci-cost]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), CREATE INDEX, Building Indexes Concurrently — <https://www.postgresql.org/docs/16/sql-createindex.html> — "When this option is used, PostgreSQL must perform two scans of the table, and in addition it must wait for all existing transactions that could potentially modify or use the index to terminate."
[^ci-invalid]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), CREATE INDEX, Building Indexes Concurrently — <https://www.postgresql.org/docs/16/sql-createindex.html> — "If a problem arises while scanning the table, such as a deadlock or a uniqueness violation in a unique index, the CREATE INDEX command will fail but leave behind an “invalid” index."
[^lt-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.11 lock_timeout — <https://www.postgresql.org/docs/16/runtime-config-client.html> — "Abort any statement that waits longer than the specified amount of time while attempting to acquire a lock on a table, index, row, or other database object. The time limit applies separately to each lock acquisition attempt."
[^lt-default]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.11 lock_timeout — <https://www.postgresql.org/docs/16/runtime-config-client.html> — "The limit applies both to explicit locking requests (such as LOCK TABLE, or SELECT FOR UPDATE without NOWAIT) and to implicitly-acquired locks. If this value is specified without units, it is taken as milliseconds. A value of zero (the default) disables the timeout."
[^lt-conf]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.11 lock_timeout — <https://www.postgresql.org/docs/16/runtime-config-client.html> — "Setting lock_timeout in postgresql.conf is not recommended because it would affect all sessions."
[^lt-vs-st]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.11 lock_timeout — <https://www.postgresql.org/docs/16/runtime-config-client.html> — "Note that if statement_timeout is nonzero, it is rather pointless to set lock_timeout to the same or larger value, since the statement timeout would always trigger first."
[^st-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.11 statement_timeout — <https://www.postgresql.org/docs/16/runtime-config-client.html> — "In extended query protocol, the timeout starts running when any query-related message (Parse, Bind, Execute, Describe) arrives, and it is canceled by completion of an Execute or Sync message."
[^st-default]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.11 statement_timeout — <https://www.postgresql.org/docs/16/runtime-config-client.html> — "Abort any statement that takes more than the specified amount of time. If log_min_error_statement is set to ERROR or lower, the statement that timed out will also be logged. If this value is specified without units, it is taken as milliseconds. A value of zero (the default) disables the timeout."
[^iit-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.11 idle_in_transaction_session_timeout — <https://www.postgresql.org/docs/16/runtime-config-client.html> — "A value of zero (the default) disables the timeout. This option can be used to ensure that idle sessions do not hold locks for an unreasonable amount of time."
[^idle-sess-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.11 idle_session_timeout — <https://www.postgresql.org/docs/16/runtime-config-client.html> — "Terminate any session that has been idle (that is, waiting for a client query), but not within an open transaction, for longer than the specified amount of time. If this value is specified without units, it is taken as milliseconds. A value of zero (the default) disables the timeout."
[^idle-sess-warn]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.11 idle_session_timeout — <https://www.postgresql.org/docs/16/runtime-config-client.html> — "Be wary of enforcing this timeout on connections made through connection-pooling software or other middleware, as such a layer may not react well to unexpected connection closure."
[^planner-intro]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 52.5 Planner/Optimizer — <https://www.postgresql.org/docs/16/planner-optimizer.html> — "If it is computationally feasible, the query optimizer will examine each of these possible execution plans, ultimately selecting the execution plan that is expected to run the fastest."
[^plan-seq]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 52.5.1 Generating Possible Plans — <https://www.postgresql.org/docs/16/planner-optimizer.html> — "There is always the possibility of performing a sequential scan on a relation, so a sequential scan plan is always created."
[^join-3]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 52.5.1 Generating Possible Plans — <https://www.postgresql.org/docs/16/planner-optimizer.html> — "If the query requires joining two or more relations, plans for joining relations are considered after all feasible plans have been found for scanning single relations. The three available join strategies are:"
[^join-nl]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 52.5.1 Generating Possible Plans — <https://www.postgresql.org/docs/16/planner-optimizer.html> — "nested loop join: The right relation is scanned once for every row found in the left relation. This strategy is easy to implement but can be very time consuming."
[^join-nl-index]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 52.5.1 Generating Possible Plans — <https://www.postgresql.org/docs/16/planner-optimizer.html> — "However, if the right relation can be scanned with an index scan, this can be a good strategy. It is possible to use values from the current row of the left relation as keys for the index scan of the right."
[^join-merge]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 52.5.1 Generating Possible Plans — <https://www.postgresql.org/docs/16/planner-optimizer.html> — "merge join: Each relation is sorted on the join attributes before the join starts. Then the two relations are scanned in parallel, and matching rows are combined to form join rows. This kind of join is attractive because each relation has to be scanned only once."
[^join-hash]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 52.5.1 Generating Possible Plans — <https://www.postgresql.org/docs/16/planner-optimizer.html> — "hash join: the right relation is first scanned and loaded into a hash table, using its join attributes as hash keys. Next the left relation is scanned and the appropriate values of every row found are used as hash keys to locate the matching rows in the table."
[^geqo]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 52.5.1 Generating Possible Plans — <https://www.postgresql.org/docs/16/planner-optimizer.html> — "If the query uses fewer than geqo_threshold relations, a near-exhaustive search is conducted to find the best join sequence."
[^explain-tree]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.1 EXPLAIN Basics — <https://www.postgresql.org/docs/16/using-explain.html> — "The structure of a query plan is a tree of plan nodes. Nodes at the bottom level of the tree are scan nodes: they return raw rows from a table."
[^explain-numbers]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.1 EXPLAIN Basics — <https://www.postgresql.org/docs/16/using-explain.html> — "Estimated start-up cost. This is the time expended before the output phase can begin, e.g., time to do the sorting in a sort node. Estimated total cost. This is stated on the assumption that the plan node is run to completion, i.e., all available rows are retrieved."
[^explain-numbers2]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.1 EXPLAIN Basics — <https://www.postgresql.org/docs/16/using-explain.html> — "Estimated number of rows output by this plan node. Again, the node is assumed to be run to completion. Estimated average width of rows output by this plan node (in bytes)."
[^cost-unit]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.1 EXPLAIN Basics — <https://www.postgresql.org/docs/16/using-explain.html> — "The costs are measured in arbitrary units determined by the planner's cost parameters (see Section 20.7.2). Traditional practice is to measure the costs in units of disk page fetches; that is, seq_page_cost is conventionally set to 1.0 and the other cost parameters are set relative to that."
[^cost-child]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.1 EXPLAIN Basics — <https://www.postgresql.org/docs/16/using-explain.html> — "It's important to understand that the cost of an upper-level node includes the cost of all its child nodes. It's also important to realize that the cost only reflects things that the planner cares about. In particular, the cost does not consider the time spent transmitting result rows to the client"
[^explain-rows]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.1 EXPLAIN Basics — <https://www.postgresql.org/docs/16/using-explain.html> — "The rows value is a little tricky because it is not the number of rows processed or scanned by the plan node, but rather the number emitted by the node."
[^cost-formula]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.1 EXPLAIN Basics — <https://www.postgresql.org/docs/16/using-explain.html> — "The estimated cost is computed as (disk pages read * seq_page_cost) + (rows scanned * cpu_tuple_cost). By default, seq_page_cost is 1.0 and cpu_tuple_cost is 0.01, so the estimated cost is (358 * 1.0) + (10000 * 0.01) = 458."
[^explain-9-3]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1 Using EXPLAIN — <https://www.postgresql.org/docs/16/using-explain.html> — "Examples in this section are drawn from the regression test database after doing a VACUUM ANALYZE, using 9.3 development sources. You should be able to get similar results if you try the examples yourself, but your estimated costs and row counts might vary slightly"
[^explain-indent]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.1 EXPLAIN Basics — <https://www.postgresql.org/docs/16/using-explain.html> — "The indentation of the node summary lines reflects the plan tree structure."
[^toy-seq]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.3 Caveats — <https://www.postgresql.org/docs/16/using-explain.html> — "An extreme example is that on a table that only occupies one disk page, you'll nearly always get a sequential scan plan whether indexes are available or not."
[^indexscan-use]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.1 EXPLAIN Basics — <https://www.postgresql.org/docs/16/using-explain.html> — "In this type of plan the table rows are fetched in index order, which makes them even more expensive to read, but there are so few that the extra cost of sorting the row locations is not worth it. You'll most often see this plan type for queries that fetch just a single row."
[^bitmap-why]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.1 EXPLAIN Basics — <https://www.postgresql.org/docs/16/using-explain.html> — "The reason for using two plan levels is that the upper plan node sorts the row locations identified by the index into physical order before reading them, to minimize the cost of separate fetches."
[^bitmap-cheaper]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.1 EXPLAIN Basics — <https://www.postgresql.org/docs/16/using-explain.html> — "Fetching rows separately is much more expensive than reading them sequentially, but because not all the pages of the table have to be visited, this is still cheaper than a sequential scan."
[^bitmap-combine]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 11.5 Combining Multiple Indexes — <https://www.postgresql.org/docs/16/indexes-bitmap-scans.html> — "The table rows are visited in physical order, because that is how the bitmap is laid out; this means that any ordering of the original indexes is lost, and so a separate sort step will be needed if the query has an ORDER BY clause."
[^vm-only]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.4 Updating the Visibility Map — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "An index-only scan, on the other hand, checks the visibility map first. If it's known that all tuples on the page are visible, the heap fetch can be skipped."
[^rpc-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.7.2 random_page_cost — <https://www.postgresql.org/docs/16/runtime-config-query.html> — "Sets the planner's estimate of the cost of a non-sequentially-fetched disk page. The default is 4.0."
[^spc-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.7.2 seq_page_cost — <https://www.postgresql.org/docs/16/runtime-config-query.html> — "Sets the planner's estimate of the cost of a disk page fetch that is part of a series of sequential fetches. The default is 1.0."
[^rpc-why]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.7.2 random_page_cost — <https://www.postgresql.org/docs/16/runtime-config-query.html> — "Random access to durable storage is normally much more expensive than four times sequential access. However, a lower default is used (4.0) because the majority of random accesses to storage, such as indexed reads, are assumed to be in cache."
[^rpc-tip]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.7.2 random_page_cost — <https://www.postgresql.org/docs/16/runtime-config-query.html> — "Correspondingly, if your data is likely to be completely in cache, such as when the database is smaller than the total server memory, or network latency is high, decreasing random_page_cost might be appropriate."
[^explain-merge]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.1 EXPLAIN Basics — <https://www.postgresql.org/docs/16/using-explain.html> — "Merge join requires its input data to be sorted on the join keys."
[^explain-analyze]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.2 EXPLAIN ANALYZE — <https://www.postgresql.org/docs/16/using-explain.html> — "With this option, EXPLAIN actually executes the query, and then displays the true row counts and true run time accumulated within each plan node, along with the same estimates that a plain EXPLAIN shows."
[^explain-sideeffect]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.2 EXPLAIN ANALYZE — <https://www.postgresql.org/docs/16/using-explain.html> — "Keep in mind that because EXPLAIN ANALYZE actually runs the query, any side-effects will happen as usual, even though whatever results the query might output are discarded in favor of printing the EXPLAIN data. If you want to analyze a data-modifying query without changing your tables, you can roll the command back afterwards"
[^actual-ms]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.2 EXPLAIN ANALYZE — <https://www.postgresql.org/docs/16/using-explain.html> — "Note that the “actual time” values are in milliseconds of real time, whereas the cost estimates are expressed in arbitrary units; so they are unlikely to match up. The thing that's usually most important to look for is whether the estimated row counts are reasonably close to reality."
[^loops]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.2 EXPLAIN ANALYZE — <https://www.postgresql.org/docs/16/using-explain.html> — "In such cases, the loops value reports the total number of executions of the node, and the actual time and rows values shown are averages per-execution. This is done to make the numbers comparable with the way that the cost estimates are shown. Multiply by the loops value to get the total time actually spent in the node. In the above example, we spent a total of 0.220 milliseconds executing the index scans on tenk2."
[^rows-removed]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.2 EXPLAIN ANALYZE — <https://www.postgresql.org/docs/16/using-explain.html> — "The “Rows Removed” line only appears when at least one scanned row, or potential join pair in the case of a join node, is rejected by the filter condition."
[^explain-buffers]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.2 EXPLAIN ANALYZE — <https://www.postgresql.org/docs/16/using-explain.html> — "The numbers provided by BUFFERS help to identify which parts of the query are the most I/O-intensive."
[^explain-times]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.2 EXPLAIN ANALYZE — <https://www.postgresql.org/docs/16/using-explain.html> — "The Planning time shown by EXPLAIN ANALYZE is the time it took to generate the query plan from the parsed query and optimize it. It does not include parsing or rewriting. The Execution time shown by EXPLAIN ANALYZE includes executor start-up and shut-down time, as well as the time to run any triggers that are fired, but it does not include parsing, rewriting, or planning time."
[^explain-caveat]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.3 Caveats — <https://www.postgresql.org/docs/16/using-explain.html> — "First, since no output rows are delivered to the client, network transmission costs and I/O conversion costs are not included. Second, the measurement overhead added by EXPLAIN ANALYZE can be significant, especially on machines with slow gettimeofday() operating-system calls."
[^explain-toy]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.3 Caveats — <https://www.postgresql.org/docs/16/using-explain.html> — "EXPLAIN results should not be extrapolated to situations much different from the one you are actually testing; for example, results on a toy-sized table cannot be assumed to apply to large tables."
[^explain-limit]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.3 Caveats — <https://www.postgresql.org/docs/16/using-explain.html> — "There are cases in which the actual and estimated values won't match up well, but nothing is really wrong. One such case occurs when plan node execution is stopped short by a LIMIT or similar effect."
[^explain-bitmapzero]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.3 Caveats — <https://www.postgresql.org/docs/16/using-explain.html> — "BitmapAnd and BitmapOr nodes always report their actual row counts as zero, due to implementation limitations."
[^stats-pgclass]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.2.1 Single-Column Statistics — <https://www.postgresql.org/docs/16/planner-stats.html> — "For efficiency reasons, reltuples and relpages are not updated on-the-fly, and so they usually contain somewhat out-of-date values. They are updated by VACUUM, ANALYZE, and a few DDL commands such as CREATE INDEX."
[^stats-approx]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.2.1 Single-Column Statistics — <https://www.postgresql.org/docs/16/planner-stats.html> — "The information used for this task is stored in the pg_statistic system catalog. Entries in pg_statistic are updated by the ANALYZE and VACUUM ANALYZE commands, and are always approximate even when freshly updated."
[^analyze-sample]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.3 Updating Planner Statistics — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "ANALYZE uses a statistically random sampling of the rows of a table rather than reading every single row."
[^stats-target]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.2.1 Single-Column Statistics — <https://www.postgresql.org/docs/16/planner-stats.html> — "The default limit is presently 100 entries. Raising the limit might allow more accurate planner estimates to be made, particularly for columns with irregular data distributions, at the price of consuming more space in pg_statistic and slightly more time to compute the estimates."
[^ext-indep]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.2.2 Extended Statistics — <https://www.postgresql.org/docs/16/planner-stats.html> — "It is common to see slow queries running bad execution plans because multiple columns used in the query clauses are correlated. The planner normally assumes that multiple conditions are independent of each other, an assumption that does not hold when column values are correlated. Regular statistics, because of their per-individual-column nature, cannot capture any knowledge about cross-column correlation."
[^ext-create]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.2.2 Extended Statistics — <https://www.postgresql.org/docs/16/planner-stats.html> — "Because the number of possible column combinations is very large, it's impractical to compute multivariate statistics automatically. Instead, extended statistics objects, more often called just statistics objects, can be created to instruct the server to obtain statistics across interesting sets of columns."
[^ext-analyze]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.2.2 Extended Statistics — <https://www.postgresql.org/docs/16/planner-stats.html> — "Creation of such an object merely creates a catalog entry expressing interest in the statistics. Actual data collection is performed by ANALYZE (either a manual command, or background auto-analyze)."
[^ext-kinds]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), CREATE STATISTICS — <https://www.postgresql.org/docs/16/sql-createstatistics.html> — "Currently supported kinds are ndistinct, which enables n-distinct statistics, dependencies, which enables functional dependency statistics, and mcv which enables most-common values lists."
[^ext-advice]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.2.2.1 Functional Dependencies — <https://www.postgresql.org/docs/16/planner-stats.html> — "It is advisable to create dependencies statistics only for column groups that are strongly correlated, to avoid unnecessary overhead in both ANALYZE and later query planning."
[^func-stats]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.3 Updating Planner Statistics — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "However, if you create a statistics object or an expression index that uses a function call, useful statistics will be gathered about the function, which can greatly improve query plans that use the expression index."
[^prep-plan]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), PREPARE, Notes — <https://www.postgresql.org/docs/16/sql-prepare.html> — "A generic plan is the same across all executions, while a custom plan is generated for a specific execution using the parameter values given in that call. Use of a generic plan avoids planning overhead, but in some situations a custom plan will be much more efficient to execute because the planner can make use of knowledge of the parameter values."
[^prep-auto]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), PREPARE, Notes — <https://www.postgresql.org/docs/16/sql-prepare.html> — "By default (that is, when plan_cache_mode is set to auto), the server will automatically choose whether to use a generic or custom plan for a prepared statement that has parameters. The current rule for this is that the first five executions are done with custom plans and the average estimated cost of those plans is calculated."
[^enable-flags]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.7.1 enable_seqscan — <https://www.postgresql.org/docs/16/runtime-config-query.html> — "It is impossible to suppress sequential scans entirely, but turning this variable off discourages the planner from using one if there are other methods available."
[^enable-crude]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 14.1.1 EXPLAIN Basics — <https://www.postgresql.org/docs/16/using-explain.html> — "One way to look at variant plans is to force the planner to disregard whatever strategy it thought was the cheapest, using the enable/disable flags described in Section 20.7.1. (This is a crude tool, but useful."
[^psa-row]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2.3 pg_stat_activity — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "The pg_stat_activity view will have one row per server process, showing information related to the current activity of that process."
[^psa-wait]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2.3 pg_stat_activity, cột wait_event_type và wait_event — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "The type of event for which the backend is waiting, if any; otherwise NULL. See Table 28.4. wait_event text Wait event name if backend is currently waiting, otherwise NULL."
[^psa-state]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2.3 pg_stat_activity, cột state — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "active: The backend is executing a query. idle: The backend is waiting for a new client command. idle in transaction: The backend is in a transaction, but is not currently executing a query. idle in transaction (aborted): This state is similar to idle in transaction, except one of the statements in the transaction caused an error."
[^psa-xact]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2.3 pg_stat_activity, cột xact_start — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "Time when this process' current transaction was started, or null if no transaction is active. If the current query is the first of its transaction, this column is equal to the query_start column."
[^psa-queryid]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2.3 pg_stat_activity, cột query_id — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "Query identifiers are not computed by default so this field will be null unless compute_query_id parameter is enabled or a third-party module that computes query identifiers is configured."
[^cancel-term]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 9.27.2 Server Signaling Functions — <https://www.postgresql.org/docs/16/functions-admin.html> — "pg_cancel_backend and pg_terminate_backend send signals (SIGINT or SIGTERM respectively) to backend processes identified by process ID."
[^we-activity]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 Wait Event Types — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "The server process is idle. This event type indicates a process waiting for activity in its main processing loop."
[^we-type-bufferpin]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 Wait Event Types — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "The server process is waiting for exclusive access to a data buffer. Buffer pin waits can be protracted if another process holds an open cursor that last read data from the buffer in question."
[^we-type-client]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 Wait Event Types — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "The server process is waiting for activity on a socket connected to a user application. Thus, the server expects something to happen that is independent of its internal processes."
[^we-extension]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 Wait Event Types — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "The server process is waiting for some condition defined by an extension module."
[^we-io]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 Wait Event Types — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "The server process is waiting for an I/O operation to complete."
[^we-ipc]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 Wait Event Types — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "The server process is waiting for some interaction with another server process."
[^we-type-lock]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 Wait Event Types — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "The server process is waiting for a heavyweight lock. Heavyweight locks, also known as lock manager locks or simply locks, primarily protect SQL-visible objects such as tables."
[^we-timeout]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 Wait Event Types — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "The server process is waiting for a timeout to expire."
[^we-relation]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 Lock Wait Events — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "Waiting to acquire a lock on a relation."
[^we-clientread]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 Client Wait Events — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "Waiting to read data from the client."
[^we-datafileread]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 IO Wait Events — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "Waiting for a read from a relation data file."
[^we-walwrite]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 IO Wait Events — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "Waiting for a write to a WAL file."
[^we-walsync]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 IO Wait Events — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "Waiting for a WAL file to reach durable storage."
[^we-bufferpin]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 BufferPin Wait Events — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "Waiting to acquire an exclusive pin on a buffer."
[^we-lockmanager]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 LWLock Wait Events — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "Waiting to read or update information about “heavyweight” locks."
[^we-example]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 Wait Event Reference — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "Here is an example of how wait events can be viewed: SELECT pid, wait_event_type, wait_event FROM pg_stat_activity WHERE wait_event is NOT NULL;"
[^pglocks-objs]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 54.12 pg_locks — <https://www.postgresql.org/docs/16/view-pg-locks.html> — "There are several distinct types of lockable objects: whole relations (e.g., tables), individual pages of relations, individual tuples of relations, transaction IDs (both virtual and permanent IDs), and general database objects"
[^pglocks-granted]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 54.12 pg_locks — <https://www.postgresql.org/docs/16/view-pg-locks.html> — "granted is true in a row representing a lock held by the indicated process. False indicates that this process is currently waiting to acquire this lock"
[^pglocks-join]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 54.12 pg_locks — <https://www.postgresql.org/docs/16/view-pg-locks.html> — "The pid column can be joined to the pid column of the pg_stat_activity view to get more information on the session holding or awaiting each lock"
[^pglocks-blocking]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 54.12 pg_locks — <https://www.postgresql.org/docs/16/view-pg-locks.html> — "Worse, the pg_locks view does not expose information about which processes are ahead of which others in lock wait queues, nor information about which processes are parallel workers running on behalf of which other client sessions. It is better to use the pg_blocking_pids() function (see Table 9.67) to identify which process(es) a waiting process is blocked behind."
[^pgss-intro]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), F.32 pg_stat_statements — <https://www.postgresql.org/docs/16/pgstatstatements.html> — "The pg_stat_statements module provides a means for tracking planning and execution statistics of all SQL statements executed by a server."
[^pgss-setup]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), F.32 pg_stat_statements — <https://www.postgresql.org/docs/16/pgstatstatements.html> — "The module must be loaded by adding pg_stat_statements to shared_preload_libraries in postgresql.conf, because it requires additional shared memory. This means that a server restart is needed to add or remove the module. In addition, query identifier calculation must be enabled in order for the module to be active, which is done automatically if compute_query_id is set to auto or on, or any third-party module that calculates query identifiers is loaded."
[^pgss-row]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), F.32.1 The pg_stat_statements View — <https://www.postgresql.org/docs/16/pgstatstatements.html> — "This view contains one row for each distinct combination of database ID, user ID, query ID and whether it's a top-level statement or not (up to the maximum number of distinct statements that the module can track)."
[^pgss-normalize]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), F.32.1 The pg_stat_statements View — <https://www.postgresql.org/docs/16/pgstatstatements.html> — "Typically, two queries will be considered the same for this purpose if they are semantically equivalent except for the values of literal constants appearing in the query."
[^pgss-cols]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), F.32.1 The pg_stat_statements View — <https://www.postgresql.org/docs/16/pgstatstatements.html> — "calls bigint Number of times the statement was executed total_exec_time double precision Total time spent executing the statement, in milliseconds"
[^pgss-sample]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), F.32.5 Sample Output — <https://www.postgresql.org/docs/16/pgstatstatements.html> — "SELECT query, calls, total_exec_time, rows, 100.0 * shared_blks_hit / nullif(shared_blks_hit + shared_blks_read, 0) AS hit_percent FROM pg_stat_statements ORDER BY total_exec_time DESC LIMIT 5;"
[^pgss-privacy]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), F.32.1 The pg_stat_statements View — <https://www.postgresql.org/docs/16/pgstatstatements.html> — "For security reasons, only superusers and roles with privileges of the pg_read_all_stats role are allowed to see the SQL text and queryid of queries executed by other users."
[^pgss-planning]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), F.32.4 Configuration Parameters — <https://www.postgresql.org/docs/16/pgstatstatements.html> — "Enabling this parameter may incur a noticeable performance penalty, especially when statements with identical query structure are executed by many concurrent connections which compete to update a small number of pg_stat_statements entries. The default value is off."
[^pgss-queryid-stability]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), F.32.1 The pg_stat_statements View — <https://www.postgresql.org/docs/16/pgstatstatements.html> — "Since the identifier is derived from the post-parse-analysis tree, its value is a function of, among other things, the internal object identifiers appearing in this representation. This has some counterintuitive implications. For example, pg_stat_statements will consider two apparently-identical queries to be distinct, if they reference a table that was dropped and recreated between the executions of the two queries."
[^pgss-max]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), F.32.4 Configuration Parameters — <https://www.postgresql.org/docs/16/pgstatstatements.html> — "If more distinct statements than that are observed, information about the least-executed statements is discarded. The number of times such information was discarded can be seen in the pg_stat_statements_info view. The default value is 5000."
[^pgss-mem]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), F.32.4 Configuration Parameters — <https://www.postgresql.org/docs/16/pgstatstatements.html> — "The module requires additional shared memory proportional to pg_stat_statements.max. Note that this memory is consumed whenever the module is loaded, even if pg_stat_statements.track is set to none."
[^pgss-text]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), F.32.1 The pg_stat_statements View — <https://www.postgresql.org/docs/16/pgstatstatements.html> — "As a recovery method if that happens, pg_stat_statements may choose to discard the query texts, whereupon all existing entries in the pg_stat_statements view will show null query fields, though the statistics associated with each queryid are preserved."
[^pgss-plans-calls]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), F.32.1 The pg_stat_statements View — <https://www.postgresql.org/docs/16/pgstatstatements.html> — "plans and calls aren't always expected to match because planning and execution statistics are updated at their respective end phase, and only for successful operations."
[^pgstat-user]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2.18 pg_stat_all_tables — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "The pg_stat_user_tables and pg_stat_sys_tables views contain the same information, but filtered to only show user and system tables respectively."
[^pgstat-dead]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2.18 pg_stat_all_tables, cột n_dead_tup — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "n_dead_tup bigint Estimated number of dead rows"
[^pgstat-hot]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2.18 pg_stat_all_tables, cột n_tup_hot_upd — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "Number of rows HOT updated. These are updates where no successor versions are required in indexes."
[^pgstat-lastav]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2.18 pg_stat_all_tables, cột last_autovacuum — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "Last time at which this table was vacuumed by the autovacuum daemon"
[^pgstat-ckpt]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2.14 pg_stat_bgwriter — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "checkpoints_timed bigint Number of scheduled checkpoints that have been performed checkpoints_req bigint Number of requested checkpoints that have been performed"
[^ckpt-warning]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.5.2 checkpoint_warning — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "Write a message to the server log if checkpoints caused by the filling of WAL segment files happen closer together than this amount of time (which suggests that max_wal_size ought to be raised)."
[^log-min-dur]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.8.2 log_min_duration_statement — <https://www.postgresql.org/docs/16/runtime-config-logging.html> — "Causes the duration of each completed statement to be logged if the statement ran for at least the specified amount of time. For example, if you set it to 250ms then all SQL statements that run 250ms or longer will be logged. Enabling this parameter can be helpful in tracking down unoptimized queries in your applications."
[^log-ckpt]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.8.3 log_checkpoints — <https://www.postgresql.org/docs/16/runtime-config-logging.html> — "Some statistics are included in the log messages, including the number of buffers written and the time spent writing them. This parameter can only be set in the postgresql.conf file or on the server command line. The default is on."
[^pgss-ext]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), F.32 pg_stat_statements — <https://www.postgresql.org/docs/16/pgstatstatements.html> — "These are not available globally but can be enabled for a specific database with CREATE EXTENSION pg_stat_statements."
[^pgss-extfile]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), F.32.1 The pg_stat_statements View — <https://www.postgresql.org/docs/16/pgstatstatements.html> — "The representative query texts are kept in an external disk file, and do not consume shared memory."
[^log-min-dur-def]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.8.2 log_min_duration_statement — <https://www.postgresql.org/docs/16/runtime-config-logging.html> — "Setting this to zero prints all statement durations. -1 (the default) disables logging statement durations."
[^sb-min]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 shared_buffers — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "If you have a dedicated database server with 1GB or more of RAM, a reasonable starting value for shared_buffers is 25% of the memory in your system."
[^max-prep]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.4.1 max_prepared_transactions — <https://www.postgresql.org/docs/16/runtime-config-resource.html> — "Setting this parameter to zero (which is the default) disables the prepared-transaction feature. This parameter can only be set at server start. If you are not planning to use prepared transactions, this parameter should be set to zero to prevent accidental creation of prepared transactions."
[^av-busy]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.6 The Autovacuum Daemon — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "If several large tables all become eligible for vacuuming in a short amount of time, all autovacuum workers might become occupied with vacuuming those tables for a long period. This would result in other tables and databases not being vacuumed until a worker becomes available."
[^wal-accum]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 30.5 WAL Configuration — <https://www.postgresql.org/docs/16/wal-configuration.html> — "If WAL archiving cannot keep up with the pace that WAL is generated, or if archive_command or archive_library fails repeatedly, old WAL files will accumulate in pg_wal until the situation is resolved. A slow or failed standby server that uses a replication slot will have the same effect"
[^fpw-default]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.5.1 full_page_writes — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "This parameter can only be set in the postgresql.conf file or on the server command line. The default is on."
[^fsync-on]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.5.1 fsync — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "If this parameter is on, the PostgreSQL server will try to make sure that updates are physically written to disk, by issuing fsync() system calls or various equivalent methods (see wal_sync_method)."
[^fsync-off]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.5.1 fsync — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "While turning off fsync is often a performance benefit, this can result in unrecoverable data corruption in the event of a power failure or system crash. Thus it is only advisable to turn off fsync if you can easily recreate your entire database from external data."
[^sc-default]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.5.1 synchronous_commit — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "Valid values are remote_apply, on (the default), remote_write, local, and off."
[^sc-local]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.5.1 synchronous_commit — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "It is therefore possible, and useful, to have some transactions commit synchronously and others asynchronously. For example, to make a single multistatement transaction commit asynchronously when the default is the opposite, issue SET LOCAL synchronous_commit TO OFF within the transaction."
[^st-conf]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.11 statement_timeout — <https://www.postgresql.org/docs/16/runtime-config-client.html> — "Setting statement_timeout in postgresql.conf is not recommended because it would affect all sessions."
[^oom-default]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 19.4.4 Linux Memory Overcommit — <https://www.postgresql.org/docs/16/kernel-resources.html> — "The default virtual memory behavior on Linux is not optimal for PostgreSQL. Because of the way that the kernel implements memory overcommit, the kernel might terminate the PostgreSQL postmaster (the supervisor server process) if the memory demands of either PostgreSQL or another process cause the system to run out of virtual memory."
[^oom-strict]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 19.4.4 Linux Memory Overcommit — <https://www.postgresql.org/docs/16/kernel-resources.html> — "It is possible to modify the kernel's behavior so that it will not “overcommit” memory. Although this setting will not prevent the OOM killer from being invoked altogether, it will lower the chances significantly and will therefore lead to more robust system behavior."
[^oom-score]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 19.4.4 Linux Memory Overcommit — <https://www.postgresql.org/docs/16/kernel-resources.html> — "set the process-specific OOM score adjustment value for the postmaster process to -1000, thereby guaranteeing it will not be targeted by the OOM killer."
[^fsync-sc]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.5.1 fsync — <https://www.postgresql.org/docs/16/runtime-config-wal.html> — "In many situations, turning off synchronous_commit for noncritical transactions can provide much of the potential performance benefit of turning off fsync, without the attendant risks of data corruption."
[^sqlx-maxconn]: docs.rs sqlx-core 0.9.0, PoolOptions::max_connections — <https://docs.rs/sqlx-core/0.9.0/sqlx_core/pool/struct.PoolOptions.html> — "Be mindful of the connection limits for your database as well as other applications which may want to connect to the same database (or even multiple instances of the same application in high-availability deployments)."
[^sqlx-acquire]: docs.rs sqlx-core 0.9.0, PoolOptions::acquire_timeout — <https://docs.rs/sqlx-core/0.9.0/sqlx_core/pool/struct.PoolOptions.html> — "Caps the total amount of time Pool::acquire() can spend waiting across multiple phases: First, it may need to wait for a permit from the semaphore, which grants it the privilege of opening a connection or popping one from the idle queue."
[^sqlx-tba]: docs.rs sqlx-core 0.9.0, PoolOptions::acquire_timeout — <https://docs.rs/sqlx-core/0.9.0/sqlx_core/pool/struct.PoolOptions.html> — "If an existing idle connection is acquired, by default it will be checked for liveness and integrity before being returned, which may require executing a command on the connection."
[^sqlx-after]: docs.rs sqlx-core 0.9.0, PoolOptions::after_connect — <https://docs.rs/sqlx-core/0.9.0/sqlx_core/pool/struct.PoolOptions.html> — "This callback may be used to set additional configuration parameters that are not exposed by the database's ConnectOptions."
[^sqlx-cache]: docs.rs sqlx 0.9.0, PgConnectOptions::statement_cache_capacity — <https://docs.rs/sqlx/0.9.0/sqlx/postgres/struct.PgConnectOptions.html> — "Sets the capacity of the connection's statement cache in a number of stored distinct statements. Caching is handled using LRU, meaning when the amount of queries hits the defined limit, the oldest statement will get dropped. The default cache capacity is 100 statements."
[^sqlx-persistent]: docs.rs sqlx 0.9.0, Query::persistent — <https://docs.rs/sqlx/0.9.0/sqlx/query/struct.Query.html> — "If true, the statement will get prepared once and cached to the connection's statement cache. If queried once with the flag set to true, all subsequent queries matching the one with the flag will use the cached statement until the cache is cleared. If false, the prepared statement will be closed after execution. Default: true."
[^sqlx-options]: docs.rs sqlx 0.9.0, PgConnectOptions::options — <https://docs.rs/sqlx/0.9.0/sqlx/postgres/struct.PgConnectOptions.html> — "Set additional startup options for the connection as a list of key-value pairs."
[^sqlx-appname]: docs.rs sqlx 0.9.0, PgConnectOptions::application_name — <https://docs.rs/sqlx/0.9.0/sqlx/postgres/struct.PgConnectOptions.html> — "Sets the application name. Defaults to None"
[^tp-prepare]: docs.rs tokio-postgres 0.7.18, Client::prepare — <https://docs.rs/tokio-postgres/0.7.18/tokio_postgres/struct.Client.html> — "Prepared statements can be executed repeatedly, and may contain query parameters (indicated by $1, $2, etc), which are set when executed. Prepared statements can only be used with the connection that created them."
[^tp-query]: docs.rs tokio-postgres 0.7.18, Client::query — <https://docs.rs/tokio-postgres/0.7.18/tokio_postgres/struct.Client.html> — "The statement argument can either be a Statement, or a raw query string. If the same statement will be repeatedly executed (perhaps with different query parameters), consider preparing the statement up front with the prepare method."
[^tp-typed]: docs.rs tokio-postgres 0.7.18, Client::query_typed — <https://docs.rs/tokio-postgres/0.7.18/tokio_postgres/struct.Client.html> — "Compared to query, this method allows performing queries without three round trips (for prepare, execute, and close) by requiring the caller to specify parameter values along with their Postgres type. Thus, this is suitable in environments where prepared statements aren't supported (such as Cloudflare Workers with Hyperdrive)."
[^tp-options]: docs.rs tokio-postgres 0.7.18, config::Config — <https://docs.rs/tokio-postgres/0.7.18/tokio_postgres/config/struct.Config.html> — "options - Command line options used to configure the server."
[^tp-cancel]: docs.rs tokio-postgres 0.7.18, Client::cancel_token — <https://docs.rs/tokio-postgres/0.7.18/tokio_postgres/struct.Client.html> — "Constructs a cancellation token that can later be used to request cancellation of a query running on the connection associated with this client."
[^dp-maxsize]: docs.rs deadpool 0.13.1, managed::PoolConfig — <https://docs.rs/deadpool/0.13.1/deadpool/managed/struct.PoolConfig.html> — "Maximum size of the Pool. Default: cpu_core_count * 2"
[^dp-timeouts]: docs.rs deadpool 0.13.1, managed::PoolConfig — <https://docs.rs/deadpool/0.13.1/deadpool/managed/struct.PoolConfig.html> — "Timeouts of the Pool. Default: No timeouts"
[^dp-cache]: docs.rs deadpool-postgres 0.14.2, ClientWrapper::prepare_cached — <https://docs.rs/deadpool-postgres/0.14.2/deadpool_postgres/struct.ClientWrapper.html> — "Like tokio_postgres::Client::prepare(), but uses an existing Statement from the StatementCache if possible."
[^dp-fast]: docs.rs deadpool-postgres 0.14.2, RecyclingMethod — <https://docs.rs/deadpool-postgres/0.14.2/deadpool_postgres/enum.RecyclingMethod.html> — "The default is Fast which does not check the connection health or perform any clean-up queries."
[^dp-clean]: docs.rs deadpool-postgres 0.14.2, RecyclingMethod::Clean — <https://docs.rs/deadpool-postgres/0.14.2/deadpool_postgres/enum.RecyclingMethod.html> — "This is similar to calling DISCARD ALL. but doesn't call DEALLOCATE ALL and DISCARD PLAN, so that the statement cache is not rendered ineffective."
[^psa-appname]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2.3 pg_stat_activity, cột application_name — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "Name of the application that is connected to this backend"
[^set-config]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 9.27.1 Configuration Settings Functions — <https://www.postgresql.org/docs/16/functions-admin.html> — "Sets the parameter setting_name to new_value, and returns that value. If is_local is true, the new value will only apply during the current transaction."
[^pgb-qtimeout]: PgBouncer docs (trang sống, truy cập 2026-10-12), Configuration, query_timeout — <https://www.pgbouncer.org/config.html> — "Queries running longer than that are canceled. This should be used only with a slightly smaller server-side statement_timeout, to apply only for network problems."
[^set-session]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), SET — <https://www.postgresql.org/docs/16/sql-set.html> — "Once the surrounding transaction is committed, the effects will persist until the end of the session, unless overridden by another SET."
[^prep-unnamed]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 55.2.3 Extended Query — <https://www.postgresql.org/docs/16/protocol-flow.html> — "An unnamed prepared statement lasts only until the next Parse statement specifying the unnamed statement as destination is issued."
[^pgb-maxprep-how]: PgBouncer docs (trang sống, truy cập 2026-10-12), Configuration, max_prepared_statements — <https://www.pgbouncer.org/config.html> — "PgBouncer keeps track of the name that the client gave to each prepared statement. It then rewrites each command that uses a prepared statement to by replacing the client side name with the internal name (e.g. replacing my_prepared_statement with PGBOUNCER_123) before forwarding that command to the server."
[^pgb-maxprep-auto]: PgBouncer docs (trang sống, truy cập 2026-10-12), Configuration, max_prepared_statements — <https://www.pgbouncer.org/config.html> — "More importantly, if the prepared statement that the client wants to execute is not yet prepared on the server (e.g. because a different server is now assigned to the client than when the client prepared the statement), then PgBouncer transparently prepares the statement before executing it."
[^pgb-maxprep-all]: PgBouncer docs (trang sống, truy cập 2026-10-12), Configuration, max_prepared_statements — <https://www.pgbouncer.org/config.html> — "The exception to this rule are the DEALLOCATE ALL and DISCARD ALL commands, these do work as expected and will clear the prepared statements that PgBouncer tracked for the client that sends this command."
[^pgb-maxprep-zero]: PgBouncer docs (trang sống, truy cập 2026-10-12), Configuration, max_prepared_statements — <https://www.pgbouncer.org/config.html> — "When the setting is set to 0 prepared statement support for transaction and statement pooling is disabled."
[^pgb-prep-1-21]: PgBouncer docs (trang sống, truy cập 2026-10-12), Changelog, PgBouncer 1.21.0 — <https://www.pgbouncer.org/changelog.html> — "To benefit from this new feature you need to change the new max_prepared_statements setting to a non-zero value"
[^pgb-prep-default]: PgBouncer docs (trang sống, truy cập 2026-10-12), Changelog, PgBouncer 1.24.0 (2025-01-10) — <https://www.pgbouncer.org/changelog.html> — "Enable prepared statement support by default, max_prepared_statements is now set to 200 by default."
[^pgb-maxprep-err]: PgBouncer docs (trang sống, truy cập 2026-10-12), Configuration, max_prepared_statements — <https://www.pgbouncer.org/config.html> — "If the return or argument types of a prepared statement changes across executions then PostgreSQL currently throws an error such as: ERROR: cached plan must not change result type You can avoid such errors by not having multiple clients that use the exact same query string in a prepared statement, but expecting different argument or result types."
[^pgb-maxprep-ddl]: PgBouncer docs (trang sống, truy cập 2026-10-12), Configuration, max_prepared_statements — <https://www.pgbouncer.org/config.html> — "One of the most common ways of running into this issue is during a DDL migration where you add a new column or change a column type on an existing table. In those cases you can run RECONNECT on the PgBouncer admin console after doing the migration to force a re-prepare of the query and make the error go away."
[^pgb-reserve]: PgBouncer docs (trang sống, truy cập 2026-10-12), Configuration, reserve_pool_size — <https://www.pgbouncer.org/config.html> — "How many additional connections to allow to a pool (see reserve_pool_timeout). 0 disables. Default: 0 (disabled)"
[^clock-sweep]: PostgreSQL REL_16_4, src/backend/storage/buffer/README, Buffer Replacement Strategy — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/buffer/README> — "If the selected buffer is pinned or has a nonzero usage count, it cannot be used. Decrement its usage count (if nonzero), reacquire buffer_strategy_lock, and return to step 3 to examine the next buffer."
[^hot-readme-all]: PostgreSQL REL_16_4, src/backend/access/heap/README.HOT — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/README.HOT> — "If any columns that are included by non-summarizing indexes are updated, the HOT optimization is not applied, and the new tuple is inserted into all indexes of the table."
[^hot-chain2]: PostgreSQL REL_16_4, src/backend/access/heap/README.HOT — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/README.HOT> — "after traversing from the index to tuple 1, the index search proceeds forward to child tuples as long as it sees the HEAP_HOT_UPDATED flag set."
[^vm-frozen]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 73.4 Visibility Map — <https://www.postgresql.org/docs/16/storage-vm.html> — "The second bit, if set, means that all tuples on the page have been frozen. That means that even an anti-wraparound vacuum need not revisit the page."
[^vm-aggr]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.5 Preventing Transaction ID Wraparound Failures — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "As discussed in the previous section, an aggressive vacuum means that only those pages which are known to be all-frozen will be skipped."
[^ph-xid]: PostgreSQL REL_16_4, procarray.c, ComputeXidHorizons — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/ipc/procarray.c> — "We must check both because a transaction might have an Xmin but not (yet) an Xid; conversely, if it has an Xid, that could determine some not-yet-set Xmin."
[^ph-otherdb]: PostgreSQL REL_16_4, procarray.c, ComputeXidHorizons — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/storage/ipc/procarray.c> — "Normally sessions in other databases are ignored for anything but the shared horizon."
[^vl-bypass2]: PostgreSQL REL_16_4, vacuumlazy.c — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/access/heap/vacuumlazy.c> — "bypass = (vacrel->lpdead_item_pages < threshold && vacrel->lpdead_items < MAXDEADITEMS(32L * 1024L * 1024L));"
[^multixact32]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.5.1 Multixacts and Wraparound — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "Like transaction IDs, multixact IDs are implemented as a 32-bit counter and corresponding storage, all of which requires careful aging management, storage cleanup, and wraparound handling."
[^lk-table]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 13.3.1 Table-Level Locks, Table 13.2 (bảng bị làm phẳng khi trích: ô trống không hiện, mỗi X là một xung đột) — <https://www.postgresql.org/docs/16/explicit-locking.html> — "ACCESS SHARE X ROW SHARE X X ROW EXCL. X X X X SHARE UPDATE EXCL. X X X X X"
[^at-rewrite-exc]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), ALTER TABLE, Notes — <https://www.postgresql.org/docs/16/sql-altertable.html> — "As an exception, when changing the type of an existing column, if the USING clause does not change the column contents and the old type is either binary coercible to the new type or an unconstrained domain over the new type, a table rewrite is not needed."
[^at-notvalid-scope]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), ALTER TABLE, ADD table_constraint — <https://www.postgresql.org/docs/16/sql-altertable.html> — "plus the option NOT VALID, which is currently only allowed for foreign key and CHECK constraints."
[^geqo-thr]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.7.3 Genetic Query Optimizer, geqo_threshold — <https://www.postgresql.org/docs/16/runtime-config-query.html> — "Use genetic query optimization to plan queries with at least this many FROM items involved. (Note that a FULL OUTER JOIN construct counts as only one FROM item.) The default is 12."
[^av-foreign]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 25.1.6 The Autovacuum Daemon — <https://www.postgresql.org/docs/16/routine-vacuuming.html> — "The autovacuum daemon does not issue ANALYZE commands for foreign tables, since it has no means of determining how often that might be useful."
[^explain-label]: PostgreSQL REL_16_4, src/backend/commands/explain.c — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/commands/explain.c> — "ExplainPropertyFloat("Planning Time", "ms", 1000.0 * plantime, 3, es);"
[^explain-label2]: PostgreSQL REL_16_4, src/backend/commands/explain.c — <https://raw.githubusercontent.com/postgres/postgres/REL_16_4/src/backend/commands/explain.c> — "ExplainPropertyFloat("Execution Time", "ms", 1000.0 * totaltime, 3,"
[^bxid]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 28.2 pg_stat_activity, cột backend_xid — <https://www.postgresql.org/docs/16/monitoring-stats.html> — "Top-level transaction identifier of this backend, if any"
[^dl-tradeoff]: PostgreSQL 16 docs (trang sống, truy cập 2026-10-12), 20.12 deadlock_timeout — <https://www.postgresql.org/docs/16/runtime-config-locks.html> — "Increasing this value reduces the amount of time wasted in needless deadlock checks, but slows down reporting of real deadlock errors."
[^sqlx-unnamed]: sqlx 0.9.0 (tag v0.9.0), sqlx-postgres/src/connection/executor.rs, hàm prepare — <https://raw.githubusercontent.com/launchbadge/sqlx/v0.9.0/sqlx-postgres/src/connection/executor.rs> — "let id = if persistent { let id = conn.inner.next_statement_id; conn.inner.next_statement_id = id.next(); id } else { StatementId::UNNAMED };"
[^dp-fast2]: docs.rs deadpool-postgres 0.14.2, RecyclingMethod::Fast — <https://docs.rs/deadpool-postgres/0.14.2/deadpool_postgres/enum.RecyclingMethod.html> — "Only run Client::is_closed() when recycling existing connections."
