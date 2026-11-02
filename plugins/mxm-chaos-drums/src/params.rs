//! The parameter surface: the brief's 41 a cell and the kit's, their IDs, ranges and readings.
//!
//! nice-plug's nested arrays make a cell's IDs `<suffix>_1` … `<suffix>_16`. **A control that
//! reaches a cell's state is a plain linear amount** whose musical curve is the DSP's
//! (`mxm_chaos_drums_dsp::curves`): nice-plug's skewed ranges and logarithmic smoothers call `powf`,
//! and the performance must be the same on every platform (the owner, 2026-11-02). Only Level, Pan
//! and Master, which act after the performance, take the collection's skewed gain; the DSP's mix
//! smooths them. IDs are free to change during pre-alpha; the freeze applies from the first release.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, LazyLock, RwLock};

use mxm_chaos_drums_dsp as dsp;
use mxm_chaos_drums_dsp::{CELLS, CONTROLS, LfoShape, ModelId, available, curves};
use nice_plug::prelude::*;

use crate::routes::{KitRoutes, Routes};

/// The model's plain domain, fixed permanently so appending never moves a saved value.
pub const MODEL_ID_MAX: u8 = 255;

/// The tempo-sync ladder every LFO uses (the collection's, `mxm-tempo`).
pub const LFO_SYNC: mxm_tempo::Ladder =
    mxm_tempo::Ladder::new(mxm_tempo::Span::LFO, mxm_tempo::Direction::Rate);

/// A cell's general controls, nested so their IDs are `c01`…`c12` inside it.
#[derive(Params)]
pub struct ControlParams {
    #[id = "c01"]
    pub c01: FloatParam,
    #[id = "c02"]
    pub c02: FloatParam,
    #[id = "c03"]
    pub c03: FloatParam,
    #[id = "c04"]
    pub c04: FloatParam,
    #[id = "c05"]
    pub c05: FloatParam,
    #[id = "c06"]
    pub c06: FloatParam,
    #[id = "c07"]
    pub c07: FloatParam,
    #[id = "c08"]
    pub c08: FloatParam,
    #[id = "c09"]
    pub c09: FloatParam,
    #[id = "c10"]
    pub c10: FloatParam,
    #[id = "c11"]
    pub c11: FloatParam,
    #[id = "c12"]
    pub c12: FloatParam,
}

impl Default for ControlParams {
    fn default() -> Self {
        let c = |k: usize| bipolar(&format!("Control {k}"));
        Self {
            c01: c(1),
            c02: c(2),
            c03: c(3),
            c04: c(4),
            c05: c(5),
            c06: c(6),
            c07: c(7),
            c08: c(8),
            c09: c(9),
            c10: c(10),
            c11: c(11),
            c12: c(12),
        }
    }
}

impl ControlParams {
    #[must_use]
    pub fn all(&self) -> [&FloatParam; CONTROLS] {
        [
            &self.c01, &self.c02, &self.c03, &self.c04, &self.c05, &self.c06, &self.c07, &self.c08,
            &self.c09, &self.c10, &self.c11, &self.c12,
        ]
    }
}

/// What else powers a cell.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusChoice {
    #[id = "off"]
    #[name = "Off"]
    Off,
    #[id = "a"]
    #[name = "A"]
    A,
    #[id = "b"]
    #[name = "B"]
    B,
}

/// An LFO's shape.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LfoShapeChoice {
    #[id = "sine"]
    #[name = "Sine"]
    Sine,
    #[id = "triangle"]
    #[name = "Triangle"]
    Triangle,
    #[id = "ramp_up"]
    #[name = "Ramp up"]
    RampUp,
    #[id = "ramp_down"]
    #[name = "Ramp down"]
    RampDown,
    #[id = "square"]
    #[name = "Square"]
    Square,
    #[id = "sample_hold"]
    #[name = "Sample and hold"]
    SampleHold,
}

impl LfoShapeChoice {
    #[must_use]
    pub const fn dsp(self) -> LfoShape {
        match self {
            Self::Sine => LfoShape::Sine,
            Self::Triangle => LfoShape::Triangle,
            Self::RampUp => LfoShape::RampUp,
            Self::RampDown => LfoShape::RampDown,
            Self::Square => LfoShape::Square,
            Self::SampleHold => LfoShape::SampleHold,
        }
    }
}

