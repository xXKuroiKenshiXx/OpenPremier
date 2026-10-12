//! Caption editing policy for the Captions panel (DM-TIME-001, DM-TL-001, DM-FX-004).
//! Reject unsupported edits before changing the document; commit each operation as one step.

use op_core::captions::{self, TimedWord};
use op_core::{catalog, *};

use crate::{Editor, Selection};

fn invalid(message: &str) -> EditError {
    EditError::Invalid(message.into())
}

fn caption_words(clip: &Clip) -> EditResult<Vec<TimedWord>> {
    if clip.source != ClipSource::Graphic
        || clip.speed != Speed::NORMAL
        || clip.reverse
        || clip.hold.is_some()
        || clip.link.is_some()
        || clip.group.is_some()
    {
        return Err(invalid(
            "Caption editing requires an unlinked graphic at normal speed",
        ));
    }
    if clip
        .components
        .iter()
        .any(|c| c.foreign.is_some() || c.def().is_none())
    {
        return Err(invalid(
            "Caption editing cannot reinterpret foreign or unknown components",
        ));
    }
    let comp = clip
        .component(catalog::CAPTION)
        .ok_or_else(|| invalid("Select captions only"))?;
    let text = match comp.param("text") {
        Some(p) if !p.animated && p.keys.is_empty() => match &p.value {
            Value::Text(text) => text.as_str(),
            _ => return Err(invalid("The caption text is not supported")),
        },
        _ => return Err(invalid("Animated caption text is not supported")),
    };
    let timing = match comp.param("timing") {
        Some(p) if !p.animated && p.keys.is_empty() => match &p.value {
            Value::Text(timing) => timing.as_str(),
            _ => return Err(invalid("The caption timing is not supported")),
        },
        _ => return Err(invalid("Animated caption timing is not supported")),
    };
    let words = captions::words(text, timing, clip.duration.seconds());
    if words.is_empty()
        || words.iter().any(|w| {
            !w.start.is_finite()
                || !w.end.is_finite()
                || w.start < 0.0
                || w.end < w.start
                || w.end > clip.duration.seconds() + 1e-9
        })
        || words.windows(2).any(|w| w[0].end > w[1].start)
    {
        return Err(invalid(
            "The caption has empty, overlapping or invalid word timing",
        ));
    }
    Ok(words)
}

fn set_words(clip: &mut Clip, words: &[TimedWord]) {
    let text = words
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    clip.name = text.chars().take(40).collect();
    let comp = clip
        .component_mut(catalog::CAPTION)
        .expect("validated caption");
    comp.param_mut("text").expect("validated text").value = Value::Text(text);
    // Use shortest round-trip representations; three-decimal encoding would shift words when
    // splitting at a frame boundary such as 1001/30000 seconds.
    comp.param_mut("timing").expect("validated timing").value = Value::Text(
        words
            .iter()
            .map(|w| format!("{}-{}", w.start, w.end))
            .collect::<Vec<_>>()
            .join(" "),
    );
}

fn transitions_touch(track: &Track, clips: &[ClipId]) -> bool {
    track.transitions.iter().any(|t| {
        t.from.is_some_and(|id| clips.contains(&id)) || t.to.is_some_and(|id| clips.contains(&id))
    })
}

fn merge_compatible(first: &Clip, next: &Clip) -> bool {
    // A merged component cannot represent independent animation/foreign payloads from both
    // originals. Static, identical component stacks are safe; keep the first component IDs.
    if first.components.iter().chain(&next.components).any(|c| {
        c.foreign.is_some()
            || c.def().is_none()
            || c.params.iter().any(|p| p.animated || !p.keys.is_empty())
    }) || first.components.len() != next.components.len()
    {
        return false;
    }
    let mut normalized = next.clone();
    normalized.id = first.id;
    normalized.name = first.name.clone();
    normalized.start = first.start;
    normalized.duration = first.duration;
    normalized.source_in = first.source_in;
    for (a, b) in first.components.iter().zip(&mut normalized.components) {
        b.id = a.id;
        if a.effect == catalog::CAPTION && b.effect == catalog::CAPTION {
            for key in ["text", "timing"] {
                if let (Some(src), Some(dst)) = (a.param(key), b.param_mut(key)) {
                    *dst = src.clone();
                }
            }
        }
    }
    *first == normalized
}

