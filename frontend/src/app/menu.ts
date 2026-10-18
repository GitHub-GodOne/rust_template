import { t } from "../i18n";
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
    key: "home",
    label: t("首页"),
    path: "/admin/dashboard",
    icon: "home",
    permission: "dashboard:view",
  },
  {
    key: "system",
    label: t("系统管理"),
    icon: "setting",
    children: [
      {
        key: "system-users",
        label: t("用户管理"),
        path: "/admin/system/users",
        icon: "user",
        permission: "system:user:list",
      },
      {
        key: "system-roles",
        label: t("角色管理"),
        path: "/admin/system/roles",
        icon: "team",
        permission: "system:role:list",
      },
      {
        key: "system-tenants",
        label: t("租户管理"),
        path: "/admin/system/tenants",
        icon: "apartment",
        permission: "system:tenant:list",
      },
      {
        key: "system-departments",
        label: t("部门管理"),
        path: "/admin/system/departments",
        icon: "branches",
        permission: "system:department:list",
      },
      {
        key: "system-menus",
        label: t("菜单管理"),
        path: "/admin/system/menus",
        icon: "menu",
        permission: "system:menu:list",
      },
      {
        key: "system-permissions",
        label: t("权限配置"),
        path: "/admin/system/permissions",
        icon: "safety",
        permission: "system:permission:list",
      },
      {
        key: "system-logs",
        label: t("日志中心"),
        path: "/admin/system/logs",
        icon: "file-search",
        permission: "system:log:list",
      },
      {
        key: "system-uploads",
        label: t("素材库"),
        path: "/admin/system/uploads",
        icon: "picture",
        permission: "system:upload:list",
      },
      {
        key: "system-upload-tasks",
        label: t("上传任务"),
        path: "/admin/system/upload-tasks",
        icon: "cloud-sync",
        permission: "system:upload:list",
      },
      {
        key: "system-files",
        label: t("文件管理"),
        path: "/admin/system/files",
        icon: "folder-open",
        permission: "system:file:list",
      },
      {
        key: "system-static-hosting",
        label: t("静态托管"),
        path: "/admin/system/static-hosting",
        icon: "global",
        permission: "system:static_host:list",
      },
      {
        key: "system-chat-messages",
        label: t("聊天管理"),
        path: "/admin/system/chat-messages",
        icon: "message",
        permission: "system:chat:list",
      },
      {
        key: "system-storage",
        label: t("存储配置"),
        path: "/admin/system/storage",
        icon: "cloud-server",
        permission: "system:storage:list",
      },
      {
        key: "system-settings",
        label: t("系统设置"),
        path: "/admin/system/settings",
        icon: "control",
        permission: "system:setting:list",
      },
      {
        key: "system-http-client",
        label: t("HTTP 客户端配置"),
        path: "/admin/system/http-client",
        icon: "api",
        permission: "system:http_client:config",
      },
      {
        key: "system-notifications",
        label: t("通知中心"),
        path: "/admin/system/notifications",
        icon: "notification",
        permission: "system:notification:list",
      },
      {
        key: "system-scheduled-tasks",
        label: t("定时任务"),
        path: "/admin/system/scheduled-tasks",
        icon: "schedule",
        permission: "system:scheduled_task:list",
      },
      {
        key: "system-backups",
        label: t("数据库备份"),
        path: "/admin/system/backups",
        icon: "database",
        permission: "system:backup:list",
      },
      {
        key: "system-docs",
        label: t("接口文档"),
        path: "/admin/system/docs",
        icon: "api",
        permission: "system:docs:view",
      },
      {
        key: "system-monitoring",
        label: t("系统监控"),
        path: "/admin/system/monitoring",
        icon: "monitor",
        permission: "system:monitor:view",
      },
      {
        key: "system-commands",
        label: t("命令管理"),
        path: "/admin/system/commands",
        icon: "code",
        permission: "system:command:list",
      },
      {
        key: "system-project-cli",
        label: t("项目命令"),
        path: "/admin/system/project-cli",
        icon: "build",
        permission: "system:project_cli:view",
      },
      {
        key: "system-api-keys",
        label: t("API 授权"),
        path: "/admin/system/api-keys",
        icon: "key",
        permission: "system:api_key:list",
      },
      {
        key: "system-ssh",
        label: t("SSH 管理"),
        path: "/admin/system/ssh",
        icon: "code",
        permission: "system:ssh:list",
      },
      {
        key: "system-vnc",
        label: t("VNC 管理"),
        path: "/admin/system/vnc",
        icon: "monitor",
        permission: "system:vnc:list",
      },
      {
        key: "system-rdp",
        label: t("RDP 管理"),
        path: "/admin/system/rdp",
        icon: "monitor",
        permission: "system:rdp:list",
      },
      {
        key: "system-certificates",
        label: t("证书与域名"),
        path: "/admin/system/certificates",
        icon: "safety-certificate",
        permission: "system:certificate:list",
      },
      {
        key: "system-dock",
        label: t("快捷 Dock"),
        path: "/admin/system/dock",
        icon: "appstore",
      },
      {
        key: "system-ddns",
        label: t("IPv6 动态解析"),
        path: "/admin/system/ddns",
        icon: "wifi",
        permission: "system:ddns:view",
      },
      {
        key: "system-ai-images",
        label: t("AI 图片生成"),
        path: "/admin/system/ai-images",
        icon: "picture",
        permission: "system:ai_image:list",
      },
      {
        key: "system-email-templates",
        label: t("邮箱模板"),
        path: "/admin/system/email-templates",
        icon: "mail",
        permission: "system:email_template:list",
      },
      {
        key: "system-work-orders",
        label: t("工单管理"),
        path: "/admin/system/work-orders",
        icon: "profile",
        permission: "system:work_order:list",
      },
      {
        key: "system-payments",
        label: t("支付管理"),
        path: "/admin/system/payments",
        icon: "credit-card",
        permission: "system:payment_order:list",
      },
      {
        key: "system-content",
        label: t("内容管理"),
        path: "/admin/system/content",
        icon: "read",
        permission: "system:content_article:list",
      },
    ],
  },
];

