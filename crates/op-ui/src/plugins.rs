//! Native extension tools; the browser is deliberately offline and has no provider.
use crate::{
    app::State,
    dialogs,
    i18n::{t, tn},
    theme,
};
use egui::{RichText, Ui};
use op_application::plugins::{Action, ActionMacro, FolderPortal};
use op_application::{Chord, Focus};
use op_core::catalog::{self, EffectKind};

#[derive(Default)]
pub struct PluginsView {
    pub open: bool,
    pub actions_open: bool,
    search: String,
    selected: usize,
    favorites_only: bool,
    show_hidden: bool,
    draft: Vec<Action>,
    name: String,
    shortcut: String,
}

pub fn settings(ui: &mut Ui, p: &mut op_application::plugins::NativePlugins) {
    ui.heading(t("Plugins"));
    ui.label(t("Installed / Included with Open Premiere"));
    ui.checkbox(
        &mut p.motion_curves,
        t("Motion Curves — animation curve editor"),
    );
    ui.checkbox(
        &mut p.folder_portals,
        t("Folder Portals — favorite folders"),
    );
    ui.checkbox(
        &mut p.quick_actions,
        t("Quick Actions — effects, commands and macros"),
    );
    ui.small(t(
        "Disabling a tool keeps your saved settings and project animations.",
    ));
    ui.collapsing(t("Plugin Browser"), |ui| {
        ui.label(t(
            "No catalog connected yet. This browser does not search or download plugins.",
        ));
    });
    ui.small(t(
        "Included native adaptations. Adobe extensions and VST files cannot be loaded here.",
    ));
}

fn name(a: &Action) -> String {
    match a {
        Action::Effect(id) => catalog::find(id)
            .map(|d| tn(d.name))
            .or_else(|| op_core::presets::find(id).map(|d| tn(d.name)))
            .unwrap_or_else(|| id.clone()),
        Action::Command(id) => dialogs::command_name(id),
    }
}
fn id(a: &Action) -> &str {
    match a {
        Action::Effect(id) | Action::Command(id) => id,
    }
}

pub fn run(s: &mut State, a: &Action) {
    if !s.ed.prefs.plugins.quick_actions {
        return;
    }
    match a {
        Action::Command(id) if op_application::commands::EDITOR_COMMANDS.contains(&id.as_str()) => {
            // Editor commands run in timeline context, regardless of the palette's text focus.
            let focus = s.focus;
            s.focus = Focus::Timeline;
            s.command(id);
            s.focus = focus;
        }
        Action::Effect(id) => {
            if let Some(d) = catalog::find(id)
                && d.kind.is_transition()
            {
                let kind = if d.kind.is_video() {
                    op_core::TrackKind::Video
                } else {
                    op_core::TrackKind::Audio
                };
                let seconds = if d.kind.is_video() {
                    s.ed.prefs.video_transition_seconds
                } else {
                    s.ed.prefs.audio_transition_seconds
                };
                let clips = s.ed.selection.clips.clone();
                let at = s.ed.playhead();
                let targets = s.ed.targets();
                s.ed.seq_edit("Add Transition", |p, sid, opts| {
                    let seq = p.sequence(sid).ok_or(op_core::EditError::Nothing)?.clone();
                    let cuts: Vec<_> = if clips.is_empty() {
                        targets
                            .iter()
                            .filter(|r| r.kind == kind)
                            .map(|r| (*r, at))
                            .collect()
                    } else {
                        clips
                            .iter()
                            .filter_map(|id| seq.find_clip(*id))
                            .filter(|(r, _)| r.kind == kind)
                            .flat_map(|(r, c)| [(r, c.start), (r, c.end())])
                            .collect()
                    };
                    let mut any = false;
                    for (r, t) in cuts {
                        if op_timeline::add_transition(
                            p,
                            sid,
                            r,
                            t,
                            id,
                            op_core::Dur::from_seconds(seconds),
                            None,
                            opts,
                        )
                        .is_ok()
                        {
                            any = true;
                        }
                    }
                    if any {
                        Ok(())
                    } else {
                        Err(op_core::EditError::Nothing)
                    }
                });
            } else {
                s.ed.apply_effect(id, None);
            }
        }
        _ => {}
    }
}

