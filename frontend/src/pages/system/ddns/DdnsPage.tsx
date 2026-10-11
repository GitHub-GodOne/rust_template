import {
  CheckCircleFilled,
  CloseCircleFilled,
  CloudSyncOutlined,
  ReloadOutlined,
  SaveOutlined,
  SendOutlined,
  WifiOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  Button,
  Card,
  Col,
  Descriptions,
  Empty,
  Form,
  Input,
  InputNumber,
  Row,
  Select,
  Space,
  Switch,
  Tag,
  Typography,
  message,
} from "antd";
import { useEffect, useState } from "react";
import {
  type DdnsConfig,
  type DdnsRecordAction,
  type DdnsSyncTrigger,
  type SaveDdnsConfigParams,
  fetchDdnsConfig,
  fetchDdnsStatus,
  saveDdnsConfig,
  syncDdnsNow,
} from "../../../api/admin/ddns";
import { PageHeader } from "../../../components/admin/PageHeader";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { useAuthStore } from "../../../stores/auth";
import { isValidManualIpv6 } from "./ipv6";
import "./ddns.css";

type FormValues = Omit<DdnsConfig, "token_configured"> & { api_token?: string };

const sourceOptions = [
  { value: "auto", label: "自动（优先读取网卡，失败再外部查询）" },
  { value: "interface", label: "仅读取本机网卡" },
  { value: "url", label: "仅通过外部地址查询" },
  { value: "manual", label: "手动指定固定地址" },
];

const sourceLabels: Record<string, string> = {
  auto: "自动",
  interface: "本机网卡",
  url: "外部查询",
  manual: "手动指定",
};

const triggerLabels: Record<DdnsSyncTrigger, string> = {
  schedule: "定时检测",
  manual: "手动同步",
  manual_address: "手动指定地址",
};

const actionTags: Record<DdnsRecordAction, { color: string; label: string }> = {
  created: { color: "green", label: "已创建" },
  updated: { color: "blue", label: "已更新" },
  unchanged: { color: "default", label: "无变化" },
  failed: { color: "red", label: "失败" },
};

const ipv6Rule = {
  validator: (_: unknown, value?: string) =>
    !value?.trim() || isValidManualIpv6(value)
      ? Promise.resolve()
      : Promise.reject(new Error("请输入可用于解析的 IPv6 地址")),
};

function formatTime(value?: string | null) {
  return value ? new Date(value).toLocaleString() : "-";
}

