# AGENTS.md — crates/mxm-chaos-drums-dsp

## Purpose

The instrument's whole sound as plain Rust: charge, the pool and the buses, the five engines, the
models, and the sixteen cells on the 4×4 torus with their events, routes and outputs. No framework
types, no GUI, no allocation once a kit is built. The crate's docs (`src/lib.rs`) explain the design;
the brief (`docs/briefs/mxm-chaos-drums.md`) is what it implements.

**P2, the DSP crate** (2026-11-02): all five engines, three drum models (Draining kick, Wired snare,
Spark clap) and the five raw ones; the rest of the catalogue is P4's.

## Ownership

- `src/cell.rs`: one cell and the charge law: Strike, Supply (from a held note or the cell's bus),
  Hunger, Leak, the floor and rest; Touch, the choke latch, the declick offset, tuning, the velocity
  curve, and the cell's parameters with its routes applied.
- `src/pool.rs`: the shared supply, divided among every demand at once.
- `src/kit.rs`: the torus, Flow and Coupling, the events and their order, group chokes, the LFOs, the
  kit routes, Level, Pan, Mute, Solo, the outputs, Master, and the one-shot for Resample.
- `src/routes.rs`: the four routes a cell and four for the kit, on the collection's standard.
- `src/voice.rs`: each model's engine network, the controls it reads and its constants.
- `src/engine/`: Ring (van der Pol), Spark (leaky integrate-and-fire), Fold (a phase oscillator
  kicked by the circle map), Loop (`strand.rs`, a nonlinear delay loop) and Dust (velvet noise), and
  the band two of them ring through.
- `src/model.rs`: the permanent model IDs, the twenty-five kinds, and the available models with their
  controls and notes.
- `src/fmath.rs`: `exp2`, semitones and the sine, in basic operations.
- `examples/charge_grid.rs`: P1's listening grids. `examples/cpu_cost.rs`: sixteen busy cells at
  96 kHz, as a share of real time.
- `tests/charge.rs`: the contracts below.

## Local Contracts

- **No charge, no sound.** A voice returns a raw sample within [-1, 1], its engines' mix weights
  summing to at most one, and the cell multiplies it by its charge (0…1). Every model keeps it;
  nothing else bounds the output, and nothing may be added that does (no limiter).
- **A transition never cuts the output.** A model change, a choke and a transport reset put the
  jump into the cell's one declick offset, clamped to ±1, which falls to exact zero within
  `DECLICK_S`; a cell's output is its live part plus its offset, so the declared peak is 2. No
  neighbour hears the offset and no Hunger spends it.
- **A choke latches** (a host's, a group's, a panic's): the cell takes no Flow, no Coupling and no
  bus, and holds no charge, until its own strike or touch press, or a reset. A latched cell's link is
  zero, so the edges refuse and charge stays conserved.
- **Rest is a fixed point.** A cell with no charge, no gate, no touch and no offset clears its engines
  (reseeding their noise, so a hit from rest is always the same hit) and changes nothing until fed;
  the pool snaps to full once a step stops moving it. So a kit at rest is skipped (only the LFOs run
  on) and the skip changes no sample. Leak is never infinite, so every unfed cell gets there. A pool
  of no capacity grants nothing, so no bus feeds through it. `Kit::lfo_may_feed` says whether an LFO
  could still wake a resting cell: some LFO values lift its bus, its Supply and the pool's capacity
  above zero together (routes summed as they sum; exact for one LFO, a bound for several).
- **Reproducible.** Everything advances per sample. Events at one sample apply in a fixed order —
  resets (a transport reset, then a panic), then releases and chokes, then strikes with the group
  chokes they make, then expression — each reduced so their order in the slice cannot matter (the
  harder strike, the larger tuning). A hit's Velocity and Random are staged before its Strike and
  controls are read; then every route is applied (`settle`) before the rest check. A reset fills the
  pool after the routes at its own phase are applied, and a reset or a panic lets go of the wheel and
  pressure. A host callback with no frames applies its events at once (`Kit::apply_now`).
  The cells read each other's previous sample and the pool grants every demand the same share, so no
  index decides anything. Every random stream is seeded from the kit's Seed, the cell and the
  stream's slot. After a transport reset the performance equals a render from rest once the window
  has passed.
- **No platform maths in `src/`** (the owner, 2026-11-02: the performance identical on every
  platform). Basic IEEE operations, `sqrt`, `floor`, the kit's `tan_approx` and `tanh_approx`, and
  [`fmath`](src/fmath.rs); `the_core_never_calls_the_platforms_maths_library` scans for the rest. The
  kit LFO's Sine is its RampUp phase through `fmath::sin_turns`. Tests and examples may use anything.
- **Temper 0 is steady**: no Flow, no Coupling, no jitter's effect; a hit from rest repeats exactly
  and leaves every other cell silent.
- **The model IDs and kind codes are permanent** (the brief's Appendix A).
- MSRV **1.87**; dependencies `mxm-drum-engine`, `mxm-dsp`, `mxm-modulation`, `mxm-part-routing` and
  `mxm-rng`, all framework-free kit crates.

## Work Guidance

- The constants in `voice.rs`, `cell.rs`, `pool.rs` and `kit.rs` are starting points. The owner's
  listening settles them; change one with the grid rendered before and after.
- `cpu_cost` measured 7.5–14.5% of one core for sixteen busy cells at 96 kHz (2026-11-02); a change
  that moves it says so.

## Verification

```bash
cargo test -p mxm-chaos-drums-dsp
cargo run -p mxm-chaos-drums-dsp --example charge_grid --release   # target/charge-grid/
cargo run -p mxm-chaos-drums-dsp --example cpu_cost --release
```

## Child DOX Index

None.
