//! The P1 crate's contracts: reproducible, bounded, the charge law, Temper 0's steadiness, rest,
//! the pool, the catalogue and the platform rule.

use mxm_chaos_drums_dsp::fmath;
use mxm_chaos_drums_dsp::{
    AVAILABLE_MODELS, CELLS, CellParams, Event, EventKind, Kind, Kit, KitParams, ModelId,
};
use mxm_rng::Xorshift32;

const RATE: f32 = 48_000.0;

fn on(at: usize, cell: usize, velocity: f32) -> (usize, EventKind) {
    (at, EventKind::NoteOn { cell, velocity })
}

fn off(at: usize, cell: usize) -> (usize, EventKind) {
    (at, EventKind::NoteOff { cell })
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
        kit.process(&here, chunk);
    }
    out
}

fn kit(temper: f32, cells: [CellParams; CELLS]) -> Kit {
    Kit::new(
        RATE,
        KitParams {
            temper,
            flow: 1.0,
            coupling: 1.0,
            ..KitParams::default()
        },
        cells,
    )
}

fn bits(frames: &[[f32; CELLS]]) -> Vec<[u32; CELLS]> {
    frames.iter().map(|frame| frame.map(f32::to_bits)).collect()
}

/// A wild, linked scene: hits on both cells, a held note, two events on one sample.
fn scene() -> Vec<(usize, EventKind)> {
    let mut events = vec![
        on(0, 0, 1.0),
        off(40, 0),
        on(9_000, 1, 0.8),
        off(30_000, 1),
        on(12_345, 0, 0.6),
        off(12_400, 0),
        on(20_000, 0, 0.9),
        on(20_000, 1, 0.5),
        off(20_000, 0),
        off(60_000, 0),
    ];
    events.sort_by_key(|(at, _)| *at);
    events
}

#[test]
fn any_block_size_and_any_same_sample_order_renders_the_same_samples() {
    let cells = [
        CellParams::new(ModelId::DRAINING_KICK),
        CellParams::new(ModelId::WIRED_SNARE),
    ];
    let reference = bits(&render(&mut kit(0.8, cells), &scene(), 96_000, 4096));
    for block in [1, 7, 64, 512] {
        assert!(
            bits(&render(&mut kit(0.8, cells), &scene(), 96_000, block)) == reference,
            "block {block}"
        );
    }
    // The events at sample 20 000 in the opposite order.
    let mut shuffled = scene();
    let at = shuffled.iter().position(|(t, _)| *t == 20_000).unwrap();
    shuffled[at..at + 3].reverse();
    assert!(bits(&render(&mut kit(0.8, cells), &shuffled, 96_000, 256)) == reference);
}

