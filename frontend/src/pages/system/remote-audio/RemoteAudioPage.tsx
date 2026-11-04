import {
  MutedOutlined,
  PauseCircleOutlined,
  PlayCircleOutlined,
  ReloadOutlined,
  SoundOutlined,
} from "@ant-design/icons";
import { useQuery } from "@tanstack/react-query";
import {
  Alert,
  Button,
  Card,
  Input,
  Segmented,
  Select,
  Slider,
  Space,
  Tag,
  Tooltip,
  Typography,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import dayjs from "dayjs";
import { useEffect, useState } from "react";
import {
  REMOTE_AUDIO_SESSIONS_KEY,
  type RemoteAudioQuality,
  type RemoteAudioQualityKey,
  type RemoteAudioSession,
  type RemoteAudioSessionStatus,
  fetchRemoteAudioSessions,
  fetchRemoteAudioStatus,
} from "../../../api/admin/remoteAudio";
import { CrudPage } from "../../../components/admin/CrudPage";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { formatBytes } from "../../../components/admin/finder/finderModel";
import { t } from "../../../i18n";
import { useAuthStore } from "../../../stores/auth";
import { useRemoteAudioStore } from "../../../stores/remoteAudio";
import "./remoteAudio.css";

const QUALITY_KEY = "gpt-images-admin-remote-audio-quality";

const qualityLabel = (key: string) =>
  key === "high" ? t("高音质") : key === "low" ? t("省流") : t("标准");

const qualityDetail = (quality: RemoteAudioQuality) =>
  t(
    "{0} kHz {1}，约 {2} kbps",
    quality.sample_rate / 1000,
    quality.channels > 1 ? t("立体声") : t("单声道"),
    quality.bitrate_kbps,
  );

/** `1:02:03` / `02:03`. */
export function formatDuration(ms: number) {
  const total = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  const pad = (value: number) => String(value).padStart(2, "0");
  return hours > 0
    ? `${hours}:${pad(minutes)}:${pad(seconds)}`
    : `${pad(minutes)}:${pad(seconds)}`;
}

const closeReasonLabel = (reason?: string | null) => {
  switch (reason) {
    case "client":
      return t("主动停止");
    case "disconnected":
      return t("连接断开");
    case "admin":
      return t("管理员断开");
    case "user_disabled":
      return t("账号停用");
    case "error":
      return t("采集出错");
    case "server_stopped":
      return t("服务停止");
    default:
      return reason ?? "";
  }
};

function loadQuality(): RemoteAudioQualityKey | undefined {
  try {
    const value = localStorage.getItem(QUALITY_KEY);
    return value === "high" || value === "standard" || value === "low"
      ? value
      : undefined;
  } catch {
    return undefined;
  }
}

function useTicker(active: boolean) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [active]);
  return now;
}

