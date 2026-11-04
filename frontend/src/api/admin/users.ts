import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import { downloadCsv } from "../download";

export type PageResponse<T> = {
  items: T[];
  page: number;
  page_size: number;
  total: number;
};

export type UserRecord = {
  id: number;
  pid: string;
  name: string;
  email: string;
  tenant_id?: number | null;
  current_department_id?: number | null;
  is_verified: boolean;
  /** `active` or `disabled`. */
  status: string;
  /** Locked after too many failed logins. */
  locked: boolean;
  locked_until?: string | null;
  last_login_at?: string | null;
  last_login_ip?: string | null;
  created_at: string;
  updated_at: string;
};

export type UserFilters = {
  page?: number;
  page_size?: number;
  keyword?: string;
  status?: string;
};

export type AssignedRoleRecord = {
  id: number;
  name: string;
  code: string;
};

export type AssignedDepartmentRecord = {
  id: number;
  tenant_id: number;
  name: string;
  code: string;
  is_primary: boolean;
};

export type SaveUserDepartmentsParams = {
  department_ids: number[];
  current_department_id?: number | null;
};

export type SaveUserParams = {
  name: string;
  email: string;
  password?: string;
  tenant_id?: number | null;
};

export async function fetchUsers(params?: UserFilters) {
  const response = await apiClient.get<ApiResponse<PageResponse<UserRecord>>>(
    "/admin/users",
    { params },
  );
  return unwrap(response.data);
}

export async function createUser(
  payload: SaveUserParams & { password: string },
) {
  const response = await apiClient.post<ApiResponse<UserRecord>>(
    "/admin/users",
    payload,
  );
  return unwrap(response.data);
}

export async function updateUser(id: number, payload: SaveUserParams) {
  const response = await apiClient.put<ApiResponse<UserRecord>>(
    `/admin/users/${id}`,
    { name: payload.name, email: payload.email, tenant_id: payload.tenant_id },
  );
  return unwrap(response.data);
}

export async function deleteUser(id: number) {
  await apiClient.delete(`/admin/users/${id}`);
}

export async function fetchUserRoles(id: number) {
  const response = await apiClient.get<ApiResponse<AssignedRoleRecord[]>>(
    `/admin/users/${id}/roles`,
  );
  return unwrap(response.data);
}

export async function saveUserRoles(id: number, roleIds: number[]) {
  await apiClient.put(`/admin/users/${id}/roles`, { role_ids: roleIds });
}

export async function fetchUserDepartments(id: number) {
  const response = await apiClient.get<ApiResponse<AssignedDepartmentRecord[]>>(
    `/admin/users/${id}/departments`,
  );
  return unwrap(response.data);
}

export async function saveUserDepartments(
  id: number,
  payload: SaveUserDepartmentsParams,
) {
  await apiClient.put(`/admin/users/${id}/departments`, payload);
}

export async function setUserStatus(id: number, status: "active" | "disabled") {
  const response = await apiClient.put<ApiResponse<UserRecord>>(
    `/admin/users/${id}/status`,
    { status },
  );
  return unwrap(response.data);
}

export async function resetUserPassword(id: number, password: string) {
  await apiClient.post(`/admin/users/${id}/reset-password`, { password });
}

export async function unlockUser(id: number) {
  const response = await apiClient.post<ApiResponse<UserRecord>>(
    `/admin/users/${id}/unlock`,
  );
  return unwrap(response.data);
}

export function exportUsers(filters: UserFilters) {
  return downloadCsv(
    "/admin/users/export",
    { keyword: filters.keyword, status: filters.status },
    "users.csv",
  );
}
