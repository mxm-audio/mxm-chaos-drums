//! The models' engine networks: how each turns its cell's charge into each engine's operating point.
//!
//! Every voice returns a raw sample within [-1, 1], its engines' mix weights summing to at most
//! one; the cell multiplies it by its charge, which is what bounds a live cell by its charge.
//! The constants are starting points, to be settled by listening.

use core::f32::consts::TAU;

use mxm_dsp::delay::AgeLine;
use mxm_dsp::guard::flush;
use mxm_rng::Xorshift32;

use crate::engine::{Dust, Fold, Ring, Spark, Strand, StrandDrive};
use crate::fmath::exp2;
use crate::model::ModelId;
use crate::routes::CONTROLS;

/// The delay lines a cell owns for its model's loops.
pub(crate) type Lines = [AgeLine; 2];

/// What a cell gives its voice each sample.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Drive {
    /// The cell's charge, 0…1.
    pub charge: f32,
    /// The cell's Threshold, −1…+1, routes included.
    pub threshold: f32,
    /// The kit's Temper, 0…1.
    pub temper: f32,
    /// The cell's seeded slow jitter, −1…+1; the voice scales it by Temper.
    pub jitter: f32,
    /// What the neighbours and a touch put in, already scaled.
    pub neighbour: f32,
    /// The charge flowing in this sample, per second, scaled: the current a held note or a bus
    /// pushes through the drum. It is what starts an unstruck ring or loop.
    pub supply: f32,
    /// The frequency ratio from Coarse, Fine, tuning and Pitch routes.
    pub pitch: f32,
    /// The general controls, −1…+1, routes included; zero is the model's own setting.
    pub controls: [f32; CONTROLS],
}

impl Drive {
    /// Control `number` (1…12).
    fn control(&self, number: usize) -> f32 {
        self.controls[number - 1]
    }

    /// Control `number` as a factor of `octaves` either way, widened by [`CONTROL_REACH`].
    fn octaves(&self, number: usize, octaves: f32) -> f32 {
        let control = self.control(number);
        if control == 0.0 {
            1.0
        } else {
            exp2(CONTROL_REACH * octaves * control)
        }
    }
}

/// How far every model control reaches past the drum it starts as: each `octaves` a control is
/// written with is its drum-like span, and the control goes this many times as far, out to where the
/// drum stops being one (the owner, 2026-11-03: the inspiration "has a more crazy range"). Every
/// engine clamps what it cannot take (a band below 0.45 of the rate, a loop's damping and length, a
/// ring's state), so a reach is never a stability question.
const CONTROL_REACH: f32 = 2.5;

