import {
  CloudDownloadOutlined,
  CloudUploadOutlined,
  DeleteOutlined,
  EditOutlined,
  ExclamationCircleOutlined,
  EyeOutlined,
  FolderOpenOutlined,
  FolderOutlined,
  LinkOutlined,
  PlusOutlined,
  ReloadOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Breadcrumb,
  Button,
  Card,
  Empty,
  Form,
  Input,
  Modal,
  Popconfirm,
  Select,
  Space,
  Spin,
  Tag,
  Tree,
  Typography,
  Upload,
  message,
} from "antd";
import type { UploadProps } from "antd";
import type { ColumnsType } from "antd/es/table";
import type { DataNode } from "antd/es/tree";
import { type Key, useEffect, useMemo, useState } from "react";
import {
  type FileRootRecord,
  type ManagedFileRecord,
  createFileFolder,
  deleteManagedFile,
  downloadManagedFile,
  fetchFileBrowser,
  fetchFileRoots,
  previewManagedFile,
  renameManagedFile,
  uploadManagedFile,
} from "../../../api/admin/files";
import { CrudPage } from "../../../components/admin/CrudPage";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { fileIcon, fileKind, formatBytes } from "../uploads/uploadTaskManager";

type FolderFormValues = {
  name: string;
};

type RenameFormValues = {
  name: string;
};

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

export function FilesPage() {
  const [rootKey, setRootKey] = useState<string>();
  const [currentPath, setCurrentPath] = useState("");
  const [keyword, setKeyword] = useState("");
  const [folderModalOpen, setFolderModalOpen] = useState(false);
  const [renaming, setRenaming] = useState<ManagedFileRecord | null>(null);
  const [previewing, setPreviewing] = useState<ManagedFileRecord | null>(null);
  const [previewObjectUrl, setPreviewObjectUrl] = useState<string>();
  const [previewText, setPreviewText] = useState<string>();
  const [previewError, setPreviewError] = useState<string>();
  const [previewLoading, setPreviewLoading] = useState(false);
  const [uploadPercent, setUploadPercent] = useState(0);
  const [uploadingName, setUploadingName] = useState<string>();
  const [treeData, setTreeData] = useState<DataNode[]>([]);
  const [expandedTreeKeys, setExpandedTreeKeys] = useState<Key[]>([""]);
  const [folderForm] = Form.useForm<FolderFormValues>();
  const [renameForm] = Form.useForm<RenameFormValues>();
  const queryClient = useQueryClient();

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
        setPreviewText(await blob.text());
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

  const copyUrl = async (record: ManagedFileRecord | FileRootRecord) => {
    const url = "path" in record ? record.url : record.url_path;
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
  };

  const renderActions = (record: ManagedFileRecord) => (
    <Space wrap size={6}>
      {record.is_dir ? (
        <Button
          size="small"
          icon={<FolderOpenOutlined />}
          onClick={() => openPath(record.path)}
        >
          打开
        </Button>
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
    { title: "URL", dataIndex: "url", width: 260, ellipsis: true },
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
      width: 360,
      render: (_, record) => renderActions(record),
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
            onSearch={setKeyword}
            className="admin-search-input"
          />
          <Button icon={<ReloadOutlined />} onClick={refreshFiles}>
            刷新
          </Button>
        </Space>
      }
    >
      <div className="resource-library-layout">
        <div className="resource-tree-panel">
          <Space direction="vertical" size="small" className="full-width">
            {selectedRoot ? (
              <Card size="small" className="admin-card">
                <Space direction="vertical" size={4} className="full-width">
                  <Typography.Text strong>{selectedRoot.name}</Typography.Text>
                  <Typography.Text type="secondary" copyable>
                    {selectedRoot.url_path}
                  </Typography.Text>
                  <Typography.Text type="secondary" ellipsis>
                    {selectedRoot.local_root}
                  </Typography.Text>
                </Space>
              </Card>
            ) : null}
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
        </div>
        <Space direction="vertical" size="middle" className="full-width">
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
              {selectedRoot ? (
                <Button
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
            <DataTable<ManagedFileRecord>
              rowKey="path"
              columns={columns}
              dataSource={rows}
              loading={browserQuery.isLoading}
              pagination={false}
              onRow={(record) => ({
                onDoubleClick: () =>
                  record.is_dir ? openPath(record.path) : openPreview(record),
              })}
            />
          </Upload.Dragger>
        </Space>
      </div>
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
