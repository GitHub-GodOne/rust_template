import {
  ApiOutlined,
  CloudOutlined,
  DeleteOutlined,
  EditOutlined,
  ExportOutlined,
  GlobalOutlined,
  PlusOutlined,
  ReloadOutlined,
  SafetyCertificateOutlined,
  SaveOutlined,
  SyncOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  Button,
  Card,
  Col,
  Empty,
  Form,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Radio,
  Row,
  Select,
  Space,
  Switch,
  Tabs,
  Tag,
  Tooltip,
  Typography,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useEffect, useMemo, useState } from "react";
import {
  type CertificateRecord,
  type CertificateSettings,
  type DomainMappingRecord,
  type IpMode,
  type SaveCertificateSettingsParams,
  type SaveDomainMappingParams,
  createCertificate,
  createDomainMapping,
  deleteCertificate,
  deleteDomainMapping,
  fetchCertificateRuntime,
  fetchCertificateSettings,
  fetchCertificates,
  fetchDomainMappings,
  issueCertificate,
  restartTunnel,
  saveCertificateSettings,
  syncDomainMapping,
  updateCertificate,
  updateDomainMapping,
} from "../../../api/admin/certificates";
import { DataTable } from "../../../components/admin/DataTable";
import { PageHeader } from "../../../components/admin/PageHeader";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { useAuthStore } from "../../../stores/auth";
import {
  certificateTone,
  expiryTone,
  isPrivilegedPort,
  mappingTone,
  parseDomainList,
  previewUrl,
  upstreamLabel,
} from "./certificateView";
import "./certificates.css";
import { t } from "../../../i18n";

type MappingFormValues = Required<
  Pick<SaveDomainMappingParams, "domain" | "mode" | "upstream_port">
> &
  Omit<SaveDomainMappingParams, "domain" | "mode" | "upstream_port"> & {
    allowed_ips_text: string;
  };

type CertificateFormValues = {
  domain: string;
  alt_names: string[];
  auto_renew: boolean;
  issue_now: boolean;
};

type SettingsFormValues = SaveCertificateSettingsParams;

const mappingStatusLabels: Record<string, string> = {
  active: t("运行中"),
  pending: t("等待中"),
  error: t("异常"),
  disabled: t("已停用"),
};

const certificateStatusLabels: Record<string, string> = {
  pending: t("待申请"),
  issuing: t("签发中"),
  active: t("有效"),
  failed: t("失败"),
};

const ipModeOptions: { value: IpMode; label: string }[] = [
  { value: "off", label: t("不解析") },
  { value: "auto", label: t("自动检测本机公网地址") },
  { value: "manual", label: t("手动指定地址") },
];

const ipModeLabels: Record<string, string> = {
  off: t("不解析"),
  auto: t("自动"),
  manual: t("手动"),
};

const defaultMapping: MappingFormValues = {
  name: "",
  domain: "",
  mode: "direct",
  upstream_host: "127.0.0.1",
  upstream_port: 3000,
  public_port: 443,
  ipv4_mode: "auto",
  ipv4_address: "",
  ipv6_mode: "off",
  ipv6_address: "",
  allowed_ips_text: "",
  enabled: true,
  issue_certificate: true,
};

function errorText(error: unknown, fallback: string) {
  return error instanceof Error && error.message ? error.message : fallback;
}