export function DdnsPage() {
  const [form] = Form.useForm<FormValues>();
  const [manualAddress, setManualAddress] = useState("");
  const queryClient = useQueryClient();
  const canEdit = useAuthStore((state) =>
    state.hasPermission("system:ddns:update"),
  );
  const source = Form.useWatch("ipv6_source", form);

  const statusQuery = useQuery({
    queryKey: ["admin-ddns-status"],
    queryFn: fetchDdnsStatus,
    refetchInterval: 30_000,
  });
  const configQuery = useQuery({
    queryKey: ["admin-ddns-config"],
    queryFn: fetchDdnsConfig,
  });

  useEffect(() => {
    if (configQuery.data) {
      form.setFieldsValue({ ...configQuery.data, api_token: "" });
    }
  }, [form, configQuery.data]);

  const saveMutation = useMutation({
    mutationFn: (values: FormValues) => {
      // Include fields hidden by the current address source as well.
      const all = { ...form.getFieldsValue(true), ...values } as FormValues;
      const payload: SaveDdnsConfigParams = {
        ...all,
        api_token: all.api_token?.trim() || undefined,
      };
      return saveDdnsConfig(payload);
    },
    onSuccess: (config) => {
      message.success("配置已保存，下一次检测即生效");
      queryClient.setQueryData(["admin-ddns-config"], config);
      form.setFieldValue("api_token", "");
      queryClient.invalidateQueries({ queryKey: ["admin-ddns-status"] });
      queryClient.invalidateQueries({ queryKey: ["admin-settings"] });
    },
    onError: (error) => message.error(error.message || "保存失败"),
  });

  const syncMutation = useMutation({
    mutationFn: (ipv6?: string) => syncDdnsNow(ipv6),
    onSuccess: (status, ipv6) => {
      queryClient.setQueryData(["admin-ddns-status"], status);
      if (status.last_error) {
        message.warning("同步完成，但有域名更新失败");
        return;
      }
      message.success(ipv6 ? `已将 ${ipv6} 同步到域名` : "已同步到 Cloudflare");
      if (ipv6) setManualAddress("");
    },
    onError: (error) => message.error(error.message || "同步失败"),
  });

  const status = statusQuery.data;
  const ready = Boolean(status?.enabled && status.configured);
  const manualValid = isValidManualIpv6(manualAddress);

  return (
    <div className="ddns-page">
      <PageHeader
        title="IPv6 动态解析"
        subtitle="自动把本机的公网 IPv6 更新到 Cloudflare AAAA 记录"
        breadcrumb={["系统管理", "IPv6 动态解析"]}
        icon={<WifiOutlined />}
      />
      <Row gutter={[16, 16]}>
        <Col xs={24} xl={10}>
          <Space direction="vertical" size={16} className="full-width">
            <Card
              className="admin-card"
              title="运行状态"
              extra={
                <Space size={8}>
                  <Button
                    icon={<ReloadOutlined spin={statusQuery.isFetching} />}
                    onClick={() => statusQuery.refetch()}
                    aria-label="刷新状态"
                  />
                  <PermissionButton
                    type="primary"
                    icon={<CloudSyncOutlined />}
                    permission="system:ddns:update"
                    loading={syncMutation.isPending && !syncMutation.variables}
                    disabled={!ready}
                    onClick={() => syncMutation.mutate(undefined)}
                  >
                    立即同步
                  </PermissionButton>
                </Space>
              }
            >
              {status ? (
                <Space
                  direction="vertical"
                  size="middle"
                  className="full-width"
                >
                  {!status.enabled && (
                    <Alert
                      type="info"
                      showIcon
                      message="IPv6 动态解析未启用"
                      description="填写 Cloudflare Token 和域名后打开「启用」并保存，系统会按检测间隔自动更新解析。"
                    />
                  )}
                  {status.enabled && !status.configured && (
                    <Alert
                      type="warning"
                      showIcon
                      message="配置不完整：需要 API Token、至少一个域名；手动指定模式还需要填写地址"
                    />
                  )}
                  {status.last_error && (
                    <Alert
                      type="error"
                      showIcon
                      message="最近一次同步失败"
                      description={status.last_error}
                    />
                  )}
                  <Descriptions
                    column={1}
                    size="small"
                    bordered
                    className="ddns-status"
                  >
                    <Descriptions.Item label="状态">
                      {status.enabled ? (
                        <Tag color="green">已启用</Tag>
                      ) : (
                        <Tag>未启用</Tag>
                      )}
                    </Descriptions.Item>
                    <Descriptions.Item label="检测到的 IPv6">
                      <Typography.Text copyable={Boolean(status.current_ipv6)}>
                        {status.current_ipv6 ?? "尚未检测"}
                      </Typography.Text>
                    </Descriptions.Item>
                    <Descriptions.Item label="已同步 IPv6">
                      {status.last_synced_ipv6 ? (
                        <Space size={6} wrap>
                          <Typography.Text copyable>
                            {status.last_synced_ipv6}
                          </Typography.Text>
                          {status.last_sync_trigger && (
                            <Tag>{triggerLabels[status.last_sync_trigger]}</Tag>
                          )}
                        </Space>
                      ) : (
                        "-"
                      )}
                    </Descriptions.Item>
                    <Descriptions.Item label="获取方式">
                      {sourceLabels[status.source] ?? status.source}
                      {status.interface_name
                        ? `（${status.interface_name}）`
                        : ""}
                    </Descriptions.Item>
                    <Descriptions.Item label="检测间隔">
                      {status.interval_seconds} 秒
                    </Descriptions.Item>
                    <Descriptions.Item label="上次检测">
                      {formatTime(status.last_checked_at)}
                    </Descriptions.Item>
                    <Descriptions.Item label="上次同步">
                      {formatTime(status.last_synced_at)}
                    </Descriptions.Item>
                  </Descriptions>
                  <div>
                    <Typography.Text strong>域名记录</Typography.Text>
                    {status.record_names.length === 0 ? (
                      <Empty
                        image={Empty.PRESENTED_IMAGE_SIMPLE}
                        description="尚未配置域名"
                      />
                    ) : (
                      <ul className="ddns-record-list">
                        {status.record_names.map((name) => {
                          const record = status.records.find(
                            (item) => item.name === name,
                          );
                          const tag = record ? actionTags[record.action] : null;
                          return (
                            <li key={name}>
                              <span
                                className="ddns-record-icon"
                                aria-hidden="true"
                              >
                                {record?.action === "failed" ? (
                                  <CloseCircleFilled className="ddns-error" />
                                ) : (
                                  <CheckCircleFilled
                                    className={record ? "ddns-ok" : "ddns-idle"}
                                  />
                                )}
                              </span>
                              <span className="ddns-record-main">
                                <Typography.Text strong>{name}</Typography.Text>
                                <Typography.Text
                                  type={
                                    record?.action === "failed"
                                      ? "danger"
                                      : "secondary"
                                  }
                                  className="ddns-record-detail"
                                >
                                  {record?.message ??
                                    (record?.content
                                      ? `AAAA → ${record.content}`
                                      : "等待首次同步")}
                                </Typography.Text>
                              </span>
                              {tag && <Tag color={tag.color}>{tag.label}</Tag>}
                            </li>
                          );
                        })}
                      </ul>
                    )}
                  </div>
                </Space>
              ) : (
                <Empty
                  description={statusQuery.isLoading ? "加载中" : "暂无状态"}
                />
              )}
            </Card>
            {canEdit && (
              <Card className="admin-card" title="手动同步指定地址">
                <Space direction="vertical" size="small" className="full-width">
                  <Space.Compact className="full-width">
                    <Input
                      allowClear
                      value={manualAddress}
                      status={manualAddress && !manualValid ? "error" : ""}
                      placeholder="例如 2409:8a1e:1234::1"
                      onChange={(event) => setManualAddress(event.target.value)}
                      onPressEnter={() =>
                        manualValid &&
                        ready &&
                        syncMutation.mutate(manualAddress.trim())
                      }
                    />
                    <Button
                      type="primary"
                      icon={<SendOutlined />}
                      disabled={!manualValid || !status?.enabled}
                      loading={
                        syncMutation.isPending &&
                        Boolean(syncMutation.variables)
                      }
                      onClick={() => syncMutation.mutate(manualAddress.trim())}
                    >
                      同步此地址
                    </Button>
                  </Space.Compact>
                  <Typography.Text type="secondary" className="ddns-hint">
                    {status?.source === "manual"
                      ? "这是一次性同步；要长期使用某个地址，请修改右侧「手动指定 IPv6」。"
                      : "这是一次性同步，下一次自动检测到不同地址时会被覆盖；要长期固定，请把获取方式改为「手动指定固定地址」。"}
                  </Typography.Text>
                </Space>
              </Card>
            )}
          </Space>
        </Col>
        <Col xs={24} xl={14}>
          <Card className="admin-card" title="解析配置">
            <Form<FormValues>
              form={form}
              layout="vertical"
              disabled={!canEdit}
              onFinish={(values) => saveMutation.mutate(values)}
            >
              <Row gutter={16}>
                <Col xs={12} md={8}>
                  <Form.Item
                    name="enabled"
                    label="启用"
                    valuePropName="checked"
                  >
                    <Switch />
                  </Form.Item>
                </Col>
                <Col xs={12} md={8}>
                  <Form.Item
                    name="proxied"
                    label="Cloudflare 代理"
                    valuePropName="checked"
                    tooltip="直连 IPv6 访问时请保持关闭"
                  >
                    <Switch />
                  </Form.Item>
                </Col>
                <Col xs={24} md={8}>
                  <Form.Item
                    name="ttl"
                    label="TTL（秒）"
                    tooltip="1 表示自动，也可填 60 到 86400"
                    rules={[
                      {
                        validator: (_, value: number) =>
                          value === 1 || (value >= 60 && value <= 86_400)
                            ? Promise.resolve()
                            : Promise.reject(
                                new Error("填写 1 或 60 到 86400 之间的整数"),
                              ),
                      },
                    ]}
                  >
                    <InputNumber precision={0} className="full-width" />
                  </Form.Item>
                </Col>
              </Row>
              <Form.Item
                name="api_token"
                label="Cloudflare API Token"
                extra="需要目标域名的 Zone.DNS 编辑权限；保存后不会再显示，留空表示不修改。"
              >
                <Input.Password
                  autoComplete="new-password"
                  placeholder={
                    configQuery.data?.token_configured
                      ? "已配置（留空保持不变）"
                      : "未配置"
                  }
                />
              </Form.Item>
              <Form.Item
                name="record_names"
                label="解析域名"
                extra="完整域名，多个用逗号分隔，例如 home.example.com,nas.example.com"
              >
                <Input placeholder="home.example.com" />
              </Form.Item>
              <Form.Item
                name="zone_id"
                label="Zone ID"
                extra="留空时根据域名自动查找"
              >
                <Input placeholder="自动查找" />
              </Form.Item>
              <Row gutter={16}>
                <Col xs={24} md={12}>
                  <Form.Item name="ipv6_source" label="IPv6 获取方式">
                    <Select options={sourceOptions} />
                  </Form.Item>
                </Col>
                <Col xs={24} md={12}>
                  <Form.Item
                    name="check_interval_seconds"
                    label="检测间隔（秒）"
                    rules={[
                      {
                        type: "number",
                        min: 30,
                        max: 86_400,
                        message: "30 到 86400 秒",
                      },
                    ]}
                  >
                    <InputNumber precision={0} className="full-width" />
                  </Form.Item>
                </Col>
              </Row>
              {source === "manual" ? (
                <Form.Item
                  name="manual_ipv6"
                  label="手动指定 IPv6"
                  extra="每次检测都会把这个地址同步到域名；也支持 fd00::/8 内网地址"
                  rules={[
                    { required: true, message: "请填写 IPv6 地址" },
                    ipv6Rule,
                  ]}
                >
                  <Input placeholder="2409:8a1e:1234::1" />
                </Form.Item>
              ) : (
                <Row gutter={16}>
                  <Col xs={24} md={12}>
                    <Form.Item
                      name="interface_name"
                      label="指定网卡"
                      extra="如 eth0、en0、以太网；留空自动选择稳定的公网地址"
                    >
                      <Input placeholder="自动选择" />
                    </Form.Item>
                  </Col>
                  <Col xs={24} md={12}>
                    <Form.Item
                      name="ipv6_lookup_url"
                      label="IPv6 查询地址"
                      extra="返回纯文本 IPv6 的地址"
                    >
                      <Input placeholder="https://api6.ipify.org" />
                    </Form.Item>
                  </Col>
                </Row>
              )}
              <Form.Item
                name="api_base"
                label="Cloudflare API 地址"
                extra="一般无需修改"
              >
                <Input placeholder="https://api.cloudflare.com/client/v4" />
              </Form.Item>
              <PermissionButton
                type="primary"
                htmlType="submit"
                icon={<SaveOutlined />}
                permission="system:ddns:update"
                loading={saveMutation.isPending}
              >
                保存配置
              </PermissionButton>
            </Form>
          </Card>
        </Col>
      </Row>
    </div>
  );
}
