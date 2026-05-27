import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type DictTypeRecord = {
  id: number;
  code: string;
  name: string;
  description?: string | null;
  enabled: boolean;
  is_builtin: boolean;
  sort_order: number;
  created_at: string;
  updated_at: string;
};

export type SaveDictTypeParams = {
  code: string;
  name: string;
  description?: string | null;
  enabled?: boolean;
  is_builtin?: boolean;
  sort_order?: number;
};

export type DictItemRecord = {
  id: number;
  dict_type_id: number;
  label: string;
  value: string;
  color?: string | null;
  extra?: string | null;
  enabled: boolean;
  is_default: boolean;
  sort_order: number;
  created_at: string;
  updated_at: string;
};

export type SaveDictItemParams = {
  dict_type_id: number;
  label: string;
  value: string;
  color?: string | null;
  extra?: string | null;
  enabled?: boolean;
  is_default?: boolean;
  sort_order?: number;
};

export async function fetchDictTypes(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<DictTypeRecord>>
  >("/admin/dict-types", { params });
  return unwrap(response.data);
}

export async function createDictType(payload: SaveDictTypeParams) {
  const response = await apiClient.post<ApiResponse<DictTypeRecord>>(
    "/admin/dict-types",
    payload,
  );
  return unwrap(response.data);
}

export async function updateDictType(id: number, payload: SaveDictTypeParams) {
  const response = await apiClient.put<ApiResponse<DictTypeRecord>>(
    `/admin/dict-types/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteDictType(id: number) {
  await apiClient.delete(`/admin/dict-types/${id}`);
}

export async function fetchDictItems(dictTypeId: number) {
  const response = await apiClient.get<ApiResponse<DictItemRecord[]>>(
    `/admin/dict-types/${dictTypeId}/items`,
  );
  return unwrap(response.data);
}

export async function createDictItem(payload: SaveDictItemParams) {
  const response = await apiClient.post<ApiResponse<DictItemRecord>>(
    "/admin/dict-items",
    payload,
  );
  return unwrap(response.data);
}

export async function updateDictItem(id: number, payload: SaveDictItemParams) {
  const response = await apiClient.put<ApiResponse<DictItemRecord>>(
    `/admin/dict-items/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteDictItem(id: number) {
  await apiClient.delete(`/admin/dict-items/${id}`);
}
