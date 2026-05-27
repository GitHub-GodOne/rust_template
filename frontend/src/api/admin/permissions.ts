import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type PermissionRecord = {
  id: number;
  name: string;
  code: string;
  group_name: string;
  description?: string | null;
  created_at: string;
  updated_at: string;
};

export type SavePermissionParams = {
  name: string;
  code: string;
  group_name: string;
  description?: string | null;
};

export async function fetchPermissions(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<PermissionRecord>>
  >("/admin/permissions", { params });
  return unwrap(response.data);
}

export async function createPermission(payload: SavePermissionParams) {
  const response = await apiClient.post<ApiResponse<PermissionRecord>>(
    "/admin/permissions",
    payload,
  );
  return unwrap(response.data);
}

export async function updatePermission(
  id: number,
  payload: SavePermissionParams,
) {
  const response = await apiClient.put<ApiResponse<PermissionRecord>>(
    `/admin/permissions/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deletePermission(id: number) {
  await apiClient.delete(`/admin/permissions/${id}`);
}
