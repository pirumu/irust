# P5 · Chương 08 — Rust và lưu trữ

Các chương trước của phase này đi từ thiết bị, file system, page cache đến crash consistency và durability. Chương này đứng ở phía Rust: `std::fs`, `std::io`, `std::os::unix::fs` và hai crate hay gặp (`memmap2`, `tokio::fs`) chuyển thành những system call nào, hứa gì với bạn, và bẫy nằm ở đâu. Mọi ví dụ đều đã chạy thật bằng rustc 1.95.0 trong container Linux (aarch64, kernel của Lima/Colima), và syscall được xác nhận bằng `strace`.

Xuyên suốt chương ta bám theo `ledger`, một service Rust ghi sổ thanh toán (nhận xét của người soạn: ví dụ giả định, không phải code trong repo). Mỗi giao dịch `ledger` nối một dòng vào `journal.log`; định kỳ nó ghi `state.json` làm checkpoint; cuối ngày nó xuất báo cáo CSV; nó giữ một file khóa để chỉ một instance chạy trên máy; khi khởi động nó đọc lại các file segment và tra một file index lớn; còn handler async của nó đọc file cấu hình qua `tokio::fs`. Mỗi mục dưới đây lấy một việc trong số đó và hỏi: Rust gọi kernel thế nào, và `ledger` được bảo đảm gì.

## Bạn sẽ hiểu được gì

1. **Q1.** `OpenOptions` với `append`, `create`, `create_new`, `truncate` biến thành cờ `open(2)` nào, và vì sao `create_new` là cách tạo file "nếu và chỉ nếu chưa tồn tại" không có race? → mục 1
2. **Q2.** `write` và `write_all` khác nhau thế nào khi gặp partial write; `BufWriter` có buffer bao nhiêu, vì sao `drop` nuốt lỗi flush và vì sao phải gọi `flush()` tường minh? → mục 2
3. **Q3.** `sync_all` và `sync_data` làm gì, và `File::drop`/`flush` có giúp bạn bền dữ liệu không? → mục 3
4. **Q4.** Mẫu "ghi file tạm → `sync_all` → `rename`" viết bằng Rust ra sao, và vì sao `fs::write` không đủ? → mục 4
5. **Q5.** `read_at`/`write_at` (pread/pwrite) khác `seek` + `read` chỗ nào, và bẫy `O_APPEND` là gì? → mục 5
6. **Q6.** `File::lock` có stable ở 1.95.0 không, nó là loại khóa gì? → mục 7
7. **Q7.** Vì sao `Mmap::map` là `unsafe`? → mục 8
8. **Q8.** `tokio::fs` có thật sự async không? → mục 9

(Mục phụ trợ: thứ tự `read_dir`, metadata và quyền Unix → mục 6.)

## Kiến thức cần trước

- [File descriptor, OFD, inode và VFS](../01-linux/04-file-descriptor-va-vfs.md) — fd, open file description, offset dùng chung.
- [Rust trên giao diện hệ thống](../01-linux/07-rust-tren-giao-dien-he-thong.md) — `OwnedFd`, `io::Error` và `errno`, `Drop` đóng fd.
- [Durability trong thực tế](./06-durability-trong-thuc-te.md) — `fsync`, thư mục cha, mẫu ghi an toàn (chương này chỉ làm phần API Rust).
- [Bộ nhớ tiến trình Linux](../03-memory/05-bo-nho-tien-trinh-linux.md) — `mmap`, page fault.

## 1. `OpenOptions`: cờ nào đi xuống kernel (Q1)

> **Trả lời ngắn.** Mỗi phương thức của `OpenOptions` bật một cờ của `open(2)`: `append` → `O_APPEND`, `truncate` → `O_TRUNC`, `create_new` → `O_CREAT|O_EXCL` (đây là quan sát bằng `strace` trên Linux; docs Rust chỉ hứa hành vi, không hứa tên cờ). `create_new` là cách tạo file "chỉ khi chưa tồn tại" không có race, vì nó nguyên tử còn "kiểm tra tồn tại rồi tạo" thì có TOCTOU race[^rs-createnew-atomic]. `append` bảo đảm mỗi lần ghi nằm ở cuối file kể cả khi nhiều process cùng ghi[^rs-append].

**Giải thích.**

*Với `ledger`.* Ngay lúc khởi động `ledger` đã phải mở ba file, mỗi file với một ý định khác nhau: `journal.log` để nối thêm, `state.json` để ghi đè từ đầu, file khóa chỉ được tạo nếu chưa ai tạo. Ý định nào cũng phải nói rõ trước khi kernel đưa fd: chỉ đọc hay có ghi, chưa có thì tạo không, có rồi thì xóa nội dung cũ không. `OpenOptions` là chỗ khai báo những lựa chọn đó, và mỗi lựa chọn trở thành một cờ của `open(2)`. Câu hỏi của mục này: cờ nào, và với từng cờ `ledger` được hứa gì?

`File::open` chỉ mở để đọc, `File::create` tạo hoặc cắt file để ghi. Mọi tổ hợp khác (nối đuôi, tạo độc quyền, đọc-ghi không cắt) cần `OpenOptions`. Docs Rust chỉ mô tả hành vi, nên muốn biết cờ thật ta phải chạy và đọc `strace`.

### 1.1 Bảng cờ quan sát được

Một chương trình Rust mở file theo bảy cách khác nhau; `strace -e trace=openat` cho:

| Mã Rust | `openat` flags quan sát được (Linux, std 1.95.0) |
|---|---|
| `File::open(p)` | `O_RDONLY\|O_CLOEXEC` |
| `File::create(p)` | `O_WRONLY\|O_CREAT\|O_TRUNC\|O_CLOEXEC`, mode `0666` |
| `.append(true).create(true)` | `O_WRONLY\|O_CREAT\|O_APPEND\|O_CLOEXEC` |
| `.write(true).create_new(true)` | `O_WRONLY\|O_CREAT\|O_EXCL\|O_CLOEXEC` |
| `.write(true).truncate(true)` | `O_WRONLY\|O_TRUNC\|O_CLOEXEC` |
| `.read(true).write(true)` | `O_RDWR\|O_CLOEXEC` |
| `.write(true).create(true)` | `O_WRONLY\|O_CREAT\|O_CLOEXEC` |

Đây là **quan sát** một lần chạy trên Linux; docs Rust không hứa tên cờ, chỉ hứa hành vi. `O_CLOEXEC` lúc nào cũng có: std tự đặt close-on-exec để fd không rò sang process con (xem chương [Rust trên giao diện hệ thống](../01-linux/07-rust-tren-giao-dien-he-thong.md)). Mode `0666` là mode xin kernel; kernel còn áp `umask` lên đó.

### 1.2 `append`: journal của `ledger`

`journal.log` mở bằng `.append(true).create(true)`, tức `O_APPEND`. Docs Rust hứa mỗi lần ghi nằm ở cuối file, kể cả khi nhiều process hoặc thread cùng nối vào một file[^rs-append]. Man page nói cùng ý ở tầng kernel: dịch offset và ghi diễn ra trong một bước nguyên tử[^man-oappend]. Ghi bằng `seek(End(0))` rồi `write()` thì là hai bước, nên giữa hai bước đó một writer khác có thể chen vào (suy luận của người soạn).

Nhưng "nguyên tử về vị trí" chưa phải "nguyên tử về nội dung". Một lần `write()` thành công vẫn được phép chỉ ghi một phần dữ liệu, cho nên dù `ledger` đưa trọn một dòng journal vào một lần gọi, docs Rust vẫn không bảo đảm dòng đó được ghi đủ[^rs-append-interleave]. Hệ quả (suy luận của người soạn): hai writer cùng nối vào một journal vẫn có thể làm dòng của nhau bị cắt xen.

### 1.3 `truncate`: cái giá của ghi đè tại chỗ

`state.json` mà `ledger` ghi bằng `File::create` mang cờ `O_TRUNC`: nếu file đã có thì bị cắt về độ dài 0[^rs-trunc]. Man page nói rõ điều kiện: file là regular file và được mở với quyền ghi[^man-otrunc]. Còn `File::create` nói gọn là `write + create + truncate`, đúng như bảng ở trên.

Hệ quả vận hành (nhận xét của người soạn): ngay khi `open` thành công, nội dung checkpoint cũ đã mất. Nếu `ledger` chết trước khi ghi xong, trên đĩa chỉ còn file rỗng hoặc dở dang, và đây là lý do chương [Durability](./06-durability-trong-thuc-te.md) dùng file tạm cộng `rename` thay vì ghi đè tại chỗ.

### 1.4 `create_new`: file khóa và file tạm

`create_new(true)` đi xuống thành `O_CREAT|O_EXCL`. Hai thứ nó cần: không được có file nào ở vị trí đó (kể cả symlink treo), và nếu lệnh thành công thì file trả về chắc chắn là file mới[^rs-createnew]. Lợi ích chính là tính nguyên tử: nếu bạn kiểm tra file có tồn tại rồi mới tạo, giữa hai bước đó process khác có thể tạo mất, và đó là TOCTOU race[^rs-createnew-atomic]. Khi `create_new` bật, `create` và `truncate` bị bỏ qua[^rs-createnew-ignored].

