//! Release packages.
//!
//! * Windows: `dist/OpenPremier-<version>-windows-x64.zip`, a folder with the program, the
//!   FFmpeg libraries it links, the licenses and the third-party notices. It runs from any
//!   folder; a `portable.txt` file next to the program keeps settings in that folder.
//! * Linux: the AppImage (see `linux.rs`).
//! * macOS: a disk image with the app bundle (see `macos.rs`).
//!
//! Each package is started with `--self-test` from its final layout before it is accepted.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util::{self, Result};
use crate::{configured_cargo, deps};

/// FFmpeg libraries the program links (the others in the FFmpeg build are not needed).
pub const FFMPEG_LIBS: [&str; 5] = ["avcodec", "avformat", "avutil", "swresample", "swscale"];

pub fn dist(args: &[String]) -> Result {
    if cfg!(target_os = "macos") {
        let dmg = crate::macos::package()?;
        eprintln!(
            "\nmacOS package: {} ({})",
            dmg.display(),
            util::size_text(&dmg)
        );
    } else if cfg!(windows) {
        let zip = windows_zip()?;
        if !args.iter().any(|a| a == "--windows-only") {
            crate::linux::appimage_task()?;
        }
        eprintln!(
            "\nWindows package: {} ({})",
            zip.display(),
            util::size_text(&zip)
        );
    } else {
        crate::linux::inside(args)?;
    }
    write_checksums()?;
    Ok(())
}

/// Builds the release binary with the FFmpeg configuration.
pub fn release_build() -> Result {
    let mut c = configured_cargo(&["build", "--release", "--locked", "--package", util::BIN])?;
    util::run(&mut c)
}

/// Build the isolated worker for every supported desktop package. Cargo metadata resolves
/// its actual target directory, including container CARGO_TARGET_DIR overrides.
pub fn worker_build() -> Result<PathBuf> {
    let manifest = util::root().join("tools/fframes-worker/Cargo.toml");
    let manifest = manifest.to_str().ok_or("worker manifest is not UTF-8")?;
    let mut build = configured_cargo(&[
        "build",
        "--release",
        "--locked",
        "--manifest-path",
        manifest,
    ])?;
    if cfg!(target_os = "linux") {
        build.env("RUSTFLAGS", "-C link-arg=-Wl,-rpath,$ORIGIN/../lib");
    }
    util::run(&mut build)?;
    let meta: serde_json::Value = serde_json::from_str(&util::output(&mut configured_cargo(&[
        "metadata",
        "--no-deps",
        "--format-version",
        "1",
        "--manifest-path",
        manifest,
    ])?)?)?;
    let target = meta["target_directory"]
        .as_str()
        .ok_or("worker target directory missing")?;
    Ok(PathBuf::from(target)
        .join("release")
        .join(if cfg!(windows) {
            "OpenPremier-FFrames.exe"
        } else {
            "OpenPremier-FFrames"
        }))
}

fn windows_zip() -> Result<PathBuf> {
    let version = util::version()?;
    let name = format!("{}-{version}-windows-x64", util::APP_NAME);
    let dist = util::dist_dir();
    let stage = dist.join(&name);
    let zip = dist.join(format!("{name}.zip"));
    let sources = dist.join(format!("OpenPremier-{version}-source.zip"));
    if stage.exists() || zip.exists() || sources.exists() {
        return Err("Windows package destination already exists; preserve or move the previous stage and ZIP before packaging".into());
    }
    release_build()?;
    // A distinct Cargo graph keeps fframes' native FFmpeg binding out of the editor's graph.
    let worker = worker_build()?;
    verify_skia_receipt()?;
    verify_ffmpeg_receipt()?;
    fs::create_dir_all(stage.join("licenses"))?;
    let exe = util::root()
        .join("target/release")
        .join(format!("{}.exe", util::BIN));
    fs::copy(&exe, stage.join(format!("{}.exe", util::APP_NAME)))?;
    fs::copy(worker, stage.join("OpenPremier-FFrames.exe"))?;
    for lib in deps::runtime_libraries()? {
        let file = lib.file_name().unwrap().to_string_lossy().to_string();
        if FFMPEG_LIBS
            .iter()
            .any(|l| file.starts_with(&format!("{l}-")))
        {
            fs::copy(&lib, stage.join(&file))?;
        }
    }
    for lib in deps::fframes_msvc_runtime()? {
        fs::copy(
            &lib,
            stage.join(lib.file_name().ok_or("runtime filename missing")?),
        )?;
    }
    write_docs(&stage, "windows")?;
    self_test(&stage.join(format!("{}.exe", util::APP_NAME)))?;
    self_test(&stage.join("OpenPremier-FFrames.exe"))?;
    crate::legal::source_archive()?;
    zip_dir(&stage, &zip, &name)?;
    Ok(zip)
}

