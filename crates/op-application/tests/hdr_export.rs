//! Real encoded/decoded files on native and installed fframes backends; invalid HDR settings
//! are rejected before opening an existing destination. Pixel equations are tested in op-render.
use op_application::render_engine::{FframesRenderer, RenderEngine, fframes_worker_path};
use op_application::{ExportJob, ExportSettings, MediaService};
use op_core::*;
use op_media::{
    DeliveryColor, Hdr10Metadata, PixelLayout, VideoCodec, VideoDecoder, VideoSettings,
};
use op_render::Gpu;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn scene() -> (Project, SequenceId) {
    let mut p = Project::new("HDR white");
    let (sid, _) = p.add_sequence(
        p.root,
        "S",
        SequenceSettings {
            width: 320,
            height: 180,
            ..Default::default()
        },
    );
    let item = p.add_item(
        p.root,
        "White",
        ItemKind::Synthetic {
            generator: Generator::ColorMatte {
                color: Rgba {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                },
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
fn video(codec: VideoCodec, color: DeliveryColor, hardware: bool) -> VideoSettings {
    VideoSettings {
        color,
        hdr10: Hdr10Metadata::default(),
        codec,
        width: 320,
        height: 180,
        rate: Rate::FPS_25,
        bitrate_kbps: None,
        quality: 0,
        hardware,
    }
}
fn wait(job: &ExportJob) {
    let started = Instant::now();
    while !job.finished() {
        assert!(
            started.elapsed() < Duration::from_secs(90),
            "HDR export stalled"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn hdr_files_keep_transfer_primaries_ten_bit_pixels_and_fframes_precision() {
    let gpu = match Gpu::headless() {
        Ok(g) => g,
        Err(e) if std::env::var("OPENPREMIER_REQUIRE_GPU").as_deref() == Ok("1") => panic!("{e}"),
        Err(_) => return,
    };
    let (p, sid) = scene();
    let p = Arc::new(p);
    let dir = tempfile::tempdir().unwrap();
    let media = MediaService::new(dir.path().join("cache"), 32 << 20);
    let mut engines = vec![RenderEngine::Wgpu];
    if fframes_worker_path().is_some() {
        engines.push(RenderEngine::FframesCpu);
        match FframesRenderer::start_hybrid(
            RenderEngine::FframesGpu,
            320,
            180,
            &std::sync::atomic::AtomicBool::new(false),
        ) {
            Ok(_) => engines.push(RenderEngine::FframesGpu),
            Err(e) if std::env::var("OPENPREMIER_REQUIRE_FFRAMES_GPU").as_deref() == Ok("1") => {
                panic!("{e}")
            }
            Err(_) => (),
        }
    } else {
        assert_ne!(
            std::env::var("OPENPREMIER_REQUIRE_FFRAMES").as_deref(),
            Ok("1")
        );
    }
    for color in [DeliveryColor::Pq, DeliveryColor::Hlg] {
        for codec in [
            VideoCodec::Hevc,
            VideoCodec::ProRes422Hq,
            VideoCodec::ProRes4444,
        ] {
            for engine in &engines {
                for hardware in if codec == VideoCodec::Hevc {
                    vec![false, true]
                } else {
                    vec![false]
                } {
                    let path = dir.path().join(format!(
                        "{color:?}-{codec:?}-{engine:?}-{hardware}.{}",
                        codec.extension()
                    ));
                    let settings = ExportSettings {
                        engine: *engine,
                        path: path.clone(),
                        sequence: sid,
                        range: SeqRange::new(SeqTime::ZERO, SeqTime::from_seconds(0.12)),
                        video: Some(video(codec, color, hardware)),
                        audio: None,
                    };
                    let job = ExportJob::start(p.clone(), settings, gpu.clone(), media.clone());
                    wait(&job);
                    let progress = job.progress.lock().clone();
                    assert!(
                        progress.error.is_none(),
                        "{engine:?}/{codec:?}/{color:?}: {:?}",
                        progress.error
                    );
                    eprintln!(
                        "HDR {engine:?} {color:?} {codec:?} hardware={hardware}: {} / {}",
                        progress.encoder, progress.renderer
                    );
                    if codec == VideoCodec::Hevc
                        && hardware
                        && std::env::var("OPENPREMIER_REQUIRE_NVENC").as_deref() == Ok("1")
                    {
                        assert_eq!(progress.encoder, "hevc_nvenc");
                    }
                    if engine.is_fframes() {
                        assert!(
                            progress.renderer.contains("hybrid RGBA16F"),
                            "{}",
                            progress.renderer
                        );
                    }
                    let asset = op_media::probe(&path).unwrap();
                    let v = asset.video.as_ref().unwrap();
                    assert_eq!(v.color.primaries, Primaries::Bt2020);
                    assert_eq!(v.color.matrix, ColorMatrix::Bt2020);
                    assert_eq!(
                        v.color.transfer,
                        if color == DeliveryColor::Pq {
                            Transfer::Pq
                        } else {
                            Transfer::Hlg
                        }
                    );
                    let mut decoder =
                        VideoDecoder::open(&path, None, None, v.color, false).unwrap();
                    for i in 0..3 {
                        let frame = decoder.frame(i).unwrap();
                        assert!(
                            matches!(frame.layout, PixelLayout::Yuv16 { bits: 10..=16, .. }),
                            "{:?}",
                            frame.layout
                        );
                        let y =
                            u16::from_le_bytes([frame.planes[0].data[0], frame.planes[0].data[1]]);
                        let expected = if color == DeliveryColor::Pq { 573 } else { 721 };
                        let shift = match frame.layout {
                            PixelLayout::Yuv16 { bits, .. } => bits - 10,
                            _ => 0,
                        };
                        assert!(
                            (y >> shift).abs_diff(expected) <= 4,
                            "{engine:?}/{codec:?}/{color:?} decoded Y={} ({})",
                            y >> shift,
                            progress.encoder
                        );
                    }
                    // Where ffprobe is installed, also check container/SEI mastering metadata.
                    let ffprobe = std::env::var_os("OPENPREMIER_FFPROBE")
                        .map(std::path::PathBuf::from)
                        .or_else(|| {
                            let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                                .join("../../third_party/ffmpeg/bin/ffprobe.exe");
                            p.exists().then_some(p)
                        });
                    if let Some(ffprobe) = ffprobe {
                        let output = std::process::Command::new(ffprobe)
                            .args([
                                "-v",
                                "error",
                                "-select_streams",
                                "v:0",
                                "-read_intervals",
                                "%+#1",
                                "-show_streams",
                                "-show_frames",
                                "-of",
                                "json",
                            ])
                            .arg(&path)
                            .output()
                            .unwrap();
                        assert!(output.status.success());
                        let json: serde_json::Value =
                            serde_json::from_slice(&output.stdout).unwrap();
                        let side: Vec<_> = json["streams"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .chain(json["frames"].as_array().unwrap())
                            .flat_map(|x| x["side_data_list"].as_array().into_iter().flatten())
                            .collect();
                        if color == DeliveryColor::Pq {
                            assert!(
                                side.iter()
                                    .any(|x| x["side_data_type"] == "Mastering display metadata"),
                                "missing MDCV: {json}"
                            );
                            assert!(
                                side.iter()
                                    .any(|x| x["max_content"] == 1000 && x["max_average"] == 400),
                                "missing CLL/FALL: {json}"
                            );
                        } else {
                            assert!(
                                !side
                                    .iter()
                                    .any(|x| x["side_data_type"] == "Mastering display metadata")
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn unsupported_hdr_and_invalid_metadata_do_not_truncate_destinations() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keep.mp4");
    std::fs::write(&path, b"KEEP").unwrap();
    for codec in [VideoCodec::H264, VideoCodec::Png, VideoCodec::DnxhrHq] {
        assert!(
            op_media::Muxer::create(&path, Some(&video(codec, DeliveryColor::Pq, false)), None)
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"KEEP");
    }
    for metadata in [
        Hdr10Metadata {
            mastering_max_nits: f64::NAN,
            ..Default::default()
        },
        Hdr10Metadata {
            max_fall: 1001,
            ..Default::default()
        },
    ] {
        let mut v = video(VideoCodec::Hevc, DeliveryColor::Pq, false);
        v.hdr10 = metadata;
        assert!(op_media::Muxer::create(&path, Some(&v), None).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"KEEP");
    }
}

#[test]
fn decoded_hdr_sources_keep_thousand_nit_highlights_in_native_and_fframes_exports() {
    let gpu = match Gpu::headless() {
        Ok(g) => g,
        Err(e) if std::env::var("OPENPREMIER_REQUIRE_GPU").as_deref() == Ok("1") => panic!("{e}"),
        Err(_) => return,
    };
    let dir = tempfile::tempdir().unwrap();
    let media = MediaService::new(dir.path().join("cache"), 32 << 20);
    let mut engines = vec![RenderEngine::Wgpu];
    if fframes_worker_path().is_some() {
        engines.push(RenderEngine::FframesCpu);
        if FframesRenderer::start_hybrid(
            RenderEngine::FframesGpu,
            320,
            180,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .is_ok()
        {
            engines.push(RenderEngine::FframesGpu);
        } else {
            assert_ne!(
                std::env::var("OPENPREMIER_REQUIRE_FFRAMES_GPU").as_deref(),
                Ok("1")
            );
        }
    }
    for (color, expected) in [(DeliveryColor::Pq, 723u16), (DeliveryColor::Hlg, 940u16)] {
        let input = dir.path().join(format!("source-{color:?}.mp4"));
        let v = video(VideoCodec::Hevc, color, false);
        let y: Vec<u8> = (0..320 * 180)
            .flat_map(|_| expected.to_le_bytes())
            .collect();
        let c: Vec<u8> = (0..160 * 90).flat_map(|_| 512u16.to_le_bytes()).collect();
        let mut muxer = op_media::Muxer::create(&input, Some(&v), None).unwrap();
        for _ in 0..3 {
            muxer.push_video(&[&y, &c, &c]).unwrap();
        }
        muxer.finish().unwrap();
        let mut p = Project::new("HDR media");
        let (sid, _) = p.add_sequence(
            p.root,
            "HDR",
            SequenceSettings {
                width: 320,
                height: 180,
                rate: Rate::FPS_25,
                ..Default::default()
            },
        );
        let (_, item) = p.add_asset(p.root, op_media::probe(&input).unwrap());
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
        let p = Arc::new(p);
        for engine in &engines {
            let output = dir.path().join(format!("media-{color:?}-{engine:?}.mp4"));
            let job = ExportJob::start(
                p.clone(),
                ExportSettings {
                    engine: *engine,
                    path: output.clone(),
                    sequence: sid,
                    range: SeqRange::new(SeqTime::ZERO, SeqTime::from_seconds(0.12)),
                    video: Some(v.clone()),
                    audio: None,
                },
                gpu.clone(),
                media.clone(),
            );
            wait(&job);
            assert!(
                job.progress.lock().error.is_none(),
                "{:?}",
                job.progress.lock().error
            );
            let asset = op_media::probe(&output).unwrap();
            let mut decoder = VideoDecoder::open(
                &output,
                None,
                None,
                asset.video.as_ref().unwrap().color,
                false,
            )
            .unwrap();
            let frame = decoder.frame(1).unwrap();
            let actual = u16::from_le_bytes([frame.planes[0].data[0], frame.planes[0].data[1]]);
            assert!(
                actual.abs_diff(expected) <= 4,
                "{color:?}/{engine:?}: {actual} vs {expected}; HDR source was clipped or transfer was applied incorrectly"
            );
        }
    }
}
