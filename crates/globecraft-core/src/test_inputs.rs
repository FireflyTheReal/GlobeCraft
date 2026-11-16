//! Deterministic pseudo-random inputs for tests that feed parsers damaged or odd data.
//! The sequence depends only on the seed, so a failing case reproduces.

pub(crate) struct Inputs(u64);

impl Inputs {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    /// xorshift64*: fast, and plenty for test inputs.
    pub(crate) fn number(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    /// A number below `n` (which must not be zero).
    pub(crate) fn below(&mut self, n: usize) -> usize {
        (self.number() % n as u64) as usize
    }
    /// Up to `max_parts` pieces of `alphabet`, joined.
    pub(crate) fn text(&mut self, alphabet: &[&str], max_parts: usize) -> String {
        let parts = self.below(max_parts + 1);
        (0..parts)
            .map(|_| alphabet[self.below(alphabet.len())])
            .collect()
    }
}
