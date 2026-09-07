import {
  AppstoreOutlined,
  CloudDownloadOutlined,
  CloudUploadOutlined,
  DeleteOutlined,
  EditOutlined,
  ExclamationCircleOutlined,
  EyeOutlined,
  FileAddOutlined,
  FileTextOutlined,
  FolderOpenOutlined,
  FolderOutlined,
  LinkOutlined,
  MoreOutlined,
  PlusOutlined,
  ReloadOutlined,
  SaveOutlined,
  TableOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Breadcrumb,
  Button,
  Card,
  Drawer,
  Dropdown,
  Empty,
  Form,
  Grid,
  Input,
  Modal,
  Popconfirm,
  Popover,
  Segmented,
  Select,
  Space,
  Spin,
  Switch,
  Tag,
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
  type KeyboardEvent as ReactKeyboardEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  type FileRootRecord,
  type ManagedFileRecord,
  createFileFolder,
  createManagedFile,
  deleteManagedFile,
  downloadManagedFile,
  fetchFileBrowser,
  fetchFileRoots,
  fetchManagedFileContent,
  previewManagedFile,
  renameManagedFile,
  updateManagedFileAccess,
  updateManagedFileContent,
  uploadManagedFile,
} from "../../../api/admin/files";
import { CrudPage } from "../../../components/admin/CrudPage";
import { DataTable } from "../../../components/admin/DataTable";
import { FinderWorkspace } from "../../../components/admin/FinderWorkspace";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { StandardTablePagination } from "../../../components/admin/StandardTablePagination";
import { TableScrollArea } from "../../../components/admin/TableScrollArea";
import { useAuthStore } from "../../../stores/auth";
import { decodeTextBlob } from "../../../utils/textEncoding";
import { fileIcon, fileKind, formatBytes } from "../uploads/uploadTaskManager";

type FolderFormValues = {
  name: string;
};

type FileFormValues = {
  name: string;
  content?: string;
};

type RenameFormValues = {
  name: string;
};

type AccessFormValues = {
  visibility: string;
  password_enabled?: boolean;
  password?: string;
  clear_password?: boolean;
};

type FileViewMode = "table" | "card";

