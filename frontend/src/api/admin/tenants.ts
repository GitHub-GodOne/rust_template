import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type TenantRecord = {
  id: number;
  name: string;
  code: string;
  description?: string | null;
  enabled: boolean;
  is_system: boolean;
  created_at: string;
  updated_at: string;
};

export type SaveTenantParams = {
  name: string;
  code: string;
  description?: string | null;
  enabled?: boolean;
};

export async function fetchTenants(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
}) {
  const response = await apiClient.get<ApiResponse<PageResponse<TenantRecord>>>(
    "/admin/tenants",
    { params },
  );
  return unwrap(response.data);
}

export async function createTenant(payload: SaveTenantParams) {
  const response = await apiClient.post<ApiResponse<TenantRecord>>(
    "/admin/tenants",
    payload,
  );
  return unwrap(response.data);
}

export async function updateTenant(id: number, payload: SaveTenantParams) {
  const response = await apiClient.put<ApiResponse<TenantRecord>>(
    `/admin/tenants/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteTenant(id: number) {
  await apiClient.delete(`/admin/tenants/${id}`);
}
