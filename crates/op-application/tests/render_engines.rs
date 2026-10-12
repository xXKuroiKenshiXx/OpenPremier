//! Independent compositor, exact output time, codec conversion and failure preservation.
use op_application::render_engine::{
    FframesRenderer, RenderEngine, fframes_blend_name, fframes_worker_path, scene_svg,
    validate_fframes,
};
use op_application::{ExportJob, ExportSettings, MediaService};
use op_core::*;
use op_render::NoFrames;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

fn project() -> (Project, SequenceId) {
    let mut p = Project::new("Renderer");
    let root = p.root;
    let (sid, _) = p.add_sequence(
        root,
        "S",
        SequenceSettings {
            width: 64,
            height: 36,
            rate: Rate::new(30_000, 1001),
            ..Default::default()
        },
    );
    let item = p.add_item(
        root,
        "Red",
        ItemKind::Synthetic {
            generator: Generator::ColorMatte {
                color: Rgba::new(1.0, 0.0, 0.0, 1.0),
            },
            duration: Dur::from_seconds(1.0),
        },
    );
    let source = op_timeline::SourceClip::from_item(&p, item).unwrap();
    let (p, _) = p
        .transact(|p| {
            op_timeline::overwrite(
                p,
                sid,
                &source,
                SeqTime::ZERO,
                &op_timeline::Patch {
                    video: Some(0),
                    audio: vec![],
                },
                Default::default(),
            )
        })
        .unwrap();
    (p, sid)
}
fn worker() -> bool {
    if fframes_worker_path().is_some() {
        true
    } else {
        assert!(
            std::env::var("OPENPREMIER_REQUIRE_FFRAMES").as_deref() != Ok("1"),
            "fframes worker required"
        );
        eprintln!("fframes worker unavailable; runtime test skipped");
        false
    }
}
fn engines() -> Vec<RenderEngine> {
    let mut engines = vec![RenderEngine::FframesCpu];
    let gpu = FframesRenderer::start_with_engine(
        RenderEngine::FframesGpu,
        16,
        16,
        &AtomicBool::new(false),
    );
    match gpu {
        Ok(renderer) => {
            eprintln!("GPU: {}", renderer.description());
            engines.push(RenderEngine::FframesGpu);
        }
        Err(error) => {
            assert!(
                std::env::var("OPENPREMIER_REQUIRE_FFRAMES_GPU").as_deref() != Ok("1"),
                "fframes GPU required: {error}"
            );
            eprintln!("fframes GPU unavailable: {error}");
        }
    }
    engines
}
fn wait(job: &ExportJob) {
    let start = Instant::now();
    while !job.finished() {
        assert!(start.elapsed() < Duration::from_secs(90), "export timeout");
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
fn old_preferences_keep_native_and_engine_names_round_trip() {
    let prefs: op_application::Preferences = serde_json::from_str("{}").unwrap();
    assert_eq!(prefs.export_engine, RenderEngine::Wgpu);
    for engine in RenderEngine::ALL {
        let json = serde_json::to_string(&engine).unwrap();
        assert_eq!(serde_json::from_str::<RenderEngine>(&json).unwrap(), engine);
    }
}
#[test]
fn workers_render_scenes_alpha_and_cancellation() {
    if !worker() {
        return;
    }
    for engine in engines() {
        let (mut p, sid) = project();
        validate_fframes(&p, sid).unwrap();
        let mut renderer =
            FframesRenderer::start_with_engine(engine, 64, 36, &AtomicBool::new(false)).unwrap();
        let svg = scene_svg(&p, sid, SeqTime::ZERO, &NoFrames).unwrap();
        let pixels = renderer
            .render(svg, 64, 36, &AtomicBool::new(false))
            .unwrap();
        assert_eq!(
            &pixels[(18 * 64 + 32) * 4..(18 * 64 + 32) * 4 + 4],
            &[255, 0, 0, 255]
        );
        let clip = &mut Arc::make_mut(&mut p.sequence_mut(sid).unwrap().video[0]).clips[0];
        clip.component_mut(catalog::MOTION)
            .unwrap()
            .param_mut("scale")
            .unwrap()
            .value = Value::Float(50.0);
        clip.component_mut(catalog::OPACITY)
            .unwrap()
            .param_mut("opacity")
            .unwrap()
            .value = Value::Float(50.0);
        let pixels = renderer
            .render(
                scene_svg(&p, sid, SeqTime::ZERO, &NoFrames).unwrap(),
                64,
                36,
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(pixels[3], 0);
        let center = &pixels[(18 * 64 + 32) * 4..(18 * 64 + 32) * 4 + 4];
        assert_eq!(&center[..3], &[255, 0, 0]);
        assert!((center[3] as i32 - 128).abs() <= 1);
        // Cancellation must release the pipes and child even when a reply is pending.
        assert!(
            renderer
                .render(
                    scene_svg(&p, sid, SeqTime::ZERO, &NoFrames).unwrap(),
                    64,
                    36,
                    &AtomicBool::new(true)
                )
                .is_err()
        );
        drop(renderer);
    }
}
#[test]
fn fframes_exports_every_codec_with_exact_ntsc_rate_and_native_audio() {
    if !worker() {
        return;
    }
    for engine in engines() {
        let dir = tempfile::tempdir().unwrap();
        let (mut p, sid) = project();
        let opacity = Arc::make_mut(&mut p.sequence_mut(sid).unwrap().video[0]).clips[0]
            .component_mut(catalog::OPACITY)
            .unwrap()
            .param_mut("opacity")
            .unwrap();
        opacity.animated = true;
        opacity.keys = vec![
            Keyframe::new(SrcTime::ZERO, Value::Float(50.0), Interp::Linear),
            Keyframe::new(
                SrcTime::ZERO + Rate::new(30_000, 1001).frames_to_dur(2),
                Value::Float(100.0),
                Interp::Linear,
            ),
        ];
        for codec in op_media::VideoCodec::ALL {
            let path = dir
                .path()
                .join(format!("{:?}.{}", codec, codec.extension()));
            let rate = Rate::new(30_000, 1001);
            let settings = ExportSettings {
                engine,
                path: path.clone(),
                sequence: sid,
                range: SeqRange::with_duration(SeqTime::ZERO, rate.frames_to_dur(3)),
                video: Some(op_media::VideoSettings {
                    color: Default::default(),
                    hdr10: Default::default(),
                    codec,
                    width: 320,
                    height: 180,
                    rate,
                    bitrate_kbps: None,
                    quality: 18,
                    hardware: false,
                }),
                audio: Some(op_media::AudioSettings {
                    codec: op_media::AudioCodec::Pcm16,
                    rate: 48_000,
                    channels: 2,
                    bitrate_kbps: 0,
                }),
            };
            // MP4 requires AAC; other containers carry the native PCM mix.
            let mut settings = settings;
            if matches!(
                codec,
                op_media::VideoCodec::H264 | op_media::VideoCodec::Hevc
            ) {
                settings.audio.as_mut().unwrap().codec = op_media::AudioCodec::Aac;
            }
            let media = MediaService::new(dir.path().join("cache"), 64 << 20);
            let job = ExportJob::start_headless(Arc::new(p.clone()), settings, media);
            wait(&job);
            let progress = job.progress.lock();
            assert!(progress.error.is_none(), "{codec:?}: {:?}", progress.error);
            assert_eq!(progress.total, 3);
            assert!(progress.renderer.contains("fframes"));
            drop(progress);
            let asset = op_media::probe(&path).unwrap();
            assert_eq!(asset.video.as_ref().unwrap().rate, rate);
            assert!(asset.has_audio());
            let v = asset.video.as_ref().unwrap();
            let mut decoder =
                op_media::VideoDecoder::open(&path, Some(v.index), Some(v.rate), v.color, false)
                    .unwrap();
            let pixels = decoder.frame(0).unwrap().to_rgba8();
            let center = &pixels[(90 * 320 + 160) * 4..(90 * 320 + 160) * 4 + 4];
            if matches!(
                codec,
                op_media::VideoCodec::Png | op_media::VideoCodec::ProRes4444
            ) {
                assert!(
                    center[0] > 240 && (i32::from(center[3]) - 128).abs() < 5,
                    "{codec:?}: {center:?}"
                );
            } else {
                assert!(
                    (i32::from(center[0]) - 128).abs() < 6 && center[3] == 255,
                    "{codec:?}: {center:?}"
                );
            }
            let pixels = decoder.frame(2).unwrap().to_rgba8();
            let pixel = &pixels[(90 * 320 + 160) * 4..(90 * 320 + 160) * 4 + 4];
            assert!(
                pixel[0] > 240 && pixel[1] < 12 && pixel[2] < 12,
                "{codec:?}: {pixel:?}"
            );
        }
    }
}
#[test]
fn unsupported_features_leave_project_and_existing_destination_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let (mut p, sid) = project();
    let mut ids = p.ids.clone();
    let mut effect = Component::new(catalog::find("op.video.gaussian_blur").unwrap(), &mut ids);
    effect.effect = "external.unimplemented.plugin".into();
    Arc::make_mut(&mut p.sequence_mut(sid).unwrap().video[0]).clips[0]
        .components
        .push(effect);
    p.ids = ids;
    let before = p.clone();
    assert!(
        validate_fframes(&p, sid)
            .unwrap_err()
            .contains("external.unimplemented.plugin")
    );
    let path = dir.path().join("existing.mov");
    std::fs::write(&path, b"keep this file").unwrap();
    let settings = ExportSettings {
        engine: RenderEngine::FframesCpu,
        path: path.clone(),
        sequence: sid,
        range: SeqRange::with_duration(SeqTime::ZERO, Dur::from_seconds(0.1)),
        video: Some(op_media::VideoSettings {
            color: Default::default(),
            hdr10: Default::default(),
            codec: op_media::VideoCodec::Png,
            width: 64,
            height: 36,
            rate: Rate::FPS_25,
            bitrate_kbps: None,
            quality: 0,
            hardware: false,
        }),
        audio: None,
    };
    let job = ExportJob::start_headless(
        Arc::new(p.clone()),
        settings,
        MediaService::new(dir.path().join("cache"), 64 << 20),
    );
    wait(&job);
    assert!(
        job.progress
            .lock()
            .error
            .as_ref()
            .unwrap()
            .contains("external.unimplemented.plugin")
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"keep this file");
    assert_eq!(p, before);
}

#[test]
fn fframes_renders_nested_shapes_and_native_text_glyphs() {
    if !worker() {
        return;
    }
    for engine in engines() {
        let (mut p, sid) = project();
        let mut ids = p.ids.clone();
        let mut shape = Component::new(catalog::find(catalog::SHAPE).unwrap(), &mut ids);
        for (key, value) in [
            ("width", Value::Float(20.0)),
            ("height", Value::Float(12.0)),
            ("fill", Value::Color(Rgba::new(0.0, 1.0, 0.0, 1.0))),
        ] {
            shape.param_mut(key).unwrap().value = value;
        }
        let mut text = Component::new(catalog::find(catalog::TEXT).unwrap(), &mut ids);
        for (key, value) in [
            ("text", Value::Text("OK".into())),
            ("font_size", Value::Float(10.0)),
            ("position", Value::Point([0.5, 0.5])),
        ] {
            text.param_mut(key).unwrap().value = value;
        }
        let clip = &mut Arc::make_mut(&mut p.sequence_mut(sid).unwrap().video[0]).clips[0];
        clip.source = ClipSource::Graphic;
        clip.components.push(shape);
        clip.components.push(text);
        p.ids = ids;
        let root = p.root;
        let item = p
            .items
            .values()
            .find(|i| matches!(i.kind, ItemKind::Sequence { sequence } if sequence == sid))
            .unwrap()
            .id;
        let (outer, _) = p.add_sequence(root, "Nested", p.sequence(sid).unwrap().settings.clone());
        let source = op_timeline::SourceClip::from_item(&p, item).unwrap();
        let (p, _) = p
            .transact(|p| {
                op_timeline::overwrite(
                    p,
                    outer,
                    &source,
                    SeqTime::ZERO,
                    &op_timeline::Patch {
                        video: Some(0),
                        audio: vec![],
                    },
                    Default::default(),
                )
            })
            .unwrap();
        validate_fframes(&p, outer).unwrap();
        let svg = scene_svg(&p, outer, SeqTime::ZERO, &NoFrames).unwrap();
        assert!(svg.contains("data:image/png;base64,"));
        let mut renderer =
            FframesRenderer::start_with_engine(engine, 64, 36, &AtomicBool::new(false)).unwrap();
        let pixels = renderer
            .render(svg, 64, 36, &AtomicBool::new(false))
            .unwrap();
        assert_eq!(pixels[3], 0);
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[1] > 240 && p[0] < 10 && p[3] == 255),
            "green shape missing"
        );
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[0] > 180 && p[1] > 180 && p[2] > 180 && p[3] > 0),
            "native text missing"
        );
    }
}

