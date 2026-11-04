import { ApiOutlined } from "@ant-design/icons";
import { useMutation } from "@tanstack/react-query";
import {
  Alert,
  AutoComplete,
  Button,
  Checkbox,
  Col,
  Form,
  Input,
  InputNumber,
  Modal,
  Radio,
  Row,
  Select,
  Space,
  Switch,
  message,
} from "antd";
import { useEffect, useState } from "react";
import {
  type DbConnection,
  type DbType,
  type SaveDbConnectionParams,
  createDbConnection,
  testDbConnection,
  updateDbConnection,
} from "../../../api/admin/dbManager";
import { t } from "../../../i18n";
import { DB_TYPE_LABELS, DEFAULT_PORTS } from "./dbUtils";

type FormValues = {
  name: string;
  db_type: DbType;
  host?: string;
  port?: number | null;
  username?: string;
  password?: string;
  clear_password?: boolean;
  database_name?: string;
  ssl_mode?: string;
  read_only?: boolean;
  group_name?: string;
  color?: string;
  remark?: string;
};

const SSL_MODES: Record<DbType, { value: string; label: string }[]> = {
  postgres: [
    { value: "disable", label: "disable" },
    { value: "prefer", label: "prefer" },
    { value: "require", label: "require" },
    { value: "verify-ca", label: "verify-ca" },
    { value: "verify-full", label: "verify-full" },
  ],
  mysql: [
    { value: "disabled", label: "disabled" },
    { value: "preferred", label: "preferred" },
    { value: "required", label: "required" },
    { value: "verify_ca", label: "verify_ca" },
    { value: "verify_identity", label: "verify_identity" },
  ],
  sqlite: [],
};

export const CONNECTION_COLORS = [
  "#1677ff",
  "#52c41a",
  "#faad14",
  "#f5222d",
  "#722ed1",
  "#13c2c2",
  "#eb2f96",
];

function toParams(values: FormValues): SaveDbConnectionParams {
  const sqlite = values.db_type === "sqlite";
  return {
    name: values.name,
    db_type: values.db_type,
    host: sqlite ? null : values.host,
    port: sqlite ? null : (values.port ?? DEFAULT_PORTS[values.db_type]),
    username: sqlite ? null : values.username,
    password: values.password || null,
    clear_password: values.clear_password ?? false,
    database_name: values.database_name || null,
    ssl_mode: sqlite ? null : values.ssl_mode || null,
    read_only: values.read_only ?? false,
    group_name: values.group_name || null,
    color: values.color || null,
    remark: values.remark || null,
  };
}

