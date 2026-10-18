import { BellOutlined, DeleteOutlined, PlusOutlined } from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Button,
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
  type NotificationRecord,
  type SaveNotificationParams,
  createNotification,
  deleteNotification,
  fetchNotifications,
  markNotificationRead,
} from "../../../api/admin/notifications";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { t } from "../../../i18n";

const levelColor: Record<string, string> = {
  info: "blue",
  success: "green",
  warning: "orange",
  error: "red",
};

export function NotificationsPage() {
  const [page, setPage] = useState(1);
  const [keyword, setKeyword] = useState("");
  const [level, setLevel] = useState<string>();
  const [detail, setDetail] = useState<NotificationRecord | null>(null);
  const [open, setOpen] = useState(false);
  const [form] = Form.useForm<SaveNotificationParams>();
  const queryClient = useQueryClient();

  const notificationsQuery = useQuery({
    queryKey: ["admin-notifications", page, keyword, level],
    queryFn: () =>
      fetchNotifications({
        page,
        page_size: 10,
        keyword: keyword || undefined,
        level,
      }),
  });
  const createMutation = useMutation({
    mutationFn: createNotification,
    onSuccess: () => {
      message.success(t("通知已创建"));
      setOpen(false);
      form.resetFields();
      queryClient.invalidateQueries({ queryKey: ["admin-notifications"] });
    },
  });
  const readMutation = useMutation({
    mutationFn: markNotificationRead,
    onSuccess: () => {
      message.success(t("通知已标记已读"));
      queryClient.invalidateQueries({ queryKey: ["admin-notifications"] });
    },
  });
  const deleteMutation = useMutation({
    mutationFn: deleteNotification,
    onSuccess: () => {
      message.success(t("通知已删除"));
      queryClient.invalidateQueries({ queryKey: ["admin-notifications"] });
    },
  });

  const columns: ColumnsType<NotificationRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    { title: t("标题"), dataIndex: "title", width: 220 },
    {
      title: t("级别"),
      dataIndex: "level",
      width: 100,
      render: (value) => (
        <Tag color={levelColor[value] ?? "default"}>{value}</Tag>
      ),
    },
    { title: t("分类"), dataIndex: "category", width: 120 },
    { title: t("目标"), dataIndex: "target_type", width: 100 },
    {
      title: t("已读"),
      dataIndex: "read_at",
      width: 90,
      render: (value) => (
        <Tag color={value ? "green" : "orange"}>
          {value ? t("已读") : t("未读")}
        </Tag>
      ),
    },
    { title: t("时间"), dataIndex: "created_at", width: 220 },
    {
      title: t("操作"),
      key: "actions",
      width: 240,
      render: (_, record) => (
        <Space>
          <Button size="small" onClick={() => setDetail(record)}>
            {t("详情")}
          </Button>
          <PermissionButton
            size="small"
            permission="system:notification:update"
            disabled={Boolean(record.read_at)}
            onClick={() => readMutation.mutate(record.id)}
          >
            {t("已读")}
          </PermissionButton>
          <Popconfirm
            title={t("确认删除通知？")}
            onConfirm={() => deleteMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              icon={<DeleteOutlined />}
              permission="system:notification:delete"
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
      title={t("通知中心")}
      subtitle={t("站内通知、系统提醒和运维事件的统一入口")}
      breadcrumb={[t("系统管理"), t("通知中心")]}
      icon={<BellOutlined />}
      toolbar={
        <Space wrap>
          <Input.Search
            allowClear
            placeholder={t("搜索标题或内容")}
            onSearch={(value) => {
              setPage(1);
              setKeyword(value);
            }}
            className="admin-search-input"
          />
          <Select
            allowClear
            placeholder={t("级别")}
            value={level}
            onChange={(value) => {
              setPage(1);
              setLevel(value);
            }}
            options={[
              { value: "info", label: t("信息") },
              { value: "success", label: t("成功") },
              { value: "warning", label: t("警告") },
              { value: "error", label: t("错误") },
            ]}
            className="admin-filter-select"
          />
          <CrudToolbar
            actions={[
              {
                key: "create",
                label: t("新增通知"),
                icon: <PlusOutlined />,
                primary: true,
                permission: "system:notification:create",
                onClick: () => {
                  form.resetFields();
                  form.setFieldsValue({
                    level: "info",
                    category: "system",
                    target_type: "all",
                  });
                  setOpen(true);
                },
              },
            ]}
          />
        </Space>
      }
    >
      <DataTable<NotificationRecord>
        columns={columns}
        dataSource={notificationsQuery.data?.items ?? []}
        loading={notificationsQuery.isLoading}
        pagination={{
          current: page,
          total: notificationsQuery.data?.total ?? 0,
          onChange: setPage,
        }}
      />
      <Modal
        title={t("新增通知")}
        open={open}
        onCancel={() => setOpen(false)}
        onOk={() => form.submit()}
        confirmLoading={createMutation.isPending}
      >
        <Form
          form={form}
          layout="vertical"
          onFinish={(values) => createMutation.mutate(values)}
        >
          <Form.Item
            name="title"
            label={t("标题")}
            rules={[{ required: true }]}
          >
            <Input />
          </Form.Item>
          <Form.Item
            name="content"
            label={t("内容")}
            rules={[{ required: true }]}
          >
            <Input.TextArea rows={4} />
          </Form.Item>
          <Space className="full-width" align="start">
            <Form.Item
              name="level"
              label={t("级别")}
              rules={[{ required: true }]}
            >
              <Select
                options={["info", "success", "warning", "error"].map(
                  (value) => ({ value, label: value }),
                )}
              />
            </Form.Item>
            <Form.Item
              name="category"
              label={t("分类")}
              rules={[{ required: true }]}
            >
              <Input />
            </Form.Item>
            <Form.Item
              name="target_type"
              label={t("目标")}
              rules={[{ required: true }]}
            >
              <Select
                options={[
                  { value: "all", label: t("全部") },
                  { value: "user", label: t("用户") },
                  { value: "tenant", label: t("租户") },
                ]}
              />
            </Form.Item>
          </Space>
        </Form>
      </Modal>
      <Drawer
        title={t("通知详情")}
        open={Boolean(detail)}
        onClose={() => setDetail(null)}
        width={560}
      >
        {detail && (
          <Space direction="vertical" size="middle" className="full-width">
            <Tag color={levelColor[detail.level] ?? "default"}>
              {detail.level}
            </Tag>
            <h3>{detail.title}</h3>
            <div>{detail.content}</div>
            <div>{t("创建时间：{0}", detail.created_at)}</div>
          </Space>
        )}
      </Drawer>
    </CrudPage>
  );
}
