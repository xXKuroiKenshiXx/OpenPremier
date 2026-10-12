//! Coalesced asynchronous fframes previews. At most one frame runs and one waits.
use crate::media::PreviewFrames;
use crate::render_engine::{FframesRenderer, RenderEngine, validate_fframes_hybrid};
use op_core::*;
use op_render::{Gpu, Renderer, Request};
use parking_lot::Mutex;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

pub struct PreviewRequest<K> {
    pub key: K,
    pub project: Arc<Project>,
    pub sequence: SequenceId,
    pub time: SeqTime,
    pub scale: f32,
    pub engine: RenderEngine,
    pub frames: PreviewFrames,
}
pub struct PreviewResult<K> {
    pub key: K,
    pub width: u32,
    pub height: u32,
    pub pixels: Result<Vec<u8>, String>,
    pub missing: u32,
}
type Pending<K> = Arc<Mutex<Option<PreviewRequest<K>>>>;
type Completed<K> = Arc<Mutex<Option<PreviewResult<K>>>>;
pub struct FframesPreview<K> {
    pending: Pending<K>,
    completed: Completed<K>,
    wake: crossbeam_channel::Sender<()>,
    cancel: Arc<AtomicBool>,
}
impl<K: Send + 'static> FframesPreview<K> {
    pub fn new(gpu: Arc<Gpu>) -> Self {
        let pending: Pending<K> = Arc::new(Mutex::new(None));
        let completed: Completed<K> = Arc::new(Mutex::new(None));
        let cancel = Arc::new(AtomicBool::new(false));
        let (wake, receive) = crossbeam_channel::bounded(1);
        let (requests, results, stop) = (pending.clone(), completed.clone(), cancel.clone());
        std::thread::spawn(move || {
            let mut processor: Option<(RenderEngine, Renderer)> = None;
            let mut worker: Option<(RenderEngine, u32, u32, FframesRenderer)> = None;
            while receive.recv().is_ok() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let Some(request) = requests.lock().take() else {
                    continue;
                };
                let seq = request.project.sequence(request.sequence);
                let (w, h) = seq
                    .map(|s| {
                        (
                            (s.settings.width as f32 * request.scale).round().max(1.0) as u32,
                            (s.settings.height as f32 * request.scale).round().max(1.0) as u32,
                        )
                    })
                    .unwrap_or((1, 1));
                let mut missing = 0;
                let pixels = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || -> Result<Vec<u8>, String> {
                        validate_fframes_hybrid(&request.project, request.sequence)?;
                        if worker
                            .as_ref()
                            .is_none_or(|(e, x, y, _)| (*e, *x, *y) != (request.engine, w, h))
                        {
                            worker.take();
                            worker = Some((
                                request.engine,
                                w,
                                h,
                                FframesRenderer::start_hybrid(request.engine, w, h, &stop)?,
                            ));
                        }
                        if processor
                            .as_ref()
                            .is_none_or(|(engine, _)| *engine != request.engine)
                        {
                            let adapter = if request.engine == RenderEngine::FframesCpu {
                                Gpu::software().map_err(|e| e.to_string())?
                            } else {
                                gpu.clone()
                            };
                            if request.engine == RenderEngine::FframesGpu && adapter.is_software() {
                                return Err(
                                    "fframes GPU preview requires a physical effects adapter"
                                        .into(),
                                );
                            }
                            processor = Some((request.engine, Renderer::new(adapter)));
                        }
                        let r = &mut processor.as_mut().unwrap().1;
                        // Source-monitor snapshots can reuse asset IDs for different files.
                        r.forget_media();
                        let layers = r.render_contributions(&Request {
                            project: &request.project,
                            sequence: request.sequence,
                            time: request.time,
                            scale: request.scale,
                            source: &request.frames,
                        });
                        missing = r.stats.missing_frames;
                        let floats =
                            r.read_textures(&layers.iter().map(|t| (t, 8)).collect::<Vec<_>>())?;
                        for layer in layers {
                            r.recycle(layer);
                        }
                        worker.as_mut().unwrap().3.render_layers(floats, &stop)
                    },
                ))
                .unwrap_or_else(|_| Err("fframes preview stopped unexpectedly".into()));
                if pixels.is_err() {
                    worker.take();
                    processor.take();
                }
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                *results.lock() = Some(PreviewResult {
                    key: request.key,
                    width: w,
                    height: h,
                    pixels,
                    missing,
                });
            }
        });
        Self {
            pending,
            completed,
            wake,
            cancel,
        }
    }
    pub fn request(&self, request: PreviewRequest<K>) {
        *self.pending.lock() = Some(request);
        let _ = self.wake.try_send(());
    }
    pub fn take(&self) -> Option<PreviewResult<K>> {
        self.completed.lock().take()
    }
    pub fn pending(&self) -> bool {
        self.pending.lock().is_some()
    }
}
impl<K> Drop for FframesPreview<K> {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.pending.lock().take();
        let _ = self.wake.try_send(());
        // Never join from the UI thread: cancellation stops worker IPC within 25 ms,
        // and the owning thread releases its GPU/child resources when it exits.
    }
}

/// Polling interval used by monitors while a background frame is pending.
pub const PREVIEW_POLL: Duration = Duration::from_millis(16);
