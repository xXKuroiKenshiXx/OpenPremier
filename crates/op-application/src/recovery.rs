//! Crash safety. The newest project state is kept where the panic hook can reach it and written
//! to a recovery file on a background thread at the configured interval when I/O succeeds.
//! Successful writes are acknowledged; failed revisions remain eligible for retry. The file sits in the autosave
//! folder, where the next start offers it, and is removed when the program ends normally or the
//! project is saved.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use op_core::Project;
use parking_lot::Mutex;

use crate::editor::GENERATOR;

/// How long changes wait before the recovery file is refreshed.
pub const INTERVAL: Duration = Duration::from_secs(15);

struct Latest {
    project: Arc<Project>,
    revision: u64,
    file: PathBuf,
}

/// Completion of a write, delivered only while its document generation is still active.
pub struct WriteResult {
    pub revision: u64,
    pub result: Result<(), String>,
}

struct State {
    latest: Option<Latest>,
    generation: u64,
    completion: Option<WriteResult>,
}

static STATE: Mutex<State> = Mutex::new(State {
    latest: None,
    generation: 0,
    completion: None,
});
// Writers and removal share this gate: invalidating a document before acquiring it ensures a
// queued writer cannot recreate the recovery file after discard has returned.
static IO: Mutex<()> = Mutex::new(());

/// Tests that use the shared recovery state run one at a time.
#[cfg(test)]
pub(crate) static TEST_GUARD: Mutex<()> = Mutex::new(());
static WRITING: AtomicBool = AtomicBool::new(false);
static LAST_EMERGENCY: Mutex<Option<Instant>> = Mutex::new(None);

/// The recovery file of a project.
pub fn file_for(autosave_dir: &Path, name: &str) -> PathBuf {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe = if safe.is_empty() {
        "Untitled".to_string()
    } else {
        safe
    };
    autosave_dir.join(format!("{safe}-recovery.{}", op_project::native::EXTENSION))
}

/// Records the newest state (cheap: the project is shared, not copied).
pub fn remember(project: Arc<Project>, revision: u64, file: PathBuf) {
    let mut state = STATE.lock();
    if state.latest.as_ref().is_none_or(|l| l.file != file) {
        state.generation = state.generation.wrapping_add(1);
        state.completion = None;
    }
    state.latest = Some(Latest {
        project,
        revision,
        file,
    });
}

/// Takes an acknowledged successful or failed write. Starting a job is never an acknowledgement.
pub fn take_result() -> Option<WriteResult> {
    STATE.lock().completion.take()
}

fn write(project: &Project, file: &Path, emergency: bool) -> Result<(), String> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let save = if emergency {
        op_project::native::try_save
    } else {
        op_project::native::save
    };
    save(file, project, &serde_json::Value::Null, GENERATOR).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(file.with_file_name(format!(
        "{}.bak",
        file.file_name().unwrap_or_default().to_string_lossy()
    )));
    Ok(())
}

fn run_write(
    revision: u64,
    generation: u64,
    file: &Path,
    writer: impl FnOnce() -> Result<(), String>,
) {
    let _io = IO.lock();
    {
        let state = STATE.lock();
        if state.generation != generation
            || state
                .latest
                .as_ref()
                .is_none_or(|l| l.revision != revision || l.file != file)
        {
            return;
        }
    }
    let started = Instant::now();
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(writer)).unwrap_or_else(|panic| {
            Err(format!(
                "recovery writer panicked: {}",
                crate::media::panic_message(&*panic)
            ))
        });
    match &result {
        Ok(()) => log::debug!(
            "recovery file updated in {} ms: {}",
            started.elapsed().as_millis(),
            file.display()
        ),
        Err(e) => log::warn!("recovery file not written: {e}"),
    }
    let mut state = STATE.lock();
    if state.generation == generation {
        state.completion = Some(WriteResult { revision, result });
    }
}

