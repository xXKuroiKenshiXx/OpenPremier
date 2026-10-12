//! Attribution collection and an explicit distribution review boundary.
use crate::util::{self, Result};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn metadata(worker: bool) -> Result<Value> {
    let mut cmd = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    cmd.current_dir(util::root()).args([
        "metadata",
        "--locked",
        "--offline",
        "--format-version",
        "1",
    ]);
    if worker {
        cmd.args(["--manifest-path", "tools/fframes-worker/Cargo.toml"]);
    }
    Ok(serde_json::from_str(&util::output(&mut cmd)?)?)
}

fn is_notice(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    if Path::new(&name)
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| {
            matches!(
                s,
                "a" | "lib" | "dll" | "exe" | "o" | "obj" | "png" | "ttf" | "woff" | "woff2"
            )
        })
    {
        return false;
    }
    name.contains("license")
        || name.contains("licence")
        || name.contains("copyright")
        || name == "copying"
        || name.starts_with("copying.")
        || name.starts_with("copying-")
        || name == "notice"
        || name.starts_with("notice.")
        || name.starts_with("notice-")
        || name == "unlicense"
}

fn collect_notices(dir: &Path, out: &mut Vec<PathBuf>) -> Result {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if !matches!(entry.file_name().to_str(), Some("target" | ".git")) {
                collect_notices(&path, out)?;
            }
        } else if kind.is_file() && is_notice(&entry.file_name().to_string_lossy()) {
            out.push(path);
        }
    }
    Ok(())
}

