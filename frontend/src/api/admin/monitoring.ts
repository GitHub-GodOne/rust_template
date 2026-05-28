import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";

export type MonitoringOverview = {
  db_ok: boolean;
  task_success_count: number;
  task_failed_count: number;
  backup_success_count: number;
  backup_failed_count: number;
  rate_limit_event_count: number;
  error_log_count: number;
  health_links: string[];
};

export async function fetchMonitoringOverview() {
  const response = await apiClient.get<ApiResponse<MonitoringOverview>>(
    "/admin/monitoring/overview",
  );
  return unwrap(response.data);
}
