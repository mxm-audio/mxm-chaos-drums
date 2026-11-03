//! **The fifty factory kits** (the plan's P6, asked for on 2026-11-03): their design, the generator
//! that balances them and writes their files, and the checks that hold them. Tests only: what ships
//! is `presets/`, installed for the plugin's version.
//!
//! **Every kit is its own sound world** (the owner, 2026-11-03, of a first set that kept Init's drums
//! and nudged them: "they sound far too similar... this is abstract sounds. So go wild"): its models,
//! pitches, regimes and motion far from Init's and from each other's, as
//! [`no_two_kits_sound_alike`] measures. They keep Init's keys, so one beat plays the whole bank,
//! and its hats on 46 and 47 cut each other; what sounds on each key is the kit's. **The design is
//! sparse**: a cell names
//! its model, Coarse, pan and only the controls it moves; everything else is Init, and a Leak it does
//! not name is its model's own. [`generated`] lays a kit over Init; the ignored
//! [`write_the_factory_kits`] measures its balance and writes the files, complete, so a shipped kit
//! never leans on a default; [`the_shipped_kits_are_their_designs`] compares them in memory.
//!
//! **Their Levels are the brief's balance** (the owner, 2026-11-03), as Init's: every cell equally
//! loud per hit by ITU-R BS.1770's K-weighting, a noise cell (a hat, the maraca or Raw dust) or a
//! cell a bus drones 6 dB under it, the kick at 0 dB unless a quiet cell needs more than the Level's
//! ceiling, when every Level comes down together, by up to 6 dB ([`balance`]). The ignored
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
use LfoShapeChoice::{RampDown, RampUp, SampleHold, Sine, Square, Triangle};
use SourceChoice::{Lfo1, Lfo2, Lfo3, Random, Velocity};

const KICK: ModelId = ModelId::DRAINING_KICK;
const SNARE: ModelId = ModelId::WIRED_SNARE;
const CLAP: ModelId = ModelId::SPARK_CLAP;
const RING: ModelId = ModelId::RAW_RING;
const SPARK: ModelId = ModelId::RAW_SPARK;
const FOLD: ModelId = ModelId::RAW_FOLD;
const LOOP: ModelId = ModelId::RAW_LOOP;
const DUST: ModelId = ModelId::RAW_DUST;
const TOM: ModelId = ModelId::FALLING_TOM;
const CONGA: ModelId = ModelId::SLAPPED_CONGA;
const BONGO: ModelId = ModelId::SLAPPED_BONGO;
const RIM: ModelId = ModelId::DAMPED_RIM;
const CLAVE: ModelId = ModelId::DAMPED_CLAVE;
const COWBELL: ModelId = ModelId::COUPLED_COWBELL;
const BELL: ModelId = ModelId::COUPLED_BELL;
const CLOSED: ModelId = ModelId::LEAKING_CLOSED_HAT;
const OPEN: ModelId = ModelId::LEAKING_OPEN_HAT;
const FOLDED: ModelId = ModelId::FOLDED_HAT;
const CYMBAL: ModelId = ModelId::LOOP_CYMBAL;
const CRASH: ModelId = ModelId::LOOP_CRASH;
const RIDE: ModelId = ModelId::RINGING_RIDE;
const MARACA: ModelId = ModelId::GRAIN_MARACA;
const TAMB: ModelId = ModelId::JINGLE_TAMBOURINE;
const GUIRO: ModelId = ModelId::SCRAPING_GUIRO;
const BRUSH: ModelId = ModelId::SWEPT_BRUSH;

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

