//! Catalog compatibility is tested against native pixels, including nonidentity parameters.
use op_application::render_engine::{
    FframesRenderer, RenderEngine, fframes_worker_path, validate_fframes_hybrid,
};
use op_core::*;
use op_render::{Gpu, NoFrames, Renderer, Request};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

fn project() -> (Project, SequenceId) {
    let mut p = Project::new("Floating compatibility");
    let (sid, _) = p.add_sequence(
        p.root,
        "S",
        SequenceSettings {
            width: 64,
            height: 36,
            ..Default::default()
        },
    );
    let item = p.add_item(
        p.root,
        "Bars",
        ItemKind::Synthetic {
            generator: Generator::BarsAndTone,
            duration: Dur::from_seconds(2.0),
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
fn engines() -> Vec<RenderEngine> {
    if fframes_worker_path().is_none() {
        assert_ne!(
            std::env::var("OPENPREMIER_REQUIRE_FFRAMES").as_deref(),
            Ok("1")
        );
        return vec![];
    }
    let mut engines = vec![RenderEngine::FframesCpu];
    match FframesRenderer::start_hybrid(RenderEngine::FframesGpu, 64, 36, &AtomicBool::new(false)) {
        Ok(_) => engines.push(RenderEngine::FframesGpu),
        Err(e) => {
            assert_ne!(
                std::env::var("OPENPREMIER_REQUIRE_FFRAMES_GPU").as_deref(),
                Ok("1"),
                "{e}"
            );
        }
    }
    engines
}
fn compare(
    p: &Project,
    sid: SequenceId,
    time: SeqTime,
    native: &mut Renderer,
    processor: &mut Renderer,
    worker: &mut FframesRenderer,
    label: &str,
) -> usize {
    validate_fframes_hybrid(p, sid).unwrap();
    let req = Request {
        project: p,
        sequence: sid,
        time,
        scale: 1.0,
        source: &NoFrames,
    };
    let frame = native.render(&req);
    let expected = native.rgba8(&frame).unwrap();
    native.recycle(frame);
    let layers = processor.render_contributions(&req);
    let count = layers.len();
    let pixels = processor
        .read_textures(&layers.iter().map(|t| (t, 8)).collect::<Vec<_>>())
        .unwrap();
    for layer in layers {
        processor.recycle(layer);
    }
    let floats = worker
        .render_layers(pixels, &AtomicBool::new(false))
        .unwrap();
    let frame = processor.upload_working(64, 36, &floats).unwrap();
    let actual = processor.rgba8(&frame).unwrap();
    processor.recycle(frame);
    let worst = actual
        .iter()
        .zip(&expected)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert!(worst <= 2, "{label}: maximum pixel difference {worst}");
    count
}

#[test]
fn floating_transport_preserves_superwhite_and_sub_eight_bit_values() {
    for engine in engines() {
        let mut worker =
            FframesRenderer::start_hybrid(engine, 64, 36, &AtomicBool::new(false)).unwrap();
        // RGBA16F: 2.0, 0.50048828125, 0.25, 1.0; no 8-bit representation exists.
        let pixel = [0x00, 0x40, 0x01, 0x38, 0x00, 0x34, 0x00, 0x3c];
        let input = pixel.repeat(64 * 36);
        assert_eq!(
            worker
                .render_layers(vec![input.clone()], &AtomicBool::new(false))
                .unwrap(),
            input,
            "{engine:?}"
        );
        // Cover every positive half-float subnormal at tiny premultiplied alpha.
        // The CPU Skia F16 raster path previously turned these pixels into zero.
        let shadows = (1_u16..=1024)
            .cycle()
            .take(64 * 36)
            .flat_map(|bits| [bits; 4])
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            worker
                .render_layers(vec![shadows.clone()], &AtomicBool::new(false))
                .unwrap(),
            shadows,
            "tiny-alpha shadows {engine:?}"
        );
        let transparent = worker
            .render_layers(vec![], &AtomicBool::new(false))
            .unwrap();
        assert!(transparent.iter().all(|v| *v == 0));
        let red = [0x00, 0x38, 0, 0, 0, 0, 0x00, 0x38].repeat(64 * 36);
        let green = [0, 0, 0x00, 0x34, 0, 0, 0x00, 0x38].repeat(64 * 36);
        let over = worker
            .render_layers(vec![red, green], &AtomicBool::new(false))
            .unwrap();
        assert_eq!(
            over,
            [0x00, 0x34, 0x00, 0x34, 0, 0, 0x00, 0x3a].repeat(64 * 36),
            "premultiplied source-over {engine:?}"
        );
        assert!(
            worker
                .render_layers(vec![vec![0; 4]], &AtomicBool::new(false))
                .is_err()
        );
        assert!(
            worker
                .render_layers(vec![pixel.repeat(64 * 36)], &AtomicBool::new(true))
                .unwrap_err()
                .contains("cancelled")
        );
    }
}

#[test]
fn interpreted_high_depth_rotated_hdr_sources_match_native_conversion() {
    struct Source(Arc<op_media::VideoFrame>);
    impl op_render::FrameSource for Source {
        fn video_frame(&self, _: &MediaAsset, _: SrcTime) -> Option<Arc<op_media::VideoFrame>> {
            Some(self.0.clone())
        }
    }
    for engine in engines() {
        let gpu = Gpu::headless().unwrap();
        let mut native = Renderer::new(gpu.clone());
        let mut processor = Renderer::new(gpu);
        let mut worker =
            FframesRenderer::start_hybrid(engine, 64, 36, &AtomicBool::new(false)).unwrap();
        for (transfer, rotation, invert) in [
            (Transfer::Bt709, 90, false),
            (Transfer::Pq, 270, false),
            (Transfer::Hlg, 0, true),
            (Transfer::Linear, 180, false),
        ] {
            let (mut p, sid) = project();
            let color = ColorInfo {
                transfer,
                primaries: Primaries::Bt2020,
                ..Default::default()
            };
            let asset = MediaAsset {
                id: AssetId(0),
                path: "synthetic.rgba16".into(),
                proxy: None,
                kind: MediaKind::Video,
                video: Some(VideoStream {
                    index: 0,
                    codec: "rawvideo".into(),
                    width: 32,
                    height: 16,
                    pixel_aspect: (4, 3),
                    rate: Rate::FPS_25,
                    frames: 50,
                    pixel_format: "rgba64le".into(),
                    bit_depth: 16,
                    alpha: AlphaMode::Premultiplied,
                    color,
                    field_order: FieldOrder::Progressive,
                    start: Dur::ZERO,
                    rotation,
                    timecode: None,
                }),
                audio: vec![],
                duration: Dur::from_seconds(2.0),
                interpretation: Interpretation {
                    invert_alpha: invert,
                    ..Default::default()
                },
                file_size: 0,
                modified_unix: 0,
            };
            let (aid, _) = p.add_asset(p.root, asset);
            Arc::make_mut(&mut p.sequence_mut(sid).unwrap().video[0]).clips[0].source =
                ClipSource::Asset {
                    asset: aid,
                    item: None,
                    stream: 0,
                };
            validate_fframes_hybrid(&p, sid).unwrap();
            let data: Vec<u8> = (0..32 * 16)
                .flat_map(|i| [((i % 32) * 700) as u16, 16000u16, 8000u16, 40000u16])
                .flat_map(u16::to_le_bytes)
                .collect();
            let source = Source(Arc::new(op_media::VideoFrame {
                width: 32,
                height: 16,
                layout: op_media::PixelLayout::Rgba16,
                planes: vec![op_media::Plane {
                    data,
                    width: 32,
                    height: 16,
                    bytes_per_row: 32 * 8,
                }],
                color,
                index: 0,
            }));
            native.forget_media();
            processor.forget_media();
            let req = Request {
                project: &p,
                sequence: sid,
                time: SeqTime::ZERO,
                scale: 1.0,
                source: &source,
            };
            let frame = native.render(&req);
            let expected = native.rgba8(&frame).unwrap();
            native.recycle(frame);
            let layers = processor.render_contributions(&req);
            let pixels = processor
                .read_textures(&layers.iter().map(|t| (t, 8)).collect::<Vec<_>>())
                .unwrap();
            for layer in layers {
                processor.recycle(layer);
            }
            let floats = worker
                .render_layers(pixels, &AtomicBool::new(false))
                .unwrap();
            let frame = processor.upload_working(64, 36, &floats).unwrap();
            let actual = processor.rgba8(&frame).unwrap();
            processor.recycle(frame);
            assert!(
                actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
                "{transfer:?} rotation {rotation} invert {invert}"
            );
        }
    }
}

#[test]
fn hybrid_exports_every_codec_and_reports_the_actual_processors() {
    use op_application::{ExportJob, ExportSettings, MediaService};
    use std::time::{Duration, Instant};
    let engines = engines();
    if engines.is_empty() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut p, sid) = project();
    p.sequence_mut(sid).unwrap().settings.width = 320;
    p.sequence_mut(sid).unwrap().settings.height = 180;
    let effect = Component::new(catalog::find("op.video.invert").unwrap(), &mut p.ids);
    Arc::make_mut(&mut p.sequence_mut(sid).unwrap().video[0]).clips[0]
        .components
        .push(effect);
    let snapshot = Arc::new(p);
    for codec in op_media::VideoCodec::ALL {
        let mut expected = None;
        for engine in std::iter::once(RenderEngine::Wgpu).chain(engines.iter().copied()) {
            let path = dir.path().join(format!("{codec:?}-{engine:?}.mov"));
            let job = ExportJob::start_headless(
                snapshot.clone(),
                ExportSettings {
                    path: path.clone(),
                    sequence: sid,
                    range: SeqRange::with_duration(
                        SeqTime::ZERO,
                        Rate::new(30000, 1001).frames_to_dur(3),
                    ),
                    engine,
                    video: Some(op_media::VideoSettings {
                        color: Default::default(),
                        hdr10: Default::default(),
                        codec,
                        width: 320,
                        height: 180,
                        rate: Rate::new(30000, 1001),
                        bitrate_kbps: None,
                        quality: 18,
                        hardware: false,
                    }),
                    audio: None,
                },
                MediaService::new(
                    dir.path().join(format!("cache-{codec:?}-{engine:?}")),
                    64 << 20,
                ),
            );
            let since = Instant::now();
            while !job.finished() {
                assert!(since.elapsed() < Duration::from_secs(60));
                std::thread::sleep(Duration::from_millis(20));
            }
            let progress = job.progress.lock();
            assert!(
                progress.error.is_none(),
                "{codec:?} {engine:?}: {:?}",
                progress.error
            );
            assert_eq!(progress.frame, 3);
            if engine.is_fframes() {
                assert!(
                    progress.renderer.contains("hybrid RGBA16F"),
                    "{}",
                    progress.renderer
                );
                assert!(
                    progress
                        .renderer
                        .contains(if engine == RenderEngine::FframesCpu {
                            "Skia CPU RGBA16F"
                        } else {
                            "Skia GPU"
                        }),
                    "{}",
                    progress.renderer
                );
            }
            let stream = op_media::probe(&path).unwrap().video.unwrap();
            assert_eq!(stream.rate, Rate::new(30000, 1001));
            let mut decoder = op_media::VideoDecoder::open(
                &path,
                Some(stream.index),
                Some(stream.rate),
                stream.color,
                false,
            )
            .unwrap();
            let pixels = decoder.frame(2).unwrap().to_rgba8();
            if let Some(reference) = &expected {
                let reference: &Vec<u8> = reference;
                assert!(
                    pixels
                        .iter()
                        .zip(reference)
                        .all(|(a, b)| a.abs_diff(*b) <= 12),
                    "{codec:?} {engine:?} output differs"
                );
            } else {
                expected = Some(pixels);
            }
        }
    }
}

#[test]
fn every_catalog_effect_and_transition_matches_native_pixels() {
    for engine in engines() {
        let gpu = Gpu::headless().unwrap();
        let mut native = Renderer::new(gpu.clone());
        let mut processor = Renderer::new(gpu);
        let mut worker =
            FframesRenderer::start_hybrid(engine, 64, 36, &AtomicBool::new(false)).unwrap();
        let (base, sid) = project();
        for def in catalog::CATALOG.iter().filter(|d| {
            matches!(
                d.kind,
                catalog::EffectKind::VideoEffect | catalog::EffectKind::Graphic
            )
        }) {
            let mut p = base.clone();
            let comp = Component::new(def, &mut p.ids);
            Arc::make_mut(&mut p.sequence_mut(sid).unwrap().video[0]).clips[0]
                .components
                .push(comp);
            compare(
                &p,
                sid,
                SeqTime::from_seconds(0.4),
                &mut native,
                &mut processor,
                &mut worker,
                def.id,
            );
        }
        for def in catalog::by_kind(catalog::EffectKind::VideoTransition) {
            let mut p = base.clone();
            let item = p.add_item(
                p.root,
                "Blue",
                ItemKind::Synthetic {
                    generator: Generator::ColorMatte {
                        color: Rgba::new(0.1, 0.2, 0.9, 0.7),
                    },
                    duration: Dur::from_seconds(2.0),
                },
            );
            let cid = ClipId(p.ids.next_raw());
            let tid = TransitionId(p.ids.next_raw());
            let params = Component::new(def, &mut p.ids).params.to_vec();
            let track = Arc::make_mut(&mut p.sequence_mut(sid).unwrap().video[0]);
            track.clips[0].duration = Dur::from_seconds(1.0);
            let mut incoming = track.clips[0].clone();
            incoming.id = cid;
            incoming.start = SeqTime::from_seconds(1.0);
            incoming.source = ClipSource::Generator { item };
            let from = track.clips[0].id;
            track.clips.push(incoming);
            track.transitions.push(Transition {
                id: tid,
                effect: def.id.into(),
                cut: SeqTime::from_seconds(1.0),
                duration: Dur::from_seconds(1.0),
                alignment: Alignment::CenterAtCut,
                from: Some(from),
                to: Some(cid),
                params,
            });
            for time in [0.6, 1.0, 1.4] {
                compare(
                    &p,
                    sid,
                    SeqTime::from_seconds(time),
                    &mut native,
                    &mut processor,
                    &mut worker,
                    def.id,
                );
            }
        }
    }
}

#[test]
fn separate_tracks_all_blends_and_linear_checkpoints_match_native() {
    for engine in engines() {
        let gpu = Gpu::headless().unwrap();
        let mut native = Renderer::new(gpu.clone());
        let mut processor = Renderer::new(gpu);
        let mut worker =
            FframesRenderer::start_hybrid(engine, 64, 36, &AtomicBool::new(false)).unwrap();
        let (mut p, sid) = project();
        let item = p.add_item(
            p.root,
            "Upper",
            ItemKind::Synthetic {
                generator: Generator::ColorMatte {
                    color: Rgba::new(0.8, 0.15, 0.4, 0.65),
                },
                duration: Dur::from_seconds(2.0),
            },
        );
        let mut top = (*p.sequence(sid).unwrap().video[0]).clone();
        top.id = TrackId(p.ids.next_raw());
        top.clips[0].id = ClipId(p.ids.next_raw());
        top.clips[0].source = ClipSource::Generator { item };
        p.sequence_mut(sid).unwrap().video.push(Arc::new(top));
        for mode in 0..catalog::BLEND_MODES.len() as u32 {
            Arc::make_mut(p.sequence_mut(sid).unwrap().video.last_mut().unwrap()).clips[0]
                .component_mut(catalog::OPACITY)
                .unwrap()
                .param_mut("blend_mode")
                .unwrap()
                .value = Value::Choice(mode);
            let count = compare(
                &p,
                sid,
                SeqTime::ZERO,
                &mut native,
                &mut processor,
                &mut worker,
                catalog::BLEND_MODES[mode as usize],
            );
            assert_eq!(count, if mode == 0 { 2 } else { 1 });
        }
        p.sequence_mut(sid).unwrap().settings.linear_compositing = true;
        compare(
            &p,
            sid,
            SeqTime::ZERO,
            &mut native,
            &mut processor,
            &mut worker,
            "linear checkpoint",
        );
    }
}

#[test]
fn animated_graphics_adjustment_stacks_and_async_preview_keep_their_pixels() {
    use op_application::media::PreviewFrames;
    use op_application::render_preview::{FframesPreview, PreviewRequest};
    use std::time::{Duration, Instant};
    for engine in engines() {
        let gpu = Gpu::headless().unwrap();
        let mut native = Renderer::new(gpu.clone());
        let mut processor = Renderer::new(gpu.clone());
        let mut worker =
            FframesRenderer::start_hybrid(engine, 64, 36, &AtomicBool::new(false)).unwrap();
        let (mut p, sid) = project();
        let mut top = (*p.sequence(sid).unwrap().video[0]).clone();
        top.id = TrackId(p.ids.next_raw());
        top.clips[0].id = ClipId(p.ids.next_raw());
        let mut caption = Component::new(catalog::find(catalog::CAPTION).unwrap(), &mut p.ids);
        caption.param_mut("text").unwrap().value = Value::Text("Shared caption".into());
        caption.param_mut("font_size").unwrap().value = Value::Float(12.0);
        let mut shape = Component::new(catalog::find(catalog::SHAPE).unwrap(), &mut p.ids);
        shape.param_mut("feather").unwrap().value = Value::Float(5.0);
        top.clips[0].components.extend([shape, caption]);
        p.sequence_mut(sid).unwrap().video.push(Arc::new(top));
        for style in 0..catalog::CAPTION_STYLES.len() as u32 {
            Arc::make_mut(p.sequence_mut(sid).unwrap().video.last_mut().unwrap()).clips[0]
                .component_mut(catalog::CAPTION)
                .unwrap()
                .param_mut("style")
                .unwrap()
                .value = Value::Choice(style);
            for t in [0.2, 0.7] {
                assert_eq!(
                    compare(
                        &p,
                        sid,
                        SeqTime::from_seconds(t),
                        &mut native,
                        &mut processor,
                        &mut worker,
                        "caption with feather"
                    ),
                    2
                );
            }
        }
        let adjustment = p.add_item(
            p.root,
            "Adjustment",
            ItemKind::Synthetic {
                generator: Generator::AdjustmentLayer,
                duration: Dur::from_seconds(2.0),
            },
        );
        let mut track = (*p.sequence(sid).unwrap().video[0]).clone();
        track.id = TrackId(p.ids.next_raw());
        track.clips[0].id = ClipId(p.ids.next_raw());
        track.clips[0].source = ClipSource::Generator { item: adjustment };
        let mut vignette = Component::new(catalog::find("op.video.vignette").unwrap(), &mut p.ids);
        vignette.param_mut("amount").unwrap().value = Value::Float(80.0);
        track.clips[0].components.push(vignette);
        p.sequence_mut(sid).unwrap().video.push(Arc::new(track));
        assert_eq!(
            compare(
                &p,
                sid,
                SeqTime::from_seconds(0.7),
                &mut native,
                &mut processor,
                &mut worker,
                "adjustment over caption"
            ),
            1
        );

        let dir = tempfile::tempdir().unwrap();
        let service = op_application::MediaService::new(dir.path().join("cache"), 64 << 20);
        let preview = FframesPreview::new(gpu);
        let snapshot = Arc::new(p);
        for ticket in 1..=8 {
            preview.request(PreviewRequest {
                key: ticket,
                project: snapshot.clone(),
                sequence: sid,
                time: SeqTime::from_seconds(0.7),
                scale: 1.0,
                engine,
                frames: PreviewFrames {
                    service: service.clone(),
                    wait: Duration::ZERO,
                    ahead: 0,
                },
            });
        }
        let since = Instant::now();
        let result = loop {
            assert!(since.elapsed() < Duration::from_secs(30));
            if let Some(result) = preview.take()
                && result.key == 8
            {
                break result;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!((result.width, result.height, result.missing), (64, 36, 0));
        let frame = native
            .upload_working(64, 36, &result.pixels.unwrap())
            .unwrap();
        let actual = native.rgba8(&frame).unwrap();
        native.recycle(frame);
        let frame = native.render(&Request {
            project: &snapshot,
            sequence: sid,
            time: SeqTime::from_seconds(0.7),
            scale: 1.0,
            source: &NoFrames,
        });
        let expected = native.rgba8(&frame).unwrap();
        native.recycle(frame);
        assert!(actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2));
        let stop = Instant::now();
        drop(preview);
        assert!(
            stop.elapsed() < Duration::from_millis(100),
            "preview shutdown blocked UI"
        );
    }
}
