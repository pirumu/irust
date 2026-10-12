# Lab P10 — Rust internals

> Đây là phần **thực hành** của [P10 · Rust internals](../10-rust-internals/00-tong-quan.md). Hãy đọc xong lý thuyết trước rồi mới làm lab: lab chỉ kiểm chứng và minh họa những gì các chương P10 đã nêu, và mỗi lab ghi rõ chương và mục cần đọc. Mọi fact có chú thích cuối trang, kiểm bằng `python3 tools/verify_citations.py docs/labs/10-rust-internals-lab.md`. Output trong bài là output **thật** đã chạy trên máy người soạn (macOS aarch64, rustc 1.95.0); giá trị phụ thuộc máy được ghi chú.

## Chuẩn bị và các bài lab

Crate: `labs/crates/lab-internals`, không phụ thuộc crate ngoài. Bốn binary + một snippet. Không có code chỉ-Linux nên `cargo build -p lab-internals` chạy ngay trên macOS; người soạn cũng chạy `type_layout` và `mono` trong container Linux (`./labs/linux.sh`). Máy người soạn: macOS aarch64 và container Linux aarch64 — **chưa thử x86_64**, nơi các số `repr(Rust)` có thể khác.

```bash
rustc -Vv | head -3
```

```text
rustc 1.95.0 (59807616e 2026-04-14)
binary: rustc
commit-hash: 59807616e1fa2540724bfbac14d7976d7e4a3860
```

### Lab (a) — `type_layout`: size, align, niche, fat pointer

Lý thuyết: [chương 02 mục 1](../10-rust-internals/02-type-layout.md) (size/alignment/padding, số đo không phải hợp đồng), [mục 2](../10-rust-internals/02-type-layout.md) (`repr(Rust)` vs `repr(C)`), [mục 3](../10-rust-internals/02-type-layout.md) (`packed`, `transparent`), [mục 4](../10-rust-internals/02-type-layout.md) (enum, tag), [mục 5](../10-rust-internals/02-type-layout.md) và [mục 6](../10-rust-internals/02-type-layout.md) (niche, danh sách std hứa), [mục 7](../10-rust-internals/02-type-layout.md) (ZST), [chương 03 mục 2](../10-rust-internals/03-dst-fat-pointer-vtable.md) (fat pointer) và [mục 4](../10-rust-internals/03-dst-fat-pointer-vtable.md) (vtable, `size_of_val` của `dyn`).

```bash
cd labs && cargo run -q -p lab-internals --bin type_layout    # exit 0
```

Output (rút gọn; macOS aarch64 và Linux aarch64 trong container cho output **giống hệt** khi `diff`):

```text
== same fields, different repr ==
RustRepr                           size= 8 align= 4
CRepr                              size=12 align= 4
PackedRepr                         size= 7 align= 1
RustRepr   offsets: a=6 b=0 c=4
CRepr      offsets: a=0 b=4 c=8
PackedRepr offsets: a=0 b=1 c=5
Meters                             size= 8 align= 8
f64                                size= 8 align= 8

== enums ==
FieldlessRust                      size= 1 align= 1
FieldlessU32                       size= 4 align= 4
WithPayload                        size=16 align= 8

== niche optimization ==
&u64                               size= 8 align= 8
Option<&u64>                       size= 8 align= 8
Option<Box<u64>>                   size= 8 align= 8
Option<NonNull<u64>>               size= 8 align= 8
*const u64                         size= 8 align= 8
Option<*const u64>                 size=16 align= 8
u32                                size= 4 align= 4
Option<u32>                        size= 8 align= 4
NonZeroU32                         size= 4 align= 4
Option<NonZeroU32>                 size= 4 align= 4
Option<bool>                       size= 1 align= 1
Option<MaybeUninit<bool>>          size= 2 align= 1
Option<fn(u8) -> u8>               size= 8 align= 8

== zero-sized types ==
Nothing                            size= 0 align= 1
[u64; 0]                           size= 0 align= 8
PhantomData<String>                size= 0 align= 1
Tagged<String>                     size= 4 align= 4
Vec<()> with len=1000000 has capacity=18446744073709551615

== wide (fat) pointers ==
&[u8; 4]                           size= 8 align= 8
&[u8]                              size=16 align= 8
&str                               size=16 align= 8
&dyn Shape                         size=16 align= 8
Box<dyn Shape>                     size=16 align= 8
Option<Box<dyn Shape>>             size=16 align= 8
slice: data ptr == &arr[1]? true  len metadata = 3  size_of_val = 12
dyn Shape -> Square: size_of_val =  8  area =    4  data ptr points at the concrete value? true
dyn Shape -> Circle: size_of_val = 24  area =    3  data ptr points at the concrete value? true
&dyn Debug of &str: size_of_val = 16
```

