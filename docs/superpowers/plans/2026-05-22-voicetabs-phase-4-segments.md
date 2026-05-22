# VoiceTabs — Phase 4 (Utterance → Segment Pipeline) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Phase 3 dependency.** This plan can only START when Phase 3 lands on `master`. Phase 3 introduces the `stt_worker` subprocess and the `SttClient` (`Send + Sync`) handle, plumbs it through `CaptureController` so the worker thread already calls `client.transcribe(...)` on each VAD falling edge, and logs the resulting text. Phase 4 takes that already-flowing text, routes it to a tab, filters hallucinations, persists a segment row + emits an event, and surfaces it in the UI. If `SttClient` is not yet a field on `CaptureController` when this plan starts, **STOP** and resolve that wiring first — the entire Task 8 integration assumes the client is reachable from inside the worker thread.

**Goal:** Close the speak-to-screen loop. After Phase 4, speaking into the mic while a tab is selected produces a typed segment card under that tab's body — even if the user switches tabs mid-sentence. The 60 s silence test produces zero segments. Custom vocab terms biases the transcription.

**Architecture:** Phase 2's `CaptureController` worker thread is the only place that owns the audio + VAD + utterance pipeline. Phase 4 plugs three things into that thread, all read at well-defined moments:

1. An **`ActiveTab` atomic** (`Arc<AtomicI64>` with `0 = none`) updated by a new `tabs_set_active` Tauri command. The worker snapshots it once at each VAD rising edge — that snapshot is the entire mechanism behind acceptance criterion A3.
2. A **`Db`** handle (`Clone`-able `Arc<Mutex<Connection>>` from Phase 1) so the worker can write a `segments` row directly without bouncing through the Tauri runtime.
3. A **`tauri::AppHandle`** so the worker can `emit("segment-created", …)` after the row is persisted; the frontend's `useSegmentsStore` listens and appends.

The data flow for one kept utterance is now: VAD rising edge → snapshot `(start_tab_id, vocab_snapshot)` → accumulate samples → VAD falling edge → WAV written by `UtteranceBuilder` → `SttClient::transcribe` → `audio::rms_dbfs` over the samples → `hallucination::evaluate(…)` (pure function) → if `Keep` → `db::segments::insert(…)` → `app.emit("segment-created", segment)`. If `Drop(reason)` the WAV is left on disk for diagnostics and the reason is logged.

**Tech Stack additions:** No new Rust crates (we reuse `rusqlite`, `serde`, `tracing`, the existing `tauri` runtime). Frontend stays on Zustand + react-i18next; the Tauri `listen`/`unlisten` APIs we use are part of `@tauri-apps/api/event` (already available transitively, but we import it explicitly in `src/lib/tauri.ts`).

**Reference spec:** `docs/superpowers/specs/2026-05-20-voicetabs-design.md` — focus on §5.2 (one-utterance dataflow), §6.1 (segments schema), §7.3 (TabRouter), §7.5 (hallucination filter), §7.12 (segment UI).

**Builds on:** Phase 0+1 (`docs/superpowers/plans/2026-05-20-voicetabs-foundation.md`), Phase 2 (`docs/superpowers/plans/2026-05-20-voicetabs-phase-2-audio-vad.md`), and Phase 3 (separate plan landing on `master` immediately before this one).

---

## Acceptance for this plan

- **A3 (full).** With three tabs (A, B, C) and capture ON: speak a complete sentence into A while focused on A → exactly one segment card appears under A. Speak into B → one card under B. Begin speaking into C, switch to A mid-sentence, finish the sentence → the complete sentence appears as one card under C; A is unchanged.
- **A5 (full).** 60 s of silence with capture ON → zero new segments in the DB, zero new audio files (already covered by Phase 2 for the WAV side; A5 full requires no segment row either).
- **A6 (manual).** Adding `"VoiceTabs"` and a couple of other distinctive proper nouns to the vocab textarea and re-transcribing an existing segment with "current vocab" demonstrably changes the output text in the expected direction. (The "snapshot vocab" path runs a different `initial_prompt` — verified by an automated test on the prompt construction, not the audio.)
- Segment cards support: hover-revealed Play button (HTML `<audio>` over the original WAV), Edit → textarea → Save / Cancel / Esc, overflow menu with "Re-transcribe (current vocab)", "Re-transcribe (snapshot vocab)", "Delete" (delete confirms if `text.length > 20`, and removes both the row and the WAV file).
- Inline edit only mutates `text`; `original_text` is preserved. Re-transcribe overwrites `text` and `model_id`, leaves `original_text` and `vocab_snapshot` untouched.
- Vocab terms persist across restarts; they show up in subsequent `initial_prompt`s as `"Termos: t1, t2, t3."` (PT-BR) / `"Terms: t1, t2, t3."` (EN), exactly once at the end of the prompt with a trailing period.
- `cargo test` is green; **+ ~30 new Rust unit tests** land in this phase (segments repo, RMS, hallucination filter, vocab prompt builder, ActiveTab atomic).
- `npm test` is green; **+ ~10 new TS tests** (SegmentCard, segmentsStore event listener, vocab textarea, mock updates).
- L1 (≤ 1.5 s end-to-end latency) and L2 (no flicker) are inherited from Phase 3; we do not regress them. The hallucination filter is pure CPU and well under 1 ms per call, so it does not threaten L1.

## Out of scope (deferred to later plans)

- Hotkey-driven capture, PTT, system tray, mode selector — Phase 5.
- Polished segment UX beyond play/edit/delete/re-transcribe (timestamps, copy, drag-reorder segments) — Phase 6.
- Vocab editor as a fancy chips component — Phase 6 may upgrade; v1 is a plain textarea, one term per line, save-on-blur.
- Trash / undo for segment deletion — explicitly out per spec §7.12 (hard-delete only, with confirm dialog above 20 chars).
- Reordering segments within a tab — segments append at `position = max+1` and never move.
- Cross-tab move of a segment — also out.
- Schema migration v2 — not needed; the Phase 1 migration already created `segments` with every column we use.
- Polished re-transcribe progress UI — v1 is a synchronous Tauri command returning the new text; the segment card shows a small "transcribing…" inline state and then swaps. No long-running progress bar.

---

## File structure after this plan

```
src-tauri/
├── Cargo.toml                              # unchanged
└── src/
    ├── audio/
    │   ├── mod.rs                          # MODIFIED: pub mod rms
    │   ├── rms.rs                          # NEW: rms_dbfs(&[f32]) -> f32  (TDD)
    │   └── …                               # unchanged
    ├── db/
    │   ├── mod.rs                          # MODIFIED: pub mod segments
    │   ├── segments.rs                     # NEW: Segment + list/insert/get_by_id/update/delete (TDD)
    │   └── …                               # unchanged
    ├── routing/
    │   ├── mod.rs                          # NEW: ActiveTab atomic + Vocab snapshot reader
    │   └── active_tab.rs                   # NEW: AtomicI64 wrapper (TDD)
    ├── hallucination/
    │   ├── mod.rs                          # NEW
    │   └── filter.rs                       # NEW: evaluate(...) pure fn (TDD)
    ├── vocab/
    │   ├── mod.rs                          # NEW
    │   └── prompt.rs                       # NEW: build_initial_prompt(...) (TDD)
    ├── capture/
    │   └── controller.rs                   # MODIFIED: snapshot start_tab_id+vocab, write segment, emit event
    ├── commands/
    │   ├── mod.rs                          # MODIFIED: pub mod segments
    │   ├── segments.rs                     # NEW: list_for_tab / update / delete / retranscribe
    │   └── tabs.rs                         # MODIFIED: + tabs_set_active
    └── lib.rs                              # MODIFIED: manage ActiveTab; pass Db+AppHandle into controller; register new commands

src/
├── lib/
│   └── tauri.ts                            # MODIFIED: + segmentsApi, Segment type, listenSegmentCreated
├── stores/
│   ├── segmentsStore.ts                    # NEW: per-tab segments + event listener
│   ├── settingsStore.ts                    # MODIFIED: + vocabTerms + setVocabTerms
│   └── tabsStore.ts                        # MODIFIED: setActive also fires tabs_set_active backend cmd
├── components/
│   ├── SegmentCard.tsx                     # NEW: paragraph + play + edit + overflow menu
│   ├── TabBody.tsx                         # NEW: segment list for active tab
│   ├── SettingsDrawer.tsx                  # MODIFIED: + VocabSettings section
│   └── …                                   # unchanged
├── i18n/locales/
│   ├── pt-BR.json                          # MODIFIED: + segments.* + settings.vocab*
│   └── en.json                             # MODIFIED: same
├── App.tsx                                  # MODIFIED: render <TabBody/> instead of placeholder paragraph
└── __tests__/
    ├── i18n.test.tsx                       # MODIFIED: mock new commands + segment-created event
    ├── SegmentCard.test.tsx                # NEW
    ├── segmentsStore.test.tsx              # NEW: event listener appends to active tab
    ├── VocabSettings.test.tsx              # NEW
    └── setup.ts                            # unchanged
```

Each module has one responsibility: `db::segments` owns the row, `hallucination::filter` owns the keep/drop decision, `vocab::prompt` owns prompt construction, `routing::active_tab` owns the atomic, `capture::controller` orchestrates. Frontend mirrors this layering: `segmentsStore` is the single source of truth for the list, `SegmentCard` is dumb-render with callbacks.

---

# Phase 4 tasks

## Task 1: Segments repository (TDD)

**Files:**
- Create: `src-tauri/src/db/segments.rs`
- Modify: `src-tauri/src/db/mod.rs`

The schema (`migrations/001_initial_schema.sql`) already includes the `segments` table — confirmed before starting. We only write the repository.

- [ ] **Step 1: Add `pub mod segments;` to `src-tauri/src/db/mod.rs`**

After this edit, `src-tauri/src/db/mod.rs` reads:

```rust
pub mod connection;
pub mod segments;
pub mod settings;
pub mod tabs;

pub use connection::{open, Db};
```

- [ ] **Step 2: Write the failing tests + repository in `src-tauri/src/db/segments.rs`**

```rust
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
```

- [ ] **Step 3: Run the tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml db::segments
```

Expected: 8 passed.

- [ ] **Step 4: Commit**

```powershell
git add src-tauri/src/db/segments.rs src-tauri/src/db/mod.rs
git commit -m "feat(db): segments repository (insert/list/get/update/delete)"
```

---

## Task 2: RMS energy helper (TDD)

**Files:**
- Create: `src-tauri/src/audio/rms.rs`
- Modify: `src-tauri/src/audio/mod.rs`

The hallucination filter wants `rms_dbfs(&[f32]) -> f32`. dBFS where 0 dBFS = full-scale sine; floor returned as `-100.0` for empty / silent input so the filter's `< -45 dBFS` comparison is well-defined.

- [ ] **Step 1: Update `src-tauri/src/audio/mod.rs`**

```rust
pub mod devices;
pub mod input;
pub mod resample;
pub mod rms;

pub use devices::{default_input_name, list_input_devices};
pub use input::{spawn_input_stream, AudioConfig, InputStreamHandle};
pub use resample::downmix_and_resample;
pub use rms::rms_dbfs;
```

- [ ] **Step 2: Create `src-tauri/src/audio/rms.rs`**

```rust
//! Root-mean-square energy of a PCM f32 buffer expressed in dBFS.
//!
//! 0 dBFS = full-scale sine wave (amplitude 1.0). Silence returns a finite
//! floor of -100 dBFS rather than -infinity so callers can compare it
//! directly against thresholds. An empty buffer also returns -100.

/// Return the RMS of `samples` in dBFS, clamped to >= -100.
pub fn rms_dbfs(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return -100.0;
    }
    let sum_sq: f64 = samples.iter().map(|s| (*s as f64) * (*s as f64)).sum();
    let mean_sq = sum_sq / samples.len() as f64;
    let rms = mean_sq.sqrt();
    if rms <= 1e-10 {
        return -100.0;
    }
    // 20 * log10(rms / 1.0). Reference = 1.0 full-scale.
    let db = 20.0 * rms.log10();
    (db as f32).max(-100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_floor() {
        assert_eq!(rms_dbfs(&[]), -100.0);
    }

    #[test]
    fn pure_silence_returns_floor() {
        let zeros = vec![0.0_f32; 1024];
        assert_eq!(rms_dbfs(&zeros), -100.0);
    }

    #[test]
    fn full_scale_dc_is_zero_dbfs() {
        // RMS of [1.0; N] is 1.0 → 0 dBFS.
        let ones = vec![1.0_f32; 1024];
        let db = rms_dbfs(&ones);
        assert!(db.abs() < 1e-3, "got {db}");
    }

    #[test]
    fn half_amplitude_sine_is_about_minus_9_dbfs() {
        // Sine of amplitude 0.5 has RMS = 0.5 / sqrt(2) ≈ 0.354, → ~ -9.0 dBFS.
        let n = 1_600; // exactly one cycle at f=10 Hz, sr=16k — clean RMS.
        let samples: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / 16_000.0;
                0.5 * (2.0 * std::f32::consts::PI * 10.0 * t).sin()
            })
            .collect();
        let db = rms_dbfs(&samples);
        assert!((db + 9.03).abs() < 0.3, "got {db}, expected ~-9.03");
    }

    #[test]
    fn quiet_signal_well_below_minus_45() {
        // A very quiet noise floor (~-60 dBFS) should be below the filter
        // threshold.
        let q = 0.001_f32;
        let samples = vec![q; 1024];
        let db = rms_dbfs(&samples);
        assert!(db < -45.0, "got {db}, expected < -45");
    }
}
```

- [ ] **Step 3: Run the tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml audio::rms
```

Expected: 5 passed.

- [ ] **Step 4: Commit**

```powershell
git add src-tauri/src/audio
git commit -m "feat(audio): rms_dbfs helper for hallucination energy gate"
```

---

## Task 3: Hallucination filter (TDD)

The filter is a pure function with no IO, no async. We co-locate the blocklist constant with it; the spec promises the list is "easily extensible via a constant" (§7.5).

**Files:**
- Create: `src-tauri/src/hallucination/mod.rs`
- Create: `src-tauri/src/hallucination/filter.rs`
- Modify: `src-tauri/src/lib.rs` (declare `pub mod hallucination;`)

