//! Resample: each cell rendered to a one-shot, and the one-shots played in the cells' place (the
//! plan's P3, as the drum machine's Resample, whose capture thread and pack are mxm-kit's
//! `mxm-drum-shell`).
//!
//! # A capture is a function of the patch
//!
//! Each cell is struck alone from rest at [`CAPTURE_VELOCITY`], held for [`CAPTURE_GATE_S`], its
//! neighbours at rest but coupled, and recorded before its Level until it falls silent: the kit at
//! rest, or the cell [`QUIET_DB`] under its own peak for [`QUIET_HOLD_SECONDS`], the inaudible rest of
//! its tail left out. One still sounding at [`MAX_CAPTURE_SECONDS`] is faded there and reported
//! truncated. A capture blocks activation when Resample is on, so it has to be quick: a full kit's
//! takes a few hundred milliseconds, every cell sustaining itself at the cap a few seconds
//! (`examples/capture_cost.rs`). The pool and the LFOs
//! start from reset, so the capture is the patch's sound, not its history (the plan's *Resample for
//! a lattice*). **The buses are off while it renders**: a frozen cell plays its hit, and a bus has no
//! hit to give; a droning cell captures what a strike adds to it.
//!
//! What a capture leaves out is what stays live: Level, Pan, Mute, Solo, the cell's output and
//! Master, routes onto them included, act on the one-shot as on the cell. Nothing else reaches a
//! frozen cell; turn Resample off to change it.
//!
//! # Playback
//!
//! A strike starts its cell's one-shot at the strike's gain: its velocity through Dynamics, so a
//! capture at full strength is played quieter rather than struck softer, a loss the plan names. A
//! new strike fades the one-shot still playing over [`FADE_SECONDS`], as a choke, a group's choke, a
//! panic and a transport reset do; releases, touch, tuning, the wheel and pressure play nothing.

use crate::cell::{Bus, CellParams};
use crate::kit::{CELLS, Event, EventKind, Kit, KitParams};
use crate::routes::{CellTarget, KitTarget, Route, Source};

/// The strike every capture is taken at: full strength, which a pack is too.
pub const CAPTURE_VELOCITY: f32 = 1.0;
/// How long a capture's note is held: half a sixteenth at 120 BPM, a sequencer's gate.
pub const CAPTURE_GATE_S: f32 = 0.0625;
/// The longest one-shot: a tail still sounding here is faded out and reported truncated. Half the
/// drum machine's, as a cell that sustains itself would otherwise make every capture that long.
pub const MAX_CAPTURE_SECONDS: f32 = 4.0;
/// A cell this far under its own peak has gone quiet: 80 dB, under a 16-bit file's floor.
pub const QUIET_DB: f32 = -80.0;
/// How long a cell must stay quiet before its capture ends: long enough that a sparse crackle's gap
/// between two grains is not taken for the end.
pub const QUIET_HOLD_SECONDS: f32 = 0.25;
/// The fade on a truncated tail, long enough not to click.
pub const TRUNCATION_FADE_SECONDS: f32 = 0.010;
/// How long a one-shot takes to fade when a new strike, a choke or a reset ends it.
pub const FADE_SECONDS: f32 = 0.004;

/// How much of a capture renders before it looks at whether the kit has rested.
const REST_CHECK_FRAMES: usize = 256;

