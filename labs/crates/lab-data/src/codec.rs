//! Lab (d): a hand-written tagged binary encoding, and what schema evolution does to it.
//!
//! The wire format is the one Protocol Buffers documents: a message is a sequence of *records*;
//! each record starts with a varint *tag* `(field_number << 3) | wire_type`, followed by a payload
//! whose size the wire type determines (a varint, 4 or 8 fixed bytes, or a length-prefixed byte
//! string). Names and declared types are not on the wire; they exist only in the code that reads and
//! writes the message. That is what makes evolution possible (a reader skips records it does not know,
//! because the wire type tells it how long they are) and what makes some changes silently wrong
//! (nothing on the wire says which declaration wrote a record).
//!
//! Only the four wire types that matter here are implemented; the deprecated group types are rejected.

use std::fmt;

pub type Result<T, E = DecodeError> = std::result::Result<T, E>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The buffer ends inside a record.
    Truncated,
    /// More than ten bytes with the continuation bit set, or a tenth byte above 1.
    VarintOverflow,
    /// Field number 0 or above 2^29 - 1.
    BadFieldNumber(u64),
    /// Wire types 3 and 4 (groups, deprecated) and 6, 7 (unassigned).
    UnsupportedWireType(u8),
    /// The record exists but has another wire type than the schema being used declares.
    WireTypeMismatch { field: u32, expected: WireType, found: WireType },
    InvalidUtf8 { field: u32 },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Truncated => write!(f, "buffer ends inside a record"),
            DecodeError::VarintOverflow => write!(f, "varint longer than 10 bytes or above 64 bits"),
            DecodeError::BadFieldNumber(n) => write!(f, "field number {n} out of range"),
            DecodeError::UnsupportedWireType(w) => write!(f, "unsupported wire type {w}"),
            DecodeError::WireTypeMismatch { field, expected, found } => {
                write!(f, "field {field}: schema says {expected:?} but the record is {found:?}")
            }
            DecodeError::InvalidUtf8 { field } => write!(f, "field {field}: not valid UTF-8"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Appends `v` as a varint: 7 bits per byte, least significant group first, high bit set on every
/// byte except the last. 1 to 10 bytes.
pub fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8 & 0x7F) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Reads a varint at `*pos` and advances it.
pub fn get_varint(buf: &[u8], pos: &mut usize) -> Result<u64> {
    let mut v = 0u64;
    for i in 0..10 {
        let byte = *buf.get(*pos).ok_or(DecodeError::Truncated)?;
        *pos += 1;
        if i == 9 && byte > 1 {
            return Err(DecodeError::VarintOverflow); // would not fit in 64 bits
        }
        v |= u64::from(byte & 0x7F) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err(DecodeError::VarintOverflow)
}

/// Bytes `v` occupies as a varint.
pub fn varint_len(v: u64) -> usize {
    let mut out = Vec::with_capacity(10);
    put_varint(&mut out, v);
    out.len()
}

/// ZigZag: small magnitudes of either sign become small unsigned numbers
/// (0, -1, 1, -2, 2 ... map to 0, 1, 2, 3, 4 ...); `p` becomes `2p`, `n < 0` becomes `2|n| - 1`.
pub fn zigzag_encode(n: i64) -> u64 {
    ((n << 1) ^ (n >> 63)) as u64
}

pub fn zigzag_decode(u: u64) -> i64 {
    ((u >> 1) as i64) ^ -((u & 1) as i64)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireType {
    Varint = 0,
    I64 = 1,
    Len = 2,
    I32 = 5,
}

impl WireType {
    fn from_bits(bits: u8) -> Result<WireType> {
        match bits {
            0 => Ok(WireType::Varint),
            1 => Ok(WireType::I64),
            2 => Ok(WireType::Len),
            5 => Ok(WireType::I32),
            other => Err(DecodeError::UnsupportedWireType(other)),
        }
    }
}

pub const MAX_FIELD_NUMBER: u32 = (1 << 29) - 1;

/// The tag of a record: `(field_number << 3) | wire_type`.
pub fn tag(field: u32, wire: WireType) -> u64 {
    (u64::from(field) << 3) | wire as u64
}

/// One record's payload, interpreted by wire type only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Varint(u64),
    I64(u64),
    Len(Vec<u8>),
    I32(u32),
}

impl Value {
    pub fn wire_type(&self) -> WireType {
        match self {
            Value::Varint(_) => WireType::Varint,
            Value::I64(_) => WireType::I64,
            Value::Len(_) => WireType::Len,
            Value::I32(_) => WireType::I32,
        }
    }
}

/// A record whose meaning is not known: field number and payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub number: u32,
    pub value: Value,
}

