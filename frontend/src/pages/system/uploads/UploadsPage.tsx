import {
  CloudDownloadOutlined,
  CloudUploadOutlined,
  DeleteOutlined,
  EditOutlined,
  EyeOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Descriptions,
  Drawer,
  Form,
  Input,
  Modal,
  Popconfirm,
  Select,
  Space,
  Tag,
  Upload,
  message,
} from "antd";
import type { UploadProps } from "antd";
import type { ColumnsType } from "antd/es/table";
import { useState } from "react";
import {
  type UpdateUploadParams,
  type UploadRecord,
  deleteUpload,
  downloadUpload,
  fetchUploads,
  updateUpload,
  uploadMaterial,
} from "../../../api/admin/uploads";
import { CrudPage } from "../../../components/admin/CrudPage";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";

function formatBytes(value: number) {
  if (value < 1024) {
    return `${value} B`;
  }
  if (value < 1024 * 1024) {
    return `${(value / 1024).toFixed(1)} KB`;
  }
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}

export function UploadsPage() {
  const [page, setPage] = useState(1);
  const [keyword, setKeyword] = useState("");
  const [status, setStatus] = useState<string>();
  const [detail, setDetail] = useState<UploadRecord | null>(null);
  const [editing, setEditing] = useState<UploadRecord | null>(null);
  const [form] = Form.useForm<UpdateUploadParams>();
  const queryClient = useQueryClient();

  const uploadsQuery = useQuery({
    queryKey: ["admin-uploads", page, keyword, status],
    queryFn: () =>
      fetchUploads({
        page,
        page_size: 10,
        keyword: keyword || undefined,
        status,
      }),
  });

  const uploadMutation = useMutation({
    mutationFn: uploadMaterial,
    onSuccess: () => {
      message.success("素材已上传");
      queryClient.invalidateQueries({ queryKey: ["admin-uploads"] });
    },
  });
  const updateMutation = useMutation({
    mutationFn: (values: UpdateUploadParams) =>
      updateUpload(editing?.id ?? 0, values),
    onSuccess: () => {
      message.success("素材信息已保存");
      setEditing(null);
      queryClient.invalidateQueries({ queryKey: ["admin-uploads"] });
    },
  });
  const deleteMutation = useMutation({
    mutationFn: deleteUpload,
    onSuccess: () => {
      message.success("素材已删除");
      queryClient.invalidateQueries({ queryKey: ["admin-uploads"] });
    },
  });
  const downloadMutation = useMutation({
    mutationFn: downloadUpload,
  });

  const uploadProps: UploadProps = {
    multiple: false,
    showUploadList: false,
    customRequest: ({ file, onSuccess, onError }) => {
      uploadMutation.mutate(file as File, {
        onSuccess: () => onSuccess?.("ok"),
        onError: (error) => onError?.(error as Error),
      });
    },
  };

  const columns: ColumnsType<UploadRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    { title: "文件名", dataIndex: "original_name", width: 260, ellipsis: true },
    {
      title: "分类",
      dataIndex: "category",
      width: 120,
      render: (value) => (value ? <Tag>{value}</Tag> : "-"),
    },
    { title: "MIME", dataIndex: "mime_type", width: 180, ellipsis: true },
    { title: "大小", dataIndex: "size_bytes", width: 120, render: formatBytes },
    {
      title: "可见性",
      dataIndex: "visibility",
      width: 100,
      render: (value) => (
        <Tag color={value === "public" ? "green" : "default"}>{value}</Tag>
      ),
    },
    {
      title: "状态",
      dataIndex: "status",
      width: 100,
      render: (value) => (
        <Tag color={value === "active" ? "blue" : "red"}>{value}</Tag>
      ),
    },
    { title: "时间", dataIndex: "created_at", width: 220 },
    {
      title: "操作",
      key: "actions",
      width: 260,
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            icon={<EyeOutlined />}
            permission="system:upload:detail"
            onClick={() => setDetail(record)}
          >
            详情
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<CloudDownloadOutlined />}
            permission="system:upload:download"
            loading={downloadMutation.isPending}
            onClick={() => {
              downloadMutation.mutate(record.id, {
                onSuccess: (blob) => {
                  const url = URL.createObjectURL(blob);
                  const link = document.createElement("a");
                  link.href = url;
                  link.download = record.original_name;
                  link.click();
                  URL.revokeObjectURL(url);
                },
              });
            }}
          >
            下载
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:upload:update"
            onClick={() => {
              setEditing(record);
              form.setFieldsValue({
                category: record.category,
                tags: record.tags,
                visibility: record.visibility,
                status: record.status,
              });
            }}
          >
            编辑
          </PermissionButton>
          <Popconfirm
            title="确认删除素材？"
            onConfirm={() => deleteMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              icon={<DeleteOutlined />}
              permission="system:upload:delete"
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
      title="文件上传 / 素材库"
      subtitle="统一管理图片、附件、导入模板和导出文件"
      breadcrumb={["系统管理", "素材库"]}
      icon={<CloudUploadOutlined />}
      toolbar={
        <Space wrap>
          <Input.Search
            allowClear
            placeholder="搜索文件名、分类"
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
            options={[
              { value: "active", label: "正常" },
              { value: "deleted", label: "已删除" },
            ]}
            className="admin-filter-select"
          />
        </Space>
      }
    >
      <Space direction="vertical" size="middle" className="full-width">
        <div className="upload-panel">
          <Upload.Dragger {...uploadProps} disabled={uploadMutation.isPending}>
            <p className="ant-upload-drag-icon">
              <CloudUploadOutlined />
            </p>
            <p className="ant-upload-text">点击或拖拽文件上传到素材库</p>
            <p className="ant-upload-hint">
              后端会校验扩展名、大小并保存素材元数据。
            </p>
          </Upload.Dragger>
        </div>
        <DataTable<UploadRecord>
          columns={columns}
          dataSource={uploadsQuery.data?.items ?? []}
          loading={uploadsQuery.isLoading}
          pagination={{
            current: page,
            total: uploadsQuery.data?.total ?? 0,
            onChange: setPage,
          }}
        />
      </Space>
      <Drawer
        title="素材详情"
        open={Boolean(detail)}
        onClose={() => setDetail(null)}
        width={640}
      >
        {detail && (
          <Descriptions column={1} bordered size="small">
            <Descriptions.Item label="文件名">
              {detail.original_name}
            </Descriptions.Item>
            <Descriptions.Item label="对象键">
              {detail.object_key}
            </Descriptions.Item>
            <Descriptions.Item label="URL">{detail.url}</Descriptions.Item>
            <Descriptions.Item label="MIME">
              {detail.mime_type ?? "-"}
            </Descriptions.Item>
            <Descriptions.Item label="大小">
              {formatBytes(detail.size_bytes)}
            </Descriptions.Item>
            <Descriptions.Item label="SHA256">
              {detail.sha256}
            </Descriptions.Item>
            <Descriptions.Item label="分类">
              {detail.category ?? "-"}
            </Descriptions.Item>
            <Descriptions.Item label="标签">
              {detail.tags ?? "-"}
            </Descriptions.Item>
            <Descriptions.Item label="状态">{detail.status}</Descriptions.Item>
            <Descriptions.Item label="上传时间">
              {detail.created_at}
            </Descriptions.Item>
          </Descriptions>
        )}
      </Drawer>
      <Modal
        title="编辑素材"
        open={Boolean(editing)}
        onCancel={() => setEditing(null)}
        onOk={() => form.submit()}
        confirmLoading={updateMutation.isPending}
      >
        <Form
          form={form}
          layout="vertical"
          onFinish={(values) => updateMutation.mutate(values)}
        >
          <Form.Item name="category" label="分类">
            <Input />
          </Form.Item>
          <Form.Item name="tags" label="标签">
            <Input />
          </Form.Item>
          <Form.Item name="visibility" label="可见性">
            <Select
              options={[
                { value: "private", label: "私有" },
                { value: "public", label: "公开" },
              ]}
            />
          </Form.Item>
          <Form.Item name="status" label="状态">
            <Select
              options={[
                { value: "active", label: "正常" },
                { value: "deleted", label: "已删除" },
              ]}
            />
          </Form.Item>
        </Form>
      </Modal>
    </CrudPage>
  );
}
