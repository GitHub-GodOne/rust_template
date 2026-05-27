import { Grid, Layout, theme } from "antd";
import { useState } from "react";
import { Outlet } from "react-router-dom";
import { HeaderBar } from "./HeaderBar";
import { Sidebar } from "./Sidebar";

const { Content } = Layout;

export function AdminLayout() {
  const [collapsed, setCollapsed] = useState(false);
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);
  const screens = Grid.useBreakpoint();
  const isMobile = !screens.md;
  const {
    token: { borderRadiusLG },
  } = theme.useToken();

  return (
    <Layout className="admin-root">
      <Sidebar
        collapsed={collapsed}
        isMobile={isMobile}
        open={mobileMenuOpen}
        onClose={() => setMobileMenuOpen(false)}
      />
      <Layout>
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
        <Content
          className="admin-content"
          style={{ borderRadius: isMobile ? 0 : borderRadiusLG }}
        >
          <Outlet />
        </Content>
      </Layout>
    </Layout>
  );
}