impl Field {
    pub fn varint(number: u32, v: u64) -> Field {
        Field { number, value: Value::Varint(v) }
    }

    pub fn string(number: u32, s: &str) -> Field {
        Field { number, value: Value::Len(s.as_bytes().to_vec()) }
    }

    pub fn encode_into(&self, out: &mut Vec<u8>) {
        put_varint(out, tag(self.number, self.value.wire_type()));
        match &self.value {
            Value::Varint(v) => put_varint(out, *v),
            Value::I64(v) => out.extend_from_slice(&v.to_le_bytes()),
            Value::Len(bytes) => {
                put_varint(out, bytes.len() as u64);
                out.extend_from_slice(bytes);
            }
            Value::I32(v) => out.extend_from_slice(&v.to_le_bytes()),
        }
    }
}

/// Splits a message into its records without knowing any schema. This is all a reader needs in order
/// to skip a field it does not understand.
pub fn decode_fields(buf: &[u8]) -> Result<Vec<Field>> {
    let mut fields = Vec::new();
    let mut pos = 0;
    while pos < buf.len() {
        let t = get_varint(buf, &mut pos)?;
        let number = t >> 3;
        if number == 0 || number > u64::from(MAX_FIELD_NUMBER) {
            return Err(DecodeError::BadFieldNumber(number));
        }
        let wire = WireType::from_bits((t & 7) as u8)?;
        let value = match wire {
            WireType::Varint => Value::Varint(get_varint(buf, &mut pos)?),
            WireType::I64 => Value::I64(u64::from_le_bytes(take(buf, &mut pos, 8)?.try_into().expect("8 bytes"))),
            WireType::I32 => Value::I32(u32::from_le_bytes(take(buf, &mut pos, 4)?.try_into().expect("4 bytes"))),
            WireType::Len => {
                let len = get_varint(buf, &mut pos)?;
                let len = usize::try_from(len).map_err(|_| DecodeError::Truncated)?;
                Value::Len(take(buf, &mut pos, len)?.to_vec())
            }
        };
        fields.push(Field { number: number as u32, value });
    }
    Ok(fields)
}

fn take<'a>(buf: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8]> {
    let end = pos.checked_add(n).filter(|&e| e <= buf.len()).ok_or(DecodeError::Truncated)?;
    let s = &buf[*pos..end];
    *pos = end;
    Ok(s)
}

fn expect_varint(f: &Field) -> Result<u64> {
    match f.value {
        Value::Varint(v) => Ok(v),
        ref other => Err(DecodeError::WireTypeMismatch { field: f.number, expected: WireType::Varint, found: other.wire_type() }),
    }
}

fn expect_string(f: &Field) -> Result<String> {
    match &f.value {
        Value::Len(bytes) => String::from_utf8(bytes.clone()).map_err(|_| DecodeError::InvalidUtf8 { field: f.number }),
        other => Err(DecodeError::WireTypeMismatch { field: f.number, expected: WireType::Len, found: other.wire_type() }),
    }
}

/// Schema version 1:
///
/// ```text
/// message Order { uint64 id = 1; string customer = 2; string legacy_note = 3; }
/// ```
///
/// Absent fields decode to their default (0 / empty); fields with other numbers are kept in
/// `unknown` and written back by [`OrderV1::encode`], the way protobuf libraries preserve them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OrderV1 {
    pub id: u64,
    pub customer: String,
    pub legacy_note: String,
    pub unknown: Vec<Field>,
}

