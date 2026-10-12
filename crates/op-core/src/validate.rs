//! Invariants and transactions (DM-005, docs/data-model.md 8).
//!
//! A transaction edits a draft revision and commits only if every invariant holds and no locked
//! track changed; otherwise the previous revision is untouched.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use thiserror::Error;

use crate::ids::*;
use crate::model::*;
use crate::time::*;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum EditError {
    #[error("track {0} is locked")]
    Locked(String),
    #[error("nothing to do")]
    Nothing,
    #[error("clips would overlap on {0}")]
    Overlap(String),
    #[error("not enough media: {0}")]
    OutOfMedia(String),
    #[error("a sequence cannot contain itself")]
    Recursive,
    #[error("not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
}

pub type EditResult<T = ()> = Result<T, EditError>;

impl Project {
    /// Runs `f` on a draft and returns the committed revision, or the error with no change.
    pub fn transact<R>(
        &self,
        f: impl FnOnce(&mut Project) -> EditResult<R>,
    ) -> EditResult<(Project, R)> {
        let mut draft = self.clone();
        let r = f(&mut draft)?;
        draft.normalize();
        draft.validate()?;
        check_locks(self, &draft)?;
        Ok((draft, r))
    }

    /// Repairs derived state after an edit: sorting, removing dangling transitions and links,
    /// dropping empty clips.
    pub fn normalize(&mut self) {
        let ids: Vec<SequenceId> = self.sequences.keys().copied().collect();
        for sid in ids {
            let needs = {
                let seq = self.sequence(sid).unwrap();
                sequence_needs_normalizing(seq)
            };
            if needs {
                normalize_sequence(self.sequence_mut(sid).unwrap());
            }
        }
    }

    /// Checks every invariant.
    pub fn validate(&self) -> EditResult {
        validate_graph(self)?;
        for item in self.items.values() {
            if item.id != self.root {
                let parent = item.parent.and_then(|p| self.item(p));
                match parent {
                    Some(p) if p.is_bin() => {}
                    _ => return Err(EditError::Invalid(format!("{} has no bin", item.name))),
                }
            }
            match &item.kind {
                ItemKind::Media { asset, .. } if self.asset(*asset).is_none() => {
                    return Err(EditError::NotFound(format!("media of {}", item.name)));
                }
                ItemKind::Sequence { sequence } if self.sequence(*sequence).is_none() => {
                    return Err(EditError::NotFound(format!("sequence of {}", item.name)));
                }
                _ => {}
            }
        }
        for seq in self.sequences.values() {
            validate_sequence(self, seq)?;
        }
        validate_sequence_graph(self)?;
        Ok(())
    }
}

