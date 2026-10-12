//! Native settings adapter for OpenJumpCuts' four detection controls.
//! The CEP/ExtendScript runtime is not required. Times are seconds, threshold is dBFS.

use serde::{Deserialize, Serialize};

use crate::{Method, Params};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub silence_cutoff: f32,
    pub remove_over: f64,
    pub keep_over: f64,
    pub padding: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            silence_cutoff: -80.0,
            remove_over: 1.0,
            keep_over: 0.3,
            padding: 0.5,
        }
    }
}

impl Settings {
    pub fn params(self) -> Result<Params, String> {
        let bounded = |v: f64, lo, hi| v.is_finite() && (lo..=hi).contains(&v);
        if !bounded(self.silence_cutoff as f64, -100.0, 0.0)
            || !bounded(self.remove_over, 0.01, 60.0)
            || !bounded(self.keep_over, 0.0, 60.0)
            || !bounded(self.padding, 0.0, 30.0)
        {
            return Err("Invalid OpenJumpCuts settings: cutoff -100..0 dBFS, silence 0.01..60 s, sound 0..60 s, padding 0..30 s.".into());
        }
        Ok(Params {
            method: Method::Loudness {
                threshold_db: self.silence_cutoff,
            },
            min_silence: self.remove_over,
            min_sound: self.keep_over,
            pad_before: self.padding,
            pad_after: self.padding,
            voice_only: false,
        })
    }

    /// The plugin's optional in/out/start metadata is ignored: scope belongs to our timeline.
    pub fn from_json(text: &str) -> Result<Params, String> {
        serde_json::from_str::<Self>(text)
            .map_err(|e| e.to_string())?
            .params()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Analysis, Range, detect};

    #[test]
    fn reads_seconds_and_db_without_scaling_or_timeline_metadata() {
        let p = Settings::from_json(r#"{"silenceCutoff":-45,"removeOver":0.5,"keepOver":0.3,"padding":0.1,"in":5,"out":8,"start":20}"#).unwrap();
        let mut db = vec![-10.0; 100];
        db.extend(vec![-90.0; 100]);
        db.extend(vec![-10.0; 10]); // a click: joined transitively into the pause
        db.extend(vec![-90.0; 100]);
        db.extend(vec![-10.0; 100]);
        let cuts = detect(
            &Analysis {
                db,
                ..Default::default()
            },
            &p,
        );
        assert_eq!(cuts.len(), 1);
        assert!((cuts[0].start - 1.1).abs() < 1e-6);
        assert!((cuts[0].end - 3.0).abs() < 1e-6);
    }

    #[test]
    fn edges_no_silence_and_padding_that_consumes_a_pause() {
        let p = Settings::default().params().unwrap();
        let a = |db| Analysis {
            db,
            ..Default::default()
        };
        assert!(detect(&a(vec![-10.0; 300]), &p).is_empty());
        assert!(detect(&a(vec![]), &p).is_empty());
        assert_eq!(
            detect(&a(vec![-100.0; 300]), &p),
            vec![Range {
                start: 0.0,
                end: 3.0
            }]
        );
        let mut db = vec![-10.0; 100];
        db.extend(vec![-100.0; 100]);
        db.extend(vec![-10.0; 100]);
        assert!(detect(&a(db), &p).is_empty());
    }

    #[test]
    fn rejects_missing_invalid_and_nonfinite_values() {
        assert!(Settings::from_json("{}").is_err());
        assert!(
            Settings::from_json(
                r#"{"silenceCutoff":-200,"removeOver":1,"keepOver":0.3,"padding":0.5}"#
            )
            .is_err()
        );
        assert!(
            Settings {
                padding: f64::NAN,
                ..Default::default()
            }
            .params()
            .is_err()
        );
        assert!(
            Settings {
                remove_over: -1.0,
                ..Default::default()
            }
            .params()
            .is_err()
        );
    }
}
