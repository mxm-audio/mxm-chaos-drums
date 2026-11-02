//! The kit: sixteen cells on a 4×4 torus, the pool, the buses, the events that drive them and the
//! outputs they reach.

use mxm_drum_engine::{LFO_SEEDS, Lfo, LfoShape, OUTPUT_TRANSFER_SECONDS, OutputFrame, SLOT_COUNT};
use mxm_dsp::guard::finite_or;
use mxm_part_routing::{Destination, DestinationRouter};

use crate::cell::{Cell, CellParams, velocity_curve};
use crate::fmath;
use crate::pool::Pool;
use crate::routes::{self, KitMods, KitTarget, ROUTES, Route, Sources};

/// Sixteen cells, one a key, as the drum engine's sixteen slots.
pub const CELLS: usize = SLOT_COUNT;
/// The grid's side: cell `n` sits at row `n / 4`, column `n % 4`.
pub const SIDE: usize = 4;
/// Every edge of the torus once: each cell to its right and its lower neighbour.
pub const EDGES: usize = 2 * CELLS;

/// Flow at full moves charge between neighbours at this rate, per second.
const FLOW_FULL: f32 = 25.0;
/// The most one edge may move in a sample, as a share of the difference: four edges a cell at this
/// stay under half, so Flow can never overshoot.
const FLOW_EDGE_MAX: f32 = 0.12;
/// Coupling at full feeds a cell this much of its neighbours' mean sound.
const COUPLE_FULL: f32 = 2.0;

/// The four neighbours of `cell`: above, below, left, right, wrapping round.
#[must_use]
pub const fn neighbours(cell: usize) -> [usize; 4] {
    let (row, col) = (cell / SIDE, cell % SIDE);
    [
        ((row + SIDE - 1) % SIDE) * SIDE + col,
        ((row + 1) % SIDE) * SIDE + col,
        row * SIDE + (col + SIDE - 1) % SIDE,
        row * SIDE + (col + 1) % SIDE,
    ]
}

/// Edge `e`'s two cells: edges `0…16` run right, `16…32` run down.
#[must_use]
pub const fn edge(e: usize) -> (usize, usize) {
    let cell = e % CELLS;
    let (row, col) = (cell / SIDE, cell % SIDE);
    if e < CELLS {
        (cell, row * SIDE + (col + 1) % SIDE)
    } else {
        (cell, ((row + 1) % SIDE) * SIDE + col)
    }
}

/// The kit-wide settings.
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
    /// Bus A's and Bus B's levels, 0…1.
    pub bus: [f32; 2],
    /// Chooses every seeded stream. Applied at the next reset.
    pub seed: u32,
    /// The velocity curve, −1…+1; 0 is linear.
    pub dynamics: f32,
    /// Linear gain on every output.
    pub master: f32,
    pub lfo_rate_hz: [f32; 3],
    pub lfo_shape: [LfoShape; 3],
    pub routes: [Route<KitTarget>; ROUTES],
}

impl Default for KitParams {
    fn default() -> Self {
        Self {
            temper: 0.0,
            flow: 0.5,
            coupling: 0.5,
            pool: 1.0,
            recharge_ms: 500.0,
            bus: [0.0; 2],
            seed: 1,
            dynamics: 0.0,
            master: 1.0,
            lfo_rate_hz: [0.7, 1.3, 4.0],
            lfo_shape: [LfoShape::Sine, LfoShape::Triangle, LfoShape::Sine],
            routes: [Route::default(); ROUTES],
        }
    }
}

