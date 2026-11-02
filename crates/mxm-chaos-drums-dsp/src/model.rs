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
    pub const SPARK_CLAP: Self = Self(11);

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

/// One model the selector offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelSpec {
    pub id: ModelId,
    pub label: &'static str,
    pub kind: Kind,
}

/// The models that render, in ID order. P1 has the kick, the snare and the clap.
pub const AVAILABLE_MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: ModelId::DRAINING_KICK,
        label: "Draining kick",
        kind: Kind::Kick,
    },
    ModelSpec {
        id: ModelId::WIRED_SNARE,
        label: "Wired snare",
        kind: Kind::Snare,
    },
    ModelSpec {
        id: ModelId::SPARK_CLAP,
        label: "Spark clap",
        kind: Kind::Clap,
    },
];

/// The model `id` names, if it renders.
#[must_use]
pub fn available(id: ModelId) -> Option<&'static ModelSpec> {
    AVAILABLE_MODELS.iter().find(|spec| spec.id == id)
}
