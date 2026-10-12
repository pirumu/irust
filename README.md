# Rust Internals: nền tảng hệ thống cho backend engineer

Giáo trình tự học bằng tiếng Việt về những gì nằm bên dưới một service Rust: kernel và system call, CPU, bộ nhớ, concurrency, lưu trữ, networking, async I/O, container, ngôn ngữ và trình biên dịch Rust, đo đạc và gỡ lỗi, hệ thống dữ liệu, đặc tả hình thức với TLA+, toán nền cho hệ thống và thuật toán phân tán.

Giáo trình gồm **15 phase, 137 chương lý thuyết và 15 bài lab** có code chạy được. Mọi khẳng định sự thật đều có chú thích kèm câu trích nguyên văn từ nguồn, và có script tự động kiểm tra lại các câu trích đó.

**Đọc online:** <https://pirumu.github.io/irust/>

**Bắt đầu đọc:** [Lộ trình](docs/00-roadmap.md) · [Bảng thuật ngữ Việt–Anh](docs/thuat-ngu.md) · [Lab đầu tiên](docs/labs/01-linux-lab.md)

---

## Dành cho ai

Giáo trình viết cho backend engineer đã đi làm nhiều năm, đang học hoặc đang dùng Rust, và bắt đầu vướng khi chạm tới tầng dưới. Ví dụ:

- Vì sao `Mutex` chậm khi tranh chấp?
- Future thực sự được poll thế nào?
- Container "hết RAM" là hết cái gì?
- Vì sao replica đọc ra dữ liệu cũ?

Giáo trình không giả định bạn đã học hệ điều hành hay mạng máy tính bài bản. Mỗi khái niệm được giải nghĩa ở lần đầu xuất hiện.

## Nội dung

| Phase | Chủ đề | Lab (crate) |
|---|---|---|
| [P1](docs/01-linux/00-tong-quan.md) | Linux và system interface: kernel, system call, process, file descriptor, signal, `/proc` | [Lab P1](docs/labs/01-linux-lab.md) (`lab-linux`) |
| [P2](docs/02-cpu/00-tong-quan.md) | CPU: assembly x86-64/AArch64, calling convention, pipeline, branch prediction, cache | [Lab P2](docs/labs/02-cpu-lab.md) (`lab-cpu`) |
| [P3](docs/03-memory/00-tong-quan.md) | Bộ nhớ: address space, paging, TLB, allocator, stack, Rust và bộ nhớ | [Lab P3](docs/labs/03-memory-lab.md) (`lab-memory`) |
| [P4](docs/04-concurrency/00-tong-quan.md) | Concurrency: thread, scheduling, lock, memory model, lock-free, `Send`/`Sync` | [Lab P4](docs/labs/04-concurrency-lab.md) (`lab-concurrency`) |
| [P5](docs/05-storage/00-tong-quan.md) | Lưu trữ: thiết bị, file system, page cache, journaling, durability | [Lab P5](docs/labs/05-storage-lab.md) (`lab-storage`) |
| [P6](docs/06-networking/00-tong-quan.md) | Networking: IP, TCP, congestion control, socket, DNS/HTTP/TLS, zero-copy | [Lab P6](docs/labs/06-networking-lab.md) (`lab-networking`) |
| [P7](docs/07-io-async/00-tong-quan.md) | I/O và async: epoll, io_uring, `Future`/`Waker`/`Pin`, mio, Tokio, async trong production | [Lab P7](docs/labs/07-io-async-lab.md) (`lab-io-async`) |
| [P8](docs/08-containers/00-tong-quan.md) | Container: namespaces, cgroups v2, filesystem, bảo mật, OCI, mạng container | [Lab P8](docs/labs/08-containers-lab.md) (`lab-containers`) |
| [P9](docs/09-rust-language/00-tong-quan.md) | Ngôn ngữ Rust: ownership, trait, generics, smart pointer, lỗi, closure, macro, Cargo | [Lab P9](docs/labs/09-rust-language-lab.md) (`lab-language`) |
| [P10](docs/10-rust-internals/00-tong-quan.md) | Rust internals: pipeline rustc, layout, vtable, monomorphization, borrow checker, unsafe/UB, FFI | [Lab P10](docs/labs/10-rust-internals-lab.md) (`lab-internals`) |
| [P11](docs/11-do-dac-va-go-loi/00-tong-quan.md) | Đo đạc và gỡ lỗi: perf, eBPF, hiệu năng service, debugger/DWARF, core dump, ELF/linker/loader | [Lab P11](docs/labs/11-do-dac-va-go-loi-lab.md) (`lab-observability`) |
| [P12](docs/12-he-thong-du-lieu/00-tong-quan.md) | Hệ thống dữ liệu: storage engine, encoding, replication, partitioning, transaction, consensus, stream; case study PostgreSQL và Redis; chương chuyên sâu về thuật toán đồng thuận | [P12a](docs/labs/12a-du-lieu-mot-node-lab.md) (`lab-data`), [P12b](docs/labs/12b-du-lieu-phan-tan-lab.md) (`lab-distributed`) |
| [P13](docs/13-tla-plus/00-tong-quan.md) | TLA+: state/action, invariant và TLC, liveness, PlusCal, refinement, 2PC/Paxos/Raft, từ spec đến code Rust | [Lab P13](docs/labs/13-tla-plus-lab.md) (`lab-tla`) |
| [P14](docs/14-toan-va-ly-thuyet/00-tong-quan.md) | Toán và lý thuyết nền: logic và chứng minh, toán rời rạc, xác suất, độ phức tạp, queueing theory, thống kê cho benchmark, tổ hợp và đếm, số học modular và mật mã, chuỗi Markov và quá trình Poisson; mỗi chương có bài tập kèm lời giải | — |
| [P15](docs/15-thuat-toan-phan-tan/00-tong-quan.md) | Thuật toán phân tán: FLP, broadcast, consistency, CRDT, mutex và bầu leader, snapshot, gossip, chain replication, giao dịch phân tán, BFT | — |
| — | [Capstone](docs/labs/capstone.md): HTTP server trên epoll, rồi tự viết mini async runtime | `lab-capstone` |