/// One cell's parameters.
#[derive(Params)]
pub struct CellParams {
    #[id = "model"]
    pub model: IntParam,
    #[id = "coarse"]
    pub coarse: IntParam,
    #[id = "fine"]
    pub fine: FloatParam,
    /// Boxed, as the routes are, so building the surface never leans on a host thread's stack.
    #[nested]
    pub controls: Box<ControlParams>,
    #[id = "strike"]
    pub strike: FloatParam,
    #[id = "supply"]
    pub supply: FloatParam,
    #[id = "hunger"]
    pub hunger: FloatParam,
    #[id = "leak"]
    pub leak: FloatParam,
    #[id = "threshold"]
    pub threshold: FloatParam,
    #[id = "link"]
    pub link: FloatParam,
    #[id = "touch"]
    pub touch: FloatParam,
    #[id = "bus"]
    pub bus: EnumParam<BusChoice>,
    #[id = "level"]
    pub level: FloatParam,
    #[id = "pan"]
    pub pan: FloatParam,
    #[id = "mute"]
    pub mute: BoolParam,
    #[id = "solo"]
    pub solo: BoolParam,
    /// A kit setting: which cells cut each other is sound design, so presets carry it.
    #[id = "choke_group"]
    pub choke_group: IntParam,
    /// An instance setting: 0 is the main pair, 1…16 an individual output; presets never carry it.
    #[id = "output"]
    pub output: IntParam,
    #[nested]
    pub routes: Box<Routes>,
}

/// Init's kit, on the drum machine's role map so the same beat plays both instruments (the brief's
/// *Init*): the model, Coarse, Leak and choke group of each cell. Until P4 builds the brief's drums,
/// a role whose drum is not built yet plays a raw engine that stands in for it.
const INIT: [(ModelId, i32, Option<f32>, i32); CELLS] = [
    (ModelId::DRAINING_KICK, 0, None, 0),
    (ModelId::WIRED_SNARE, 0, None, 0),
    // The three toms, and the congas and bongo, rings and folds a fourth or a fifth apart.
    (ModelId::RAW_RING, -5, None, 0),
    (ModelId::RAW_RING, 0, None, 0),
    (ModelId::RAW_RING, 5, None, 0),
    (ModelId::RAW_FOLD, 0, None, 0),
    (ModelId::RAW_FOLD, 5, None, 0),
    (ModelId::RAW_LOOP, 7, None, 0),
    // Rim, clap, the hats (the closed one cuts the open one), crash, cowbell, clave, maraca.
    (ModelId::RAW_SPARK, 0, None, 0),
    (ModelId::SPARK_CLAP, 0, None, 0),
    (ModelId::RAW_DUST, 0, Some(0.2), 1),
    (ModelId::RAW_DUST, 0, Some(0.5), 1),
    (ModelId::RAW_LOOP, 12, Some(0.6), 0),
    (ModelId::RAW_FOLD, 12, None, 0),
    (ModelId::RAW_SPARK, 12, Some(0.25), 0),
    (ModelId::RAW_DUST, 5, Some(0.3), 0),
];

impl CellParams {
    fn new(index: usize) -> Self {
        let (model, coarse, leak, choke) = INIT[index];
        let defaults = dsp::CellParams::new(model);
        Self {
            model: model_param(model.raw()),
            coarse: IntParam::new("Coarse", coarse, IntRange::Linear { min: -24, max: 24 })
                .with_unit(" st"),
            fine: FloatParam::new(
                "Fine",
                0.0,
                FloatRange::Linear {
                    min: -100.0,
                    max: 100.0,
                },
            )
            .with_unit(" ct")
            // Whole cents, and never a negative zero.
            .with_value_to_string(Arc::new(|cents| format!("{:.0}", cents.round() + 0.0))),
            controls: Box::default(),
            strike: amount("Strike", defaults.strike),
            supply: amount("Supply", defaults.supply),
            hunger: amount("Hunger", defaults.hunger),
            leak: time(
                "Leak",
                leak.unwrap_or(defaults.leak),
                curves::leak_ms,
                curves::LEAK_MS,
                curves::LEAK_OCTAVES,
            ),
            threshold: bipolar("Threshold"),
            link: amount("Link", defaults.link),
            touch: amount("Touch", defaults.touch),
            bus: EnumParam::new("Bus", BusChoice::Off),
            level: gain("Level", 0.0),
            pan: pan_param(),
            mute: BoolParam::new("Mute", false),
            solo: BoolParam::new("Solo", false),
            choke_group: choke_group_param(choke),
            output: output_param(),
            routes: Box::default(),
        }
    }

