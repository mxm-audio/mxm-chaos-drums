//! Balances the Init kit's Levels. Each cell is struck alone, as a sequencer plays it, and its peak
//! short-term spectrum is measured in third-octave bands. Two balances come out of it, each with the
//! kick as the reference at 0 dB:
//!
//! - **Pink**: the Level that puts a cell's strongest band as high as every other cell's. Pink noise
//!   has the same power in every third-octave band, so a kit balanced this way follows its spectrum,
//!   which is the pink-noise mixing method (each part heard over pink noise by the same amount).
//! - **Loudness**: the Level that gives every cell the same K-weighted short-term power, the
//!   weighting loudness meters use (ITU-R BS.1770-4, its 48 kHz filters): how loud the ear hears it.
//!
//! It changes nothing: a balance goes into `params::INIT` by hand (the owner, 2026-11-03: a way to
//! set a kit's levels, not the sounds themselves), and the last column is the Level Init has now.
//! Init takes the loudness balance, its noise cells 6 dB under it (`params::INIT`).
//!
//! ```text
//! cargo run -p mxm-chaos-drums --example kit_balance --release
//! ```

use mxm_chaos_drums::params::MxmChaosDrumsParams;
use mxm_chaos_drums_dsp::{CELLS, CellParams, Event, EventKind, Kit, KitParams, available};
use mxm_measure::spectrum::fft;

const RATE: f32 = 48_000.0;
/// The editor's audition velocity.
const VELOCITY: f32 = 100.0 / 127.0;
/// Half a sixteenth at 120 BPM: a sequencer's gate.
const GATE_S: f32 = 0.0625;
const SECONDS: f32 = 3.0;
/// The short-term window, 170 ms at 48 kHz: wide enough to resolve the lowest band, and about the
/// ear's own integration time. And its hop.
const WINDOW: usize = 8192;
const HOP: usize = 1024;
/// The third-octave bands, `1 kHz · 2^(k/3)`: 31.25 Hz to 16 kHz.
const BANDS: std::ops::RangeInclusive<i32> = -15..=12;
/// The Level a cell may take: the parameter's range.
const LEVEL_DB: (f64, f64) = (-60.0, 6.0);

/// One cell's hit, measured.
struct Hit {
    /// The highest short-term power any band reaches, in dB, and that band's centre.
    band_db: f64,
    band_hz: f64,
    /// The highest short-term power over all bands, K-weighted, in dB.
    loudness_db: f64,
}

fn main() {
    let params = MxmChaosDrumsParams::default();
    let kit = params.kit(params.lfo_hz(None));
    let cells: [_; CELLS] = std::array::from_fn(|i| params.cells[i].dsp(false));
    let hits: Vec<Hit> = (0..CELLS)
        .map(|cell| measure(&render(kit, cells, cell)))
        .collect();
    let level = |db: f64, reference: f64| {
        (((reference - db) * 2.0).round() / 2.0).clamp(LEVEL_DB.0, LEVEL_DB.1)
    };
    println!("cell  model            strongest band      K-weighted   Pink   Loudness   Init");
    for (cell, hit) in hits.iter().enumerate() {
        let model = available(cells[cell].model).map_or("none", |spec| spec.label);
        println!(
            "{:>4}  {model:<15} {:>6.1} dB at {:>6.0} Hz  {:>6.1} dB  {:>+5.1}  {:>+5.1}     {:>+5.1}",
            cell + 1,
            hit.band_db,
            hit.band_hz,
            hit.loudness_db,
            level(hit.band_db, hits[0].band_db),
            level(hit.loudness_db, hits[0].loudness_db),
            nice_plug::util::gain_to_db(params.cells[cell].level.value()),
        );
    }
}

