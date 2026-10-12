//! Isolated fframes CPU / Skia GPU rasterizer, sampled at exact rational time.
//! Wire format: JSON line in, JSON status line + exactly width*height*4 RGBA8 bytes out.
use base64::Engine;
use fframes::{CpuFrameRenderer, FrameRenderer};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
use std::sync::Arc;
mod gpu;
mod layers;

#[derive(Deserialize)]
struct Request {
    version: u32,
    width: u32,
    height: u32,
    #[serde(default)]
    svg: String,
    #[serde(default)]
    layers: Option<usize>,
}

/// usvgr resolves only preloaded images. Decode the parent's inline PNGs locally;
/// arbitrary paths and URLs are rejected instead of silently omitting their pixels.
fn images(svg: &str) -> Result<HashMap<String, Arc<fframes::usvgr::PreloadedImageData>>, String> {
    let document = roxmltree::Document::parse(svg).map_err(|e| e.to_string())?;
    let mut images = HashMap::new();
    for node in document
        .descendants()
        .filter(|n| n.has_tag_name("image") || n.has_tag_name("feImage"))
    {
        let href = node
            .attribute("href")
            .or_else(|| node.attribute(("http://www.w3.org/1999/xlink", "href")))
            .ok_or("image without href")?;
        if node.has_tag_name("feImage") && href.starts_with('#') {
            continue; // Internal transition side; usvgr resolves its group.
        }
        if images.contains_key(href) {
            continue;
        }
        let data = href
            .strip_prefix("data:image/png;base64,")
            .ok_or("only inline PNG images are supported")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| e.to_string())?;
        let mut reader =
            image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Png);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(16384);
        limits.max_image_height = Some(16384);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode().map_err(|e| e.to_string())?.to_rgba8();
        images.insert(
            href.to_string(),
            Arc::new(fframes::usvgr::PreloadedImageData::new(
                href.to_string(),
                image.width(),
                image.height(),
                image.as_raw(),
            )),
        );
    }
    Ok(images)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|a| a == "--version") {
        println!("OpenPremier-FFrames protocol 1 / fframes 1.2.0 / CPU + Skia GPU");
        return Ok(());
    }
    let mut fonts = fframes::usvgr::fontdb::Database::new();
    fonts.load_system_fonts();
    let args = std::env::args().collect::<Vec<_>>();
    let dimension = |key: &str| -> Result<u32, Box<dyn std::error::Error>> {
        Ok(args
            .windows(2)
            .find(|a| a[0] == key)
            .map(|a| a[1].parse())
            .transpose()?
            .unwrap_or(16))
    };
    let (width, height) = (dimension("--width")?, dimension("--height")?);
    if width == 0
        || height == 0
        || width > 16384
        || height > 16384
        || u64::from(width) * u64::from(height) > 33_177_600
    {
        return Err("invalid startup frame dimensions".into());
    }
    let backend = args
        .windows(2)
        .find(|a| a[0] == "--backend")
        .map(|a| a[1].as_str())
        .unwrap_or("cpu");
    let context = if backend == "gpu" {
        match gpu::context(width as usize, height as usize) {
            Ok(context) => Some(context),
            Err(error) => {
                println!("{}", serde_json::json!({"version":1,"error":error}));
                return Err(error.into());
            }
        }
    } else if backend == "cpu" {
        None
    } else {
        return Err("backend must be cpu or gpu".into());
    };
    let mut description = context
        .as_ref()
        .map(|(_, name)| format!("fframes 1.2.0 / Skia GPU / {name}"))
        .unwrap_or_else(|| "fframes 1.2.0 / CPU".into());
    let float_layers = args.iter().any(|a| a == "--float-layers");
    if float_layers {
        description.push_str(if backend == "cpu" {
            " / Skia CPU RGBA16F compatibility (F32 raster)"
        } else {
            " / Skia RGBA16F compatibility"
        });
    }
    let cpu_context = fframes_skia_renderer::SkiaCpuCtx::new(width as usize, height as usize);
    let mut layers = if float_layers {
        let backend: &dyn fframes_skia_renderer::SkiaBackend = match &context {
            Some((context, _)) => context,
            None => &cpu_context,
        };
        match layers::Layers::new(backend, width, height) {
            Ok(layers) => Some(layers),
            Err(error) => {
                println!("{}", serde_json::json!({"version":1,"error":error}));
                return Err(error.into());
            }
        }
    } else {
        None
    };
    if let Some(layers) = &mut layers {
        // Probe floating readback before the parent is allowed to open the destination.
        if let Err(error) = layers.render(&mut std::io::empty(), 0) {
            println!("{}", serde_json::json!({"version":1,"error":error}));
            return Err(error.into());
        }
    }
    let mut renderer: Box<dyn FrameRenderer + '_> = if let Some((context, _)) = &context {
        Box::new(fframes_skia_renderer::SkiaFrameRenderer::new(context))
    } else {
        Box::new(CpuFrameRenderer::new(20))
    };
    // Force surface/context creation before reporting availability. GPU initialization
    // may fail later than Vulkan device creation; that must not truncate an export.
    let probe = fframes::usvgr::Tree::from_str(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\"/>",
        &Default::default(),
        &fonts,
    )?;
    if !float_layers
        && let Err(error) = renderer.render_tree(&probe, fframes::Color::TRANSPARENT, width, height)
    {
        println!(
            "{}",
            serde_json::json!({"version":1,"error":error.to_string()})
        );
        return Err(error.into());
    }
    if std::env::args().any(|a| a == "--self-test") {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 255, 128]))
            .write_to(&mut png, image::ImageFormat::Png)?;
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\"><rect x=\"4\" y=\"4\" width=\"8\" height=\"8\" fill=\"red\"/><image width=\"2\" height=\"2\" href=\"data:image/png;base64,{}\"/></svg>",
            base64::engine::general_purpose::STANDARD.encode(png.into_inner())
        );
        let images = images(&svg)?;
        let options = fframes::usvgr::Options {
            image_data: Some(&images),
            ..Default::default()
        };
        let tree = fframes::usvgr::Tree::from_str(&svg, &options, &fonts)?;
        let frame = renderer.render_tree(&tree, fframes::Color::TRANSPARENT, 16, 16)?;
        if frame.pixels[(8 * 16 + 8) * 4..(8 * 16 + 8) * 4 + 4] != [255, 0, 0, 255]
            || frame.pixels[..4] != [0, 0, 255, 128]
            || frame.pixels[(15 * 16 + 15) * 4 + 3] != 0
        {
            return Err("fframes pixel self-test failed".into());
        }
        println!("{description} self-test passed");
        return Ok(());
    }
    let mut input = std::io::stdin().lock();
    let mut output = std::io::BufWriter::new(std::io::stdout().lock());
    if args.iter().any(|a| a == "--handshake") {
        serde_json::to_writer(
            &mut output,
            &serde_json::json!({"version":1,"ready":true,"backend":backend,"renderer":description,"format":if float_layers {"rgba16f-premul"} else {"rgba8-straight"}}),
        )?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    loop {
        let mut bytes = Vec::new();
        // A bounded request prevents accidentally allocating an unlimited scene from stdin.
        let count = input
            .by_ref()
            .take(64 * 1024 * 1024 + 1)
            .read_until(b'\n', &mut bytes)?;
        if count == 0 {
            break;
        }
        let frame = (|| -> Result<_, String> {
            if bytes.len() > 64 * 1024 * 1024 {
                return Err("scene exceeds 64 MiB".into());
            }
            let request: Request = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if request.version != 1
                || request.width == 0
                || request.height == 0
                || request.width > 16384
                || request.height > 16384
                || u64::from(request.width) * u64::from(request.height) > 33_177_600
            {
                return Err("unsupported protocol or frame size".into());
            }
            if let Some(layers) = &mut layers {
                if (request.width, request.height) != (width, height) {
                    return Err("floating frame size differs from startup".into());
                }
                return layers.render(&mut input, request.layers.ok_or("missing floating layers")?);
            }
            if request.layers.is_some() {
                return Err("floating layers require float mode".into());
            }
            let images = images(&request.svg)?;
            let options = fframes::usvgr::Options {
                image_data: Some(&images),
                ..Default::default()
            };
            let tree = fframes::usvgr::Tree::from_str(&request.svg, &options, &fonts)
                .map_err(|e| e.to_string())?;
            renderer
                .render_tree(
                    &tree,
                    fframes::Color::TRANSPARENT,
                    request.width,
                    request.height,
                )
                .map(|frame| frame.pixels)
                .map_err(|e| e.to_string())
        })();
        match frame {
            Ok(frame) => {
                serde_json::to_writer(
                    &mut output,
                    &serde_json::json!({"version":1,"bytes":frame.len()}),
                )?;
                output.write_all(b"\n")?;
                output.write_all(&frame)?;
            }
            Err(error) => {
                serde_json::to_writer(
                    &mut output,
                    &serde_json::json!({"version":1,"error":error}),
                )?;
                output.write_all(b"\n")?;
            }
        }
        output.flush()?;
    }
    Ok(())
}
