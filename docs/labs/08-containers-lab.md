# Lab P8 — Containers từ bên trong

Đây là phần thực hành của Phase 8. Lý thuyết nằm ở [tổng quan Phase 8](../08-containers/00-tong-quan.md); mỗi bài dưới đây trỏ tới đúng chương và mục cần đọc trước. Mục tiêu chung: tự tay gọi các system call mà runtime container gọi (`unshare`, `fork`, `pivot_root`, `mount`, `sigaction`) và đọc các file kernel (`/proc/self/ns`, cgroup v2) mà `docker run` dựa vào, để container thôi là hộp đen.

> **Tài liệu này không ghi output.** Mỗi bài có phần **Dự đoán**: điều lý thuyết (man page, kernel docs, source) nói sẽ xảy ra, kèm chú thích. Bạn chạy lệnh, rồi *đối chiếu* output của mình với dự đoán; chỗ nào khác thì đó chính là chỗ đáng học. Dự đoán chỉ dựa trên nguồn đã trích hoặc trên quan sát đã ghi trong chương lý thuyết (có link); chỗ nào là suy luận của người soạn thì ghi rõ. Quy ước chú thích như ở [00-roadmap.md](../00-roadmap.md).

## 1. Chuẩn bị

Crate: `labs/crates/lab-containers`, 5 binary, chỉ có nghĩa trên Linux (trên macOS `cargo build` vẫn qua; chạy binary chỉ in hướng dẫn). Phụ thuộc duy nhất: `libc = "0.2"` (để gọi `unshare`, `fork`, `mount`, `pivot_root`, `sigaction`, `sysconf` trực tiếp). Mọi lệnh chạy từ thư mục gốc repo.

Build một lần trong container lab (cũng dựng image `rust-foundations-lab:1.95` nếu chưa có):

```bash
./labs/linux.sh cargo build -p lab-containers
```

Binary nằm ở `labs/target-linux/debug/<tên>` trên máy bạn, tức `/work/labs/target-linux/debug/<tên>` trong container.

### 1.1 Quyền cần có

`./labs/linux.sh` chạy container với `SYS_PTRACE`, `NET_ADMIN`, `NET_RAW` và `--security-opt seccomp=unconfined` (AppArmor để mặc định), nhưng **không có `CAP_SYS_ADMIN`**. Nghĩa là container lab *không có seccomp* và khác container Docker mặc định. Việc tạo hầu hết loại namespace mới, `mount` và `pivot_root` đều cần capability đó[^ns-priv][^pr-cap]; man page `clone(2)` liệt kê `EPERM` cho `CLONE_NEWNS`, `CLONE_NEWPID`, `CLONE_NEWUTS`... khi caller không có nó[^clone-eperm]. Ngoại lệ duy nhất là user namespace, không đòi đặc quyền từ Linux 3.8[^ns-userexc]. Chương lý thuyết cũng đã quan sát đúng điều này trong container lab thường: `unshare --uts true` bị `Operation not permitted` còn `unshare --user true` thành công ([chương 02, mục 3](../08-containers/02-namespaces.md)); riêng vế sau còn nhờ lab tắt seccomp, vì trong container Docker mặc định seccomp chặn cả `unshare -U` ([chương 05, mục 5](../08-containers/05-bao-mat-container.md)).

Vì vậy các bài cần quyền chạy bằng `docker run --privileged` (Docker docs: cờ này bật mọi Linux capability của kernel cho container[^docker-priv]) trên image đã dựng:

```bash
# Hàm dùng cho các bài cần CAP_SYS_ADMIN. Chỉ dùng trong lab: container privileged không phải sandbox an toàn.
priv() { docker run --rm --privileged -v "$PWD":/work -w /work/labs rust-foundations-lab:1.95 "$@"; }
```

(Chạy từ thư mục gốc repo, như khi chạy `linux.sh`; `priv ./target-linux/debug/<tên> ...` tương đương `linux.sh` nhưng có đặc quyền.) Docker trên macOS chạy container trong một VM Linux (ở môi trường soạn bài này là VM của Colima; [chương 01, mục 4](../08-containers/01-container-la-gi.md)) nên `--privileged` chỉ cho quyền trong VM đó, không đụng macOS.

| Bài | Binary | Cần `CAP_SYS_ADMIN`? | Chạy bằng |
|---|---|---|---|
| 8.1 Namespace của chính mình | `ns_self` | không | `linux.sh` |
| 8.2 `unshare` UTS/PID/mount | `unshare_ns` | có (hoặc thêm `--userns`) | `priv` |
| 8.3 Giới hạn cgroup v2 | `cgroup_limits` | không | `docker run --cpus ... --memory ...` |
| 8.4 Mini container | `mini_container` | có | `docker run -it --privileged` |
| 8.5 PID 1 và signal | `pid1` | không | `docker run -d` + `docker stop` |

## 2. Bài 8.1 — `ns_self`: process của tôi nằm trong những namespace nào?

**Mục tiêu.** Đọc `/proc/self/ns/*`, lấy `st_dev`/`st_ino` của từng namespace và so với PID 1: nhìn thấy "cùng namespace" là cùng số inode, không phải khái niệm trừu tượng.

**Đọc trước.** [Chương 02 — Namespaces](../08-containers/02-namespaces.md), mục 1 (8 loại namespace) và mục 2 (`/proc/<pid>/ns` và so sánh inode); mục 6 (user namespace, vì bài này in `uid_map`/`gid_map`).

**Mã.** `src/bin/ns_self.rs`. Liệt kê thư mục `/proc/self/ns`, với mỗi entry in nội dung symlink (`readlink`), `st_ino`/`st_dev` (qua `fs::metadata`, theo symlink), và so với `/proc/1/ns/<tên>`; cuối cùng in `uid_map` và `gid_map`.

**Chạy.**

```bash
# (A) sh là PID 1, ns_self là con của nó: cùng namespace với PID 1.
./labs/linux.sh sh -c './target-linux/debug/ns_self; true'

# (B) nhìn PID 1 của *máy chủ Docker* (VM): dùng PID namespace của host.
docker run --rm --pid=host --cap-add SYS_PTRACE -v "$PWD":/work -w /work/labs rust-foundations-lab:1.95 ./target-linux/debug/ns_self
```

(Với (A): lý do cần `; true` là để shell không `exec` thẳng lệnh cuối, xem lab Linux về `bash -c` ở [01-linux-lab.md](01-linux-lab.md), mục 4.2. `sh` trên Debian là `dash`; tôi không kiểm tra hành vi `exec` của nó nên dùng `; true` để tránh phụ thuộc. Cờ `--cap-add SYS_PTRACE` ở (B) là vì quyền đọc/readlink các symlink này được quyết định bởi kiểm tra ptrace access mode `PTRACE_MODE_READ_FSCREDS`[^ns-ptrace]; chi tiết kiểm tra nằm ở `ptrace(2)`, tôi chưa đọc — nếu cột cuối vẫn là `?`, đó là chỗ để bạn tra.)

**Dự đoán (từ lý thuyết).**

1. Thư mục có một entry cho mỗi namespace thao tác được bằng `setns(2)`[^ns-dir]; ngoài ra có cặp `pid` / `pid_for_children` và `time` / `time_for_children`: file `*_for_children` là handle của namespace dành cho *con sinh ra sau này*[^ns-time-children], và với PID namespace nó có thể khác `pid` sau `unshare`/`setns`[^ns-pidchildren]. Chương 02 mục 2 liệt kê danh sách thực tế của container lab.
2. (A): mọi dòng cuối cột phải là `yes`: hai process cùng namespace thì device ID và inode của các symlink tương ứng bằng nhau[^ns-inode]. Bạn vừa tự kiểm chứng bằng code điều chương lý thuyết nói bằng `readlink`.
3. Cột `pid` và `pid_for_children` hiện cùng một số inode vì chưa ai gọi `unshare(CLONE_NEWPID)` (bài 8.2 sẽ làm khác đi).
4. (B): container Docker thường được tạo với mnt/uts/ipc/pid/net/cgroup namespace riêng nên các dòng này phải là `NO` so với PID 1 của VM nếu bạn đọc được; riêng `user` thì chương 02 đã quan sát container lab *không dùng* user namespace, nên `/proc/self/uid_map` là `0 0 4294967295`, đúng dạng map của namespace gốc[^user-initial]. (Suy luận của người soạn: do đó dòng `user` có thể là `yes` ở (B).)
5. Nếu bạn chạy binary trực tiếp trên một user namespace mới, `uid_map` rỗng cho tới khi có ai ghi map: namespace vừa tạo chưa có ánh xạ ID nào[^user-map-empty] và ID chưa ánh xạ hiện ra là 65534[^user-overflow].

**Câu hỏi.**

1. Tại sao dùng `fs::metadata` (theo symlink) chứ không phải `symlink_metadata` để so hai namespace? Output của `readlink` đủ để so sánh không?
   <details><summary>Đáp án</summary>

   Hai process cùng namespace khi device ID và inode của symlink giống nhau; man page nói đúng thế và nói cách kiểm là `stat`[^ns-inode]. Nội dung `readlink` có dạng loại + số inode (`uts:[…]`), nên so chuỗi cũng được; `st_dev` thêm một lớp chắc chắn (chương 02 mục 2 dùng cả hai).

   </details>

2. Trong lần chạy (A) cột `pid` là `yes`. Điều đó liên quan thế nào tới việc `ps` trong container chỉ thấy vài process?
   <details><summary>Đáp án</summary>

   `/proc` chỉ hiện các process thấy được trong PID namespace của process đã mount nó[^pid-proc]; hai process cùng PID namespace (cùng inode ở cột `pid`) cùng một tập PID. Container có PID namespace riêng nên procfs mount trong đó chỉ liệt kê process của namespace đó.

   </details>

3. Namespace sống bao lâu nếu bạn mở `/proc/self/ns/uts` rồi giữ fd, còn mọi process trong nó đã thoát?
   <details><summary>Đáp án</summary>

   Mặc định namespace bị hủy khi process cuối cùng thoát hoặc rời đi[^ns-lifetime]; chương 02 mục 2 nói thêm rằng chừng nào fd mở tới file trong thư mục này còn thì namespace còn sống. Đây là cơ chế `setns` vào namespace "mồ côi".

   </details>

