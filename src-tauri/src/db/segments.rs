use serde::{Deserialize, Serialize};

use super::Db;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Segment {
    pub id: i64,
    pub tab_id: i64,
    pub position: i64,
    pub text: String,
    pub original_text: String,
    pub audio_path: String,         // relative to %APPDATA%\voicetabs\audio\
    pub started_at: i64,            // unix ms
    pub ended_at: i64,
    pub duration_ms: i64,
    pub vocab_snapshot: String,     // JSON array of strings
    pub avg_logprob: f64,
    pub no_speech_prob: f64,
    pub model_id: String,
}

/// All fields except `id` and `position`; the repository assigns those at
/// insert time. `text` and `original_text` are normally identical at insert
/// time (the user has not edited yet); `update_text` later diverges them.
#[derive(Debug, Clone)]
pub struct NewSegment {
    pub tab_id: i64,
    pub text: String,
    pub audio_path: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub duration_ms: i64,
    pub vocab_snapshot: String,
    pub avg_logprob: f64,
    pub no_speech_prob: f64,
    pub model_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SegmentsError {
    #[error("segment not found: {0}")]
    NotFound(i64),
    #[error("tab not found: {0}")]
    TabNotFound(i64),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
}

pub fn list_for_tab(db: &Db, tab_id: i64) -> Result<Vec<Segment>, SegmentsError> {
    db.with(|c| {
        let mut stmt = c.prepare(
            "SELECT id, tab_id, position, text, original_text, audio_path,
                    started_at, ended_at, duration_ms, vocab_snapshot,
                    avg_logprob, no_speech_prob, model_id
             FROM segments
             WHERE tab_id = ?
             ORDER BY position ASC, id ASC",
        )?;
        let rows = stmt
            .query_map([tab_id], row_to_segment)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

pub fn get_by_id(db: &Db, id: i64) -> Result<Segment, SegmentsError> {
    db.with(|c| {
        c.query_row(
            "SELECT id, tab_id, position, text, original_text, audio_path,
                    started_at, ended_at, duration_ms, vocab_snapshot,
                    avg_logprob, no_speech_prob, model_id
             FROM segments WHERE id = ?",
            [id],
            row_to_segment,
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => SegmentsError::NotFound(id),
            other => other.into(),
        })
    })
}

pub fn insert(db: &Db, new: &NewSegment) -> Result<Segment, SegmentsError> {
    db.with(|c| {
        let tx = c.transaction()?;
        // Confirm the tab still exists. If it was deleted between the
        // utterance starting and the STT result landing, we drop the segment
        // here rather than letting the FK constraint blow up with a less
        // helpful error.
        let tab_exists: i64 = tx.query_row(
            "SELECT COUNT(1) FROM tabs WHERE id = ?",
            [new.tab_id],
            |r| r.get(0),
        )?;
        if tab_exists == 0 {
            return Err(SegmentsError::TabNotFound(new.tab_id));
        }
        let next_position: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM segments WHERE tab_id = ?",
            [new.tab_id],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO segments
               (tab_id, position, text, original_text, audio_path,
                started_at, ended_at, duration_ms, vocab_snapshot,
                avg_logprob, no_speech_prob, model_id)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                new.tab_id,
                next_position,
                &new.text,
                // original_text == text at insert time; edits diverge them.
                &new.text,
                &new.audio_path,
                new.started_at,
                new.ended_at,
                new.duration_ms,
                &new.vocab_snapshot,
                new.avg_logprob,
                new.no_speech_prob,
                &new.model_id,
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(Segment {
            id,
            tab_id: new.tab_id,
            position: next_position,
            text: new.text.clone(),
            original_text: new.text.clone(),
            audio_path: new.audio_path.clone(),
            started_at: new.started_at,
            ended_at: new.ended_at,
            duration_ms: new.duration_ms,
            vocab_snapshot: new.vocab_snapshot.clone(),
            avg_logprob: new.avg_logprob,
            no_speech_prob: new.no_speech_prob,
            model_id: new.model_id.clone(),
        })
    })
}

/// Update only `text`. Used by inline edit. `original_text` is never touched.
pub fn update_text(db: &Db, id: i64, new_text: &str) -> Result<(), SegmentsError> {
    let changed = db.with(|c| {
        c.execute(
            "UPDATE segments SET text = ? WHERE id = ?",
            rusqlite::params![new_text, id],
        )
    })?;
    if changed == 0 {
        return Err(SegmentsError::NotFound(id));
    }
    Ok(())
}

/// Update `text` and `model_id`. Used by re-transcribe. `original_text` and
/// `vocab_snapshot` are preserved so the user can always go back to the
/// first transcription and we still know what biased it.
pub fn update_retranscribed(
    db: &Db,
    id: i64,
    new_text: &str,
    new_model_id: &str,
    new_avg_logprob: f64,
    new_no_speech_prob: f64,
) -> Result<(), SegmentsError> {
    let changed = db.with(|c| {
        c.execute(
            "UPDATE segments
             SET text = ?, model_id = ?, avg_logprob = ?, no_speech_prob = ?
             WHERE id = ?",
            rusqlite::params![new_text, new_model_id, new_avg_logprob, new_no_speech_prob, id],
        )
    })?;
    if changed == 0 {
        return Err(SegmentsError::NotFound(id));
    }
    Ok(())
}

pub fn delete(db: &Db, id: i64) -> Result<(), SegmentsError> {
    let changed = db.with(|c| c.execute("DELETE FROM segments WHERE id = ?", [id]))?;
    if changed == 0 {
        return Err(SegmentsError::NotFound(id));
    }
    Ok(())
}

fn row_to_segment(r: &rusqlite::Row) -> rusqlite::Result<Segment> {
    Ok(Segment {
        id: r.get(0)?,
        tab_id: r.get(1)?,
        position: r.get(2)?,
        text: r.get(3)?,
        original_text: r.get(4)?,
        audio_path: r.get(5)?,
        started_at: r.get(6)?,
        ended_at: r.get(7)?,
        duration_ms: r.get(8)?,
        vocab_snapshot: r.get(9)?,
        avg_logprob: r.get(10)?,
        no_speech_prob: r.get(11)?,
        model_id: r.get(12)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn mem_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/001_initial_schema.sql"))
            .unwrap();
        conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])
            .unwrap();
        // Insert two tabs so foreign keys are satisfiable.
        conn.execute(
            "INSERT INTO tabs (id, title, order_idx, created_at, updated_at) VALUES (1, 'A', 0, 0, 0)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO tabs (id, title, order_idx, created_at, updated_at) VALUES (2, 'B', 1, 0, 0)",
            [],
        ).unwrap();
        Db::from_connection(conn)
    }

    fn sample(tab_id: i64, text: &str) -> NewSegment {
        NewSegment {
            tab_id,
            text: text.into(),
            audio_path: format!("{tab_id}-{text}.wav"),
            started_at: 100,
            ended_at: 200,
            duration_ms: 100,
            vocab_snapshot: "[]".into(),
            avg_logprob: -0.3,
            no_speech_prob: 0.02,
            model_id: "ggml-small-q5_0".into(),
        }
    }

    #[test]
    fn insert_assigns_position_per_tab_starting_at_zero() {
        let db = mem_db();
        let a0 = insert(&db, &sample(1, "first")).unwrap();
        let a1 = insert(&db, &sample(1, "second")).unwrap();
        let b0 = insert(&db, &sample(2, "other-tab")).unwrap();
        assert_eq!(a0.position, 0);
        assert_eq!(a1.position, 1);
        assert_eq!(b0.position, 0, "positions reset per tab");
    }

    #[test]
    fn insert_sets_original_text_equal_to_text() {
        let db = mem_db();
        let s = insert(&db, &sample(1, "hello world")).unwrap();
        assert_eq!(s.text, "hello world");
        assert_eq!(s.original_text, "hello world");
    }

    #[test]
    fn insert_rejects_unknown_tab() {
        let db = mem_db();
        let err = insert(&db, &sample(999, "ghost")).unwrap_err();
        assert!(matches!(err, SegmentsError::TabNotFound(999)), "got {err:?}");
    }

    #[test]
    fn list_for_tab_returns_only_that_tabs_segments_in_position_order() {
        let db = mem_db();
        insert(&db, &sample(1, "a")).unwrap();
        insert(&db, &sample(2, "b")).unwrap();
        insert(&db, &sample(1, "c")).unwrap();
        let listed = list_for_tab(&db, 1).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].text, "a");
        assert_eq!(listed[1].text, "c");
        assert_eq!(listed[0].position, 0);
        assert_eq!(listed[1].position, 1);
    }

