# Lab P5 — Lưu trữ

> Đây là phần thực hành của [Phase P5: Lưu trữ](../05-storage/00-tong-quan.md). **Hãy đọc chương lý thuyết được trỏ ở đầu mỗi lab trước.** Mỗi lab có: mục tiêu, chương cần đọc, lệnh chạy, **dự đoán theo lý thuyết** (kèm chú thích) và câu hỏi tự kiểm tra.
>
> Quy ước chú thích: mọi sự thật có chú thích cuối trang (footnote) trỏ về nguồn và câu trích nguyên văn ở cuối file (chạy `python3 tools/verify_citations.py docs/labs/05-storage-lab.md`). Câu ghi "(nhận xét của người soạn)" hoặc "(suy luận của người soạn)" là lời khuyên/suy luận, không phải sự thật có nguồn.
>
> **File này cố ý KHÔNG ghi output hay số đo.** Người soạn không chạy các lab để viết lý thuyết; mục "Dự đoán" chỉ là điều lý thuyết cho phép đoán trước. Bạn chạy lab, **ghi lại kết quả của máy bạn**, rồi đối chiếu với dự đoán. Lệch dự đoán thì đó là chỗ đáng học nhất: đọc lại phần "Đi sâu" của chương tương ứng. Số đo thời gian phụ thuộc vào đĩa, filesystem, cache của máy ảo — chỉ so sánh *tương đối* giữa các dòng của cùng một lần chạy.

## Chuẩn bị

Crate: `labs/crates/lab-storage`, chỉ phụ thuộc `libc = "0.2"` (cho `posix_fadvise`, `mmap`/`mincore`, `sysconf` và cờ `O_DIRECT`; std không bọc các thứ này). Helper chung ở `src/lib.rs`: thư mục scratch, đọc `/proc/meminfo`, đo độ "resident" của file trong page cache, buffer căn lề cho `O_DIRECT`.

| Lab | Bin | Chạy trên | Chương |
| --- | --- | --- | --- |
| 1. Chi phí của `fsync` | `sync_cost` | macOS hoặc Linux | [06 §1–2](../05-storage/06-durability-trong-thuc-te.md), [08 §3](../05-storage/08-rust-va-luu-tru.md) |
| 2. Mẫu ghi an toàn | `atomic_write` | macOS hoặc Linux | [06 §3–4](../05-storage/06-durability-trong-thuc-te.md), [02 §4](../05-storage/02-file-va-directory-api.md) |
| 3. Page cache và `posix_fadvise` | `page_cache` | **Linux** | [04 §1–3](../05-storage/04-page-cache-va-direct-io.md) |
| 4. `O_DIRECT` | `direct_io` | **Linux** | [04 §7](../05-storage/04-page-cache-va-direct-io.md) |
| 5. Inode, hard link, `stat`, `read_dir` | `inode_stat`, `read_dir_meta` | macOS hoặc Linux | [02 §3](../05-storage/02-file-va-directory-api.md), [03 §2, §4](../05-storage/03-cai-dat-file-system.md), [08 §6](../05-storage/08-rust-va-luu-tru.md) |
| 6. Quan sát writeback | `writeback` | **Linux** | [04 §3–4](../05-storage/04-page-cache-va-direct-io.md) |

Các bin Linux-only (`#[cfg(target_os = "linux")]`) khi chạy trên macOS chỉ in hướng dẫn dùng `./labs/linux.sh`; chạy trong container thì dùng:

```bash
./labs/linux.sh cargo run -q -p lab-storage --bin <tên-bin> -- [tham số]
```

Mọi bin nhận tham số đầu tiên là **thư mục scratch** (mặc định `<tmp>/lab-storage-<pid>`). Điều này quan trọng: filesystem dưới thư mục đó quyết định hành vi của `fsync`, `O_DIRECT`… (nhận xét của người soạn: `/tmp` trong container và thư mục bind-mount từ máy chủ có thể là hai filesystem khác nhau; ghi lại `df <dir>` / `stat -f <dir>` cùng kết quả lab).

Dọn dẹp: mỗi bin tự xóa file nó tạo, trừ khi bị dừng giữa chừng (lab 2 cố ý để lại file tạm).

## Lab 1: chi phí và syscall của `sync_data` / `sync_all`

**Mục tiêu.** Thấy `write` thành công chưa phải là dữ liệu đã bền, và `sync_all`/`sync_data` đắt thế nào so với không sync; thấy Rust gọi syscall nào.

**Đọc trước.** [Chương 06 §1](../05-storage/06-durability-trong-thuc-te.md) (`write` chưa bền), [§2](../05-storage/06-durability-trong-thuc-te.md) (`fsync` vs `fdatasync`), [§8](../05-storage/06-durability-trong-thuc-te.md) (Rust gọi syscall nào); [Chương 08 §3](../05-storage/08-rust-va-luu-tru.md).

**Chạy.**

```bash
cd labs && cargo run -q -p lab-storage --bin sync_cost -- /tmp/lab-storage 200      # macOS hoặc Linux
./labs/linux.sh bash -c 'cargo build -q -p lab-storage --bin sync_cost && \
  strace -f -c -e trace=write,fsync,fdatasync ./target-linux/debug/sync_cost'          # đếm syscall trên Linux
```

Bin ghi `records` bản ghi 4 KiB vào một file theo bốn cách: (1) chỉ `write_all`; (2) `write_all` + `sync_data` sau mỗi bản ghi; (3) `write_all` + `sync_all` sau mỗi bản ghi; (4) ghi hết rồi `sync_all` một lần. In tổng và thời gian trung bình mỗi bản ghi.

**Dự đoán theo lý thuyết.**

1. Dòng (1) rẻ nhất: man page `write(2)` nói `write` thành công không bảo đảm dữ liệu đã nằm trên đĩa[^write-nogarantee], cách duy nhất để chắc là gọi `fsync` sau khi ghi xong[^write-fsync]. (Suy luận của người soạn: vì `write` không phải đợi thiết bị nên nó rẻ hơn; con số cụ thể bạn tự đo.)
2. Dòng (2) và (3) đắt hơn (1) nhiều lần: `fsync` chặn cho tới khi thiết bị báo đã xong việc truyền, kể cả việc xả cache của đĩa nếu có[^fsync-cache]. Đây là điều bạn trả để có bảo đảm "dữ liệu lấy lại được kể cả khi máy sập hoặc khởi động lại".[^fsync-def]
3. **Dòng (2) có thể không rẻ hơn (3) nhiều** — và đây là chỗ dễ đoán sai. `fdatasync` nhằm giảm hoạt động đĩa cho ứng dụng không cần đồng bộ mọi metadata,[^fdatasync-aim] nhưng man page nêu rõ một thay đổi kích thước file (`st_size`) vẫn đòi flush metadata.[^fdatasync-def] Mỗi lần `write_all` ở lab này **làm file dài ra**, tức đổi `st_size`. Dự đoán: chênh lệch giữa (2) và (3) nhỏ hơn bạn tưởng trên Linux; bài tập 1 bên dưới kiểm chứng điều này.
4. Dòng (4) rẻ hơn (3) vì chỉ phải đợi flush một lần cho cả lô. Cái giá không nằm trong bảng thời gian: nếu máy sập giữa chừng thì mọi bản ghi sau lần sync cuối đều có thể mất, vì `fsync` chỉ bảo đảm cho dữ liệu đã ghi *tính đến lúc gọi* (nhận xét của người soạn; ý tưởng này chính là gom nhóm commit của WAL ở [chương 05 §8](../05-storage/05-crash-consistency-va-journaling.md)).
5. Syscall thấy qua `strace` trên **Linux**: `File::sync_all` gọi `libc::fsync`[^src-fsync-linux] và `File::sync_data` gọi `libc::fdatasync`.[^src-datasync] Số `write` bằng số bản ghi.
6. Trên **macOS** kết quả khác về bản chất: nhánh `target_vendor = "apple"` của `sync_all` gọi `fcntl(fd, F_FULLFSYNC)`,[^src-fsync] và `sync_data` cũng vậy.[^src-datasync-apple] Docs Rust cũng cảnh báo `sync_data` trên một số nền tảng chỉ đơn giản được cài bằng `sync_all`.[^rust-syncdata] Dự đoán: trên macOS (2) và (3) cùng cơ chế nên cùng cỡ chi phí. `F_FULLFSYNC` đòi ổ đĩa xả bộ đệm của chính nó[^xnu-fullfsync] — đây là lý do số đo macOS không so được với số đo Linux (xem [chương 06 §7](../05-storage/06-durability-trong-thuc-te.md)).
7. Số tuyệt đối trong container chỉ phản ánh chồng lưu trữ ảo hóa của máy chạy Docker, không phản ánh một ổ vật lý: tài liệu kernel nói rõ nhiều thiết bị báo hoàn tất I/O *trước khi* dữ liệu thật sự chạm bộ nhớ không mất điện.[^wcc-intro] (Nhận xét của người soạn: không rút kết luận về độ bền thật từ một lần đo trong VM.)