/// The bin tree has two stored directions. Both must describe the same rooted tree.
fn validate_graph(p: &Project) -> EditResult {
    if p.ids.exhausted() {
        return Err(EditError::Invalid(
            "project identity allocator is exhausted".into(),
        ));
    }
    let root = p
        .item(p.root)
        .ok_or_else(|| EditError::Invalid("project root is missing".into()))?;
    if !root.is_bin() || root.parent.is_some() {
        return Err(EditError::Invalid(
            "project root must be a bin without a parent".into(),
        ));
    }
    let mut owners = HashMap::new();
    for (key, item) in &p.items {
        valid_id(key.0)?;
        if *key != item.id {
            return Err(EditError::Invalid(format!(
                "item map key {key} differs from {}",
                item.id
            )));
        }
        for marker in &item.markers {
            valid_id(marker.id.0)?;
        }
        if let ItemKind::Bin { children } = &item.kind {
            for child in children {
                let entry = p
                    .item(*child)
                    .ok_or_else(|| EditError::NotFound(format!("bin {key} child {child}")))?;
                if *child == p.root || entry.parent != Some(*key) {
                    return Err(EditError::Invalid(format!(
                        "bin {key} child {child} has a different parent"
                    )));
                }
                if owners.insert(*child, *key).is_some() {
                    return Err(EditError::Invalid(format!(
                        "item {child} is listed more than once"
                    )));
                }
            }
        }
    }
    for key in p.items.keys().filter(|key| **key != p.root) {
        if !owners.contains_key(key) {
            return Err(EditError::Invalid(format!(
                "item {key} is missing from its parent bin"
            )));
        }
    }
    // An internally consistent disconnected cycle still has reciprocal parent/child links.
    let mut reached = HashSet::new();
    let mut stack = vec![p.root];
    while let Some(id) = stack.pop() {
        if reached.insert(id) {
            stack.extend(p.children(id));
        }
    }
    if reached.len() != p.items.len() {
        return Err(EditError::Invalid(
            "bin hierarchy contains a disconnected cycle".into(),
        ));
    }
    for (key, asset) in &p.assets {
        valid_id(key.0)?;
        if *key != asset.id {
            return Err(EditError::Invalid(format!(
                "asset map key {key} differs from {}",
                asset.id
            )));
        }
    }
    for (key, seq) in &p.sequences {
        valid_id(key.0)?;
        if *key != seq.id {
            return Err(EditError::Invalid(format!(
                "sequence map key {key} differs from {}",
                seq.id
            )));
        }
        for marker in &seq.markers {
            valid_id(marker.id.0)?;
        }
        for component in &seq.master {
            valid_id(component.id.0)?;
        }
        for (_, track) in seq.all_tracks() {
            valid_id(track.id.0)?;
            for component in &track.components {
                valid_id(component.id.0)?;
            }
            for transition in &track.transitions {
                valid_id(transition.id.0)?;
            }
            for clip in &track.clips {
                valid_id(clip.id.0)?;
                if let Some(link) = clip.link {
                    valid_id(link.0)?;
                }
                if let Some(group) = clip.group {
                    valid_id(group.0)?;
                }
                for component in &clip.components {
                    valid_id(component.id.0)?;
                }
            }
        }
    }
    Ok(())
}

fn valid_id(id: u64) -> EditResult {
    if id == u64::MAX {
        Err(EditError::Invalid(
            "stored identity exhausts the project allocator".into(),
        ))
    } else {
        Ok(())
    }
}

/// Detect nested-sequence cycles in O(sequences + references), without recursive traversal.
fn validate_sequence_graph(p: &Project) -> EditResult {
    let mut incoming: HashMap<_, usize> = p.sequences.keys().map(|id| (*id, 0)).collect();
    for seq in p.sequences.values() {
        for nested in seq.nested() {
            let count = incoming
                .get_mut(&nested)
                .ok_or_else(|| EditError::NotFound(format!("nested sequence {nested}")))?;
            *count += 1;
        }
    }
    let mut ready: Vec<_> = incoming
        .iter()
        .filter_map(|(id, n)| (*n == 0).then_some(*id))
        .collect();
    let mut removed = 0;
    while let Some(id) = ready.pop() {
        removed += 1;
        if let Some(seq) = p.sequence(id) {
            for nested in seq.nested() {
                let count = incoming.get_mut(&nested).ok_or(EditError::Recursive)?;
                *count -= 1;
                if *count == 0 {
                    ready.push(nested);
                }
            }
        }
    }
    if removed != p.sequences.len() {
        return Err(EditError::Recursive);
    }
    Ok(())
}

fn sequence_needs_normalizing(seq: &Sequence) -> bool {
    for (_, t) in seq.all_tracks() {
        if t.clips
            .windows(2)
            .any(|w| (w[0].start, w[0].id) > (w[1].start, w[1].id))
        {
            return true;
        }
        if t.clips.iter().any(|c| c.duration.0 <= 0) {
            return true;
        }
        for tr in &t.transitions {
            if !transition_attached(t, tr) {
                return true;
            }
        }
    }
    let mut links: HashMap<LinkId, usize> = HashMap::new();
    let mut groups: HashMap<GroupId, usize> = HashMap::new();
    for (_, c) in seq.clips() {
        if let Some(l) = c.link {
            *links.entry(l).or_default() += 1;
        }
        if let Some(g) = c.group {
            *groups.entry(g).or_default() += 1;
        }
    }
    links.values().any(|n| *n < 2) || groups.values().any(|n| *n < 2)
}