Không có thời gian đọc hết? [Lộ trình](docs/00-roadmap.md) có một **đường lõi 37 chương** chọn cho người làm backend service bằng Rust.

## Cách đọc

Mỗi chương mở đầu bằng danh sách câu hỏi Q1…Qn. Mỗi câu được trả lời theo ba tầng:

1. **Trả lời ngắn:** 2–4 câu lời thường. Chỉ đọc tầng này cũng đủ biết đáp án.
2. **Giải thích:** một hình dung đời thường hoặc tình huống backend, rồi cơ chế từng bước.
3. **Đi sâu:** khối gập chứa source code, cờ syscall, phiên bản kernel, số đo và các trường hợp hiếm.

Cuối chương có phần liên hệ với Rust, tóm tắt, câu hỏi tự kiểm tra (đáp án gập sẵn) và tài liệu đọc thêm. Lab làm sau khi đã đọc phase lý thuyết tương ứng.

## Nguyên tắc biên soạn

**Có nguồn cho mọi khẳng định.** Mỗi khẳng định sự thật đi kèm một chú thích, định nghĩa ở cuối file theo dạng:

```
[^id]: Tên nguồn, mục — <https://url-nguồn> — "câu trích nguyên văn từ nguồn"
```

- Nguồn được ghim phiên bản khi có thể: docs Rust **1.95.0**, crate theo phiên bản trên docs.rs, man page Debian bookworm, mã nguồn theo tag hoặc commit.
- Một số nguồn chỉ có bản đang cập nhật nên không ghim được: rustc-dev-guide, docs.kernel.org, OSTEP, Rust Atomics and Locks, HPBN. Nếu verifier báo `FAIL` ở các nguồn này, khả năng cao là nguồn đã sửa câu chữ.
- Lời khuyên hoặc nhận định của người soạn được ghi rõ là như vậy. Câu không có chú thích không được coi là sự thật đã kiểm chứng.

**Output lab là output thật.** Mọi khối output trong lab là kết quả chạy thật; giá trị phụ thuộc máy (PID, địa chỉ, thời gian) sẽ khác trên máy bạn. Có hai ngoại lệ:

- Lab P9 không in output mẫu, vì kết quả là test của bạn xanh hay đỏ.
- Ở lab P11, P12a, P12b và P13, chỉ phần Rust đã được chạy (trên macOS). Các bước cần Linux (`perf`, `gdb`, `bpftrace`, core dump), PostgreSQL, Redis hoặc TLC được ghi rõ là **chưa chạy**. Các bước đó có lệnh và kỳ vọng rút từ nguồn, nhưng không có output.

## Bắt đầu nhanh

### Đọc dạng sách

