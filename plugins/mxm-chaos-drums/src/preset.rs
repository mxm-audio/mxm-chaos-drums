//! The collection's presets (`mxm-preset`): Init is the parameter defaults, factory kits are files
//! installed for the plugin's version (`presets/`, the fifty `kits` designs and writes), user
//! presets live where every MXM instrument keeps them.

use std::sync::RwLock;

use mxm_chaos_drums_dsp::CELLS;
use mxm_preset::{Instrument, PresetIdentity};

use crate::params::{KIT_IDS, MxmChaosDrumsParams, cell_ids, route_of};

pub use mxm_preset::Library;

impl Instrument for MxmChaosDrumsParams {
    fn clap_id(&self) -> &'static str {
        crate::CLAP_ID
    }

    /// The plugin's version, its `Plugin::VERSION`: its factory presets are installed for it.
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn parameters(&self) -> Vec<(&'static str, &dyn mxm_params::ErasedParam)> {
        let mut out = Vec::new();
        for (cell, params) in self.cells.iter().enumerate() {
            out.extend(cell_ids(cell).iter().copied().zip(params.parameters()));
        }
        out.extend(KIT_IDS.iter().copied().zip(self.globals()));
        out
    }

    fn identity(&self) -> &RwLock<PresetIdentity> {
        &self.preset
    }

    /// The installed set, but in a unit test the repository's, never the machine's.
    #[cfg(test)]
    fn factory_files(&self) -> Vec<(String, String)> {
        mxm_preset::read_factory_dir(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("presets"),
        )
    }

    /// Where a cell is heard, and Resample, are the instance's, as in the drum machine; no kit moves
    /// them.
    fn is_instance_setting(&self, id: &str) -> bool {
        id.starts_with("output_") || id == "resample"
    }

    /// A kit load or Init: the drums changed, so every cell goes back to the name it is given.
    fn patch_replaced(&self) {
        mxm_drum_shell::slot_names::clear(self);
    }

    // Routes are stored sparsely, as the drum machine's: a route not in use stores nothing, and
    // resolving an omission writes the default, so loading a kit clears unrelated live routes.

    fn default_missing_legacy_parameter(&self, id: &str) -> bool {
        route_of(id).is_some()
    }

    fn omit_default_parameter(&self, id: &str) -> bool {
        route_of(id).is_some()
    }

    fn omit_captured_parameter(&self, id: &str) -> bool {
        route_of(id).is_some_and(|(cell, r)| {
            if cell == CELLS {
                !self.kit_routes.all()[r].in_use()
            } else {
                !self.cells[cell].routes.all()[r].in_use()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_host_parameter_is_declared_once_for_presets() {
        let params = MxmChaosDrumsParams::default();
        let ids: Vec<&str> = params.parameters().iter().map(|(id, _)| *id).collect();
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ids.len());
        let host: Vec<String> = nice_plug::params::Params::param_map(&params)
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();
        assert_eq!(
            host.len(),
            ids.len(),
            "every host parameter, and no other, is a preset's"
        );
        for id in &host {
            assert!(ids.contains(&id.as_str()), "{id} is not in the preset list");
        }
    }
}
