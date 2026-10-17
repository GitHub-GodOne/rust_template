import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";

export type CliArgKind = "flag" | "option" | "positional" | "task_param";
export type CliRisk = "safe" | "caution" | "danger";

export type CliArgSpec = {
  id: string;
  kind: CliArgKind;
  long?: string | null;
  short?: string | null;
  value_name?: string | null;
  help: string;
  required: boolean;
  multiple: boolean;
  optional_value: boolean;
  default_value?: string | null;
  possible_values: string[];
  secret: boolean;
};

export type CliCommandSpec = {
  path: string[];
  source: string;
  about: string;
  note?: string | null;
  runnable: boolean;
  risk: CliRisk;
  long_running: boolean;
  args: CliArgSpec[];
};

export type ProjectCliCatalog = {
  environment: string;
  binary: string;
  binary_name: string;
  working_directory: string;
  cargo_project: boolean;
  commands: CliCommandSpec[];
  warnings: string[];
  generated_at: string;
};

export type ProjectCliRunStatus =
  | "running"
  | "success"
  | "failed"
  | "cancelled";

export type ProjectCliRunSummary = {
  id: number;
  path: string[];
  args: string[];
  command_line: string;
  environment: string;
  status: ProjectCliRunStatus;
  exit_code?: number | null;
  error?: string | null;
  started_at: string;
  finished_at?: string | null;
  duration_ms?: number | null;
  created_by?: number | null;
  created_by_name: string;
  last_seq: number;
  output_bytes: number;
  truncated: boolean;
};

export type ProjectCliOutputChunk = {
  seq: number;
  stream: "stdout" | "stderr" | "system";
  text: string;
};

export type ProjectCliRunDetail = {
  run: ProjectCliRunSummary;
  output: ProjectCliOutputChunk[];
};

export type CliValue = boolean | string | string[];

export type RunProjectCliParams = {
  path: string[];
  values: Record<string, CliValue>;
  extra_args: string[];
};

export async function fetchProjectCliCatalog(refresh = false) {
  const response = await apiClient.get<ApiResponse<ProjectCliCatalog>>(
    "/admin/project-cli",
    { params: refresh ? { refresh: true } : undefined },
  );
  return unwrap(response.data);
}

export type ProjectCliRunPage = {
  items: ProjectCliRunSummary[];
  page: number;
  page_size: number;
  total: number;
};

export async function fetchProjectCliRuns(page = 1, pageSize = 20) {
  const response = await apiClient.get<ApiResponse<ProjectCliRunPage>>(
    "/admin/project-cli/runs",
    { params: { page, page_size: pageSize } },
  );
  return unwrap(response.data);
}

export async function runProjectCli(payload: RunProjectCliParams) {
  const response = await apiClient.post<ApiResponse<ProjectCliRunSummary>>(
    "/admin/project-cli/runs",
    payload,
  );
  return unwrap(response.data);
}

export async function fetchProjectCliRun(id: number, after = 0) {
  const response = await apiClient.get<ApiResponse<ProjectCliRunDetail>>(
    `/admin/project-cli/runs/${id}`,
    { params: { after } },
  );
  return unwrap(response.data);
}

export async function cancelProjectCliRun(id: number) {
  const response = await apiClient.post<ApiResponse<ProjectCliRunSummary>>(
    `/admin/project-cli/runs/${id}/cancel`,
  );
  return unwrap(response.data);
}