**Câu hỏi.**

1. Vì sao dòng (3) chậm hơn dòng (1) nhiều lần dù ghi cùng lượng dữ liệu?
<details><summary>Đáp án</summary>

Dòng (1) chỉ chép vào page cache và `write` không hứa gì về đĩa.[^write-nogarantee] Dòng (3) gọi `fsync` sau mỗi bản ghi, và `fsync` chặn tới khi thiết bị báo truyền xong, gồm cả xả cache đĩa nếu có.[^fsync-cache]

</details>

2. Dòng (4) nhanh hơn (3) — bạn mất gì để đổi lấy điều đó?
<details><summary>Đáp án</summary>

Khoảng thời gian giữa hai lần sync mà máy sập thì các bản ghi trong khoảng đó có thể mất; `fsync` chỉ cam kết cho dữ liệu đã ghi tại thời điểm gọi (suy luận của người soạn từ định nghĩa `fsync`[^fsync-def]).

</details>

3. Trên Linux, `strace` sẽ thấy mấy lần `fsync` và mấy lần `fdatasync` với `records = 200`?
<details><summary>Đáp án</summary>

Ba chế độ có sync tạo ra: 200 `fdatasync` (chế độ 2, qua `File::sync_data`[^src-datasync]), 200 `fsync` (chế độ 3, qua `File::sync_all`[^src-fsync-linux]) và 1 `fsync` (chế độ 4) — tổng 201 `fsync` + 200 `fdatasync`; có thể thêm vài lệnh sync khác ngoài lab nếu runtime/loader tạo ra, bạn đọc `strace -c` để biết chắc.

</details>

4. Rust `drop(File)` có thay được `sync_all` không?
<details><summary>Đáp án</summary>

Không. Docs nói `sync_all` thường đắt hơn việc đóng file bằng drop vì drop không bắt buộc chặn đến khi dữ liệu được ghi xuống filesystem.[^rust-drop]

</details>

**Bài tập mở rộng.** (1) Tạo file trước bằng cách ghi sẵn `records × 4096` byte rồi `sync_all`, sau đó đo lại (2) và (3) bằng cách *ghi đè* trong file (`write_at` từ `FileExt`) thay vì nối thêm: kích thước không đổi nên theo man page `fdatasync` có thể bỏ qua flush metadata.[^fdatasync-def] Ghi lại chênh lệch và đối chiếu với dự đoán 3. (2) Thêm chế độ mở file với `custom_flags(libc::O_DSYNC)`/`O_SYNC`[^rs-customflags] và so với `sync_data`/`sync_all`; đọc `open(2)` về hai cờ trước[^osync-def] (tóm tắt ở [chương 06 §6](../05-storage/06-durability-trong-thuc-te.md) và [chương 04 §5](../05-storage/04-page-cache-va-direct-io.md)).

## Lab 2: mẫu ghi an toàn — file tạm, `fsync`, `rename`, `fsync` thư mục

**Mục tiêu.** Hiểu từng bước của mẫu "ghi file mới rồi `rename` đè" và vì sao thiếu bước nào cũng hở. Dùng một bin "giả lập sập" dừng sau bước N để xem người đọc khác thấy trạng thái nào.

**Đọc trước.** [Chương 06 §3–4](../05-storage/06-durability-trong-thuc-te.md), [Chương 02 §4](../05-storage/02-file-va-directory-api.md), [Chương 08 §4](../05-storage/08-rust-va-luu-tru.md).

**Chạy.**

```bash
cd labs
cargo run -q -p lab-storage --bin atomic_write -- /tmp/lab-storage naive 1
cargo run -q -p lab-storage --bin atomic_write -- /tmp/lab-storage naive 2
cargo run -q -p lab-storage --bin atomic_write -- /tmp/lab-storage safe  3
cargo run -q -p lab-storage --bin atomic_write -- /tmp/lab-storage safe  4
cargo run -q -p lab-storage --bin atomic_write -- /tmp/lab-storage safe          # chạy hết (không dừng)
# Linux: ./labs/linux.sh cargo run -q -p lab-storage --bin atomic_write -- /work/labs/target-linux/atomic safe 3
```

Tham số: `<dir> <naive|safe> [stop_after]`. Mỗi lần chạy bắt đầu bằng việc ghi `OLD` vào `config.txt`, rồi cập nhật thành `NEW` (dài hơn) bằng chế độ đã chọn, **dừng sau bước `stop_after`** (0 = không dừng), rồi in nội dung `config.txt` và danh sách thư mục — đúng thứ mà một tiến trình khác sẽ thấy.

- `naive` (4 bước): mở với `truncate` → ghi nửa đầu → ghi nửa sau → `sync_all`.
- `safe` (5 bước): tạo file tạm cạnh đích bằng `create_new` → ghi hết → `sync_all` file tạm → `rename` tạm thành đích → `sync_all` thư mục cha.

**Lưu ý về phạm vi.** "Dừng" ở đây là dừng *tiến trình* (hàm trả về sớm). Page cache thuộc về kernel nên dữ liệu đã `write` vẫn còn sau khi tiến trình chết, vì vậy lab này chỉ cho thấy **tính nguyên tử khi quan sát** giữa các bước; nó **không** mô phỏng mất điện, nơi chuyện đã `fsync` hay chưa mới quyết định cái gì còn trên đĩa (suy luận của người soạn; chương 06 §1 phân tích riêng trường hợp mất điện).

**Dự đoán theo lý thuyết.**

| Chế độ, `stop_after` | Nội dung `config.txt` dự đoán | Cơ sở |
| --- | --- | --- |
| `naive`, 1 | Rỗng (0 byte) | Mở với cờ truncate cắt file đã có về độ dài 0[^man-otrunc] |
| `naive`, 2 | Nửa đầu của `NEW` | Hệ quả của việc ghi từng đoạn; không có bước nào ẩn nửa file (suy luận của người soạn) |
| `naive`, 3–4 | `NEW` đầy đủ trên mọi cách đọc; chưa chắc bền | `write` thành công không bảo đảm dữ liệu đã lên đĩa[^write-nogarantee]; bước 4 mới gọi `sync_all` |
| `safe`, 1–3 | Vẫn là `OLD` nguyên vẹn; thư mục còn một file `config.txt.tmp.<pid>` | Chưa `rename` nên đường dẫn đích chưa đổi (suy luận của người soạn từ [^rename-atomic]) |
| `safe`, 4–5 | `NEW` đầy đủ | `rename` thay đích một cách nguyên tử: không có lúc nào một tiến trình khác thấy đích bị thiếu[^rename-atomic] |

Một số điểm cần chú ý:

- Ở `safe` không có thời điểm nào người đọc thấy file rỗng hay nửa vời: đường dẫn đích hoặc trỏ tới `OLD` hoặc tới `NEW`. Đó là toàn bộ lý do dùng mẫu này thay cho ghi đè tại chỗ.
- Bước 3 (fsync file tạm *trước* `rename`) và bước 5 (fsync thư mục) là phần **độ bền**, không phải phần nguyên tử, và lab này không quan sát được chúng. Lý thuyết: `fsync` file không đảm bảo entry trong thư mục chứa nó đã lên đĩa; muốn vậy phải `fsync` cả file descriptor của thư mục.[^fsync-dir] Một số filesystem còn tự thêm heuristic: ext4 với `auto_da_alloc` ép khối dữ liệu của file mới xuống đĩa trước khi `rename` được commit,[^ext4-autoda] nhưng đó là hành vi của ext4 chứ không phải bảo đảm chung (nhận xét của người soạn). PostgreSQL làm đủ hai bước, ghi rõ trong mã `durable_rename`.[^pg-rename]
- File tạm đặt **cùng thư mục** với đích: `rename` qua hai mount point khác nhau thất bại với `EXDEV`.[^rename-exdev] Docs Rust cũng nói `fs::rename` không hoạt động nếu tên mới ở mount point khác.[^rs-rename]
- Dùng `create_new` cho file tạm: nếu tên đã tồn tại thì mở thất bại thay vì đè lên file của ai đó, và việc kiểm-rồi-tạo là một thao tác nguyên tử.[^rs-createnew-atomic]

**Câu hỏi.**

1. Với `naive 1`, vì sao một tiến trình khác đọc `config.txt` ở thời điểm đó có thể đọc ra file rỗng?
<details><summary>Đáp án</summary>

