import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";

export type FileRootRecord = {
  key: string;
  name: string;
  url_path: string;
  local_root: string;
  enabled: boolean;
};

export type ManagedFileRecord = {
  name: string;
  path: string;
  url: string;
  public_url?: string | null;
  visibility: string;
  requires_password: boolean;
  is_dir: boolean;
  extension?: string | null;
  mime_type?: string | null;
  size_bytes: number;
  updated_at?: string | null;
};

export type FileBrowserRecord = {
  root: FileRootRecord;
  path: string;
  directories: ManagedFileRecord[];
  files: ManagedFileRecord[];
};

export type CreateFileFolderParams = {
  root_key: string;
  path: string;
};

export type CreateManagedFileParams = {
  root_key: string;
  path: string;
  content?: string;
};

export type RenameFileParams = {
  root_key: string;
  path: string;
  name: string;
};

export type ManagedFileContentRecord = {
  content: string;
  size_bytes: number;
  updated_at?: string | null;
};

export type UpdateManagedFileContentParams = {
  root_key: string;
  path: string;
  content: string;
};

export type UpdateManagedFileAccessParams = {
  root_key: string;
  path: string;
  visibility: string;
  password?: string | null;
  clear_password?: boolean;
};

export async function fetchFileRoots() {
  const response =
    await apiClient.get<ApiResponse<FileRootRecord[]>>("/admin/files/roots");
  return unwrap(response.data);
}

export async function fetchFileBrowser(params: {
  root_key: string;
  path?: string | null;
}) {
  const response = await apiClient.get<ApiResponse<FileBrowserRecord>>(
    "/admin/files/browser",
    { params },
  );
  return unwrap(response.data);
}

export async function createFileFolder(payload: CreateFileFolderParams) {
  const response = await apiClient.post<ApiResponse<ManagedFileRecord>>(
    "/admin/files/folders",
    payload,
  );
  return unwrap(response.data);
}

export async function createManagedFile(payload: CreateManagedFileParams) {
  const response = await apiClient.post<ApiResponse<ManagedFileRecord>>(
    "/admin/files",
    payload,
  );
  return unwrap(response.data);
}

export async function uploadManagedFile(
  file: File,
  options: {
    root_key: string;
    path?: string | null;
    onUploadProgress?: (percent: number) => void;
  },
) {
  const formData = new FormData();
  formData.append("file", file);
  formData.append("root_key", options.root_key);
  if (options.path) {
    formData.append("path", options.path);
  }
  const response = await apiClient.post<ApiResponse<ManagedFileRecord>>(
    "/admin/files/upload",
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

export async function renameManagedFile(payload: RenameFileParams) {
  const response = await apiClient.put<ApiResponse<ManagedFileRecord>>(
    "/admin/files/rename",
    payload,
  );
  return unwrap(response.data);
}

export async function fetchManagedFileContent(params: {
  root_key: string;
  path: string;
}) {
  const response = await apiClient.get<ApiResponse<ManagedFileContentRecord>>(
    "/admin/files/content",
    { params },
  );
  return unwrap(response.data);
}

export async function updateManagedFileContent(
  payload: UpdateManagedFileContentParams,
) {
  const response = await apiClient.put<ApiResponse<ManagedFileRecord>>(
    "/admin/files/content",
    payload,
  );
  return unwrap(response.data);
}

export async function updateManagedFileAccess(
  payload: UpdateManagedFileAccessParams,
) {
  const response = await apiClient.put<ApiResponse<ManagedFileRecord>>(
    "/admin/files/access",
    payload,
  );
  return unwrap(response.data);
}

export async function deleteManagedFile(params: {
  root_key: string;
  path: string;
}) {
  await apiClient.delete("/admin/files", { params });
}

export async function previewManagedFile(params: {
  root_key: string;
  path: string;
}) {
  const response = await apiClient.get("/admin/files/preview", {
    params,
    responseType: "blob",
    timeout: 0,
  });
  return response.data as Blob;
}

export async function downloadManagedFile(params: {
  root_key: string;
  path: string;
}) {
  const response = await apiClient.get("/admin/files/download", {
    params,
    responseType: "blob",
    timeout: 0,
  });
  return response.data as Blob;
}