#[test]
fn fframes_exports_decoded_media_and_audible_native_mix() {
    if !worker() {
        return;
    }
    for engine in engines() {
        let dir = tempfile::tempdir().unwrap();
        let rate = Rate::FPS_25;
        let video = op_media::VideoSettings {
            color: Default::default(),
            hdr10: Default::default(),
            codec: op_media::VideoCodec::Png,
            width: 64,
            height: 36,
            rate,
            bitrate_kbps: None,
            quality: 18,
            hardware: false,
        };
        let audio = op_media::AudioSettings {
            codec: op_media::AudioCodec::Pcm16,
            rate: 48_000,
            channels: 2,
            bitrate_kbps: 0,
        };
        let source_path = dir.path().join("source.mov");
        let mut muxer = op_media::Muxer::create(&source_path, Some(&video), Some(&audio)).unwrap();
        let pixels = [0, 0, 255, 255].repeat(64 * 36);
        for n in 0..5 {
            muxer.push_rgba(&pixels).unwrap();
            let tone = (0..1920 * 2)
                .map(|k| {
                    ((n * 1920 + k / 2) as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin()
                        * 0.25
                })
                .collect::<Vec<_>>();
            muxer.push_audio(&tone).unwrap();
        }
        muxer.finish().unwrap();
        let (mut p, sid) = project();
        Arc::make_mut(&mut p.sequence_mut(sid).unwrap().video[0])
            .clips
            .clear();
        let root = p.root;
        let (_, item) = p.add_asset(root, op_media::probe(&source_path).unwrap());
        let source = op_timeline::SourceClip::from_item(&p, item).unwrap();
        let (p, _) = p
            .transact(|p| {
                op_timeline::overwrite(
                    p,
                    sid,
                    &source,
                    SeqTime::ZERO,
                    &op_timeline::Patch {
                        video: Some(0),
                        audio: vec![Some(0)],
                    },
                    Default::default(),
                )
            })
            .unwrap();
        let path = dir.path().join("output.mov");
        let job = ExportJob::start_headless(
            Arc::new(p),
            ExportSettings {
                engine,
                path: path.clone(),
                sequence: sid,
                range: SeqRange::with_duration(SeqTime::ZERO, rate.frames_to_dur(3)),
                video: Some(video),
                audio: Some(audio),
            },
            MediaService::new(dir.path().join("cache"), 64 << 20),
        );
        wait(&job);
        assert!(
            job.progress.lock().error.is_none(),
            "{:?}",
            job.progress.lock().error
        );
        let asset = op_media::probe(&path).unwrap();
        let stream = asset.video.unwrap();
        let mut decoder = op_media::VideoDecoder::open(
            &path,
            Some(stream.index),
            Some(stream.rate),
            stream.color,
            false,
        )
        .unwrap();
        let pixels = decoder.frame(2).unwrap().to_rgba8();
        assert_eq!(
            &pixels[(18 * 64 + 32) * 4..(18 * 64 + 32) * 4 + 4],
            &[0, 0, 255, 255]
        );
        let conformed = dir.path().join("audio.bin");
        op_media::audio::conform(
            &path,
            0,
            Dur::ZERO,
            &conformed,
            &dir.path().join("peaks.bin"),
            &mut |_| true,
        )
        .unwrap();
        let samples = op_media::audio::ConformedAudio::open(&conformed).unwrap();
        let mut values = vec![0.0; 4096];
        samples.read(0, &mut values);
        let rms = (values.iter().map(|v| v * v).sum::<f32>() / values.len() as f32).sqrt();
        assert!((0.1..0.2).contains(&rms), "native audio RMS: {rms}");
    }
}

#[test]
fn native_software_selection_keeps_wgsl_effects() {
    let dir = tempfile::tempdir().unwrap();
    let (mut p, sid) = project();
    let mut ids = p.ids.clone();
    let effect = Component::new(catalog::find("op.video.gaussian_blur").unwrap(), &mut ids);
    Arc::make_mut(&mut p.sequence_mut(sid).unwrap().video[0]).clips[0]
        .components
        .push(effect);
    p.ids = ids;
    let path = dir.path().join("software.mov");
    let job = ExportJob::start_headless(
        Arc::new(p),
        ExportSettings {
            engine: RenderEngine::WgpuSoftware,
            path: path.clone(),
            sequence: sid,
            range: SeqRange::with_duration(SeqTime::ZERO, Rate::FPS_25.frames_to_dur(1)),
            video: Some(op_media::VideoSettings {
                color: Default::default(),
                hdr10: Default::default(),
                codec: op_media::VideoCodec::Png,
                width: 64,
                height: 36,
                rate: Rate::FPS_25,
                bitrate_kbps: None,
                quality: 18,
                hardware: false,
            }),
            audio: None,
        },
        MediaService::new(dir.path().join("cache"), 64 << 20),
    );
    wait(&job);
    let progress = job.progress.lock();
    if let Some(error) = &progress.error {
        assert!(
            std::env::var("OPENPREMIER_REQUIRE_SOFTWARE").as_deref() != Ok("1"),
            "{error}"
        );
        assert!(
            error.starts_with("software renderer unavailable:"),
            "{error}"
        );
        assert!(!path.exists());
        return;
    }
    eprintln!("Native software adapter: {}", progress.renderer);
    assert_eq!(progress.total, 1);
    let asset = op_media::probe(&path).unwrap();
    let stream = asset.video.unwrap();
    let mut decoder = op_media::VideoDecoder::open(
        &path,
        Some(stream.index),
        Some(stream.rate),
        stream.color,
        false,
    )
    .unwrap();
    let pixels = decoder.frame(0).unwrap().to_rgba8();
    assert_eq!(
        &pixels[(18 * 64 + 32) * 4..(18 * 64 + 32) * 4 + 4],
        &[255, 0, 0, 255]
    );
}

#[test]
fn fframes_dissolves_mix_premultiplied_alpha_and_dips_reach_their_color() {
    if !worker() {
        return;
    }
    for engine in engines() {
        let (mut p, sid) = project();
        let blue = p.add_item(
            p.root,
            "Blue",
            ItemKind::Synthetic {
                generator: Generator::ColorMatte {
                    color: Rgba::new(0.0, 0.0, 1.0, 1.0),
                },
                duration: Dur::from_seconds(1.0),
            },
        );
        let incoming_id = ClipId(p.ids.next_raw());
        let transition_id = TransitionId(p.ids.next_raw());
        let track = Arc::make_mut(&mut p.sequence_mut(sid).unwrap().video[0]);
        let mut incoming = track.clips[0].clone();
        incoming.id = incoming_id;
        incoming.source = ClipSource::Generator { item: blue };
        incoming.start = SeqTime::from_seconds(1.0);
        let outgoing = track.clips[0].id;
        track.clips.push(incoming);
        track.transitions.push(Transition {
            id: transition_id,
            effect: catalog::CROSS_DISSOLVE.into(),
            cut: SeqTime::from_seconds(1.0),
            duration: Dur::from_seconds(1.0),
            alignment: Alignment::CenterAtCut,
            from: Some(outgoing),
            to: Some(incoming_id),
            params: vec![],
        });
        let mut renderer =
            FframesRenderer::start_with_engine(engine, 64, 36, &AtomicBool::new(false)).unwrap();
        for (effect, expected) in [
            (catalog::CROSS_DISSOLVE, [128, 0, 128, 255]),
            ("op.tr.dip_to_black", [0, 0, 0, 255]),
            ("op.tr.dip_to_white", [255, 255, 255, 255]),
        ] {
            Arc::make_mut(&mut p.sequence_mut(sid).unwrap().video[0]).transitions[0].effect =
                effect.into();
            validate_fframes(&p, sid).unwrap();
            let pixels = renderer
                .render(
                    scene_svg(&p, sid, SeqTime::from_seconds(1.0), &NoFrames).unwrap(),
                    64,
                    36,
                    &AtomicBool::new(false),
                )
                .unwrap();
            let center = &pixels[(18 * 64 + 32) * 4..(18 * 64 + 32) * 4 + 4];
            assert!(
                center
                    .iter()
                    .zip(expected)
                    .all(|(a, b)| (i32::from(*a) - b).abs() <= 2),
                "{engine:?} {effect}: {center:?}"
            );
        }
        // Fading a single half-transparent side must retain straight color and
        // halve its alpha, rather than accidentally darkening RGB a second time.
        let track = Arc::make_mut(&mut p.sequence_mut(sid).unwrap().video[0]);
        track.transitions[0].effect = catalog::CROSS_DISSOLVE.into();
        track.transitions[0].to = None;
        track.clips[0]
            .component_mut(catalog::OPACITY)
            .unwrap()
            .param_mut("opacity")
            .unwrap()
            .value = Value::Float(50.0);
        let pixels = renderer
            .render(
                scene_svg(&p, sid, SeqTime::from_seconds(1.0), &NoFrames).unwrap(),
                64,
                36,
                &AtomicBool::new(false),
            )
            .unwrap();
        let center = &pixels[(18 * 64 + 32) * 4..(18 * 64 + 32) * 4 + 4];
        assert!(
            center[0] > 250 && (i32::from(center[3]) - 64).abs() <= 2,
            "{engine:?}: {center:?}"
        );
    }
}

#[test]
fn fframes_blends_layers_and_draws_text_shadows() {
    if !worker() {
        return;
    }
    for engine in engines() {
        let (mut p, sid) = project();
        let mut ids = p.ids.clone();
        let mut text = Component::new(catalog::find(catalog::TEXT).unwrap(), &mut ids);
        for (key, value) in [
            ("text", Value::Text("A".into())),
            ("font_size", Value::Float(14.0)),
            ("shadow", Value::Bool(true)),
            ("shadow_color", Value::Color(Rgba::new(0.0, 0.0, 1.0, 1.0))),
            ("shadow_blur", Value::Float(3.0)),
            ("shadow_distance", Value::Float(8.0)),
        ] {
            text.param_mut(key).unwrap().value = value;
        }
        let track = Arc::make_mut(&mut p.sequence_mut(sid).unwrap().video[0]);
        track.clips[0].source = ClipSource::Graphic;
        track.clips[0].components.push(text);
        p.ids = ids;
        validate_fframes(&p, sid).unwrap();
        let mut renderer =
            FframesRenderer::start_with_engine(engine, 64, 36, &AtomicBool::new(false)).unwrap();
        let pixels = renderer
            .render(
                scene_svg(&p, sid, SeqTime::ZERO, &NoFrames).unwrap(),
                64,
                36,
                &AtomicBool::new(false),
            )
            .unwrap();
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[2] > 200 && p[0] < 20 && p[3] > 20),
            "{engine:?}: shadow missing"
        );
        let (mut p, sid) = project();
        let item = p.add_item(
            p.root,
            "Gray",
            ItemKind::Synthetic {
                generator: Generator::ColorMatte {
                    color: Rgba::new(0.5, 0.5, 0.5, 1.0),
                },
                duration: Dur::from_seconds(1.0),
            },
        );
        let mut top = (*p.sequence(sid).unwrap().video[0]).clone();
        top.id = TrackId(p.ids.next_raw());
        top.clips[0].id = ClipId(p.ids.next_raw());
        top.clips[0].source = ClipSource::Generator { item };
        top.clips[0]
            .component_mut(catalog::OPACITY)
            .unwrap()
            .param_mut("blend_mode")
            .unwrap()
            .value = Value::Choice(3);
        p.sequence_mut(sid).unwrap().video.push(Arc::new(top));
        validate_fframes(&p, sid).unwrap();
        let pixels = renderer
            .render(
                scene_svg(&p, sid, SeqTime::ZERO, &NoFrames).unwrap(),
                64,
                36,
                &AtomicBool::new(false),
            )
            .unwrap();
        let center = &pixels[(18 * 64 + 32) * 4..(18 * 64 + 32) * 4 + 4];
        assert!(
            (i32::from(center[0]) - 128).abs() <= 2
                && center[1] == 0
                && center[2] == 0
                && center[3] == 255,
            "{engine:?}: multiply {center:?}"
        );
    }
}

