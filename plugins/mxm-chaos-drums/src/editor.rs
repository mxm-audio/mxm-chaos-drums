//! The editor (the brief's §4–§6): the grid of cells and the kit's dials, the LFOs, then the
//! selected cell's model, charge, feeds, controls and output.
//!
//! Every card is one tree (`mxm_ui::tree`), measured for its floor and drawn leaf by leaf through
//! the collection's bindings (`mxm_params::binding`), as the drum machine's are. **A model shows only
//! the controls it reads**, under its own names. There is no Routes card: each knob's routes are rows
//! under it, over the cell's or the kit's four route slots, as the drum machine draws them.

use std::collections::HashMap;
use std::sync::Arc;

use egui::Ui;
use mxm_chaos_drums_dsp::{
    AVAILABLE_MODELS, CELLS, CONTROLS, CellTarget, EDGES, ROUTES, SIDE, available, edge,
};
use mxm_params::binding::{self, Bound, ErasedParam};
use mxm_preset::PresetUi;
use mxm_ui::control::{self, ParamView, Size, Steps, Wave, Wheel};
use mxm_ui::space::{MIN_TARGET, SPACE_2, SPACE_3, SPACE_5};
use mxm_ui::theme::Tokens;
use mxm_ui::tree::{
    self, Flow, Font, Height, Kind, Node, group, leaf, pad, pad_all, row_gap, stack, stack_gap,
};
use nice_plug::context::gui::GuiContext;
use nice_plug::prelude::*;

use crate::params::{KIT_IDS, MxmChaosDrumsParams, at, cell_ids};
use crate::routes::{
    Choice, KIT_TARGETS, KitSourceChoice, SourceChoice, cell_target_index, cell_target_name,
};
use crate::telemetry::Telemetry;

/// The opening size: the quarter-4K budget, hugged. Measured by `the_opening_size_is_the_budget_hugged`,
/// which fails with the size it should be.
const REFERENCE: (u32, u32) = (1346, 791);
/// The smallest window advertised: the widest card with the workspace's gutters, or the app bar.
const MINIMUM: (u32, u32) = (444, 320);
/// The keyboard cursor's card for the app bar's Master, outside the paging keys.
const MASTER_CARD: u64 = 64;
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

