export type PermissionCode = string;

export type MenuActions = {
  create: boolean;
  update: boolean;
  delete: boolean;
  import: boolean;
  export: boolean;
  print: boolean;
  help: boolean;
};

export type AdminMenuItem = {
  id?: number;
  key: string;
  label: string;
  title?: string;
  path?: string;
  icon?: string;
  permission?: PermissionCode;
  permission_code?: PermissionCode;
  actions?: MenuActions;
  children?: AdminMenuItem[];
};

export const defaultAdminMenus: AdminMenuItem[] = [
  {
    key: "dashboard",
    label: "控制台",
    path: "/admin/dashboard",
    icon: "dashboard",
    permission: "dashboard:view",
  },
  {
    key: "system",
    label: "系统管理",
    icon: "setting",
    children: [
      {
        key: "system-users",
        label: "用户管理",
        path: "/admin/system/users",
        icon: "user",
        permission: "system:user:list",
      },
      {
        key: "system-roles",
        label: "角色管理",
        path: "/admin/system/roles",
        icon: "team",
        permission: "system:role:list",
      },
      {
        key: "system-tenants",
        label: "租户管理",
        path: "/admin/system/tenants",
        icon: "apartment",
        permission: "system:tenant:list",
      },
      {
        key: "system-menus",
        label: "菜单管理",
        path: "/admin/system/menus",
        icon: "menu",
        permission: "system:menu:list",
      },
      {
        key: "system-permissions",
        label: "权限配置",
        path: "/admin/system/permissions",
        icon: "safety",
        permission: "system:permission:list",
      },
      {
        key: "system-logs",
        label: "日志中心",
        path: "/admin/system/logs",
        icon: "file-search",
        permission: "system:log:list",
      },
      {
        key: "system-uploads",
        label: "素材库",
        path: "/admin/system/uploads",
        icon: "cloud-upload",
        permission: "system:upload:list",
      },
      {
        key: "system-settings",
        label: "系统设置",
        path: "/admin/system/settings",
        icon: "control",
        permission: "system:setting:list",
      },
      {
        key: "system-notifications",
        label: "通知中心",
        path: "/admin/system/notifications",
        icon: "notification",
        permission: "system:notification:list",
      },
      {
        key: "system-scheduled-tasks",
        label: "定时任务",
        path: "/admin/system/scheduled-tasks",
        icon: "schedule",
        permission: "system:scheduled_task:list",
      },
      {
        key: "system-backups",
        label: "数据库备份",
        path: "/admin/system/backups",
        icon: "database",
        permission: "system:backup:list",
      },
      {
        key: "system-monitoring",
        label: "系统监控",
        path: "/admin/system/monitoring",
        icon: "monitor",
        permission: "system:monitor:view",
      },
      {
        key: "system-email-templates",
        label: "邮箱模板",
        path: "/admin/system/email-templates",
        icon: "mail",
        permission: "system:email_template:list",
      },
    ],
  },
];

export function findMenuByPath(pathname: string, items = defaultAdminMenus) {
  for (const item of items) {
    if (item.path === pathname) {
      return item;
    }

    if (item.children) {
      const child = findMenuByPath(pathname, item.children);
      if (child) {
        return child;
      }
    }
  }

  return undefined;
}