Bước 1 mở file với truncate nên nội dung cũ bị cắt về 0 byte[^man-otrunc] trước khi dữ liệu mới được ghi; giữa hai việc đó đường dẫn đích chỉ có file rỗng.

</details>

2. Với `safe 3`, đường dẫn đích chứa gì và thư mục có gì lạ?
<details><summary>Đáp án</summary>

Đích vẫn là `OLD`; thư mục có thêm file tạm `config.txt.tmp.<pid>` vì chưa `rename` (suy luận từ tính nguyên tử của `rename`[^rename-atomic]). Chương trình thật cần dọn file tạm mồ côi khi khởi động lại (nhận xét của người soạn).

</details>

3. Nếu bỏ bước `sync_all` thư mục (bước 5) thì lý thuyết cho phép chuyện gì xảy ra sau mất điện?
<details><summary>Đáp án</summary>

`fsync` file không bảo đảm entry thư mục chứa file đã lên đĩa[^fsync-dir], nên theo lý thuyết sau mất điện tên mới có thể chưa tồn tại (suy luận của người soạn; chi tiết từng filesystem ở chương 06 §3).

</details>

4. Vì sao không được đặt file tạm ở `/tmp` rồi `rename` vào thư mục dữ liệu?
<details><summary>Đáp án</summary>

Hai đường dẫn có thể thuộc hai filesystem khác nhau, khi đó `rename` lỗi `EXDEV`.[^rename-exdev]

</details>

**Bài tập mở rộng.** Chạy `safe 3` rồi chạy lại `safe` lần nữa trên cùng thư mục và liệt kê thư mục: file tạm của lần trước còn đó không? Viết hàm dọn file `*.tmp.*` mồ côi. Thử cả `safe` trong thư mục có quyền ghi nhưng đích nằm trên mount khác, và ghi lại mã lỗi.

## Lab 3: page cache, `mincore` và `posix_fadvise` (Linux)

**Mục tiêu.** Nhìn thấy trực tiếp số page của một file đang nằm trong page cache, rồi dùng `posix_fadvise` để đổi trạng thái đó; thấy khác biệt giữa đọc "lạnh" và đọc "nóng".

**Đọc trước.** [Chương 04 §1](../05-storage/04-page-cache-va-direct-io.md) (page cache), [§2](../05-storage/04-page-cache-va-direct-io.md) (readahead và `posix_fadvise`), [§3](../05-storage/04-page-cache-va-direct-io.md) (ghi chỉ chép vào page cache).

**Chạy.**

```bash
./labs/linux.sh cargo run -q -p lab-storage --bin page_cache -- /work/labs/target-linux/pc 128
```

Tham số `<dir> [size_mib]` (mặc định 128 MiB). Bin tạo file, `sync_all`, rồi báo "số page resident / tổng số page" qua `mmap` + `mincore` ở từng bước: sau khi ghi; sau `posix_fadvise(DONTNEED)`; sau lần đọc đầy đủ thứ nhất (kèm thời gian); lần đọc thứ hai (kèm thời gian); sau `DONTNEED` lần nữa; ngay sau `posix_fadvise(WILLNEED)` và 2 giây sau.

Kỹ thuật đo là chính cách man page `posix_fadvise` gợi ý: mở file, `mmap`, rồi `mincore` trên vùng ánh xạ.[^fadv-mincore] `mincore` báo từng page có đang nằm trong RAM (và truy cập sẽ không gây truy cập đĩa) hay không.[^mincore-def] Việc `mmap` chỉ tạo ánh xạ nên không tự đọc file vào cache (suy luận của người soạn, xem [lab P3 mục 1.3](03-memory-lab.md) về `mmap` chưa chạm page).

**Dự đoán theo lý thuyết.** Quan trọng: lời khuyên `posix_fadvise` *không ràng buộc*, nó chỉ là kỳ vọng của ứng dụng,[^fadv-nb] nên mọi dự đoán dưới đây là "xu hướng", không phải bảo đảm.

1. **Sau khi ghi + `sync_all`:** `write` thông thường đi qua page cache,[^pcg] nên nhiều page của file nên đang resident. Nguồn của chương không nói `fsync` có loại page khỏi cache hay không; đây chính là điều bước 1 trả lời cho bạn.
2. **Sau `DONTNEED`:** `POSIX_FADV_DONTNEED` cố gắng giải phóng các page cache của vùng chỉ định;[^fadv-dontneed-def] số resident nên giảm mạnh (không chắc về 0, vì lời khuyên không ràng buộc). Page **dirty** thì không bị giải phóng[^fadv-dontneed] — đó là lý do bin gọi `sync_all` trước.
3. **Đọc lần đầu:** các page chưa có trong cache nên phải nạp từ thiết bị, và đọc thông thường đi qua page cache[^pcg] nên sau lần đọc resident quay lại cao. Kernel còn đọc trước (readahead): VFS có hook để VM đọc nhiều page liên tiếp cùng lúc.[^vfs-ra] Dự đoán: lần đọc đầu chậm hơn lần hai; bao nhiêu lần phụ thuộc thiết bị và host.
4. **Đọc lần hai:** dữ liệu đã resident nên không cần chạm thiết bị; dự đoán nhanh hơn lần đầu. (Nếu máy thiếu RAM hoặc cgroup giới hạn thấp hơn kích thước file, kỳ vọng này có thể sai — đọc [chương 04 §6](../05-storage/04-page-cache-va-direct-io.md).)
5. **`WILLNEED`:** khởi động một lần đọc *không chặn* vùng đó vào page cache.[^fadv-willneed] "Không chặn" nghĩa là ngay sau lời gọi resident có thể mới chỉ một phần; vài giây sau nên nhiều hơn. Man page cũng nói lượng dữ liệu thực đọc có thể bị kernel giảm bớt, nên không cần bằng 100%.[^fadv-willneed]
6. Trên Linux, `POSIX_FADV_RANDOM` tắt hẳn readahead còn `SEQUENTIAL` nhân đôi cửa sổ readahead[^fadv-linux] (bài tập 2).

**Câu hỏi.**

1. Vì sao bin phải `sync_all` trước khi gọi `DONTNEED`?
<details><summary>Đáp án</summary>

Page dirty chưa ghi ra thì không bị giải phóng bởi `DONTNEED`.[^fadv-dontneed]

</details>

2. `mincore` trả về gì cho từng page, và vì sao cần `mmap` file mới dùng được?
<details><summary>Đáp án</summary>

`mincore` trả một vector cho biết page của *bộ nhớ ảo* của tiến trình có resident (và truy cập không gây truy cập đĩa) hay không;[^mincore-def] nên phải ánh xạ file vào không gian địa chỉ trước. Man page `posix_fadvise` mô tả đúng cách này.[^fadv-mincore]

</details>

3. `posix_fadvise` trả lỗi bằng cách nào khác phần lớn syscall?
<details><summary>Đáp án</summary>

Thành công trả 0; lỗi trả *mã lỗi trực tiếp* chứ không đặt `errno`.[^fadv-ret] Hàm `fadvise` trong `lib.rs` dùng `io::Error::from_raw_os_error(rc)` vì lý do này.

</details>

4. Lần đọc thứ hai nhanh hơn lần đầu nói lên điều gì về "đọc từ đĩa"?
<details><summary>Đáp án</summary>

Phần lớn thời gian lần đầu là chờ thiết bị; khi page đã nằm trong cache thì đọc chỉ là chép từ RAM (suy luận của người soạn từ [^pcg]). Đây là lý do một con số "đọc file nhanh" đo trên máy đã ấm không dùng để dự báo hiệu năng cold start.

</details>

**Bài tập mở rộng.** (1) Giảm `size_mib` xuống 1 và 8 và xem quan hệ resident/total sau `DONTNEED` thay đổi thế nào. (2) Thêm bước `POSIX_FADV_RANDOM` và `SEQUENTIAL` trước lần đọc đầu (cần `fadvise` lấy tham số `advice` — đã có sẵn), so sánh thời gian đọc cold; nhớ rằng đó là lời khuyên không ràng buộc.[^fadv-nb] (3) Dùng `/proc/meminfo` (`Cached`) trước và sau để thấy page cache toàn hệ thống thay đổi: trường này là "bộ nhớ đệm cho file đọc từ đĩa".[^proc-cached]

## Lab 4: `O_DIRECT` — bỏ qua page cache và luật căn lề (Linux)

**Mục tiêu.** Quan sát rằng đọc bằng `O_DIRECT` không làm đầy page cache, rằng hai lần đọc liên tiếp không nhanh dần như đọc thường, và đụng vào luật căn lề địa chỉ/độ dài/offset.

