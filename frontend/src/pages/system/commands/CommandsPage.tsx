import {
  ApartmentOutlined,
  CodeOutlined,
  DeleteOutlined,
  EditOutlined,
  EyeOutlined,
  FileSearchOutlined,
  MinusCircleOutlined,
  NumberOutlined,
  PlayCircleOutlined,
  PlusOutlined,
  ReloadOutlined,
  StopOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  Button,
  Drawer,
  Empty,
  Form,
  Input,
  Modal,
  Popconfirm,
  Select,
  Space,
  Spin,
  Switch,
  Tabs,
  Tag,
  Typography,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  type CommandRunLogRecord,
  type CommandRunRecord,
  type CommandTemplateRecord,
  type CommandWorkflowRecord,
  type CommandWorkflowRunRecord,
  type RunCommandParams,
  type SaveCommandTemplateParams,
  type SaveCommandWorkflowParams,
  buildCommandRunLogWebSocketUrl,
  cancelCommandRun,
  createCommandRunLogTicket,
  createCommandTemplate,
  createCommandWorkflow,
  deleteCommandTemplate,
  deleteCommandWorkflow,
  fetchCommandRun,
  fetchCommandRunLogs,
  fetchCommandRuns,
  fetchCommandTemplates,
  fetchCommandWorkflowRun,
  fetchCommandWorkflowRuns,
  fetchCommandWorkflows,
  previewCommandRunArtifact,
  runAdHocCommand,
  runCommandTemplate,
  runCommandWorkflow,
  updateCommandTemplate,
  updateCommandWorkflow,
} from "../../../api/admin/commands";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { StatusTag } from "../../../components/admin/StatusTag";
import { fileKind, mimeFromExtension } from "../uploads/uploadTaskManager";

const statusColor: Record<string, string> = {
  queued: "processing",
  running: "blue",
  success: "green",
  failed: "red",
  cancelled: "orange",
};

type RunFormValues = RunCommandParams & {
  template_id?: number | null;
  base_command?: string;
  args?: Array<string | null | undefined>;
};

type WorkflowFormValues = Omit<SaveCommandWorkflowParams, "steps"> & {
  steps?: Array<SaveCommandWorkflowParams["steps"][number] | undefined>;
};

const RANDOM_PLACEHOLDER = "{{random:8}}";

type InputElementRef = {
  input?: HTMLInputElement | null;
  setSelectionRange?: (start: number, end: number) => void;
  focus?: () => void;
};

type TextAreaElementRef = {
  resizableTextArea?: { textArea?: HTMLTextAreaElement | null };
  focus?: () => void;
};

function normalizeEmpty(value?: string | null) {
  return value?.trim() ? value : null;
}

function isActiveRun(status?: string) {
  return status === "queued" || status === "running";
}

function splitCommandArgs(value?: string | null) {
  const input = value?.trim();
  if (!input) {
    return [];
  }
  const args: string[] = [];
  let current = "";
  let quote: '"' | "'" | null = null;
  let escaped = false;
  for (const char of input) {
    if (escaped) {
      current += char;
      escaped = false;
      continue;
    }
    if (char === "\\") {
      escaped = true;
      continue;
    }
    if (quote) {
      if (char === quote) {
        quote = null;
      } else {
        current += char;
      }
      continue;
    }
    if (char === '"' || char === "'") {
      quote = char;
      continue;
    }
    if (/\s/.test(char)) {
      if (current) {
        args.push(current);
        current = "";
      }
      continue;
    }
    current += char;
  }
  if (current) {
    args.push(current);
  }
  return args;
}

function quoteCommandArg(value: string) {
  if (!value) {
    return "''";
  }
  if (/^[A-Za-z0-9_./:=+@%-]+$/.test(value)) {
    return value;
  }
  return `'${value.replaceAll("'", "'\\''")}'`;
}

function buildCommandLine(
  baseCommand?: string,
  args?: Array<string | null | undefined>,
) {
  return [
    baseCommand?.trim(),
    ...(args ?? [])
      .map((arg) => arg?.trim())
      .filter(Boolean)
      .map((arg) => quoteCommandArg(arg as string)),
  ]
    .filter(Boolean)
    .join(" ");
}

function insertAtSelection(
  text: string,
  insertText: string,
  start?: number | null,
  end?: number | null,
) {
  const safeStart = start ?? text.length;
  const safeEnd = end ?? safeStart;
  return `${text.slice(0, safeStart)}${insertText}${text.slice(safeEnd)}`;
}

function restoreTemplateRandomArgs(
  args: string[],
  template?: CommandTemplateRecord,
) {
  const templateArgs = splitCommandArgs(template?.default_args);
  return args.map((arg, index) =>
    templateArgs[index]?.includes("{{random") ? templateArgs[index] : arg,
  );
}

function commandArtifactName(run: CommandRunRecord) {
  return run.preview_path?.split(/[\\/]/).filter(Boolean).at(-1) ?? run.name;
}

function commandArtifactRecord(run: CommandRunRecord) {
  const name = commandArtifactName(run);
  const extension = name.includes(".") ? name.split(".").at(-1) : undefined;
  return {
    name,
    extension,
    mime_type: mimeFromExtension(extension),
    is_dir: false,
  };
}

function isTextArtifact(run: CommandRunRecord) {
  const extension = commandArtifactRecord(run).extension?.toLowerCase() ?? "";
  return ["txt", "log", "json", "md", "csv", "yaml", "yml", "xml"].includes(
    extension,
  );
}

