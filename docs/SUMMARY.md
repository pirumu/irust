# Tóm tắt

[Lộ trình](00-roadmap.md)
[Bảng thuật ngữ Việt–Anh](thuat-ngu.md)

# P1 — Linux và system interface

- [Tổng quan P1](01-linux/00-tong-quan.md)
  - [01. OS và kernel](01-linux/01-os-va-kernel.md)
  - [02. System call](01-linux/02-system-call.md)
  - [03. Process](01-linux/03-process.md)
  - [04. File descriptor và VFS](01-linux/04-file-descriptor-va-vfs.md)
  - [05. Signal](01-linux/05-signal.md)
  - [06. User, quyền, /proc, /sys và giới hạn tài nguyên](01-linux/06-user-quyen-proc-sys.md)
  - [07. Rust trên giao diện hệ thống](01-linux/07-rust-tren-giao-dien-he-thong.md)

# P2 — Kiến trúc CPU và assembly

- [Tổng quan P2](02-cpu/00-tong-quan.md)
  - [01. Mô hình máy và ISA](02-cpu/01-mo-hinh-may-va-isa.md)
  - [02. Biểu diễn dữ liệu](02-cpu/02-bieu-dien-du-lieu.md)
  - [03. Assembly x86-64 và AArch64](02-cpu/03-assembly-x86-64-va-aarch64.md)
  - [04. Calling convention và stack frame](02-cpu/04-calling-convention-va-stack-frame.md)
  - [05. Pipeline, branch prediction và speculative execution](02-cpu/05-pipeline-va-speculation.md)
  - [06. Cache hierarchy và NUMA](02-cpu/06-cache-va-numa.md)
  - [07. Rust và máy](02-cpu/07-rust-va-may.md)

# P3 — Bộ nhớ

- [Tổng quan P3](03-memory/00-tong-quan.md)
  - [01. Address space](03-memory/01-address-space.md)
  - [02. Address translation: từ base/bounds, segmentation đến paging](03-memory/02-address-translation.md)
  - [03. Page table nhiều cấp và TLB](03-memory/03-page-table-va-tlb.md)
  - [04. Page fault, demand paging và swapping](03-memory/04-page-fault-va-swapping.md)
  - [05. Bộ nhớ của tiến trình trên Linux](03-memory/05-bo-nho-tien-trinh-linux.md)
  - [06. Heap allocator](03-memory/06-heap-allocator.md)
  - [07. Stack, alignment và padding](03-memory/07-stack-alignment-padding.md)
  - [08. Rust và bộ nhớ](03-memory/08-rust-va-bo-nho.md)

# P4 — Concurrency

- [Tổng quan P4](04-concurrency/00-tong-quan.md)
  - [01. Thread](04-concurrency/01-thread.md)
  - [02. CPU scheduling](04-concurrency/02-scheduling.md)
  - [03. Race condition và lock](04-concurrency/03-lock.md)
  - [04. Condition variable và semaphore](04-concurrency/04-condition-variable-va-semaphore.md)
  - [05. Bug concurrency và deadlock](04-concurrency/05-concurrency-bug-va-deadlock.md)
  - [06. Memory model](04-concurrency/06-memory-model.md)
  - [07. Atomic operation và lock-free](04-concurrency/07-lock-free.md)
  - [08. Concurrency trong Rust](04-concurrency/08-rust-concurrency.md)
  - [09. Thread-local storage](04-concurrency/09-thread-local-storage.md)

# P5 — Lưu trữ và file system

- [Tổng quan P5](05-storage/00-tong-quan.md)
  - [01. Thiết bị lưu trữ](05-storage/01-thiet-bi-luu-tru.md)
  - [02. File và directory API](05-storage/02-file-va-directory-api.md)
  - [03. Cài đặt file system](05-storage/03-cai-dat-file-system.md)
  - [04. Page cache, writeback và direct I/O](05-storage/04-page-cache-va-direct-io.md)
  - [05. Crash consistency và journaling](05-storage/05-crash-consistency-va-journaling.md)
  - [06. Durability trong thực tế](05-storage/06-durability-trong-thuc-te.md)
  - [07. RAID và copy-on-write file system](05-storage/07-raid-va-copy-on-write-fs.md)
  - [08. Rust và lưu trữ](05-storage/08-rust-va-luu-tru.md)

# P6 — Networking

- [Tổng quan P6](06-networking/00-tong-quan.md)
  - [01. Layering và encapsulation](06-networking/01-layering.md)
  - [02. Link layer](06-networking/02-link-layer.md)
  - [03. IP](06-networking/03-ip.md)
  - [04. UDP và TCP cơ bản](06-networking/04-udp-va-tcp-co-ban.md)
  - [05. TCP: tin cậy và flow control](06-networking/05-tcp-reliability-va-flow-control.md)
  - [06. Congestion control](06-networking/06-congestion-control.md)
  - [07. Socket API, phía kernel và `std::net`](06-networking/07-socket-va-kernel.md)
  - [08. DNS, HTTP và TLS](06-networking/08-dns-http-tls.md)
  - [09. Zero-copy I/O: sendfile, splice, MSG_ZEROCOPY](06-networking/09-zero-copy-io.md)

