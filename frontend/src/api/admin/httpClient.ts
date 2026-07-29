import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type HttpClientRuntimeConfig = {
  enabled: boolean;
  request_timeout_seconds: number;
  connect_timeout_seconds: number;
  pool_idle_timeout_seconds: number;
  proxy_enabled: boolean;
  proxy_url?: string | null;
  danger_accept_invalid_certs: boolean;
  user_agent?: string | null;
};

export type HttpClientTestParams = {
  url: string;
};

export type HttpClientTestRecord = {
  ok: boolean;
  status_code?: number | null;
  duration_ms: number;
  message: string;
};

export type HttpRequestPresetRecord = {
  id: number;
  name: string;
  code: string;
  description?: string | null;
  method: string;
  url: string;
  headers_json?: string | null;
  query_json?: string | null;
  body_type: string;
  body_text?: string | null;
  content_type?: string | null;
  source_format: string;
  enabled: boolean;
  last_used_at?: string | null;
  created_at: string;
  updated_at: string;
};

export type SaveHttpRequestPresetParams = {
  name: string;
  code: string;
  description?: string | null;
  method: string;
  url: string;
  headers_json?: string | null;
  query_json?: string | null;
  body_type?: string | null;
  body_text?: string | null;
  content_type?: string | null;
  source_format?: string | null;
  enabled?: boolean;
};

export type HttpRequestRunRecord = {
  id: number;
  preset_id?: number | null;
  name: string;
  method: string;
  url: string;
  headers_json?: string | null;
  query_json?: string | null;
  request_body_type: string;
  request_body_text?: string | null;
  request_body_size_bytes: number;
  status: string;
  response_status_code?: number | null;
  response_headers_json?: string | null;
  response_content_type?: string | null;
  response_size_bytes: number;
  duration_ms: number;
  error_message?: string | null;
  response_preview_text?: string | null;
  has_response_body: boolean;
  created_at: string;
};

export type HttpRequestExecuteParams = {
  name?: string | null;
  method: string;
  url: string;
  headers_json?: string | null;
  query_json?: string | null;
  body_type?: string | null;
  body_text?: string | null;
  content_type?: string | null;
};

export type ParseHttpRequestImportParams = {
  format: string;
  content: string;
};

export type HttpRequestDraft = {
  name?: string | null;
  method: string;
  url: string;
  headers_json?: string | null;
  query_json?: string | null;
  body_type: string;
  body_text?: string | null;
  content_type?: string | null;
  source_format?: string | null;
};

export async function fetchHttpClientConfig() {
  const response = await apiClient.get<ApiResponse<HttpClientRuntimeConfig>>(
    "/admin/http-client/config",
  );
  return unwrap(response.data);
}

export async function updateHttpClientConfig(payload: HttpClientRuntimeConfig) {
  const response = await apiClient.put<ApiResponse<HttpClientRuntimeConfig>>(
    "/admin/http-client/config",
    payload,
  );
  return unwrap(response.data);
}

export async function testHttpClientRequest(payload: HttpClientTestParams) {
  const response = await apiClient.post<ApiResponse<HttpClientTestRecord>>(
    "/admin/http-client/test",
    payload,
  );
  return unwrap(response.data);
}

export async function fetchHttpRequestPresets(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
  enabled?: boolean;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<HttpRequestPresetRecord>>
  >("/admin/http-client/requests", { params });
  return unwrap(response.data);
}

export async function createHttpRequestPreset(
  payload: SaveHttpRequestPresetParams,
) {
  const response = await apiClient.post<ApiResponse<HttpRequestPresetRecord>>(
    "/admin/http-client/requests",
    payload,
  );
  return unwrap(response.data);
}

export async function updateHttpRequestPreset(
  id: number,
  payload: SaveHttpRequestPresetParams,
) {
  const response = await apiClient.put<ApiResponse<HttpRequestPresetRecord>>(
    `/admin/http-client/requests/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteHttpRequestPreset(id: number) {
  const response = await apiClient.delete<ApiResponse<null>>(
    `/admin/http-client/requests/${id}`,
  );
  return response.data;
}

export async function sendHttpRequestPreset(id: number) {
  const response = await apiClient.post<ApiResponse<HttpRequestRunRecord>>(
    `/admin/http-client/requests/${id}/send`,
  );
  return unwrap(response.data);
}

export async function sendAdHocHttpRequest(payload: HttpRequestExecuteParams) {
  const response = await apiClient.post<ApiResponse<HttpRequestRunRecord>>(
    "/admin/http-client/runs",
    payload,
  );
  return unwrap(response.data);
}

export async function fetchHttpRequestRuns(params?: {
  page?: number;
  page_size?: number;
  preset_id?: number;
  status?: string;
  keyword?: string;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<HttpRequestRunRecord>>
  >("/admin/http-client/runs", { params });
  return unwrap(response.data);
}

export async function fetchHttpRequestRun(id: number) {
  const response = await apiClient.get<ApiResponse<HttpRequestRunRecord>>(
    `/admin/http-client/runs/${id}`,
  );
  return unwrap(response.data);
}

export async function parseHttpRequestImport(
  payload: ParseHttpRequestImportParams,
) {
  const response = await apiClient.post<ApiResponse<HttpRequestDraft>>(
    "/admin/http-client/parse-import",
    payload,
  );
  return unwrap(response.data);
}

export async function downloadHttpRequestRunResult(run: HttpRequestRunRecord) {
  const response = await apiClient.get<Blob>(
    `/admin/http-client/runs/${run.id}/download`,
    { responseType: "blob" },
  );
  const url = window.URL.createObjectURL(response.data);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = `http-response-${run.id}.bin`;
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
  window.URL.revokeObjectURL(url);
}