/// How far the Threshold parameter moves a threshold: far enough that any model can be tipped into
/// sustaining itself, or breaking up, at Temper 0. And how far jitter moves it at full Temper.
const THRESHOLD_REACH: f32 = 0.8;
const JITTER_REACH: f32 = 0.45;

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
    /// One sample, with Drop, Drive and Damping read from controls 3, 7 and 9.
    fn tick(&self, ring: &mut Ring, sample_rate: f32, drive: &Drive) -> f32 {
        let q = drive.charge;
        let rise = if self.power == 2 { q * q } else { q };
        let drop = self.drop * drive.octaves(3, 1.0);
        let hz = self.hz * drive.pitch * (1.0 + drop * rise);
        let w = (TAU * hz / sample_rate).min(1.0);
        let over = q - threshold(self.threshold, self.temper_shift, drive);
        let slope = if over > 0.0 {
            self.growth * drive.octaves(7, 1.5)
        } else {
            self.gain
        };
        let eps = slope * over - self.damping * drive.octaves(9, 2.0);
        ring.tick(w, eps, drive.neighbour + drive.supply)
    }

    /// A hit's impulse, with Snap from control 6: half an octave of it either way, widened.
    fn strike(&self, ring: &mut Ring, velocity: f32, controls: &[f32; CONTROLS]) {
        ring.strike(self.strike * velocity * exp2(CONTROL_REACH * 0.5 * controls[5]));
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

const RAW_RING: RingLaw = RingLaw {
    hz: 110.0,
    drop: 0.5,
    power: 1,
    gain: 0.04,
    growth: 0.9,
    damping: 0.006,
    threshold: 0.6,
    temper_shift: 0.4,
    strike: 1.5,
};

/// The snare's wires: their centre and Q, and how dense they get.
const WIRES_HZ: f32 = 3500.0;
const WIRES_Q: f32 = 0.7;
const WIRES_DENSITY: f32 = 14_000.0;
/// How much the wires answer the body's motion rather than the charge alone.
const WIRES_FOLLOW_BODY: f32 = 0.8;
/// The body's share of the snare at Body 0; the wires take the rest.
const SNARE_BODY_MIX: f32 = 0.65;

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
/// Where a click's high-pass sits, to take a pulse train's DC away.
const CLICK_HZ: f32 = 300.0;

/// The raw spark: its threshold, membrane, refractory period and pulse width.
const SPARK_THRESHOLD: f32 = 0.3;
const SPARK_TEMPER_SHIFT: f32 = 0.12;
const SPARK_MEMBRANE_S: f32 = 0.004;
const SPARK_REFRACTORY_S: f32 = 0.001;
const SPARK_PULSE_S: f32 = 0.00025;

/// The raw fold: its clock, its natural ratio to it, and the charge at which the map turns chaotic.
const FOLD_HZ: f32 = 110.0;
const FOLD_RATIO: f32 = 1.618;
const FOLD_THRESHOLD: f32 = 0.6;
const FOLD_TEMPER_SHIFT: f32 = 0.35;

/// The raw loop: its pitch, second tap, gain law, damping and the burst a hit feeds in.
const LOOP_HZ: f32 = 220.0;
const LOOP_SECOND_RATIO: f32 = 1.41;
const LOOP_REST_GAIN: f32 = 0.99;
const LOOP_GAIN_BELOW: f32 = 0.2;
const LOOP_GROWTH: f32 = 0.35;
const LOOP_GAIN_MAX: f32 = 1.25;
const LOOP_THRESHOLD: f32 = 0.7;
const LOOP_TEMPER_SHIFT: f32 = 0.35;
const LOOP_DAMPING: f32 = 0.3;
const LOOP_BURST_S: f32 = 0.0015;

/// The raw dust: its centre, Q and density at full charge.
const DUST_HZ: f32 = 2000.0;
const DUST_Q: f32 = 0.7;
const DUST_DENSITY: f32 = 20_000.0;

/// A band's tuning, kept so it is recomputed only when its control moves.
#[derive(Clone, Copy, Debug, Default)]
struct Tuned {
    centre: f32,
    q: f32,
}

impl Tuned {
    fn retune(&mut self, dust: &mut Dust, centre: f32, q: f32, sample_rate: f32) {
        if centre != self.centre || q != self.q {
            dust.tune(centre, q, sample_rate);
            self.centre = centre;
            self.q = q;
        }
    }
}

/// A click: a spike train with its DC taken away.
#[derive(Clone, Copy, Debug, Default)]
struct Click {
    low: f32,
}

impl Click {
    fn tick(&mut self, pulse: f32, alpha: f32) -> f32 {
        let click = pulse - self.low;
        self.low = flush(self.low + click * alpha);
        click.clamp(-1.0, 1.0)
    }
}

pub(crate) struct Kick {
    ring: Ring,
}

pub(crate) struct Snare {
    body: Ring,
    wires: Dust,
    tuned: Tuned,
}

pub(crate) struct Clap {
    spark: Spark,
    noise: Dust,
    tuned: Tuned,
    burst: f32,
    click: Click,
}

pub(crate) struct RawSpark {
    spark: Spark,
    click: Click,
}

pub(crate) struct RawLoop {
    strand: Strand,
    seed: u32,
    noise: Xorshift32,
    burst_left: u32,
    burst_level: f32,
}

pub(crate) struct RawDust {
    dust: Dust,
    tuned: Tuned,
}

/// A cell's model, its engines and their state. Held inline, never boxed: a model change arrives
/// as automation inside `process`, where nothing may allocate.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Voice {
    Silent,
    Kick(Kick),
    Snare(Snare),
    Clap(Clap),
    RawRing(Ring),
    RawSpark(RawSpark),
    RawFold(Fold),
    RawLoop(RawLoop),
    RawDust(RawDust),
}

impl Voice {
    /// The voice for `model`, its noise seeded from `seed`, at rest.
    pub fn new(model: ModelId, seed: u32) -> Self {
        match model {
            ModelId::DRAINING_KICK => Self::Kick(Kick {
                ring: Ring::default(),
            }),
            ModelId::WIRED_SNARE => Self::Snare(Snare {
                body: Ring::default(),
                wires: Dust::new(seed),
                tuned: Tuned::default(),
            }),
            ModelId::SPARK_CLAP => Self::Clap(Clap {
                spark: Spark::default(),
                noise: Dust::new(seed),
                tuned: Tuned::default(),
                burst: 0.0,
                click: Click::default(),
            }),
            ModelId::RAW_RING => Self::RawRing(Ring::default()),
            ModelId::RAW_SPARK => Self::RawSpark(RawSpark {
                spark: Spark::default(),
                click: Click::default(),
            }),
            ModelId::RAW_FOLD => Self::RawFold(Fold::default()),
            ModelId::RAW_LOOP => Self::RawLoop(RawLoop {
                strand: Strand::default(),
                seed,
                noise: Xorshift32::new(seed),
                burst_left: 0,
                burst_level: 0.0,
            }),
            ModelId::RAW_DUST => Self::RawDust(RawDust {
                dust: Dust::new(seed),
                tuned: Tuned::default(),
            }),
            _ => Self::Silent,
        }
    }