Giải thích:

1. **Padding.** `CRepr` = 12: `a@0`, 3 byte padding, `b@4`, `c@8`, 2 byte padding cuối để size là bội của alignment 4 — đúng thuật toán của Reference[^ref-c-algo][^ref-c-size]. `PackedRepr` = 7, không padding[^ref-packed].
2. **`repr(Rust)` đã đổi thứ tự.** `RustRepr` có offset `b=0, c=4, a=6`, size 8 — nhỏ hơn 12. Đây là *quan sát* trên rustc 1.95.0, còn tài liệu chỉ hứa các bảo đảm soundness[^ref-rust-order]. Đừng viết code dựa vào con số này.
3. **Niche.** `Option<&u64>`, `Option<Box<u64>>`, `Option<NonNull<u64>>`, `Option<NonZeroU32>`, `Option<fn>` đều bằng kiểu gốc — khớp danh sách được hứa[^opt-rep]. `Option<u32>` = 8 (cần tag + padding), `Option<*const u64>` = 16: raw pointer không nằm trong danh sách mà [chương 02 mục 6](../10-rust-internals/02-type-layout.md) nêu, và output cho thấy kiểu này không có chỗ cho `None` trong chính nó (suy luận của người soạn từ output). `Option<bool>` = 1 là điều quan sát được nhưng **không** nằm trong danh sách bảo đảm. `Option<MaybeUninit<bool>>` = 2 cho thấy `MaybeUninit<bool>` không còn niche của `bool` (suy luận của người soạn từ output; nguồn chỉ bảo đảm `MaybeUninit<T>` cùng size/align/ABI với `T`[^mu-layout]).
4. **ZST.** `Tagged<String>` = 4: `PhantomData<String>` không thêm byte. `Vec<()>` với 1.000.000 phần tử báo capacity `usize::MAX` — doc chỉ nói với ZST `Vec` có thể không báo capacity 0[^vec-zst]; giá trị `usize::MAX` là quan sát.
5. **Fat pointer.** `&[u8; 4]` 8 byte (thin), còn `&[u8]`, `&str`, `&dyn Shape`, `Box<dyn Shape>` đều 16 — hai lần `usize`, hiện trạng mà Reference dặn đừng dựa vào[^ref-ptr-unsized]. `size_of_val(&dyn Shape)` trả 8 cho `Square`, 24 cho `Circle`: size lấy từ vtable lúc chạy[^nomicon-vtsize]. Con trỏ dữ liệu của `&dyn` chính là địa chỉ giá trị cụ thể (`true`), phần còn lại là vtable.

### Lab (b) — `mir_demo`: đọc MIR và NLL

Lý thuyết: [chương 01 mục 4](../10-rust-internals/01-pipeline-rustc.md) (MIR), [chương 06 mục 2](../10-rust-internals/06-borrow-checker.md) (đọc dump MIR), [mục 3](../10-rust-internals/06-borrow-checker.md) (từ lexical lifetime đến NLL), [mục 5](../10-rust-internals/06-borrow-checker.md) (loan và xung đột).

Kiểm toolchain trước: trên máy này có stable (active), nightly, và các bản `1.88.0`…`1.95.0`:

```bash
rustup toolchain list
```

```text
stable-aarch64-apple-darwin (active, default)
nightly-aarch64-apple-darwin
nightly-2026-05-01-aarch64-apple-darwin
1.88.0-aarch64-apple-darwin
1.89-aarch64-apple-darwin
1.91-aarch64-apple-darwin
1.95.0-aarch64-apple-darwin
```

