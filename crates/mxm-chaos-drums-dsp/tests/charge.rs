//! The crate's contracts: reproducible, bounded, the charge law, Temper 0's steadiness, the latch,
//! choke groups, the buses, the transport reset, touch, outputs, rest, the pool, the catalogue and the
//! platform rule.

use mxm_chaos_drums_dsp::fmath;
use mxm_chaos_drums_dsp::{
    AVAILABLE_MODELS, Bus, CELLS, CellParams, CellTarget, DECLICK_S, Event, EventKind, Kind, Kit,
    KitParams, KitTarget, ModelId, OutputFrame, Route, Source, neighbours,
};
use mxm_rng::Xorshift32;

const RATE: f32 = 48_000.0;

fn strike(at: usize, cell: usize, velocity: f32) -> (usize, EventKind) {
    (
        at,
        EventKind::Strike {
            cell,
            velocity,
            gate: true,
        },
    )
}

fn release(at: usize, cell: usize) -> (usize, EventKind) {
    (at, EventKind::Release { cell })
}

/// A kit with `models` at their cells and the rest empty.
fn cells(models: &[(usize, ModelId)]) -> [CellParams; CELLS] {
    let mut cells = [CellParams::new(ModelId::NONE); CELLS];
    for &(cell, model) in models {
        cells[cell] = CellParams::new(model);
    }
    cells
}

fn wild() -> KitParams {
    KitParams {
        temper: 1.0,
        flow: 1.0,
        coupling: 1.0,
        ..KitParams::default()
    }
}

fn steady() -> KitParams {
    KitParams {
        temper: 0.0,
        flow: 1.0,
        coupling: 1.0,
        ..KitParams::default()
    }
}

/// Render `len` samples in blocks of `block`, the events (in time order) at their samples.
fn render(
    kit: &mut Kit,
    events: &[(usize, EventKind)],
    len: usize,
    block: usize,
) -> Vec<[f32; CELLS]> {
    let mut out = vec![[0.0; CELLS]; len];
    let mut next = 0;
    for (start, chunk) in (0..len).step_by(block).zip(out.chunks_mut(block)) {
        let mut here = Vec::new();
        while next < events.len() && events[next].0 < start + chunk.len() {
            here.push(Event {
                offset: events[next].0 - start,
                kind: events[next].1,
            });
            next += 1;
        }
        kit.process_raw(&here, chunk);
    }
    out
}

fn sorted(mut events: Vec<(usize, EventKind)>) -> Vec<(usize, EventKind)> {
    events.sort_by_key(|(at, _)| *at);
    events
}

fn bits(frames: &[[f32; CELLS]]) -> Vec<[u32; CELLS]> {
    frames.iter().map(|frame| frame.map(f32::to_bits)).collect()
}

fn peak(frames: &[[f32; CELLS]], cell: usize) -> f32 {
    frames
        .iter()
        .fold(0.0f32, |peak, frame| peak.max(frame[cell].abs()))
}

fn window() -> usize {
    (DECLICK_S * RATE) as usize + 1
}

/// Every model on the grid, neighbours trading hits, a touch and a bus, two events on one sample.
fn scene() -> (KitParams, [CellParams; CELLS], Vec<(usize, EventKind)>) {
    let models: Vec<(usize, ModelId)> = AVAILABLE_MODELS
        .iter()
        .enumerate()
        .map(|(i, spec)| (i, spec.id))
        .collect();
    let mut cells = cells(&models);
    cells[3].bus = Bus::A;
    cells[4].choke_group = 1;
    cells[5].choke_group = 1;
    let params = KitParams {
        bus: [0.4, 0.0],
        ..wild()
    };
    let mut events = vec![
        strike(0, 0, 1.0),
        release(40, 0),
        strike(900, 1, 0.8),
        (
            1_000,
            EventKind::TouchOn {
                cell: 2,
                velocity: 0.7,
            },
        ),
        strike(9_000, 4, 0.9),
        strike(12_345, 5, 0.6),
        release(30_000, 1),
        (20_000, EventKind::TouchOff { cell: 2 }),
        strike(20_000, 6, 0.9),
        strike(20_000, 7, 0.5),
        release(20_000, 6),
        (25_000, EventKind::Choke { cell: 7 }),
        release(60_000, 6),
    ];
    events = sorted(events);
    (params, cells, events)
}

#[test]
fn any_block_size_and_any_same_sample_order_renders_the_same_samples() {
    let (params, cells, events) = scene();
    let reference = bits(&render(
        &mut Kit::new(RATE, params, cells),
        &events,
        72_000,
        4096,
    ));
    for block in [1, 7, 64, 512] {
        let again = bits(&render(
            &mut Kit::new(RATE, params, cells),
            &events,
            72_000,
            block,
        ));
        assert!(again == reference, "block {block}");
    }
    let mut shuffled = events.clone();
    let at = shuffled.iter().position(|(t, _)| *t == 20_000).unwrap();
    shuffled[at..at + 4].reverse();
    assert!(
        bits(&render(
            &mut Kit::new(RATE, params, cells),
            &shuffled,
            72_000,
            256
        )) == reference
    );
}

