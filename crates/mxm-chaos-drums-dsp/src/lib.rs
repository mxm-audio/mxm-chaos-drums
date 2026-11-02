//! Framework-free DSP for mxm-chaos-drums: an original drum instrument that runs on charge.
//!
//! **A note supplies energy; it does not fire a hit.** Each cell holds a charge with a ceiling. A
//! note-on adds a Strike packet scaled by velocity; a held note, or the cell's bus, keeps filling it
//! from the kit's one pool (Supply); sounding spends it (Hunger) and rest drains it (Leak). A live
//! cell's output is its model's raw sample, within [-1, 1], times its charge, so **no charge, no
//! sound**, and that bound with the ceiling is the whole stability argument: there is no limiter.
//! A transition (a model change, a choke, a transport reset) never cuts the output: the jump goes
//! into one declick offset per cell, which decays to exact zero within [`DECLICK_S`] and which no
//! neighbour hears.
//!
//! A cell plays a [model](model): a network of [engines](engine) on its one charge, each changing
//! character as the charge fills and drains. The sixteen cells sit on a 4×4 torus: they pass charge
//! to their neighbours (Flow) and hear them (Coupling), both scaled by Temper, which also moves every
//! threshold toward onset and adds seeded jitter. At Temper 0 a cell is a predictable drum. A touch
//! key feeds a cell's sound back into its charge, bridges it to its neighbours and adds a skin; a
//! choke latches a cell empty until it is played again.
//!
//! **Reproducible.** Everything advances per sample, so any block size renders the same samples;
//! the events at one sample apply in a fixed order whatever the host's; the cells read each other's
//! previous sample and the pool grants every demand the same share, so their order cannot matter;
//! every random stream is seeded; and nothing that reaches a cell's state calls the platform's maths
//! library ([`fmath`]), so every platform renders the same performance ([`Kit::process_raw`]). A
//! kit at rest is a fixed point, and skipped.
//!
//! The design is the repository's `docs/briefs/mxm-chaos-drums.md`.

pub mod cell;
pub mod engine;
pub mod fmath;
pub mod kit;
pub mod model;
mod pool;
pub mod routes;
mod voice;

pub use cell::{Bus, CHARGE_FLOOR, CellParams, DECLICK_S, LEAK_MS, velocity_curve};
pub use kit::{CELLS, EDGES, Event, EventKind, Kit, KitParams, SIDE, edge, neighbours};
pub use model::{AVAILABLE_MODELS, ControlSpec, Kind, ModelId, ModelSpec, available};
pub use mxm_drum_engine::{LfoShape, OutputFrame};
pub use routes::{CONTROLS, CellTarget, KitTarget, ROUTES, Route, Source};
