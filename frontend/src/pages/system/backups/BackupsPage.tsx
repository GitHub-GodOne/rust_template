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
  Input,
  Modal,
  Popconfirm,
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
  fetchBackupRestores,
  fetchBackups,
  restoreBackup,
} from "../../../api/admin/backups";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";

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
  const queryClient = useQueryClient();

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
      message.success(`备份任务已完成：${backup.status}`);
      queryClient.invalidateQueries({ queryKey: ["admin-backups"] });
    },
  });
  const deliverMutation = useMutation({
    mutationFn: deliverBackup,
    onSuccess: () => {
      message.success("备份推送状态已更新");
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
      message.success("数据库还原已执行");
      setRestoreTarget(null);
      setConfirmPhrase("");
      queryClient.invalidateQueries({ queryKey: ["admin-backups"] });
      queryClient.invalidateQueries({ queryKey: ["admin-backup-restores"] });
    },
  });
  const deleteMutation = useMutation({
    mutationFn: deleteBackup,
    onSuccess: () => {
      message.success("备份记录已删除");
      queryClient.invalidateQueries({ queryKey: ["admin-backups"] });
    },
  });

  const columns: ColumnsType<BackupRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    { title: "文件名", dataIndex: "filename", width: 220 },
    {
      title: "大小",
      dataIndex: "size_bytes",
      width: 120,
      render: (value) => `${value} B`,
    },
    {
      title: "状态",
      dataIndex: "status",
      width: 100,
      render: (value) => (
        <Tag color={statusColor[value] ?? "default"}>{value}</Tag>
      ),
    },
    { title: "触发", dataIndex: "trigger_type", width: 100 },
    {
      title: "耗时",
      dataIndex: "duration_ms",
      width: 100,
      render: (value) => (value ? `${value} ms` : "-"),
    },
    { title: "开始时间", dataIndex: "started_at", width: 220 },
    { title: "错误", dataIndex: "error_message", width: 260, ellipsis: true },
    {
      title: "操作",
      key: "actions",
      width: 250,
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            permission="system:backup:list"
            onClick={() => setDetail(record)}
          >
            详情
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<CloudUploadOutlined />}
            permission="system:backup:deliver"
            onClick={() => deliverMutation.mutate(record.id)}
          >
            推送
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
            还原
          </PermissionButton>
          <Popconfirm
            title="确认删除备份记录？"
            onConfirm={() => deleteMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              icon={<DeleteOutlined />}
              permission="system:backup:delete"
            >
              删除
            </PermissionButton>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <CrudPage
      title="数据库备份"
      subtitle="手动创建 PostgreSQL 备份，记录校验值和推送状态"
      breadcrumb={["系统管理", "数据库备份"]}
      icon={<DatabaseOutlined />}
      toolbar={
        <CrudToolbar
          actions={[
            {
              key: "create",
              label: "立即备份",
              icon: <PlusOutlined />,
              primary: true,
              permission: "system:backup:create",
              onClick: () => createMutation.mutate(),
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
      <Drawer
        title="备份详情"
        open={Boolean(detail)}
        onClose={() => setDetail(null)}
        width={680}
      >
        {detail && (
          <Space direction="vertical" size="middle" style={{ width: "100%" }}>
            <Descriptions column={1} bordered size="small">
              <Descriptions.Item label="文件名">
                {detail.filename}
              </Descriptions.Item>
              <Descriptions.Item label="路径">
                {detail.storage_path}
              </Descriptions.Item>
              <Descriptions.Item label="SHA-256">
                {detail.sha256 ?? "-"}
              </Descriptions.Item>
              <Descriptions.Item label="推送目标">
                {detail.delivery_targets ?? "-"}
              </Descriptions.Item>
              <Descriptions.Item label="推送状态">
                <pre className="admin-code-block">
                  {formatDeliveryStatus(detail.delivery_status)}
                </pre>
              </Descriptions.Item>
              <Descriptions.Item label="错误">
                {detail.error_message ?? "-"}
              </Descriptions.Item>
            </Descriptions>
            <Descriptions title="还原记录" column={1} bordered size="small">
              {restoresQuery.isLoading ? (
                <Descriptions.Item label="记录">加载中</Descriptions.Item>
              ) : (restoresQuery.data ?? []).length === 0 ? (
                <Descriptions.Item label="记录">-</Descriptions.Item>
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
                        安全备份：{restore.pre_restore_backup_id ?? "-"}
                      </span>
                      <span>操作者：{restore.restored_by ?? "-"}</span>
                      <span>开始：{restore.started_at}</span>
                      <span>结束：{restore.finished_at ?? "-"}</span>
                      <span>耗时：{restore.duration_ms ?? "-"} ms</span>
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
        title="确认还原数据库"
        open={Boolean(restoreTarget)}
        okText="确认还原"
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
            message="该操作会先自动创建安全备份，然后用所选备份覆盖当前数据库。"
          />
          <Descriptions column={1} size="small" bordered>
            <Descriptions.Item label="备份 ID">
              {restoreTarget?.id ?? "-"}
            </Descriptions.Item>
            <Descriptions.Item label="文件名">
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
