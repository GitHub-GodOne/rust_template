import {
  CommentOutlined,
  DeleteOutlined,
  EditOutlined,
  PlusOutlined,
  ProfileOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Button,
  Descriptions,
  Drawer,
  Form,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Select,
  Space,
  Tag,
  Timeline,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useState } from "react";
import {
  type SaveWorkOrderParams,
  type WorkOrderDetailRecord,
  type WorkOrderRecord,
  addWorkOrderAttachment,
  assignWorkOrder,
  createWorkOrder,
  createWorkOrderComment,
  deleteWorkOrder,
  deleteWorkOrderAttachment,
  fetchWorkOrder,
  fetchWorkOrders,
  transitionWorkOrder,
  updateWorkOrder,
} from "../../../api/admin/workOrders";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";

const statusOptions = [
  { value: "open", label: "待处理" },
  { value: "assigned", label: "已分配" },
  { value: "in_progress", label: "处理中" },
  { value: "pending", label: "挂起" },
  { value: "resolved", label: "已解决" },
  { value: "closed", label: "已关闭" },
  { value: "cancelled", label: "已取消" },
];

const priorityOptions = [
  { value: "low", label: "低" },
  { value: "normal", label: "普通" },
  { value: "high", label: "高" },
  { value: "urgent", label: "紧急" },
];

const statusColor: Record<string, string> = {
  open: "blue",
  assigned: "cyan",
  in_progress: "geekblue",
  pending: "orange",
  resolved: "green",
  closed: "default",
  cancelled: "red",
};

const priorityColor: Record<string, string> = {
  low: "default",
  normal: "blue",
  high: "orange",
  urgent: "red",
};

function labelOf(options: { value: string; label: string }[], value: string) {
  return options.find((item) => item.value === value)?.label ?? value;
}

function toPayload(values: SaveWorkOrderParams): SaveWorkOrderParams {
  const attachmentIds = values.attachment_file_ids
    ?.map((value) => Number(value))
    .filter((value) => Number.isInteger(value) && value > 0);

  return {
    ...values,
    assignee_id: values.assignee_id ?? null,
    attachment_file_ids: attachmentIds?.length ? attachmentIds : null,
  };
}

