import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";

export type DataScopeRecord = {
  id: number;
  name: string;
  code: string;
  rule?: string | null;
  description?: string | null;
  created_at: string;
  updated_at: string;
};

export async function fetchDataScopes() {
  const response =
    await apiClient.get<ApiResponse<DataScopeRecord[]>>("/admin/data-scopes");
  return unwrap(response.data);
}