fn verify_ffmpeg_receipt() -> Result {
    let receipt: serde_json::Value = serde_json::from_str(&fs::read_to_string(
        util::root().join("licenses/ffmpeg/prebuilt-windows-x64.json"),
    )?)?;
    for library in receipt["verified_dlls"]
        .as_array()
        .ok_or("FFmpeg binary receipt is missing DLLs")?
    {
        let name = library["name"]
            .as_str()
            .ok_or("FFmpeg DLL receipt name missing")?;
        if name.contains(['/', '\\']) {
            return Err("invalid FFmpeg receipt filename".into());
        }
        let path = deps::ffmpeg_dir().join("bin").join(name);
        if library["sha256"].as_str() != Some(util::sha256_file(&path)?.as_str()) {
            return Err(format!("FFmpeg DLL identity changed: {name}; review and retain its exact source/build provenance before packaging").into());
        }
    }
    Ok(())
}

/// A cached native archive must match its recorded licenses and static library identity.
fn verify_skia_receipt() -> Result {
    let root = util::root();
    let receipt: serde_json::Value = serde_json::from_str(&fs::read_to_string(
        root.join("licenses/skia/prebuilt-windows-x64.json"),
    )?)?;
    if receipt["license_sha256"].as_str()
        != Some(util::sha256_file(&root.join("licenses/skia/LICENSE_SKIA.txt"))?.as_str())
    {
        return Err("Skia license receipt checksum mismatch".into());
    }
    let libraries = receipt["verified_static_libraries"]
        .as_array()
        .filter(|libraries| !libraries.is_empty())
        .ok_or("Skia static library receipt missing")?;
    for entry in fs::read_dir(root.join("tools/fframes-worker/target/release/build"))? {
        let directory = entry?.path().join("out/skia");
        if !directory.is_dir() {
            continue;
        }
        let mut matches = true;
        for library in libraries {
            let name = library["name"]
                .as_str()
                .ok_or("Skia library name missing")?;
            if !name.ends_with(".lib") || name.contains(['/', '\\']) || name.contains("..") {
                return Err("invalid Skia receipt library path".into());
            }
            let path = directory.join(name);
            if !path.is_file()
                || library["sha256"].as_str() != Some(util::sha256_file(&path)?.as_str())
            {
                matches = false;
                break;
            }
        }
        if matches {
            return Ok(());
        }
    }
    Err("Built Skia libraries do not match the pinned prebuilt receipt; review the new native source/license provenance".into())
}