    #[must_use]
    pub fn model_id(&self) -> ModelId {
        ModelId::new(self.model.value().clamp(0, i32::from(MODEL_ID_MAX)) as u8)
    }

    /// The DSP's settings. A stereo-only host folds every cell to the main pair.
    #[must_use]
    pub fn dsp(&self, multi_output: bool) -> dsp::CellParams {
        dsp::CellParams {
            model: self.model_id(),
            coarse: self.coarse.value().clamp(-24, 24) as i8,
            fine: self.fine.value(),
            controls: self.controls.all().map(FloatParam::value),
            strike: self.strike.value(),
            supply: self.supply.value(),
            hunger: self.hunger.value(),
            leak: self.leak.value(),
            threshold: self.threshold.value(),
            link: self.link.value(),
            touch: self.touch.value(),
            bus: match self.bus.value() {
                BusChoice::Off => dsp::Bus::Off,
                BusChoice::A => dsp::Bus::A,
                BusChoice::B => dsp::Bus::B,
            },
            level: self.level.value(),
            pan: self.pan.value(),
            mute: self.mute.value(),
            solo: self.solo.value(),
            choke_group: self.choke_group.value().clamp(0, 16) as u8,
            output: if multi_output {
                self.output.value().clamp(0, 16) as u8
            } else {
                0
            },
            routes: self.routes.dsp(),
        }
    }

    /// The cell's parameters in declaration order, beside [`cell_ids`].
    #[must_use]
    pub fn parameters(&self) -> Vec<&dyn mxm_params::ErasedParam> {
        let mut out: Vec<&dyn mxm_params::ErasedParam> =
            vec![&self.model, &self.coarse, &self.fine];
        out.extend(
            self.controls
                .all()
                .map(|c| c as &dyn mxm_params::ErasedParam),
        );
        out.extend([
            &self.strike as &dyn mxm_params::ErasedParam,
            &self.supply,
            &self.hunger,
            &self.leak,
            &self.threshold,
            &self.link,
            &self.touch,
            &self.bus,
            &self.level,
            &self.pan,
            &self.mute,
            &self.solo,
            &self.choke_group,
            &self.output,
        ]);
        for route in self.routes.all() {
            out.extend([
                &route.source as &dyn mxm_params::ErasedParam,
                &route.target,
                &route.amount,
            ]);
        }
        out
    }
}

#[derive(Params)]
pub struct MxmChaosDrumsParams {
    #[nested(array, group = "Cell")]
    pub cells: [CellParams; CELLS],
    #[id = "temper"]
    pub temper: FloatParam,
    #[id = "flow"]
    pub flow: FloatParam,
    #[id = "coupling"]
    pub coupling: FloatParam,
    #[id = "pool"]
    pub pool: FloatParam,
    #[id = "recharge"]
    pub recharge: FloatParam,
    #[id = "bus_a"]
    pub bus_a: FloatParam,
    #[id = "bus_b"]
    pub bus_b: FloatParam,
    #[id = "seed"]
    pub seed: IntParam,
    #[id = "reset_on_play"]
    pub reset_on_play: BoolParam,
    #[id = "dynamics"]
    pub dynamics: FloatParam,
    #[id = "master"]
    pub master: FloatParam,
    #[id = "lfo1_rate"]
    pub lfo1_rate: FloatParam,
    #[id = "lfo1_shape"]
    pub lfo1_shape: EnumParam<LfoShapeChoice>,
    #[id = "lfo1_sync"]
    pub lfo1_sync: BoolParam,
    #[id = "lfo2_rate"]
    pub lfo2_rate: FloatParam,
    #[id = "lfo2_shape"]
    pub lfo2_shape: EnumParam<LfoShapeChoice>,
    #[id = "lfo2_sync"]
    pub lfo2_sync: BoolParam,
    #[id = "lfo3_rate"]
    pub lfo3_rate: FloatParam,
    #[id = "lfo3_shape"]
    pub lfo3_shape: EnumParam<LfoShapeChoice>,
    #[id = "lfo3_sync"]
    pub lfo3_sync: BoolParam,
    #[nested]
    pub kit_routes: Box<KitRoutes>,
    #[persist = "preset"]
    pub preset: RwLock<mxm_preset::PresetIdentity>,
    /// The names typed over the cells' made-up ones, one a cell, empty where none is
    /// (`crate::slot_names`). The song's, never a kit's.
    #[persist = "slot-names"]
    pub slot_names: RwLock<Vec<String>>,
    /// Raised by every write of `slot_names`; the editor lowers it and tells the host. Not saved.
    pub names_written: AtomicBool,
}

