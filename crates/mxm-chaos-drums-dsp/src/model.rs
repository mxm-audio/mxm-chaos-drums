//! The catalogue: permanent model IDs, the kinds, and which models render.

/// A model's permanent ID, in the fixed domain 0…255. 0 and every ID not in
/// [`AVAILABLE_MODELS`] are silent. An ID never moves or changes meaning; a new model takes the
/// next unused one (the brief's Appendix A reserves 1–25).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModelId(u8);

impl ModelId {
    /// No model: silence.
    pub const NONE: Self = Self(0);
    pub const DRAINING_KICK: Self = Self(1);
    pub const WIRED_SNARE: Self = Self(2);
    pub const LEAKING_CLOSED_HAT: Self = Self(3);
    pub const LEAKING_OPEN_HAT: Self = Self(4);
    pub const FOLDED_HAT: Self = Self(5);
    pub const FALLING_TOM: Self = Self(6);
    pub const SLAPPED_CONGA: Self = Self(7);
    pub const SLAPPED_BONGO: Self = Self(8);
    pub const SPARK_CLAP: Self = Self(11);
    pub const LOOP_CYMBAL: Self = Self(13);
    pub const LOOP_CRASH: Self = Self(14);
    pub const RINGING_RIDE: Self = Self(15);
    pub const RAW_RING: Self = Self(21);
    pub const RAW_SPARK: Self = Self(22);
    pub const RAW_FOLD: Self = Self(23);
    pub const RAW_LOOP: Self = Self(24);
    pub const RAW_DUST: Self = Self(25);

    #[must_use]
    pub const fn new(raw: u8) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }
}

/// What drum a model is: the drum machine's twenty, then one raw kind per engine. Its code starts a
/// cell's made-up name (`bd1`, `tm2`), as in the drum machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Kick,
    Snare,
    ClosedHat,
    OpenHat,
    /// Neither open nor closed: a noise hat.
    Hat,
    Tom,
    Conga,
    Bongo,
    Rim,
    Clave,
    Clap,
    Cowbell,
    Cymbal,
    Crash,
    Ride,
    Maraca,
    Tambourine,
    Guiro,
    Bell,
    Brush,
    Ring,
    Spark,
    Fold,
    Loop,
    Dust,
}

impl Kind {
    /// Every kind, in the brief's order.
    pub const ALL: [Self; 25] = [
        Self::Kick,
        Self::Snare,
        Self::ClosedHat,
        Self::OpenHat,
        Self::Hat,
        Self::Tom,
        Self::Conga,
        Self::Bongo,
        Self::Rim,
        Self::Clave,
        Self::Clap,
        Self::Cowbell,
        Self::Cymbal,
        Self::Crash,
        Self::Ride,
        Self::Maraca,
        Self::Tambourine,
        Self::Guiro,
        Self::Bell,
        Self::Brush,
        Self::Ring,
        Self::Spark,
        Self::Fold,
        Self::Loop,
        Self::Dust,
    ];

    /// The two letters a cell's made-up name starts with.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Kick => "bd",
            Self::Snare => "sn",
            Self::ClosedHat => "ch",
            Self::OpenHat => "oh",
            Self::Hat => "hh",
            Self::Tom => "tm",
            Self::Conga => "cg",
            Self::Bongo => "bg",
            Self::Rim => "rs",
            Self::Clave => "cl",
            Self::Clap => "cp",
            Self::Cowbell => "cb",
            Self::Cymbal => "cy",
            Self::Crash => "cr",
            Self::Ride => "rd",
            Self::Maraca => "ma",
            Self::Tambourine => "tb",
            Self::Guiro => "gu",
            Self::Bell => "be",
            Self::Brush => "br",
            Self::Ring => "ri",
            Self::Spark => "sp",
            Self::Fold => "fo",
            Self::Loop => "lo",
            Self::Dust => "du",
        }
    }

    /// The words a label of this kind ends with.
    #[must_use]
    pub const fn words(self) -> &'static str {
        match self {
            Self::Kick => "kick",
            Self::Snare => "snare",
            Self::ClosedHat => "closed hat",
            Self::OpenHat => "open hat",
            Self::Hat => "hat",
            Self::Tom => "tom",
            Self::Conga => "conga",
            Self::Bongo => "bongo",
            Self::Rim => "rim",
            Self::Clave => "clave",
            Self::Clap => "clap",
            Self::Cowbell => "cowbell",
            Self::Cymbal => "cymbal",
            Self::Crash => "crash",
            Self::Ride => "ride",
            Self::Maraca => "maraca",
            Self::Tambourine => "tambourine",
            Self::Guiro => "guiro",
            Self::Bell => "bell",
            Self::Brush => "brush",
            Self::Ring => "ring",
            Self::Spark => "spark",
            Self::Fold => "fold",
            Self::Loop => "loop",
            Self::Dust => "dust",
        }
    }
}