impl Editor {
    /// Edit visible caption boundaries without moving its spoken words. Times are sequence
    /// positions, rounded to frames. Changing the start offsets relative word timing and
    /// source_in together; no spoken word may be cropped and no neighboring clip is moved.
    pub fn set_caption_bounds(&mut self, id: ClipId, start: SeqTime, end: SeqTime) -> bool {
        let result = self.seq_edit("Caption Times", move |p, sid, _| {
            let seq = p
                .sequence(sid)
                .ok_or_else(|| invalid("Open a sequence first"))?;
            if start < SeqTime::ZERO
                || end < SeqTime::ZERO
                || start > SeqTime::MAX
                || end > SeqTime::MAX
            {
                return Err(invalid(
                    "Caption times must be nonnegative supported sequence positions",
                ));
            }
            let start = start.round_frame(seq.rate());
            let end = end.round_frame(seq.rate());
            if end <= start {
                return Err(invalid("Caption end must be after its start"));
            }
            let (r, original) = seq
                .find_clip(id)
                .ok_or_else(|| invalid("Select a caption first"))?;
            let track = seq.track(r).expect("existing track");
            if track.locked {
                return Err(EditError::Locked(track.name.clone()));
            }
            if transitions_touch(track, &[id]) {
                return Err(invalid("Remove caption transitions before editing times"));
            }
            let mut words = caption_words(original)?;
            if start == original.start && end == original.end() {
                return Err(EditError::Nothing);
            }
            let shift = start - original.start;
            let duration = end - start;
            for word in &mut words {
                word.start -= shift.seconds();
                word.end -= shift.seconds();
                if word.start < -1e-9 || word.end > duration.seconds() + 1e-9 {
                    return Err(invalid("Caption boundaries cannot cut off spoken words"));
                }
                word.start = word.start.max(0.0);
                word.end = word.end.min(duration.seconds());
            }
            let range = SeqRange::new(start, end);
            if track
                .clips
                .iter()
                .any(|c| c.id != id && c.range().overlaps(&range))
            {
                return Err(EditError::Overlap(track.name.clone()));
            }
            let mut edited = original.clone();
            let source_in = original.source_in + shift;
            if source_in < SrcTime::ZERO {
                if original
                    .components
                    .iter()
                    .any(|c| c.params.iter().any(|p| p.animated || !p.keys.is_empty()))
                {
                    return Err(invalid(
                        "Extending this animated caption would require negative source time",
                    ));
                }
                // Static Graphic components have no source content to reveal. Retain their
                // original source offset rather than introducing an invalid negative offset.
            } else {
                edited.source_in = source_in;
            }
            edited.start = start;
            edited.duration = duration;
            // Preserve original text/whitespace/name. Only the hidden timing representation
            // changes when needed to keep the words at their original absolute sequence times.
            set_words(&mut edited, &words);
            edited.name = original.name.clone();
            edited
                .component_mut(catalog::CAPTION)
                .expect("validated caption")
                .param_mut("text")
                .expect("validated text")
                .value = original
                .component(catalog::CAPTION)
                .expect("validated caption")
                .param("text")
                .expect("validated text")
                .value
                .clone();
            *p.sequence_mut(sid)
                .expect("existing sequence")
                .clip_mut(id)
                .expect("existing caption") = edited;
            Ok(())
        });
        result.is_some()
    }

    /// Split an explicit caption without replacing the caller's selection on failure.
    /// The requested cut does not move the caller's playhead.
    pub fn split_caption(&mut self, id: ClipId, at: SeqTime) -> bool {
        let Some(seq) = self.active_seq() else {
            self.error("Open a sequence first");
            return false;
        };
        if at < SeqTime::ZERO || at > SeqTime::MAX {
            self.error("Caption cut must be a nonnegative supported sequence position");
            return false;
        }
        let at = at.round_frame(seq.rate());
        let selection = self.selection.clone();
        let playhead = self.playhead();
        self.selection = Selection::only(vec![id]);
        self.set_playhead(at);
        let result = self.split_caption_at_playhead();
        self.set_playhead(playhead);
        if !result {
            self.selection = selection;
        }
        result
    }

