//! Mixer tests with generated audio.

use std::collections::HashMap;
use std::sync::Arc;

use op_audio::*;
use op_core::catalog;
use op_core::*;
use op_media::ConformedAudio;
use op_timeline::*;

#[derive(Clone)]
struct Source(HashMap<AssetId, Arc<ConformedAudio>>);

impl AudioSource for Source {
    fn audio(&self, asset: AssetId, _stream: usize) -> Option<Arc<ConformedAudio>> {
        self.0.get(&asset).cloned()
    }
}

fn tone_asset(
    p: &mut Project,
    dir: &std::path::Path,
    name: &str,
    freq: f64,
    amp: f32,
) -> (AssetId, ItemId, Arc<ConformedAudio>) {
    let rate = 48000;
    let n = rate * 10;
    let samples: Vec<f32> = (0..n)
        .flat_map(|i| {
            let v = (i as f64 * freq * std::f64::consts::TAU / rate as f64).sin() as f32 * amp;
            [v, v]
        })
        .collect();
    let path = dir.join(format!("{name}.f32"));
    op_media::audio::write_cache(&path, rate as u32, 2, &samples).unwrap();
    let asset = MediaAsset {
        id: AssetId(0),
        path: path.to_string_lossy().into_owned(),
        proxy: None,
        kind: MediaKind::Audio,
        video: None,
        audio: vec![AudioStream {
            index: 0,
            codec: "pcm".into(),
            sample_rate: 48000,
            layout: ChannelLayout::Stereo,
            samples: n as i64,
            start: Dur::ZERO,
        }],
        duration: Dur::from_seconds(10.0),
        interpretation: Interpretation::default(),
        file_size: 0,
        modified_unix: 0,
    };
    let (id, item) = p.add_asset(p.root, asset);
    (id, item, Arc::new(ConformedAudio::open(&path).unwrap()))
}

fn rms(v: &[f32]) -> f64 {
    (v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / v.len().max(1) as f64).sqrt()
}

#[test]
fn tape_stop_changes_speed_and_pitch_and_is_seek_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = Project::new("tape");
    let (asset, item, audio) = tone_asset(&mut p, dir.path(), "tone", 1000.0, 0.5);
    let source = Source(HashMap::from([(asset, audio)]));
    let (sid, _) = p.add_sequence(p.root, "S", SequenceSettings::default());
    let spec = SourceClip::from_item(&p, item).unwrap();
    let (mut p, clips) = p
        .transact(|p| {
            overwrite(
                p,
                sid,
                &spec,
                SeqTime::ZERO,
                &Patch {
                    video: None,
                    audio: vec![Some(0)],
                },
                EditOptions::default(),
            )
        })
        .unwrap();
    let mut effect = Component::new(catalog::find("op.audio.tape_stop").unwrap(), &mut p.ids);
    effect.param_mut("duration").unwrap().value = Value::Float(2.0);
    p.sequence_mut(sid)
        .unwrap()
        .clip_mut(clips[0])
        .unwrap()
        .components
        .push(effect);
    let mut mixer = Mixer::new(48000, 2);
    let mut first = vec![0.0; 4800 * 2];
    mixer.render(
        &p,
        sid,
        SeqTime::from_seconds(0.95),
        1.0,
        &mut first,
        &source,
    );
    let left: Vec<_> = first.iter().step_by(2).copied().collect();
    assert!(
        (zero_crossings(&left) as i64 - 100).abs() <= 2,
        "half speed: {}",
        zero_crossings(&left)
    );
    assert!(rms(&first) > 0.34);
    let mut stopped = vec![0.0; first.len()];
    mixer.render(
        &p,
        sid,
        SeqTime::from_seconds(3.0),
        1.0,
        &mut stopped,
        &source,
    );
    assert!(
        rms(&stopped) < 1e-6,
        "stop must be silent, not a frozen DC sample"
    );
    let mut seek = vec![0.0; first.len()];
    mixer.render(
        &p,
        sid,
        SeqTime::from_seconds(0.95),
        1.0,
        &mut seek,
        &source,
    );
    assert_eq!(first, seek);
    p.sequence_mut(sid)
        .unwrap()
        .clip_mut(clips[0])
        .unwrap()
        .component_mut("op.audio.tape_stop")
        .unwrap()
        .enabled = false;
    mixer.render(
        &p,
        sid,
        SeqTime::from_seconds(0.95),
        1.0,
        &mut seek,
        &source,
    );
    let left: Vec<_> = seek.iter().step_by(2).copied().collect();
    assert!((zero_crossings(&left) as i64 - 200).abs() <= 2);
}