const TITLES: [&str; 8] = [
    "Cells", "Kit", "LFOs", "Model", "Charge", "Feeds", "Controls", "Output",
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
    const CATEGORIES: [Category; 8] = [
        Category::Performance,
        Category::Performance,
        Category::Modulators,
        Category::Generators,
        Category::Generators,
        Category::Generators,
        Category::Generators,
        Category::Tone,
    ];
    const KINDS: [&str; 8] = [
        "Cells", "Kit", "LFO", "Model", "Shape", "Shape", "Shape", "Output",
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
    /// A knob's routes, by the target parameter's value they carry: the selected cell's, or the
    /// kit's.
    Routes(i32),
    KitRoutes(i32),
    Model,
    Notes,
    NoControls,
    Knob(usize),
    Bus,
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
                    min_width: grid_width(ui),
                    height: Height::Fixed(grid_height(ui)),
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
            rows.extend(stacks((1..KIT_TARGETS.len() as i32).map(|target| {
                let stack = kit_stack(params, target);
                (Leaf::KitRoutes(target), stack.size(ui))
            })));
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
        3 => {
            let parameters = p.parameters();
            let mut body = vec![
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
            ];
            body.extend(cell_stacks(ui, params, cell, [CellTarget::Pitch]));
            body.push(pad(
                SPACE_3,
                leaf(
                    Leaf::Notes,
                    Kind::Text {
                        text: notes(p).to_owned(),
                        font: Font::Caption,
                        flow: Flow::Wrap,
                    },
                ),
            ));
            stack(body)
        }
        4 => {
            let parameters = p.parameters();
            let mut body = vec![
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
            ];
            body.extend(cell_stacks(
                ui,
                params,
                cell,
                [
                    CellTarget::Strike,
                    CellTarget::Supply,
                    CellTarget::Hunger,
                    CellTarget::Leak,
                    CellTarget::Threshold,
                ],
            ));
            stack(body)
        }
        5 => {
            let parameters = p.parameters();
            let mut body = vec![
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
            ];
            body.extend(cell_stacks(
                ui,
                params,
                cell,
                [CellTarget::Link, CellTarget::Touch],
            ));
            stack(body)
        }
        6 => {
            let parameters = p.parameters();
            let controls = available(p.model_id()).map_or(&[][..], |spec| spec.controls);
            // The model's controls in panel order, then any control it does not read that a route
            // is still aimed at, so a route a change of model left behind is seen and can be removed.
            let read = |k: usize| controls.iter().any(|c| usize::from(c.number) == k + 1);
            let targets = controls
                .iter()
                .map(|c| CellTarget::Control(c.number - 1))
                .chain(
                    (0..CONTROLS)
                        .filter(|&k| !read(k))
                        .map(|k| CellTarget::Control(k as u8))
                        .filter(|&target| !cell_stack(params, cell, target).on().is_empty()),
                );
            let mut body: Vec<Node<Leaf>> = if controls.is_empty() {
                vec![leaf(
                    Leaf::NoControls,
                    Kind::Text {
                        text: NO_CONTROLS.to_owned(),
                        font: Font::Caption,
                        flow: Flow::Wrap,
                    },
                )]
            } else {
                controls
                    .chunks(3)
                    .map(|row| {
                        let row = row.iter().map(|c| {
                            let i = at::control(usize::from(c.number));
                            (Leaf::Knob(i), parameters[i])
                        });
                        knobs_named(ui, row, |leaf| control_name(params, cell, leaf))
                    })
                    .collect()
            };
            body.extend(cell_stacks(ui, params, cell, targets));
            stack(body)
        }
        _ => {
            let parameters = p.parameters();
            let mut body = vec![knobs(
                ui,
                [at::LEVEL, at::PAN]
                    .map(|i| (Leaf::Knob(i), parameters[i]))
                    .into_iter(),
            )];
            body.extend(cell_stacks(
                ui,
                params,
                cell,
                [CellTarget::Level, CellTarget::Pan],
            ));
            // Mute and Solo are on the cell's square, as the drum machine's are on its slot rows.
            body.extend([
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
            ]);
            stack(body)
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

/// Route stacks as leaves, each sized as it stands ([`Stack::size`]) and filling the card's width,
/// the first `SPACE_3` below the knobs above them.
fn stacks(sized: impl Iterator<Item = (Leaf, egui::Vec2)>) -> Vec<Node<Leaf>> {
    sized
        .enumerate()
        .map(|(n, (key, size))| {
            let node = leaf(
                key,
                Kind::Custom {
                    min_width: size.x,
                    height: Height::Fixed(size.y),
                    fills: true,
                },
            );
            if n == 0 { pad(SPACE_3, node) } else { node }
        })
        .collect()
}

/// The selected cell's route stacks for `targets`, in that order.
fn cell_stacks(
    ui: &Ui,
    params: &MxmChaosDrumsParams,
    cell: usize,
    targets: impl IntoIterator<Item = CellTarget>,
) -> Vec<Node<Leaf>> {
    stacks(targets.into_iter().map(|target| {
        (
            Leaf::Routes(cell_target_index(target)),
            cell_stack(params, cell, target).size(ui),
        )
    }))
}

/// One route slot as a knob's routes read it: a cell's or the kit's.
struct Slot<'a, S: Choice> {
    source: &'a EnumParam<S>,
    target: &'a IntParam,
    amount: &'a FloatParam,
    /// Its source's and amount's ids, which the keyboard cursor registers.
    source_id: &'static str,
    amount_id: &'static str,
}

impl<S: Choice> Slot<'_, S> {
    /// Free to take: its source or its target is Off.
    fn free(&self) -> bool {
        self.source.value() == S::OFF || self.target.value() == 0
    }
}

/// A knob's routes: the four slots that can carry them, the target they carry, and the knob's name,
/// which every row reads (*Drop from LFO 1*).
struct Stack<'a, S: Choice> {
    slots: [Slot<'a, S>; ROUTES],
    target: i32,
    name: String,
    /// Whether the panel shows its knob. A control the model does not read keeps the routes still
    /// aimed at it drawn, so they can be removed, and offers nothing.
    shown: bool,
}

/// The selected cell's routes on `target`, named as their knob is.
fn cell_stack(
    params: &MxmChaosDrumsParams,
    cell: usize,
    target: CellTarget,
) -> Stack<'_, SourceChoice> {
    let p = &params.cells[cell];
    let ids = cell_ids(cell);
    let routes = p.routes.all();
    let (name, shown) = match target {
        CellTarget::Control(k) => available(p.model_id())
            .and_then(|spec| spec.controls.iter().find(|c| c.number == k + 1))
            .map_or_else(
                || {
                    (
                        mxm_chaos_drums_dsp::GENERAL_NAMES[usize::from(k)].to_owned(),
                        false,
                    )
                },
                |c| (c.name.to_owned(), true),
            ),
        target => (cell_target_name(cell_target_index(target)), true),
    };
    Stack {
        slots: std::array::from_fn(|r| Slot {
            source: &routes[r].source,
            target: &routes[r].target,
            amount: &routes[r].amount,
            source_id: ids[at::route(r)[0]],
            amount_id: ids[at::route(r)[2]],
        }),
        target: cell_target_index(target),
        name,
        shown,
    }
}

