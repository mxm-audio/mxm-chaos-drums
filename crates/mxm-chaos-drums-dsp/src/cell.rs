//! A cell: one drum, its charge, and the law that charge follows.

use mxm_dsp::delay::AgeLine;
use mxm_dsp::guard::flush;
use mxm_modulation::standard;
use mxm_rng::{SplitMix64, Xorshift32};

use crate::curves::{self, LEAK_OCTAVES};
use crate::engine::strand::LOWEST_HZ;
use crate::fmath;
use crate::model::ModelId;
use crate::routes::{self, CONTROLS, CellMods, CellTarget, ROUTES, Route, Sources};
use crate::voice::{Drive, Lines, Voice};

/// Below this a cell's charge is exactly zero: about −90 dB, so every unfed cell comes to rest.
pub const CHARGE_FLOOR: f32 = 3e-5;
/// The declick window: a transition's jump decays to exact zero within it.
pub const DECLICK_S: f32 = 0.005;
/// Supply just above zero: a held note at full velocity fills an empty cell's time constant in
/// this many seconds…
const FILL_SLOWEST_S: f32 = 10.0;
/// …and at full, a thousandth of it, 10 ms: `log2(0.001)`, written out, since no logarithm runs here.
const FILL_LOG2_RANGE: f32 = -9.965_784;
/// Hunger at full spends charge at this rate per unit of output power, per second.
const HUNGER_FULL: f32 = 80.0;
/// The jitter's wander: a one-pole low-pass on seeded noise, at this corner.
const JITTER_HZ: f32 = 8.0;
/// A full touch feeds this much of the cell's own output back into its charge, per second.
const TOUCH_FEED: f32 = 20.0;
/// How strongly the charge flowing in drives the engines, per unit of charge per second.
const SUPPLY_DRIVE: f32 = 0.02;
/// A full touch's skin: seeded noise, low-passed here, and a low hum at this pitch.
const SKIN_NOISE_HZ: f32 = 600.0;
const SKIN_HUM_HZ: f32 = 50.0;
const SKIN_LEVEL: f32 = 0.3;

/// What else powers a cell with no note.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Bus {
    #[default]
    Off,
    A,
    B,
}

/// One cell's settings: the brief's per-cell parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellParams {
    pub model: ModelId,
    /// Semitones, −48…+48: four octaves either way, from a subsonic thump to a piercing tone.
    pub coarse: i8,
    /// Cents, −100…+100.
    pub fine: f32,
    /// The general controls, −1…+1; zero is the model's own setting.
    pub controls: [f32; CONTROLS],
    /// The packet a full-velocity note-on adds, as a share of the ceiling, 0…1.
    pub strike: f32,
    /// How fast a held note or a bus fills the cell, 0…1; 0 is none.
    pub supply: f32,
    /// How fast sounding spends charge, 0…1.
    pub hunger: f32,
    /// The time to drain at rest, 0…1 along [`curves::leak_ms`]: 20 ms to 30 s, never infinite.
    pub leak: f32,
    /// The model's threshold moved, −1…+1.
    pub threshold: f32,
    /// The cell's share in Flow and Coupling, 0…1.
    pub link: f32,
    /// How strongly its touch key acts, 0…1.
    pub touch: f32,
    pub bus: Bus,
    /// Linear gain after the cell.
    pub level: f32,
    /// −1 (left) … +1 (right).
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    /// 0 is none; 1…16.
    pub choke_group: u8,
    /// 0 is the main pair; 1…16 an individual output.
    pub output: u8,
    pub routes: [Route<CellTarget>; ROUTES],
}

impl CellParams {
    /// The brief's defaults for `model`.
    #[must_use]
    pub fn new(model: ModelId) -> Self {
        Self {
            model,
            coarse: 0,
            fine: 0.0,
            controls: [0.0; CONTROLS],
            strike: 0.6,
            supply: 0.3,
            hunger: 0.5,
            leak: Voice::default_leak(model),
            threshold: 0.0,
            link: 1.0,
            touch: 0.5,
            bus: Bus::Off,
            level: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            choke_group: 0,
            output: 0,
            routes: [Route::default(); ROUTES],
        }
    }
}

/// The settings a cell plays with this sample: its parameters with its routes applied.
#[derive(Clone, Copy, Debug, Default)]
struct Effective {
    fill_rate: f32,
    hunger_rate: f32,
    leak_rate: f32,
    threshold: f32,
    link: f32,
    touch: f32,
    strike: f32,
    controls: [f32; CONTROLS],
    semitones: f32,
    level: f32,
    pan: f32,
}

