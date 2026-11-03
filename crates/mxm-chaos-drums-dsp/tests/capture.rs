//! Resample's contracts: a capture is a function of the patch, a frozen strike plays its cell's
//! one-shot at its velocity's gain, a one-shot ends by fading, the mix stays live, and a frozen kit
//! comes to rest.

use mxm_chaos_drums_dsp::capture::{
    CAPTURE_GATE_S, FADE_SECONDS, KitCapture, MAX_CAPTURE_SECONDS, capture_kit, capture_patches,
};
use mxm_chaos_drums_dsp::{
    CELLS, CellParams, Event, EventKind, Kit, KitParams, ModelId, OutputFrame, velocity_curve,
};

const RATE: f32 = 16_000.0;

fn cells(models: &[(usize, ModelId)]) -> [CellParams; CELLS] {
    let mut cells = [CellParams::new(ModelId::NONE); CELLS];
    for &(cell, model) in models {
        cells[cell] = CellParams::new(model);
    }
    cells
}

fn strike(cell: usize, velocity: f32) -> EventKind {
    EventKind::Strike {
        cell,
        velocity,
        gate: false,
    }
}

/// `len` frames before Level, the events at their offsets.
fn render(kit: &mut Kit, events: &[(usize, EventKind)], len: usize) -> Vec<[f32; CELLS]> {
    let events: Vec<Event> = events
        .iter()
        .map(|&(offset, kind)| Event { offset, kind })
        .collect();
    let mut out = vec![[0.0; CELLS]; len];
    kit.process_raw(&events, &mut out);
    out
}

/// A kit of `cells` frozen on their own capture.
fn frozen(params: KitParams, cells: [CellParams; CELLS]) -> (Kit, KitCapture) {
    let capture = capture_kit(&capture_patches(params, cells), RATE);
    let mut kit = Kit::new(RATE, params, cells);
    assert!(kit.set_captures(Some(Box::new(capture.clone()))).is_none());
    assert!(kit.is_frozen());
    (kit, capture)
}

#[test]
fn a_capture_is_a_function_of_the_patch_and_not_of_the_mix() {
    let kit = KitParams::default();
    let base = cells(&[(0, ModelId::DRAINING_KICK), (3, ModelId::RAW_RING)]);
    let first = capture_kit(&capture_patches(kit, base), RATE);
    assert_eq!(
        first,
        capture_kit(&capture_patches(kit, base), RATE),
        "the same patch"
    );
    let mut mixed = base;
    mixed[0].level = 0.1;
    mixed[0].pan = -1.0;
    mixed[3].mute = true;
    mixed[3].output = 4;
    let master = KitParams { master: 0.2, ..kit };
    assert_eq!(
        first,
        capture_kit(&capture_patches(master, mixed), RATE),
        "the mix"
    );
    let mut tuned = base;
    tuned[0].coarse = 7;
    assert_ne!(
        first,
        capture_kit(&capture_patches(kit, tuned), RATE),
        "the pitch"
    );
    assert!(!first.cell(0).is_empty() && !first.cell(3).is_empty());
    assert!(
        first.cell(1).is_empty(),
        "a cell that sounds nothing captures nothing"
    );
    assert!(
        first.cell(0).len() > (CAPTURE_GATE_S * RATE) as usize,
        "the kick rings past its gate"
    );
}

#[test]
fn a_frozen_strike_plays_its_cells_one_shot_at_its_velocitys_gain() {
    let params = KitParams {
        dynamics: 0.4,
        ..KitParams::default()
    };
    let (mut kit, capture) = frozen(params, cells(&[(0, ModelId::DRAINING_KICK)]));
    let shot = capture.cell(0);
    let out = render(&mut kit, &[(0, strike(0, 1.0))], shot.len() + 10);
    for (n, frame) in out.iter().enumerate() {
        assert_eq!(frame[0], shot.get(n).copied().unwrap_or(0.0), "at {n}");
    }
    let gain = velocity_curve(0.5, params.dynamics);
    let out = render(&mut kit, &[(0, strike(0, 0.5))], shot.len() + 1);
    for (n, frame) in out.iter().enumerate().take(shot.len()) {
        assert_eq!(frame[0], shot[n] * gain, "at {n}");
    }
    assert!(kit.is_at_rest(), "a frozen kit rests after its one-shot");
}

