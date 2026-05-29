import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type ContentCategoryRecord = {
  id: number;
  name: string;
  slug: string;
  description?: string | null;
  sort_order: number;
  enabled: boolean;
  created_by?: number | null;
  updated_by?: number | null;
  created_at: string;
  updated_at: string;
};

export type SaveContentCategoryParams = {
  name: string;
  slug: string;
  description?: string | null;
  sort_order?: number | null;
  enabled?: boolean | null;
};

export type ContentArticleRecord = {
  id: number;
  category_id: number;
  title: string;
  slug: string;
  summary?: string | null;
  content: string;
  cover_image_url?: string | null;
  status: string;
  is_featured: boolean;
  published_at?: string | null;
  seo_title?: string | null;
  seo_description?: string | null;
  created_by?: number | null;
  updated_by?: number | null;
  created_at: string;
  updated_at: string;
};

export type SaveContentArticleParams = {
  category_id: number;
  title: string;
  slug: string;
  summary?: string | null;
  content: string;
  cover_image_url?: string | null;
  status?: string | null;
  is_featured?: boolean | null;
  published_at?: string | null;
  seo_title?: string | null;
  seo_description?: string | null;
};

export async function fetchContentCategories(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
  enabled?: boolean;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<ContentCategoryRecord>>
  >("/admin/content-categories", { params });
  return unwrap(response.data);
}

export async function createContentCategory(
  payload: SaveContentCategoryParams,
) {
  const response = await apiClient.post<ApiResponse<ContentCategoryRecord>>(
    "/admin/content-categories",
    payload,
  );
  return unwrap(response.data);
}

export async function updateContentCategory(
  id: number,
  payload: SaveContentCategoryParams,
) {
  const response = await apiClient.put<ApiResponse<ContentCategoryRecord>>(
    `/admin/content-categories/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteContentCategory(id: number) {
  await apiClient.delete(`/admin/content-categories/${id}`);
}

export async function fetchContentArticles(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
  category_id?: number;
  status?: string;
  is_featured?: boolean;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<ContentArticleRecord>>
  >("/admin/content-articles", { params });
  return unwrap(response.data);
}

export async function createContentArticle(payload: SaveContentArticleParams) {
  const response = await apiClient.post<ApiResponse<ContentArticleRecord>>(
    "/admin/content-articles",
    payload,
  );
  return unwrap(response.data);
}

export async function updateContentArticle(
  id: number,
  payload: SaveContentArticleParams,
) {
  const response = await apiClient.put<ApiResponse<ContentArticleRecord>>(
    `/admin/content-articles/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function publishContentArticle(id: number) {
  const response = await apiClient.post<ApiResponse<ContentArticleRecord>>(
    `/admin/content-articles/${id}/publish`,
  );
  return unwrap(response.data);
}

export async function archiveContentArticle(id: number) {
  const response = await apiClient.post<ApiResponse<ContentArticleRecord>>(
    `/admin/content-articles/${id}/archive`,
  );
  return unwrap(response.data);
}

export async function deleteContentArticle(id: number) {
  await apiClient.delete(`/admin/content-articles/${id}`);
}
