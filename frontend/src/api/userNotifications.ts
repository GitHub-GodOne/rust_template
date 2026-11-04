import { type ApiResponse, unwrap } from "./auth";
import { apiClient } from "./client";

export type FeedItem = {
  id: number;
  title: string;
  content: string;
  level: string;
  category: string;
  created_at: string;
  read: boolean;
};

export type FeedResponse = {
  items: FeedItem[];
  unread_count: number;
};

export async function fetchNotificationFeed(limit = 20) {
  const response = await apiClient.get<ApiResponse<FeedResponse>>(
    "/user/notifications",
    { params: { limit } },
  );
  return unwrap(response.data);
}

export async function markNotificationRead(id: number) {
  await apiClient.post(`/user/notifications/${id}/read`);
}

export async function markAllNotificationsRead() {
  await apiClient.post("/user/notifications/read-all");
}

/** Single-use ticket for the `/api/user/events` stream. */
export async function fetchEventsTicket() {
  const response = await apiClient.post<ApiResponse<{ ticket: string }>>(
    "/user/events/ticket",
  );
  return unwrap(response.data).ticket;
}
