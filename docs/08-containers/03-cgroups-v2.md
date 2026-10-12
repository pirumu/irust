# P8 · Chương 03 — cgroups v2

> **Môi trường thí nghiệm.** Số liệu "quan sát" do tôi chạy thật: Docker 29.5.2 trên macOS (engine chạy trong VM Linux của Colima, như các chương khác của phase), VM Linux kernel `6.8.0-117-generic` (aarch64), 4 vCPU, `MemTotal` khoảng 5,8 GiB, cgroup v2, driver `cgroupfs`. Container lab `./labs/linux.sh` chạy root nhưng cgroup của nó **chỉ đọc** (mục 2). Để thử ghi file cgroup tôi dùng container dùng một lần `docker run --rm --privileged` với cùng image; để xem Docker cấu hình gì tôi chạy `docker run --memory/--cpus` rồi đọc file bên trong; mọi container đã xóa. Quan sát là một lần chạy trên một kernel, không phải bảo đảm.

## Bạn sẽ hiểu được gì

1. **Q1.** cgroup là gì, khác namespace ở đâu, và vì sao Docker/Kubernetes/systemd đều dựa vào nó? → mục 1
2. **Q2.** cgroup v1 sai ở đâu để phải có v2; "unified hierarchy" nghĩa là gì; `/sys/fs/cgroup` thực chất là cái gì; các file lõi `cgroup.*` làm gì? → mục 2
3. **Q3.** Quy tắc "no internal process" và "delegation" là gì, và vì sao có lúc `echo +memory > cgroup.subtree_control` báo `Device or resource busy`? → mục 3
4. **Q4.** Controller điều khiển theo kiểu *trọng số*, *giới hạn* hay *bảo vệ*; controller memory đưa ra file nào, đọc `memory.events` ra sao để biết giới hạn đã cắn chưa? → mục 4
5. **Q5.** Controller cpu, io, pids: `cpu.weight` khác `cpu.max` thế nào, `cpu.stat` cho biết gì, io và pids giới hạn thế nào? → mục 5
6. **Q6.** PSI (pressure stall information) đo cái gì mà `memory.current` không đo được? → mục 6
7. **Q7.** `docker run --memory 256m --cpus 1.5`, `resources.limits/requests` của Kubernetes và directive systemd biến thành file cgroup nào? → mục 7
8. **Q8.** Vì sao `nproc` và `/proc/meminfo` trong container vẫn hiện số của máy host, và `std::thread::available_parallelism` của Rust làm gì để tránh bẫy đó? → mục 8 và phần Liên hệ với Rust

## Kiến thức cần trước

- [Page fault, swapping và OOM](../03-memory/04-page-fault-va-swapping.md): chương này không lặp lại cơ chế reclaim/OOM, chỉ đào sâu cgroup như một hệ thống. Phần memory cgroup nhìn từ góc RSS (`RssAnon` so với `memory.current`) nằm ở [P3 · Chương 05, mục 8](../03-memory/05-bo-nho-tien-trinh-linux.md); danh sách file `memory.*` chuẩn của cả giáo trình là mục 4 của chương này.
- [Scheduling](../04-concurrency/02-scheduling.md): `cpu.weight`/`cpu.max` đã được giới thiệu ở đó.
- [Container là gì](01-container-la-gi.md) và [Namespaces](02-namespaces.md): container = process + namespace (cái nhìn thấy) + cgroup (cái được dùng).

## 1. cgroup giải quyết vấn đề gì (Q1)

> **Trả lời ngắn.** Namespace quyết định process *nhìn thấy* gì; cgroup quyết định process *dùng được* bao nhiêu. Kernel doc định nghĩa cgroup là cơ chế tổ chức process thành cây và phân phối tài nguyên hệ thống dọc theo cây[^cg-what]. Vì mọi process con tự chịu giới hạn của cgroup chứa nó, runtime container chỉ cần một cgroup cho mỗi container (suy luận của người soạn).

**Giải thích.**

Hình dung (nhận xét của người soạn): namespace là bức vách kính mờ giữa các văn phòng — mỗi công ty chỉ thấy phòng mình. Nhưng không ai ngăn một công ty bật hết điều hòa và ăn hết điện của tòa nhà. cgroup là đồng hồ điện và cầu dao riêng cho từng công ty.

Một process trong PID namespace riêng vẫn có thể ăn hết RAM và CPU của máy nếu không có cgroup (suy luận của người soạn); cgroup là nửa còn lại của container. Hai phần: *core* lo tổ chức process thành cây, còn *controller* phân phối từng loại tài nguyên dọc theo cây (ví dụ CPU, memory, IO, số process ở các mục sau)[^cg-parts]. Ba quy tắc nền:

1. Cgroup tạo thành cây và mỗi process thuộc một và chỉ một cgroup[^cg-one]; mặc định mọi thread của một process thuộc cùng cgroup (có thread mode cho một số controller, chương này không đi vào)[^cg-threads].
2. Process mới sinh ra nằm trong cgroup của cha tại thời điểm fork[^cg-fork].
3. Hierarchical: giới hạn đặt gần gốc không bị ghi đè từ xa hơn[^cg-hier]. Con không thể vượt trần của cha, dù tự cấu hình thế nào.

Vì vậy runtime container chỉ cần tạo một cgroup cho mỗi container, ghi giới hạn, rồi đưa process vào; mọi process con tự động chịu cùng giới hạn (suy luận của người soạn từ ba quy tắc).

<details><summary>Đi sâu: nguyên văn định nghĩa và hai phần core/controller</summary>

- Câu định nghĩa trong kernel doc v6.8 mở đầu mục "What is cgroup?": cgroup tổ chức process theo thứ bậc và phân phối tài nguyên "in a controlled and configurable manner".[^cg-what]
- Core chịu trách nhiệm chính về việc tổ chức process theo thứ bậc.[^cg-parts]

</details>

**Quay lại Q1:** namespace lo cái *nhìn thấy*, cgroup lo cái *được dùng*; một cgroup cho mỗi container là đủ vì giới hạn đi theo cây.

## 2. cgroup v2: cây duy nhất, `cgroupfs` và các file lõi (Q2)

> **Trả lời ngắn.** v1 cho phép gắn các controller vào những cây riêng; v2 gom mọi controller vào *một* cây duy nhất[^cg7-unified]. Cgroup được tạo bằng cách tạo thư mục trong filesystem `cgroup2` (mount bằng `mount -t cgroup2`; trên máy quan sát là `/sys/fs/cgroup`)[^cg-mkdir][^cg-mount]. File `cgroup.procs` dùng để di chuyển process[^cg-procs], `cgroup.controllers` liệt kê controller có thể bật[^cg-ctrl-file] và `cgroup.subtree_control` bật/tắt chúng cho các con[^cg-subtree-file].

**Giải thích.**

Hình dung (nhận xét của người soạn): v1 giống mỗi phòng ban có một sơ đồ tổ chức riêng (sơ đồ lương, sơ đồ điện, sơ đồ bàn ghế) và chúng không khớp nhau; v2 là một sơ đồ duy nhất, ai cũng nằm ở đúng một ô.

**v1 và v2.** Theo `cgroups(7)`, cgroup v1 ra từ Linux 2.6.24[^cg7-224]; các controller của v1 được phát triển gần như không phối hợp với nhau, dẫn tới nhiều bất nhất giữa controller và việc quản lý hierarchy trở nên phức tạp[^cg7-why]. V2 bắt đầu được làm từ Linux 3.10[^cg7-310] và chính thức từ Linux 4.5[^cg7-45]. Ở v1 mỗi controller có thể gắn vào một hierarchy riêng (cây `cpu` khác cây `memory`); `cgroups(7)` nói tính linh hoạt này kém hữu ích hơn dự kiến và nhiều khi chỉ thêm phức tạp, nên ở v2 mọi controller gắn vào một hierarchy duy nhất[^cg7-unified]. (Nhận xét của người soạn: nhờ vậy một container ở v2 có thể chỉ cần một đường dẫn cgroup cho mọi controller.)

Ở v2 còn có hai thay đổi nền. Thứ nhất, nói đại khái, process chỉ được nằm ở nút lá (trừ root)[^cg7-nip]; quy tắc chính xác hơn ở mục 3[^cg7-nip-precise]. Thứ hai, controller chỉ bật được khi nằm trong `cgroup.controllers` của cgroup đó, qua việc ghi `cgroup.subtree_control`[^cg-ctrl-only]. Một controller không thể dùng đồng thời ở v1 và v2[^cg7-both].

**`cgroupfs`: cây thư mục là API.** Doc kernel mô tả cgroup con được tạo bằng cách tạo thư mục con; tôi không thấy doc nhắc syscall riêng để tạo cgroup (suy luận của người soạn). Tạo thư mục = tạo cgroup; xóa thư mục = xóa cgroup (khi không còn con và process sống[^cg-rmdir]); đọc/ghi file = đọc/cấu hình. Ban đầu chỉ có cgroup gốc chứa mọi process; cgroup con được tạo bằng cách tạo thư mục con[^cg-mkdir]. Quan sát trong container lab: `stat -f -c %T /sys/fs/cgroup` in `cgroup2fs`; `mount` cho `cgroup on /sys/fs/cgroup type cgroup2 (ro,nosuid,nodev,noexec,relatime,nsdelegate,memory_recursiveprot)`. Tức là `/sys/fs/cgroup` là một mount kiểu `cgroup2` chứ không phải thư mục thường.

**Các file lõi (`cgroup.*`).**

- `cgroup.procs`: đọc ra PID của các process thuộc cgroup, mỗi dòng một PID[^cg-procs-read]. Ghi một PID vào để di chuyển process: mỗi lần `write(2)` chỉ di chuyển một process, và ghi PID của một thread bất kỳ thì cả process (mọi thread) được di chuyển[^cg-procs].
- `cgroup.controllers`: các controller mà cgroup *có thể bật cho con*[^cg-ctrl-file]; `cgroup.subtree_control`: những controller *đang* bật cho con[^cg-subtree-file]. Mặc định không controller nào được bật[^cg-nodef].

