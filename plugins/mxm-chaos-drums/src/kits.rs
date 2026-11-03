//! **The fifty factory kits** (the plan's P6, asked for on 2026-11-03): their design, the generator
//! that balances them and writes their files, and the checks that hold them. Tests only: what ships
//! is `presets/`, installed for the plugin's version.
//!
//! Every kit is on Init's role map, the drum machine's (36 kick, 37 snare, 38–40 low, mid and high
//! tom, 41–43 percussion, 44 rim, 45 clap, 46 and 47 closed and open hat, 48 cymbal, 49 cowbell, 50
//! clave, 51 shaker), so one beat plays across the whole bank. **The design is sparse**: a cell names
//! its model, Coarse, pan and only the controls it moves; everything else is Init, and a Leak it does
//! not name is its model's own. [`generated`] lays a kit over Init; the ignored
//! [`write_the_factory_kits`] measures its balance and writes the files, complete, so a shipped kit
//! never leans on a default; [`the_shipped_kits_are_their_designs`] compares them in memory.
//!
//! **Their Levels are the brief's balance** (the owner, 2026-11-03), as Init's: every cell equally
//! loud per hit by ITU-R BS.1770's K-weighting, a noise cell (Raw dust) or a cell a bus drones 6 dB
//! under it, the kick at 0 dB unless a quiet cell needs more than the Level's ceiling, when every
//! Level comes down together, by up to 6 dB ([`balance`]). The ignored
//! [`init_and_every_kit_are_balanced`] re-measures them and prints each kit's table, with the
//! pink-noise balance beside it.
//!
//! **The kits move only what their models read** ([`every_cell_moves_only_what_its_model_reads`]),
//! **every closed hat cuts its open hat**, and every cell sounds ([`every_cell_of_every_kit_sounds`]).

use std::path::Path;
use std::sync::LazyLock;

use mxm_chaos_drums_dsp::{
    self as dsp, CELLS, CellTarget, Event, EventKind, KitTarget, ModelId, ROUTES, available, curves,
};
use mxm_measure::spectrum::fft;
use mxm_preset::{Category, Instrument, Preset, Value};
use mxm_tempo::Division;
use nice_plug::params::Param;
use nice_plug::prelude::{Params, util};

use crate::params::{BusChoice, LFO_SYNC, LfoShapeChoice, MxmChaosDrumsParams, at, cell_ids};
use crate::routes::{KIT_TARGETS, KitSourceChoice, SourceChoice, cell_target_index};

use BusChoice::{A, B};
use Category::{Drone, Fx, Percussion};
use KitSourceChoice as K;
use LfoShapeChoice::{RampDown, SampleHold, Sine, Square, Triangle};
use SourceChoice::{Lfo1, Lfo2, Lfo3, Random, Velocity};

const KICK: ModelId = ModelId::DRAINING_KICK;
const SNARE: ModelId = ModelId::WIRED_SNARE;
const CLAP: ModelId = ModelId::SPARK_CLAP;
const RING: ModelId = ModelId::RAW_RING;
const SPARK: ModelId = ModelId::RAW_SPARK;
const FOLD: ModelId = ModelId::RAW_FOLD;
const LOOP: ModelId = ModelId::RAW_LOOP;
const DUST: ModelId = ModelId::RAW_DUST;

/// The general controls, by number (the brief's table).
const TONE: u8 = 1;
const BODY: u8 = 2;
const DROP: u8 = 3;
const SPREAD: u8 = 4;
const GRAIN: u8 = 5;
const SNAP: u8 = 6;
const DRIVE: u8 = 7;
const SPACING: u8 = 8;
const DAMPING: u8 = 9;

/// The closed and open hats' choke group, the same in every kit.
const HATS: i32 = 1;

/// A route aimed at control `k`.
const fn to(k: u8) -> CellTarget {
    CellTarget::Control(k - 1)
}

/// One cell of a kit. Whatever it does not name stays at Init.
#[derive(Clone, Copy)]
struct Key {
    model: ModelId,
    coarse: i32,
    pan: f32,
    /// `(control, plain value −1…+1)`, by number.
    controls: &'static [(u8, f32)],
    strike: Option<f32>,
    supply: Option<f32>,
    hunger: Option<f32>,
    /// Milliseconds; none is the model's own.
    leak_ms: Option<f32>,
    threshold: f32,
    touch: Option<f32>,
    bus: BusChoice,
    choke: i32,
    /// `(source, target, amount)`, route slots 1… in order.
    routes: &'static [(SourceChoice, CellTarget, f32)],
}

const fn k(model: ModelId, coarse: i32, pan: f32, controls: &'static [(u8, f32)]) -> Key {
    Key {
        model,
        coarse,
        pan,
        controls,
        strike: None,
        supply: None,
        hunger: None,
        leak_ms: None,
        threshold: 0.0,
        touch: None,
        bus: BusChoice::Off,
        choke: 0,
        routes: &[],
    }
}

impl Key {
    const fn strike(self, strike: f32) -> Self {
        Self {
            strike: Some(strike),
            ..self
        }
    }

    const fn supply(self, supply: f32) -> Self {
        Self {
            supply: Some(supply),
            ..self
        }
    }

    const fn hunger(self, hunger: f32) -> Self {
        Self {
            hunger: Some(hunger),
            ..self
        }
    }

    const fn leak(self, ms: f32) -> Self {
        Self {
            leak_ms: Some(ms),
            ..self
        }
    }

    const fn threshold(self, threshold: f32) -> Self {
        Self { threshold, ..self }
    }

    const fn touch(self, touch: f32) -> Self {
        Self {
            touch: Some(touch),
            ..self
        }
    }

    const fn bus(self, bus: BusChoice) -> Self {
        Self { bus, ..self }
    }

    const fn hats(self) -> Self {
        Self {
            choke: HATS,
            ..self
        }
    }

    const fn routes(self, routes: &'static [(SourceChoice, CellTarget, f32)]) -> Self {
        Self { routes, ..self }
    }
}

/// A kit-wide LFO the kit sets; the others stay at Init.
struct Lfo {
    lfo: usize,
    shape: LfoShapeChoice,
    rate: Rate,
}

enum Rate {
    Free(f32),
    Synced(Division),
}

const fn free(lfo: usize, shape: LfoShapeChoice, hz: f32) -> Lfo {
    Lfo {
        lfo,
        shape,
        rate: Rate::Free(hz),
    }
}

const fn synced(lfo: usize, shape: LfoShapeChoice, division: Division) -> Lfo {
    Lfo {
        lfo,
        shape,
        rate: Rate::Synced(division),
    }
}

struct Kit {
    name: &'static str,
    category: Category,
    temper: f32,
    flow: f32,
    coupling: f32,
    pool: f32,
    /// Milliseconds; none is Init's.
    recharge_ms: Option<f32>,
    bus: [f32; 2],
    seed: i32,
    dynamics: f32,
    lfos: &'static [Lfo],
    routes: &'static [(KitSourceChoice, KitTarget, f32)],
    keys: [Key; CELLS],
}

/// What a kit names nothing of: Init's kit-wide settings.
const INIT: Kit = Kit {
    name: "",
    category: Percussion,
    temper: 0.0,
    flow: 0.5,
    coupling: 0.5,
    pool: 1.0,
    recharge_ms: None,
    bus: [0.0; 2],
    seed: 1,
    dynamics: 0.0,
    lfos: &[],
    routes: &[],
    keys: [k(ModelId::NONE, 0, 0.0, &[]); CELLS],
};