/// What happens at an event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EventKind {
    /// A hit at `velocity`, 0…1. `gate`: the note is held past this sample, so it keeps feeding.
    Strike {
        cell: usize,
        velocity: f32,
        gate: bool,
    },
    Release {
        cell: usize,
    },
    /// The host's choke: the cell empties and latches.
    Choke {
        cell: usize,
    },
    TouchOn {
        cell: usize,
        velocity: f32,
    },
    TouchOff {
        cell: usize,
    },
    /// A pitch curve on a cell, in semitones; the next strike clears it.
    Tune {
        cell: usize,
        semitones: f32,
    },
    Wheel(f32),
    Pressure(f32),
    /// Every gate and touch closes; the tails ring on.
    AllNotesOff,
    /// Immediate exact silence, every cell latched.
    Panic,
    /// Every cell from rest at this sample, the old sound declicked: the performance from here on is
    /// the same whatever came before.
    TransportReset,
}

/// An event at `offset` samples into the block. A block's events are in offset order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Event {
    pub offset: usize,
    pub kind: EventKind,
}

/// Everything at one sample, reduced so that the host's order cannot matter.
#[derive(Default)]
struct Sample {
    panic: bool,
    restart: bool,
    all_off: bool,
    release: [bool; CELLS],
    choke: [bool; CELLS],
    touch_off: [bool; CELLS],
    strike: [f32; CELLS],
    gate: [bool; CELLS],
    touch: [f32; CELLS],
    tune: [Option<f32>; CELLS],
    wheel: Option<f32>,
    pressure: Option<f32>,
}

impl Sample {
    fn add(&mut self, kind: EventKind) {
        let larger =
            |held: Option<f32>, value: f32| Some(held.map_or(value, |held| held.max(value)));
        match kind {
            EventKind::Strike {
                cell,
                velocity,
                gate,
            } if cell < CELLS && velocity > 0.0 => {
                self.strike[cell] = self.strike[cell].max(velocity.min(1.0));
                self.gate[cell] |= gate;
            }
            EventKind::Release { cell } if cell < CELLS => self.release[cell] = true,
            EventKind::Choke { cell } if cell < CELLS => self.choke[cell] = true,
            EventKind::TouchOn { cell, velocity } if cell < CELLS && velocity > 0.0 => {
                self.touch[cell] = self.touch[cell].max(velocity.min(1.0));
            }
            EventKind::TouchOff { cell } if cell < CELLS => self.touch_off[cell] = true,
            EventKind::Tune { cell, semitones } if cell < CELLS && semitones.is_finite() => {
                self.tune[cell] = larger(self.tune[cell], semitones);
            }
            EventKind::Wheel(value) if value.is_finite() => self.wheel = larger(self.wheel, value),
            EventKind::Pressure(value) if value.is_finite() => {
                self.pressure = larger(self.pressure, value)
            }
            EventKind::AllNotesOff => self.all_off = true,
            EventKind::Panic => self.panic = true,
            EventKind::TransportReset => self.restart = true,
            _ => {}
        }
    }
}

pub struct Kit {
    sample_rate: f32,
    dt: f32,
    params: KitParams,
    cells: [Cell; CELLS],
    pool: Pool,
    lfos: [Lfo; 3],
    wheel: f32,
    pressure: f32,
    routers: [DestinationRouter; CELLS],
    /// The charge moved across each edge since the last [`Kit::take_flows`], first cell to second.
    flows: [f32; EDGES],
}

impl Kit {
    /// A kit at rest. Allocates the cells' delay lines: call when activating, never in `process`.
    #[must_use]
    pub fn new(sample_rate: f32, params: KitParams, cells: [CellParams; CELLS]) -> Self {
        let sample_rate = finite_or(sample_rate, 48_000.0).max(8_000.0);
        let mut index = 0;
        let transition = (OUTPUT_TRANSFER_SECONDS * sample_rate) as u32;
        Self {
            sample_rate,
            dt: 1.0 / sample_rate,
            params,
            cells: cells.map(|cell| {
                index += 1;
                Cell::new(cell, sample_rate, params.seed, index - 1)
            }),
            pool: Pool::new(params.pool, params.recharge_ms, sample_rate),
            lfos: LFO_SEEDS.map(Lfo::new),
            wheel: 0.0,
            pressure: 0.0,
            routers: cells.map(|cell| DestinationRouter::new(destination(cell.output), transition)),
            flows: [0.0; EDGES],
        }
    }

