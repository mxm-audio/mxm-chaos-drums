//! A cell: one drum, its charge, and the law that charge follows.

use mxm_dsp::guard::flush;
use mxm_rng::{SplitMix64, Xorshift32};

use crate::fmath;
use crate::model::ModelId;
use crate::voice::{Drive, Voice};

/// Below this a cell's charge is exactly zero: about −90 dB, so every unfed cell comes to rest.
pub const CHARGE_FLOOR: f32 = 3e-5;
/// The Leak range, in milliseconds. Leak never reaches zero, so rest is always reached.
pub const LEAK_MS: (f32, f32) = (20.0, 30_000.0);
/// Supply just above zero: a held note at full velocity fills an empty cell's time constant in
/// this many seconds…
const FILL_SLOWEST_S: f32 = 10.0;
/// …and at full, a thousandth of it, 10 ms: `log2(0.001)`, written out, since no logarithm runs here.
const FILL_LOG2_RANGE: f32 = -9.965_784;
/// Hunger at full spends charge at this rate per unit of output power, per second.
const HUNGER_FULL: f32 = 80.0;
/// The jitter's wander: a one-pole low-pass on seeded noise, at this corner.
const JITTER_HZ: f32 = 8.0;

/// One cell's settings: the brief's per-cell parameters that P1 has.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellParams {
    pub model: ModelId,
    /// Semitones, −24…+24.
    pub coarse: i8,
    /// The packet a full-velocity note-on adds, as a share of the ceiling, 0…1.
    pub strike: f32,
    /// How fast a held note fills the cell, 0…1; 0 is none.
    pub supply: f32,
    /// How fast sounding spends charge, 0…1.
    pub hunger: f32,
    /// The time to drain at rest, in milliseconds, within [`LEAK_MS`].
    pub leak_ms: f32,
    /// The model's threshold moved, −1…+1.
    pub threshold: f32,
    /// The cell's share in Flow and Coupling, 0…1.
    pub link: f32,
}

impl CellParams {
    /// The brief's defaults for `model`.
    #[must_use]
    pub fn new(model: ModelId) -> Self {
        Self {
            model,
            coarse: 0,
            strike: 0.6,
            supply: 0.3,
            hunger: 0.5,
            leak_ms: Voice::default_leak_ms(model),
            threshold: 0.0,
            link: 1.0,
        }
    }
}

pub(crate) struct Cell {
    index: usize,
    params: CellParams,
    sample_rate: f32,
    dt: f32,
    /// Derived from the parameters: per second, and the pitch ratio.
    fill_rate: f32,
    hunger_rate: f32,
    leak_rate: f32,
    pitch: f32,

    charge: f32,
    gate: bool,
    velocity: f32,
    voice: Voice,
    voice_seed: u32,
    jitter_seed: u32,
    jitter_rng: Xorshift32,
    jitter: f32,
    jitter_alpha: f32,
    /// At rest: no charge, no gate, every engine cleared. Ticking a resting cell changes nothing.
    resting: bool,
}

/// A stream's seed from the kit's Seed, the cell and the stream's slot.
fn stream_seed(kit_seed: u32, cell: usize, slot: u64) -> u32 {
    let index = (u64::from(kit_seed) << 16) | ((cell as u64) << 4) | slot;
    SplitMix64::mix(index) as u32
}

impl Cell {
    pub fn new(params: CellParams, sample_rate: f32, kit_seed: u32, index: usize) -> Self {
        let voice_seed = stream_seed(kit_seed, index, 1);
        let jitter_seed = stream_seed(kit_seed, index, 2);
        let mut cell = Self {
            index,
            params,
            sample_rate,
            dt: 1.0 / sample_rate,
            fill_rate: 0.0,
            hunger_rate: 0.0,
            leak_rate: 0.0,
            pitch: 1.0,
            charge: 0.0,
            gate: false,
            velocity: 0.0,
            voice: Voice::new(params.model, voice_seed, sample_rate),
            voice_seed,
            jitter_seed,
            jitter_rng: Xorshift32::new(jitter_seed),
            jitter: 0.0,
            jitter_alpha: (core::f32::consts::TAU * JITTER_HZ / sample_rate).min(1.0),
            resting: true,
        };
        cell.derive();
        cell
    }

