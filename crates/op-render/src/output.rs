//! Frame output: presentation for the monitors, delivery planes for export, and scopes.

use op_media::{DeliveryColor, VideoInput};

use crate::params::P;
use crate::pipelines::DISPLAY;
use crate::pool::Tex;
use crate::render::Renderer;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ScopeKind {
    Waveform,
    Parade,
    Vectorscope,
    Histogram,
}

impl ScopeKind {
    pub const ALL: [ScopeKind; 4] = [
        ScopeKind::Waveform,
        ScopeKind::Parade,
        ScopeKind::Vectorscope,
        ScopeKind::Histogram,
    ];

    fn index(self) -> u32 {
        match self {
            ScopeKind::Waveform => 0,
            ScopeKind::Parade => 1,
            ScopeKind::Vectorscope => 2,
            ScopeKind::Histogram => 3,
        }
    }

    fn grid(self) -> (u32, u32) {
        match self {
            ScopeKind::Waveform => (384, 256),
            ScopeKind::Parade => (384, 256),
            ScopeKind::Vectorscope => (256, 256),
            ScopeKind::Histogram => (256, 4),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ScopeUniform {
    size: [u32; 2],
    grid: [u32; 2],
    kind: u32,
    gain: f32,
    pad: [u32; 2],
}

pub(crate) struct ScopeResources {
    uniform: wgpu::Buffer,
    counts: wgpu::Buffer,
}

impl Renderer {
    /// Uploads a tightly packed working frame without quantizing its premultiplied floats.
    pub fn upload_working(
        &mut self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Tex, String> {
        if width == 0
            || height == 0
            || pixels.len() as u64 != u64::from(width) * u64::from(height) * 8
        {
            return Err("invalid RGBA16F working frame".into());
        }
        let frame = self.work(width, height);
        self.gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &frame.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 8),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        Ok(frame)
    }

    /// Converts a finished frame into a display texture (Rgba8Unorm) for a monitor.
    pub fn present_into(&mut self, frame: &Tex, target: &Tex, show_alpha: bool) {
        self.pass(
            if self.hdr_working {
                "fs_hdr_present"
            } else {
                "fs_present"
            },
            &[&frame.view],
            P::new().b(show_alpha),
            frame.size(),
            target,
        );
    }

    /// A display texture of the given size.
    pub fn display_target(&self, w: u32, h: u32) -> Tex {
        Tex::new(
            self.device(),
            w,
            h,
            DISPLAY,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            "display",
        )
    }

    /// Reads a texture back as tightly packed rows (blocking).
    pub fn read_texture(&mut self, t: &Tex, bytes_per_pixel: u32) -> Result<Vec<u8>, String> {
        self.read_textures(&[(t, bytes_per_pixel)])?
            .pop()
            .ok_or_else(|| "GPU readback returned no texture".into())
    }

    /// Reads several textures back with one submission and one wait (blocking). Buffers are
    /// reused between calls.
    pub fn read_textures(&mut self, list: &[(&Tex, u32)]) -> Result<Vec<Vec<u8>>, String> {
        let pending = self.begin_read(list);
        self.end_read(pending)
    }

    /// Starts reading textures back: the copies are queued and the call returns at once, so the
    /// GPU works on them while the caller prepares the next frame. `end_read` collects them.
    pub fn begin_read(&mut self, list: &[(&Tex, u32)]) -> PendingRead {
        let mut enc = self
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("readback"),
            });
        let mut jobs = Vec::with_capacity(list.len());
        for (t, bpp) in list {
            let unpadded = t.width * bpp;
            let padded = unpadded.div_ceil(256) * 256;
            let size = padded as u64 * t.height as u64;
            let buffer = self
                .readbacks
                .get_mut(&size)
                .and_then(|v| v.pop())
                .unwrap_or_else(|| {
                    self.device().create_buffer(&wgpu::BufferDescriptor {
                        label: Some("readback"),
                        size,
                        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                        mapped_at_creation: false,
                    })
                });
            enc.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &t.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded),
                        rows_per_image: Some(t.height),
                    },
                },
                wgpu::Extent3d {
                    width: t.width,
                    height: t.height,
                    depth_or_array_layers: 1,
                },
            );
            jobs.push((buffer, size, unpadded, padded, t.height));
        }
        // pending passes must run first
        self.submit();
        let index = self.gpu.queue.submit(Some(enc.finish()));
        let (tx, rx) = std::sync::mpsc::channel();
        for (i, (buffer, ..)) in jobs.iter().enumerate() {
            let tx = tx.clone();
            buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send((i, r.map_err(|e| e.to_string())));
            });
        }
        PendingRead {
            jobs,
            index,
            mapped: rx,
            post: Post::None,
        }
    }

    /// Waits up to 30 seconds for reads started with `begin_read` (only their own submission)
    /// and returns tightly packed pixels. Wait, mapping and range failures fail the whole batch.
    pub fn end_read(&mut self, mut pending: PendingRead) -> Result<Vec<Vec<u8>>, String> {
        let jobs = std::mem::take(&mut pending.jobs);
        let index = pending.index.clone();
        let post = std::mem::replace(&mut pending.post, Post::None);
        if let Err(e) = self.device().poll(wgpu::PollType::Wait {
            submission_index: Some(index),
            timeout: Some(std::time::Duration::from_secs(30)),
        }) {
            // Discard every outstanding buffer; failed buffers must never enter the pool.
            for (buffer, ..) in &jobs {
                buffer.destroy();
            }
            return Err(format!("GPU readback wait failed: {e}"));
        }
        let mut status = vec![None; jobs.len()];
        for (i, r) in pending.mapped.try_iter() {
            status[i] = Some(r);
        }
        let mut out = Vec::with_capacity(jobs.len());
        let mut failure = None;
        for (i, (buffer, size, unpadded, padded, height)) in jobs.into_iter().enumerate() {
            let data = (|| {
                status[i]
                    .take()
                    .ok_or_else(|| format!("GPU readback mapping {i} did not complete"))?
                    .map_err(|e| format!("GPU readback mapping {i} failed: {e}"))?;
                let slice = buffer.slice(..);
                let mapped = slice
                    .get_mapped_range()
                    .map_err(|e| format!("GPU readback range {i} failed: {e}"))?;
                let mut data = vec![0u8; (unpadded * height) as usize];
                if unpadded == padded {
                    let n = data.len();
                    data.copy_from_slice(&mapped[..n]);
                } else {
                    for y in 0..height as usize {
                        data[y * unpadded as usize..(y + 1) * unpadded as usize].copy_from_slice(
                            &mapped[y * padded as usize..y * padded as usize + unpadded as usize],
                        );
                    }
                }
                Ok::<_, String>(data)
            })();
            match data {
                Ok(data) => {
                    buffer.unmap();
                    let pool = self.readbacks.entry(size).or_default();
                    if pool.len() < 4 {
                        pool.push(buffer);
                    }
                    out.push(data);
                }
                Err(e) => {
                    // A failed mapping is already unmapped; unmap again would be invalid.
                    buffer.destroy();
                    failure.get_or_insert(e);
                }
            }
        }
        if let Some(e) = failure {
            return Err(e);
        }
        Ok(match post {
            Post::None => out,
            Post::Planes { bytes, alpha } => split_planes(out, bytes, alpha),
        })
    }

    /// Converts a finished frame to the planes an encoder expects (tightly packed rows).
    pub fn delivery_planes(
        &mut self,
        frame: &Tex,
        input: VideoInput,
    ) -> Result<Vec<Vec<u8>>, String> {
        let pending = self.begin_delivery(frame, input);
        self.end_read(pending)
    }

    /// Like `delivery_planes`, without waiting: `end_read` returns the planes.
    pub fn begin_delivery(&mut self, frame: &Tex, input: VideoInput) -> PendingRead {
        self.begin_delivery_matrix(frame, input, false)
    }

    /// HDR conversion happens in floating point before quantizing to 10-bit BT.2020 YUV.
    pub fn begin_delivery_color(
        &mut self,
        frame: &Tex,
        input: VideoInput,
        color: DeliveryColor,
    ) -> PendingRead {
        if !color.is_hdr() {
            return self.begin_delivery(frame, input);
        }
        let encoded = self.work(frame.width, frame.height);
        self.pass(
            "fs_delivery_hdr",
            &[&frame.view],
            P::new()
                .f(if color == DeliveryColor::Pq { 1.0 } else { 2.0 })
                .b(input == VideoInput::Yuva444p10),
            frame.size(),
            &encoded,
        );
        let pending = self.begin_delivery_matrix(&encoded, input, true);
        self.put(encoded);
        pending
    }

    fn begin_delivery_matrix(
        &mut self,
        frame: &Tex,
        input: VideoInput,
        bt2020: bool,
    ) -> PendingRead {
        let (w, h) = (frame.width, frame.height);
        let (planes, cx, cy, bytes) = input.planes();
        let bits = input.bits() as f32;
        let straight = matches!(input, VideoInput::Yuva444p10);
        if !input.is_yuv() {
            let t = self.target(w, h, DISPLAY);
            self.pass(
                "fs_rgba8",
                &[&frame.view],
                P::new().b(true),
                frame.size(),
                &t,
            );
            let pending = self.begin_read(&[(&t, 4)]);
            // the copy is queued before anything can draw into the texture again
            self.put(t);
            return pending;
        }
        let (cw, ch) = (w.div_ceil(1 << cx), h.div_ceil(1 << cy));
        let wide = bytes == 2;
        // luma
        let (luma_entry, luma_fmt, luma_bpp) = if wide {
            ("fs_luma16", wgpu::TextureFormat::R16Uint, 2)
        } else {
            ("fs_luma8", wgpu::TextureFormat::R8Unorm, 1)
        };
        let y = self.target(w, h, luma_fmt);
        self.pass(
            luma_entry,
            &[&frame.view],
            P::new().b(straight).f(bits).b(bt2020),
            frame.size(),
            &y,
        );

        // chroma, interleaved on the GPU and split here
        let (chroma_entry, chroma_fmt, chroma_bpp) = if wide {
            ("fs_chroma16", wgpu::TextureFormat::Rg16Uint, 4)
        } else {
            ("fs_chroma8", wgpu::TextureFormat::Rg8Unorm, 2)
        };
        let c = self.target(cw, ch, chroma_fmt);
        self.pass(
            chroma_entry,
            &[&frame.view],
            P::new().b(straight).f(bits).b(bt2020),
            frame.size(),
            &c,
        );
        // the alpha plane (if any) is drawn too, then everything is read back at once
        let a = (planes == 4).then(|| {
            let a = self.target(w, h, wgpu::TextureFormat::R16Uint);
            self.pass(
                "fs_alpha16",
                &[&frame.view],
                P::new().b(true).f(bits),
                frame.size(),
                &a,
            );
            a
        });
        let mut list: Vec<(&Tex, u32)> = vec![(&y, luma_bpp), (&c, chroma_bpp)];
        if let Some(a) = &a {
            list.push((a, 2));
        }
        let mut pending = self.begin_read(&list);
        pending.post = Post::Planes {
            bytes,
            alpha: planes == 4,
        };
        self.put(y);
        self.put(c);
        if let Some(a) = a {
            self.put(a);
        }
        pending
    }

    /// Straight RGBA8 of a frame (thumbnails, still export, tests).
    pub fn rgba8(&mut self, frame: &Tex) -> Result<Vec<u8>, String> {
        let t = self.target(frame.width, frame.height, DISPLAY);
        self.pass(
            if self.hdr_working {
                "fs_hdr_preview"
            } else {
                "fs_straight"
            },
            &[&frame.view],
            P::new(),
            frame.size(),
            &t,
        );
        let data = self.read_texture(&t, 4);
        self.put(t);
        data
    }

    /// Draws a scope of `frame` into `target` (a display texture).
    pub fn scope(&mut self, frame: &Tex, kind: ScopeKind, target: &Tex, gain: f32) {
        // analyze a reduced copy: scopes need distribution, not every pixel
        let k = (480.0 / frame.width as f32)
            .min(270.0 / frame.height as f32)
            .min(1.0);
        let (aw, ah) = (
            ((frame.width as f32 * k) as u32).max(1),
            ((frame.height as f32 * k) as u32).max(1),
        );
        let small = self.work(aw, ah);
        self.pass("fs_copy", &[&frame.view], P::new(), frame.size(), &small);
        let (gw, gh) = kind.grid();
        let res = self.scope_resources(kind, gw * gh);
        let u = ScopeUniform {
            size: [aw, ah],
            grid: [gw, gh],
            kind: kind.index(),
            gain,
            pad: [0; 2],
        };
        self.gpu
            .queue
            .write_buffer(&res.uniform, 0, bytemuck::bytes_of(&u));
        let device = self.gpu.device.clone();
        let compute_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scope compute"),
            layout: &self.pipes().scope_compute_layout.clone(),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: res.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&small.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: res.counts.as_entire_binding(),
                },
            ],
        });
        let draw_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scope draw"),
            layout: &self.pipes().scope_draw_layout.clone(),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: res.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: res.counts.as_entire_binding(),
                },
            ],
        });
        let compute = self.pipes().scope_compute.clone();
        let draw = self.pipes().scope_draw.clone();
        let enc = self.encoder_mut();
        enc.clear_buffer(&res.counts, 0, None);
        {
            let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("scope"),
                timestamp_writes: None,
            });
            cp.set_pipeline(&compute);
            cp.set_bind_group(0, &compute_bg, &[]);
            cp.dispatch_workgroups(aw.div_ceil(16), ah.div_ceil(16), 1);
        }
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scope draw"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&draw);
            rp.set_bind_group(0, &draw_bg, &[]);
            rp.draw(0..3, 0..1);
        }
        self.put(small);
    }
}