    #[test]
    fn update_text_changes_only_text_not_original() {
        let db = mem_db();
        let s = insert(&db, &sample(1, "orig")).unwrap();
        update_text(&db, s.id, "edited").unwrap();
        let after = get_by_id(&db, s.id).unwrap();
        assert_eq!(after.text, "edited");
        assert_eq!(after.original_text, "orig", "original_text must be preserved");
    }

    #[test]
    fn update_retranscribed_changes_text_and_model_id_keeps_originals() {
        let db = mem_db();
        let s = insert(&db, &sample(1, "first transcription")).unwrap();
        update_retranscribed(&db, s.id, "second transcription", "ggml-large-v3-turbo-q5_0", -0.1, 0.01).unwrap();
        let after = get_by_id(&db, s.id).unwrap();
        assert_eq!(after.text, "second transcription");
        assert_eq!(after.model_id, "ggml-large-v3-turbo-q5_0");
        assert_eq!(after.original_text, "first transcription");
        assert_eq!(after.vocab_snapshot, "[]", "vocab_snapshot is the row's snapshot, never overwritten");
    }

    #[test]
    fn delete_removes_the_row() {
        let db = mem_db();
        let s = insert(&db, &sample(1, "die")).unwrap();
        delete(&db, s.id).unwrap();
        assert!(matches!(get_by_id(&db, s.id), Err(SegmentsError::NotFound(_))));
    }

    #[test]
    fn delete_unknown_returns_not_found() {
        let db = mem_db();
        assert!(matches!(delete(&db, 12345), Err(SegmentsError::NotFound(12345))));
    }
}
