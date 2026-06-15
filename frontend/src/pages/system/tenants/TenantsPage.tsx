import {
  ApartmentOutlined,
  DeleteOutlined,
  EditOutlined,
  PlusOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Form, Input, Modal, Popconfirm, Space, Switch, message } from "antd";
import type { ColumnsType } from "antd/es/table";
import { useState } from "react";
import {
  type SaveTenantParams,
  type TenantRecord,
  createTenant,
  deleteTenant,
  fetchTenants,
  updateTenant,
} from "../../../api/admin/tenants";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { StatusTag } from "../../../components/admin/StatusTag";

export function TenantsPage() {
  const [page, setPage] = useState(1);
  const [keyword, setKeyword] = useState("");
  const [editing, setEditing] = useState<TenantRecord | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [form] = Form.useForm<SaveTenantParams>();
  const queryClient = useQueryClient();

  const tenantsQuery = useQuery({
    queryKey: ["admin-tenants", page, keyword],
    queryFn: () =>
      fetchTenants({ page, page_size: 10, keyword: keyword || undefined }),
  });

  const saveMutation = useMutation({
    mutationFn: (values: SaveTenantParams) =>
      editing ? updateTenant(editing.id, values) : createTenant(values),
    onSuccess: () => {
      message.success("租户已保存");
      setFormOpen(false);
      setEditing(null);
      form.resetFields();
      queryClient.invalidateQueries({ queryKey: ["admin-tenants"] });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: deleteTenant,
    onSuccess: () => {
      message.success("租户已删除");
      queryClient.invalidateQueries({ queryKey: ["admin-tenants"] });
    },
  });

  const columns: ColumnsType<TenantRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    { title: "租户名称", dataIndex: "name", width: 180 },
    { title: "租户编码", dataIndex: "code", width: 180 },
    { title: "说明", dataIndex: "description", ellipsis: true },
    {
      title: "状态",
      dataIndex: "enabled",
      width: 100,
      render: (enabled) => <StatusTag active={enabled} />,
    },
    {
      title: "部门管理",
      dataIndex: "departments_enabled",
      width: 110,
      render: (enabled) => <StatusTag active={enabled} />,
    },
    {
      title: "系统租户",
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
            permission="system:tenant:update"
            onClick={() => {
              setEditing(record);
              form.setFieldsValue({
                name: record.name,
                code: record.code,
                description: record.description,
                enabled: record.enabled,
                departments_enabled: record.departments_enabled,
              });
              setFormOpen(true);
            }}
          >
            编辑
          </PermissionButton>
          <Popconfirm
            title="确认删除租户？"
            onConfirm={() => deleteMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              disabled={record.is_system}
              icon={<DeleteOutlined />}
              permission="system:tenant:delete"
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
      title="租户管理"
      subtitle="维护平台租户，作为用户、角色和后续业务数据隔离的基础"
      breadcrumb={["系统管理", "租户管理"]}
      icon={<ApartmentOutlined />}
      toolbar={
        <Space wrap>
          <Input.Search
            allowClear
            placeholder="搜索租户名称、编码"
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
                permission: "system:tenant:create",
                onClick: () => {
                  setEditing(null);
                  form.resetFields();
                  form.setFieldsValue({
                    enabled: true,
                    departments_enabled: false,
                  });
                  setFormOpen(true);
                },
              },
            ]}
          />
        </Space>
      }
    >
      <DataTable<TenantRecord>
        columns={columns}
        dataSource={tenantsQuery.data?.items ?? []}
        loading={tenantsQuery.isLoading}
        pagination={{
          current: page,
          total: tenantsQuery.data?.total ?? 0,
          onChange: setPage,
        }}
      />
      <Modal
        title={editing ? "编辑租户" : "新增租户"}
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
          <Form.Item name="name" label="租户名称" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="code" label="租户编码" rules={[{ required: true }]}>
            <Input disabled={editing?.is_system} />
          </Form.Item>
          <Form.Item name="description" label="说明">
            <Input.TextArea rows={3} />
          </Form.Item>
          <Form.Item name="enabled" label="启用" valuePropName="checked">
            <Switch />
          </Form.Item>
          <Form.Item
            name="departments_enabled"
            label="启用部门管理"
            valuePropName="checked"
          >
            <Switch />
          </Form.Item>
        </Form>
      </Modal>
    </CrudPage>
  );
}
