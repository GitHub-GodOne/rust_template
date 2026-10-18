import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";

export type MappingMode = "direct" | "tunnel";
export type IpMode = "off" | "auto" | "manual";
export type CertificateStatus = "pending" | "issuing" | "active" | "failed";
export type MappingStatus = "active" | "pending" | "error" | "disabled";

export type CertificateRecord = {
  id: number;
  domain: string;
  alt_names: string[];
  status: CertificateStatus;
  auto_renew: boolean;
  issuing: boolean;
  issuer?: string | null;
  not_before?: string | null;
  not_after?: string | null;
  days_remaining?: number | null;
  last_error?: string | null;
  last_attempt_at?: string | null;
  renewed_at?: string | null;
  mapping_count: number;
  created_at: string;
};

export type DomainMappingRecord = {
  id: number;
  name: string;
  domain: string;
  mode: MappingMode;
  upstream_host: string;
  upstream_port: number;
  public_port: number;
  public_url: string;
  ipv4_mode: IpMode;
  ipv4_address?: string | null;
  ipv6_mode: IpMode;
  ipv6_address?: string | null;
  enabled: boolean;
  status: MappingStatus;
  last_error?: string | null;
  synced_at?: string | null;
  certificate_id?: number | null;
  certificate_status?: CertificateStatus | null;
  certificate_not_after?: string | null;
  created_at: string;
  updated_at: string;
};

export type SaveDomainMappingParams = {
  name?: string;
  domain: string;
  mode: MappingMode;
  upstream_host?: string;
  upstream_port: number;
  public_port?: number;
  ipv4_mode?: IpMode;
  ipv4_address?: string;
  ipv6_mode?: IpMode;
  ipv6_address?: string;
  enabled?: boolean;
  issue_certificate?: boolean;
};

export type SaveDomainMappingResult = {
  mapping: DomainMappingRecord;
  notice?: string | null;
};

export type CreateCertificateParams = {
  domain: string;
  alt_names?: string[];
  auto_renew?: boolean;
  issue_now?: boolean;
};

export type CertificateSettings = {
  acme_email: string;
  acme_directory: string;
  cloudflare_auth_mode: "api_token" | "global_api_key";
  cloudflare_api_token_set: boolean;
  cloudflare_api_email: string;
  cloudflare_account_id: string;
  cloudflare_api_base: string;
  uses_ddns_credentials: boolean;
  renew_before_days: number;
  propagation_timeout_seconds: number;
  ipv4_lookup_url: string;
  cloudflared_path: string;
  tunnel_name: string;
  tunnel_id: string;
};

export type SaveCertificateSettingsParams = Partial<
  Omit<
    CertificateSettings,
    "cloudflare_api_token_set" | "uses_ddns_credentials" | "tunnel_id"
  >
> & { cloudflare_api_token?: string };

export type ListenerStatus = {
  port: number;
  running: boolean;
  hosts: string[];
  error?: string | null;
};

export type TunnelStatus = {
  binary?: string | null;
  running: boolean;
  pid?: number | null;
  restarts: number;
  last_error?: string | null;
  recent_logs: string[];
};

export type CertificateRuntime = {
  listeners: ListenerStatus[];
  tunnel: TunnelStatus;
  tunnel_id: string;
  tunnel_name: string;
  server_port: number;
};

export async function fetchCertificates() {
  const response = await apiClient.get<ApiResponse<CertificateRecord[]>>(
    "/admin/certificates",
  );
  return unwrap(response.data);
}

export async function createCertificate(payload: CreateCertificateParams) {
  const response = await apiClient.post<ApiResponse<CertificateRecord>>(
    "/admin/certificates",
    payload,
  );
  return unwrap(response.data);
}

export async function updateCertificate(
  id: number,
  payload: { auto_renew?: boolean; alt_names?: string[] },
) {
  const response = await apiClient.put<ApiResponse<CertificateRecord>>(
    `/admin/certificates/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteCertificate(id: number) {
  const response = await apiClient.delete<ApiResponse<unknown>>(
    `/admin/certificates/${id}`,
  );
  return unwrap(response.data);
}

export async function issueCertificate(id: number) {
  const response = await apiClient.post<ApiResponse<CertificateRecord>>(
    `/admin/certificates/${id}/issue`,
  );
  return unwrap(response.data);
}

export async function fetchCertificateSettings() {
  const response = await apiClient.get<ApiResponse<CertificateSettings>>(
    "/admin/certificates/settings",
  );
  return unwrap(response.data);
}

export async function saveCertificateSettings(
  payload: SaveCertificateSettingsParams,
) {
  const response = await apiClient.put<ApiResponse<CertificateSettings>>(
    "/admin/certificates/settings",
    payload,
  );
  return unwrap(response.data);
}

export async function fetchDomainMappings() {
  const response = await apiClient.get<ApiResponse<DomainMappingRecord[]>>(
    "/admin/domain-mappings",
  );
  return unwrap(response.data);
}

export async function createDomainMapping(payload: SaveDomainMappingParams) {
  const response = await apiClient.post<ApiResponse<SaveDomainMappingResult>>(
    "/admin/domain-mappings",
    payload,
  );
  return unwrap(response.data);
}

export async function updateDomainMapping(
  id: number,
  payload: SaveDomainMappingParams,
) {
  const response = await apiClient.put<ApiResponse<SaveDomainMappingResult>>(
    `/admin/domain-mappings/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteDomainMapping(id: number, removeDns = true) {
  const response = await apiClient.delete<ApiResponse<unknown>>(
    `/admin/domain-mappings/${id}`,
    { params: { remove_dns: removeDns } },
  );
  return unwrap(response.data);
}

export async function syncDomainMapping(id: number) {
  const response = await apiClient.post<ApiResponse<SaveDomainMappingResult>>(
    `/admin/domain-mappings/${id}/sync`,
  );
  return unwrap(response.data);
}

export async function fetchCertificateRuntime() {
  const response = await apiClient.get<ApiResponse<CertificateRuntime>>(
    "/admin/domain-mappings/runtime",
  );
  return unwrap(response.data);
}

export async function restartTunnel() {
  const response = await apiClient.post<ApiResponse<unknown>>(
    "/admin/domain-mappings/tunnel/restart",
  );
  return unwrap(response.data);
}
