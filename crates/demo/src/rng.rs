//! Tiny deterministic PRNG (SplitMix64). No `rand` dependency so the demo
//! dataset stays byte-identical across dependency upgrades.

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    /// Independent stream derived from this seed and a label.
    pub fn fork(seed: u64, label: &str) -> Self {
        Rng::new(seed ^ fnv(label.as_bytes()))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n` (`n > 0`).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    /// Uniform in `lo..=hi`.
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }

    /// True with probability `p` (0..=1).
    pub fn chance(&mut self, p: f64) -> bool {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64 <= p
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }

    /// `k` distinct indices below `n`.
    pub fn distinct(&mut self, n: usize, k: usize) -> Vec<usize> {
        let mut out = Vec::with_capacity(k);
        while out.len() < k.min(n) {
            let i = self.below(n as u64) as usize;
            if !out.contains(&i) {
                out.push(i);
            }
        }
        out
    }
}

pub fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Deterministic GUID-shaped id from a label.
pub fn guid(label: &str) -> String {
    let a = fnv(label.as_bytes());
    let b = fnv(format!("{label}#2").as_bytes());
    format!(
        "{:08x}-{:04x}-4{:03x}-a{:03x}-{:012x}",
        a >> 32,
        (a >> 16) & 0xffff,
        a & 0xfff,
        (b >> 48) & 0xfff,
        b & 0xffff_ffff_ffff
    )
}

/// Deterministic 40-hex git-style object id for content.
pub fn object_id(content: &str) -> String {
    let a = fnv(content.as_bytes());
    let b = fnv(format!("{}\u{1}", content.len()).as_bytes()) ^ a.rotate_left(17);
    let c = fnv(&a.to_le_bytes()) ^ b;
    format!("{a:016x}{b:016x}{:08x}", c >> 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_bounded() {
        let a: Vec<u64> = (0..5)
            .map({
                let mut r = Rng::new(7);
                move |_| r.next_u64()
            })
            .collect();
        let b: Vec<u64> = (0..5)
            .map({
                let mut r = Rng::new(7);
                move |_| r.next_u64()
            })
            .collect();
        assert_eq!(a, b);
        let mut r = Rng::new(1);
        for _ in 0..1000 {
            let v = r.range(-3, 3);
            assert!((-3..=3).contains(&v));
        }
        assert_eq!(guid("x").len(), 36);
        assert_eq!(object_id("x").len(), 40);
        assert_ne!(object_id("a"), object_id("b"));
    }
}
