import {
  ApiOutlined,
  CheckCircleOutlined,
  ReloadOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  Button,
  Card,
  Descriptions,
  Form,
  Input,
  InputNumber,
  Space,
  Switch,
  Tag,
  Typography,
  message,
} from "antd";
import { useEffect } from "react";
import {
  type HttpClientRuntimeConfig,
  fetchHttpClientConfig,
  testHttpClientRequest,
  updateHttpClientConfig,
} from "../../../api/admin/httpClient";
import { CrudPage } from "../../../components/admin/CrudPage";
import { PermissionButton } from "../../../components/admin/PermissionButton";

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

type TestFormValues = {
  url: string;
};

export function HttpClientPage() {
  const [configForm] = Form.useForm<HttpClientRuntimeConfig>();
  const [testForm] = Form.useForm<TestFormValues>();
  const queryClient = useQueryClient();
  const proxyEnabled = Form.useWatch("proxy_enabled", configForm) ?? false;

  const configQuery = useQuery({
    queryKey: ["admin-http-client-config"],
    queryFn: fetchHttpClientConfig,
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

  const saveMutation = useMutation({
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
  });

  const testMutation = useMutation({
    mutationFn: testHttpClientRequest,
  });

  return (
    <CrudPage
      title="HTTP 客户端配置"
      subtitle="统一管理项目对外 reqwest 请求的超时、代理与证书参数"
      breadcrumb={["系统管理", "HTTP 客户端配置"]}
      icon={<ApiOutlined />}
      notice="AI 图片生成、备份推送等后端外部 HTTP 请求会使用这里的配置。未单独覆盖时，会按这里的超时、代理与证书策略发起请求。"
      toolbar={
        <Space wrap>
          <Button
            icon={<ReloadOutlined />}
            onClick={() => configQuery.refetch()}
          >
            刷新
          </Button>
          <PermissionButton
            type="primary"
            icon={<CheckCircleOutlined />}
            permission="system:http_client:config"
            loading={saveMutation.isPending}
            onClick={() => configForm.submit()}
          >
            保存配置
          </PermissionButton>
        </Space>
      }
    >
      <Space direction="vertical" size="middle" className="full-width">
        <Card title="运行时参数" className="admin-card">
          <Form
            form={configForm}
            layout="vertical"
            initialValues={defaultConfig}
            onFinish={(values) =>
              saveMutation.mutate({
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
                <Switch checkedChildren="启用" unCheckedChildren="默认" />
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
                <Switch checkedChildren="开启" unCheckedChildren="关闭" />
              </Form.Item>
            </Space>
            <div className="http-client-grid">
              <Form.Item
                name="request_timeout_seconds"
                label="整体请求超时（秒）"
                rules={[{ required: true, message: "请输入整体请求超时" }]}
              >
                <InputNumber min={1} max={3600} style={{ width: "100%" }} />
              </Form.Item>
              <Form.Item
                name="connect_timeout_seconds"
                label="连接超时（秒）"
                rules={[{ required: true, message: "请输入连接超时" }]}
              >
                <InputNumber min={1} max={3600} style={{ width: "100%" }} />
              </Form.Item>
              <Form.Item
                name="pool_idle_timeout_seconds"
                label="连接池空闲超时（秒）"
                rules={[{ required: true, message: "请输入连接池空闲超时" }]}
              >
                <InputNumber min={1} max={3600} style={{ width: "100%" }} />
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
                  ? [{ required: true, message: "启用代理后必须填写代理地址" }]
                  : []
              }
            >
              <Input placeholder="http://127.0.0.1:7890 或 socks5://127.0.0.1:1080" />
            </Form.Item>
          </Form>
        </Card>

        <Card title="说明" className="admin-card">
          <Descriptions column={1} size="small">
            <Descriptions.Item label="当前行为">
              AI
              生图原先未单独设置整体请求超时；保存这里的配置后，新发起的外部请求会按这里的参数构建
              reqwest client。
            </Descriptions.Item>
            <Descriptions.Item label="推荐值">
              <Space wrap>
                <Tag color="blue">整体超时 60~120 秒</Tag>
                <Tag color="blue">连接超时 10~20 秒</Tag>
                <Tag color="blue">空闲超时 60~90 秒</Tag>
              </Space>
            </Descriptions.Item>
            <Descriptions.Item label="证书策略">
              <Typography.Text type="warning">
                “接受无效证书”只建议用于内网或测试环境。
              </Typography.Text>
            </Descriptions.Item>
          </Descriptions>
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
    </CrudPage>
  );
}
