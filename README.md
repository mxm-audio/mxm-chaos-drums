# mxm-chaos-drums

An MXM instrument: an original sixteen-cell drum instrument that runs on charge. A note supplies
energy instead of firing a hit, the cells feed each other on a 4×4 grid and share one supply, and
one dial, Temper, takes the kit from steady to unpredictable, while every render stays exactly
reproducible.

Part of the MXM collection: every instrument, effect and tool lives in its own repository under
[github.com/mxm-audio](https://github.com/mxm-audio), built on
[mxm-kit](https://github.com/mxm-audio/mxm-kit).

**Status:** pre-alpha. [The brief](docs/briefs/mxm-chaos-drums.md) is the design. The plugin
plays sixteen cells with all five engines, a kick, a snare, a clap and the five raw models, through
its editor; the rest of the drums are next. Nothing is released.

## Building

Rust 1.95 or newer. On Linux, install ALSA, JACK, X11, xkbcommon and a GL loader first.

```bash
cargo xtask bundle mxm-chaos-drums --release   # -> target/bundled/mxm-chaos-drums.clap and .vst3
cargo test
cargo run -p mxm-chaos-drums-dsp --example charge_grid --release
```

The bundle installs itself, CLAP and VST®3, in the standard plugin folders. VST is a registered
trademark of Steinberg Media Technologies GmbH.

The example writes `target/charge-grid/`: for each model, at Temper 0 and at 80%, a WAV of gate
length × velocity, a Supply sweep, repetition rates and the two linked cells, with a cue sheet
beside it.

## Licence

GPL-3.0-or-later — see [`LICENSE`](LICENSE) and [`NOTICE.md`](NOTICE.md). The MXM
name and logo are trademarks; [`TRADEMARKS.md`](TRADEMARKS.md) says how they may be used.

Contributions are welcome: see [`CONTRIBUTING.md`](CONTRIBUTING.md). How the repository is
organised, and the rules each part keeps, are in [`AGENTS.md`](AGENTS.md).