    #[must_use]
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    #[must_use]
    pub fn params(&self) -> &KitParams {
        &self.params
    }

    /// New kit settings. A new Seed applies at the next reset.
    pub fn set_params(&mut self, params: KitParams) {
        self.params = params;
        self.pool
            .set(params.pool, params.recharge_ms, self.sample_rate);
    }

    #[must_use]
    pub fn cell_params(&self, cell: usize) -> &CellParams {
        self.cells[cell].params()
    }

    /// New settings for one cell. A new model starts from rest with the cell's charge, declicked;
    /// a new output moves there over the drum engine's transfer time.
    pub fn set_cell(&mut self, cell: usize, params: CellParams) {
        if params.output != self.cells[cell].params().output {
            self.routers[cell].request(destination(params.output));
        }
        self.cells[cell].set_params(params);
    }

    /// Exact silence and rest: every charge empty, every engine cleared, the pool full, the LFOs at
    /// their start and every stream back to its seed. Activation's and a host reset's.
    pub fn reset(&mut self) {
        for (cell, router) in self.cells.iter_mut().zip(&mut self.routers) {
            cell.reset(self.params.seed);
            router.reset(destination(cell.params().output));
        }
        self.pool.fill();
        self.lfos.iter_mut().for_each(Lfo::reset);
        self.wheel = 0.0;
        self.pressure = 0.0;
        self.flows = [0.0; EDGES];
    }

    #[must_use]
    pub fn charge(&self, cell: usize) -> f32 {
        self.cells[cell].charge()
    }

    #[must_use]
    pub fn is_latched(&self, cell: usize) -> bool {
        self.cells[cell].is_latched()
    }

    #[must_use]
    pub fn pool_level(&self) -> f32 {
        self.pool.level()
    }

    /// The charge each edge moved since the last call (see [`edge`]), and zero for the next.
    pub fn take_flows(&mut self) -> [f32; EDGES] {
        core::mem::replace(&mut self.flows, [0.0; EDGES])
    }

    /// Nothing sounding and nothing about to feed a cell: processing would change no cell, so it is
    /// skipped; only the LFOs run on.
    #[must_use]
    pub fn is_at_rest(&self) -> bool {
        self.pool.is_full()
            && self
                .cells
                .iter()
                .all(|cell| cell.is_at_rest(self.params.bus))
    }

    /// Render `out.len()` frames, applying `events` at their offsets.
    pub fn process(&mut self, events: &[Event], out: &mut [OutputFrame]) {
        let mut next = 0;
        for (n, frame) in out.iter_mut().enumerate() {
            let raw = self.step(events, &mut next, n);
            *frame = self.mix(&raw);
        }
    }

    /// Render each cell's output before Level, Pan and Master: the performance, which is the same on
    /// every platform.
    pub fn process_raw(&mut self, events: &[Event], out: &mut [[f32; CELLS]]) {
        let mut next = 0;
        for (n, frame) in out.iter_mut().enumerate() {
            *frame = self.step(events, &mut next, n);
        }
    }

