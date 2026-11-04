import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type RecycleEntryRecord = {
  id: number;
  resource: string;
  resource_label: string;
  record_id: string;
  title: string;
  item_count: number;
  deleted_by?: number | null;
  deleted_by_name?: string | null;
  deleted_at: string;
  expires_at?: string | null;
};

export type RecycleSummary = {
  retention_days: number;
  resources: { key: string; label: string }[];
};

export type RecycleFilters = {
  page?: number;
  page_size?: number;
  keyword?: string;
  resource?: string;
};

export async function fetchRecycleBin(params: RecycleFilters) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<RecycleEntryRecord>>
  >("/admin/recycle-bin", { params });
  return unwrap(response.data);
}

export async function fetchRecycleSummary() {
  const response = await apiClient.get<ApiResponse<RecycleSummary>>(
    "/admin/recycle-bin/summary",
  );
  return unwrap(response.data);
}

export async function restoreRecycleEntry(id: number) {
  const response = await apiClient.post<ApiResponse<RecycleEntryRecord>>(
    `/admin/recycle-bin/${id}/restore`,
  );
  return unwrap(response.data);
}

export async function purgeRecycleEntry(id: number) {
  await apiClient.delete(`/admin/recycle-bin/${id}`);
}

export async function emptyRecycleBin(resource?: string) {
  const response = await apiClient.post<ApiResponse<{ purged: number }>>(
    "/admin/recycle-bin/empty",
    undefined,
    { params: { resource: resource || undefined } },
  );
  return unwrap(response.data).purged;
}
