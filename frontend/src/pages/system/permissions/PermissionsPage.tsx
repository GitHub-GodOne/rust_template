import { SafetyCertificateOutlined, SaveOutlined } from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Card, Checkbox, Col, List, Row, Space, Tag, message } from "antd";
import type { CheckboxValueType } from "antd/es/checkbox/Group";
import { useEffect, useMemo, useState } from "react";
import { type MenuRecord, fetchMenus } from "../../../api/admin/menus";
import { fetchPermissions } from "../../../api/admin/permissions";
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

export function PermissionsPage() {
  const [selectedRoleId, setSelectedRoleId] = useState<number | null>(null);
  const [selectedPermissionIds, setSelectedPermissionIds] = useState<number[]>(
    [],
  );
  const [menuGrants, setMenuGrants] = useState<Record<number, RoleMenuGrant>>(
    {},
  );
  const queryClient = useQueryClient();

  const rolesQuery = useQuery({
    queryKey: ["admin-roles-all"],
    queryFn: () => fetchRoles({ page: 1, page_size: 100 }),
  });
  const permissionsQuery = useQuery({
    queryKey: ["admin-permissions-all"],
    queryFn: () => fetchPermissions({ page: 1, page_size: 100 }),
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

  return (
    <div>
      <PageHeader
        title="权限配置"
        subtitle="左侧选择角色，右侧维护接口权限、菜单权限和按钮权限"
        breadcrumb={["系统管理", "权限配置"]}
        icon={<SafetyCertificateOutlined />}
      />
      <Row gutter={[16, 16]}>
        <Col xs={24} lg={6}>
          <Card title="角色" className="admin-card">
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
        </Col>
        <Col xs={24} lg={18}>
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
            <div className="permission-grid">
              <div className="permission-row">
                <strong>接口 / 功能权限</strong>
                <Checkbox.Group
                  value={selectedPermissionIds}
                  onChange={(values: CheckboxValueType[]) =>
                    setSelectedPermissionIds(values.map(Number))
                  }
                >
                  <Space wrap>
                    {(permissionsQuery.data?.items ?? []).map((permission) => (
                      <Checkbox key={permission.id} value={permission.id}>
                        {permission.name}
                      </Checkbox>
                    ))}
                  </Space>
                </Checkbox.Group>
              </div>
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
          </Card>
        </Col>
      </Row>
    </div>
  );
}
