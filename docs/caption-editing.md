# Caption editing

The existing Captions panel now has **Split at Playhead**, **Merge Selected**, and a **Times...** editor on each caption row. This is an
independent Rust implementation using OpenPremier's project transactions and history. It does
not claim identical behavior to a particular Premiere build.

## Split at Playhead

Select one caption and place the playhead between two words, inside the caption. The operation
rounds the playhead to the nearest sequence frame, keeps the first clip's identity, and selects
both resulting clips. Its total duration, absolute word timing, styles, source-time mapping and
graphics keyframes are preserved; the second clip gets independent clip/component IDs. Undo
restores the original project in one step.

A cut through a word, an empty half, a clip boundary or a locked track is rejected. Retimed,
reversed, held, linked or grouped captions, caption transitions, animated text/timing and foreign
or unknown components are unsupported. Errors retain the document and selection.

## Merge Selected

Select at least two consecutive captions on the same unlocked track. Their boundaries must
touch, and their static styles and component stacks must agree. Track order determines text
order even when selection order differs. The first clip's identity and style are retained;
word times are offset into the merged clip without millisecond rounding. The merged caption is
selected. Undo restores every original clip in one step.

Gaps, overlaps, unselected intervening clips, transitions, different tracks/styles, animation,
foreign data and unsupported caption sources are rejected. Merge does not fill a gap or discard
another clip's styling. Text whitespace is normalized when rebuilding word-based captions.

## Implementation and verification

Policy belongs in `op-application/src/caption_edit.rs`; `op-ui/src/captions_panel.rs` only supplies
the controls. No panel was removed and no new implementation language or dependency was added.
Thirteen original regression tests cover NTSC frame placement, absolute word timing, style and
keyframe preservation, source offsets, selection, unchanged unrelated tracks, undo/redo,
split/merge timing round trips, locks and unsupported inputs:

```text
cargo xtask cargo test -p op-application --lib caption_edit::tests -- --test-threads=1
```

The rendered font/pixel appearance, speech alignment and matching Adobe caption behavior remain
separate validation tasks. See the boundary-editing contract below for supported start/end edits.

## Edit Times

On a caption row, open **Times...**, enter sequence start/end positions in seconds, and choose
**Apply Times**. Applying changes commits one transaction; typing does not mutate the project.
Input must be finite, nonnegative and within the supported sequence-time range. Both positions
round to the nearest sequence frame, including exact 30000/1001 frame boundaries, and the end
must follow the start. Unchanged rounded boundaries add no undo step.

The operation changes the on-screen interval while preserving the words' absolute sequence
times. Changing the start shifts relative word timing in the opposite direction. Changing only
the end also materializes the original rendered word timing when an earlier text edit caused
the renderer to derive timings from word lengths. Text whitespace, custom clip names, styles,
component IDs and keyframes stay intact. No neighbor is moved or overwritten. Undo/redo restores
the entire document revision.

The normal source-time rule is `new_source_in = old_source_in + (new_start - old_start)`, keeping
source-domain keyframes attached to their original sequence positions. There is one explicit
exception: an entirely static Graphic stack may expand earlier than source time zero; its
original source offset is retained because it has no source media or keyframes to reveal.
Such static expansion preserves word times, but does not promise a reversible source offset
through subsequent boundary edits. Any active or stored keyframes reject expansion that would
require negative source time. Normal-speed Graphic captions only are supported.

A boundary that cuts off any word, creates an empty/negative interval, overlaps another clip,
changes a locked track or touches a caption transition is rejected without changing the document,
selection or history. Existing split restrictions on sources, links/groups, foreign/unknown
components and animated text/timing also apply.

## Application commands and MCP

`op.captions.split` and `op.captions.merge` route the current selection/playhead through the same
policy. Explicit application methods `split_caption(id, at)`, `merge_captions(ids)` and
`set_caption_bounds(id, start, end)` support callers without panel widgets. An explicit split
preserves the caller's playhead, and failed structural edits restore the caller's selection.

Three tools are available in both local and live MCP editing:

| Tool | Required arguments | Result |
|---|---|---|
| `split_caption` | `clip_id`, `at` (sequence seconds) | Original and new right-hand clip IDs |
| `merge_captions` | `clip_ids` (at least two distinct positive integer IDs) | Retained first clip ID |
| `set_caption_bounds` | `clip_id`, `start`, `end` (sequence seconds) | Applied seconds, exact start/end ticks and `changed` |

Malformed IDs/arrays, missing arguments, negative/nonfinite/out-of-range times and unsupported
edits return errors. Explicit tools retain project, history, selection and playhead on failure.
Each successful change is one undo step; a boundary no-op returns `changed: false`.

Additional protocol regression coverage:

```text
cargo xtask cargo test -p op-mcp --test caption_editing -- --test-threads=1
```