fn zero_crossings(v: &[f32]) -> usize {
    v.windows(2)
        .filter(|w| (w[0] <= 0.0) != (w[1] <= 0.0))
        .count()
}

#[test]
fn levels_speed_and_crossfades() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = Project::new("audio");
    let (a, a_item, a_audio) = tone_asset(&mut p, dir.path(), "a", 1000.0, 0.5);
    let (b, b_item, b_audio) = tone_asset(&mut p, dir.path(), "b", 500.0, 0.5);
    let src = Source(HashMap::from([(a, a_audio), (b, b_audio)]));
    let (seq, _) = p.add_sequence(p.root, "S", SequenceSettings::default());
    let put = |p: &mut Project, item: ItemId, from: f64, to: f64, at: f64| {
        let spec = SourceClip::from_asset(
            p,
            item,
            SrcRange::new(SrcTime::from_seconds(from), SrcTime::from_seconds(to)),
        )
        .unwrap();
        let (next, ids) = p
            .transact(|p| {
                overwrite(
                    p,
                    seq,
                    &spec,
                    SeqTime::from_seconds(at),
                    &Patch {
                        video: None,
                        audio: vec![Some(0)],
                    },
                    EditOptions::default(),
                )
            })
            .unwrap();
        *p = next;
        ids[0]
    };
    let clip_a = put(&mut p, a_item, 0.0, 4.0, 0.0);
    put(&mut p, b_item, 0.0, 4.0, 4.0);

    let mut m = Mixer::new(48000, 2);
    let mut buf = vec![0f32; 4800 * 2];
    // unity: sine of amplitude 0.5 has RMS 0.354
    m.render(&p, seq, SeqTime::from_seconds(1.0), 1.0, &mut buf, &src);
    assert!((rms(&buf) - 0.3536).abs() < 0.005, "{}", rms(&buf));
    let left: Vec<f32> = buf.iter().step_by(2).copied().collect();
    assert!((zero_crossings(&left) as i64 - 200).abs() <= 2);

    // clip volume -6.02 dB halves the amplitude
    p.sequence_mut(seq)
        .unwrap()
        .clip_mut(clip_a)
        .unwrap()
        .component_mut(catalog::VOLUME)
        .unwrap()
        .param_mut("level")
        .unwrap()
        .value = Value::Float(-6.0206);
    m.render(&p, seq, SeqTime::from_seconds(1.0), 1.0, &mut buf, &src);
    assert!((rms(&buf) - 0.1768).abs() < 0.004, "{}", rms(&buf));

    // double speed doubles the frequency
    let (next, _) = p
        .transact(|p| {
            set_speed(
                p,
                seq,
                &[clip_a],
                Speed::new(2, 1),
                false,
                false,
                EditOptions::default(),
            )
        })
        .unwrap();
    p = next;
    m.render(&p, seq, SeqTime::from_seconds(0.5), 1.0, &mut buf, &src);
    let left: Vec<f32> = buf.iter().step_by(2).copied().collect();
    assert!(
        (zero_crossings(&left) as i64 - 400).abs() <= 4,
        "{}",
        zero_crossings(&left)
    );

    // track mute silences, master volume scales
    p.sequence_mut(seq)
        .unwrap()
        .track_mut(TrackRef::audio(0))
        .unwrap()
        .muted = true;
    m.render(&p, seq, SeqTime::from_seconds(0.5), 1.0, &mut buf, &src);
    assert!(rms(&buf) < 1e-6);
}

