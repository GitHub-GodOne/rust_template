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
  FileAddOutlined,
  FileOutlined,
  FileTextOutlined,
  FolderOpenOutlined,
  FolderOutlined,
  LinkOutlined,
  MoreOutlined,
  SaveOutlined,
  SoundOutlined,
  TableOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Breadcrumb,
  Button,
  Card,
  Descriptions,
  Drawer,
  Dropdown,
  Empty,
  Form,
  Grid,
  Input,
  Modal,
  Popconfirm,
  Popover,
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
import type { MenuProps } from "antd";
import type { UploadProps } from "antd";
import type { ColumnsType } from "antd/es/table";
import type { DataNode } from "antd/es/tree";
import {
  type Key,
  type DragEvent as ReactDragEvent,
  type KeyboardEvent as ReactKeyboardEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
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
  createUploadFile,
  createUploadFolder,
  deleteUpload,
  downloadUpload,
  fetchUploadBrowser,
  fetchUploadContent,
  fetchUploads,
  importUploadObject,
  importUploadObjects,
  previewUpload,
  renameUpload,
  updateUpload,
  updateUploadContent,
  uploadMaterial,
} from "../../../api/admin/uploads";
import { CrudPage } from "../../../components/admin/CrudPage";
import { DataTable } from "../../../components/admin/DataTable";
import { FinderWorkspace } from "../../../components/admin/FinderWorkspace";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { StandardTablePagination } from "../../../components/admin/StandardTablePagination";
import { TableScrollArea } from "../../../components/admin/TableScrollArea";
import { useAuthStore } from "../../../stores/auth";
import { decodeTextBlob } from "../../../utils/textEncoding";
import {
  type ChunkUploadStage,
  extensionFromName,
  fileIcon,
  fileKind,
  formatBytes,
  mimeFromExtension,
  useUploadTaskManager,
} from "./uploadTaskManager";

function publicLink(record: UploadRecord) {
  if (
    record.visibility !== "public" ||
    (record.status !== "active" && record.status !== "external")
  ) {
    return null;
  }
  const candidate = record.public_url?.trim() || record.url?.trim();
  if (candidate) {
    const url = new URL(candidate, window.location.origin);
    const protectedDownloadPath = `/api/admin/uploads/${record.id}/download`;
    if (
      record.status === "external" ||
      url.pathname !== protectedDownloadPath
    ) {
      return url.toString();
    }
  }
  return record.id > 0
    ? new URL(`/api/uploads/${record.id}`, window.location.origin).toString()
    : null;
}

type ExternalObjectRow = StorageObjectRecord & {
  id: string;
  extension?: string | null;
  mime_type?: string | null;
};

type MobileMaterialAction =
  | { kind: "managed"; record: UploadRecord }
  | { kind: "external"; object: ExternalObjectRow };

type FolderFormValues = {
  name: string;
};

type FileFormValues = {
  name: string;
  content?: string;
  visibility: "private" | "public";
};

type RenameFormValues = {
  original_name: string;
};

type AccessFormValues = {
  visibility: "private" | "public";
};