/// Writes the remembered state on a background thread unless a write is already running.
/// Returns the attempted revision; its successful or failed acknowledgement comes via `take_result`.
pub fn write_in_background() -> Option<u64> {
    let (project, revision, file, generation) = {
        let state = STATE.lock();
        let l = state.latest.as_ref()?;
        (
            l.project.clone(),
            l.revision,
            l.file.clone(),
            state.generation,
        )
    };
    if WRITING.swap(true, Ordering::AcqRel) {
        return None;
    }
    let spawned = std::thread::Builder::new()
        .name("recovery".into())
        .spawn(move || {
            // Reset even if code outside the guarded write panics.
            struct Writing;
            impl Drop for Writing {
                fn drop(&mut self) {
                    WRITING.store(false, Ordering::Release);
                }
            }
            let _writing = Writing;
            run_write(revision, generation, &file, || {
                write(&project, &file, false)
            });
        });
    if let Err(error) = spawned {
        let mut state = STATE.lock();
        if state.generation == generation {
            state.completion = Some(WriteResult {
                revision,
                result: Err(format!("could not start recovery writer: {error}")),
            });
        }
        WRITING.store(false, Ordering::Release);
        return Some(revision);
    }
    Some(revision)
}

/// Saves the remembered state immediately (panic hook). Never blocks on a lock held by the
/// failing thread, and runs at most every few seconds.
pub fn emergency_save() -> Option<PathBuf> {
    // The panic may originate while this thread holds an I/O or state lock. Never wait on either.
    let _io = IO.try_lock()?;
    let (project, file) = {
        let state = STATE.try_lock()?;
        let l = state.latest.as_ref()?;
        (l.project.clone(), l.file.clone())
    };
    {
        let mut last = LAST_EMERGENCY.try_lock()?;
        if last.is_some_and(|t| t.elapsed() < Duration::from_secs(5)) {
            return None;
        }
        *last = Some(Instant::now());
    }
    match write(&project, &file, true) {
        Ok(()) => {
            log::error!("project state saved for recovery: {}", file.display());
            Some(file)
        }
        Err(e) => {
            log::error!("emergency save failed: {e}");
            None
        }
    }
}

/// Removes the recovery file (normal exit, or the project was saved).
pub fn discard() {
    let file = {
        let mut state = STATE.lock();
        state.generation = state.generation.wrapping_add(1);
        state.completion = None;
        state.latest.take().map(|l| l.file)
    };
    let _io = IO.lock();
    if let Some(f) = file
        && let Err(error) = std::fs::remove_file(&f)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("recovery file not removed ({}): {error}", f.display());
    }
}

/// Forgets the remembered state (a new or another project was opened).
pub fn forget() {
    discard();
}

