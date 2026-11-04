//! PGF's seeded random number generator.
//!
//! Port of `semantic/pgfmath/rng.ts`: a Lehmer/Schrage generator
//! (`seed' = 69621·seed mod 2147483647`) so `rnd`, `rand` and `random(a,b)`
//! give the same sequence as the legacy pipeline for a given seed.
//!
//! ```
//! use tikz_core::semantic::pgfmath::rng::PgfRandom;
//! let mut rng = PgfRandom::new(1.0);
//! assert_eq!(rng.next_raw(), 69621);
//! assert_eq!(rng.next_raw(), 552_116_347);
//! ```

const RND_M: i64 = 2_147_483_647;
const RND_A: i64 = 69_621;
const RND_Q: i64 = 30_845;
const RND_R: i64 = 23_902;

/// Deterministic PGF random source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgfRandom {
    seed: i64,
}

impl Default for PgfRandom {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl PgfRandom {
    /// New generator; `initial_seed` is normalised like `\pgfmathsetseed`.
    pub fn new(initial_seed: f64) -> Self {
        Self {
            seed: normalize_seed(initial_seed),
        }
    }

    /// Current seed.
    pub fn seed(&self) -> i64 {
        self.seed
    }

    /// Reset the seed (`\pgfmathsetseed`).
    pub fn set_seed(&mut self, value: f64) {
        self.seed = normalize_seed(value);
    }

    /// Advance and return the raw seed value.
    pub fn next_raw(&mut self) -> i64 {
        let hi = self.seed / RND_Q;
        let lo = self.seed - hi * RND_Q;
        let mut next = RND_A * lo - RND_R * hi;
        if next <= 0 {
            next += RND_M;
        }
        self.seed = next;
        next
    }

    /// `rnd`: uniform in `[0, 1]`.
    pub fn rnd(&mut self) -> f64 {
        (self.next_raw() % 100_001) as f64 / 100_000.0
    }

    /// `rand`: uniform in `[-1, 1]`.
    pub fn rand(&mut self) -> f64 {
        ((self.next_raw() % 200_001) - 100_000) as f64 / 100_000.0
    }

    /// `random(from, to)`: integer in the inclusive range (bounds may be swapped).
    pub fn random_integer(&mut self, from: f64, to: f64) -> f64 {
        let (mut lower, mut upper) = (from.trunc(), to.trunc());
        if lower > upper {
            std::mem::swap(&mut lower, &mut upper);
        }
        let span = (upper - lower + 1.0).max(1.0);
        lower + (self.next_raw() as f64) % span
    }
}

fn normalize_seed(value: f64) -> i64 {
    if !value.is_finite() {
        return 1;
    }
    let truncated = value.trunc();
    if truncated <= 0.0 {
        return 1;
    }
    let normalized = (truncated % RND_M as f64) as i64;
    if normalized == 0 {
        1
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_matches_the_legacy_spec() {
        let mut rng = PgfRandom::new(1.0);
        assert_eq!(rng.next_raw(), 69_621);
        assert_eq!(rng.next_raw(), 552_116_347);
        assert_eq!(rng.next_raw(), 1_082_396_834);
    }

    #[test]
    fn seeds_are_normalised() {
        assert_eq!(PgfRandom::new(f64::INFINITY).seed(), 1);
        assert_eq!(PgfRandom::new(0.0).seed(), 1);
        assert_eq!(PgfRandom::new(2_147_483_647.0).seed(), 1);
        assert_eq!(PgfRandom::new(-5.0).seed(), 1);
        assert_eq!(PgfRandom::new(f64::NAN).seed(), 1);
        assert_eq!(PgfRandom::new(30_845.0).next_raw(), 2_147_459_745);
    }

    #[test]
    fn rnd_and_rand_scale_the_raw_value() {
        assert!((PgfRandom::new(1.0).rnd() - 0.69621).abs() < 1e-12);
        assert!((PgfRandom::new(1.0).rand() + 0.30379).abs() < 1e-12);
    }

    #[test]
    fn random_integer_swaps_bounds_and_stays_in_range() {
        for seed in 1..50 {
            let mut rng = PgfRandom::new(f64::from(seed));
            let value = rng.random_integer(10.0, 1.0);
            assert!((1.0..=10.0).contains(&value), "{value}");
        }
        assert_eq!(PgfRandom::new(1.0).random_integer(5.0, 5.0), 5.0);
    }
}
