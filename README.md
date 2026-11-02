# mxm-chaos-drums

An MXM instrument: an original sixteen-cell drum instrument that runs on charge. A note supplies
energy instead of firing a hit, the cells feed each other on a 4×4 grid and share one supply, and
one dial, Temper, takes the kit from steady to unpredictable, while every render stays exactly
reproducible.

Part of the MXM collection: every instrument, effect and tool lives in its own repository under
[github.com/mxm-audio](https://github.com/mxm-audio), built on
[mxm-kit](https://github.com/mxm-audio/mxm-kit).

**Status:** pre-alpha. [The brief](docs/briefs/mxm-chaos-drums.md) is the design; the DSP is at
its first spike, a kick, a snare and a clap on two linked cells, and there is no plugin yet.
Nothing is released.

## Hearing the spike

Rust 1.87 or newer.

```bash
cargo run -p mxm-chaos-drums-dsp --example charge_grid --release
cargo test
```

The example writes `target/charge-grid/`: for each model, at Temper 0 and at 80%, a WAV of gate
length × velocity, a Supply sweep, repetition rates and the two linked cells, with a cue sheet
beside it.

## Licence

GPL-3.0-or-later — see [`LICENSE`](LICENSE) and [`NOTICE.md`](NOTICE.md). The MXM
name and logo are trademarks; [`TRADEMARKS.md`](TRADEMARKS.md) says how they may be used.

Contributions are welcome: see [`CONTRIBUTING.md`](CONTRIBUTING.md). How the repository is
organised, and the rules each part keeps, are in [`AGENTS.md`](AGENTS.md).
