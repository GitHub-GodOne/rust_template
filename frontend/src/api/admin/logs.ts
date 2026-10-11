import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type LogRecord = {
  id: number;
  trace_id?: string | null;
  log_type: string;
  level: string;
  module: string;
  action: string;
  message: string;
  method?: string | null;
  path?: string | null;
  status?: number | null;
  duration_ms?: number | null;
  ip?: string | null;
  user_agent?: string | null;
  user_id?: number | null;
  operator?: string | null;
  request_summary?: string | null;
  response_summary?: string | null;
  error_message?: string | null;
  created_at: string;
  updated_at: string;
};

export type LogFilters = {
  keyword?: string;
  level?: string;
  log_type?: string;
  module?: string;
  status?: number;
  /** RFC 3339 */
  start_time?: string;
  /** RFC 3339 */
  end_time?: string;
};

export async function fetchLogs(
  params?: LogFilters & { page?: number; page_size?: number },
) {
  const response = await apiClient.get<ApiResponse<PageResponse<LogRecord>>>(
    "/admin/logs",
    { params },
  );
  return unwrap(response.data);
}

export async function fetchLog(id: number) {
  const response = await apiClient.get<ApiResponse<LogRecord>>(
    `/admin/logs/${id}`,
  );
  return unwrap(response.data);
}

export async function deleteLog(id: number) {
  await apiClient.delete(`/admin/logs/${id}`);
}

export type AuditRule = {
  enabled: boolean;
  log_reads: boolean;
  log_body: boolean;
};

export type AuditFeature = AuditRule & {
  key: string;
  label: string;
  group: string;
  prefixes: string[];
  defaults: AuditRule;
};

export type AuditSettings = {
  enabled: boolean;
  features: AuditFeature[];
};

export type SaveAuditSettings = {
  enabled: boolean;
  features: (AuditRule & { key: string })[];
};

export async function fetchAuditSettings() {
  const response =
    await apiClient.get<ApiResponse<AuditSettings>>("/admin/audit-rules");
  return unwrap(response.data);
}

export async function saveAuditSettings(payload: SaveAuditSettings) {
  const response = await apiClient.put<ApiResponse<AuditSettings>>(
    "/admin/audit-rules",
    payload,
  );
  return unwrap(response.data);
}

/** File name from `Content-Disposition`, if the server sent one. */
export function filenameFromDisposition(value?: string | null) {
  const match = value?.match(/filename="?([^";]+)"?/i);
  return match?.[1];
}

/** Downloads the logs matching `filters` as CSV; returns the row count. */
export async function exportLogs(filters: LogFilters) {
  const response = await apiClient.get<Blob>("/admin/logs/export", {
    params: filters,
    responseType: "blob",
    // Large exports stream for a while.
    timeout: 5 * 60_000,
  });
  const name =
    filenameFromDisposition(response.headers["content-disposition"]) ??
    "operation-logs.csv";
  const url = window.URL.createObjectURL(response.data);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = name;
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
  window.URL.revokeObjectURL(url);
  return Number(response.headers["x-total-count"] ?? 0);
}

export type CleanupResult = {
  ran_at: string;
  deleted: number;
  trigger: "manual" | "schedule";
  error?: string | null;
};

export type RetentionStatus = {
  /** 0 keeps logs forever. */
  retention_days: number;
  expired_count: number;
  total_count: number;
  oldest_at?: string | null;
  last_cleanup?: CleanupResult | null;
  next_run_at?: string | null;
  interval_hours: number;
};

export async function fetchRetention() {
  const response = await apiClient.get<ApiResponse<RetentionStatus>>(
    "/admin/logs/retention",
  );
  return unwrap(response.data);
}

export async function saveRetention(retentionDays: number) {
  const response = await apiClient.put<ApiResponse<RetentionStatus>>(
    "/admin/logs/retention",
    { retention_days: retentionDays },
  );
  return unwrap(response.data);
}

export async function runRetention() {
  const response = await apiClient.post<ApiResponse<CleanupResult>>(
    "/admin/logs/retention/run",
    undefined,
    { timeout: 5 * 60_000 },
  );
  return unwrap(response.data);
}
