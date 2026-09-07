import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type ApiKeyRecord = {
  id: number;
  name: string;
  token: string;
  description?: string | null;
  enabled: boolean;
  last_used_at?: string | null;
  created_by?: number | null;
  updated_by?: number | null;
  created_at: string;
  updated_at: string;
};

export type SaveApiKeyParams = {
  name: string;
  token?: string | null;
  description?: string | null;
  enabled?: boolean;
};

export async function fetchApiKeys(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
  enabled?: boolean;
}) {
  const response = await apiClient.get<ApiResponse<PageResponse<ApiKeyRecord>>>(
    "/admin/api-keys",
    { params },
  );
  return unwrap(response.data);
}

export async function createApiKey(payload: SaveApiKeyParams) {
  const response = await apiClient.post<ApiResponse<ApiKeyRecord>>(
    "/admin/api-keys",
    payload,
  );
  return unwrap(response.data);
}

export async function updateApiKey(id: number, payload: SaveApiKeyParams) {
  const response = await apiClient.put<ApiResponse<ApiKeyRecord>>(
    `/admin/api-keys/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteApiKey(id: number) {
  await apiClient.delete(`/admin/api-keys/${id}`);
}
