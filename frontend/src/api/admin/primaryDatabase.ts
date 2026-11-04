import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";

export type PrimaryDatabaseStatus = {
  engine: string;
  masked_uri: string;
  environment: string;
  override_file: string;
  restart_required: boolean;
};

export type ApplyPrimaryDatabaseResult = {
  engine: string;
  masked_uri: string;
  override_file: string;
  migrations_applied: boolean;
  reinitialized: boolean;
  restart_scheduled: boolean;
};

export async function fetchPrimaryDatabase() {
  const response = await apiClient.get<ApiResponse<PrimaryDatabaseStatus>>(
    "/admin/primary-database",
  );
  return unwrap(response.data);
}

export async function applyPrimaryDatabase(payload: {
  connection_id?: number | null;
  reinitialize?: boolean;
  confirm_phrase?: string;
  restart?: boolean;
}) {
  const response = await apiClient.post<
    ApiResponse<ApplyPrimaryDatabaseResult>
  >("/admin/primary-database/apply", payload);
  return unwrap(response.data);
}
