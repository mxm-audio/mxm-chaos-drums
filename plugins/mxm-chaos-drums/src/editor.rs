//! The editor (the brief's §4–§6): the grid of cells and the kit's dials, the LFOs and routes, then
//! the selected cell's model, charge, feeds, controls and output.
//!
//! Every card is one tree (`mxm_ui::tree`), measured for its floor and drawn leaf by leaf through
//! the collection's bindings (`mxm_params::binding`), as the drum machine's are. **A model shows only
//! the controls it reads**, under its own names. *Not yet as the brief has it:* a cell's routes are
//! a card of their own, not rows under each control they move.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use egui::Ui;
use mxm_chaos_drums_dsp::{AVAILABLE_MODELS, CELLS, EDGES, SIDE, available, edge};
use mxm_params::binding::{self, Bound, ErasedParam};
use mxm_preset::PresetUi;
use mxm_ui::control::{Size, Wave};
use mxm_ui::space::{SPACE_2, SPACE_3, SPACE_5};
use mxm_ui::theme::Tokens;
use mxm_ui::tree::{
    Flow, Font, Height, Kind, Node, group, leaf, pad, pad_all, row_gap, stack, stack_gap,
};
use nice_plug::context::gui::GuiContext;
use nice_plug::prelude::*;

use crate::params::{KIT_IDS, MxmChaosDrumsParams, at, cell_ids};
use crate::routes::{CELL_TARGETS, KIT_TARGETS, cell_target_name};
use crate::telemetry::Telemetry;

/// The opening size: the quarter-4K budget, hugged. Measured by `the_opening_size_is_the_budget_hugged`,
/// which fails with the size it should be.
const REFERENCE: (u32, u32) = (1820, 968);
/// The smallest window advertised: the widest card with the workspace's gutters, or the app bar.
const MINIMUM: (u32, u32) = (444, 320);
/// The keyboard cursor's card for the app bar's Master, outside the paging keys.
const MASTER_CARD: u64 = 64;
/// One cell's square on the grid, at its narrowest.
const SQUARE: egui::Vec2 = egui::Vec2::new(104.0, 56.0);
/// What the Controls card says for a model that reads none.
const NO_CONTROLS: &str = "This model has no controls of its own.";
/// The Pool's bar under the grid.
const POOL_HEIGHT: f32 = 24.0;
/// The charge a full flow mark stands for, per second.
const FLOW_FULL: f32 = 2.0;

const GRID_KEYS: [&str; CELLS] = [
    "cell-01", "cell-02", "cell-03", "cell-04", "cell-05", "cell-06", "cell-07", "cell-08",
    "cell-09", "cell-10", "cell-11", "cell-12", "cell-13", "cell-14", "cell-15", "cell-16",
];

const TITLES: [&str; 10] = [
    "Cells",
    "Kit",
    "LFOs",
    "Routes",
    "Kit routes",
    "Model",
    "Charge",
    "Feeds",
    "Controls",
    "Output",
];

const LFO_SHAPES: [(Wave, &str); 6] = [
    (Wave::Sine, "Sine"),
    (Wave::Triangle, "Triangle"),
    (Wave::RampUp, "Ramp up"),
    (Wave::RampDown, "Ramp down"),
    (Wave::Square, "Square"),
    (Wave::Random, "Sample & hold"),
];
const LFO_DETAILS: [&str; 6] = [
    "A smooth wobble.",
    "Rises and falls in straight lines.",
    "Rises, then snaps back down.",
    "Falls, then snaps back up.",
    "Jumps between two values, like a trill.",
    "A new random value on every cycle.",
];

const SOURCES: [&str; 8] = [
    "Off", "LFO 1", "LFO 2", "LFO 3", "Wheel", "Pressure", "Velocity", "Random",
];
const KIT_SOURCES: [&str; 6] = ["Off", "LFO 1", "LFO 2", "LFO 3", "Wheel", "Pressure"];
const BUSES: [&str; 3] = ["Off", "A", "B"];
const OUTPUTS: [&str; 17] = [
    "L+R", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
];
/// The kit-wide output row's choices, and its cursor key.
const ALL_CELLS: [&str; 2] = ["L+R", "Own outputs"];
const ALL_CELLS_ID: &str = "outputs_all";

const CHOKE_GROUPS: [&str; 17] = [
    "Off", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
];

static CELL_TARGET_NAMES: LazyLock<Vec<String>> =
    LazyLock::new(|| (0..CELL_TARGETS as i32).map(cell_target_name).collect());

fn cell_target_options() -> Vec<&'static str> {
    CELL_TARGET_NAMES.iter().map(String::as_str).collect()
}

fn kit_target_options() -> Vec<&'static str> {
    KIT_TARGETS.iter().map(|(_, name)| *name).collect()
}

pub type MxmChaosDrumsEditor = mxm_editor::Editor<App>;

/// Wakes a host that has let the plugin sleep, so a cell asked to be heard sounds now.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

pub fn create(
    params: Arc<MxmChaosDrumsParams>,
    telemetry: Arc<Telemetry>,
    wake: Wake,
) -> Option<MxmChaosDrumsEditor> {
    mxm_editor::create(
        crate::NAME,
        REFERENCE,
        MINIMUM,
        App::new(params, telemetry, wake),
    )
}

pub struct App {
    params: Arc<MxmChaosDrumsParams>,
    telemetry: Arc<Telemetry>,
    wake: Wake,
    selected: usize,
    text_entry: HashMap<&'static str, Option<String>>,
    presets: PresetUi,
    nav: mxm_ui::navigation::State,
}

impl App {
    fn new(params: Arc<MxmChaosDrumsParams>, telemetry: Arc<Telemetry>, wake: Wake) -> Self {
        let presets = PresetUi::new(params.as_ref());
        Self {
            params,
            telemetry,
            wake,
            selected: 0,
            text_entry: HashMap::new(),
            presets,
            nav: mxm_ui::navigation::State::default(),
        }
    }
}

