import {
  LogoutOutlined,
  MenuFoldOutlined,
  MenuUnfoldOutlined,
  SettingOutlined,
  UserOutlined,
} from "@ant-design/icons";
import { Avatar, Button, Dropdown, Layout, Space, Typography } from "antd";
import type { MenuProps } from "antd";
import { useNavigate } from "react-router-dom";
import { logout } from "../api/auth";
import { useAuthStore } from "../stores/auth";

const { Header } = Layout;

export function HeaderBar({
  collapsed,
  onToggle,
}: {
  collapsed: boolean;
  onToggle: () => void;
}) {
  const user = useAuthStore((state) => state.user);
  const signOut = useAuthStore((state) => state.signOut);
  const navigate = useNavigate();

  const menuItems: MenuProps["items"] = [
    {
      key: "profile",
      icon: <UserOutlined />,
      label: "个人资料",
      disabled: true,
    },
    {
      key: "settings",
      icon: <SettingOutlined />,
      label: "系统设置",
      onClick: () => navigate("/admin/system/settings"),
    },
    {
      type: "divider",
    },
    {
      key: "logout",
      icon: <LogoutOutlined />,
      label: "退出登录",
      onClick: async () => {
        await logout();
        signOut();
        navigate("/login", { replace: true });
      },
    },
  ];

  return (
    <Header className="admin-header">
      <Space size={16}>
        <Button
          type="text"
          icon={collapsed ? <MenuUnfoldOutlined /> : <MenuFoldOutlined />}
          onClick={onToggle}
        />
        <div>
          <Typography.Text strong>后台管理模板</Typography.Text>
          <Typography.Text type="secondary" className="admin-header-subtitle">
            专业、组件化、可扩展
          </Typography.Text>
        </div>
      </Space>
      <Dropdown menu={{ items: menuItems }} placement="bottomRight">
        <Button type="text" className="admin-user-button">
          <Space>
            <Avatar size="small" icon={<UserOutlined />} />
            <span>{user?.name ?? "管理员"}</span>
          </Space>
        </Button>
      </Dropdown>
    </Header>
  );
}
