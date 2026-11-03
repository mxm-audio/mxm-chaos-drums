//! Resample's contracts: a capture is a function of the patch, a frozen strike plays its cell's
//! one-shot at its velocity's gain, a one-shot ends by fading, the mix stays live, and a frozen kit
//! comes to rest.

use mxm_chaos_drums_dsp::capture::{
    CAPTURE_GATE_S, FADE_SECONDS, KitCapture, KitCaptureInProgress, MAX_CAPTURE_SECONDS,
    capture_kit, capture_patches,
};
use mxm_chaos_drums_dsp::{
    Bus, CELLS, CellParams, CellTarget, Event, EventKind, Kit, KitParams, KitTarget, ModelId,
    OutputFrame, Route, Source, velocity_curve,
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

/// The capture leaves out what stays live while frozen (the mix's routes, Dynamics) and what would
/// disturb the isolated hit (a bus and the routes onto one, a choke group, whose strike would latch
/// a coupled neighbour out of the lattice): neither changes a one-shot, so neither is a new capture.
#[test]
fn what_stays_live_or_would_disturb_the_hit_is_not_part_of_a_capture() {
    let kit = KitParams {
        temper: 0.6,
        coupling: 1.0,
        ..KitParams::default()
    };
    let base = cells(&[
        (10, ModelId::LEAKING_CLOSED_HAT),
        (11, ModelId::LEAKING_OPEN_HAT),
    ]);
    let mut played = base;
    played[10].choke_group = 1;
    played[11].choke_group = 1;
    played[10].bus = Bus::A;
    played[11].routes[0] = Route {
        source: Source::Velocity,
        target: CellTarget::Pan,
        amount: 1.0,
    };
    played[10].routes[1] = Route {
        source: Source::Random,
        target: CellTarget::Level,
        amount: -1.0,
    };
    let mut routed = KitParams {
        dynamics: -0.8,
        ..kit
    };
    routed.routes[0] = Route {
        source: Source::Lfo1,
        target: KitTarget::BusA,
        amount: 1.0,
    };
    assert_eq!(capture_patches(routed, played), capture_patches(kit, base));
    assert_eq!(
        capture_kit(&capture_patches(routed, played), RATE),
        capture_kit(&capture_patches(kit, base), RATE),
        "the hats' group latches neither out of the coupling"
    );
    let mut bent = base;
    bent[10].routes[0] = Route {
        source: Source::Random,
        target: CellTarget::Pitch,
        amount: 1.0,
    };
    assert_ne!(
        capture_patches(kit, bent),
        capture_patches(kit, base),
        "a route onto the sound is"
    );
}

/// A capture renders at most the frames it is asked for, its gates' included, so the thread doing it
/// can stop within a chunk; and in any steps it renders the same samples.
#[test]
fn a_capture_renders_no_more_than_it_is_asked_and_the_same_samples() {
    let rate = 1_000.0;
    let patches = capture_patches(
        KitParams::default(),
        cells(&[(0, ModelId::DAMPED_CLAVE), (5, ModelId::GRAIN_MARACA)]),
    );
    let mut progress = KitCaptureInProgress::begin(&patches, rate);
    let mut calls = 1;
    while !progress.render(1) {
        calls += 1;
    }
    let whole = capture_kit(&patches, rate);
    let kept: usize = (0..CELLS).map(|cell| whole.cell(cell).len()).sum();
    let gates = CELLS * (CAPTURE_GATE_S * rate) as usize;
    assert!(
        calls >= kept.max(gates),
        "{calls} calls rendered {kept} frames kept and {gates} of gates"
    );
    assert_eq!(progress.finish(), whole);
}

/// A frozen hit stages its own Velocity and Random, as a live one does, for the routes onto the mix.
#[test]
fn a_frozen_hit_moves_the_routes_onto_its_mix_hit_by_hit() {
    let plain = cells(&[(0, ModelId::DRAINING_KICK)]);
    let peaks = |cells: [CellParams; CELLS], velocities: &[f32]| -> Vec<f32> {
        let (mut kit, _) = frozen(KitParams::default(), cells);
        velocities
            .iter()
            .map(|&velocity| {
                let mut frames = vec![OutputFrame::default(); 4_000];
                kit.process(
                    &[Event {
                        offset: 0,
                        kind: strike(0, velocity),
                    }],
                    &mut frames,
                );
                frames
                    .iter()
                    .map(|frame| frame.main[0].abs().max(frame.main[1].abs()))
                    .fold(0.0, f32::max)
            })
            .collect()
    };
    let mut by_velocity = plain;
    by_velocity[0].routes[0] = Route {
        source: Source::Velocity,
        target: CellTarget::Level,
        amount: 1.0,
    };
    assert!(
        peaks(by_velocity, &[0.5])[0] < peaks(plain, &[0.5])[0],
        "Velocity onto Level"
    );
    let mut by_random = plain;
    by_random[0].routes[0] = Route {
        source: Source::Random,
        target: CellTarget::Level,
        amount: 1.0,
    };
    let hits = peaks(by_random, &[1.0, 1.0, 1.0]);
    assert!(
        hits[0] != hits[1] || hits[1] != hits[2],
        "Random onto Level, hit by hit: {hits:?}"
    );
}

/// Frozen, the buses feed no live cell, whether set or routed, so the kit rests; unfrozen, a bus
/// feeds again at once. A note held as Resample engages lets go, and its tail decays to rest.
#[test]
fn a_frozen_kit_feeds_no_bus_and_comes_to_rest() {
    for routed in [false, true] {
        let mut params = KitParams {
            bus: [if routed { 0.0 } else { 0.5 }, 0.0],
            ..KitParams::default()
        };
        if routed {
            params.routes[0] = Route {
                source: Source::Lfo1,
                target: KitTarget::BusA,
                amount: 1.0,
            };
        }
        let mut droning = cells(&[(0, ModelId::RAW_RING)]);
        droning[0].bus = Bus::A;
        let (mut kit, _) = frozen(params, droning);
        let out = render(&mut kit, &[], 16_000);
        assert!(
            out.iter().all(|frame| *frame == [0.0; CELLS]),
            "no bus feeds a frozen kit (routed: {routed})"
        );
        assert!(kit.is_at_rest(), "routed: {routed}");
        if !routed {
            kit.set_captures(None);
            assert!(!kit.is_at_rest(), "unfrozen, the bus feeds again at once");
            let out = render(&mut kit, &[], 16_000);
            assert!(out.iter().any(|frame| frame[0] != 0.0));
        }
    }
    let base = cells(&[(0, ModelId::RAW_RING)]);
    let capture = capture_kit(&capture_patches(KitParams::default(), base), RATE);
    let mut kit = Kit::new(RATE, KitParams::default(), base);
    let held = EventKind::Strike {
        cell: 0,
        velocity: 1.0,
        gate: true,
    };
    render(&mut kit, &[(0, held)], 2_000);
    kit.set_captures(Some(Box::new(capture)));
    let mut seconds = 0;
    while !kit.is_at_rest() {
        render(&mut kit, &[], 16_000);
        seconds += 1;
        assert!(seconds < 60, "a held note keeps a frozen kit awake");
    }
}

/// An LFO that only a live route reads (the mix, a bus) moves no captured sample, so editing it is
/// not a new capture; once a route onto the sound reads it, it is.
#[test]
fn an_lfo_only_the_live_mix_reads_is_not_part_of_a_capture() {
    let kit = KitParams::default();
    let mut mixed = cells(&[(0, ModelId::DRAINING_KICK)]);
    mixed[0].routes[0] = Route {
        source: Source::Lfo1,
        target: CellTarget::Level,
        amount: 1.0,
    };
    let mut retuned = kit;
    retuned.lfo_rate_hz[0] = 9.0;
    retuned.lfo_shape[0] = mxm_chaos_drums_dsp::LfoShape::Square;
    assert_eq!(
        capture_patches(retuned, mixed),
        capture_patches(kit, mixed),
        "read only by the mix"
    );
    let mut bent = mixed;
    bent[0].routes[1] = Route {
        source: Source::Lfo1,
        target: CellTarget::Pitch,
        amount: 0.5,
    };
    assert_ne!(
        capture_patches(retuned, bent),
        capture_patches(kit, bent),
        "read by the sound"
    );
}

/// A capture's Velocity (a velocity-1 hit's, and an unstruck neighbour's), Wheel and Pressure are
/// exactly zero, so a route from them moves no captured sample: rendered with and without them, the
/// one-shots are the same, bit for bit. So they are no part of the key either.
#[test]
fn routes_from_a_captures_zero_sources_move_no_captured_sample() {
    let kit = KitParams {
        temper: 0.6,
        coupling: 1.0,
        ..KitParams::default()
    };
    let base = cells(&[(5, ModelId::RAW_RING), (6, ModelId::SLAPPED_CONGA)]);
    let route = |source, target| Route {
        source,
        target,
        amount: 0.8,
    };
    let mut routed = base;
    routed[5].routes = [
        route(Source::Velocity, CellTarget::Pitch),
        route(Source::Wheel, CellTarget::Supply),
        route(Source::Pressure, CellTarget::Link),
        route(Source::Velocity, CellTarget::Threshold),
    ];
    routed[6].routes = [
        route(Source::Wheel, CellTarget::Control(2)),
        route(Source::Pressure, CellTarget::Strike),
        route(Source::Velocity, CellTarget::Link),
        route(Source::Wheel, CellTarget::Pitch),
    ];
    let mut routed_kit = kit;
    routed_kit.routes[0] = Route {
        source: Source::Wheel,
        target: KitTarget::Temper,
        amount: 1.0,
    };
    routed_kit.routes[1] = Route {
        source: Source::Pressure,
        target: KitTarget::Coupling,
        amount: -1.0,
    };
    assert_eq!(
        capture_kit(&(routed_kit, routed), RATE),
        capture_kit(&(kit, base), RATE),
        "rendered"
    );
    assert_eq!(
        capture_patches(routed_kit, routed),
        capture_patches(kit, base),
        "the key"
    );
}

/// Strikes faster than a fade never jump: a one-shot a third strike cuts inside its fade goes on as
/// a falling tail from where it was, so no sample steps further than the shots' own movement.
#[test]
fn strikes_faster_than_a_fade_never_jump() {
    let base = cells(&[(0, ModelId::RAW_RING)]);
    let (mut kit, capture) = frozen(KitParams::default(), base);
    let shot = capture.cell(0);
    // The first one-shot at its loudest when the next two arrive, a sample apart.
    let at = (300..shot.len().min(2_000))
        .max_by(|&a, &b| shot[a].abs().total_cmp(&shot[b].abs()))
        .expect("a long shot");
    let moves = shot
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(shot[0].abs(), f32::max);
    let out = render(
        &mut kit,
        &[
            (0, strike(0, 1.0)),
            (at, strike(0, 1.0)),
            (at + 1, strike(0, 1.0)),
        ],
        at + 400,
    );
    let jump = out
        .windows(2)
        .map(|pair| (pair[1][0] - pair[0][0]).abs())
        .fold(0.0, f32::max);
    let step = 1.0 / (FADE_SECONDS * RATE);
    assert!(
        jump <= 3.0 * (moves + step) + 1e-6,
        "a step of {jump} where the shots move {moves} (the cut one-shot stood at {})",
        shot[at]
    );
    let mut seconds = 0;
    while !kit.is_at_rest() {
        render(&mut kit, &[], 16_000);
        seconds += 1;
        assert!(seconds < 60, "the tail never ends");
    }
}

/// Any Solo isolates the soloed cells, a muted one too: Mute wins for its own cell only.
#[test]
fn a_muted_solo_still_silences_the_rest() {
    let mut mixed = cells(&[(0, ModelId::DRAINING_KICK), (1, ModelId::WIRED_SNARE)]);
    mixed[0].solo = true;
    mixed[0].mute = true;
    let mut kit = Kit::new(RATE, KitParams::default(), mixed);
    let mut frames = vec![OutputFrame::default(); 2_000];
    kit.process(
        &[
            Event {
                offset: 0,
                kind: strike(0, 1.0),
            },
            Event {
                offset: 0,
                kind: strike(1, 1.0),
            },
        ],
        &mut frames,
    );
    assert!(frames.iter().all(|frame| frame.main == [0.0; 2]));
    mixed[0].solo = false;
    kit.set_cell(0, mixed[0]);
    let mut frames = vec![OutputFrame::default(); 2_000];
    kit.process(
        &[Event {
            offset: 0,
            kind: strike(1, 1.0),
        }],
        &mut frames,
    );
    assert!(
        frames.iter().any(|frame| frame.main != [0.0; 2]),
        "no Solo: the snare is heard"
    );
}