## 3. Bài 8.2 — `unshare_ns`: tạo UTS, PID, mount namespace bằng `libc::unshare`

**Mục tiêu.** Gọi `unshare(2)` trực tiếp, thấy: (a) cần `CAP_SYS_ADMIN` và `EPERM` trông thế nào; (b) UTS namespace nhận *bản sao* hostname rồi tách ra; (c) PID namespace **không** chuyển chính caller, chỉ con; (d) mount mới trong mount namespace không lan ra ngoài; (e) vì sao thêm `CLONE_NEWUSER` giúp process không đặc quyền làm được.

**Đọc trước.** [Chương 02](../08-containers/02-namespaces.md): mục 3 (`clone`/`unshare`/`setns` và quyền), mục 4 (PID namespace), mục 5 (mount namespace), mục 6 (user namespace và `uid_map`), mục 7 (UTS).

**Mã.** `src/bin/unshare_ns.rs`; cú pháp `unshare_ns [uts|pid|mount] [--userns] [--hold GIÂY]`. Điểm đáng đọc trong code:

- Một lời gọi `libc::unshare(flags)`; `--userns` chỉ thêm `CLONE_NEWUSER` vào *cùng* `flags`.
- Sau khi tạo user namespace, ghi `/proc/self/setgroups` (`deny`), `uid_map`, `gid_map` — đúng thứ tự mà man page đòi cho process không đặc quyền.
- `pid`: `fork` sau `unshare`, rồi in `getpid()`/`getppid()` của con.
- `mount`: `mount("/", MS_REC|MS_PRIVATE)` rồi mount một `tmpfs` lên `/tmp/lab-8-2-mnt`.
- `--hold N`: giữ process N giây để bạn nhìn nó từ shell khác.

**Chạy.**

```bash
# (1) Không đặc quyền: thấy EPERM (chạy bằng linux.sh, không --privileged).
./labs/linux.sh ./target-linux/debug/unshare_ns uts

# (2) Cùng lệnh nhưng thêm user namespace.
./labs/linux.sh ./target-linux/debug/unshare_ns uts --userns

# (3) Có CAP_SYS_ADMIN: ba loại.
priv ./target-linux/debug/unshare_ns uts
priv ./target-linux/debug/unshare_ns pid
priv ./target-linux/debug/unshare_ns mount

# (4) Nhìn từ ngoài: giữ namespace 60 giây, rồi so từ một process khác trong cùng container.
docker run -d --name lab82 --privileged -v "$PWD":/work -w /work/labs rust-foundations-lab:1.95 ./target-linux/debug/unshare_ns uts --hold 60
docker exec lab82 sh -c 'hostname; readlink /proc/1/ns/uts; readlink /proc/self/ns/uts'
docker rm -f lab82
```

**Dự đoán (từ lý thuyết).**

1. **(1) `EPERM`.** `unshare(CLONE_NEWUTS)` từ process không có `CAP_SYS_ADMIN` thất bại: tạo namespace qua `clone`/`unshare` hầu hết cần capability này[^ns-priv], và `EPERM` được man page nêu cho đúng cờ `CLONE_NEWUTS`/`CLONE_NEWPID`/`CLONE_NEWNS` khi thiếu nó[^clone-eperm]. Chương trình in thông báo và thoát mã 1.
2. **(2) `--userns` thành công.** User namespace là ngoại lệ không cần đặc quyền[^ns-userexc]; khi `CLONE_NEWUSER` đi chung các cờ khác, user namespace được bảo đảm tạo **trước**, nên caller có quyền trên các namespace còn lại trong cùng lời gọi[^user-first]. Quy tắc này cũng là lý do `unshare -Ur --uts` chạy được còn `unshare --uts` thì không ([chương 02, mục 3](../08-containers/02-namespaces.md)). Caller nhận đủ capability *trong namespace mới*[^user-caps]; `sethostname` thành công vì kernel kiểm `CAP_SYS_ADMIN` trong user namespace sở hữu UTS namespace[^user-hostname]. Hai điều kiện của code lab: `CLONE_NEWUSER` đòi caller không đa luồng[^unshare-thread] (hàm `main` của lab chưa tạo thread nào) và map phải ghi một dòng ánh xạ EUID của chính mình[^user-unpriv]; `gid_map` thì phải `deny` `setgroups` trước[^user-setgroups]. Trước khi ghi map, UID hiện là 65534[^user-map-empty][^user-overflow]; chương trình in `getuid()` *sau* khi ghi map nên phải ra `0`. Lưu ý: bước này chạy được trong `linux.sh` vì lab tắt seccomp; trong container Docker mặc định, seccomp chặn `unshare` user namespace ([chương 05, mục 5](../08-containers/05-bao-mat-container.md)).
3. **(3) `uts` có đặc quyền.** Hostname lúc đầu là bản sao hostname của namespace cũ[^uts-copy]; sau `sethostname("lab-ns-demo")` chỉ namespace mới đổi: UTS cô lập hostname và NIS domain name[^uts-def]. Dòng `before:` (in cả bốn namespace) và dòng `uts namespace now:` phải có **hai số inode `uts` khác nhau**; `pid`, `mnt`, `user` không nằm trong cờ nên không đổi (chương trình chỉ in lại `uts` sau `unshare`; muốn so ba loại kia dùng `--hold` và `readlink /proc/<pid>/ns/...` từ shell khác).
4. **(3) `pid`.** `unshare(CLONE_NEWPID)` *không* đổi PID namespace của caller vì làm thế sẽ đổi `getpid()` của chính nó[^pid-unshare]; bởi vậy dòng "caller after unshare" có `pid` giữ nguyên số inode cũ còn `pid_for_children` đã khác[^ns-pidchildren]. Con đầu tiên sinh ra sau đó có PID 1 trong namespace mới[^pid-1] và đóng vai init: nó nhận nuôi mọi orphan của namespace[^pid-init]. Cha của nó nằm ngoài namespace nên `getppid()` trả 0[^pid-ppid0]. `readlink /proc/self` trong con **không** ra 1: `/proc` vẫn là procfs mount từ namespace cũ, và procfs chỉ hiện process thấy được trong PID namespace của process đã mount nó[^pid-proc]; nên `/proc/self` cho PID của con *theo cách nhìn namespace cũ*[^pid-self] — chính là số `pid` mà dòng "parent: forked child pid" in ra.
5. **(3) `mount`.** Namespace mới bắt đầu bằng bản sao danh sách mount của caller[^mnt-copy]; mount/umount ở một bên mặc định không ảnh hưởng bên kia[^mnt-nopropagate]. Code đổi `/` sang private để không còn peer group nào chia sẻ sự kiện[^mnt-private]. Dòng `mountinfo:` phải có đúng một entry kiểu `tmpfs` ở `/tmp/lab-8-2-mnt`; trong `/proc/self/mountinfo` mount private không có thẻ `shared:`/`master:` nào[^mnt-mountinfo]. Nếu bạn giữ process (`--hold`) và xem `/proc/self/mountinfo` từ shell khác ở namespace gốc thì không thấy mount này (suy luận của người soạn từ hai nguồn trên; chạy để kiểm).
6. **(4) Nhìn từ ngoài.** `docker exec` chạy process mới trong namespace *gốc* của container, còn PID 1 (chương trình lab) ở UTS namespace mới: hai dòng `readlink` phải khác nhau[^ns-inode] và `hostname` in tên cũ của container, không phải `lab-ns-demo` (suy luận của người soạn từ hai nguồn: `ns-inode`, `uts-def`).
7. **`mount --userns` trong container lab thường.** Về lý thuyết, giữ `CAP_SYS_ADMIN` trong user namespace sở hữu mount namespace cho phép bind mount và một số filesystem như `tmpfs`[^mnt-userns-fs]. Nhưng chương 02 mục 5 (Thí nghiệm 5) *đã quan sát* trong container lab thường rằng mọi `mount` bên trong namespace do `unshare -Urm` tạo bị từ chối. Lab đã tắt seccomp, và quan sát ở [chương 07, mục 2.3](../08-containers/07-mang-container.md) (`mount --make-shared` chỉ qua khi thêm `apparmor=unconfined`) cho thấy nhiều khả năng AppArmor `docker-default` là lớp chặn (suy luận của người soạn; chương 02 không tách riêng lớp chặn này). Vì vậy nếu bạn chạy `unshare_ns mount --userns` bằng `linux.sh` mà `mount` thất bại thì đó là kết quả *đã được chương dự báo*, không phải lỗi code lab.

**Câu hỏi.**

1. Tại sao dùng `fork` sau `unshare(CLONE_NEWPID)`, mà không đọc `getpid()` ngay trong process gọi `unshare`?
   <details><summary>Đáp án</summary>

   Vì `unshare(CLONE_NEWPID)` không chuyển caller vào namespace mới[^pid-unshare]; chỉ con đầu tiên sinh ra sau đó là PID 1[^pid-1]. `unshare(1)` có option `--fork` vì lý do này[^unshare1-fork].

   </details>

2. Sau khi con PID 1 thoát, cha gọi `fork` lần nữa trong cùng PID namespace mới (bài tập (a) bên dưới). Dự đoán kết quả và gọi tên `errno`.
   <details><summary>Đáp án</summary>

   Nếu con đầu tiên đã kết thúc thì các `fork` sau trả `ENOMEM`[^pid-enomem]; và một process chỉ được gọi `unshare` với `CLONE_NEWPID` một lần[^pid-once].

   </details>

3. Vì sao đổi `/` sang `MS_PRIVATE` trước khi mount `tmpfs`? Điều gì có thể xảy ra nếu không?
   <details><summary>Đáp án</summary>

   Mount shared chia sẻ sự kiện với peer group[^mnt-shared]; private thì không có peer group nên không lan vào hoặc ra[^mnt-private]. Mount trên một cây còn shared có khả năng lan sang namespace gốc nếu hai bên còn là peer (suy luận của người soạn từ định nghĩa shared); chương 02 mục 5 nói `unshare(1)` mặc định đặt private vì lý do tương tự.

   </details>