**Đọc trước.** [Chương 04 §7](../05-storage/04-page-cache-va-direct-io.md) (`O_DIRECT`), [§5](../05-storage/04-page-cache-va-direct-io.md) (`O_SYNC`/`O_DSYNC`, vì `O_DIRECT` không thay được `fsync`).

**Chạy.**

```bash
./labs/linux.sh cargo run -q -p lab-storage --bin direct_io -- /work/labs/target-linux/dio 128
```

Nếu `open(O_DIRECT)` lỗi `EINVAL`, filesystem dưới thư mục đó không hỗ trợ cờ này: đổi thư mục (ví dụ thư mục khác mount) rồi chạy lại.[^open-einval] Bin: tạo file bằng ghi thường + `sync_all` + `DONTNEED`; đo resident; đọc hai lượt bằng `O_DIRECT` với buffer 1 MiB căn 4096; đọc hai lượt đọc thường để so sánh; cuối cùng thử bốn yêu cầu `O_DIRECT` cố ý lệch căn lề và in `Ok(n)` hoặc lỗi.

`custom_flags(libc::O_DIRECT)` là đường của Rust để chuyền cờ thẳng cho `open`.[^rs-customflags] Buffer căn lề dựng bằng `std::alloc::Layout` với `align = 4096` (struct `AlignedBuf` trong `lib.rs`); không có gì trong docs bảo đảm `Vec<u8>` thông thường căn 4096 (nhận xét của người soạn).

**Dự đoán theo lý thuyết.**

1. **Cache:** `O_DIRECT` làm I/O đi thẳng giữa buffer người dùng và thiết bị,[^open-directbuf] và page cache "có thể bị bỏ qua (ví dụ bằng `O_DIRECT`)".[^pcg] Dự đoán: số resident sau hai lượt đọc direct gần như không đổi so với bước 0. Man page chỉ nói cờ này *cố gắng giảm* hiệu ứng cache,[^open-direct] nên đừng kỳ vọng tuyệt đối.
2. **Lần hai không nhanh hơn:** vì không có cache để hưởng, dự đoán thời gian hai lượt direct cùng cỡ nhau (khác với hai lượt đọc thường ở cuối, nơi lượt hai nên nhanh hơn như lab 3). Thiết bị bên dưới có cache riêng thì có thể vẫn nhanh dần; đó không phải page cache (nhận xét của người soạn).
3. **Lệch căn lề:** `O_DIRECT` có thể áp ràng buộc căn lề lên độ dài, địa chỉ buffer *và* offset trong file;[^open-align] từ Linux 2.6.0 đơn vị là kích thước block logic của thiết bị (thường 512 byte) — trước đó là kích thước page.[^open-align-hist] Cách xử lý I/O lệch căn lề cũng khác nhau: có thể lỗi `EINVAL`, có thể rơi về buffered I/O.[^open-direct-fallback] Dự đoán: dòng "aligned, len 4096, offset 0" thành công; ba dòng còn lại (offset 1; len 100; địa chỉ + 1) *có thể* lỗi hoặc *có thể* thành công do fallback — kết quả phụ thuộc filesystem và kernel của máy bạn, nên đây là dữ liệu bạn phải tự ghi lại.
4. Bộ đệm 4096 là lựa chọn thận trọng chứ không phải con số bắt buộc (suy luận của người soạn từ [^open-align-hist]). Từ Linux 6.1 có thể hỏi chính xác ràng buộc của một file bằng `statx` với `STATX_DIOALIGN`[^open-statx] (bài tập 2).
5. `O_DIRECT` tự nó cố gắng làm I/O đồng bộ nhưng không cho các bảo đảm của `O_SYNC`; muốn bảo đảm phải dùng thêm `O_SYNC`.[^open-direct-sync]

**Câu hỏi.**

1. Vì sao `O_DIRECT` không thay được `fsync` khi cần độ bền?
<details><summary>Đáp án</summary>

Cờ này chỉ cố gắng truyền dữ liệu đồng bộ nhưng không cho các bảo đảm của `O_SYNC` về dữ liệu và metadata cần thiết.[^open-direct-sync]

</details>

2. Hai tiến trình, một đọc bằng `O_DIRECT` và một đọc thường cùng vùng file, có vấn đề gì?
<details><summary>Đáp án</summary>

Man page khuyên tránh trộn `O_DIRECT` với I/O thường trên cùng file, nhất là các vùng byte chồng nhau.[^open-mix]

</details>

3. Vì sao các hệ quản trị CSDL vẫn phải mở file bằng cờ này dù đọc thường có cache của OS?
<details><summary>Đáp án</summary>

Chương 04 §7 trình bày: CSDL có buffer pool riêng nên cache của OS thành bản sao thừa; tuy vậy man page khuyên coi `O_DIRECT` là tùy chọn hiệu năng, tắt theo mặc định.[^open-default] Đọc chương để biết trade-off.

</details>

**Bài tập mở rộng.** (1) Thử với thư mục scratch trên hai filesystem khác nhau (ví dụ thư mục bind-mount `/work/...` và `/dev/shm`), ghi lại kết quả `open` và bốn dòng căn lề của mỗi nơi. (2) Dùng `libc::statx` (cần Linux ≥ 6.1, kiểm tra bằng `uname -r`)[^open-statx] để in `stx_dio_mem_align`/`stx_dio_offset_align` của file rồi so với kết quả thực nghiệm ở bốn dòng căn lề. (3) Thêm `O_DIRECT` ghi + `fsync`, rồi so với ghi thường + `fsync` ở lab 1.

## Lab 5: inode, hard link, `stat` và `read_dir`

Lab này gồm hai bin nhỏ, không cần Linux.

### 5a. `inode_stat`: số inode, `nlink`, hard link, symlink, `rename`, `unlink` khi file đang mở

**Mục tiêu.** Thấy rằng tên và file là hai thứ khác nhau: nhiều tên có thể cùng trỏ vào một inode; `nlink` đếm các tên; `rename` đổi tên không đổi inode; `unlink` chỉ bỏ tên.

**Đọc trước.** [Chương 02 §3](../05-storage/02-file-va-directory-api.md) (`unlink`, hard link, symlink, đếm tham chiếu), [Chương 03 §2](../05-storage/03-cai-dat-file-system.md) (inode), [Chương 03 §8](../05-storage/03-cai-dat-file-system.md) (Rust: `MetadataExt`).

**Chạy.**

```bash
cd labs && cargo run -q -p lab-storage --bin inode_stat -- /tmp/lab-storage-inode
./labs/linux.sh cargo run -q -p lab-storage --bin inode_stat -- /work/labs/target-linux/inode_lab /dev/shm   # tham số 2: thư mục ở filesystem khác
```

Bin in `dev`, `ino`, `nlink`, `len`, `blocks512` (qua `MetadataExt`) và cờ `symlink` sau từng bước: (1) tạo `a.txt`; (2) `hard_link(a → b)`; (3) `symlink(a → s)` rồi so `fs::metadata(s)` với `fs::symlink_metadata(s)`; (4) `rename(b → c)`; (5) xóa `a.txt`; (6) mở `c.txt` rồi xóa nó khi fd còn mở; (7) `hard_link` vào một thư mục; (8, nếu có tham số 2) `hard_link` sang filesystem khác.

**Dự đoán theo lý thuyết.**

