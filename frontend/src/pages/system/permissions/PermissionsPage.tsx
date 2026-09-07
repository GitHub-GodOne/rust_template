import {
  ApiOutlined,
  MenuOutlined,
  SafetyCertificateOutlined,
  SaveOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Card,
  Checkbox,
  Input,
  List,
  Space,
  Tabs,
  Tag,
  Typography,
  message,
} from "antd";
import type { CheckboxValueType } from "antd/es/checkbox/Group";
import { useEffect, useMemo, useState } from "react";
import { type MenuRecord, fetchMenus } from "../../../api/admin/menus";
import { fetchAllPermissions } from "../../../api/admin/permissions";
import {
  type RoleMenuGrant,
  fetchRoleMenus,
  fetchRolePermissions,
  fetchRoles,
  saveRoleMenus,
  saveRolePermissions,
} from "../../../api/admin/roles";
import { PageHeader } from "../../../components/admin/PageHeader";
import { PermissionButton } from "../../../components/admin/PermissionButton";

const actionOptions: Array<{
  key: keyof Omit<RoleMenuGrant, "menu_id">;
  label: string;
}> = [
  { key: "can_create", label: "新增" },
  { key: "can_update", label: "编辑" },
  { key: "can_delete", label: "删除" },
  { key: "can_import", label: "导入" },
  { key: "can_export", label: "导出" },
  { key: "can_print", label: "打印" },
  { key: "can_help", label: "帮助" },
];

function flattenMenus(menus: MenuRecord[]): MenuRecord[] {
  return menus.flatMap((menu) => [menu, ...flattenMenus(menu.children ?? [])]);
}

function emptyGrant(menuId: number): RoleMenuGrant {
  return {
    menu_id: menuId,
    can_create: false,
    can_update: false,
    can_delete: false,
    can_import: false,
    can_export: false,
    can_print: false,
    can_help: false,
  };
}

const permissionGroupLabels: Record<string, string> = {
  dashboard: "仪表盘",
  "system:user": "用户管理",
  "system:role": "角色管理",
  "system:tenant": "租户管理",
  "system:department": "部门管理",
  "system:menu": "菜单管理",
  "system:permission": "权限管理",
  "system:log": "日志审计",
  "system:upload": "素材库",
  "system:file": "文件管理",
  "system:static_host": "静态托管",
  "system:chat": "聊天管理",
  "system:storage": "存储配置",
  "system:setting": "系统设置",
  "system:dict": "字典管理",
  "system:http_client": "HTTP 客户端",
  "system:notification": "通知中心",
  "system:scheduled_task": "定时任务",
  "system:backup": "数据库备份",
  "system:docs": "接口文档",
  "system:monitor": "系统监控",
  "system:rate_limit": "限流规则",
  "system:command": "命令管理",
  "system:api_key": "API 授权",
  "system:ssh": "SSH 管理",
  "system:vnc": "VNC 管理",
  "system:ai_image": "AI 图片",
  "system:email_template": "邮箱模板",
  "system:work_order": "工单管理",
  "system:payment_order": "支付管理 · 订单",
  "system:payment_channel": "支付管理 · 渠道",
  "system:payment_refund": "支付管理 · 退款",
  "system:content_article": "内容管理 · 文章",
  "system:content_category": "内容管理 · 分类",
};

function permissionGroupLabel(groupName: string) {
  return (
    permissionGroupLabels[groupName] ??
    groupName.split(":").at(-1)?.replaceAll("_", " ") ??
    groupName
  );
}

