//! `mxm-chaos-drums` — an original sixteen-cell drum instrument that runs on charge.
//!
//! The thin nice-plug shell over `mxm-chaos-drums-dsp`: parameters, the host contract, note
//! ownership and the outputs. A cell key (36–51) strikes a cell and holds its gate while the note
//! is held; a touch key (52–67) touches it. Who owns each key's note is `mxm-part-routing`'s
//! ownership, so a late release from an older press is ignored and a host's choke still finds a
//! released note's tail.

macro_rules! plugin_name {
    () => {
        "mxm-chaos-drums"
    };
}

pub const NAME: &str = plugin_name!();
pub const CLAP_ID: &str = concat!("dk.mxm.", plugin_name!());

pub mod editor;
pub mod params;
pub mod preset;
pub mod routes;
pub mod slot_names;
pub mod telemetry;

use std::num::NonZeroU32;
use std::sync::Arc;

use mxm_chaos_drums_dsp::{CELLS, Event, EventKind, Kit, MIN_SAMPLE_RATE, OutputFrame};
use mxm_part_routing::{
    ArbitrationResult, NoteAddress, NoteCandidate, NoteOwner, PartEvent, PartPolicy, PartState,
    ResolveFlags,
};
use nice_plug::midi::{Channel, Key, VoiceID};
use nice_plug::prelude::*;

use params::MxmChaosDrumsParams;
use telemetry::Telemetry;

/// How hard a cell is struck when the editor asks to hear it: 100 of MIDI's 127, as the drum
/// machine's audition.
pub const AUDITION_VELOCITY: f32 = 100.0 / 127.0;

/// The editor's one task: wake a host that has let the plugin sleep, so a cell asked to be heard
/// sounds now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    Wake,
}

/// The key cell 1 answers on; cell `n` on `FIRST_KEY + n`.
pub(crate) const FIRST_KEY: u8 = 36;
/// The key cell 1's touch answers on, just above the cell keys.
pub(crate) const FIRST_TOUCH_KEY: u8 = FIRST_KEY + CELLS as u8;
/// A part a key: the sixteen cell keys, then the sixteen touch keys.
const PARTS: usize = 2 * CELLS;
const NUM_CHANNELS: usize = 16;
const MAX_GROUP_NOTE_EVENTS: usize = 256;
/// The most DSP events one sample can make: a strike or touch, a release and a choke a part, a
/// tuning a cell, and the wheel, the pressure, a panic and a transport reset.
const MAX_SAMPLE_EVENTS: usize = 3 * PARTS + CELLS + 4;
const INDIVIDUAL_OUTPUT_PORTS: [NonZeroU32; CELLS] = [new_nonzero_u32(1); CELLS];
/// Named by cell number only, as the drum machine's: a host keeps a port's first name.
const INDIVIDUAL_OUTPUT_NAMES: [&str; CELLS] = [
    "Cell 01", "Cell 02", "Cell 03", "Cell 04", "Cell 05", "Cell 06", "Cell 07", "Cell 08",
    "Cell 09", "Cell 10", "Cell 11", "Cell 12", "Cell 13", "Cell 14", "Cell 15", "Cell 16",
];

/// A note's identity as `mxm-part-routing` takes it, a host's wildcards as 255.
fn legacy_note(voice_id: VoiceID, channel: Channel, key: Key) -> (Option<i32>, u8, u8) {
    (
        voice_id.id(),
        channel.number().unwrap_or(u8::MAX),
        key.number().unwrap_or(u8::MAX),
    )
}

fn note_address(channel: u8, note: u8, note_id: Option<i32>) -> NoteAddress {
    NoteAddress {
        channel: (channel != u8::MAX).then_some(channel),
        key: (note != u8::MAX).then_some(note),
        note_id,
    }
}

/// One sample offset's events, as `mxm_part_routing` collects them.
type EventGroup = mxm_part_routing::EventGroup<MAX_GROUP_NOTE_EVENTS, NUM_CHANNELS>;

/// Puts a host event into the group, as the drum machine does.
trait Collect {
    fn collect(&mut self, event: NoteEvent<()>);
}

