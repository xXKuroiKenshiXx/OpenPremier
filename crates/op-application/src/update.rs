//! Updating from inside the program: finds the newest release on GitHub, downloads the package
//! for this system, checks it against the release's SHA256SUMS.txt and installs it when the
//! program closes.
//!
//! - Windows (the ZIP): the new files are extracted next to the download; on exit the new
//!   `OpenPremier.exe` is started from there with `--finish-update`, waits for this process to
//!   end, copies itself and the other files over the installed folder and starts it again.
//! - Linux (the AppImage): on exit the new AppImage replaces the running one (same path, so
//!   shortcuts keep working) and is started.
//! - macOS (the disk image): the downloaded DMG is opened so the app can be dragged to
//!   Applications.
//!
//! A build that was not installed from a package (a development build) cannot replace itself;
//! the download is then only offered from the releases page.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const REPO: &str = "xXKuroiKenshiXx/OpenPremier";
pub const RELEASES_PAGE: &str = "https://github.com/xXKuroiKenshiXx/OpenPremier/releases";
/// Argument the new program is started with to finish a Windows update.
pub const FINISH_ARG: &str = "--finish-update";

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// "v0.10.1" or "0.10.1" as (0, 10, 1).
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>().ok());
    let v = (it.next()??, it.next()??, it.next().unwrap_or(Some(0))?);
    Some(v)
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// The end of the package file name for this system.
pub fn package_suffix() -> &'static str {
    if cfg!(windows) {
        "-windows-x64.zip"
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "-macos-arm64.dmg"
        } else {
            "-macos-x86_64.dmg"
        }
    } else {
        "-x86_64.AppImage"
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub tag: String,
    /// The release page.
    pub page: String,
    /// Release notes (Markdown).
    pub notes: String,
    /// The package for this system, when the release has one.
    pub package: Option<Asset>,
    pub sums: Option<Asset>,
}