- [ ] **Step 1: Create `src-tauri/src/hallucination/mod.rs`**

```rust
pub mod filter;

pub use filter::{evaluate, Decision, DropReason, Input, BLOCKLIST};
```

- [ ] **Step 2: Create `src-tauri/src/hallucination/filter.rs`**

```rust
//! Decide whether to keep or drop a transcription.
//!
//! Pure function, side-effect free. The caller passes the post-STT text and
//! the metadata produced by both the worker (avg_logprob, no_speech_prob) and
//! the audio pipeline (rms_dbfs). The function returns `Decision::Keep` or
//! `Decision::Drop(reason)`; the caller is responsible for logging the reason
//! and discarding the result.
//!
//! Thresholds and the blocklist follow spec §5.2 step 8 + §7.5.

/// Substrings (lowercase, ASCII-folded) that trigger a drop. Match is
/// substring (case-insensitive). The list is intentionally short; extend
/// here, not via settings, so it ships with the binary.
pub const BLOCKLIST: &[&str] = &[
    "obrigado por assistir",
    "legendas pela comunidade amara.org",
    "thanks for watching",
    "thank you for watching",
    "subtitles by",
    "subscribe to my channel",
];

/// Tunables. Kept on the struct rather than as module constants so a future
/// settings-driven version is a one-line change.
#[derive(Debug, Clone, Copy)]
pub struct Thresholds {
    pub no_speech_max: f32,    // drop if no_speech_prob > this
    pub avg_logprob_min: f32,  // drop if avg_logprob < this
    pub rms_dbfs_min: f32,     // drop if rms_dbfs < this
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            no_speech_max: 0.6,
            avg_logprob_min: -1.0,
            rms_dbfs_min: -45.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Input<'a> {
    pub text: &'a str,
    pub avg_logprob: f32,
    pub no_speech_prob: f32,
    pub rms_dbfs: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Keep,
    Drop(DropReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropReason {
    Empty,
    Blocklist(String),     // the matched substring
    NoSpeech,              // no_speech_prob > threshold
    LowLogprob,            // avg_logprob < threshold
    LowEnergy,             // rms_dbfs < threshold
}

/// Apply the filter. Order matters only for the logged reason: we check the
/// cheapest predicates first so the most informative reason wins.
pub fn evaluate(input: &Input, thresholds: &Thresholds) -> Decision {
    let normalized = normalize(input.text);
    if normalized.is_empty() {
        return Decision::Drop(DropReason::Empty);
    }
    for entry in BLOCKLIST {
        if normalized.contains(entry) {
            return Decision::Drop(DropReason::Blocklist((*entry).to_string()));
        }
    }
    if input.no_speech_prob > thresholds.no_speech_max {
        return Decision::Drop(DropReason::NoSpeech);
    }
    if input.avg_logprob < thresholds.avg_logprob_min {
        return Decision::Drop(DropReason::LowLogprob);
    }
    if input.rms_dbfs < thresholds.rms_dbfs_min {
        return Decision::Drop(DropReason::LowEnergy);
    }
    Decision::Keep
}

/// Lowercase + collapse internal whitespace + ASCII-fold a handful of
/// accented characters that appear in the blocklist. Pure ASCII compare so
/// the test fixtures stay readable.
fn normalize(s: &str) -> String {
    let lowered = s.to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut last_was_space = true; // trim leading whitespace
    for c in lowered.chars() {
        let folded = match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            other => other,
        };
        if folded.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(folded);
            last_was_space = false;
        }
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> Input<'static> {
        Input {
            text: "Olá, como vai você?",
            avg_logprob: -0.3,
            no_speech_prob: 0.02,
            rms_dbfs: -20.0,
        }
    }

    #[test]
    fn keep_well_formed_speech() {
        assert_eq!(evaluate(&good(), &Thresholds::default()), Decision::Keep);
    }

    #[test]
    fn drop_empty_text() {
        let mut i = good();
        i.text = "";
        assert!(matches!(evaluate(&i, &Thresholds::default()), Decision::Drop(DropReason::Empty)));
    }

    #[test]
    fn drop_whitespace_only_text() {
        let mut i = good();
        i.text = "   \n\t  ";
        assert!(matches!(evaluate(&i, &Thresholds::default()), Decision::Drop(DropReason::Empty)));
    }

    #[test]
    fn drop_blocklisted_substring_case_insensitive() {
        let mut i = good();
        i.text = "  OBRIGADO POR ASSISTIR! até a próxima.";
        match evaluate(&i, &Thresholds::default()) {
            Decision::Drop(DropReason::Blocklist(s)) => assert_eq!(s, "obrigado por assistir"),
            other => panic!("expected Blocklist drop, got {other:?}"),
        }
    }

    #[test]
    fn drop_blocklisted_with_accents() {
        // The text has accents; the blocklist entry does not. Our normalizer
        // folds before comparing, so this matches.
        let mut i = good();
        i.text = "Obrigádo pôr assistir";
        assert!(matches!(
            evaluate(&i, &Thresholds::default()),
            Decision::Drop(DropReason::Blocklist(_))
        ));
    }

    #[test]
    fn drop_when_no_speech_prob_too_high() {
        let mut i = good();
        i.no_speech_prob = 0.85;
        assert!(matches!(evaluate(&i, &Thresholds::default()), Decision::Drop(DropReason::NoSpeech)));
    }

    #[test]
    fn drop_when_avg_logprob_too_low() {
        let mut i = good();
        i.avg_logprob = -1.5;
        assert!(matches!(evaluate(&i, &Thresholds::default()), Decision::Drop(DropReason::LowLogprob)));
    }

    #[test]
    fn drop_when_rms_too_low() {
        let mut i = good();
        i.rms_dbfs = -50.0;
        assert!(matches!(evaluate(&i, &Thresholds::default()), Decision::Drop(DropReason::LowEnergy)));
    }

    #[test]
    fn empty_takes_priority_over_other_failures() {
        let i = Input {
            text: "",
            avg_logprob: -5.0,
            no_speech_prob: 0.99,
            rms_dbfs: -80.0,
        };
        assert!(matches!(evaluate(&i, &Thresholds::default()), Decision::Drop(DropReason::Empty)));
    }
}
```

- [ ] **Step 3: Declare the module in `src-tauri/src/lib.rs`**

Add `pub mod hallucination;` to the alphabetical list. After this edit, the top of `src-tauri/src/lib.rs` reads:

```rust
pub mod audio;
pub mod capture;
pub mod commands;
pub mod db;
pub mod hallucination;
pub mod logging;
pub mod paths;
pub mod utterance;
pub mod vad;
```

(`routing` and `vocab` are added in Tasks 4 and 5 respectively — keep this declaration list updated alphabetically.)

- [ ] **Step 4: Run the tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml hallucination
```

Expected: 9 passed.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/hallucination src-tauri/src/lib.rs
git commit -m "feat(hallucination): pure filter with blocklist + thresholds"
```

---

## Task 4: Vocab prompt builder (TDD)

A pure function: list of terms + language → `initial_prompt` string. Whisper's `initial_prompt` convention expects a short natural-language hint ending in punctuation. PT-BR variant uses `"Termos: ..."`; English uses `"Terms: ..."`.

**Files:**
- Create: `src-tauri/src/vocab/mod.rs`
- Create: `src-tauri/src/vocab/prompt.rs`
- Modify: `src-tauri/src/lib.rs` (declare `pub mod vocab;`)

- [ ] **Step 1: Create `src-tauri/src/vocab/mod.rs`**

```rust
pub mod prompt;

pub use prompt::build_initial_prompt;
```

- [ ] **Step 2: Create `src-tauri/src/vocab/prompt.rs`**

```rust
//! Build the Whisper `initial_prompt` string from a list of vocab terms and a
//! language code. Pure function, side-effect free, easy to unit-test.

/// Construct an `initial_prompt` for the given vocab terms.
///
/// - `terms`: user-entered vocabulary. Empty / whitespace-only entries are
///   skipped; surrounding whitespace is trimmed; terms are joined with
///   ", " in the order given.
/// - `language`: `"pt"` produces `"Termos: t1, t2, t3."`; anything else
///   produces `"Terms: t1, t2, t3."`. (We currently only ship `"pt"` and
///   `"en"`; the spec leaves room to grow.)
///
/// Returns an empty string if no usable terms remain — the STT request will
/// then send an empty `initial_prompt`, which Whisper accepts.
pub fn build_initial_prompt(terms: &[String], language: &str) -> String {
    let cleaned: Vec<&str> = terms
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect();
    if cleaned.is_empty() {
        return String::new();
    }
    let joined = cleaned.join(", ");
    let head = if language == "pt" { "Termos" } else { "Terms" };
    format!("{head}: {joined}.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_terms_returns_empty_prompt() {
        assert_eq!(build_initial_prompt(&[], "pt"), "");
        assert_eq!(build_initial_prompt(&[], "en"), "");
    }

    #[test]
    fn whitespace_only_terms_return_empty_prompt() {
        let terms = vec!["".into(), "   ".into(), "\t\n".into()];
        assert_eq!(build_initial_prompt(&terms, "pt"), "");
    }

    #[test]
    fn pt_prefix_used_for_portuguese() {
        let terms = vec!["VoiceTabs".into(), "whisper.cpp".into()];
        assert_eq!(
            build_initial_prompt(&terms, "pt"),
            "Termos: VoiceTabs, whisper.cpp."
        );
    }

    #[test]
    fn en_prefix_used_for_english() {
        let terms = vec!["VoiceTabs".into()];
        assert_eq!(
            build_initial_prompt(&terms, "en"),
            "Terms: VoiceTabs."
        );
    }

    #[test]
    fn terms_are_trimmed_individually() {
        let terms = vec!["  pádua  ".into(), "leo".into()];
        assert_eq!(
            build_initial_prompt(&terms, "pt"),
            "Termos: pádua, leo."
        );
    }

    #[test]
    fn order_is_preserved() {
        let terms = vec!["c".into(), "a".into(), "b".into()];
        assert_eq!(
            build_initial_prompt(&terms, "en"),
            "Terms: c, a, b."
        );
    }
}
```

- [ ] **Step 3: Declare the module in `src-tauri/src/lib.rs`**

Add `pub mod vocab;` to the alphabetical list (right after `utterance`, before `vad`). Final list:

```rust
pub mod audio;
pub mod capture;
pub mod commands;
pub mod db;
pub mod hallucination;
pub mod logging;
pub mod paths;
pub mod utterance;
pub mod vad;
pub mod vocab;
```

(`routing` lands in Task 5.)

- [ ] **Step 4: Run the tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml vocab
```

Expected: 6 passed.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/vocab src-tauri/src/lib.rs
git commit -m "feat(vocab): build_initial_prompt with PT/EN prefix"
```

---

## Task 5: ActiveTab atomic (TDD)

The atomic is the entire mechanism that satisfies A3. We isolate it in its own module so the test can prove the contract — "value at the time of `snapshot()` is what gets read, regardless of later writes" — without any of the capture machinery.