/// The settings a capture is rendered from, which are also what decides whether a capture still
/// stands: the kit's and every cell's, with all that stays live while frozen neutral (the mix, its
/// routes, Dynamics, which a velocity-1 hit cannot hear), and all that would disturb the isolated
/// hit taken away (the buses and every route onto them, the choke groups, whose strike would latch
/// a neighbour out of the coupling). A capture's sources are its LFOs and each hit's Random: a
/// velocity-1 hit's Velocity, and the Wheel and Pressure no capture sends, are exactly zero, so a
/// route from them moves nothing and is left out. An LFO no route left reads stands still and moves
/// nothing, so its Rate and Shape are Init's in the key.
#[must_use]
pub fn capture_patches(
    kit: KitParams,
    cells: [CellParams; CELLS],
) -> (KitParams, [CellParams; CELLS]) {
    let kit = KitParams {
        bus: [0.0; 2],
        master: 1.0,
        dynamics: 0.0,
        routes: kit.routes.map(|route| match (route.source, route.target) {
            (_, KitTarget::BusA | KitTarget::BusB) => Route::default(),
            (Source::Lfo1 | Source::Lfo2 | Source::Lfo3, _) => route,
            _ => Route::default(),
        }),
        ..kit
    };
    let cells = cells.map(|cell| CellParams {
        level: 1.0,
        pan: 0.0,
        mute: false,
        solo: false,
        output: 0,
        bus: Bus::Off,
        choke_group: 0,
        routes: cell.routes.map(|route| match (route.source, route.target) {
            (_, CellTarget::Level | CellTarget::Pan) => Route::default(),
            (Source::Lfo1 | Source::Lfo2 | Source::Lfo3 | Source::Random, _) => route,
            _ => Route::default(),
        }),
        ..cell
    });
    let reads = |lfo: Source| {
        kit.routes
            .iter()
            .any(|route| route.source == lfo && route.target != KitTarget::Off)
            || cells.iter().any(|cell| {
                cell.routes
                    .iter()
                    .any(|route| route.source == lfo && route.target != CellTarget::Off)
            })
    };
    let init = KitParams::default();
    let mut kit = kit;
    for (i, lfo) in [Source::Lfo1, Source::Lfo2, Source::Lfo3]
        .into_iter()
        .enumerate()
    {
        if !reads(lfo) {
            kit.lfo_rate_hz[i] = init.lfo_rate_hz[i];
            kit.lfo_shape[i] = init.lfo_shape[i];
        }
    }
    (kit, cells)
}

/// Every cell's one-shot, at the rate it was rendered, stamped with the engage it answers.
#[derive(Clone, Debug, PartialEq)]
pub struct KitCapture {
    cells: [Vec<f32>; CELLS],
    truncated: [bool; CELLS],
    sample_rate: f32,
    request: u32,
}

impl KitCapture {
    /// The same kit, answering engage `request`.
    #[must_use]
    pub fn answering(self, request: u32) -> Self {
        Self { request, ..self }
    }

    #[must_use]
    pub fn request(&self) -> u32 {
        self.request
    }

    #[must_use]
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Cell `cell`'s one-shot, before its Level; empty for a cell that sounds nothing.
    #[must_use]
    pub fn cell(&self, cell: usize) -> &[f32] {
        &self.cells[cell]
    }

    /// Whether `cell`'s tail was still sounding at [`MAX_CAPTURE_SECONDS`] and was faded there.
    #[must_use]
    pub fn truncated(&self, cell: usize) -> bool {
        self.truncated[cell]
    }

    /// The longest one-shot, in frames: what a frozen hit can still ring for.
    #[must_use]
    pub fn longest(&self) -> usize {
        self.cells.iter().map(Vec::len).max().unwrap_or(0)
    }
}

/// A capture under way, rendered a chunk at a time so the thread doing it can stop between chunks.
pub struct KitCaptureInProgress {
    kit: KitParams,
    cells: [CellParams; CELLS],
    sample_rate: f32,
    /// The cell being rendered, and the kit rendering it.
    cell: usize,
    engine: Option<Box<Kit>>,
    recorded: Vec<f32>,
    /// The frames of the cell's gate still to render; its release follows the last.
    gate_left: usize,
    /// The cell's loudest sample so far, and where its present quiet began, if it is quiet.
    peak: f32,
    quiet_from: Option<usize>,
    done: [Vec<f32>; CELLS],
    truncated: [bool; CELLS],
}

impl KitCaptureInProgress {
    /// Begins capturing `patches` (as [`capture_patches`] makes them) at `sample_rate`.
    #[must_use]
    pub fn begin(patches: &(KitParams, [CellParams; CELLS]), sample_rate: f32) -> Self {
        Self {
            kit: patches.0,
            cells: patches.1,
            sample_rate,
            cell: 0,
            engine: None,
            recorded: Vec::new(),
            gate_left: 0,
            peak: 0.0,
            quiet_from: None,
            done: std::array::from_fn(|_| Vec::new()),
            truncated: [false; CELLS],
        }
    }

