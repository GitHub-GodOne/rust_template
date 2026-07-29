import {
  ApiOutlined,
  CheckCircleOutlined,
  DeleteOutlined,
  DownloadOutlined,
  EditOutlined,
  PlayCircleOutlined,
  PlusOutlined,
  ReloadOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  Button,
  Card,
  Descriptions,
  Drawer,
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
  Typography,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useEffect, useMemo, useState } from "react";
import {
  type HttpClientRuntimeConfig,
  type HttpRequestDraft,
  type HttpRequestExecuteParams,
  type HttpRequestPresetRecord,
  type HttpRequestRunRecord,
  type SaveHttpRequestPresetParams,
  createHttpRequestPreset,
  deleteHttpRequestPreset,
  downloadHttpRequestRunResult,
  fetchHttpClientConfig,
  fetchHttpRequestPresets,
  fetchHttpRequestRun,
  fetchHttpRequestRuns,
  parseHttpRequestImport,
  sendAdHocHttpRequest,
  sendHttpRequestPreset,
  testHttpClientRequest,
  updateHttpClientConfig,
  updateHttpRequestPreset,
} from "../../../api/admin/httpClient";
import { CrudPage } from "../../../components/admin/CrudPage";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { StatusTag } from "../../../components/admin/StatusTag";

const defaultConfig: HttpClientRuntimeConfig = {
  enabled: true,
  request_timeout_seconds: 120,
  connect_timeout_seconds: 20,
  pool_idle_timeout_seconds: 90,
  proxy_enabled: false,
  proxy_url: "",
  danger_accept_invalid_certs: false,
  user_agent: "",
};

const methodOptions = [
  "GET",
  "POST",
  "PUT",
  "PATCH",
  "DELETE",
  "HEAD",
  "OPTIONS",
].map((method) => ({ value: method, label: method }));

const bodyTypeOptions = [
  { value: "none", label: "无" },
  { value: "json", label: "JSON" },
  { value: "form", label: "Form" },
  { value: "raw", label: "Raw" },
];

const statusColor: Record<string, string> = {
  success: "green",
  http_error: "orange",
  network_error: "red",
};

type TestFormValues = {
  url: string;
};

type RequestFormValues = SaveHttpRequestPresetParams;

type ImportFormValues = {
  format: string;
  content: string;
};

function normalizeEmpty(value?: string | null) {
  return value?.trim() ? value : null;
}

function requestPayload(
  values: RequestFormValues,
): SaveHttpRequestPresetParams {
  return {
    ...values,
    description: normalizeEmpty(values.description),
    headers_json: normalizeEmpty(values.headers_json),
    query_json: normalizeEmpty(values.query_json),
    body_type: values.body_type ?? "none",
    body_text: normalizeEmpty(values.body_text),
    content_type: normalizeEmpty(values.content_type),
    source_format: values.source_format ?? "manual",
    enabled: values.enabled ?? true,
  };
}

function executePayload(values: RequestFormValues): HttpRequestExecuteParams {
  return {
    name: values.name,
    method: values.method,
    url: values.url,
    headers_json: normalizeEmpty(values.headers_json),
    query_json: normalizeEmpty(values.query_json),
    body_type: values.body_type ?? "none",
    body_text: normalizeEmpty(values.body_text),
    content_type: normalizeEmpty(values.content_type),
  };
}

function draftToFormValues(
  draft: HttpRequestDraft,
): Partial<RequestFormValues> {
  return {
    name: draft.name ?? "导入请求",
    code: `request_${Date.now()}`,
    method: draft.method,
    url: draft.url,
    headers_json: draft.headers_json,
    query_json: draft.query_json,
    body_type: draft.body_type,
    body_text: draft.body_text,
    content_type: draft.content_type,
    source_format: draft.source_format ?? "manual",
    enabled: true,
  };
}

