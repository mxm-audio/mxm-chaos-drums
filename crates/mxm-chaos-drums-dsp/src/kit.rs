//! The kit: sixteen cells on a 4×4 torus, the pool, the buses, the events that drive them and the
//! outputs they reach.

use mxm_drum_engine::{LFO_SEEDS, Lfo, LfoShape, OUTPUT_TRANSFER_SECONDS, OutputFrame, SLOT_COUNT};
use mxm_dsp::guard::finite_or;
use mxm_part_routing::{Destination, DestinationRouter};

use crate::capture::{FADE_SECONDS, KitCapture, Player};
use crate::cell::{Cell, CellParams, velocity_curve};
use crate::curves::{self, RECHARGE_OCTAVES};
use crate::fmath;
use crate::pool::Pool;
use crate::routes::{self, CellTarget, KitMods, KitTarget, ROUTES, Route, Sources};

/// The lowest sample rate a kit runs at, as the drum machine's.
pub const MIN_SAMPLE_RATE: f32 = 1_000.0;

/// Sixteen cells, one a key, as the drum engine's sixteen slots.
pub const CELLS: usize = SLOT_COUNT;
/// The grid's side: cell `n` sits at row `n / 4`, column `n % 4`.
pub const SIDE: usize = 4;
/// Every edge of the torus once: each cell to its right and its lower neighbour.
pub const EDGES: usize = 2 * CELLS;

/// Flow at full moves charge between neighbours at this rate, per second: fast enough that a full
/// cell empties into its neighbours within a beat, so cells drain each other hard (the owner,
/// 2026-11-03).
const FLOW_FULL: f32 = 100.0;
/// The most one edge may move in a sample, as a share of the difference: four edges a cell at this
/// stay under half, so Flow can never overshoot.
const FLOW_EDGE_MAX: f32 = 0.12;
/// Coupling at full feeds a cell this much of its neighbours' mean sound: enough to drive a quiet
/// cell into its own regime from theirs. Every engine bounds what it is fed.
const COUPLE_FULL: f32 = 6.0;

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
    /// The time the pool takes to refill, 0…1 along [`curves::recharge_ms`]: 10 ms to 10 s.
    pub recharge: f32,
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
            // About 510 ms.
            recharge: 0.57,
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
    /// Which LFOs a route carries with a net amount: only they advance.
    reach: [bool; 3],
    wheel: f32,
    pressure: f32,
    routers: [DestinationRouter; CELLS],
    /// The charge moved across each edge since the last [`Kit::take_flows`], first cell to second.
    flows: [f32; EDGES],
    /// This sample's kit settings, its routes applied.
    now: Now,
    /// Each cell's Level and Pan, and Master, as the mix follows them, and how fast.
    mixer: [[f32; 2]; CELLS],
    master: f32,
    smoothing: f32,
    /// Resample's frozen kit, when one is installed, what each cell plays from it, and how fast a
    /// one-shot fades (`capture`).
    frozen: Option<Box<KitCapture>>,
    players: [Player; CELLS],
    fade_step: f32,
}

/// The kit's settings for one sample, its routes applied.
#[derive(Clone, Copy, Debug, Default)]
struct Now {
    temper: f32,
    flow: f32,
    coupling: f32,
    buses: [f32; 2],
}

/// How fast the mix follows Level, Pan and Master: a one-pole of this time constant.
const MIX_FOLLOW_S: f32 = 0.005;

/// One step of a one-pole toward `target`, landing on it exactly once close.
fn follow(current: f32, target: f32, alpha: f32) -> f32 {
    let next = current + (target - current) * alpha;
    if (target - next).abs() < 1e-6 {
        target
    } else {
        next
    }
}

