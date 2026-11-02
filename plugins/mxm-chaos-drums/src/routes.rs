//! The route parameters: four a cell and four for the kit, each a source, a target and an amount.

use std::sync::Arc;

use mxm_chaos_drums_dsp::routes::{CellTarget, KitTarget, Route, Source};
use mxm_chaos_drums_dsp::{CONTROLS, ROUTES};
use nice_plug::prelude::*;

/// A cell route's source.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceChoice {
    #[id = "off"]
    #[name = "Off"]
    Off,
    #[id = "lfo1"]
    #[name = "LFO 1"]
    Lfo1,
    #[id = "lfo2"]
    #[name = "LFO 2"]
    Lfo2,
    #[id = "lfo3"]
    #[name = "LFO 3"]
    Lfo3,
    #[id = "wheel"]
    #[name = "Wheel"]
    Wheel,
    #[id = "pressure"]
    #[name = "Pressure"]
    Pressure,
    #[id = "velocity"]
    #[name = "Velocity"]
    Velocity,
    #[id = "random"]
    #[name = "Random"]
    Random,
}

impl SourceChoice {
    #[must_use]
    pub const fn dsp(self) -> Source {
        match self {
            Self::Off => Source::Off,
            Self::Lfo1 => Source::Lfo1,
            Self::Lfo2 => Source::Lfo2,
            Self::Lfo3 => Source::Lfo3,
            Self::Wheel => Source::Wheel,
            Self::Pressure => Source::Pressure,
            Self::Velocity => Source::Velocity,
            Self::Random => Source::Random,
        }
    }
}

/// A kit route's source: the kit has no Velocity and no Random, which are a hit's.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum KitSourceChoice {
    #[id = "off"]
    #[name = "Off"]
    Off,
    #[id = "lfo1"]
    #[name = "LFO 1"]
    Lfo1,
    #[id = "lfo2"]
    #[name = "LFO 2"]
    Lfo2,
    #[id = "lfo3"]
    #[name = "LFO 3"]
    Lfo3,
    #[id = "wheel"]
    #[name = "Wheel"]
    Wheel,
    #[id = "pressure"]
    #[name = "Pressure"]
    Pressure,
}

impl KitSourceChoice {
    #[must_use]
    pub const fn dsp(self) -> Source {
        match self {
            Self::Off => Source::Off,
            Self::Lfo1 => Source::Lfo1,
            Self::Lfo2 => Source::Lfo2,
            Self::Lfo3 => Source::Lfo3,
            Self::Wheel => Source::Wheel,
            Self::Pressure => Source::Pressure,
        }
    }
}

/// A cell route's targets, by the target parameter's value: Off, Pitch, the twelve controls, then
/// the charge controls, Level and Pan.
pub const CELL_TARGETS: usize = 2 + CONTROLS + 9;

/// The targets after the controls, in order.
const AFTER_CONTROLS: [CellTarget; 9] = [
    CellTarget::Strike,
    CellTarget::Supply,
    CellTarget::Hunger,
    CellTarget::Leak,
    CellTarget::Threshold,
    CellTarget::Link,
    CellTarget::Touch,
    CellTarget::Level,
    CellTarget::Pan,
];

/// The cell target a target parameter's value names.
#[must_use]
pub fn cell_target(index: i32) -> CellTarget {
    match index.max(0) as usize {
        0 => CellTarget::Off,
        1 => CellTarget::Pitch,
        i if i < 2 + CONTROLS => CellTarget::Control((i - 2) as u8),
        i => AFTER_CONTROLS
            .get(i - 2 - CONTROLS)
            .copied()
            .unwrap_or(CellTarget::Off),
    }
}

/// A cell target's name, as the host and the panel show it.
#[must_use]
pub fn cell_target_name(index: i32) -> String {
    match cell_target(index) {
        CellTarget::Off => "Off".to_owned(),
        CellTarget::Pitch => "Pitch".to_owned(),
        CellTarget::Control(k) => format!("Control {}", k + 1),
        CellTarget::Strike => "Strike".to_owned(),
        CellTarget::Supply => "Supply".to_owned(),
        CellTarget::Hunger => "Hunger".to_owned(),
        CellTarget::Leak => "Leak".to_owned(),
        CellTarget::Threshold => "Threshold".to_owned(),
        CellTarget::Link => "Link".to_owned(),
        CellTarget::Touch => "Touch".to_owned(),
        CellTarget::Level => "Level".to_owned(),
        CellTarget::Pan => "Pan".to_owned(),
    }
}

/// The kit targets, by the target parameter's value.
pub const KIT_TARGETS: [(KitTarget, &str); 8] = [
    (KitTarget::Off, "Off"),
    (KitTarget::Temper, "Temper"),
    (KitTarget::Flow, "Flow"),
    (KitTarget::Coupling, "Coupling"),
    (KitTarget::Pool, "Pool"),
    (KitTarget::Recharge, "Recharge"),
    (KitTarget::BusA, "Bus A"),
    (KitTarget::BusB, "Bus B"),
];

fn amount(r: usize) -> FloatParam {
    FloatParam::new(
        format!("Route {r} amount"),
        0.0,
        FloatRange::Linear {
            min: -1.0,
            max: 1.0,
        },
    )
    .with_value_to_string(mxm_params::text::whole_percentage())
    .with_string_to_value(formatters::s2v_f32_percentage())
}

