import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";

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
  is_verified: boolean;
  created_at: string;
  updated_at: string;
};

export type AssignedRoleRecord = {
  id: number;
  name: string;
  code: string;
};

export type SaveUserParams = {
  name: string;
  email: string;
  password?: string;
  tenant_id?: number | null;
};

export async function fetchUsers(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
}) {
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