/// **The fifty**, in the browser's order, five of each: steady, breathing, tuned, metal, noise,
/// drones, feedback, starved, moving and wild. Pans are the player's view: low tom left, high tom
/// right.
static KITS: [Kit; 50] = [
    // ---- Steady: Temper 0, a drum every time -------------------------------------------------
    // Short and tight: every body damped, every tail cut, for fast dry patterns.
    Kit {
        name: "Dry Cell",
        keys: [
            k(KICK, 0, 0.0, &[(DAMPING, 0.15), (DROP, -0.2), (SNAP, 0.2)])
                .hunger(0.65)
                .leak(350.0),
            k(SNARE, 0, 0.0, &[(BODY, 0.2), (TONE, 0.1), (DAMPING, 0.15)]).leak(250.0),
            k(RING, -5, -0.3, &[(DAMPING, 0.2)]).leak(300.0),
            k(RING, 0, 0.0, &[(DAMPING, 0.2)]).leak(300.0),
            k(RING, 5, 0.3, &[(DAMPING, 0.2)]).leak(300.0),
            k(FOLD, 0, -0.4, &[(DRIVE, -0.2)]).leak(250.0),
            k(FOLD, 5, 0.4, &[(DRIVE, -0.2)]).leak(250.0),
            k(LOOP, 7, 0.15, &[(DAMPING, 0.2)]).leak(250.0),
            k(SPARK, 0, -0.1, &[(SNAP, 0.3)]).leak(120.0),
            k(CLAP, 0, 0.05, &[(SNAP, 0.3), (SPACING, -0.1)]),
            k(DUST, 0, 0.25, &[(TONE, 0.1)]).leak(60.0).hats(),
            k(DUST, 0, 0.25, &[(TONE, 0.1)]).leak(450.0).hats(),
            k(LOOP, 12, -0.3, &[(DAMPING, 0.1)]).leak(1100.0),
            k(FOLD, 12, 0.35, &[]).leak(250.0),
            k(SPARK, 0, -0.35, &[(TONE, 0.15)]).leak(90.0),
            k(DUST, 5, 0.45, &[(GRAIN, -0.1)]).leak(120.0),
        ],
        ..INIT
    },
    // Low and long: the kick and toms tuned down and let ring, the noise darkened.
    Kit {
        name: "Deep Charge",
        keys: [
            k(KICK, -3, 0.0, &[(DAMPING, -0.25), (DROP, 0.3)])
                .supply(0.4)
                .leak(900.0),
            k(
                SNARE,
                -3,
                0.0,
                &[(TONE, -0.2), (BODY, 0.3), (DAMPING, -0.1)],
            ),
            k(RING, -9, -0.3, &[(DAMPING, -0.25)]).leak(800.0),
            k(RING, -4, 0.0, &[(DAMPING, -0.25)]).leak(800.0),
            k(RING, 1, 0.3, &[(DAMPING, -0.25)]).leak(800.0),
            k(FOLD, -5, -0.4, &[(DRIVE, -0.1)]),
            k(FOLD, 0, 0.4, &[(DRIVE, -0.1)]),
            k(LOOP, 0, 0.15, &[(DAMPING, 0.3)]).leak(700.0),
            k(SPARK, 0, -0.1, &[(TONE, -0.15)]),
            k(CLAP, 0, 0.05, &[(TONE, -0.25), (GRAIN, -0.1)]),
            k(DUST, 0, 0.25, &[(TONE, -0.15)]).leak(110.0).hats(),
            k(DUST, 0, 0.25, &[(TONE, -0.15)]).leak(900.0).hats(),
            k(LOOP, 7, -0.3, &[(DAMPING, 0.2)]).leak(2200.0),
            k(FOLD, 7, 0.35, &[]),
            k(SPARK, 0, -0.35, &[(TONE, -0.1)]).leak(140.0),
            k(DUST, 0, 0.45, &[(TONE, -0.2)]).leak(220.0),
        ],
        ..INIT
    },
    // Bright and crisp: a snappy kick, a cracking snare, hats and clave pushed up.
    Kit {
        name: "Bright Wire",
        keys: [
            k(KICK, 2, 0.0, &[(SNAP, 0.4), (DROP, 0.2), (DAMPING, 0.05)]),
            k(SNARE, 2, 0.0, &[(TONE, 0.25), (BODY, -0.2), (SNAP, 0.3)]),
            k(RING, -2, -0.3, &[(SNAP, 0.3)]),
            k(RING, 3, 0.0, &[(SNAP, 0.3)]),
            k(RING, 8, 0.3, &[(SNAP, 0.3)]),
            k(FOLD, 2, -0.4, &[(SPREAD, 0.1)]),
            k(FOLD, 7, 0.4, &[(SPREAD, 0.1)]),
            k(LOOP, 12, 0.15, &[(DAMPING, -0.2)]),
            k(SPARK, 0, -0.1, &[(TONE, 0.15), (SNAP, 0.2)]),
            k(CLAP, 0, 0.05, &[(TONE, 0.25), (GRAIN, 0.15)]),
            k(DUST, 0, 0.25, &[(TONE, 0.2), (SPREAD, -0.1)])
                .leak(70.0)
                .hats(),
            k(DUST, 0, 0.25, &[(TONE, 0.2), (SPREAD, -0.1)])
                .leak(650.0)
                .hats(),
            k(LOOP, 19, -0.3, &[(DAMPING, -0.15), (SPREAD, 0.2)]).leak(1800.0),
            k(FOLD, 14, 0.35, &[]),
            k(SPARK, 0, -0.35, &[(TONE, 0.2)]).leak(110.0),
            k(DUST, 7, 0.45, &[(TONE, 0.25)]).leak(170.0),
        ],
        ..INIT
    },
    // Gentle: softer strikes and attacks, soft hits brought up, for brushed, quiet playing.
    Kit {
        name: "Soft Current",
        dynamics: 0.4,
        keys: [
            k(KICK, 0, 0.0, &[(SNAP, -0.3), (DAMPING, -0.1)]).strike(0.5),
            k(SNARE, 0, 0.0, &[(SNAP, -0.3), (BODY, 0.1), (TONE, -0.1)]).strike(0.5),
            k(RING, -5, -0.3, &[(SNAP, -0.3)]).strike(0.5),
            k(RING, 0, 0.0, &[(SNAP, -0.3)]).strike(0.5),
            k(RING, 5, 0.3, &[(SNAP, -0.3)]).strike(0.5),
            k(FOLD, 0, -0.4, &[(DRIVE, -0.3)]).strike(0.45),
            k(FOLD, 5, 0.4, &[(DRIVE, -0.3)]).strike(0.45),
            k(LOOP, 7, 0.15, &[(DAMPING, 0.2)]),
            k(SPARK, 0, -0.1, &[(GRAIN, -0.2), (SNAP, -0.2)]),
            k(
                CLAP,
                0,
                0.05,
                &[(SNAP, -0.2), (GRAIN, -0.2), (SPACING, 0.15)],
            ),
            k(DUST, 0, 0.25, &[(GRAIN, -0.15), (TONE, -0.05)])
                .leak(90.0)
                .hats(),
            k(DUST, 0, 0.25, &[(GRAIN, -0.15), (TONE, -0.05)])
                .leak(750.0)
                .hats(),
            k(LOOP, 12, -0.3, &[(DAMPING, 0.1)]).leak(1600.0),
            k(FOLD, 12, 0.35, &[(DRIVE, -0.3)]),
            k(SPARK, 0, -0.35, &[(SNAP, -0.2)]).leak(125.0),
            k(DUST, 5, 0.45, &[(GRAIN, -0.2)]).leak(180.0),
        ],
        ..INIT
    },
    // Long tails: little hunger and slow leaks, so every drum rings out.
    Kit {
        name: "Long Fuse",
        keys: [
            k(KICK, 0, 0.0, &[(DAMPING, -0.3)]).hunger(0.3).leak(1800.0),
            k(SNARE, 0, 0.0, &[(DAMPING, -0.2)])
                .hunger(0.35)
                .leak(900.0),
            k(RING, -5, -0.3, &[(DAMPING, -0.35)])
                .hunger(0.3)
                .leak(1600.0),
            k(RING, 0, 0.0, &[(DAMPING, -0.35)])
                .hunger(0.3)
                .leak(1600.0),
            k(RING, 5, 0.3, &[(DAMPING, -0.35)])
                .hunger(0.3)
                .leak(1600.0),
            k(FOLD, 0, -0.4, &[]).leak(1400.0),
            k(FOLD, 5, 0.4, &[]).leak(1400.0),
            k(LOOP, 7, 0.15, &[(DAMPING, -0.3)]).leak(1500.0),
            k(SPARK, 0, -0.1, &[]).leak(300.0),
            k(CLAP, 0, 0.05, &[(SNAP, -0.3)]).leak(700.0),
            k(DUST, 0, 0.25, &[]).leak(120.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(2500.0).hats(),
            k(LOOP, 12, -0.3, &[(DAMPING, -0.3)]).leak(6000.0),
            k(FOLD, 12, 0.35, &[]).leak(1200.0),
            k(SPARK, 0, -0.35, &[]).leak(400.0),
            k(DUST, 5, 0.45, &[]).leak(500.0),
        ],
        ..INIT
    },
    // ---- Breathing: a little Temper, the grid starts to share --------------------------------
    // Init's drums with the first breath of Temper: hits begin to lean on their neighbours.
    Kit {
        name: "First Breath",
        temper: 0.2,
        flow: 0.4,
        coupling: 0.3,
        seed: 11,
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[]),
            k(RING, -5, -0.3, &[]).threshold(-0.1),
            k(RING, 0, 0.0, &[]).threshold(-0.1),
            k(RING, 5, 0.3, &[]).threshold(-0.1),
            k(FOLD, 0, -0.4, &[]),
            k(FOLD, 5, 0.4, &[]),
            k(LOOP, 7, 0.15, &[]),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // The sound leaks across the grid: each hit rings faintly in its neighbours.
    Kit {
        name: "Warm Lattice",
        temper: 0.3,
        flow: 0.3,
        coupling: 0.6,
        seed: 23,
        keys: [
            k(KICK, -1, 0.0, &[(DAMPING, -0.1)]),
            k(SNARE, 0, 0.0, &[(BODY, 0.15)]),
            k(RING, -6, -0.3, &[(DAMPING, -0.1)]),
            k(RING, -1, 0.0, &[(DAMPING, -0.1)]),
            k(RING, 4, 0.3, &[(DAMPING, -0.1)]),
            k(FOLD, -2, -0.4, &[]),
            k(FOLD, 3, 0.4, &[]),
            k(LOOP, 5, 0.15, &[(DAMPING, 0.1)]),
            k(SPARK, 0, -0.1, &[(TONE, -0.1)]),
            k(CLAP, 0, 0.05, &[(TONE, -0.1)]),
            k(DUST, 0, 0.25, &[(TONE, -0.1)]).leak(90.0).hats(),
            k(DUST, 0, 0.25, &[(TONE, -0.1)]).leak(800.0).hats(),
            k(LOOP, 10, -0.3, &[]).leak(1800.0),
            k(FOLD, 10, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 3, 0.45, &[(TONE, -0.1)]).leak(200.0),
        ],
        ..INIT
    },
    // Charge runs fast between cells and the pool is smaller: held notes pull on each other.
    Kit {
        name: "Shared Wire",
        temper: 0.25,
        flow: 0.85,
        coupling: 0.2,
        pool: 0.7,
        seed: 37,
        keys: [
            k(KICK, 0, 0.0, &[]).supply(0.5),
            k(SNARE, 0, 0.0, &[]).supply(0.5),
            k(RING, -5, -0.3, &[]).supply(0.5),
            k(RING, 0, 0.0, &[]).supply(0.5),
            k(RING, 5, 0.3, &[]).supply(0.5),
            k(FOLD, 0, -0.4, &[]).supply(0.5),
            k(FOLD, 5, 0.4, &[]).supply(0.5),
            k(LOOP, 7, 0.15, &[]).supply(0.5),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]).supply(0.5),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // Temper rises and falls over two bars: the kit is steady, then loose, then steady again.
    Kit {
        name: "Pulse Field",
        temper: 0.3,
        seed: 41,
        lfos: &[synced(1, Sine, Division::TwoBars)],
        routes: &[(K::Lfo1, KitTarget::Temper, 0.25)],
        keys: [
            k(KICK, 0, 0.0, &[(DAMPING, -0.05)]),
            k(SNARE, 0, 0.0, &[]),
            k(RING, -5, -0.3, &[]),
            k(RING, 0, 0.0, &[]),
            k(RING, 5, 0.3, &[]),
            k(FOLD, 0, -0.4, &[(SPREAD, 0.1)]),
            k(FOLD, 5, 0.4, &[(SPREAD, 0.1)]),
            k(LOOP, 7, 0.15, &[]),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // Soft hits into a loose grid: quiet playing that still drifts.
    Kit {
        name: "Quiet Storm",
        temper: 0.35,
        flow: 0.5,
        coupling: 0.4,
        dynamics: 0.3,
        seed: 53,
        keys: [
            k(KICK, 0, 0.0, &[(SNAP, -0.2)]).strike(0.5),
            k(SNARE, 0, 0.0, &[(SNAP, -0.2)]).strike(0.5),
            k(RING, -5, -0.3, &[]).strike(0.5),
            k(RING, 0, 0.0, &[]).strike(0.5),
            k(RING, 5, 0.3, &[]).strike(0.5),
            k(FOLD, 0, -0.4, &[]).strike(0.5),
            k(FOLD, 5, 0.4, &[]).strike(0.5),
            k(LOOP, 7, 0.15, &[]).strike(0.5),
            k(SPARK, 0, -0.1, &[]).strike(0.5),
            k(CLAP, 0, 0.05, &[(SPACING, 0.1)]).strike(0.5),
            k(DUST, 0, 0.25, &[(GRAIN, -0.1)])
                .strike(0.5)
                .leak(85.0)
                .hats(),
            k(DUST, 0, 0.25, &[(GRAIN, -0.1)])
                .strike(0.5)
                .leak(775.0)
                .hats(),
            k(LOOP, 12, -0.3, &[]).strike(0.5).leak(1600.0),
            k(FOLD, 12, 0.35, &[]).strike(0.5),
            k(SPARK, 0, -0.35, &[]).strike(0.5).leak(120.0),
            k(DUST, 5, 0.45, &[]).strike(0.5).leak(180.0),
        ],
        ..INIT
    },
    // ---- Tuned: the toms and percussion on scales --------------------------------------------
    // Six rings on a minor pentatonic, a melody under the beat.
    Kit {
        name: "Pentatone",
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[]),
            k(RING, -12, -0.4, &[(DAMPING, -0.15)]).leak(900.0),
            k(RING, -9, -0.2, &[(DAMPING, -0.15)]).leak(900.0),
            k(RING, -7, 0.0, &[(DAMPING, -0.15)]).leak(900.0),
            k(RING, -5, 0.2, &[(DAMPING, -0.15)]).leak(900.0),
            k(RING, -2, 0.4, &[(DAMPING, -0.15)]).leak(900.0),
            k(RING, 0, 0.1, &[(DAMPING, -0.15)]).leak(900.0),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(RING, 3, 0.35, &[(DAMPING, -0.15)]).leak(900.0),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // Low rings set just past their tipping point, so each one sings like a small gong.
    Kit {
        name: "Gong Steps",
        keys: [
            k(KICK, -2, 0.0, &[(DAMPING, -0.15)]),
            k(SNARE, 0, 0.0, &[(BODY, 0.2)]),
            k(RING, -14, -0.4, &[(DAMPING, -0.4), (DROP, -0.6)])
                .threshold(-0.2)
                .hunger(0.3)
                .leak(2000.0),
            k(RING, -10, -0.2, &[(DAMPING, -0.4), (DROP, -0.6)])
                .threshold(-0.2)
                .hunger(0.3)
                .leak(2000.0),
            k(RING, -7, 0.0, &[(DAMPING, -0.4), (DROP, -0.6)])
                .threshold(-0.2)
                .hunger(0.3)
                .leak(2000.0),
            k(RING, -2, 0.2, &[(DAMPING, -0.4), (DROP, -0.6)])
                .threshold(-0.2)
                .hunger(0.3)
                .leak(2000.0),
            k(RING, 3, 0.4, &[(DAMPING, -0.4), (DROP, -0.6)])
                .threshold(-0.2)
                .hunger(0.3)
                .leak(2000.0),
            k(RING, 5, 0.1, &[(DAMPING, -0.4), (DROP, -0.6)])
                .threshold(-0.2)
                .hunger(0.3)
                .leak(2000.0),
            k(SPARK, 0, -0.1, &[(TONE, -0.1)]),
            k(CLAP, 0, 0.05, &[(TONE, -0.1)]),
            k(DUST, 0, 0.25, &[(TONE, -0.1)]).leak(90.0).hats(),
            k(DUST, 0, 0.25, &[(TONE, -0.1)]).leak(1200.0).hats(),
            k(LOOP, 5, -0.3, &[(SPREAD, 0.4), (DAMPING, -0.2)]).leak(3500.0),
            k(FOLD, 10, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 2, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // Bright plucked loops in fifths and fourths, glassy and long.
    Kit {
        name: "Glass Tiers",
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[(TONE, 0.2)]),
            k(LOOP, 0, -0.4, &[(DAMPING, -0.5), (SPREAD, -1.0)]).leak(1200.0),
            k(LOOP, 5, -0.2, &[(DAMPING, -0.5), (SPREAD, -1.0)]).leak(1200.0),
            k(LOOP, 7, 0.0, &[(DAMPING, -0.5), (SPREAD, -1.0)]).leak(1200.0),
            k(LOOP, 12, 0.2, &[(DAMPING, -0.5), (SPREAD, -1.0)]).leak(1200.0),
            k(LOOP, 17, 0.4, &[(DAMPING, -0.5), (SPREAD, -1.0)]).leak(1200.0),
            k(LOOP, 19, 0.1, &[(DAMPING, -0.5), (SPREAD, -1.0)]).leak(1200.0),
            k(SPARK, 0, -0.1, &[(TONE, 0.15)]),
            k(CLAP, 0, 0.05, &[(TONE, 0.15)]),
            k(DUST, 0, 0.25, &[(TONE, 0.25)]).leak(80.0).hats(),
            k(DUST, 0, 0.25, &[(TONE, 0.25)]).leak(700.0).hats(),
            k(LOOP, 24, -0.3, &[(DAMPING, -0.4), (SPREAD, 0.3)]).leak(2500.0),
            k(FOLD, 19, 0.35, &[]),
            k(SPARK, 0, -0.35, &[(TONE, 0.2)]).leak(110.0),
            k(DUST, 7, 0.45, &[(TONE, 0.2)]).leak(170.0),
        ],
        ..INIT
    },
    // Toms whose pitch falls far as they drain: the classic electronic tom, here from charge.
    Kit {
        name: "Bent Toms",
        keys: [
            k(KICK, 0, 0.0, &[(DROP, 0.5)]),
            k(SNARE, 0, 0.0, &[(DROP, 0.4)]),
            k(RING, -10, -0.4, &[(DROP, 0.6)]).leak(700.0),
            k(RING, -5, -0.1, &[(DROP, 0.6)]).leak(700.0),
            k(RING, 0, 0.2, &[(DROP, 0.6)]).leak(700.0),
            k(RING, 3, -0.3, &[(DROP, 0.5)]).leak(600.0),
            k(RING, 7, 0.4, &[(DROP, 0.5)]).leak(600.0),
            k(RING, 10, 0.1, &[(DROP, 0.5)]).leak(600.0),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(RING, 14, 0.35, &[(DROP, 0.4)]).leak(400.0),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // Metal loops on a major chord, rung like a yard of oil drums.
    Kit {
        name: "Steel Yard",
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[(TONE, 0.1)]),
            k(LOOP, 0, -0.4, &[(SPREAD, 0.3), (DAMPING, -0.2)]).leak(1000.0),
            k(LOOP, 4, -0.2, &[(SPREAD, 0.3), (DAMPING, -0.2)]).leak(1000.0),
            k(LOOP, 7, 0.0, &[(SPREAD, 0.3), (DAMPING, -0.2)]).leak(1000.0),
            k(LOOP, 12, 0.2, &[(SPREAD, 0.3), (DAMPING, -0.2)]).leak(1000.0),
            k(LOOP, 16, 0.4, &[(SPREAD, 0.3), (DAMPING, -0.2)]).leak(1000.0),
            k(LOOP, 19, 0.1, &[(SPREAD, 0.3), (DAMPING, -0.2)]).leak(1000.0),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[(SPREAD, 0.2)]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[(SPREAD, 0.2)]).leak(900.0).hats(),
            k(LOOP, 24, -0.3, &[(SPREAD, 0.6)]).leak(2500.0),
            k(FOLD, 12, 0.35, &[(SPREAD, 0.3)]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // ---- Metal ---------------------------------------------------------------------------------
    // High, inharmonic loops for bells; the hats ring as well as hiss.
    Kit {
        name: "Rust Bells",
        keys: [
            k(KICK, 0, 0.0, &[(DAMPING, 0.05)]),
            k(SNARE, 0, 0.0, &[(TONE, 0.15)]),
            k(LOOP, 7, -0.4, &[(SPREAD, 0.6), (DAMPING, -0.3)]).leak(1400.0),
            k(LOOP, 10, -0.2, &[(SPREAD, 0.6), (DAMPING, -0.3)]).leak(1400.0),
            k(LOOP, 14, 0.0, &[(SPREAD, 0.6), (DAMPING, -0.3)]).leak(1400.0),
            k(LOOP, 17, 0.2, &[(SPREAD, 0.6), (DAMPING, -0.3)]).leak(1400.0),
            k(LOOP, 19, 0.4, &[(SPREAD, 0.6), (DAMPING, -0.3)]).leak(1400.0),
            k(LOOP, 24, 0.1, &[(SPREAD, 0.6), (DAMPING, -0.3)]).leak(1400.0),
            k(SPARK, 0, -0.1, &[(TONE, 0.1)]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[(SPREAD, 0.3), (TONE, 0.2)])
                .leak(90.0)
                .hats(),
            k(DUST, 0, 0.25, &[(SPREAD, 0.3), (TONE, 0.2)])
                .leak(1000.0)
                .hats(),
            k(LOOP, 19, -0.3, &[(SPREAD, 0.8)]).leak(3000.0),
            k(FOLD, 14, 0.35, &[(SPREAD, 0.4)]),
            k(SPARK, 0, -0.35, &[(TONE, 0.2)]).leak(120.0),
            k(DUST, 9, 0.45, &[(SPREAD, 0.2)]).leak(180.0),
        ],
        ..INIT
    },
    // Wide-spread, driven loops for toms and a crash that shimmers: a junkyard kit.
    Kit {
        name: "Sheet Metal",
        keys: [
            k(KICK, 0, 0.0, &[(DAMPING, 0.1), (DRIVE, 0.2)]),
            k(SNARE, 0, 0.0, &[(TONE, 0.3), (BODY, -0.3)]),
            k(LOOP, -5, -0.3, &[(SPREAD, 0.8), (DRIVE, 0.2)]).leak(900.0),
            k(LOOP, 0, 0.0, &[(SPREAD, 0.8), (DRIVE, 0.2)]).leak(900.0),
            k(LOOP, 5, 0.3, &[(SPREAD, 0.8), (DRIVE, 0.2)]).leak(900.0),
            k(FOLD, 0, -0.4, &[(SPREAD, 0.5), (DRIVE, 0.1)]),
            k(FOLD, 5, 0.4, &[(SPREAD, 0.5), (DRIVE, 0.1)]),
            k(LOOP, 10, 0.15, &[(SPREAD, 0.6)]),
            k(SPARK, 0, -0.1, &[(TONE, 0.2)]),
            k(CLAP, 0, 0.05, &[(TONE, 0.2)]),
            k(DUST, 0, 0.25, &[(TONE, 0.15), (SPREAD, 0.15)])
                .leak(85.0)
                .hats(),
            k(DUST, 0, 0.25, &[(TONE, 0.15), (SPREAD, 0.15)])
                .leak(900.0)
                .hats(),
            k(LOOP, 14, -0.3, &[(SPREAD, 0.9)]).leak(2500.0),
            k(FOLD, 12, 0.35, &[(SPREAD, 0.4)]),
            k(SPARK, 0, -0.35, &[(TONE, 0.25)]).leak(110.0),
            k(DUST, 5, 0.45, &[(SPREAD, 0.3)]).leak(180.0),
        ],
        ..INIT
    },
    // Small, short metal: a tiny high kick and toms like struck tins.
    Kit {
        name: "Tin Can",
        keys: [
            k(KICK, 7, 0.0, &[(DAMPING, 0.3)]).leak(250.0),
            k(SNARE, 5, 0.0, &[(BODY, -0.2), (TONE, 0.2)]).leak(250.0),
            k(LOOP, 5, -0.3, &[(DAMPING, 0.2), (SPREAD, 0.5)]).leak(250.0),
            k(LOOP, 10, 0.0, &[(DAMPING, 0.2), (SPREAD, 0.5)]).leak(250.0),
            k(LOOP, 15, 0.3, &[(DAMPING, 0.2), (SPREAD, 0.5)]).leak(250.0),
            k(FOLD, 12, -0.4, &[(SPREAD, -0.3), (DRIVE, -0.2)]).leak(200.0),
            k(FOLD, 17, 0.4, &[(SPREAD, -0.3), (DRIVE, -0.2)]).leak(200.0),
            k(LOOP, 19, 0.15, &[(DAMPING, 0.1), (SPREAD, 0.4)]).leak(250.0),
            k(SPARK, 0, -0.1, &[(TONE, 0.2), (SNAP, 0.3)]).leak(80.0),
            k(CLAP, 0, 0.05, &[(TONE, 0.3), (SNAP, 0.3)]),
            k(DUST, 0, 0.25, &[(TONE, 0.25), (SPREAD, 0.25)])
                .leak(50.0)
                .hats(),
            k(DUST, 0, 0.25, &[(TONE, 0.25), (SPREAD, 0.25)])
                .leak(350.0)
                .hats(),
            k(LOOP, 22, -0.3, &[(SPREAD, 0.7)]).leak(900.0),
            k(FOLD, 19, 0.35, &[(SPREAD, -0.2)]).leak(200.0),
            k(SPARK, 0, -0.35, &[(TONE, 0.3)]).leak(70.0),
            k(DUST, 9, 0.45, &[(TONE, 0.3)]).leak(100.0),
        ],
        ..INIT
    },
    // Ringing noise and metal that leaks into its neighbours, like rain on a roof.
    Kit {
        name: "Iron Rain",
        temper: 0.2,
        coupling: 0.5,
        seed: 97,
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[(BODY, -0.2)]),
            k(LOOP, -2, -0.3, &[(SPREAD, 0.5)]).leak(800.0),
            k(LOOP, 3, 0.0, &[(SPREAD, 0.5)]).leak(800.0),
            k(LOOP, 8, 0.3, &[(SPREAD, 0.5)]).leak(800.0),
            k(DUST, 0, -0.4, &[(SPREAD, 0.4)]).leak(250.0),
            k(DUST, 0, 0.4, &[(SPREAD, 0.4), (TONE, 0.15)]).leak(250.0),
            k(SPARK, 0, 0.15, &[(GRAIN, 0.2)]).leak(200.0),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[(GRAIN, 0.1)]),
            k(DUST, 0, 0.25, &[(SPREAD, 0.3)]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[(SPREAD, 0.3)]).leak(900.0).hats(),
            k(LOOP, 17, -0.3, &[(SPREAD, 0.7)]).leak(2500.0),
            k(FOLD, 12, 0.35, &[(SPREAD, 0.3)]),
            k(SPARK, 0, -0.35, &[(TONE, 0.2)]).leak(120.0),
            k(DUST, 5, 0.45, &[(SPREAD, 0.4), (GRAIN, -0.3)]).leak(250.0),
        ],
        ..INIT
    },
    // Long, bright, nearly pure plucks on a major pentatonic: a harp of loops.
    Kit {
        name: "Wire Harp",
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[]),
            k(LOOP, 0, -0.4, &[(DAMPING, -0.6), (SPREAD, -0.8)])
                .threshold(-0.1)
                .strike(0.7)
                .leak(2500.0),
            k(LOOP, 2, -0.2, &[(DAMPING, -0.6), (SPREAD, -0.8)])
                .threshold(-0.1)
                .strike(0.7)
                .leak(2500.0),
            k(LOOP, 4, 0.0, &[(DAMPING, -0.6), (SPREAD, -0.8)])
                .threshold(-0.1)
                .strike(0.7)
                .leak(2500.0),
            k(LOOP, 7, 0.2, &[(DAMPING, -0.6), (SPREAD, -0.8)])
                .threshold(-0.1)
                .strike(0.7)
                .leak(2500.0),
            k(LOOP, 9, 0.4, &[(DAMPING, -0.6), (SPREAD, -0.8)])
                .threshold(-0.1)
                .strike(0.7)
                .leak(2500.0),
            k(LOOP, 12, 0.1, &[(DAMPING, -0.6), (SPREAD, -0.8)])
                .threshold(-0.1)
                .strike(0.7)
                .leak(2500.0),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 24, -0.3, &[(DAMPING, -0.5)]).leak(4000.0),
            k(LOOP, 14, 0.35, &[(DAMPING, -0.6), (SPREAD, -0.8)]).leak(2500.0),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // ---- Noise ---------------------------------------------------------------------------------
    // Noise in place of the percussion: three bands of hiss, a crackling rim, a driven kick.
    Kit {
        name: "Static Field",
        keys: [
            k(KICK, 0, 0.0, &[(DRIVE, 0.3)]),
            k(SNARE, 0, 0.0, &[(BODY, -0.3)]),
            k(RING, -5, -0.3, &[]),
            k(RING, 0, 0.0, &[]),
            k(RING, 5, 0.3, &[]),
            k(DUST, 0, -0.4, &[(TONE, -0.2), (GRAIN, -0.3)]).leak(300.0),
            k(DUST, 0, 0.4, &[(GRAIN, -0.3)]).leak(300.0),
            k(DUST, 0, 0.15, &[(TONE, 0.2), (GRAIN, -0.3)]).leak(300.0),
            k(SPARK, 0, -0.1, &[]).threshold(-0.2),
            k(CLAP, 0, 0.05, &[(GRAIN, 0.2)]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[(DRIVE, 0.2)]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[(GRAIN, -0.5)]).leak(220.0),
        ],
        ..INIT
    },
    // Sparse grit: the noise thinned to single grains, the percussion to ticks.
    Kit {
        name: "Sand Glass",
        keys: [
            k(KICK, 0, 0.0, &[(DAMPING, 0.1)]),
            k(SNARE, 0, 0.0, &[(BODY, -0.2), (TONE, 0.15)]),
            k(RING, -5, -0.3, &[(DAMPING, 0.1)]),
            k(RING, 0, 0.0, &[(DAMPING, 0.1)]),
            k(RING, 5, 0.3, &[(DAMPING, 0.1)]),
            k(SPARK, 0, -0.4, &[(GRAIN, -0.3), (TONE, 0.1)]).leak(300.0),
            k(SPARK, 0, 0.4, &[(GRAIN, -0.3), (TONE, 0.2)]).leak(300.0),
            k(SPARK, 0, 0.15, &[(GRAIN, -0.3), (TONE, 0.3)]).leak(300.0),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[(GRAIN, -0.2)]),
            k(DUST, 0, 0.25, &[(GRAIN, -0.6), (TONE, 0.1)])
                .leak(120.0)
                .hats(),
            k(DUST, 0, 0.25, &[(GRAIN, -0.6), (TONE, 0.1)])
                .leak(900.0)
                .hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[(TONE, 0.25)]).leak(120.0),
            k(DUST, 5, 0.45, &[(GRAIN, -0.4)]).leak(250.0),
        ],
        ..INIT
    },
    // Sparks low enough to crackle at any charge, and a held note keeps them going.
    Kit {
        name: "Crackle Box",
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[]),
            k(SPARK, 0, -0.4, &[(GRAIN, 0.3), (TONE, -0.3)])
                .threshold(-0.4)
                .supply(0.6),
            k(SPARK, 0, -0.2, &[(GRAIN, 0.3), (TONE, -0.15)])
                .threshold(-0.4)
                .supply(0.6),
            k(SPARK, 0, 0.0, &[(GRAIN, 0.3)])
                .threshold(-0.4)
                .supply(0.6),
            k(SPARK, 0, 0.2, &[(GRAIN, 0.3), (TONE, 0.15)])
                .threshold(-0.4)
                .supply(0.6),
            k(SPARK, 0, 0.4, &[(GRAIN, 0.3), (TONE, 0.3)])
                .threshold(-0.4)
                .supply(0.6),
            k(SPARK, 0, 0.1, &[(GRAIN, 0.3), (TONE, 0.45)])
                .threshold(-0.4)
                .supply(0.6),
            k(SPARK, 0, -0.1, &[]).threshold(-0.2),
            k(CLAP, 0, 0.05, &[]).threshold(-0.2),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[(TONE, 0.3)]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // Pitched hiss whose tone jumps on every hit, like a radio between stations.
    Kit {
        name: "Radio Dust",
        seed: 211,
        lfos: &[synced(3, SampleHold, Division::Eighth)],
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[(BODY, -0.2)]),
            k(RING, -5, -0.3, &[]),
            k(RING, 0, 0.0, &[]),
            k(RING, 5, 0.3, &[]),
            k(DUST, 0, -0.4, &[(SPREAD, 0.5), (TONE, -0.3)])
                .leak(300.0)
                .routes(&[(Random, to(TONE), 0.3)]),
            k(DUST, 0, 0.4, &[(SPREAD, 0.5)])
                .leak(300.0)
                .routes(&[(Random, to(TONE), 0.3)]),
            k(DUST, 0, 0.15, &[(SPREAD, 0.5), (TONE, 0.3)])
                .leak(300.0)
                .routes(&[(Random, to(TONE), 0.3)]),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[(SPREAD, 0.4)])
                .leak(85.0)
                .hats()
                .routes(&[(Lfo3, to(TONE), 0.25)]),
            k(DUST, 0, 0.25, &[(SPREAD, 0.4)])
                .leak(775.0)
                .hats()
                .routes(&[(Lfo3, to(TONE), 0.25)]),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[(SPREAD, 0.5)])
                .leak(180.0)
                .routes(&[(Random, to(TONE), 0.4)]),
        ],
        ..INIT
    },
    // Dark, heavy noise: low hats, a wiry snare, a driven kick and toms.
    Kit {
        name: "Gravel Pit",
        keys: [
            k(KICK, -2, 0.0, &[(DRIVE, 0.3), (DAMPING, -0.1)]),
            k(SNARE, -2, 0.0, &[(TONE, -0.35), (BODY, -0.3)]),
            k(RING, -7, -0.3, &[(DRIVE, 0.2)]),
            k(RING, -2, 0.0, &[(DRIVE, 0.2)]),
            k(RING, 3, 0.3, &[(DRIVE, 0.2)]),
            k(FOLD, -3, -0.4, &[(DRIVE, 0.2)]),
            k(FOLD, 2, 0.4, &[(DRIVE, 0.2)]),
            k(DUST, 0, 0.15, &[(TONE, -0.5), (GRAIN, -0.2)]).leak(300.0),
            k(SPARK, 0, -0.1, &[(TONE, -0.2)]),
            k(CLAP, 0, 0.05, &[(TONE, -0.3)]),
            k(DUST, 0, 0.25, &[(TONE, -0.4), (GRAIN, -0.2)])
                .leak(100.0)
                .hats(),
            k(DUST, 0, 0.25, &[(TONE, -0.4), (GRAIN, -0.2)])
                .leak(900.0)
                .hats(),
            k(LOOP, 7, -0.3, &[(DAMPING, 0.3)]).leak(2000.0),
            k(FOLD, 7, 0.35, &[(DRIVE, 0.2)]),
            k(SPARK, 0, -0.35, &[(TONE, -0.1)]).leak(120.0),
            k(DUST, 0, 0.45, &[(TONE, -0.3), (GRAIN, -0.3)]).leak(250.0),
        ],
        ..INIT
    },
    // ---- Drones: a bus powers some cells with no note ------------------------------------------
    // Bus A holds three low rings at a chord under the drums, a bed to play over.
    Kit {
        name: "Hum Bed",
        category: Drone,
        bus: [0.3, 0.0],
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[]),
            k(RING, -17, -0.3, &[(DAMPING, -0.2)])
                .bus(A)
                .threshold(-0.3)
                .supply(0.4),
            k(RING, -10, 0.0, &[(DAMPING, -0.2)])
                .bus(A)
                .threshold(-0.3)
                .supply(0.4),
            k(RING, -5, 0.3, &[(DAMPING, -0.2)])
                .bus(A)
                .threshold(-0.3)
                .supply(0.4),
            k(FOLD, 0, -0.4, &[]),
            k(FOLD, 5, 0.4, &[]),
            k(LOOP, 7, 0.15, &[]),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // A mains hum on Bus A and a hiss on Bus B that swells and fades.
    Kit {
        name: "Power Line",
        category: Drone,
        bus: [0.4, 0.2],
        lfos: &[free(2, Sine, 0.1)],
        routes: &[(K::Lfo2, KitTarget::BusB, 0.4)],
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[]),
            k(RING, -14, -0.2, &[(DAMPING, -0.3)])
                .bus(A)
                .threshold(-0.3)
                .supply(0.4),
            k(FOLD, -14, 0.2, &[(DRIVE, -0.2)]).bus(A).supply(0.4),
            k(RING, 5, 0.3, &[]),
            k(FOLD, 0, -0.4, &[]),
            k(FOLD, 5, 0.4, &[]),
            k(LOOP, 7, 0.15, &[]),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[(TONE, 0.2)]).bus(B).supply(0.4),
        ],
        ..INIT
    },
    // Driven folds on Bus A rumble and break up, the bus rising over a bar.
    Kit {
        name: "Engine Room",
        category: Drone,
        temper: 0.3,
        coupling: 0.4,
        bus: [0.5, 0.0],
        seed: 307,
        lfos: &[synced(1, Sine, Division::Whole)],
        routes: &[(K::Lfo1, KitTarget::BusA, 0.3)],
        keys: [
            k(KICK, 0, 0.0, &[(DRIVE, 0.2)]),
            k(SNARE, 0, 0.0, &[]),
            k(RING, -5, -0.3, &[]),
            k(RING, 0, 0.0, &[]),
            k(RING, 5, 0.3, &[]),
            k(FOLD, -12, -0.4, &[(DRIVE, 0.3)]).bus(A).supply(0.4),
            k(FOLD, -7, 0.4, &[(DRIVE, 0.3)]).bus(A).supply(0.4),
            k(LOOP, 7, 0.15, &[]),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // Six rings on Bus A sing a chord, two of them with a slow vibrato.
    Kit {
        name: "Choir of Rings",
        category: Drone,
        bus: [0.35, 0.0],
        lfos: &[free(2, Triangle, 0.3)],
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[]),
            k(RING, -12, -0.4, &[(DAMPING, -0.2)])
                .bus(A)
                .threshold(-0.3)
                .supply(0.35),
            k(RING, -5, -0.2, &[(DAMPING, -0.2)])
                .bus(A)
                .threshold(-0.3)
                .supply(0.35),
            k(RING, 0, 0.0, &[(DAMPING, -0.2)])
                .bus(A)
                .threshold(-0.3)
                .supply(0.35)
                .routes(&[(Lfo2, CellTarget::Pitch, 0.04)]),
            k(RING, 4, 0.2, &[(DAMPING, -0.2)])
                .bus(A)
                .threshold(-0.3)
                .supply(0.35),
            k(RING, 7, 0.4, &[(DAMPING, -0.2)])
                .bus(A)
                .threshold(-0.3)
                .supply(0.35)
                .routes(&[(Lfo2, CellTarget::Pitch, -0.04)]),
            k(RING, 12, 0.1, &[(DAMPING, -0.2)])
                .bus(A)
                .threshold(-0.3)
                .supply(0.35),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // Sparse chirps and crackle on Bus B, the bus jumping at random four times a bar.
    Kit {
        name: "Night Insects",
        category: Drone,
        bus: [0.0, 0.3],
        seed: 401,
        lfos: &[synced(3, SampleHold, Division::Quarter)],
        routes: &[(K::Lfo3, KitTarget::BusB, 0.5)],
        keys: [
            k(KICK, 0, 0.0, &[(DAMPING, 0.1)]),
            k(SNARE, 0, 0.0, &[]),
            k(RING, -5, -0.3, &[]),
            k(RING, 0, 0.0, &[]),
            k(RING, 5, 0.3, &[]),
            k(SPARK, 0, -0.4, &[(GRAIN, -0.3), (TONE, 0.3)])
                .bus(B)
                .threshold(-0.3)
                .supply(0.4),
            k(SPARK, 0, 0.4, &[(GRAIN, -0.3), (TONE, 0.45)])
                .bus(B)
                .threshold(-0.3)
                .supply(0.4),
            k(SPARK, 0, 0.15, &[(GRAIN, -0.3), (TONE, 0.6)])
                .bus(B)
                .threshold(-0.3)
                .supply(0.4),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[(GRAIN, -0.6), (TONE, 0.4)])
                .bus(B)
                .supply(0.4),
        ],
        ..INIT
    },
    // ---- Feedback: thresholds low, cells sustain themselves -------------------------------------
    // Loops past their tipping point howl while their charge lasts; touch makes them scream.
    Kit {
        name: "Howl Room",
        keys: [
            k(KICK, 0, 0.0, &[]).threshold(-0.2).touch(0.8),
            k(SNARE, 0, 0.0, &[]),
            k(LOOP, -5, -0.3, &[(DRIVE, 0.2)])
                .threshold(-0.4)
                .touch(0.9),
            k(LOOP, 0, 0.0, &[(DRIVE, 0.2)]).threshold(-0.4).touch(0.9),
            k(LOOP, 5, 0.3, &[(DRIVE, 0.2)]).threshold(-0.4).touch(0.9),
            k(FOLD, 0, -0.4, &[]),
            k(FOLD, 5, 0.4, &[]),
            k(LOOP, 9, 0.15, &[(DRIVE, 0.2)]).threshold(-0.4).touch(0.9),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).threshold(-0.3).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // The kick and toms tip into singing on a held note or a hard hit.
    Kit {
        name: "Singing Skins",
        keys: [
            k(KICK, 0, 0.0, &[(DAMPING, -0.1)])
                .threshold(-0.35)
                .supply(0.6)
                .touch(0.8)
                .leak(1200.0),
            k(SNARE, 0, 0.0, &[]).threshold(-0.2),
            k(RING, -5, -0.3, &[])
                .threshold(-0.35)
                .supply(0.6)
                .touch(0.8)
                .leak(1200.0),
            k(RING, 0, 0.0, &[])
                .threshold(-0.35)
                .supply(0.6)
                .touch(0.8)
                .leak(1200.0),
            k(RING, 5, 0.3, &[])
                .threshold(-0.35)
                .supply(0.6)
                .touch(0.8)
                .leak(1200.0),
            k(RING, 9, -0.4, &[]).threshold(-0.3).supply(0.6),
            k(RING, 12, 0.4, &[]).threshold(-0.3).supply(0.6),
            k(LOOP, 7, 0.15, &[]),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // Howling cells coupled to each other, so one sets off the next.
    Kit {
        name: "Feedback Choir",
        temper: 0.3,
        coupling: 0.7,
        flow: 0.3,
        seed: 509,
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[]),
            k(RING, -5, -0.3, &[]).threshold(-0.3),
            k(RING, 0, 0.0, &[]).threshold(-0.3),
            k(RING, 5, 0.3, &[]).threshold(-0.3),
            k(LOOP, 0, -0.4, &[]).threshold(-0.3),
            k(LOOP, 5, 0.4, &[]).threshold(-0.3),
            k(LOOP, 7, 0.15, &[]).threshold(-0.3),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).threshold(-0.3).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // A snare that rings on, a clap and a rim that fire more sparks than they should.
    Kit {
        name: "Hot Wires",
        keys: [
            k(KICK, 0, 0.0, &[(DRIVE, 0.2)]),
            k(SNARE, 0, 0.0, &[(DRIVE, 0.3)]).threshold(-0.4),
            k(RING, -5, -0.3, &[(DRIVE, 0.2)]).threshold(-0.15),
            k(RING, 0, 0.0, &[(DRIVE, 0.2)]).threshold(-0.15),
            k(RING, 5, 0.3, &[(DRIVE, 0.2)]).threshold(-0.15),
            k(FOLD, 0, -0.4, &[(DRIVE, 0.15)]),
            k(FOLD, 5, 0.4, &[(DRIVE, 0.15)]),
            k(LOOP, 7, 0.15, &[(DRIVE, 0.2)]),
            k(SPARK, 0, -0.1, &[]).threshold(-0.3),
            k(CLAP, 0, 0.05, &[(GRAIN, 0.2)]).threshold(-0.3),
            k(DUST, 0, 0.25, &[(GRAIN, 0.2)]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[(GRAIN, 0.2)]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[(DRIVE, 0.15)]),
            k(SPARK, 0, -0.35, &[]).threshold(-0.2).leak(120.0),
            k(DUST, 5, 0.45, &[(GRAIN, 0.2)]).leak(180.0),
        ],
        ..INIT
    },
    // Folds on the edge of chaos everywhere: a hit locks, a harder one breaks into noise.
    Kit {
        name: "Live Coil",
        temper: 0.2,
        seed: 601,
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[]),
            k(FOLD, -12, -0.4, &[(DRIVE, 0.25)])
                .threshold(-0.4)
                .touch(1.0),
            k(FOLD, -7, -0.2, &[(DRIVE, 0.25)])
                .threshold(-0.4)
                .touch(1.0),
            k(FOLD, -2, 0.0, &[(DRIVE, 0.25)])
                .threshold(-0.4)
                .touch(1.0),
            k(FOLD, 3, 0.2, &[(DRIVE, 0.25), (SPREAD, 0.2)])
                .threshold(-0.4)
                .touch(1.0),
            k(FOLD, 8, 0.4, &[(DRIVE, 0.25), (SPREAD, 0.2)])
                .threshold(-0.4)
                .touch(1.0),
            k(FOLD, 13, 0.1, &[(DRIVE, 0.25), (SPREAD, -0.2)])
                .threshold(-0.4)
                .touch(1.0),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 18, 0.35, &[(DRIVE, 0.25)]).threshold(-0.4),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // ---- Starved: a small pool, so held notes and busy beats sag ------------------------------
    // A quarter of the supply and slow to refill: a busy pattern dims, then recovers.
    Kit {
        name: "Brownout",
        pool: 0.25,
        recharge_ms: Some(4000.0),
        keys: [
            k(KICK, 0, 0.0, &[]).supply(0.7),
            k(SNARE, 0, 0.0, &[]).supply(0.7),
            k(RING, -5, -0.3, &[]).supply(0.7),
            k(RING, 0, 0.0, &[]).supply(0.7),
            k(RING, 5, 0.3, &[]).supply(0.7),
            k(FOLD, 0, -0.4, &[]).supply(0.7),
            k(FOLD, 5, 0.4, &[]).supply(0.7),
            k(LOOP, 7, 0.15, &[]).supply(0.7),
            k(SPARK, 0, -0.1, &[]).supply(0.7),
            k(CLAP, 0, 0.05, &[]).supply(0.7),
            k(DUST, 0, 0.25, &[]).supply(0.7).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).supply(0.7).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).supply(0.7).leak(1600.0),
            k(FOLD, 12, 0.35, &[]).supply(0.7),
            k(SPARK, 0, -0.35, &[]).supply(0.7).leak(120.0),
            k(DUST, 5, 0.45, &[]).supply(0.7).leak(180.0),
        ],
        ..INIT
    },
    // Weaker strikes and soft hits held back: the kit sounds tired, and tires further.
    Kit {
        name: "Low Battery",
        pool: 0.4,
        recharge_ms: Some(3000.0),
        dynamics: -0.3,
        keys: [
            k(KICK, 0, 0.0, &[(DROP, -0.3)]).strike(0.5).supply(0.6),
            k(SNARE, 0, 0.0, &[]).strike(0.5).supply(0.6),
            k(RING, -5, -0.3, &[(DROP, -0.3)]).strike(0.5).supply(0.6),
            k(RING, 0, 0.0, &[(DROP, -0.3)]).strike(0.5).supply(0.6),
            k(RING, 5, 0.3, &[(DROP, -0.3)]).strike(0.5).supply(0.6),
            k(FOLD, 0, -0.4, &[]).strike(0.5).supply(0.6),
            k(FOLD, 5, 0.4, &[]).strike(0.5).supply(0.6),
            k(LOOP, 7, 0.15, &[]).strike(0.5).supply(0.6),
            k(SPARK, 0, -0.1, &[]).strike(0.5),
            k(CLAP, 0, 0.05, &[]).strike(0.5),
            k(DUST, 0, 0.25, &[]).strike(0.5).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).strike(0.5).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).strike(0.5).leak(1600.0),
            k(FOLD, 12, 0.35, &[]).strike(0.5),
            k(SPARK, 0, -0.35, &[]).strike(0.5).leak(120.0),
            k(DUST, 5, 0.45, &[]).strike(0.5).leak(180.0),
        ],
        ..INIT
    },
    // A small pool that refills fast: held kicks and snares pump against each other.
    Kit {
        name: "Pump Station",
        pool: 0.3,
        recharge_ms: Some(230.0),
        keys: [
            k(KICK, 0, 0.0, &[]).supply(0.9),
            k(SNARE, 0, 0.0, &[]).supply(0.9),
            k(RING, -5, -0.3, &[]).supply(0.7),
            k(RING, 0, 0.0, &[]).supply(0.7),
            k(RING, 5, 0.3, &[]).supply(0.7),
            k(FOLD, 0, -0.4, &[]).supply(0.6),
            k(FOLD, 5, 0.4, &[]).supply(0.6),
            k(LOOP, 7, 0.15, &[]).supply(0.6),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]).supply(0.6),
            k(DUST, 0, 0.25, &[]).supply(0.6).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).supply(0.6).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).supply(0.6).leak(180.0),
        ],
        ..INIT
    },
    // Half the pool, and charge flowing to whichever cell is emptiest.
    Kit {
        name: "Shared Breath",
        temper: 0.25,
        flow: 0.7,
        coupling: 0.3,
        pool: 0.5,
        seed: 709,
        keys: [
            k(KICK, 0, 0.0, &[]).supply(0.6),
            k(SNARE, 0, 0.0, &[]).supply(0.6),
            k(RING, -5, -0.3, &[]).supply(0.6),
            k(RING, 0, 0.0, &[]).supply(0.6),
            k(RING, 5, 0.3, &[]).supply(0.6),
            k(FOLD, 0, -0.4, &[]).supply(0.6),
            k(FOLD, 5, 0.4, &[]).supply(0.6),
            k(LOOP, 7, 0.15, &[]).supply(0.6),
            k(SPARK, 0, -0.1, &[]).supply(0.6),
            k(CLAP, 0, 0.05, &[]).supply(0.6),
            k(DUST, 0, 0.25, &[]).supply(0.6).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).supply(0.6).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]).supply(0.6),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).supply(0.6).leak(180.0),
        ],
        ..INIT
    },
    // The smallest pool and a slow refill, with long tails that spend little: a bar fades as it
    // plays.
    Kit {
        name: "Last Drop",
        pool: 0.2,
        recharge_ms: Some(6500.0),
        keys: [
            k(KICK, 0, 0.0, &[]).supply(0.8).hunger(0.3).leak(1200.0),
            k(SNARE, 0, 0.0, &[]).supply(0.8).hunger(0.3).leak(700.0),
            k(RING, -5, -0.3, &[]).supply(0.8).hunger(0.3).leak(1200.0),
            k(RING, 0, 0.0, &[]).supply(0.8).hunger(0.3).leak(1200.0),
            k(RING, 5, 0.3, &[]).supply(0.8).hunger(0.3).leak(1200.0),
            k(FOLD, 0, -0.4, &[]).supply(0.8).hunger(0.3),
            k(FOLD, 5, 0.4, &[]).supply(0.8).hunger(0.3),
            k(LOOP, 7, 0.15, &[]).supply(0.8).hunger(0.3),
            k(SPARK, 0, -0.1, &[]).supply(0.8),
            k(CLAP, 0, 0.05, &[]).supply(0.8),
            k(DUST, 0, 0.25, &[]).supply(0.8).leak(100.0).hats(),
            k(DUST, 0, 0.25, &[]).supply(0.8).leak(1200.0).hats(),
            k(LOOP, 12, -0.3, &[]).supply(0.8).leak(3000.0),
            k(FOLD, 12, 0.35, &[]).supply(0.8),
            k(SPARK, 0, -0.35, &[]).supply(0.8).leak(150.0),
            k(DUST, 5, 0.45, &[]).supply(0.8).leak(250.0),
        ],
        ..INIT
    },
    // ---- Moving: LFOs, velocity and randomness through the routes -----------------------------
    // A two-bar tide: Temper swells, the kick bends and the toms open and close with it.
    Kit {
        name: "Tidal",
        temper: 0.15,
        seed: 809,
        lfos: &[synced(1, Sine, Division::TwoBars)],
        routes: &[(K::Lfo1, KitTarget::Temper, 0.3)],
        keys: [
            k(KICK, 0, 0.0, &[]).routes(&[(Lfo1, CellTarget::Pitch, 0.1)]),
            k(SNARE, 0, 0.0, &[]),
            k(RING, -5, -0.3, &[]).routes(&[(Lfo1, to(DAMPING), -0.2)]),
            k(RING, 0, 0.0, &[]).routes(&[(Lfo1, to(DAMPING), -0.2)]),
            k(RING, 5, 0.3, &[]).routes(&[(Lfo1, to(DAMPING), -0.2)]),
            k(FOLD, 0, -0.4, &[]),
            k(FOLD, 5, 0.4, &[]),
            k(LOOP, 7, 0.15, &[]),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[])
                .leak(775.0)
                .hats()
                .routes(&[(Lfo1, to(TONE), 0.2)]),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // Hats whose tone wobbles on the beat, a snare whose body flips on the eighths.
    Kit {
        name: "Wobble Grid",
        lfos: &[
            synced(1, Sine, Division::Whole),
            synced(2, Triangle, Division::Quarter),
            synced(3, Square, Division::Eighth),
        ],
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[]).routes(&[(Lfo3, to(BODY), 0.3)]),
            k(RING, -5, -0.3, &[]).routes(&[(Lfo1, CellTarget::Pitch, 0.05)]),
            k(RING, 0, 0.0, &[]).routes(&[(Lfo1, CellTarget::Pitch, 0.05)]),
            k(RING, 5, 0.3, &[]).routes(&[(Lfo1, CellTarget::Pitch, 0.05)]),
            k(FOLD, 0, -0.4, &[]).routes(&[(Lfo2, to(SPREAD), 0.2)]),
            k(FOLD, 5, 0.4, &[]).routes(&[(Lfo2, to(SPREAD), -0.2)]),
            k(LOOP, 7, 0.15, &[]),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[])
                .leak(85.0)
                .hats()
                .routes(&[(Lfo2, to(TONE), 0.3)]),
            k(DUST, 0, 0.25, &[])
                .leak(775.0)
                .hats()
                .routes(&[(Lfo2, to(TONE), 0.3)]),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[])
                .leak(180.0)
                .routes(&[(Lfo2, to(TONE), -0.3)]),
        ],
        ..INIT
    },
    // Every hit draws its own pitch and tone from the seed: no two alike, every bounce the same.
    Kit {
        name: "Random Walk",
        seed: 777,
        keys: [
            k(KICK, 0, 0.0, &[]).routes(&[(Random, CellTarget::Pitch, 0.05)]),
            k(SNARE, 0, 0.0, &[]).routes(&[(Random, to(TONE), 0.2)]),
            k(RING, -5, -0.3, &[]).routes(&[(Random, CellTarget::Pitch, 0.1)]),
            k(RING, 0, 0.0, &[]).routes(&[(Random, CellTarget::Pitch, 0.1)]),
            k(RING, 5, 0.3, &[]).routes(&[(Random, CellTarget::Pitch, 0.1)]),
            k(FOLD, 0, -0.4, &[]).routes(&[(Random, CellTarget::Pitch, 0.1)]),
            k(FOLD, 5, 0.4, &[]).routes(&[(Random, CellTarget::Pitch, 0.1)]),
            k(LOOP, 7, 0.15, &[]).routes(&[(Random, CellTarget::Pitch, 0.1)]),
            k(SPARK, 0, -0.1, &[]).routes(&[(Random, to(TONE), 0.15)]),
            k(CLAP, 0, 0.05, &[]).routes(&[(Random, to(SPACING), 0.2)]),
            k(DUST, 0, 0.25, &[])
                .leak(85.0)
                .hats()
                .routes(&[(Random, to(TONE), 0.2)]),
            k(DUST, 0, 0.25, &[])
                .leak(775.0)
                .hats()
                .routes(&[(Random, to(TONE), 0.2)]),
            k(LOOP, 12, -0.3, &[])
                .leak(1600.0)
                .routes(&[(Random, to(SPREAD), 0.3)]),
            k(FOLD, 12, 0.35, &[]).routes(&[(Random, CellTarget::Pitch, 0.1)]),
            k(SPARK, 0, -0.35, &[])
                .leak(120.0)
                .routes(&[(Random, to(TONE), 0.15)]),
            k(DUST, 5, 0.45, &[])
                .leak(180.0)
                .routes(&[(Random, to(TONE), 0.2)]),
        ],
        ..INIT
    },
    // Velocity plays more than loudness: harder is higher toms, a longer kick drop, brighter hats.
    Kit {
        name: "Velocity Ladder",
        dynamics: 0.2,
        keys: [
            k(KICK, 0, 0.0, &[]).routes(&[(Velocity, to(DROP), 0.4)]),
            k(SNARE, 0, 0.0, &[]).routes(&[(Velocity, to(SNAP), 0.4)]),
            k(RING, -5, -0.3, &[]).routes(&[(Velocity, CellTarget::Pitch, 0.3)]),
            k(RING, 0, 0.0, &[]).routes(&[(Velocity, CellTarget::Pitch, 0.3)]),
            k(RING, 5, 0.3, &[]).routes(&[(Velocity, CellTarget::Pitch, 0.3)]),
            k(FOLD, 0, -0.4, &[]).routes(&[(Velocity, to(DRIVE), 0.3)]),
            k(FOLD, 5, 0.4, &[]).routes(&[(Velocity, to(DRIVE), 0.3)]),
            k(LOOP, 7, 0.15, &[]).routes(&[(Velocity, to(DAMPING), -0.3)]),
            k(SPARK, 0, -0.1, &[]).routes(&[(Velocity, to(TONE), 0.2)]),
            k(CLAP, 0, 0.05, &[]).routes(&[(Velocity, to(GRAIN), 0.3)]),
            k(DUST, 0, 0.25, &[])
                .leak(85.0)
                .hats()
                .routes(&[(Velocity, to(TONE), 0.3)]),
            k(DUST, 0, 0.25, &[])
                .leak(775.0)
                .hats()
                .routes(&[(Velocity, to(TONE), 0.3)]),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]).routes(&[(Velocity, to(DRIVE), 0.3)]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[])
                .leak(180.0)
                .routes(&[(Velocity, to(GRAIN), 0.3)]),
        ],
        ..INIT
    },
    // The mod wheel takes the kit from steady to wild; pressure powers the toms on Bus A.
    Kit {
        name: "Wheel Bender",
        seed: 1009,
        routes: &[
            (K::Wheel, KitTarget::Temper, 0.8),
            (K::Wheel, KitTarget::Coupling, 0.5),
            (K::Pressure, KitTarget::BusA, 0.5),
        ],
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[]),
            k(RING, -5, -0.3, &[]).bus(A).threshold(-0.2),
            k(RING, 0, 0.0, &[]).bus(A).threshold(-0.2),
            k(RING, 5, 0.3, &[]).bus(A).threshold(-0.2),
            k(FOLD, 0, -0.4, &[]),
            k(FOLD, 5, 0.4, &[]),
            k(LOOP, 7, 0.15, &[]),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // ---- Wild: Temper high, the grid feeding and draining itself ------------------------------
    // Three quarters of the way to wild: a beat that breaks into weather.
    Kit {
        name: "Storm Cell",
        category: Fx,
        temper: 0.75,
        flow: 0.8,
        coupling: 0.7,
        seed: 1201,
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[]),
            k(RING, -5, -0.3, &[]),
            k(RING, 0, 0.0, &[]),
            k(RING, 5, 0.3, &[]),
            k(FOLD, 0, -0.4, &[]),
            k(FOLD, 5, 0.4, &[]),
            k(LOOP, 7, 0.15, &[]),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // Full coupling and sparks near their tipping point: every hit arcs across the grid.
    Kit {
        name: "Arc Flash",
        category: Fx,
        temper: 0.9,
        flow: 0.3,
        coupling: 1.0,
        seed: 1777,
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[]),
            k(RING, -5, -0.3, &[]),
            k(SPARK, 0, 0.0, &[(TONE, -0.1)]).threshold(-0.2),
            k(RING, 5, 0.3, &[]),
            k(SPARK, 0, -0.4, &[(TONE, 0.1)]).threshold(-0.2),
            k(FOLD, 5, 0.4, &[]),
            k(SPARK, 0, 0.15, &[(TONE, 0.25)]).threshold(-0.2),
            k(SPARK, 0, -0.1, &[]).threshold(-0.2),
            k(CLAP, 0, 0.05, &[]).threshold(-0.2),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).threshold(-0.2).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // Strong coupling and rings near singing: one hit sets off its neighbours, and theirs.
    Kit {
        name: "Chain Reaction",
        category: Fx,
        temper: 0.65,
        flow: 0.4,
        coupling: 0.9,
        seed: 2048,
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[]),
            k(RING, -5, -0.3, &[]).threshold(-0.15),
            k(RING, 0, 0.0, &[]).threshold(-0.15),
            k(RING, 5, 0.3, &[]).threshold(-0.15),
            k(RING, 9, -0.4, &[]).threshold(-0.15),
            k(RING, 12, 0.4, &[]).threshold(-0.15),
            k(LOOP, 7, 0.15, &[]),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(RING, 16, 0.35, &[]).threshold(-0.15),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // Driven folds and howling loops, Temper jumping at random each beat: the kit melts and sets.
    Kit {
        name: "Melt",
        category: Fx,
        temper: 0.8,
        flow: 0.6,
        coupling: 0.6,
        seed: 3141,
        lfos: &[synced(3, SampleHold, Division::Quarter)],
        routes: &[(K::Lfo3, KitTarget::Temper, 0.2)],
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[]),
            k(LOOP, -5, -0.3, &[]).threshold(-0.3),
            k(LOOP, 0, 0.0, &[]).threshold(-0.3),
            k(LOOP, 5, 0.3, &[]).threshold(-0.3),
            k(FOLD, 0, -0.4, &[(DRIVE, 0.3)]),
            k(FOLD, 5, 0.4, &[(DRIVE, 0.3)]),
            k(LOOP, 7, 0.15, &[]).threshold(-0.3),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).leak(1600.0),
            k(FOLD, 12, 0.35, &[(DRIVE, 0.3)]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[]).leak(180.0),
        ],
        ..INIT
    },
    // Everything at full: Temper, Flow and Coupling all the way, every ring and loop near onset.
    Kit {
        name: "White Heat",
        category: Fx,
        temper: 1.0,
        flow: 1.0,
        coupling: 1.0,
        seed: 9001,
        lfos: &[free(2, RampDown, 0.25)],
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[]),
            k(RING, -5, -0.3, &[]).threshold(-0.2),
            k(RING, 0, 0.0, &[]).threshold(-0.2),
            k(RING, 5, 0.3, &[]).threshold(-0.2),
            k(FOLD, 0, -0.4, &[]),
            k(FOLD, 5, 0.4, &[]),
            k(LOOP, 7, 0.15, &[]).threshold(-0.2),
            k(SPARK, 0, -0.1, &[]),
            k(CLAP, 0, 0.05, &[]),
            k(DUST, 0, 0.25, &[]).leak(85.0).hats(),
            k(DUST, 0, 0.25, &[]).leak(775.0).hats(),
            k(LOOP, 12, -0.3, &[]).threshold(-0.2).leak(1600.0),
            k(FOLD, 12, 0.35, &[]),
            k(SPARK, 0, -0.35, &[]).leak(120.0),
            k(DUST, 5, 0.45, &[])
                .leak(180.0)
                .routes(&[(Lfo2, to(TONE), 0.3)]),
        ],
        ..INIT
    },
];

