import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type SettingRecord = {
  id: number;
  key: string;
  name: string;
  group_key: string;
  value: string;
  value_type: "string" | "number" | "boolean" | "json" | "secret";
  default_value?: string | null;
  description?: string | null;
  is_public: boolean;
  is_builtin: boolean;
  is_encrypted: boolean;
  sort_order: number;
  created_by?: number | null;
  updated_by?: number | null;
  created_at: string;
  updated_at: string;
};

export type SaveSettingParams = {
  key: string;
  name: string;
  group_key: string;
  value: string;
  value_type: SettingRecord["value_type"];
  default_value?: string | null;
  description?: string | null;
  is_public?: boolean;
  is_builtin?: boolean;
  is_encrypted?: boolean;
  sort_order?: number;
};

export async function fetchSettings(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
  group_key?: string;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<SettingRecord>>
  >("/admin/settings", { params });
  return unwrap(response.data);
}

export async function createSetting(payload: SaveSettingParams) {
  const response = await apiClient.post<ApiResponse<SettingRecord>>(
    "/admin/settings",
    payload,
  );
  return unwrap(response.data);
}

export async function updateSetting(id: number, payload: SaveSettingParams) {
  const response = await apiClient.put<ApiResponse<SettingRecord>>(
    `/admin/settings/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteSetting(id: number) {
  await apiClient.delete(`/admin/settings/${id}`);
}