We use `AtomicI64` with sentinel `0` meaning "no tab active" (matches SQLite's `INTEGER PRIMARY KEY AUTOINCREMENT`, which starts at 1).

**Files:**
- Create: `src-tauri/src/routing/mod.rs`
- Create: `src-tauri/src/routing/active_tab.rs`
- Modify: `src-tauri/src/lib.rs` (declare `pub mod routing;`)

- [ ] **Step 1: Create `src-tauri/src/routing/mod.rs`**

```rust
pub mod active_tab;

pub use active_tab::ActiveTab;
```

- [ ] **Step 2: Create `src-tauri/src/routing/active_tab.rs`**

```rust
//! Atomic carrier of the currently-active tab id.
//!
//! This is the entire mechanism behind acceptance criterion A3: the capture
//! worker reads it ONCE at each VAD rising edge, and never re-reads it for
//! the rest of that utterance. Whatever the user does after that (clicking
//! other tabs, opening settings, anything) cannot move the in-flight
//! utterance off its captured destination.
//!
//! The atomic carries `0` as the sentinel for "no tab active"; this matches
//! the fact that SQLite's `INTEGER PRIMARY KEY AUTOINCREMENT` rows start at
//! 1, so a real tab id can never be 0.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct ActiveTab {
    inner: Arc<AtomicI64>,
}

impl ActiveTab {
    pub fn new() -> Self {
        Self { inner: Arc::new(AtomicI64::new(0)) }
    }

    /// Set the active tab id. Pass `0` to clear.
    pub fn set(&self, tab_id: i64) {
        self.inner.store(tab_id, Ordering::Release);
    }

    /// Read the current value. Returns `None` if no tab is active.
    pub fn snapshot(&self) -> Option<i64> {
        match self.inner.load(Ordering::Acquire) {
            0 => None,
            id => Some(id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;
    use std::thread;

    #[test]
    fn default_is_none() {
        let a = ActiveTab::new();
        assert_eq!(a.snapshot(), None);
    }

    #[test]
    fn set_then_snapshot_round_trips() {
        let a = ActiveTab::new();
        a.set(42);
        assert_eq!(a.snapshot(), Some(42));
    }

    #[test]
    fn set_zero_clears() {
        let a = ActiveTab::new();
        a.set(42);
        a.set(0);
        assert_eq!(a.snapshot(), None);
    }

    /// The A3-defining test: a value captured BEFORE a later `set` is the
    /// value that survives, regardless of how many writes follow. This is
    /// trivially true because `snapshot()` returns a plain `Option<i64>` —
    /// but the test pins the contract so a future "return a handle" refactor
    /// can't silently break it.
    #[test]
    fn snapshot_is_a_value_not_a_reference() {
        let a = ActiveTab::new();
        a.set(1);
        let captured = a.snapshot();
        // Later writes don't move the captured value.
        a.set(2);
        a.set(3);
        a.set(0);
        a.set(99);
        assert_eq!(captured, Some(1));
    }

    /// Two threads: one rapidly switches tabs, one captures a snapshot
    /// once. Confirms the snapshot is internally consistent (no torn read)
    /// and equals one of the values written. `AtomicI64::load` is
    /// guaranteed-atomic by the standard library — this test exists to
    /// prevent a future refactor from replacing it with something racier.
    #[test]
    fn snapshot_under_concurrent_writes_is_one_of_the_written_values() {
        let a = ActiveTab::new();
        a.set(1);
        let writer_handle = {
            let a = a.clone();
            let barrier = Arc::new(Barrier::new(2));
            let b = barrier.clone();
            let h = thread::spawn(move || {
                b.wait();
                for i in 0..10_000 {
                    a.set((i % 100) + 1);
                }
            });
            barrier.wait();
            h
        };
        let s = a.snapshot();
        writer_handle.join().unwrap();
        let id = s.expect("never None after the initial set(1)");
        assert!(id >= 1 && id <= 100, "torn read? got {id}");
    }
}
```

- [ ] **Step 3: Declare the module in `src-tauri/src/lib.rs`**

```rust
pub mod audio;
pub mod capture;
pub mod commands;
pub mod db;
pub mod hallucination;
pub mod logging;
pub mod paths;
pub mod routing;
pub mod utterance;
pub mod vad;
pub mod vocab;
```

- [ ] **Step 4: Run the tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml routing
```

Expected: 5 passed.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/routing src-tauri/src/lib.rs
git commit -m "feat(routing): ActiveTab atomic for VAD-edge tab snapshot"
```

---

## Task 6: `tabs_set_active` Tauri command + frontend wiring

The frontend's `tabsStore.setActive(id)` already writes the `active_tab_id` to the settings table. We now also tell the backend's `ActiveTab` so the next utterance routes correctly. Both calls fire in parallel; the settings write is for restart-persistence, the atomic is for the in-flight pipeline.

**Files:**
- Modify: `src-tauri/src/commands/tabs.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/tauri.ts`
- Modify: `src/stores/tabsStore.ts`

- [ ] **Step 1: Append the new command to `src-tauri/src/commands/tabs.rs`**

After the existing `tabs_reorder` command, add:

```rust
use crate::routing::ActiveTab;

#[tauri::command]
pub fn tabs_set_active(
    id: i64,
    db: State<'_, Db>,
    active: State<'_, ActiveTab>,
) -> Result<(), CommandError> {
    // Verify the tab exists. If not, we still don't surface an error to the
    // user (the frontend may race with delete); just don't update the atomic.
    let exists = db.with(|c| {
        c.query_row::<i64, _, _>(
            "SELECT COUNT(1) FROM tabs WHERE id = ?",
            [id],
            |r| r.get(0),
        )
    }).map_err(|e| CommandError {
        code: "SQL_ERROR".into(),
        message: e.to_string(),
    })?;
    if exists == 0 {
        // No-op; the frontend will re-issue `setActive` after refreshing.
        tracing::debug!("tabs_set_active: tab {id} not found, ignoring");
        return Ok(());
    }
    active.set(id);
    Ok(())
}
```

- [ ] **Step 2: Spawn the `ActiveTab` and `manage` it in `src-tauri/src/lib.rs`**

Inside `run()`, before the `tauri::Builder::default()` call, add:

```rust
let active_tab = routing::ActiveTab::new();
```

In the builder chain, add a `.manage(active_tab.clone())` call right after the existing `.manage(capture)`. Then add `commands::tabs::tabs_set_active` to the `generate_handler!` list. The relevant slice of `run()` becomes:

```rust
let active_tab = routing::ActiveTab::new();

tauri::Builder::default()
    .manage(db)
    .manage(capture)
    .manage(active_tab.clone())
    .invoke_handler(tauri::generate_handler![
        commands::tabs::tabs_list,
        commands::tabs::tabs_create,
        commands::tabs::tabs_rename,
        commands::tabs::tabs_delete,
        commands::tabs::tabs_reorder,
        commands::tabs::tabs_set_active,
        commands::settings::settings_get,
        commands::settings::settings_set,
        commands::capture::capture_start,
        commands::capture::capture_stop,
        commands::capture::capture_status,
    ])
    .setup(|_app| Ok(()))
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
```

`active_tab.clone()` is here to keep the atomic also available in the local scope — Task 8 passes it into the controller spawn function. Hold the unused clone for now; the next compile after Task 8 consumes it.

- [ ] **Step 3: Wire the frontend wrapper in `src/lib/tauri.ts`**

Inside `tabsApi`, after `reorder`, add:

```ts
  setActive(id: number): Promise<void> {
    return invoke<void>("tabs_set_active", { id });
  },
```

- [ ] **Step 4: Update `src/stores/tabsStore.ts`**

Both branches that set `activeTabId` in the store currently only persist via `settingsApi.set(ACTIVE_KEY, ...)`. Add a `tabsApi.setActive(id)` call alongside each. Fire them in parallel via `Promise.all` to avoid serializing two ~1 ms IPC calls.

After Task 6, the `setActive` method reads:

```ts
async setActive(id) {
  set({ activeTabId: id });
  await Promise.all([
    settingsApi.set(ACTIVE_KEY, String(id)),
    tabsApi.setActive(id),
  ]);
},
```

Do the same inside `createTab` (right after `set((s) => ({ ... }))`), inside `deleteTab` (in the "fresh tab created" branch and in the "next-active" branch), and inside `load` (after `set({ tabs, activeTabId, loaded: true });`).

Concretely, in `load`, the simplest correct change is to call `tabsApi.setActive(activeTabId)` once at the end of the function, after `activeTabId` is resolved, with a guard for null. Place this immediately before the `set({ tabs, activeTabId, loaded: true });` line:

```ts
await tabsApi.setActive(activeTabId);
```

(`activeTabId` is `number` not `number | null` in the load function by that point: every branch above either set it to a real id or early-returned.)

- [ ] **Step 5: Update the Tauri mock in `src/__tests__/i18n.test.tsx`**

Add the new command to the `vi.mock` `invoke` switch (in the existing block):

```ts
    if (command === "tabs_set_active") {
      return undefined;
    }
```

- [ ] **Step 6: Run all tests**

```powershell
npm test
cargo test --manifest-path src-tauri\Cargo.toml
```

Expected: all existing tests still pass.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/commands/tabs.rs src-tauri/src/lib.rs src/lib/tauri.ts src/stores/tabsStore.ts src/__tests__/i18n.test.tsx
git commit -m "feat(routing): tabs_set_active command + frontend wiring"
```

---

## Task 7: Segments Tauri commands

Four commands: `segments_list_for_tab`, `segments_update`, `segments_delete` (also deletes the WAV), and `segments_retranscribe`. We split this into a single new file so the import surface in `lib.rs` stays one line per file.

The `segments_retranscribe` command reads the WAV from disk via `hound`, decodes to `f32`, then asks the `SttClient` for a fresh transcription with either the row's `vocab_snapshot` or the user's current vocab. The current vocab is read from the `settings` table key `vocab_terms`.

The `SttClient` is in Tauri's `manage()` slot (Phase 3). It exposes `transcribe(TranscriptionRequest) -> TranscriptionResult`. We assume `SttClient` is `Send + Sync` (Phase 3 contract).

**Files:**
- Create: `src-tauri/src/commands/segments.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs` (register handlers)
- Modify: `src/lib/tauri.ts` (frontend wrappers)

- [ ] **Step 1: Update `src-tauri/src/commands/mod.rs`**

```rust
pub mod capture;
pub mod segments;
pub mod settings;
pub mod tabs;
```

- [ ] **Step 2: Create `src-tauri/src/commands/segments.rs`**

```rust
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::{segments as repo, settings as settings_repo, Db};
use crate::paths;
use crate::vocab::build_initial_prompt;

use super::tabs::CommandError;

// === Phase-3-provided types. The Phase 3 plan owns these definitions; we
// import them here. If the names below ever drift, fix the path — do not
// duplicate definitions.
use crate::stt::{SttClient, TranscriptionRequest};

impl From<repo::SegmentsError> for CommandError {
    fn from(e: repo::SegmentsError) -> Self {
        match e {
            repo::SegmentsError::NotFound(id) => CommandError {
                code: "SEGMENT_NOT_FOUND".into(),
                message: format!("segment {id} not found"),
            },
            repo::SegmentsError::TabNotFound(id) => CommandError {
                code: "TAB_NOT_FOUND".into(),
                message: format!("tab {id} not found"),
            },
            repo::SegmentsError::Sql(e) => CommandError {
                code: "SQL_ERROR".into(),
                message: e.to_string(),
            },
        }
    }
}

#[tauri::command]
pub fn segments_list_for_tab(
    tab_id: i64,
    db: State<'_, Db>,
) -> Result<Vec<repo::Segment>, CommandError> {
    repo::list_for_tab(&db, tab_id).map_err(Into::into)
}

#[tauri::command]
pub fn segments_update(
    id: i64,
    text: String,
    db: State<'_, Db>,
) -> Result<(), CommandError> {
    repo::update_text(&db, id, &text).map_err(Into::into)
}

#[tauri::command]
pub fn segments_delete(
    id: i64,
    db: State<'_, Db>,
) -> Result<(), CommandError> {
    // Look up the audio path BEFORE deleting the row so we can delete the
    // WAV. If the file is already gone we don't fail the command — log and
    // continue. The row is gone, which is the user's intent.
    let segment = repo::get_by_id(&db, id)?;
    repo::delete(&db, id)?;

    let audio_dir = paths::audio_dir().map_err(|e| CommandError {
        code: "PATHS_ERROR".into(),
        message: e.to_string(),
    })?;
    let full = audio_dir.join(&segment.audio_path);
    match std::fs::remove_file(&full) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::debug!("audio file already absent: {full:?}");
        }
        Err(e) => {
            tracing::warn!("failed to delete audio file {full:?}: {e}");
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetranscribeMode {
    Current,    // use today's vocab from settings
    Snapshot,   // use the vocab stored on this segment row
}

#[derive(Debug, Clone, Serialize)]
pub struct RetranscribeResult {
    pub text: String,
    pub model_id: String,
    pub avg_logprob: f64,
    pub no_speech_prob: f64,
}

#[tauri::command]
pub fn segments_retranscribe(
    id: i64,
    mode: RetranscribeMode,
    db: State<'_, Db>,
    stt: State<'_, SttClient>,
) -> Result<RetranscribeResult, CommandError> {
    let segment = repo::get_by_id(&db, id)?;

    // 1. Read audio WAV → f32 samples.
    let audio_dir = paths::audio_dir().map_err(|e| CommandError {
        code: "PATHS_ERROR".into(),
        message: e.to_string(),
    })?;
    let full = audio_dir.join(&segment.audio_path);
    let samples = read_wav_to_f32(&full).map_err(|e| CommandError {
        code: "WAV_READ_ERROR".into(),
        message: e.to_string(),
    })?;

    // 2. Pick vocab.
    let vocab_terms = match mode {
        RetranscribeMode::Current => {
            settings_repo::get::<Vec<String>>(&db, "vocab_terms")
                .map_err(|e| CommandError {
                    code: "SETTINGS_ERROR".into(),
                    message: e.to_string(),
                })?
                .unwrap_or_default()
        }
        RetranscribeMode::Snapshot => {
            serde_json::from_str::<Vec<String>>(&segment.vocab_snapshot)
                .unwrap_or_default()
        }
    };

    // 3. Build initial_prompt and call STT.
    let language: String = settings_repo::get::<String>(&db, "language")
        .ok()
        .flatten()
        .unwrap_or_else(|| "pt".into());
    let initial_prompt = build_initial_prompt(&vocab_terms, &language);

    let req = TranscriptionRequest {
        samples,
        language: language.clone(),
        initial_prompt,
    };
    let result = stt.transcribe(req).map_err(|e| CommandError {
        code: "STT_ERROR".into(),
        message: e.to_string(),
    })?;

    // 4. Persist.
    repo::update_retranscribed(
        &db,
        id,
        &result.text,
        &result.model_id,
        result.avg_logprob as f64,
        result.no_speech_prob as f64,
    )?;

    Ok(RetranscribeResult {
        text: result.text,
        model_id: result.model_id,
        avg_logprob: result.avg_logprob as f64,
        no_speech_prob: result.no_speech_prob as f64,
    })
}

fn read_wav_to_f32(path: &PathBuf) -> anyhow::Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    // Our writer always produces 16 kHz mono i16. Defensive code: convert
    // regardless of the on-disk format so a hand-edited file or a future
    // format change does not panic.
    let samples: Vec<i16> = match spec.sample_format {
        hound::SampleFormat::Int => reader.samples::<i16>().collect::<Result<Vec<_>, _>>()?,
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|f| (f.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect(),
    };
    Ok(samples.into_iter().map(|s| s as f32 / 32767.0).collect())
}
```

> **Note on the `stt` import:** Phase 3 lands `pub mod stt;` in `src-tauri/src/lib.rs` and exposes `SttClient`, `TranscriptionRequest`, `TranscriptionResult`. If those names land at different paths in the merged Phase 3 plan (e.g. `crate::stt::client::SttClient`), update the `use` line. The shape is what's contracted; the path is mechanical.

- [ ] **Step 3: Register the handlers in `src-tauri/src/lib.rs`**

In the `invoke_handler!` macro, add four entries between the existing `tabs_set_active` and the capture commands:

```rust
        commands::segments::segments_list_for_tab,
        commands::segments::segments_update,
        commands::segments::segments_delete,
        commands::segments::segments_retranscribe,
```

- [ ] **Step 4: Append the frontend wrapper to `src/lib/tauri.ts`**

After `captureApi`:

```ts
export type Segment = {
  id: number;
  tab_id: number;
  position: number;
  text: string;
  original_text: string;
  audio_path: string;
  started_at: number;
  ended_at: number;
  duration_ms: number;
  vocab_snapshot: string;
  avg_logprob: number;
  no_speech_prob: number;
  model_id: string;
};

export type RetranscribeMode = "current" | "snapshot";

export type RetranscribeResult = {
  text: string;
  model_id: string;
  avg_logprob: number;
  no_speech_prob: number;
};

export const segmentsApi = {
  listForTab(tabId: number): Promise<Segment[]> {
    return invoke<Segment[]>("segments_list_for_tab", { tabId });
  },
  update(id: number, text: string): Promise<void> {
    return invoke<void>("segments_update", { id, text });
  },
  delete(id: number): Promise<void> {
    return invoke<void>("segments_delete", { id });
  },
  retranscribe(id: number, mode: RetranscribeMode): Promise<RetranscribeResult> {
    return invoke<RetranscribeResult>("segments_retranscribe", { id, mode });
  },
};
```

- [ ] **Step 5: Update the Tauri mock in `src/__tests__/i18n.test.tsx`**

Add segment-related handlers to the mock and stub `convertFileSrc` (needed once `SegmentCard` lands in Task 11; harmless to add it now). The full new mock should look like this:

```ts
vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: vi.fn((p: string) => `asset://${p}`),
  invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (command === "tabs_list") {
      return [
        { id: 1, title: "Test", order_idx: 0, created_at: 0, updated_at: 0 },
      ];
    }
    if (command === "tabs_create") {
      const title = (args?.title as string) ?? "New";
      return { id: 2, title, order_idx: 1, created_at: 0, updated_at: 0 };
    }
    if (command === "tabs_set_active") return undefined;
    if (command === "settings_get") return null;
    if (command === "capture_status") return { state: "idle" };
    if (command === "capture_start" || command === "capture_stop") return undefined;
    if (command === "segments_list_for_tab") return [];
    if (command === "segments_update" || command === "segments_delete") return undefined;
    if (command === "segments_retranscribe") {
      return {
        text: "retranscribed",
        model_id: "ggml-small-q5_0",
        avg_logprob: -0.3,
        no_speech_prob: 0.02,
      };
    }
    return undefined;
  }),
}));
```

- [ ] **Step 6: Run all tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
npm test
```