/// A kit's file name: its name in lower case, its words joined by hyphens.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

/// The amount along a geometric curve from `lo` over `octaves` that gives `value`: the inverse of
/// `curves::leak_ms`, `recharge_ms` and `lfo_hz`.
fn along(value: f32, lo: f32, octaves: f32) -> f32 {
    ((value / lo).log2() / octaves).clamp(0.0, 1.0)
}

/// **`kit` laid over Init, as its file holds it**, its cells at `levels` (dB): every parameter a kit
/// stores, the kit's values in place of Init's, each `text` the parameter's own reading. Unused
/// routes stay out, as capture leaves them (`Instrument::omit_captured_parameter`).
fn generated(params: &MxmChaosDrumsParams, kit: &Kit, levels: &[f32; CELLS]) -> Preset {
    let mut preset = Preset::capture(kit.name, kit.category, params);
    let mut put = |id: &str, param: &dyn mxm_params::ErasedParam, v: f32| {
        preset.params.insert(
            id.to_owned(),
            Value {
                v,
                text: param.format(v),
            },
        );
    };
    for (cell, key) in kit.keys.iter().enumerate() {
        let (p, ids) = (&params.cells[cell], cell_ids(cell));
        let own = dsp::CellParams::new(key.model);
        let amount = |param: &nice_plug::prelude::FloatParam, value: Option<f32>, own: f32| {
            param.preview_normalized(value.unwrap_or(own))
        };
        put(
            ids[at::MODEL],
            &p.model,
            p.model.preview_normalized(i32::from(key.model.raw())),
        );
        put(
            ids[at::COARSE],
            &p.coarse,
            p.coarse.preview_normalized(key.coarse),
        );
        for &(number, value) in key.controls {
            let control = p.controls.all()[usize::from(number) - 1];
            put(
                ids[at::control(usize::from(number))],
                control,
                control.preview_normalized(value),
            );
        }
        put(
            ids[at::STRIKE],
            &p.strike,
            amount(&p.strike, key.strike, own.strike),
        );
        put(
            ids[at::SUPPLY],
            &p.supply,
            amount(&p.supply, key.supply, own.supply),
        );
        put(
            ids[at::HUNGER],
            &p.hunger,
            amount(&p.hunger, key.hunger, own.hunger),
        );
        let leak = key
            .leak_ms
            .map(|ms| along(ms, curves::LEAK_MS.0, curves::LEAK_OCTAVES));
        put(ids[at::LEAK], &p.leak, amount(&p.leak, leak, own.leak));
        put(
            ids[at::THRESHOLD],
            &p.threshold,
            p.threshold.preview_normalized(key.threshold),
        );
        put(
            ids[at::TOUCH],
            &p.touch,
            amount(&p.touch, key.touch, own.touch),
        );
        put(ids[at::BUS], &p.bus, p.bus.preview_normalized(key.bus));
        put(
            ids[at::LEVEL],
            &p.level,
            p.level.preview_normalized(util::db_to_gain(levels[cell])),
        );
        put(ids[at::PAN], &p.pan, p.pan.preview_normalized(key.pan));
        put(
            ids[at::CHOKE_GROUP],
            &p.choke_group,
            p.choke_group.preview_normalized(key.choke),
        );
        for (r, &(source, target, depth)) in key.routes.iter().enumerate() {
            let route = p.routes.all()[r];
            let [source_id, target_id, amount_id] = at::route(r).map(|i| ids[i]);
            put(
                source_id,
                &route.source,
                route.source.preview_normalized(source),
            );
            put(
                target_id,
                &route.target,
                route.target.preview_normalized(cell_target_index(target)),
            );
            put(
                amount_id,
                &route.amount,
                route.amount.preview_normalized(depth),
            );
        }
    }
    put(
        "temper",
        &params.temper,
        params.temper.preview_normalized(kit.temper),
    );
    put(
        "flow",
        &params.flow,
        params.flow.preview_normalized(kit.flow),
    );
    put(
        "coupling",
        &params.coupling,
        params.coupling.preview_normalized(kit.coupling),
    );
    put(
        "pool",
        &params.pool,
        params.pool.preview_normalized(kit.pool),
    );
    if let Some(ms) = kit.recharge_ms {
        let recharge = along(ms, curves::RECHARGE_MS.0, curves::RECHARGE_OCTAVES);
        put(
            "recharge",
            &params.recharge,
            params.recharge.preview_normalized(recharge),
        );
    }
    put(
        "bus_a",
        &params.bus_a,
        params.bus_a.preview_normalized(kit.bus[0]),
    );
    put(
        "bus_b",
        &params.bus_b,
        params.bus_b.preview_normalized(kit.bus[1]),
    );
    put(
        "seed",
        &params.seed,
        params.seed.preview_normalized(kit.seed),
    );
    put(
        "dynamics",
        &params.dynamics,
        params.dynamics.preview_normalized(kit.dynamics),
    );
    for lfo in kit.lfos {
        let (rate, shape, sync) = params.lfos()[lfo.lfo - 1];
        // A synced Rate stores the place on the ladder that picks its division.
        let (position, synced) = match lfo.rate {
            Rate::Free(hz) => (along(hz, curves::LFO_HZ.0, curves::LFO_OCTAVES), false),
            Rate::Synced(division) => (LFO_SYNC.position(division), true),
        };
        let n = lfo.lfo;
        put(
            &format!("lfo{n}_rate"),
            rate,
            rate.preview_normalized(position),
        );
        put(
            &format!("lfo{n}_shape"),
            shape,
            shape.preview_normalized(lfo.shape),
        );
        put(
            &format!("lfo{n}_sync"),
            sync,
            sync.preview_normalized(synced),
        );
    }
    for (r, &(source, target, depth)) in kit.routes.iter().enumerate() {
        let route = params.kit_routes.all()[r];
        let index = KIT_TARGETS
            .iter()
            .position(|(t, _)| *t == target)
            .expect("a kit target") as i32;
        let n = r + 1;
        put(
            &format!("kroute{n}_source"),
            &route.source,
            route.source.preview_normalized(source),
        );
        put(
            &format!("kroute{n}_target"),
            &route.target,
            route.target.preview_normalized(index),
        );
        put(
            &format!("kroute{n}_amount"),
            &route.amount,
            route.amount.preview_normalized(depth),
        );
    }
    preset
}

