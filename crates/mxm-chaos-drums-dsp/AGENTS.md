# AGENTS.md — crates/mxm-chaos-drums-dsp

## Purpose

The instrument's whole sound as plain Rust: charge, the pool, the engines, the models and the
kit that links the cells. No framework types, no GUI, no allocation once a kit is built. The
crate's docs (`src/lib.rs`) explain the design; the brief (`docs/briefs/mxm-chaos-drums.md`) is
what it implements.

**This is P1, the spike** (2026-11-02): two linked cells, three engines (Ring, Spark, Dust) and three
models (Draining kick, Wired snare, Spark clap), so the owner can hear whether charge breathes
before the grid exists. P2 adds Fold and Loop, the raw models, sixteen cells on the 4×4 torus,
Touch, the buses, choke groups and their latch, the declick offset, the modulation routing and the
capture renderer.

## Ownership

- `src/cell.rs`: one cell and the charge law: Strike, Supply, Hunger, Leak, the floor, rest.
- `src/pool.rs`: the shared supply, divided among every demand at once.
- `src/kit.rs`: the cells, Flow and Coupling between them, events and their order, rest skipping.
- `src/voice.rs`: each model's engine network and its constants.
- `src/engine/`: Ring (van der Pol), Spark (leaky integrate-and-fire), Dust (velvet noise) and the
  band they ring through.
- `src/model.rs`: the permanent model IDs, the twenty-five kinds and the available models.
- `src/fmath.rs`: `exp2` and semitones in basic operations.
- `examples/charge_grid.rs`: P1's listening grids.
- `tests/charge.rs`: the contracts below.

## Local Contracts

- **No charge, no sound.** A voice returns a raw sample within [-1, 1], its engines' mix weights
  summing to at most one, and the cell multiplies it by its charge (0…1). Every model keeps it;
  nothing else bounds the output, and nothing may be added that does (no limiter).
- **Rest is a fixed point.** A cell with no charge and no gate clears its engines (reseeding their
  noise, so a hit from rest is always the same hit) and changes nothing more until fed; the pool snaps
  to full once a step stops moving it. So a kit at rest is skipped and the skip changes no sample.
  Leak is never infinite, so every unfed cell gets there.
- **Reproducible.** Everything advances per sample. Events at one sample apply as releases, then
  strikes (the harder kept per cell), whatever their order. The cells read each other's previous
  sample and the pool grants every demand the same share, so no index decides anything. Every random
  stream is seeded from the kit's Seed, the cell and the stream's slot.
- **No platform maths in `src/`** (the owner, 2026-11-02: the performance identical on every
  platform). Basic IEEE operations, `sqrt`, `floor`, the kit's `tan_approx` and `tanh_approx`, and
  [`fmath`](src/fmath.rs); `the_core_never_calls_the_platforms_maths_library` scans for the rest.
  Tests and examples may use anything.
- **Temper 0 is steady**: no Flow, no Coupling, no jitter's effect; a hit from rest repeats exactly
  and leaves its neighbour silent.
- **The model IDs and kind codes are permanent** (the brief's Appendix A).
- MSRV **1.87**; dependencies `mxm-dsp` and `mxm-rng`.

## Work Guidance

- The constants in `voice.rs`, `cell.rs` and `pool.rs` are P1's starting points. The owner's
  listening settles them; change one with the grid rendered before and after.

## Verification

```bash
cargo test -p mxm-chaos-drums-dsp
cargo run -p mxm-chaos-drums-dsp --example charge_grid --release   # target/charge-grid/
```

## Child DOX Index

None.
