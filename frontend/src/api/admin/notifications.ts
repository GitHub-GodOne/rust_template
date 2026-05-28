import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type NotificationRecord = {
  id: number;
  title: string;
  content: string;
  level: string;
  category: string;
  target_type: "all" | "user" | "tenant" | string;
  target_user_id?: number | null;
  tenant_id?: number | null;
  read_at?: string | null;
  created_by?: number | null;
  created_at: string;
  updated_at: string;
};

export type SaveNotificationParams = {
  title: string;
  content: string;
  level: string;
  category: string;
  target_type: string;
  target_user_id?: number | null;
  tenant_id?: number | null;
};

export async function fetchNotifications(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
  level?: string;
  category?: string;
  read?: boolean;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<NotificationRecord>>
  >("/admin/notifications", { params });
  return unwrap(response.data);
}

export async function createNotification(payload: SaveNotificationParams) {
  const response = await apiClient.post<ApiResponse<NotificationRecord>>(
    "/admin/notifications",
    payload,
  );
  return unwrap(response.data);
}

export async function markNotificationRead(id: number) {
  const response = await apiClient.put<ApiResponse<NotificationRecord>>(
    `/admin/notifications/${id}/read`,
  );
  return unwrap(response.data);
}

export async function deleteNotification(id: number) {
  await apiClient.delete(`/admin/notifications/${id}`);
}
