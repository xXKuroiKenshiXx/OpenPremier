//! Autosave and crash recovery. A lock file marks a running session; if it is still there at the
//! next start, the previous session ended unexpectedly and its newest autosave is offered.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::editor::{Editor, GENERATOR};
use crate::prefs::Dirs;

/// "YYYYMMDD-HHMMSS" in UTC.
pub fn stamp(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        (rem / 60) % 60,
        rem % 60
    )
}

fn safe_name(s: &str) -> String {
    let n: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if n.is_empty() { "Untitled".into() } else { n }
}

/// Only files generated for this exact project belong to its retention set. Matching just a
/// prefix would also prune another project such as `A-B` when saving project `A`.
fn belongs_to_project(path: &Path, name: &str) -> bool {
    let Some(file) = path.file_name().and_then(|f| f.to_str()) else {
        return false;
    };
    let Some(stamp) = file
        .strip_prefix(name)
        .and_then(|f| f.strip_prefix('-'))
        .and_then(|f| f.strip_suffix(".opproj"))
    else {
        return false;
    };
    stamp.len() == 15
        && stamp.as_bytes()[8] == b'-'
        && stamp
            .bytes()
            .enumerate()
            .all(|(i, c)| i == 8 || c.is_ascii_digit())
}

/// Writes an autosave copy and prunes old ones for this project.
pub fn write(editor: &mut Editor) -> Result<PathBuf, String> {
    let project = editor.snapshot();
    write_project(
        &editor.dirs.autosave(),
        &editor.document_name(),
        editor.prefs.autosave_keep,
        &project,
    )
}

/// Writes `project` as a new autosave version in `dir` and keeps the newest `keep` versions.
pub fn write_project(
    dir: &Path,
    name: &str,
    keep: usize,
    project: &op_core::Project,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let name = safe_name(name);
    let path = dir.join(format!("{name}-{}.opproj", stamp(SystemTime::now())));
    op_project::native::save(&path, project, &serde_json::Value::Null, GENERATOR)
        .map_err(|e| e.to_string())?;
    let keep = keep.max(1);
    let mut mine: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| belongs_to_project(p, &name))
        .collect();
    mine.sort();
    while mine.len() > keep {
        let old = mine.remove(0);
        let _ = std::fs::remove_file(&old);
        let _ = std::fs::remove_file(old.with_file_name(format!(
            "{}.bak",
            old.file_name().unwrap().to_string_lossy()
        )));
    }
    Ok(path)
}

fn lock_path(dirs: &Dirs) -> PathBuf {
    dirs.data.join("session.lock")
}

/// Records that a session is running. Returns the newest autosave when the previous session
/// did not end cleanly.
pub fn begin_session(dirs: &Dirs) -> Option<PathBuf> {
    let lock = lock_path(dirs);
    let crashed = lock.exists();
    let _ = std::fs::create_dir_all(&dirs.data);
    let _ = std::fs::write(&lock, std::process::id().to_string());
    if !crashed {
        return None;
    }
    newest(&dirs.autosave())
}

pub fn end_session(dirs: &Dirs) {
    let _ = std::fs::remove_file(lock_path(dirs));
}

fn newest(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "opproj"))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| e.path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps() {
        assert_eq!(stamp(UNIX_EPOCH), "19700101-000000");
        let t = UNIX_EPOCH + std::time::Duration::from_secs(1_790_000_000);
        assert_eq!(stamp(t), "20260921-141320");
    }

    #[test]
    fn retention_preserves_other_projects_and_recovery_files() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("A-19700101-000000.opproj");
        let old_backup = dir.path().join("A-19700101-000000.opproj.bak");
        std::fs::write(&old, "old").unwrap();
        std::fs::write(&old_backup, "backup").unwrap();
        let preserved = [
            "A-B-19700101-000000.opproj",
            "A-recovery.opproj",
            "A-not-a-timestamp.opproj",
            "A-19700101-000000.opproj.bak.other",
        ];
        for file in preserved {
            std::fs::write(dir.path().join(file), file).unwrap();
        }
        let saved = write_project(dir.path(), "A", 1, &op_core::Project::new("A")).unwrap();
        assert!(saved.exists());
        assert!(op_project::native::load(&saved).is_ok());
        assert!(!old.exists());
        assert!(!old_backup.exists());
        for file in preserved {
            assert_eq!(
                std::fs::read_to_string(dir.path().join(file)).unwrap(),
                file
            );
        }
    }

    #[test]
    fn retention_matches_exact_generated_names() {
        assert!(belongs_to_project(
            Path::new("Project-A-20261006-120000.opproj"),
            "Project-A"
        ));
        for file in [
            "Project-A-B-20261006-120000.opproj",
            "Project-A-20261006-120000.opproj.bak",
            "Project-A-20261006-1200000.opproj",
            "Project-A-20261006-12000x.opproj",
            "Project-A-recovery.opproj",
        ] {
            assert!(!belongs_to_project(Path::new(file), "Project-A"));
        }
    }

    #[test]
    fn crash_detection() {
        let dir = tempfile::tempdir().unwrap();
        let dirs = Dirs::portable(dir.path());
        assert!(begin_session(&dirs).is_none());
        std::fs::create_dir_all(dirs.autosave()).unwrap();
        std::fs::write(dirs.autosave().join("a-1.opproj"), "{}").unwrap();
        // the lock is still there: the previous run crashed
        assert!(begin_session(&dirs).is_some());
        end_session(&dirs);
        assert!(begin_session(&dirs).is_none());
    }
}