/// **The fifty**, in the browser's order (the owner, 2026-11-03: "this is abstract sounds. So go
/// wild"): every kit its own sound world, its models, pitches, regimes and motion far from Init's and
/// each other's ([`no_two_kits_sound_alike`]). Pans are the player's view. Written on the raw engines
/// before P4, then re-voiced across the whole catalogue once it rendered (the owner, 2026-11-03:
/// "With all the new models we need to redo the 50 presets"): a cell moved onto its role's model
/// where that model suits the kit's world, re-pitched to sound where it did, and a kit whose world
/// is its raw engines (the folds of *Chaos Folds*, the falling rings of *Rubber Band*) kept them.
static KITS: [Kit; 50] = [
    // ---- Struck: wild drums, each played once and left to ring -------------------------------
    // Every cell a falling ring, pitched high and swept far: a kit of laser shots.
    Kit {
        name: "Laser Grid",
        keys: [
            k(KICK, 12, 0.0, &[(DROP, 1.0), (DAMPING, 0.3), (SNAP, 0.6)]).leak(250.0),
            k(SNARE, 7, 0.0, &[(DROP, 1.0), (BODY, 0.8), (TONE, 0.625)]).leak(200.0),
            k(TOM, 24, -0.6, &[(DROP, 1.0), (DAMPING, 0.4)]).leak(180.0),
            k(TOM, 31, -0.2, &[(DROP, 1.0), (DAMPING, 0.4)]).leak(180.0),
            k(TOM, 36, 0.2, &[(DROP, 1.0), (DAMPING, 0.4)]).leak(180.0),
            k(RING, 19, 0.6, &[(DROP, 0.8), (DAMPING, 0.2)]).leak(300.0),
            k(BONGO, 24, -0.8, &[(DROP, 1.0), (DAMPING, 0.6)]).leak(120.0),
            k(RING, 12, 0.8, &[(DROP, 1.0), (DAMPING, 0.1)]).leak(400.0),
            k(RIM, 12, -0.3, &[(TONE, 1.0), (SNAP, 1.0), (DROP, 1.0)]).leak(120.0),
            k(CLAP, 0, 0.3, &[(TONE, 0.9), (SPACING, -0.8), (SNAP, 0.8)]),
            k(FOLDED, 0, 0.5, &[(TONE, 1.0), (GRAIN, 0.9)])
                .leak(120.0)
                .hats(),
            k(FOLDED, 0, -0.5, &[(TONE, 1.0), (GRAIN, 0.9)])
                .leak(300.0)
                .hats(),
            k(CYMBAL, 24, 0.0, &[(DAMPING, -0.6), (SPREAD, 0.5)]).leak(900.0),
            k(CLAVE, 12, 0.4, &[(DROP, 1.0), (DAMPING, 0.8)]).leak(120.0),
            k(SPARK, 0, -0.6, &[(TONE, 1.0), (SNAP, 1.0)]).leak(120.0),
            k(DUST, 12, 0.7, &[(TONE, 1.0), (SPREAD, 1.0), (GRAIN, 0.5)]).leak(150.0),
        ],
        ..INIT
    },
    // Two to four octaves down and let ring: subsonic rings, buzzing loops, rumbling noise.
    Kit {
        name: "Sub Pressure",
        keys: [
            k(KICK, -9, 0.0, &[(DAMPING, -0.7), (DROP, 0.4), (DRIVE, 0.5)])
                .hunger(0.2)
                .leak(2500.0),
            k(
                SNARE,
                -14,
                0.0,
                &[(TONE, -0.9), (BODY, 0.6), (DAMPING, -0.4)],
            )
            .leak(1200.0),
            k(TOM, -24, -0.4, &[(DAMPING, -0.8)])
                .hunger(0.2)
                .leak(3000.0),
            k(TOM, -19, 0.0, &[(DAMPING, -0.8)])
                .hunger(0.2)
                .leak(3000.0),
            k(TOM, -17, 0.4, &[(DAMPING, -0.8)])
                .hunger(0.2)
                .leak(3000.0),
            k(FOLD, -24, -0.6, &[(DRIVE, -0.6), (SPREAD, -0.5)]).leak(1500.0),
            k(FOLD, -29, 0.6, &[(DRIVE, 0.5)]).leak(1500.0),
            k(LOOP, -36, 0.2, &[(DAMPING, 0.5), (DRIVE, 0.6)]).leak(1200.0),
            k(RIM, -24, -0.2, &[(TONE, -1.0), (SNAP, -1.0)]).leak(400.0),
            k(CLAP, 0, 0.2, &[(TONE, -1.0), (GRAIN, -0.4), (SPACING, 0.6)]),
            k(CLOSED, 0, 0.3, &[(TONE, -1.0), (GRAIN, 0.4)])
                .leak(150.0)
                .hats(),
            k(OPEN, 0, -0.3, &[(TONE, -0.8), (GRAIN, 0.6)])
                .leak(2000.0)
                .hats(),
            k(CRASH, -24, 0.0, &[(SPREAD, 0.9), (DAMPING, 0.2)]).leak(4000.0),
            k(COWBELL, -24, 0.5, &[(DRIVE, 0.8)]).leak(800.0),
            k(RING, -36, -0.5, &[(DROP, 1.0), (DAMPING, -0.3)]).leak(1000.0),
            k(BRUSH, -24, 0.6, &[(TONE, -0.9), (SPREAD, 0.9)]).leak(600.0),
        ],
        ..INIT
    },
    // Pure, bright, long plucks three octaves up, and whistling noise: ice falling on glass.
    Kit {
        name: "Glass Rain",
        keys: [
            k(KICK, 24, 0.0, &[(DAMPING, -0.3), (DROP, -1.0), (SNAP, 0.8)]).leak(800.0),
            k(
                SNARE,
                19,
                0.0,
                &[(TONE, 0.625), (BODY, 1.0), (DAMPING, -0.5)],
            )
            .leak(900.0),
            k(BELL, 9, -0.7, &[(DAMPING, -0.9), (SPREAD, -1.0)])
                .strike(0.7)
                .leak(3000.0),
            k(BELL, 12, -0.4, &[(DAMPING, -0.9), (SPREAD, -1.0)])
                .strike(0.7)
                .leak(3000.0),
            k(BELL, 16, -0.1, &[(DAMPING, -0.9), (SPREAD, -1.0)])
                .strike(0.7)
                .leak(3000.0),
            k(LOOP, 34, 0.2, &[(DAMPING, -0.9), (SPREAD, -1.0)])
                .strike(0.7)
                .leak(3000.0),
            k(LOOP, 36, 0.5, &[(DAMPING, -0.9), (SPREAD, -1.0)])
                .strike(0.7)
                .leak(3000.0),
            k(LOOP, 39, 0.8, &[(DAMPING, -0.9), (SPREAD, -1.0)])
                .strike(0.7)
                .leak(3000.0),
            k(MARACA, 0, -0.2, &[(TONE, 1.0), (GRAIN, 0.4), (SNAP, 1.0)]).leak(150.0),
            k(CLAP, 0, 0.2, &[(TONE, 1.0), (GRAIN, 1.0), (SNAP, 1.0)]),
            k(CLOSED, 0, 0.4, &[(TONE, 1.0), (SPREAD, 1.0), (GRAIN, -0.6)])
                .leak(150.0)
                .hats(),
            k(OPEN, 0, -0.4, &[(TONE, 1.0), (SPREAD, 1.0), (GRAIN, -0.4)])
                .leak(2500.0)
                .hats(),
            k(RIDE, 28, 0.0, &[(DAMPING, -1.0), (SPREAD, 0.4)]).leak(8000.0),
            k(LOOP, 46, 0.3, &[(DAMPING, -0.8), (SPREAD, -1.0)]).leak(2000.0),
            k(CLAVE, 12, -0.5, &[(DAMPING, -0.6)]).leak(150.0),
            k(TAMB, 0, 0.6, &[(TONE, 1.0), (SPREAD, 1.0)]).leak(300.0),
        ],
        ..INIT
    },
    // Dark, low plucks that die at once and thumping sparks: a kit with the lights off.
    Kit {
        name: "Plucked Void",
        keys: [
            k(KICK, -7, 0.0, &[(DAMPING, -0.6), (DROP, -0.5)]).leak(1500.0),
            k(SNARE, -5, 0.0, &[(BODY, 0.9), (TONE, -1.0), (DAMPING, 0.4)]),
            k(
                LOOP,
                -24,
                -0.5,
                &[(DAMPING, 0.6), (SPREAD, -1.0), (SNAP, -0.6)],
            )
            .leak(700.0),
            k(
                LOOP,
                -19,
                -0.1,
                &[(DAMPING, 0.6), (SPREAD, -1.0), (SNAP, -0.6)],
            )
            .leak(700.0),
            k(
                LOOP,
                -14,
                0.3,
                &[(DAMPING, 0.6), (SPREAD, -1.0), (SNAP, -0.6)],
            )
            .leak(700.0),
            k(CONGA, -12, -0.7, &[(DAMPING, 0.8), (BODY, 0.6)]).leak(500.0),
            k(BONGO, -12, 0.7, &[(DAMPING, 0.8), (BODY, 0.6)]).leak(500.0),
            k(MARACA, 0, 0.1, &[(TONE, -1.0), (SNAP, -0.8), (GRAIN, -0.4)]).leak(300.0),
            k(RIM, -12, -0.3, &[(TONE, -0.9), (SNAP, -1.0)]).leak(200.0),
            k(CLAP, 0, 0.1, &[(TONE, -0.8), (SPACING, 0.5)]),
            k(CLOSED, 0, 0.4, &[(TONE, -0.6), (GRAIN, -0.7)])
                .leak(100.0)
                .hats(),
            k(OPEN, 0, 0.4, &[(TONE, -0.6), (GRAIN, -0.7)])
                .leak(1500.0)
                .hats(),
            k(CRASH, -24, -0.2, &[(DAMPING, 0.8), (SPREAD, 0.5)]).leak(4000.0),
            k(COWBELL, -12, 0.5, &[(DAMPING, 1.0), (SNAP, -1.0)]).leak(300.0),
            k(SPARK, 0, -0.6, &[(TONE, -0.7), (GRAIN, -0.8)]).leak(150.0),
            k(BRUSH, -12, 0.6, &[(TONE, -0.9), (SPREAD, 0.6)]).leak(400.0),
        ],
        ..INIT
    },
    // Folds and a kick whose pitch ramps up on the sixteenths and eighths: a kit of rising chirps.
    Kit {
        name: "Chirp Machine",
        lfos: &[
            synced(1, RampUp, Division::Sixteenth),
            synced(2, RampUp, Division::Eighth),
        ],
        keys: [
            k(KICK, 0, 0.0, &[(DROP, -1.0), (DAMPING, 0.5)])
                .leak(250.0)
                .routes(&[(Lfo1, CellTarget::Pitch, 1.0)]),
            k(SNARE, 5, 0.0, &[(BODY, 1.0), (DROP, -1.0), (DAMPING, 0.4)])
                .leak(250.0)
                .routes(&[(Lfo2, CellTarget::Pitch, 1.0)]),
            k(TOM, 12, -0.6, &[(DROP, -1.0), (DRIVE, -0.6)])
                .leak(120.0)
                .routes(&[(Lfo1, CellTarget::Pitch, 1.0)]),
            k(TOM, 17, -0.2, &[(DROP, -1.0), (DRIVE, -0.6)])
                .leak(120.0)
                .routes(&[(Lfo2, CellTarget::Pitch, 1.0)]),
            k(TOM, 22, 0.2, &[(DROP, -1.0), (DRIVE, -0.6)])
                .leak(120.0)
                .routes(&[(Lfo1, CellTarget::Pitch, 1.0)]),
            k(FOLD, 26, 0.6, &[(DRIVE, -0.4), (SPREAD, 0.4)])
                .leak(120.0)
                .routes(&[(Lfo2, CellTarget::Pitch, 1.0)]),
            k(FOLD, 29, -0.8, &[(DRIVE, -0.4), (SPREAD, -0.4)])
                .leak(120.0)
                .routes(&[(Lfo1, CellTarget::Pitch, -1.0)]),
            k(BONGO, 12, 0.8, &[(DRIVE, -0.6)]).leak(100.0).routes(&[(
                Lfo2,
                CellTarget::Pitch,
                -1.0,
            )]),
            k(SPARK, 0, -0.3, &[(TONE, 0.8), (GRAIN, 0.3)]).leak(100.0),
            k(CLAP, 0, 0.3, &[(SNAP, 1.0), (SPACING, -1.0)]),
            k(CLOSED, 0, 0.5, &[(TONE, 1.0), (GRAIN, 0.4)])
                .leak(50.0)
                .hats(),
            k(OPEN, 0, -0.5, &[(TONE, 1.0), (GRAIN, 0.4)])
                .leak(300.0)
                .hats(),
            k(CYMBAL, 12, 0.0, &[(SPREAD, -1.0), (DAMPING, 0.3)])
                .leak(500.0)
                .routes(&[(Lfo1, CellTarget::Pitch, 1.0)]),
            k(CLAVE, -12, 0.4, &[(DRIVE, -1.0)]).leak(80.0).routes(&[(
                Lfo2,
                CellTarget::Pitch,
                1.0,
            )]),
            k(SPARK, 0, -0.6, &[(TONE, 1.0)]).leak(60.0),
            k(
                BRUSH,
                0,
                0.6,
                &[(SPREAD, 1.0), (TONE, 0.667), (GRAIN, -0.6)],
            )
            .leak(250.0)
            .routes(&[(Lfo2, to(TONE), 1.0)]),
        ],
        ..INIT
    },
    // Wide-spread, hard-driven metal low down, coupled so one strike rattles the rest.
    Kit {
        name: "Metal Shop",
        temper: 0.3,
        coupling: 0.7,
        seed: 271,
        keys: [
            k(KICK, -2, 0.0, &[(DRIVE, 0.9), (DAMPING, 0.5)]),
            k(SNARE, -5, 0.0, &[(TONE, 0.625), (BODY, -1.0), (DRIVE, 1.0)]),
            k(COWBELL, -24, -0.5, &[(SPREAD, 1.0), (DRIVE, 0.9)]).threshold(-0.4),
            k(COWBELL, -19, -0.1, &[(SPREAD, 1.0), (DRIVE, 0.9)]).threshold(-0.4),
            k(COWBELL, -14, 0.3, &[(SPREAD, 1.0), (DRIVE, 0.9)]).threshold(-0.4),
            k(FOLD, -5, -0.7, &[(SPREAD, 1.0), (DRIVE, 0.8)]),
            k(FOLD, 0, 0.7, &[(SPREAD, -1.0), (DRIVE, 0.8)]),
            k(BELL, -17, 0.1, &[(SPREAD, 1.0), (DRIVE, 0.9)]).threshold(-0.4),
            k(
                LOOP,
                5,
                -0.3,
                &[(SPREAD, 1.0), (DAMPING, 0.6), (DRIVE, 0.6)],
            )
            .leak(150.0),
            k(CLAP, 0, 0.2, &[(TONE, 0.3), (SPACING, -0.5)]),
            k(
                CLOSED,
                -12,
                0.4,
                &[(SPREAD, 1.0), (DRIVE, 1.0), (BODY, 0.6)],
            )
            .leak(100.0)
            .hats(),
            k(OPEN, -12, 0.4, &[(SPREAD, 1.0), (DRIVE, 1.0), (BODY, 0.6)])
                .leak(1500.0)
                .hats(),
            k(
                CRASH,
                -7,
                -0.4,
                &[(SPREAD, 1.0), (DAMPING, -0.4), (BODY, 0.8)],
            )
            .leak(6000.0),
            k(FOLD, 5, 0.5, &[(SPREAD, 1.0), (DRIVE, 0.8)]),
            k(
                LOOP,
                17,
                -0.6,
                &[(SPREAD, 0.8), (DAMPING, 0.5), (DRIVE, 0.6)],
            )
            .leak(200.0),
            k(TAMB, -12, 0.6, &[(SPREAD, 0.8), (TONE, -0.3), (GRAIN, 0.7)]).leak(300.0),
        ],
        ..INIT
    },
    // Rings that hardly lose anything and sing at their pitch for seconds, sympathetic on the grid.
    Kit {
        name: "Gong Hall",
        temper: 0.2,
        flow: 0.2,
        coupling: 0.9,
        seed: 313,
        keys: [
            k(KICK, -7, 0.0, &[(DAMPING, -0.8), (DROP, -0.6)]).leak(6000.0),
            k(SNARE, -2, 0.0, &[(BODY, 1.0), (DAMPING, -0.8)]).leak(3000.0),
            k(BELL, -36, -0.6, &[(BODY, 0.6), (DAMPING, -1.0)])
                .threshold(-0.3)
                .hunger(0.1)
                .leak(15000.0),
            k(RING, -17, -0.3, &[(DAMPING, -1.0), (DROP, -1.0)])
                .threshold(-0.3)
                .hunger(0.1)
                .leak(15000.0),
            k(BELL, -29, 0.0, &[(BODY, 0.6), (DAMPING, -1.0)])
                .threshold(-0.3)
                .hunger(0.1)
                .leak(15000.0),
            k(RING, -9, 0.3, &[(DAMPING, -1.0), (DROP, -1.0)])
                .threshold(-0.3)
                .hunger(0.1)
                .leak(15000.0),
            k(BELL, -24, 0.6, &[(BODY, 0.6), (DAMPING, -1.0)])
                .threshold(-0.3)
                .hunger(0.1)
                .leak(15000.0),
            k(CRASH, -10, -0.8, &[(SPREAD, 0.8), (DAMPING, -0.8)]).leak(8000.0),
            k(RIDE, -8, 0.8, &[(SPREAD, 0.9), (DAMPING, -0.7)]).leak(5000.0),
            k(CLAP, 0, 0.0, &[(TONE, -0.4), (SPACING, 0.4)]),
            k(CLOSED, 0, 0.3, &[(SPREAD, 0.6), (TONE, 0.5)])
                .leak(200.0)
                .hats(),
            k(OPEN, 0, 0.3, &[(SPREAD, 0.6), (TONE, 0.5)])
                .leak(5000.0)
                .hats(),
            k(CYMBAL, -24, 0.0, &[(SPREAD, 1.0), (DAMPING, -1.0)]).leak(30000.0),
            k(BELL, -12, -0.4, &[(DAMPING, -1.0)])
                .hunger(0.1)
                .leak(10000.0),
            k(SPARK, 0, 0.4, &[(TONE, -0.5)]).leak(400.0),
            k(BRUSH, 0, -0.5, &[(TONE, -0.3), (SPREAD, 0.8)]).leak(3000.0),
        ],
        ..INIT
    },
    // Cells driven past their tipping point: every key screams and growls while its charge lasts.
    Kit {
        name: "Toy Riot",
        temper: 0.4,
        seed: 421,
        keys: [
            k(KICK, 24, 0.0, &[(DRIVE, 1.0), (DAMPING, 0.2)]).threshold(-0.3),
            k(SNARE, 24, 0.0, &[(DRIVE, 1.0), (TONE, 0.625)]).threshold(-0.3),
            k(TOM, 24, -0.6, &[(DRIVE, 1.0)]).threshold(-0.3),
            k(TOM, 29, -0.2, &[(DRIVE, 1.0)]).threshold(-0.3),
            k(TOM, 34, 0.2, &[(DRIVE, 1.0)]).threshold(-0.3),
            k(CONGA, 14, 0.6, &[(DRIVE, 1.0)]).threshold(-0.3),
            k(BONGO, 12, -0.8, &[(DRIVE, 1.0)]).threshold(-0.3),
            k(LOOP, 24, 0.8, &[(DRIVE, 1.0)]).threshold(-0.3),
            k(RIM, 12, -0.3, &[(TONE, 1.0)]).threshold(-0.3),
            k(CLAP, 0, 0.3, &[(TONE, 0.9)]).threshold(-0.3),
            k(CLOSED, 12, 0.5, &[(TONE, 1.0)]).leak(150.0).hats(),
            k(OPEN, 12, -0.5, &[(TONE, 1.0)]).leak(500.0).hats(),
            k(LOOP, 36, 0.0, &[(DRIVE, 1.0), (SPREAD, 0.5)])
                .threshold(-0.3)
                .leak(1200.0),
            k(COWBELL, 12, 0.4, &[(DRIVE, 1.0)]).threshold(-0.3),
            k(SPARK, 0, -0.6, &[(TONE, 1.0)]).threshold(-0.3),
            k(CLAVE, -14, 0.6, &[(DRIVE, 1.0)]).threshold(-0.3),
        ],
        ..INIT
    },
    // The whole kit two octaves down and three times as long: a beat played at half speed, twice.
    Kit {
        name: "Half Speed",
        keys: [
            k(KICK, -24, 0.0, &[(DAMPING, -0.5)]).leak(1800.0),
            k(SNARE, -24, 0.0, &[(DAMPING, -0.5), (TONE, -0.6)]).leak(1200.0),
            k(TOM, -29, -0.4, &[(DAMPING, -0.5)]).leak(1800.0),
            k(TOM, -24, 0.0, &[(DAMPING, -0.5)]).leak(1800.0),
            k(TOM, -19, 0.4, &[(DAMPING, -0.5)]).leak(1800.0),
            k(CONGA, -24, -0.6, &[]).leak(1200.0),
            k(CONGA, -19, 0.6, &[]).leak(1200.0),
            k(BONGO, -24, 0.2, &[(DAMPING, -0.3)]).leak(1200.0),
            k(RIM, -24, -0.2, &[(TONE, -0.8)]).leak(400.0),
            k(CLAP, 0, 0.1, &[(SPACING, 1.0), (TONE, -0.8), (SNAP, -1.0)]),
            k(CLOSED, -24, 0.3, &[(TONE, -0.6), (GRAIN, -0.5)])
                .leak(250.0)
                .hats(),
            k(OPEN, -24, 0.3, &[(TONE, -0.6), (GRAIN, -0.5)])
                .leak(2400.0)
                .hats(),
            k(CRASH, -24, -0.3, &[(DAMPING, -0.3)]).leak(5000.0),
            k(COWBELL, -24, 0.4, &[]).leak(800.0),
            k(CLAVE, -24, -0.4, &[(DAMPING, -0.5)]).leak(350.0),
            k(MARACA, -24, 0.5, &[(TONE, -0.6), (GRAIN, -0.6)]).leak(550.0),
        ],
        ..INIT
    },
    // Tin, wire and plastic: bright noise snares, little cans, buzzing folds, a ratchet clap.
    Kit {
        name: "Junk Percussion",
        keys: [
            k(KICK, 5, 0.0, &[(DAMPING, 0.6), (DRIVE, 0.6)]).leak(200.0),
            k(SNARE, 7, 0.0, &[(BODY, -1.0), (TONE, 0.625)]).leak(150.0),
            k(COWBELL, -11, -0.5, &[(SPREAD, 1.0), (DAMPING, 0.4)]).leak(250.0),
            k(COWBELL, -6, -0.1, &[(SPREAD, 1.0), (DAMPING, 0.4)]).leak(250.0),
            k(COWBELL, -1, 0.3, &[(SPREAD, 1.0), (DAMPING, 0.4)]).leak(250.0),
            k(FOLD, 12, -0.7, &[(DRIVE, 0.3), (SPREAD, -0.8)]).leak(200.0),
            k(FOLD, 19, 0.7, &[(DRIVE, 0.3), (SPREAD, 0.8)]).leak(200.0),
            k(GUIRO, 12, 0.1, &[(TONE, -0.4), (SPACING, -0.5)]).leak(120.0),
            k(MARACA, 0, -0.3, &[(TONE, 0.8), (GRAIN, 0.6)]).leak(80.0),
            k(CLAP, 0, 0.3, &[(SPACING, 0.7), (TONE, 0.5)]),
            k(
                CLOSED,
                0,
                0.5,
                &[(GRAIN, 0.8), (TONE, 0.667), (SPREAD, -1.0)],
            )
            .leak(50.0)
            .hats(),
            k(OPEN, 0, 0.5, &[(GRAIN, 0.8), (TONE, 0.667), (SPREAD, -1.0)])
                .leak(400.0)
                .hats(),
            k(CYMBAL, 12, -0.4, &[(SPREAD, 1.0), (DAMPING, 0.2)]).leak(700.0),
            k(FOLDED, 0, 0.4, &[(DRIVE, 0.4), (SPREAD, 1.0)]).leak(150.0),
            k(TAMB, 0, -0.6, &[(SPREAD, 0.7), (DAMPING, 0.7)]).leak(60.0),
            k(BRUSH, 0, 0.6, &[(GRAIN, -0.4), (SPREAD, 0.5)]).leak(200.0),
        ],
        ..INIT
    },
    // ---- Ringing: tones that sustain, howl and feed each other ------------------------------
    // High loops and rings that barely lose anything, coupled so the grid shimmers around a hit.
    Kit {
        name: "Crystal Cave",
        temper: 0.35,
        flow: 0.1,
        coupling: 0.85,
        seed: 509,
        keys: [
            k(KICK, 0, 0.0, &[(DAMPING, -0.4), (DROP, -0.8)]).leak(3000.0),
            k(
                SNARE,
                12,
                0.0,
                &[(BODY, 1.0), (TONE, 0.625), (DAMPING, -0.6)],
            )
            .leak(2500.0),
            k(BELL, -3, -0.6, &[(SPREAD, 0.6), (DAMPING, -0.9)]).leak(10000.0),
            k(BELL, 1, -0.3, &[(SPREAD, 0.6), (DAMPING, -0.9)]).leak(10000.0),
            k(BELL, 4, 0.0, &[(SPREAD, 0.6), (DAMPING, -0.9)]).leak(10000.0),
            k(RING, 24, 0.3, &[(DAMPING, -0.8)]).leak(8000.0),
            k(RING, 28, 0.6, &[(DAMPING, -0.8)]).leak(8000.0),
            k(LOOP, 31, -0.8, &[(SPREAD, 0.6), (DAMPING, -0.9)]).leak(10000.0),
            k(MARACA, 0, 0.8, &[(TONE, 1.0), (GRAIN, -0.5)]).leak(600.0),
            k(CLAP, 0, 0.0, &[(TONE, 1.0), (SPACING, 0.3)]),
            k(CLOSED, 0, 0.4, &[(SPREAD, 1.0), (TONE, 1.0), (GRAIN, -0.8)])
                .leak(600.0)
                .hats(),
            k(OPEN, 0, -0.4, &[(SPREAD, 1.0), (TONE, 1.0), (GRAIN, -0.8)])
                .leak(6000.0)
                .hats(),
            k(RIDE, 28, 0.0, &[(DAMPING, -1.0), (SPREAD, 0.3)]).leak(20000.0),
            k(CLAVE, -18, 0.5, &[(DAMPING, -1.0)]).leak(12000.0),
            k(SPARK, 0, -0.6, &[(TONE, 1.0), (GRAIN, -0.5)]).leak(600.0),
            k(TAMB, 0, 0.7, &[(SPREAD, 1.0), (TONE, 1.0)]).leak(2000.0),
        ],
        ..INIT
    },
    // Rings tipped into sustaining themselves, steady rather than growling: a held key plays a pipe.
    Kit {
        name: "Organ Pipes",
        keys: [
            k(KICK, -12, 0.0, &[(DRIVE, -0.4), (DROP, -1.0)])
                .threshold(-0.6)
                .supply(0.8)
                .hunger(0.2)
                .leak(6000.0),
            k(SNARE, 0, 0.0, &[(BODY, 1.0), (DRIVE, -0.4)])
                .threshold(-0.6)
                .supply(0.8),
            k(TOM, -12, -0.7, &[(DRIVE, -0.5), (DROP, -1.0)])
                .threshold(-0.7)
                .supply(0.8)
                .hunger(0.2)
                .leak(8000.0),
            k(TOM, -5, -0.4, &[(DRIVE, -0.5), (DROP, -1.0)])
                .threshold(-0.7)
                .supply(0.8)
                .hunger(0.2)
                .leak(8000.0),
            k(TOM, 0, -0.1, &[(DRIVE, -0.5), (DROP, -1.0)])
                .threshold(-0.7)
                .supply(0.8)
                .hunger(0.2)
                .leak(8000.0),
            k(TOM, 4, 0.2, &[(DRIVE, -0.5), (DROP, -1.0)])
                .threshold(-0.7)
                .supply(0.8)
                .hunger(0.2)
                .leak(8000.0),
            k(TOM, 7, 0.5, &[(DRIVE, -0.5), (DROP, -1.0)])
                .threshold(-0.7)
                .supply(0.8)
                .hunger(0.2)
                .leak(8000.0),
            k(TOM, 12, 0.8, &[(DRIVE, -0.5), (DROP, -1.0)])
                .threshold(-0.7)
                .supply(0.8)
                .hunger(0.2)
                .leak(8000.0),
            k(RING, 16, -0.6, &[(DRIVE, -0.5), (DROP, -1.0)])
                .threshold(-0.7)
                .supply(0.8)
                .hunger(0.2)
                .leak(8000.0),
            k(RING, 19, 0.6, &[(DRIVE, -0.5), (DROP, -1.0)])
                .threshold(-0.7)
                .supply(0.8)
                .hunger(0.2)
                .leak(8000.0),
            k(CLOSED, 12, 0.3, &[(SPREAD, 1.0), (TONE, 0.833)])
                .leak(200.0)
                .hats(),
            k(OPEN, 12, 0.3, &[(SPREAD, 1.0), (TONE, 0.833)])
                .supply(0.8)
                .leak(3000.0)
                .hats(),
            k(CLAVE, -30, -0.3, &[(DRIVE, -0.5), (DROP, -1.0)])
                .threshold(-0.7)
                .supply(0.8)
                .leak(8000.0),
            k(CLAVE, -26, 0.3, &[(DRIVE, -0.5), (DROP, -1.0)])
                .threshold(-0.7)
                .supply(0.8)
                .leak(8000.0),
            k(CLAVE, -23, 0.0, &[(DRIVE, -0.5), (DROP, -1.0)])
                .threshold(-0.7)
                .supply(0.8)
                .leak(8000.0),
            k(TOM, -24, 0.0, &[(DRIVE, -0.5), (DROP, -1.0)])
                .threshold(-0.7)
                .supply(0.8)
                .leak(8000.0),
        ],
        ..INIT
    },
    // Loops past onset, driven and coupled hard: each hit howls and sets its neighbours howling.
    Kit {
        name: "Feedback Loop",
        category: Fx,
        temper: 0.5,
        flow: 0.2,
        coupling: 1.0,
        seed: 613,
        keys: [
            k(KICK, 0, 0.0, &[(DRIVE, 0.6)]).threshold(-0.6),
            k(SNARE, 0, 0.0, &[(DRIVE, 0.6)]).threshold(-0.6),
            k(LOOP, -5, -0.6, &[(DRIVE, 0.8), (DAMPING, -0.5)])
                .threshold(-0.8)
                .touch(1.0),
            k(LOOP, 0, -0.2, &[(DRIVE, 0.8), (DAMPING, -0.5)])
                .threshold(-0.8)
                .touch(1.0),
            k(LOOP, 7, 0.2, &[(DRIVE, 0.8), (DAMPING, -0.5)])
                .threshold(-0.8)
                .touch(1.0),
            k(COWBELL, -4, 0.6, &[(DRIVE, 0.8), (SPREAD, 0.5)])
                .threshold(-0.8)
                .touch(1.0),
            k(COWBELL, 1, -0.8, &[(DRIVE, 0.8), (SPREAD, 0.5)])
                .threshold(-0.8)
                .touch(1.0),
            k(BELL, 7, 0.8, &[(DRIVE, 0.8), (SPREAD, 0.8)])
                .threshold(-0.8)
                .touch(1.0),
            k(MARACA, 0, -0.3, &[(GRAIN, 0.5)]).threshold(-0.8),
            k(CLAP, 0, 0.3, &[(GRAIN, 0.5)]).threshold(-0.7),
            k(CLOSED, 0, 0.4, &[(SPREAD, 0.7)]).leak(100.0).hats(),
            k(OPEN, 0, 0.4, &[(SPREAD, 0.7)]).leak(1500.0).hats(),
            k(LOOP, 29, 0.0, &[(DRIVE, 0.8), (SPREAD, 1.0)])
                .threshold(-0.8)
                .leak(4000.0),
            k(FOLDED, 12, 0.5, &[(DRIVE, 1.0)]).threshold(-0.6),
            k(LOOP, 34, -0.5, &[(DRIVE, 0.8)]).threshold(-0.8),
            k(GUIRO, 0, 0.6, &[(SPREAD, 1.0), (TONE, 0.667)]).leak(400.0),
        ],
        ..INIT
    },
    // Pure, tuned rings on a pentatonic with a shimmer, velocity bending them, ringing together.
    Kit {
        name: "Bell Riot",
        temper: 0.5,
        coupling: 0.8,
        flow: 0.3,
        seed: 727,
        lfos: &[free(2, Sine, 5.0)],
        keys: [
            k(KICK, 0, 0.0, &[(DROP, -0.8), (DAMPING, 0.2)]).routes(&[(
                Velocity,
                CellTarget::Pitch,
                0.4,
            )]),
            k(SNARE, 0, 0.0, &[(BODY, 1.0), (TONE, -0.4)]),
            k(BELL, -20, -0.7, &[(BODY, 0.5), (DAMPING, -0.5)])
                .leak(1200.0)
                .routes(&[
                    (Velocity, CellTarget::Pitch, 0.5),
                    (Lfo2, CellTarget::Pitch, 0.05),
                ]),
            k(BELL, -17, -0.4, &[(BODY, 0.5), (DAMPING, -0.5)])
                .leak(1200.0)
                .routes(&[
                    (Velocity, CellTarget::Pitch, 0.5),
                    (Lfo2, CellTarget::Pitch, 0.05),
                ]),
            k(BELL, -15, -0.1, &[(BODY, 0.5), (DAMPING, -0.5)])
                .leak(1200.0)
                .routes(&[
                    (Velocity, CellTarget::Pitch, 0.5),
                    (Lfo2, CellTarget::Pitch, -0.05),
                ]),
            k(BELL, -13, 0.2, &[(BODY, 0.5), (DAMPING, -0.5)])
                .leak(1200.0)
                .routes(&[
                    (Velocity, CellTarget::Pitch, 0.5),
                    (Lfo2, CellTarget::Pitch, -0.05),
                ]),
            k(BELL, -10, 0.5, &[(BODY, 0.5), (DAMPING, -0.5)])
                .leak(1200.0)
                .routes(&[
                    (Velocity, CellTarget::Pitch, 0.5),
                    (Lfo2, CellTarget::Pitch, 0.05),
                ]),
            k(BELL, -8, 0.8, &[(BODY, 0.5), (DAMPING, -0.5)])
                .leak(1200.0)
                .routes(&[
                    (Velocity, CellTarget::Pitch, 0.5),
                    (Lfo2, CellTarget::Pitch, -0.05),
                ]),
            k(RIM, -15, -0.3, &[(DAMPING, -0.5), (DROP, -0.8)]).leak(600.0),
            k(CLAP, 0, 0.3, &[(TONE, -0.3), (SPACING, 0.4)]),
            k(CLOSED, 0, 0.5, &[(SPREAD, 0.8)]).leak(80.0).hats(),
            k(OPEN, 0, 0.5, &[(SPREAD, 0.8)]).leak(700.0).hats(),
            k(RIDE, -3, -0.5, &[(DAMPING, -0.5)])
                .leak(2500.0)
                .routes(&[(Lfo2, CellTarget::Pitch, 0.05)]),
            k(COWBELL, -12, 0.6, &[(DAMPING, -0.5)]).leak(1500.0),
            k(CLAVE, -15, -0.8, &[(DAMPING, -0.5), (DROP, -0.8)]).leak(500.0),
            k(TAMB, 0, 0.7, &[(GRAIN, 0.5), (TONE, 0.4)]).leak(150.0),
        ],
        ..INIT
    },
    // Rings that drop far and wobble: the harder the hit, the longer the boing.
    Kit {
        name: "Rubber Band",
        lfos: &[free(2, Sine, 6.0)],
        keys: [
            k(KICK, -5, 0.0, &[(DROP, 0.8), (DAMPING, -0.3)]).routes(&[(Velocity, to(DROP), 1.0)]),
            k(SNARE, 0, 0.0, &[(DROP, 0.8), (BODY, 0.8)]).routes(&[(Velocity, to(DROP), 1.0)]),
            k(RING, -12, -0.6, &[(DROP, 0.8), (DAMPING, -0.3)])
                .routes(&[(Velocity, to(DROP), 1.0), (Lfo2, CellTarget::Pitch, 0.1)]),
            k(RING, -7, -0.2, &[(DROP, 0.8), (DAMPING, -0.3)])
                .routes(&[(Velocity, to(DROP), 1.0), (Lfo2, CellTarget::Pitch, 0.1)]),
            k(RING, -2, 0.2, &[(DROP, 0.8), (DAMPING, -0.3)])
                .routes(&[(Velocity, to(DROP), 1.0), (Lfo2, CellTarget::Pitch, 0.1)]),
            k(RING, 3, 0.6, &[(DROP, 0.8), (DAMPING, -0.3)])
                .routes(&[(Velocity, to(DROP), 1.0), (Lfo2, CellTarget::Pitch, -0.1)]),
            k(RING, 8, -0.8, &[(DROP, 0.8), (DAMPING, -0.3)])
                .routes(&[(Velocity, to(DROP), 1.0), (Lfo2, CellTarget::Pitch, -0.1)]),
            k(RING, 12, 0.8, &[(DROP, 0.8), (DAMPING, -0.3)]).routes(&[(Velocity, to(DROP), 1.0)]),
            k(SPARK, 0, -0.3, &[(TONE, -0.3), (SNAP, -0.5)]),
            k(CLAP, 0, 0.3, &[(SPACING, 0.5)]),
            k(CLOSED, 0, 0.4, &[(SPREAD, 0.5)]).leak(100.0).hats(),
            k(OPEN, 0, 0.4, &[(SPREAD, 0.5)]).leak(900.0).hats(),
            k(CYMBAL, -6, 0.0, &[(DAMPING, 0.4), (SPREAD, 0.4)])
                .leak(1500.0)
                .routes(&[(Lfo2, CellTarget::Pitch, 0.15)]),
            k(RIM, -8, 0.5, &[(DROP, 1.0), (DAMPING, 0.2)]).routes(&[(Velocity, to(DROP), 1.0)]),
            k(CLAVE, -30, -0.5, &[(DROP, 1.0), (DAMPING, 0.5)]),
            k(DUST, 0, 0.6, &[(GRAIN, -0.3)]).leak(200.0),
        ],
        ..INIT
    },
    // Huge, low, spread loops and dark noise, coupled into one another like a shaken sheet.
    Kit {
        name: "Thunder Sheet",
        temper: 0.6,
        flow: 0.5,
        coupling: 0.9,
        seed: 839,
        keys: [
            k(KICK, -10, 0.0, &[(DAMPING, -0.4), (DRIVE, 0.5)]).leak(2000.0),
            k(SNARE, -7, 0.0, &[(BODY, -0.9), (TONE, -0.6), (DRIVE, 0.8)]),
            k(LOOP, -24, -0.6, &[(SPREAD, 1.0), (DRIVE, 0.4)])
                .threshold(-0.3)
                .leak(6000.0),
            k(LOOP, -19, -0.2, &[(SPREAD, 1.0), (DRIVE, 0.4)])
                .threshold(-0.3)
                .leak(6000.0),
            k(LOOP, -14, 0.2, &[(SPREAD, 1.0), (DRIVE, 0.4)])
                .threshold(-0.3)
                .leak(6000.0),
            k(DUST, -12, 0.6, &[(TONE, -0.8), (GRAIN, 0.6)]).leak(3000.0),
            k(
                BRUSH,
                -12,
                -0.8,
                &[(TONE, -0.6), (GRAIN, 0.6), (SPREAD, 0.4)],
            )
            .leak(3000.0),
            k(BELL, -36, 0.8, &[(SPREAD, 1.0), (DRIVE, 0.4)])
                .threshold(-0.3)
                .leak(6000.0),
            k(MARACA, 0, -0.3, &[(TONE, -1.0), (GRAIN, 0.6)]).leak(1000.0),
            k(CLAP, 0, 0.3, &[(SPACING, 0.9), (TONE, -0.6)]),
            k(CLOSED, 0, 0.4, &[(TONE, -0.5), (GRAIN, 0.7)])
                .leak(300.0)
                .hats(),
            k(OPEN, 0, 0.4, &[(TONE, -0.5), (GRAIN, 0.7)])
                .leak(8000.0)
                .hats(),
            k(CRASH, -7, 0.0, &[(SPREAD, 1.0), (DAMPING, -0.5)]).leak(15000.0),
            k(FOLDED, -12, 0.5, &[(DRIVE, 1.0)]).leak(2000.0),
            k(TOM, -30, -0.5, &[(DROP, 1.0), (DRIVE, 0.6)]).leak(3000.0),
            k(DUST, -24, 0.6, &[(TONE, -1.0), (GRAIN, 0.8)]).leak(4000.0),
        ],
        ..INIT
    },
    // High rings that fall far and fast, each at a random pitch: drips into a bucket.
    Kit {
        name: "Water Drops",
        seed: 947,
        keys: [
            k(KICK, 12, 0.0, &[(DROP, 1.0), (DAMPING, 0.6)])
                .leak(200.0)
                .routes(&[(Random, CellTarget::Pitch, 0.3)]),
            k(SNARE, 12, 0.0, &[(BODY, 1.0), (DROP, 1.0), (DAMPING, 0.5)]),
            k(RING, 19, -0.7, &[(DROP, 1.0), (DAMPING, 0.3)])
                .leak(250.0)
                .routes(&[(Random, CellTarget::Pitch, 0.5)]),
            k(RING, 24, -0.4, &[(DROP, 1.0), (DAMPING, 0.3)])
                .leak(250.0)
                .routes(&[(Random, CellTarget::Pitch, 0.5)]),
            k(RING, 28, -0.1, &[(DROP, 1.0), (DAMPING, 0.3)])
                .leak(250.0)
                .routes(&[(Random, CellTarget::Pitch, 0.5)]),
            k(BONGO, 12, 0.2, &[(DROP, 1.0), (DAMPING, 0.3)])
                .leak(250.0)
                .routes(&[(Random, CellTarget::Pitch, 0.5)]),
            k(BONGO, 15, 0.5, &[(DROP, 1.0), (DAMPING, 0.3)])
                .leak(250.0)
                .routes(&[(Random, CellTarget::Pitch, 0.5)]),
            k(RING, 36, 0.8, &[(DROP, 1.0), (DAMPING, 0.3)])
                .leak(250.0)
                .routes(&[(Random, CellTarget::Pitch, 0.5)]),
            k(MARACA, 0, -0.3, &[(TONE, 1.0), (GRAIN, -0.6)]).leak(150.0),
            k(
                LOOP,
                24,
                0.3,
                &[(DAMPING, 0.2), (SNAP, -1.0), (SPREAD, -1.0)],
            )
            .leak(400.0),
            k(
                CLOSED,
                0,
                0.5,
                &[(TONE, 0.333), (SPREAD, 1.0), (GRAIN, -1.0)],
            )
            .leak(150.0)
            .hats(),
            k(OPEN, 0, 0.5, &[(TONE, 0.333), (SPREAD, 1.0), (GRAIN, -1.0)])
                .leak(1500.0)
                .hats(),
            k(RIDE, 16, -0.5, &[(DAMPING, -0.4), (SPREAD, -1.0)]).leak(2500.0),
            k(CLAVE, -14, 0.4, &[(DROP, 1.0), (DAMPING, 0.4)])
                .leak(150.0)
                .routes(&[(Random, CellTarget::Pitch, 0.5)]),
            k(SPARK, 0, -0.6, &[(TONE, 0.8), (GRAIN, -1.0)]).leak(400.0),
            k(DUST, 7, 0.6, &[(SPREAD, 1.0), (TONE, 0.667), (GRAIN, -0.8)]).leak(500.0),
        ],
        ..INIT
    },
    // Driven, sub-low rings and folds and rumbling noise that shake the grid as they spread.
    Kit {
        name: "Earthquake",
        temper: 0.5,
        flow: 0.8,
        coupling: 0.8,
        seed: 1051,
        keys: [
            k(KICK, -12, 0.0, &[(DRIVE, 1.0), (DAMPING, -0.4)])
                .threshold(-0.6)
                .leak(2000.0),
            k(SNARE, -12, 0.0, &[(TONE, -1.0), (BODY, -0.4), (DRIVE, 1.0)]),
            k(TOM, -24, -0.6, &[(DRIVE, 0.8)])
                .threshold(-0.5)
                .leak(2500.0),
            k(TOM, -19, -0.2, &[(DRIVE, 0.8)])
                .threshold(-0.5)
                .leak(2500.0),
            k(TOM, -12, 0.2, &[(DRIVE, 0.8)])
                .threshold(-0.5)
                .leak(2500.0),
            k(FOLD, -36, 0.6, &[(DRIVE, 1.0)]).leak(2000.0),
            k(FOLD, -31, -0.8, &[(DRIVE, 1.0)]).leak(2000.0),
            k(COWBELL, -36, 0.8, &[(DRIVE, 0.8), (SPREAD, 0.8)]).leak(2000.0),
            k(RIM, -24, -0.3, &[(TONE, -1.0), (SNAP, -1.0)]).leak(800.0),
            k(CLAP, 0, 0.3, &[(TONE, -1.0), (SPACING, 0.8)]),
            k(CLOSED, 0, 0.4, &[(TONE, -1.0), (GRAIN, 0.5)])
                .leak(300.0)
                .hats(),
            k(OPEN, 0, 0.4, &[(TONE, -1.0), (GRAIN, 0.5)])
                .leak(4000.0)
                .hats(),
            k(LOOP, -24, 0.0, &[(SPREAD, 1.0), (DRIVE, 1.0)])
                .threshold(-0.4)
                .leak(8000.0),
            k(BELL, -36, 0.5, &[(DRIVE, 1.0)]).leak(1500.0),
            k(RING, -30, -0.5, &[(DROP, 1.0), (DRIVE, 0.5)]).leak(1500.0),
            k(BRUSH, -12, 0.6, &[(TONE, -1.0), (GRAIN, 1.0)]).leak(3000.0),
        ],
        ..INIT
    },
    // The highest, shortest, most driven metal and spark and noise: every hit a spray of shards.
    Kit {
        name: "Shattered",
        category: Fx,
        temper: 0.6,
        flow: 0.4,
        coupling: 0.7,
        seed: 1163,
        keys: [
            k(KICK, 24, 0.0, &[(DRIVE, 1.0), (DAMPING, 0.8), (SNAP, 1.0)]).leak(150.0),
            k(SNARE, 24, 0.0, &[(TONE, 0.625), (BODY, -1.0), (SNAP, 1.0)]).leak(150.0),
            k(COWBELL, 24, -0.6, &[(SPREAD, 1.0), (DRIVE, 1.0)])
                .threshold(-0.5)
                .leak(150.0),
            k(COWBELL, 27, -0.2, &[(SPREAD, 1.0), (DRIVE, 1.0)])
                .threshold(-0.5)
                .leak(150.0),
            k(COWBELL, 29, 0.2, &[(SPREAD, 1.0), (DRIVE, 1.0)])
                .threshold(-0.5)
                .leak(150.0),
            k(MARACA, 0, 0.6, &[(GRAIN, 1.0), (TONE, 1.0), (SNAP, 1.0)]).leak(150.0),
            k(TAMB, 12, -0.8, &[(TONE, 1.0), (GRAIN, 0.4), (SPREAD, -0.5)]).leak(150.0),
            k(SPARK, 0, 0.8, &[(GRAIN, 1.0), (TONE, 1.0)]).leak(150.0),
            k(SPARK, 0, -0.3, &[(GRAIN, 1.0), (TONE, 1.0)]).leak(150.0),
            k(
                CLAP,
                0,
                0.3,
                &[(GRAIN, 1.0), (TONE, 1.0), (SPACING, -1.0), (SNAP, 1.0)],
            ),
            k(CLOSED, 0, 0.5, &[(TONE, 1.0), (GRAIN, 1.0)])
                .leak(150.0)
                .hats(),
            k(OPEN, 0, 0.5, &[(TONE, 1.0), (GRAIN, 1.0)])
                .leak(300.0)
                .hats(),
            k(LOOP, 48, 0.0, &[(SPREAD, 1.0), (DRIVE, 1.0)])
                .threshold(-0.5)
                .leak(200.0),
            k(FOLDED, 24, 0.4, &[(DRIVE, 1.0), (SPREAD, 1.0)]).leak(150.0),
            k(SPARK, 0, -0.6, &[(GRAIN, 1.0), (TONE, 1.0), (SNAP, 1.0)]).leak(150.0),
            k(DUST, 24, 0.7, &[(TONE, 1.0), (GRAIN, 1.0)]).leak(150.0),
        ],
        ..INIT
    },
    // A tiny pool that refills like a pulse, and a kick that swells on the beat and drains between.
    Kit {
        name: "Heartbeat",
        pool: 0.2,
        recharge_ms: Some(800.0),
        lfos: &[synced(1, Sine, Division::Quarter)],
        keys: [
            k(KICK, -7, 0.0, &[(DAMPING, -0.4)]).supply(1.0).routes(&[(
                Lfo1,
                CellTarget::Supply,
                0.8,
            )]),
            k(SNARE, -5, 0.0, &[(BODY, 0.8), (TONE, -0.5)]).supply(1.0),
            k(TOM, -19, -0.5, &[(DROP, 0.5), (DAMPING, -0.3)]).supply(1.0),
            k(TOM, -14, 0.0, &[(DROP, 0.5), (DAMPING, -0.3)]).supply(1.0),
            k(TOM, -9, 0.5, &[(DROP, 0.5), (DAMPING, -0.3)]).supply(1.0),
            k(CONGA, -22, -0.7, &[(DRIVE, -0.3)]).supply(1.0),
            k(CONGA, -17, 0.7, &[(DRIVE, -0.3)]).supply(1.0),
            k(BONGO, -19, 0.2, &[(DAMPING, 0.6)]).supply(1.0),
            k(RIM, -12, -0.3, &[(TONE, -0.7)]).supply(1.0),
            k(CLAP, 0, 0.3, &[(TONE, -0.5), (SPACING, 0.4)]).supply(1.0),
            k(CLOSED, 0, 0.4, &[(TONE, -0.4)])
                .supply(1.0)
                .leak(150.0)
                .hats(),
            k(OPEN, 0, 0.4, &[(TONE, -0.4)])
                .supply(1.0)
                .leak(1500.0)
                .hats(),
            k(CRASH, -5, -0.4, &[(SPREAD, 0.6), (DAMPING, 0.3)])
                .supply(1.0)
                .leak(3000.0),
            k(COWBELL, -12, 0.4, &[]).supply(1.0),
            k(MARACA, 0, -0.6, &[(TONE, -0.5)]).supply(1.0),
            k(BRUSH, -5, 0.6, &[(TONE, -0.6), (GRAIN, -0.4)])
                .supply(1.0)
                .leak(300.0),
        ],
        ..INIT
    },
    // ---- Grit: sparks, noise and broken machines ---------------------------------------------
    // Sparks firing so fast they buzz, gated by a fast square: a kit of arcing coils.
    Kit {
        name: "Tesla Coil",
        lfos: &[free(1, Square, 9.0)],
        keys: [
            k(KICK, 0, 0.0, &[(DRIVE, 1.0), (SNAP, 1.0)]).threshold(-0.5),
            k(SNARE, 0, 0.0, &[(DRIVE, 1.0), (BODY, -0.7), (TONE, 0.625)]).threshold(-0.6),
            k(SPARK, 0, -0.6, &[(GRAIN, 0.8), (TONE, -0.6)])
                .threshold(-0.7)
                .supply(1.0)
                .hunger(0.3)
                .leak(1500.0)
                .routes(&[(Lfo1, CellTarget::Threshold, 0.5)]),
            k(SPARK, 0, -0.2, &[(GRAIN, 0.9), (TONE, -0.2)])
                .threshold(-0.7)
                .supply(1.0)
                .hunger(0.3)
                .leak(1500.0)
                .routes(&[(Lfo1, CellTarget::Threshold, 0.5)]),
            k(SPARK, 0, 0.2, &[(GRAIN, 1.0), (TONE, 0.4)])
                .threshold(-0.7)
                .supply(1.0)
                .hunger(0.3)
                .leak(1500.0)
                .routes(&[(Lfo1, CellTarget::Threshold, -0.5)]),
            k(MARACA, 0, 0.6, &[(GRAIN, 0.8), (TONE, 1.0), (SNAP, -0.8)])
                .threshold(-0.7)
                .supply(1.0)
                .leak(1500.0),
            k(SPARK, 0, -0.8, &[(GRAIN, 0.9), (TONE, 1.0), (SNAP, 0.8)])
                .threshold(-0.7)
                .supply(1.0)
                .leak(1500.0),
            k(SPARK, 0, 0.8, &[(GRAIN, 1.0), (TONE, -0.9)])
                .threshold(-0.7)
                .supply(1.0)
                .leak(1500.0),
            k(SPARK, 0, -0.3, &[(GRAIN, 1.0), (TONE, 0.6)])
                .threshold(-0.8)
                .leak(200.0),
            k(CLAP, 0, 0.3, &[(GRAIN, 1.0), (SPACING, -1.0), (TONE, 0.6)]).threshold(-0.6),
            k(FOLDED, 0, 0.4, &[(GRAIN, 0.9), (TONE, 1.0)])
                .leak(120.0)
                .hats(),
            k(OPEN, 0, 0.4, &[(GRAIN, 0.9), (TONE, 1.0)])
                .threshold(-0.6)
                .leak(1500.0)
                .hats(),
            k(LOOP, 24, 0.0, &[(DRIVE, 1.0), (SPREAD, 1.0)])
                .threshold(-0.6)
                .leak(2000.0),
            k(FOLD, 19, 0.5, &[(DRIVE, 1.0)]).threshold(-0.5),
            k(SPARK, 0, -0.5, &[(GRAIN, 1.0), (TONE, 1.0)]).leak(120.0),
            k(GUIRO, 0, 0.6, &[(TONE, 1.0), (SPACING, -1.0)])
                .threshold(-0.8)
                .leak(800.0),
        ],
        ..INIT
    },
    // Noise on nearly every key, from rumble to hiss, sparse to dense, all of it coupled.
    Kit {
        name: "Static Storm",
        category: Fx,
        temper: 0.7,
        flow: 1.0,
        coupling: 1.0,
        seed: 1277,
        keys: [
            k(KICK, -2, 0.0, &[(DRIVE, 0.8)]),
            k(SNARE, 0, 0.0, &[(BODY, -1.0), (TONE, -0.3)]),
            k(
                BRUSH,
                0,
                -0.6,
                &[(TONE, -1.0), (SPREAD, -1.0), (GRAIN, 0.6)],
            )
            .leak(800.0),
            k(TAMB, 0, -0.2, &[(TONE, -0.6), (SPREAD, 0.5), (GRAIN, -0.6)]).leak(800.0),
            k(CRASH, 0, 0.2, &[(TONE, -0.2), (SPREAD, 1.0), (GRAIN, 0.2)]).leak(800.0),
            k(
                BRUSH,
                0,
                0.6,
                &[(TONE, 0.333), (SPREAD, -0.5), (GRAIN, -1.0)],
            )
            .leak(800.0),
            k(CYMBAL, 0, -0.8, &[(TONE, 1.0), (SPREAD, 0.8), (GRAIN, 0.5)]).leak(800.0),
            k(DUST, 0, 0.8, &[(TONE, 1.0), (SPREAD, -0.2), (GRAIN, -0.3)]).leak(800.0),
            k(MARACA, 0, -0.3, &[(GRAIN, 1.0), (TONE, -0.5)]).leak(400.0),
            k(CLAP, 0, 0.3, &[(GRAIN, 1.0), (SPACING, -0.6)]),
            k(CLOSED, 0, 0.4, &[(TONE, 1.0), (GRAIN, 1.0)])
                .leak(80.0)
                .hats(),
            k(OPEN, 0, 0.4, &[(TONE, 0.833), (GRAIN, 1.0)])
                .leak(2000.0)
                .hats(),
            k(RIDE, 0, 0.0, &[(TONE, 0.667), (SPREAD, 1.0)]).leak(4000.0),
            k(FOLDED, 0, 0.5, &[(DRIVE, 1.0)]),
            k(SPARK, 0, -0.5, &[(GRAIN, 1.0), (TONE, 1.0)]).leak(200.0),
            k(DUST, 0, 0.6, &[(GRAIN, -1.0), (TONE, 0.5)]).leak(1500.0),
        ],
        ..INIT
    },
    // Sparks that crackle for seconds after a hit, cut in and out by a stepped LFO on the eighths.
    Kit {
        name: "Crackle Gate",
        seed: 1381,
        lfos: &[synced(3, SampleHold, Division::Eighth)],
        keys: [
            k(KICK, 0, 0.0, &[(DAMPING, 0.3)]).leak(300.0),
            k(SNARE, 0, 0.0, &[(BODY, -0.5)]).leak(300.0),
            k(SPARK, 0, -0.6, &[(GRAIN, -0.6), (TONE, -0.5)])
                .threshold(-0.8)
                .hunger(0.1)
                .leak(6000.0)
                .routes(&[(Lfo3, CellTarget::Threshold, 0.6)]),
            k(SPARK, 0, -0.2, &[(GRAIN, -0.2), (TONE, 0.0)])
                .threshold(-0.8)
                .hunger(0.1)
                .leak(6000.0)
                .routes(&[(Lfo3, CellTarget::Threshold, -0.6)]),
            k(SPARK, 0, 0.2, &[(GRAIN, 0.2), (TONE, 0.8)])
                .threshold(-0.8)
                .hunger(0.1)
                .leak(6000.0)
                .routes(&[(Lfo3, CellTarget::Threshold, 0.6)]),
            k(SPARK, 0, 0.6, &[(GRAIN, 0.6), (TONE, 1.0)])
                .threshold(-0.8)
                .hunger(0.1)
                .leak(6000.0)
                .routes(&[(Lfo3, CellTarget::Threshold, -0.6)]),
            k(MARACA, 0, -0.8, &[(GRAIN, -0.9), (TONE, 1.0), (SNAP, -1.0)])
                .threshold(-0.8)
                .hunger(0.1)
                .leak(6000.0),
            k(SPARK, 0, 0.8, &[(GRAIN, 0.9), (TONE, -0.8)])
                .threshold(-0.8)
                .hunger(0.1)
                .leak(6000.0),
            k(SPARK, 0, -0.3, &[(GRAIN, 0.3)])
                .threshold(-0.6)
                .leak(1500.0),
            k(CLAP, 0, 0.3, &[(SPACING, 0.4)]).threshold(-0.6),
            k(CLOSED, 0, 0.4, &[(GRAIN, -0.8)]).leak(100.0).hats(),
            k(OPEN, 0, 0.4, &[(GRAIN, -0.8)]).leak(3000.0).hats(),
            k(CYMBAL, 12, 0.0, &[(SPREAD, 0.5)]).leak(1600.0),
            k(COWBELL, 0, 0.5, &[(DRIVE, 0.5)]),
            k(GUIRO, 0, -0.5, &[(TONE, 1.0)])
                .threshold(-0.8)
                .leak(3000.0)
                .routes(&[(Lfo3, CellTarget::Threshold, 0.8)]),
            k(DUST, 0, 0.6, &[(GRAIN, -1.0), (TONE, 1.0)]).leak(2000.0),
        ],
        ..INIT
    },
    // Thin ticks and the sparsest, highest whistling noise: frost forming.
    Kit {
        name: "Ice Crackle",
        keys: [
            k(KICK, 19, 0.0, &[(DAMPING, 0.5), (SNAP, 1.0)]).leak(150.0),
            k(SNARE, 12, 0.0, &[(TONE, 0.625), (BODY, -1.0), (SNAP, 1.0)]).leak(150.0),
            k(SPARK, 0, -0.6, &[(TONE, 1.0), (GRAIN, -0.3)]).leak(600.0),
            k(MARACA, 0, -0.2, &[(TONE, 1.0), (GRAIN, -0.5)]).leak(600.0),
            k(SPARK, 0, 0.2, &[(TONE, 1.0), (GRAIN, -0.1)]).leak(600.0),
            k(TAMB, 0, 0.6, &[(TONE, 1.0), (SPREAD, 0.6), (GRAIN, -0.9)]).leak(1000.0),
            k(DUST, 0, -0.8, &[(TONE, 1.0), (SPREAD, 0.8), (GRAIN, -1.0)]).leak(1000.0),
            k(BELL, 21, 0.8, &[(DAMPING, -0.5), (SPREAD, 0.3)]).leak(1500.0),
            k(RIM, 24, -0.3, &[(TONE, 1.0), (SNAP, 1.0)]).leak(60.0),
            k(CLAP, 0, 0.3, &[(TONE, 1.0), (GRAIN, 1.0), (SNAP, 1.0)]),
            k(CLOSED, 0, 0.4, &[(TONE, 1.0), (GRAIN, -0.6)])
                .leak(100.0)
                .hats(),
            k(OPEN, 0, 0.4, &[(TONE, 1.0), (GRAIN, -0.6)])
                .leak(2500.0)
                .hats(),
            k(RIDE, 31, 0.0, &[(DAMPING, -0.8), (SPREAD, 0.6)]).leak(5000.0),
            k(LOOP, 43, 0.5, &[(DAMPING, 0.3), (SPREAD, -1.0)]).leak(200.0),
            k(SPARK, 0, -0.5, &[(TONE, 1.0), (GRAIN, -1.0)]).leak(1500.0),
            k(TAMB, 0, 0.6, &[(TONE, 1.0), (SPREAD, 1.0), (GRAIN, -1.0)]).leak(2000.0),
        ],
        ..INIT
    },
    // Sparse random clicks that go on long after a hit, each its own tone, and a bus that ticks alone.
    Kit {
        name: "Geiger Counter",
        category: Fx,
        bus: [0.0, 0.3],
        seed: 1489,
        keys: [
            k(KICK, 0, 0.0, &[(DAMPING, 0.4)]).leak(300.0),
            k(SNARE, 5, 0.0, &[(BODY, -0.6), (SNAP, 0.6)]).leak(300.0),
            k(SPARK, 0, -0.6, &[(GRAIN, -1.0), (SNAP, 1.0)])
                .threshold(-0.8)
                .supply(1.0)
                .leak(10000.0)
                .routes(&[(Random, to(TONE), 0.5)]),
            k(SPARK, 0, -0.2, &[(GRAIN, -1.0), (SNAP, 1.0), (TONE, 0.8)])
                .threshold(-0.8)
                .supply(1.0)
                .leak(10000.0)
                .routes(&[(Random, to(TONE), 0.5)]),
            k(MARACA, 0, 0.2, &[(GRAIN, -1.0), (SNAP, 1.0), (TONE, 1.0)])
                .threshold(-0.8)
                .supply(1.0)
                .leak(10000.0)
                .routes(&[(Random, to(TONE), 0.5)]),
            k(SPARK, 0, 0.6, &[(GRAIN, -0.8), (TONE, -0.4)])
                .bus(B)
                .threshold(-0.8)
                .supply(0.5),
            k(SPARK, 0, -0.8, &[(GRAIN, -0.8), (TONE, 1.0)])
                .bus(B)
                .threshold(-0.8)
                .supply(0.5),
            k(SPARK, 0, 0.8, &[(GRAIN, -0.7), (TONE, 1.0)])
                .bus(B)
                .threshold(-0.8)
                .supply(0.5),
            k(SPARK, 0, -0.3, &[(GRAIN, -0.5), (SNAP, 1.0)]).leak(800.0),
            k(CLAP, 0, 0.3, &[(GRAIN, -1.0), (SPACING, 1.0)]),
            k(CLOSED, 0, 0.4, &[(GRAIN, -1.0), (TONE, 1.0)])
                .leak(150.0)
                .hats(),
            k(OPEN, 0, 0.4, &[(GRAIN, -1.0), (TONE, 1.0)])
                .leak(4000.0)
                .hats(),
            k(CYMBAL, 12, 0.0, &[(SPREAD, -1.0), (DAMPING, 0.5)]).leak(600.0),
            k(CLAVE, -12, 0.5, &[(DRIVE, -0.5)]).leak(300.0),
            k(SPARK, 0, -0.5, &[(GRAIN, -1.0), (TONE, 1.0)])
                .threshold(-0.8)
                .leak(6000.0),
            k(BRUSH, 0, 0.6, &[(GRAIN, -1.0), (SPREAD, 0.8)]).leak(3000.0),
        ],
        ..INIT
    },
    // Ticks gated on the eighths and short high tocks: a clock, its works, its spring.
    Kit {
        name: "Clockwork",
        lfos: &[synced(2, Square, Division::Eighth)],
        keys: [
            k(KICK, 7, 0.0, &[(DAMPING, 0.7), (SNAP, 0.8)]).leak(120.0),
            k(SNARE, 12, 0.0, &[(BODY, 0.8), (DAMPING, 0.7), (SNAP, 1.0)]).leak(100.0),
            k(SPARK, 0, -0.6, &[(GRAIN, -0.4), (SNAP, 0.8), (TONE, 1.0)])
                .threshold(-0.7)
                .leak(2000.0)
                .routes(&[(Lfo2, CellTarget::Threshold, 0.6)]),
            k(SPARK, 0, -0.2, &[(GRAIN, -0.3), (SNAP, 0.8), (TONE, 1.0)])
                .threshold(-0.7)
                .leak(2000.0)
                .routes(&[(Lfo2, CellTarget::Threshold, -0.6)]),
            k(LOOP, 29, 0.2, &[(DAMPING, 0.6), (SPREAD, -1.0)]).leak(80.0),
            k(COWBELL, 19, 0.6, &[(DAMPING, 0.6), (SPREAD, 0.4)]).leak(80.0),
            k(CLAVE, -12, -0.8, &[(DAMPING, 0.7)]).leak(80.0),
            k(COWBELL, 24, 0.8, &[(DAMPING, 0.6), (SPREAD, -1.0)]).leak(60.0),
            k(RIM, 24, -0.3, &[(SNAP, 1.0), (TONE, 1.0)]).leak(40.0),
            k(CLAP, 0, 0.3, &[(SPACING, 0.8), (SNAP, 1.0), (GRAIN, -0.6)]),
            k(CLOSED, 0, 0.4, &[(TONE, 1.0), (BODY, 0.8)])
                .leak(30.0)
                .hats(),
            k(OPEN, 0, 0.4, &[(TONE, 1.0), (GRAIN, -0.6)])
                .threshold(-0.6)
                .leak(1500.0)
                .hats(),
            k(RIDE, 16, 0.0, &[(SPREAD, 1.0), (DAMPING, -0.2)]).leak(3000.0),
            k(FOLD, 24, 0.5, &[(DRIVE, -0.6)]).leak(80.0),
            k(SPARK, 0, -0.5, &[(SNAP, 1.0), (TONE, 1.0)]).leak(30.0),
            k(GUIRO, 0, 0.6, &[(DAMPING, 0.9), (SPACING, 0.6)]).leak(100.0),
        ],
        ..INIT
    },
    // Slow ratchets, hissing bursts and pumped thuds from a tiny pool that refills fast.
    Kit {
        name: "Hydraulic",
        pool: 0.2,
        recharge_ms: Some(150.0),
        keys: [
            k(KICK, -3, 0.0, &[(DRIVE, 0.5), (DAMPING, 0.2)]).supply(1.0),
            k(SNARE, -5, 0.0, &[(BODY, -1.0), (TONE, -0.8)]).supply(1.0),
            k(TOM, 7, -0.6, &[(DAMPING, 0.8), (SNAP, 1.0)]).supply(1.0),
            k(TOM, 12, -0.2, &[(DAMPING, 0.8), (SNAP, 1.0)]).supply(1.0),
            k(TOM, 17, 0.2, &[(DAMPING, 0.8), (SNAP, 1.0)]).supply(1.0),
            k(FOLD, -12, 0.6, &[(DRIVE, 0.6)]).supply(1.0),
            k(FOLD, -7, -0.8, &[(DRIVE, 0.6)]).supply(1.0),
            k(GUIRO, -12, 0.8, &[(SPACING, 1.0), (TONE, -0.3)]).supply(1.0),
            k(RIM, -12, -0.3, &[(SNAP, -1.0), (TONE, -0.6)]).supply(1.0),
            k(CLAP, 0, 0.3, &[(SPACING, 0.8), (GRAIN, -0.3)]).supply(1.0),
            k(CLOSED, 0, 0.4, &[(TONE, -0.4), (GRAIN, 0.8)])
                .supply(1.0)
                .leak(150.0)
                .hats(),
            k(OPEN, 0, 0.4, &[(TONE, -0.4), (GRAIN, 0.8)])
                .supply(1.0)
                .leak(1200.0)
                .hats(),
            k(BRUSH, 0, 0.0, &[(TONE, 0.0), (GRAIN, 1.0)])
                .supply(1.0)
                .leak(2500.0),
            k(COWBELL, -12, 0.5, &[(DRIVE, 0.4)]).supply(1.0),
            k(MARACA, 0, -0.6, &[(SNAP, -1.0)]).supply(1.0),
            k(GUIRO, 0, 0.6, &[(SPACING, 1.0), (TONE, 0.4)]).supply(1.0),
        ],
        ..INIT
    },
    // Every tone warbling slowly and fluttering fast, crackle on top: a stretched old tape.
    Kit {
        name: "Tape Mangle",
        lfos: &[free(1, Triangle, 0.4), free(2, Sine, 7.0)],
        keys: [
            k(KICK, -2, 0.0, &[(DROP, -0.6), (DAMPING, -0.2)]).routes(&[(
                Lfo1,
                CellTarget::Pitch,
                0.3,
            )]),
            k(SNARE, 0, 0.0, &[(TONE, -0.4), (BODY, 0.4)]).routes(&[(
                Lfo1,
                CellTarget::Pitch,
                0.3,
            )]),
            k(TOM, -8, -0.6, &[(DAMPING, 0.8)]).routes(&[
                (Lfo1, CellTarget::Pitch, 0.3),
                (Lfo2, CellTarget::Pitch, 0.06),
            ]),
            k(TOM, -3, -0.2, &[(DAMPING, 0.8)]).routes(&[
                (Lfo1, CellTarget::Pitch, 0.3),
                (Lfo2, CellTarget::Pitch, 0.06),
            ]),
            k(TOM, 2, 0.2, &[(DAMPING, 0.8)]).routes(&[
                (Lfo1, CellTarget::Pitch, 0.3),
                (Lfo2, CellTarget::Pitch, 0.06),
            ]),
            k(CONGA, -15, 0.6, &[(DAMPING, -0.4)])
                .leak(1500.0)
                .routes(&[(Lfo1, CellTarget::Pitch, 0.3)]),
            k(CONGA, -8, -0.8, &[(DAMPING, -0.4)])
                .leak(1500.0)
                .routes(&[(Lfo1, CellTarget::Pitch, 0.3)]),
            k(BONGO, -26, 0.8, &[(DRIVE, -0.4)]).routes(&[(Lfo1, CellTarget::Pitch, 0.3)]),
            k(MARACA, 0, -0.3, &[(TONE, -0.6), (GRAIN, -0.8)]).leak(1500.0),
            k(CLAP, 0, 0.3, &[(TONE, -0.5)]),
            k(CLOSED, 0, 0.4, &[(TONE, -0.6), (GRAIN, -0.8)])
                .leak(200.0)
                .hats(),
            k(OPEN, 0, 0.4, &[(TONE, -0.6), (GRAIN, -0.8)])
                .leak(3000.0)
                .hats(),
            k(CRASH, -5, 0.0, &[(SPREAD, 0.5), (DAMPING, 0.4)])
                .leak(4000.0)
                .routes(&[(Lfo1, CellTarget::Pitch, 0.3)]),
            k(COWBELL, -7, 0.5, &[]).routes(&[(Lfo1, CellTarget::Pitch, 0.3)]),
            k(SPARK, 0, -0.5, &[(GRAIN, -1.0), (TONE, -0.3)]).leak(3000.0),
            k(DUST, 0, 0.6, &[(TONE, -0.6), (GRAIN, -0.8)]).leak(5000.0),
        ],
        ..INIT
    },
    // Whistling noise whose tone jumps on the sixteenths and at random, between chaotic folds.
    Kit {
        name: "Broken Radio",
        category: Fx,
        temper: 0.4,
        coupling: 0.6,
        seed: 4242,
        lfos: &[
            free(1, Sine, 0.13),
            synced(3, SampleHold, Division::Sixteenth),
        ],
        keys: [
            k(KICK, -4, 0.0, &[(DRIVE, 0.6), (DAMPING, 0.3)]),
            k(SNARE, 0, 0.0, &[(BODY, -1.0), (TONE, 0.375)]).routes(&[(Lfo3, to(TONE), 0.8)]),
            k(DUST, 0, -0.6, &[(SPREAD, 1.0), (TONE, -0.6)])
                .leak(400.0)
                .routes(&[(Lfo3, to(TONE), 0.6), (Random, to(TONE), 0.4)]),
            k(DUST, 0, -0.2, &[(SPREAD, 1.0), (TONE, -0.2)])
                .leak(400.0)
                .routes(&[(Lfo3, to(TONE), -0.6), (Random, to(TONE), 0.4)]),
            k(DUST, 0, 0.2, &[(SPREAD, 1.0), (TONE, 0.333)])
                .leak(400.0)
                .routes(&[(Lfo3, to(TONE), 0.6), (Random, to(TONE), 0.4)]),
            k(TAMB, 0, 0.6, &[(SPREAD, 1.0), (TONE, 1.0)])
                .leak(400.0)
                .routes(&[(Lfo3, to(TONE), -0.6), (Random, to(TONE), 0.4)]),
            k(FOLDED, 0, -0.8, &[(DRIVE, 1.0), (SPREAD, 0.6)]).leak(200.0),
            k(FOLD, 19, 0.8, &[(DRIVE, 1.0), (SPREAD, -0.6)]).leak(200.0),
            k(SPARK, 0, -0.3, &[(GRAIN, -0.8), (TONE, 1.0)])
                .threshold(-0.6)
                .leak(2000.0),
            k(CLAP, 0, 0.3, &[(TONE, -0.5), (SPACING, 0.9), (GRAIN, -0.7)]),
            k(CLOSED, 0, 0.4, &[(SPREAD, 0.9)])
                .leak(150.0)
                .hats()
                .routes(&[(Lfo1, to(TONE), 0.8)]),
            k(OPEN, 0, 0.4, &[(SPREAD, 0.9)])
                .leak(1500.0)
                .hats()
                .routes(&[(Lfo1, to(TONE), 0.8)]),
            k(LOOP, 5, 0.0, &[(SPREAD, 1.0), (DAMPING, 0.7), (DRIVE, 0.8)])
                .threshold(-0.5)
                .leak(1500.0),
            k(COWBELL, -19, 0.5, &[(DRIVE, 0.9)]).routes(&[(Lfo3, CellTarget::Pitch, 1.0)]),
            k(SPARK, 0, -0.5, &[(GRAIN, -0.8), (TONE, 1.0)]).leak(1000.0),
            k(GUIRO, 0, 0.6, &[(SPREAD, 1.0), (TONE, 1.0)])
                .leak(800.0)
                .routes(&[(Lfo1, to(TONE), -0.8)]),
        ],
        ..INIT
    },
    // Folds on every key, driven into chaos and spread over five octaves: tone torn into noise.
    Kit {
        name: "Chaos Folds",
        category: Fx,
        temper: 0.3,
        seed: 1597,
        keys: [
            k(FOLD, -24, 0.0, &[(DRIVE, 0.4), (SPREAD, -0.5)]).leak(800.0),
            k(FOLD, 12, 0.0, &[(DRIVE, 1.0), (SPREAD, 1.0)]).leak(300.0),
            k(FOLD, -12, -0.6, &[(DRIVE, 0.8), (SPREAD, -1.0)]).threshold(-0.5),
            k(FOLD, -5, -0.2, &[(DRIVE, 0.9), (SPREAD, 0.5)]).threshold(-0.5),
            k(FOLD, 2, 0.2, &[(DRIVE, 1.0), (SPREAD, -0.5)]).threshold(-0.5),
            k(FOLD, 9, 0.6, &[(DRIVE, 0.8), (SPREAD, 1.0)]).threshold(-0.5),
            k(FOLD, 16, -0.8, &[(DRIVE, 0.9), (SPREAD, -0.8)]).threshold(-0.5),
            k(FOLD, 23, 0.8, &[(DRIVE, 1.0), (SPREAD, 0.3)]).threshold(-0.5),
            k(FOLD, 30, -0.3, &[(DRIVE, 1.0), (SPREAD, 1.0)]).leak(150.0),
            k(FOLD, 36, 0.3, &[(DRIVE, 1.0), (SPREAD, -1.0)]).leak(150.0),
            k(FOLDED, 36, 0.4, &[(DRIVE, 1.0), (SPREAD, 0.7)])
                .leak(60.0)
                .hats(),
            k(FOLDED, 36, 0.4, &[(DRIVE, 1.0), (SPREAD, 0.7)])
                .leak(800.0)
                .hats(),
            k(FOLD, 0, 0.0, &[(DRIVE, 0.6), (SPREAD, 1.0)]).leak(3000.0),
            k(FOLD, 28, 0.5, &[(DRIVE, -0.3), (SPREAD, 0.4)]),
            k(FOLD, 40, -0.5, &[(DRIVE, 1.0)]).leak(80.0),
            k(FOLD, -36, 0.6, &[(DRIVE, 1.0)]).leak(1500.0),
        ],
        ..INIT
    },
    // ---- Drones: a bus powers cells with no note, and an LFO moves the bus ------------------
    // Sparks on Bus B chirp sparsely in the dark, the bus jumping at random six times a second.
    Kit {
        name: "Insect Swarm",
        category: Drone,
        bus: [0.0, 0.6],
        seed: 1709,
        lfos: &[free(3, SampleHold, 6.0)],
        routes: &[(K::Lfo3, KitTarget::BusB, 0.7)],
        keys: [
            k(KICK, 5, 0.0, &[(DAMPING, 0.4)]).leak(250.0),
            k(SNARE, 7, 0.0, &[(BODY, -0.8), (TONE, 0.625)]).leak(200.0),
            k(SPARK, 0, -0.6, &[(GRAIN, -0.7), (TONE, 0.8)])
                .bus(B)
                .threshold(-0.6)
                .supply(0.5),
            k(SPARK, 0, -0.2, &[(GRAIN, -0.5), (TONE, 1.0)])
                .bus(B)
                .threshold(-0.6)
                .supply(0.5),
            k(SPARK, 0, 0.2, &[(GRAIN, -0.3), (TONE, 1.0)])
                .bus(B)
                .threshold(-0.6)
                .supply(0.5),
            k(MARACA, 0, 0.6, &[(GRAIN, -0.6), (TONE, 1.0)])
                .bus(B)
                .threshold(-0.6)
                .supply(0.5),
            k(SPARK, 0, -0.8, &[(GRAIN, -0.2), (TONE, 1.0)])
                .bus(B)
                .threshold(-0.6)
                .supply(0.5),
            k(SPARK, 0, 0.8, &[(GRAIN, -0.4), (TONE, 1.0)])
                .bus(B)
                .threshold(-0.6)
                .supply(0.5),
            k(RIM, 12, -0.3, &[(TONE, 1.0)]).leak(100.0),
            k(CLAP, 0, 0.3, &[(SPACING, 1.0), (GRAIN, -1.0)]),
            k(CLOSED, 0, 0.4, &[(GRAIN, -0.9), (TONE, 1.0)])
                .leak(100.0)
                .hats(),
            k(OPEN, 0, 0.4, &[(GRAIN, -0.9), (TONE, 1.0)])
                .leak(1500.0)
                .hats(),
            k(TAMB, 0, 0.0, &[(DAMPING, -0.5), (SPREAD, 0.8)]).leak(2000.0),
            k(FOLD, 24, 0.5, &[(DRIVE, 0.3)]),
            k(SPARK, 0, -0.5, &[(TONE, 1.0)]).leak(60.0),
            k(DUST, 0, 0.6, &[(GRAIN, -1.0), (TONE, 1.0), (SPREAD, 0.7)])
                .bus(B)
                .supply(0.5),
        ],
        ..INIT
    },
    // Rings on Bus A play on their own, their pitches stepping at random on the sixteenths and eighths.
    Kit {
        name: "Pattern Engine",
        category: Drone,
        bus: [0.5, 0.0],
        seed: 1823,
        lfos: &[
            synced(1, SampleHold, Division::Sixteenth),
            synced(2, SampleHold, Division::Eighth),
        ],
        keys: [
            k(KICK, 0, 0.0, &[]),
            k(SNARE, 0, 0.0, &[(BODY, 0.4)]),
            k(TOM, 0, -0.6, &[(DAMPING, 0.2)])
                .bus(A)
                .threshold(-0.4)
                .supply(0.5)
                .routes(&[(Lfo1, CellTarget::Pitch, 1.0)]),
            k(TOM, 7, -0.2, &[(DAMPING, 0.2)])
                .bus(A)
                .threshold(-0.4)
                .supply(0.5)
                .routes(&[(Lfo1, CellTarget::Pitch, -1.0)]),
            k(CONGA, 2, 0.2, &[(DAMPING, 0.2)])
                .bus(A)
                .threshold(-0.4)
                .supply(0.5)
                .routes(&[(Lfo2, CellTarget::Pitch, 1.0)]),
            k(CONGA, 9, 0.6, &[(DAMPING, 0.2)])
                .bus(A)
                .threshold(-0.4)
                .supply(0.5)
                .routes(&[(Lfo2, CellTarget::Pitch, -1.0)]),
            k(BONGO, -14, -0.8, &[]),
            k(FOLD, 10, 0.8, &[]),
            k(SPARK, 0, -0.3, &[]),
            k(CLAP, 0, 0.3, &[]),
            k(CLOSED, 0, 0.4, &[(TONE, 0.667)]).leak(80.0).hats(),
            k(OPEN, 0, 0.4, &[(TONE, 0.667)]).leak(800.0).hats(),
            k(LOOP, 24, 0.0, &[(SPREAD, 0.5)]).leak(2000.0),
            k(FOLD, 17, 0.5, &[]),
            k(SPARK, 0, -0.5, &[(TONE, 1.0)]).leak(80.0),
            k(DUST, 0, 0.6, &[(GRAIN, -0.4)]).leak(200.0),
        ],
        ..INIT
    },
    // Both buses swell and ebb over bars in opposition: low rings breathe in as the noise breathes out.
    Kit {
        name: "Breathing Machine",
        category: Drone,
        bus: [0.6, 0.6],
        lfos: &[
            synced(1, Sine, Division::TwoBars),
            synced(2, Triangle, Division::Whole),
        ],
        routes: &[
            (K::Lfo1, KitTarget::BusA, 1.0),
            (K::Lfo2, KitTarget::BusB, -1.0),
        ],
        keys: [
            k(KICK, -5, 0.0, &[(DAMPING, -0.3)]),
            k(SNARE, 0, 0.0, &[(BODY, 0.5)]),
            k(TOM, -17, -0.5, &[(DAMPING, -0.3)])
                .bus(A)
                .threshold(-0.4)
                .supply(0.4),
            k(TOM, -12, 0.0, &[(DAMPING, -0.3)])
                .bus(A)
                .threshold(-0.4)
                .supply(0.4),
            k(TOM, -5, 0.5, &[(DAMPING, -0.3)])
                .bus(A)
                .threshold(-0.4)
                .supply(0.4),
            k(FOLD, -12, -0.8, &[(DRIVE, -0.5)]).bus(A).supply(0.4),
            k(FOLD, -5, 0.8, &[(DRIVE, -0.5)]).bus(A).supply(0.4),
            k(BONGO, 0, 0.2, &[(DAMPING, 0.3)]),
            k(SPARK, 0, -0.3, &[]),
            k(CLAP, 0, 0.3, &[]),
            k(CLOSED, 0, 0.4, &[(TONE, -0.2), (SPREAD, 0.4)])
                .bus(B)
                .supply(0.4)
                .leak(300.0)
                .hats(),
            k(OPEN, 0, -0.4, &[(TONE, 0.5), (SPREAD, 0.4)])
                .bus(B)
                .supply(0.4)
                .leak(1500.0)
                .hats(),
            k(CRASH, 7, 0.0, &[(SPREAD, 0.6), (DAMPING, -0.3)])
                .bus(B)
                .threshold(-0.5)
                .supply(0.4),
            k(FOLD, 12, 0.5, &[]),
            k(SPARK, 0, -0.5, &[]),
            k(BRUSH, 0, 0.6, &[(TONE, 1.0), (SPREAD, 0.6)])
                .bus(B)
                .supply(0.4),
        ],
        ..INIT
    },
    // Pitched, whistling noise on Bus B, its level shivering eight times a second: a summer night.
    Kit {
        name: "Cicada Choir",
        category: Drone,
        bus: [0.0, 0.5],
        seed: 1931,
        lfos: &[free(3, SampleHold, 8.0)],
        routes: &[(K::Lfo3, KitTarget::BusB, 0.8)],
        keys: [
            k(KICK, 0, 0.0, &[(DAMPING, 0.2)]),
            k(SNARE, 0, 0.0, &[(BODY, -0.4)]),
            k(DUST, 0, -0.6, &[(SPREAD, 1.0), (TONE, 1.0)])
                .bus(B)
                .supply(0.5),
            k(DUST, 0, -0.2, &[(SPREAD, 1.0), (TONE, 1.0)])
                .bus(B)
                .supply(0.5),
            k(GUIRO, 0, 0.2, &[(SPREAD, 1.0), (TONE, 1.0)])
                .bus(B)
                .supply(0.5),
            k(GUIRO, 0, 0.6, &[(SPREAD, 1.0), (TONE, 1.0)])
                .bus(B)
                .supply(0.5),
            k(DUST, 0, -0.8, &[(SPREAD, 0.8), (TONE, 1.0), (GRAIN, 0.5)])
                .bus(B)
                .supply(0.5),
            k(DUST, 0, 0.8, &[(SPREAD, 0.8), (TONE, 0.833), (GRAIN, 0.5)])
                .bus(B)
                .supply(0.5),
            k(MARACA, 0, -0.3, &[(GRAIN, 0.7), (TONE, 1.0)]).leak(400.0),
            k(CLAP, 0, 0.3, &[(TONE, 0.6)]),
            k(CLOSED, 0, 0.4, &[(TONE, 1.0)]).leak(80.0).hats(),
            k(OPEN, 0, 0.4, &[(TONE, 1.0)]).leak(1000.0).hats(),
            k(CYMBAL, 7, 0.0, &[(SPREAD, 0.5)]).leak(2000.0),
            k(FOLD, 12, 0.5, &[]),
            k(SPARK, 0, -0.5, &[(GRAIN, 0.7), (TONE, 1.0)]).leak(300.0),
            k(MARACA, 0, 0.6, &[(GRAIN, -0.5)]).leak(300.0),
        ],
        ..INIT
    },
    // Sub drones on Bus A drifting in pitch over minutes, under long, spread, bright loops.
    Kit {
        name: "Deep Space",
        category: Drone,
        bus: [0.4, 0.0],
        lfos: &[free(1, Sine, 0.05)],
        keys: [
            k(KICK, -7, 0.0, &[(DAMPING, -0.6)]).leak(4000.0),
            k(SNARE, -5, 0.0, &[(BODY, 0.6), (DAMPING, -0.5)]).leak(2000.0),
            k(TOM, -36, -0.4, &[(DAMPING, -0.5)])
                .bus(A)
                .threshold(-0.5)
                .supply(0.5)
                .routes(&[(Lfo1, CellTarget::Pitch, 0.2)]),
            k(TOM, -29, 0.4, &[(DAMPING, -0.5)])
                .bus(A)
                .threshold(-0.5)
                .supply(0.5)
                .routes(&[(Lfo1, CellTarget::Pitch, -0.2)]),
            k(DUST, -24, 0.0, &[(TONE, -1.0), (SPREAD, 1.0)])
                .bus(A)
                .supply(0.5),
            k(RIDE, 9, -0.7, &[(SPREAD, 1.0), (DAMPING, -0.9)]).leak(12000.0),
            k(RIDE, 16, 0.7, &[(SPREAD, 1.0), (DAMPING, -0.9)]).leak(12000.0),
            k(RIDE, 21, 0.2, &[(SPREAD, 1.0), (DAMPING, -0.9)]).leak(12000.0),
            k(SPARK, 0, -0.3, &[(TONE, 1.0), (GRAIN, -0.8)]).leak(2000.0),
            k(CLAP, 0, 0.3, &[(SPACING, 0.8), (TONE, 0.4)]),
            k(CLOSED, 0, 0.4, &[(TONE, 1.0), (SPREAD, 0.8)])
                .leak(200.0)
                .hats(),
            k(OPEN, 0, 0.4, &[(TONE, 1.0), (SPREAD, 0.8)])
                .leak(6000.0)
                .hats(),
            k(CYMBAL, 31, 0.0, &[(SPREAD, 0.8), (DAMPING, -1.0)]).leak(25000.0),
            k(BELL, -3, 0.5, &[(DAMPING, -1.0)]).leak(10000.0),
            k(SPARK, 0, -0.5, &[(TONE, 1.0)]).leak(100.0),
            k(DUST, 0, 0.6, &[(TONE, 0.833), (SPREAD, 1.0)]).leak(3000.0),
        ],
        ..INIT
    },
    // Folds driven to noise on Bus A and noise swept slowly through its band: a machine idling.
    Kit {
        name: "Hiss Engine",
        category: Drone,
        temper: 0.3,
        bus: [0.6, 0.0],
        seed: 2039,
        lfos: &[free(1, Triangle, 0.15), free(2, Sine, 0.37)],
        keys: [
            k(KICK, -3, 0.0, &[(DRIVE, 0.4)]),
            k(SNARE, 0, 0.0, &[(BODY, -0.6)]),
            k(BRUSH, 0, -0.6, &[(SPREAD, -0.3)])
                .bus(A)
                .supply(0.4)
                .routes(&[(Lfo1, to(TONE), 1.0)]),
            k(DUST, 0, 0.6, &[(SPREAD, -0.3)])
                .bus(A)
                .supply(0.4)
                .routes(&[(Lfo2, to(TONE), -1.0)]),
            k(FOLDED, -12, -0.2, &[(DRIVE, 1.0)]).bus(A).supply(0.4),
            k(FOLD, -5, 0.2, &[(DRIVE, 1.0)]).bus(A).supply(0.4),
            k(TOM, -5, -0.8, &[]),
            k(CONGA, -8, 0.8, &[]),
            k(MARACA, 0, -0.3, &[(GRAIN, 0.4)]),
            k(CLAP, 0, 0.3, &[(GRAIN, 0.6)]),
            k(CLOSED, 0, 0.4, &[(GRAIN, 0.6)]).leak(80.0).hats(),
            k(OPEN, 0, 0.4, &[(GRAIN, 0.6)]).leak(900.0).hats(),
            k(CRASH, 7, 0.0, &[(SPREAD, 0.6)]).leak(2000.0),
            k(FOLD, 12, 0.5, &[(DRIVE, 0.6)]),
            k(SPARK, 0, -0.5, &[]),
            k(DUST, 0, 0.6, &[(SPREAD, 0.6)])
                .bus(A)
                .supply(0.4)
                .routes(&[(Lfo1, to(TONE), -0.8)]),
        ],
        ..INIT
    },
    // Wide noise on Bus A, each band sweeping at its own slow rate: air through a long pipe.
    Kit {
        name: "Wind Tunnel",
        category: Drone,
        bus: [0.4, 0.0],
        lfos: &[
            free(1, Sine, 0.09),
            free(2, Triangle, 0.21),
            free(3, Sine, 0.33),
        ],
        keys: [
            k(KICK, 0, 0.0, &[(DAMPING, -0.2)]),
            k(SNARE, 0, 0.0, &[(BODY, -0.5), (TONE, -0.3)]),
            k(BRUSH, 0, -0.6, &[(SPREAD, -0.6), (TONE, -0.6)])
                .bus(A)
                .supply(0.4)
                .routes(&[(Lfo1, to(TONE), 0.8)]),
            k(DUST, 0, -0.2, &[(SPREAD, -0.6), (TONE, -0.2)])
                .bus(A)
                .supply(0.4)
                .routes(&[(Lfo2, to(TONE), 0.8)]),
            k(BRUSH, 0, 0.2, &[(SPREAD, -0.6), (TONE, 0.333)])
                .bus(A)
                .supply(0.4)
                .routes(&[(Lfo3, to(TONE), 0.8)]),
            k(DUST, 0, 0.6, &[(SPREAD, 0.2), (TONE, 0.833)])
                .bus(A)
                .supply(0.4)
                .routes(&[(Lfo1, to(TONE), -0.8)]),
            k(TOM, -7, -0.8, &[(DAMPING, -0.3)]),
            k(TOM, 0, 0.8, &[(DAMPING, -0.3)]),
            k(RIM, 0, -0.3, &[(TONE, -0.3)]),
            k(CLAP, 0, 0.3, &[(TONE, -0.3)]),
            k(CLOSED, 0, 0.4, &[(TONE, 0.5)]).leak(100.0).hats(),
            k(OPEN, 0, 0.4, &[(TONE, 0.5)]).leak(1500.0).hats(),
            k(CRASH, 2, 0.0, &[(SPREAD, 0.7), (DAMPING, -0.3)]).leak(3000.0),
            k(COWBELL, -5, 0.5, &[]),
            k(SPARK, 0, -0.5, &[]),
            k(DUST, 0, 0.6, &[(SPREAD, -1.0), (TONE, -0.8)])
                .bus(A)
                .supply(0.4)
                .routes(&[(Lfo2, to(TONE), -0.8)]),
        ],
        ..INIT
    },
    // Pure, locked low tones on every key, a chord of folds on Bus A swaying slowly beneath.
    Kit {
        name: "Magnet Hum",
        category: Drone,
        bus: [0.5, 0.0],
        lfos: &[free(1, Sine, 0.2)],
        routes: &[(K::Lfo1, KitTarget::BusA, 0.6)],
        keys: [
            k(KICK, -12, 0.0, &[(DAMPING, -0.6), (DROP, -1.0)]).leak(4000.0),
            k(
                SNARE,
                -12,
                0.0,
                &[(BODY, 1.0), (TONE, -1.0), (DAMPING, -0.6)],
            )
            .leak(2500.0),
            k(FOLD, -24, -0.6, &[(DRIVE, -1.0)])
                .bus(A)
                .threshold(-0.6)
                .supply(0.4),
            k(FOLD, -17, -0.2, &[(DRIVE, -1.0)])
                .bus(A)
                .threshold(-0.6)
                .supply(0.4),
            k(FOLD, -12, 0.2, &[(DRIVE, -1.0)])
                .bus(A)
                .threshold(-0.6)
                .supply(0.4),
            k(FOLD, -8, 0.6, &[(DRIVE, -1.0), (SPREAD, 0.3)])
                .bus(A)
                .threshold(-0.6)
                .supply(0.4),
            k(TOM, -29, -0.8, &[(DAMPING, -0.8), (DROP, -1.0)]).leak(6000.0),
            k(TOM, -22, 0.8, &[(DAMPING, -0.8), (DROP, -1.0)]).leak(6000.0),
            k(MARACA, 0, -0.3, &[(GRAIN, 1.0), (TONE, -1.0)]).threshold(-0.6),
            k(CLAP, 0, 0.3, &[(TONE, -1.0), (GRAIN, 0.8)]),
            k(CLOSED, 0, 0.4, &[(TONE, -1.0), (SPREAD, 1.0)])
                .leak(200.0)
                .hats(),
            k(OPEN, 0, 0.4, &[(TONE, -1.0), (SPREAD, 1.0)])
                .leak(2000.0)
                .hats(),
            k(BELL, -39, 0.0, &[(SPREAD, -1.0), (DAMPING, -0.6)]).leak(6000.0),
            k(COWBELL, -24, 0.5, &[(DRIVE, -1.0)]),
            k(SPARK, 0, -0.5, &[(GRAIN, 1.0), (TONE, -0.6)]),
            k(BRUSH, -12, 0.6, &[(TONE, -0.8), (SPREAD, 1.0)]).leak(1500.0),
        ],
        ..INIT
    },
    // Buzzing sparks on Bus A that flicker on the quarters like a failing tube.
    Kit {
        name: "Neon Buzz",
        category: Drone,
        bus: [0.4, 0.0],
        lfos: &[synced(1, Square, Division::Quarter)],
        routes: &[(K::Lfo1, KitTarget::BusA, 1.0)],
        keys: [
            k(KICK, 0, 0.0, &[(DAMPING, 0.2)]),
            k(SNARE, 0, 0.0, &[(TONE, 0.5)]),
            k(SPARK, 0, -0.6, &[(GRAIN, 1.0), (TONE, -0.4)])
                .bus(A)
                .threshold(-0.7)
                .supply(0.5),
            k(MARACA, 0, 0.0, &[(GRAIN, 1.0), (TONE, 0.4)])
                .bus(A)
                .threshold(-0.7)
                .supply(0.5),
            k(SPARK, 0, 0.6, &[(GRAIN, 1.0), (TONE, 1.0)])
                .bus(A)
                .threshold(-0.7)
                .supply(0.5),
            k(FOLDED, 0, -0.8, &[(DRIVE, 0.6)]),
            k(FOLD, 19, 0.8, &[(DRIVE, 0.6)]),
            k(COWBELL, -3, 0.2, &[]),
            k(SPARK, 0, -0.3, &[(GRAIN, 0.6)]),
            k(CLAP, 0, 0.3, &[(GRAIN, 0.6)]),
            k(CLOSED, 0, 0.4, &[(TONE, 1.0)]).leak(80.0).hats(),
            k(OPEN, 0, 0.4, &[(TONE, 1.0)]).leak(800.0).hats(),
            k(CYMBAL, 12, 0.0, &[(SPREAD, 0.6)]).leak(1600.0),
            k(FOLD, 24, 0.5, &[(DRIVE, 0.6)]),
            k(SPARK, 0, -0.5, &[(GRAIN, 1.0)]),
            k(GUIRO, 0, 0.6, &[(SPREAD, 0.8), (TONE, 1.0)])
                .bus(A)
                .supply(0.5),
        ],
        ..INIT
    },
    // Low noise on Bus A rolling in waves over twelve seconds, long low rings calling under it.
    Kit {
        name: "Ocean Floor",
        category: Drone,
        bus: [0.5, 0.0],
        lfos: &[free(1, Sine, 0.08)],
        routes: &[(K::Lfo1, KitTarget::BusA, 1.0)],
        keys: [
            k(KICK, -9, 0.0, &[(DAMPING, -0.5)]).leak(3000.0),
            k(SNARE, -7, 0.0, &[(TONE, -0.7), (BODY, -0.3)]),
            k(BRUSH, -12, -0.6, &[(TONE, -0.8), (GRAIN, 0.5)])
                .bus(A)
                .supply(0.4),
            k(
                BRUSH,
                -12,
                0.6,
                &[(TONE, -0.6), (GRAIN, 0.5), (SPREAD, 0.3)],
            )
            .bus(A)
            .supply(0.4),
            k(TOM, -24, -0.2, &[(DAMPING, -0.8), (DROP, 0.4)]).leak(6000.0),
            k(TOM, -19, 0.2, &[(DAMPING, -0.8), (DROP, 0.4)]).leak(6000.0),
            k(CRASH, -29, -0.8, &[(SPREAD, 0.6), (DAMPING, 0.3)]).leak(5000.0),
            k(BELL, -32, 0.8, &[(SPREAD, 0.6), (DAMPING, 0.3)]).leak(5000.0),
            k(SPARK, 0, -0.3, &[(TONE, -0.8)]),
            k(CLAP, 0, 0.3, &[(TONE, -0.6), (SPACING, 0.6)]),
            k(CLOSED, 0, 0.4, &[(TONE, -0.4)]).leak(150.0).hats(),
            k(OPEN, 0, 0.4, &[(TONE, -0.4)]).leak(3000.0).hats(),
            k(CYMBAL, -24, 0.0, &[(SPREAD, 1.0), (DAMPING, -0.5)]).leak(12000.0),
            k(RING, -14, 0.5, &[(DAMPING, -0.9)]).leak(8000.0),
            k(SPARK, 0, -0.5, &[(TONE, -0.5)]),
            k(DUST, -12, 0.6, &[(TONE, -0.9), (SPREAD, 0.8)])
                .bus(A)
                .supply(0.4),
        ],
        ..INIT
    },
    // ---- Moving and wild: modulation everywhere, Temper high ----------------------------------
    // Every tone lands an octave either side of where it is set, at random, hit by hit.
    Kit {
        name: "Pitch Rain",
        seed: 2141,
        keys: [
            k(KICK, 0, 0.0, &[(DROP, 0.5)]).routes(&[(Random, CellTarget::Pitch, 0.4)]),
            k(SNARE, 0, 0.0, &[(BODY, 0.6)]).routes(&[(Random, to(TONE), 0.8)]),
            k(TOM, 0, -0.6, &[(DAMPING, -0.4)]).routes(&[(Random, CellTarget::Pitch, 1.0)]),
            k(TOM, 5, -0.2, &[(DAMPING, -0.4)]).routes(&[(Random, CellTarget::Pitch, 1.0)]),
            k(TOM, 12, 0.2, &[(DAMPING, -0.4)]).routes(&[(Random, CellTarget::Pitch, 1.0)]),
            k(COWBELL, -21, 0.6, &[(SPREAD, 0.3)]).routes(&[(Random, CellTarget::Pitch, 1.0)]),
            k(COWBELL, -14, -0.8, &[(SPREAD, -0.3)]).routes(&[(Random, CellTarget::Pitch, 1.0)]),
            k(BONGO, 5, 0.8, &[(DAMPING, -0.4)]).routes(&[(Random, CellTarget::Pitch, 1.0)]),
            k(RIM, 0, -0.3, &[]).routes(&[(Random, to(TONE), 1.0)]),
            k(CLAP, 0, 0.3, &[]).routes(&[(Random, to(SPACING), 1.0)]),
            k(CLOSED, 0, 0.4, &[(SPREAD, 0.6)])
                .leak(100.0)
                .hats()
                .routes(&[(Random, to(TONE), 1.0)]),
            k(OPEN, 0, 0.4, &[(SPREAD, 0.6)])
                .leak(1000.0)
                .hats()
                .routes(&[(Random, to(TONE), 1.0)]),
            k(RIDE, 9, 0.0, &[(SPREAD, 0.6), (DAMPING, -0.4)])
                .leak(2000.0)
                .routes(&[(Random, CellTarget::Pitch, 1.0)]),
            k(CLAVE, -24, 0.5, &[(DAMPING, -0.2)]).routes(&[(Random, CellTarget::Pitch, 1.0)]),
            k(MARACA, 0, -0.5, &[]).routes(&[(Random, to(GRAIN), 1.0)]),
            k(TAMB, 0, 0.6, &[(SPREAD, 1.0)]).routes(&[(Random, to(TONE), 1.0)]),
        ],
        ..INIT
    },
    // Pitch shaken at twenty, thirteen and seventeen cycles a second: tones turned to growls.
    Kit {
        name: "FM Growl",
        lfos: &[
            free(1, Sine, 20.0),
            free(2, Triangle, 13.0),
            free(3, Square, 17.0),
        ],
        keys: [
            k(KICK, -5, 0.0, &[(DAMPING, -0.3)]).routes(&[(Lfo1, CellTarget::Pitch, 0.8)]),
            k(SNARE, 0, 0.0, &[(BODY, 0.6)]).routes(&[(Lfo2, CellTarget::Pitch, 0.6)]),
            k(TOM, -12, -0.6, &[])
                .threshold(-0.3)
                .routes(&[(Lfo1, CellTarget::Pitch, 0.8)]),
            k(TOM, -5, -0.2, &[])
                .threshold(-0.3)
                .routes(&[(Lfo2, CellTarget::Pitch, 0.8)]),
            k(TOM, 2, 0.2, &[])
                .threshold(-0.3)
                .routes(&[(Lfo1, CellTarget::Pitch, -0.8)]),
            k(CONGA, -17, 0.6, &[]).routes(&[(Lfo2, CellTarget::Pitch, 0.5)]),
            k(CONGA, -10, -0.8, &[]).routes(&[(Lfo1, CellTarget::Pitch, 0.5)]),
            k(BONGO, -7, 0.8, &[(DAMPING, -0.3)]).routes(&[(Lfo2, CellTarget::Pitch, 0.5)]),
            k(RIM, 0, -0.3, &[]).routes(&[(Lfo3, to(TONE), 1.0)]),
            k(CLAP, 0, 0.3, &[]).routes(&[(Lfo3, to(TONE), 1.0)]),
            k(CLOSED, 0, 0.4, &[(SPREAD, 0.8)])
                .leak(150.0)
                .hats()
                .routes(&[(Lfo3, to(TONE), 1.0)]),
            k(OPEN, 0, 0.4, &[(SPREAD, 0.8)])
                .leak(1500.0)
                .hats()
                .routes(&[(Lfo3, to(TONE), 1.0)]),
            k(BELL, -3, 0.0, &[(SPREAD, 0.5)]).leak(2500.0).routes(&[(
                Lfo1,
                CellTarget::Pitch,
                0.4,
            )]),
            k(COWBELL, -12, 0.5, &[]).routes(&[(Lfo2, CellTarget::Pitch, 0.8)]),
            k(SPARK, 0, -0.5, &[]).routes(&[(Lfo3, to(GRAIN), 1.0)]),
            k(DUST, 0, 0.6, &[(SPREAD, 1.0)]).routes(&[(Lfo1, to(TONE), 1.0)]),
        ],
        ..INIT
    },
    // Low rings tipped into sustaining, their pitch wobbling on the eighths: a kit that wubs.
    Kit {
        name: "Wobble Bass",
        lfos: &[
            synced(2, Sine, Division::Eighth),
            synced(3, Triangle, Division::Quarter),
        ],
        keys: [
            k(KICK, -12, 0.0, &[(DAMPING, -0.3)])
                .threshold(-0.5)
                .supply(0.8)
                .routes(&[(Lfo2, CellTarget::Pitch, 0.6)]),
            k(SNARE, -5, 0.0, &[(BODY, 0.8)]).routes(&[(Lfo3, to(TONE), 0.6)]),
            k(TOM, -12, -0.6, &[])
                .threshold(-0.5)
                .supply(0.8)
                .routes(&[(Lfo2, CellTarget::Pitch, 0.6)]),
            k(TOM, -7, -0.2, &[]).threshold(-0.5).supply(0.8).routes(&[(
                Lfo2,
                CellTarget::Pitch,
                -0.6,
            )]),
            k(TOM, -5, 0.2, &[]).threshold(-0.5).supply(0.8).routes(&[(
                Lfo3,
                CellTarget::Pitch,
                0.6,
            )]),
            k(FOLD, -12, 0.6, &[(DRIVE, 0.4)])
                .supply(0.8)
                .routes(&[(Lfo2, to(DRIVE), 0.8)]),
            k(FOLD, -5, -0.8, &[(DRIVE, 0.4)])
                .supply(0.8)
                .routes(&[(Lfo3, to(DRIVE), 0.8)]),
            k(CONGA, -10, 0.8, &[(DAMPING, 0.4)])
                .threshold(-0.4)
                .routes(&[(Lfo2, to(DAMPING), 0.8)]),
            k(SPARK, 0, -0.3, &[]),
            k(CLAP, 0, 0.3, &[]),
            k(CLOSED, 0, 0.4, &[]).leak(85.0).hats(),
            k(OPEN, 0, 0.4, &[])
                .leak(775.0)
                .hats()
                .routes(&[(Lfo2, to(TONE), 0.6)]),
            k(LOOP, 0, 0.0, &[(SPREAD, 0.5)]).leak(1600.0),
            k(BELL, -15, 0.5, &[(DRIVE, 0.4)]).routes(&[(Lfo3, to(DRIVE), 0.8)]),
            k(SPARK, 0, -0.5, &[]),
            k(DUST, 0, 0.6, &[]).leak(180.0),
        ],
        ..INIT
    },
    // Bloops and rising chirps, and sparse popping noise on Bus B: a bath of bubbles.
    Kit {
        name: "Bubble Bath",
        bus: [0.0, 0.3],
        seed: 2251,
        lfos: &[synced(1, RampUp, Division::Sixteenth)],
        keys: [
            k(KICK, 12, 0.0, &[(DROP, 1.0), (DAMPING, 0.6)]).leak(200.0),
            k(SNARE, 12, 0.0, &[(BODY, 1.0), (DROP, 1.0), (DAMPING, 0.4)]).leak(200.0),
            k(TOM, 24, -0.6, &[(DAMPING, 0.5), (DROP, 0.6)]).leak(150.0),
            k(TOM, 28, -0.2, &[(DAMPING, 0.5), (DROP, 0.6)]).leak(150.0),
            k(TOM, 33, 0.2, &[(DAMPING, 0.5), (DROP, 0.6)]).leak(150.0),
            k(FOLD, 24, 0.6, &[(DRIVE, -0.6)]).leak(150.0).routes(&[(
                Lfo1,
                CellTarget::Pitch,
                0.8,
            )]),
            k(FOLD, 29, -0.8, &[(DRIVE, -0.6)]).leak(150.0).routes(&[(
                Lfo1,
                CellTarget::Pitch,
                0.8,
            )]),
            k(CLAVE, -18, 0.8, &[(DAMPING, 0.5), (DROP, 0.8)]).leak(120.0),
            k(MARACA, 0, -0.3, &[(TONE, -0.4), (GRAIN, -0.6)]).leak(300.0),
            k(CLAP, 0, 0.3, &[(SPACING, 0.8), (TONE, 0.2)]),
            k(CLOSED, 0, 0.4, &[(SPREAD, 1.0), (TONE, 0.5), (GRAIN, -1.0)])
                .leak(200.0)
                .hats(),
            k(OPEN, 0, 0.4, &[(SPREAD, 1.0), (TONE, 0.5), (GRAIN, -1.0)])
                .leak(2000.0)
                .hats(),
            k(TAMB, 0, 0.0, &[(DAMPING, 0.4), (SPREAD, -1.0)]).leak(300.0),
            k(RIM, 14, 0.5, &[(DAMPING, 0.6), (DROP, 1.0)]).leak(100.0),
            k(SPARK, 0, -0.5, &[(GRAIN, -0.8)]).leak(500.0),
            k(DUST, 0, 0.6, &[(SPREAD, 1.0), (TONE, 0.167), (GRAIN, -1.0)])
                .bus(B)
                .supply(0.5),
        ],
        ..INIT
    },
    // Falling rings and metal, each hit thrown to a random place in the stereo field.
    Kit {
        name: "Ricochet",
        seed: 2357,
        keys: [
            k(KICK, 0, 0.0, &[(DROP, 0.6)]).routes(&[(Random, CellTarget::Pan, 0.4)]),
            k(SNARE, 5, 0.0, &[(DROP, 0.6)]).routes(&[(Random, CellTarget::Pan, 0.6)]),
            k(RING, 12, 0.0, &[(DROP, 1.0), (DAMPING, -0.3)]).routes(&[(
                Random,
                CellTarget::Pan,
                1.0,
            )]),
            k(RING, 17, 0.0, &[(DROP, 1.0), (DAMPING, -0.3)]).routes(&[(
                Random,
                CellTarget::Pan,
                1.0,
            )]),
            k(RING, 22, 0.0, &[(DROP, 1.0), (DAMPING, -0.3)]).routes(&[(
                Random,
                CellTarget::Pan,
                1.0,
            )]),
            k(COWBELL, -4, 0.0, &[(SPREAD, 0.8), (DAMPING, 0.2)]).routes(&[(
                Random,
                CellTarget::Pan,
                1.0,
            )]),
            k(COWBELL, 3, 0.0, &[(SPREAD, 0.8), (DAMPING, 0.2)]).routes(&[(
                Random,
                CellTarget::Pan,
                1.0,
            )]),
            k(BONGO, 10, 0.0, &[(DROP, 1.0), (DAMPING, 0.2)]).routes(&[(
                Random,
                CellTarget::Pan,
                1.0,
            )]),
            k(RIM, 12, 0.0, &[(TONE, 1.0)]).routes(&[(Random, CellTarget::Pan, 1.0)]),
            k(CLAP, 0, 0.0, &[(SPACING, -0.4)]).routes(&[(Random, CellTarget::Pan, 1.0)]),
            k(CLOSED, 0, 0.0, &[(TONE, 1.0)])
                .leak(100.0)
                .hats()
                .routes(&[(Random, CellTarget::Pan, 1.0)]),
            k(OPEN, 0, 0.0, &[(TONE, 1.0)])
                .leak(1000.0)
                .hats()
                .routes(&[(Random, CellTarget::Pan, 1.0)]),
            k(RIDE, 16, 0.0, &[(SPREAD, 1.0), (DAMPING, -0.4)])
                .leak(3000.0)
                .routes(&[(Random, CellTarget::Pan, 1.0)]),
            k(CLAVE, -18, 0.0, &[(DROP, 1.0), (DAMPING, 0.4)]).routes(&[(
                Random,
                CellTarget::Pan,
                1.0,
            )]),
            k(SPARK, 0, 0.0, &[(TONE, 1.0)]).routes(&[(Random, CellTarget::Pan, 1.0)]),
            k(DUST, 0, 0.0, &[(SPREAD, 0.8)]).routes(&[(Random, CellTarget::Pan, 1.0)]),
        ],
        ..INIT
    },
    // Pitch and tone stepping at random on the thirty-seconds: every hit stutters through registers.
    Kit {
        name: "Glitch Kit",
        category: Fx,
        seed: 2459,
        lfos: &[
            synced(1, SampleHold, Division::ThirtySecond),
            synced(2, SampleHold, Division::SixteenthTriplet),
        ],
        keys: [
            k(KICK, 0, 0.0, &[(DAMPING, 0.2)]).routes(&[(Lfo1, CellTarget::Pitch, 0.6)]),
            k(SNARE, 0, 0.0, &[(BODY, 0.2)]).routes(&[(Lfo2, to(TONE), 1.0)]),
            k(TOM, 0, -0.6, &[(DAMPING, -0.3)]).routes(&[(Lfo1, CellTarget::Pitch, 1.0)]),
            k(RING, 7, -0.2, &[(DAMPING, -0.3)]).routes(&[(Lfo2, CellTarget::Pitch, 1.0)]),
            k(RING, 14, 0.2, &[(DAMPING, -0.3)]).routes(&[(Lfo1, CellTarget::Pitch, -1.0)]),
            k(FOLD, 5, 0.6, &[(DRIVE, 0.5)])
                .routes(&[(Lfo1, CellTarget::Pitch, 1.0), (Lfo2, to(DRIVE), 1.0)]),
            k(FOLDED, 12, -0.8, &[(DRIVE, 0.5)])
                .routes(&[(Lfo2, CellTarget::Pitch, 1.0), (Lfo1, to(SPREAD), 1.0)]),
            k(COWBELL, -3, 0.8, &[(SPREAD, 0.5)])
                .routes(&[(Lfo1, CellTarget::Pitch, 1.0), (Lfo2, to(DAMPING), 0.8)]),
            k(MARACA, 0, -0.3, &[]).routes(&[(Lfo1, to(TONE), 1.0), (Lfo2, to(GRAIN), 1.0)]),
            k(CLAP, 0, 0.3, &[]).routes(&[(Lfo2, to(TONE), 1.0)]),
            k(CLOSED, 0, 0.4, &[(SPREAD, 0.8)])
                .leak(100.0)
                .hats()
                .routes(&[(Lfo1, to(TONE), 1.0)]),
            k(OPEN, 0, 0.4, &[(SPREAD, 0.8)])
                .leak(1200.0)
                .hats()
                .routes(&[(Lfo2, to(TONE), 1.0)]),
            k(CYMBAL, 12, 0.0, &[(SPREAD, 0.8)]).leak(2000.0).routes(&[(
                Lfo1,
                CellTarget::Pitch,
                1.0,
            )]),
            k(BELL, -8, 0.5, &[(DRIVE, 0.6)]).routes(&[(Lfo2, CellTarget::Pitch, 1.0)]),
            k(SPARK, 0, -0.5, &[]).routes(&[(Lfo1, to(GRAIN), 1.0)]),
            k(GUIRO, 0, 0.6, &[(SPREAD, 1.0)]).routes(&[(Lfo2, to(TONE), 1.0)]),
        ],
        ..INIT
    },
    // Buzzing sparks gated by squares at three rates, so one hit taps out a code for seconds.
    Kit {
        name: "Morse Field",
        lfos: &[
            synced(1, Square, Division::Sixteenth),
            synced(2, Square, Division::EighthTriplet),
            synced(3, Square, Division::Eighth),
        ],
        keys: [
            k(KICK, 0, 0.0, &[(DAMPING, 0.3)]),
            k(SNARE, 0, 0.0, &[(BODY, 0.6)]),
            k(SPARK, 0, -0.6, &[(GRAIN, 1.0), (TONE, -0.3)])
                .threshold(-0.8)
                .hunger(0.1)
                .leak(4000.0)
                .routes(&[(Lfo1, CellTarget::Threshold, 1.0)]),
            k(SPARK, 0, -0.2, &[(GRAIN, 1.0), (TONE, 0.2)])
                .threshold(-0.8)
                .hunger(0.1)
                .leak(4000.0)
                .routes(&[(Lfo2, CellTarget::Threshold, 1.0)]),
            k(SPARK, 0, 0.2, &[(GRAIN, 1.0), (TONE, 1.0)])
                .threshold(-0.8)
                .hunger(0.1)
                .leak(4000.0)
                .routes(&[(Lfo3, CellTarget::Threshold, 1.0)]),
            k(SPARK, 0, 0.6, &[(GRAIN, 0.9), (TONE, 1.0)])
                .threshold(-0.8)
                .hunger(0.1)
                .leak(4000.0)
                .routes(&[(Lfo1, CellTarget::Threshold, -1.0)]),
            k(SPARK, 0, -0.8, &[(GRAIN, 0.9), (TONE, -0.6)])
                .threshold(-0.8)
                .hunger(0.1)
                .leak(4000.0)
                .routes(&[(Lfo2, CellTarget::Threshold, -1.0)]),
            k(SPARK, 0, 0.8, &[(GRAIN, 0.9), (TONE, 0.6)])
                .threshold(-0.8)
                .hunger(0.1)
                .leak(4000.0)
                .routes(&[(Lfo3, CellTarget::Threshold, -1.0)]),
            k(SPARK, 0, -0.3, &[(GRAIN, 0.8)]).leak(200.0),
            k(CLAP, 0, 0.3, &[(SPACING, -0.5)]),
            k(CLOSED, 0, 0.4, &[(TONE, 0.833)]).leak(80.0).hats(),
            k(OPEN, 0, 0.4, &[(TONE, 0.833)]).leak(800.0).hats(),
            k(CYMBAL, 12, 0.0, &[(SPREAD, -1.0)]).leak(1600.0),
            k(COWBELL, 0, 0.5, &[(DRIVE, -0.8)])
                .threshold(-0.6)
                .leak(3000.0)
                .routes(&[(Lfo1, CellTarget::Threshold, 1.0)]),
            k(MARACA, 0, -0.5, &[(GRAIN, 1.0), (TONE, 1.0)])
                .threshold(-0.8)
                .leak(4000.0)
                .routes(&[(Lfo3, CellTarget::Threshold, 1.0)]),
            k(GUIRO, 0, 0.6, &[(SPREAD, 1.0), (TONE, 1.0)]).leak(300.0),
        ],
        ..INIT
    },
    // A zapping kick, bursting clap and sparks on a fully coupled, nearly wild grid.
    Kit {
        name: "Lightning Rod",
        category: Fx,
        temper: 0.8,
        flow: 0.4,
        coupling: 1.0,
        seed: 2633,
        keys: [
            k(KICK, 36, 0.0, &[(DROP, 1.0), (DAMPING, 0.4)]).leak(300.0),
            k(SNARE, 12, 0.0, &[(BODY, -0.8), (TONE, 0.625), (SNAP, 1.0)]).threshold(-0.5),
            k(TOM, 24, -0.6, &[(DROP, 1.0)]).threshold(-0.3),
            k(SPARK, 0, -0.2, &[(GRAIN, 1.0)]).threshold(-0.8),
            k(TOM, 36, 0.2, &[(DROP, 1.0)]).threshold(-0.3),
            k(SPARK, 0, 0.6, &[(GRAIN, 1.0), (TONE, 1.0)]).threshold(-0.8),
            k(FOLD, 24, -0.8, &[(DRIVE, 1.0)]),
            k(SPARK, 0, 0.8, &[(GRAIN, 1.0), (TONE, -0.6)]).threshold(-0.8),
            k(MARACA, 0, -0.3, &[(GRAIN, 1.0), (TONE, 1.0)]).threshold(-0.8),
            k(CLAP, 0, 0.3, &[(GRAIN, 1.0), (SPACING, -1.0)]).threshold(-0.8),
            k(CLOSED, 0, 0.4, &[(TONE, 1.0), (GRAIN, 1.0)])
                .leak(60.0)
                .hats(),
            k(OPEN, 0, 0.4, &[(TONE, 1.0), (GRAIN, 1.0)])
                .leak(800.0)
                .hats(),
            k(LOOP, 31, 0.0, &[(SPREAD, 1.0), (DRIVE, 1.0)])
                .threshold(-0.5)
                .leak(2000.0),
            k(CLAVE, 0, 0.5, &[(DROP, 1.0), (DAMPING, 0.5)]),
            k(SPARK, 0, -0.5, &[(GRAIN, 1.0)]).threshold(-0.8),
            k(TAMB, 12, 0.6, &[(TONE, 1.0), (SPREAD, 0.8)]).leak(300.0),
        ],
        ..INIT
    },
    // Full Temper, Flow and Coupling on the smallest pool, Temper jumping each beat: it caves in.
    Kit {
        name: "Collapsing Star",
        category: Fx,
        temper: 1.0,
        flow: 1.0,
        coupling: 1.0,
        pool: 0.15,
        recharge_ms: Some(3000.0),
        seed: 2731,
        lfos: &[synced(3, SampleHold, Division::Quarter)],
        routes: &[(K::Lfo3, KitTarget::Temper, -0.5)],
        keys: [
            k(KICK, -5, 0.0, &[(DROP, 0.6)]).threshold(-0.5).strike(1.0),
            k(SNARE, 0, 0.0, &[]).threshold(-0.5).strike(1.0),
            k(TOM, -12, -0.6, &[]).threshold(-0.5).strike(1.0),
            k(TOM, -5, -0.2, &[]).threshold(-0.5).strike(1.0),
            k(TOM, 2, 0.2, &[]).threshold(-0.5).strike(1.0),
            k(CONGA, -17, 0.6, &[(DRIVE, 0.4)])
                .threshold(-0.5)
                .strike(1.0),
            k(CYMBAL, -6, -0.8, &[(SPREAD, 0.6)])
                .threshold(-0.5)
                .strike(1.0),
            k(BELL, -8, 0.8, &[(SPREAD, 0.6)])
                .threshold(-0.5)
                .strike(1.0),
            k(RIM, 0, -0.3, &[]).threshold(-0.5).strike(1.0),
            k(CLAP, 0, 0.3, &[]).threshold(-0.5).strike(1.0),
            k(CLOSED, 0, 0.4, &[]).strike(1.0).leak(100.0).hats(),
            k(OPEN, 0, 0.4, &[]).strike(1.0).leak(1500.0).hats(),
            k(CRASH, 7, 0.0, &[(SPREAD, 1.0)])
                .threshold(-0.5)
                .strike(1.0)
                .leak(4000.0),
            k(COWBELL, -3, 0.5, &[(DRIVE, 0.4)])
                .threshold(-0.5)
                .strike(1.0),
            k(CLAVE, -12, -0.5, &[]).threshold(-0.5).strike(1.0),
            k(TAMB, 0, 0.6, &[(SPREAD, 0.6)]).strike(1.0),
        ],
        ..INIT
    },
    // Everything at its most: full Temper, Flow and Coupling, driven past onset, both buses on.
    Kit {
        name: "Supernova",
        category: Fx,
        temper: 1.0,
        flow: 1.0,
        coupling: 1.0,
        bus: [0.3, 0.3],
        seed: 9001,
        lfos: &[free(1, Sine, 0.3), free(2, RampDown, 0.25)],
        routes: &[
            (K::Lfo1, KitTarget::BusA, 0.5),
            (K::Lfo2, KitTarget::BusB, 0.5),
        ],
        keys: [
            k(KICK, -5, 0.0, &[(DRIVE, 1.0), (DROP, 1.0)])
                .threshold(-0.8)
                .strike(1.0)
                .touch(1.0),
            k(SNARE, 0, 0.0, &[(DRIVE, 1.0)])
                .threshold(-0.8)
                .strike(1.0)
                .touch(1.0),
            k(TOM, -12, -0.6, &[(DRIVE, 1.0)])
                .threshold(-0.8)
                .strike(1.0)
                .bus(A),
            k(TOM, 0, -0.2, &[(DRIVE, 1.0)])
                .threshold(-0.8)
                .strike(1.0)
                .touch(1.0),
            k(BELL, -15, 0.2, &[(DRIVE, 1.0)])
                .threshold(-0.8)
                .strike(1.0)
                .bus(B),
            k(FOLDED, 0, 0.6, &[(DRIVE, 1.0)])
                .threshold(-0.8)
                .strike(1.0)
                .touch(1.0),
            k(COWBELL, -17, -0.8, &[(DRIVE, 1.0), (SPREAD, 1.0)])
                .threshold(-0.8)
                .strike(1.0)
                .bus(A),
            k(LOOP, 12, 0.8, &[(DRIVE, 1.0), (SPREAD, 1.0)])
                .threshold(-0.8)
                .strike(1.0)
                .touch(1.0),
            k(MARACA, 0, -0.3, &[(GRAIN, 1.0)])
                .threshold(-0.8)
                .strike(1.0)
                .bus(B),
            k(CLAP, 0, 0.3, &[(GRAIN, 1.0)]).threshold(-0.8).strike(1.0),
            k(CLOSED, 0, 0.4, &[(GRAIN, 1.0)])
                .strike(1.0)
                .leak(150.0)
                .hats(),
            k(OPEN, 0, 0.4, &[(GRAIN, 1.0)])
                .strike(1.0)
                .leak(3000.0)
                .hats(),
            k(LOOP, 24, 0.0, &[(DRIVE, 1.0), (SPREAD, 1.0)])
                .threshold(-0.8)
                .strike(1.0)
                .leak(8000.0),
            k(FOLD, 24, 0.5, &[(DRIVE, 1.0)])
                .threshold(-0.8)
                .strike(1.0)
                .bus(A),
            k(SPARK, 0, -0.5, &[(GRAIN, 1.0)])
                .threshold(-0.8)
                .strike(1.0),
            k(BRUSH, 0, 0.6, &[(GRAIN, 1.0), (SPREAD, 1.0)])
                .strike(1.0)
                .bus(B),
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
/// The least two kits may differ by ([`distance`]): two kits nearer than this sound alike.
const ALIKE_DB: f32 = 10.0;
/// How far under the others a noise cell or a droning one sits.
const UNDER_DB: f32 = 6.0;
/// How far every Level may come down together so a quiet cell can reach its balance under the
/// Level's ceiling: the kick then sits this far under 0 dB at most.
const MAKE_ROOM_DB: f32 = 6.0;

/// The stretches of a hit its fingerprint is taken over, in seconds from the strike: the attack,
/// the body and the tail.
const STRETCHES: [f64; 3] = [0.1, 0.5, f64::INFINITY];
/// The fingerprint's floor, under the hit's own loudest band.
const PRINT_FLOOR_DB: f32 = -60.0;
const PRINT: usize = STRETCHES.len() * 28;

/// One cell's hit, measured.
#[derive(Clone, Copy, Debug)]
struct Hit {
    /// The highest short-term power any band reaches, in dB: the pink-noise measure.
    band_db: f64,
    /// The highest short-term power over all bands, K-weighted, in dB: the loudness measure.
    loudness_db: f64,
    /// **What it sounds like, whatever its level**: each band's energy over each stretch, in dB under
    /// the loudest of them, floored at [`PRINT_FLOOR_DB`].
    print: [f32; PRINT],
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

/// **The balance**: every cell as loud as the kick by its K-weighted hit, a noise cell
/// ([`is_noise`]) or one a bus drones [`UNDER_DB`] under that. The kick is at 0 dB unless a cell would need more
/// than the Level's ceiling; then every Level comes down together, by up to [`MAKE_ROOM_DB`], so it
/// fits. To half a decibel, within the Level's range.
fn balance(params: &MxmChaosDrumsParams, hits: &[Hit; CELLS]) -> [f32; CELLS] {
    let reference = hits[0].loudness_db;
    let wanted: [f32; CELLS] = std::array::from_fn(|cell| {
        let p = &params.cells[cell];
        let under = is_noise(p.model_id()) || p.bus.value() != BusChoice::Off;
        (reference - hits[cell].loudness_db) as f32 - if under { UNDER_DB } else { 0.0 }
    });
    let loudest = wanted.iter().copied().fold(f32::MIN, f32::max);
    let room = (loudest - LEVEL_DB.1).clamp(0.0, MAKE_ROOM_DB);
    wanted.map(|db| (((db - room) * 2.0).round() / 2.0).clamp(LEVEL_DB.0, LEVEL_DB.1))
}

/// The brief's noise cells, which sit under the others: the hats, the maraca and raw noise.
fn is_noise(model: ModelId) -> bool {
    dsp::available(model).is_some_and(|spec| {
        matches!(
            spec.kind,
            dsp::Kind::ClosedHat
                | dsp::Kind::OpenHat
                | dsp::Kind::Hat
                | dsp::Kind::Maraca
                | dsp::Kind::Dust
        )
    })
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
    let mut energies = vec![vec![0.0f64; BANDS.count()]; STRETCHES.len()];
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
        // The window's centre, in the hit's own time.
        let at = start as f64 / f64::from(RATE);
        let stretch = STRETCHES.iter().position(|end| at < *end).unwrap_or(0);
        for (energy, band) in energies[stretch].iter_mut().zip(&bands) {
            *energy += band;
        }
        for (peak, band) in peak.iter_mut().zip(bands) {
            *peak = peak.max(band);
        }
        loudest = loudest.max(weighted);
    }
    let db = |power: f64| 10.0 * power.max(1e-30).log10();
    let top = energies.iter().flatten().copied().fold(0.0, f64::max);
    let mut print = [PRINT_FLOOR_DB; PRINT];
    for (slot, energy) in print.iter_mut().zip(energies.iter().flatten()) {
        *slot = ((db(*energy) - db(top)) as f32).max(PRINT_FLOOR_DB);
    }
    Hit {
        band_db: db(peak.iter().copied().fold(0.0, f64::max)),
        loudness_db: db(loudest),
        print,
    }
}

/// How far apart two kits sound: the mean over their cells of the RMS difference, in dB, of each
/// cell's fingerprint. Key by key, since one beat plays every kit.
fn distance(a: &[Hit; CELLS], b: &[Hit; CELLS]) -> f32 {
    let cell = |x: &Hit, y: &Hit| {
        let sum: f32 = x
            .print
            .iter()
            .zip(&y.print)
            .map(|(p, q)| (p - q) * (p - q))
            .sum();
        (sum / PRINT as f32).sqrt()
    };
    a.iter().zip(b).map(|(x, y)| cell(x, y)).sum::<f32>() / CELLS as f32
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

/// **No two kits sound alike, and none like Init**: every pair of the bank, Init included, at least
/// [`ALIKE_DB`] apart by [`distance`]. Prints the closest pairs and each kit's nearest neighbour.
/// Ignored, as it measures 816 hits.
///
/// `cargo test -p mxm-chaos-drums --release --lib no_two_kits_sound_alike -- --ignored --nocapture`
#[test]
#[ignore = "measures every cell of every kit"]
fn no_two_kits_sound_alike() {
    let mut bank = vec![("Init".to_owned(), measured(&MxmChaosDrumsParams::default()))];
    for preset in FILES.iter() {
        let params = MxmChaosDrumsParams::default();
        apply(&params, preset);
        bank.push((preset.name.clone(), measured(&params)));
    }
    let mut pairs = Vec::new();
    for (i, (a, x)) in bank.iter().enumerate() {
        for (b, y) in &bank[i + 1..] {
            pairs.push((distance(x, y), a.as_str(), b.as_str()));
        }
    }
    pairs.sort_by(|p, q| p.0.total_cmp(&q.0));
    let median = pairs[pairs.len() / 2].0;
    println!("{} pairs, median {median:.1} dB; the closest:", pairs.len());
    for (d, a, b) in pairs.iter().take(12) {
        println!("  {d:5.1} dB  {a} / {b}");
    }
    assert!(
        pairs[0].0 >= ALIKE_DB,
        "{} and {} are only {:.1} dB apart",
        pairs[0].1,
        pairs[0].2,
        pairs[0].0
    );
}