/// Cell `cell`'s output before Level, struck alone from rest.
fn render(kit: KitParams, cells: [CellParams; CELLS], cell: usize) -> Vec<f64> {
    let mut kit = Kit::new(RATE, kit, cells);
    let mut out = vec![[0.0; CELLS]; (SECONDS * RATE) as usize];
    let events = [
        Event {
            offset: 0,
            kind: EventKind::Strike {
                cell,
                velocity: VELOCITY,
                gate: true,
            },
        },
        Event {
            offset: (GATE_S * RATE) as usize,
            kind: EventKind::Release { cell },
        },
    ];
    kit.process_raw(&events, &mut out);
    out.iter().map(|frame| f64::from(frame[cell])).collect()
}

fn measure(x: &[f64]) -> Hit {
    let hann: Vec<f64> = (0..WINDOW)
        .map(|n| 0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / WINDOW as f64).cos())
        .collect();
    let bin_hz = f64::from(RATE) / WINDOW as f64;
    let centre = |k: i32| 1000.0 * 2f64.powf(f64::from(k) / 3.0);
    // Each bin's band, if it falls in one, and its K-weighting as a power.
    let bins: Vec<(Option<usize>, f64)> = (0..WINDOW / 2)
        .map(|bin| {
            let hz = bin as f64 * bin_hz;
            let band = BANDS.clone().position(|k| {
                (centre(k) * 2f64.powf(-1.0 / 6.0)..centre(k) * 2f64.powf(1.0 / 6.0)).contains(&hz)
            });
            (band, k_weight(hz))
        })
        .collect();
    let mut peak = vec![0.0f64; BANDS.count()];
    let mut loudest = 0.0f64;
    // Silence before the hit, so its attack is in the middle of a window as well as at the end.
    let padded: Vec<f64> = std::iter::repeat_n(0.0, WINDOW / 2)
        .chain(x.iter().copied())
        .collect();
    for start in (0..padded.len().saturating_sub(WINDOW)).step_by(HOP) {
        let mut re: Vec<f64> = padded[start..start + WINDOW]
            .iter()
            .zip(&hann)
            .map(|(x, w)| x * w)
            .collect();
        let mut im = vec![0.0; WINDOW];
        fft(&mut re, &mut im).expect("a power of two");
        let mut bands = vec![0.0f64; BANDS.count()];
        let mut weighted = 0.0;
        for (bin, (band, weight)) in bins.iter().enumerate().skip(1) {
            let power = re[bin] * re[bin] + im[bin] * im[bin];
            if let Some(band) = band {
                bands[*band] += power;
            }
            weighted += power * weight;
        }
        for (peak, band) in peak.iter_mut().zip(bands) {
            *peak = peak.max(band);
        }
        loudest = loudest.max(weighted);
    }
    let (band, power) =
        peak.iter().enumerate().fold(
            (0, 0.0),
            |best, (band, &power)| if power > best.1 { (band, power) } else { best },
        );
    let db = |power: f64| 10.0 * power.max(1e-30).log10();
    Hit {
        band_db: db(power),
        band_hz: centre(*BANDS.start() + band as i32),
        loudness_db: db(loudest),
    }
}

/// The K-weighting's power gain at `hz`: BS.1770-4's two 48 kHz biquads, a high shelf for the head
/// and a high-pass, evaluated on the unit circle.
fn k_weight(hz: f64) -> f64 {
    const SHELF: ([f64; 3], [f64; 2]) = (
        [
            1.535_124_859_586_97,
            -2.691_696_189_406_38,
            1.198_392_810_852_85,
        ],
        [-1.690_659_293_182_41, 0.732_480_774_215_85],
    );
    const HIGH_PASS: ([f64; 3], [f64; 2]) = (
        [1.0, -2.0, 1.0],
        [-1.990_047_454_833_98, 0.990_072_250_366_21],
    );
    let w = std::f64::consts::TAU * hz / f64::from(RATE);
    let power = |(b, a): ([f64; 3], [f64; 2])| {
        let at = |c: [f64; 3]| {
            let re = c[0] + c[1] * w.cos() + c[2] * (2.0 * w).cos();
            let im = -c[1] * w.sin() - c[2] * (2.0 * w).sin();
            re * re + im * im
        };
        at(b) / at([1.0, a[0], a[1]])
    };
    power(SHELF) * power(HIGH_PASS)
}