function ListenCard() {
  const statusQuery = useQuery({
    queryKey: ["admin-remote-audio-status"],
    queryFn: fetchRemoteAudioStatus,
  });
  const player = useRemoteAudioStore();
  const canListen = useAuthStore((state) =>
    state.hasPermission("system:remote_audio:listen"),
  );
  const [quality, setQuality] = useState<RemoteAudioQualityKey | undefined>(
    loadQuality,
  );
  const now = useTicker(player.status === "playing");
  const status = statusQuery.data;
  const selected = quality ?? status?.default_quality ?? "standard";
  const active = player.status !== "idle";
  const elapsed =
    player.status === "playing" && player.startedAt
      ? now - player.startedAt
      : 0;

  const chooseQuality = (value: RemoteAudioQualityKey) => {
    setQuality(value);
    try {
      localStorage.setItem(QUALITY_KEY, value);
    } catch {
      // Only a convenience.
    }
  };

  return (
    <Card className="remote-audio-card" bordered={false}>
      {status && !status.supported ? (
        <Alert
          type="warning"
          showIcon
          message={t("本机暂时无法采集声音")}
          description={status.message}
          className="remote-audio-alert"
        />
      ) : null}
      {player.error && player.status === "idle" ? (
        <Alert
          type="error"
          showIcon
          closable
          message={player.error}
          className="remote-audio-alert"
        />
      ) : null}
      <div className="remote-audio-player">
        <PermissionButton
          type="primary"
          shape="circle"
          size="large"
          className={`remote-audio-toggle${active ? " is-active" : ""}`}
          permission="system:remote_audio:listen"
          icon={active ? <PauseCircleOutlined /> : <PlayCircleOutlined />}
          aria-label={active ? t("停止收听") : t("开始收听")}
          loading={player.status === "connecting"}
          disabled={!status?.supported}
          onClick={() =>
            active ? player.stop() : void player.start(selected).catch(() => {})
          }
        />
        <div className="remote-audio-player-main">
          <div className="remote-audio-title">
            <Typography.Text strong>
              {player.status === "playing"
                ? t("正在收听")
                : player.status === "connecting"
                  ? player.reconnectAttempt
                    ? t(
                        "连接中断，正在重连（第 {0} 次）…",
                        player.reconnectAttempt,
                      )
                    : t("正在连接…")
                  : t("收听本机声音")}
            </Typography.Text>
            <Typography.Text type="secondary" className="remote-audio-device">
              {player.device || status?.device || t("默认播放设备")}
            </Typography.Text>
          </div>
          <div
            className="remote-audio-meter"
            role="meter"
            aria-label={t("音量电平")}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(player.level * 100)}
          >
            <span style={{ width: `${Math.min(100, player.level * 100)}%` }} />
          </div>
          <Typography.Text type="secondary" className="remote-audio-hint">
            {t(
              "只传输声音，不传画面；本机没有声音播放时不产生流量。手机熄屏或切到其他应用时会继续播放，可在锁屏或通知栏停止",
            )}
          </Typography.Text>
        </div>
      </div>

      <div className="remote-audio-controls">
        <div className="remote-audio-field">
          <Typography.Text type="secondary">{t("音质")}</Typography.Text>
          <Segmented
            value={active ? player.quality : selected}
            disabled={active}
            onChange={(value) => chooseQuality(value as RemoteAudioQualityKey)}
            options={(status?.qualities ?? []).map((item) => ({
              value: item.key,
              label: (
                <Tooltip title={qualityDetail(item)}>
                  <span>{qualityLabel(item.key)}</span>
                </Tooltip>
              ),
            }))}
          />
          <Typography.Text type="secondary" className="remote-audio-quality">
            {(() => {
              const item = status?.qualities.find(
                (entry) => entry.key === (active ? player.quality : selected),
              );
              return item ? qualityDetail(item) : null;
            })()}
          </Typography.Text>
        </div>
        <div className="remote-audio-field remote-audio-volume">
          <Typography.Text type="secondary">{t("音量")}</Typography.Text>
          <Space size={8} className="remote-audio-volume-row">
            <Button
              type="text"
              icon={player.muted ? <MutedOutlined /> : <SoundOutlined />}
              aria-label={player.muted ? t("取消静音") : t("静音")}
              onClick={() => player.setMuted(!player.muted)}
            />
            <Slider
              min={0}
              max={100}
              value={Math.round(player.volume * 100)}
              onChange={(value) => player.setVolume(value / 100)}
              className="remote-audio-slider"
            />
          </Space>
        </div>
      </div>

      <div className="remote-audio-stats">
        <div>
          <span>{t("本次时长")}</span>
          <strong>{formatDuration(elapsed)}</strong>
        </div>
        <div>
          <span>{t("本次流量")}</span>
          <strong>{formatBytes(player.bytesReceived)}</strong>
        </div>
        <div>
          <span>{t("当前码率")}</span>
          <strong>
            {player.status === "playing"
              ? `${Math.round(player.bitrateKbps)} kbps`
              : "--"}
          </strong>
        </div>
      </div>
      {!canListen ? (
        <Typography.Text type="secondary">
          {t("你只能查看收听记录，没有收听权限")}
        </Typography.Text>
      ) : null}
    </Card>
  );
}