1. **Bước 1:** mỗi file có một inode chứa metadata của nó;[^inode-meta] số inode (i-number) là "tên cấp thấp" của file.[^ostep-inum] `nlink` = số hard link tới file[^inode-nlink] nên dự đoán `nlink=1`. `blocks512` tính theo đơn vị 512 byte[^rust-blocks] — lab không dự đoán giá trị cụ thể, chỉ dự đoán nó khác `len` (độ dài logic khác dung lượng cấp phát; chương 03 §8).
2. **Bước 2:** `a` và `b` có cùng `dev` và `ino`, `nlink=2` ở cả hai: cả hai tên cùng trỏ vào một file, cùng quyền và chủ sở hữu, và không thể biết tên nào là "bản gốc".[^link-def] Số inode chỉ bảo đảm duy nhất trong một filesystem,[^inode-unique] vì vậy phải so cả `dev` lẫn `ino`.
3. **Bước 3:** `fs::metadata` tương ứng `stat` trên Unix[^rust-stat] nên đi theo symlink: kết quả mang `ino` của `a`. `fs::symlink_metadata` tương ứng `lstat`[^rust-metadata] — trả thông tin về chính symlink,[^lstat] tức một inode **khác**, `symlink=true`, và `len` bằng độ dài chuỗi đường dẫn nó chứa (không tính byte NUL cuối).[^inode-size] Bin in độ dài đường dẫn của `a.txt` ngay bên dưới để bạn so.
4. **Bước 4:** `rename` giữ nguyên inode và các hard link khác không bị ảnh hưởng.[^rename-def] Dự đoán: `c.txt` có đúng `ino` đã thấy ở bước 2, `nlink` vẫn 2.
5. **Bước 5:** xóa một tên chỉ giảm số tham chiếu: file chỉ bị giải phóng khi count về 0.[^ostep-unlink] `c.txt` còn `nlink=1`, dữ liệu nguyên vẹn. Symlink `s` giờ trỏ tới tên không còn tồn tại; `stat` lên nó nên lỗi vì "một thành phần của pathname không tồn tại hoặc là symlink treo".[^stat-enoent] (Lab in `ErrorKind`; việc ánh xạ errno sang `ErrorKind::NotFound` là của std, bạn đối chiếu bằng cách đọc output.)
6. **Bước 6:** tên cuối cùng bị xóa nhưng file vẫn đang mở: file chưa bị xóa hẳn cho tới khi fd cuối cùng đóng.[^unlink-open] Dự đoán: đọc qua fd vẫn được; `File::metadata` qua fd trả `nlink=0` (suy luận của người soạn: không còn tên nào); `c.exists()` là `false`. Đây là lý do server "xóa log mà `df` không giảm" (nhận xét của người soạn; [chương 02 §3](../05-storage/02-file-va-directory-api.md) nêu điều kiện "không còn fd mở" và câu tự kiểm tra 3 cuối chương hỏi đúng tình huống này).
7. **Bước 7:** `link(2)` trả `EPERM` khi nguồn là thư mục.[^link-eperm] Lý do thiết kế: hard link tới thư mục có thể tạo vòng lặp trong cây thư mục làm nhiều chương trình rối.[^symlink-hard]
8. **Bước 8 (nếu có):** hard link không hoạt động giữa các mount khác nhau, kể cả cùng một filesystem mount hai nơi[^link-exdev] — dự đoán lỗi `EXDEV`. Symlink thì khác: nó là con trỏ tới *tên*, nên có thể qua thư mục và qua ranh giới filesystem.[^symlink-def]

**Câu hỏi.**

1. Sau bước 2, nếu bạn ghi thêm vào `b.txt`, `a.txt` có thấy không? Vì sao?
<details><summary>Đáp án</summary>

Có: hai tên cùng trỏ vào một file[^link-def] (cùng inode nên cùng nội dung).

</details>

2. Vì sao `rename` trong cùng filesystem không đổi `ino` nhưng `cp` thì có?
<details><summary>Đáp án</summary>

`rename` chỉ đổi tên, các hard link khác và fd đang mở không bị ảnh hưởng;[^rename-def] `cp` tạo file mới nên có inode mới (suy luận của người soạn; `cp` không có trong bin, thử bằng tay và đọc `stat`).

</details>

3. Thư mục cha của `a.txt` chứa gì để biết `a.txt` là inode nào?
<details><summary>Đáp án</summary>

Thư mục là danh sách cặp (tên đọc được, tên cấp thấp = inode number).[^ostep-dir]

</details>

**Bài tập mở rộng (5a).** (1) Thêm bước tạo symlink tới một *thư mục* rồi so `fs::metadata` với `fs::symlink_metadata` trên nó. (2) Chạy bước 8 với tham số thứ hai là một thư mục ở filesystem khác và ghi lại mã lỗi so với dự đoán `EXDEV`.

### 5b. `read_dir_meta`: thứ tự `read_dir`, `d_ino` và `DirEntry::metadata`

**Mục tiêu.** Biết đừng dựa vào thứ tự `read_dir`; phân biệt `DirEntry::file_type`/`metadata` (không theo symlink) với `fs::metadata` (theo symlink); thấy `d_ino`.

**Đọc trước.** [Chương 08 §6](../05-storage/08-rust-va-luu-tru.md) (`read_dir`, metadata), [Chương 03 §4](../05-storage/03-cai-dat-file-system.md) (thư mục là danh sách (tên, inode)), [Chương 02 §3](../05-storage/02-file-va-directory-api.md) (symlink).

**Chạy.**

```bash
cd labs && cargo run -q -p lab-storage --bin read_dir_meta -- /tmp/lab-storage-rd
./labs/linux.sh cargo run -q -p lab-storage --bin read_dir_meta
./labs/linux.sh bash -c 'cargo build -q -p lab-storage --bin read_dir_meta && strace -f -e trace=getdents64,statx,newfstatat,lstat ./target-linux/debug/read_dir_meta'
```

Bin tạo 7 file theo thứ tự không theo bảng chữ cái, một thư mục con, một symlink hợp lệ và một symlink treo. Rồi: gọi `read_dir` hai lần và in thứ tự; in bảng `(tên, d_ino, loại từ file_type(), len/ino từ metadata())` sau khi sắp xếp; so `fs::metadata` (theo symlink) với kết quả của entry.

**Dự đoán theo lý thuyết.**

1. **Thứ tự:** docs nói thứ tự `read_dir` phụ thuộc nền tảng và filesystem[^rs-readdir-platform] và có thể thay đổi giữa các lần gọi; cần ổn định thì phải tự sắp xếp.[^rs-readdir-order] Dự đoán: thứ tự in ra *không được bảo đảm* trùng thứ tự tạo hay thứ tự bảng chữ cái; hai lượt có thể giống nhau trên máy bạn nhưng docs không hứa. Ghi lại cả hai lượt và đối chiếu với thứ tự tạo.
2. **`.` và `..`:** các entry thư mục hiện tại và cha bị bỏ qua.[^readdir-dots] Dòng cuối của bin (`contains "." or ".."?`) in `false`.
3. **`d_ino`:** `DirEntryExt::ino` trả đúng trường `d_ino` của dirent;[^dirent-ino] thư mục là danh sách cặp (tên, inode).[^ost-dir] Với file thường dự đoán `entry.ino()` bằng `md.ino` của `entry.metadata()`.
4. **`entry.metadata()` không theo symlink:** trên Unix nó tương đương `symlink_metadata` trên đường dẫn,[^dirent-meta] nên với `link-to-alpha` và `dangling` đều trả `Ok`, `len` là độ dài chuỗi đường dẫn mà symlink chứa, `ino` là inode của chính symlink.[^inode-size] Trong khi đó `fs::metadata("dangling")` tương ứng `stat`[^rust-stat] nên đi theo link tới đích không tồn tại và lỗi.[^stat-enoent] Với `link-to-alpha`, `fs::metadata` ra thông tin của `alpha.txt`.
5. **`file_type()` rẻ:** docs nói trên hầu hết nền tảng Unix hàm này "miễn phí" (không cần syscall thêm), dù một số nền tảng có thể phải gọi tương đương `symlink_metadata`.[^dirent-ftype] Lý do plausible trên ext4: filesystem lưu bản sao loại file ngay trong entry thư mục.[^e4-ftype] Kiểm chứng bằng `strace` (lệnh thứ ba): bin gọi cả `file_type()` lẫn `metadata()` trong cùng vòng lặp, nên hãy so số lời gọi `statx`/`newfstatat` với số entry — nếu `file_type()` cần syscall riêng thì số lời gọi sẽ nhiều gần gấp đôi số entry (suy luận của người soạn). `read_dir` đi qua `readdir` của libc trên Unix.[^readdir-map]

**Câu hỏi.**

1. Vì sao test của bạn không được `assert_eq!` thứ tự `read_dir` với thứ tự tạo file?
<details><summary>Đáp án</summary>

Docs không bảo đảm thứ tự: nó phụ thuộc nền tảng/filesystem và có thể đổi giữa các lần gọi; cần ổn định thì sắp xếp tường minh.[^rs-readdir-order][^rs-readdir-platform]

</details>

2. `entry.file_type()?.is_dir()` trên một symlink trỏ tới thư mục trả gì? Vì sao điều đó quan trọng với đoạn mã đi bộ cây thư mục?
<details><summary>Đáp án</summary>

`file_type()` không theo symlink,[^dirent-nosym] nên entry đó báo là symlink chứ không phải thư mục (bin in `symlink` cho `link-to-alpha`). Duyệt đệ quy theo `is_dir()` vì vậy không đi vào symlink-tới-thư mục, tránh vòng lặp (suy luận của người soạn).

</details>

3. Vì sao `fs::metadata("dangling")` lỗi mà `entry.metadata()` của cùng entry thì không?
<details><summary>Đáp án</summary>

`fs::metadata` ~ `stat` đi theo link tới đích không tồn tại → lỗi;[^rust-stat][^stat-enoent] `entry.metadata()` ~ `symlink_metadata` chỉ hỏi chính symlink.[^dirent-meta]

</details>