/// Writes `preset` into `params`, as a load would.
fn apply(params: &MxmChaosDrumsParams, preset: &Preset) {
    let (writes, problems) = preset.resolve(params);
    assert!(problems.is_empty(), "{}: {problems:?}", preset.name);
    let map = params.param_map();
    for (id, _, value) in writes {
        let (_, ptr, _) = map.iter().find(|(m, _, _)| *m == id).expect("a parameter");
        // SAFETY: exclusive test ownership; no process or GUI thread exists.
        unsafe { ptr._internal_set_normalized_value(value) };
    }
}

/// The repository's factory set, in its order: what `cargo xtask bundle` installs.
static FILES: LazyLock<Vec<Preset>> = LazyLock::new(|| {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("presets");
    mxm_preset::read_factory_dir(&dir)
        .into_iter()
        .map(|(file, json)| {
            Preset::parse(&json, crate::CLAP_ID).unwrap_or_else(|e| panic!("{file}: {e:?}"))
        })
        .collect()
});

// ---- The balance ------------------------------------------------------------------------------

const RATE: f32 = 48_000.0;
/// The tempo a synced LFO runs at while a kit is measured.
const TEMPO: f64 = 120.0;
/// The editor's audition velocity.
const VELOCITY: f32 = 100.0 / 127.0;
/// Half a sixteenth at 120 BPM: a sequencer's gate.
const GATE_S: f32 = 0.0625;
const SECONDS: f32 = 3.0;
/// The short-term window, 170 ms at 48 kHz: wide enough to resolve the lowest band, and about the
/// ear's own integration time. And its hop.
const WINDOW: usize = 8192;
const HOP: usize = 1024;
/// The third-octave bands, `1 kHz · 2^(k/3)`: 31.25 Hz to 16 kHz.
const BANDS: std::ops::RangeInclusive<i32> = -15..=12;
/// The Level a cell may take: the parameter's range.
const LEVEL_DB: (f32, f32) = (-60.0, 6.0);
/// How far under the others a noise cell or a droning one sits.
const UNDER_DB: f32 = 6.0;
/// How far every Level may come down together so a quiet cell can reach its balance under the
/// Level's ceiling: the kick then sits this far under 0 dB at most.
const MAKE_ROOM_DB: f32 = 6.0;