Với file khóa của `ledger`, điều đó có nghĩa là "tạo được là mình giữ". Nếu file đã tồn tại, kernel trả `EEXIST`[^man-oexcl] và Rust đổi nó thành `ErrorKind::AlreadyExists`[^rs-errors]. Chương trình thí nghiệm xác nhận: lần mở thứ hai trả `openat(...) = -1 EEXIST` và `AlreadyExists File exists`. Cùng cơ chế này cho ta tên file tạm độc nhất ở mục 4.

Một hiểu lầm hay gặp: `.create(true)` mà không kèm `.write(true)` hoặc `.append(true)` không tạo ra file rỗng, mà `open` báo lỗi `InvalidInput`[^rs-create-needs-write].

<details><summary>Đi sâu: <code>Drop</code> của <code>File</code>, lỗi khi đóng và <code>O_APPEND</code> trên NFS</summary>

File tự đóng khi ra khỏi scope, và `Drop` bỏ qua lỗi lúc đóng; nếu cần xử lý lỗi đó thì gọi `sync_all`[^rs-file-drop]. Điểm này quay lại ở mục 3.

Nếu `journal.log` nằm trên NFS thì phải thận trọng hơn: man page cảnh báo `O_APPEND` có thể làm hỏng file khi nhiều process cùng nối đuôi[^man-nfs-append].

</details>

**Quay lại Q1:** mỗi phương thức builder bật một cờ `open(2)`; muốn "tạo nếu chưa có" không race, như với file khóa của `ledger`, thì dùng `create_new` (`O_CREAT|O_EXCL`).

## 2. `write`, `write_all` và `BufWriter` (Q2)

> **Trả lời ngắn.** `write` được phép ghi ít byte hơn bạn đưa mà không bị coi là lỗi[^rs-write-partial]; `write_all` lặp lại đến hết dữ liệu hoặc gặp lỗi thật[^rs-write-all]. `BufWriter` gom nhiều lần ghi nhỏ vào một buffer[^rs-bufwriter-when], mặc định hiện là 8 KiB[^rs-bufwriter-8k]. Lỗi xảy ra khi nó bị drop bị bỏ qua, nên phải gọi `flush()` và kiểm tra `Result`[^rs-bufwriter-flush].

**Giải thích.**

*Với `ledger`.* Cuối ngày `ledger` xuất báo cáo CSV: hàng chục nghìn dòng ngắn, ghi lần lượt vào một file. Có hai câu hỏi. Một là mỗi lần gọi ghi có chắc ghi hết dòng không? Hai là gọi `File::write` hàng chục nghìn lần thì có đắt không, và nếu đĩa đầy giữa chừng thì `ledger` có biết không?

### 2.1 `write` có thể ghi ít hơn bạn yêu cầu

`Write::write` trả `Ok(n)` với `n` là số byte đã ghi. Ghi chưa hết buffer không bị coi là lỗi[^rs-write-partial]. Man page của `write(2)` kể các nguyên nhân: hết chỗ trên thiết bị, chạm `RLIMIT_FSIZE`, hoặc bị signal handler ngắt sau khi đã ghi một phần; khi đó người gọi nên gọi `write()` lại cho phần còn lại[^man-write-partial].

`write_all` chính là vòng lặp làm việc đó: nó gọi `write` liên tục đến khi hết dữ liệu hoặc gặp lỗi không phải `Interrupted`[^rs-write-all]. Thí nghiệm với một writer giả cố tình chỉ nhận 3 byte mỗi lần:

```rust
struct Short(Vec<u8>);
impl Write for Short {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        let n = b.len().min(3);
        self.0.extend_from_slice(&b[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}
// s.write(b"abcdefgh")  -> Ok(3)
// s.write_all(b"abcdefgh") -> Ok(()), s.0.len() == 11 (3 + 8)
```

Chạy thật: `write -> 3`, rồi sau `write_all` tổng độ dài là 11. Quy tắc (nhận xét của người soạn): luôn dùng `write_all` trừ khi bạn thật sự muốn điều khiển từng lần ghi, chẳng hạn với non-blocking I/O. Bỏ qua giá trị trả về của `write` là một bug im lặng.

### 2.2 `BufWriter` gom ghi nhỏ

Mỗi lần gọi `File::write` là một syscall, nên ghi từng dòng CSV một sẽ tốn rất nhiều syscall. `BufWriter` giữ một buffer trong bộ nhớ và chỉ ghi xuống theo mẻ lớn. Docs nói nó có ích khi có nhiều lần ghi nhỏ, và không giúp gì khi ghi một lượng rất lớn một lần hoặc chỉ ghi vài lần[^rs-bufwriter-when]. Dung lượng mặc định hiện là 8 KiB và có thể đổi trong tương lai[^rs-bufwriter-8k].

```mermaid
flowchart LR
  A[write_all 17 byte] --> B{BufWriter<br/>buffer 8 KiB}
  B -- chưa đầy --> B
  B -- đầy hoặc flush --> C[write syscall]
  C --> D[(page cache)]
```

### 2.3 `BufWriter::drop` nuốt lỗi, nên phải `flush()`

Cái giá của việc gom là dữ liệu nằm trong buffer chưa tới kernel, và lúc `BufWriter` bị drop thì nó cố xả buffer nốt. Nhưng `Drop` không có giá trị trả về nên không báo lỗi được, vì vậy docs dặn: gọi `flush` trước khi drop, vì mọi lỗi xảy ra lúc drop đều bị bỏ qua[^rs-bufwriter-flush].

Thí nghiệm dùng `/dev/full` (thiết bị luôn trả `ENOSPC` khi ghi):

```rust
let f = OpenOptions::new().write(true).open("/dev/full")?;
{
    let mut w = BufWriter::new(f);
    w.write_all(b"data")?;          // Ok: chỉ vào buffer
}                                   // drop: write -> ENOSPC, im lặng
// flush() tường minh: Err("No space left on device (os error 28)")
// into_inner() cũng trả Err(IntoInnerError) mang lỗi ENOSPC
```

Kết quả thật: `write_all` thành công, sau khi drop không có dấu hiệu lỗi nào ở mức chương trình, trong khi `strace` cho thấy `write(3, "data", 4) = -1 ENOSPC` đã xảy ra bên trong drop. Còn `flush()` và `into_inner()` đều trả `Err` với "No space left on device (os error 28)". Gọi `flush` xong thì buffer rỗng, nên drop sau đó không thử thao tác file nào nữa[^rs-bufwriter-flush].

Với `ledger` (nhận xét của người soạn), đây là phiên bản thu nhỏ của lỗi quen thuộc: báo cáo cuối ngày bị cụt vì đĩa đầy mà job vẫn báo thành công. Mẫu đúng cho code cần tin kết quả:

```rust
let mut w = BufWriter::new(file);
// ... write_all ...
w.flush()?;                       // lỗi buffer lớp Rust
w.get_ref().sync_all()?;          // lỗi từ kernel/thiết bị (mục 3)
```

<details><summary>Đi sâu: số đo syscall của <code>BufWriter</code> và <code>BufReader</code></summary>

Số đo (quan sát một lần chạy, 100.000 lần `write_all` 17 byte = 1.700.000 byte, `strace -c`):

| Cách | Số syscall `write` |
|---|---|
| `File` trực tiếp | 100.001 |
| `BufWriter::new(file)` | 209 |

Chạy lại thí nghiệm: các lần ghi xuống kernel đều là `write(5, ..., 8177)` — không phải đúng 8192. Lý do (quan sát): mỗi bản ghi dài 17 byte nên buffer 8 KiB chứa tối đa 481 bản ghi = 8177 byte, và `BufWriter` xả khi bản ghi kế tiếp không còn vừa. Vậy 1.700.000 byte thành 207 lần ghi 8177 byte cộng một lần ghi 7361 byte (lúc `flush`) = 208, cộng một `write` in ra stdout = 209. Với thí nghiệm khác, ghi 20.000 byte một lần vào `BufWriter` cho đúng một `write(3, ..., 20000)` ngay lập tức: lượng dữ liệu lớn hơn buffer đi thẳng xuống, buffer không giữ lại (quan sát; docs không hứa chi tiết này).

`BufReader` đối xứng: mặc định 8 KiB[^rs-bufreader-8k], và đọc 3 byte đơn lẻ chỉ gây một `read(3, ..., 8192)` đầu tiên (quan sát). Cạm bẫy: tạo nhiều `BufReader` trên cùng một stream có thể làm mất dữ liệu, vì phần đã đọc trước nằm trong buffer của reader bị bỏ đi[^rs-bufreader-loss].

</details>

**Quay lại Q2:** `write` có thể ghi thiếu nên dùng `write_all`; `BufWriter` gom ghi nhỏ nhưng drop nuốt lỗi, vì vậy khi xuất báo cáo `ledger` phải `flush()` tường minh và kiểm tra kết quả.