4. Vì sao trước khi ghi `gid_map` phải ghi `deny` vào `setgroups`?
   <details><summary>Đáp án</summary>

   Quy tắc của kernel cho process không đặc quyền: phải `deny` trước khi ghi `gid_map`; `deny` ngăn mọi process trong user namespace dùng `setgroups(2)`[^user-setgroups]. Chương 02 mục 6 nêu quy tắc cùng với giới hạn "một dòng map EUID"[^user-unpriv].

   </details>

**Bài tập mở rộng (8.2).** (a) Trong nhánh `pid`, thêm một `fork` thứ hai *sau* khi con đầu tiên đã `wait`; in `errno`. (b) Gọi `unshare(CLONE_NEWPID)` hai lần trong cùng process. (c) Trong nhánh `uts --userns`, đổi thứ tự: gọi `sethostname` *trước* khi ghi map và xem có khác không (gợi ý: capability có ngay khi vào user namespace[^user-caps]; map là việc của ID). (d) Thêm `CLONE_NEWIPC` và đọc xem `ipcs -q` có thấy queue tạo trong namespace mới không (chương 02, mục 7).

## 4. Bài 8.3 — `cgroup_limits`: container bị giới hạn bao nhiêu, và Rust thấy thế nào?

**Mục tiêu.** Đọc các file giới hạn cgroup v2 của chính process bạn (`cpu.max`, `memory.max`, `memory.high`, `cpu.stat`, `memory.current`, `memory.events`, `pids.max`), tự tính "quota ra mấy core", rồi đặt cạnh `available_parallelism()`, `sysconf(_SC_NPROCESSORS_ONLN)` và số CPU trong affinity mask.

**Đọc trước.** [Chương 03 — cgroups v2](../08-containers/03-cgroups-v2.md): mục 2 (cgroupfs và các file lõi), mục 4 (controller memory), mục 5 (cpu, io, pids), mục 8 (vì sao app thấy RAM/CPU của host) và phần "Liên hệ với Rust: `available_parallelism` và cgroup". Thêm [Chương 08, mục 1](../08-containers/08-rust-trong-container.md) (CPU: Rust và Tokio thấy mấy CPU).

**Mã.** `src/bin/cgroup_limits.rs`. Đọc `/proc/self/cgroup` (dòng `0::<đường dẫn>` của v2), ghép với mount `/sys/fs/cgroup`, đọc từng file, parse `cpu.max` (`$MAX $PERIOD`) bằng *phép chia số nguyên*, parse `memory.max`; cuối cùng in ba con số CPU. Code cố ý lặp lại cách std đọc quota (đọc `/proc/self/cgroup`, tại mount `/sys/fs/cgroup`) để bạn so sánh với source.

**Chạy.**

```bash
# (A) Container lab thường: không đặt giới hạn.
./labs/linux.sh ./target-linux/debug/cgroup_limits

# (B) --cpus 1.5 và --memory 256m.
docker run --rm --cpus=1.5 --memory=256m -v "$PWD":/work -w /work/labs rust-foundations-lab:1.95 ./target-linux/debug/cgroup_limits

# (C) --cpus 0.5, rồi --cpus 2.
docker run --rm --cpus=0.5 -v "$PWD":/work -w /work/labs rust-foundations-lab:1.95 ./target-linux/debug/cgroup_limits
docker run --rm --cpus=2 -v "$PWD":/work -w /work/labs rust-foundations-lab:1.95 ./target-linux/debug/cgroup_limits

# (D) --cpuset-cpus: giới hạn bằng affinity thay vì quota.
docker run --rm --cpuset-cpus=0 -v "$PWD":/work -w /work/labs rust-foundations-lab:1.95 ./target-linux/debug/cgroup_limits
```

**Dự đoán (từ lý thuyết).**

1. **Đường dẫn.** v2 gom mọi controller vào một hierarchy duy nhất[^cg7-unified], nên `/proc/self/cgroup` chỉ có một dòng `0::<path>`; trong container có cgroup namespace riêng, cgroup hiện tại thành thư mục gốc của namespace[^cgns-root], nên chương 03 mục 2 quan sát `0::/` và code ghép ra `/sys/fs/cgroup`. Nếu bạn chạy ngoài container và đường dẫn dài hơn thì code vẫn ghép đúng.
2. **(A) không giới hạn.** Giá trị mặc định của `cpu.max` là `max 100000` (nghĩa là không trần)[^cg-cpumax-default] và của `memory.max` là `max` (kernel doc ghi mặc định)[^cg-memmax-default]; "quota" in ra "no CPU quota parsed" và `memory.max = unlimited`. Cả hai file chỉ tồn tại ở cgroup *không phải root*: nếu chương trình báo lỗi `No such file or directory` thì rất có thể bạn đang ở cgroup gốc (suy luận của người soạn: không đọc được file không có nghĩa là không giới hạn).
3. **(B) `--cpus=1.5`.** Chương 08 mục 1 đã quan sát Docker dịch `--cpus=1.5` thành `cpu.max = 150000 100000`. Ý nghĩa: nhóm được dùng tối đa `$MAX` trong mỗi `$PERIOD`[^cg-cpu-max]; hết quota thì mọi thread của nhóm bị throttle tới period kế[^cg-bwc]. Phép chia số nguyên ra `1` core. `memory.max = 268435456` byte = 256 × 1024 × 1024 (số học của người soạn), và đó là trần cứng: chạm trần mà không giảm được thì OOM killer chạy trong cgroup[^cg-max].
4. **`available_parallelism()`.** Docs 1.95.0 nói nó là *ước lượng* mức song song nên dùng[^rs-ap] và có thể đếm dư khi không đọc được affinity/cgroup (ví dụ trong sandbox)[^rs-over]; tức bình thường nó **có** tính quota. Source 1.95.0 (chi tiết cài đặt, có thể đổi): `quota()` trả quota "core-equivalents, rounded down"[^rs-quota-doc], lấy `quota.min(limit / period)` khi leo qua các cgroup tổ tiên[^rs-min], rồi `count.min(quota)` với số CPU trong affinity[^rs-affinity]; đọc `/proc/self/cgroup` để biết đường dẫn (trường thứ hai rỗng là v2)[^rs-proc-cgroup] và dùng mount chuẩn[^rs-mount]. Dự đoán suy ra: kết quả là `min(affinity, floor(quota/period))`, tối thiểu 1 (suy luận của người soạn từ các dòng source; chương 03 và 08 ghi các lần đo khớp). Vậy (B) → `1` (không phải 2), (C `--cpus=0.5`) → `1` (làm tròn xuống ra 0 nhưng tối thiểu 1), (C `--cpus=2`) → `2`.
5. **Hai con số CPU không theo quota.** `sysconf(_SC_NPROCESSORS_ONLN)` và số bit trong `sched_getaffinity` được in để đối chiếu: chương 03 mục 8 và chương 08 mục 1 *quan sát* `nproc` không đổi theo `--cpus` (vẫn bằng số CPU của VM) nhưng đổi theo `--cpuset-cpus`. Lab in thêm `sysconf` mà chương chưa đo; dự đoán duy nhất có cơ sở là affinity mask (D) giảm còn 1 CPU, và `available_parallelism` theo nó (source đọc affinity rồi `min` với quota[^rs-affinity]). Giá trị `sysconf` bạn tự quan sát, đừng suy từ tài liệu ở đây.
6. **Bộ nhớ không có tương đương `available_parallelism`.** `/proc/meminfo` định nghĩa `MemTotal` là tổng RAM khả dụng của máy[^proc-memtotal]; chương 03 mục 8 quan sát nó vẫn là RAM của VM dù `memory.max` nhỏ hơn. Muốn biết trần thật phải đọc `memory.max` — đúng việc lab này làm.
7. `cpu.stat` có các trường `nr_periods`, `nr_throttled`, `throttled_usec`, `nr_bursts`, `burst_usec` khi controller cpu bật[^cg-cpu-stat]; chạy ngay như bài này (không burn CPU) thì `nr_throttled` hầu như không tăng (suy luận của người soạn: chương trình chỉ chạy một thoáng).
8. **`--cpus=0.5`: lab và std lệch nhau có chủ ý.** Dòng `quota/period (integer division)` của lab là phép chia số nguyên thuần nên in `0 core(s)` khi `cpu.max` là `50000 100000`; còn `available_parallelism()` làm tròn xuống[^rs-quota-doc] rồi thêm bước tối thiểu 1 (chương 03, phần Liên hệ với Rust), nên vẫn in `1`. Bạn đối chiếu hai dòng đó để thấy chỗ lab không bắt chước std.

**Câu hỏi.**

1. Một service Rust trong pod `limits.cpu: 1500m` tạo pool tokio theo `available_parallelism()`. Pool có bao nhiêu worker, và 0,5 core quota còn lại đi đâu?
   <details><summary>Đáp án</summary>

   1 worker (làm tròn xuống[^rs-quota-doc]); 0,5 core bị bỏ phí nếu bạn không chỉnh tay (chương 03, phần Liên hệ với Rust, nói đúng điều này với `--cpus 1.5`). Số worker của Tokio khớp `available_parallelism` trong các lần đo của chương 08 mục 1, nhưng docs không hứa điều đó.

   </details>

2. Quota 150000/100000 và một chương trình chạy 4 thread song song. Quota được tính theo core hay theo thời gian?
   <details><summary>Đáp án</summary>

   Theo thời gian CPU của mọi thread: trong mỗi period nhóm được tối đa `quota` micro giây CPU, hết thì các thread bị throttle tới period sau[^cg-bwc]. Chương 03 mục 5 suy luận hệ quả: 4 thread song song cạn quota sau khoảng 37,5 ms của mỗi 100 ms (suy luận của người soạn từ cơ chế).

   </details>

