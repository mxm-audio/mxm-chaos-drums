//! Framework-free DSP for mxm-chaos-drums: an original drum instrument that runs on charge.
//!
//! **A note supplies energy; it does not fire a hit.** Each cell holds a charge with a ceiling. A
//! note-on adds a Strike packet scaled by velocity; a held note keeps filling it from the kit's one
//! [pool](pool) (Supply); sounding spends it (Hunger) and rest drains it (Leak). A live cell's output
//! is its model's raw sample, within [-1, 1], times its charge, so **no charge, no sound**, and that
//! bound with the ceiling is the whole stability argument: there is no limiter.
//!
//! A cell plays a [model](model): a network of [engines](engine) on its one charge, each changing
//! character as the charge fills and drains. Cells pass charge to their neighbours (Flow) and hear
//! them (Coupling), both scaled by Temper, which also moves every threshold toward onset and adds
//! seeded jitter. At Temper 0 a cell is a predictable drum.
//!
//! **Reproducible.** Everything advances per sample, so any block size renders the same samples;
//! the cells read each other's previous sample, so their order cannot matter; every random stream is
//! seeded; and nothing that reaches a cell's state calls the platform's maths library
//! ([`fmath`]), so every platform renders the same performance. A kit at rest is a fixed point, and
//! skipped.
//!
//! **This is P1, the spike**: charge, the pool, three of the five engines (Ring, Spark, Dust) and
//! three models (a kick, a snare and a clap) on two linked cells, to hear whether charge breathes.
//! The design is the repository's `docs/briefs/mxm-chaos-drums.md`.

pub mod cell;
pub mod engine;
pub mod fmath;
pub mod kit;
pub mod model;
mod pool;
mod voice;

pub use cell::{CHARGE_FLOOR, CellParams, LEAK_MS};
pub use kit::{CELLS, Event, EventKind, Kit, KitParams};
pub use model::{AVAILABLE_MODELS, Kind, ModelId, ModelSpec, available};
