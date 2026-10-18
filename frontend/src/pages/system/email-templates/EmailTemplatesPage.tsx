import {
  DeleteOutlined,
  EditOutlined,
  EyeOutlined,
  MailOutlined,
  PlusOutlined,
  SendOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Descriptions,
  Drawer,
  Form,
  Input,
  Modal,
  Popconfirm,
  Select,
  Space,
  Switch,
  Tag,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useState } from "react";
import {
  type EmailTemplateRecord,
  type RenderedEmailTemplate,
  type SaveEmailTemplateParams,
  createEmailTemplate,
  deleteEmailTemplate,
  fetchEmailTemplates,
  previewEmailTemplate,
  testSendEmailTemplate,
  updateEmailTemplate,
} from "../../../api/admin/emailTemplates";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { t } from "../../../i18n";

const templateTypeOptions = [
  { value: "auth", label: t("认证") },
  { value: "system", label: t("系统") },
  { value: "marketing", label: t("营销") },
];

const defaultVariables = JSON.stringify(
  [{ name: "name", description: t("用户名称") }],
  null,
  2,
);

const defaultLocals = JSON.stringify(
  {
    name: "Admin",
    domain: "http://localhost:5150",
    host: "http://localhost:5150",
    verifyToken: "verify-token",
    resetToken: "reset-token",
    token: "magic-token",
  },
  null,
  2,
);

function parseJsonInput(value: string) {
  try {
    return JSON.parse(value || "{}");
  } catch {
    message.error(t("变量 JSON 格式不正确"));
    return undefined;
  }
}

function formatJson(value: string) {
  try {
    return JSON.stringify(JSON.parse(value), null, 2);
  } catch {
    return value;
  }
}