3. `memory.current` có tính page cache không? Vì sao điều đó quan trọng với `memory.max`?
   <details><summary>Đáp án</summary>

   Có: kernel doc định nghĩa `file` trong `memory.stat` là bộ nhớ cache dữ liệu filesystem, gồm cả tmpfs và shared memory[^cg-stat-file]; `memory.current` là tổng đang dùng của cgroup và con cháu[^cg-cur]. Page cache reclaim được nên thường không gây OOM kill, nhưng đẩy `memory.current` sát trần (chương 03 mục 4).

   </details>

**Bài tập mở rộng (8.3).** (a) Chạy `docker run --rm --cpus=0.2 ... sh -c 'timeout 3 sh -c "while :; do :; done"; cat /sys/fs/cgroup/cpu.stat'` và đọc `nr_throttled`, đối chiếu với cơ chế throttle ở CFS bandwidth control[^cg-bwc] (dự đoán: > 0). (b) Chạy `dd if=/dev/zero of=/tmp/big bs=1M count=300` trong container `--memory=50m` rồi đọc `memory.current`/`memory.events` (chương 03 mục 4 có kết quả đã quan sát để so). (c) Đặt `memory.max` thấp hơn `memory.current` bằng `docker update --memory` trên container đang chạy và xem điều gì xảy ra (câu hỏi mở: kernel doc `memory.max` nói OOM killer chạy khi không giảm được[^cg-max]). (d) Sửa code để leo ngược các cgroup tổ tiên như std (`while path.starts_with(cgroup_mount)`) và lấy `min`: đối chiếu với `quota.min(limit / period)`[^rs-min].

## 5. Bài 8.4 — `mini_container`: `unshare` + `fork` + `pivot_root` vào một rootfs nhỏ

**Mục tiêu.** Dựng bằng tay phần lõi mà runc làm khi tạo container: namespace mới, PID 1, đổi root bằng `pivot_root` (hoặc `chroot` để so sánh), mount `/proc`, `exec` một chương trình. Sau bài này `docker run` không còn là phép màu: nó là chuỗi syscall bạn đọc được trong ~170 dòng.

**Đọc trước.** [Chương 04 — Filesystem của container](../08-containers/04-filesystem-cua-container.md), mục 1 (`chroot`, `pivot_root` và trình tự trong runtime) — đọc kỹ bảng ràng buộc và sơ đồ trình tự; [Chương 02](../08-containers/02-namespaces.md) mục 4 (PID 1) và mục 5 (mount namespace); [Chương 06](../08-containers/06-runtime-oci.md) mục 2 (chuỗi tiến trình khi `docker run`).

**Mã.** `src/bin/mini_container.rs`; cú pháp `mini_container <rootfs> [--chroot] [--leak-fd] [-- lệnh args...]`. Trình tự trong code:

1. Cha: `unshare(CLONE_NEWNS | CLONE_NEWPID | CLONE_NEWUTS)` rồi `fork` (cùng tập cờ như `unshare --mount --pid --uts --fork`).
2. Con (là PID 1 của namespace mới): `sethostname("mini")`, rồi **nhánh `pivot_root`** — `mount("/", MS_SLAVE|MS_REC)`, bind mount rootfs lên chính nó, `open("/")` giữ root cũ, `fchdir(rootfs)`, `pivot_root(".", ".")`, `fchdir(oldroot)`, `mount(".", MS_SLAVE|MS_REC)`, `umount2(".", MNT_DETACH)`, `chdir("/")` — hoặc **nhánh `--chroot`**: `chroot(rootfs)` + `chdir("/")`.
3. Nếu rootfs có `/proc`: mount procfs mới.
4. Không có lệnh: in báo cáo (PID, `ls /`, vài dòng `mountinfo`) rồi thoát. Có lệnh: `execvp`.
5. Cha `waitpid` và trả mã thoát của con.

Đây là bản rút gọn của trình tự trong runc v1.2.5: `prepareRoot` đặt `/` về `MS_SLAVE | MS_REC`[^runc-prep-slave], bind mount rootfs lên chính nó[^runc-prep-bind]; `pivotRoot` gọi `pivot_root(".", ".")`; rồi đặt root cũ `rslave`[^runc-rslave] và `umount2(".", MNT_DETACH)`[^runc-detach] (đọc [chương 04 mục 1](../08-containers/04-filesystem-cua-container.md) để thấy sơ đồ đầy đủ). Nó **không** làm: user namespace, cgroup, mount `/dev` và `/sys`, capability, seccomp, mạng, đọc OCI bundle.

**Dựng rootfs và chạy.** Cần một thư mục có sẵn vài chương trình và các thư viện chúng nạp. Mỗi `docker run --rm` là một container mới (mất `/tmp`), nên mở **một shell privileged** và làm mọi thứ trong đó:

```bash
docker run -it --rm --privileged -v "$PWD":/work -w /work/labs rust-foundations-lab:1.95 bash
```

Công thức dựng rootfs dưới đây dùng `ldd` để chép thư viện. Tôi **không chạy** nó khi soạn tài liệu (quy ước repo: không chạy thí nghiệm cho phần lý thuyết), nên nếu `ldd` in dòng có dạng khác, bạn tự thêm tay thư viện còn thiếu:

```bash
# trong shell privileged
R=/tmp/rootfs; mkdir -p $R/bin $R/proc $R/tmp
add() {                                   # chép chương trình $1 vào $R/bin cùng thư viện nó nạp
  cp "$1" "$R/bin/$(basename "$1")"
  ldd "$1" | grep -o '/[[:graph:]]*' | while read -r lib; do cp --parents "$lib" "$R"; done
}
for b in /bin/sh /bin/ls /bin/cat /bin/ps /bin/hostname /work/labs/target-linux/debug/pid1; do add "$b"; done
echo host-only > /tmp/host-only.txt       # có ở cây của container, KHÔNG có trong rootfs
M=/work/labs/target-linux/debug/mini_container
```

Rồi các biến thể (lần lượt, trong cùng shell):

```bash
$M $R                                                 # (1) báo cáo mặc định, pivot_root
$M $R -- /bin/sh -c 'ps -e; hostname; ls /; ls /tmp'  # (2) chạy một lệnh trong container
$M $R --chroot                                        # (3) cùng báo cáo nhưng bằng chroot
$M $R --chroot --leak-fd                              # (4) giữ fd tới root cũ, chroot
$M $R --leak-fd                                       # (5) giữ fd tới root cũ, pivot_root
```

Và một lần **không đặc quyền**, từ máy bạn (container lab thường, không `--privileged`):

```bash
./labs/linux.sh bash -c 'mkdir -p /tmp/rootfs/proc; ./target-linux/debug/mini_container /tmp/rootfs'
```

**Dự đoán (từ lý thuyết).**

1. **Không đặc quyền.** `unshare` trả `EPERM` vì thiếu `CAP_SYS_ADMIN`[^ns-priv][^clone-eperm]; chương trình in dòng hướng dẫn và thoát mã 1 — chưa có gì thay đổi trong hệ thống.
2. **PID.** Con là process đầu tiên sinh ra sau `unshare(CLONE_NEWPID)` nên có PID 1 trong namespace mới[^pid-1] và là init của nó[^pid-init]; `unshare` không chuyển cha vào namespace mới[^pid-unshare]. Dòng báo cáo `pid = 1`, `ppid = 0`[^pid-ppid0].
3. **Nếu con PID 1 thoát**, kernel giết mọi process còn lại trong namespace bằng `SIGKILL`[^pid-initdie]. Với báo cáo mặc định không có process khác, nên không quan sát được; xem 8.5.
4. **`ls /` sau `pivot_root`** chỉ thấy các entry của rootfs: `pivot_root` đổi root mount của mount namespace và đặt root cũ dưới `new_root` để gỡ được[^pr-desc][^pr-unmount]; code gỡ nó bằng `MNT_DETACH`. Dòng đầu tiên báo cáo `ls /` phải liệt kê `bin`, `lib` (hoặc `lib64`/`usr` tùy các thư viện `ldd` in ra), `proc`, `tmp` (những gì bạn dựng) và *không* có thư mục của container ngoài rootfs. (Suy luận của người soạn: tên cụ thể phụ thuộc vào công thức bạn dùng; riêng `/tmp/host-only.txt` không được phép hiện — chương 04 mục 1, thí nghiệm 2-C, quan sát tương tự với `/host-secret`.)
5. **`mountinfo` sau `pivot_root`.** Mount ở ngoài thư mục root của process không xuất hiện trong `mountinfo`[^proc-mountinfo-outside]; chương 04 (thí nghiệm 2-C) quan sát sau `pivot_root` + `umount -l` chỉ còn root và `/proc`. Dự đoán số dòng nhỏ (2 trong lần quan sát đó, trên cấu hình khác; bạn có thể ra khác).
6. **`--chroot` khác `pivot_root` ở đâu?** `chroot` đổi *một con trỏ*: thư mục gốc dùng để phân giải đường dẫn, kế thừa cho con[^chroot-inherit]; nó "không làm gì khác" và không dành cho mục đích bảo mật[^chroot-purpose]. Nó cũng không động tới mount table nên cây mount của container vẫn nằm trong cùng mount namespace (suy luận của người soạn từ câu trên; chương 04 mục 1). Còn `pivot_root` thay root mount và cho phép gỡ root cũ[^pr-desc][^pr-unmount].
7. **`--leak-fd` + `--chroot`.** `chroot` không đóng fd đã mở, nên một fd trỏ ra ngoài cây có thể cho truy cập file ngoài cây[^chroot-fd]; do đó sau `fchdir(fd)`, `ls .` phải liệt kê thư mục gốc *cũ* của container (có `host-only.txt` ở `/tmp` nếu bạn đi tới đó). Nhắc lại hai lối thoát khác trong man page: cwd không đổi[^chroot-cwd].
8. **`--leak-fd` + `pivot_root`: câu hỏi mở.** Man page `pivot_root(2)` và các chú thích runc ở trên không nói trực tiếp điều gì xảy ra với một fd mở tới root cũ sau khi bị `MNT_DETACH`. Runc tự `close` cả hai fd root sau khi xong (code lab cũng vậy cho fd nội bộ). Đây là câu bạn dự đoán bằng suy luận, ghi dự đoán *trước khi* chạy, rồi đối chiếu — đừng coi kết quả của một lần chạy là bảo đảm.
9. **Hostname và `/proc`.** UTS mới nhận bản sao hostname cũ[^uts-copy]; `sethostname("mini")` chỉ đổi trong namespace mới[^uts-def] nên `hostname` trong lệnh `sh -c` in `mini`. `ps -e` (cần procfs) chỉ hiện process thấy được trong PID namespace của process đã mount procfs[^pid-proc]: bạn thấy `sh` là PID 1 và chính `ps`, không thấy gì của container ngoài.
10. **Thiếu một bước.** Bỏ dòng bind mount rootfs lên chính nó thì `pivot_root` phải thất bại: `new_root` phải là mount point và path chưa là mount point thì bind mount nó lên chính nó[^pr-mountpoint]; `new_root` không được nằm trên cùng mount với root hiện tại, vi phạm cho `EBUSY`[^pr-ebusy]. Bỏ dòng `mount("/", MS_SLAVE|MS_REC)` và nếu cây mount của bạn còn `MS_SHARED` thì `EINVAL`[^pr-shared][^pr-einval]. Chương 04 mục 1 (thí nghiệm 2-A, 2-B) đã quan sát đúng hai lỗi này.
11. **`pivot_root(".", ".")` hợp lệ** vì man page cho phép `new_root` và `put_old` là cùng một thư mục[^pr-same]; lời gọi xếp root cũ chồng lên root mới tại `/`[^pr-stack] — đó là lý do code phải `fchdir(old_root)` rồi `umount2(".")`. Runc ghi chú kết quả `/` là root mới nhưng `/proc/self/cwd` là root cũ[^runc-dotdot].

