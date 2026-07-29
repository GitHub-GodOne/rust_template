import {
  CheckCircleOutlined,
  CopyOutlined,
  DeleteOutlined,
  DownloadOutlined,
  EditOutlined,
  EyeOutlined,
  MessageOutlined,
  PlusOutlined,
  UploadOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Drawer,
  Form,
  Input,
  Modal,
  Popconfirm,
  Progress,
  Select,
  Space,
  Tag,
  Tooltip,
  Typography,
  Upload,
  message,
} from "antd";
import type { UploadProps } from "antd";
import type { ColumnsType } from "antd/es/table";
import { useEffect, useState } from "react";
import {
  type ChatAttachmentParams,
  type ChatAttachmentRecord,
  type ChatAttachmentSource,
  type ChatMessageDetailRecord,
  type ChatMessageRecord,
  copyChatMessage,
  createChatMessage,
  deleteChatMessage,
  fetchChatMessage,
  fetchChatMessages,
  markChatMessageSent,
  updateChatMessage,
} from "../../../api/admin/chatMessages";
import {
  downloadManagedFile,
  fetchFileRoots,
  uploadManagedFile,
} from "../../../api/admin/files";
import {
  fetchStorageBuckets,
  fetchStorageProfiles,
} from "../../../api/admin/storage";
import { downloadUpload, uploadMaterial } from "../../../api/admin/uploads";
import { CrudPage } from "../../../components/admin/CrudPage";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { formatBytes } from "../uploads/uploadTaskManager";

type ChatFormValues = {
  title: string;
  content: string;
  status: string;
  remark?: string | null;
};

type DraftAttachment = ChatAttachmentParams & {
  key: string;
  display_name: string;
  size_bytes: number;
};

const statusOptions = [
  { label: "草稿", value: "draft" },
  { label: "已发送", value: "sent" },
  { label: "已归档", value: "archived" },
];

function statusTag(status: string) {
  const map: Record<string, { color: string; text: string }> = {
    draft: { color: "default", text: "草稿" },
    sent: { color: "green", text: "已发送" },
    archived: { color: "blue", text: "已归档" },
  };
  const item = map[status] ?? { color: "default", text: status };
  return <Tag color={item.color}>{item.text}</Tag>;
}

function attachmentKey(source: ChatAttachmentSource, id: string | number) {
  return `${source}:${id}`;
}

function triggerDownload(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  link.click();
  URL.revokeObjectURL(url);
}

function attachmentSourceText(source: string) {
  return source === "material" ? "素材库" : "本地文件";
}

