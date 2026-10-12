//! Caption tools exercise the same transactions as the panel, with strict JSON validation.
use op_application::{Dirs, Editor, Preferences, Selection};
use op_core::captions::{Cue, TimedWord};
use op_core::*;
use op_mcp::tools;
use serde_json::{Value, json};

fn fixture() -> (tempfile::TempDir, Editor) {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = Editor::new(Dirs::portable(dir.path()), Preferences::default(), false);
    ed.new_sequence("Caption tools", SequenceSettings::default())
        .unwrap();
    ed.create_captions(
        &[Cue {
            start: 0.0,
            end: 2.0,
            words: vec![
                TimedWord {
                    text: "Synthetic".into(),
                    start: 0.1,
                    end: 0.3,
                },
                TimedWord {
                    text: "fixture".into(),
                    start: 1.0,
                    end: 1.2,
                },
            ],
        }],
        &Default::default(),
    );
    (dir, ed)
}

fn output(ed: &mut Editor, name: &str, args: Value) -> Value {
    let content = tools::call(ed, name, &args).unwrap();
    match &content[0] {
        tools::Content::Text(text) => serde_json::from_str(text).unwrap(),
        _ => panic!("expected JSON text"),
    }
}

#[test]
fn caption_tools_are_listed_with_required_explicit_arguments() {
    let list = tools::list();
    for (name, required) in [
        ("split_caption", vec!["clip_id", "at"]),
        ("merge_captions", vec!["clip_ids"]),
        ("set_caption_bounds", vec!["clip_id", "start", "end"]),
    ] {
        let tool = list
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap();
        assert_eq!(tool["inputSchema"]["required"], json!(required));
    }
}

#[test]
fn caption_tool_split_merge_and_bounds_have_one_step_undo_and_live_support() {
    let (_dir, mut ed) = fixture();
    let id = ed.caption_clips()[0];
    ed.selection.clear();
    ed.set_playhead(SeqTime::from_seconds(7.0));
    let before = ed.project.clone();
    let count = ed.history.labels().1;
    let split = output(
        &mut ed,
        "split_caption",
        json!({"clip_id": id.0, "at": 0.423}),
    );
    assert_eq!(ed.history.labels().1, count + 1);
    assert_eq!(ed.playhead(), SeqTime::from_seconds(7.0));
    let right = split["clip_ids"][1].as_u64().unwrap();
    assert_eq!(
        ed.active_seq().unwrap().clip(ClipId(right)).unwrap().start,
        SeqTime::from_seconds(0.44)
    );
    let parts = ed.project.clone();
    output(
        &mut ed,
        "merge_captions",
        json!({"clip_ids": [right, id.0]}),
    );
    assert_eq!(ed.selection, Selection::only(vec![id]));
    assert_eq!(ed.history.labels().1, count + 2);
    ed.undo();
    assert_eq!(ed.project, parts);
    ed.undo();
    assert_eq!(ed.project, before);
    let before = ed.project.clone();
    let count = ed.history.labels().1;
    // Live calls use the same boundary policy and reply with exact ticks alongside seconds.
    let content = tools::call_live(
        &mut ed,
        "set_caption_bounds",
        &json!({"clip_id": id.0, "start": 0.04, "end": 1.6}),
    )
    .unwrap();
    let values: Value = match &content[0] {
        tools::Content::Text(text) => serde_json::from_str(text).unwrap(),
        _ => panic!("expected JSON"),
    };
    assert_eq!(
        values["start_ticks"],
        json!(SeqTime::from_seconds(0.04).ticks())
    );
    assert_eq!(
        values["end_ticks"],
        json!(SeqTime::from_seconds(1.6).ticks())
    );
    assert_eq!(ed.history.labels().1, count + 1);
    ed.undo();
    assert_eq!(ed.project, before);
}

#[test]
fn invalid_caption_tools_preserve_document_and_caller_context() {
    let (_dir, mut ed) = fixture();
    let id = ed.caption_clips()[0];
    ed.selection.clear();
    ed.set_playhead(SeqTime::from_seconds(7.0));
    for (name, args) in [
        ("split_caption", json!({"clip_id": id.0, "at": 0.2})),
        ("split_caption", json!({"clip_id": id.0, "at": -1.0})),
        ("split_caption", json!({"clip_id": id.0, "at": "NaN"})),
        ("split_caption", json!({"clip_id": 1.5, "at": 0.4})),
        ("merge_captions", json!({"clip_ids": [id.0, id.0]})),
        ("merge_captions", json!({"clip_ids": [id.0, "invalid"]})),
        ("merge_captions", json!({"clip_ids": [id.0, 999_999]})),
        (
            "set_caption_bounds",
            json!({"clip_id": id.0, "start": 0.2, "end": 2.0}),
        ),
        (
            "set_caption_bounds",
            json!({"clip_id": id.0, "start": 0.0, "end": 1e300}),
        ),
    ] {
        let before = ed.project.clone();
        let selection = ed.selection.clone();
        let count = ed.history.labels().1;
        let playhead = ed.playhead();
        assert!(tools::call(&mut ed, name, &args).is_err(), "{name}: {args}");
        assert_eq!(ed.project, before);
        assert_eq!(ed.selection, selection);
        assert_eq!(ed.playhead(), playhead);
        assert_eq!(ed.history.labels().1, count);
    }
    let count = ed.history.labels().1;
    let no_op = output(
        &mut ed,
        "set_caption_bounds",
        json!({"clip_id": id.0, "start": 0.0, "end": 2.0}),
    );
    assert_eq!(no_op["changed"], false);
    assert_eq!(ed.history.labels().1, count);
}