impl Collect for EventGroup {
    fn collect(&mut self, event: NoteEvent<()>) {
        match event {
            NoteEvent::NoteOn {
                voice_id,
                channel,
                key,
                velocity,
                ..
            } if velocity.is_finite() && velocity > 0.0 => {
                let (voice_id, channel, note) = legacy_note(voice_id, channel, key);
                // A same-sample flood is panic rather than a chord whose winner depends on which
                // event crossed the bound.
                if !self.note_on(NoteCandidate {
                    owner: NoteOwner {
                        channel,
                        key: note,
                        note_id: voice_id,
                    },
                    strike: velocity.clamp(0.0, 1.0),
                }) {
                    self.panic();
                }
            }
            NoteEvent::NoteOn {
                voice_id,
                channel,
                key,
                ..
            }
            | NoteEvent::NoteOff {
                voice_id,
                channel,
                key,
                ..
            } => {
                let (voice_id, channel, note) = legacy_note(voice_id, channel, key);
                self.release(note_address(channel, note, voice_id));
            }
            NoteEvent::Choke {
                voice_id,
                channel,
                key,
                ..
            } => {
                let (voice_id, channel, note) = legacy_note(voice_id, channel, key);
                self.choke(note_address(channel, note, voice_id));
            }
            NoteEvent::PolyTuning {
                voice_id,
                channel,
                key,
                tuning,
                ..
            } if tuning.is_finite() => {
                let (voice_id, channel, note) = legacy_note(voice_id, channel, key);
                self.tune(
                    note_address(channel, note, voice_id),
                    tuning.clamp(-48.0, 48.0),
                );
            }
            NoteEvent::MidiChannelPressure {
                channel, pressure, ..
            } if pressure.is_finite() => {
                self.pressure(channel as usize, pressure.clamp(0.0, 1.0));
            }
            NoteEvent::MidiCC {
                channel,
                cc: 1,
                value,
                ..
            } if value.is_finite() => {
                self.wheel(channel as usize, value.clamp(0.0, 1.0));
            }
            NoteEvent::MidiCC { cc, .. } if cc == control_change::ALL_SOUND_OFF => self.panic(),
            NoteEvent::MidiCC { cc, .. } if cc == control_change::ALL_NOTES_OFF => {
                self.all_notes_off()
            }
            _ => {}
        }
    }
}

/// This instrument's note laws: a part plays its own key on any channel; a total order picks the
/// owner, so host order never does; a hit is its own velocity.
struct NoteLaws;

impl PartPolicy for NoteLaws {
    fn accepts(&self, part: usize, owner: NoteOwner) -> bool {
        usize::from(owner.key) == usize::from(FIRST_KEY) + part
    }

    fn outranks(&self, candidate: NoteCandidate, current: NoteCandidate) -> bool {
        (
            candidate.owner.key,
            candidate.strike.to_bits(),
            candidate.owner.note_id,
            candidate.owner.channel,
        ) > (
            current.owner.key,
            current.strike.to_bits(),
            current.owner.note_id,
            current.owner.channel,
        )
    }

    fn reduce(&self, accumulated: f32, next: f32) -> f32 {
        f32::max(accumulated, next)
    }

    fn sounding(&self, result: &ArbitrationResult) -> f32 {
        result.owner_strike
    }

    fn chromatic_pitch(&self, _part: usize, _owner: NoteOwner) -> f32 {
        0.0
    }
}

/// One sample's DSP events, in a fixed array: `process` allocates nothing.
struct SampleEvents {
    events: [Event; MAX_SAMPLE_EVENTS],
    len: usize,
}

impl SampleEvents {
    fn new() -> Self {
        Self {
            events: [Event {
                offset: 0,
                kind: EventKind::AllNotesOff,
            }; MAX_SAMPLE_EVENTS],
            len: 0,
        }
    }

    fn push(&mut self, kind: EventKind) {
        if let Some(slot) = self.events.get_mut(self.len) {
            *slot = Event { offset: 0, kind };
            self.len += 1;
        }
    }

    fn as_slice(&self) -> &[Event] {
        &self.events[..self.len]
    }
}

/// Who owns each key's note, which gates are open, and the expression the kit last heard.
struct Notes {
    parts: PartState<PARTS, NUM_CHANNELS>,
    /// Open while the owner holds its note. A note-on released at its own sample strikes with no
    /// gate, though ownership would hand it one.
    gates: [bool; PARTS],
    tunings: [f32; CELLS],
    wheel: f32,
    pressure: f32,
}

impl Notes {
    fn new() -> Self {
        Self {
            parts: PartState::new(),
            gates: [false; PARTS],
            tunings: [0.0; CELLS],
            wheel: 0.0,
            pressure: 0.0,
        }
    }