impl mxm_editor::Panel for App {
    fn ui(&mut self, ui: &mut Ui, context: &GuiContext) {
        let (telemetry, wake) = (&self.telemetry, &self.wake);
        let audition = |cell: usize| {
            telemetry.request_audition(cell);
            wake();
        };
        panel(
            ui,
            &self.params,
            &self.telemetry,
            &context.param_setter(),
            &mut self.selected,
            &mut self.text_entry,
            &mut self.presets,
            &mut self.nav,
            &audition,
        );
        // A name emptied by a kit load or Init: the host hears of it.
        mxm_drum_shell::slot_names::announce(self.params.as_ref(), context);
    }
}

fn tokens_for(ui: &Ui) -> Tokens {
    if ui.visuals().dark_mode {
        mxm_ui::DARK
    } else {
        mxm_ui::LIGHT
    }
}

#[allow(clippy::too_many_arguments)]
pub fn panel(
    ui: &mut Ui,
    params: &MxmChaosDrumsParams,
    telemetry: &Telemetry,
    setter: &ParamSetter<'_>,
    selected: &mut usize,
    text_entry: &mut HashMap<&'static str, Option<String>>,
    presets: &mut PresetUi,
    nav: &mut mxm_ui::navigation::State,
    audition: &dyn Fn(usize),
) {
    let tokens = tokens_for(ui);
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(33));
    let busy = presets.holds_the_keyboard() || text_entry.values().any(Option::is_some);
    mxm_ui::paging::editor::hold(ui.ctx(), busy);
    mxm_ui::navigation::paged_with_bar(ui.ctx(), nav, busy, &[MASTER_CARD]);

    mxm_ui::AppBar::new(crate::NAME).show_with(
        ui,
        &tokens,
        |ui| mxm_preset::ui::preset_row(ui, &tokens, params, setter, presets),
        |ui| {
            mxm_ui::shell::peak_meter(ui, &tokens, &telemetry.meter);
            mxm_ui::navigation::bar_card(ui, MASTER_CARD, |ui| {
                ui.scope(|ui| {
                    Bound::new(
                        "master",
                        &params.master,
                        "Overall volume of the whole kit, on every output.",
                    )
                    .slider_inline(ui, &tokens, setter, text_entry, 96.0);
                })
                .response
                .rect
            });
            mxm_ui::shell::zoom_control(ui);
            mxm_ui::shell::editor_theme_control(ui);
        },
    );
    mxm_preset::ui::overlays(ui, &tokens, params, setter, presets);

    let cell = (*selected).min(CELLS - 1);
    let items = page_items(ui, params, cell);
    let text_editing = text_entry.values().any(Option::is_some);
    let mut live = Live {
        params,
        setter,
        cell,
        selected,
        entries: text_entry,
        charges: telemetry.charges(),
        flows: telemetry.flows(),
        pool: telemetry.pool(),
        text_editing,
        tempo: telemetry.tempo.get(),
        audition,
    };
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(tokens.canvas)
                .inner_margin(egui::Margin::same(SPACE_5 as i8)),
        )
        .show(ui, |ui| {
            mxm_ui::paging::editor::show(
                ui,
                &tokens,
                &items,
                &[&[mxm_ui::paging::Key(0), mxm_ui::paging::Key(1)]],
                text_editing,
                &mut |ui, index| card(ui, index, params, cell),
                &mut |ui, _, leaf, rect| paint(ui, &tokens, leaf, rect, &mut live),
            );
        });
}

/// Every paging item, each card exactly as wide as its floor, for the cell being edited.
pub fn page_items(
    ui: &Ui,
    params: &MxmChaosDrumsParams,
    cell: usize,
) -> Vec<mxm_ui::paging::Item<'static>> {
    use mxm_ui::flow::Card;
    use mxm_ui::paging::{Category, Item, Key};
    const CATEGORIES: [Category; 10] = [
        Category::Performance,
        Category::Performance,
        Category::Modulators,
        Category::Modulators,
        Category::Modulators,
        Category::Generators,
        Category::Generators,
        Category::Generators,
        Category::Generators,
        Category::Tone,
    ];
    const KINDS: [&str; 10] = [
        "Cells", "Kit", "LFO", "Routes", "Routes", "Model", "Shape", "Shape", "Shape", "Output",
    ];
    TITLES
        .iter()
        .enumerate()
        .map(|(index, title)| {
            let content = mxm_ui::tree::card_floor(ui, title, &card(ui, index, params, cell));
            Item {
                key: Key(index as u64),
                card: Card::new(title, content).capped(content),
                category: CATEGORIES[index],
                kind: KINDS[index],
            }
        })
        .collect()
}

/// What a leaf is. The cell's leaves name the cell's parameter by its place in `cell_ids`, the
/// kit's by its place in `KIT_IDS`.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum Leaf {
    Grid,
    Pool,
    Kit(usize),
    ResetOnPlay,
    Rate(usize),
    Sync(usize),
    Shape(usize),
    Source(usize),
    Target(usize),
    Amount(usize),
    KitSource(usize),
    KitTarget(usize),
    KitAmount(usize),
    Model,
    Notes,
    NoControls,
    Knob(usize),
    Bus,
    Mute,
    Solo,
    Output,
    AllCells,
    ChokeGroup,
}

/// The kit's knobs, by their place in `KIT_IDS`, in rows.
const KIT_ROWS: [[usize; 3]; 3] = [[0, 1, 2], [3, 4, 9], [5, 6, 7]];
const RESET_ON_PLAY: usize = 8;