/// Whether a transition still sits on an edit point of its clips.
fn transition_attached(t: &Track, tr: &Transition) -> bool {
    if tr.duration.0 <= 0 {
        return false;
    }
    let from = tr.from.map(|id| t.clip(id));
    let to = tr.to.map(|id| t.clip(id));
    match (from, to) {
        (None, None) => false,
        (Some(None), _) | (_, Some(None)) => false,
        (Some(Some(a)), Some(Some(b))) => {
            a.end() == tr.cut
                && b.start == tr.cut
                && tr.before_cut() <= a.duration
                && tr.duration - tr.before_cut() <= b.duration
        }
        (Some(Some(a)), None) => a.end() == tr.cut && tr.range().start >= a.start,
        (None, Some(Some(b))) => b.start == tr.cut && tr.range().end <= b.end(),
    }
}

fn normalize_sequence(seq: &mut Sequence) {
    for kind in [TrackKind::Video, TrackKind::Audio] {
        for t in seq.tracks_mut(kind) {
            let t = Arc::make_mut(t);
            t.clips.retain(|c| c.duration.0 > 0);
            t.sort();
            // a transition whose neighbor disappeared becomes one-sided when the other clip still
            // touches the cut; otherwise it goes
            let clips = t.clips.clone();
            let find = |id: Option<ClipId>| id.and_then(|i| clips.iter().find(|c| c.id == i));
            t.transitions.retain_mut(|tr| {
                let a = find(tr.from).filter(|a| a.end() == tr.cut);
                let b = find(tr.to).filter(|b| b.start == tr.cut);
                tr.from = a.map(|c| c.id);
                tr.to = b.map(|c| c.id);
                match (a, b) {
                    (None, None) => false,
                    (Some(a), None) => {
                        tr.alignment = Alignment::EndAtCut;
                        tr.duration = tr.duration.min(a.duration);
                        tr.duration.0 > 0
                    }
                    (None, Some(b)) => {
                        tr.alignment = Alignment::StartAtCut;
                        tr.duration = tr.duration.min(b.duration);
                        tr.duration.0 > 0
                    }
                    (Some(a), Some(b)) => {
                        // keep inside both clips
                        let before = tr.before_cut().min(a.duration);
                        let after = (tr.duration - tr.before_cut()).min(b.duration);
                        if before + after != tr.duration {
                            tr.duration = before + after;
                            tr.alignment = Alignment::Custom(before);
                        }
                        tr.duration.0 > 0
                    }
                }
            });
        }
    }
    // links and groups with a single member dissolve
    let mut links: HashMap<LinkId, usize> = HashMap::new();
    let mut groups: HashMap<GroupId, usize> = HashMap::new();
    for (_, c) in seq.clips() {
        if let Some(l) = c.link {
            *links.entry(l).or_default() += 1;
        }
        if let Some(g) = c.group {
            *groups.entry(g).or_default() += 1;
        }
    }
    let lonely_links: HashSet<LinkId> = links
        .into_iter()
        .filter(|(_, n)| *n < 2)
        .map(|(l, _)| l)
        .collect();
    let lonely_groups: HashSet<GroupId> = groups
        .into_iter()
        .filter(|(_, n)| *n < 2)
        .map(|(g, _)| g)
        .collect();
    if lonely_links.is_empty() && lonely_groups.is_empty() {
        return;
    }
    for kind in [TrackKind::Video, TrackKind::Audio] {
        for t in seq.tracks_mut(kind) {
            let touches = t.clips.iter().any(|c| {
                c.link.is_some_and(|l| lonely_links.contains(&l))
                    || c.group.is_some_and(|g| lonely_groups.contains(&g))
            });
            if touches {
                for c in &mut Arc::make_mut(t).clips {
                    if c.link.is_some_and(|l| lonely_links.contains(&l)) {
                        c.link = None;
                    }
                    if c.group.is_some_and(|g| lonely_groups.contains(&g)) {
                        c.group = None;
                    }
                }
            }
        }
    }
}

