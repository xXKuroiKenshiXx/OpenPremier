//! Export engine selection and isolated fframes CPU / Skia GPU compositors.
use base64::Engine;
use image::ImageEncoder;
use op_core::plan::{self, ClipLayer, LayerSource, TrackContent};
use op_core::*;
use op_render::FrameSource;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderEngine {
    #[default]
    Wgpu,
    WgpuSoftware,
    FframesCpu,
    FframesGpu,
}
impl RenderEngine {
    pub const ALL: [Self; 4] = [
        Self::Wgpu,
        Self::WgpuSoftware,
        Self::FframesCpu,
        Self::FframesGpu,
    ];
    pub fn is_fframes(self) -> bool {
        matches!(self, Self::FframesCpu | Self::FframesGpu)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Wgpu => "Native GPU (wgpu)",
            Self::WgpuSoftware => "Native software (wgpu)",
            Self::FframesCpu => "fframes CPU (experimental)",
            Self::FframesGpu => "fframes GPU (Skia, experimental)",
        }
    }
}

pub fn fframes_worker_path() -> Option<PathBuf> {
    let path = std::env::var_os("OPENPREMIER_FFRAMES_WORKER")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe().ok()?.parent().map(|p| {
                p.join(if cfg!(windows) {
                    "OpenPremier-FFrames.exe"
                } else {
                    "OpenPremier-FFrames"
                })
            })
        })?;
    path.is_file().then_some(path)
}

fn has_value(component: &Component, key: &str, predicate: impl Fn(&Value) -> bool) -> bool {
    component
        .param(key)
        .is_some_and(|p| predicate(&p.value) || p.keys.iter().any(|k| predicate(&k.value)))
}

pub fn fframes_blend_name(mode: u32) -> Option<&'static str> {
    Some(match mode {
        0 => "normal",
        2 => "darken",
        3 => "multiply",
        4 => "color-burn",
        7 => "lighten",
        8 => "screen",
        9 => "color-dodge",
        12 => "overlay",
        13 => "soft-light",
        14 => "hard-light",
        19 => "difference",
        20 => "exclusion",
        23 => "hue",
        24 => "saturation",
        25 => "color",
        26 => "luminosity",
        _ => return None,
    })
}

pub fn fframes_transition_supported(effect: &str) -> bool {
    matches!(
        effect,
        catalog::CROSS_DISSOLVE | "op.tr.dip_to_black" | "op.tr.dip_to_white"
    )
}

pub fn fframes_component_supported(effect: &str) -> bool {
    matches!(
        effect,
        catalog::MOTION | catalog::OPACITY | catalog::TEXT | catalog::SHAPE
    )
}

/// Conservative preflight covers all enabled clips of reachable sequences, including off-range.
/// Unsupported features fail before opening the destination. They are never silently bypassed.
pub fn validate_fframes(project: &Project, sequence: SequenceId) -> Result<(), String> {
    let mut pending = vec![sequence];
    let mut visited = std::collections::HashSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let seq = project.sequence(id).ok_or("missing sequence")?;
        if seq.settings.linear_compositing || seq.settings.pixel_aspect != (1, 1) {
            return Err("fframes supports nonlinear, square-pixel SDR sequences only".into());
        }
        for track in seq.video.iter().filter(|t| t.enabled) {
            if let Some(tr) = track
                .transitions
                .iter()
                .find(|tr| !fframes_transition_supported(&tr.effect))
            {
                return Err(format!(
                    "fframes does not support {}; select the native engine",
                    tr.effect
                ));
            }
            for clip in track.clips.iter().filter(|c| c.enabled) {
                match clip.source {
                    ClipSource::Sequence { sequence, .. } => pending.push(sequence),
                    ClipSource::Asset { asset, .. } => {
                        let a = project.asset(asset).ok_or("missing media asset")?;
                        if let Some(v) = &a.video {
                            let color = v.color;
                            if v.bit_depth > 8
                                || v.rotation != 0
                                || v.pixel_aspect != (1, 1)
                                || a.interpretation.pixel_aspect.is_some_and(|p| p != (1, 1))
                                || !matches!(color.transfer, Transfer::Bt709 | Transfer::Srgb)
                                || !matches!(color.primaries, Primaries::Bt709 | Primaries::Bt601)
                                || a.interpretation.alpha.unwrap_or(v.alpha)
                                    == AlphaMode::Premultiplied
                                || a.interpretation.ignore_alpha
                                || a.interpretation.invert_alpha
                            {
                                return Err(format!(
                                    "fframes: unsupported color/layout in {}",
                                    clip.name
                                ));
                            }
                        }
                    }
                    ClipSource::Generator { item }
                        if !matches!(
                            project.item(item).map(|i| &i.kind),
                            Some(ItemKind::Synthetic {
                                generator: Generator::ColorMatte { .. }
                                    | Generator::BlackVideo
                                    | Generator::TransparentVideo,
                                ..
                            })
                        ) =>
                    {
                        return Err("fframes does not support this generator".into());
                    }
                    _ => {}
                }
                for c in clip.components.iter().filter(|c| c.enabled) {
                    let unsupported =
                        c.foreign.is_some() || !fframes_component_supported(&c.effect);
                    if unsupported {
                        return Err(format!(
                            "fframes does not support {} in {}; select the native engine",
                            c.effect, clip.name
                        ));
                    }
                    if c.effect == catalog::OPACITY
                        && has_value(c, "blend_mode", |v| {
                            fframes_blend_name(v.as_choice()).is_none()
                        })
                    {
                        return Err(
                            "fframes does not support this blend mode; select the native engine"
                                .into(),
                        );
                    }
                    if c.effect == catalog::SHAPE && has_value(c, "feather", |v| v.as_f64() != 0.0)
                    {
                        return Err("fframes does not support shape feathering".into());
                    }
                }
            }
        }
    }
    Ok(())
}

