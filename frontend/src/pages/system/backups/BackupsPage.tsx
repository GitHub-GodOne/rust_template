import {
  CloudUploadOutlined,
  DatabaseOutlined,
  DeleteOutlined,
  PlusOutlined,
  RollbackOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  Descriptions,
  Drawer,
  Form,
  Input,
  Modal,
  Popconfirm,
  Select,
  Space,
  Tag,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useState } from "react";
import {
  type BackupRecord,
  createBackup,
  deleteBackup,
  deliverBackup,
  fetchBackupDestinations,
  fetchBackupRestores,
  fetchBackups,
  restoreBackup,
} from "../../../api/admin/backups";
import { fetchDbConnections } from "../../../api/admin/dbManager";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { t } from "../../../i18n";

const RESTORE_CONFIRM_PHRASE = "RESTORE DATABASE";

const statusColor: Record<string, string> = {
  success: "green",
  failed: "red",
  running: "blue",
};

function formatDeliveryStatus(value?: string | null) {
  if (!value) {
    return "-";
  }

  try {
    return JSON.stringify(JSON.parse(value), null, 2);
  } catch {
    return value;
  }
}

export function BackupsPage() {
  const [page, setPage] = useState(1);
  const [detail, setDetail] = useState<BackupRecord | null>(null);
  const [restoreTarget, setRestoreTarget] = useState<BackupRecord | null>(null);
  const [confirmPhrase, setConfirmPhrase] = useState("");
  const [createOpen, setCreateOpen] = useState(false);
  const [createConnectionId, setCreateConnectionId] = useState<number>();
  const [createDeliveryTargets, setCreateDeliveryTargets] = useState("");
  const [createSelectedTargets, setCreateSelectedTargets] = useState<string[]>(
    [],
  );
  const queryClient = useQueryClient();

  const connectionsQuery = useQuery({
    queryKey: ["db-connections"],
    queryFn: fetchDbConnections,
  });
  const destinationsQuery = useQuery({
    queryKey: ["backup-destinations"],
    queryFn: fetchBackupDestinations,
  });

  const backupsQuery = useQuery({
    queryKey: ["admin-backups", page],
    queryFn: () => fetchBackups({ page, page_size: 10 }),
  });
  const restoresQuery = useQuery({
    queryKey: ["admin-backup-restores", detail?.id],
    queryFn: () => fetchBackupRestores(detail?.id ?? 0),
    enabled: Boolean(detail),
  });
  const createMutation = useMutation({
    mutationFn: createBackup,
    onSuccess: (backup) => {
      message.success(t("备份任务已完成：{0}", backup.status));
      queryClient.invalidateQueries({ queryKey: ["admin-backups"] });
    },
  });
  const deliverMutation = useMutation({
    mutationFn: deliverBackup,
    onSuccess: () => {
      message.success(t("备份推送状态已更新"));
      queryClient.invalidateQueries({ queryKey: ["admin-backups"] });
    },
  });
  const restoreMutation = useMutation({
    mutationFn: ({
      id,
      confirm_phrase,
    }: {
      id: number;
      confirm_phrase: string;
    }) => restoreBackup(id, { confirm_phrase }),
    onSuccess: () => {
      message.success(t("数据库还原已执行"));
      setRestoreTarget(null);
      setConfirmPhrase("");
      queryClient.invalidateQueries({ queryKey: ["admin-backups"] });
      queryClient.invalidateQueries({ queryKey: ["admin-backup-restores"] });
    },
  });
  const deleteMutation = useMutation({
    mutationFn: deleteBackup,
    onSuccess: () => {
      message.success(t("备份记录已删除"));
      queryClient.invalidateQueries({ queryKey: ["admin-backups"] });
    },
  });

  const columns: ColumnsType<BackupRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    {
      title: t("数据库"),
      dataIndex: "database_label",
      width: 180,
      render: (value) => value || t("当前项目数据库"),
    },
    { title: t("文件名"), dataIndex: "filename", width: 220 },
    {
      title: t("大小"),
      dataIndex: "size_bytes",
      width: 120,
      render: (value) => `${value} B`,
    },
    {
      title: t("状态"),
      dataIndex: "status",
      width: 100,
      render: (value) => (
        <Tag color={statusColor[value] ?? "default"}>{value}</Tag>
      ),
    },
    { title: t("触发"), dataIndex: "trigger_type", width: 100 },
    {
      title: t("耗时"),
      dataIndex: "duration_ms",
      width: 100,
      render: (value) => (value ? `${value} ms` : "-"),
    },
    { title: t("开始时间"), dataIndex: "started_at", width: 220 },
    {
      title: t("错误"),
      dataIndex: "error_message",
      width: 260,
      ellipsis: true,
    },
    {
      title: t("操作"),
      key: "actions",
      width: 250,
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            permission="system:backup:list"
            onClick={() => setDetail(record)}
          >
            {t("详情")}
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<CloudUploadOutlined />}
            permission="system:backup:deliver"
            onClick={() => deliverMutation.mutate(record.id)}
          >
            {t("推送")}
          </PermissionButton>
          <PermissionButton
            size="small"
            danger
            disabled={record.status !== "success"}
            icon={<RollbackOutlined />}
            permission="system:backup:restore"
            onClick={() => {
              setRestoreTarget(record);
              setConfirmPhrase("");
            }}
          >
            {t("还原")}
          </PermissionButton>
          <Popconfirm
            title={t("确认删除备份记录？")}
            onConfirm={() => deleteMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              icon={<DeleteOutlined />}
              permission="system:backup:delete"
            >
              {t("删除")}
            </PermissionButton>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <CrudPage
      title={t("数据库备份")}
      subtitle={t("手动创建 PostgreSQL 备份，记录校验值和推送状态")}
      breadcrumb={[t("系统管理"), t("数据库备份")]}
      icon={<DatabaseOutlined />}
      toolbar={
        <CrudToolbar
          actions={[
            {
              key: "create",
              label: t("立即备份"),
              icon: <PlusOutlined />,
              primary: true,
              permission: "system:backup:create",
              onClick: () => {
                setCreateConnectionId(undefined);
                setCreateDeliveryTargets("");
                setCreateSelectedTargets([]);
                setCreateOpen(true);
              },
            },
          ]}
        />
      }
    >
      <DataTable<BackupRecord>
        columns={columns}
        dataSource={backupsQuery.data?.items ?? []}
        loading={backupsQuery.isLoading || createMutation.isPending}
        pagination={{
          current: page,
          total: backupsQuery.data?.total ?? 0,
          onChange: setPage,
        }}
      />
      <Modal
        title={t("创建数据库备份")}
        open={createOpen}
        okText={t("立即备份")}
        confirmLoading={createMutation.isPending}
        onCancel={() => setCreateOpen(false)}
        onOk={() => {
          createMutation.mutate(
            {
              ...(createConnectionId
                ? { connection_id: createConnectionId }
                : {}),
              ...(createDeliveryTargets.trim() || createSelectedTargets.length
                ? {
                    delivery_targets: [
                      ...createSelectedTargets,
                      ...createDeliveryTargets
                        .split(/[\n,，]/)
                        .map((value) => value.trim())
                        .filter(Boolean),
                    ].filter(
                      (value, index, values) => values.indexOf(value) === index,
                    ),
                  }
                : {}),
            },
            { onSuccess: () => setCreateOpen(false) },
          );
        }}
      >
        <Form layout="vertical">
          <Form.Item label={t("备份数据库")}>
            <Select
              allowClear
              value={createConnectionId}
              loading={connectionsQuery.isLoading}
              placeholder={t("当前项目数据库（默认）")}
              options={(connectionsQuery.data ?? [])
                .filter((connection) => connection.db_type === "postgres")
                .map((connection) => ({
                  value: connection.id,
                  label: `${connection.name} (${connection.db_type})`,
                }))}
              onChange={(value: number | undefined) =>
                setCreateConnectionId(value)
              }
            />
          </Form.Item>
          <Alert
            type="info"
            showIcon
            message={t(
              "留空时备份当前项目数据库；选择已保存的连接后，将按该连接执行 PostgreSQL 备份。",
            )}
          />
          <Form.Item label={t("推送目标")}>
            <Select
              mode="multiple"
              allowClear
              value={createSelectedTargets}
              loading={destinationsQuery.isLoading}
              placeholder={t("选择已配置的 Telegram 目标")}
              options={[
                { value: "telegram", label: t("Telegram（全部目标）") },
                ...(destinationsQuery.data ?? [])
                  .filter((destination) => destination.enabled)
                  .map((destination) => ({
                    value: `telegram:${destination.id}`,
                    label: `${destination.name ?? destination.id} (Telegram)`,
                  })),
                { value: "wecom", label: "WeCom" },
                { value: "dingtalk", label: "DingTalk" },
                { value: "custom", label: t("自定义 Webhook") },
              ]}
              onChange={(values: string[]) => setCreateSelectedTargets(values)}
              style={{ width: "100%", marginBottom: 8 }}
            />
            <Input.TextArea
              value={createDeliveryTargets}
              autoSize={{ minRows: 2, maxRows: 4 }}
              placeholder={t(
                "可选：telegram:目标ID、wecom、dingtalk 或 custom；多个目标用逗号或换行分隔",
              )}
              onChange={(event) => setCreateDeliveryTargets(event.target.value)}
            />
          </Form.Item>
          <Alert
            type="warning"
            showIcon
            message={t(
              "Telegram 目标在系统设置的 backup.telegram_destinations JSON 中配置；国内网络可在 http_client.runtime JSON 中将 proxy_enabled 设为 true 并填写 proxy_url（HTTP/HTTPS/SOCKS5）。",
            )}
          />
        </Form>
      </Modal>
      <Drawer
        title={t("备份详情")}
        open={Boolean(detail)}
        onClose={() => setDetail(null)}
        width={680}
      >
        {detail && (
          <Space direction="vertical" size="middle" style={{ width: "100%" }}>
            <Descriptions column={1} bordered size="small">
              <Descriptions.Item label={t("数据库")}>
                {detail.database_label ?? t("当前项目数据库")}
              </Descriptions.Item>
              <Descriptions.Item label={t("文件名")}>
                {detail.filename}
              </Descriptions.Item>
              <Descriptions.Item label={t("路径")}>
                {detail.storage_path}
              </Descriptions.Item>
              <Descriptions.Item label="SHA-256">
                {detail.sha256 ?? "-"}
              </Descriptions.Item>
              <Descriptions.Item label={t("推送目标")}>
                {detail.delivery_targets ?? "-"}
              </Descriptions.Item>
              <Descriptions.Item label={t("推送状态")}>
                <pre className="admin-code-block">
                  {formatDeliveryStatus(detail.delivery_status)}
                </pre>
              </Descriptions.Item>
              <Descriptions.Item label={t("错误")}>
                {detail.error_message ?? "-"}
              </Descriptions.Item>
            </Descriptions>
            <Descriptions
              title={t("还原记录")}
              column={1}
              bordered
              size="small"
            >
              {restoresQuery.isLoading ? (
                <Descriptions.Item label={t("记录")}>
                  {t("加载中")}
                </Descriptions.Item>
              ) : (restoresQuery.data ?? []).length === 0 ? (
                <Descriptions.Item label={t("记录")}>-</Descriptions.Item>
              ) : (
                (restoresQuery.data ?? []).map((restore) => (
                  <Descriptions.Item
                    key={restore.id}
                    label={`#${restore.id} ${restore.status}`}
                  >
                    <Space
                      direction="vertical"
                      size={4}
                      style={{ width: "100%" }}
                    >
                      <span>
                        {t(
                          "安全备份：{0}",
                          restore.pre_restore_backup_id ?? "-",
                        )}
                      </span>
                      <span>
                        {t("操作者：{0}", restore.restored_by ?? "-")}
                      </span>
                      <span>{t("开始：{0}", restore.started_at)}</span>
                      <span>{t("结束：{0}", restore.finished_at ?? "-")}</span>
                      <span>
                        {t("耗时：{0} ms", restore.duration_ms ?? "-")}
                      </span>
                      {restore.output && (
                        <pre className="admin-code-block">{restore.output}</pre>
                      )}
                      {restore.error_message && (
                        <pre className="admin-code-block">
                          {restore.error_message}
                        </pre>
                      )}
                    </Space>
                  </Descriptions.Item>
                ))
              )}
            </Descriptions>
          </Space>
        )}
      </Drawer>
      <Modal
        title={t("确认还原数据库")}
        open={Boolean(restoreTarget)}
        okText={t("确认还原")}
        okButtonProps={{
          danger: true,
          disabled: confirmPhrase.trim() !== RESTORE_CONFIRM_PHRASE,
        }}
        confirmLoading={restoreMutation.isPending}
        onCancel={() => {
          setRestoreTarget(null);
          setConfirmPhrase("");
        }}
        onOk={() => {
          if (!restoreTarget) {
            return;
          }
          restoreMutation.mutate({
            id: restoreTarget.id,
            confirm_phrase: confirmPhrase,
          });
        }}
      >
        <Space direction="vertical" size="middle" style={{ width: "100%" }}>
          <Alert
            type="warning"
            showIcon
            message={t(
              "该操作会先自动创建安全备份，然后用所选备份覆盖当前数据库。",
            )}
          />
          <Descriptions column={1} size="small" bordered>
            <Descriptions.Item label={t("备份 ID")}>
              {restoreTarget?.id ?? "-"}
            </Descriptions.Item>
            <Descriptions.Item label={t("文件名")}>
              {restoreTarget?.filename ?? "-"}
            </Descriptions.Item>
            <Descriptions.Item label="SHA-256">
              {restoreTarget?.sha256 ?? "-"}
            </Descriptions.Item>
          </Descriptions>
          <Input
            value={confirmPhrase}
            placeholder={RESTORE_CONFIRM_PHRASE}
            onChange={(event) => setConfirmPhrase(event.target.value)}
          />
        </Space>
      </Modal>
    </CrudPage>
  );
}
