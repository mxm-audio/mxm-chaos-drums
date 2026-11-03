//! The models' engine networks: how each turns its cell's charge into each engine's operating point.
//!
//! Every voice returns a raw sample within [-1, 1], its engines' mix weights summing to at most
//! one; the cell multiplies it by its charge, which is what bounds a live cell by its charge.
//! The constants are starting points, to be settled by listening.

use core::f32::consts::TAU;

use mxm_dsp::delay::AgeLine;
use mxm_dsp::guard::flush;
use mxm_rng::Xorshift32;

use crate::engine::svf::Band;
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

    /// As [`Drive::octaves`], `down` octaves below and `up` above: for a Tone whose top would leave
    /// hearing at the full reach.
    fn octaves_split(&self, number: usize, down: f32, up: f32) -> f32 {
        let octaves = if self.control(number) > 0.0 { up } else { down };
        self.octaves(number, octaves)
    }
}

/// How far up Tone reaches on the three models whose band left hearing at the full reach (the owner,
/// 2026-11-03: "pull those three ranges back"), so all of its travel is heard: the snare's wires to
/// 14 kHz, a spark's click corner to 4 kHz, past which the click filters itself away, a dust's band
/// to 16 kHz. Down, each keeps the full reach.
const WIRES_TONE_UP: f32 = 0.8;
const CLICK_TONE_UP: f32 = 1.5;
const DUST_TONE_UP: f32 = 1.2;

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

/// The tom: the kick's law an octave and more up, its pitch falling less far as it drains.
const TOM: RingLaw = RingLaw {
    hz: 110.0,
    drop: 0.9,
    power: 2,
    gain: 0.05,
    growth: 0.9,
    damping: 0.009,
    threshold: 0.85,
    temper_shift: 0.45,
    strike: 1.5,
};

/// A hand drum: a ring body, and a Fold slap at the strike that dies within milliseconds.
struct HandLaw {
    body: RingLaw,
    /// The slap's clock, and its natural rate against the clock, as multiples of the body's pitch.
    slap_ratio: f32,
    natural_ratio: f32,
    /// The slap's share of the drum at Body 0, how long it rings at Snap 0, and how hard it kicks
    /// the fold: past 1 the circle map is chaotic, which is what makes a slap a slap.
    slap_mix: f32,
    slap_s: f32,
    slap_kick: f32,
}

/// A hand drum's state: its body, its slap, and how much of the slap is left.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Hand {
    body: Ring,
    slap: Fold,
    left: f32,
}

impl HandLaw {
    /// One sample: Drop, Drive and Damping on the body as a ring's, Drive also kicking the slap
    /// harder, Snap shortening it and Body taking the slap's share.
    fn tick(&self, hand: &mut Hand, sample_rate: f32, drive: &Drive) -> f32 {
        let body = self.body.tick(&mut hand.body, sample_rate, drive);
        let dt = 1.0 / sample_rate;
        let decay = (dt / (self.slap_s * drive.octaves(6, -1.0))).min(1.0);
        hand.left = flush(hand.left - hand.left * decay);
        let slap = if hand.left > 0.0 {
            let forcing = (self.body.hz * self.slap_ratio * drive.pitch * dt).min(0.45);
            let natural = (forcing * self.natural_ratio).min(0.45);
            let kick = self.slap_kick * drive.octaves(7, 1.0) * (0.5 + drive.charge);
            hand.slap.tick(natural, forcing, kick)
        } else {
            0.0
        };
        let mix = (self.slap_mix - 0.3 * CONTROL_REACH * drive.control(2)).clamp(0.0, 1.0);
        (1.0 - mix) * body + mix * hand.left * slap
    }

    /// A hit: the body's impulse with Snap, and the slap from its start.
    fn strike(&self, hand: &mut Hand, velocity: f32, controls: &[f32; CONTROLS]) {
        self.body.strike(&mut hand.body, velocity, controls);
        hand.left = velocity.clamp(0.0, 1.0);
        hand.slap = Fold::default();
    }
}

