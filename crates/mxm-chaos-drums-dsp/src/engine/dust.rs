//! Dust: velvet noise through a resonant band.

use mxm_dsp::shape::tanh_approx;
use mxm_rng::Xorshift32;

use super::svf::Band;

/// Below this many impulses a second there are none.
const MIN_DENSITY: f32 = 1.0;
/// The RMS the band is normalised to, so a dense hiss peaks near full scale.
const TARGET_RMS: f32 = 0.35;
/// The peak one isolated impulse may ring to, before the saturator.
const SPARSE_PEAK: f32 = 1.2;

/// Velvet noise (Karjalainen and Järveläinen, *Reverberation modeling using velvet noise*, AES 30th
/// International Conference, 2007): one impulse of random sign at a random place in each period. The
/// period follows the density, so the cell's charge thins it to grit or fills it to hiss. The band
/// is normalised by the density, so its loudness follows the charge, not the density.
///
/// The generator is reseeded on [`Dust::clear`], so a hit from rest is always the same hit.
#[derive(Clone, Debug)]
pub struct Dust {
    seed: u32,
    rng: Xorshift32,
    /// Samples left in the current period, and the count at which its impulse falls.
    left: u32,
    fire_at: u32,
    sign: f32,
    band: Band,
    /// The band's width in Hz, for the normalisation.
    width_hz: f32,
}

impl Dust {
    #[must_use]
    pub fn new(seed: u32) -> Self {
        Self {
            seed,
            rng: Xorshift32::new(seed),
            left: 0,
            fire_at: 0,
            sign: 1.0,
            band: Band::default(),
            width_hz: 1.0,
        }
    }

    pub fn tune(&mut self, centre_hz: f32, q: f32, sample_rate: f32) {
        self.band.tune(centre_hz, q, sample_rate);
        self.width_hz = centre_hz / q.max(0.1);
    }

    /// One sample at `density` impulses a second. Returns the output, within [-1, 1].
    pub fn tick(&mut self, density: f32, sample_rate: f32) -> f32 {
        let impulse = self.impulse(density, sample_rate);
        // White noise of mean square d/fs through a unity-peak band of noise bandwidth (π/2)·width
        // has RMS √(d·π·width)/fs; a lone impulse rings to about 2π·width/fs.
        let dense = TARGET_RMS * sample_rate
            / (density.max(MIN_DENSITY) * core::f32::consts::PI * self.width_hz).sqrt();
        let sparse = SPARSE_PEAK * sample_rate / (2.0 * core::f32::consts::PI * self.width_hz);
        tanh_approx(dense.min(sparse) * self.band.tick(impulse))
    }

    fn impulse(&mut self, density: f32, sample_rate: f32) -> f32 {
        if self.left == 0 {
            if density < MIN_DENSITY {
                return 0.0;
            }
            let period = ((sample_rate / density) as u32).max(1);
            let draw = self.rng.next_u32();
            self.sign = if draw & 0x8000_0000 != 0 { 1.0 } else { -1.0 };
            self.fire_at = (draw & 0x7FFF_FFFF) % period;
            self.left = period;
        }
        self.left -= 1;
        if self.left == self.fire_at {
            self.sign
        } else {
            0.0
        }
    }

    pub fn clear(&mut self) {
        self.rng = Xorshift32::new(self.seed);
        self.left = 0;
        self.fire_at = 0;
        self.band.clear();
    }
}
