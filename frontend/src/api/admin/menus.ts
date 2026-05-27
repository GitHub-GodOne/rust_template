import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";

export type MenuRecord = {
  id: number;
  parent_id?: number | null;
  title: string;
  path?: string | null;
  icon?: string | null;
  permission_code?: string | null;
  sort_order: number;
  visible: boolean;
  enabled: boolean;
  children: MenuRecord[];
};

export type SaveMenuParams = {
  parent_id?: number | null;
  title: string;
  path?: string | null;
  icon?: string | null;
  permission_code?: string | null;
  sort_order?: number;
  visible?: boolean;
  enabled?: boolean;
};

export async function fetchMenus() {
  const response =
    await apiClient.get<ApiResponse<MenuRecord[]>>("/admin/menus");
  return unwrap(response.data);
}

export async function createMenu(payload: SaveMenuParams) {
  const response = await apiClient.post<ApiResponse<MenuRecord>>(
    "/admin/menus",
    payload,
  );
  return unwrap(response.data);
}

export async function updateMenu(id: number, payload: SaveMenuParams) {
  const response = await apiClient.put<ApiResponse<MenuRecord>>(
    `/admin/menus/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteMenu(id: number) {
  await apiClient.delete(`/admin/menus/${id}`);
}