    /// One sample's group, resolved into the kit's events.
    fn resolve(&mut self, events: &EventGroup, out: &mut SampleEvents) {
        // A flood keeps only what no host order can change: a choke anywhere in it is panic;
        // otherwise every gate closes and no tuning in it applies.
        let overflow = events.overflowed();
        if events.panicked() || (overflow && events.any_choke()) {
            self.parts.panic();
            self.gates = [false; PARTS];
            self.tunings = [0.0; CELLS];
            out.push(EventKind::Panic);
            return;
        }
        let flags = ResolveFlags {
            release_all: events.all_notes_off_requested() || overflow,
            apply_tunings: !overflow,
        };
        let resolution = self.parts.resolve(events, &NoteLaws, flags);
        for part in 0..PARTS {
            let (cell, touch) = (part % CELLS, part >= CELLS);
            if resolution.chokes[part] {
                self.gates[part] = false;
                out.push(if touch {
                    EventKind::TouchOff { cell }
                } else {
                    EventKind::Choke { cell }
                });
            } else if self.gates[part] && !self.parts.owns_expression(part) {
                self.gates[part] = false;
                out.push(if touch {
                    EventKind::TouchOff { cell }
                } else {
                    EventKind::Release { cell }
                });
            }
            if let (Some(velocity), Some(owner)) =
                (resolution.strikes[part], self.parts.owner(part))
            {
                let released = events.events().any(
                    |event| matches!(event, PartEvent::Release(address) if address.matches(owner)),
                );
                self.gates[part] = !released;
                if touch {
                    // A touch let go at its own sample touches nothing.
                    if !released {
                        out.push(EventKind::TouchOn { cell, velocity });
                    }
                } else {
                    out.push(EventKind::Strike {
                        cell,
                        velocity,
                        gate: !released,
                    });
                    self.tunings[cell] = 0.0;
                }
            }
            if !touch && self.parts.tuning(part) != self.tunings[cell] {
                self.tunings[cell] = self.parts.tuning(part);
                out.push(EventKind::Tune {
                    cell,
                    semitones: self.tunings[cell],
                });
            }
        }
        // Wheel and pressure are the kit's: the highest any channel holds.
        let wheel = (0..NUM_CHANNELS)
            .map(|c| self.parts.wheel(c))
            .fold(0.0, f32::max);
        if wheel != self.wheel {
            self.wheel = wheel;
            out.push(EventKind::Wheel(wheel));
        }
        let pressure = (0..NUM_CHANNELS)
            .map(|c| self.parts.pressure(c))
            .fold(0.0, f32::max);
        if pressure != self.pressure {
            self.pressure = pressure;
            out.push(EventKind::Pressure(pressure));
        }
    }
}

pub struct MxmChaosDrums {
    params: Arc<MxmChaosDrumsParams>,
    telemetry: Arc<Telemetry>,
    /// Made at activation, which knows the sample rate: the cells' delay lines are allocated then.
    kit: Option<Box<Kit>>,
    frames: Vec<OutputFrame>,
    notes: Notes,
    multi_output: bool,
    /// Whether the host's transport was playing at the last callback: a start resets the kit when
    /// Reset on play is on.
    playing: bool,
}

impl Default for MxmChaosDrums {
    fn default() -> Self {
        Self {
            params: Arc::new(MxmChaosDrumsParams::default()),
            telemetry: Telemetry::shared(),
            kit: None,
            frames: Vec::new(),
            notes: Notes::new(),
            multi_output: false,
            playing: false,
        }
    }
}

impl MxmChaosDrums {
    fn cell_settings(&self) -> [mxm_chaos_drums_dsp::CellParams; CELLS] {
        std::array::from_fn(|cell| self.params.cells[cell].dsp(self.multi_output))
    }
}