fn validate_sequence(p: &Project, seq: &Sequence) -> EditResult {
    let mut seen = HashSet::new();
    for (r, t) in seq.all_tracks() {
        let name = seq.track_name(r);
        for c in &t.clips {
            if !seen.insert(c.id) {
                return Err(EditError::Invalid(format!("clip {} appears twice", c.id)));
            }
            if c.kind != t.kind {
                return Err(EditError::Invalid(format!(
                    "{} clip on {name}",
                    if c.is_video() { "video" } else { "audio" }
                )));
            }
            if c.duration.0 <= 0 {
                return Err(EditError::Invalid("empty clip".into()));
            }
            if c.start < SeqTime::ZERO {
                return Err(EditError::Invalid("clip before the sequence start".into()));
            }
            // Deserialization and direct model edits bypass Speed::new. Reject invalid ratios
            // for every source kind, including holds, before any source-time arithmetic.
            if c.speed.num <= 0 || c.speed.den <= 0 {
                return Err(EditError::Invalid(format!(
                    "clip {} speed must be positive",
                    c.id
                )));
            }
            // An absent source is not an unbounded source. In particular, stills and graphics
            // legitimately return no available range, while a dangling identity must fail.
            match &c.source {
                ClipSource::Asset { asset, .. } if p.asset(*asset).is_none() => {
                    return Err(EditError::NotFound(format!("clip {} asset {asset}", c.id)));
                }
                ClipSource::Sequence { sequence, .. } if p.sequence(*sequence).is_none() => {
                    return Err(EditError::NotFound(format!(
                        "clip {} sequence {sequence}",
                        c.id
                    )));
                }
                ClipSource::Generator { item } => match p.item(*item) {
                    None => {
                        return Err(EditError::NotFound(format!(
                            "clip {} generator {item}",
                            c.id
                        )));
                    }
                    Some(item) if !matches!(item.kind, ItemKind::Synthetic { .. }) => {
                        return Err(EditError::Invalid(format!(
                            "clip {} generator is not a synthetic item",
                            c.id
                        )));
                    }
                    _ => {}
                },
                _ => {}
            }
            if let Some(avail) = p.available(&c.source)
                && c.hold.is_none()
            {
                let used = c.source_range();
                // one tick of tolerance absorbs rounding from speed changes
                if used.start < avail.start - Dur(1) || used.end > avail.end + Dur(1) {
                    return Err(EditError::OutOfMedia(c.name.clone()));
                }
            }
        }
        for w in t.clips.windows(2) {
            if w[0].end() > w[1].start {
                return Err(EditError::Overlap(name));
            }
        }
    }
    Ok(())
}