/// The newest published release newer than `current` in a GitHub `/releases` listing.
pub fn pick(listing: &Value, current: &str) -> Option<Release> {
    let mut best: Option<((u64, u64, u64), Release)> = None;
    for r in listing.as_array()? {
        if r["draft"].as_bool().unwrap_or(false) {
            continue;
        }
        let tag = r["tag_name"].as_str().unwrap_or_default();
        let Some(v) = parse_version(tag) else {
            continue;
        };
        if !is_newer(tag, current) || best.as_ref().is_some_and(|(b, _)| *b >= v) {
            continue;
        }
        let assets: Vec<Asset> = r["assets"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        Some(Asset {
                            name: x["name"].as_str()?.to_string(),
                            url: x["browser_download_url"].as_str()?.to_string(),
                            size: x["size"].as_u64().unwrap_or(0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let find = |pred: &dyn Fn(&str) -> bool| assets.iter().find(|a| pred(&a.name)).cloned();
        best = Some((
            v,
            Release {
                version: format!("{}.{}.{}", v.0, v.1, v.2),
                tag: tag.to_string(),
                page: r["html_url"].as_str().unwrap_or(RELEASES_PAGE).to_string(),
                notes: r["body"].as_str().unwrap_or_default().to_string(),
                package: find(&|n| n.ends_with(package_suffix())),
                sums: find(&|n| n == "SHA256SUMS.txt"),
            },
        ));
    }
    best.map(|(_, r)| r)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .build()
        .into()
}

/// Asks GitHub for the releases. `Ok(None)`: this is the newest version.
pub fn check() -> Result<Option<Release>, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=20");
    let text = agent()
        .get(&url)
        .header("User-Agent", "OpenPremier")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("could not reach GitHub: {e}"))?
        .into_body()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let listing: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(pick(&listing, current_version()))
}

/// A check running on a background thread.
#[derive(Clone, Default)]
pub struct Check {
    pub result: Arc<Mutex<Option<Result<Option<Release>, String>>>>,
}

impl Check {
    pub fn start() -> Check {
        let c = Check::default();
        let out = c.result.clone();
        let _ = std::thread::Builder::new()
            .name("update-check".into())
            .spawn(move || {
                let r = check();
                match &r {
                    Ok(Some(rel)) => log::info!("update available: {}", rel.version),
                    Ok(None) => log::info!("no newer version than {}", current_version()),
                    Err(e) => log::info!("update check failed: {e}"),
                }
                *out.lock() = Some(r);
            });
        c
    }

    pub fn take(&self) -> Option<Result<Option<Release>, String>> {
        self.result.lock().take()
    }
}

// --------------------------------------------------------------------------------- install

/// What installing a downloaded update takes.
#[derive(Clone, Debug, PartialEq)]
pub enum Install {
    /// Windows: the extracted package folder and the installed folder.
    Folder { staged: PathBuf, installed: PathBuf },
    /// Linux: the downloaded AppImage and the running one.
    AppImage {
        downloaded: PathBuf,
        running: PathBuf,
    },
    /// macOS: the disk image to open.
    DiskImage(PathBuf),
}

/// Where the running program can be replaced, or why not.
pub fn install_target() -> Result<PathBuf, String> {
    if cfg!(target_os = "linux") {
        return std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .ok_or_else(|| "only the AppImage can update itself".to_string());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no program folder")?.to_path_buf();
    if cfg!(windows) && !dir.join("THIRD-PARTY-NOTICES.txt").is_file() {
        return Err("this copy was not installed from a release package".into());
    }
    Ok(dir)
}

/// A download running on a background thread.
#[derive(Clone, Default)]
pub struct Download {
    /// Bytes received and expected.
    pub progress: Arc<Mutex<(u64, u64)>>,
    pub cancel: Arc<AtomicBool>,
    pub result: Arc<Mutex<Option<Result<Install, String>>>>,
}

impl Download {
    /// Downloads `release` into `folder` and prepares it.
    pub fn start(release: Release, folder: PathBuf) -> Download {
        let d = Download::default();
        let (progress, cancel, out) = (d.progress.clone(), d.cancel.clone(), d.result.clone());
        let _ = std::thread::Builder::new()
            .name("update-download".into())
            .spawn(move || {
                let r = prepare(&release, &folder, &progress, &cancel);
                if let Err(e) = &r {
                    log::warn!("update {}: {e}", release.version);
                }
                *out.lock() = Some(r);
            });
        d
    }

    pub fn finished(&self) -> bool {
        self.result.lock().is_some()
    }
}

fn fetch(
    url: &str,
    to: &Path,
    progress: &Mutex<(u64, u64)>,
    expected: u64,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let resp = agent()
        .get(url)
        .header("User-Agent", "OpenPremier")
        .call()
        .map_err(|e| format!("download failed: {e}"))?;
    let mut reader = resp.into_body().into_reader();
    let part = to.with_extension("part");
    let mut out = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(out);
            let _ = std::fs::remove_file(&part);
            return Err("cancelled".into());
        }
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        hash.update(&buf[..n]);
        done += n as u64;
        *progress.lock() = (done, expected.max(done));
    }
    out.flush().map_err(|e| e.to_string())?;
    drop(out);
    std::fs::rename(&part, to).map_err(|e| e.to_string())?;
    Ok(hex(&hash.finalize()))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The checksum of `name` in a SHA256SUMS.txt text.
pub fn sum_of(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut p = l.split_whitespace();
        let (sum, file) = (p.next()?, p.next()?);
        (file.trim_start_matches('*') == name).then(|| sum.to_ascii_lowercase())
    })
}