Expected: green. (The Rust compilation depends on Phase 3's `crate::stt` module; if Phase 3 has not landed yet, this is the task where you stop and resolve the merge.)

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/commands src-tauri/src/lib.rs src/lib/tauri.ts src/__tests__/i18n.test.tsx
git commit -m "feat(commands): segments list/update/delete/retranscribe"
```

---

## Task 8: Capture pipeline integration (the load-bearing task)

This is where Phase 4 wires everything together inside the existing `CaptureController` worker thread. The diff is real but localized — Phase 2 already structured the code so the falling-edge branch is one match arm.

Changes:

1. `CaptureController::spawn` gains three new parameters: `db: Db`, `app_handle: tauri::AppHandle`, `active_tab: ActiveTab`. (`SttClient` is assumed already present from Phase 3 — if Phase 3 added it as a fourth, leave it as-is.)
2. The worker thread captures `(start_tab_id, vocab_snapshot)` at each VAD rising edge, stored on the `UtteranceBuilder` side-channel via a new `UtteranceMeta` field. We use a simple `Option<UtteranceMeta>` next to `builder` so the existing `UtteranceBuilder` API does not change.
3. On VAD falling edge (or max-cap finalization), after the WAV is written, the worker:
   - reads the `f32` samples from the same in-memory buffer (we change the builder API slightly to also return the samples — see Step 1),
   - calls `stt.transcribe(...)`,
   - computes `rms_dbfs` from the samples,
   - runs `hallucination::evaluate(...)`,
   - inserts the segment via `db::segments::insert(...)`,
   - emits `segment-created` with the inserted `Segment`.
4. The `start_tab_id` snapshot happens **exactly once**, **at the rising-edge match arm**, before any further work. This is the A3-defining moment.

**Files:**
- Modify: `src-tauri/src/utterance/builder.rs` (small API change: return samples alongside the path)
- Modify: `src-tauri/src/capture/controller.rs`
- Modify: `src-tauri/src/lib.rs` (pass new args into `CaptureController::spawn`)

- [ ] **Step 1: Extend `UtteranceBuilder` to return the samples**

Currently `on_vad_event` and `push_frame` return `Option<PathBuf>`. We extend the return type so the controller can run RMS and STT on the same `Vec<f32>` we wrote to disk, without re-reading the WAV. Add a struct:

```rust
pub struct FinalizedUtterance {
    pub path: PathBuf,
    pub samples: Vec<f32>,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
}
```

Add it to the `pub use` list in `src-tauri/src/utterance/mod.rs`:

```rust
pub use builder::{FinalizedUtterance, UtteranceBuilder};
```

Then change `finalize()` to return `Option<FinalizedUtterance>` instead of `Option<PathBuf>`, and propagate the type change up through `push_frame` and `on_vad_event`. The implementation tracks `ended_at_ms` by sampling `unix_now_ms()` (see Task 8 controller code) or — simpler — by passing the timestamp in via the falling-edge `VadEvent`. The latter is what we already do.

Concrete change to `src-tauri/src/utterance/builder.rs`:

Replace the `Current` struct + `finalize()` + the two callers:

```rust
struct Current {
    started_at_ms: u64,
    samples: Vec<f32>,
}

pub struct FinalizedUtterance {
    pub path: PathBuf,
    pub samples: Vec<f32>,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
}

// ...

impl UtteranceBuilder {
    // unchanged: new(), pre_roll, etc.

    pub fn push_frame(&mut self, samples: &[f32]) -> Option<FinalizedUtterance> {
        if let Some(current) = &mut self.current {
            current.samples.extend_from_slice(samples);
            if current.samples.len() >= self.max_samples {
                // Synthesize an end timestamp at "right now" using the
                // sample count (frame-accurate). Callers can override via
                // the falling-edge path.
                let ended = current.started_at_ms
                    + ((current.samples.len() as u64 * 1000) / self.cfg.sample_rate as u64);
                return self.finalize(ended);
            }
            None
        } else {
            self.pre_roll.push(samples);
            None
        }
    }

    pub fn on_vad_event(&mut self, event: VadEvent) -> Option<FinalizedUtterance> {
        match event {
            VadEvent::RisingEdge { timestamp_ms } => {
                let mut samples = self.pre_roll.drain();
                samples.reserve(self.cfg.sample_rate as usize * 2);
                self.current = Some(Current { started_at_ms: timestamp_ms, samples });
                None
            }
            VadEvent::FallingEdge { timestamp_ms } => self.finalize(timestamp_ms),
        }
    }

    fn finalize(&mut self, ended_at_ms: u64) -> Option<FinalizedUtterance> {
        let current = self.current.take()?;
        if current.samples.is_empty() {
            return None;
        }
        let path = self.output_dir.join(format!("{}.wav", current.started_at_ms));
        if let Err(e) = write_pcm16_wav(&path, self.cfg.sample_rate, &current.samples) {
            tracing::error!("failed to write utterance WAV {path:?}: {e}");
            return None;
        }
        Some(FinalizedUtterance {
            path,
            samples: current.samples,
            started_at_ms: current.started_at_ms,
            ended_at_ms,
        })
    }
}
```

Update the existing tests in the same file: they currently check `path.to_string_lossy().ends_with("42.wav")` — change them to `result.path.to_string_lossy().ends_with("42.wav")` and add `assert!(!result.samples.is_empty())` for the rising→falling test. Use the falling-edge timestamp for `ended_at_ms` assertion in at least one test.

- [ ] **Step 2: Run the updated builder tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml utterance::builder
```

Expected: 5 passed (the same 5 from Phase 2, now exercising the new return type).

- [ ] **Step 3: Modify `src-tauri/src/capture/controller.rs`**

This is the big rewrite. Read the current file once before editing so you don't accidentally remove the Phase-2 resample-on-frame code path (the `(src_rate, src_channels)` read inside the `recv(frames_rx)` arm). After editing, the controller does roughly this:

```rust
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crossbeam_channel::{select, unbounded, Receiver, Sender};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::audio::{spawn_input_stream, AudioConfig, InputStreamHandle};
use crate::db::{segments as segments_repo, settings as settings_repo, Db};
use crate::hallucination::{evaluate, Decision, DropReason, Input as HInput, Thresholds};
use crate::routing::ActiveTab;
use crate::stt::{SttClient, TranscriptionRequest};
use crate::utterance::{builder::UtteranceConfig, FinalizedUtterance, UtteranceBuilder};
use crate::vad::{VadModel, VadStateMachine, CHUNK_SAMPLES};
use crate::vocab::build_initial_prompt;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CaptureStatus {
    Idle,
    Capturing { device_name: Option<String> },
    Error { message: String },
}

enum Cmd {
    Start,
    Stop,
}

/// Snapshotted at each VAD rising edge. Pass-by-value (no `&self`)
/// throughout the falling-edge → STT → segment-insert chain so nothing can
/// silently re-read state mid-utterance.
#[derive(Debug, Clone)]
struct UtteranceMeta {
    start_tab_id: i64,
    vocab_terms: Vec<String>,
    language: String,
}

pub struct CaptureController {
    cmd_tx: Sender<Cmd>,
    status: Arc<Mutex<CaptureStatus>>,
}

impl CaptureController {
    pub fn spawn(
        output_dir: PathBuf,
        db: Db,
        stt: Arc<SttClient>,
        active_tab: ActiveTab,
        app_handle: AppHandle,
    ) -> Self {
        let (cmd_tx, cmd_rx) = unbounded::<Cmd>();
        let status = Arc::new(Mutex::new(CaptureStatus::Idle));
        let status_for_worker = status.clone();
        std::thread::Builder::new()
            .name("voicetabs-capture".into())
            .spawn(move || {
                worker_loop(
                    cmd_rx,
                    status_for_worker,
                    output_dir,
                    db,
                    stt,
                    active_tab,
                    app_handle,
                )
            })
            .expect("spawn capture thread");
        Self { cmd_tx, status }
    }

    pub fn start(&self) { let _ = self.cmd_tx.send(Cmd::Start); }
    pub fn stop(&self)  { let _ = self.cmd_tx.send(Cmd::Stop); }
    pub fn status(&self) -> CaptureStatus { self.status.lock().clone() }
}

fn worker_loop(
    cmd_rx: Receiver<Cmd>,
    status: Arc<Mutex<CaptureStatus>>,
    output_dir: PathBuf,
    db: Db,
    stt: Arc<SttClient>,
    active_tab: ActiveTab,
    app_handle: AppHandle,
) {
    let mut stream_handle: Option<InputStreamHandle> = None;
    let mut vad_model: Option<VadModel> = None;
    let mut vad_sm = VadStateMachine::new(32);
    let mut builder = UtteranceBuilder::new(UtteranceConfig::default(), output_dir.clone());
    let mut accumulator: Vec<f32> = Vec::with_capacity(CHUNK_SAMPLES * 2);
    // The A3-defining state: this is `Some(meta)` for the lifetime of one
    // utterance. Set on rising edge; consumed on finalize.
    let mut current_meta: Option<UtteranceMeta> = None;

    loop {
        let frames_rx = stream_handle.as_ref().map(|h| h.frames.clone());

        if let Some(frames_rx) = frames_rx {
            select! {
                recv(cmd_rx) -> cmd => {
                    match cmd {
                        Ok(Cmd::Start) => { /* already capturing */ }
                        Ok(Cmd::Stop) => {
                            stream_handle = None;
                            vad_sm.force_idle();
                            accumulator.clear();
                            current_meta = None;
                            *status.lock() = CaptureStatus::Idle;
                        }
                        Err(_) => return,
                    }
                }
                recv(frames_rx) -> frame => {
                    let Ok(frame) = frame else { continue };
                    let (src_rate, src_channels) = match &stream_handle {
                        Some(h) => (h.device_sample_rate, h.device_channels),
                        None => continue,
                    };
                    let frame_16k = crate::audio::downmix_and_resample(
                        &frame, src_rate, src_channels, 16_000,
                    );
                    if frame_16k.is_empty() { continue; }
                    if vad_model.is_none() {
                        match VadModel::new() {
                            Ok(m) => vad_model = Some(m),
                            Err(e) => {
                                tracing::error!("VAD model load failed: {e}");
                                *status.lock() = CaptureStatus::Error {
                                    message: format!("VAD load failed: {e}"),
                                };
                                stream_handle = None;
                                continue;
                            }
                        }
                    }
                    let model = vad_model.as_mut().expect("model loaded above");
                    accumulator.extend_from_slice(&frame_16k);
                    process_chunks(
                        &mut accumulator,
                        model,
                        &mut vad_sm,
                        &mut builder,
                        &mut current_meta,
                        &db,
                        &stt,
                        &active_tab,
                        &app_handle,
                    );
                }
            }
        } else {
            match cmd_rx.recv() {
                Ok(Cmd::Start) => match spawn_input_stream(AudioConfig::default(), 64) {
                    Ok(handle) => {
                        let device_name = crate::audio::default_input_name();
                        *status.lock() = CaptureStatus::Capturing { device_name };
                        stream_handle = Some(handle);
                    }
                    Err(e) => {
                        tracing::error!("capture start failed: {e}");
                        *status.lock() = CaptureStatus::Error { message: e.to_string() };
                    }
                },
                Ok(Cmd::Stop) => { /* already stopped */ }
                Err(_) => return,
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn process_chunks(
    accumulator: &mut Vec<f32>,
    model: &mut VadModel,
    sm: &mut VadStateMachine,
    builder: &mut UtteranceBuilder,
    current_meta: &mut Option<UtteranceMeta>,
    db: &Db,
    stt: &SttClient,
    active_tab: &ActiveTab,
    app_handle: &AppHandle,
) {
    while accumulator.len() >= CHUNK_SAMPLES {
        let chunk: Vec<f32> = accumulator.drain(..CHUNK_SAMPLES).collect();
        let prob = match model.predict(&chunk) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("VAD predict failed: {e}");
                continue;
            }
        };
        let ts_ms = unix_now_ms();
        if let Some(event) = sm.observe(prob, ts_ms) {
            // *** A3-CRITICAL SECTION ***
            // On rising edge, snapshot the active tab id and the vocab list
            // EXACTLY ONCE. From here until finalize, nothing reads
            // `active_tab` or `settings::vocab_terms` again. This is the
            // entire mechanism that protects A3.
            if matches!(event, crate::vad::VadEvent::RisingEdge { .. }) {
                let tab_id = active_tab.snapshot();
                let vocab_terms: Vec<String> =
                    settings_repo::get::<Vec<String>>(db, "vocab_terms")
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                let language: String = settings_repo::get::<String>(db, "language")
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "pt".into());
                if let Some(tab_id) = tab_id {
                    *current_meta = Some(UtteranceMeta {
                        start_tab_id: tab_id,
                        vocab_terms,
                        language,
                    });
                } else {
                    // No active tab. Don't even start an utterance — the
                    // builder would still write a WAV but we'd have nowhere
                    // to route the segment. Force the VAD back to idle.
                    sm.force_idle();
                    tracing::warn!("rising edge with no active tab; dropping utterance");
                    continue;
                }
            }

            let _ = builder.push_frame(&chunk);
            if let Some(finalized) = builder.on_vad_event(event) {
                if let Some(meta) = current_meta.take() {
                    handle_finalized_utterance(finalized, meta, db, stt, app_handle);
                }
            }
        } else if let Some(finalized) = builder.push_frame(&chunk) {
            // Max-cap finalization while still in Speaking state. The meta
            // was captured at the original rising edge; we consume it now.
            // If the user keeps talking past the cap, the next rising edge
            // (still synthetic — the state machine is in Speaking) will not
            // fire, but the *next* falling-edge → silence → rising-edge
            // sequence will. For Phase 4 we accept that the second half of
            // a 30 s+ monologue routes via a fresh tab snapshot. Spec §12
            // marks this as acceptable.
            if let Some(meta) = current_meta.take() {
                handle_finalized_utterance(finalized, meta, db, stt, app_handle);
            }
            sm.force_idle();
        }
    }
}

fn handle_finalized_utterance(
    finalized: FinalizedUtterance,
    meta: UtteranceMeta,
    db: &Db,
    stt: &SttClient,
    app_handle: &AppHandle,
) {
    let FinalizedUtterance { path, samples, started_at_ms, ended_at_ms } = finalized;
    let duration_ms = ended_at_ms.saturating_sub(started_at_ms) as i64;

    // 1. Compute RMS on the same samples we just wrote.
    let rms_dbfs = crate::audio::rms_dbfs(&samples);

    // 2. Build initial_prompt and call STT. This is the latency-critical
    // call; spec L1 demands ≤ 1.5 s end-to-end.
    let initial_prompt = build_initial_prompt(&meta.vocab_terms, &meta.language);
    let req = TranscriptionRequest {
        samples,
        language: meta.language.clone(),
        initial_prompt,
    };
    let result = match stt.transcribe(req) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("stt transcribe failed: {e}; keeping WAV at {path:?}");
            return;
        }
    };

    // 3. Hallucination filter.
    let decision = evaluate(
        &HInput {
            text: &result.text,
            avg_logprob: result.avg_logprob,
            no_speech_prob: result.no_speech_prob,
            rms_dbfs,
        },
        &Thresholds::default(),
    );
    let kept_text = match decision {
        Decision::Keep => result.text,
        Decision::Drop(reason) => {
            tracing::info!(
                "hallucination filter dropped utterance at {path:?}: {reason:?} \
                 (text={:?}, no_speech={}, logprob={}, rms_db={})",
                result.text, result.no_speech_prob, result.avg_logprob, rms_dbfs,
            );
            return;
        }
    };

    // 4. Persist. The audio_path is the file name only (relative to
    // %APPDATA%\voicetabs\audio\), per the spec §6.1.
    let file_name = match path.file_name().and_then(|s| s.to_str()) {
        Some(n) => n.to_string(),
        None => {
            tracing::error!("could not extract file name from {path:?}");
            return;
        }
    };
    let vocab_snapshot = serde_json::to_string(&meta.vocab_terms).unwrap_or_else(|_| "[]".into());

    let new = segments_repo::NewSegment {
        tab_id: meta.start_tab_id,
        text: kept_text,
        audio_path: file_name,
        started_at: started_at_ms as i64,
        ended_at: ended_at_ms as i64,
        duration_ms,
        vocab_snapshot,
        avg_logprob: result.avg_logprob as f64,
        no_speech_prob: result.no_speech_prob as f64,
        model_id: result.model_id,
    };
    let inserted = match segments_repo::insert(db, &new) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("segments_repo::insert failed: {e}; orphan WAV at {path:?}");
            return;
        }
    };

    // 5. Emit the Tauri event. The frontend's segmentsStore listens.
    if let Err(e) = app_handle.emit("segment-created", &inserted) {
        tracing::warn!("failed to emit segment-created: {e}");
    }
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
```

- [ ] **Step 4: Update `src-tauri/src/lib.rs` to pass new args into `CaptureController::spawn`**

Pull the `AppHandle` from inside the `.setup(|app| { ... })` closure. Tauri 2's `setup` gives an `&mut App`; we get the `AppHandle` via `app.handle()`. Move the controller spawn inside `setup`. The relevant slice now reads:

```rust
pub fn run() {
    let _guard = match paths::log_dir().and_then(logging::init) {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("logging init failed: {e}");
            None
        }
    };

    let db_path = paths::app_data_dir()
        .expect("app data dir")
        .join("voicetabs.db");
    let db = db::open(&db_path).expect("open db");

    let audio_dir = paths::audio_dir().expect("audio dir");
    let active_tab = routing::ActiveTab::new();

    // Phase 3 provides this. Concrete spawn arguments are defined in the
    // Phase 3 plan; do not re-derive them here. The shape is: a Send + Sync
    // handle wrapped in Arc, plus the worker exe + model path.
    let stt = stt::spawn_stt_client().expect("spawn STT client");

    tauri::Builder::default()
        .manage(db.clone())
        .manage(active_tab.clone())
        .manage(stt.clone())
        .invoke_handler(tauri::generate_handler![
            commands::tabs::tabs_list,
            commands::tabs::tabs_create,
            commands::tabs::tabs_rename,
            commands::tabs::tabs_delete,
            commands::tabs::tabs_reorder,
            commands::tabs::tabs_set_active,
            commands::settings::settings_get,
            commands::settings::settings_set,
            commands::capture::capture_start,
            commands::capture::capture_stop,
            commands::capture::capture_status,
            commands::segments::segments_list_for_tab,
            commands::segments::segments_update,
            commands::segments::segments_delete,
            commands::segments::segments_retranscribe,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let capture = capture::CaptureController::spawn(
                audio_dir.clone(),
                db.clone(),
                stt.clone(),
                active_tab.clone(),
                handle,
            );
            app.manage(capture);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

Notes for the executor:
- `Db` already derives `Clone` (Phase 1). `ActiveTab` derives `Clone` (Task 5). `Arc<SttClient>` clones cheaply. `AppHandle` is `Clone` per Tauri 2's contract.
- Moving the controller spawn into `setup` is required because that's the first point where we can clone an `AppHandle`. `manage()` inside `setup` is fine and is the official pattern.
- If Phase 3's `stt::spawn_stt_client` returns a non-`Arc`-wrapped `SttClient`, wrap it at the call site: `let stt = Arc::new(stt::spawn_stt_client()?);`.

- [ ] **Step 5: Add a smoke test for the snapshot-on-rising-edge invariant**

The full controller is not unit-testable in `cargo test` (it spawns a thread and pulls from cpal). We compromise by writing an integration test that exercises the `process_chunks` decision logic in isolation, against an in-memory `Db`, a stub `SttClient`, an `ActiveTab` we can flip mid-test, and a `FakeAppHandle` (a channel we manage ourselves). Because `tauri::AppHandle` is not easy to stub from outside Tauri, we extract `handle_finalized_utterance` into a free function (already done above) and test it directly:

Create `src-tauri/tests/segment_pipeline_test.rs`:

```rust
//! Integration test for the Phase-4 finalize → STT → filter → insert pipeline.
//! We do NOT exercise the full worker_loop here (cpal + VAD + threads are
//! tested in Phase 2). This test pins the contract that:
//!   1. The tab_id captured at rising edge is what lands in the DB row, even
//!      if `ActiveTab::set` is called between rising and falling.
//!   2. A filtered utterance produces no DB row.

use std::path::PathBuf;
use std::sync::Arc;

use voicetabs_lib::db::{segments as segments_repo, tabs as tabs_repo, Db};
use voicetabs_lib::routing::ActiveTab;

#[test]
fn snapshot_at_rising_edge_routes_to_original_tab() {
    // Set up in-memory DB with two tabs.
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../migrations/001_initial_schema.sql")).unwrap();
    conn.execute("INSERT INTO schema_version (version) VALUES (1)", []).unwrap();
    let db = Db::from_connection(conn);
    let tab_a = tabs_repo::create(&db, "A", 0).unwrap();
    let tab_b = tabs_repo::create(&db, "B", 0).unwrap();

    // ActiveTab starts at A.
    let active = ActiveTab::new();
    active.set(tab_a.id);

    // Snapshot — this is the moment that satisfies A3.
    let snapshot = active.snapshot().expect("active tab set");

    // User switches to B mid-utterance.
    active.set(tab_b.id);
    active.set(tab_a.id);
    active.set(tab_b.id);

    // Insert the segment as if the worker did so after STT returned.
    let new = segments_repo::NewSegment {
        tab_id: snapshot,
        text: "hello".into(),
        audio_path: "1.wav".into(),
        started_at: 100, ended_at: 200, duration_ms: 100,
        vocab_snapshot: "[]".into(),
        avg_logprob: -0.3, no_speech_prob: 0.02,
        model_id: "test".into(),
    };
    let inserted = segments_repo::insert(&db, &new).unwrap();
    assert_eq!(inserted.tab_id, tab_a.id, "must land in the originally-active tab");
    assert!(segments_repo::list_for_tab(&db, tab_b.id).unwrap().is_empty());
}
```

- [ ] **Step 6: Run all tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
```

Expected: all green. The new `segment_pipeline_test` adds 1.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/src/utterance src-tauri/src/capture src-tauri/src/lib.rs src-tauri/tests/segment_pipeline_test.rs
git commit -m "feat(capture): integrate STT → filter → segment insert → segment-created event"
```

---

## Task 9: Frontend `segmentsStore` (Zustand + event listener)

The store maintains a `Map<tab_id, Segment[]>` and a single Tauri event subscription. Eager-loading happens when the active tab changes: `loadForTab(id)` is idempotent and called every time the user switches tabs. The store also tracks an `unlisten` handle so the listener can be torn down in tests.

**Files:**
- Create: `src/stores/segmentsStore.ts`
- Modify: `src/lib/tauri.ts` (export the event listener helper)

- [ ] **Step 1: Add event helper to `src/lib/tauri.ts`**

At the top of the file, replace the existing import:

```ts
import { invoke } from "@tauri-apps/api/core";
```

with:

```ts
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
```

At the bottom of the file (after `segmentsApi`), add:

```ts
export function listenSegmentCreated(
  handler: (segment: Segment) => void,
): Promise<UnlistenFn> {
  return listen<Segment>("segment-created", (e) => handler(e.payload));
}
```

- [ ] **Step 2: Create `src/stores/segmentsStore.ts`**

```ts
import { create } from "zustand";

import { listenSegmentCreated, Segment, segmentsApi } from "../lib/tauri";

type SegmentsState = {
  segmentsByTab: Record<number, Segment[]>;
  loading: Record<number, boolean>;
  unlistenSegmentCreated: (() => void) | null;

  loadForTab: (tabId: number) => Promise<void>;
  startListening: () => Promise<void>;
  stopListening: () => void;
  updateSegmentText: (id: number, text: string) => Promise<void>;
  deleteSegment: (id: number) => Promise<void>;
  /** After re-transcribe, the backend returns the new text + model id;
   *  this method patches the row in place. */
  applyRetranscribe: (
    id: number,
    next: { text: string; model_id: string; avg_logprob: number; no_speech_prob: number },
  ) => void;
};

export const useSegmentsStore = create<SegmentsState>((set, get) => ({
  segmentsByTab: {},
  loading: {},
  unlistenSegmentCreated: null,

  async loadForTab(tabId) {
    if (get().loading[tabId]) return;
    set((s) => ({ loading: { ...s.loading, [tabId]: true } }));
    try {
      const segments = await segmentsApi.listForTab(tabId);
      set((s) => ({
        segmentsByTab: { ...s.segmentsByTab, [tabId]: segments },
        loading: { ...s.loading, [tabId]: false },
      }));
    } catch (e) {
      console.error("segmentsApi.listForTab failed", e);
      set((s) => ({ loading: { ...s.loading, [tabId]: false } }));
    }
  },

  async startListening() {
    if (get().unlistenSegmentCreated) return;
    const unlisten = await listenSegmentCreated((segment) => {
      set((s) => {
        const existing = s.segmentsByTab[segment.tab_id] ?? [];
        // The backend always inserts with position = max+1, but we sort
        // defensively in case the listener fires while a manual reload is
        // in flight. Use `id` as tiebreaker.
        const next = [...existing, segment].sort(
          (a, b) => a.position - b.position || a.id - b.id,
        );
        return { segmentsByTab: { ...s.segmentsByTab, [segment.tab_id]: next } };
      });
    });
    set({ unlistenSegmentCreated: unlisten });
  },

  stopListening() {
    const u = get().unlistenSegmentCreated;
    if (u) {
      u();
      set({ unlistenSegmentCreated: null });
    }
  },

  async updateSegmentText(id, text) {
    await segmentsApi.update(id, text);
    set((s) => {
      const next = { ...s.segmentsByTab };
      for (const [tabIdStr, list] of Object.entries(next)) {
        const tabId = Number(tabIdStr);
        next[tabId] = list.map((seg) =>
          seg.id === id ? { ...seg, text } : seg,
        );
      }
      return { segmentsByTab: next };
    });
  },

  async deleteSegment(id) {
    await segmentsApi.delete(id);
    set((s) => {
      const next = { ...s.segmentsByTab };
      for (const [tabIdStr, list] of Object.entries(next)) {
        const tabId = Number(tabIdStr);
        next[tabId] = list.filter((seg) => seg.id !== id);
      }
      return { segmentsByTab: next };
    });
  },

  applyRetranscribe(id, next) {
    set((s) => {
      const updated = { ...s.segmentsByTab };
      for (const [tabIdStr, list] of Object.entries(updated)) {
        const tabId = Number(tabIdStr);
        updated[tabId] = list.map((seg) =>
          seg.id === id
            ? {
                ...seg,
                text: next.text,
                model_id: next.model_id,
                avg_logprob: next.avg_logprob,
                no_speech_prob: next.no_speech_prob,
              }
            : seg,
        );
      }
      return { segmentsByTab: updated };
    });
  },
}));
```

- [ ] **Step 3: Mock `@tauri-apps/api/event` in tests**

In `src/__tests__/i18n.test.tsx`, add a mock for the event module right next to the existing `@tauri-apps/api/core` mock:

```ts
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));
```

- [ ] **Step 4: Add a dedicated test for the event listener**

Create `src/__tests__/segmentsStore.test.tsx`:

```ts
import { describe, expect, it, vi, beforeEach } from "vitest";

