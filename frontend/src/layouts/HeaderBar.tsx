import {
  BranchesOutlined,
  FontSizeOutlined,
  LogoutOutlined,
  MenuFoldOutlined,
  MenuUnfoldOutlined,
  SettingOutlined,
  UserOutlined,
  ZoomInOutlined,
  ZoomOutOutlined,
} from "@ant-design/icons";
import {
  Avatar,
  Button,
  Dropdown,
  Layout,
  Select,
  Space,
  Typography,
  message,
} from "antd";
import type { MenuProps } from "antd";
import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { current, logout, switchCurrentDepartment } from "../api/auth";
import { useAuthStore } from "../stores/auth";
import { useUiStore } from "../stores/ui";

const { Header } = Layout;

export function HeaderBar({
  collapsed,
  onToggle,
}: {
  collapsed: boolean;
  onToggle: () => void;
}) {
  const user = useAuthStore((state) => state.user);
  const tenant = useAuthStore((state) => state.tenant);
  const departments = useAuthStore((state) => state.departments);
  const currentDepartment = useAuthStore((state) => state.currentDepartment);
  const signOut = useAuthStore((state) => state.signOut);
  const setSession = useAuthStore((state) => state.setSession);
  const contentZoom = useUiStore((state) => state.contentZoom);
  const zoomIn = useUiStore((state) => state.zoomIn);
  const zoomOut = useUiStore((state) => state.zoomOut);
  const resetZoom = useUiStore((state) => state.resetZoom);
  const [switchingDepartment, setSwitchingDepartment] = useState(false);
  const navigate = useNavigate();

  const showDepartmentSwitch =
    Boolean(tenant?.departments_enabled) && departments.length > 1;

  const handleDepartmentChange = async (departmentId?: number) => {
    setSwitchingDepartment(true);
    try {
      await switchCurrentDepartment(departmentId ?? null);
      const session = await current();
      setSession({
        user: {
          pid: session.pid,
          name: session.name,
          email: session.email,
        },
        roles: session.roles,
        permissions: session.permissions,
        menus: session.menus,
        tenant: session.tenant ?? null,
        departments: session.departments,
        currentDepartment: session.current_department ?? null,
        dataScopes: session.data_scopes,
        effectiveDataScope: session.effective_data_scope,
      });
      message.success("当前部门已切换");
    } catch {
      message.error("部门切换失败");
    } finally {
      setSwitchingDepartment(false);
    }
  };

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
      <Space size={12} className="admin-header-actions">
        <Space.Compact className="admin-zoom-control">
          <Button
            type="text"
            aria-label="缩小内容"
            icon={<ZoomOutOutlined />}
            onClick={zoomOut}
          />
          <Button
            type="text"
            aria-label="重置内容缩放"
            icon={<FontSizeOutlined />}
            onClick={resetZoom}
          >
            {Math.round(contentZoom * 100)}%
          </Button>
          <Button
            type="text"
            aria-label="放大内容"
            icon={<ZoomInOutlined />}
            onClick={zoomIn}
          />
        </Space.Compact>
        {showDepartmentSwitch ? (
          <Select
            allowClear
            className="admin-department-select"
            loading={switchingDepartment}
            placeholder="当前部门"
            value={currentDepartment?.id}
            suffixIcon={<BranchesOutlined />}
            options={departments.map((department) => ({
              value: department.id,
              label: department.name,
            }))}
            onChange={handleDepartmentChange}
          />
        ) : null}
        <Dropdown menu={{ items: menuItems }} placement="bottomRight">
          <Button type="text" className="admin-user-button">
            <Space>
              <Avatar size="small" icon={<UserOutlined />} />
              <span>{user?.name ?? "管理员"}</span>
            </Space>
          </Button>
        </Dropdown>
      </Space>
    </Header>
  );
}
