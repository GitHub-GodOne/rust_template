import { MenuFoldOutlined, MenuUnfoldOutlined } from "@ant-design/icons";
import { Drawer, Layout, Menu, Tooltip } from "antd";
import type { MenuProps } from "antd";
import {
  type KeyboardEvent,
  type PointerEvent,
  useMemo,
  useRef,
  useState,
} from "react";
import { useLocation, useNavigate } from "react-router-dom";
import {
  type AdminMenuItem,
  defaultAdminMenus,
  mergeMenuItems,
} from "../app/menu";
import { useAuthStore } from "../stores/auth";
import {
  SIDEBAR_COLLAPSED_WIDTH,
  SIDEBAR_DEFAULT_WIDTH,
  SIDEBAR_MIN_WIDTH,
  useUiStore,
} from "../stores/ui";
import { renderMenuIcon } from "./icons";
import "./sidebar.css";

const { Sider } = Layout;

function buildMenuItems(
  menus: AdminMenuItem[],
  hasPermission: (permission?: AdminMenuItem["permission"]) => boolean,
): MenuProps["items"] {
  return menus
    .filter((item) => hasPermission(item.permission ?? item.permission_code))
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

function findOpenKeys(
  pathname: string,
  menus: AdminMenuItem[],
): string[] | undefined {
  for (const item of menus) {
    if (item.path === pathname) {
      return [];
    }

    if (item.children?.length) {
      const childKeys = findOpenKeys(pathname, item.children);
      if (childKeys) {
        return [item.path ?? item.key, ...childKeys];
      }
    }
  }

  return undefined;
}

function SidebarContent({
  collapsed,
  onNavigate,
  onToggleCollapsed,
}: {
  collapsed: boolean;
  onNavigate?: () => void;
  onToggleCollapsed?: () => void;
}) {
  const menus = useAuthStore((state) => state.menus);
  const userId = useAuthStore((state) => state.user?.pid ?? "anonymous");
  const hasPermission = useAuthStore((state) => state.hasPermission);
  const storedOpenKeys = useUiStore(
    (state) => state.sidebarOpenKeysByUser[userId],
  );
  const setSidebarOpenKeys = useUiStore((state) => state.setSidebarOpenKeys);
  const navigate = useNavigate();
  const location = useLocation();
  const visibleMenus = useMemo(
    () =>
      menus.length > 0
        ? mergeMenuItems(menus, defaultAdminMenus)
        : defaultAdminMenus,
    [menus],
  );
  const items = useMemo(
    () => buildMenuItems(visibleMenus, hasPermission),
    [visibleMenus, hasPermission],
  );
  const openKeys =
    storedOpenKeys ?? findOpenKeys(location.pathname, visibleMenus);

  return (
    <>
      <div className="admin-brand">
        {onToggleCollapsed && collapsed ? (
          <Tooltip title="展开导航" placement="right">
            <button
              type="button"
              className="admin-brand-mark admin-brand-mark-toggle"
              aria-label="展开导航"
              onClick={onToggleCollapsed}
            >
              <span className="admin-brand-mark-letter">G</span>
              <MenuUnfoldOutlined className="admin-brand-mark-icon" />
            </button>
          </Tooltip>
        ) : (
          <div className="admin-brand-mark">G</div>
        )}
        {!collapsed && (
          <div className="admin-brand-copy">
            <div className="admin-brand-title">后台管理</div>
            <div className="admin-brand-subtitle">Admin Template</div>
          </div>
        )}
        {onToggleCollapsed && !collapsed ? (
          <Tooltip title="收起导航" placement="right">
            <button
              type="button"
              className="admin-sidebar-collapse"
              aria-label="收起导航"
              onClick={onToggleCollapsed}
            >
              <MenuFoldOutlined />
            </button>
          </Tooltip>
        ) : null}
      </div>
      <Menu
        theme="dark"
        mode="inline"
        selectedKeys={[location.pathname]}
        openKeys={openKeys}
        items={items}
        onOpenChange={(keys) => setSidebarOpenKeys(userId, keys)}
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
  onToggleCollapsed,
}: {
  collapsed: boolean;
  isMobile: boolean;
  open: boolean;
  onClose: () => void;
  onToggleCollapsed: () => void;
}) {
  const sidebarWidth = useUiStore((state) => state.sidebarWidth);
  if (isMobile) {
    return (
      <Drawer
        open={open}
        onClose={onClose}
        placement="left"
        width={282}
        closable={false}
        className="admin-mobile-drawer"
        styles={{ body: { padding: 0, background: "#282623" } }}
      >
        <div className="admin-mobile-sidebar">
          <SidebarContent collapsed={false} onNavigate={onClose} />
        </div>
      </Drawer>
    );
  }

  return (
    <>
      <Sider
        collapsed={collapsed}
        collapsedWidth={SIDEBAR_COLLAPSED_WIDTH}
        width={sidebarWidth}
        className="admin-sidebar"
      >
        <SidebarContent
          collapsed={collapsed}
          onToggleCollapsed={onToggleCollapsed}
        />
      </Sider>
      <SidebarSplitter collapsed={collapsed} />
    </>
  );
}

/** Pixels the pointer must travel before a press counts as a drag. */
const DRAG_THRESHOLD = 4;
/** Dragging this far below the minimum width collapses the sidebar. */
const COLLAPSE_SNAP = 56;
const KEYBOARD_STEP = 16;

/**
 * Divider on the sidebar's right edge: drag to resize, click to collapse or
 * expand, double-click to restore the default width.
 */
function SidebarSplitter({ collapsed }: { collapsed: boolean }) {
  const width = useUiStore((state) => state.sidebarWidth);
  const setWidth = useUiStore((state) => state.setSidebarWidth);
  const setCollapsed = useUiStore((state) => state.setSidebarCollapsed);
  const [dragging, setDragging] = useState(false);
  const drag = useRef<{
    startX: number;
    startWidth: number;
    moved: boolean;
    wasCollapsed: boolean;
  } | null>(null);

  const finish = (pointerId: number, target: HTMLElement) => {
    if (target.hasPointerCapture(pointerId)) {
      target.releasePointerCapture(pointerId);
    }
    document.body.classList.remove("admin-sidebar-resizing");
    setDragging(false);
  };

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = {
      startX: event.clientX,
      startWidth: collapsed ? SIDEBAR_COLLAPSED_WIDTH : width,
      moved: false,
      wasCollapsed: collapsed,
    };
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const state = drag.current;
    if (!state) return;
    const delta = event.clientX - state.startX;
    if (!state.moved && Math.abs(delta) < DRAG_THRESHOLD) return;
    if (!state.moved) {
      state.moved = true;
      setDragging(true);
      document.body.classList.add("admin-sidebar-resizing");
    }
    const next = state.startWidth + delta;
    if (next < SIDEBAR_MIN_WIDTH - COLLAPSE_SNAP) {
      setCollapsed(true);
      return;
    }
    setCollapsed(false);
    setWidth(next);
  };

  const onPointerUp = (event: PointerEvent<HTMLDivElement>) => {
    const state = drag.current;
    drag.current = null;
    finish(event.pointerId, event.currentTarget);
    if (!state) return;
    if (state.moved) {
      setWidth(useUiStore.getState().sidebarWidth, true);
    } else {
      // A plain click toggles, like the collapse buttons in the header.
      setCollapsed(!state.wasCollapsed);
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      setCollapsed(!collapsed);
    } else if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      event.preventDefault();
      const step = event.key === "ArrowRight" ? KEYBOARD_STEP : -KEYBOARD_STEP;
      if (collapsed && step > 0) {
        setCollapsed(false);
        return;
      }
      setWidth(useUiStore.getState().sidebarWidth + step, true);
    }
  };

  return (
    <Tooltip
      title={
        collapsed
          ? "点击展开，拖动调整宽度"
          : "拖动调整宽度，点击收起，双击恢复默认"
      }
      placement="right"
      mouseEnterDelay={0.6}
      open={dragging ? false : undefined}
    >
      <div
        // biome-ignore lint/a11y/useSemanticElements: a focusable, draggable window splitter; <hr> cannot be interactive
        role="separator"
        aria-orientation="vertical"
        aria-label="调整导航宽度"
        aria-valuemin={SIDEBAR_COLLAPSED_WIDTH}
        aria-valuemax={420}
        aria-valuenow={collapsed ? SIDEBAR_COLLAPSED_WIDTH : width}
        tabIndex={0}
        className={`admin-sidebar-splitter${dragging ? " is-dragging" : ""}${collapsed ? " is-collapsed" : ""}`}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={(event) => {
          drag.current = null;
          finish(event.pointerId, event.currentTarget);
        }}
        onDoubleClick={() => {
          setCollapsed(false);
          setWidth(SIDEBAR_DEFAULT_WIDTH, true);
        }}
        onKeyDown={onKeyDown}
      >
        <span className="admin-sidebar-splitter-line" />
        <span className="admin-sidebar-splitter-grip">
          {collapsed ? <MenuUnfoldOutlined /> : <MenuFoldOutlined />}
        </span>
      </div>
    </Tooltip>
  );
}