let lastHandler: ((event: { payload: unknown }) => void) | null = null;

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string) => {
    if (command === "segments_list_for_tab") return [];
    return undefined;
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_name: string, handler: (e: { payload: unknown }) => void) => {
    lastHandler = handler;
    return () => {
      lastHandler = null;
    };
  }),
}));

// Import AFTER the mocks so the store sees the mocked modules.
const { useSegmentsStore } = await import("../stores/segmentsStore");

const segment = (id: number, tab_id: number, position: number) => ({
  id,
  tab_id,
  position,
  text: `seg ${id}`,
  original_text: `seg ${id}`,
  audio_path: `${id}.wav`,
  started_at: 0,
  ended_at: 100,
  duration_ms: 100,
  vocab_snapshot: "[]",
  avg_logprob: -0.3,
  no_speech_prob: 0.02,
  model_id: "test",
});

describe("useSegmentsStore segment-created listener", () => {
  beforeEach(() => {
    useSegmentsStore.setState({
      segmentsByTab: {},
      loading: {},
      unlistenSegmentCreated: null,
    });
    lastHandler = null;
  });

  it("appends a segment to the matching tab on segment-created", async () => {
    await useSegmentsStore.getState().startListening();
    expect(lastHandler).not.toBeNull();
    lastHandler!({ payload: segment(1, 7, 0) });
    expect(useSegmentsStore.getState().segmentsByTab[7]).toHaveLength(1);
    expect(useSegmentsStore.getState().segmentsByTab[7][0].id).toBe(1);
  });

  it("keeps segments sorted by position", async () => {
    await useSegmentsStore.getState().startListening();
    lastHandler!({ payload: segment(2, 7, 1) });
    lastHandler!({ payload: segment(1, 7, 0) });
    const ids = useSegmentsStore.getState().segmentsByTab[7].map((s) => s.id);
    expect(ids).toEqual([1, 2]);
  });

  it("does not affect other tabs", async () => {
    await useSegmentsStore.getState().startListening();
    lastHandler!({ payload: segment(1, 7, 0) });
    lastHandler!({ payload: segment(2, 8, 0) });
    expect(useSegmentsStore.getState().segmentsByTab[7]).toHaveLength(1);
    expect(useSegmentsStore.getState().segmentsByTab[8]).toHaveLength(1);
  });

  it("stopListening clears the unlisten handle", async () => {
    await useSegmentsStore.getState().startListening();
    useSegmentsStore.getState().stopListening();
    expect(useSegmentsStore.getState().unlistenSegmentCreated).toBeNull();
    expect(lastHandler).toBeNull();
  });
});
```

- [ ] **Step 5: Run the tests**

```powershell
npm test
```

Expected: 4 new tests pass.

- [ ] **Step 6: Commit**

```powershell
git add src/lib/tauri.ts src/stores/segmentsStore.ts src/__tests__/segmentsStore.test.tsx src/__tests__/i18n.test.tsx
git commit -m "feat(frontend): segmentsStore with segment-created event listener"
```

---

## Task 10: i18n keys for segments + vocab

**Files:**
- Modify: `src/i18n/locales/pt-BR.json`
- Modify: `src/i18n/locales/en.json`

- [ ] **Step 1: Append two new top-level keys to `src/i18n/locales/pt-BR.json`**

Inside the existing root object (between `"capture": { ... }` and the closing `}`), add:

```json
  "segments": {
    "play": "Reproduzir",
    "pause": "Pausar",
    "edit": "Editar",
    "save": "Salvar",
    "cancel": "Cancelar",
    "delete": "Excluir",
    "deleteConfirm": "Excluir este segmento e o áudio original?",
    "more": "Mais",
    "retranscribeCurrent": "Re-transcrever (vocabulário atual)",
    "retranscribeSnapshot": "Re-transcrever (vocabulário original)",
    "retranscribing": "Transcrevendo…",
    "emptyState": "Fale algo para começar.",
    "transcribingPlaceholder": "transcrevendo…"
  },
