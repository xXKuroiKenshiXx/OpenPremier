//! Generates an original project and renderer snapshots for visual review of native effects.
use op_application::{Dirs, Editor, Preferences};
use op_core::catalog;
use op_core::*;
use std::path::PathBuf;

fn set(e: &mut Editor, sid: SequenceId, clip: ClipId, effect: &str, key: &str, value: Value) {
    let mut ids = e.project.ids.clone();
    let c = e.project.sequence_mut(sid).unwrap().clip_mut(clip).unwrap();
    if c.component(effect).is_none() {
        c.components
            .push(Component::new(catalog::find(effect).unwrap(), &mut ids));
    }
    c.component_mut(effect)
        .unwrap()
        .param_mut(key)
        .unwrap()
        .value = value;
    e.project.ids = ids;
}
fn animate(
    e: &mut Editor,
    sid: SequenceId,
    clip: ClipId,
    effect: &str,
    key: &str,
    a: Value,
    b: Value,
) {
    set(e, sid, clip, effect, key, a.clone());
    let c = e.project.sequence_mut(sid).unwrap().clip_mut(clip).unwrap();
    let p = c.component_mut(effect).unwrap().param_mut(key).unwrap();
    p.animated = true;
    p.keys = vec![
        Keyframe::new(SrcTime::ZERO, a, Interp::Bezier),
        Keyframe::new(SrcTime::from_seconds(4.0), b, Interp::Bezier),
    ];
}
fn matte(e: &mut Editor, name: &str, color: Rgba) -> ItemId {
    e.project.add_item(
        e.project.root,
        name,
        ItemKind::Synthetic {
            generator: Generator::ColorMatte { color },
            duration: Dur::from_seconds(5.0),
        },
    )
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("output folder required")?;
    std::fs::create_dir_all(&output)?;
    let mut e = Editor::new(
        Dirs::portable(&output.join("runtime")),
        Preferences::default(),
        false,
    );
    e.new_sequence(
        "Parallax y diálogo",
        SequenceSettings {
            width: 960,
            height: 540,
            ..SequenceSettings::default()
        },
    );
    let sid = e.active.unwrap();
    let item = matte(&mut e, "Fondo", Rgba::new(0.10, 0.12, 0.18, 1.0));
    e.place_on_top(item).unwrap();
    let text = e.add_graphic(catalog::TEXT, None).unwrap();
    set(
        &mut e,
        sid,
        text,
        catalog::TEXT,
        "text",
        Value::Text("OPEN PREMIERE\nPARALLAX".into()),
    );
    set(
        &mut e,
        sid,
        text,
        catalog::TEXT,
        "font_size",
        Value::Float(76.0),
    );
    animate(
        &mut e,
        sid,
        text,
        "op.video.parallax",
        "camera",
        Value::Point([0.47, 0.5]),
        Value::Point([0.53, 0.5]),
    );
    set(
        &mut e,
        sid,
        text,
        "op.video.parallax",
        "strength",
        Value::Float(0.0),
    );
    set(
        &mut e,
        sid,
        text,
        "op.video.parallax",
        "depth",
        Value::Float(0.4),
    );
    let subject = e.add_graphic(catalog::SHAPE, Some(1)).unwrap();
    set(
        &mut e,
        sid,
        subject,
        catalog::SHAPE,
        "width",
        Value::Float(200.0),
    );
    set(
        &mut e,
        sid,
        subject,
        catalog::SHAPE,
        "height",
        Value::Float(380.0),
    );
    set(
        &mut e,
        sid,
        subject,
        catalog::SHAPE,
        "fill",
        Value::Color(Rgba::new(0.92, 0.23, 0.28, 1.0)),
    );
    animate(
        &mut e,
        sid,
        subject,
        "op.video.parallax",
        "camera",
        Value::Point([0.44, 0.5]),
        Value::Point([0.56, 0.5]),
    );
    set(
        &mut e,
        sid,
        subject,
        "op.video.parallax",
        "strength",
        Value::Float(0.0),
    );
    set(
        &mut e,
        sid,
        subject,
        "op.video.parallax",
        "depth",
        Value::Float(2.0),
    );
    let gpu = op_render::Gpu::headless()?;
    for (name, time) in [("parallax-start", 0.0), ("parallax-end", 4.0)] {
        op_application::export::export_frame(
            &e.project,
            sid,
            SeqTime::from_seconds(time),
            &output.join(format!("{name}.png")),
            gpu.clone(),
            e.media.clone(),
        )?;
    }
    e.new_sequence(
        "Página",
        SequenceSettings {
            width: 960,
            height: 540,
            ..SequenceSettings::default()
        },
    );
    let pages = e.active.unwrap();
    let a = matte(&mut e, "Página A", Rgba::new(0.88, 0.86, 0.79, 1.0));
    let b = matte(&mut e, "Página B", Rgba::new(0.20, 0.44, 0.58, 1.0));
    e.place_item(a, Some(SeqTime::ZERO), Some(TrackRef::video(0)))
        .unwrap();
    e.place_item(
        b,
        Some(SeqTime::from_seconds(5.0)),
        Some(TrackRef::video(0)),
    )
    .unwrap();
    op_timeline::add_transition(
        &mut e.project,
        pages,
        TrackRef::video(0),
        SeqTime::from_seconds(5.0),
        "op.tr.page_turn",
        Dur::from_seconds(2.0),
        None,
        op_timeline::EditOptions::default(),
    )?;
    for (name, time) in [
        ("page-before", 4.0),
        ("page-fold", 5.0),
        ("page-after", 6.0),
    ] {
        op_application::export::export_frame(
            &e.project,
            pages,
            SeqTime::from_seconds(time),
            &output.join(format!("{name}.png")),
            gpu.clone(),
            e.media.clone(),
        )?;
    }
    e.new_sequence(
        "Cámara manga",
        SequenceSettings {
            width: 960,
            height: 540,
            ..SequenceSettings::default()
        },
    );
    let camera = e.active.unwrap();
    let bars = e.project.add_item(
        e.project.root,
        "Referencia",
        ItemKind::Synthetic {
            generator: Generator::BarsAndTone,
            duration: Dur::from_seconds(5.0),
        },
    );
    let clip = e.place_on_top(bars).unwrap();
    set(
        &mut e,
        camera,
        clip,
        "op.video.manga_camera",
        "direction",
        Value::Choice(0),
    );
    set(
        &mut e,
        camera,
        clip,
        "op.video.manga_frame",
        "horizontal",
        Value::Float(4.0),
    );
    for (name, time) in [("camera-start", 0.0), ("camera-end", 4.99)] {
        op_application::export::export_frame(
            &e.project,
            camera,
            SeqTime::from_seconds(time),
            &output.join(format!("{name}.png")),
            gpu.clone(),
            e.media.clone(),
        )?;
    }
    op_project::native::save(
        &output.join("Open-Premiere-demo.opproj"),
        &e.project,
        &serde_json::json!({}),
        "Open Premiere native effects demo",
    )?;
    println!("Demo: {}", output.display());
    Ok(())
}
