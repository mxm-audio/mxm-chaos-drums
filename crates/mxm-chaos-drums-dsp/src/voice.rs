//! The models' engine networks: how each turns its cell's charge into each engine's operating point.
//!
//! Every voice returns a raw sample within [-1, 1], its engines' mix weights summing to at most
//! one; the cell multiplies it by its charge, which is what bounds a live cell by its charge.
//! The constants are P1's starting points, to be settled by listening.

use core::f32::consts::TAU;

use mxm_dsp::guard::flush;

use crate::engine::{Dust, Ring, Spark};
use crate::model::ModelId;

/// What a cell gives its voice each sample.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Drive {
    /// The cell's charge, 0…1.
    pub charge: f32,
    /// The cell's Threshold parameter, −1…+1.
    pub threshold: f32,
    /// The kit's Temper, 0…1.
    pub temper: f32,
    /// The cell's seeded slow jitter, −1…+1; the voice scales it by Temper.
    pub jitter: f32,
    /// What the neighbours couple in, already scaled by Coupling and Temper.
    pub neighbour: f32,
    /// The frequency ratio from Coarse.
    pub pitch: f32,
}

/// How far the Threshold parameter moves a threshold, and how far jitter can at full Temper.
const THRESHOLD_REACH: f32 = 0.25;
const JITTER_REACH: f32 = 0.15;

/// A threshold in charge: the model's own at Temper 0, moved toward onset by Temper.
fn threshold(base: f32, temper_shift: f32, drive: &Drive) -> f32 {
    let t = drive.temper;
    (base + THRESHOLD_REACH * drive.threshold - temper_shift * t + JITTER_REACH * t * drive.jitter)
        .clamp(0.05, 0.98)
}

/// A Ring's law: its pitch, how its pitch follows charge, and where it starts to sustain itself.
struct RingLaw {
    hz: f32,
    /// Pitch rises by `drop · charge^power` over the base.
    drop: f32,
    power: i32,
    /// `ε = gain · (charge − threshold) − damping` below the threshold, and with `growth` in place
    /// of `gain` above it: crossing it tips the ring into growing on its own within tens of
    /// milliseconds, rather than barely.
    gain: f32,
    growth: f32,
    damping: f32,
    /// The threshold at Temper 0, and how far full Temper lowers it.
    threshold: f32,
    temper_shift: f32,
    /// A full-velocity hit's impulse.
    strike: f32,
}

impl RingLaw {
    fn tick(&self, ring: &mut Ring, sample_rate: f32, drive: &Drive) -> f32 {
        let q = drive.charge;
        let rise = if self.power == 2 { q * q } else { q };
        let hz = self.hz * drive.pitch * (1.0 + self.drop * rise);
        let w = (TAU * hz / sample_rate).min(1.0);
        let over = q - threshold(self.threshold, self.temper_shift, drive);
        let eps = if over > 0.0 { self.growth } else { self.gain } * over - self.damping;
        ring.tick(w, eps, drive.neighbour)
    }
}

const KICK: RingLaw = RingLaw {
    hz: 46.0,
    drop: 2.6,
    power: 2,
    gain: 0.05,
    growth: 0.9,
    damping: 0.004,
    threshold: 0.85,
    temper_shift: 0.45,
    strike: 1.6,
};

const SNARE_BODY: RingLaw = RingLaw {
    hz: 185.0,
    drop: 0.25,
    power: 1,
    gain: 0.06,
    growth: 0.6,
    damping: 0.02,
    threshold: 0.85,
    temper_shift: 0.4,
    strike: 1.4,
};

/// The snare's wires: their centre and Q, and how dense they get.
const WIRES_HZ: f32 = 3500.0;
const WIRES_Q: f32 = 0.7;
/// The body's and the wires' shares of the snare; they sum to one.
const SNARE_BODY_MIX: f32 = 0.65;
const WIRES_DENSITY: f32 = 14_000.0;
/// How much the wires answer the body's motion rather than the charge alone.
const WIRES_FOLLOW_BODY: f32 = 0.8;

/// The clap: the spark's threshold in charge, its membrane, refractory period and pulse width,
/// and the noise each spark opens.
const CLAP_THRESHOLD: f32 = 0.32;
const CLAP_TEMPER_SHIFT: f32 = 0.12;
const CLAP_MEMBRANE_S: f32 = 0.009;
const CLAP_REFRACTORY_S: f32 = 0.0045;
const CLAP_PULSE_S: f32 = 0.0003;
const CLAP_BURST_S: f32 = 0.006;
const CLAP_HZ: f32 = 1300.0;
const CLAP_Q: f32 = 1.4;
const CLAP_DENSITY: f32 = 9000.0;
/// Where the click's high-pass sits, to take the pulse train's DC away.
const CLICK_HZ: f32 = 300.0;

pub(crate) struct Kick {
    ring: Ring,
}

pub(crate) struct Snare {
    body: Ring,
    wires: Dust,
}

pub(crate) struct Clap {
    spark: Spark,
    noise: Dust,
    burst: f32,
    click_low: f32,
    leak: f32,
    refractory: u32,
    width: u32,
    burst_decay: f32,
    click_alpha: f32,
}

