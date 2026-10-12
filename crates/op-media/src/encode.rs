//! Encoding and muxing for export.
//!
//! The renderer hands over planar YUV frames already converted with the output matrix (BT.709
//! limited range for SDR, BT.2020 PQ/HLG for HDR) or RGBA for SDR alpha codecs. No transfer
//! conversion happens here beyond rearranging planes for encoders that need NV12/P010.

use std::path::Path;

use ff::format::Pixel;
use ffmpeg_next as ff;
use op_core::Rate;

use crate::{MediaError, Result, init};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum VideoCodec {
    H264,
    Hevc,
    ProRes422Hq,
    ProRes4444,
    DnxhrHq,
    Png,
}

impl VideoCodec {
    pub const ALL: [VideoCodec; 6] = [
        VideoCodec::H264,
        VideoCodec::Hevc,
        VideoCodec::ProRes422Hq,
        VideoCodec::ProRes4444,
        VideoCodec::DnxhrHq,
        VideoCodec::Png,
    ];

    pub fn label(self) -> &'static str {
        match self {
            VideoCodec::H264 => "H.264",
            VideoCodec::Hevc => "HEVC (H.265)",
            VideoCodec::ProRes422Hq => "Apple ProRes 422 HQ",
            VideoCodec::ProRes4444 => "Apple ProRes 4444",
            VideoCodec::DnxhrHq => "DNxHR HQ",
            VideoCodec::Png => "PNG (QuickTime)",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            VideoCodec::H264 | VideoCodec::Hevc => "mp4",
            _ => "mov",
        }
    }

    /// Pixel format the renderer must deliver.
    pub fn input(self) -> VideoInput {
        match self {
            VideoCodec::H264 | VideoCodec::Hevc => VideoInput::Yuv420p,
            VideoCodec::ProRes422Hq => VideoInput::Yuv422p10,
            VideoCodec::DnxhrHq => VideoInput::Yuv422p,
            VideoCodec::ProRes4444 => VideoInput::Yuva444p10,
            VideoCodec::Png => VideoInput::Rgba8,
        }
    }

    /// Candidate encoders, preferred first. Hardware encoders are tried only when asked.
    fn encoders(self, hardware: bool) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = Vec::new();
        match self {
            VideoCodec::H264 => {
                if hardware {
                    v.extend([
                        "h264_nvenc",
                        "h264_amf",
                        "h264_qsv",
                        "h264_mf",
                        "h264_videotoolbox",
                        "h264_vaapi",
                    ]);
                }
                v.push("libx264");
            }
            VideoCodec::Hevc => {
                if hardware {
                    v.extend([
                        "hevc_nvenc",
                        "hevc_amf",
                        "hevc_qsv",
                        "hevc_mf",
                        "hevc_videotoolbox",
                    ]);
                }
                v.push("libx265");
            }
            VideoCodec::ProRes422Hq | VideoCodec::ProRes4444 => v.push("prores_ks"),
            VideoCodec::DnxhrHq => v.push("dnxhd"),
            VideoCodec::Png => v.push("png"),
        }
        v
    }
}

/// True for encoders that run on the graphics card or a media engine (their failures are
/// worth retrying with the software encoder).
pub fn is_hardware_encoder(name: &str) -> bool {
    ["nvenc", "amf", "qsv", "_mf", "videotoolbox", "vaapi"]
        .iter()
        .any(|k| name.contains(k))
}

/// How H.264/HEVC streams are laid out: delivery files favour size, proxies favour seeking.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Tuning {
    /// Keyframe interval in seconds.
    pub gop_seconds: f64,
    pub b_frames: u32,
    /// x264/x265 preset.
    pub preset: &'static str,
}

impl Tuning {
    pub const DELIVERY: Tuning = Tuning {
        gop_seconds: 2.0,
        b_frames: 2,
        preset: "medium",
    };
    /// Editing proxies: a keyframe every half second and no reordered frames, so any frame is
    /// reached by decoding a handful of others.
    pub const PROXY: Tuning = Tuning {
        gop_seconds: 0.5,
        b_frames: 0,
        preset: "veryfast",
    };
}

/// Adds which step failed to an FFmpeg error ("Invalid argument" alone says little).
fn step(what: impl std::fmt::Display) -> impl FnOnce(MediaError) -> MediaError {
    move |e| MediaError::Failed(format!("{what}: {e}"))
}

/// Layout of the frames given to `Muxer::push_video`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum VideoInput {
    Yuv420p,
    Yuv420p10,
    Yuv422p,
    Yuv422p10,
    Yuva444p10,
    Rgba8,
}

impl VideoInput {
    fn pixel(self) -> Pixel {
        match self {
            VideoInput::Yuv420p => Pixel::YUV420P,
            VideoInput::Yuv420p10 => Pixel::YUV420P10LE,
            VideoInput::Yuv422p => Pixel::YUV422P,
            VideoInput::Yuv422p10 => Pixel::YUV422P10LE,
            VideoInput::Yuva444p10 => Pixel::YUVA444P10LE,
            VideoInput::Rgba8 => Pixel::RGBA,
        }
    }