#[test]
fn constant_power_crossfade_keeps_level() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = Project::new("audio");
    // the same tone on both sides: a constant-power crossfade of uncorrelated material keeps
    // power; of identical material it peaks at +3 dB mid-way
    let (a, a_item, a_audio) = tone_asset(&mut p, dir.path(), "a", 1000.0, 0.5);
    let src = Source(HashMap::from([(a, a_audio)]));
    let (seq, _) = p.add_sequence(p.root, "S", SequenceSettings::default());
    for (from, at) in [(0.0, 0.0), (4.0, 2.0)] {
        let spec = SourceClip::from_asset(
            &p,
            a_item,
            SrcRange::new(
                SrcTime::from_seconds(from),
                SrcTime::from_seconds(from + 2.0),
            ),
        )
        .unwrap();
        let (next, _) = p
            .transact(|p| {
                overwrite(
                    p,
                    seq,
                    &spec,
                    SeqTime::from_seconds(at),
                    &Patch {
                        video: None,
                        audio: vec![Some(0)],
                    },
                    EditOptions::default(),
                )
            })
            .unwrap();
        p = next;
    }
    let (next, _) = p
        .transact(|p| {
            add_transition(
                p,
                seq,
                TrackRef::audio(0),
                SeqTime::from_seconds(2.0),
                catalog::CONSTANT_POWER,
                Dur::from_seconds(1.0),
                Some(Alignment::CenterAtCut),
                EditOptions::default(),
            )
        })
        .unwrap();
    p = next;
    let mut m = Mixer::new(48000, 2);
    let mut buf = vec![0f32; 480 * 2];
    // before the transition: unity
    m.render(&p, seq, SeqTime::from_seconds(1.0), 1.0, &mut buf, &src);
    assert!((rms(&buf) - 0.3536).abs() < 0.01);
    // well inside the incoming clip: unity again
    m.render(&p, seq, SeqTime::from_seconds(3.0), 1.0, &mut buf, &src);
    assert!((rms(&buf) - 0.3536).abs() < 0.01);
    // during the fade the level stays within the constant-power envelope for in-phase material
    m.render(&p, seq, SeqTime::from_seconds(2.0), 1.0, &mut buf, &src);
    let level = rms(&buf);
    assert!(level > 0.3 && level < 0.51, "{level}");
}

#[test]
fn offline_render_matches_length() {
    let p = {
        let mut p = Project::new("x");
        p.add_sequence(p.root, "S", SequenceSettings::default());
        p
    };
    let seq = *p.sequences.keys().next().unwrap();
    let mut total = 0;
    render_range(
        &p,
        seq,
        SeqRange::new(SeqTime::ZERO, SeqTime::from_seconds(1.5)),
        48000,
        2,
        &NoAudio,
        |b| {
            total += b.len();
            true
        },
    );
    assert_eq!(total, 72000 * 2);
}

fn impulse_channels(channels: usize) -> Vec<f32> {
    let mut samples = vec![0.0; 3072 * channels];
    for channel in 0..channels {
        samples[(32 + channel * 16) * channels + channel] = (channel + 1) as f32 * 0.1;
    }
    samples
}

fn channel_fixture(
    master: MasterLayout,
    channels: usize,
    samples: &[f32],
) -> (tempfile::TempDir, Project, SequenceId, ClipId, Source) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("original-channel-impulses.pcm");
    op_media::audio::write_cache(&path, 48000, channels as u16, samples).unwrap();
    let mut p = Project::new("channel routing");
    let frames = samples.len() / channels;
    let duration = Rate::fps(48000).frames_to_dur(frames as i64);
    let (asset, item) = p.add_asset(
        p.root,
        MediaAsset {
            id: AssetId(0),
            path: path.to_string_lossy().into_owned(),
            proxy: None,
            kind: MediaKind::Audio,
            video: None,
            audio: vec![AudioStream {
                index: 0,
                codec: "original-f32-fixture".into(),
                sample_rate: 48000,
                layout: ChannelLayout::from_count(channels as u16),
                samples: frames as i64,
                start: Dur::ZERO,
            }],
            duration,
            interpretation: Interpretation::default(),
            file_size: 0,
            modified_unix: 0,
        },
    );
    let source = Source(HashMap::from([(
        asset,
        Arc::new(ConformedAudio::open(&path).unwrap()),
    )]));
    let (sid, _) = p.add_sequence(
        p.root,
        "sequence",
        SequenceSettings {
            master,
            video_tracks: 0,
            audio_tracks: 1,
            ..SequenceSettings::default()
        },
    );
    let spec = SourceClip::from_item(&p, item).unwrap();
    let (p, ids) = p
        .transact(|p| {
            overwrite(
                p,
                sid,
                &spec,
                SeqTime::ZERO,
                &Patch {
                    video: None,
                    audio: vec![Some(0)],
                },
                EditOptions::default(),
            )
        })
        .unwrap();
    (dir, p, sid, ids[0], source)
}

