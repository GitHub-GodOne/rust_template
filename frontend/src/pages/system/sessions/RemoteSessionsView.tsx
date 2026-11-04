import { DisconnectOutlined, ReloadOutlined } from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Button,
  Input,
  Popconfirm,
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
  type RemoteSessionKind,
  type RemoteSessionRecord,
  closeRemoteSession,
  fetchRemoteSessions,
} from "../../../api/admin/sessions";
import { CrudPage } from "../../../components/admin/CrudPage";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { t } from "../../../i18n";

const formatTime = (value?: string | null) =>
  value ? dayjs(value).format("YYYY-MM-DD HH:mm:ss") : "-";

const KIND_COLORS: Record<RemoteSessionKind, string> = {
  ssh: "geekblue",
  vnc: "purple",
  rdp: "cyan",
  audio: "orange",
};

const kindLabel = (kind: RemoteSessionKind) =>
  kind === "audio" ? t("音频") : kind.toUpperCase();

const statusTag = (status: string) => {
  switch (status) {
    case "running":
    case "connected":
      return <Tag color="success">{t("使用中")}</Tag>;
    case "ready":
      return <Tag>{t("空闲")}</Tag>;
    case "error":
      return <Tag color="error">{t("异常")}</Tag>;
    case "closed":
      return <Tag>{t("已关闭")}</Tag>;
    default:
      return <Tag>{status}</Tag>;
  }
};

/** Every user's SSH / VNC / RDP sessions; users only see their own elsewhere. */
export function RemoteSessionsView({ switcher }: { switcher: ReactNode }) {
  const [kind, setKind] = useState<RemoteSessionKind | "all">("all");
  const [keyword, setKeyword] = useState("");
  const [search, setSearch] = useState("");
  const queryClient = useQueryClient();

  const sessionsQuery = useQuery({
    queryKey: ["admin-remote-sessions", kind, search],
    queryFn: () =>
      fetchRemoteSessions({
        kind: kind === "all" ? undefined : kind,
        keyword: search || undefined,
      }),
    refetchInterval: 10_000,
  });
  const closeMutation = useMutation({
    mutationFn: (record: RemoteSessionRecord) =>
      closeRemoteSession(record.kind, record.id),
    onSuccess: () => {
      message.success(t("连接已断开"));
      void queryClient.invalidateQueries({
        queryKey: ["admin-remote-sessions"],
      });
    },
  });

  const columns: ColumnsType<RemoteSessionRecord> = [
    {
      title: t("类型"),
      dataIndex: "kind",
      width: 90,
      render: (value: RemoteSessionKind) => (
        <Tag color={KIND_COLORS[value]}>{kindLabel(value)}</Tag>
      ),
    },
    {
      title: t("用户"),
      key: "user",
      render: (_, record) => (
        <Space direction="vertical" size={0}>
          <span>{record.user_name || `#${record.user_id}`}</span>
          <Typography.Text type="secondary">
            {record.user_email}
          </Typography.Text>
        </Space>
      ),
    },
    {
      title: t("目标"),
      key: "target",
      render: (_, record) => (
        <Space direction="vertical" size={0}>
          <span>{record.target_name}</span>
          <Typography.Text type="secondary">
            {record.address || t("本机")}
          </Typography.Text>
        </Space>
      ),
    },
    {
      title: t("状态"),
      dataIndex: "status",
      width: 110,
      render: statusTag,
    },
    {
      title: t("打开的窗口"),
      dataIndex: "viewers",
      width: 110,
    },
    {
      title: t("创建时间"),
      dataIndex: "created_at",
      width: 180,
      render: formatTime,
    },
    {
      title: t("最近活动"),
      dataIndex: "last_active_at",
      width: 180,
      render: formatTime,
    },
    {
      title: t("操作"),
      key: "actions",
      width: 110,
      render: (_, record) => (
        <Popconfirm
          title={t("断开这个连接？对方的页面会立即断开。")}
          okText={t("确定")}
          cancelText={t("取消")}
          onConfirm={() => closeMutation.mutate(record)}
        >
          <PermissionButton
            size="small"
            danger
            icon={<DisconnectOutlined />}
            permission="system:session:remote_close"
          >
            {t("断开")}
          </PermissionButton>
        </Popconfirm>
      ),
    },
  ];

  return (
    <CrudPage
      title={t("在线会话")}
      toolbar={
        <Space wrap>
          {switcher}
          <Select
            value={kind}
            className="sessions-status-select"
            options={[
              { value: "all", label: t("全部类型") },
              { value: "ssh", label: "SSH" },
              { value: "vnc", label: "VNC" },
              { value: "rdp", label: "RDP" },
              { value: "audio", label: t("远程音频") },
            ]}
            onChange={setKind}
          />
          <Input.Search
            allowClear
            value={keyword}
            placeholder={t("用户、邮箱或目标")}
            onChange={(event) => setKeyword(event.target.value)}
            onSearch={(value) => setSearch(value.trim())}
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
      <DataTable<RemoteSessionRecord>
        rowKey={(record) => `${record.kind}:${record.id}`}
        columns={columns}
        dataSource={sessionsQuery.data ?? []}
        loading={sessionsQuery.isLoading}
        pagination={false}
      />
    </CrudPage>
  );
}
