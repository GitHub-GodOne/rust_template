import {
  CopyOutlined,
  DeleteOutlined,
  EditOutlined,
  KeyOutlined,
  PlusOutlined,
  ReloadOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  Button,
  Form,
  Input,
  Modal,
  Popconfirm,
  Select,
  Space,
  Switch,
  Tag,
  Typography,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useState } from "react";
import {
  type ApiKeyRecord,
  type SaveApiKeyParams,
  createApiKey,
  deleteApiKey,
  fetchApiKeys,
  updateApiKey,
} from "../../../api/admin/apiKeys";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { StatusTag } from "../../../components/admin/StatusTag";

function normalizeEmpty(value?: string | null) {
  return value?.trim() ? value.trim() : null;
}

function formatDateTime(value?: string | null) {
  if (!value) {
    return "-";
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(date);
}

function quoteCurlValue(value: string) {
  return `'${value.replaceAll("'", "'\\''")}'`;
}

function buildApiCurl(token: string) {
  const url = new URL("/api/auth/current", window.location.origin).toString();
  return [
    `curl ${quoteCurlValue(url)} \\`,
    `  -H ${quoteCurlValue(`Authorization: Bearer ${token}`)} \\`,
    `  -H ${quoteCurlValue("Accept: application/json")}`,
  ].join("\n");
}

export function ApiKeysPage() {
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(10);
  const [keywordInput, setKeywordInput] = useState("");
  const [keyword, setKeyword] = useState("");
  const [enabled, setEnabled] = useState<boolean>();
  const [editing, setEditing] = useState<ApiKeyRecord | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [form] = Form.useForm<SaveApiKeyParams>();
  const queryClient = useQueryClient();

  const apiKeysQuery = useQuery({
    queryKey: ["admin-api-keys", page, pageSize, keyword, enabled],
    queryFn: () =>
      fetchApiKeys({
        page,
        page_size: pageSize,
        keyword: keyword || undefined,
        enabled,
      }),
  });

  const saveMutation = useMutation({
    mutationFn: (values: SaveApiKeyParams) => {
      const payload: SaveApiKeyParams = {
        name: values.name.trim(),
        token: normalizeEmpty(values.token),
        description: normalizeEmpty(values.description),
        enabled: values.enabled ?? true,
      };
      return editing
        ? updateApiKey(editing.id, payload)
        : createApiKey(payload);
    },
    onSuccess: (record) => {
      message.success(editing ? "API 授权已更新" : "API 授权已创建");
      setFormOpen(false);
      setEditing(null);
      form.resetFields();
      queryClient.invalidateQueries({ queryKey: ["admin-api-keys"] });
      if (!editing) {
        void navigator.clipboard
          .writeText(record.token)
          .then(() => message.success("新密钥已复制"))
          .catch(() => undefined);
      }
    },
    onError: (error) => {
      message.error(error.message || "API 授权保存失败");
    },
  });

  const deleteMutation = useMutation({
    mutationFn: deleteApiKey,
    onSuccess: () => {
      message.success("API 授权已删除");
      queryClient.invalidateQueries({ queryKey: ["admin-api-keys"] });
    },
    onError: (error) => {
      message.error(error.message || "API 授权删除失败");
    },
  });

  const copyText = async (value: string, success: string) => {
    try {
      await navigator.clipboard.writeText(value);
      message.success(success);
    } catch {
      message.error("复制失败，请检查浏览器剪贴板权限");
    }
  };

  const openEditor = (record?: ApiKeyRecord) => {
    setEditing(record ?? null);
    form.resetFields();
    form.setFieldsValue(
      record
        ? {
            name: record.name,
            token: record.token,
            description: record.description,
            enabled: record.enabled,
          }
        : { enabled: true },
    );
    setFormOpen(true);
  };

  const columns: ColumnsType<ApiKeyRecord> = [
    { title: "ID", dataIndex: "id", width: 72 },
    { title: "名称", dataIndex: "name", width: 180, ellipsis: true },
    {
      title: "Bearer Token",
      dataIndex: "token",
      width: 340,
      render: (value) => (
        <Typography.Text
          className="api-key-token"
          code
          ellipsis={{ tooltip: String(value) }}
          copyable={{ text: String(value), tooltips: ["复制密钥", "已复制"] }}
        >
          {String(value)}
        </Typography.Text>
      ),
    },
    {
      title: "说明",
      dataIndex: "description",
      width: 220,
      ellipsis: true,
      render: (value) => value || "-",
    },
    {
      title: "状态",
      dataIndex: "enabled",
      width: 90,
      render: (value) => <StatusTag active={value} />,
    },
    {
      title: "权限主体",
      dataIndex: "created_by",
      width: 150,
      render: (value) =>
        value ? <Tag color="default">用户 #{value}</Tag> : <Tag>未绑定</Tag>,
    },
    {
      title: "最近调用",
      dataIndex: "last_used_at",
      width: 190,
      render: formatDateTime,
    },
    {
      title: "创建时间",
      dataIndex: "created_at",
      width: 190,
      render: formatDateTime,
    },
    {
      title: "操作",
      key: "actions",
      width: 330,
      fixed: "right",
      render: (_, record) => (
        <Space>
          <Button
            size="small"
            icon={<CopyOutlined />}
            onClick={() =>
              copyText(buildApiCurl(record.token), "通用 curl 已复制")
            }
          >
            复制 curl
          </Button>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:api_key:update"
            onClick={() => openEditor(record)}
          >
            编辑
          </PermissionButton>
          <Popconfirm
            title="确认删除这个 API 授权？"
            description="使用该密钥的第三方应用会立即失去访问权限。"
            onConfirm={() => deleteMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              icon={<DeleteOutlined />}
              permission="system:api_key:delete"
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
      title="API 授权"
      subtitle="管理可长期调用后端接口的 Bearer 密钥"
      breadcrumb={["系统管理", "API 授权"]}
      icon={<KeyOutlined />}
      toolbar={
        <CrudToolbar
          actions={[
            {
              key: "create",
              label: "新增授权",
              icon: <PlusOutlined />,
              primary: true,
              permission: "system:api_key:create",
              onClick: () => openEditor(),
            },
            {
              key: "refresh",
              label: "刷新",
              icon: <ReloadOutlined />,
              onClick: () => apiKeysQuery.refetch(),
            },
          ]}
        />
      }
    >
      <Alert
        className="api-key-scope-alert"
        type="info"
        showIcon
        message="永久 API 密钥可用于全部后端接口"
        description="密钥不会绕过权限：每次请求都会继承创建者当前的角色和功能权限。停用密钥、删除所属用户或移除权限后会立即失效。"
      />
      <Space wrap className="api-key-filter-bar">
        <Input.Search
          allowClear
          placeholder="搜索名称或说明"
          value={keywordInput}
          onChange={(event) => {
            const value = event.target.value;
            setKeywordInput(value);
            if (!value) {
              setPage(1);
              setKeyword("");
            }
          }}
          onSearch={(value) => {
            setPage(1);
            setKeyword(value.trim());
          }}
          className="admin-search-input"
        />
        <Select
          allowClear
          placeholder="全部状态"
          value={enabled}
          onChange={(value) => {
            setPage(1);
            setEnabled(value);
          }}
          options={[
            { value: true, label: "已启用" },
            { value: false, label: "已停用" },
          ]}
          className="admin-filter-select"
        />
      </Space>
      <DataTable<ApiKeyRecord>
        columns={columns}
        dataSource={apiKeysQuery.data?.items ?? []}
        loading={apiKeysQuery.isLoading}
        pagination={{
          current: page,
          pageSize,
          total: apiKeysQuery.data?.total ?? 0,
          onChange: (nextPage, nextPageSize) => {
            setPage(nextPageSize === pageSize ? nextPage : 1);
            setPageSize(nextPageSize);
          },
        }}
      />
      <Modal
        title={editing ? "编辑 API 授权" : "新增 API 授权"}
        open={formOpen}
        onCancel={() => setFormOpen(false)}
        onOk={() => form.submit()}
        confirmLoading={saveMutation.isPending}
        width={680}
      >
        <Alert
          type="warning"
          showIcon
          message="请把密钥当作密码保存"
          description="自定义 Token 不能包含空格，也不能使用登录 JWT 的三段式格式。创建时留空会自动生成 eyJ0_ 开头的永久密钥；编辑时清空可轮换为新密钥。"
          className="api-key-form-alert"
        />
        <Form
          form={form}
          layout="vertical"
          onFinish={(values) => saveMutation.mutate(values)}
        >
          <Form.Item
            name="name"
            label="名称"
            rules={[
              { required: true, whitespace: true, message: "请输入授权名称" },
            ]}
          >
            <Input placeholder="例如 n8n 自动化" maxLength={120} />
          </Form.Item>
          <Form.Item name="token" label="Bearer Token">
            <Input.TextArea
              rows={3}
              autoSize={{ minRows: 3, maxRows: 6 }}
              placeholder="留空自动生成，或输入不含空格的自定义密钥"
            />
          </Form.Item>
          <Form.Item name="description" label="说明">
            <Input.TextArea rows={2} maxLength={500} />
          </Form.Item>
          <Form.Item name="enabled" label="启用" valuePropName="checked">
            <Switch />
          </Form.Item>
        </Form>
      </Modal>
    </CrudPage>
  );
}