rustc book liệt kê `mir` trong các giá trị của `--emit`[^rustc-emit]. rustc-dev-guide có nói muốn xem MIR *chưa tối ưu* bằng `-Z mir-opt-level=0` thì cần nightly[^dev-mir-nightly]; nhưng riêng `--emit=mir` thì **chạy được trên stable** (đã thử):

```bash
cd labs
cargo rustc -p lab-internals --bin mir_demo -- --emit=mir   # exit 0
ls target/debug/deps/mir_demo*.mir
cargo run -q -p lab-internals --bin mir_demo
```

```text
target/debug/deps/mir_demo-85fc2c65533aa017.mir      # hash sẽ khác trên máy bạn
add_then_double(3, 4) = 14
nll_ok() = 4
```

MIR của `add_then_double` (build debug, trích nguyên văn từ file `.mir`):

```text
fn add_then_double(_1: i32, _2: i32) -> i32 {
    debug a => _1;
    debug b => _2;
    let mut _0: i32;
    ...
    bb0: {
        _4 = AddWithOverflow(copy _1, copy _2);
        assert(!move (_4.1: bool), "attempt to compute `{} + {}`, which would overflow", copy _1, copy _2) -> [success: bb1, unwind continue];
    }
    bb1: {
        _3 = move (_4.0: i32);
        _5 = MulWithOverflow(copy _3, const 2_i32);
        assert(!move (_5.1: bool), "attempt to compute `{} * {}`, which would overflow", copy _3, const 2_i32) -> [success: bb2, unwind continue];
    }
    bb2: {
        _0 = move (_5.0: i32);
        return;
    }
}
```

Cách đọc: `_0` là chỗ trả về, `_1`/`_2` là tham số, `_3` là biến `s`[^dev-mir-locals]. Mỗi block kết thúc bằng một terminator (`assert` có hai nhánh: success hoặc panic)[^dev-mir-bb]. Không có biểu thức lồng: `a + b` thành `AddWithOverflow` rồi `assert` — vì build debug bật kiểm tra tràn số (điều này người soạn thấy trong output; build release sẽ khác).

MIR của `nll_ok` (trích, bỏ phần cấp phát `vec![...]`):

```text
    bb2: {
        _4 = &_1;
        _3 = <Vec<i32> as Index<usize>>::index(move _4, const 0_usize) -> [return: bb3, unwind: bb7];
    }
    bb3: {
        _5 = copy (*_3);
        _7 = &mut _1;
        _6 = Vec::<i32>::push(move _7, copy _5) -> [return: bb4, unwind: bb7];
    }
    bb4: {
        _8 = &_1;
        _0 = Vec::<i32>::len(move _8) -> [return: bb5, unwind: bb7];
    }
```

`first` là `_3` (borrow `&_1` từ `index`). Lần dùng cuối của `_3` là `_5 = copy (*_3)` ở `bb3`, *trước* `_7 = &mut _1`. Vì NLL tính region từ CFG và liveness[^dev-nll][^dev-bc-live], borrow chia sẻ đã chết khi `&mut` bắt đầu → được chấp nhận. Chú ý `unwind: bb7`/`cleanup` — đường dọn dẹp cũng là một phần của CFG (`drop(_1)` ở `bb7`).

Giờ so với `snippets/nll_error.rs`, nơi `first` còn được dùng *sau* `push`:

```bash
cd labs && rustc --edition 2024 --crate-type lib crates/lab-internals/snippets/nll_error.rs -o /tmp/nll_error.rlib   # exit 1
```

```text
error[E0502]: cannot borrow `v` as mutable because it is also borrowed as immutable
 --> crates/lab-internals/snippets/nll_error.rs:7:5
  |
6 |     let first = &v[0];
  |                  - immutable borrow occurs here
7 |     v.push(4);
  |     ^^^^^^^^^ mutable borrow occurs here
8 |     *first
  |     ------ immutable borrow later used here
```

Đúng quy tắc "không mutate place khi đang bị mượn `&`"[^dev-bc-rules]: ở đây region của `first` phải sống đến dòng 8, bao trùm điểm `push`.

### Lab (c) — `mono`: đếm bản monomorphized