impl Default for MxmChaosDrumsParams {
    fn default() -> Self {
        let kit = dsp::KitParams::default();
        Self {
            cells: std::array::from_fn(CellParams::new),
            temper: amount("Temper", kit.temper),
            flow: amount("Flow", kit.flow),
            coupling: amount("Coupling", kit.coupling),
            pool: amount("Pool", kit.pool),
            recharge: time(
                "Recharge",
                kit.recharge,
                curves::recharge_ms,
                curves::RECHARGE_MS,
                curves::RECHARGE_OCTAVES,
            ),
            bus_a: amount("Bus A", 0.0),
            bus_b: amount("Bus B", 0.0),
            seed: IntParam::new("Seed", 1, IntRange::Linear { min: 1, max: 9999 }),
            // On by default (the owner, 2026-11-02): every play starts from rest.
            reset_on_play: BoolParam::new("Reset on play", true),
            dynamics: bipolar("Dynamics"),
            master: gain("Master", 0.0),
            // 0.7, 1.3 and 4 Hz, as the drum machine's, as amounts along `curves::lfo_hz`.
            lfo1_rate: lfo_rate("LFO 1 rate", 0.4405),
            lfo1_shape: EnumParam::new("LFO 1 shape", LfoShapeChoice::Sine),
            lfo1_sync: BoolParam::new("LFO 1 sync", false),
            lfo2_rate: lfo_rate("LFO 2 rate", 0.5438),
            lfo2_shape: EnumParam::new("LFO 2 shape", LfoShapeChoice::Triangle),
            lfo2_sync: BoolParam::new("LFO 2 sync", false),
            lfo3_rate: lfo_rate("LFO 3 rate", 0.7314),
            lfo3_shape: EnumParam::new("LFO 3 shape", LfoShapeChoice::Sine),
            lfo3_sync: BoolParam::new("LFO 3 sync", false),
            kit_routes: Box::default(),
            preset: RwLock::new(mxm_preset::PresetIdentity::default()),
            slot_names: RwLock::new(mxm_drum_shell::slot_names::empty()),
            names_written: AtomicBool::new(false),
        }
    }
}

impl MxmChaosDrumsParams {
    /// The LFOs' rate, shape and sync parameters.
    #[must_use]
    pub fn lfos(&self) -> [(&FloatParam, &EnumParam<LfoShapeChoice>, &BoolParam); 3] {
        [
            (&self.lfo1_rate, &self.lfo1_shape, &self.lfo1_sync),
            (&self.lfo2_rate, &self.lfo2_shape, &self.lfo2_sync),
            (&self.lfo3_rate, &self.lfo3_shape, &self.lfo3_sync),
        ]
    }

    /// The DSP's kit settings, each LFO at `lfo_hz` (its free rate, or the division its sync chose).
    #[must_use]
    pub fn kit(&self, lfo_hz: [f32; 3]) -> dsp::KitParams {
        dsp::KitParams {
            temper: self.temper.value(),
            flow: self.flow.value(),
            coupling: self.coupling.value(),
            pool: self.pool.value(),
            recharge: self.recharge.value(),
            bus: [self.bus_a.value(), self.bus_b.value()],
            seed: self.seed.value().max(1) as u32,
            dynamics: self.dynamics.value(),
            master: self.master.value(),
            lfo_rate_hz: lfo_hz,
            lfo_shape: self.lfos().map(|(_, shape, _)| shape.value().dsp()),
            routes: self.kit_routes.dsp(),
        }
    }

    /// Each LFO's rate for a block: its sync's division at the host's tempo, or its free rate.
    #[must_use]
    pub fn lfo_hz(&self, tempo: Option<f64>) -> [f32; 3] {
        let (lo, hi) = (f64::from(curves::LFO_HZ.0), f64::from(curves::LFO_HZ.1));
        self.lfos().map(|(rate, _, sync)| {
            LFO_SYNC
                .resolve(sync.value(), tempo, rate.value(), lo, hi)
                .map_or_else(|| curves::lfo_hz(rate.value()), |hz| hz as f32)
        })
    }