/// One cell's hit, measured.
#[derive(Clone, Copy, Debug, Default)]
struct Hit {
    /// The highest short-term power any band reaches, in dB: the pink-noise measure.
    band_db: f64,
    /// The highest short-term power over all bands, K-weighted, in dB: the loudness measure.
    loudness_db: f64,
}

/// Each cell of the kit `params` holds, struck alone from rest as a sequencer plays it, before its
/// Level.
fn measured(params: &MxmChaosDrumsParams) -> [Hit; CELLS] {
    let kit = params.kit(params.lfo_hz(Some(TEMPO)));
    let cells: [_; CELLS] = std::array::from_fn(|i| params.cells[i].dsp(false));
    std::array::from_fn(|cell| {
        let mut engine = dsp::Kit::new(RATE, kit, cells);
        let mut out = vec![[0.0; CELLS]; (SECONDS * RATE) as usize];
        let events = [
            Event {
                offset: 0,
                kind: EventKind::Strike {
                    cell,
                    velocity: VELOCITY,
                    gate: true,
                },
            },
            Event {
                offset: (GATE_S * RATE) as usize,
                kind: EventKind::Release { cell },
            },
        ];
        engine.process_raw(&events, &mut out);
        let x: Vec<f64> = out.iter().map(|frame| f64::from(frame[cell])).collect();
        measure(&x)
    })
}

