//! Help > Check for Updates, and the notice when the program finds a newer version on start:
//! what is new, download with progress, and install when the program closes.

use op_application::update::{self, Download, Release};

use super::*;

pub enum Stage {
    Checking,
    UpToDate,
    Failed(String),
    Available,
    Downloading(Download),
    /// Downloaded and checked; installs on close.
    Ready,
}

pub struct UpdateForm {
    pub release: Option<Release>,
    pub stage: Stage,
}

impl UpdateForm {
    pub fn checking() -> UpdateForm {
        UpdateForm {
            release: None,
            stage: Stage::Checking,
        }
    }

    pub fn available(release: Release) -> UpdateForm {
        UpdateForm {
            release: Some(release),
            stage: Stage::Available,
        }
    }
}

/// Starts a check: `manual` shows the result even when there is nothing new.
pub fn start_check(s: &mut State, manual: bool) {
    if s.update_check.is_some() {
        return;
    }
    s.update_check = Some((update::Check::start(), manual));
    if manual && !s.dialogs.iter().any(|d| matches!(d, Dialog::Update(_))) {
        s.dialogs
            .push(Dialog::Update(Box::new(UpdateForm::checking())));
    }
}

/// Picks up a finished check.
pub fn poll(s: &mut State) {
    let Some((check, manual)) = &s.update_check else {
        return;
    };
    let Some(result) = check.take() else {
        s.ctx
            .request_repaint_after(std::time::Duration::from_millis(250));
        return;
    };
    let manual = *manual;
    s.update_check = None;
    let open = s.dialogs.iter_mut().find_map(|d| match d {
        Dialog::Update(f) => Some(f),
        _ => None,
    });
    match result {
        Ok(Some(r)) => {
            if !manual && s.ed.prefs.skipped_update == r.version {
                return;
            }
            match open {
                Some(f) => **f = UpdateForm::available(r),
                None => s
                    .dialogs
                    .push(Dialog::Update(Box::new(UpdateForm::available(r)))),
            }
        }
        Ok(None) => {
            if let Some(f) = open {
                f.stage = Stage::UpToDate;
            }
        }
        Err(e) => {
            if let Some(f) = open {
                f.stage = Stage::Failed(e);
            }
        }
    }
}

