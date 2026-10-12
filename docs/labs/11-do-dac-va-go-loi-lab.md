# Lab P11 — Đo đạc và gỡ lỗi

> Đây là phần **thực hành** của [P11 · Đo đạc và gỡ lỗi](../11-do-dac-va-go-loi/00-tong-quan.md). Hãy đọc xong lý thuyết trước rồi mới làm lab: lab chỉ minh họa và kiểm chứng những gì các chương P11 đã nêu, và mỗi lab ghi rõ chương và mục cần đọc. Mọi fact có chú thích cuối trang, kiểm bằng `python3 tools/verify_citations.py docs/labs/11-do-dac-va-go-loi-lab.md`.
>
> **Quy ước về output.** Output trong khối `text` là output **thật** của các lệnh `cargo build`, `cargo test`, `cargo run` (và của chính các binary lab) đã chạy trên máy người soạn: macOS aarch64, rustc 1.95.0. Giá trị phụ thuộc máy (thời gian, địa chỉ, pid) được ghi chú. Mọi thứ cần Linux hoặc công cụ ngoài — `perf`, `bpftrace`, `strace`, `gdb`, `lldb`, `readelf`, core dump — người soạn **chưa chạy** (chỉ được phép chạy `cargo`): các lệnh đó được viết sẵn cho bạn, đánh dấu **chưa chạy**, kèm *điều cần tìm* được suy ra từ nguồn trích dẫn và từ con số mà chính chương trình in ra. Đó là kỳ vọng, không phải kết quả đã quan sát.

## Chuẩn bị và các bài lab

Crate: `labs/crates/lab-observability`. Dependency (ghim phiên bản): `hdrhistogram` 7.5.4 (tắt feature mặc định), `tracing` 0.1.44, `tracing-subscriber` 0.3.23 (chỉ feature `registry`), `tokio` 1.53.2 (chỉ feature `rt`), `parking_lot` 0.12.5 và `libc` 0.2 (đồng hồ CPU của thread, `getrlimit`, `mmap`). Thư viện `src/lib.rs` chứa phần kiểm thử được mà không cần profiler hay debugger (đếm cấp phát, CRC, vòng ghi, workload latency, `Layer` của `tracing`, đọc file ELF, "vì sao không có core"); mỗi lab là một binary trong `src/bin/`.

| Lab | Bin | Chạy trên macOS | Phần cần Linux (**chưa chạy**) | Chương |
| --- | --- | --- | --- | --- |
| (a) Điểm nóng và lấy mẫu | `hotspot` | chạy chương trình, đọc số tự đo | `perf stat`, `perf record`, `perf report`, flame graph | [01](../11-do-dac-va-go-loi/01-perf-va-sampling-profiler.md) |
| (b) Syscall và cấp phát | `trace_target` | chạy chương trình, đọc số tự đếm | `strace`, `perf trace`, `bpftrace`, uprobe, ftrace | [02](../11-do-dac-va-go-loi/02-ebpf-va-tracing.md) |
| (c) Percentile của service | `latency` | chạy đủ, mọi số liệu thật | profile bằng `perf`, off-CPU | [03](../11-do-dac-va-go-loi/03-hieu-nang-service-rust.md) |
| (d) Span và `Span::enter` qua `.await` | `spans` | chạy đủ, output xác định | — | [03](../11-do-dac-va-go-loi/03-hieu-nang-service-rust.md) |
| (e) Debugger và DWARF | `debug_target`, `elf_probe` | chạy chương trình; đọc section của file ELF Linux có sẵn | `gdb`/`rust-gdb`, `lldb`/`rust-lldb`, `readelf --debug-dump` | [04](../11-do-dac-va-go-loi/04-debugger-va-dwarf.md) |
| (f) Crash, backtrace, core | `crash_lab` | chạy hầu hết các case | `ulimit -c`, `core_pattern`, đọc core bằng `gdb`, `gcore` | [05](../11-do-dac-va-go-loi/05-core-dump-va-backtrace.md) |
| (g) ELF, loader, ASLR | `elf_probe`, `aslr_probe` | đọc ELF Linux có sẵn; ASLR của macOS | `readelf`, `ldd`, `LD_DEBUG`, `setarch -R` | [06](../11-do-dac-va-go-loi/06-elf-linker-loader.md) |

Build và test (từ thư mục `labs/`). Phiên bản toolchain và kết quả `cargo test`:

```bash
cargo --version && rustc --version
cd labs && cargo test -q -p lab-observability --lib
```

```text
cargo 1.95.0 (f2d3ce0bd 2026-03-21)
rustc 1.95.0 (59807616e 2026-04-14)
running 47 tests
test result: ok. 47 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
```

`cargo build -p lab-observability --all-targets` và `cargo clippy -p lab-observability --all-targets` đều không có cảnh báo (người soạn đã chạy cả hai). Các test kiểm tra hành vi thật chứ không kiểm tra "đã nối dây": CRC tra bảng so với bản bit-từng-bit và với giá trị kiểm chuẩn đã công bố, histogram so với mảng sắp xếp, hai biến thể của service cho cùng đáp án, bản "tuned" không cấp phát, parser ELF không panic với file bị cắt hoặc sửa byte, transcript của `Span::enter` qua `.await` được ghim từng dòng, bảng quyết định "có core hay không".

**Đo trên máy bận.** Các bin lab in `load1`, tải trung bình 1 phút: số tiến trình đang chạy hoặc đang chờ CPU, tính trung bình[^loadavg]. Khi `load1` gần hoặc vượt số core, thời gian tường (wall) của một đoạn code CPU-bound gồm cả lúc nó chờ được lên CPU, nên số wall không còn là "chi phí của đoạn code". Vì vậy lab (a) và (c) đo thêm thời gian CPU của chính thread — `clock_gettime` với `CLOCK_THREAD_CPUTIME_ID`, đồng hồ đo thời gian CPU mà thread này tiêu thụ[^clock-thread]. Máy người soạn có 11 core và đang chạy các tác vụ khác, `load1` ghi ngay trong từng output; đây là lý do mọi so sánh dưới đây là so sánh *trong cùng một lần chạy* hoặc có nêu rõ lần chạy khác.

### Container Linux: có gì, thiếu gì

`labs/Dockerfile` lấy image `rust:1.95-bookworm`, `apt-get install` `strace gdb procps psmisc iproute2 iputils-ping dnsutils netcat-openbsd tcpdump curl less man-db manpages manpages-dev`, rồi `rustup component add clippy`. `labs/linux.sh` chạy container với `--cap-add SYS_PTRACE --cap-add NET_ADMIN --cap-add NET_RAW --security-opt seccomp=unconfined`. Đối chiếu với những gì P11 cần:

| Công cụ | Lab | Trong image? | Cần thêm |
| --- | --- | --- | --- |
| `strace` | (b) | có (Dockerfile) | `SYS_PTRACE` đã có trong `linux.sh`; `strace -c` đếm lần gọi từng syscall[^strace-c] |
| `gdb` | (e), (f) | có (Dockerfile) | `rust-gdb` là script đi kèm toolchain. Người soạn đọc danh sách file của component `rustc` trong toolchain macOS và thấy `bin/rust-gdb` cùng các script pretty-printer; image cài toolchain bằng `rustup-init --profile minimal`[^rustup-minimal], profile gồm rustc, rust-std và cargo[^rustup-profile-min]. Vì vậy *nhiều khả năng* `rust-gdb` có sẵn — chưa kiểm trong container (`command -v rust-gdb`) |
| `readelf`, `nm`, `objdump` | (e), (g) | Dockerfile không cài; image gốc dựa trên `buildpack-deps:bookworm`[^docker-rust-base] | `readelf` hiển thị thông tin file ELF[^readelf-desc], `nm` liệt kê symbol[^nm-desc]; thuộc gói `binutils`. Người soạn chưa kiểm xem image đã kéo gói này chưa (`command -v readelf nm objdump`) |
| `perf` | (a), (b), (c) | không | gói `linux-perf` (nguồn của man page `perf(1)` ở Debian bookworm[^perf-desc]); quyền ở dưới |
| `bpftrace` | (b) | không | gói `bpftrace`, bản trong bookworm là 0.17.0[^bt-ver]; cần `CAP_BPF`/`CAP_PERFMON` (từ Linux 5.8)[^cap-bpf-since][^cap-perfmon-since] hoặc `--privileged`[^docker-privileged] |
| `lldb`, `llvm-dwarfdump` | (e) | không | tùy chọn: `llvm-dwarfdump` dump và kiểm tra DWARF[^llvm-dwarfdump]; tên gói Debian là `lldb` và `llvm` (người soạn chưa chạy `apt-get`) |
| `coredumpctl` | (f) | không | thuộc `systemd-coredump`[^sd-svc], chỉ có nghĩa khi máy chạy systemd; lab dùng `ulimit` và `core_pattern` dạng file |

Đề xuất của người soạn (không sửa `Dockerfile`, chỉ để bạn tự làm trong một phiên shell; **chưa chạy**): vào shell trong container rồi cài thêm, và nếu cần `bpftrace` thì chạy lại container với `--privileged` (bản sao của `linux.sh` có thêm cờ đó; `--cap-add` cho phép thêm từng capability[^docker-capadd]):

```bash
./labs/linux.sh                                              # shell tương tác trong container
apt-get update && apt-get install -y linux-perf bpftrace lldb llvm   # chưa chạy; tên gói theo Debian bookworm

# chỉ khi cần bpftrace/ftrace: cùng image, thêm --privileged (chưa chạy)
docker run --rm -it --privileged -v "$PWD":/work -v rust-foundations-cargo:/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR=/work/labs/target-linux -w /work/labs rust-foundations-lab:1.95 bash
```

Quyền của `perf` trong container, theo thứ tự bạn sẽ gặp: seccomp mặc định của Docker chặn `perf_event_open`[^dock-block] nhưng `linux.sh` chạy với `seccomp=unconfined`[^dock-unconf]; rồi đến `kernel.perf_event_paranoid`, mặc định 2, chỉ cho đo user space của chính process[^ksys-default][^ksec-level2]; rồi đến phần cứng: Docker Desktop chạy container trong một VM Linux[^docker-vmm], và event cần tính năng CPU mà môi trường không có sẽ trả `ENODEV` hoặc `EOPNOTSUPP`[^peo-enodev][^peo-eopnotsupp]. Nếu `perf stat`/`perf record` báo lỗi với event mặc định, thử event phần mềm như `cpu-clock`[^peo-cpuclock], vốn có cả khi không có hỗ trợ phần cứng[^peo-sw] (chưa chạy).

### Lab (a) — `hotspot`: hàm nóng, hàm lạnh và thời gian chờ

Lý thuyết: [chương 01 mục 1](../11-do-dac-va-go-loi/01-perf-va-sampling-profiler.md) (đo trước khi tối ưu), [mục 4](../11-do-dac-va-go-loi/01-perf-va-sampling-profiler.md) (`perf stat` so với `perf record`, tần số, skid), [mục 5](../11-do-dac-va-go-loi/01-perf-va-sampling-profiler.md) (unwinding `fp`/`dwarf`/`lbr`, `Self` và `Children`), [mục 6](../11-do-dac-va-go-loi/01-perf-va-sampling-profiler.md) (symbol, debuginfo, demangle), [mục 7](../11-do-dac-va-go-loi/01-perf-va-sampling-profiler.md) (on-CPU và off-CPU, flame graph), [mục "Liên hệ với Rust"](../11-do-dac-va-go-loi/01-perf-va-sampling-profiler.md).

`hotspot` là chương trình CPU-bound có hàm nóng biết trước. Chuỗi gọi của đường nóng là `main → run_round → process_batch → hot_checksum`: `hot_checksum` là vòng CRC-32 tra bảng trên buffer 64 KiB, gọi 120 lần mỗi vòng. `warm_format` tạo 15.000 chuỗi bằng `format!` mỗi vòng (cấp phát, nhẹ hơn hẳn); các hàm `cold_*` dựng bảng, đổ buffer và cứ 8 vòng đối chiếu bảng với bản tham chiếu bit-từng-bit trên 4 KiB. Cuối mỗi vòng thread `sleep` 10 ms: bị chặn, không dùng CPU. Mọi hàm đều `#[inline(never)]`, thuộc tính gợi ý rằng không nên inline[^inline-never]; nhưng mọi dạng của attribute `inline` chỉ là gợi ý và compiler được phép bỏ qua[^inline-hint], nên bài tập 1 yêu cầu bạn kiểm xem symbol còn trong binary release hay không. Chương trình tự đo từng pha bằng đồng hồ tường và bằng đồng hồ CPU của thread, và in số đơn vị công việc (byte đã băm, chuỗi đã tạo): đó là đáp án mà một profiler lấy mẫu phải tái hiện — nhưng chỉ cho phần chạy trên CPU.

```bash
cd labs && cargo run -q --release -p lab-observability --bin hotspot -- 30 10   # 30 vòng, ngủ 10 ms mỗi vòng
```

Output (macOS aarch64; `pid`, thời gian và `load1` phụ thuộc máy, `work` và `crc` thì không):

```text
hotspot: rounds=30 hot_calls/round=120 buffer=65536 B warm_items/round=15000 sleep=10 ms/round pid=52041
machine: cores=11 load1=7.02 (wall time includes time spent waiting for a CPU when load1 is near or above cores)
phase                       wall_ms     cpu_ms             work
cold (setup + validate)         0.2        0.2            65796
hot_checksum                  596.1      589.1        235929600
warm_format                    50.1       48.6           450000
sleep (blocked)               362.8        0.4               30
total                        1009.3      638.3
share of wall: hot 59.1%  warm 5.0%  cold 0.0%  sleep 35.9%
share of cpu:  hot 92.3%  warm 7.6%  cold 0.0%  sleep 0.1%
elapsed 1009.3 ms (main loop), cpu/wall = 0.63
crc=0x88a1f313 warm=40518480
```

Giải thích:

1. **`work` và `crc` xác định.** 235.929.600 byte = 30 vòng × 120 lần × 65.536 B. `crc=0x88a1f313` là CRC chạy nối tiếp qua mọi lần gọi; nó giống nhau ở mọi lần chạy người soạn thử và không phụ thuộc thời gian. Kỳ vọng (chưa chạy): trên Linux cũng ra đúng `0x88a1f313` vì chỉ là số học nguyên (nhận xét của người soạn).
2. **Wall và CPU tách nhau ở pha `sleep`.** Pha ngủ chiếm 35,9% thời gian tường nhưng 0,1% thời gian CPU (0,4 ms trên 362,8 ms). Đó là thời gian off-CPU: thread chờ khi bị chặn, ví dụ vì timer[^off-def]. Sampling cài bằng ngắt timer theo từng CPU chỉ xem process nào đang bị ngắt, nên chỉ cho profile on-CPU[^off-timer]; off-CPU analysis bổ sung để hiểu 100% thời gian của thread[^off-comp]. Hệ quả cho dự đoán: tỉ lệ mẫu của `perf record` phải đi theo cột `cpu_ms` chứ không theo cột `wall_ms`.
3. **Dự đoán cho `perf` (chưa chạy).** Trong `perf report --no-children`, `hot_checksum` có `Self` khoảng 92% (phần CPU của pha hot ở trên), `warm_format` cùng `malloc`/`free` và hàm định dạng của std cộng lại khoảng 8%, các hàm `cold_*` gần như không thấy, và **không có dòng nào cho `sleep`**. Con số "phần trăm mẫu" là cột `overhead`[^rep-overhead]; vì là thống kê nên số mẫu ít thì nhiễu lớn (suy luận của chương 01). `perf stat` sẽ cho `task-clock` cỡ tổng `cpu_ms` (≈ 638 ms) và "CPUs utilized" cỡ `cpu/wall` = 0,63 — ví dụ output trong man page có đúng hai thành phần đó[^stat-taskclock].
4. **Khi máy bận.** Output ở trên có `load1=7,02` trên 11 core nên wall của pha hot (596,1 ms) xấp xỉ CPU của nó (589,1 ms). Ở một lần chạy trước đó trong cùng phiên, lúc `load1` ≈ 12,5, người soạn thấy wall của pha hot là 1659,9 ms so với CPU 654,4 ms: gần 1 giây chờ được lên CPU. Số CPU gần như không đổi, số wall thì gấp 2,5 lần.

**Phần Linux (chưa chạy): `perf stat`, `perf record`, `perf report`.** Build "để profile đọc được" mà không sửa `Cargo.toml`: profile của Cargo ghi đè được bằng biến môi trường `CARGO_PROFILE_<tên>_<khóa>`[^env-debug], nên không cần thêm profile vào workspace.

```bash
./labs/linux.sh bash -c '
  export CARGO_PROFILE_RELEASE_DEBUG=line-tables-only      # có file:line, không có biến/tham số
  export RUSTFLAGS="-C force-frame-pointers=yes"            # để --call-graph fp đi được
  cargo build -q -p lab-observability --release --bin hotspot
  perf stat -e task-clock,context-switches,page-faults ./target-linux/release/hotspot
  perf record -F 99 -g -o /tmp/hotspot.fp.data -- ./target-linux/release/hotspot
  perf report -i /tmp/hotspot.fp.data --no-children --stdio | head -40
'                                                            # chưa chạy
```

Điều cần tìm, và vì sao build như vậy:

- **Build.** `debug = "line-tables-only"` cho số dòng ở mức tối thiểu[^pb-debugline][^cg-ltonly]; `perf report -s srcline` cần DWARF[^rep-srcline]. `-C force-frame-pointers=yes` ép compiler giữ frame pointer[^pb-fp-flag][^cg-fp-flag]: Rust có thể bỏ nó và làm xấu stack trace khi profile[^pb-fp], mà `perf record` mặc định dùng `fp` cho user space[^rec-cg-default] và sẽ cho call graph "giả" với binary không có frame pointer[^rec-fomit]. Hai cờ này đổi code sinh ra (frame pointer) hoặc chỉ thêm thông tin tra cứu (line tables), nên bản build này không giống hệt production (nhận xét của chương 01).
- **Tần số và đối tượng.** `-F 99` đặt tần số lấy mẫu[^rec-freq]; con số 99 chỉ là ví dụ quen thuộc, không phải quy định của man page. `-g` bật ghi call graph cho kernel và user space[^rec-g].
- **Đọc bảng.** Mặc định `perf report` gộp theo `comm`, `dso`, `symbol`[^rep-default-sort]. `--no-children` tắt cột `Children`, vốn cộng cả chi phí của hàm con và có thể làm tổng vượt 100%[^rep-nochildren][^rep-sum]; `Self` là chi phí của chính hàm[^rep-self]. Hãy thử cả hai: `main` sẽ có `Children` cỡ 100% và `Self` gần 0, theo đúng định nghĩa[^rep-children].
- **Tên hàm.** Nếu thấy `_ZN…` hoặc `_R…` thay cho `hotspot::hot_checksum`, đó là tên đã mangle mà profiler không biết giải[^pb-mangle]; có thể đổi sang định dạng v0[^pb-v0].
- **Hai cách unwinding khác.** Chạy lại với `--call-graph dwarf` (mặc định chép 8192 byte stack mỗi mẫu[^rec-dwarf]; suy luận của người soạn: `perf.data` phình theo) và so các caller của `hot_checksum` (`process_batch`, `run_round`, `main`, theo đúng chuỗi gọi ở trên) giữa ba cấu hình: không có frame pointer + `fp`, có frame pointer + `fp`, `dwarf`. Ghi lại cấu hình nào đứt stack: đó là phép đo của bạn, người soạn không dự đoán kết quả. `lbr` chỉ có trên một số CPU Intel[^rec-lbr] nên không thể có trong VM trên máy Apple Silicon (suy luận của người soạn).
- **Flame graph.** `perf script report flamegraph`[^flame-perf] hoặc `cargo flamegraph`[^pb-list] cho cùng thông tin dạng đồ thị: ô rộng nghĩa là xuất hiện nhiều trong các stack, trục ngang không phải thời gian[^flame-sum]. Kỳ vọng: một "cao nguyên" `hot_checksum` nằm trên `process_batch`, `warm_format` hẹp hơn nhiều, và `sleep` vắng mặt.
- **Không strip.** Build lại với `CARGO_PROFILE_RELEASE_STRIP=symbols`[^env-strip]: rustc cảnh báo `strip = "symbols"` có thể làm trace không đọc nổi trên nền tảng dựa vào bảng symbol[^cg-strip-sym], tức `perf report` chỉ còn địa chỉ. Lab (e) cho thấy `.symtab` biến mất khỏi một binary đã strip.
- **Skid.** Mẫu có thể rơi sau lệnh gây ra event vài lệnh[^peo-skid]: đọc kết quả ở mức hàm thì ổn, đọc ở mức dòng hay lệnh phải nhớ điều này.

### Lab (b) — `trace_target`: syscall và cấp phát, nhìn từ trong và từ ngoài

Lý thuyết: [chương 02 mục 1](../11-do-dac-va-go-loi/02-ebpf-va-tracing.md) (tracing và sampling), [mục 3](../11-do-dac-va-go-loi/02-ebpf-va-tracing.md) (tracepoint, kprobe, uprobe), [mục 5](../11-do-dac-va-go-loi/02-ebpf-va-tracing.md) (đọc one-liner bpftrace), [mục 7](../11-do-dac-va-go-loi/02-ebpf-va-tracing.md) (chi phí), [mục 9.1](../11-do-dac-va-go-loi/02-ebpf-va-tracing.md) (uprobe trên symbol Rust), [chương 06 mục 1](../11-do-dac-va-go-loi/06-elf-linker-loader.md) (segment và offset trong file).

`trace_target` ghi `requests` dòng văn bản vào `/dev/null` theo một trong hai cách và **tự đếm** những gì nó làm: số lần `write` xuống file bên dưới, và số lần gọi allocator (qua một `GlobalAlloc` đếm theo thread[^ga-desc], mã ở `lab_observability::alloc`). `File` không đệm: std khuyên bọc nó trong `BufWriter` khi có nhiều lần ghi nhỏ[^rs-file-nobuf], nên với `File` trần, mỗi `write_all` của một dòng ngắn là một `write(2)` (suy luận của người soạn: file thường, ghi nhỏ nên không có lần ghi dở). Hai con số tự đếm là đáp án để đối chiếu với những gì một tracer thấy *từ bên ngoài*.

- `unbuffered`: mỗi request `format!` một dòng rồi `write_all` thẳng lên `File`.
- `buffered`: dòng được ghi vào một `String` dùng lại (`clear()` rồi ghi tiếp), đi qua `BufWriter`, `flush` một lần ở cuối.
- Cả hai gọi `handle_request(id)`: một hàm `extern "C"` có tên cố định `#[unsafe(no_mangle)]` và `#[inline(never)]`, mỗi lần cấp phát một chuỗi. Nó là "điểm móc" cho uprobe ở phần Linux.

```bash
cd labs
cargo run -q --release -p lab-observability --bin trace_target -- unbuffered 20000
cargo run -q --release -p lab-observability --bin trace_target -- buffered 20000
```

Output (macOS aarch64; `pid` và `elapsed_ms` phụ thuộc máy, các số đếm thì không):

```text
trace_target: pid=52073 sink=/dev/null handle_request is an unmangled symbol
mode=unbuffered requests=20000 bytes=168890 write_calls=20000 allocs=40000 reallocs=10000 frees=40000 elapsed_ms=13.32
trace_target: pid=52074 sink=/dev/null handle_request is an unmangled symbol
mode=buffered requests=20000 bytes=168890 write_calls=21 allocs=20002 reallocs=0 frees=20002 elapsed_ms=1.62
```

Giải thích:

1. **Cùng 168.890 byte, 20.000 lần ghi so với 21 lần.** `unbuffered` có `write_calls=20000`: một lần mỗi request. `buffered` đi qua `BufWriter` dung lượng mặc định 8 KiB[^bufwriter-8k]: 168.890 / 8.192 ≈ 20,6 nên khoảng 21 lần ghi lớn là hợp lý, và đó đúng là việc `BufWriter` làm: gom ghi nhỏ thành ít lần ghi lớn[^bufwriter-batch]. Thời gian (13,32 ms so với 1,62 ms) chỉ để tham khảo: ghi vào `/dev/null` không chạm đĩa nên số này đánh giá thấp chi phí trên file hoặc socket thật.
2. **Cấp phát.** `unbuffered` có `allocs=40000`: hai lần mỗi request, một ở `handle_request`, một ở vòng ghi. `reallocs` đếm riêng các lần gọi `realloc`, vì `GlobalAlloc` có hàm riêng cho nó[^ga-realloc]; con số 10.000 bằng đúng số request có `id ≥ 10000`. Suy luận của người soạn (chưa tra nguồn): dòng của các id đó dài từ 9 byte, vượt dung lượng ban đầu 8 byte của một `String` rỗng nên phải `realloc`. `buffered` có `allocs=20002` — 20.000 của `handle_request` cộng hai buffer dựng một lần (`BufWriter` và `String::with_capacity(64)`) — và `reallocs=0`: dung lượng đặt trước và tái dùng bằng `clear()` loại bỏ cấp phát lặp[^pb-withcap].
3. **Đây là quan sát, không phải hợp đồng.** Docs của `GlobalAlloc` dặn không được dựa vào việc allocation có thật sự xảy ra, vì optimizer có thể bỏ hoặc chuyển nó[^ga-notrely]; vì vậy hãy xem `allocs` là số đếm trên *bản build này*, và so sánh cùng cấu hình. Về thiết kế bộ đếm: docs cảnh báo rất dễ tạo allocator đệ quy vô hạn vì nhiều cấu trúc của std tự cấp phát[^ga-reent], ví dụ `std::sync::Mutex` trên một số nền tảng[^ga-mutex]. Vì thế bộ đếm không khóa gì: nó chỉ tăng một số nguyên trong `thread_local!` khởi tạo `const` và không có destructor, thứ mà người soạn *giả định* không cấp phát (không có nguồn riêng; chương trình chạy bình thường nên chưa thấy dấu hiệu tái nhập).

**Phần Linux (chưa chạy): nhìn từ bên ngoài.** Vì `trace_target` đã in số đúng, mọi lệnh dưới đây đều có đáp án kiểm được. Chạy trong container (cài `perf`, `bpftrace` như ở phần chuẩn bị). Đường dẫn trong container: repo gắn ở `/work`, binary nằm ở `/work/labs/target-linux/release/trace_target`.

```bash
cd /work/labs && cargo build -q -p lab-observability --release --bin trace_target
T=/work/labs/target-linux/release/trace_target

# 1. Đếm syscall: strace -c (summary), -f (theo cả thread), chỉ lọc write
strace -f -c -e trace=write $T unbuffered 20000 2>&1 | tail -8
strace -f -c -e trace=write $T buffered   20000 2>&1 | tail -8

# 2. Cùng việc bằng perf trace: tóm tắt syscall theo thread
perf trace -s $T unbuffered 20000 2>&1 | tail -15

# 3. bpftrace: xem tên trường trước, rồi đếm và vẽ histogram kích thước
bpftrace -l 'tracepoint:syscalls:sys_enter_write' -v
bpftrace -e 'tracepoint:syscalls:sys_enter_write /pid == cpid/ { @calls = count(); @bytes = hist(args->count); }' \
  -c "$T unbuffered 20000"

# 4. uprobe ngay trên hàm Rust có tên cố định
bpftrace -e 'uprobe:/work/labs/target-linux/release/trace_target:handle_request /pid == cpid/ { @calls = count(); @id = hist(arg0); }' \
  -c "$T unbuffered 20000"

# 5. Cấp phát nhìn từ libc: malloc/realloc/free
bpftrace -e 'uprobe:libc:malloc /pid == cpid/ { @malloc = count(); }
             uprobe:libc:realloc /pid == cpid/ { @realloc = count(); }
             uprobe:libc:free /pid == cpid/ { @free = count(); }' -c "$T unbuffered 20000"
```

(Tất cả: **chưa chạy**.) Điều cần tìm, kèm cơ sở:

- **`strace -c`** đếm số lần gọi, thời gian và lỗi của từng syscall rồi in tóm tắt khi chương trình kết thúc[^strace-c]; `-f` theo cả process con và thread[^strace-f]; `-e trace=write` chỉ giữ syscall `write`[^strace-trace]. Kỳ vọng: dòng `write` có số `calls` bằng `write_calls` của chương trình cộng vài lần ghi của chính nó (một dòng `pid=` ra stderr và dòng kết quả ra stdout): khoảng 20.002 với `unbuffered` và khoảng 23 với `buffered`. Lệch vài đơn vị là bình thường; lệch hàng nghìn nghĩa là bạn đang đo thứ khác.
- **`perf trace -s`** là công cụ lấy cảm hứng từ `strace`, hiện các event của target, đầu tiên là syscall[^perftrace-desc], và `-s` chỉ in tóm tắt syscall theo thread kèm min, max, trung bình[^perftrace-s]. Kỳ vọng cùng số `write` như `strace`, với overhead khác.
- **Đọc one-liner số 3** theo khung của chương 02: probe `tracepoint:syscalls:sys_enter_write` (điểm tracing tĩnh tại lúc syscall được *vào*, như ví dụ `openat` của tutorial bpftrace[^bttut-openat]), predicate `/pid == cpid/` (tùy chọn[^bt-predicate]; `cpid` là pid của process con và chỉ có giá trị với cờ `-c`[^cpid], cờ chạy một lệnh con rồi dừng khi lệnh đó kết thúc[^bt-c-usdt]), action `{ ... }`. `@calls = count()` và `@bytes = hist(...)` là map toàn cục[^bt-map-var]: tất cả map được in khi chương trình thoát[^btref-print-maps]; `count()` dùng map per-CPU nên nhanh[^bt-count-percpu]; `hist` tạo histogram log2[^bt-hist]. Tên trường `args->count` là điều người soạn *giả định*: hãy liệt kê trường bằng `bpftrace -l ... -v`[^bt-list-v] và sửa nếu khác. Kỳ vọng: `@calls` cùng cỡ với `strace`; `@bytes` của `unbuffered` dồn vào vài bucket nhỏ nhất (mỗi dòng 5–9 byte; trung bình 168.890 / 20.000 ≈ 8,4 byte), còn của `buffered` chỉ có vài chục lần ghi cỡ 8 KiB.
- **Đọc one-liner số 4 (uprobe).** `uprobe:binary:func` gắn vào một hàm của binary bằng tên[^bt-uprobe-variants]. Tên đó phải còn đúng và còn tồn tại: `#[unsafe(no_mangle)]` tắt mangling nên symbol chính là tên hàm[^ref-nomangle] (từ edition 2024 phải viết `unsafe(...)`[^ref-nomangle-2024], vì symbol không mangle có thể đụng tên khác[^ref-nomangle-unsafe]); `#[inline(never)]` để hàm không bị inline hết — rustc tự inline khi thấy đáng làm[^ref-inline-auto] — và bản release mặc định còn bảng symbol (lab (e) cho thấy `.symtab` còn nguyên ở bản release của một crate khác), trừ khi bạn đặt `strip = "symbols"`[^cg-strip-sym]. `arg0` lấy từ thanh ghi CPU[^bt-arg-regs] — `extern "C"` làm rõ quy ước gọi để `arg0` đúng là `id` — còn kiểu thì bpftrace không biết, bạn tự chịu trách nhiệm[^bt-no-signature]. Kỳ vọng: `@calls` đúng 20.000 ở *cả hai* chế độ, vì `handle_request` chạy một lần mỗi request dù có đệm hay không. Với uretprobe (đo latency một hàm), nguồn chương 02 chỉ có ví dụ Go bị ghi đè địa chỉ trả về gây lỗi[^bt-uretprobe-go]; với Rust hãy tự kiểm (bài tập 4).
- **Đọc one-liner số 5 (malloc).** `uprobe:libc:malloc` lấy tên thư viện thay cho đường dẫn đầy đủ[^bt-uprobe-lib]. Trong std 1.95.0, `System::alloc` gọi `libc::malloc`[^rust-sys-malloc], `realloc` gọi `libc::realloc`[^rust-sys-realloc] và `dealloc` gọi `libc::free`[^rust-sys-free] (chi tiết cài đặt, có thể đổi), và `CountingAlloc` của lab chuyển thẳng mọi lời gọi cho `System`. Kỳ vọng với `unbuffered`: `@malloc` ≈ 40.000, `@realloc` ≈ 10.000, `@free` ≈ 40.000, cộng một số ít (vài chục) cấp phát của runtime lúc khởi động. Ba con số này phải khớp với `allocs`, `reallocs`, `frees` mà chương trình tự đếm — nếu lệch nhiều, nghi ngờ trước hết là probe gắn nhầm tiến trình hoặc tên hàm.
- **Offset cho uprobe.** Nếu bạn chỉ có địa chỉ, uprobe event của kernel đòi offset tính trong object: cú pháp `p[:[GRP/][EVENT]] PATH:OFFSET`[^uprobe-syntax], và người dùng tự tính offset[^uprobe-offset]; ví dụ trong tài liệu kernel: `zfree` ở `0x446420` trong `/bin/zsh` nạp tại `0x00400000` có offset `0x46420`[^uprobe-zsh-ex]. bpftrace cũng nhận `uprobe:binary:offset`[^bt-uprobe-variants]. `elf_probe sym` ở lab (g) đổi `st_value` thành offset trong file qua segment `PT_LOAD` chứa nó (mô tả `PT_LOAD` ở [^elf-load]); đó là suy luận của người soạn từ hai nguồn trên, hãy đối chiếu với `readelf -lW` và `nm`.
- **Chi phí.** Chương 02 không có số đo uprobe. Chạy `trace_target unbuffered 200000` có và không có `bpftrace`/`strace` gắn vào, so `elapsed_ms` in ra: đó là cách đo chi phí gắn probe lên một hàm gọi 200.000 lần (bài tập 4).

### Lab (c) — `latency`: percentile, cấp phát và khóa thô

Lý thuyết: [chương 03 mục 1](../11-do-dac-va-go-loi/03-hieu-nang-service-rust.md) (benchmark: warm-up, nhiễu, `black_box`), [mục 2](../11-do-dac-va-go-loi/03-hieu-nang-service-rust.md) (percentile, `hdrhistogram`), [mục 3](../11-do-dac-va-go-loi/03-hieu-nang-service-rust.md) (cấu hình build), [mục 6](../11-do-dac-va-go-loi/03-hieu-nang-service-rust.md) (cấp phát, `GlobalAlloc`), [mục 8](../11-do-dac-va-go-loi/03-hieu-nang-service-rust.md) (latency đuôi).

`latency` là một service giả lập chạy trong process: bảng 1.024 khóa sau một khóa, và mỗi request tra bảng rồi dựng một phản hồi JSON nhỏ. Cứ 200 request có một "report" cần đốt khoảng 170 µs CPU. Bốn thread phát request liên tục (mỗi thread gửi request kế tiếp ngay khi xong request trước) và ghi latency từng request vào một `hdrhistogram::Histogram` với 3 chữ số có nghĩa[^hdr-precision]; percentile lấy bằng `value_at_quantile` với quantile trong [0, 1][^hdr-quantile][^hdr-quant-pct]. Hai biến thể phục vụ *cùng* các request:

- `naive`: một khóa thô giữ suốt request, **đốt CPU của report trong khi giữ khóa**, copy giá trị bằng `to_string()` và dựng phản hồi bằng `format!` (hai cấp phát mỗi request).
- `tuned`: khóa chỉ giữ cho lần tra bảng, report chạy ngoài khóa, giá trị là `Arc<str>` (clone không cấp phát), phản hồi ghi vào buffer đã đặt trước dung lượng.

Hai biến thể trả cùng đáp án (cột `checksum` bằng nhau, và test so từng id), nên chỉ khác *cách* làm. Khóa là `parking_lot::Mutex`: không có poisoning[^pl-nopoison] và dùng "eventual fairness" để khóa công bằng trung bình mà không hy sinh throughput[^pl-fair]. Mỗi thread có 5.000 request khởi động không ghi nhận (làm nóng cache, cấp phát, trang bộ nhớ; chương 03 nói warm-up làm nóng cache của bộ xử lý[^crit-warm]) với id lấy từ dải mà các request đo không dùng, và id đưa qua `black_box` để compiler không tối ưu theo giá trị biết trước[^bb-inputs].

```bash
cd labs
cargo run -q --release -p lab-observability --bin latency                                  # 4 thread, 100.000 request mỗi thread
cargo run -q --release -p lab-observability --bin latency -- both 1 400000                 # cùng tổng số request, 1 thread
cargo run -q -p lab-observability --bin latency -- both 4 20000                            # profile dev (không tối ưu)
CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_TARGET_DIR=/tmp/obs-lto \
  cargo run -q --release -p lab-observability --bin latency                                # LTO "fat" + 1 codegen unit
```

Output chạy mặc định (macOS aarch64, đơn vị µs; `load1` ghi trong output):

```text
latency: threads=4 requests/thread=100000 warmup/thread=5000 heavy_every=200 heavy_iters=100000 (one report needs ~170 us of CPU) build=release
machine: cores=11 load1=7.02
variant   requests   wall_ms      req/s   mean_us    p50_us    p99_us  p99.9_us     max_us  allocs/req  checksum
naive       400000     530.3     754260      4.83      0.21    188.54    410.11    1327.10       2.005  0x27d92591c8d6871b
tuned       400000     378.1    1057959      1.38      0.33      8.08    175.23     308.22       0.000  0x27d92591c8d6871b
```

Giải thích:

1. **Trung bình đẹp, đuôi xấu.** Với `naive`, trung bình là 4,83 µs còn p99 là 188,54 µs, gấp 39 lần; với `tuned` p99 chỉ 8,08 µs. p50 thì cùng cỡ (0,21 và 0,33 µs; ở các lần chạy 4 thread khác của người soạn, p50 của cả hai biến thể nằm trong khoảng 0,21–0,33 µs và không biến thể nào ổn định thấp hơn). Đúng tinh thần của chương 03 mục 2: trung bình che đuôi[^sre-100ms], và một backend có đuôi xấu càng đau khi service khác gọi nó nhiều lần trong một trang[^sre-fan]. Số p99 của `naive` xấp xỉ thời gian CPU của *một* report (170 µs): giải thích hợp lý nhất (nhận xét của người soạn, không phải điều đã chứng minh bằng profiler) là các request thường bị xếp hàng sau một report đang giữ khóa. p99,9 của `tuned` (175 µs) chính là thời gian của report: 0,5% request là report nên chúng chỉ lộ ra ở p99,5 trở lên.
2. **Khóa thô chỉ có hại khi có tranh chấp.** Chạy 1 thread (cùng 400.000 request):

```text
latency: threads=1 requests/thread=400000 warmup/thread=5000 heavy_every=200 heavy_iters=100000 (one report needs ~171 us of CPU) build=release
machine: cores=11 load1=7.02
variant   requests   wall_ms      req/s   mean_us    p50_us    p99_us  p99.9_us     max_us  allocs/req  checksum
naive       400000     453.8     881370      1.10      0.17      0.54    190.85    1012.22       2.005  0x27d92591c8d6871b
tuned       400000     453.3     882426      1.10      0.12      0.50    218.75     351.74       0.000  0x27d92591c8d6871b
```

   p99 của hai biến thể gần nhau (0,54 và 0,50 µs) và wall time gần như trùng (453,8 và 453,3 ms): không có thread nào chờ thì khóa thô vô hại. Thủ phạm của đuôi ở lần chạy 4 thread là *tranh chấp* cộng với *việc nặng nằm trong vùng giữ khóa*, không phải bản thân `Mutex`.
3. **Cấp phát không phải thủ phạm ở đây.** `naive` gọi allocator 2,005 lần mỗi request, `tuned` chưa đến 0,001 (hiện `0.000`; test của crate chỉ cho phép dưới 64 lần trong cả lần chạy 4 thread, và người soạn quan sát được bookkeeping nhỏ, không phụ thuộc số request, của chính `parking_lot` khi các thread lần đầu phải chờ khóa — bảng băm của "parking lot" ánh xạ địa chỉ khóa tới hàng đợi thread[^plcore-hash]; việc nó cấp phát là suy luận từ quan sát, chưa kiểm nguồn). Nhưng p50 của hai biến thể không khác biệt ổn định: allocator nhỏ rẻ ở workload này. Chương 03 nói cấp phát heap "có thể phải lấy khóa toàn cục hoặc gọi system call"[^pb-heap-cost]; "có thể" nghĩa là phải đo, và ở đây số đếm cấp phát không dự đoán được p99.
4. **Đừng benchmark bản dev.** Cùng workload ở profile dev (`-- both 4 20000`):

```text
latency: threads=4 requests/thread=20000 warmup/thread=5000 heavy_every=200 heavy_iters=100000 (one report needs ~571 us of CPU) build=dev (unoptimized)
machine: cores=11 load1=7.02
variant   requests   wall_ms      req/s   mean_us    p50_us    p99_us  p99.9_us     max_us  allocs/req  checksum
naive        80000     377.2     212082     17.11      0.88    569.34    795.65    1488.89       2.005  0xbb9bcb0310d8674f
tuned        80000     251.5     318116      4.79      1.83     21.14    623.62    1293.31       0.000  0xbb9bcb0310d8674f
```

   p99,9 của `naive` ở dev là 795,65 µs so với 410,11 µs ở release, trung bình 17,11 µs so với 4,83 µs: chậm hơn 2–6 lần tùy chỉ số, thấp hơn khoảng "10–100 lần" mà Performance Book nói là phổ biến[^pb-release] (sách nói "phổ biến", không phải cận dưới). Quan trọng hơn: p50 ở dev **đảo ngược** (naive 0,88 µs, tuned 1,83 µs). Kết luận rút từ bản dev có thể sai chiều. Người soạn không có giải thích cho chỗ đảo ngược này trong nguồn nào.
5. **LTO và một codegen unit chưa chắc đổi gì.** Cùng lệnh với `lto = "fat"` và `codegen-units = 1`: tối ưu lúc link nhìn xuyên crate[^rustc-lto] và một codegen unit có thể cho code nhanh hơn với giá build chậm hơn[^rustc-cgu]; cả hai ghi đè bằng biến môi trường `CARGO_PROFILE_RELEASE_*`[^env-lto][^env-cgu]:

```text
latency: threads=4 requests/thread=100000 warmup/thread=5000 heavy_every=200 heavy_iters=100000 (one report needs ~169 us of CPU) build=release
machine: cores=11 load1=6.94
variant   requests   wall_ms      req/s   mean_us    p50_us    p99_us  p99.9_us     max_us  allocs/req  checksum
naive       400000     531.0     753248      4.94      0.25    188.80    455.68    3987.45       2.005  0x27d92591c8d6871b
tuned       400000     384.2    1041004      1.38      0.29      5.63    186.37     481.54       0.000  0x27d92591c8d6871b
```

   Không có thay đổi vượt khỏi dao động giữa các lần chạy: p50 0,25 và 0,29 µs, p99 của `naive` 188,80 µs (so với 188,54), của `tuned` 5,63 µs (so với 8,08). Cargo cảnh báo thẳng rằng mức tối ưu cao hơn có thể cho kết quả bất ngờ và cần tự đánh giá[^cargo-opt-surprise]. Các build này dùng thư mục target riêng (`/tmp/obs-lto`) để không làm hỏng cache của `target/release`.

**Giới hạn của phép đo, và cách đọc cho đúng.** (i) Tải là *closed-loop*: khi một thread bị kẹt, nó cũng ngừng sinh các request lẽ ra đã xếp hàng sau đó, nên p99 thật của một client phát request theo lịch cố định sẽ cao hơn; `hdrhistogram` có `record_correct` "ghi một giá trị trong khi hiệu chỉnh cho coordinated omission"[^hdr-record-correct] và `clone_correct` bù cho các mẫu bị mất khi một giá trị ghi nhận vượt khoảng cách kỳ vọng giữa các mẫu[^hdr-clone-correct] (bài tập 7). (ii) Hai lần gọi `Instant::now()` cho mỗi request nằm trong con số ở cột p50 — với p50 cỡ 0,2 µs, chi phí đo là một phần đáng kể (nhận xét của người soạn). (iii) Máy chạy nhiều tác vụ khác: chương 03 dặn benchmark trên máy yên tĩnh[^crit-quiet]; `load1` luôn in cạnh kết quả để bạn tự cân nhắc. (iv) `parking_lot` đổi hành vi hàng chờ so với `std::sync::Mutex`: bài tập 6.

**Phần Linux (chưa chạy): on-CPU không thấy chờ khóa.** `perf record -g` trên `naive` sẽ cho `burn` nóng (đó là CPU của các report), nhưng thời gian một request *chờ khóa* là thời gian off-CPU: on-CPU profile bỏ qua nó[^off-timer], và off-CPU analysis (tracing các hàm kernel đưa thread ra khỏi CPU, kèm stack[^off-trace]; cẩn thận vì sự kiện scheduler có thể lên hàng triệu mỗi giây[^off-warn]) mới thấy. Làm thử trong bài tập 3 và so với kết quả `tuned`.

### Lab (d) — `spans`: span, event, subscriber và cái bẫy `Span::enter` qua `.await`

Lý thuyết: [chương 03 mục 7](../11-do-dac-va-go-loi/03-hieu-nang-service-rust.md) (`tracing`: span, event, subscriber; `Span::enter` và `.await`), [chương 02 mục 9.2](../11-do-dac-va-go-loi/02-ebpf-va-tracing.md) (`tracing` khác eBPF thế nào).

`tracing` là framework để gắn instrumentation có cấu trúc vào chương trình Rust[^tr-what]: *span* là một khoảng thời gian có đầu có cuối, *event* là một thời điểm, *subscriber* là thứ nhận thông báo khi event xảy ra và khi span được vào hoặc ra[^tr-span][^tr-event][^tr-sub]. Thư viện không có sẵn subscriber[^tr-nosub], nên lab viết một `Layer` của `tracing-subscriber` tên `Recorder`, ghép vào `Registry` (nơi lưu span)[^tokio-tracing-layer]. `Recorder` in mỗi thông báo thành một dòng chữ: `new` (span được tạo, kèm span cha), `enter`/`exit`, và `event` kèm *đường dẫn span* nó được báo cáo trong đó. Không có thời gian nào tham gia, nên output xác định và giống nhau trên mọi OS.

Hai task `A` và `B` chạy trên một runtime current-thread của Tokio (`LocalSet` chạy các future `!Send` trên thread đang gọi `block_on`[^tokio-localset][^tokio-current]), mỗi task phát một event `start`, nhường một lần (`yield_once`, future tự viết cho thứ tự chạy xác định A, B, A, B; người soạn thử `tokio::task::yield_now` thì thứ tự thành A, B, B, A và chưa tra lý do trong docs), rồi phát `resume`. Ba cách mở span:

```bash
cd labs && cargo run -q -p lab-observability --bin spans
```

Output (xác định; mỗi khối là một cách mở span):

