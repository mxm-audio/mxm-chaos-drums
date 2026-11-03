//! Each cell's name, typed or made up from its model's kind (`bd1`, `tm2`), and the names the host
//! shows on every key: a cell's on its cell key, and `bd1 touch` on its touch key. The names and
//! their storage are mxm-drum-shell's (`mxm_drum_shell::slot_names`); this plugin plays every key on
//! every channel, so it names each one channel-wide.

use std::sync::RwLock;
use std::sync::atomic::AtomicBool;

use mxm_chaos_drums_dsp::{CELLS, ModelId, available};
use mxm_drum_shell::slot_names::{self as names, DrumSlots};
use nice_plug::prelude::*;

use crate::params::MxmChaosDrumsParams;
use crate::{FIRST_KEY, FIRST_TOUCH_KEY};

/// The names each kind's cells take, in cell order over the models alone: `bd1`, `tm1`, `tm2`.
/// A cell whose model is Off or not built yet makes none and takes no number.
#[must_use]
pub fn made_up_names(models: &[ModelId; CELLS]) -> [Option<String>; CELLS] {
    let mut seen: Vec<(&'static str, usize)> = Vec::new();
    models.map(|id| {
        let code = available(id)?.kind.code();
        let number = match seen.iter_mut().find(|(each, _)| *each == code) {
            Some((_, count)) => {
                *count += 1;
                *count
            }
            None => {
                seen.push((code, 1));
                1
            }
        };
        Some(format!("{code}{number}"))
    })
}

static NAME_DEPENDENCIES: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(|| {
    (0..CELLS)
        .map(|cell| crate::params::cell_ids(cell)[0])
        .collect()
});

impl DrumSlots for MxmChaosDrumsParams {
    const FIRST_KEY: u8 = FIRST_KEY;

    fn typed_names(&self) -> &RwLock<Vec<String>> {
        &self.slot_names
    }

    fn names_written(&self) -> &AtomicBool {
        &self.names_written
    }

    fn made_up_names(&self) -> [Option<String>; CELLS] {
        made_up_names(&std::array::from_fn(|cell| self.cells[cell].model_id()))
    }

    fn plays(&self, slot: usize) -> bool {
        available(self.cells[slot].model_id()).is_some()
    }

    /// Every cell is a Kit cell: every channel plays the kit.
    fn midi_channel(&self, _slot: usize) -> i32 {
        0
    }

    fn name_dependencies(&self) -> Vec<&'static str> {
        NAME_DEPENDENCIES.clone()
    }
}

/// The names the host is told, through the nice-plug fork's note-name capability: every cell whose
/// model plays, on its cell key and its touch key, on any channel.
pub struct HostNames(pub std::sync::Arc<MxmChaosDrumsParams>);

impl ClapNoteNames for HostNames {
    fn names(&self) -> Vec<NoteName> {
        let params = &*self.0;
        let mut out = Vec::new();
        for (cell, name) in names::shown(params).into_iter().enumerate() {
            let Some(name) = name.filter(|_| params.plays(cell)) else {
                continue;
            };
            out.push(NoteName {
                name: format!("{name} touch"),
                port: 0,
                key: i16::from(FIRST_TOUCH_KEY) + cell as i16,
                channel: -1,
            });
            out.push(NoteName {
                name,
                port: 0,
                key: i16::from(FIRST_KEY) + cell as i16,
                channel: -1,
            });
        }
        out
    }

    fn depends_on(&self) -> Vec<&'static str> {
        self.0.name_dependencies()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_names_each_kind_in_cell_order_and_its_touch_key() {
        let params = std::sync::Arc::new(MxmChaosDrumsParams::default());
        let made = params.made_up_names();
        assert_eq!(made[0].as_deref(), Some("bd1"));
        assert_eq!(made[1].as_deref(), Some("sn1"));
        assert_eq!(made[2].as_deref(), Some("tm1"));
        assert_eq!(made[4].as_deref(), Some("tm3"));
        assert_eq!(made[5].as_deref(), Some("cg1"));
        assert_eq!(made[7].as_deref(), Some("bg1"));
        let names = HostNames(params).names();
        assert!(names.iter().any(|n| n.key == 36 && n.name == "bd1"));
        assert!(names.iter().any(|n| n.key == 52 && n.name == "bd1 touch"));
        assert_eq!(names.len(), 2 * CELLS);
    }
}