Nếu `A` có `+cpu +memory` trong `subtree_control` thì các file `cpu.*`, `memory.*` xuất hiện **trong thư mục con** của `A`: bật controller ở một cgroup nghĩa là việc phân phối tài nguyên cho các con *trực tiếp* của nó sẽ được điều khiển[^cg-enable], và file giao diện của controller (không bắt đầu bằng `cgroup.`) do cgroup *cha* sở hữu chứ không phải chính cgroup đó[^cg-owned]. Ràng buộc top-down: một cgroup chỉ phân phối tiếp tài nguyên nếu đã được cha phân phối cho, và không thể tắt controller khi một cgroup con còn bật nó[^cg-topdown].

**Cgroup trong container nhìn ra sao.** Trong container, `cat /proc/self/cgroup` in `0::/` — "tôi ở gốc". Khác biệt là **cgroup namespace**: nó ảo hóa cách nhìn cgroup của process[^cgns], mỗi namespace có thư mục gốc riêng và đường dẫn trong `/proc/pid/cgroup` được tính tương đối từ đó[^cgns-root]. Chi tiết ở [chương Namespaces](02-namespaces.md).

<details><summary>Đi sâu: tình trạng v1/Kubernetes, <code>nsdelegate</code>, quan sát cgroup chỉ đọc</summary>

- `cgroups(7)` nói v1 vẫn còn tồn tại (vì tương thích nên khó bị gỡ) và v2 lúc đó chỉ cài một tập con controller của v1[^cg7-subset]. Kubernetes thì đã tuyên bố ngừng ủng hộ (deprecated) cgroup v1[^k8s-v1]. (docs Kubernetes không ghim phiên bản, đọc ở thời điểm soạn.)
- Quan sát (container privileged): `lab/child/cgroup.controllers` trống cho tới khi ghi `+memory +cpu +pids` vào `lab/cgroup.subtree_control`; sau đó nó liệt kê `cpu memory pids` và có `memory.max`, `cpu.max`, `pids.max`.
- Với Docker chạy `--cgroupns=host` (chỉ để quan sát), `cat /proc/self/cgroup` in `0::/docker/<64 ký tự id>`.
- Mount có `nsdelegate`: theo kernel doc, đây là tùy chọn coi cgroup namespace là ranh giới của việc giao quyền (delegation)[^cg-nsdeleg]. Quan sát: container lab không đặc quyền thử `mkdir /sys/fs/cgroup/x` và `echo 1 > /sys/fs/cgroup/cpu.weight` đều trả `Read-only file system`; `mount -o remount,rw /sys/fs/cgroup` cũng bị từ chối (`cannot remount cgroup read-write, is write-protected`). Vậy ở lab này container (kể cả root) không tự nới giới hạn của mình.

</details>

**Quay lại Q2:** v2 là một cây duy nhất điều khiển qua filesystem `cgroup2`; `cgroup.procs` di chuyển process, `cgroup.subtree_control` bật controller cho con.

## 3. "No internal process" và delegation (Q3)

> **Trả lời ngắn.** cgroup không phải root chỉ được chia tài nguyên cho con khi bản thân nó không có process nào[^cg-nip]. Vì vậy muốn bật controller ở một cgroup đang chứa process thì phải tạo cgroup con, chuyển hết process sang đó rồi mới bật controller[^cg-nip-how]; nếu làm sai thứ tự, thí nghiệm bên dưới cho `EBUSY` (quan sát, doc không nêu mã lỗi). Delegation là giao một nhánh cây cho kẻ kém quyền hơn, vẫn bị trần của cha khống chế[^cg-deleg-contain].

**Giải thích.**

Hình dung (nhận xét của người soạn): một trưởng phòng muốn chia ngân sách cho các tổ thì bản thân ông không được vừa là thành viên đòi ngân sách cạnh tranh với các tổ; ông phải đưa mình vào một tổ riêng rồi mới đi chia.

**No internal process.** Quy tắc: cgroup không phải root chỉ phân phối domain resource cho con khi không có process của chính nó[^cg-nip]. Mục đích: khi controller nhìn một đoạn cây, process luôn ở lá, không có cảnh con cạnh tranh với process của chính cha[^cg-nip-why]. Gốc (root) được miễn[^cg-root-exempt]. Quy tắc hẹp hơn câu "process chỉ ở lá": một cgroup không-root vẫn có thể vừa có process vừa có cgroup con, chỉ là phải dời process ra trước khi bật controller cho con[^cg7-nip-precise]. Cách làm đúng, theo doc: cgroup phải tạo con và chuyển mọi process sang con *trước khi* bật controller trong `cgroup.subtree_control` của nó[^cg-nip-how].

Quan sát (container privileged, root của namespace chứa 3 process lúc đầu):

```text
echo "+memory" > /sys/fs/cgroup/cgroup.subtree_control   # rc=1, Device or resource busy
mkdir init; (chuyển hết PID trong cgroup.procs sang init/)
echo "+memory +cpu +pids +io" > cgroup.subtree_control   # rc=0
mkdir lab; echo $$ > lab2/cgroup.procs; echo +memory > lab2/cgroup.subtree_control  # rc=1, busy
echo $$ > lab/cgroup.procs   # khi lab đã có subtree_control: busy
```

Cả hai chiều đều hiện ra: bật controller khi cgroup đang có process thì `EBUSY`; di chuyển process vào cgroup đã bật controller cho con cũng `EBUSY`. Mẫu "tạo cgroup `init`, dời mọi process vào đó, rồi mới bật controller" là cách vượt qua (kết quả thí nghiệm trên).

**Delegation.** Một cgroup được giao theo hai cách: cấp quyền ghi vào thư mục cùng `cgroup.procs`, `cgroup.threads`, `cgroup.subtree_control` cho user ít đặc quyền, hoặc (mount có `nsdelegate`) tự động giao cho cgroup namespace lúc tạo namespace[^cg-deleg]. Phần còn lại tự do, nhưng không thứ gì trong nhánh được giao thoát khỏi các giới hạn tài nguyên mà cgroup cha áp lên[^cg-deleg-contain]. Người được giao cũng không di chuyển được process vào hay ra khỏi nhánh[^cg-deleg-contain2].

<details><summary>Đi sâu: delegation qua <code>nsdelegate</code> và systemd</summary>

- Với cách giao thứ hai, kernel từ chối ghi mọi file ngoài `cgroup.procs` và `cgroup.subtree_control` ở gốc namespace từ bên trong[^cg-deleg-files], vì các file điều khiển đó quyết định phần chia *của cha*.
- Systemd có khái niệm tương tự: người được giao (delegatee) có thể bật controller cho các con của mình khi cần.[^sd-deleg]

</details>

**Quay lại Q3:** `EBUSY` đến từ quy tắc "no internal process"; tạo con, dời process, rồi mới bật controller, và nhánh được giao vẫn nằm dưới trần của cha.

## 4. Ba kiểu điều khiển và controller memory (Q4)

> **Trả lời ngắn.** Controller điều khiển tài nguyên theo ba kiểu: *weights* (chia theo tỉ lệ trọng số), *limits* (trần tuyệt đối) và *protections* (bảo vệ tối thiểu)[^cg-weights][^cg-limits][^cg-prot]. Controller memory tính cả page cache vào cgroup[^cg-stat-file]; vượt `memory.high` chỉ bị throttle và không bao giờ gọi OOM killer[^cg-high], còn `memory.max` khi mức dùng chạm trần mà không giảm được thì OOM killer chạy trong cgroup[^cg-max]. `memory.events` đếm các sự kiện như `high` (số lần bị throttle) và `oom_kill` (số process bị OOM killer giết)[^cg-ev-high][^cg-ev-kill].

**Giải thích.**

Hình dung (nhận xét của người soạn): trọng số giống chia phần pizza theo số người đang đói; limit giống khóa tủ lạnh của từng nhà; protection giống để dành phần ăn tối thiểu cho ai đó dù nhà khác có đói.

| Kiểu | Ý nghĩa | Ví dụ | Đặc điểm |
|---|---|---|---|
| Weights | chia theo tỉ lệ trọng số của các con *đang hoạt động*; "work-conserving"[^cg-weights] | `cpu.weight`, `io.weight` | khoảng [1, 10000], mặc định 100[^cg-w-def] |
| Limits | trần tuyệt đối; có thể over-commit tổng[^cg-limits] | `cpu.max`, `memory.max`, `io.max`, `pids.max` | mặc định `max` (không giới hạn)[^cg-lim-def] |
| Protections | bảo vệ tối thiểu; có thể over-commit | `memory.min`, `memory.low`[^cg-prot] | mặc định 0[^cg-prot-def] |

Weight chỉ có ý nghĩa khi *tranh chấp*; limit có hiệu lực kể cả khi máy rảnh. Vì thế container với `--cpu-shares` thấp chạy một mình vẫn dùng hết CPU, còn `--cpus` thì không (suy luận của người soạn từ hai định nghĩa trên). Reclaim và OOM xem [chương page fault](../03-memory/04-page-fault-va-swapping.md).

**Cái gì bị tính.** Kernel doc liệt kê loại bộ nhớ được theo dõi: bộ nhớ userland (page cache và anonymous memory), cấu trúc dữ liệu kernel như dentry/inode, và TCP socket buffers[^cg-tracked]. Điều đáng nhấn mạnh: **page cache được tính vào cgroup**. `memory.stat` định nghĩa `file` là bộ nhớ dùng để cache dữ liệu filesystem, gồm cả tmpfs và shared memory[^cg-stat-file]. Một process chỉ ghi file lớn cũng làm `memory.current` tăng (suy luận của người soạn từ định nghĩa `file`; thí nghiệm ở phần Đi sâu). Ai bị tính? Vùng nhớ bị tính cho cgroup đã khởi tạo nó và ở đó cho tới khi được giải phóng[^cg-charge], và chuyển process sang cgroup khác không chuyển phần đã tính[^cg-charge-move].

**Các file.**

- `memory.current`: mức dùng hiện tại, gồm cả cgroup con[^cg-cur]; `memory.peak`: mức cao nhất từ khi tạo[^cg-peak].
- `memory.min`: bảo vệ cứng, bộ nhớ của cgroup nằm trong ngưỡng min *hiệu lực* sẽ không bị reclaim trong bất kỳ điều kiện nào[^cg-min]; đặt bảo vệ vượt quá lượng bộ nhớ sẵn có bị khuyến cáo không nên và có thể dẫn tới OOM liên tục[^cg-min-over]. `memory.low`: bảo vệ mềm, chỉ bị reclaim khi không còn bộ nhớ reclaim được ở các cgroup không được bảo vệ[^cg-low].
- `memory.high`: ngưỡng throttle, không bao giờ gọi OOM killer[^cg-high]. `memory.max`: trần cứng, không giảm được thì OOM killer chạy trong cgroup[^cg-max].
- `memory.stat`: phân rã `anon`, `file`, `kernel`…; phải tra theo *key* vì có thể có mục mới chen vào giữa[^cg-stat-keys].
- `memory.events`: `high` đếm số lần process bị throttle và buộc direct reclaim[^cg-ev-high]; `oom_kill` đếm số process của cgroup bị mọi loại OOM killer giết[^cg-ev-kill]. Mọi trường trong file này mang tính phân cấp (tính cả cgroup con)[^cg-ev-hier]; `memory.events.local` là bản chỉ tính riêng cgroup[^cg-ev-local].