/// Include original notices, including nested font/native-source licenses and NOTICE files.
/// Listing an SPDX expression alone does not preserve an upstream copyright notice.
pub fn notices(meta: &Value) -> Result<(String, Value)> {
    let upstream: Value = fs::read_to_string(util::root().join("licenses/upstream/index.json"))
        .ok()
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_else(|| json!({"packages":[]}));
    let packages = meta["packages"]
        .as_array()
        .ok_or("missing Cargo packages")?;
    let mut packages: Vec<_> = packages.iter().filter(|p| !p["source"].is_null()).collect();
    packages.sort_by_key(|p| (p["name"].as_str(), p["version"].as_str()));
    let mut text = String::from(
        "Resolved Cargo graph (includes platform alternatives and build dependencies). Original notice texts follow.\n\n",
    );
    let mut inventory = Vec::new();
    for package in packages {
        let name = package["name"].as_str().ok_or("missing crate name")?;
        let version = package["version"].as_str().ok_or("missing crate version")?;
        let root = Path::new(
            package["manifest_path"]
                .as_str()
                .ok_or("missing crate path")?,
        )
        .parent()
        .ok_or("invalid crate path")?;
        let mut paths = Vec::new();
        collect_notices(root, &mut paths)?;
        if let Some(file) = package["license_file"].as_str() {
            let path = root.join(file);
            if path.is_file() && path.starts_with(root) {
                paths.push(path);
            }
        }
        paths.sort();
        paths.dedup();
        text.push_str(&format!(
            "\n===== {name} {version} =====\nLicense: {}\nRepository: {}\n",
            package["license"].as_str().unwrap_or("UNSPECIFIED"),
            package["repository"].as_str().unwrap_or("")
        ));
        let mut retained = Vec::new();
        for path in paths {
            // Notices are text. Fail instead of silently truncating or dropping one.
            let contents = fs::read_to_string(&path)?;
            let relative = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            text.push_str(&format!("\n--- {relative} ---\n{contents}\n"));
            retained.push(json!({"path": relative, "sha256": util::sha256_file(&path)?}));
        }
        if retained.is_empty()
            && let Some(supplement) = upstream["packages"].as_array().and_then(|a| {
                a.iter()
                    .find(|p| p["name"] == name && p["version"] == version)
            })
        {
            let vcs: Value =
                serde_json::from_str(&fs::read_to_string(root.join(".cargo_vcs_info.json"))?)?;
            if vcs["git"]["sha1"] != supplement["revision"] {
                return Err(format!("upstream notice revision mismatch for {name}").into());
            }
            for notice in supplement["files"]
                .as_array()
                .ok_or("missing upstream notice list")?
            {
                let relative = notice["path"]
                    .as_str()
                    .ok_or("missing upstream notice path")?;
                if !relative.starts_with("licenses/upstream/")
                    || relative.contains("..")
                    || relative.contains('\\')
                {
                    return Err("invalid upstream notice path".into());
                }
                let path = util::root().join(relative);
                if notice["sha256"].as_str() != Some(util::sha256_file(&path)?.as_str()) {
                    return Err(format!("upstream notice checksum mismatch for {name}").into());
                }
                text.push_str(&format!(
                    "\n--- {} (upstream revision {}) ---\n{}\n",
                    notice["url"].as_str().unwrap_or(""),
                    supplement["revision"].as_str().unwrap_or(""),
                    fs::read_to_string(path)?
                ));
                retained.push(notice.clone());
            }
        }
        if retained.is_empty() {
            text.push_str("No notice file in the published crate: retain/review its upstream notice before redistribution.\n");
        }
        let checksum = fs::read_to_string(root.join(".cargo-checksum.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v["package"].as_str().map(str::to_owned));
        inventory.push(json!({"name":name,"version":version,"license":package["license"],"repository":package["repository"],"source":package["source"],"crate_source_url":format!("https://static.crates.io/crates/{name}/{name}-{version}.crate"),"crate_sha256":checksum,"notices":retained}));
    }
    Ok((
        text,
        json!({"scope":"resolved Cargo graph, not a linked-binary SBOM","packages":inventory}),
    ))
}

pub fn check_release() -> Result {
    let path = util::root().join("docs/legal/distribution-review.json");
    let review: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
    validate_review(&review)?;
    if review["reviewed_source_digest"].as_str() != Some(source_digest()?.as_str()) {
        return Err("distribution review does not match the current source snapshot".into());
    }
    let root = util::root().canonicalize()?;
    for field in [
        "corresponding_source",
        "native_source_manifest",
        "provenance_review",
    ] {
        let evidence = root
            .join(
                review[field]
                    .as_str()
                    .ok_or("missing legal evidence path")?,
            )
            .canonicalize()?;
        if !evidence.starts_with(&root) || !evidence.is_file() {
            return Err(format!(
                "distribution review evidence {field} must be a retained file inside the workspace"
            )
            .into());
        }
    }
    Ok(())
}

fn validate_review(review: &Value) -> Result {
    if review["status"] != "approved" {
        return Err("Redistribution review is pending: exact FFmpeg/dependency sources and proprietary-research provenance must be resolved; see docs/legal/audit-2026-10-07.md".into());
    }
    if !review["blockers"].as_array().is_some_and(Vec::is_empty) {
        return Err("distribution review still has unresolved or unrecorded blockers".into());
    }
    // An approval must identify its evidence; a bare boolean is insufficient.
    for field in [
        "reviewed_revision",
        "reviewed_source_digest",
        "corresponding_source",
        "native_source_manifest",
        "provenance_review",
    ] {
        if review[field].as_str().is_none_or(|s| s.trim().is_empty()) {
            return Err(format!("distribution review is missing {field}").into());
        }
    }
    Ok(())
}

pub fn audit() -> Result {
    let dir = util::dist_dir().join("license-audit");
    fs::create_dir_all(&dir)?;
    for (worker, label) in [(false, "editor"), (true, "fframes-worker")] {
        let (text, inventory) = notices(&metadata(worker)?)?;
        fs::write(dir.join(format!("{label}-NOTICES.txt")), text)?;
        fs::write(
            dir.join(format!("{label}-SOURCES.json")),
            serde_json::to_string_pretty(&inventory)?,
        )?;
    }
    fs::write(
        dir.join("native-BINARIES.json"),
        serde_json::to_string_pretty(&native_inventory()?)?,
    )?;
    eprintln!("Attributions and source inventory: {}", dir.display());
    eprintln!(
        "Source digest (excludes the review marker itself): {}",
        source_digest()?
    );
    Ok(())
}

fn source_digest() -> Result<String> {
    use sha2::{Digest, Sha256};
    let root = util::root();
    let files = util::output(Command::new("git").current_dir(&root).args([
        "ls-files",
        "--cached",
        "--others",
        "--exclude-standard",
        "-z",
    ]))?;
    let mut paths: Vec<_> = files
        .split('\0')
        .filter(|s| !s.is_empty() && source_path(s) && *s != "docs/legal/distribution-review.json")
        .collect();
    paths.sort_unstable();
    paths.dedup();
    let mut hash = Sha256::new();
    for path in paths {
        hash.update(path.as_bytes());
        hash.update([0]);
        hash.update(util::sha256_file(&root.join(path))?.as_bytes());
        hash.update(b"\n");
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

pub fn native_inventory() -> Result<Value> {
    let directory = crate::deps::ffmpeg_dir();
    let executable = directory.join("bin").join(if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    });
    let version = if executable.is_file() {
        Some(util::output(Command::new(executable).arg("-version"))?)
    } else {
        None
    };
    let mut libraries = Vec::new();
    for path in crate::deps::runtime_libraries()? {
        let name = path
            .file_name()
            .ok_or("invalid library filename")?
            .to_string_lossy();
        if crate::dist::FFMPEG_LIBS
            .iter()
            .any(|prefix| name.starts_with(prefix) || name.starts_with(&format!("lib{prefix}.so")))
        {
            libraries.push(json!({"name":name,"sha256":util::sha256_file(&path)?,"bytes":fs::metadata(path)?.len()}));
        }
    }
    libraries.sort_by_key(|v| v["name"].as_str().map(str::to_string));
    let receipt = fs::read_to_string(directory.join("BUILD-RECEIPT.json"))
        .ok()
        .map(|s| serde_json::from_str::<Value>(&s))
        .transpose()?;
    let skia = if cfg!(windows) {
        fs::read_to_string(util::root().join("licenses/skia/prebuilt-windows-x64.json"))
            .ok()
            .map(|s| serde_json::from_str::<Value>(&s))
            .transpose()?
    } else {
        None
    };
    let mut msvc = Vec::new();
    if cfg!(windows) {
        for path in crate::deps::fframes_msvc_runtime()? {
            msvc.push(json!({"name":path.file_name().unwrap().to_string_lossy(),"sha256":util::sha256_file(&path)?,"bytes":fs::metadata(&path)?.len(),"origin":path.display().to_string()}));
        }
    }
    Ok(
        json!({"version_and_configuration":version,"libraries":libraries,"acquisition_receipt":receipt,"skia_prebuilt":skia,"fframes_msvc_runtime":msvc,"msvc_license_review":"pending; installed Visual Studio redist files are not GPL code, see official terms and redistribution list","corresponding_source_review":"pending; checksum integrity does not establish corresponding source"}),
    )
}

/// Snapshot the exact local application source, including uncommitted/untracked source files.
/// Native corresponding-source completeness is still reported separately; this is not approval.
pub fn source_archive() -> Result<PathBuf> {
    use std::io::Write;
    let root = util::root();
    let destination = util::dist_dir().join(format!("OpenPremier-{}-source.zip", util::version()?));
    fs::create_dir_all(util::dist_dir())?;
    let output = util::output(Command::new("git").current_dir(&root).args([
        "ls-files",
        "--cached",
        "--others",
        "--exclude-standard",
        "-z",
    ]))?;
    let mut paths: Vec<_> = output
        .split('\0')
        .filter(|s| !s.is_empty() && source_path(s))
        .collect();
    paths.sort_unstable();
    paths.dedup();
    let canonical_root = root.canonicalize()?;
    // Check every path before creating the archive.
    for path in &paths {
        let path = root.join(path);
        if fs::symlink_metadata(&path)?.file_type().is_symlink()
            || !path.canonicalize()?.starts_with(&canonical_root)
        {
            return Err(format!("source path escapes repository: {}", path.display()).into());
        }
    }
    let file = fs::File::create_new(&destination)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut manifest = Vec::new();
    for path in paths {
        let full = root.join(path);
        let contents = fs::read(&full)?;
        manifest
            .push(json!({"path":path,"bytes":contents.len(),"sha256":util::sha256_file(&full)?}));
        zip.start_file(path, options)?;
        zip.write_all(&contents)?;
    }
    for (worker, label) in [(false, "editor"), (true, "fframes-worker")] {
        let (notices, inventory) = notices(&metadata(worker)?)?;
        zip.start_file(format!("dependency-sources/{label}-SOURCES.json"), options)?;
        zip.write_all(serde_json::to_string_pretty(&inventory)?.as_bytes())?;
        zip.start_file(format!("dependency-sources/{label}-NOTICES.txt"), options)?;
        zip.write_all(notices.as_bytes())?;
    }
    zip.start_file("SOURCE-MANIFEST.json", options)?;
    zip.write_all(serde_json::to_string_pretty(&json!({"application_version":util::version()?,"files":manifest,"native_corresponding_source":"pending; see docs/legal/audit-2026-10-07.md","source_kind":"current working tree, not a claim of published revision"}))?.as_bytes())?;
    zip.finish()?;
    eprintln!("Application source snapshot: {}", destination.display());
    Ok(destination)
}

fn source_path(path: &str) -> bool {
    let path = path.replace('\\', "/");
    let parts: Vec<_> = path.split('/').collect();
    if parts.iter().any(|p| {
        matches!(
            *p,
            ".." | "." | "target" | ".git" | "local-research" | "third_party" | "dist"
        )
    }) {
        return false;
    }
    let root = parts[0];
    let allowed = matches!(
        root,
        "crates" | "xtask" | "docs" | "assets" | "licenses" | ".cargo" | ".github" | "scripts"
    ) || (root == "tools"
        && (parts.get(1) == Some(&"fframes-worker")
            || (parts.len() == 2 && path.ends_with(".py"))))
        || (parts.len() == 1
            && matches!(
                root,
                "Cargo.toml"
                    | "Cargo.lock"
                    | "LICENSE"
                    | "NOTICE.md"
                    | "README.md"
                    | "CHANGELOG.md"
                    | "CONTRIBUTING.md"
                    | ".gitignore"
                    | "rust-toolchain.toml"
            ));
    allowed
        && !Path::new(&path)
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| {
                matches!(
                    s.to_ascii_lowercase().as_str(),
                    "exe" | "dll" | "prm" | "aex" | "pdb" | "opproj" | "prproj"
                )
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attribution_names_include_nested_fonts_and_native_notices() {
        for name in [
            "LICENSE-MIT",
            "Ubuntu-LICENSE.txt",
            "COPYING.LESSER",
            "NOTICE",
            "copyright",
            "UNLICENSE",
            "font-licence.txt",
        ] {
            assert!(is_notice(name), "{name}");
        }
        for name in ["Cargo.toml", "main.rs", "readme.md"] {
            assert!(!is_notice(name));
        }
    }
    #[test]
    fn unresolved_distribution_review_blocks_publication() {
        assert!(validate_review(&json!({"status":"pending"})).is_err());
        assert!(validate_review(&json!({"status":"approved"})).is_err());
        let mut approved = json!({"status":"approved", "blockers":[], "reviewed_revision":"abc", "reviewed_source_digest":"digest", "corresponding_source":"source.zip", "native_source_manifest":"sources.json", "provenance_review":"review.md"});
        assert!(validate_review(&approved).is_ok());
        approved["blockers"] = json!(["unreviewed source"]);
        assert!(validate_review(&approved).is_err());
    }
    #[test]
    fn source_snapshot_excludes_research_binaries_and_traversal() {
        for path in [
            "Cargo.lock",
            "crates/op-ui/src/theme.rs",
            "tools/fframes-worker/licenses/fframes-MIT.txt",
            "docs/legal/audit-2026-10-07.md",
        ] {
            assert!(source_path(path));
        }
        for path in [
            "local-research/evidence/private.json",
            "crates/op-ui/../../secret.txt",
            "tools/fframes-worker/target/release/worker.exe",
            "assets/vendor.prm",
            "config/preferences.json",
            "third_party/ffmpeg/bin/avcodec-62.dll",
            "dist/release.zip",
        ] {
            assert!(!source_path(path), "{path}");
        }
    }
}