fn mix(p: &Project, sid: SequenceId, source: &Source, channels: usize, frames: usize) -> Vec<f32> {
    let mut mixer = Mixer::new(48000, channels);
    let mut out = vec![0.0; frames * channels];
    mixer.render(p, sid, SeqTime::ZERO, 1.0, &mut out, source);
    out
}

#[test]
fn surround_impulses_preserve_every_channel_in_preview_export_and_master_meters() {
    let input = impulse_channels(6);
    let (_dir, p, sid, _, source) = channel_fixture(MasterLayout::Surround51, 6, &input);
    let original = p.clone();
    let mut mixer = Mixer::new(48000, 6);
    let mut preview = Vec::new();
    for start in (0..3072).step_by(512) {
        let mut block = vec![0.0; 512 * 6];
        mixer.render(
            &p,
            sid,
            SeqTime::ZERO + Rate::fps(48000).frames_to_dur(start),
            1.0,
            &mut block,
            &source,
        );
        preview.extend(block);
    }
    assert_eq!(preview.len(), input.len());
    for (index, (actual, expected)) in preview.iter().zip(&input).enumerate() {
        assert_eq!(actual, expected, "preview sample {index}");
    }
    let peaks = mixer.meters.take_master(6);
    for (channel, peak) in peaks.iter().enumerate() {
        assert!((*peak - (channel + 1) as f32 * 0.1).abs() < 1e-6);
    }
    let mut export = Vec::new();
    render_range(
        &p,
        sid,
        SeqRange::with_duration(SeqTime::ZERO, Rate::fps(48000).frames_to_dur(3072)),
        48000,
        6,
        &source,
        |block| {
            export.extend_from_slice(block);
            true
        },
    );
    assert_eq!(export.len(), preview.len());
    for (index, (actual, expected)) in export.iter().zip(&preview).enumerate() {
        assert_eq!(actual, expected, "export sample {index}");
    }
    assert_eq!(
        p, original,
        "rendering must not mutate source/channel metadata"
    );
}

#[test]
fn surround_stereo_and_mono_downmix_follow_the_explicit_contract() {
    let input = impulse_channels(6);
    let (_dir, p, sid, _, source) = channel_fixture(MasterLayout::Surround51, 6, &input);
    for (master, output_channels) in [
        (MasterLayout::Surround51, 2),
        (MasterLayout::Surround51, 1),
        (MasterLayout::Stereo, 2),
        (MasterLayout::Mono, 1),
    ] {
        let mut p = p.clone();
        p.sequence_mut(sid).unwrap().settings.master = master;
        let out = mix(&p, sid, &source, output_channels, 3072);
        for (frame, src) in input.as_chunks::<6>().0.iter().enumerate() {
            let l = src[0] + (src[2] + src[4]) * std::f32::consts::FRAC_1_SQRT_2;
            let r = src[1] + (src[2] + src[5]) * std::f32::consts::FRAC_1_SQRT_2;
            if output_channels == 1 {
                assert!((out[frame] - (l + r) * 0.5).abs() < 1e-6);
            } else {
                assert!((out[frame * 2] - l).abs() < 1e-6);
                assert!((out[frame * 2 + 1] - r).abs() < 1e-6);
            }
        }
    }
    // Existing mono/stereo sources feed the front pair of a surround master, not C/LFE.
    for channels in [1, 2] {
        let input = impulse_channels(channels);
        let (_dir, p, sid, _, source) = channel_fixture(MasterLayout::Surround51, channels, &input);
        let out = mix(&p, sid, &source, 6, 3072);
        for (src, dst) in input
            .chunks_exact(channels)
            .zip(out.as_chunks::<6>().0.iter())
        {
            let mono_pair = [src[0], src[0]];
            assert_eq!(&dst[..2], if channels == 1 { &mono_pair[..] } else { src });
            assert!(dst[2..].iter().all(|v| *v == 0.0));
        }
    }
}

