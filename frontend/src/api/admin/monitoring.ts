import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type MonitoringOverview = {
  db_ok: boolean;
  task_success_count: number;
  task_failed_count: number;
  backup_success_count: number;
  backup_failed_count: number;
  rate_limit_event_count: number;
  error_log_count: number;
  health_links: string[];
  server: ServerMonitorSnapshot;
};

export type ServerMonitorSnapshot = {
  captured_at: string;
  host: HostInfo;
  cpu: CpuInfo;
  memory: MemoryInfo;
  disks: DiskInfo[];
  networks: NetworkInfo[];
  gpus: GpuInfo[];
  gpu_available: boolean;
  gpu_message?: string | null;
  load?: LoadInfo | null;
};

export type HostInfo = {
  hostname?: string | null;
  system_name?: string | null;
  kernel_version?: string | null;
  os_version?: string | null;
  long_os_version?: string | null;
  architecture: string;
  uptime_seconds: number;
  boot_time_seconds: number;
  process_count: number;
};

export type CpuInfo = {
  global_usage: number;
  logical_cores: number;
  physical_cores?: number | null;
  brand?: string | null;
  frequency_mhz: number;
  cores: CpuCoreInfo[];
};

export type CpuCoreInfo = {
  name: string;
  usage: number;
  frequency_mhz: number;
};

export type MemoryInfo = {
  total_bytes: number;
  used_bytes: number;
  free_bytes: number;
  available_bytes: number;
  swap_total_bytes: number;
  swap_used_bytes: number;
  used_percent: number;
  swap_used_percent: number;
};

export type DiskInfo = {
  name: string;
  mount_point: string;
  file_system: string;
  total_bytes: number;
  available_bytes: number;
  used_bytes: number;
  used_percent: number;
  removable: boolean;
};

export type NetworkInfo = {
  interface: string;
  received_bytes: number;
  transmitted_bytes: number;
  received_packets: number;
  transmitted_packets: number;
  received_errors: number;
  transmitted_errors: number;
};

export type GpuInfo = {
  name: string;
  driver_version?: string | null;
  utilization_percent?: number | null;
  memory_total_bytes?: number | null;
  memory_used_bytes?: number | null;
  temperature_celsius?: number | null;
};

export type LoadInfo = {
  one: number;
  five: number;
  fifteen: number;
};

export type ProcessRecord = {
  pid: string;
  name: string;
  exe?: string | null;
  command: string;
  status: string;
  cpu_usage: number;
  memory_bytes: number;
  virtual_memory_bytes: number;
  run_time_seconds: number;
  start_time_seconds: number;
  user_id?: string | null;
};

export type ProcessQueryParams = {
  page?: number;
  page_size?: number;
  keyword?: string;
  sort?: string;
  order?: string;
};

export async function fetchMonitoringOverview() {
  const response = await apiClient.get<ApiResponse<MonitoringOverview>>(
    "/admin/monitoring/overview",
  );
  return unwrap(response.data);
}

export async function fetchServerMonitor() {
  const response = await apiClient.get<ApiResponse<ServerMonitorSnapshot>>(
    "/admin/monitoring/server",
  );
  return unwrap(response.data);
}

export async function fetchMonitorProcesses(params?: ProcessQueryParams) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<ProcessRecord>>
  >("/admin/monitoring/processes", { params });
  return unwrap(response.data);
}
