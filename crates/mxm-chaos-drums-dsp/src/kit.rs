//! The kit: the cells, the link between them, the pool, and the events that drive them.

use crate::cell::{Cell, CellParams};
use crate::pool::Pool;

/// P1's two cells, linked. P2 puts sixteen on a 4×4 torus.
pub const CELLS: usize = 2;

/// Flow at full moves charge between neighbours at this rate, per second.
const FLOW_FULL: f32 = 25.0;

/// The kit-wide settings P1 has.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KitParams {
    /// Steady to wild, 0…1. At 0 there is no Flow, no Coupling and no jitter.
    pub temper: f32,
    /// How fast charge moves between neighbours, 0…1, scaled by Temper.
    pub flow: f32,
    /// How much of each cell's sound reaches its neighbours, 0…1, scaled by Temper.
    pub coupling: f32,
    /// The shared supply's capacity, 0…1.
    pub pool: f32,
    /// The time the pool takes to refill, in milliseconds.
    pub recharge_ms: f32,
    /// Chooses every seeded stream. Applied at the next reset.
    pub seed: u32,
}

impl Default for KitParams {
    fn default() -> Self {
        Self {
            temper: 0.0,
            flow: 0.5,
            coupling: 0.5,
            pool: 1.0,
            recharge_ms: 500.0,
            seed: 1,
        }
    }
}

/// What happens at an event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EventKind {
    /// A note-on at `velocity`, 0…1; zero is a release.
    NoteOn {
        cell: usize,
        velocity: f32,
    },
    NoteOff {
        cell: usize,
    },
}

/// An event at `offset` samples into the block. A block's events are in offset order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Event {
    pub offset: usize,
    pub kind: EventKind,
}

pub struct Kit {
    sample_rate: f32,
    dt: f32,
    params: KitParams,
    cells: [Cell; CELLS],
    pool: Pool,
    /// Each cell's last output, which its neighbours hear one sample later.
    last: [f32; CELLS],
}

impl Kit {
    #[must_use]
    pub fn new(sample_rate: f32, params: KitParams, cells: [CellParams; CELLS]) -> Self {
        let sample_rate = mxm_dsp::guard::finite_or(sample_rate, 48_000.0).max(8_000.0);
        let mut index = 0;
        Self {
            sample_rate,
            dt: 1.0 / sample_rate,
            params,
            cells: cells.map(|cell| {
                index += 1;
                Cell::new(cell, sample_rate, params.seed, index - 1)
            }),
            pool: Pool::new(params.pool, params.recharge_ms, sample_rate),
            last: [0.0; CELLS],
        }
    }

    /// New kit settings. A new Seed applies at the next [`Kit::reset`].
    pub fn set_params(&mut self, params: KitParams) {
        self.params = params;
        self.pool
            .set(params.pool, params.recharge_ms, self.sample_rate);
    }

    pub fn set_cell(&mut self, cell: usize, params: CellParams) {
        self.cells[cell].set_params(params);
    }

    /// Exact silence and rest: every charge empty, every engine cleared, the pool full and every
    /// stream back to its seed.
    pub fn reset(&mut self) {
        for cell in &mut self.cells {
            cell.reset(self.params.seed);
        }
        self.pool.fill();
        self.last = [0.0; CELLS];
    }

    pub fn charge(&self, cell: usize) -> f32 {
        self.cells[cell].charge()
    }

    pub fn pool_level(&self) -> f32 {
        self.pool.level()
    }

    /// Nothing sounding and nothing pending: processing would change nothing, so it is skipped.
    pub fn is_at_rest(&self) -> bool {
        self.pool.is_full() && self.cells.iter().all(Cell::is_at_rest)
    }

    /// Render `out.len()` samples, one output per cell, applying `events` at their offsets. Events
    /// at one sample apply as releases, then strikes, whatever their order in the slice; two strikes
    /// on one cell keep the harder.
    pub fn process(&mut self, events: &[Event], out: &mut [[f32; CELLS]]) {
        debug_assert!(
            events
                .windows(2)
                .all(|pair| pair[0].offset <= pair[1].offset)
        );
        let mut next = 0;
        for (n, frame) in out.iter_mut().enumerate() {
            let mut strikes = [0.0f32; CELLS];
            let mut releases = [false; CELLS];
            let mut any = false;
            while next < events.len() && events[next].offset <= n {
                if events[next].offset == n {
                    any = true;
                    match events[next].kind {
                        EventKind::NoteOn { cell, velocity } if cell < CELLS => {
                            if velocity > 0.0 {
                                strikes[cell] = strikes[cell].max(velocity.min(1.0));
                            } else {
                                releases[cell] = true;
                            }
                        }
                        EventKind::NoteOff { cell } if cell < CELLS => releases[cell] = true,
                        _ => {}
                    }
                }
                next += 1;
            }
            if !any && self.is_at_rest() {
                *frame = [0.0; CELLS];
                continue;
            }
            for (cell, release) in self.cells.iter_mut().zip(releases) {
                if release {
                    cell.note_off();
                }
            }
            for (cell, velocity) in self.cells.iter_mut().zip(strikes) {
                if velocity > 0.0 {
                    cell.note_on(velocity);
                }
            }
            *frame = self.tick();
        }
    }

    /// One sample of the whole kit. Every cell reads the previous sample's charges and sounds, so
    /// the cells' order cannot matter.
    fn tick(&mut self) -> [f32; CELLS] {
        let temper = self.params.temper.clamp(0.0, 1.0);
        let grants = self.pool.grant(self.cells.each_ref().map(Cell::demand));

        let [a, b] = &self.cells;
        let link = a.link() * b.link();
        // Flow moves charge toward the emptier cell and never creates it; at most half the
        // difference, so it can never overshoot.
        let gap = b.charge() - a.charge();
        let flow =
            (FLOW_FULL * self.params.flow.clamp(0.0, 1.0) * temper * link * self.dt).min(0.5) * gap;
        let couple = self.params.coupling.clamp(0.0, 1.0) * temper * link;
        let inflow = [grants[0] + flow, grants[1] - flow];
        let heard = [couple * self.last[1], couple * self.last[0]];

        let mut out = [0.0; CELLS];
        for (index, cell) in self.cells.iter_mut().enumerate() {
            out[index] = cell.tick(inflow[index], heard[index], temper);
        }
        self.pool.recharge();
        self.last = out;
        out
    }
}