<details><summary>Đi sâu: thí nghiệm ghi file lớn, OOM, <code>memory.oom.group</code>, <code>memory.high</code></summary>

**Ghi file lớn trong cgroup 50 MiB.** Đặt `memory.max=50M`, chạy `dd if=/dev/zero of=/tmp/big bs=1M count=300` trong cgroup. `dd` thành công (300 MiB ghi, khoảng 0,32 s), không có OOM kill (`oom 0, oom_kill 0`), nhưng `memory.current = 51146752` (khoảng 48,8 MiB, sát trần), `memory.stat` có `file 48496640` và `file_dirty 6877184`, còn `memory.events` có `max 1015`. Page cache chiếm gần hết ngân sách; trường `max` đếm số lần mức dùng sắp vượt ngưỡng max và nếu direct reclaim không kéo xuống được thì cgroup mới sang trạng thái OOM[^cg-ev-max]. Ở đây page cache reclaim được nên không OOM (suy luận của người soạn từ doc và quan sát `oom 0`).

**OOM và `memory.oom.group`.** Chương trình Rust nhỏ `hog mem N` cấp phát và ghi N MiB; đặt `memory.max=64M`, `memory.swap.max=0` (không swap để reclaim không cứu được anonymous memory).

- Cgroup A, `hog mem 200`: process bị giết (exit 137 = 128 + SIGKILL). `memory.events`: `max 38`, `oom 1`, `oom_kill 1`, `oom_group_kill 0`; `memory.peak = 67108864` = đúng 64 MiB dù chương trình đòi 200 MiB.
- Cgroup B0 (`memory.oom.group=0`) và B1 (`=1`), mỗi cái có thêm một process "ngủ" cùng hog. B0: `oom_kill 1`, `oom_group_kill 0`. B1: `oom_kill 4`, `oom_group_kill 1`. Tức là với `group=1` kernel giết toàn bộ các process trong cgroup (4 process; có lẽ gồm cả subshell cha của hog, vì script của tôi không in được dòng "sleeper dead" — suy luận của người soạn) thay vì chọn một nạn nhân. Đúng với mô tả của doc: mọi task của cgroup bị giết cùng nhau hoặc không task nào bị giết[^cg-oomgroup].

**`memory.high` và throttle.** Đặt `memory.high=32M`, `memory.swap.max=0`, chạy `hog mem 48` với `timeout 25`. Quan sát: **không xong sau 25 giây** (exit 124 từ `timeout`), `memory.events` có `high 1245` và `oom 0`; `memory.pressure` ghi `some avg10=89.13 ... total=24264246` (khoảng 24,3 giây stall tích lũy trong 25 giây). Đây đúng là hành vi doc mô tả: vượt `high` thì process bị throttle và chịu áp lực reclaim nặng, không bị giết[^cg-high]. Anonymous memory không reclaim được (không swap) nên throttle làm tiến trình gần như đứng. (Nhận xét của người soạn: `memory.high` không có ai giám sát có thể tệ hơn OOM, vì tiến trình không chết mà đứng; doc cũng dặn dùng khi có bên ngoài theo dõi[^cg-high].)

</details>

**Quay lại Q4:** weights chia tỉ lệ, limits đặt trần, protections giữ phần tối thiểu; memory tính cả page cache, `high` làm chậm, `max` mới OOM, và `memory.events` cho biết cái nào đã xảy ra.

## 5. Controller cpu, io và pids (Q5)

> **Trả lời ngắn.** `cpu.weight` chia CPU theo tỉ lệ giữa các cgroup con đang hoạt động[^cg-weights]; `cpu.max` là quota theo chu kỳ — dùng hết quota thì các thread bị chặn tới chu kỳ sau[^cg-cpu-max][^cg-bwc]. `cpu.stat` có `nr_throttled` cho biết bị chặn bao nhiêu lần[^cg-bwc-stat]. `io.max` giới hạn tốc độ/IOPS theo thiết bị[^cg-iomax] (doc v6.8 ghi hạn chế với thiết bị blk-mq, xem phần Đi sâu[^cg-io-blkmq]); `pids.max` làm `fork`/`clone` trả `-EAGAIN` khi sẽ vượt giới hạn[^cg-pids-eagain].

**Giải thích.**

Hình dung (nhận xét của người soạn): `cpu.max` giống một thẻ phút gọi điện: 20 ms mỗi 100 ms; hết thẻ là cúp máy tới kỳ sau, dù bao nhiêu người cùng dùng chung thẻ.

**`cpu.weight`.** Khoảng [1, 10000], mặc định 100[^cg-cpu-weight]. Vì là weight kiểu work-conserving nên chỉ có ý nghĩa khi các cgroup tranh chấp CPU (suy luận của người soạn từ định nghĩa weights ở mục 4).

**`cpu.max`: quota và period (CFS bandwidth).** Dạng `$MAX $PERIOD`, nghĩa là cgroup có thể dùng tới `$MAX` trong mỗi `$PERIOD`; `max` là không giới hạn[^cg-cpu-max]. Cơ chế (tài liệu CFS bandwidth control): trong mỗi period, nhóm được cấp tối đa `quota` micro giây CPU; quota được giao cho run queue từng CPU theo từng lát khi thread runnable; khi hết quota thì các thread bị throttle và không chạy lại được cho tới period sau, khi quota được nạp lại[^cg-bwc].

Hệ quả (suy luận của người soạn từ cơ chế): quota tính theo *tổng CPU-time của mọi thread*, không theo số core. Quota `150000/100000` là 150 ms CPU mỗi 100 ms; 4 thread chạy song song hết quota sau 37,5 ms thời gian thực rồi tất cả bị chặn đến hết period; request đến đúng lúc đó phải chờ, nên đây là nguồn latency spike.

`cpu.stat` cho thấy chuyện này xảy ra hay chưa: khi controller bật có `nr_periods`, `nr_throttled`, `throttled_usec`, `nr_bursts`, `burst_usec`[^cg-cpu-stat]. Tài liệu bandwidth: `nr_throttled` là số lần nhóm bị throttle/giới hạn[^cg-bwc-stat]; trang đó (mô tả theo tên file v1, `throttled_time` tính bằng nanosecond) nói thời gian throttle được tính trên các thực thể (entities) lập lịch của nhóm[^cg-bwc-time].

**io.** `io.max` giới hạn tốc độ theo thiết bị, khóa bằng `$MAJ:$MIN`, với `rbps`, `wbps`, `riops`, `wiops`; BPS và IOPS đo theo từng chiều IO và IO bị trì hoãn khi chạm giới hạn[^cg-iomax]. `io.weight` là trọng số tương đối giữa anh em[^cg-ioweight]. Kernel doc nêu chuyện ghi buffered: page cache bị làm bẩn qua buffered write và mmap chung, nên cần cgroup-aware writeback — memory và io controller phối hợp[^cg-writeback].

**pids.** `pids.max` là giới hạn cứng số process[^cg-pidsmax]. Không thể vi phạm chính sách PID của cgroup qua `fork()` hay `clone()`; chúng trả `-EAGAIN`[^cg-pids-eagain].

<details><summary>Đi sâu: các thí nghiệm weight, quota, io, pids và lưu ý blk-mq</summary>

**`cpu.weight`.** Hai cgroup `w1` (weight 100) và `w2` (weight 400), mỗi cái chạy chương trình 4 thread quay vòng 3 giây trên VM 4 vCPU, tranh chấp hoàn toàn. `usage_usec` đọc được: `w1` 2.694.028, `w2` 9.191.331 — tỉ lệ khoảng 1 : 3,4, quanh mức 1 : 4 mong đợi từ 100 : 400.

**`cpu.max`.** `cpu.max = "20000 100000"` (20% của một CPU), chương trình 4 thread quay vòng ~3 giây:

```text
usage_usec 623732
nr_periods 31
nr_throttled 31
throttled_usec 11226380
```

Cả 31 period đều bị throttle; CPU dùng 0,62 s ≈ 31 × 20 ms, đúng quota. `throttled_usec` ≈ 11,2 s lớn hơn thời gian thực vì cộng dồn theo các thread/CPU bị chặn (suy luận của người soạn, khớp với chữ "entities" trong doc).

**io.** `lsblk` cho `vda 253:0`, `vdb 253:16`. Tôi đọc 100 MiB từ `/dev/vdb` bằng `dd iflag=direct` trong cgroup: không giới hạn đạt 4,2 GB/s (0,025 s). Đặt `echo "253:16 rbps=10485760" > io.max` (10 MiB/s) rồi đọc 50 MiB: 5,0 giây, 10,5 MB/s. `io.stat` ghi `rbytes=157286400 rios=150`. Cách đọc thẳng (`O_DIRECT`) tránh page cache, nên giới hạn thấy rõ. Kernel doc v6.8 nói phân phối theo weight chỉ khả dụng khi dùng cfq-iosched và cả hai cách (weight, giới hạn tuyệt đối) đều không khả dụng cho thiết bị blk-mq[^cg-io-blkmq], nhưng ở đây `io.max` hoạt động; tôi chỉ khẳng định quan sát này.

**pids.** `pids.max=5`, chương trình Rust tạo tối đa 20 thread: tạo được 4 thread rồi `spawn error: Resource temporarily unavailable (os error 11)` (EAGAIN = 11); thread chính cộng 4 thread bằng 5 task; `pids.events` ghi `max 1`. Số đếm 5 = 1 + 4 phù hợp với việc thread cũng tính vào `pids.max`: kernel doc nói rõ "PID" của controller này là TID, tức ID mà kernel dùng cho từng task[^cg-pids-tid].

</details>

