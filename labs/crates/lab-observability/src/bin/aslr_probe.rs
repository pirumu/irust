//! Lab (g, part 2): where does the kernel put this process's code, data, heap, stack and an
//! anonymous mapping, and does it put them somewhere else on the next run? (chapter 06 §6)
//!
//!   aslr_probe            # run it twice and compare
//!
//! Each line is the address of one object: the code of `main`, a `static`, a function of libc, a
//! `Box` on the heap, a fresh anonymous `mmap` and a local variable on the stack.

use std::ptr;

static A_STATIC: u64 = 0x5a5a_5a5a_5a5a_5a5a;

fn main() {
    let on_stack = 0u64;
    let on_heap = Box::new(0u64);
    // SAFETY: a plain anonymous private mapping of one page, unmapped again below.
    let page = unsafe {
        libc::mmap(ptr::null_mut(), 4096, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_PRIVATE | libc::MAP_ANON, -1, 0)
    };
    assert_ne!(page, libc::MAP_FAILED, "mmap failed: {}", std::io::Error::last_os_error());

    println!("pid                       {}", std::process::id());
    println!("code    fn main           {:#018x}", main as *const () as usize);
    println!("data    a static          {:#018x}", &raw const A_STATIC as usize);
    println!("libc    getpid            {:#018x}", libc::getpid as *const () as usize);
    println!("heap    Box<u64>          {:#018x}", &raw const *on_heap as usize);
    println!("mmap    anonymous page    {:#018x}", page as usize);
    println!("stack   local variable    {:#018x}", &raw const on_stack as usize);
    match std::fs::read_to_string("/proc/sys/kernel/randomize_va_space") {
        Ok(v) => println!("kernel.randomize_va_space = {}", v.trim()),
        Err(_) => println!("kernel.randomize_va_space = n/a (no /proc/sys/kernel/randomize_va_space: not Linux)"),
    }

    // SAFETY: `page` came from the successful mmap above and is not used afterwards.
    unsafe { libc::munmap(page, 4096) };
}