function MappingDialog({
  open,
  editing,
  serverPort,
  onClose,
}: {
  open: boolean;
  editing: DomainMappingRecord | null;
  serverPort?: number;
  onClose: () => void;
}) {
  const [form] = Form.useForm<MappingFormValues>();
  const queryClient = useQueryClient();
  const mode = Form.useWatch("mode", form) ?? "direct";
  const domain = Form.useWatch("domain", form) ?? "";
  const publicPort = Form.useWatch("public_port", form);
  const upstreamHost = Form.useWatch("upstream_host", form);
  const upstreamPort = Form.useWatch("upstream_port", form);
  const ipv4Mode = Form.useWatch("ipv4_mode", form);
  const ipv6Mode = Form.useWatch("ipv6_mode", form);

  useEffect(() => {
    if (!open) {
      return;
    }
    form.resetFields();
    form.setFieldsValue(
      editing
        ? {
            ...editing,
            ipv4_address: editing.ipv4_address ?? "",
            ipv6_address: editing.ipv6_address ?? "",
            allowed_ips_text: editing.allowed_ips.join("\n"),
            issue_certificate: true,
          }
        : defaultMapping,
    );
  }, [editing, form, open]);

  const saveMutation = useMutation({
    mutationFn: (values: MappingFormValues) => {
      const { allowed_ips_text, ...rest } = values;
      const payload: SaveDomainMappingParams = {
        ...rest,
        allowed_ips: allowed_ips_text
          .split(/[\s,;]+/)
          .map((value) => value.trim())
          .filter(Boolean),
        ipv4_address: values.ipv4_address?.trim() || undefined,
        ipv6_address: values.ipv6_address?.trim() || undefined,
      };
      return editing
        ? updateDomainMapping(editing.id, payload)
        : createDomainMapping(payload);
    },
    onSuccess: (result) => {
      message.success(editing ? t("域名映射已更新") : t("域名映射已创建"));
      if (result.notice) {
        message.warning(result.notice, 6);
      }
      queryClient.invalidateQueries({ queryKey: ["admin-domain-mappings"] });
      queryClient.invalidateQueries({ queryKey: ["admin-certificates"] });
      onClose();
    },
    onError: (error) => message.error(errorText(error, t("保存域名映射失败"))),
  });

  return (
    <Modal
      title={editing ? t("编辑域名映射") : t("新增域名映射")}
      open={open}
      onCancel={onClose}
      onOk={() => form.submit()}
      okText={editing ? t("保存") : t("创建")}
      confirmLoading={saveMutation.isPending}
      width="min(680px, 96vw)"
      className="cert-modal"
      forceRender
      destroyOnHidden
    >
      <Form
        form={form}
        layout="vertical"
        requiredMark={false}
        initialValues={defaultMapping}
        onFinish={(values) => saveMutation.mutate(values)}
      >
        <Form.Item name="mode" label={t("映射方式")}>
          <Radio.Group className="cert-mode-group">
            <Radio.Button value="direct">
              <span className="cert-mode-title">
                <SafetyCertificateOutlined /> {t("内置 HTTPS 直连")}
              </span>
              <span className="cert-mode-description">
                {t(
                  "自动申请证书，监听 443 或自定义端口，解析 IPv4/IPv6 到本机",
                )}
              </span>
            </Radio.Button>
            <Radio.Button value="tunnel">
              <span className="cert-mode-title">
                <CloudOutlined /> Cloudflare Tunnel
              </span>
              <span className="cert-mode-description">
                {t("通过 cloudflared 穿透，无需公网 IP 和端口转发，固定 443")}
              </span>
            </Radio.Button>
          </Radio.Group>
        </Form.Item>
        <div className="cert-form-grid">
          <Form.Item
            name="domain"
            label={t("域名")}
            rules={[{ required: true, message: t("请输入域名") }]}
          >
            <Input placeholder="app.example.com" autoComplete="off" />
          </Form.Item>
          <Form.Item name="name" label={t("名称")}>
            <Input placeholder={t("默认使用域名")} />
          </Form.Item>
        </div>
        <div className="cert-form-grid">
          <Form.Item
            name="upstream_host"
            label={t("本地服务地址")}
            rules={[{ required: true, message: t("请输入本地服务地址") }]}
          >
            <Input placeholder="127.0.0.1" />
          </Form.Item>
          <Form.Item
            name="upstream_port"
            label={t("本地服务端口")}
            rules={[{ required: true, message: t("请输入本地服务端口") }]}
          >
            <InputNumber min={1} max={65535} className="full-width" />
          </Form.Item>
        </div>
        <Form.Item
          name="allowed_ips_text"
          label={t("允许访问 IP")}
          extra={t(
            "每行填写一个 IP 或 CIDR，留空表示所有 IP；内置 HTTPS 直连会在转发前校验客户端地址",
          )}
        >
          <Input.TextArea
            rows={3}
            placeholder="203.0.113.10\n192.168.1.0/24\n2001:db8::/32"
            autoComplete="off"
          />
        </Form.Item>
        {mode === "direct" ? (
          <>
            <Form.Item
              name="public_port"
              label={t("HTTPS 端口")}
              rules={[
                { required: true, message: t("请输入 HTTPS 端口") },
                {
                  validator: (_, value?: number) =>
                    value && value === serverPort
                      ? Promise.reject(
                          new Error(t("与后台服务端口 {0} 冲突", serverPort)),
                        )
                      : Promise.resolve(),
                },
              ]}
              extra={
                isPrivilegedPort(publicPort)
                  ? t(
                      "1024 以下端口需要以管理员身份运行（Linux 可授予 CAP_NET_BIND_SERVICE），否则请改用 8443 等端口",
                    )
                  : t("访问地址会带上端口号，同一端口可服务多个域名")
              }
            >
              <InputNumber min={1} max={65535} className="full-width" />
            </Form.Item>
            <div className="cert-form-grid">
              <Form.Item name="ipv4_mode" label={t("IPv4 解析（A 记录）")}>
                <Select options={ipModeOptions} />
              </Form.Item>
              <Form.Item name="ipv6_mode" label={t("IPv6 解析（AAAA 记录）")}>
                <Select options={ipModeOptions} />
              </Form.Item>
            </div>
            {ipv4Mode === "manual" || ipv6Mode === "manual" ? (
              <div className="cert-form-grid">
                {ipv4Mode === "manual" ? (
                  <Form.Item
                    name="ipv4_address"
                    label={t("IPv4 地址")}
                    rules={[{ required: true, message: t("请输入 IPv4 地址") }]}
                  >
                    <Input placeholder="203.0.113.10" />
                  </Form.Item>
                ) : (
                  <span />
                )}
                {ipv6Mode === "manual" ? (
                  <Form.Item
                    name="ipv6_address"
                    label={t("IPv6 地址")}
                    rules={[{ required: true, message: t("请输入 IPv6 地址") }]}
                  >
                    <Input placeholder="2001:db8::10" />
                  </Form.Item>
                ) : null}
              </div>
            ) : null}
            <div className="cert-switch-row">
              <span>
                <strong>{t("自动申请证书")}</strong>
                <Typography.Text type="secondary">
                  {t("没有覆盖该域名的证书时，保存后立即用 Let's Encrypt 申请")}
                </Typography.Text>
              </span>
              <Form.Item
                name="issue_certificate"
                valuePropName="checked"
                noStyle
              >
                <Switch aria-label={t("自动申请证书")} />
              </Form.Item>
            </div>
          </>
        ) : (
          <Alert
            type="info"
            showIcon
            className="cert-inline-alert"
            message={t(
              "Cloudflare 会在边缘提供 HTTPS 并通过隧道转发到本地服务，域名会被设置为指向隧道的代理 CNAME 记录。",
            )}
          />
        )}
        <div className="cert-switch-row">
          <span>
            <strong>{t("启用")}</strong>
            <Typography.Text type="secondary">
              {t("停用后不再对外提供访问，配置保留")}
            </Typography.Text>
          </span>
          <Form.Item name="enabled" valuePropName="checked" noStyle>
            <Switch aria-label={t("启用")} />
          </Form.Item>
        </div>
        <div className="cert-route-preview" aria-live="polite">
          <GlobalOutlined />
          <code>{previewUrl(domain, mode, publicPort)}</code>
          <span aria-hidden="true">→</span>
          <code>{upstreamLabel(upstreamHost, upstreamPort)}</code>
        </div>
      </Form>
    </Modal>
  );
}

function CertificateDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const [form] = Form.useForm<CertificateFormValues>();
  const queryClient = useQueryClient();
  useEffect(() => {
    if (open) {
      form.setFieldsValue({
        domain: "",
        alt_names: [],
        auto_renew: true,
        issue_now: true,
      });
    }
  }, [form, open]);
  const createMutation = useMutation({
    mutationFn: (values: CertificateFormValues) =>
      createCertificate({
        domain: values.domain,
        alt_names: values.alt_names.flatMap(parseDomainList),
        auto_renew: values.auto_renew,
        issue_now: values.issue_now,
      }),
    onSuccess: (certificate) => {
      message.success(
        certificate.issuing
          ? t("已开始申请证书，通常需要 1 到 3 分钟")
          : t("证书记录已创建"),
      );
      queryClient.invalidateQueries({ queryKey: ["admin-certificates"] });
      onClose();
    },
    onError: (error) => message.error(errorText(error, t("创建证书失败"))),
  });

  return (
    <Modal
      title={t("申请证书")}
      open={open}
      onCancel={onClose}
      onOk={() => form.submit()}
      okText={t("确定")}
      confirmLoading={createMutation.isPending}
      width="min(560px, 96vw)"
      className="cert-modal"
      forceRender
    >
      <Form
        form={form}
        layout="vertical"
        requiredMark={false}
        onFinish={(values) => createMutation.mutate(values)}
      >
        <Form.Item
          name="domain"
          label={t("主域名")}
          rules={[{ required: true, message: t("请输入主域名") }]}
          extra={t("支持通配符，例如 *.example.com")}
        >
          <Input placeholder="example.com" />
        </Form.Item>
        <Form.Item name="alt_names" label={t("附加域名")}>
          <Select
            mode="tags"
            tokenSeparators={[",", " ", "，"]}
            placeholder="*.example.com"
            open={false}
          />
        </Form.Item>
        <div className="cert-switch-row">
          <span>
            <strong>{t("自动续签")}</strong>
            <Typography.Text type="secondary">
              {t("到期前按设置的天数自动续签")}
            </Typography.Text>
          </span>
          <Form.Item name="auto_renew" valuePropName="checked" noStyle>
            <Switch aria-label={t("自动续签")} />
          </Form.Item>
        </div>
        <div className="cert-switch-row">
          <span>
            <strong>{t("立即申请")}</strong>
            <Typography.Text type="secondary">
              {t("通过 Cloudflare DNS 验证域名所有权")}
            </Typography.Text>
          </span>
          <Form.Item name="issue_now" valuePropName="checked" noStyle>
            <Switch aria-label={t("立即申请")} />
          </Form.Item>
        </div>
      </Form>
    </Modal>
  );
}

