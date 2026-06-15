import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type DepartmentRecord = {
  id: number;
  tenant_id: number;
  parent_id?: number | null;
  name: string;
  code: string;
  description?: string | null;
  sort_order: number;
  enabled: boolean;
  is_system: boolean;
  created_at: string;
  updated_at: string;
};

export type SaveDepartmentParams = {
  tenant_id?: number | null;
  parent_id?: number | null;
  name: string;
  code: string;
  description?: string | null;
  sort_order?: number;
  enabled?: boolean;
};

export async function fetchDepartments(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<DepartmentRecord>>
  >("/admin/departments", { params });
  return unwrap(response.data);
}

export async function createDepartment(payload: SaveDepartmentParams) {
  const response = await apiClient.post<ApiResponse<DepartmentRecord>>(
    "/admin/departments",
    payload,
  );
  return unwrap(response.data);
}

export async function updateDepartment(
  id: number,
  payload: SaveDepartmentParams,
) {
  const response = await apiClient.put<ApiResponse<DepartmentRecord>>(
    `/admin/departments/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteDepartment(id: number) {
  await apiClient.delete(`/admin/departments/${id}`);
}