export function ConnectionModal({
  open,
  editing,
  groups,
  onClose,
  onSaved,
}: {
  open: boolean;
  editing?: DbConnection | null;
  /** Existing group names, offered in the group field. */
  groups: string[];
  onClose: () => void;
  onSaved: (connection: DbConnection) => void;
}) {
  const [form] = Form.useForm<FormValues>();
  const dbType = Form.useWatch("db_type", form) ?? "postgres";
  const [testResult, setTestResult] = useState<{
    ok: boolean;
    text: string;
  } | null>(null);

  useEffect(() => {
    if (!open) return;
    setTestResult(null);
    form.resetFields();
    if (editing) {
      form.setFieldsValue({
        name: editing.name,
        db_type: editing.db_type,
        host: editing.host ?? undefined,
        port: editing.port ?? undefined,
        username: editing.username ?? undefined,
        database_name: editing.database_name ?? undefined,
        ssl_mode: editing.ssl_mode ?? undefined,
        read_only: editing.read_only,
        group_name: editing.group_name ?? undefined,
        color: editing.color ?? undefined,
        remark: editing.remark ?? undefined,
      });
    } else {
      form.setFieldsValue({
        db_type: "postgres",
        host: "127.0.0.1",
        port: 5432,
        read_only: false,
      });
    }
  }, [open, editing, form]);

  const save = useMutation({
    mutationFn: (values: FormValues) =>
      editing
        ? updateDbConnection(editing.id, toParams(values))
        : createDbConnection(toParams(values)),
    onSuccess: (connection) => {
      message.success(editing ? t("连接已更新") : t("连接已创建"));
      onSaved(connection);
    },
    onError: (error) => message.error(error.message),
  });

  const test = useMutation({
    mutationFn: async () => {
      const values = await form.validateFields();
      return testDbConnection({ ...toParams(values), id: editing?.id });
    },
    onSuccess: (result) =>
      setTestResult({
        ok: true,
        text: t("连接成功：{0}（{1} ms）", result.version, result.duration_ms),
      }),
    onError: (error) => {
      if (error && "errorFields" in error) return;
      setTestResult({ ok: false, text: error.message });
    },
  });

  return (
    <Modal
      open={open}
      title={editing ? t("编辑连接") : t("新建连接")}
      width={620}
      onCancel={onClose}
      destroyOnClose
      footer={
        <div className="db-connection-footer">
          <Button
            icon={<ApiOutlined />}
            loading={test.isPending}
            onClick={() => test.mutate()}
          >
            {t("测试连接")}
          </Button>
          <Space>
            <Button onClick={onClose}>{t("取消")}</Button>
            <Button
              type="primary"
              loading={save.isPending}
              onClick={() =>
                void form.validateFields().then((values) => save.mutate(values))
              }
            >
              {t("保存")}
            </Button>
          </Space>
        </div>
      }
    >
      <Form<FormValues> form={form} layout="vertical" requiredMark={false}>
        <Form.Item name="db_type" label={t("数据库类型")}>
          <Radio.Group
            optionType="button"
            buttonStyle="solid"
            options={(Object.keys(DB_TYPE_LABELS) as DbType[]).map((value) => ({
              value,
              label: DB_TYPE_LABELS[value],
            }))}
            onChange={(event) => {
              const next = event.target.value as DbType;
              form.setFieldsValue({
                port: DEFAULT_PORTS[next],
                ssl_mode: undefined,
              });
            }}
          />
        </Form.Item>
        <Form.Item
          name="name"
          label={t("连接名称")}
          rules={[{ required: true, message: t("请输入连接名称") }]}
        >
          <Input maxLength={100} placeholder={t("例如：生产库（只读）")} />
        </Form.Item>
        {dbType === "sqlite" ? (
          <Form.Item
            name="database_name"
            label={t("数据库文件路径")}
            extra={t("服务器上的文件路径，相对路径从后端工作目录算起")}
            rules={[{ required: true, message: t("请输入数据库文件路径") }]}
          >
            <Input placeholder="data/app.sqlite" />
          </Form.Item>
        ) : (
          <>
            <Row gutter={12}>
              <Col xs={24} sm={16}>
                <Form.Item
                  name="host"
                  label={t("主机")}
                  rules={[{ required: true, message: t("请输入主机地址") }]}
                >
                  <Input placeholder="127.0.0.1" />
                </Form.Item>
              </Col>
              <Col xs={24} sm={8}>
                <Form.Item name="port" label={t("端口")}>
                  <InputNumber
                    min={1}
                    max={65535}
                    className="db-full-width"
                    placeholder={String(DEFAULT_PORTS[dbType] ?? "")}
                  />
                </Form.Item>
              </Col>
            </Row>
            <Row gutter={12}>
              <Col xs={24} sm={12}>
                <Form.Item
                  name="username"
                  label={t("用户名")}
                  rules={[{ required: true, message: t("请输入用户名") }]}
                >
                  <Input autoComplete="off" />
                </Form.Item>
              </Col>
              <Col xs={24} sm={12}>
                <Form.Item
                  name="password"
                  label={t("密码")}
                  extra={
                    editing?.password_saved
                      ? t("已保存密码，留空则保持不变")
                      : undefined
                  }
                >
                  <Input.Password autoComplete="new-password" />
                </Form.Item>
              </Col>
            </Row>
            {editing?.password_saved ? (
              <Form.Item name="clear_password" valuePropName="checked">
                <Checkbox>{t("清除已保存的密码")}</Checkbox>
              </Form.Item>
            ) : null}
            <Row gutter={12}>
              <Col xs={24} sm={12}>
                <Form.Item
                  name="database_name"
                  label={t("默认数据库")}
                  extra={
                    dbType === "postgres"
                      ? t("留空时连接 postgres 库")
                      : t("留空时不指定默认库")
                  }
                >
                  <Input />
                </Form.Item>
              </Col>
              <Col xs={24} sm={12}>
                <Form.Item name="ssl_mode" label={t("SSL 模式")}>
                  <Select
                    allowClear
                    placeholder={dbType === "postgres" ? "prefer" : "preferred"}
                    options={SSL_MODES[dbType]}
                  />
                </Form.Item>
              </Col>
            </Row>
          </>
        )}
        <Row gutter={12}>
          <Col xs={24} sm={12}>
            <Form.Item name="group_name" label={t("分组")}>
              <AutoComplete
                allowClear
                maxLength={100}
                placeholder={t("不分组")}
                options={groups.map((group) => ({ value: group }))}
                filterOption={(input, option) =>
                  String(option?.value ?? "")
                    .toLowerCase()
                    .includes(input.toLowerCase())
                }
              />
            </Form.Item>
          </Col>
          <Col xs={24} sm={12}>
            <Form.Item name="color" label={t("颜色")}>
              <Radio.Group className="db-color-options">
                <Radio.Button value="">{t("无")}</Radio.Button>
                {CONNECTION_COLORS.map((color) => (
                  <Radio.Button key={color} value={color}>
                    <span
                      className="db-color-dot"
                      style={{ background: color }}
                    />
                  </Radio.Button>
                ))}
              </Radio.Group>
            </Form.Item>
          </Col>
        </Row>
        <Form.Item
          name="read_only"
          label={t("只读连接")}
          valuePropName="checked"
          extra={t("由数据库拒绝所有写操作，适合连接生产环境")}
        >
          <Switch />
        </Form.Item>
        <Form.Item name="remark" label={t("备注")}>
          <Input.TextArea rows={2} maxLength={500} />
        </Form.Item>
      </Form>
      {testResult ? (
        <Alert
          showIcon
          type={testResult.ok ? "success" : "error"}
          message={testResult.text}
        />
      ) : null}
    </Modal>
  );
}
