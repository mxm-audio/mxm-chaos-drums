//! Resample's capture: the one part of it mxm-kit's `mxm-drum-shell` cannot know, what this kit
//! renders and from which settings. The capture thread, the bank, the kit each engage heard, the
//! state machine and the pack's writer are the shell's; the capture itself and the players a frozen
//! kit strikes are the DSP crate's `capture`.

use std::path::Path;
use std::sync::Arc;

use mxm_chaos_drums_dsp::capture::{
    KitCapture, KitCaptureInProgress, capture_kit_until, capture_patches,
};
use mxm_chaos_drums_dsp::{CELLS, CellParams, Kit, KitParams};
use mxm_drum_shell::capture::Capturer;
use mxm_drum_shell::pack::Written;

use crate::params::MxmChaosDrumsParams;

/// The tempo a synced LFO runs at in a capture: fixed, as the drum machine's, so a capture is a
/// function of the patch alone and a project's tempo moving re-renders nothing.
pub const CAPTURE_TEMPO: f64 = 120.0;

/// What a capture renders: the kit's settings and every cell's, the mix neutral and the buses off.
pub type Patches = (KitParams, [CellParams; CELLS]);

/// The patch every capture renders from, read unsmoothed: nice-plug smooths none of these.
#[must_use]
pub fn patches(params: &MxmChaosDrumsParams) -> Patches {
    let kit = params.kit(params.lfo_hz(Some(CAPTURE_TEMPO)));
    let cells = std::array::from_fn(|cell| params.cells[cell].dsp(true));
    capture_patches(kit, cells)
}

/// What to call an exported pack: the loaded kit's name, or a plain default.
#[must_use]
pub fn kit_name(params: &MxmChaosDrumsParams) -> String {
    let identity = params
        .preset
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match identity.loaded.as_ref() {
        Some(loaded) if !loaded.name.trim().is_empty() => loaded.name.trim().to_owned(),
        _ => "chaos kit".to_owned(),
    }
}

/// This kit's capture, reading its parameters.
pub struct Capture(pub Arc<MxmChaosDrumsParams>);

impl Capturer for Capture {
    const THREAD_NAME: &'static str = "mxm-chaos-drums-capture";
    type Patches = Patches;
    type Kit = KitCapture;
    type Progress = KitCaptureInProgress;
    type Engine = Kit;

    fn read(&self) -> (Self::Patches, String) {
        (patches(&self.0), kit_name(&self.0))
    }

    /// The patches are read with the mix already neutral, so equal patches are the same capture.
    fn same_capture(was: &Self::Patches, now: &Self::Patches) -> bool {
        was == now
    }

    fn begin(
        &self,
        patches: &Self::Patches,
        sample_rate: f32,
        _abandon: &dyn Fn() -> bool,
    ) -> Option<Self::Progress> {
        Some(KitCaptureInProgress::begin(patches, sample_rate))
    }

    fn render(progress: &mut Self::Progress, frames: usize) -> Option<bool> {
        Some(progress.render(frames))
    }

    fn finish(progress: Self::Progress, engagement: u32) -> Self::Kit {
        progress.finish().answering(engagement)
    }

    /// Nothing a capture holds stays live: the mix reaches a frozen cell through its settings, which
    /// a capture never held.
    fn live(heard: &Self::Patches, _now: &Self::Patches) -> Self::Patches {
        *heard
    }

    fn write_pack(
        &self,
        root: &Path,
        name: &str,
        captured: &Self::Patches,
        _played: &Self::Patches,
        sample_rate: f32,
        stop: &dyn Fn() -> bool,
    ) -> Result<Written, String> {
        let kit = capture_kit_until(captured, sample_rate, stop)
            .ok_or_else(mxm_drum_shell::capture::cancelled)?;
        crate::pack::write_until(root, name, &kit, captured, stop)
            .map_err(|error| error.to_string())
    }

    fn install(engine: &mut Self::Engine, kit: Option<Box<Self::Kit>>) -> Option<Box<Self::Kit>> {
        engine.set_captures(kit)
    }

    fn is_frozen(engine: &Self::Engine) -> bool {
        engine.is_frozen()
    }

    fn kit_engagement(kit: &Self::Kit) -> u32 {
        kit.request()
    }

    fn kit_rate(kit: &Self::Kit) -> f32 {
        kit.sample_rate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nice_plug::params::InternalParamMut;

    /// **Engaged at activation, the kit is frozen on its own capture** through the shell's state
    /// machine: a strike plays the cell's one-shot, and turning Resample off is immediate.
    #[test]
    fn engaged_at_activation_a_strike_plays_the_cells_capture() {
        use mxm_chaos_drums_dsp::{Event, EventKind};
        const RATE: f32 = 16_000.0;
        let params = Arc::new(MxmChaosDrumsParams::default());
        // The kick alone, so a debug build captures it quickly.
        for cell in &params.cells[1..] {
            // SAFETY: exclusive test ownership; no process or GUI thread exists.
            unsafe { cell.model._internal_set_plain_value(0) };
        }
        let telemetry = crate::telemetry::Telemetry::shared();
        let mut resample = mxm_drum_shell::capture::Resample::new(Capture(Arc::clone(&params)));
        let cells = std::array::from_fn(|cell| params.cells[cell].dsp(false));
        let mut kit = Kit::new(RATE, params.kit(params.lfo_hz(None)), cells);
        resample.activate(&mut kit, true, &telemetry, RATE);
        assert!(
            kit.is_frozen(),
            "a session that opens frozen is frozen at once"
        );

        let capture = mxm_chaos_drums_dsp::capture::capture_kit(&patches(&params), RATE);
        let shot = capture.cell(0);
        assert!(!shot.is_empty());
        let mut out = vec![[0.0; CELLS]; shot.len()];
        let hit = [Event {
            offset: 0,
            kind: EventKind::Strike {
                cell: 0,
                velocity: 1.0,
                gate: false,
            },
        }];
        kit.process_raw(&hit, &mut out);
        for (n, frame) in out.iter().enumerate() {
            assert_eq!(frame[0], shot[n], "at {n}");
        }

        resample.service(&mut kit, false, &telemetry.signals, RATE);
        assert!(!kit.is_frozen(), "off at once");
    }

    /// A capture is the same kit when only the mix moved, and a new one when a cell's sound did.
    #[test]
    fn the_mix_is_not_a_new_capture_and_a_sound_is() {
        let params = MxmChaosDrumsParams::default();
        let heard = patches(&params);
        // SAFETY: exclusive test ownership; no process or GUI thread exists.
        unsafe {
            params.cells[0].level._internal_set_plain_value(0.1);
            params.cells[0].pan._internal_set_plain_value(-1.0);
            params.master._internal_set_plain_value(0.2);
            params.bus_a._internal_set_plain_value(0.7);
        }
        assert!(Capture::same_capture(&heard, &patches(&params)));
        // SAFETY: as above.
        unsafe { params.cells[0].coarse._internal_set_plain_value(5) };
        assert!(!Capture::same_capture(&heard, &patches(&params)));
    }
}
