//! A tiny deterministic PRNG (SplitMix64) so that every lab prints the same numbers on every run.
//! Not for cryptography.

#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
    spare_normal: Option<f64>,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng { state: seed, spare_normal: None }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n` (`n > 0`); multiply-shift, bias below 2^-64 * n.
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "below(0)");
        ((u128::from(self.next_u64()) * u128::from(n)) >> 64) as u64
    }

    pub fn index(&mut self, len: usize) -> usize {
        self.below(len as u64) as usize
    }

    /// Uniform in `[0, 1)` with 53 random bits.
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.f64() < p
    }

    /// Exponential with the given mean.
    pub fn exp(&mut self, mean: f64) -> f64 {
        -mean * (1.0 - self.f64()).ln()
    }

    /// Standard normal (Box-Muller; the second value of each pair is cached).
    pub fn normal(&mut self) -> f64 {
        if let Some(v) = self.spare_normal.take() {
            return v;
        }
        let u1 = 1.0 - self.f64(); // (0, 1]
        let u2 = self.f64();
        let r = (-2.0 * u1.ln()).sqrt();
        let theta = std::f64::consts::TAU * u2;
        self.spare_normal = Some(r * theta.sin());
        r * theta.cos()
    }

    pub fn fill(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            let bytes = self.next_u64().to_le_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
        }
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.index(i + 1);
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        let mut c = Rng::new(43);
        let sa: Vec<u64> = (0..8).map(|_| a.next_u64()).collect();
        let sb: Vec<u64> = (0..8).map(|_| b.next_u64()).collect();
        let sc: Vec<u64> = (0..8).map(|_| c.next_u64()).collect();
        assert_eq!(sa, sb);
        assert_ne!(sa, sc);
    }

    #[test]
    fn below_stays_in_range_and_covers_it() {
        let mut r = Rng::new(7);
        let mut seen = [false; 10];
        for _ in 0..1000 {
            let v = r.below(10) as usize;
            seen[v] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn f64_is_in_unit_interval_with_sane_mean() {
        let mut r = Rng::new(1);
        let n = 100_000;
        let mut sum = 0.0;
        for _ in 0..n {
            let x = r.f64();
            assert!((0.0..1.0).contains(&x));
            sum += x;
        }
        let mean = sum / f64::from(n);
        assert!((mean - 0.5).abs() < 0.01, "mean {mean}");
    }

    #[test]
    fn normal_and_exp_have_the_right_moments() {
        let mut r = Rng::new(9);
        let n = 200_000;
        let (mut s, mut s2, mut e) = (0.0, 0.0, 0.0);
        for _ in 0..n {
            let x = r.normal();
            s += x;
            s2 += x * x;
            e += r.exp(5.0);
        }
        let n = f64::from(n);
        assert!((s / n).abs() < 0.02, "normal mean {}", s / n);
        assert!((s2 / n - 1.0).abs() < 0.03, "normal variance {}", s2 / n);
        assert!((e / n - 5.0).abs() < 0.1, "exp mean {}", e / n);
    }

    #[test]
    fn shuffle_is_a_permutation() {
        let mut r = Rng::new(3);
        let mut v: Vec<u32> = (0..100).collect();
        r.shuffle(&mut v);
        let mut sorted = v.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..100).collect::<Vec<_>>());
        assert_ne!(v, sorted);
    }
}