export function ChatMessagesPage() {
  const [page, setPage] = useState(1);
  const [keyword, setKeyword] = useState("");
  const [status, setStatus] = useState<string>();
  const [modalOpen, setModalOpen] = useState(false);
  const [editing, setEditing] = useState<ChatMessageDetailRecord | null>(null);
  const [detailId, setDetailId] = useState<number>();
  const [attachments, setAttachments] = useState<DraftAttachment[]>([]);
  const [attachmentSource, setAttachmentSource] =
    useState<ChatAttachmentSource>("material");
  const [storageProfileId, setStorageProfileId] = useState<number>();
  const [storageBucketId, setStorageBucketId] = useState<number>();
  const [materialPrefix, setMaterialPrefix] = useState("chat/");
  const [fileRootKey, setFileRootKey] = useState<string>();
  const [filePath, setFilePath] = useState("");
  const [uploadingName, setUploadingName] = useState<string>();
  const [uploadPercent, setUploadPercent] = useState(0);
  const [form] = Form.useForm<ChatFormValues>();
  const queryClient = useQueryClient();

  const messagesQuery = useQuery({
    queryKey: ["admin-chat-messages", page, keyword, status],
    queryFn: () =>
      fetchChatMessages({
        page,
        page_size: 10,
        keyword: keyword || undefined,
        status,
      }),
  });

  const detailQuery = useQuery({
    queryKey: ["admin-chat-message", detailId],
    enabled: Boolean(detailId),
    queryFn: () => fetchChatMessage(detailId ?? 0),
  });

  const profilesQuery = useQuery({
    queryKey: ["admin-storage-profiles"],
    queryFn: () => fetchStorageProfiles({ page_size: 100 }),
  });
  const profiles = profilesQuery.data?.items ?? [];

  useEffect(() => {
    if (!storageProfileId && profiles.length > 0) {
      setStorageProfileId(
        profiles.find((profile) => profile.is_default)?.id ?? profiles[0].id,
      );
    }
  }, [profiles, storageProfileId]);

  const bucketsQuery = useQuery({
    queryKey: ["admin-storage-buckets", storageProfileId],
    enabled: Boolean(storageProfileId),
    queryFn: () => fetchStorageBuckets(storageProfileId ?? 0),
  });
  const buckets = bucketsQuery.data ?? [];

  useEffect(() => {
    if (buckets.length === 0) {
      setStorageBucketId(undefined);
      return;
    }
    if (
      !storageBucketId ||
      !buckets.some((bucket) => bucket.id === storageBucketId)
    ) {
      setStorageBucketId(
        buckets.find((bucket) => bucket.is_default)?.id ?? buckets[0].id,
      );
    }
  }, [buckets, storageBucketId]);

  const rootsQuery = useQuery({
    queryKey: ["admin-file-roots"],
    queryFn: fetchFileRoots,
  });
  const roots = rootsQuery.data ?? [];

  useEffect(() => {
    if (!fileRootKey && roots.length > 0) {
      setFileRootKey(roots[0].key);
    }
  }, [fileRootKey, roots]);

  const saveMutation = useMutation({
    mutationFn: (values: ChatFormValues) => {
      const payload = {
        title: values.title,
        content: values.content,
        status: values.status,
        remark: values.remark,
        attachments: attachments.map(
          ({ key: _key, ...attachment }) => attachment,
        ),
      };
      if (editing) {
        return updateChatMessage(editing.id, payload);
      }
      return createChatMessage(payload);
    },
    onSuccess: () => {
      message.success(editing ? "聊天内容已更新" : "聊天内容已创建");
      closeModal();
      queryClient.invalidateQueries({ queryKey: ["admin-chat-messages"] });
    },
    onError: (error) => {
      message.error(error instanceof Error ? error.message : "保存失败");
    },
  });

  const deleteMutation = useMutation({
    mutationFn: deleteChatMessage,
    onSuccess: () => {
      message.success("聊天内容已删除");
      queryClient.invalidateQueries({ queryKey: ["admin-chat-messages"] });
    },
    onError: (error) => {
      message.error(error instanceof Error ? error.message : "删除失败");
    },
  });

  const markSentMutation = useMutation({
    mutationFn: markChatMessageSent,
    onSuccess: () => {
      message.success("已标记发送");
      queryClient.invalidateQueries({ queryKey: ["admin-chat-messages"] });
    },
    onError: (error) => {
      message.error(error instanceof Error ? error.message : "标记失败");
    },
  });

  const copyMutation = useMutation({
    mutationFn: copyChatMessage,
    onSuccess: async (record) => {
      await navigator.clipboard.writeText(record.content);
      message.success("内容已复制");
      queryClient.invalidateQueries({ queryKey: ["admin-chat-messages"] });
      queryClient.invalidateQueries({
        queryKey: ["admin-chat-message", record.id],
      });
    },
    onError: (error) => {
      message.error(error instanceof Error ? error.message : "复制失败");
    },
  });

  const downloadMutation = useMutation({
    mutationFn: async (attachment: ChatAttachmentRecord | DraftAttachment) => {
      if (attachment.source_type === "material") {
        if (!attachment.upload_file_id) {
          throw new Error("素材附件缺少文件 ID");
        }
        const blob = await downloadUpload(attachment.upload_file_id);
        return { blob, filename: attachment.display_name };
      }
      if (!attachment.file_root_key || !attachment.file_path) {
        throw new Error("本地文件附件缺少路径");
      }
      const blob = await downloadManagedFile({
        root_key: attachment.file_root_key,
        path: attachment.file_path,
      });
      return { blob, filename: attachment.display_name };
    },
    onSuccess: ({ blob, filename }) => {
      triggerDownload(blob, filename);
    },
    onError: (error) => {
      message.error(error instanceof Error ? error.message : "下载失败");
    },
  });

  const uploadProps: UploadProps = {
    multiple: false,
    showUploadList: false,
    customRequest: ({ file, onSuccess, onError }) => {
      const uploadFile = file as File;
      setUploadingName(uploadFile.name);
      setUploadPercent(0);
      const upload =
        attachmentSource === "material"
          ? uploadMaterial(uploadFile, {
              storage_profile_id: storageProfileId,
              storage_bucket_id: storageBucketId,
              prefix: materialPrefix || undefined,
              category: "chat",
              visibility: "private",
              onUploadProgress: setUploadPercent,
            }).then((record) => ({
              key: attachmentKey("material", record.id),
              source_type: "material" as const,
              upload_file_id: record.id,
              display_name: record.original_name,
              mime_type: record.mime_type,
              size_bytes: record.size_bytes,
            }))
          : uploadManagedFile(uploadFile, {
              root_key: fileRootKey ?? "",
              path: filePath || undefined,
              onUploadProgress: setUploadPercent,
            }).then((record) => ({
              key: attachmentKey("local_file", record.path),
              source_type: "local_file" as const,
              file_root_key: fileRootKey,
              file_path: record.path,
              display_name: record.name,
              mime_type: record.mime_type,
              size_bytes: record.size_bytes,
            }));

      upload
        .then((attachment) => {
          setAttachments((current) => [...current, attachment]);
          setUploadingName(undefined);
          setUploadPercent(100);
          message.success("附件已上传");
          onSuccess?.("ok");
        })
        .catch((error) => {
          setUploadingName(undefined);
          setUploadPercent(0);
          message.error(error instanceof Error ? error.message : "上传失败");
          onError?.(error as Error);
        });
    },
  };

  const openCreate = () => {
    setEditing(null);
    setAttachments([]);
    form.setFieldsValue({ status: "draft" });
    setModalOpen(true);
  };

  const openEdit = async (record: ChatMessageRecord) => {
    try {
      const detail = await fetchChatMessage(record.id);
      setEditing(detail);
      setAttachments(
        detail.attachments.map((attachment) => ({
          key: attachmentKey(attachment.source_type, attachment.id),
          source_type: attachment.source_type,
          upload_file_id: attachment.upload_file_id,
          file_root_key: attachment.file_root_key,
          file_path: attachment.file_path,
          display_name: attachment.display_name,
          mime_type: attachment.mime_type,
          size_bytes: attachment.size_bytes,
        })),
      );
      form.setFieldsValue({
        title: detail.title,
        content: detail.content,
        status: detail.status,
        remark: detail.remark,
      });
      setModalOpen(true);
    } catch (error) {
      message.error(error instanceof Error ? error.message : "加载失败");
    }
  };

  const closeModal = () => {
    setModalOpen(false);
    setEditing(null);
    setAttachments([]);
    setUploadingName(undefined);
    setUploadPercent(0);
    form.resetFields();
  };

  const removeAttachment = (key: string) => {
    setAttachments((current) => current.filter((item) => item.key !== key));
  };

  const columns: ColumnsType<ChatMessageRecord> = [
    { title: "ID", dataIndex: "id", width: 72 },
    {
      title: "标题",
      dataIndex: "title",
      width: 180,
      render: (value: string) => (
        <Typography.Text strong>{value}</Typography.Text>
      ),
    },
    {
      title: "内容",
      dataIndex: "content",
      width: 320,
      render: (value: string) => (
        <Typography.Text
          copyable={{ text: value }}
          ellipsis
          style={{ maxWidth: 300 }}
        >
          {value}
        </Typography.Text>
      ),
    },
    {
      title: "状态",
      dataIndex: "status",
      width: 100,
      render: statusTag,
    },
    { title: "附件", dataIndex: "attachment_count", width: 90 },
    { title: "复制", dataIndex: "copy_count", width: 90 },
    {
      title: "发送时间",
      dataIndex: "sent_at",
      width: 180,
      render: (value?: string | null) => value ?? "-",
    },
    {
      title: "更新时间",
      dataIndex: "updated_at",
      width: 180,
    },
    {
      title: "操作",
      key: "actions",
      fixed: "right",
      width: 280,
      render: (_, record) => (
        <Space size={6} wrap>
          <Tooltip title="详情">
            <PermissionButton
              size="small"
              icon={<EyeOutlined />}
              permission="system:chat:detail"
              onClick={() => setDetailId(record.id)}
            />
          </Tooltip>
          <Tooltip title="复制发送内容">
            <PermissionButton
              size="small"
              icon={<CopyOutlined />}
              permission="system:chat:copy"
              disabled={copyMutation.isPending}
              onClick={() => copyMutation.mutate(record.id)}
            />
          </Tooltip>
          {record.status !== "sent" ? (
            <Tooltip title="标记发送">
              <PermissionButton
                size="small"
                icon={<CheckCircleOutlined />}
                permission="system:chat:update"
                disabled={markSentMutation.isPending}
                onClick={() => markSentMutation.mutate(record.id)}
              />
            </Tooltip>
          ) : null}
          <Tooltip title="编辑">
            <PermissionButton
              size="small"
              icon={<EditOutlined />}
              permission="system:chat:update"
              onClick={() => openEdit(record)}
            />
          </Tooltip>
          <Popconfirm
            title="确认删除这条聊天内容？"
            onConfirm={() => deleteMutation.mutate(record.id)}
          >
            <PermissionButton
              danger
              size="small"
              icon={<DeleteOutlined />}
              permission="system:chat:delete"
            />
          </Popconfirm>
        </Space>
      ),
    },
  ];

  const detail = detailQuery.data;

  return (
    <CrudPage
      title="聊天管理"
      subtitle="保存待发送内容并管理附件"
      breadcrumb={["系统管理", "聊天管理"]}
      icon={<MessageOutlined />}
      toolbar={
        <Space wrap>
          <Input.Search
            allowClear
            placeholder="搜索标题、内容或备注"
            style={{ width: 260 }}
            onSearch={(value) => {
              setKeyword(value.trim());
              setPage(1);
            }}
          />
          <Select
            allowClear
            placeholder="状态"
            style={{ width: 140 }}
            options={statusOptions}
            value={status}
            onChange={(value) => {
              setStatus(value);
              setPage(1);
            }}
          />
          <PermissionButton
            type="primary"
            icon={<PlusOutlined />}
            permission="system:chat:create"
            onClick={openCreate}
          >
            新增
          </PermissionButton>
        </Space>
      }
    >
      <DataTable<ChatMessageRecord>
        loading={messagesQuery.isLoading}
        columns={columns}
        dataSource={messagesQuery.data?.items ?? []}
        pagination={{
          current: page,
          pageSize: messagesQuery.data?.page_size ?? 10,
          total: messagesQuery.data?.total ?? 0,
          onChange: setPage,
        }}
      />

      <Modal
        title={editing ? "编辑聊天内容" : "新增聊天内容"}
        open={modalOpen}
        onCancel={closeModal}
        onOk={() => form.submit()}
        confirmLoading={saveMutation.isPending}
        width="min(860px, 94vw)"
        destroyOnHidden
      >
        <Form<ChatFormValues>
          form={form}
          layout="vertical"
          initialValues={{ status: "draft" }}
          onFinish={(values) => saveMutation.mutate(values)}
        >
          <Form.Item
            label="标题"
            name="title"
            rules={[{ required: true, message: "请输入标题" }]}
          >
            <Input placeholder="例如：客户跟进话术" />
          </Form.Item>
          <Form.Item
            label="发送内容"
            name="content"
            rules={[{ required: true, message: "请输入发送内容" }]}
          >
            <Input.TextArea rows={8} placeholder="粘贴要发送的完整内容" />
          </Form.Item>
          <Space align="start" wrap style={{ width: "100%" }}>
            <Form.Item label="状态" name="status" style={{ width: 180 }}>
              <Select options={statusOptions} />
            </Form.Item>
            <Form.Item label="备注" name="remark" style={{ width: 360 }}>
              <Input placeholder="可选" />
            </Form.Item>
          </Space>
        </Form>

        <Space direction="vertical" size={12} style={{ width: "100%" }}>
          <Space wrap>
            <Select<ChatAttachmentSource>
              value={attachmentSource}
              style={{ width: 140 }}
              options={[
                { label: "素材库", value: "material" },
                { label: "本地文件", value: "local_file" },
              ]}
              onChange={setAttachmentSource}
            />
            {attachmentSource === "material" ? (
              <>
                <Select
                  placeholder="存储配置"
                  style={{ width: 180 }}
                  value={storageProfileId}
                  options={profiles.map((profile) => ({
                    label: profile.name,
                    value: profile.id,
                  }))}
                  onChange={setStorageProfileId}
                />
                <Select
                  placeholder="存储桶"
                  style={{ width: 180 }}
                  value={storageBucketId}
                  options={buckets.map((bucket) => ({
                    label: bucket.name,
                    value: bucket.id,
                  }))}
                  onChange={setStorageBucketId}
                />
                <Input
                  placeholder="前缀"
                  style={{ width: 160 }}
                  value={materialPrefix}
                  onChange={(event) => setMaterialPrefix(event.target.value)}
                />
              </>
            ) : (
              <>
                <Select
                  placeholder="文件根目录"
                  style={{ width: 180 }}
                  value={fileRootKey}
                  options={roots.map((root) => ({
                    label: root.name,
                    value: root.key,
                  }))}
                  onChange={setFileRootKey}
                />
                <Input
                  placeholder="上传目录，如 chat"
                  style={{ width: 220 }}
                  value={filePath}
                  onChange={(event) => setFilePath(event.target.value)}
                />
              </>
            )}
            <Upload {...uploadProps}>
              <PermissionButton
                icon={<UploadOutlined />}
                permission={
                  attachmentSource === "material"
                    ? "system:upload:create"
                    : "system:file:create"
                }
              >
                上传附件
              </PermissionButton>
            </Upload>
          </Space>

          {uploadingName ? (
            <div>
              <Typography.Text type="secondary">
                {uploadingName}
              </Typography.Text>
              <Progress percent={uploadPercent} size="small" />
            </div>
          ) : null}

          <DataTable<DraftAttachment>
            rowKey="key"
            size="small"
            columns={[
              {
                title: "来源",
                dataIndex: "source_type",
                width: 100,
                render: attachmentSourceText,
              },
              { title: "文件名", dataIndex: "display_name" },
              {
                title: "大小",
                dataIndex: "size_bytes",
                width: 120,
                render: (value: number) => formatBytes(value),
              },
              {
                title: "操作",
                width: 120,
                render: (_, record) => (
                  <Space>
                    <Tooltip title="下载">
                      <PermissionButton
                        size="small"
                        icon={<DownloadOutlined />}
                        permission={
                          record.source_type === "material"
                            ? "system:upload:download"
                            : "system:file:download"
                        }
                        onClick={() => downloadMutation.mutate(record)}
                      />
                    </Tooltip>
                    <PermissionButton
                      danger
                      size="small"
                      icon={<DeleteOutlined />}
                      permission="system:chat:update"
                      onClick={() => removeAttachment(record.key)}
                    />
                  </Space>
                ),
              },
            ]}
            dataSource={attachments}
            pagination={false}
          />
        </Space>
      </Modal>

      <Drawer
        title="聊天内容详情"
        open={Boolean(detailId)}
        onClose={() => setDetailId(undefined)}
        width="min(720px, 94vw)"
      >
        {detail ? (
          <Space direction="vertical" size={16} style={{ width: "100%" }}>
            <Space wrap>
              <Typography.Title level={4} style={{ margin: 0 }}>
                {detail.title}
              </Typography.Title>
              {statusTag(detail.status)}
            </Space>
            <Typography.Paragraph
              copyable={{ text: detail.content }}
              style={{ whiteSpace: "pre-wrap" }}
            >
              {detail.content}
            </Typography.Paragraph>
            <Space wrap>
              <PermissionButton
                icon={<CopyOutlined />}
                permission="system:chat:copy"
                onClick={() => copyMutation.mutate(detail.id)}
              >
                复制内容
              </PermissionButton>
              <Typography.Text type="secondary">
                已复制 {detail.copy_count} 次
              </Typography.Text>
            </Space>
            <DataTable<ChatAttachmentRecord>
              rowKey="id"
              columns={[
                {
                  title: "来源",
                  dataIndex: "source_type",
                  width: 100,
                  render: attachmentSourceText,
                },
                { title: "文件名", dataIndex: "display_name" },
                {
                  title: "大小",
                  dataIndex: "size_bytes",
                  width: 120,
                  render: (value: number) => formatBytes(value),
                },
                {
                  title: "操作",
                  width: 100,
                  render: (_, record) => (
                    <PermissionButton
                      size="small"
                      icon={<DownloadOutlined />}
                      permission={
                        record.source_type === "material"
                          ? "system:upload:download"
                          : "system:file:download"
                      }
                      onClick={() => downloadMutation.mutate(record)}
                    >
                      下载
                    </PermissionButton>
                  ),
                },
              ]}
              dataSource={detail.attachments}
              pagination={false}
            />
          </Space>
        ) : null}
      </Drawer>
    </CrudPage>
  );
}