fn prepare(
    release: &Release,
    folder: &Path,
    progress: &Mutex<(u64, u64)>,
    cancel: &AtomicBool,
) -> Result<Install, String> {
    let package = release
        .package
        .as_ref()
        .ok_or("this release has no package for this system")?;
    let sums = release
        .sums
        .as_ref()
        .ok_or("this release has no SHA256SUMS.txt")?;
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let sums_text = agent()
        .get(&sums.url)
        .header("User-Agent", "OpenPremier")
        .call()
        .map_err(|e| format!("download failed: {e}"))?
        .into_body()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let expected = sum_of(&sums_text, &package.name)
        .ok_or_else(|| format!("{} is not listed in SHA256SUMS.txt", package.name))?;
    let file = folder.join(&package.name);
    let got = fetch(&package.url, &file, progress, package.size, cancel)?;
    if got != expected {
        let _ = std::fs::remove_file(&file);
        return Err(format!("{} does not match its SHA-256", package.name));
    }
    log::info!("downloaded {} ({got})", file.display());
    if cfg!(target_os = "macos") {
        return Ok(Install::DiskImage(file));
    }
    let target = install_target()?;
    if cfg!(target_os = "linux") {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
        }
        return Ok(Install::AppImage {
            downloaded: file,
            running: target,
        });
    }
    // Windows: extract the ZIP; its single top folder holds the program
    let staged_root = folder.join(format!("OpenPremier-{}", release.version));
    if staged_root.exists() {
        std::fs::remove_dir_all(&staged_root).map_err(|e| e.to_string())?;
    }
    extract(&file, &staged_root)?;
    let staged = single_folder(&staged_root)?;
    if !staged.join(exe_name()).is_file() {
        return Err("the package does not contain the program".into());
    }
    Ok(Install::Folder {
        staged,
        installed: target,
    })
}

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "OpenPremier.exe"
    } else {
        "OpenPremier"
    }
}

fn single_folder(root: &Path) -> Result<PathBuf, String> {
    let entries: Vec<PathBuf> = std::fs::read_dir(root)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    match &entries[..] {
        [one] if one.is_dir() => Ok(one.clone()),
        _ => Ok(root.to_path_buf()),
    }
}