**Quay lại Q5:** `cpu.weight` chỉ có nghĩa khi tranh chấp, `cpu.max` là quota theo period (xem `nr_throttled`), `io.max` đặt trần theo thiết bị, `pids.max` làm `fork` trả `EAGAIN`.

## 6. PSI: đo sự nghẽn thay vì đo mức dùng (Q6)

> **Trả lời ngắn.** `memory.current` cho biết bạn dùng bao nhiêu, nhưng không cho biết workload có đang *chờ* vì thiếu hay không. PSI đo ảnh hưởng về thời gian (stall) của việc tranh chấp CPU, memory hay IO lên workload[^psi-quant] và xuất ra file `cpu.pressure`, `memory.pressure`, `io.pressure` trong mỗi cgroup[^psi-cg].

**Giải thích.**

Hình dung (nhận xét của người soạn): `memory.current` là số khách trong quán; PSI là thời gian khách phải đứng chờ bàn. Quán vừa đầy vừa không ai chờ là ổn; quán chưa đầy mà ai cũng chờ là có vấn đề.

Khi CPU, memory hoặc IO bị tranh chấp, workload gặp latency spike, mất throughput, và có nguy cơ bị OOM kill[^psi-why]. Kernel xuất thông tin từng tài nguyên qua `/proc/pressure/{cpu,memory,io}`[^psi-proc]; với cgroup v2 thì mỗi thư mục con trong cgroupfs có `cpu.pressure`, `memory.pressure`, `io.pressure`[^psi-cg]. Định dạng:

```text
some avg10=0.00 avg60=0.00 avg300=0.00 total=0
full avg10=0.00 avg60=0.00 avg300=0.00 total=0
```

- `some`: tỉ lệ thời gian mà *ít nhất một số* task bị stall vì tài nguyên đó[^psi-some].
- `full`: tỉ lệ thời gian mà *mọi* task không idle cùng stall; trạng thái này nghĩa là chu kỳ CPU thực sự đang bị lãng phí và workload ở đó lâu là "thrashing"[^psi-full].
- `avg10/60/300` là phần trăm trung bình theo 10/60/300 giây; `total` là micro giây stall tích lũy, để bắt spike mà trung bình làm mờ[^psi-avg].

<details><summary>Đi sâu: PSI trong thí nghiệm <code>memory.high</code>, trigger/poll</summary>

- Trong thí nghiệm `memory.high` (mục 4), dòng `some` của `memory.pressure` báo `avg10=89.13`: 89% thời gian 10 giây gần nhất có task chờ bộ nhớ; `memory.current` chỉ kẹt ở 32 MiB, không nói được thảm họa đó.
- PSI còn có trigger/poll: ghi một ngưỡng như "some 150000 1000000" vào file pressure[^psi-trigger] (tôi không chạy; trang PSI trên docs.kernel.org không ghim phiên bản).
- Dòng `full` của CPU ở cấp hệ thống không có định nghĩa; kernel vẫn xuất ra nhưng đặt bằng 0 để tương thích ngược[^psi-cpufull]. Doc này nói về `/proc/pressure/cpu`; với `cpu.pressure` của cgroup tôi không khẳng định thêm.

</details>

**Quay lại Q6:** PSI đo thời gian *chờ* (`some`/`full`), thứ mà số đo mức dùng như `memory.current` không cho thấy.

## 7. systemd, Docker, Kubernetes → file cgroup (Q7)

> **Trả lời ngắn.** Cả ba đều dẫn tới việc cấu hình file cgroup. Quan sát thật: Docker `--memory`→`memory.max`, `--cpus`→`cpu.max`, `--pids-limit`→`pids.max`, `--cpu-shares`→`cpu.weight`. systemd có directive tương ứng cho từng file[^sd-intro]. Kubernetes docs nói runtime cấu hình cgroup để áp giới hạn bạn khai báo[^k8s-cg]; docs không nêu tên file, nên ánh xạ sang tên file ở dưới là suy luận của người soạn.

**Giải thích.**

Hình dung (nhận xét của người soạn): các công cụ này là bảng điều khiển trên tường; mỗi nút bấm cuối cùng là ghi một con số vào một file trong `/sys/fs/cgroup`.

**systemd.** Các unit (service, slice, scope, socket, mount, swap) dùng cgroup để quản lý tài nguyên của process mà chúng sinh ra, và directive trong unit file được dịch sang file cgroup[^sd-intro]. systemd chỉ ra lệnh cho kernel bật controller cho unit khi unit có cấu hình cho controller đó[^sd-enable].

| Directive systemd | File cgroup v2 | Nguồn |
|---|---|---|
| `CPUWeight=` | `cpu.weight` | [^sd-cpuweight] |
| `CPUQuota=20%` | `cpu.max` (giá trị `>100%` để dùng nhiều CPU) | [^sd-cpuquota] |
| `MemoryHigh=` | `memory.high`; dùng làm cơ chế chính | [^sd-memhigh][^sd-memhigh-file] |
| `MemoryMax=` | `memory.max`; tuyến phòng thủ cuối | [^sd-memmax][^sd-memmax-file] |
| `TasksMax=` | `pids.max` | [^sd-tasks] |
| `IOWeight=` | `io.weight` (1–10000, mặc định 100) | [^sd-ioweight] |
| `IOReadBandwidthMax=` | `io.max` (không work-conserving) | [^sd-iomax][^sd-iomax-file] |

**Docker (quan sát thật).** Docs Docker (không ghim phiên bản) mô tả `--cpus` là lượng tài nguyên CPU khả dụng mà một container được dùng, tương đương `--cpu-period` + `--cpu-quota`[^dk-cpus]. `--memory-swap` là tổng memory+swap[^dk-swap-total]; bằng `--memory` (và `--memory` là số dương) thì container không được dùng swap[^dk-swap]; còn khi chỉ đặt `--memory` mà không đặt `--memory-swap`, container được dùng swap bằng đúng `--memory` nếu host có cấu hình swap (ví dụ `--memory=300m` → tổng 600m)[^dk-swap-default]; `--cpu-shares` mặc định 1024 và chỉ có hiệu lực khi CPU bị tranh chấp[^dk-shares]; `--memory` tối thiểu 6m[^dk-min]. Quan sát, chạy `docker run --memory 256m --memory-swap 512m --cpus 1.5 --pids-limit 100 --cpu-shares 512` rồi đọc bên trong:

| Cờ Docker | File cgroup v2 | Giá trị đọc được |
|---|---|---|
| `--memory 256m` | `memory.max` | `268435456` |
| `--memory-swap 512m` | `memory.swap.max` | `268435456` (= swap − memory = 512m − 256m) |
| `--cpus 1.5` | `cpu.max` | `150000 100000` |
| `--cpu-shares 512` | `cpu.weight` | `59` |
| `--pids-limit 100` | `pids.max` | `100` |
| (không đặt) | `memory.high`, `memory.low`, `memory.min`, `memory.oom.group` | `max`, `0`, `0`, `0` |

**Kubernetes (chỉ điều docs nói).** Docs Kubernetes (không ghim phiên bản, đọc ở thời điểm soạn): trên Linux, container runtime thường cấu hình kernel cgroup để áp và thực thi các giới hạn bạn khai báo[^k8s-cg].

- CPU limit: trần cứng về thời gian CPU; kernel kiểm tra mỗi khoảng lập lịch và nếu vượt thì chờ trước khi cho cgroup chạy tiếp[^k8s-cpulimit]. Doc không nêu tên file, nhưng đây là hành vi của `cpu.max` ở mục 5 (suy luận của người soạn).
- CPU request: thường định nghĩa một trọng số; khi tranh chấp, request lớn hơn được nhiều CPU hơn[^k8s-cpureq]. Đây là hành vi của `cpu.weight` (suy luận của người soạn; trang này không nêu tên file).
- Memory limit: vượt thì OOM subsystem thường can thiệp bằng cách dừng một process trong container[^k8s-memlimit]; limit memory được thực thi *phản ứng* (reactively), chỉ khi kernel phát hiện áp lực[^k8s-reactive]. Docs không nêu tên file; đây là hành vi của `memory.max` (suy luận của người soạn).
- Memory request: chủ yếu dùng khi lập lịch Pod; trên cgroup v2 runtime có thể dùng nó làm gợi ý để đặt `memory.min` và `memory.low`[^k8s-memreq].

Nhận xét của người soạn: CPU limit nhỏ hơn số thread song song là công thức dễ gây throttle và tail latency; xác nhận bằng `nr_throttled`/`throttled_usec` trong `cpu.stat`, không đoán từ biểu đồ CPU%.

<details><summary>Đi sâu: <code>--cpu-shares 512</code> ra 59, <code>memory.high</code> của kubelet, quan sát <code>docker inspect</code></summary>

- Với `--memory 128m --cpus 0.5`, `docker inspect` cho `HostConfig.Memory=134217728`, `NanoCpus=500000000`; từ ngoài (`--cgroupns=host`), cgroup của container là `/sys/fs/cgroup/docker/<id>` với `memory.max=134217728`, `cpu.max=50000 100000`. Docs nói `--memory-swap` bằng `--memory` thì không có swap[^dk-swap]; tôi không chạy tổ hợp đó.
- Hai điểm bất ngờ: (1) `--cpu-shares 512` ra `cpu.weight=59`, không phải phép chia tuyến tính 512/1024×100 = 50: runtime áp một công thức chuyển đổi nào đó (tôi chỉ quan sát kết quả, không đọc source runtime); (2) `--memory` không đặt `memory.high`, nên Docker thuần chỉ có trần cứng `memory.max`, không có "vùng đệm throttle" của `memory.high` (suy luận của người soạn từ bảng quan sát và mục 4). Đừng suy ra từ đó rằng vượt `--memory` là OOM ngay: nếu chỉ đặt `--memory` mà không đặt `--memory-swap`, docs Docker cho container dùng thêm swap bằng đúng `--memory` khi host có swap[^dk-swap-default], nên anonymous memory có thể bị đẩy ra swap (chậm dần) trước khi OOM (suy luận của người soạn từ docs); quan sát ở [Chương 06, mục 3](06-runtime-oci.md): `--memory 20m` ra `memory.swap = 41943040` trong `config.json` và `memory.swap.max = 20971520` trong container. Muốn trần cứng không có swap thì đặt `--memory-swap` bằng `--memory`[^dk-swap]; các thí nghiệm OOM và `memory.high` ở mục 4 đặt `memory.swap.max=0` để loại yếu tố swap.
- Docs QoS của Kubernetes nêu cấu hình tùy chọn của kubelet: mặc định kubelet không đặt `memory.high` (khi `memoryThrottlingFactor` còn nil)[^k8s-high], và nếu bật thì với Pod Burstable, `memory.high = requests + memoryThrottlingFactor * (limits - requests)`[^k8s-high-formula]. Với `memoryReservationPolicy: TieredReservation`, Guaranteed pod có `memory.min` bằng memory request[^k8s-tier]; mặc định `None` thì kubelet không đặt `memory.min`/`memory.low`[^k8s-tier-none]. Các điều này có thể đổi theo phiên bản; đối chiếu docs đúng phiên bản cụm của bạn.

