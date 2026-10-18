import {
  DeleteOutlined,
  EditOutlined,
  PlayCircleOutlined,
  PlusOutlined,
  ScheduleOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Drawer,
  Form,
  Input,
  Modal,
  Popconfirm,
  Select,
  Space,
  Switch,
  Tabs,
  Tag,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useState } from "react";
import {
  type SaveScheduledTaskParams,
  type ScheduledTaskRecord,
  type TaskRunRecord,
  createScheduledTask,
  deleteScheduledTask,
  fetchScheduledTasks,
  fetchTaskRuns,
  runScheduledTask,
  updateScheduledTask,
} from "../../../api/admin/scheduledTasks";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { StatusTag } from "../../../components/admin/StatusTag";
import { t } from "../../../i18n";

const statusColor: Record<string, string> = {
  idle: "default",
  success: "green",
  failed: "red",
  running: "blue",
};

export function ScheduledTasksPage() {
  const [page, setPage] = useState(1);
  const [runPage, setRunPage] = useState(1);
  const [editing, setEditing] = useState<ScheduledTaskRecord | null>(null);
  const [open, setOpen] = useState(false);
  const [runDetail, setRunDetail] = useState<TaskRunRecord | null>(null);
  const [form] = Form.useForm<SaveScheduledTaskParams>();
  const queryClient = useQueryClient();

  const tasksQuery = useQuery({
    queryKey: ["admin-scheduled-tasks", page],
    queryFn: () => fetchScheduledTasks({ page, page_size: 10 }),
  });
  const runsQuery = useQuery({
    queryKey: ["admin-scheduled-task-runs", runPage],
    queryFn: () => fetchTaskRuns({ page: runPage, page_size: 10 }),
  });
  const saveMutation = useMutation({
    mutationFn: (values: SaveScheduledTaskParams) =>
      editing
        ? updateScheduledTask(editing.id, values)
        : createScheduledTask(values),
    onSuccess: () => {
      message.success(t("定时任务已保存"));
      setOpen(false);
      setEditing(null);
      queryClient.invalidateQueries({ queryKey: ["admin-scheduled-tasks"] });
    },
  });
  const deleteMutation = useMutation({
    mutationFn: deleteScheduledTask,
    onSuccess: () => {
      message.success(t("定时任务已删除"));
      queryClient.invalidateQueries({ queryKey: ["admin-scheduled-tasks"] });
    },
  });
  const runMutation = useMutation({
    mutationFn: runScheduledTask,
    onSuccess: (run) => {
      message.success(t("任务执行完成：{0}", run.status));
      queryClient.invalidateQueries({ queryKey: ["admin-scheduled-tasks"] });
      queryClient.invalidateQueries({
        queryKey: ["admin-scheduled-task-runs"],
      });
    },
  });

  const taskColumns: ColumnsType<ScheduledTaskRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    { title: t("名称"), dataIndex: "name", width: 160 },
    { title: t("编码"), dataIndex: "code", width: 180 },
    { title: t("类型"), dataIndex: "task_type", width: 160 },
    { title: "Cron", dataIndex: "cron_expr", width: 150 },
    {
      title: t("启用"),
      dataIndex: "enabled",
      width: 90,
      render: (value) => <StatusTag active={value} />,
    },
    {
      title: t("状态"),
      dataIndex: "status",
      width: 100,
      render: (value) => (
        <Tag color={statusColor[value] ?? "default"}>{value}</Tag>
      ),
    },
    {
      title: t("上次执行"),
      dataIndex: "last_run_at",
      width: 220,
      render: (value) => value ?? "-",
    },
    {
      title: t("操作"),
      key: "actions",
      width: 280,
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            icon={<PlayCircleOutlined />}
            permission="system:scheduled_task:run"
            onClick={() => runMutation.mutate(record.id)}
          >
            {t("运行")}
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:scheduled_task:update"
            onClick={() => {
              setEditing(record);
              form.setFieldsValue(record);
              setOpen(true);
            }}
          >
            {t("编辑")}
          </PermissionButton>
          <Popconfirm
            title={t("确认删除任务？")}
            onConfirm={() => deleteMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              icon={<DeleteOutlined />}
              permission="system:scheduled_task:delete"
            >
              {t("删除")}
            </PermissionButton>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  const runColumns: ColumnsType<TaskRunRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    { title: t("任务"), dataIndex: "code", width: 180 },
    {
      title: t("状态"),
      dataIndex: "status",
      width: 100,
      render: (value) => (
        <Tag color={statusColor[value] ?? "default"}>{value}</Tag>
      ),
    },
    { title: t("触发"), dataIndex: "triggered_by", width: 100 },
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
      width: 240,
      ellipsis: true,
    },
    {
      title: t("操作"),
      key: "actions",
      width: 100,
      render: (_, record) => (
        <PermissionButton
          size="small"
          permission="system:scheduled_task:list"
          onClick={() => setRunDetail(record)}
        >
          {t("详情")}
        </PermissionButton>
      ),
    },
  ];

  return (
    <CrudPage
      title={t("定时任务")}
      subtitle={t("管理可配置任务、手动触发和查看执行记录")}
      breadcrumb={[t("系统管理"), t("定时任务")]}
      icon={<ScheduleOutlined />}
      toolbar={
        <CrudToolbar
          actions={[
            {
              key: "create",
              label: t("新增任务"),
              icon: <PlusOutlined />,
              primary: true,
              permission: "system:scheduled_task:create",
              onClick: () => {
                setEditing(null);
                form.resetFields();
                form.setFieldsValue({
                  task_type: "cleanup_logs",
                  cron_expr: "0 3 * * *",
                  enabled: true,
                  status: "idle",
                });
                setOpen(true);
              },
            },
          ]}
        />
      }
    >
      <Tabs
        items={[
          {
            key: "tasks",
            label: t("任务列表"),
            children: (
              <DataTable<ScheduledTaskRecord>
                columns={taskColumns}
                dataSource={tasksQuery.data?.items ?? []}
                loading={tasksQuery.isLoading}
                pagination={{
                  current: page,
                  total: tasksQuery.data?.total ?? 0,
                  onChange: setPage,
                }}
              />
            ),
          },
          {
            key: "runs",
            label: t("执行记录"),
            children: (
              <DataTable<TaskRunRecord>
                columns={runColumns}
                dataSource={runsQuery.data?.items ?? []}
                loading={runsQuery.isLoading}
                pagination={{
                  current: runPage,
                  total: runsQuery.data?.total ?? 0,
                  onChange: setRunPage,
                }}
              />
            ),
          },
        ]}
      />
      <Modal
        title={editing ? t("编辑任务") : t("新增任务")}
        open={open}
        onCancel={() => setOpen(false)}
        onOk={() => form.submit()}
        confirmLoading={saveMutation.isPending}
      >
        <Form
          form={form}
          layout="vertical"
          onFinish={(values) => saveMutation.mutate(values)}
        >
          <Form.Item name="name" label={t("名称")} rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="code" label={t("编码")} rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item
            name="task_type"
            label={t("类型")}
            rules={[{ required: true }]}
          >
            <Select
              options={[
                { value: "cleanup_logs", label: t("日志清理") },
                { value: "database_backup", label: t("数据库备份") },
                { value: "webhook_notify", label: t("Webhook 通知") },
              ]}
            />
          </Form.Item>
          <Form.Item name="cron_expr" label="Cron" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="payload" label="Payload JSON">
            <Input.TextArea rows={3} />
          </Form.Item>
          <Form.Item name="enabled" label={t("启用")} valuePropName="checked">
            <Switch />
          </Form.Item>
        </Form>
      </Modal>
      <Drawer
        title={t("执行详情")}
        open={Boolean(runDetail)}
        onClose={() => setRunDetail(null)}
        width={640}
      >
        {runDetail && (
          <Space direction="vertical" className="full-width">
            <Tag color={statusColor[runDetail.status] ?? "default"}>
              {runDetail.status}
            </Tag>
            <div>{t("输出：{0}", runDetail.output ?? "-")}</div>
            <div>{t("错误：{0}", runDetail.error_message ?? "-")}</div>
          </Space>
        )}
      </Drawer>
    </CrudPage>
  );
}
