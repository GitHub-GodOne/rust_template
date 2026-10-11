import { Tabs } from "antd";
import { useEffect, useMemo, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { defaultAdminMenus, findMenuByPath, mergeMenuItems } from "../app/menu";
import { useAuthStore } from "../stores/auth";

type AdminTab = {
  key: string;
  label: string;
  closable: boolean;
};

const homePath = "/admin/dashboard";
const homeTab: AdminTab = { key: homePath, label: "首页", closable: false };

function normalizeAdminPath(pathname: string) {
  return pathname === "/admin" ? homePath : pathname;
}

function fallbackLabel(pathname: string) {
  return (
    pathname.split("/").filter(Boolean).at(-1)?.replaceAll("-", " ") ?? "页面"
  );
}

export function AdminTabs() {
  const menus = useAuthStore((state) => state.menus);
  const location = useLocation();
  const navigate = useNavigate();
  const visibleMenus = useMemo(
    () =>
      menus.length > 0
        ? mergeMenuItems(menus, defaultAdminMenus)
        : defaultAdminMenus,
    [menus],
  );
  const [tabs, setTabs] = useState<AdminTab[]>([homeTab]);
  const activePath = normalizeAdminPath(location.pathname);

  useEffect(() => {
    if (!activePath.startsWith("/admin")) {
      return;
    }

    const menu = findMenuByPath(activePath, visibleMenus);
    const nextTab: AdminTab = {
      key: activePath,
      label: menu?.label ?? fallbackLabel(activePath),
      closable: activePath !== homePath,
    };
    setTabs((current) => {
      if (current.some((tab) => tab.key === activePath)) {
        return current.map((tab) =>
          tab.key === activePath ? { ...tab, label: nextTab.label } : tab,
        );
      }
      return [...current, nextTab];
    });
  }, [activePath, visibleMenus]);

  const closeTab = (targetKey: string) => {
    if (targetKey === homePath) {
      return;
    }

    setTabs((current) => {
      const targetIndex = current.findIndex((tab) => tab.key === targetKey);
      const nextTabs = current.filter((tab) => tab.key !== targetKey);
      if (targetKey === activePath) {
        const nextActive =
          nextTabs[Math.max(0, targetIndex - 1)]?.key ?? homePath;
        navigate(nextActive, { replace: true });
      }
      return nextTabs.length > 0 ? nextTabs : [homeTab];
    });
  };

  return (
    <div className="admin-tabs-bar">
      <Tabs
        hideAdd
        type="editable-card"
        size="small"
        activeKey={activePath}
        items={tabs.map((tab) => ({
          key: tab.key,
          label: tab.label,
          closable: tab.closable,
        }))}
        onChange={(key) => navigate(key)}
        onEdit={(targetKey, action) => {
          if (action === "remove" && typeof targetKey === "string") {
            closeTab(targetKey);
          }
        }}
      />
    </div>
  );
}