    /// The kit-wide parameters in declaration order, beside [`KIT_IDS`].
    #[must_use]
    pub fn globals(&self) -> Vec<&dyn mxm_params::ErasedParam> {
        let mut out: Vec<&dyn mxm_params::ErasedParam> = vec![
            &self.temper,
            &self.flow,
            &self.coupling,
            &self.pool,
            &self.recharge,
            &self.bus_a,
            &self.bus_b,
            &self.seed,
            &self.reset_on_play,
            &self.dynamics,
            &self.master,
        ];
        for (rate, shape, sync) in self.lfos() {
            out.extend([rate as &dyn mxm_params::ErasedParam, shape, sync]);
        }
        for route in self.kit_routes.all() {
            out.extend([
                &route.source as &dyn mxm_params::ErasedParam,
                &route.target,
                &route.amount,
            ]);
        }
        out
    }
}

/// A cell's suffixes in declaration order, beside [`CellParams::parameters`].
fn cell_suffixes() -> Vec<String> {
    let mut out: Vec<String> = ["model", "coarse", "fine"].map(str::to_owned).to_vec();
    out.extend((1..=CONTROLS).map(|k| format!("c{k:02}")));
    out.extend(
        [
            "strike",
            "supply",
            "hunger",
            "leak",
            "threshold",
            "link",
            "touch",
            "bus",
            "level",
            "pan",
            "mute",
            "solo",
            "choke_group",
            "output",
        ]
        .map(str::to_owned),
    );
    for r in 1..=dsp::ROUTES {
        out.extend(["source", "target", "amount"].map(|field| format!("route{r}_{field}")));
    }
    out
}

/// Every cell's IDs, in declaration order. Made once and kept, because presets take `&'static str`.
static CELL_IDS: LazyLock<Vec<Vec<&'static str>>> = LazyLock::new(|| {
    (1..=CELLS)
        .map(|n| {
            cell_suffixes()
                .into_iter()
                .map(|suffix| &*Box::leak(format!("{suffix}_{n}").into_boxed_str()))
                .collect()
        })
        .collect()
});

/// The kit-wide IDs in declaration order.
pub static KIT_IDS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    let mut out = vec![
        "temper",
        "flow",
        "coupling",
        "pool",
        "recharge",
        "bus_a",
        "bus_b",
        "seed",
        "reset_on_play",
        "dynamics",
        "master",
    ];
    for n in 1..=3 {
        for field in ["rate", "shape", "sync"] {
            out.push(Box::leak(format!("lfo{n}_{field}").into_boxed_str()));
        }
    }
    for r in 1..=dsp::ROUTES {
        for field in ["source", "target", "amount"] {
            out.push(Box::leak(format!("kroute{r}_{field}").into_boxed_str()));
        }
    }
    out
});

/// Cell `cell`'s (0-based) IDs, in declaration order.
#[must_use]
pub fn cell_ids(cell: usize) -> &'static [&'static str] {
    &CELL_IDS[cell]
}

/// The cell and route (0-based) a route ID belongs to, or the kit route as `(CELLS, r)`.
#[must_use]
pub fn route_of(id: &str) -> Option<(usize, usize)> {
    let route = |rest: &str, prefix: &str| -> Option<usize> {
        let rest = rest.strip_prefix(prefix)?;
        let r: usize = rest.get(..1)?.parse().ok()?;
        (1..=dsp::ROUTES).contains(&r).then_some(r - 1)
    };
    if let Some(r) = route(id, "kroute") {
        return Some((CELLS, r));
    }
    let r = route(id, "route")?;
    let cell: usize = id.rsplit('_').next()?.parse().ok()?;
    (1..=CELLS).contains(&cell).then_some((cell - 1, r))
}

fn model_param(default: u8) -> IntParam {
    IntParam::new(
        "Model",
        i32::from(default),
        IntRange::Linear {
            min: 0,
            max: i32::from(MODEL_ID_MAX),
        },
    )
    .with_value_to_string(Arc::new(|plain| {
        let id = ModelId::new(plain.clamp(0, i32::from(MODEL_ID_MAX)) as u8);
        available(id).map_or_else(
            || {
                if id == ModelId::NONE {
                    "Off".to_owned()
                } else {
                    format!("Unavailable {}", id.raw())
                }
            },
            |spec| spec.label.to_owned(),
        )
    }))
    .with_string_to_value(Arc::new(|text| {
        let text = text.trim();
        dsp::AVAILABLE_MODELS
            .iter()
            .find(|spec| spec.label.eq_ignore_ascii_case(text))
            .map(|spec| i32::from(spec.id.raw()))
            .or_else(|| text.eq_ignore_ascii_case("off").then_some(0))
            .or_else(|| {
                text.strip_prefix("Unavailable ")
                    .unwrap_or(text)
                    .parse()
                    .ok()
            })
    }))
}