fn target(r: usize, count: usize, name: Arc<dyn Fn(i32) -> String + Send + Sync>) -> IntParam {
    let parse = name.clone();
    IntParam::new(
        format!("Route {r} target"),
        0,
        IntRange::Linear {
            min: 0,
            max: count as i32 - 1,
        },
    )
    .with_value_to_string(name)
    .with_string_to_value(Arc::new(move |text| {
        let text = text.trim();
        (0..count as i32)
            .find(|&i| parse(i).eq_ignore_ascii_case(text))
            .or_else(|| text.parse().ok())
    }))
}

/// One cell route.
#[derive(Params)]
pub struct RouteParams {
    #[id = "source"]
    pub source: EnumParam<SourceChoice>,
    #[id = "target"]
    pub target: IntParam,
    #[id = "amount"]
    pub amount: FloatParam,
}

impl RouteParams {
    fn new(r: usize) -> Self {
        Self {
            source: EnumParam::new(format!("Route {r} source"), SourceChoice::Off),
            target: target(r, CELL_TARGETS, Arc::new(cell_target_name)),
            amount: amount(r),
        }
    }

    #[must_use]
    pub fn dsp(&self) -> Route<CellTarget> {
        Route {
            source: self.source.value().dsp(),
            target: cell_target(self.target.value()),
            amount: self.amount.value(),
        }
    }

    /// In use: neither source nor target Off.
    #[must_use]
    pub fn in_use(&self) -> bool {
        self.source.value() != SourceChoice::Off && self.target.value() != 0
    }
}

/// A cell's four routes, their IDs `route1_source`…`route4_amount` inside the cell.
#[derive(Params)]
pub struct Routes {
    #[nested(id_prefix = "route1")]
    pub r1: RouteParams,
    #[nested(id_prefix = "route2")]
    pub r2: RouteParams,
    #[nested(id_prefix = "route3")]
    pub r3: RouteParams,
    #[nested(id_prefix = "route4")]
    pub r4: RouteParams,
}

impl Default for Routes {
    fn default() -> Self {
        Self {
            r1: RouteParams::new(1),
            r2: RouteParams::new(2),
            r3: RouteParams::new(3),
            r4: RouteParams::new(4),
        }
    }
}

impl Routes {
    #[must_use]
    pub fn all(&self) -> [&RouteParams; ROUTES] {
        [&self.r1, &self.r2, &self.r3, &self.r4]
    }

    #[must_use]
    pub fn dsp(&self) -> [Route<CellTarget>; ROUTES] {
        self.all().map(RouteParams::dsp)
    }
}

/// One kit route.
#[derive(Params)]
pub struct KitRouteParams {
    #[id = "source"]
    pub source: EnumParam<KitSourceChoice>,
    #[id = "target"]
    pub target: IntParam,
    #[id = "amount"]
    pub amount: FloatParam,
}

impl KitRouteParams {
    fn new(r: usize) -> Self {
        Self {
            source: EnumParam::new(format!("Kit route {r} source"), KitSourceChoice::Off),
            target: target(
                r,
                KIT_TARGETS.len(),
                Arc::new(|i| {
                    KIT_TARGETS
                        .get(i.max(0) as usize)
                        .map_or("Off", |(_, name)| name)
                        .to_owned()
                }),
            ),
            amount: amount(r),
        }
    }

    #[must_use]
    pub fn dsp(&self) -> Route<KitTarget> {
        Route {
            source: self.source.value().dsp(),
            target: KIT_TARGETS
                .get(self.target.value().max(0) as usize)
                .map_or(KitTarget::Off, |(t, _)| *t),
            amount: self.amount.value(),
        }
    }

    #[must_use]
    pub fn in_use(&self) -> bool {
        self.source.value() != KitSourceChoice::Off && self.target.value() != 0
    }
}

/// The kit's four routes, their IDs `kroute1_source`…`kroute4_amount`.
#[derive(Params)]
pub struct KitRoutes {
    #[nested(id_prefix = "kroute1")]
    pub r1: KitRouteParams,
    #[nested(id_prefix = "kroute2")]
    pub r2: KitRouteParams,
    #[nested(id_prefix = "kroute3")]
    pub r3: KitRouteParams,
    #[nested(id_prefix = "kroute4")]
    pub r4: KitRouteParams,
}

impl Default for KitRoutes {
    fn default() -> Self {
        Self {
            r1: KitRouteParams::new(1),
            r2: KitRouteParams::new(2),
            r3: KitRouteParams::new(3),
            r4: KitRouteParams::new(4),
        }
    }
}

impl KitRoutes {
    #[must_use]
    pub fn all(&self) -> [&KitRouteParams; ROUTES] {
        [&self.r1, &self.r2, &self.r3, &self.r4]
    }

    #[must_use]
    pub fn dsp(&self) -> [Route<KitTarget>; ROUTES] {
        self.all().map(KitRouteParams::dsp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cell_target_has_one_index_and_a_name() {
        let names: Vec<String> = (0..CELL_TARGETS as i32).map(cell_target_name).collect();
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), CELL_TARGETS, "{names:?}");
        assert_eq!(cell_target(0), CellTarget::Off);
        assert_eq!(cell_target(CELL_TARGETS as i32 - 1), CellTarget::Pan);
        assert_eq!(cell_target(CELL_TARGETS as i32), CellTarget::Off);
    }
}