    /// The Leak a cell takes with this model, as an amount (`curves::leak_ms`): exact constants, so
    /// a default is the same on every platform. 0.47 is about 600 ms, 0.4 about 370, 0.5 about 775
    /// and 0.42 about 430.
    pub fn default_leak(model: ModelId) -> f32 {
        match model {
            ModelId::DRAINING_KICK => 0.47,
            ModelId::WIRED_SNARE => 0.4,
            ModelId::RAW_FOLD | ModelId::RAW_LOOP => 0.5,
            _ => 0.42,
        }
    }

    /// A hit at `velocity`, 0…1; the cell has already added its charge.
    pub fn strike(&mut self, velocity: f32, controls: &[f32; CONTROLS], sample_rate: f32) {
        match self {
            Self::Silent | Self::RawFold(_) | Self::RawDust(_) => {}
            Self::Kick(kick) => KICK.strike(&mut kick.ring, velocity, controls),
            Self::Snare(snare) => SNARE_BODY.strike(&mut snare.body, velocity, controls),
            Self::RawRing(ring) => RAW_RING.strike(ring, velocity, controls),
            Self::Clap(clap) => clap.spark.prime(),
            Self::RawSpark(raw) => raw.spark.prime(),
            Self::RawLoop(raw) => {
                let snap = exp2(CONTROL_REACH * controls[5]);
                raw.burst_left = ((LOOP_BURST_S * snap * sample_rate) as u32).max(1);
                raw.burst_level = velocity;
            }
        }
    }