</details>

**Quay lại Q7:** systemd và Docker (đã quan sát) ghi vào cùng một nhóm file (`cpu.max`, `cpu.weight`, `memory.max`, `pids.max`, `io.*`); Kubernetes nói runtime cấu hình cgroup (ánh xạ tên file là suy luận); hãy đọc file thật để biết giá trị hiệu lực.

## 8. Vì sao app thấy RAM/CPU của host (Q8)

> **Trả lời ngắn.** Giới hạn cgroup là *giới hạn*, không phải *ảo hóa* các file thông tin: trong container `--memory 256m --cpus 1.5` ở máy này, `nproc` vẫn là 4 và `/proc/meminfo` vẫn báo RAM của VM. Thư viện nào tự đo tài nguyên bằng hai nguồn đó sẽ đoán sai (nhận xét của người soạn); runtime phải đọc thẳng `memory.max` và `cpu.max`, như `available_parallelism` của Rust làm cho CPU.

**Giải thích.**

Hình dung (nhận xét của người soạn): bạn thuê một tầng của tòa nhà và bị khóa cầu dao ở mức 20 kW, nhưng bảng thông tin ở sảnh vẫn ghi tổng công suất cả tòa. Ai đọc bảng sảnh để tự chia tải sẽ tính sai.

Quan sát: container `--memory 256m --cpus 1.5` vẫn cho `nproc = 4` và `/proc/meminfo` báo `MemTotal: 6052192 kB` (khoảng 5,8 GiB), giống VM, trong khi `memory.max=268435456` và `cpu.max=150000 100000`. `proc_meminfo(5)` định nghĩa `MemTotal` là tổng RAM khả dụng (RAM vật lý trừ vài phần dành riêng và mã nhị phân kernel)[^proc-memtotal]. Tức là ở máy này giới hạn cgroup không xuất hiện ở hai nguồn thông tin quen thuộc đó; chương này không đọc source kernel để giải thích sâu hơn.

Hệ quả (nhận xét của người soạn): thư viện tính kích thước pool/heap theo `/proc/meminfo` hoặc `nproc` có thể vượt giới hạn container và bị OOM-kill hoặc throttle. Runtime nên đọc thẳng `memory.max` cho bộ nhớ và `cpu.max` cho số worker thread, đúng như `available_parallelism` làm cho CPU (phần Liên hệ với Rust).

<details><summary>Đi sâu: phạm vi quan sát</summary>

- Kết quả trên là một lần chạy ở một VM, kernel 6.8, Docker 29.5.2; chương không khẳng định rằng mọi kernel/runtime đều như vậy.
- `proc_meminfo(5)` ở đây chỉ dùng để định nghĩa `MemTotal`; man page này không bàn về cgroup.[^proc-memtotal]

</details>

**Quay lại Q8:** `nproc` và `/proc/meminfo` không phản ánh giới hạn cgroup; muốn đúng phải đọc `memory.max`/`cpu.max`.

## Liên hệ với Rust: `available_parallelism` và cgroup

`std::thread::available_parallelism()` trả `io::Result<NonZero<usize>>`: một ước lượng về mức song song mặc định mà chương trình nên dùng[^rs-ap]. Docs 1.95.0 viết cụ thể về Linux: nó có thể đếm dư mức song song khả dụng khi bị giới hạn bởi affinity mask của process hoặc quota cgroup mà không truy vấn được `sched_getaffinity()` hay cgroup fs (ví dụ do sandbox)[^rs-over]. Từ câu đó và từ source bên dưới, tôi suy ra rằng khi truy vấn được thì hàm có tính cgroup quota (suy luận của người soạn); docs còn nêu các trường hợp đếm thiếu hoặc đếm dư khác, nên không nên đọc thành "chỉ sai khi không đọc được". Docs còn nêu: khi có cả giới hạn steady-state và burst thì dùng steady-state để độ trễ dễ dự đoán hơn[^rs-burst]; giá trị không được cache mà tính lại mỗi lần gọi, nên không nên gọi từ hot code[^rs-nocache]; và hàm không cố tính đến `ulimit` — nó không tính giới hạn `ulimit` về số thread[^rs-ulimit]. Với cgroup v1, docs nói hàm có thể phải quét các mountpoint để tìm controller cpu của v1; điều này không áp dụng cho cgroup v2[^rs-v1]. Docs không nhắc `pids.max`; việc hàm này không đọc `pids.max` là suy luận của người soạn từ source dưới đây.

Source 1.95.0 (`library/std/src/sys/thread/unix.rs`, chi tiết cài đặt, có thể đổi) cho thấy cách hiểu quota:

- Hàm `quota()` (theo comment trong source) trả quota CPU của cgroup tính theo "core tương đương", làm tròn xuống, hoặc `usize::MAX` nếu không xác định được hoặc không đặt quota[^rs-quota-doc].
- Đọc `/proc/self/cgroup` để biết cgroup của process (field thứ hai rỗng là v2)[^rs-proc-cgroup], rồi với v2 đọc `cpu.max` tại mount cố định `/sys/fs/cgroup` (comment trong source gọi đây là vị trí mount chuẩn được định nghĩa trong man page `file-hierarchy(7)`)[^rs-mount]. Nó leo ngược lên các cgroup tổ tiên (`while path.starts_with(cgroup_mount)`)[^rs-walk] và lấy `quota.min(limit / period)`[^rs-min] — chia số nguyên, tức làm tròn xuống.
- `quota().max(1)` rồi `count.min(quota)` với số CPU trong affinity mask[^rs-affinity]. Vậy kết quả = `min(affinity, floor(quota/period))`, tối thiểu 1.
- Source ghi các trường hợp chưa hỗ trợ, trong đó có cgroup v2 ở mount point không chuẩn[^rs-notcovered].

**Quan sát (chạy thật, binary Rust 1.95.0 gọi `available_parallelism`, VM 4 vCPU):**

| Điều kiện | `available_parallelism()` | `nproc` |
|---|---|---|
| không giới hạn (`cpu.max = max 100000`) | `Ok(4)` | 4 |
| `cpu.max = 20000 100000` (cgroup tự tạo) | `Ok(1)` | — |
| `--cpus 0.5` (`50000 100000`) | `Ok(1)` | 4 |
| `--cpus 1.5` (`150000 100000`) | `Ok(1)` | 4 |
| `--cpus 2` / `--cpus 3` | `Ok(2)` / `Ok(3)` | — |
| `--cpuset-cpus 0,1` | `Ok(2)` (từ affinity) | 2 |

Điểm cần ghi nhớ: `--cpus 1.5` cho `1`, không phải `2`, đúng như doc của `quota()` nói "rounded down". Muốn 2 thread thì phải tự chỉnh pool. Cách làm tròn xuống tránh throttle là lý do hợp lý (suy luận của người soạn). `available_parallelism` chỉ nói về CPU; docs của nó không đề cập giới hạn memory, nên heap/cache sizing của service Rust trong container phải tự đọc `memory.max`. Về thread pool lớn hơn quota, xem [chương Scheduling](../04-concurrency/02-scheduling.md).

## Tóm tắt

- cgroup tổ chức process thành cây và phân phối tài nguyên dọc cây; giới hạn gần gốc không bị vượt từ dưới[^cg-what][^cg-hier].
- v2 có một hierarchy, giao diện là filesystem `cgroup2`; bật controller bằng `cgroup.subtree_control`[^cg7-unified][^cg-nodef].
- "No internal process": cgroup không phải root chỉ chia tài nguyên cho con khi tự nó không có process; vi phạm cho `EBUSY` (đã quan sát)[^cg-nip].
- Memory tính cả page cache; `memory.high` throttle, `memory.max` OOM trong cgroup; `memory.events` cho biết cái nào đã xảy ra[^cg-tracked][^cg-ev-kill].
- `cpu.weight` chỉ có tác dụng khi tranh chấp; `cpu.max` là quota theo period và gây throttling (`nr_throttled`)[^cg-cpu-max][^cg-bwc].
- `io.max` giới hạn BPS/IOPS theo thiết bị; `pids.max` làm `fork`/`clone` trả `EAGAIN`[^cg-iomax][^cg-pids-eagain]; PSI đo thời gian stall[^psi-why].
- Docker `--memory/--cpus/--pids-limit` ra `memory.max`/`cpu.max`/`pids.max` (đã quan sát); `--cpu-shares 512` ra `cpu.weight=59`.
- App vẫn thấy RAM/CPU host qua `/proc/meminfo`, `nproc`; Rust `available_parallelism` đọc `cpu.max`, làm tròn xuống, tối thiểu 1[^rs-quota-doc].

## Câu hỏi tự kiểm tra

1. Namespace và cgroup khác nhau ở điểm nào?
<details><summary>Đáp án</summary>

Namespace quyết định process nhìn thấy gì; cgroup phân phối/giới hạn tài nguyên[^cg-what]. Ví dụ: `memory.max` giới hạn RAM[^cg-max].

</details>

2. Vì sao `echo +memory > cgroup.subtree_control` trả `Device or resource busy` khi cgroup đang có process, và cách làm đúng?
<details><summary>Đáp án</summary>

No internal process[^cg-nip]; tạo con và chuyển hết process sang con trước[^cg-nip-how].

</details>

3. Ghi file 300 MiB trong cgroup `memory.max=50M` không bị OOM, nhưng `hog mem 200` thì bị. Vì sao?
<details><summary>Đáp án</summary>

Page cache được tính vào cgroup nhưng reclaim được (`max` tăng, không OOM); anonymous memory không có swap thì không reclaim được[^cg-tracked][^cg-max][^cg-ev-max].

</details>

