import {
  DesktopOutlined,
  LockOutlined,
  LogoutOutlined,
  UserOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Button,
  Card,
  Checkbox,
  Descriptions,
  Form,
  Input,
  List,
  Popconfirm,
  Space,
  Tag,
  Typography,
  message,
} from "antd";
import dayjs from "dayjs";
import { useNavigate } from "react-router-dom";
import {
  changePassword,
  fetchMySessions,
  fetchProfile,
  revokeMyOtherSessions,
  revokeMySession,
  updateProfile,
} from "../../api/profile";
import { t } from "../../i18n";
import { useAuthStore } from "../../stores/auth";
import { describeUserAgent } from "../../utils/userAgent";
import "./profile.css";

type PasswordForm = {
  current_password: string;
  new_password: string;
  confirm_password: string;
  sign_out_others: boolean;
};

const formatTime = (value?: string | null) =>
  value ? dayjs(value).format("YYYY-MM-DD HH:mm:ss") : "-";

export function ProfilePage() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const signOut = useAuthStore((state) => state.signOut);
  const user = useAuthStore((state) => state.user);
  const setUser = useAuthStore((state) => state.setUser);
  const [nameForm] = Form.useForm<{ name: string }>();
  const [passwordForm] = Form.useForm<PasswordForm>();

  const profileQuery = useQuery({
    queryKey: ["my-profile"],
    queryFn: fetchProfile,
  });
  const sessionsQuery = useQuery({
    queryKey: ["my-sessions"],
    queryFn: fetchMySessions,
  });
  const refreshSessions = () =>
    queryClient.invalidateQueries({ queryKey: ["my-sessions"] });

  const nameMutation = useMutation({
    mutationFn: (name: string) => updateProfile(name),
    onSuccess: (profile) => {
      message.success(t("资料已保存"));
      queryClient.setQueryData(["my-profile"], profile);
      if (user) setUser({ ...user, name: profile.name });
    },
  });
  const passwordMutation = useMutation({
    mutationFn: (values: PasswordForm) =>
      changePassword({
        current_password: values.current_password,
        new_password: values.new_password,
        sign_out_others: values.sign_out_others,
      }),
    onSuccess: () => {
      message.success(t("密码已修改"));
      passwordForm.resetFields();
      void queryClient.invalidateQueries({ queryKey: ["my-profile"] });
      void refreshSessions();
    },
  });
  const revokeMutation = useMutation({
    mutationFn: revokeMySession,
    onSuccess: (_, id) => {
      const wasCurrent = sessionsQuery.data?.find(
        (session) => session.id === id,
      )?.current;
      if (wasCurrent) {
        signOut();
        navigate("/login", { replace: true });
        return;
      }
      message.success(t("该设备已下线"));
      void refreshSessions();
    },
  });
  const revokeOthersMutation = useMutation({
    mutationFn: revokeMyOtherSessions,
    onSuccess: () => {
      message.success(t("其他设备已全部下线"));
      void refreshSessions();
    },
  });

  const profile = profileQuery.data;
  const sessions = sessionsQuery.data ?? [];

  return (
    <div className="profile-page">
      <h1 className="visually-hidden">{t("个人中心")}</h1>
      <div className="profile-grid">
        <Card
          className="admin-card"
          title={
            <Space>
              <UserOutlined />
              {t("基本资料")}
            </Space>
          }
          loading={profileQuery.isLoading}
        >
          {profile ? (
            <>
              <Descriptions column={1} size="small" className="profile-facts">
                <Descriptions.Item label={t("邮箱")}>
                  {profile.email}{" "}
                  {profile.is_verified ? (
                    <Tag color="success">{t("已验证")}</Tag>
                  ) : (
                    <Tag>{t("未验证")}</Tag>
                  )}
                </Descriptions.Item>
                <Descriptions.Item label={t("最近登录")}>
                  {formatTime(profile.last_login_at)}
                  {profile.last_login_ip ? `（${profile.last_login_ip}）` : ""}
                </Descriptions.Item>
                <Descriptions.Item label={t("密码修改于")}>
                  {formatTime(profile.password_changed_at)}
                </Descriptions.Item>
                <Descriptions.Item label={t("注册时间")}>
                  {formatTime(profile.created_at)}
                </Descriptions.Item>
              </Descriptions>
              <Form
                form={nameForm}
                layout="inline"
                className="profile-name-form"
                initialValues={{ name: profile.name }}
                onFinish={(values) => nameMutation.mutate(values.name.trim())}
              >
                <Form.Item
                  name="name"
                  label={t("姓名")}
                  rules={[
                    {
                      required: true,
                      whitespace: true,
                      message: t("请输入姓名"),
                    },
                    { max: 64 },
                  ]}
                >
                  <Input />
                </Form.Item>
                <Form.Item>
                  <Button
                    type="primary"
                    htmlType="submit"
                    loading={nameMutation.isPending}
                  >
                    {t("保存")}
                  </Button>
                </Form.Item>
              </Form>
            </>
          ) : null}
        </Card>

        <Card
          className="admin-card"
          title={
            <Space>
              <LockOutlined />
              {t("修改密码")}
            </Space>
          }
        >
          <Form<PasswordForm>
            form={passwordForm}
            layout="vertical"
            initialValues={{ sign_out_others: true }}
            onFinish={(values) => passwordMutation.mutate(values)}
          >
            <Form.Item
              name="current_password"
              label={t("当前密码")}
              rules={[{ required: true, message: t("请输入当前密码") }]}
            >
              <Input.Password autoComplete="current-password" />
            </Form.Item>
            <Form.Item
              name="new_password"
              label={t("新密码")}
              extra={t("至少 8 位，需同时包含字母和数字")}
              rules={[
                { required: true, message: t("请输入新密码") },
                { min: 8, message: t("至少 8 位") },
                {
                  pattern: /(?=.*[A-Za-zÀ-￿])(?=.*\d)/,
                  message: t("需同时包含字母和数字"),
                },
              ]}
            >
              <Input.Password autoComplete="new-password" />
            </Form.Item>
            <Form.Item
              name="confirm_password"
              label={t("确认新密码")}
              dependencies={["new_password"]}
              rules={[
                { required: true, message: t("请再次输入新密码") },
                ({ getFieldValue }) => ({
                  validator: (_, value) =>
                    !value || value === getFieldValue("new_password")
                      ? Promise.resolve()
                      : Promise.reject(new Error(t("两次输入的密码不一致"))),
                }),
              ]}
            >
              <Input.Password autoComplete="new-password" />
            </Form.Item>
            <Form.Item name="sign_out_others" valuePropName="checked">
              <Checkbox>{t("同时让其他设备退出登录")}</Checkbox>
            </Form.Item>
            <Button
              type="primary"
              htmlType="submit"
              loading={passwordMutation.isPending}
            >
              {t("修改密码")}
            </Button>
          </Form>
        </Card>
      </div>

      <Card
        className="admin-card profile-sessions"
        title={
          <Space>
            <DesktopOutlined />
            {t("登录设备")}
          </Space>
        }
        extra={
          <Popconfirm
            title={t("让除当前设备外的所有设备退出登录？")}
            okText={t("确定")}
            cancelText={t("取消")}
            onConfirm={() => revokeOthersMutation.mutate()}
            disabled={
              sessions.filter((session) => !session.current).length === 0
            }
          >
            <Button
              icon={<LogoutOutlined />}
              loading={revokeOthersMutation.isPending}
              disabled={
                sessions.filter((session) => !session.current).length === 0
              }
            >
              {t("下线其他设备")}
            </Button>
          </Popconfirm>
        }
      >
        <List
          loading={sessionsQuery.isLoading}
          dataSource={sessions}
          locale={{ emptyText: t("没有登录中的设备") }}
          renderItem={(session) => (
            <List.Item
              actions={[
                <Popconfirm
                  key="revoke"
                  title={
                    session.current
                      ? t("退出当前设备的登录？")
                      : t("让这台设备退出登录？")
                  }
                  okText={t("确定")}
                  cancelText={t("取消")}
                  onConfirm={() => revokeMutation.mutate(session.id)}
                >
                  <Button size="small" danger>
                    {session.current ? t("退出登录") : t("下线")}
                  </Button>
                </Popconfirm>,
              ]}
            >
              <List.Item.Meta
                title={
                  <Space wrap>
                    <span>
                      {describeUserAgent(session.user_agent) || t("未知设备")}
                    </span>
                    {session.current ? (
                      <Tag color="processing">{t("当前设备")}</Tag>
                    ) : null}
                  </Space>
                }
                description={
                  <Typography.Text type="secondary">
                    {t(
                      "IP {0} · 登录于 {1} · 最近活动 {2}",
                      session.ip || "-",
                      formatTime(session.created_at),
                      formatTime(session.last_seen_at),
                    )}
                  </Typography.Text>
                }
              />
            </List.Item>
          )}
        />
      </Card>
    </div>
  );
}
