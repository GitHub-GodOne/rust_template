import {
  AppstoreOutlined,
  CloudDownloadOutlined,
  CloudServerOutlined,
  CloudSyncOutlined,
  CloudUploadOutlined,
  DeleteOutlined,
  EditOutlined,
  ExclamationCircleOutlined,
  EyeOutlined,
  FileOutlined,
  FolderOpenOutlined,
  FolderOutlined,
  LinkOutlined,
  TableOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Breadcrumb,
  Button,
  Card,
  Descriptions,
  Drawer,
  Empty,
  Form,
  Input,
  Modal,
  Popconfirm,
  Progress,
  Segmented,
  Select,
  Space,
  Spin,
  Tag,
  Tooltip,
  Tree,
  Typography,
  Upload,
  message,
} from "antd";
import type { UploadProps } from "antd";
import type { ColumnsType } from "antd/es/table";
import type { DataNode } from "antd/es/tree";
import { type Key, useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  fetchStorageBuckets,
  fetchStorageProfiles,
} from "../../../api/admin/storage";
import {
  type StorageObjectRecord,
  type StoragePrefixRecord,
  type UpdateUploadParams,
  type UploadRecord,
  createUploadFolder,
  deleteUpload,
  downloadUpload,
  fetchUploadBrowser,
  fetchUploads,
  importUploadObject,
  importUploadObjects,
  previewUpload,
  renameUpload,
  updateUpload,
  uploadMaterial,
} from "../../../api/admin/uploads";
import { CrudPage } from "../../../components/admin/CrudPage";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import {
  extensionFromName,
  fileIcon,
  fileKind,
  formatBytes,
  mimeFromExtension,
  useUploadTaskManager,
} from "./uploadTaskManager";

function publicLink(record: UploadRecord) {
  if (record.visibility !== "public" || !record.url) {
    return null;
  }
  return new URL(record.url, window.location.origin).toString();
}

type ExternalObjectRow = StorageObjectRecord & {
  id: string;
  extension?: string | null;
  mime_type?: string | null;
};

type FolderFormValues = {
  name: string;
};

type RenameFormValues = {
  original_name: string;
};

function joinPrefix(prefix: string | undefined, name: string) {
  const cleanName = name.trim().replace(/^\/+|\/+$/g, "");
  if (!cleanName) {
    return undefined;
  }
  return `${prefix ?? ""}${cleanName}/`;
}

function pathSegments(prefix?: string) {
  const segments = prefix?.split("/").filter(Boolean) ?? [];
  return segments.map((name, index) => ({
    name,
    prefix: `${segments.slice(0, index + 1).join("/")}/`,
  }));
}

function isFolderMarkerObject(object: StorageObjectRecord) {
  return object.key.endsWith("/") || object.key.endsWith("/.keep");
}

function folderTreeNode(folder: StoragePrefixRecord): DataNode {
  return {
    key: folder.prefix,
    title: folder.name,
    icon: <FolderOutlined />,
    isLeaf: false,
  };
}

function applyTreeChildren(
  nodes: DataNode[],
  key: string,
  children: DataNode[],
): DataNode[] {
  return nodes.map((node) => {
    if (String(node.key) === key) {
      return { ...node, children, isLeaf: key !== "" && children.length === 0 };
    }
    return {
      ...node,
      children: node.children
        ? applyTreeChildren(node.children, key, children)
        : node.children,
    };
  });
}

