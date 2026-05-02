export function runStatusLabel(status: string): string {
  if (status === "ok") return "успешно";
  if (status === "error") return "ошибка";
  if (status === "running") return "выполняется";
  if (status === "skipped") return "пропущено";
  if (status === "cancelled") return "отменено";
  return status;
}

export function callerLabel(caller: string): string {
  if (caller === "manual") return "вручную";
  if (caller === "schedule") return "расписание";
  if (caller === "app") return "утилита";
  if (caller === "system") return "система";
  return caller;
}