4. `memory.high` khác `memory.max` thế nào về hậu quả?
<details><summary>Đáp án</summary>

`high` throttle, không bao giờ gọi OOM killer; `max` là trần cứng, OOM trong cgroup[^cg-high][^cg-max].

</details>

5. `cpu.max` là `150000 100000`, ứng dụng có 4 thread busy. Điều gì xảy ra mỗi period?
<details><summary>Đáp án</summary>

Quota 150 ms CPU mỗi 100 ms; 4 thread dùng hết sau ~37,5 ms rồi bị throttle tới period sau[^cg-bwc]; `nr_throttled` tăng[^cg-bwc-stat].

</details>

6. `cpu.weight` khác `cpu.max` thế nào khi máy rảnh?
<details><summary>Đáp án</summary>

Weight chia tỉ lệ giữa con đang hoạt động (work-conserving), nên một mình dùng hết CPU; max là trần cả khi rảnh[^cg-weights][^cg-limits].

</details>

7. PSI `some` và `full` khác nhau thế nào?
<details><summary>Đáp án</summary>

`some`: ít nhất một số task stall; `full`: mọi task non-idle cùng stall[^psi-some][^psi-full].

</details>

8. Vì sao `/proc/meminfo` trong container `--memory 256m` vẫn báo ~5,8 GiB?
<details><summary>Đáp án</summary>

Quan sát: giới hạn cgroup không xuất hiện ở file này; `MemTotal` được định nghĩa là RAM khả dụng của máy[^proc-memtotal]. Runtime phải đọc `memory.max`.

</details>

9. `available_parallelism` với `--cpus 1.5` trên host 4 CPU trả gì, theo cơ chế nào?
<details><summary>Đáp án</summary>

`1`: `limit / period` chia số nguyên, min với affinity, tối thiểu 1[^rs-quota-doc][^rs-min][^rs-affinity].

</details>

10. Khi nào `available_parallelism` có thể trả quá lớn trong container?
<details><summary>Đáp án</summary>

Không đọc được `sched_getaffinity()` hay cgroup fs (sandbox), hoặc cgroup v2 ở mount không chuẩn[^rs-over][^rs-notcovered].

</details>

## Đọc thêm

