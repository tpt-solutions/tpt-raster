//! Local stub of `tpt-testkit`: golden comparison, fuzz input generation and timing.
#![forbid(unsafe_code)]

pub mod golden {
    /// Panics with the first differing byte if `actual != expected`.
    pub fn assert_bytes_eq(actual: &[u8], expected: &[u8]) {
        assert_eq!(actual.len(), expected.len(), "golden length mismatch");
        if let Some(i) = actual.iter().zip(expected).position(|(a, e)| a != e) {
            panic!(
                "golden mismatch at byte {i}: actual {:#04x}, expected {:#04x}",
                actual[i], expected[i]
            );
        }
    }
}

pub mod fuzz {
    /// Deterministic xorshift64* generator, so fuzz failures reproduce from a seed.
    #[derive(Clone, Debug)]
    pub struct Rng(u64);

    impl Rng {
        pub fn new(seed: u64) -> Self {
            // Zero is a fixed point of xorshift, so map it to a non-zero state.
            Self(if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            })
        }

        pub fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        /// Any bit pattern as `f32`, including NaN, infinities and subnormals.
        pub fn next_f32_bits(&mut self) -> f32 {
            f32::from_bits(self.next_u64() as u32)
        }

        /// Uniform-ish integer in `0..n`. `n` must be non-zero.
        pub fn below(&mut self, n: u64) -> u64 {
            self.next_u64() % n
        }
    }
}

pub mod bench {
    use std::time::{Duration, Instant};

    /// Runs `f` `iters` times and returns the total elapsed wall-clock time.
    pub fn time_iters<F: FnMut()>(iters: u32, mut f: F) -> Duration {
        let start = Instant::now();
        for _ in 0..iters {
            f();
        }
        start.elapsed()
    }
}
