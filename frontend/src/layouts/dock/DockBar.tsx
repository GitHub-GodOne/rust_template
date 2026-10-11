import { Tooltip } from "antd";
import type { CSSProperties, ReactNode } from "react";
import type { AdminMenuItem, PermissionCode } from "../../app/menu";
import { findMenuByPath } from "../../app/menu";
import { renderMenuIcon } from "../icons";
import { DOCK_SIZES, type DockConfig, type DockItemConfig } from "./dockConfig";
import "./dock.css";

export type ResolvedDockItem = DockItemConfig & {
  label: string;
  icon?: string | null;
  /** Page no longer exists or the user lost access to it. */
  unavailable?: "missing" | "forbidden";
};

/** Attaches menu names/icons; marks pages the user can no longer open. */
export function resolveDockItems(
  items: DockItemConfig[],
  menus: AdminMenuItem[],
  hasPermission: (permission?: PermissionCode) => boolean,
): ResolvedDockItem[] {
  return items.map((item) => {
    const menu = findMenuByPath(item.path, menus);
    if (!menu) {
      return { ...item, label: item.path, unavailable: "missing" };
    }
    const permission = menu.permission ?? menu.permission_code;
    return {
      ...item,
      label: menu.label ?? menu.title ?? item.path,
      icon: menu.icon,
      unavailable: hasPermission(permission) ? undefined : "forbidden",
    };
  });
}

export function DockBar({
  config,
  items,
  activePath,
  onOpen,
  preview,
  revealed = true,
  trailing,
  onMouseEnter,
  onMouseLeave,
}: {
  config: DockConfig;
  items: ResolvedDockItem[];
  activePath?: string;
  onOpen?: (path: string) => void;
  /** Static rendering for the settings page. */
  preview?: boolean;
  revealed?: boolean;
  trailing?: ReactNode;
  onMouseEnter?: () => void;
  onMouseLeave?: () => void;
}) {
  const size = DOCK_SIZES[config.size];
  const vertical = config.position !== "bottom";
  const classes = [
    "admin-dock",
    `is-${config.position}`,
    `is-${config.size}`,
    config.magnify ? "is-magnify" : "",
    config.showLabels ? "has-labels" : "",
    preview ? "is-preview" : "",
    config.autoHide && !preview ? "is-autohide" : "",
    revealed ? "is-revealed" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <nav
      className={classes}
      aria-label="快捷页面"
      style={
        {
          "--dock-item": `${size.item}px`,
          "--dock-icon": `${size.icon}px`,
          "--dock-font": `${size.font}px`,
        } as CSSProperties
      }
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
    >
      <div className="admin-dock-items">
        {items.map((item) => {
          const active = activePath === item.path;
          return (
            <Tooltip
              key={item.path}
              title={config.showLabels ? null : item.label}
              placement={
                vertical
                  ? config.position === "left"
                    ? "right"
                    : "left"
                  : "top"
              }
            >
              <button
                type="button"
                className={`admin-dock-item${active ? " is-active" : ""}`}
                aria-label={item.label}
                aria-current={active ? "page" : undefined}
                onClick={() => onOpen?.(item.path)}
              >
                <span
                  className="admin-dock-item-icon"
                  style={{ "--dock-color": item.color } as CSSProperties}
                >
                  {renderMenuIcon(item.icon)}
                </span>
                {config.showLabels ? (
                  <span className="admin-dock-item-label">{item.label}</span>
                ) : null}
                <span className="admin-dock-active-dot" />
              </button>
            </Tooltip>
          );
        })}
      </div>
      {trailing}
    </nav>
  );
}