**Bài tập mở rộng (5b).** (1) Thêm một vòng đệ quy duyệt cây thư mục chỉ dùng `file_type()` và kiểm tra nó không đi vào `link-to-alpha`; sau đó sắp xếp kết quả để test ổn định. (2) Chạy `strace -c -e trace=getdents64,statx,newfstatat` và đếm syscall theo số entry để kiểm tra dự đoán 5.

## Lab 6: quan sát writeback qua `/proc/meminfo` (Linux)

**Mục tiêu.** Nhìn `Dirty` và `Writeback` tăng khi ghi có đệm, rồi giảm khi kernel xả xuống đĩa hoặc khi `sync_all`; liên hệ với các núm `vm.dirty_*`.

**Đọc trước.** [Chương 04 §3](../05-storage/04-page-cache-va-direct-io.md) (ghi chỉ chép vào page cache), [§4](../05-storage/04-page-cache-va-direct-io.md) (writeback, bốn núm `vm.dirty_*`), [§5](../05-storage/04-page-cache-va-direct-io.md) (`fsync`).

**Chạy.**

```bash
./labs/linux.sh cargo run -q -p lab-storage --bin writeback -- /work/labs/target-linux/wb 512 40
```

Tham số `<dir> [size_mib] [seconds]` (mặc định 512 MiB, 40 giây). Bin in các giá trị `vm.dirty_*` hiện tại (đọc `/proc/sys/vm/`), lấy mẫu `Dirty` và `Writeback` (kB) ở trạng thái nền, sau khi ghi `size_mib` MiB **không sync**, mỗi giây trong `seconds` giây chờ, sau `sync_all` (kèm thời gian của `sync_all`) và sau khi xóa file.

Lưu ý: `/proc/meminfo` là số của kernel đang chạy (nhận xét của người soạn: trong container đó là kernel của máy ảo Docker, và là số toàn máy, không riêng tiến trình của bạn — tiến trình khác gây nhiễu). Chạy khi máy rảnh.

**Dự đoán theo lý thuyết.**

1. **`Dirty`** là bộ nhớ đang chờ được ghi trở lại đĩa; **`Writeback`** là bộ nhớ *đang* được ghi.[^proc-dirty][^proc-wb] `write` chỉ chép vào page cache và không bảo đảm gì về đĩa[^write-nogarantee] nên sau khi ghi, `Dirty` nên tăng.
2. **Tăng bao nhiêu?** Không nhất thiết bằng `size_mib`. Kernel có ngưỡng nền: số page mà tại đó các luồng flusher nền bắt đầu ghi dữ liệu dirty ra;[^vm-bg] và ngưỡng cứng hơn: số page mà tại đó *chính tiến trình đang ghi* phải tự ghi dữ liệu dirty.[^vm-dr] Hai ngưỡng tính trên "bộ nhớ khả dụng", không phải tổng RAM.[^vm-notequal] Dự đoán: nếu `size_mib` nhỏ hơn ngưỡng nền của máy bạn (tính từ các `vm.dirty_*` in ở đầu) thì `Dirty` tăng gần bằng `size_mib` và đứng đó chờ; nếu lớn hơn, `Writeback` xuất hiện sớm và `Dirty` bị chặn quanh ngưỡng trong lúc `write_all` còn chạy. Chỉ một trong cặp `*_ratio` / `*_bytes` được dùng tại một thời điểm.[^vm-bytes]
3. **Khi chờ (không sync):** dữ liệu dirty quá "tuổi" `dirty_expire_centisecs` sẽ được ghi ra ở lần flusher thức dậy kế tiếp,[^vm-exp] và flusher thức dậy định kỳ theo `dirty_writeback_centisecs`.[^vm-wbc2] Đặt giá trị này bằng 0 là tắt hẳn writeback định kỳ.[^vm-wbc] Dự đoán: `Dirty` giữ nguyên một lúc rồi giảm dần; thời điểm giảm cỡ `dirty_expire_centisecs / 100` giây cộng tối đa một chu kỳ thức dậy. Giáo trình OSTEP mô tả hệ thống thường đệm ghi "từ năm đến ba mươi giây".[^osi-5-30] Nếu `seconds` nhỏ hơn thời gian đó, bạn sẽ chưa thấy `Dirty` giảm — tăng `seconds`.
4. **`sync_all`:** buộc dữ liệu dirty của file đó xuống đĩa;[^ostep-fsync] dự đoán `sync_all` còn mất đáng kể thời gian nếu lúc gọi `Dirty` còn cao, và gần như tức thì nếu flusher đã xả hết (ghi lại cả hai trường hợp bằng cách chạy với `seconds` = 0 và `seconds` lớn).
5. **Sau `remove_file`:** lab này không đưa dự đoán có nguồn; bạn chỉ quan sát và ghi lại. (Câu hỏi mở: điều gì xảy ra với page dirty của file bị xóa? Chương 04 không trả lời; đọc kernel docs trước khi khẳng định.)

**Câu hỏi.**

1. Vì sao `write_all` 512 MiB có thể trả về trong khi `Dirty` thấp hơn 512 MiB?
<details><summary>Đáp án</summary>

Flusher nền bắt đầu ghi khi vượt ngưỡng nền,[^vm-bg] và khi vượt ngưỡng cứng thì tiến trình ghi tự phải ghi dữ liệu dirty[^vm-dr] — phần đó đã chuyển sang `Writeback` hoặc xuống đĩa; thêm vào đó, ngưỡng tính trên bộ nhớ khả dụng chứ không phải tổng RAM.[^vm-notequal]

</details>

2. Nếu bạn đặt `vm.dirty_writeback_centisecs = 0`, điều gì thay đổi trong lab?
<details><summary>Đáp án</summary>

Writeback định kỳ bị tắt hẳn;[^vm-wbc] dữ liệu dirty không còn được xả theo chu kỳ — còn ngưỡng nền/ngưỡng cứng và `sync_all` vẫn tác dụng như trên (suy luận của người soạn; thử bằng `sysctl` chỉ nên làm trong container/VM lab, đừng làm trên máy production).

</details>

3. Mất điện đúng lúc `Dirty` đang cao thì mất gì, và `Writeback` có giúp không?
<details><summary>Đáp án</summary>

Dữ liệu dirty chưa ghi ra thì không có bảo đảm gì về đĩa;[^write-nogarantee] `Writeback` chỉ là dữ liệu *đang* được ghi, cũng chưa xong.[^proc-wb] Cách bảo đảm là `fsync`.[^write-fsync]

</details>

**Bài tập mở rộng (lab 6).** (1) Chạy với `size_mib` nhỏ hơn và lớn hơn ngưỡng nền tính từ các `vm.dirty_*` in ở đầu và so cách `Dirty`/`Writeback` thay đổi (dự đoán 2). (2) Chạy với `seconds` = 0 rồi với `seconds` đủ lớn để so thời gian `sync_all` (dự đoán 4).

## Build và clippy

```bash
cd labs && cargo clippy -q -p lab-storage --all-targets -- -D warnings          # macOS (bin Linux-only được cfg ra ngoài)
cd labs && cargo clippy -q -p lab-storage --all-targets --target x86_64-unknown-linux-gnu -- -D warnings   # kiểm các bin Linux-only
./labs/linux.sh bash -c 'rustup component add clippy && cargo clippy -p lab-storage --all-targets -- -D warnings'
```

(Lệnh thứ hai chỉ cần target `x86_64-unknown-linux-gnu` đã cài trong rustup để kiểm cú pháp/lint; nó không liên kết hay chạy binary.)

## Bài tập chung

1. Với mỗi bin, trước khi chạy, ghi dự đoán của bạn ra giấy (số, thứ tự, lỗi gì) *kèm số chú thích bạn dựa vào*, rồi mới chạy. Lệch ở đâu thì tìm trong "Đi sâu" của chương tương ứng.
2. Kết hợp lab 2 và lab 1: đo chi phí của `safe` (hai lần `sync_all`: file và thư mục) so với `fs::write` thuần.
3. Kết hợp lab 3 và lab 6: ghi file 1 GiB, quan sát `Cached` và `Dirty` cùng lúc trong `/proc/meminfo` (`Cached` là cache của file đọc từ đĩa[^proc-cached]), rồi `DONTNEED` và quan sát lại.

