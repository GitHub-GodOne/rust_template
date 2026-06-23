import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type CommandTemplateRecord = {
  id: number;
  name: string;
  code: string;
  description?: string | null;
  working_directory: string;
  command: string;
  default_args?: string | null;
  env_vars?: string | null;
  setup_script?: string | null;
  python_venv_path?: string | null;
  timeout_seconds?: number | null;
  preview_path_template?: string | null;
  enabled: boolean;
  created_by?: number | null;
  updated_by?: number | null;
  created_at: string;
  updated_at: string;
};

export type CommandRunRecord = {
  id: number;
  template_id?: number | null;
  name: string;
  working_directory: string;
  command_line: string;
  effective_script: string;
  status: string;
  exit_code?: number | null;
  started_at?: string | null;
  finished_at?: string | null;
  duration_ms?: number | null;
  triggered_by: string;
  created_by?: number | null;
  error_message?: string | null;
  output_tail?: string | null;
  preview_path_template?: string | null;
  preview_path?: string | null;
  created_at: string;
  updated_at: string;
};

export type CommandRunLogRecord = {
  id: number;
  run_id: number;
  seq: number;
  stream: "stdout" | "stderr" | "system" | string;
  chunk: string;
  created_at: string;
};

export type CommandWorkflowStepRecord = {
  id: number;
  workflow_id: number;
  template_id: number;
  name: string;
  sort_order: number;
  args?: string | null;
  env_vars?: string | null;
  working_directory?: string | null;
  timeout_seconds?: number | null;
  enabled: boolean;
  created_at: string;
  updated_at: string;
};

export type CommandWorkflowRecord = {
  id: number;
  name: string;
  code: string;
  description?: string | null;
  enabled: boolean;
  created_by?: number | null;
  updated_by?: number | null;
  steps: CommandWorkflowStepRecord[];
  created_at: string;
  updated_at: string;
};

export type CommandWorkflowRunStepRecord = {
  id: number;
  workflow_run_id: number;
  workflow_step_id?: number | null;
  command_run_id?: number | null;
  step_name: string;
  sort_order: number;
  status: string;
  resolved_args?: string | null;
  started_at?: string | null;
  finished_at?: string | null;
  error_message?: string | null;
  created_at: string;
  updated_at: string;
};

export type CommandWorkflowRunRecord = {
  id: number;
  workflow_id?: number | null;
  name: string;
  status: string;
  started_at?: string | null;
  finished_at?: string | null;
  duration_ms?: number | null;
  created_by?: number | null;
  error_message?: string | null;
  steps: CommandWorkflowRunStepRecord[];
  created_at: string;
  updated_at: string;
};

export type SaveCommandTemplateParams = {
  name: string;
  code: string;
  description?: string | null;
  working_directory: string;
  command: string;
  default_args?: string | null;
  env_vars?: string | null;
  setup_script?: string | null;
  python_venv_path?: string | null;
  timeout_seconds?: number | null;
  preview_path_template?: string | null;
  enabled?: boolean;
};

export type RunCommandParams = {
  name?: string;
  working_directory?: string;
  command_line?: string;
  setup_script?: string | null;
  python_venv_path?: string | null;
  env_vars?: string | null;
  timeout_seconds?: number | null;
  preview_path_template?: string | null;
};

export type SaveCommandWorkflowStepParams = {
  id?: number | null;
  template_id: number;
  name: string;
  sort_order: number;
  args?: string | null;
  env_vars?: string | null;
  working_directory?: string | null;
  timeout_seconds?: number | null;
  enabled?: boolean;
};

export type SaveCommandWorkflowParams = {
  name: string;
  code: string;
  description?: string | null;
  enabled?: boolean;
  steps: SaveCommandWorkflowStepParams[];
};

export type RunCommandWorkflowParams = {
  name?: string;
};

export type CommandRunLogTicketRecord = {
  ticket: string;
  expires_at: string;
};

export async function fetchCommandTemplates(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
  enabled?: boolean;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<CommandTemplateRecord>>
  >("/admin/commands", { params });
  return unwrap(response.data);
}

