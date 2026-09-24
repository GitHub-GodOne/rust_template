import axios from "axios";
import { useAuthStore } from "../stores/auth";

type ApiEnvelope<T> = {
  success: boolean;
  code: string;
  message: string;
  data: T | null;
};

type RefreshResponse = {
  token: string;
  refresh_token: string;
};

export const apiClient = axios.create({
  baseURL: "/api",
  timeout: 15_000,
});

let refreshRequest: Promise<RefreshResponse> | null = null;

export class ApiRequestError extends Error {
  constructor(
    message: string,
    public readonly status?: number,
    public readonly code?: string,
  ) {
    super(message);
    this.name = "ApiRequestError";
  }
}

function refreshSession(refreshToken: string) {
  if (!refreshRequest) {
    refreshRequest = axios
      .post<ApiEnvelope<RefreshResponse>>(
        "/api/auth/refresh",
        { refresh_token: refreshToken },
        { timeout: 15_000 },
      )
      .then((response) => {
        const data = response.data.data;
        if (!data || useAuthStore.getState().refreshToken !== refreshToken) {
          throw new Error("登录状态已变化，请重新登录");
        }
        useAuthStore
          .getState()
          .setTokens({ token: data.token, refreshToken: data.refresh_token });
        return data;
      })
      .finally(() => {
        refreshRequest = null;
      });
  }
  return refreshRequest;
}

apiClient.interceptors.request.use((config) => {
  const token = useAuthStore.getState().accessToken;
  if (token) {
    config.headers.Authorization = `Bearer ${token}`;
  }

  return config;
});

apiClient.interceptors.response.use(
  (response) => response,
  async (error) => {
    const originalRequest = error.config;
    const refreshToken = useAuthStore.getState().refreshToken;

    if (
      error.response?.status === 401 &&
      refreshToken &&
      originalRequest &&
      !originalRequest._retry &&
      originalRequest?.url !== "/auth/refresh"
    ) {
      originalRequest._retry = true;
      try {
        const data = await refreshSession(refreshToken);
        originalRequest.headers.Authorization = `Bearer ${data.token}`;
        return apiClient(originalRequest);
      } catch {
        if (useAuthStore.getState().refreshToken === refreshToken) {
          useAuthStore.getState().signOut();
        }
      }
    } else if (error.response?.status === 401) {
      useAuthStore.getState().signOut();
    }

    if (axios.isCancel(error)) return Promise.reject(error);
    const message =
      error.response?.data?.message ??
      (error.code === "ECONNABORTED"
        ? "请求超时，请稍后重试"
        : !error.response
          ? "无法连接服务器，请检查网络后重试"
          : "请求失败，请稍后重试");
    return Promise.reject(
      new ApiRequestError(
        message,
        error.response?.status,
        error.response?.data?.code,
      ),
    );
  },
);
