//! The native project file (`.opproj`): versioned JSON.
//!
//! Saving is atomic (PR-SAVE-005 applied to our own format): the data goes to a sibling temporary
//! file, is flushed and parsed back, the previous file is kept as `.bak`, and only then is the
//! temporary file moved over the target. Media paths are stored absolute and relative to the
//! project file, so a project folder can move to another disk or computer.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Component as PathPart, Path, PathBuf};
use std::sync::{Arc, Mutex};

use op_core::*;
use serde::{Deserialize, Serialize};

use crate::ProjectError;

pub const EXTENSION: &str = "opproj";
const FORMAT: &str = "openpremier-project";
// Independent temporary files can be prepared concurrently. Backup and replacement form one
// ordered commit within this process, so the backup always contains the preceding committed file.
static COMMIT: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize)]
struct FileData {
    format: String,
    version: u32,
    generator: String,
    project: Project,
    /// Asset -> path relative to the project folder.
    #[serde(default)]
    relative_paths: BTreeMap<u64, String>,
    /// Application view state (open sequences, panels); opaque to this crate.
    #[serde(default)]
    view: serde_json::Value,
}

/// A loaded project.
#[derive(Debug)]
pub struct Loaded {
    pub project: Project,
    pub view: serde_json::Value,
    /// Assets found through their relative path after the absolute one went missing.
    pub relinked: Vec<AssetId>,
    /// Assets whose files are missing (offline).
    pub offline: Vec<AssetId>,
}

/// Serializes a project. `base` is the folder the relative paths are computed from.
pub fn to_string(
    project: &Project,
    view: &serde_json::Value,
    base: Option<&Path>,
    generator: &str,
) -> Result<String, ProjectError> {
    let mut relative_paths = BTreeMap::new();
    if let Some(base) = base {
        for (id, a) in &project.assets {
            if let Some(rel) = relative(base, Path::new(&a.path)) {
                relative_paths.insert(id.0, rel);
            }
        }
    }
    let data = FileData {
        format: FORMAT.into(),
        version: FORMAT_VERSION,
        generator: generator.into(),
        project: project.clone(),
        relative_paths,
        view: view.clone(),
    };
    serde_json::to_string_pretty(&data).map_err(|e| ProjectError::Format(e.to_string()))
}

/// Parses a project file's text. `base` resolves relative media paths.
pub fn from_str(text: &str, base: Option<&Path>) -> Result<Loaded, ProjectError> {
    let head: serde_json::Value =
        serde_json::from_str(text).map_err(|e| ProjectError::Format(e.to_string()))?;
    if head.get("format").and_then(|f| f.as_str()) != Some(FORMAT) {
        return Err(ProjectError::Format("not an OpenPremier project".into()));
    }
    let version = head.get("version").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    if version == 0 || version > FORMAT_VERSION {
        return Err(ProjectError::Version(version));
    }
    let data: FileData =
        serde_json::from_value(head).map_err(|e| ProjectError::Format(e.to_string()))?;
    let mut project = data.project;
    project.format_version = FORMAT_VERSION;
    // Reject malformed identities and graphs before refreshing counters or exposing the model.
    project.validate().map_err(ProjectError::Invalid)?;
    project.refresh_ids();
    complete_components(&mut project);
    project.validate().map_err(ProjectError::Invalid)?;

    let mut relinked = Vec::new();
    let mut offline = Vec::new();
    let ids: Vec<AssetId> = project.assets.keys().copied().collect();
    for id in ids {
        let path = project.asset(id).unwrap().path.clone();
        if Path::new(&path).exists() {
            continue;
        }
        let candidate = base
            .zip(data.relative_paths.get(&id.0))
            .map(|(b, rel)| b.join(rel));
        match candidate.filter(|c| c.exists()) {
            Some(found) => {
                let asset = Arc::make_mut(project.assets.get_mut(&id).unwrap());
                asset.path = found.to_string_lossy().into_owned();
                relinked.push(id);
            }
            None => offline.push(id),
        }
    }
    Ok(Loaded {
        project,
        view: data.view,
        relinked,
        offline,
    })
}

