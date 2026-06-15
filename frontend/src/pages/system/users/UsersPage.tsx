import {
  BranchesOutlined,
  EditOutlined,
  TeamOutlined,
  UserOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Checkbox, Form, Input, Modal, Select, Space, message } from "antd";
import type { CheckboxValueType } from "antd/es/checkbox/Group";
import type { ColumnsType } from "antd/es/table";
import { useState } from "react";
import { fetchDepartments } from "../../../api/admin/departments";
import { fetchRoles } from "../../../api/admin/roles";
import { fetchTenants } from "../../../api/admin/tenants";
import {
  type SaveUserParams,
  type UserRecord,
  createUser,
  deleteUser,
  fetchUserDepartments,
  fetchUserRoles,
  fetchUsers,
  saveUserDepartments,
  saveUserRoles,
  updateUser,
} from "../../../api/admin/users";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { StatusTag } from "../../../components/admin/StatusTag";
import { useAuthStore } from "../../../stores/auth";

type UserFormValues = SaveUserParams & { password?: string };

export function UsersPage() {
  const [page, setPage] = useState(1);
  const [editing, setEditing] = useState<UserRecord | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [roleUser, setRoleUser] = useState<UserRecord | null>(null);
  const [departmentUser, setDepartmentUser] = useState<UserRecord | null>(null);
  const [selectedRoleIds, setSelectedRoleIds] = useState<number[]>([]);
  const [selectedDepartmentIds, setSelectedDepartmentIds] = useState<number[]>(
    [],
  );
  const [selectedCurrentDepartmentId, setSelectedCurrentDepartmentId] =
    useState<number | null>(null);
  const [form] = Form.useForm<UserFormValues>();
  const queryClient = useQueryClient();
  const effectiveDataScope = useAuthStore((state) => state.effectiveDataScope);
  const showTenantSelect = effectiveDataScope === "all";

  const usersQuery = useQuery({
    queryKey: ["admin-users", page],
    queryFn: () => fetchUsers({ page, page_size: 10 }),
  });
  const rolesQuery = useQuery({
    queryKey: ["admin-roles-all"],
    queryFn: () => fetchRoles({ page: 1, page_size: 100 }),
  });
  const tenantsQuery = useQuery({
    queryKey: ["admin-tenants-all"],
    queryFn: () => fetchTenants({ page: 1, page_size: 100 }),
    enabled: showTenantSelect,
  });
  const departmentsQuery = useQuery({
    queryKey: ["admin-departments-all"],
    queryFn: () => fetchDepartments({ page: 1, page_size: 100 }),
  });
  const userRolesQuery = useQuery({
    queryKey: ["admin-user-roles", roleUser?.id],
    queryFn: () => fetchUserRoles(roleUser?.id ?? 0),
    enabled: Boolean(roleUser),
  });

  const saveMutation = useMutation({
    mutationFn: (values: UserFormValues) => {
      if (editing) {
        return updateUser(editing.id, values);
      }
      if (!values.password) {
        throw new Error("请输入初始密码");
      }
      return createUser({ ...values, password: values.password });
    },
    onSuccess: () => {
      message.success("用户已保存");
      setFormOpen(false);
      setEditing(null);
      form.resetFields();
      queryClient.invalidateQueries({ queryKey: ["admin-users"] });
    },
  });
  const deleteMutation = useMutation({
    mutationFn: deleteUser,
    onSuccess: () => {
      message.success("用户已删除");
      queryClient.invalidateQueries({ queryKey: ["admin-users"] });
    },
  });
  const saveRolesMutation = useMutation({
    mutationFn: () => saveUserRoles(roleUser?.id ?? 0, selectedRoleIds),
    onSuccess: () => {
      message.success("角色已保存");
      setRoleUser(null);
    },
  });
  const saveDepartmentsMutation = useMutation({
    mutationFn: () =>
      saveUserDepartments(departmentUser?.id ?? 0, {
        department_ids: selectedDepartmentIds,
        current_department_id: selectedCurrentDepartmentId,
      }),
    onSuccess: () => {
      message.success("部门已保存");
      setDepartmentUser(null);
      queryClient.invalidateQueries({ queryKey: ["admin-users"] });
    },
  });

  const availableDepartments = (departmentsQuery.data?.items ?? []).filter(
    (department) =>
      !departmentUser?.tenant_id ||
      department.tenant_id === departmentUser.tenant_id,
  );
  const selectedDepartmentOptions = availableDepartments.filter((department) =>
    selectedDepartmentIds.includes(department.id),
  );

  const columns: ColumnsType<UserRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    { title: "姓名", dataIndex: "name", width: 160 },
    { title: "邮箱", dataIndex: "email" },
    {
      title: "租户",
      dataIndex: "tenant_id",
      width: 140,
      render: (tenantId) =>
        tenantId
          ? ((tenantsQuery.data?.items ?? []).find(
              (tenant) => tenant.id === tenantId,
            )?.name ?? tenantId)
          : "平台级",
    },
    {
      title: "当前部门",
      dataIndex: "current_department_id",
      width: 150,
      render: (departmentId) =>
        departmentId
          ? ((departmentsQuery.data?.items ?? []).find(
              (department) => department.id === departmentId,
            )?.name ?? departmentId)
          : "未设置",
    },
    {
      title: "验证状态",
      dataIndex: "is_verified",
      render: (verified) => <StatusTag active={verified} />,
    },
    { title: "创建时间", dataIndex: "created_at", width: 210 },
    {
      title: "操作",
      key: "actions",
      width: 260,
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:user:update"
            onClick={() => {
              setEditing(record);
              form.setFieldsValue({
                name: record.name,
                email: record.email,
                tenant_id: record.tenant_id,
              });
              setFormOpen(true);
            }}
          >
            编辑
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<TeamOutlined />}
            permission="system:user:assign_roles"
            onClick={() => {
              setRoleUser(record);
              fetchUserRoles(record.id).then((roles) => {
                setSelectedRoleIds(roles.map((role) => role.id));
              });
            }}
          >
            角色
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<BranchesOutlined />}
            permission="system:user:assign_departments"
            onClick={() => {
              setDepartmentUser(record);
              fetchUserDepartments(record.id).then((departments) => {
                setSelectedDepartmentIds(
                  departments.map((department) => department.id),
                );
                setSelectedCurrentDepartmentId(
                  record.current_department_id ??
                    departments.find((department) => department.is_primary)
                      ?.id ??
                    null,
                );
              });
            }}
          >
            部门
          </PermissionButton>
          <PermissionButton
            size="small"
            danger
            permission="system:user:delete"
            onClick={() => deleteMutation.mutate(record.id)}
          >
            删除
          </PermissionButton>
        </Space>
      ),
    },
  ];

  return (
    <CrudPage
      title="用户管理"
      subtitle="维护后台登录账号、状态和角色关系"
      breadcrumb={["系统管理", "用户管理"]}
      icon={<UserOutlined />}
      toolbar={
        <CrudToolbar
          actions={[
            {
              key: "create",
              label: "新增",
              icon: <UserOutlined />,
              primary: true,
              permission: "system:user:create",
              onClick: () => {
                setEditing(null);
                form.resetFields();
                setFormOpen(true);
              },
            },
          ]}
        />
      }
    >
      <DataTable<UserRecord>
        columns={columns}
        dataSource={usersQuery.data?.items ?? []}
        loading={usersQuery.isLoading}
        pagination={{
          current: page,
          total: usersQuery.data?.total ?? 0,
          onChange: setPage,
        }}
      />
      <Modal
        title={editing ? "编辑用户" : "新增用户"}
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
          <Form.Item name="name" label="姓名" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item
            name="email"
            label="邮箱"
            rules={[{ required: true, type: "email" }]}
          >
            <Input />
          </Form.Item>
          {showTenantSelect ? (
            <Form.Item name="tenant_id" label="租户">
              <Select
                allowClear
                placeholder="请选择租户"
                options={(tenantsQuery.data?.items ?? []).map((tenant) => ({
                  value: tenant.id,
                  label: `${tenant.name}（${tenant.code}）`,
                }))}
              />
            </Form.Item>
          ) : null}
          {!editing && (
            <Form.Item
              name="password"
              label="初始密码"
              rules={[{ required: true }]}
            >
              <Input.Password />
            </Form.Item>
          )}
        </Form>
      </Modal>
      <Modal
        title={`分配角色：${roleUser?.name ?? ""}`}
        open={Boolean(roleUser)}
        onCancel={() => setRoleUser(null)}
        onOk={() => saveRolesMutation.mutate()}
        confirmLoading={saveRolesMutation.isPending}
      >
        <Checkbox.Group
          value={selectedRoleIds}
          onChange={(values: CheckboxValueType[]) =>
            setSelectedRoleIds(values.map(Number))
          }
        >
          <Space direction="vertical">
            {(rolesQuery.data?.items ?? []).map((role) => (
              <Checkbox key={role.id} value={role.id}>
                {role.name}（{role.code}）
              </Checkbox>
            ))}
          </Space>
        </Checkbox.Group>
        {userRolesQuery.isFetching ? (
          <div className="modal-hint">正在加载已分配角色...</div>
        ) : null}
      </Modal>
      <Modal
        title={`分配部门：${departmentUser?.name ?? ""}`}
        open={Boolean(departmentUser)}
        onCancel={() => setDepartmentUser(null)}
        onOk={() => saveDepartmentsMutation.mutate()}
        confirmLoading={saveDepartmentsMutation.isPending}
      >
        <Space direction="vertical" className="admin-form-stack">
          <Select
            mode="multiple"
            allowClear
            className="admin-full-width"
            placeholder="请选择用户所属部门"
            value={selectedDepartmentIds}
            options={availableDepartments.map((department) => ({
              value: department.id,
              label: `${department.name}（${department.code}）`,
            }))}
            onChange={(values) => {
              const ids = values.map(Number);
              setSelectedDepartmentIds(ids);
              if (
                selectedCurrentDepartmentId &&
                !ids.includes(selectedCurrentDepartmentId)
              ) {
                setSelectedCurrentDepartmentId(null);
              }
            }}
          />
          <Select
            allowClear
            className="admin-full-width"
            disabled={selectedDepartmentIds.length === 0}
            placeholder="请选择当前默认部门"
            value={selectedCurrentDepartmentId ?? undefined}
            options={selectedDepartmentOptions.map((department) => ({
              value: department.id,
              label: `${department.name}（${department.code}）`,
            }))}
            onChange={(value) => setSelectedCurrentDepartmentId(value ?? null)}
          />
        </Space>
      </Modal>
    </CrudPage>
  );
}
