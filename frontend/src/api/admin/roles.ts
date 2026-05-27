import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type RoleRecord = {
  id: number;
  name: string;
  code: string;
  description?: string | null;
  is_system: boolean;
  enabled: boolean;
  tenant_id?: number | null;
  created_at: string;
  updated_at: string;
};

export type SaveRoleParams = {
  name: string;
  code: string;
  description?: string | null;
  enabled?: boolean;
  tenant_id?: number | null;
};

export type RoleMenuGrant = {
  menu_id: number;
  can_create: boolean;
  can_update: boolean;
  can_delete: boolean;
  can_import: boolean;
  can_export: boolean;
  can_print: boolean;
  can_help: boolean;
};

export async function fetchRoles(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
}) {
  const response = await apiClient.get<ApiResponse<PageResponse<RoleRecord>>>(
    "/admin/roles",
    { params },
  );
  return unwrap(response.data);
}

export async function createRole(payload: SaveRoleParams) {
  const response = await apiClient.post<ApiResponse<RoleRecord>>(
    "/admin/roles",
    payload,
  );
  return unwrap(response.data);
}

export async function updateRole(id: number, payload: SaveRoleParams) {
  const response = await apiClient.put<ApiResponse<RoleRecord>>(
    `/admin/roles/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteRole(id: number) {
  await apiClient.delete(`/admin/roles/${id}`);
}

export async function fetchRolePermissions(id: number) {
  const response = await apiClient.get<
    ApiResponse<{ permission_ids: number[] }>
  >(`/admin/roles/${id}/permissions`);
  return unwrap(response.data).permission_ids;
}

export async function saveRolePermissions(id: number, permissionIds: number[]) {
  await apiClient.put(`/admin/roles/${id}/permissions`, {
    permission_ids: permissionIds,
  });
}

export async function fetchRoleMenus(id: number) {
  const response = await apiClient.get<
    ApiResponse<{ grants: RoleMenuGrant[] }>
  >(`/admin/roles/${id}/menus`);
  return unwrap(response.data).grants;
}

export async function saveRoleMenus(id: number, grants: RoleMenuGrant[]) {
  await apiClient.put(`/admin/roles/${id}/menus`, { grants });
}

export async function fetchRoleDataScopes(id: number) {
  const response = await apiClient.get<
    ApiResponse<{ data_scope_ids: number[] }>
  >(`/admin/roles/${id}/data-scopes`);
  return unwrap(response.data).data_scope_ids;
}

export async function saveRoleDataScopes(id: number, dataScopeIds: number[]) {
  await apiClient.put(`/admin/roles/${id}/data-scopes`, {
    data_scope_ids: dataScopeIds,
  });
}