/// Parameters added to effects in newer versions get their defaults.
fn complete_components(project: &mut Project) {
    let ids: Vec<SequenceId> = project.sequences.keys().copied().collect();
    for sid in ids {
        let seq = project.sequence_mut(sid).unwrap();
        for kind in [TrackKind::Video, TrackKind::Audio] {
            for t in seq.tracks_mut(kind) {
                let incomplete = t
                    .clips
                    .iter()
                    .any(|c| c.components.iter().any(needs_params));
                if incomplete {
                    for c in &mut Arc::make_mut(t).clips {
                        for comp in &mut c.components {
                            comp.complete_params();
                        }
                    }
                }
            }
        }
    }
}

fn needs_params(c: &Component) -> bool {
    c.def()
        .is_some_and(|d| d.params.iter().any(|p| c.param(p.key).is_none()))
}

/// Saves atomically, keeping the previous version as `<file>.bak`.
pub fn save(
    path: &Path,
    project: &Project,
    view: &serde_json::Value,
    generator: &str,
) -> Result<(), ProjectError> {
    let base = path.parent();
    let text = to_string(project, view, base, generator)?;
    write_atomic(path, text.as_bytes(), |bytes| {
        let s = std::str::from_utf8(bytes).map_err(|e| ProjectError::Format(e.to_string()))?;
        from_str(s, None).map(|_| ())
    })
}

/// Saves without waiting for another commit; suitable for a panic hook that could interrupt one.
pub fn try_save(
    path: &Path,
    project: &Project,
    view: &serde_json::Value,
    generator: &str,
) -> Result<(), ProjectError> {
    let text = to_string(project, view, path.parent(), generator)?;
    write_atomic_inner(path, text.as_bytes(), false, |bytes| {
        let s = std::str::from_utf8(bytes).map_err(|e| ProjectError::Format(e.to_string()))?;
        from_str(s, None).map(|_| ())
    })
}

pub fn load(path: &Path) -> Result<Loaded, ProjectError> {
    let text = fs::read_to_string(path)?;
    from_str(&text, path.parent())
}

/// Writes through a verified temporary file and keeps a backup of the previous version.
pub fn write_atomic(
    path: &Path,
    bytes: &[u8],
    verify: impl Fn(&[u8]) -> Result<(), ProjectError>,
) -> Result<(), ProjectError> {
    write_atomic_inner(path, bytes, true, verify)
}

fn write_atomic_inner(
    path: &Path,
    bytes: &[u8],
    wait: bool,
    verify: impl Fn(&[u8]) -> Result<(), ProjectError>,
) -> Result<(), ProjectError> {
    let file_name = path
        .file_name()
        .ok_or_else(|| ProjectError::Format("no file name".into()))?
        .to_string_lossy()
        .into_owned();
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::Builder::new()
        .prefix(&format!(".{file_name}."))
        .suffix(".tmp")
        .tempfile_in(parent)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    verify(&fs::read(tmp.path())?)?;
    let _commit = if wait {
        COMMIT.lock().unwrap_or_else(|e| e.into_inner())
    } else {
        match COMMIT.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "another project save is committing",
                )
                .into());
            }
        }
    };
    if path.exists() {
        let bak = path.with_file_name(format!("{file_name}.bak"));
        let backup = tempfile::Builder::new()
            .prefix(&format!(".{file_name}.backup."))
            .suffix(".tmp")
            .tempfile_in(parent)?;
        fs::copy(path, backup.path())?;
        backup.as_file().sync_all()?;
        backup.persist(&bak).map_err(|e| e.error)?;
    }
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// Path of `target` relative to `base`, when both are on the same root.
pub fn relative(base: &Path, target: &Path) -> Option<String> {
    let base: Vec<PathPart> = base.components().collect();
    let target: Vec<PathPart> = target.components().collect();
    let same_root =
        matches!((base.first(), target.first()), (Some(a), Some(b)) if norm(a) == norm(b));
    if !same_root {
        return None;
    }
    let common = base
        .iter()
        .zip(&target)
        .take_while(|(a, b)| norm(a) == norm(b))
        .count();
    let mut out = PathBuf::new();
    for _ in common..base.len() {
        out.push("..");
    }
    for c in &target[common..] {
        out.push(c.as_os_str());
    }
    Some(out.to_string_lossy().replace('\\', "/"))
}

