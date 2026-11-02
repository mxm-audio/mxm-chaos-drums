//! What the audio thread tells the editor, and what the editor asks of it: atomics, written once a
//! block, never a lock or an allocation on the audio thread (the collection's editor contract).

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use mxm_chaos_drums_dsp::{CELLS, EDGES};

#[derive(Debug)]
pub struct Telemetry {
    pub meter: mxm_telemetry::PeakMeter,
    pub tempo: mxm_tempo::TempoCell,
    /// Each cell's charge, as of the last block.
    charges: [AtomicU32; CELLS],
    /// The charge each edge moved in the last block, per second: positive from its first cell to its
    /// second (`mxm_chaos_drums_dsp::edge`).
    flows: [AtomicU32; EDGES],
    pool: AtomicU32,
    /// The cells the editor asked to hear, one bit each, struck at the next block.
    audition: AtomicU32,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            meter: mxm_telemetry::PeakMeter::new(),
            tempo: mxm_tempo::TempoCell::new(),
            charges: std::array::from_fn(|_| AtomicU32::new(0)),
            flows: std::array::from_fn(|_| AtomicU32::new(0)),
            pool: AtomicU32::new(0),
            audition: AtomicU32::new(0),
        }
    }
}

fn store(target: &AtomicU32, value: f32) {
    target.store(
        if value.is_finite() { value } else { 0.0 }.to_bits(),
        Ordering::Relaxed,
    );
}

fn load(source: &AtomicU32) -> f32 {
    f32::from_bits(source.load(Ordering::Relaxed))
}

impl Telemetry {
    #[must_use]
    pub fn shared() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The audio thread's report for one block.
    pub fn publish(&self, charges: [f32; CELLS], flows: [f32; EDGES], seconds: f32, pool: f32) {
        for (target, charge) in self.charges.iter().zip(charges) {
            store(target, charge);
        }
        let per_second = if seconds > 0.0 { 1.0 / seconds } else { 0.0 };
        for (target, flow) in self.flows.iter().zip(flows) {
            store(target, flow * per_second);
        }
        store(&self.pool, pool);
    }

    #[must_use]
    pub fn charges(&self) -> [f32; CELLS] {
        std::array::from_fn(|cell| load(&self.charges[cell]))
    }

    #[must_use]
    pub fn flows(&self) -> [f32; EDGES] {
        std::array::from_fn(|edge| load(&self.flows[edge]))
    }

    #[must_use]
    pub fn pool(&self) -> f32 {
        load(&self.pool)
    }

    /// Strike `cell` at the next block, as a note would: the editor's way to hear a cell.
    pub fn request_audition(&self, cell: usize) {
        if cell < CELLS {
            self.audition.fetch_or(1 << cell, Ordering::AcqRel);
        }
    }

    /// The cells asked to be heard since the last call, one bit each.
    pub fn take_audition(&self) -> u32 {
        self.audition.swap(0, Ordering::AcqRel)
    }
}
