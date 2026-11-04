import { BellOutlined, CheckOutlined } from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge, Button, Empty, Popover, Spin, Tooltip, Typography } from "antd";
import dayjs from "dayjs";
import relativeTime from "dayjs/plugin/relativeTime";
import { useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  type FeedItem,
  fetchNotificationFeed,
  markAllNotificationsRead,
  markNotificationRead,
} from "../api/userNotifications";
import { t } from "../i18n";
import { useAuthStore } from "../stores/auth";

dayjs.extend(relativeTime);

export const NOTIFICATION_FEED_KEY = ["user-notifications"];

const levelClass: Record<string, string> = {
  warning: "is-warning",
  warn: "is-warning",
  error: "is-error",
  success: "is-success",
};

export function NotificationBell() {
  const [open, setOpen] = useState(false);
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const hasPermission = useAuthStore((state) => state.hasPermission);
  const feedQuery = useQuery({
    queryKey: NOTIFICATION_FEED_KEY,
    queryFn: () => fetchNotificationFeed(20),
    // Live events refresh it; this only catches up after a lost stream.
    refetchInterval: 5 * 60_000,
  });
  const refresh = () =>
    queryClient.invalidateQueries({ queryKey: NOTIFICATION_FEED_KEY });
  const readMutation = useMutation({
    mutationFn: markNotificationRead,
    onSuccess: refresh,
  });
  const readAllMutation = useMutation({
    mutationFn: markAllNotificationsRead,
    onSuccess: refresh,
  });
  const unread = feedQuery.data?.unread_count ?? 0;
  const items = feedQuery.data?.items ?? [];

  const renderItem = (item: FeedItem) => (
    <button
      type="button"
      key={item.id}
      className={`notification-item ${item.read ? "is-read" : ""} ${
        levelClass[item.level] ?? ""
      }`}
      onClick={() => {
        if (!item.read) readMutation.mutate(item.id);
      }}
    >
      <span className="notification-dot" aria-hidden="true" />
      <span className="notification-body">
        <Typography.Text strong ellipsis className="notification-title">
          {item.title}
        </Typography.Text>
        <Typography.Paragraph
          type="secondary"
          ellipsis={{ rows: 2 }}
          className="notification-content"
        >
          {item.content}
        </Typography.Paragraph>
        <span className="notification-time">
          {dayjs(item.created_at).fromNow()}
        </span>
      </span>
    </button>
  );

  const panel = (
    <div className="notification-panel">
      <div className="notification-panel-header">
        <Typography.Text strong>{t("通知")}</Typography.Text>
        <Button
          type="link"
          size="small"
          icon={<CheckOutlined />}
          disabled={unread === 0}
          loading={readAllMutation.isPending}
          onClick={() => readAllMutation.mutate()}
        >
          {t("全部已读")}
        </Button>
      </div>
      <div className="notification-list">
        {feedQuery.isLoading ? (
          <div className="notification-empty">
            <Spin />
          </div>
        ) : items.length === 0 ? (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={t("暂无通知")}
          />
        ) : (
          items.map(renderItem)
        )}
      </div>
      {hasPermission("system:notification:list") ? (
        <div className="notification-panel-footer">
          <Button
            type="link"
            size="small"
            onClick={() => {
              setOpen(false);
              navigate("/admin/system/notifications");
            }}
          >
            {t("管理通知")}
          </Button>
        </div>
      ) : null}
    </div>
  );

  return (
    <Popover
      content={panel}
      trigger="click"
      placement="bottomRight"
      open={open}
      onOpenChange={setOpen}
      arrow={false}
      overlayClassName="notification-popover"
    >
      <Tooltip title={t("通知")}>
        <Button
          type="text"
          className="admin-icon-button"
          aria-label={unread > 0 ? t("通知（{0} 条未读）", unread) : t("通知")}
          icon={
            <Badge count={unread} size="small" overflowCount={99}>
              <BellOutlined />
            </Badge>
          }
        />
      </Tooltip>
    </Popover>
  );
}
