//! CRC-32C (Castagnoli), table driven, written out so the algorithm is visible.
//!
//! Generator polynomial `0x11EDC6F41` (RFC 3720 section 12.1). The table is built for the bit-reflected
//! form `0x82F63B78` of the same polynomial, i.e. the usual "reflected input, reflected output,
//! initial value and final xor `0xFFFFFFFF`" variant. The unit tests use the four test vectors of
//! RFC 3720 appendix B.4.

const POLY_REFLECTED: u32 = 0x82F6_3B78;

const TABLE: [u32; 256] = build_table();

const fn build_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ POLY_REFLECTED } else { crc >> 1 };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// Checksum of `data`.
pub fn crc32c(data: &[u8]) -> u32 {
    crc32c_update(0, data)
}

/// Continues a checksum: `crc32c_update(crc32c(a), b) == crc32c(a ++ b)`.
pub fn crc32c_update(crc: u32, data: &[u8]) -> u32 {
    let mut c = !crc;
    for &byte in data {
        c = TABLE[((c ^ u32::from(byte)) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 3720 appendix B.4 prints the CRC as four bytes in digest order; the numbers below are those
    // bytes read as a little-endian u32.
    #[test]
    fn rfc3720_vectors() {
        assert_eq!(crc32c(&[0u8; 32]), 0x8A91_36AA); // aa 36 91 8a
        assert_eq!(crc32c(&[0xFFu8; 32]), 0x62A8_AB43); // 43 ab a8 62
        let inc: Vec<u8> = (0u8..32).collect();
        assert_eq!(crc32c(&inc), 0x46DD_794E); // 4e 79 dd 46
        let dec: Vec<u8> = (0u8..32).rev().collect();
        assert_eq!(crc32c(&dec), 0x113F_DB5C); // 5c db 3f 11
    }

    #[test]
    fn iscsi_read_command_pdu() {
        // "An iSCSI - SCSI Read (10) Command PDU", same appendix: CRC 56 3a 96 d9.
        let pdu: [u8; 48] = [
            0x01, 0xc0, 0x00, 0x00, // 0
            0x00, 0x00, 0x00, 0x00, // 4
            0x00, 0x00, 0x00, 0x00, // 8
            0x00, 0x00, 0x00, 0x00, // 12
            0x14, 0x00, 0x00, 0x00, // 16
            0x00, 0x00, 0x04, 0x00, // 20
            0x00, 0x00, 0x00, 0x14, // 24
            0x00, 0x00, 0x00, 0x18, // 28
            0x28, 0x00, 0x00, 0x00, // 32
            0x00, 0x00, 0x00, 0x00, // 36
            0x02, 0x00, 0x00, 0x00, // 40
            0x00, 0x00, 0x00, 0x00, // 44
        ];
        assert_eq!(crc32c(&pdu), 0xD996_3A56);
    }

    #[test]
    fn empty_input_and_incremental_update() {
        assert_eq!(crc32c(&[]), 0);
        let data: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        let whole = crc32c(&data);
        for split in [0, 1, 7, 500, 999, 1000] {
            let (a, b) = data.split_at(split);
            assert_eq!(crc32c_update(crc32c(a), b), whole, "split at {split}");
        }
    }

    #[test]
    fn every_single_bit_flip_is_detected() {
        let data: Vec<u8> = (0..100u8).map(|i| i.wrapping_mul(37)).collect();
        let good = crc32c(&data);
        for byte in 0..data.len() {
            for bit in 0..8 {
                let mut bad = data.clone();
                bad[byte] ^= 1 << bit;
                assert_ne!(crc32c(&bad), good, "flip byte {byte} bit {bit}");
            }
        }
    }
}
