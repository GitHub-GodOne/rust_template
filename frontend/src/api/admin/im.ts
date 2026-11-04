import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";

export type ImContact = {
  id: number;
  name: string;
  email: string;
};

export type ImMember = {
  user_id: number;
  name: string;
  email: string;
  role: "owner" | "member" | string;
};

export type ImMessageKind = "text" | "image" | "file" | "system";

export type ImMessage = {
  id: number;
  conversation_id: number;
  sender_id?: number | null;
  sender_name?: string | null;
  kind: ImMessageKind;
  /** Text, or for `system` a JSON group event. */
  content: string;
  file_name?: string | null;
  mime_type?: string | null;
  size_bytes?: number | null;
  created_at: string;
};

export type ImConversation = {
  id: number;
  kind: "direct" | "group";
  name: string;
  owner_id?: number | null;
  peer_id?: number | null;
  viewer_id: number;
  member_count: number;
  unread_count: number;
  last_message?: ImMessage | null;
  last_message_at?: string | null;
  created_at: string;
};

export type ImConversationDetail = ImConversation & {
  members: ImMember[];
};

/** Payload of the `im` server-sent event. */
export type ImEvent =
  | {
      type: "message";
      conversation_id: number;
      message: ImMessage;
      /** Sent by the receiving user (from another tab or device). */
      mine: boolean;
    }
  | { type: "conversation"; conversation_id: number };

export const IM_CONVERSATIONS_KEY = ["im-conversations"] as const;
/** Window event the realtime stream re-dispatches `im` events as. */
export const IM_EVENT = "admin-im-event";

export async function fetchImContacts(keyword?: string) {
  const response = await apiClient.get<ApiResponse<ImContact[]>>(
    "/admin/im/contacts",
    { params: { keyword: keyword || undefined } },
  );
  return unwrap(response.data);
}

export async function fetchImConversations() {
  const response = await apiClient.get<ApiResponse<ImConversation[]>>(
    "/admin/im/conversations",
  );
  return unwrap(response.data);
}

export async function fetchImConversation(id: number) {
  const response = await apiClient.get<ApiResponse<ImConversationDetail>>(
    `/admin/im/conversations/${id}`,
  );
  return unwrap(response.data);
}

export async function openImDirect(userId: number) {
  const response = await apiClient.post<ApiResponse<ImConversationDetail>>(
    "/admin/im/conversations/direct",
    { user_id: userId },
  );
  return unwrap(response.data);
}

export async function createImGroup(payload: {
  name?: string;
  member_ids: number[];
}) {
  const response = await apiClient.post<ApiResponse<ImConversationDetail>>(
    "/admin/im/conversations/group",
    payload,
  );
  return unwrap(response.data);
}

export async function renameImGroup(id: number, name: string) {
  const response = await apiClient.put<ApiResponse<ImConversationDetail>>(
    `/admin/im/conversations/${id}`,
    { name },
  );
  return unwrap(response.data);
}

export async function inviteImMembers(id: number, userIds: number[]) {
  const response = await apiClient.post<ApiResponse<ImConversationDetail>>(
    `/admin/im/conversations/${id}/members`,
    { user_ids: userIds },
  );
  return unwrap(response.data);
}

/** Removes a member; passing your own id leaves the group. */
export async function removeImMember(id: number, userId: number) {
  await apiClient.delete(`/admin/im/conversations/${id}/members/${userId}`);
}

export async function dissolveImGroup(id: number) {
  await apiClient.delete(`/admin/im/conversations/${id}`);
}

export async function fetchImMessages(
  id: number,
  params: { before_id?: number; after_id?: number; limit?: number } = {},
) {
  const response = await apiClient.get<ApiResponse<ImMessage[]>>(
    `/admin/im/conversations/${id}/messages`,
    { params },
  );
  return response.data.data ?? [];
}

export async function sendImText(id: number, content: string) {
  const response = await apiClient.post<ApiResponse<ImMessage>>(
    `/admin/im/conversations/${id}/messages`,
    { content },
  );
  return unwrap(response.data);
}

export async function sendImFile(
  id: number,
  file: File,
  onProgress?: (percent: number) => void,
) {
  const formData = new FormData();
  formData.append("file", file);
  const response = await apiClient.post<ApiResponse<ImMessage>>(
    `/admin/im/conversations/${id}/files`,
    formData,
    {
      timeout: 0,
      onUploadProgress: (event) => {
        if (event.total && onProgress) {
          onProgress(Math.round((event.loaded / event.total) * 100));
        }
      },
    },
  );
  return unwrap(response.data);
}

export async function markImRead(id: number, messageId?: number) {
  await apiClient.post(`/admin/im/conversations/${id}/read`, {
    message_id: messageId,
  });
}

export async function fetchImFile(messageId: number, download = false) {
  const response = await apiClient.get(`/admin/im/messages/${messageId}/file`, {
    params: { download: download || undefined },
    responseType: "blob",
    timeout: 0,
  });
  return response.data as Blob;
}
