//! Undo history. Each step keeps the previous project revision; revisions share every entity that
//! did not change, so a step costs the size of what it touched.

use std::sync::Arc;
use std::time::Instant;

use crate::ids::{AssetId, ItemId, SequenceId};
use crate::model::Project;

/// What a committed step changed, derived by comparing revisions (semantic events for autosave,
/// scripting and the UI).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub items: Vec<ItemId>,
    pub assets: Vec<AssetId>,
    pub sequences: Vec<SequenceId>,
    pub settings: bool,
}

impl Changes {
    pub fn between(before: &Project, after: &Project) -> Changes {
        let mut c = Changes::default();
        for (id, v) in &after.items {
            if before
                .items
                .get(id)
                .is_none_or(|o| !Arc::ptr_eq(o, v) && o != v)
            {
                c.items.push(*id);
            }
        }
        for id in before.items.keys() {
            if !after.items.contains_key(id) {
                c.items.push(*id);
            }
        }
        for (id, v) in &after.assets {
            if before
                .assets
                .get(id)
                .is_none_or(|o| !Arc::ptr_eq(o, v) && o != v)
            {
                c.assets.push(*id);
            }
        }
        for id in before.assets.keys() {
            if !after.assets.contains_key(id) {
                c.assets.push(*id);
            }
        }
        for (id, v) in &after.sequences {
            if before
                .sequences
                .get(id)
                .is_none_or(|o| !Arc::ptr_eq(o, v) && o != v)
            {
                c.sequences.push(*id);
            }
        }
        for id in before.sequences.keys() {
            if !after.sequences.contains_key(id) {
                c.sequences.push(*id);
            }
        }
        c.settings = before.settings != after.settings || before.name != after.name;
        c
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
            && self.assets.is_empty()
            && self.sequences.is_empty()
            && !self.settings
    }
}

#[derive(Clone, Debug)]
pub struct Step {
    pub label: String,
    /// The project as it was before the step (for undo) or after it (for redo).
    pub project: Project,
    /// Identity of the document held in `project`, independent of the event revision.
    state: Arc<()>,
    merge_key: Option<String>,
    at: Instant,
    /// Clips this step holds that the step before it does not share (see `weight`).
    weight: usize,
}

/// Clip copies the undo steps may hold in all, about 220 MB; older steps are dropped beyond it.
const CLIP_BUDGET: usize = 1_000_000;
/// Steps kept even over the budget.
const MIN_STEPS: usize = 30;

/// How many clips `p` holds in tracks it does not share with `prev` (an edit copies only the
/// tracks it changes; the others stay shared between steps).
fn weight(p: &Project, prev: Option<&Project>) -> usize {
    let mut n = 1;
    for (id, seq) in &p.sequences {
        let old = prev.and_then(|q| q.sequences.get(id));
        if old.is_some_and(|o| Arc::ptr_eq(o, seq)) {
            continue;
        }
        for (i, tr) in seq.video.iter().chain(seq.audio.iter()).enumerate() {
            let shared = old.is_some_and(|o| {
                o.video
                    .iter()
                    .chain(o.audio.iter())
                    .nth(i)
                    .is_some_and(|x| Arc::ptr_eq(x, tr))
            });
            if !shared {
                n += tr.clips.len();
            }
        }
    }
    n
}

#[derive(Clone, Debug)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    limit: usize,
    /// Monotonic event counter for caches/recovery, including undo and redo.
    revision: u64,
    current_state: Arc<()>,
    saved_state: Arc<()>,
}

impl Default for History {
    fn default() -> Self {
        History::new(500)
    }
}

impl History {
    pub fn new(limit: usize) -> History {
        let state = Arc::new(());
        History {
            undo: Vec::new(),
            redo: Vec::new(),
            limit: limit.max(1),
            revision: 0,
            current_state: state.clone(),
            saved_state: state,
        }
    }

