//! Deterministic tape speed ramps, integrated analytically so seeking and export agree.
use op_core::*;

fn params(c: &Component) -> (f64, f64, f64, f64) {
    let v = |key, default| c.param(key).map(|p| p.value.as_f64()).unwrap_or(default);
    (
        v("initial_speed", 100.0).clamp(1.0, 100.0) / 100.0,
        v("final_speed", 0.0).clamp(0.0, 100.0) / 100.0,
        v("start", 0.0).max(0.0),
        v("duration", 1.5).max(0.01),
    )
}
fn integrate(t: f64, a: f64, b: f64, start: f64, duration: f64) -> (f64, f64) {
    if t <= start {
        return (t * a, a);
    }
    let ramp = (t - start).min(duration);
    let position = start * a
        + a * ramp
        + (b - a) * ramp * ramp / (2.0 * duration)
        + (t - start - duration).max(0.0) * b;
    let speed = a + (b - a) * ramp / duration;
    (position, speed)
}
pub(crate) fn time(clip: &Clip, t: SeqTime) -> SeqTime {
    let mut elapsed = (t - clip.start).seconds();
    for c in clip
        .components
        .iter()
        .filter(|c| c.enabled && c.effect == "op.audio.tape_stop")
    {
        let (a, b, start, duration) = params(c);
        elapsed = integrate(elapsed, a, b, start, duration).0;
    }
    clip.start + Dur::from_seconds(elapsed)
}
pub(crate) fn gain(clip: &Clip, t: SeqTime) -> f32 {
    let mut elapsed = (t - clip.start).seconds();
    let mut gain = 1.0;
    for c in clip
        .components
        .iter()
        .filter(|c| c.enabled && c.effect == "op.audio.tape_stop")
    {
        let (a, b, start, duration) = params(c);
        let (position, speed) = integrate(elapsed, a, b, start, duration);
        // Fade out the final 3% of tape speed, avoiding a held DC sample at full stop.
        gain *= (speed / 0.03).clamp(0.0, 1.0);
        elapsed = position;
    }
    gain as f32
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn integrated_ramp_is_continuous_and_has_the_expected_pitch_ratio() {
        assert_eq!(integrate(0.5, 1.0, 0.0, 1.0, 2.0), (0.5, 1.0));
        assert_eq!(integrate(2.0, 1.0, 0.0, 1.0, 2.0), (1.75, 0.5));
        assert_eq!(integrate(4.0, 1.0, 0.0, 1.0, 2.0), (2.0, 0.0));
        let dt = 1e-5;
        let derivative = (integrate(2.0 + dt, 1.0, 0.0, 1.0, 2.0).0
            - integrate(2.0 - dt, 1.0, 0.0, 1.0, 2.0).0)
            / (2.0 * dt);
        assert!((derivative - 0.5).abs() < 1e-8);
    }
}