#[test]
fn every_model_stays_finite_and_under_the_declared_peak_whatever_drives_it() {
    let mut rng = Xorshift32::new(7);
    let models: Vec<ModelId> = AVAILABLE_MODELS.iter().map(|spec| spec.id).collect();
    for rate in [1_000.0, 44_100.0, 48_000.0, 96_000.0, 192_000.0] {
        let random_cell = |rng: &mut Xorshift32| {
            let mut cell = CellParams::new(models[rng.next_u32() as usize % models.len()]);
            cell.coarse = (rng.bipolar_f32() * 24.0) as i8;
            cell.strike = rng.unit_f32();
            cell.supply = rng.unit_f32();
            cell.hunger = rng.unit_f32();
            cell.leak = rng.unit_f32();
            cell.threshold = rng.bipolar_f32();
            cell.touch = 1.0;
            cell.bus = if rng.unit_f32() < 0.5 { Bus::A } else { Bus::B };
            for control in &mut cell.controls {
                *control = rng.bipolar_f32();
            }
            cell
        };
        let cells: [CellParams; CELLS] = core::array::from_fn(|_| random_cell(&mut rng));
        let params = KitParams {
            bus: [1.0, 1.0],
            pool: rng.unit_f32(),
            ..wild()
        };
        let mut kit = Kit::new(rate, params, cells);
        let second = rate as usize;
        let mut events = Vec::new();
        for cell in 0..CELLS {
            events.push(strike(cell, cell, 1.0));
            events.push((
                cell + 1,
                EventKind::TouchOn {
                    cell,
                    velocity: 1.0,
                },
            ));
        }
        events.push((0, EventKind::Pressure(1.0)));
        for k in 1..40 {
            let cell = rng.next_u32() as usize % CELLS;
            events.push(strike(k * second / 20, cell, rng.unit_f32().max(0.01)));
            if k % 7 == 0 {
                events.push((
                    k * second / 20 + 3,
                    EventKind::Choke {
                        cell: (cell + 5) % CELLS,
                    },
                ));
            }
        }
        let events = sorted(events);
        // Half a second at a time, every cell's model and controls drawn again in between.
        for half in 0..4 {
            let start = half * second / 2;
            let here: Vec<_> = events
                .iter()
                .filter(|(at, _)| (start..start + second / 2).contains(at))
                .map(|(at, kind)| (at - start, *kind))
                .collect();
            for frame in render(&mut kit, &here, second / 2, 1) {
                for y in frame {
                    assert!(y.is_finite() && y.abs() <= 2.0, "at {rate}: {y}");
                }
            }
            for cell in 0..CELLS {
                kit.set_cell(cell, random_cell(&mut rng));
            }
        }
    }
}

#[test]
fn no_charge_is_exact_silence() {
    let mut silent = cells(&[(0, ModelId::WIRED_SNARE), (1, ModelId::RAW_LOOP)]);
    for cell in &mut silent {
        cell.strike = 0.0;
        cell.supply = 0.0;
    }
    let mut kit = Kit::new(RATE, wild(), silent);
    let out = render(
        &mut kit,
        &[strike(0, 0, 1.0), strike(10, 1, 1.0)],
        10_000,
        64,
    );
    assert!(out.iter().all(|frame| *frame == [0.0; CELLS]));

    let mut kit = Kit::new(
        RATE,
        KitParams::default(),
        cells(&[(0, ModelId::DRAINING_KICK)]),
    );
    render(&mut kit, &[strike(0, 0, 1.0)], 4_000, 64);
    kit.reset();
    assert!(kit.is_at_rest());
    assert!(
        render(&mut kit, &[], 1_000, 64)
            .iter()
            .all(|frame| *frame == [0.0; CELLS])
    );
}

/// The last sample of `cell` above −60 dB.
fn last_audible(out: &[[f32; CELLS]], cell: usize) -> usize {
    out.iter()
        .rposition(|frame| frame[cell].abs() > 1e-3)
        .unwrap_or(0)
}

#[test]
fn a_longer_note_sounds_longer_and_faster_hits_stack() {
    let kick = cells(&[(0, ModelId::DRAINING_KICK)]);
    let lengths: Vec<usize> = [1, 4_800, 14_400, 38_400]
        .iter()
        .map(|&gate| {
            let mut kit = Kit::new(RATE, steady(), kick);
            last_audible(
                &render(
                    &mut kit,
                    &[strike(0, 0, 1.0), release(gate, 0)],
                    200_000,
                    256,
                ),
                0,
            )
        })
        .collect();
    assert!(
        lengths.windows(2).all(|pair| pair[0] < pair[1]),
        "{lengths:?}"
    );

    let peak_at = |every: usize| {
        let mut kit = Kit::new(RATE, steady(), kick);
        let events = sorted(
            (0..8)
                .flat_map(|k| [strike(k * every, 0, 0.8), release(k * every + 960, 0)])
                .collect(),
        );
        peak(&render(&mut kit, &events, 8 * every + 20_000, 256), 0)
    };
    let (slow, fast) = (peak_at(12_000), peak_at(2_880));
    assert!(fast > slow, "fast {fast}, slow {slow}");
}

#[test]
fn at_temper_0_a_hit_from_rest_repeats_exactly_and_leaves_every_other_cell_silent() {
    let mut kit = Kit::new(
        RATE,
        steady(),
        cells(&[
            (0, ModelId::WIRED_SNARE),
            (1, ModelId::RAW_LOOP),
            (4, ModelId::SPARK_CLAP),
        ]),
    );
    let first = render(&mut kit, &[strike(0, 0, 0.9), release(500, 0)], 24_000, 256);
    let mut waited = 0;
    while !kit.is_at_rest() {
        render(&mut kit, &[], 4_800, 256);
        waited += 1;
        assert!(waited < 100, "no rest within 10 s");
    }
    let second = render(&mut kit, &[strike(0, 0, 0.9), release(500, 0)], 24_000, 256);
    assert!(bits(&first) == bits(&second));
    assert!(first.iter().any(|frame| frame[0] != 0.0));
    assert!((1..CELLS).all(|cell| first.iter().all(|frame| frame[cell] == 0.0)));
}