## 3. `sync_all` và `sync_data` (Q3)

> **Trả lời ngắn.** `sync_all` cố bảo đảm mọi dữ liệu trong bộ nhớ của OS đến được file system[^rs-sync-all-attempt]; `sync_data` tương tự nhưng có thể không đồng bộ metadata[^rs-sync-data]. `File::drop` không thay được chúng. Drop không bắt buộc phải chờ dữ liệu xuống đĩa[^rs-sync-all], và lỗi lúc đóng bị bỏ qua[^rs-file-drop].

**Giải thích.**

*Với `ledger`.* `ledger` vừa ghi xong một giao dịch, vừa trả "đã nhận" cho client thì máy mất điện. Khi khởi động lại, giao dịch đó còn trong journal không? Câu trả lời phụ thuộc dữ liệu đã đi tới đâu trước lúc mất điện, và mục này vạch ra đường đi đó.

`flush()` của `BufWriter` chỉ đẩy dữ liệu từ buffer user-space xuống kernel, cụ thể là page cache. Từ page cache xuống thiết bị lưu trữ còn một chặng nữa, và chặng đó là việc của `sync_all`. Docs `File::sync_all` hứa sẽ cố bảo đảm mọi dữ liệu trong bộ nhớ của OS đến được file system trước khi trả về[^rs-sync-all-attempt]. Docs cũng lưu ý nó thường đắt hơn việc đóng file bằng drop[^rs-sync-all], và đó là lý do drop rẻ hơn: drop không phải chờ.

`sync_data` làm gần như cùng việc nhưng có thể bỏ qua metadata[^rs-sync-data]. Docs cảnh báo thêm: một số nền tảng cài `sync_data` bằng chính `sync_all`[^rs-sync-data-platform].

Trên Linux ta quan sát đúng như vậy: `sync_all()` → `fsync(3)`, `sync_data()` → `fdatasync(3)` (strace). Hợp đồng của kernel cho hai lời gọi này (xả cache của đĩa, metadata nào bị bỏ qua, việc `fsync` file không bảo đảm entry thư mục) và phân tích sâu (errseq, fsyncgate, chi phí thực) nằm ở [chương 06, mục 2, 3 và 5](./06-durability-trong-thuc-te.md).

<details><summary>Đi sâu: <code>File::flush</code> là no-op và không thay được <code>fsync</code></summary>

Một cạm bẫy riêng của Rust: `File` **không có buffer riêng** (mục 2: 100.000 lần `write_all` cho 100.001 `write`), nên với `File` trần không có buffer user-space nào để `flush`. Docs ghi thẳng điều này: vì `File` không chứa buffer nào, `flush` hiện là no-op trên Unix và Windows[^rs-file-flush]. Source std 1.95.0 cho thấy phần cài đặt Unix chỉ là `Ok(())`[^src-file-flush]. Cho nên `flush()` của `File` không thay được `fsync`.

Docs của `tokio::fs::File` nhắc điều tương tự cho kiểu async: flush không bảo đảm file đã ghi hết xuống đĩa vì hệ điều hành có thể giữ thay đổi trong buffer bộ nhớ[^tokio-file-flush].

</details>

**Quay lại Q3:** `flush` chỉ tới kernel; muốn giao dịch của `ledger` sống sót qua mất điện thì phải `sync_all`/`sync_data`, còn drop của `File` không làm việc đó.

## 4. Ghi nguyên tử: `fs::write`, `rename` và file tạm (Q4)

> **Trả lời ngắn.** Mẫu đúng: ghi vào một file tạm mới (`create_new`), `sync_all`, `rename` đè lên file đích, rồi `sync_all` thư mục cha. Bước cuối cần vì `fsync` file chưa chắc đã đưa entry thư mục chứa nó xuống đĩa[^man-fsync-dir]. `rename` thay đích một cách nguyên tử, không có thời điểm nào process khác thấy đích biến mất[^man-rename]. Còn `fs::write` thay hoàn toàn nội dung tại chỗ[^rs-fs-write] và (quan sát bằng `strace`) không có `fsync`, nên không nguyên tử và không bền (suy luận của người soạn).

**Giải thích.**

*Với `ledger`.* Checkpoint `state.json` là file mà mất nửa chừng là thảm họa: khởi động lại mà đọc phải file cụt thì `ledger` không biết số dư. Ở mục 1 ta đã thấy ghi đè tại chỗ làm file rỗng ngay khi `open`. Mục này đưa ra cách ghi mà người đọc, kể cả `ledger` sau khi crash, chỉ thấy hoặc bản cũ nguyên vẹn hoặc bản mới nguyên vẹn.

### 4.1 Hai hàm tiện ích và vì sao chúng chưa đủ

Hai hàm tiện ích cho trường hợp "cả file một lần":

- `fs::write(path, bytes)` tạo file nếu chưa có và **thay hoàn toàn** nội dung nếu có[^rs-fs-write]. Trên Linux ta thấy `openat(..., O_WRONLY|O_CREAT|O_TRUNC|O_CLOEXEC, 0666)`, `write(3, "v1", 2)`, `close(3)` — không `fsync`.
- `fs::read_to_string(path)` mở file, đọc hết, lỗi nếu không phải UTF-8, và tự retry khi gặp `Interrupted`[^rs-fs-rts]. Quan sát: nó gọi `statx` trên fd để lấy kích thước làm gợi ý cấp phát, rồi `read(3, ..., 2)` và một `read` nữa trả 0 để phát hiện EOF.

Hệ quả: `fs::write` đi đường **không bền** và **không nguyên tử** (cắt rồi mới ghi). Nó hợp với file dễ tái tạo; nó không hợp với config hay state như `state.json` (nhận xét của người soạn).

### 4.2 `rename` và mẫu ghi nguyên tử

Chỗ dựa của mẫu này là `fs::rename`: nó đổi tên và thay thế đích nếu đích đã có, nhưng không chạy qua ranh giới mount point[^rs-rename]. Ở tầng kernel, man page `rename(2)` nói nếu `newpath` đã tồn tại thì nó bị thay thế nguyên tử, không có lúc nào process khác thấy `newpath` biến mất[^man-rename]. Khác mount point thì kernel trả `EXDEV`[^man-exdev], và docs Rust liệt kê "from và to ở hai file system khác nhau" trong các tình huống lỗi[^rs-rename-err]. Vì vậy file tạm phải nằm cùng thư mục, tức cùng file system, với file đích (suy luận của người soạn).

Ghép với mục 3 ta được mẫu đã phân tích ở chương 06 (lý do từng bước ở đó). Dưới đây là bản Rust, đã chạy thật:

```rust
fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap();
    let tmp = dir.join(".config.tmp");
    let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    f.write_all(data)?;
    f.sync_all()?;               // dữ liệu file tạm bền trước khi đổi tên
    drop(f);
    fs::rename(&tmp, path)?;     // thay thế nguyên tử
    File::open(dir)?.sync_all()?; // làm bền entry thư mục
    Ok(())
}
```

<details><summary>Đi sâu: thứ tự syscall quan sát được và tên file tạm</summary>

`strace` xác nhận đúng thứ tự: `openat(... .config.tmp, O_WRONLY|O_CREAT|O_EXCL ...)` → `write(3, "version-2", 9)` → `fsync(3)` → `close(3)` → `renameat(...)` → `openat(dir, O_RDONLY)` → `fsync(3)`. Hai điểm Rust cần lưu ý. Thứ nhất, `create_new` cho tên tạm độc quyền: nếu hai tiến trình dùng chung tên tạm cố định như ví dụ này thì cái thứ hai nhận `AlreadyExists`, tốt hơn là âm thầm giẫm lên nhau; thực tế nên đặt tên tạm có PID hoặc số ngẫu nhiên (nhận xét của người soạn). Thứ hai, mở thư mục bằng `File::open(dir)` rồi `sync_all` là cách `std` cho phép `fsync` một thư mục trên Unix (quan sát: `openat(dir, O_RDONLY)` rồi `fsync`).

</details>

**Quay lại Q4:** ghi file tạm → `sync_all` → `rename` → sync thư mục; `fs::write` thì không nguyên tử, không bền, nên không dùng cho `state.json`.

## 5. Seek, `pread` và `pwrite` (Q5)

> **Trả lời ngắn.** `read_at`/`write_at` đọc/ghi tại offset chỉ định, độc lập với con trỏ file và không làm đổi nó[^rs-readat]; `seek` rồi `read` thì dùng một con trỏ chung nên là hai bước có trạng thái chia sẻ (suy luận của người soạn từ mô hình OFD). Bẫy: với file mở bằng `append(true)`, trên Linux `pwrite` vẫn nối vào cuối bất kể offset[^man-pwrite-bug].

**Giải thích.**

