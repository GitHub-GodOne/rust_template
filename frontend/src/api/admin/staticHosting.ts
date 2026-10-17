import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";

export type StaticSiteRecord = {
  key: string;
  name: string;
  url_path: string;
  local_root: string;
  index_file: string;
  spa_fallback: boolean;
  directory_listing: boolean;
  enabled: boolean;
  cache_seconds: number;
};

export type SaveStaticSiteParams = {
  key: string;
  name: string;
  url_path: string;
  local_root: string;
  index_file?: string;
  spa_fallback?: boolean;
  directory_listing?: boolean;
  enabled?: boolean;
  cache_seconds?: number;
};

export type StaticFileRecord = {
  name: string;
  path: string;
  url: string;
  is_dir: boolean;
  extension?: string | null;
  mime_type?: string | null;
  size_bytes: number;
  updated_at?: string | null;
};

export type StaticBrowserRecord = {
  site: StaticSiteRecord;
  path: string;
  directories: StaticFileRecord[];
  files: StaticFileRecord[];
};

export async function fetchStaticSites() {
  const response = await apiClient.get<ApiResponse<StaticSiteRecord[]>>(
    "/admin/static-sites",
  );
  return unwrap(response.data);
}

export async function createStaticSite(payload: SaveStaticSiteParams) {
  const response = await apiClient.post<ApiResponse<StaticSiteRecord>>(
    "/admin/static-sites",
    payload,
  );
  return unwrap(response.data);
}

export async function updateStaticSite(
  key: string,
  payload: SaveStaticSiteParams,
) {
  const response = await apiClient.put<ApiResponse<StaticSiteRecord>>(
    `/admin/static-sites/${encodeURIComponent(key)}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteStaticSite(key: string) {
  await apiClient.delete(`/admin/static-sites/${encodeURIComponent(key)}`);
}

export async function fetchStaticBrowser(params: {
  site_key: string;
  path?: string | null;
}) {
  const response = await apiClient.get<ApiResponse<StaticBrowserRecord>>(
    "/admin/static-sites/browser",
    { params },
  );
  return unwrap(response.data);
}

export async function createStaticFolder(payload: {
  site_key: string;
  path: string;
}) {
  const response = await apiClient.post<ApiResponse<StaticFileRecord>>(
    "/admin/static-sites/folders",
    payload,
  );
  return unwrap(response.data);
}

export async function uploadStaticFile(
  file: File,
  options: {
    site_key: string;
    path: string;
    overwrite: boolean;
    onUploadProgress?: (percent: number) => void;
  },
) {
  const formData = new FormData();
  formData.append("site_key", options.site_key);
  formData.append("path", options.path);
  formData.append("overwrite", String(options.overwrite));
  formData.append("file", file, file.name);
  const response = await apiClient.post<ApiResponse<StaticFileRecord>>(
    "/admin/static-sites/upload",
    formData,
    {
      timeout: 0,
      onUploadProgress: (event) => {
        if (event.total && options.onUploadProgress) {
          options.onUploadProgress(
            Math.round((event.loaded / event.total) * 100),
          );
        }
      },
    },
  );
  return unwrap(response.data);
}

export async function deleteStaticFile(params: {
  site_key: string;
  path: string;
}) {
  await apiClient.delete("/admin/static-sites/files", { params });
}
