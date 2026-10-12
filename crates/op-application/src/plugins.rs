//! Bundled native extensions. They extend editor tools, not the project file or renderer.
use op_core::{Interp, Keyframe, Value};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NativePlugins {
    pub motion_curves: bool,
    pub folder_portals: bool,
    pub quick_actions: bool,
    pub folders: Vec<FolderPortal>,
    pub favorites: Vec<String>,
    pub hidden: Vec<String>,
    pub macros: Vec<ActionMacro>,
    pub curves: Vec<CurvePreset>,
}
impl Default for NativePlugins {
    fn default() -> Self {
        Self {
            motion_curves: true,
            folder_portals: true,
            quick_actions: true,
            folders: vec![],
            favorites: vec![],
            hidden: vec![],
            macros: vec![],
            curves: vec![],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FolderPortal {
    pub name: String,
    pub path: PathBuf,
}
impl FolderPortal {
    pub fn resolve(&self, project: Option<&Path>) -> Option<PathBuf> {
        if self.path.is_absolute() {
            Some(self.path.clone())
        } else {
            project.and_then(Path::parent).map(|p| p.join(&self.path))
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Action {
    Effect(String),
    Command(String),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionMacro {
    pub name: String,
    pub actions: Vec<Action>,
    pub shortcut: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurvePreset {
    pub name: String,
    pub handles: [f64; 4],
}
impl CurvePreset {
    /// Normalized Bezier handles -> native speed/influence, in source seconds.
    /// All project animations use the standard evaluator, including when this tool is disabled.
    pub fn apply(&self, a: &mut Keyframe, b: &mut Keyframe) {
        let span = (b.time - a.time).seconds();
        if span <= 0.0 {
            return;
        }
        let delta = match (&a.value, &b.value) {
            (Value::Point(a), Value::Point(b)) => (b[0] - a[0]).hypot(b[1] - a[1]),
            (Value::Color(a), Value::Color(b)) => ((b.r - a.r).powi(2)
                + (b.g - a.g).powi(2)
                + (b.b - a.b).powi(2)
                + (b.a - a.a).powi(2))
            .sqrt() as f64,
            _ => b.value.as_f64() - a.value.as_f64(),
        };
        let [x1, y1, x2, y2] = self.handles;
        a.interp_out = Interp::Bezier;
        b.interp_in = Interp::Bezier;
        a.ease_out.influence = x1.clamp(0.001, 0.999);
        b.ease_in.influence = (1.0 - x2).clamp(0.001, 0.999);
        a.ease_out.speed = y1.clamp(-2.0, 3.0) * delta / span / a.ease_out.influence;
        b.ease_in.speed = (1.0 - y2.clamp(-2.0, 3.0)) * delta / span / b.ease_in.influence;
    }
}
pub const BUILTIN_CURVES: &[(&str, [f64; 4])] = &[
    ("Smooth", [0.42, 0.0, 0.58, 1.0]),
    ("Ease In", [0.42, 0.0, 1.0, 1.0]),
    ("Ease Out", [0.0, 0.0, 0.58, 1.0]),
    ("Back / Overshoot", [0.34, 1.56, 0.64, 1.0]),
    ("Anticipation", [0.36, -0.56, 0.66, 1.0]),
    ("Linear", [1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0]),
];

/// Damped bounce as ordinary native keyframes; no extension is required during export.
pub fn bounce(a: &Keyframe, b: &Keyframe) -> Vec<Keyframe> {
    let mix = |fraction: f64| match (&a.value, &b.value) {
        (Value::Float(x), Value::Float(y)) => Value::Float(x + (y - x) * fraction),
        (Value::Int(x), Value::Int(y)) => {
            Value::Int((*x as f64 + (*y as f64 - *x as f64) * fraction).round() as i64)
        }
        (Value::Point(x), Value::Point(y)) => Value::Point([
            x[0] + (y[0] - x[0]) * fraction,
            x[1] + (y[1] - x[1]) * fraction,
        ]),
        (Value::Color(x), Value::Color(y)) => Value::Color(op_core::Rgba::new(
            x.r + (y.r - x.r) * fraction as f32,
            x.g + (y.g - x.g) * fraction as f32,
            x.b + (y.b - x.b) * fraction as f32,
            x.a + (y.a - x.a) * fraction as f32,
        )),
        _ => a.value.clone(),
    };
    let span = (b.time - a.time).seconds();
    let mut keys: Vec<_> = [
        (0.0, 0.0),
        (0.55, 1.0),
        (0.70, 0.82),
        (0.82, 1.0),
        (0.91, 0.94),
        (1.0, 1.0),
    ]
    .iter()
    .map(|(time, value)| {
        Keyframe::new(
            a.time + op_core::Dur::from_seconds(span * time),
            mix(*value),
            Interp::Bezier,
        )
    })
    .collect();
    keys[0].interp_in = a.interp_in;
    keys[0].ease_in = a.ease_in;
    if let Some(last) = keys.last_mut() {
        last.time = b.time;
        last.interp_out = b.interp_out;
        last.ease_out = b.ease_out;
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_core::{SrcTime, params::evaluate};
    #[test]
    fn old_preferences_and_plugin_settings_round_trip() {
        let mut p: crate::Preferences = serde_json::from_str(r#"{"language":"es"}"#).unwrap();
        assert!(p.plugins.motion_curves && p.plugins.quick_actions && p.plugins.folder_portals);
        p.plugins.quick_actions = false;
        p.plugins.macros.push(ActionMacro {
            name: "Look".into(),
            actions: vec![Action::Effect("op.video.vignette".into())],
            shortcut: "Ctrl+Alt+1".into(),
        });
        let d = tempfile::tempdir().unwrap();
        let dirs = crate::Dirs::portable(d.path());
        p.save(&dirs).unwrap();
        assert_eq!(crate::Preferences::load(&dirs), p);
    }
    #[test]
    fn bounce_lands_and_rebounds_before_settling() {
        let a = Keyframe::new(SrcTime::ZERO, Value::Point([0.0, 0.0]), Interp::Linear);
        let b = Keyframe::new(
            SrcTime::from_seconds(2.0),
            Value::Point([1.0, 1.0]),
            Interp::Linear,
        );
        let keys = bounce(&a, &b);
        assert_eq!(
            evaluate(&keys, SrcTime::from_seconds(1.1)),
            Value::Point([1.0, 1.0])
        );
        assert_eq!(
            evaluate(&keys, SrcTime::from_seconds(1.4)),
            Value::Point([0.82, 0.82])
        );
        assert_eq!(evaluate(&keys, SrcTime::from_seconds(2.0)), b.value);
    }
    #[test]
    fn curves_survive_serialization_and_change_native_animation() {
        let mut a = Keyframe::new(SrcTime::ZERO, Value::Float(100.0), Interp::Linear);
        let mut b = Keyframe::new(
            SrcTime::from_seconds(2.0),
            Value::Float(0.0),
            Interp::Linear,
        );
        CurvePreset {
            name: "Smooth".into(),
            handles: BUILTIN_CURVES[0].1,
        }
        .apply(&mut a, &mut b);
        let keys: Vec<Keyframe> =
            serde_json::from_str(&serde_json::to_string(&vec![a, b]).unwrap()).unwrap();
        let quarter = evaluate(&keys, SrcTime::from_seconds(0.5)).as_f64();
        assert!(quarter > 85.0 && quarter < 89.0, "{quarter}");
        assert!((evaluate(&keys, SrcTime::from_seconds(1.0)).as_f64() - 50.0).abs() < 0.001);
    }
    #[test]
    fn relative_portals_require_a_saved_project() {
        let p = FolderPortal {
            name: "Media".into(),
            path: "media".into(),
        };
        assert!(p.resolve(None).is_none());
        assert_eq!(
            p.resolve(Some(Path::new("project/edit.oproj"))),
            Some(PathBuf::from("project/media"))
        );
    }
}