pub fn hotkey(s: &mut State, chord: &Chord) -> bool {
    if !s.ed.prefs.plugins.quick_actions {
        return false;
    }
    if s.ed
        .keymap
        .bindings
        .iter()
        .any(|b| Chord::parse(&b.keys).as_ref() == Some(chord))
    {
        return false;
    }
    let actions =
        s.ed.prefs
            .plugins
            .macros
            .iter()
            .find(|m| Chord::parse(&m.shortcut).as_ref() == Some(chord))
            .map(|m| m.actions.clone());
    if let Some(actions) = actions {
        for a in actions {
            run(s, &a);
        }
        true
    } else {
        false
    }
}

pub fn show(s: &mut State, ctx: &egui::Context) {
    let before = s.ed.prefs.plugins.clone();
    if s.plugins.open {
        let mut open = true;
        egui::Window::new(t("Plugins"))
            .id(egui::Id::new("native-plugins"))
            .open(&mut open)
            .default_width(560.0)
            .show(ctx, |ui| {
                settings(ui, &mut s.ed.prefs.plugins);
                ui.separator();
                ui.add_enabled_ui(s.ed.prefs.plugins.motion_curves, |ui| {
                    if ui.button(t("Open Motion Curves...")).clicked() {
                        s.curves.open = true;
                    }
                });
                ui.add_enabled_ui(s.ed.prefs.plugins.quick_actions, |ui| {
                    if ui.button(t("Open Quick Actions...")).clicked() {
                        s.plugins.actions_open = true;
                    }
                });
                ui.add_enabled_ui(s.ed.prefs.plugins.folder_portals, |ui| portals(s, ui));
            });
        s.plugins.open = open;
    }
    if !s.ed.prefs.plugins.quick_actions {
        s.plugins.actions_open = false;
    }
    if s.plugins.actions_open {
        actions(s, ctx);
    }
    if before != s.ed.prefs.plugins {
        let _ = s.ed.prefs.save(&s.ed.dirs);
    }
}

fn portals(s: &mut State, ui: &mut Ui) {
    ui.heading(t("Folder Portals"));
    let mut remove = None;
    for (i, p) in s.ed.prefs.plugins.folders.clone().iter().enumerate() {
        ui.push_id(i, |ui| {
            let resolved = p.resolve(s.ed.path.as_deref());
            let exists = resolved.as_ref().is_some_and(|p| p.is_dir());
            ui.horizontal(|ui| {
                if let Some(saved) = s.ed.prefs.plugins.folders.get_mut(i) {
                    ui.text_edit_singleline(&mut saved.name);
                }
                if ui
                    .add_enabled(exists, egui::Button::new(t("Open")))
                    .clicked()
                    && let Some(path) = &resolved
                {
                    crate::app::open_folder(path);
                }
                if ui
                    .add_enabled(exists, egui::Button::new(t("Import...")))
                    .clicked()
                    && let Some(path) = &resolved
                    && let Some(files) = rfd::FileDialog::new().set_directory(path).pick_files()
                {
                    s.ed.import(files, s.ed.project.root);
                }
                if ui.small_button("×").clicked() {
                    remove = Some(i);
                }
            });
            let mut path = p.path.display().to_string();
            if ui.text_edit_singleline(&mut path).changed() {
                s.ed.prefs.plugins.folders[i].path = path.into();
            }
            if !exists {
                ui.small(t(
                    "Folder unavailable. Relative paths need a saved project.",
                ));
            }
        });
    }
    if let Some(i) = remove {
        s.ed.prefs.plugins.folders.remove(i);
    }
    ui.horizontal(|ui| {
        if ui.button(t("Add Folder...")).clicked()
            && let Some(path) = rfd::FileDialog::new().pick_folder()
        {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            s.ed.prefs.plugins.folders.push(FolderPortal { name, path });
        }
        if ui.button(t("Add Project Media Folder")).clicked() {
            s.ed.prefs.plugins.folders.push(FolderPortal {
                name: t("Project Media").into(),
                path: "media".into(),
            });
        }
    });
}