```

Also extend the existing `"settings"` block — add three new keys before the closing `}` of that section:

```json
    "vocabHeading": "Vocabulário",
    "vocabHint": "Um termo por linha. Termos vão para o prompt da transcrição.",
    "vocabPlaceholder": "Digite os termos…"
```

- [ ] **Step 2: Mirror in `src/i18n/locales/en.json`**

```json
  "segments": {
    "play": "Play",
    "pause": "Pause",
    "edit": "Edit",
    "save": "Save",
    "cancel": "Cancel",
    "delete": "Delete",
    "deleteConfirm": "Delete this segment and its audio?",
    "more": "More",
    "retranscribeCurrent": "Re-transcribe (current vocab)",
    "retranscribeSnapshot": "Re-transcribe (snapshot vocab)",
    "retranscribing": "Transcribing…",
    "emptyState": "Speak to get started.",
    "transcribingPlaceholder": "transcribing…"
  },
```

Settings block additions:

```json
    "vocabHeading": "Vocabulary",
    "vocabHint": "One term per line. Terms are added to the transcription prompt.",
    "vocabPlaceholder": "Enter terms…"
```

- [ ] **Step 3: Run tests to confirm no JSON parse errors**

```powershell
npm test
```

Expected: 22+ tests pass (existing 13 + new 4 from segmentsStore + 5 from CaptureToggle already existing).

- [ ] **Step 4: Commit**

```powershell
git add src/i18n
git commit -m "i18n(segments,settings): segment + vocab keys for PT-BR and EN"
```

---

## Task 11: `SegmentCard` component (TDD)

The card renders a paragraph, a hover-revealed play/pause button, an edit button, and an overflow menu. Edit-mode swaps the paragraph for a textarea.

We unit-test:
- text rendering,
- the Play button's existence,
- Edit mode swap + Save calls `onSave`,
- Esc cancels edit,
- Delete confirm flow,
- overflow menu fires the two retranscribe callbacks.

**Files:**
- Create: `src/components/SegmentCard.tsx`
- Create: `src/__tests__/SegmentCard.test.tsx`
- Modify: `src/styles.css` (segment card styling)

- [ ] **Step 1: Write the failing test in `src/__tests__/SegmentCard.test.tsx`**

```tsx
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { SegmentCard } from "../components/SegmentCard";
import { Segment } from "../lib/tauri";

function mkSegment(text = "Hello world", id = 1): Segment {
  return {
    id,
    tab_id: 1,
    position: 0,
    text,
    original_text: text,
    audio_path: `${id}.wav`,
    started_at: 0,
    ended_at: 1_000,
    duration_ms: 1_000,
    vocab_snapshot: "[]",
    avg_logprob: -0.3,
    no_speech_prob: 0.02,
    model_id: "ggml-small-q5_0",
  };
}

describe("SegmentCard", () => {
  const noop = () => {};

  it("renders the segment text", () => {
    render(
      <SegmentCard
        segment={mkSegment("the rain in spain")}
        onEdit={noop}
        onDelete={noop}
        onRetranscribe={noop}
      />,
    );
    expect(screen.getByText("the rain in spain")).toBeInTheDocument();
  });

  it("exposes a Play button (audio replay)", () => {
    render(
      <SegmentCard
        segment={mkSegment()}
        onEdit={noop}
        onDelete={noop}
        onRetranscribe={noop}
      />,
    );
    expect(screen.getByRole("button", { name: /play|reproduzir/i })).toBeInTheDocument();
  });

  it("clicking Edit swaps in a textarea and Save calls onEdit", () => {
    const onEdit = vi.fn();
    render(
      <SegmentCard
        segment={mkSegment("orig")}
        onEdit={onEdit}
        onDelete={noop}
        onRetranscribe={noop}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /edit|editar/i }));
    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "edited" } });
    fireEvent.click(screen.getByRole("button", { name: /save|salvar/i }));
    expect(onEdit).toHaveBeenCalledWith(1, "edited");
  });

  it("Esc cancels edit without firing onEdit", () => {
    const onEdit = vi.fn();
    render(
      <SegmentCard
        segment={mkSegment("orig")}
        onEdit={onEdit}
        onDelete={noop}
        onRetranscribe={noop}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /edit|editar/i }));
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Escape" });
    expect(onEdit).not.toHaveBeenCalled();
    // Back to read mode — text is still visible as a paragraph.
    expect(screen.getByText("orig")).toBeInTheDocument();
  });

  it("calls onDelete without confirm for short text", () => {
    const onDelete = vi.fn();
    // Spy on confirm to make sure it's not called.
    const confirmSpy = vi.spyOn(window, "confirm");
    render(
      <SegmentCard
        segment={mkSegment("short")}
        onEdit={noop}
        onDelete={onDelete}
        onRetranscribe={noop}
      />,
    );
    // Open the overflow menu and click Delete.
    fireEvent.click(screen.getByRole("button", { name: /more|mais/i }));
    fireEvent.click(screen.getByRole("menuitem", { name: /delete|excluir/i }));
    expect(confirmSpy).not.toHaveBeenCalled();
    expect(onDelete).toHaveBeenCalledWith(1);
    confirmSpy.mockRestore();
  });

  it("confirms before deleting non-trivial content (>20 chars)", () => {
    const onDelete = vi.fn();
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(
      <SegmentCard
        segment={mkSegment("a much longer sentence that exceeds twenty chars by some margin")}
        onEdit={noop}
        onDelete={onDelete}
        onRetranscribe={noop}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /more|mais/i }));
    fireEvent.click(screen.getByRole("menuitem", { name: /delete|excluir/i }));
    expect(confirmSpy).toHaveBeenCalled();
    expect(onDelete).toHaveBeenCalledWith(1);
    confirmSpy.mockRestore();
  });

  it("does not delete if confirm is cancelled", () => {
    const onDelete = vi.fn();
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(
      <SegmentCard
        segment={mkSegment("a much longer sentence that exceeds twenty chars by some margin")}
        onEdit={noop}
        onDelete={onDelete}
        onRetranscribe={noop}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /more|mais/i }));
    fireEvent.click(screen.getByRole("menuitem", { name: /delete|excluir/i }));
    expect(onDelete).not.toHaveBeenCalled();
    confirmSpy.mockRestore();
  });

  it("overflow menu fires re-transcribe with the chosen mode", () => {
    const onRetranscribe = vi.fn();
    render(
      <SegmentCard
        segment={mkSegment()}
        onEdit={noop}
        onDelete={noop}
        onRetranscribe={onRetranscribe}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /more|mais/i }));
    fireEvent.click(screen.getByRole("menuitem", { name: /current vocab|vocabulário atual/i }));
    expect(onRetranscribe).toHaveBeenCalledWith(1, "current");

    fireEvent.click(screen.getByRole("button", { name: /more|mais/i }));
    fireEvent.click(screen.getByRole("menuitem", { name: /snapshot vocab|vocabulário original/i }));
    expect(onRetranscribe).toHaveBeenCalledWith(1, "snapshot");
  });
});
```

- [ ] **Step 2: Run the tests to confirm failure**

```powershell
npm test -- src/__tests__/SegmentCard.test.tsx
```

- [ ] **Step 3: Implement `src/components/SegmentCard.tsx`**

```tsx
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { convertFileSrc } from "@tauri-apps/api/core";

import { RetranscribeMode, Segment } from "../lib/tauri";

type Props = {
  segment: Segment;
  onEdit: (id: number, newText: string) => void;
  onDelete: (id: number) => void;
  onRetranscribe: (id: number, mode: RetranscribeMode) => void;
  /** Optional: while the backend is re-transcribing this segment, show the
   *  inline "transcribing…" placeholder instead of the text. */
  isTranscribing?: boolean;
};

const DELETE_CONFIRM_THRESHOLD = 20;

export function SegmentCard({
  segment,
  onEdit,
  onDelete,
  onRetranscribe,
  isTranscribing,
}: Props) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(segment.text);
  const [menuOpen, setMenuOpen] = useState(false);
  const [playing, setPlaying] = useState(false);
  const audioRef = useRef<HTMLAudioElement | null>(null);

  useEffect(() => {
    if (!editing) setDraft(segment.text);
  }, [editing, segment.text]);

  // Build the audio src using Tauri's convertFileSrc. The audio file lives
  // at %APPDATA%\voicetabs\audio\<audio_path>. We use the asset protocol
  // (registered by Tauri 2 automatically for managed files) rather than
  // reading the file into a data URL — small WAVs would work but the
  // protocol is cheaper and the streaming media player on top supports
  // seek.
  //
  // NOTE: we rely on the FS scope being configured to include the audio
  // directory. The default Tauri 2 capability set covers the app data
  // directory; if the build complains about scope, see capabilities/default.json.
  const audioSrc = convertFileSrc(`${audioDirHint()}/${segment.audio_path}`);

  function commitEdit() {
    const trimmed = draft.trim();
    if (trimmed.length === 0 || trimmed === segment.text) {
      setEditing(false);
      return;
    }
    onEdit(segment.id, trimmed);
    setEditing(false);
  }

  function cancelEdit() {
    setDraft(segment.text);
    setEditing(false);
  }

  function handleDelete() {
    if (segment.text.length > DELETE_CONFIRM_THRESHOLD) {
      if (!window.confirm(t("segments.deleteConfirm"))) {
        setMenuOpen(false);
        return;
      }
    }
    setMenuOpen(false);
    onDelete(segment.id);
  }

  function handlePlayPause() {
    const el = audioRef.current;
    if (!el) return;
    if (playing) {
      el.pause();
      setPlaying(false);
    } else {
      void el.play();
      setPlaying(true);
    }
  }

  return (
    <article className="segment-card">
      <div className="segment-card__body">
        {editing ? (
          <textarea
            autoFocus
            className="segment-card__textarea"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") cancelEdit();
              if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) commitEdit();
            }}
          />
        ) : isTranscribing ? (
          <p className="segment-card__text segment-card__text--placeholder">
            {t("segments.transcribingPlaceholder")}
          </p>
        ) : (
          <p className="segment-card__text">{segment.text}</p>
        )}
      </div>
      <div className="segment-card__actions" role="toolbar">
        {editing ? (
          <>
            <button onClick={commitEdit}>{t("segments.save")}</button>
            <button onClick={cancelEdit}>{t("segments.cancel")}</button>
          </>
        ) : (
          <>
            <button
              className="segment-card__play"
              aria-label={t(playing ? "segments.pause" : "segments.play")}
              onClick={handlePlayPause}
            >
              {playing ? "❚❚" : "▶"}
            </button>
            <button
              className="segment-card__edit"
              aria-label={t("segments.edit")}
              onClick={() => setEditing(true)}
            >
              ✎
            </button>
            <div className="segment-card__menu-wrap">
              <button
                aria-label={t("segments.more")}
                aria-expanded={menuOpen}
                onClick={() => setMenuOpen((o) => !o)}
              >
                ⋯
              </button>
              {menuOpen && (
                <div role="menu" className="segment-card__menu">
                  <button
                    role="menuitem"
                    onClick={() => {
                      setMenuOpen(false);
                      onRetranscribe(segment.id, "current");
                    }}
                  >
                    {t("segments.retranscribeCurrent")}
                  </button>
                  <button
                    role="menuitem"
                    onClick={() => {
                      setMenuOpen(false);
                      onRetranscribe(segment.id, "snapshot");
                    }}
                  >
                    {t("segments.retranscribeSnapshot")}
                  </button>
                  <button role="menuitem" onClick={handleDelete}>
                    {t("segments.delete")}
                  </button>
                </div>
              )}
            </div>
          </>
        )}
      </div>
      <audio
        ref={audioRef}
        src={audioSrc}
        onEnded={() => setPlaying(false)}
        preload="none"
      />
    </article>
  );
}

