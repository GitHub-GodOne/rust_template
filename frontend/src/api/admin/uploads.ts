import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type UploadRecord = {
  id: number;
  storage: string;
  object_key: string;
  url: string;
  original_name: string;
  filename: string;
  extension?: string | null;
  mime_type?: string | null;
  size_bytes: number;
  sha256: string;
  category?: string | null;
  tags?: string | null;
  visibility: "private" | "public" | string;
  status: "active" | "deleted" | string;
  uploader_id?: number | null;
  created_at: string;
  updated_at: string;
};

export type UpdateUploadParams = {
  category?: string | null;
  tags?: string | null;
  visibility?: string;
  status?: string;
};

export async function fetchUploads(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
  category?: string;
  mime_type?: string;
  status?: string;
}) {
  const response = await apiClient.get<ApiResponse<PageResponse<UploadRecord>>>(
    "/admin/uploads",
    { params },
  );
  return unwrap(response.data);
}

export async function uploadMaterial(file: File) {
  const formData = new FormData();
  formData.append("file", file);
  const response = await apiClient.post<ApiResponse<UploadRecord>>(
    "/admin/uploads",
    formData,
  );
  return unwrap(response.data);
}

export async function updateUpload(id: number, payload: UpdateUploadParams) {
  const response = await apiClient.put<ApiResponse<UploadRecord>>(
    `/admin/uploads/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteUpload(id: number) {
  await apiClient.delete(`/admin/uploads/${id}`);
}

export async function downloadUpload(id: number) {
  const response = await apiClient.get(`/admin/uploads/${id}/download`, {
    responseType: "blob",
  });
  return response.data as Blob;
}