export function menuIdentity(item: AdminMenuItem) {
  return item.path ?? `${item.label}::${item.title ?? item.label}`;
}

export function cloneMenuItem(item: AdminMenuItem): AdminMenuItem {
  return {
    ...item,
    children: item.children?.map(cloneMenuItem),
  };
}

function collectMenuIdentities(
  items: AdminMenuItem[],
  identities = new Set<string>(),
) {
  for (const item of items) {
    identities.add(menuIdentity(item));
    collectMenuIdentities(item.children ?? [], identities);
  }
  return identities;
}

export function mergeMenuItems(
  sourceMenus: AdminMenuItem[],
  fallbackMenus: AdminMenuItem[],
): AdminMenuItem[] {
  const sourceIdentities = collectMenuIdentities(sourceMenus);

  const mergeLevel = (
    sourceLevel: AdminMenuItem[],
    fallbackLevel: AdminMenuItem[],
  ): AdminMenuItem[] => {
    const merged = sourceLevel.map(cloneMenuItem);

    for (const fallback of fallbackLevel) {
      const identity = menuIdentity(fallback);
      const existing = merged.find((item) => menuIdentity(item) === identity);
      if (existing) {
        if (fallback.children?.length) {
          existing.children = mergeLevel(
            existing.children ?? [],
            fallback.children,
          );
        }
        continue;
      }

      // The API may have moved this item under another parent. Do not restore
      // a second copy at its default location.
      if (sourceIdentities.has(identity)) {
        continue;
      }

      merged.push({
        ...fallback,
        children: fallback.children
          ? mergeLevel([], fallback.children)
          : undefined,
      });
    }

    return merged;
  };

  return mergeLevel(sourceMenus, fallbackMenus);
}

export function findMenuByPath(
  pathname: string,
  items = defaultAdminMenus,
): AdminMenuItem | undefined {
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
