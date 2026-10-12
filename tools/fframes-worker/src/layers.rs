//! Floating-point compatibility compositor built on fframes' public Skia backend.
//! Source preparation is shared with OpenPremier, while Skia composes separate tracks.
use fframes_skia_renderer::{SkiaBackend, SkiaContext, lock_queue, skia_safe as skia};
use half::f16;
use std::io::Read;

pub struct Layers {
    context: Option<SkiaContext>,
    info: skia::ImageInfo,
    cpu_f32: bool,
}

impl Layers {
    pub fn new(backend: &dyn SkiaBackend, width: u32, height: u32) -> Result<Self, String> {
        let mut context = backend.create_skia_context().map_err(|e| e.to_string())?;
        // Skia's CPU half conversion flushes half-float subnormals. Keep its raster
        // compositor in F32 and perform wire conversions in Rust to retain shadows
        // and small alpha. GPU storage and the shared wire format remain RGBA16F.
        let cpu_f32 = context.gpu.is_none();
        let info = skia::ImageInfo::new(
            (width as i32, height as i32),
            if cpu_f32 {
                skia::ColorType::RGBAF32
            } else {
                skia::ColorType::RGBAF16
            },
            skia::AlphaType::Premul,
            None,
        );
        context.surface = if let Some(gpu) = context.gpu.as_mut() {
            skia::gpu::surfaces::render_target(
                gpu,
                skia::gpu::Budgeted::Yes,
                &info,
                None,
                skia::gpu::SurfaceOrigin::TopLeft,
                None,
                false,
                None,
            )
        } else {
            skia::surfaces::raster(&info, None, None)
        }
        .ok_or("cannot allocate a floating Skia surface")?;
        Ok(Self {
            context: Some(context),
            info,
            cpu_f32,
        })
    }

    pub fn render(&mut self, input: &mut impl Read, count: usize) -> Result<Vec<u8>, String> {
        let context = self.context.as_mut().ok_or("floating context closed")?;
        let bytes = self.info.min_row_bytes() * self.info.height() as usize;
        let wire_bytes = if self.cpu_f32 { bytes / 2 } else { bytes };
        if (count as u64)
            .checked_mul(wire_bytes as u64)
            .is_none_or(|n| n > 512 * 1024 * 1024)
        {
            return Err("floating layers exceed 512 MiB".into());
        }
        context.surface.canvas().clear(skia::Color::TRANSPARENT);
        for _ in 0..count {
            let mut pixels = vec![0; wire_bytes];
            input.read_exact(&mut pixels).map_err(|e| e.to_string())?;
            let pixels = if self.cpu_f32 {
                pixels
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .flat_map(|b| f16::from_le_bytes([b[0], b[1]]).to_f32().to_ne_bytes())
                    .collect::<Vec<_>>()
            } else {
                pixels
            };
            let data = skia::Data::new_copy(&pixels);
            let image = skia::images::raster_from_data(&self.info, data, self.info.min_row_bytes())
                .ok_or("invalid floating layer")?;
            context
                .surface
                .canvas()
                .draw_image(&image, (0.0, 0.0), None);
        }
        let _queue = lock_queue(context.queue_lock.as_ref());
        if let Some(gpu) = context.gpu.as_mut() {
            gpu.flush_submit_and_sync_cpu();
        }
        let mut pixels = vec![0; bytes];
        let image = context.surface.image_snapshot();
        let pixmap = skia::Pixmap::new(&self.info, &mut pixels, self.info.min_row_bytes())
            .ok_or("cannot allocate floating output")?;
        if !image.read_pixels_to_pixmap_with_context(
            context.gpu.as_mut(),
            &pixmap,
            (0, 0),
            skia::image::CachingHint::Disallow,
        ) {
            return Err("cannot read floating Skia output".into());
        }
        drop(pixmap);
        if self.cpu_f32 {
            Ok(pixels
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|b| {
                    f16::from_f32(f32::from_ne_bytes([b[0], b[1], b[2], b[3]])).to_le_bytes()
                })
                .collect())
        } else {
            Ok(pixels)
        }
    }
}

impl Drop for Layers {
    fn drop(&mut self) {
        // Queue ownership is shared by fframes contexts. Drop GPU resources under its lock.
        if let Some(SkiaContext {
            surface,
            gpu,
            queue_lock,
            reader,
        }) = self.context.take()
        {
            let _queue = lock_queue(queue_lock.as_ref());
            drop(surface);
            drop(reader);
            drop(gpu);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu(width: u32) -> Layers {
        Layers::new(
            &fframes_skia_renderer::SkiaCpuCtx::new(width as usize, 1),
            width,
            1,
        )
        .unwrap()
    }

    #[test]
    fn cpu_preserves_all_positive_half_subnormals_and_hdr_channels() {
        // Every positive subnormal, plus normal boundary values and an HDR highlight.
        let values = (1..=1024).chain([0x3801, 0x3c00, 0x4000]);
        let pixels = values
            .flat_map(|bits| [f16::from_bits(bits), f16::ZERO, f16::ZERO, f16::ONE])
            .flat_map(f16::to_le_bytes)
            .collect::<Vec<_>>();
        let output = cpu((pixels.len() / 8) as u32)
            .render(&mut pixels.as_slice(), 1)
            .unwrap();
        assert_eq!(output, pixels);
    }

    #[test]
    fn cpu_composites_tiny_alpha_without_flushing_shadows() {
        let a = f16::from_bits(1);
        let b = f16::from_bits(511);
        let input = [a, a, a, a, b, b, b, b]
            .into_iter()
            .flat_map(f16::to_le_bytes)
            .collect::<Vec<_>>();
        let expected = f16::from_f32(b.to_f32() + a.to_f32() * (1.0 - b.to_f32()));
        let output = cpu(1).render(&mut input.as_slice(), 2).unwrap();
        assert_eq!(
            output,
            [expected; 4]
                .into_iter()
                .flat_map(f16::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert_eq!(cpu(1).render(&mut std::io::empty(), 0).unwrap(), [0; 8]);
    }
}