impl OrderV1 {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if self.id != 0 {
            Field::varint(1, self.id).encode_into(&mut out);
        }
        if !self.customer.is_empty() {
            Field::string(2, &self.customer).encode_into(&mut out);
        }
        if !self.legacy_note.is_empty() {
            Field::string(3, &self.legacy_note).encode_into(&mut out);
        }
        for f in &self.unknown {
            f.encode_into(&mut out);
        }
        out
    }

    pub fn decode(buf: &[u8]) -> Result<OrderV1> {
        let mut o = OrderV1::default();
        for f in decode_fields(buf)? {
            match f.number {
                1 => o.id = expect_varint(&f)?,
                2 => o.customer = expect_string(&f)?,
                3 => o.legacy_note = expect_string(&f)?,
                _ => o.unknown.push(f),
            }
        }
        Ok(o)
    }

    /// What a service does when it copies the fields it knows into a fresh message: unknown
    /// records are lost.
    pub fn without_unknown(&self) -> OrderV1 {
        OrderV1 { unknown: Vec::new(), ..self.clone() }
    }
}

/// Schema version 2: `legacy_note` removed (number 3 stays reserved forever), two fields added.
///
/// ```text
/// message Order { reserved 3; uint64 id = 1; string customer = 2; string currency = 4; int64 amount_cents = 5; }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OrderV2 {
    pub id: u64,
    pub customer: String,
    pub currency: String,
    pub amount_cents: i64,
    pub unknown: Vec<Field>,
}

impl OrderV2 {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if self.id != 0 {
            Field::varint(1, self.id).encode_into(&mut out);
        }
        if !self.customer.is_empty() {
            Field::string(2, &self.customer).encode_into(&mut out);
        }
        if !self.currency.is_empty() {
            Field::string(4, &self.currency).encode_into(&mut out);
        }
        if self.amount_cents != 0 {
            // int64: negative numbers are written as their two's complement, ten bytes long.
            Field::varint(5, self.amount_cents as u64).encode_into(&mut out);
        }
        for f in &self.unknown {
            f.encode_into(&mut out);
        }
        out
    }

    pub fn decode(buf: &[u8]) -> Result<OrderV2> {
        let mut o = OrderV2::default();
        for f in decode_fields(buf)? {
            match f.number {
                1 => o.id = expect_varint(&f)?,
                2 => o.customer = expect_string(&f)?,
                4 => o.currency = expect_string(&f)?,
                5 => o.amount_cents = expect_varint(&f)? as i64,
                _ => o.unknown.push(f), // includes number 3: v1 data, no longer declared
            }
        }
        Ok(o)
    }
}

/// A *wrong* version 3: the retired number 3 is reused for a `uint32 retries`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OrderV3RetypedTag3 {
    pub id: u64,
    pub retries: u32,
}

impl OrderV3RetypedTag3 {
    pub fn decode(buf: &[u8]) -> Result<OrderV3RetypedTag3> {
        let mut o = OrderV3RetypedTag3::default();
        for f in decode_fields(buf)? {
            match f.number {
                1 => o.id = expect_varint(&f)?,
                3 => o.retries = expect_varint(&f)? as u32,
                _ => {}
            }
        }
        Ok(o)
    }
}

/// A *wrong* version 3, subtler: number 3 is reused for another `string`, `coupon_code`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OrderV3ReusedTag3 {
    pub id: u64,
    pub coupon_code: String,
}

impl OrderV3ReusedTag3 {
    pub fn decode(buf: &[u8]) -> Result<OrderV3ReusedTag3> {
        let mut o = OrderV3ReusedTag3::default();
        for f in decode_fields(buf)? {
            match f.number {
                1 => o.id = expect_varint(&f)?,
                3 => o.coupon_code = expect_string(&f)?,
                _ => {}
            }
        }
        Ok(o)
    }
}

/// How the same varint is interpreted under different declarations of one field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntDecl {
    /// `int64`: two's complement.
    Int64,
    /// `sint64`: ZigZag.
    Sint64,
    /// `int32`: the varint is truncated to its low 32 bits.
    Int32,
}

impl IntDecl {
    pub fn read(self, varint: u64) -> i64 {
        match self {
            IntDecl::Int64 => varint as i64,
            IntDecl::Sint64 => zigzag_decode(varint),
            IntDecl::Int32 => i64::from(varint as u32 as i32),
        }
    }
}

