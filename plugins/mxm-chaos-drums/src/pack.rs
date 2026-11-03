//! The sample pack: each cell's one-shot as its frozen cell plays it at full strength, and what a
//! recording of this instrument loses. The writer, the manifest, the naming and the
//! never-overwriting are mxm-kit's `mxm_drum_shell::pack`.
//!
//! A frozen cell is heard as its capture, then Level, Mute, Solo, Pan, its output and Master; the
//! pack takes the capture alone, so a file is at unity gain and centred and moving a fader cannot
//! change an exported byte.

use std::path::Path;

use mxm_chaos_drums_dsp::capture::KitCapture;
use mxm_chaos_drums_dsp::{CELLS, ModelId, available};
use mxm_drum_shell::pack::{OneShots, render_one_shot};
pub use mxm_drum_shell::pack::{PackError, Written, root};

use crate::capture::Patches;

/// What a recording of this instrument loses: the manifest's last paragraph.
const LOSSES: &str = "Each one is a single hit from rest:\n\
     what a cell does with its history is not in it, the charge a hit finds, the neighbours that feed \
     it, its bus and its touch;\n\
     velocity scales the file rather than re-entering the cell, and its jitter is fixed where its \
     Seed starts it, so a file repeats exactly where the instrument would not.\n";

/// A captured kit as a pack reads it.
struct Shots<'a> {
    capture: &'a KitCapture,
    models: [ModelId; CELLS],
}

impl OneShots for Shots<'_> {
    fn sample_rate(&self) -> f32 {
        self.capture.sample_rate()
    }

    fn is_empty(&self, slot: usize) -> bool {
        self.capture.cell(slot).is_empty()
    }

    fn one_shot_until(
        &self,
        slot: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<f32>, PackError> {
        let shot = self.capture.cell(slot);
        let mut samples = shot.iter().copied();
        render_one_shot(slot, shot.len(), cancelled, || samples.next())
    }

    fn label(&self, slot: usize) -> &str {
        available(self.models[slot]).map_or("none", |spec| spec.label)
    }

    fn note(&self, slot: usize) -> Option<u8> {
        Some(crate::FIRST_KEY + slot as u8)
    }

    fn product(&self) -> &str {
        "mxm-chaos-drums"
    }

    fn losses(&self) -> &str {
        LOSSES
    }
}

/// Writes `capture`, the kit `patches` made, as a pack named `kit` under `root`, stopping between
/// files if `cancelled` says to.
///
/// # Errors
///
/// The shell's [`PackError`]s: no destination, nothing to export, an encoding or file-system
/// failure, or a stop.
pub fn write_until(
    root: &Path,
    kit: &str,
    capture: &KitCapture,
    patches: &Patches,
    cancelled: &dyn Fn() -> bool,
) -> Result<Written, PackError> {
    let shots = Shots {
        capture,
        models: patches.1.map(|cell| cell.model),
    };
    mxm_drum_shell::pack::write_until(root, kit, &shots, cancelled)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::params::MxmChaosDrumsParams;

    /// A folder for a test's packs under the build's own, never the real pack root.
    pub(crate) fn scratch(name: &str) -> std::path::PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-packs")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        dir
    }

    /// Init's pack: a file for every cell that sounds, each its capture to the byte's worth.
    #[test]
    fn a_pack_holds_every_cells_one_shot() {
        let params = MxmChaosDrumsParams::default();
        let patches = crate::capture::patches(&params);
        let capture = mxm_chaos_drums_dsp::capture::capture_kit(&patches, 16_000.0);
        let root = scratch("init");
        let written = write_until(&root, "Init", &capture, &patches, &|| false).expect("a pack");
        let pack = written.directory.clone();
        let wavs = std::fs::read_dir(&pack)
            .expect("the pack")
            .filter(|entry| {
                entry
                    .as_ref()
                    .is_ok_and(|e| e.path().extension().is_some_and(|x| x == "wav"))
            })
            .count();
        let sounding = (0..CELLS)
            .filter(|&cell| !capture.cell(cell).is_empty())
            .count();
        assert_eq!(wavs, sounding, "{written:?}");
        let _ = std::fs::remove_dir_all(root);
    }
}