[^write-nogarantee]: write(2), NOTES (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/write.2.en.html> — "A successful return from write() does not make any guarantee that data has been committed to disk."
[^write-fsync]: write(2), NOTES (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/write.2.en.html> — "The only way to be sure is to call fsync(2) after you are done writing all your data."
[^fsync-cache]: fsync(2), DESCRIPTION (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/fsync.2.en.html> — "This includes writing through or flushing a disk cache if present. The call blocks until the device reports that the transfer has completed."
[^fsync-def]: fsync(2), DESCRIPTION (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/fsync.2.en.html> — "so that all changed information can be retrieved even if the system crashes or is rebooted."
[^fdatasync-aim]: fsync(2), DESCRIPTION (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/fsync.2.en.html> — "The aim of fdatasync() is to reduce disk activity for applications that do not require all metadata to be synchronized with the disk."
[^fdatasync-def]: fsync(2), DESCRIPTION (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/fsync.2.en.html> — "On the other hand, a change to the file size (st_size, as made by say ftruncate(2)), would require a metadata flush."
[^src-fsync-linux]: rust-lang/rust tag 1.95.0, library/std/src/sys/fs/unix.rs, File::fsync (nhánh `#[cfg(not(target_vendor = "apple"))]`) — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/fs/unix.rs> — "unsafe fn os_fsync(fd: c_int) -> c_int { libc::fsync(fd) }"
[^src-datasync]: rust-lang/rust tag 1.95.0, library/std/src/sys/fs/unix.rs, File::datasync — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/fs/unix.rs> — "unsafe fn os_datasync(fd: c_int) -> c_int { libc::fdatasync(fd) }"
[^src-fsync]: rust-lang/rust tag 1.95.0, library/std/src/sys/fs/unix.rs, File::fsync (nhánh `#[cfg(target_vendor = "apple")]`) — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/fs/unix.rs> — "unsafe fn os_fsync(fd: c_int) -> c_int { libc::fcntl(fd, libc::F_FULLFSYNC) }"
[^src-datasync-apple]: rust-lang/rust tag 1.95.0, library/std/src/sys/fs/unix.rs, File::datasync (nhánh `#[cfg(target_vendor = "apple")]`) — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/fs/unix.rs> — "unsafe fn os_datasync(fd: c_int) -> c_int { libc::fcntl(fd, libc::F_FULLFSYNC) }"
[^rust-syncdata]: Rust std 1.95.0, File::sync_data — <https://doc.rust-lang.org/1.95.0/std/fs/struct.File.html> — "Note that some platforms may simply implement this in terms of sync_all."
[^xnu-fullfsync]: Apple XNU tag xnu-10002.1.13, bsd/sys/fcntl.h — <https://raw.githubusercontent.com/apple-oss-distributions/xnu/xnu-10002.1.13/bsd/sys/fcntl.h> — "fsync + ask the drive to flush to the media"
[^wcc-intro]: Explicit volatile write back cache control, Introduction — <https://docs.kernel.org/block/writeback_cache_control.html> — "That means the devices signal I/O completion to the operating system before data actually has hit the non-volatile storage."
[^rust-drop]: Rust std 1.95.0, File::sync_all — <https://doc.rust-lang.org/1.95.0/std/fs/struct.File.html> — "Note, however, that sync_all is generally more expensive than closing a file by dropping it, because the latter is not required to block until the data has been written to the filesystem."
[^rs-customflags]: Rust 1.95.0 std, OpenOptionsExt::custom_flags — <https://doc.rust-lang.org/1.95.0/std/os/unix/fs/trait.OpenOptionsExt.html> — "Pass custom flags to the flags argument of open."
[^osync-def]: open(2), O_SYNC (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "Write operations on the file will complete according to the requirements of synchronized I/O file integrity completion"
[^man-otrunc]: open(2), O_TRUNC — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "If the file already exists and is a regular file and the access mode allows writing (i.e., is O_RDWR or O_WRONLY) it will be truncated to length 0."
[^rename-atomic]: rename(2), DESCRIPTION (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/rename.2.en.html> — "If newpath already exists, it will be atomically replaced, so that there is no point at which another process attempting to access newpath will find it missing."
[^fsync-dir]: fsync(2), DESCRIPTION (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/fsync.2.en.html> — "Calling fsync() does not necessarily ensure that the entry in the directory containing the file has also reached disk. For that an explicit fsync() on a file descriptor for the directory is also needed."
[^ext4-autoda]: ext4 General Information, auto_da_alloc — <https://docs.kernel.org/admin-guide/ext4.html> — "the data blocks of the new file are forced to disk before the rename() operation is committed."
[^pg-rename]: PostgreSQL REL_16_0, src/backend/storage/file/fd.c, durable_rename — <https://raw.githubusercontent.com/postgres/postgres/REL_16_0/src/backend/storage/file/fd.c> — "To guarantee renaming the file is persistent, fsync the file with its new name, and its containing directory."
[^rename-exdev]: rename(2), ERRORS (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/rename.2.en.html> — "oldpath and newpath are not on the same mounted filesystem."
[^rs-rename]: Rust std, fs::rename — <https://doc.rust-lang.org/1.95.0/std/fs/fn.rename.html> — "Renames a file or directory to a new name, replacing the original file if to already exists. This will not work if the new name is on a different mount point."
[^rs-createnew-atomic]: Rust std, OpenOptions::create_new — <https://doc.rust-lang.org/1.95.0/std/fs/struct.OpenOptions.html> — "This option is useful because it is atomic. Otherwise between checking whether a file exists and creating a new one, the file may have been created by another process (a TOCTOU race condition / attack)."
[^fadv-mincore]: posix_fadvise(2), NOTES — <https://manpages.debian.org/bookworm/manpages-dev/posix_fadvise.2.en.html> — "One can obtain a snapshot of which pages of a file are resident in the buffer cache by opening a file, mapping it with mmap(2), and then applying mincore(2) to the mapping."
[^mincore-def]: mincore(2), DESCRIPTION (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/mincore.2.en.html> — "mincore() returns a vector that indicates whether pages of the calling process's virtual memory are resident in core (RAM), and so will not cause a disk access (page fault) if referenced."
[^fadv-nb]: posix_fadvise(2), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/posix_fadvise.2.en.html> — "The advice is not binding; it merely constitutes an expectation on behalf of the application."
[^pcg]: Linux kernel docs, Page Cache — <https://docs.kernel.org/mm/page_cache.html> — "It can be bypassed (e.g. with O_DIRECT), but normal reads, writes and mmaps go through the page cache."
[^fadv-dontneed-def]: posix_fadvise(2), DESCRIPTION (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/posix_fadvise.2.en.html> — "POSIX_FADV_DONTNEED attempts to free cached pages associated with the specified region."
[^fadv-dontneed]: posix_fadvise(2), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/posix_fadvise.2.en.html> — "Any unwritten dirty pages will not be freed."
[^vfs-ra]: Linux kernel docs, Overview of the Linux Virtual File System, address_space_operations — <https://docs.kernel.org/filesystems/vfs.html> — "Called by the VM to read pages associated with the address_space object."
[^fadv-willneed]: posix_fadvise(2), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/posix_fadvise.2.en.html> — "POSIX_FADV_WILLNEED initiates a nonblocking read of the specified region into the page cache."
[^fadv-linux]: posix_fadvise(2), NOTES — <https://manpages.debian.org/bookworm/manpages-dev/posix_fadvise.2.en.html> — "POSIX_FADV_NORMAL sets the readahead window to the default size for the backing device; POSIX_FADV_SEQUENTIAL doubles this size, and POSIX_FADV_RANDOM disables file readahead entirely."
[^fadv-ret]: posix_fadvise(2), RETURN VALUE (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/posix_fadvise.2.en.html> — "On success, zero is returned. On error, an error number is returned."
[^proc-cached]: proc(5), /proc/meminfo — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "In-memory cache for files read from the disk (the page cache). Doesn't include SwapCached."
[^open-einval]: open(2), NOTES — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "Some filesystems may not implement the flag, in which case open() fails with the error EINVAL if it is used."
[^open-directbuf]: open(2), O_DIRECT — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "File I/O is done directly to/from user-space buffers."
[^open-direct]: open(2), O_DIRECT — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "Try to minimize cache effects of the I/O to and from this file."
[^open-align]: open(2), NOTES — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "The O_DIRECT flag may impose alignment restrictions on the length and address of user-space buffers and the file offset of I/Os."
[^open-align-hist]: open(2), NOTES — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "In Linux 2.6.0, this was relaxed to the logical block size of the block device (typically 512 bytes)."
[^open-direct-fallback]: open(2), NOTES (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "The handling of misaligned O_DIRECT I/Os also varies; they can either fail with EINVAL or fall back to buffered I/O."
[^open-statx]: open(2), NOTES — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "Since Linux 6.1, O_DIRECT support and alignment restrictions for a file can be queried using statx(2), using the STATX_DIOALIGN flag."
[^open-direct-sync]: open(2), O_DIRECT — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "The O_DIRECT flag on its own makes an effort to transfer data synchronously, but does not give the guarantees of the O_SYNC flag that data and necessary metadata are transferred."
[^open-mix]: open(2), NOTES — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "Applications should avoid mixing O_DIRECT and normal I/O to the same file, and especially to overlapping byte regions in the same file."
[^open-default]: open(2), NOTES — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "It is recommended that applications treat use of O_DIRECT as a performance option which is disabled by default."
[^inode-meta]: inode(7), Debian bookworm — <https://manpages.debian.org/bookworm/manpages/inode.7.en.html> — "Each file has an inode containing metadata about the file."
[^ostep-inum]: OSTEP, Chương 39 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-intro.pdf> — "the low-level name of a file is often referred to as its inode number (i-number)"
[^inode-nlink]: inode(7), Debian bookworm — <https://manpages.debian.org/bookworm/manpages/inode.7.en.html> — "This field contains the number of hard links to the file."
[^rust-blocks]: Rust 1.95.0, std::os::unix::fs::MetadataExt::blocks — <https://doc.rust-lang.org/1.95.0/std/os/unix/fs/trait.MetadataExt.html> — "Returns the number of blocks allocated to the file, in 512-byte units."
[^link-def]: link(2), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/link.2.en.html> — "both names refer to the same file (and so have the same permissions and ownership) and it is impossible to tell which name was the "original"."
[^inode-unique]: inode(7), Debian bookworm — <https://manpages.debian.org/bookworm/manpages/inode.7.en.html> — "Inode numbers are guaranteed to be unique only within a filesystem"
[^rust-stat]: Rust 1.95.0, fs::metadata — <https://doc.rust-lang.org/1.95.0/std/fs/fn.metadata.html> — "This function currently corresponds to the stat function on Unix"
[^rust-metadata]: Rust 1.95.0, fs::symlink_metadata — <https://doc.rust-lang.org/1.95.0/std/fs/fn.symlink_metadata.html> — "This function currently corresponds to the lstat function on Unix"
[^lstat]: stat(2), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/stat.2.en.html> — "lstat() is identical to stat(), except that if pathname is a symbolic link, then it returns information about the link itself, not the file that the link refers to."
[^inode-size]: inode(7), Debian bookworm — <https://manpages.debian.org/bookworm/manpages/inode.7.en.html> — "The size of a symbolic link is the length of the pathname it contains, without a terminating null byte."
[^rename-def]: rename(2), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/rename.2.en.html> — "Any other hard links to the file (as created using link(2)) are unaffected. Open file descriptors for oldpath are also unaffected."
[^ostep-unlink]: OSTEP, Chương 39, mục 39.14 Hard Links — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-intro.pdf> — "only when the reference count reaches zero does the file system also free the inode and related data blocks"
[^stat-enoent]: stat(2), ERRORS (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/stat.2.en.html> — "A component of pathname does not exist or is a dangling symbolic link."
[^unlink-open]: unlink(2), Debian bookworm — <https://manpages.debian.org/bookworm/manpages-dev/unlink.2.en.html> — "the file will remain in existence until the last file descriptor referring to it is closed."
[^link-eperm]: link(2), mục ERRORS — <https://manpages.debian.org/bookworm/manpages-dev/link.2.en.html> — "EPERM oldpath is a directory."
[^symlink-hard]: symlink(7), Debian bookworm — <https://manpages.debian.org/bookworm/manpages/symlink.7.en.html> — "Hard links may not refer to directories (to prevent the possibility of loops within the filesystem tree, which would confuse many programs) and may not refer to files on different filesystems (because inode numbers are not unique across filesystems)."
[^link-exdev]: link(2), mục ERRORS — <https://manpages.debian.org/bookworm/manpages-dev/link.2.en.html> — "link() does not work across different mounts, even if the same filesystem is mounted on both."
[^symlink-def]: symlink(7), Debian bookworm — <https://manpages.debian.org/bookworm/manpages/symlink.7.en.html> — "a symbolic link is a pointer to another name, and not to an underlying object. For this reason, symbolic links may refer to directories and may cross filesystem boundaries."
[^ostep-dir]: OSTEP, Chương 39 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-intro.pdf> — "it contains a list of (user-readable name, low-level name) pairs"
[^rs-readdir-platform]: Rust std, fs::read_dir — <https://doc.rust-lang.org/1.95.0/std/fs/fn.read_dir.html> — "The order in which this iterator returns entries is platform and filesystem dependent."
[^rs-readdir-order]: Rust std, fs::read_dir — <https://doc.rust-lang.org/1.95.0/std/fs/fn.read_dir.html> — "The order in which read_dir returns entries can change between calls. If reproducible ordering is required, the entries should be explicitly sorted."
[^readdir-dots]: Rust std 1.95.0, fs::read_dir — <https://doc.rust-lang.org/1.95.0/std/fs/fn.read_dir.html> — "Entries for the current and parent directories (typically . and ..) are skipped."
[^dirent-ino]: Rust std 1.95.0, std::os::unix::fs::DirEntryExt::ino — <https://doc.rust-lang.org/1.95.0/std/os/unix/fs/trait.DirEntryExt.html> — "Returns the underlying d_ino field in the contained dirent structure."
[^ost-dir]: OSTEP, File System Implementation, §40.4 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-implementation.pdf> — "a directory basically just contains a list of (entry name, inode number) pairs."
[^dirent-meta]: Rust std 1.95.0, fs::DirEntry::metadata — <https://doc.rust-lang.org/1.95.0/std/fs/struct.DirEntry.html> — "On Windows this function is cheap to call (no extra system calls needed), but on Unix platforms this function is the equivalent of calling symlink_metadata on the path."
[^dirent-ftype]: Rust std 1.95.0, fs::DirEntry::file_type — <https://doc.rust-lang.org/1.95.0/std/fs/struct.DirEntry.html> — "On Windows and most Unix platforms this function is free (no extra system calls needed), but some Unix platforms may require the equivalent call to symlink_metadata to learn about the target file type."
[^e4-ftype]: Linux kernel docs, ext4 Index Nodes — <https://docs.kernel.org/filesystems/ext4/inodes.html> — "ext4 appears to cheat (for performance reasons) a little bit by storing a copy of the file type (normally stored in the inode) in the directory entry."
[^readdir-map]: Rust std 1.95.0, fs::read_dir (Platform-specific behavior) — <https://doc.rust-lang.org/1.95.0/std/fs/fn.read_dir.html> — "Advancing the iterator currently corresponds to readdir on Unix and FindNextFile on Windows."
[^dirent-nosym]: Rust std 1.95.0, fs::DirEntry::file_type — <https://doc.rust-lang.org/1.95.0/std/fs/struct.DirEntry.html> — "This function will not traverse symlinks if this entry points at a symlink."
[^proc-dirty]: proc(5), /proc/meminfo — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "Memory which is waiting to get written back to the disk."
[^proc-wb]: proc(5), /proc/meminfo — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "Memory which is actively being written back to the disk."
[^vm-bg]: Linux v6.8 Documentation/admin-guide/sysctl/vm.rst, dirty_background_ratio — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/sysctl/vm.rst> — "the number of pages at which the background kernel flusher threads will start writing out dirty data."
[^vm-dr]: Linux v6.8 Documentation/admin-guide/sysctl/vm.rst, dirty_ratio — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/sysctl/vm.rst> — "the number of pages at which a process which is generating disk writes will itself start writing out dirty data."
[^vm-notequal]: Linux v6.8 Documentation/admin-guide/sysctl/vm.rst, dirty_ratio — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/sysctl/vm.rst> — "The total available memory is not equal to total system memory."
[^vm-bytes]: Linux v6.8 Documentation/admin-guide/sysctl/vm.rst, dirty_bytes — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/sysctl/vm.rst> — "Only one of them may be specified at a time."
[^vm-exp]: Linux v6.8 Documentation/admin-guide/sysctl/vm.rst, dirty_expire_centisecs — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/sysctl/vm.rst> — "Data which has been dirty in-memory for longer than this interval will be written out next time a flusher thread wakes up."
[^vm-wbc2]: Linux kernel docs, Documentation for /proc/sys/vm/, dirty_writeback_centisecs — <https://docs.kernel.org/admin-guide/sysctl/vm.html> — "The kernel flusher threads will periodically wake up and write old data out to disk."
[^vm-wbc]: Linux v6.8 Documentation/admin-guide/sysctl/vm.rst, dirty_writeback_centisecs — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/sysctl/vm.rst> — "Setting this to zero disables periodic writeback altogether."
[^osi-5-30]: OSTEP, File System Implementation, mục 40.7 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-implementation.pdf> — "buffer writes in memory for anywhere between five and thirty seconds"
[^ostep-fsync]: OSTEP, Chương 39, mục 39.7 — <https://pages.cs.wisc.edu/~remzi/OSTEP/file-intro.pdf> — "the file system responds by forcing all dirty (i.e., not yet written) data to disk, for the file referred to by the specified file descriptor."
