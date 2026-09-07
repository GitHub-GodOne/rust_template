import { ConfigProvider, Grid, Layout, theme } from "antd";
import { type CSSProperties, useEffect, useState } from "react";
import { Outlet, useLocation } from "react-router-dom";
import { SshPage } from "../pages/system/ssh/SshPage";
import { VncPage } from "../pages/system/vnc/VncPage";
import { useAuthStore } from "../stores/auth";
import { useUiStore } from "../stores/ui";
import { AdminDock } from "./AdminDock";
import { AdminTabs } from "./AdminTabs";
import { HeaderBar } from "./HeaderBar";
import { MenuCommandPalette } from "./MenuCommandPalette";
import { Sidebar } from "./Sidebar";

const { Content } = Layout;

export function AdminLayout() {
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);
  const [menuSearchOpen, setMenuSearchOpen] = useState(false);
  const [sshPageMounted, setSshPageMounted] = useState(false);
  const [vncPageMounted, setVncPageMounted] = useState(false);
  const location = useLocation();
  const hasSshPermission = useAuthStore((state) =>
    state.hasPermission("system:ssh:list"),
  );
  const hasVncPermission = useAuthStore((state) =>
    state.hasPermission("system:vnc:list"),
  );
  const contentZoom = useUiStore((state) => state.contentZoom);
  const collapsed = useUiStore((state) => state.sidebarCollapsed);
  const dockVisible = useUiStore((state) => state.dockVisible);
  const toggleSidebarCollapsed = useUiStore(
    (state) => state.toggleSidebarCollapsed,
  );
  const screens = Grid.useBreakpoint();
  const isMobile = !screens.md;
  const effectiveContentZoom = contentZoom;
  const densityLevel = Math.round((effectiveContentZoom - 0.7) * 10);
  const densityFontSize = 11 + densityLevel;
  const densityControlHeight = 28 + Math.round((densityLevel * 8) / 3);
  const densityStyle = {
    "--admin-density-font-size": `${densityFontSize}px`,
    "--admin-density-table-font-size": `${densityFontSize}px`,
    "--admin-density-table-head-y": `${6 + densityLevel}px`,
    "--admin-density-table-cell-y": `${5 + densityLevel}px`,
    "--admin-density-table-x": `${7 + densityLevel}px`,
    "--admin-density-page-title": `${19 + densityLevel * 2}px`,
    "--admin-density-page-icon": `${32 + densityLevel * 2}px`,
  } as CSSProperties;
  const isSshPage = location.pathname === "/admin/system/ssh";
  const isVncPage = location.pathname === "/admin/system/vnc";
  const isKeepAlivePage = isSshPage || isVncPage;
  const {
    token: { borderRadiusLG },
  } = theme.useToken();

  useEffect(() => {
    if (isSshPage && hasSshPermission) {
      setSshPageMounted(true);
    }
    if (isVncPage && hasVncPermission) {
      setVncPageMounted(true);
    }
  }, [hasSshPermission, hasVncPermission, isSshPage, isVncPage]);

  return (
    <Layout className={`admin-root${dockVisible ? " admin-dock-visible" : ""}`}>
      <Sidebar
        collapsed={collapsed}
        isMobile={isMobile}
        open={mobileMenuOpen}
        onClose={() => setMobileMenuOpen(false)}
      />
      <Layout className="admin-main-layout">
        <HeaderBar
          collapsed={isMobile ? true : collapsed}
          onToggle={() => {
            if (isMobile) {
              setMobileMenuOpen(true);
            } else {
              toggleSidebarCollapsed();
            }
          }}
          onOpenMenuSearch={() => setMenuSearchOpen(true)}
        />
        <AdminTabs />
        <Content className="admin-content-shell">
          <div className="admin-content-frame">
            <ConfigProvider
              theme={{
                token: {
                  controlHeight: densityControlHeight,
                  fontSize: densityFontSize,
                },
              }}
            >
              <div className="admin-content-density" style={densityStyle}>
                <div
                  className={`admin-content${isKeepAlivePage ? " admin-content-immersive" : ""}`}
                  style={{ borderRadius: isMobile ? 0 : borderRadiusLG }}
                >
                  <div hidden={isKeepAlivePage}>
                    <Outlet />
                  </div>
                  {sshPageMounted && hasSshPermission ? (
                    <div hidden={!isSshPage}>
                      <SshPage visible={isSshPage} />
                    </div>
                  ) : null}
                  {vncPageMounted && hasVncPermission ? (
                    <div hidden={!isVncPage}>
                      <VncPage visible={isVncPage} />
                    </div>
                  ) : null}
                </div>
              </div>
            </ConfigProvider>
          </div>
        </Content>
      </Layout>
      <MenuCommandPalette
        open={menuSearchOpen}
        onOpenChange={setMenuSearchOpen}
      />
      <AdminDock />
    </Layout>
  );
}