/// License, readme and third-party notices shipped with every package.
pub fn write_docs(dir: &Path, platform: &str) -> Result {
    let root = util::root();
    copy_legal_materials(&root.join("licenses"), &dir.join("licenses"))?;
    fs::copy(
        root.join("docs/hdr-and-platform-acceptance.md"),
        dir.join("HDR-PLATFORM-ACCEPTANCE.md"),
    )?;
    fs::copy(root.join("LICENSE"), dir.join("LICENSE.txt"))?;
    let ff_license = deps::ffmpeg_dir().join("LICENSE.txt");
    if ff_license.exists() {
        fs::create_dir_all(dir.join("licenses"))?;
        fs::copy(&ff_license, dir.join("licenses/FFmpeg-LICENSE.txt"))?;
    }
    let inter = root.join("crates/op-ui/assets/fonts/Inter-OFL.txt");
    fs::create_dir_all(dir.join("licenses"))?;
    fs::copy(&inter, dir.join("licenses/Inter-OFL.txt"))?;
    let version = util::version()?;
    let start = if platform == "windows" {
        format!(
            "Start {}.exe. To keep settings, caches and autosaves in this folder instead of your user profile, create an empty file named portable.txt next to it.",
            util::APP_NAME
        )
    } else if platform == "macos" {
        format!(
            "Drag {name}.app to Applications and open it. The app is not notarized: the first time, right-click it and choose Open (or allow it in System Settings > Privacy & Security).",
            name = util::APP_NAME
        )
    } else {
        "Make the AppImage executable (chmod +x) and start it. Integrate it with your desktop using your AppImage launcher of choice.".to_string()
    };
    let readme = format!(
        "OpenPremier {version}\r\n\
         A free and open source video editor for Windows and Linux.\r\n\r\n\
         {start}\r\n\r\n\
         Supported structures from Adobe Premiere Pro (.prproj) import read-only; Final Cut Pro XML and OpenTimelineIO interchange open through File > Open Project.\r\n\
         The keyboard shortcuts match the Premiere Pro defaults; your own .kys file can be imported in Edit > Keyboard Shortcuts.\r\n\r\n\
         Export Settings > Render Engine selects native GPU, native software, fframes CPU or fframes GPU. The worker stays beside the editor; Windows also needs its bundled MSVC runtime. Hybrid RGBA16F shares the editor's effect/transition/blend catalog; the independent SVG path covers a smaller SDR subset. GPU mode requires a physical Vulkan GPU on Windows/Linux or Metal on macOS, with no CPU fallback. Preferences selects asynchronous native/fframes previews.\r\n\r\n\
         Delivery Color selects SDR Rec.709 or HDR Rec.2100 PQ/HLG with 10-bit HEVC/ProRes and authored HDR10 metadata. See HDR-PLATFORM-ACCEPTANCE.md for validated scope and remaining grading/platform/UI limits.\r\n\r\n\
         License: GNU General Public License version 3 or later (LICENSE.txt).\r\n\
         Third-party components and their licenses: THIRD-PARTY-NOTICES.txt.\r\n\r\n\
         Isolated fframes worker notices: licenses/FFrames-THIRD-PARTY-NOTICES.txt.\r\n\r\n\
         Exact application source snapshot: OpenPremier-{version}-source.zip, alongside this package. Native-source/provenance review remains pending: licenses/DISTRIBUTION-REVIEW.md. Local validation is not redistribution approval.\r\n\r\n\
         Adobe and Premiere Pro are trademarks of Adobe Inc. OpenPremier is an independent project and is not affiliated with Adobe.\r\n"
    );
    fs::write(dir.join("README.txt"), readme)?;
    fs::write(dir.join("THIRD-PARTY-NOTICES.txt"), third_party_notices()?)?;
    let (rust_notices, sources) = crate::legal::notices(&crate::legal::metadata(false)?)?;
    fs::write(
        dir.join("licenses/Rust-THIRD-PARTY-NOTICES.txt"),
        rust_notices,
    )?;
    fs::write(
        dir.join("licenses/Rust-SOURCES.json"),
        serde_json::to_string_pretty(&sources)?,
    )?;
    fs::copy(root.join("NOTICE.md"), dir.join("NOTICE.md"))?;
    if platform == "windows" {
        fs::copy(
            root.join("licenses/skia/LICENSE_SKIA.txt"),
            dir.join("licenses/Skia-LICENSE.txt"),
        )?;
        fs::copy(
            root.join("licenses/skia/prebuilt-windows-x64.json"),
            dir.join("licenses/Skia-PREBUILT.json"),
        )?;
        fs::copy(
            root.join("docs/legal/skia-and-msvc.md"),
            dir.join("licenses/Skia-MSVC-REVIEW.md"),
        )?;
    }
    fs::copy(
        root.join("docs/legal/audit-2026-10-07.md"),
        dir.join("licenses/DISTRIBUTION-REVIEW.md"),
    )?;
    fs::write(
        dir.join("licenses/Native-BINARIES.json"),
        serde_json::to_string_pretty(&crate::legal::native_inventory()?)?,
    )?;
    {
        // Every platform now bundles the worker and retains its original notices.
        let meta_text = util::output(
            Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
                .current_dir(util::root())
                .args([
                    "metadata",
                    "--format-version",
                    "1",
                    "--locked",
                    "--manifest-path",
                    "tools/fframes-worker/Cargo.toml",
                ]),
        )?;
        let meta: serde_json::Value = serde_json::from_str(&meta_text)?;
        let mut notices =
            String::from("OpenPremier-FFrames worker dependencies (separate Cargo graph):\n\n");
        // Published fframes crates omit their workspace-level license file. Ship the exact
        // upstream notice explicitly rather than relying only on package license metadata.
        notices.push_str("fframes workspace MIT notice (upstream revision 30b48f3da60040e0fd7c811fdcd5b159e70a5b02):\n");
        notices.push_str(&fs::read_to_string(
            root.join("tools/fframes-worker/licenses/fframes-MIT.txt"),
        )?);
        notices.push_str("\n\n");
        for package in meta["packages"]
            .as_array()
            .ok_or("missing worker packages")?
        {
            notices.push_str(&format!(
                "{} {} — {}\n{}\n\n",
                package["name"].as_str().unwrap_or(""),
                package["version"].as_str().unwrap_or(""),
                package["license"].as_str().unwrap_or(""),
                package["repository"].as_str().unwrap_or("")
            ));
            if let Some(manifest) = package["manifest_path"].as_str() {
                let root = Path::new(manifest).parent().ok_or("invalid crate path")?;
                for name in [
                    "LICENSE",
                    "LICENSE.txt",
                    "LICENSE-MIT",
                    "LICENSE-APACHE",
                    "LICENSE-MPL",
                ] {
                    let path = root.join(name);
                    if path.is_file()
                        && let Ok(license) = fs::read_to_string(&path)
                    {
                        notices.push_str(&license);
                        notices.push_str("\n\n");
                    }
                }
            }
        }
        fs::write(
            dir.join("licenses/FFrames-THIRD-PARTY-NOTICES.txt"),
            notices,
        )?;
        let (complete, sources) = crate::legal::notices(&meta)?;
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(dir.join("licenses/FFrames-THIRD-PARTY-NOTICES.txt"))?;
        file.write_all(complete.as_bytes())?;
        fs::write(
            dir.join("licenses/FFrames-SOURCES.json"),
            serde_json::to_string_pretty(&sources)?,
        )?;
    }
    Ok(())
}