*Với `ledger`.* Khi khởi động `ledger` tra một file index gồm các record nằm ở những offset biết trước, và nhiều handler thread cùng tra trên một `File` dùng chung. Nếu mỗi thread `seek` tới record của mình rồi `read`, chúng sẽ giẫm lên vị trí của nhau. Mục này giải thích vì sao, và `read_at` tránh điều đó thế nào. Nó cũng nói tới bẫy khi ta thử dùng `write_at` để vá một record trong `journal.log` vốn mở bằng `append`.

### 5.1 Con trỏ vị trí dùng chung

`File` implement `Seek`, tức bên trong file có một con trỏ logic mà `seek` dịch chuyển[^rs-file-seek]. Con trỏ đó nằm trong open file description của kernel và dùng chung giữa các fd nhân bản bằng `dup` hoặc `fork` (xem [chương 04 P1](../01-linux/04-file-descriptor-va-vfs.md)). Do đó `seek` rồi `read` là hai bước có trạng thái chia sẻ: hai thread dùng chung một `File` qua `&File` có thể giẫm vị trí của nhau (suy luận của người soạn từ mô hình OFD).

### 5.2 `FileExt::read_at` và `write_at`

Trait `std::os::unix::fs::FileExt` cho đọc/ghi tại offset chỉ định mà **không đụng con trỏ file**: offset tính từ đầu file, độc lập với cursor, và cursor không bị ảnh hưởng[^rs-readat]. Chữ ký của chúng nhận `&self`[^rs-fileext-self], nên các thread của `ledger` có thể gọi qua tham chiếu chung mà không cần khóa riêng cho con trỏ (suy luận của người soạn). Trên Linux chúng thành `pread64`/`pwrite64`: strace của thí nghiệm cho `pread64(4, "2", 3, 8) = 1`, `pwrite64(4, "XY", 2, 2) = 2`, `pread64(4, "veX", 3, 0) = 3`.

### 5.3 Bẫy `O_APPEND`

Giờ đến `journal.log`, file mở bằng `append(true)`. Nếu `ledger` thử `write_at` để sửa một record cũ, nó sẽ không sửa gì cả. Chính docs của `write_at` ghi một "Bug": trên một số hệ thống, file mở với `O_APPEND` thì `pwrite64` không tôn trọng offset mà luôn ghi vào cuối[^rs-writeat-bug]. Trên Linux, mục BUGS của man page `pwrite(2)` xác nhận: POSIX yêu cầu `O_APPEND` không ảnh hưởng vị trí ghi của `pwrite()`, nhưng Linux vẫn nối vào cuối bất kể `offset`[^man-pwrite-bug].

Thí nghiệm (chạy lại trong container): file 5 byte `12345`, mở bằng `.append(true)`, gọi `write_at(b"sushi", 10)` → strace `pwrite64(4, "sushi", 5, 10) = 5`, độ dài file là 10 (nối đuôi, không có khoảng đệm 0 tại offset 10). Kết luận (nhận xét của người soạn): file mà ứng dụng ghi page tại offset, như database làm, không được mở bằng `append(true)`.

<details><summary>Đi sâu: short read, <code>read_exact_at</code>/<code>write_all_at</code> và ghi vượt cuối file</summary>

Như `read`/`write`, hai hàm này có thể trả short: `read_at` đọc yêu cầu 3 byte tại offset 8 của file dài 9 byte chỉ trả 1 byte (đúng do hết file). Docs nói trả short read không phải lỗi[^rs-readat-short]. Vì vậy có `read_exact_at` và `write_all_at` để lặp cho đủ. Ghi vượt cuối file thì file được kéo dài và khoảng giữa điền 0[^rs-writeat-extend].

</details>

**Quay lại Q5:** `read_at`/`write_at` không đụng con trỏ file nên nhiều thread tra index của `ledger` không giẫm nhau, nhưng `O_APPEND` làm `write_at` bỏ qua offset trên Linux.

## 6. Mục phụ trợ: `read_dir`, metadata và quyền

> **Trả lời ngắn.** Thứ tự các entry mà `fs::read_dir` trả về phụ thuộc nền tảng và file system[^rs-readdir-platform]. Nó còn có thể đổi giữa các lần gọi[^rs-readdir-order]. Cần thứ tự ổn định thì phải tự sắp xếp.

**Giải thích.**

*Với `ledger`.* Khi khởi động, `ledger` replay các file segment `journal-000.log`, `journal-001.log`, … trong thư mục dữ liệu. Nếu nó đi theo thứ tự `read_dir` trả về và cho rằng đó là thứ tự tạo file, thì giao dịch sẽ bị replay lộn thứ tự.

`fs::read_dir` không hứa thứ tự nào: docs nói thứ tự có thể đổi giữa các lần gọi, và nếu cần tái lập thì phải sắp xếp tường minh[^rs-readdir-order]. Thứ tự đó phụ thuộc nền tảng và file system[^rs-readdir-platform]. Quan sát: trong thư mục thí nghiệm có 9 entry (gồm 7 file tạo theo thứ tự `zeta, alpha, mid, beta, omega, a1, k`), `read_dir` trả `["alpha.txt", "omega.txt", "ap", "a1.txt", "k.txt", "lock", "zeta.txt", "beta.txt", "mid.txt"]` — không theo thứ tự tạo hay thứ tự chữ cái. Đó là virtiofs trong container thí nghiệm; file system khác cho thứ tự khác.

Cách xử lý (nhận xét của người soạn): thu entry vào một `Vec` rồi `sort()`; tên segment đánh số cố định độ rộng như `journal-000.log` sort theo chữ cái là đúng thứ tự. Test hay output cần tái lập cũng làm vậy. Cơ chế thư mục ở [chương 03](./03-cai-dat-file-system.md).

<details><summary>Đi sâu: <code>metadata</code>, <code>PermissionsExt</code> và <code>statx</code></summary>

`File::metadata` và `fs::metadata` lấy thông tin inode; ở Linux std dùng `statx` (quan sát từ strace). `fs::Permissions` trên Unix được mở rộng bởi `PermissionsExt` với `mode()`, `set_mode()`, `from_mode()`[^rs-permsext]. Thí nghiệm: file tạo bởi `fs::write` có `mode()` = `0o100644` — tức giá trị `st_mode` đầy đủ gồm cả bit loại file `0o100000` (regular file) lẫn quyền `0o644` (quan sát; nhớ mask `& 0o7777` nếu chỉ muốn quyền). `fs::set_permissions(p, Permissions::from_mode(0o600))` chạy ra `fchmodat(AT_FDCWD, ..., 0600)`.

</details>

**Chốt mục phụ trợ:** không dựa vào thứ tự `read_dir` khi replay segment; `mode()` trên Unix trả cả bit loại file lẫn quyền.

## 7. File locking: `File::lock` (Q6)

> **Trả lời ngắn.** Có: `File::lock` đã stable từ 1.89.0[^src-lock-stable]. Trên Unix nó hiện tương ứng với `flock`[^rs-lock-flock]. Khóa có thể là advisory hoặc mandatory tùy nền tảng[^rs-lock-advisory]. Trên Linux, advisory nghĩa là process khác vẫn tự do bỏ qua khóa và I/O file nếu có quyền[^man-flock-advisory].

**Giải thích.**

*Với `ledger`.* Chạy hai instance `ledger` cùng ghi một thư mục dữ liệu là cách nhanh nhất để hỏng journal. Nên `ledger` giữ một file khóa: instance nào khóa được thì chạy, instance kia thoát. Trước 1.89.0 việc này cần crate ngoài (`fs2`, `fs4`) hoặc `libc::flock`. Giờ `std` có sẵn, và câu hỏi là khóa đó thực sự bảo vệ được tới đâu.

Source std 1.95.0 đánh dấu `File::lock` là `stable(feature = "file_lock", since = "1.89.0")`[^src-lock-stable], và docs 1.95.0 cũng ghi phiên bản 1.89.0 cạnh chữ ký hàm[^rs-lock-stable]. Các phương thức gồm `lock`, `lock_shared`, `try_lock`, `try_lock_shared`, `unlock`. Riêng `try_lock` trả `Err(TryLockError::WouldBlock)` nếu một khóa khác đang giữ qua handle khác[^rs-trylock]; đó là cách instance thứ hai của `ledger` biết mình phải thoát.

Có bốn điều cần nắm:

- Khóa có thể là advisory hoặc mandatory; tương tác của nó với `read`/`write` tùy nền tảng[^rs-lock-advisory].
- Trên Unix nó hiện tương ứng với `flock` (`LOCK_EX`, `LOCK_SH`), và docs nói điều này có thể đổi trong tương lai[^rs-lock-flock]. Source std 1.95.0 cho Linux gọi `flock` với `LOCK_EX | LOCK_NB` cho `try_lock`[^src-trylock-flock].
- Nếu handle này (hoặc bản clone của nó) đã giữ khóa thì gọi lại là hành vi không xác định và tùy nền tảng, kể cả deadlock[^rs-lock-reentrant].
- Khóa nhả khi file (cùng mọi descriptor nhân bản từ nó) bị đóng, hoặc khi gọi `unlock`[^rs-lock-release].

