//! Seedable, dependency-free PRNG used by every simulation in this crate.
//!
//! The generator is SplitMix64. It is **not** cryptographic; it exists so that a simulation is a
//! pure function of its seed and prints identical output on every machine.

/// SplitMix64 state. `Clone` copies the stream position, so a clone replays the same numbers.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Creates a generator. Any seed is valid (including 0).
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Next 64 uniformly distributed bits.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, n)` without modulo bias (rejection sampling). Panics if `n == 0`.
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "Rng::below(0)");
        // Reject the top `2^64 mod n` values so every residue class has the same size.
        let zone = u64::MAX - (u64::MAX % n + 1) % n;
        loop {
            let x = self.next_u64();
            if x <= zone {
                return x % n;
            }
        }
    }

    /// Uniform in `[lo, hi)`. Panics if `lo >= hi`.
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        assert!(lo < hi, "Rng::range({lo}, {hi})");
        lo + self.below(hi - lo)
    }

    /// Uniform in `[0, 1)` with 53 random mantissa bits.
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// `true` with probability `p` (`p <= 0` never, `p >= 1` always).
    pub fn chance(&mut self, p: f64) -> bool {
        self.f64() < p
    }

    /// In-place Fisher–Yates shuffle.
    pub fn shuffle<T>(&mut self, xs: &mut [T]) {
        for i in (1..xs.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            xs.swap(i, j);
        }
    }

    /// A uniformly chosen element, or `None` for an empty slice.
    pub fn choose<'a, T>(&mut self, xs: &'a [T]) -> Option<&'a T> {
        if xs.is_empty() {
            None
        } else {
            Some(&xs[self.below(xs.len() as u64) as usize])
        }
    }

    /// An independent stream derived from this one and `label`. Giving each component of a
    /// simulation its own fork keeps their random choices from perturbing each other when one
    /// component draws more numbers than before.
    pub fn fork(&mut self, label: u64) -> Rng {
        let mixed = self.next_u64() ^ label.wrapping_mul(0xD6E8_FEB8_6659_FD93);
        Rng::new(mixed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pinned outputs: if this changes, every number printed in the lab write-up changes too.
    #[test]
    fn splitmix64_regression_vector() {
        let mut r = Rng::new(0);
        assert_eq!(r.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(r.next_u64(), 0x6E78_9E6A_A1B9_65F4);
        assert_eq!(r.next_u64(), 0x06C4_5D18_8009_454F);
    }

    #[test]
    fn same_seed_same_stream_different_seed_different_stream() {
        let a: Vec<u64> = {
            let mut r = Rng::new(7);
            (0..16).map(|_| r.next_u64()).collect()
        };
        let b: Vec<u64> = {
            let mut r = Rng::new(7);
            (0..16).map(|_| r.next_u64()).collect()
        };
        let c: Vec<u64> = {
            let mut r = Rng::new(8);
            (0..16).map(|_| r.next_u64()).collect()
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn below_is_in_range_and_roughly_uniform() {
        let mut r = Rng::new(1);
        let mut counts = [0u32; 6];
        for _ in 0..60_000 {
            counts[r.below(6) as usize] += 1;
        }
        // Expected 10_000 per bucket; 5 sigma of a binomial(60000, 1/6) is about 456.
        for c in counts {
            assert!((9_500..=10_500).contains(&c), "bucket {c}");
        }
    }

    #[test]
    fn below_one_is_always_zero_and_range_respects_bounds() {
        let mut r = Rng::new(3);
        for _ in 0..100 {
            assert_eq!(r.below(1), 0);
            let x = r.range(10, 13);
            assert!((10..13).contains(&x));
        }
    }

    #[test]
    fn f64_is_in_unit_interval_and_chance_edges() {
        let mut r = Rng::new(5);
        for _ in 0..10_000 {
            let x = r.f64();
            assert!((0.0..1.0).contains(&x));
        }
        assert!((0..1000).all(|_| !r.chance(0.0)));
        assert!((0..1000).all(|_| r.chance(1.0)));
    }

    #[test]
    fn shuffle_is_a_permutation_and_choose_handles_empty() {
        let mut r = Rng::new(9);
        let mut v: Vec<u32> = (0..50).collect();
        r.shuffle(&mut v);
        let mut sorted = v.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..50).collect::<Vec<_>>());
        assert_ne!(
            v, sorted,
            "50 elements staying in order has probability 1/50!"
        );
        assert_eq!(r.choose::<u8>(&[]), None);
        assert_eq!(r.choose(&[42]), Some(&42));
    }

    #[test]
    fn forks_are_independent_of_how_much_the_parent_is_used_afterwards() {
        let mut parent = Rng::new(11);
        let mut child = parent.fork(1);
        let first = child.next_u64();
        let mut parent2 = Rng::new(11);
        let mut child2 = parent2.fork(1);
        for _ in 0..100 {
            parent2.next_u64();
        }
        assert_eq!(first, child2.next_u64());
        let mut other = Rng::new(11).fork(2);
        assert_ne!(first, other.next_u64());
    }
}
