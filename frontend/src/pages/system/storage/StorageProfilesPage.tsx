import {
  CheckCircleOutlined,
  CloudServerOutlined,
  DeleteOutlined,
  EditOutlined,
  PlusOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Form,
  Input,
  Modal,
  Popconfirm,
  Select,
  Space,
  Switch,
  Tabs,
  Tag,
  Typography,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useMemo, useState } from "react";
import {
  type SaveStorageBucketParams,
  type SaveStorageProfileParams,
  type StorageBucketRecord,
  type StorageProfileRecord,
  createStorageBucket,
  createStorageProfile,
  deleteStorageBucket,
  deleteStorageProfile,
  fetchStorageBuckets,
  fetchStorageProfiles,
  testStorageProfile,
  updateStorageBucket,
  updateStorageProfile,
} from "../../../api/admin/storage";
import { fetchTenants } from "../../../api/admin/tenants";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { StatusTag } from "../../../components/admin/StatusTag";
import { useAuthStore } from "../../../stores/auth";

const providerOptions = [
  { value: "local", label: "本地存储" },
  { value: "s3_compatible", label: "S3 兼容 / R2 / MinIO" },
];

export function StorageProfilesPage() {
  const [page, setPage] = useState(1);
  const [keyword, setKeyword] = useState("");
  const [selectedProfileId, setSelectedProfileId] = useState<number>();
  const [editingProfile, setEditingProfile] =
    useState<StorageProfileRecord | null>(null);
  const [profileOpen, setProfileOpen] = useState(false);
  const [editingBucket, setEditingBucket] =
    useState<StorageBucketRecord | null>(null);
  const [bucketOpen, setBucketOpen] = useState(false);
  const [profileForm] = Form.useForm<SaveStorageProfileParams>();
  const [bucketForm] = Form.useForm<SaveStorageBucketParams>();
  const currentTenant = useAuthStore((state) => state.tenant);
  const effectiveDataScope = useAuthStore((state) => state.effectiveDataScope);
  const queryClient = useQueryClient();

  const profilesQuery = useQuery({
    queryKey: ["admin-storage-profiles", page, keyword],
    queryFn: () =>
      fetchStorageProfiles({
        page,
        page_size: 10,
        keyword: keyword || undefined,
      }),
  });
  const tenantsQuery = useQuery({
    queryKey: ["admin-storage-tenants"],
    enabled: effectiveDataScope === "all",
    queryFn: () => fetchTenants({ page: 1, page_size: 100 }),
  });
  const tenantOptions = useMemo(() => {
    const tenants = tenantsQuery.data?.items ?? [];
    if (
      currentTenant &&
      !tenants.some((tenant) => tenant.id === currentTenant.id)
    ) {
      return [currentTenant, ...tenants];
    }
    return tenants;
  }, [currentTenant, tenantsQuery.data?.items]);
  const defaultTenantId = currentTenant?.id ?? tenantOptions[0]?.id;
  const profiles = profilesQuery.data?.items ?? [];
  const activeProfileId = selectedProfileId ?? profiles[0]?.id;
  const activeProfile = useMemo(
    () => profiles.find((profile) => profile.id === activeProfileId),
    [activeProfileId, profiles],
  );

  const bucketsQuery = useQuery({
    queryKey: ["admin-storage-buckets", activeProfileId],
    enabled: Boolean(activeProfileId),
    queryFn: () => fetchStorageBuckets(activeProfileId ?? 0),
  });

  const saveProfileMutation = useMutation({
    mutationFn: (values: SaveStorageProfileParams) => {
      const payload = {
        ...values,
        tenant_id:
          values.tenant_id ?? editingProfile?.tenant_id ?? defaultTenantId,
      };
      return editingProfile
        ? updateStorageProfile(editingProfile.id, payload)
        : createStorageProfile(payload);
    },
    onSuccess: (profile) => {
      message.success("存储配置已保存");
      setProfileOpen(false);
      setEditingProfile(null);
      setSelectedProfileId(profile.id);
      queryClient.invalidateQueries({ queryKey: ["admin-storage-profiles"] });
    },
  });

  const deleteProfileMutation = useMutation({
    mutationFn: deleteStorageProfile,
    onSuccess: () => {
      message.success("存储配置已删除");
      setSelectedProfileId(undefined);
      queryClient.invalidateQueries({ queryKey: ["admin-storage-profiles"] });
    },
  });

  const testProfileMutation = useMutation({
    mutationFn: testStorageProfile,
    onSuccess: (result) => {
      if (result.ok) {
        message.success(result.message);
        return;
      }
      message.warning(result.message);
    },
  });

  const saveBucketMutation = useMutation({
    mutationFn: (values: SaveStorageBucketParams) => {
      const payload = {
        ...values,
        tenant_id:
          values.tenant_id ??
          editingBucket?.tenant_id ??
          activeProfile?.tenant_id,
      };
      return editingBucket
        ? updateStorageBucket(editingBucket.id, payload)
        : createStorageBucket(activeProfileId ?? 0, payload);
    },
    onSuccess: () => {
      message.success("桶配置已保存");
      setBucketOpen(false);
      setEditingBucket(null);
      queryClient.invalidateQueries({ queryKey: ["admin-storage-buckets"] });
      queryClient.invalidateQueries({ queryKey: ["admin-storage-profiles"] });
    },
  });

  const deleteBucketMutation = useMutation({
    mutationFn: deleteStorageBucket,
    onSuccess: () => {
      message.success("桶配置已删除");
      queryClient.invalidateQueries({ queryKey: ["admin-storage-buckets"] });
      queryClient.invalidateQueries({ queryKey: ["admin-storage-profiles"] });
    },
  });

  const openCreateProfile = () => {
    setEditingProfile(null);
    profileForm.resetFields();
    profileForm.setFieldsValue({
      tenant_id: defaultTenantId,
      provider: "local",
      enabled: true,
      is_default: false,
      path_style: false,
    });
    setProfileOpen(true);
  };

  const openEditProfile = (record: StorageProfileRecord) => {
    setEditingProfile(record);
    profileForm.setFieldsValue(record);
    setProfileOpen(true);
  };

  const openCreateBucket = () => {
    setEditingBucket(null);
    bucketForm.resetFields();
    bucketForm.setFieldsValue({
      tenant_id: activeProfile?.tenant_id,
      enabled: true,
      is_default: false,
    });
    setBucketOpen(true);
  };

  const openEditBucket = (record: StorageBucketRecord) => {
    setEditingBucket(record);
    bucketForm.setFieldsValue(record);
    setBucketOpen(true);
  };

  const profileColumns: ColumnsType<StorageProfileRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    { title: "名称", dataIndex: "name", width: 180, ellipsis: true },
    { title: "编码", dataIndex: "code", width: 170, ellipsis: true },
    {
      title: "类型",
      dataIndex: "provider",
      width: 150,
      render: (value) => <Tag icon={<CloudServerOutlined />}>{value}</Tag>,
    },
    {
      title: "本地路径 / Endpoint",
      key: "location",
      width: 300,
      render: (_, record) => {
        const value =
          record.provider === "local" ? record.local_root : record.endpoint;
        if (!value) {
          return <Typography.Text type="secondary">-</Typography.Text>;
        }
        return (
          <Typography.Text
            ellipsis={{ tooltip: value }}
            copyable={{
              text: value,
              tooltips: [
                record.provider === "local" ? "复制本地路径" : "复制 Endpoint",
                "已复制",
              ],
            }}
            style={{ maxWidth: 260 }}
          >
            {value}
          </Typography.Text>
        );
      },
    },
    {
      title: "存储桶 / Region",
      key: "bucket-region",
      width: 160,
      ellipsis: true,
      render: (_, record) =>
        record.provider === "local"
          ? (record.storage_bucket_name ?? "-")
          : (record.region ?? "-"),
    },
    {
      title: "启用",
      dataIndex: "enabled",
      width: 90,
      render: (value) => <StatusTag active={value} />,
    },
    {
      title: "默认",
      dataIndex: "is_default",
      width: 90,
      render: (value) => <StatusTag active={value} />,
    },
    { title: "创建时间", dataIndex: "created_at", width: 210 },
    {
      title: "操作",
      key: "actions",
      width: 280,
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            permission="system:storage:list"
            onClick={() => setSelectedProfileId(record.id)}
          >
            桶配置
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<CheckCircleOutlined />}
            permission="system:storage:test"
            loading={testProfileMutation.isPending}
            onClick={() => testProfileMutation.mutate(record.id)}
          >
            测试
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:storage:update"
            onClick={() => openEditProfile(record)}
          >
            编辑
          </PermissionButton>
          <Popconfirm
            title="确认删除存储配置？"
            onConfirm={() => deleteProfileMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              icon={<DeleteOutlined />}
              permission="system:storage:delete"
            >
              删除
            </PermissionButton>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  const bucketColumns: ColumnsType<StorageBucketRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    { title: "名称", dataIndex: "name", width: 180, ellipsis: true },
    { title: "桶", dataIndex: "bucket", width: 180, ellipsis: true },
    {
      title: "基础 Prefix",
      dataIndex: "base_prefix",
      width: 180,
      ellipsis: true,
      render: (value) => value ?? "-",
    },
    {
      title: "本地根目录",
      dataIndex: "local_root",
      width: 240,
      ellipsis: true,
      render: (value) => value ?? "-",
    },
    {
      title: "公开前缀",
      dataIndex: "public_prefix",
      width: 220,
      ellipsis: true,
      render: (value) => value ?? "-",
    },
    {
      title: "启用",
      dataIndex: "enabled",
      width: 90,
      render: (value) => <StatusTag active={value} />,
    },
    {
      title: "默认",
      dataIndex: "is_default",
      width: 90,
      render: (value) => <StatusTag active={value} />,
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
            permission="system:storage:update"
            onClick={() => openEditBucket(record)}
          >
            编辑
          </PermissionButton>
          <Popconfirm
            title="确认删除桶配置？"
            onConfirm={() => deleteBucketMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              icon={<DeleteOutlined />}
              permission="system:storage:delete"
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
      title="存储配置"
      subtitle="统一配置本地存储、Cloudflare R2 和 S3 兼容资源桶"
      breadcrumb={["系统管理", "存储配置"]}
      icon={<CloudServerOutlined />}
    >
      <Tabs
        items={[
          {
            key: "profiles",
            label: "存储配置",
            children: (
              <Space direction="vertical" size="middle" className="full-width">
                <Space wrap>
                  <Input.Search
                    allowClear
                    placeholder="搜索名称、编码"
                    onSearch={(value) => {
                      setPage(1);
                      setKeyword(value);
                    }}
                    className="admin-search-input"
                  />
                  <CrudToolbar
                    actions={[
                      {
                        key: "create",
                        label: "新增配置",
                        icon: <PlusOutlined />,
                        primary: true,
                        permission: "system:storage:create",
                        onClick: openCreateProfile,
                      },
                    ]}
                  />
                </Space>
                <DataTable<StorageProfileRecord>
                  columns={profileColumns}
                  dataSource={profiles}
                  loading={profilesQuery.isLoading}
                  rowClassName={(record) =>
                    record.id === activeProfileId ? "selected-list-item" : ""
                  }
                  pagination={{
                    current: page,
                    total: profilesQuery.data?.total ?? 0,
                    onChange: setPage,
                  }}
                />
              </Space>
            ),
          },
          {
            key: "buckets",
            label: "桶配置",
            children: (
              <Space direction="vertical" size="middle" className="full-width">
                <Space wrap>
                  <Select
                    placeholder="选择存储配置"
                    value={activeProfileId}
                    options={profiles.map((profile) => ({
                      value: profile.id,
                      label: `${profile.name} / ${profile.provider}`,
                    }))}
                    className="admin-filter-select"
                    onChange={setSelectedProfileId}
                  />
                  {activeProfile ? (
                    <Tag color="blue">{activeProfile.code}</Tag>
                  ) : null}
                  <CrudToolbar
                    actions={[
                      {
                        key: "create-bucket",
                        label: "新增桶",
                        icon: <PlusOutlined />,
                        primary: true,
                        permission: "system:storage:create",
                        disabled: !activeProfileId,
                        onClick: openCreateBucket,
                      },
                    ]}
                  />
                </Space>
                <DataTable<StorageBucketRecord>
                  columns={bucketColumns}
                  dataSource={bucketsQuery.data ?? []}
                  loading={bucketsQuery.isLoading}
                  pagination={false}
                />
              </Space>
            ),
          },
        ]}
      />
      <Modal
        title={editingProfile ? "编辑存储配置" : "新增存储配置"}
        open={profileOpen}
        onCancel={() => setProfileOpen(false)}
        onOk={() => profileForm.submit()}
        confirmLoading={saveProfileMutation.isPending}
        width={720}
      >
        <Form
          form={profileForm}
          layout="vertical"
          onFinish={(values) => saveProfileMutation.mutate(values)}
        >
          <Form.Item
            name="tenant_id"
            label="所属租户"
            rules={[{ required: true }]}
          >
            <Select
              disabled={Boolean(editingProfile) || effectiveDataScope !== "all"}
              options={tenantOptions.map((tenant) => ({
                value: tenant.id,
                label: `${tenant.name} / ${tenant.code}`,
              }))}
              placeholder="请选择租户"
            />
          </Form.Item>
          <Form.Item name="name" label="名称" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="code" label="编码" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item
            name="provider"
            label="存储类型"
            rules={[{ required: true }]}
          >
            <Select options={providerOptions} />
          </Form.Item>
          <Form.Item name="endpoint" label="Endpoint">
            <Input placeholder="R2 / MinIO / S3 兼容服务地址" />
          </Form.Item>
          <Form.Item name="region" label="Region">
            <Input />
          </Form.Item>
          <Form.Item name="access_key_id" label="Access Key ID">
            <Input />
          </Form.Item>
          <Form.Item name="secret_access_key" label="Secret Access Key">
            <Input.Password />
          </Form.Item>
          <Form.Item name="public_base_url" label="公开访问 Base URL">
            <Input />
          </Form.Item>
          <Form.Item name="description" label="说明">
            <Input.TextArea rows={2} />
          </Form.Item>
          <Space wrap>
            <Form.Item name="enabled" valuePropName="checked">
              <Switch checkedChildren="启用" unCheckedChildren="停用" />
            </Form.Item>
            <Form.Item name="is_default" valuePropName="checked">
              <Switch checkedChildren="默认" unCheckedChildren="普通" />
            </Form.Item>
            <Form.Item name="path_style" valuePropName="checked">
              <Switch
                checkedChildren="Path Style"
                unCheckedChildren="Virtual Host"
              />
            </Form.Item>
          </Space>
        </Form>
      </Modal>
      <Modal
        title={editingBucket ? "编辑桶配置" : "新增桶配置"}
        open={bucketOpen}
        onCancel={() => setBucketOpen(false)}
        onOk={() => bucketForm.submit()}
        confirmLoading={saveBucketMutation.isPending}
        width={680}
      >
        <Form
          form={bucketForm}
          layout="vertical"
          onFinish={(values) => saveBucketMutation.mutate(values)}
        >
          <Form.Item name="tenant_id" label="所属租户">
            <Select
              disabled
              options={tenantOptions.map((tenant) => ({
                value: tenant.id,
                label: `${tenant.name} / ${tenant.code}`,
              }))}
              placeholder="跟随当前存储配置"
            />
          </Form.Item>
          <Form.Item name="name" label="名称" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item
            name="bucket"
            label="桶 / 本地桶标识"
            rules={[{ required: true }]}
          >
            <Input />
          </Form.Item>
          <Form.Item name="base_prefix" label="基础 Prefix">
            <Input />
          </Form.Item>
          <Form.Item name="local_root" label="本地根目录">
            <Input placeholder="本地存储使用，例如 storage/uploads/platform-assets" />
          </Form.Item>
          <Form.Item name="public_prefix" label="公开访问 Prefix">
            <Input />
          </Form.Item>
          <Space wrap>
            <Form.Item name="enabled" valuePropName="checked">
              <Switch checkedChildren="启用" unCheckedChildren="停用" />
            </Form.Item>
            <Form.Item name="is_default" valuePropName="checked">
              <Switch checkedChildren="默认" unCheckedChildren="普通" />
            </Form.Item>
          </Space>
        </Form>
      </Modal>
    </CrudPage>
  );
}
