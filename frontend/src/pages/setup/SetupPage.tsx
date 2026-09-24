import {
  CheckCircleFilled,
  DatabaseOutlined,
  LockOutlined,
  MailOutlined,
  PlayCircleOutlined,
  ReloadOutlined,
  SafetyCertificateOutlined,
  UserOutlined,
} from "@ant-design/icons";
import {
  Alert,
  Button,
  Form,
  Input,
  InputNumber,
  Select,
  Spin,
  Tag,
  Typography,
  message,
} from "antd";
import { useEffect, useState } from "react";
import {
  type DatabaseTestResult,
  type InstallResult,
  type SetupDatabase,
  type SetupStatus,
  completeSetup,
  getSetupStatus,
  testSetupDatabase,
} from "../../api/setup";

type SetupFormValues = {
  engine: "postgres";
  host: string;
  port: number;
  username: string;
  password: string;
  database: string;
  ssl_mode: "disable" | "prefer" | "require" | "verify-ca" | "verify-full";
  admin_name: string;
  admin_email: string;
  admin_password: string;
  confirm_password: string;
};

const steps = [
  { title: "数据库", description: "连接与存储" },
  { title: "管理员", description: "首个系统账户" },
  { title: "完成", description: "迁移并启动" },
];

function databaseFromValues(values: SetupFormValues): SetupDatabase {
  return {
    engine: "postgres",
    host: values.host.trim(),
    port: Number(values.port),
    username: values.username.trim(),
    password: values.password,
    database: values.database.trim(),
    ssl_mode: values.ssl_mode,
  };
}

function databaseLabel(values: SetupDatabase): string {
  return `${values.username}@${values.host}:${values.port}/${values.database}`;
}

