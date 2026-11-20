//! Which outputs the cells are routed to, for the host: mxm-kit's `mxm_drum_shell::outputs` (the
//! MXM CLAP extension `dk.mxm.outputs-in-use/1` and VST3 interface `IOutputsInUse`), read from each
//! cell's Output, so a host can give a cell routed to its own output a track of its own (the owner,
//! 2026-11-19: "if the drum machine opens an individual out, then it makes a group in the daw").

pub use mxm_drum_shell::outputs::*;

use crate::params::{MxmChaosDrumsParams, at, cell_ids};

impl DrumOutputs for MxmChaosDrumsParams {
    fn output(&self, slot: usize) -> i32 {
        self.cells[slot].output.value()
    }

    fn output_id(&self, slot: usize) -> &'static str {
        cell_ids(slot)[at::OUTPUT]
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nice_plug::params::InternalParamMut;
    use nice_plug::prelude::{OutputsInUse, Plugin};

    use super::*;
    use crate::MxmChaosDrums;
    use mxm_chaos_drums_dsp::CELLS as SLOT_COUNT;
    use mxm_drum_shell::slot_names::shown;

    fn route(params: &MxmChaosDrumsParams, slot: usize, output: i32) {
        // SAFETY: a test's own parameters, nothing else reading them.
        unsafe { params.cells[slot].output._internal_set_plain_value(output) };
    }

    fn told(params: Arc<MxmChaosDrumsParams>) -> Vec<(u32, String)> {
        HostOutputs(params)
            .outputs()
            .into_iter()
            .map(|output| (output.output, output.name))
            .collect()
    }

    #[test]
    fn the_init_kit_on_main_uses_no_output() {
        assert_eq!(told(Arc::new(MxmChaosDrumsParams::default())), []);
    }

    #[test]
    fn own_outputs_name_each_playing_cell_on_its_port() {
        let params = Arc::new(MxmChaosDrumsParams::default());
        for slot in 0..SLOT_COUNT {
            route(&params, slot, slot as i32 + 1);
        }
        let expected: Vec<(u32, String)> = shown(&*params)
            .into_iter()
            .enumerate()
            .filter_map(|(slot, name)| Some((aux_output(slot), name?)))
            .collect();
        assert!(!expected.is_empty());
        assert_eq!(told(params), expected);
    }

    #[test]
    fn two_cells_on_one_output_are_named_together() {
        let params = Arc::new(MxmChaosDrumsParams::default());
        route(&params, 0, 3);
        route(&params, 1, 3);
        let shown = shown(&*params);
        let together = format!(
            "{} / {}",
            shown[0].clone().unwrap(),
            shown[1].clone().unwrap()
        );
        assert_eq!(told(params), [(3, together)]);
    }

    /// `aux_output` counts on no audio input, a main output and the sixteen in cell order.
    #[test]
    fn the_full_configuration_numbers_its_outputs_as_aux_output_does() {
        let full = &<MxmChaosDrums as Plugin>::AUDIO_IO_LAYOUTS[0];
        assert!(full.main_input_channels.is_none() && full.aux_input_ports.is_empty());
        assert!(full.main_output_channels.is_some());
        assert_eq!(full.aux_output_ports.len(), SLOT_COUNT);
    }

    #[test]
    fn the_outputs_follow_every_cells_output() {
        let ids = HostOutputs(Arc::new(MxmChaosDrumsParams::default())).depends_on();
        assert!(
            ids.contains(&"output_1") && ids.contains(&"output_16"),
            "{ids:?}"
        );
    }
}

/// The real VST3 wrapper, as a host reads it: `IOutputsInUse` on the component, and each output
/// the bus the full arrangement names after its cell.
#[cfg(test)]
mod through_the_vst3_wrapper {
    use nice_plug::wrapper::vst3::Wrapper;
    use nice_plug::wrapper::vst3::outputs_in_use::IOutputsInUse;
    use nice_plug::wrapper::vst3::vst3::ComWrapper;
    use nice_plug::wrapper::vst3::vst3::Steinberg::Vst::BusDirections_::kOutput;
    use nice_plug::wrapper::vst3::vst3::Steinberg::Vst::MediaTypes_::kAudio;
    use nice_plug::wrapper::vst3::vst3::Steinberg::Vst::{
        BusDirection, BusInfo, IComponentTrait, MediaType,
    };
    use nice_plug::wrapper::vst3::vst3::Steinberg::kResultOk;

    use super::aux_output;
    use crate::MxmChaosDrums;
    use mxm_chaos_drums_dsp::CELLS as SLOT_COUNT;

    #[test]
    fn it_says_which_outputs_and_each_is_the_bus_named_after_its_cell() {
        let wrapper = ComWrapper::new(Wrapper::<MxmChaosDrums>::new());
        let in_use = wrapper.to_com_ptr::<IOutputsInUse>().expect("offered");
        let ptr = in_use.as_ptr();
        let mut count = -1;
        assert_eq!(
            unsafe { ((*(*ptr).vtbl).getOutputCount)(ptr, &mut count) },
            kResultOk,
            "it says"
        );
        assert_eq!(count, 0, "the Init kit on main");
        for aux in 0..SLOT_COUNT {
            let mut info: BusInfo = unsafe { std::mem::zeroed() };
            let bus = aux_output(aux) as i32;
            assert_eq!(
                unsafe {
                    wrapper.getBusInfo(kAudio as MediaType, kOutput as BusDirection, bus, &mut info)
                },
                kResultOk
            );
            let end = info.name.iter().position(|&unit| unit == 0).unwrap();
            let name = String::from_utf16(&info.name[..end]).unwrap();
            assert_eq!(name, format!("Cell {:02}", aux + 1));
        }
    }
}
