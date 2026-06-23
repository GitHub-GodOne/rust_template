import {
  DeleteOutlined,
  DownloadOutlined,
  EditOutlined,
  ExperimentOutlined,
  EyeOutlined,
  PictureOutlined,
  PlusOutlined,
  RedoOutlined,
  ReloadOutlined,
} from "@ant-design/icons";
import {
  useMutation,
  useQueries,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import {
  Button,
  Card,
  Descriptions,
  Drawer,
  Form,
  Image,
  Input,
  InputNumber,
  List,
  Modal,
  Popconfirm,
  Select,
  Space,
  Spin,
  Switch,
  Tabs,
  Tag,
  Typography,
  Upload,
  message,
} from "antd";
import type { UploadFile, UploadProps } from "antd";
import type { ColumnsType } from "antd/es/table";
import { useEffect, useMemo, useState } from "react";
import {
  type AiImageConfigRecord,
  type AiImageGenerationRecord,
  type SaveAiImageConfigParams,
  createAiImageConfig,
  deleteAiImageConfig,
  downloadAiImageGeneration,
  fetchAiImageConfigs,
  fetchAiImageGenerations,
  generateAiImages,
  previewAiImageGeneration,
  updateAiImageConfig,
} from "../../../api/admin/aiImages";
import {
  fetchStorageBuckets,
  fetchStorageProfiles,
} from "../../../api/admin/storage";
import { type UploadRecord, fetchUploads } from "../../../api/admin/uploads";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";

const sizeOptions = [
  { value: "1024x1024", label: "1024 x 1024" },
  { value: "1024x1536", label: "1024 x 1536" },
  { value: "1536x1024", label: "1536 x 1024" },
];

const qualityOptions = [
  { value: "high", label: "高质量" },
  { value: "medium", label: "中质量" },
  { value: "low", label: "低质量" },
];

const saveModeOptions = [
  { value: "local", label: "本地目录" },
  { value: "storage", label: "存储桶 / 素材库" },
];

type ConfigFormValues = SaveAiImageConfigParams;

type GenerateFormValues = {
  config_key: string;
  prompt: string;
  model?: string;
  size?: string;
  quality?: string;
  n?: number;
};

function statusTag(status: string) {
  if (status === "success") {
    return <Tag color="green">成功</Tag>;
  }
  if (status === "failed") {
    return <Tag color="red">失败</Tag>;
  }
  return <Tag>{status}</Tag>;
}

function saveModeTag(mode: string) {
  return <Tag color={mode === "storage" ? "blue" : "gold"}>{mode}</Tag>;
}

export function AiImagesPage() {
  const [configOpen, setConfigOpen] = useState(false);
  const [editingConfig, setEditingConfig] =
    useState<AiImageConfigRecord | null>(null);
  const [configKeyword, setConfigKeyword] = useState("");
  const [page, setPage] = useState(1);
  const [keyword, setKeyword] = useState("");
  const [selectedUploadIds, setSelectedUploadIds] = useState<number[]>([]);
  const [selectedUploads, setSelectedUploads] = useState<UploadRecord[]>([]);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickerKeyword, setPickerKeyword] = useState("");
  const [referenceFiles, setReferenceFiles] = useState<UploadFile[]>([]);
  const [previewing, setPreviewing] = useState<AiImageGenerationRecord | null>(
    null,
  );
  const [regenerating, setRegenerating] =
    useState<AiImageGenerationRecord | null>(null);
  const [previewObjectUrl, setPreviewObjectUrl] = useState<string>();
  const [previewLoading, setPreviewLoading] = useState(false);
  const [latestBatch, setLatestBatch] = useState<AiImageGenerationRecord[]>([]);
  const [configForm] = Form.useForm<ConfigFormValues>();
  const [generateForm] = Form.useForm<GenerateFormValues>();
  const queryClient = useQueryClient();

  useEffect(() => {
    return () => {
      if (previewObjectUrl) {
        URL.revokeObjectURL(previewObjectUrl);
      }
    };
  }, [previewObjectUrl]);

  const configsQuery = useQuery({
    queryKey: ["admin-ai-image-configs"],
    queryFn: fetchAiImageConfigs,
  });
  const generationsQuery = useQuery({
    queryKey: ["admin-ai-image-generations", page, keyword],
    queryFn: () =>
      fetchAiImageGenerations({
        page,
        page_size: 10,
        keyword: keyword || undefined,
      }),
  });
  const storageProfilesQuery = useQuery({
    queryKey: ["admin-storage-profiles", "ai-images"],
    queryFn: () => fetchStorageProfiles({ page_size: 100 }),
  });
  const uploadsQuery = useQuery({
    queryKey: ["admin-ai-image-upload-picker", pickerKeyword],
    enabled: pickerOpen,
    queryFn: () =>
      fetchUploads({
        page: 1,
        page_size: 50,
        keyword: pickerKeyword || undefined,
      }),
  });

  const configs = configsQuery.data ?? [];
  const filteredConfigs = useMemo(() => {
    const nextKeyword = configKeyword.trim().toLowerCase();
    if (!nextKeyword) {
      return configs;
    }
    return configs.filter(
      (item) =>
        item.name.toLowerCase().includes(nextKeyword) ||
        item.key.toLowerCase().includes(nextKeyword),
    );
  }, [configKeyword, configs]);
  const enabledConfigs = configs.filter((config) => config.enabled);
  const selectedConfigKey = Form.useWatch("config_key", generateForm);
  const selectedConfig = configs.find(
    (config) => config.key === selectedConfigKey,
  );
  const storageProfiles = storageProfilesQuery.data?.items ?? [];
  const bucketQueries = useQueries({
    queries: storageProfiles.map((profile) => ({
      queryKey: ["admin-storage-buckets", profile.id, "ai-images"],
      queryFn: () => fetchStorageBuckets(profile.id),
    })),
  });
  const storageBuckets = bucketQueries.flatMap((query) => query.data ?? []);
  const selectedBucket = storageBuckets.find(
    (bucket) => bucket.id === selectedConfig?.storage_bucket_id,
  );

  useEffect(() => {
    if (!selectedConfigKey && enabledConfigs[0]) {
      generateForm.setFieldValue("config_key", enabledConfigs[0].key);
    }
  }, [enabledConfigs, generateForm, selectedConfigKey]);

  useEffect(() => {
    if (!selectedConfig) {
      return;
    }
    generateForm.setFieldsValue({
      model: selectedConfig.model,
      size: selectedConfig.size,
      quality: selectedConfig.quality,
      n: selectedConfig.n,
    });
  }, [generateForm, selectedConfig]);

  const saveConfigMutation = useMutation({
    mutationFn: (values: ConfigFormValues) =>
      editingConfig
        ? updateAiImageConfig(editingConfig.key, values)
        : createAiImageConfig(values),
    onSuccess: () => {
      message.success("配置已保存");
      setConfigOpen(false);
      setEditingConfig(null);
      queryClient.invalidateQueries({ queryKey: ["admin-ai-image-configs"] });
    },
  });

  const deleteConfigMutation = useMutation({
    mutationFn: deleteAiImageConfig,
    onSuccess: () => {
      message.success("配置已删除");
      queryClient.invalidateQueries({ queryKey: ["admin-ai-image-configs"] });
    },
  });

  const generateMutation = useMutation({
    mutationFn: generateAiImages,
    onSuccess: (batch) => {
      message.success(`已生成 ${batch.items.length} 张图片`);
      setLatestBatch(batch.items);
      queryClient.invalidateQueries({
        queryKey: ["admin-ai-image-generations"],
      });
    },
  });

  const regenerateMutation = useMutation({
    mutationFn: generateAiImages,
    onSuccess: (batch) => {
      message.success(`已重新生成 ${batch.items.length} 张图片`);
      setLatestBatch(batch.items);
      queryClient.invalidateQueries({
        queryKey: ["admin-ai-image-generations"],
      });
      setRegenerating(null);
    },
  });

  const configColumns: ColumnsType<AiImageConfigRecord> = [
    { title: "配置键", dataIndex: "key", width: 180 },
    { title: "名称", dataIndex: "name", width: 180 },
    {
      title: "状态",
      dataIndex: "enabled",
      width: 90,
      render: (value) => <Switch checked={value} disabled />,
    },
    { title: "Base URL", dataIndex: "base_url", width: 260, ellipsis: true },
    { title: "模型", dataIndex: "model", width: 140 },
    { title: "尺寸", dataIndex: "size", width: 120 },
    { title: "质量", dataIndex: "quality", width: 100 },
    {
      title: "保存位置",
      dataIndex: "save_mode",
      width: 120,
      render: (value) => saveModeTag(value),
    },
    {
      title: "Key",
      dataIndex: "api_key_configured",
      width: 90,
      render: (value) => (
        <Tag color={value ? "green" : "default"}>
          {value ? "已配置" : "未配置"}
        </Tag>
      ),
    },
    {
      title: "操作",
      key: "actions",
      width: 180,
      fixed: "right",
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:ai_image:config"
            onClick={() => {
              setEditingConfig(record);
              configForm.setFieldsValue({
                key: record.key,
                name: record.name,
                enabled: record.enabled,
                base_url: record.base_url,
                api_key: undefined,
                model: record.model,
                size: record.size,
                quality: record.quality,
                n: record.n,
                save_mode: record.save_mode,
                local_output_dir: record.local_output_dir ?? undefined,
                storage_bucket_id: record.storage_bucket_id ?? undefined,
                storage_prefix: record.storage_prefix ?? undefined,
                description: record.description ?? undefined,
              });
              setConfigOpen(true);
            }}
          >
            编辑
          </PermissionButton>
          <Popconfirm
            title="确认删除该配置？"
            onConfirm={() => deleteConfigMutation.mutate(record.key)}
          >
            <PermissionButton
              size="small"
              danger
              icon={<DeleteOutlined />}
              permission="system:ai_image:config"
            >
              删除
            </PermissionButton>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  const generationColumns: ColumnsType<AiImageGenerationRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    { title: "配置", dataIndex: "config_name", width: 160 },
    {
      title: "状态",
      dataIndex: "status",
      width: 90,
      render: (value) => statusTag(value),
    },
    {
      title: "提示词",
      dataIndex: "prompt",
      width: 320,
      ellipsis: true,
      render: (value) => (
        <Typography.Text
          copyable={{
            text: value,
            tooltips: ["复制提示词", "已复制"],
          }}
        >
          {value}
        </Typography.Text>
      ),
    },
    { title: "模型", dataIndex: "model", width: 140 },
    { title: "尺寸", dataIndex: "size", width: 120 },
    {
      title: "保存方式",
      dataIndex: "save_mode",
      width: 120,
      render: (value) => saveModeTag(value),
    },
    {
      title: "参考图",
      key: "references",
      width: 120,
      render: (_, record) =>
        record.reference_summary ?? `${record.reference_count} 张`,
    },
    {
      title: "失败信息",
      dataIndex: "error_message",
      width: 420,
      ellipsis: true,
      render: (value) =>
        value ? (
          <Typography.Text
            copyable={{ text: value, tooltips: ["复制失败信息", "已复制"] }}
            ellipsis={{ tooltip: value }}
            style={{ maxWidth: 390 }}
          >
            {value}
          </Typography.Text>
        ) : (
          "-"
        ),
    },
    { title: "时间", dataIndex: "created_at", width: 220 },
    {
      title: "操作",
      key: "actions",
      width: 180,
      fixed: "right",
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            icon={<EyeOutlined />}
            permission="system:ai_image:list"
            onClick={async () => {
              setPreviewing(record);
              setPreviewLoading(true);
              setPreviewObjectUrl(undefined);
              try {
                const blob = await previewAiImageGeneration(record.id);
                setPreviewObjectUrl(URL.createObjectURL(blob));
              } catch (error) {
                message.error(
                  error instanceof Error ? error.message : "预览失败",
                );
              } finally {
                setPreviewLoading(false);
              }
            }}
          >
            预览
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<DownloadOutlined />}
            permission="system:ai_image:list"
            onClick={() => {
              downloadAiImageGeneration(record.id)
                .then((blob) => {
                  const url = URL.createObjectURL(blob);
                  const link = document.createElement("a");
                  link.href = url;
                  link.download = record.original_name;
                  link.click();
                  URL.revokeObjectURL(url);
                })
                .catch((error) => {
                  message.error(
                    error instanceof Error ? error.message : "下载失败",
                  );
                });
            }}
          >
            下载
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<RedoOutlined />}
            permission="system:ai_image:generate"
            loading={
              regenerating?.id === record.id && regenerateMutation.isPending
            }
            onClick={() => {
              if (record.reference_count > 0) {
                message.info("已回填提示词和配置；参考图请重新选择后再生成。");
                fillGenerateForm(record);
                return;
              }
              regenerateRecord(record);
            }}
          >
            重新生成
          </PermissionButton>
        </Space>
      ),
    },
  ];

  const uploadPickerColumns: ColumnsType<UploadRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    { title: "名称", dataIndex: "original_name", width: 260, ellipsis: true },
    { title: "类型", dataIndex: "mime_type", width: 180 },
    { title: "目录", dataIndex: "prefix", width: 180, ellipsis: true },
  ];

  const uploadProps: UploadProps = {
    multiple: true,
    beforeUpload: () => false,
    fileList: referenceFiles,
    onChange: ({ fileList }) => setReferenceFiles(fileList),
  };

  const selectedUploadMap = useMemo(
    () => new Map(selectedUploads.map((item) => [item.id, item])),
    [selectedUploads],
  );

  const activeSaveMode = Form.useWatch("save_mode", configForm) ?? "local";

  function fillGenerateForm(record: AiImageGenerationRecord) {
    generateForm.setFieldsValue({
      config_key: record.config_key,
      prompt: record.prompt,
      model: record.model,
      size: record.size,
      quality: record.quality,
      n: 1,
    });
    setSelectedUploadIds([]);
    setSelectedUploads([]);
    setReferenceFiles([]);
  }

  function regenerateRecord(record: AiImageGenerationRecord) {
    fillGenerateForm(record);
    setRegenerating(record);
    regenerateMutation.mutate({
      config_key: record.config_key,
      prompt: record.prompt,
      model: record.model,
      size: record.size,
      quality: record.quality,
      n: 1,
    });
  }

  return (
    <CrudPage
      title="AI 图片生成"
      subtitle="维护生图配置、组合参考图并查看生成结果"
      breadcrumb={["系统管理", "AI 图片生成"]}
      icon={<ExperimentOutlined />}
      toolbar={<CrudToolbar />}
      notice="API Key 只在后端存储。参考图是可选的：未选择参考图时走普通文生图接口，选择参考图后自动走带参考图接口。生成失败时，历史记录会显示上游请求 URL、状态码和响应摘要。编辑现有配置时留空即表示不修改密钥。"
    >
      <Tabs
        items={[
          {
            key: "configs",
            label: "配置管理",
            children: (
              <DataTable
                loading={configsQuery.isLoading}
                columns={configColumns}
                dataSource={filteredConfigs}
                pagination={false}
                title={() => (
                  <Space>
                    <Input.Search
                      placeholder="筛选配置名称或 key"
                      allowClear
                      onChange={(event) => setConfigKeyword(event.target.value)}
                      style={{ width: 260 }}
                    />
                    <PermissionButton
                      type="primary"
                      icon={<PlusOutlined />}
                      permission="system:ai_image:config"
                      onClick={() => {
                        setEditingConfig(null);
                        configForm.resetFields();
                        configForm.setFieldsValue({
                          enabled: true,
                          model: "gpt-image-2",
                          size: "1024x1536",
                          quality: "high",
                          n: 1,
                          save_mode: "local",
                          local_output_dir: "storage/ai-images/output",
                        });
                        setConfigOpen(true);
                      }}
                    >
                      新增配置
                    </PermissionButton>
                    <Button
                      icon={<ReloadOutlined />}
                      onClick={() => configsQuery.refetch()}
                    >
                      刷新
                    </Button>
                  </Space>
                )}
              />
            ),
          },
          {
            key: "generate",
            label: "图片生成",
            children: (
              <div className="ai-images-workbench">
                <Card className="ai-panel" title="生成参数">
                  <Form
                    form={generateForm}
                    layout="vertical"
                    onFinish={(values) => {
                      generateMutation.mutate({
                        ...values,
                        reference_upload_ids: selectedUploadIds,
                        files: referenceFiles
                          .map((file) => file.originFileObj)
                          .filter((file): file is File => Boolean(file)),
                      });
                    }}
                  >
                    <Form.Item
                      name="config_key"
                      label="配置组"
                      rules={[{ required: true, message: "请选择配置组" }]}
                    >
                      <Select
                        placeholder="选择一个启用的配置"
                        options={enabledConfigs.map((config) => ({
                          value: config.key,
                          label: `${config.name} (${config.model})`,
                        }))}
                      />
                    </Form.Item>
                    <Form.Item
                      name="prompt"
                      label="提示词"
                      rules={[{ required: true, message: "请输入提示词" }]}
                    >
                      <Input.TextArea
                        rows={5}
                        placeholder="描述你希望生成的画面、风格、构图和细节"
                      />
                    </Form.Item>
                    <div className="ai-images-grid">
                      <Form.Item name="model" label="模型">
                        <Input placeholder="默认使用配置中的模型" />
                      </Form.Item>
                      <Form.Item name="size" label="尺寸">
                        <Select allowClear options={sizeOptions} />
                      </Form.Item>
                      <Form.Item name="quality" label="质量">
                        <Select allowClear options={qualityOptions} />
                      </Form.Item>
                      <Form.Item name="n" label="数量">
                        <InputNumber
                          min={1}
                          max={10}
                          style={{ width: "100%" }}
                        />
                      </Form.Item>
                    </div>
                    <Space align="start" size={16} className="ai-reference-row">
                      <Card
                        size="small"
                        title="本次上传参考图（可选）"
                        className="ai-reference-card"
                      >
                        <Upload {...uploadProps} listType="picture-card">
                          <div>
                            <PictureOutlined />
                            <div style={{ marginTop: 8 }}>添加图片</div>
                          </div>
                        </Upload>
                      </Card>
                      <Card
                        size="small"
                        title="从素材库选择（可选）"
                        className="ai-reference-card"
                      >
                        <Space direction="vertical" style={{ width: "100%" }}>
                          <Button onClick={() => setPickerOpen(true)}>
                            选择素材
                          </Button>
                          <List
                            size="small"
                            dataSource={selectedUploads}
                            locale={{ emptyText: "未选择素材" }}
                            renderItem={(item) => (
                              <List.Item
                                actions={[
                                  <Button
                                    key="remove"
                                    type="link"
                                    danger
                                    onClick={() => {
                                      setSelectedUploadIds((current) =>
                                        current.filter((id) => id !== item.id),
                                      );
                                      setSelectedUploads((current) =>
                                        current.filter(
                                          (record) => record.id !== item.id,
                                        ),
                                      );
                                    }}
                                  >
                                    移除
                                  </Button>,
                                ]}
                              >
                                <List.Item.Meta
                                  title={item.original_name}
                                  description={
                                    item.mime_type ?? item.object_key
                                  }
                                />
                              </List.Item>
                            )}
                          />
                        </Space>
                      </Card>
                    </Space>
                    <Space>
                      <PermissionButton
                        type="primary"
                        htmlType="submit"
                        permission="system:ai_image:generate"
                        loading={generateMutation.isPending}
                      >
                        开始生成
                      </PermissionButton>
                      <Button
                        onClick={() => {
                          generateForm.resetFields();
                          setSelectedUploadIds([]);
                          setSelectedUploads([]);
                          setReferenceFiles([]);
                          if (enabledConfigs[0]) {
                            generateForm.setFieldValue(
                              "config_key",
                              enabledConfigs[0].key,
                            );
                          }
                        }}
                      >
                        清空
                      </Button>
                    </Space>
                  </Form>
                </Card>
                <Card className="ai-panel" title="当前配置摘要">
                  {selectedConfig ? (
                    <Descriptions column={1} size="small">
                      <Descriptions.Item label="名称">
                        {selectedConfig.name}
                      </Descriptions.Item>
                      <Descriptions.Item label="Base URL">
                        {selectedConfig.base_url}
                      </Descriptions.Item>
                      <Descriptions.Item label="模型">
                        {selectedConfig.model}
                      </Descriptions.Item>
                      <Descriptions.Item label="尺寸">
                        {selectedConfig.size}
                      </Descriptions.Item>
                      <Descriptions.Item label="质量">
                        {selectedConfig.quality}
                      </Descriptions.Item>
                      <Descriptions.Item label="保存方式">
                        {selectedConfig.save_mode}
                      </Descriptions.Item>
                      <Descriptions.Item label="输出位置">
                        {selectedConfig.save_mode === "local"
                          ? (selectedConfig.local_output_dir ?? "-")
                          : selectedBucket
                            ? `${selectedBucket.name} / ${selectedConfig.storage_prefix ?? "根目录"}`
                            : (selectedConfig.storage_prefix ?? "-")}
                      </Descriptions.Item>
                      <Descriptions.Item label="说明">
                        {selectedConfig.description || "-"}
                      </Descriptions.Item>
                    </Descriptions>
                  ) : (
                    <Typography.Text type="secondary">
                      请选择启用的配置后再发起生成。
                    </Typography.Text>
                  )}
                </Card>
                <Card className="ai-panel" title="最近一次生成结果">
                  {generateMutation.isPending ? (
                    <div className="ai-preview-loading">
                      <Spin />
                    </div>
                  ) : latestBatch.length > 0 ? (
                    <div className="ai-result-grid">
                      {latestBatch.map((item) => (
                        <button
                          type="button"
                          key={item.id}
                          className="ai-result-tile"
                          onClick={async () => {
                            setPreviewing(item);
                            setPreviewLoading(true);
                            setPreviewObjectUrl(undefined);
                            try {
                              const blob = await previewAiImageGeneration(
                                item.id,
                              );
                              setPreviewObjectUrl(URL.createObjectURL(blob));
                            } catch (error) {
                              message.error(
                                error instanceof Error
                                  ? error.message
                                  : "预览失败",
                              );
                            } finally {
                              setPreviewLoading(false);
                            }
                          }}
                        >
                          <span className="ai-result-title">
                            #{item.output_index}
                          </span>
                          <span className="ai-result-meta">
                            {item.original_name}
                          </span>
                          <span className="ai-result-meta">{item.size}</span>
                        </button>
                      ))}
                    </div>
                  ) : (
                    <Typography.Text type="secondary">
                      生成完成后，这里会展示当前批次结果入口。
                    </Typography.Text>
                  )}
                </Card>
              </div>
            ),
          },
          {
            key: "history",
            label: "生成记录",
            children: (
              <DataTable
                columns={generationColumns}
                dataSource={generationsQuery.data?.items ?? []}
                loading={generationsQuery.isLoading}
                pagination={{
                  current: page,
                  pageSize: generationsQuery.data?.page_size ?? 10,
                  total: generationsQuery.data?.total ?? 0,
                  onChange: setPage,
                }}
                title={() => (
                  <Space>
                    <Input.Search
                      placeholder="搜索提示词、配置名、批次号"
                      allowClear
                      onSearch={(value) => {
                        setKeyword(value.trim());
                        setPage(1);
                      }}
                      style={{ width: 320 }}
                    />
                    <Button
                      icon={<ReloadOutlined />}
                      onClick={() => generationsQuery.refetch()}
                    >
                      刷新
                    </Button>
                  </Space>
                )}
              />
            ),
          },
        ]}
      />

      <Modal
        title={editingConfig ? "编辑配置" : "新增配置"}
        open={configOpen}
        onCancel={() => {
          setConfigOpen(false);
          setEditingConfig(null);
        }}
        onOk={() => configForm.submit()}
        confirmLoading={saveConfigMutation.isPending}
        width={720}
      >
        <Form
          form={configForm}
          layout="vertical"
          onFinish={(values) => saveConfigMutation.mutate(values)}
        >
          <div className="ai-images-grid">
            <Form.Item
              name="key"
              label="配置键"
              rules={[{ required: true, message: "请输入配置键" }]}
            >
              <Input disabled={Boolean(editingConfig)} />
            </Form.Item>
            <Form.Item
              name="name"
              label="名称"
              rules={[{ required: true, message: "请输入名称" }]}
            >
              <Input />
            </Form.Item>
            <Form.Item name="enabled" label="启用" valuePropName="checked">
              <Switch />
            </Form.Item>
            <Form.Item
              name="save_mode"
              label="保存方式"
              rules={[{ required: true, message: "请选择保存方式" }]}
            >
              <Select options={saveModeOptions} />
            </Form.Item>
          </div>
          <Form.Item
            name="base_url"
            label="Base URL"
            rules={[{ required: true, message: "请输入 Base URL" }]}
          >
            <Input placeholder="https://example.test/v1" />
          </Form.Item>
          <Form.Item
            name="api_key"
            label={editingConfig ? "API Key（留空表示不修改）" : "API Key"}
            rules={
              editingConfig
                ? []
                : [{ required: true, message: "请输入 API Key" }]
            }
          >
            <Input.Password
              placeholder={editingConfig ? "留空则保留旧值" : "输入 API Key"}
            />
          </Form.Item>
          <div className="ai-images-grid">
            <Form.Item name="model" label="默认模型">
              <Input />
            </Form.Item>
            <Form.Item name="size" label="默认尺寸">
              <Select options={sizeOptions} />
            </Form.Item>
            <Form.Item name="quality" label="默认质量">
              <Select options={qualityOptions} />
            </Form.Item>
            <Form.Item name="n" label="默认数量">
              <InputNumber min={1} max={10} style={{ width: "100%" }} />
            </Form.Item>
          </div>
          {activeSaveMode === "local" ? (
            <Form.Item
              name="local_output_dir"
              label="本地输出目录"
              rules={[{ required: true, message: "请输入本地输出目录" }]}
            >
              <Input placeholder="storage/ai-images/output" />
            </Form.Item>
          ) : (
            <div className="ai-images-grid">
              <Form.Item
                name="storage_bucket_id"
                label="存储桶"
                rules={[{ required: true, message: "请选择存储桶" }]}
              >
                <Select
                  options={storageProfiles.flatMap((profile, index) =>
                    (bucketQueries[index]?.data ?? []).map((bucket) => ({
                      value: bucket.id,
                      label: `${profile.name} / ${bucket.name}`,
                    })),
                  )}
                />
              </Form.Item>
              <Form.Item name="storage_prefix" label="对象前缀">
                <Input placeholder="ai-images/generated" />
              </Form.Item>
            </div>
          )}
          <Form.Item name="description" label="说明">
            <Input.TextArea rows={3} />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title="从素材库选择参考图"
        open={pickerOpen}
        onCancel={() => setPickerOpen(false)}
        onOk={() => {
          const items = (uploadsQuery.data?.items ?? []).filter((item) =>
            selectedUploadIds.includes(item.id),
          );
          setSelectedUploads(
            Array.from(
              new Map(
                [...selectedUploads, ...items].map((item) => [item.id, item]),
              ).values(),
            ),
          );
          setPickerOpen(false);
        }}
        width={900}
      >
        <Space direction="vertical" style={{ width: "100%" }}>
          <Input.Search
            placeholder="搜索素材名称"
            allowClear
            onSearch={setPickerKeyword}
          />
          <DataTable
            rowSelection={{
              selectedRowKeys: selectedUploadIds,
              onChange: (keys) => setSelectedUploadIds(keys as number[]),
            }}
            columns={uploadPickerColumns}
            dataSource={uploadsQuery.data?.items ?? []}
            loading={uploadsQuery.isLoading}
            pagination={false}
          />
          <Typography.Text type="secondary">
            已选 {selectedUploadIds.length} 个素材，当前缓存{" "}
            {selectedUploadMap.size} 项。
          </Typography.Text>
        </Space>
      </Modal>

      <Drawer
        title={previewing ? `预览 ${previewing.original_name}` : "预览"}
        open={Boolean(previewing)}
        onClose={() => {
          setPreviewing(null);
          setPreviewObjectUrl(undefined);
          setPreviewLoading(false);
        }}
        width={720}
      >
        {previewing ? (
          <Space direction="vertical" style={{ width: "100%" }} size={16}>
            <Descriptions column={1} size="small" bordered>
              <Descriptions.Item label="配置">
                {previewing.config_name}
              </Descriptions.Item>
              <Descriptions.Item label="状态">
                {previewing.status}
              </Descriptions.Item>
              <Descriptions.Item label="提示词">
                <Typography.Text
                  copyable={{
                    text: previewing.prompt,
                    tooltips: ["复制提示词", "已复制"],
                  }}
                >
                  {previewing.prompt}
                </Typography.Text>
              </Descriptions.Item>
              <Descriptions.Item label="尺寸">
                {previewing.size}
              </Descriptions.Item>
              <Descriptions.Item label="质量">
                {previewing.quality}
              </Descriptions.Item>
              <Descriptions.Item label="参考图">
                {previewing.reference_summary ??
                  `${previewing.reference_count} 张`}
              </Descriptions.Item>
              <Descriptions.Item label="失败信息">
                {previewing.error_message ? (
                  <Typography.Paragraph
                    copyable={{
                      text: previewing.error_message,
                      tooltips: ["复制失败信息", "已复制"],
                    }}
                    style={{ marginBottom: 0, whiteSpace: "pre-wrap" }}
                  >
                    {previewing.error_message}
                  </Typography.Paragraph>
                ) : (
                  "-"
                )}
              </Descriptions.Item>
            </Descriptions>
            <div className="ai-preview-stage">
              {previewLoading ? (
                <Spin />
              ) : previewObjectUrl ? (
                <Image src={previewObjectUrl} alt={previewing.original_name} />
              ) : (
                <Typography.Text type="secondary">
                  暂无可预览内容
                </Typography.Text>
              )}
            </div>
          </Space>
        ) : null}
      </Drawer>
    </CrudPage>
  );
}
