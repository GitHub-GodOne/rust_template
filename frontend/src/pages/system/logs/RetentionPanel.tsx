import { ClearOutlined } from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Button,
  InputNumber,
  Popconfirm,
  Space,
  Switch,
  Typography,
  message,
} from "antd";
import { useEffect, useState } from "react";
import {
  fetchRetention,
  runRetention,
  saveRetention,
} from "../../../api/admin/logs";

const DEFAULT_DAYS = 90;
const MAX_DAYS = 3650;

function formatTime(value?: string | null) {
  return value ? new Date(value).toLocaleString() : "-";
}

/** Retention period and cleanup controls for the log settings drawer. */
export function RetentionPanel({
  canEdit,
  canPurge,
}: {
  canEdit: boolean;
  canPurge: boolean;
}) {
  const queryClient = useQueryClient();
  const statusQuery = useQuery({
    queryKey: ["log-retention"],
    queryFn: fetchRetention,
  });
  const status = statusQuery.data;
  const [days, setDays] = useState<number>(DEFAULT_DAYS);
  const [forever, setForever] = useState(false);

  useEffect(() => {
    if (!status) return;
    setForever(status.retention_days === 0);
    setDays(status.retention_days || DEFAULT_DAYS);
  }, [status]);

  const wanted = forever ? 0 : days;
  const dirty = status !== undefined && wanted !== status.retention_days;

  const save = useMutation({
    mutationFn: () => saveRetention(wanted),
    onSuccess: (next) => {
      queryClient.setQueryData(["log-retention"], next);
      message.success(
        next.retention_days === 0
          ? "日志将永久保留"
          : `日志保留 ${next.retention_days} 天`,
      );
    },
    onError: (error) =>
      message.error(error instanceof Error ? error.message : "保存失败"),
  });

  const purge = useMutation({
    mutationFn: runRetention,
    onSuccess: (result) => {
      message.success(
        result.deleted > 0
          ? `已清理 ${result.deleted} 条过期日志`
          : "没有需要清理的日志",
      );
      queryClient.invalidateQueries({ queryKey: ["log-retention"] });
      queryClient.invalidateQueries({ queryKey: ["admin-logs"] });
    },
    onError: (error) =>
      message.error(error instanceof Error ? error.message : "清理失败"),
  });

  const last = status?.last_cleanup;

  return (
    <div className="retention-panel">
      <div className="retention-head">
        <div>
          <strong>日志保留</strong>
          <Typography.Paragraph type="secondary">
            超过保留期的日志每 {status?.interval_hours ?? 6}{" "}
            小时自动清理一次，对应系统设置中的「日志保留天数」。
          </Typography.Paragraph>
        </div>
      </div>
      <div className="retention-controls">
        <Space wrap>
          <span>保留</span>
          <InputNumber
            min={1}
            max={MAX_DAYS}
            precision={0}
            value={days}
            disabled={!canEdit || forever}
            onChange={(value) => setDays(value ?? DEFAULT_DAYS)}
            addonAfter="天"
            aria-label="保留天数"
            className="retention-days"
          />
          <Switch
            checked={forever}
            disabled={!canEdit}
            onChange={setForever}
            aria-label="永久保留"
          />
          <span>永久保留</span>
        </Space>
        <Space wrap>
          {canEdit ? (
            <Button
              type="primary"
              disabled={!dirty}
              loading={save.isPending}
              onClick={() => save.mutate()}
            >
              保存
            </Button>
          ) : null}
          {canPurge ? (
            <Popconfirm
              title="立即清理过期日志？"
              description={
                status && status.retention_days > 0
                  ? `将删除 ${status.expired_count} 条超过 ${status.retention_days} 天的日志，无法恢复`
                  : "当前为永久保留，不会删除任何日志"
              }
              okText="清理"
              okButtonProps={{ danger: true }}
              cancelText="取消"
              onConfirm={() => purge.mutate()}
              disabled={dirty}
            >
              <Button
                icon={<ClearOutlined />}
                loading={purge.isPending}
                disabled={dirty}
                title={dirty ? "请先保存保留天数" : undefined}
              >
                立即清理
              </Button>
            </Popconfirm>
          ) : null}
        </Space>
      </div>
      {status ? (
        <ul className="retention-facts">
          <li>
            共 {status.total_count} 条，最早 {formatTime(status.oldest_at)}
          </li>
          <li>
            {status.retention_days > 0
              ? `超过保留期待清理：${status.expired_count} 条`
              : "永久保留，不会自动清理"}
          </li>
          <li>
            上次清理：
            {last
              ? `${formatTime(last.ran_at)}（${
                  last.trigger === "manual" ? "手动" : "自动"
                }），${
                  last.error ? `失败：${last.error}` : `删除 ${last.deleted} 条`
                }`
              : "服务启动后尚未运行"}
          </li>
          {status.next_run_at ? (
            <li>下次自动清理：{formatTime(status.next_run_at)}</li>
          ) : null}
        </ul>
      ) : null}
    </div>
  );
}