impl Plugin for MxmChaosDrums {
    const NAME: &'static str = NAME;
    const VENDOR: &'static str = "mxm";
    const URL: &'static str = "https://mxm.dk";
    const EMAIL: &'static str = "plugins@mxm.dk";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    /// Full routing first, as the drum machine: a default-layout host sees every individual output;
    /// stereo only second, which MXM Player takes.
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(2),
            aux_output_ports: &INDIVIDUAL_OUTPUT_PORTS,
            names: PortNames {
                layout: Some("Main plus 16 mono outputs"),
                main_input: None,
                main_output: Some("L+R"),
                aux_inputs: &[],
                aux_outputs: &INDIVIDUAL_OUTPUT_NAMES,
            },
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(2),
            names: PortNames {
                layout: Some("Stereo compatibility"),
                main_input: None,
                main_output: Some("L+R"),
                aux_inputs: &[],
                aux_outputs: &[],
            },
            ..AudioIOLayout::const_default()
        },
    ];
    const MIDI_INPUT: MidiConfig = MidiConfig::MidiCCs;
    /// The performance is the same at any block size only if automation lands on its sample.
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type Editor = editor::MxmChaosDrumsEditor;
    type SysExMessage = ();
    type BackgroundTask = Task;

    /// Nothing runs on nice-plug's shared background thread: the one task only wakes the host.
    fn task_executor(&mut self) -> TaskExecutor<Self> {
        Box::new(|_task| {})
    }

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    /// The cell names a state holds, canonical before they are restored.
    fn filter_state(state: &mut PluginState) {
        mxm_drum_shell::slot_names::filter(&mut state.fields);
    }

    fn editor(&mut self, async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        let wake: editor::Wake = Arc::new(move || async_executor.execute_gui(Task::Wake));
        editor::create(self.params.clone(), self.telemetry.clone(), wake)
    }

    fn activate(
        &mut self,
        audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl ActivateContext<Self>,
    ) -> bool {
        let sample_rate = buffer_config.sample_rate;
        if !sample_rate.is_finite() || sample_rate < MIN_SAMPLE_RATE {
            return false;
        }
        self.multi_output = audio_io_layout.aux_output_ports.len() == CELLS;
        self.frames = vec![OutputFrame::default(); buffer_config.max_buffer_size as usize];
        // A new activation starts with no tempo: the first callback reports one.
        self.telemetry.tempo.publish(None);
        let kit = self.params.kit(self.params.lfo_hz(None));
        self.kit = Some(Box::new(Kit::new(sample_rate, kit, self.cell_settings())));
        self.notes = Notes::new();
        self.playing = false;
        true
    }

    fn reset(&mut self) {
        if let Some(kit) = &mut self.kit {
            kit.reset();
        }
        self.notes = Notes::new();
        // A cell the editor asked to hear before the reset is pending input, cleared with the rest.
        let _ = self.telemetry.take_audition();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let transport = context.transport();
        let tempo = mxm_tempo::tempo(transport.tempo);
        self.telemetry.tempo.publish(tempo);
        let restart = transport.playing && !self.playing && self.params.reset_on_play.value();
        self.playing = transport.playing;
        let cells = self.cell_settings();
        let Some(kit) = self.kit.as_mut() else {
            return ProcessStatus::Normal;
        };
        // Sample-accurate automation splits the buffer at every change, so a block's settings hold
        // for all of it.
        kit.set_params(self.params.kit(self.params.lfo_hz(tempo)));
        for (cell, settings) in cells.into_iter().enumerate() {
            kit.set_cell(cell, settings);
        }

        let samples = buffer.samples().min(self.frames.len());
        let frames = &mut self.frames[..samples];
        let mut next_event = context.next_event();
        // The cells the editor asked to hear, struck at the buffer's first sample as a note would be;
        // a callback of no frames leaves them for the next.
        let audition = if samples > 0 {
            self.telemetry.take_audition()
        } else {
            0
        };
        let mut start = 0;
        while start < samples {
            let mut sample = SampleEvents::new();
            if start == 0 && restart {
                sample.push(EventKind::TransportReset);
            }
            if start == 0 {
                for cell in (0..CELLS).filter(|cell| audition & (1 << cell) != 0) {
                    sample.push(EventKind::Strike {
                        cell,
                        velocity: AUDITION_VELOCITY,
                        gate: false,
                    });
                }
            }
            let mut group = EventGroup::new();
            let mut any = false;
            while let Some(event) = next_event {
                if event.timing() as usize > start {
                    break;
                }
                group.collect(event);
                any = true;
                next_event = context.next_event();
            }
            if any {
                self.notes.resolve(&group, &mut sample);
            }
            let end = next_event.map_or(samples, |event| {
                (event.timing() as usize).clamp(start + 1, samples)
            });
            kit.process(sample.as_slice(), &mut frames[start..end]);
            start = end;
        }

        let output = buffer.as_slice();
        let mut peak = 0.0f32;
        for (n, frame) in frames.iter().enumerate() {
            output[0][n] = frame.main[0];
            output[1][n] = frame.main[1];
            peak = peak.max(frame.main[0].abs()).max(frame.main[1].abs());
        }
        if self.multi_output {
            for (cell, port) in aux.outputs.iter_mut().enumerate().take(CELLS) {
                let channel = &mut port.as_slice()[0];
                for (n, frame) in frames.iter().enumerate() {
                    channel[n] = frame.individual[cell];
                    peak = peak.max(frame.individual[cell].abs());
                }
            }
        }
        self.telemetry.meter.publish(peak);
        let charges = std::array::from_fn(|cell| kit.charge(cell));
        let seconds = samples as f32 / kit.sample_rate();
        self.telemetry
            .publish(charges, kit.take_flows(), seconds, kit.pool_level());

        if kit.is_at_rest() {
            ProcessStatus::Normal
        } else {
            ProcessStatus::KeepAlive
        }
    }
}