/// The velocity curve: `v·k / (1 + (k − 1)·v)` with `k = 4^dynamics`, so 0 is linear, +1 brings
/// soft hits up and −1 holds them down, and 0 and full velocity never move.
#[must_use]
pub fn velocity_curve(velocity: f32, dynamics: f32) -> f32 {
    let v = velocity.clamp(0.0, 1.0);
    let k = fmath::exp2(2.0 * dynamics.clamp(-1.0, 1.0));
    v * k / (1.0 + (k - 1.0) * v)
}

/// A stream's seed from the kit's Seed, the cell and the stream's slot.
fn stream_seed(kit_seed: u32, cell: usize, slot: u64) -> u32 {
    let index = (u64::from(kit_seed) << 16) | ((cell as u64) << 4) | slot;
    SplitMix64::mix(index) as u32
}

pub(crate) struct Cell {
    index: usize,
    params: CellParams,
    sample_rate: f32,
    dt: f32,
    routed: bool,
    /// The parameters changed since the effective settings were last computed.
    dirty: bool,
    eff: Effective,
    /// The pitch ratio, and the semitones it was computed for.
    pitch: f32,
    pitch_for: f32,

    charge: f32,
    gate: bool,
    velocity: f32,
    /// The standard's Velocity and Random sources for the last hit.
    velocity_source: f32,
    random_source: f32,
    tuning: f32,
    touch_held: bool,
    firmness: f32,
    /// Choked: no Flow, no Coupling, no bus, no charge, until its own strike or touch, or a reset.
    latched: bool,
    /// The declick offset, and the last live output it bridges from.
    offset: f32,
    live: f32,

    voice: Voice,
    lines: Lines,
    kit_seed: u32,
    random_rng: Xorshift32,
    jitter_rng: Xorshift32,
    jitter: f32,
    skin_rng: Xorshift32,
    skin: f32,
    hum: f32,
    /// At rest: no charge, no gate, no touch, no offset, every engine cleared. Ticking a resting,
    /// unfed cell changes nothing.
    resting: bool,
}

impl Cell {
    /// A cell at rest. Allocates its delay lines: call when the kit is made, never in `process`.
    pub fn new(params: CellParams, sample_rate: f32, kit_seed: u32, index: usize) -> Self {
        let length = (sample_rate / LOWEST_HZ) as usize + 4;
        let mut cell = Self {
            index,
            params,
            sample_rate,
            dt: 1.0 / sample_rate,
            routed: false,
            dirty: true,
            eff: Effective::default(),
            pitch: 1.0,
            pitch_for: 0.0,
            charge: 0.0,
            gate: false,
            velocity: 0.0,
            velocity_source: 0.0,
            random_source: 0.0,
            tuning: 0.0,
            touch_held: false,
            firmness: 0.0,
            latched: false,
            offset: 0.0,
            live: 0.0,
            voice: Voice::new(params.model, stream_seed(kit_seed, index, 1)),
            lines: [AgeLine::new(length), AgeLine::new(length)],
            kit_seed,
            random_rng: Xorshift32::new(1),
            jitter_rng: Xorshift32::new(1),
            jitter: 0.0,
            skin_rng: Xorshift32::new(1),
            skin: 0.0,
            hum: 0.0,
            resting: true,
        };
        cell.set_params(params);
        cell.reset(kit_seed);
        cell
    }

    /// New settings. A new model starts from rest with the cell's charge, the jump declicked.
    pub fn set_params(&mut self, params: CellParams) {
        let model_changed = params.model != self.params.model;
        self.params = params;
        self.routed = routes::any_in_use(&params.routes);
        self.dirty = true;
        if model_changed {
            self.bridge();
            self.voice = Voice::new(params.model, stream_seed(self.kit_seed, self.index, 1));
            for line in &mut self.lines {
                line.clear();
            }
        }
        self.prepare(&Sources::default());
    }

    pub fn params(&self) -> &CellParams {
        &self.params
    }

    /// The jump from the last live output to whatever comes next goes into the declick offset.
    fn bridge(&mut self) {
        self.offset = (self.offset + self.live).clamp(-1.0, 1.0);
        self.live = 0.0;
    }

    /// Exact rest and silence, every stream back to its seed from `kit_seed`, unlatched.
    pub fn reset(&mut self, kit_seed: u32) {
        self.restart(kit_seed);
        self.offset = 0.0;
        self.live = 0.0;
        self.resting = true;
    }