export async function createCommandTemplate(
  payload: SaveCommandTemplateParams,
) {
  const response = await apiClient.post<ApiResponse<CommandTemplateRecord>>(
    "/admin/commands",
    payload,
  );
  return unwrap(response.data);
}

export async function updateCommandTemplate(
  id: number,
  payload: SaveCommandTemplateParams,
) {
  const response = await apiClient.put<ApiResponse<CommandTemplateRecord>>(
    `/admin/commands/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteCommandTemplate(id: number) {
  await apiClient.delete(`/admin/commands/${id}`);
}

export async function fetchCommandWorkflows(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
  enabled?: boolean;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<CommandWorkflowRecord>>
  >("/admin/command-workflows", { params });
  return unwrap(response.data);
}

export async function createCommandWorkflow(
  payload: SaveCommandWorkflowParams,
) {
  const response = await apiClient.post<ApiResponse<CommandWorkflowRecord>>(
    "/admin/command-workflows",
    payload,
  );
  return unwrap(response.data);
}

export async function updateCommandWorkflow(
  id: number,
  payload: SaveCommandWorkflowParams,
) {
  const response = await apiClient.put<ApiResponse<CommandWorkflowRecord>>(
    `/admin/command-workflows/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteCommandWorkflow(id: number) {
  await apiClient.delete(`/admin/command-workflows/${id}`);
}

export async function runCommandWorkflow(
  id: number,
  payload: RunCommandWorkflowParams,
) {
  const response = await apiClient.post<ApiResponse<CommandWorkflowRunRecord>>(
    `/admin/command-workflows/${id}/run`,
    payload,
  );
  return unwrap(response.data);
}

export async function runCommandTemplate(
  id: number,
  payload: RunCommandParams,
) {
  const response = await apiClient.post<ApiResponse<CommandRunRecord>>(
    `/admin/commands/${id}/run`,
    payload,
  );
  return unwrap(response.data);
}

export async function runAdHocCommand(payload: RunCommandParams) {
  const response = await apiClient.post<ApiResponse<CommandRunRecord>>(
    "/admin/command-runs",
    payload,
  );
  return unwrap(response.data);
}

export async function fetchCommandRuns(params?: {
  page?: number;
  page_size?: number;
  template_id?: number;
  status?: string;
  keyword?: string;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<CommandRunRecord>>
  >("/admin/command-runs", { params });
  return unwrap(response.data);
}

export async function fetchCommandRun(id: number) {
  const response = await apiClient.get<ApiResponse<CommandRunRecord>>(
    `/admin/command-runs/${id}`,
  );
  return unwrap(response.data);
}

export async function fetchCommandRunLogs(
  id: number,
  params?: { after_seq?: number; limit?: number },
) {
  const response = await apiClient.get<ApiResponse<CommandRunLogRecord[]>>(
    `/admin/command-runs/${id}/logs`,
    { params },
  );
  return unwrap(response.data);
}

export async function previewCommandRunArtifact(id: number) {
  const response = await apiClient.get(`/admin/command-runs/${id}/preview`, {
    responseType: "blob",
  });
  return response.data as Blob;
}

export async function createCommandRunLogTicket(id: number) {
  const response = await apiClient.post<ApiResponse<CommandRunLogTicketRecord>>(
    `/admin/command-runs/${id}/log-ticket`,
  );
  return unwrap(response.data);
}

export async function cancelCommandRun(id: number) {
  const response = await apiClient.post<ApiResponse<CommandRunRecord>>(
    `/admin/command-runs/${id}/cancel`,
  );
  return unwrap(response.data);
}

export async function fetchCommandWorkflowRuns(params?: {
  page?: number;
  page_size?: number;
  workflow_id?: number;
  status?: string;
  keyword?: string;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<CommandWorkflowRunRecord>>
  >("/admin/command-workflow-runs", { params });
  return unwrap(response.data);
}

export async function fetchCommandWorkflowRun(id: number) {
  const response = await apiClient.get<ApiResponse<CommandWorkflowRunRecord>>(
    `/admin/command-workflow-runs/${id}`,
  );
  return unwrap(response.data);
}

export function buildCommandRunLogWebSocketUrl(ticket: string) {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/api/admin/command-run-logs/${ticket}/ws`;
}
