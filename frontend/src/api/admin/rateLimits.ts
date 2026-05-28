import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type RateLimitRuleRecord = {
  id: number;
  name: string;
  scope: string;
  path_pattern: string;
  method?: string | null;
  limit_count: number;
  window_seconds: number;
  enabled: boolean;
  description?: string | null;
  created_at: string;
  updated_at: string;
};

export type RateLimitEventRecord = {
  id: number;
  ip: string;
  method: string;
  path: string;
  rule_id?: number | null;
  user_id?: number | null;
  occurred_at: string;
  created_at: string;
  updated_at: string;
};

export type SaveRateLimitRuleParams = {
  name: string;
  scope: string;
  path_pattern: string;
  method?: string | null;
  limit_count: number;
  window_seconds: number;
  enabled?: boolean;
  description?: string | null;
};

export async function fetchRateLimitRules(params?: {
  page?: number;
  page_size?: number;
  enabled?: boolean;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<RateLimitRuleRecord>>
  >("/admin/rate-limits", { params });
  return unwrap(response.data);
}

export async function createRateLimitRule(payload: SaveRateLimitRuleParams) {
  const response = await apiClient.post<ApiResponse<RateLimitRuleRecord>>(
    "/admin/rate-limits",
    payload,
  );
  return unwrap(response.data);
}

export async function updateRateLimitRule(
  id: number,
  payload: SaveRateLimitRuleParams,
) {
  const response = await apiClient.put<ApiResponse<RateLimitRuleRecord>>(
    `/admin/rate-limits/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteRateLimitRule(id: number) {
  await apiClient.delete(`/admin/rate-limits/${id}`);
}

export async function fetchRateLimitEvents(params?: {
  page?: number;
  page_size?: number;
  rule_id?: number;
  ip?: string;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<RateLimitEventRecord>>
  >("/admin/rate-limit-events", { params });
  return unwrap(response.data);
}