/// A 0…100% amount.
fn amount(name: &str, default: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 1.0 })
        .with_value_to_string(mxm_params::text::whole_percentage())
        .with_string_to_value(formatters::s2v_f32_percentage())
}

/// A −100…+100% amount; zero is the model's own setting.
fn bipolar(name: &str) -> FloatParam {
    FloatParam::new(
        name,
        0.0,
        FloatRange::Linear {
            min: -1.0,
            max: 1.0,
        },
    )
    .with_value_to_string(mxm_params::text::whole_percentage())
    .with_string_to_value(formatters::s2v_f32_percentage())
}

/// Milliseconds below a second, seconds from what rounds to one, so a reading never names 1000 ms.
fn milliseconds_text(ms: f32) -> String {
    if ms.round() < 1000.0 {
        format!("{ms:.0} ms")
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

/// A typed time in milliseconds: `350`, `350 ms`, `1.2 s`.
fn parse_milliseconds(text: &str) -> Option<f32> {
    let text = text.trim().to_ascii_lowercase();
    if let Some(ms) = text.strip_suffix("ms") {
        ms.trim().parse().ok()
    } else if let Some(s) = text.strip_suffix('s') {
        s.trim().parse::<f32>().ok().map(|s| s * 1000.0)
    } else {
        text.parse().ok()
    }
}

/// A time as an amount along `curve`, read in milliseconds and seconds. Typing a time finds the
/// amount that plays it; that runs on the editor's side only, so the platform's logarithm is fine.
fn time(
    name: &str,
    default: f32,
    curve: fn(f32) -> f32,
    range: (f32, f32),
    octaves: f32,
) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 1.0 })
        .with_value_to_string(Arc::new(move |amount| milliseconds_text(curve(amount))))
        .with_string_to_value(Arc::new(move |text| {
            let ms = parse_milliseconds(text)?.clamp(range.0, range.1);
            Some(((ms / range.0).log2() / octaves).clamp(0.0, 1.0))
        }))
}

/// An LFO's rate as an amount along `curves::lfo_hz`; a typed division picks its place on the
/// sync ladder.
fn lfo_rate(name: &str, default: f32) -> FloatParam {
    let hz_parser = formatters::s2v_f32_hz_then_khz();
    FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 1.0 })
        .with_value_to_string(Arc::new(|amount| {
            format!("{:.2} Hz", curves::lfo_hz(amount))
        }))
        .with_string_to_value(Arc::new(move |text| {
            if let Some(division) =
                mxm_tempo::Division::parse(text).filter(|d| LFO_SYNC.span.contains(*d))
            {
                return Some(LFO_SYNC.position(division));
            }
            let hz = hz_parser(text)?.clamp(curves::LFO_HZ.0, curves::LFO_HZ.1);
            Some(((hz / curves::LFO_HZ.0).log2() / curves::LFO_OCTAVES).clamp(0.0, 1.0))
        }))
}

fn pan_param() -> FloatParam {
    FloatParam::new(
        "Pan",
        0.0,
        FloatRange::Linear {
            min: -1.0,
            max: 1.0,
        },
    )
    .with_value_to_string(Arc::new(|value| {
        if value.abs() < 0.005 {
            "Centre".to_owned()
        } else if value < 0.0 {
            format!("L {:.0}%", -100.0 * value)
        } else {
            format!("R {:.0}%", 100.0 * value)
        }
    }))
    .with_string_to_value(Arc::new(|text| {
        let text = text.trim().to_ascii_lowercase();
        if text == "centre" || text == "center" || text == "c" {
            return Some(0.0);
        }
        let (number, sign) = if let Some(number) = text.strip_prefix('l') {
            (number, -1.0)
        } else if let Some(number) = text.strip_prefix('r') {
            (number, 1.0)
        } else {
            (text.as_str(), 1.0)
        };
        number
            .trim()
            .trim_end_matches('%')
            .trim()
            .parse::<f32>()
            .ok()
            .map(|value| sign * value / 100.0)
    }))
}