Chữ "advisory" ở tầng Linux có nghĩa cụ thể: man page `flock(2)` nói process vẫn tự do bỏ qua khóa và I/O trên file nếu có quyền[^man-flock-advisory]. Thêm nữa, khóa gắn với open file description, nên fd nhân bản bằng `fork` hoặc `dup` cùng chia sẻ một khóa[^man-flock-ofd]. Với `ledger`: khóa chỉ ngăn được những instance cũng chịu hỏi khóa, chứ không ngăn được một công cụ khác cứ thế mở và ghi vào journal.

<details><summary>Đi sâu: thí nghiệm hai handle và <code>flock</code></summary>

Thí nghiệm (strace + kết quả):

```text
a.lock()               -> flock(3, LOCK_EX) = 0
b.try_lock()           -> flock(4, LOCK_EX|LOCK_NB) = -1 EAGAIN  => Err(WouldBlock)
b.write_all_at(...)    -> vẫn ghi được, đọc lại thấy "ignored-lock"
a.unlock()             -> flock(3, LOCK_UN) = 0
b.try_lock()           -> Ok (sau unlock)
```

`a` và `b` là hai lần `open` độc lập cùng file (hai open file description khác nhau nên chặn nhau như hai process). Kết quả: khóa **không** ngăn một kẻ không hỏi khóa ghi đè dữ liệu. Chỉ dùng cho các bên hợp tác. Lưu ý: `flock` với NFS có lịch sử riêng (man page mục NFS), nên đừng giả định trên file system mạng.

</details>

**Quay lại Q6:** `File::lock` stable từ 1.89.0, hiện là `flock` advisory — đủ để `ledger` chặn instance thứ hai, nhưng chỉ dùng cho các bên hợp tác.

## 8. Memory-mapped file với `memmap2` (Q7)

> **Trả lời ngắn.** `Mmap::map` là `unsafe` vì có thể có Undefined Behavior khi dùng map nếu file nền bị sửa sau đó, trong hay ngoài process[^mm-safety]. Compiler của Rust không kiểm tra được điều này, nên trách nhiệm chuyển sang người gọi (suy luận của người soạn).

**Giải thích.**

*Với `ledger`.* File index của `ledger` lớn và các handler tra theo offset ngẫu nhiên, nên map nó vào bộ nhớ trông rất hấp dẫn: tra index chỉ còn là đọc một `&[u8]`, không syscall `read` nào. Nhưng chính vì `&[u8]` trông như dữ liệu bất biến, mà file phía sau lại là file thật mà process khác (ví dụ job rebuild index) có thể ghi, nên API đánh dấu `unsafe`. Mục này giải thích chỗ nguy hiểm.

`std` không có mmap; crate phổ biến là `memmap2`, và ví dụ ở đây ghim `memmap2 = "=0.9.11"` (docs.rs/memmap2/0.9.11).

### 8.1 Cơ chế

Với `mmap(2)` và `MAP_SHARED`, cập nhật trên mapping nhìn thấy được với process khác map cùng vùng, và với file-backed mapping thì được chuyển xuống file nền[^man-mmap-shared]. Muốn kiểm soát chính xác lúc nào chuyển xuống file thì phải dùng `msync(2)`[^man-msync]. Trong `memmap2`, `Mmap` là handle cho buffer map bất biến[^mm-immutable], còn `MmapMut` là phiên bản khả biến[^mm-mutable]. Mapping không phụ thuộc vào `File` đã tạo ra nó: nó vẫn hợp lệ sau khi `File` bị drop[^mm-independent]. Truy cập byte có thể gây page fault (chuyển trang vào RAM), chi tiết tùy nền tảng[^mm-pagefault]; cơ chế xem [chương bộ nhớ tiến trình](../03-memory/05-bo-nho-tien-trinh-linux.md) và [page fault](../03-memory/04-page-fault-va-swapping.md).

### 8.2 Vì sao `Mmap::map` là `unsafe`

Docs memmap2 (mục Safety) giải thích: mọi constructor file-backed đều `unsafe` vì có thể có Undefined Behavior khi dùng map nếu file nền sau đó bị sửa, trong hay ngoài process. Ứng dụng phải tự cân nhắc rủi ro; các biện pháp như quyền file, khóa hay file private (đã unlink) có tồn tại nhưng phụ thuộc nền tảng và hạn chế[^mm-safety].

Hiểu theo mô hình Rust (suy luận của người soạn): `&[u8]` lấy từ `Mmap` hứa là bất biến trong suốt lifetime. Nhưng RAM phía sau thực ra là page cache dùng chung, nên một process khác, hoặc chính ta qua đường khác, có thể ghi vào cùng trang. Compiler tối ưu dựa trên giả định bất biến, nên mâu thuẫn đó là UB.

Truncation là dạng khác của cùng vấn đề. Nếu job rebuild index làm file ngắn đi thì truy cập vào trang nằm ngoài cuối file đã map cho `SIGBUS`[^man-sigbus]. Signal này không đi qua `Result` của Rust, vì truy cập `&[u8]` chỉ là một phép đọc bộ nhớ thường, không có chỗ để trả lỗi (suy luận của người soạn).

### 8.3 Khi nào dùng

Trade-off (nhận xét của người soạn): mmap bỏ một lần copy kernel→user và cho truy cập ngẫu nhiên tiện. Đổi lại, lỗi I/O trở thành signal (`SIGBUS`) thay vì `Result`, độ trễ page fault khó thấy trong profile, và an toàn dựa vào kỷ luật không ai sửa file. Nhiều database chọn `pread`/`pwrite` hoặc direct I/O chính vì kiểm soát được lỗi và lịch ghi; xem [page cache và direct I/O](./04-page-cache-va-direct-io.md).

<details><summary>Đi sâu: thí nghiệm — hai mapping thấy nhau, slice "bất biến" đổi dưới chân</summary>

Thí nghiệm cho thấy chính nó (và vì vậy đoạn code dưới là minh họa **vì sao unsafe**, không phải mẫu nên theo):

```rust
let m  = unsafe { Mmap::map(&f)? };        // &[u8] "bất biến"
let mut mm = unsafe { MmapMut::map_mut(&f)? };
mm[0..5].copy_from_slice(b"HELLO");
mm.flush()?;
// std::fs::read_to_string(p)  -> "HELLO mmap"
// &m[..5] (slice "bất biến") -> "HELLO"   <- đã đổi dưới chân
```

Hai mapping của cùng file nhìn thấy nhau; slice mà type system gọi là bất biến đã đổi nội dung. (Đây đúng là điều `unsafe` ghi nhận trách nhiệm cho người gọi.)

</details>

**Quay lại Q7:** `Mmap::map` là `unsafe` vì an toàn bộ nhớ phụ thuộc việc không ai sửa file nền, điều type system không kiểm tra được; với index của `ledger`, đó là rủi ro bạn phải tự quản.

## 9. `tokio::fs`: async trên bề mặt, thread pool bên dưới (Q8)

> **Trả lời ngắn.** Không theo nghĩa kernel async. Đa số hệ điều hành không có API async cho file system, nên Tokio mặc định dùng thao tác file blocking thông thường chạy trên thread pool `spawn_blocking`[^tokio-blocking] (đường io_uring chỉ có khi bật thêm ba điều kiện, xem bên dưới). Bạn vẫn `await` được, nhưng việc thật nằm ở thread khác.

**Giải thích.**

*Với `ledger`.* Handler async của `ledger` đọc file cấu hình bằng `tokio::fs::read_to_string(...).await`. Nhìn vào code, người ta dễ nghĩ kernel đang đọc file bất đồng bộ và task không chặn gì cả. Thực tế không phải vậy, và hiểu đúng giúp bạn đoán được vì sao một loạt đọc file nhỏ có thể làm pool bận.

Tokio dùng thao tác file blocking thông thường phía sau, chạy trên thread pool `spawn_blocking`[^tokio-blocking]. Task gọi `.await` chỉ nhường executor trong lúc một thread của pool làm việc blocking đó (suy luận của người soạn). Docs của module còn nói hiện Tokio luôn dùng `spawn_blocking` trên mọi nền tảng và có thể đổi sang API async như io_uring trong tương lai[^tokio-uring]; câu này đã lỗi thời so với source, xem phần Đi sâu.

Hệ quả thực hành, theo docs, là gộp thao tác vào ít lần `spawn_blocking` nhất có thể[^tokio-batch]. Chẳng hạn `tokio::fs::read` và `tokio::fs::write` làm cả file trong một lần. Với nhiều lần đọc ghi nhỏ, hãy dùng `BufReader`/`BufWriter` để gom lại[^tokio-bufwriter]. Cũng như `BufWriter` đồng bộ, docs của `tokio::fs::File` dặn gọi `flush` trước khi drop[^tokio-file-flush]. Cơ chế `spawn_blocking`, executor và so sánh với io_uring ở phase P7: [Tokio](../07-io-async/08-tokio.md) và [kqueue/io_uring](../07-io-async/03-kqueue-io-uring.md).

<details><summary>Đi sâu: io_uring, named pipe và xác nhận bằng <code>strace</code> thread nào chạy <code>open</code></summary>

