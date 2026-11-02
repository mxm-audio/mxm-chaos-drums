//! What sixteen busy cells cost: each model on every cell, every cell fed by a bus and struck, at
//! full Temper, Flow and Coupling, at 96 kHz. Prints the render time as a share of real time.
//!
//! ```text
//! cargo run -p mxm-chaos-drums-dsp --example cpu_cost --release
//! ```

use std::time::Instant;

use mxm_chaos_drums_dsp::{
    AVAILABLE_MODELS, Bus, CELLS, CellParams, Event, EventKind, Kit, KitParams, OutputFrame,
};

const RATE: f32 = 96_000.0;
const SECONDS: usize = 10;
const BLOCK: usize = 256;

fn main() {
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
        let mut kit = Kit::new(RATE, params, [cell; CELLS]);
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
            let events: &[Event] = if block % 100 == 0 { &strikes } else { &[] };
            kit.process(events, &mut frames);
        }
        let share = start.elapsed().as_secs_f64() / SECONDS as f64;
        println!("{:20} {:5.2}% of real time", spec.label, 100.0 * share);
    }
}