export function HttpClientPage() {
  const [configForm] = Form.useForm<HttpClientRuntimeConfig>();
  const [testForm] = Form.useForm<TestFormValues>();
  const [requestForm] = Form.useForm<RequestFormValues>();
  const [importForm] = Form.useForm<ImportFormValues>();
  const [editing, setEditing] = useState<HttpRequestPresetRecord | null>(null);
  const [requestOpen, setRequestOpen] = useState(false);
  const [detailRunId, setDetailRunId] = useState<number | null>(null);
  const queryClient = useQueryClient();
  const proxyEnabled = Form.useWatch("proxy_enabled", configForm) ?? false;

  const configQuery = useQuery({
    queryKey: ["admin-http-client-config"],
    queryFn: fetchHttpClientConfig,
  });
  const presetsQuery = useQuery({
    queryKey: ["admin-http-request-presets"],
    queryFn: () => fetchHttpRequestPresets({ page_size: 50 }),
  });
  const runsQuery = useQuery({
    queryKey: ["admin-http-request-runs"],
    queryFn: () => fetchHttpRequestRuns({ page_size: 50 }),
  });
  const detailQuery = useQuery({
    queryKey: ["admin-http-request-run", detailRunId],
    queryFn: () => fetchHttpRequestRun(detailRunId as number),
    enabled: Boolean(detailRunId),
  });

  useEffect(() => {
    if (configQuery.data) {
      configForm.setFieldsValue({
        ...defaultConfig,
        ...configQuery.data,
        proxy_url: configQuery.data.proxy_url ?? "",
        user_agent: configQuery.data.user_agent ?? "",
      });
    }
  }, [configForm, configQuery.data]);

  const saveConfigMutation = useMutation({
    mutationFn: updateHttpClientConfig,
    onSuccess: (config) => {
      message.success("HTTP 客户端配置已保存");
      configForm.setFieldsValue({
        ...config,
        proxy_url: config.proxy_url ?? "",
        user_agent: config.user_agent ?? "",
      });
      queryClient.setQueryData(["admin-http-client-config"], config);
    },
    onError: (error) =>
      message.error(error.message || "HTTP 客户端配置保存失败"),
  });
  const testMutation = useMutation({ mutationFn: testHttpClientRequest });
  const savePresetMutation = useMutation({
    mutationFn: (values: RequestFormValues) =>
      editing
        ? updateHttpRequestPreset(editing.id, requestPayload(values))
        : createHttpRequestPreset(requestPayload(values)),
    onSuccess: () => {
      message.success("请求已保存");
      setRequestOpen(false);
      setEditing(null);
      queryClient.invalidateQueries({
        queryKey: ["admin-http-request-presets"],
      });
    },
    onError: (error) => message.error(error.message || "请求保存失败"),
  });
  const deletePresetMutation = useMutation({
    mutationFn: deleteHttpRequestPreset,
    onSuccess: () => {
      message.success("请求已删除");
      queryClient.invalidateQueries({
        queryKey: ["admin-http-request-presets"],
      });
    },
    onError: (error) => message.error(error.message || "请求删除失败"),
  });
  const sendPresetMutation = useMutation({
    mutationFn: sendHttpRequestPreset,
    onSuccess: (run) => {
      message.success("请求已发送");
      setDetailRunId(run.id);
      queryClient.invalidateQueries({ queryKey: ["admin-http-request-runs"] });
      queryClient.invalidateQueries({
        queryKey: ["admin-http-request-presets"],
      });
    },
    onError: (error) => message.error(error.message || "请求发送失败"),
  });
  const sendAdHocMutation = useMutation({
    mutationFn: (values: RequestFormValues) =>
      sendAdHocHttpRequest(executePayload(values)),
    onSuccess: (run) => {
      message.success("请求已发送");
      setDetailRunId(run.id);
      queryClient.invalidateQueries({ queryKey: ["admin-http-request-runs"] });
    },
    onError: (error) => message.error(error.message || "请求发送失败"),
  });
  const importMutation = useMutation({
    mutationFn: parseHttpRequestImport,
    onSuccess: (draft) => {
      requestForm.setFieldsValue(draftToFormValues(draft));
      message.success("请求已解析到编辑器");
    },
    onError: (error) => message.error(error.message || "请求解析失败"),
  });
  const downloadMutation = useMutation({
    mutationFn: downloadHttpRequestRunResult,
    onError: (error) => message.error(error.message || "结果下载失败"),
  });

  const openRequestEditor = (record?: HttpRequestPresetRecord) => {
    setEditing(record ?? null);
    requestForm.resetFields();
    requestForm.setFieldsValue(
      record ?? {
        method: "GET",
        body_type: "none",
        source_format: "manual",
        enabled: true,
      },
    );
    setRequestOpen(true);
  };

  const requestColumns: ColumnsType<HttpRequestPresetRecord> = [
    { title: "ID", dataIndex: "id", width: 70 },
    { title: "名称", dataIndex: "name", width: 180 },
    { title: "编码", dataIndex: "code", width: 180 },
    {
      title: "方法",
      dataIndex: "method",
      width: 90,
      render: (value) => <Tag>{value}</Tag>,
    },
    { title: "URL", dataIndex: "url", width: 360, ellipsis: true },
    {
      title: "启用",
      dataIndex: "enabled",
      width: 90,
      render: (value) => <StatusTag active={value} />,
    },
    {
      title: "操作",
      key: "actions",
      width: 300,
      fixed: "right",
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            icon={<PlayCircleOutlined />}
            permission="system:http_client:request:send"
            loading={sendPresetMutation.isPending}
            onClick={() => sendPresetMutation.mutate(record.id)}
          >
            发送
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:http_client:request:update"
            onClick={() => openRequestEditor(record)}
          >
            编辑
          </PermissionButton>
          <Popconfirm
            title="确认删除请求？"
            onConfirm={() => deletePresetMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              icon={<DeleteOutlined />}
              permission="system:http_client:request:delete"
            >
              删除
            </PermissionButton>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  const runColumns: ColumnsType<HttpRequestRunRecord> = [
    { title: "ID", dataIndex: "id", width: 70 },
    { title: "名称", dataIndex: "name", width: 180 },
    {
      title: "方法",
      dataIndex: "method",
      width: 90,
      render: (value) => <Tag>{value}</Tag>,
    },
    { title: "URL", dataIndex: "url", width: 360, ellipsis: true },
    {
      title: "状态",
      dataIndex: "status",
      width: 120,
      render: (value) => (
        <Tag color={statusColor[value] ?? "default"}>{value}</Tag>
      ),
    },
    {
      title: "HTTP",
      dataIndex: "response_status_code",
      width: 90,
      render: (value) => value ?? "-",
    },
    {
      title: "耗时",
      dataIndex: "duration_ms",
      width: 100,
      render: (value) => `${value} ms`,
    },
    { title: "时间", dataIndex: "created_at", width: 210 },
    {
      title: "操作",
      key: "actions",
      width: 180,
      fixed: "right",
      render: (_, record) => (
        <Space>
          <Button size="small" onClick={() => setDetailRunId(record.id)}>
            详情
          </Button>
          {record.has_response_body ? (
            <PermissionButton
              size="small"
              icon={<DownloadOutlined />}
              permission="system:http_client:run:download"
              loading={downloadMutation.isPending}
              onClick={() => downloadMutation.mutate(record)}
            >
              下载
            </PermissionButton>
          ) : null}
        </Space>
      ),
    },
  ];

  const detailRun = detailQuery.data;
  const requestEditor = useMemo(
    () => (
      <Form
        form={requestForm}
        layout="vertical"
        onFinish={(values) => savePresetMutation.mutate(values)}
      >
        <Space wrap className="full-width">
          <Form.Item
            name="name"
            label="名称"
            rules={[{ required: true, message: "请输入名称" }]}
          >
            <Input placeholder="例如 查询用户接口" />
          </Form.Item>
          <Form.Item
            name="code"
            label="编码"
            rules={[{ required: true, message: "请输入编码" }]}
          >
            <Input placeholder="例如 get_users" />
          </Form.Item>
          <Form.Item
            name="method"
            label="方法"
            rules={[{ required: true, message: "请选择方法" }]}
          >
            <Select options={methodOptions} style={{ width: 120 }} />
          </Form.Item>
          <Form.Item name="enabled" label="启用" valuePropName="checked">
            <Switch />
          </Form.Item>
        </Space>
        <Form.Item
          name="url"
          label="URL"
          rules={[{ required: true, message: "请输入 URL" }]}
        >
          <Input placeholder="https://api.example.com/users" />
        </Form.Item>
        <Form.Item name="description" label="描述">
          <Input.TextArea rows={2} />
        </Form.Item>
        <Form.Item name="headers_json" label="请求头 JSON">
          <Input.TextArea
            rows={3}
            placeholder='{"Authorization":"Bearer token"}'
          />
        </Form.Item>
        <Form.Item name="query_json" label="查询参数 JSON">
          <Input.TextArea rows={2} placeholder='{"page":"1"}' />
        </Form.Item>
        <Space wrap className="full-width">
          <Form.Item name="body_type" label="Body 类型">
            <Select options={bodyTypeOptions} style={{ width: 140 }} />
          </Form.Item>
          <Form.Item name="content_type" label="Content-Type">
            <Input placeholder="application/json" style={{ width: 260 }} />
          </Form.Item>
        </Space>
        <Form.Item name="body_text" label="Body 内容">
          <Input.TextArea rows={6} placeholder='{"name":"demo"}' />
        </Form.Item>
        <Space wrap>
          <PermissionButton
            type="primary"
            permission={
              editing
                ? "system:http_client:request:update"
                : "system:http_client:request:create"
            }
            loading={savePresetMutation.isPending}
            onClick={() => requestForm.submit()}
          >
            保存请求
          </PermissionButton>
          <PermissionButton
            icon={<PlayCircleOutlined />}
            permission="system:http_client:request:send"
            loading={sendAdHocMutation.isPending}
            onClick={() =>
              requestForm
                .validateFields()
                .then((values) => sendAdHocMutation.mutate(values))
            }
          >
            仅发送
          </PermissionButton>
        </Space>
      </Form>
    ),
    [editing, requestForm, savePresetMutation, sendAdHocMutation],
  );

  return (
    <CrudPage
      title="HTTP 客户端"
      subtitle="统一管理项目对外请求配置、请求模板与执行结果"
      breadcrumb={["系统管理", "HTTP 客户端"]}
      icon={<ApiOutlined />}
      notice="可配置后端外部 HTTP 请求参数，也可以保存常用请求、发送请求并下载响应结果。"
      toolbar={
        <Space wrap>
          <Button
            icon={<ReloadOutlined />}
            onClick={() => {
              configQuery.refetch();
              presetsQuery.refetch();
              runsQuery.refetch();
            }}
          >
            刷新
          </Button>
          <PermissionButton
            type="primary"
            icon={<PlusOutlined />}
            permission="system:http_client:request:create"
            onClick={() => openRequestEditor()}
          >
            新增请求
          </PermissionButton>
        </Space>
      }
    >
      <Tabs
        items={[
          {
            key: "config",
            label: "运行时配置",
            children: (
              <Space direction="vertical" size="middle" className="full-width">
                <Card title="运行时参数" className="admin-card">
                  <Form
                    form={configForm}
                    layout="vertical"
                    initialValues={defaultConfig}
                    onFinish={(values) =>
                      saveConfigMutation.mutate({
                        ...values,
                        proxy_url: values.proxy_url?.trim() || undefined,
                        user_agent: values.user_agent?.trim() || undefined,
                      })
                    }
                  >
                    <Space wrap size={24}>
                      <Form.Item
                        name="enabled"
                        label="启用全局配置"
                        valuePropName="checked"
                      >
                        <Switch
                          checkedChildren="启用"
                          unCheckedChildren="默认"
                        />
                      </Form.Item>
                      <Form.Item
                        name="danger_accept_invalid_certs"
                        label="接受无效证书"
                        valuePropName="checked"
                      >
                        <Switch checkedChildren="是" unCheckedChildren="否" />
                      </Form.Item>
                      <Form.Item
                        name="proxy_enabled"
                        label="启用代理"
                        valuePropName="checked"
                      >
                        <Switch
                          checkedChildren="开启"
                          unCheckedChildren="关闭"
                        />
                      </Form.Item>
                    </Space>
                    <div className="http-client-grid">
                      <Form.Item
                        name="request_timeout_seconds"
                        label="整体请求超时（秒）"
                        rules={[
                          { required: true, message: "请输入整体请求超时" },
                        ]}
                      >
                        <InputNumber
                          min={1}
                          max={3600}
                          style={{ width: "100%" }}
                        />
                      </Form.Item>
                      <Form.Item
                        name="connect_timeout_seconds"
                        label="连接超时（秒）"
                        rules={[{ required: true, message: "请输入连接超时" }]}
                      >
                        <InputNumber
                          min={1}
                          max={3600}
                          style={{ width: "100%" }}
                        />
                      </Form.Item>
                      <Form.Item
                        name="pool_idle_timeout_seconds"
                        label="连接池空闲超时（秒）"
                        rules={[
                          { required: true, message: "请输入连接池空闲超时" },
                        ]}
                      >
                        <InputNumber
                          min={1}
                          max={3600}
                          style={{ width: "100%" }}
                        />
                      </Form.Item>
                      <Form.Item name="user_agent" label="全局 User-Agent">
                        <Input placeholder="留空则使用 reqwest 默认值" />
                      </Form.Item>
                    </div>
                    <Form.Item
                      name="proxy_url"
                      label="代理地址"
                      rules={
                        proxyEnabled
                          ? [
                              {
                                required: true,
                                message: "启用代理后必须填写代理地址",
                              },
                            ]
                          : []
                      }
                    >
                      <Input placeholder="http://127.0.0.1:7890 或 socks5://127.0.0.1:1080" />
                    </Form.Item>
                  </Form>
                  <PermissionButton
                    type="primary"
                    icon={<CheckCircleOutlined />}
                    permission="system:http_client:config"
                    loading={saveConfigMutation.isPending}
                    onClick={() => configForm.submit()}
                  >
                    保存配置
                  </PermissionButton>
                </Card>
                <Card title="测试请求" className="admin-card">
                  <Form
                    form={testForm}
                    layout="inline"
                    initialValues={{ url: "https://example.com" }}
                    onFinish={(values) => testMutation.mutate(values)}
                  >
                    <Form.Item
                      name="url"
                      rules={[{ required: true, message: "请输入测试 URL" }]}
                      style={{ flex: 1, minWidth: 320 }}
                    >
                      <Input placeholder="https://example.com" />
                    </Form.Item>
                    <Form.Item>
                      <PermissionButton
                        permission="system:http_client:test"
                        loading={testMutation.isPending}
                        onClick={() => testForm.submit()}
                      >
                        测试请求
                      </PermissionButton>
                    </Form.Item>
                  </Form>
                  {testMutation.data ? (
                    <Alert
                      className="page-notice"
                      type={testMutation.data.ok ? "success" : "warning"}
                      showIcon
                      message={
                        testMutation.data.status_code
                          ? `HTTP ${testMutation.data.status_code} · ${testMutation.data.duration_ms} ms`
                          : `${testMutation.data.duration_ms} ms`
                      }
                      description={testMutation.data.message}
                    />
                  ) : null}
                </Card>
              </Space>
            ),
          },
          {
            key: "requests",
            label: "请求管理",
            children: (
              <Space direction="vertical" size="middle" className="full-width">
                <Card title="导入请求" className="admin-card">
                  <Form
                    form={importForm}
                    layout="vertical"
                    initialValues={{ format: "curl" }}
                    onFinish={(values) => importMutation.mutate(values)}
                  >
                    <Space wrap className="full-width">
                      <Form.Item name="format" label="格式">
                        <Select
                          style={{ width: 160 }}
                          options={[
                            { value: "curl", label: "curl" },
                            { value: "postman", label: "Postman" },
                            { value: "request", label: "request" },
                          ]}
                        />
                      </Form.Item>
                      <Form.Item label="操作">
                        <PermissionButton
                          permission="system:http_client:request:create"
                          loading={importMutation.isPending}
                          onClick={() => importForm.submit()}
                        >
                          解析到编辑器
                        </PermissionButton>
                      </Form.Item>
                    </Space>
                    <Form.Item
                      name="content"
                      rules={[{ required: true, message: "请输入导入内容" }]}
                    >
                      <Input.TextArea
                        rows={4}
                        placeholder="curl -X POST https://api.example.com -H 'Content-Type: application/json' -d '{...}'"
                      />
                    </Form.Item>
                  </Form>
                </Card>
                <Card title="已保存请求" className="admin-card">
                  <DataTable<HttpRequestPresetRecord>
                    loading={presetsQuery.isLoading}
                    columns={requestColumns}
                    dataSource={presetsQuery.data?.items ?? []}
                    pagination={false}
                  />
                </Card>
              </Space>
            ),
          },
          {
            key: "runs",
            label: "执行记录",
            children: (
              <Card title="请求执行记录" className="admin-card">
                <DataTable<HttpRequestRunRecord>
                  loading={runsQuery.isLoading}
                  columns={runColumns}
                  dataSource={runsQuery.data?.items ?? []}
                  pagination={false}
                />
              </Card>
            ),
          },
        ]}
      />

      <Modal
        title={editing ? "编辑请求" : "新增请求"}
        open={requestOpen}
        onCancel={() => setRequestOpen(false)}
        footer={null}
        width={900}
      >
        {requestEditor}
      </Modal>

      <Drawer
        title="请求结果详情"
        open={Boolean(detailRunId)}
        onClose={() => setDetailRunId(null)}
        width={720}
      >
        {detailRun ? (
          <Space direction="vertical" size="middle" className="full-width">
            <Descriptions column={1} size="small" bordered>
              <Descriptions.Item label="请求">
                {detailRun.method} {detailRun.url}
              </Descriptions.Item>
              <Descriptions.Item label="状态">
                <Tag color={statusColor[detailRun.status] ?? "default"}>
                  {detailRun.status}
                </Tag>
                {detailRun.response_status_code
                  ? ` HTTP ${detailRun.response_status_code}`
                  : null}
              </Descriptions.Item>
              <Descriptions.Item label="耗时">
                {detailRun.duration_ms} ms
              </Descriptions.Item>
              <Descriptions.Item label="响应大小">
                {detailRun.response_size_bytes} B
              </Descriptions.Item>
              <Descriptions.Item label="错误">
                {detailRun.error_message ?? "-"}
              </Descriptions.Item>
            </Descriptions>
            <Typography.Text strong>响应头</Typography.Text>
            <pre className="admin-code-block">
              {detailRun.response_headers_json ?? "{}"}
            </pre>
            <Typography.Text strong>响应预览</Typography.Text>
            <pre className="admin-code-block">
              {detailRun.response_preview_text ?? "无响应内容"}
            </pre>
            {detailRun.has_response_body ? (
              <PermissionButton
                icon={<DownloadOutlined />}
                permission="system:http_client:run:download"
                loading={downloadMutation.isPending}
                onClick={() => downloadMutation.mutate(detailRun)}
              >
                下载结果
              </PermissionButton>
            ) : null}
          </Space>
        ) : null}
      </Drawer>
    </CrudPage>
  );
}
