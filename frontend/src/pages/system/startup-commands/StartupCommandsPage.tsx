import {
  DeleteOutlined,
  EditOutlined,
  InfoCircleOutlined,
  PlayCircleOutlined,
  PlusOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  Form,
  Input,
  Modal,
  Popconfirm,
  Space,
  Switch,
  Tag,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useState } from "react";
import {
  type SaveScheduledTaskParams,
  type ScheduledTaskRecord,
  createScheduledTask,
  deleteScheduledTask,
  fetchScheduledTasks,
  runScheduledTask,
  updateScheduledTask,
} from "../../../api/admin/scheduledTasks";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { StatusTag } from "../../../components/admin/StatusTag";
import { t } from "../../../i18n";

type StartupCommandPayload = {
  command?: string;
  working_directory?: string | null;
};

type StartupCommandForm = {
  name: string;
  code: string;
  command: string;
  working_directory?: string;
  enabled: boolean;
};

function readPayload(record: ScheduledTaskRecord): StartupCommandPayload {
  if (!record.payload) {
    return {};
  }
  try {
    return JSON.parse(record.payload) as StartupCommandPayload;
  } catch {
    return {};
  }
}

function toPayload(values: StartupCommandForm) {
  return JSON.stringify({
    command: values.command.trim(),
    working_directory: values.working_directory?.trim() || null,
  });
}

export function StartupCommandsPage() {
  const [editing, setEditing] = useState<ScheduledTaskRecord | null>(null);
  const [open, setOpen] = useState(false);
  const [form] = Form.useForm<StartupCommandForm>();
  const queryClient = useQueryClient();
  const tasksQuery = useQuery({
    queryKey: ["admin-startup-commands"],
    queryFn: () =>
      fetchScheduledTasks({
        page: 1,
        page_size: 100,
        task_type: "startup_command",
      }),
  });

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ["admin-startup-commands"] });
  };
  const saveMutation = useMutation({
    mutationFn: (values: StartupCommandForm) => {
      const payload: SaveScheduledTaskParams = {
        name: values.name.trim(),
        code: values.code.trim(),
        task_type: "startup_command",
        cron_expr: "@startup",
        payload: toPayload(values),
        enabled: values.enabled,
        status: "idle",
      };
      return editing
        ? updateScheduledTask(editing.id, payload)
        : createScheduledTask(payload);
    },
    onSuccess: () => {
      message.success(t("开机自启命令已保存"));
      setOpen(false);
      setEditing(null);
      invalidate();
    },
  });
  const deleteMutation = useMutation({
    mutationFn: deleteScheduledTask,
    onSuccess: () => {
      message.success(t("开机自启命令已删除"));
      invalidate();
    },
  });
  const runMutation = useMutation({
    mutationFn: runScheduledTask,
    onSuccess: (run) => {
      message.success(t("命令执行完成：{0}", run.status));
      invalidate();
    },
  });

  const openCreate = () => {
    setEditing(null);
    form.resetFields();
    form.setFieldsValue({ enabled: true });
    setOpen(true);
  };
  const openEdit = (record: ScheduledTaskRecord) => {
    const payload = readPayload(record);
    setEditing(record);
    form.setFieldsValue({
      name: record.name,
      code: record.code,
      command: payload.command ?? "",
      working_directory: payload.working_directory ?? undefined,
      enabled: record.enabled,
    });
    setOpen(true);
  };

  const columns: ColumnsType<ScheduledTaskRecord> = [
    { title: t("名称"), dataIndex: "name", width: 170 },
    { title: t("编码"), dataIndex: "code", width: 170 },
    {
      title: t("启动命令"),
      key: "command",
      width: 360,
      ellipsis: true,
      render: (_, record) => readPayload(record).command || "-",
    },
    {
      title: t("工作目录"),
      key: "working_directory",
      width: 220,
      ellipsis: true,
      render: (_, record) => readPayload(record).working_directory || "-",
    },
    {
      title: t("启用"),
      dataIndex: "enabled",
      width: 90,
      render: (value) => <StatusTag active={value} />,
    },
    {
      title: t("上次执行"),
      dataIndex: "last_run_at",
      width: 220,
      render: (value) => value ?? "-",
    },
    {
      title: t("状态"),
      dataIndex: "status",
      width: 100,
      render: (value) => (
        <Tag
          color={
            value === "success"
              ? "green"
              : value === "failed"
                ? "red"
                : "default"
          }
        >
          {value}
        </Tag>
      ),
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
            onClick={() => openEdit(record)}
          >
            {t("编辑")}
          </PermissionButton>
          <Popconfirm
            title={t("确认删除命令？")}
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

  return (
    <CrudPage
      title={t("开机自启命令")}
      subtitle={t("配置应用服务启动时自动执行的系统命令")}
      breadcrumb={[t("系统管理"), t("开机自启命令")]}
      icon={<InfoCircleOutlined />}
      toolbar={
        <CrudToolbar
          actions={[
            {
              key: "create",
              label: t("新增自启命令"),
              icon: <PlusOutlined />,
              primary: true,
              permission: "system:scheduled_task:create",
              onClick: openCreate,
            },
          ]}
        />
      }
    >
      <Alert
        showIcon
        type="info"
        message={t("执行时机")}
        description={t(
          "命令会在本程序服务启动时执行一次。要随电脑开机运行，请先用 service install 配置本程序的系统服务；命令使用服务运行账户的权限。支持 Windows、Linux 和 macOS。",
        )}
        style={{ marginBottom: 16 }}
      />
      <DataTable<ScheduledTaskRecord>
        columns={columns}
        dataSource={tasksQuery.data?.items ?? []}
        loading={tasksQuery.isLoading}
        scroll={{ x: 1500 }}
        pagination={false}
      />
      <Modal
        title={editing ? t("编辑自启命令") : t("新增自启命令")}
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
            name="command"
            label={t("启动命令")}
            rules={[{ required: true, whitespace: true }]}
            extra={t(
              "例如 Windows 使用 ipconfig，Linux/macOS 使用 ip addr 或 ifconfig",
            )}
          >
            <Input.TextArea rows={4} placeholder="ipconfig" />
          </Form.Item>
          <Form.Item name="working_directory" label={t("工作目录")}>
            <Input placeholder={t("留空使用服务默认工作目录")} />
          </Form.Item>
          <Form.Item name="enabled" label={t("启用")} valuePropName="checked">
            <Switch />
          </Form.Item>
        </Form>
      </Modal>
    </CrudPage>
  );
}