    /// Merge explicit caption IDs with the same safety policy as the Captions panel.
    pub fn merge_captions(&mut self, ids: &[ClipId]) -> bool {
        let selection = self.selection.clone();
        self.selection = Selection::only(ids.to_vec());
        let result = self.merge_selected_captions();
        if !result {
            self.selection = selection;
        }
        result
    }

    /// Split one selected caption at the nearest playhead frame, without cutting a spoken word.
    /// Word gaps are valid split points. Empty halves and unsupported metadata fail atomically.
    pub fn split_caption_at_playhead(&mut self) -> bool {
        if self.selection.clips.len() != 1
            || !self.selection.transitions.is_empty()
            || self.selection.gap.is_some()
        {
            self.error("Select one caption to split");
            return false;
        }
        let selected = self.selection.clips[0];
        let playhead = self.playhead();
        let result = self.seq_edit("Split Caption", move |p, sid, _| {
            let seq = p
                .sequence(sid)
                .ok_or_else(|| invalid("Open a sequence first"))?;
            let cut = playhead.round_frame(seq.rate());
            let (r, original) = seq
                .find_clip(selected)
                .ok_or_else(|| invalid("Select a caption first"))?;
            let track = seq.track(r).expect("existing track");
            if track.locked {
                return Err(EditError::Locked(track.name.clone()));
            }
            if transitions_touch(track, &[selected]) {
                return Err(invalid("Remove caption transitions before splitting"));
            }
            if cut <= original.start || cut >= original.end() {
                return Err(invalid("Place the playhead inside the caption"));
            }
            let words = caption_words(original)?;
            let offset = (cut - original.start).seconds();
            let boundary = words.partition_point(|w| w.end <= offset);
            if boundary == 0 || boundary == words.len() || words[boundary].start < offset {
                return Err(invalid(
                    "Place the playhead between caption words to preserve their timing",
                ));
            }
            let mut left = original.clone();
            let mut right = original.clone();
            left.duration = cut - original.start;
            right.duration = original.end() - cut;
            right.start = cut;
            right.source_in += cut - original.start;
            right.id = p.ids.clip();
            for comp in &mut right.components {
                comp.id = p.ids.component();
            }
            set_words(&mut left, &words[..boundary]);
            let mut later = words[boundary..].to_vec();
            for word in &mut later {
                word.start -= offset;
                word.end -= offset;
            }
            set_words(&mut right, &later);
            let right_id = right.id;
            let track = p
                .sequence_mut(sid)
                .expect("existing sequence")
                .track_mut(r)
                .expect("existing track");
            let index = track.index_of(selected).expect("existing caption");
            track.clips[index] = left;
            track.clips.insert(index + 1, right);
            Ok(vec![selected, right_id])
        });
        if let Some(ids) = result {
            self.selection = Selection::only(ids);
            true
        } else {
            false
        }
    }

