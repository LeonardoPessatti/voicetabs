-- 001_initial_schema.sql
-- Initial schema: tabs, segments, settings.
-- Segments table is created now (with a foreign key on tabs) but used in a later phase.

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS tabs (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  title      TEXT    NOT NULL,
  order_idx  INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tabs_order ON tabs(order_idx);

CREATE TABLE IF NOT EXISTS segments (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  tab_id         INTEGER NOT NULL REFERENCES tabs(id) ON DELETE CASCADE,
  position       INTEGER NOT NULL,
  text           TEXT    NOT NULL,
  original_text  TEXT    NOT NULL,
  audio_path     TEXT    NOT NULL,
  started_at     INTEGER NOT NULL,
  ended_at       INTEGER NOT NULL,
  duration_ms    INTEGER NOT NULL,
  vocab_snapshot TEXT    NOT NULL,
  avg_logprob    REAL    NOT NULL,
  no_speech_prob REAL    NOT NULL,
  model_id       TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_segments_tab ON segments(tab_id, position);

CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS schema_version (
  version INTEGER PRIMARY KEY
);