/// Refuses revisions that change the contents of a track locked in both revisions.
fn check_locks(before: &Project, after: &Project) -> EditResult {
    for (sid, seq) in &after.sequences {
        let Some(old) = before.sequences.get(sid) else {
            continue;
        };
        if Arc::ptr_eq(old, seq) {
            continue;
        }
        for kind in [TrackKind::Video, TrackKind::Audio] {
            for (i, t) in seq.tracks(kind).iter().enumerate() {
                let Some(o) = old.tracks(kind).iter().find(|o| o.id == t.id) else {
                    continue;
                };
                if Arc::ptr_eq(o, t) || !(o.locked && t.locked) {
                    continue;
                }
                if o.clips != t.clips || o.transitions != t.transitions {
                    return Err(EditError::Locked(
                        seq.track_name(TrackRef { kind, index: i }),
                    ));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::EffectKind;
    use crate::color::Label;

    #[test]
    fn malformed_bin_graphs_never_commit() {
        let mut p = Project::new("bins");
        let bin = p.add_bin(p.root, "bin");
        let leaf = p.add_bin(bin, "leaf");
        p.validate().unwrap();
        let before = p.clone();
        for case in 0..11 {
            let result = p.transact(|d| {
                let root = d.root;
                match case {
                    0 => d.root = ItemId(999),
                    1 => {
                        d.item_mut(root).unwrap().kind = ItemKind::Synthetic {
                            generator: Generator::BlackVideo,
                            duration: Dur::ZERO,
                        }
                    }
                    2 => d.item_mut(root).unwrap().parent = Some(bin),
                    3 => d.item_mut(leaf).unwrap().parent = None,
                    4 => d.item_mut(leaf).unwrap().parent = Some(leaf),
                    5 | 6 | 7 | 10 => {
                        let target = if case == 7 { root } else { bin };
                        let child = match case {
                            5 => ItemId(999),
                            10 => root,
                            _ => leaf,
                        };
                        let ItemKind::Bin { children } = &mut d.item_mut(target).unwrap().kind
                        else {
                            unreachable!()
                        };
                        children.push(child);
                    }
                    8 => d.item_mut(bin).unwrap().kind = ItemKind::Bin { children: vec![] },
                    9 => {
                        // A disconnected self-cycle with perfectly reciprocal links.
                        d.item_mut(root).unwrap().kind = ItemKind::Bin { children: vec![] };
                        d.item_mut(bin).unwrap().parent = Some(bin);
                        d.item_mut(bin).unwrap().kind = ItemKind::Bin {
                            children: vec![bin, leaf],
                        };
                    }
                    _ => unreachable!(),
                }
                Ok(())
            });
            assert!(
                matches!(result, Err(EditError::Invalid(_) | EditError::NotFound(_))),
                "case {case}: {result:?}"
            );
            assert_eq!(p, before, "case {case} changed the original");
        }
    }

    #[test]
    fn item_asset_and_sequence_map_keys_must_match_stored_identities() {
        let mut p = Project::new("maps");
        let (sid, _) = p.add_sequence(p.root, "seq", SequenceSettings::default());
        let mut bad = p.clone();
        bad.item_mut(bad.root).unwrap().id = ItemId(999);
        assert!(matches!(bad.validate(), Err(EditError::Invalid(_))));
        let mut bad = p.clone();
        bad.sequence_mut(sid).unwrap().id = SequenceId(999);
        assert!(matches!(bad.validate(), Err(EditError::Invalid(_))));
        let asset = crate::media::MediaAsset {
            id: AssetId(0),
            path: "offline.png".into(),
            proxy: None,
            kind: crate::media::MediaKind::Still,
            video: None,
            audio: vec![],
            duration: Dur::ZERO,
            interpretation: crate::media::Interpretation::default(),
            file_size: 0,
            modified_unix: 0,
        };
        let (aid, _) = p.add_asset(p.root, asset);
        Arc::make_mut(p.assets.get_mut(&aid).unwrap()).id = AssetId(999);
        assert!(matches!(p.validate(), Err(EditError::Invalid(_))));
    }

    #[test]
    fn sequence_graph_accepts_shared_and_repeated_edges_but_rejects_cycles() {
        let mut p = Project::new("sequence graph");
        let (a, _) = p.add_sequence(p.root, "a", SequenceSettings::default());
        let (b, _) = p.add_sequence(p.root, "b", SequenceSettings::default());
        let (c, _) = p.add_sequence(p.root, "c", SequenceSettings::default());
        let mut add = |outer, inner, start| {
            let mut cl = clip(&mut p.ids, start, 1.0);
            cl.source = ClipSource::Sequence {
                sequence: inner,
                item: None,
            };
            cl.hold = Some(SrcTime::ZERO);
            p.sequence_mut(outer)
                .unwrap()
                .track_mut(TrackRef::video(0))
                .unwrap()
                .clips
                .push(cl);
        };
        add(a, b, 0.0);
        add(a, b, 1.0);
        add(a, c, 2.0);
        add(b, c, 0.0);
        p.validate().unwrap();
        for target in [a, b, c] {
            let result = p.transact(|d| {
                let mut cl = clip(&mut d.ids, 0.0, 1.0);
                cl.source = ClipSource::Sequence {
                    sequence: target,
                    item: None,
                };
                cl.hold = Some(SrcTime::ZERO);
                d.sequence_mut(c)
                    .unwrap()
                    .track_mut(TrackRef::video(0))
                    .unwrap()
                    .clips
                    .push(cl);
                Ok(())
            });
            assert!(matches!(result, Err(EditError::Recursive)), "{result:?}");
        }
    }

    #[test]
    fn refresh_of_exhausted_identity_is_bounded_and_validation_rejects_it() {
        let mut p = Project::new("ids");
        p.item_mut(p.root)
            .unwrap()
            .markers
            .push(Marker::new(MarkerId(u64::MAX), SrcTime::ZERO));
        assert!(p.validate().is_err());
        p.refresh_ids();
        assert!(p.validate().is_err());
    }

    #[test]
    fn an_allocator_near_the_limit_rejects_multi_identity_edits_without_overflow() {
        let mut p = Project::new("near identity limit");
        p.ids.observe(u64::MAX - 2);
        p.validate().unwrap();
        let before = p.clone();
        let result = p.transact(|d| {
            d.add_sequence(d.root, "needs several IDs", SequenceSettings::default());
            Ok(())
        });
        assert!(matches!(result, Err(EditError::Invalid(_))));
        assert_eq!(p, before);
    }

    pub fn clip(ids: &mut IdGen, start: f64, dur: f64) -> Clip {
        Clip {
            id: ids.clip(),
            name: "c".into(),
            kind: TrackKind::Video,
            source: ClipSource::Graphic,
            start: SeqTime::from_seconds(start),
            duration: Dur::from_seconds(dur),
            source_in: SrcTime::ZERO,
            speed: Speed::NORMAL,
            reverse: false,
            hold: None,
            enabled: true,
            link: None,
            group: None,
            label: Label::None,
            components: default_components(EffectKind::VideoFixed, ids).into(),
            gain_db: 0.0,
            scale_to_frame: false,
            channels: None,
        }
    }

    #[test]
    fn overlaps_are_rejected_and_nothing_changes() {
        let mut p = Project::new("t");
        let (sid, _) = p.add_sequence(p.root, "S", SequenceSettings::default());
        let r = p.transact(|d| {
            let mut ids = d.ids.clone();
            let a = clip(&mut ids, 0.0, 5.0);
            let b = clip(&mut ids, 4.0, 5.0);
            d.ids = ids;
            let t = d
                .sequence_mut(sid)
                .unwrap()
                .track_mut(TrackRef::video(0))
                .unwrap();
            t.clips.push(a);
            t.clips.push(b);
            Ok(())
        });
        assert!(matches!(r, Err(EditError::Overlap(_))));
        assert!(p.sequence(sid).unwrap().video[0].clips.is_empty());
    }

    #[test]
    fn invalid_speeds_are_rejected_without_committing() {
        let mut p = Project::new("t");
        let (sid, _) = p.add_sequence(p.root, "S", SequenceSettings::default());
        let mut ids = p.ids.clone();
        let c = clip(&mut ids, 0.0, 1.0);
        let cid = c.id;
        p.ids = ids;
        p.sequence_mut(sid)
            .unwrap()
            .track_mut(TrackRef::video(0))
            .unwrap()
            .clips
            .push(c);
        let before = p.clone();
        for (num, den) in [(0, 1), (1, 0), (-1, 1), (1, -1)] {
            for source in [
                ClipSource::Graphic,
                ClipSource::Asset {
                    asset: AssetId(999),
                    item: None,
                    stream: 0,
                },
                ClipSource::Sequence {
                    sequence: sid,
                    item: None,
                },
                ClipSource::Generator { item: p.root },
            ] {
                for hold in [None, Some(SrcTime::ZERO)] {
                    let result = p.transact(|draft| {
                        let c = draft.sequence_mut(sid).unwrap().clip_mut(cid).unwrap();
                        c.speed = Speed { num, den };
                        c.source = source.clone();
                        c.hold = hold;
                        Ok(())
                    });
                    assert_eq!(
                        result.unwrap_err(),
                        EditError::Invalid(format!("clip {cid} speed must be positive"))
                    );
                    assert_eq!(p, before);
                }
            }
        }
    }

    #[test]
    fn dangling_clip_sources_are_rejected_without_committing() {
        let mut p = Project::new("t");
        let (sid, _) = p.add_sequence(p.root, "S", SequenceSettings::default());
        let c = clip(&mut p.ids, 0.0, 1.0);
        let cid = c.id;
        p.sequence_mut(sid)
            .unwrap()
            .track_mut(TrackRef::video(0))
            .unwrap()
            .clips
            .push(c);
        let before = p.clone();
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
            let result = p.transact(|draft| {
                draft
                    .sequence_mut(sid)
                    .unwrap()
                    .clip_mut(cid)
                    .unwrap()
                    .source = source;
                Ok(())
            });
            assert!(matches!(
                result,
                Err(EditError::NotFound(_) | EditError::Invalid(_))
            ));
            assert_eq!(p, before);
        }
    }

    #[test]
    fn unbounded_sources_and_optional_source_items_remain_valid() {
        let mut p = Project::new("t");
        let (sid, _) = p.add_sequence(p.root, "S", SequenceSettings::default());
        let (asset, item) = p.add_asset(
            p.root,
            crate::media::MediaAsset {
                id: AssetId(0),
                path: "offline-still.png".into(),
                proxy: None,
                kind: crate::media::MediaKind::Still,
                video: None,
                audio: vec![],
                duration: Dur::ZERO,
                interpretation: crate::media::Interpretation::default(),
                file_size: 0,
                modified_unix: 0,
            },
        );
        let generator = p.add_item(
            p.root,
            "Black",
            ItemKind::Synthetic {
                generator: Generator::BlackVideo,
                duration: Dur::from_seconds(1.0),
            },
        );
        let c = clip(&mut p.ids, 0.0, 60.0);
        let cid = c.id;
        p.sequence_mut(sid)
            .unwrap()
            .track_mut(TrackRef::video(0))
            .unwrap()
            .clips
            .push(c);
        for source in [
            ClipSource::Graphic,
            ClipSource::Generator { item: generator },
            ClipSource::Asset {
                asset,
                item: Some(item),
                stream: 0,
            },
            // A deleted origin item does not remove an asset referenced by another item.
            ClipSource::Asset {
                asset,
                item: Some(ItemId(999)),
                stream: 0,
            },
        ] {
            assert!(p.available(&source).is_none());
            assert!(
                p.transact(|draft| {
                    draft
                        .sequence_mut(sid)
                        .unwrap()
                        .clip_mut(cid)
                        .unwrap()
                        .source = source;
                    Ok(())
                })
                .is_ok()
            );
        }
    }

    #[test]
    fn locked_tracks_refuse_edits() {
        let mut p = Project::new("t");
        let (sid, _) = p.add_sequence(p.root, "S", SequenceSettings::default());
        p.sequence_mut(sid)
            .unwrap()
            .track_mut(TrackRef::video(0))
            .unwrap()
            .locked = true;
        let r = p.transact(|d| {
            let mut ids = d.ids.clone();
            let c = clip(&mut ids, 0.0, 1.0);
            d.sequence_mut(sid)
                .unwrap()
                .track_mut(TrackRef::video(0))
                .unwrap()
                .clips
                .push(c);
            Ok(())
        });
        assert!(matches!(r, Err(EditError::Locked(_))));
    }

    #[test]
    fn dangling_transitions_and_single_links_are_cleaned() {
        let mut p = Project::new("t");
        let (sid, _) = p.add_sequence(p.root, "S", SequenceSettings::default());
        let (next, _) = p
            .transact(|d| {
                let mut ids = d.ids.clone();
                let mut a = clip(&mut ids, 0.0, 5.0);
                let b = clip(&mut ids, 5.0, 5.0);
                a.link = Some(ids.link());
                let tr = Transition {
                    id: ids.transition(),
                    effect: crate::catalog::CROSS_DISSOLVE.into(),
                    cut: SeqTime::from_seconds(5.0),
                    duration: Dur::from_seconds(1.0),
                    alignment: Alignment::CenterAtCut,
                    from: Some(a.id),
                    to: Some(b.id),
                    params: vec![],
                };
                d.ids = ids;
                let t = d
                    .sequence_mut(sid)
                    .unwrap()
                    .track_mut(TrackRef::video(0))
                    .unwrap();
                t.clips = vec![a, b.clone()];
                t.transitions.push(tr);
                // remove the incoming clip: the transition becomes one-sided on `a`
                t.clips.retain(|c| c.id != b.id);
                Ok(())
            })
            .unwrap();
        let t = &next.sequence(sid).unwrap().video[0];
        assert_eq!(t.transitions.len(), 1);
        assert_eq!(t.transitions[0].alignment, Alignment::EndAtCut);
        assert!(t.clips[0].link.is_none());
    }
}
