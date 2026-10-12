//! A Bloom filter: a bit array that answers "possibly present" or "definitely absent".
//!
//! `k` probe positions per key come from two 64-bit hashes of the key (`h1 + i * h2`). The filter
//! never reports an inserted key as absent; it may report an absent key as present (false positive).

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a, 64 bit. Chosen because it is short enough to read; it has no seed and no randomness.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// Two hashes of `key`; the second is forced odd so that the probe sequence does not collapse.
pub fn hash_pair(key: &[u8]) -> (u64, u64) {
    let h1 = fnv1a(key);
    // Finalizer of SplitMix64 applied to h1 gives a second, differently-mixed value.
    let mut z = h1.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (h1, (z ^ (z >> 31)) | 1)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bloom {
    bits: Vec<u8>,
    nbits: u64,
    k: u32,
}

impl Bloom {
    /// A filter sized for `keys` keys at `bits_per_key` bits each, with the number of probes
    /// `k = round(0.69 * bits_per_key)` that minimises the false-positive rate for that size.
    pub fn with_capacity(keys: usize, bits_per_key: usize) -> Bloom {
        let nbits = (keys.max(1) * bits_per_key).max(64).next_multiple_of(8) as u64;
        let k = ((bits_per_key as f64 * 0.69).round() as u32).clamp(1, 30);
        Bloom { bits: vec![0; (nbits / 8) as usize], nbits, k }
    }

    pub fn insert(&mut self, key: &[u8]) {
        self.insert_hashed(hash_pair(key));
    }

    pub fn insert_hashed(&mut self, (h1, h2): (u64, u64)) {
        for i in 0..u64::from(self.k) {
            let bit = h1.wrapping_add(i.wrapping_mul(h2)) % self.nbits;
            self.bits[(bit / 8) as usize] |= 1 << (bit % 8);
        }
    }

    /// `false` means the key was certainly never inserted.
    pub fn may_contain(&self, key: &[u8]) -> bool {
        let (h1, h2) = hash_pair(key);
        (0..u64::from(self.k)).all(|i| {
            let bit = h1.wrapping_add(i.wrapping_mul(h2)) % self.nbits;
            self.bits[(bit / 8) as usize] & (1 << (bit % 8)) != 0
        })
    }

    pub fn probes(&self) -> u32 {
        self.k
    }

    pub fn size_bytes(&self) -> usize {
        self.bits.len()
    }

    /// `k: u32 | nbits: u64 | bits`
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(12 + self.bits.len());
        out.extend_from_slice(&self.k.to_le_bytes());
        out.extend_from_slice(&self.nbits.to_le_bytes());
        out.extend_from_slice(&self.bits);
        out
    }

    pub fn from_bytes(buf: &[u8]) -> Option<Bloom> {
        let k = u32::from_le_bytes(buf.get(0..4)?.try_into().ok()?);
        let nbits = u64::from_le_bytes(buf.get(4..12)?.try_into().ok()?);
        let bits = buf.get(12..)?.to_vec();
        let valid = k >= 1 && nbits >= 8 && nbits % 8 == 0 && bits.len() as u64 == nbits / 8;
        valid.then_some(Bloom { bits, nbits, k })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(i: u32) -> Vec<u8> {
        format!("key-{i:08}").into_bytes()
    }

    #[test]
    fn fnv1a_known_values() {
        // Offset basis for the empty input; "a" is the standard FNV-1a 64-bit test string.
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn no_false_negatives_ever() {
        let mut b = Bloom::with_capacity(10_000, 10);
        for i in 0..10_000 {
            b.insert(&key(i));
        }
        assert!((0..10_000).all(|i| b.may_contain(&key(i))));
    }

    #[test]
    fn false_positive_rate_is_near_one_percent_at_ten_bits_per_key() {
        let mut b = Bloom::with_capacity(10_000, 10);
        for i in 0..10_000 {
            b.insert(&key(i));
        }
        let probes = 200_000;
        let fp = (10_000..10_000 + probes).filter(|&i| b.may_contain(&key(i))).count();
        let rate = fp as f64 / f64::from(probes);
        assert!(rate > 0.001 && rate < 0.03, "false positive rate {rate}");
    }

    #[test]
    fn more_bits_per_key_means_fewer_false_positives() {
        let rate = |bpk: usize| {
            let mut b = Bloom::with_capacity(5_000, bpk);
            for i in 0..5_000 {
                b.insert(&key(i));
            }
            (5_000..105_000).filter(|&i| b.may_contain(&key(i))).count() as f64 / 100_000.0
        };
        let (r4, r10, r16) = (rate(4), rate(10), rate(16));
        assert!(r4 > r10 && r10 > r16, "{r4} {r10} {r16}");
    }

    #[test]
    fn serialization_round_trip_and_rejection_of_garbage() {
        let mut b = Bloom::with_capacity(100, 10);
        for i in 0..100 {
            b.insert(&key(i));
        }
        let bytes = b.to_bytes();
        let back = Bloom::from_bytes(&bytes).unwrap();
        assert_eq!(back, b);
        assert!((0..100).all(|i| back.may_contain(&key(i))));
        assert!(Bloom::from_bytes(&bytes[..bytes.len() - 1]).is_none());
        assert!(Bloom::from_bytes(&[0; 4]).is_none());
        let mut zero_k = bytes.clone();
        zero_k[..4].copy_from_slice(&0u32.to_le_bytes());
        assert!(Bloom::from_bytes(&zero_k).is_none());
    }
}
