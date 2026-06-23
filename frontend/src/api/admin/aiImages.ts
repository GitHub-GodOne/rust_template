import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type AiImageConfigRecord = {
  key: string;
  name: string;
  enabled: boolean;
  base_url: string;
  api_key_configured: boolean;
  model: string;
  size: string;
  quality: string;
  n: number;
  save_mode: string;
  local_output_dir?: string | null;
  storage_bucket_id?: number | null;
  storage_prefix?: string | null;
  description?: string | null;
};

export type SaveAiImageConfigParams = {
  key: string;
  name: string;
  enabled?: boolean;
  base_url: string;
  api_key?: string | null;
  model?: string | null;
  size?: string | null;
  quality?: string | null;
  n?: number | null;
  save_mode: string;
  local_output_dir?: string | null;
  storage_bucket_id?: number | null;
  storage_prefix?: string | null;
  description?: string | null;
};

export type AiImageGenerationRecord = {
  id: number;
  batch_id: string;
  config_key: string;
  config_name: string;
  prompt: string;
  model: string;
  size: string;
  quality: string;
  output_index: number;
  save_mode: string;
  storage_profile_id?: number | null;
  storage_bucket_id?: number | null;
  output_upload_file_id?: number | null;
  local_output_path?: string | null;
  original_name: string;
  mime_type?: string | null;
  status: string;
  error_message?: string | null;
  reference_summary?: string | null;
  reference_count: number;
  created_at: string;
  updated_at: string;
};

export type AiImageGenerationBatchRecord = {
  batch_id: string;
  items: AiImageGenerationRecord[];
};

export type GenerateAiImagesParams = {
  config_key: string;
  prompt: string;
  model?: string | null;
  size?: string | null;
  quality?: string | null;
  n?: number | null;
  reference_upload_ids?: number[];
  files?: File[];
};

export async function fetchAiImageConfigs() {
  const response = await apiClient.get<ApiResponse<AiImageConfigRecord[]>>(
    "/admin/ai-images/configs",
  );
  return unwrap(response.data);
}

export async function createAiImageConfig(payload: SaveAiImageConfigParams) {
  const response = await apiClient.post<ApiResponse<AiImageConfigRecord>>(
    "/admin/ai-images/configs",
    payload,
  );
  return unwrap(response.data);
}

export async function updateAiImageConfig(
  key: string,
  payload: SaveAiImageConfigParams,
) {
  const response = await apiClient.put<ApiResponse<AiImageConfigRecord>>(
    `/admin/ai-images/configs/${key}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteAiImageConfig(key: string) {
  await apiClient.delete(`/admin/ai-images/configs/${key}`);
}

export async function fetchAiImageGenerations(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<AiImageGenerationRecord>>
  >("/admin/ai-images/generations", { params });
  return unwrap(response.data);
}

export async function generateAiImages(payload: GenerateAiImagesParams) {
  const formData = new FormData();
  formData.append("config_key", payload.config_key);
  formData.append("prompt", payload.prompt);
  if (payload.model) {
    formData.append("model", payload.model);
  }
  if (payload.size) {
    formData.append("size", payload.size);
  }
  if (payload.quality) {
    formData.append("quality", payload.quality);
  }
  if (payload.n) {
    formData.append("n", String(payload.n));
  }
  for (const uploadId of payload.reference_upload_ids ?? []) {
    formData.append("reference_upload_ids[]", String(uploadId));
  }
  for (const file of payload.files ?? []) {
    formData.append("image", file);
  }
  const response = await apiClient.post<
    ApiResponse<AiImageGenerationBatchRecord>
  >("/admin/ai-images/generations", formData, { timeout: 0 });
  return unwrap(response.data);
}

export async function previewAiImageGeneration(id: number) {
  const response = await apiClient.get(
    `/admin/ai-images/generations/${id}/preview`,
    {
      responseType: "blob",
    },
  );
  return response.data as Blob;
}

export async function downloadAiImageGeneration(id: number) {
  const response = await apiClient.get(
    `/admin/ai-images/generations/${id}/download`,
    {
      responseType: "blob",
    },
  );
  return response.data as Blob;
}
