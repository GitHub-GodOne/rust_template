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
    const token = useAuthStore.getState().accessToken;
    const isCurrentSessionRequest =
      Boolean(token) &&
      originalRequest?.headers?.Authorization === `Bearer ${token}`;
    let requestError = error;

    if (
      error.response?.status === 401 &&
      refreshToken &&
      isCurrentSessionRequest &&
      originalRequest &&
      !originalRequest._retry &&
      originalRequest?.url !== "/auth/refresh" &&
      originalRequest?.url !== "/auth/login"
    ) {
      originalRequest._retry = true;
      try {
        const data = await refreshSession(refreshToken);
        originalRequest.headers.Authorization = `Bearer ${data.token}`;
        return apiClient(originalRequest);
      } catch (refreshError) {
        requestError = refreshError;
        if (
          axios.isAxiosError(refreshError) &&
          refreshError.response?.status === 401 &&
          useAuthStore.getState().refreshToken === refreshToken
        ) {
          useAuthStore.getState().signOut();
        }
      }
    } else if (
      error.response?.status === 401 &&
      isCurrentSessionRequest &&
      originalRequest?.url !== "/auth/login"
    ) {
      useAuthStore.getState().signOut();
    }

    if (axios.isCancel(requestError)) return Promise.reject(requestError);
    if (!axios.isAxiosError(requestError)) return Promise.reject(requestError);
    const message =
      requestError.response?.data?.message ??
      (requestError.code === "ECONNABORTED"
        ? "请求超时，请稍后重试"
        : !requestError.response
          ? "无法连接服务器，请检查网络后重试"
          : "请求失败，请稍后重试");
    return Promise.reject(
      new ApiRequestError(
        message,
        requestError.response?.status,
        requestError.response?.data?.code,
      ),
    );
  },
);