Lý thuyết: [chương 04 mục 1](../10-rust-internals/04-monomorphization.md) (monomorphization), [mục 2](../10-rust-internals/04-monomorphization.md) (static vs dynamic dispatch), [mục 6](../10-rust-internals/04-monomorphization.md) (code bloat), [mục 7](../10-rust-internals/04-monomorphization.md) (`#[inline]`, bài tập mở rộng 3), [chương 03 mục 4](../10-rust-internals/03-dst-fat-pointer-vtable.md) (vtable và lời gọi `dyn`).

`mono.rs` gọi `describe_generic<T: Debug>` với `u8`, `i64`, `&str`, `Vec<u16>`; và `describe_dyn(&dyn Debug)` với bốn kiểu tương tự; `total_area_static<S: Shape>` với `Square` và `Rect`; `total_area_dyn(&[&dyn Shape])` một lần. Mọi hàm là `#[inline(never)]` để symbol còn tồn tại.

```bash
cd labs
cargo run -q -p lab-internals --release --bin mono      # exit 0
nm target/release/mono | grep -E 'describe|total_area'
```

```text
generic: ["7", "-7", "\"seven\"", "[7]"]
dyn:     ["7", "-7", "\"seven\"", "[7]"]
static squares=5 rects=14  dyn mixed=16
0000000100000a94 t __ZN4mono12describe_dyn17hb6daf87eeee8254fE
0000000100000b38 t __ZN4mono16describe_generic17h290dca8371fa6a8bE
0000000100000b7c t __ZN4mono16describe_generic17h63242607dc56fa13E
0000000100000bc0 t __ZN4mono16describe_generic17h75c478f071585f2cE
0000000100000c44 t __ZN4mono16describe_generic17h8a2b0a64d7d7fab6E
0000000100000ad8 t __ZN4mono14total_area_dyn17h43417f03e1ce4a5cE
0000000100000c88 t __ZN4mono17total_area_static17h1b89d073c0fd79b5E
0000000100000d5c t __ZN4mono17total_area_static17h489323f7cdde090fE
```

Trong container Linux (`./labs/linux.sh bash -c 'cargo build -q -p lab-internals --release --bin mono && nm -C target-linux/release/mono | grep -E "describe|total_area"'`, exit 0), `nm -C` giải mã tên:

```text
mono::describe_dyn
mono::total_area_dyn
mono::describe_generic   (x4)
mono::total_area_static  (x2)
```

Kết luận (từ số đếm): `describe_generic` có **4** bản (một mỗi kiểu), `total_area_static` có **2** (`Square`, `Rect`), còn `describe_dyn` và `total_area_dyn` mỗi hàm **1** bản dù nhận nhiều kiểu — khớp mô tả "stamps out a copy per concrete type"[^dev-mono] và dynamic dispatch qua vtable[^ref-to-virtual]. Hậu tố `17h...E` là hash phân biệt các bản; địa chỉ và hash sẽ khác trên máy bạn. Lưu ý: số bản là quan sát của rustc 1.95.0 ở `--release` với `#[inline(never)]`; compiler có thể gộp hoặc inline các bản khi không chặn (người soạn không kiểm trường hợp đó).

### Lab (d) — `miri_ub`: UB cố ý, chỉ chạy dưới Miri

Lý thuyết: [chương 07 mục 2](../10-rust-internals/07-unsafe-ub-miri.md) (undefined behavior), [mục 4](../10-rust-internals/07-unsafe-ub-miri.md) (aliasing), [mục 5](../10-rust-internals/07-unsafe-ub-miri.md) (raw pointer, provenance), [mục 6](../10-rust-internals/07-unsafe-ub-miri.md) (`MaybeUninit`), [mục 7](../10-rust-internals/07-unsafe-ub-miri.md) (Miri chạy thế nào, không chứng minh được gì).

`miri_ub` là bin nhưng **từ chối chạy UB khi chạy native** (`!cfg!(miri)` → exit 2), nên `cargo build --workspace`/`clippy` vẫn sạch và không có chương trình UB nào thật sự chạy trên CPU. Cờ `cfg(miri)` do `cargo miri` bật[^miri-cfg]. Native:

```bash
(cd labs && cargo run -q -p lab-internals --bin miri_ub -- uaf)    # exit 2
```

