import {
  CheckOutlined,
  CloudDownloadOutlined,
  CloudServerOutlined,
  CloudSyncOutlined,
  CloudUploadOutlined,
  DatabaseOutlined,
  DeleteOutlined,
  EditOutlined,
  EllipsisOutlined,
  ExclamationCircleOutlined,
  EyeOutlined,
  FileAddOutlined,
  FileOutlined,
  FileTextOutlined,
  FolderOpenOutlined,
  FolderOutlined,
  LinkOutlined,
  PlusOutlined,
  ReloadOutlined,
  SaveOutlined,
  SoundOutlined,
  UnorderedListOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Button,
  Descriptions,
  Drawer,
  Dropdown,
  Empty,
  Form,
  Input,
  Modal,
  Pagination,
  Segmented,
  Select,
  Space,
  Spin,
  Tag,
  Tooltip,
  Typography,
  Upload,
  message,
} from "antd";
import type { MenuProps } from "antd";
import type { UploadProps } from "antd";
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
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { FinderItems } from "../../../components/admin/finder/FinderItems";
import {
  FinderProgress,
  FinderShell,
  FinderSidebarItem,
  FinderSidebarSection,
  FinderTree,
} from "../../../components/admin/finder/FinderShell";
import {
  type FinderItem,
  finderKindOf,
  parentPathOf,
} from "../../../components/admin/finder/finderModel";
import {
  useFinderHistory,
  useFinderView,
} from "../../../components/admin/finder/useFinderHistory";
import { useAuthStore } from "../../../stores/auth";
import { decodeTextBlob } from "../../../utils/textEncoding";
import {
  type ChunkUploadStage,
  extensionFromName,
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
  const [pageSize] = useState(60);
  const [keyword, setKeyword] = useState("");
  const [status, setStatus] = useState<string | undefined>("active");
  const [profileId, setProfileId] = useState<number>();
  const [bucketId, setBucketId] = useState<number>();
  const [prefix, setPrefix] = useState<string>();
  const [view, setView] = useFinderView("uploads");
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
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
  const uploadTaskManager = useUploadTaskManager({
    bucketId,
    // Uploads land in the folder being viewed, the root included.
    prefix: prefix ?? "",
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
        exact_prefix: true,
      }),
  });

  const uploadMutation = useMutation({
    mutationFn: (file: File) =>
      uploadMaterial(file, {
        storage_profile_id: profileId,
        storage_bucket_id: bucketId,
        prefix: prefix ?? "",
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
    setSelectedKey(null);
  };

  const history = useFinderHistory(
    `${profileId ?? ""}|${bucketId ?? ""}|${prefix ?? ""}`,
    (location) => {
      const [profile, bucket, ...rest] = location.split("|");
      setProfileId(profile ? Number(profile) : undefined);
      setBucketId(bucket ? Number(bucket) : undefined);
      setPrefix(rest.join("|") || undefined);
      setPage(1);
      setSelectedKey(null);
    },
  );

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

  const managedMenuItems = (record: UploadRecord) => {
    const items: MenuProps["items"] = [
      isEditableTextUpload(record) && hasPermission("system:upload:update")
        ? {
            key: "edit-text",
            icon: <FileTextOutlined />,
            label: "编辑文本",
            onClick: () => openContentEditor(record),
          }
        : hasPermission("system:upload:download")
          ? {
              key: "open-preview",
              icon: <EyeOutlined />,
              label: "预览",
              onClick: () => openPreview(record),
            }
          : null,
      { type: "divider" },
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
    return items;
  };

  const externalMenuItems = (object: ExternalObjectRow) => {
    const record = externalObjectRecord(object);
    const items: MenuProps["items"] = [
      {
        key: "open-preview",
        icon: <EyeOutlined />,
        label: "预览",
        onClick: () => openExternalPreview(object),
      },
      { type: "divider" },
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
    return items;
  };

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

  const managedById = new Map(files.map((file) => [`m:${file.id}`, file]));
  const externalById = new Map(
    externalObjects.map((object) => [`x:${object.id}`, object]),
  );
  const finderItems: FinderItem[] = [
    ...folderPrefixes.map((folder) => ({
      key: `d:${folder.prefix}`,
      name: folder.name,
      kind: "folder" as const,
    })),
    ...files.map((record) => ({
      key: `m:${record.id}`,
      name: record.original_name,
      kind: finderKindOf({
        name: record.original_name,
        extension: record.extension,
        mimeType: record.mime_type,
      }),
      sizeBytes: record.size_bytes,
      modifiedAt: record.updated_at,
      muted: record.status !== "active",
      badge:
        record.status !== "active"
          ? { label: "已删除", tone: "red" as const }
          : record.visibility === "public"
            ? { label: "公开", tone: "green" as const }
            : undefined,
    })),
    ...externalObjects.map((object) => ({
      key: `x:${object.id}`,
      name: object.name,
      kind: finderKindOf({
        name: object.name,
        extension: object.extension,
        mimeType: object.mime_type,
      }),
      sizeBytes: object.size_bytes,
      modifiedAt: object.updated_at,
      badge: { label: "未入库", tone: "orange" as const },
    })),
  ];
  const managedTotal = uploadsQuery.data?.total ?? files.length;
  const canUpload = hasPermission("system:upload:create") && Boolean(bucketId);

  const openItem = (key: string) => {
    if (key.startsWith("d:")) {
      openPrefix(key.slice(2));
      return;
    }
    const record = managedById.get(key);
    if (record) {
      if (
        isEditableTextUpload(record) &&
        hasPermission("system:upload:update")
      ) {
        openContentEditor(record);
      } else {
        openPreview(record);
      }
      return;
    }
    const object = externalById.get(key);
    if (object) openExternalPreview(object);
  };

  const itemMenu = (key: string): MenuProps["items"] => {
    if (key.startsWith("d:")) {
      return [
        {
          key: "open",
          icon: <FolderOpenOutlined />,
          label: "打开",
          onClick: () => openPrefix(key.slice(2)),
        },
      ];
    }
    const record = managedById.get(key);
    if (record) return managedMenuItems(record);
    const object = externalById.get(key);
    return object ? externalMenuItems(object) : [];
  };

  const selectedName = selectedKey
    ? finderItems.find((item) => item.key === selectedKey)?.name
    : undefined;

  const createMenuItems: MenuProps["items"] = [
    {
      key: "upload",
      icon: <CloudUploadOutlined />,
      label: (
        <Upload {...uploadProps} disabled={isUploadBusy}>
          <span className="finder-menu-upload">上传文件</span>
        </Upload>
      ),
    },
    { type: "divider" },
    {
      key: "folder",
      icon: <FolderOutlined />,
      label: "新建文件夹",
      onClick: () => setFolderModalOpen(true),
    },
    {
      key: "file",
      icon: <FileAddOutlined />,
      label: "新建文本文件",
      onClick: () => {
        fileForm.setFieldsValue({
          name: "新建文件.txt",
          content: "",
          visibility: uploadVisibility,
        });
        setFileModalOpen(true);
      },
    },
  ];

  const check = (active: boolean) =>
    active ? <CheckOutlined /> : <span className="finder-menu-check" />;
  const moreMenuItems: MenuProps["items"] = [
    {
      key: "upload-mode",
      label: "上传方式",
      type: "group",
      children: [
        {
          key: "mode-chunk",
          icon: check(uploadMode === "chunk"),
          label: "分片上传（大文件可续传）",
          onClick: () => setUploadMode("chunk"),
        },
        {
          key: "mode-normal",
          icon: check(uploadMode === "normal"),
          label: "普通上传",
          onClick: () => setUploadMode("normal"),
        },
      ],
    },
    {
      key: "visibility",
      label: hasPublicAccess
        ? "新上传文件的可见性"
        : "新上传文件的可见性（未配置公开地址）",
      type: "group",
      children: [
        {
          key: "visibility-private",
          icon: check(uploadVisibility === "private"),
          label: "私有",
          onClick: () => setUploadVisibility("private"),
        },
        {
          key: "visibility-public",
          icon: check(uploadVisibility === "public"),
          label: "公开",
          onClick: () => setUploadVisibility("public"),
        },
      ],
    },
    { type: "divider" },
    {
      key: "import-all",
      icon: <CloudSyncOutlined />,
      label: `全部入库（${externalObjects.length}）`,
      disabled:
        !canUpload ||
        externalObjects.length === 0 ||
        importAllMutation.isPending,
      onClick: () => importAllMutation.mutate(),
    },
    {
      key: "tasks",
      icon: <UnorderedListOutlined />,
      label: "上传任务",
      onClick: () => navigate("/admin/system/upload-tasks"),
    },
  ];

  return (
    <div className="finder-page-shell">
      <FinderShell
        label="素材库"
        rootLabel={selectedBucket?.name ?? "素材库"}
        crumbs={currentPathSegments.map((segment) => ({
          key: segment.prefix,
          label: segment.name,
        }))}
        onCrumbClick={(key) => openPrefix(key ?? undefined)}
        nav={{
          canBack: history.canBack,
          canForward: history.canForward,
          canUp: Boolean(prefix),
          onBack: history.back,
          onForward: history.forward,
          onUp: () => {
            const parent = parentPathOf(prefix);
            openPrefix(parent ? `${parent}/` : undefined);
          },
        }}
        view={view}
        onViewChange={setView}
        search={{
          value: keyword,
          placeholder: "搜索素材",
          onChange: (value) => {
            setPage(1);
            setKeyword(value);
          },
        }}
        actions={
          <>
            {canUpload ? (
              <Dropdown
                menu={{ items: createMenuItems }}
                trigger={["click"]}
                placement="bottomRight"
              >
                <Button
                  type="primary"
                  size="small"
                  icon={<PlusOutlined />}
                  aria-label="新建"
                >
                  <span className="finder-action-label">新建</span>
                </Button>
              </Dropdown>
            ) : null}
            <Tooltip title="刷新">
              <Button
                type="text"
                size="small"
                icon={
                  <ReloadOutlined
                    spin={uploadsQuery.isFetching || browserQuery.isFetching}
                  />
                }
                aria-label="刷新"
                onClick={() => {
                  void uploadsQuery.refetch();
                  void browserQuery.refetch();
                }}
              />
            </Tooltip>
            <Dropdown
              menu={{ items: moreMenuItems }}
              trigger={["click"]}
              placement="bottomRight"
            >
              <Button
                type="text"
                size="small"
                icon={<EllipsisOutlined />}
                aria-label="更多"
              />
            </Dropdown>
          </>
        }
        sidebar={
          <>
            <FinderSidebarSection title="存储">
              {profiles.map((profile) => (
                <FinderSidebarItem
                  key={profile.id}
                  icon={<CloudServerOutlined />}
                  label={profile.name}
                  hint={profile.provider}
                  active={profile.id === profileId}
                  onClick={() => {
                    setProfileId(profile.id);
                    setBucketId(undefined);
                    setPrefix(undefined);
                    setPage(1);
                  }}
                />
              ))}
            </FinderSidebarSection>
            <FinderSidebarSection title="桶">
              {buckets.length === 0 && !bucketsQuery.isLoading ? (
                <Typography.Text
                  type="secondary"
                  className="finder-sidebar-hint"
                >
                  当前存储没有可用的桶
                </Typography.Text>
              ) : null}
              {buckets.map((bucket) => (
                <FinderSidebarItem
                  key={bucket.id}
                  icon={<DatabaseOutlined />}
                  label={bucket.name}
                  hint={bucket.bucket}
                  active={bucket.id === bucketId}
                  onClick={() => {
                    setBucketId(bucket.id);
                    setPrefix(undefined);
                    setPage(1);
                  }}
                />
              ))}
            </FinderSidebarSection>
            {bucketId ? (
              <FinderSidebarSection title="文件夹">
                <FinderTree
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
              </FinderSidebarSection>
            ) : null}
            <FinderSidebarSection title="显示">
              <FinderSidebarItem
                icon={<span className="finder-filter-dot is-green" />}
                label="正常素材"
                active={status === "active"}
                onClick={() => {
                  setPage(1);
                  setStatus("active");
                }}
              />
              <FinderSidebarItem
                icon={<span className="finder-filter-dot is-red" />}
                label="已删除"
                active={status === "deleted"}
                onClick={() => {
                  setPage(1);
                  setStatus("deleted");
                }}
              />
              <FinderSidebarItem
                icon={<span className="finder-filter-dot is-gray" />}
                label="全部"
                active={!status}
                onClick={() => {
                  setPage(1);
                  setStatus(undefined);
                }}
              />
            </FinderSidebarSection>
          </>
        }
        status={
          selectedName
            ? `已选择“${selectedName}”`
            : `${folderPrefixes.length} 个文件夹，${managedTotal} 个素材${
                externalObjects.length > 0
                  ? `，${externalObjects.length} 个未入库`
                  : ""
              }`
        }
        statusAside={
          managedTotal > pageSize ? (
            <Pagination
              simple
              size="small"
              current={page}
              pageSize={pageSize}
              total={managedTotal}
              onChange={(nextPage) => setPage(nextPage)}
            />
          ) : (
            [selectedProfile?.name, selectedBucket?.bucket]
              .filter(Boolean)
              .join(" · ")
          )
        }
        overlay={
          uploadingName ? (
            <FinderProgress
              name={uploadingName}
              stage={uploadStageLabel}
              percent={uploadPercent}
            />
          ) : null
        }
      >
        <div
          className={`finder-drop${uploadDragActive ? " is-dragging" : ""}`}
          onDragEnter={handleUploadDragEnter}
          onDragOver={handleUploadDragOver}
          onDragLeave={handleUploadDragLeave}
          onDrop={handleUploadDrop}
        >
          <Upload.Dragger
            {...uploadProps}
            openFileDialogOnClick={false}
            disabled={!canUpload || isUploadBusy}
          >
            <FinderItems
              items={finderItems}
              view={view}
              loading={uploadsQuery.isLoading || browserQuery.isLoading}
              selectedKey={selectedKey}
              onSelect={setSelectedKey}
              onOpen={openItem}
              menuItems={itemMenu}
              empty={
                <Empty
                  image={Empty.PRESENTED_IMAGE_SIMPLE}
                  description={
                    !bucketId
                      ? "请选择左侧的存储和桶"
                      : keyword
                        ? "没有匹配的素材"
                        : canUpload
                          ? "空文件夹，可拖拽文件到这里上传"
                          : "空文件夹"
                  }
                />
              }
            />
          </Upload.Dragger>
        </div>
      </FinderShell>
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
    </div>
  );
}
