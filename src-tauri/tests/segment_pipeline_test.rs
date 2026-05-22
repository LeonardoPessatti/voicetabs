//! Integration test for the Phase-4 finalize → STT → filter → insert pipeline.
//!
//! We do NOT exercise the full worker_loop here (cpal + VAD + threads are
//! tested in Phase 2). This test pins the contract that:
//!   1. The tab_id captured at rising edge is what lands in the DB row, even
//!      if `ActiveTab::set` is called between rising and falling.
//!   2. The vocab snapshot captured at rising edge — not whatever was in
//!      settings at insert time — is what lands in the row's
//!      `vocab_snapshot` column.

use rusqlite::Connection;

use voicetabs_lib::db::{segments as segments_repo, settings as settings_repo, tabs as tabs_repo, Db};
use voicetabs_lib::routing::ActiveTab;

fn mem_db() -> Db {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../migrations/001_initial_schema.sql"))
        .unwrap();
    conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])
        .unwrap();
    Db::from_connection(conn)
}

#[test]
fn snapshot_at_rising_edge_routes_to_original_tab() {
    // Set up in-memory DB with two tabs.
    let db = mem_db();
    let tab_a = tabs_repo::create(&db, "A", 0).unwrap();
    let tab_b = tabs_repo::create(&db, "B", 0).unwrap();

    // ActiveTab starts at A.
    let active = ActiveTab::new();
    active.set(tab_a.id);

    // Snapshot — this is the moment that satisfies A3. The capture worker
    // does the same `active_tab.snapshot()` call at every VAD rising edge.
    let snapshot = active.snapshot().expect("active tab set");

    // User switches to B mid-utterance, then thrashes a bit. None of this
    // can affect the already-snapshotted tab id.
    active.set(tab_b.id);
    active.set(tab_a.id);
    active.set(tab_b.id);

    // Insert the segment as if the worker did so after STT returned.
    let new = segments_repo::NewSegment {
        tab_id: snapshot,
        text: "hello".into(),
        audio_path: "1.wav".into(),
        started_at: 100,
        ended_at: 200,
        duration_ms: 100,
        vocab_snapshot: "[]".into(),
        avg_logprob: -0.3,
        no_speech_prob: 0.02,
        model_id: "test".into(),
    };
    let inserted = segments_repo::insert(&db, &new).unwrap();
    assert_eq!(
        inserted.tab_id, tab_a.id,
        "must land in the originally-active tab"
    );
    assert!(segments_repo::list_for_tab(&db, tab_b.id)
        .unwrap()
        .is_empty());
}

#[test]
fn vocab_snapshot_at_rising_edge_is_what_lands_in_the_row() {
    // The pipeline reads `vocab_terms` from settings at the VAD rising edge
    // and embeds it in the row's `vocab_snapshot` column. Whatever the user
    // changes in settings between rising and finalize must NOT show up in
    // the row.
    let db = mem_db();
    let tab = tabs_repo::create(&db, "A", 0).unwrap();

    settings_repo::set(&db, "vocab_terms", &vec!["pádua".to_string(), "leo".to_string()])
        .unwrap();

    // Read settings snapshot, same as the controller does at the rising edge.
    let snapshot: Vec<String> = settings_repo::get::<Vec<String>>(&db, "vocab_terms")
        .unwrap()
        .unwrap();
    assert_eq!(snapshot, vec!["pádua", "leo"]);

    // User edits vocab mid-utterance.
    settings_repo::set(
        &db,
        "vocab_terms",
        &vec!["something-else".to_string(), "another".to_string()],
    )
    .unwrap();

    // Insert the segment with the snapshot captured at rising edge.
    let snapshot_json = serde_json::to_string(&snapshot).unwrap();
    let new = segments_repo::NewSegment {
        tab_id: tab.id,
        text: "hello".into(),
        audio_path: "1.wav".into(),
        started_at: 100,
        ended_at: 200,
        duration_ms: 100,
        vocab_snapshot: snapshot_json,
        avg_logprob: -0.3,
        no_speech_prob: 0.02,
        model_id: "test".into(),
    };
    let inserted = segments_repo::insert(&db, &new).unwrap();

    // The row carries the rising-edge snapshot, not the post-edit value.
    let stored: Vec<String> = serde_json::from_str(&inserted.vocab_snapshot).unwrap();
    assert_eq!(stored, vec!["pádua", "leo"]);
}
