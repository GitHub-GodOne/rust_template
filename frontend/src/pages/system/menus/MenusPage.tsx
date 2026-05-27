import { EditOutlined, MenuOutlined, PlusOutlined } from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Form,
  Input,
  InputNumber,
  Modal,
  Select,
  Space,
  Switch,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useState } from "react";
import {
  type MenuRecord,
  type SaveMenuParams,
  createMenu,
  deleteMenu,
  fetchMenus,
  updateMenu,
} from "../../../api/admin/menus";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { StatusTag } from "../../../components/admin/StatusTag";

function flattenMenus(menus: MenuRecord[]): MenuRecord[] {
  return menus.flatMap((menu) => [menu, ...flattenMenus(menu.children ?? [])]);
}

export function MenusPage() {
  const [editing, setEditing] = useState<MenuRecord | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [form] = Form.useForm<SaveMenuParams>();
  const queryClient = useQueryClient();

  const menusQuery = useQuery({
    queryKey: ["admin-menus"],
    queryFn: fetchMenus,
  });
  const flatMenus = flattenMenus(menusQuery.data ?? []);
  const saveMutation = useMutation({
    mutationFn: (values: SaveMenuParams) =>
      editing ? updateMenu(editing.id, values) : createMenu(values),
    onSuccess: () => {
      message.success("菜单已保存");
      setFormOpen(false);
      setEditing(null);
      form.resetFields();
      queryClient.invalidateQueries({ queryKey: ["admin-menus"] });
    },
  });
  const deleteMutation = useMutation({
    mutationFn: deleteMenu,
    onSuccess: () => {
      message.success("菜单已删除");
      queryClient.invalidateQueries({ queryKey: ["admin-menus"] });
    },
  });

  const columns: ColumnsType<MenuRecord> = [
    { title: "菜单名称", dataIndex: "title", width: 220 },
    { title: "路径", dataIndex: "path" },
    { title: "图标", dataIndex: "icon", width: 120 },
    { title: "权限编码", dataIndex: "permission_code" },
    { title: "排序", dataIndex: "sort_order", width: 90 },
    {
      title: "显示",
      dataIndex: "visible",
      width: 90,
      render: (visible) => <StatusTag active={visible} />,
    },
    {
      title: "启用",
      dataIndex: "enabled",
      width: 90,
      render: (enabled) => <StatusTag active={enabled} />,
    },
    {
      title: "操作",
      key: "actions",
      width: 180,
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:menu:update"
            onClick={() => {
              setEditing(record);
              form.setFieldsValue({
                parent_id: record.parent_id,
                title: record.title,
                path: record.path,
                icon: record.icon,
                permission_code: record.permission_code,
                sort_order: record.sort_order,
                visible: record.visible,
                enabled: record.enabled,
              });
              setFormOpen(true);
            }}
          >
            编辑
          </PermissionButton>
          <PermissionButton
            size="small"
            danger
            permission="system:menu:delete"
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
      title="菜单管理"
      subtitle="维护后台侧边栏、前端路由和菜单权限编码"
      breadcrumb={["系统管理", "菜单管理"]}
      icon={<MenuOutlined />}
      toolbar={
        <CrudToolbar
          actions={[
            {
              key: "create",
              label: "新增",
              icon: <PlusOutlined />,
              primary: true,
              permission: "system:menu:create",
              onClick: () => {
                setEditing(null);
                form.resetFields();
                form.setFieldsValue({
                  visible: true,
                  enabled: true,
                  sort_order: 0,
                });
                setFormOpen(true);
              },
            },
          ]}
        />
      }
    >
      <DataTable<MenuRecord>
        columns={columns}
        dataSource={menusQuery.data ?? []}
        loading={menusQuery.isLoading}
        pagination={false}
      />
      <Modal
        title={editing ? "编辑菜单" : "新增菜单"}
        open={formOpen}
        onCancel={() => setFormOpen(false)}
        onOk={() => form.submit()}
        confirmLoading={saveMutation.isPending}
        width={720}
      >
        <Form
          form={form}
          layout="vertical"
          onFinish={(values) => saveMutation.mutate(values)}
        >
          <Form.Item name="parent_id" label="上级菜单">
            <Select
              allowClear
              options={flatMenus
                .filter((menu) => menu.id !== editing?.id)
                .map((menu) => ({ label: menu.title, value: menu.id }))}
            />
          </Form.Item>
          <Form.Item name="title" label="菜单名称" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="path" label="前端路径">
            <Input placeholder="/admin/system/users" />
          </Form.Item>
          <Form.Item name="icon" label="图标编码">
            <Input placeholder="user / team / menu" />
          </Form.Item>
          <Form.Item name="permission_code" label="菜单权限编码">
            <Input placeholder="system:user:list" />
          </Form.Item>
          <Form.Item name="sort_order" label="排序">
            <InputNumber min={0} className="full-width" />
          </Form.Item>
          <Space>
            <Form.Item name="visible" label="显示" valuePropName="checked">
              <Switch />
            </Form.Item>
            <Form.Item name="enabled" label="启用" valuePropName="checked">
              <Switch />
            </Form.Item>
          </Space>
        </Form>
      </Modal>
    </CrudPage>
  );
}
