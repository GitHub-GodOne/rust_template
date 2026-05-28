import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type BackupRecord = {
  id: number;
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

export async function createBackup() {
  const response =
    await apiClient.post<ApiResponse<BackupRecord>>("/admin/backups");
  return unwrap(response.data);
}

export async function deliverBackup(id: number) {
  const response = await apiClient.post<ApiResponse<BackupRecord>>(
    `/admin/backups/${id}/deliver`,
  );
  return unwrap(response.data);
}

export async function deleteBackup(id: number) {
  await apiClient.delete(`/admin/backups/${id}`);
}