    /// (plane count, chroma shift x, chroma shift y, bytes per sample) for YUV inputs.
    pub fn planes(self) -> (usize, u32, u32, usize) {
        match self {
            VideoInput::Yuv420p => (3, 1, 1, 1),
            VideoInput::Yuv420p10 => (3, 1, 1, 2),
            VideoInput::Yuv422p => (3, 1, 0, 1),
            VideoInput::Yuv422p10 => (3, 1, 0, 2),
            VideoInput::Yuva444p10 => (4, 0, 0, 2),
            VideoInput::Rgba8 => (1, 0, 0, 4),
        }
    }

    pub fn is_yuv(self) -> bool {
        !matches!(self, VideoInput::Rgba8)
    }

    pub fn bits(self) -> u32 {
        match self {
            VideoInput::Yuv420p10 | VideoInput::Yuv422p10 | VideoInput::Yuva444p10 => 10,
            _ => 8,
        }
    }
}

/// Delivery encoding. HDR uses BT.2020 primaries and limited-range 10-bit YUV.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryColor {
    #[default]
    Bt709,
    Pq,
    Hlg,
}

impl DeliveryColor {
    pub const ALL: [Self; 3] = [Self::Bt709, Self::Pq, Self::Hlg];
    pub fn is_hdr(self) -> bool {
        self != Self::Bt709
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Bt709 => "SDR — Rec.709",
            Self::Pq => "HDR — Rec.2100 PQ (10 bit)",
            Self::Hlg => "HDR — Rec.2100 HLG (10 bit)",
        }
    }
    fn tags(
        self,
    ) -> (
        ff::color::Space,
        ff::color::Primaries,
        ff::color::TransferCharacteristic,
    ) {
        use ff::color::{Primaries, Space, TransferCharacteristic as Transfer};
        match self {
            Self::Bt709 => (Space::BT709, Primaries::BT709, Transfer::BT709),
            Self::Pq => (Space::BT2020NCL, Primaries::BT2020, Transfer::SMPTE2084),
            Self::Hlg => (Space::BT2020NCL, Primaries::BT2020, Transfer::ARIB_STD_B67),
        }
    }
}