fn kit_tip(index: usize) -> &'static str {
    match index {
        0 => {
            "How unpredictable the kit is: at zero every cell is a steady drum; up, cells sit near the edge where small differences grow, and feed each other."
        }
        1 => {
            "How fast charge moves from fuller cells to emptier neighbours on the grid. Needs Temper above zero."
        }
        2 => "How much of each cell's sound reaches its neighbours. Needs Temper above zero.",
        3 => {
            "How much every held note and bus can draw from at once: lower, and holding many notes makes the whole kit sag."
        }
        4 => "How fast the shared supply refills after it has been drawn on.",
        5 => {
            "A level that powers every cell set to Bus A, with no note at all: steady, it drones; moving, it feeds and starves them."
        }
        6 => "A level that powers every cell set to Bus B, with no note at all.",
        7 => {
            "Chooses the jitter and noise every cell plays. The same seed and the same notes play the same performance."
        }
        9 => "How velocity answers: up brings soft hits up, down holds them back; zero is linear.",
        _ => "",
    }
}

fn cell_tip(index: usize) -> &'static str {
    match index {
        at::COARSE => "The cell's pitch, in semitones.",
        at::FINE => "The cell's pitch, in cents.",
        at::STRIKE => "How much charge a hit adds, at full velocity.",
        at::SUPPLY => "How fast a held note, or the cell's bus, keeps filling it.",
        at::HUNGER => "How fast sounding spends the charge: high, loud hits die sooner.",
        at::LEAK => "How long the charge lasts at rest.",
        at::THRESHOLD => {
            "Moves the charge at which the drum changes character: lower, it sings or crackles sooner."
        }
        at::LINK => "How much this cell shares with its neighbours on the grid.",
        at::TOUCH => {
            "How strongly its touch key acts: feeding its own sound back into it and bridging it to its neighbours."
        }
        at::LEVEL => "How loud this cell is.",
        at::PAN => "Where this cell sits between left and right.",
        _ => "One of this model's own controls.",
    }
}

/// Card `index`'s body, as a tree, for the cell being edited.
fn card(ui: &Ui, index: usize, params: &MxmChaosDrumsParams, cell: usize) -> Node<Leaf> {
    let p = &params.cells[cell];
    match index {
        0 => stack(vec![
            leaf(
                Leaf::Grid,
                Kind::Custom {
                    min_width: grid_width(),
                    height: Height::Fixed(grid_height()),
                    fills: true,
                },
            ),
            pad(
                SPACE_3,
                leaf(
                    Leaf::Pool,
                    Kind::Custom {
                        min_width: 0.0,
                        height: Height::Fixed(POOL_HEIGHT),
                        fills: true,
                    },
                ),
            ),
        ]),
        1 => {
            let globals = params.globals();
            let mut rows: Vec<Node<Leaf>> = KIT_ROWS
                .iter()
                .map(|row| knobs(ui, row.iter().map(|&i| (Leaf::Kit(i), globals[i]))))
                .collect();
            rows.push(pad(
                SPACE_3,
                leaf(
                    Leaf::ResetOnPlay,
                    Kind::Toggle {
                        label: ErasedParam::name(&params.reset_on_play).to_owned(),
                    },
                ),
            ));
            stack(rows)
        }
        2 => stack(
            (0..3)
                .map(|i| {
                    let row = group(vec![lfo_row(ui, params, i)]);
                    if i == 0 { row } else { pad(SPACE_3, row) }
                })
                .collect(),
        ),
        3 => routes_card(
            ui,
            |r| (Leaf::Source(r), Leaf::Target(r), Leaf::Amount(r)),
            &SOURCES,
            &cell_target_options(),
        ),
        4 => routes_card(
            ui,
            |r| (Leaf::KitSource(r), Leaf::KitTarget(r), Leaf::KitAmount(r)),
            &KIT_SOURCES,
            &kit_target_options(),
        ),
        5 => {
            let parameters = p.parameters();
            stack(vec![
                leaf(
                    Leaf::Model,
                    Kind::Selector {
                        label: "Model".to_owned(),
                        options: AVAILABLE_MODELS
                            .iter()
                            .map(|spec| spec.label.to_owned())
                            .collect(),
                        caption: false,
                        width: None,
                    },
                ),
                pad(
                    SPACE_3,
                    knobs(
                        ui,
                        [at::COARSE, at::FINE]
                            .map(|i| (Leaf::Knob(i), parameters[i]))
                            .into_iter(),
                    ),
                ),
                pad(
                    SPACE_3,
                    leaf(
                        Leaf::Notes,
                        Kind::Text {
                            text: notes(p).to_owned(),
                            font: Font::Caption,
                            flow: Flow::Wrap,
                        },
                    ),
                ),
            ])
        }
        6 => {
            let parameters = p.parameters();
            stack(vec![
                knobs(
                    ui,
                    [at::STRIKE, at::SUPPLY, at::HUNGER]
                        .map(|i| (Leaf::Knob(i), parameters[i]))
                        .into_iter(),
                ),
                knobs(
                    ui,
                    [at::LEAK, at::THRESHOLD]
                        .map(|i| (Leaf::Knob(i), parameters[i]))
                        .into_iter(),
                ),
            ])
        }
        7 => {
            let parameters = p.parameters();
            stack(vec![
                selector_leaf(Leaf::Bus, "Bus", &BUSES),
                pad(
                    SPACE_3,
                    knobs(
                        ui,
                        [at::LINK, at::TOUCH]
                            .map(|i| (Leaf::Knob(i), parameters[i]))
                            .into_iter(),
                    ),
                ),
            ])
        }
        8 => {
            let parameters = p.parameters();
            let controls = available(p.model_id()).map_or(&[][..], |spec| spec.controls);
            if controls.is_empty() {
                return leaf(
                    Leaf::NoControls,
                    Kind::Text {
                        text: NO_CONTROLS.to_owned(),
                        font: Font::Caption,
                        flow: Flow::Wrap,
                    },
                );
            }
            stack(
                controls
                    .chunks(3)
                    .map(|row| {
                        let row = row.iter().map(|c| {
                            let i = at::control(usize::from(c.number));
                            (Leaf::Knob(i), parameters[i])
                        });
                        knobs_named(ui, row, |leaf| control_name(params, cell, leaf))
                    })
                    .collect(),
            )
        }
        _ => {
            let parameters = p.parameters();
            stack(vec![
                knobs(
                    ui,
                    [at::LEVEL, at::PAN]
                        .map(|i| (Leaf::Knob(i), parameters[i]))
                        .into_iter(),
                ),
                pad(
                    SPACE_3,
                    row_gap(
                        ui.spacing().item_spacing.x,
                        vec![
                            leaf(
                                Leaf::Mute,
                                Kind::Toggle {
                                    label: "Mute".to_owned(),
                                },
                            ),
                            leaf(
                                Leaf::Solo,
                                Kind::Toggle {
                                    label: "Solo".to_owned(),
                                },
                            ),
                        ],
                    ),
                ),
                pad(SPACE_3, selector_leaf(Leaf::Output, "Output", &OUTPUTS)),
                leaf(
                    Leaf::AllCells,
                    Kind::Segmented {
                        label: "All cells".to_owned(),
                        options: ALL_CELLS.map(str::to_owned).to_vec(),
                        beside: None,
                    },
                ),
                selector_leaf(Leaf::ChokeGroup, "Choke group", &CHOKE_GROUPS),
            ])
        }
    }
}

