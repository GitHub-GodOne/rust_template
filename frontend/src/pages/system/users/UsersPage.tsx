import {
  BranchesOutlined,
  CloudDownloadOutlined,
  EditOutlined,
  KeyOutlined,
  LogoutOutlined,
  MoreOutlined,
  StopOutlined,
  TeamOutlined,
  UnlockOutlined,
  UserOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Button,
  Checkbox,
  Dropdown,
  Form,
  Input,
  Modal,
  Select,
  Space,
  Tag,
  Tooltip,
  message,
} from "antd";
import type { MenuProps } from "antd";
import dayjs from "dayjs";
type CheckboxValueType = string | number | boolean;
import type { ColumnsType } from "antd/es/table";
import { useState } from "react";
import { fetchDepartments } from "../../../api/admin/departments";
import { fetchRoles } from "../../../api/admin/roles";
import { revokeUserSessions } from "../../../api/admin/sessions";
import { fetchTenants } from "../../../api/admin/tenants";
import {
  type SaveUserParams,
  type UserRecord,
  createUser,
  deleteUser,
  exportUsers,
  fetchUserDepartments,
  fetchUserRoles,
  fetchUsers,
  resetUserPassword,
  saveUserDepartments,
  saveUserRoles,
  setUserStatus,
  unlockUser,
  updateUser,
} from "../../../api/admin/users";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { StatusTag } from "../../../components/admin/StatusTag";
import { t, tData } from "../../../i18n";
import { useAuthStore } from "../../../stores/auth";

type UserFormValues = SaveUserParams & { password?: string };

const formatTime = (value?: string | null) =>
  value ? dayjs(value).format("YYYY-MM-DD HH:mm") : "-";