    /// From rest at this sample, every stream from `kit_seed`, the old output bridged by the offset:
    /// a transport reset.
    pub fn restart(&mut self, kit_seed: u32) {
        self.kit_seed = kit_seed;
        self.bridge();
        self.charge = 0.0;
        self.gate = false;
        self.touch_held = false;
        self.latched = false;
        self.tuning = 0.0;
        self.velocity = 0.0;
        self.velocity_source = 0.0;
        self.random_source = 0.0;
        self.voice = Voice::new(self.params.model, stream_seed(self.kit_seed, self.index, 1));
        self.voice.clear(&mut self.lines);
        self.random_rng = Xorshift32::new(stream_seed(self.kit_seed, self.index, 2));
        self.jitter_rng = Xorshift32::new(stream_seed(self.kit_seed, self.index, 3));
        self.skin_rng = Xorshift32::new(stream_seed(self.kit_seed, self.index, 4));
        self.jitter = 0.0;
        self.skin = 0.0;
        self.hum = 0.0;
    }

    /// Immediate exact silence, latched: a panic.
    pub fn panic(&mut self, kit_seed: u32) {
        self.restart(kit_seed);
        self.offset = 0.0;
        self.live = 0.0;
        self.latched = true;
        self.resting = true;
    }

    /// A choke: the gate and the touch close, the charge empties, the engines stop, the jump is
    /// declicked, and the cell latches.
    pub fn choke(&mut self) {
        self.bridge();
        self.charge = 0.0;
        self.gate = false;
        self.touch_held = false;
        self.latched = true;
        self.voice.clear(&mut self.lines);
    }

    /// A hit's own sources, before its settings are read: the standard's Velocity for `velocity`,
    /// 0…1, and one Random draw.
    pub fn stage_hit(&mut self, velocity: f32) {
        self.velocity_source = standard::velocity(velocity);
        self.random_source = standard::random(self.random_rng.unit_f32());
    }

    /// A hit at `curved`, its velocity through the kit's curve, its sources staged and its settings
    /// prepared; `gate` keeps it held.
    pub fn strike(&mut self, curved: f32, gate: bool) {
        self.latched = false;
        self.charge = (self.charge + self.eff.strike * curved).min(1.0);
        self.voice
            .strike(curved, &self.eff.controls, self.sample_rate);
        self.velocity = curved;
        self.tuning = 0.0;
        self.gate = gate;
        self.resting = false;
    }

    pub fn release(&mut self) {
        self.gate = false;
    }

    pub fn touch_on(&mut self, velocity: f32) {
        self.latched = false;
        self.touch_held = true;
        self.firmness = velocity.clamp(0.0, 1.0);
        self.resting = false;
    }

    pub fn touch_off(&mut self) {
        self.touch_held = false;
    }

    pub fn tune(&mut self, semitones: f32) {
        self.tuning = mxm_dsp::guard::finite_or(semitones, 0.0).clamp(-48.0, 48.0);
    }

    /// Apply this sample's routes to the parameters; with none in use, only after a change.
    pub fn prepare(&mut self, sources: &Sources) {
        if !self.routed && !self.dirty {
            return;
        }
        self.dirty = false;
        let p = &self.params;
        let mods = if self.routed {
            routes::cell_mods(&p.routes, sources, self.velocity_source, self.random_source)
        } else {
            CellMods::default()
        };
        let supply = (p.supply + mods.supply).clamp(0.0, 1.0);
        self.eff.fill_rate = if supply > 0.0 {
            // The fill time runs geometrically from the slowest to the fastest.
            1.0 / (FILL_SLOWEST_S * fmath::exp2(FILL_LOG2_RANGE * supply))
        } else {
            0.0
        };
        self.eff.hunger_rate = HUNGER_FULL * (p.hunger + mods.hunger).clamp(0.0, 1.0);
        let leak = (p.leak + mods.leak_octaves / LEAK_OCTAVES).clamp(0.0, 1.0);
        self.eff.leak_rate = 1000.0 / curves::leak_ms(leak);
        self.eff.threshold = (p.threshold + mods.threshold).clamp(-1.0, 1.0);
        self.eff.link = (p.link + mods.link).clamp(0.0, 1.0);
        self.eff.touch = (p.touch + mods.touch).clamp(0.0, 1.0);
        self.eff.strike = (p.strike + mods.strike).clamp(0.0, 1.0);
        for (out, (base, add)) in self
            .eff
            .controls
            .iter_mut()
            .zip(p.controls.iter().zip(mods.controls))
        {
            *out = (base + add).clamp(-1.0, 1.0);
        }
        self.eff.semitones = f32::from(p.coarse.clamp(-48, 48))
            + p.fine.clamp(-100.0, 100.0) / 100.0
            + mods.pitch_semitones;
        self.eff.level = p.level.max(0.0) * standard::amplitude_factor(mods.level);
        self.eff.pan = (p.pan + mods.pan).clamp(-1.0, 1.0);
    }

