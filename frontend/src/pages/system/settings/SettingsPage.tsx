import {
  ControlOutlined,
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
  Tabs,
  Tag,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useState } from "react";
import {
  type DictItemRecord,
  type DictTypeRecord,
  type SaveDictItemParams,
  type SaveDictTypeParams,
  createDictItem,
  createDictType,
  deleteDictItem,
  deleteDictType,
  fetchDictItems,
  fetchDictTypes,
  updateDictItem,
  updateDictType,
} from "../../../api/admin/dicts";
import {
  type SaveSettingParams,
  type SettingRecord,
  createSetting,
  deleteSetting,
  fetchSettings,
  updateSetting,
} from "../../../api/admin/settings";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { t } from "../../../i18n";

const valueTypeOptions = [
  { value: "string", label: t("字符串") },
  { value: "number", label: t("数字") },
  { value: "boolean", label: t("布尔") },
  { value: "json", label: "JSON" },
  { value: "secret", label: t("密文") },
];

export function SettingsPage() {
  const [settingPage, setSettingPage] = useState(1);
  const [settingKeyword, setSettingKeyword] = useState("");
  const [editingSetting, setEditingSetting] = useState<SettingRecord | null>(
    null,
  );
  const [settingOpen, setSettingOpen] = useState(false);
  const [dictPage, setDictPage] = useState(1);
  const [selectedDictTypeId, setSelectedDictTypeId] = useState<number>();
  const [editingDictType, setEditingDictType] = useState<DictTypeRecord | null>(
    null,
  );
  const [dictTypeOpen, setDictTypeOpen] = useState(false);
  const [editingDictItem, setEditingDictItem] = useState<DictItemRecord | null>(
    null,
  );
  const [dictItemOpen, setDictItemOpen] = useState(false);
  const [settingForm] = Form.useForm<SaveSettingParams>();
  const [dictTypeForm] = Form.useForm<SaveDictTypeParams>();
  const [dictItemForm] = Form.useForm<SaveDictItemParams>();
  const queryClient = useQueryClient();

  const settingsQuery = useQuery({
    queryKey: ["admin-settings", settingPage, settingKeyword],
    queryFn: () =>
      fetchSettings({
        page: settingPage,
        page_size: 10,
        keyword: settingKeyword || undefined,
      }),
  });
  const dictTypesQuery = useQuery({
    queryKey: ["admin-dict-types", dictPage],
    queryFn: () => fetchDictTypes({ page: dictPage, page_size: 10 }),
  });
  const activeDictTypeId =
    selectedDictTypeId ?? dictTypesQuery.data?.items[0]?.id;
  const dictItemsQuery = useQuery({
    queryKey: ["admin-dict-items", activeDictTypeId],
    queryFn: () => fetchDictItems(activeDictTypeId ?? 0),
    enabled: Boolean(activeDictTypeId),
  });

  const saveSettingMutation = useMutation({
    mutationFn: (values: SaveSettingParams) =>
      editingSetting
        ? updateSetting(editingSetting.id, values)
        : createSetting(values),
    onSuccess: () => {
      message.success(t("配置已保存"));
      setSettingOpen(false);
      setEditingSetting(null);
      queryClient.invalidateQueries({ queryKey: ["admin-settings"] });
    },
  });
  const deleteSettingMutation = useMutation({
    mutationFn: deleteSetting,
    onSuccess: () => {
      message.success(t("配置已删除"));
      queryClient.invalidateQueries({ queryKey: ["admin-settings"] });
    },
  });
  const saveDictTypeMutation = useMutation({
    mutationFn: (values: SaveDictTypeParams) =>
      editingDictType
        ? updateDictType(editingDictType.id, values)
        : createDictType(values),
    onSuccess: () => {
      message.success(t("字典类型已保存"));
      setDictTypeOpen(false);
      setEditingDictType(null);
      queryClient.invalidateQueries({ queryKey: ["admin-dict-types"] });
    },
  });
  const deleteDictTypeMutation = useMutation({
    mutationFn: deleteDictType,
    onSuccess: () => {
      message.success(t("字典类型已删除"));
      queryClient.invalidateQueries({ queryKey: ["admin-dict-types"] });
    },
  });
  const saveDictItemMutation = useMutation({
    mutationFn: (values: SaveDictItemParams) =>
      editingDictItem
        ? updateDictItem(editingDictItem.id, values)
        : createDictItem(values),
    onSuccess: () => {
      message.success(t("字典项已保存"));
      setDictItemOpen(false);
      setEditingDictItem(null);
      queryClient.invalidateQueries({ queryKey: ["admin-dict-items"] });
    },
  });
  const deleteDictItemMutation = useMutation({
    mutationFn: deleteDictItem,
    onSuccess: () => {
      message.success(t("字典项已删除"));
      queryClient.invalidateQueries({ queryKey: ["admin-dict-items"] });
    },
  });

  const settingColumns: ColumnsType<SettingRecord> = [
    { title: t("配置键"), dataIndex: "key", width: 220 },
    { title: t("名称"), dataIndex: "name", width: 160 },
    {
      title: t("分组"),
      dataIndex: "group_key",
      width: 120,
      render: (value) => <Tag>{value}</Tag>,
    },
    { title: t("类型"), dataIndex: "value_type", width: 110 },
    { title: t("配置值"), dataIndex: "value", width: 260, ellipsis: true },
    {
      title: t("公开"),
      dataIndex: "is_public",
      width: 90,
      render: (value) => <Switch checked={value} disabled />,
    },
    {
      title: t("内置"),
      dataIndex: "is_builtin",
      width: 90,
      render: (value) => <Switch checked={value} disabled />,
    },
    {
      title: t("操作"),
      key: "actions",
      width: 180,
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:setting:update"
            onClick={() => {
              setEditingSetting(record);
              settingForm.setFieldsValue(record);
              setSettingOpen(true);
            }}
          >
            {t("编辑")}
          </PermissionButton>
          <Popconfirm
            title={t("确认删除配置？")}
            onConfirm={() => deleteSettingMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              disabled={record.is_builtin}
              icon={<DeleteOutlined />}
              permission="system:setting:delete"
            >
              {t("删除")}
            </PermissionButton>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  const dictTypeColumns: ColumnsType<DictTypeRecord> = [
    { title: t("编码"), dataIndex: "code", width: 180 },
    { title: t("名称"), dataIndex: "name", width: 160 },
    {
      title: t("启用"),
      dataIndex: "enabled",
      width: 90,
      render: (value) => <Switch checked={value} disabled />,
    },
    {
      title: t("内置"),
      dataIndex: "is_builtin",
      width: 90,
      render: (value) => <Switch checked={value} disabled />,
    },
    {
      title: t("操作"),
      key: "actions",
      width: 240,
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            onClick={() => setSelectedDictTypeId(record.id)}
            permission="system:dict:list"
          >
            {t("字典项")}
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:dict:update"
            onClick={() => {
              setEditingDictType(record);
              dictTypeForm.setFieldsValue(record);
              setDictTypeOpen(true);
            }}
          >
            {t("编辑")}
          </PermissionButton>
          <Popconfirm
            title={t("确认删除字典类型？")}
            onConfirm={() => deleteDictTypeMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              disabled={record.is_builtin}
              permission="system:dict:delete"
            >
              {t("删除")}
            </PermissionButton>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  const dictItemColumns: ColumnsType<DictItemRecord> = [
    { title: t("标签"), dataIndex: "label", width: 160 },
    { title: t("值"), dataIndex: "value", width: 180 },
    {
      title: t("颜色"),
      dataIndex: "color",
      width: 120,
      render: (value) => (value ? <Tag color={value}>{value}</Tag> : "-"),
    },
    {
      title: t("启用"),
      dataIndex: "enabled",
      width: 90,
      render: (value) => <Switch checked={value} disabled />,
    },
    {
      title: t("默认"),
      dataIndex: "is_default",
      width: 90,
      render: (value) => <Switch checked={value} disabled />,
    },
    {
      title: t("操作"),
      key: "actions",
      width: 170,
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:dict:update"
            onClick={() => {
              setEditingDictItem(record);
              dictItemForm.setFieldsValue(record);
              setDictItemOpen(true);
            }}
          >
            {t("编辑")}
          </PermissionButton>
          <Popconfirm
            title={t("确认删除字典项？")}
            onConfirm={() => deleteDictItemMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              disabled={record.is_default}
              permission="system:dict:delete"
            >
              {t("删除")}
            </PermissionButton>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <CrudPage
      title={t("系统设置")}
      subtitle={t("品牌、上传、安全配置和通用业务字典")}
      breadcrumb={[t("系统管理"), t("系统设置")]}
      icon={<ControlOutlined />}
    >
      <Tabs
        items={[
          {
            key: "settings",
            label: t("系统配置"),
            children: (
              <Space direction="vertical" size="middle" className="full-width">
                <Space wrap>
                  <Input.Search
                    allowClear
                    placeholder={t("搜索配置键、名称")}
                    onSearch={(value) => {
                      setSettingPage(1);
                      setSettingKeyword(value);
                    }}
                    className="admin-search-input"
                  />
                  <CrudToolbar
                    actions={[
                      {
                        key: "create",
                        label: t("新增配置"),
                        icon: <PlusOutlined />,
                        primary: true,
                        permission: "system:setting:create",
                        onClick: () => {
                          setEditingSetting(null);
                          settingForm.resetFields();
                          settingForm.setFieldsValue({
                            value_type: "string",
                            is_public: false,
                            is_builtin: false,
                            is_encrypted: false,
                          });
                          setSettingOpen(true);
                        },
                      },
                    ]}
                  />
                </Space>
                <DataTable<SettingRecord>
                  columns={settingColumns}
                  dataSource={settingsQuery.data?.items ?? []}
                  loading={settingsQuery.isLoading}
                  pagination={{
                    current: settingPage,
                    total: settingsQuery.data?.total ?? 0,
                    onChange: setSettingPage,
                  }}
                />
              </Space>
            ),
          },
          {
            key: "dicts",
            label: t("字典管理"),
            children: (
              <Space direction="vertical" size="middle" className="full-width">
                <CrudToolbar
                  actions={[
                    {
                      key: "create-type",
                      label: t("新增字典类型"),
                      icon: <PlusOutlined />,
                      primary: true,
                      permission: "system:dict:create",
                      onClick: () => {
                        setEditingDictType(null);
                        dictTypeForm.resetFields();
                        dictTypeForm.setFieldsValue({
                          enabled: true,
                          is_builtin: false,
                        });
                        setDictTypeOpen(true);
                      },
                    },
                    {
                      key: "create-item",
                      label: t("新增字典项"),
                      icon: <PlusOutlined />,
                      permission: "system:dict:create",
                      onClick: () => {
                        setEditingDictItem(null);
                        dictItemForm.resetFields();
                        dictItemForm.setFieldsValue({
                          dict_type_id: activeDictTypeId,
                          enabled: true,
                          is_default: false,
                        });
                        setDictItemOpen(true);
                      },
                    },
                  ]}
                />
                <DataTable<DictTypeRecord>
                  columns={dictTypeColumns}
                  dataSource={dictTypesQuery.data?.items ?? []}
                  loading={dictTypesQuery.isLoading}
                  rowClassName={(record) =>
                    record.id === activeDictTypeId ? "selected-list-item" : ""
                  }
                  pagination={{
                    current: dictPage,
                    total: dictTypesQuery.data?.total ?? 0,
                    onChange: setDictPage,
                  }}
                />
                <DataTable<DictItemRecord>
                  columns={dictItemColumns}
                  dataSource={dictItemsQuery.data ?? []}
                  loading={dictItemsQuery.isLoading}
                  pagination={false}
                />
              </Space>
            ),
          },
        ]}
      />
      <Modal
        title={editingSetting ? t("编辑配置") : t("新增配置")}
        open={settingOpen}
        onCancel={() => setSettingOpen(false)}
        onOk={() => settingForm.submit()}
        confirmLoading={saveSettingMutation.isPending}
      >
        <Form
          form={settingForm}
          layout="vertical"
          onFinish={(values) => saveSettingMutation.mutate(values)}
        >
          <Form.Item
            name="key"
            label={t("配置键")}
            rules={[{ required: true }]}
          >
            <Input />
          </Form.Item>
          <Form.Item name="name" label={t("名称")} rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item
            name="group_key"
            label={t("分组")}
            rules={[{ required: true }]}
          >
            <Input />
          </Form.Item>
          <Form.Item
            name="value_type"
            label={t("值类型")}
            rules={[{ required: true }]}
          >
            <Select options={valueTypeOptions} />
          </Form.Item>
          <Form.Item
            name="value"
            label={t("配置值")}
            rules={[{ required: true }]}
          >
            <Input.TextArea rows={3} />
          </Form.Item>
          <Form.Item name="default_value" label={t("默认值")}>
            <Input />
          </Form.Item>
          <Form.Item name="description" label={t("说明")}>
            <Input.TextArea rows={2} />
          </Form.Item>
          <Form.Item name="sort_order" label={t("排序")}>
            <InputNumber className="full-width" />
          </Form.Item>
          <Space>
            <Form.Item name="is_public" valuePropName="checked">
              <Switch
                checkedChildren={t("公开")}
                unCheckedChildren={t("私有")}
              />
            </Form.Item>
            <Form.Item name="is_builtin" valuePropName="checked">
              <Switch
                checkedChildren={t("内置")}
                unCheckedChildren={t("自定义")}
              />
            </Form.Item>
            <Form.Item name="is_encrypted" valuePropName="checked">
              <Switch
                checkedChildren={t("加密")}
                unCheckedChildren={t("明文")}
              />
            </Form.Item>
          </Space>
        </Form>
      </Modal>
      <Modal
        title={editingDictType ? t("编辑字典类型") : t("新增字典类型")}
        open={dictTypeOpen}
        onCancel={() => setDictTypeOpen(false)}
        onOk={() => dictTypeForm.submit()}
        confirmLoading={saveDictTypeMutation.isPending}
      >
        <Form
          form={dictTypeForm}
          layout="vertical"
          onFinish={(values) => saveDictTypeMutation.mutate(values)}
        >
          <Form.Item name="code" label={t("编码")} rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="name" label={t("名称")} rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="description" label={t("说明")}>
            <Input.TextArea rows={2} />
          </Form.Item>
          <Form.Item name="sort_order" label={t("排序")}>
            <InputNumber className="full-width" />
          </Form.Item>
          <Space>
            <Form.Item name="enabled" valuePropName="checked">
              <Switch
                checkedChildren={t("启用")}
                unCheckedChildren={t("停用")}
              />
            </Form.Item>
            <Form.Item name="is_builtin" valuePropName="checked">
              <Switch
                checkedChildren={t("内置")}
                unCheckedChildren={t("自定义")}
              />
            </Form.Item>
          </Space>
        </Form>
      </Modal>
      <Modal
        title={editingDictItem ? t("编辑字典项") : t("新增字典项")}
        open={dictItemOpen}
        onCancel={() => setDictItemOpen(false)}
        onOk={() => dictItemForm.submit()}
        confirmLoading={saveDictItemMutation.isPending}
      >
        <Form
          form={dictItemForm}
          layout="vertical"
          onFinish={(values) => saveDictItemMutation.mutate(values)}
        >
          <Form.Item
            name="dict_type_id"
            label={t("字典类型")}
            rules={[{ required: true }]}
          >
            <Select
              options={(dictTypesQuery.data?.items ?? []).map((item) => ({
                value: item.id,
                label: `${item.name}（${item.code}）`,
              }))}
            />
          </Form.Item>
          <Form.Item
            name="label"
            label={t("标签")}
            rules={[{ required: true }]}
          >
            <Input />
          </Form.Item>
          <Form.Item name="value" label={t("值")} rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="color" label={t("颜色")}>
            <Input />
          </Form.Item>
          <Form.Item name="extra" label={t("扩展 JSON")}>
            <Input.TextArea rows={2} />
          </Form.Item>
          <Form.Item name="sort_order" label={t("排序")}>
            <InputNumber className="full-width" />
          </Form.Item>
          <Space>
            <Form.Item name="enabled" valuePropName="checked">
              <Switch
                checkedChildren={t("启用")}
                unCheckedChildren={t("停用")}
              />
            </Form.Item>
            <Form.Item name="is_default" valuePropName="checked">
              <Switch
                checkedChildren={t("默认")}
                unCheckedChildren={t("普通")}
              />
            </Form.Item>
          </Space>
        </Form>
      </Modal>
    </CrudPage>
  );
}