/// The name a control knob paints: its model's name for it, or the parameter's.
fn control_name(params: &MxmChaosDrumsParams, cell: usize, leaf: Leaf) -> Option<&'static str> {
    let Leaf::Knob(i) = leaf else { return None };
    let spec = available(params.cells[cell].model_id())?;
    spec.controls
        .iter()
        .find(|c| at::control(usize::from(c.number)) == i)
        .map(|c| c.name)
}

fn knobs<'a>(ui: &Ui, knobs: impl Iterator<Item = (Leaf, &'a dyn ErasedParam)>) -> Node<Leaf> {
    knobs_named(ui, knobs, |_| None)
}

/// The collection's knob row, each knob under `name` or its parameter's own.
fn knobs_named<'a>(
    ui: &Ui,
    knobs: impl Iterator<Item = (Leaf, &'a dyn ErasedParam)>,
    name: impl Fn(Leaf) -> Option<&'static str>,
) -> Node<Leaf> {
    mxm_ui::tree::knob_row(
        ui,
        knobs
            .map(|(key, param)| {
                let knob = leaf(
                    key,
                    Kind::Knob {
                        name: name(key).unwrap_or(param.name()).to_owned(),
                        widest: mxm_ui::control::widest_value(|n| param.format(n as f32)),
                        size: Size::Standard,
                        column: 0.0,
                    },
                );
                (Size::Standard, knob)
            })
            .collect(),
    )
}

fn selector_leaf(key: Leaf, label: &str, options: &[&str]) -> Node<Leaf> {
    leaf(
        key,
        Kind::Selector {
            label: label.to_owned(),
            options: options.iter().map(|option| (*option).to_owned()).collect(),
            caption: false,
            width: None,
        },
    )
}

/// Four routes, each a source and a target side by side and its amount below them.
fn routes_card(
    ui: &Ui,
    keys: impl Fn(usize) -> (Leaf, Leaf, Leaf),
    sources: &[&str],
    targets: &[&str],
) -> Node<Leaf> {
    stack(
        (0..4)
            .map(|r| {
                let (source, target, amount) = keys(r);
                let body = stack(vec![
                    row_gap(
                        ui.spacing().item_spacing.x,
                        vec![
                            selector_leaf(source, &format!("Route {}", r + 1), sources),
                            selector_leaf(target, "To", targets),
                        ],
                    ),
                    leaf(
                        amount,
                        Kind::Slider {
                            label: "Amount".to_owned(),
                            quiet: false,
                            widest: "-100%".to_owned(),
                        },
                    ),
                ]);
                if r == 0 { body } else { pad(SPACE_3, body) }
            })
            .collect(),
    )
}

/// One LFO: its rate knob, and beside it Sync above the shapes, as the drum machine's.
fn lfo_row(ui: &Ui, params: &MxmChaosDrumsParams, index: usize) -> Node<Leaf> {
    let (rate, _, _) = params.lfos()[index];
    row_gap(
        ui.spacing().item_spacing.x,
        vec![
            leaf(
                Leaf::Rate(index),
                Kind::Knob {
                    name: format!("Rate {}", index + 1),
                    widest: binding::synced_widest(rate, crate::params::LFO_SYNC.span),
                    size: Size::Standard,
                    column: mxm_ui::control::KNOB_COLUMN_MIN,
                },
            ),
            pad_all(
                0.0,
                SPACE_3,
                0.0,
                stack_gap(
                    SPACE_2,
                    vec![
                        pad(
                            mxm_ui::control::name_box_height(ui),
                            leaf(Leaf::Sync(index), Kind::SyncToggle),
                        ),
                        leaf(
                            Leaf::Shape(index),
                            Kind::Waves {
                                label: None,
                                count: LFO_SHAPES.len(),
                                marks: Vec::new(),
                                beside: None,
                            },
                        ),
                    ],
                ),
            ),
        ],
    )
}

fn grid_width() -> f32 {
    SIDE as f32 * SQUARE.x + (SIDE - 1) as f32 * SPACE_2
}

fn grid_height() -> f32 {
    SIDE as f32 * SQUARE.y + (SIDE - 1) as f32 * SPACE_2
}