pub(super) fn update_dialog(s: &mut State, ctx: &egui::Context, f: &mut UpdateForm) -> bool {
    let mut open = true;
    let mut close = false;
    window(ctx, t("Software Update"))
        .open(&mut open)
        .resizable(false)
        .show(ctx, |ui| {
            ui.set_width(460.0);
            let current = update::current_version();
            match &mut f.stage {
                Stage::Checking => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(t("Looking for a newer version..."));
                    });
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
                Stage::UpToDate => {
                    ui.label(tf("OpenPremier {} is the newest version.", &[&current]));
                    ui.add_space(8.0);
                    if ui.button(t("Close")).clicked() {
                        close = true;
                    }
                }
                Stage::Failed(e) => {
                    ui.label(RichText::new(t("Could not look for updates.")).color(theme::ERROR));
                    ui.label(RichText::new(e.as_str()).size(11.0).color(theme::TEXT_DIM));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(t("Open the Releases Page")).clicked() {
                            update::open_url(update::RELEASES_PAGE);
                        }
                        if ui.button(t("Close")).clicked() {
                            close = true;
                        }
                    });
                }
                Stage::Available => {
                    let Some(r) = f.release.clone() else {
                        close = true;
                        return;
                    };
                    ui.label(
                        RichText::new(tf("OpenPremier {} is available", &[&r.version]))
                            .size(15.0)
                            .color(theme::TEXT_BRIGHT),
                    );
                    ui.label(
                        RichText::new(tf("You have version {}.", &[&current]))
                            .color(theme::TEXT_DIM),
                    );
                    ui.add_space(6.0);
                    notes(ui, &r.notes);
                    ui.add_space(8.0);
                    let installable = update::install_target().is_ok() || cfg!(target_os = "macos");
                    if r.package.is_none() || !installable {
                        ui.label(
                            RichText::new(t(
                                "This copy cannot update itself; download the new version from the releases page.",
                            ))
                            .size(11.5)
                            .color(theme::TEXT_DIM),
                        );
                    }
                    ui.horizontal(|ui| {
                        if r.package.is_some()
                            && installable
                            && ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(t("Download and Install"))
                                            .color(egui::Color32::WHITE),
                                    )
                                    .fill(theme::ACCENT_DIM),
                                )
                                .clicked()
                        {
                            f.stage = Stage::Downloading(Download::start(
                                r.clone(),
                                s.ed.dirs.cache.join("Updates"),
                            ));
                        }
                        if ui.button(t("What's New")).clicked() {
                            update::open_url(&r.page);
                        }
                        if ui.button(t("Later")).clicked() {
                            close = true;
                        }
                        if ui.button(t("Skip This Version")).clicked() {
                            s.ed.prefs.skipped_update = r.version.clone();
                            let _ = s.ed.prefs.save(&s.ed.dirs);
                            close = true;
                        }
                    });
                }
                Stage::Downloading(d) => {
                    let (done, total) = *d.progress.lock();
                    let finished = d.result.lock().take();
                    match finished {
                        Some(Ok(install)) => {
                            s.update_install = Some(install);
                            f.stage = Stage::Ready;
                        }
                        Some(Err(e)) if e == "cancelled" => f.stage = Stage::Available,
                        Some(Err(e)) => f.stage = Stage::Failed(e),
                        None => {
                            ui.label(t("Downloading the new version..."));
                            let frac = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                            ui.add(egui::ProgressBar::new(frac).show_percentage());
                            ui.label(
                                RichText::new(format!(
                                    "{:.1} / {:.1} MB",
                                    done as f64 / 1e6,
                                    total as f64 / 1e6
                                ))
                                .size(11.0)
                                .color(theme::TEXT_DIM),
                            );
                            ui.label(
                                RichText::new(t("You can keep working while it downloads."))
                                    .size(11.0)
                                    .color(theme::TEXT_DIM),
                            );
                            if ui.button(t("Cancel")).clicked() {
                                d.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                            ctx.request_repaint_after(std::time::Duration::from_millis(200));
                        }
                    }
                }
                Stage::Ready => {
                    let version = f.release.as_ref().map(|r| r.version.clone()).unwrap_or_default();
                    ui.label(
                        RichText::new(tf("OpenPremier {} is ready to install.", &[&version]))
                            .size(15.0)
                            .color(theme::TEXT_BRIGHT),
                    );
                    ui.label(if cfg!(target_os = "macos") {
                        t("When OpenPremier closes, the downloaded disk image opens: drag OpenPremier to Applications.")
                    } else {
                        t("It is installed when OpenPremier closes, and the new version opens by itself. You are asked to save your work first.")
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(t("Restart and Update"))
                                        .color(egui::Color32::WHITE),
                                )
                                .fill(theme::ACCENT_DIM),
                            )
                            .clicked()
                        {
                            s.command("cmd.file.exit");
                            close = true;
                        }
                        if ui.button(t("When I Close OpenPremier")).clicked() {
                            close = true;
                        }
                    });
                }
            }
        });
    open && !close
}

/// The first lines of the release notes, without the banner image.
fn notes(ui: &mut Ui, text: &str) {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("!["))
        .take(6)
        .collect();
    if lines.is_empty() {
        return;
    }
    egui::Frame::new()
        .fill(theme::PANEL_DARK)
        .corner_radius(4)
        .inner_margin(8)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            for l in lines {
                let plain = l.replace("**", "").replace('`', "");
                let heading = plain.starts_with('#');
                let plain = plain.trim_start_matches('#').trim().to_string();
                ui.label(if heading {
                    RichText::new(plain).strong().color(theme::TEXT_BRIGHT)
                } else {
                    RichText::new(plain).size(11.5)
                });
            }
        });
}
