import { Drawer, Layout, Menu } from "antd";
import type { MenuProps } from "antd";
import { useMemo } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { type AdminMenuItem, defaultAdminMenus } from "../app/menu";
import { useAuthStore } from "../stores/auth";
import { renderMenuIcon } from "./icons";

const { Sider } = Layout;

function buildMenuItems(
  menus: AdminMenuItem[],
  hasPermission: (permission?: AdminMenuItem["permission"]) => boolean,
): MenuProps["items"] {
  return menus
    .filter((item) => hasPermission(item.permission))
    .map((item) => {
      const children = item.children?.length
        ? buildMenuItems(item.children, hasPermission)
        : undefined;

      return {
        key: item.path ?? item.key,
        icon: renderMenuIcon(item.icon ?? "menu"),
        label: item.label,
        children,
      };
    });
}

function findOpenKeys(pathname: string, menus: AdminMenuItem[]) {
  const keys: string[] = [];

  for (const item of menus) {
    if (item.children?.some((child) => child.path === pathname)) {
      keys.push(item.path ?? item.key);
    }
  }

  return keys;
}

function menuIdentity(item: AdminMenuItem) {
  return item.path ?? item.title ?? item.label ?? item.key;
}

function mergeMenus(primary: AdminMenuItem[], fallback: AdminMenuItem[]) {
  const merged = [...primary];

  for (const fallbackItem of fallback) {
    const fallbackIdentity = menuIdentity(fallbackItem);
    const index = merged.findIndex(
      (item) => menuIdentity(item) === fallbackIdentity,
    );
    if (index === -1) {
      merged.push(fallbackItem);
      continue;
    }

    merged[index] = {
      ...fallbackItem,
      ...merged[index],
      children: mergeMenus(
        merged[index].children ?? [],
        fallbackItem.children ?? [],
      ),
    };
  }

  return merged;
}

function SidebarContent({
  collapsed,
  onNavigate,
}: {
  collapsed: boolean;
  onNavigate?: () => void;
}) {
  const menus = useAuthStore((state) => state.menus);
  const hasPermission = useAuthStore((state) => state.hasPermission);
  const navigate = useNavigate();
  const location = useLocation();
  const visibleMenus = useMemo(
    () => mergeMenus(menus, defaultAdminMenus),
    [menus],
  );
  const items = useMemo(
    () => buildMenuItems(visibleMenus, hasPermission),
    [visibleMenus, hasPermission],
  );

  return (
    <>
      <div className="admin-brand">
        <div className="admin-brand-mark">G</div>
        {!collapsed && (
          <div>
            <div className="admin-brand-title">GPT Images</div>
            <div className="admin-brand-subtitle">Admin Template</div>
          </div>
        )}
      </div>
      <Menu
        theme="dark"
        mode="inline"
        selectedKeys={[location.pathname]}
        defaultOpenKeys={findOpenKeys(location.pathname, visibleMenus)}
        items={items}
        onClick={({ key }) => {
          if (key.startsWith("/")) {
            navigate(key);
            onNavigate?.();
          }
        }}
      />
    </>
  );
}

export function Sidebar({
  collapsed,
  isMobile,
  open,
  onClose,
}: {
  collapsed: boolean;
  isMobile: boolean;
  open: boolean;
  onClose: () => void;
}) {
  if (isMobile) {
    return (
      <Drawer
        open={open}
        onClose={onClose}
        placement="left"
        width={282}
        closable={false}
        className="admin-mobile-drawer"
        styles={{ body: { padding: 0, background: "#001529" } }}
      >
        <div className="admin-mobile-sidebar">
          <SidebarContent collapsed={false} onNavigate={onClose} />
        </div>
      </Drawer>
    );
  }

  return (
    <Sider collapsed={collapsed} width={256} className="admin-sidebar">
      <SidebarContent collapsed={collapsed} />
    </Sider>
  );
}