/// Authored HDR10 metadata, not a measurement of the exported content. Mastering primaries
/// are BT.2020/D65; luminance is in cd/m². HLG does not use HDR10 static metadata.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Hdr10Metadata {
    pub mastering_max_nits: f64,
    pub mastering_min_nits: f64,
    pub max_cll: u32,
    pub max_fall: u32,
}
impl Default for Hdr10Metadata {
    fn default() -> Self {
        Self {
            mastering_max_nits: 1000.0,
            mastering_min_nits: 0.0001,
            max_cll: 1000,
            max_fall: 400,
        }
    }
}
impl Hdr10Metadata {
    pub fn validate(self) -> Result {
        if !self.mastering_max_nits.is_finite()
            || !self.mastering_min_nits.is_finite()
            || !(100.0..=10000.0).contains(&self.mastering_max_nits)
            || self.mastering_min_nits < 0.0
            || self.mastering_min_nits >= self.mastering_max_nits
            || self.max_cll > 10000
            || self.max_fall > self.max_cll
        {
            return Err(MediaError::Unsupported(
                "invalid HDR10 luminance metadata".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VideoSettings {
    pub color: DeliveryColor,
    pub hdr10: Hdr10Metadata,
    pub codec: VideoCodec,
    pub width: u32,
    pub height: u32,
    pub rate: Rate,
    /// Target bitrate for H.264/HEVC; None uses constant quality.
    pub bitrate_kbps: Option<u32>,
    /// Constant-quality level (lower is better), used when no bitrate is set.
    pub quality: u8,
    pub hardware: bool,
}

impl VideoSettings {
    pub fn input(&self) -> VideoInput {
        if self.color.is_hdr() && self.codec == VideoCodec::Hevc {
            VideoInput::Yuv420p10
        } else {
            self.codec.input()
        }
    }
    /// Checked before the destination is opened; HDR can never downgrade to 8 bits.
    pub fn validate_delivery(&self) -> Result {
        if self.color.is_hdr()
            && !matches!(
                self.codec,
                VideoCodec::Hevc | VideoCodec::ProRes422Hq | VideoCodec::ProRes4444
            )
        {
            return Err(MediaError::Unsupported(
                "HDR requires HEVC or ProRes (10 bit)".into(),
            ));
        }
        if self.width == 0 || self.height == 0 {
            return Err(MediaError::Unsupported("empty video dimensions".into()));
        }
        if self.color == DeliveryColor::Pq {
            self.hdr10.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AudioCodec {
    Aac,
    Pcm16,
    Pcm24,
    Mp3,
}

impl AudioCodec {
    pub fn label(self) -> &'static str {
        match self {
            AudioCodec::Aac => "AAC",
            AudioCodec::Pcm16 => "PCM 16-bit",
            AudioCodec::Pcm24 => "PCM 24-bit",
            AudioCodec::Mp3 => "MP3",
        }
    }

    fn encoder(self) -> &'static str {
        match self {
            AudioCodec::Aac => "aac",
            AudioCodec::Pcm16 => "pcm_s16le",
            AudioCodec::Pcm24 => "pcm_s24le",
            AudioCodec::Mp3 => "libmp3lame",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioSettings {
    pub codec: AudioCodec,
    pub rate: u32,
    pub channels: u16,
    pub bitrate_kbps: u32,
}

struct VideoOut {
    color: DeliveryColor,
    encoder: ff::encoder::Video,
    stream: usize,
    time_base: ff::Rational,
    input: VideoInput,
    frame: ff::frame::Video,
    staging: Option<ff::frame::Video>,
    scaler: Option<ff::software::scaling::Context>,
    next_pts: i64,
    name: String,
}

struct AudioOut {
    encoder: ff::encoder::Audio,
    stream: usize,
    time_base: ff::Rational,
    frame_size: usize,
    channels: usize,
    pending: Vec<f32>,
    next_pts: i64,
    format: ff::format::Sample,
}

pub struct Muxer {
    octx: ff::format::context::Output,
    video: Option<VideoOut>,
    audio: Option<AudioOut>,
    header_written: bool,
}

fn rational(rate: Rate) -> ff::Rational {
    ff::Rational::new(rate.num as i32, rate.den as i32)
}

impl Muxer {
    /// Creates the output file with the requested streams. The container follows the file
    /// extension.
    pub fn create(
        path: &Path,
        video: Option<&VideoSettings>,
        audio: Option<&AudioSettings>,
    ) -> Result<Muxer> {
        Self::create_tuned(path, video, audio, Tuning::DELIVERY)
    }

    /// Like `create`, with the stream layout of `tuning` for H.264/HEVC.
    pub fn create_tuned(
        path: &Path,
        video: Option<&VideoSettings>,
        audio: Option<&AudioSettings>,
        tuning: Tuning,
    ) -> Result<Muxer> {
        if let Some(video) = video {
            video.validate_delivery()?;
        }
        init();
        let mut octx = ff::format::output(path)?;
        let global_header = octx
            .format()
            .flags()
            .contains(ff::format::Flags::GLOBAL_HEADER);
        let video_out = match video {
            Some(v) => Some(open_video(&mut octx, v, global_header, tuning)?),
            None => None,
        };
        let audio_out = match audio {
            Some(a) => Some(open_audio(&mut octx, a, global_header)?),
            None => None,
        };
        let mut m = Muxer {
            octx,
            video: video_out,
            audio: audio_out,
            header_written: false,
        };
        m.octx
            .write_header()
            .map_err(MediaError::from)
            .map_err(step("writing the file header"))?;
        m.header_written = true;
        // the muxer may change stream time bases while writing the header
        if let Some(v) = &mut m.video {
            v.time_base = m.octx.stream(v.stream).unwrap().time_base();
        }
        if let Some(a) = &mut m.audio {
            a.time_base = m.octx.stream(a.stream).unwrap().time_base();
        }
        Ok(m)
    }

    /// Name of the video encoder in use (e.g. `h264_nvenc`).
    pub fn video_encoder(&self) -> Option<&str> {
        self.video.as_ref().map(|v| v.name.as_str())
    }

    pub fn audio_frame_size(&self) -> usize {
        self.audio.as_ref().map(|a| a.frame_size).unwrap_or(1024)
    }

    /// Converts a CPU compositor's straight RGBA8 into the same delivery layout as GPU export.
    /// BT.709 limited YUV, or full-range RGBA for alpha codecs. Reuses FFmpeg in this process.
    pub fn push_rgba(&mut self, pixels: &[u8]) -> Result {
        let Some(v) = &self.video else {
            return Ok(());
        };
        if v.color.is_hdr() {
            return Err(MediaError::Unsupported(
                "HDR requires floating-point delivery conversion; RGBA8 cannot carry HDR".into(),
            ));
        }
        let (w, h) = (v.frame.width(), v.frame.height());
        let expected = (w as usize)
            .checked_mul(h as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| MediaError::Failed("RGBA frame size overflow".into()))?;
        if pixels.len() != expected {
            return Err(MediaError::Failed("RGBA frame size mismatch".into()));
        }
        if v.input == VideoInput::Rgba8 {
            return self.push_video(&[pixels]);
        }
        let mut input = ff::frame::Video::new(Pixel::RGBA, w, h);
        for y in 0..h as usize {
            let stride = input.stride(0);
            input.data_mut(0)[y * stride..y * stride + w as usize * 4]
                .copy_from_slice(&pixels[y * w as usize * 4..(y + 1) * w as usize * 4]);
            if v.input != VideoInput::Yuva444p10 {
                // Delivery formats without alpha composite onto black, like native export.
                for pixel in input.data_mut(0)[y * stride..y * stride + w as usize * 4]
                    .as_chunks_mut::<4>()
                    .0
                {
                    let alpha = u16::from(pixel[3]);
                    for channel in &mut pixel[..3] {
                        *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
                    }
                    pixel[3] = 255;
                }
            }
        }
        let pixel = v.staging.as_ref().unwrap_or(&v.frame).format();
        let mut scaler = ff::software::scaling::Context::get(
            Pixel::RGBA,
            w,
            h,
            pixel,
            w,
            h,
            ff::software::scaling::flag::Flags::BILINEAR,
        )?;
        // SAFETY: the context is live; swscale copies the static coefficient table. RGB input
        // is full range, delivery YUV is BT.709 limited range, with unity brightness/contrast.
        unsafe {
            let coefficients = ff::ffi::sws_getCoefficients(ff::ffi::SWS_CS_ITU709);
            if ff::ffi::sws_setColorspaceDetails(
                scaler.as_mut_ptr(),
                coefficients,
                1,
                coefficients,
                0,
                0,
                1 << 16,
                1 << 16,
            ) < 0
            {
                return Err(MediaError::Failed(
                    "cannot configure CPU delivery color conversion".into(),
                ));
            }
        }
        let mut output = ff::frame::Video::empty();
        scaler.run(&input, &mut output)?;
        let (count, cx, cy, bps) = v.input.planes();
        let mut planes = Vec::new();
        for i in 0..count {
            let (pw, ph) = if i == 0 || i == 3 {
                (w, h)
            } else {
                (w.div_ceil(1 << cx), h.div_ceil(1 << cy))
            };
            let row = pw as usize * bps;
            let stride = output.stride(i);
            let data = output.data(i);
            let mut packed = Vec::with_capacity(row * ph as usize);
            for y in 0..ph as usize {
                packed.extend_from_slice(&data[y * stride..y * stride + row]);
            }
            planes.push(packed);
        }
        self.push_video(&planes.iter().map(Vec::as_slice).collect::<Vec<_>>())
    }

    /// Encodes one frame. `planes` are tightly packed rows in the layout of `VideoInput`.
    pub fn push_video(&mut self, planes: &[&[u8]]) -> Result {
        let Some(v) = &mut self.video else {
            return Ok(());
        };
        let (count, cx, cy, bps) = v.input.planes();
        let (w, h) = (v.frame.width() as usize, v.frame.height() as usize);
        if planes.len() != count {
            return Err(MediaError::Failed("video plane count mismatch".into()));
        }
        for (i, plane) in planes.iter().enumerate() {
            let (pw, ph) = if i == 0 || i == 3 || !v.input.is_yuv() {
                (w, h)
            } else {
                (w.div_ceil(1 << cx), h.div_ceil(1 << cy))
            };
            if plane.len() != pw * ph * bps {
                return Err(MediaError::Failed(format!("video plane {i} size mismatch")));
            }
        }
        let target = v.staging.as_mut().unwrap_or(&mut v.frame);
        for (i, plane) in planes.iter().enumerate().take(count) {
            let (pw, ph) = if i == 0 || i == 3 || !v.input.is_yuv() {
                (w, h)
            } else {
                (w.div_ceil(1 << cx), h.div_ceil(1 << cy))
            };
            let row = pw * bps;
            let stride = target.stride(i);
            let dst = target.data_mut(i);
            for y in 0..ph {
                let src = &plane[y * row..][..row];
                dst[y * stride..][..row].copy_from_slice(src);
            }
        }
        if let (Some(scaler), Some(staging)) = (&mut v.scaler, &v.staging) {
            scaler
                .run(staging, &mut v.frame)
                .map_err(MediaError::from)
                .map_err(step("converting the frame"))?;
        }
        v.frame.set_pts(Some(v.next_pts));
        v.next_pts += 1;
        v.encoder
            .send_frame(&v.frame)
            .map_err(MediaError::from)
            .map_err(step(format!("video encoder {}", v.name)))?;
        drain_video(&mut self.octx, v)?;
        Ok(())
    }

    /// Queues interleaved float samples.
    pub fn push_audio(&mut self, interleaved: &[f32]) -> Result {
        let Some(a) = &mut self.audio else {
            return Ok(());
        };
        a.pending.extend_from_slice(interleaved);
        while a.pending.len() >= a.frame_size * a.channels {
            let chunk: Vec<f32> = a.pending.drain(..a.frame_size * a.channels).collect();
            encode_audio(&mut self.octx, a, &chunk)?;
        }
        Ok(())
    }

    /// Flushes encoders and writes the trailer.
    pub fn finish(mut self) -> Result {
        if let Some(a) = &mut self.audio {
            if !a.pending.is_empty() {
                let chunk = std::mem::take(&mut a.pending);
                encode_audio(&mut self.octx, a, &chunk)?;
            }
            a.encoder
                .send_eof()
                .map_err(MediaError::from)
                .map_err(step("audio encoder"))?;
            drain_audio(&mut self.octx, a)?;
        }
        if let Some(v) = &mut self.video {
            v.encoder
                .send_eof()
                .map_err(MediaError::from)
                .map_err(step(format!("video encoder {}", v.name)))?;
            drain_video(&mut self.octx, v)?;
        }
        self.octx
            .write_trailer()
            .map_err(MediaError::from)
            .map_err(step("finishing the file"))?;
        Ok(())
    }
}

fn open_video(
    octx: &mut ff::format::context::Output,
    s: &VideoSettings,
    global_header: bool,
    tuning: Tuning,
) -> Result<VideoOut> {
    let mut last_err = None;
    for name in s.codec.encoders(s.hardware) {
        let Some(codec) = ff::encoder::find_by_name(name) else {
            continue;
        };
        match try_video(octx, s, codec, name, global_header, tuning) {
            Ok(v) => return Ok(v),
            Err(e) => {
                log::info!("video encoder {name} unavailable: {e}");
                last_err = Some(e);
            }
        }
    }
    Err(last_err
        .unwrap_or_else(|| MediaError::Unsupported(format!("no encoder for {}", s.codec.label()))))
}

fn try_video(
    octx: &mut ff::format::context::Output,
    s: &VideoSettings,
    codec: ff::Codec,
    name: &str,
    global_header: bool,
    tuning: Tuning,
) -> Result<VideoOut> {
    let input = s.input();
    let wanted = input.pixel();
    let supported: Vec<Pixel> = codec
        .video()
        .ok()
        .and_then(|v| v.formats())
        .map(|f| f.collect())
        .unwrap_or_default();
    let enc_format = if supported.is_empty() || supported.contains(&wanted) {
        wanted
    } else if supported.contains(&Pixel::NV12) && wanted == Pixel::YUV420P {
        Pixel::NV12
    } else if supported.contains(&Pixel::P010LE) && wanted == Pixel::YUV420P10LE {
        Pixel::P010LE
    } else {
        return Err(MediaError::Unsupported(format!(
            "{name} does not accept {wanted:?}"
        )));
    };
    let mut ctx = ff::codec::context::Context::new_with_codec(codec);
    // FFmpeg encodes on one core unless told otherwise; ProRes, DNxHR and PNG split each frame
    // into slices, x264 and x265 take the count for their own threads (0: one per core)
    ctx.set_threading(ff::threading::Config {
        kind: if matches!(s.codec, VideoCodec::Png) {
            ff::threading::Type::Frame
        } else {
            ff::threading::Type::Slice
        },
        count: 0,
    });
    let mut enc = ctx.encoder().video()?;
    enc.set_width(s.width);
    enc.set_height(s.height);
    enc.set_format(enc_format);
    let tb = ff::Rational::new(s.rate.den as i32, s.rate.num as i32);
    enc.set_time_base(tb);
    enc.set_frame_rate(Some(rational(s.rate)));
    enc.set_aspect_ratio(ff::Rational::new(1, 1));
    if input.is_yuv() {
        let (space, primaries, transfer) = s.color.tags();
        enc.set_colorspace(space);
        enc.set_color_range(ff::color::Range::MPEG);
        enc.set_color_primaries(primaries);
        enc.set_color_transfer_characteristic(transfer);
    }
    let mut opts = ff::Dictionary::new();
    let delivery = matches!(s.codec, VideoCodec::H264 | VideoCodec::Hevc);
    if delivery {
        enc.set_gop(((s.rate.as_f64() * tuning.gop_seconds).round() as u32).max(1));
        // VideoToolbox can give B-frame timestamps the MP4 muxer rejects ("Invalid argument")
        let videotoolbox = name.contains("videotoolbox");
        enc.set_max_b_frames(if videotoolbox {
            0
        } else {
            tuning.b_frames as usize
        });
        if videotoolbox {
            // the software path of VideoToolbox when the media engine is busy or missing
            opts.set("allow_sw", "1");
        }
        match s.bitrate_kbps {
            Some(kbps) => {
                enc.set_bit_rate(kbps as usize * 1000);
                enc.set_max_bit_rate(kbps as usize * 1500);
            }
            None => {
                let q = s.quality.to_string();
                if name.starts_with("libx26") {
                    opts.set("crf", &q);
                } else if name.contains("nvenc") {
                    opts.set("rc", "vbr");
                    opts.set("cq", &q);
                    opts.set("b", "0");
                } else if name.contains("qsv") {
                    opts.set("global_quality", &q);
                } else {
                    // other hardware encoders take a bitrate
                    let px = (s.width * s.height) as f64 * s.rate.as_f64();
                    enc.set_bit_rate((px * 0.12) as usize);
                }
            }
        }
        if name.starts_with("libx26") {
            opts.set("preset", tuning.preset);
        } else if name.contains("nvenc") {
            opts.set("preset", "p5");
        }
        if name == "libx264" {
            opts.set("profile", "high");
        }
        if name == "libx265" && s.color == DeliveryColor::Pq {
            let h = s.hdr10;
            opts.set("x265-params", &format!("hdr10=1:master-display=G(8500,39850)B(6550,2300)R(35400,14600)WP(15635,16450)L({},{})\u{003a}max-cll={},{}",
                (h.mastering_max_nits * 10000.0).round() as u64,
                (h.mastering_min_nits * 10000.0).round() as u64, h.max_cll, h.max_fall));
        }
        if s.codec == VideoCodec::Hevc && s.color.is_hdr() {
            opts.set("profile", "main10");
        }
    }
    match s.codec {
        VideoCodec::ProRes422Hq => opts.set("profile", "3"),
        VideoCodec::ProRes4444 => opts.set("profile", "4"),
        VideoCodec::DnxhrHq => opts.set("profile", "dnxhr_hq"),
        _ => {}
    }
    if global_header {
        enc.set_flags(ff::codec::Flags::GLOBAL_HEADER);
    }
    let encoder = enc.open_as_with(codec, opts)?;
    let mut stream = octx.add_stream(codec)?;
    stream.set_parameters(&encoder);
    stream.set_time_base(tb);
    let index = stream.index();
    let mut frame = ff::frame::Video::new(enc_format, s.width, s.height);
    set_frame_color(&mut frame, s.color);
    if s.color == DeliveryColor::Pq {
        attach_hdr10(&mut frame, s.hdr10)?;
        // Stream side data lets MOV/MP4 retain mastering information even for ProRes.
        let mut parameters = stream.parameters();
        copy_hdr10_to_stream(&frame, &mut parameters)?;
    }
    let (staging, scaler) = if enc_format != wanted {
        let mut staging = ff::frame::Video::new(wanted, s.width, s.height);
        set_frame_color(&mut staging, s.color);
        if s.color == DeliveryColor::Pq {
            attach_hdr10(&mut staging, s.hdr10)?;
        }
        let scaler = ff::software::scaling::Context::get(
            wanted,
            s.width,
            s.height,
            enc_format,
            s.width,
            s.height,
            ff::software::scaling::Flags::POINT,
        )?;
        (Some(staging), Some(scaler))
    } else {
        (None, None)
    };
    Ok(VideoOut {
        encoder,
        stream: index,
        time_base: tb,
        input,
        frame,
        staging,
        scaler,
        next_pts: 0,
        name: name.to_string(),
        color: s.color,
    })
}

fn set_frame_color(frame: &mut ff::frame::Video, color: DeliveryColor) {
    let (space, primaries, transfer) = color.tags();
    frame.set_color_space(space);
    frame.set_color_range(ff::color::Range::MPEG);
    frame.set_color_primaries(primaries);
    frame.set_color_transfer_characteristic(transfer);
}

fn attach_hdr10(frame: &mut ff::frame::Video, h: Hdr10Metadata) -> Result {
    // SAFETY: FFmpeg allocates side data owned by this live frame. No pointer escapes and
    // the allocations are checked before dereferencing. These are the FFmpeg 8 public APIs.
    unsafe {
        let mastering = hdr_ffi::av_mastering_display_metadata_create_side_data(frame.as_mut_ptr());
        let light = hdr_ffi::av_content_light_metadata_create_side_data(frame.as_mut_ptr());
        if mastering.is_null() || light.is_null() {
            return Err(MediaError::Failed("cannot allocate HDR10 metadata".into()));
        }
        let xy = |n| ff::ffi::AVRational { num: n, den: 50000 };
        (*mastering).display_primaries = [
            [xy(35400), xy(14600)],
            [xy(8500), xy(39850)],
            [xy(6550), xy(2300)],
        ];
        (*mastering).white_point = [xy(15635), xy(16450)];
        (*mastering).max_luminance = ff::ffi::AVRational {
            num: (h.mastering_max_nits * 10000.0).round() as i32,
            den: 10000,
        };
        (*mastering).min_luminance = ff::ffi::AVRational {
            num: (h.mastering_min_nits * 10000.0).round() as i32,
            den: 10000,
        };
        (*mastering).has_primaries = 1;
        (*mastering).has_luminance = 1;
        (*light).max_cll = h.max_cll;
        (*light).max_fall = h.max_fall;
    }
    Ok(())
}

// ffmpeg-sys does not include mastering_display_metadata.h. These declarations mirror its
// public FFmpeg 8 fields; the library allocates both structs so their size is never assumed.
mod hdr_ffi {
    use ffmpeg_next::ffi::{AVFrame, AVRational};
    #[repr(C)]
    pub struct Mastering {
        pub display_primaries: [[AVRational; 2]; 3],
        pub white_point: [AVRational; 2],
        pub min_luminance: AVRational,
        pub max_luminance: AVRational,
        pub has_primaries: std::ffi::c_int,
        pub has_luminance: std::ffi::c_int,
    }
    #[repr(C)]
    pub struct Light {
        pub max_cll: std::ffi::c_uint,
        pub max_fall: std::ffi::c_uint,
    }
    unsafe extern "C" {
        pub fn av_mastering_display_metadata_create_side_data(
            frame: *mut AVFrame,
        ) -> *mut Mastering;
        pub fn av_content_light_metadata_create_side_data(frame: *mut AVFrame) -> *mut Light;
    }
}

fn copy_hdr10_to_stream(
    frame: &ff::frame::Video,
    parameters: &mut ff::codec::Parameters,
) -> Result {
    // SAFETY: packet allocations become owned by the stream codec parameters. Side data
    // remains owned by the frame; only the payload bytes are copied using its recorded size.
    unsafe {
        let p = &mut *parameters.as_mut_ptr();
        for (frame_kind, packet_kind) in [
            (
                ff::ffi::AVFrameSideDataType::AV_FRAME_DATA_MASTERING_DISPLAY_METADATA,
                ff::ffi::AVPacketSideDataType::AV_PKT_DATA_MASTERING_DISPLAY_METADATA,
            ),
            (
                ff::ffi::AVFrameSideDataType::AV_FRAME_DATA_CONTENT_LIGHT_LEVEL,
                ff::ffi::AVPacketSideDataType::AV_PKT_DATA_CONTENT_LIGHT_LEVEL,
            ),
        ] {
            let source = ff::ffi::av_frame_get_side_data(frame.as_ptr(), frame_kind);
            if source.is_null() {
                return Err(MediaError::Failed("missing HDR10 side data".into()));
            }
            let dest = ff::ffi::av_packet_side_data_new(
                &mut p.coded_side_data,
                &mut p.nb_coded_side_data,
                packet_kind,
                (*source).size,
                0,
            );
            if dest.is_null() {
                return Err(MediaError::Failed(
                    "cannot allocate HDR10 stream metadata".into(),
                ));
            }
            std::ptr::copy_nonoverlapping((*source).data, (*dest).data, (*source).size);
        }
    }
    Ok(())
}

fn open_audio(
    octx: &mut ff::format::context::Output,
    s: &AudioSettings,
    global_header: bool,
) -> Result<AudioOut> {
    let name = s.codec.encoder();
    let codec = ff::encoder::find_by_name(name)
        .ok_or_else(|| MediaError::Unsupported(format!("no {name} encoder")))?;
    let ctx = ff::codec::context::Context::new_with_codec(codec);
    let mut enc = ctx.encoder().audio()?;
    let formats: Vec<ff::format::Sample> = codec
        .audio()
        .ok()
        .and_then(|a| a.formats())
        .map(|f| f.collect())
        .unwrap_or_default();
    let prefer = [
        ff::format::Sample::F32(ff::format::sample::Type::Planar),
        ff::format::Sample::F32(ff::format::sample::Type::Packed),
        ff::format::Sample::I32(ff::format::sample::Type::Packed),
        ff::format::Sample::I16(ff::format::sample::Type::Packed),
        ff::format::Sample::I16(ff::format::sample::Type::Planar),
        ff::format::Sample::I32(ff::format::sample::Type::Planar),
    ];
    let format = prefer
        .iter()
        .copied()
        .find(|f| formats.is_empty() || formats.contains(f))
        .unwrap_or(prefer[0]);
    enc.set_rate(s.rate as i32);
    enc.set_channel_layout(ff::ChannelLayout::default(s.channels as i32));
    enc.set_format(format);
    enc.set_bit_rate(s.bitrate_kbps as usize * 1000);
    let tb = ff::Rational::new(1, s.rate as i32);
    enc.set_time_base(tb);
    if global_header {
        enc.set_flags(ff::codec::Flags::GLOBAL_HEADER);
    }
    let encoder = enc.open_as(codec)?;
    let frame_size = match encoder.frame_size() {
        0 => 1024,
        n => n as usize,
    };
    let mut stream = octx.add_stream(codec)?;
    stream.set_parameters(&encoder);
    stream.set_time_base(tb);
    let index = stream.index();
    Ok(AudioOut {
        encoder,
        stream: index,
        time_base: tb,
        frame_size,
        channels: s.channels as usize,
        pending: Vec::new(),
        next_pts: 0,
        format,
    })
}

fn encode_audio(
    octx: &mut ff::format::context::Output,
    a: &mut AudioOut,
    interleaved: &[f32],
) -> Result {
    let n = interleaved.len() / a.channels;
    if n == 0 {
        return Ok(());
    }
    let mut frame =
        ff::frame::Audio::new(a.format, n, ff::ChannelLayout::default(a.channels as i32));
    frame.set_rate(a.encoder.rate());
    match a.format {
        ff::format::Sample::F32(ff::format::sample::Type::Planar) => {
            for c in 0..a.channels {
                let plane = frame.plane_mut::<f32>(c);
                for (i, v) in plane.iter_mut().enumerate().take(n) {
                    *v = interleaved[i * a.channels + c];
                }
            }
        }
        ff::format::Sample::F32(ff::format::sample::Type::Packed) => {
            let data = frame.data_mut(0);
            for (i, v) in interleaved.iter().enumerate() {
                data[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        ff::format::Sample::I32(t) => {
            let q = |v: f32| (v.clamp(-1.0, 1.0) as f64 * 2147483647.0) as i32;
            for c in 0..a.channels {
                for i in 0..n {
                    let (plane, at) = if t == ff::format::sample::Type::Planar {
                        (c, i)
                    } else {
                        (0, i * a.channels + c)
                    };
                    frame.data_mut(plane)[at * 4..at * 4 + 4]
                        .copy_from_slice(&q(interleaved[i * a.channels + c]).to_le_bytes());
                }
            }
        }
        ff::format::Sample::I16(t) => {
            let q = |v: f32| (v.clamp(-1.0, 1.0) * 32767.0).round() as i16;
            for c in 0..a.channels {
                for i in 0..n {
                    let (plane, at) = if t == ff::format::sample::Type::Planar {
                        (c, i)
                    } else {
                        (0, i * a.channels + c)
                    };
                    frame.data_mut(plane)[at * 2..at * 2 + 2]
                        .copy_from_slice(&q(interleaved[i * a.channels + c]).to_le_bytes());
                }
            }
        }
        f => return Err(MediaError::Unsupported(format!("sample format {f:?}"))),
    }
    frame.set_pts(Some(a.next_pts));
    a.next_pts += n as i64;
    a.encoder
        .send_frame(&frame)
        .map_err(MediaError::from)
        .map_err(step("audio encoder"))?;
    drain_audio(octx, a)
}

fn drain_video(octx: &mut ff::format::context::Output, v: &mut VideoOut) -> Result {
    let mut packet = ff::Packet::empty();
    loop {
        match v.encoder.receive_packet(&mut packet) {
            Ok(()) => {
                packet.set_stream(v.stream);
                // without a duration the MP4 edit list would end before the last frame
                if packet.duration() <= 0 {
                    packet.set_duration(1);
                }
                packet.rescale_ts(v.encoder.time_base(), v.time_base);
                packet
                    .write_interleaved(octx)
                    .map_err(MediaError::from)
                    .map_err(step(format!("writing video from {}", v.name)))?;
            }
            Err(ff::Error::Eof) => return Ok(()),
            Err(ff::Error::Other { errno }) if errno == ff::error::EAGAIN => return Ok(()),
            Err(e) => return Err(step(format!("video encoder {}", v.name))(e.into())),
        }
    }
}

fn drain_audio(octx: &mut ff::format::context::Output, a: &mut AudioOut) -> Result {
    let mut packet = ff::Packet::empty();
    loop {
        match a.encoder.receive_packet(&mut packet) {
            Ok(()) => {
                packet.set_stream(a.stream);
                packet.rescale_ts(a.encoder.time_base(), a.time_base);
                packet
                    .write_interleaved(octx)
                    .map_err(MediaError::from)
                    .map_err(step("writing audio"))?;
            }
            Err(ff::Error::Eof) => return Ok(()),
            Err(ff::Error::Other { errno }) if errno == ff::error::EAGAIN => return Ok(()),
            Err(e) => return Err(step("audio encoder")(e.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{audio, probe, video::VideoDecoder};

    /// Writes a 2 s 320x240 H.264 + AAC file whose frame `i` has luma `16 + 4 * i` and a
    /// 1 kHz tone, then reads it back through probe, the decoder and the audio conform.
    #[test]
    fn encode_probe_decode_conform() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.mp4");
        let (w, h) = (320u32, 240u32);
        let v = VideoSettings {
            color: Default::default(),
            hdr10: Default::default(),
            codec: VideoCodec::H264,
            width: w,
            height: h,
            rate: Rate::FPS_25,
            bitrate_kbps: None,
            quality: 10,
            hardware: false,
        };
        let a = AudioSettings {
            codec: AudioCodec::Aac,
            rate: 48000,
            channels: 2,
            bitrate_kbps: 192,
        };
        let mut m = Muxer::create(&path, Some(&v), Some(&a)).unwrap();
        assert_eq!(m.video_encoder(), Some("libx264"));
        let chroma = vec![128u8; (w / 2 * h / 2) as usize];
        for i in 0..50u32 {
            let luma = vec![(16 + 4 * i) as u8; (w * h) as usize];
            m.push_video(&[&luma, &chroma, &chroma]).unwrap();
        }
        let samples: Vec<f32> = (0..96000)
            .flat_map(|n| {
                let s = (n as f32 * 1000.0 * std::f32::consts::TAU / 48000.0).sin() * 0.5;
                [s, s]
            })
            .collect();
        m.push_audio(&samples).unwrap();
        m.finish().unwrap();

        let asset = probe::probe(&path).unwrap();
        let vs = asset.video.as_ref().unwrap();
        assert_eq!(
            (vs.width, vs.height, vs.rate, vs.frames),
            (320, 240, Rate::FPS_25, 50)
        );
        assert_eq!(asset.audio.len(), 1);
        assert_eq!(asset.audio[0].sample_rate, 48000);

        let mut dec = VideoDecoder::open(&path, None, None, vs.color, false).unwrap();
        for i in [0i64, 17, 18, 40, 3, 49] {
            let f = dec.frame(i).unwrap();
            assert_eq!(f.index, i);
            let y = f.planes[0].data[(120 * f.planes[0].bytes_per_row + 160) as usize] as i32;
            assert!((y - (16 + 4 * i as i32)).abs() <= 2, "frame {i}: luma {y}");
        }

        let cache = dir.path().join("t.f32");
        let peaks = dir.path().join("t.peaks");
        let info = audio::conform(&path, 0, vs.start, &cache, &peaks, &mut |_| true).unwrap();
        assert_eq!((info.rate, info.channels), (48000, 2));
        assert!((info.samples - 96000).abs() < 2048, "{}", info.samples);
        let c = audio::ConformedAudio::open(&cache).unwrap();
        let mut buf = vec![0f32; 4800 * 2];
        c.read(24000, &mut buf);
        let peak = buf.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 0.05, "{peak}");
        let p = audio::Peaks::open(&peaks).unwrap();
        let (lo, hi) = p.range(0, 24000, 28800);
        assert!(hi > 0.4 && lo < -0.4);
    }
}
