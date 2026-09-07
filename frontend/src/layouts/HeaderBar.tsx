import {
  BranchesOutlined,
  FontSizeOutlined,
  LogoutOutlined,
  MenuFoldOutlined,
  MenuUnfoldOutlined,
  SearchOutlined,
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
  Tooltip,
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
  onOpenMenuSearch,
}: {
  collapsed: boolean;
  onToggle: () => void;
  onOpenMenuSearch: () => void;
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
    ...(showDepartmentSwitch
      ? [
          {
            type: "group" as const,
            label: "切换部门",
            children: departments.map((department) => ({
              key: `department-${department.id}`,
              icon: <BranchesOutlined />,
              label:
                department.id === currentDepartment?.id
                  ? `${department.name}（当前）`
                  : department.name,
              disabled:
                switchingDepartment || department.id === currentDepartment?.id,
              onClick: () => handleDepartmentChange(department.id),
            })),
          },
        ]
      : []),
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
      <Space size={12} className="admin-header-leading">
        <Tooltip title={collapsed ? "展开导航" : "收起导航"}>
          <Button
            type="text"
            className="admin-icon-button"
            aria-label={collapsed ? "展开导航" : "收起导航"}
            icon={collapsed ? <MenuUnfoldOutlined /> : <MenuFoldOutlined />}
            onClick={onToggle}
          />
        </Tooltip>
        <div className="admin-header-title-wrap">
          <Typography.Text strong className="admin-header-title">
            后台管理
          </Typography.Text>
          <Typography.Text type="secondary" className="admin-header-subtitle">
            管理平台
          </Typography.Text>
        </div>
      </Space>
      <Space size={12} className="admin-header-actions">
        <Tooltip title="搜索菜单">
          <Button
            type="text"
            className="admin-icon-button admin-menu-search-trigger"
            aria-label="搜索菜单"
            icon={<SearchOutlined />}
            onClick={onOpenMenuSearch}
          />
        </Tooltip>
        <Space.Compact className="admin-zoom-control">
          <Button
            type="text"
            aria-label="缩小页面"
            icon={<ZoomOutOutlined />}
            disabled={contentZoom <= 0.7}
            onClick={zoomOut}
          />
          <Button
            type="text"
            aria-label="重置页面缩放"
            icon={<FontSizeOutlined />}
            onClick={resetZoom}
          >
            {Math.round(contentZoom * 100)}%
          </Button>
          <Button
            type="text"
            aria-label="放大页面"
            icon={<ZoomInOutlined />}
            disabled={contentZoom >= 1}
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
        <Dropdown
          menu={{ items: menuItems }}
          placement="bottomRight"
          trigger={["click"]}
        >
          <Button
            type="text"
            className="admin-user-button"
            aria-label="账户菜单"
          >
            <Space>
              <Avatar size={30} icon={<UserOutlined />} />
              <span className="admin-user-name">{user?.name ?? "管理员"}</span>
            </Space>
          </Button>
        </Dropdown>
      </Space>
    </Header>
  );
}