const CONGA: HandLaw = HandLaw {
    body: RingLaw {
        hz: 196.0,
        drop: 0.2,
        power: 1,
        gain: 0.05,
        growth: 0.7,
        damping: 0.012,
        threshold: 0.85,
        temper_shift: 0.4,
        strike: 1.3,
    },
    slap_ratio: 2.7,
    natural_ratio: 1.618,
    slap_mix: 0.35,
    slap_s: 0.012,
    slap_kick: 1.6,
};

/// The bongo: a smaller, higher, shorter conga with a brighter slap.
const BONGO: HandLaw = HandLaw {
    body: RingLaw {
        hz: 330.0,
        drop: 0.25,
        power: 1,
        gain: 0.05,
        growth: 0.7,
        damping: 0.02,
        threshold: 0.85,
        temper_shift: 0.4,
        strike: 1.2,
    },
    slap_ratio: 3.1,
    natural_ratio: 1.618,
    slap_mix: 0.4,
    slap_s: 0.008,
    slap_kick: 1.8,
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

/// How far up a hat's and a cymbal's Tone reach, so all of its travel is heard: a hat's band an
/// octave, to 16 kHz at most (the leaking hats'), a cymbal's noise an octave and a quarter, to
/// 15.4 kHz at most (the ride's). Down, each keeps the full reach.
const HAT_TONE_UP: f32 = 0.4;
const CYMBAL_TONE_UP: f32 = 0.5;

/// A hat's six metal partials, as octaves over its lowest: inharmonic, a struck plate's spread
/// rather than a string's, in the manner of the six-oscillator hats electronic kits built.
const HAT_OCTAVES: [f32; 6] = [0.0, 0.557, 0.858, 1.094, 1.307, 1.552];

/// A band filter, retuned only when its centre or Q moves.
#[derive(Clone, Copy, Debug, Default)]
struct TunedBand {
    band: Band,
    centre: f32,
    q: f32,
}

impl TunedBand {
    fn tick(&mut self, x: f32, centre: f32, q: f32, sample_rate: f32) -> f32 {
        if centre != self.centre || q != self.q {
            self.band.tune(centre, q, sample_rate);
            self.centre = centre;
            self.q = q;
        }
        self.band.tick(x)
    }
}

/// A hat: Fold phases at inharmonic rates through a high band, under Dust noise in the same band.
/// The clock kicks each phase by the circle map as charge rises: gently, a metal that roughens;
/// hard, the folded hat's, a tone that breaks into noise. Closed or open is how fast the cell
/// leaks.
struct HatLaw {
    /// The lowest partial, and the band the metal and the noise sound through.
    hz: f32,
    band_hz: f32,
    band_q: f32,
    /// The noise's share at Body 0, and how dense it is at full charge.
    noise_mix: f32,
    density: f32,
    /// How hard the clock kicks the phases at full charge: past 1, chaos.
    kick: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct Hat {
    phases: [Fold; 6],
    /// Each partial's rate a sample, and the pitch and Spread it was worked out for.
    rates: [f32; 6],
    rates_for: (f32, f32, f32),
    band: TunedBand,
    noise: Dust,
    tuned: Tuned,
}

impl Hat {
    fn new(seed: u32) -> Self {
        Self {
            phases: [Fold::default(); 6],
            rates: [0.0; 6],
            rates_for: (f32::NAN, f32::NAN, f32::NAN),
            band: TunedBand::default(),
            noise: Dust::new(seed),
            tuned: Tuned::default(),
        }
    }

    fn clear(&mut self) {
        self.phases = [Fold::default(); 6];
        self.band.band.clear();
        self.noise.clear();
    }
}

impl HatLaw {
    /// One sample: Tone moves the band, Body trades the noise for the metal, Spread widens the
    /// partials, Grain thickens the noise and Drive kicks the phases harder.
    fn tick(&self, hat: &mut Hat, sample_rate: f32, drive: &Drive) -> f32 {
        let spread = drive.control(4);
        if hat.rates_for != (drive.pitch, spread, sample_rate) {
            let widen = exp2(CONTROL_REACH * 0.4 * spread);
            let base = self.hz * drive.pitch / sample_rate;
            for (rate, octaves) in hat.rates.iter_mut().zip(HAT_OCTAVES) {
                *rate = (base * exp2(widen * octaves)).min(0.45);
            }
            hat.rates_for = (drive.pitch, spread, sample_rate);
        }
        let kick = self.kick * drive.octaves(7, 1.0) * drive.charge;
        let mut metal = 0.0;
        for (phase, &rate) in hat.phases.iter_mut().zip(&hat.rates) {
            metal += phase.tick(rate, 0.5 * rate, kick);
        }
        let centre = self.band_hz * drive.octaves_split(1, 1.0, HAT_TONE_UP);
        let metal = hat
            .band
            .tick(metal / 6.0, centre, self.band_q, sample_rate)
            .clamp(-1.0, 1.0);
        hat.tuned
            .retune(&mut hat.noise, centre, self.band_q, sample_rate);
        let density = self.density * drive.octaves(5, 1.5) * drive.charge;
        let noise = hat.noise.tick(density, sample_rate);
        let mix = (self.noise_mix - 0.3 * CONTROL_REACH * drive.control(2)).clamp(0.0, 1.0);
        (1.0 - mix) * metal + mix * noise
    }
}

/// The closed and the open hat: one law, a fast or a slow leak.
const LEAKING_HAT: HatLaw = HatLaw {
    hz: 3300.0,
    band_hz: 8000.0,
    band_q: 0.9,
    noise_mix: 0.45,
    density: 16_000.0,
    kick: 0.25,
};

/// The folded hat: the phases kicked hard, so a full one is noise and a draining one turns to tone.
const FOLDED_HAT: HatLaw = HatLaw {
    hz: 2600.0,
    band_hz: 7000.0,
    band_q: 0.8,
    noise_mix: 0.15,
    density: 8_000.0,
    kick: 3.0,
};

/// A cymbal: Loop metal fed by Dust for as long as the charge lasts, the crash's with a dense burst
/// at the strike, the ride's with a bell of two Rings over it.
struct CymbalLaw {
    /// The loop's pitch, its second tap's ratio and share, its gain and its damping.
    hz: f32,
    second_ratio: f32,
    second: f32,
    gain: f32,
    damping: f32,
    /// The noise that feeds it: its centre, Q and density at full charge, how much more the
    /// strike's burst adds and how fast that dies.
    noise_hz: f32,
    noise_q: f32,
    density: f32,
    burst: f32,
    burst_s: f32,
    /// How much of the noise is heard as well as feeding the loop, at Body 0.
    noise_mix: f32,
    /// The bell: its pitch, its second partial's ratio and its share at Body 0; none without one.
    bell: Option<(f32, f32, f32)>,
}

#[derive(Clone, Debug)]
pub(crate) struct Cymbal {
    strand: Strand,
    noise: Dust,
    tuned: Tuned,
    burst: f32,
    bell: [Ring; 2],
}

impl Cymbal {
    fn new(seed: u32) -> Self {
        Self {
            strand: Strand::default(),
            noise: Dust::new(seed),
            tuned: Tuned::default(),
            burst: 0.0,
            bell: [Ring::default(); 2],
        }
    }

    fn clear(&mut self, lines: &mut Lines) {
        self.strand.clear(&mut lines[0]);
        self.noise.clear();
        self.burst = 0.0;
        self.bell = [Ring::default(); 2];
    }
}

impl CymbalLaw {
    /// One sample: Tone moves the noise, Body trades the noise (the ride's bell) for the metal,
    /// Spread spreads the metal, Grain thickens the noise and Damping darkens the loop.
    fn tick(&self, cymbal: &mut Cymbal, sample_rate: f32, drive: &Drive, lines: &mut Lines) -> f32 {
        let dt = 1.0 / sample_rate;
        cymbal.burst = flush(cymbal.burst - cymbal.burst * (dt / self.burst_s).min(1.0));
        let centre = self.noise_hz * drive.octaves_split(1, 2.0, CYMBAL_TONE_UP);
        cymbal
            .tuned
            .retune(&mut cymbal.noise, centre, self.noise_q, sample_rate);
        let density =
            self.density * drive.octaves(5, 1.5) * (drive.charge + self.burst * cymbal.burst);
        let noise = cymbal.noise.tick(density, sample_rate);
        let age = sample_rate / (self.hz * drive.pitch);
        let spread = drive.control(4);
        let metal = cymbal.strand.tick(
            &mut lines[0],
            &StrandDrive {
                age,
                second_age: age * self.second_ratio * exp2(CONTROL_REACH * 0.5 * spread),
                second: (self.second + 0.2 * CONTROL_REACH * spread).clamp(0.0, 1.0),
                gain: self.gain,
                damping: self.damping * drive.octaves(9, 1.0),
                input: 0.25 * noise + 0.3 * drive.neighbour,
            },
        );
        let body = drive.control(2);
        match self.bell {
            None => {
                let mix = (self.noise_mix - 0.3 * CONTROL_REACH * body).clamp(0.0, 1.0);
                (1.0 - mix) * metal + mix * noise
            }
            Some((hz, ratio, share)) => {
                let w = |hz: f32| (TAU * hz * drive.pitch * dt).min(1.0);
                let bell = 0.5
                    * (cymbal.bell[0].tick(w(hz), -0.002, 0.0)
                        + cymbal.bell[1].tick(w(hz * ratio), -0.003, 0.0));
                let bell_mix = (share + 0.3 * CONTROL_REACH * body).clamp(0.0, 1.0);
                let rest = 1.0 - bell_mix;
                bell_mix * bell + rest * ((1.0 - self.noise_mix) * metal + self.noise_mix * noise)
            }
        }
    }

    /// A hit: the burst from its start, and the bell struck.
    fn strike(&self, cymbal: &mut Cymbal, velocity: f32) {
        cymbal.burst = velocity.clamp(0.0, 1.0);
        if self.bell.is_some() {
            for ring in &mut cymbal.bell {
                ring.strike(1.2 * velocity);
            }
        }
    }
}

const LOOP_CYMBAL: CymbalLaw = CymbalLaw {
    hz: 420.0,
    second_ratio: 1.53,
    second: 0.45,
    gain: 0.985,
    damping: 0.18,
    noise_hz: 6000.0,
    noise_q: 0.6,
    density: 12_000.0,
    burst: 1.0,
    burst_s: 0.05,
    noise_mix: 0.35,
    bell: None,
};

/// The crash: a lower, wider loop and a dense burst at the strike that washes out.
const LOOP_CRASH: CymbalLaw = CymbalLaw {
    hz: 300.0,
    second_ratio: 1.71,
    second: 0.5,
    gain: 0.99,
    damping: 0.15,
    noise_hz: 5000.0,
    noise_q: 0.5,
    density: 14_000.0,
    burst: 3.0,
    burst_s: 0.3,
    noise_mix: 0.4,
    bell: None,
};

/// The ride: a darker, steadier loop with little noise, and a bell of two Rings over it.
const RINGING_RIDE: CymbalLaw = CymbalLaw {
    hz: 520.0,
    second_ratio: 1.41,
    second: 0.35,
    gain: 0.98,
    damping: 0.25,
    noise_hz: 6500.0,
    noise_q: 0.7,
    density: 6_000.0,
    burst: 0.5,
    burst_s: 0.02,
    noise_mix: 0.2,
    bell: Some((760.0, 2.76, 0.3)),
};

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
    Tom(Ring),
    Conga(Hand),
    Bongo(Hand),
    Kick(Kick),
    Snare(Snare),
    Clap(Clap),
    RawRing(Ring),
    RawSpark(RawSpark),
    RawFold(Fold),
    RawLoop(RawLoop),
    RawDust(RawDust),
    ClosedHat(Hat),
    OpenHat(Hat),
    FoldedHat(Hat),
    Cymbal(Cymbal),
    Crash(Cymbal),
    Ride(Cymbal),
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
            ModelId::FALLING_TOM => Self::Tom(Ring::default()),
            ModelId::SLAPPED_CONGA => Self::Conga(Hand::default()),
            ModelId::SLAPPED_BONGO => Self::Bongo(Hand::default()),
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
            ModelId::LEAKING_CLOSED_HAT => Self::ClosedHat(Hat::new(seed)),
            ModelId::LEAKING_OPEN_HAT => Self::OpenHat(Hat::new(seed)),
            ModelId::FOLDED_HAT => Self::FoldedHat(Hat::new(seed)),
            ModelId::LOOP_CYMBAL => Self::Cymbal(Cymbal::new(seed)),
            ModelId::LOOP_CRASH => Self::Crash(Cymbal::new(seed)),
            ModelId::RINGING_RIDE => Self::Ride(Cymbal::new(seed)),
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
            ModelId::FALLING_TOM => 0.45,
            ModelId::SLAPPED_BONGO => 0.36,
            // About 42 ms and 223 ms, 40 dB down in about 0.18 s and 1 s: closed or open is how
            // fast it leaks.
            ModelId::LEAKING_CLOSED_HAT => 0.1,
            ModelId::LEAKING_OPEN_HAT => 0.33,
            ModelId::FOLDED_HAT => 0.35,
            // About 578 ms, 775 ms and 669 ms.
            ModelId::LOOP_CYMBAL => 0.46,
            ModelId::LOOP_CRASH => 0.5,
            ModelId::RINGING_RIDE => 0.48,
            _ => 0.42,
        }
    }

    /// A hit at `velocity`, 0…1; the cell has already added its charge.
    pub fn strike(&mut self, velocity: f32, controls: &[f32; CONTROLS], sample_rate: f32) {
        match self {
            Self::Silent
            | Self::RawFold(_)
            | Self::RawDust(_)
            | Self::ClosedHat(_)
            | Self::OpenHat(_)
            | Self::FoldedHat(_) => {}
            Self::Cymbal(cymbal) => LOOP_CYMBAL.strike(cymbal, velocity),
            Self::Crash(cymbal) => LOOP_CRASH.strike(cymbal, velocity),
            Self::Ride(cymbal) => RINGING_RIDE.strike(cymbal, velocity),
            Self::Kick(kick) => KICK.strike(&mut kick.ring, velocity, controls),
            Self::Snare(snare) => SNARE_BODY.strike(&mut snare.body, velocity, controls),
            Self::RawRing(ring) => RAW_RING.strike(ring, velocity, controls),
            Self::Tom(ring) => TOM.strike(ring, velocity, controls),
            Self::Conga(hand) => CONGA.strike(hand, velocity, controls),
            Self::Bongo(hand) => BONGO.strike(hand, velocity, controls),
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
            Self::Tom(ring) => TOM.tick(ring, sample_rate, drive),
            Self::Conga(hand) => CONGA.tick(hand, sample_rate, drive),
            Self::Bongo(hand) => BONGO.tick(hand, sample_rate, drive),
            Self::Snare(snare) => {
                snare.tuned.retune(
                    &mut snare.wires,
                    WIRES_HZ * drive.octaves_split(1, 1.0, WIRES_TONE_UP),
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
                let corner = CLICK_HZ * drive.octaves_split(1, 3.0, CLICK_TONE_UP);
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
                let centre = DUST_HZ * drive.octaves_split(1, 2.0, DUST_TONE_UP);
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
            Self::ClosedHat(hat) | Self::OpenHat(hat) => LEAKING_HAT.tick(hat, sample_rate, drive),
            Self::FoldedHat(hat) => FOLDED_HAT.tick(hat, sample_rate, drive),
            Self::Cymbal(cymbal) => LOOP_CYMBAL.tick(cymbal, sample_rate, drive, lines),
            Self::Crash(cymbal) => LOOP_CRASH.tick(cymbal, sample_rate, drive, lines),
            Self::Ride(cymbal) => RINGING_RIDE.tick(cymbal, sample_rate, drive, lines),
        }
    }

    /// Back to rest: every engine's state zero, its noise reseeded, its lines forgotten.
    pub fn clear(&mut self, lines: &mut Lines) {
        match self {
            Self::Silent => {}
            Self::Kick(kick) => kick.ring.clear(),
            Self::RawRing(ring) | Self::Tom(ring) => ring.clear(),
            Self::Conga(hand) | Self::Bongo(hand) => *hand = Hand::default(),
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
            Self::ClosedHat(hat) | Self::OpenHat(hat) | Self::FoldedHat(hat) => hat.clear(),
            Self::Cymbal(cymbal) | Self::Crash(cymbal) | Self::Ride(cymbal) => cymbal.clear(lines),
        }
    }
}
