import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type ScheduledTaskRecord = {
  id: number;
  name: string;
  code: string;
  task_type: string;
  cron_expr: string;
  payload?: string | null;
  enabled: boolean;
  status: string;
  last_run_at?: string | null;
  next_run_at?: string | null;
  created_by?: number | null;
  updated_by?: number | null;
  created_at: string;
  updated_at: string;
};

export type TaskRunRecord = {
  id: number;
  task_id: number;
  code: string;
  status: string;
  started_at: string;
  finished_at?: string | null;
  duration_ms?: number | null;
  output?: string | null;
  error_message?: string | null;
  triggered_by: string;
  created_at: string;
  updated_at: string;
};

export type SaveScheduledTaskParams = {
  name: string;
  code: string;
  task_type: string;
  cron_expr: string;
  payload?: string | null;
  enabled?: boolean;
  status?: string;
  next_run_at?: string | null;
};

export async function fetchScheduledTasks(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
  task_type?: string;
  status?: string;
  enabled?: boolean;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<ScheduledTaskRecord>>
  >("/admin/scheduled-tasks", { params });
  return unwrap(response.data);
}

export async function createScheduledTask(payload: SaveScheduledTaskParams) {
  const response = await apiClient.post<ApiResponse<ScheduledTaskRecord>>(
    "/admin/scheduled-tasks",
    payload,
  );
  return unwrap(response.data);
}

export async function updateScheduledTask(
  id: number,
  payload: SaveScheduledTaskParams,
) {
  const response = await apiClient.put<ApiResponse<ScheduledTaskRecord>>(
    `/admin/scheduled-tasks/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteScheduledTask(id: number) {
  await apiClient.delete(`/admin/scheduled-tasks/${id}`);
}

export async function runScheduledTask(id: number) {
  const response = await apiClient.post<ApiResponse<TaskRunRecord>>(
    `/admin/scheduled-tasks/${id}/run`,
  );
  return unwrap(response.data);
}

export async function fetchTaskRuns(params?: {
  page?: number;
  page_size?: number;
  task_id?: number;
  status?: string;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<TaskRunRecord>>
  >("/admin/scheduled-task-runs", { params });
  return unwrap(response.data);
}
