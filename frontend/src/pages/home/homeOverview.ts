import type {
  DashboardTodo,
  DashboardTodoLevel,
  DashboardTrendPoint,
} from "../../api/admin/dashboard";
import type { AdminMenuItem, PermissionCode } from "../../app/menu";

export const workOrderStatusLabels: Record<string, string> = {
  open: "待处理",
  assigned: "已分配",
  in_progress: "处理中",
  pending: "挂起",
  resolved: "已解决",
  closed: "已关闭",
  cancelled: "已取消",
};

export const workOrderPriorityLabels: Record<string, string> = {
  low: "低",
  normal: "普通",
  high: "高",
  urgent: "紧急",
};

export const workOrderPriorityColors: Record<string, string> = {
  low: "default",
  normal: "blue",
  high: "orange",
  urgent: "red",
};

export const levelLabels: Record<string, string> = {
  info: "信息",
  success: "成功",
  warning: "警告",
  warn: "警告",
  error: "错误",
};

/** Paths shown as shortcuts, in display order, when the user may open them. */
export const quickEntryPaths = [
  "/admin/system/users",
  "/admin/system/roles",
  "/admin/system/menus",
  "/admin/system/work-orders",
  "/admin/system/payments",
  "/admin/system/notifications",
  "/admin/system/logs",
  "/admin/system/settings",
  "/admin/system/monitoring",
  "/admin/system/uploads",
  "/admin/system/backups",
  "/admin/system/docs",
];

const QUICK_ENTRY_LIMIT = 8;

export function greeting(hour: number) {
  if (hour < 5) return "夜深了";
  if (hour < 9) return "早上好";
  if (hour < 12) return "上午好";
  if (hour < 14) return "中午好";
  if (hour < 18) return "下午好";
  return "晚上好";
}

const weekdays = ["日", "一", "二", "三", "四", "五", "六"];

export function formatToday(date: Date) {
  return `${date.getFullYear()} 年 ${date.getMonth() + 1} 月 ${date.getDate()} 日 星期${weekdays[date.getDay()]}`;
}

export function formatUptime(seconds: number) {
  const total = Math.max(0, Math.floor(seconds));
  const days = Math.floor(total / 86_400);
  const hours = Math.floor((total % 86_400) / 3_600);
  const minutes = Math.floor((total % 3_600) / 60);
  if (days > 0) return `${days} 天 ${hours} 小时`;
  if (hours > 0) return `${hours} 小时 ${minutes} 分钟`;
  return `${Math.max(1, minutes)} 分钟`;
}

function pad(value: number) {
  return String(value).padStart(2, "0");
}

export function formatDateTime(value?: string | null) {
  if (!value) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function formatRelativeTime(value: string, now: Date = new Date()) {
  const date = new Date(value);
  const diff = now.getTime() - date.getTime();
  if (Number.isNaN(diff)) return value;
  const minutes = Math.floor(diff / 60_000);
  if (minutes < 1) return "刚刚";
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days} 天前`;
  return formatDateTime(value).slice(0, 10);
}

export function isOverdue(dueAt?: string | null, now: Date = new Date()) {
  if (!dueAt) return false;
  const due = new Date(dueAt).getTime();
  return !Number.isNaN(due) && due < now.getTime();
}

const levelRank: Record<DashboardTodoLevel, number> = {
  error: 0,
  warning: 1,
  info: 2,
};

/** Outstanding items first (most severe on top); cleared items keep their order. */
export function sortTodos(todos: DashboardTodo[]) {
  return todos
    .map((todo, index) => ({ todo, index }))
    .sort((left, right) => {
      const leftDone = left.todo.count === 0;
      const rightDone = right.todo.count === 0;
      if (leftDone !== rightDone) return leftDone ? 1 : -1;
      if (!leftDone) {
        const rank =
          (levelRank[left.todo.level] ?? 3) -
          (levelRank[right.todo.level] ?? 3);
        if (rank !== 0) return rank;
      }
      return left.index - right.index;
    })
    .map(({ todo }) => todo);
}

export function pendingTodoTotal(todos: DashboardTodo[]) {
  return todos.reduce((sum, todo) => sum + todo.count, 0);
}

/** Rounds the largest value up to a 1/2/5 × 10ⁿ step so axis ticks stay clean. */
export function niceMax(value: number) {
  if (value <= 0) return 1;
  const magnitude = 10 ** Math.floor(Math.log10(value));
  for (const step of [1, 2, 5, 10]) {
    if (value <= step * magnitude) return step * magnitude;
  }
  return 10 * magnitude;
}

export type TrendBar = DashboardTrendPoint & {
  label: string;
  isToday: boolean;
  normal: number;
  totalPercent: number;
  errorPercent: number;
};

export function buildTrendBars(points: DashboardTrendPoint[]) {
  const max = niceMax(Math.max(0, ...points.map((point) => point.total)));
  const bars: TrendBar[] = points.map((point, index) => {
    const errors = Math.min(point.errors, point.total);
    return {
      ...point,
      errors,
      label: index === points.length - 1 ? "今天" : point.date.slice(5),
      isToday: index === points.length - 1,
      normal: point.total - errors,
      totalPercent: (point.total / max) * 100,
      errorPercent: (errors / max) * 100,
    };
  });
  return { max, bars };
}

function flattenMenus(items: AdminMenuItem[]): AdminMenuItem[] {
  return items.flatMap((item) => [item, ...flattenMenus(item.children ?? [])]);
}

export function pickQuickEntries(
  menus: AdminMenuItem[],
  hasPermission: (permission?: PermissionCode) => boolean,
) {
  const byPath = new Map(
    flattenMenus(menus)
      .filter((item) => item.path)
      .map((item) => [item.path as string, item]),
  );
  return quickEntryPaths
    .map((path) => byPath.get(path))
    .filter((item): item is AdminMenuItem => {
      if (!item) return false;
      return hasPermission(item.permission ?? item.permission_code);
    })
    .slice(0, QUICK_ENTRY_LIMIT);
}
