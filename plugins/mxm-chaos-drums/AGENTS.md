# AGENTS.md — plugins/mxm-chaos-drums

Parent: [`../AGENTS.md`](../AGENTS.md)

# Purpose

The nice-plug shell over `mxm-chaos-drums-dsp`: identity, the parameters, the host contract, note
ownership, the outputs, presets and the cells' names. **P3** (2026-11-02); the editor is P5, and
until it lands the plugin has none (`type Editor = ()`): a host's generic panel shows every
parameter. Resample and Export (mxm-drum-shell's `capture` and `pack`) follow the editor.

# Ownership

- `src/lib.rs`: identity, the two layouts, `activate`, `reset`, `process`, note ownership and the
  gates, the note names and the preset loader; its tests hold the note laws and the identity.
- `src/params.rs`: the 41 parameters a cell and the kit's, their IDs, ranges and readings, the
  Init kit, and the conversion to the DSP's settings.
- `src/routes.rs`: the route parameters, four a cell and four for the kit.
- `src/preset.rs`: the `mxm_preset::Instrument` binding: Init, sparse routes, instance settings.
- `src/slot_names.rs`: the cells' names (mxm-drum-shell's `DrumSlots`) and the host's note names.
- `control-map.json`: Master and two LFOs on existing roles.

# Local Contracts

- **Identity.** `CLAP_ID` `dk.mxm.mxm-chaos-drums`, from `plugin_name!`; `VST3_CLASS_ID` its
  FNV-1a hash written out, held by `identity::the_vst3_class_id_is_the_clap_ids_hash`. Permanent.
- **Keys.** Cell keys 36–51 strike cells 1–16; touch keys 52–67 touch them. Every channel plays
  the kit. A part a key, thirty-two, through `mxm-part-routing`'s ownership: a total order picks
  the owner; a hit is its own velocity; an older press's late release is ignored; a host's choke
  finds a released note's tail.
- **Gates.** A cell's gate stays open while its owner holds the note, and closes at its release,
  a choke, CC123 or a flood. A note-on released at its own sample strikes with no gate, though
  ownership would hand it one; a touch let go at its own sample touches nothing. CC120 is panic.
  Wheel and pressure are the kit's: the highest any channel holds. A pitch curve on a cell key
  tunes that cell until its next strike.
- **No `powf` before the performance.** Every parameter that reaches a cell's state is a linear
  amount with its curve in the DSP (`curves`), unsmoothed by nice-plug; Level, Pan and Master alone
  are skewed gain, smoothed in the DSP's mix. `SAMPLE_ACCURATE_AUTOMATION` is on, so a block's
  settings hold for all of it and the performance does not depend on the host's block size.
- **Transport.** A start of the host's transport is a transport reset at the block's first sample
  when Reset on play is on (its default, the owner, 2026-11-02).
- **Layouts.** Main plus sixteen mono outputs first; stereo only second, which MXM Player takes. A
  stereo-only host folds every cell's Output to the main pair.
- **Parameters.** 41 a cell (`cell_ids`), 32 for the kit (`KIT_IDS`): 688. Output is an instance
  setting no preset carries; Choke group is a kit setting presets carry. Routes are stored sparsely.
  *Deviation:* four route slots a cell, as the drum machine's, so `routing_checks` does not apply.
  IDs free during pre-alpha.
- **Init** is the defaults: the brief's kit on the drum machine's role map, every route Off, Temper
  0. A role whose drum P4 has not built yet plays a raw engine standing in for it (`params::INIT`).
- **Realtime.** The kit, its delay lines and the frame buffer are made in `activate`; `process`
  allocates nothing (one sample's events are a fixed array). At rest it reports `Normal` and runs
  none of the core; otherwise `KeepAlive`.

# Work Guidance

- A change to what the instrument does starts in the brief, then the DSP, then here.

# Verification

```bash
cargo test -p mxm-chaos-drums
cargo xtask bundle mxm-chaos-drums --release
clap-validator validate "target/bundled/mxm-chaos-drums.clap"   # 38 passed, 6 skipped (2026-11-02)
```

# Child DOX Index

None.