fn norm(c: &PathPart) -> String {
    let s = c.as_os_str().to_string_lossy();
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s.into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_native_bin_graphs_are_reported_without_exposing_a_project() {
        let mut original = Project::new("native bins");
        let bin = original.add_bin(original.root, "bin");
        let leaf = original.add_bin(bin, "leaf");
        for case in 0..8 {
            let mut p = original.clone();
            let root = p.root;
            match case {
                0 => p.root = ItemId(999),
                1 => p.item_mut(root).unwrap().parent = Some(bin),
                2 => p.item_mut(root).unwrap().id = ItemId(999),
                3 => {
                    p.item_mut(bin).unwrap().kind = ItemKind::Bin {
                        children: vec![leaf, leaf],
                    }
                }
                4 => p.item_mut(bin).unwrap().kind = ItemKind::Bin { children: vec![] },
                5 => {
                    p.item_mut(bin).unwrap().kind = ItemKind::Bin {
                        children: vec![ItemId(999)],
                    }
                }
                6 => {
                    p.item_mut(root).unwrap().kind = ItemKind::Bin { children: vec![] };
                    p.item_mut(bin).unwrap().parent = Some(leaf);
                    p.item_mut(leaf).unwrap().kind = ItemKind::Bin {
                        children: vec![bin],
                    };
                }
                7 => p.item_mut(leaf).unwrap().parent = Some(root),
                _ => unreachable!(),
            }
            let text = to_string(&p, &serde_json::Value::Null, None, "synthetic-fixture").unwrap();
            assert!(
                matches!(from_str(&text, None), Err(ProjectError::Invalid(_))),
                "case {case}"
            );
        }
    }

    #[test]
    fn malformed_native_entity_maps_and_identity_limits_return_errors() {
        let original = project_with_asset("offline.wav");
        for case in 0..6 {
            let mut p = original.clone();
            let aid = *p.assets.keys().next().unwrap();
            let sid = *p.sequences.keys().next().unwrap();
            match case {
                0 => Arc::make_mut(p.assets.get_mut(&aid).unwrap()).id = AssetId(999),
                1 => p.sequence_mut(sid).unwrap().id = SequenceId(999),
                2 | 3 => {
                    let id = if case == 2 { u64::MAX } else { u64::MAX - 1 };
                    p.item_mut(p.root)
                        .unwrap()
                        .markers
                        .push(Marker::new(MarkerId(id), SrcTime::ZERO));
                }
                4 => {
                    p.sequence_mut(sid).unwrap().master[0].id = ComponentId(u64::MAX);
                }
                5 => p.ids.observe(u64::MAX),
                _ => unreachable!(),
            }
            let text = to_string(&p, &serde_json::Value::Null, None, "synthetic-fixture").unwrap();
            assert!(
                matches!(
                    from_str(&text, None),
                    Err(ProjectError::Invalid(EditError::Invalid(_)))
                ),
                "case {case}"
            );
        }
    }

    #[test]
    fn native_loading_refreshes_a_stale_allocator_after_graph_validation() {
        let p = project_with_asset("offline.wav");
        let text = to_string(&p, &serde_json::Value::Null, None, "synthetic-fixture").unwrap();
        let mut fixture: serde_json::Value = serde_json::from_str(&text).unwrap();
        fixture["project"]["ids"]["next"] = serde_json::json!(0);
        let mut loaded = from_str(&fixture.to_string(), None).unwrap().project;
        let new = loaded.add_bin(loaded.root, "new bin");
        assert!(!p.items.contains_key(&new));
        loaded.validate().unwrap();
    }

    #[test]
    fn cyclic_native_sequences_are_refused() {
        let (mut p, a, clip_id) = project_with_audio_clip();
        let (b, _) = p.add_sequence(p.root, "nested", SequenceSettings::default());
        let first = p.sequence_mut(a).unwrap().clip_mut(clip_id).unwrap();
        first.source = ClipSource::Sequence {
            sequence: b,
            item: None,
        };
        first.hold = Some(SrcTime::ZERO);
        let mut second = first.clone();
        second.id = p.ids.clip();
        second.source = ClipSource::Sequence {
            sequence: a,
            item: None,
        };
        p.sequence_mut(b)
            .unwrap()
            .track_mut(TrackRef::audio(0))
            .unwrap()
            .clips
            .push(second);
        let text = to_string(&p, &serde_json::Value::Null, None, "synthetic-fixture").unwrap();
        assert!(matches!(
            from_str(&text, None),
            Err(ProjectError::Invalid(EditError::Recursive))
        ));
    }

    fn project_with_asset(path: &str) -> Project {
        let mut p = Project::new("demo");
        let asset = MediaAsset {
            id: AssetId(0),
            path: path.into(),
            proxy: None,
            kind: MediaKind::Audio,
            video: None,
            audio: vec![AudioStream {
                index: 0,
                codec: "pcm_s16le".into(),
                sample_rate: 48000,
                layout: ChannelLayout::Stereo,
                samples: 48000,
                start: Dur::ZERO,
            }],
            duration: Dur::from_seconds(1.0),
            interpretation: Interpretation::default(),
            file_size: 0,
            modified_unix: 0,
        };
        p.add_asset(p.root, asset);
        p.add_sequence(p.root, "Sequence 01", SequenceSettings::default());
        p
    }

    #[test]
    fn save_load_round_trip_with_backup() {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("media").join("tone.wav");
        fs::create_dir_all(media.parent().unwrap()).unwrap();
        fs::write(&media, b"x").unwrap();
        let p = project_with_asset(&media.to_string_lossy());
        let file = dir.path().join("proj").join("demo.opproj");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let view = serde_json::json!({"open": [1]});
        save(&file, &p, &view, "test").unwrap();
        save(&file, &p, &view, "test").unwrap();
        assert!(file.with_file_name("demo.opproj.bak").exists());
        let loaded = load(&file).unwrap();
        assert_eq!(loaded.project, p);
        assert_eq!(loaded.view, view);
        assert!(loaded.offline.is_empty());
    }

    fn project_with_audio_clip() -> (Project, SequenceId, ClipId) {
        let mut p = project_with_asset("missing-tone.wav");
        let asset = *p.assets.keys().next().unwrap();
        let sid = *p.sequences.keys().next().unwrap();
        let clip_id = p.ids.clip();
        let c: Clip = serde_json::from_value(serde_json::json!({
            "id": clip_id,
            "name": "Tone",
            "kind": "Audio",
            "source": {"Asset": {"asset": asset, "item": null, "stream": 0}},
            "start": SeqTime::ZERO,
            "duration": Dur::from_seconds(1.0),
            "source_in": SrcTime::ZERO,
            "components": []
        }))
        .unwrap();
        p.sequence_mut(sid)
            .unwrap()
            .track_mut(TrackRef::audio(0))
            .unwrap()
            .clips
            .push(c);
        (p, sid, clip_id)
    }

    #[test]
    fn invalid_clip_speed_is_reported_when_loading_native_projects() {
        let (mut p, sid, clip_id) = project_with_audio_clip();
        for (num, den) in [(0, 1), (1, 0), (-1, 1), (1, -1)] {
            p.sequence_mut(sid)
                .unwrap()
                .clip_mut(clip_id)
                .unwrap()
                .speed = Speed { num, den };
            let text = to_string(&p, &serde_json::Value::Null, None, "test").unwrap();
            assert!(matches!(
                from_str(&text, None),
                Err(ProjectError::Invalid(EditError::Invalid(_)))
            ));
        }
    }

    #[test]
    fn dangling_required_clip_sources_are_reported_when_loading() {
        let (mut p, sid, clip_id) = project_with_audio_clip();
        for source in [
            ClipSource::Asset {
                asset: AssetId(999),
                item: None,
                stream: 0,
            },
            ClipSource::Sequence {
                sequence: SequenceId(999),
                item: None,
            },
            ClipSource::Generator { item: ItemId(999) },
            ClipSource::Generator { item: p.root },
        ] {
            p.sequence_mut(sid)
                .unwrap()
                .clip_mut(clip_id)
                .unwrap()
                .source = source;
            let text = to_string(&p, &serde_json::Value::Null, None, "test").unwrap();
            assert!(matches!(
                from_str(&text, None),
                Err(ProjectError::Invalid(
                    EditError::NotFound(_) | EditError::Invalid(_)
                ))
            ));
        }
        let asset = *p.assets.keys().next().unwrap();
        p.sequence_mut(sid)
            .unwrap()
            .clip_mut(clip_id)
            .unwrap()
            .source = ClipSource::Asset {
            asset,
            item: Some(ItemId(999)),
            stream: 0,
        };
        let text = to_string(&p, &serde_json::Value::Null, None, "test").unwrap();
        assert!(from_str(&text, None).is_ok());
    }

    #[test]
    fn concurrent_atomic_writers_keep_the_preceding_commit_as_backup() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("concurrent.opproj");
        write_atomic(&file, b"initial", |_| Ok(())).unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let writers: Vec<_> = [b"first".as_slice(), b"second".as_slice()]
            .into_iter()
            .map(|bytes| {
                let path = file.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    write_atomic(&path, bytes, |written| {
                        let own_content = written == bytes;
                        barrier.wait();
                        if own_content {
                            Ok(())
                        } else {
                            Err(ProjectError::Format(
                                "another writer replaced the temporary file".into(),
                            ))
                        }
                    })
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap().unwrap();
        }
        let committed = fs::read(&file).unwrap();
        let previous = fs::read(file.with_file_name("concurrent.opproj.bak")).unwrap();
        assert!(committed == b"first" || committed == b"second");
        assert!(previous == b"first" || previous == b"second");
        assert_ne!(committed, previous);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn failed_verification_and_io_preserve_committed_files_without_temp_leaks() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("verified.opproj");
        write_atomic(&file, b"first", |_| Ok(())).unwrap();
        write_atomic(&file, b"second", |_| Ok(())).unwrap();
        assert!(
            write_atomic(&file, b"bad", |_| Err(ProjectError::Format(
                "invalid".into()
            )))
            .is_err()
        );
        assert_eq!(fs::read(&file).unwrap(), b"second");
        assert_eq!(
            fs::read(file.with_file_name("verified.opproj.bak")).unwrap(),
            b"first"
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        let destination = dir.path().join("directory.opproj");
        fs::create_dir(&destination).unwrap();
        assert!(write_atomic(&destination, b"cannot replace directory", |_| Ok(())).is_err());
        assert!(destination.is_dir());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    }

    #[test]
    fn emergency_native_save_does_not_wait_for_a_commit_lock() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("emergency.opproj");
        let _commit = COMMIT.lock().unwrap_or_else(|e| e.into_inner());
        assert!(matches!(
            try_save(&file, &Project::new("Emergency"), &serde_json::Value::Null, "test"),
            Err(ProjectError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock
        ));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn moved_folders_relink_through_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old");
        fs::create_dir_all(old.join("media")).unwrap();
        fs::write(old.join("media").join("tone.wav"), b"x").unwrap();
        let p = project_with_asset(&old.join("media").join("tone.wav").to_string_lossy());
        save(
            &old.join("demo.opproj"),
            &p,
            &serde_json::Value::Null,
            "test",
        )
        .unwrap();
        let new = dir.path().join("new");
        fs::rename(&old, &new).unwrap();
        let loaded = load(&new.join("demo.opproj")).unwrap();
        assert_eq!(loaded.relinked.len(), 1);
        let asset = loaded.project.assets.values().next().unwrap();
        assert!(Path::new(&asset.path).exists());
    }

    #[test]
    fn foreign_or_future_files_are_refused() {
        assert!(matches!(
            from_str("{\"format\":\"x\"}", None),
            Err(ProjectError::Format(_))
        ));
        let future = format!("{{\"format\":\"{FORMAT}\",\"version\":999}}");
        assert!(matches!(
            from_str(&future, None),
            Err(ProjectError::Version(999))
        ));
    }

    #[test]
    fn relative_paths() {
        let base = Path::new("/a/b/proj");
        assert_eq!(
            relative(base, Path::new("/a/b/media/x.mov")).as_deref(),
            Some("../media/x.mov")
        );
        assert_eq!(
            relative(base, Path::new("/a/b/proj/x.mov")).as_deref(),
            Some("x.mov")
        );
    }
}