```text
== enter-across-await ==
new    handle{task=A}  (parent: none)
enter  handle{task=A}
event  start   reported in [handle{task=A}]
new    handle{task=B}  (parent: handle{task=A})
enter  handle{task=B}
event  start   reported in [handle{task=A} > handle{task=B}]
event  resume  reported in [handle{task=A} > handle{task=B}]
exit   handle{task=A}
event  resume  reported in [handle{task=A} > handle{task=B}]
exit   handle{task=B}

== instrument ==
new    handle{task=A}  (parent: none)
enter  handle{task=A}
event  start   reported in [handle{task=A}]
exit   handle{task=A}
new    handle{task=B}  (parent: none)
enter  handle{task=B}
event  start   reported in [handle{task=B}]
exit   handle{task=B}
enter  handle{task=A}
event  resume  reported in [handle{task=A}]
exit   handle{task=A}
enter  handle{task=A}
exit   handle{task=A}
enter  handle{task=B}
event  resume  reported in [handle{task=B}]
exit   handle{task=B}
enter  handle{task=B}
exit   handle{task=B}

== instrument-attribute ==
new    with_attribute{task=A}  (parent: none)
enter  with_attribute{task=A}
event  start   reported in [with_attribute{task=A}]
exit   with_attribute{task=A}
new    with_attribute{task=B}  (parent: none)
enter  with_attribute{task=B}
event  start   reported in [with_attribute{task=B}]
exit   with_attribute{task=B}
enter  with_attribute{task=A}
event  resume  reported in [with_attribute{task=A}]
exit   with_attribute{task=A}
enter  with_attribute{task=A}
exit   with_attribute{task=A}
enter  with_attribute{task=B}
event  resume  reported in [with_attribute{task=B}]
exit   with_attribute{task=B}
enter  with_attribute{task=B}
exit   with_attribute{task=B}
```

Giải thích:

1. **`enter-across-await`: giữ guard của `Span::enter` qua `.await`.** Docs của `Span::enter` cảnh báo: trong code async nên dùng rất cẩn thận hoặc tránh hẳn, vì giữ guard qua điểm `.await` cho trace sai[^tr-enter-warn]; lý do là khi async fn dừng ở `.await`, scope bị thoát nhưng các giá trị trong scope *không bị drop*, nên task khác bắt đầu chạy trong khi vẫn "ở trong" span đó[^tr-enter-why]. Transcript thấy đúng điều đó: A vào span và nhường mà không `exit`; B được tạo lúc đó nên span của B có cha là `handle{task=A}` (dòng `new ... (parent: handle{task=A})`); event `start` của B được báo cáo trong `A > B`; rồi event `resume` của *A* cũng được báo cáo trong `A > B`: subscriber tin rằng A đang chạy bên trong span của B. Cả hai event đều mang nhãn sai, và cây span sai (B lồng trong A).
2. **`instrument`: `.instrument(span)`.** Mỗi lần future được poll, span được vào, và thoát khi future nhường[^tr-instrument-combinator]. Mỗi event nằm đúng span của task mình, và không span nào được tạo trong span khác (`parent: none` ở cả hai). Mỗi task có **ba** cặp `enter`/`exit` chứ không phải hai: hai lần poll cộng một lần nữa khi future bọc ngoài bị drop — tracing cố ý vào span lúc drop future bên trong để việc drop được tính vào span đó, theo comment trong source 0.1.44: `ManuallyDrop` được dùng để vào `Span` rồi chạy `ManuallyDrop::drop`[^tr-instrumented-drop].
3. **`instrument-attribute`: `#[tracing::instrument]` trên `async fn`.** Cho đúng cùng transcript với `instrument` (tên span là tên hàm `with_attribute`): attribute sinh code đúng khi dùng trên `async fn`[^tr-instrument-async].
4. **Cách thứ ba để tránh bẫy.** `Span::in_scope` chạy một closure đồng bộ và thoát span trước điểm `.await` kế tiếp[^tr-in-scope] (bài tập 10).

Phần này không cần quyền gì và không đụng kernel — khác với lab (b): `tracing` là instrumentation *trong tiến trình*, bạn phải sửa code; eBPF quan sát từ ngoài (chương 02 mục 9.2). Test `enter_across_await_leaks_one_tasks_span_into_the_other` ghim đúng 10 dòng của khối đầu, nên nếu bản `tracing` sau này đổi hành vi, `cargo test` sẽ báo.

### Lab (e) — `debug_target` và `elf_probe`: debugger cần gì trong binary

Lý thuyết: [chương 04 mục 2](../11-do-dac-va-go-loi/04-debugger-va-dwarf.md) (software breakpoint), [mục 3](../11-do-dac-va-go-loi/04-debugger-va-dwarf.md) (watchpoint), [mục 4](../11-do-dac-va-go-loi/04-debugger-va-dwarf.md) (symbol table và debug info), [mục 5](../11-do-dac-va-go-loi/04-debugger-va-dwarf.md) (DWARF, `<optimized out>`, inline), [mục 6](../11-do-dac-va-go-loi/04-debugger-va-dwarf.md) (backtrace, CFI), [mục 7](../11-do-dac-va-go-loi/04-debugger-va-dwarf.md) (debug info ngoài binary, build ID), [mục 9](../11-do-dac-va-go-loi/04-debugger-va-dwarf.md) (Rust: `-C debuginfo`, `rust-gdb`, pretty printer).

**Phần 1 — `debug_target`, đích để gỡ lỗi.** Chương trình định giá bốn đơn hàng và có đủ thứ một debugger phải hiểu trong Rust: struct `Order` sở hữu một `Vec<Item>` và một `String`; enum `Status` có payload (`New`, `Paid { txn }`, `Shipped { carrier, tracking }`, `Cancelled(String)`); tuple struct `Pair`; trait object `&dyn Pricing` (hai cài đặt `Flat` và `Tiered`); closure `visit` bắt `seen` và `running` bằng tham chiếu `&mut`; hàm `percent` là `#[inline(always)]`; và biến `items_x3` chết ngay sau khi dùng. `checkpoint` là `#[inline(never)]` và in giá trị các tham số của nó: output của chương trình là đáp án để đối chiếu với `info args` và `print` của debugger.

```bash
cd labs && cargo run -q -p lab-observability --bin debug_target
```

```text
checkpoint visit: order=#1001 customer="mai" items=["sku-ssd", "sku-cable"] status=paid(txn 90210) coupon=Some(7) running=13762 pricing=flat pair=(0, 2)
checkpoint visit: order=#1002 customer="bao" items=["sku-ram"] status=shipped(vnpost #77001) coupon=None running=25462 pricing=tiered pair=(1, 1)
checkpoint visit: order=#1003 customer="linh" items=[] status=cancelled(out of stock) coupon=Some(0) running=25462 pricing=flat pair=(2, 0)
checkpoint visit: order=#1004 customer="khanh" items=["sku-usb"] status=new coupon=None running=29212 pricing=tiered pair=(3, 1)
done: seen=4 total_cents=29212 last=29216
```

Kiểm bằng tay: đơn 1001 = 2 × 5.499 + 5 × 349 = 12.743 cent, `Flat` cộng thuế 8% (1.019) thành 13.762 nên `running=13762`; đơn 1002 = 12.999 ≥ ngưỡng 10.000 nên `Tiered` giảm 10% (1.299) còn 11.700, `running=25462`; đơn 1003 không có món nào nên giữ nguyên; đơn 1004 = 3 × 1.250 = 3.750 dưới ngưỡng nên cộng nguyên, `running=29212`; `last=29216` vì `checkpoint` trả `running + pair.0 + pair.1` (29.212 + 3 + 1).

**Phần Linux (chưa chạy): phiên `gdb` và `lldb`.** Build profile dev (không tối ưu, DWARF đầy đủ[^prof-debug]) rồi mở bằng `rust-gdb`, script bọc `gdb` và nạp pretty printer của Rust[^rust-gdb-run]:

```bash
./labs/linux.sh bash -c 'cargo build -q -p lab-observability --bin debug_target'
./labs/linux.sh rust-gdb -q target-linux/debug/debug_target       # chưa chạy; hoặc `gdb` để so
```

```text
(gdb) break debug_target::checkpoint        # theo tên hàm
(gdb) run
(gdb) info args                              # stage, order, running_cents, pricing, pair
(gdb) print *order                           # struct có Vec và String
(gdb) print order.status                     # enum có payload
(gdb) print *pair
(gdb) ptype Status
(gdb) bt                                     # checkpoint <- closure <- main
(gdb) up
(gdb) info locals
(gdb) up                                     # sang frame của main
(gdb) watch running                          # watchpoint; xem gdb báo "Hardware watchpoint" hay "Watchpoint"
(gdb) continue
(gdb) finish
(gdb) x/6i $pc                               # lệnh máy quanh điểm dừng
```

LLDB có lệnh tương ứng cho từng việc: `break test.c:12` của GDB là `breakpoint set --file test.c --line 12`[^lldb-map-bp], `bt` là `thread backtrace`[^lldb-map-bt], `watch global_var` là `watchpoint set variable global_var`[^lldb-map-watch]; `rust-lldb` là script bọc `lldb` nạp `lldb_lookup.py` và file lệnh của Rust[^rust-lldb-run].

Điều cần tìm (kỳ vọng suy ra từ nguồn; **chưa quan sát**):

- **Breakpoint theo tên và theo dòng.** Breakpoint theo `file:line` cần `.debug_line`, bảng gắn vị trí nguồn với địa chỉ lệnh[^dw-line-intro]; mức `line-tables-only` đủ cho việc đó nhưng không có thông tin biến hay tham số[^cg-ltonly], nên với build đó `info locals` sẽ không có gì để hiện (suy luận của người soạn).
- **`print *order`.** Với `rust-gdb`, `Vec` và `String` được hiển thị qua pretty printer viết bằng Python[^gdb-pp]; printer bám tên trường *private* của std nên đi kèm toolchain (chương 04 mục 9.4). Với `gdb` trần, binary có sẵn section `.debug_gdb_scripts` để GDB tự nạp script — danh sách entry kết thúc bằng byte 0[^gdb-scripts-sect] — nhưng script đó đòi `auto-load safe-path` cấu hình đúng[^gdb-scripts-safe]; phần 2 dưới đây dump section đó.
- **Tuple struct và trait object.** DWARF không phân biệt struct với tuple: rustc phát trường tên `__0`, `__1` và debugger nhận ra dãy tên đó[^rdg-limits], nên `Pair` hiện hai trường. `pricing: &dyn Pricing` mổ được vì rustc phát DIE cho vtable với `DW_AT_containing_type` trỏ tới kiểu thật[^rdg-ext]; nhưng trait không hề được phát vào DWARF nên gọi `pricing.name()` từ debugger sẽ không chạy như thường[^rdg-traits]. Enum `Status`: chương 04 không bàn kiểu hiển thị của enum có payload; hãy ghi lại gdb in ra gì và so với `describe()` của chương trình (bài tập 11).
- **Watchpoint.** `watch running` dừng khi chương trình *ghi* vào biến và giá trị đổi[^gdb-watch-simple]; GDB đặt watchpoint phần cứng nếu được, báo thay đổi đúng lệnh gây ra[^gdb-watch-prefer]; nếu không thì single-step và kiểm giá trị sau mỗi bước, chậm hàng trăm lần[^gdb-watch-sw]. Trên x86 chỉ có bốn thanh ghi địa chỉ debug[^k-uapi-dr]; máy Apple Silicon chạy container aarch64, nơi con số này khác và người soạn chưa tra.
- **Bản tối ưu.** Build `CARGO_PROFILE_RELEASE_DEBUG=2 cargo build --release ...` (DWARF đầy đủ nhưng có tối ưu): GDB nói trình tối ưu đã sắp xếp lại code và debugger hiển thị cái *thật sự có đó*[^gdb-optim]. Biến chết `items_x3` kỳ vọng hiện `<optimized out>`: khi biến không còn chỗ nào trong chương trình chạy thì đặc tả DWARF coi nó "tồn tại trong source nhưng không tồn tại trong chương trình thực thi"[^dw-noloc], và vị trí của biến có thể đổi theo từng đoạn lệnh nên được mô tả bằng location list[^dw-loclist]; tham số không được lưu trong frame cũng hiện `<optimized out>`[^gdb-optout-args]. `percent` (inline) kỳ vọng xuất hiện như một frame thường — GDB hiển thị hàm inline giống hàm không inline: có trong backtrace, `step` vào được, `finish` thoát được[^gdb-inline] — miễn compiler đã ghi thông tin inline vào debug info[^gdb-inline-dwarf].

**Phần 2 — `elf_probe`: ba binary Linux, ba mức debug.** `elf_probe` là một `readelf` nhỏ viết trong lab (mã ở `lab_observability::elf`, có test với file ELF dựng tay, cắt cụt và sửa byte). Nó chỉ đọc file nên chạy được trên macOS với các binary Linux có sẵn trong `labs/target-linux/`, do các lab trước build trong container (không phải do lần chạy này): `debug/count_alloc` (lab P3, profile dev), `release/counter` (lab P4, profile release mặc định của workspace) và `release/hello` (người soạn đoán là từ phần đo kích thước binary của P8; nó không có `.symtab` và người soạn không biết chính xác cách file này được build). Để có các file tương tự, build trong container `cargo build -p lab-memory --bin count_alloc` và `cargo build --release -p lab-concurrency --bin counter`; số byte sẽ khác nếu toolchain khác.

```bash
cd labs && E="cargo run -q -p lab-observability --bin elf_probe --"
$E target-linux/debug/count_alloc | sed -n '/^symbols and debug data/,$p'
```

```text
symbols and debug data (the tools' view):
  .symtab: present, 3183 symbols
  .dynsym: present, 69 symbols
  .debug_* sections: 9 sections, 4029760 bytes in total
    .debug_gdb_scripts         34 bytes
    .debug_aranges          35520 bytes
    .debug_info           1032254 bytes
    .debug_abbrev           14950 bytes
    .debug_line            468504 bytes
    .debug_frame              136 bytes
    .debug_str            1851361 bytes
    .debug_loc               1561 bytes
    .debug_ranges          625440 bytes
  .eh_frame: present, 35072 bytes
  .eh_frame_hdr: present, 6004 bytes
  .gnu_debuglink: absent
  build id: 5cb06d4b7782223b18e446ea57f7214bf684ea87
```

```bash
$E target-linux/release/counter | sed -n '/^symbols and debug data/,$p'
```

```text
symbols and debug data (the tools' view):
  .symtab: present, 3042 symbols
  .dynsym: present, 78 symbols
  .debug_* sections: none
  .eh_frame: present, 38384 bytes
  .eh_frame_hdr: present, 6140 bytes
  .gnu_debuglink: absent
  build id: 8bfe1652ad6c947e426955a651f2fc645acab814
```

```bash
$E target-linux/release/hello | sed -n '/^symbols and debug data/,$p'
```

```text
symbols and debug data (the tools' view):
  .symtab: absent
  .dynsym: present, 62 symbols
  .debug_* sections: none
  .eh_frame: present, 23368 bytes
  .eh_frame_hdr: present, 3756 bytes
  .gnu_debuglink: absent
  build id: 069b21ee39a37e5d47ea77562f6bea6e8678e35a
```

| Build | `.symtab` | `.dynsym` | `.debug_*` | `.eh_frame` |
| --- | --- | --- | --- | --- |
| dev (`count_alloc`) | 3.183 symbol | 69 | 9 section, 4.029.760 byte | 35.072 byte |
| release mặc định (`counter`) | 3.042 symbol | 78 | không có | 38.384 byte |
| đã strip (`hello`) | không có | 62 | không có | 23.368 byte |

Đọc bảng:

1. **Release mặc định giữ tên hàm, không có debuginfo.** `counter` có `.symtab` nhưng không có section `.debug_*` nào: khớp với mặc định của Cargo cho profile `release` — `debug` bằng không (không có debug info nào)[^prof-debug] và `strip = "none"`[^prof-strip]. Đây là bằng chứng cho suy luận ở cuối chương 01 mục 6 ("release mặc định giữ symbol nhưng không có debuginfo"), và là lý do `perf` và `bpftrace` ở lab (a) và (b) tìm được tên hàm trong bản release mà vẫn cần `line-tables-only` để có số dòng.
2. **Strip bỏ `.symtab`, không bỏ `.dynsym` và `.eh_frame`.** `hello` còn 62 symbol động vì loader cần chúng để liên kết (`.dynsym` là bảng symbol tối thiểu cho liên kết động[^elf-dynsym]) và còn `.eh_frame`, thông tin unwind[^lsb-ehframe]. Mất `.symtab` là cái rustc cảnh báo: trên nền tảng dựa vào bảng symbol cho backtrace và profiling, trace có thể không đọc nổi[^cg-strip-sym].
3. **Debug info lớn hơn cả code.** `count_alloc` dài 4.592.760 byte và `.debug_*` chiếm 4.029.760 byte, khoảng 88% file; GDB nói debug info có thể rất lớn, đôi khi lớn hơn chính phần code[^gdb-sep-why]. Đó là lý do mục 7 của chương 04 nói về tách debug info ra file riêng.
4. **DWARF 4 chứ chưa phải 5.** Binary dev có `.debug_loc` và `.debug_ranges` nhưng không có `.debug_loclists`. Rustc phát DWARF phiên bản 4 mặc định trên Linux và macOS[^rustc-dwarf], còn danh sách vị trí kiểu mới nằm ở `.debug_loclists`, thứ mới có từ DWARF 5[^dw-loclists-sect][^dw-loclist-v5]; kết luận "đây là DWARF 4" là suy luận của người soạn từ tên section, không đọc phiên bản trong `.debug_info`.
5. **Build ID có, debug link không.** Cả ba file có build ID 40 chữ số hex (160 bit, SHA1 mặc định[^gdb-buildid-sha]) nhưng không có `.gnu_debuglink`. Build ID là chuỗi định danh duy nhất có mặt ở cả binary lẫn file debug tương ứng[^gdb-buildid], và GDB tìm file debug theo thư mục `.build-id/nn/…`[^gdb-buildid-path]. Hãy tách debug info ra (`objcopy --only-keep-debug`, rồi `strip`)[^gdb-objcopy] hoặc dùng `-C split-debuginfo`[^rustc-split-def] và chạy lại `elf_probe` (bài tập 12).

Section `.debug_gdb_scripts` là cơ chế đưa pretty printer vào binary. Dump nó:

```bash
$E dump target-linux/debug/count_alloc .debug_gdb_scripts
```

```text
section .debug_gdb_scripts: 34 bytes (showing 34)
  0x0000  01 67 64 62 5f 6c 6f 61 64 5f 72 75 73 74 5f 70  |.gdb_load_rust_p|
  0x0010  72 65 74 74 79 5f 70 72 69 6e 74 65 72 73 2e 70  |retty_printers.p|
  0x0020  79 00                                            |y.|
```

34 byte: một byte `01`, tên script `gdb_load_rust_pretty_printers.py`, rồi byte 0 kết thúc entry. Đúng như mô tả ở chương 04: rustc đặt entry đầu tiên là script pretty printer của thư viện chuẩn, theo dạng `\x01gdb_load_rust_pretty_printers.py` trong source `gdb.rs` của 1.95.0[^rs-gdbrs-std], vào biến đặt ở section `.debug_gdb_scripts`[^rs-gdbrs-section]; và GDB đọc section này như danh sách entry kết thúc bằng byte 0[^gdb-scripts-sect].

**Phần Linux (chưa chạy): đọc DWARF bằng công cụ chuẩn.** `readelf --debug-dump` hiển thị nội dung các section DWARF nếu có[^readelf-w]; `llvm-dwarfdump` dump và kiểm tra DWARF[^llvm-dwarfdump]; `readelf -x` dump một section dạng hex[^readelf-x]:

```bash
./labs/linux.sh bash -c '
  B=target-linux/debug/debug_target
  readelf --debug-dump=info $B | grep -A4 -E "DW_AT_name.*(Order|Status|Pair)" | head -40
  readelf --debug-dump=decodedline $B | head -20
  readelf -x .debug_gdb_scripts $B                       # so với `elf_probe dump`
  llvm-dwarfdump --debug-line $B | head -30              # cần gói llvm
'                                                         # chưa chạy
```

Kỳ vọng: `.debug_info` là cây DIE mô tả từng thực thể bằng một tag và các attribute[^dw-die]; bạn sẽ thấy DIE tên `Order`, `Status`, `Pair`, và trong `Pair` hai trường tên `__0` và `__1`[^rdg-limits]. Bảng dòng nằm ở `.debug_line`[^dw-line-sect] và mỗi hàng có cờ `is_stmt` đánh dấu vị trí breakpoint được khuyến nghị[^dw-isstmt]. Hai lệnh `readelf -x .debug_gdb_scripts` và `elf_probe dump` phải cho cùng 34 byte.

### Lab (f) — `crash_lab`: panic, abort, SIGSEGV, tràn stack, treo và core

Lý thuyết: [chương 05 mục 1](../11-do-dac-va-go-loi/05-core-dump-va-backtrace.md) (signal nào sinh core), [mục 2](../11-do-dac-va-go-loi/05-core-dump-va-backtrace.md) (vì sao không thấy core), [mục 3](../11-do-dac-va-go-loi/05-core-dump-va-backtrace.md) (`core_pattern`), [mục 4](../11-do-dac-va-go-loi/05-core-dump-va-backtrace.md) (đọc core bằng gdb), [mục 5](../11-do-dac-va-go-loi/05-core-dump-va-backtrace.md) (panic và abort), [mục 6](../11-do-dac-va-go-loi/05-core-dump-va-backtrace.md) (`RUST_BACKTRACE`, `Backtrace`, panic hook), [mục 7](../11-do-dac-va-go-loi/05-core-dump-va-backtrace.md) (tràn stack), [mục 8](../11-do-dac-va-go-loi/05-core-dump-va-backtrace.md) (process treo).

`crash_lab <case>` là một binary với nhiều cách chết, mỗi cách một case, và in một dòng `crash_lab: case ... pid=...` ra stderr trước khi hỏng. Các lệnh dưới đây chạy bản dev (`target/debug`) trừ khi ghi khác; `exit=` là mã thoát mà shell thấy (`$?`). Build một lần:

```bash
cd labs && cargo build -q -p lab-observability --bins && cargo build -q --release -p lab-observability --bin crash_lab
```

**1. `panic!` không phải crash.**

```bash
./target/debug/crash_lab panic; echo "exit=$?"
./target/debug/crash_lab panic-thread; echo "exit=$?"
```

```text
crash_lab: case `panic` pid=52230

thread 'main' (139248979) panicked at crates/lab-observability/src/bin/crash_lab.rs:49:9:
order 7: odd ids are not allowed
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
exit=101
crash_lab: case `panic-thread` pid=52307

thread 'worker' (139249253) panicked at crates/lab-observability/src/bin/crash_lab.rs:49:9:
order 9: odd ids are not allowed
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
main: the worker thread panicked (order 9: odd ids are not allowed); the process carries on
exit=0
```

Panic ở main thread kết thúc chương trình với mã `101`: source runtime của std bọc `main` và trả 101 khi panic[^rs-main-exit]. Đó là *exit code*, không phải signal, nên theo `core(5)` — core gắn với hành động mặc định của signal[^core-default] — panic thường không sinh core (suy luận của người soạn, như chương 05). Thông báo có tên thread, vị trí trong source, và dòng nhắc đặt `RUST_BACKTRACE=1`[^rs-note-run-with]. Panic ở thread phụ thì process **không chết**: `join` trả `Err` còn main thread tiếp tục và thoát `0`[^rs-thread-uncaught]; nhiều server "chết một nửa" vì worker panic mà không ai `join` (nhận xét của chương 05).

Cùng chương trình đó, đổi một cấu hình — `panic = "abort"`, ghi đè bằng biến môi trường `CARGO_PROFILE_DEV_PANIC`[^env-panic] (profile `panic` điều khiển chiến lược `unwind` hay `abort`[^cargo-panic]), build ra thư mục target riêng:

```bash
CARGO_PROFILE_DEV_PANIC=abort CARGO_TARGET_DIR=/tmp/obs-abort cargo build -q -p lab-observability --bin crash_lab
/tmp/obs-abort/debug/crash_lab panic; echo "exit=$?"
```

```text
crash_lab: case `panic` pid=67300

thread 'main' (139284824) panicked at crates/lab-observability/src/bin/crash_lab.rs:49:9:
order 7: odd ids are not allowed
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
exit=134
```