    /// Records that `current` replaced `before`. Consecutive steps with the same `merge_key` within
    /// a short time (dragging a value) collapse into one undo step.
    pub fn record(&mut self, label: impl Into<String>, before: Project, merge_key: Option<String>) {
        self.revision += 1;
        let before_state = std::mem::replace(&mut self.current_state, Arc::new(()));
        if !self.redo.is_empty() {
            self.seal();
        }
        self.redo.clear();
        let now = Instant::now();
        if let (Some(key), Some(last)) = (&merge_key, self.undo.last_mut())
            && last.merge_key.as_ref() == Some(key)
            && now.duration_since(last.at).as_secs_f32() < 1.5
        {
            last.at = now;
            return;
        }
        let weight = weight(&before, self.undo.last().map(|s| &s.project));
        self.undo.push(Step {
            label: label.into(),
            project: before,
            state: before_state,
            merge_key,
            at: now,
            weight,
        });
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
        // a timeline of many thousands of clips keeps fewer steps rather than all its memory
        let mut total: usize = self.undo.iter().map(|s| s.weight).sum();
        let mut drop = 0;
        while total > CLIP_BUDGET && self.undo.len() - drop > MIN_STEPS {
            total -= self.undo[drop].weight;
            drop += 1;
        }
        if drop > 0 {
            self.undo.drain(..drop);
        }
    }