export function PermissionsPage() {
  const [selectedRoleId, setSelectedRoleId] = useState<number | null>(null);
  const [selectedPermissionIds, setSelectedPermissionIds] = useState<number[]>(
    [],
  );
  const [menuGrants, setMenuGrants] = useState<Record<number, RoleMenuGrant>>(
    {},
  );
  const [permissionKeyword, setPermissionKeyword] = useState("");
  const queryClient = useQueryClient();

  const rolesQuery = useQuery({
    queryKey: ["admin-roles-all"],
    queryFn: () => fetchRoles({ page: 1, page_size: 100 }),
  });
  const permissionsQuery = useQuery({
    queryKey: ["admin-permissions-all"],
    queryFn: () => fetchAllPermissions(),
  });
  const menusQuery = useQuery({
    queryKey: ["admin-menus"],
    queryFn: fetchMenus,
  });
  const rolePermissionsQuery = useQuery({
    queryKey: ["admin-role-permissions", selectedRoleId],
    queryFn: () => fetchRolePermissions(selectedRoleId ?? 0),
    enabled: Boolean(selectedRoleId),
  });
  const roleMenusQuery = useQuery({
    queryKey: ["admin-role-menus", selectedRoleId],
    queryFn: () => fetchRoleMenus(selectedRoleId ?? 0),
    enabled: Boolean(selectedRoleId),
  });

  useEffect(() => {
    const firstRole = rolesQuery.data?.items[0];
    if (!selectedRoleId && firstRole) {
      setSelectedRoleId(firstRole.id);
    }
  }, [rolesQuery.data?.items, selectedRoleId]);

  useEffect(() => {
    if (rolePermissionsQuery.data) {
      setSelectedPermissionIds(rolePermissionsQuery.data);
    }
  }, [rolePermissionsQuery.data]);

  useEffect(() => {
    if (roleMenusQuery.data) {
      setMenuGrants(
        Object.fromEntries(
          roleMenusQuery.data.map((grant) => [grant.menu_id, grant]),
        ),
      );
    }
  }, [roleMenusQuery.data]);

  const menus = useMemo(
    () => flattenMenus(menusQuery.data ?? []),
    [menusQuery.data],
  );
  const selectedRole = rolesQuery.data?.items.find(
    (role) => role.id === selectedRoleId,
  );
  const permissionGroups = useMemo(() => {
    const keyword = permissionKeyword.trim().toLowerCase();
    const filtered = (permissionsQuery.data ?? []).filter(
      (permission) =>
        !keyword ||
        permission.name.toLowerCase().includes(keyword) ||
        permission.code.toLowerCase().includes(keyword) ||
        permission.group_name.toLowerCase().includes(keyword),
    );
    const groups = filtered.reduce(
      (current, permission) => {
        const group = current[permission.group_name] ?? [];
        group.push(permission);
        current[permission.group_name] = group;
        return current;
      },
      {} as Record<string, typeof filtered>,
    );
    return Object.entries(groups)
      .map(([groupName, permissions]) => ({
        groupName,
        label: permissionGroupLabel(groupName),
        permissions,
      }))
      .sort((left, right) => left.label.localeCompare(right.label, "zh-CN"));
  }, [permissionKeyword, permissionsQuery.data]);
  const saveMutation = useMutation({
    mutationFn: async () => {
      if (!selectedRoleId) {
        return;
      }
      await saveRolePermissions(selectedRoleId, selectedPermissionIds);
      await saveRoleMenus(selectedRoleId, Object.values(menuGrants));
    },
    onSuccess: () => {
      message.success("权限已保存");
      queryClient.invalidateQueries({
        queryKey: ["admin-role-permissions", selectedRoleId],
      });
      queryClient.invalidateQueries({
        queryKey: ["admin-role-menus", selectedRoleId],
      });
    },
  });

  function toggleMenu(menuId: number, checked: boolean) {
    setMenuGrants((current) => {
      const next = { ...current };
      if (checked) {
        next[menuId] = next[menuId] ?? emptyGrant(menuId);
      } else {
        delete next[menuId];
      }
      return next;
    });
  }

  function toggleAction(
    menuId: number,
    action: keyof Omit<RoleMenuGrant, "menu_id">,
    checked: boolean,
  ) {
    setMenuGrants((current) => ({
      ...current,
      [menuId]: {
        ...(current[menuId] ?? emptyGrant(menuId)),
        [action]: checked,
      },
    }));
  }

  function togglePermissionGroup(permissionIds: number[], checked: boolean) {
    setSelectedPermissionIds((current) => {
      const next = new Set(current);
      for (const permissionId of permissionIds) {
        if (checked) {
          next.add(permissionId);
        } else {
          next.delete(permissionId);
        }
      }
      return Array.from(next);
    });
  }

  function updatePermissionGroup(
    permissionIds: number[],
    selectedIds: CheckboxValueType[],
  ) {
    setSelectedPermissionIds((current) => {
      const groupIds = new Set(permissionIds);
      return [
        ...current.filter((permissionId) => !groupIds.has(permissionId)),
        ...selectedIds.map(Number),
      ];
    });
  }

  return (
    <div className="permissions-page">
      <PageHeader
        title="权限配置"
        subtitle="左侧选择角色，右侧维护接口权限、菜单权限和按钮权限"
        breadcrumb={["系统管理", "权限配置"]}
        icon={<SafetyCertificateOutlined />}
      />
      <div className="permissions-workspace">
        <aside className="permissions-role-column">
          <Card title="角色" className="admin-card permissions-role-card">
            <List
              loading={rolesQuery.isLoading}
              dataSource={rolesQuery.data?.items ?? []}
              renderItem={(role) => (
                <List.Item
                  className={
                    role.id === selectedRoleId
                      ? "selected-list-item"
                      : undefined
                  }
                  onClick={() => setSelectedRoleId(role.id)}
                >
                  <Space>
                    <SafetyCertificateOutlined />
                    {role.name}
                  </Space>
                  <Tag>{role.code}</Tag>
                </List.Item>
              )}
            />
          </Card>
        </aside>
        <main className="permissions-matrix-column">
          <Card
            title={`权限矩阵${selectedRole ? `：${selectedRole.name}` : ""}`}
            className="admin-card"
            extra={
              <PermissionButton
                type="primary"
                icon={<SaveOutlined />}
                permission="system:role:assign_permissions"
                loading={saveMutation.isPending}
                onClick={() => saveMutation.mutate()}
              >
                保存授权
              </PermissionButton>
            }
          >
            <Tabs
              className="permissions-tabs"
              defaultActiveKey="api"
              size="small"
              items={[
                {
                  key: "api",
                  label: (
                    <Space size={6}>
                      <ApiOutlined />
                      接口 / 功能权限
                    </Space>
                  ),
                  children: (
                    <div className="permission-api-section">
                      <div className="permission-section-heading">
                        <Typography.Text type="secondary">
                          按管理模块分组，可按名称或权限代码快速筛选
                        </Typography.Text>
                        <Input.Search
                          allowClear
                          value={permissionKeyword}
                          placeholder="搜索权限名称或代码"
                          onChange={(event) =>
                            setPermissionKeyword(event.target.value)
                          }
                          className="permission-search"
                        />
                      </div>
                      <div className="permission-module-grid">
                        {permissionGroups.map((group) => {
                          const permissionIds = group.permissions.map(
                            (permission) => permission.id,
                          );
                          const selectedCount = permissionIds.filter((id) =>
                            selectedPermissionIds.includes(id),
                          ).length;
                          return (
                            <section
                              key={group.groupName}
                              className="permission-module"
                            >
                              <div className="permission-module-heading">
                                <Checkbox
                                  checked={
                                    permissionIds.length > 0 &&
                                    selectedCount === permissionIds.length
                                  }
                                  indeterminate={
                                    selectedCount > 0 &&
                                    selectedCount < permissionIds.length
                                  }
                                  onChange={(event) =>
                                    togglePermissionGroup(
                                      permissionIds,
                                      event.target.checked,
                                    )
                                  }
                                >
                                  <strong>{group.label}</strong>
                                </Checkbox>
                                <Typography.Text type="secondary">
                                  {selectedCount}/{permissionIds.length}
                                </Typography.Text>
                              </div>
                              <Checkbox.Group
                                value={permissionIds.filter((id) =>
                                  selectedPermissionIds.includes(id),
                                )}
                                onChange={(values: CheckboxValueType[]) =>
                                  updatePermissionGroup(permissionIds, values)
                                }
                                className="permission-module-options"
                              >
                                {group.permissions.map((permission) => (
                                  <Checkbox
                                    key={permission.id}
                                    value={permission.id}
                                  >
                                    <span className="permission-option-label">
                                      <span>{permission.name}</span>
                                      <code>{permission.code}</code>
                                    </span>
                                  </Checkbox>
                                ))}
                              </Checkbox.Group>
                            </section>
                          );
                        })}
                      </div>
                    </div>
                  ),
                },
                {
                  key: "menu",
                  label: (
                    <Space size={6}>
                      <MenuOutlined />
                      菜单与按钮权限
                    </Space>
                  ),
                  children: (
                    <div className="permission-menu-section">
                      <Typography.Text
                        type="secondary"
                        className="permission-tab-intro"
                      >
                        控制导航入口是否显示，以及页面内允许使用的操作
                      </Typography.Text>
                      <div className="permission-grid">
                        {menus.map((menu) => {
                          const grant = menuGrants[menu.id];
                          return (
                            <div key={menu.id} className="permission-row">
                              <strong>{menu.title}</strong>
                              <Space wrap>
                                <Checkbox
                                  checked={Boolean(grant)}
                                  onChange={(event) =>
                                    toggleMenu(menu.id, event.target.checked)
                                  }
                                >
                                  菜单可见
                                </Checkbox>
                                {actionOptions.map((action) => (
                                  <Checkbox
                                    key={action.key}
                                    checked={Boolean(grant?.[action.key])}
                                    disabled={!grant}
                                    onChange={(event) =>
                                      toggleAction(
                                        menu.id,
                                        action.key,
                                        event.target.checked,
                                      )
                                    }
                                  >
                                    {action.label}
                                  </Checkbox>
                                ))}
                              </Space>
                            </div>
                          );
                        })}
                      </div>
                    </div>
                  ),
                },
              ]}
            />
          </Card>
        </main>
      </div>
    </div>
  );
}