fn copy_legal_materials(source: &Path, target: &Path) -> Result {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err("license material must not follow a symlink".into());
        }
        let to = target.join(entry.file_name());
        if kind.is_dir() {
            copy_legal_materials(&entry.path(), &to)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

/// Every Rust crate in the lock file with its license, plus FFmpeg.
fn third_party_notices() -> Result<String> {
    let out = util::output(
        Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .current_dir(util::root())
            .args(["metadata", "--format-version", "1", "--locked"]),
    )?;
    let meta: serde_json::Value = serde_json::from_str(&out)?;
    let members: Vec<String> = meta["workspace_members"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let mut rows: Vec<(String, String, String, String)> = meta["packages"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|p| !members.iter().any(|m| Some(m.as_str()) == p["id"].as_str()))
                .map(|p| {
                    let s = |k: &str| p[k].as_str().unwrap_or("").to_string();
                    (
                        s("name"),
                        s("version"),
                        if s("license").is_empty() {
                            s("license_file")
                        } else {
                            s("license")
                        },
                        s("repository"),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    rows.sort();
    rows.dedup();
    let mut text = String::new();
    text.push_str("OpenPremier includes or links the following third-party software.\r\n\r\n");
    text.push_str("FFmpeg 8.1 (https://ffmpeg.org), shared libraries built by https://github.com/BtbN/FFmpeg-Builds with GPL components.\r\n");
    text.push_str(
        "License: GNU General Public License version 3 or later (licenses/FFmpeg-LICENSE.txt).\r\n",
    );
    text.push_str("Upstream source locations: https://ffmpeg.org/releases/ and https://github.com/BtbN/FFmpeg-Builds. These generic URLs are not proof of corresponding source for the supplied DLLs. Redistribution review remains pending (licenses/DISTRIBUTION-REVIEW.md).\r\n\r\n");
    text.push_str("Original Rust copyright/license/NOTICE texts: licenses/Rust-THIRD-PARTY-NOTICES.txt. Exact crate sources/checksums: licenses/Rust-SOURCES.json.\r\n\r\n");
    text.push_str("Inter typeface (Inter Regular and SemiBold), Copyright (c) 2016 The Inter Project Authors, SIL Open Font License 1.1 (licenses/Inter-OFL.txt), https://github.com/rsms/inter

");
    text.push_str("Speech models for automatic captions are not included: OpenAI Whisper (tiny, base or small; MIT License, https://github.com/openai/whisper) is downloaded from https://huggingface.co/openai when the user asks for captions.\r\n\r\n");
    text.push_str("Rust crates (name, version, license, repository):\r\n\r\n");
    for (n, v, l, r) in rows {
        text.push_str(&format!("{n} {v}\r\n    License: {l}\r\n"));
        if !r.is_empty() {
            text.push_str(&format!("    {r}\r\n"));
        }
    }
    Ok(text)
}

/// Runs the packaged program's self-test without the development FFmpeg on the search path.
pub fn self_test(program: &Path) -> Result {
    let mut cmd = Command::new(program);
    cmd.arg("--self-test");
    if cfg!(windows) {
        let system = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("C:\\Windows"));
        cmd.env("PATH", system.join("System32"));
    } else {
        cmd.env_remove("LD_LIBRARY_PATH");
        cmd.env_remove("DYLD_LIBRARY_PATH");
        cmd.env_remove("DYLD_FALLBACK_LIBRARY_PATH");
    }
    let out = cmd
        .output()
        .map_err(|e| format!("cannot start {}: {e}", program.display()))?;
    let text = String::from_utf8_lossy(&out.stdout);
    eprint!("{text}");
    if !out.status.success() || !text.contains("self-test passed") {
        return Err(format!(
            "self-test of {} failed: {}",
            program.display(),
            String::from_utf8_lossy(&out.stderr)
        )
        .into());
    }
    Ok(())
}

fn zip_dir(dir: &Path, to: &Path, prefix: &str) -> Result {
    let file = fs::File::create_new(to)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(9));
    let mut files = Vec::new();
    collect(dir, &mut files)?;
    files.sort();
    for f in files {
        let rel = f.strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
        zip.start_file(format!("{prefix}/{rel}"), options)?;
        zip.write_all(&fs::read(&f)?)?;
    }
    zip.finish()?;
    Ok(())
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> Result {
    for e in fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            collect(&p, out)?;
        } else {
            out.push(p);
        }
    }
    Ok(())
}

/// dist/SHA256SUMS.txt for every package in dist/.
pub fn write_checksums() -> Result {
    let dist = util::dist_dir();
    let mut lines = Vec::new();
    let mut entries: Vec<PathBuf> = fs::read_dir(&dist)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if name.ends_with(".zip") || name.ends_with(".AppImage") || name.ends_with(".dmg") {
            lines.push(format!("{}  {name}", util::sha256_file(&p)?));
        }
    }
    fs::write(dist.join("SHA256SUMS.txt"), lines.join("\n") + "\n")?;
    eprintln!("{}", lines.join("\n"));
    Ok(())
}