impl Renderer {
    pub(crate) fn scope_resources(
        &mut self,
        kind: ScopeKind,
        cells: u32,
    ) -> std::sync::Arc<ScopeResources> {
        let key = kind.index();
        if let Some(r) = self.scopes.get(&key) {
            return r.clone();
        }
        let device = &self.gpu.device;
        let r = std::sync::Arc::new(ScopeResources {
            uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("scope uniform"),
                size: std::mem::size_of::<ScopeUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            counts: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("scope counts"),
                size: cells as u64 * 4,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        });
        self.scopes.insert(key, r.clone());
        r
    }
}

/// Reads in flight (see `Output::begin_read`).
pub struct PendingRead {
    jobs: Vec<(wgpu::Buffer, u64, u32, u32, u32)>,
    index: wgpu::SubmissionIndex,
    mapped: std::sync::mpsc::Receiver<(usize, Result<(), String>)>,
    post: Post,
}

impl Drop for PendingRead {
    fn drop(&mut self) {
        // Export may stop while the next frame is still in flight.
        for (buffer, ..) in &self.jobs {
            buffer.destroy();
        }
    }
}

/// What happens to the pixels once read.
enum Post {
    None,
    /// Luma, interleaved chroma and maybe alpha become Y, U, V (and A) planes.
    Planes {
        bytes: usize,
        alpha: bool,
    },
}

