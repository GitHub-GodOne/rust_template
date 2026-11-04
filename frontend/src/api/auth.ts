import type { AdminMenuItem } from "../app/menu";
import {
  type AuthDataScope,
  type AuthDepartment,
  type AuthRole,
  type AuthTenant,
  useAuthStore,
} from "../stores/auth";
import { apiClient } from "./client";

export type ApiResponse<T> = {
  success: boolean;
  code: string;
  message: string;
  data: T | null;
};

export type LoginRequest = {
  email: string;
  password: string;
  captcha_id?: string;
  captcha_code?: string;
};

export type CaptchaChallenge = {
  /** The form should show the captcha before the first attempt. */
  required: boolean;
  captcha_id: string;
  /** `data:image/svg+xml;base64,…` */
  image: string;
};

export async function fetchCaptcha() {
  const response =
    await apiClient.get<ApiResponse<CaptchaChallenge>>("/auth/captcha");
  return unwrap(response.data);
}

export type LoginResponse = {
  token: string;
  refresh_token: string;
  pid: string;
  name: string;
  is_verified: boolean;
};

export type CurrentResponse = {
  pid: string;
  name: string;
  email: string;
  roles: AuthRole[];
  permissions: string[];
  menus: AdminMenuItem[];
  tenant?: AuthTenant | null;
  departments: AuthDepartment[];
  current_department?: AuthDepartment | null;
  data_scopes: AuthDataScope[];
  effective_data_scope: string;
};

export function unwrap<T>(response: ApiResponse<T>) {
  if (!response.success || !response.data) {
    throw new Error(response.message || "request failed");
  }

  return response.data;
}

export async function login(payload: LoginRequest) {
  const response = await apiClient.post<ApiResponse<LoginResponse>>(
    "/auth/login",
    payload,
  );
  return unwrap(response.data);
}

export async function current() {
  const response =
    await apiClient.get<ApiResponse<CurrentResponse>>("/auth/current");
  return unwrap(response.data);
}

export async function switchCurrentDepartment(departmentId: number | null) {
  await apiClient.post("/auth/current-department", {
    department_id: departmentId,
  });
}

export async function logout() {
  const refreshToken = useAuthStore.getState().refreshToken;
  if (!refreshToken) {
    return;
  }

  await apiClient
    .post("/auth/logout", { refresh_token: refreshToken })
    .catch(() => undefined);
}
