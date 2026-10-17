import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";

export type DashboardSystemStatus = {
  db_ok: boolean;
  version: string;
  environment: string;
  uptime_seconds: number;
};

export type DashboardUserStats = {
  total: number;
  new_today: number;
  locked: number;
};

export type DashboardWorkOrderStats = {
  active: number;
  unassigned: number;
  assigned_to_me: number;
  overdue: number;
};

export type DashboardPaymentStats = {
  today_orders: number;
  today_paid: number;
  pending: number;
};

export type DashboardNotificationStats = {
  total: number;
  unread: number;
};

export type DashboardActivityStats = {
  today_operations: number;
  today_logins: number;
  today_errors: number;
};

export type DashboardStats = {
  users?: DashboardUserStats | null;
  work_orders?: DashboardWorkOrderStats | null;
  payments?: DashboardPaymentStats | null;
  notifications?: DashboardNotificationStats | null;
  activity?: DashboardActivityStats | null;
};

export type DashboardTodoLevel = "info" | "warning" | "error";

export type DashboardTodo = {
  key: string;
  title: string;
  description: string;
  count: number;
  path: string;
  level: DashboardTodoLevel;
};

export type DashboardWorkOrder = {
  id: number;
  order_no: string;
  title: string;
  priority: string;
  status: string;
  due_at?: string | null;
  updated_at: string;
};

export type DashboardNotification = {
  id: number;
  title: string;
  level: string;
  category: string;
  read: boolean;
  created_at: string;
};

export type DashboardLog = {
  id: number;
  log_type: string;
  level: string;
  module: string;
  action: string;
  message: string;
  operator?: string | null;
  created_at: string;
};

export type DashboardTrendPoint = {
  date: string;
  total: number;
  errors: number;
};

export type DashboardLogin = {
  success: boolean;
  message: string;
  ip?: string | null;
  created_at: string;
};

export type DashboardOverview = {
  generated_at: string;
  system: DashboardSystemStatus;
  stats: DashboardStats;
  todos: DashboardTodo[];
  my_work_orders?: DashboardWorkOrder[] | null;
  recent_notifications?: DashboardNotification[] | null;
  recent_logs?: DashboardLog[] | null;
  activity_trend?: DashboardTrendPoint[] | null;
  recent_logins: DashboardLogin[];
};

export async function fetchDashboardOverview() {
  const response = await apiClient.get<ApiResponse<DashboardOverview>>(
    "/admin/dashboard/overview",
  );
  return unwrap(response.data);
}
