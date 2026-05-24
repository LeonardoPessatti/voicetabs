import { useTranslation } from "react-i18next";

import { SttStatus } from "../lib/tauri";

type Props = { status: SttStatus };

function prettyBackend(b: string): string {
  // Phase 7 will add "openai"; map here so the UI doesn't show the raw key.
  switch (b) {
    case "cpu":
      return "Local CPU";
    case "openai":
      return "OpenAI";
    default:
      return b;
  }
}

export function SttStatusDot({ status }: Props) {
  const { t } = useTranslation();
  let cls = "stt-dot";
  let title = "";

  switch (status.state) {
    case "ready":
      cls += " stt-dot--ready";
      title = `${t("stt.ready")} (${prettyBackend(status.backend)} · ${status.model_id})`;
      break;
    case "loading":
      cls += " stt-dot--loading";
      title = `${t("stt.loading")} (${prettyBackend(status.backend)})`;
      break;
    case "restarting":
      cls += " stt-dot--restarting";
      title = `${t("stt.restarting")} (${prettyBackend(status.backend)})`;
      break;
    case "error":
      cls += " stt-dot--error";
      title = `${t("stt.error")}: ${status.message}`;
      break;
  }

  return <span data-testid="stt-dot" className={cls} title={title} />;
}