# P7 — I/O models và async

- [Tổng quan P7](07-io-async/00-tong-quan.md)
  - [01. Các mô hình I/O](07-io-async/01-mo-hinh-io.md)
  - [02. select, poll và epoll](07-io-async/02-select-poll-epoll.md)
  - [03. kqueue, io_uring, readiness vs completion](07-io-async/03-kqueue-io-uring.md)
  - [04. Kiến trúc event loop](07-io-async/04-kien-truc-event-loop.md)
  - [05. Future, poll và Waker](07-io-async/05-future-poll-waker.md)
  - [06. async/await, state machine và Pin](07-io-async/06-async-await-pin.md)
  - [07. Executor, reactor và mio](07-io-async/07-executor-va-mio.md)
  - [08. Tokio](07-io-async/08-tokio.md)
  - [09. Async Rust trong production: cancellation, backpressure, blocking](07-io-async/09-async-trong-production.md)

# P8 — Container

- [Tổng quan P8](08-containers/00-tong-quan.md)
  - [01. Container là gì](08-containers/01-container-la-gi.md)
  - [02. Namespaces](08-containers/02-namespaces.md)
  - [03. cgroups v2](08-containers/03-cgroups-v2.md)
  - [04. Filesystem của container](08-containers/04-filesystem-cua-container.md)
  - [05. Bảo mật container](08-containers/05-bao-mat-container.md)
  - [06. Runtime và chuẩn OCI](08-containers/06-runtime-oci.md)
  - [07. Mạng của container](08-containers/07-mang-container.md)
  - [08. Rust trong container](08-containers/08-rust-trong-container.md)

# P9 — Rust language sâu

- [Tổng quan P9](09-rust-language/00-tong-quan.md)
  - [01. Ownership, borrowing và lifetime nhìn sâu](09-rust-language/01-ownership-borrowing-lifetime.md)
  - [02. Trait nhìn sâu](09-rust-language/02-trait.md)
  - [03. Generics nâng cao](09-rust-language/03-generics-nang-cao.md)
  - [04. Smart pointer](09-rust-language/04-smart-pointer.md)
  - [05. Error handling](09-rust-language/05-error-handling.md)
  - [06. Iterator và closure](09-rust-language/06-iterator-va-closure.md)
  - [07. Macro](09-rust-language/07-macro.md)
  - [08. Module, crate và Cargo](09-rust-language/08-module-crate-cargo.md)

# P10 — Rust internals

- [Tổng quan P10](10-rust-internals/00-tong-quan.md)
  - [01. Pipeline của rustc](10-rust-internals/01-pipeline-rustc.md)
  - [02. Type layout: size, alignment, repr, enum và niche](10-rust-internals/02-type-layout.md)
  - [03. DST, fat pointer và vtable](10-rust-internals/03-dst-fat-pointer-vtable.md)
  - [04. Monomorphization, static vs dynamic dispatch, zero-cost abstraction](10-rust-internals/04-monomorphization.md)
  - [05. Subtyping, variance và drop check](10-rust-internals/05-variance-va-dropck.md)
  - [06. Borrow checker hoạt động thế nào](10-rust-internals/06-borrow-checker.md)
  - [07. unsafe, undefined behavior và Miri](10-rust-internals/07-unsafe-ub-miri.md)
  - [08. Panic, unwinding, ABI và FFI](10-rust-internals/08-panic-abi-ffi.md)
  - [09. Bên trong collection của std: Vec, String, HashMap, BTreeMap, VecDeque](10-rust-internals/09-collections-internals.md)

# P11 — Đo đạc và gỡ lỗi

- [Tổng quan P11](11-do-dac-va-go-loi/00-tong-quan.md)
  - [01. perf và sampling profiler](11-do-dac-va-go-loi/01-perf-va-sampling-profiler.md)
  - [02. eBPF, tracepoint, kprobe/uprobe và ftrace](11-do-dac-va-go-loi/02-ebpf-va-tracing.md)
  - [03. Đo và cải thiện hiệu năng service Rust](11-do-dac-va-go-loi/03-hieu-nang-service-rust.md)
  - [04. Debugger: gdb/lldb, breakpoint và DWARF](11-do-dac-va-go-loi/04-debugger-va-dwarf.md)
  - [05. Core dump, crash và backtrace](11-do-dac-va-go-loi/05-core-dump-va-backtrace.md)
  - [06. ELF, linker và loader](11-do-dac-va-go-loi/06-elf-linker-loader.md)

# P12 — Hệ thống dữ liệu