/// The kit's routes on the target `KIT_TARGETS[target]` names.
fn kit_stack(params: &MxmChaosDrumsParams, target: i32) -> Stack<'_, KitSourceChoice> {
    let routes = params.kit_routes.all();
    Stack {
        slots: std::array::from_fn(|r| Slot {
            source: &routes[r].source,
            target: &routes[r].target,
            amount: &routes[r].amount,
            source_id: KIT_IDS[20 + 3 * r],
            amount_id: KIT_IDS[22 + 3 * r],
        }),
        target,
        name: KIT_TARGETS[target as usize].1.to_owned(),
        shown: true,
    }
}

impl<S: Choice> Stack<'_, S> {
    /// The slots carrying a source onto this target.
    fn on(&self) -> Vec<usize> {
        (0..ROUTES)
            .filter(|&r| {
                let slot = &self.slots[r];
                slot.source.value() != S::OFF && slot.target.value() == self.target
            })
            .collect()
    }

    /// The slot adding a source takes: a free one already aimed here first (a source removed from
    /// this knob, so adding one back keeps its depth), else the first free one.
    fn free(&self) -> Option<usize> {
        (0..ROUTES)
            .find(|&r| self.slots[r].free() && self.slots[r].target.value() == self.target)
            .or_else(|| (0..ROUTES).find(|&r| self.slots[r].free()))
    }

    /// What `‹ modulate ›` offers: the sources not already moving it, while a slot is free, on a knob
    /// the panel shows.
    fn offered(&self) -> Vec<S> {
        if !self.shown || self.free().is_none() {
            return Vec::new();
        }
        let on: Vec<S> = self
            .on()
            .into_iter()
            .map(|r| self.slots[r].source.value())
            .collect();
        S::SOURCES
            .iter()
            .copied()
            .filter(|s| !on.contains(s))
            .collect()
    }

    /// The line over the rows: the knob's name, and that it is unused where the model does not read
    /// it.
    fn label(&self) -> String {
        if self.shown {
            self.name.clone()
        } else {
            format!("{} (unused)", self.name)
        }
    }

    /// Adds `source` in one gesture: the free slot's source and target, and its amount to zero
    /// unless the slot was already aimed here.
    fn add(&self, source: S, setter: &ParamSetter<'_>) {
        let Some(r) = self.free() else {
            return;
        };
        let slot = &self.slots[r];
        let mut edits: Vec<(&dyn ErasedParam, f32)> = vec![
            (slot.source, slot.source.preview_normalized(source)),
            (slot.target, slot.target.preview_normalized(self.target)),
        ];
        if slot.target.value() != self.target {
            edits.push((slot.amount, slot.amount.preview_normalized(0.0)));
        }
        binding::set_together(setter, &edits);
    }

    /// What it occupies, without drawing it: at its narrowest the widest row any source could
    /// draw, or its line; its height as the patch stands. The collection's own stack's rule
    /// (`mxm_modulation_params::ui::stack_size`), over route slots.
    fn size(&self, ui: &Ui) -> egui::Vec2 {
        let spacing = ui.spacing().item_spacing;
        let inset = 2.0 * tree::GROUP_INSET;
        let amount: &dyn ErasedParam = self.slots[0].amount;
        let widest = control::widest_value(|n| amount.format(n as f32));
        let row = |source: S| {
            let slider = control::slider_size(ui, source.name(), true, &widest);
            egui::Vec2::new(
                slider.x.max(mxm_modulation_params::ui::TRACK_MIN)
                    + spacing.x
                    + control::REMOVE_SIZE,
                slider.y.max(control::REMOVE_SIZE),
            )
        };
        let line = control::selector_size(ui, &self.label(), &["modulate"]);
        let widest_row = S::SOURCES.iter().map(|s| row(*s).x).fold(0.0, f32::max);
        let width = (widest_row.max(line.x) + inset).max(line.x);
        let rows: Vec<f32> = self
            .on()
            .into_iter()
            .map(|r| row(self.slots[r].source.value()).y)
            .collect();
        let height = if !rows.is_empty() {
            inset + line.y + SPACE_2 + rows.iter().sum::<f32>() + SPACE_2 * (rows.len() - 1) as f32
        } else if !self.offered().is_empty() {
            line.y
        } else {
            0.0
        };
        egui::Vec2::new(width, height)
    }

    /// Its line offering `‹ modulate ›`, and a row for each route that moves it.
    fn draw(
        &self,
        ui: &mut Ui,
        tokens: &Tokens,
        setter: &ParamSetter<'_>,
        entries: &mut HashMap<&'static str, Option<String>>,
    ) {
        let on = self.on();
        let offered = self.offered();
        if !on.is_empty() {
            mxm_ui::shell::group(ui, tokens, |ui| {
                self.line(ui, tokens, &offered, setter);
                for r in on {
                    self.row(ui, tokens, r, setter, entries);
                }
            });
        } else if !offered.is_empty() {
            self.line(ui, tokens, &offered, setter);
        }
    }

    /// The knob's line: its name, and `‹ modulate ›` offering the sources it can still take. Over
    /// a control the model does not read, the line says it is unused and offers nothing.
    fn line(&self, ui: &mut Ui, tokens: &Tokens, offered: &[S], setter: &ParamSetter<'_>) {
        let label = self.label();
        if offered.is_empty() {
            ui.horizontal(|ui| {
                ui.set_min_height(MIN_TARGET);
                let line = ui.label(egui::RichText::new(&label).color(tokens.text_primary));
                if !self.shown {
                    line.on_hover_text(format!(
                        "This model has no {}, so these routes move nothing. Remove them to free their slots.",
                        self.name
                    ));
                }
            });
            return;
        }
        let labels: Vec<&str> = std::iter::once("modulate")
            .chain(offered.iter().map(|s| s.name()))
            .collect();
        let mut chosen = 0usize;
        let description = format!("Add a modulation source to {label}");
        let changed = control::selector(
            ui,
            tokens,
            &label,
            &labels,
            &mut chosen,
            None,
            Some(0),
            &description,
        );
        if changed && chosen > 0 {
            self.add(offered[chosen - 1], setter);
        }
    }

    /// One route: its amount, painted as its source and named in full, with a remove beside it
    /// that switches the source off, one write, the depth kept.
    fn row(
        &self,
        ui: &mut Ui,
        tokens: &Tokens,
        r: usize,
        setter: &ParamSetter<'_>,
        entries: &mut HashMap<&'static str, Option<String>>,
    ) {
        let slot = &self.slots[r];
        let source = slot.source.value().name();
        let target = &self.name;
        let amount: &dyn ErasedParam = slot.amount;
        let name = format!("{target} from {source}");
        let text = amount.text();
        let description = if self.shown {
            format!("How much {source} moves {target}. Signed: below the centre inverts it.")
        } else {
            format!("This model has no {target}, so {source} moves nothing here.")
        };
        let (fine_up, fine_down, coarse_up, coarse_down) = amount.stepping();
        let view = ParamView {
            name: &name,
            label: source,
            text: &text,
            widest: "",
            description: &description,
            default: f64::from(amount.default_normalised()),
            bipolar: true,
            read_only: false,
            modulation: f64::from(amount.modulation()),
            marked: amount.modulation() != 0.0,
            steps: Steps {
                fine_up,
                fine_down,
                coarse_up,
                coarse_down,
            },
            next: None,
            quiet_label: true,
        };
        let mut normalised = f64::from(amount.normalised());
        let entry = entries.entry(slot.amount_id).or_default();
        let mut removed = false;
        let outcome = ui
            .horizontal(|ui| {
                // The remove's width is reserved before the slider takes the rest.
                let gap = ui.spacing().item_spacing.x;
                let track = (ui.available_width() - control::REMOVE_SIZE - gap)
                    .max(mxm_modulation_params::ui::TRACK_MIN);
                let out = ui
                    .scope(|ui| {
                        ui.set_max_width(track);
                        mxm_ui::navigation::at(ui, slot.amount_id, |ui| {
                            control::slider(
                                ui,
                                tokens,
                                &view,
                                &mut normalised,
                                track,
                                entry,
                                Wheel::FocusOrModifier,
                            )
                        })
                    })
                    .inner;
                // Bottom-aligned onto the slider's track, as the collection's route rows are.
                let height = ui.min_rect().height().max(control::REMOVE_SIZE);
                ui.allocate_ui_with_layout(
                    egui::Vec2::new(control::REMOVE_SIZE, height),
                    egui::Layout::bottom_up(egui::Align::Center),
                    |ui| {
                        mxm_ui::navigation::at(ui, slot.source_id, |ui| {
                            if control::remove_mark(
                                ui,
                                tokens,
                                control::REMOVE_SIZE,
                                true,
                                &format!("Remove {source} from {target}"),
                                ErasedParam::modulation(slot.source) != 0.0,
                                "The depth stays, so adding the source back restores it.",
                            ) {
                                binding::set_together(
                                    setter,
                                    &[(
                                        slot.source as &dyn ErasedParam,
                                        slot.source.preview_normalized(S::OFF),
                                    )],
                                );
                                removed = true;
                            }
                        });
                    },
                );
                out
            })
            .inner;
        if removed {
            return;
        }
        if outcome.gesture_started {
            amount.begin(setter);
        }
        if outcome.changed {
            amount.set(setter, normalised as f32);
        }
        if outcome.gesture_ended {
            amount.end(setter);
        }
    }
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

