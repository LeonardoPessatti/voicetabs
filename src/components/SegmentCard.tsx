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
              {"✎"}
            </button>
            <div className="segment-card__menu-wrap">
              <button
                aria-label={t("segments.more")}
                aria-expanded={menuOpen}
                onClick={() => setMenuOpen((o) => !o)}
              >
                {"⋯"}
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