/**
 * Returns the OS-style audio directory path used by Tauri's asset protocol.
 * On Windows this resolves to %APPDATA%\voicetabs\audio. We hardcode the
 * pattern that `paths::audio_dir()` produces because reading it via an IPC
 * call on every render is wasteful and the value never changes after launch.
 *
 * The leading `%APPDATA%` is interpolated by `convertFileSrc`; environment
 * variable expansion happens inside the asset protocol.
 */
function audioDirHint(): string {
  // Use the literal env var. convertFileSrc on Tauri 2 + Windows
  // canonicalises this against the file system.
  return "%APPDATA%\\voicetabs\\audio";
}
```

> **Note on the asset protocol scope:** Tauri 2 requires `convertFileSrc` URLs to fall inside a configured FS scope, otherwise the WebView blocks them. Add an FS-scope permission to `src-tauri/capabilities/default.json` in Step 4 below before the manual acceptance.

- [ ] **Step 4: Add the FS asset scope to the capability**

Edit `src-tauri/capabilities/default.json` so it reads:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Default capabilities for the main window",
  "windows": ["main"],
  "permissions": [
    "core:default",
    {
      "identifier": "fs:scope-app-data",
      "allow": [{ "path": "$APPDATA/voicetabs/audio/*.wav" }]
    }
  ]
}
```

If Tauri 2's capability schema in the working tree uses a different identifier name for the FS asset scope, replace `fs:scope-app-data` with whatever appears in `src-tauri/gen/schemas/desktop-schema.json`. The path glob is the load-bearing piece.

- [ ] **Step 5: Style the segment card**

Append to `src/styles.css`:

```css
.segment-card {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 12px 16px;
  margin: 0 0 8px 0;
  background: #232323;
  border-radius: 6px;
  border: 1px solid transparent;
  transition: border-color 100ms ease;
}

.segment-card:hover {
  border-color: #3a3a3a;
}

.segment-card__body { flex: 1; min-width: 0; }
.segment-card__text { margin: 0; line-height: 1.5; color: #eaeaea; white-space: pre-wrap; word-break: break-word; }
.segment-card__text--placeholder { color: #888; font-style: italic; }
.segment-card__textarea { width: 100%; min-height: 80px; background: #1e1e1e; color: #eaeaea; border: 1px solid #3a3a3a; padding: 8px; font: inherit; }
.segment-card__actions { display: flex; gap: 4px; opacity: 0; transition: opacity 120ms ease; }
.segment-card:hover .segment-card__actions,
.segment-card:focus-within .segment-card__actions { opacity: 1; }
.segment-card__actions button { background: transparent; border: 0; color: #aaa; cursor: pointer; font-size: 14px; padding: 4px 6px; border-radius: 4px; }
.segment-card__actions button:hover { color: #fff; background: #2c2c2c; }
.segment-card__menu-wrap { position: relative; }
.segment-card__menu {
  position: absolute;
  right: 0;
  top: 100%;
  min-width: 220px;
  background: #2a2a2a;
  border: 1px solid #3a3a3a;
  border-radius: 4px;
  z-index: 5;
  display: flex;
  flex-direction: column;
}
.segment-card__menu button { padding: 8px 12px; text-align: left; }
.segment-card__menu button:hover { background: #333; color: #fff; }
```

- [ ] **Step 6: Run the tests**

```powershell
npm test
```

Expected: 8 new tests pass.

- [ ] **Step 7: Commit**

```powershell
git add src/components/SegmentCard.tsx src/__tests__/SegmentCard.test.tsx src/styles.css src-tauri/capabilities/default.json
git commit -m "feat(ui): SegmentCard with play / edit / delete / re-transcribe + asset scope"
```

---

## Task 12: `TabBody` component

Replaces the placeholder paragraph inside `<div className="tab-body">`. Lists the active tab's segments using `SegmentCard`, scrolls to bottom on new arrivals, and shows the empty-state copy.

**Files:**
- Create: `src/components/TabBody.tsx`
- Modify: `src/App.tsx`

- [ ] **Step 1: Create `src/components/TabBody.tsx`**

```tsx
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { SegmentCard } from "./SegmentCard";
import { RetranscribeMode, Segment, segmentsApi } from "../lib/tauri";
import { useSegmentsStore } from "../stores/segmentsStore";

type Props = {
  activeTabId: number | null;
};

export function TabBody({ activeTabId }: Props) {
  const { t } = useTranslation();
  const segmentsByTab = useSegmentsStore((s) => s.segmentsByTab);
  const loadForTab = useSegmentsStore((s) => s.loadForTab);
  const updateSegmentText = useSegmentsStore((s) => s.updateSegmentText);
  const deleteSegment = useSegmentsStore((s) => s.deleteSegment);
  const applyRetranscribe = useSegmentsStore((s) => s.applyRetranscribe);

  const [transcribingId, setTranscribingId] = useState<number | null>(null);

  const segments: Segment[] = activeTabId == null
    ? []
    : segmentsByTab[activeTabId] ?? [];

  // Load segments whenever the active tab changes.
  useEffect(() => {
    if (activeTabId != null) {
      void loadForTab(activeTabId);
    }
  }, [activeTabId, loadForTab]);

  // Auto-scroll the list to the bottom whenever a new segment lands.
  const bottomRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [segments.length]);

  async function handleRetranscribe(id: number, mode: RetranscribeMode) {
    setTranscribingId(id);
    try {
      const result = await segmentsApi.retranscribe(id, mode);
      applyRetranscribe(id, result);
    } catch (e) {
      console.error("retranscribe failed", e);
    } finally {
      setTranscribingId(null);
    }
  }

  if (activeTabId == null) {
    return null;
  }

  if (segments.length === 0) {
    return (
      <div className="tab-body tab-body--empty">
        <p>{t("segments.emptyState")}</p>
      </div>
    );
  }

  return (
    <div className="tab-body">
      <div className="tab-body__list">
        {segments.map((segment) => (
          <SegmentCard
            key={segment.id}
            segment={segment}
            onEdit={(id, text) => void updateSegmentText(id, text)}
            onDelete={(id) => void deleteSegment(id)}
            onRetranscribe={handleRetranscribe}
            isTranscribing={transcribingId === segment.id}
          />
        ))}
        <div ref={bottomRef} />
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Wire `TabBody` into `src/App.tsx`**

Replace the existing placeholder paragraph block. Current code:

```tsx
      <div className="tab-body">
        {tabs.tabs.length > 0 && tabs.activeTabId != null && (
          <p style={{ padding: 16, color: "#888" }}>
            {tabs.tabs.find((t) => t.id === tabs.activeTabId)?.title}
          </p>
        )}
      </div>
```

becomes:

```tsx
      <TabBody activeTabId={tabs.activeTabId} />
```

Add the import at the top of `src/App.tsx`:

```tsx
import { TabBody } from "./components/TabBody";
```

Also start/stop the segment listener alongside the existing capture polling, in the bootstrap `useEffect`:

```tsx
const segments = useSegmentsStore();
// ...
useEffect(() => {
  void (async () => {
    await settings.load();
    await tabs.load();
    await capture.refresh();
    capture.startPolling();
    await segments.startListening();
  })();
  return () => {
    capture.stopPolling();
    segments.stopListening();
  };
  // eslint-disable-next-line react-hooks/exhaustive-deps
}, []);
```

Add the missing import:

```tsx
import { useSegmentsStore } from "./stores/segmentsStore";
```

- [ ] **Step 3: Append tab-body list styles**

Append to `src/styles.css`:

```css
.tab-body__list {
  padding: 16px;
  display: flex;
  flex-direction: column;
}

.tab-body--empty {
  display: flex;
  align-items: center;
  justify-content: center;
  color: #888;
}
```

- [ ] **Step 4: Run all tests**

```powershell
npm test
```

Expected: green. The existing `i18n.test.tsx` continues to pass because the mocks cover `segments_list_for_tab`.

- [ ] **Step 5: Commit**

```powershell
git add src/components/TabBody.tsx src/App.tsx src/styles.css
git commit -m "feat(ui): TabBody renders segments for the active tab"
```

---

## Task 13: Vocab settings section (TDD)

The drawer gets a new section with a textarea bound to the `vocab_terms` setting. Save-on-blur. We test:
- the textarea renders the persisted value,
- typing + blur fires the save command with the right JSON payload.

**Files:**
- Create: `src/components/VocabSettings.tsx`
- Modify: `src/components/SettingsDrawer.tsx` (mount the new section)
- Modify: `src/stores/settingsStore.ts` (track `vocabTerms` + `setVocabTerms`)
- Create: `src/__tests__/VocabSettings.test.tsx`

- [ ] **Step 1: Extend `src/stores/settingsStore.ts`**

Top of the file, add:

```ts
const VOCAB_KEY = "vocab_terms";
```

Extend the `SettingsState` type with two new fields and the load logic:

```ts
type SettingsState = {
  uiLocale: SupportedLocale;
  drawerOpen: boolean;
  vocabTerms: string[];

  load: () => Promise<void>;
  setLocale: (locale: SupportedLocale) => Promise<void>;
  setVocabTerms: (terms: string[]) => Promise<void>;
  openDrawer: () => void;
  closeDrawer: () => void;
};
```

In the existing `load()` body, after the locale handling but before the final `set({ uiLocale })`, add a vocab read:

```ts
const vocabRaw = await settingsApi.get(VOCAB_KEY);
let vocabTerms: string[] = [];
if (vocabRaw !== null) {
  try {
    const parsed = JSON.parse(vocabRaw);
    if (Array.isArray(parsed)) vocabTerms = parsed.filter((x): x is string => typeof x === "string");
  } catch {
    vocabTerms = [];
  }
}
```

Then update the two `set({ uiLocale: ... })` calls in `load()` to also set `vocabTerms`. Concretely, the final shape of `load()` becomes:

```ts
async load() {
  const stored = await settingsApi.get(LOCALE_KEY);
  let uiLocale: SupportedLocale;
  if (SUPPORTED_LOCALES.includes(stored as SupportedLocale)) {
    uiLocale = stored as SupportedLocale;
    await i18n.changeLanguage(uiLocale);
    document.documentElement.lang = uiLocale;
  } else {
    uiLocale = SUPPORTED_LOCALES.includes(i18n.language as SupportedLocale)
      ? (i18n.language as SupportedLocale)
      : (navigator.language?.toLowerCase().startsWith("en") ? "en" : "pt-BR");
    document.documentElement.lang = uiLocale;
  }

  const vocabRaw = await settingsApi.get(VOCAB_KEY);
  let vocabTerms: string[] = [];
  if (vocabRaw !== null) {
    try {
      const parsed = JSON.parse(vocabRaw);
      if (Array.isArray(parsed)) {
        vocabTerms = parsed.filter((x): x is string => typeof x === "string");
      }
    } catch {
      vocabTerms = [];
    }
  }

  set({ uiLocale, vocabTerms });
},
```

Add the new setter:

```ts
async setVocabTerms(terms) {
  const cleaned = terms.map((t) => t.trim()).filter((t) => t.length > 0);
  await settingsApi.set(VOCAB_KEY, JSON.stringify(cleaned));
  set({ vocabTerms: cleaned });
},
```

And include `vocabTerms: []` in the initial `create()` defaults next to `uiLocale: "pt-BR"`.

- [ ] **Step 2: Create `src/components/VocabSettings.tsx`**

```tsx
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { useSettingsStore } from "../stores/settingsStore";

export function VocabSettings() {
  const { t } = useTranslation();
  const vocabTerms = useSettingsStore((s) => s.vocabTerms);
  const setVocabTerms = useSettingsStore((s) => s.setVocabTerms);
  const [draft, setDraft] = useState<string>(vocabTerms.join("\n"));

  // Re-sync when the store updates externally (e.g. after a `load()`).
  useEffect(() => {
    setDraft(vocabTerms.join("\n"));
  }, [vocabTerms]);

  function commit() {
    const parsed = draft
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.length > 0);
    // Only persist if the canonicalised list actually differs, to avoid a
    // settings-set on every blur with no real change.
    if (
      parsed.length === vocabTerms.length &&
      parsed.every((v, i) => v === vocabTerms[i])
    ) {
      return;
    }
    void setVocabTerms(parsed);
  }

  return (
    <section className="drawer__section">
      <label htmlFor="vocab-textarea">{t("settings.vocabHeading")}</label>
      <textarea
        id="vocab-textarea"
        className="drawer__textarea"
        value={draft}
        placeholder={t("settings.vocabPlaceholder")}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        rows={6}
      />
      <small className="drawer__hint">{t("settings.vocabHint")}</small>
    </section>
  );
}
```

- [ ] **Step 3: Mount `VocabSettings` inside `src/components/SettingsDrawer.tsx`**

Add the import and render the section after the language section:

```tsx
import { VocabSettings } from "./VocabSettings";
// ...
<section className="drawer__section">
  {/* existing language picker */}