**io_uring.** Câu "luôn dùng `spawn_blocking`" trong docs module đã cũ so với source. Docs của `tokio::fs::read` ghi rằng trên Linux muốn dùng io_uring cần `--cfg tokio_unstable`, cargo feature `io-uring` và tùy chọn runtime `Builder::enable_io_uring`[^tokio-fs-uring]. Hỗ trợ này còn đang thử nghiệm (experimental)[^tokio-fs-uring-exp]. Vậy `spawn_blocking` vẫn là đường mặc định; chi tiết ba điều kiện và cách đọc hai đoạn docs không khớp nhau ở [P7 · Chương 03, mục 6](../07-io-async/03-kqueue-io-uring.md).

**File đặc biệt.** Chỉ nên dùng `tokio::fs` cho file thường: docs nói dùng nó với named pipe trên Linux có thể gây hành vi bất ngờ như treo lúc shutdown runtime[^tokio-special]. Với file đặc biệt hãy dùng `tokio::net::unix::pipe` hoặc `AsyncFd`.

**Xác nhận bằng strace** (`current_thread` runtime, `tokio = "=1.53.2"`): `std::fs::write` chạy trên thread chính (tid 91) nhưng `tokio::fs::read_to_string(...).await` cho `[pid 92] openat(... O_RDONLY|O_CLOEXEC)` sau một lần `clone(...CLONE_THREAD...)` — `open` chạy trên thread khác sinh ra cho pool.

</details>

**Quay lại Q8:** `tokio::fs` là giao diện async bọc thao tác blocking trên thread pool (mặc định; io_uring chỉ là đường thử nghiệm opt-in), nên handler của `ledger` nên gộp thao tác để giảm số lần `spawn_blocking`.

## Tóm tắt

- `OpenOptions` ánh xạ: `append`→`O_APPEND`, `truncate`→`O_TRUNC`, `create_new`→`O_CREAT|O_EXCL`; `File::create` = write+create+truncate (đã strace; docs Rust chỉ hứa hành vi, không hứa tên cờ).
- `create_new` là cách nguyên tử để "tạo nếu chưa tồn tại", như file khóa của `ledger`[^rs-createnew-atomic]. Khi file đã có, lỗi trả về là `AlreadyExists`[^rs-errors].
- `write` có thể ghi thiếu[^rs-write-partial]; `write_all` lặp lại đến hết hoặc lỗi[^rs-write-all].
- `File` không có buffer nên `File::flush` là no-op[^rs-file-flush]. `BufWriter` mặc định 8 KiB[^rs-bufwriter-8k] và gom 100.000 lần ghi thành 209 syscall (số đo một lần chạy). Drop của `BufWriter` nuốt lỗi flush nên phải `flush()` tường minh[^rs-bufwriter-flush].
- `flush` chỉ tới kernel. Muốn xuống thiết bị thì dùng `sync_all` (`fsync`)[^rs-sync-all-attempt] hoặc `sync_data` (`fdatasync`)[^rs-sync-data]. Với file mới phải sync cả thư mục[^man-fsync-dir].
- Mẫu bền cho `state.json`: file tạm `create_new` → `sync_all` → `rename` → sync thư mục; `fs::write` thì không nguyên tử, không bền (suy luận của người soạn từ quan sát `strace`).
- `FileExt::read_at/write_at` = `pread/pwrite`, không đổi cursor[^rs-readat]. Nhưng `O_APPEND` làm `write_at` bỏ qua offset[^rs-writeat-bug].
- `read_dir` không bảo đảm thứ tự; muốn ổn định thì sort[^rs-readdir-order].
- `File::lock` stable từ 1.89.0[^src-lock-stable]. Hiện nó là `flock`, advisory hoặc mandatory tùy nền tảng[^rs-lock-flock], và nhả khi đóng hoặc `unlock`[^rs-lock-release].
- `Mmap::map` là `unsafe` vì file nền bị sửa thì UB[^mm-safety]. `tokio::fs` mặc định chạy trên `spawn_blocking` (io_uring chỉ là đường thử nghiệm opt-in, ba điều kiện ở P7·03 mục 6)[^tokio-blocking].

## Câu hỏi tự kiểm tra

1. `OpenOptions::new().write(true).create_new(true)` truyền cờ `open(2)` nào, và lỗi khi file đã tồn tại là gì?
<details><summary>Đáp án</summary>

`O_WRONLY|O_CREAT|O_EXCL|O_CLOEXEC`. Khi file đã có, kernel trả `EEXIST`[^man-oexcl] và Rust đổi thành `ErrorKind::AlreadyExists`. Nếu lệnh thành công, file trả về chắc chắn là file mới[^rs-createnew].

</details>

2. Vì sao `seek(End(0))` + `write` không thay được `append(true)`?
<details><summary>Đáp án</summary>

Có race giữa seek và write[^rs-append]. `O_APPEND` gộp việc dịch offset và ghi thành một bước nguyên tử[^man-oappend].

</details>

3. `BufWriter` drop khi đĩa đầy: chương trình thấy gì, và cách đúng?
<details><summary>Đáp án</summary>

Không thấy gì vì lỗi bị bỏ qua. Cách đúng là gọi `flush()` (hoặc `into_inner()`) và kiểm tra `Result`[^rs-bufwriter-flush].

</details>

4. `write` trả `Ok(3)` cho buffer 8 byte: có phải lỗi không?
<details><summary>Đáp án</summary>

Không, đó không bị coi là lỗi[^rs-write-partial]. Phải gọi lại phần còn lại, hoặc dùng `write_all`[^rs-write-all].

</details>

5. `flush()` của `BufWriter` có bảo đảm dữ liệu bền khi mất điện không?
<details><summary>Đáp án</summary>

Không, nó chỉ đẩy dữ liệu tới kernel. `File::flush` còn là no-op[^rs-file-flush]. Muốn bền thì cần `sync_all`/`sync_data`: drop không phải chờ dữ liệu xuống đĩa[^rs-sync-all], và docs `tokio::fs::File` nói flush không bảo đảm file đã ghi hết xuống đĩa[^tokio-file-flush].

</details>

6. Điều gì xảy ra nếu file mở bằng `append(true)` rồi `write_at(buf, 10)`?
<details><summary>Đáp án</summary>

Dữ liệu vẫn được ghi vào cuối file, bỏ qua offset[^rs-writeat-bug].

</details>

7. Vì sao test dựa vào thứ tự `read_dir` là sai?
<details><summary>Đáp án</summary>

Thứ tự phụ thuộc nền tảng và file system[^rs-readdir-platform]. Nó còn có thể đổi giữa các lần gọi[^rs-readdir-order].

</details>

8. `File::lock` ngăn process khác ghi vào file được không?
<details><summary>Đáp án</summary>

Không bảo đảm: trên Linux là `flock` advisory và tiến trình có quyền vẫn ghi được (đã chạy thật)[^man-flock-advisory]. Docs Rust cũng chỉ nói khóa có thể là advisory hoặc mandatory tùy nền tảng[^rs-lock-advisory].

</details>

9. Vì sao `Mmap::map` là `unsafe` dù chỉ để đọc?
<details><summary>Đáp án</summary>

File nền bị sửa trong hay ngoài process có thể gây UB khi dùng map[^mm-safety].

</details>

10. `tokio::fs::read_to_string` chạy trên thread nào, và vì sao?
<details><summary>Đáp án</summary>

Mặc định trên thread của pool `spawn_blocking`, vì OS phổ thông thiếu async file API[^tokio-blocking]. Chỉ khi bật `--cfg tokio_unstable` + feature `io-uring` + `Builder::enable_io_uring` thì Linux mới có đường io_uring thử nghiệm[^tokio-fs-uring].

</details>

11. Vì sao nên `create_new` cho file tạm trong mẫu ghi nguyên tử?
<details><summary>Đáp án</summary>

Bảo đảm file là mới và tránh TOCTOU hay giẫm lên file của bên khác[^rs-createnew-atomic].

</details>

## Đọc thêm