/// The general controls' names, control 1 first, as the brief's table has them: what a control is
/// called where no model names it, in the host and on a route left on a control the model does not
/// read. Controls 10–12 are a model's own and have no general name.
pub const GENERAL_NAMES: [&str; crate::routes::CONTROLS] = [
    "Tone",
    "Body",
    "Drop",
    "Spread",
    "Grain",
    "Snap",
    "Drive",
    "Size",
    "Damping",
    "Control 10",
    "Control 11",
    "Control 12",
];

/// A general control a model reads: its number (1…12) and the name the model shows it under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlSpec {
    pub number: u8,
    pub name: &'static str,
}

const fn control(number: u8, name: &'static str) -> ControlSpec {
    ControlSpec { number, name }
}

/// One model the selector offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelSpec {
    pub id: ModelId,
    pub label: &'static str,
    pub kind: Kind,
    /// The general controls it reads, in panel order; every other one is an exact no-op on it.
    pub controls: &'static [ControlSpec],
    /// What the drum is made of and how it answers charge, in the player's words.
    pub notes: &'static str,
}

/// What every hat reads.
const HAT_CONTROLS: [ControlSpec; 5] = [
    control(1, "Tone"),
    control(2, "Body"),
    control(4, "Spread"),
    control(5, "Grain"),
    control(7, "Drive"),
];

/// What every cymbal reads.
const CYMBAL_CONTROLS: [ControlSpec; 5] = [
    control(1, "Tone"),
    control(2, "Body"),
    control(4, "Spread"),
    control(5, "Grain"),
    control(9, "Damping"),
];