const FILE_TABLE_SCROLL_X = 1680;
const MAX_EDITABLE_FILE_BYTES = 1024 * 1024;
const EDITABLE_TEXT_EXTENSIONS = new Set([
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
const EDITABLE_TEXT_FILENAMES = new Set([
  "dockerfile",
  "license",
  "makefile",
  "procfile",
  "readme",
]);
const EDITABLE_TEXT_MIME_TYPES = new Set([
  "application/javascript",
  "application/json",
  "application/toml",
  "application/xml",
  "application/x-yaml",
  "application/yaml",
]);

function joinRelativePath(prefix: string, name: string) {
  const cleanName = name.trim().replace(/^\/+|\/+$/g, "");
  if (!cleanName) {
    return prefix;
  }
  return prefix ? `${prefix}/${cleanName}` : cleanName;
}

function parentPath(path: string) {
  const index = path.lastIndexOf("/");
  return index > -1 ? path.slice(0, index) : "";
}

function pathSegments(path: string) {
  const segments = path.split("/").filter(Boolean);
  return segments.map((name, index) => ({
    name,
    path: segments.slice(0, index + 1).join("/"),
  }));
}

function folderTreeNode(folder: ManagedFileRecord): DataNode {
  return {
    key: folder.path,
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

function isTextFile(record: ManagedFileRecord) {
  const mimeType = record.mime_type?.toLowerCase() ?? "";
  const extension = record.extension?.toLowerCase() ?? "";
  return (
    mimeType.startsWith("text/") ||
    ["json", "md", "markdown", "yaml", "yml", "csv", "log"].includes(extension)
  );
}

function isEditableTextFile(record: ManagedFileRecord) {
  if (record.is_dir) {
    return false;
  }
  const mimeType = record.mime_type?.toLowerCase() ?? "";
  const extension = record.extension?.toLowerCase() ?? "";
  return (
    mimeType.startsWith("text/") ||
    EDITABLE_TEXT_MIME_TYPES.has(mimeType) ||
    EDITABLE_TEXT_EXTENSIONS.has(extension) ||
    EDITABLE_TEXT_FILENAMES.has(record.name.toLowerCase())
  );
}

export function FilesPage() {
  const [rootKey, setRootKey] = useState<string>();
  const [currentPath, setCurrentPath] = useState("");
  const [keyword, setKeyword] = useState("");
  const [tablePage, setTablePage] = useState(1);
  const [tablePageSize, setTablePageSize] = useState(10);
  const [folderModalOpen, setFolderModalOpen] = useState(false);
  const [fileModalOpen, setFileModalOpen] = useState(false);
  const [renaming, setRenaming] = useState<ManagedFileRecord | null>(null);
  const [accessEditing, setAccessEditing] = useState<ManagedFileRecord | null>(
    null,
  );
  const [previewing, setPreviewing] = useState<ManagedFileRecord | null>(null);
  const [previewObjectUrl, setPreviewObjectUrl] = useState<string>();
  const [previewText, setPreviewText] = useState<string>();
  const [previewError, setPreviewError] = useState<string>();
  const [previewLoading, setPreviewLoading] = useState(false);
  const [contentEditing, setContentEditing] =
    useState<ManagedFileRecord | null>(null);
  const [editorContent, setEditorContent] = useState("");
  const [editorOriginalContent, setEditorOriginalContent] = useState("");
  const [editorUpdatedAt, setEditorUpdatedAt] = useState<string>();
  const [editorLoading, setEditorLoading] = useState(false);
  const [uploadPercent, setUploadPercent] = useState(0);
  const [uploadingName, setUploadingName] = useState<string>();
  const [viewMode, setViewMode] = useState<FileViewMode>("card");
  const [mobileActionRecord, setMobileActionRecord] =
    useState<ManagedFileRecord | null>(null);
  const [treeData, setTreeData] = useState<DataNode[]>([]);
  const [expandedTreeKeys, setExpandedTreeKeys] = useState<Key[]>([""]);
  const [folderForm] = Form.useForm<FolderFormValues>();
  const [fileForm] = Form.useForm<FileFormValues>();
  const [renameForm] = Form.useForm<RenameFormValues>();
  const [accessForm] = Form.useForm<AccessFormValues>();
  const editorRequestId = useRef(0);
  const accessPasswordEnabled = Form.useWatch("password_enabled", accessForm);
  const accessVisibility = Form.useWatch("visibility", accessForm);
  const queryClient = useQueryClient();
  const hasPermission = useAuthStore((state) => state.hasPermission);
  const screens = Grid.useBreakpoint();
  const effectiveViewMode: FileViewMode =
    screens.md === false ? "card" : viewMode;
  const editorByteSize = useMemo(
    () => new TextEncoder().encode(editorContent).length,
    [editorContent],
  );
  const editorDirty = editorContent !== editorOriginalContent;
  const editorOverLimit = editorByteSize > MAX_EDITABLE_FILE_BYTES;

  useEffect(() => {
    return () => {
      if (previewObjectUrl) {
        URL.revokeObjectURL(previewObjectUrl);
      }
    };
  }, [previewObjectUrl]);

  const rootsQuery = useQuery({
    queryKey: ["admin-file-roots"],
    queryFn: fetchFileRoots,
  });
  const roots = rootsQuery.data ?? [];

  useEffect(() => {
    if (!rootKey && roots.length > 0) {
      setRootKey(roots[0].key);
    }
  }, [rootKey, roots]);

  const selectedRoot = roots.find((root) => root.key === rootKey);

  const browserQuery = useQuery({
    queryKey: ["admin-file-browser", rootKey, currentPath],
    enabled: Boolean(rootKey),
    queryFn: () =>
      fetchFileBrowser({
        root_key: rootKey ?? "",
        path: currentPath || undefined,
      }),
  });

  useEffect(() => {
    if (!rootKey) {
      setTreeData([]);
      setExpandedTreeKeys([""]);
      return;
    }
    let cancelled = false;
    fetchFileBrowser({ root_key: rootKey })
      .then((browser) => {
        if (cancelled) {
          return;
        }
        setTreeData([
          {
            key: "",
            title: "根目录",
            icon: <FolderOpenOutlined />,
            children: browser.directories.map(folderTreeNode),
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
  }, [rootKey]);

  const loadTreeChildren = async (targetPath: string) => {
    if (!rootKey) {
      return;
    }
    const browser = await fetchFileBrowser({
      root_key: rootKey,
      path: targetPath || undefined,
    });
    setTreeData((current) =>
      applyTreeChildren(
        current,
        targetPath,
        browser.directories.map(folderTreeNode),
      ),
    );
  };

  const refreshFiles = () => {
    queryClient.invalidateQueries({ queryKey: ["admin-file-browser"] });
  };

  const createFolderMutation = useMutation({
    mutationFn: (values: FolderFormValues) => {
      if (!rootKey) {
        throw new Error("请先选择文件根目录");
      }
      const path = joinRelativePath(currentPath, values.name);
      if (!path) {
        throw new Error("请输入文件夹名称");
      }
      return createFileFolder({ root_key: rootKey, path });
    },
    onSuccess: () => {
      message.success("文件夹已创建");
      setFolderModalOpen(false);
      folderForm.resetFields();
      refreshFiles();
      loadTreeChildren(currentPath);
    },
    onError: (error) => {
      message.error(error instanceof Error ? error.message : "创建文件夹失败");
    },
  });

  const createFileMutation = useMutation({
    mutationFn: (values: FileFormValues) => {
      if (!rootKey) {
        throw new Error("请先选择文件根目录");
      }
      const path = joinRelativePath(currentPath, values.name);
      if (!path) {
        throw new Error("请输入文件名称");
      }
      return createManagedFile({
        root_key: rootKey,
        path,
        content: values.content ?? "",
      });
    },
    onSuccess: (record, values) => {
      const content = values.content ?? "";
      message.success("文件已创建");
      setFileModalOpen(false);
      fileForm.resetFields();
      refreshFiles();
      if (isEditableTextFile(record)) {
        editorRequestId.current += 1;
        setContentEditing(record);
        setEditorContent(content);
        setEditorOriginalContent(content);
        setEditorUpdatedAt(record.updated_at ?? undefined);
        setEditorLoading(false);
      }
    },
    onError: (error) => {
      message.error(error instanceof Error ? error.message : "创建文件失败");
    },
  });

  const renameMutation = useMutation({
    mutationFn: (values: RenameFormValues) => {
      if (!rootKey || !renaming) {
        throw new Error("请选择文件");
      }
      return renameManagedFile({
        root_key: rootKey,
        path: renaming.path,
        name: values.name,
      });
    },
    onSuccess: () => {
      message.success("文件已重命名");
      setRenaming(null);
      renameForm.resetFields();
      refreshFiles();
      loadTreeChildren(parentPath(renaming?.path ?? currentPath));
      if (previewing?.path === renaming?.path) {
        closePreview();
      }
    },
    onError: (error) => {
      message.error(error instanceof Error ? error.message : "重命名失败");
    },
  });

  const accessMutation = useMutation({
    mutationFn: (values: AccessFormValues) => {
      if (!rootKey || !accessEditing) {
        throw new Error("请选择文件");
      }
      return updateManagedFileAccess({
        root_key: rootKey,
        path: accessEditing.path,
        visibility: values.visibility,
        password:
          values.visibility === "public" && values.password_enabled
            ? values.password?.trim() || null
            : null,
        clear_password:
          values.visibility === "private" ||
          (values.visibility === "public" &&
            !values.password_enabled &&
            accessEditing.requires_password) ||
          values.clear_password,
      });
    },
    onSuccess: (record) => {
      message.success("访问设置已保存");
      setAccessEditing(null);
      accessForm.resetFields();
      refreshFiles();
      if (previewing?.path === record.path) {
        setPreviewing(record);
      }
    },
    onError: (error) => {
      message.error(
        error instanceof Error ? error.message : "访问设置保存失败",
      );
    },
  });

  const updateContentMutation = useMutation({
    mutationFn: (variables: {
      rootKey: string;
      record: ManagedFileRecord;
      content: string;
    }) =>
      updateManagedFileContent({
        root_key: variables.rootKey,
        path: variables.record.path,
        content: variables.content,
      }),
    onSuccess: (record, variables) => {
      message.success("文件内容已保存");
      setContentEditing(record);
      setEditorOriginalContent(variables.content);
      setEditorUpdatedAt(record.updated_at ?? undefined);
      refreshFiles();
      if (previewing?.path === record.path) {
        closePreview();
      }
    },
    onError: (error) => {
      message.error(
        error instanceof Error ? error.message : "文件内容保存失败",
      );
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (record: ManagedFileRecord) => {
      if (!rootKey) {
        throw new Error("请先选择文件根目录");
      }
      return deleteManagedFile({ root_key: rootKey, path: record.path });
    },
    onSuccess: (_, record) => {
      message.success(record.is_dir ? "文件夹已删除" : "文件已删除");
      refreshFiles();
      loadTreeChildren(parentPath(record.path));
      if (previewing?.path === record.path) {
        closePreview();
      }
    },
    onError: (error) => {
      message.error(error instanceof Error ? error.message : "删除失败");
    },
  });

  const downloadMutation = useMutation({
    mutationFn: (record: ManagedFileRecord) => {
      if (!rootKey) {
        throw new Error("请先选择文件根目录");
      }
      return downloadManagedFile({ root_key: rootKey, path: record.path });
    },
    onSuccess: (blob, record) => {
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = record.name;
      link.click();
      URL.revokeObjectURL(url);
    },
    onError: (error) => {
      message.error(error instanceof Error ? error.message : "下载失败");
    },
  });

  const uploadMutation = useMutation({
    mutationFn: (file: File) => {
      if (!rootKey) {
        throw new Error("请先选择文件根目录");
      }
      return uploadManagedFile(file, {
        root_key: rootKey,
        path: currentPath || undefined,
        onUploadProgress: setUploadPercent,
      });
    },
    onSuccess: () => {
      message.success("文件已上传");
      setUploadingName(undefined);
      setUploadPercent(100);
      refreshFiles();
    },
    onError: (error) => {
      setUploadingName(undefined);
      setUploadPercent(0);
      message.error(error instanceof Error ? error.message : "上传失败");
    },
  });

  const uploadProps: UploadProps = {
    multiple: false,
    showUploadList: false,
    customRequest: ({ file, onSuccess, onError }) => {
      const uploadFile = file as File;
      setUploadingName(uploadFile.name);
      setUploadPercent(0);
      uploadMutation.mutate(uploadFile, {
        onSuccess: () => onSuccess?.("ok"),
        onError: (error) => onError?.(error as Error),
      });
    },
  };

  const closePreview = () => {
    if (previewObjectUrl) {
      URL.revokeObjectURL(previewObjectUrl);
    }
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

  const openContentEditor = async (record: ManagedFileRecord) => {
    if (!rootKey || !isEditableTextFile(record)) {
      return;
    }
    const requestId = editorRequestId.current + 1;
    editorRequestId.current = requestId;
    setMobileActionRecord(null);
    setContentEditing(record);
    setEditorContent("");
    setEditorOriginalContent("");
    setEditorUpdatedAt(record.updated_at ?? undefined);
    setEditorLoading(true);
    try {
      const data = await fetchManagedFileContent({
        root_key: rootKey,
        path: record.path,
      });
      if (editorRequestId.current !== requestId) {
        return;
      }
      setEditorContent(data.content);
      setEditorOriginalContent(data.content);
      setEditorUpdatedAt(data.updated_at ?? undefined);
    } catch (error) {
      if (editorRequestId.current !== requestId) {
        return;
      }
      setContentEditing(null);
      const reason =
        error instanceof Error ? error.message : "文件内容加载失败";
      message.error(reason);
    } finally {
      if (editorRequestId.current === requestId) {
        setEditorLoading(false);
      }
    }
  };

  const saveContent = () => {
    if (!rootKey || !contentEditing) {
      message.error("请选择要编辑的文件");
      return;
    }
    if (editorOverLimit) {
      message.error("文件超过 1 MiB，无法保存");
      return;
    }
    updateContentMutation.mutate({
      rootKey,
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

  const openPreview = async (record: ManagedFileRecord) => {
    if (!rootKey || record.is_dir) {
      return;
    }
    setPreviewing(record);
    setPreviewObjectUrl(undefined);
    setPreviewText(undefined);
    setPreviewError(undefined);
    setPreviewLoading(true);
    try {
      const blob = await previewManagedFile({
        root_key: rootKey,
        path: record.path,
      });
      if (isTextFile(record)) {
        setPreviewText(await decodeTextBlob(blob));
      } else {
        setPreviewObjectUrl(URL.createObjectURL(blob));
      }
    } catch (error) {
      const reason =
        error instanceof Error ? error.message : "文件预览加载失败";
      setPreviewError(reason);
      message.error(reason);
    } finally {
      setPreviewLoading(false);
    }
  };

  const openRename = (record: ManagedFileRecord) => {
    setRenaming(record);
    renameForm.setFieldsValue({ name: record.name });
  };

  const openAccessEditor = (record: ManagedFileRecord) => {
    setAccessEditing(record);
    accessForm.setFieldsValue({
      visibility: record.visibility ?? "private",
      password_enabled: record.requires_password,
      password: undefined,
      clear_password: false,
    });
  };

  const copyUrl = async (record: ManagedFileRecord | FileRootRecord) => {
    const url =
      "path" in record
        ? record.visibility === "public" && record.public_url
          ? record.public_url
          : `/api/admin/files/download?root_key=${encodeURIComponent(rootKey ?? "")}&path=${encodeURIComponent(record.path)}`
        : record.url_path;
    try {
      await navigator.clipboard.writeText(
        new URL(url, window.location.origin).toString(),
      );
      message.success("URL 已复制");
    } catch {
      message.error("复制失败，请检查浏览器权限");
    }
  };

  const openPath = (path: string) => {
    setCurrentPath(path);
    setTablePage(1);
  };

  const renderActions = (record: ManagedFileRecord) => (
    <Space wrap size={6} className="material-table-actions">
      {record.is_dir ? (
        <Button
          size="small"
          icon={<FolderOpenOutlined />}
          onClick={() => openPath(record.path)}
        >
          打开
        </Button>
      ) : isEditableTextFile(record) ? (
        <PermissionButton
          size="small"
          icon={<FileTextOutlined />}
          permission="system:file:update"
          onClick={() => openContentEditor(record)}
        >
          编辑文本
        </PermissionButton>
      ) : (
        <PermissionButton
          size="small"
          icon={<EyeOutlined />}
          permission="system:file:download"
          onClick={() => openPreview(record)}
        >
          预览
        </PermissionButton>
      )}
      {!record.is_dir && isEditableTextFile(record) ? (
        <PermissionButton
          size="small"
          icon={<EyeOutlined />}
          permission="system:file:download"
          onClick={() => openPreview(record)}
        >
          预览
        </PermissionButton>
      ) : null}
      <Button
        size="small"
        icon={<LinkOutlined />}
        onClick={() => copyUrl(record)}
      >
        复制URL
      </Button>
      {!record.is_dir ? (
        <PermissionButton
          size="small"
          icon={<CloudDownloadOutlined />}
          permission="system:file:download"
          loading={downloadMutation.isPending}
          onClick={() => downloadMutation.mutate(record)}
        >
          下载
        </PermissionButton>
      ) : null}
      {!record.is_dir ? (
        <PermissionButton
          size="small"
          icon={<LinkOutlined />}
          permission="system:file:update"
          onClick={() => openAccessEditor(record)}
        >
          访问设置
        </PermissionButton>
      ) : null}
      <PermissionButton
        size="small"
        icon={<EditOutlined />}
        permission="system:file:update"
        onClick={() => openRename(record)}
      >
        重命名
      </PermissionButton>
      <Popconfirm
        title={record.is_dir ? "确认删除空文件夹？" : "确认删除文件？"}
        onConfirm={() => deleteMutation.mutate(record)}
      >
        <PermissionButton
          size="small"
          danger
          icon={<DeleteOutlined />}
          permission="system:file:delete"
        >
          删除
        </PermissionButton>
      </Popconfirm>
    </Space>
  );

  const renderTableActions = (record: ManagedFileRecord) => {
    const items: MenuProps["items"] = [
      !record.is_dir &&
      isEditableTextFile(record) &&
      hasPermission("system:file:download")
        ? {
            key: "preview",
            icon: <EyeOutlined />,
            label: "预览",
            onClick: () => openPreview(record),
          }
        : null,
      {
        key: "copy-url",
        icon: <LinkOutlined />,
        label: "复制 URL",
        onClick: () => copyUrl(record),
      },
      !record.is_dir && hasPermission("system:file:download")
        ? {
            key: "download",
            icon: <CloudDownloadOutlined />,
            label: "下载",
            onClick: () => downloadMutation.mutate(record),
          }
        : null,
      !record.is_dir && hasPermission("system:file:update")
        ? {
            key: "access",
            icon: <LinkOutlined />,
            label: "访问设置",
            onClick: () => openAccessEditor(record),
          }
        : null,
      hasPermission("system:file:update")
        ? {
            key: "rename",
            icon: <EditOutlined />,
            label: "重命名",
            onClick: () => openRename(record),
          }
        : null,
      hasPermission("system:file:delete") ? { type: "divider" } : null,
      hasPermission("system:file:delete")
        ? {
            key: "delete",
            icon: <DeleteOutlined />,
            label: "删除",
            danger: true,
            onClick: () =>
              Modal.confirm({
                title: record.is_dir ? "确认删除空文件夹？" : "确认删除文件？",
                okText: "删除",
                okButtonProps: { danger: true },
                cancelText: "取消",
                onOk: () => deleteMutation.mutateAsync(record),
              }),
          }
        : null,
    ];

    return (
      <Space size={4} className="material-table-compact-actions">
        {record.is_dir ? (
          <Button
            size="small"
            icon={<FolderOpenOutlined />}
            onClick={() => openPath(record.path)}
          >
            打开
          </Button>
        ) : isEditableTextFile(record) &&
          hasPermission("system:file:update") ? (
          <PermissionButton
            size="small"
            icon={<FileTextOutlined />}
            permission="system:file:update"
            onClick={() => openContentEditor(record)}
          >
            编辑文本
          </PermissionButton>
        ) : (
          <PermissionButton
            size="small"
            icon={<EyeOutlined />}
            permission="system:file:download"
            onClick={() => openPreview(record)}
          >
            预览
          </PermissionButton>
        )}
        <Dropdown menu={{ items }} trigger={["click"]} placement="bottomRight">
          <Button
            size="small"
            icon={<MoreOutlined />}
            aria-label="更多文件操作"
          />
        </Dropdown>
      </Space>
    );
  };

  const renderVisibilityTag = (record: ManagedFileRecord) => {
    if (record.is_dir) {
      return <Tag>目录</Tag>;
    }
    if (record.visibility === "public") {
      return record.requires_password ? (
        <Tag color="orange">公开-需密码</Tag>
      ) : (
        <Tag color="green">公开</Tag>
      );
    }
    return <Tag>私有</Tag>;
  };

  const renderFileCard = (record: ManagedFileRecord) => {
    const opensEditor =
      isEditableTextFile(record) && hasPermission("system:file:update");
    const openRecord = () => {
      if (record.is_dir) {
        openPath(record.path);
      } else if (opensEditor) {
        openContentEditor(record);
      } else {
        openPreview(record);
      }
    };

    return (
      <Card
        key={record.path}
        className={[
          "material-card",
          record.is_dir ? "material-folder-card" : undefined,
        ]
          .filter(Boolean)
          .join(" ")}
        hoverable
        onDoubleClick={openRecord}
      >
        <button
          className="material-card-preview"
          type="button"
          onClick={openRecord}
        >
          {record.is_dir ? <FolderOutlined /> : fileIcon(record)}
        </button>
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
            aria-label={`更多操作：${record.name}`}
            onClick={(event) => event.stopPropagation()}
            onDoubleClick={(event) => event.stopPropagation()}
          />
        </Popover>
        <Space
          direction="vertical"
          size={8}
          className="full-width material-card-details"
        >
          <Typography.Text strong ellipsis title={record.name}>
            {record.name}
          </Typography.Text>
          <div className="material-card-meta">
            <span>
              {record.is_dir ? "文件夹" : (record.mime_type ?? "文件")}
            </span>
            <span>{record.is_dir ? "-" : formatBytes(record.size_bytes)}</span>
          </div>
          <div className="material-card-visibility-row">
            {renderVisibilityTag(record)}
          </div>
          <Typography.Text
            type="secondary"
            ellipsis
            title={record.path}
            className="material-card-path"
          >
            {record.path || "根目录"}
          </Typography.Text>
          <div className="material-card-actions material-card-actions-desktop">
            {renderActions(record)}
          </div>
          <div className="material-card-actions-mobile">
            <Button
              type="primary"
              size="small"
              aria-label={
                record.is_dir ? "打开" : opensEditor ? "编辑文本" : "预览"
              }
              icon={
                record.is_dir ? (
                  <FolderOpenOutlined />
                ) : opensEditor ? (
                  <FileTextOutlined />
                ) : (
                  <EyeOutlined />
                )
              }
              onClick={openRecord}
            >
              {record.is_dir ? "打开" : opensEditor ? "编辑文本" : "预览"}
            </Button>
            <Button
              size="small"
              icon={<MoreOutlined />}
              aria-label={`更多操作：${record.name}`}
              onClick={() => setMobileActionRecord(record)}
            >
              更多
            </Button>
          </div>
        </Space>
      </Card>
    );
  };

  const columns: ColumnsType<ManagedFileRecord> = [
    {
      title: "名称",
      dataIndex: "name",
      width: 260,
      ellipsis: true,
      render: (value, record) => (
        <Space size={8}>
          <span className="material-table-file-icon">
            {record.is_dir ? <FolderOutlined /> : fileIcon(record)}
          </span>
          <Typography.Text ellipsis className="material-table-file-name">
            {value}
          </Typography.Text>
        </Space>
      ),
    },
    {
      title: "类型",
      key: "type",
      width: 110,
      render: (_, record) =>
        record.is_dir ? <Tag color="blue">文件夹</Tag> : <Tag>文件</Tag>,
    },
    { title: "路径", dataIndex: "path", width: 260, ellipsis: true },
    {
      title: "访问",
      key: "visibility",
      width: 130,
      render: (_, record) => renderVisibilityTag(record),
    },
    {
      title: "URL",
      dataIndex: "url",
      width: 260,
      ellipsis: true,
      render: (_, record) =>
        record.visibility === "public" && record.public_url
          ? record.public_url
          : record.url,
    },
    {
      title: "MIME",
      dataIndex: "mime_type",
      width: 180,
      ellipsis: true,
      render: (value) => value ?? "-",
    },
    {
      title: "大小",
      dataIndex: "size_bytes",
      width: 110,
      render: (_, record) =>
        record.is_dir ? "-" : formatBytes(record.size_bytes),
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
      render: (_, record) => renderTableActions(record),
    },
  ];

  const rows = useMemo(() => {
    const directories = browserQuery.data?.directories ?? [];
    const files = browserQuery.data?.files ?? [];
    const allRows = [...directories, ...files];
    if (!keyword) {
      return allRows;
    }
    const normalizedKeyword = keyword.toLowerCase();
    return allRows.filter(
      (row) =>
        row.name.toLowerCase().includes(normalizedKeyword) ||
        row.path.toLowerCase().includes(normalizedKeyword),
    );
  }, [browserQuery.data, keyword]);

  useEffect(() => {
    const lastPage = Math.max(1, Math.ceil(rows.length / tablePageSize));
    if (tablePage > lastPage) {
      setTablePage(lastPage);
    }
  }, [rows.length, tablePage, tablePageSize]);

  const pagedRows = useMemo(() => {
    const start = (tablePage - 1) * tablePageSize;
    return rows.slice(start, start + tablePageSize);
  }, [rows, tablePage, tablePageSize]);

  const currentPathSegments = pathSegments(currentPath);

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
          alt={previewing.name}
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
          title={previewing.name}
        />
      );
    }
    return (
      <div className="material-preview-empty">
        <Typography.Text type="secondary">
          该文件类型暂不支持在线预览，请下载后查看。
        </Typography.Text>
        <PermissionButton
          icon={<CloudDownloadOutlined />}
          permission="system:file:download"
          onClick={() => downloadMutation.mutate(previewing)}
        >
          下载
        </PermissionButton>
      </div>
    );
  };

  return (
    <CrudPage
      className="finder-page finder-files-page"
      title="文件管理"
      subtitle="管理系统设置中声明的本地 URL 映射目录"
      breadcrumb={["系统管理", "文件管理"]}
      icon={<FolderOpenOutlined />}
      toolbar={
        <Space wrap className="material-filter-bar">
          <Select
            placeholder="文件根目录"
            value={rootKey}
            loading={rootsQuery.isLoading}
            onChange={(value) => {
              setRootKey(value);
              setCurrentPath("");
              setTablePage(1);
            }}
            options={roots.map((root) => ({
              value: root.key,
              label: `${root.name} / ${root.url_path}`,
            }))}
            className="admin-filter-select"
          />
          <Input.Search
            allowClear
            placeholder="搜索名称、路径"
            onSearch={(value) => {
              setKeyword(value);
              setTablePage(1);
            }}
            className="admin-search-input"
          />
          <Button icon={<ReloadOutlined />} onClick={refreshFiles}>
            刷新
          </Button>
        </Space>
      }
    >
      <FinderWorkspace
        title="文件管理"
        icon={<FolderOpenOutlined />}
        meta={`${selectedRoot?.name ?? "未选择根目录"} · ${currentPath || "根目录"}`}
        status={`${rows.filter((record) => record.is_dir).length} 个文件夹 · ${rows.filter((record) => !record.is_dir).length} 个文件`}
        statusAside={selectedRoot?.url_path ?? "未配置 URL"}
      >
        <div className="resource-library-layout">
          <aside className="resource-tree-panel">
            <Space direction="vertical" size="small" className="full-width">
              <div className="finder-sidebar-label">位置</div>
              {selectedRoot ? (
                <div className="finder-location-summary">
                  <Typography.Text strong>{selectedRoot.name}</Typography.Text>
                  <Typography.Text type="secondary" copyable>
                    {selectedRoot.url_path}
                  </Typography.Text>
                  <Typography.Text type="secondary" ellipsis>
                    {selectedRoot.local_root}
                  </Typography.Text>
                </div>
              ) : null}
              <div className="finder-sidebar-label finder-sidebar-label-secondary">
                目录
              </div>
              <Tree
                showIcon
                selectedKeys={[currentPath]}
                expandedKeys={expandedTreeKeys}
                treeData={treeData}
                loadData={(node) => loadTreeChildren(String(node.key))}
                onExpand={(keys) => setExpandedTreeKeys(keys)}
                onSelect={(keys) => openPath(String(keys[0] ?? ""))}
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
                  当前根目录：{selectedRoot?.name ?? "未选择"}
                </Typography.Text>
                <Breadcrumb
                  items={[
                    {
                      title: (
                        <Button
                          type="link"
                          size="small"
                          onClick={() => openPath("")}
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
                          onClick={() => openPath(segment.path)}
                        >
                          {segment.name}
                        </Button>
                      ),
                    })),
                  ]}
                />
              </Space>
              <Space wrap className="material-upload-inline-actions">
                <PermissionButton
                  icon={<PlusOutlined />}
                  permission="system:file:create"
                  disabled={!rootKey}
                  onClick={() => setFolderModalOpen(true)}
                >
                  新建文件夹
                </PermissionButton>
                <PermissionButton
                  icon={<FileAddOutlined />}
                  permission="system:file:create"
                  disabled={!rootKey}
                  onClick={() => {
                    fileForm.setFieldsValue({
                      name: "新建文件.txt",
                      content: "",
                    });
                    setFileModalOpen(true);
                  }}
                >
                  新建文件
                </PermissionButton>
                <Upload
                  {...uploadProps}
                  disabled={!rootKey || uploadMutation.isPending}
                >
                  <PermissionButton
                    icon={<CloudUploadOutlined />}
                    permission="system:file:create"
                    disabled={!rootKey || uploadMutation.isPending}
                    hideWhenDenied={false}
                  >
                    上传文件
                  </PermissionButton>
                </Upload>
                <Segmented<FileViewMode>
                  className="material-view-toggle"
                  value={viewMode}
                  onChange={(value) => setViewMode(value)}
                  options={[
                    { label: "表格", value: "table", icon: <TableOutlined /> },
                    {
                      label: "文件",
                      value: "card",
                      icon: <AppstoreOutlined />,
                    },
                  ]}
                />
                {selectedRoot ? (
                  <Button
                    className="material-secondary-mobile-action"
                    icon={<LinkOutlined />}
                    onClick={() => copyUrl(selectedRoot)}
                  >
                    复制根URL
                  </Button>
                ) : null}
              </Space>
            </div>
            {uploadingName ? (
              <div className="upload-progress-panel">
                <Typography.Text ellipsis>{uploadingName}</Typography.Text>
                <Typography.Text type="secondary">
                  {uploadPercent}%
                </Typography.Text>
              </div>
            ) : null}
            <Upload.Dragger
              {...uploadProps}
              className="material-drop-zone"
              disabled={!rootKey || uploadMutation.isPending}
              openFileDialogOnClick={false}
            >
              <div className="material-drop-zone-hint">
                <CloudUploadOutlined />
                <Typography.Text type="secondary">
                  拖拽文件到这里上传到 {currentPath || "根目录"}
                </Typography.Text>
              </div>
            </Upload.Dragger>
            {effectiveViewMode === "table" ? (
              <TableScrollArea
                contentWidth={FILE_TABLE_SCROLL_X}
                className="files-table-scroll-area"
                label="文件管理表格"
                footer={
                  <StandardTablePagination
                    current={tablePage}
                    pageSize={tablePageSize}
                    total={rows.length}
                    onChange={(nextPage, nextPageSize) => {
                      const sizeChanged = nextPageSize !== tablePageSize;
                      setTablePageSize(nextPageSize);
                      setTablePage(sizeChanged ? 1 : nextPage);
                    }}
                  />
                }
              >
                <div className="files-material-table">
                  <DataTable<ManagedFileRecord>
                    rowKey="path"
                    columns={columns}
                    dataSource={pagedRows}
                    loading={browserQuery.isLoading}
                    pagination={false}
                    tableLayout="fixed"
                    scroll={{ x: FILE_TABLE_SCROLL_X }}
                    onRow={(record) => ({
                      onDoubleClick: () => {
                        if (record.is_dir) {
                          openPath(record.path);
                        } else if (
                          isEditableTextFile(record) &&
                          hasPermission("system:file:update")
                        ) {
                          openContentEditor(record);
                        } else {
                          openPreview(record);
                        }
                      },
                    })}
                  />
                </div>
              </TableScrollArea>
            ) : (
              <Spin spinning={browserQuery.isLoading}>
                {rows.length > 0 ? (
                  <div className="material-card-grid">
                    {rows.map(renderFileCard)}
                  </div>
                ) : (
                  <Empty description="暂无文件" />
                )}
              </Spin>
            )}
          </Space>
        </div>
      </FinderWorkspace>
      <Drawer
        title={mobileActionRecord?.name ?? "文件操作"}
        open={Boolean(mobileActionRecord)}
        placement="bottom"
        height="auto"
        className="material-mobile-action-drawer"
        onClose={() => setMobileActionRecord(null)}
      >
        {mobileActionRecord ? (
          <div className="material-mobile-action-sheet">
            {renderActions(mobileActionRecord)}
          </div>
        ) : null}
      </Drawer>
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
          onFinish={(values) => createFileMutation.mutate(values)}
        >
          <Form.Item label="当前目录">
            <Typography.Text>{currentPath || "根目录"}</Typography.Text>
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
          <Form.Item name="content" label="初始内容（可选）">
            <Input.TextArea
              rows={8}
              placeholder="可留空，创建后可以继续编辑"
              spellCheck={false}
            />
          </Form.Item>
        </Form>
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
            <Typography.Text>{currentPath || "根目录"}</Typography.Text>
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
        title="重命名"
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
          <Form.Item label="当前路径">
            <Typography.Text>{renaming?.path ?? "-"}</Typography.Text>
          </Form.Item>
          <Form.Item
            name="name"
            label="新名称"
            rules={[{ required: true, message: "请输入新名称" }]}
          >
            <Input />
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
          initialValues={{ visibility: "private", password_enabled: false }}
          onFinish={(values) => accessMutation.mutate(values)}
        >
          <Form.Item label="文件路径">
            <Typography.Text>{accessEditing?.path ?? "-"}</Typography.Text>
          </Form.Item>
          <Form.Item name="visibility" label="访问方式">
            <Segmented
              options={[
                { label: "私有", value: "private" },
                { label: "公开", value: "public" },
              ]}
            />
          </Form.Item>
          {accessVisibility === "public" ? (
            <Form.Item
              name="password_enabled"
              label="访问密码"
              valuePropName="checked"
            >
              <Switch checkedChildren="启用" unCheckedChildren="关闭" />
            </Form.Item>
          ) : null}
          {accessVisibility === "public" && accessPasswordEnabled ? (
            <Form.Item
              name="password"
              label={
                accessEditing?.requires_password ? "新访问密码" : "访问密码"
              }
              rules={[
                {
                  required: !accessEditing?.requires_password,
                  message: "请输入访问密码",
                },
              ]}
            >
              <Input.Password placeholder="留空则保留原密码" />
            </Form.Item>
          ) : null}
          {accessEditing?.requires_password ? (
            <Form.Item name="clear_password" valuePropName="checked">
              <Switch
                checkedChildren="清除原密码"
                unCheckedChildren="保留原密码"
              />
            </Form.Item>
          ) : null}
          {accessEditing?.public_url ? (
            <Form.Item label="公开链接">
              <Typography.Text copyable>
                {new URL(
                  accessEditing.public_url,
                  window.location.origin,
                ).toString()}
              </Typography.Text>
            </Form.Item>
          ) : null}
        </Form>
      </Modal>
      <Modal
        title={contentEditing ? `编辑文本：${contentEditing.name}` : "编辑文本"}
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
        <Spin spinning={editorLoading} tip="正在读取文件">
          <div className="managed-file-editor-shell">
            <div className="managed-file-editor-meta">
              <Space wrap size={6}>
                <Tag>UTF-8</Tag>
                <Typography.Text type="secondary" ellipsis>
                  {contentEditing?.path ?? "-"}
                </Typography.Text>
              </Space>
              <Typography.Text type="secondary">
                {editorUpdatedAt
                  ? `更新于 ${new Date(editorUpdatedAt).toLocaleString("zh-CN")}`
                  : "尚无更新时间"}
              </Typography.Text>
            </div>
            <Input.TextArea
              aria-label="文件内容编辑器"
              className="managed-file-editor-textarea"
              value={editorContent}
              disabled={editorLoading || updateContentMutation.isPending}
              autoSize={false}
              spellCheck={false}
              placeholder="输入文件内容"
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
        title={previewing ? `预览：${previewing.name}` : "文件预览"}
        open={Boolean(previewing)}
        onCancel={closePreview}
        footer={
          previewing ? (
            <Space wrap className="material-preview-actions">
              <Button
                icon={<LinkOutlined />}
                onClick={() => copyUrl(previewing)}
              >
                复制URL
              </Button>
              <PermissionButton
                icon={<CloudDownloadOutlined />}
                permission="system:file:download"
                loading={downloadMutation.isPending}
                onClick={() => downloadMutation.mutate(previewing)}
              >
                下载
              </PermissionButton>
              {isEditableTextFile(previewing) ? (
                <PermissionButton
                  icon={<FileTextOutlined />}
                  permission="system:file:update"
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
    </CrudPage>
  );
}