export function UploadsPage() {
  const [page, setPage] = useState(1);
  const [keyword, setKeyword] = useState("");
  const [status, setStatus] = useState<string>();
  const [profileId, setProfileId] = useState<number>();
  const [bucketId, setBucketId] = useState<number>();
  const [prefix, setPrefix] = useState<string>();
  const [layoutMode, setLayoutMode] = useState<"table" | "grid">("grid");
  const [uploadMode, setUploadMode] = useState<"normal" | "chunk">("chunk");
  const [uploadVisibility, setUploadVisibility] = useState<
    "private" | "public"
  >("private");
  const [uploadPercent, setUploadPercent] = useState(0);
  const [uploadingName, setUploadingName] = useState<string>();
  const [detail, setDetail] = useState<UploadRecord | null>(null);
  const [editing, setEditing] = useState<UploadRecord | null>(null);
  const [renaming, setRenaming] = useState<UploadRecord | null>(null);
  const [folderModalOpen, setFolderModalOpen] = useState(false);
  const [treeData, setTreeData] = useState<DataNode[]>([]);
  const [expandedTreeKeys, setExpandedTreeKeys] = useState<Key[]>([""]);
  const [previewing, setPreviewing] = useState<UploadRecord | null>(null);
  const [previewObjectUrl, setPreviewObjectUrl] = useState<string>();
  const [previewError, setPreviewError] = useState<string>();
  const [previewLoading, setPreviewLoading] = useState(false);
  const [form] = Form.useForm<UpdateUploadParams>();
  const [folderForm] = Form.useForm<FolderFormValues>();
  const [renameForm] = Form.useForm<RenameFormValues>();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const uploadTaskManager = useUploadTaskManager({
    bucketId,
    prefix,
    visibility: uploadVisibility,
  });

  useEffect(() => {
    return () => {
      if (previewObjectUrl) {
        URL.revokeObjectURL(previewObjectUrl);
      }
    };
  }, [previewObjectUrl]);

  const profilesQuery = useQuery({
    queryKey: ["admin-storage-profiles"],
    queryFn: () => fetchStorageProfiles({ page_size: 100 }),
  });
  const profiles = profilesQuery.data?.items ?? [];
  const selectedProfile = profiles.find((profile) => profile.id === profileId);

  useEffect(() => {
    if (!profileId && profiles.length > 0) {
      setProfileId(
        profiles.find((profile) => profile.is_default)?.id ?? profiles[0].id,
      );
    }
  }, [profileId, profiles]);

  const bucketsQuery = useQuery({
    queryKey: ["admin-storage-buckets", profileId],
    enabled: Boolean(profileId),
    queryFn: () => fetchStorageBuckets(profileId ?? 0),
  });
  const buckets = bucketsQuery.data ?? [];
  const selectedBucket = buckets.find((bucket) => bucket.id === bucketId);
  const hasPublicAccess = Boolean(
    selectedProfile?.public_base_url || selectedBucket?.public_prefix,
  );

  useEffect(() => {
    if (buckets.length === 0) {
      setBucketId(undefined);
      return;
    }
    if (!bucketId || !buckets.some((bucket) => bucket.id === bucketId)) {
      setBucketId(
        buckets.find((bucket) => bucket.is_default)?.id ?? buckets[0].id,
      );
      setPrefix(undefined);
      setPage(1);
    }
  }, [bucketId, buckets]);

  useEffect(() => {
    if (hasPublicAccess) {
      setUploadVisibility("public");
    }
  }, [hasPublicAccess]);

  const browserQuery = useQuery({
    queryKey: ["admin-upload-browser", bucketId, prefix],
    enabled: Boolean(bucketId),
    queryFn: () => fetchUploadBrowser({ storage_bucket_id: bucketId, prefix }),
  });

  const loadTreeChildren = useCallback(
    async (targetPrefix?: string) => {
      if (!bucketId) {
        return;
      }
      const browser = await fetchUploadBrowser({
        storage_bucket_id: bucketId,
        prefix: targetPrefix,
      });
      const children = browser.prefixes.map(folderTreeNode);
      const treeKey = targetPrefix ?? "";
      setTreeData((current) => {
        if (treeKey === "" && current.length === 0) {
          return [
            {
              key: "",
              title: "根目录",
              icon: <FolderOpenOutlined />,
              children,
            },
          ];
        }
        return applyTreeChildren(current, treeKey, children);
      });
    },
    [bucketId],
  );

  useEffect(() => {
    if (!bucketId) {
      setTreeData([]);
      setExpandedTreeKeys([""]);
      return;
    }
    let cancelled = false;
    fetchUploadBrowser({ storage_bucket_id: bucketId })
      .then((browser) => {
        if (cancelled) {
          return;
        }
        setTreeData([
          {
            key: "",
            title: "根目录",
            icon: <FolderOpenOutlined />,
            children: browser.prefixes.map(folderTreeNode),
          },
        ]);
        setExpandedTreeKeys([""]);
      })
      .catch((error) => {
        if (!cancelled) {
          message.error(
            error instanceof Error ? error.message : "目录树加载失败",
          );
        }
      });
    return () => {
      cancelled = true;
    };
  }, [bucketId]);

  const uploadsQuery = useQuery({
    queryKey: [
      "admin-uploads",
      page,
      keyword,
      status,
      profileId,
      bucketId,
      prefix,
    ],
    queryFn: () =>
      fetchUploads({
        page,
        page_size: 10,
        keyword: keyword || undefined,
        status,
        storage_profile_id: profileId,
        storage_bucket_id: bucketId,
        prefix,
      }),
  });

  const uploadMutation = useMutation({
    mutationFn: (file: File) =>
      uploadMaterial(file, {
        storage_profile_id: profileId,
        storage_bucket_id: bucketId,
        prefix,
        visibility: uploadVisibility,
        onUploadProgress: setUploadPercent,
      }),
    onSuccess: () => {
      message.success("素材已上传");
      setUploadPercent(100);
      setUploadingName(undefined);
      queryClient.invalidateQueries({ queryKey: ["admin-uploads"] });
      queryClient.invalidateQueries({ queryKey: ["admin-upload-browser"] });
    },
    onError: (error) => {
      const reason = error instanceof Error ? error.message : "素材上传失败";
      message.error(reason);
      setUploadingName(undefined);
      setUploadPercent(0);
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
  const importMutation = useMutation({
    mutationFn: (object: ExternalObjectRow) =>
      importUploadObject({
        storage_bucket_id: bucketId ?? 0,
        object_key: object.key,
        original_name: object.name,
        mime_type: object.mime_type,
        visibility: object.url ? "public" : "private",
      }),
    onSuccess: () => {
      message.success("外部对象已入库");
      queryClient.invalidateQueries({ queryKey: ["admin-uploads"] });
      queryClient.invalidateQueries({ queryKey: ["admin-upload-browser"] });
    },
    onError: (error) => {
      message.error(
        error instanceof Error ? error.message : "外部对象入库失败",
      );
    },
  });
  const importAllMutation = useMutation({
    mutationFn: () =>
      importUploadObjects({
        storage_bucket_id: bucketId ?? 0,
        prefix,
        visibility: uploadVisibility,
      }),
    onSuccess: (result) => {
      message.success(
        `已入库 ${result.imported} 个，跳过 ${result.skipped} 个`,
      );
      queryClient.invalidateQueries({ queryKey: ["admin-uploads"] });
      queryClient.invalidateQueries({ queryKey: ["admin-upload-browser"] });
    },
    onError: (error) => {
      message.error(error instanceof Error ? error.message : "批量入库失败");
    },
  });
  const createFolderMutation = useMutation({
    mutationFn: (values: FolderFormValues) => {
      const targetPrefix = joinPrefix(prefix, values.name);
      if (!targetPrefix) {
        throw new Error("请输入文件夹名称");
      }
      return createUploadFolder({
        storage_bucket_id: bucketId ?? 0,
        prefix: targetPrefix,
      });
    },
    onSuccess: () => {
      message.success("文件夹已创建");
      setFolderModalOpen(false);
      folderForm.resetFields();
      loadTreeChildren(prefix);
      queryClient.invalidateQueries({ queryKey: ["admin-upload-browser"] });
    },
    onError: (error) => {
      message.error(error instanceof Error ? error.message : "创建文件夹失败");
    },
  });
  const renameMutation = useMutation({
    mutationFn: (values: RenameFormValues) =>
      renameUpload(renaming?.id ?? 0, values),
    onSuccess: (record) => {
      message.success("文件已重命名");
      if (previewing?.id === record.id) {
        closePreview();
      }
      setRenaming(null);
      renameForm.resetFields();
      queryClient.invalidateQueries({ queryKey: ["admin-uploads"] });
      queryClient.invalidateQueries({ queryKey: ["admin-upload-browser"] });
    },
    onError: (error) => {
      message.error(error instanceof Error ? error.message : "文件重命名失败");
    },
  });
  const downloadMutation = useMutation({
    mutationFn: downloadUpload,
  });

  const uploadProps: UploadProps = {
    multiple: false,
    showUploadList: false,
    customRequest: ({ file, onSuccess, onError }) => {
      const uploadFile = file as File;
      if (uploadMode === "chunk") {
        uploadTaskManager
          .startChunkUpload(uploadFile)
          .then(() => onSuccess?.("ok"))
          .catch((error) => onError?.(error as Error));
        return;
      }
      setUploadingName(uploadFile.name);
      setUploadPercent(0);
      uploadMutation.mutate(uploadFile, {
        onSuccess: () => onSuccess?.("ok"),
        onError: (error) => onError?.(error as Error),
      });
    },
  };

  const openPrefix = (nextPrefix?: string) => {
    setPrefix(nextPrefix);
    setPage(1);
  };

  const openRename = (record: UploadRecord) => {
    setRenaming(record);
    renameForm.setFieldsValue({ original_name: record.original_name });
  };

  const currentPathSegments = pathSegments(prefix);
  const folderPrefixes = browserQuery.data?.prefixes ?? [];

  const openPreview = async (record: UploadRecord) => {
    setPreviewing(record);
    setPreviewObjectUrl(undefined);
    setPreviewError(undefined);
    setPreviewLoading(true);
    try {
      const blob = await previewUpload(record.id);
      setPreviewObjectUrl(URL.createObjectURL(blob));
    } catch (error) {
      const reason =
        error instanceof Error ? error.message : "素材预览加载失败";
      setPreviewError(reason);
      message.error(reason);
    } finally {
      setPreviewLoading(false);
    }
  };

  const closePreview = () => {
    setPreviewing(null);
    setPreviewObjectUrl(undefined);
    setPreviewError(undefined);
    setPreviewLoading(false);
  };

  const copyLink = async (record: UploadRecord) => {
    const link = publicLink(record);
    if (!link) {
      if (record.visibility !== "public") {
        message.warning("该文件不是公开文件，无法复制公开分享链接");
      } else {
        message.warning("当前存储未配置公开访问地址");
      }
      return;
    }
    try {
      await navigator.clipboard.writeText(link);
      message.success("公开链接已复制");
    } catch {
      message.error("复制失败，请检查浏览器权限");
    }
  };

  const downloadRecord = (record: UploadRecord) => {
    if (record.status === "external") {
      const link = publicLink(record);
      if (!link) {
        message.warning("外部对象未配置公开访问地址，无法直接下载");
        return;
      }
      const anchor = document.createElement("a");
      anchor.href = link;
      anchor.download = record.original_name;
      anchor.target = "_blank";
      anchor.click();
      return;
    }
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
  };

  const externalObjectRecord = (object: ExternalObjectRow): UploadRecord => ({
    id: 0,
    storage: selectedProfile?.provider ?? "external",
    storage_profile_id: profileId,
    storage_bucket_id: bucketId,
    bucket: selectedBucket?.bucket ?? null,
    prefix: object.prefix || null,
    etag: object.etag,
    object_key: object.key,
    url: object.url,
    original_name: object.name,
    filename: object.name,
    extension: object.extension,
    mime_type: object.mime_type,
    size_bytes: object.size_bytes,
    sha256: "-",
    category: "外部对象",
    tags: null,
    visibility: object.url ? "public" : "private",
    status: "external",
    uploader_id: null,
    created_at: object.updated_at ?? "-",
    updated_at: object.updated_at ?? "-",
  });

  const openExternalPreview = (object: ExternalObjectRow) => {
    const record = externalObjectRecord(object);
    const link = publicLink(record);
    setPreviewing(record);
    setPreviewObjectUrl(link ?? undefined);
    setPreviewError(
      link
        ? undefined
        : "外部对象未配置公开访问地址，无法在线预览；请先入库后使用后台鉴权预览或配置公开访问地址。",
    );
    setPreviewLoading(false);
  };

  const editRecord = (record: UploadRecord) => {
    setEditing(record);
    form.setFieldsValue({
      category: record.category,
      tags: record.tags,
      visibility: record.visibility,
      status: record.status,
    });
  };

  const renderActions = (record: UploadRecord) => (
    <Space wrap size={6}>
      <PermissionButton
        size="small"
        icon={<EyeOutlined />}
        permission="system:upload:download"
        onClick={() => openPreview(record)}
      >
        预览
      </PermissionButton>
      <Tooltip title="复制公开分享链接">
        <Button
          size="small"
          icon={<LinkOutlined />}
          onClick={() => copyLink(record)}
        >
          复制链接
        </Button>
      </Tooltip>
      <PermissionButton
        size="small"
        icon={<CloudDownloadOutlined />}
        permission="system:upload:download"
        loading={downloadMutation.isPending}
        onClick={() => downloadRecord(record)}
      >
        下载
      </PermissionButton>
      <PermissionButton
        size="small"
        icon={<FileOutlined />}
        permission="system:upload:detail"
        onClick={() => setDetail(record)}
      >
        详情
      </PermissionButton>
      <PermissionButton
        size="small"
        icon={<EditOutlined />}
        permission="system:upload:update"
        onClick={() => editRecord(record)}
      >
        编辑
      </PermissionButton>
      <PermissionButton
        size="small"
        icon={<EditOutlined />}
        permission="system:upload:update"
        onClick={() => openRename(record)}
      >
        重命名
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
  );

  const columns: ColumnsType<UploadRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    {
      title: "文件名",
      dataIndex: "original_name",
      width: 240,
      ellipsis: true,
      render: (value, record) => (
        <Space size={8}>
          <span className="material-table-file-icon">{fileIcon(record)}</span>
          <Typography.Text ellipsis className="material-table-file-name">
            {value}
          </Typography.Text>
        </Space>
      ),
    },
    {
      title: "来源",
      key: "source",
      width: 100,
      render: () => <Tag color="blue">已入库</Tag>,
    },
    {
      title: "存储",
      dataIndex: "storage",
      width: 120,
      render: (value) => <Tag icon={<CloudServerOutlined />}>{value}</Tag>,
    },
    { title: "桶", dataIndex: "bucket", width: 150, ellipsis: true },
    {
      title: "目录",
      dataIndex: "prefix",
      width: 180,
      ellipsis: true,
      render: (value) => value ?? "-",
    },
    {
      title: "分类",
      dataIndex: "category",
      width: 120,
      render: (value) => (value ? <Tag>{value}</Tag> : "-"),
    },
    { title: "MIME", dataIndex: "mime_type", width: 170, ellipsis: true },
    { title: "大小", dataIndex: "size_bytes", width: 110, render: formatBytes },
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
      width: 420,
      render: (_, record) => renderActions(record),
    },
  ];

  const externalColumns: ColumnsType<ExternalObjectRow> = [
    {
      title: "对象名",
      dataIndex: "name",
      width: 260,
      ellipsis: true,
      render: (value, record) => (
        <Space size={8}>
          <span className="material-table-file-icon">
            {fileIcon(externalObjectRecord(record))}
          </span>
          <Typography.Text ellipsis className="material-table-file-name">
            {value}
          </Typography.Text>
        </Space>
      ),
    },
    {
      title: "来源",
      key: "source",
      width: 110,
      render: () => <Tag color="orange">外部对象</Tag>,
    },
    { title: "对象键", dataIndex: "key", width: 260, ellipsis: true },
    {
      title: "大小",
      dataIndex: "size_bytes",
      width: 110,
      render: formatBytes,
    },
    {
      title: "更新时间",
      dataIndex: "updated_at",
      width: 220,
      render: (value) => value ?? "-",
    },
    {
      title: "操作",
      key: "actions",
      width: 260,
      render: (_, record) => (
        <Space wrap size={6}>
          <Button
            size="small"
            icon={<EyeOutlined />}
            onClick={() => openExternalPreview(record)}
          >
            预览
          </Button>
          <Button
            size="small"
            icon={<LinkOutlined />}
            onClick={() => copyLink(externalObjectRecord(record))}
          >
            复制链接
          </Button>
          <Button
            size="small"
            icon={<CloudDownloadOutlined />}
            onClick={() => downloadRecord(externalObjectRecord(record))}
          >
            下载
          </Button>
          <PermissionButton
            size="small"
            icon={<CloudSyncOutlined />}
            permission="system:upload:create"
            loading={importMutation.isPending}
            onClick={() => importMutation.mutate(record)}
          >
            入库
          </PermissionButton>
        </Space>
      ),
    },
  ];

  const renderPreviewContent = () => {
    if (previewLoading) {
      return (
        <div className="material-preview-empty">
          <Spin tip="正在加载预览" />
        </div>
      );
    }
    if (!previewing) {
      return <Empty description="暂无预览内容" />;
    }
    if (previewError) {
      return (
        <div className="material-preview-empty">
          <ExclamationCircleOutlined className="material-preview-large-icon" />
          <Typography.Text type="secondary">{previewError}</Typography.Text>
          <Typography.Text type="secondary">
            {previewing.mime_type ?? previewing.extension ?? "未知文件类型"}
          </Typography.Text>
          <Space wrap className="material-preview-actions">
            <Button
              icon={<LinkOutlined />}
              onClick={() => copyLink(previewing)}
            >
              复制公开链接
            </Button>
            <PermissionButton
              icon={<CloudDownloadOutlined />}
              permission="system:upload:download"
              onClick={() => downloadRecord(previewing)}
            >
              下载
            </PermissionButton>
          </Space>
        </div>
      );
    }
    if (!previewObjectUrl) {
      return <Empty description="暂无预览内容" />;
    }
    const kind = fileKind(previewing);
    if (kind === "image") {
      return (
        <img
          className="material-preview-image"
          src={previewObjectUrl}
          alt={previewing.original_name}
        />
      );
    }
    if (kind === "video") {
      return (
        <video
          className="material-preview-frame"
          src={previewObjectUrl}
          controls
        >
          <track kind="captions" />
        </video>
      );
    }
    if (kind === "audio") {
      return (
        <div className="material-preview-empty">
          <SoundOutlined className="material-preview-large-icon" />
          <audio
            className="material-preview-audio"
            src={previewObjectUrl}
            controls
          >
            <track kind="captions" />
          </audio>
        </div>
      );
    }
    if (kind === "pdf") {
      return (
        <iframe
          className="material-preview-frame"
          src={previewObjectUrl}
          title={previewing.original_name}
        />
      );
    }
    return (
      <div className="material-preview-empty">
        <FileOutlined className="material-preview-large-icon" />
        <Typography.Text type="secondary">
          该文件类型暂不支持在线预览，请下载后查看。
        </Typography.Text>
        <Space wrap className="material-preview-actions">
          <Button icon={<LinkOutlined />} onClick={() => copyLink(previewing)}>
            复制公开链接
          </Button>
          <PermissionButton
            icon={<CloudDownloadOutlined />}
            permission="system:upload:download"
            onClick={() => downloadRecord(previewing)}
          >
            下载
          </PermissionButton>
        </Space>
      </div>
    );
  };

  const files = uploadsQuery.data?.items ?? [];
  const managedObjectKeys = useMemo(
    () => new Set(files.map((file) => file.object_key)),
    [files],
  );
  const externalObjects = useMemo<ExternalObjectRow[]>(() => {
    return (browserQuery.data?.objects ?? [])
      .filter((object) => !isFolderMarkerObject(object))
      .filter((object) => !managedObjectKeys.has(object.key))
      .map((object) => {
        const extension = extensionFromName(object.name);
        return {
          ...object,
          id: object.key,
          extension,
          mime_type: mimeFromExtension(extension),
        };
      });
  }, [browserQuery.data?.objects, managedObjectKeys]);

  return (
    <CrudPage
      title="文件上传 / 素材库"
      subtitle="统一管理本地存储、R2 和 S3 兼容资源"
      breadcrumb={["系统管理", "素材库"]}
      icon={<CloudUploadOutlined />}
      toolbar={
        <Space wrap className="material-filter-bar">
          <Select
            placeholder="存储配置"
            value={profileId}
            loading={profilesQuery.isLoading}
            onChange={(value) => {
              setProfileId(value);
              setBucketId(undefined);
              setPrefix(undefined);
              setPage(1);
            }}
            options={profiles.map((profile) => ({
              value: profile.id,
              label: `${profile.name} / ${profile.provider}`,
            }))}
            className="admin-filter-select"
          />
          <Select
            placeholder="桶"
            value={bucketId}
            loading={bucketsQuery.isLoading}
            onChange={(value) => {
              setBucketId(value);
              setPrefix(undefined);
              setPage(1);
            }}
            options={buckets.map((bucket) => ({
              value: bucket.id,
              label: bucket.name,
            }))}
            className="admin-filter-select"
          />
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
      <div className="resource-library-layout">
        <div className="resource-tree-panel">
          <Space className="full-width" direction="vertical" size="small">
            <Tree
              showIcon
              selectedKeys={[prefix ?? ""]}
              expandedKeys={expandedTreeKeys}
              treeData={treeData}
              loadData={(node) =>
                loadTreeChildren(String(node.key) || undefined)
              }
              onExpand={(keys) => setExpandedTreeKeys(keys)}
              onSelect={(keys) =>
                openPrefix(String(keys[0] ?? "") || undefined)
              }
            />
          </Space>
        </div>
        <Space direction="vertical" size="middle" className="full-width">
          <div className="material-path-bar">
            <Space direction="vertical" size={4} className="full-width">
              <Typography.Text type="secondary">
                当前桶：{selectedBucket?.name ?? "未选择"}
              </Typography.Text>
              <Breadcrumb
                items={[
                  {
                    title: (
                      <Button
                        type="link"
                        size="small"
                        onClick={() => openPrefix(undefined)}
                      >
                        根目录
                      </Button>
                    ),
                  },
                  ...currentPathSegments.map((segment) => ({
                    title: (
                      <Button
                        type="link"
                        size="small"
                        onClick={() => openPrefix(segment.prefix)}
                      >
                        {segment.name}
                      </Button>
                    ),
                  })),
                ]}
              />
            </Space>
          </div>
          <div className="material-library-toolbar">
            <Space direction="vertical" size={2}>
              <Typography.Text type="secondary">
                双击文件可预览，公开文件可复制分享链接
              </Typography.Text>
              <Typography.Text type="secondary">
                可直接把文件拖拽到下方内容区域，上传到 {prefix ?? "根目录"}
              </Typography.Text>
            </Space>
            <Space wrap className="material-upload-inline-actions">
              <Typography.Text type="secondary">上传方式</Typography.Text>
              <Segmented
                value={uploadMode}
                onChange={(value) => setUploadMode(value as "normal" | "chunk")}
                options={[
                  { label: "分片上传", value: "chunk" },
                  { label: "普通上传", value: "normal" },
                ]}
              />
              <Typography.Text type="secondary">可见性</Typography.Text>
              <Segmented
                value={uploadVisibility}
                onChange={(value) =>
                  setUploadVisibility(value as "private" | "public")
                }
                options={[
                  { label: "私有", value: "private" },
                  { label: "公开", value: "public" },
                ]}
              />
              {uploadVisibility === "public" && !hasPublicAccess ? (
                <Typography.Text type="warning">
                  当前存储未配置公开访问地址
                </Typography.Text>
              ) : null}
              <Upload
                {...uploadProps}
                disabled={!bucketId || uploadMutation.isPending}
              >
                <PermissionButton
                  icon={<CloudUploadOutlined />}
                  permission="system:upload:create"
                  hideWhenDenied={false}
                  disabled={!bucketId || uploadMutation.isPending}
                >
                  上传文件
                </PermissionButton>
              </Upload>
              <PermissionButton
                icon={<FolderOutlined />}
                permission="system:upload:create"
                disabled={!bucketId}
                onClick={() => setFolderModalOpen(true)}
              >
                新建文件夹
              </PermissionButton>
              <PermissionButton
                icon={<CloudSyncOutlined />}
                permission="system:upload:create"
                disabled={!bucketId || externalObjects.length === 0}
                loading={importAllMutation.isPending}
                onClick={() => importAllMutation.mutate()}
              >
                一键入库
              </PermissionButton>
              <Button
                icon={<CloudSyncOutlined />}
                onClick={() => navigate("/admin/system/upload-tasks")}
              >
                上传任务
              </Button>
              <Segmented
                value={layoutMode}
                onChange={(value) => setLayoutMode(value as "table" | "grid")}
                options={[
                  { label: "表格", value: "table", icon: <TableOutlined /> },
                  {
                    label: "文件视图",
                    value: "grid",
                    icon: <AppstoreOutlined />,
                  },
                ]}
              />
            </Space>
          </div>
          {uploadingName ? (
            <div className="upload-progress-panel">
              <Typography.Text ellipsis>{uploadingName}</Typography.Text>
              <Progress percent={uploadPercent} size="small" status="active" />
            </div>
          ) : null}
          <Upload.Dragger
            {...uploadProps}
            className="material-drop-zone"
            disabled={!bucketId || uploadMutation.isPending}
            openFileDialogOnClick={false}
          >
            <div className="material-drop-zone-hint">
              <CloudUploadOutlined />
              <Typography.Text type="secondary">
                拖拽文件到这里上传到当前目录
              </Typography.Text>
            </div>
            {layoutMode === "table" ? (
              <Space direction="vertical" size="middle" className="full-width">
                <DataTable<UploadRecord>
                  columns={columns}
                  dataSource={files}
                  loading={uploadsQuery.isLoading}
                  onRow={(record) => ({
                    onDoubleClick: () => openPreview(record),
                  })}
                  pagination={{
                    current: page,
                    total: uploadsQuery.data?.total ?? 0,
                    onChange: setPage,
                  }}
                />
                <Card
                  size="small"
                  title={`桶内未入库对象（${externalObjects.length}）`}
                  className="admin-card material-external-card"
                  extra={
                    <PermissionButton
                      size="small"
                      icon={<CloudSyncOutlined />}
                      permission="system:upload:create"
                      disabled={externalObjects.length === 0}
                      loading={importAllMutation.isPending}
                      onClick={() => importAllMutation.mutate()}
                    >
                      一键入库
                    </PermissionButton>
                  }
                >
                  <DataTable<ExternalObjectRow>
                    columns={externalColumns}
                    dataSource={externalObjects}
                    loading={browserQuery.isLoading}
                    pagination={false}
                    onRow={(record) => ({
                      onDoubleClick: () => openExternalPreview(record),
                    })}
                  />
                </Card>
              </Space>
            ) : (
              <Spin spinning={uploadsQuery.isLoading || browserQuery.isLoading}>
                {folderPrefixes.length > 0 ||
                files.length > 0 ||
                externalObjects.length > 0 ? (
                  <div className="material-card-grid">
                    {folderPrefixes.map((folder) => (
                      <Card
                        key={folder.prefix}
                        hoverable
                        className="material-card material-folder-card"
                        onClick={() => openPrefix(folder.prefix)}
                      >
                        <div className="material-card-preview">
                          <FolderOpenOutlined />
                          <Tag
                            color="blue"
                            className="material-card-visibility"
                          >
                            文件夹
                          </Tag>
                        </div>
                        <Space
                          direction="vertical"
                          size={8}
                          className="full-width"
                        >
                          <Tooltip title={folder.prefix}>
                            <Typography.Text strong ellipsis>
                              {folder.name}
                            </Typography.Text>
                          </Tooltip>
                          <div className="material-card-meta">
                            <span>{folder.prefix}</span>
                          </div>
                          <div className="material-card-actions">
                            <Button
                              size="small"
                              icon={<FolderOpenOutlined />}
                              onClick={() => openPrefix(folder.prefix)}
                            >
                              打开
                            </Button>
                          </div>
                        </Space>
                      </Card>
                    ))}
                    {files.map((record) => (
                      <Card
                        key={record.id}
                        hoverable
                        className="material-card"
                        onDoubleClick={() => openPreview(record)}
                      >
                        <div className="material-card-preview">
                          {fileIcon(record)}
                          <Tag
                            color={
                              record.visibility === "public"
                                ? "green"
                                : "default"
                            }
                            className="material-card-visibility"
                          >
                            {record.visibility === "public" ? "公开" : "私有"}
                          </Tag>
                        </div>
                        <Space
                          direction="vertical"
                          size={8}
                          className="full-width"
                        >
                          <Tooltip title={record.original_name}>
                            <Typography.Text strong ellipsis>
                              {record.original_name}
                            </Typography.Text>
                          </Tooltip>
                          <div className="material-card-meta">
                            <span>{formatBytes(record.size_bytes)}</span>
                            <span>
                              {record.mime_type ??
                                record.extension ??
                                "未知类型"}
                            </span>
                          </div>
                          <div className="material-card-meta">
                            <span>{record.bucket ?? "-"}</span>
                            <span>{record.prefix ?? "根目录"}</span>
                          </div>
                          <Space wrap size={4}>
                            {record.category ? (
                              <Tag>{record.category}</Tag>
                            ) : null}
                            <Tag
                              color={
                                record.status === "active" ? "blue" : "red"
                              }
                            >
                              {record.status}
                            </Tag>
                          </Space>
                          <div className="material-card-actions">
                            {renderActions(record)}
                          </div>
                        </Space>
                      </Card>
                    ))}
                    {externalObjects.map((object) => {
                      const record = externalObjectRecord(object);
                      return (
                        <Card
                          key={object.id}
                          hoverable
                          className="material-card"
                          onDoubleClick={() => openExternalPreview(object)}
                        >
                          <div className="material-card-preview">
                            {fileIcon(record)}
                            <Tag
                              color="orange"
                              className="material-card-visibility"
                            >
                              未入库
                            </Tag>
                          </div>
                          <Space
                            direction="vertical"
                            size={8}
                            className="full-width"
                          >
                            <Tooltip title={object.name}>
                              <Typography.Text strong ellipsis>
                                {object.name}
                              </Typography.Text>
                            </Tooltip>
                            <div className="material-card-meta">
                              <span>{formatBytes(object.size_bytes)}</span>
                              <span>
                                {object.mime_type ??
                                  object.extension ??
                                  "未知类型"}
                              </span>
                            </div>
                            <div className="material-card-meta">
                              <span>{record.bucket ?? "-"}</span>
                              <span>{object.prefix || "根目录"}</span>
                            </div>
                            <Space wrap size={4}>
                              <Tag color="orange">外部对象</Tag>
                              <Tag
                                color={
                                  record.visibility === "public"
                                    ? "green"
                                    : "default"
                                }
                              >
                                {record.visibility === "public"
                                  ? "公开"
                                  : "私有"}
                              </Tag>
                            </Space>
                            <div className="material-card-actions">
                              <Space wrap size={6}>
                                <Button
                                  size="small"
                                  icon={<EyeOutlined />}
                                  onClick={() => openExternalPreview(object)}
                                >
                                  预览
                                </Button>
                                <Button
                                  size="small"
                                  icon={<LinkOutlined />}
                                  onClick={() => copyLink(record)}
                                >
                                  复制链接
                                </Button>
                                <Button
                                  size="small"
                                  icon={<CloudDownloadOutlined />}
                                  onClick={() => downloadRecord(record)}
                                >
                                  下载
                                </Button>
                                <PermissionButton
                                  size="small"
                                  icon={<CloudSyncOutlined />}
                                  permission="system:upload:create"
                                  loading={importMutation.isPending}
                                  onClick={() => importMutation.mutate(object)}
                                >
                                  入库
                                </PermissionButton>
                              </Space>
                            </div>
                          </Space>
                        </Card>
                      );
                    })}
                  </div>
                ) : (
                  <Empty description="当前目录暂无素材，可拖拽文件到此处上传" />
                )}
              </Spin>
            )}
          </Upload.Dragger>
        </Space>
      </div>
      <Drawer
        title="素材详情"
        open={Boolean(detail)}
        onClose={() => setDetail(null)}
        width="min(680px, 96vw)"
      >
        {detail && (
          <Descriptions column={1} bordered size="small">
            <Descriptions.Item label="文件名">
              {detail.original_name}
            </Descriptions.Item>
            <Descriptions.Item label="存储">{detail.storage}</Descriptions.Item>
            <Descriptions.Item label="桶">
              {detail.bucket ?? "-"}
            </Descriptions.Item>
            <Descriptions.Item label="目录">
              {detail.prefix ?? "-"}
            </Descriptions.Item>
            <Descriptions.Item label="对象键">
              {detail.object_key}
            </Descriptions.Item>
            <Descriptions.Item label="公开链接">
              <Space wrap>
                <Typography.Text copyable={Boolean(publicLink(detail))}>
                  {publicLink(detail) ?? "未配置公开链接或文件不是公开状态"}
                </Typography.Text>
                <Button
                  size="small"
                  icon={<LinkOutlined />}
                  onClick={() => copyLink(detail)}
                >
                  复制
                </Button>
              </Space>
            </Descriptions.Item>
            <Descriptions.Item label="URL">
              {detail.url || "-"}
            </Descriptions.Item>
            <Descriptions.Item label="MIME">
              {detail.mime_type ?? "-"}
            </Descriptions.Item>
            <Descriptions.Item label="大小">
              {formatBytes(detail.size_bytes)}
            </Descriptions.Item>
            <Descriptions.Item label="SHA256">
              {detail.sha256}
            </Descriptions.Item>
            <Descriptions.Item label="ETag">
              {detail.etag ?? "-"}
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
        title={previewing ? `预览：${previewing.original_name}` : "素材预览"}
        open={Boolean(previewing)}
        onCancel={closePreview}
        footer={
          previewing ? (
            <Space wrap className="material-preview-actions">
              <Button
                icon={<LinkOutlined />}
                onClick={() => copyLink(previewing)}
              >
                复制公开链接
              </Button>
              <PermissionButton
                icon={<CloudDownloadOutlined />}
                permission="system:upload:download"
                loading={downloadMutation.isPending}
                onClick={() => downloadRecord(previewing)}
              >
                下载
              </PermissionButton>
              <Button onClick={closePreview}>关闭</Button>
            </Space>
          ) : null
        }
        width="min(960px, 96vw)"
        className="material-preview-modal"
      >
        <div className="material-preview-body">{renderPreviewContent()}</div>
      </Modal>
      <Modal
        title="新建文件夹"
        open={folderModalOpen}
        onCancel={() => setFolderModalOpen(false)}
        onOk={() => folderForm.submit()}
        confirmLoading={createFolderMutation.isPending}
      >
        <Form
          form={folderForm}
          layout="vertical"
          onFinish={(values) => createFolderMutation.mutate(values)}
        >
          <Form.Item label="当前目录">
            <Typography.Text>{prefix ?? "根目录"}</Typography.Text>
          </Form.Item>
          <Form.Item
            name="name"
            label="文件夹名称"
            rules={[{ required: true, message: "请输入文件夹名称" }]}
          >
            <Input placeholder="例如 images" />
          </Form.Item>
        </Form>
      </Modal>
      <Modal
        title="重命名文件"
        open={Boolean(renaming)}
        onCancel={() => setRenaming(null)}
        onOk={() => renameForm.submit()}
        confirmLoading={renameMutation.isPending}
      >
        <Form
          form={renameForm}
          layout="vertical"
          onFinish={(values) => renameMutation.mutate(values)}
        >
          <Form.Item label="当前对象键">
            <Typography.Text>{renaming?.object_key ?? "-"}</Typography.Text>
          </Form.Item>
          <Form.Item
            name="original_name"
            label="新文件名"
            rules={[{ required: true, message: "请输入新文件名" }]}
          >
            <Input placeholder="例如 avatar.png" />
          </Form.Item>
        </Form>
      </Modal>
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