impl ClapPlugin for MxmChaosDrums {
    const CLAP_ID: &'static str = CLAP_ID;
    const CLAP_DESCRIPTION: Option<&'static str> = Some(
        "A sixteen-cell drum instrument in which a note supplies energy instead of firing a hit",
    );
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Drum,
        ClapFeature::Stereo,
    ];

    /// Each cell's name on its cell key and its touch key (`slot_names::HostNames`).
    fn note_names(&self) -> Option<Arc<dyn ClapNoteNames>> {
        Some(Arc::new(slot_names::HostNames(self.params.clone())))
    }

    const CLAP_PRESETS: Option<ClapPresets> = Some(mxm_preset::clap::PRESETS);

    fn preset_loader(&self) -> Option<Arc<dyn ClapPresetLoader>> {
        Some(Arc::new(mxm_preset::clap::Loader(self.params.clone())))
    }
}

impl Vst3Plugin for MxmChaosDrums {
    /// Permanent, as `CLAP_ID` is: the CLAP id's hash written out, held to it by
    /// `identity::the_vst3_class_id_is_the_clap_ids_hash`.
    const VST3_CLASS_ID: [u8; 16] = [
        0xf5, 0xa4, 0xb2, 0x2d, 0x10, 0x56, 0x9c, 0x75, 0x63, 0xb5, 0x54, 0x99, 0x01, 0xdc, 0x0c,
        0xdd,
    ];
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Instrument,
        Vst3SubCategory::Drum,
        Vst3SubCategory::Stereo,
    ];
}

nice_export_clap!(MxmChaosDrums);
nice_export_vst3!(MxmChaosDrums);

#[cfg(test)]
mod identity {
    use super::{CLAP_ID, NAME};
    use mxm_plugin_test::formats;

    #[test]
    fn the_vst3_class_id_is_the_clap_ids_hash() {
        formats::the_vst3_class_id_is_the_clap_ids_hash::<super::MxmChaosDrums>();
    }

    #[test]
    fn the_vst3_categories_follow_the_clap_features() {
        formats::the_vst3_categories_follow_the_clap_features::<super::MxmChaosDrums>();
    }

    #[test]
    fn clap_id_and_bundle_name_follow_the_single_plugin_literal() {
        assert_eq!(CLAP_ID, format!("dk.mxm.{NAME}"));
        mxm_plugin_test::bundle::is_named(env!("CARGO_MANIFEST_DIR"), env!("CARGO_PKG_NAME"), NAME);
    }
}

#[cfg(test)]
mod notes {
    use super::*;

    fn note(on: bool, channel: u8, key: u8, id: Option<i32>, velocity: f32) -> NoteEvent<()> {
        let voice_id = id.map_or(VoiceID::Wildcard, VoiceID::ID);
        let (channel, key) = (Channel::Number(channel), Key::Number(key));
        if on {
            NoteEvent::NoteOn {
                timing: 0,
                voice_id,
                channel,
                key,
                velocity,
            }
        } else {
            NoteEvent::NoteOff {
                timing: 0,
                voice_id,
                channel,
                key,
                velocity: 0.0,
            }
        }
    }

    fn choke(channel: u8, key: u8, id: Option<i32>) -> NoteEvent<()> {
        NoteEvent::Choke {
            timing: 0,
            voice_id: id.map_or(VoiceID::Wildcard, VoiceID::ID),
            channel: Channel::Number(channel),
            key: Key::Number(key),
        }
    }

