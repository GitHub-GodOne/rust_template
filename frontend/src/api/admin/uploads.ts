import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type UploadRecord = {
  id: number;
  storage: string;
  storage_profile_id?: number | null;
  storage_bucket_id?: number | null;
  bucket?: string | null;
  prefix?: string | null;
  etag?: string | null;
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

export type StoragePrefixRecord = {
  prefix: string;
  name: string;
};

export type StorageObjectRecord = {
  key: string;
  name: string;
  prefix: string;
  url: string;
  size_bytes: number;
  updated_at?: string | null;
  etag?: string | null;
};

export type StorageBrowserRecord = {
  prefixes: StoragePrefixRecord[];
  objects: StorageObjectRecord[];
};

export type UpdateUploadParams = {
  category?: string | null;
  tags?: string | null;
  visibility?: string;
  status?: string;
};

export type UploadMaterialOptions = {
  storage_profile_id?: number;
  storage_bucket_id?: number;
  prefix?: string | null;
  category?: string | null;
  tags?: string | null;
  visibility?: string;
  onUploadProgress?: (percent: number) => void;
};

export type ImportUploadObjectParams = {
  storage_bucket_id: number;
  object_key: string;
  original_name?: string | null;
  mime_type?: string | null;
  category?: string | null;
  tags?: string | null;
  visibility?: string;
};

export type ImportUploadObjectsParams = {
  storage_bucket_id: number;
  prefix?: string | null;
  category?: string | null;
  tags?: string | null;
  visibility?: string;
};

export type ImportUploadObjectsRecord = {
  imported: number;
  skipped: number;
  items: UploadRecord[];
};

export type CreateUploadFolderParams = {
  storage_bucket_id: number;
  prefix: string;
};

export type RenameUploadParams = {
  original_name: string;
};

export type CreateUploadTaskParams = {
  storage_bucket_id: number;
  original_name: string;
  mime_type?: string | null;
  size_bytes: number;
  chunk_size: number;
  total_chunks: number;
  prefix?: string | null;
  category?: string | null;
  tags?: string | null;
  visibility?: string;
};

export type UploadTaskRecord = {
  id: number;
  storage: string;
  storage_profile_id?: number | null;
  storage_bucket_id?: number | null;
  bucket?: string | null;
  prefix?: string | null;
  object_key: string;
  original_name: string;
  filename: string;
  extension?: string | null;
  mime_type?: string | null;
  size_bytes: number;
  chunk_size: number;
  total_chunks: number;
  uploaded_chunks: number[];
  uploaded_bytes: number;
  category?: string | null;
  tags?: string | null;
  visibility: string;
  status: string;
  error_message?: string | null;
  completed_at?: string | null;
  upload_file_id?: number | null;
  uploader_id?: number | null;
  created_at: string;
  updated_at: string;
};

export async function fetchUploads(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
  category?: string;
  mime_type?: string;
  status?: string;
  storage_profile_id?: number;
  storage_bucket_id?: number;
  bucket?: string;
  prefix?: string;
}) {
  const response = await apiClient.get<ApiResponse<PageResponse<UploadRecord>>>(
    "/admin/uploads",
    { params },
  );
  return unwrap(response.data);
}

export async function fetchUploadBrowser(params?: {
  storage_bucket_id?: number;
  prefix?: string | null;
}) {
  const response = await apiClient.get<ApiResponse<StorageBrowserRecord>>(
    "/admin/uploads/browser",
    { params },
  );
  return unwrap(response.data);
}

export async function uploadMaterial(
  file: File,
  options: UploadMaterialOptions = {},
) {
  const { onUploadProgress, ...fields } = options;
  const formData = new FormData();
  formData.append("file", file);
  for (const [key, value] of Object.entries(fields)) {
    if (value !== undefined && value !== null && value !== "") {
      formData.append(key, String(value));
    }
  }
  const response = await apiClient.post<ApiResponse<UploadRecord>>(
    "/admin/uploads",
    formData,
    {
      timeout: 0,
      onUploadProgress: (event) => {
        if (event.total && onUploadProgress) {
          onUploadProgress(Math.round((event.loaded / event.total) * 100));
        }
      },
    },
  );
  return unwrap(response.data);
}

export async function importUploadObject(payload: ImportUploadObjectParams) {
  const response = await apiClient.post<ApiResponse<UploadRecord>>(
    "/admin/uploads/import-object",
    payload,
  );
  return unwrap(response.data);
}

export async function importUploadObjects(payload: ImportUploadObjectsParams) {
  const response = await apiClient.post<ApiResponse<ImportUploadObjectsRecord>>(
    "/admin/uploads/import-objects",
    payload,
  );
  return unwrap(response.data);
}

export async function createUploadFolder(payload: CreateUploadFolderParams) {
  const response = await apiClient.post<ApiResponse<StoragePrefixRecord>>(
    "/admin/uploads/folders",
    payload,
  );
  return unwrap(response.data);
}

export async function renameUpload(id: number, payload: RenameUploadParams) {
  const response = await apiClient.put<ApiResponse<UploadRecord>>(
    `/admin/uploads/${id}/rename`,
    payload,
  );
  return unwrap(response.data);
}

export async function fetchUploadTasks() {
  const response = await apiClient.get<ApiResponse<UploadTaskRecord[]>>(
    "/admin/uploads/tasks",
  );
  return unwrap(response.data);
}

export async function createUploadTask(payload: CreateUploadTaskParams) {
  const response = await apiClient.post<ApiResponse<UploadTaskRecord>>(
    "/admin/uploads/tasks",
    payload,
  );
  return unwrap(response.data);
}

export async function uploadTaskChunk(
  taskId: number,
  chunkIndex: number,
  chunk: Blob,
  onUploadProgress?: (percent: number) => void,
) {
  const formData = new FormData();
  formData.append("chunk", chunk);
  const response = await apiClient.post<ApiResponse<UploadTaskRecord>>(
    `/admin/uploads/tasks/${taskId}/chunks/${chunkIndex}`,
    formData,
    {
      timeout: 0,
      onUploadProgress: (event) => {
        if (event.total && onUploadProgress) {
          onUploadProgress(Math.round((event.loaded / event.total) * 100));
        }
      },
    },
  );
  return unwrap(response.data);
}

export async function completeUploadTask(taskId: number) {
  const response = await apiClient.post<ApiResponse<UploadTaskRecord>>(
    `/admin/uploads/tasks/${taskId}/complete`,
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

export async function previewUpload(id: number) {
  const response = await apiClient.get(`/admin/uploads/${id}/preview`, {
    responseType: "blob",
  });
  return response.data as Blob;
}