```text
refusing to run UB case `uaf` natively; run it under Miri:
  cargo +nightly miri run -p lab-internals --bin miri_ub -- uaf
```

Miri có sẵn trên toolchain `nightly` của máy này (stable báo `the 'miri' component ... is not available for the 'stable-aarch64-apple-darwin' toolchain`). Phiên bản đã dùng:

```bash
rustc +nightly -V && cargo +nightly miri --version
```

```text
rustc 1.97.0-nightly (67bcaa9c4 2026-05-01)
miri 0.1.0 (67bcaa9c4b 2026-05-01)
```

Nếu máy bạn chưa có: `rustup toolchain install nightly --component miri` (hoặc `rustup +nightly component add miri`[^miri-run]). Kết quả sẽ phụ thuộc bản nightly — thông điệp lỗi của Miri không ổn định. Chạy từng case (`cargo +nightly miri run -q -p lab-internals --bin miri_ub -- <case>`, từ `labs/`):

```text
$ ... -- ok                          # exit 0
well-defined raw pointer use: 1

$ ... -- uaf                         # exit 1 (rút gọn)
error: Undefined Behavior: memory access failed: alloc808 has been freed, so this pointer is dangling
  --> crates/lab-internals/src/bin/miri_ub.rs:14:9
14 |         *p
   |         ^^ Undefined Behavior occurred here
help: alloc808 was allocated here:   (miri_ub.rs:10:37, Box::new(42))
help: alloc808 was deallocated here: (miri_ub.rs:13:9, drop(Box::from_raw(p)))

$ ... -- oob                         # exit 1 (rút gọn)
error: Undefined Behavior: memory access failed: attempting to access 4 bytes, but got alloc797+0xc which is at or beyond the end of the allocation of size 12 bytes
  --> crates/lab-internals/src/bin/miri_ub.rs:22:14
22 |     unsafe { p.add(3).read() }

$ ... -- alias                       # exit 1 (rút gọn)
error: Undefined Behavior: attempting a write access using <1766> at alloc801[0x0], but that tag does not exist in the borrow stack for this location
  --> crates/lab-internals/src/bin/miri_ub.rs:33:9
33 |         *a = 1;
   = help: ... the Stacked Borrows rules it violated are still experimental
help: <1766> was created by a Unique retag at offsets [0x0..0x4]   (miri_ub.rs:30:17, let a = &mut *raw;)
help: <1766> was later invalidated at offsets [0x0..0x4] by a Unique retag (miri_ub.rs:31:17, let b = &mut *raw;)

$ ... -- invalid-bool                # exit 1 (rút gọn)
error: Undefined Behavior: constructing invalid value of type bool: encountered 0x03, but expected a boolean
  --> crates/lab-internals/src/bin/miri_ub.rs:41:14
41 |     unsafe { (&raw const byte).cast::<bool>().read() }
```

Ánh xạ về danh sách UB của Reference:

| case | Lỗi | Mục Reference | Miri |
|---|---|---|---|
| `uaf` | đọc sau `drop(Box::from_raw(p))` | truy cập place dangling[^ref-ub-access] | bắt use-after-free[^miri-oob] |
| `oob` | `p.add(3).read()` trên mảng 3 phần tử | truy cập ngoài allocation (dangling: không nằm trong cùng allocation sống)[^ref-ub-dangling] | bắt truy cập ngoài biên[^miri-oob] |
| `alias` | hai `&mut` cùng trỏ vào `x`, dùng cái cũ | phá luật aliasing[^ref-ub-alias] | Stacked Borrows, experimental[^miri-sb] |
| `invalid-bool` | đọc byte `3` thành `bool` | `bool` phải là 0 hoặc 1[^ref-ub-bool] | bất biến kiểu[^miri-invariant] |
| `ok` | chỉ dùng raw pointer của biến sống, không có reference chồng | — | không báo gì (nhưng im lặng không chứng minh sound[^miri-limit]) |

