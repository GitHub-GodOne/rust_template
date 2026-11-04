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

export type CreateSshSessionParams = {
  target_key: string;
  cols?: number;
  rows?: number;
  /** Tab name; the target name when empty. */
  title?: string;
};

export type SshTicketRecord = {
  ticket: string;
  expires_at: string;
};

export type SshSessionRecord = {
  id: string;
  target_key: string;
  target_name: string;
  target_type: "local" | "ssh" | string;
  host?: string | null;
  port?: number | null;
  username?: string | null;
  status: string;
  cols: number;
  rows: number;
  current_directory: string;
  /** Tab name chosen by the user. */
  title?: string | null;
  created_by: number;
  created_at: string;
  updated_at: string;
};

export type SshFileRecord = {
  name: string;
  path: string;
  is_dir: boolean;
  extension?: string | null;
  size_bytes: number;
  updated_at?: string | null;
};

export type SshFileBrowserRecord = {
  session_id: string;
  current_directory: string;
  path: string;
  /** `null` at a filesystem root (`/`, `C:\`). */
  parent_path?: string | null;
  directories: SshFileRecord[];
  files: SshFileRecord[];
};

export async function fetchSshTargets() {
  const response =
    await apiClient.get<ApiResponse<SshTargetRecord[]>>("/admin/ssh/targets");
  return unwrap(response.data);
}

export async function fetchSshSessions() {
  const response = await apiClient.get<ApiResponse<SshSessionRecord[]>>(
    "/admin/ssh/sessions",
  );
  return unwrap(response.data);
}

export async function createSshSession(payload: CreateSshSessionParams) {
  const response = await apiClient.post<ApiResponse<SshSessionRecord>>(
    "/admin/ssh/sessions",
    payload,
  );
  return unwrap(response.data);
}

/** Renames a tab; an empty title restores the default name. */
export async function renameSshSession(id: string, title: string) {
  const response = await apiClient.put<ApiResponse<SshSessionRecord>>(
    `/admin/ssh/sessions/${id}`,
    { title },
  );
  return unwrap(response.data);
}

export async function closeSshSession(id: string) {
  const response = await apiClient.delete<ApiResponse<SshSessionRecord>>(
    `/admin/ssh/sessions/${id}`,
  );
  return unwrap(response.data);
}

export async function createSshTicket(payload: CreateSshTicketParams) {
  const response = await apiClient.post<ApiResponse<SshTicketRecord>>(
    "/admin/ssh/tickets",
    payload,
  );
  return unwrap(response.data);
}

export async function createSshSessionTicket(id: string) {
  const response = await apiClient.post<ApiResponse<SshTicketRecord>>(
    `/admin/ssh/sessions/${id}/tickets`,
  );
  return unwrap(response.data);
}

export async function fetchSshSessionFiles(params: {
  sessionId: string;
  path?: string | null;
}) {
  const response = await apiClient.get<ApiResponse<SshFileBrowserRecord>>(
    `/admin/ssh/sessions/${params.sessionId}/files`,
    { params: { path: params.path || undefined } },
  );
  return unwrap(response.data);
}

export async function previewSshSessionFile(params: {
  sessionId: string;
  path: string;
}) {
  const response = await apiClient.get<Blob>(
    `/admin/ssh/sessions/${params.sessionId}/files/preview`,
    {
      params: { path: params.path },
      responseType: "blob",
    },
  );
  return response.data;
}

export function buildSshWebSocketUrl(ticket: string) {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/api/admin/ssh/sessions/${ticket}/ws`;
}

export function buildSshSessionWebSocketUrl(sessionId: string, ticket: string) {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/api/admin/ssh/sessions/${sessionId}/ws/${ticket}`;
}