/// The record layout of an order as JSON text, for a size comparison with the binary form.
pub fn order_v2_as_json(o: &OrderV2) -> String {
    format!(
        "{{\"id\":{},\"customer\":\"{}\",\"currency\":\"{}\",\"amount_cents\":{}}}",
        o.id, o.customer, o.currency, o.amount_cents
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;
    use crate::util::hex;

    fn enc_varint(v: u64) -> Vec<u8> {
        let mut out = Vec::new();
        put_varint(&mut out, v);
        out
    }

    #[test]
    fn varint_encodings_for_boundary_values_and_the_guide_example_150() {
        assert_eq!(hex(&enc_varint(150)), "96 01"); // 150 = 0b1_0010110 -> 0x96 0x01
        assert_eq!(hex(&enc_varint(0)), "00");
        assert_eq!(hex(&enc_varint(127)), "7f");
        assert_eq!(hex(&enc_varint(128)), "80 01");
        assert_eq!(hex(&enc_varint(300)), "ac 02");
    }

    #[test]
    fn varint_lengths_grow_with_magnitude_up_to_ten_bytes() {
        assert_eq!(varint_len(127), 1);
        assert_eq!(varint_len(128), 2);
        assert_eq!(varint_len((1 << 14) - 1), 2);
        assert_eq!(varint_len(1 << 14), 3);
        assert_eq!(varint_len(u64::MAX), 10);
        // A negative int64 is its two's complement: all high bits set, ten bytes.
        assert_eq!(varint_len(-1i64 as u64), 10);
    }

    #[test]
    fn varint_round_trips_and_rejects_malformed_input() {
        let mut rng = Rng::new(1);
        for _ in 0..10_000 {
            let v = rng.next_u64() >> rng.below(64);
            let bytes = enc_varint(v);
            let mut pos = 0;
            assert_eq!(get_varint(&bytes, &mut pos), Ok(v));
            assert_eq!(pos, bytes.len());
        }
        // Continuation bit set on the last available byte.
        assert_eq!(get_varint(&[0x80], &mut 0), Err(DecodeError::Truncated));
        assert_eq!(get_varint(&[], &mut 0), Err(DecodeError::Truncated));
        // Eleven bytes, or a tenth byte that overflows 64 bits.
        assert_eq!(get_varint(&[0x80; 11], &mut 0), Err(DecodeError::VarintOverflow));
        let mut too_big = vec![0xFF; 9];
        too_big.push(0x02);
        assert_eq!(get_varint(&too_big, &mut 0), Err(DecodeError::VarintOverflow));
        too_big[9] = 0x01;
        assert_eq!(get_varint(&too_big, &mut 0), Ok(u64::MAX));
    }

    #[test]
    fn zigzag_maps_small_magnitudes_to_small_numbers() {
        let pairs = [(0, 0), (-1, 1), (1, 2), (-2, 3), (2, 4), (i64::from(i32::MAX), 4_294_967_294), (i64::from(i32::MIN), 4_294_967_295)];
        for (n, u) in pairs {
            assert_eq!(zigzag_encode(n), u, "encode {n}");
            assert_eq!(zigzag_decode(u), n, "decode {u}");
        }
        for n in [i64::MIN, i64::MAX, i64::MIN + 1, -1234567, 1234567] {
            assert_eq!(zigzag_decode(zigzag_encode(n)), n);
        }
        // p -> 2p and n -> 2|n| - 1.
        assert_eq!(zigzag_encode(21), 42);
        assert_eq!(zigzag_encode(-21), 41);
    }

    #[test]
    fn tags_follow_field_number_shift_3_or_wire_type() {
        assert_eq!(tag(1, WireType::Varint), 0x08);
        assert_eq!(tag(2, WireType::Len), 0x12);
        assert_eq!(hex(&enc_varint(tag(15, WireType::Varint))), "78"); // field 15 still fits one byte
        assert_eq!(hex(&enc_varint(tag(16, WireType::Varint))), "80 01"); // field 16 needs two
        assert_eq!(hex(&enc_varint(tag(2047, WireType::Varint))).split(' ').count(), 2);
        assert_eq!(hex(&enc_varint(tag(2048, WireType::Varint))).split(' ').count(), 3);
    }

    #[test]
    fn the_documented_examples_encode_to_the_documented_bytes() {
        // message Test1 { int32 a = 1; } with a = 150 is 08 96 01.
        let mut out = Vec::new();
        Field::varint(1, 150).encode_into(&mut out);
        assert_eq!(hex(&out), "08 96 01");
        // message Test2 { string b = 2; } with b = "testing" is 12 07 74 65 73 74 69 6e 67.
        let mut out = Vec::new();
        Field::string(2, "testing").encode_into(&mut out);
        assert_eq!(hex(&out), "12 07 74 65 73 74 69 6e 67");
        // And back.
        assert_eq!(decode_fields(&out).unwrap(), vec![Field::string(2, "testing")]);
    }

    #[test]
    fn fixed_width_records_and_unsupported_wire_types() {
        let mut out = Vec::new();
        Field { number: 7, value: Value::I64(0x0102_0304_0506_0708) }.encode_into(&mut out);
        Field { number: 8, value: Value::I32(0xAABB_CCDD) }.encode_into(&mut out);
        assert_eq!(decode_fields(&out).unwrap().len(), 2);
        assert_eq!(decode_fields(&out).unwrap()[0].value, Value::I64(0x0102_0304_0506_0708));
        // Group start (wire type 3) is deprecated: rejected rather than guessed.
        assert_eq!(decode_fields(&[(1 << 3) | 3]), Err(DecodeError::UnsupportedWireType(3)));
        assert_eq!(decode_fields(&[(1 << 3) | 6, 0]), Err(DecodeError::UnsupportedWireType(6)));
        assert_eq!(decode_fields(&[0x00, 0x00]), Err(DecodeError::BadFieldNumber(0)));
    }

    fn sample_v1() -> OrderV1 {
        OrderV1 { id: 150, customer: "testing".into(), legacy_note: "gift wrap".into(), unknown: vec![] }
    }

    fn sample_v2() -> OrderV2 {
        OrderV2 { id: 150, customer: "testing".into(), currency: "USD".into(), amount_cents: 1990, unknown: vec![] }
    }

    #[test]
    fn same_version_round_trips() {
        let v1 = sample_v1();
        assert_eq!(OrderV1::decode(&v1.encode()).unwrap(), v1);
        let v2 = sample_v2();
        assert_eq!(OrderV2::decode(&v2.encode()).unwrap(), v2);
        let neg = OrderV2 { amount_cents: -250, ..sample_v2() };
        assert_eq!(OrderV2::decode(&neg.encode()).unwrap(), neg);
    }

    #[test]
    fn new_reader_old_writer_is_backward_compatible() {
        // v2 code reads bytes written by v1 code: `currency` and `amount_cents` are absent (defaults),
        // the retired field 3 is skipped as unknown.
        let old_bytes = sample_v1().encode();
        let read = OrderV2::decode(&old_bytes).unwrap();
        assert_eq!((read.id, read.customer.as_str()), (150, "testing"));
        assert_eq!((read.currency.as_str(), read.amount_cents), ("", 0));
        assert_eq!(read.unknown, vec![Field::string(3, "gift wrap")]);
    }

    #[test]
    fn old_reader_new_writer_is_forward_compatible() {
        // v1 code reads bytes written by v2 code: it skips fields 4 and 5 by wire type alone.
        let new_bytes = sample_v2().encode();
        let read = OrderV1::decode(&new_bytes).unwrap();
        assert_eq!((read.id, read.customer.as_str(), read.legacy_note.as_str()), (150, "testing", ""));
        assert_eq!(read.unknown, vec![Field::string(4, "USD"), Field::varint(5, 1990)]);
    }

    #[test]
    fn a_read_modify_write_in_old_code_only_keeps_new_fields_if_unknown_fields_are_preserved() {
        let new_bytes = sample_v2().encode();
        let mut order = OrderV1::decode(&new_bytes).unwrap();
        order.customer = "renamed".into(); // the old service edits a field it knows

        let preserved = OrderV2::decode(&order.encode()).unwrap();
        assert_eq!((preserved.customer.as_str(), preserved.currency.as_str(), preserved.amount_cents), ("renamed", "USD", 1990));

        // Copying known fields into a new message drops the rest.
        let lossy = OrderV2::decode(&order.without_unknown().encode()).unwrap();
        assert_eq!((lossy.customer.as_str(), lossy.currency.as_str(), lossy.amount_cents), ("renamed", "", 0));
    }

    #[test]
    fn reusing_a_retired_number_with_another_wire_type_is_caught_when_the_old_data_is_read() {
        let old_bytes = sample_v1().encode(); // field 3 is a string (LEN)
        let err = OrderV3RetypedTag3::decode(&old_bytes).unwrap_err();
        assert_eq!(err, DecodeError::WireTypeMismatch { field: 3, expected: WireType::Varint, found: WireType::Len });
    }

    #[test]
    fn reusing_a_retired_number_with_the_same_wire_type_silently_misreads_old_data() {
        let old_bytes = sample_v1().encode();
        let read = OrderV3ReusedTag3::decode(&old_bytes).unwrap(); // no error at all
        assert_eq!(read.coupon_code, "gift wrap", "the old free-text note is now a coupon code");
    }

    #[test]
    fn changing_int64_to_sint64_or_int32_changes_the_meaning_of_the_same_bytes() {
        for (value, as_sint64, as_int32) in [(150i64, 75, 150), (-1, i64::MIN, -1), (5_000_000_000, 2_500_000_000, 705_032_704), (-5, i64::MIN + 2, -5)] {
            let wire = value as u64; // what an int64 field writes
            assert_eq!(IntDecl::Int64.read(wire), value);
            assert_eq!(IntDecl::Sint64.read(wire), as_sint64, "{value} read as sint64");
            assert_eq!(IntDecl::Int32.read(wire), as_int32, "{value} read as int32");
        }
        // sint64 on both sides is fine, of course.
        assert_eq!(IntDecl::Sint64.read(zigzag_encode(-5)), -5);
    }

    #[test]
    fn a_truncated_message_is_only_detected_when_the_cut_falls_inside_a_record() {
        let bytes = sample_v2().encode();
        let mut boundaries = vec![0];
        let mut pos = 0;
        for f in decode_fields(&bytes).unwrap() {
            let mut one = Vec::new();
            f.encode_into(&mut one);
            pos += one.len();
            boundaries.push(pos);
        }
        for cut in 0..=bytes.len() {
            let r = OrderV2::decode(&bytes[..cut]);
            if boundaries.contains(&cut) {
                assert!(r.is_ok(), "cut at a record boundary ({cut}) looks like a shorter, valid message");
            } else {
                assert!(r.is_err(), "cut at {cut} falls inside a record");
            }
        }
    }

    #[test]
    fn decoding_arbitrary_bytes_never_panics() {
        let mut rng = Rng::new(99);
        let mut ok = 0;
        for _ in 0..20_000 {
            let mut buf = vec![0u8; rng.index(40)];
            rng.fill(&mut buf);
            if OrderV1::decode(&buf).is_ok() {
                ok += 1;
            }
            let _ = OrderV2::decode(&buf);
            let _ = decode_fields(&buf);
        }
        assert!(ok < 20_000, "most random buffers are not valid messages");
        // Flipping each single bit of a valid message either still decodes or errors; it never panics.
        let bytes = sample_v2().encode();
        for i in 0..bytes.len() * 8 {
            let mut b = bytes.clone();
            b[i / 8] ^= 1 << (i % 8);
            let _ = OrderV2::decode(&b);
        }
    }

    #[test]
    fn binary_is_smaller_than_json_because_names_are_not_repeated() {
        let o = sample_v2();
        let json = order_v2_as_json(&o);
        assert_eq!(json, r#"{"id":150,"customer":"testing","currency":"USD","amount_cents":1990}"#);
        assert!(o.encode().len() * 2 < json.len(), "{} vs {}", o.encode().len(), json.len());
    }
}