impl Kit {
    /// A kit at rest. Allocates the cells' delay lines: call when activating, never in `process`.
    #[must_use]
    pub fn new(sample_rate: f32, params: KitParams, cells: [CellParams; CELLS]) -> Self {
        let sample_rate = finite_or(sample_rate, 48_000.0).max(MIN_SAMPLE_RATE);
        let mut index = 0;
        let transition = (OUTPUT_TRANSFER_SECONDS * sample_rate) as u32;
        let mut kit = Self {
            sample_rate,
            dt: 1.0 / sample_rate,
            params,
            cells: cells.map(|cell| {
                index += 1;
                Cell::new(cell, sample_rate, params.seed, index - 1)
            }),
            pool: Pool::new(
                params.pool,
                curves::recharge_ms(params.recharge),
                sample_rate,
            ),
            lfos: lfos(params.seed),
            reach: [false; 3],
            wheel: 0.0,
            pressure: 0.0,
            routers: cells.map(|cell| DestinationRouter::new(destination(cell.output), transition)),
            flows: [0.0; EDGES],
            now: Now::default(),
            mixer: cells.map(|cell| [cell.level.max(0.0), cell.pan.clamp(-1.0, 1.0)]),
            master: params.master.max(0.0),
            smoothing: (1.0 / (MIX_FOLLOW_S * sample_rate)).min(1.0),
            frozen: None,
            players: [Player::default(); CELLS],
            fade_step: 1.0 / (FADE_SECONDS * sample_rate).max(1.0),
        };
        kit.reach = kit.lfo_reach();
        // A fresh kit is a reset one: its routes settled at the LFOs' start, its pool full at the
        // capacity they make, its mixer on the settings they make.
        kit.reset();
        kit
    }

    #[must_use]
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    #[must_use]
    pub fn params(&self) -> &KitParams {
        &self.params
    }

    /// New kit settings. A new Seed applies at the next reset or transport reset. The pool takes its
    /// new capacity and refill at the next sample, with the routes applied (`settle`).
    pub fn set_params(&mut self, params: KitParams) {
        self.params = params;
        self.reach = self.lfo_reach();
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
        self.reach = self.lfo_reach();
    }

    /// Which LFOs reach the performance, from the settings as they stand.
    fn lfo_reach(&self) -> [bool; 3] {
        let mut reach = routes::lfo_reach(&self.params.routes, KitTarget::Off);
        for cell in &self.cells {
            let cell_reach = routes::lfo_reach(&cell.params().routes, CellTarget::Off);
            for (lfo, cell) in reach.iter_mut().zip(cell_reach) {
                *lfo |= cell;
            }
        }
        reach
    }

