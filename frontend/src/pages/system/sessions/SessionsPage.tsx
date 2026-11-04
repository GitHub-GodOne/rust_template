import {
  LaptopOutlined,
  LogoutOutlined,
  ReloadOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Button,
  Input,
  Popconfirm,
  Segmented,
  Select,
  Space,
  Tag,
  Typography,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import dayjs from "dayjs";
import { type ReactNode, useState } from "react";
import {
  fetchSessions,
  revokeSession,
  revokeUserSessions,
} from "../../../api/admin/sessions";
import type { SessionRecord } from "../../../api/profile";
import { CrudPage } from "../../../components/admin/CrudPage";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { t } from "../../../i18n";
import { useAuthStore } from "../../../stores/auth";
import { describeUserAgent } from "../../../utils/userAgent";
import { RemoteSessionsView } from "./RemoteSessionsView";

const formatTime = (value?: string | null) =>
  value ? dayjs(value).format("YYYY-MM-DD HH:mm:ss") : "-";

const reasonLabel = (reason?: string | null) => {
  switch (reason) {
    case "logout":
      return t("主动退出");
    case "admin":
      return t("管理员下线");
    case "self":
      return t("本人下线");
    case "disabled":
      return t("账号被禁用");
    case "deleted":
      return t("账号被删除");
    case "password_changed":
      return t("修改密码");
    case "password_reset":
      return t("密码被重置");
    case "reauth_failed":
      return t("确认密码失败");
    default:
      return reason ?? t("已过期");
  }
};

type SessionsTab = "login" | "remote";

/** 在线会话: login sessions, plus every user's SSH / VNC / RDP connections. */
export function SessionsPage() {
  const [tab, setTab] = useState<SessionsTab>("login");
  const canSeeRemote = useAuthStore((state) =>
    state.hasPermission("system:session:remote_list"),
  );
  const switcher = canSeeRemote ? (
    <Segmented<SessionsTab>
      value={tab}
      onChange={setTab}
      options={[
        { value: "login", label: t("登录会话") },
        { value: "remote", label: t("远程连接") },
      ]}
    />
  ) : null;

  return canSeeRemote && tab === "remote" ? (
    <RemoteSessionsView switcher={switcher} />
  ) : (
    <LoginSessionsView switcher={switcher} />
  );
}

function LoginSessionsView({ switcher }: { switcher: ReactNode }) {
  const [page, setPage] = useState(1);
  const [status, setStatus] = useState("active");
  const [keyword, setKeyword] = useState("");
  const [search, setSearch] = useState("");
  const queryClient = useQueryClient();

  const sessionsQuery = useQuery({
    queryKey: ["admin-sessions", page, status, search],
    queryFn: () =>
      fetchSessions({
        page,
        page_size: 20,
        status,
        keyword: search || undefined,
      }),
  });
  const refresh = () =>
    queryClient.invalidateQueries({ queryKey: ["admin-sessions"] });
  const revokeMutation = useMutation({
    mutationFn: revokeSession,
    onSuccess: () => {
      message.success(t("会话已下线"));
      void refresh();
    },
  });
  const revokeUserMutation = useMutation({
    mutationFn: revokeUserSessions,
    onSuccess: (count) => {
      message.success(t("已下线 {0} 个会话", count));
      void refresh();
    },
  });

  const columns: ColumnsType<SessionRecord> = [
    {
      title: t("用户"),
      key: "user",
      render: (_, record) => (
        <Space direction="vertical" size={0}>
          <span>
            {record.user_name || "-"}{" "}
            {record.current ? (
              <Tag color="processing">{t("当前会话")}</Tag>
            ) : null}
          </span>
          <Typography.Text type="secondary">
            {record.user_email}
          </Typography.Text>
        </Space>
      ),
    },
    {
      title: t("设备"),
      dataIndex: "user_agent",
      render: (value: string | null) => (
        <Typography.Text title={value ?? undefined}>
          {describeUserAgent(value) || t("未知设备")}
        </Typography.Text>
      ),
    },
    {
      title: "IP",
      dataIndex: "ip",
      width: 150,
      render: (value) => value || "-",
    },
    {
      title: t("登录时间"),
      dataIndex: "created_at",
      width: 180,
      render: formatTime,
    },
    {
      title: t("最近活动"),
      dataIndex: "last_seen_at",
      width: 180,
      render: formatTime,
    },
    {
      title: t("状态"),
      key: "status",
      width: 140,
      render: (_, record) =>
        record.active ? (
          <Tag color="success">{t("在线")}</Tag>
        ) : (
          <Tag>{reasonLabel(record.revoked_reason)}</Tag>
        ),
    },
    {
      title: t("操作"),
      key: "actions",
      width: 200,
      render: (_, record) =>
        record.active ? (
          <Space>
            <Popconfirm
              title={t("让这个会话立即下线？")}
              okText={t("确定")}
              cancelText={t("取消")}
              onConfirm={() => revokeMutation.mutate(record.id)}
            >
              <PermissionButton
                size="small"
                danger
                permission="system:session:revoke"
              >
                {t("下线")}
              </PermissionButton>
            </Popconfirm>
            <Popconfirm
              title={t("让该用户的所有会话下线？")}
              okText={t("确定")}
              cancelText={t("取消")}
              onConfirm={() => revokeUserMutation.mutate(record.user_id)}
            >
              <PermissionButton
                size="small"
                icon={<LogoutOutlined />}
                permission="system:session:revoke"
              >
                {t("全部下线")}
              </PermissionButton>
            </Popconfirm>
          </Space>
        ) : (
          <Typography.Text type="secondary">
            {formatTime(record.revoked_at ?? record.expires_at)}
          </Typography.Text>
        ),
    },
  ];

  return (
    <CrudPage
      title={t("在线会话")}
      icon={<LaptopOutlined />}
      toolbar={
        <Space wrap>
          {switcher}
          <Select
            value={status}
            className="sessions-status-select"
            options={[
              { value: "active", label: t("在线") },
              { value: "ended", label: t("已结束") },
              { value: "all", label: t("全部") },
            ]}
            onChange={(value) => {
              setStatus(value);
              setPage(1);
            }}
          />
          <Input.Search
            allowClear
            value={keyword}
            placeholder={t("用户、邮箱或 IP")}
            onChange={(event) => setKeyword(event.target.value)}
            onSearch={(value) => {
              setSearch(value.trim());
              setPage(1);
            }}
          />
          <Button
            icon={<ReloadOutlined />}
            loading={sessionsQuery.isFetching}
            onClick={() => void sessionsQuery.refetch()}
          >
            {t("刷新")}
          </Button>
        </Space>
      }
    >
      <DataTable<SessionRecord>
        rowKey="id"
        columns={columns}
        dataSource={sessionsQuery.data?.items ?? []}
        loading={sessionsQuery.isLoading}
        pagination={{
          current: page,
          pageSize: 20,
          total: sessionsQuery.data?.total ?? 0,
          onChange: setPage,
        }}
      />
    </CrudPage>
  );
}
