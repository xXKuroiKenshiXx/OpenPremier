//! File > Downgrade Adobe Premiere Project: an independent file utility.

use std::sync::mpsc::{Receiver, TryRecvError};

use op_project::prproj_version::{self as versions, VersionInfo};

use super::*;

enum JobResult {
    Loaded(Vec<u8>, VersionInfo),
    Saved(PathBuf),
}

#[derive(Default)]
pub struct VersionForm {
    source: Option<PathBuf>,
    bytes: Vec<u8>,
    info: Option<VersionInfo>,
    target: u32,
    job: Option<Receiver<Result<JobResult, String>>>,
    message: Option<String>,
}

pub(super) fn version_dialog(s: &mut State, ctx: &egui::Context, f: &mut VersionForm) -> bool {
    if let Some(rx) = &f.job {
        let result = match rx.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("The compatibility job stopped.".into())),
        };
        if let Some(r) = result {
            f.job = None;
            match r {
                Ok(JobResult::Loaded(bytes, info)) => {
                    f.bytes = bytes;
                    f.info = Some(info);
                    f.target = 1;
                }
                Ok(JobResult::Saved(path)) => {
                    let message = tf("Downgraded copy saved to {}", &[&path.display()]);
                    s.ed.info(&message);
                    f.message = Some(message);
                }
                Err(e) => f.message = Some(e),
            }
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
    let busy = f.job.is_some();
    let mut close = false;
    let (_, esc) = modal(
        ctx,
        "prproj-version",
        t("Downgrade Adobe Premiere Project..."),
        560.0,
        |ui| {
            ui.label(t("Save a copy of a .prproj file to try opening it in an older Adobe Premiere. Choose the file to downgrade, then save the new copy."));
            ui.add_space(8.0);
            if ui
                .add_enabled(!busy, egui::Button::new(t("Choose Premiere Project...")))
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Premiere", &["prproj"])
                    .pick_file()
            {
                f.info = None;
                f.bytes.clear();
                f.message = None;
                f.source = Some(path.clone());
                let (tx, rx) = std::sync::mpsc::channel();
                match std::thread::Builder::new()
                    .name("inspect-prproj".into())
                    .spawn(move || {
                        let result = (|| {
                            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                            let info = versions::inspect(&bytes).map_err(|e| e.to_string())?;
                            Ok(JobResult::Loaded(bytes, info))
                        })();
                        let _ = tx.send(result);
                    }) {
                    Ok(_) => f.job = Some(rx),
                    Err(e) => f.message = Some(e.to_string()),
                }
            }
            if let Some(path) = &f.source {
                ui.label(path.display().to_string());
            }
            if busy {
                ui.spinner();
            }
            if let Some(info) = &f.info {
                ui.label(tf(
                    "Container version: {}; Project version: {}",
                    &[&info.container_version, &info.project_version],
                ));
                ui.horizontal(|ui| {
                    ui.label(t("Target Project version"));
                    ui.add_enabled(
                        !busy,
                        egui::DragValue::new(&mut f.target).range(1..=info.project_version),
                    );
                });
                ui.label(t("This is an internal serialization number, not a Premiere release year. Version 1 is the workaround used by downgrade-premiere-prproj."));
                ui.label(RichText::new(t("Only the version attribute changes. New features are not converted and may be lost or fail to open in older Premiere. Test the copy before using it." )).color(theme::WARN));
                if ui
                    .add_enabled(!busy, egui::Button::new(t("Save Downgraded Copy...")))
                    .clicked()
                {
                    let source = f.source.as_ref().unwrap();
                    let name = format!(
                        "{}-downgraded.prproj",
                        source.file_stem().unwrap_or_default().to_string_lossy()
                    );
                    let dialog = rfd::FileDialog::new()
                        .add_filter("Premiere", &["prproj"])
                        .set_directory(source.parent().unwrap_or(Path::new(".")))
                        .set_file_name(name);
                    if let Some(output) = dialog.save_file() {
                        let bytes = f.bytes.clone();
                        let target = f.target;
                        let (tx, rx) = std::sync::mpsc::channel();
                        match std::thread::Builder::new()
                            .name("downgrade-prproj".into())
                            .spawn(move || {
                                let result = versions::save_copy(&bytes, target, &output)
                                    .map(|()| JobResult::Saved(output))
                                    .map_err(|e| e.to_string());
                                let _ = tx.send(result);
                            }) {
                            Ok(_) => {
                                f.job = Some(rx);
                                f.message = None;
                            }
                            Err(e) => f.message = Some(e.to_string()),
                        }
                    }
                }
            }
            if let Some(message) = &f.message {
                ui.label(message);
            }
            ui.separator();
            if ui
                .add_enabled(!busy, egui::Button::new(t("Close")))
                .clicked()
            {
                close = true;
            }
        },
    );
    !(close || esc && !busy)
}
