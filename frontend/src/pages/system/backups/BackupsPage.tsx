import {
  CloudUploadOutlined,
  DatabaseOutlined,
  DeleteOutlined,
  PlusOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Descriptions, Drawer, Popconfirm, Space, Tag, message } from "antd";
import type { ColumnsType } from "antd/es/table";
import { useState } from "react";
import {
  type BackupRecord,
  createBackup,
  deleteBackup,
  deliverBackup,
  fetchBackups,
} from "../../../api/admin/backups";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";

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
  const queryClient = useQueryClient();

  const backupsQuery = useQuery({
    queryKey: ["admin-backups", page],
    queryFn: () => fetchBackups({ page, page_size: 10 }),
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
        )}
      </Drawer>
    </CrudPage>
  );
}