export function WorkOrdersPage() {
  const [page, setPage] = useState(1);
  const [keyword, setKeyword] = useState("");
  const [status, setStatus] = useState<string>();
  const [priority, setPriority] = useState<string>();
  const [editing, setEditing] = useState<WorkOrderRecord | null>(null);
  const [detailId, setDetailId] = useState<number | null>(null);
  const [open, setOpen] = useState(false);
  const [form] = Form.useForm<SaveWorkOrderParams>();
  const [commentForm] = Form.useForm<{ body: string }>();
  const [transitionForm] = Form.useForm<{ status: string; comment?: string }>();
  const [assignForm] = Form.useForm<{ assignee_id: number; note?: string }>();
  const [attachmentForm] = Form.useForm<{
    upload_file_id: number;
    description?: string;
  }>();
  const queryClient = useQueryClient();

  const workOrdersQuery = useQuery({
    queryKey: ["admin-work-orders", page, keyword, status, priority],
    queryFn: () =>
      fetchWorkOrders({
        page,
        page_size: 10,
        keyword: keyword || undefined,
        status,
        priority,
      }),
  });
  const detailQuery = useQuery({
    queryKey: ["admin-work-order", detailId],
    queryFn: () => fetchWorkOrder(detailId as number),
    enabled: Boolean(detailId),
  });

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ["admin-work-orders"] });
    queryClient.invalidateQueries({ queryKey: ["admin-work-order"] });
  };
  const createMutation = useMutation({
    mutationFn: createWorkOrder,
    onSuccess: () => {
      message.success("工单已创建");
      setOpen(false);
      form.resetFields();
      invalidate();
    },
  });
  const updateMutation = useMutation({
    mutationFn: ({
      id,
      payload,
    }: { id: number; payload: SaveWorkOrderParams }) =>
      updateWorkOrder(id, payload),
    onSuccess: () => {
      message.success("工单已更新");
      setOpen(false);
      setEditing(null);
      form.resetFields();
      invalidate();
    },
  });
  const deleteMutation = useMutation({
    mutationFn: deleteWorkOrder,
    onSuccess: () => {
      message.success("工单已删除");
      invalidate();
    },
  });
  const transitionMutation = useMutation({
    mutationFn: ({
      id,
      payload,
    }: {
      id: number;
      payload: { status: string; comment?: string };
    }) => transitionWorkOrder(id, payload),
    onSuccess: () => {
      message.success("状态已更新");
      transitionForm.resetFields();
      invalidate();
    },
  });
  const commentMutation = useMutation({
    mutationFn: ({ id, body }: { id: number; body: string }) =>
      createWorkOrderComment(id, { body }),
    onSuccess: () => {
      message.success("评论已添加");
      commentForm.resetFields();
      invalidate();
    },
  });
  const assignMutation = useMutation({
    mutationFn: ({
      id,
      payload,
    }: {
      id: number;
      payload: { assignee_id: number; note?: string };
    }) => assignWorkOrder(id, payload),
    onSuccess: () => {
      message.success("工单已分配");
      assignForm.resetFields();
      invalidate();
    },
  });
  const addAttachmentMutation = useMutation({
    mutationFn: ({
      id,
      payload,
    }: {
      id: number;
      payload: { upload_file_id: number; description?: string };
    }) => addWorkOrderAttachment(id, payload),
    onSuccess: () => {
      message.success("附件已绑定");
      attachmentForm.resetFields();
      invalidate();
    },
  });
  const deleteAttachmentMutation = useMutation({
    mutationFn: ({ id, attachmentId }: { id: number; attachmentId: number }) =>
      deleteWorkOrderAttachment(id, attachmentId),
    onSuccess: () => {
      message.success("附件引用已移除");
      invalidate();
    },
  });

  const openEditor = (record?: WorkOrderRecord) => {
    setEditing(record ?? null);
    form.resetFields();
    form.setFieldsValue(
      record ?? {
        priority: "normal",
        category: "technical",
      },
    );
    setOpen(true);
  };

  const columns: ColumnsType<WorkOrderRecord> = [
    { title: "编号", dataIndex: "order_no", width: 190 },
    { title: "标题", dataIndex: "title", width: 240 },
    {
      title: "状态",
      dataIndex: "status",
      width: 110,
      render: (value) => (
        <Tag color={statusColor[value] ?? "default"}>
          {labelOf(statusOptions, value)}
        </Tag>
      ),
    },
    {
      title: "优先级",
      dataIndex: "priority",
      width: 100,
      render: (value) => (
        <Tag color={priorityColor[value] ?? "default"}>
          {labelOf(priorityOptions, value)}
        </Tag>
      ),
    },
    { title: "分类", dataIndex: "category", width: 120 },
    { title: "创建人", dataIndex: "creator_id", width: 100 },
    { title: "负责人", dataIndex: "assignee_id", width: 100 },
    { title: "截止时间", dataIndex: "due_at", width: 220 },
    { title: "创建时间", dataIndex: "created_at", width: 220 },
    {
      title: "操作",
      key: "actions",
      width: 260,
      fixed: "right",
      render: (_, record) => (
        <Space>
          <Button size="small" onClick={() => setDetailId(record.id)}>
            详情
          </Button>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:work_order:update"
            onClick={() => openEditor(record)}
          >
            编辑
          </PermissionButton>
          <Popconfirm
            title="确认删除工单？"
            onConfirm={() => deleteMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              icon={<DeleteOutlined />}
              permission="system:work_order:delete"
            >
              删除
            </PermissionButton>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  const detail = detailQuery.data;

  return (
    <CrudPage
      title="工单管理"
      subtitle="统一处理问题反馈、任务分派、状态流转和附件记录"
      breadcrumb={["系统管理", "工单管理"]}
      icon={<ProfileOutlined />}
      toolbar={
        <Space wrap>
          <Input.Search
            allowClear
            placeholder="搜索编号、标题或描述"
            onSearch={(value) => {
              setPage(1);
              setKeyword(value);
            }}
            className="admin-search-input"
          />
          <Select
            allowClear
            placeholder="状态"
            value={status}
            onChange={(value) => {
              setPage(1);
              setStatus(value);
            }}
            options={statusOptions}
            className="admin-filter-select"
          />
          <Select
            allowClear
            placeholder="优先级"
            value={priority}
            onChange={(value) => {
              setPage(1);
              setPriority(value);
            }}
            options={priorityOptions}
            className="admin-filter-select"
          />
          <CrudToolbar
            actions={[
              {
                key: "create",
                label: "新增工单",
                icon: <PlusOutlined />,
                primary: true,
                permission: "system:work_order:create",
                onClick: () => openEditor(),
              },
            ]}
          />
        </Space>
      }
    >
      <DataTable<WorkOrderRecord>
        columns={columns}
        dataSource={workOrdersQuery.data?.items ?? []}
        loading={workOrdersQuery.isLoading}
        pagination={{
          current: page,
          total: workOrdersQuery.data?.total ?? 0,
          onChange: setPage,
        }}
      />
      <Modal
        title={editing ? "编辑工单" : "新增工单"}
        open={open}
        onCancel={() => setOpen(false)}
        onOk={() => form.submit()}
        confirmLoading={createMutation.isPending || updateMutation.isPending}
        width={720}
      >
        <Form
          form={form}
          layout="vertical"
          onFinish={(values) => {
            const payload = toPayload(values);
            if (editing) {
              updateMutation.mutate({ id: editing.id, payload });
            } else {
              createMutation.mutate(payload);
            }
          }}
        >
          <Form.Item name="title" label="标题" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item
            name="description"
            label="描述"
            rules={[{ required: true }]}
          >
            <Input.TextArea rows={4} />
          </Form.Item>
          <Space className="full-width" align="start" wrap>
            <Form.Item name="category" label="分类">
              <Select
                options={[
                  { value: "technical", label: "技术" },
                  { value: "billing", label: "计费" },
                  { value: "account", label: "账号" },
                  { value: "other", label: "其他" },
                ]}
                className="admin-filter-select"
              />
            </Form.Item>
            <Form.Item name="priority" label="优先级">
              <Select
                options={priorityOptions}
                className="admin-filter-select"
              />
            </Form.Item>
            <Form.Item name="assignee_id" label="负责人 ID">
              <InputNumber min={1} />
            </Form.Item>
          </Space>
          <Form.Item name="due_at" label="截止时间 RFC3339">
            <Input placeholder="2026-05-28T12:00:00+08:00" />
          </Form.Item>
          <Form.Item name="metadata" label="扩展数据 JSON">
            <Input.TextArea rows={3} placeholder='{"source":"admin"}' />
          </Form.Item>
          <Form.Item name="attachment_file_ids" label="附件素材 ID">
            <Select
              mode="tags"
              tokenSeparators={[","]}
              placeholder="输入素材 ID"
            />
          </Form.Item>
        </Form>
      </Modal>
      <Drawer
        title="工单详情"
        open={Boolean(detailId)}
        onClose={() => setDetailId(null)}
        width={760}
      >
        {detail && <DetailContent detail={detail} />}
        {detail && (
          <Space direction="vertical" size="middle" className="full-width">
            <Form
              form={transitionForm}
              layout="inline"
              onFinish={(values) =>
                transitionMutation.mutate({ id: detail.id, payload: values })
              }
            >
              <Form.Item name="status" rules={[{ required: true }]}>
                <Select
                  placeholder="新状态"
                  options={statusOptions}
                  style={{ width: 140 }}
                />
              </Form.Item>
              <Form.Item name="comment">
                <Input placeholder="流转备注" />
              </Form.Item>
              <PermissionButton
                htmlType="submit"
                permission="system:work_order:transition"
                loading={transitionMutation.isPending}
              >
                流转
              </PermissionButton>
            </Form>
            <Form
              form={assignForm}
              layout="inline"
              onFinish={(values) =>
                assignMutation.mutate({ id: detail.id, payload: values })
              }
            >
              <Form.Item name="assignee_id" rules={[{ required: true }]}>
                <InputNumber min={1} placeholder="负责人 ID" />
              </Form.Item>
              <Form.Item name="note">
                <Input placeholder="分配备注" />
              </Form.Item>
              <PermissionButton
                htmlType="submit"
                permission="system:work_order:assign"
                loading={assignMutation.isPending}
              >
                分配
              </PermissionButton>
            </Form>
            <Form
              form={attachmentForm}
              layout="inline"
              onFinish={(values) =>
                addAttachmentMutation.mutate({ id: detail.id, payload: values })
              }
            >
              <Form.Item name="upload_file_id" rules={[{ required: true }]}>
                <InputNumber min={1} placeholder="素材 ID" />
              </Form.Item>
              <Form.Item name="description">
                <Input placeholder="附件说明" />
              </Form.Item>
              <PermissionButton
                htmlType="submit"
                permission="system:work_order:attachment"
                loading={addAttachmentMutation.isPending}
              >
                绑定附件
              </PermissionButton>
            </Form>
            <Form
              form={commentForm}
              layout="vertical"
              onFinish={({ body }) =>
                commentMutation.mutate({ id: detail.id, body })
              }
            >
              <Form.Item name="body" rules={[{ required: true }]}>
                <Input.TextArea rows={3} placeholder="添加评论" />
              </Form.Item>
              <PermissionButton
                htmlType="submit"
                icon={<CommentOutlined />}
                permission="system:work_order:comment"
                loading={commentMutation.isPending}
              >
                添加评论
              </PermissionButton>
            </Form>
            <div>
              <h3>附件</h3>
              <Space direction="vertical" className="full-width">
                {detail.attachments.map((attachment) => (
                  <Space key={attachment.id}>
                    <span>
                      #{attachment.upload_file_id}{" "}
                      {attachment.original_name ?? "-"}
                    </span>
                    {attachment.url && <a href={attachment.url}>打开</a>}
                    <Popconfirm
                      title="确认移除附件引用？"
                      onConfirm={() =>
                        deleteAttachmentMutation.mutate({
                          id: detail.id,
                          attachmentId: attachment.id,
                        })
                      }
                    >
                      <PermissionButton
                        size="small"
                        danger
                        permission="system:work_order:attachment"
                      >
                        移除
                      </PermissionButton>
                    </Popconfirm>
                  </Space>
                ))}
              </Space>
            </div>
          </Space>
        )}
      </Drawer>
    </CrudPage>
  );
}

function DetailContent({ detail }: { detail: WorkOrderDetailRecord }) {
  return (
    <Space direction="vertical" size="large" className="full-width">
      <Descriptions bordered size="small" column={2}>
        <Descriptions.Item label="编号">{detail.order_no}</Descriptions.Item>
        <Descriptions.Item label="状态">
          <Tag color={statusColor[detail.status] ?? "default"}>
            {labelOf(statusOptions, detail.status)}
          </Tag>
        </Descriptions.Item>
        <Descriptions.Item label="标题" span={2}>
          {detail.title}
        </Descriptions.Item>
        <Descriptions.Item label="优先级">
          <Tag color={priorityColor[detail.priority] ?? "default"}>
            {labelOf(priorityOptions, detail.priority)}
          </Tag>
        </Descriptions.Item>
        <Descriptions.Item label="分类">
          {detail.category ?? "-"}
        </Descriptions.Item>
        <Descriptions.Item label="创建人">
          {detail.creator_id ?? "-"}
        </Descriptions.Item>
        <Descriptions.Item label="负责人">
          {detail.assignee_id ?? "-"}
        </Descriptions.Item>
        <Descriptions.Item label="截止时间" span={2}>
          {detail.due_at ?? "-"}
        </Descriptions.Item>
        <Descriptions.Item label="描述" span={2}>
          {detail.description}
        </Descriptions.Item>
      </Descriptions>
      <div>
        <h3>评论与流转</h3>
        <Timeline
          items={detail.comments.map((comment) => ({
            children: (
              <Space direction="vertical" size={2}>
                <span>{comment.body}</span>
                <span className="text-muted">
                  {comment.comment_type} · {comment.created_at}
                </span>
              </Space>
            ),
          }))}
        />
      </div>
    </Space>
  );
}
