import {
  DatabaseOutlined,
  EditOutlined,
  PlusOutlined,
  TeamOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Checkbox,
  Form,
  Input,
  Modal,
  Select,
  Space,
  Switch,
  message,
} from "antd";
type CheckboxValueType = string | number | boolean;
import type { ColumnsType } from "antd/es/table";
import { useState } from "react";
import { fetchDataScopes } from "../../../api/admin/dataScopes";
import {
  type RoleRecord,
  type SaveRoleParams,
  createRole,
  deleteRole,
  fetchRoleDataScopes,
  fetchRoles,
  saveRoleDataScopes,
  updateRole,
} from "../../../api/admin/roles";
import { fetchTenants } from "../../../api/admin/tenants";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { StatusTag } from "../../../components/admin/StatusTag";
import { t, tData } from "../../../i18n";
import { useAuthStore } from "../../../stores/auth";

export function RolesPage() {
  const [page, setPage] = useState(1);
  const [editing, setEditing] = useState<RoleRecord | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [dataScopeRole, setDataScopeRole] = useState<RoleRecord | null>(null);
  const [selectedDataScopeIds, setSelectedDataScopeIds] = useState<number[]>(
    [],
  );
  const [form] = Form.useForm<SaveRoleParams>();
  const queryClient = useQueryClient();
  const effectiveDataScope = useAuthStore((state) => state.effectiveDataScope);
  const showTenantSelect = effectiveDataScope === "all";

  const rolesQuery = useQuery({
    queryKey: ["admin-roles", page],
    queryFn: () => fetchRoles({ page, page_size: 10 }),
  });
  const tenantsQuery = useQuery({
    queryKey: ["admin-tenants-all"],
    queryFn: () => fetchTenants({ page: 1, page_size: 100 }),
    enabled: showTenantSelect,
  });
  const dataScopesQuery = useQuery({
    queryKey: ["admin-data-scopes"],
    queryFn: fetchDataScopes,
    enabled: Boolean(dataScopeRole),
  });
  const saveMutation = useMutation({
    mutationFn: (values: SaveRoleParams) =>
      editing ? updateRole(editing.id, values) : createRole(values),
    onSuccess: () => {
      message.success(t("角色已保存"));
      setFormOpen(false);
      setEditing(null);
      form.resetFields();
      queryClient.invalidateQueries({ queryKey: ["admin-roles"] });
      queryClient.invalidateQueries({ queryKey: ["admin-roles-all"] });
    },
  });
  const deleteMutation = useMutation({
    mutationFn: deleteRole,
    onSuccess: () => {
      message.success(t("角色已删除"));
      queryClient.invalidateQueries({ queryKey: ["admin-roles"] });
      queryClient.invalidateQueries({ queryKey: ["admin-roles-all"] });
    },
  });
  const saveDataScopesMutation = useMutation({
    mutationFn: () =>
      saveRoleDataScopes(dataScopeRole?.id ?? 0, selectedDataScopeIds),
    onSuccess: () => {
      message.success(t("数据权限已保存"));
      setDataScopeRole(null);
    },
  });

  const columns: ColumnsType<RoleRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    {
      title: t("角色名称"),
      dataIndex: "name",
      render: (value: string) => tData(value),
    },
    { title: t("角色编码"), dataIndex: "code" },
    { title: t("说明"), dataIndex: "description" },
    {
      title: t("租户"),
      dataIndex: "tenant_id",
      width: 140,
      render: (tenantId) =>
        tenantId
          ? ((tenantsQuery.data?.items ?? []).find(
              (tenant) => tenant.id === tenantId,
            )?.name ?? tenantId)
          : t("平台级"),
    },
    {
      title: t("系统角色"),
      dataIndex: "is_system",
      render: (isSystem) => <StatusTag active={isSystem} />,
    },
    {
      title: t("状态"),
      dataIndex: "enabled",
      render: (enabled) => <StatusTag active={enabled} />,
    },
    {
      title: t("操作"),
      key: "actions",
      width: 260,
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:role:update"
            onClick={() => {
              setEditing(record);
              form.setFieldsValue({
                name: record.name,
                code: record.code,
                description: record.description,
                enabled: record.enabled,
                tenant_id: record.tenant_id,
              });
              setFormOpen(true);
            }}
          >
            {t("编辑")}
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<DatabaseOutlined />}
            permission="system:role:assign_data_scopes"
            onClick={() => {
              setDataScopeRole(record);
              fetchRoleDataScopes(record.id).then(setSelectedDataScopeIds);
            }}
          >
            {t("数据权限")}
          </PermissionButton>
          <PermissionButton
            size="small"
            danger
            disabled={record.is_system}
            permission="system:role:delete"
            onClick={() => deleteMutation.mutate(record.id)}
          >
            {t("删除")}
          </PermissionButton>
        </Space>
      ),
    },
  ];

  return (
    <CrudPage
      title={t("角色管理")}
      subtitle={t("按角色聚合用户、菜单、按钮和接口权限")}
      breadcrumb={[t("系统管理"), t("角色管理")]}
      icon={<TeamOutlined />}
      toolbar={
        <CrudToolbar
          actions={[
            {
              key: "create",
              label: t("新增"),
              icon: <PlusOutlined />,
              primary: true,
              permission: "system:role:create",
              onClick: () => {
                setEditing(null);
                form.resetFields();
                form.setFieldsValue({ enabled: true });
                setFormOpen(true);
              },
            },
          ]}
        />
      }
    >
      <DataTable<RoleRecord>
        columns={columns}
        dataSource={rolesQuery.data?.items ?? []}
        loading={rolesQuery.isLoading}
        pagination={{
          current: page,
          total: rolesQuery.data?.total ?? 0,
          onChange: setPage,
        }}
      />
      <Modal
        title={editing ? t("编辑角色") : t("新增角色")}
        open={formOpen}
        onCancel={() => setFormOpen(false)}
        onOk={() => form.submit()}
        confirmLoading={saveMutation.isPending}
      >
        <Form
          form={form}
          layout="vertical"
          onFinish={(values) => saveMutation.mutate(values)}
        >
          <Form.Item
            name="name"
            label={t("角色名称")}
            rules={[{ required: true }]}
          >
            <Input />
          </Form.Item>
          <Form.Item
            name="code"
            label={t("角色编码")}
            rules={[{ required: true }]}
          >
            <Input disabled={editing?.is_system} />
          </Form.Item>
          <Form.Item name="description" label={t("说明")}>
            <Input.TextArea rows={3} />
          </Form.Item>
          {showTenantSelect ? (
            <Form.Item name="tenant_id" label={t("租户")}>
              <Select
                allowClear
                placeholder={t("留空表示平台级角色")}
                options={(tenantsQuery.data?.items ?? []).map((tenant) => ({
                  value: tenant.id,
                  label: `${tenant.name}（${tenant.code}）`,
                }))}
              />
            </Form.Item>
          ) : null}
          <Form.Item name="enabled" label={t("启用")} valuePropName="checked">
            <Switch />
          </Form.Item>
        </Form>
      </Modal>
      <Modal
        title={t("分配数据权限：{0}", dataScopeRole?.name ?? "")}
        open={Boolean(dataScopeRole)}
        onCancel={() => setDataScopeRole(null)}
        onOk={() => saveDataScopesMutation.mutate()}
        confirmLoading={saveDataScopesMutation.isPending}
      >
        <Checkbox.Group
          value={selectedDataScopeIds}
          onChange={(values: CheckboxValueType[]) =>
            setSelectedDataScopeIds(values.map(Number))
          }
        >
          <Space direction="vertical">
            {(dataScopesQuery.data ?? []).map((scope) => (
              <Checkbox key={scope.id} value={scope.id}>
                {scope.name}（{scope.code}）
              </Checkbox>
            ))}
          </Space>
        </Checkbox.Group>
        {dataScopesQuery.isFetching ? (
          <div className="modal-hint">{t("正在加载数据权限...")}</div>
        ) : null}
      </Modal>
    </CrudPage>
  );
}