/// The models that render, in ID order.
pub const AVAILABLE_MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: ModelId::DRAINING_KICK,
        label: "Draining kick",
        kind: Kind::Kick,
        controls: &[
            control(3, "Drop"),
            control(6, "Snap"),
            control(7, "Drive"),
            control(9, "Damping"),
        ],
        notes: "A ringing body whose pitch falls as its charge drains. Fill it past its threshold and \
                it stops dying and starts to sing.",
    },
    ModelSpec {
        id: ModelId::WIRED_SNARE,
        label: "Wired snare",
        kind: Kind::Snare,
        controls: &[
            control(1, "Tone"),
            control(2, "Body"),
            control(3, "Drop"),
            control(6, "Snap"),
            control(7, "Drive"),
            control(9, "Damping"),
        ],
        notes: "A drum body whose swing spills into noisy wires: the harder it rings, the thicker the \
                wires.",
    },
    ModelSpec {
        id: ModelId::LEAKING_CLOSED_HAT,
        label: "Leaking closed hat",
        kind: Kind::ClosedHat,
        controls: &HAT_CONTROLS,
        notes: "Six metal partials through a high band, under noise; it leaks fast, so a hit is a tick \
                and a held note a hiss.",
    },
    ModelSpec {
        id: ModelId::LEAKING_OPEN_HAT,
        label: "Leaking open hat",
        kind: Kind::OpenHat,
        controls: &HAT_CONTROLS,
        notes: "The closed hat's metal and noise, leaking slowly, so it rings on until a closed hat \
                cuts it.",
    },
    ModelSpec {
        id: ModelId::FOLDED_HAT,
        label: "Folded hat",
        kind: Kind::Hat,
        controls: &HAT_CONTROLS,
        notes: "Metal partials kicked hard by their clock: full, they break into noise; draining, \
                they turn back into a tone.",
    },
    ModelSpec {
        id: ModelId::FALLING_TOM,
        label: "Falling tom",
        kind: Kind::Tom,
        controls: &[
            control(3, "Drop"),
            control(6, "Snap"),
            control(7, "Drive"),
            control(9, "Damping"),
        ],
        notes: "A ringing head whose pitch falls as its charge drains, less far than the kick's. Fill \
                it past its threshold and it sings.",
    },
    ModelSpec {
        id: ModelId::SLAPPED_CONGA,
        label: "Slapped conga",
        kind: Kind::Conga,
        controls: &[
            control(2, "Body"),
            control(3, "Drop"),
            control(6, "Snap"),
            control(7, "Drive"),
            control(9, "Damping"),
        ],
        notes: "A ringing head with a slap at the strike, a fold kicked into chaos for a few \
                milliseconds; Body trades the slap for the tone.",
    },
    ModelSpec {
        id: ModelId::SLAPPED_BONGO,
        label: "Slapped bongo",
        kind: Kind::Bongo,
        controls: &[
            control(2, "Body"),
            control(3, "Drop"),
            control(6, "Snap"),
            control(7, "Drive"),
            control(9, "Damping"),
        ],
        notes: "A smaller, higher conga: a short ring and a bright slap.",
    },
    ModelSpec {
        id: ModelId::SPARK_CLAP,
        label: "Spark clap",
        kind: Kind::Clap,
        controls: &[
            control(1, "Tone"),
            control(5, "Grain"),
            control(6, "Snap"),
            control(8, "Spacing"),
        ],
        notes: "Sparks fire while the charge is high, each opening a burst of noise; as it drains they \
                slow and stop.",
    },
    ModelSpec {
        id: ModelId::LOOP_CYMBAL,
        label: "Loop cymbal",
        kind: Kind::Cymbal,
        controls: &CYMBAL_CONTROLS,
        notes: "A loop of metal fed by noise for as long as the charge lasts: it washes while full and \
                thins as it drains.",
    },
    ModelSpec {
        id: ModelId::LOOP_CRASH,
        label: "Loop crash",
        kind: Kind::Crash,
        controls: &CYMBAL_CONTROLS,
        notes: "A lower, wider loop of metal and a dense burst of noise at the strike that washes out \
                over seconds.",
    },
    ModelSpec {
        id: ModelId::RINGING_RIDE,
        label: "Ringing ride",
        kind: Kind::Ride,
        controls: &CYMBAL_CONTROLS,
        notes: "A steadier loop with little noise, and a bell of two rings over it; Body brings the \
                bell up.",
    },
    ModelSpec {
        id: ModelId::RAW_RING,
        label: "Raw ring",
        kind: Kind::Ring,
        controls: &[
            control(3, "Drop"),
            control(6, "Snap"),
            control(7, "Drive"),
            control(9, "Damping"),
        ],
        notes: "One self-oscillating ring: a thud when starved, a drone when full, its pitch drifting \
                with the charge.",
    },
    ModelSpec {
        id: ModelId::RAW_SPARK,
        label: "Raw spark",
        kind: Kind::Spark,
        controls: &[control(1, "Tone"), control(5, "Grain"), control(6, "Snap")],
        notes: "A spiking cell: silent, then isolated ticks, then crackle as it fills, off any beat.",
    },
    ModelSpec {
        id: ModelId::RAW_FOLD,
        label: "Raw fold",
        kind: Kind::Fold,
        controls: &[control(4, "Spread"), control(7, "Drive")],
        notes: "A tone kicked by a clock: it locks onto steps as the charge rises and breaks into noise \
                when full.",
    },
    ModelSpec {
        id: ModelId::RAW_LOOP,
        label: "Raw loop",
        kind: Kind::Loop,
        controls: &[
            control(4, "Spread"),
            control(6, "Snap"),
            control(7, "Drive"),
            control(9, "Damping"),
        ],
        notes: "A plucked loop that dies on its own; fill it and it howls, held by its own loss. Spread \
                makes it metal.",
    },
    ModelSpec {
        id: ModelId::RAW_DUST,
        label: "Raw dust",
        kind: Kind::Dust,
        controls: &[
            control(1, "Tone"),
            control(4, "Spread"),
            control(5, "Grain"),
        ],
        notes: "Noise whose density is the charge: sparse grit when low, dense hiss when full.",
    },
];

/// The model `id` names, if it renders.
#[must_use]
pub fn available(id: ModelId) -> Option<&'static ModelSpec> {
    AVAILABLE_MODELS.iter().find(|spec| spec.id == id)
}
