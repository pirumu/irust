//! Lab 6: endianness và biểu diễn số nguyên/số thực trong bộ nhớ.
//!
//!   cargo run -q -p lab-cpu --bin endian
//!
//! Chỉ dùng std; không có phần đo thời gian nên chạy debug hay release đều được.

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn main() {
    println!(
        "target_arch={} target_endian={}",
        std::env::consts::ARCH,
        if cfg!(target_endian = "little") {
            "little"
        } else {
            "big"
        }
    );

    println!("\n== u32 0x12345678: byte theo thứ tự địa chỉ thấp -> cao ==");
    let x: u32 = 0x1234_5678;
    println!("to_le_bytes: {}", hex(&x.to_le_bytes()));
    println!("to_be_bytes: {}", hex(&x.to_be_bytes()));
    println!("to_ne_bytes: {}", hex(&x.to_ne_bytes()));
    // Đọc chính vùng nhớ của biến `x` từng byte, không qua `to_*_bytes`.
    // SAFETY: `x` là một u32 hợp lệ, dài size_of::<u32>() byte; u8 có alignment 1 và mọi
    // mẫu bit đều hợp lệ; slice chỉ sống trong khối này.
    let raw = unsafe { std::slice::from_raw_parts(&raw const x as *const u8, size_of::<u32>()) };
    println!("bộ nhớ thật của x: {}", hex(raw));

    println!("\n== Giải mã trường 16 bit trên dây: port 80 = 00 50 ==");
    let wire = [0x00_u8, 0x50];
    println!(
        "from_be_bytes  = {:#06x} ({})",
        u16::from_be_bytes(wire),
        u16::from_be_bytes(wire)
    );
    println!(
        "from_le_bytes  = {:#06x} ({})",
        u16::from_le_bytes(wire),
        u16::from_le_bytes(wire)
    );
    println!(
        "from_ne_bytes  = {:#06x} ({})",
        u16::from_ne_bytes(wire),
        u16::from_ne_bytes(wire)
    );

    println!("\n== Cùng một dãy bit, hai cách diễn giải ==");
    for v in [-1_i8, i8::MIN, i8::MAX] {
        println!("{v:>5}i8 as u8 = {:>3} = {:#010b}", v as u8, v as u8);
    }
    println!("-1i32 as u32 = {:#x}", -1_i32 as u32);
    println!("200u8 as i8 = {}", 200_u8 as i8);
    println!("i8::MIN.wrapping_neg() = {}", i8::MIN.wrapping_neg());
    println!("i8::MIN.checked_neg()  = {:?}", i8::MIN.checked_neg());

    println!("\n== f64 trong bộ nhớ ==");
    for v in [1.0_f64, -2.0, 0.1, 0.0, -0.0] {
        println!(
            "{v:>5} bits = {:#018x}  be bytes = {}",
            v.to_bits(),
            hex(&v.to_be_bytes())
        );
    }
    println!("0.1 + 0.2 == 0.3 ? {}", 0.1_f64 + 0.2 == 0.3);
    // `black_box` để compiler/clippy không coi hai vế là cùng một biểu thức hằng.
    let nan = std::hint::black_box(f64::NAN);
    println!("NaN == NaN ? {}", nan == std::hint::black_box(f64::NAN));
    println!("0.0 == -0.0 ? {}", 0.0_f64 == -0.0);
}