export function EmailTemplatesPage() {
  const [page, setPage] = useState(1);
  const [keyword, setKeyword] = useState("");
  const [templateType, setTemplateType] = useState<string>();
  const [editing, setEditing] = useState<EmailTemplateRecord | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [previewing, setPreviewing] = useState<EmailTemplateRecord | null>(
    null,
  );
  const [previewResult, setPreviewResult] =
    useState<RenderedEmailTemplate | null>(null);
  const [testing, setTesting] = useState<EmailTemplateRecord | null>(null);
  const [form] = Form.useForm<SaveEmailTemplateParams>();
  const [previewForm] = Form.useForm<{ locals: string }>();
  const [testForm] = Form.useForm<{ to: string; locals: string }>();
  const queryClient = useQueryClient();

  const templatesQuery = useQuery({
    queryKey: ["admin-email-templates", page, keyword, templateType],
    queryFn: () =>
      fetchEmailTemplates({
        page,
        page_size: 10,
        keyword: keyword || undefined,
        template_type: templateType,
      }),
  });

  const saveMutation = useMutation({
    mutationFn: (values: SaveEmailTemplateParams) =>
      editing
        ? updateEmailTemplate(editing.id, values)
        : createEmailTemplate(values),
    onSuccess: () => {
      message.success(t("邮箱模板已保存"));
      setFormOpen(false);
      setEditing(null);
      queryClient.invalidateQueries({ queryKey: ["admin-email-templates"] });
    },
  });
  const deleteMutation = useMutation({
    mutationFn: deleteEmailTemplate,
    onSuccess: () => {
      message.success(t("邮箱模板已删除"));
      queryClient.invalidateQueries({ queryKey: ["admin-email-templates"] });
    },
  });
  const previewMutation = useMutation({
    mutationFn: ({ id, locals }: { id: number; locals: unknown }) =>
      previewEmailTemplate(id, locals),
    onSuccess: setPreviewResult,
  });
  const testSendMutation = useMutation({
    mutationFn: ({
      id,
      payload,
    }: {
      id: number;
      payload: { to: string; locals: unknown };
    }) => testSendEmailTemplate(id, payload),
    onSuccess: () => {
      message.success(t("测试邮件已提交发送"));
      setTesting(null);
    },
  });

  const columns: ColumnsType<EmailTemplateRecord> = [
    { title: t("编码"), dataIndex: "code", width: 220 },
    { title: t("名称"), dataIndex: "name", width: 180 },
    {
      title: t("类型"),
      dataIndex: "template_type",
      width: 100,
      render: (value) => <Tag>{value}</Tag>,
    },
    { title: t("主题"), dataIndex: "subject", width: 220, ellipsis: true },
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
    { title: t("更新时间"), dataIndex: "updated_at", width: 220 },
    {
      title: t("操作"),
      key: "actions",
      width: 320,
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            icon={<EyeOutlined />}
            permission="system:email_template:list"
            onClick={() => {
              setPreviewing(record);
              setPreviewResult(null);
              previewForm.setFieldsValue({ locals: defaultLocals });
            }}
          >
            {t("预览")}
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<SendOutlined />}
            permission="system:email_template:test"
            onClick={() => {
              setTesting(record);
              testForm.setFieldsValue({ locals: defaultLocals });
            }}
          >
            {t("测试")}
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:email_template:update"
            onClick={() => {
              setEditing(record);
              form.setFieldsValue({
                ...record,
                variables: formatJson(record.variables),
              });
              setFormOpen(true);
            }}
          >
            {t("编辑")}
          </PermissionButton>
          <Popconfirm
            title={t("确认删除邮箱模板？")}
            onConfirm={() => deleteMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              disabled={record.is_builtin}
              icon={<DeleteOutlined />}
              permission="system:email_template:delete"
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
      title={t("邮箱模板")}
      subtitle={t("管理认证、系统和营销邮件模板，支持变量预览与测试发送")}
      breadcrumb={[t("系统管理"), t("邮箱模板")]}
      icon={<MailOutlined />}
      toolbar={
        <CrudToolbar
          actions={[
            {
              key: "create",
              label: t("新增模板"),
              icon: <PlusOutlined />,
              primary: true,
              permission: "system:email_template:create",
              onClick: () => {
                setEditing(null);
                form.resetFields();
                form.setFieldsValue({
                  template_type: "auth",
                  variables: defaultVariables,
                  enabled: true,
                  is_builtin: false,
                });
                setFormOpen(true);
              },
            },
          ]}
        />
      }
    >
      <Space direction="vertical" size="middle" className="full-width">
        <Space wrap>
          <Input.Search
            allowClear
            placeholder={t("搜索编码、名称")}
            onSearch={(value) => {
              setPage(1);
              setKeyword(value);
            }}
            className="admin-search-input"
          />
          <Select
            allowClear
            placeholder={t("模板类型")}
            options={templateTypeOptions}
            value={templateType}
            onChange={(value) => {
              setPage(1);
              setTemplateType(value);
            }}
            className="admin-filter-select"
          />
        </Space>
        <DataTable<EmailTemplateRecord>
          columns={columns}
          dataSource={templatesQuery.data?.items ?? []}
          loading={templatesQuery.isLoading}
          pagination={{
            current: page,
            total: templatesQuery.data?.total ?? 0,
            onChange: setPage,
          }}
        />
      </Space>

      <Modal
        title={editing ? t("编辑邮箱模板") : t("新增邮箱模板")}
        open={formOpen}
        onCancel={() => setFormOpen(false)}
        onOk={() => form.submit()}
        confirmLoading={saveMutation.isPending}
        width={860}
        destroyOnHidden
      >
        <Form
          form={form}
          layout="vertical"
          onFinish={(values) => saveMutation.mutate(values)}
        >
          <Form.Item
            name="code"
            label={t("模板编码")}
            rules={[{ required: true }]}
          >
            <Input
              placeholder="auth_welcome"
              disabled={Boolean(editing?.is_builtin)}
            />
          </Form.Item>
          <Form.Item
            name="name"
            label={t("模板名称")}
            rules={[{ required: true }]}
          >
            <Input />
          </Form.Item>
          <Form.Item
            name="template_type"
            label={t("模板类型")}
            rules={[{ required: true }]}
          >
            <Select options={templateTypeOptions} />
          </Form.Item>
          <Form.Item
            name="subject"
            label={t("邮件主题")}
            rules={[{ required: true }]}
          >
            <Input />
          </Form.Item>
          <Form.Item
            name="html_body"
            label={t("HTML 内容")}
            rules={[{ required: true }]}
          >
            <Input.TextArea rows={8} />
          </Form.Item>
          <Form.Item
            name="text_body"
            label={t("纯文本内容")}
            rules={[{ required: true }]}
          >
            <Input.TextArea rows={5} />
          </Form.Item>
          <Form.Item
            name="variables"
            label={t("变量说明 JSON")}
            rules={[{ required: true }]}
          >
            <Input.TextArea rows={5} />
          </Form.Item>
          <Form.Item name="description" label={t("描述")}>
            <Input.TextArea rows={2} />
          </Form.Item>
          <Space>
            <Form.Item name="enabled" label={t("启用")} valuePropName="checked">
              <Switch />
            </Form.Item>
            <Form.Item
              name="is_builtin"
              label={t("内置")}
              valuePropName="checked"
            >
              <Switch disabled={Boolean(editing?.is_builtin)} />
            </Form.Item>
          </Space>
        </Form>
      </Modal>

      <Drawer
        title={t("模板预览")}
        open={Boolean(previewing)}
        onClose={() => setPreviewing(null)}
        width={760}
      >
        {previewing && (
          <Space direction="vertical" size="middle" className="full-width">
            <Form
              form={previewForm}
              layout="vertical"
              onFinish={(values) => {
                const locals = parseJsonInput(values.locals);
                if (locals) {
                  previewMutation.mutate({ id: previewing.id, locals });
                }
              }}
            >
              <Form.Item name="locals" label={t("预览变量 JSON")}>
                <Input.TextArea rows={8} />
              </Form.Item>
              <PermissionButton
                type="primary"
                permission="system:email_template:list"
                loading={previewMutation.isPending}
                onClick={() => previewForm.submit()}
              >
                {t("生成预览")}
              </PermissionButton>
            </Form>
            {previewResult && (
              <Descriptions column={1} bordered size="small">
                <Descriptions.Item label={t("主题")}>
                  {previewResult.subject}
                </Descriptions.Item>
                <Descriptions.Item label="HTML">
                  <pre className="admin-code-block">
                    {previewResult.html_body}
                  </pre>
                </Descriptions.Item>
                <Descriptions.Item label={t("纯文本")}>
                  <pre className="admin-code-block">
                    {previewResult.text_body}
                  </pre>
                </Descriptions.Item>
              </Descriptions>
            )}
          </Space>
        )}
      </Drawer>

      <Modal
        title={t("测试发送")}
        open={Boolean(testing)}
        onCancel={() => setTesting(null)}
        onOk={() => testForm.submit()}
        confirmLoading={testSendMutation.isPending}
        destroyOnHidden
      >
        {testing && (
          <Form
            form={testForm}
            layout="vertical"
            onFinish={(values) => {
              const locals = parseJsonInput(values.locals);
              if (locals) {
                testSendMutation.mutate({
                  id: testing.id,
                  payload: { to: values.to, locals },
                });
              }
            }}
          >
            <Form.Item
              name="to"
              label={t("收件邮箱")}
              rules={[{ required: true }]}
            >
              <Input placeholder="admin@example.com" />
            </Form.Item>
            <Form.Item name="locals" label={t("测试变量 JSON")}>
              <Input.TextArea rows={8} />
            </Form.Item>
          </Form>
        )}
      </Modal>
    </CrudPage>
  );
}
