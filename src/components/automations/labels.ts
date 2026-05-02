import type { Translate } from "../../i18n";

export function runStatusLabel(status: string, t: Translate): string {
  if (status === "ok") return t("automations.statusOk");
  if (status === "error") return t("automations.statusError");
  if (status === "running") return t("automations.stateRunning");
  if (status === "skipped") return t("automations.statusSkipped");
  if (status === "cancelled") return t("automations.statusCancelled");
  return status;
}

export function callerLabel(caller: string, t: Translate): string {
  if (caller === "manual") return t("automations.callerManual");
  if (caller === "schedule") return t("automations.callerSchedule");
  if (caller === "app") return t("automations.callerApp");
  if (caller === "system") return t("automations.callerSystem");
  return caller;
}