#[test]
fn a_new_strike_a_choke_and_a_reset_fade_a_one_shot_and_a_panic_cuts_it() {
    let base = cells(&[(0, ModelId::RAW_RING)]);
    let fade = (FADE_SECONDS * RATE) as usize + 2;
    let (mut kit, capture) = frozen(KitParams::default(), base);
    let shot = capture.cell(0).to_vec();
    let at = shot.len() / 4;
    // A retrigger: the old one-shot under the new one, gone within the fade.
    let out = render(
        &mut kit,
        &[(0, strike(0, 1.0)), (at, strike(0, 1.0))],
        at + 2 * fade,
    );
    assert_ne!(out[at][0], shot[0], "the old one-shot sounds under the new");
    for n in at + fade..at + 2 * fade {
        assert_eq!(
            out[n][0],
            shot[n - at],
            "only the new one-shot after the fade, at {n}"
        );
    }
    for (stop, cut) in [
        (EventKind::Choke { cell: 0 }, false),
        (EventKind::TransportReset, false),
        (EventKind::Panic, true),
    ] {
        let (mut kit, _) = frozen(KitParams::default(), base);
        let out = render(&mut kit, &[(0, strike(0, 1.0)), (at, stop)], at + 2 * fade);
        let gone = if cut { at } else { at + fade };
        assert!(
            out[gone..].iter().all(|frame| frame[0] == 0.0),
            "{stop:?} ends the one-shot"
        );
        if !cut {
            assert!(out[at][0] != 0.0, "{stop:?} fades rather than cuts");
        }
    }
}

#[test]
fn a_strike_in_a_choke_group_ends_the_others_frozen_hits() {
    let mut base = cells(&[(10, ModelId::RAW_DUST), (11, ModelId::RAW_DUST)]);
    base[10].choke_group = 1;
    base[11].choke_group = 1;
    base[11].leak = 0.6;
    let (mut kit, _) = frozen(KitParams::default(), base);
    let fade = (FADE_SECONDS * RATE) as usize + 2;
    let out = render(
        &mut kit,
        &[(0, strike(11, 1.0)), (400, strike(10, 1.0))],
        400 + 2 * fade,
    );
    assert!(out[399][11] != 0.0, "the open hat sounds");
    assert!(
        out[400 + fade..].iter().all(|frame| frame[11] == 0.0),
        "the closed hat cuts it"
    );
}

#[test]
fn the_mix_stays_live_on_a_frozen_cell() {
    let base = cells(&[(0, ModelId::DRAINING_KICK)]);
    let (mut kit, _) = frozen(KitParams::default(), base);
    let mut quiet = base[0];
    quiet.mute = true;
    kit.set_cell(0, quiet);
    let mut frames = vec![OutputFrame::default(); 2_000];
    kit.process(
        &[Event {
            offset: 0,
            kind: strike(0, 1.0),
        }],
        &mut frames,
    );
    assert!(frames.iter().all(|frame| frame.main == [0.0; 2]), "muted");
    let mut loud = base[0];
    loud.output = 3;
    kit.set_cell(0, loud);
    let mut frames = vec![OutputFrame::default(); 2_000];
    kit.process(
        &[Event {
            offset: 0,
            kind: strike(0, 1.0),
        }],
        &mut frames,
    );
    // Output 3 is the third individual output.
    assert!(
        frames.iter().any(|frame| frame.individual[2] != 0.0),
        "on its own output"
    );
}

#[test]
fn engaging_resample_lets_a_live_tail_ring_and_any_block_size_renders_the_same() {
    let base = cells(&[(0, ModelId::DRAINING_KICK), (5, ModelId::RAW_FOLD)]);
    let capture = capture_kit(&capture_patches(KitParams::default(), base), RATE);
    let mut kit = Kit::new(RATE, KitParams::default(), base);
    let live = render(&mut kit, &[(0, strike(0, 1.0))], 200);
    assert!(live[199][0] != 0.0);
    kit.set_captures(Some(Box::new(capture.clone())));
    let after = render(&mut kit, &[], 10);
    assert!(after[0][0] != 0.0, "the live tail rings on");

    let events = [
        (0, strike(5, 0.7)),
        (90, strike(0, 1.0)),
        (130, strike(5, 1.0)),
    ];
    let mut whole = Kit::new(RATE, KitParams::default(), base);
    whole.set_captures(Some(Box::new(capture.clone())));
    let one = render(&mut whole, &events, 600);
    let mut blocks = Kit::new(RATE, KitParams::default(), base);
    blocks.set_captures(Some(Box::new(capture)));
    let mut out = Vec::new();
    for start in (0..600).step_by(7) {
        let here: Vec<(usize, EventKind)> = events
            .iter()
            .filter(|(at, _)| (start..start + 7).contains(at))
            .map(|&(at, kind)| (at - start, kind))
            .collect();
        out.extend(render(&mut blocks, &here, 7.min(600 - start)));
    }
    assert_eq!(one, out);
}

#[test]
fn a_tail_still_sounding_at_the_cap_is_faded_and_said() {
    let mut endless = CellParams::new(ModelId::RAW_RING);
    endless.threshold = -1.0;
    endless.hunger = 0.0;
    endless.leak = 1.0;
    let mut base = cells(&[]);
    base[2] = endless;
    let capture = capture_kit(&capture_patches(KitParams::default(), base), 1_000.0);
    assert!(capture.truncated(2));
    assert_eq!(
        capture.cell(2).len(),
        (MAX_CAPTURE_SECONDS * 1_000.0) as usize
    );
    let last = *capture.cell(2).last().unwrap();
    assert!(last.abs() < 1e-3, "faded to its end: {last}");
    assert!(!capture.truncated(0));
}