/// Extracts a ZIP, refusing entries that would land outside `to`.
pub fn extract(zip: &Path, to: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else {
            return Err(format!("unsafe path in the package: {}", entry.name()));
        };
        let out = to.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Called as the program closes: hands over to the new version.
pub fn finish(install: &Install) -> Result<(), String> {
    match install {
        Install::Folder { staged, installed } => {
            // the new program finishes the job once this one has ended
            std::process::Command::new(staged.join(exe_name()))
                .arg(FINISH_ARG)
                .arg(installed)
                .arg(std::process::id().to_string())
                .current_dir(staged)
                .spawn()
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        Install::AppImage {
            downloaded,
            running,
        } => {
            std::fs::rename(downloaded, running)
                .or_else(|_| std::fs::copy(downloaded, running).map(|_| ()))
                .map_err(|e| e.to_string())?;
            std::process::Command::new(running)
                .spawn()
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        Install::DiskImage(dmg) => {
            std::process::Command::new("open")
                .arg(dmg)
                .spawn()
                .map_err(|e| e.to_string())?;
            Ok(())
        }
    }
}

/// The new program's side of a Windows update (`--finish-update <installed folder> <pid>`):
/// waits for the old program to end, copies the package over the installed folder and starts
/// the installed program. A failure restores the old program file.
pub fn finish_windows(installed: &Path, pid: Option<u32>) -> Result<(), String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let staged = me.parent().ok_or("no package folder")?.to_path_buf();
    let exe = installed.join(exe_name());
    let backup = installed.join(format!("{}.old", exe_name()));
    // a running program file can be renamed on Windows, but its libraries stay in use: wait
    // for the process itself to end
    if let Some(pid) = pid
        && !wait_for_exit(pid, Duration::from_secs(120))
    {
        return Err("the previous version is still running".into());
    }
    let mut moved = false;
    for _ in 0..40 {
        let _ = std::fs::remove_file(&backup);
        if !exe.exists() || std::fs::rename(&exe, &backup).is_ok() {
            moved = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    if !moved {
        return Err(format!("{} cannot be replaced", exe.display()));
    }
    // its libraries are free a moment later
    std::thread::sleep(Duration::from_millis(500));
    let copied = copy_tree(&staged, installed);
    if let Err(e) = copied {
        if !exe.exists() {
            let _ = std::fs::rename(&backup, &exe);
        }
        return Err(e);
    }
    let _ = std::fs::remove_file(&backup);
    // tests check the handover without opening a window
    if std::env::var_os("OPENPREMIER_UPDATE_NO_RESTART").is_some() {
        return Ok(());
    }
    std::process::Command::new(&exe)
        .current_dir(installed)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Whether process `pid` ended within `limit`.
fn wait_for_exit(pid: u32, limit: Duration) -> bool {
    let pid = sysinfo::Pid::from_u32(pid);
    let mut sys = sysinfo::System::new();
    let start = std::time::Instant::now();
    loop {
        sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        if sys.process(pid).is_none() {
            return true;
        }
        if start.elapsed() > limit {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for e in std::fs::read_dir(from).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let src = e.path();
        let dst = to.join(e.file_name());
        if src.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            let mut last = String::new();
            let mut ok = false;
            for _ in 0..20 {
                match std::fs::copy(&src, &dst) {
                    Ok(_) => {
                        ok = true;
                        break;
                    }
                    Err(err) => {
                        last = format!("{}: {err}", dst.display());
                        std::thread::sleep(Duration::from_millis(250));
                    }
                }
            }
            if !ok {
                return Err(last);
            }
        }
    }
    Ok(())
}

/// Opens a web page in the system browser.
pub fn open_url(url: &str) {
    let r = if cfg!(windows) {
        std::process::Command::new("explorer").arg(url).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if let Err(e) = r {
        log::warn!("cannot open {url}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reaches GitHub: `cargo test -p op-application --lib update -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn the_real_releases_are_read() {
        let listing: Value = serde_json::from_str(
            &agent()
                .get(&format!(
                    "https://api.github.com/repos/{REPO}/releases?per_page=20"
                ))
                .header("User-Agent", "OpenPremier")
                .call()
                .unwrap()
                .into_body()
                .read_to_string()
                .unwrap(),
        )
        .unwrap();
        let r = pick(&listing, "0.9.0").expect("0.10.0 is newer than 0.9.0");
        println!("{} {:?}", r.version, r.package);
        assert!(r.package.is_some() && r.sums.is_some());
    }

    /// Downloads the published package and checks it (about 80 MB):
    /// `cargo test -p op-application --lib update::tests::the_real_package -- --ignored`.
    #[test]
    #[ignore]
    fn the_real_package_downloads_and_matches_its_checksum() {
        let listing: Value = serde_json::from_str(
            &agent()
                .get(&format!(
                    "https://api.github.com/repos/{REPO}/releases?per_page=20"
                ))
                .header("User-Agent", "OpenPremier")
                .call()
                .unwrap()
                .into_body()
                .read_to_string()
                .unwrap(),
        )
        .unwrap();
        let release = pick(&listing, "0.9.0").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let progress = Mutex::new((0, 0));
        let r = prepare(&release, dir.path(), &progress, &AtomicBool::new(false));
        let name = release.package.unwrap().name;
        assert!(dir.path().join(&name).is_file(), "{r:?}");
        // a development build is not a package: it must not try to replace itself
        if install_target().is_err() {
            assert!(r.is_err());
        }
        println!("{name}: {:?} {:?}", progress.lock(), r.map(|_| ()));
    }

    #[test]
    fn versions_compare_by_number() {
        assert_eq!(parse_version("v0.10.1"), Some((0, 10, 1)));
        assert_eq!(parse_version("0.9"), Some((0, 9, 0)));
        assert!(is_newer("v0.10.0", "0.9.0"));
        assert!(!is_newer("v0.9.0", "0.10.0"));
        assert!(!is_newer("v0.10.0", "0.10.0"));
        assert_eq!(parse_version("nightly"), None);
    }

    #[test]
    fn the_newest_release_with_its_package_is_picked() {
        let suffix = package_suffix();
        let listing = serde_json::json!([
            {"tag_name": "v0.11.0", "draft": true, "assets": []},
            {"tag_name": "v0.10.1", "draft": false, "prerelease": true,
             "html_url": "https://example/0.10.1", "body": "notes",
             "assets": [
                {"name": format!("OpenPremier-0.10.1{suffix}"), "browser_download_url": "https://example/pkg", "size": 10},
                {"name": "SHA256SUMS.txt", "browser_download_url": "https://example/sums", "size": 1}
             ]},
            {"tag_name": "v0.10.0", "draft": false, "assets": []},
            {"tag_name": "v0.9.0", "draft": false, "assets": []}
        ]);
        let r = pick(&listing, "0.10.0").unwrap();
        assert_eq!(r.version, "0.10.1");
        assert_eq!(r.package.unwrap().url, "https://example/pkg");
        assert_eq!(r.sums.unwrap().url, "https://example/sums");
        assert!(pick(&listing, "0.10.1").is_none(), "drafts are not offered");
    }

    #[test]
    fn checksums_are_read_from_the_sums_file() {
        let sums =
            "ABC123  OpenPremier-0.10.1-windows-x64.zip\nfff *OpenPremier-0.10.1-x86_64.AppImage\n";
        assert_eq!(
            sum_of(sums, "OpenPremier-0.10.1-windows-x64.zip").as_deref(),
            Some("abc123")
        );
        assert_eq!(
            sum_of(sums, "OpenPremier-0.10.1-x86_64.AppImage").as_deref(),
            Some("fff")
        );
        assert_eq!(sum_of(sums, "missing.zip"), None);
    }

    #[test]
    fn a_package_is_extracted_and_copied_over_an_installed_folder() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("pkg.zip");
        {
            let f = std::fs::File::create(&zip_path).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let o: zip::write::SimpleFileOptions = Default::default();
            z.start_file("OpenPremier-9.9.9/OpenPremier.exe", o)
                .unwrap();
            z.write_all(b"new program").unwrap();
            z.start_file("OpenPremier-9.9.9/licenses/a.txt", o).unwrap();
            z.write_all(b"license").unwrap();
            z.finish().unwrap();
        }
        let staged_root = dir.path().join("staged");
        extract(&zip_path, &staged_root).unwrap();
        let staged = single_folder(&staged_root).unwrap();
        assert!(staged.ends_with("OpenPremier-9.9.9"));
        let installed = dir.path().join("installed");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::write(installed.join("OpenPremier.exe"), b"old").unwrap();
        std::fs::write(installed.join("keep.txt"), b"user file").unwrap();
        copy_tree(&staged, &installed).unwrap();
        assert_eq!(
            std::fs::read(installed.join("OpenPremier.exe")).unwrap(),
            b"new program"
        );
        assert_eq!(
            std::fs::read(installed.join("licenses/a.txt")).unwrap(),
            b"license"
        );
        assert_eq!(
            std::fs::read(installed.join("keep.txt")).unwrap(),
            b"user file"
        );
    }

    #[test]
    fn zip_entries_outside_the_folder_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("bad.zip");
        {
            let f = std::fs::File::create(&zip_path).unwrap();
            let mut z = zip::ZipWriter::new(f);
            z.start_file("../evil.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(b"x").unwrap();
            z.finish().unwrap();
        }
        assert!(extract(&zip_path, &dir.path().join("out")).is_err());
        assert!(!dir.path().join("evil.txt").exists());
    }
}