/// The fonts of a square's two lines: its number and name, and its model.
const NAME_FONT: f32 = 13.0;
const MODEL_FONT: f32 = 11.0;
/// How far a square's text sits in from its edge, and its M and S from the corner.
const TEXT_INSET: f32 = 6.0;
const BUTTON_INSET: f32 = 4.0;

fn text_size(ui: &Ui, text: &str, size: f32) -> egui::Vec2 {
    ui.ctx().fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(
                text.to_owned(),
                egui::FontId::proportional(size),
                egui::Color32::PLACEHOLDER,
            )
            .rect
            .size()
    })
}

/// Mute and Solo side by side, as the drum machine's slot rows have them.
fn toggles_width(ui: &Ui) -> f32 {
    2.0 * control::toggle_compact_size().x + ui.spacing().item_spacing.x
}

/// One cell's square at its narrowest, measured from what it holds (the brief's §7): the widest
/// made-up name beside M and S on its top line, and the widest model label on its bottom one.
fn square(ui: &Ui) -> egui::Vec2 {
    let name = mxm_chaos_drums_dsp::Kind::ALL
        .iter()
        .map(|kind| {
            text_size(
                ui,
                &format!("{CELLS:02}  {}{CELLS}", kind.code()),
                NAME_FONT,
            )
            .x
        })
        .fold(0.0, f32::max);
    let model = AVAILABLE_MODELS
        .iter()
        .map(|spec| text_size(ui, spec.label, MODEL_FONT).x)
        .fold(0.0, f32::max);
    let top = TEXT_INSET + name + ui.spacing().item_spacing.x + toggles_width(ui) + BUTTON_INSET;
    let bottom = 2.0 * TEXT_INSET + model;
    let height = BUTTON_INSET
        + control::toggle_compact_size().y
        + SPACE_2
        + text_size(ui, "Ag", MODEL_FONT).y
        + TEXT_INSET;
    egui::vec2(top.max(bottom).ceil(), height.ceil())
}