/// Everything a leaf draws with.
struct Live<'a, 'b> {
    params: &'a MxmChaosDrumsParams,
    setter: &'a ParamSetter<'b>,
    /// The cell every card was built around this frame; a click on another lands next frame.
    cell: usize,
    selected: &'a mut usize,
    entries: &'a mut HashMap<&'static str, Option<String>>,
    charges: [f32; CELLS],
    flows: [f32; EDGES],
    pool: f32,
    text_editing: bool,
    tempo: Option<f64>,
    audition: &'a dyn Fn(usize),
}

fn paint(ui: &mut Ui, tokens: &Tokens, leaf: &Leaf, rect: egui::Rect, live: &mut Live<'_, '_>) {
    let params = live.params;
    let setter = live.setter;
    let cell = live.cell;
    let ids = cell_ids(cell);
    let p = &params.cells[cell];
    match *leaf {
        Leaf::Grid => grid(ui, tokens, rect, live),
        Leaf::Pool => pool_bar(ui, tokens, rect, params, live.pool),
        Leaf::Kit(i) => {
            let globals = params.globals();
            Bound::new(KIT_IDS[i], globals[i], kit_tip(i)).knob(
                ui,
                tokens,
                setter,
                Size::Standard,
                rect.width(),
                live.entries,
            );
        }
        Leaf::ResetOnPlay => binding::toggle(
            ui,
            tokens,
            KIT_IDS[RESET_ON_PLAY],
            &params.reset_on_play,
            "Restarts every cell from rest when the host starts playing, so a bounce is what was heard.",
            setter,
        ),
        Leaf::Rate(i) => {
            let (rate, _, sync) = params.lfos()[i];
            let id = KIT_IDS[11 + 3 * i];
            let (lo, hi) = (
                f64::from(mxm_chaos_drums_dsp::curves::LFO_HZ.0),
                f64::from(mxm_chaos_drums_dsp::curves::LFO_HZ.1),
            );
            let reading = sync
                .value()
                .then(|| {
                    crate::params::LFO_SYNC.shown(
                        rate.unmodulated_plain_value(),
                        live.tempo,
                        lo,
                        hi,
                    )
                })
                .flatten()
                .map(mxm_tempo::Division::label);
            let label = format!("Rate {}", i + 1);
            let bound = Bound::new(
                id,
                rate,
                "How fast this LFO runs; with Sync on, a note length.",
            )
            .labelled(label);
            match reading {
                Some(reading) => bound.knob_with_reading(
                    ui,
                    tokens,
                    setter,
                    Size::Standard,
                    rect.width(),
                    live.entries,
                    reading,
                ),
                None => bound.knob(
                    ui,
                    tokens,
                    setter,
                    Size::Standard,
                    rect.width(),
                    live.entries,
                ),
            }
        }
        Leaf::Sync(i) => {
            binding::sync_picture(ui, tokens, KIT_IDS[13 + 3 * i], params.lfos()[i].2, setter)
        }
        Leaf::Shape(i) => binding::segmented_waves_unlabelled(
            ui,
            tokens,
            KIT_IDS[12 + 3 * i],
            params.lfos()[i].1,
            &LFO_SHAPES,
            &LFO_DETAILS,
            setter,
        ),
        Leaf::Source(r) => {
            let route = p.routes.all()[r];
            binding::selector_named(
                ui,
                tokens,
                ids[at::route(r)[0]],
                &route.source,
                Some(&format!("Route {}", r + 1)),
                &SOURCES,
                "What moves this route: an LFO, the wheel, pressure, the hit's velocity or a random draw a hit.",
                setter,
            );
        }
        Leaf::Target(r) => binding::selector_named(
            ui,
            tokens,
            ids[at::route(r)[1]],
            &p.routes.all()[r].target,
            Some("To"),
            &cell_target_options(),
            "What this route moves on this cell.",
            setter,
        ),
        Leaf::Amount(r) => Bound::new(
            ids[at::route(r)[2]],
            &p.routes.all()[r].amount,
            "How far, and which way, the route moves it.",
        )
        .labelled("Amount")
        .slider(ui, tokens, setter, live.entries),
        Leaf::KitSource(r) => {
            let route = params.kit_routes.all()[r];
            binding::selector_named(
                ui,
                tokens,
                KIT_IDS[20 + 3 * r],
                &route.source,
                Some(&format!("Route {}", r + 1)),
                &KIT_SOURCES,
                "What moves this kit route: an LFO, the wheel or pressure.",
                setter,
            );
        }
        Leaf::KitTarget(r) => binding::selector_named(
            ui,
            tokens,
            KIT_IDS[21 + 3 * r],
            &params.kit_routes.all()[r].target,
            Some("To"),
            &kit_target_options(),
            "What this route moves for the whole kit.",
            setter,
        ),
        Leaf::KitAmount(r) => Bound::new(
            KIT_IDS[22 + 3 * r],
            &params.kit_routes.all()[r].amount,
            "How far, and which way, the route moves it.",
        )
        .labelled("Amount")
        .slider(ui, tokens, setter, live.entries),
        Leaf::Model => model_selector(ui, tokens, ids[at::MODEL], p, setter),
        Leaf::Notes => caption(ui, tokens, notes(p)),
        Leaf::NoControls => caption(ui, tokens, NO_CONTROLS),
        Leaf::Knob(i) => {
            let parameters = p.parameters();
            let bound = Bound::new(ids[i], parameters[i], cell_tip(i));
            let bound = match control_name(params, cell, Leaf::Knob(i)) {
                Some(name) => bound.labelled(name),
                None => bound,
            };
            bound.knob(
                ui,
                tokens,
                setter,
                Size::Standard,
                rect.width(),
                live.entries,
            );
        }
        Leaf::Bus => binding::selector(
            ui,
            tokens,
            ids[at::BUS],
            &p.bus,
            &BUSES,
            None,
            "What else powers this cell, with no note: Bus A's or Bus B's level.",
            setter,
        ),
        Leaf::Mute => binding::toggle(
            ui,
            tokens,
            ids[at::MUTE],
            &p.mute,
            "Silence this cell without stopping it: its charge and its neighbours carry on.",
            setter,
        ),
        Leaf::Solo => binding::toggle(
            ui,
            tokens,
            ids[at::SOLO],
            &p.solo,
            "Hear this cell on its own.",
            setter,
        ),
        Leaf::Output => binding::selector(
            ui,
            tokens,
            ids[at::OUTPUT],
            &p.output,
            &OUTPUTS,
            None,
            "Where this cell plays: the main stereo output, or an output of its own.",
            setter,
        ),
        Leaf::AllCells => all_cells(ui, tokens, params, setter),
        Leaf::ChokeGroup => binding::selector(
            ui,
            tokens,
            ids[at::CHOKE_GROUP],
            &p.choke_group,
            &CHOKE_GROUPS,
            None,
            "Cells sharing a group cut each other: a closed hat its open one. A cut cell stays empty until it is played again.",
            setter,
        ),
    }
}