    /// Renders at most `frames` more, the gate's included; `true` once every cell is captured.
    pub fn render(&mut self, frames: usize) -> bool {
        let mut budget = frames;
        while self.cell < CELLS && budget > 0 {
            budget = budget.saturating_sub(self.render_cell(budget));
        }
        self.cell >= CELLS
    }

    /// Renders up to `budget` frames of the cell under way, finishing it when it falls silent or
    /// reaches the cap; returns the frames it rendered.
    fn render_cell(&mut self, budget: usize) -> usize {
        let rate = self.sample_rate;
        let cell = self.cell;
        let max = (MAX_CAPTURE_SECONDS * rate) as usize;
        let gate = ((CAPTURE_GATE_S * rate) as usize).max(1);
        let hold = (QUIET_HOLD_SECONDS * rate) as usize;
        let quiet = crate::fmath::exp2(QUIET_DB / 20.0 * core::f32::consts::LOG2_10);
        // Each new sample: the loudest so far, and where the present quiet began.
        let note = |peak: &mut f32, quiet_from: &mut Option<usize>, at: usize, x: f32| {
            *peak = peak.max(x.abs());
            if x.abs() > *peak * quiet {
                *quiet_from = None;
            } else if quiet_from.is_none() {
                *quiet_from = Some(at);
            }
        };
        if self.engine.is_none() {
            self.engine = Some(Box::new(Kit::new(rate, self.kit, self.cells)));
            self.recorded.clear();
            self.gate_left = gate;
            self.peak = 0.0;
            self.quiet_from = None;
        }
        let Some(engine) = self.engine.as_mut() else {
            unreachable!("made above");
        };
        let strike = [Event {
            offset: 0,
            kind: EventKind::Strike {
                cell,
                velocity: CAPTURE_VELOCITY,
                gate: true,
            },
        }];
        let mut rendered = 0;
        let mut chunk = [[0.0; CELLS]; REST_CHECK_FRAMES];
        // The gate in chunks like the rest, each within the budget: the kit renders the same samples
        // whatever the chunks, so the release still falls after exactly `gate` frames. Where the shot
        // ends is a function of its samples alone, whatever the chunks: the quiet is judged on every
        // sample, rest before the cap, and either way the shot ends where its last quiet began (at
        // rest the output is exact silence, so both find the same place).
        let finished = 'cell: loop {
            if self.gate_left == 0 {
                if engine.cells_rest() {
                    break false;
                }
                if self.recorded.len() >= max {
                    break true;
                }
                if self
                    .quiet_from
                    .is_some_and(|from| self.recorded.len() >= from + hold)
                {
                    break false;
                }
            }
            if rendered >= budget {
                return rendered;
            }
            let mut n = REST_CHECK_FRAMES
                .min(budget - rendered)
                .min(max - self.recorded.len());
            if self.gate_left > 0 {
                n = n.min(self.gate_left);
            }
            let events: &[Event] = if self.recorded.is_empty() {
                &strike
            } else {
                &[]
            };
            engine.process_raw(events, &mut chunk[..n]);
            rendered += n;
            let after_gate = self.gate_left == 0;
            for frame in &chunk[..n] {
                let at = self.recorded.len();
                self.recorded.push(frame[cell]);
                note(&mut self.peak, &mut self.quiet_from, at, frame[cell]);
                if after_gate
                    && self
                        .quiet_from
                        .is_some_and(|from| self.recorded.len() >= from + hold)
                {
                    break 'cell false;
                }
            }
            if self.gate_left > 0 {
                self.gate_left -= n;
                if self.gate_left == 0 {
                    engine.apply_now(&[Event {
                        offset: 0,
                        kind: EventKind::Release { cell },
                    }]);
                }
            }
        };
        if !finished && let Some(from) = self.quiet_from {
            // Under the floor since `from`, to the end: left out.
            self.recorded.truncate(from);
        }
        let mut shot = core::mem::take(&mut self.recorded);
        if finished {
            // Still sounding at the cap: faded out over its last moments, and said.
            let fade = ((TRUNCATION_FADE_SECONDS * rate) as usize).clamp(1, shot.len().max(1));
            let start = shot.len().saturating_sub(fade);
            for (k, sample) in shot[start..].iter_mut().enumerate() {
                *sample *= 1.0 - (k + 1) as f32 / fade as f32;
            }
            self.truncated[cell] = true;
        }
        // Rest is exact silence, so a rested shot's trailing zeros carry nothing; a truncated one
        // keeps its length, faded to zero.
        while !finished && shot.last() == Some(&0.0) {
            shot.pop();
        }
        self.done[cell] = shot;
        self.engine = None;
        self.cell += 1;
        rendered
    }

    /// The finished capture, answering engage 0 until [`KitCapture::answering`] stamps it.
    #[must_use]
    pub fn finish(self) -> KitCapture {
        KitCapture {
            cells: self.done,
            truncated: self.truncated,
            sample_rate: self.sample_rate,
            request: 0,
        }
    }
}