Cần [mdBook](https://github.com/rust-lang/mdBook) 0.5.2 và [mdbook-mermaid](https://github.com/badboy/mdbook-mermaid) 0.17.0:

```bash
cargo install mdbook --version 0.5.2 --locked
cargo install mdbook-mermaid --version 0.17.0 --locked

mdbook serve --open    # http://localhost:3000, tự reload khi sửa
mdbook build           # build ra book/
```

Đọc trực tiếp trên GitHub cũng được: mọi chương là Markdown thuần trong `docs/`.

### Chạy lab

Cần Rust 1.95 (qua `rustup`). Các lab dùng interface chỉ có trên Linux cần thêm Docker (Docker Desktop hoặc `colima start`).

```bash
cd labs
cargo build --workspace                         # build mọi lab
cargo test --workspace --exclude lab-language   # chạy test của các lab
cargo run -p lab-data --bin kvlog               # chạy một lab ngay trên macOS

cd ..
./labs/linux.sh                                 # shell trong container Debian bookworm + Rust 1.95
./labs/linux.sh cargo run -p lab-linux --bin fileio
./labs/linux.sh bash -c 'cargo build -p lab-linux && strace -f ./target-linux/debug/fileio'
```

- Lần đầu chạy, `linux.sh` build image `rust-foundations-lab:1.95`. Image có sẵn `strace`, `gdb`, `ss`, `tcpdump`, `nc`, `dig`, `curl` và man pages.
- Build output trong container nằm ở `labs/target-linux/`, tách khỏi bản build trên máy host.
- Lệnh cụ thể của từng bài nằm trong file lab tương ứng ở `docs/labs/`.

> **Lưu ý về `lab-language`.** Lab P9 là lab điền code: mỗi bài là một hàm `todo!()`, bộ test là đặc tả, nên **test cố ý fail cho tới khi bạn làm xong bài**. Lời giải mẫu chạy bằng `cargo test -p lab-language --features solution`. Chỉ nên mở lời giải sau khi đã tự làm. Chi tiết ở [lab P9](docs/labs/09-rust-language-lab.md).

## Kiểm tra chất lượng

```bash
python3 tools/verify_citations.py                              # kiểm mọi câu trích trong docs/
python3 tools/verify_citations.py docs/04-concurrency/03-lock.md
python3 tools/check_links.py                                   # link tương đối trong docs/ và README
python3 tools/fix_markdown_html.py --check                     # Markdown render đúng trên cả GitHub và mdBook
```

| Script | Làm gì |
|---|---|
| `verify_citations.py` | Tải từng URL, trích văn bản (HTML, hoặc PDF qua `pdftotext`), chuẩn hóa, rồi kiểm câu trích có xuất hiện liền mạch trong nguồn. Báo cả chú thích dùng mà không định nghĩa và ngược lại. Nguồn được cache ở `.cache/`; xóa `.cache/sources` để tải lại từ đầu. Cần Python 3.10+, `pdftotext` (`brew install poppler`) và mạng. |
| `check_links.py` | Kiểm mọi link Markdown tương đối trỏ tới file có thật. |
| `fix_markdown_html.py` | Sửa những chỗ GitHub render được nhưng mdBook thì không: dòng trống quanh `<details>`, `Vec<T>` ngoài code span bị hiểu là thẻ HTML, link nằm trong câu trích. Chạy không có `--check` để tự sửa. Chạy lại nhiều lần cho cùng kết quả. |

**Giới hạn của verifier:** nó chỉ chứng minh câu trích **có thật trong nguồn**, không chứng minh câu trích đỡ được khẳng định đứng trước nó. Phần đó dựa vào review và vào chính người đọc.

## Cấu trúc repo

```
docs/                     nguồn của sách (mdBook src)
  SUMMARY.md              mục lục sách
  00-roadmap.md           lộ trình, đường lõi, cách đọc
  thuat-ngu.md            bảng thuật ngữ Việt–Anh
  NN-<phase>/             mỗi phase: 00-tong-quan.md (bản đồ chương, câu hỏi tổng hợp) + các chương
  labs/                   tài liệu lab P1–P13 và capstone
labs/                     Cargo workspace của lab
  crates/lab-*/           một crate cho mỗi lab
  crates/lab-data/sql/    script PostgreSQL cho lab P12a
  crates/lab-tla/specs/   spec TLA+/PlusCal và cấu hình TLC cho lab P13
  Dockerfile, linux.sh    môi trường Linux cho các lab cần kernel Linux
tools/                    verify_citations.py, check_links.py, fix_markdown_html.py
book.toml, book-assets/   cấu hình mdBook và script Mermaid
.github/workflows/        CI: kiểm Markdown và link, build sách, deploy GitHub Pages
```

## Đóng góp

Báo lỗi hoặc gửi bản sửa qua issue / pull request. Một khẳng định vượt quá điều nguồn nói, hoặc một câu trích không đỡ được câu đứng trước, đều là lỗi của tài liệu. Khi báo, hãy kèm tên file, id chú thích và đoạn nguồn liên quan.

Khi sửa hoặc thêm chương, giữ cấu trúc ba tầng và quy ước chú thích ở trên. Trước khi gửi, chạy ba script ở mục [Kiểm tra chất lượng](#kiểm-tra-chất-lượng). Chương mới phải được thêm vào `docs/SUMMARY.md` thì mới vào sách.

## Giấy phép

Phát hành theo [giấy phép MIT](LICENSE).

Các câu trích trong chú thích thuộc bản quyền của tác giả và nhà xuất bản gốc. Chúng chỉ được dẫn ngắn để làm bằng chứng, kèm link về nguồn, và không thuộc phạm vi giấy phép MIT của repo.