#[test]
fn nested_sequences_render_their_own_layout_before_parent_conversion() {
    let input = impulse_channels(6);
    let (_dir, base, child, _, source) = channel_fixture(MasterLayout::Surround51, 6, &input);
    for (child_master, parent_master) in [
        (MasterLayout::Surround51, MasterLayout::Surround51),
        (MasterLayout::Surround51, MasterLayout::Stereo),
        (MasterLayout::Stereo, MasterLayout::Surround51),
        (MasterLayout::Mono, MasterLayout::Surround51),
    ] {
        let mut p = base.clone();
        p.sequence_mut(child).unwrap().settings.master = child_master;
        let item = p.sequence_item(child).unwrap();
        let (parent, _) = p.add_sequence(
            p.root,
            "parent",
            SequenceSettings {
                master: parent_master,
                video_tracks: 0,
                audio_tracks: 1,
                ..SequenceSettings::default()
            },
        );
        let spec = SourceClip::from_item(&p, item).unwrap();
        let (p, _) = p
            .transact(|p| {
                overwrite(
                    p,
                    parent,
                    &spec,
                    SeqTime::ZERO,
                    &Patch {
                        video: None,
                        audio: vec![Some(0)],
                    },
                    EditOptions::default(),
                )
            })
            .unwrap();
        let out = mix(&p, parent, &source, parent_master.channels(), 3072);
        for (frame, src) in input.as_chunks::<6>().0.iter().enumerate() {
            let l = src[0] + (src[2] + src[4]) * std::f32::consts::FRAC_1_SQRT_2;
            let r = src[1] + (src[2] + src[5]) * std::f32::consts::FRAC_1_SQRT_2;
            let expected = if child_master == MasterLayout::Mono {
                [(l + r) * 0.5; 2]
            } else {
                [l, r]
            };
            let dst =
                &out[frame * parent_master.channels()..(frame + 1) * parent_master.channels()];
            if child_master == MasterLayout::Surround51 && parent_master == MasterLayout::Surround51
            {
                assert_eq!(dst, src);
            } else {
                assert!((dst[0] - expected[0]).abs() < 1e-6);
                assert!((dst[1] - expected[1]).abs() < 1e-6);
                assert!(dst[2..].iter().all(|v| *v == 0.0));
            }
        }
    }
}