fn split_planes(mut data: Vec<Vec<u8>>, s: usize, alpha: bool) -> Vec<Vec<u8>> {
    let a = alpha.then(|| data.pop().unwrap_or_default());
    let inter = data.pop().unwrap_or_default();
    let mut out = vec![data.pop().unwrap_or_default()];
    let mut u = Vec::with_capacity(inter.len() / 2);
    let mut v = Vec::with_capacity(inter.len() / 2);
    for px in inter.chunks_exact(2 * s) {
        u.extend_from_slice(&px[..s]);
        v.extend_from_slice(&px[s..]);
    }
    out.push(u);
    out.push(v);
    if let Some(a) = a {
        out.push(a);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer() -> Option<Renderer> {
        match crate::Gpu::headless() {
            Ok(gpu) => Some(Renderer::new(gpu)),
            Err(e) if std::env::var("OPENPREMIER_REQUIRE_GPU").is_ok_and(|v| v == "1") => {
                panic!("no GPU adapter: {e}")
            }
            Err(e) => {
                eprintln!("skipping GPU readback tests: {e}");
                None
            }
        }
    }

    #[test]
    fn cancelled_mapping_fails_the_batch_and_cleans_up_other_planes() {
        let Some(mut r) = renderer() else { return };
        let texture = r.display_target(3, 2);
        let pending = r.begin_read(&[(&texture, 4), (&texture, 4)]);
        let buffers: Vec<_> = pending.jobs.iter().map(|j| j.0.clone()).collect();
        // Cancel a real asynchronous mapping before its completion callback is collected.
        buffers[0].unmap();
        let error = r
            .end_read(pending)
            .expect_err("a cancelled mapping must fail");
        assert!(error.contains("mapping 0 failed"), "{error}");
        for buffer in &buffers {
            assert!(buffer.get_mapped_range(..).is_err(), "mapping left open");
        }
        assert_eq!(r.readbacks.values().map(Vec::len).sum::<usize>(), 1);
        // Successful buffers remain reusable, including rows needing 256-byte alignment.
        assert_eq!(r.read_texture(&texture, 4).expect("next read").len(), 24);
    }

    #[test]
    fn abandoning_a_pending_read_cancels_all_mappings() {
        let Some(mut r) = renderer() else { return };
        let texture = r.display_target(3, 2);
        let pending = r.begin_read(&[(&texture, 4), (&texture, 4)]);
        let buffers: Vec<_> = pending.jobs.iter().map(|j| j.0.clone()).collect();
        drop(pending);
        for buffer in &buffers {
            assert!(buffer.get_mapped_range(..).is_err(), "mapping left open");
        }
        assert!(r.readbacks.is_empty(), "cancelled buffers entered the pool");
        assert_eq!(r.read_texture(&texture, 4).expect("next read").len(), 24);
    }

    #[test]
    fn an_unavailable_mapped_range_returns_an_error_instead_of_black_pixels() {
        let Some(mut r) = renderer() else { return };
        let texture = r.display_target(3, 2);
        let pending = r.begin_read(&[(&texture, 4)]);
        r.device()
            .poll(wgpu::PollType::Wait {
                submission_index: Some(pending.index.clone()),
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .expect("readback completes");
        // The callback reported success, but the CPU range is no longer available.
        pending.jobs[0].0.unmap();
        let error = r
            .end_read(pending)
            .expect_err("an unavailable range must fail");
        assert!(error.contains("range 0 failed"), "{error}");
        assert!(r.readbacks.is_empty(), "failed buffer entered the pool");
        assert_eq!(r.read_texture(&texture, 4).expect("next read").len(), 24);
    }
}