    /// Ends a merge sequence so the next change starts a new step.
    pub fn seal(&mut self) {
        if let Some(last) = self.undo.last_mut() {
            last.merge_key = None;
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    /// Returns the revision to restore; `current` goes to the redo stack.
    pub fn undo(&mut self, current: &Project) -> Option<Project> {
        let step = self.undo.pop()?;
        self.revision += 1;
        let state = std::mem::replace(&mut self.current_state, step.state);
        self.seal();
        self.redo.push(Step {
            label: step.label.clone(),
            project: current.clone(),
            state,
            merge_key: None,
            at: step.at,
            weight: step.weight,
        });
        Some(step.project)
    }

    pub fn redo(&mut self, current: &Project) -> Option<Project> {
        let step = self.redo.pop()?;
        self.revision += 1;
        let state = std::mem::replace(&mut self.current_state, step.state);
        self.seal();
        self.undo.push(Step {
            label: step.label.clone(),
            project: current.clone(),
            state,
            merge_key: None,
            at: step.at,
            weight: step.weight,
        });
        Some(step.project)
    }

    /// Labels for the History panel, oldest first, and the index of the current state.
    pub fn labels(&self) -> (Vec<&str>, usize) {
        let mut v: Vec<&str> = self.undo.iter().map(|s| s.label.as_str()).collect();
        let current = v.len();
        v.extend(self.redo.iter().rev().map(|s| s.label.as_str()));
        (v, current)
    }

    /// Jumps to state `index` of `labels()` (0 = before the oldest step).
    pub fn jump(&mut self, index: usize, current: &Project) -> Option<Project> {
        let mut project = current.clone();
        let mut changed = false;
        while self.undo.len() > index {
            project = self.undo(&project)?;
            changed = true;
        }
        while self.undo.len() < index && !self.redo.is_empty() {
            project = self.redo(&project)?;
            changed = true;
        }
        changed.then_some(project)
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.revision += 1;
        self.current_state = Arc::new(());
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn mark_saved(&mut self) {
        // A later drag must not merge across the saved state and make it unreachable.
        self.seal();
        self.saved_state = self.current_state.clone();
    }

    /// Whether the current document state differs from the last save (or creation).
    /// Undo/redo can restore that identity while the event revision keeps increasing.
    pub fn is_dirty(&self) -> bool {
        !Arc::ptr_eq(&self.saved_state, &self.current_state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SequenceSettings;

    #[test]
    fn undo_redo_and_merge() {
        let mut h = History::default();
        let mut p = Project::new("a");
        let before = p.clone();
        p.name = "b".into();
        h.record("Rename", before, None);
        let before = p.clone();
        p.add_sequence(p.root, "S", SequenceSettings::default());
        h.record("New Sequence", before, None);
        assert_eq!(h.labels().0, vec!["Rename", "New Sequence"]);

        // merged drag steps
        for i in 0..5 {
            let before = p.clone();
            p.name = format!("drag{i}");
            h.record("Drag", before, Some("x".into()));
        }
        assert_eq!(h.labels().0.len(), 3);
        p = h.undo(&p).unwrap();
        assert_eq!(p.name, "b", "one undo reverts the whole drag");
        p = h.undo(&p).unwrap();
        assert!(p.sequences.is_empty());
        p = h.redo(&p).unwrap();
        assert_eq!(p.sequences.len(), 1);
        let p2 = h.jump(0, &p).unwrap();
        assert_eq!(p2.name, "a");
    }

    #[test]
    fn dirty_tracking() {
        let mut h = History::default();
        let mut p = Project::new("a");
        h.mark_saved();
        assert!(!h.is_dirty());
        let before = p.clone();
        p.name = "b".into();
        h.record("Rename", before, None);
        assert!(h.is_dirty());
        let changes = Changes::between(&Project::new("a"), &p);
        assert!(changes.settings);
    }

    fn rename(h: &mut History, p: &mut Project, name: &str, merge: Option<&str>) {
        let before = p.clone();
        let revision = h.revision();
        p.name = name.into();
        h.record("Rename", before, merge.map(str::to_string));
        assert_eq!(
            h.revision(),
            revision + 1,
            "each event advances the cache revision"
        );
    }

    #[test]
    fn undo_and_redo_restore_saved_identity_without_rewinding_revision() {
        let mut h = History::default();
        let mut p = Project::new("A");
        rename(&mut h, &mut p, "B", None);
        h.mark_saved();
        let saved = p.clone();
        rename(&mut h, &mut p, "C", None);
        assert!(h.is_dirty());
        let revision = h.revision();
        p = h.undo(&p).unwrap();
        assert_eq!(p, saved);
        assert!(!h.is_dirty());
        assert_eq!(h.revision(), revision + 1);
        p = h.undo(&p).unwrap();
        assert_eq!(p.name, "A");
        assert!(
            h.is_dirty(),
            "a state older than the saved one is dirty too"
        );
        p = h.redo(&p).unwrap();
        assert_eq!(p, saved);
        assert!(!h.is_dirty());
        p = h.redo(&p).unwrap();
        assert_eq!(p.name, "C");
        assert!(h.is_dirty());
        assert_eq!(h.revision(), revision + 4);
    }

    #[test]
    fn saving_seals_drag_to_preserve_saved_point_between_merged_gestures() {
        let mut h = History::default();
        let mut p = Project::new("A");
        rename(&mut h, &mut p, "B", Some("drag"));
        rename(&mut h, &mut p, "C", Some("drag"));
        assert_eq!(h.labels().1, 1);
        let revision = h.revision();
        h.mark_saved();
        h.mark_saved();
        assert_eq!(h.revision(), revision, "saving is not a document mutation");
        let saved = p.clone();
        rename(&mut h, &mut p, "D", Some("drag"));
        rename(&mut h, &mut p, "E", Some("drag"));
        assert_eq!(
            h.labels().1,
            2,
            "drag after save cannot merge across the saved state"
        );
        assert!(h.is_dirty());
        p = h.undo(&p).unwrap();
        assert_eq!(p, saved);
        assert!(!h.is_dirty());
        p = h.undo(&p).unwrap();
        assert_eq!(p.name, "A");
        assert!(h.is_dirty());
        p = h.redo(&p).unwrap();
        assert_eq!(p, saved);
        assert!(!h.is_dirty());
        p = h.redo(&p).unwrap();
        assert_eq!(p.name, "E");
        assert!(h.is_dirty());
    }

    #[test]
    fn saved_initial_state_survives_many_merged_events_with_one_step_limit() {
        let mut h = History::new(1);
        let mut p = Project::new("A");
        assert!(!h.is_dirty());
        for name in ["B", "C", "D", "E", "F"] {
            rename(&mut h, &mut p, name, Some("drag"));
        }
        assert_eq!(h.labels().1, 1);
        p = h.undo(&p).unwrap();
        assert_eq!(p.name, "A");
        assert!(
            !h.is_dirty(),
            "event count must not invalidate a saved state still reachable by undo"
        );
        p = h.redo(&p).unwrap();
        assert_eq!(p.name, "F");
        assert!(h.is_dirty());
    }

    #[test]
    fn branching_uses_new_state_identity_even_at_an_old_saved_depth() {
        let mut h = History::default();
        let mut p = Project::new("A");
        rename(&mut h, &mut p, "B", None);
        h.mark_saved();
        rename(&mut h, &mut p, "C", None);
        p = h.undo(&p).unwrap();
        assert!(!h.is_dirty());
        rename(&mut h, &mut p, "new C", None);
        assert!(h.is_dirty());
        assert!(!h.can_redo());
        p = h.undo(&p).unwrap();
        assert!(!h.is_dirty());
        p = h.undo(&p).unwrap();
        assert_eq!(p.name, "A");
        rename(&mut h, &mut p, "new B", None);
        assert_eq!(h.labels().1, 1, "same depth as the discarded saved state");
        assert!(
            h.is_dirty(),
            "a branch at the old saved index is a different state"
        );
        p = h.undo(&p).unwrap();
        assert!(h.is_dirty());
        p = h.redo(&p).unwrap();
        assert_eq!(p.name, "new B");
        assert!(h.is_dirty());
    }

    #[test]
    fn undo_and_branch_end_prior_drag_merges() {
        let mut h = History::default();
        let mut p = Project::new("A");
        rename(&mut h, &mut p, "B", Some("drag"));
        rename(&mut h, &mut p, "C", None);
        p = h.undo(&p).unwrap();
        rename(&mut h, &mut p, "D", Some("drag"));
        assert_eq!(
            h.labels().1,
            2,
            "branch cannot collapse into the earlier drag"
        );
        p = h.undo(&p).unwrap();
        assert_eq!(p.name, "B");
        p = h.undo(&p).unwrap();
        assert_eq!(p.name, "A");
        assert!(!h.is_dirty());
    }

    #[test]
    fn saved_state_eviction_cannot_make_other_retained_states_clean() {
        let mut h = History::new(1);
        let mut p = Project::new("A");
        rename(&mut h, &mut p, "B", None);
        h.mark_saved();
        rename(&mut h, &mut p, "C", None);
        rename(&mut h, &mut p, "D", None);
        assert_eq!(h.labels().1, 1);
        p = h.undo(&p).unwrap();
        assert_eq!(p.name, "C");
        assert!(h.is_dirty());
        let revision = h.revision();
        assert!(h.undo(&p).is_none());
        assert_eq!(h.revision(), revision);
        p = h.redo(&p).unwrap();
        assert_eq!(p.name, "D");
        assert!(h.is_dirty());
        h.mark_saved();
        assert!(
            !h.is_dirty(),
            "an evicted saved identity can be replaced explicitly"
        );
    }

    #[test]
    fn history_jump_tracks_saved_state_and_noop_preserves_revision() {
        let mut h = History::default();
        let mut p = Project::new("A");
        rename(&mut h, &mut p, "B", None);
        h.mark_saved();
        let saved = p.clone();
        rename(&mut h, &mut p, "C", None);
        rename(&mut h, &mut p, "D", None);
        let revision = h.revision();
        p = h.jump(1, &p).unwrap();
        assert_eq!(p, saved);
        assert!(!h.is_dirty());
        assert_eq!(h.revision(), revision + 2);
        let revision = h.revision();
        assert!(h.jump(1, &p).is_none());
        assert_eq!(h.revision(), revision);
        assert!(!h.is_dirty());
        p = h.jump(0, &p).unwrap();
        assert_eq!(p.name, "A");
        assert!(h.is_dirty());
        p = h.jump(1, &p).unwrap();
        assert_eq!(p, saved);
        assert!(!h.is_dirty());
        p = h.jump(3, &p).unwrap();
        assert_eq!(p.name, "D");
        assert!(h.is_dirty());
    }

    #[test]
    fn clear_invalidates_saved_identity_and_saving_noop_keeps_revision() {
        let mut h = History::new(0);
        let p = Project::new("A");
        assert!(!h.is_dirty());
        assert!(h.undo(&p).is_none());
        assert!(h.redo(&p).is_none());
        assert!(h.jump(0, &p).is_none());
        assert_eq!(h.revision(), 0);
        h.clear();
        assert_eq!(h.revision(), 1);
        assert!(h.is_dirty());
        assert!(!h.can_undo() && !h.can_redo());
        h.mark_saved();
        h.mark_saved();
        assert!(!h.is_dirty());
        assert_eq!(h.revision(), 1);
        let mut p = p;
        rename(&mut h, &mut p, "B", None);
        p = h.undo(&p).unwrap();
        assert_eq!(p.name, "A");
        assert!(!h.is_dirty());
        h.clear();
        assert!(h.is_dirty());
        assert_eq!(h.revision(), 4);
    }

    #[test]
    fn cloned_histories_share_saved_point_but_not_future_branch_identities() {
        let mut h = History::default();
        let mut p = Project::new("A");
        rename(&mut h, &mut p, "B", None);
        h.mark_saved();
        let mut cloned = h.clone();
        let mut branch = p.clone();
        rename(&mut cloned, &mut branch, "C", None);
        assert!(!h.is_dirty());
        assert!(cloned.is_dirty());
        assert_eq!(h.revision(), 1);
        assert_eq!(cloned.revision(), 2);
        branch = cloned.undo(&branch).unwrap();
        assert_eq!(branch, p);
        assert!(!cloned.is_dirty());
        rename(&mut h, &mut p, "D", None);
        assert!(h.is_dirty());
        assert!(!cloned.is_dirty());
        assert!(
            format!("{cloned:?}").contains("History"),
            "derived Debug remains available"
        );
    }
}
