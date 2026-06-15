import {
  CheckCircleOutlined,
  CloudSyncOutlined,
  CloudUploadOutlined,
  ExclamationCircleOutlined,
  FileImageOutlined,
  FileOutlined,
  FilePdfOutlined,
  FileTextOutlined,
  PauseCircleOutlined,
  PlayCircleOutlined,
  ReloadOutlined,
  SoundOutlined,
} from "@ant-design/icons";
import { useQueryClient } from "@tanstack/react-query";
import {
  Button,
  Progress,
  Space,
  Tag,
  Typography,
  Upload,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import {
  type UploadRecord,
  type UploadTaskRecord,
  completeUploadTask,
  createUploadTask,
  uploadTaskChunk,
} from "../../../api/admin/uploads";
import {
  type RunningUpload,
  useUploadTasksStore,
} from "../../../stores/uploadTasks";

export const CHUNK_SIZE = 8 * 1024 * 1024;

export function formatBytes(value: number) {
  if (value < 1024) {
    return `${value} B`;
  }
  if (value < 1024 * 1024) {
    return `${(value / 1024).toFixed(1)} KB`;
  }
  if (value < 1024 * 1024 * 1024) {
    return `${(value / 1024 / 1024).toFixed(1)} MB`;
  }
  return `${(value / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function extensionFromName(name: string) {
  const extension = name.split(".").pop()?.toLowerCase();
  return extension && extension !== name ? extension : null;
}

export function mimeFromExtension(extension?: string | null) {
  if (!extension) {
    return null;
  }
  if (["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp"].includes(extension)) {
    return `image/${extension === "jpg" ? "jpeg" : extension}`;
  }
  if (["mp4", "webm", "ogg", "mov"].includes(extension)) {
    return `video/${extension === "mov" ? "quicktime" : extension}`;
  }
  if (["mp3", "wav", "ogg", "m4a", "aac"].includes(extension)) {
    return `audio/${extension}`;
  }
  if (extension === "pdf") {
    return "application/pdf";
  }
  if (["txt", "log", "md", "csv"].includes(extension)) {
    return "text/plain";
  }
  if (extension === "json") {
    return "application/json";
  }
  if (["yaml", "yml"].includes(extension)) {
    return "application/yaml";
  }
  return null;
}

export function fileKind(
  record: Pick<UploadRecord, "mime_type" | "extension">,
) {
  const mimeType = record.mime_type?.toLowerCase() ?? "";
  const extension = record.extension?.toLowerCase() ?? "";
  if (mimeType.startsWith("image/")) {
    return "image";
  }
  if (mimeType.startsWith("video/")) {
    return "video";
  }
  if (mimeType.startsWith("audio/")) {
    return "audio";
  }
  if (mimeType === "application/pdf" || extension === "pdf") {
    return "pdf";
  }
  if (
    mimeType.startsWith("text/") ||
    ["json", "md", "markdown", "yaml", "yml", "csv", "log"].includes(extension)
  ) {
    return "text";
  }
  return "file";
}

export function fileIcon(
  record: Pick<UploadRecord, "mime_type" | "extension">,
) {
  const kind = fileKind(record);
  if (kind === "image") {
    return <FileImageOutlined />;
  }
  if (kind === "video") {
    return <PlayCircleOutlined />;
  }
  if (kind === "audio") {
    return <SoundOutlined />;
  }
  if (kind === "pdf") {
    return <FilePdfOutlined />;
  }
  if (kind === "text") {
    return <FileTextOutlined />;
  }
  return <FileOutlined />;
}

export function uploadTaskStatus(
  task: UploadTaskRecord,
  running?: RunningUpload,
) {
  if (running?.paused) {
    return { label: "暂停", color: "default", icon: <PauseCircleOutlined /> };
  }
  if (running) {
    return {
      label: "上传中",
      color: "processing",
      icon: <CloudUploadOutlined />,
    };
  }
  if (task.status === "completed") {
    return { label: "已完成", color: "success", icon: <CheckCircleOutlined /> };
  }
  if (task.status === "failed") {
    return {
      label: "可重试",
      color: "error",
      icon: <ExclamationCircleOutlined />,
    };
  }
  if (task.status === "importing") {
    return { label: "入库中", color: "gold", icon: <CloudSyncOutlined /> };
  }
  if (task.uploaded_chunks.length > 0) {
    return { label: "可续传", color: "warning", icon: <ReloadOutlined /> };
  }
  return { label: "等待上传", color: "default", icon: <PauseCircleOutlined /> };
}

export function isTaskFullyUploaded(task: UploadTaskRecord) {
  return task.uploaded_chunks.length === task.total_chunks;
}

type UseUploadTaskManagerOptions = {
  bucketId?: number;
  prefix?: string;
  visibility: "private" | "public";
};

export function useUploadTaskManager({
  bucketId,
  prefix,
  visibility,
}: UseUploadTaskManagerOptions) {
  const queryClient = useQueryClient();
  const runningUploads = useUploadTasksStore((state) => state.runningUploads);
  const setTaskRunning = useUploadTasksStore((state) => state.setTaskRunning);
  const pauseTask = useUploadTasksStore((state) => state.pauseTask);

  const startChunkUpload = async (
    file: File,
    existingTask?: UploadTaskRecord,
  ) => {
    if (!bucketId && !existingTask?.storage_bucket_id) {
      message.warning("请先选择存储桶");
      return;
    }
    let task = existingTask;
    try {
      task ??= await createUploadTask({
        storage_bucket_id: bucketId ?? 0,
        original_name: file.name,
        mime_type: file.type || mimeFromExtension(extensionFromName(file.name)),
        size_bytes: file.size,
        chunk_size: CHUNK_SIZE,
        total_chunks: Math.ceil(file.size / CHUNK_SIZE),
        prefix,
        visibility,
      });
      setTaskRunning(task.id, { file, paused: false });
      queryClient.setQueryData<UploadTaskRecord[]>(
        ["admin-upload-tasks"],
        (tasks) => {
          const current = tasks ?? [];
          if (current.some((item) => item.id === task.id)) {
            return current.map((item) => (item.id === task.id ? task : item));
          }
          return [task, ...current];
        },
      );
      const uploaded = new Set(task.uploaded_chunks);
      for (let index = 0; index < task.total_chunks; index += 1) {
        const running = useUploadTasksStore.getState().runningUploads[task.id];
        if (!running || running.paused) {
          break;
        }
        if (uploaded.has(index)) {
          continue;
        }
        const start = index * task.chunk_size;
        const chunk = file.slice(
          start,
          Math.min(file.size, start + task.chunk_size),
        );
        task = await uploadTaskChunk(task.id, index, chunk);
        uploaded.add(index);
        queryClient.setQueryData<UploadTaskRecord[]>(
          ["admin-upload-tasks"],
          (tasks) =>
            (tasks ?? []).map((item) => (item.id === task?.id ? task : item)),
        );
      }
      const running = useUploadTasksStore.getState().runningUploads[task.id];
      if (running?.paused) {
        message.info("上传已暂停，可在上传任务页面继续");
        return;
      }
      if (isTaskFullyUploaded(task)) {
        task = await completeUploadTask(task.id);
        message.success(
          `${task.original_name} 已提交入库，上传任务页面会自动更新结果`,
        );
        setTaskRunning(task.id);
        queryClient.invalidateQueries({ queryKey: ["admin-upload-tasks"] });
        queryClient.invalidateQueries({ queryKey: ["admin-uploads"] });
        queryClient.invalidateQueries({ queryKey: ["admin-upload-browser"] });
      }
    } catch (error) {
      if (task) {
        setTaskRunning(task.id);
      }
      message.error(error instanceof Error ? error.message : "分片上传失败");
      queryClient.invalidateQueries({ queryKey: ["admin-upload-tasks"] });
    }
  };

  const resumeTask = (task: UploadTaskRecord) => {
    const running = useUploadTasksStore.getState().runningUploads[task.id];
    if (!running) {
      return;
    }
    startChunkUpload(running.file, task);
  };

  const submitTaskImport = async (task: UploadTaskRecord) => {
    try {
      const nextTask = await completeUploadTask(task.id);
      queryClient.setQueryData<UploadTaskRecord[]>(
        ["admin-upload-tasks"],
        (tasks) =>
          (tasks ?? []).map((item) =>
            item.id === nextTask.id ? nextTask : item,
          ),
      );
      message.success(
        `${nextTask.original_name} 已提交入库，上传任务页面会自动更新结果`,
      );
      queryClient.invalidateQueries({ queryKey: ["admin-upload-tasks"] });
      queryClient.invalidateQueries({ queryKey: ["admin-uploads"] });
      queryClient.invalidateQueries({ queryKey: ["admin-upload-browser"] });
    } catch (error) {
      message.error(
        error instanceof Error ? error.message : "入库任务提交失败",
      );
      queryClient.invalidateQueries({ queryKey: ["admin-upload-tasks"] });
    }
  };

  return {
    runningUploads,
    startChunkUpload,
    pauseTask,
    resumeTask,
    submitTaskImport,
  };
}

type UploadTaskColumnActions = ReturnType<typeof useUploadTaskManager>;

export function createUploadTaskColumns({
  runningUploads,
  pauseTask,
  resumeTask,
  startChunkUpload,
  submitTaskImport,
}: UploadTaskColumnActions): ColumnsType<UploadTaskRecord> {
  return [
    {
      title: "文件",
      dataIndex: "original_name",
      ellipsis: true,
      render: (value, task) => (
        <Space size={8}>
          <span className="material-table-file-icon">
            {fileIcon(task as UploadRecord)}
          </span>
          <Typography.Text ellipsis className="material-table-file-name">
            {value}
          </Typography.Text>
        </Space>
      ),
    },
    {
      title: "状态",
      key: "status",
      width: 110,
      render: (_, task) => {
        const statusInfo = uploadTaskStatus(task, runningUploads[task.id]);
        return (
          <Tag color={statusInfo.color} icon={statusInfo.icon}>
            {statusInfo.label}
          </Tag>
        );
      },
    },
    {
      title: "进度",
      key: "progress",
      width: 220,
      render: (_, task) => {
        const percent = Math.round(
          (task.uploaded_chunks.length / task.total_chunks) * 100,
        );
        return (
          <Progress
            percent={percent}
            size="small"
            status={task.status === "failed" ? "exception" : "active"}
          />
        );
      },
    },
    {
      title: "大小",
      dataIndex: "size_bytes",
      width: 110,
      render: formatBytes,
    },
    {
      title: "分片",
      key: "chunks",
      width: 120,
      render: (_, task) =>
        `${task.uploaded_chunks.length}/${task.total_chunks}`,
    },
    {
      title: "更新时间",
      dataIndex: "updated_at",
      width: 220,
    },
    {
      title: "操作",
      key: "actions",
      width: 260,
      render: (_, task) => {
        const running = runningUploads[task.id];
        const fullyUploaded = isTaskFullyUploaded(task);
        return (
          <Space wrap size={6}>
            {running && !running.paused ? (
              <Button
                size="small"
                icon={<PauseCircleOutlined />}
                onClick={() => pauseTask(task.id)}
              >
                暂停
              </Button>
            ) : null}
            {running?.paused ? (
              <Button
                size="small"
                icon={<PlayCircleOutlined />}
                onClick={() => resumeTask(task)}
              >
                继续
              </Button>
            ) : null}
            {!running && task.status !== "completed" ? (
              fullyUploaded ? (
                <Button
                  size="small"
                  icon={<CloudSyncOutlined />}
                  onClick={() => submitTaskImport(task)}
                >
                  {task.status === "importing" ? "重新入库" : "重试入库"}
                </Button>
              ) : (
                <Upload
                  showUploadList={false}
                  beforeUpload={(file) => {
                    startChunkUpload(file, task);
                    return false;
                  }}
                >
                  <Button size="small" icon={<ReloadOutlined />}>
                    续传缺失分片
                  </Button>
                </Upload>
              )
            ) : null}
          </Space>
        );
      },
    },
  ];
}
