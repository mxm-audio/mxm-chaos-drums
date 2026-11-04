//! What sixteen busy cells cost: each model on every cell, every cell fed by a bus and struck, at
//! full Temper, Flow and Coupling, at 96 kHz; then the same strikes on the same kit frozen by
//! Resample, which only reads its one-shots; then a kit at rest. Prints each render time as a share
//! of real time, and how long the capture took (once per engage, on its own thread).
//!
//! ```text
//! cargo run -p mxm-chaos-drums-dsp --example cpu_cost --release
//! ```

use std::time::Instant;

use mxm_chaos_drums_dsp::capture::{capture_kit, capture_patches};
use mxm_chaos_drums_dsp::{
    AVAILABLE_MODELS, Bus, CELLS, CellParams, Event, EventKind, Kit, KitParams, OutputFrame,
};

const RATE: f32 = 96_000.0;
const SECONDS: usize = 10;
const BLOCK: usize = 256;

/// The share of real time `SECONDS` of `kit` take, struck every hundred blocks if `struck`.
fn share(kit: &mut Kit, struck: bool) -> f64 {
    let strikes: Vec<Event> = (0..CELLS)
        .map(|c| Event {
            offset: c,
            kind: EventKind::Strike {
                cell: c,
                velocity: 1.0,
                gate: false,
            },
        })
        .collect();
    let mut frames = vec![OutputFrame::default(); BLOCK];
    let blocks = SECONDS * RATE as usize / BLOCK;
    let start = Instant::now();
    for block in 0..blocks {
        let events: &[Event] = if struck && block % 100 == 0 {
            &strikes
        } else {
            &[]
        };
        kit.process(events, &mut frames);
    }
    100.0 * start.elapsed().as_secs_f64() / SECONDS as f64
}

fn main() {
    println!("{:20} {:>7} {:>7} {:>9}", "", "live", "frozen", "capture");
    for spec in AVAILABLE_MODELS {
        let mut cell = CellParams::new(spec.id);
        cell.bus = Bus::A;
        cell.supply = 0.6;
        let params = KitParams {
            temper: 1.0,
            flow: 1.0,
            coupling: 1.0,
            bus: [0.8, 0.0],
            ..KitParams::default()
        };
        let cells = [cell; CELLS];
        let live = share(&mut Kit::new(RATE, params, cells), true);
        let start = Instant::now();
        let capture = capture_kit(&capture_patches(params, cells), RATE);
        let captured = start.elapsed().as_secs_f64();
        let mut kit = Kit::new(RATE, params, cells);
        kit.set_captures(Some(Box::new(capture)));
        let frozen = share(&mut kit, true);
        println!(
            "{:20} {:6.2}% {:6.2}% {:7.0} ms",
            spec.label,
            live,
            frozen,
            1000.0 * captured
        );
    }
    let resting = share(
        &mut Kit::new(
            RATE,
            KitParams::default(),
            [CellParams::new(AVAILABLE_MODELS[0].id); CELLS],
        ),
        false,
    );
    println!("{:20} {:6.2}%", "A kit at rest", resting);
}
