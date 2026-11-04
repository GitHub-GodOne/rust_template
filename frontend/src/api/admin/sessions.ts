import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { SessionRecord } from "../profile";
import type { PageResponse } from "./users";

export type SessionFilters = {
  page?: number;
  page_size?: number;
  keyword?: string;
  user_id?: number;
  /** `active` (default), `ended` or `all`. */
  status?: string;
};

export async function fetchSessions(params: SessionFilters) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<SessionRecord>>
  >("/admin/sessions", { params });
  return unwrap(response.data);
}

export async function revokeSession(id: number) {
  await apiClient.delete(`/admin/sessions/${id}`);
}

export async function revokeUserSessions(userId: number) {
  const response = await apiClient.post<ApiResponse<{ revoked: number }>>(
    `/admin/users/${userId}/sessions/revoke`,
  );
  return unwrap(response.data).revoked;
}

export type RemoteSessionKind = "ssh" | "vnc" | "rdp" | "audio";

export type RemoteSessionRecord = {
  kind: RemoteSessionKind;
  id: string;
  user_id: number;
  user_name?: string | null;
  user_email?: string | null;
  target_key: string;
  target_name: string;
  /** `host:port`; empty for the local shell. */
  address?: string | null;
  status: string;
  /** Browser windows attached right now. */
  viewers: number;
  created_at: string;
  last_active_at: string;
};

export type RemoteSessionFilters = {
  kind?: RemoteSessionKind;
  keyword?: string;
  user_id?: number;
};

export async function fetchRemoteSessions(params: RemoteSessionFilters) {
  const response = await apiClient.get<ApiResponse<RemoteSessionRecord[]>>(
    "/admin/sessions/remote",
    { params },
  );
  return unwrap(response.data);
}

export async function closeRemoteSession(kind: RemoteSessionKind, id: string) {
  await apiClient.delete(`/admin/sessions/remote/${kind}/${id}`);
}