function parseCommandLineForEdit(
  commandLine: string,
  template?: CommandTemplateRecord,
) {
  const trimmed = commandLine.trim();
  const templateCommand = template?.command.trim();
  if (templateCommand && trimmed.startsWith(templateCommand)) {
    return {
      baseCommand: templateCommand,
      args: restoreTemplateRandomArgs(
        splitCommandArgs(trimmed.slice(templateCommand.length)),
        template,
      ),
    };
  }
  const [baseCommand = "", ...args] = splitCommandArgs(trimmed);
  return { baseCommand, args };
}

export function CommandsPage() {
  const [templatePage, setTemplatePage] = useState(1);
  const [runPage, setRunPage] = useState(1);
  const [workflowPage, setWorkflowPage] = useState(1);
  const [workflowRunPage, setWorkflowRunPage] = useState(1);
  const [editing, setEditing] = useState<CommandTemplateRecord | null>(null);
  const [editingWorkflow, setEditingWorkflow] =
    useState<CommandWorkflowRecord | null>(null);
  const [templateOpen, setTemplateOpen] = useState(false);
  const [workflowOpen, setWorkflowOpen] = useState(false);
  const [runOpen, setRunOpen] = useState(false);
  const [detailRunId, setDetailRunId] = useState<number | null>(null);
  const [detailWorkflowRunId, setDetailWorkflowRunId] = useState<number | null>(
    null,
  );
  const [previewingRun, setPreviewingRun] = useState<CommandRunRecord | null>(
    null,
  );
  const [previewObjectUrl, setPreviewObjectUrl] = useState<string>();
  const [previewText, setPreviewText] = useState<string>();
  const [previewError, setPreviewError] = useState<string>();
  const [previewLoading, setPreviewLoading] = useState(false);
  const [logs, setLogs] = useState<CommandRunLogRecord[]>([]);
  const [templateForm] = Form.useForm<SaveCommandTemplateParams>();
  const [workflowForm] = Form.useForm<WorkflowFormValues>();
  const [runForm] = Form.useForm<RunFormValues>();
  const runArgRefs = useRef<Record<number, InputElementRef | null>>({});
  const workflowStepArgRefs = useRef<Record<number, TextAreaElementRef | null>>(
    {},
  );
  const lastSeqRef = useRef(0);
  const queryClient = useQueryClient();

  const templatesQuery = useQuery({
    queryKey: ["admin-command-templates", templatePage],
    queryFn: () => fetchCommandTemplates({ page: templatePage, page_size: 10 }),
  });
  const templateOptionsQuery = useQuery({
    queryKey: ["admin-command-template-options"],
    queryFn: () => fetchCommandTemplates({ page: 1, page_size: 100 }),
  });
  const runsQuery = useQuery({
    queryKey: ["admin-command-runs", runPage],
    queryFn: () => fetchCommandRuns({ page: runPage, page_size: 10 }),
    refetchInterval: 3000,
  });
  const workflowsQuery = useQuery({
    queryKey: ["admin-command-workflows", workflowPage],
    queryFn: () => fetchCommandWorkflows({ page: workflowPage, page_size: 10 }),
  });
  const workflowRunsQuery = useQuery({
    queryKey: ["admin-command-workflow-runs", workflowRunPage],
    queryFn: () =>
      fetchCommandWorkflowRuns({ page: workflowRunPage, page_size: 10 }),
    refetchInterval: 3000,
  });
  const detailQuery = useQuery({
    queryKey: ["admin-command-run", detailRunId],
    queryFn: () => fetchCommandRun(detailRunId as number),
    enabled: Boolean(detailRunId),
    refetchInterval: (query) =>
      isActiveRun(query.state.data?.status) ? 1500 : false,
  });
  const workflowDetailQuery = useQuery({
    queryKey: ["admin-command-workflow-run", detailWorkflowRunId],
    queryFn: () => fetchCommandWorkflowRun(detailWorkflowRunId as number),
    enabled: Boolean(detailWorkflowRunId),
    refetchInterval: (query) =>
      isActiveRun(query.state.data?.status) ? 1500 : false,
  });
  const runBaseCommand = Form.useWatch("base_command", runForm);
  const runArgs = Form.useWatch("args", runForm);

  useEffect(() => {
    return () => {
      if (previewObjectUrl) {
        URL.revokeObjectURL(previewObjectUrl);
      }
    };
  }, [previewObjectUrl]);

  useEffect(() => {
    if (!runOpen) {
      return;
    }
    runForm.setFieldValue(
      "command_line",
      buildCommandLine(runBaseCommand, runArgs),
    );
  }, [runOpen, runBaseCommand, runArgs, runForm]);

  const saveMutation = useMutation({
    mutationFn: (values: SaveCommandTemplateParams) => {
      const payload = {
        ...values,
        description: normalizeEmpty(values.description),
        default_args: normalizeEmpty(values.default_args),
        env_vars: normalizeEmpty(values.env_vars),
        setup_script: normalizeEmpty(values.setup_script),
        python_venv_path: normalizeEmpty(values.python_venv_path),
        preview_path_template: normalizeEmpty(values.preview_path_template),
        timeout_seconds: values.timeout_seconds ?? null,
      };
      return editing
        ? updateCommandTemplate(editing.id, payload)
        : createCommandTemplate(payload);
    },
    onSuccess: () => {
      message.success("命令模板已保存");
      setTemplateOpen(false);
      setEditing(null);
      queryClient.invalidateQueries({ queryKey: ["admin-command-templates"] });
    },
  });
  const deleteMutation = useMutation({
    mutationFn: deleteCommandTemplate,
    onSuccess: () => {
      message.success("命令模板已删除");
      queryClient.invalidateQueries({ queryKey: ["admin-command-templates"] });
    },
  });
  const saveWorkflowMutation = useMutation({
    mutationFn: (values: WorkflowFormValues) => {
      const payload: SaveCommandWorkflowParams = {
        name: values.name,
        code: values.code,
        description: normalizeEmpty(values.description),
        enabled: values.enabled ?? true,
        steps: (values.steps ?? []).filter(Boolean).map((step, index) => ({
          template_id: step?.template_id as number,
          name: step?.name ?? `步骤 ${index + 1}`,
          sort_order: step?.sort_order ?? index + 1,
          args: normalizeEmpty(step?.args),
          env_vars: normalizeEmpty(step?.env_vars),
          working_directory: normalizeEmpty(step?.working_directory),
          timeout_seconds: step?.timeout_seconds ?? null,
          enabled: step?.enabled ?? true,
        })),
      };
      return editingWorkflow
        ? updateCommandWorkflow(editingWorkflow.id, payload)
        : createCommandWorkflow(payload);
    },
    onSuccess: () => {
      message.success("任务编排已保存");
      setWorkflowOpen(false);
      setEditingWorkflow(null);
      queryClient.invalidateQueries({ queryKey: ["admin-command-workflows"] });
    },
  });
  const deleteWorkflowMutation = useMutation({
    mutationFn: deleteCommandWorkflow,
    onSuccess: () => {
      message.success("任务编排已删除");
      queryClient.invalidateQueries({ queryKey: ["admin-command-workflows"] });
    },
  });
  const runWorkflowMutation = useMutation({
    mutationFn: (workflow: CommandWorkflowRecord) =>
      runCommandWorkflow(workflow.id, { name: workflow.name }),
    onSuccess: (run) => {
      message.success("任务编排已开始执行");
      setDetailWorkflowRunId(run.id);
      queryClient.invalidateQueries({
        queryKey: ["admin-command-workflow-runs"],
      });
    },
  });
  const runMutation = useMutation({
    mutationFn: (values: RunFormValues) => {
      const payload: RunCommandParams = {
        name: values.name,
        working_directory: values.working_directory,
        command_line: values.command_line,
        setup_script: normalizeEmpty(values.setup_script),
        python_venv_path: normalizeEmpty(values.python_venv_path),
        env_vars: normalizeEmpty(values.env_vars),
        timeout_seconds: values.timeout_seconds ?? null,
        preview_path_template: normalizeEmpty(values.preview_path_template),
      };
      return values.template_id
        ? runCommandTemplate(values.template_id, payload)
        : runAdHocCommand(payload);
    },
    onSuccess: (run) => {
      message.success("命令已开始执行");
      setRunOpen(false);
      openRunDetail(run.id);
      queryClient.invalidateQueries({ queryKey: ["admin-command-runs"] });
    },
  });
  const cancelMutation = useMutation({
    mutationFn: cancelCommandRun,
    onSuccess: (run) => {
      message.success("命令已取消");
      queryClient.setQueryData(["admin-command-run", run.id], run);
      queryClient.invalidateQueries({ queryKey: ["admin-command-runs"] });
    },
  });

  const openRunDetail = useCallback((runId: number) => {
    setDetailRunId(runId);
    setLogs([]);
    lastSeqRef.current = 0;
  }, []);

  const closePreview = () => {
    if (previewObjectUrl) {
      URL.revokeObjectURL(previewObjectUrl);
    }
    setPreviewingRun(null);
    setPreviewObjectUrl(undefined);
    setPreviewText(undefined);
    setPreviewError(undefined);
    setPreviewLoading(false);
  };

  const openPreview = async (run: CommandRunRecord) => {
    if (!run.preview_path) {
      return;
    }
    if (previewObjectUrl) {
      URL.revokeObjectURL(previewObjectUrl);
    }
    setPreviewingRun(run);
    setPreviewObjectUrl(undefined);
    setPreviewText(undefined);
    setPreviewError(undefined);
    setPreviewLoading(true);
    try {
      const blob = await previewCommandRunArtifact(run.id);
      if (isTextArtifact(run)) {
        setPreviewText(await blob.text());
      } else {
        setPreviewObjectUrl(URL.createObjectURL(blob));
      }
    } catch (error) {
      setPreviewError(error instanceof Error ? error.message : "预览失败");
    } finally {
      setPreviewLoading(false);
    }
  };

  useEffect(() => {
    if (!detailRunId) {
      return;
    }
    let cancelled = false;
    const loadLogs = async () => {
      const nextLogs = await fetchCommandRunLogs(detailRunId, {
        after_seq: lastSeqRef.current,
        limit: 500,
      });
      if (cancelled || nextLogs.length === 0) {
        return;
      }
      lastSeqRef.current = nextLogs[nextLogs.length - 1].seq;
      setLogs((current) => [...current, ...nextLogs]);
    };
    void loadLogs();
    const interval = window.setInterval(loadLogs, 1500);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [detailRunId]);

  useEffect(() => {
    if (!detailRunId) {
      return;
    }
    let socket: WebSocket | null = null;
    let closed = false;
    createCommandRunLogTicket(detailRunId)
      .then((ticket) => {
        if (closed) {
          return;
        }
        socket = new WebSocket(buildCommandRunLogWebSocketUrl(ticket.ticket));
        socket.onmessage = (event) => {
          const log = JSON.parse(String(event.data)) as CommandRunLogRecord;
          if (log.seq <= lastSeqRef.current) {
            return;
          }
          lastSeqRef.current = log.seq;
          setLogs((current) => [...current, log]);
        };
      })
      .catch(() => undefined);
    return () => {
      closed = true;
      socket?.close();
    };
  }, [detailRunId]);

  const logText = useMemo(
    () =>
      logs
        .map((log) => {
          const prefix =
            log.stream === "stderr"
              ? "[stderr] "
              : log.stream === "system"
                ? "[system] "
                : "";
          return `${prefix}${log.chunk}`;
        })
        .join(""),
    [logs],
  );

  const openTemplateEditor = (record?: CommandTemplateRecord) => {
    setEditing(record ?? null);
    templateForm.resetFields();
    templateForm.setFieldsValue(
      record ?? {
        working_directory: "/tmp",
        enabled: true,
      },
    );
    setTemplateOpen(true);
  };

  const openWorkflowEditor = (record?: CommandWorkflowRecord) => {
    setEditingWorkflow(record ?? null);
    workflowForm.resetFields();
    workflowForm.setFieldsValue(
      record
        ? {
            name: record.name,
            code: record.code,
            description: record.description,
            enabled: record.enabled,
            steps: record.steps.map((step) => ({
              template_id: step.template_id,
              name: step.name,
              sort_order: step.sort_order,
              args: step.args,
              env_vars: step.env_vars,
              working_directory: step.working_directory,
              timeout_seconds: step.timeout_seconds,
              enabled: step.enabled,
            })),
          }
        : {
            enabled: true,
            steps: [{ sort_order: 1, enabled: true, name: "步骤 1" }],
          },
    );
    setWorkflowOpen(true);
  };

  const insertRunArg = (index: number, value: string) => {
    const args = [...(runForm.getFieldValue("args") ?? [])];
    const currentValue = args[index] ?? "";
    const input = runArgRefs.current[index]?.input;
    const start = input?.selectionStart;
    const end = input?.selectionEnd;
    args[index] = insertAtSelection(currentValue, value, start, end);
    runForm.setFieldValue("args", args);
    window.requestAnimationFrame(() => {
      const nextPosition = (start ?? currentValue.length) + value.length;
      runArgRefs.current[index]?.setSelectionRange?.(
        nextPosition,
        nextPosition,
      );
      runArgRefs.current[index]?.focus?.();
    });
  };

  const insertWorkflowStepArg = (index: number, value: string) => {
    const steps = [...(workflowForm.getFieldValue("steps") ?? [])];
    const currentValue = steps[index]?.args ?? "";
    const textArea =
      workflowStepArgRefs.current[index]?.resizableTextArea?.textArea;
    const start = textArea?.selectionStart;
    const end = textArea?.selectionEnd;
    steps[index] = {
      ...(steps[index] ?? {}),
      args: insertAtSelection(currentValue, value, start, end),
    };
    workflowForm.setFieldValue("steps", steps);
    window.requestAnimationFrame(() => {
      const nextPosition = (start ?? currentValue.length) + value.length;
      textArea?.setSelectionRange(nextPosition, nextPosition);
      workflowStepArgRefs.current[index]?.focus?.();
    });
  };

  const openRunModal = (
    template?: CommandTemplateRecord,
    run?: CommandRunRecord,
  ) => {
    runForm.resetFields();
    if (run) {
      const sourceTemplate = templateOptionsQuery.data?.items.find(
        (item) => item.id === run.template_id,
      );
      const parsed = parseCommandLineForEdit(run.command_line, sourceTemplate);
      runForm.setFieldsValue({
        template_id: sourceTemplate?.id ?? null,
        name: run.name,
        working_directory: run.working_directory,
        base_command: parsed.baseCommand,
        args: parsed.args,
        command_line: buildCommandLine(parsed.baseCommand, parsed.args),
        setup_script: sourceTemplate?.setup_script,
        python_venv_path: sourceTemplate?.python_venv_path,
        env_vars: sourceTemplate?.env_vars,
        timeout_seconds: sourceTemplate?.timeout_seconds,
        preview_path_template:
          run.preview_path_template ?? sourceTemplate?.preview_path_template,
      });
    } else if (template) {
      const args = splitCommandArgs(template.default_args);
      runForm.setFieldsValue({
        template_id: template.id,
        name: template.name,
        working_directory: template.working_directory,
        base_command: template.command,
        args,
        command_line: buildCommandLine(template.command, args),
        setup_script: template.setup_script,
        python_venv_path: template.python_venv_path,
        env_vars: template.env_vars,
        timeout_seconds: template.timeout_seconds,
        preview_path_template: template.preview_path_template,
      });
    } else {
      runForm.setFieldsValue({
        working_directory: "/tmp",
        name: "临时命令",
        base_command: "",
        args: [],
        command_line: "",
      });
    }
    setRunOpen(true);
  };

  const templateColumns: ColumnsType<CommandTemplateRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    { title: "名称", dataIndex: "name", width: 160 },
    { title: "编码", dataIndex: "code", width: 160 },
    {
      title: "目录",
      dataIndex: "working_directory",
      width: 240,
      ellipsis: true,
    },
    { title: "命令", dataIndex: "command", width: 260, ellipsis: true },
    {
      title: "启用",
      dataIndex: "enabled",
      width: 90,
      render: (value) => <StatusTag active={value} />,
    },
    {
      title: "操作",
      key: "actions",
      width: 280,
      fixed: "right",
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            icon={<PlayCircleOutlined />}
            permission="system:command:run"
            onClick={() => openRunModal(record)}
          >
            运行
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:command:update"
            onClick={() => openTemplateEditor(record)}
          >
            编辑
          </PermissionButton>
          <Popconfirm
            title="确认删除命令模板？"
            onConfirm={() => deleteMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              icon={<DeleteOutlined />}
              permission="system:command:delete"
            >
              删除
            </PermissionButton>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  const workflowColumns: ColumnsType<CommandWorkflowRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    { title: "名称", dataIndex: "name", width: 180 },
    { title: "编码", dataIndex: "code", width: 180 },
    {
      title: "步骤数",
      key: "steps",
      width: 90,
      render: (_, record) => record.steps.filter((step) => step.enabled).length,
    },
    {
      title: "启用",
      dataIndex: "enabled",
      width: 90,
      render: (value) => <StatusTag active={value} />,
    },
    {
      title: "操作",
      key: "actions",
      width: 300,
      fixed: "right",
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            icon={<PlayCircleOutlined />}
            permission="system:command:run"
            onClick={() => runWorkflowMutation.mutate(record)}
          >
            排队执行
          </PermissionButton>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:command:update"
            onClick={() => openWorkflowEditor(record)}
          >
            编辑
          </PermissionButton>
          <Popconfirm
            title="确认删除任务编排？"
            onConfirm={() => deleteWorkflowMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              icon={<DeleteOutlined />}
              permission="system:command:delete"
            >
              删除
            </PermissionButton>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  const workflowRunColumns: ColumnsType<CommandWorkflowRunRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    { title: "名称", dataIndex: "name", width: 180 },
    {
      title: "状态",
      dataIndex: "status",
      width: 110,
      render: (value) => (
        <Tag color={statusColor[value] ?? "default"}>{value}</Tag>
      ),
    },
    {
      title: "步骤进度",
      key: "steps",
      width: 160,
      render: (_, record) =>
        `${record.steps.filter((step) => step.status === "success").length}/${record.steps.length}`,
    },
    {
      title: "耗时",
      dataIndex: "duration_ms",
      width: 100,
      render: (value) => (value ? `${value} ms` : "-"),
    },
    {
      title: "开始时间",
      dataIndex: "started_at",
      width: 220,
      render: (value) => value ?? "-",
    },
    {
      title: "操作",
      key: "actions",
      width: 100,
      fixed: "right",
      render: (_, record) => (
        <Button size="small" onClick={() => setDetailWorkflowRunId(record.id)}>
          详情
        </Button>
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
    if (!previewingRun) {
      return <Empty description="暂无预览内容" />;
    }
    if (previewError) {
      return (
        <div className="material-preview-empty">
          <FileSearchOutlined className="material-preview-large-icon" />
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
    const kind = fileKind(commandArtifactRecord(previewingRun));
    if (kind === "image") {
      return (
        <img
          className="material-preview-image"
          src={previewObjectUrl}
          alt={commandArtifactName(previewingRun)}
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
          title={commandArtifactName(previewingRun)}
        />
      );
    }
    return (
      <div className="material-preview-empty">
        <Typography.Text type="secondary">
          该文件类型暂不支持在线预览，可以复制路径后在服务器查看。
        </Typography.Text>
      </div>
    );
  };

  const runColumns: ColumnsType<CommandRunRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    { title: "名称", dataIndex: "name", width: 160 },
    {
      title: "状态",
      dataIndex: "status",
      width: 110,
      render: (value) => (
        <Tag color={statusColor[value] ?? "default"}>{value}</Tag>
      ),
    },
    {
      title: "目录",
      dataIndex: "working_directory",
      width: 220,
      ellipsis: true,
    },
    { title: "命令", dataIndex: "command_line", width: 300, ellipsis: true },
    {
      title: "退出码",
      dataIndex: "exit_code",
      width: 90,
      render: (value) => value ?? "-",
    },
    {
      title: "耗时",
      dataIndex: "duration_ms",
      width: 100,
      render: (value) => (value ? `${value} ms` : "-"),
    },
    {
      title: "开始时间",
      dataIndex: "started_at",
      width: 220,
      render: (value) => value ?? "-",
    },
    {
      title: "操作",
      key: "actions",
      width: 340,
      fixed: "right",
      render: (_, record) => (
        <Space>
          <Button size="small" onClick={() => openRunDetail(record.id)}>
            日志
          </Button>
          {record.preview_path ? (
            <Button
              size="small"
              icon={<EyeOutlined />}
              onClick={() => openPreview(record)}
            >
              预览
            </Button>
          ) : null}
          <PermissionButton
            size="small"
            permission="system:command:run"
            onClick={() => openRunModal(undefined, record)}
          >
            编辑重跑
          </PermissionButton>
          {isActiveRun(record.status) ? (
            <PermissionButton
              size="small"
              danger
              icon={<StopOutlined />}
              permission="system:command:cancel"
              onClick={() => cancelMutation.mutate(record.id)}
            >
              取消
            </PermissionButton>
          ) : null}
        </Space>
      ),
    },
  ];

  const detailRun = detailQuery.data;
  const detailWorkflowRun = workflowDetailQuery.data;

  return (
    <CrudPage
      title="命令管理"
      subtitle="管理常用命令模板、执行历史和长任务实时输出"
      breadcrumb={["系统管理", "命令管理"]}
      icon={<CodeOutlined />}
      toolbar={
        <CrudToolbar
          actions={[
            {
              key: "create",
              label: "新增模板",
              icon: <PlusOutlined />,
              primary: true,
              permission: "system:command:create",
              onClick: () => openTemplateEditor(),
            },
            {
              key: "workflow",
              label: "新增编排",
              icon: <ApartmentOutlined />,
              permission: "system:command:create",
              onClick: () => openWorkflowEditor(),
            },
            {
              key: "adhoc",
              label: "临时执行",
              icon: <PlayCircleOutlined />,
              permission: "system:command:run",
              onClick: () => openRunModal(),
            },
            {
              key: "refresh",
              label: "刷新",
              icon: <ReloadOutlined />,
              onClick: () => {
                templatesQuery.refetch();
                runsQuery.refetch();
                workflowsQuery.refetch();
                workflowRunsQuery.refetch();
              },
            },
          ]}
        />
      }
    >
      <Alert
        type="warning"
        showIcon
        message="命令输出会持久化保存，请避免在命令或日志中打印密钥、Token 等敏感信息。"
      />
      <Tabs
        items={[
          {
            key: "templates",
            label: "命令模板",
            children: (
              <DataTable<CommandTemplateRecord>
                columns={templateColumns}
                dataSource={templatesQuery.data?.items ?? []}
                loading={templatesQuery.isLoading}
                pagination={{
                  current: templatePage,
                  total: templatesQuery.data?.total ?? 0,
                  onChange: setTemplatePage,
                }}
              />
            ),
          },
          {
            key: "workflows",
            label: "任务编排",
            children: (
              <DataTable<CommandWorkflowRecord>
                columns={workflowColumns}
                dataSource={workflowsQuery.data?.items ?? []}
                loading={workflowsQuery.isLoading}
                pagination={{
                  current: workflowPage,
                  total: workflowsQuery.data?.total ?? 0,
                  onChange: setWorkflowPage,
                }}
              />
            ),
          },
          {
            key: "workflow-runs",
            label: "编排记录",
            children: (
              <DataTable<CommandWorkflowRunRecord>
                columns={workflowRunColumns}
                dataSource={workflowRunsQuery.data?.items ?? []}
                loading={workflowRunsQuery.isLoading}
                pagination={{
                  current: workflowRunPage,
                  total: workflowRunsQuery.data?.total ?? 0,
                  onChange: setWorkflowRunPage,
                }}
              />
            ),
          },
          {
            key: "runs",
            label: "执行记录",
            children: (
              <DataTable<CommandRunRecord>
                columns={runColumns}
                dataSource={runsQuery.data?.items ?? []}
                loading={runsQuery.isLoading}
                pagination={{
                  current: runPage,
                  total: runsQuery.data?.total ?? 0,
                  onChange: setRunPage,
                }}
              />
            ),
          },
        ]}
      />
      <Modal
        title={editing ? "编辑命令模板" : "新增命令模板"}
        open={templateOpen}
        onCancel={() => setTemplateOpen(false)}
        onOk={() => templateForm.submit()}
        confirmLoading={saveMutation.isPending}
        width={760}
      >
        <Form
          form={templateForm}
          layout="vertical"
          onFinish={(values) => saveMutation.mutate(values)}
        >
          <Form.Item name="name" label="名称" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="code" label="编码" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="description" label="描述">
            <Input.TextArea rows={2} />
          </Form.Item>
          <Form.Item
            name="working_directory"
            label="工作目录"
            rules={[{ required: true }]}
          >
            <Input placeholder="/path/to/project" />
          </Form.Item>
          <Form.Item name="command" label="命令" rules={[{ required: true }]}>
            <Input.TextArea
              rows={3}
              placeholder="python script.py --arg value"
            />
          </Form.Item>
          <Form.Item name="default_args" label="默认参数">
            <Input.TextArea rows={5} placeholder="--env dev --limit 10" />
          </Form.Item>
          <Form.Item
            name="preview_path_template"
            label="预览文件路径模板"
            extra="相对路径会按工作目录解析；支持 {{random:8}}，执行记录会保存当次实际文件路径。"
          >
            <Input placeholder={`outputs/result-${RANDOM_PLACEHOLDER}.mp4`} />
          </Form.Item>
          <Form.Item name="env_vars" label="环境变量 JSON">
            <Input.TextArea rows={3} placeholder='{"PYTHONPATH":"src"}' />
          </Form.Item>
          <Form.Item name="python_venv_path" label="Python 虚拟环境路径">
            <Input placeholder="/path/to/.venv" />
          </Form.Item>
          <Form.Item name="setup_script" label="前置脚本">
            <Input.TextArea
              rows={4}
              placeholder="source .env\nexport APP_ENV=dev"
            />
          </Form.Item>
          <Form.Item name="timeout_seconds" label="超时时间（秒）">
            <Input type="number" min={1} />
          </Form.Item>
          <Form.Item name="enabled" label="启用" valuePropName="checked">
            <Switch />
          </Form.Item>
        </Form>
      </Modal>
      <Modal
        title={editingWorkflow ? "编辑任务编排" : "新增任务编排"}
        open={workflowOpen}
        onCancel={() => setWorkflowOpen(false)}
        onOk={() => workflowForm.submit()}
        confirmLoading={saveWorkflowMutation.isPending}
        width={920}
      >
        <Form
          form={workflowForm}
          layout="vertical"
          onFinish={(values) => saveWorkflowMutation.mutate(values)}
        >
          <Form.Item name="name" label="名称" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="code" label="编码" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="description" label="描述">
            <Input.TextArea rows={2} />
          </Form.Item>
          <Form.List name="steps">
            {(fields, { add, remove }) => (
              <Form.Item label="编排步骤" required>
                <Space
                  direction="vertical"
                  className="full-width"
                  size="middle"
                >
                  {fields.map((field, index) => (
                    <Space
                      key={field.key}
                      direction="vertical"
                      className="command-workflow-step-card"
                      size="small"
                    >
                      <Space
                        wrap
                        className="full-width command-workflow-step-head"
                      >
                        <Typography.Text strong>
                          步骤 {index + 1}
                        </Typography.Text>
                        <Button
                          danger
                          size="small"
                          icon={<MinusCircleOutlined />}
                          onClick={() => remove(field.name)}
                        >
                          删除步骤
                        </Button>
                      </Space>
                      <Space wrap className="full-width">
                        <Form.Item
                          name={[field.name, "name"]}
                          label="步骤名称"
                          rules={[
                            { required: true, message: "请输入步骤名称" },
                          ]}
                        >
                          <Input placeholder="例如 构建前端" />
                        </Form.Item>
                        <Form.Item
                          name={[field.name, "sort_order"]}
                          label="顺序"
                          rules={[{ required: true, message: "请输入顺序" }]}
                        >
                          <Input type="number" min={1} />
                        </Form.Item>
                        <Form.Item
                          name={[field.name, "enabled"]}
                          label="启用"
                          valuePropName="checked"
                        >
                          <Switch />
                        </Form.Item>
                      </Space>
                      <Form.Item
                        name={[field.name, "template_id"]}
                        label="命令模板"
                        rules={[{ required: true, message: "请选择命令模板" }]}
                      >
                        <Select
                          showSearch
                          placeholder="选择已保存的命令模板"
                          optionFilterProp="label"
                          options={(templateOptionsQuery.data?.items ?? []).map(
                            (template) => ({
                              value: template.id,
                              label: `${template.name} / ${template.code}`,
                            }),
                          )}
                        />
                      </Form.Item>
                      <Form.Item
                        name={[field.name, "args"]}
                        label="运行参数覆盖"
                      >
                        <Input.TextArea
                          ref={(node) => {
                            workflowStepArgRefs.current[index] = node;
                          }}
                          rows={3}
                          placeholder={`例如 --seed ${RANDOM_PLACEHOLDER}`}
                        />
                      </Form.Item>
                      <Button
                        icon={<NumberOutlined />}
                        onMouseDown={(event) => event.preventDefault()}
                        onClick={() =>
                          insertWorkflowStepArg(index, RANDOM_PLACEHOLDER)
                        }
                      >
                        插入随机数占位符
                      </Button>
                      <Form.Item
                        name={[field.name, "working_directory"]}
                        label="工作目录覆盖"
                      >
                        <Input placeholder="不填则使用命令模板工作目录" />
                      </Form.Item>
                      <Form.Item
                        name={[field.name, "env_vars"]}
                        label="环境变量覆盖 JSON"
                      >
                        <Input.TextArea
                          rows={2}
                          placeholder='{"KEY":"VALUE"}'
                        />
                      </Form.Item>
                      <Form.Item
                        name={[field.name, "timeout_seconds"]}
                        label="超时时间覆盖（秒）"
                      >
                        <Input type="number" min={1} />
                      </Form.Item>
                    </Space>
                  ))}
                  <Button
                    type="dashed"
                    icon={<PlusOutlined />}
                    onClick={() =>
                      add({
                        sort_order: fields.length + 1,
                        enabled: true,
                        name: `步骤 ${fields.length + 1}`,
                      })
                    }
                    block
                  >
                    添加步骤
                  </Button>
                </Space>
              </Form.Item>
            )}
          </Form.List>
          <Form.Item name="enabled" label="启用" valuePropName="checked">
            <Switch />
          </Form.Item>
        </Form>
      </Modal>
      <Modal
        title="执行命令"
        open={runOpen}
        onCancel={() => setRunOpen(false)}
        onOk={() => runForm.submit()}
        confirmLoading={runMutation.isPending}
        width={760}
      >
        <Form
          form={runForm}
          layout="vertical"
          onFinish={(values) => runMutation.mutate(values)}
        >
          <Form.Item name="template_id" hidden>
            <Input />
          </Form.Item>
          <Form.Item name="name" label="执行名称">
            <Input />
          </Form.Item>
          <Form.Item
            name="working_directory"
            label="工作目录"
            rules={[{ required: true }]}
          >
            <Input />
          </Form.Item>
          <Form.Item
            name="base_command"
            label="基础命令"
            rules={[{ required: true }]}
          >
            <Input.TextArea rows={3} />
          </Form.Item>
          <Form.List name="args">
            {(fields, { add, remove }) => (
              <Form.Item label="运行参数">
                <Space direction="vertical" className="full-width" size="small">
                  {fields.map((field, index) => (
                    <Space.Compact key={field.key} className="full-width">
                      <Form.Item {...field} noStyle>
                        <Input
                          ref={(node) => {
                            runArgRefs.current[index] = node;
                          }}
                          placeholder={`参数 ${index + 1}`}
                        />
                      </Form.Item>
                      <Button
                        icon={<NumberOutlined />}
                        onMouseDown={(event) => event.preventDefault()}
                        onClick={() => insertRunArg(index, RANDOM_PLACEHOLDER)}
                      >
                        随机数
                      </Button>
                      <Button
                        icon={<MinusCircleOutlined />}
                        onClick={() => remove(field.name)}
                      />
                    </Space.Compact>
                  ))}
                  <Space.Compact className="full-width">
                    <Button type="dashed" onClick={() => add("")} block>
                      添加参数
                    </Button>
                    <Button
                      icon={<NumberOutlined />}
                      onClick={() => add(RANDOM_PLACEHOLDER)}
                    >
                      添加随机数
                    </Button>
                  </Space.Compact>
                </Space>
              </Form.Item>
            )}
          </Form.List>
          <Form.Item
            name="command_line"
            label="最终命令行"
            rules={[{ required: true }]}
          >
            <Input.TextArea rows={4} readOnly />
          </Form.Item>
          <Form.Item
            name="preview_path_template"
            label="预览文件路径模板"
            extra="例如命令参数中有 -o outputs/a-{{random:8}}.mp4，这里填写同样的输出路径即可按本次实际随机文件名预览。"
          >
            <Input placeholder={`outputs/result-${RANDOM_PLACEHOLDER}.png`} />
          </Form.Item>
          <Form.Item name="env_vars" label="环境变量 JSON">
            <Input.TextArea rows={3} />
          </Form.Item>
          <Form.Item name="python_venv_path" label="Python 虚拟环境路径">
            <Input />
          </Form.Item>
          <Form.Item name="setup_script" label="前置脚本">
            <Input.TextArea rows={4} />
          </Form.Item>
          <Form.Item name="timeout_seconds" label="超时时间（秒）">
            <Input type="number" min={1} />
          </Form.Item>
        </Form>
      </Modal>
      <Drawer
        title={`执行日志${detailRun ? ` #${detailRun.id}` : ""}`}
        open={Boolean(detailRunId)}
        onClose={() => setDetailRunId(null)}
        width={780}
      >
        {detailRun ? (
          <Space direction="vertical" size="middle" className="full-width">
            <Space wrap>
              <Tag color={statusColor[detailRun.status] ?? "default"}>
                {detailRun.status}
              </Tag>
              <Typography.Text>{detailRun.working_directory}</Typography.Text>
              <Typography.Text type="secondary">
                {detailRun.command_line}
              </Typography.Text>
              {isActiveRun(detailRun.status) ? (
                <PermissionButton
                  danger
                  icon={<StopOutlined />}
                  permission="system:command:cancel"
                  onClick={() => cancelMutation.mutate(detailRun.id)}
                >
                  取消
                </PermissionButton>
              ) : null}
              <PermissionButton
                icon={<PlayCircleOutlined />}
                permission="system:command:run"
                onClick={() => openRunModal(undefined, detailRun)}
              >
                编辑重跑
              </PermissionButton>
              {detailRun.preview_path ? (
                <Button
                  icon={<EyeOutlined />}
                  onClick={() => openPreview(detailRun)}
                >
                  预览产物
                </Button>
              ) : null}
            </Space>
            {detailRun.preview_path ? (
              <Alert
                type="info"
                showIcon
                message="预览文件"
                description={detailRun.preview_path}
              />
            ) : null}
            {detailRun.error_message ? (
              <Alert type="error" showIcon message={detailRun.error_message} />
            ) : null}
            <pre className="command-log-viewer">
              {logText || detailRun.output_tail || "暂无输出"}
            </pre>
          </Space>
        ) : null}
      </Drawer>
      <Drawer
        title={`编排详情${detailWorkflowRun ? ` #${detailWorkflowRun.id}` : ""}`}
        open={Boolean(detailWorkflowRunId)}
        onClose={() => setDetailWorkflowRunId(null)}
        width={780}
      >
        {detailWorkflowRun ? (
          <Space direction="vertical" size="middle" className="full-width">
            <Space wrap>
              <Tag color={statusColor[detailWorkflowRun.status] ?? "default"}>
                {detailWorkflowRun.status}
              </Tag>
              <Typography.Text>{detailWorkflowRun.name}</Typography.Text>
            </Space>
            {detailWorkflowRun.error_message ? (
              <Alert
                type="error"
                showIcon
                message={detailWorkflowRun.error_message}
              />
            ) : null}
            <DataTable
              rowKey="id"
              columns={[
                { title: "顺序", dataIndex: "sort_order", width: 80 },
                { title: "步骤", dataIndex: "step_name", width: 180 },
                {
                  title: "状态",
                  dataIndex: "status",
                  width: 110,
                  render: (value) => (
                    <Tag color={statusColor[String(value)] ?? "default"}>
                      {String(value)}
                    </Tag>
                  ),
                },
                {
                  title: "参数",
                  dataIndex: "resolved_args",
                  width: 220,
                  ellipsis: true,
                  render: (value) => value ?? "-",
                },
                {
                  title: "操作",
                  key: "actions",
                  width: 100,
                  fixed: "right",
                  render: (_, record) =>
                    record.command_run_id ? (
                      <Button
                        size="small"
                        onClick={() =>
                          openRunDetail(record.command_run_id as number)
                        }
                      >
                        日志
                      </Button>
                    ) : (
                      "-"
                    ),
                },
              ]}
              dataSource={detailWorkflowRun.steps}
              pagination={false}
            />
          </Space>
        ) : null}
      </Drawer>
      <Modal
        title={previewingRun ? `产物预览 #${previewingRun.id}` : "产物预览"}
        open={Boolean(previewingRun)}
        onCancel={closePreview}
        footer={null}
        width={960}
        className="material-preview-modal"
      >
        {previewingRun?.preview_path ? (
          <Typography.Paragraph
            copyable={{ text: previewingRun.preview_path }}
            type="secondary"
          >
            {previewingRun.preview_path}
          </Typography.Paragraph>
        ) : null}
        <div className="material-preview-body">{renderPreviewContent()}</div>
      </Modal>
    </CrudPage>
  );
}
