import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type ChatAttachmentSource = "material" | "local_file";

export type ChatAttachmentParams = {
  source_type: ChatAttachmentSource;
  upload_file_id?: number | null;
  file_root_key?: string | null;
  file_path?: string | null;
  display_name?: string | null;
  mime_type?: string | null;
  size_bytes?: number | null;
};

export type SaveChatMessageParams = {
  title: string;
  content: string;
  status?: string | null;
  sent_at?: string | null;
  remark?: string | null;
  attachments?: ChatAttachmentParams[];
};

export type ChatMessageRecord = {
  id: number;
  title: string;
  content: string;
  status: string;
  sent_at?: string | null;
  copy_count: number;
  last_copied_at?: string | null;
  remark?: string | null;
  attachment_count: number;
  created_by?: number | null;
  updated_by?: number | null;
  created_at: string;
  updated_at: string;
};

export type ChatAttachmentRecord = {
  id: number;
  source_type: ChatAttachmentSource;
  upload_file_id?: number | null;
  file_root_key?: string | null;
  file_path?: string | null;
  display_name: string;
  mime_type?: string | null;
  size_bytes: number;
  sort_order: number;
  material_url?: string | null;
  created_at: string;
};

export type ChatMessageDetailRecord = ChatMessageRecord & {
  attachments: ChatAttachmentRecord[];
};

export type CopyChatMessageRecord = {
  id: number;
  content: string;
  copy_count: number;
  last_copied_at?: string | null;
};

export async function fetchChatMessages(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
  status?: string;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<ChatMessageRecord>>
  >("/admin/chat-messages", { params });
  return unwrap(response.data);
}

export async function fetchChatMessage(id: number) {
  const response = await apiClient.get<ApiResponse<ChatMessageDetailRecord>>(
    `/admin/chat-messages/${id}`,
  );
  return unwrap(response.data);
}

export async function createChatMessage(payload: SaveChatMessageParams) {
  const response = await apiClient.post<ApiResponse<ChatMessageDetailRecord>>(
    "/admin/chat-messages",
    payload,
  );
  return unwrap(response.data);
}

export async function updateChatMessage(
  id: number,
  payload: SaveChatMessageParams,
) {
  const response = await apiClient.put<ApiResponse<ChatMessageDetailRecord>>(
    `/admin/chat-messages/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteChatMessage(id: number) {
  await apiClient.delete(`/admin/chat-messages/${id}`);
}

export async function markChatMessageSent(id: number) {
  const response = await apiClient.post<ApiResponse<ChatMessageRecord>>(
    `/admin/chat-messages/${id}/mark-sent`,
  );
  return unwrap(response.data);
}

export async function copyChatMessage(id: number) {
  const response = await apiClient.post<ApiResponse<CopyChatMessageRecord>>(
    `/admin/chat-messages/${id}/copy`,
  );
  return unwrap(response.data);
}
