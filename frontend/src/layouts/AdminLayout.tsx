import { Grid, Layout, theme } from "antd";
import { type CSSProperties, useEffect, useState } from "react";
import { Outlet, useLocation } from "react-router-dom";
import { SshPage } from "../pages/system/ssh/SshPage";
import { VncPage } from "../pages/system/vnc/VncPage";
import { useAuthStore } from "../stores/auth";
import { useUiStore } from "../stores/ui";
import { AdminTabs } from "./AdminTabs";
import { HeaderBar } from "./HeaderBar";
import { Sidebar } from "./Sidebar";

const { Content } = Layout;

export function AdminLayout() {
  const [collapsed, setCollapsed] = useState(false);
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);
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
  const screens = Grid.useBreakpoint();
  const isMobile = !screens.md;
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
    <Layout className="admin-root">
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
              setCollapsed((value) => !value);
            }
          }}
        />
        <AdminTabs />
        <Content className="admin-content-shell">
          <div
            className="admin-content"
            style={
              {
                "--admin-content-zoom": contentZoom,
                borderRadius: isMobile ? 0 : borderRadiusLG,
              } as CSSProperties
            }
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
        </Content>
      </Layout>
    </Layout>
  );
}