- [Tổng quan P12](12-he-thong-du-lieu/00-tong-quan.md)
  - [01. Reliability, scalability, maintainability](12-he-thong-du-lieu/01-reliability-scalability-maintainability.md)
  - [02. Mô hình dữ liệu và ngôn ngữ truy vấn](12-he-thong-du-lieu/02-mo-hinh-du-lieu.md)
  - [03. Storage engine: log-structured vs B-tree, OLTP vs OLAP](12-he-thong-du-lieu/03-storage-engine.md)
  - [04. Encoding và tiến hóa schema](12-he-thong-du-lieu/04-encoding-va-tien-hoa-schema.md)
  - [05. Replication](12-he-thong-du-lieu/05-replication.md)
  - [06. Partitioning (sharding)](12-he-thong-du-lieu/06-partitioning.md)
  - [07. Transaction và isolation](12-he-thong-du-lieu/07-transaction.md)
  - [08. Sự cố của hệ phân tán](12-he-thong-du-lieu/08-su-co-he-phan-tan.md)
  - [09. Consistency và consensus](12-he-thong-du-lieu/09-consistency-va-consensus.md)
  - [10. Batch processing](12-he-thong-du-lieu/10-batch-processing.md)
  - [11. Stream processing: log, delivery semantics, CDC, window và watermark](12-he-thong-du-lieu/11-stream-processing.md)
  - [12. Đúng đắn và tích hợp dữ liệu](12-he-thong-du-lieu/12-dung-dan-va-tich-hop-du-lieu.md)
  - [13. Kiến trúc hệ dữ liệu: operational vs analytical, warehouse/lake, cloud vs tự vận hành](12-he-thong-du-lieu/13-kien-truc-he-du-lieu.md)
  - [14. Trách nhiệm với dữ liệu: quyền riêng tư, xóa dữ liệu, đạo đức](12-he-thong-du-lieu/14-trach-nhiem-voi-du-lieu.md)
  - [15. PostgreSQL bên trong: process, bộ nhớ, VACUUM, lock, planner](12-he-thong-du-lieu/15-postgresql-ben-trong.md)
  - [16. Redis bên trong: event loop, cấu trúc dữ liệu, persistence, replication, cluster](12-he-thong-du-lieu/16-redis-ben-trong.md)
  - [17. Redis trong thực chiến: kết hợp primitive thành pattern](12-he-thong-du-lieu/17-redis-patterns.md)

# P13 — Đặc tả hình thức với TLA+

- [Tổng quan P13](13-tla-plus/00-tong-quan.md)
  - [01. Vì sao đặc tả hình thức, và TLA+ là gì](13-tla-plus/01-vi-sao-dac-ta-hinh-thuc.md)
  - [02. Toán nền tảng: logic, tập hợp, hàm, record, sequence](13-tla-plus/02-toan-nen-tang.md)
  - [03. Trạng thái, hành vi và action](13-tla-plus/03-trang-thai-hanh-vi-action.md)
  - [04. Invariant, safety và model checker TLC](13-tla-plus/04-invariant-va-tlc.md)
  - [05. Liveness, fairness và logic thời gian](13-tla-plus/05-liveness-va-fairness.md)
  - [06. PlusCal: viết thuật toán rồi dịch sang TLA+](13-tla-plus/06-pluscal.md)
  - [07. Refinement, abstraction và implementation](13-tla-plus/07-refinement.md)
  - [08. Đặc tả giao thức phân tán: 2PC, Paxos, Raft](13-tla-plus/08-dac-ta-giao-thuc-phan-tan.md)
  - [09. Mô hình hóa hệ backend thực tế](13-tla-plus/09-mo-hinh-hoa-he-backend.md)
  - [10. Từ đặc tả đến code Rust](13-tla-plus/10-tu-dac-ta-den-code-rust.md)

# Thực hành

- [Lab P1 — Linux và system call interface](labs/01-linux-lab.md)
- [Lab P2 — CPU: asm, calling convention, branch, cache, endianness](labs/02-cpu-lab.md)
- [Lab P3 — Bộ nhớ](labs/03-memory-lab.md)
- [Lab P4 — Concurrency](labs/04-concurrency-lab.md)
- [Lab P5 — Lưu trữ](labs/05-storage-lab.md)
- [Lab P6 — Networking](labs/06-networking-lab.md)
- [Lab P7 — I/O models và async (`O_NONBLOCK`, epoll, mio, `block_on`, Tokio)](labs/07-io-async-lab.md)
- [Lab P8 — Containers từ bên trong](labs/08-containers-lab.md)
- [Lab P9 — Rust language](labs/09-rust-language-lab.md)
- [Lab P10 — Rust internals](labs/10-rust-internals-lab.md)
- [Lab P11 — Đo đạc và gỡ lỗi](labs/11-do-dac-va-go-loi-lab.md)
- [Lab P12a — Hệ thống dữ liệu: một node](labs/12a-du-lieu-mot-node-lab.md)
- [Lab P12b — Hệ thống dữ liệu: phân tán](labs/12b-du-lieu-phan-tan-lab.md)
- [Lab P13 — TLA+](labs/13-tla-plus-lab.md)
- [Capstone: HTTP server trên epoll, rồi tự viết mini async runtime](labs/capstone.md)