    /// Merge all selected captions when they are consecutive, touching and identically styled.
    /// Track order determines text order; gaps, transitions and distinct animation are rejected.
    pub fn merge_selected_captions(&mut self) -> bool {
        if self.selection.clips.len() < 2
            || !self.selection.transitions.is_empty()
            || self.selection.gap.is_some()
        {
            self.error("Select at least two adjacent captions to merge");
            return false;
        }
        let selected = self.selection.clips.clone();
        let result = self.seq_edit("Merge Captions", move |p, sid, _| {
            let seq = p
                .sequence(sid)
                .ok_or_else(|| invalid("Open a sequence first"))?;
            let (r, _) = seq
                .find_clip(selected[0])
                .ok_or_else(|| invalid("Select captions only"))?;
            if selected
                .iter()
                .any(|id| seq.find_clip(*id).is_none_or(|(other, _)| other != r))
            {
                return Err(invalid("Select captions from the same track"));
            }
            let track = seq.track(r).expect("existing track");
            if track.locked {
                return Err(EditError::Locked(track.name.clone()));
            }
            if transitions_touch(track, &selected) {
                return Err(invalid("Remove caption transitions before merging"));
            }
            let indices: Vec<_> = track
                .clips
                .iter()
                .enumerate()
                .filter(|(_, c)| selected.contains(&c.id))
                .map(|(i, _)| i)
                .collect();
            if indices.len() != selected.len() || indices.windows(2).any(|w| w[1] != w[0] + 1) {
                return Err(invalid(
                    "Select consecutive captions without other clips between them",
                ));
            }
            let begin = indices[0];
            let end = *indices.last().expect("selected captions") + 1;
            let clips = &track.clips[begin..end];
            if clips.windows(2).any(|w| w[0].end() != w[1].start) {
                return Err(invalid(
                    "Caption boundaries must touch; gaps and overlaps are not merged",
                ));
            }
            let first = &clips[0];
            let mut words = Vec::new();
            for clip in clips {
                let mut current = caption_words(clip)?;
                if !merge_compatible(first, clip) {
                    return Err(invalid(
                        "Captions must have identical static styles and effects to merge",
                    ));
                }
                let offset = (clip.start - first.start).seconds();
                for word in &mut current {
                    word.start += offset;
                    word.end += offset;
                }
                words.extend(current);
            }
            let mut merged = first.clone();
            merged.duration = clips.last().expect("selected captions").end() - first.start;
            set_words(&mut merged, &words);
            let id = merged.id;
            let track = p
                .sequence_mut(sid)
                .expect("existing sequence")
                .track_mut(r)
                .expect("existing track");
            track.clips.splice(begin..end, [merged]);
            Ok(id)
        });
        if let Some(id) = result {
            self.selection = Selection::only(vec![id]);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Dirs, Preferences};

    fn fixture(rate: Rate, cues: &[captions::Cue]) -> (tempfile::TempDir, Editor) {
        let dir = tempfile::tempdir().unwrap();
        let mut e = Editor::new(Dirs::portable(dir.path()), Preferences::default(), false);
        e.new_sequence(
            "Captions",
            SequenceSettings {
                rate,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            e.create_captions(cues, &crate::captions::CaptionOptions::default()),
            cues.len()
        );
        (dir, e)
    }

    fn gap_cue() -> captions::Cue {
        captions::Cue {
            start: 0.0,
            end: 2.002,
            words: vec![
                TimedWord {
                    text: "Hello".into(),
                    start: 0.1,
                    end: 0.3,
                },
                TimedWord {
                    text: "world".into(),
                    start: 1.0,
                    end: 1.2,
                },
            ],
        }
    }

    fn touching_cues() -> Vec<captions::Cue> {
        (0..3)
            .map(|i| {
                captions::Cue::from_text(
                    ["First phrase", "Second phrase", "Last phrase"][i],
                    i as f64 * 2.0,
                    (i + 1) as f64 * 2.0,
                )
            })
            .collect()
    }

    fn assert_rejected(e: &mut Editor, split: bool) {
        let before = e.project.clone();
        let selection = e.selection.clone();
        let steps = e.history.labels().0.len();
        let accepted = if split {
            e.split_caption_at_playhead()
        } else {
            e.merge_selected_captions()
        };
        assert!(!accepted);
        assert_eq!(
            e.project, before,
            "failed edit must not mutate the project or ID generator"
        );
        assert_eq!(e.selection, selection);
        assert_eq!(e.history.labels().0.len(), steps);
        assert!(e.status.as_ref().is_some_and(|s| s.error));
    }

    #[test]
    fn split_preserves_exact_frames_word_times_graphics_and_undo() {
        let _guard = crate::recovery::TEST_GUARD.lock();
        let rate = Rate::new(30_000, 1001);
        let (_dir, mut e) = fixture(rate, &[gap_cue()]);
        let id = e.caption_clips()[0];
        let sid = e.active.unwrap();
        e.edit("Animate and style", |p| {
            let c = p.sequence_mut(sid).unwrap().clip_mut(id).unwrap();
            c.source_in = SrcTime::from_seconds(3.0);
            c.component_mut(catalog::CAPTION)
                .unwrap()
                .param_mut("fill")
                .unwrap()
                .value = Value::Color(Rgba::new(0.2, 0.4, 0.8, 1.0));
            let param = c
                .component_mut(catalog::MOTION)
                .unwrap()
                .param_mut("position")
                .unwrap();
            param.animated = true;
            param.keys = vec![
                Keyframe::new(
                    SrcTime::from_seconds(3.0),
                    Value::Point([0.2, 0.3]),
                    Interp::Linear,
                ),
                Keyframe::new(
                    SrcTime::from_seconds(5.0),
                    Value::Point([0.4, 0.5]),
                    Interp::Linear,
                ),
            ];
            Ok(())
        })
        .unwrap();
        let cut = rate.frame_start::<Seq>(11);
        // nearest frame, rather than storing a fractional frame in the caption.
        e.set_playhead(cut + Dur(1));
        e.selection = Selection::only(vec![id]);
        let before = e.project.clone();
        let original = before.sequence(sid).unwrap().clip(id).unwrap();
        let steps = e.history.labels().0.len();
        assert!(e.split_caption_at_playhead());
        assert_eq!(e.history.labels().0.len(), steps + 1);
        let right_id = e.selection.clips[1];
        assert_eq!(e.selection.clips[0], id);
        let seq = e.active_seq().unwrap();
        let left = seq.clip(id).unwrap();
        let right = seq.clip(right_id).unwrap();
        assert_eq!(left.end(), cut);
        assert_eq!(right.start, cut);
        assert_eq!(left.duration + right.duration, original.duration);
        assert_eq!(right.end(), original.end());
        assert_eq!(right.source, original.source);
        assert_eq!(right.source_in, original.source_in + (cut - original.start));
        assert_eq!(
            left.component(catalog::MOTION).unwrap(),
            original.component(catalog::MOTION).unwrap()
        );
        let mut right_motion = right.component(catalog::MOTION).unwrap().clone();
        assert_ne!(
            right_motion.id,
            original.component(catalog::MOTION).unwrap().id
        );
        right_motion.id = original.component(catalog::MOTION).unwrap().id;
        assert_eq!(right_motion, *original.component(catalog::MOTION).unwrap());
        for clip in [left, right] {
            assert_eq!(
                clip.component(catalog::CAPTION).unwrap().param("fill"),
                original.component(catalog::CAPTION).unwrap().param("fill")
            );
        }
        let left_words = caption_words(left).unwrap();
        let right_words = caption_words(right).unwrap();
        assert_eq!(left_words, gap_cue().words[..1]);
        assert_eq!(right_words[0].text, "world");
        assert!((right_words[0].start + cut.seconds() - 1.0).abs() < 1e-12);
        assert!((right_words[0].end + cut.seconds() - 1.2).abs() < 1e-12);
        // Unrelated video and audio track revisions keep their original data.
        assert_eq!(seq.video[..3], before.sequence(sid).unwrap().video[..3]);
        assert_eq!(seq.audio, before.sequence(sid).unwrap().audio);
        let after = e.project.clone();
        e.undo();
        assert_eq!(e.project, before);
        e.redo();
        assert_eq!(e.project, after);
    }

    #[test]
    fn merge_uses_track_order_preserves_word_times_and_is_one_undo_step() {
        let _guard = crate::recovery::TEST_GUARD.lock();
        let (_dir, mut e) = fixture(Rate::FPS_25, &touching_cues());
        let mut ids = e.caption_clips();
        let kept = ids[0];
        let original_cues = e.caption_cues();
        ids.reverse();
        e.selection = Selection::only(ids);
        let before = e.project.clone();
        let steps = e.history.labels().0.len();
        assert!(e.merge_selected_captions());
        assert_eq!(e.history.labels().0.len(), steps + 1);
        assert_eq!(e.selection, Selection::only(vec![kept]));
        assert_eq!(e.caption_clips(), vec![kept]);
        let cue = &e.caption_cues()[0];
        assert_eq!(cue.text(), "First phrase Second phrase Last phrase");
        assert_eq!(cue.start, 0.0);
        assert_eq!(cue.end, 6.0);
        assert_eq!(
            cue.words,
            original_cues
                .into_iter()
                .flat_map(|c| c.words)
                .collect::<Vec<_>>()
        );
        e.undo();
        assert_eq!(e.project, before);
    }

    #[test]
    fn split_then_merge_round_trips_word_timing_without_millisecond_drift() {
        let _guard = crate::recovery::TEST_GUARD.lock();
        let rate = Rate::new(30_000, 1001);
        let (_dir, mut e) = fixture(rate, &[gap_cue()]);
        let cues = e.caption_cues();
        e.set_playhead(rate.frame_start(11));
        assert!(e.split_caption_at_playhead());
        assert!(e.merge_selected_captions());
        assert_eq!(e.caption_cues(), cues);
    }

    #[test]
    fn split_rejects_word_cuts_empty_halves_boundaries_and_noncaption_selection() {
        let _guard = crate::recovery::TEST_GUARD.lock();
        let (_dir, mut e) = fixture(Rate::FPS_25, &[gap_cue()]);
        for seconds in [0.0, 0.04, 0.2, 1.6, 2.04] {
            e.set_playhead(SeqTime::from_seconds(seconds));
            assert_rejected(&mut e, true);
        }
        e.selection.clear();
        assert_rejected(&mut e, true);
    }

    #[test]
    fn merge_rejects_missing_selection_gaps_unselected_neighbors_and_styles() {
        let _guard = crate::recovery::TEST_GUARD.lock();
        let (_dir, mut e) = fixture(Rate::FPS_25, &touching_cues());
        let ids = e.caption_clips();
        e.selection = Selection::only(vec![ids[0], ids[2]]);
        assert_rejected(&mut e, false);
        e.selection = Selection::only(vec![ids[0]]);
        assert_rejected(&mut e, false);
        let sid = e.active.unwrap();
        e.edit("Gap", |p| {
            p.sequence_mut(sid).unwrap().clip_mut(ids[2]).unwrap().start += Dur::from_seconds(1.0);
            Ok(())
        })
        .unwrap();
        e.selection = Selection::only(ids.clone());
        assert_rejected(&mut e, false);
        e.undo();
        e.set_caption_values(
            &[ids[1]],
            vec![("font_size".into(), Value::Float(99.0))],
            None,
        );
        e.selection = Selection::only(ids);
        assert_rejected(&mut e, false);
    }

    #[test]
    fn structural_edits_respect_locked_tracks_and_merge_preserves_keyframes() {
        let _guard = crate::recovery::TEST_GUARD.lock();
        let (_dir, mut e) = fixture(Rate::FPS_25, &touching_cues());
        let ids = e.caption_clips();
        let sid = e.active.unwrap();
        let (r, _) = e.active_seq().unwrap().find_clip(ids[0]).unwrap();
        e.edit("Lock", |p| {
            p.sequence_mut(sid).unwrap().track_mut(r).unwrap().locked = true;
            Ok(())
        })
        .unwrap();
        assert_rejected(&mut e, false);
        e.selection = Selection::only(vec![ids[0]]);
        e.set_playhead(SeqTime::from_seconds(1.0));
        assert_rejected(&mut e, true);
        e.undo();
        e.edit("Saved keyframe", |p| {
            let c = p.sequence_mut(sid).unwrap().clip_mut(ids[1]).unwrap();
            c.component_mut(catalog::CAPTION)
                .unwrap()
                .param_mut("font_size")
                .unwrap()
                .keys
                .push(Keyframe::new(
                    SrcTime::ZERO,
                    Value::Float(42.0),
                    Interp::Linear,
                ));
            Ok(())
        })
        .unwrap();
        e.selection = Selection::only(ids);
        assert_rejected(&mut e, false);
    }

    #[test]
    fn merge_rejects_different_tracks_and_split_preserves_invalid_timing_without_mutation() {
        let _guard = crate::recovery::TEST_GUARD.lock();
        let (_dir, mut e) = fixture(Rate::FPS_25, &touching_cues());
        let ids = e.caption_clips();
        let sid = e.active.unwrap();
        let (r, _) = e.active_seq().unwrap().find_clip(ids[0]).unwrap();
        e.edit("Different track", |p| {
            let seq = p.sequence_mut(sid).unwrap();
            let clip = seq.track_mut(r).unwrap().clips.remove(1);
            seq.track_mut(TrackRef::video(0)).unwrap().clips.push(clip);
            Ok(())
        })
        .unwrap();
        e.selection = Selection::only(ids.clone());
        assert_rejected(&mut e, false);
        e.selection = Selection::only(vec![ids[0]]);
        e.set_caption_values(
            &[ids[0]],
            vec![("timing".into(), Value::Text("NaN-0.5 0.5-1.0".into()))],
            None,
        );
        e.set_playhead(SeqTime::from_seconds(1.0));
        assert_rejected(&mut e, true);
    }

    #[test]
    fn boundaries_preserve_ntsc_word_positions_source_animation_text_and_undo() {
        let _guard = crate::recovery::TEST_GUARD.lock();
        let rate = Rate::new(30_000, 1001);
        let (_dir, mut e) = fixture(rate, &[gap_cue()]);
        let id = e.caption_clips()[0];
        let sid = e.active.unwrap();
        e.edit("Original animation", |p| {
            let clip = p.sequence_mut(sid).unwrap().clip_mut(id).unwrap();
            clip.source_in = SrcTime::from_seconds(3.0);
            clip.name = "Custom caption name".into();
            clip.component_mut(catalog::CAPTION)
                .unwrap()
                .param_mut("text")
                .unwrap()
                .value = Value::Text("Hello  \nworld".into());
            let param = clip
                .component_mut(catalog::MOTION)
                .unwrap()
                .param_mut("position")
                .unwrap();
            param.animated = true;
            param.keys.push(Keyframe::new(
                SrcTime::from_seconds(3.0),
                Value::Point([0.2, 0.3]),
                Interp::Linear,
            ));
            Ok(())
        })
        .unwrap();
        let words = e.caption_cues()[0].words.clone();
        let before = e.project.clone();
        let selection = e.selection.clone();
        let steps = e.history.labels().0.len();
        let start = rate.frame_start::<Seq>(1);
        let end = rate.frame_start::<Seq>(46);
        assert!(e.set_caption_bounds(id, start + Dur(1), end - Dur(1)));
        assert_eq!(e.history.labels().0.len(), steps + 1);
        assert_eq!(e.selection, selection);
        let original = before.sequence(sid).unwrap().clip(id).unwrap();
        let edited = e.active_seq().unwrap().clip(id).unwrap();
        assert_eq!(edited.start, start);
        assert_eq!(edited.end(), end);
        assert_eq!(
            edited.source_in,
            original.source_in + (start - original.start)
        );
        assert_eq!(
            edited.component(catalog::MOTION),
            original.component(catalog::MOTION)
        );
        assert_eq!(edited.name, original.name);
        assert_eq!(
            edited.component(catalog::CAPTION).unwrap().param("text"),
            original.component(catalog::CAPTION).unwrap().param("text")
        );
        for (a, b) in words.iter().zip(&e.caption_cues()[0].words) {
            assert_eq!(a.text, b.text);
            assert!((a.start - b.start).abs() < 1e-12);
            assert!((a.end - b.end).abs() < 1e-12);
        }
        let after = e.project.clone();
        e.undo();
        assert_eq!(e.project, before);
        e.redo();
        assert_eq!(e.project, after);
    }

    #[test]
    fn static_caption_can_expand_before_its_source_zero_without_moving_words() {
        let _guard = crate::recovery::TEST_GUARD.lock();
        let mut cue = gap_cue();
        cue.start += 2.0;
        cue.end += 2.0;
        for word in &mut cue.words {
            word.start += 2.0;
            word.end += 2.0;
        }
        let (_dir, mut e) = fixture(Rate::FPS_25, &[cue]);
        let id = e.caption_clips()[0];
        let original = e.active_seq().unwrap().clip(id).unwrap().clone();
        let words = e.caption_cues()[0].words.clone();
        assert!(e.set_caption_bounds(id, SeqTime::from_seconds(1.52), SeqTime::from_seconds(4.4)));
        assert_eq!(
            e.active_seq().unwrap().clip(id).unwrap().source_in,
            SrcTime::ZERO
        );
        assert_eq!(e.caption_cues()[0].words, words);
        assert!(e.set_caption_bounds(id, original.start, original.end()));
        assert_eq!(e.caption_cues()[0].words, words);
        let steps = e.history.labels().0.len();
        assert!(!e.set_caption_bounds(id, original.start, original.end()));
        assert_eq!(
            e.history.labels().0.len(),
            steps,
            "unchanged times do not add undo steps"
        );
    }

    #[test]
    fn boundary_failures_keep_project_selection_history_and_neighbors() {
        let _guard = crate::recovery::TEST_GUARD.lock();
        let (_dir, mut e) = fixture(Rate::FPS_25, &[gap_cue()]);
        let id = e.caption_clips()[0];
        for (a, b) in [(-1.0, 2.0), (0.0, 0.0), (1.0, 0.5), (0.2, 2.0), (0.0, 1.0)] {
            let before = e.project.clone();
            let selection = e.selection.clone();
            let steps = e.history.labels().0.len();
            assert!(!e.set_caption_bounds(id, SeqTime::from_seconds(a), SeqTime::from_seconds(b)));
            assert_eq!(e.project, before);
            assert_eq!(e.selection, selection);
            assert_eq!(e.history.labels().0.len(), steps);
        }
        let (_dir, mut e) = fixture(Rate::FPS_25, &touching_cues());
        let id = e.caption_clips()[0];
        let before = e.project.clone();
        assert!(!e.set_caption_bounds(id, SeqTime::ZERO, SeqTime::from_seconds(2.4)));
        assert_eq!(e.project, before);
        let sid = e.active.unwrap();
        let r = e.active_seq().unwrap().find_clip(id).unwrap().0;
        e.edit("Lock", |p| {
            p.sequence_mut(sid).unwrap().track_mut(r).unwrap().locked = true;
            Ok(())
        })
        .unwrap();
        let before = e.project.clone();
        assert!(!e.set_caption_bounds(id, SeqTime::ZERO, SeqTime::from_seconds(2.4)));
        assert_eq!(e.project, before);
    }

    #[test]
    fn explicit_split_failure_retains_context_and_boundary_animation_needs_source_headroom() {
        let _guard = crate::recovery::TEST_GUARD.lock();
        let mut cue = gap_cue();
        cue.start += 2.0;
        cue.end += 2.0;
        for word in &mut cue.words {
            word.start += 2.0;
            word.end += 2.0;
        }
        let (_dir, mut e) = fixture(Rate::FPS_25, &[cue]);
        let id = e.caption_clips()[0];
        let sid = e.active.unwrap();
        e.edit("Saved animation", |p| {
            p.sequence_mut(sid)
                .unwrap()
                .clip_mut(id)
                .unwrap()
                .component_mut(catalog::MOTION)
                .unwrap()
                .param_mut("position")
                .unwrap()
                .keys
                .push(Keyframe::new(
                    SrcTime::ZERO,
                    Value::Point([0.2, 0.3]),
                    Interp::Linear,
                ));
            Ok(())
        })
        .unwrap();
        let before = e.project.clone();
        assert!(!e.set_caption_bounds(id, SeqTime::from_seconds(1.0), SeqTime::from_seconds(4.0)));
        assert_eq!(e.project, before);
        e.selection.clear();
        let selection = e.selection.clone();
        let playhead = e.playhead();
        assert!(!e.split_caption(id, SeqTime::from_seconds(2.2)));
        assert_eq!(e.selection, selection);
        assert_eq!(e.playhead(), playhead);
        assert_eq!(e.project, before);
    }

    #[test]
    fn end_only_edit_freezes_existing_rendered_timing_after_a_text_word_count_change() {
        let _guard = crate::recovery::TEST_GUARD.lock();
        let (_dir, mut e) = fixture(Rate::FPS_25, &[gap_cue()]);
        let id = e.caption_clips()[0];
        e.set_caption_values(
            &[id],
            vec![("text".into(), Value::Text("Three synthetic words".into()))],
            None,
        );
        let before = e.caption_cues()[0].words.clone();
        // Existing renderer spreads edited text when its word count differs from the saved
        // timing. Capture that interpretation before changing duration, rather than re-spread.
        assert!(e.set_caption_bounds(id, SeqTime::ZERO, SeqTime::from_seconds(2.4)));
        assert_eq!(e.caption_cues()[0].words, before);
    }

    #[test]
    fn caption_commands_route_to_the_same_structural_transactions() {
        let _guard = crate::recovery::TEST_GUARD.lock();
        let (_dir, mut e) = fixture(Rate::FPS_25, &[gap_cue()]);
        e.set_playhead(SeqTime::from_seconds(0.4));
        let steps = e.history.labels().0.len();
        assert!(e.execute("op.captions.split", crate::Focus::Timeline));
        assert_eq!(e.caption_clips().len(), 2);
        assert_eq!(e.history.labels().0.len(), steps + 1);
        assert!(e.execute("op.captions.merge", crate::Focus::Timeline));
        assert_eq!(e.caption_clips().len(), 1);
        assert_eq!(e.history.labels().0.len(), steps + 2);
    }
}