Thông báo y hệt (panic hook vẫn chạy trước khi abort[^rs-abort-vs-panic][^hook-reg]) nhưng mã thoát là `134` thay vì `101`. Shell đặt mã thoát bằng `128 + N` khi lệnh chết vì signal có số `N`[^bash-128n]; `134 = 128 + 6`, và 6 là số của `SIGABRT`[^sig-num-abrt], signal có hành động mặc định `Core`[^sg-abrt]. Một dòng cấu hình đổi "thoát có trật tự, không core" thành "chết vì signal, có thể có core".

**2. `RUST_BACKTRACE` và debuginfo.** Ba build, cùng một panic:

```bash
RUST_BACKTRACE=1 ./target/debug/crash_lab panic                                   # dev
RUST_BACKTRACE=1 ./target/release/crash_lab panic                                 # release mặc định
CARGO_PROFILE_RELEASE_DEBUG=line-tables-only CARGO_TARGET_DIR=/tmp/obs-lt \
  cargo build -q --release -p lab-observability --bin crash_lab \
  && RUST_BACKTRACE=1 /tmp/obs-lt/release/crash_lab panic                         # release + line-tables-only
```

Output (rút gọn: bỏ các dòng `at` trỏ vào thư mục toolchain của máy người soạn):

```text
crash_lab: case `panic` pid=92548

thread 'main' (139344534) panicked at crates/lab-observability/src/bin/crash_lab.rs:49:9:
order 7: odd ids are not allowed
stack backtrace:
   0: __rustc::rust_begin_unwind
             at /rustc/59807616e1fa2540724bfbac14d7976d7e4a3860/library/std/src/panicking.rs:689:5
   1: core::panicking::panic_fmt
             at /rustc/59807616e1fa2540724bfbac14d7976d7e4a3860/library/core/src/panicking.rs:80:14
   2: crash_lab::validate
             at ./crates/lab-observability/src/bin/crash_lab.rs:49:9
   3: crash_lab::handle
             at ./crates/lab-observability/src/bin/crash_lab.rs:56:5
   4: crash_lab::panic_main
             at ./crates/lab-observability/src/bin/crash_lab.rs:60:29
   5: crash_lab::main
             at ./crates/lab-observability/src/bin/crash_lab.rs:30:20
   6: core::ops::function::FnOnce::call_once
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.
exit=101
```

```text
crash_lab: case `panic` pid=92550

thread 'main' (139344539) panicked at crates/lab-observability/src/bin/crash_lab.rs:49:9:
order 7: odd ids are not allowed
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: crash_lab::validate
   3: crash_lab::handle
   4: crash_lab::main
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.
exit=101
```

```text
crash_lab: case `panic` pid=92552

thread 'main' (139344542) panicked at crates/lab-observability/src/bin/crash_lab.rs:49:9:
order 7: odd ids are not allowed
stack backtrace:
   0: __rustc::rust_begin_unwind
             at /rustc/59807616e1fa2540724bfbac14d7976d7e4a3860/library/std/src/panicking.rs:689:5
   1: core::panicking::panic_fmt
             at /rustc/59807616e1fa2540724bfbac14d7976d7e4a3860/library/core/src/panicking.rs:80:14
   2: validate
             at ./crates/lab-observability/src/bin/crash_lab.rs:49:9
   3: handle
             at ./crates/lab-observability/src/bin/crash_lab.rs:56:5
   4: panic_main
             at ./crates/lab-observability/src/bin/crash_lab.rs:60:29
   5: main
             at ./crates/lab-observability/src/bin/crash_lab.rs:30:20
   6: call_once<fn(), ()>
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.
exit=101
```

`RUST_BACKTRACE=1` bật backtrace ngắn, `full` bật đầy đủ, `0` tắt[^bt-style-1][^bt-style-full][^bt-style-0]. Ba khối cho thấy chuỗi nhân quả của chương 01 và 05 bằng chứng thật:

- **Dev** có `at ./crates/.../crash_lab.rs:49:9`: số dòng cần debug info[^bt-debuginfo], và profile dev mặc định có debug info đầy đủ.
- **Release mặc định có tên hàm nhưng không có số dòng**, vì `debug` mặc định của release là không có debug info[^prof-debug]; tên lấy được từ bảng symbol (lab (e): `.symtab` còn trong bản release). Chú ý chỉ còn 4 frame của chương trình thay vì 5: `panic_main` không có mặt — rustc tự inline hàm khi thấy đáng làm[^ref-inline-auto], và không còn thông tin inline nào để dựng lại frame đó.
- **Release + `line-tables-only` có số dòng trở lại**: mức này sinh lượng debug info tối thiểu đủ cho backtrace có tên file và số dòng, không có thông tin biến[^cg-ltonly], đúng cấu hình Performance Book khuyên cho profile release[^pb-debugline]. Frame `panic_main` hiện lại, nhưng như một frame *inline* — debugger và backtrace dựng lại được hàm đã inline khi compiler ghi thông tin inline vào debug info[^gdb-inline-dwarf][^dw-inl-tag]. Hai quan sát nữa mà người soạn không có nguồn để giải thích: tên hàm trong khối này không có tiền tố `crash_lab::`, và frame `core::ops::function::FnOnce::call_once` hiện là `call_once<fn(), ()>`.

**3. `Backtrace` và panic hook.** `capture()` chỉ chụp khi biến môi trường cho phép, `force_capture()` thì luôn chụp[^bt-capture-noop][^bt-force]; `RUST_LIB_BACKTRACE` được ưu tiên hơn `RUST_BACKTRACE` và nếu chưa đặt thì dùng `RUST_BACKTRACE`[^bt-lib][^bt-fallback]; trạng thái được cache từ lần chụp đầu tiên[^bt-cache]. Ba lần chạy, in dòng trạng thái và số frame:

```bash
for e in "" "RUST_BACKTRACE=1" "RUST_LIB_BACKTRACE=0 RUST_BACKTRACE=1"; do
  echo "== env: ${e:-<none>}"; env $e ./target/debug/crash_lab backtrace 2>&1 | grep -E 'status|frames printed'; done
```

```text
== env: <none>
capture():       status = Disabled
force_capture(): status = Captured
frames printed:  capture() = 0, force_capture() = 20
== env: RUST_BACKTRACE=1
capture():       status = Captured
force_capture(): status = Captured
frames printed:  capture() = 21, force_capture() = 20
== env: RUST_LIB_BACKTRACE=0 RUST_BACKTRACE=1
capture():       status = Disabled
force_capture(): status = Captured
frames printed:  capture() = 0, force_capture() = 20
```

Khớp từng điều ở trên: không đặt gì thì `capture()` là `Disabled` (0 frame) còn `force_capture()` luôn `Captured`; `RUST_BACKTRACE=1` bật `capture()`; `RUST_LIB_BACKTRACE=0` thắng `RUST_BACKTRACE=1`. Còn `hook` cài một panic hook: hook là hàm gọi khi thread panic, *trước* khi panic runtime chạy, nhận `PanicHookInfo` có payload và vị trí nguồn[^hook-reg][^hook-info][^phi-location]. Hook của lab in vị trí, payload và một backtrace chụp ngay trong hook, rồi gọi hook mặc định:

```bash
./target/debug/crash_lab hook 2>&1 | grep -E 'HOOK|crash_lab::|panicked|odd ids'; echo "exit=${PIPESTATUS[0]}"
```

```text
HOOK: panic at crates/lab-observability/src/bin/crash_lab.rs:49: order 3: odd ids are not allowed
HOOK: backtrace (Captured):
   3: crash_lab::hook::{{closure}}
  10: crash_lab::validate
  11: crash_lab::handle
  12: crash_lab::hook
  13: crash_lab::main
thread 'main' (139344547) panicked at crates/lab-observability/src/bin/crash_lab.rs:49:9:
order 3: odd ids are not allowed
exit=101
```

(Đã lọc: backtrace đầy đủ trong hook có 28 frame, đánh số 0–27; ở trên chỉ giữ dòng `HOOK`, frame của chương trình và thông báo mặc định.) Frame 10–13 là `validate`, `handle`, `hook`, `main`: đường gọi dẫn tới panic, còn nguyên vì hook chạy *trước* khi unwind; mã thoát vẫn là `101` vì hook mặc định được gọi sau cùng.

**4. Chết bằng signal.**

```bash
./target/debug/crash_lab abort;   echo "exit=$?"
./target/debug/crash_lab segv;    echo "exit=$?"     # dev
./target/release/crash_lab segv;  echo "exit=$?"     # release
./target/debug/crash_lab stack;   echo "exit=$?"
```

```text
crash_lab: case `abort` pid=52308
exit=134
crash_lab: case `segv` pid=52309

thread 'main' (139249258) panicked at crates/lab-observability/src/bin/crash_lab.rs:119:14:
null pointer dereference occurred
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
thread caused non-unwinding panic. aborting.
exit=134
crash_lab: case `segv` pid=52310
exit=139
crash_lab: case `stack` pid=52311

thread 'recurser' (139249266) has overflowed its stack
fatal runtime error: stack overflow, aborting
exit=134
```

- **`abort`**: `process::abort()` kết thúc process bằng `SIGABRT` trên Unix[^rs-abort-sig] và không gọi panic hook[^rs-abort-vs-panic] — đúng như output chỉ có dòng tiêu đề, và `exit=134`.
- **`segv`**: *cùng một dòng nguồn* (`*p = 1` với `p` là con trỏ null, giấu bằng `black_box`) cho hai kết cục theo profile. Release: `exit=139 = 128 + 11`, số 11 là của `SIGSEGV`[^sig-num-segv], tham chiếu bộ nhớ không hợp lệ và có hành động mặc định `Core`[^sg-segv]. Dev: thông báo `null pointer dereference occurred`, một non-unwinding panic rồi abort, tức `134`. Rust 1.86 thêm các assertion gỡ lỗi kiểm con trỏ null khi đọc ghi (không phải kích thước 0) và khi reborrow, và ví dụ của release notes tạo một non-unwinding panic khi bật debug assertions[^rust186-null][^rust186-nounwind]. Người soạn có thử riêng (không nằm trong lab) `p.write(1)` và `p.write_volatile(1)` thay cho `*p = 1`: ở dev chúng vẫn chết bằng `139`, tức assertion kể trên không bắt hai lời gọi này (đo trên 1.95.0, không có nguồn). Hệ quả cho vận hành: *cùng một bug* sinh core ở cả hai profile (`SIGSEGV` và `SIGABRT` đều là signal `Core`) nhưng stack trong core khác nhau.
- **`stack`**: thread `recurser` có stack 256 KiB đệ quy vô hạn; std in `has overflowed its stack` kèm tên thread rồi `fatal runtime error: stack overflow, aborting`, và `exit=134`: signal handler của std bắt `SIGSEGV` ở guard page rồi gọi abort, nên bạn thấy `SIGABRT` kèm thông báo chứ không phải `SIGSEGV` trần[^so-misleading][^so-msg][^so-abort]. Stack tràn chạm guard page, trang bộ nhớ cấm truy cập đặt cạnh stack, và thường sinh `SIGSEGV`[^pthr-guard-sigsegv]. Output này đo trên macOS; chương 05 mục 7 mô tả cơ chế trên Linux (handler của std, guard page) và người soạn chưa chạy case này trên Linux.

**5. Process treo.**

```bash
./target/debug/crash_lab hang 2; echo "exit=$?"
```

```text
crash_lab: case `hang` pid=52312
crash_lab: two threads are about to deadlock; watchdog = 2 s (0 = wait forever)
crash_lab: watchdog: no thread finished in 2 s; exiting with code 3
exit=3
```

Hai thread `worker-AB` và `worker-BA` mỗi thread giữ một `parking_lot::Mutex` rồi xin cái kia (có `Barrier` để cả hai chắc chắn đã giữ khóa đầu): deadlock kinh điển AB/BA, không thread nào kết thúc. Watchdog trong main thread thoát với mã `3` sau 2 giây để lab không treo máy bạn; `hang 0` chờ vô hạn, dùng cho phần Linux bên dưới. Trong trạng thái này CPU gần như bằng không (suy luận của người soạn: cả hai thread bị chặn trong hàm khóa).

**6. `preflight`: vì sao có hay không có core?**

```bash
./target/debug/crash_lab preflight; echo "exit=$?"
```

```text
crash_lab: case `preflight` pid=52341
RLIMIT_CORE soft = 0 (no core file is created)
RLIMIT_CORE hard = unlimited
RLIM_INFINITY    = 0x7fffffffffffffff
crash_lab preflight: reading /proc/sys/kernel/core_pattern and /proc/self/coredump_filter is Linux-only; this build is for another OS.
Run it in the lab container from the repository root:
  ./labs/linux.sh cargo run -q -p lab-observability --bin crash_lab -- preflight
exit=0
```

Trên macOS: giới hạn mềm `RLIMIT_CORE` là 0, tức theo `getrlimit(2)` không tạo core file[^rl-core]; giới hạn cứng là không giới hạn, và `RLIM_INFINITY` là `0x7fffffffffffffff` trên máy này (test của crate dùng giá trị đó, vì thế `CoreLimit::from_rlim` nhận `RLIM_INFINITY` của nền tảng làm tham số). Phần đọc `/proc/sys/kernel/core_pattern` chỉ chạy được trên Linux nên chương trình in hướng dẫn. Trên Linux `preflight` đưa ba số vào hàm `corecheck::verdict`, bảng quyết định mà các test của crate kiểm tra:

| `core_pattern` | `RLIMIT_CORE` (mềm) | Kết quả |
| --- | --- | --- |
| tên file | 0 | không có core file[^core-notproduced][^rl-core] |
| tên file | khác 0 | file theo template; core lớn hơn giới hạn bị cắt[^rl-core] |
| bắt đầu bằng `\|` | bất kỳ, kể cả 0 | giới hạn bị **bỏ qua**; chương trình handler quyết định giữ gì[^core-rlimit-zero][^core-pipe] |
| rỗng và `core_uses_pid` = 0 | — | không có core[^core-cp-empty] |

**Phần Linux (chưa chạy): tạo core rồi đọc nó.**

```bash
./labs/linux.sh bash -c '
  cargo build -q -p lab-observability --release --bin crash_lab && cd target-linux/release
  ./crash_lab preflight                    # RLIMIT_CORE, core_pattern, coredump_filter và phán quyết
  ulimit -c unlimited                      # chỉ cho shell này và lệnh nó chạy
  ./crash_lab segv; echo "exit=$?"         # kỳ vọng 139
  ls -la core* 2>/dev/null                 # file core (nếu core_pattern cho phép)
  gdb -q -batch ./crash_lab core -ex "bt" -ex "info threads"
'                                           # chưa chạy
```

Điều cần tìm:

- `ulimit` là lệnh của shell đặt `RLIMIT_CORE`; `core(5)` trỏ người đọc tới tài liệu `ulimit` của shell[^core-rlimit-fsize]. Tên file mặc định là `core` hoặc `core.pid` trong thư mục làm việc của process[^core-default-name]; nhưng nếu ký tự đầu của `core_pattern` là `|` thì kernel không ghi file mà đưa core vào stdin của một chương trình[^core-pipe] (và khi đó `ulimit -c` không có tác dụng, xem bảng trên). Kernel chỉ có *một* giá trị `core_pattern` cho cả máy, không phải mỗi container một giá trị[^kern-cp-array], và handler dạng pipe chạy ở namespace gốc[^core-pipe-ns]: trong Docker Desktop "máy" là VM Linux, nên `preflight` mới là nguồn sự thật. Nếu không thấy file `core`, hãy chạy `preflight` trước khi nghi ngờ code. Đặt `core_pattern` đổi cấu hình của cả VM, hãy cân nhắc trước khi làm. Với Docker, `docker run --ulimit core=-1` đặt `RLIMIT_CORE`[^docker-ulimit][^docker-core].
- `gdb chương_trình core` mở cả file thực thi lẫn core[^gdb-prog-core] — core chỉ chứa *một số phần* của không gian địa chỉ, GDB lấy phần còn lại từ file thực thi[^gdb-corefile], nên phải đúng binary đã crash. `bt` in một dòng mỗi frame, từ frame đang chạy lên các caller[^gdb-bt-def]; `thread apply all bt` in stack mọi thread[^gdb-bt-all]. Bản `--release` mặc định có tên hàm nhưng không có số dòng (lab (e)); build lại với `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only` để có.
- Với từng case: `abort` → `SIGABRT` nên có core; `segv` ở release → `SIGSEGV`, signal đồng bộ gửi cho đúng thread đã chạy lệnh gây lỗi[^sig-thread-directed], nên frame trên cùng kỳ vọng là `crash_lab::segv` (suy luận của người soạn); `segv` ở dev → `SIGABRT` từ đường panic nên stack khác — so hai core là bài tập 13. `stack`: handler của std gọi abort nên core chứa stack tại thời điểm abort trong handler, còn stack của thread tràn vẫn nằm nguyên trong bộ nhớ; kỳ vọng `thread apply all bt` thấy frame `recurse` lặp lại nhiều lần (suy luận của chương 05).
- **Process treo**: chạy `./crash_lab hang 0 &` rồi lấy stack mà không cần restart sạch: `gcore PID` trong `gdb` tạo core cho process đang chạy[^core-gcore], `gdb -p PID` gắn thẳng vào nó[^gdb-attach] (cần `CAP_SYS_PTRACE`; `linux.sh` đã thêm `SYS_PTRACE`, còn Docker mặc định bỏ capability này[^docker-ptrace]), hoặc `kill -QUIT PID` vì `SIGQUIT` có hành động mặc định `Core`[^sg-quit] (và process sẽ chết). Rồi `thread apply all bt`[^gdb-bt-all]: kỳ vọng thấy `worker-AB` và `worker-BA` mỗi thread đang đứng trong một hàm khóa gọi từ closure của `hang`, main thread đang ngủ — và mỗi thread giữ cái khóa mà thread kia cần (suy ra từ code, không quan sát).

### Lab (g) — `elf_probe` và `aslr_probe`: ELF, loader, PIE và ASLR

Lý thuyết: [chương 06 mục 1](../11-do-dac-va-go-loi/06-elf-linker-loader.md) (header, section và segment), [mục 3](../11-do-dac-va-go-loi/06-elf-linker-loader.md) (`execve`, `PT_INTERP`), [mục 4](../11-do-dac-va-go-loi/06-elf-linker-loader.md) (`ld.so` tìm thư viện), [mục 5](../11-do-dac-va-go-loi/06-elf-linker-loader.md) (PLT/GOT, RELRO), [mục 6](../11-do-dac-va-go-loi/06-elf-linker-loader.md) (PIE và ASLR), [mục 7](../11-do-dac-va-go-loi/06-elf-linker-loader.md) (tĩnh so với động), [mục 8](../11-do-dac-va-go-loi/06-elf-linker-loader.md) (Rust).

**Phần 1 — macOS không chạy ELF.** `elf_probe` là một `readelf` nhỏ (xem lab (e)); cho nó đọc chính binary của nó, một file macOS:

```bash
cd labs && cargo run -q -p lab-observability --bin elf_probe -- target/debug/elf_probe; echo "exit=$?"
```

```text
elf_probe: target/debug/elf_probe: not an ELF file: first bytes are cf fa ed fe, expected 7f 45 4c 46
exit=1
```

File ELF luôn bắt đầu bằng byte `0x7f` rồi `E`, `L`, `F`[^elf-magic]; file này bắt đầu bằng `cf fa ed fe`, tức số 32-bit `0xFEEDFACF` viết little-endian — hằng `MH_MAGIC_64` của định dạng Mach-O, định dạng file thực thi của macOS (kiến thức nền của người soạn; nguồn chỉ cho hằng số)[^macho-magic]. Test `a_macos_executable_is_reported_as_not_elf` ghim đúng trường hợp này. Hệ quả: mọi lệnh `readelf`, `ldd`, `LD_DEBUG` của chương 06 là chuyện của Linux; trên macOS bạn chỉ có thể đọc file ELF *như dữ liệu*, đúng như `elf_probe` làm với các binary Linux dưới đây.

**Phần 2 — một binary Linux động (glibc).**

```bash
cargo run -q -p lab-observability --bin elf_probe -- target-linux/release/hello
```

```text
file: target-linux/release/hello (332384 bytes)
ELF header:
  type:     DYN (shared object or PIE)
  machine:  AArch64
  entry:    0x7500
  segments: 10 program headers at offset 64 (56 bytes each)
  sections: 29 section headers at offset 330528 (64 bytes each)
kind: ET_DYN + PT_INTERP: dynamically linked PIE executable
interpreter: /lib/ld-linux-aarch64.so.1
segments (the loader's view):
  type          flags     offset        vaddr    filesz     memsz   align
  PHDR          R           0x40         0x40     0x230     0x230     0x8
  INTERP        R          0x270        0x270      0x1b      0x1b     0x1
  LOAD          R E          0x0          0x0   0x406f8   0x406f8 0x10000
  LOAD          RW       0x4e548      0x5e548    0x2458    0x2510 0x10000
  DYNAMIC       RW       0x4fbe0      0x5fbe0     0x200     0x200     0x8
  NOTE          R          0x28c        0x28c      0x44      0x44     0x4
  TLS           R        0x4e548      0x5e548       0x0      0x18     0x8
  GNU_EH_FRAME  R        0x38e60      0x38e60     0xeac     0xeac     0x4
  GNU_STACK     RW           0x0          0x0       0x0       0x0    0x10
  GNU_RELRO     R        0x4e548      0x5e548    0x1ab8    0x1ab8     0x1
dynamic (what ld.so reads):
  NEEDED   libgcc_s.so.1
  NEEDED   libc.so.6
  SONAME   -
  RPATH    -
  RUNPATH  -
  FLAGS    0x8
  bind now (no lazy binding): yes
PT_GNU_RELRO present: yes
symbols and debug data (the tools' view):
  .symtab: absent
  .dynsym: present, 62 symbols
  .debug_* sections: none
  .eh_frame: present, 23368 bytes
  .eh_frame_hdr: present, 3756 bytes
  .gnu_debuglink: absent
  build id: 069b21ee39a37e5d47ea77562f6bea6e8678e35a
```

Đọc output theo chương 06:

- **Loại file.** `ET_DYN` cộng `PT_INTERP` là một PIE động: kernel phân biệt hai loại `ET_DYN`, chương trình (PIE, có `INTERP`) và loader (không có `INTERP`)[^binfmt-pie]; trường `e_type` nhận `ET_REL`, `ET_EXEC`, `ET_DYN`[^elf-etype]. Interpreter là `/lib/ld-linux-aarch64.so.1`: kernel map cả chương trình lẫn interpreter rồi nhảy vào *interpreter* chứ không vào chương trình[^binfmt-interp-load][^dr-entry]. `entry: 0x7500` là địa chỉ ảo trong file[^elf-entry]; vì là PIE nên địa chỉ thật lúc chạy là base ngẫu nhiên cộng số này (suy luận từ cách kernel đặt `load_bias`[^binfmt-load-bias]).
- **Segment (góc nhìn của loader).** Hai `LOAD`: `R E` (mã và dữ liệu chỉ đọc) và `RW`. Segment `RW` có `filesz` 0x2458 nhưng `memsz` 0x2510: phần dư là vùng zero-fill, thứ `.bss` cần[^elf-load][^elf-load-zero]. Chú ý `offset` 0x4e548 nhưng `vaddr` 0x5e548: chênh 0x10000, đúng bằng `align` của segment — ở segment này địa chỉ ảo và vị trí trong file *khác nhau*, nên đổi một symbol sang offset trong file phải đi qua đúng segment chứa nó, việc `elf_probe sym` làm ở phần 4. `GNU_STACK` có cờ `RW` (không có `E`): `elf(5)` nói kernel Linux dùng `p_flags` của segment này để điều khiển trạng thái stack[^elf-gnustack]; hiểu `RW` là stack không thực thi là suy luận của người soạn.
- **Dynamic section (góc nhìn của `ld.so`).** `NEEDED libgcc_s.so.1` và `libc.so.6`: mỗi dependency là một entry `DT_NEEDED`[^elf-needed]. `RPATH` và `RUNPATH` đều là `-` (`DT_RUNPATH` là chuỗi chứa đường dẫn tìm thư viện[^elf-dt-runpath]), nên theo thứ tự ở chương 06 mục 4 các thư viện này được tìm qua `ld.so.cache`[^ldso-order-cache] (cộng `LD_LIBRARY_PATH` nếu bạn đặt). `FLAGS 0x8` là `DF_BIND_NOW` — cờ bảo `ld.so` xử lý mọi relocation trước khi chuyển điều khiển, tức không có lazy binding[^gabi-dtbindnow] — và `-z now` chính là cách linker đặt cờ đó[^dr-bindnow]. Cùng với `PT_GNU_RELRO` (linker chuyển các section chỉ bị relocation sửa sang trang riêng, `ld.so` gỡ quyền ghi sau khi xong[^dr-relro]) đó là *Full RELRO*: GOT được điền sớm rồi chuyển thành chỉ-đọc[^rs-relro-full]. Đúng mặc định của rustc: Full RELRO trên nền tảng hỗ trợ[^rs-relro-default], và target Linux đặt `relro_level: Full`[^rs-base-linux].
- **Symbol và unwind.** `.symtab` vắng, `.dynsym` còn 62 symbol (lab (e) giải thích vì sao); `.eh_frame` và `.eh_frame_hdr` còn nguyên; có build ID.

Section `.interp` là chuỗi mà `PT_INTERP` trỏ tới. Dump nó từ file `counter`:

```bash
cargo run -q -p lab-observability --bin elf_probe -- dump target-linux/release/counter .interp
```

```text
section .interp: 27 bytes (showing 27)
  0x0000  2f 6c 69 62 2f 6c 64 2d 6c 69 6e 75 78 2d 61 61  |/lib/ld-linux-aa|
  0x0010  72 63 68 36 34 2e 73 6f 2e 31 00                 |rch64.so.1.|
```

`PT_INTERP` là đường dẫn kết thúc bằng null của interpreter cần gọi[^elf-interp]: 26 ký tự và byte 0 cuối, tổng 27 byte, khớp `filesz 0x1b` trong bảng segment ở trên.

**Phần 3 — một binary Linux tĩnh (musl).**

```bash
cargo run -q -p lab-observability --bin elf_probe -- target-linux/aarch64-unknown-linux-musl/release/hello
```

```text
file: target-linux/aarch64-unknown-linux-musl/release/hello (397344 bytes)
ELF header:
  type:     EXEC (executable file)
  machine:  AArch64
  entry:    0x404568
  segments: 7 program headers at offset 64 (56 bytes each)
  sections: 18 section headers at offset 396192 (64 bytes each)
kind: ET_EXEC, no PT_INTERP: static executable at a fixed address (not PIE)
interpreter: none (no PT_INTERP)
segments (the loader's view):
  type          flags     offset        vaddr    filesz     memsz   align
  LOAD          R E          0x0     0x400000   0x52a50   0x52a50 0x10000
  LOAD          RW       0x5e9b0     0x46e9b0    0x2108    0x3040 0x10000
  NOTE          R          0x1c8     0x4001c8      0x24      0x24     0x4
  TLS           R        0x5e9b0     0x46e9b0       0x0      0x18     0x8
  GNU_EH_FRAME  R        0x4a460     0x44a460    0x1064    0x1064     0x4
  GNU_STACK     RW           0x0          0x0       0x0       0x0    0x10
  GNU_RELRO     R        0x5e9b0     0x46e9b0    0x1650    0x1650     0x1
dynamic (what ld.so reads): none (no PT_DYNAMIC)
PT_GNU_RELRO present: yes
symbols and debug data (the tools' view):
  .symtab: absent
  .dynsym: absent
  .debug_* sections: none
  .eh_frame: present, 26544 bytes
  .eh_frame_hdr: present, 4196 bytes
  .gnu_debuglink: absent
  build id: 79a86e84019a11cfd6ad769ace4777c7a677ba81
```

So với `hello` glibc: `e_type` là `ET_EXEC`, **không có `PT_INTERP`, không có `PT_DYNAMIC`** nên không có gì cho `ld.so` làm, và segment `LOAD` đầu nằm ở địa chỉ ảo cố định `0x400000`. Executable truyền thống chứa mã tuyệt đối nên các segment phải nằm đúng địa chỉ ảo đã dùng khi build[^gabi-exec-abs], và kernel map `PT_LOAD` tại `load_bias + vaddr`, với `load_bias` bằng 0 cho `ET_EXEC`[^binfmt-elf-load]: đây là binary *không* PIE (chương 06 mục 6). Đây là một quan sát đáng nhớ: chương 06 mục 6 chỉ nói spec của `x86_64-unknown-linux-musl` khai báo static-PIE được hỗ trợ[^rs-x64-musl] và dặn tự kiểm bằng `e_type` và `PT_INTERP`; binary `aarch64` musl ở đây cho `ET_EXEC`, nên *đừng suy từ x86_64 sang aarch64*. `PT_GNU_RELRO` vẫn có mặt. Mặc định `crt-static` của target này và các khác biệt glibc/musl nằm ở chương 06 mục 7, người soạn không đo ở đây.

**Phần 4 — từ tên hàm đến offset của uprobe.** `elf_probe sym` tìm symbol theo tên con và đổi `st_value` thành offset trong file qua `PT_LOAD` chứa nó (lab (b) dùng offset này cho `uprobe_events`):

```bash
cargo run -q -p lab-observability --bin elf_probe -- sym target-linux/release/counter counter4main
```

```text
symbols containing "counter4main" in target-linux/release/counter
  [.symtab] _ZN7counter4main17hc1badc18f9d1a320E value=0xe180 size=1520 type=2 binding=1
           uprobe offset (value -> file offset through its PT_LOAD): 0xe180
```

Tên `_ZN7counter4main17hc1badc18f9d1a320E` là tên mangle kiểu *legacy* (`_ZN`, độ dài từng thành phần, hậu tố hash `17h…E`): đó là lý do chương 02 mục 9 dặn đừng viết script phụ thuộc đúng chuỗi mangled và cần `#[unsafe(no_mangle)]` cho hàm "mốc". Tìm bằng `main` thì ra rất nhiều dòng (người soạn thấy cả tên `_R…` kiểu v0 của std/core lẫn `_ZN…` của `counter`, trong cùng một binary; hai định dạng mangling cùng tồn tại[^mang-legacy][^cg-mangling]). Với segment này, `p_vaddr` bằng `p_offset` nên offset bằng đúng `value` (`0xe180`); với segment `RW` ở phần 2 thì không. Các section của file này, chọn lọc:

```bash
cargo run -q -p lab-observability --bin elf_probe -- target-linux/release/counter --sections \
  | grep -E ' \.(interp|dynsym|plt|text|rodata|eh_frame|data|bss|symtab) |^ +nr '
```

```text
    nr name                     type      flags          addr     offset       size
     1 .interp                  PROGBITS  A             0x270      0x270       0x1b
     5 .dynsym                  DYNSYM    A             0x2f0      0x2f0      0x750
    12 .plt                     PROGBITS  AX           0x48a0     0x48a0      0x4b0
    13 .text                    PROGBITS  AX           0x4d80     0x4d80    0x448a4
    15 .rodata                  PROGBITS  A           0x49640    0x49640     0x7170
    17 .eh_frame                PROGBITS  A           0x51fb0    0x51fb0     0x95f0
    26 .data                    PROGBITS  WA          0x70000    0x70000      0x9c0
    27 .bss                     NOBITS    WA          0x709c0    0x709c0       0xe8 (no file bytes)
    29 .symtab                  SYMTAB                    0x0    0x70a30    0x11d30
```

`.symtab` không có cờ `A` (`SHF_ALLOC`): nó không nằm trong ảnh bộ nhớ lúc chạy, nên strip nó đi mà chương trình vẫn chạy[^elf-shf-alloc]; `.bss` là `NOBITS` — chiếm chỗ trong bộ nhớ nhưng không chiếm byte nào trong file[^elf-nobits].

**Phần 5 — ASLR trên macOS.** `aslr_probe` in địa chỉ của code (`main`), một `static`, một hàm libc (`getpid`), một `Box` trên heap, một trang `mmap` ẩn danh và một biến trên stack. Chạy hai lần:

```bash
cargo run -q -p lab-observability --bin aslr_probe
cargo run -q -p lab-observability --bin aslr_probe
```

```text
pid                       52228
code    fn main           0x00000001042a48d0
data    a static          0x00000001042de650
libc    getpid            0x0000000186542178
heap    Box<u64>          0x0000000104bbde70
mmap    anonymous page    0x000000010435c000
stack   local variable    0x000000016bb59800
kernel.randomize_va_space = n/a (no /proc/sys/kernel/randomize_va_space: not Linux)

pid                       52229
code    fn main           0x0000000102b608d0
data    a static          0x0000000102b9a650
libc    getpid            0x0000000186542178
heap    Box<u64>          0x00000001030dde70
mmap    anonymous page    0x0000000102d08000
stack   local variable    0x000000016d29d800
kernel.randomize_va_space = n/a (no /proc/sys/kernel/randomize_va_space: not Linux)
```

Quan sát (macOS aarch64): địa chỉ của code, `static`, heap, `mmap` và stack **đều khác** giữa hai lần chạy; nhưng hiệu `static − main` luôn là `0x39d80` ở cả hai lần: bố cục *bên trong* binary không đổi, chỉ cái base trượt đi, và phần dưới của mỗi địa chỉ code và `static` (`…8d0`, `…650`) giữ nguyên. Địa chỉ `getpid` của libc **không đổi** (`0x186542178`) giữa hai lần chạy. Người soạn không giải thích điều đó: nó là hành vi của macOS, nằm ngoài các nguồn Linux của chương 06. Đây cũng là lý do bạn không thể rút ra cơ chế của Linux từ output này.

**Phần Linux (chưa chạy): ASLR, loader và `readelf`.**

```bash
./labs/linux.sh bash -c '
  cargo build -q -p lab-observability --bins && B=target-linux/debug/aslr_probe
  cat /proc/sys/kernel/randomize_va_space
  $B | sed -n "2,7p"; $B | sed -n "2,7p"                                     # hai lần chạy bình thường
  setarch $(uname -m) -R $B | sed -n "2,7p"; setarch $(uname -m) -R $B | sed -n "2,7p"   # tắt ASLR
  readelf -h $B | grep Type
  readelf -lW $B | grep -E "INTERP|LOAD|GNU_RELRO"
  readelf -d $B | grep -E "NEEDED|FLAGS"
  ldd $B                                                                     # chỉ với binary của chính bạn
  LD_DEBUG=libs $B 2>&1 | head -20
  cargo run -q -p lab-observability --bin elf_probe -- $B | head -40          # đối chiếu với readelf
'                                                                             # chưa chạy
```

Điều cần tìm:

- **ASLR.** `randomize_va_space` bằng 1 ngẫu nhiên hóa base của mmap, stack và trang vDSO, và với binary PIE cả điểm bắt đầu của code[^aslr-1]; bằng 2 thêm heap[^aslr-2]; bằng 0 tắt[^aslr-0]. Kỳ vọng ở mức 2: hai lần chạy cho địa chỉ code, heap, stack, mmap và địa chỉ hàm libc **khác nhau** (khác với macOS, nơi `getpid` đứng yên). Code đổi vì binary là PIE và kernel cộng `arch_mmap_rnd()` vào `load_bias` khi cờ `PF_RANDOMIZE` bật[^binfmt-load-bias]. `setarch -R` tắt ngẫu nhiên hóa không gian địa chỉ (bật `ADDR_NO_RANDOMIZE`)[^setarch-R], cờ mà kernel kiểm khi quyết định đặt `PF_RANDOMIZE`[^binfmt-pfrandomize]: kỳ vọng hai lần chạy dưới `setarch -R` cho đúng các số giống nhau (suy ra từ hai nguồn đó). Địa chỉ trong crash log của binary PIE đổi theo từng lần chạy: khi symbolize cần địa chỉ nạp (chương 06 mục 6).
- **`readelf` đối chiếu với `elf_probe`.** `readelf -h` hiển thị ELF header[^readelf-h], `-l` các segment[^readelf-l], `-d` section dynamic[^readelf-d], và `-S` các section[^readelf-S]; `-W` giữ mỗi dòng không bị ngắt ở cột 80[^readelf-W]. Kỳ vọng với `aslr_probe` build trong container: `Type: DYN`, có `INTERP`, hai `LOAD` (`R E` và `RW`), `GNU_RELRO`, `NEEDED libc.so.6` và `FLAGS` có `BIND_NOW`; mọi trường đó phải khớp từng dòng với output của `elf_probe` cho cùng file, vì cả hai đọc cùng các byte.
- **`ldd` và `LD_DEBUG`.** `ldd` gọi dynamic linker chuẩn với `LD_TRACE_LOADED_OBJECTS` bằng 1[^ldd-trace]; vì có thể dẫn tới thực thi code tùy ý, đừng bao giờ chạy nó trên executable không tin cậy[^ldd-sec] (ở đây là binary do chính bạn build). `LD_DEBUG` in thông tin chi tiết về hoạt động của dynamic linker[^ldso-debug] và hạng mục `libs` hiển thị các đường dẫn tìm thư viện[^ldso-debug-libs]: hãy ghi lại nguyên văn dòng log cho `libc.so.6` và đối chiếu thứ tự tìm ở chương 06 mục 4 (người soạn không dự đoán định dạng dòng log).
- **Static so với dynamic.** `rustup target add aarch64-unknown-linux-musl` rồi `cargo build --target aarch64-unknown-linux-musl --bin aslr_probe` trong container, và chạy `elf_probe` trên kết quả: kỳ vọng (từ phần 3, chưa chạy với `aslr_probe`) không có `PT_INTERP`; hãy kiểm `e_type` thay vì suy ra, và chạy nó hai lần xem địa chỉ code có đổi không.

### Bài tập mở rộng

1. (a) Trong container, build `hotspot` bản release và chạy `elf_probe sym target-linux/release/hotspot hot_checksum`. Nếu symbol có mặt, `#[inline(never)]` đã giữ được hàm; nếu không, hoặc hàm đã bị inline hết (thuộc tính chỉ là gợi ý[^inline-hint]) hoặc binary đã bị strip[^cg-strip-sym]. Rồi bỏ `#[inline(never)]` khỏi `process_batch`, build lại và xem `perf report` còn thấy frame đó không.
2. (a) Lập bảng ba cấu hình unwinding ở lab (a): frame pointer tắt + `fp`, bật + `fp`, và `--call-graph dwarf`. Với mỗi cấu hình ghi: stack của `hot_checksum` đủ bao nhiêu frame, kích thước `perf.data`, và `Self`/`Children` của `main`. Nếu CPU của bạn có LBR[^rec-lbr], thêm `--call-graph lbr`.
3. (a), (c) Tìm thời gian off-CPU: dùng cách tracing các hàm kernel chuyển thread ra khỏi CPU kèm stack[^off-trace] (hoặc một bpftrace tương đương bạn tự viết sau khi đọc Gregg) để thấy `sleep` của `hotspot` và thời gian chờ khóa của `latency naive`; nhớ cảnh báo overhead khi sự kiện scheduler rất dày[^off-warn].
4. (b) Đo overhead của probe: chạy `trace_target unbuffered 200000` ba lần không có gì, ba lần dưới `bpftrace` với uprobe `handle_request`, ba lần dưới `strace -f -c`, và so `elapsed_ms` in ra. Rồi thử uretprobe để đo latency của `handle_request` bằng mẫu `kprobe`/`kretprobe` của tutorial (lưu `nsecs` theo `tid`, tính hiệu ở return probe): chương 02 chỉ có ví dụ Go cho uretprobe[^bt-uretprobe-go], hãy ghi lại điều gì xảy ra với Rust.
5. (b) ftrace thuần: gắn tracefs ở `/sys/kernel/tracing`[^ftrace-mount], đặt `current_tracer` thành `function_graph` (theo dõi cả lúc vào lẫn lúc ra hàm[^ftrace-fngraph]), giới hạn bằng `set_ftrace_filter` chỉ còn `vfs_write`[^ftrace-filter], chạy `trace_target unbuffered 2000` và đọc `trace`. Nhớ `tracing_on = 0` chỉ dừng việc ghi vào ring buffer, overhead có thể vẫn còn[^ftrace-tracing-on]. Nâng cao: cho `trace_target` mở `trace_marker` và ghi dấu "bắt đầu request" để đặt cạnh các event kernel[^ftrace-marker].
6. (c) Thay `parking_lot::Mutex` trong `lab_observability::latency` bằng `std::sync::Mutex` (và `.lock().unwrap()`), chạy lại lab và so p99 của `naive`. `parking_lot` dùng eventual fairness[^pl-fair]; hãy dự đoán chiều thay đổi trước rồi đo.
7. (c) Biến tải thành gần open-loop: ghi `record_correct(latency, khoảng_cách_kỳ_vọng)`[^hdr-record-correct] thay cho `saturating_record`, hoặc `clone_correct` cuối lần chạy[^hdr-clone-correct], và so p99 trước/sau hiệu chỉnh của `naive`.
8. (c) Đặt `heavy_every = 100` (tham số thứ tư) rồi `heavy_every = 50`. Dự đoán trước p99 của `tuned` nằm ở đâu khi 1% rồi 2% request là report, đối chiếu với phép tính nearest-rank của `exact_quantile` trong crate, rồi chạy.
9. (c) PGO bốn bước cho `latency`: build có instrument, chạy workload đại diện sinh `.profraw`, `llvm-profdata merge`, build lại với `-Cprofile-use`[^pgo-steps]; `llvm-profdata` lấy qua `rustup component add llvm-tools-preview` và không được đưa vào `PATH`[^pgo-llvmtools]. Ghi: workload ở bước hai đại diện cho cái gì, và p99 có đổi không.
10. (d) Thêm biến thể thứ tư vào `spans`: bọc phần đồng bộ bằng `Span::in_scope`, sao cho span luôn thoát trước điểm `.await` kế tiếp[^tr-in-scope]; viết test dạng ghim transcript cho nó.
11. (e) Mở `debug_target` trong `rust-gdb` và ghi lại cách GDB hiển thị `Status` ở từng biến thể (`New`, `Paid`, `Shipped`, `Cancelled`) so với hàm `describe()`; thử `print pricing.name()` rồi đối chiếu với điều chương 04 nói về trait và DWARF[^rdg-traits].
12. (e) Tách debug info: build `counter` hoặc `debug_target` ở profile dev, `objcopy --only-keep-debug foo foo.debug`, `strip -g foo`, rồi `objcopy --add-gnu-debuglink=foo.debug foo`[^gdb-objcopy]; chạy `elf_probe` trước và sau để thấy `.debug_*` biến mất khỏi `foo` và `.gnu_debuglink` xuất hiện. Thử thay bằng `CARGO_PROFILE_DEV_SPLIT_DEBUGINFO=packed` và quan sát file `*.dwp` trên Linux[^rustc-split-packed].
13. (f) Tạo core của `crash_lab segv` ở bản dev và bản release trong container, mở cả hai bằng `gdb` và so frame trên cùng của `bt`: lab (f) dự đoán signal khác nhau (`SIGABRT` so với `SIGSEGV`) và vì thế stack khác nhau.
14. (g) Build `aslr_probe` cho `aarch64-unknown-linux-musl` và so với bản glibc: `e_type`, `PT_INTERP`, `PT_DYNAMIC`, `NEEDED`, kích thước file, và địa chỉ có đổi giữa hai lần chạy không. Rồi thử `RUSTFLAGS="-C target-feature=-crt-static"`[^rs-crt-flag] và `-C relocation-model=pic`[^rs-reloc-crt] để xem `e_type` đổi thế nào.

## Câu hỏi tự kiểm tra (về output lab)

1. Ở lab (a), pha `sleep` chiếm 35,9% thời gian tường nhưng không thể hiện trong `perf record` mặc định. Vì sao, và `perf stat` sẽ báo "CPUs utilized" cỡ bao nhiêu?
   <details><summary>Đáp án</summary>

   Pha ngủ chỉ có 0,4 ms CPU trên 362,8 ms tường: thread bị chặn, tức off-CPU[^off-def]. Sampling theo ngắt timer của từng CPU chỉ thấy cái đang chạy nên cho profile on-CPU[^off-timer]. `task-clock` cỡ tổng CPU (≈ 638 ms) chia thời gian trôi qua (≈ 1009 ms) cho "CPUs utilized" ≈ 0,63[^stat-taskclock] — đúng `cpu/wall = 0.63` mà chương trình tự in.

   </details>

2. Ở lab (b), `unbuffered` có `write_calls=20000` và `buffered` có 21 với cùng 168.890 byte. `strace -f -c -e trace=write` sẽ báo bao nhiêu `write` cho từng chế độ, và vì sao không đúng bằng hai số đó?
   <details><summary>Đáp án</summary>

   Khoảng 20.002 và 23: số lần ghi của lab cộng các lần ghi của chính chương trình (dòng `pid=` ra stderr, dòng kết quả ra stdout). `strace -c` đếm lần gọi từng syscall[^strace-c], `-e trace=write` lọc[^strace-trace], `-f` theo cả thread[^strace-f]. `File` không đệm[^rs-file-nobuf] nên mỗi dòng là một `write`; `BufWriter` 8 KiB gom thành ít lần ghi lớn[^bufwriter-8k][^bufwriter-batch]. (Kỳ vọng, chưa chạy.)

   </details>

3. Ở lab (c), trung bình của `naive` là 4,83 µs còn p99 là 188,54 µs. Điều đó nói gì, vì sao chạy 1 thread thì biến mất, và vì sao số cấp phát không giải thích được nó?
   <details><summary>Đáp án</summary>

   Trung bình che đuôi[^sre-100ms]: khoảng 1% request chờ cỡ thời gian của một report đang giữ khóa (nhận xét của người soạn). Với 1 thread không ai chờ ai nên p99 chỉ 0,54 µs: nguyên nhân là tranh chấp cộng việc nặng trong vùng giữ khóa. Cấp phát 2,005 lần mỗi request so với gần 0 không làm p50 khác biệt ổn định, nên không phải thủ phạm; chương 03 nói cấp phát *có thể* tốn vì khóa toàn cục hay syscall[^pb-heap-cost], "có thể" nghĩa là phải đo.

   </details>

4. Ở lab (d), vì sao dòng `event resume reported in [handle{task=A} > handle{task=B}]` ở khối `enter-across-await` là sai, và khối `instrument` sửa nó bằng cách nào?
   <details><summary>Đáp án</summary>

   Event `resume` là của task A, nhưng A đã nhường mà guard của `Span::enter` vẫn sống: scope bị thoát ở `.await` nhưng giá trị trong scope không bị drop, nên task khác (B) chạy tiếp trong khi vẫn "ở trong" span của A[^tr-enter-why]. B được tạo trong span của A (cha là A) và subscriber coi A cũng nằm trong B: trace sai[^tr-enter-warn]. `.instrument(span)` vào span mỗi lần poll và thoát khi future nhường[^tr-instrument-combinator], nên mỗi event nằm đúng span của task mình và không có span cha-con giả.

   </details>