    /// One raw sample, within [-1, 1].
    pub fn tick(&mut self, drive: &Drive, sample_rate: f32, lines: &mut Lines) -> f32 {
        let dt = 1.0 / sample_rate;
        match self {
            Self::Silent => 0.0,
            Self::Kick(kick) => KICK.tick(&mut kick.ring, sample_rate, drive),
            Self::RawRing(ring) => RAW_RING.tick(ring, sample_rate, drive),
            Self::Snare(snare) => {
                snare.tuned.retune(
                    &mut snare.wires,
                    WIRES_HZ * drive.octaves(1, 1.0),
                    WIRES_Q,
                    sample_rate,
                );
                let body = SNARE_BODY.tick(&mut snare.body, sample_rate, drive);
                // The body's motion spills into the wires: they thicken while it swings.
                let swing = (0.5 * snare.body.position().abs()).min(1.0);
                let density = WIRES_DENSITY
                    * (drive.charge * (1.0 - WIRES_FOLLOW_BODY + WIRES_FOLLOW_BODY * swing)
                        + 0.3 * drive.neighbour.abs().min(1.0));
                let wires = snare.wires.tick(density, sample_rate);
                // Body, widened, runs from all wires to all drum.
                let mix = (SNARE_BODY_MIX + 0.3 * CONTROL_REACH * drive.control(2)).clamp(0.0, 1.0);
                mix * body + (1.0 - mix) * wires
            }
            Self::Clap(clap) => {
                clap.tuned.retune(
                    &mut clap.noise,
                    CLAP_HZ * drive.octaves(1, 1.0),
                    CLAP_Q,
                    sample_rate,
                );
                let spark_threshold = threshold(CLAP_THRESHOLD, CLAP_TEMPER_SHIFT, drive);
                let input = drive.charge / spark_threshold + 0.5 * drive.neighbour.abs();
                let width = ((CLAP_PULSE_S * sample_rate) as u32).max(2);
                let refractory = ((CLAP_REFRACTORY_S * drive.octaves(8, 1.0) * sample_rate) as u32)
                    .max(width + 2);
                let leak = (dt / CLAP_MEMBRANE_S).min(1.0);
                let (pulse, fired) = clap.spark.tick(input, leak, refractory, width);
                let decay = (dt / (CLAP_BURST_S * drive.octaves(6, -1.0))).min(1.0);
                clap.burst = if fired {
                    1.0
                } else {
                    flush(clap.burst - clap.burst * decay)
                };
                let click = clap.click.tick(pulse, (TAU * CLICK_HZ * dt).min(1.0));
                let density = CLAP_DENSITY * drive.octaves(5, 1.0) * (0.35 + 0.65 * clap.burst);
                let noise = clap.noise.tick(density, sample_rate);
                0.85 * noise * (0.3 + 0.7 * clap.burst) + 0.15 * click
            }
            Self::RawSpark(raw) => {
                let spark_threshold = threshold(SPARK_THRESHOLD, SPARK_TEMPER_SHIFT, drive);
                let input = drive.charge / spark_threshold + 0.5 * drive.neighbour.abs();
                let width = ((SPARK_PULSE_S * drive.octaves(6, -1.0) * sample_rate) as u32).max(2);
                let refractory = ((SPARK_REFRACTORY_S * sample_rate) as u32).max(width + 2);
                let leak = (dt / (SPARK_MEMBRANE_S * drive.octaves(5, -2.0))).min(1.0);
                let (pulse, _) = raw.spark.tick(input, leak, refractory, width);
                let corner = CLICK_HZ * drive.octaves(1, 3.0);
                raw.click.tick(pulse, (TAU * corner * dt).min(1.0))
            }
            Self::RawFold(fold) => {
                let forcing = FOLD_HZ * drive.pitch * dt;
                let natural = forcing * FOLD_RATIO * drive.octaves(4, 1.0);
                let onset = threshold(FOLD_THRESHOLD, FOLD_TEMPER_SHIFT, drive);
                let kick =
                    drive.octaves(7, 1.0) * drive.charge / onset + 0.5 * drive.neighbour.abs();
                fold.tick(natural.min(0.45), forcing.min(0.45), kick)
            }
            Self::RawLoop(raw) => {
                let age = sample_rate / (LOOP_HZ * drive.pitch);
                let spread = drive.control(4);
                let over = drive.charge - threshold(LOOP_THRESHOLD, LOOP_TEMPER_SHIFT, drive);
                let gain = if over > 0.0 {
                    LOOP_REST_GAIN + LOOP_GROWTH * drive.octaves(7, 1.0) * over
                } else {
                    LOOP_REST_GAIN + LOOP_GAIN_BELOW * over
                };
                let input = if raw.burst_left > 0 {
                    raw.burst_left -= 1;
                    raw.burst_level * raw.noise.bipolar_f32()
                } else {
                    0.0
                };
                raw.strand.tick(
                    &mut lines[0],
                    &StrandDrive {
                        age,
                        second_age: age * LOOP_SECOND_RATIO * exp2(CONTROL_REACH * 0.5 * spread),
                        second: 0.25 * (spread + 1.0),
                        gain: gain.clamp(0.0, LOOP_GAIN_MAX),
                        damping: LOOP_DAMPING * drive.octaves(9, 1.0),
                        input: input
                            + 0.3 * drive.neighbour
                            + 0.1 * drive.supply * raw.noise.bipolar_f32(),
                    },
                )
            }
            Self::RawDust(raw) => {
                let centre = DUST_HZ * drive.octaves(1, 2.0);
                raw.tuned.retune(
                    &mut raw.dust,
                    centre,
                    DUST_Q * drive.octaves(4, 2.0),
                    sample_rate,
                );
                let density = DUST_DENSITY * drive.octaves(5, 2.0) * drive.charge
                    + 4000.0 * drive.neighbour.abs().min(1.0);
                raw.dust.tick(density, sample_rate)
            }
        }
    }

    /// Back to rest: every engine's state zero, its noise reseeded, its lines forgotten.
    pub fn clear(&mut self, lines: &mut Lines) {
        match self {
            Self::Silent => {}
            Self::Kick(kick) => kick.ring.clear(),
            Self::RawRing(ring) => ring.clear(),
            Self::Snare(snare) => {
                snare.body.clear();
                snare.wires.clear();
            }
            Self::Clap(clap) => {
                clap.spark.clear();
                clap.noise.clear();
                clap.burst = 0.0;
                clap.click = Click::default();
            }
            Self::RawSpark(raw) => {
                raw.spark.clear();
                raw.click = Click::default();
            }
            Self::RawFold(fold) => fold.clear(),
            Self::RawLoop(raw) => {
                raw.strand.clear(&mut lines[0]);
                raw.noise = Xorshift32::new(raw.seed);
                raw.burst_left = 0;
            }
            Self::RawDust(raw) => raw.dust.clear(),
        }
    }
}
