//! Ring: a van der Pol oscillator whose damping follows the charge.

use mxm_dsp::guard::flush;
use mxm_dsp::shape::tanh_approx;

/// The bounds that keep the state finite whatever drives it. The limit cycle sits near `x = 2`.
const X_MAX: f32 = 3.0;
const U_MAX: f32 = 6.0;

/// `ẍ − ε ω (1 − x²) ẋ + ω² x = f` (van der Pol, 1926). A negative `ε` damps a struck tone; a
/// positive one makes rest unstable, and the oscillator grows into its limit cycle and sustains
/// itself. The cell sets `ε` from its charge against a threshold, so a ring crosses from a damped
/// thud to a drone as it fills, and back as it drains.
///
/// Integrated with semi-implicit (symplectic) Euler in the normalised velocity `u = ẋ / ω`, which
/// keeps a lossless ring lossless at every sample rate.
#[derive(Clone, Copy, Debug, Default)]
pub struct Ring {
    x: f32,
    u: f32,
}

impl Ring {
    /// A hit: an impulse into the velocity.
    pub fn strike(&mut self, amount: f32) {
        self.u = (self.u + amount).clamp(-U_MAX, U_MAX);
    }

    /// One sample. `w` is ω in radians per sample, `eps` the damping, `force` an outside drive.
    /// Returns the output, within [-1, 1].
    pub fn tick(&mut self, w: f32, eps: f32, force: f32) -> f32 {
        let x = self.x;
        let u = self.u + w * (eps * (1.0 - x * x) * self.u - x + force);
        self.u = flush(u.clamp(-U_MAX, U_MAX));
        self.x = flush((x + w * self.u).clamp(-X_MAX, X_MAX));
        tanh_approx(0.7 * self.x)
    }

    /// The displacement, for an engine the ring drives.
    pub fn position(&self) -> f32 {
        self.x
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}
