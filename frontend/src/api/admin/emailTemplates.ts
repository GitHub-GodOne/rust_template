import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type EmailTemplateRecord = {
  id: number;
  code: string;
  name: string;
  template_type: "auth" | "system" | "marketing" | string;
  subject: string;
  html_body: string;
  text_body: string;
  variables: string;
  enabled: boolean;
  is_builtin: boolean;
  description?: string | null;
  created_by?: number | null;
  updated_by?: number | null;
  created_at: string;
  updated_at: string;
};

export type SaveEmailTemplateParams = {
  code: string;
  name: string;
  template_type: string;
  subject: string;
  html_body: string;
  text_body: string;
  variables: string;
  enabled?: boolean;
  is_builtin?: boolean;
  description?: string | null;
};

export type RenderedEmailTemplate = {
  subject: string;
  html_body: string;
  text_body: string;
};

export async function fetchEmailTemplates(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
  template_type?: string;
  enabled?: boolean;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<EmailTemplateRecord>>
  >("/admin/email-templates", { params });
  return unwrap(response.data);
}

export async function createEmailTemplate(payload: SaveEmailTemplateParams) {
  const response = await apiClient.post<ApiResponse<EmailTemplateRecord>>(
    "/admin/email-templates",
    payload,
  );
  return unwrap(response.data);
}

export async function updateEmailTemplate(
  id: number,
  payload: SaveEmailTemplateParams,
) {
  const response = await apiClient.put<ApiResponse<EmailTemplateRecord>>(
    `/admin/email-templates/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteEmailTemplate(id: number) {
  await apiClient.delete(`/admin/email-templates/${id}`);
}

export async function previewEmailTemplate(id: number, locals: unknown) {
  const response = await apiClient.post<ApiResponse<RenderedEmailTemplate>>(
    `/admin/email-templates/${id}/preview`,
    { locals },
  );
  return unwrap(response.data);
}

export async function testSendEmailTemplate(
  id: number,
  payload: { to: string; locals: unknown },
) {
  await apiClient.post(`/admin/email-templates/${id}/test-send`, payload);
}