    fn step(&mut self, events: &[Event], next: &mut usize, n: usize) -> [f32; CELLS] {
        debug_assert!(
            events
                .windows(2)
                .all(|pair| pair[0].offset <= pair[1].offset)
        );
        let mut sample = Sample::default();
        let mut any = false;
        while *next < events.len() && events[*next].offset <= n {
            if events[*next].offset == n {
                sample.add(events[*next].kind);
                any = true;
            }
            *next += 1;
        }
        if any {
            self.apply(&sample);
        }

        let mut sources = Sources {
            lfo: [0.0; 3],
            wheel: self.wheel,
            pressure: self.pressure,
        };
        for ((lfo, value), (shape, rate)) in self
            .lfos
            .iter_mut()
            .zip(&mut sources.lfo)
            .zip(self.params.lfo_shape.iter().zip(self.params.lfo_rate_hz))
        {
            *value = match shape {
                // The kit LFO's Sine calls the platform's `sin`: its phase, from RampUp, through ours.
                LfoShape::Sine => fmath::sin_turns(0.5 * (lfo.value(LfoShape::RampUp) + 1.0)),
                shape => lfo.value(*shape),
            };
            lfo.advance(rate, self.sample_rate);
        }

        if !any && self.is_at_rest() {
            return [0.0; CELLS];
        }
        self.tick(&sources)
    }

    /// One sample's events, in their fixed order: resets, releases and chokes, strikes and the
    /// group chokes they make, then expression.
    fn apply(&mut self, sample: &Sample) {
        if sample.restart {
            for cell in &mut self.cells {
                cell.restart();
            }
            self.pool.fill();
            self.lfos.iter_mut().for_each(Lfo::reset);
        }
        if sample.panic {
            for cell in &mut self.cells {
                cell.panic();
            }
        }
        for (index, cell) in self.cells.iter_mut().enumerate() {
            if sample.all_off || sample.release[index] {
                cell.release();
            }
            if sample.all_off || sample.touch_off[index] {
                cell.touch_off();
            }
            if sample.choke[index] {
                cell.choke();
            }
        }
        // A strike chokes the other cells of its group, never one struck at the same sample.
        let mut groups = 0u32;
        for (cell, velocity) in self.cells.iter().zip(sample.strike) {
            let group = cell.params().choke_group;
            if velocity > 0.0 && group > 0 {
                groups |= 1 << group.min(31);
            }
        }
        let dynamics = self.params.dynamics;
        for (index, cell) in self.cells.iter_mut().enumerate() {
            let group = cell.params().choke_group;
            let velocity = sample.strike[index];
            if velocity > 0.0 {
                cell.strike(
                    velocity,
                    velocity_curve(velocity, dynamics),
                    sample.gate[index],
                );
            } else if group > 0 && groups & (1 << group.min(31)) != 0 {
                cell.choke();
            }
            if sample.touch[index] > 0.0 {
                cell.touch_on(sample.touch[index]);
            }
            if let Some(semitones) = sample.tune[index] {
                cell.tune(semitones);
            }
        }
        if let Some(wheel) = sample.wheel {
            self.wheel = wheel.clamp(0.0, 1.0);
        }
        if let Some(pressure) = sample.pressure {
            self.pressure = pressure.clamp(0.0, 1.0);
        }
    }

