import axios from "axios";

type ApiEnvelope<T> = {
  success: boolean;
  code: string;
  message: string;
  data: T | null;
};

export type SetupStatus = {
  configured: boolean;
  phase: "setup" | "restarting" | "ready";
  environment: string;
  config_file: string | null;
  port: number | null;
};

export type SetupDatabase = {
  engine: "postgres";
  host: string;
  port: number;
  username: string;
  password: string;
  database: string;
  ssl_mode: "disable" | "prefer" | "require" | "verify-ca" | "verify-full";
};

export type DatabaseTestResult = {
  connected: boolean;
  engine: "postgres";
  database_exists: boolean;
  initialized: boolean;
};

export type SetupAdmin = {
  name: string;
  email: string;
  password: string;
};

export type InstallResult = {
  configured: boolean;
  config_file: string;
  migrations_applied: boolean;
  default_data_created: boolean;
  admin_created: boolean;
};

const setupClient = axios.create({
  baseURL: "/api/setup",
  timeout: 12_000,
});

function unwrap<T>(response: { data: ApiEnvelope<T> }): T {
  if (!response.data.success || response.data.data === null) {
    throw new Error(response.data.message || "请求失败");
  }
  return response.data.data;
}

function setupError(error: unknown): Error {
  if (axios.isAxiosError(error)) {
    const message = error.response?.data?.message;
    if (typeof message === "string" && message.trim()) {
      return new Error(message);
    }
    if (error.code === "ECONNABORTED") {
      return new Error("连接测试超时，请检查数据库地址和网络");
    }
    if (error.message === "Network Error") {
      return new Error("无法连接安装服务，请确认后端仍在运行");
    }
    return new Error(error.message || "请求失败");
  }
  return error instanceof Error ? error : new Error("请求失败");
}

export async function getSetupStatus(): Promise<SetupStatus> {
  try {
    return unwrap(
      await setupClient.get<ApiEnvelope<SetupStatus>>("/status", {
        timeout: 5_000,
      }),
    );
  } catch (error) {
    throw setupError(error);
  }
}

export async function testSetupDatabase(
  database: SetupDatabase,
): Promise<DatabaseTestResult> {
  try {
    return unwrap(
      await setupClient.post<ApiEnvelope<DatabaseTestResult>>("/test", {
        database,
      }),
    );
  } catch (error) {
    throw setupError(error);
  }
}

export async function completeSetup(input: {
  database: SetupDatabase;
  admin: SetupAdmin;
}): Promise<InstallResult> {
  try {
    return unwrap(
      await setupClient.post<ApiEnvelope<InstallResult>>("/complete", input, {
        timeout: 120_000,
      }),
    );
  } catch (error) {
    throw setupError(error);
  }
}
