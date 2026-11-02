//! Fold: a phase oscillator kicked by the sine circle map.

use core::f32::consts::TAU;

use crate::fmath::sin_turns;

/// A phase oscillator at its own natural frequency, kicked once every period of a forcing clock by
/// the sine circle map, `θ ← θ − (K / 2π) · sin(2πθ)` (Arnold, 1965; the standard map of a kicked
/// rotor). With no kick it is a plain tone at its natural frequency. As `K` grows toward 1 the
/// oscillator locks onto rational ratios of the clock, Arnold's tongues, and steps between them;
/// past 1 the map is chaotic and the tone breaks into noise. The cell sets `K` from its charge.
///
/// Both rates are in cycles per second over the sample rate, so the tongues sit at the same pitches
/// at every sample rate.
#[derive(Clone, Copy, Debug, Default)]
pub struct Fold {
    phase: f32,
    clock: f32,
}

impl Fold {
    /// One sample. `natural` and `forcing` are in cycles per sample, `kick` is `K`. Returns the
    /// output, within [-1, 1].
    pub fn tick(&mut self, natural: f32, forcing: f32, kick: f32) -> f32 {
        self.phase += natural;
        self.phase -= self.phase.floor();
        self.clock += forcing;
        if self.clock >= 1.0 {
            self.clock -= self.clock.floor();
            self.phase -= kick / TAU * sin_turns(self.phase);
            self.phase -= self.phase.floor();
        }
        sin_turns(self.phase).clamp(-1.0, 1.0)
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}