    /// One sample of the whole kit. Every cell reads the previous sample's charges and sounds, and
    /// the pool grants every demand the same share, so no cell's index decides anything.
    fn tick(&mut self, sources: &Sources) -> [f32; CELLS] {
        let mods = if routes::any_in_use(&self.params.routes) {
            routes::kit_mods(&self.params.routes, sources)
        } else {
            KitMods::default()
        };
        let p = &self.params;
        let temper = (p.temper + mods.temper).clamp(0.0, 1.0);
        let flow = (p.flow + mods.flow).clamp(0.0, 1.0);
        let coupling = (p.coupling + mods.coupling).clamp(0.0, 1.0);
        let buses = [
            (p.bus[0] + mods.bus[0]).clamp(0.0, 1.0),
            (p.bus[1] + mods.bus[1]).clamp(0.0, 1.0),
        ];
        if mods.pool != 0.0 || mods.recharge_octaves != 0.0 {
            let recharge = p.recharge_ms * fmath::exp2(mods.recharge_octaves);
            self.pool.set(
                (p.pool + mods.pool).clamp(0.0, 1.0),
                recharge,
                self.sample_rate,
            );
        }

        for cell in &mut self.cells {
            cell.prepare(sources);
        }
        let mut demands = [0.0; CELLS];
        for (demand, cell) in demands.iter_mut().zip(&self.cells) {
            *demand = cell.demand(buses);
        }
        let mut inflow = self.pool.grant(demands);

        let pressure = self.pressure;
        let links: [f32; CELLS] = core::array::from_fn(|i| self.cells[i].link(pressure));
        let rate = (FLOW_FULL * flow * temper * self.dt).min(FLOW_EDGE_MAX);
        if rate > 0.0 {
            for e in 0..EDGES {
                let (a, b) = edge(e);
                // A latched cell's link is zero: the edge refuses, and charge is still conserved.
                let moved =
                    rate * links[a] * links[b] * (self.cells[b].charge() - self.cells[a].charge());
                inflow[a] += moved;
                inflow[b] -= moved;
                self.flows[e] -= moved;
            }
        }
        let couple = COUPLE_FULL * coupling * temper;
        let heard: [f32; CELLS] = core::array::from_fn(|i| {
            if couple == 0.0 {
                return 0.0;
            }
            let sum: f32 = neighbours(i)
                .iter()
                .map(|&j| links[j] * self.cells[j].live())
                .sum();
            couple * links[i] * 0.25 * sum
        });

        let mut out = [0.0; CELLS];
        for (index, cell) in self.cells.iter_mut().enumerate() {
            out[index] = cell.tick(inflow[index], heard[index], temper, pressure);
        }
        self.pool.recharge();
        out
    }

    /// Level, Mute and Solo, Pan, the cell's output, and Master.
    fn mix(&mut self, raw: &[f32; CELLS]) -> OutputFrame {
        let mut frame = OutputFrame::default();
        let soloing = self
            .cells
            .iter()
            .any(|cell| cell.params().solo && !cell.params().mute);
        let master = self.params.master.max(0.0);
        for ((cell, router), &sample) in self.cells.iter().zip(&mut self.routers).zip(raw) {
            let p = cell.params();
            let audible = !p.mute && (!soloing || p.solo);
            let signal = if audible {
                sample * cell.level() * master
            } else {
                0.0
            };
            for route in router.next_partition().into_iter().flatten() {
                let routed = signal * route.gain;
                if routed == 0.0 {
                    continue;
                }
                match route.destination {
                    Destination::Main => {
                        // Equal power, as the drum machine: left cos, right sin of (pan + 1)·π/4.
                        let turns = 0.125 * (cell.pan() + 1.0);
                        frame.main[0] += routed * fmath::cos_turns(turns);
                        frame.main[1] += routed * fmath::sin_turns(turns);
                    }
                    Destination::Auxiliary(index) => frame.individual[index] += routed,
                }
            }
        }
        frame
    }

    /// One cell's one-shot for Resample: that cell struck from rest at `velocity`, held for `gate`
    /// samples, its neighbours at rest but coupled, `len` samples of its output before Level.
    /// Allocates a whole kit: call off the audio thread.
    #[must_use]
    pub fn one_shot(&self, cell: usize, velocity: f32, gate: usize, len: usize) -> Vec<f32> {
        let cells: [CellParams; CELLS] = core::array::from_fn(|i| *self.cells[i].params());
        let mut kit = Kit::new(self.sample_rate, self.params, cells);
        let events = [
            Event {
                offset: 0,
                kind: EventKind::Strike {
                    cell,
                    velocity,
                    gate: gate > 0,
                },
            },
            Event {
                offset: gate.max(1),
                kind: EventKind::Release { cell },
            },
        ];
        let mut frames = vec![[0.0; CELLS]; len];
        kit.process_raw(&events, &mut frames);
        frames.iter().map(|frame| frame[cell]).collect()
    }
}

/// A cell's output setting as a destination: 0 the main pair, 1…16 an individual output.
fn destination(output: u8) -> Destination {
    Destination::from_parameter(i32::from(output), CELLS)
}
