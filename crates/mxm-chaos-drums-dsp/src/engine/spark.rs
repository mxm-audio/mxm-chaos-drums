//! Spark: a leaky integrate-and-fire neuron.

/// A leaky integrate-and-fire neuron (Lapicque, 1907; Gerstner and Kistler, *Spiking Neuron
/// Models*, 2002) with a refractory period. Its input is in units of the firing threshold: below 1
/// it never fires, just above it fires isolated ticks, and well above it crackles at up to one
/// spike per refractory period, off any grid the notes keep.
///
/// Each spike is a short pulse whose two edges are band-limited with PolyBLEP at the sub-sample
/// time the threshold was crossed (Välimäki and Huovilainen, 2007), which costs one sample of delay.
#[derive(Clone, Copy, Debug, Default)]
pub struct Spark {
    /// The membrane, in units of the threshold.
    v: f32,
    refractory: u32,
    /// High samples still to come, and whether the falling edge is due next.
    high: u32,
    falling: bool,
    /// How far past the crossing the first high sample was, in samples: both edges share it.
    frac: f32,
    /// The previous sample, held for its correction.
    held: f32,
}

/// The PolyBLEP residual the sample after a unit step takes, `d` samples past the step.
fn after(d: f32) -> f32 {
    0.5 * (2.0 * d - d * d - 1.0)
}

/// The residual the sample before it takes.
fn before(d: f32) -> f32 {
    0.5 * d * d
}

impl Spark {
    /// Fire on the next sample if the input is still above threshold then: a hit's spark.
    pub fn prime(&mut self) {
        if self.refractory == 0 {
            self.v = self.v.max(1.0);
        }
    }

    /// One sample. `input` is in units of the threshold, `leak` is the sample period over the
    /// membrane's time constant, `refractory` and `width` are in samples (`refractory > width ≥ 2`).
    /// Returns the pulse train, within [0, 1], one sample late, and whether a spike fired.
    pub fn tick(&mut self, input: f32, leak: f32, refractory: u32, width: u32) -> (f32, bool) {
        let mut now = 0.0;
        let mut prev = self.held;
        if self.high > 0 {
            now = 1.0;
            self.high -= 1;
            self.falling = self.high == 0;
        } else if self.falling {
            self.falling = false;
            now -= after(self.frac);
            prev -= before(self.frac);
        }

        let fired = if self.refractory > 0 {
            self.refractory -= 1;
            self.v = 0.0;
            false
        } else {
            let start = self.v;
            self.v += (input - self.v) * leak;
            if self.v >= 1.0 {
                let d = if self.v > start {
                    ((self.v - 1.0) / (self.v - start)).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                self.v = 0.0;
                self.refractory = refractory;
                self.frac = d;
                self.high = width.max(2) - 1;
                now += 1.0 + after(d);
                prev += before(d);
                true
            } else {
                false
            }
        };

        self.held = now;
        (prev.clamp(0.0, 1.0), fired)
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn below_threshold_it_never_fires_and_above_it_fires_at_most_once_a_refractory_period() {
        let mut spark = Spark::default();
        assert!((0..10_000).all(|_| !spark.tick(0.99, 0.01, 200, 12).1));
        let mut spark = Spark::default();
        let fired = (0..10_000)
            .filter(|_| spark.tick(50.0, 0.5, 200, 12).1)
            .count();
        assert!(fired > 40 && fired <= 10_000 / 200, "{fired}");
    }
}