#[cfg(test)]
pub(crate) fn wait_for_background() {
    let start = Instant::now();
    while WRITING.load(Ordering::Acquire) && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        !WRITING.load(Ordering::Acquire),
        "recovery writer did not finish"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_and_emergency_writes() {
        let _guard = TEST_GUARD.lock();
        forget();
        *LAST_EMERGENCY.lock() = None;
        let dir = tempfile::tempdir().unwrap();
        let file = file_for(dir.path(), "My Project");
        assert!(file.ends_with("My_Project-recovery.opproj"));
        remember(Arc::new(Project::new("P")), 1, file.clone());
        assert_eq!(write_in_background(), Some(1));
        wait_for_background();
        let result = take_result().unwrap();
        assert_eq!(result.revision, 1);
        assert!(result.result.is_ok());
        assert!(file.exists());
        std::fs::remove_file(&file).unwrap();
        assert_eq!(emergency_save().as_deref(), Some(file.as_path()));
        assert!(op_project::native::load(&file).is_ok());
        forget();
        assert!(!file.exists());
    }

    #[test]
    fn failed_io_is_acknowledged_and_the_same_revision_can_retry() {
        let _guard = TEST_GUARD.lock();
        forget();
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("blocked");
        std::fs::write(&blocker, "not a directory").unwrap();
        let file = file_for(&blocker, "Retry");
        remember(Arc::new(Project::new("Retry")), 7, file.clone());
        assert_eq!(write_in_background(), Some(7));
        wait_for_background();
        let failed = take_result().unwrap();
        assert_eq!(failed.revision, 7);
        assert!(failed.result.is_err());
        std::fs::remove_file(&blocker).unwrap();
        assert_eq!(write_in_background(), Some(7));
        wait_for_background();
        let successful = take_result().unwrap();
        assert_eq!(successful.revision, 7);
        assert!(successful.result.is_ok());
        assert_eq!(
            op_project::native::load(&file).unwrap().project.name,
            "Retry"
        );
        forget();
    }

    fn wait_for_discard(generation: u64) {
        let start = Instant::now();
        while STATE.lock().generation == generation && start.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_ne!(
            STATE.lock().generation,
            generation,
            "discard did not invalidate document"
        );
    }

    #[test]
    fn discarded_queued_writer_cannot_recreate_the_file() {
        let _guard = TEST_GUARD.lock();
        forget();
        let dir = tempfile::tempdir().unwrap();
        let file = file_for(dir.path(), "Queued");
        remember(Arc::new(Project::new("Queued")), 8, file.clone());
        let generation = STATE.lock().generation;
        let io = IO.lock();
        assert_eq!(write_in_background(), Some(8));
        let discarded = std::thread::spawn(discard);
        wait_for_discard(generation);
        drop(io);
        discarded.join().unwrap();
        wait_for_background();
        assert!(!file.exists());
        assert!(take_result().is_none());
        assert!(write_in_background().is_none());
    }

    #[test]
    fn discard_waits_for_active_write_and_suppresses_its_acknowledgement() {
        let _guard = TEST_GUARD.lock();
        forget();
        let dir = tempfile::tempdir().unwrap();
        let file = file_for(dir.path(), "Active");
        remember(Arc::new(Project::new("Active")), 9, file.clone());
        let generation = STATE.lock().generation;
        let (created_tx, created_rx) = std::sync::mpsc::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        let writer_file = file.clone();
        let writer = std::thread::spawn(move || {
            run_write(9, generation, &writer_file, || {
                write(&Project::new("Active"), &writer_file, false)?;
                created_tx.send(()).unwrap();
                finish_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                Ok(())
            });
        });
        created_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(file.exists());
        let discarded = std::thread::spawn(discard);
        wait_for_discard(generation);
        finish_tx.send(()).unwrap();
        writer.join().unwrap();
        discarded.join().unwrap();
        assert!(!file.exists());
        assert!(take_result().is_none());
    }

    #[test]
    fn superseded_snapshot_cannot_overwrite_a_newer_remembered_revision() {
        let _guard = TEST_GUARD.lock();
        forget();
        let dir = tempfile::tempdir().unwrap();
        let file = file_for(dir.path(), "Newest");
        remember(Arc::new(Project::new("Old")), 1, file.clone());
        let generation = STATE.lock().generation;
        remember(Arc::new(Project::new("New")), 2, file.clone());
        run_write(1, generation, &file, || {
            panic!("superseded writer must not run")
        });
        assert!(!file.exists());
        assert!(take_result().is_none());
        assert_eq!(write_in_background(), Some(2));
        wait_for_background();
        let result = take_result().unwrap();
        assert_eq!(result.revision, 2);
        assert!(result.result.is_ok());
        assert_eq!(op_project::native::load(&file).unwrap().project.name, "New");
        forget();
    }

    #[test]
    fn emergency_save_does_not_wait_on_the_io_gate() {
        let _guard = TEST_GUARD.lock();
        forget();
        let dir = tempfile::tempdir().unwrap();
        let file = file_for(dir.path(), "Emergency");
        remember(Arc::new(Project::new("Emergency")), 10, file.clone());
        let io = IO.lock();
        assert!(emergency_save().is_none());
        drop(io);
        assert!(!file.exists());
        forget();
    }
}