#[test]
fn a_choked_cell_stays_silent_beside_loud_coupled_neighbours_until_it_is_played() {
    let mut cells = cells(&[
        (0, ModelId::RAW_RING),
        (1, ModelId::RAW_RING),
        (4, ModelId::RAW_RING),
    ]);
    for cell in &mut cells {
        cell.supply = 1.0;
        cell.bus = Bus::A;
    }
    let params = KitParams {
        bus: [1.0, 0.0],
        ..wild()
    };
    let mut kit = Kit::new(RATE, params, cells);
    let events = [
        strike(0, 0, 1.0),
        strike(0, 1, 1.0),
        strike(0, 4, 1.0),
        (4_800, EventKind::Choke { cell: 0 }),
    ];
    let out = render(&mut kit, &events, 48_000, 256);
    assert!(
        out[4_800 + window()..].iter().all(|frame| frame[0] == 0.0),
        "choked cell 1 sounded"
    );
    assert!(
        peak(&out[24_000..], 1) > 0.01 && peak(&out[24_000..], 4) > 0.01,
        "its neighbours should sound on"
    );
    assert!(kit.is_latched(0));
    let after = render(&mut kit, &[strike(0, 0, 1.0)], 4_800, 256);
    assert!(peak(&after, 0) > 0.01 && !kit.is_latched(0));
}

#[test]
fn a_strike_chokes_its_group_but_never_a_cell_struck_with_it() {
    let mut cells = cells(&[(10, ModelId::WIRED_SNARE), (11, ModelId::RAW_RING)]);
    cells[10].choke_group = 1;
    cells[11].choke_group = 1;
    let mut kit = Kit::new(RATE, steady(), cells);
    let out = render(
        &mut kit,
        &[strike(0, 11, 1.0), strike(2_400, 10, 0.8)],
        24_000,
        256,
    );
    assert!(peak(&out[..2_400], 11) > 0.01);
    assert!(
        out[2_400 + window()..].iter().all(|frame| frame[11] == 0.0),
        "the open cell rang on"
    );

    let mut kit = Kit::new(RATE, steady(), cells);
    let both = render(
        &mut kit,
        &[strike(0, 10, 1.0), strike(0, 11, 1.0)],
        4_800,
        256,
    );
    assert!(peak(&both[window()..], 10) > 0.01 && peak(&both[window()..], 11) > 0.01);
}

#[test]
fn a_bus_drones_with_no_note_and_a_panic_holds_it_silent_until_played() {
    let mut cells = cells(&[(0, ModelId::RAW_RING)]);
    cells[0].bus = Bus::B;
    cells[0].supply = 0.8;
    let mut kit = Kit::new(
        RATE,
        KitParams {
            bus: [0.0, 1.0],
            ..steady()
        },
        cells,
    );
    let drone = render(&mut kit, &[], 48_000, 256);
    assert!(peak(&drone[24_000..], 0) > 0.05, "no drone");
    let after = render(&mut kit, &[(0, EventKind::Panic)], 48_000, 256);
    assert!(
        after.iter().all(|frame| frame[0] == 0.0),
        "the bus brought it back"
    );
    let played = render(&mut kit, &[strike(0, 0, 1.0), release(10, 0)], 48_000, 256);
    assert!(peak(&played[24_000..], 0) > 0.05);
}

#[test]
fn after_a_transport_reset_the_performance_is_a_render_from_rest() {
    let (params, cells, events) = scene();
    let reset = 30_000;
    let later = [
        strike(reset + 100, 0, 0.7),
        strike(reset + 5_000, 4, 1.0),
        release(reset + 9_000, 0),
    ];
    let mut through = events
        .into_iter()
        .filter(|(at, _)| *at < reset)
        .collect::<Vec<_>>();
    through.push((reset, EventKind::TransportReset));
    through.extend(later);
    let played = render(
        &mut Kit::new(RATE, params, cells),
        &sorted(through),
        reset + 24_000,
        256,
    );
    let shifted: Vec<_> = later.iter().map(|(at, kind)| (at - reset, *kind)).collect();
    let fresh = render(&mut Kit::new(RATE, params, cells), &shifted, 24_000, 256);
    let w = window();
    assert!(bits(&played[reset + w..]) == bits(&fresh[w..]));
}

#[test]
fn a_held_touch_sustains_its_cell() {
    let run = |touch: bool| {
        let mut cells = cells(&[(0, ModelId::RAW_RING)]);
        cells[0].touch = 1.0;
        let mut kit = Kit::new(RATE, steady(), cells);
        let mut events = vec![strike(0, 0, 0.8), release(10, 0)];
        if touch {
            events.push((
                20,
                EventKind::TouchOn {
                    cell: 0,
                    velocity: 1.0,
                },
            ));
        }
        let out = render(&mut kit, &events, 48_000, 256);
        out[24_000..]
            .iter()
            .map(|frame| frame[0] * frame[0])
            .sum::<f32>()
    };
    let (alone, touched) = (run(false), run(true));
    assert!(touched > 2.0 * alone, "touched {touched}, alone {alone}");
}

#[test]
fn a_cell_on_its_own_output_leaves_the_main_pair() {
    let mut cells = cells(&[(2, ModelId::DRAINING_KICK)]);
    cells[2].output = 3;
    let mut kit = Kit::new(RATE, steady(), cells);
    let mut frames = vec![OutputFrame::default(); 4_800];
    kit.process(
        &[Event {
            offset: 0,
            kind: EventKind::Strike {
                cell: 2,
                velocity: 1.0,
                gate: false,
            },
        }],
        &mut frames,
    );
    assert!(frames.iter().any(|frame| frame.individual[2] != 0.0));
    assert!(frames.iter().all(|frame| frame.main == [0.0; 2]));
}

