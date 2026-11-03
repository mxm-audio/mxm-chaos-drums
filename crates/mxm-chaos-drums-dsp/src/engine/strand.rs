//! Loop: a short nonlinear delay loop, plucked or howling.

use mxm_dsp::delay::AgeLine;
use mxm_dsp::guard::flush;
use mxm_dsp::shape::tanh_approx;

/// The lowest pitch a loop holds, which sets its line's length: below the raw loop's four octaves
/// down (13.75 Hz), where it is a flutter more than a tone.
pub const LOWEST_HZ: f32 = 10.0;

/// The Karplus–Strong family (Karplus and Strong, 1983; Jaffe and Smith, 1983): a delay line fed
/// back through a one-pole low-pass, here with a saturator in the loop. Below unity loop gain it is a
/// pluck that dies by its own loss; past it the loop grows until the saturator holds it, a howl. The
/// cell sets the gain from its charge. A second tap at an inharmonic length makes it metal.
///
/// The line belongs to the cell, made once when the kit is, so changing model never allocates.
#[derive(Clone, Copy, Debug, Default)]
pub struct Strand {
    low: f32,
}

/// A loop's settings for one sample.
#[derive(Clone, Copy, Debug)]
pub struct StrandDrive {
    /// The loop length, in samples.
    pub age: f32,
    /// A second tap's length, in samples, and its share of the loop (0 for none).
    pub second_age: f32,
    pub second: f32,
    /// The loop gain: below 1 a pluck, above 1 a howl.
    pub gain: f32,
    /// The loop low-pass coefficient, 0 (open) … 1 (shut).
    pub damping: f32,
    /// What is fed in this sample.
    pub input: f32,
}

impl Strand {
    /// One sample through `line`. Returns the output, within [-1, 1].
    pub fn tick(&mut self, line: &mut AgeLine, drive: &StrandDrive) -> f32 {
        let longest = (line.capacity() - 1) as f32;
        let first = line.read_linear(drive.age.clamp(2.0, longest));
        let tap = if drive.second > 0.0 {
            let second = line.read_linear(drive.second_age.clamp(2.0, longest));
            first + drive.second.clamp(0.0, 1.0) * (second - first)
        } else {
            first
        };
        self.low = flush(self.low + (tap - self.low) * (1.0 - drive.damping.clamp(0.0, 0.99)));
        let out = tanh_approx(drive.gain * self.low + drive.input);
        line.push(flush(out));
        out
    }

    pub fn clear(&mut self, line: &mut AgeLine) {
        self.low = 0.0;
        line.clear();
    }
}