Chú ý hai điều. (1) Miri phân biệt rõ mức chắc chắn: `uaf`/`oob`/`invalid-bool` là vi phạm đã được Reference nêu; `alias` dựa trên mô hình Stacked Borrows mà chính Miri gắn nhãn thử nghiệm[^miri-sb] và Reference nói luật chính xác chưa xác định[^ref-ub-alias-open]. (2) Nếu bạn chạy `uaf` native, kết quả "in ra gì đó" không chứng minh gì: chương trình có UB không có nghĩa xác định (nhận xét của người soạn, theo ý của Reference rằng UB là hành vi không được phép xảy ra[^ref-ub-intro]) — vì thế bin từ chối chạy.

### Bài tập mở rộng

1. Thêm `#[repr(u8)] enum` có payload vào `type_layout.rs`, dùng ví dụ `repr(u8)` của Reference để đoán size trước rồi đối chiếu[^ref-primitive-enum] (xem [chương 02 mục 4](../10-rust-internals/02-type-layout.md)).
2. Đoán rồi đo `size_of::<Option<Option<bool>>>()` và `Option<Option<&u8>>`. Đừng ghi nó vào tài liệu như một bảo đảm: std chỉ hứa danh sách trong [chương 02 mục 6](../10-rust-internals/02-type-layout.md).
3. Chạy `cargo rustc -p lab-internals --release --bin mono -- --emit=asm` và tìm lệnh gọi gián tiếp (`blr` trên aarch64 / `call *` trên x86_64) trong `total_area_dyn` so với `total_area_static` ([chương 03 mục 4](../10-rust-internals/03-dst-fat-pointer-vtable.md)); rồi bỏ `#[inline(never)]` và quan sát bản generic có bị inline không ([chương 04 mục 7](../10-rust-internals/04-monomorphization.md)).
4. Với `cargo rustc ... -- --emit=llvm-ir` (rustc book liệt kê `llvm-ir`[^rustc-emit]) đọc IR của `describe_dyn`. Tìm vtable constant.
5. Viết `MyBox<T>` với `PhantomData<*mut T>` và `PhantomData<fn(T)>`; thử gán `MyBox<&'static str>` cho `MyBox<&'a str>` để tự thấy variance đổi[^phantom][^variance-table] (xem [chương 05 mục 5](../10-rust-internals/05-variance-va-dropck.md)).
6. Thêm case Miri `uninit`: `MaybeUninit::<bool>::uninit().assume_init()` — dự đoán Miri sẽ nói gì dựa trên [^mu-bool] ([chương 07 mục 6](../10-rust-internals/07-unsafe-ub-miri.md)), chạy, và so sánh. Giữ nó trong `miri_ub` (không chạy native).
7. Chạy case `alias` với `MIRIFLAGS="-Zmiri-disable-stacked-borrows"` và với `MIRIFLAGS="-Zmiri-tree-borrows"`. Người soạn đã chạy cả hai (nightly ở trên): cờ đầu cho `alias read 1`, exit 0 — Miri không báo gì vì đã tắt kiểm tra aliasing, **nhưng chương trình vẫn vi phạm luật aliasing của Reference**[^ref-ub-alias]; cờ sau vẫn báo `Undefined Behavior: write access through <1712> ... is forbidden` (theo "Tree Borrows rules ... still experimental"). README liệt kê Tree Borrows như lựa chọn thay cho Stacked Borrows[^miri-tb]. Bài học: "Miri không báo" phụ thuộc vào model và cờ bạn chọn[^miri-limit].

## Câu hỏi tự kiểm tra (về output lab)

1. Vì sao đoạn `nll_ok` biên dịch được còn `nll_error` thì không (lab b)?
   <details><summary>Đáp án</summary>

   NLL tính region từ CFG; borrow sống tới lần dùng cuối[^dev-nll][^dev-bc-live]. Ở `nll_ok` lần dùng cuối trước `&mut`; ở `nll_error` `first` còn được dùng sau `push`, vi phạm quy tắc không mutate khi đang mượn `&`[^dev-bc-rules].

   </details>

2. Trong lab (c), vì sao `describe_generic` có 4 symbol còn `describe_dyn` chỉ có 1 dù cùng nhận bốn kiểu?
   <details><summary>Đáp án</summary>

   Monomorphization đóng dấu một bản cho mỗi kiểu cụ thể[^dev-mono]; hàm nhận `&dyn Debug` chỉ có một bản và gọi method qua vtable[^ref-to-virtual].

   </details>