    /// The kit's events for one sample's host events.
    fn resolve(notes: &mut Notes, events: &[NoteEvent<()>]) -> Vec<EventKind> {
        let mut group = EventGroup::new();
        for event in events {
            group.collect(*event);
        }
        let mut out = SampleEvents::new();
        notes.resolve(&group, &mut out);
        out.as_slice().iter().map(|event| event.kind).collect()
    }

    #[test]
    fn a_held_cell_key_keeps_its_gate_until_its_release() {
        let mut notes = Notes::new();
        let on = resolve(&mut notes, &[note(true, 0, 36, Some(1), 0.5)]);
        assert_eq!(
            on,
            [EventKind::Strike {
                cell: 0,
                velocity: 0.5,
                gate: true
            }]
        );
        assert_eq!(
            resolve(&mut notes, &[note(false, 0, 36, Some(1), 0.0)]),
            [EventKind::Release { cell: 0 }]
        );
    }

    #[test]
    fn a_note_released_at_its_own_sample_strikes_with_no_gate() {
        let mut notes = Notes::new();
        let events = [
            note(true, 0, 38, Some(4), 0.9),
            note(false, 0, 38, Some(4), 0.0),
        ];
        assert_eq!(
            resolve(&mut notes, &events),
            [EventKind::Strike {
                cell: 2,
                velocity: 0.9,
                gate: false
            }]
        );
        assert_eq!(resolve(&mut notes, &events[1..]), []);
    }

    #[test]
    fn a_touch_key_touches_its_cell_and_its_release_lets_go() {
        let mut notes = Notes::new();
        assert_eq!(
            resolve(&mut notes, &[note(true, 3, 52 + 5, None, 0.7)]),
            [EventKind::TouchOn {
                cell: 5,
                velocity: 0.7
            }]
        );
        assert_eq!(
            resolve(&mut notes, &[note(false, 3, 52 + 5, None, 0.0)]),
            [EventKind::TouchOff { cell: 5 }]
        );
        let both = [note(true, 0, 52, None, 0.7), note(false, 0, 52, None, 0.0)];
        assert_eq!(
            resolve(&mut notes, &both),
            [],
            "a touch let go at its own sample touches nothing"
        );
    }

    #[test]
    fn a_choke_finds_a_released_notes_tail_and_a_late_release_is_ignored() {
        let mut notes = Notes::new();
        resolve(&mut notes, &[note(true, 0, 40, Some(1), 1.0)]);
        resolve(&mut notes, &[note(false, 0, 40, Some(1), 0.0)]);
        assert_eq!(
            resolve(&mut notes, &[choke(0, 40, Some(1))]),
            [EventKind::Choke { cell: 4 }]
        );

        let mut notes = Notes::new();
        resolve(&mut notes, &[note(true, 0, 41, Some(1), 1.0)]);
        resolve(&mut notes, &[note(true, 0, 41, Some(2), 0.6)]);
        assert_eq!(
            resolve(&mut notes, &[note(false, 0, 41, Some(1), 0.0)]),
            [],
            "the older press's release"
        );
        assert_eq!(
            resolve(&mut notes, &[note(false, 0, 41, Some(2), 0.0)]),
            [EventKind::Release { cell: 5 }]
        );
    }

    #[test]
    fn all_sound_off_is_panic_and_same_sample_events_resolve_in_any_order() {
        let mut notes = Notes::new();
        let panic = NoteEvent::MidiCC {
            timing: 0,
            channel: 0,
            cc: control_change::ALL_SOUND_OFF,
            value: 1.0,
        };
        assert_eq!(
            resolve(&mut notes, &[note(true, 0, 36, None, 1.0), panic]),
            [EventKind::Panic]
        );

        let events = [
            note(true, 0, 37, Some(1), 0.4),
            note(true, 1, 37, Some(2), 0.8),
            note(true, 0, 60, None, 0.5),
        ];
        let forward = resolve(&mut Notes::new(), &events);
        let mut reversed = events;
        reversed.reverse();
        assert_eq!(resolve(&mut Notes::new(), &reversed), forward);
        assert_eq!(
            forward,
            [
                EventKind::Strike {
                    cell: 1,
                    velocity: 0.8,
                    gate: true
                },
                EventKind::TouchOn {
                    cell: 8,
                    velocity: 0.5
                }
            ]
        );
    }
}