function SettingsPanel({ settings }: { settings?: CertificateSettings }) {
  const [form] = Form.useForm<SettingsFormValues>();
  const queryClient = useQueryClient();
  const canEdit = useAuthStore((state) =>
    state.hasPermission("system:certificate:update"),
  );
  const authMode = Form.useWatch("cloudflare_auth_mode", form);

  useEffect(() => {
    if (settings) {
      form.setFieldsValue({ ...settings, cloudflare_api_token: "" });
    }
  }, [form, settings]);

  const saveMutation = useMutation({
    mutationFn: (values: SettingsFormValues) =>
      saveCertificateSettings({
        ...values,
        cloudflare_api_token: values.cloudflare_api_token?.trim() || undefined,
      }),
    onSuccess: (saved) => {
      message.success(t("设置已保存"));
      queryClient.setQueryData(["admin-certificate-settings"], saved);
      form.setFieldValue("cloudflare_api_token", "");
      queryClient.invalidateQueries({ queryKey: ["admin-domain-mappings"] });
    },
    onError: (error) => message.error(errorText(error, t("保存设置失败"))),
  });

  return (
    <Form
      form={form}
      layout="vertical"
      requiredMark={false}
      disabled={!canEdit}
      onFinish={(values) => saveMutation.mutate(values)}
      className="cert-settings"
    >
      <Row gutter={[16, 0]}>
        <Col xs={24} lg={12}>
          <Card size="small" title={t("证书申请（Let's Encrypt）")}>
            <Form.Item
              name="acme_email"
              label={t("联系邮箱")}
              extra={t("用于接收证书到期等通知，可留空")}
            >
              <Input placeholder="ops@example.com" />
            </Form.Item>
            <Form.Item name="acme_directory" label={t("证书环境")}>
              <Select
                options={[
                  { value: "production", label: t("正式环境（浏览器信任）") },
                  {
                    value: "staging",
                    label: t("测试环境（不受信任，用于调试，不占额度）"),
                  },
                ]}
              />
            </Form.Item>
            <div className="cert-form-grid">
              <Form.Item name="renew_before_days" label={t("提前续签（天）")}>
                <InputNumber min={1} max={60} className="full-width" />
              </Form.Item>
              <Form.Item
                name="propagation_timeout_seconds"
                label={t("DNS 生效等待（秒）")}
              >
                <InputNumber min={0} max={600} className="full-width" />
              </Form.Item>
            </div>
            <Form.Item
              name="ipv4_lookup_url"
              label={t("公网 IPv4 查询地址")}
              extra={t(
                "IPv4 解析选择自动时使用；IPv6 沿用「IPv6 动态解析」的检测方式",
              )}
            >
              <Input placeholder="https://api.ipify.org" />
            </Form.Item>
          </Card>
        </Col>
        <Col xs={24} lg={12}>
          <Card size="small" title="Cloudflare">
            {settings?.uses_ddns_credentials ? (
              <Alert
                type="info"
                showIcon
                className="cert-inline-alert"
                message={t(
                  "未单独设置 Token，正在使用「IPv6 动态解析」中的 Cloudflare 凭据。",
                )}
              />
            ) : null}
            <Form.Item name="cloudflare_auth_mode" label={t("认证方式")}>
              <Select
                options={[
                  { value: "api_token", label: t("API Token（推荐）") },
                  { value: "global_api_key", label: "Global API Key" },
                ]}
              />
            </Form.Item>
            <Form.Item
              name="cloudflare_api_token"
              label={
                authMode === "global_api_key" ? "Global API Key" : "API Token"
              }
              extra={t(
                "需要权限：Zone · DNS · Edit；使用隧道还需 Account · Cloudflare Tunnel · Edit",
              )}
            >
              <Input.Password
                autoComplete="new-password"
                placeholder={
                  settings?.cloudflare_api_token_set
                    ? t("已设置，留空保持不变")
                    : t("留空则使用 IPv6 动态解析的凭据")
                }
              />
            </Form.Item>
            {authMode === "global_api_key" ? (
              <Form.Item
                name="cloudflare_api_email"
                label={t("Cloudflare 账号邮箱")}
              >
                <Input />
              </Form.Item>
            ) : null}
            <Form.Item
              name="cloudflare_account_id"
              label="Account ID"
              extra={t(
                "使用 Cloudflare Tunnel 时必填，可在 Cloudflare 控制台右侧找到",
              )}
            >
              <Input placeholder="0123456789abcdef0123456789abcdef" />
            </Form.Item>
            <div className="cert-form-grid">
              <Form.Item name="tunnel_name" label={t("隧道名称")}>
                <Input placeholder={t("默认 gpt-images-主机名")} />
              </Form.Item>
              <Form.Item
                name="cloudflared_path"
                label={t("cloudflared 路径")}
                extra={t("留空则自动在 PATH 中查找")}
              >
                <Input placeholder="/usr/local/bin/cloudflared" />
              </Form.Item>
            </div>
            <Form.Item name="cloudflare_api_base" label={t("API 地址（高级）")}>
              <Input placeholder="https://api.cloudflare.com/client/v4" />
            </Form.Item>
          </Card>
        </Col>
      </Row>
      <div className="cert-settings-actions">
        <PermissionButton
          permission="system:certificate:update"
          type="primary"
          htmlType="submit"
          icon={<SaveOutlined />}
          loading={saveMutation.isPending}
        >
          {t("保存设置")}
        </PermissionButton>
      </div>
    </Form>
  );
}

