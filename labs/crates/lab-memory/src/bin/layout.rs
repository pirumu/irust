//! Lab (a): addresses of code, static data, stack and heap, matched against /proc/self/maps.
//!
//! Run on Linux: ./labs/linux.sh cargo run -p lab-memory --bin layout

use std::hint::black_box;

static GREETING: &str = "xin chao"; // the &str itself lives in static memory
static mut COUNTER: u64 = 0; // writable static: .data/.bss

fn main() {
    let on_stack: u64 = 42;
    let small_box = Box::new(7_u64);
    // 1 MiB is far above glibc's default M_MMAP_THRESHOLD (128 KiB, mallopt(3)),
    // so glibc serves it with its own mmap rather than from [heap].
    let big_vec: Vec<u8> = vec![1; 1 << 20];

    let addrs: [(&str, usize); 7] = [
        ("code   fn main", main as fn() as usize),
        ("rodata \"xin chao\" bytes", GREETING.as_ptr() as usize),
        ("static GREETING (the &str)", &raw const GREETING as usize),
        ("static mut COUNTER", &raw const COUNTER as usize),
        ("stack  local on_stack", &raw const on_stack as usize),
        ("heap   Box<u64> (8 B)", &raw const *small_box as usize),
        ("heap   Vec<u8> 1 MiB buffer", big_vec.as_ptr() as usize),
    ];

    println!("{:<30} {:>18}", "what", "address");
    for (what, addr) in addrs {
        println!("{what:<30} {addr:#18x}");
    }

    #[cfg(target_os = "linux")]
    {
        let maps = match lab_memory::read_maps() {
            Ok(m) => m,
            Err(e) => {
                eprintln!("cannot read /proc/self/maps: {e}");
                std::process::exit(1);
            }
        };
        println!("\nregion in /proc/self/maps for each address:");
        for (what, addr) in addrs {
            match lab_memory::region_of(&maps, addr) {
                Some(entry) => println!("{what:<30} -> {}", entry.line),
                None => println!("{what:<30} -> (not mapped?)"),
            }
        }
        println!("\nfull /proc/self/maps:");
        for entry in &maps {
            println!("{}", entry.line);
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        println!();
        lab_memory::linux_only("layout");
    }

    // Keep the values alive (and observable) until after the maps were printed.
    black_box((&on_stack, &small_box, &big_vec));
}
