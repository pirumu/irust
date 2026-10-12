//! Lab (e): a program to stand in front of with `gdb`, `lldb`, `rust-gdb` and `rust-lldb`
//! (chapter 04). It has what a debugger has to understand in Rust: a struct that owns a `Vec`
//! and a `String`, an enum with payloads, tuple structs, a trait object, a closure that captures
//! its environment, an `#[inline(always)]` function and a local that is dead right after it is used.
//!
//! `checkpoint` is `#[inline(never)]` and prints the values its arguments hold, so the output
//! of the program is the ground truth to compare `info args` / `print` / `frame variable` with.
//!
//!   cargo run -q -p lab-observability --bin debug_target

use std::hint::black_box;

struct Item {
    sku: &'static str,
    qty: u32,
    unit_cents: u64,
}

enum Status {
    New,
    Paid { txn: u64 },
    Shipped { carrier: String, tracking: u32 },
    Cancelled(String),
}

struct Order {
    id: u64,
    customer: String,
    items: Vec<Item>,
    status: Status,
    coupon: Option<u8>,
}

/// A tuple struct: DWARF does not distinguish it from a struct, its fields are named `__0`, `__1`.
struct Pair(u32, u32);

trait Pricing {
    fn name(&self) -> &'static str;
    fn total(&self, order: &Order) -> u64;
}

struct Flat {
    tax_pct: u64,
}

struct Tiered {
    threshold_cents: u64,
    discount_pct: u64,
}

/// `#[inline(always)]` asks the compiler to copy the body into every caller. In a build with
/// optimization and debuginfo it then shows up in a debugger as an *inlined frame* of `total`,
/// not as a call.
#[inline(always)]
fn percent(amount: u64, pct: u64) -> u64 {
    amount * pct / 100
}

#[inline(never)]
fn subtotal(order: &Order) -> u64 {
    let mut sum = 0;
    for item in &order.items {
        sum += u64::from(item.qty) * item.unit_cents;
    }
    sum
}

impl Pricing for Flat {
    fn name(&self) -> &'static str {
        "flat"
    }

    #[inline(never)]
    fn total(&self, order: &Order) -> u64 {
        let base = subtotal(order);
        base + percent(base, self.tax_pct)
    }
}

impl Pricing for Tiered {
    fn name(&self) -> &'static str {
        "tiered"
    }

    #[inline(never)]
    fn total(&self, order: &Order) -> u64 {
        let base = subtotal(order);
        if base >= self.threshold_cents { base - percent(base, self.discount_pct) } else { base }
    }
}

fn describe(status: &Status) -> String {
    match status {
        Status::New => "new".to_string(),
        Status::Paid { txn } => format!("paid(txn {txn})"),
        Status::Shipped { carrier, tracking } => format!("shipped({carrier} #{tracking})"),
        Status::Cancelled(reason) => format!("cancelled({reason})"),
    }
}

/// Set a breakpoint here: `break debug_target::checkpoint`.
#[inline(never)]
fn checkpoint(stage: &str, order: &Order, running_cents: u64, pricing: &dyn Pricing, pair: &Pair) -> u64 {
    let skus: Vec<&str> = order.items.iter().map(|i| i.sku).collect();
    println!(
        "checkpoint {stage}: order=#{} customer={:?} items={skus:?} status={} coupon={:?} running={running_cents} pricing={} pair=({}, {})",
        order.id,
        order.customer,
        describe(&order.status),
        order.coupon,
        pricing.name(),
        pair.0,
        pair.1
    );
    black_box(running_cents) + u64::from(pair.0 + pair.1)
}

fn build_orders() -> Vec<Order> {
    vec![
        Order {
            id: 1001,
            customer: "mai".to_string(),
            items: vec![Item { sku: "sku-ssd", qty: 2, unit_cents: 5_499 }, Item { sku: "sku-cable", qty: 5, unit_cents: 349 }],
            status: Status::Paid { txn: 90_210 },
            coupon: Some(7),
        },
        Order {
            id: 1002,
            customer: "bao".to_string(),
            items: vec![Item { sku: "sku-ram", qty: 1, unit_cents: 12_999 }],
            status: Status::Shipped { carrier: "vnpost".to_string(), tracking: 77_001 },
            coupon: None,
        },
        Order { id: 1003, customer: "linh".to_string(), items: Vec::new(), status: Status::Cancelled("out of stock".to_string()), coupon: Some(0) },
        Order {
            id: 1004,
            customer: "khanh".to_string(),
            items: vec![Item { sku: "sku-usb", qty: 3, unit_cents: 1_250 }],
            status: Status::New,
            coupon: None,
        },
    ]
}

fn main() {
    let orders = build_orders();
    let flat = Flat { tax_pct: 8 };
    let tiered = Tiered { threshold_cents: 10_000, discount_pct: 10 };
    let pricers: [&dyn Pricing; 2] = [&flat, &tiered];

    let mut running = 0u64;
    let mut seen = 0u32;
    // A closure that captures `seen` and `running` by mutable reference.
    let mut visit = |order: &Order, pricing: &dyn Pricing, pair: &Pair| {
        seen += 1;
        running += pricing.total(order);
        checkpoint("visit", order, running, pricing, pair)
    };

    let mut last = 0;
    for (i, order) in orders.iter().enumerate() {
        let pricing = pricers[i % 2];
        let pair = Pair(i as u32, order.items.len() as u32);
        let items_x3 = order.items.len() * 3; // dead right after `black_box`: `<optimized out>` in a release build
        black_box(items_x3);
        last = visit(order, pricing, &pair);
    }
    println!("done: seen={seen} total_cents={running} last={last}");
}