export function CertificatesPage() {
  const queryClient = useQueryClient();
  const canUpdate = useAuthStore((state) =>
    state.hasPermission("system:certificate:update"),
  );
  const [tab, setTab] = useState("mappings");
  const [mappingDialog, setMappingDialog] = useState<{
    open: boolean;
    editing: DomainMappingRecord | null;
  }>({ open: false, editing: null });
  const [certificateDialogOpen, setCertificateDialogOpen] = useState(false);

  const certificatesQuery = useQuery({
    queryKey: ["admin-certificates"],
    queryFn: fetchCertificates,
    refetchInterval: (query) =>
      query.state.data?.some((certificate) => certificate.issuing)
        ? 4000
        : 30_000,
  });
  const mappingsQuery = useQuery({
    queryKey: ["admin-domain-mappings"],
    queryFn: fetchDomainMappings,
    refetchInterval: (query) =>
      query.state.data?.some((mapping) => mapping.status === "pending")
        ? 5000
        : 30_000,
  });
  const runtimeQuery = useQuery({
    queryKey: ["admin-certificate-runtime"],
    queryFn: fetchCertificateRuntime,
    refetchInterval: 10_000,
  });
  const settingsQuery = useQuery({
    queryKey: ["admin-certificate-settings"],
    queryFn: fetchCertificateSettings,
  });

  const refresh = () => {
    for (const key of [
      "admin-certificates",
      "admin-domain-mappings",
      "admin-certificate-runtime",
    ]) {
      queryClient.invalidateQueries({ queryKey: [key] });
    }
  };

  const syncMutation = useMutation({
    mutationFn: syncDomainMapping,
    onSuccess: (result) => {
      if (result.mapping.status === "error") {
        message.warning(result.mapping.last_error || t("同步完成，但存在问题"));
      } else {
        message.success(t("已同步"));
      }
      refresh();
    },
    onError: (error) => message.error(errorText(error, t("同步失败"))),
  });
  const deleteMappingMutation = useMutation({
    mutationFn: (mapping: DomainMappingRecord) =>
      deleteDomainMapping(mapping.id),
    onSuccess: () => {
      message.success(t("域名映射已删除"));
      refresh();
    },
    onError: (error) => message.error(errorText(error, t("删除失败"))),
  });
  const toggleMappingMutation = useMutation({
    mutationFn: (mapping: DomainMappingRecord) =>
      updateDomainMapping(mapping.id, {
        ...mapping,
        ipv4_address: mapping.ipv4_address ?? undefined,
        ipv6_address: mapping.ipv6_address ?? undefined,
        enabled: !mapping.enabled,
        issue_certificate: false,
      }),
    onSuccess: refresh,
    onError: (error) => message.error(errorText(error, t("操作失败"))),
  });
  const issueMutation = useMutation({
    mutationFn: (certificate: CertificateRecord) =>
      issueCertificate(certificate.id),
    onSuccess: () => {
      message.success(t("已开始申请证书，通常需要 1 到 3 分钟"));
      refresh();
    },
    onError: (error) => message.error(errorText(error, t("申请证书失败"))),
  });
  const deleteCertificateMutation = useMutation({
    mutationFn: (certificate: CertificateRecord) =>
      deleteCertificate(certificate.id),
    onSuccess: () => {
      message.success(t("证书已删除"));
      refresh();
    },
    onError: (error) => message.error(errorText(error, t("删除失败"))),
  });
  const autoRenewMutation = useMutation({
    mutationFn: (certificate: CertificateRecord) =>
      updateCertificate(certificate.id, {
        auto_renew: !certificate.auto_renew,
      }),
    onSuccess: refresh,
    onError: (error) => message.error(errorText(error, t("操作失败"))),
  });
  const restartMutation = useMutation({
    mutationFn: restartTunnel,
    onSuccess: () => {
      message.success(t("cloudflared 已重启"));
      refresh();
    },
    onError: (error) => message.error(errorText(error, t("重启失败"))),
  });

  const runtime = runtimeQuery.data;
  const mappings = mappingsQuery.data ?? [];
  const certificates = certificatesQuery.data ?? [];
  const summary = useMemo(
    () => ({
      active: mappings.filter((mapping) => mapping.status === "active").length,
      problems: mappings.filter((mapping) => mapping.status === "error").length,
      expiring: certificates.filter(
        (certificate) =>
          certificate.days_remaining !== null &&
          certificate.days_remaining !== undefined &&
          certificate.days_remaining <= 14,
      ).length,
    }),
    [certificates, mappings],
  );
  const usesTunnel = mappings.some((mapping) => mapping.mode === "tunnel");

  const mappingColumns: ColumnsType<DomainMappingRecord> = [
    {
      title: t("域名"),
      dataIndex: "domain",
      render: (_, mapping) => (
        <div className="cert-cell-main">
          <a href={mapping.public_url} target="_blank" rel="noreferrer">
            {mapping.public_url.replace("https://", "")} <ExportOutlined />
          </a>
          {mapping.name !== mapping.domain ? (
            <Typography.Text type="secondary">{mapping.name}</Typography.Text>
          ) : null}
        </div>
      ),
    },
    {
      title: t("方式"),
      dataIndex: "mode",
      width: 128,
      render: (mode: string) =>
        mode === "tunnel" ? (
          <Tag icon={<CloudOutlined />} color="orange">
            Tunnel
          </Tag>
        ) : (
          <Tag icon={<SafetyCertificateOutlined />} color="blue">
            {t("HTTPS 直连")}
          </Tag>
        ),
    },
    {
      title: t("本地服务"),
      key: "upstream",
      render: (_, mapping) => (
        <code className="cert-code">
          {upstreamLabel(mapping.upstream_host, mapping.upstream_port)}
        </code>
      ),
    },
    {
      title: t("允许访问 IP"),
      key: "allowed_ips",
      width: 220,
      render: (_, mapping) =>
        mapping.allowed_ips.length ? (
          <Space size={4} wrap>
            {mapping.allowed_ips.slice(0, 3).map((value) => (
              <Tag key={value}>{value}</Tag>
            ))}
            {mapping.allowed_ips.length > 3 ? (
              <Tooltip title={mapping.allowed_ips.slice(3).join(", ")}>
                <Tag>+{mapping.allowed_ips.length - 3}</Tag>
              </Tooltip>
            ) : null}
          </Space>
        ) : (
          <Typography.Text type="secondary">{t("全部 IP")}</Typography.Text>
        ),
    },
    {
      title: "DNS",
      key: "dns",
      render: (_, mapping) =>
        mapping.mode === "tunnel" ? (
          <Typography.Text type="secondary">
            {t("CNAME → 隧道")}
          </Typography.Text>
        ) : (
          <Space size={4} wrap>
            <Tag>IPv4 {ipModeLabels[mapping.ipv4_mode]}</Tag>
            <Tag>IPv6 {ipModeLabels[mapping.ipv6_mode]}</Tag>
          </Space>
        ),
    },
    {
      title: t("状态"),
      dataIndex: "status",
      width: 120,
      render: (status: string, mapping) => {
        const tag = (
          <Tag color={mappingTone(status)}>
            {mappingStatusLabels[status] ?? status}
          </Tag>
        );
        return mapping.last_error ? (
          <Tooltip title={mapping.last_error}>{tag}</Tooltip>
        ) : (
          tag
        );
      },
    },
    {
      title: t("操作"),
      key: "actions",
      fixed: "right",
      width: 260,
      render: (_, mapping) => (
        <>
          <PermissionButton
            permission="system:certificate:issue"
            size="small"
            type="text"
            icon={<SyncOutlined />}
            loading={
              syncMutation.isPending && syncMutation.variables === mapping.id
            }
            onClick={() => syncMutation.mutate(mapping.id)}
          >
            {t("同步")}
          </PermissionButton>
          <PermissionButton
            permission="system:certificate:update"
            size="small"
            type="text"
            icon={<EditOutlined />}
            onClick={() => setMappingDialog({ open: true, editing: mapping })}
          >
            {t("编辑")}
          </PermissionButton>
          <PermissionButton
            permission="system:certificate:update"
            size="small"
            type="text"
            onClick={() => toggleMappingMutation.mutate(mapping)}
          >
            {mapping.enabled ? t("停用") : t("启用")}
          </PermissionButton>
          <Popconfirm
            title={t("删除该域名映射？")}
            description={t("同时删除它在 Cloudflare 上创建的解析记录")}
            okButtonProps={{ danger: true }}
            okText={t("删除")}
            onConfirm={() => deleteMappingMutation.mutate(mapping)}
          >
            <PermissionButton
              permission="system:certificate:delete"
              size="small"
              type="text"
              danger
              icon={<DeleteOutlined />}
            >
              {t("删除")}
            </PermissionButton>
          </Popconfirm>
        </>
      ),
    },
  ];

  const certificateColumns: ColumnsType<CertificateRecord> = [
    {
      title: t("域名"),
      dataIndex: "domain",
      render: (_, certificate) => (
        <div className="cert-cell-main">
          <strong>{certificate.domain}</strong>
          {certificate.alt_names.length > 0 ? (
            <Typography.Text type="secondary">
              {certificate.alt_names.join("，")}
            </Typography.Text>
          ) : null}
        </div>
      ),
    },
    {
      title: t("状态"),
      dataIndex: "status",
      width: 110,
      render: (status: string, certificate) => {
        const shown = certificate.issuing ? "issuing" : status;
        const tag = (
          <Tag color={certificateTone(status, certificate.issuing)}>
            {certificateStatusLabels[shown] ?? shown}
          </Tag>
        );
        return certificate.last_error ? (
          <Tooltip title={certificate.last_error}>{tag}</Tooltip>
        ) : (
          tag
        );
      },
    },
    {
      title: t("到期时间"),
      dataIndex: "not_after",
      render: (value: string | null, certificate) =>
        value ? (
          <Space size={6} wrap>
            <span>{value}</span>
            <Tag color={expiryTone(certificate.days_remaining)}>
              {(certificate.days_remaining ?? 0) < 0
                ? t("已过期")
                : t("剩余 {0} 天", certificate.days_remaining)}
            </Tag>
          </Space>
        ) : (
          <Typography.Text type="secondary">-</Typography.Text>
        ),
    },
    {
      title: t("颁发者"),
      dataIndex: "issuer",
      render: (value?: string | null) => value || "-",
    },
    {
      title: t("自动续签"),
      dataIndex: "auto_renew",
      width: 100,
      render: (value: boolean, certificate) => (
        <Switch
          size="small"
          checked={value}
          aria-label={t("自动续签")}
          disabled={!canUpdate}
          onChange={() => autoRenewMutation.mutate(certificate)}
        />
      ),
    },
    {
      title: t("操作"),
      key: "actions",
      fixed: "right",
      width: 190,
      render: (_, certificate) => (
        <>
          <PermissionButton
            permission="system:certificate:issue"
            size="small"
            type="text"
            icon={<SafetyCertificateOutlined />}
            disabled={certificate.issuing}
            onClick={() => issueMutation.mutate(certificate)}
          >
            {certificate.status === "active" ? t("续签") : t("申请")}
          </PermissionButton>
          <Popconfirm
            title={t("删除该证书？")}
            description={t("证书文件会一并删除")}
            okButtonProps={{ danger: true }}
            okText={t("删除")}
            onConfirm={() => deleteCertificateMutation.mutate(certificate)}
          >
            <PermissionButton
              permission="system:certificate:delete"
              size="small"
              type="text"
              danger
              icon={<DeleteOutlined />}
            >
              {t("删除")}
            </PermissionButton>
          </Popconfirm>
        </>
      ),
    },
  ];

  const tunnelInstalled = Boolean(runtime?.tunnel.binary);

  return (
    <div className="cert-page">
      <PageHeader
        title={t("证书与域名")}
        extra={
          <Space wrap>
            <Button icon={<ReloadOutlined />} onClick={refresh}>
              {t("刷新")}
            </Button>
            {tab === "certificates" ? (
              <PermissionButton
                permission="system:certificate:create"
                type="primary"
                icon={<PlusOutlined />}
                onClick={() => setCertificateDialogOpen(true)}
              >
                {t("申请证书")}
              </PermissionButton>
            ) : (
              <PermissionButton
                permission="system:certificate:create"
                type="primary"
                icon={<PlusOutlined />}
                onClick={() => setMappingDialog({ open: true, editing: null })}
              >
                {t("新增映射")}
              </PermissionButton>
            )}
          </Space>
        }
      />
      <div className="cert-summary">
        <div className="cert-summary-item">
          <span>{t("运行中的映射")}</span>
          <strong>{summary.active}</strong>
        </div>
        <div
          className={`cert-summary-item${summary.problems ? " is-error" : ""}`}
        >
          <span>{t("异常映射")}</span>
          <strong>{summary.problems}</strong>
        </div>
        <div
          className={`cert-summary-item${summary.expiring ? " is-warning" : ""}`}
        >
          <span>{t("14 天内到期证书")}</span>
          <strong>{summary.expiring}</strong>
        </div>
        <div className="cert-summary-item">
          <span>{t("HTTPS 监听端口")}</span>
          <strong>
            {runtime?.listeners.filter((listener) => listener.running).length ??
              0}
          </strong>
        </div>
      </div>
      <Tabs
        activeKey={tab}
        onChange={setTab}
        items={[
          {
            key: "mappings",
            label: t("域名映射"),
            children: (
              <DataTable<DomainMappingRecord>
                loading={mappingsQuery.isLoading}
                dataSource={mappings}
                columns={mappingColumns}
                pagination={false}
                locale={{
                  emptyText: (
                    <Empty
                      description={t(
                        "还没有域名映射：把本地服务端口发布为 https://域名",
                      )}
                    />
                  ),
                }}
                expandable={{
                  rowExpandable: (mapping) => Boolean(mapping.last_error),
                  expandedRowRender: (mapping) => (
                    <Alert
                      type={mapping.status === "error" ? "error" : "warning"}
                      showIcon
                      message={mapping.last_error}
                    />
                  ),
                }}
              />
            ),
          },
          {
            key: "certificates",
            label: t("证书"),
            children: (
              <DataTable<CertificateRecord>
                loading={certificatesQuery.isLoading}
                dataSource={certificates}
                columns={certificateColumns}
                pagination={false}
                locale={{ emptyText: <Empty description={t("还没有证书")} /> }}
                expandable={{
                  rowExpandable: (certificate) =>
                    Boolean(certificate.last_error),
                  expandedRowRender: (certificate) => (
                    <Alert
                      type="error"
                      showIcon
                      message={certificate.last_error}
                    />
                  ),
                }}
              />
            ),
          },
          {
            key: "runtime",
            label: t("运行状态"),
            children: (
              <Row gutter={[16, 16]}>
                <Col xs={24} lg={10}>
                  <Card size="small" title={t("HTTPS 监听")}>
                    {runtime?.listeners.length ? (
                      <div className="cert-listener-list">
                        {runtime.listeners.map((listener) => (
                          <div key={listener.port} className="cert-listener">
                            <div className="cert-listener-head">
                              <code>:{listener.port}</code>
                              <Tag
                                color={listener.running ? "success" : "error"}
                              >
                                {listener.running ? t("监听中") : t("未监听")}
                              </Tag>
                            </div>
                            {listener.hosts.length ? (
                              <Typography.Text type="secondary">
                                {listener.hosts.join("，")}
                              </Typography.Text>
                            ) : null}
                            {listener.error ? (
                              <Typography.Text type="danger">
                                {listener.error}
                              </Typography.Text>
                            ) : null}
                          </div>
                        ))}
                      </div>
                    ) : (
                      <Empty
                        image={Empty.PRESENTED_IMAGE_SIMPLE}
                        description={t("证书签发成功后会自动开始监听")}
                      />
                    )}
                  </Card>
                </Col>
                <Col xs={24} lg={14}>
                  <Card
                    size="small"
                    title={
                      <Space>
                        <CloudOutlined />
                        <span>Cloudflare Tunnel</span>
                        {runtime ? (
                          <Tag
                            color={
                              runtime.tunnel.running ? "success" : "default"
                            }
                          >
                            {runtime.tunnel.running ? t("运行中") : t("未运行")}
                          </Tag>
                        ) : null}
                      </Space>
                    }
                    extra={
                      <PermissionButton
                        permission="system:certificate:issue"
                        size="small"
                        icon={<ReloadOutlined />}
                        disabled={!runtime?.tunnel.running}
                        loading={restartMutation.isPending}
                        onClick={() => restartMutation.mutate()}
                      >
                        {t("重启")}
                      </PermissionButton>
                    }
                  >
                    {!tunnelInstalled && usesTunnel ? (
                      <Alert
                        type="warning"
                        showIcon
                        className="cert-inline-alert"
                        message={t("本机未找到 cloudflared")}
                        description={
                          <span>
                            macOS：<code>brew install cloudflared</code>
                            {t(
                              "； Debian/Ubuntu：参考 Cloudflare 官方安装文档； Windows：",
                            )}
                            <code>winget install Cloudflare.cloudflared</code>
                            {t(
                              "。 安装后在「设置」中填写路径或确保它在 PATH 中。",
                            )}
                          </span>
                        }
                      />
                    ) : null}
                    <dl className="cert-facts">
                      <dt>{t("程序")}</dt>
                      <dd>{runtime?.tunnel.binary ?? t("未找到")}</dd>
                      <dt>{t("隧道")}</dt>
                      <dd>
                        {runtime?.tunnel_name}
                        {runtime?.tunnel_id ? (
                          <Typography.Text type="secondary">
                            {" "}
                            （{runtime.tunnel_id}）
                          </Typography.Text>
                        ) : null}
                      </dd>
                      <dt>{t("进程")}</dt>
                      <dd>
                        {runtime?.tunnel.pid
                          ? `PID ${runtime.tunnel.pid}`
                          : "-"}
                        {runtime?.tunnel.restarts
                          ? t("，已重启 {0} 次", runtime.tunnel.restarts)
                          : ""}
                      </dd>
                    </dl>
                    {runtime?.tunnel.recent_logs.length ? (
                      <pre
                        className="cert-logs"
                        aria-label={t("cloudflared 日志")}
                      >
                        {runtime.tunnel.recent_logs.slice(-30).join("\n")}
                      </pre>
                    ) : null}
                  </Card>
                </Col>
              </Row>
            ),
          },
          {
            key: "settings",
            label: (
              <span>
                <ApiOutlined /> {t("设置")}
              </span>
            ),
            children: <SettingsPanel settings={settingsQuery.data} />,
          },
        ]}
      />
      <MappingDialog
        open={mappingDialog.open}
        editing={mappingDialog.editing}
        serverPort={runtime?.server_port}
        onClose={() => setMappingDialog({ open: false, editing: null })}
      />
      <CertificateDialog
        open={certificateDialogOpen}
        onClose={() => setCertificateDialogOpen(false)}
      />
    </div>
  );
}