/// A whole capture, now. Off the audio thread: it allocates a kit for every cell.
#[must_use]
pub fn capture_kit(patches: &(KitParams, [CellParams; CELLS]), sample_rate: f32) -> KitCapture {
    capture_kit_until(patches, sample_rate, &|| false).expect("never stopped")
}

/// [`capture_kit`], stopping between chunks if `stop` says to: `None` when it stopped.
#[must_use]
pub fn capture_kit_until(
    patches: &(KitParams, [CellParams; CELLS]),
    sample_rate: f32,
    stop: &dyn Fn() -> bool,
) -> Option<KitCapture> {
    let mut progress = KitCaptureInProgress::begin(patches, sample_rate);
    loop {
        if stop() {
            return None;
        }
        if progress.render(65_536) {
            return Some(progress.finish());
        }
    }
}

/// One one-shot being read: where it is, at what gain, and how far it has faded.
#[derive(Clone, Copy, Debug, Default)]
struct Reader {
    at: usize,
    gain: f32,
    /// 1 while it plays, falling to 0 once it fades.
    fade: f32,
    fading: bool,
    live: bool,
    /// What it played last, for a strike that cuts it short.
    last: f32,
}

impl Reader {
    fn sample(&mut self, shot: &[f32], step: f32) -> f32 {
        if !self.live {
            return 0.0;
        }
        let Some(&x) = shot.get(self.at) else {
            *self = Self::default();
            return 0.0;
        };
        self.at += 1;
        let out = x * self.gain * self.fade;
        self.last = out;
        if self.fading {
            self.fade -= step;
            if self.fade <= 0.0 {
                *self = Self::default();
            }
        }
        out
    }
}

/// What a frozen cell plays: its one-shot, the one a new strike is fading out, and the tail of one a
/// third strike cut short inside its fade.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Player {
    now: Reader,
    gone: Reader,
    /// Where a fade cut short left off, falling to zero by `tail_fall` a sample over one fade, so
    /// strikes faster than a fade never jump; clamped as a cell's declick is.
    tail: f32,
    tail_fall: f32,
}

impl Player {
    /// A hit at `gain`: the one-shot from its start, the one playing faded out under it; one still
    /// fading from before goes on from where it was as a falling tail. `step` is a fade's fall a
    /// sample.
    pub(crate) fn strike(&mut self, gain: f32, step: f32) {
        self.stop();
        if self.gone.live {
            self.tail = (self.tail + self.gone.last).clamp(-1.0, 1.0);
            self.tail_fall = self.tail.abs() * step;
        }
        self.gone = self.now;
        self.now = Reader {
            at: 0,
            gain,
            fade: 1.0,
            fading: false,
            live: true,
            last: 0.0,
        };
    }

    /// Whatever plays fades out.
    pub(crate) fn stop(&mut self) {
        for reader in [&mut self.now, &mut self.gone] {
            if reader.live {
                reader.fading = true;
            }
        }
    }

    /// Silence at once: a reset's, or a frozen kit taken away.
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn is_quiet(&self) -> bool {
        !self.now.live && !self.gone.live && self.tail == 0.0
    }

    /// One sample of `shot`, `step` the fade's fall a sample.
    pub(crate) fn tick(&mut self, shot: &[f32], step: f32) -> f32 {
        let tail = self.tail;
        if tail != 0.0 {
            self.tail = if tail.abs() <= self.tail_fall {
                0.0
            } else if tail > 0.0 {
                tail - self.tail_fall
            } else {
                tail + self.tail_fall
            };
        }
        self.now.sample(shot, step) + self.gone.sample(shot, step) + tail
    }
}
