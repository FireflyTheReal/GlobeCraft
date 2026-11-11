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
    /// `seed` with one to four bytes changed, flipped, inserted, or the end cut off.
    pub(crate) fn mutate(&mut self, seed: &[u8]) -> Vec<u8> {
        let mut out = seed.to_vec();
        for _ in 0..1 + self.below(4) {
            let at = self.below(out.len() + 1);
            match self.below(4) {
                0 if at < out.len() => out[at] = self.number() as u8,
                1 if at < out.len() => out[at] ^= 1 << self.below(8),
                2 => out.insert(at, self.number() as u8),
                _ => out.truncate(at),
            }
        }
        out
    }
}
