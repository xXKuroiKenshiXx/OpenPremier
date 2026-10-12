//! Native build dependencies.
//!
//! * FFmpeg 8.1 development files and shared libraries (GPL build from BtbN/FFmpeg-Builds),
//!   verified against the checksums published with each build. Every platform builds and ships
//!   the same FFmpeg release. `FFMPEG_DIR` overrides the download.
//! * libclang for bindgen on Windows (the `libclang` wheel on PyPI, pinned by SHA-256).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util::{self, Result, third_party};

const FFMPEG_BASE: &str =
    "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-09-28-13-06";
const LIBCLANG_WHEEL: (&str, &str) = (
    "https://files.pythonhosted.org/packages/0b/2d/3f480b1e1d31eb3d6de5e3ef641954e5c67430d5ac93b7fa7e07589576c7/libclang-18.1.1-py2.py3-none-win_amd64.whl",
    "4dd2d3b82fab35e2bf9ca717d7b63ac990a3519c7e312f19fa8e86dcc712f7fb",
);
const WHEEL_DLL: &str = "libclang-18.1.1.data/platlib/clang/native/libclang.dll";

pub fn ffmpeg_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("FFMPEG_DIR") {
        return PathBuf::from(d);
    }
    // on macOS the Homebrew FFmpeg is used (`brew install ffmpeg`)
    if cfg!(target_os = "macos")
        && let Ok(out) = Command::new("brew").args(["--prefix", "ffmpeg"]).output()
        && out.status.success()
    {
        return PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    }
    third_party().join("ffmpeg")
}

fn ffmpeg_archive() -> Result<(String, &'static str, &'static str)> {
    let (os, ext, digest) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => (
            "win64",
            ".zip",
            "79ac04d9b71462e1a3f5f1ea9be3c97856a8791eb29b11835f80b2629fea12a1",
        ),
        ("windows", "aarch64") => (
            "winarm64",
            ".zip",
            "2b4968788ad52080216d3437200e393f0ef1b72e2fa222452b84303d801d3af2",
        ),
        ("linux", "x86_64") => (
            "linux64",
            ".tar.xz",
            "57523ecdd62e78eda9a7571e431be6c40e39cd371a8c6e7ddfbf3c2676487d58",
        ),
        ("linux", "aarch64") => (
            "linuxarm64",
            ".tar.xz",
            "627a243ac5456dd8d48f088c284c0895ca193df541c595d9946994a1e7925027",
        ),
        (os, arch) => {
            return Err(format!(
                "no FFmpeg build for {os}/{arch}; install FFmpeg 8 and set FFMPEG_DIR"
            )
            .into());
        }
    };
    Ok((
        format!("ffmpeg-n8.1.3-6-gff48edd8b2-{os}-gpl-shared-8.1"),
        ext,
        digest,
    ))
}

pub fn ensure_ffmpeg() -> Result {
    if std::env::var_os("FFMPEG_DIR").is_some() {
        return Ok(());
    }
    if cfg!(target_os = "macos") {
        return if ffmpeg_dir().join("include").join("libavcodec").exists() {
            Ok(())
        } else {
            Err("FFmpeg 8 is needed: brew install ffmpeg".into())
        };
    }
    let dest = ffmpeg_dir();
    if dest.join("include").join("libavcodec").exists() {
        return Ok(());
    }
    let (name, ext, pinned_digest) = ffmpeg_archive()?;
    let work = third_party();
    fs::create_dir_all(&work)?;
    let sums = work.join("checksums.sha256");
    util::download(&format!("{FFMPEG_BASE}/checksums.sha256"), &sums)?;
    let file = format!("{name}{ext}");
    let expected = fs::read_to_string(&sums)?
        .lines()
        .find(|l| l.split_whitespace().nth(1) == Some(file.as_str()))
        .and_then(|l| l.split_whitespace().next().map(str::to_string))
        .ok_or_else(|| format!("{file} is not in the published checksums"))?;
    let archive = work.join(&file);
    if expected != pinned_digest {
        return Err("FFmpeg release checksum differs from the pinned build identity".into());
    }
    util::fetch_verified(&format!("{FFMPEG_BASE}/{file}"), &expected, &archive)?;
    util::run(util::tar().arg("-xf").arg(&archive).arg("-C").arg(&work))?;
    if dest.exists() {
        return Err(
            "FFmpeg destination exists but is incomplete; preserve it before retrying installation"
                .into(),
        );
    }
    fs::rename(work.join(&name), &dest)?;
    // Keep provenance and the verified archive instead of losing the exact acquired build.
    fs::write(
        dest.join("BUILD-RECEIPT.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "download_url": format!("{FFMPEG_BASE}/{file}"),
            "archive": file,
            "archive_sha256": expected,
            "checksum_url": format!("{FFMPEG_BASE}/checksums.sha256"),
            "checksum_file_sha256": util::sha256_file(&sums)?,
            "downloaded_at_unix_seconds": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
            "corresponding_source_review": "pending: download integrity is not source/license verification"
        }))?,
    )?;
    fs::copy(&sums, dest.join("DOWNLOAD-CHECKSUMS.sha256"))?;
    eprintln!("FFmpeg 8.1 ready in {} (SHA-256 verified)", dest.display());
    Ok(())
}

