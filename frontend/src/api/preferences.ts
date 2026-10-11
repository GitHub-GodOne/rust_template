import { type ApiResponse, unwrap } from "./auth";
import { apiClient } from "./client";

export type PreferenceRecord<T> = {
  key: string;
  value: T | null;
  updated_at?: string | null;
};

/** The signed-in user's own preference; `value` is null until first saved. */
export async function fetchPreference<T>(key: string) {
  const response = await apiClient.get<ApiResponse<PreferenceRecord<T>>>(
    `/user/preferences/${key}`,
  );
  return unwrap(response.data);
}

export async function savePreference<T>(key: string, value: T) {
  const response = await apiClient.put<ApiResponse<PreferenceRecord<T>>>(
    `/user/preferences/${key}`,
    { value },
  );
  return unwrap(response.data);
}

export async function resetPreference(key: string) {
  await apiClient.delete(`/user/preferences/${key}`);
}