/// **The balance**: every cell as loud as the kick by its K-weighted hit, a noise cell (Raw dust)
/// or one a bus drones [`UNDER_DB`] under that. The kick is at 0 dB unless a cell would need more
/// than the Level's ceiling; then every Level comes down together, by up to [`MAKE_ROOM_DB`], so it
/// fits. To half a decibel, within the Level's range.
fn balance(params: &MxmChaosDrumsParams, hits: &[Hit; CELLS]) -> [f32; CELLS] {
    let reference = hits[0].loudness_db;
    let wanted: [f32; CELLS] = std::array::from_fn(|cell| {
        let p = &params.cells[cell];
        let under = p.model_id() == DUST || p.bus.value() != BusChoice::Off;
        (reference - hits[cell].loudness_db) as f32 - if under { UNDER_DB } else { 0.0 }
    });
    let loudest = wanted.iter().copied().fold(f32::MIN, f32::max);
    let room = (loudest - LEVEL_DB.1).clamp(0.0, MAKE_ROOM_DB);
    wanted.map(|db| (((db - room) * 2.0).round() / 2.0).clamp(LEVEL_DB.0, LEVEL_DB.1))
}

/// The pink-noise balance, for comparison: every cell's strongest band as high as the kick's.
fn pink(hits: &[Hit; CELLS]) -> [f32; CELLS] {
    let reference = hits[0].band_db;
    std::array::from_fn(|cell| {
        let db = (reference - hits[cell].band_db) as f32;
        ((db * 2.0).round() / 2.0).clamp(LEVEL_DB.0, LEVEL_DB.1)
    })
}