fn toggle(list: &mut Vec<String>, id: &str) {
    if list.iter().any(|s| s == id) {
        list.retain(|s| s != id);
    } else {
        list.push(id.into());
    }
}

fn actions(s: &mut State, ctx: &egui::Context) {
    let mut open = true;
    egui::Window::new(t("Quick Actions"))
        .id(egui::Id::new("quick-actions"))
        .open(&mut open)
        .default_width(640.0)
        .show(ctx, |ui| {
            let search = ui.add(
                egui::TextEdit::singleline(&mut s.plugins.search)
                    .hint_text(t("Search effects, presets, transitions and commands"))
                    .desired_width(f32::INFINITY),
            );
            if !ctx.text_edit_focused() {
                search.request_focus();
            }
            if search.changed() {
                s.plugins.selected = 0;
            }
            let navigation = search.has_focus() || search.lost_focus() || !ctx.text_edit_focused();
            ui.horizontal(|ui| {
                ui.checkbox(&mut s.plugins.favorites_only, t("Favorites only"));
                ui.checkbox(&mut s.plugins.show_hidden, t("Show hidden"));
            });
            let q = s.plugins.search.trim().to_lowercase();
            let mut list: Vec<Action> = catalog::CATALOG
                .iter()
                .filter(|d| {
                    matches!(
                        d.kind,
                        EffectKind::VideoEffect
                            | EffectKind::AudioEffect
                            | EffectKind::VideoTransition
                            | EffectKind::AudioTransition
                    )
                })
                .map(|d| Action::Effect(d.id.into()))
                .collect();
            list.extend(
                op_core::presets::PRESETS
                    .iter()
                    .map(|d| Action::Effect(d.id.into())),
            );
            list.extend(
                op_application::commands::EDITOR_COMMANDS
                    .iter()
                    .map(|id| Action::Command((*id).into())),
            );
            list.retain(|a| {
                (q.is_empty() || name(a).to_lowercase().contains(&q) || id(a).contains(&q))
                    && (!s.plugins.favorites_only
                        || s.ed.prefs.plugins.favorites.iter().any(|f| f == id(a)))
                    && (s.plugins.show_hidden
                        || !s.ed.prefs.plugins.hidden.iter().any(|f| f == id(a)))
            });
            list.sort_by_key(|a| {
                (
                    !s.ed.prefs.plugins.favorites.iter().any(|f| f == id(a)),
                    name(a),
                )
            });
            if navigation && ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                s.plugins.selected = s.plugins.selected.saturating_add(1);
            }
            if navigation && ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                s.plugins.selected = s.plugins.selected.saturating_sub(1);
            }
            s.plugins.selected = s.plugins.selected.min(list.len().saturating_sub(1));
            if navigation
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && let Some(a) = list.get(s.plugins.selected)
            {
                let a = a.clone();
                run(s, &a);
            }
            egui::ScrollArea::vertical()
                .id_salt("actions-results")
                .max_height(260.0)
                .show(ui, |ui| {
                    for (i, a) in list.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let favorite = s.ed.prefs.plugins.favorites.iter().any(|f| f == id(a));
                            if ui.small_button(if favorite { "★" } else { "☆" }).clicked() {
                                toggle(&mut s.ed.prefs.plugins.favorites, id(a));
                            }
                            let resp = ui.selectable_label(s.plugins.selected == i, name(a));
                            if resp.clicked() {
                                s.plugins.selected = i;
                                run(s, a);
                            }
                            if s.plugins.selected == i
                                && ui.input(|i| {
                                    i.key_pressed(egui::Key::ArrowDown)
                                        || i.key_pressed(egui::Key::ArrowUp)
                                })
                            {
                                resp.scroll_to_me(Some(egui::Align::Center));
                            }
                            resp.on_hover_text(id(a)).context_menu(|ui| {
                                if ui.button(t("Hide / Show")).clicked() {
                                    toggle(&mut s.ed.prefs.plugins.hidden, id(a));
                                    ui.close();
                                }
                            });
                            if ui
                                .small_button("+")
                                .on_hover_text(t("Add to Macro"))
                                .clicked()
                            {
                                s.plugins.draft.push(a.clone());
                            }
                        });
                    }
                });
            ui.separator();
            ui.collapsing(t("Macro Builder"), |ui| {
                ui.label(t(
                    "Add actions with +. They run in order; each edit can be undone.",
                ));
                let mut remove = None;
                let mut swap = None;
                for (i, a) in s.plugins.draft.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("{}. {}", i + 1, name(a)));
                        if i > 0 && ui.small_button("↑").clicked() {
                            swap = Some((i, i - 1));
                        }
                        if ui.small_button("×").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some((a, b)) = swap {
                    s.plugins.draft.swap(a, b);
                }
                if let Some(i) = remove {
                    s.plugins.draft.remove(i);
                }
                ui.add(egui::TextEdit::singleline(&mut s.plugins.name).hint_text(t("Macro name")));
                ui.add(
                    egui::TextEdit::singleline(&mut s.plugins.shortcut)
                        .hint_text(t("Shortcut (optional, e.g. Ctrl+Alt+1)")),
                );
                let chord = Chord::parse(&s.plugins.shortcut);
                let conflict = chord.as_ref().is_some_and(|c| {
                    s.ed.keymap
                        .bindings
                        .iter()
                        .any(|b| Chord::parse(&b.keys).as_ref() == Some(c))
                        || s.ed.prefs.plugins.macros.iter().any(|m| {
                            m.name != s.plugins.name
                                && Chord::parse(&m.shortcut).as_ref() == Some(c)
                        })
                });
                let valid = s.plugins.shortcut.trim().is_empty() || chord.is_some();
                if conflict || !valid {
                    ui.colored_label(theme::WARN, t("Invalid shortcut or already assigned."));
                }
                if ui
                    .add_enabled(
                        !s.plugins.name.trim().is_empty()
                            && !s.plugins.draft.is_empty()
                            && valid
                            && !conflict,
                        egui::Button::new(t("Save Macro")),
                    )
                    .clicked()
                {
                    let m = ActionMacro {
                        name: s.plugins.name.trim().into(),
                        actions: s.plugins.draft.clone(),
                        shortcut: chord.map(|c| c.format()).unwrap_or_default(),
                    };
                    s.ed.prefs.plugins.macros.retain(|old| old.name != m.name);
                    s.ed.prefs.plugins.macros.push(m);
                    s.plugins.draft.clear();
                    s.plugins.name.clear();
                    s.plugins.shortcut.clear();
                }
            });
            ui.collapsing(t("Saved Macros"), |ui| {
                let mut remove = None;
                for (i, m) in s.ed.prefs.plugins.macros.clone().iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.button(RichText::new(&m.name)).clicked() {
                            for a in &m.actions {
                                run(s, a);
                            }
                        }
                        ui.small(&m.shortcut);
                        if ui.small_button(t("Edit")).clicked() {
                            s.plugins.name = m.name.clone();
                            s.plugins.shortcut = m.shortcut.clone();
                            s.plugins.draft = m.actions.clone();
                        }
                        if ui.small_button("×").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    s.ed.prefs.plugins.macros.remove(i);
                }
            });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        open = false;
    }
    s.plugins.actions_open = open;
}
