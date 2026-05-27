import { LockOutlined, UserOutlined } from "@ant-design/icons";
import { Alert, Button, Card, Form, Input, Typography, message } from "antd";
import { useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { current, login } from "../../api/auth";
import { useAuthStore } from "../../stores/auth";

type LoginForm = {
  email: string;
  password: string;
};

type LocationState = {
  from?: {
    pathname?: string;
  };
};

export function LoginPage() {
  const [loading, setLoading] = useState(false);
  const signIn = useAuthStore((state) => state.signIn);
  const setSession = useAuthStore((state) => state.setSession);
  const navigate = useNavigate();
  const location = useLocation();
  const from =
    (location.state as LocationState | null)?.from?.pathname ??
    "/admin/dashboard";

  async function handleSubmit(values: LoginForm) {
    setLoading(true);
    try {
      const result = await login(values);
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
      });
      message.success("登录成功");
      navigate(from, { replace: true });
    } catch {
      message.error("登录失败，请确认后端已启动且账号密码正确");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="login-page">
      <Card className="login-card">
        <div className="login-brand">
          <div className="admin-brand-mark">G</div>
          <div>
            <Typography.Title level={3}>GPT Images Admin</Typography.Title>
            <Typography.Text type="secondary">专业后台管理模板</Typography.Text>
          </div>
        </div>
        <Alert
          type="info"
          showIcon
          message="当前登录接口已接入 access token、refresh token 与自动刷新。"
          className="login-tip"
        />
        <Form<LoginForm>
          layout="vertical"
          initialValues={{ email: "admin@example.com" }}
          onFinish={handleSubmit}
        >
          <Form.Item
            name="email"
            label="邮箱"
            rules={[{ required: true, message: "请输入邮箱" }]}
          >
            <Input prefix={<UserOutlined />} placeholder="admin@example.com" />
          </Form.Item>
          <Form.Item
            name="password"
            label="密码"
            rules={[{ required: true, message: "请输入密码" }]}
          >
            <Input.Password
              prefix={<LockOutlined />}
              placeholder="请输入密码"
            />
          </Form.Item>
          <Button
            type="primary"
            htmlType="submit"
            loading={loading}
            block
            size="large"
          >
            登录后台
          </Button>
        </Form>
      </Card>
    </div>
  );
}
