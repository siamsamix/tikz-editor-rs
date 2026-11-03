//! Scoped RNG runtime so deep callers (dimensions, coordinates, foreach
//! `evaluate=`) can use `rnd`/`random` without threading an RNG through every
//! function. Mirrors legacy `withPgfMathRuntime`; each [`install`] gets a
//! fresh seed-1 generator and restores the previous runtime on drop.
//!
//! ```
//! use tikz_core::semantic::pgfmath::{evaluate, runtime};
//! assert!(evaluate("rnd").is_err());
//! let _scope = runtime::install();
//! assert!((evaluate("rnd").unwrap().value - 0.69621).abs() < 1e-12);
//! ```

use super::rng::PgfRandom;
use std::cell::RefCell;

thread_local! {
    static CURRENT: RefCell<Option<PgfRandom>> = const { RefCell::new(None) };
}

/// Keeps a runtime installed; dropping it restores the previous one.
pub struct RuntimeScope {
    previous: Option<PgfRandom>,
}

/// Install a fresh seed-1 runtime for the current thread.
pub fn install() -> RuntimeScope {
    let previous = CURRENT.with(|cell| cell.replace(Some(PgfRandom::default())));
    RuntimeScope { previous }
}

impl Drop for RuntimeScope {
    fn drop(&mut self) {
        let previous = self.previous.take();
        CURRENT.with(|cell| *cell.borrow_mut() = previous);
    }
}

/// Run `f` with the installed RNG, or `None` when no runtime is installed.
pub fn with_current_rng<T>(f: impl FnOnce(Option<&mut PgfRandom>) -> T) -> T {
    CURRENT.with(|cell| f(cell.borrow_mut().as_mut()))
}

/// Reseed the installed runtime (`\pgfmathsetseed`); `false` without one.
pub fn set_seed(value: f64) -> bool {
    with_current_rng(|rng| rng.map(|rng| rng.set_seed(value)).is_some())
}