fn output_param() -> IntParam {
    IntParam::new("Output", 0, IntRange::Linear { min: 0, max: 16 })
        .with_value_to_string(Arc::new(|value| {
            if value == 0 {
                "L+R".to_owned()
            } else {
                value.to_string()
            }
        }))
        .with_string_to_value(Arc::new(|text| {
            let text = text.trim();
            if text.eq_ignore_ascii_case("L+R") || text.eq_ignore_ascii_case("main") {
                Some(0)
            } else {
                text.strip_prefix("Output ")
                    .or_else(|| text.strip_prefix("Cell "))
                    .unwrap_or(text)
                    .parse()
                    .ok()
            }
        }))
}

fn choke_group_param(default: i32) -> IntParam {
    IntParam::new("Choke group", default, IntRange::Linear { min: 0, max: 16 })
        .with_value_to_string(Arc::new(|value| {
            if value == 0 {
                "Off".to_owned()
            } else {
                value.to_string()
            }
        }))
        .with_string_to_value(Arc::new(|text| {
            let text = text.trim();
            if text.eq_ignore_ascii_case("off") || text.eq_ignore_ascii_case("none") {
                Some(0)
            } else {
                text.strip_prefix("Group ").unwrap_or(text).parse().ok()
            }
        }))
}

/// Linear gain, skewed in dB as the collection's: it acts after the performance.
fn gain(name: &str, default_db: f32) -> FloatParam {
    FloatParam::new(
        name,
        util::db_to_gain(default_db),
        FloatRange::Skewed {
            min: util::db_to_gain(-60.0),
            max: util::db_to_gain(6.0),
            factor: FloatRange::gain_skew_factor(-60.0, 6.0),
        },
    )
    .with_unit(" dB")
    .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
    .with_string_to_value(formatters::s2v_f32_gain_to_db())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_parameter_text_survives_the_hosts_round_trip() {
        mxm_plugin_test::text_checks::every_text_round_trips(&MxmChaosDrumsParams::default(), &[]);
    }

    #[test]
    fn each_cell_lists_its_41_parameters_under_their_ids() {
        let params = MxmChaosDrumsParams::default();
        for (cell, ids) in params.cells.iter().zip(0..CELLS) {
            assert_eq!(cell.parameters().len(), 41);
            assert_eq!(cell_ids(ids).len(), 41);
        }
        assert_eq!(params.globals().len(), KIT_IDS.len());
        assert_eq!(cell_ids(0)[0], "model_1");
        assert_eq!(cell_ids(15)[40], "route4_amount_16");
        assert_eq!(route_of("route3_target_12"), Some((11, 2)));
        assert_eq!(route_of("kroute2_amount"), Some((CELLS, 1)));
        assert_eq!(route_of("level_3"), None);
    }

    #[test]
    fn the_lfo_defaults_are_the_drum_machines_rates() {
        let params = MxmChaosDrumsParams::default();
        let hz = params.lfo_hz(None);
        for (got, wanted) in hz.iter().zip([0.7, 1.3, 4.0]) {
            assert!((got - wanted).abs() < 0.01, "{got} Hz, wanted {wanted}");
        }
    }
}

/// Where each of a cell's parameters sits in [`cell_ids`] and [`CellParams::parameters`].
pub mod at {
    pub const MODEL: usize = 0;
    pub const COARSE: usize = 1;
    pub const FINE: usize = 2;
    pub const STRIKE: usize = 15;
    pub const SUPPLY: usize = 16;
    pub const HUNGER: usize = 17;
    pub const LEAK: usize = 18;
    pub const THRESHOLD: usize = 19;
    pub const LINK: usize = 20;
    pub const TOUCH: usize = 21;
    pub const BUS: usize = 22;
    pub const LEVEL: usize = 23;
    pub const PAN: usize = 24;
    pub const MUTE: usize = 25;
    pub const SOLO: usize = 26;
    pub const CHOKE_GROUP: usize = 27;
    pub const OUTPUT: usize = 28;

    /// Control `k`, 1…12.
    #[must_use]
    pub const fn control(k: usize) -> usize {
        2 + k
    }

    /// Route `r`'s (0-based) source, target and amount.
    #[must_use]
    pub const fn route(r: usize) -> [usize; 3] {
        [29 + 3 * r, 30 + 3 * r, 31 + 3 * r]
    }
}
