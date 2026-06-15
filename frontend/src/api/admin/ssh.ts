import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";

export type SshTargetRecord = {
  key: string;
  name: string;
  target_type: "local" | "ssh" | string;
  host?: string | null;
  port?: number | null;
  username?: string | null;
  enabled: boolean;
};

export type CreateSshTicketParams = {
  target_key: string;
  cols?: number;
  rows?: number;
};

export type SshTicketRecord = {
  ticket: string;
  expires_at: string;
};

export async function fetchSshTargets() {
  const response =
    await apiClient.get<ApiResponse<SshTargetRecord[]>>("/admin/ssh/targets");
  return unwrap(response.data);
}

export async function createSshTicket(payload: CreateSshTicketParams) {
  const response = await apiClient.post<ApiResponse<SshTicketRecord>>(
    "/admin/ssh/tickets",
    payload,
  );
  return unwrap(response.data);
}

export function buildSshWebSocketUrl(ticket: string) {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/api/admin/ssh/sessions/${ticket}/ws`;
}