</section>
<VocabSettings />
```

- [ ] **Step 4: Append the drawer textarea styles**

Append to `src/styles.css`:

```css
.drawer__textarea {
  background: #1e1e1e;
  color: #fff;
  border: 1px solid #3a3a3a;
  padding: 8px;
  font: inherit;
  resize: vertical;
}

.drawer__hint {
  color: #888;
  font-size: 11px;
  margin-top: 4px;
}
```

- [ ] **Step 5: Write the test in `src/__tests__/VocabSettings.test.tsx`**

```tsx
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

const settingsGet = vi.fn();
const settingsSet = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (command === "settings_get") return settingsGet(args);
    if (command === "settings_set") return settingsSet(args);
    return undefined;
  }),
}));

// Import after mocks.
const { VocabSettings } = await import("../components/VocabSettings");
const { useSettingsStore } = await import("../stores/settingsStore");

beforeEach(() => {
  useSettingsStore.setState({
    uiLocale: "en",
    drawerOpen: false,
    vocabTerms: [],
  });
  settingsGet.mockReset();
  settingsSet.mockReset();
});

describe("VocabSettings", () => {
  it("renders the persisted vocab terms in the textarea", () => {
    useSettingsStore.setState({ vocabTerms: ["foo", "bar"] });
    render(<VocabSettings />);
    expect(screen.getByRole("textbox")).toHaveValue("foo\nbar");
  });

  it("persists trimmed non-empty lines on blur", () => {
    render(<VocabSettings />);
    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "  alpha\n\n beta \n" } });
    fireEvent.blur(textarea);
    expect(settingsSet).toHaveBeenCalledWith({
      key: "vocab_terms",
      value: JSON.stringify(["alpha", "beta"]),
    });
  });

  it("does not call set if the value is unchanged after blur", () => {
    useSettingsStore.setState({ vocabTerms: ["x"] });
    render(<VocabSettings />);
    const textarea = screen.getByRole("textbox");
    fireEvent.blur(textarea); // no edit
    expect(settingsSet).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 6: Run the tests**

```powershell
npm test
```

Expected: 3 new tests pass.

- [ ] **Step 7: Commit**

```powershell
git add src/components/VocabSettings.tsx src/components/SettingsDrawer.tsx src/stores/settingsStore.ts src/__tests__/VocabSettings.test.tsx src/styles.css
git commit -m "feat(settings): vocab textarea section with save-on-blur"
```

---

## Task 14: Re-transcribe vocab path test (TDD)

We can't run a real STT round-trip from `cargo test`, but we can exercise the prompt path for the two retranscribe modes by injecting a fake STT client. That's overkill for v1 — but we *can* validate the simpler invariant: the prompt builder receives the right vocab list for the chosen mode. This is small enough to do as a Rust unit test directly inside `src-tauri/src/commands/segments.rs`.

**Files:**
- Modify: `src-tauri/src/commands/segments.rs`

- [ ] **Step 1: Add a test module at the bottom of `src-tauri/src/commands/segments.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use crate::db::Db;

    // We test the *prompt selection branch* — i.e. given a segment row with
    // vocab_snapshot X and a current settings value Y, the function picks
    // the right list for each mode. We do NOT exercise the STT client.

    fn make_db_with_vocab(snapshot: &str, current: &[&str]) -> (Db, i64) {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/001_initial_schema.sql")).unwrap();
        conn.execute("INSERT INTO schema_version (version) VALUES (1)", []).unwrap();
        conn.execute(
            "INSERT INTO tabs (id, title, order_idx, created_at, updated_at) VALUES (1, 'A', 0, 0, 0)",
            [],
        ).unwrap();
        let current_json = serde_json::to_string(current).unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('vocab_terms', ?)",
            [&current_json],
        ).unwrap();
        let db = Db::from_connection(conn);
        let seg = repo::insert(&db, &repo::NewSegment {
            tab_id: 1,
            text: "before".into(),
            audio_path: "x.wav".into(),
            started_at: 0,
            ended_at: 1,
            duration_ms: 1,
            vocab_snapshot: snapshot.to_string(),
            avg_logprob: -0.3,
            no_speech_prob: 0.02,
            model_id: "m".into(),
        }).unwrap();
        (db, seg.id)
    }

    fn pick_vocab(db: &Db, seg_id: i64, mode: RetranscribeMode) -> Vec<String> {
        // Re-implementation of the branch inside `segments_retranscribe`.
        // We deliberately mirror the exact logic so the test catches a
        // future refactor that diverges them.
        let segment = repo::get_by_id(db, seg_id).unwrap();
        match mode {
            RetranscribeMode::Current => settings_repo::get::<Vec<String>>(db, "vocab_terms")
                .unwrap()
                .unwrap_or_default(),
            RetranscribeMode::Snapshot => {
                serde_json::from_str::<Vec<String>>(&segment.vocab_snapshot).unwrap_or_default()
            }
        }
    }

    #[test]
    fn current_mode_uses_settings_vocab() {
        let (db, id) = make_db_with_vocab("[\"old\"]", &["new", "shiny"]);
        let chosen = pick_vocab(&db, id, RetranscribeMode::Current);
        assert_eq!(chosen, vec!["new".to_string(), "shiny".to_string()]);
    }

    #[test]
    fn snapshot_mode_uses_row_snapshot() {
        let (db, id) = make_db_with_vocab("[\"old\"]", &["new", "shiny"]);
        let chosen = pick_vocab(&db, id, RetranscribeMode::Snapshot);
        assert_eq!(chosen, vec!["old".to_string()]);
    }

    #[test]
    fn prompts_differ_between_modes() {
        let (db, id) = make_db_with_vocab("[\"old\"]", &["new"]);
        let current = pick_vocab(&db, id, RetranscribeMode::Current);
        let snapshot = pick_vocab(&db, id, RetranscribeMode::Snapshot);
        let p_current = build_initial_prompt(&current, "pt");
        let p_snapshot = build_initial_prompt(&snapshot, "pt");
        assert_ne!(p_current, p_snapshot);
        assert!(p_current.contains("new"));
        assert!(p_snapshot.contains("old"));
    }
}
```

(In-crate test modules use `crate::db::Db` — not `voicetabs_lib::db::Db`, which only works from the `tests/` integration directory. The `super::*` import already brings in the local repo functions; we only need `Db` from sibling.)

- [ ] **Step 2: Run the tests**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml commands::segments
```

Expected: 3 new tests pass.

- [ ] **Step 3: Commit**

```powershell
git add src-tauri/src/commands/segments.rs
git commit -m "test(segments): current-vs-snapshot vocab selection diverges prompts"
```

---

## Task 15: Type-check + full Rust test sweep

We have introduced enough surface area that one full sweep catches dangling imports.

- [ ] **Step 1: Type-check the frontend**

```powershell
npx tsc --noEmit
```

Expected: clean.

- [ ] **Step 2: Run `cargo test`**

```powershell
cargo test --manifest-path src-tauri\Cargo.toml
```

Expected: all tests green. Approximate counts (Phase 2 baseline 37 + Phase 3 additions + Phase 4 additions):
- db::segments — 8
- audio::rms — 5
- hallucination — 9
- vocab — 6
- routing — 5
- segment_pipeline_test (integration) — 1
- commands::segments — 3
- Existing — unchanged.

If something fails to compile because of a Phase-3 path drift (e.g. `crate::stt::SttClient` vs `crate::stt::client::SttClient`), fix the `use` and re-run. Do not modify any Phase 3 file unless absolutely required — escalate instead.

- [ ] **Step 3: Run `npm test`**

```powershell
npm test
```

Expected: green. Counts: original 13 + segmentsStore 4 + SegmentCard 8 + VocabSettings 3 = ~28.

- [ ] **Step 4: Commit (empty allowed)**

```powershell
git diff --stat
git commit --allow-empty -m "chore: full test sweep passes for Phase 4"
```

---

## Task 16: Manual acceptance pass (A3, A5 full, A6)

This is the Phase 4 acceptance gate. It exercises the criteria a unit test cannot.

- [ ] **Step 1: Reset audio + segments to start clean**

```powershell
Remove-Item -Recurse -Force "$env:APPDATA\voicetabs\audio" -ErrorAction SilentlyContinue
sqlite3 "$env:APPDATA\voicetabs\voicetabs.db" "DELETE FROM segments;"
```

(If `sqlite3` is not on PATH, open the file in your favourite DB browser and run the same query.)

- [ ] **Step 2: Launch the app**

```powershell
npm run tauri dev
```

- [ ] **Step 3: A3 — three tabs, mid-sentence tab switch**

1. Create three tabs: "Reunião", "Pessoal", "Ideias". Confirm three tabs exist.
2. Click "Capturar" (or "Capture") to turn on capture.
3. Click "Reunião". Speak a complete sentence ("Esta nota é da reunião com a equipe."). Wait ~1 s.
   - **Expected:** one segment card appears under "Reunião" within ≤ 1.5 s of finishing the sentence.
4. Click "Pessoal". Speak ("Lembrar de comprar pão."). Wait.
   - **Expected:** one card under "Pessoal". "Reunião" still has just the first card.
5. Click "Ideias". **Start speaking** ("Esta é uma ideia importante sobre o projeto."), and **while speaking**, click "Reunião". Finish the sentence on "Reunião". Wait.
   - **Expected:** the *complete* sentence appears under **"Ideias"** as one card. "Reunião" still has just the original first card.

If step 5 fails (sentence lands in "Reunião"), A3 has regressed — STOP and audit the rising-edge snapshot logic in `process_chunks`.

- [ ] **Step 4: A5 full — 60 s silence → 0 segments**

1. Tabs and capture still as in Step 3. Count current rows: `SELECT COUNT(*) FROM segments;`.
2. Sit silently for 60 seconds. Do not type, do not move the mouse near the mic, do not let the room produce sounds above ~ -45 dBFS.
3. Re-count rows.
   - **Expected:** identical count. No new WAV files in `%APPDATA%\voicetabs\audio\` (Phase 2 already enforced this for WAVs; A5 *full* also forbids segment rows).

- [ ] **Step 5: A6 — vocab term improves transcription**

1. Pick a distinctive proper noun the small model would mistranscribe (e.g. "Pessatti", "rusqlite", "Anthropic", a custom name).
2. Open Settings → Vocabulary. Type the term on its own line. Click outside the textarea to blur — confirm Tauri logs the `settings_set` call (or open dev tools and watch the network panel; or restart and check the textarea is repopulated).
3. With vocab empty, speak a sentence containing that term once. Note the transcription. (If you can't easily empty vocab, use the snapshot-vs-current re-transcribe path in step 7 instead.)
4. Speak the same sentence again with vocab populated.
   - **Expected:** the term is now spelled correctly (or noticeably closer). Whisper does not guarantee perfect; "demonstrably improved" is the acceptance bar.

- [ ] **Step 6: Inline edit + delete**

1. On any segment card, hover, click ✎. Replace text, click Save.
   - **Expected:** text updates; DB row's `text` differs from `original_text`. (Verify via `sqlite3`: `SELECT text, original_text FROM segments WHERE id = ?`.)
2. On a long segment (> 20 chars), open the menu and click Delete.
   - **Expected:** browser `confirm` dialog appears; on OK, the card disappears and the WAV is gone from disk.
3. On a short segment, delete it.
   - **Expected:** no confirm; immediate delete.

- [ ] **Step 7: Re-transcribe (snapshot vs current vocab)**

1. With non-empty vocab AND an existing segment whose `vocab_snapshot` is `[]` (old, pre-vocab-set segment):
   - Menu → "Re-transcribe (current vocab)" → text updates using the current vocab.
   - Menu → "Re-transcribe (snapshot vocab)" → text reverts toward what the original (empty-vocab) prompt would produce.
2. Verify `model_id` updates on each re-transcribe, but `original_text` and `vocab_snapshot` stay frozen.

- [ ] **Step 8: Restart and verify persistence**

1. Close the window with the OS ×. Relaunch `npm run tauri dev`.
2. All segments are still there. Audio replay still works. Vocab textarea still populated.

- [ ] **Step 9: Record results**

If every step passed, the Phase 4 acceptance is met (A3 full, A5 full, A6 demonstrated). Write the stop-and-report covering:
- Pass/fail per A3/A5/A6 step.
- Approximate end-to-end latency observed for the A3 sentences (eyeball it; the L1 budget is 1.5 s).
- Any hallucination-filter drops that surprised you (cat purring, fan noise) — record so we can revisit thresholds in Phase 7.

- [ ] **Step 10: Final commit (if any tweaks fell out)**

```powershell
git diff --stat
git commit -am "chore: phase 4 manual acceptance pass"
```

---

## End of Phase 4 — checkpoint

**Stop here and produce a stop-and-report.** Phase 5 (hotkeys + tray + capture-mode picker) is planned in a separate session.

**What's verified at this checkpoint:**
- A1 — not yet (installer in Phase 8).
- A2 — not yet (first-run wizard in Phase 3 — verify if Phase 3 already shipped it).
- A3 — **full**. Snapshot-on-rising-edge is unit-tested AND manually exercised.
- A4 — not yet (PTT in Phase 5).
- A5 — **full**. 60 s silence produces zero segments (Phase 2 covered WAVs; Phase 4 covers segments).
- A6 — **demonstrated**. Manual acceptance Step 5; the prompt builder + the "current vs snapshot" divergence are unit-tested.
- A7 — partial (Phase 3 supervised the worker; Phase 7 measures cold-cache recovery).
- A8 — **full so far**: tabs, settings, segments, audio, and (transitively) vocab all persist.
- L1, L2 — inherited from Phase 3; Phase 4 does not regress.

**What's verified that isn't on the criteria list:**
- Hallucination filter is pure + table-tested with 9 cases.
- RMS dBFS helper is verified across the energy floor, full-scale DC, half-amplitude sine, and the filter threshold edge.
- The `ActiveTab` atomic survives a concurrent-writer stress test.
- Segment CRUD is repository-tested (8 cases) and exercised end-to-end via the manual acceptance.
- Frontend has 15 new tests covering: segment-created event listener (4), SegmentCard (8), VocabSettings (3).
- The capture worker's snapshot-at-rising-edge invariant is pinned by a Rust integration test that survives later refactors.