5. Ở lab (e), bản release mặc định (`counter`) có `.symtab` nhưng không có `.debug_*`. Backtrace, `perf` và `gdb` làm được và không làm được gì với nó?
   <details><summary>Đáp án</summary>

   Có tên hàm (từ `.symtab`) nhưng không có số dòng, biến cục bộ hay kiểu: `debug` mặc định của release là không có debug info[^prof-debug], và `strip` mặc định `"none"`[^prof-strip]. `std::backtrace` cần debug info mới có tên file và số dòng[^bt-debuginfo]; `perf report -s srcline` cần DWARF[^rep-srcline]; GDB coi symbol đó là non-debugging symbol, không có kiểu[^gdb-nodebuginfo]. Thêm `debug = "line-tables-only"` cho số dòng[^pb-debugline].

   </details>

6. Ở lab (f), cùng dòng `*p = 1` với con trỏ null cho `exit=134` ở dev và `exit=139` ở release. Giải thích hai con số, và cho biết có sinh core trong cả hai trường hợp không.
   <details><summary>Đáp án</summary>

   Shell đặt mã thoát `128 + N` khi lệnh chết vì signal `N`[^bash-128n]: 134 = 128 + 6 (`SIGABRT`)[^sig-num-abrt] và 139 = 128 + 11 (`SIGSEGV`)[^sig-num-segv]. Dev có assertion mà compiler chèn khi bật debug assertions (từ Rust 1.86): non-unwinding panic rồi abort[^rust186-null][^rust186-nounwind]; release thì CPU thật sự ghi vào địa chỉ 0 và kernel gửi `SIGSEGV`. Cả hai đều là signal có hành động mặc định `Core`[^sg-abrt][^sg-segv] nên *có thể* sinh core, miễn `RLIMIT_CORE` và `core_pattern` cho phép.

   </details>

7. Trên macOS `preflight` báo giới hạn mềm `RLIMIT_CORE` là 0. Trên Linux với `core_pattern` bắt đầu bằng `|`, giới hạn đó còn có tác dụng không?
   <details><summary>Đáp án</summary>

   Không: `RLIMIT_CORE` bị bỏ qua khi hệ thống pipe core sang một chương trình[^core-rlimit-zero]; core vào stdin của chương trình đó[^core-pipe] và nó quyết định giữ hay bỏ. Với tên file thì 0 nghĩa là không tạo core file[^rl-core]. Test `rlimit_zero_does_not_stop_a_piped_core` ghim đúng trường hợp này.

   </details>

8. Từ output của `elf_probe` cho `hello` glibc và `hello` musl, hai binary khác nhau ở những trường nào, và `FLAGS 0x8` cùng `GNU_RELRO` của bản glibc nghĩa là gì?
   <details><summary>Đáp án</summary>

   glibc: `ET_DYN` + `PT_INTERP` (`/lib/ld-linux-aarch64.so.1`) + `PT_DYNAMIC` với `NEEDED libc.so.6`: PIE động[^binfmt-pie]. musl: `ET_EXEC`, không có `PT_INTERP` và `PT_DYNAMIC`, `LOAD` ở `0x400000`: static, không PIE[^gabi-exec-abs][^binfmt-elf-load]. `FLAGS 0x8` là `DF_BIND_NOW` (không lazy binding)[^gabi-dtbindnow] và `GNU_RELRO` là vùng bị khóa chỉ-đọc sau khi relocate[^dr-relro]: cả hai là Full RELRO, mặc định của rustc trên Linux[^rs-relro-default][^rs-relro-full].

   </details>