fn libclang_dir() -> Result<Option<PathBuf>> {
    if !cfg!(windows) {
        return Ok(None);
    }
    if let Some(p) = std::env::var_os("LIBCLANG_PATH") {
        return Ok(Some(PathBuf::from(p)));
    }
    let dir = third_party().join("libclang");
    if dir.join("libclang.dll").exists() {
        return Ok(Some(dir));
    }
    fs::create_dir_all(&dir)?;
    let wheel = third_party().join("libclang.whl");
    util::fetch_verified(LIBCLANG_WHEEL.0, LIBCLANG_WHEEL.1, &wheel)?;
    let mut zip = zip::ZipArchive::new(fs::File::open(&wheel)?)?;
    let mut entry = zip.by_name(WHEEL_DLL)?;
    let mut out = fs::File::create(dir.join("libclang.dll"))?;
    std::io::copy(&mut entry, &mut out)?;
    drop(entry);
    fs::remove_file(&wheel)?;
    eprintln!("libclang ready in {}", dir.display());
    Ok(Some(dir))
}

/// Makes sure native dependencies exist and configures a cargo command to use them.
pub fn configure(cmd: &mut Command) -> Result {
    ensure_ffmpeg()?;
    let ff = ffmpeg_dir();
    cmd.env("FFMPEG_DIR", &ff);
    if let Some(clang) = libclang_dir()? {
        cmd.env("LIBCLANG_PATH", clang);
    }
    runtime_env(cmd, &ff);
    Ok(())
}

/// Lets programs started from cargo find the FFmpeg shared libraries.
pub fn runtime_env(cmd: &mut Command, ff: &Path) {
    if cfg!(windows) {
        cmd.env("PATH", util::prepend_path("PATH", &ff.join("bin")));
    } else if cfg!(target_os = "macos") {
        cmd.env(
            "DYLD_FALLBACK_LIBRARY_PATH",
            util::prepend_path("DYLD_FALLBACK_LIBRARY_PATH", &ff.join("lib")),
        );
    } else {
        cmd.env(
            "LD_LIBRARY_PATH",
            util::prepend_path("LD_LIBRARY_PATH", &ff.join("lib")),
        );
    }
}

/// Shared libraries to ship next to the program.
pub fn runtime_libraries() -> Result<Vec<PathBuf>> {
    let ff = ffmpeg_dir();
    let mut out = Vec::new();
    if cfg!(windows) {
        for e in fs::read_dir(ff.join("bin"))? {
            let p = e?.path();
            if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dll")) {
                out.push(p);
            }
        }
    } else {
        for e in fs::read_dir(ff.join("lib"))? {
            let p = e?.path();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            // the versioned libraries (libavcodec.so.62, libavcodec.62.dylib), not the
            // development symlinks
            let versioned = if cfg!(target_os = "macos") {
                name.ends_with(".dylib")
            } else {
                name.contains(".so.")
            };
            if versioned && name.matches('.').count() == 2 {
                out.push(p);
            }
        }
    }
    if out.is_empty() {
        return Err(format!("no FFmpeg libraries in {}", ff.display()).into());
    }
    Ok(out)
}

/// The unmodified, redistributable x64 CRT from Visual Studio, never System32 or DebugCRT.
/// Publication still requires the documented native license review.
pub fn fframes_msvc_runtime() -> Result<Vec<PathBuf>> {
    if !cfg!(windows) {
        return Ok(Vec::new());
    }
    let redist = if let Some(path) = std::env::var_os("VCToolsRedistDir") {
        PathBuf::from(path)
    } else {
        let program_files =
            std::env::var_os("ProgramFiles(x86)").ok_or("ProgramFiles(x86) missing")?;
        let vswhere =
            PathBuf::from(program_files).join("Microsoft Visual Studio/Installer/vswhere.exe");
        let installation = util::output(Command::new(vswhere).args([
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ]))?;
        let directory = PathBuf::from(installation.trim()).join("VC/Redist/MSVC");
        let mut versions: Vec<_> = fs::read_dir(directory)?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("14."))
            })
            .collect();
        versions.sort();
        versions
            .pop()
            .ok_or("MSVC redist not installed; set VCToolsRedistDir")?
    };
    let mut files = Vec::new();
    for entry in fs::read_dir(redist.join("x64"))? {
        let path = entry?.path();
        if path.file_name().is_some_and(|name| {
            let name = name.to_string_lossy();
            name.starts_with("Microsoft.VC") && name.ends_with(".CRT")
        }) {
            for name in ["msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll"] {
                let file = path.join(name);
                if !file.is_file() {
                    return Err(format!("MSVC redistributable missing: {}", file.display()).into());
                }
                files.push(file);
            }
            return Ok(files);
        }
    }
    Err("x64 MSVC redistributable CRT directory missing".into())
}
