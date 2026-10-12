//! Heavy editing benchmark: thousands of cuts on one long clip with effects, the way a remix or
//! a YTPH is edited, timing each cut, the frame plan, a tick and the memory the program holds.
//! Run with `cargo test --release -p op-application --test stress_edits -- --ignored --nocapture`.

use std::time::Instant;

use op_application::{Dirs, Editor, Preferences};
use op_core::*;
use op_timeline::{
    EditOptions, MoveSpec, Patch, SourceClip, add_edit, move_clips, overwrite, ripple_delete,
};

fn asset(minutes: f64) -> MediaAsset {
    MediaAsset {
        id: AssetId(0),
        path: "missing-source.mp4".into(),
        proxy: None,
        kind: MediaKind::Video,
        video: Some(VideoStream {
            index: 0,
            codec: "h264".into(),
            width: 1920,
            height: 1080,
            pixel_aspect: (1, 1),
            rate: Rate::FPS_25,
            frames: (25.0 * 60.0 * minutes) as i64,
            pixel_format: "yuv420p".into(),
            bit_depth: 8,
            alpha: AlphaMode::None,
            color: ColorInfo::default(),
            field_order: FieldOrder::Progressive,
            start: Dur::ZERO,
            rotation: 0,
            timecode: None,
        }),
        audio: vec![AudioStream {
            index: 1,
            codec: "aac".into(),
            sample_rate: 48000,
            layout: ChannelLayout::Stereo,
            samples: (48000.0 * 60.0 * minutes) as i64,
            start: Dur::ZERO,
        }],
        duration: Dur::from_seconds(60.0 * minutes),
        interpretation: Interpretation::default(),
        file_size: 0,
        modified_unix: 0,
    }
}

fn rss_mb() -> f64 {
    let mut sys = sysinfo::System::new();
    let pid = sysinfo::get_current_pid().unwrap();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    sys.process(pid)
        .map(|p| p.memory() as f64 / 1048576.0)
        .unwrap_or(0.0)
}