    /// How strongly the held touch acts: its key's velocity, its Touch, and the channel pressure.
    fn touch_strength(&self, pressure: f32) -> f32 {
        if self.touch_held {
            self.eff.touch * self.firmness * 0.5 * (1.0 + pressure.clamp(0.0, 1.0))
        } else {
            0.0
        }
    }

    /// The cell's link this sample: a held touch bridges it harder to its neighbours.
    pub fn link(&self, pressure: f32) -> f32 {
        if self.latched {
            0.0
        } else {
            (self.eff.link + self.touch_strength(pressure)).min(1.0)
        }
    }

    /// What it asks of the pool this sample: a held note, or its bus, fills it toward the ceiling.
    pub fn demand(&self, buses: [f32; 2]) -> f32 {
        if self.latched {
            return 0.0;
        }
        let bus = self.bus_level(buses);
        let feed = if self.gate {
            self.velocity.max(bus)
        } else {
            bus
        };
        self.eff.fill_rate * feed * (1.0 - self.charge) * self.dt
    }

    fn bus_level(&self, buses: [f32; 2]) -> f32 {
        match self.params.bus {
            Bus::Off => 0.0,
            Bus::A => buses[0].clamp(0.0, 1.0),
            Bus::B => buses[1].clamp(0.0, 1.0),
        }
    }

    pub fn charge(&self) -> f32 {
        self.charge
    }

    pub fn is_latched(&self) -> bool {
        self.latched
    }

    /// The last live output: what its neighbours hear.
    pub fn live(&self) -> f32 {
        self.live
    }

    pub fn level(&self) -> f32 {
        self.eff.level
    }

    pub fn pan(&self) -> f32 {
        self.eff.pan
    }

    /// At rest, and nothing about to feed it.
    pub fn is_at_rest(&self, buses: [f32; 2]) -> bool {
        self.resting && (self.latched || self.eff.fill_rate == 0.0 || self.bus_level(buses) == 0.0)
    }

    /// One sample: `inflow` from the pool and the neighbours, `heard` the neighbours' sound.
    /// Returns the output, its live part (`|live| ≤ charge`) plus the declick offset.
    pub fn tick(&mut self, inflow: f32, heard: f32, temper: f32, pressure: f32) -> f32 {
        let strength = self.touch_strength(pressure);
        let feedback = strength * TOUCH_FEED * self.live.abs() * (1.0 - self.charge) * self.dt;
        let flowing = if self.latched {
            0.0
        } else {
            (inflow + feedback).max(0.0)
        };
        self.charge = if self.latched {
            0.0
        } else {
            (self.charge + inflow + feedback).clamp(0.0, 1.0)
        };

        let step = self.dt / DECLICK_S;
        self.offset = if self.offset.abs() <= step {
            0.0
        } else {
            self.offset - step * self.offset.signum()
        };

        if self.charge == 0.0 && !self.gate && !self.touch_held {
            if !self.resting {
                self.voice.clear(&mut self.lines);
                self.live = 0.0;
                self.resting = self.offset == 0.0;
            }
            return self.offset;
        }
        self.resting = false;

        let noise = self.jitter_rng.bipolar_f32();
        self.jitter = flush(
            self.jitter
                + (noise - self.jitter) * (core::f32::consts::TAU * JITTER_HZ * self.dt).min(1.0),
        );
        let skin = if strength > 0.0 {
            let noise = self.skin_rng.bipolar_f32();
            self.skin = flush(
                self.skin
                    + (noise - self.skin)
                        * (core::f32::consts::TAU * SKIN_NOISE_HZ * self.dt).min(1.0),
            );
            self.hum += SKIN_HUM_HZ * self.dt;
            self.hum -= self.hum.floor();
            strength * SKIN_LEVEL * (0.6 * self.skin + 0.4 * fmath::sin_turns(self.hum))
        } else {
            0.0
        };

        let semitones = self.eff.semitones + self.tuning;
        if semitones != self.pitch_for {
            self.pitch = fmath::semitones(semitones);
            self.pitch_for = semitones;
        }
        let drive = Drive {
            charge: self.charge,
            threshold: self.eff.threshold,
            temper,
            jitter: self.jitter,
            neighbour: if self.latched { skin } else { heard + skin },
            supply: (SUPPLY_DRIVE * flowing / self.dt).min(2.0),
            pitch: self.pitch,
            controls: self.eff.controls,
        };
        let live = self.charge * self.voice.tick(&drive, self.sample_rate, &mut self.lines);

        let spent =
            (self.eff.hunger_rate * live * live + self.eff.leak_rate * self.charge) * self.dt;
        self.charge = (self.charge - spent).max(0.0);
        if self.charge < CHARGE_FLOOR {
            self.charge = 0.0;
        }
        self.live = live;
        live + self.offset
    }
}
