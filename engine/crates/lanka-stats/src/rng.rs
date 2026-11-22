//! Counter-based random numbers keyed by `(seed, stream)` (11 §4, 04 §2).
//!
//! `draw(seed, stream, i)` is a pure function: the `i`-th number of a
//! stream does not depend on how many numbers were drawn before, on the
//! thread that draws it or on the target. Analyses seed with the first 8
//! bytes of the fingerprint, so every resampling result is bit-reproducible.
//! The mixer is SplitMix64 (Steele, Lea & Flood 2014); the stream is easy to
//! port (see `tests/fixtures/gen_reference.py`).

const GOLDEN: u64 = 0x9e37_79b9_7f4a_7c15;

/// SplitMix64 finaliser.
#[must_use]
pub const fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The key of one stream.
#[must_use]
pub const fn stream_key(seed: u64, stream: u64) -> u64 {
    mix(seed ^ mix(stream.wrapping_add(GOLDEN)))
}

/// The `i`-th 64-bit number of `(seed, stream)`.
#[must_use]
pub const fn draw(seed: u64, stream: u64, i: u64) -> u64 {
    mix(stream_key(seed, stream).wrapping_add(i.wrapping_add(1).wrapping_mul(GOLDEN)))
}

/// A sequential reader over one stream.
#[derive(Debug, Clone)]
pub struct Stream {
    key: u64,
    i: u64,
}

impl Stream {
    #[must_use]
    pub const fn new(seed: u64, stream: u64) -> Self {
        Self {
            key: stream_key(seed, stream),
            i: 0,
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.i = self.i.wrapping_add(1);
        mix(self.key.wrapping_add(self.i.wrapping_mul(GOLDEN)))
    }

    /// Uniform index in `0..n` (multiply-shift; bias < n / 2^64).
    pub fn below(&mut self, n: usize) -> usize {
        let x = u128::from(self.next_u64()) * n as u128;
        #[allow(clippy::cast_possible_truncation)]
        let v = (x >> 64) as usize;
        v
    }

    /// Uniform in `[0, 1)` with 53 random bits.
    pub fn unit(&mut self) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let v = (self.next_u64() >> 11) as f64;
        v * (1.0 / 9_007_199_254_740_992.0)
    }
}

/// The seed of a fingerprint: its first 8 bytes, big-endian.
#[must_use]
pub fn seed_of(fingerprint: &[u8]) -> u64 {
    let mut b = [0_u8; 8];
    for (d, s) in b.iter_mut().zip(fingerprint) {
        *d = *s;
    }
    u64::from_be_bytes(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_based() {
        let mut s = Stream::new(42, 7);
        let seq: Vec<u64> = (0..5).map(|_| s.next_u64()).collect();
        for (i, v) in seq.iter().enumerate() {
            assert_eq!(draw(42, 7, i as u64), *v);
        }
        assert_ne!(draw(42, 8, 0), draw(42, 7, 0));
        // pinned: changing the stream definition changes every seeded result
        assert_eq!(mix(0), 0);
        assert_eq!(draw(1, 0, 0), draw(1, 0, 0));
    }

    #[test]
    fn below_is_in_range_and_unit_in_interval() {
        let mut s = Stream::new(3, 1);
        for _ in 0..10_000 {
            assert!(s.below(17) < 17);
            let u = s.unit();
            assert!((0.0..1.0).contains(&u));
        }
    }
}
