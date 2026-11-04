import { type ApiResponse, unwrap } from "./auth";
import { apiClient } from "./client";

export type ProfileRecord = {
  id: number;
  pid: string;
  name: string;
  email: string;
  status: string;
  is_verified: boolean;
  last_login_at?: string | null;
  last_login_ip?: string | null;
  password_changed_at?: string | null;
  created_at: string;
};

export type SessionRecord = {
  id: number;
  user_id: number;
  user_name?: string | null;
  user_email?: string | null;
  ip?: string | null;
  user_agent?: string | null;
  created_at: string;
  last_seen_at: string;
  expires_at: string;
  revoked_at?: string | null;
  revoked_reason?: string | null;
  active: boolean;
  /** The session making the request. */
  current: boolean;
};

export async function fetchProfile() {
  const response =
    await apiClient.get<ApiResponse<ProfileRecord>>("/user/profile");
  return unwrap(response.data);
}

export async function updateProfile(name: string) {
  const response = await apiClient.put<ApiResponse<ProfileRecord>>(
    "/user/profile",
    { name },
  );
  return unwrap(response.data);
}

export async function changePassword(payload: {
  current_password: string;
  new_password: string;
  sign_out_others: boolean;
}) {
  await apiClient.post("/user/profile/password", payload);
}

export async function fetchMySessions() {
  const response = await apiClient.get<ApiResponse<SessionRecord[]>>(
    "/user/profile/sessions",
  );
  return unwrap(response.data);
}

export async function revokeMySession(id: number) {
  await apiClient.delete(`/user/profile/sessions/${id}`);
}

export async function revokeMyOtherSessions() {
  await apiClient.post("/user/profile/sessions/revoke-others");
}

/** Confirms the password before a sensitive operation. */
export async function confirmPassword(password: string) {
  const response = await apiClient.post<ApiResponse<{ valid_minutes: number }>>(
    "/auth/reauth",
    { password },
  );
  return unwrap(response.data);
}