- Kernel docs, [Control Group v2](https://docs.kernel.org/admin-guide/cgroup-v2.html): đặc tả đầy đủ mọi file; đọc mục "Resource Distribution Models" và "Memory Interface Files".
- `cgroups(7)` bookworm: [cgroups.7](https://manpages.debian.org/bookworm/manpages/cgroups.7.en.html) — lịch sử v1/v2 và danh sách file.
- Kernel docs, [CFS Bandwidth Control](https://docs.kernel.org/scheduler/sched-bwc.html): cơ chế quota/period/slice và `cpu.stat`.
- Kernel docs, [PSI](https://docs.kernel.org/accounting/psi.html): trigger/poll và cách dùng PSI để load-shed.
- `systemd.resource-control(5)`: [bản bookworm](https://manpages.debian.org/bookworm/systemd/systemd.resource-control.5.en.html) — directive ↔ file cgroup.
- [Namespaces](02-namespaces.md) và [Container là gì](01-container-la-gi.md) — nửa còn lại của container; [Page fault và OOM](../03-memory/04-page-fault-va-swapping.md) — cơ chế đằng sau `memory.high/max`.

[^cg-what]: Linux kernel v6.8, Control Group v2, What is cgroup? — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "cgroup is a mechanism to organize processes hierarchically and distribute system resources along the hierarchy in a controlled and configurable manner."
[^cg-parts]: Linux kernel v6.8, Control Group v2, What is cgroup? — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "cgroup is largely composed of two parts - the core and controllers. cgroup core is primarily responsible for hierarchically organizing processes. A cgroup controller is usually responsible for distributing a specific type of system resource along the hierarchy"
[^cg-one]: Linux kernel v6.8, Control Group v2, What is cgroup? — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "every process in the system belongs to one and only one cgroup. All threads of a process belong to the same cgroup."
[^cg-threads]: Linux kernel v6.8, Control Group v2, Threads — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "cgroup v2 supports thread granularity for a subset of controllers to support use cases requiring hierarchical resource distribution across the threads of a group of processes. By default, all threads of a process belong to the same cgroup"
[^cg-fork]: Linux kernel v6.8, Control Group v2, Processes — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "the new process is born into the cgroup that the forking process belongs to at the time of the operation."
[^cg-hier]: Linux kernel v6.8, Control Group v2, What is cgroup? — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "The restrictions set closer to the root in the hierarchy can not be overridden from further away."
[^cg7-why]: cgroups(7), Cgroups version 1 and version 2 — <https://manpages.debian.org/bookworm/manpages/cgroups.7.en.html> — "the development of these controllers was largely uncoordinated, with the result that many inconsistencies arose between controllers and management of the cgroup hierarchies became rather complex."
[^cg7-224]: cgroups(7), Cgroups version 1 and version 2 — <https://manpages.debian.org/bookworm/manpages/cgroups.7.en.html> — "The initial release of the cgroups implementation was in Linux 2.6.24."
[^cg7-310]: cgroups(7), Cgroups version 1 and version 2 — <https://manpages.debian.org/bookworm/manpages/cgroups.7.en.html> — "starting in Linux 3.10, work began on a new, orthogonal implementation to remedy these problems."
[^cg-rmdir]: Linux kernel v6.8, Control Group v2, Processes — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "A cgroup which doesn't have any children or live processes can be destroyed by removing the directory."
[^cg7-45]: cgroups(7), Cgroups version 1 and version 2 — <https://manpages.debian.org/bookworm/manpages/cgroups.7.en.html> — "was eventually made official with the release of Linux 4.5."
[^cg7-unified]: cgroups(7), Cgroups v2 unified hierarchy — <https://manpages.debian.org/bookworm/manpages/cgroups.7.en.html> — "the flexibility turned out to be less useful than expected, and in many cases added complexity. Therefore, in cgroups v2, all available controllers are mounted against a single hierarchy."
[^cg7-nip]: cgroups(7), CGROUPS VERSION 2 — <https://manpages.debian.org/bookworm/manpages/cgroups.7.en.html> — "processes may reside only in leaf nodes (cgroups that do not themselves contain child cgroups)."
[^cg7-nip-precise]: cgroups(7), Cgroups v2 "no internal processes" rule — <https://manpages.debian.org/bookworm/manpages/cgroups.7.en.html> — "Thus, it is possible for a cgroup to have both member processes and child cgroups, but before controllers can be enabled for that cgroup, the member processes must be moved out of the cgroup (e.g., perhaps into the child cgroups)."
[^cg7-both]: cgroups(7), Cgroups version 1 and version 2 — <https://manpages.debian.org/bookworm/manpages/cgroups.7.en.html> — "a controller can't be simultaneously employed in both a cgroups v1 hierarchy and in the cgroups v2 hierarchy."
[^cg7-subset]: cgroups(7), Cgroups version 1 and version 2 — <https://manpages.debian.org/bookworm/manpages/cgroups.7.en.html> — "Currently, cgroups v2 implements only a subset of the controllers available in cgroups v1."
[^k8s-v1]: Kubernetes docs, About cgroup v2 (Deprecation of cgroup v1; không ghim) — <https://kubernetes.io/docs/concepts/architecture/cgroups/> — "Kubernetes has deprecated cgroup v1."
[^cg-mkdir]: Linux kernel v6.8, Control Group v2, Processes — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Initially, only the root cgroup exists to which all processes belong. A child cgroup can be created by creating a sub-directory"
[^cg-procs]: Linux kernel v6.8, Control Group v2, Processes — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Only one process can be migrated on a single write(2) call. If a process is composed of multiple threads, writing the PID of any thread migrates all threads of the process."
[^cg-procs-read]: Linux kernel v6.8, Control Group v2, Core Interface Files — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "When read, it lists the PIDs of all processes which belong to the cgroup one-per-line."
[^cg-mount]: Linux kernel v6.8, Control Group v2, Mounting — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Unlike v1, cgroup v2 has only single hierarchy. The cgroup v2 hierarchy can be mounted with the following mount command"
[^cg-ctrl-file]: Linux kernel v6.8, Control Group v2, Core Interface Files — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "It shows space separated list of all controllers available to the cgroup."
[^cg-subtree-file]: Linux kernel v6.8, Control Group v2, Core Interface Files — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "When read, it shows space separated list of the controllers which are enabled to control resource distribution from the cgroup to its children."
[^cg-ctrl-only]: Linux kernel v6.8, Control Group v2, Enabling and Disabling — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Only controllers which are listed in "cgroup.controllers" can be enabled."
[^cg-nodef]: Linux kernel v6.8, Control Group v2, Enabling and Disabling — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "No controller is enabled by default."
[^cg-enable]: Linux kernel v6.8, Control Group v2, Enabling and Disabling — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Enabling a controller in a cgroup indicates that the distribution of the target resource across its immediate children will be controlled."
[^cg-owned]: Linux kernel v6.8, Control Group v2, Enabling and Disabling — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "are owned by the parent rather than the cgroup itself."
[^cg-topdown]: Linux kernel v6.8, Control Group v2, Top-down Constraint — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "a controller can't be disabled if one or more children have it enabled."
[^cgns]: cgroup_namespaces(7) — <https://manpages.debian.org/bookworm/manpages/cgroup_namespaces.7.en.html> — "Cgroup namespaces virtualize the view of a process's cgroups"
[^cgns-root]: cgroup_namespaces(7) — <https://manpages.debian.org/bookworm/manpages/cgroup_namespaces.7.en.html> — "Each cgroup namespace has its own set of cgroup root directories."
[^cg-nsdeleg]: Linux kernel v6.8, Control Group v2, Mount options — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Consider cgroup namespaces as delegation boundaries."
[^cg-nip]: Linux kernel v6.8, Control Group v2, No Internal Process Constraint — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Non-root cgroups can distribute domain resources to their children only when they don't have any processes of their own."
[^cg-nip-why]: Linux kernel v6.8, Control Group v2, No Internal Process Constraint — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "This rules out situations where child cgroups compete against internal processes of the parent."
[^cg-root-exempt]: Linux kernel v6.8, Control Group v2, No Internal Process Constraint — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "The root cgroup is exempt from this restriction."
[^cg-nip-how]: Linux kernel v6.8, Control Group v2, No Internal Process Constraint — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "the cgroup must create children and transfer all its processes to the children before enabling controllers in its"
[^cg-deleg]: Linux kernel v6.8, Control Group v2, Model of Delegation — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "A cgroup can be delegated in two ways."
[^cg-deleg-files]: Linux kernel v6.8, Control Group v2, Model of Delegation — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "the kernel rejects writes to all files other than "cgroup.procs" and "cgroup.subtree_control" on a namespace root from inside the namespace."
[^cg-deleg-contain]: Linux kernel v6.8, Control Group v2, Model of Delegation — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "nothing can escape the resource restrictions imposed by the parent."
[^cg-deleg-contain2]: Linux kernel v6.8, Control Group v2, Delegation Containment — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "processes can't be moved into or out of the sub-hierarchy by the delegatee."
[^sd-deleg]: systemd.resource-control(5), Enabling and disabling controllers — <https://manpages.debian.org/bookworm/systemd/systemd.resource-control.5.en.html> — "The delegatee may then enable controllers for its children as appropriate."
[^cg-weights]: Linux kernel v6.8, Control Group v2, Weights — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "As only children which can make use of the resource at the moment participate in the distribution, this is work-conserving."
[^cg-limits]: Linux kernel v6.8, Control Group v2, Limits — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Limits can be over-committed - the sum of the limits of children can exceed the amount of resource available to the parent."
[^cg-prot]: Linux kernel v6.8, Control Group v2, Protections — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Protections can also be over-committed in which case only up to the amount available to the parent is protected among children."
[^cg-w-def]: Linux kernel v6.8, Control Group v2, Weights — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "All weights are in the range [1, 10000] with the default at 100."
[^cg-lim-def]: Linux kernel v6.8, Control Group v2, Limits — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Limits are in the range [0, max] and defaults to "max", which is noop."
[^cg-prot-def]: Linux kernel v6.8, Control Group v2, Protections — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Protections are in the range [0, max] and defaults to 0, which is noop."
[^cg-tracked]: Linux kernel v6.8, Control Group v2, Memory — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Userland memory - page cache and anonymous memory."
[^cg-stat-file]: Linux kernel v6.8, Control Group v2, memory.stat — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Amount of memory used to cache filesystem data, including tmpfs and shared memory."
[^cg-charge]: Linux kernel v6.8, Control Group v2, Memory Ownership — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "A memory area is charged to the cgroup which instantiated it and stays charged to the cgroup until the area is released."
[^cg-charge-move]: Linux kernel v6.8, Control Group v2, Memory Ownership — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Migrating a process to a different cgroup doesn't move the memory usages that it instantiated while in the previous cgroup to the new cgroup."
[^cg-ev-max]: Linux kernel v6.8, Control Group v2, memory.events — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "The number of times the cgroup's memory usage was about to go over the max boundary. If direct reclaim fails to bring it down, the cgroup goes to OOM state."
[^cg-cur]: Linux kernel v6.8, Control Group v2, memory.current — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "The total amount of memory currently being used by the cgroup and its descendants."
[^cg-min]: Linux kernel v6.8, Control Group v2, memory.min — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "the cgroup's memory won't be reclaimed under any conditions."
[^cg-min-over]: Linux kernel v6.8, Control Group v2, memory.min — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Putting more memory than generally available under this protection is discouraged and may lead to constant OOMs."
[^cg-low]: Linux kernel v6.8, Control Group v2, memory.low — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "memory won't be reclaimed unless there is no reclaimable memory available in unprotected cgroups."
[^cg-high]: Linux kernel v6.8, Control Group v2, memory.high — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Going over the high limit never invokes the OOM killer and under extreme conditions the limit may be breached."
[^cg-max]: Linux kernel v6.8, Control Group v2, memory.max — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "If a cgroup's memory usage reaches this limit and can't be reduced, the OOM killer is invoked in the cgroup."
[^cg-stat-keys]: Linux kernel v6.8, Control Group v2, memory.stat — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "new entries can show up in the middle. Don't rely on items remaining in a fixed position; use the keys to look up specific values!"
[^cg-peak]: Linux kernel v6.8, Control Group v2, memory.peak — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "The max memory usage recorded for the cgroup and its descendants since the creation of the cgroup."
[^cg-ev-high]: Linux kernel v6.8, Control Group v2, memory.events — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "The number of times processes of the cgroup are throttled and routed to perform direct memory reclaim because the high memory boundary was exceeded."
[^cg-ev-kill]: Linux kernel v6.8, Control Group v2, memory.events — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "The number of processes belonging to this cgroup killed by any kind of OOM killer."
[^cg-ev-hier]: Linux kernel v6.8, Control Group v2, memory.events — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Note that all fields in this file are hierarchical"
[^cg-ev-local]: Linux kernel v6.8, Control Group v2, memory.events.local — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Similar to memory.events but the fields in the file are local to the cgroup i.e. not hierarchical."
[^cg-oomgroup]: Linux kernel v6.8, Control Group v2, memory.oom.group — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "all tasks belonging to the cgroup or to its descendants (if the memory cgroup is not a leaf cgroup) are killed together or not at all."
[^cg-cpu-weight]: Linux kernel v6.8, Control Group v2, cpu.weight — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "A read-write single value file which exists on non-root cgroups. The default is "100". For non idle groups (cpu.idle = 0), the weight is in the range [1, 10000]."
[^cg-cpu-max]: Linux kernel v6.8, Control Group v2, cpu.max — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "which indicates that the group may consume up to $MAX in each $PERIOD duration."
[^cg-bwc]: Linux kernel docs, CFS Bandwidth Control (docs.kernel.org, không ghim) — <https://docs.kernel.org/scheduler/sched-bwc.html> — "Once all quota has been assigned any additional requests for quota will result in those threads being throttled. Throttled threads will not be able to run again until the next period when the quota is replenished."
[^cg-cpu-stat]: Linux kernel v6.8, Control Group v2, cpu.stat — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "and the following five when the controller is enabled:"
[^cg-bwc-stat]: Linux kernel docs, CFS Bandwidth Control (docs.kernel.org, không ghim) — <https://docs.kernel.org/scheduler/sched-bwc.html> — "nr_throttled: Number of times the group has been throttled/limited."
[^cg-bwc-time]: Linux kernel docs, CFS Bandwidth Control (docs.kernel.org, không ghim) — <https://docs.kernel.org/scheduler/sched-bwc.html> — "throttled_time: The total time duration (in nanoseconds) for which entities of the group have been throttled."
[^cg-iomax]: Linux kernel v6.8, Control Group v2, io.max — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "BPS and IOPS are measured in each IO direction and IOs are delayed if limit is reached."
[^cg-ioweight]: Linux kernel v6.8, Control Group v2, io.weight — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "specifies the relative amount IO time the cgroup can use in relation to its siblings."
[^cg-writeback]: Linux kernel v6.8, Control Group v2, Writeback — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Page cache is dirtied through buffered writes and shared mmaps and written asynchronously to the backing filesystem by the writeback mechanism. Writeback sits between the memory and IO domains and regulates the proportion of dirty memory by balancing dirtying and write IOs."
[^cg-io-blkmq]: Linux kernel v6.8, Control Group v2, IO — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "weight based distribution is available only if cfq-iosched is in use and neither scheme is available for blk-mq devices."
[^cg-pidsmax]: Linux kernel v6.8, Control Group v2, pids.max — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Hard limit of number of processes."
[^cg-pids-eagain]: Linux kernel v6.8, Control Group v2, PID Interface Files — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "it is not possible to violate a cgroup PID policy through fork() or clone(). These will return -EAGAIN"
[^cg-pids-tid]: Linux kernel v6.8, Control Group v2, PID — <https://raw.githubusercontent.com/torvalds/linux/v6.8/Documentation/admin-guide/cgroup-v2.rst> — "Note that PIDs used in this controller refer to TIDs, process IDs as used by the kernel."
[^psi-why]: Linux kernel docs, PSI - Pressure Stall Information (docs.kernel.org, không ghim) — <https://docs.kernel.org/accounting/psi.html> — "When CPU, memory or IO devices are contended, workloads experience latency spikes, throughput losses, and run the risk of OOM kills."
[^psi-quant]: Linux kernel docs, PSI - Pressure Stall Information (docs.kernel.org, không ghim) — <https://docs.kernel.org/accounting/psi.html> — "The psi feature identifies and quantifies the disruptions caused by such resource crunches and the time impact it has on complex workloads or even entire systems."
[^psi-proc]: Linux kernel docs, PSI, Pressure interface (docs.kernel.org, không ghim) — <https://docs.kernel.org/accounting/psi.html> — "Pressure information for each resource is exported through the respective file in /proc/pressure/ -- cpu, memory, and io."
[^psi-cpufull]: Linux kernel docs, PSI, Pressure interface (docs.kernel.org, không ghim) — <https://docs.kernel.org/accounting/psi.html> — "CPU full is undefined at the system level, but has been reported since 5.13, so it is set to zero for backward compatibility."
[^psi-cg]: Linux kernel docs, PSI, Cgroup2 interface — <https://docs.kernel.org/accounting/psi.html> — "Each subdirectory in the cgroupfs mountpoint contains cpu.pressure, memory.pressure, and io.pressure files"
[^psi-some]: Linux kernel docs, PSI, Pressure interface — <https://docs.kernel.org/accounting/psi.html> — "the share of time in which at least some tasks are stalled on a given resource."
[^psi-full]: Linux kernel docs, PSI, Pressure interface — <https://docs.kernel.org/accounting/psi.html> — "the share of time in which all non-idle tasks are stalled on a given resource simultaneously. In this state actual CPU cycles are going to waste"
[^psi-avg]: Linux kernel docs, PSI, Pressure interface — <https://docs.kernel.org/accounting/psi.html> — "The ratios (in %) are tracked as recent trends over ten, sixty, and three hundred second windows, which gives insight into short term events as well as medium and long term trends. The total absolute stall time (in us) is tracked and exported as well, to allow detection of latency spikes"
[^psi-trigger]: Linux kernel docs, PSI, Monitoring for pressure thresholds — <https://docs.kernel.org/accounting/psi.html> — "For example writing “some 150000 1000000” into /proc/pressure/memory would add 150ms threshold for partial memory stall measured within 1sec time window."
[^sd-enable]: systemd.resource-control(5), Enabling and disabling controllers — <https://manpages.debian.org/bookworm/systemd/systemd.resource-control.5.en.html> — "systemd will instruct the kernel to enable a controller for a given unit when this unit has configuration for a given controller."
[^sd-intro]: systemd.resource-control(5), DESCRIPTION — <https://manpages.debian.org/bookworm/systemd/systemd.resource-control.5.en.html> — "Unit configuration files for services, slices, scopes, sockets, mount points, and swap devices share a subset of configuration options for resource control of spawned processes. Internally, this relies on the Linux Control Groups (cgroups) kernel concept"
[^sd-cpuweight]: systemd.resource-control(5), CPUWeight= — <https://manpages.debian.org/bookworm/systemd/systemd.resource-control.5.en.html> — "These options control the "cpu.weight" control group attribute. The allowed range is 1 to 10000."
[^sd-cpuquota]: systemd.resource-control(5), CPUQuota= — <https://manpages.debian.org/bookworm/systemd/systemd.resource-control.5.en.html> — "Use values > 100% for allotting CPU time on more than one CPU. This controls the "cpu.max" attribute on the unified control group hierarchy"
[^sd-memhigh]: systemd.resource-control(5), MemoryHigh= — <https://manpages.debian.org/bookworm/systemd/systemd.resource-control.5.en.html> — "This is the main mechanism to control memory usage of a unit."
[^sd-memhigh-file]: systemd.resource-control(5), MemoryHigh= — <https://manpages.debian.org/bookworm/systemd/systemd.resource-control.5.en.html> — "no memory throttling is applied. This controls the "memory.high" control group attribute."
[^sd-memmax]: systemd.resource-control(5), MemoryMax= — <https://manpages.debian.org/bookworm/systemd/systemd.resource-control.5.en.html> — "It is recommended to use MemoryHigh= as the main control mechanism and use MemoryMax= as the last line of defense."
[^sd-memmax-file]: systemd.resource-control(5), MemoryMax= — <https://manpages.debian.org/bookworm/systemd/systemd.resource-control.5.en.html> — "no memory limit is applied. This controls the "memory.max" control group attribute."
[^sd-tasks]: systemd.resource-control(5), TasksMax= — <https://manpages.debian.org/bookworm/systemd/systemd.resource-control.5.en.html> — "no tasks limit is applied. This controls the "pids.max" control group attribute."
[^sd-ioweight]: systemd.resource-control(5), IOWeight= — <https://manpages.debian.org/bookworm/systemd/systemd.resource-control.5.en.html> — "Takes a single weight value (between 1 and 10000) to set the default block I/O weight. This controls the "io.weight" control group attribute, which defaults to 100."
[^sd-iomax]: systemd.resource-control(5), IOReadBandwidthMax= — <https://manpages.debian.org/bookworm/systemd/systemd.resource-control.5.en.html> — "This limit is not work-conserving and the executed processes are not allowed to use more even if the device has idle capacity."
[^sd-iomax-file]: systemd.resource-control(5), IOReadBandwidthMax= — <https://manpages.debian.org/bookworm/systemd/systemd.resource-control.5.en.html> — "This controls the "io.max" control group attributes. Use this option multiple times to set bandwidth limits for multiple devices."
[^dk-cpus]: Docker docs, Resource constraints, --cpus (không ghim) — <https://docs.docker.com/engine/containers/resource_constraints/> — "Specify how much of the available CPU resources a container can use. For instance, if the host machine has two CPUs and you set --cpus="1.5", the container is guaranteed at most one and a half of the CPUs. This is the equivalent of setting --cpu-period="100000" and --cpu-quota="150000"."
[^dk-swap]: Docker docs, Resource constraints, --memory-swap details (không ghim) — <https://docs.docker.com/engine/containers/resource_constraints/> — "If --memory-swap is set to the same value as --memory, and --memory is set to a positive integer, the container doesn't have access to swap."
[^dk-swap-default]: Docker docs, Resource constraints, --memory-swap details (không ghim) — <https://docs.docker.com/engine/containers/resource_constraints/> — "If --memory-swap is unset, and --memory is set, the container can use as much swap as the --memory setting, if the host has swap memory configured."
[^dk-swap-total]: Docker docs, Resource constraints, --memory-swap details (không ghim) — <https://docs.docker.com/engine/containers/resource_constraints/> — "--memory-swap represents the total amount of memory and swap that can be used, and --memory controls the amount used by non-swap memory."
[^dk-shares]: Docker docs, Resource constraints, --cpu-shares (không ghim) — <https://docs.docker.com/engine/containers/resource_constraints/> — "Set this flag to a value greater or less than the default of 1024 to increase or reduce the container's weight, and give it access to a greater or lesser proportion of the host machine's CPU cycles. This is only enforced when CPU cycles are constrained."
[^dk-min]: Docker docs, Resource constraints, -m (không ghim) — <https://docs.docker.com/engine/containers/resource_constraints/> — "the minimum allowed value is 6m (6 megabytes)."
[^k8s-cg]: Kubernetes docs, Resource Management for Pods and Containers (không ghim) — <https://kubernetes.io/docs/concepts/configuration/manage-resources-containers/> — "On Linux, the container runtime typically configures kernel cgroups that apply and enforce the limits you defined."
[^k8s-cpulimit]: Kubernetes docs, Resource Management for Pods and Containers (không ghim) — <https://kubernetes.io/docs/concepts/configuration/manage-resources-containers/> — "The CPU limit defines a hard ceiling on how much CPU time the container can use."
[^k8s-cpureq]: Kubernetes docs, Resource Management for Pods and Containers (không ghim) — <https://kubernetes.io/docs/concepts/configuration/manage-resources-containers/> — "The CPU request typically defines a weighting."
[^k8s-memlimit]: Kubernetes docs, Resource Management for Pods and Containers (không ghim) — <https://kubernetes.io/docs/concepts/configuration/manage-resources-containers/> — "the Linux kernel out-of-memory subsystem activates and, typically, intervenes by stopping one of the processes in the container that tried to allocate memory."
[^k8s-reactive]: Kubernetes docs, Resource Management for Pods and Containers (không ghim) — <https://kubernetes.io/docs/concepts/configuration/manage-resources-containers/> — "This means memory limits are enforced reactively."
[^k8s-memreq]: Kubernetes docs, Resource Management for Pods and Containers (không ghim) — <https://kubernetes.io/docs/concepts/configuration/manage-resources-containers/> — "the container runtime might use the memory request as a hint to set memory.min and memory.low."
[^k8s-high]: Kubernetes docs, Pod Quality of Service Classes (không ghim) — <https://kubernetes.io/docs/concepts/workloads/pods/pod-qos/> — "Its default value is nil, which means that the kubelet does not set memory.high."
[^k8s-high-formula]: Kubernetes docs, Pod Quality of Service Classes (không ghim) — <https://kubernetes.io/docs/concepts/workloads/pods/pod-qos/> — "memory.high = requests + memoryThrottlingFactor * (limits - requests)"
[^k8s-tier]: Kubernetes docs, Pod Quality of Service Classes (không ghim) — <https://kubernetes.io/docs/concepts/workloads/pods/pod-qos/> — "Guaranteed pods: memory.min is set to memory requests."
[^k8s-tier-none]: Kubernetes docs, Pod Quality of Service Classes (không ghim) — <https://kubernetes.io/docs/concepts/workloads/pods/pod-qos/> — "None (default): the kubelet does not set memory.min or memory.low for containers and pods."
[^proc-memtotal]: proc_meminfo(5), MemTotal — <https://manpages.debian.org/bookworm/manpages/proc_meminfo.5.en.html> — "Total usable RAM (i.e., physical RAM minus a few reserved bits and the kernel binary code)."
[^rs-ap]: Rust 1.95.0 std, thread::available_parallelism — <https://doc.rust-lang.org/1.95.0/std/thread/fn.available_parallelism.html> — "Returns an estimate of the default amount of parallelism a program should use."
[^rs-over]: Rust 1.95.0 std, thread::available_parallelism — <https://doc.rust-lang.org/1.95.0/std/thread/fn.available_parallelism.html> — "It may overcount the amount of parallelism available when limited by a process-wide affinity mask or cgroup quotas"
[^rs-v1]: Rust 1.95.0 std, thread::available_parallelism — <https://doc.rust-lang.org/1.95.0/std/thread/fn.available_parallelism.html> — "If the process is in a cgroup v1 cpu controller, this may need to scan mountpoints to find the corresponding cgroup v1 controller, which may take time on systems with large numbers of mountpoints."
[^rs-burst]: Rust 1.95.0 std, thread::available_parallelism — <https://doc.rust-lang.org/1.95.0/std/thread/fn.available_parallelism.html> — "Where both fixed steady-state and burst limits are available the steady-state capacity will be used to ensure more predictable latencies."
[^rs-nocache]: Rust 1.95.0 std, thread::available_parallelism — <https://doc.rust-lang.org/1.95.0/std/thread/fn.available_parallelism.html> — "therefore the value is not cached and instead recomputed every time this function is called. It should not be called from hot code."
[^rs-ulimit]: Rust 1.95.0 std, thread::available_parallelism — <https://doc.rust-lang.org/1.95.0/std/thread/fn.available_parallelism.html> — "It does not attempt to take ulimit into account."
[^rs-quota-doc]: Rust 1.95.0 source, library/std/src/sys/thread/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/thread/unix.rs> — "Returns cgroup CPU quota in core-equivalents, rounded down or usize::MAX if the quota cannot be determined or is not set."
[^rs-proc-cgroup]: Rust 1.95.0 source, library/std/src/sys/thread/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/thread/unix.rs> — "2nd field is a list of controllers for v1 or empty for v2"
[^rs-mount]: Rust 1.95.0 source, library/std/src/sys/thread/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/thread/unix.rs> — "standard mount location defined in file-hierarchy(7) manpage"
[^rs-walk]: Rust 1.95.0 source, library/std/src/sys/thread/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/thread/unix.rs> — "while path.starts_with(cgroup_mount) {"
[^rs-min]: Rust 1.95.0 source, library/std/src/sys/thread/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/thread/unix.rs> — "quota = quota.min(limit / period);"
[^rs-affinity]: Rust 1.95.0 source, library/std/src/sys/thread/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/thread/unix.rs> — "let count = count.min(quota);"
[^rs-notcovered]: Rust 1.95.0 source, library/std/src/sys/thread/unix.rs — <https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/thread/unix.rs> — "cgroup v2 in non-standard mountpoints"
