//! Generate the catalog coverage used by the integration audit, without requiring a GPU.
use op_application::render_engine::{
    fframes_blend_name, fframes_component_supported, fframes_transition_supported,
};
use op_core::catalog::{self, EffectKind};

fn main() {
    let entries: Vec<_> = catalog::CATALOG.iter().filter(|e| e.kind.is_video()).map(|e| {
        let supported = if e.kind == EffectKind::VideoTransition {
            fframes_transition_supported(e.id)
        } else { fframes_component_supported(e.id) };
        serde_json::json!({"id":e.id,"name":e.name,"kind":format!("{:?}",e.kind),"supported":supported,"hybrid_supported":true,"implementation":if supported {"independent SVG subset or shared preparation"} else {"shared native preparation; Skia floating composition"}})
    }).collect();
    let blends: Vec<_> = catalog::BLEND_MODES.iter().enumerate().map(|(i,name)| {
        serde_json::json!({"id":i,"name":name,"supported":fframes_blend_name(i as u32).is_some(),"hybrid_supported":true})
    }).collect();
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"fframes":"1.2.0","skia":"0.153.3","scope":"Independent SVG subset and explicit RGBA16F hybrid compatibility for CPU/GPU exports and async previews; hybrid support reuses native effects, not SkSL ports","video_catalog":entries,"blend_modes":blends})).unwrap());
}
