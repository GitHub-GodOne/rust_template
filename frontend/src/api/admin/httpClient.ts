import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";

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
