import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";

export type RdpTarget = {
  key: string;
  name: string;
  host: string;
  port: number;
  enabled: boolean;
  ignore_certificate: boolean;
};

export type RdpLogin = {
  target_key: string;
  username?: string;
  password?: string;
  domain?: string;
  width?: number;
  height?: number;
  remember_credentials?: boolean;
  use_saved_credentials?: boolean;
};

export async function fetchRdpCredentialStatus(key: string) {
  return unwrap(
    (
      await apiClient.get<ApiResponse<{ saved: boolean }>>(
        `/admin/rdp/credentials/${encodeURIComponent(key)}`,
      )
    ).data,
  );
}

export async function deleteRdpCredentials(key: string) {
  return unwrap(
    (
      await apiClient.delete<ApiResponse<{ saved: boolean }>>(
        `/admin/rdp/credentials/${encodeURIComponent(key)}`,
      )
    ).data,
  );
}

export type RdpGatewayStatus = {
  reachable: boolean;
  message: string;
  address: string;
  /** Backend OS: `windows`, `linux`, `macos`, ... */
  platform: string;
  /** The gateway is on the backend machine, which starts it itself. */
  local: boolean;
  /** Installed guacd the backend can start; null when not installed. */
  runtime?: string | null;
  last_error?: string | null;
  recent_logs: string[];
};

export async function fetchRdpGatewayStatus() {
  return unwrap(
    (await apiClient.get<ApiResponse<RdpGatewayStatus>>("/admin/rdp/gateway"))
      .data,
  );
}

export async function fetchRdpTargets() {
  return unwrap(
    (await apiClient.get<ApiResponse<RdpTarget[]>>("/admin/rdp/targets")).data,
  );
}

export async function saveRdpTargets(targets: RdpTarget[]) {
  return unwrap(
    (
      await apiClient.put<ApiResponse<RdpTarget[]>>(
        "/admin/rdp/targets",
        targets,
      )
    ).data,
  );
}

export async function createRdpTicket(login: RdpLogin) {
  return unwrap(
    (
      await apiClient.post<ApiResponse<{ ticket: string; expires_at: string }>>(
        "/admin/rdp/tickets",
        login,
      )
    ).data,
  );
}

export function rdpSocketUrl(ticket: string) {
  return `${window.location.protocol === "https:" ? "wss:" : "ws:"}//${window.location.host}/api/admin/rdp/ws/${encodeURIComponent(ticket)}`;
}
