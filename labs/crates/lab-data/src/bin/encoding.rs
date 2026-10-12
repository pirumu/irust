//! Lab (d): varints and tags by hand, then rolling-upgrade compatibility and what breaks it.
//!
//! Run: cargo run -q -p lab-data --bin encoding

use lab_data::codec::{
    self, DecodeError, Field, IntDecl, OrderV1, OrderV2, OrderV3ReusedTag3, OrderV3RetypedTag3, Value, WireType,
};
use lab_data::util::hex;

fn show_field(f: &Field) -> String {
    match &f.value {
        Value::Varint(v) => format!("{}:VARINT({v})", f.number),
        Value::I64(v) => format!("{}:I64({v})", f.number),
        Value::I32(v) => format!("{}:I32({v})", f.number),
        Value::Len(b) => match std::str::from_utf8(b) {
            Ok(s) => format!("{}:LEN({s:?})", f.number),
            Err(_) => format!("{}:LEN({} bytes)", f.number, b.len()),
        },
    }
}

fn show_unknown(unknown: &[Field]) -> String {
    format!("[{}]", unknown.iter().map(show_field).collect::<Vec<_>>().join(", "))
}

fn varint_bytes(v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    codec::put_varint(&mut out, v);
    out
}

fn main() {
    println!("== 1. varint: 7 payload bits per byte, least significant group first, high bit = \"more bytes follow\" ==");
    println!("{:<22} {:<32} {:>3}", "value", "bytes", "len");
    let rows: [(&str, u64); 11] = [
        ("0", 0),
        ("1", 1),
        ("127", 127),
        ("128", 128),
        ("150", 150),
        ("300", 300),
        ("16384 (2^14)", 1 << 14),
        ("u32::MAX", u64::from(u32::MAX)),
        ("u64::MAX", u64::MAX),
        ("-1 written as int64", -1i64 as u64),
        ("zigzag(-1) = sint64 -1", codec::zigzag_encode(-1)),
    ];
    for (name, v) in rows {
        let b = varint_bytes(v);
        println!("{name:<22} {:<32} {:>3}", hex(&b), b.len());
    }
    println!("zigzag: n -> 2n for n >= 0, -n -> 2n - 1:");
    for n in [0i64, -1, 1, -2, 2, -64, 63, i64::from(i32::MIN)] {
        println!("  {n:>12} -> {:>10}", codec::zigzag_encode(n));
    }

    println!("\n== 2. tag = (field_number << 3) | wire_type, itself a varint ==");
    for (field, wire) in [(1, WireType::Varint), (2, WireType::Len), (15, WireType::Varint), (16, WireType::Varint), (2047, WireType::Varint), (2048, WireType::Varint)] {
        let t = codec::tag(field, wire);
        println!("field {field:>4} {:<7} tag={t:>6}  bytes: {}", format!("{wire:?}"), hex(&varint_bytes(t)));
    }

    println!("\n== 3. the two worked examples of the Protocol Buffers encoding guide ==");
    let mut a = Vec::new();
    Field::varint(1, 150).encode_into(&mut a);
    let mut b = Vec::new();
    Field::string(2, "testing").encode_into(&mut b);
    println!("{{ a = 150 }}        field 1, VARINT: {}", hex(&a));
    println!("{{ b = \"testing\" }}  field 2, LEN    : {}", hex(&b));
    println!("decode_fields(second) = {:?}", codec::decode_fields(&b).unwrap().iter().map(show_field).collect::<Vec<_>>());

    println!("\n== 4. rolling upgrade: v1 and v2 code reading each other's bytes ==");
    println!("v1: Order {{ uint64 id = 1; string customer = 2; string legacy_note = 3; }}");
    println!("v2: Order {{ reserved 3; uint64 id = 1; string customer = 2; string currency = 4; int64 amount_cents = 5; }}");
    let v1 = OrderV1 { id: 150, customer: "testing".into(), legacy_note: "gift wrap".into(), unknown: vec![] };
    let v2 = OrderV2 { id: 150, customer: "testing".into(), currency: "USD".into(), amount_cents: 1990, unknown: vec![] };
    let old_bytes = v1.encode();
    let new_bytes = v2.encode();
    println!("\nwritten by v1 ({} bytes): {}", old_bytes.len(), hex(&old_bytes));
    let r = OrderV2::decode(&old_bytes).unwrap();
    println!(
        "  read by v2 (backward compatible): id={} customer={:?} currency={:?} amount_cents={} unknown={}",
        r.id, r.customer, r.currency, r.amount_cents, show_unknown(&r.unknown)
    );
    println!("\nwritten by v2 ({} bytes): {}", new_bytes.len(), hex(&new_bytes));
    let r = OrderV1::decode(&new_bytes).unwrap();
    println!(
        "  read by v1 (forward compatible):  id={} customer={:?} legacy_note={:?} unknown={}",
        r.id, r.customer, r.legacy_note, show_unknown(&r.unknown)
    );

    println!("\nthe v1 service reads a v2 message, edits `customer`, writes it back:");
    let mut edited = OrderV1::decode(&new_bytes).unwrap();
    edited.customer = "renamed".into();
    let kept = OrderV2::decode(&edited.encode()).unwrap();
    println!("  unknown fields preserved : v2 then reads customer={:?} currency={:?} amount_cents={}", kept.customer, kept.currency, kept.amount_cents);
    let lost = OrderV2::decode(&edited.without_unknown().encode()).unwrap();
    println!("  copied field by field    : v2 then reads customer={:?} currency={:?} amount_cents={}   <- new fields silently gone", lost.customer, lost.currency, lost.amount_cents);

    println!("\ntruncated v2 message (cut after n bytes):");
    for n in 0..=new_bytes.len() {
        match OrderV2::decode(&new_bytes[..n]) {
            Ok(o) => println!("  n={n:>2}: decodes, id={} customer={:?} currency={:?} amount_cents={}", o.id, o.customer, o.currency, o.amount_cents),
            Err(e) => println!("  n={n:>2}: error: {e}"),
        }
    }

    println!("\n== 5. changes that break ==");
    println!("(a) number 3 reused as `uint32 retries` (another wire type), reading v1 bytes:");
    match OrderV3RetypedTag3::decode(&old_bytes) {
        Ok(o) => println!("    decoded {o:?}"),
        Err(e @ DecodeError::WireTypeMismatch { .. }) => println!("    error: {e}"),
        Err(e) => println!("    other error: {e}"),
    }
    println!("(b) number 3 reused as `string coupon_code` (same wire type), reading v1 bytes:");
    let o = OrderV3ReusedTag3::decode(&old_bytes).unwrap();
    println!("    no error: coupon_code = {:?}   <- the old free-text note, now treated as a coupon", o.coupon_code);
    println!("(c) number 5 declared int64 by the writer, read under other declarations:");
    println!("    {:>14} {:>12} {:>20} {:>12}", "written value", "as int64", "as sint64", "as int32");
    for value in [150i64, -1, -5, 5_000_000_000] {
        let wire = value as u64;
        println!(
            "    {value:>14} {:>12} {:>20} {:>12}",
            IntDecl::Int64.read(wire),
            IntDecl::Sint64.read(wire),
            IntDecl::Int32.read(wire)
        );
    }

    println!("\n== 6. names are not on the wire ==");
    let json = codec::order_v2_as_json(&v2);
    println!("JSON  ({} bytes): {json}", json.len());
    println!("v2 binary ({} bytes): {}", new_bytes.len(), hex(&new_bytes));
}
