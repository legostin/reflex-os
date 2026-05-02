export interface ScheduleListItem {
  schedule_id: string;
  app_id: string;
  app_name: string;
  name: string;
  cron: string;
  enabled: boolean;
  paused: boolean;
  valid: boolean;
  next_fire_ms: number | null;
  last_fire_at_ms: number;
  last_run_id: string | null;
  steps_count: number;
}

export interface RunSummary {
  run_id: string;
  app_id: string;
  schedule_id: string | null;
  action_id: string | null;
  caller: string;
  status: string;
  started_ms: number;
  ended_ms: number | null;
  error_preview: string | null;
}

export interface SchedulerLastError {
  run_id: string;
  app_id: string;
  schedule_id: string | null;
  action_id: string | null;
  started_ms: number;
  error_preview: string | null;
}

export interface SchedulerStats {
  schedules: {
    total: number;
    enabled: number;
    active: number;
    paused: number;
    invalid: number;
    next_fire_ms: number | null;
  };
  recent_runs: {
    sample: number;
    ok: number;
    error: number;
    last_error: SchedulerLastError | null;
  };
}

export interface StepTrace {
  name: string;
  method: string;
  status: string;
  started_ms: number;
  ended_ms: number;
  output_preview: string | null;
  output_size: number;
  error: string | null;
}

export interface RunRecord {
  run_id: string;
  app_id: string;
  schedule_id: string | null;
  action_id: string | null;
  caller: string;
  started_ms: number;
  ended_ms: number | null;
  status: string;
  steps: StepTrace[];
  error: string | null;
}