[^loadavg]: getloadavg(3), DESCRIPTION (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/getloadavg.3.en.html> — "The getloadavg() function returns the number of processes in the system run queue averaged over various periods of time."
[^clock-thread]: clock_gettime(2), CLOCK_THREAD_CPUTIME_ID (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/clock_gettime.2.en.html> — "This is a clock that measures CPU time consumed by this thread."
[^strace-c]: strace(1), Statistics (Debian bookworm) — <https://manpages.debian.org/bookworm/strace/strace.1.en.html> — "Count time, calls, and errors for each system call and report a summary on program exit, suppressing the regular output."
[^rustup-minimal]: rust-lang/docker-rust, stable/bookworm/Dockerfile — <https://raw.githubusercontent.com/rust-lang/docker-rust/master/stable/bookworm/Dockerfile> — "./rustup-init -y --no-modify-path --profile minimal --default-toolchain"
[^rustup-profile-min]: The rustup book, Profiles — <https://rust-lang.github.io/rustup/concepts/profiles.html> — "The minimal profile includes as few components as possible to get a working compiler (rustc, rust-std, and cargo)."
[^docker-rust-base]: rust-lang/docker-rust, stable/bookworm/Dockerfile — <https://raw.githubusercontent.com/rust-lang/docker-rust/master/stable/bookworm/Dockerfile> — "FROM buildpack-deps:bookworm LABEL org.opencontainers.image.source=https://github.com/rust-lang/docker-rust"
[^readelf-desc]: readelf(1), NAME (Debian bookworm, binutils-common) — <https://manpages.debian.org/bookworm/binutils-common/readelf.1.en.html> — "readelf - display information about ELF files"
[^nm-desc]: nm(1), DESCRIPTION (Debian bookworm, binutils-common) — <https://manpages.debian.org/bookworm/binutils-common/nm.1.en.html> — "GNU nm lists the symbols from object files objfile"
[^perf-desc]: perf(1), DESCRIPTION (Debian bookworm, linux-perf) — <https://manpages.debian.org/bookworm/linux-perf/perf.1.en.html> — "It covers hardware level (CPU/PMU, Performance Monitoring Unit) features and software features (software counters, tracepoints) as well."
[^bt-ver]: bpftrace(8), Debian bookworm — <https://manpages.debian.org/bookworm/bpftrace/bpftrace.8.en.html> — "Source file: bpftrace.8.en.gz (from bpftrace 0.17.0-1)"
[^cap-bpf-since]: capabilities(7), Debian bookworm — <https://manpages.debian.org/bookworm/manpages/capabilities.7.en.html> — "This capability was added in Linux 5.8 to separate out BPF functionality from the overloaded CAP_SYS_ADMIN capability."
[^cap-perfmon-since]: capabilities(7), Debian bookworm — <https://manpages.debian.org/bookworm/manpages/capabilities.7.en.html> — "This capability was added in Linux 5.8 to separate out performance monitoring functionality from the overloaded CAP_SYS_ADMIN capability."
[^docker-privileged]: Docker Docs, docker run, runtime privilege and Linux capabilities — <https://docs.docker.com/engine/containers/run/> — "The --privileged flag gives all capabilities to the container."
[^llvm-dwarfdump]: LLVM Command Guide, llvm-dwarfdump — <https://llvm.org/docs/CommandGuide/llvm-dwarfdump.html> — "llvm-dwarfdump - dump and verify DWARF debug information"
[^sd-svc]: systemd-coredump(8), DESCRIPTION — <https://manpages.debian.org/bookworm/systemd-coredump/systemd-coredump.8.en.html> — "It will log a summary of the event to systemd-journald.service(8), including information about the process identifier, owner, the signal that killed the process, and the stack trace if possible."
[^docker-capadd]: Docker Docs, docker run, runtime privilege and Linux capabilities — <https://docs.docker.com/engine/containers/run/> — "the operator can have fine grain control over the capabilities using --cap-add and --cap-drop."
[^dock-block]: Docker Docs, Seccomp security profiles for Docker (Significant syscalls blocked by the default profile) — <https://docs.docker.com/engine/security/seccomp/> — "Tracing/profiling syscall, which could leak a lot of information on the host."
[^dock-unconf]: Docker Docs, Seccomp security profiles for Docker (Run without the default seccomp profile) — <https://docs.docker.com/engine/security/seccomp/> — "You can pass unconfined to run a container without the default seccomp profile."
[^ksys-default]: Documentation for /proc/sys/kernel/, perf_event_paranoid (kernel docs) — <https://docs.kernel.org/admin-guide/sysctl/kernel.html> — "Controls use of the performance events system by unprivileged users (without CAP_PERFMON). The default value is 2."
[^ksec-level2]: Perf events and tool security, Unprivileged users (kernel docs) — <https://docs.kernel.org/admin-guide/perf-security.html> — "scope includes per-process performance monitoring only. CPU and system events happened when executing in user space only can be monitored and captured for later analysis."
[^docker-vmm]: Docker Docs, Virtual Machine Manager — <https://docs.docker.com/desktop/features/vmm/> — "Docker Desktop supports multiple Virtual Machine Managers (VMMs) to power the Linux VM that runs containers."
[^peo-enodev]: perf_event_open(2), ERRORS (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/perf_event_open.2.en.html> — "Returned when the event involves a feature not supported by the current CPU."
[^peo-eopnotsupp]: perf_event_open(2), ERRORS (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/perf_event_open.2.en.html> — "Returned if an event requiring a specific hardware feature is requested but there is no hardware support. This includes requesting low-skid events if not supported, branch tracing if it is not available, sampling if no PMU interrupt is available, and branch stacks for software events."
[^peo-cpuclock]: perf_event_open(2), PERF_COUNT_SW_CPU_CLOCK (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/perf_event_open.2.en.html> — "This reports the CPU clock, a high-resolution per-CPU timer."
[^peo-sw]: perf_event_open(2), perf_event_attr.type (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/perf_event_open.2.en.html> — "This indicates one of the software-defined events provided by the kernel (even if no hardware support is available)."
[^inline-never]: The Rust Reference, The inline attribute — <https://doc.rust-lang.org/1.95.0/reference/attributes/codegen.html> — "#[inline(never)] suggests that inline expansion should never be performed."
[^inline-hint]: The Rust Reference, The inline attribute — <https://doc.rust-lang.org/1.95.0/reference/attributes/codegen.html> — "In every form the attribute is a hint. The compiler may ignore it."
[^off-def]: Brendan Gregg, Off-CPU Analysis — <https://www.brendangregg.com/offcpuanalysis.html> — "Off-CPU: where time is spent waiting while blocked on I/O, locks, timers, paging/swapping, etc."
[^off-timer]: Brendan Gregg, Off-CPU Analysis — <https://www.brendangregg.com/offcpuanalysis.html> — "Sampling is usually implemented as per-CPU timer interrupts, that then examine the currently running process that was interrupted: producing an on-CPU profile."
[^off-comp]: Brendan Gregg, Off-CPU Analysis — <https://www.brendangregg.com/offcpuanalysis.html> — "Off-CPU analysis is complementary to CPU analysis, so that 100% of thread time can be understood."
[^rep-overhead]: perf-report(1), --fields — <https://manpages.debian.org/bookworm/linux-perf/perf-report.1.en.html> — "overhead: Overhead percentage of sample"
[^stat-taskclock]: perf-stat(1), ví dụ output (Debian bookworm, linux-perf) — <https://manpages.debian.org/bookworm/linux-perf/perf-stat.1.en.html> — "task-clock:u (msec) # 1.004 CPUs utilized"
[^env-debug]: Cargo Reference 1.95.0, Environment variables — <https://doc.rust-lang.org/1.95.0/cargo/reference/environment-variables.html> — "CARGO_PROFILE_\<name>_DEBUG — What kind of debug info to include, see profile.\<name>.debug."
[^pb-debugline]: The Rust Performance Book, Profiling (Debug Info) — <https://nnethercote.github.io/perf-book/profiling.html> — "[profile.release] debug = "line-tables-only""
[^cg-ltonly]: rustc book 1.95.0, Codegen options (debuginfo) — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "line-tables-only: line tables only. Generates the minimal amount of debug info for backtraces with filename/line number info, but not anything else, i.e. no variable or function parameter info."
[^rep-srcline]: perf-report(1), --sort srcline (Debian bookworm, linux-perf) — <https://manpages.debian.org/bookworm/linux-perf/perf-report.1.en.html> — "srcline: filename and line number executed at the time of sample. The DWARF debugging info must be provided."
[^pb-fp-flag]: The Rust Performance Book, Profiling (Frame pointers) — <https://nnethercote.github.io/perf-book/profiling.html> — "To force the compiler to use frame pointers, use the -C force-frame-pointers=yes flag."
[^cg-fp-flag]: rustc book 1.95.0, Codegen options (force-frame-pointers) — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "This flag forces the use of frame pointers."
[^pb-fp]: The Rust Performance Book, Profiling (Frame pointers) — <https://nnethercote.github.io/perf-book/profiling.html> — "The Rust compiler may optimize away frame pointers, which can hurt the quality of profiling information such as stack traces."
[^rec-cg-default]: perf-record(1), --call-graph (Debian bookworm, linux-perf) — <https://manpages.debian.org/bookworm/linux-perf/perf-record.1.en.html> — "Default is "fp" (for user space)."
[^rec-fomit]: perf-record(1), --call-graph (Debian bookworm, linux-perf) — <https://manpages.debian.org/bookworm/linux-perf/perf-record.1.en.html> — "In some systems, where binaries are build with gcc --fomit-frame-pointer, using the "fp" method will produce bogus call graphs, using "dwarf", if available (perf tools linked to the libunwind or libdw library) should be used instead."
[^rec-freq]: perf-record(1), -F (Debian bookworm, linux-perf) — <https://manpages.debian.org/bookworm/linux-perf/perf-record.1.en.html> — "Profile at this frequency. Use max to use the currently maximum allowed frequency, i.e. the value in the kernel.perf_event_max_sample_rate sysctl. Will throttle down to the currently maximum allowed frequency."
[^rec-g]: perf-record(1), -g (Debian bookworm, linux-perf) — <https://manpages.debian.org/bookworm/linux-perf/perf-record.1.en.html> — "Enables call-graph (stack chain/backtrace) recording for both kernel space and user space."
[^rep-default-sort]: perf-report(1), --sort (Debian bookworm, linux-perf) — <https://manpages.debian.org/bookworm/linux-perf/perf-report.1.en.html> — "By default, comm, dso and symbol keys are used."
[^rep-nochildren]: perf-report(1), --children (Debian bookworm, linux-perf) — <https://manpages.debian.org/bookworm/linux-perf/perf-report.1.en.html> — "Enabled by default, disable with --no-children."
[^rep-sum]: perf-report(1), OVERHEAD CALCULATION (Debian bookworm, linux-perf) — <https://manpages.debian.org/bookworm/linux-perf/perf-report.1.en.html> — "It might be confusing that the sum of all the children overhead values exceeds 100% since each of them is already an accumulation of self overhead of its child functions."
[^rep-self]: perf-report(1), OVERHEAD CALCULATION (Debian bookworm, linux-perf) — <https://manpages.debian.org/bookworm/linux-perf/perf-report.1.en.html> — "The self overhead is simply calculated by adding all period values of the entry - usually a function (symbol)."
[^rep-children]: perf-report(1), OVERHEAD CALCULATION (Debian bookworm, linux-perf) — <https://manpages.debian.org/bookworm/linux-perf/perf-report.1.en.html> — "The children overhead is calculated by adding all period values of the child functions so that it can show the total overhead of the higher level functions even if they don’t directly execute much."
[^pb-mangle]: The Rust Performance Book, Profiling (Symbol Demangling) — <https://nnethercote.github.io/perf-book/profiling.html> — "Rust uses a form of name mangling to encode function names in compiled code. If a profiler is unaware of this, its output may contain symbol names beginning with _ZN or _R"
[^pb-v0]: The Rust Performance Book, Profiling (Symbol Demangling) — <https://nnethercote.github.io/perf-book/profiling.html> — "it may be worth changing the mangling format from the default legacy format to the newer v0 format."
[^rec-dwarf]: perf-record(1), --call-graph (Debian bookworm, linux-perf) — <https://manpages.debian.org/bookworm/linux-perf/perf-record.1.en.html> — "When "dwarf" recording is used, perf also records (user) stack dump when sampled. Default size of the stack dump is 8192 (bytes)."
[^rec-lbr]: perf-record(1), --call-graph (Debian bookworm, linux-perf) — <https://manpages.debian.org/bookworm/linux-perf/perf-record.1.en.html> — "Using the "lbr" method doesn't require any compiler options. It will produce call graphs from the hardware LBR registers. The main limitation is that it is only available on new Intel platforms, such as Haswell. It can only get user call chain. It doesn't work with branch stack sampling at the same time."
[^flame-perf]: Brendan Gregg, Flame Graphs (Operating Systems) — <https://www.brendangregg.com/flamegraphs.html> — "Linux: perf (perf script report flamegraph)"
[^pb-list]: The Rust Performance Book, Profiling — <https://nnethercote.github.io/perf-book/profiling.html> — "flamegraph is a Cargo command that uses perf/DTrace to profile your code and then displays the results in a flame graph."
[^flame-sum]: Brendan Gregg, Flame Graphs (Summary) — <https://www.brendangregg.com/flamegraphs.html> — "The x-axis shows the stack profile population, sorted alphabetically (it is not the passage of time), and the y-axis shows stack depth, counting from zero at the bottom. Each rectangle represents a stack frame. The wider a frame is is, the more often it was present in the stacks. The top edge shows what is on-CPU, and beneath it is its ancestry. Original flame graphs use random colors to help visually differentiate adjacent frames."
[^env-strip]: Cargo Reference 1.95.0, Environment variables — <https://doc.rust-lang.org/1.95.0/cargo/reference/environment-variables.html> — "CARGO_PROFILE_\<name>_STRIP — Controls stripping of symbols and/or debuginfos, see profile.\<name>.strip."
[^cg-strip-sym]: rustc book 1.95.0, Codegen options (strip) — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "On platforms which depend on this symbol table for backtraces, profiling, and similar, this can affect them so negatively as to make the trace incomprehensible."
[^peo-skid]: perf_event_open(2), precise_ip (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/perf_event_open.2.en.html> — "Skid is how many instructions execute between an event of interest happening and the kernel being able to stop and record the event. Smaller skid is better and allows more accurate reporting of which events correspond to which instructions, but hardware is often limited with how small this can be."
[^ga-desc]: std::alloc::GlobalAlloc, Description — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html> — "A memory allocator that can be registered as the standard library's default through the #[global_allocator] attribute."
[^rs-file-nobuf]: Rust std 1.95.0, `std::fs::File` — <https://doc.rust-lang.org/1.95.0/std/fs/struct.File.html> — "File does not buffer reads and writes. For efficiency, consider wrapping the file in a BufReader or BufWriter when performing many small read or write calls, unless unbuffered reads and writes are required."
[^bufwriter-8k]: Rust std, BufWriter::new — <https://doc.rust-lang.org/1.95.0/std/io/struct.BufWriter.html> — "Creates a new BufWriter\<W> with a default buffer capacity. The default is currently 8 KiB, but may change in the future."
[^bufwriter-batch]: Rust std, BufWriter — <https://doc.rust-lang.org/1.95.0/std/io/struct.BufWriter.html> — "every call to write on TcpStream results in a system call. A BufWriter\<W> keeps an in-memory buffer of data and writes it to an underlying writer in large, infrequent batches."
[^ga-realloc]: std::alloc::GlobalAlloc::realloc — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html> — "The range 0..min(layout.size(), new_size) of the new memory block is guaranteed to have the same values as the original block."
[^pb-withcap]: The Rust Performance Book, Heap Allocations, Vec (trang sống) — <https://nnethercote.github.io/perf-book/heap-allocations.html> — "If you know the minimum or exact size of a vector, you can reserve a specific capacity with Vec::with_capacity, Vec::reserve, or Vec::reserve_exact."
[^ga-notrely]: std::alloc::GlobalAlloc, Safety — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html> — "You must not rely on allocations actually happening, even if there are explicit heap allocations in the source."
[^ga-reent]: std::alloc::GlobalAlloc, Re-entrance — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html> — "When implementing a global allocator, one has to be careful not to create an infinitely recursive implementation by accident, as many constructs in the Rust standard library may allocate in their implementation."
[^ga-mutex]: std::alloc::GlobalAlloc, Re-entrance — <https://doc.rust-lang.org/1.95.0/std/alloc/trait.GlobalAlloc.html> — "For example, on some platforms, std::sync::Mutex may allocate, so using it is highly problematic in a global allocator."
[^strace-f]: strace(1), --follow-forks (Debian bookworm) — <https://manpages.debian.org/bookworm/strace/strace.1.en.html> — "Trace child processes as they are created by currently traced processes as a result of the fork(2), vfork(2) and clone(2) system calls."
[^strace-trace]: strace(1), Filtering (Debian bookworm) — <https://manpages.debian.org/bookworm/strace/strace.1.en.html> — "Trace only the specified set of system calls."
[^perftrace-desc]: perf-trace(1), DESCRIPTION (Debian bookworm, linux-perf) — <https://manpages.debian.org/bookworm/linux-perf/perf-trace.1.en.html> — "This command will show the events associated with the target, initially syscalls, but other system events like pagefaults, task lifetime events, scheduling events, etc."
[^perftrace-s]: perf-trace(1), -s (Debian bookworm, linux-perf) — <https://manpages.debian.org/bookworm/linux-perf/perf-trace.1.en.html> — "Show only a summary of syscalls by thread with min, max, and average times (in msec) and relative stddev."
[^bttut-openat]: bpftrace one-liner tutorial v0.17.0, Lesson 3 — <https://raw.githubusercontent.com/iovisor/bpftrace/v0.17.0/docs/tutorial_one_liners.md> — "Note: In modern Linux systems (glibc >= 2.26) the `open` wrapper always calls the `openat` syscall."
[^bt-predicate]: bpftrace(8), Debian bookworm, language — <https://manpages.debian.org/bookworm/bpftrace/bpftrace.8.en.html> — "The predicate is optional condition that must be met for the action to be executed."
[^cpid]: bpftrace reference guide v0.17.0, Builtins — <https://raw.githubusercontent.com/iovisor/bpftrace/v0.17.0/docs/reference_guide.md> — "Child pid(u32), only valid with the -c command flag"
[^bt-c-usdt]: bpftrace(8), Debian bookworm, options — <https://manpages.debian.org/bookworm/bpftrace/bpftrace.8.en.html> — "Run COMMAND as a child process. When the child terminates bpftrace stops as well"
[^bt-map-var]: bpftrace(8), Debian bookworm, variables — <https://manpages.debian.org/bookworm/bpftrace/bpftrace.8.en.html> — "These exist for the lifetime of bpftrace itself and can be accessed from all action blocks and user-space."
[^btref-print-maps]: bpftrace reference guide v0.17.0 — <https://raw.githubusercontent.com/iovisor/bpftrace/v0.17.0/docs/reference_guide.md> — "When a program exits, all populated maps are printed"
[^bt-count-percpu]: bpftrace(8), Debian bookworm, count — <https://manpages.debian.org/bookworm/bpftrace/bpftrace.8.en.html> — "count() function uses a map type optimized for this (PER_CPU), increasing performance."
[^bt-hist]: bpftrace(8), Debian bookworm, hist — <https://manpages.debian.org/bookworm/bpftrace/bpftrace.8.en.html> — "Create a log2 histogram of n."
[^bt-list-v]: bpftrace(8), Debian bookworm, listing probes — <https://manpages.debian.org/bookworm/bpftrace/bpftrace.8.en.html> — "The verbose flag (-v) can be specified to inspect arguments (args) for providers that support it:"
[^bt-uprobe-variants]: bpftrace(8), uprobe, uretprobe (Debian bookworm) — <https://manpages.debian.org/bookworm/bpftrace/bpftrace.8.en.html> — "uprobe:binary:func uprobe:binary:func+offset uprobe:binary:offset uretprobe:binary:func"
[^ref-nomangle]: The Rust Reference 1.95.0, ABI — <https://doc.rust-lang.org/1.95.0/reference/abi.html> — "The no_mangle attribute may be used on any item to disable standard symbol name mangling."
[^ref-nomangle-2024]: The Rust Reference 1.95.0, ABI — <https://doc.rust-lang.org/1.95.0/reference/abi.html> — "Before the 2024 edition it is allowed to use the no_mangle attribute without the unsafe qualification."
[^ref-nomangle-unsafe]: The Rust Reference 1.95.0, ABI — <https://doc.rust-lang.org/1.95.0/reference/abi.html> — "This attribute is unsafe as an unmangled symbol may collide with another symbol with the same name (or with a well-known symbol), leading to undefined behavior."
[^ref-inline-auto]: The Rust Reference 1.95.0, Code generation attributes — <https://doc.rust-lang.org/1.95.0/reference/attributes/codegen.html> — "rustc automatically inlines functions when doing so seems worthwhile."
[^bt-arg-regs]: bpftrace(8), Debian bookworm, builtins — <https://manpages.debian.org/bookworm/bpftrace/bpftrace.8.en.html> — "nth argument passed to the function being traced. These are extracted from the CPU registers."
[^bt-no-signature]: bpftrace(8), Debian bookworm, kprobe — <https://manpages.debian.org/bookworm/bpftrace/bpftrace.8.en.html> — "bpftrace does not detect the function signature so it is not aware of the argument count or their type."
[^bt-uretprobe-go]: bpftrace(8), Debian bookworm, uprobe — <https://manpages.debian.org/bookworm/bpftrace/bpftrace.8.en.html> — "the kernel runs a special helper on user-space function entry which overrides the return address on the stack. This can cause issues with languages that have their own runtime like Golang:"
[^bt-uprobe-lib]: bpftrace(8), Debian bookworm, uprobe — <https://manpages.debian.org/bookworm/bpftrace/bpftrace.8.en.html> — "When tracing libraries, it is sufficient to specify the library name instead of a full path."
[^rust-sys-malloc]: rust-lang/rust tag 1.95.0, library/std/src/sys/alloc/unix.rs, `System::alloc` — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/alloc/unix.rs> — "unsafe { libc::malloc(layout.size()) as *mut u8 }"
[^rust-sys-realloc]: rust-lang/rust tag 1.95.0, library/std/src/sys/alloc/unix.rs, `System::realloc` — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/alloc/unix.rs> — "unsafe { libc::realloc(ptr as *mut libc::c_void, new_size) as *mut u8 }"
[^rust-sys-free]: rust-lang/rust tag 1.95.0, library/std/src/sys/alloc/unix.rs, `System::dealloc` — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/alloc/unix.rs> — "unsafe { libc::free(ptr as *mut libc::c_void) }"
[^uprobe-syntax]: Linux kernel docs, Uprobe-tracer — <https://docs.kernel.org/trace/uprobetracer.html> — "p[:[GRP/][EVENT]] PATH:OFFSET [FETCHARGS] : Set a uprobe"
[^uprobe-offset]: Linux kernel docs, Uprobe-tracer — <https://docs.kernel.org/trace/uprobetracer.html> — "However unlike kprobe-event tracer, the uprobe event interface expects the user to calculate the offset of the probepoint in the object."
[^uprobe-zsh-ex]: Linux kernel docs, Uprobe-tracer — <https://docs.kernel.org/trace/uprobetracer.html> — "0x46420 is the offset of zfree in object /bin/zsh that is loaded at 0x00400000."
[^elf-load]: elf(5), PT_LOAD — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "The array element specifies a loadable segment, described by p_filesz and p_memsz. The bytes from the file are mapped to the beginning of the memory segment."
[^hdr-precision]: hdrhistogram 7.5.4, crate docs, HdrHistogram — <https://docs.rs/hdrhistogram/7.5.4/hdrhistogram/> — "Value precision is expressed as the number of significant digits in the value recording, and provides control over value quantization behavior across the value range and the subsequent value resolution at any given level."
[^hdr-quantile]: hdrhistogram 7.5.4, crate docs, Querying — <https://docs.rs/hdrhistogram/7.5.4/hdrhistogram/> — "At any time, the histogram can be queried to return interesting statistical measurements, such as the total number of recorded samples, or the value at a given quantile:"
[^hdr-quant-pct]: hdrhistogram 7.5.4, crate docs, Floating point issues — <https://docs.rs/hdrhistogram/7.5.4/hdrhistogram/> — "To minimize FP precision losses, we favor working with quantiles rather than percentiles. A quantile represents a portion of a set with a number in [0, 1]. A percentile is the same concept, except it uses the range [0, 100]."
[^pl-nopoison]: parking_lot 0.12.5, Mutex, Differences from the standard library Mutex — <https://docs.rs/parking_lot/0.12.5/parking_lot/type.Mutex.html> — "No poisoning, the lock is released normally on panic."
[^pl-fair]: parking_lot 0.12.5, Mutex, Fairness — <https://docs.rs/parking_lot/0.12.5/parking_lot/type.Mutex.html> — "This mutex uses eventual fairness to ensure that the lock will be fair on average without sacrificing throughput."
[^crit-warm]: Criterion.rs Book, Command-Line Output (trang sống) — <https://bheisler.github.io/criterion.rs/book/user_guide/command_line_output.html> — "Every Criterion.rs benchmark iterates the benchmarked function automatically for a configurable warmup period (by default, for three seconds). For Rust function benchmarks, this is to warm up the processor caches and (if applicable) file system caches."
[^bb-inputs]: std::hint::black_box, How to use this — <https://doc.rust-lang.org/1.95.0/std/hint/fn.black_box.html> — "When benchmarking a function, you usually want to wrap its inputs in black_box so the compiler cannot make optimizations that would be unrealistic in real-life use."
[^sre-100ms]: Google SRE book, Monitoring Distributed Systems, Worrying About Your Tail (trang sống) — <https://sre.google/sre-book/monitoring-distributed-systems/> — "If you run a web service with an average latency of 100 ms at 1,000 requests per second, 1% of requests might easily take 5"
[^sre-fan]: Google SRE book, Monitoring Distributed Systems, Worrying About Your Tail (trang sống) — <https://sre.google/sre-book/monitoring-distributed-systems/> — "If your users depend on several such web services to render their page, the 99th percentile of one backend can easily become the median response of your frontend."
[^plcore-hash]: parking_lot_core 0.9.12, crate docs — <https://docs.rs/parking_lot_core/0.9.12/parking_lot_core/> — "which essentially consists of a hash table mapping of lock addresses to queues of parked (sleeping) threads."
[^pb-heap-cost]: The Rust Performance Book, Heap Allocations (trang sống) — <https://nnethercote.github.io/perf-book/heap-allocations.html> — "Heap allocations are moderately expensive. The exact details depend on which allocator is in use, but each allocation (and deallocation) typically involves acquiring a global lock, doing some non-trivial data structure manipulation, and possibly executing a system call."
[^pb-release]: The Rust Performance Book, Build Configuration (trang sống) — <https://nnethercote.github.io/perf-book/build-configuration.html> — "In comparison, release builds are much more optimized, omit debug assertions and integer overflow checks, and omit debug info. 10-100x speedups over dev builds are common!"
[^rustc-lto]: rustc book, Codegen options, lto — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "This flag controls whether LLVM uses link time optimizations to produce better optimized code, using whole-program analysis, at the cost of longer linking time."
[^rustc-cgu]: rustc book, Codegen options, codegen-units — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "When a crate is split into multiple codegen units, LLVM is able to process them in parallel. Increasing parallelism may speed up compile times, but may also produce slower code. Setting this to 1 may improve the performance of generated code, but may be slower to compile."
[^env-lto]: Cargo Reference 1.95.0, Environment variables — <https://doc.rust-lang.org/1.95.0/cargo/reference/environment-variables.html> — "CARGO_PROFILE_\<name>_LTO — Link-time optimization, see profile.\<name>.lto."
[^env-cgu]: Cargo Reference 1.95.0, Environment variables — <https://doc.rust-lang.org/1.95.0/cargo/reference/environment-variables.html> — "CARGO_PROFILE_\<name>_CODEGEN_UNITS — Set code generation units, see profile.\<name>.codegen-units."
[^cargo-opt-surprise]: Cargo Book, Profiles, opt-level — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "There may be surprising results, such as level 3 being slower than 2,"
[^hdr-record-correct]: hdrhistogram 7.5.4, Histogram::record_correct — <https://docs.rs/hdrhistogram/7.5.4/hdrhistogram/struct.Histogram.html> — "Record a value in the histogram while correcting for coordinated omission."
[^hdr-clone-correct]: hdrhistogram 7.5.4, Histogram::clone_correct — <https://docs.rs/hdrhistogram/7.5.4/hdrhistogram/struct.Histogram.html> — "To compensate for the loss of sampled values when a recorded value is larger than the expected interval between value samples"
[^crit-quiet]: Criterion.rs Book, Command-Line Output (trang sống) — <https://bheisler.github.io/criterion.rs/book/user_guide/command_line_output.html> — "In order to ensure reliable results, benchmarks should be run on a quiet computer and should be designed to do approximately the same amount of work for each iteration."
[^off-trace]: Brendan Gregg, Off-CPU Analysis — <https://www.brendangregg.com/offcpuanalysis.html> — "With this approach, only the kernel functions that switch the thread off-CPU are traced, along with timestamps and user-land stack traces."
[^off-warn]: Brendan Gregg, Off-CPU Analysis — <https://www.brendangregg.com/offcpuanalysis.html> — "WARNING: With off-CPU tracing, be aware that scheduler events can be very frequent – in extreme cases, millions of events per second"
[^tr-what]: tracing 0.1.44, crate docs — <https://docs.rs/tracing/0.1.44/tracing/> — "tracing is a framework for instrumenting Rust programs to collect structured, event-based diagnostic information."
[^tr-span]: tracing 0.1.44, crate docs, Spans — <https://docs.rs/tracing/0.1.44/tracing/> — "Unlike a log line that represents a moment in time, a span represents a period of time with a beginning and an end."
[^tr-event]: tracing 0.1.44, crate docs, Events — <https://docs.rs/tracing/0.1.44/tracing/> — "In general, events should be used to represent points in time within a span — a request returned with a given status code, n new items were taken from a queue, and so on."
[^tr-sub]: tracing 0.1.44, crate docs, Subscribers — <https://docs.rs/tracing/0.1.44/tracing/> — "As Spans and Events occur, they are recorded or aggregated by implementations of the Subscriber trait."
[^tr-nosub]: tracing 0.1.44, crate docs, In executables — <https://docs.rs/tracing/0.1.44/tracing/> — "This library does not contain any Subscriber implementations; these are provided by other crates."
[^tokio-tracing-layer]: Tokio topics, Getting started with Tracing — <https://tokio.rs/tokio/topics/tracing> — "the tracing-subscriber crate provides a Layer trait that represents a component that may be composed together with other Layers to form a Subscriber."
[^tokio-localset]: tokio 1.53.2, task::LocalSet — <https://docs.rs/tokio/1.53.2/tokio/task/struct.LocalSet.html> — "A set of tasks which are executed on the same thread."
[^tokio-current]: tokio 1.53.2, runtime::Builder::new_current_thread — <https://docs.rs/tokio/1.53.2/tokio/runtime/struct.Builder.html> — "Returns a new builder with the current thread scheduler selected."
[^tr-enter-warn]: tracing 0.1.44, Span::enter — <https://docs.rs/tracing/0.1.44/tracing/struct.Span.html> — "Warning: in asynchronous code that uses async/await syntax, Span::enter should be used very carefully or avoided entirely. Holding the drop guard returned by Span::enter across .await points will result in incorrect traces."
[^tr-enter-why]: tracing 0.1.44, Span::enter — <https://docs.rs/tracing/0.1.44/tracing/struct.Span.html> — "This means that another task will begin executing while remaining in the entered span. This results in an incorrect trace."
[^tr-instrument-combinator]: tracing 0.1.44, Span::enter — <https://docs.rs/tracing/0.1.44/tracing/struct.Span.html> — "This will enter the span every time the future is polled, and exit it whenever the future yields."
[^tr-instrumented-drop]: tracing 0.1.44 source, tracing/src/instrument.rs (Instrumented) — <https://raw.githubusercontent.com/tokio-rs/tracing/tracing-0.1.44/tracing/src/instrument.rs> — "ManuallyDrop is used here to to enter instrument Drop by entering Span and executing ManuallyDrop::drop."
[^tr-instrument-async]: tracing 0.1.44, Span::enter — <https://docs.rs/tracing/0.1.44/tracing/struct.Span.html> — "The #[instrument] attribute macro can automatically generate correct code when used on an async function:"
[^tr-in-scope]: tracing 0.1.44, Span::enter — <https://docs.rs/tracing/0.1.44/tracing/struct.Span.html> — "the span will always be exited before the next await point."
[^prof-debug]: Cargo Reference 1.95.0, Profiles (debug) — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "0, false, or "none": no debug info at all, default for release"
[^rust-gdb-run]: rust-lang/rust tag 1.95.0, src/etc/rust-gdb — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/src/etc/rust-gdb> — "Run GDB with the additional arguments that load the pretty printers"
[^lldb-map-bp]: LLDB, GDB to LLDB command map — <https://lldb.llvm.org/use/map.html> — "Set a breakpoint in file test.c at line 12 (gdb) break test.c:12 (lldb) breakpoint set --file test.c --line 12"
[^lldb-map-bt]: LLDB, GDB to LLDB command map — <https://lldb.llvm.org/use/map.html> — "Show the stack backtrace for the current thread (gdb) bt (lldb) thread backtrace"
[^lldb-map-watch]: LLDB, GDB to LLDB command map — <https://lldb.llvm.org/use/map.html> — "Set a watchpoint on a variable when it is written to (gdb) watch global_var (lldb) watchpoint set variable global_var"
[^rust-lldb-run]: rust-lang/rust tag 1.95.0, src/etc/rust-lldb — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/src/etc/rust-lldb> — "Call LLDB with the commands added to the argument list"
[^dw-line-intro]: DWARF Debugging Information Format Version 5, §6.2 — <https://dwarfstd.org/doc/DWARF5.pdf> — "A source-level debugger needs to know how to associate locations in the source files with the corresponding machine instruction addresses in the executable or the shared object files used by that executable object file."
[^gdb-pp]: GDB manual, Pretty Printing (current) — <https://sourceware.org/gdb/current/onlinedocs/gdb/Pretty-Printing.html> — "GDB provides a mechanism to allow pretty-printing of values using Python code. It greatly simplifies the display of complex objects."
[^gdb-scripts-sect]: GDB manual, The .debug_gdb_scripts section (current) — <https://sourceware.org/gdb/current/onlinedocs/gdb/dotdebug_005fgdb_005fscripts-section.html> — "If this section exists, its contents is a list of null-terminated entries specifying scripts to load."
[^gdb-scripts-safe]: GDB manual, The .debug_gdb_scripts section (current) — <https://sourceware.org/gdb/current/onlinedocs/gdb/dotdebug_005fgdb_005fscripts-section.html> — "Loading of inlined scripts requires a properly configured auto-load safe-path (see Auto-loading safe path). The path to specify in auto-load safe-path is the path of the file containing the .debug_gdb_scripts section."
[^rdg-limits]: rustc-dev-guide, Debugging support in the Rust compiler — <https://rustc-dev-guide.rust-lang.org/debugging-support-in-rustc.html> — "DWARF provides no way to differentiate between Structs and Tuples. Rust compiler emits fields with __0 and debuggers look for a sequence of such names to overcome this limitation."
[^rdg-ext]: rustc-dev-guide, Debugging support in the Rust compiler — <https://rustc-dev-guide.rust-lang.org/debugging-support-in-rustc.html> — "Rust compiler will emit DWARF for a virtual table, and this vtable object will have a DW_AT_containing_type that points to the real type. This lets debuggers dissect a trait object pointer to correctly find the payload."
[^rdg-traits]: rustc-dev-guide, Debugging support in the Rust compiler — <https://rustc-dev-guide.rust-lang.org/debugging-support-in-rustc.html> — "Rust traits are not emitted into DWARF at all. The impact of this is calling a method x.method() does not work as-is."
[^gdb-watch-simple]: GDB manual, Setting Watchpoints (current) — <https://sourceware.org/gdb/current/onlinedocs/gdb/Set-Watchpoints.html> — "GDB will break when the expression expr is written into by the program and its value changes."
[^gdb-watch-prefer]: GDB manual, Setting Watchpoints (current) — <https://sourceware.org/gdb/current/onlinedocs/gdb/Set-Watchpoints.html> — "GDB sets a hardware watchpoint if possible. Hardware watchpoints execute very quickly, and the debugger reports a change in value at the exact instruction where the change occurs. If GDB cannot set a hardware watchpoint, it sets a software watchpoint, which executes more slowly and reports the change in value at the next statement, not the instruction, after the change occurs."
[^gdb-watch-sw]: GDB manual, Setting Watchpoints (current) — <https://sourceware.org/gdb/current/onlinedocs/gdb/Set-Watchpoints.html> — "GDB does software watchpointing by single-stepping your program and testing the variable's value each time, which is hundreds of times slower than normal execution."
[^k-uapi-dr]: Linux kernel v6.8, arch/x86/include/uapi/asm/debugreg.h — <https://raw.githubusercontent.com/torvalds/linux/v6.8/arch/x86/include/uapi/asm/debugreg.h> — "Indicate the register numbers for a number of the specific debug registers. Registers 0-3 contain the addresses we wish to trap on */"
[^gdb-optim]: GDB manual, Debugging Optimized Code (current) — <https://sourceware.org/gdb/current/onlinedocs/gdb/Optimized-Code.html> — "When you debug a program compiled with '-g -O', remember that the optimizer has rearranged your code; the debugger shows you what is really there."
[^dw-noloc]: DWARF Debugging Information Format Version 5, §4.1 — <https://dwarfstd.org/doc/DWARF5.pdf> — "the variable is assumed to exist in the source code but not in the executable program"
[^dw-loclist]: DWARF Debugging Information Format Version 5, §2.6.2 — <https://dwarfstd.org/doc/DWARF5.pdf> — "Location lists are used in place of location descriptions whenever the object whose location is being described can change location during its lifetime."
[^gdb-optout-args]: GDB manual, Backtrace (current) — <https://sourceware.org/gdb/current/onlinedocs/gdb/Backtrace.html> — "The values of arguments that were not saved in their stack frames are shown as '\<optimized out>'."
[^gdb-inline]: GDB manual, Inline Functions (current) — <https://sourceware.org/gdb/current/onlinedocs/gdb/Inline-Functions.html> — "GDB displays inlined functions just like non-inlined functions. They appear in backtraces. You can view their arguments and local variables, step into them with step, skip them with next, and escape from them with finish."
[^gdb-inline-dwarf]: GDB manual, Inline Functions (current) — <https://sourceware.org/gdb/current/onlinedocs/gdb/Inline-Functions.html> — "For GDB to support inlined functions, the compiler must record information about inlining in the debug information"
[^prof-strip]: Cargo Reference 1.95.0, Profiles (strip) — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "Possible string values of strip are "none", "debuginfo", and "symbols". The default is "none"."
[^elf-dynsym]: elf(5), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This section holds the dynamic linking symbol table. This section is of type SHT_DYNSYM. The attribute used is SHF_ALLOC."
[^lsb-ehframe]: Linux Standard Base Core Specification 5.0, Exception Frames — <https://refspecs.linuxfoundation.org/LSB_5.0.0/LSB-Core-generic/LSB-Core-generic/ehframechpt.html> — "When using languages that support exceptions, such as C++, additional information must be provided to the runtime environment that describes the call frames that must be unwound during the processing of an exception. This information is contained in the special sections .eh_frame and .eh_framehdr."
[^gdb-sep-why]: GDB manual, Debugging Information in Separate Files (current) — <https://sourceware.org/gdb/current/onlinedocs/gdb/Separate-Debug-Files.html> — "Since debugging information can be very large—sometimes larger than the executable code itself—some systems distribute debugging information for their executables in separate files, which users can install only when they need to debug a problem."
[^rustc-dwarf]: The rustc book 1.95.0, Codegen options — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "4: DWARF version 4 (the default on most platforms, like Linux & macOS)."
[^dw-loclists-sect]: DWARF Debugging Information Format Version 5, §2.6.2 — <https://dwarfstd.org/doc/DWARF5.pdf> — "Location lists are contained in a separate object file section called .debug_loclists or .debug_loclists.dwo (for split DWARF object files)."
[^dw-loclist-v5]: DWARF Debugging Information Format Version 5, §2.6.2 — <https://dwarfstd.org/doc/DWARF5.pdf> — "This location list representation, the loclist class, and the related DW_AT_loclists_base attribute are new in DWARF Version 5."
[^gdb-buildid-sha]: GDB manual, Debugging Information in Separate Files (current) — <https://sourceware.org/gdb/current/onlinedocs/gdb/Separate-Debug-Files.html> — "The same section with an identical value is present in the original built binary with symbols, in its stripped variant, and in the separate debugging information file."
[^gdb-buildid]: GDB manual, Debugging Information in Separate Files (current) — <https://sourceware.org/gdb/current/onlinedocs/gdb/Separate-Debug-Files.html> — "The executable contains a build ID, a unique bit string that is also present in the corresponding debug info file."
[^gdb-buildid-path]: GDB manual, Debugging Information in Separate Files (current) — <https://sourceware.org/gdb/current/onlinedocs/gdb/Separate-Debug-Files.html> — "For the "build ID" method, GDB looks in the .build-id subdirectory of each one of the global debug directories for a file named nn/nnnnnnnn.debug, where nn are the first 2 hex characters of the build ID bit string, and nnnnnnnn are the rest of the bit string."
[^gdb-objcopy]: GDB manual, Debugging Information in Separate Files (current) — <https://sourceware.org/gdb/current/onlinedocs/gdb/Separate-Debug-Files.html> — "The debug link method needs the following additional command to also leave behind a debug link in foo:"
[^rustc-split-def]: The rustc book 1.95.0, Codegen options — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "off - This is the default for platforms with ELF binaries and windows-gnu (not Windows MSVC and not macOS). This typically means that DWARF debug information can be found in the final artifact in sections of the executable."
[^rs-gdbrs-std]: rust-lang/rust tag 1.95.0, compiler/rustc_codegen_llvm/src/debuginfo/gdb.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/compiler/rustc_codegen_llvm/src/debuginfo/gdb.rs> — "Add the pretty printers for the standard library first. section_contents.extend_from_slice(b"\x01gdb_load_rust_pretty_printers.py\0");"
[^rs-gdbrs-section]: rust-lang/rust tag 1.95.0, compiler/rustc_codegen_llvm/src/debuginfo/gdb.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/compiler/rustc_codegen_llvm/src/debuginfo/gdb.rs> — "Allocates the global variable responsible for the .debug_gdb_scripts binary section. pub(crate) fn get_or_insert_gdb_debug_scripts_section_global\<'ll>( cx: &CodegenCx\<'ll, '_>, ) -> &'ll Value { let c_section_var_name = c"__rustc_debug_gdb_scripts_section__";"
[^readelf-w]: readelf(1), OPTIONS (Debian bookworm, binutils-common) — <https://manpages.debian.org/bookworm/binutils-common/readelf.1.en.html> — "Displays the contents of the DWARF debug sections in the file, if any are present."
[^readelf-x]: readelf(1), OPTIONS (Debian bookworm, binutils-common) — <https://manpages.debian.org/bookworm/binutils-common/readelf.1.en.html> — "Displays the contents of the indicated section as a hexadecimal bytes."
[^dw-die]: DWARF Debugging Information Format Version 5, §2.1 — <https://dwarfstd.org/doc/DWARF5.pdf> — "DWARF uses a series of debugging information entries (DIEs) to define a low-level representation of a source program. Each debugging information entry consists of an identifying tag and a series of attributes."
[^dw-line-sect]: DWARF Debugging Information Format Version 5, §6.2 — <https://dwarfstd.org/doc/DWARF5.pdf> — "Line number information generated for a compilation unit is represented in the .debug_line section of an object file"
[^dw-isstmt]: DWARF Debugging Information Format Version 5, §6.2.2 — <https://dwarfstd.org/doc/DWARF5.pdf> — "A boolean indicating that the current instruction is a recommended breakpoint location."
[^rs-main-exit]: rust-lang/rust tag 1.95.0, library/std/src/rt.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/rt.rs> — "Return error code for panicking programs. 101"
[^core-default]: core(5), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/core.5.en.html> — "The default action of certain signals is to cause a process to terminate and produce a core dump file, a file containing an image of the process's memory at the time of termination."
[^rs-note-run-with]: rust-lang/rust tag 1.95.0, library/std/src/panicking.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/panicking.rs> — "note: run with `RUST_BACKTRACE=1` environment variable to display a"
[^rs-thread-uncaught]: std::thread, module docs — <https://doc.rust-lang.org/1.95.0/std/thread/index.html> — "If the main thread panics without the panic being caught, the application will exit with a non-zero exit code."
[^env-panic]: Cargo Reference 1.95.0, Environment variables — <https://doc.rust-lang.org/1.95.0/cargo/reference/environment-variables.html> — "CARGO_PROFILE_\<name>_PANIC — The panic strategy to use, see profile.\<name>.panic."
[^cargo-panic]: The Cargo Book 1.95.0, Profiles (panic) — <https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html> — "Terminate the process upon panic."
[^rs-abort-vs-panic]: std::process::abort, docs — <https://doc.rust-lang.org/1.95.0/std/process/fn.abort.html> — "However, panic! will still call the panic hook while abort will not."
[^hook-reg]: std::panic::set_hook, docs — <https://doc.rust-lang.org/1.95.0/std/panic/fn.set_hook.html> — "The panic hook is invoked when a thread panics, but before the panic runtime is invoked. As such, the hook will run with both the aborting and unwinding runtimes."
[^bash-128n]: Bash Reference Manual, Exit Status — <https://www.gnu.org/software/bash/manual/bash.html> — "When a command terminates on a fatal signal whose number is N, Bash uses the value 128+N as the exit status."
[^sig-num-abrt]: signal(7), Signal numbering for standard signals (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "SIGABRT 6 6 6 6"
[^sg-abrt]: signal(7), Standard signals — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "SIGABRT P1990 Core Abort signal from abort(3)"
[^bt-style-1]: rust-lang/rust tag 1.95.0, library/std/src/panic.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/panic.rs> — "* `full` for `BacktraceStyle::Full` /// * `1` for `BacktraceStyle::Short`"
[^bt-style-full]: rust-lang/rust tag 1.95.0, library/std/src/panic.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/panic.rs> — "* `0` for `BacktraceStyle::Off` /// * `full` for `BacktraceStyle::Full`"
[^bt-style-0]: rust-lang/rust tag 1.95.0, library/std/src/panic.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/panic.rs> — "`RUST_BACKTRACE` is read according to these rules: /// /// * `0` for `BacktraceStyle::Off`"
[^bt-debuginfo]: std::backtrace, module docs — <https://doc.rust-lang.org/1.95.0/std/backtrace/index.html> — "For most platforms a backtrace with a filename/line number requires that programs be compiled with debug information."
[^dw-inl-tag]: DWARF Debugging Information Format Version 5, §3.3 — <https://dwarfstd.org/doc/DWARF5.pdf> — "A particular inlined instance of a subroutine or function"
[^bt-capture-noop]: std::backtrace::Backtrace, docs — <https://doc.rust-lang.org/1.95.0/std/backtrace/struct.Backtrace.html> — "This function will be a noop if the RUST_BACKTRACE or RUST_LIB_BACKTRACE backtrace variables are both not set."
[^bt-force]: std::backtrace::Backtrace, docs — <https://doc.rust-lang.org/1.95.0/std/backtrace/struct.Backtrace.html> — "This function behaves the same as capture except that it ignores the values of the RUST_BACKTRACE and RUST_LIB_BACKTRACE environment variables, always capturing a backtrace."
[^bt-lib]: std::backtrace, module docs — <https://doc.rust-lang.org/1.95.0/std/backtrace/index.html> — "RUST_LIB_BACKTRACE - if this is set to 0 then Backtrace::capture will never capture a backtrace. Any other value set will enable Backtrace::capture."
[^bt-fallback]: std::backtrace, module docs — <https://doc.rust-lang.org/1.95.0/std/backtrace/index.html> — "RUST_BACKTRACE - if RUST_LIB_BACKTRACE is not set, then this variable is consulted with the same rules of RUST_LIB_BACKTRACE."
[^bt-cache]: std::backtrace, module docs — <https://doc.rust-lang.org/1.95.0/std/backtrace/index.html> — "Also note that the state of environment variables is cached once the first backtrace is created, so altering RUST_LIB_BACKTRACE or RUST_BACKTRACE at runtime might not actually change how backtraces are captured."
[^hook-info]: std::panic::set_hook, docs — <https://doc.rust-lang.org/1.95.0/std/panic/fn.set_hook.html> — "The hook is provided with a PanicHookInfo struct which contains information about the origin of the panic, including the payload passed to panic! and the source code location from which the panic originated."
[^phi-location]: std::panic::PanicHookInfo, docs — <https://doc.rust-lang.org/1.95.0/std/panic/struct.PanicHookInfo.html> — "Returns information about the location from which the panic originated, if available."
[^rs-abort-sig]: std::process::abort, docs — <https://doc.rust-lang.org/1.95.0/std/process/fn.abort.html> — "On Unix, the process will terminate with signal SIGABRT, which typically means that the shell prints"
[^sig-num-segv]: signal(7), Signal numbering for standard signals (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "SIGSEGV 11 11 11 11"
[^sg-segv]: signal(7), Standard signals — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "SIGSEGV P1990 Core Invalid memory reference"
[^rust186-null]: Rust 1.86.0 release notes — <https://blog.rust-lang.org/2025/04/03/Rust-1.86.0/> — "The compiler will now insert debug assertions that a pointer is not null upon non-zero-sized reads and writes, and also when the pointer is reborrowed into a reference."
[^rust186-nounwind]: Rust 1.86.0 release notes — <https://blog.rust-lang.org/2025/04/03/Rust-1.86.0/> — "For example, the following code will now produce a non-unwinding panic when debug assertions are enabled"
[^so-misleading]: rust-lang/rust tag 1.95.0, library/std/src/sys/pal/unix/stack_overflow.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/pal/unix/stack_overflow.rs> — "users to believe that unsafe code has accessed an invalid pointer; the"
[^so-msg]: rust-lang/rust tag 1.95.0, library/std/src/sys/pal/unix/stack_overflow.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/pal/unix/stack_overflow.rs> — "'{name}' ({tid}) has overflowed its"
[^so-abort]: rust-lang/rust tag 1.95.0, library/std/src/sys/pal/unix/stack_overflow.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/pal/unix/stack_overflow.rs> — "whenever a thread overflows its stack. We then abort to exit and"
[^pthr-guard-sigsegv]: pthread_attr_setguardsize(3), NOTES — <https://manpages.debian.org/bookworm/manpages-dev/pthread_attr_setguardsize.3.en.html> — "If a thread overflows its stack into the guard area, then, on most hard architectures, it receives a SIGSEGV signal, thus notifying it of the overflow."
[^rl-core]: getrlimit(2), RLIMIT_CORE — <https://manpages.debian.org/bookworm/manpages-dev/getrlimit.2.en.html> — "This is the maximum size of a core file (see core(5)) in bytes that the process may dump. When 0 no core dump files are created. When nonzero, larger dumps are truncated to this size."
[^core-notproduced]: core(5), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/core.5.en.html> — "There are various circumstances in which a core dump file is not produced:"
[^core-rlimit-zero]: core(5), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/core.5.en.html> — "However, RLIMIT_CORE will be ignored if the system is configured to pipe core dumps to a program."
[^core-pipe]: core(5), Piping core dumps to a program — <https://manpages.debian.org/bookworm/manpages/core.5.en.html> — "If the first character of this file is a pipe symbol (|), then the remainder of the line is interpreted as the command-line for a user-space program (or script) that is to be executed."
[^core-cp-empty]: core(5), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/core.5.en.html> — "/proc/sys/kernel/core_pattern is empty and /proc/sys/kernel/core_uses_pid contains the value 0."
[^core-rlimit-fsize]: core(5), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/core.5.en.html> — "The RLIMIT_CORE (core file size) or RLIMIT_FSIZE (file size) resource limits for the process are set to zero; see getrlimit(2) and the documentation of the shell's ulimit command"
[^core-default-name]: core(5), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/core.5.en.html> — "By default, the core file is called core or core.pid, where pid is the ID of the process that dumped core, and is created in the current working directory."
[^kern-cp-array]: Linux v6.1, fs/coredump.c — <https://raw.githubusercontent.com/torvalds/linux/v6.1/fs/coredump.c> — "static char core_pattern[CORENAME_MAX_SIZE] = "core";"
[^core-pipe-ns]: core(5), Piping core dumps to a program — <https://manpages.debian.org/bookworm/manpages/core.5.en.html> — "The process runs in the initial namespaces (PID, mount, user, and so on) and not in the namespaces of the crashing process."
[^docker-ulimit]: Docker docs, docker container run — <https://docs.docker.com/reference/cli/docker/container/run/> — "Since setting ulimit settings in a container requires extra privileges not available in the default container, you can set these using the --ulimit flag."
[^docker-core]: Docker docs, docker container run — <https://docs.docker.com/reference/cli/docker/container/run/> — "core Maximum size of core files created (RLIMIT_CORE)"
[^gdb-prog-core]: GDB manual, Invoking GDB — <https://sourceware.org/gdb/current/onlinedocs/gdb/Invoking-GDB.html> — "You can also start with both an executable program and a core file specified:"
[^gdb-corefile]: GDB manual, Commands to Specify Files — <https://sourceware.org/gdb/current/onlinedocs/gdb/Files.html> — "Traditionally, core files contain only some parts of the address space of the process that generated them; GDB can access the executable file itself for other parts."
[^gdb-bt-def]: GDB manual, Backtraces — <https://sourceware.org/gdb/current/onlinedocs/gdb/Backtrace.html> — "It shows one line per frame, for many frames, starting with the currently executing frame (frame zero), followed by its caller (frame one), and on up the stack."
[^gdb-bt-all]: GDB manual, Backtraces — <https://sourceware.org/gdb/current/onlinedocs/gdb/Backtrace.html> — "For example, if you type thread apply all backtrace, GDB will display the backtrace for all the threads; this is handy when you debug a core dump of a multi-threaded program."
[^sig-thread-directed]: signal(7), Signal mask and pending signals — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "A signal may be thread-directed because it was generated as a consequence of executing a specific machine-language instruction that triggered a hardware exception (e.g., SIGSEGV for an invalid memory access, or SIGFPE for a math error)"
[^core-gcore]: core(5), NOTES — <https://manpages.debian.org/bookworm/manpages/core.5.en.html> — "The gdb(1) gcore command can be used to obtain a core dump of a running process."
[^gdb-attach]: GDB manual, Invoking GDB — <https://sourceware.org/gdb/current/onlinedocs/gdb/Invoking-GDB.html> — "You can, instead, specify a process ID as a second argument or use option -p, if you want to debug a running process:"
[^docker-ptrace]: Docker docs, Seccomp security profiles for Docker — <https://docs.docker.com/engine/security/seccomp/> — "Tracing/profiling syscall. Blocked in Linux kernel versions before 4.8 to avoid seccomp bypass. Tracing/profiling arbitrary processes is already blocked by dropping CAP_SYS_PTRACE, because it could leak a lot of information on the host."
[^sg-quit]: signal(7), Standard signals — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "SIGQUIT P1990 Core Quit from keyboard"
[^elf-magic]: elf(5), e_ident — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "The first byte of the magic number. It must be filled with ELFMAG0. (0: 0x7f)"
[^macho-magic]: LLVM, llvm/include/llvm/BinaryFormat/MachO.h (constants from \<mach-o/loader.h>) — <https://raw.githubusercontent.com/llvm/llvm-project/main/llvm/include/llvm/BinaryFormat/MachO.h> — "MH_MAGIC = 0xFEEDFACEu, MH_CIGAM = 0xCEFAEDFEu, MH_MAGIC_64 = 0xFEEDFACFu, MH_CIGAM_64 = 0xCFFAEDFEu"
[^binfmt-pie]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "There are effectively two types of ET_DYN binaries: programs (i.e. PIE: ET_DYN with INTERP) and loaders (ET_DYN without INTERP, since they _are_ the ELF interpreter)."
[^elf-etype]: elf(5), e_type — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "ET_REL A relocatable file. ET_EXEC An executable file. ET_DYN A shared object."
[^binfmt-interp-load]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "elf_entry = load_elf_interp(interp_elf_ex, interpreter, load_bias, interp_elf_phdata, &arch_state);"
[^dr-entry]: Drepper, How To Write Shared Libraries v4.1.2, 1.4 — <https://akkadia.org/drepper/dsohowto.pdf> — "After setting up the auxiliary vector the kernel is finally ready to transfer control to the dynamic linker in user mode. The entry point is defined in e entry field of the ELF header of the dynamic linker."
[^elf-entry]: elf(5), e_entry — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This member gives the virtual address to which the system first transfers control, thus starting the process."
[^binfmt-load-bias]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "if (interpreter) { load_bias = ELF_ET_DYN_BASE; if (current->flags & PF_RANDOMIZE) load_bias += arch_mmap_rnd();"
[^elf-load-zero]: elf(5), PT_LOAD — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "bytes are defined to hold the value 0 and to follow the segment's initialized area. The file size may not be larger than the memory size."
[^elf-gnustack]: elf(5), PT_GNU_STACK — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "GNU extension which is used by the Linux kernel to control the state of the stack via the flags set in the p_flags member."
[^elf-needed]: elf(5), Dynamic tags — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "DT_NEEDED String table offset to name of a needed library"
[^elf-dt-runpath]: elf(5), Dynamic tags — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "DT_RUNPATH String table offset to library search path"
[^ldso-order-cache]: ld.so(8), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "From the cache file /etc/ld.so.cache, which contains a compiled list of candidate shared objects previously found in the augmented library path."
[^gabi-dtbindnow]: System V gABI, ch.5, Dynamic Section, DT_BIND_NOW — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch5.dynamic.html> — "If present in a shared object or executable, this entry instructs the dynamic linker to process all relocations for the object containing this entry before transferring control to the program. The presence of this entry takes precedence over a directive to use lazy binding for this object when specified through the environment or via dlopen(BA_LIB). This entry is at level 2. Its use has been superseded by the DF_BIND_NOW flag."
[^dr-bindnow]: Drepper, How To Write Shared Libraries v4.1.2, 1.5.1 — <https://akkadia.org/drepper/dsohowto.pdf> — "Lazy relocation can be disabled for an individual object by adding the -z now option to the linker command line. The linker will set the DF BIND NOW flag in the DT FLAGS entry of the dynamic section to mark the DSO."
[^dr-relro]: Drepper, How To Write Shared Libraries v4.1.2, 2.6 — <https://akkadia.org/drepper/dsohowto.pdf> — "Creating objects this way is enabled by the -z relro linker option. The linker is instructed to move the sections, which are only modified by relocations, onto separate memory page and emit a new program header entry PT GNU RELRO to point the dynamic linker to these special pages. At runtime the dynamic linker can then remove the write access to these pages after it is done."
[^rs-relro-full]: rustc book 1.95.0, Codegen options, relro-level — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "full: Dynamically linked functions are resolved at the start of program execution and the Global Offset Table (.got/.got.plt) is populated eagerly and then made read-only. The GOT is also moved to prevent writing from buffer overflows. Full RELRO uses more memory and increases process startup time."
[^rs-relro-default]: rustc book 1.95.0, Codegen options, relro-level — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "Each rustc target has its own default for RELRO. rustc enables Full RELRO by default on platforms where it is supported."
[^rs-base-linux]: rust-lang/rust tag 1.95.0, compiler/rustc_target/src/spec/base/linux.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/compiler/rustc_target/src/spec/base/linux.rs> — "position_independent_executables: true, relro_level: RelroLevel::Full, has_thread_local: true, crt_static_respected: true,"
[^elf-interp]: elf(5), PT_INTERP — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "The array element specifies the location and size of a null-terminated pathname to invoke as an interpreter. This segment type is meaningful only for executable files (though it may occur for shared objects). However it may not occur more than once in a file. If it is present, it must precede any loadable segment entry."
[^gabi-exec-abs]: System V gABI, ch.5, Base Address — <https://refspecs.linuxfoundation.org/elf/gabi4+/ch5.pheader.html> — "Executable files typically contain absolute code. To let the process execute correctly, the segments must reside at the virtual addresses used to build the executable file."
[^binfmt-elf-load]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "error = elf_load(bprm->file, load_bias + vaddr, elf_ppnt, elf_prot, elf_flags, total_size);"
[^rs-x64-musl]: rust-lang/rust tag 1.95.0, compiler/rustc_target/src/spec/targets/x86_64_unknown_linux_musl.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/compiler/rustc_target/src/spec/targets/x86_64_unknown_linux_musl.rs> — "base.stack_probes = StackProbeType::Inline; base.static_position_independent_executables = true;"
[^mang-legacy]: rustc book 1.95.0, Symbol Mangling — <https://doc.rust-lang.org/1.95.0/rustc/symbol-mangling/index.html> — "The legacy version (which is currently the default on beta/stable) is not described here."
[^cg-mangling]: rustc book 1.95.0, Codegen options (symbol-mangling-version) — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "This option controls the name mangling format for encoding Rust item names for the purpose of generating object code and linking."
[^elf-shf-alloc]: elf(5), sh_flags SHF_ALLOC — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "This section occupies memory during process execution. Some control sections do not reside in the memory image of an object file."
[^elf-nobits]: elf(5), sh_type SHT_NOBITS — <https://manpages.debian.org/bookworm/manpages/elf.5.en.html> — "A section of this type occupies no space in the file but otherwise resembles SHT_PROGBITS."
[^aslr-1]: Linux kernel docs, admin-guide/sysctl/kernel, randomize_va_space — <https://docs.kernel.org/admin-guide/sysctl/kernel.html> — "Make the addresses of mmap base, stack and VDSO page randomized. This, among other things, implies that shared libraries will be loaded to random addresses. Also for PIE-linked binaries, the location of code start is randomized."
[^aslr-2]: Linux kernel docs, admin-guide/sysctl/kernel, randomize_va_space — <https://docs.kernel.org/admin-guide/sysctl/kernel.html> — "Additionally enable heap randomization. This is the default if CONFIG_COMPAT_BRK is disabled."
[^aslr-0]: Linux kernel docs, admin-guide/sysctl/kernel, randomize_va_space — <https://docs.kernel.org/admin-guide/sysctl/kernel.html> — "Turn the process address space randomization off."
[^setarch-R]: setarch(8), OPTIONS (Debian bookworm, util-linux) — <https://manpages.debian.org/bookworm/util-linux/setarch.8.en.html> — "Disables randomization of the virtual address space. Turns on ADDR_NO_RANDOMIZE."
[^binfmt-pfrandomize]: Linux v6.8, fs/binfmt_elf.c, load_elf_binary — <https://raw.githubusercontent.com/torvalds/linux/v6.8/fs/binfmt_elf.c> — "if (!(current->personality & ADDR_NO_RANDOMIZE) && randomize_va_space) current->flags |= PF_RANDOMIZE;"
[^readelf-h]: readelf(1), OPTIONS (Debian bookworm, binutils-common) — <https://manpages.debian.org/bookworm/binutils-common/readelf.1.en.html> — "Displays the information contained in the ELF header at the start of the file."
[^readelf-l]: readelf(1), OPTIONS (Debian bookworm, binutils-common) — <https://manpages.debian.org/bookworm/binutils-common/readelf.1.en.html> — "Displays the information contained in the file's segment headers, if it has any."
[^readelf-d]: readelf(1), OPTIONS (Debian bookworm, binutils-common) — <https://manpages.debian.org/bookworm/binutils-common/readelf.1.en.html> — "Displays the contents of the file's dynamic section, if it has one."
[^readelf-S]: readelf(1), OPTIONS (Debian bookworm, binutils-common) — <https://manpages.debian.org/bookworm/binutils-common/readelf.1.en.html> — "Displays the information contained in the file's section headers, if it has any."
[^readelf-W]: readelf(1), OPTIONS (Debian bookworm, binutils-common) — <https://manpages.debian.org/bookworm/binutils-common/readelf.1.en.html> — "Don't break output lines to fit into 80 columns."
[^ldd-trace]: ldd(1), Debian bookworm — <https://manpages.debian.org/bookworm/manpages/ldd.1.en.html> — "In the usual case, ldd invokes the standard dynamic linker (see ld.so(8)) with the LD_TRACE_LOADED_OBJECTS environment variable set to 1."
[^ldd-sec]: ldd(1), Security — <https://manpages.debian.org/bookworm/manpages/ldd.1.en.html> — "Thus, you should never employ ldd on an untrusted executable, since this may result in the execution of arbitrary code."
[^ldso-debug]: ld.so(8), LD_DEBUG — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "Output verbose debugging information about operation of the dynamic linker."
[^ldso-debug-libs]: ld.so(8), LD_DEBUG — <https://manpages.debian.org/bookworm/manpages/ld.so.8.en.html> — "libs Display library search paths."
[^ftrace-mount]: Linux kernel docs, ftrace — <https://docs.kernel.org/trace/ftrace.html> — "When tracefs is configured into the kernel (which selecting any ftrace option will do) the directory /sys/kernel/tracing will be created."
[^ftrace-fngraph]: Linux kernel docs, ftrace — <https://docs.kernel.org/trace/ftrace.html> — "whereas the function graph tracer traces on both entry and exit of the functions."
[^ftrace-filter]: Linux kernel docs, ftrace — <https://docs.kernel.org/trace/ftrace.html> — "Echoing names of functions into this file will limit the trace to only those functions."
[^ftrace-tracing-on]: Linux kernel docs, ftrace — <https://docs.kernel.org/trace/ftrace.html> — "Note, this only disables writing to the ring buffer, the tracing overhead may still be occurring."
[^ftrace-marker]: Linux kernel docs, ftrace — <https://docs.kernel.org/trace/ftrace.html> — "This is a very useful file for synchronizing user space with events happening in the kernel."
[^pgo-steps]: rustc book, Profile-guided Optimization, Usage — <https://doc.rust-lang.org/1.95.0/rustc/profile-guided-optimization.html> — "Convert the .profraw file into a .profdata file using LLVM's llvm-profdata tool"
[^pgo-llvmtools]: rustc book, Profile-guided Optimization, Usage — <https://doc.rust-lang.org/1.95.0/rustc/profile-guided-optimization.html> — "Note that installing the llvm-tools-preview component won't add llvm-profdata to the PATH."
[^rustc-split-packed]: The rustc book 1.95.0, Codegen options — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "packed - This is the default for Windows MSVC and macOS. The term "packed" here means that all the debug information is packed into a separate file from the main executable. On Windows MSVC this is a *.pdb file, on macOS this is a *.dSYM folder, and on other platforms this is a *.dwp file."
[^rs-crt-flag]: Rust Reference 1.95.0, Linkage, crt-static — <https://doc.rust-lang.org/1.95.0/reference/linkage.html> — "The linkage of the C runtime is configured to respect the crt-static target feature."
[^rs-reloc-crt]: rustc book 1.95.0, Codegen options, relocation-model (Linking effects) — <https://doc.rust-lang.org/1.95.0/rustc/codegen-options/index.html> — "If the target doesn’t support both position-independent and statically linked executables, then -C target-feature=+crt-static “wins” over -C relocation-model=pic, and the linker is instructed (-static) to produce a statically linked but not position-independent executable."
[^gdb-nodebuginfo]: GDB manual, Examining the Symbol Table (current) — <https://sourceware.org/gdb/current/onlinedocs/gdb/Symbols.html> — "GDB knows the variable exists from inspecting the linker/loader symbol table (e.g., the ELF dynamic symbol table), but such symbols do not contain type information."
