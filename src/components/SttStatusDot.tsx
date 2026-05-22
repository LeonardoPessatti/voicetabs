import { useTranslation } from "react-i18next";

import { SttStatus } from "../lib/tauri";

type Props = { status: SttStatus };

export function SttStatusDot({ status }: Props) {
  const { t } = useTranslation();
  let cls = "stt-dot";
  let title = "";

  switch (status.state) {
    case "ready":
      cls += " stt-dot--ready";
      title = `${t("stt.ready")} (${status.backend} · ${status.model_id})`;
      break;
    case "loading":
      cls += " stt-dot--loading";
      title = `${t("stt.loading")} (${status.backend})`;
      break;
    case "restarting":
      cls += " stt-dot--restarting";
      title = `${t("stt.restarting")} (${status.backend})`;
      break;
    case "error":
      cls += " stt-dot--error";
      title = `${t("stt.error")}: ${status.message}`;
      break;
  }

  return <span data-testid="stt-dot" className={cls} title={title} />;
}