#[test]
fn an_unfed_kit_comes_to_rest_and_rest_changes_nothing() {
    let (params, cells, _) = scene();
    let params = KitParams {
        bus: [0.0; 2],
        ..params
    };
    let mut kit = Kit::new(RATE, params, cells);
    let all: Vec<_> = (0..CELLS)
        .map(|cell| strike(cell * 10, cell, 1.0))
        .chain((0..CELLS).map(|cell| release(24_000, cell)))
        .collect();
    render(&mut kit, &sorted(all), 48_000, 256);
    let mut seconds = 1;
    while !kit.is_at_rest() {
        render(&mut kit, &[], 48_000, 256);
        seconds += 1;
        assert!(seconds < 60, "no rest within 60 s");
    }
    assert!(
        render(&mut kit, &[], 48_000, 256)
            .iter()
            .all(|frame| *frame == [0.0; CELLS])
    );
    assert!(kit.is_at_rest());
}

#[test]
fn holding_more_notes_gives_each_less() {
    let mut held = cells(&[(0, ModelId::DRAINING_KICK), (5, ModelId::DRAINING_KICK)]);
    for cell in &mut held {
        cell.strike = 0.0;
        cell.supply = 1.0;
    }
    let tight = KitParams {
        pool: 0.02,
        ..KitParams::default()
    };
    let charge_after = |events: &[(usize, EventKind)]| {
        let mut kit = Kit::new(RATE, tight, held);
        render(&mut kit, events, 1_440, 64);
        kit.charge(0)
    };
    let alone = charge_after(&[strike(0, 0, 1.0)]);
    let shared = charge_after(&[strike(0, 0, 1.0), strike(0, 5, 1.0)]);
    assert!(
        shared < alone && shared > 0.0,
        "alone {alone}, shared {shared}"
    );
}

#[test]
fn the_grid_wraps_round() {
    assert_eq!(neighbours(0), [12, 4, 3, 1]);
    assert_eq!(neighbours(15), [11, 3, 14, 12]);
}

#[test]
fn every_model_has_the_kind_its_label_names_and_its_own_controls() {
    for spec in AVAILABLE_MODELS {
        let label = spec.label.to_lowercase();
        let named = Kind::ALL
            .iter()
            .filter(|kind| label == kind.words() || label.ends_with(&format!(" {}", kind.words())))
            .max_by_key(|kind| kind.words().len())
            .copied();
        assert_eq!(named, Some(spec.kind), "{}", spec.label);
        let numbers: Vec<u8> = spec.controls.iter().map(|control| control.number).collect();
        assert!(
            numbers.iter().all(|n| (1..=12).contains(n)),
            "{}",
            spec.label
        );
        assert!(
            numbers.windows(2).all(|pair| pair[0] < pair[1]),
            "{}",
            spec.label
        );
        assert!(!spec.notes.is_empty());
    }
    let ids: Vec<u8> = AVAILABLE_MODELS.iter().map(|spec| spec.id.raw()).collect();
    assert!(
        ids.windows(2).all(|pair| pair[0] < pair[1]) && ids[0] > 0,
        "{ids:?}"
    );
}

