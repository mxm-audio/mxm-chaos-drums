# AGENTS.md — mxm-chaos-drums

DOX rail for this repository. Project-wide instructions, durable workflow rules, and the
top-level Child DOX Index.

---

# DOX framework

- DOX is a highly performant AGENTS.md hierarchy installed here
- Agents must follow DOX instructions across any edits

## Core Contract

- AGENTS.md files are binding work contracts for their subtrees
- Work products, source materials, instructions, records, assets, and durable docs must stay
  understandable from the nearest applicable AGENTS.md plus every parent AGENTS.md above it

## Read Before Editing

1. Read the root AGENTS.md
2. Identify every file or folder you expect to touch
3. Walk from the repository root to each target path
4. Read every AGENTS.md found along each route
5. If a parent AGENTS.md lists a child AGENTS.md whose scope contains the path, read that child and
   continue from there
6. Use the nearest AGENTS.md as the local contract and parent docs for repo-wide rules
7. If docs conflict, the closer doc controls local work details, but no child doc may weaken DOX

Do not rely on memory. Re-read the applicable DOX chain in the current session before editing.

## Update After Editing

Every meaningful change requires a DOX pass before the task is done.

Update the closest owning AGENTS.md when a change affects:

- purpose, scope, ownership, or responsibilities
- durable structure, contracts, workflows, or operating rules
- required inputs, outputs, permissions, constraints, side effects, or artifacts
- user preferences about behavior, communication, process, organization, or quality
- AGENTS.md creation, deletion, move, rename, or index contents

Update parent docs when parent-level structure, ownership, workflow, or child index changes. Update
child docs when parent changes alter local rules. Correct stale or contradictory text immediately, and move its history to `NOTES.md` rather than deleting it; a discarded idea or replaced design is deleted outright, not archived: git keeps it (the owner, 2026-10-08).
Small edits that do not change behavior or contracts may leave docs unchanged, but the DOX pass
still must happen.

## Hierarchy

- Root AGENTS.md is the DOX rail
- Child AGENTS.md files own domain-specific instructions and their own Child DOX Index
- Each parent explains what its direct children cover and what stays owned by the parent
- The closer a doc is to the work, the more specific and practical it must be

## Child Doc Shape

- Create a child AGENTS.md when a folder becomes a durable boundary with its own purpose, rules,
  responsibilities, workflow, materials, or quality standards
- Work Guidance must reflect current project standards or user instructions; leave it empty if
  there are none yet
- Verification must reflect an existing check; leave it empty until one exists

Default section order: Purpose · Ownership · Local Contracts · Work Guidance · Verification ·
Child DOX Index

## Style