#[test]
fn every_supported_blend_matches_native_opaque_reference_pixels() {
    if !worker() {
        return;
    }
    let gpu = match op_render::Gpu::headless() {
        Ok(gpu) => gpu,
        Err(error) => {
            assert!(
                std::env::var("OPENPREMIER_REQUIRE_GPU").as_deref() != Ok("1"),
                "{error}"
            );
            return;
        }
    };
    let mut native = op_render::Renderer::new(gpu);
    for engine in engines() {
        let (mut p, sid) = project();
        let bottom = p.add_item(
            p.root,
            "Bottom",
            ItemKind::Synthetic {
                generator: Generator::ColorMatte {
                    color: Rgba::new(0.3, 0.6, 0.8, 1.0),
                },
                duration: Dur::from_seconds(1.0),
            },
        );
        let upper = p.add_item(
            p.root,
            "Upper",
            ItemKind::Synthetic {
                generator: Generator::ColorMatte {
                    color: Rgba::new(0.8, 0.4, 0.2, 1.0),
                },
                duration: Dur::from_seconds(1.0),
            },
        );
        Arc::make_mut(&mut p.sequence_mut(sid).unwrap().video[0]).clips[0].source =
            ClipSource::Generator { item: bottom };
        let mut top = (*p.sequence(sid).unwrap().video[0]).clone();
        top.id = TrackId(p.ids.next_raw());
        top.clips[0].id = ClipId(p.ids.next_raw());
        top.clips[0].source = ClipSource::Generator { item: upper };
        p.sequence_mut(sid).unwrap().video.push(Arc::new(top));
        let mut renderer =
            FframesRenderer::start_with_engine(engine, 64, 36, &AtomicBool::new(false)).unwrap();
        for mode in 0..catalog::BLEND_MODES.len() as u32 {
            if fframes_blend_name(mode).is_none() {
                continue;
            }
            Arc::make_mut(p.sequence_mut(sid).unwrap().video.last_mut().unwrap()).clips[0]
                .component_mut(catalog::OPACITY)
                .unwrap()
                .param_mut("blend_mode")
                .unwrap()
                .value = Value::Choice(mode);
            validate_fframes(&p, sid).unwrap();
            let frame = native.render(&op_render::Request {
                project: &p,
                sequence: sid,
                time: SeqTime::ZERO,
                scale: 1.0,
                source: &NoFrames,
            });
            let display = native.display_target(64, 36);
            native.present_into(&frame, &display, false);
            let expected = native.read_texture(&display, 4).unwrap();
            native.recycle(frame);
            let actual = renderer
                .render(
                    scene_svg(&p, sid, SeqTime::ZERO, &NoFrames).unwrap(),
                    64,
                    36,
                    &AtomicBool::new(false),
                )
                .unwrap();
            let k = (18 * 64 + 32) * 4;
            assert!(
                actual[k..k + 4]
                    .iter()
                    .zip(&expected[k..k + 4])
                    .all(|(a, b)| (i32::from(*a) - i32::from(*b)).abs() <= 4),
                "{engine:?} {}: {:?} vs native {:?}",
                catalog::BLEND_MODES[mode as usize],
                &actual[k..k + 4],
                &expected[k..k + 4]
            );
        }
    }
}