**Câu hỏi.**

1. Vì sao code phải mount `/proc` *sau* khi vào root mới và *từ trong* PID namespace mới (con), chứ không phải ở cha trước khi `fork`?
   <details><summary>Đáp án</summary>

   procfs chỉ hiện process thấy được trong PID namespace của process đã mount nó[^pid-proc]. Mount từ cha (namespace cũ) thì `ps` vẫn thấy toàn bộ PID cũ; nếu đồng thời có mount namespace thì có thể mount procfs mới thẳng lên `/proc` mà không cần đổi root[^pid-procmount].

   </details>

2. Vì sao runtime dùng `pivot_root` thay vì `chroot` dù cả hai đều cho `ls /` ra rootfs?
   <details><summary>Đáp án</summary>

   `chroot` chỉ thay đổi điểm bắt đầu phân giải đường dẫn và không dùng cho bảo mật[^chroot-purpose] (fd mở, cwd và thư mục bị di chuyển đều là lối thoát[^chroot-fd][^chroot-cwd]); `pivot_root` thay root mount rồi cho gỡ root cũ[^pr-unmount]. Trong mã runc, nhánh `chroot` chỉ còn khi không có mount namespace[^runc-newns] (chương 04 mục 1).

   </details>

3. `pid1` được copy vào rootfs trong công thức dựng. Bạn chạy `$M $R -- /bin/pid1 ignore &` rồi `kill -TERM` tới nó từ shell cha: process nào là PID 1 của namespace mới, và signal có tới nó không?
   <details><summary>Đáp án</summary>

   Con của `mini_container` là PID 1 và `execvp` thay nó bằng `pid1`, nên `pid1` là PID 1 của namespace. Mode `ignore` không cài handler; từ namespace tổ tiên, `SIGTERM` chỉ được giao nếu init có handler[^pid-anc-handler] (`SIGKILL`/`SIGSTOP` là ngoại lệ[^pid-ancestor]). Chi tiết ở bài 8.5.

   </details>

4. Chương trình có thể chạy ở macOS không? Vì sao tài liệu viết `docker run --privileged` thay vì chạy trên máy?
   <details><summary>Đáp án</summary>

   Không: macOS không có các namespace này; Docker trên macOS chạy container trong một VM Linux (chương 01 mục 4; ở môi trường soạn là VM của Colima), nên bài này chạy trong VM đó. `--privileged` cấp mọi capability[^docker-priv], đủ cho `CAP_SYS_ADMIN` mà `unshare`/`pivot_root` đòi[^ns-priv][^pr-cap].

   </details>

**Bài tập mở rộng (8.4).** (a) Bỏ từng bước của trình tự pivot_root (bind mount, `rslave` đầu, `umount2`) và ghi lại `errno`/hành vi, đối chiếu với dự đoán 10. (b) Thêm `CLONE_NEWUSER` vào `unshare` cùng map `0 <uid> 1` để chạy không cần `--privileged`; theo chương 02 mục 5, rất có thể bước mount vẫn bị chặn trong container lab thường (đây là quan sát đã ghi, không phải khẳng định chung; AppArmor mặc định là nghi phạm chính theo [chương 07, mục 2.3](../08-containers/07-mang-container.md)). (c) Mount thêm `tmpfs` lên `/tmp` trong rootfs và `bind mount` thư mục `/work` chỉ đọc; đọc [chương 04 mục 5](../08-containers/04-filesystem-cua-container.md) về read-only/bind/tmpfs. (d) Thêm `CLONE_NEWNET` và dùng `ip link` (có trong image) để thấy một namespace chỉ có `lo` ([chương 07 mục 2](../08-containers/07-mang-container.md)).

## 6. Bài 8.5 — `pid1`: PID 1, signal và graceful shutdown

**Mục tiêu.** Thấy tận mắt cùng một binary đáp `docker stop` theo hai cách khác nhau tùy việc nó có handler `SIGTERM` hay không; thấy `--init` thay đổi điều gì (và không thay đổi điều gì); thấy một zombie thật, và đọc mặt nạ signal `SigCgt`/`SigIgn` từ `/proc/self/status`.

**Đọc trước.** [Chương 08 — Rust trong container](../08-containers/08-rust-trong-container.md), mục 3 (PID 1 và signal) và mục 4 (graceful shutdown bằng Rust); [Chương 02, mục 4](../08-containers/02-namespaces.md) (PID namespace: "init bất tử"). Về signal và zombie: [01-linux/05-signal.md](../01-linux/05-signal.md), [01-linux/03-process.md](../01-linux/03-process.md).

**Mã.** `src/bin/pid1.rs`; cú pháp `pid1 [graceful|ignore|zombie]`. Cả ba mode in trước `Pid`, `NSpid`, `PPid`, `SigIgn`, `SigCgt` từ `/proc/self/status`.

- `graceful` (mặc định): cài handler `SIGTERM` và `SIGINT` bằng `libc::sigaction`, handler chỉ làm một `AtomicI32::store`; vòng chính poll cờ 20 ms/lần; nhận signal thì "drain" 200 ms rồi trả về → exit 0.
- `ignore`: không cài handler nào, ngủ vô hạn.
- `zombie`: `fork` một con thoát ngay, cha *không* `wait`, đọc trạng thái trong `/proc/<pid>/stat`, rồi `waitpid(-1)` và đọc lại.

Handler chỉ ghi vào một biến atomic vì tập hàm gọi được an toàn trong signal handler rất hạn chế — man page `signal-safety(7)` đòi handler chỉ gọi hàm async-signal-safe[^sigsafe-rule] (nhận xét của người soạn: một `store` atomic không phải lời gọi hàm libc, nên hợp quy tắc đó; `println!` thì không).

**Chạy.**

```bash
BIN=/work/labs/target-linux/debug/pid1
run1() { docker run -d -v "$PWD":/work -w /work/labs "$@"; }

# (A) có handler: docker stop -> thoát nhanh, mã 0
run1 --name p1-graceful rust-foundations-lab:1.95 $BIN graceful
sleep 1; time docker stop p1-graceful
docker inspect -f '{{.State.ExitCode}}' p1-graceful; docker logs p1-graceful

# (B) không handler, PID 1: docker stop phải chờ hết grace period (rút ngắn còn 3 s)
run1 --name p1-ignore rust-foundations-lab:1.95 $BIN ignore
sleep 1; time docker stop -t 3 p1-ignore
docker inspect -f '{{.State.ExitCode}}' p1-ignore; docker logs p1-ignore

# (C) như (B) nhưng có --init
run1 --init --name p1-init rust-foundations-lab:1.95 $BIN ignore
sleep 1; time docker stop -t 3 p1-init
docker inspect -f '{{.State.ExitCode}}' p1-init; docker logs p1-init

# (D) zombie
docker run --rm -v "$PWD":/work -w /work/labs rust-foundations-lab:1.95 $BIN zombie

# dọn
docker rm -f p1-graceful p1-ignore p1-init 2>/dev/null
```

**Dự đoán (từ lý thuyết).**