fn grid_width(ui: &Ui) -> f32 {
    SIDE as f32 * square(ui).x + (SIDE - 1) as f32 * SPACE_2
}

fn grid_height(ui: &Ui) -> f32 {
    SIDE as f32 * square(ui).y + (SIDE - 1) as f32 * SPACE_2
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
        Leaf::Routes(target) => cell_stack(params, cell, crate::routes::cell_target(target)).draw(
            ui,
            tokens,
            setter,
            live.entries,
        ),
        Leaf::KitRoutes(target) => {
            kit_stack(params, target).draw(ui, tokens, setter, live.entries);
        }
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

/// The grid: each cell's square with its number, name, model and charge, its Mute and Solo, and the
/// charge moving across each edge drawn between them. A click, or OPEN with the cursor on it,
/// selects the cell and strikes it, to hear it while editing; Solo selects it too, as the drum
/// machine's does.
fn grid(ui: &mut Ui, tokens: &Tokens, rect: egui::Rect, live: &mut Live<'_, '_>) {
    let gap = SPACE_2;
    let width = (rect.width() - (SIDE - 1) as f32 * gap) / SIDE as f32;
    let height = (rect.height() - (SIDE - 1) as f32 * gap) / SIDE as f32;
    let square = |cell: usize| {
        let (row, col) = ((cell / SIDE) as f32, (cell % SIDE) as f32);
        egui::Rect::from_min_size(
            rect.min + egui::vec2(col * (width + gap), row * (height + gap)),
            egui::vec2(width, height),
        )
    };
    let toggles = egui::vec2(toggles_width(ui), control::toggle_compact_size().y);
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
        // M and S in the top right corner; the name stops short of them.
        let buttons = egui::Rect::from_min_size(
            egui::pos2(r.right() - BUTTON_INSET - toggles.x, r.top() + BUTTON_INSET),
            toggles,
        );
        let top = egui::Rect::from_min_max(
            r.min + egui::vec2(4.0, 4.0),
            egui::pos2(
                buttons.left() - ui.spacing().item_spacing.x,
                r.bottom() - 4.0,
            ),
        );
        painter.with_clip_rect(top).text(
            egui::pos2(r.left() + TEXT_INSET, buttons.center().y),
            egui::Align2::LEFT_CENTER,
            format!("{:02}  {name}", cell + 1),
            egui::FontId::proportional(NAME_FONT),
            tokens.text_primary,
        );
        painter.with_clip_rect(r.shrink(4.0)).text(
            r.left_bottom() + egui::vec2(TEXT_INSET, -TEXT_INSET),
            egui::Align2::LEFT_BOTTOM,
            model,
            egui::FontId::proportional(MODEL_FONT),
            tokens.text_secondary,
        );
        let p = &live.params.cells[cell];
        let ids = cell_ids(cell);
        let setter = live.setter;
        let solo = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .max_rect(buttons)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    ui.add_enabled_ui(!live.text_editing, |ui| {
                        binding::toggle_compact(
                            ui,
                            tokens,
                            ids[at::MUTE],
                            "M",
                            &p.mute,
                            "Silence this cell without stopping it: its charge and its neighbours carry on.",
                            setter,
                        );
                        binding::toggle_compact(
                            ui,
                            tokens,
                            ids[at::SOLO],
                            "S",
                            &p.solo,
                            "Hear this cell on its own, and select it for editing.",
                            setter,
                        )
                    })
                    .inner
                },
            )
            .inner;
        if solo {
            *live.selected = cell;
        }
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

    /// Every card for every model, with every route in use, on its knob's tallest stack, and every
    /// LFO synced, passes the collection's tree checks: its floor holds it and nothing paints
    /// outside it.
    #[test]
    fn every_card_passes_the_tree_checks_for_every_model() {
        use nice_plug::params::InternalParamMut;
        for spec in AVAILABLE_MODELS {
            let params = MxmChaosDrumsParams::default();
            // Four sources on one knob each: the cell's on its model's first control, or on Pitch
            // for a model of none, and the kit's on Temper.
            let target = spec
                .controls
                .first()
                .map_or(CellTarget::Pitch, |c| CellTarget::Control(c.number - 1));
            for (r, source) in SourceChoice::SOURCES.iter().take(ROUTES).enumerate() {
                route(&params, r, *source, target, 0.5);
            }
            // SAFETY: exclusive test ownership; no process or GUI thread exists.
            unsafe {
                params.cells[0]
                    .model
                    ._internal_set_plain_value(i32::from(spec.id.raw()));
                for (route, source) in params.kit_routes.all().iter().zip(KitSourceChoice::SOURCES)
                {
                    route.source._internal_set_plain_value(*source);
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

    /// Sets cell 1's route `r`.
    fn route(
        params: &MxmChaosDrumsParams,
        r: usize,
        source: SourceChoice,
        target: CellTarget,
        amount: f32,
    ) {
        use nice_plug::params::InternalParamMut;
        let route = params.cells[0].routes.all()[r];
        // SAFETY: exclusive test ownership; no process or GUI thread exists.
        unsafe {
            route.source._internal_set_plain_value(source);
            route
                .target
                ._internal_set_plain_value(cell_target_index(target));
            route.amount._internal_set_plain_value(amount);
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        Begin,
        Set,
        End,
    }

    /// A host that applies each write at once and keeps the order of every call.
    #[derive(Default)]
    struct RecordingHost {
        calls: std::sync::Mutex<Vec<Call>>,
    }

    impl nice_plug::context::gui::GuiContextInner for RecordingHost {
        fn request_restart(&self) {}
        fn plugin_api(&self) -> nice_plug::prelude::PluginApi {
            nice_plug::prelude::PluginApi::Clap
        }
        unsafe fn raw_begin_set_parameter(&self, _param: nice_plug::params::internals::ParamPtr) {
            self.calls.lock().unwrap().push(Call::Begin);
        }
        unsafe fn raw_set_parameter_normalized(
            &self,
            param: nice_plug::params::internals::ParamPtr,
            normalized: f32,
        ) {
            self.calls.lock().unwrap().push(Call::Set);
            // SAFETY: the test owns the parameters and no other thread touches them.
            unsafe { param._internal_set_normalized_value(normalized) };
        }
        unsafe fn raw_end_set_parameter(&self, _param: nice_plug::params::internals::ParamPtr) {
            self.calls.lock().unwrap().push(Call::End);
        }
        fn get_state(&self) -> nice_plug::prelude::PluginState {
            nice_plug::prelude::PluginState {
                version: String::new(),
                params: Default::default(),
                fields: Default::default(),
            }
        }
        fn set_state(&self, _state: nice_plug::prelude::PluginState) {}
    }

    #[test]
    fn adding_a_route_takes_the_first_free_slot_in_one_gesture() {
        use nice_plug::params::InternalParamMut;
        let params = MxmChaosDrumsParams::default();
        // Route 1 busy on Level; routes 2–4 free, route 2 with a depth left from elsewhere.
        route(&params, 0, SourceChoice::Lfo1, CellTarget::Level, 0.5);
        route(&params, 1, SourceChoice::Off, CellTarget::Pan, 0.7);
        let host = RecordingHost::default();
        let setter = ParamSetter::new(&host);
        let r2 = params.cells[0].routes.all()[1];
        let supply = cell_stack(&params, 0, CellTarget::Supply);
        supply.add(SourceChoice::Wheel, &setter);
        assert_eq!(r2.source.value(), SourceChoice::Wheel);
        assert_eq!(r2.target.value(), cell_target_index(CellTarget::Supply));
        assert_eq!(
            r2.amount.value(),
            0.0,
            "a depth aimed elsewhere is not inherited"
        );
        assert_eq!(cell_stack(&params, 0, CellTarget::Supply).on(), vec![1]);
        let calls = host.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 9, "three writes, each bracketed: {calls:?}");
        let phase = |call: &Call| *call as u8;
        assert!(calls.windows(2).all(|w| phase(&w[0]) <= phase(&w[1])));

        // Removed and added back, the route keeps its depth.
        // SAFETY: exclusive test ownership.
        unsafe {
            r2.amount._internal_set_plain_value(-0.4);
            r2.source._internal_set_plain_value(SourceChoice::Off);
        }
        let supply = cell_stack(&params, 0, CellTarget::Supply);
        assert!(supply.on().is_empty());
        supply.add(SourceChoice::Lfo2, &setter);
        assert_eq!(r2.amount.value(), -0.4);
        assert_eq!(r2.source.value(), SourceChoice::Lfo2);
    }

    /// With every slot taken, nothing more is offered anywhere; a knob's own sources are never
    /// offered twice; a control the model does not read offers nothing, and says so.
    #[test]
    fn a_knob_offers_what_it_can_still_take() {
        let params = MxmChaosDrumsParams::default();
        let (strike, leak) = (CellTarget::Strike, CellTarget::Leak);
        assert_eq!(
            cell_stack(&params, 0, strike).offered().len(),
            SourceChoice::SOURCES.len()
        );
        route(&params, 0, SourceChoice::Lfo1, strike, 0.2);
        assert!(
            !cell_stack(&params, 0, strike)
                .offered()
                .contains(&SourceChoice::Lfo1)
        );
        assert!(
            cell_stack(&params, 0, leak)
                .offered()
                .contains(&SourceChoice::Lfo1)
        );
        // The Draining kick reads no control 1.
        let unused = cell_stack(&params, 0, CellTarget::Control(0));
        assert!(!unused.shown && unused.offered().is_empty());
        assert_eq!(unused.label(), "Tone (unused)");
        let drop = cell_stack(&params, 0, CellTarget::Control(2));
        assert!(drop.shown && drop.name == "Drop");
        for (r, source) in SourceChoice::SOURCES
            .iter()
            .enumerate()
            .take(ROUTES)
            .skip(1)
        {
            route(&params, r, *source, CellTarget::Pan, 0.2);
        }
        assert!(cell_stack(&params, 0, leak).offered().is_empty());
        assert!(cell_stack(&params, 0, leak).free().is_none());
    }

    #[test]
    fn a_kit_knob_takes_the_kits_routes() {
        let params = MxmChaosDrumsParams::default();
        let host = RecordingHost::default();
        let setter = ParamSetter::new(&host);
        let temper = kit_stack(&params, 1);
        assert_eq!(temper.name, "Temper");
        assert_eq!(temper.offered(), KitSourceChoice::SOURCES);
        temper.add(KitSourceChoice::Lfo3, &setter);
        let r1 = params.kit_routes.all()[0];
        assert_eq!(r1.source.value(), KitSourceChoice::Lfo3);
        assert_eq!(r1.target.value(), 1);
        assert_eq!(kit_stack(&params, 1).on(), vec![0]);
        assert!(
            kit_stack(&params, 2).on().is_empty(),
            "Flow carries nothing"
        );
    }

    /// Every square's Mute and Solo join the keyboard cursor, not only the selected cell's.
    #[test]
    fn every_squares_mute_and_solo_join_the_keyboard_cursor() {
        let (params, telemetry) = (MxmChaosDrumsParams::default(), Telemetry::default());
        let recorder = Arc::new(keyboard_checks::Recorder::default());
        let setter = ParamSetter::new(recorder.as_ref());
        let mut state = state(&params);
        let mut panel = drive(&params, &telemetry, &setter, &mut state);
        let session = keyboard_checks::Session::new(reference());
        mxm_ui::paging::editor::request_card(session.context(), mxm_ui::paging::Key(0));
        REVEAL(session.context());
        session.settle(&mut panel);
        let found: Vec<String> = session.registered().into_iter().collect();
        for cell in 0..CELLS {
            let ids = cell_ids(cell);
            for id in [ids[at::MUTE], ids[at::SOLO]] {
                assert!(
                    found.iter().any(|f| f == id),
                    "{id} is out of the cursor's reach"
                );
            }
        }
    }
}
