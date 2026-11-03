//! How long Resample's capture takes: a tame kit, and the worst case, every cell sustaining itself on
//! a wild grid so each one-shot runs to the cap.
//!
//! `cargo run -p mxm-chaos-drums-dsp --example capture_cost --release`

use std::time::Instant;

use mxm_chaos_drums_dsp::capture::{capture_kit, capture_patches};
use mxm_chaos_drums_dsp::{CELLS, CellParams, KitParams, ModelId};

fn main() {
    let rate = 48_000.0;
    let models = [
        ModelId::DRAINING_KICK,
        ModelId::WIRED_SNARE,
        ModelId::RAW_RING,
        ModelId::RAW_FOLD,
        ModelId::RAW_LOOP,
        ModelId::RAW_SPARK,
        ModelId::SPARK_CLAP,
        ModelId::RAW_DUST,
    ];
    let tame: [CellParams; CELLS] = std::array::from_fn(|i| CellParams::new(models[i % 8]));
    let wild_cells = tame.map(|cell| CellParams {
        threshold: -1.0,
        hunger: 0.0,
        leak: 1.0,
        ..cell
    });
    let wild = KitParams {
        temper: 1.0,
        flow: 1.0,
        coupling: 1.0,
        ..KitParams::default()
    };
    for (name, kit, cells) in [
        ("tame", KitParams::default(), tame),
        ("worst", wild, wild_cells),
    ] {
        let start = Instant::now();
        let capture = capture_kit(&capture_patches(kit, cells), rate);
        let seconds: f32 = (0..CELLS)
            .map(|c| capture.cell(c).len() as f32 / rate)
            .sum();
        println!(
            "{name:>5}: {:>6.2} s to capture {seconds:>6.1} s of one-shots, {} truncated",
            start.elapsed().as_secs_f32(),
            (0..CELLS).filter(|&c| capture.truncated(c)).count()
        );
    }
}