export function UsersPage() {
  const [page, setPage] = useState(1);
  const [keyword, setKeyword] = useState("");
  const [search, setSearch] = useState("");
  const [statusFilter, setStatusFilter] = useState<string>();
  const [resetUser, setResetUser] = useState<UserRecord | null>(null);
  const [resetForm] = Form.useForm<{ password: string; confirm: string }>();
  const hasPermission = useAuthStore((state) => state.hasPermission);
  const currentUserEmail = useAuthStore((state) => state.user?.email);
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

  const filters = { keyword: search || undefined, status: statusFilter };
  const usersQuery = useQuery({
    queryKey: ["admin-users", page, search, statusFilter],
    queryFn: () => fetchUsers({ page, page_size: 10, ...filters }),
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
        throw new Error(t("请输入初始密码"));
      }
      return createUser({ ...values, password: values.password });
    },
    onSuccess: () => {
      message.success(t("用户已保存"));
      setFormOpen(false);
      setEditing(null);
      form.resetFields();
      queryClient.invalidateQueries({ queryKey: ["admin-users"] });
    },
  });
  const deleteMutation = useMutation({
    mutationFn: deleteUser,
    onSuccess: () => {
      message.success(t("用户已删除，可在回收站恢复"));
      queryClient.invalidateQueries({ queryKey: ["admin-users"] });
    },
  });
  const refreshUsers = () =>
    queryClient.invalidateQueries({ queryKey: ["admin-users"] });
  const statusMutation = useMutation({
    mutationFn: ({
      id,
      status,
    }: { id: number; status: "active" | "disabled" }) =>
      setUserStatus(id, status),
    onSuccess: (user) => {
      message.success(
        user.status === "disabled"
          ? t("已禁用，该用户的登录已全部失效")
          : t("已启用"),
      );
      void refreshUsers();
    },
  });
  const unlockMutation = useMutation({
    mutationFn: unlockUser,
    onSuccess: () => {
      message.success(t("已解锁"));
      void refreshUsers();
    },
  });
  const revokeSessionsMutation = useMutation({
    mutationFn: revokeUserSessions,
    onSuccess: (count) => message.success(t("已下线 {0} 个会话", count)),
  });
  const resetPasswordMutation = useMutation({
    mutationFn: (password: string) =>
      resetUserPassword(resetUser?.id ?? 0, password),
    onSuccess: () => {
      message.success(t("密码已重置，该用户需要用新密码重新登录"));
      setResetUser(null);
      resetForm.resetFields();
      void refreshUsers();
    },
  });
  const exportMutation = useMutation({
    mutationFn: () => exportUsers(filters),
    onSuccess: (count) => message.success(t("已导出 {0} 条", count)),
  });

  const moreActions = (record: UserRecord): MenuProps["items"] => {
    const self = record.email === currentUserEmail;
    const disabled = record.status === "disabled";
    return [
      hasPermission("system:user:status") && !self
        ? {
            key: "status",
            icon: disabled ? <UserOutlined /> : <StopOutlined />,
            danger: !disabled,
            label: disabled ? t("启用") : t("禁用"),
            onClick: () =>
              disabled
                ? statusMutation.mutate({ id: record.id, status: "active" })
                : Modal.confirm({
                    title: t("禁用 {0}？", record.name),
                    content: t(
                      "禁用后该用户无法登录，已登录的设备会立即下线。",
                    ),
                    okText: t("禁用"),
                    okButtonProps: { danger: true },
                    cancelText: t("取消"),
                    onOk: () =>
                      statusMutation.mutateAsync({
                        id: record.id,
                        status: "disabled",
                      }),
                  }),
          }
        : null,
      hasPermission("system:user:reset_password")
        ? {
            key: "reset",
            icon: <KeyOutlined />,
            label: t("重置密码"),
            onClick: () => {
              resetForm.resetFields();
              setResetUser(record);
            },
          }
        : null,
      record.locked && hasPermission("system:user:unlock")
        ? {
            key: "unlock",
            icon: <UnlockOutlined />,
            label: t("解锁"),
            onClick: () => unlockMutation.mutate(record.id),
          }
        : null,
      hasPermission("system:session:revoke")
        ? {
            key: "sessions",
            icon: <LogoutOutlined />,
            label: t("强制下线"),
            onClick: () =>
              Modal.confirm({
                title: t("让 {0} 的所有设备下线？", record.name),
                okText: t("下线"),
                cancelText: t("取消"),
                onOk: () => revokeSessionsMutation.mutateAsync(record.id),
              }),
          }
        : null,
    ].filter(Boolean) as MenuProps["items"];
  };

  const saveRolesMutation = useMutation({
    mutationFn: () => saveUserRoles(roleUser?.id ?? 0, selectedRoleIds),
    onSuccess: () => {
      message.success(t("角色已保存"));
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
      message.success(t("部门已保存"));
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
    { title: t("姓名"), dataIndex: "name", width: 160 },
    { title: t("邮箱"), dataIndex: "email" },
    {
      title: t("租户"),
      dataIndex: "tenant_id",
      width: 140,
      render: (tenantId) =>
        tenantId
          ? tData(
              (tenantsQuery.data?.items ?? []).find(
                (tenant) => tenant.id === tenantId,
              )?.name,
            ) || tenantId
          : t("平台级"),
    },
    {
      title: t("当前部门"),
      dataIndex: "current_department_id",
      width: 150,
      render: (departmentId) =>
        departmentId
          ? ((departmentsQuery.data?.items ?? []).find(
              (department) => department.id === departmentId,
            )?.name ?? departmentId)
          : t("未设置"),
    },
    {
      title: t("状态"),
      dataIndex: "status",
      width: 150,
      render: (status: string, record) => (
        <Space size={4} wrap>
          {status === "disabled" ? (
            <Tag color="error">{t("已禁用")}</Tag>
          ) : (
            <Tag color="success">{t("正常")}</Tag>
          )}
          {record.locked ? (
            <Tooltip
              title={t(
                "登录失败次数过多，锁定到 {0}",
                formatTime(record.locked_until),
              )}
            >
              <Tag color="warning">{t("已锁定")}</Tag>
            </Tooltip>
          ) : null}
        </Space>
      ),
    },
    {
      title: t("验证状态"),
      dataIndex: "is_verified",
      width: 110,
      render: (verified) => <StatusTag active={verified} />,
    },
    {
      title: t("最后登录"),
      dataIndex: "last_login_at",
      width: 190,
      render: (value: string | null, record) =>
        value ? (
          <Tooltip title={record.last_login_ip}>{formatTime(value)}</Tooltip>
        ) : (
          t("从未登录")
        ),
    },
    {
      title: t("操作"),
      key: "actions",
      width: 330,
      fixed: "right",
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
            {t("编辑")}
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
            {t("角色")}
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
            {t("部门")}
          </PermissionButton>
          <PermissionButton
            size="small"
            danger
            permission="system:user:delete"
            onClick={() =>
              Modal.confirm({
                title: t("删除 {0}？", record.name),
                content: t("删除后可在回收站恢复。"),
                okText: t("删除"),
                okButtonProps: { danger: true },
                cancelText: t("取消"),
                onOk: () => deleteMutation.mutateAsync(record.id),
              })
            }
          >
            {t("删除")}
          </PermissionButton>
          {(moreActions(record) ?? []).length > 0 ? (
            <Dropdown menu={{ items: moreActions(record) }} trigger={["click"]}>
              <Button
                size="small"
                icon={<MoreOutlined />}
                aria-label={t("更多操作")}
              />
            </Dropdown>
          ) : null}
        </Space>
      ),
    },
  ];

  return (
    <CrudPage
      title={t("用户管理")}
      subtitle={t("维护后台登录账号、状态和角色关系")}
      breadcrumb={[t("系统管理"), t("用户管理")]}
      icon={<UserOutlined />}
      extra={
        <Space wrap>
          <Input.Search
            allowClear
            value={keyword}
            className="users-search"
            placeholder={t("姓名或邮箱")}
            onChange={(event) => setKeyword(event.target.value)}
            onSearch={(value) => {
              setSearch(value.trim());
              setPage(1);
            }}
          />
          <Select
            allowClear
            className="users-status-select"
            placeholder={t("全部状态")}
            value={statusFilter}
            options={[
              { value: "active", label: t("正常") },
              { value: "disabled", label: t("已禁用") },
            ]}
            onChange={(value) => {
              setStatusFilter(value);
              setPage(1);
            }}
          />
        </Space>
      }
      toolbar={
        <CrudToolbar
          actions={[
            {
              key: "create",
              label: t("新增"),
              icon: <UserOutlined />,
              primary: true,
              permission: "system:user:create",
              onClick: () => {
                setEditing(null);
                form.resetFields();
                setFormOpen(true);
              },
            },
            {
              key: "export",
              label: exportMutation.isPending ? t("导出中...") : t("导出"),
              icon: <CloudDownloadOutlined />,
              permission: "system:user:export",
              disabled: exportMutation.isPending,
              onClick: () => exportMutation.mutate(),
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
        title={editing ? t("编辑用户") : t("新增用户")}
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
          <Form.Item name="name" label={t("姓名")} rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item
            name="email"
            label={t("邮箱")}
            rules={[{ required: true, type: "email" }]}
          >
            <Input />
          </Form.Item>
          {showTenantSelect ? (
            <Form.Item name="tenant_id" label={t("租户")}>
              <Select
                allowClear
                placeholder={t("请选择租户")}
                options={(tenantsQuery.data?.items ?? []).map((tenant) => ({
                  value: tenant.id,
                  label: `${tData(tenant.name)}（${tenant.code}）`,
                }))}
              />
            </Form.Item>
          ) : null}
          {!editing && (
            <Form.Item
              name="password"
              label={t("初始密码")}
              rules={[{ required: true }]}
            >
              <Input.Password />
            </Form.Item>
          )}
        </Form>
      </Modal>
      <Modal
        title={t("重置密码：{0}", resetUser?.name ?? "")}
        open={Boolean(resetUser)}
        okText={t("重置")}
        cancelText={t("取消")}
        confirmLoading={resetPasswordMutation.isPending}
        onCancel={() => setResetUser(null)}
        onOk={() => resetForm.submit()}
        destroyOnClose
      >
        <Form
          form={resetForm}
          layout="vertical"
          onFinish={(values) => resetPasswordMutation.mutate(values.password)}
        >
          <Form.Item
            name="password"
            label={t("新密码")}
            extra={t(
              "至少 8 位，需同时包含字母和数字。重置后该用户的所有设备都会下线。",
            )}
            rules={[
              { required: true, message: t("请输入新密码") },
              { min: 8, message: t("至少 8 位") },
            ]}
          >
            <Input.Password autoComplete="new-password" />
          </Form.Item>
          <Form.Item
            name="confirm"
            label={t("确认新密码")}
            dependencies={["password"]}
            rules={[
              { required: true, message: t("请再次输入新密码") },
              ({ getFieldValue }) => ({
                validator: (_, value) =>
                  !value || value === getFieldValue("password")
                    ? Promise.resolve()
                    : Promise.reject(new Error(t("两次输入的密码不一致"))),
              }),
            ]}
          >
            <Input.Password autoComplete="new-password" />
          </Form.Item>
        </Form>
      </Modal>
      <Modal
        title={t("分配角色：{0}", roleUser?.name ?? "")}
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
                {tData(role.name)}（{role.code}）
              </Checkbox>
            ))}
          </Space>
        </Checkbox.Group>
        {userRolesQuery.isFetching ? (
          <div className="modal-hint">{t("正在加载已分配角色...")}</div>
        ) : null}
      </Modal>
      <Modal
        title={t("分配部门：{0}", departmentUser?.name ?? "")}
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
            placeholder={t("请选择用户所属部门")}
            value={selectedDepartmentIds}
            options={availableDepartments.map((department) => ({
              value: department.id,
              label: `${tData(department.name)}（${department.code}）`,
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
            placeholder={t("请选择当前默认部门")}
            value={selectedCurrentDepartmentId ?? undefined}
            options={selectedDepartmentOptions.map((department) => ({
              value: department.id,
              label: `${tData(department.name)}（${department.code}）`,
            }))}
            onChange={(value) => setSelectedCurrentDepartmentId(value ?? null)}
          />
        </Space>
      </Modal>
    </CrudPage>
  );
}