export function SetupPage() {
  const [form] = Form.useForm<SetupFormValues>();
  const [activeStep, setActiveStep] = useState(0);
  const [status, setStatus] = useState<SetupStatus>();
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<DatabaseTestResult>();
  const [testedDatabase, setTestedDatabase] = useState<SetupDatabase>();
  const [testError, setTestError] = useState<string>();
  const [installing, setInstalling] = useState(false);
  const [installResult, setInstallResult] = useState<InstallResult>();
  const [applicationReady, setApplicationReady] = useState(false);
  const [restartError, setRestartError] = useState(false);

  useEffect(() => {
    getSetupStatus()
      .then(setStatus)
      .catch(() => undefined);
  }, []);

  const databaseSummary = testedDatabase
    ? databaseLabel(testedDatabase)
    : "尚未验证数据库";

  function invalidateConnectionTest(changed: Partial<SetupFormValues>) {
    const databaseFields = new Set([
      "engine",
      "host",
      "port",
      "username",
      "password",
      "database",
      "ssl_mode",
    ]);
    if (Object.keys(changed).some((field) => databaseFields.has(field))) {
      setTestResult(undefined);
      setTestedDatabase(undefined);
      setTestError(undefined);
    }
  }

  async function handleTest() {
    setTesting(true);
    setTestResult(undefined);
    setTestedDatabase(undefined);
    setTestError(undefined);
    try {
      const fieldNames = [
        "engine",
        "host",
        "port",
        "username",
        "password",
        "database",
        "ssl_mode",
      ] as const;
      const databaseValues = await form.validateFields([...fieldNames]);
      const database = databaseFromValues(databaseValues as SetupFormValues);
      const result = await testSetupDatabase(database);
      setTestResult(result);
      setTestedDatabase(database);
      message.success("数据库连接成功");
    } catch (error) {
      if (error instanceof Error) {
        setTestError(error.message);
      }
    } finally {
      setTesting(false);
    }
  }

  async function handleDatabaseNext() {
    if (!testResult || !testedDatabase) {
      setTestError("请先测试当前数据库配置");
      return;
    }
    setActiveStep(1);
  }

  async function handleInstall() {
    if (!testResult || !testedDatabase) {
      setActiveStep(0);
      setTestError("数据库配置已变化，请重新测试连接");
      return;
    }
    setInstalling(true);
    try {
      const formValues = await form.validateFields([
        "admin_name",
        "admin_email",
        "admin_password",
        "confirm_password",
      ]);
      const result = await completeSetup({
        database: testedDatabase,
        admin: {
          name: formValues.admin_name.trim(),
          email: formValues.admin_email.trim(),
          password: formValues.admin_password,
        },
      });
      setInstallResult(result);
      setActiveStep(2);
      await waitForApplication();
    } catch (error) {
      message.error(error instanceof Error ? error.message : "安装失败");
    } finally {
      setInstalling(false);
    }
  }

  async function waitForApplication() {
    setRestartError(false);
    for (let attempt = 0; attempt < 40; attempt += 1) {
      await new Promise((resolve) => window.setTimeout(resolve, 750));
      try {
        const nextStatus = await getSetupStatus();
        if (nextStatus.configured && nextStatus.phase === "ready") {
          setApplicationReady(true);
          window.setTimeout(() => window.location.replace("/login"), 900);
          return;
        }
      } catch {
        // A short connection gap is expected while the setup server hands over.
      }
    }
    setRestartError(true);
  }

  return (
    <div className="setup-page">
      <aside className="setup-rail">
        <div className="setup-brand">
          <span className="setup-brand-mark">G</span>
          <span>
            <strong>后台管理</strong>
            <small>首次安装</small>
          </span>
        </div>

        <ol className="setup-steps" aria-label="安装步骤">
          {steps.map((step, index) => {
            const complete = index < activeStep;
            const active = index === activeStep;
            return (
              <li
                className={`${active ? "is-active" : ""} ${complete ? "is-complete" : ""}`}
                key={step.title}
              >
                <span className="setup-step-index">
                  {complete ? <CheckCircleFilled /> : index + 1}
                </span>
                <span>
                  <strong>{step.title}</strong>
                  <small>{step.description}</small>
                </span>
              </li>
            );
          })}
        </ol>

        <div className="setup-runtime">
          <span>运行环境</span>
          <strong>{status?.environment ?? "development"}</strong>
          {status?.config_file ? <small>{status.config_file}</small> : null}
        </div>
      </aside>

      <main className="setup-main">
        <header className="setup-mobile-header">
          <div className="setup-brand">
            <span className="setup-brand-mark">G</span>
            <strong>后台管理</strong>
          </div>
          <span className="setup-mobile-step">
            {activeStep + 1} / {steps.length}
          </span>
        </header>

        <div className="setup-workspace">
          <div className="setup-eyebrow">
            <span>系统初始化</span>
            <span className="setup-eyebrow-line" />
            <span>{steps[activeStep].title}</span>
          </div>

          <Form<SetupFormValues>
            className="setup-form"
            form={form}
            layout="vertical"
            initialValues={{
              engine: "postgres",
              host: "127.0.0.1",
              port: 5432,
              username: "postgres",
              password: "",
              database: "gpt_images",
              ssl_mode: "prefer",
              admin_name: "Admin",
              admin_email: "admin@example.com",
            }}
            onValuesChange={invalidateConnectionTest}
          >
            {activeStep === 0 ? (
              <section className="setup-panel setup-panel-enter" key="database">
                <div className="setup-heading">
                  <DatabaseOutlined />
                  <div>
                    <Typography.Title level={1}>连接数据库</Typography.Title>
                    <Typography.Paragraph>
                      测试通过后，安装程序会创建数据库和系统表。
                    </Typography.Paragraph>
                  </div>
                </div>

                <div className="setup-engine-label">
                  <DatabaseOutlined />
                  <span>
                    <small>数据库引擎</small>
                    <strong>PostgreSQL</strong>
                  </span>
                </div>

                <div className="setup-field-grid">
                  <Form.Item
                    className="setup-field-wide"
                    name="host"
                    label="主机地址"
                    rules={[{ required: true, message: "请输入数据库主机" }]}
                  >
                    <Input placeholder="127.0.0.1" />
                  </Form.Item>
                  <Form.Item
                    name="port"
                    label="端口"
                    rules={[{ required: true, message: "请输入端口" }]}
                  >
                    <InputNumber min={1} max={65535} controls={false} />
                  </Form.Item>
                  <Form.Item
                    name="username"
                    label="用户名"
                    rules={[{ required: true, message: "请输入用户名" }]}
                  >
                    <Input autoComplete="username" placeholder="postgres" />
                  </Form.Item>
                  <Form.Item name="password" label="密码">
                    <Input.Password
                      autoComplete="current-password"
                      placeholder="数据库密码"
                    />
                  </Form.Item>
                  <Form.Item
                    name="database"
                    label="数据库名"
                    rules={[
                      { required: true, message: "请输入数据库名" },
                      {
                        pattern: /^[A-Za-z_][A-Za-z0-9_]{0,62}$/,
                        message: "仅支持字母、数字和下划线",
                      },
                    ]}
                  >
                    <Input placeholder="gpt_images" />
                  </Form.Item>
                  <Form.Item name="ssl_mode" label="SSL 模式">
                    <Select
                      options={[
                        { label: "Prefer", value: "prefer" },
                        { label: "Disable", value: "disable" },
                        { label: "Require", value: "require" },
                        { label: "Verify CA", value: "verify-ca" },
                        { label: "Verify Full", value: "verify-full" },
                      ]}
                    />
                  </Form.Item>
                </div>

                {testResult ? (
                  <div className="setup-connection-result is-success">
                    <CheckCircleFilled />
                    <span>
                      <strong>连接正常</strong>
                      <small>
                        {testResult.database_exists
                          ? testResult.initialized
                            ? "检测到已有系统表，将保留现有数据"
                            : "数据库已存在，将创建系统表"
                          : "数据库尚不存在，安装时会自动创建"}
                      </small>
                    </span>
                  </div>
                ) : null}
                {testError ? (
                  <Alert showIcon type="error" message={testError} />
                ) : null}

                <div className="setup-actions">
                  <Button
                    icon={<PlayCircleOutlined />}
                    loading={testing}
                    onClick={handleTest}
                    size="large"
                  >
                    测试连接
                  </Button>
                  <Button
                    type="primary"
                    size="large"
                    disabled={!testResult}
                    onClick={handleDatabaseNext}
                  >
                    继续
                  </Button>
                </div>
              </section>
            ) : null}

            {activeStep === 1 ? (
              <section className="setup-panel setup-panel-enter" key="admin">
                <div className="setup-heading">
                  <SafetyCertificateOutlined />
                  <div>
                    <Typography.Title level={1}>创建管理员</Typography.Title>
                    <Typography.Paragraph>
                      此账户拥有完整系统权限，请使用独立的安全密码。
                    </Typography.Paragraph>
                  </div>
                </div>

                <div className="setup-database-summary">
                  <DatabaseOutlined />
                  <span>
                    <small>目标数据库</small>
                    <strong>{databaseSummary}</strong>
                  </span>
                  <Tag color="success">已验证</Tag>
                </div>

                <div className="setup-field-grid setup-admin-fields">
                  <Form.Item
                    name="admin_name"
                    label="管理员名称"
                    rules={[
                      { required: true, message: "请输入管理员名称" },
                      { min: 2, max: 80, message: "请输入 2 到 80 个字符" },
                    ]}
                  >
                    <Input prefix={<UserOutlined />} autoComplete="name" />
                  </Form.Item>
                  <Form.Item
                    name="admin_email"
                    label="登录邮箱"
                    rules={[
                      { required: true, message: "请输入登录邮箱" },
                      { type: "email", message: "邮箱格式不正确" },
                    ]}
                  >
                    <Input prefix={<MailOutlined />} autoComplete="email" />
                  </Form.Item>
                  <Form.Item
                    name="admin_password"
                    label="登录密码"
                    rules={[
                      { required: true, message: "请输入登录密码" },
                      { min: 10, message: "密码至少需要 10 个字符" },
                    ]}
                  >
                    <Input.Password
                      prefix={<LockOutlined />}
                      autoComplete="new-password"
                    />
                  </Form.Item>
                  <Form.Item
                    name="confirm_password"
                    label="确认密码"
                    dependencies={["admin_password"]}
                    rules={[
                      { required: true, message: "请再次输入密码" },
                      ({ getFieldValue }) => ({
                        validator(_, value) {
                          if (
                            !value ||
                            getFieldValue("admin_password") === value
                          ) {
                            return Promise.resolve();
                          }
                          return Promise.reject(
                            new Error("两次输入的密码不一致"),
                          );
                        },
                      }),
                    ]}
                  >
                    <Input.Password
                      prefix={<LockOutlined />}
                      autoComplete="new-password"
                    />
                  </Form.Item>
                </div>

                <Alert
                  className="setup-install-note"
                  type="info"
                  showIcon
                  message="安装过程中会执行数据库迁移并写入本地配置，已有数据库内容不会被清空。"
                />

                <div className="setup-actions">
                  <Button size="large" onClick={() => setActiveStep(0)}>
                    返回
                  </Button>
                  <Button
                    type="primary"
                    size="large"
                    loading={installing}
                    onClick={handleInstall}
                  >
                    创建并启动
                  </Button>
                </div>
              </section>
            ) : null}

            {activeStep === 2 ? (
              <section
                className="setup-panel setup-finish setup-panel-enter"
                key="finish"
              >
                <div className="setup-success-mark">
                  <CheckCircleFilled />
                </div>
                <Typography.Title level={1}>系统已完成初始化</Typography.Title>
                <Typography.Paragraph>
                  配置和数据库迁移已完成，正在切换到正式服务。
                </Typography.Paragraph>

                <div className="setup-result-list">
                  <div>
                    <span>配置文件</span>
                    <strong>{installResult?.config_file}</strong>
                  </div>
                  <div>
                    <span>数据库迁移</span>
                    <strong>已完成</strong>
                  </div>
                  <div>
                    <span>管理员账户</span>
                    <strong>
                      {installResult?.admin_created ? "已创建" : "保留已有账户"}
                    </strong>
                  </div>
                </div>

                <div className="setup-restart-state">
                  {applicationReady ? (
                    <>
                      <CheckCircleFilled />
                      <span>正式服务已就绪，正在进入登录页</span>
                    </>
                  ) : (
                    <>
                      <Spin size="small" />
                      <span>正在启动正式服务</span>
                    </>
                  )}
                </div>

                {restartError ? (
                  <Alert
                    showIcon
                    type="warning"
                    message="正式服务尚未恢复，请检查后端启动日志后重新检测。"
                    action={
                      <Button
                        icon={<ReloadOutlined />}
                        size="small"
                        onClick={waitForApplication}
                      >
                        重新检测
                      </Button>
                    }
                  />
                ) : null}
              </section>
            ) : null}
          </Form>
        </div>
      </main>
    </div>
  );
}
