import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type StorageProvider = "local" | "s3_compatible" | string;

export type StorageProfileRecord = {
  id: number;
  tenant_id: number;
  name: string;
  code: string;
  provider: StorageProvider;
  enabled: boolean;
  is_default: boolean;
  endpoint?: string | null;
  region?: string | null;
  access_key_id?: string | null;
  secret_access_key?: string | null;
  public_base_url?: string | null;
  path_style: boolean;
  description?: string | null;
  created_at: string;
  updated_at: string;
};

export type StorageBucketRecord = {
  id: number;
  storage_profile_id: number;
  tenant_id: number;
  name: string;
  bucket: string;
  base_prefix?: string | null;
  local_root?: string | null;
  public_prefix?: string | null;
  enabled: boolean;
  is_default: boolean;
  created_at: string;
  updated_at: string;
};

export type SaveStorageProfileParams = {
  tenant_id?: number;
  name: string;
  code: string;
  provider: StorageProvider;
  enabled?: boolean;
  is_default?: boolean;
  endpoint?: string | null;
  region?: string | null;
  access_key_id?: string | null;
  secret_access_key?: string | null;
  public_base_url?: string | null;
  path_style?: boolean;
  description?: string | null;
};

export type SaveStorageBucketParams = {
  tenant_id?: number;
  name: string;
  bucket: string;
  base_prefix?: string | null;
  local_root?: string | null;
  public_prefix?: string | null;
  enabled?: boolean;
  is_default?: boolean;
};

export type StorageTestRecord = {
  ok: boolean;
  message: string;
};

export async function fetchStorageProfiles(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<StorageProfileRecord>>
  >("/admin/storage-profiles", { params });
  return unwrap(response.data);
}

export async function createStorageProfile(payload: SaveStorageProfileParams) {
  const response = await apiClient.post<ApiResponse<StorageProfileRecord>>(
    "/admin/storage-profiles",
    payload,
  );
  return unwrap(response.data);
}

export async function updateStorageProfile(
  id: number,
  payload: SaveStorageProfileParams,
) {
  const response = await apiClient.put<ApiResponse<StorageProfileRecord>>(
    `/admin/storage-profiles/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteStorageProfile(id: number) {
  await apiClient.delete(`/admin/storage-profiles/${id}`);
}

export async function testStorageProfile(id: number) {
  const response = await apiClient.post<ApiResponse<StorageTestRecord>>(
    `/admin/storage-profiles/${id}/test`,
  );
  return unwrap(response.data);
}

export async function fetchStorageBuckets(profileId: number) {
  const response = await apiClient.get<ApiResponse<StorageBucketRecord[]>>(
    `/admin/storage-profiles/${profileId}/buckets`,
  );
  return unwrap(response.data);
}

export async function createStorageBucket(
  profileId: number,
  payload: SaveStorageBucketParams,
) {
  const response = await apiClient.post<ApiResponse<StorageBucketRecord>>(
    `/admin/storage-profiles/${profileId}/buckets`,
    payload,
  );
  return unwrap(response.data);
}

export async function updateStorageBucket(
  id: number,
  payload: SaveStorageBucketParams,
) {
  const response = await apiClient.put<ApiResponse<StorageBucketRecord>>(
    `/admin/storage-buckets/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteStorageBucket(id: number) {
  await apiClient.delete(`/admin/storage-buckets/${id}`);
}