/// Full catalog compatibility through shared native preparation, never by dropping effects.
/// Unresolved imported plugins remain an error; this bridge does not implement their code.
pub fn validate_fframes_hybrid(project: &Project, sequence: SequenceId) -> Result<(), String> {
    let mut pending = vec![sequence];
    let mut visited = std::collections::HashSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let seq = project.sequence(id).ok_or("missing sequence")?;
        for track in seq.video.iter().filter(|t| t.enabled) {
            for transition in &track.transitions {
                if !catalog::find(&transition.effect)
                    .is_some_and(|d| d.kind == catalog::EffectKind::VideoTransition)
                {
                    return Err(format!("unsupported transition {}", transition.effect));
                }
            }
            for clip in track.clips.iter().filter(|c| c.enabled) {
                if let ClipSource::Sequence { sequence, .. } = clip.source {
                    pending.push(sequence);
                }
                if let ClipSource::Asset { asset, .. } = clip.source {
                    project.asset(asset).ok_or("missing media asset")?;
                }
                for component in clip.components.iter().filter(|c| c.enabled) {
                    if component.foreign.is_some()
                        || !catalog::find(&component.effect)
                            .is_some_and(|d| d.kind.is_video() && !d.kind.is_transition())
                    {
                        return Err(format!(
                            "fframes hybrid does not implement {} in {}",
                            component.effect, clip.name
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

fn png_image(rgba: &[u8], width: u32, height: u32) -> Result<String, String> {
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(rgba, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "<image width=\"{width}\" height=\"{height}\" href=\"data:image/png;base64,{}\"/>",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
}
fn unpremultiply(pixels: &mut [u8]) {
    for p in pixels.as_chunks_mut::<4>().0 {
        let a = u32::from(p[3]);
        if a > 0 && a < 255 {
            for c in &mut p[..3] {
                *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}
fn colored(c: Rgba) -> String {
    format!(
        "fill=\"{}\" fill-opacity=\"{}\"",
        c.to_hex(),
        c.a.clamp(0.0, 1.0)
    )
}

fn graphics(layer: &ClipLayer, width: f64, height: f64) -> Result<String, String> {
    let mut out = String::new();
    for fx in layer.effects() {
        let position = fx.point("position");
        let (x, y) = (position[0] * width, position[1] * height);
        if fx.effect == catalog::TEXT {
            let size_scale = fx.f32("scale") / 100.0;
            let size = (fx.f32("font_size") * size_scale).clamp(1.0, 2000.0);
            let raster = (size * 4.0).round() / 4.0;
            let style = op_render::text::TextStyle {
                text: fx.text("text"),
                family: fx.text("font"),
                style: fx.choice("font_style"),
                size: raster,
                align: fx.choice("align"),
                tracking: fx.f32("tracking"),
                leading: fx.f32("leading") * size_scale,
                fill: fx.color("fill"),
                stroke: fx.bool("stroke").then(|| {
                    (
                        fx.color("stroke_color"),
                        fx.f32("stroke_width") * size_scale,
                    )
                }),
                background: fx.bool("background").then(|| {
                    let mut c = fx.color("background_color");
                    c.a *= fx.f32("background_opacity") / 100.0;
                    (c, fx.f32("background_size") * size_scale)
                }),
            };
            let mut image = op_render::text::rasterize(&style, &op_render::text::Fonts::global());
            unpremultiply(&mut image.rgba);
            let content = png_image(&image.rgba, image.width, image.height)?;
            let placed = format!(
                "<g opacity=\"{}\" transform=\"translate({x} {y}) rotate({}) scale({}) translate({} {})\">{content}</g>",
                fx.f64("opacity") / 100.0,
                fx.f64("rotation"),
                size / raster,
                -image.anchor[0],
                -image.anchor[1]
            );
            if fx.bool("shadow") {
                let id = format!("shadow-{}", fx.id.0);
                let angle = fx.f64("shadow_angle").to_radians();
                let distance = fx.f64("shadow_distance");
                let color = fx.color("shadow_color");
                out.push_str(&format!("<defs><filter id=\"{id}\" filterUnits=\"userSpaceOnUse\" x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\" color-interpolation-filters=\"sRGB\"><feGaussianBlur in=\"SourceAlpha\" stdDeviation=\"{}\"/><feOffset dx=\"{}\" dy=\"{}\" result=\"offset\"/><feFlood flood-color=\"{}\" flood-opacity=\"{}\"/><feComposite in2=\"offset\" operator=\"in\"/><feMerge><feMergeNode/><feMergeNode in=\"SourceGraphic\"/></feMerge></filter></defs><g filter=\"url(#{id})\">{placed}</g>",fx.f64("shadow_blur")/3.0,angle.sin()*distance,-angle.cos()*distance,color.to_hex(),fx.f64("shadow_opacity")/100.0));
            } else {
                out.push_str(&placed);
            }
        } else if fx.effect == catalog::SHAPE {
            let scale = fx.f64("scale") / 100.0;
            let (w, h) = (fx.f64("width") * scale, fx.f64("height") * scale);
            let mut style = colored(fx.color("fill"));
            if fx.bool("stroke") {
                style.push_str(&format!(
                    " stroke=\"{}\" stroke-opacity=\"{}\" stroke-width=\"{}\"",
                    fx.color("stroke_color").to_hex(),
                    fx.color("stroke_color").a,
                    fx.f64("stroke_width") * scale
                ));
            }
            let shape = if fx.choice("shape") == 1 {
                format!("<ellipse rx=\"{}\" ry=\"{}\" {style}/>", w / 2.0, h / 2.0)
            } else {
                format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{w}\" height=\"{h}\" rx=\"{}\" {style}/>",
                    -w / 2.0,
                    -h / 2.0,
                    fx.f64("corner_radius") * scale
                )
            };
            out.push_str(&format!(
                "<g opacity=\"{}\" transform=\"translate({x} {y}) rotate({})\">{shape}</g>",
                fx.f64("opacity") / 100.0,
                fx.f64("rotation")
            ));
        }
    }
    Ok(out)
}

pub fn scene_svg(
    project: &Project,
    sequence: SequenceId,
    time: SeqTime,
    source: &dyn FrameSource,
) -> Result<String, String> {
    fn scene(
        project: &Project,
        sid: SequenceId,
        time: SeqTime,
        source: &dyn FrameSource,
        depth: usize,
    ) -> Result<String, String> {
        if depth >= plan::MAX_NESTING {
            return Err("fframes nesting limit exceeded".into());
        }
        let plan = plan::frame_plan(project, sid, time);
        let (w, h) = (plan.width as f64, plan.height as f64);
        let mut svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" overflow=\"hidden\">"
        );
        for track in plan.tracks {
            let placed = |layer: &ClipLayer| -> Result<String, String> {
                let (mut lw, mut lh) = (w, h);
                let mut content = match layer.source {
                    LayerSource::Color(c) => {
                        format!("<rect width=\"{w}\" height=\"{h}\" {}/>", colored(c))
                    }
                    LayerSource::Transparent | LayerSource::Graphic => String::new(),
                    LayerSource::Media { asset, time } => {
                        let asset = project.asset(asset).ok_or("missing media")?;
                        let frame = source
                            .video_frame(asset, time)
                            .ok_or_else(|| format!("frame unavailable: {}", asset.path))?;
                        lw = frame.width as f64;
                        lh = frame.height as f64;
                        png_image(&frame.to_rgba8(), frame.width, frame.height)?
                    }
                    LayerSource::Sequence { sequence, time } => {
                        let seq = project
                            .sequence(sequence)
                            .ok_or("missing nested sequence")?;
                        lw = seq.settings.width as f64;
                        lh = seq.settings.height as f64;
                        scene(project, sequence, time, source, depth + 1)?
                    }
                    _ => return Err("unsupported fframes source".into()),
                };
                if layer.scale_to_frame {
                    let k = (w / lw).min(h / lh);
                    content = format!("<g transform=\"scale({k})\">{content}</g>");
                    lw *= k;
                    lh *= k;
                }
                content.push_str(&graphics(layer, lw, lh)?);
                let motion = layer.fixed(catalog::MOTION);
                let pos = motion.map(|m| m.point("position")).unwrap_or([0.5, 0.5]);
                let anchor = motion.map(|m| m.point("anchor")).unwrap_or([0.5, 0.5]);
                let sy = motion.map(|m| m.f64("scale") / 100.0).unwrap_or(1.0);
                let sx = motion
                    .map(|m| {
                        if m.bool("uniform_scale") {
                            sy
                        } else {
                            m.f64("scale_width") / 100.0
                        }
                    })
                    .unwrap_or(sy);
                let rotation = motion.map(|m| m.f64("rotation")).unwrap_or(0.0);
                let opacity = layer
                    .fixed(catalog::OPACITY)
                    .map(|o| o.f64("opacity") / 100.0)
                    .unwrap_or(1.0)
                    .clamp(0.0, 1.0);
                let blend = layer
                    .fixed(catalog::OPACITY)
                    .map(|o| o.choice("blend_mode"))
                    .unwrap_or(0);
                let blend = fframes_blend_name(blend).ok_or("unsupported blend mode")?;
                Ok(format!(
                    "<g style=\"mix-blend-mode:{blend}\" opacity=\"{opacity}\" transform=\"translate({} {}) rotate({rotation}) scale({sx} {sy}) translate({} {})\">{content}</g>",
                    pos[0] * w,
                    pos[1] * h,
                    -anchor[0] * lw,
                    -anchor[1] * lh
                ))
            };
            match track.content {
                TrackContent::Clip(layer) => svg.push_str(&placed(&layer)?),
                TrackContent::Transition {
                    effect,
                    progress,
                    from,
                    to,
                    ..
                } => {
                    if !fframes_transition_supported(&effect) {
                        return Err("unsupported transition".into());
                    }
                    let mut a = from.as_ref().map(&placed).transpose()?.unwrap_or_default();
                    let mut b = to.as_ref().map(&placed).transpose()?.unwrap_or_default();
                    let mut t = progress.clamp(0.0, 1.0);
                    if effect != catalog::CROSS_DISSOLVE {
                        let color = if effect == "op.tr.dip_to_white" {
                            "white"
                        } else {
                            "black"
                        };
                        let dip = format!("<rect width=\"{w}\" height=\"{h}\" fill=\"{color}\"/>");
                        if t < 0.5 {
                            b = dip;
                            t *= 2.0;
                        } else {
                            a = dip;
                            t = t * 2.0 - 1.0;
                        }
                    }
                    // CPU svgr caches referenced groups by SVG identity. Include their
                    // contents so reusing a worker never reuses an old transition side.
                    let mut fingerprint = std::hash::DefaultHasher::new();
                    a.hash(&mut fingerprint);
                    b.hash(&mut fingerprint);
                    let id = format!(
                        "tr-{}-{}-{}-{:x}",
                        sid.0,
                        time.ticks(),
                        track.track,
                        fingerprint.finish()
                    );
                    // Arithmetic combines premultiplied sides. Two source-over fades would
                    // reduce alpha and produce the wrong midpoint for opaque pictures.
                    svg.push_str(&format!("<defs><g id=\"{id}-a\">{a}</g><g id=\"{id}-b\">{b}</g><filter id=\"{id}\" filterUnits=\"userSpaceOnUse\" primitiveUnits=\"userSpaceOnUse\" x=\"0\" y=\"0\" width=\"{w}\" height=\"{h}\" color-interpolation-filters=\"sRGB\"><feImage href=\"#{id}-a\" result=\"a\"/><feImage href=\"#{id}-b\" result=\"b\"/><feComposite in=\"a\" in2=\"b\" operator=\"arithmetic\" k1=\"0\" k2=\"{}\" k3=\"{t}\" k4=\"0\"/></filter></defs><rect width=\"{w}\" height=\"{h}\" filter=\"url(#{id})\"/>", 1.0-t));
                }
            }
        }
        svg.push_str("</svg>");
        Ok(svg)
    }
    scene(project, sequence, time, source, 0)
}

/// Owns only the spawned worker. Reader/writer threads keep blocked pipes cancellable.
pub struct FframesRenderer {
    child: Child,
    requests: Option<crossbeam_channel::Sender<Vec<Vec<u8>>>>,
    replies: crossbeam_channel::Receiver<Result<Vec<u8>, String>>,
    threads: Vec<std::thread::JoinHandle<()>>,
    description: String,
    size: (u32, u32),
    float_layers: bool,
}
impl FframesRenderer {
    pub fn start(width: u32, height: u32) -> Result<Self, String> {
        Self::start_with_engine(
            RenderEngine::FframesCpu,
            width,
            height,
            &AtomicBool::new(false),
        )
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn start_with_engine(
        engine: RenderEngine,
        width: u32,
        height: u32,
        cancel: &AtomicBool,
    ) -> Result<Self, String> {
        Self::start_mode(engine, width, height, cancel, false)
    }
    pub fn start_hybrid(
        engine: RenderEngine,
        width: u32,
        height: u32,
        cancel: &AtomicBool,
    ) -> Result<Self, String> {
        Self::start_mode(engine, width, height, cancel, true)
    }
    fn start_mode(
        engine: RenderEngine,
        width: u32,
        height: u32,
        cancel: &AtomicBool,
        float_layers: bool,
    ) -> Result<Self, String> {
        if !engine.is_fframes() {
            return Err("not a fframes engine".into());
        }
        if width == 0
            || height == 0
            || width > 16384
            || height > 16384
            || u64::from(width) * u64::from(height) > 33_177_600
        {
            return Err("fframes supports nonempty frames up to 33,177,600 pixels".into());
        }
        let expected = width as usize * height as usize * if float_layers { 8 } else { 4 };
        let path = fframes_worker_path().ok_or("fframes worker is not installed")?;
        let mut command = Command::new(path);
        let backend = if engine == RenderEngine::FframesGpu {
            "gpu"
        } else {
            "cpu"
        };
        command
            .args(["--backend", backend, "--handshake"])
            .args([
                "--width",
                &width.to_string(),
                "--height",
                &height.to_string(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if float_layers {
            command.arg("--float-layers");
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("cannot start fframes: {e}"))?;
        let mut stdin = child.stdin.take().ok_or("fframes stdin missing")?;
        let stdout = child.stdout.take().ok_or("fframes stdout missing")?;
        let (tx, rx) = crossbeam_channel::bounded::<Vec<Vec<u8>>>(1);
        let (reply_tx, replies) = crossbeam_channel::bounded(1);
        let (ready_tx, ready_rx) = crossbeam_channel::bounded(1);
        let writer = std::thread::spawn(move || {
            for request in rx {
                if request
                    .iter()
                    .try_for_each(|part| stdin.write_all(part))
                    .and_then(|_| stdin.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
        let reader = std::thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            let ready = (|| -> Result<String, String> {
                let mut line = String::new();
                let count = stdout
                    .by_ref()
                    .take(4097)
                    .read_line(&mut line)
                    .map_err(|e| e.to_string())?;
                if count == 0 || count > 4096 {
                    return Err("fframes startup status missing or too large".into());
                }
                let header: serde_json::Value =
                    serde_json::from_str(&line).map_err(|e| e.to_string())?;
                if let Some(error) = header["error"].as_str() {
                    return Err(error.into());
                }
                if header["version"] != 1 || header["ready"] != true || header["backend"] != backend
                {
                    return Err("fframes startup protocol/backend mismatch".into());
                }
                if float_layers && header["format"] != "rgba16f-premul" {
                    return Err("fframes worker lacks floating compatibility protocol".into());
                }
                header["renderer"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .ok_or("fframes renderer identity missing".into())
            })();
            let failed = ready.is_err();
            if ready_tx.send(ready).is_err() || failed {
                return;
            }
            loop {
                let result = (|| -> Result<Vec<u8>, String> {
                    let mut line = String::new();
                    let count = stdout
                        .by_ref()
                        .take(4097)
                        .read_line(&mut line)
                        .map_err(|e| e.to_string())?;
                    if count == 0 {
                        return Err("fframes worker closed stdout".into());
                    }
                    if count > 4096 {
                        return Err("fframes status too large".into());
                    }
                    let header: serde_json::Value =
                        serde_json::from_str(&line).map_err(|e| e.to_string())?;
                    if let Some(error) = header["error"].as_str() {
                        return Err(error.into());
                    }
                    if header["version"] != 1 || header["bytes"].as_u64() != Some(expected as u64) {
                        return Err("fframes protocol mismatch".into());
                    }
                    let mut pixels = vec![0; expected];
                    stdout.read_exact(&mut pixels).map_err(|e| e.to_string())?;
                    Ok(pixels)
                })();
                let failed = result.is_err();
                if reply_tx.send(result).is_err() || failed {
                    break;
                }
            }
        });
        let mut renderer = Self {
            child,
            requests: Some(tx),
            replies,
            threads: vec![writer, reader],
            description: String::new(),
            size: (width, height),
            float_layers,
        };
        let since = Instant::now();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            if since.elapsed() > Duration::from_secs(30) {
                return Err("fframes initialization timed out".into());
            }
            match ready_rx.recv_timeout(Duration::from_millis(25)) {
                Ok(ready) => {
                    renderer.description = ready?;
                    return Ok(renderer);
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err("fframes startup disconnected".into()),
            }
        }
    }
    pub fn render(
        &mut self,
        svg: String,
        width: u32,
        height: u32,
        cancel: &AtomicBool,
    ) -> Result<Vec<u8>, String> {
        if self.float_layers {
            return Err("floating worker requires prepared layers".into());
        }
        if (width, height) != self.size {
            return Err("fframes frame size differs from the export".into());
        }
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let mut request = serde_json::to_vec(
            &serde_json::json!({"version":1,"width":width,"height":height,"svg":svg}),
        )
        .map_err(|e| e.to_string())?;
        request.push(b'\n');
        if request.len() > 64 * 1024 * 1024 {
            return Err("fframes scene exceeds 64 MiB".into());
        }
        self.exchange(vec![request], cancel)
    }

    pub fn render_layers(
        &mut self,
        layers: Vec<Vec<u8>>,
        cancel: &AtomicBool,
    ) -> Result<Vec<u8>, String> {
        if !self.float_layers {
            return Err("worker does not accept floating layers".into());
        }
        let (width, height) = self.size;
        let bytes = width as usize * height as usize * 8;
        if layers.iter().any(|l| l.len() != bytes)
            || (layers.len() as u64 * bytes as u64) > 512 * 1024 * 1024
        {
            return Err("invalid floating layer size or memory budget".into());
        }
        let mut request = serde_json::to_vec(
            &serde_json::json!({"version":1,"width":width,"height":height,"layers":layers.len()}),
        )
        .map_err(|e| e.to_string())?;
        request.push(b'\n');
        let parts = std::iter::once(request).chain(layers).collect();
        self.exchange(parts, cancel)
    }

    fn exchange(&mut self, request: Vec<Vec<u8>>, cancel: &AtomicBool) -> Result<Vec<u8>, String> {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        self.requests
            .as_ref()
            .ok_or("worker closed")?
            .send(request)
            .map_err(|e| e.to_string())?;
        let start = Instant::now();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            if start.elapsed() > Duration::from_secs(60) {
                return Err("fframes frame timed out".into());
            }
            match self.replies.recv_timeout(Duration::from_millis(25)) {
                Ok(reply) => return reply,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err("fframes worker disconnected".into()),
            }
        }
    }
}
impl Drop for FframesRenderer {
    fn drop(&mut self) {
        self.requests.take();
        let (_, dummy) = crossbeam_channel::bounded(0);
        drop(std::mem::replace(&mut self.replies, dummy));
        let _ = self.child.kill();
        let _ = self.child.wait();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}
