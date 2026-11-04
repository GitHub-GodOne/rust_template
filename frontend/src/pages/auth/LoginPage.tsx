import { LockOutlined, SafetyOutlined, UserOutlined } from "@ant-design/icons";
import { Alert, Button, Card, Form, Input, Typography, message } from "antd";
import { useCallback, useEffect, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import {
  type CaptchaChallenge,
  current,
  fetchCaptcha,
  login,
} from "../../api/auth";
import { ApiRequestError } from "../../api/client";
import { LanguageSwitch } from "../../components/LanguageSwitch";
import { t } from "../../i18n";
import { ThemeSwitch } from "../../layouts/ThemeSwitch";
import { useAuthStore } from "../../stores/auth";

type LoginForm = {
  email: string;
  password: string;
  captcha_code?: string;
};

const CAPTCHA_CODES = ["CAPTCHA_REQUIRED", "CAPTCHA_INVALID"];

type LocationState = {
  from?: {
    pathname?: string;
  };
};

export function LoginPage() {
  const [loading, setLoading] = useState(false);
  const [loginError, setLoginError] = useState<string>();
  const [captcha, setCaptcha] = useState<CaptchaChallenge | null>(null);
  const [form] = Form.useForm<LoginForm>();

  const loadCaptcha = useCallback(
    async (force: boolean) => {
      try {
        const challenge = await fetchCaptcha();
        // Shown when the server asks for it up front, or after a rejection.
        setCaptcha((current) =>
          force || challenge.required || current ? challenge : null,
        );
        form.setFieldValue("captcha_code", undefined);
      } catch {
        // The form still works; the server will say if a captcha is needed.
      }
    },
    [form],
  );

  useEffect(() => {
    void loadCaptcha(false);
  }, [loadCaptcha]);
  const signIn = useAuthStore((state) => state.signIn);
  const setSession = useAuthStore((state) => state.setSession);
  const navigate = useNavigate();
  const location = useLocation();
  const from =
    (location.state as LocationState | null)?.from?.pathname ??
    "/admin/dashboard";

  async function handleSubmit(values: LoginForm) {
    setLoading(true);
    setLoginError(undefined);
    try {
      const result = await login({
        email: values.email,
        password: values.password,
        captcha_id: captcha?.captcha_id,
        captcha_code: values.captcha_code,
      });
      signIn({
        token: result.token,
        refreshToken: result.refresh_token,
        user: {
          pid: result.pid,
          name: result.name,
          email: values.email,
          isVerified: result.is_verified,
        },
      });
      const session = await current();
      setSession({
        user: {
          pid: session.pid,
          name: session.name,
          email: session.email,
          isVerified: result.is_verified,
        },
        roles: session.roles,
        permissions: session.permissions,
        menus: session.menus,
        tenant: session.tenant ?? null,
        departments: session.departments,
        currentDepartment: session.current_department ?? null,
        dataScopes: session.data_scopes,
        effectiveDataScope: session.effective_data_scope,
      });
      message.success(t("登录成功"));
      navigate(from, { replace: true });
    } catch (error) {
      const backendMessage = error instanceof Error ? error.message.trim() : "";
      const errorMessage =
        !backendMessage || backendMessage === "Network Error"
          ? t("无法连接后端服务，请先在项目目录运行 cargo loco start")
          : backendMessage;
      setLoginError(errorMessage);
      message.error(errorMessage);
      // Every attempt uses up the captcha; failures may start requiring one.
      const code = error instanceof ApiRequestError ? error.code : undefined;
      if (
        captcha ||
        (code && CAPTCHA_CODES.includes(code)) ||
        code === "UNAUTHORIZED"
      ) {
        void loadCaptcha(Boolean(code && CAPTCHA_CODES.includes(code)));
      }
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="login-page">
      <div className="page-language-switch">
        <ThemeSwitch />
        <LanguageSwitch />
      </div>
      <Card className="login-card">
        <div className="login-brand">
          <div className="admin-brand-mark">G</div>
          <div>
            <Typography.Title level={3}>{t("后台管理 Admin")}</Typography.Title>
            <Typography.Text type="secondary">
              {t("专业后台管理模板")}
            </Typography.Text>
          </div>
        </div>
        <Form<LoginForm>
          form={form}
          layout="vertical"
          initialValues={{ email: "admin@example.com" }}
          onFinish={handleSubmit}
          onValuesChange={() => setLoginError(undefined)}
        >
          {loginError ? (
            <Alert
              className="login-error-alert"
              type="error"
              showIcon
              message={loginError}
            />
          ) : null}
          <Form.Item
            name="email"
            label={t("邮箱")}
            rules={[{ required: true, message: t("请输入邮箱") }]}
          >
            <Input prefix={<UserOutlined />} placeholder="admin@example.com" />
          </Form.Item>
          <Form.Item
            name="password"
            label={t("密码")}
            rules={[{ required: true, message: t("请输入密码") }]}
          >
            <Input.Password
              prefix={<LockOutlined />}
              placeholder={t("请输入密码")}
            />
          </Form.Item>
          {captcha ? (
            <Form.Item
              label={t("验证码")}
              required
              className="login-captcha-item"
            >
              <div className="login-captcha">
                <Form.Item
                  name="captcha_code"
                  noStyle
                  rules={[{ required: true, message: t("请输入验证码") }]}
                >
                  <Input
                    prefix={<SafetyOutlined />}
                    placeholder={t("请输入右侧字符")}
                    maxLength={8}
                    autoComplete="off"
                  />
                </Form.Item>
                <button
                  type="button"
                  className="login-captcha-image"
                  title={t("看不清？点击换一张")}
                  aria-label={t("看不清？点击换一张")}
                  onClick={() => void loadCaptcha(true)}
                >
                  <img src={captcha.image} alt={t("验证码")} />
                </button>
              </div>
            </Form.Item>
          ) : null}
          <Button
            type="primary"
            htmlType="submit"
            loading={loading}
            block
            size="large"
          >
            {t("登录后台")}
          </Button>
        </Form>
      </Card>
    </div>
  );
}
