//! The musical curves of the controls that reach a cell's state, in basic operations.
//!
//! A parameter that reaches the performance is a plain linear amount, 0…1, and its curve lives
//! here (the owner, 2026-11-02: the performance identical on every platform; nice-plug's skewed
//! ranges and logarithmic smoothers call `powf`). The plugin shows a control's reading through the
//! same function, so what is shown is what plays.

use crate::fmath::exp2;

/// Leak's time at 0 and 1, in milliseconds: 20 ms to 30 s, geometric.
pub const LEAK_MS: (f32, f32) = (20.0, 30_000.0);
/// `log2(LEAK_MS.1 / LEAK_MS.0)`, written out.
pub const LEAK_OCTAVES: f32 = 10.550_747;

/// The Pool's refill time at 0 and 1: 10 ms to 10 s.
pub const RECHARGE_MS: (f32, f32) = (10.0, 10_000.0);
/// `log2(1000)`.
pub const RECHARGE_OCTAVES: f32 = 9.965_784;

/// An LFO's rate at 0 and 1: 0.05 Hz to 20 Hz.
pub const LFO_HZ: (f32, f32) = (0.05, 20.0);
/// `log2(400)`.
pub const LFO_OCTAVES: f32 = 8.643_856;

/// Leak's time for an amount, 0…1.
#[must_use]
pub fn leak_ms(amount: f32) -> f32 {
    LEAK_MS.0 * exp2(LEAK_OCTAVES * amount.clamp(0.0, 1.0))
}

/// The Pool's refill time for an amount, 0…1.
#[must_use]
pub fn recharge_ms(amount: f32) -> f32 {
    RECHARGE_MS.0 * exp2(RECHARGE_OCTAVES * amount.clamp(0.0, 1.0))
}

/// An LFO's free rate for an amount, 0…1.
#[must_use]
pub fn lfo_hz(amount: f32) -> f32 {
    LFO_HZ.0 * exp2(LFO_OCTAVES * amount.clamp(0.0, 1.0))
}