    fn derive(&mut self) {
        let p = &self.params;
        self.fill_rate = if p.supply > 0.0 {
            // The fill time runs geometrically from the slowest to the fastest.
            1.0 / (FILL_SLOWEST_S * fmath::exp2(FILL_LOG2_RANGE * p.supply.min(1.0)))
        } else {
            0.0
        };
        self.hunger_rate = HUNGER_FULL * p.hunger.clamp(0.0, 1.0);
        self.leak_rate = 1000.0 / p.leak_ms.clamp(LEAK_MS.0, LEAK_MS.1);
        self.pitch = fmath::semitones(f32::from(p.coarse.clamp(-24, 24)));
    }

    /// New settings. A new model starts from rest with the cell's charge.
    pub fn set_params(&mut self, params: CellParams) {
        if params.model != self.params.model {
            self.voice = Voice::new(params.model, self.voice_seed, self.sample_rate);
        }
        self.params = params;
        self.derive();
    }

    /// Exact rest, and every stream back to its seed, taken from `kit_seed`.
    pub fn reset(&mut self, kit_seed: u32) {
        self.voice_seed = stream_seed(kit_seed, self.index, 1);
        self.jitter_seed = stream_seed(kit_seed, self.index, 2);
        self.voice = Voice::new(self.params.model, self.voice_seed, self.sample_rate);
        self.charge = 0.0;
        self.gate = false;
        self.velocity = 0.0;
        self.voice.clear();
        self.jitter_rng = Xorshift32::new(self.jitter_seed);
        self.jitter = 0.0;
        self.resting = true;
    }

    /// A note-on at `velocity`, 0…1: the Strike packet, the engines' hit, and the gate opens.
    pub fn note_on(&mut self, velocity: f32) {
        self.charge = (self.charge + self.params.strike.clamp(0.0, 1.0) * velocity).min(1.0);
        self.voice.strike(velocity);
        self.velocity = velocity;
        self.gate = true;
        self.resting = false;
    }

    pub fn note_off(&mut self) {
        self.gate = false;
    }

    /// What the open gate asks of the pool this sample: it fills toward the ceiling.
    pub fn demand(&self) -> f32 {
        if self.gate {
            self.fill_rate * self.velocity * (1.0 - self.charge) * self.dt
        } else {
            0.0
        }
    }

    pub fn link(&self) -> f32 {
        self.params.link.clamp(0.0, 1.0)
    }

    pub fn charge(&self) -> f32 {
        self.charge
    }

    pub fn is_at_rest(&self) -> bool {
        self.resting
    }

    /// One sample: `inflow` from the pool and the neighbours, `neighbour` the coupled sound.
    /// Returns the live output, `|y| ≤ charge`.
    pub fn tick(&mut self, inflow: f32, neighbour: f32, temper: f32) -> f32 {
        self.charge = (self.charge + inflow).clamp(0.0, 1.0);
        if self.charge == 0.0 && !self.gate {
            if !self.resting {
                self.voice.clear();
                self.resting = true;
            }
            return 0.0;
        }
        self.resting = false;

        let noise = self.jitter_rng.bipolar_f32();
        self.jitter = flush(self.jitter + (noise - self.jitter) * self.jitter_alpha);

        let drive = Drive {
            charge: self.charge,
            threshold: self.params.threshold.clamp(-1.0, 1.0),
            temper,
            jitter: self.jitter,
            neighbour,
            pitch: self.pitch,
        };
        let y = self.charge * self.voice.tick(&drive, self.sample_rate);

        let spent = (self.hunger_rate * y * y + self.leak_rate * self.charge) * self.dt;
        self.charge = (self.charge - spent).max(0.0);
        if self.charge < CHARGE_FLOOR {
            self.charge = 0.0;
        }
        y
    }
}