#[test]
#[ignore]
fn thousands_of_cuts_stay_fast() {
    let cuts: usize = std::env::var("OP_STRESS_CUTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3000);
    let dir = tempfile::tempdir().unwrap();
    let mut ed = Editor::new(Dirs::portable(dir.path()), Preferences::default(), false);
    let root = ed.project.root;
    let (_, item) = ed
        .edit("Import", |p| Ok(p.add_asset(root, asset(60.0))))
        .unwrap();
    let sid = ed.new_sequence("S", SequenceSettings::default()).unwrap();
    ed.open_sequence(sid);
    let spec = SourceClip::from_asset(
        &ed.project,
        item,
        SrcRange::new(SrcTime::ZERO, SrcTime::from_seconds(3000.0)),
    )
    .unwrap();
    ed.edit("Overwrite", |p| {
        overwrite(
            p,
            sid,
            &spec,
            SeqTime::ZERO,
            &Patch::default(),
            EditOptions::default(),
        )
    })
    .unwrap();
    // a few effects on the clip, so every piece carries them
    let first = ed.active_seq().unwrap().video[0].clips[0].id;
    for fx in ["op.video.lumetri", "op.video.gaussian_blur", "op.video.vhs"] {
        ed.apply_effect(fx, Some(vec![first]));
    }
    let tracks = [TrackRef::video(0), TrackRef::audio(0)];
    println!(
        "{:>6} {:>10} {:>10} {:>10} {:>10} {:>9}",
        "clips", "cut ms", "plan ms", "tick ms", "snap ms", "RSS MB"
    );
    let mut worst_cut = 0.0f64;
    let mut x = 12345u64;
    for i in 1..=cuts {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let t = SeqTime::from_seconds(1.0 + (x >> 11) as f64 / (1u64 << 53) as f64 * 2990.0);
        let t0 = Instant::now();
        let _ = ed.seq_edit("Add Edit", |p, sid, opts| {
            add_edit(p, sid, &tracks, t, opts)
        });
        let cut = t0.elapsed().as_secs_f64() * 1000.0;
        worst_cut = worst_cut.max(cut);
        let t1 = Instant::now();
        ed.tick();
        let tick = t1.elapsed().as_secs_f64() * 1000.0;
        if i % (cuts / 6).max(1) == 0 || i == cuts {
            let t2 = Instant::now();
            let snap = ed.snapshot();
            let snap_ms = t2.elapsed().as_secs_f64() * 1000.0;
            let t3 = Instant::now();
            for k in 0..25 {
                let _ = plan::frame_plan(&snap, sid, SeqTime::from_seconds(k as f64 * 100.0));
            }
            let plan_ms = t3.elapsed().as_secs_f64() * 1000.0 / 25.0;
            let n = ed.active_seq().unwrap().video[0].clips.len();
            println!(
                "{n:>6} {cut:>10.2} {plan_ms:>10.3} {tick:>10.2} {snap_ms:>10.3} {:>9.0}",
                rss_mb()
            );
        }
    }
    println!("worst cut {worst_cut:.1} ms");
    // the whole timeline selected, moved by a frame, then half of it deleted with ripple
    let all: Vec<ClipId> = ed
        .active_seq()
        .unwrap()
        .clips()
        .map(|(_, c)| c.id)
        .collect();
    ed.selection.clips = all.clone();
    let t = Instant::now();
    let spec = MoveSpec {
        clips: all.clone(),
        delta: Dur::from_seconds(0.04),
        track_delta: 0,
        insert: false,
        duplicate: false,
    };
    ed.seq_edit("Move", |p, sid, opts| move_clips(p, sid, &spec, opts))
        .unwrap();
    println!(
        "move all {} clips {:.1} ms",
        all.len(),
        t.elapsed().as_secs_f64() * 1000.0
    );
    let half: Vec<ClipId> = all.iter().step_by(4).copied().collect();
    let t = Instant::now();
    ed.seq_edit("Ripple Delete", |p, sid, opts| {
        ripple_delete(p, sid, &half, opts)
    })
    .unwrap();
    println!(
        "ripple delete {} clips {:.1} ms",
        half.len(),
        t.elapsed().as_secs_f64() * 1000.0
    );
    let t = Instant::now();
    let text = op_project::native::to_string(&ed.project, &serde_json::Value::Null, None, "stress")
        .unwrap();
    println!(
        "save {:.1} ms, {:.1} MB",
        t.elapsed().as_secs_f64() * 1000.0,
        text.len() as f64 / 1048576.0
    );
    let t = Instant::now();
    ed.undo();
    println!("undo {:.2} ms", t.elapsed().as_secs_f64() * 1000.0);
}

/// Cut pieces share their effects until one is changed, so undo steps and razor cuts do not copy
/// every effect of every clip (a timeline of thousands of cuts ran out of memory that way).
#[test]
fn cut_pieces_share_their_effects_until_one_changes() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = Editor::new(Dirs::portable(dir.path()), Preferences::default(), false);
    let root = ed.project.root;
    let (_, item) = ed
        .edit("Import", |p| Ok(p.add_asset(root, asset(10.0))))
        .unwrap();
    let sid = ed.new_sequence("S", SequenceSettings::default()).unwrap();
    ed.open_sequence(sid);
    let spec = SourceClip::from_asset(
        &ed.project,
        item,
        SrcRange::new(SrcTime::ZERO, SrcTime::from_seconds(500.0)),
    )
    .unwrap();
    ed.edit("Overwrite", |p| {
        overwrite(
            p,
            sid,
            &spec,
            SeqTime::ZERO,
            &Patch::default(),
            EditOptions::default(),
        )
    })
    .unwrap();
    let first = ed.active_seq().unwrap().video[0].clips[0].id;
    ed.apply_effect("op.video.lumetri", Some(vec![first]));
    let tracks = [TrackRef::video(0), TrackRef::audio(0)];
    for i in 1..300 {
        let t = SeqTime::from_seconds(i as f64 * 1.5);
        ed.seq_edit("Add Edit", |p, sid, opts| {
            add_edit(p, sid, &tracks, t, opts)
        })
        .unwrap();
    }
    let clips = &ed.active_seq().unwrap().video[0].clips;
    assert_eq!(clips.len(), 300);
    // the pieces have their own components (own ids) but share every parameter list
    let shared = |a: &Clip, b: &Clip| {
        a.components.len() == b.components.len()
            && a.components
                .iter()
                .zip(b.components.iter())
                .all(|(x, y)| x.params.shares(&y.params))
    };
    assert!(
        clips.iter().all(|c| shared(c, &clips[0])),
        "pieces share their parameters"
    );
    // changing one piece copies only that piece's parameters
    let one = clips[10].id;
    ed.apply_effect("op.video.vhs", Some(vec![one]));
    let clips = &ed.active_seq().unwrap().video[0].clips;
    assert_eq!(clips[10].components.len(), clips[0].components.len() + 1);
    assert!(shared(&clips[11], &clips[0]));
}