/// A hit's peak short-term spectrum: its strongest third-octave band, and its K-weighted power.
fn measure(x: &[f64]) -> Hit {
    let hann: Vec<f64> = (0..WINDOW)
        .map(|n| 0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / WINDOW as f64).cos())
        .collect();
    let bin_hz = f64::from(RATE) / WINDOW as f64;
    let centre = |k: i32| 1000.0 * 2f64.powf(f64::from(k) / 3.0);
    // Each bin's band, if it falls in one, and its K-weighting as a power.
    let bins: Vec<(Option<usize>, f64)> = (0..WINDOW / 2)
        .map(|bin| {
            let hz = bin as f64 * bin_hz;
            let band = BANDS.clone().position(|k| {
                (centre(k) * 2f64.powf(-1.0 / 6.0)..centre(k) * 2f64.powf(1.0 / 6.0)).contains(&hz)
            });
            (band, k_weight(hz))
        })
        .collect();
    let mut peak = vec![0.0f64; BANDS.count()];
    let mut loudest = 0.0f64;
    // Silence before the hit, so its attack is in the middle of a window as well as at the end.
    let padded: Vec<f64> = std::iter::repeat_n(0.0, WINDOW / 2)
        .chain(x.iter().copied())
        .collect();
    for start in (0..padded.len().saturating_sub(WINDOW)).step_by(HOP) {
        let mut re: Vec<f64> = padded[start..start + WINDOW]
            .iter()
            .zip(&hann)
            .map(|(x, w)| x * w)
            .collect();
        let mut im = vec![0.0; WINDOW];
        fft(&mut re, &mut im).expect("a power of two");
        let mut bands = vec![0.0f64; BANDS.count()];
        let mut weighted = 0.0;
        for (bin, (band, weight)) in bins.iter().enumerate().skip(1) {
            let power = re[bin] * re[bin] + im[bin] * im[bin];
            if let Some(band) = band {
                bands[*band] += power;
            }
            weighted += power * weight;
        }
        for (peak, band) in peak.iter_mut().zip(bands) {
            *peak = peak.max(band);
        }
        loudest = loudest.max(weighted);
    }
    let db = |power: f64| 10.0 * power.max(1e-30).log10();
    Hit {
        band_db: db(peak.iter().copied().fold(0.0, f64::max)),
        loudness_db: db(loudest),
    }
}