    /// Exact silence and rest: every charge empty, every engine cleared, the pool full, the LFOs at
    /// their start and every stream back to its seed. Activation's and a host reset's.
    pub fn reset(&mut self) {
        for (cell, router) in self.cells.iter_mut().zip(&mut self.routers) {
            cell.reset(self.params.seed);
            router.reset(destination(cell.params().output));
        }
        self.lfos = lfos(self.params.seed);
        self.wheel = 0.0;
        self.pressure = 0.0;
        self.settle_and_fill();
        self.flows = [0.0; EDGES];
        for player in &mut self.players {
            player.clear();
        }
        for (mixer, cell) in self.mixer.iter_mut().zip(&self.cells) {
            *mixer = [cell.level(), cell.pan()];
        }
        self.master = self.params.master.max(0.0);
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

    /// At rest and staying there: every cell rests and the pool is full, so nothing changes until an
    /// event or an LFO feeds a cell, and a host may stop calling. The pool must be full because its
    /// level is what the next note draws on.
    #[must_use]
    pub fn is_at_rest(&self) -> bool {
        self.pool.is_full() && self.cells_rest() && self.players_quiet()
    }

    /// No frozen one-shot plays.
    fn players_quiet(&self) -> bool {
        self.players.iter().all(Player::is_quiet)
    }

    /// Installs Resample's frozen kit, or with `None` takes it away, returning the one displaced.
    /// What a frozen cell was playing stops at once, as its one-shot goes with the kit. Engaging lets
    /// a live tail ring on but feeds it nothing: every gate and touch lets go, and while frozen the
    /// buses are off (a frozen cell plays its hit, and a bus has none), so the live core only decays
    /// to rest. Either way the kit settles at once, so a kit at rest the host skips still hears it.
    /// **Audio thread** too: the box moves, nothing is made or freed.
    pub fn set_captures(&mut self, capture: Option<Box<KitCapture>>) -> Option<Box<KitCapture>> {
        for player in &mut self.players {
            player.clear();
        }
        if capture.is_some() {
            for cell in &mut self.cells {
                cell.release();
                cell.touch_off();
            }
        }
        let displaced = core::mem::replace(&mut self.frozen, capture);
        self.settle_now();
        displaced
    }

    /// Whether a frozen kit plays: strikes start one-shots, not cells.
    #[must_use]
    pub fn is_frozen(&self) -> bool {
        self.frozen.is_some()
    }

    /// Nothing sounding and nothing about to feed a cell, so the kit skips its core: a tick of
    /// resting cells only recharges the pool, which still runs, and the mixer sits on its settings.
    #[must_use]
    pub fn cells_rest(&self) -> bool {
        // A pool of no capacity grants nothing, so a bus feeds nothing through it.
        let fed = if self.pool.has_capacity() {
            self.now.buses
        } else {
            [0.0; 2]
        };
        self.cells.iter().all(|cell| cell.is_at_rest(fed))
    }

    /// Whether an LFO reaches the performance: a route carries one onto a target with a net amount.
    /// Its phase is then state a later strike reads (its pitch, its strike, a bus, the pool), and a
    /// host that slept would freeze it where one that kept calling would not, so the same events
    /// would play differently. The plugin keeps processing while this holds; the kit still skips its
    /// core while its cells rest.
    #[must_use]
    pub fn lfo_reaches_performance(&self) -> bool {
        self.reach.contains(&true)
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
        let resting = self.cells_rest() && self.players_quiet();
        let mut sample = Sample::default();
        let mut any = false;
        while *next < events.len() && events[*next].offset <= n {
            if events[*next].offset == n {
                sample.add(events[*next].kind);
                any = true;
            }
            *next += 1;
        }
        // Resets first, so what follows reads the reset's sources; then each hit with its own
        // Velocity and Random and this sample's LFOs; then the kit's and the cells' settings with the
        // routes applied, before the rest check, since a route can be what feeds a resting cell.
        if any {
            self.apply_resets(&sample);
            self.apply_events(&sample);
        }
        let sources = self.sources();
        self.settle(&sources);
        if resting {
            self.settle_mix();
        }
        self.advance_lfos();
        let mut out = if !any && self.cells_rest() {
            // Resting cells ask nothing of the pool and change nothing, so their tick is this.
            self.pool.recharge();
            [0.0; CELLS]
        } else {
            self.tick()
        };
        self.play(&mut out);
        out
    }

    /// One sample's notes, to the cells or, while frozen, to their one-shots.
    fn apply_events(&mut self, sample: &Sample) {
        if self.frozen.is_some() {
            self.apply_frozen(sample);
        } else {
            let sources = self.sources();
            self.apply_notes(sample, &sources);
        }
    }

    /// One sample's notes while frozen: a strike starts its cell's one-shot and chokes its group as
    /// a live one does; a choke ends a one-shot; releases and touches reach the live tails still
    /// ringing, which a frozen strike on their cell cuts; tuning plays nothing.
    fn apply_frozen(&mut self, sample: &Sample) {
        for (index, cell) in self.cells.iter_mut().enumerate() {
            if sample.all_off || sample.release[index] {
                cell.release();
            }
            if sample.all_off || sample.touch_off[index] {
                cell.touch_off();
            }
            if sample.choke[index] {
                cell.choke();
                self.players[index].stop();
            }
        }
        let mut groups = 0u32;
        for (cell, velocity) in self.cells.iter().zip(sample.strike) {
            let group = cell.params().choke_group;
            if velocity > 0.0 && group > 0 {
                groups |= 1 << group.min(31);
            }
        }
        let dynamics = self.params.dynamics;
        let fade_step = self.fade_step;
        for (index, (cell, player)) in self.cells.iter_mut().zip(&mut self.players).enumerate() {
            let group = cell.params().choke_group;
            let velocity = sample.strike[index];
            if velocity > 0.0 {
                // The hit's own Velocity and Random, as a live hit's, for the routes onto its mix.
                cell.stage_hit(velocity);
                player.strike(velocity_curve(velocity, dynamics), fade_step);
                cell.choke();
            } else if group > 0 && groups & (1 << group.min(31)) != 0 {
                cell.choke();
                player.stop();
            }
        }
        if let Some(wheel) = sample.wheel {
            self.wheel = wheel.clamp(0.0, 1.0);
        }
        if let Some(pressure) = sample.pressure {
            self.pressure = pressure.clamp(0.0, 1.0);
        }
    }

    /// The frozen one-shots playing, added to what the cells made.
    fn play(&mut self, out: &mut [f32; CELLS]) {
        let Some(frozen) = self.frozen.as_deref() else {
            return;
        };
        for (cell, player) in self.players.iter_mut().enumerate() {
            if !player.is_quiet() {
                out[cell] += player.tick(frozen.cell(cell), self.fade_step);
            }
        }
    }

    /// Apply `events` now, before the next sample, whatever their offsets: a host callback with no
    /// frames still carries notes, and they take effect where the next frame starts.
    pub fn apply_now(&mut self, events: &[Event]) {
        // Whether the next sample's events find every cell at rest, asked before they apply, as
        // the sample would ask it.
        let resting = self.cells_rest() && self.players_quiet();
        let mut sample = Sample::default();
        for event in events {
            sample.add(event.kind);
        }
        self.apply_resets(&sample);
        self.apply_events(&sample);
        self.settle_now();
        if resting {
            self.settle_mix();
        }
    }

    /// Apply the routes as the next sample will, without advancing time: after settings or events
    /// that arrive with no frames, so [`Kit::is_at_rest`] answers for what they made. The next
    /// sample settles the same way first, so this changes nothing it renders.
    pub fn settle_now(&mut self) {
        let sources = self.sources();
        self.settle(&sources);
    }

    /// This sample's sources: the LFOs where they stand, the wheel and the pressure.
    fn sources(&self) -> Sources {
        let mut sources = Sources {
            lfo: [0.0; 3],
            wheel: self.wheel,
            pressure: self.pressure,
        };
        for ((lfo, value), shape) in self
            .lfos
            .iter()
            .zip(&mut sources.lfo)
            .zip(self.params.lfo_shape)
        {
            *value = match shape {
                // The kit LFO's Sine calls the platform's `sin`: its phase, from RampUp, through ours.
                LfoShape::Sine => fmath::sin_turns(0.5 * (lfo.value(LfoShape::RampUp) + 1.0)),
                shape => lfo.value(shape),
            };
        }
        sources
    }

    fn advance_lfos(&mut self) {
        // An LFO that reaches nothing stands still, so no host's sleep can move a phase anyone
        // reads: a route that starts carrying it finds it where it stood.
        for ((lfo, rate), moves) in self
            .lfos
            .iter_mut()
            .zip(self.params.lfo_rate_hz)
            .zip(self.reach)
        {
            if moves {
                lfo.advance(rate, self.sample_rate);
            }
        }
    }

    /// The routes at the sources as they stand, then the pool full at the capacity they make: a
    /// reset's pool is full whatever the LFOs had made of it before.
    fn settle_and_fill(&mut self) {
        let sources = self.sources();
        self.settle(&sources);
        self.pool.fill();
    }

    /// Apply this sample's routes: the kit's effective settings, the pool's capacity and refill, and
    /// every cell's. Each is a function of the parameters and the sources alone, and the pool takes a
    /// setting only when it differs, so settling twice is settling once.
    fn settle(&mut self, sources: &Sources) {
        let mods = if routes::any_in_use(&self.params.routes) {
            routes::kit_mods(&self.params.routes, sources)
        } else {
            KitMods::default()
        };
        let p = &self.params;
        self.now = Now {
            temper: (p.temper + mods.temper).clamp(0.0, 1.0),
            flow: (p.flow + mods.flow).clamp(0.0, 1.0),
            coupling: (p.coupling + mods.coupling).clamp(0.0, 1.0),
            // Frozen, the buses feed no live cell.
            buses: if self.frozen.is_some() {
                [0.0; 2]
            } else {
                [
                    (p.bus[0] + mods.bus[0]).clamp(0.0, 1.0),
                    (p.bus[1] + mods.bus[1]).clamp(0.0, 1.0),
                ]
            },
        };
        let recharge = (p.recharge + mods.recharge_octaves / RECHARGE_OCTAVES).clamp(0.0, 1.0);
        let pool = (p.pool + mods.pool).clamp(0.0, 1.0);
        self.pool
            .configure(pool, curves::recharge_ms(recharge), self.sample_rate);
        for cell in &mut self.cells {
            cell.prepare(sources);
        }
    }

    /// One sample's resets: a transport reset, then a panic.
    fn apply_resets(&mut self, sample: &Sample) {
        // A transport reset is the performance from rest: every cell, the pool, the LFOs, the wheel
        // and the pressure, and every stream from the kit's current Seed.
        if sample.restart {
            for cell in &mut self.cells {
                cell.restart(self.params.seed);
            }
            self.lfos = lfos(self.params.seed);
            self.wheel = 0.0;
            self.pressure = 0.0;
            self.settle_and_fill();
            for player in &mut self.players {
                player.stop();
            }
        }
        // Panic is silence and no expression: what no channel holds routes nothing.
        if sample.panic {
            for cell in &mut self.cells {
                cell.panic(self.params.seed);
            }
            for player in &mut self.players {
                player.clear();
            }
            self.wheel = 0.0;
            self.pressure = 0.0;
        }
    }

    /// One sample's notes, in their fixed order: releases and chokes, strikes and the group chokes
    /// they make, then expression. A hit's Velocity and Random are its own before its Strike and
    /// controls are read, with `sources`, this sample's.
    fn apply_notes(&mut self, sample: &Sample, sources: &Sources) {
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
                cell.stage_hit(velocity);
                cell.prepare(sources);
                cell.strike(velocity_curve(velocity, dynamics), sample.gate[index]);
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
    fn tick(&mut self) -> [f32; CELLS] {
        let Now {
            temper,
            flow,
            coupling,
            buses,
        } = self.now;
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

    /// A sample whose events find every cell at rest begins with Level, Pan, Master and each output on
    /// their settings: nothing has sounded through them since the cells came to rest, so the step is
    /// one no one can hear. It is the timeline's rule, not a callback's, so how many silent samples a
    /// host renders, where its blocks end and whether it sleeps cannot change the mix.
    fn settle_mix(&mut self) {
        self.master = self.params.master.max(0.0);
        for ((cell, mixer), router) in self
            .cells
            .iter()
            .zip(&mut self.mixer)
            .zip(&mut self.routers)
        {
            *mixer = [cell.level(), cell.pan()];
            router.reset(destination(cell.params().output));
        }
    }

    /// Level, Mute and Solo, Pan, the cell's output, and Master. Level, Pan and Master follow their
    /// settings over a few milliseconds, so automating them never zippers; they act after the
    /// performance, which nothing reads back.
    fn mix(&mut self, raw: &[f32; CELLS]) -> OutputFrame {
        let mut frame = OutputFrame::default();
        // Any Solo isolates the soloed cells, a muted one too: Mute wins only for its own cell.
        let soloing = self.cells.iter().any(|cell| cell.params().solo);
        let alpha = self.smoothing;
        self.master = follow(self.master, self.params.master.max(0.0), alpha);
        let master = self.master;
        for (index, ((cell, router), &sample)) in self
            .cells
            .iter()
            .zip(&mut self.routers)
            .zip(raw)
            .enumerate()
        {
            let p = cell.params();
            let audible = !p.mute && (!soloing || p.solo);
            let [level, pan] = &mut self.mixer[index];
            *level = follow(*level, cell.level(), alpha);
            *pan = follow(*pan, cell.pan(), alpha);
            let pan = *pan;
            let signal = if audible {
                sample * *level * master
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
                        let turns = 0.125 * (pan + 1.0);
                        frame.main[0] += routed * fmath::cos_turns(turns);
                        frame.main[1] += routed * fmath::sin_turns(turns);
                    }
                    Destination::Auxiliary(index) => frame.individual[index] += routed,
                }
            }
        }
        frame
    }
}

/// The three LFOs, their sample-and-hold streams seeded from the kit's Seed and the LFO's place,
/// mixed with the drum engine's own seeds so each LFO's stream stays distinct.
fn lfos(seed: u32) -> [Lfo; 3] {
    let mut index = 0;
    LFO_SEEDS.map(|base| {
        index += 1;
        Lfo::new(base ^ mxm_rng::SplitMix64::mix((u64::from(seed) << 8) | index))
    })
}

/// A cell's output setting as a destination: 0 the main pair, 1…16 an individual output.
fn destination(output: u8) -> Destination {
    Destination::from_parameter(i32::from(output), CELLS)
}
