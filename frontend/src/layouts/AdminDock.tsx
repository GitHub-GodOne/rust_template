import { DownOutlined, UpOutlined } from "@ant-design/icons";
import { Button, Tooltip } from "antd";
import type { CSSProperties } from "react";
import { useMemo } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { defaultAdminMenus, findMenuByPath, mergeMenuItems } from "../app/menu";
import { useAuthStore } from "../stores/auth";
import { useUiStore } from "../stores/ui";
import { renderMenuIcon } from "./icons";

const dockDestinations = [
  { path: "/admin/dashboard", color: "#5f625f" },
  { path: "/admin/system/uploads", color: "#c86743" },
  { path: "/admin/system/files", color: "#4f7f9f" },
  { path: "/admin/system/commands", color: "#666b73" },
  { path: "/admin/system/ssh", color: "#313336" },
  { path: "/admin/system/settings", color: "#8b8177" },
];

export function AdminDock() {
  const menus = useAuthStore((state) => state.menus);
  const roles = useAuthStore((state) => state.roles);
  const permissions = useAuthStore((state) => state.permissions);
  const visible = useUiStore((state) => state.dockVisible);
  const toggleVisible = useUiStore((state) => state.toggleDockVisible);
  const location = useLocation();
  const navigate = useNavigate();
  const visibleMenus = useMemo(
    () =>
      menus.length > 0
        ? mergeMenuItems(menus, defaultAdminMenus)
        : defaultAdminMenus,
    [menus],
  );
  const isSuperAdmin = roles.some((role) => role.code === "super_admin");
  const items = dockDestinations.flatMap((destination) => {
    const menu = findMenuByPath(destination.path, visibleMenus);
    const permission = menu?.permission ?? menu?.permission_code;
    if (
      !menu ||
      (permission && !isSuperAdmin && !permissions.includes(permission))
    ) {
      return [];
    }
    return [{ ...destination, menu }];
  });

  if (!visible) {
    return (
      <Tooltip title="显示快捷 Dock" placement="top">
        <Button
          type="text"
          className="admin-dock-reveal"
          aria-label="显示快捷 Dock"
          icon={<UpOutlined />}
          onClick={toggleVisible}
        />
      </Tooltip>
    );
  }

  return (
    <nav className="admin-dock" aria-label="快捷页面">
      <div className="admin-dock-items">
        {items.map(({ path, color, menu }) => {
          const active = location.pathname === path;
          return (
            <Tooltip key={path} title={menu.label} placement="top">
              <button
                type="button"
                className={`admin-dock-item${active ? " is-active" : ""}`}
                aria-label={menu.label}
                aria-current={active ? "page" : undefined}
                onClick={() => navigate(path)}
              >
                <span
                  className="admin-dock-item-icon"
                  style={{ "--dock-color": color } as CSSProperties}
                >
                  {renderMenuIcon(menu.icon)}
                </span>
                <span className="admin-dock-active-dot" />
              </button>
            </Tooltip>
          );
        })}
      </div>
      <span className="admin-dock-divider" />
      <Tooltip title="收起快捷 Dock" placement="top">
        <Button
          type="text"
          className="admin-dock-hide"
          aria-label="收起快捷 Dock"
          icon={<DownOutlined />}
          onClick={toggleVisible}
        />
      </Tooltip>
    </nav>
  );
}