1. **(A) `graceful`.** `docker stop` gửi `SIGTERM` tới process chính rồi `SIGKILL` sau một grace period[^docker-stop]. Vì PID 1 *có* handler cho `SIGTERM`, signal được giao (chỉ signal có handler mới tới được init từ bên trong[^pid-sig]; từ namespace tổ tiên cũng vậy[^pid-anc-handler], riêng `SIGKILL`/`SIGSTOP` là ngoại lệ[^pid-ancestor]) → log có dòng "caught signal 15; draining for 200ms" rồi "drained", `docker stop` về gần như ngay (thời gian ≈ độ trễ drain 200 ms + overhead lệnh; chương 08 mục 3 đo 0,30 s cho chương trình tương tự, một lần chạy) và `ExitCode = 0`.
2. **(B) `ignore`.** PID 1 không có handler nên `SIGTERM` từ ancestor namespace *không được giao*[^pid-anc-handler]; `docker stop` chờ hết grace period (mặc định 10 giây trên Linux[^docker-timeout]; lệnh trên rút xuống 3 giây) rồi gửi `SIGKILL`, signal này không bắt/chặn/bỏ qua được[^sig-kill]. Kết quả: `docker stop -t 3` mất khoảng 3 giây (chương 08 mục 3 đo 3,06 s cho `--stop-timeout 3`) và `ExitCode = 137` = 128 + 9 (quy ước mã thoát khi chết vì signal: nhận xét của người soạn, như chương 08).
3. **Mặt nạ signal.** `SigIgn`/`SigCgt` là mặt nạ hex các signal bị bỏ qua / bị bắt[^proc-sigmask]; bit thứ *n* tương ứng signal *n*+1 (số học của người soạn, như chương 08 mục 3). `SIGTERM` là signal 15[^signal-nums] nên mặt nạ là `0x4000`; `SIGINT` là 2 → `0x2`. Mode `graceful`: `SigCgt` phải chứa `0x4002`; mode `ignore`: `SigCgt` không có bit `0x4000`. Chương 08 mục 3 đã quan sát một process Rust 1.95.0 chỉ ngủ có `SigIgn = 0x1000` (SIGPIPE, vì runtime Rust đặt `SIGPIPE` thành `SIG_IGN`[^rs-sigpipe]) và `SigCgt = 0x440` (SIGBUS, SIGSEGV; handler của std cho stack overflow) — suy ra (suy luận của người soạn) mode `graceful` in `SigCgt` khoảng `0x4442`, `ignore` in `0x440`; bạn kiểm.
4. **(C) `--init`.** Docker mô tả `--init` là chạy một init trong container để chuyển signal và reap process[^docker-init]; binary `docker-init` dựa trên tini[^docker-init-tini]. App không còn là PID 1 (dòng `Pid:` ≠ 1 và `NSpid:` liệt kê PID trong từng namespace[^proc-nspid]); `SIGTERM` được init chuyển tới nó và vì app *không* có handler, hành động mặc định của `SIGTERM` là `Term`[^sigterm-term] → chết ngay, khoảng 0,1 s (chương 08 mục 3 đo 0,11 s) với `ExitCode = 143` = 128 + 15[^signal-nums]. **Nhưng đây không phải graceful**: không có chỗ nào chạy "drain".
5. **(D) `zombie`.** Con thoát nhưng cha chưa `wait` thì thành zombie: kernel giữ PID, trạng thái thoát, số liệu tài nguyên cho tới khi cha `wait`[^wait-zombie]. Trường 3 của `/proc/<pid>/stat` là chữ cái trạng thái; zombie là `Z`[^proc-stat-state] (dòng "child … state = Z"). Sau `waitpid(-1)`, entry `/proc/<pid>` biến mất (dòng cuối `exists afterwards: false`) — suy luận của người soạn từ câu "zombie ... cho tới khi cha wait", chạy để kiểm. Đây là lý do PID 1 phải reap: mọi process mồ côi trong namespace được nhận nuôi bởi init[^pid-init].
6. **Ngoài container.** Chạy `./target-linux/debug/pid1 ignore` ở một shell Linux bình thường (không phải PID 1) rồi `kill -TERM <pid>` từ shell khác phải giết nó: không phải init nên không có đặc cách; hành động mặc định `Term`[^sigterm-term]. Cùng binary, cùng signal, khác kết quả chỉ vì PID 1 — thí nghiệm sạch nhất của bài. (Cách làm trên macOS: `./labs/linux.sh bash`, rồi trong shell tương tác chạy `./target-linux/debug/pid1 ignore &` và `kill -TERM %1`; ở đây bash là PID 1 của container còn `pid1` là con của nó, PID khác 1. Tôi không chạy thử khi soạn.)

**Câu hỏi.**

1. Vì sao `docker stop` ở mode `ignore` mất đúng bằng grace period, và thông số nào chỉnh nó?
   <details><summary>Đáp án</summary>

   Signal `SIGTERM` gửi từ ngoài namespace tới PID 1 không có handler bị kernel không giao[^pid-anc-handler], nên Docker chờ hết grace period rồi gửi `SIGKILL`[^docker-stop]; mặc định 10 giây trên Linux[^docker-timeout], chỉnh bằng `docker stop -t` hoặc `--stop-timeout` (chương 08 mục 3).

   </details>

2. `--init` làm `docker stop` nhanh hơn ở (C) nhưng log không có dòng "draining". Vì sao, và cách sửa đúng?
   <details><summary>Đáp án</summary>

   `--init` chỉ làm app không còn là PID 1 nên `SIGTERM` có hành động mặc định giết app[^sigterm-term]; app chết ngay, không chạy drain. Cách đúng là chính app cài handler `SIGTERM` (mode `graceful`; ở async Rust là `tokio::signal::unix::signal(SignalKind::terminate())`, chương 08 mục 4). `--init` vẫn đáng dùng khi app fork con, để reap zombie[^docker-init].

   </details>

3. Handler trong `pid1.rs` chỉ làm `store` rồi để vòng chính xử lý. Vì sao không `println!` hoặc drain ngay trong handler?
   <details><summary>Đáp án</summary>

   Handler chỉ nên gọi hàm async-signal-safe và phải reentrant với biến toàn cục[^sigsafe-rule]; stdio không an toàn vì cần buffer tĩnh dùng chung[^sigsafe-stdio]. Mẫu "đặt cờ, xử lý ở vòng chính" là mẫu chương 08 mục 4 (`signal-hook` kiểu poll).

   </details>

4. `kill -KILL` từ namespace *cha* tới PID 1 của container (mode `ignore`) khác `kill -TERM` thế nào?
   <details><summary>Đáp án</summary>

   `SIGKILL` và `SIGSTOP` được giao cưỡng bức khi gửi từ PID namespace tổ tiên[^pid-ancestor]; còn `SIGTERM` thì phải có handler mới tới[^pid-anc-handler]. Khi init chết, kernel giết mọi process trong namespace bằng `SIGKILL`[^pid-initdie].

   </details>

**Bài tập mở rộng (8.5).** (a) Kết hợp 8.4: chép `pid1` vào rootfs (công thức đã có), chạy `mini_container $R -- /bin/pid1 ignore &` trong container privileged, rồi từ shell cha `kill -TERM $(pgrep -x pid1)` — dự đoán: không chết (signal từ ancestor namespace chỉ giao khi có handler[^pid-anc-handler]) — sau đó `kill -KILL $(pgrep -x pid1)` (dự đoán: chết, vì `SIGKILL` được cưỡng bức giao[^pid-ancestor]); chú ý `pgrep -x` khớp *tên process* (`pid1`) chứ không khớp dòng lệnh của `mini_container`. (b) Thêm mode `reaper`: vòng `waitpid(-1, WNOHANG)` để reap con mồ côi, sinh một chuỗi cháu mồ côi, và đếm `State: Z` trong `/proc` trước/sau. (c) Dùng `libc::prctl(PR_SET_CHILD_SUBREAPER)` để biến một process không phải PID 1 thành reaper và xem orphan được nhận nuôi bởi nó[^pid-subreaper] (đọc `prctl(2)` trước). (d) Đặt `STOPSIGNAL`/`--stop-signal` khác (ví dụ `SIGINT`) rồi `docker stop` và đối chiếu với handler đã cài.

## 7. Kiểm tra chất lượng code lab

Trong repo, từ `labs/`: `cargo build -p lab-containers` và `cargo clippy -q -p lab-containers --all-targets -- -D warnings` (macOS: chỉ biên dịch phần "không phải Linux", mỗi binary in hướng dẫn). Để kiểm phần Linux thật, chạy trong container: `./labs/linux.sh bash -c 'rustup component add clippy && cargo clippy -p lab-containers --all-targets -- -D warnings'` (image chưa cài sẵn `clippy`, như ghi ở [01-linux-lab.md](01-linux-lab.md), mục 4.6).

## 8. Câu hỏi tổng hợp (cần output của các bài)

1. Từ output bài 8.1 và 8.2, bạn trả lời thế nào cho câu "container là gì" khi chỉ được dùng hai khái niệm kernel?
   <details><summary>Đáp án</summary>

   Một process (hay nhóm process) mà kernel gắn vào các namespace riêng (nhìn thấy gì: hostname, PID, mount…) và một cgroup (dùng được bao nhiêu); không có "đối tượng container" trong kernel — chương 01 mục 3 trình bày ý này; ba bài đầu cho từng phần: số inode namespace khác nhau (8.1), tạo namespace bằng `unshare` (8.2), giới hạn ở `cpu.max`/`memory.max` (8.3).

   </details>

2. Trong 8.4, những phần nào của `docker run` **không** có trong `mini_container`, và phần nào trong số đó bạn thấy ảnh hưởng tới an toàn nhất?
   <details><summary>Đáp án</summary>

   Không có: user namespace, giới hạn cgroup, capability, seccomp, LSM, `no_new_privs`, mạng, đọc OCI bundle. Về an toàn: `chroot` không cho mục đích bảo mật[^chroot-purpose]; lớp phòng thủ khác được bàn ở [chương 05](../08-containers/05-bao-mat-container.md) — trả lời theo ý bạn và đối chiếu với mục 1 của chương đó.

   </details>

3. Gộp 8.3 và 8.5: một service Rust chạy trong pod 1500m, PID 1, không handler `SIGTERM`. Liệt kê hai điều sai về vận hành và cách sửa mỗi điều.
   <details><summary>Đáp án</summary>

   (1) `available_parallelism()` ra 1 (làm tròn xuống[^rs-quota-doc]): thread pool mặc định bỏ phí 0,5 core nên chỉnh tay hoặc đặt số worker rõ ràng; (2) PID 1 không handler `SIGTERM` nên bị `SIGKILL` sau grace period[^pid-anc-handler][^docker-stop]: cài handler và drain có thời hạn (bài 8.5, chương 08 mục 4). Số 1500m → 1 core là suy ra từ chương 03; con số quota thực tế bạn đọc bằng 8.3.

   </details>

