//! The resonant band the noise engines ring through.

use mxm_dsp::guard::flush;
use mxm_dsp::shape::tan_approx;

/// A state-variable filter in Simper's trapezoidal form (Andrew Simper, *Linear Trapezoidal
/// Integrated SVF*, Cytomic, 2013); its band output scaled by `k` so the centre has unity gain.
/// The prewarp is the kit's basic-operation `tan_approx`, valid to 0.45 of the sample rate.
#[derive(Clone, Copy, Debug, Default)]
pub struct Band {
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
    ic1: f32,
    ic2: f32,
}

impl Band {
    /// Set the centre and Q; the state is kept.
    pub fn tune(&mut self, centre_hz: f32, q: f32, sample_rate: f32) {
        let normalised = f64::from((centre_hz / sample_rate).clamp(1e-5, 0.45));
        let g = tan_approx(core::f64::consts::PI * normalised) as f32;
        self.k = 1.0 / q.max(0.1);
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    /// One sample through the band.
    pub fn tick(&mut self, x: f32) -> f32 {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = flush(2.0 * v1 - self.ic1);
        self.ic2 = flush(2.0 * v2 - self.ic2);
        self.k * v1
    }

    pub fn clear(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
}