- Keep docs concise, current, and operational
- Document stable contracts, not diary entries
- **A value the code holds is named, not copied** (the owner, 2026-09-24: *"Why are you writing the
  opening pages size in such detail. Is that not already described in the code?"*). A size a test
  derives — an opening size, a minimum, a card floor — or a constant the code declares is stated in
  DOX as its rule, its constant and the test that holds it, never its number: a copied number goes
  stale the day the code moves. Two exceptions: the design system states its own tokens and
  rules, because it is the normative source the code implements; and a plan's revision history
  records what was measured when — a dated record, not the current value
- Put broad rules in parent docs and concrete details in child docs
- Prefer direct bullets with explicit names
- Do not duplicate rules across many files unless each scope needs a local version
- Keep AGENTS.md current: correct a stale note and move its history to `NOTES.md` instead of explaining it here;
  a discarded idea is deleted, never kept as history
- Trim obvious statements, repeated rules, misplaced detail, and warnings for risks that no longer
  exist

## Closeout

1. Re-check changed paths against the DOX chain
2. Update nearest owning docs and any affected parents or children
3. Refresh every affected Child DOX Index
4. Correct stale or contradictory text; move the reasoning still in force to `NOTES.md`, and delete a
   discarded idea outright
5. Run existing verification when relevant
6. Report any docs intentionally left unchanged and why

---

---

# Purpose

**mxm-chaos-drums** is an MXM instrument: an original sixteen-cell drum instrument that runs on
charge. A note supplies energy instead of firing a hit; cells spend it to sound, pass it to their
neighbours on a 4×4 grid and share one supply, so it is unpredictable to the ear and exactly
reproducible underneath. It copies no machine.

It is one of the MXM products, each in its own repository under
[github.com/mxm-audio](https://github.com/mxm-audio), built on the MIT-licensed
[mxm-kit](https://github.com/mxm-audio/mxm-kit). **Private until it is ready** (the owner,
2026-11-02). Its plan is the owner's and private; this repository holds the brief and the product.

Reference-quality open source: clarity beats cleverness, and every nontrivial algorithm names the
technique or paper it comes from.

# Ownership

- `docs/briefs/mxm-chaos-drums.md`: the design system §14 brief, the parameter surface, the Init
  kit and the model catalogue. **Written first; the owner approves it before any code** (P0).
- Root owns `Cargo.toml`, `Cargo.lock`, `LICENSE`, `NOTICE.md`, `TRADEMARKS.md`, `README.md`,
  `CONTRIBUTING.md`, `.gitignore`, `.cargo/`, `.github/`, `bundler.toml` and `xtask/`. Each folder
  with an `AGENTS.md` owns its contents; the index is below.

**Dependencies follow each repository's `main`, and `Cargo.lock` pins the exact commit**, as in
every MXM repository: the kit from mxm-kit's `main`, nice-plug and egui-baseview from their MXM forks
(`[patch.crates-io]`).

**The phases**, each landed and pushed on its own, each leaving a working state: P0 this repository
and the brief (approved 2026-11-02); P1 a DSP spike, which the owner heard as good starting points
(2026-11-02: "I think I need an interface to explore to test it further, so lets halt my tests until
the first version is up"); P2 the DSP crate, P3 the plugin (its Resample and Export 2026-11-03),
**P5 the editor** (landed 2026-11-02: the first version the owner explores) and the fifty factory
kits (2026-11-03, ahead of P4); then P4 the rest of the catalogue, played through the editor a group
at a time (2026-11-03: the drums, the hats and cymbals and the hand percussion, all twenty-five
models); P6 the owner's listening to the kits, and the release gates.

# Local Contracts

These bind every crate added here; each crate's own `AGENTS.md` restates what it carries out.

- **The charge law.** A live cell's output amplitude is bounded by a function of its own charge:
  no charge, no sound. With the charge ceiling it is the stability argument, so there is no output
  limiter. A transition (a model change, a choke, a reset) never cuts the output: the jump goes into
  one declick offset per cell that decays to exact zero and never feeds the grid.
- **A model** is a fixed network of at most four of the five engines on its cell's one charge, mix
  weights summing to at most one. Model IDs are permanent and append-only (the brief's Appendix A).
- **Reproducible.** State is a function of the event stream since the last reset; any block size
  renders the same samples; events at one sample apply in a fixed order whatever the host's.
- **The same performance on every platform** (the owner, 2026-11-02). Everything that can enter a
  cell's state uses basic IEEE operations and basic-operation approximations, never the platform's
  `sin`, `exp`, `powf`, `ln` or `cbrt`: chaos turns a one-ulp difference into a different
  performance. So this repository's golden digests are pinned on every platform, stricter than the
  workspace's Windows-only pins; Level, Pan and Master act after the pinned output and keep the
  collection's skewed gain.
- **No entropy.** Unpredictability comes from sensitivity, history and coupling, and every random
  stream is seeded by the patch's Seed.
- **Doing nothing costs nothing.** Leak never reaches zero and a cell snaps to exact rest below a
  floor, so an unfed kit comes to rest; at rest it runs none of its core and reports a status the
  host can sleep on.
- **MIDI in, audio out:** no audio input (the owner, 2026-11-02), no internal sequencer, no built-in
  effects in the first version.
- **No machine's name** in a label, a preset or a doc of this repository.

## Windows, Linux and macOS — all three, always

**An absolute requirement.** Everything here runs on all three; a change that works on one and
breaks another is a broken change. CI builds and tests on all three when started by hand.

- **Anything platform-specific is `cfg`-gated with every arm implemented**, never one arm and a
  silent nothing elsewhere.
- **A dependency that does not support all three cannot be taken**, whatever else it offers.

## MSRV is per crate

| Crate | MSRV | Why |
|---|---|---|
| `crates/mxm-chaos-drums-dsp` | **1.87** | Framework-free; its runtime dependencies, `mxm-drum-engine`, `mxm-dsp`, `mxm-modulation`, `mxm-part-routing` and `mxm-rng`, are kit crates at this floor. Declared, not yet verified on 1.87 |

| `plugins/mxm-chaos-drums` | **1.95** | It pulls in egui through mxm-drum-shell and mxm-params, and its editor will; depends on the DSP crate, which stays at 1.87 |

The DSP crate never inherits the plugin's floor.

## Licensing

**GPL-3.0-or-later** (`LICENSE`). `NOTICE.md` lists the third-party code in its builds. The MXM
name and logo are not covered by the licence: see `TRADEMARKS.md`. Check the licence before porting
any algorithm, and record source and licence in a comment at the top of the file. Cite techniques
even when the implementation is original.

# Work Guidance

- Answer a design question from the brief first; change the brief before the code when they differ.

# Verification

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test
cargo xtask bundle mxm-chaos-drums --release      # -> target/bundled/, installed for this user
clap-validator validate "target/bundled/mxm-chaos-drums.clap"
```

CI runs the first three on Windows, macOS and Linux, and bundles, only when started by hand. Before
a push, run them on Windows; Linux and macOS are checked later, together (the workspace's rule).

# Child DOX Index

| Doc | Scope |
|---|---|
| [`plugins/AGENTS.md`](plugins/AGENTS.md) | The collection's plugin conventions as this repository keeps them, and the plugin's own `AGENTS.md` |
| [`crates/mxm-chaos-drums-dsp/AGENTS.md`](crates/mxm-chaos-drums-dsp/AGENTS.md) | The sound as plain Rust: charge and its law, the pool and buses, the five engines, the models, sixteen cells on the torus, touch, the choke latch, the declick, routes, outputs and the one-shot |

`docs/` is owned here.