/// Text in the caption font the tree measured it in, wrapping in its leaf.
fn caption(ui: &mut Ui, tokens: &Tokens, text: &str) {
    let font = mxm_ui::tree::font_id(ui, Font::Caption);
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .font(font)
                .color(tokens.text_secondary),
        )
        .wrap(),
    );
}

/// What the cell's model is made of, in the player's words.
fn notes(p: &crate::params::CellParams) -> &'static str {
    available(p.model_id()).map_or("No model: this cell is silent.", |spec| spec.notes)
}

/// Every cell's Output at once: all on the main pair, or each on its own output, as one balanced edit
/// of the sixteen settings, as the drum machine's All slots row. Neither is lit when the cells go to
/// different places.
fn all_cells(ui: &mut Ui, tokens: &Tokens, params: &MxmChaosDrumsParams, setter: &ParamSetter<'_>) {
    let outputs = params.cells.iter().map(|cell| cell.output.value());
    let current = if outputs.clone().all(|output| output == 0) {
        0
    } else if outputs.zip(1..).all(|(output, own)| output == own) {
        1
    } else {
        2
    };
    let mut selected = current;
    let changed = mxm_ui::navigation::at(ui, ALL_CELLS_ID, |ui| {
        mxm_ui::control::segmented(
            ui,
            tokens,
            "All cells",
            &ALL_CELLS,
            &mut selected,
            None,
            None,
            &[
                "Every cell plays through the main stereo output.",
                "Each cell plays through its own output, for mixing them separately.",
            ],
        )
    });
    if changed && selected != current {
        let own = selected == 1;
        let edits: Vec<(&dyn ErasedParam, f32)> = params
            .cells
            .iter()
            .zip(1..)
            .map(|(cell, number)| {
                let param: &dyn ErasedParam = &cell.output;
                (
                    param,
                    cell.output.preview_normalized(if own { number } else { 0 }),
                )
            })
            .collect();
        binding::set_together(setter, &edits);
    }
}

/// The Model selector: the available models in ID order, mapped to their permanent IDs.
fn model_selector(
    ui: &mut Ui,
    tokens: &Tokens,
    id: &'static str,
    p: &crate::params::CellParams,
    setter: &ParamSetter<'_>,
) {
    let current = p.model_id();
    let options: Vec<&str> = AVAILABLE_MODELS.iter().map(|spec| spec.label).collect();
    let position = AVAILABLE_MODELS.iter().position(|spec| spec.id == current);
    let mut selected = position.unwrap_or(0);
    let changed = mxm_ui::navigation::at(ui, id, |ui| {
        mxm_ui::control::selector(
            ui,
            tokens,
            "Model",
            &options,
            &mut selected,
            position,
            None,
            "Which drum this cell is: a network of engines on its charge.",
        )
    });
    if changed && position != Some(selected) {
        let raw = i32::from(AVAILABLE_MODELS[selected].id.raw());
        setter.begin_set_parameter(&p.model);
        setter.set_parameter(&p.model, raw);
        setter.end_set_parameter(&p.model);
    }
}

