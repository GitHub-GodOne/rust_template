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

export async function fetchLogs(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
  level?: string;
  log_type?: string;
  module?: string;
  status?: number;
}) {
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