- [`std::fs::File`](https://doc.rust-lang.org/1.95.0/std/fs/struct.File.html) — toàn bộ API file, phần `lock`/`sync_*` và ghi chú Drop.
- [`std::fs::OpenOptions`](https://doc.rust-lang.org/1.95.0/std/fs/struct.OpenOptions.html) — bảng lỗi và quy tắc kết hợp tùy chọn.
- [`BufWriter`](https://doc.rust-lang.org/1.95.0/std/io/struct.BufWriter.html) — phần cảnh báo flush/drop và `into_inner`.
- [`open(2)`](https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html) — nghĩa chính xác của từng cờ, đặc biệt `O_APPEND`/`O_EXCL`/`O_TRUNC`.
- [`fsync(2)`](https://manpages.debian.org/bookworm/manpages-dev/fsync.2.en.html) — hợp đồng bền dữ liệu và thư mục.
- [`mmap(2)`](https://manpages.debian.org/bookworm/manpages-dev/mmap.2.en.html) — `MAP_SHARED`, `MAP_PRIVATE`, `SIGBUS`.
- [`tokio::fs`](https://docs.rs/tokio/1.53.2/tokio/fs/index.html) — mục "Tuning your file IO".
- [Durability trong thực tế](./06-durability-trong-thuc-te.md) — lý do đằng sau từng bước của mẫu ghi nguyên tử.

[^rs-append]: Rust std, OpenOptions::append — <https://doc.rust-lang.org/1.95.0/std/fs/struct.OpenOptions.html> — "Append mode guarantees that writes will be positioned at the current end of file, even when there are other processes or threads appending to the same file."
[^man-oappend]: open(2), O_APPEND — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "The modification of the file offset and the write operation are performed as a single atomic step."
[^rs-append-interleave]: Rust std, OpenOptions::append — <https://doc.rust-lang.org/1.95.0/std/fs/struct.OpenOptions.html> — "A successful write() is allowed to write only part of the given data, so even if you’re careful to provide the whole message in a single call to write(), there is no guarantee that it will be written out in full."
[^man-nfs-append]: open(2), O_APPEND — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "O_APPEND may lead to corrupted files on NFS filesystems if more than one process appends data to a file at once."
[^rs-trunc]: Rust std, OpenOptions::truncate — <https://doc.rust-lang.org/1.95.0/std/fs/struct.OpenOptions.html> — "If a file is successfully opened with this option set to true, it will truncate the file to 0 length if it already exists."
[^man-otrunc]: open(2), O_TRUNC — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "If the file already exists and is a regular file and the access mode allows writing (i.e., is O_RDWR or O_WRONLY) it will be truncated to length 0."
[^rs-createnew]: Rust std, OpenOptions::create_new — <https://doc.rust-lang.org/1.95.0/std/fs/struct.OpenOptions.html> — "In this way, if the call succeeds, the file returned is guaranteed to be new."
[^rs-createnew-atomic]: Rust std, OpenOptions::create_new — <https://doc.rust-lang.org/1.95.0/std/fs/struct.OpenOptions.html> — "This option is useful because it is atomic. Otherwise between checking whether a file exists and creating a new one, the file may have been created by another process (a TOCTOU race condition / attack)."
[^rs-createnew-ignored]: Rust std, OpenOptions::create_new — <https://doc.rust-lang.org/1.95.0/std/fs/struct.OpenOptions.html> — "If .create_new(true) is set, .create() and .truncate() are ignored."
[^man-oexcl]: open(2), O_EXCL — <https://manpages.debian.org/bookworm/manpages-dev/open.2.en.html> — "Ensure that this call creates the file: if this flag is specified in conjunction with O_CREAT, and pathname already exists, then open() fails with the error EEXIST."
[^rs-errors]: Rust std, OpenOptions::open, Errors — <https://doc.rust-lang.org/1.95.0/std/fs/struct.OpenOptions.html> — "AlreadyExists: create_new was specified and the file already exists."
[^rs-create-needs-write]: Rust std, OpenOptions::create — <https://doc.rust-lang.org/1.95.0/std/fs/struct.OpenOptions.html> — "If .create(true) is set without .write(true) or .append(true), calling open will fail with InvalidInput error."
[^rs-file-drop]: Rust std, File — <https://doc.rust-lang.org/1.95.0/std/fs/struct.File.html> — "Files are automatically closed when they go out of scope. Errors detected on closing are ignored by the implementation of Drop. Use the method sync_all if these errors must be manually handled."
[^rs-write-partial]: Rust std, Write::write — <https://doc.rust-lang.org/1.95.0/std/io/trait.Write.html> — "It is not considered an error if the entire buffer could not be written to this writer."
[^man-write-partial]: write(2) — <https://manpages.debian.org/bookworm/manpages-dev/write.2.en.html> — "In the event of a partial write, the caller can make another write() call to transfer the remaining bytes."
[^rs-write-all]: Rust std, Write::write_all — <https://doc.rust-lang.org/1.95.0/std/io/trait.Write.html> — "This method will continuously call write until there is no more data to be written or an error of non-ErrorKind::Interrupted kind is returned."
[^rs-bufwriter-when]: Rust std, BufWriter — <https://doc.rust-lang.org/1.95.0/std/io/struct.BufWriter.html> — "It does not help when writing very large amounts at once, or writing just one or a few times."
[^rs-bufwriter-8k]: Rust std, BufWriter::new — <https://doc.rust-lang.org/1.95.0/std/io/struct.BufWriter.html> — "Creates a new BufWriter\<W> with a default buffer capacity. The default is currently 8 KiB, but may change in the future."
[^rs-bufreader-8k]: Rust std, BufReader::new — <https://doc.rust-lang.org/1.95.0/std/io/struct.BufReader.html> — "Creates a new BufReader\<R> with a default buffer capacity. The default is currently 8 KiB, but may change in the future."
[^rs-bufreader-loss]: Rust std, BufReader — <https://doc.rust-lang.org/1.95.0/std/io/struct.BufReader.html> — "When the BufReader\<R> is dropped, the contents of its buffer will be discarded. Creating multiple instances of a BufReader\<R> on the same stream can cause data loss."
[^rs-bufwriter-flush]: Rust std, BufWriter — <https://doc.rust-lang.org/1.95.0/std/io/struct.BufWriter.html> — "It is critical to call flush before BufWriter\<W> is dropped. Though dropping will attempt to flush the contents of the buffer, any errors that happen in the process of dropping will be ignored."
[^rs-sync-all]: Rust std, File::sync_all — <https://doc.rust-lang.org/1.95.0/std/fs/struct.File.html> — "Note, however, that sync_all is generally more expensive than closing a file by dropping it, because the latter is not required to block until the data has been written to the filesystem."
[^rs-sync-data]: Rust std, File::sync_data — <https://doc.rust-lang.org/1.95.0/std/fs/struct.File.html> — "This function is similar to sync_all, except that it might not synchronize file metadata to the filesystem."
[^rs-sync-data-platform]: Rust std, File::sync_data — <https://doc.rust-lang.org/1.95.0/std/fs/struct.File.html> — "Note that some platforms may simply implement this in terms of sync_all."
[^man-fsync-dir]: fsync(2) — <https://manpages.debian.org/bookworm/manpages-dev/fsync.2.en.html> — "Calling fsync() does not necessarily ensure that the entry in the directory containing the file has also reached disk. For that an explicit fsync() on a file descriptor for the directory is also needed."
[^tokio-file-flush]: tokio::fs::File — <https://docs.rs/tokio/1.53.2/tokio/fs/struct.File.html> — "Note that this does not ensure that the file has been fully written to disk; the operating system might keep the changes around in an in-memory buffer."
[^rs-fs-write]: Rust std, fs::write — <https://doc.rust-lang.org/1.95.0/std/fs/fn.write.html> — "This function will create a file if it does not exist, and will entirely replace its contents if it does."
[^rs-fs-rts]: Rust std, fs::read_to_string — <https://doc.rust-lang.org/1.95.0/std/fs/fn.read_to_string.html> — "While reading from the file, this function handles io::ErrorKind::Interrupted with automatic retries."
[^rs-rename]: Rust std, fs::rename — <https://doc.rust-lang.org/1.95.0/std/fs/fn.rename.html> — "Renames a file or directory to a new name, replacing the original file if to already exists. This will not work if the new name is on a different mount point."
[^man-rename]: rename(2) — <https://manpages.debian.org/bookworm/manpages-dev/rename.2.en.html> — "If newpath already exists, it will be atomically replaced, so that there is no point at which another process attempting to access newpath will find it missing."
[^rs-rename-err]: Rust std, fs::rename, Errors — <https://doc.rust-lang.org/1.95.0/std/fs/fn.rename.html> — "from and to are on separate filesystems."
[^rs-file-seek]: Rust std, File — <https://doc.rust-lang.org/1.95.0/std/fs/struct.File.html> — "Files also implement Seek to alter the logical cursor that the file contains internally."
[^rs-readat]: Rust std, FileExt::read_at — <https://doc.rust-lang.org/1.95.0/std/os/unix/fs/trait.FileExt.html> — "The offset is relative to the start of the file and thus independent from the current cursor. The current file cursor is not affected by this function."
[^rs-readat-short]: Rust std, FileExt::read_at — <https://doc.rust-lang.org/1.95.0/std/os/unix/fs/trait.FileExt.html> — "Note that similar to File::read, it is not an error to return with a short read."
[^rs-writeat-extend]: Rust std, FileExt::write_at — <https://doc.rust-lang.org/1.95.0/std/os/unix/fs/trait.FileExt.html> — "When writing beyond the end of the file, the file is appropriately extended and the intermediate bytes are initialized with the value 0."
[^rs-writeat-bug]: Rust std, FileExt::write_at, Bug — <https://doc.rust-lang.org/1.95.0/std/os/unix/fs/trait.FileExt.html> — "However, this syscall has a bug where files opened with the O_APPEND flag fail to respect the offset parameter, always appending to the end of the file instead."
[^rs-permsext]: Rust std, PermissionsExt — <https://doc.rust-lang.org/1.95.0/std/os/unix/fs/trait.PermissionsExt.html> — "Unix-specific extensions to fs::Permissions."
[^rs-readdir-order]: Rust std, fs::read_dir — <https://doc.rust-lang.org/1.95.0/std/fs/fn.read_dir.html> — "The order in which read_dir returns entries can change between calls. If reproducible ordering is required, the entries should be explicitly sorted."
[^rs-readdir-platform]: Rust std, fs::read_dir — <https://doc.rust-lang.org/1.95.0/std/fs/fn.read_dir.html> — "The order in which this iterator returns entries is platform and filesystem dependent."
[^rs-lock-stable]: Rust std, File::lock — <https://doc.rust-lang.org/1.95.0/std/fs/struct.File.html> — "1.89.0 · Source pub fn lock(&self) -> Result<()> Acquire an exclusive lock on the file. Blocks until the lock can be acquired."
[^rs-trylock]: Rust std, File::try_lock — <https://doc.rust-lang.org/1.95.0/std/fs/struct.File.html> — "Returns Err(TryLockError::WouldBlock) if a different lock is already held on this file (via another handle/descriptor)."
[^rs-lock-advisory]: Rust std, File::lock — <https://doc.rust-lang.org/1.95.0/std/fs/struct.File.html> — "This lock may be advisory or mandatory. This lock is meant to interact with lock, try_lock, lock_shared, try_lock_shared, and unlock. Its interactions with other methods, such as read and write are platform specific, and it may or may not cause non-lockholders to block."
[^rs-lock-flock]: Rust std, File::lock — <https://doc.rust-lang.org/1.95.0/std/fs/struct.File.html> — "This function currently corresponds to the flock function on Unix with the LOCK_EX flag, and the LockFileEx function on Windows with the LOCKFILE_EXCLUSIVE_LOCK flag. Note that, this may change in the future."
[^rs-lock-reentrant]: Rust std, File::lock — <https://doc.rust-lang.org/1.95.0/std/fs/struct.File.html> — "If this file handle/descriptor, or a clone of it, already holds a lock the exact behavior is unspecified and platform dependent, including the possibility that it will deadlock."
[^rs-lock-release]: Rust std, File::lock — <https://doc.rust-lang.org/1.95.0/std/fs/struct.File.html> — "The lock will be released when this file (along with any other file descriptors/handles duplicated or inherited from it) is closed, or if the unlock method is called."
[^man-flock-advisory]: flock(2) — <https://manpages.debian.org/bookworm/manpages-dev/flock.2.en.html> — "flock() places advisory locks only; given suitable permissions on a file, a process is free to ignore the use of flock() and perform I/O on the file."
[^man-flock-ofd]: flock(2) — <https://manpages.debian.org/bookworm/manpages-dev/flock.2.en.html> — "Locks created by flock() are associated with an open file description (see open(2)). This means that duplicate file descriptors (created by, for example, fork(2) or dup(2)) refer to the same lock"
[^man-mmap-shared]: mmap(2), MAP_SHARED — <https://manpages.debian.org/bookworm/manpages-dev/mmap.2.en.html> — "Share this mapping. Updates to the mapping are visible to other processes mapping the same region, and (in the case of file-backed mappings) are carried through to the underlying file."
[^mm-independent]: memmap2 0.9.11, Mmap — <https://docs.rs/memmap2/0.9.11/memmap2/struct.Mmap.html> — "A file backed Mmap is created by &File reference, and will remain valid even after the File is dropped."
[^mm-pagefault]: memmap2 0.9.11, Mmap — <https://docs.rs/memmap2/0.9.11/memmap2/struct.Mmap.html> — "Dereferencing and accessing the bytes of the buffer may result in page faults (e.g. swapping the mapped pages into physical memory) though the details of this are platform specific."
[^mm-safety]: memmap2 0.9.11, Mmap, Safety — <https://docs.rs/memmap2/0.9.11/memmap2/struct.Mmap.html> — "All file-backed memory map constructors are marked unsafe because of the potential for Undefined Behavior (UB) using the map if the underlying file is subsequently modified, in or out of process. Applications must consider the risk and take appropriate precautions when using file-backed maps. Solutions such as file permissions, locks or process-private (e.g. unlinked) files exist but are platform specific and limited."
[^mm-immutable]: memmap2 0.9.11, Mmap — <https://docs.rs/memmap2/0.9.11/memmap2/struct.Mmap.html> — "A handle to an immutable memory mapped buffer."
[^mm-mutable]: memmap2 0.9.11, Mmap — <https://docs.rs/memmap2/0.9.11/memmap2/struct.Mmap.html> — "See MmapMut for the mutable version."
[^man-sigbus]: mmap(2), SIGBUS — <https://manpages.debian.org/bookworm/manpages-dev/mmap.2.en.html> — "Attempted access to a page of the buffer that lies beyond the end of the mapped file."
[^tokio-blocking]: tokio 1.53.2, tokio::fs — <https://docs.rs/tokio/1.53.2/tokio/fs/index.html> — "Be aware that most operating systems do not provide asynchronous file system APIs. Because of that, Tokio will use ordinary blocking file operations behind the scenes. This is done using the spawn_blocking threadpool to run them in the background."
[^tokio-uring]: tokio 1.53.2, tokio::fs — <https://docs.rs/tokio/1.53.2/tokio/fs/index.html> — "Currently, Tokio will always use spawn_blocking on all platforms, but it may be changed to use asynchronous file system APIs such as io_uring in the future."
[^tokio-fs-uring]: Tokio 1.53.2, source fs/read.rs, mục io_uring support — <https://raw.githubusercontent.com/tokio-rs/tokio/tokio-1.53.2/tokio/src/fs/read.rs> — "To enable io_uring, you need to specify the --cfg tokio_unstable flag at compile time, enable the io-uring cargo feature, and set the Builder::enable_io_uring runtime option."
[^tokio-fs-uring-exp]: Tokio 1.53.2, source fs/read.rs, mục io_uring support — <https://raw.githubusercontent.com/tokio-rs/tokio/tokio-1.53.2/tokio/src/fs/read.rs> — "Support for io_uring is currently experimental, so its behavior may change or it may be removed in future versions."
[^tokio-special]: tokio 1.53.2, tokio::fs — <https://docs.rs/tokio/1.53.2/tokio/fs/index.html> — "The tokio::fs module should only be used for ordinary files. Trying to use it with e.g., a named pipe on Linux can result in surprising behavior, such as hangs during runtime shutdown."
[^tokio-batch]: tokio 1.53.2, tokio::fs, Tuning your file IO — <https://docs.rs/tokio/1.53.2/tokio/fs/index.html> — "it is recommended to batch your operations into as few spawn_blocking calls as possible."
[^tokio-bufwriter]: tokio 1.53.2, tokio::fs, Tuning your file IO — <https://docs.rs/tokio/1.53.2/tokio/fs/index.html> — "Use BufReader and BufWriter to buffer many small reads or writes into a few large ones."
[^rs-file-flush]: Rust std, File::flush, Platform-specific behavior — <https://doc.rust-lang.org/1.95.0/std/fs/struct.File.html> — "Since a File structure doesn't contain any buffers, this function is currently a no-op on Unix and Windows."
[^src-file-flush]: Rust 1.95.0 source, library/std/src/sys/fs/unix.rs, File::flush — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/fs/unix.rs> — "pub fn flush(&self) -> io::Result<()> { Ok(()) }"
[^src-trylock-flock]: Rust 1.95.0 source, library/std/src/sys/fs/unix.rs, File::try_lock — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/fs/unix.rs> — "let result = cvt(unsafe { libc::flock(self.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) });"
[^man-pwrite-bug]: pwrite(2), BUGS — <https://manpages.debian.org/bookworm/manpages-dev/pwrite.2.en.html> — "However, on Linux, if a file is opened with O_APPEND, pwrite() appends data to the end of the file, regardless of the value of offset."
[^man-exdev]: rename(2), EXDEV — <https://manpages.debian.org/bookworm/manpages-dev/rename.2.en.html> — "oldpath and newpath are not on the same mounted filesystem."
[^man-msync]: mmap(2), MAP_SHARED — <https://manpages.debian.org/bookworm/manpages-dev/mmap.2.en.html> — "To precisely control when updates are carried through to the underlying file requires the use of msync(2)."
[^rs-sync-all-attempt]: Rust std, File::sync_all — <https://doc.rust-lang.org/1.95.0/std/fs/struct.File.html> — "This function will attempt to ensure that all in-memory data reaches the filesystem before returning."
[^rs-fileext-self]: Rust std, FileExt — <https://doc.rust-lang.org/1.95.0/std/os/unix/fs/trait.FileExt.html> — "fn read_at(&self, buf: &mut [u8], offset: u64) -> Result\<usize>;"
[^src-lock-stable]: Rust 1.95.0 source, library/std/src/fs.rs, File::lock — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/fs.rs> — "#[stable(feature = "file_lock", since = "1.89.0")]"
