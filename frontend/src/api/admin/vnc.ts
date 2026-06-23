import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";

export type VncTargetRecord = {
  key: string;
  name: string;
  target_type: "local" | "vnc" | string;
  host: string;
  port: number;
  enabled: boolean;
  requires_password: boolean;
};

export type CreateVncSessionParams = {
  target_key: string;
};

export type VncTicketRecord = {
  ticket: string;
  expires_at: string;
  password?: string | null;
};

export type VncSessionRecord = {
  id: string;
  target_key: string;
  target_name: string;
  target_type: "local" | "vnc" | string;
  host: string;
  port: number;
  status: string;
  requires_password: boolean;
  created_by: number;
  created_at: string;
  updated_at: string;
};

export async function fetchVncTargets() {
  const response =
    await apiClient.get<ApiResponse<VncTargetRecord[]>>("/admin/vnc/targets");
  return unwrap(response.data);
}

export async function fetchVncSessions() {
  const response = await apiClient.get<ApiResponse<VncSessionRecord[]>>(
    "/admin/vnc/sessions",
  );
  return unwrap(response.data);
}

export async function createVncSession(payload: CreateVncSessionParams) {
  const response = await apiClient.post<ApiResponse<VncSessionRecord>>(
    "/admin/vnc/sessions",
    payload,
  );
  return unwrap(response.data);
}

export async function closeVncSession(id: string) {
  const response = await apiClient.delete<ApiResponse<VncSessionRecord>>(
    `/admin/vnc/sessions/${id}`,
  );
  return unwrap(response.data);
}

export async function createVncSessionTicket(id: string) {
  const response = await apiClient.post<ApiResponse<VncTicketRecord>>(
    `/admin/vnc/sessions/${id}/tickets`,
  );
  return unwrap(response.data);
}

export function buildVncSessionWebSocketUrl(sessionId: string, ticket: string) {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/api/admin/vnc/sessions/${sessionId}/ws/${ticket}`;
}
