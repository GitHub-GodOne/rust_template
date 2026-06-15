import {
  BranchesOutlined,
  DeleteOutlined,
  EditOutlined,
  PlusOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Form,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Select,
  Space,
  Switch,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useState } from "react";
import {
  type DepartmentRecord,
  type SaveDepartmentParams,
  createDepartment,
  deleteDepartment,
  fetchDepartments,
  updateDepartment,
} from "../../../api/admin/departments";
import { fetchTenants } from "../../../api/admin/tenants";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { StatusTag } from "../../../components/admin/StatusTag";
import { useAuthStore } from "../../../stores/auth";

export function DepartmentsPage() {
  const [page, setPage] = useState(1);
  const [keyword, setKeyword] = useState("");
  const [editing, setEditing] = useState<DepartmentRecord | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [form] = Form.useForm<SaveDepartmentParams>();
  const queryClient = useQueryClient();
  const effectiveDataScope = useAuthStore((state) => state.effectiveDataScope);
  const showTenantSelect = effectiveDataScope === "all";

  const departmentsQuery = useQuery({
    queryKey: ["admin-departments", page, keyword],
    queryFn: () =>
      fetchDepartments({ page, page_size: 10, keyword: keyword || undefined }),
  });
  const allDepartmentsQuery = useQuery({
    queryKey: ["admin-departments-all"],
    queryFn: () => fetchDepartments({ page: 1, page_size: 100 }),
  });
  const tenantsQuery = useQuery({
    queryKey: ["admin-tenants-all"],
    queryFn: () => fetchTenants({ page: 1, page_size: 100 }),
    enabled: showTenantSelect,
  });

  const saveMutation = useMutation({
    mutationFn: (values: SaveDepartmentParams) =>
      editing ? updateDepartment(editing.id, values) : createDepartment(values),
    onSuccess: () => {
      message.success("部门已保存");
      setFormOpen(false);
      setEditing(null);
      form.resetFields();
      queryClient.invalidateQueries({ queryKey: ["admin-departments"] });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: deleteDepartment,
    onSuccess: () => {
      message.success("部门已删除");
      queryClient.invalidateQueries({ queryKey: ["admin-departments"] });
    },
  });

  const tenantName = (tenantId: number) =>
    (tenantsQuery.data?.items ?? []).find((tenant) => tenant.id === tenantId)
      ?.name ?? tenantId;
  const departmentName = (departmentId?: number | null) =>
    departmentId
      ? ((allDepartmentsQuery.data?.items ?? []).find(
          (department) => department.id === departmentId,
        )?.name ?? departmentId)
      : "顶级部门";

  const columns: ColumnsType<DepartmentRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    { title: "部门名称", dataIndex: "name", width: 180 },
    { title: "部门编码", dataIndex: "code", width: 180 },
    {
      title: "租户",
      dataIndex: "tenant_id",
      width: 150,
      render: tenantName,
    },
    {
      title: "上级部门",
      dataIndex: "parent_id",
      width: 150,
      render: departmentName,
    },
    { title: "排序", dataIndex: "sort_order", width: 90 },
    {
      title: "启用",
      dataIndex: "enabled",
      width: 90,
      render: (enabled) => <StatusTag active={enabled} />,
    },
    {
      title: "系统部门",
      dataIndex: "is_system",
      width: 110,
      render: (isSystem) => <StatusTag active={isSystem} />,
    },
    { title: "创建时间", dataIndex: "created_at", width: 210 },
    {
      title: "操作",
      key: "actions",
      width: 180,
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:department:update"
            onClick={() => {
              setEditing(record);
              form.setFieldsValue({
                tenant_id: record.tenant_id,
                parent_id: record.parent_id,
                name: record.name,
                code: record.code,
                description: record.description,
                sort_order: record.sort_order,
                enabled: record.enabled,
              });
              setFormOpen(true);
            }}
          >
            编辑
          </PermissionButton>
          <Popconfirm
            title="确认删除部门？"
            onConfirm={() => deleteMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              disabled={record.is_system}
              icon={<DeleteOutlined />}
              permission="system:department:delete"
            >
              删除
            </PermissionButton>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <CrudPage
      title="部门管理"
      subtitle="维护租户组织结构，为用户多部门归属和部门数据范围提供基础"
      breadcrumb={["系统管理", "部门管理"]}
      icon={<BranchesOutlined />}
      toolbar={
        <Space wrap>
          <Input.Search
            allowClear
            placeholder="搜索部门名称、编码"
            className="admin-search-input"
            onSearch={(value) => {
              setPage(1);
              setKeyword(value);
            }}
          />
          <CrudToolbar
            actions={[
              {
                key: "create",
                label: "新增",
                icon: <PlusOutlined />,
                primary: true,
                permission: "system:department:create",
                onClick: () => {
                  setEditing(null);
                  form.resetFields();
                  form.setFieldsValue({ enabled: true, sort_order: 0 });
                  setFormOpen(true);
                },
              },
            ]}
          />
        </Space>
      }
    >
      <DataTable<DepartmentRecord>
        columns={columns}
        dataSource={departmentsQuery.data?.items ?? []}
        loading={departmentsQuery.isLoading}
        pagination={{
          current: page,
          total: departmentsQuery.data?.total ?? 0,
          onChange: setPage,
        }}
      />
      <Modal
        title={editing ? "编辑部门" : "新增部门"}
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
          {showTenantSelect ? (
            <Form.Item
              name="tenant_id"
              label="租户"
              rules={[{ required: true }]}
            >
              <Select
                placeholder="请选择租户"
                options={(tenantsQuery.data?.items ?? []).map((tenant) => ({
                  value: tenant.id,
                  label: `${tenant.name}（${tenant.code}）`,
                }))}
              />
            </Form.Item>
          ) : null}
          <Form.Item name="parent_id" label="上级部门">
            <Select
              allowClear
              placeholder="不选择则为顶级部门"
              options={(allDepartmentsQuery.data?.items ?? [])
                .filter((department) => department.id !== editing?.id)
                .map((department) => ({
                  value: department.id,
                  label: `${department.name}（${department.code}）`,
                }))}
            />
          </Form.Item>
          <Form.Item name="name" label="部门名称" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="code" label="部门编码" rules={[{ required: true }]}>
            <Input disabled={editing?.is_system} />
          </Form.Item>
          <Form.Item name="description" label="说明">
            <Input.TextArea rows={3} />
          </Form.Item>
          <Form.Item name="sort_order" label="排序">
            <InputNumber className="admin-full-width" />
          </Form.Item>
          <Form.Item name="enabled" label="启用" valuePropName="checked">
            <Switch />
          </Form.Item>
        </Form>
      </Modal>
    </CrudPage>
  );
}