/// A cell's model, its engines and their state. Held inline, never boxed: a model change arrives
/// as automation inside `process`, where nothing may allocate.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Voice {
    Silent,
    Kick(Kick),
    Snare(Snare),
    Clap(Clap),
}

impl Voice {
    /// The voice for `model`, its noise seeded from `seed`.
    pub fn new(model: ModelId, seed: u32, sample_rate: f32) -> Self {
        let mut voice = match model {
            ModelId::DRAINING_KICK => Self::Kick(Kick {
                ring: Ring::default(),
            }),
            ModelId::WIRED_SNARE => Self::Snare(Snare {
                body: Ring::default(),
                wires: Dust::new(seed),
            }),
            ModelId::SPARK_CLAP => Self::Clap(Clap {
                spark: Spark::default(),
                noise: Dust::new(seed),
                burst: 0.0,
                click_low: 0.0,
                leak: 0.0,
                refractory: 0,
                width: 2,
                burst_decay: 0.0,
                click_alpha: 0.0,
            }),
            _ => Self::Silent,
        };
        voice.tune(sample_rate);
        voice
    }

    /// The Leak a cell takes with this model, in milliseconds.
    pub fn default_leak_ms(model: ModelId) -> f32 {
        match model {
            ModelId::DRAINING_KICK => 700.0,
            ModelId::WIRED_SNARE => 450.0,
            _ => 500.0,
        }
    }

    fn tune(&mut self, sample_rate: f32) {
        let dt = 1.0 / sample_rate;
        match self {
            Self::Silent | Self::Kick(_) => {}
            Self::Snare(snare) => snare.wires.tune(WIRES_HZ, WIRES_Q, sample_rate),
            Self::Clap(clap) => {
                clap.noise.tune(CLAP_HZ, CLAP_Q, sample_rate);
                clap.leak = (dt / CLAP_MEMBRANE_S).min(1.0);
                clap.width = ((CLAP_PULSE_S * sample_rate) as u32).max(2);
                clap.refractory = ((CLAP_REFRACTORY_S * sample_rate) as u32).max(clap.width + 2);
                clap.burst_decay = (dt / CLAP_BURST_S).min(1.0);
                clap.click_alpha = (TAU * CLICK_HZ * dt).min(1.0);
            }
        }
    }

    /// A hit at `velocity`, 0…1; the cell has already added its charge.
    pub fn strike(&mut self, velocity: f32) {
        match self {
            Self::Silent => {}
            Self::Kick(kick) => kick.ring.strike(KICK.strike * velocity),
            Self::Snare(snare) => snare.body.strike(SNARE_BODY.strike * velocity),
            Self::Clap(clap) => clap.spark.prime(),
        }
    }

    /// One raw sample, within [-1, 1].
    pub fn tick(&mut self, drive: &Drive, sample_rate: f32) -> f32 {
        match self {
            Self::Silent => 0.0,
            Self::Kick(kick) => KICK.tick(&mut kick.ring, sample_rate, drive),
            Self::Snare(snare) => {
                let body = SNARE_BODY.tick(&mut snare.body, sample_rate, drive);
                // The body's motion spills into the wires: they thicken while it swings.
                let swing = (0.5 * snare.body.position().abs()).min(1.0);
                let density = WIRES_DENSITY
                    * (drive.charge * (1.0 - WIRES_FOLLOW_BODY + WIRES_FOLLOW_BODY * swing)
                        + 0.3 * drive.neighbour.abs().min(1.0));
                let wires = snare.wires.tick(density, sample_rate);
                SNARE_BODY_MIX * body + (1.0 - SNARE_BODY_MIX) * wires
            }
            Self::Clap(clap) => {
                let t = drive.temper;
                let spark_threshold = (CLAP_THRESHOLD + THRESHOLD_REACH * drive.threshold
                    - CLAP_TEMPER_SHIFT * t
                    + JITTER_REACH * t * drive.jitter)
                    .clamp(0.05, 0.95);
                let input = drive.charge / spark_threshold + 0.5 * drive.neighbour.abs();
                let (pulse, fired) = clap
                    .spark
                    .tick(input, clap.leak, clap.refractory, clap.width);
                clap.burst = if fired {
                    1.0
                } else {
                    flush(clap.burst - clap.burst * clap.burst_decay)
                };
                let click = pulse - clap.click_low;
                clap.click_low = flush(clap.click_low + click * clap.click_alpha);
                let noise = clap
                    .noise
                    .tick(CLAP_DENSITY * (0.35 + 0.65 * clap.burst), sample_rate);
                0.85 * noise * (0.3 + 0.7 * clap.burst) + 0.15 * click.clamp(-1.0, 1.0)
            }
        }
    }

    /// Back to rest: every engine's state zero, its noise reseeded.
    pub fn clear(&mut self) {
        match self {
            Self::Silent => {}
            Self::Kick(kick) => kick.ring.clear(),
            Self::Snare(snare) => {
                snare.body.clear();
                snare.wires.clear();
            }
            Self::Clap(clap) => {
                clap.spark.clear();
                clap.noise.clear();
                clap.burst = 0.0;
                clap.click_low = 0.0;
            }
        }
    }
}
