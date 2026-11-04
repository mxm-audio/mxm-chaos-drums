//! Modulation: four routes a cell and four for the kit, each a source, a target and an amount.
//!
//! The sources mean what the collection's standard says they mean (`mxm_modulation::standard`):
//! Velocity is `v − 1`, Wheel and Pressure 0…1, Random one bipolar draw a hit, the LFOs bipolar. A
//! route the instrument adds reaches what the standard's `reach` says: an octave of pitch, four
//! octaves of a time, a control's whole range, the amplitude factor's whole swing. Several routes on
//! one target add, and the target's own range clamps once.

use mxm_modulation::standard::{self, reach};

/// Routes a cell has, and the kit has.
pub const ROUTES: usize = 4;
/// General model controls a cell has.
pub const CONTROLS: usize = 12;

/// Where a route's value comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    #[default]
    Off,
    Lfo1,
    Lfo2,
    Lfo3,
    Wheel,
    Pressure,
    /// Per cell only: the last hit's velocity, `v − 1`.
    Velocity,
    /// Per cell only: one bipolar draw a hit, from the cell's seeded stream.
    Random,
}

/// What a cell's route moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CellTarget {
    #[default]
    Off,
    Pitch,
    /// A general control, 0-based.
    Control(u8),
    Strike,
    Supply,
    Hunger,
    Leak,
    Threshold,
    Link,
    Touch,
    Level,
    Pan,
}

/// What a kit route moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KitTarget {
    #[default]
    Off,
    Temper,
    Flow,
    Coupling,
    Pool,
    Recharge,
    BusA,
    BusB,
}

/// One route. In use when neither its source nor its target is Off; an amount of zero is still a
/// route.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Route<T> {
    pub source: Source,
    pub target: T,
    /// −1…+1.
    pub amount: f32,
}

/// The kit-wide source values this sample.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Sources {
    pub lfo: [f32; 3],
    pub wheel: f32,
    pub pressure: f32,
}

impl Sources {
    /// A source's value; `velocity` and `random` are the cell's, zero for the kit.
    fn value(&self, source: Source, velocity: f32, random: f32) -> f32 {
        match source {
            Source::Off => 0.0,
            Source::Lfo1 => self.lfo[0],
            Source::Lfo2 => self.lfo[1],
            Source::Lfo3 => self.lfo[2],
            Source::Wheel => standard::wheel(self.wheel),
            Source::Pressure => standard::pressure(self.pressure),
            Source::Velocity => velocity,
            Source::Random => random,
        }
    }
}

/// What a cell's routes add this sample, each in its target's units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CellMods {
    pub pitch_semitones: f32,
    pub controls: [f32; CONTROLS],
    pub strike: f32,
    pub supply: f32,
    pub hunger: f32,
    /// Octaves of Leak time.
    pub leak_octaves: f32,
    pub threshold: f32,
    pub link: f32,
    pub touch: f32,
    /// The amplitude factor's sum (`standard::amplitude_factor`).
    pub level: f32,
    pub pan: f32,
}

/// Whether any of `routes` is in use.
pub(crate) fn any_in_use<T: PartialEq + Default + Copy>(routes: &[Route<T>; ROUTES]) -> bool {
    routes
        .iter()
        .any(|route| route.source != Source::Off && route.target != T::default())
}

/// Sum a cell's routes. `velocity` is the standard's `v − 1`, `random` the cell's draw.
pub(crate) fn cell_mods(
    routes: &[Route<CellTarget>; ROUTES],
    sources: &Sources,
    velocity: f32,
    random: f32,
) -> CellMods {
    let mut mods = CellMods::default();
    for route in routes {
        if route.source == Source::Off {
            continue;
        }
        let v = route.amount.clamp(-1.0, 1.0) * sources.value(route.source, velocity, random);
        match route.target {
            CellTarget::Off => {}
            CellTarget::Pitch => mods.pitch_semitones += v * reach::PITCH_SEMITONES,
            CellTarget::Control(index) => {
                if let Some(control) = mods.controls.get_mut(usize::from(index)) {
                    *control += v * reach::CONTROL;
                }
            }
            CellTarget::Strike => mods.strike += v * reach::CONTROL,
            CellTarget::Supply => mods.supply += v * reach::CONTROL,
            CellTarget::Hunger => mods.hunger += v * reach::CONTROL,
            CellTarget::Leak => mods.leak_octaves += v * reach::OCTAVES,
            CellTarget::Threshold => mods.threshold += v * reach::CONTROL,
            CellTarget::Link => mods.link += v * reach::CONTROL,
            CellTarget::Touch => mods.touch += v * reach::CONTROL,
            CellTarget::Level => mods.level += v * reach::AMPLITUDE,
            CellTarget::Pan => mods.pan += v * reach::CONTROL,
        }
    }
    mods
}

/// What the kit's routes add this sample.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct KitMods {
    pub temper: f32,
    pub flow: f32,
    pub coupling: f32,
    pub pool: f32,
    pub recharge_octaves: f32,
    pub bus: [f32; 2],
}

pub(crate) fn kit_mods(routes: &[Route<KitTarget>; ROUTES], sources: &Sources) -> KitMods {
    let mut mods = KitMods::default();
    for route in routes {
        // Velocity and Random belong to a cell; the kit has neither.
        if matches!(
            route.source,
            Source::Off | Source::Velocity | Source::Random
        ) {
            continue;
        }
        let v = route.amount.clamp(-1.0, 1.0) * sources.value(route.source, 0.0, 0.0);
        match route.target {
            KitTarget::Off => {}
            KitTarget::Temper => mods.temper += v * reach::CONTROL,
            KitTarget::Flow => mods.flow += v * reach::CONTROL,
            KitTarget::Coupling => mods.coupling += v * reach::CONTROL,
            KitTarget::Pool => mods.pool += v * reach::CONTROL,
            KitTarget::Recharge => mods.recharge_octaves += v * reach::OCTAVES,
            KitTarget::BusA => mods.bus[0] += v * reach::CONTROL,
            KitTarget::BusB => mods.bus[1] += v * reach::CONTROL,
        }
    }
    mods
}

/// Which LFOs reach a target through `routes` with a net amount: routes that cancel reach nothing,
/// and neither does a route to `off`.
pub(crate) fn lfo_reach<T: PartialEq + Copy>(routes: &[Route<T>; ROUTES], off: T) -> [bool; 3] {
    lfo_reach_onto(routes, |target| target != off)
}

/// [`lfo_reach`], onto only the targets `onto` takes.
pub(crate) fn lfo_reach_onto<T: PartialEq + Copy>(
    routes: &[Route<T>; ROUTES],
    onto: impl Fn(T) -> bool,
) -> [bool; 3] {
    let lfo = |source| match source {
        Source::Lfo1 => Some(0),
        Source::Lfo2 => Some(1),
        Source::Lfo3 => Some(2),
        _ => None,
    };
    let mut reach = [false; 3];
    for route in routes {
        let Some(k) = lfo(route.source) else {
            continue;
        };
        let net: f32 = routes
            .iter()
            .filter(|other| other.target == route.target && lfo(other.source) == Some(k))
            .map(|other| other.amount.clamp(-1.0, 1.0))
            .sum();
        reach[k] |= onto(route.target) && net != 0.0;
    }
    reach
}