/// The grid: each cell's square with its number, name, model and charge, and the charge moving
/// across each edge drawn between them. A click, or OPEN with the cursor on it, selects the cell and
/// strikes it, to hear it while editing.
fn grid(ui: &mut Ui, tokens: &Tokens, rect: egui::Rect, live: &mut Live<'_, '_>) {
    let gap = SPACE_2;
    let width = (rect.width() - (SIDE - 1) as f32 * gap) / SIDE as f32;
    let square = |cell: usize| {
        let (row, col) = ((cell / SIDE) as f32, (cell % SIDE) as f32);
        egui::Rect::from_min_size(
            rect.min + egui::vec2(col * (width + gap), row * (SQUARE.y + gap)),
            egui::vec2(width, SQUARE.y),
        )
    };
    let painter = ui.painter().with_clip_rect(rect);
    // Flow first, under the squares: a mark across each edge, as heavy as the charge it moved.
    for e in 0..EDGES {
        let (a, b) = edge(e);
        let weight = (live.flows[e].abs() / FLOW_FULL).min(1.0);
        if weight < 0.02 {
            continue;
        }
        let (ra, rb) = (square(a), square(b));
        let wraps =
            (a % SIDE == SIDE - 1 && b % SIDE == 0) || (a / SIDE == SIDE - 1 && b / SIDE == 0);
        let stroke = egui::Stroke::new(
            1.0 + 4.0 * weight,
            tokens.accent.gamma_multiply(0.35 + 0.65 * weight),
        );
        if wraps {
            // An edge round the torus: a stub at each end, pointing out of the grid.
            let out = if e < CELLS {
                egui::vec2(gap, 0.0)
            } else {
                egui::vec2(0.0, gap)
            };
            let (ca, cb) = (
                if e < CELLS {
                    ra.right_center()
                } else {
                    ra.center_bottom()
                },
                if e < CELLS {
                    rb.left_center()
                } else {
                    rb.center_top()
                },
            );
            painter.line_segment([ca, ca + out * 0.5], stroke);
            painter.line_segment([cb, cb - out * 0.5], stroke);
        } else {
            painter.line_segment([ra.center(), rb.center()], stroke);
        }
    }
    let names = mxm_drum_shell::slot_names::shown(live.params);
    for cell in 0..CELLS {
        let r = square(cell);
        let selected = *live.selected == cell;
        let name = names[cell].clone().unwrap_or_default();
        let model = available(live.params.cells[cell].model_id()).map_or("Off", |spec| spec.label);
        let key = GRID_KEYS[cell];
        let response = ui
            .add_enabled_ui(!live.text_editing, |ui| {
                mxm_ui::navigation::at(ui, key, |ui| {
                    let response = ui.interact(r, ui.id().with(key), egui::Sense::click());
                    let label = format!("Cell {} {name}, {model}", cell + 1);
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label)
                    });
                    mxm_ui::navigation::mark(ui, &response, r);
                    response
                })
            })
            .inner
            .on_hover_text("Plays this cell and selects it for editing.");
        if response.clicked() || (!live.text_editing && mxm_ui::navigation::take_type(ui, key)) {
            *live.selected = cell;
            (live.audition)(cell);
        }
        let fill = if selected {
            tokens.selection
        } else if response.hovered() {
            tokens.surface_3
        } else {
            tokens.surface_2
        };
        painter.rect_filled(r, 4.0, fill);
        // The charge, rising from the bottom.
        let charge = live.charges[cell].clamp(0.0, 1.0);
        if charge > 0.0 {
            let level = egui::Rect::from_min_max(
                egui::pos2(r.left(), r.bottom() - r.height() * charge),
                r.max,
            );
            painter.rect_filled(level, 4.0, tokens.accent.gamma_multiply(0.28));
        }
        let text = painter.with_clip_rect(r.shrink(4.0));
        text.text(
            r.left_top() + egui::vec2(6.0, 5.0),
            egui::Align2::LEFT_TOP,
            format!("{:02}  {name}", cell + 1),
            egui::FontId::proportional(13.0),
            tokens.text_primary,
        );
        text.text(
            r.left_bottom() + egui::vec2(6.0, -6.0),
            egui::Align2::LEFT_BOTTOM,
            model,
            egui::FontId::proportional(11.0),
            tokens.text_secondary,
        );
        let p = &live.params.cells[cell];
        let marks = match (p.mute.value(), p.solo.value()) {
            (true, true) => "M S",
            (true, false) => "M",
            (false, true) => "S",
            (false, false) => "",
        };
        text.text(
            r.right_top() + egui::vec2(-6.0, 5.0),
            egui::Align2::RIGHT_TOP,
            marks,
            egui::FontId::proportional(11.0),
            tokens.text_secondary,
        );
        if selected {
            painter.rect_stroke(
                r,
                4.0,
                egui::Stroke::new(2.0, tokens.accent),
                egui::StrokeKind::Inside,
            );
            painter.rect_filled(
                egui::Rect::from_min_max(r.min, egui::pos2(r.left() + 4.0, r.bottom())),
                2.0,
                tokens.accent,
            );
        }
    }
}

/// The Pool: how much of its capacity is left, beside its name.
fn pool_bar(
    ui: &mut Ui,
    tokens: &Tokens,
    rect: egui::Rect,
    params: &MxmChaosDrumsParams,
    level: f32,
) {
    let painter = ui.painter().with_clip_rect(rect);
    let capacity = 4.0 * params.pool.value();
    let share = if capacity > 0.0 {
        (level / capacity).clamp(0.0, 1.0)
    } else {
        0.0
    };
    painter.text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        "Pool",
        egui::FontId::proportional(12.0),
        tokens.text_secondary,
    );
    let bar = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 44.0, rect.center().y - 4.0),
        egui::pos2(rect.right(), rect.center().y + 4.0),
    );
    painter.rect_filled(bar, 3.0, tokens.surface_2);
    painter.rect_filled(
        egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * share, bar.height())),
        3.0,
        tokens.accent,
    );
}