/// The K-weighting's power gain at `hz`: BS.1770-4's two 48 kHz biquads, a high shelf for the head
/// and a high-pass, evaluated on the unit circle.
fn k_weight(hz: f64) -> f64 {
    const SHELF: ([f64; 3], [f64; 2]) = (
        [
            1.535_124_859_586_97,
            -2.691_696_189_406_38,
            1.198_392_810_852_85,
        ],
        [-1.690_659_293_182_41, 0.732_480_774_215_85],
    );
    const HIGH_PASS: ([f64; 3], [f64; 2]) = (
        [1.0, -2.0, 1.0],
        [-1.990_047_454_833_98, 0.990_072_250_366_21],
    );
    let w = std::f64::consts::TAU * hz / f64::from(RATE);
    let power = |(b, a): ([f64; 3], [f64; 2])| {
        let at = |c: [f64; 3]| {
            let re = c[0] + c[1] * w.cos() + c[2] * (2.0 * w).cos();
            let im = -c[1] * w.sin() - c[2] * (2.0 * w).sin();
            re * re + im * im
        };
        at(b) / at([1.0, a[0], a[1]])
    };
    power(SHELF) * power(HIGH_PASS)
}

/// Each cell's Level, in dB, as `params` holds it.
fn levels(params: &MxmChaosDrumsParams) -> [f32; CELLS] {
    std::array::from_fn(|cell| util::gain_to_db(params.cells[cell].level.value()))
}

// ---- The generator and the checks -----------------------------------------------------------------

/// **Writes the fifty files and their order**, each balanced as it is measured. The only thing that
/// writes them: the design above is the readable statement of each kit, and the JSON is its output.
///
/// `cargo test -p mxm-chaos-drums --release --lib write_the_factory_kits -- --ignored`
#[test]
#[ignore = "writes the factory kits' preset files"]
fn write_the_factory_kits() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("presets");
    std::fs::create_dir_all(&dir).expect("the presets folder");
    let mut order = String::new();
    for kit in &KITS {
        let params = MxmChaosDrumsParams::default();
        apply(&params, &generated(&params, kit, &[0.0; CELLS]));
        let levels = balance(&params, &measured(&params));
        let preset = generated(&MxmChaosDrumsParams::default(), kit, &levels);
        let file = format!("{}.json", slug(kit.name));
        std::fs::write(dir.join(&file), preset.to_json()).expect("write a kit");
        order.push_str(&file);
        order.push('\n');
    }
    std::fs::write(dir.join(mxm_preset::ORDER_FILE), order).expect("write the order");
}

/// **Init and every shipped kit are their balance**: each re-measured, its Levels compared, its
/// table printed with the pink-noise balance beside it. Ignored, as it measures 816 hits.
///
/// `cargo test -p mxm-chaos-drums --release --lib init_and_every_kit_are_balanced -- --ignored --nocapture`
#[test]
#[ignore = "measures every cell of every kit"]
fn init_and_every_kit_are_balanced() {
    let mut problems = Vec::new();
    let mut check = |name: &str, params: &MxmChaosDrumsParams| {
        let hits = measured(params);
        let (wanted, pink, held) = (balance(params, &hits), pink(&hits), levels(params));
        println!("{name}\n  cell  model           vs kick  balance   pink   level");
        for cell in 0..CELLS {
            let model = available(params.cells[cell].model_id()).map_or("none", |spec| spec.label);
            println!(
                "  {:>4}  {model:<15} {:>+7.1}  {:>+7.1}  {:>+5.1}  {:>+6.1}",
                cell + 1,
                hits[cell].loudness_db - hits[0].loudness_db,
                wanted[cell],
                pink[cell],
                held[cell]
            );
            if (wanted[cell] - held[cell]).abs() > 0.05 {
                problems.push(format!(
                    "{name} cell {}: {:+.1} dB, its balance {:+.1}",
                    cell + 1,
                    held[cell],
                    wanted[cell]
                ));
            }
        }
    };
    check("Init", &MxmChaosDrumsParams::default());
    for preset in FILES.iter() {
        let params = MxmChaosDrumsParams::default();
        apply(&params, preset);
        check(&preset.name, &params);
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

/// **The files are their design**, built in memory and compared, changing nothing in the checkout;
/// a file's Levels are the measured balance, which [`init_and_every_kit_are_balanced`] holds.
#[test]
fn the_shipped_kits_are_their_designs() {
    assert_eq!(
        FILES.len(),
        KITS.len(),
        "every kit is shipped, and nothing else"
    );
    let params = MxmChaosDrumsParams::default();
    for (shipped, kit) in FILES.iter().zip(&KITS) {
        assert_eq!(shipped.name, kit.name, "the order follows the design's");
        let mut design = generated(&params, kit, &[0.0; CELLS]);
        for cell in 0..CELLS {
            let id = cell_ids(cell)[at::LEVEL];
            design
                .params
                .insert(id.to_owned(), shipped.params[id].clone());
        }
        assert!(
            *shipped == design,
            "{} is not its design: run write_the_factory_kits",
            kit.name
        );
    }
}

/// Every file in `presets/` is a kit of the design under its own name, and the order lists exactly
/// them, in the design's order.
#[test]
fn every_preset_file_is_a_listed_kit() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("presets");
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .expect("the presets folder")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    let listed: Vec<String> = KITS
        .iter()
        .map(|kit| format!("{}.json", slug(kit.name)))
        .collect();
    let order = std::fs::read_to_string(dir.join(mxm_preset::ORDER_FILE)).expect("the order");
    assert_eq!(order.lines().collect::<Vec<_>>(), listed, "the order");
    let mut sorted = listed.clone();
    files.sort();
    sorted.sort();
    assert_eq!(files, sorted);
}

/// **A shipped kit never leans on a default**: the file holds every parameter a kit stores, all but
/// the instance settings and the routes it does not use, so a later change to a default cannot
/// reach a kit already shipped.
#[test]
fn a_shipped_kit_never_leans_on_a_default() {
    let params = MxmChaosDrumsParams::default();
    let stored: Vec<&str> = params
        .parameters()
        .into_iter()
        .map(|(id, _)| id)
        .filter(|id| !params.is_instance_setting(id) && crate::params::route_of(id).is_none())
        .collect();
    for preset in FILES.iter() {
        for id in &stored {
            assert!(
                preset.params.contains_key(*id),
                "{} leaves {id} to its default",
                preset.name
            );
        }
    }
}

/// **A kit moves only what its models read**: no control a model does not read, no route onto one,
/// and every value in its parameter's range.
#[test]
fn every_cell_moves_only_what_its_model_reads() {
    let mut problems = Vec::new();
    let (leak, recharge) = (curves::LEAK_MS, curves::RECHARGE_MS);
    let unit = |x: f32| (0.0..=1.0).contains(&x);
    let signed = |x: f32| (-1.0..=1.0).contains(&x);
    for kit in &KITS {
        let kit_wide = [
            kit.temper,
            kit.flow,
            kit.coupling,
            kit.pool,
            kit.bus[0],
            kit.bus[1],
        ]
        .into_iter()
        .all(unit)
            && signed(kit.dynamics)
            && (1..=9999).contains(&kit.seed)
            && kit
                .recharge_ms
                .is_none_or(|ms| (recharge.0..=recharge.1).contains(&ms))
            && kit.routes.len() <= ROUTES
            && kit.routes.iter().all(|&(_, target, depth)| {
                target != KitTarget::Off && signed(depth) && depth != 0.0
            })
            && kit.lfos.iter().all(|lfo| (1..=3).contains(&lfo.lfo));
        if !kit_wide {
            problems.push(format!("{}: a kit-wide value out of range", kit.name));
        }
        for (cell, key) in kit.keys.iter().enumerate() {
            let here = format!("{} cell {}", kit.name, cell + 1);
            let Some(spec) = available(key.model) else {
                problems.push(format!("{here}: no model"));
                continue;
            };
            let reads = |number: u8| spec.controls.iter().any(|c| c.number == number);
            for &(number, value) in key.controls {
                if !reads(number) {
                    problems.push(format!("{here}: {} reads no control {number}", spec.label));
                }
                if !signed(value) {
                    problems.push(format!("{here}: control {number} at {value}"));
                }
            }
            for &(source, target, depth) in key.routes {
                if let CellTarget::Control(k) = target
                    && !reads(k + 1)
                {
                    problems.push(format!("{here}: a route onto control {}", k + 1));
                }
                if source == SourceChoice::Off
                    || target == CellTarget::Off
                    || !signed(depth)
                    || depth == 0.0
                {
                    problems.push(format!("{here}: a route that moves nothing"));
                }
            }
            let in_range = key.routes.len() <= ROUTES
                && (-48..=48).contains(&key.coarse)
                && signed(key.pan)
                && signed(key.threshold)
                && [key.strike, key.supply, key.hunger, key.touch]
                    .into_iter()
                    .flatten()
                    .all(unit)
                && key.leak_ms.is_none_or(|ms| (leak.0..=leak.1).contains(&ms));
            if !in_range {
                problems.push(format!("{here}: a value out of range"));
            }
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

/// **Every closed hat cuts its open hat**, as a kit does, and nothing else is in their group.
#[test]
fn every_kit_cuts_its_open_hat_with_its_closed_hat() {
    for kit in &KITS {
        for (cell, key) in kit.keys.iter().enumerate() {
            let hat = cell == 10 || cell == 11;
            assert_eq!(
                key.choke == HATS,
                hat,
                "{} cell {}: the hats' group",
                kit.name,
                cell + 1
            );
        }
    }
}

/// The names are the kits' own: unique, short, never Init's, never a machine's or a maker's.
#[test]
fn the_names_are_original_and_their_own() {
    const NOT: [&str; 12] = [
        "808",
        "909",
        "606",
        "707",
        "noon",
        "landscape",
        "roland",
        "linn",
        "simmons",
        "korg",
        "moog",
        "elektron",
    ];
    for kit in &KITS {
        let lower = kit.name.to_lowercase();
        assert!(
            kit.name.len() <= 20 && kit.name != "Init" && !NOT.iter().any(|n| lower.contains(n)),
            "{:?}",
            kit.name
        );
    }
    let mut slugs: Vec<String> = KITS.iter().map(|kit| slug(kit.name)).collect();
    slugs.sort();
    slugs.dedup();
    assert_eq!(slugs.len(), KITS.len(), "two kits share a name");
}

/// Every kit's kick is the reference, at 0 dB or as far under as its quietest cell needed, and
/// every Level is in its range.
#[test]
fn every_kit_balances_to_its_kick() {
    for preset in FILES.iter() {
        let params = MxmChaosDrumsParams::default();
        apply(&params, preset);
        let levels = levels(&params);
        assert!(
            (-MAKE_ROOM_DB - 0.05..=0.05).contains(&levels[0]),
            "{}: the kick at {}",
            preset.name,
            levels[0]
        );
        for level in levels {
            assert!(
                (LEVEL_DB.0 - 0.05..=LEVEL_DB.1 + 0.05).contains(&level),
                "{}: {level} dB",
                preset.name
            );
        }
    }
}

/// **Every cell of every kit sounds** when it is struck alone, at a low rate to keep it quick.
#[test]
fn every_cell_of_every_kit_sounds() {
    const RATE: f32 = 16_000.0;
    let mut silent = Vec::new();
    for preset in FILES.iter() {
        let params = MxmChaosDrumsParams::default();
        apply(&params, preset);
        let kit = params.kit(params.lfo_hz(Some(TEMPO)));
        let cells: [_; CELLS] = std::array::from_fn(|i| params.cells[i].dsp(false));
        for cell in 0..CELLS {
            let mut engine = dsp::Kit::new(RATE, kit, cells);
            let mut out = vec![[0.0; CELLS]; (0.4 * RATE) as usize];
            let hit = [Event {
                offset: 0,
                kind: EventKind::Strike {
                    cell,
                    velocity: VELOCITY,
                    gate: false,
                },
            }];
            engine.process_raw(&hit, &mut out);
            let peak = out
                .iter()
                .map(|frame| frame[cell].abs())
                .fold(0.0, f32::max);
            if peak < 0.01 {
                silent.push(format!("{} cell {}: peak {peak}", preset.name, cell + 1));
            }
        }
    }
    assert!(silent.is_empty(), "{silent:#?}");
}
