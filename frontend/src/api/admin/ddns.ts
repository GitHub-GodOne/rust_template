import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";

export type DdnsRecordAction = "created" | "updated" | "unchanged" | "failed";

export type DdnsRecordStatus = {
  name: string;
  action: DdnsRecordAction;
  content?: string | null;
  message?: string | null;
};

export type DdnsSource = "auto" | "interface" | "url" | "manual";

export type DdnsSyncTrigger = "schedule" | "manual" | "manual_address";

export type DdnsStatus = {
  enabled: boolean;
  configured: boolean;
  token_configured: boolean;
  source: DdnsSource;
  interface_name?: string | null;
  record_names: string[];
  zone_id?: string | null;
  proxied: boolean;
  ttl: number;
  interval_seconds: number;
  manual_ipv6?: string | null;
  current_ipv6?: string | null;
  last_synced_ipv6?: string | null;
  last_checked_at?: string | null;
  last_synced_at?: string | null;
  last_error?: string | null;
  last_sync_trigger?: DdnsSyncTrigger | null;
  records: DdnsRecordStatus[];
};

export async function fetchDdnsStatus() {
  const response =
    await apiClient.get<ApiResponse<DdnsStatus>>("/admin/ddns/status");
  return unwrap(response.data);
}

export type DdnsConfig = {
  enabled: boolean;
  auth_mode: "api_token" | "global_api_key";
  api_email: string;
  token_configured: boolean;
  zone_id: string;
  record_names: string;
  proxied: boolean;
  ttl: number;
  check_interval_seconds: number;
  ipv6_source: DdnsSource;
  interface_name: string;
  manual_ipv6: string;
  ipv6_lookup_url: string;
  api_base: string;
};

export type SaveDdnsConfigParams = Omit<DdnsConfig, "token_configured"> & {
  /** Leave empty to keep the stored token. */
  api_token?: string;
};

export async function fetchDdnsConfig() {
  const response =
    await apiClient.get<ApiResponse<DdnsConfig>>("/admin/ddns/config");
  return unwrap(response.data);
}

export async function saveDdnsConfig(payload: SaveDdnsConfigParams) {
  const response = await apiClient.put<ApiResponse<DdnsConfig>>(
    "/admin/ddns/config",
    payload,
  );
  return unwrap(response.data);
}

/** Syncs the configured address, or `ipv6` once when given. */
export async function syncDdnsNow(ipv6?: string) {
  const response = await apiClient.post<ApiResponse<DdnsStatus>>(
    "/admin/ddns/sync",
    ipv6 ? { ipv6 } : {},
  );
  return unwrap(response.data);
}
