//! Partial, read-only `.sqpreset` adapter. Only independently implemented sequence settings
//! are read; preview codecs, editing modes, names and proprietary resources are not imported.

use op_core::{Rate, SequenceSettings};

use crate::{ProjectError, prproj::PREMIERE_TICKS_PER_SECOND, xml};

pub fn import(text: &str) -> Result<SequenceSettings, ProjectError> {
    if text.len() > 4 << 20 {
        return Err(ProjectError::Format("sequence preset exceeds 4 MiB".into()));
    }
    let doc = xml::parse(text.trim_start_matches('\u{feff}'), xml::Limits::default())
        .map_err(|e| ProjectError::Format(e.to_string()))?;
    if doc.name(0) != "PremiereData" {
        return Err(ProjectError::Format("expected PremiereData".into()));
    }
    let definitions: Vec<_> = doc
        .children(0)
        .filter(|n| doc.name(*n) == "SequencePreset" && doc.attr(*n, "ObjectID").is_some())
        .collect();
    if definitions.len() != 1 {
        return Err(ProjectError::Format(
            "expected exactly one SequencePreset definition".into(),
        ));
    }
    let n = definitions[0];
    let field = |name| {
        doc.text_at(n, name)
            .ok_or_else(|| ProjectError::Format(format!("missing {name}")))
    };
    let invalid = |name| ProjectError::Format(format!("invalid or unsupported {name}"));
    let rect: Vec<i64> = field("VideoFrameSize")?
        .split(',')
        .map(|v| v.trim().parse::<i64>())
        .collect::<Result<_, _>>()
        .map_err(|_| invalid("VideoFrameSize"))?;
    if rect.len() != 4
        || rect[0] != 0
        || rect[1] != 0
        || !(16..=16384).contains(&rect[2])
        || !(16..=16384).contains(&rect[3])
    {
        return Err(invalid("VideoFrameSize"));
    }
    let positive = |name| {
        field(name)?
            .parse::<i64>()
            .ok()
            .filter(|v| *v > 0)
            .ok_or_else(|| invalid(name))
    };
    let ticks = positive("VideoFrameRate")?;
    let gcd = |mut a: i64, mut b: i64| {
        while b != 0 {
            (a, b) = (b, a % b);
        }
        a
    };
    let g = gcd(PREMIERE_TICKS_PER_SECOND, ticks);
    let (num, den) = (PREMIERE_TICKS_PER_SECOND / g, ticks / g);
    let fps = num as f64 / den as f64;
    if !(1.0..=240.0).contains(&fps) || num > u32::MAX as i64 || den > u32::MAX as i64 {
        return Err(invalid("VideoFrameRate"));
    }
    let audio_ticks = positive("AudioFrameRate")?;
    let sample_rate = PREMIERE_TICKS_PER_SECOND / audio_ticks;
    if PREMIERE_TICKS_PER_SECOND % audio_ticks != 0 || !(8000..=192000).contains(&sample_rate) {
        return Err(invalid("AudioFrameRate"));
    }
    let par: Vec<u32> = field("VideoPixelAspectRatio")?
        .split(',')
        .map(|v| v.trim().parse::<u32>())
        .collect::<Result<_, _>>()
        .map_err(|_| invalid("VideoPixelAspectRatio"))?;
    if par.len() != 2 || par.contains(&0) || par.iter().any(|v| *v > 10000) {
        return Err(invalid("VideoPixelAspectRatio"));
    }
    // Our sequence model currently renders progressive video only.
    if field("VideoFieldType")? != "0" {
        return Err(invalid("VideoFieldType (progressive only)"));
    }
    let mut settings = SequenceSettings {
        width: rect[2] as u32,
        height: rect[3] as u32,
        rate: Rate::new(num as u32, den as u32),
        audio_rate: sample_rate as u32,
        pixel_aspect: (par[0], par[1]),
        ..Default::default()
    };
    if let Some(count) = doc.text_at(n, "InitialNumberOfVideoTracks") {
        settings.video_tracks = count
            .parse::<usize>()
            .ok()
            .filter(|v| (1..=99).contains(v))
            .ok_or_else(|| invalid("InitialNumberOfVideoTracks"))?;
    }
    if let Some(text) = doc.text_at(n, "AudioTracks") {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|_| invalid("AudioTracks"))?;
        let tracks = value
            .as_array()
            .or_else(|| {
                value
                    .get("SerializerWrappedObject")
                    .and_then(|v| v.as_array())
            })
            .ok_or_else(|| invalid("AudioTracks"))?;
        if !(1..=99).contains(&tracks.len()) || !tracks.iter().all(|v| v.is_object()) {
            return Err(invalid("AudioTracks"));
        }
        settings.audio_tracks = tracks.len();
    }
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(audio: &str) -> String {
        format!(
            r#"<PremiereData Version="3"><SequencePreset ObjectRef="1"/><SequencePreset ObjectID="1" Version="8"><VideoFrameSize>0,0,1280,720</VideoFrameSize><VideoFrameRate>10594584000</VideoFrameRate><VideoPixelAspectRatio>1,1</VideoPixelAspectRatio><VideoFieldType>0</VideoFieldType><AudioFrameRate>5292000</AudioFrameRate><InitialNumberOfVideoTracks>2</InitialNumberOfVideoTracks><AudioTracks>{audio}</AudioTracks></SequencePreset></PremiereData>"#
        )
    }

    #[test]
    fn imports_both_audio_track_shapes_and_exact_fractional_rate() {
        for audio in [
            r#"[{"mName":""},{"mName":""}]"#,
            r#"{"SerializerWrappedObject":[{},{}]}"#,
        ] {
            let s = import(&fixture(audio)).unwrap();
            assert_eq!(s.rate, Rate::FPS_23_976);
            assert_eq!((s.width, s.height, s.audio_rate), (1280, 720, 48000));
            assert_eq!((s.video_tracks, s.audio_tracks), (2, 2));
        }
    }

    #[test]
    fn refuses_malformed_rates_interlacing_and_excessive_track_counts() {
        let text = fixture("[{}]");
        for bad in [
            text.replace("10594584000", "84SQ667200"),
            text.replace("<VideoFieldType>0", "<VideoFieldType>1"),
            text.replace(
                "<InitialNumberOfVideoTracks>2",
                "<InitialNumberOfVideoTracks>100",
            ),
            text.replace("5292000", "0"),
            text.replace("0,0,1280,720", "0,0,0,720"),
        ] {
            assert!(import(&bad).is_err());
        }
    }
}
