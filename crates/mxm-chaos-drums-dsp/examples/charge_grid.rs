//! P1's listening grids: does charge breathe?
//!
//! For each model, at Temper 0 and at Temper 80%, one WAV file and a cue sheet beside it naming
//! every case and when it starts. Every case starts from rest.
//!
//! - **A**, the cell alone: velocity 40, 80 and 127 × gates of 1, 60, 250 and 1000 ms, at the
//!   default Supply.
//! - **B**, Supply 0, 30, 70 and 100%, velocity 100, a 600 ms gate.
//! - **C**, repetition: eight hits of 20 ms every 250, 120 and 60 ms, so charge stacks.
//! - **D**, the two cells linked: the model and its partner trading hits for two bars, then the
//!   partner held for a second and a half.
//!
//! ```text
//! cargo run -p mxm-chaos-drums-dsp --example charge_grid --release
//! ```
//!
//! writes `target/charge-grid/<model>-temper-<0|80>.wav` and `.txt`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use mxm_audio_file::Target;
use mxm_chaos_drums_dsp::{CELLS, CellParams, Event, EventKind, Kit, KitParams, ModelId};

const RATE: u32 = 48_000;
const BLOCK: usize = 256;
const GAP_S: f32 = 0.35;
const TAIL_S: f32 = 2.5;
const MIX: f32 = 0.7;

fn samples(seconds: f32) -> usize {
    (seconds * RATE as f32) as usize
}

struct Note {
    cell: usize,
    at: usize,
    gate: usize,
    velocity: u8,
}

fn note(cell: usize, at_s: f32, gate_ms: f32, velocity: u8) -> Note {
    Note {
        cell,
        at: samples(at_s),
        gate: samples(gate_ms / 1000.0).max(1),
        velocity,
    }
}

#[derive(Default)]
struct Take {
    audio: Vec<f32>,
    cues: String,
    peak: f32,
}

impl Take {
    /// One case from rest: the notes, then until the kit rests or the tail runs out, then a gap.
    fn case(&mut self, label: &str, kit: &mut Kit, notes: &[Note]) {
        kit.reset();
        let mut events: Vec<(usize, EventKind)> = Vec::new();
        for n in notes {
            let velocity = f32::from(n.velocity) / 127.0;
            events.push((
                n.at,
                EventKind::NoteOn {
                    cell: n.cell,
                    velocity,
                },
            ));
            events.push((n.at + n.gate, EventKind::NoteOff { cell: n.cell }));
        }
        events.sort_by_key(|(at, _)| *at);
        let last = events.last().map_or(0, |(at, _)| *at);
        let limit = last + samples(TAIL_S);

        let _ = writeln!(
            self.cues,
            "{:8.3} s  {label}",
            self.audio.len() as f32 / RATE as f32
        );
        let mut frames = vec![[0.0f32; CELLS]; BLOCK];
        let mut t = 0;
        let mut next = 0;
        loop {
            let mut block = Vec::new();
            while next < events.len() && events[next].0 < t + BLOCK {
                block.push(Event {
                    offset: events[next].0 - t,
                    kind: events[next].1,
                });
                next += 1;
            }
            kit.process(&block, &mut frames);
            for frame in &frames {
                let y = MIX * frame.iter().sum::<f32>();
                self.peak = self.peak.max(y.abs());
                self.audio.push(y);
            }
            t += BLOCK;
            if (t > last && kit.is_at_rest()) || t >= limit {
                break;
            }
        }
        self.audio.extend(std::iter::repeat_n(0.0, samples(GAP_S)));
    }
}

fn render(name: &str, model: ModelId, partner: ModelId, temper: f32, folder: &Path) {
    let alone = CellParams {
        link: 0.0,
        ..CellParams::new(model)
    };
    let kit_params = KitParams {
        temper,
        ..KitParams::default()
    };
    let mut kit = Kit::new(RATE as f32, kit_params, [alone, CellParams::new(partner)]);
    let mut take = Take::default();
    let percent = (temper * 100.0).round() as u32;
    let _ = writeln!(
        take.cues,
        "{name}, Temper {percent}%. Defaults: Strike 60%, Supply 30%, Hunger 50%, Leak {} ms.\n\
         A-C: the cell alone (Link 0). D: linked to its partner (cell 2).\n",
        alone.leak_ms
    );

    for velocity in [40u8, 80, 127] {
        for gate_ms in [1.0f32, 60.0, 250.0, 1000.0] {
            let label = format!("A  velocity {velocity:3}, gate {gate_ms:4} ms");
            take.case(&label, &mut kit, &[note(0, 0.0, gate_ms, velocity)]);
        }
    }
    for supply in [0.0f32, 0.3, 0.7, 1.0] {
        kit.set_cell(0, CellParams { supply, ..alone });
        let label = format!(
            "B  Supply {:3}%, velocity 100, gate 600 ms",
            (supply * 100.0) as u32
        );
        take.case(&label, &mut kit, &[note(0, 0.0, 600.0, 100)]);
    }
    kit.set_cell(0, alone);
    for every_ms in [250.0f32, 120.0, 60.0] {
        let hits: Vec<Note> = (0..8)
            .map(|i| note(0, i as f32 * every_ms / 1000.0, 20.0, 100))
            .collect();
        take.case(
            &format!("C  eight hits every {every_ms} ms"),
            &mut kit,
            &hits,
        );
    }

    kit.set_cell(0, CellParams::new(model));
    let mut pattern = Vec::new();
    for beat in 0..8 {
        let at = beat as f32 * 0.5;
        pattern.push(note(0, at, 30.0, 110));
        pattern.push(note(
            1,
            at + 0.25,
            30.0,
            if beat % 2 == 0 { 90 } else { 120 },
        ));
    }
    pattern.push(note(1, 4.0, 1500.0, 110));
    take.case(
        "D  linked: trading hits for two bars at 120 BPM, then cell 2 held 1.5 s",
        &mut kit,
        &pattern,
    );

    let path = folder.join(format!("{name}-temper-{percent}"));
    mxm_audio_file::write(
        path.with_extension("wav"),
        &take.audio,
        1,
        RATE,
        Target::WavFloat32,
    )
    .expect("write the WAV file");
    std::fs::write(path.with_extension("txt"), &take.cues).expect("write the cue sheet");
    println!(
        "{}  {:5.1} s, peak {:.2}",
        path.with_extension("wav").display(),
        take.audio.len() as f32 / RATE as f32,
        take.peak
    );
}

fn main() {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/charge-grid");
    std::fs::create_dir_all(&folder).expect("create the output folder");
    let models = [
        ("kick", ModelId::DRAINING_KICK, ModelId::WIRED_SNARE),
        ("snare", ModelId::WIRED_SNARE, ModelId::DRAINING_KICK),
        ("clap", ModelId::SPARK_CLAP, ModelId::DRAINING_KICK),
    ];
    for (name, model, partner) in models {
        for temper in [0.0, 0.8] {
            render(name, model, partner, temper, &folder);
        }
    }
}