#[test]
fn surround_gains_front_pan_mute_and_resampling_do_not_lose_or_cross_channels() {
    let input = impulse_channels(6);
    let (_dir, mut p, sid, clip, source) = channel_fixture(MasterLayout::Surround51, 6, &input);
    let c = p.sequence_mut(sid).unwrap().clip_mut(clip).unwrap();
    c.gain_db = -6.020599913279624;
    c.component_mut(catalog::PANNER)
        .unwrap()
        .param_mut("balance")
        .unwrap()
        .value = Value::Float(100.0);
    let mut volume = Component::new(catalog::find(catalog::VOLUME).unwrap(), &mut p.ids);
    volume.param_mut("level").unwrap().value = Value::Float(-6.020599913279624);
    p.sequence_mut(sid)
        .unwrap()
        .track_mut(TrackRef::audio(0))
        .unwrap()
        .components
        .push(volume);
    p.sequence_mut(sid).unwrap().master[0]
        .param_mut("level")
        .unwrap()
        .value = Value::Float(-6.020599913279624);
    let out = mix(&p, sid, &source, 6, 3072);
    for (src, dst) in input
        .as_chunks::<6>()
        .0
        .iter()
        .zip(out.as_chunks::<6>().0.iter())
    {
        for channel in 0..6 {
            let expected = if channel == 0 {
                0.0
            } else {
                src[channel] * 0.125
            };
            assert!((dst[channel] - expected).abs() < 1e-6);
        }
    }
    p.sequence_mut(sid)
        .unwrap()
        .track_mut(TrackRef::audio(0))
        .unwrap()
        .muted = true;
    assert!(mix(&p, sid, &source, 6, 3072).iter().all(|v| *v == 0.0));

    let mut center = vec![0.0; 3072 * 6];
    center[32 * 6 + 2] = 0.5;
    let (_dir, p, sid, _, source) = channel_fixture(MasterLayout::Surround51, 6, &center);
    let mut mixer = Mixer::new(44100, 6);
    let mut resampled = vec![0.0; 512 * 6];
    mixer.render(&p, sid, SeqTime::ZERO, 1.0, &mut resampled, &source);
    assert!(
        resampled
            .as_chunks::<6>()
            .0
            .iter()
            .any(|frame| frame[2].abs() > 0.1)
    );
    assert!(
        resampled
            .as_chunks::<6>()
            .0
            .iter()
            .all(|frame| frame.iter().enumerate().all(|(c, v)| c == 2 || *v == 0.0))
    );
    let mut reverse = vec![0.0; 512 * 6];
    Mixer::new(48000, 6).render(
        &p,
        sid,
        SeqTime::ZERO + Rate::fps(48000).frames_to_dur(512),
        -1.0,
        &mut reverse,
        &source,
    );
    assert!(
        reverse
            .as_chunks::<6>()
            .0
            .iter()
            .any(|frame| frame[2] == 0.5)
    );
    assert!(
        reverse
            .as_chunks::<6>()
            .0
            .iter()
            .all(|frame| frame.iter().enumerate().all(|(c, v)| c == 2 || *v == 0.0))
    );
}

#[test]
fn surround_filter_processes_the_center_without_leaking_into_other_channels() {
    let mut input = vec![0.0; 3072 * 6];
    input[32 * 6 + 2] = 0.5;
    let (_dir, mut p, sid, clip, source) = channel_fixture(MasterLayout::Surround51, 6, &input);
    let mut effect = Component::new(catalog::find("op.audio.lowpass").unwrap(), &mut p.ids);
    effect.param_mut("cutoff").unwrap().value = Value::Float(1000.0);
    p.sequence_mut(sid)
        .unwrap()
        .clip_mut(clip)
        .unwrap()
        .components
        .push(effect);
    let out = mix(&p, sid, &source, 6, 3072);
    assert!(
        out.as_chunks::<6>()
            .0
            .iter()
            .skip(33)
            .take(100)
            .any(|frame| frame[2].abs() > 1e-4)
    );
    assert!(
        out.as_chunks::<6>()
            .0
            .iter()
            .all(|frame| frame.iter().enumerate().all(|(c, v)| c == 2 || *v == 0.0))
    );
    assert!(out.iter().all(|v| v.is_finite()));
}

#[test]
fn live_and_scrub_master_meters_preserve_surround_before_stereo_device_adaptation() {
    let input = impulse_channels(6);
    let (_dir, p, sid, _, source) = channel_fixture(MasterLayout::Surround51, 6, &input);
    let playback = Playback::silent(Arc::new(source));
    for scrub in [false, true] {
        let _ = playback.meters.take_master(8);
        if scrub {
            playback.blip(
                Arc::new(p.clone()),
                sid,
                SeqTime::ZERO,
                Dur::from_seconds(0.04),
            );
        } else {
            playback.play(Arc::new(p.clone()), sid, SeqTime::ZERO, 1.0);
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut peaks = [0.0f32; 6];
        while std::time::Instant::now() < deadline {
            for (peak, next) in peaks.iter_mut().zip(playback.meters.take_master(6)) {
                *peak = peak.max(next);
            }
            if peaks.iter().all(|v| *v > 0.0) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        playback.stop();
        for (channel, peak) in peaks.iter().enumerate() {
            assert!(
                (*peak - (channel + 1) as f32 * 0.1).abs() < 1e-6,
                "scrub {scrub}, channel {channel}: {peak}"
            );
        }
    }
}