#[test]
fn the_core_never_calls_the_platforms_maths_library() {
    const BANNED: [&str; 18] = [
        ".sin(", ".cos(", ".tan(", ".tanh(", ".sinh(", ".cosh(", ".asin(", ".acos(", ".atan",
        ".exp(", ".exp2(", ".exp_m1(", ".ln", ".log", ".powf(", ".powi(", ".cbrt(", ".hypot(",
    ];
    let mut checked = 0;
    let mut stack = vec![std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(path) = stack.pop() {
        if path.is_dir() {
            stack.extend(
                std::fs::read_dir(&path)
                    .unwrap()
                    .map(|entry| entry.unwrap().path()),
            );
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for (number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap();
            for banned in BANNED {
                assert!(
                    !code.contains(banned),
                    "{}:{}: {banned}",
                    path.display(),
                    number + 1
                );
            }
        }
        checked += 1;
    }
    assert!(checked >= 12, "only {checked} files scanned");
}

/// The series of `e^x`, summed far enough to be exact in f32.
fn exp_series(x: f64) -> f64 {
    let (mut term, mut sum) = (1.0f64, 1.0f64);
    for k in 1..40 {
        term *= x / f64::from(k);
        sum += term;
    }
    sum
}

#[test]
fn exp2_and_sin_are_exact_where_they_must_be_and_close_between() {
    let mut power = 1.0f32;
    for n in 0..=20 {
        assert_eq!(fmath::exp2(n as f32), power, "2^{n}");
        assert_eq!(fmath::exp2(-(n as f32)), 1.0 / power, "2^-{n}");
        power *= 2.0;
    }
    let mut x = -10.0f64;
    while x < 10.0 {
        let whole = x.floor();
        let reference = exp_series((x - whole) * core::f64::consts::LN_2) * 2f64.powi(whole as i32);
        let relative = ((f64::from(fmath::exp2(x as f32)) - reference) / reference).abs();
        assert!(relative < 3e-6, "2^{x}: {relative}");
        x += 0.013;
    }
    // sin by its series in f64, against the quarter-turn polynomial.
    let mut t = 0.0f64;
    while t < 1.0 {
        let angle = core::f64::consts::TAU * (t - (t + 0.5).floor());
        let (mut term, mut sum) = (angle, angle);
        for k in 1..30 {
            term *= -angle * angle / f64::from((2 * k) * (2 * k + 1));
            sum += term;
        }
        assert!(
            (f64::from(fmath::sin_turns(t as f32)) - sum).abs() < 2e-7,
            "sin at {t}"
        );
        t += 0.0007;
    }
    assert_eq!(fmath::sin_turns(0.0), 0.0);
    assert!((fmath::sin_turns(0.25) - 1.0).abs() < 2e-7);
}

/// [`render`], with the kit's settings applied again before every block, as the plugin does.
fn render_resetting(
    kit: &mut Kit,
    events: &[(usize, EventKind)],
    len: usize,
    block: usize,
) -> Vec<[f32; CELLS]> {
    let params = *kit.params();
    let mut out = vec![[0.0; CELLS]; len];
    let mut next = 0;
    for (start, chunk) in (0..len).step_by(block).zip(out.chunks_mut(block)) {
        kit.set_params(params);
        let mut here = Vec::new();
        while next < events.len() && events[next].0 < start + chunk.len() {
            here.push(Event {
                offset: events[next].0 - start,
                kind: events[next].1,
            });
            next += 1;
        }
        kit.process_raw(&here, chunk);
    }
    out
}

fn kit_route(
    source: Source,
    target: KitTarget,
    amount: f32,
) -> [Route<KitTarget>; mxm_chaos_drums_dsp::ROUTES] {
    let mut routes = [Route::default(); mxm_chaos_drums_dsp::ROUTES];
    routes[0] = Route {
        source,
        target,
        amount,
    };
    routes
}

#[test]
fn a_modulated_pool_renders_the_same_at_any_block_size_with_settings_applied_every_block() {
    let mut held = cells(&[(0, ModelId::RAW_RING), (1, ModelId::WIRED_SNARE)]);
    for cell in &mut held {
        cell.supply = 1.0;
    }
    let params = KitParams {
        pool: 0.1,
        lfo_rate_hz: [9.0, 1.3, 4.0],
        routes: kit_route(Source::Lfo1, KitTarget::Pool, 0.8),
        ..wild()
    };
    let events = [strike(0, 0, 1.0), strike(5, 1, 1.0)];
    let reference = bits(&render_resetting(
        &mut Kit::new(RATE, params, held),
        &events,
        48_000,
        1,
    ));
    for block in [7, 64, 4096] {
        let again = bits(&render_resetting(
            &mut Kit::new(RATE, params, held),
            &events,
            48_000,
            block,
        ));
        assert!(again == reference, "block {block}");
    }
}

#[test]
fn a_route_onto_a_bus_or_onto_supply_wakes_a_resting_cell() {
    let mut on_bus = cells(&[(0, ModelId::RAW_RING)]);
    on_bus[0].bus = Bus::A;
    on_bus[0].supply = 0.8;
    let by_bus = KitParams {
        routes: kit_route(Source::Lfo1, KitTarget::BusA, 1.0),
        ..steady()
    };
    let out = render(&mut Kit::new(RATE, by_bus, on_bus), &[], 48_000, 256);
    assert!(peak(&out, 0) > 0.01, "an LFO on Bus A fed nothing");

    let mut starved = on_bus;
    starved[0].supply = 0.0;
    starved[0].routes[0] = Route {
        source: Source::Lfo1,
        target: CellTarget::Supply,
        amount: 1.0,
    };
    let bus_up = KitParams {
        bus: [1.0, 0.0],
        ..steady()
    };
    let out = render(&mut Kit::new(RATE, bus_up, starved), &[], 48_000, 256);
    assert!(peak(&out, 0) > 0.01, "an LFO on Supply fed nothing");
}

#[test]
fn after_a_transport_reset_the_wheel_held_before_it_is_gone() {
    let mut kick = cells(&[(0, ModelId::DRAINING_KICK)]);
    kick[0].routes[0] = Route {
        source: Source::Wheel,
        target: CellTarget::Pitch,
        amount: 1.0,
    };
    let reset = 9_600;
    let history = [
        (0, EventKind::Wheel(1.0)),
        strike(10, 0, 1.0),
        (reset, EventKind::TransportReset),
        strike(reset + 50, 0, 0.9),
    ];
    let played = render(
        &mut Kit::new(RATE, steady(), kick),
        &history,
        reset + 24_000,
        256,
    );
    let fresh = render(
        &mut Kit::new(RATE, steady(), kick),
        &[(0, EventKind::TransportReset), strike(50, 0, 0.9)],
        24_000,
        256,
    );
    let w = window();
    assert!(bits(&played[reset + w..]) == bits(&fresh[w..]));
}

#[test]
fn a_transport_reset_takes_the_seed_set_since() {
    let snare = cells(&[(0, ModelId::WIRED_SNARE)]);
    let hit = [(0, EventKind::TransportReset), strike(0, 0, 1.0)];
    let mut kit = Kit::new(
        RATE,
        KitParams {
            seed: 1,
            ..steady()
        },
        snare,
    );
    render(&mut kit, &[strike(0, 0, 1.0)], 4_800, 256);
    kit.set_params(KitParams {
        seed: 2,
        ..steady()
    });
    let reseeded = render(&mut kit, &hit, 9_600, 256);
    let two = render(
        &mut Kit::new(
            RATE,
            KitParams {
                seed: 2,
                ..steady()
            },
            snare,
        ),
        &hit,
        9_600,
        256,
    );
    let one = render(
        &mut Kit::new(
            RATE,
            KitParams {
                seed: 1,
                ..steady()
            },
            snare,
        ),
        &hit,
        9_600,
        256,
    );
    let w = window();
    assert!(
        bits(&reseeded[w..]) == bits(&two[w..]),
        "the reset took seed 1"
    );
    assert!(bits(&two[w..]) != bits(&one[w..]), "two seeds, one sound");
}

#[test]
fn both_resets_fill_a_modulated_pool_whatever_came_before() {
    let mut held = cells(&[(0, ModelId::RAW_RING)]);
    held[0].strike = 0.0;
    held[0].supply = 1.0;
    let params = KitParams {
        pool: 0.3,
        lfo_rate_hz: [3.0, 1.3, 4.0],
        lfo_shape: [mxm_chaos_drums_dsp::LfoShape::Triangle; 3],
        routes: kit_route(Source::Lfo1, KitTarget::Pool, 0.7),
        ..steady()
    };
    let after = [strike(100, 0, 1.0)];
    let fresh = render(&mut Kit::new(RATE, params, held), &after, 24_000, 256);

    // A host reset after a history in which the LFO moved the pool.
    let mut kit = Kit::new(RATE, params, held);
    render(&mut kit, &[strike(0, 0, 1.0)], 7_000, 256);
    kit.reset();
    assert!(
        bits(&render(&mut kit, &after, 24_000, 256)) == bits(&fresh),
        "host reset"
    );

    // A transport reset after the same history.
    let reset = 7_000;
    let mut history = vec![strike(0, 0, 1.0), (reset, EventKind::TransportReset)];
    history.extend(after.iter().map(|(at, kind)| (at + reset, *kind)));
    let played = render(
        &mut Kit::new(RATE, params, held),
        &history,
        reset + 24_000,
        256,
    );
    let restarted = render(
        &mut Kit::new(RATE, params, held),
        &[(0, EventKind::TransportReset), after[0]],
        24_000,
        256,
    );
    let w = window();
    assert!(
        bits(&played[reset + w..]) == bits(&restarted[w..]),
        "transport reset"
    );
}

#[test]
fn a_hit_reads_its_own_velocity_through_its_routes() {
    let mut routed = cells(&[(0, ModelId::DRAINING_KICK)]);
    routed[0].routes[0] = Route {
        source: Source::Velocity,
        target: CellTarget::Strike,
        amount: 1.0,
    };
    // Velocity is `v − 1`, so a hit at 0.5 takes Strike down by half its range.
    let mut fixed = cells(&[(0, ModelId::DRAINING_KICK)]);
    fixed[0].strike = routed[0].strike - 0.5;
    let hit = [strike(0, 0, 0.5)];
    let a = render(&mut Kit::new(RATE, steady(), routed), &hit, 12_000, 256);
    let b = render(&mut Kit::new(RATE, steady(), fixed), &hit, 12_000, 256);
    assert!(
        bits(&a) == bits(&b),
        "the first hit used the previous hit's velocity"
    );
}

#[test]
fn a_panic_lets_go_of_the_wheel() {
    let mut kick = cells(&[(0, ModelId::DRAINING_KICK)]);
    kick[0].routes[0] = Route {
        source: Source::Wheel,
        target: CellTarget::Pitch,
        amount: 1.0,
    };
    let panic = 2_000;
    let after = [(panic, EventKind::Panic), strike(panic + 100, 0, 1.0)];
    let mut history = vec![(0, EventKind::Wheel(1.0)), strike(10, 0, 1.0)];
    history.extend(after);
    let played = render(
        &mut Kit::new(RATE, steady(), kick),
        &history,
        panic + 12_000,
        256,
    );
    let fresh = render(
        &mut Kit::new(RATE, steady(), kick),
        &after,
        panic + 12_000,
        256,
    );
    assert!(bits(&played[panic..]) == bits(&fresh[panic..]));
}

#[test]
fn events_applied_now_are_events_at_the_next_frame() {
    let snare = cells(&[(0, ModelId::WIRED_SNARE), (1, ModelId::RAW_LOOP)]);
    let kinds = [
        EventKind::Strike {
            cell: 0,
            velocity: 0.8,
            gate: true,
        },
        EventKind::Strike {
            cell: 1,
            velocity: 0.6,
            gate: false,
        },
        EventKind::Wheel(0.5),
    ];
    let mut now = Kit::new(RATE, wild(), snare);
    now.apply_now(&kinds.map(|kind| Event { offset: 0, kind }));
    let a = render(&mut now, &[], 12_000, 256);
    let b = render(
        &mut Kit::new(RATE, wild(), snare),
        &kinds.map(|kind| (0, kind)),
        12_000,
        256,
    );
    assert!(bits(&a) == bits(&b));
}

/// What one model plays for a held hit, with `controls`, at a Temper that reaches its thresholds.
fn played_with(
    model: ModelId,
    controls: [f32; mxm_chaos_drums_dsp::CONTROLS],
) -> Vec<[u32; CELLS]> {
    let mut cell = cells(&[(0, model)]);
    cell[0].supply = 1.0;
    cell[0].controls = controls;
    let params = KitParams {
        temper: 0.6,
        ..steady()
    };
    bits(&render(
        &mut Kit::new(RATE, params, cell),
        &[strike(0, 0, 1.0), release(19_200, 0)],
        38_400,
        256,
    ))
}

#[test]
fn every_model_shows_exactly_the_controls_it_reads() {
    for spec in AVAILABLE_MODELS {
        let reference = played_with(spec.id, [0.0; mxm_chaos_drums_dsp::CONTROLS]);
        for number in 1..=mxm_chaos_drums_dsp::CONTROLS as u8 {
            let mut controls = [0.0; mxm_chaos_drums_dsp::CONTROLS];
            controls[usize::from(number) - 1] = 0.8;
            let moved = played_with(spec.id, controls) != reference;
            let shown = spec.controls.iter().any(|control| control.number == number);
            assert_eq!(
                moved, shown,
                "{}: control {number} moves the sound: {moved}, shown: {shown}",
                spec.label
            );
        }
    }
}

#[test]
fn settling_now_answers_for_a_bus_raised_with_no_frames() {
    let mut on_bus = cells(&[(0, ModelId::RAW_RING)]);
    on_bus[0].bus = Bus::A;
    on_bus[0].supply = 0.8;
    let mut kit = Kit::new(RATE, steady(), on_bus);
    render(&mut kit, &[], 256, 256);
    assert!(kit.is_at_rest());
    kit.set_params(KitParams {
        bus: [1.0, 0.0],
        ..steady()
    });
    kit.settle_now();
    assert!(!kit.is_at_rest(), "a raised bus feeds its cell");
}

#[test]
fn the_seed_chooses_the_sample_and_hold_and_repeats_it() {
    let mut ring = cells(&[(0, ModelId::RAW_RING)]);
    ring[0].supply = 1.0;
    ring[0].routes[0] = Route {
        source: Source::Lfo1,
        target: CellTarget::Pitch,
        amount: 1.0,
    };
    let with = |seed| {
        let params = KitParams {
            seed,
            lfo_rate_hz: [20.0, 1.3, 4.0],
            lfo_shape: [mxm_chaos_drums_dsp::LfoShape::SampleHold; 3],
            ..steady()
        };
        bits(&render(
            &mut Kit::new(RATE, params, ring),
            &[strike(0, 0, 1.0)],
            24_000,
            256,
        ))
    };
    assert!(with(1) == with(1), "a seed repeats");
    assert!(with(1) != with(2), "two seeds, one sample and hold");
}

#[test]
fn a_host_reset_takes_the_seed_set_since() {
    let snare = cells(&[(0, ModelId::WIRED_SNARE)]);
    let mut kit = Kit::new(
        RATE,
        KitParams {
            seed: 1,
            ..steady()
        },
        snare,
    );
    render(&mut kit, &[strike(0, 0, 1.0)], 4_800, 256);
    kit.set_params(KitParams {
        seed: 2,
        ..steady()
    });
    kit.reset();
    let reset = render(&mut kit, &[strike(0, 0, 1.0)], 9_600, 256);
    let fresh = render(
        &mut Kit::new(
            RATE,
            KitParams {
                seed: 2,
                ..steady()
            },
            snare,
        ),
        &[strike(0, 0, 1.0)],
        9_600,
        256,
    );
    assert!(bits(&reset) == bits(&fresh));
}

#[test]
fn a_transport_reset_applied_now_is_one_at_the_next_frame() {
    let kick = cells(&[(0, ModelId::DRAINING_KICK)]);
    let kinds = [
        EventKind::TransportReset,
        EventKind::Strike {
            cell: 0,
            velocity: 0.9,
            gate: true,
        },
    ];
    let mut now = Kit::new(RATE, steady(), kick);
    render(&mut now, &[strike(0, 0, 1.0)], 2_000, 256);
    now.apply_now(&kinds.map(|kind| Event { offset: 0, kind }));
    let a = render(&mut now, &[], 12_000, 256);
    let mut later = Kit::new(RATE, steady(), kick);
    render(&mut later, &[strike(0, 0, 1.0)], 2_000, 256);
    let b = render(&mut later, &kinds.map(|kind| (0, kind)), 12_000, 256);
    assert!(bits(&a) == bits(&b));
}

#[test]
fn a_fresh_kit_is_a_reset_one() {
    let mut on_bus = cells(&[(0, ModelId::RAW_RING)]);
    on_bus[0].bus = Bus::A;
    on_bus[0].supply = 0.8;
    let fed = KitParams {
        bus: [1.0, 0.0],
        ..steady()
    };
    assert!(
        !Kit::new(RATE, fed, on_bus).is_at_rest(),
        "a fresh kit with Bus A up rests"
    );

    let mut held = cells(&[(0, ModelId::RAW_RING)]);
    held[0].strike = 0.0;
    held[0].supply = 1.0;
    let raised = KitParams {
        pool: 0.2,
        lfo_shape: [mxm_chaos_drums_dsp::LfoShape::RampDown; 3],
        routes: kit_route(Source::Lfo1, KitTarget::Pool, 1.0),
        ..steady()
    };
    let hit = [strike(0, 0, 1.0)];
    let fresh = render(&mut Kit::new(RATE, raised, held), &hit, 24_000, 256);
    let mut reset = Kit::new(RATE, raised, held);
    reset.reset();
    assert!(bits(&fresh) == bits(&render(&mut reset, &hit, 24_000, 256)));
}

#[test]
fn an_lfo_that_may_feed_a_resting_cell_keeps_the_kit_awake() {
    let mut on_bus = cells(&[(0, ModelId::RAW_RING)]);
    on_bus[0].bus = Bus::A;
    let quiet = Kit::new(RATE, steady(), on_bus);
    assert!(
        quiet.is_at_rest() && !quiet.lfo_may_feed(),
        "nothing moves Bus A"
    );
    let lfo = KitParams {
        routes: kit_route(Source::Lfo1, KitTarget::BusA, 1.0),
        ..steady()
    };
    assert!(
        Kit::new(RATE, lfo, on_bus).lfo_may_feed(),
        "an LFO on Bus A"
    );
    let wheel = KitParams {
        routes: kit_route(Source::Wheel, KitTarget::BusA, 1.0),
        ..steady()
    };
    assert!(
        !Kit::new(RATE, wheel, on_bus).lfo_may_feed(),
        "the wheel moves only with an event"
    );
}

#[test]
fn an_lfo_keeps_a_kit_awake_only_where_it_can_feed_a_cell() {
    let lfo_on = |target| KitParams {
        routes: kit_route(Source::Lfo1, target, 1.0),
        ..steady()
    };
    let mut on_bus = cells(&[(0, ModelId::RAW_RING)]);
    on_bus[0].bus = Bus::A;
    let on_supply = |mut cells: [CellParams; CELLS], amount| {
        cells[0].supply = 0.0;
        cells[0].routes[0] = Route {
            source: Source::Lfo2,
            target: CellTarget::Supply,
            amount,
        };
        cells
    };
    // Bus A up, Supply moved by an LFO: it can feed.
    let bus_up = KitParams {
        bus: [1.0, 0.0],
        ..steady()
    };
    assert!(Kit::new(RATE, bus_up, on_supply(on_bus, 1.0)).lfo_may_feed());
    // Supply moved, but nothing can raise Bus A from zero: it never feeds.
    assert!(
        !Kit::new(RATE, steady(), on_supply(on_bus, 1.0)).lfo_may_feed(),
        "a bus that stays at zero"
    );
    // A route of no amount moves nothing.
    assert!(
        !Kit::new(RATE, bus_up, on_supply(on_bus, 0.0)).lfo_may_feed(),
        "an amount of zero"
    );
    let silent = KitParams {
        routes: kit_route(Source::Lfo1, KitTarget::BusA, 0.0),
        ..steady()
    };
    assert!(
        !Kit::new(RATE, silent, on_bus).lfo_may_feed(),
        "a bus route of no amount"
    );
    // An LFO on the bus with Supply at zero and unrouted: it never feeds.
    let mut dry = on_bus;
    dry[0].supply = 0.0;
    assert!(
        !Kit::new(RATE, lfo_on(KitTarget::BusA), dry).lfo_may_feed(),
        "no supply"
    );
    assert!(
        Kit::new(RATE, lfo_on(KitTarget::BusA), on_bus).lfo_may_feed(),
        "an LFO on Bus A, Supply up"
    );
}

#[test]
fn routes_that_cancel_or_never_agree_let_a_kit_sleep() {
    let two = |a: Route<KitTarget>, b: Route<KitTarget>| {
        let mut routes = [Route::default(); mxm_chaos_drums_dsp::ROUTES];
        routes[0] = a;
        routes[1] = b;
        routes
    };
    let mut on_bus = cells(&[(0, ModelId::RAW_RING)]);
    on_bus[0].bus = Bus::A;
    on_bus[0].supply = 0.8;
    // Two LFO 1 routes on Bus A that cancel: Bus A never moves.
    let lfo = |amount| Route {
        source: Source::Lfo1,
        target: KitTarget::BusA,
        amount,
    };
    let cancel = KitParams {
        routes: two(lfo(1.0), lfo(-1.0)),
        ..steady()
    };
    assert!(
        !Kit::new(RATE, cancel, on_bus).lfo_may_feed(),
        "cancelling routes"
    );
    // LFO 1 raises Bus A while it takes Supply down from zero: never both above zero.
    let mut opposed = on_bus;
    opposed[0].supply = 0.0;
    opposed[0].routes[0] = Route {
        source: Source::Lfo1,
        target: CellTarget::Supply,
        amount: -1.0,
    };
    let raise = KitParams {
        routes: two(lfo(1.0), Route::default()),
        ..steady()
    };
    assert!(
        !Kit::new(RATE, raise, opposed).lfo_may_feed(),
        "opposed phases"
    );
    opposed[0].routes[0].amount = 1.0;
    assert!(
        Kit::new(RATE, raise, opposed).lfo_may_feed(),
        "the same phase lifts both"
    );
}

#[test]
fn a_pool_of_no_capacity_feeds_nothing_until_an_lfo_raises_it() {
    let mut on_bus = cells(&[(0, ModelId::RAW_RING)]);
    on_bus[0].bus = Bus::A;
    on_bus[0].supply = 0.8;
    let empty = KitParams {
        pool: 0.0,
        bus: [1.0, 0.0],
        ..steady()
    };
    let mut kit = Kit::new(RATE, empty, on_bus);
    render(&mut kit, &[], 256, 256);
    assert!(
        kit.is_at_rest() && !kit.lfo_may_feed(),
        "a pool of nothing feeds nothing"
    );
    let raised = KitParams {
        routes: kit_route(Source::Lfo1, KitTarget::Pool, 1.0),
        ..empty
    };
    assert!(
        Kit::new(RATE, raised, on_bus).lfo_may_feed(),
        "an LFO can raise the pool"
    );
}

#[test]
fn a_square_lfo_wakes_only_where_one_of_its_two_values_feeds() {
    let mut opposed = cells(&[(0, ModelId::RAW_RING)]);
    opposed[0].bus = Bus::A;
    opposed[0].supply = 0.5;
    opposed[0].routes[0] = Route {
        source: Source::Lfo1,
        target: CellTarget::Supply,
        amount: -1.0,
    };
    let with = |shape| KitParams {
        bus: [0.5, 0.0],
        lfo_shape: [
            shape,
            mxm_chaos_drums_dsp::LfoShape::Triangle,
            mxm_chaos_drums_dsp::LfoShape::Sine,
        ],
        routes: kit_route(Source::Lfo1, KitTarget::BusA, 1.0),
        ..steady()
    };
    // At +1 Supply is zero, at -1 Bus A is: a square never feeds, a sweep does near zero.
    let square = Kit::new(RATE, with(mxm_chaos_drums_dsp::LfoShape::Square), opposed);
    assert!(!square.lfo_may_feed(), "a square never feeds");
    let sweep = Kit::new(RATE, with(mxm_chaos_drums_dsp::LfoShape::Triangle), opposed);
    assert!(sweep.lfo_may_feed(), "a triangle crosses where both feed");
}
