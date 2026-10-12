//! CRC-32 — the work that lab (a) `hotspot` makes hot.
//!
//! This is the reflected CRC-32 with polynomial `0xEDB88320`, the variant used by zlib/gzip and
//! Ethernet. A table-driven loop is a good profiling target: one tight loop, one dependent
//! load per byte, no allocation, so `perf` attributes (almost) every sample to one function.

/// The reflected form of the polynomial.
pub const POLY: u32 = 0xEDB8_8320;

/// Builds the 256-entry lookup table. Runs once per process: the "cold" function of the lab.
pub fn build_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { POLY ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    table
}

/// Feeds `data` into a running CRC state. Start from `!0` and invert the final state
/// (see [`checksum`]); chaining calls over consecutive slices equals one call over their
/// concatenation.
#[inline]
pub fn update(table: &[u32; 256], mut crc: u32, data: &[u8]) -> u32 {
    for &byte in data {
        crc = table[((crc ^ u32::from(byte)) & 0xff) as usize] ^ (crc >> 8);
    }
    crc
}

/// The CRC-32 of `data`.
pub fn checksum(table: &[u32; 256], data: &[u8]) -> u32 {
    !update(table, !0, data)
}

/// Bit-at-a-time reference implementation: eight shifts per byte, no table. Slow on purpose;
/// it exists so the table-driven loop can be checked against something that shares no code with it.
pub fn checksum_bitwise(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { POLY ^ (crc >> 1) } else { crc >> 1 };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    /// xorshift64*: a deterministic stream of "random" bytes, so a failing case is reproducible.
    fn bytes(seed: u64, n: usize) -> Vec<u8> {
        let mut x = seed | 1;
        (0..n)
            .map(|_| {
                x ^= x >> 12;
                x ^= x << 25;
                x ^= x >> 27;
                (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
            })
            .collect()
    }

    /// The standard check value of this CRC variant ("CRC-32/ISO-HDLC") is 0xCBF43926 for the
    /// ASCII string "123456789" (CRC RevEng catalogue, https://reveng.sourceforge.io/crc-catalogue/).
    #[test]
    fn matches_the_published_check_value() {
        let table = build_table();
        assert_eq!(checksum(&table, b"123456789"), 0xCBF4_3926);
        assert_eq!(checksum_bitwise(b"123456789"), 0xCBF4_3926);
        assert_eq!(checksum(&table, b""), 0, "the CRC of the empty message is 0");
    }

    /// Differential test: the table-driven loop and the bit-at-a-time reference agree on
    /// inputs of every length around the boundaries (0, 1, 7, 8, 9, ...) and on larger blocks.
    #[test]
    fn table_driven_equals_bitwise_reference() {
        let table = build_table();
        for len in (0..70).chain([255, 256, 257, 4096, 65_537]) {
            let data = bytes(len as u64 + 1, len);
            assert_eq!(checksum(&table, &data), checksum_bitwise(&data), "len = {len}");
        }
    }

    /// Splitting the input anywhere and chaining `update` gives the same state: this is what
    /// lets `hotspot` feed one running CRC through many calls.
    #[test]
    fn chaining_equals_one_pass_for_every_split() {
        let table = build_table();
        let data = bytes(42, 300);
        let whole = update(&table, !0, &data);
        for cut in 0..=data.len() {
            let (a, b) = data.split_at(cut);
            assert_eq!(update(&table, update(&table, !0, a), b), whole, "cut = {cut}");
        }
    }

    /// A CRC detects every single-bit error; flip each bit of a short message in turn.
    #[test]
    fn detects_every_single_bit_flip() {
        let table = build_table();
        let data = bytes(7, 64);
        let good = checksum(&table, &data);
        for bit in 0..data.len() * 8 {
            let mut bad = data.clone();
            bad[bit / 8] ^= 1 << (bit % 8);
            assert_ne!(checksum(&table, &bad), good, "bit {bit} went unnoticed");
        }
    }
}
