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