/// [`page_items`] for `params` with `cell` selected, in a scratch editor context.
#[cfg(test)]
pub(crate) fn items_in(
    params: &MxmChaosDrumsParams,
    cell: usize,
) -> Vec<mxm_ui::paging::Item<'static>> {
    let ctx = egui::Context::default();
    mxm_ui::typography::apply(&ctx);
    mxm_ui::theme::apply(&ctx);
    let mut items = Vec::new();
    for _ in 0..3 {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            items = page_items(ui, params, cell);
        });
        output.textures_delta.clear();
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use mxm_plugin_test::{keyboard_checks, opening_size, paging_checks, tree_checks};

    const REVEAL: fn(&egui::Context) = |_| {};

    /// A host that applies each write at once, as a GUI gesture reaches the plugin.
    #[derive(Default)]
    struct Host;

    impl nice_plug::context::gui::GuiContextInner for Host {
        fn request_restart(&self) {}
        fn plugin_api(&self) -> nice_plug::prelude::PluginApi {
            nice_plug::prelude::PluginApi::Clap
        }
        unsafe fn raw_begin_set_parameter(&self, _param: nice_plug::params::internals::ParamPtr) {}
        unsafe fn raw_set_parameter_normalized(
            &self,
            param: nice_plug::params::internals::ParamPtr,
            normalized: f32,
        ) {
            // SAFETY: the test owns the parameters and no other thread touches them.
            unsafe { param._internal_set_normalized_value(normalized) };
        }
        unsafe fn raw_end_set_parameter(&self, _param: nice_plug::params::internals::ParamPtr) {}
        fn get_state(&self) -> nice_plug::prelude::PluginState {
            nice_plug::prelude::PluginState {
                version: String::new(),
                params: Default::default(),
                fields: Default::default(),
            }
        }
        fn set_state(&self, _state: nice_plug::prelude::PluginState) {}
    }

    type State = (
        usize,
        HashMap<&'static str, Option<String>>,
        PresetUi,
        mxm_ui::navigation::State,
    );

    fn state(params: &MxmChaosDrumsParams) -> State {
        (
            0,
            HashMap::new(),
            PresetUi::at(crate::preset::Library::at(None), params),
            mxm_ui::navigation::State::default(),
        )
    }

    fn drive<'a>(
        params: &'a MxmChaosDrumsParams,
        telemetry: &'a Telemetry,
        setter: &'a ParamSetter<'a>,
        state: &'a mut State,
    ) -> impl FnMut(&mut Ui) + 'a {
        move |ui| {
            let (selected, entries, presets, nav) = state;
            panel(
                ui,
                params,
                telemetry,
                setter,
                selected,
                entries,
                presets,
                nav,
                &|cell| telemetry.request_audition(cell),
            );
        }
    }

    fn reference() -> egui::Vec2 {
        egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32)
    }

    fn minimum() -> egui::Vec2 {
        egui::vec2(MINIMUM.0 as f32, MINIMUM.1 as f32)
    }

    #[test]
    fn the_opening_size_is_the_budget_hugged() {
        let (params, telemetry, host) =
            (MxmChaosDrumsParams::default(), Telemetry::default(), Host);
        let setter = ParamSetter::new(&host);
        let mut state = state(&params);
        opening_size::is_the_budget_hugged(
            reference(),
            &REVEAL,
            &mut drive(&params, &telemetry, &setter, &mut state),
        );
    }

    #[test]
    fn the_app_bar_holds_in_the_minimum_window() {
        let (params, telemetry, host) =
            (MxmChaosDrumsParams::default(), Telemetry::default(), Host);
        let setter = ParamSetter::new(&host);
        let mut state = state(&params);
        opening_size::bar_holds_from_the_minimum(
            minimum(),
            &mut drive(&params, &telemetry, &setter, &mut state),
        );
    }

    #[test]
    fn every_dynamic_page_fits_and_every_card_is_reachable() {
        let (params, telemetry, host) =
            (MxmChaosDrumsParams::default(), Telemetry::default(), Host);
        let setter = ParamSetter::new(&host);
        let mut state = state(&params);
        paging_checks::verify(
            &items_in(&params, 0),
            &[
                reference(),
                minimum(),
                egui::vec2(617.0, 562.0),
                egui::vec2(900.0, 700.0),
                egui::vec2(1280.0, 800.0),
            ],
            drive(&params, &telemetry, &setter, &mut state),
        );
    }

    #[test]
    fn the_keyboard_cursor_reaches_and_operates_every_parameter() {
        use mxm_preset::Instrument;
        let (params, telemetry) = (MxmChaosDrumsParams::default(), Telemetry::default());
        let recorder = Arc::new(keyboard_checks::Recorder::default());
        let setter = ParamSetter::new(recorder.as_ref());
        let mut state = state(&params);
        let mut reachable: Vec<&str> = params.parameters().into_iter().map(|(id, _)| id).collect();
        reachable.extend(GRID_KEYS);
        reachable.push(ALL_CELLS_ID);
        keyboard_checks::the_cursor_reaches_and_operates_from(
            reference(),
            &items_in(&params, 0),
            keyboard_checks::Coverage::Within(&reachable),
            KIT_IDS[0],
            &REVEAL,
            &recorder,
            &mut drive(&params, &telemetry, &setter, &mut state),
        );
    }

    /// Every card for every model, with every route in use and every LFO synced, passes the
    /// collection's tree checks: its floor holds it and nothing paints outside it.
    #[test]
    fn every_card_passes_the_tree_checks_for_every_model() {
        use nice_plug::params::InternalParamMut;
        for spec in AVAILABLE_MODELS {
            let params = MxmChaosDrumsParams::default();
            // SAFETY: exclusive test ownership; no process or GUI thread exists.
            unsafe {
                params.cells[0]
                    .model
                    ._internal_set_plain_value(i32::from(spec.id.raw()));
                for route in params.cells[0].routes.all() {
                    route
                        .source
                        ._internal_set_plain_value(crate::routes::SourceChoice::Lfo1);
                    route.target._internal_set_plain_value(1);
                }
                for (_, _, sync) in params.lfos() {
                    sync._internal_set_plain_value(true);
                }
            }
            let host = Host;
            let setter = ParamSetter::new(&host);
            let floors: Vec<f32> = items_in(&params, 0)
                .iter()
                .map(|item| item.card.floor)
                .collect();
            for (index, title) in TITLES.iter().enumerate() {
                let mut selected = 0;
                let mut entries = HashMap::new();
                let mut live = Live {
                    params: &params,
                    setter: &setter,
                    cell: 0,
                    selected: &mut selected,
                    entries: &mut entries,
                    charges: [0.5; CELLS],
                    flows: [1.0; EDGES],
                    pool: 2.0,
                    text_editing: false,
                    tempo: Some(120.0),
                    audition: &|_| {},
                };
                tree_checks::card(
                    &|_| {},
                    spec.label,
                    title,
                    floors[index],
                    &|ui| card(ui, index, &params, 0),
                    &mut |ui, leaf, rect| paint(ui, &mxm_ui::LIGHT, leaf, rect, &mut live),
                );
            }
        }
    }
}