#[test]
fn every_model_stays_finite_and_within_its_charge_whatever_drives_it() {
    let mut rng = Xorshift32::new(7);
    let models: Vec<ModelId> = AVAILABLE_MODELS.iter().map(|spec| spec.id).collect();
    for rate in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
        for &a in &models {
            for &b in &models {
                let mut random_cell = |model| CellParams {
                    model,
                    coarse: (rng.bipolar_f32() * 24.0) as i8,
                    strike: rng.unit_f32(),
                    supply: rng.unit_f32(),
                    hunger: rng.unit_f32(),
                    leak_ms: 20.0 + rng.unit_f32() * 30_000.0,
                    threshold: rng.bipolar_f32(),
                    link: 1.0,
                };
                let cells = [random_cell(a), random_cell(b)];
                let params = KitParams {
                    temper: 1.0,
                    flow: 1.0,
                    coupling: 1.0,
                    pool: rng.unit_f32(),
                    ..KitParams::default()
                };
                let mut kit = Kit::new(rate, params, cells);
                let second = rate as usize;
                let mut events = vec![on(0, 0, 1.0), on(0, 1, 1.0)];
                for k in 1..20 {
                    events.push(on(k * second / 10, k % 2, rng.unit_f32().max(0.01)));
                }
                events.sort_by_key(|(at, _)| *at);
                for frame in render(&mut kit, &events, 2 * second, 1) {
                    for y in frame {
                        assert!(
                            y.is_finite() && y.abs() <= 1.0,
                            "{a:?}/{b:?} at {rate}: {y}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn no_charge_is_exact_silence() {
    let silent = CellParams {
        strike: 0.0,
        supply: 0.0,
        ..CellParams::new(ModelId::WIRED_SNARE)
    };
    let mut kit = kit(1.0, [silent, silent]);
    let out = render(&mut kit, &[on(0, 0, 1.0), on(10, 1, 1.0)], 10_000, 64);
    assert!(out.iter().all(|frame| *frame == [0.0; CELLS]));

    let mut kit = Kit::new(
        RATE,
        KitParams::default(),
        [CellParams::new(ModelId::DRAINING_KICK); CELLS],
    );
    render(&mut kit, &[on(0, 0, 1.0)], 4_000, 64);
    kit.reset();
    assert!(kit.is_at_rest());
    assert!(
        render(&mut kit, &[], 1_000, 64)
            .iter()
            .all(|frame| *frame == [0.0; CELLS])
    );
}

/// The last sample above −60 dB.
fn last_audible(out: &[[f32; CELLS]]) -> usize {
    out.iter()
        .rposition(|frame| frame[0].abs() > 1e-3)
        .unwrap_or(0)
}

#[test]
fn a_longer_note_sounds_longer_and_faster_hits_stack() {
    let kick = CellParams::new(ModelId::DRAINING_KICK);
    let lengths: Vec<usize> = [1, 4_800, 14_400, 38_400]
        .iter()
        .map(|&gate| {
            let mut kit = kit(0.0, [kick; CELLS]);
            last_audible(&render(
                &mut kit,
                &[on(0, 0, 1.0), off(gate, 0)],
                200_000,
                256,
            ))
        })
        .collect();
    assert!(
        lengths.windows(2).all(|pair| pair[0] < pair[1]),
        "{lengths:?}"
    );

    let peak_at = |every: usize| {
        let mut kit = kit(0.0, [kick; CELLS]);
        let mut events = Vec::new();
        for k in 0..8 {
            events.push(on(k * every, 0, 0.8));
            events.push(off(k * every + 960, 0));
        }
        let out = render(&mut kit, &events, 8 * every + 20_000, 256);
        out.iter()
            .fold(0.0f32, |peak, frame| peak.max(frame[0].abs()))
    };
    let (slow, fast) = (peak_at(12_000), peak_at(2_880));
    assert!(fast > slow, "fast {fast}, slow {slow}");
}

#[test]
fn at_temper_0_a_hit_from_rest_repeats_exactly_and_leaves_its_neighbour_silent() {
    let cells = [
        CellParams::new(ModelId::WIRED_SNARE),
        CellParams::new(ModelId::SPARK_CLAP),
    ];
    let mut kit = kit(0.0, cells);
    let first = render(&mut kit, &[on(0, 0, 0.9), off(500, 0)], 24_000, 256);
    let mut waited = 0;
    while !kit.is_at_rest() {
        render(&mut kit, &[], 4_800, 256);
        waited += 1;
        assert!(waited < 100, "no rest within 10 s");
    }
    let second = render(&mut kit, &[on(0, 0, 0.9), off(500, 0)], 24_000, 256);
    assert!(bits(&first) == bits(&second));
    assert!(first.iter().any(|frame| frame[0] != 0.0));
    assert!(
        first.iter().all(|frame| frame[1] == 0.0),
        "linked at full Flow and Coupling, Temper 0"
    );
}

#[test]
fn an_unfed_kit_comes_to_rest_and_rest_changes_nothing() {
    let cells = [
        CellParams::new(ModelId::DRAINING_KICK),
        CellParams::new(ModelId::WIRED_SNARE),
    ];
    let mut kit = kit(0.8, cells);
    render(
        &mut kit,
        &[
            on(0, 0, 1.0),
            on(100, 1, 1.0),
            off(24_000, 0),
            off(24_000, 1),
        ],
        48_000,
        256,
    );
    let mut seconds = 1;
    while !kit.is_at_rest() {
        render(&mut kit, &[], 48_000, 256);
        seconds += 1;
        assert!(seconds < 30, "no rest within 30 s");
    }
    assert!(
        render(&mut kit, &[], 48_000, 256)
            .iter()
            .all(|frame| *frame == [0.0; CELLS])
    );
    assert!(kit.is_at_rest());
}

#[test]
fn holding_both_notes_gives_each_less() {
    let cell = CellParams {
        strike: 0.0,
        supply: 1.0,
        ..CellParams::new(ModelId::DRAINING_KICK)
    };
    let tight = KitParams {
        pool: 0.02,
        ..KitParams::default()
    };
    let charge_after = |events: &[(usize, EventKind)]| {
        let mut kit = Kit::new(RATE, tight, [cell; CELLS]);
        render(&mut kit, events, 1_440, 64);
        kit.charge(0)
    };
    let alone = charge_after(&[on(0, 0, 1.0)]);
    let shared = charge_after(&[on(0, 0, 1.0), on(0, 1, 1.0)]);
    assert!(
        shared < alone && shared > 0.0,
        "alone {alone}, shared {shared}"
    );
}

#[test]
fn every_model_has_the_kind_its_label_names_and_ids_ascend() {
    for spec in AVAILABLE_MODELS {
        let label = spec.label.to_lowercase();
        let named = Kind::ALL
            .iter()
            .filter(|kind| label == kind.words() || label.ends_with(&format!(" {}", kind.words())))
            .max_by_key(|kind| kind.words().len())
            .copied();
        assert_eq!(named, Some(spec.kind), "{}", spec.label);
    }
    let ids: Vec<u8> = AVAILABLE_MODELS.iter().map(|spec| spec.id.raw()).collect();
    assert!(
        ids.windows(2).all(|pair| pair[0] < pair[1]) && ids[0] > 0,
        "{ids:?}"
    );
}

#[test]
fn the_core_never_calls_the_platforms_maths_library() {
    const BANNED: [&str; 16] = [
        ".sin(", ".cos(", ".tan(", ".tanh(", ".sinh(", ".cosh(", ".asin(", ".acos(", ".atan",
        ".exp(", ".exp2(", ".exp_m1(", ".ln", ".log", ".powf(", ".powi(",
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
            for banned in BANNED.iter().chain(&[".cbrt(", ".hypot("]) {
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
    assert!(checked >= 8, "only {checked} files scanned");
}

#[test]
fn exp2_is_exact_on_integers_and_within_3e_6_between() {
    let mut power = 1.0f32;
    for n in 0..=20 {
        assert_eq!(fmath::exp2(n as f32), power, "2^{n}");
        assert_eq!(fmath::exp2(-(n as f32)), 1.0 / power, "2^-{n}");
        power *= 2.0;
    }
    // Between the integers, against the f64 series, summed far enough to be exact in f32.
    let mut x = -10.0f64;
    while x < 10.0 {
        let whole = x.floor();
        let f = (x - whole) * core::f64::consts::LN_2;
        let (mut term, mut sum) = (1.0f64, 1.0f64);
        for k in 1..30 {
            term *= f / f64::from(k);
            sum += term;
        }
        let reference = sum * 2f64.powi(whole as i32);
        let relative = ((f64::from(fmath::exp2(x as f32)) - reference) / reference).abs();
        assert!(relative < 3e-6, "2^{x}: {relative}");
        x += 0.013;
    }
}