function SessionsCard() {
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(10);
  const [status, setStatus] = useState<RemoteAudioSessionStatus | "all">("all");
  const [keyword, setKeyword] = useState("");
  const [search, setSearch] = useState("");
  const playerStatus = useRemoteAudioStore((state) => state.status);

  const sessionsQuery = useQuery({
    queryKey: [REMOTE_AUDIO_SESSIONS_KEY, page, pageSize, status, search],
    queryFn: () =>
      fetchRemoteAudioSessions({
        page,
        page_size: pageSize,
        status: status === "all" ? undefined : status,
        keyword: search || undefined,
      }),
    // Live rows keep counting.
    refetchInterval: (query) =>
      query.state.data?.items.some((item) => item.status === "live")
        ? 5000
        : false,
  });
  const { refetch } = sessionsQuery;

  // A session that just started or ended shows up right away.
  useEffect(() => {
    if (playerStatus !== "connecting") void refetch();
  }, [playerStatus, refetch]);

  const columns: ColumnsType<RemoteAudioSession> = [
    {
      title: t("用户"),
      key: "user",
      render: (_, record) => (
        <Space direction="vertical" size={0}>
          <span>
            {record.user_name || (record.user_id ? `#${record.user_id}` : "-")}
          </span>
          <Typography.Text type="secondary">
            {record.user_email}
          </Typography.Text>
        </Space>
      ),
    },
    {
      title: t("开始时间"),
      dataIndex: "started_at",
      width: 170,
      render: (value: string) => dayjs(value).format("YYYY-MM-DD HH:mm:ss"),
    },
    {
      title: t("时长"),
      dataIndex: "duration_ms",
      width: 100,
      render: (value: number) => formatDuration(value),
    },
    {
      title: t("流量"),
      dataIndex: "bytes_sent",
      width: 110,
      render: (value: number, record) => (
        <Tooltip
          title={t("其中有声音的时长 {0}", formatDuration(record.audio_ms))}
        >
          <strong>{formatBytes(value)}</strong>
        </Tooltip>
      ),
    },
    {
      title: t("音质"),
      dataIndex: "quality",
      width: 90,
      render: (value: string) => qualityLabel(value),
    },
    {
      title: t("状态"),
      dataIndex: "status",
      width: 140,
      render: (value: RemoteAudioSessionStatus, record) =>
        value === "live" ? (
          <Tag color="processing">{t("收听中")}</Tag>
        ) : value === "failed" ? (
          <Tooltip title={record.error_message}>
            <Tag color="error">{t("失败")}</Tag>
          </Tooltip>
        ) : (
          <Tooltip title={record.error_message}>
            <Tag>{closeReasonLabel(record.close_reason) || t("已结束")}</Tag>
          </Tooltip>
        ),
    },
    {
      title: t("来源 IP"),
      dataIndex: "client_ip",
      width: 140,
      render: (value?: string | null) => value || "-",
    },
  ];

  return (
    <Card
      className="remote-audio-card"
      bordered={false}
      title={t("收听记录")}
      extra={
        <Typography.Text type="secondary">
          {t(
            "共 {0} 次，合计 {1}",
            sessionsQuery.data?.total ?? 0,
            formatBytes(sessionsQuery.data?.total_bytes ?? 0),
          )}
        </Typography.Text>
      }
    >
      <Space wrap className="remote-audio-filters">
        <Select
          value={status}
          className="remote-audio-status-select"
          onChange={(value) => {
            setStatus(value);
            setPage(1);
          }}
          options={[
            { value: "all", label: t("全部状态") },
            { value: "live", label: t("收听中") },
            { value: "ended", label: t("已结束") },
            { value: "failed", label: t("失败") },
          ]}
        />
        <Input.Search
          allowClear
          value={keyword}
          placeholder={t("用户名或邮箱")}
          onChange={(event) => setKeyword(event.target.value)}
          onSearch={(value) => {
            setSearch(value.trim());
            setPage(1);
          }}
        />
        <Button
          icon={<ReloadOutlined />}
          loading={sessionsQuery.isFetching}
          onClick={() => void refetch()}
        >
          {t("刷新")}
        </Button>
      </Space>
      <DataTable<RemoteAudioSession>
        rowKey="id"
        columns={columns}
        dataSource={sessionsQuery.data?.items ?? []}
        loading={sessionsQuery.isLoading}
        pagination={{
          current: page,
          pageSize,
          total: sessionsQuery.data?.total ?? 0,
          onChange: (nextPage, nextSize) => {
            setPage(nextPage);
            setPageSize(nextSize);
          },
        }}
      />
    </Card>
  );
}

export function RemoteAudioPage() {
  return (
    <CrudPage title={t("远程音频")} className="remote-audio-page">
      <div className="remote-audio-layout">
        <ListenCard />
        <SessionsCard />
      </div>
    </CrudPage>
  );
}