3. Trong output lab (a), vì sao `Option<&u64>` bằng 8 byte còn `Option<*const u64>` là 16?
   <details><summary>Đáp án</summary>

   `&U` nằm trong danh sách kiểu mà std hứa `Option<T>` có cùng size, alignment và ABI với `T`[^opt-rep]; `*const u64` không nằm trong danh sách đó, và output đo được 16 byte (quan sát, không phải bảo đảm).

   </details>

4. Vì sao `miri_ub` từ chối chạy case `uaf` khi chạy native?
   <details><summary>Đáp án</summary>

   Chương trình có UB không có nghĩa xác định, nên việc nó "in ra gì đó" không chứng minh gì[^ref-ub-intro]; bin chỉ chạy các case UB khi `cfg(miri)` được bật[^miri-cfg], nếu không thì thoát với exit 2.

   </details>

[^ref-c-algo]: The Rust Reference, Type layout — <https://doc.rust-lang.org/1.95.0/reference/type-layout.html> — "If the current offset is not a multiple of the field’s alignment, then add padding bytes to the current offset until it is a multiple of the field’s alignment."
[^ref-c-size]: The Rust Reference, Type layout — <https://doc.rust-lang.org/1.95.0/reference/type-layout.html> — "Finally, the size of the struct is the current offset rounded up to the nearest multiple of the struct’s alignment."
[^ref-packed]: The Rust Reference, Type layout — <https://doc.rust-lang.org/1.95.0/reference/type-layout.html> — "a type with #[repr(packed(1))] (or #[repr(packed)]) will have no inter-field padding."
[^ref-rust-order]: The Rust Reference, Type layout — <https://doc.rust-lang.org/1.95.0/reference/type-layout.html> — "The ordering does not have to be the same as the order in which the fields are specified in the declaration of the type."
[^opt-rep]: Rust std, Option, Representation — <https://doc.rust-lang.org/1.95.0/std/option/index.html> — "Rust guarantees to optimize the following types T such that Option\<T> has the same size, alignment, and function call ABI as T."
[^mu-layout]: Rust std, MaybeUninit, Layout — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "MaybeUninit\<T> is guaranteed to have the same size, alignment, and ABI as T"
[^vec-zst]: Rust std, Vec — <https://doc.rust-lang.org/1.95.0/std/vec/struct.Vec.html> — "Note that in this case the Vec might not report a capacity of 0. Vec will allocate if and only if size_of::\<T>() * capacity() > 0."
[^ref-ptr-unsized]: The Rust Reference, Type layout — <https://doc.rust-lang.org/1.95.0/reference/type-layout.html> — "Pointers to unsized types are sized. The size and alignment is guaranteed to be at least equal to the size and alignment of a pointer."
[^nomicon-vtsize]: The Rustonomicon, Exotically Sized Types — <https://doc.rust-lang.org/1.95.0/nomicon/exotic-sizes.html> — "The runtime size of the pointee can be dynamically requested from the vtable."
[^rustc-emit]: The rustc book, Command-line arguments — <https://doc.rust-lang.org/1.95.0/rustc/command-line-arguments.html> — "mir — Generates a file containing rustc’s mid-level intermediate representation. The default output filename is CRATE_NAME.mir."
[^dev-mir-nightly]: rustc-dev-guide, The MIR — <https://rustc-dev-guide.rust-lang.org/mir/index.html> — "We can use rustc [filename].rs -Z mir-opt-level=0 --emit mir to view unoptimized MIR. This requires the nightly toolchain."
[^dev-mir-locals]: rustc-dev-guide, The MIR — <https://rustc-dev-guide.rust-lang.org/mir/index.html> — "These are identified by an index, written with a leading underscore, like _1. There is also a special “local” (_0) allocated to store the return value."
[^dev-mir-bb]: rustc-dev-guide, The MIR — <https://rustc-dev-guide.rust-lang.org/mir/index.html> — "statements: actions with one successor terminators: actions with potentially multiple successors; always at the end of a block"
[^dev-nll]: rustc-dev-guide, MIR borrow check — <https://rustc-dev-guide.rust-lang.org/borrow-check.html> — "using the MIR enables “non-lexical lifetimes”, which are regions derived from the control-flow graph."
[^dev-bc-live]: rustc-dev-guide, Region inference (NLL) — <https://rustc-dev-guide.rust-lang.org/borrow-check/region-inference.html> — "Liveness constraints. Each region needs to be live at points where it can be used."
[^dev-mono]: rustc-dev-guide, Monomorphization — <https://rustc-dev-guide.rust-lang.org/backend/monomorph.html> — "This means that compiler stamps out a different copy of the code of a generic function for each concrete type needed."
[^ref-to-virtual]: The Rust Reference, Trait object types — <https://doc.rust-lang.org/1.95.0/reference/types/trait-object.html> — "Calling a method on a trait object results in virtual dispatch at runtime: that is, a function pointer is loaded from the trait object vtable and invoked indirectly."
[^miri-cfg]: Miri README, Using Miri — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "When compiling code via cargo miri, the cfg(miri) config flag is set for code that will be interpreted under Miri."
[^miri-run]: Miri README, Using Miri — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "If you have a binary project, you can run it through Miri using cargo miri run."
[^ref-ub-access]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "Accessing (loading from or storing to) a place that is dangling or based on a misaligned pointer."
[^miri-oob]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "Out-of-bounds memory accesses and use-after-free"
[^ref-ub-dangling]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "A reference/pointer is “dangling” if not all of the bytes it points to are part of the same live allocation (so in particular they all have to be part of some allocation)."
[^ref-ub-alias]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "&T must point to memory that is not mutated while they are live (except for data inside an UnsafeCell\<U>), and &mut T must point to memory that is not read or written by any pointer not derived from the reference and that no other reference points to while they are live."
[^miri-sb]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "Experimental: Violations of the Stacked Borrows rules governing aliasing for reference types"
[^ref-ub-bool]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "A bool value must be false (0) or true (1)."
[^miri-invariant]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "Violation of basic type invariants (a bool that is not 0 or 1, for example, or an invalid enum discriminant)"
[^miri-limit]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "When Miri finds UB, your code is definitely unsound, but when Miri does not find UB, then you may just have to test more inputs or more possible non-deterministic choices."
[^ref-ub-alias-open]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "The exact aliasing rules are not determined yet, but here is an outline of the general principles"
[^ref-ub-intro]: The Rust Reference, Behavior considered undefined — <https://doc.rust-lang.org/1.95.0/reference/behavior-considered-undefined.html> — "unsafe only means that avoiding undefined behavior is on the programmer; it does not change anything about the fact that Rust programs must never cause undefined behavior."
[^ref-primitive-enum]: The Rust Reference, Type layout — <https://doc.rust-lang.org/1.95.0/reference/type-layout.html> — "For field-less enums, primitive representations set the size and alignment to be the same as the primitive type of the same name."
[^phantom]: The Rustonomicon, PhantomData — <https://doc.rust-lang.org/1.95.0/nomicon/phantom-data.html> — "PhantomData consumes no space, but simulates a field of the given type for the purpose of static analysis."
[^variance-table]: The Rustonomicon, Subtyping and Variance — <https://doc.rust-lang.org/1.95.0/nomicon/subtyping.html> — "Box\<T> covariant Vec\<T> covariant UnsafeCell\<T> invariant Cell\<T> invariant"
[^mu-bool]: Rust std, MaybeUninit — <https://doc.rust-lang.org/1.95.0/std/mem/union.MaybeUninit.html> — "Hence, creating an uninitialized bool is undefined behavior"
[^miri-tb]: Miri README — <https://raw.githubusercontent.com/rust-lang/miri/0923e3ee8eccff5912b668cbccf3bc464ff3c0e1/README.md> — "Violations of the Tree Borrows aliasing rules, as an optional alternative to Stacked Borrows"
[^dev-bc-rules]: rustc-dev-guide, MIR borrow check — <https://rustc-dev-guide.rust-lang.org/borrow-check.html> — "That you can’t move a value while it is borrowed. That you can’t access a place while it is mutably borrowed (except through the reference). That you can’t mutate a place while it is immutably borrowed."
