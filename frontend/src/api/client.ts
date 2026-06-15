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
      !originalRequest?._retry &&
      originalRequest?.url !== "/auth/refresh"
    ) {
      originalRequest._retry = true;
      try {
        const response = await axios.post<ApiEnvelope<RefreshResponse>>(
          "/api/auth/refresh",
          { refresh_token: refreshToken },
        );
        const data = response.data.data;
        if (!data) {
          throw new Error("empty refresh response");
        }

        useAuthStore.getState().setTokens({
          token: data.token,
          refreshToken: data.refresh_token,
        });
        originalRequest.headers.Authorization = `Bearer ${data.token}`;
        return apiClient(originalRequest);
      } catch {
        useAuthStore.getState().signOut();
      }
    } else if (error.response?.status === 401) {
      useAuthStore.getState().signOut();
    }

    const message =
      error.response?.data?.message ??
      (error.code === "ECONNABORTED" ? "请求超时" : error.message);
    return Promise.reject(new Error(message || "请求失败"));
  },
);
