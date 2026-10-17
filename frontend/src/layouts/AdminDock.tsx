import {
  CheckOutlined,
  DownOutlined,
  EyeInvisibleOutlined,
  PlusOutlined,
  SettingOutlined,
  UpOutlined,
} from "@ant-design/icons";
import { Button, Dropdown, Tooltip, message } from "antd";
import type { MenuProps } from "antd";
import { useEffect, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { defaultAdminMenus, mergeMenuItems } from "../app/menu";
import { useAuthStore } from "../stores/auth";
import { useDockStore } from "../stores/dock";
import { useUiStore } from "../stores/ui";
import { DockBar, resolveDockItems } from "./dock/DockBar";
import {
  DOCK_PAGE_MODE_LABELS,
  DOCK_SIZES,
  type DockPageRulePatch,
  dockModeForPath,
  dockReservesSpaceForPath,
  withPageRule,
} from "./dock/dockConfig";

export const DOCK_SETTINGS_PATH = "/admin/system/dock";
const AUTO_HIDE_DELAY_MS = 450;

function isTouchDevice() {
  return Boolean(window.matchMedia?.("(pointer: coarse)").matches);
}

export function AdminDock() {
  const menus = useAuthStore((state) => state.menus);
  const user = useAuthStore((state) => state.user);
  const hasPermission = useAuthStore((state) => state.hasPermission);
  // Re-resolve items once the session's permissions arrive.
  useAuthStore((state) => state.permissions);
  const visible = useUiStore((state) => state.dockVisible);
  const toggleVisible = useUiStore((state) => state.toggleDockVisible);
  const config = useDockStore((state) => state.config);
  const loadDock = useDockStore((state) => state.load);
  const saveDock = useDockStore((state) => state.save);
  const location = useLocation();
  const navigate = useNavigate();
  const [revealed, setRevealed] = useState(false);
  const hideTimer = useRef<number>();

  useEffect(() => {
    if (user?.pid) void loadDock(user.pid);
  }, [loadDock, user?.pid]);

  const visibleMenus =
    menus.length > 0
      ? mergeMenuItems(menus, defaultAdminMenus)
      : defaultAdminMenus;
  const items = resolveDockItems(
    config.items,
    visibleMenus,
    hasPermission,
  ).filter((item) => !item.unavailable);

  // Each page can override the default: always shown, auto-hidden or no dock.
  const pageMode = dockModeForPath(config, location.pathname);
  const pageRule = config.pageRules.find(
    (rule) => rule.path === location.pathname,
  );
  const autoHide = pageMode === "autoHide" && !isTouchDevice();
  const shown = visible && config.enabled && pageMode !== "hide";
  // Pages can also opt out of the blank strip kept free for the dock.
  const reservesSpace = dockReservesSpaceForPath(config, location.pathname);

  // Tell the layout where the dock sits so content can leave room for it.
  useEffect(() => {
    const body = document.body;
    const reserve = shown && !autoHide && reservesSpace;
    for (const position of ["bottom", "left", "right"]) {
      body.classList.toggle(
        `dock-pos-${position}`,
        config.position === position,
      );
    }
    body.classList.toggle("dock-reserve", reserve);
    body.style.setProperty(
      "--dock-space",
      `${DOCK_SIZES[config.size].item + (config.showLabels ? 34 : 22)}px`,
    );
    return () => {
      body.classList.remove(
        "dock-reserve",
        "dock-pos-bottom",
        "dock-pos-left",
        "dock-pos-right",
      );
    };
  }, [
    autoHide,
    config.position,
    config.size,
    config.showLabels,
    reservesSpace,
    shown,
  ]);

  // A left dock sits beside the sidebar, not on top of it.
  useEffect(() => {
    if (config.position !== "left") return;
    const measure = () => {
      const main = document.querySelector(".admin-main-layout");
      const left = main ? Math.round(main.getBoundingClientRect().left) : 0;
      document.body.style.setProperty("--dock-left-offset", `${left}px`);
    };
    measure();
    const observer =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(measure);
    const main = document.querySelector(".admin-main-layout");
    if (main && observer) observer.observe(main);
    window.addEventListener("resize", measure);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, [config.position]);

  useEffect(() => () => window.clearTimeout(hideTimer.current), []);

  // Start tucked away whenever auto-hide applies (switching it on, or
  // arriving at a page that uses it), not wherever the last page left it.
  // biome-ignore lint/correctness/useExhaustiveDependencies: re-run on page change
  useEffect(() => {
    window.clearTimeout(hideTimer.current);
    setRevealed(false);
  }, [autoHide, location.pathname]);

  if (!config.enabled || pageMode === "hide") {
    return null;
  }

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

  const updatePageRule = async (patch: DockPageRulePatch, done: string) => {
    try {
      await saveDock(withPageRule(config, location.pathname, patch));
      message.success(done);
    } catch (error) {
      message.error(error instanceof Error ? error.message : "保存失败");
    }
  };
  const check = (active: boolean) =>
    active ? <CheckOutlined /> : <span className="admin-dock-menu-check" />;
  const pageMenu: MenuProps["items"] = [
    {
      type: "group",
      label: "显示方式",
      children: [
        {
          key: "page-default",
          icon: check(!pageRule?.mode),
          label: `跟随默认（${config.autoHide ? "自动隐藏" : "常驻显示"}）`,
          onClick: () =>
            void updatePageRule({ mode: null }, "此页面的显示方式已恢复为默认"),
        },
        ...(["show", "autoHide", "hide"] as const).map((mode) => ({
          key: `page-${mode}`,
          icon: check(pageRule?.mode === mode),
          label: DOCK_PAGE_MODE_LABELS[mode],
          onClick: () =>
            void updatePageRule(
              { mode },
              `此页面的 Dock 已设为「${DOCK_PAGE_MODE_LABELS[mode]}」`,
            ),
        })),
      ],
    },
    {
      type: "group",
      label: "预留空间（仅常驻显示时生效）",
      children: [
        {
          key: "reserve-default",
          icon: check(pageRule?.reserve === undefined),
          label: `跟随默认（${config.reserveSpace ? "预留" : "不预留"}）`,
          onClick: () =>
            void updatePageRule(
              { reserve: null },
              "此页面的预留空间已恢复为默认",
            ),
        },
        {
          key: "reserve-on",
          icon: check(pageRule?.reserve === true),
          label: "为 Dock 预留空白",
          onClick: () =>
            void updatePageRule({ reserve: true }, "此页面会为 Dock 预留空白"),
        },
        {
          key: "reserve-off",
          icon: check(pageRule?.reserve === false),
          label: "不预留，Dock 浮在内容上",
          onClick: () =>
            void updatePageRule(
              { reserve: false },
              "此页面不再为 Dock 预留空白",
            ),
        },
      ],
    },
  ];

  const reveal = () => {
    window.clearTimeout(hideTimer.current);
    setRevealed(true);
  };
  const scheduleHide = () => {
    if (!autoHide) return;
    window.clearTimeout(hideTimer.current);
    hideTimer.current = window.setTimeout(
      () => setRevealed(false),
      AUTO_HIDE_DELAY_MS,
    );
  };

  return (
    <>
      {autoHide ? (
        <div
          className={`admin-dock-hotzone is-${config.position}`}
          aria-hidden="true"
          onMouseEnter={reveal}
          onMouseLeave={scheduleHide}
        />
      ) : null}
      <Dropdown
        trigger={["contextMenu"]}
        menu={{
          items: [
            {
              key: "page",
              label: "当前页面",
              children: pageMenu,
            },
            { type: "divider" },
            {
              key: "settings",
              icon: <SettingOutlined />,
              label: "自定义 Dock…",
              onClick: () => navigate(DOCK_SETTINGS_PATH),
            },
            {
              key: "hide",
              icon: <EyeInvisibleOutlined />,
              label: "收起 Dock（所有页面）",
              onClick: toggleVisible,
            },
          ],
        }}
      >
        <div className="admin-dock-anchor">
          <DockBar
            config={{ ...config, autoHide }}
            items={items}
            activePath={location.pathname}
            revealed={revealed}
            onOpen={(path) => navigate(path)}
            onMouseEnter={reveal}
            onMouseLeave={scheduleHide}
            trailing={
              <>
                {items.length === 0 ? (
                  <Tooltip title="添加页面到 Dock">
                    <Button
                      type="text"
                      className="admin-dock-hide"
                      aria-label="添加页面到 Dock"
                      icon={<PlusOutlined />}
                      onClick={() => navigate(DOCK_SETTINGS_PATH)}
                    />
                  </Tooltip>
                ) : null}
                <span className="admin-dock-divider" />
                <Tooltip title="自定义 Dock">
                  <Button
                    type="text"
                    className="admin-dock-hide admin-dock-settings"
                    aria-label="自定义 Dock"
                    icon={<SettingOutlined />}
                    onClick={() => navigate(DOCK_SETTINGS_PATH)}
                  />
                </Tooltip>
                <Tooltip title="收起快捷 Dock">
                  <Button
                    type="text"
                    className="admin-dock-hide"
                    aria-label="收起快捷 Dock"
                    icon={<DownOutlined />}
                    onClick={toggleVisible}
                  />
                </Tooltip>
              </>
            }
          />
        </div>
      </Dropdown>
    </>
  );
}