const MAX_EDITABLE_UPLOAD_BYTES = 1024 * 1024;
const MANAGED_UPLOAD_TABLE_SCROLL_X = 1840;
const EXTERNAL_UPLOAD_TABLE_SCROLL_X = 1110;
const EDITABLE_UPLOAD_EXTENSIONS = new Set([
  "bash",
  "c",
  "cc",
  "cfg",
  "cjs",
  "conf",
  "config",
  "cpp",
  "cs",
  "css",
  "csv",
  "cts",
  "cxx",
  "dockerignore",
  "editorconfig",
  "env",
  "eslintrc",
  "fish",
  "gitignore",
  "go",
  "h",
  "hpp",
  "htm",
  "html",
  "ini",
  "java",
  "js",
  "json",
  "jsx",
  "kt",
  "kts",
  "less",
  "lock",
  "log",
  "lua",
  "markdown",
  "md",
  "mjs",
  "mts",
  "npmrc",
  "php",
  "prettierrc",
  "properties",
  "py",
  "rb",
  "rs",
  "sass",
  "scss",
  "sh",
  "sql",
  "svelte",
  "svg",
  "toml",
  "ts",
  "tsx",
  "txt",
  "vue",
  "xml",
  "yaml",
  "yml",
  "zsh",
]);
const EDITABLE_UPLOAD_FILENAMES = new Set([
  "dockerfile",
  "license",
  "makefile",
  "procfile",
  "readme",
]);
const EDITABLE_UPLOAD_MIME_TYPES = new Set([
  "application/javascript",
  "application/json",
  "application/toml",
  "application/xml",
  "application/x-yaml",
  "application/yaml",
]);
const UPLOAD_STAGE_LABELS: Record<ChunkUploadStage, string> = {
  preparing: "正在准备上传",
  uploading: "正在上传到当前目录",
  finishing: "正在完成上传",
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

function isEditableTextUpload(record: UploadRecord) {
  if (record.status !== "active") {
    return false;
  }
  const mimeType = record.mime_type?.toLowerCase() ?? "";
  const extension = record.extension?.toLowerCase() ?? "";
  return (
    mimeType.startsWith("text/") ||
    EDITABLE_UPLOAD_MIME_TYPES.has(mimeType) ||
    EDITABLE_UPLOAD_EXTENSIONS.has(extension) ||
    EDITABLE_UPLOAD_FILENAMES.has(record.original_name.toLowerCase())
  );
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
  const [pageSize, setPageSize] = useState(10);
  const [keyword, setKeyword] = useState("");
  const [status, setStatus] = useState<string>("active");
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
  const [uploadStage, setUploadStage] = useState<ChunkUploadStage>();
  const [uploadDragActive, setUploadDragActive] = useState(false);
  const uploadDragDepth = useRef(0);
  const uploadFeedbackResetTimer = useRef<number>();
  const [detail, setDetail] = useState<UploadRecord | null>(null);
  const [editing, setEditing] = useState<UploadRecord | null>(null);
  const [accessEditing, setAccessEditing] = useState<UploadRecord | null>(null);
  const [renaming, setRenaming] = useState<UploadRecord | null>(null);
  const [folderModalOpen, setFolderModalOpen] = useState(false);
  const [fileModalOpen, setFileModalOpen] = useState(false);
  const [mobileControlsOpen, setMobileControlsOpen] = useState(false);
  const [mobileMaterialAction, setMobileMaterialAction] =
    useState<MobileMaterialAction | null>(null);
  const [treeData, setTreeData] = useState<DataNode[]>([]);
  const [expandedTreeKeys, setExpandedTreeKeys] = useState<Key[]>([""]);
  const [previewing, setPreviewing] = useState<UploadRecord | null>(null);
  const [previewObjectUrl, setPreviewObjectUrl] = useState<string>();
  const [previewText, setPreviewText] = useState<string>();
  const [previewError, setPreviewError] = useState<string>();
  const [previewLoading, setPreviewLoading] = useState(false);
  const [contentEditing, setContentEditing] = useState<UploadRecord | null>(
    null,
  );
  const [editorContent, setEditorContent] = useState("");
  const [editorOriginalContent, setEditorOriginalContent] = useState("");
  const [editorUpdatedAt, setEditorUpdatedAt] = useState<string>();
  const [editorLoading, setEditorLoading] = useState(false);
  const editorRequestId = useRef(0);
  const [form] = Form.useForm<UpdateUploadParams>();
  const [accessForm] = Form.useForm<AccessFormValues>();
  const accessVisibility = Form.useWatch("visibility", accessForm);
  const [folderForm] = Form.useForm<FolderFormValues>();
  const [fileForm] = Form.useForm<FileFormValues>();
  const [renameForm] = Form.useForm<RenameFormValues>();
  const queryClient = useQueryClient();
  const hasPermission = useAuthStore((state) => state.hasPermission);
  const navigate = useNavigate();
  const screens = Grid.useBreakpoint();
  const effectiveLayoutMode = screens.md === false ? "grid" : layoutMode;
  const uploadTaskManager = useUploadTaskManager({
    bucketId,
    prefix,
    visibility: uploadVisibility,
  });
  const editorByteSize = useMemo(
    () => new TextEncoder().encode(editorContent).length,
    [editorContent],
  );
  const editorDirty = editorContent !== editorOriginalContent;
  const editorOverLimit = editorByteSize > MAX_EDITABLE_UPLOAD_BYTES;

  const resetUploadFeedback = useCallback(() => {
    if (uploadFeedbackResetTimer.current !== undefined) {
      window.clearTimeout(uploadFeedbackResetTimer.current);
      uploadFeedbackResetTimer.current = undefined;
    }
    setUploadingName(undefined);
    setUploadPercent(0);
    setUploadStage(undefined);
  }, []);

  const beginUploadFeedback = useCallback((fileName: string) => {
    if (uploadFeedbackResetTimer.current !== undefined) {
      window.clearTimeout(uploadFeedbackResetTimer.current);
      uploadFeedbackResetTimer.current = undefined;
    }
    setUploadingName(fileName);
    setUploadPercent(0);
    setUploadStage("preparing");
  }, []);

  const finishUploadFeedback = useCallback(() => {
    if (uploadFeedbackResetTimer.current !== undefined) {
      window.clearTimeout(uploadFeedbackResetTimer.current);
    }
    setUploadPercent(100);
    setUploadStage("finishing");
    uploadFeedbackResetTimer.current = window.setTimeout(() => {
      setUploadingName(undefined);
      setUploadPercent(0);
      setUploadStage(undefined);
      uploadFeedbackResetTimer.current = undefined;
    }, 700);
  }, []);

  useEffect(
    () => () => {
      if (uploadFeedbackResetTimer.current !== undefined) {
        window.clearTimeout(uploadFeedbackResetTimer.current);
      }
    },
    [],
  );

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
      pageSize,
      keyword,
      status,
      profileId,
      bucketId,
      prefix,
    ],
    queryFn: () =>
      fetchUploads({
        page,
        page_size: pageSize,
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
        onUploadProgress: (percent) => {
          setUploadStage("uploading");
          setUploadPercent(percent);
        },
      }),
    onSuccess: () => {
      message.success("素材已上传");
      finishUploadFeedback();
      queryClient.invalidateQueries({ queryKey: ["admin-uploads"] });
      queryClient.invalidateQueries({ queryKey: ["admin-upload-browser"] });
    },
    onError: (error) => {
      const reason = error instanceof Error ? error.message : "素材上传失败";
      message.error(reason);
      resetUploadFeedback();
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
  const updateContentMutation = useMutation({
    mutationFn: (variables: { record: UploadRecord; content: string }) =>
      updateUploadContent(variables.record.id, {
        content: variables.content,
      }),
    onSuccess: (record, variables) => {
      message.success("文本内容已保存");
      setContentEditing(record);
      setEditorOriginalContent(variables.content);
      setEditorUpdatedAt(record.updated_at);
      if (detail?.id === record.id) {
        setDetail(record);
      }
      queryClient.invalidateQueries({ queryKey: ["admin-uploads"] });
      queryClient.invalidateQueries({ queryKey: ["admin-upload-browser"] });
    },
    onError: (error) => {
      message.error(
        error instanceof Error ? error.message : "文本内容保存失败",
      );
    },
  });
  const accessMutation = useMutation({
    mutationFn: (values: AccessFormValues) => {
      if (!accessEditing) {
        throw new Error("请选择素材");
      }
      return updateUpload(accessEditing.id, { visibility: values.visibility });
    },
    onSuccess: (record) => {
      message.success("访问设置已保存");
      setAccessEditing(null);
      accessForm.resetFields();
      if (detail?.id === record.id) {
        setDetail(record);
      }
      if (previewing?.id === record.id) {
        setPreviewing(record);
      }
      queryClient.invalidateQueries({ queryKey: ["admin-uploads"] });
    },
    onError: (error) => {
      message.error(
        error instanceof Error ? error.message : "访问设置保存失败",
      );
    },
  });
  const deleteMutation = useMutation({
    mutationFn: deleteUpload,
    onSuccess: (_, deletedId) => {
      message.success("素材已移至已删除");
      if (detail?.id === deletedId) {
        setDetail(null);
      }
      if (previewing?.id === deletedId) {
        closePreview();
      }
      if (contentEditing?.id === deletedId) {
        dismissContentEditor();
      }
      queryClient.invalidateQueries({ queryKey: ["admin-uploads"] });
      queryClient.invalidateQueries({ queryKey: ["admin-upload-browser"] });
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
  const createFileMutation = useMutation({
    mutationFn: (values: FileFormValues) => {
      if (!bucketId) {
        throw new Error("请先选择存储桶");
      }
      return createUploadFile({
        storage_bucket_id: bucketId,
        prefix,
        name: values.name,
        content: values.content ?? "",
        visibility: values.visibility,
      });
    },
    onSuccess: () => {
      message.success("文件已创建并入库");
      setFileModalOpen(false);
      fileForm.resetFields();
      queryClient.invalidateQueries({ queryKey: ["admin-uploads"] });
      queryClient.invalidateQueries({ queryKey: ["admin-upload-browser"] });
    },
    onError: (error) => {
      message.error(error instanceof Error ? error.message : "创建文件失败");
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
      uploadDragDepth.current = 0;
      setUploadDragActive(false);
      beginUploadFeedback(uploadFile.name);
      if (uploadMode === "chunk") {
        void uploadTaskManager.startChunkUpload(uploadFile, undefined, {
          onStageChange: setUploadStage,
          onProgress: setUploadPercent,
          onComplete: () => {
            finishUploadFeedback();
            onSuccess?.("ok");
          },
          onPaused: resetUploadFeedback,
          onError: (error) => {
            resetUploadFeedback();
            onError?.(
              error instanceof Error ? error : new Error("分片上传失败"),
            );
          },
        });
        return;
      }
      uploadMutation.mutate(uploadFile, {
        onSuccess: () => onSuccess?.("ok"),
        onError: (error) => onError?.(error as Error),
      });
    },
  };

  const isUploadBusy = uploadMutation.isPending || Boolean(uploadingName);
  const uploadStageLabel = UPLOAD_STAGE_LABELS[uploadStage ?? "preparing"];
  const canAcceptUpload = Boolean(bucketId) && !isUploadBusy;
  const isFileDrag = (event: ReactDragEvent<HTMLDivElement>) =>
    Array.from(event.dataTransfer.types).includes("Files");
  const handleUploadDragEnter = (event: ReactDragEvent<HTMLDivElement>) => {
    if (!canAcceptUpload || !isFileDrag(event)) {
      return;
    }
    event.preventDefault();
    uploadDragDepth.current += 1;
    setUploadDragActive(true);
  };
  const handleUploadDragOver = (event: ReactDragEvent<HTMLDivElement>) => {
    if (!canAcceptUpload || !isFileDrag(event)) {
      return;
    }
    event.preventDefault();
    event.dataTransfer.dropEffect = "copy";
  };
  const handleUploadDragLeave = () => {
    uploadDragDepth.current = Math.max(0, uploadDragDepth.current - 1);
    if (uploadDragDepth.current === 0) {
      setUploadDragActive(false);
    }
  };
  const handleUploadDrop = () => {
    uploadDragDepth.current = 0;
    setUploadDragActive(false);
  };

  const openPrefix = (nextPrefix?: string) => {
    setPrefix(nextPrefix);
    setPage(1);
  };

  const openRename = (record: UploadRecord) => {
    setRenaming(record);
    renameForm.setFieldsValue({ original_name: record.original_name });
  };

  const openAccessEditor = (record: UploadRecord) => {
    setAccessEditing(record);
    accessForm.setFieldsValue({
      visibility: record.visibility === "public" ? "public" : "private",
    });
  };

  const currentPathSegments = pathSegments(prefix);
  const folderPrefixes = browserQuery.data?.prefixes ?? [];

  const openPreview = async (record: UploadRecord) => {
    setPreviewing(record);
    setPreviewObjectUrl(undefined);
    setPreviewText(undefined);
    setPreviewError(undefined);
    setPreviewLoading(true);
    try {
      const blob = await previewUpload(record.id);
      if (isEditableTextUpload(record)) {
        setPreviewText(await decodeTextBlob(blob));
      } else {
        setPreviewObjectUrl(URL.createObjectURL(blob));
      }
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
    setPreviewText(undefined);
    setPreviewError(undefined);
    setPreviewLoading(false);
  };

  const dismissContentEditor = () => {
    editorRequestId.current += 1;
    setContentEditing(null);
    setEditorContent("");
    setEditorOriginalContent("");
    setEditorUpdatedAt(undefined);
    setEditorLoading(false);
  };

  const closeContentEditor = () => {
    if (!editorDirty) {
      dismissContentEditor();
      return;
    }
    Modal.confirm({
      title: "放弃未保存的修改？",
      content: "关闭编辑器后，本次修改将不会保留。",
      okText: "放弃修改",
      okButtonProps: { danger: true },
      cancelText: "继续编辑",
      onOk: dismissContentEditor,
    });
  };

  const openContentEditor = async (record: UploadRecord) => {
    if (!isEditableTextUpload(record)) {
      message.warning("该素材不是可编辑的文本文件");
      return;
    }
    const requestId = editorRequestId.current + 1;
    editorRequestId.current = requestId;
    closePreview();
    setMobileMaterialAction(null);
    setContentEditing(record);
    setEditorContent("");
    setEditorOriginalContent("");
    setEditorUpdatedAt(record.updated_at);
    setEditorLoading(true);
    try {
      const data = await fetchUploadContent(record.id);
      if (editorRequestId.current !== requestId) {
        return;
      }
      setEditorContent(data.content);
      setEditorOriginalContent(data.content);
      setEditorUpdatedAt(data.updated_at ?? record.updated_at);
    } catch (error) {
      if (editorRequestId.current !== requestId) {
        return;
      }
      setContentEditing(null);
      message.error(
        error instanceof Error ? error.message : "文本内容加载失败",
      );
    } finally {
      if (editorRequestId.current === requestId) {
        setEditorLoading(false);
      }
    }
  };

  const saveContent = () => {
    if (!contentEditing) {
      message.error("请选择要编辑的文本素材");
      return;
    }
    if (editorOverLimit) {
      message.error("文件超过 1 MiB，无法保存");
      return;
    }
    updateContentMutation.mutate({
      record: contentEditing,
      content: editorContent,
    });
  };

  const handleEditorKeyDown = (
    event: ReactKeyboardEvent<HTMLTextAreaElement>,
  ) => {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
      event.preventDefault();
      if (editorDirty && !editorOverLimit && !updateContentMutation.isPending) {
        saveContent();
      }
      return;
    }
    if (event.key !== "Tab") {
      return;
    }
    event.preventDefault();
    const target = event.currentTarget;
    const start = target.selectionStart;
    const end = target.selectionEnd;
    const indentation = "  ";
    setEditorContent(
      `${target.value.slice(0, start)}${indentation}${target.value.slice(end)}`,
    );
    requestAnimationFrame(() => {
      target.setSelectionRange(
        start + indentation.length,
        start + indentation.length,
      );
    });
  };

  const copyLink = async (record: UploadRecord) => {
    const link = publicLink(record);
    if (!link) {
      if (record.visibility !== "public") {
        message.warning(
          "该文件是私有文件，请先在访问设置中改为公开再复制分享链接",
        );
      } else {
        message.warning("当前文件暂时无法生成公开链接");
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
    <Space wrap size={6} className="material-table-actions">
      <PermissionButton
        size="small"
        icon={<EyeOutlined />}
        permission="system:upload:download"
        onClick={() => openPreview(record)}
      >
        预览
      </PermissionButton>
      {isEditableTextUpload(record) ? (
        <PermissionButton
          size="small"
          icon={<FileTextOutlined />}
          permission="system:upload:update"
          onClick={() => openContentEditor(record)}
        >
          编辑文本
        </PermissionButton>
      ) : null}
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
        icon={<LinkOutlined />}
        permission="system:upload:update"
        onClick={() => openAccessEditor(record)}
      >
        访问设置
      </PermissionButton>
      <PermissionButton
        size="small"
        icon={<EditOutlined />}
        permission="system:upload:update"
        onClick={() => editRecord(record)}
      >
        编辑信息
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

  const renderExternalActions = (object: ExternalObjectRow) => {
    const record = externalObjectRecord(object);
    return (
      <Space wrap size={6} className="material-table-actions">
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
    );
  };

  const renderManagedTableActions = (record: UploadRecord) => {
    const items: MenuProps["items"] = [
      isEditableTextUpload(record) && hasPermission("system:upload:download")
        ? {
            key: "preview",
            icon: <EyeOutlined />,
            label: "预览",
            onClick: () => openPreview(record),
          }
        : null,
      {
        key: "copy-link",
        icon: <LinkOutlined />,
        label: "复制链接",
        onClick: () => copyLink(record),
      },
      hasPermission("system:upload:download")
        ? {
            key: "download",
            icon: <CloudDownloadOutlined />,
            label: "下载",
            onClick: () => downloadRecord(record),
          }
        : null,
      hasPermission("system:upload:detail")
        ? {
            key: "detail",
            icon: <FileOutlined />,
            label: "详情",
            onClick: () => setDetail(record),
          }
        : null,
      hasPermission("system:upload:update")
        ? {
            key: "access",
            icon: <LinkOutlined />,
            label: "访问设置",
            onClick: () => openAccessEditor(record),
          }
        : null,
      hasPermission("system:upload:update")
        ? {
            key: "edit",
            icon: <EditOutlined />,
            label: "编辑信息",
            onClick: () => editRecord(record),
          }
        : null,
      hasPermission("system:upload:update")
        ? {
            key: "rename",
            icon: <EditOutlined />,
            label: "重命名",
            onClick: () => openRename(record),
          }
        : null,
      hasPermission("system:upload:delete") ? { type: "divider" } : null,
      hasPermission("system:upload:delete")
        ? {
            key: "delete",
            icon: <DeleteOutlined />,
            label: "删除",
            danger: true,
            onClick: () =>
              Modal.confirm({
                title: "确认删除素材？",
                okText: "删除",
                okButtonProps: { danger: true },
                cancelText: "取消",
                onOk: () => deleteMutation.mutateAsync(record.id),
              }),
          }
        : null,
    ];

    return (
      <Space size={4} className="material-table-compact-actions">
        {isEditableTextUpload(record) &&
        hasPermission("system:upload:update") ? (
          <PermissionButton
            size="small"
            icon={<FileTextOutlined />}
            permission="system:upload:update"
            onClick={() => openContentEditor(record)}
          >
            编辑文本
          </PermissionButton>
        ) : (
          <PermissionButton
            size="small"
            icon={<EyeOutlined />}
            permission="system:upload:download"
            onClick={() => openPreview(record)}
          >
            预览
          </PermissionButton>
        )}
        <Dropdown menu={{ items }} trigger={["click"]} placement="bottomRight">
          <Button
            size="small"
            icon={<MoreOutlined />}
            aria-label="更多素材操作"
          />
        </Dropdown>
      </Space>
    );
  };

  const renderExternalTableActions = (object: ExternalObjectRow) => {
    const record = externalObjectRecord(object);
    const items: MenuProps["items"] = [
      {
        key: "copy-link",
        icon: <LinkOutlined />,
        label: "复制链接",
        onClick: () => copyLink(record),
      },
      {
        key: "download",
        icon: <CloudDownloadOutlined />,
        label: "下载",
        onClick: () => downloadRecord(record),
      },
      hasPermission("system:upload:create")
        ? {
            key: "import",
            icon: <CloudSyncOutlined />,
            label: "入库",
            onClick: () => importMutation.mutate(object),
          }
        : null,
    ];

    return (
      <Space size={4} className="material-table-compact-actions">
        <Button
          size="small"
          icon={<EyeOutlined />}
          onClick={() => openExternalPreview(object)}
        >
          预览
        </Button>
        <Dropdown menu={{ items }} trigger={["click"]} placement="bottomRight">
          <Button
            size="small"
            icon={<MoreOutlined />}
            aria-label="更多对象操作"
          />
        </Dropdown>
      </Space>
    );
  };

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
      width: 150,
      render: (_, record) => renderManagedTableActions(record),
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
      width: 150,
      render: (_, record) => renderExternalTableActions(record),
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
    if (previewText !== undefined) {
      return <pre className="material-preview-text">{previewText}</pre>;
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
    () =>
      new Set([
        ...(browserQuery.data?.managed_object_keys ?? []),
        ...files.map((file) => file.object_key),
      ]),
    [browserQuery.data?.managed_object_keys, files],
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
      className="finder-page finder-uploads-page"
      title="素材库"
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
      <FinderWorkspace
        title="素材库"
        icon={<FolderOpenOutlined />}
        meta={`${selectedBucket?.name ?? "未选择桶"} · ${prefix ?? "根目录"}`}
        status={`${folderPrefixes.length} 个文件夹 · ${uploadsQuery.data?.total ?? files.length} 个已入库 · ${externalObjects.length} 个待入库`}
        statusAside={selectedProfile?.provider ?? "未选择存储"}
      >
        <div className="resource-library-layout uploads-resource-library">
          <aside className="resource-tree-panel">
            <Space className="full-width" direction="vertical" size="small">
              <div className="finder-sidebar-label">位置</div>
              <div className="finder-location-summary">
                <strong>{selectedBucket?.name ?? "尚未选择桶"}</strong>
                <span>{selectedProfile?.name ?? "请选择存储配置"}</span>
              </div>
              <div className="finder-sidebar-label finder-sidebar-label-secondary">
                目录
              </div>
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
          </aside>
          <Space
            direction="vertical"
            size="middle"
            className="full-width resource-content-panel"
          >
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
            <div className="material-library-toolbar uploads-library-toolbar">
              <Space
                wrap
                className="material-upload-inline-actions uploads-desktop-controls"
              >
                <Typography.Text type="secondary">上传方式</Typography.Text>
                <Segmented
                  value={uploadMode}
                  onChange={(value) =>
                    setUploadMode(value as "normal" | "chunk")
                  }
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
                <Upload {...uploadProps} disabled={!bucketId || isUploadBusy}>
                  <PermissionButton
                    icon={<CloudUploadOutlined />}
                    permission="system:upload:create"
                    hideWhenDenied={false}
                    disabled={!bucketId || isUploadBusy}
                    loading={isUploadBusy}
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
                  icon={<FileAddOutlined />}
                  permission="system:upload:create"
                  disabled={!bucketId}
                  onClick={() => {
                    fileForm.setFieldsValue({
                      name: "新建文件.txt",
                      content: "",
                      visibility: uploadVisibility,
                    });
                    setFileModalOpen(true);
                  }}
                >
                  新建文件
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
              <div className="uploads-mobile-quick-actions">
                <Upload {...uploadProps} disabled={!bucketId || isUploadBusy}>
                  <PermissionButton
                    type="primary"
                    icon={<CloudUploadOutlined />}
                    permission="system:upload:create"
                    hideWhenDenied={false}
                    disabled={!bucketId || isUploadBusy}
                    loading={isUploadBusy}
                  >
                    上传文件
                  </PermissionButton>
                </Upload>
                <Button
                  icon={<MoreOutlined />}
                  onClick={() => setMobileControlsOpen(true)}
                >
                  更多
                </Button>
              </div>
            </div>
            {uploadingName ? (
              <div
                className="upload-progress-panel material-upload-progress-mobile"
                aria-live="polite"
              >
                <div className="material-upload-mobile-status">
                  <Spin size="small" />
                  <div className="material-upload-mobile-copy">
                    <Typography.Text strong ellipsis title={uploadingName}>
                      {uploadingName}
                    </Typography.Text>
                    <Typography.Text type="secondary">
                      {uploadStageLabel}
                    </Typography.Text>
                  </div>
                  <Typography.Text className="material-upload-progress-percent">
                    {uploadPercent}%
                  </Typography.Text>
                </div>
                <Progress
                  percent={uploadPercent}
                  size="small"
                  status="active"
                  showInfo={false}
                />
              </div>
            ) : null}
            <div
              className="material-drop-zone-shell"
              onDragEnter={handleUploadDragEnter}
              onDragOver={handleUploadDragOver}
              onDragLeave={handleUploadDragLeave}
              onDrop={handleUploadDrop}
            >
              <Upload.Dragger
                {...uploadProps}
                className={`material-drop-zone${
                  uploadDragActive ? " is-dragging" : ""
                }${uploadingName ? " is-uploading" : ""}`}
                disabled={!bucketId || isUploadBusy}
                openFileDialogOnClick={false}
              >
                {uploadingName ? (
                  <div
                    className="material-drop-zone-progress"
                    aria-live="polite"
                  >
                    <Spin size="small" />
                    <div className="material-drop-zone-progress-copy">
                      <Typography.Text strong>
                        {uploadStageLabel}
                      </Typography.Text>
                      <Typography.Text
                        type="secondary"
                        ellipsis
                        title={uploadingName}
                      >
                        {uploadingName}
                      </Typography.Text>
                    </div>
                    <Typography.Text className="material-upload-progress-percent">
                      {uploadPercent}%
                    </Typography.Text>
                    <Progress
                      className="material-drop-zone-progress-bar"
                      percent={uploadPercent}
                      size="small"
                      status="active"
                      showInfo={false}
                    />
                  </div>
                ) : (
                  <div className="material-drop-zone-hint">
                    <CloudUploadOutlined />
                    <Typography.Text
                      type={uploadDragActive ? undefined : "secondary"}
                    >
                      {uploadDragActive
                        ? "松开即可上传到当前目录"
                        : "拖拽文件到这里上传到当前目录"}
                    </Typography.Text>
                  </div>
                )}
              </Upload.Dragger>
            </div>
            {effectiveLayoutMode === "table" ? (
              <Space direction="vertical" size="middle" className="full-width">
                <TableScrollArea
                  contentWidth={MANAGED_UPLOAD_TABLE_SCROLL_X}
                  className="uploads-managed-table-scroll-area"
                  label="素材库已入库表格"
                  footer={
                    <StandardTablePagination
                      current={page}
                      pageSize={pageSize}
                      total={uploadsQuery.data?.total ?? 0}
                      onChange={(nextPage, nextPageSize) => {
                        const sizeChanged = nextPageSize !== pageSize;
                        setPageSize(nextPageSize);
                        setPage(sizeChanged ? 1 : nextPage);
                      }}
                    />
                  }
                >
                  <DataTable<UploadRecord>
                    columns={columns}
                    dataSource={files}
                    loading={uploadsQuery.isLoading}
                    tableLayout="fixed"
                    scroll={{ x: MANAGED_UPLOAD_TABLE_SCROLL_X }}
                    onRow={(record) => ({
                      onDoubleClick: () =>
                        isEditableTextUpload(record) &&
                        hasPermission("system:upload:update")
                          ? openContentEditor(record)
                          : openPreview(record),
                    })}
                    pagination={false}
                  />
                </TableScrollArea>
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
                  <TableScrollArea
                    contentWidth={EXTERNAL_UPLOAD_TABLE_SCROLL_X}
                    className="uploads-external-table-scroll-area"
                    label="素材库未入库对象表格"
                  >
                    <DataTable<ExternalObjectRow>
                      columns={externalColumns}
                      dataSource={externalObjects}
                      loading={browserQuery.isLoading}
                      pagination={false}
                      tableLayout="fixed"
                      scroll={{ x: EXTERNAL_UPLOAD_TABLE_SCROLL_X }}
                      onRow={(record) => ({
                        onDoubleClick: () => openExternalPreview(record),
                      })}
                    />
                  </TableScrollArea>
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
                          className="full-width uploads-card-details"
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
                        onDoubleClick={() =>
                          isEditableTextUpload(record) &&
                          hasPermission("system:upload:update")
                            ? openContentEditor(record)
                            : openPreview(record)
                        }
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
                        <Popover
                          trigger="click"
                          placement="bottomRight"
                          content={
                            <div className="finder-popover-actions">
                              {renderActions(record)}
                            </div>
                          }
                        >
                          <Button
                            type="text"
                            className="finder-item-more"
                            icon={<MoreOutlined />}
                            aria-label={`更多操作：${record.original_name}`}
                            onClick={(event) => event.stopPropagation()}
                            onDoubleClick={(event) => event.stopPropagation()}
                          />
                        </Popover>
                        <Space
                          direction="vertical"
                          size={8}
                          className="full-width uploads-card-details"
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
                          <Space wrap size={4} className="uploads-card-tags">
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
                          <div className="material-card-actions uploads-card-actions-desktop">
                            {renderActions(record)}
                          </div>
                          <div className="uploads-card-actions-mobile">
                            {isEditableTextUpload(record) &&
                            hasPermission("system:upload:update") ? (
                              <PermissionButton
                                type="primary"
                                size="small"
                                icon={<FileTextOutlined />}
                                aria-label="编辑文本"
                                permission="system:upload:update"
                                onClick={() => openContentEditor(record)}
                              >
                                编辑文本
                              </PermissionButton>
                            ) : (
                              <PermissionButton
                                type="primary"
                                size="small"
                                icon={<EyeOutlined />}
                                aria-label="预览"
                                permission="system:upload:download"
                                onClick={() => openPreview(record)}
                              >
                                预览
                              </PermissionButton>
                            )}
                            <Button
                              size="small"
                              icon={<MoreOutlined />}
                              aria-label={`更多操作：${record.original_name}`}
                              onClick={() =>
                                setMobileMaterialAction({
                                  kind: "managed",
                                  record,
                                })
                              }
                            >
                              更多
                            </Button>
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
                          <Popover
                            trigger="click"
                            placement="bottomRight"
                            content={
                              <div className="finder-popover-actions">
                                {renderExternalActions(object)}
                              </div>
                            }
                          >
                            <Button
                              type="text"
                              className="finder-item-more"
                              icon={<MoreOutlined />}
                              aria-label={`更多操作：${object.name}`}
                              onClick={(event) => event.stopPropagation()}
                              onDoubleClick={(event) => event.stopPropagation()}
                            />
                          </Popover>
                          <Space
                            direction="vertical"
                            size={8}
                            className="full-width uploads-card-details"
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
                            <Space wrap size={4} className="uploads-card-tags">
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
                            <div className="material-card-actions uploads-card-actions-desktop">
                              {renderExternalActions(object)}
                            </div>
                            <div className="uploads-card-actions-mobile">
                              <Button
                                type="primary"
                                size="small"
                                icon={<EyeOutlined />}
                                onClick={() => openExternalPreview(object)}
                              >
                                预览
                              </Button>
                              <Button
                                size="small"
                                icon={<MoreOutlined />}
                                onClick={() =>
                                  setMobileMaterialAction({
                                    kind: "external",
                                    object,
                                  })
                                }
                              >
                                更多
                              </Button>
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
          </Space>
        </div>
      </FinderWorkspace>
      <Drawer
        title="素材库操作"
        open={mobileControlsOpen}
        placement="bottom"
        height="auto"
        className="uploads-mobile-controls-drawer"
        onClose={() => setMobileControlsOpen(false)}
      >
        <Space direction="vertical" size={14} className="admin-full-width">
          <div className="uploads-mobile-setting-row">
            <Typography.Text>上传方式</Typography.Text>
            <Segmented
              value={uploadMode}
              onChange={(value) => setUploadMode(value as "normal" | "chunk")}
              options={[
                { label: "分片", value: "chunk" },
                { label: "普通", value: "normal" },
              ]}
            />
          </div>
          <div className="uploads-mobile-setting-row">
            <Typography.Text>可见性</Typography.Text>
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
          </div>
          <div className="uploads-mobile-control-grid">
            <PermissionButton
              icon={<FileAddOutlined />}
              permission="system:upload:create"
              disabled={!bucketId}
              onClick={() => {
                setMobileControlsOpen(false);
                fileForm.setFieldsValue({
                  name: "新建文件.txt",
                  content: "",
                  visibility: uploadVisibility,
                });
                setFileModalOpen(true);
              }}
            >
              新建文件
            </PermissionButton>
            <PermissionButton
              icon={<FolderOutlined />}
              permission="system:upload:create"
              disabled={!bucketId}
              onClick={() => {
                setMobileControlsOpen(false);
                setFolderModalOpen(true);
              }}
            >
              新建文件夹
            </PermissionButton>
            <PermissionButton
              icon={<CloudSyncOutlined />}
              permission="system:upload:create"
              disabled={!bucketId || externalObjects.length === 0}
              loading={importAllMutation.isPending}
              onClick={() => {
                setMobileControlsOpen(false);
                importAllMutation.mutate();
              }}
            >
              一键入库
            </PermissionButton>
            <Button
              icon={<CloudSyncOutlined />}
              onClick={() => navigate("/admin/system/upload-tasks")}
            >
              上传任务
            </Button>
          </div>
        </Space>
      </Drawer>
      <Drawer
        title={
          mobileMaterialAction?.kind === "managed"
            ? mobileMaterialAction.record.original_name
            : (mobileMaterialAction?.object.name ?? "素材操作")
        }
        open={Boolean(mobileMaterialAction)}
        placement="bottom"
        height="auto"
        className="uploads-mobile-action-drawer"
        onClose={() => setMobileMaterialAction(null)}
      >
        <div className="uploads-mobile-action-sheet">
          {mobileMaterialAction?.kind === "managed"
            ? renderActions(mobileMaterialAction.record)
            : mobileMaterialAction?.kind === "external"
              ? renderExternalActions(mobileMaterialAction.object)
              : null}
        </div>
      </Drawer>
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
              {isEditableTextUpload(previewing) ? (
                <PermissionButton
                  icon={<FileTextOutlined />}
                  permission="system:upload:update"
                  onClick={() => {
                    const record = previewing;
                    closePreview();
                    openContentEditor(record);
                  }}
                >
                  编辑文本
                </PermissionButton>
              ) : null}
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
        title={
          contentEditing
            ? `编辑文本：${contentEditing.original_name}`
            : "编辑文本"
        }
        open={Boolean(contentEditing)}
        onCancel={closeContentEditor}
        maskClosable={false}
        width="min(1120px, 96vw)"
        className="managed-file-editor-modal"
        footer={
          <div className="managed-file-editor-footer">
            <Typography.Text
              type={editorOverLimit ? "danger" : "secondary"}
              className="managed-file-editor-size"
            >
              {formatBytes(editorByteSize)} / 1 MiB
            </Typography.Text>
            <Space>
              <Button onClick={closeContentEditor}>关闭</Button>
              <Button
                type="primary"
                icon={<SaveOutlined />}
                loading={updateContentMutation.isPending}
                disabled={
                  editorLoading ||
                  !editorDirty ||
                  editorOverLimit ||
                  updateContentMutation.isPending
                }
                onClick={saveContent}
              >
                保存
              </Button>
            </Space>
          </div>
        }
      >
        <Spin spinning={editorLoading} tip="正在读取文本内容">
          <div className="managed-file-editor-shell">
            <div className="managed-file-editor-meta">
              <Space wrap size={6}>
                <Tag>UTF-8</Tag>
                <Typography.Text type="secondary" ellipsis>
                  {contentEditing?.object_key ?? "-"}
                </Typography.Text>
              </Space>
              <Typography.Text type="secondary">
                {editorUpdatedAt
                  ? `更新于 ${new Date(editorUpdatedAt).toLocaleString("zh-CN")}`
                  : "尚无更新时间"}
              </Typography.Text>
            </div>
            <Input.TextArea
              aria-label="素材文本编辑器"
              className="managed-file-editor-textarea"
              value={editorContent}
              disabled={editorLoading || updateContentMutation.isPending}
              autoSize={false}
              spellCheck={false}
              placeholder="输入文本内容"
              onChange={(event) => setEditorContent(event.target.value)}
              onKeyDown={handleEditorKeyDown}
            />
            {editorOverLimit ? (
              <Typography.Text type="danger">
                当前内容超过 1 MiB，请缩减后再保存。
              </Typography.Text>
            ) : null}
          </div>
        </Spin>
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
        title="新建文件"
        open={fileModalOpen}
        onCancel={() => {
          setFileModalOpen(false);
          fileForm.resetFields();
        }}
        onOk={() => fileForm.submit()}
        confirmLoading={createFileMutation.isPending}
      >
        <Form
          form={fileForm}
          layout="vertical"
          initialValues={{ visibility: "private" }}
          onFinish={(values) => createFileMutation.mutate(values)}
        >
          <Form.Item label="当前目录">
            <Typography.Text>{prefix ?? "根目录"}</Typography.Text>
          </Form.Item>
          <Form.Item
            name="name"
            label="文件名称"
            rules={[
              { required: true, message: "请输入文件名称" },
              {
                pattern: /^[^/\\]+$/,
                message: "文件名称不能包含 / 或 \\",
              },
            ]}
          >
            <Input placeholder="例如 notes.txt" autoFocus />
          </Form.Item>
          <Form.Item name="visibility" label="可见性">
            <Segmented
              options={[
                { label: "私有", value: "private" },
                { label: "公开", value: "public" },
              ]}
            />
          </Form.Item>
          <Form.Item name="content" label="初始内容（可选）">
            <Input.TextArea
              rows={8}
              placeholder="可留空，内容会随文件一同保存"
              spellCheck={false}
            />
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
        title="访问设置"
        open={Boolean(accessEditing)}
        onCancel={() => setAccessEditing(null)}
        onOk={() => accessForm.submit()}
        confirmLoading={accessMutation.isPending}
      >
        <Form
          form={accessForm}
          layout="vertical"
          onFinish={(values) => accessMutation.mutate(values)}
        >
          <Form.Item label="文件名">
            <Typography.Text>
              {accessEditing?.original_name ?? "-"}
            </Typography.Text>
          </Form.Item>
          <Form.Item label="对象键">
            <Typography.Text copyable>
              {accessEditing?.object_key ?? "-"}
            </Typography.Text>
          </Form.Item>
          <Form.Item name="visibility" label="访问范围">
            <Segmented
              options={[
                { value: "private", label: "私有" },
                { value: "public", label: "公开" },
              ]}
            />
          </Form.Item>
          {accessVisibility === "public" ? (
            accessEditing ? (
              <Form.Item label="公开链接">
                <Typography.Text copyable>
                  {publicLink({ ...accessEditing, visibility: "public" })}
                </Typography.Text>
              </Form.Item>
            ) : (
              <Typography.Text type="warning">
                文件信息尚未加载完成。
              </Typography.Text>
            )
          ) : (
            <Typography.Text type="secondary">
              私有素材只能通过后台登录后的预览和下载接口访问，不会复制公开分享链接。
            </Typography.Text>
          )}
        </Form>
      </Modal>
      <Modal
        title="编辑素材信息"
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
