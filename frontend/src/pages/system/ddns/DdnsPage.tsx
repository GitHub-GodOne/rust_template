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
import { t } from "../../../i18n";

type FormValues = Omit<DdnsConfig, "token_configured"> & { api_token?: string };

const sourceOptions = [
  { value: "auto", label: t("自动（优先读取网卡，失败再外部查询）") },
  { value: "interface", label: t("仅读取本机网卡") },
  { value: "url", label: t("仅通过外部地址查询") },
  { value: "manual", label: t("手动指定固定地址") },
];

const sourceLabels: Record<string, string> = {
  auto: t("自动"),
  interface: t("本机网卡"),
  url: t("外部查询"),
  manual: t("手动指定"),
};

const triggerLabels: Record<DdnsSyncTrigger, string> = {
  schedule: t("定时检测"),
  manual: t("手动同步"),
  manual_address: t("手动指定地址"),
};

const actionTags: Record<DdnsRecordAction, { color: string; label: string }> = {
  created: { color: "green", label: t("已创建") },
  updated: { color: "blue", label: t("已更新") },
  unchanged: { color: "default", label: t("无变化") },
  failed: { color: "red", label: t("失败") },
};

const ipv6Rule = {
  validator: (_: unknown, value?: string) =>
    !value?.trim() || isValidManualIpv6(value)
      ? Promise.resolve()
      : Promise.reject(new Error(t("请输入可用于解析的 IPv6 地址"))),
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
  const authMode = Form.useWatch("auth_mode", form);

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
      message.success(t("配置已保存，下一次检测即生效"));
      queryClient.setQueryData(["admin-ddns-config"], config);
      form.setFieldValue("api_token", "");
      queryClient.invalidateQueries({ queryKey: ["admin-ddns-status"] });
      queryClient.invalidateQueries({ queryKey: ["admin-settings"] });
    },
    onError: (error) => message.error(error.message || t("保存失败")),
  });

  const syncMutation = useMutation({
    mutationFn: (ipv6?: string) => syncDdnsNow(ipv6),
    onSuccess: (status, ipv6) => {
      queryClient.setQueryData(["admin-ddns-status"], status);
      if (status.last_error) {
        message.warning(t("同步完成，但有域名更新失败"));
        return;
      }
      message.success(
        ipv6 ? t("已将 {0} 同步到域名", ipv6) : t("已同步到 Cloudflare"),
      );
      if (ipv6) setManualAddress("");
    },
    onError: (error) => message.error(error.message || t("同步失败")),
  });

  const status = statusQuery.data;
  const ready = Boolean(status?.enabled && status.configured);
  const manualValid = isValidManualIpv6(manualAddress);

  return (
    <div className="ddns-page">
      <PageHeader
        title={t("IPv6 动态解析")}
        subtitle={t("自动把本机的公网 IPv6 更新到 Cloudflare AAAA 记录")}
        breadcrumb={[t("系统管理"), t("IPv6 动态解析")]}
        icon={<WifiOutlined />}
      />
      <Row gutter={[16, 16]}>
        <Col xs={24} xl={10}>
          <Space direction="vertical" size={16} className="full-width">
            <Card
              className="admin-card"
              title={t("运行状态")}
              extra={
                <Space size={8}>
                  <Button
                    icon={<ReloadOutlined spin={statusQuery.isFetching} />}
                    onClick={() => statusQuery.refetch()}
                    aria-label={t("刷新状态")}
                  />
                  <PermissionButton
                    type="primary"
                    icon={<CloudSyncOutlined />}
                    permission="system:ddns:update"
                    loading={syncMutation.isPending && !syncMutation.variables}
                    disabled={!ready}
                    onClick={() => syncMutation.mutate(undefined)}
                  >
                    {t("立即同步")}
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
                      message={t("IPv6 动态解析未启用")}
                      description={t(
                        "配置 Cloudflare 认证和域名后打开「启用」并保存，系统会按检测间隔自动更新解析。",
                      )}
                    />
                  )}
                  {status.enabled && !status.configured && (
                    <Alert
                      type="warning"
                      showIcon
                      message={t(
                        "配置不完整：需要 API Token、至少一个域名；手动指定模式还需要填写地址",
                      )}
                    />
                  )}
                  {status.last_error && (
                    <Alert
                      type="error"
                      showIcon
                      message={t("最近一次同步失败")}
                      description={status.last_error}
                    />
                  )}
                  <Descriptions
                    column={1}
                    size="small"
                    bordered
                    className="ddns-status"
                  >
                    <Descriptions.Item label={t("状态")}>
                      {status.enabled ? (
                        <Tag color="green">{t("已启用")}</Tag>
                      ) : (
                        <Tag>{t("未启用")}</Tag>
                      )}
                    </Descriptions.Item>
                    <Descriptions.Item label={t("检测到的 IPv6")}>
                      <Typography.Text copyable={Boolean(status.current_ipv6)}>
                        {status.current_ipv6 ?? t("尚未检测")}
                      </Typography.Text>
                    </Descriptions.Item>
                    <Descriptions.Item label={t("已同步 IPv6")}>
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
                    <Descriptions.Item label={t("获取方式")}>
                      {sourceLabels[status.source] ?? status.source}
                      {status.interface_name
                        ? `（${status.interface_name}）`
                        : ""}
                    </Descriptions.Item>
                    <Descriptions.Item label={t("检测间隔")}>
                      {t("{0} 秒", status.interval_seconds)}
                    </Descriptions.Item>
                    <Descriptions.Item label={t("上次检测")}>
                      {formatTime(status.last_checked_at)}
                    </Descriptions.Item>
                    <Descriptions.Item label={t("上次同步")}>
                      {formatTime(status.last_synced_at)}
                    </Descriptions.Item>
                  </Descriptions>
                  <div>
                    <Typography.Text strong>{t("域名记录")}</Typography.Text>
                    {status.record_names.length === 0 ? (
                      <Empty
                        image={Empty.PRESENTED_IMAGE_SIMPLE}
                        description={t("尚未配置域名")}
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
                                      : t("等待首次同步"))}
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
                  description={
                    statusQuery.isLoading ? t("加载中") : t("暂无状态")
                  }
                />
              )}
            </Card>
            {canEdit && (
              <Card className="admin-card" title={t("手动同步指定地址")}>
                <Space direction="vertical" size="small" className="full-width">
                  <Space.Compact className="full-width">
                    <Input
                      allowClear
                      value={manualAddress}
                      status={manualAddress && !manualValid ? "error" : ""}
                      placeholder={t("例如 2409:8a1e:1234::1")}
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
                      {t("同步此地址")}
                    </Button>
                  </Space.Compact>
                  <Typography.Text type="secondary" className="ddns-hint">
                    {status?.source === "manual"
                      ? t(
                          "这是一次性同步；要长期使用某个地址，请修改右侧「手动指定 IPv6」。",
                        )
                      : t(
                          "这是一次性同步，下一次自动检测到不同地址时会被覆盖；要长期固定，请把获取方式改为「手动指定固定地址」。",
                        )}
                  </Typography.Text>
                </Space>
              </Card>
            )}
          </Space>
        </Col>
        <Col xs={24} xl={14}>
          <Card className="admin-card" title={t("解析配置")}>
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
                    label={t("启用")}
                    valuePropName="checked"
                  >
                    <Switch />
                  </Form.Item>
                </Col>
                <Col xs={12} md={8}>
                  <Form.Item
                    name="proxied"
                    label={t("Cloudflare 代理")}
                    valuePropName="checked"
                    tooltip={t("直连 IPv6 访问时请保持关闭")}
                  >
                    <Switch />
                  </Form.Item>
                </Col>
                <Col xs={24} md={8}>
                  <Form.Item
                    name="ttl"
                    label={t("TTL（秒）")}
                    tooltip={t("1 表示自动，也可填 60 到 86400")}
                    rules={[
                      {
                        validator: (_, value: number) =>
                          value === 1 || (value >= 60 && value <= 86_400)
                            ? Promise.resolve()
                            : Promise.reject(
                                new Error(
                                  t("填写 1 或 60 到 86400 之间的整数"),
                                ),
                              ),
                      },
                    ]}
                  >
                    <InputNumber precision={0} className="full-width" />
                  </Form.Item>
                </Col>
              </Row>
              <Form.Item
                name="auth_mode"
                label={t("Cloudflare 认证方式")}
                initialValue="api_token"
              >
                <Select
                  options={[
                    { value: "api_token", label: t("API Token（推荐）") },
                    {
                      value: "global_api_key",
                      label: t("Global API Key + 账号邮箱"),
                    },
                  ]}
                />
              </Form.Item>
              {authMode === "global_api_key" && (
                <Form.Item
                  name="api_email"
                  label={t("Cloudflare 账号邮箱")}
                  extra={t("必须是当前 Global API Key 所属账号的邮箱。")}
                  rules={[
                    {
                      required: true,
                      message: t("请填写 Cloudflare 账号邮箱"),
                    },
                    { type: "email", message: t("请输入有效邮箱") },
                  ]}
                >
                  <Input autoComplete="email" placeholder="name@example.com" />
                </Form.Item>
              )}
              <Form.Item
                name="api_token"
                label={
                  authMode === "global_api_key"
                    ? "Cloudflare Global API Key"
                    : "Cloudflare API Token"
                }
                extra={
                  authMode === "global_api_key"
                    ? t(
                        "填写 Global API Key，不能使用 Origin CA Key。保存后不回显，留空保留已存密钥。",
                      )
                    : t(
                        "填写 API Token（不能使用 Global API Key）。需要目标域名的 Zone.DNS 编辑权限；自动查找 Zone 还需 Zone.Zone 读取权限。留空保留已存 Token。",
                      )
                }
              >
                <Input.Password
                  autoComplete="new-password"
                  placeholder={
                    configQuery.data?.token_configured
                      ? t("已配置（留空保持不变）")
                      : t("未配置")
                  }
                />
              </Form.Item>
              <Form.Item
                name="record_names"
                label={t("解析域名")}
                extra={t(
                  "完整域名，多个用逗号分隔，例如 home.example.com,nas.example.com",
                )}
              >
                <Input placeholder="home.example.com" />
              </Form.Item>
              <Form.Item
                name="zone_id"
                label="Zone ID"
                extra={t("留空时根据域名自动查找")}
              >
                <Input placeholder={t("自动查找")} />
              </Form.Item>
              <Row gutter={16}>
                <Col xs={24} md={12}>
                  <Form.Item name="ipv6_source" label={t("IPv6 获取方式")}>
                    <Select options={sourceOptions} />
                  </Form.Item>
                </Col>
                <Col xs={24} md={12}>
                  <Form.Item
                    name="check_interval_seconds"
                    label={t("检测间隔（秒）")}
                    rules={[
                      {
                        type: "number",
                        min: 30,
                        max: 86_400,
                        message: t("30 到 86400 秒"),
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
                  label={t("手动指定 IPv6")}
                  extra={t(
                    "每次检测都会把这个地址同步到域名；也支持 fd00::/8 内网地址",
                  )}
                  rules={[
                    { required: true, message: t("请填写 IPv6 地址") },
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
                      label={t("指定网卡")}
                      extra={t(
                        "如 eth0、en0、以太网；留空自动选择稳定的公网地址",
                      )}
                    >
                      <Input placeholder={t("自动选择")} />
                    </Form.Item>
                  </Col>
                  <Col xs={24} md={12}>
                    <Form.Item
                      name="ipv6_lookup_url"
                      label={t("IPv6 查询地址")}
                      extra={t("返回纯文本 IPv6 的地址")}
                    >
                      <Input placeholder="https://api6.ipify.org" />
                    </Form.Item>
                  </Col>
                </Row>
              )}
              <Form.Item
                name="api_base"
                label={t("Cloudflare API 地址")}
                extra={t("一般无需修改")}
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
                {t("保存配置")}
              </PermissionButton>
            </Form>
          </Card>
        </Col>
      </Row>
    </div>
  );
}