[^ns-priv]: namespaces(7), The namespaces API — <https://manpages.debian.org/bookworm/manpages/namespaces.7.en.html> — "Creation of new namespaces using clone(2) and unshare(2) in most cases requires the CAP_SYS_ADMIN capability"
[^pr-cap]: pivot_root(2), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/pivot_root.2.en.html> — "The calling process must have the CAP_SYS_ADMIN capability in the user namespace that owns the caller's mount namespace."
[^clone-eperm]: clone(2), ERRORS — <https://manpages.debian.org/bookworm/manpages-dev/clone.2.en.html> — "CLONE_NEWCGROUP, CLONE_NEWIPC, CLONE_NEWNET, CLONE_NEWNS, CLONE_NEWPID, or CLONE_NEWUTS was specified by an unprivileged process (process without CAP_SYS_ADMIN)."
[^ns-userexc]: namespaces(7), The namespaces API — <https://manpages.debian.org/bookworm/manpages/namespaces.7.en.html> — "User namespaces are the exception: since Linux 3.8, no privilege is required to create a user namespace."
[^docker-priv]: Docker Docs, docker container run, Escalate container privileges — <https://docs.docker.com/reference/cli/docker/container/run/> — "The --privileged flag gives the following capabilities to a container: Enables all Linux kernel capabilities"
[^ns-ptrace]: namespaces(7), The /proc/[pid]/ns/ directory — <https://manpages.debian.org/bookworm/manpages/namespaces.7.en.html> — "Permission to dereference or read (readlink(2)) these symbolic links is governed by a ptrace access mode PTRACE_MODE_READ_FSCREDS check; see ptrace(2)."
[^ns-dir]: namespaces(7), The /proc/[pid]/ns/ directory — <https://manpages.debian.org/bookworm/manpages/namespaces.7.en.html> — "Each process has a /proc/pid/ns/ subdirectory containing one entry for each namespace that supports being manipulated by setns(2)"
[^ns-time-children]: namespaces(7), The /proc/[pid]/ns/ directory — <https://manpages.debian.org/bookworm/manpages/namespaces.7.en.html> — "This file is a handle for the time namespace of child processes created by this process."
[^ns-pidchildren]: namespaces(7), The /proc/[pid]/ns/ directory — <https://manpages.debian.org/bookworm/manpages/namespaces.7.en.html> — "This can change as a consequence of calls to unshare(2) and setns(2) (see pid_namespaces(7)), so the file may differ from /proc/pid/ns/pid."
[^ns-inode]: namespaces(7), The /proc/[pid]/ns/ directory — <https://manpages.debian.org/bookworm/manpages/namespaces.7.en.html> — "If two processes are in the same namespace, then the device IDs and inode numbers of their /proc/pid/ns/xxx symbolic links will be the same"
[^user-initial]: user_namespaces(7), User and group ID mappings — <https://manpages.debian.org/bookworm/manpages/user_namespaces.7.en.html> — "This mapping tells us that the range starting at user ID 0 in this namespace maps to a range starting at 0 in the (nonexistent) parent namespace, and the length of the range is the largest 32-bit unsigned integer."
[^user-map-empty]: user_namespaces(7), User and group ID mappings — <https://manpages.debian.org/bookworm/manpages/user_namespaces.7.en.html> — "When a user namespace is created, it starts out without a mapping of user IDs (group IDs) to the parent user namespace."
[^user-overflow]: user_namespaces(7), Unmapped user and group IDs — <https://manpages.debian.org/bookworm/manpages/user_namespaces.7.en.html> — "the default value for the overflow user ID (group ID) is 65534."
[^pid-proc]: pid_namespaces(7), /proc and PID namespaces — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "only processes visible in the PID namespace of the process that performed the mount, even if the /proc filesystem is viewed from processes in other namespaces."
[^ns-lifetime]: namespaces(7), Namespace lifetime — <https://manpages.debian.org/bookworm/manpages/namespaces.7.en.html> — "Absent any other factors, a namespace is automatically torn down when the last process in the namespace terminates or leaves the namespace."
[^user-first]: user_namespaces(7), Interaction of user namespaces and other types of namespaces — <https://manpages.debian.org/bookworm/manpages/user_namespaces.7.en.html> — "the user namespace is guaranteed to be created first, giving the child (clone(2)) or caller (unshare(2)) privileges over the remaining namespaces created by the call."
[^user-caps]: user_namespaces(7), Capabilities — <https://manpages.debian.org/bookworm/manpages/user_namespaces.7.en.html> — "The child process created by clone(2) with the CLONE_NEWUSER flag starts out with a complete set of capabilities in the new user namespace."
[^user-hostname]: user_namespaces(7), Interaction of user namespaces and other types of namespaces — <https://manpages.debian.org/bookworm/manpages/user_namespaces.7.en.html> — "the kernel will determine which user namespace owns the process's UTS namespace, and check whether the process has the required capability (CAP_SYS_ADMIN) in that user namespace."
[^unshare-thread]: unshare(2), CLONE_NEWUSER — <https://manpages.debian.org/bookworm/manpages-dev/unshare.2.en.html> — "CLONE_NEWUSER requires that the calling process is not threaded"
[^user-unpriv]: user_namespaces(7), Defining user and group ID mappings — <https://manpages.debian.org/bookworm/manpages/user_namespaces.7.en.html> — "The data written to uid_map (gid_map) must consist of a single line that maps the writing process's effective user ID (group ID) in the parent user namespace to a user ID (group ID) in the user namespace."
[^user-setgroups]: user_namespaces(7), The /proc/[pid]/setgroups file — <https://manpages.debian.org/bookworm/manpages/user_namespaces.7.en.html> — "Writing the string deny prevents any process in the user namespace from employing setgroups(2)."
[^uts-copy]: uts_namespaces(7), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/uts_namespaces.7.en.html> — "the hostname and domain name of the new UTS namespace are copied from the corresponding values in the caller's UTS namespace."
[^uts-def]: uts_namespaces(7), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/uts_namespaces.7.en.html> — "UTS namespaces provide isolation of two system identifiers: the hostname and the NIS domain name."
[^pid-unshare]: pid_namespaces(7), setns(2) and unshare(2) semantics — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "These calls do not, however, change the PID namespace of the calling process, because doing so would change the caller's idea of its own PID (as reported by getpid()), which would break many applications and libraries."
[^pid-1]: pid_namespaces(7), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "PIDs in a new PID namespace start at 1, somewhat like a standalone system"
[^pid-init]: pid_namespaces(7), The namespace init process — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "This process becomes the parent of any child processes that are orphaned because a process that resides in this PID namespace terminated"
[^pid-ppid0]: pid_namespaces(7), Nesting PID namespaces — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "Calls to getppid(2) for such processes return 0."
[^pid-self]: pid_namespaces(7), /proc and PID namespaces — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "yields the process ID of the caller in the PID namespace of the procfs mount"
[^mnt-copy]: mount_namespaces(7), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/mount_namespaces.7.en.html> — "If the namespace is created using unshare(2), the mount list of the new namespace is a copy of the mount list in the caller's previous mount namespace."
[^mnt-nopropagate]: mount_namespaces(7), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/mount_namespaces.7.en.html> — "will not (by default) affect the mount list seen in the other namespace"
[^mnt-private]: mount_namespaces(7), SHARED SUBTREES — <https://manpages.debian.org/bookworm/manpages/mount_namespaces.7.en.html> — "This mount is private; it does not have a peer group."
[^mnt-mountinfo]: mount_namespaces(7), SHARED SUBTREES — <https://manpages.debian.org/bookworm/manpages/mount_namespaces.7.en.html> — "If none of the above tags is present, then this is a private mount."
[^mnt-userns-fs]: user_namespaces(7), Effect of capabilities within a user namespace — <https://manpages.debian.org/bookworm/manpages/user_namespaces.7.en.html> — "allows that process to create bind mounts and mount the following types of filesystems:"
[^unshare1-fork]: unshare(1), OPTIONS — <https://manpages.debian.org/bookworm/util-linux/unshare.1.en.html> — "Fork the specified program as a child process of unshare rather than running it directly. This is useful when creating a new PID namespace."
[^pid-enomem]: pid_namespaces(7), The namespace init process — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "if the first child subsequently created by a fork(2) terminates, then subsequent calls to fork(2) fail with ENOMEM."
[^pid-once]: pid_namespaces(7), setns(2) and unshare(2) semantics — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "A process may call unshare(2) with the CLONE_NEWPID flag only once."
[^mnt-shared]: mount_namespaces(7), SHARED SUBTREES — <https://manpages.debian.org/bookworm/manpages/mount_namespaces.7.en.html> — "This mount shares events with members of a peer group."
[^cg7-unified]: cgroups(7), Cgroups v2 unified hierarchy — <https://manpages.debian.org/bookworm/manpages/cgroups.7.en.html> — "the flexibility turned out to be less useful than expected, and in many cases added complexity. Therefore, in cgroups v2, all available controllers are mounted against a single hierarchy."
[^cgns-root]: cgroup_namespaces(7), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages/cgroup_namespaces.7.en.html> — "its current cgroups directories become the cgroup root directories of the new namespace."
[^cg-cpumax-default]: Linux kernel v6.8, Control Group v2, cpu.max — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "A read-write two value file which exists on non-root cgroups. The default is "max 100000"."
[^cg-memmax-default]: Linux kernel v6.8, Control Group v2, memory.max — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "A read-write single value file which exists on non-root cgroups. The default is "max"."
[^cg-cpu-max]: Linux kernel v6.8, Control Group v2, cpu.max — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "which indicates that the group may consume up to $MAX in each $PERIOD duration."
[^cg-bwc]: Linux kernel, CFS Bandwidth Control — <https://docs.kernel.org/scheduler/sched-bwc.html> — "Once all quota has been assigned any additional requests for quota will result in those threads being throttled. Throttled threads will not be able to run again until the next period when the quota is replenished."
[^cg-max]: Linux kernel v6.8, Control Group v2, memory.max — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "If a cgroup's memory usage reaches this limit and can't be reduced, the OOM killer is invoked in the cgroup."
[^rs-ap]: Rust 1.95.0 std, thread::available_parallelism — <https://doc.rust-lang.org/1.95.0/std/thread/fn.available_parallelism.html> — "Returns an estimate of the default amount of parallelism a program should use."
[^rs-over]: Rust 1.95.0 std, thread::available_parallelism — <https://doc.rust-lang.org/1.95.0/std/thread/fn.available_parallelism.html> — "It may overcount the amount of parallelism available when limited by a process-wide affinity mask or cgroup quotas"
[^rs-quota-doc]: Rust 1.95.0 source, library/std/src/sys/thread/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/thread/unix.rs> — "Returns cgroup CPU quota in core-equivalents, rounded down or usize::MAX if the quota cannot be determined or is not set."
[^rs-min]: Rust 1.95.0 source, library/std/src/sys/thread/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/thread/unix.rs> — "quota = quota.min(limit / period);"
[^rs-affinity]: Rust 1.95.0 source, library/std/src/sys/thread/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/thread/unix.rs> — "let count = count.min(quota);"
[^rs-proc-cgroup]: Rust 1.95.0 source, library/std/src/sys/thread/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/thread/unix.rs> — "2nd field is a list of controllers for v1 or empty for v2"
[^rs-mount]: Rust 1.95.0 source, library/std/src/sys/thread/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/thread/unix.rs> — "standard mount location defined in file-hierarchy(7) manpage"
[^proc-memtotal]: proc_meminfo(5), MemTotal — <https://manpages.debian.org/bookworm/manpages/proc_meminfo.5.en.html> — "Total usable RAM (i.e., physical RAM minus a few reserved bits and the kernel binary code)."
[^cg-cpu-stat]: Linux kernel v6.8, Control Group v2, cpu.stat — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "and the following five when the controller is enabled:"
[^cg-stat-file]: Linux kernel v6.8, Control Group v2, memory.stat — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Amount of memory used to cache filesystem data, including tmpfs and shared memory."
[^cg-cur]: Linux kernel v6.8, Control Group v2, memory.current — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "The total amount of memory currently being used by the cgroup and its descendants."
[^runc-prep-slave]: runc v1.2.5, libcontainer/rootfs_linux.go, prepareRoot — <https://raw.githubusercontent.com/opencontainers/runc/v1.2.5/libcontainer/rootfs_linux.go> — "flag := unix.MS_SLAVE | unix.MS_REC"
[^runc-prep-bind]: runc v1.2.5, libcontainer/rootfs_linux.go, prepareRoot — <https://raw.githubusercontent.com/opencontainers/runc/v1.2.5/libcontainer/rootfs_linux.go> — "return mount(config.Rootfs, config.Rootfs, "bind", unix.MS_BIND|unix.MS_REC, "")"
[^runc-rslave]: runc v1.2.5, libcontainer/rootfs_linux.go, pivotRoot — <https://raw.githubusercontent.com/opencontainers/runc/v1.2.5/libcontainer/rootfs_linux.go> — "Make oldroot rslave to make sure our unmounts don't propagate to the"
[^runc-detach]: runc v1.2.5, libcontainer/rootfs_linux.go, pivotRoot — <https://raw.githubusercontent.com/opencontainers/runc/v1.2.5/libcontainer/rootfs_linux.go> — "Perform the unmount. MNT_DETACH allows us to unmount /proc/self/cwd."
[^pid-initdie]: pid_namespaces(7), The namespace init process — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "the kernel terminates all of the processes in the namespace via a SIGKILL signal."
[^pr-desc]: pivot_root(2), Debian bookworm manpages-dev 6.03-2, DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/pivot_root.2.en.html> — "pivot_root() changes the root mount in the mount namespace of the calling process. More precisely, it moves the root mount to the directory put_old and makes new_root the new root mount."
[^pr-unmount]: pivot_root(2), NOTES — <https://manpages.debian.org/bookworm/manpages-dev/pivot_root.2.en.html> — "pivot_root() allows the caller to switch to a new root filesystem while at the same time placing the old root mount at a location under new_root from where it can subsequently be unmounted."
[^proc-mountinfo-outside]: proc(5), /proc/pid/mountinfo, Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "because mounts that lie outside the process's root directory are not shown in mountinfo"
[^chroot-inherit]: chroot(2), Debian bookworm manpages-dev 6.03-2, DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/chroot.2.en.html> — "chroot() changes the root directory of the calling process to that specified in path. This directory will be used for pathnames beginning with /. The root directory is inherited by all children of the calling process."
[^chroot-purpose]: chroot(2), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/chroot.2.en.html> — "This call changes an ingredient in the pathname resolution process and does nothing else. In particular, it is not intended to be used for any kind of security purpose, neither to fully sandbox a process nor to restrict filesystem system calls."
[^chroot-fd]: chroot(2), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/chroot.2.en.html> — "This call does not close open file descriptors, and such file descriptors may allow access to files outside the chroot tree."
[^chroot-cwd]: chroot(2), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/chroot.2.en.html> — "This call does not change the current working directory, so that after the call '.' can be outside the tree rooted at '/'. In particular, the superuser can escape from a "chroot jail" by doing:"
[^pr-mountpoint]: pivot_root(2), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/pivot_root.2.en.html> — "A path that is not already a mount point can be converted into one by bind mounting the path onto itself."
[^pr-ebusy]: pivot_root(2), ERRORS — <https://manpages.debian.org/bookworm/manpages-dev/pivot_root.2.en.html> — "new_root or put_old is on the current root mount."
[^pr-shared]: pivot_root(2), DESCRIPTION — <https://manpages.debian.org/bookworm/manpages-dev/pivot_root.2.en.html> — "The propagation type of the parent mount of new_root and the parent mount of the current root directory must not be MS_SHARED"
[^pr-einval]: pivot_root(2), ERRORS — <https://manpages.debian.org/bookworm/manpages-dev/pivot_root.2.en.html> — "Either the mount point at new_root, or the parent mount of that mount point, has propagation type MS_SHARED."
[^pr-same]: pivot_root(2), NOTES (pivot_root(".", ".")) — <https://manpages.debian.org/bookworm/manpages-dev/pivot_root.2.en.html> — "new_root and put_old may be the same directory. In particular, the following sequence allows a pivot-root operation without needing to create and remove a temporary directory:"
[^pr-stack]: pivot_root(2), NOTES — <https://manpages.debian.org/bookworm/manpages-dev/pivot_root.2.en.html> — "This sequence succeeds because the pivot_root() call stacks the old root mount point on top of the new root mount point at /."
[^runc-dotdot]: runc v1.2.5, libcontainer/rootfs_linux.go, pivotRoot — <https://raw.githubusercontent.com/opencontainers/runc/v1.2.5/libcontainer/rootfs_linux.go> — "What this results in is / being the new root but /proc/self/cwd being the old root."
[^pid-procmount]: pid_namespaces(7), /proc and PID namespaces — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "then it isn't necessary to change the root directory: a new procfs instance can be mounted directly over /proc."
[^runc-newns]: runc v1.2.5, libcontainer/rootfs_linux.go, prepareRootfs — <https://raw.githubusercontent.com/opencontainers/runc/v1.2.5/libcontainer/rootfs_linux.go> — "else if config.Namespaces.Contains(configs.NEWNS) {"
[^pid-sig]: pid_namespaces(7), The namespace init process — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "This restriction applies even to privileged processes, and prevents other members of the PID namespace from accidentally killing the"
[^pid-ancestor]: pid_namespaces(7), The namespace init process — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "SIGKILL or SIGSTOP are treated exceptionally: these signals are forcibly delivered when sent from an ancestor PID namespace."
[^pid-anc-handler]: pid_namespaces(7), The namespace init process — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "send signals to the "init" process of a child PID namespace only if the "init" process has established a handler for that signal."
[^sigsafe-rule]: signal-safety(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal-safety.7.en.html> — "Ensure that (1) the signal handler calls only async-signal-safe functions, and (2) the signal handler itself is reentrant with respect to global variables in the main program."
[^docker-stop]: Docker Docs, docker container stop — <https://docs.docker.com/reference/cli/docker/container/stop/> — "The main process inside the container will receive SIGTERM, and after a grace period, SIGKILL."
[^docker-timeout]: Docker docs, docker container stop, --timeout — <https://docs.docker.com/reference/cli/docker/container/stop/> — "the Daemon determines the default, and is 10 seconds for Linux containers"
[^sig-kill]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "The signals SIGKILL and SIGSTOP cannot be caught, blocked, or ignored."
[^proc-sigmask]: proc(5), /proc/pid/status, Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "Masks (expressed in hexadecimal) indicating signals being blocked, ignored, and caught (see signal(7))."
[^signal-nums]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "SIGPIPE 13 13 13 13 SIGALRM 14 14 14 14 SIGTERM 15 15 15 15"
[^rs-sigpipe]: rust-lang/rust tag 1.95.0, library/std/src/rt.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/rt.rs> — "Since 2014, the Rust runtime on Unix has set the `SIGPIPE` handler to"
[^docker-init]: Docker Docs, docker container run — <https://docs.docker.com/reference/cli/docker/container/run/> — "Run an init inside the container that forwards signals and reaps processes"
[^docker-init-tini]: Docker docs, docker container run, --init — <https://docs.docker.com/reference/cli/docker/container/run/> — "This docker-init binary, included in the default installation, is backed by tini."
[^proc-nspid]: proc(5), /proc/pid/status, Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "Thread ID in each of the PID namespaces of which pid is a member."
[^sigterm-term]: signal(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal.7.en.html> — "SIGTERM P1990 Term Termination signal"
[^wait-zombie]: wait(2), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages-dev/wait.2.en.html> — "A child that terminates, but has not been waited for becomes a \"zombie\". The kernel maintains a minimal set of information about the zombie process (PID, termination status, resource usage information) in order to allow the parent to later perform a wait to obtain information about the child."
[^proc-stat-state]: proc(5), /proc/pid/stat, Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/proc.5.en.html> — "One of the following characters, indicating process state: R Running S Sleeping in an interruptible wait D Waiting in uninterruptible disk sleep Z Zombie"
[^sigsafe-stdio]: signal-safety(7), Linux man-pages (Debian bookworm) — <https://manpages.debian.org/bookworm/manpages/signal-safety.7.en.html> — "The kinds of issues that render a function unsafe can be quickly understood when one considers the implementation of the stdio library, all of whose functions are not async-signal-safe."
[^pid-subreaper]: pid_namespaces(7), Adoption of orphaned children — <https://manpages.debian.org/bookworm/manpages/pid_namespaces.7.en.html> — "unless one of the nearer ancestors of the parent employed the prctl(2) PR_SET_CHILD_SUBREAPER command to mark itself as the reaper of orphaned descendant processes"
