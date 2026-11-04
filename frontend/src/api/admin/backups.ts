import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type BackupRecord = {
  id: number;
  database_connection_id?: number | null;
  database_label?: string | null;
  filename: string;
  storage_path: string;
  size_bytes: number;
  sha256?: string | null;
  status: string;
  trigger_type: string;
  started_at: string;
  finished_at?: string | null;
  duration_ms?: number | null;
  delivery_targets?: string | null;
  delivery_status?: string | null;
  error_message?: string | null;
  created_by?: number | null;
  created_at: string;
  updated_at: string;
};

export type RestoreRecord = {
  id: number;
  backup_id: number;
  status: string;
  confirm_phrase: string;
  pre_restore_backup_id?: number | null;
  started_at: string;
  finished_at?: string | null;
  duration_ms?: number | null;
  output?: string | null;
  error_message?: string | null;
  restored_by?: number | null;
  created_at: string;
  updated_at: string;
};

export type BackupDestination = {
  id: string;
  name?: string | null;
  enabled: boolean;
};

export async function fetchBackupDestinations() {
  const response = await apiClient.get<ApiResponse<BackupDestination[]>>(
    "/admin/backups/destinations",
  );
  return unwrap(response.data);
}

export async function fetchBackups(params?: {
  page?: number;
  page_size?: number;
  status?: string;
  trigger_type?: string;
}) {
  const response = await apiClient.get<ApiResponse<PageResponse<BackupRecord>>>(
    "/admin/backups",
    { params },
  );
  return unwrap(response.data);
}

export async function createBackup(
  payload: {
    connection_id?: number | null;
    delivery_targets?: string[];
  } = {},
) {
  const response = await apiClient.post<ApiResponse<BackupRecord>>(
    "/admin/backups",
    payload,
  );
  return unwrap(response.data);
}

export async function deliverBackup(id: number) {
  const response = await apiClient.post<ApiResponse<BackupRecord>>(
    `/admin/backups/${id}/deliver`,
  );
  return unwrap(response.data);
}

export async function fetchBackupRestores(id: number) {
  const response = await apiClient.get<ApiResponse<RestoreRecord[]>>(
    `/admin/backups/${id}/restores`,
  );
  return unwrap(response.data);
}

export async function restoreBackup(
  id: number,
  payload: { confirm_phrase: string },
) {
  const response = await apiClient.post<ApiResponse<RestoreRecord>>(
    `/admin/backups/${id}/restore`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteBackup(id: number) {
  await apiClient.delete(`/admin/backups/${id}`);
}
