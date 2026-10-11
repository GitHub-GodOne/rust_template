import {
  ArrowLeftOutlined,
  BuildOutlined,
  CaretRightOutlined,
  ClockCircleOutlined,
  CopyOutlined,
  HistoryOutlined,
  ReloadOutlined,
  StopOutlined,
  WarningOutlined,
} from "@ant-design/icons";
import {
  keepPreviousData,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import {
  Alert,
  AutoComplete,
  Badge,
  Button,
  Drawer,
  Empty,
  Grid,
  Input,
  Modal,
  Pagination,
  Segmented,
  Select,
  Spin,
  Switch,
  Tag,
  Tooltip,
  Typography,
  message,
} from "antd";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useSearchParams } from "react-router-dom";
import {
  type CliArgSpec,
  type CliCommandSpec,
  type CliRisk,
  type ProjectCliOutputChunk,
  type ProjectCliRunPage,
  type ProjectCliRunStatus,
  type ProjectCliRunSummary,
  cancelProjectCliRun,
  fetchProjectCliCatalog,
  fetchProjectCliRun,
  fetchProjectCliRuns,
  runProjectCli,
} from "../../../api/admin/projectCli";
import { PageHeader } from "../../../components/admin/PageHeader";
import { useAuthStore } from "../../../stores/auth";
import {
  type CliValues,
  argLabel,
  buildArgs,
  commandKey,
  formatCommandLine,
  formatDuration,
  groupCommands,
  initialValues,
  missingRequired,
  payloadValues,
  splitArgs,
  stripAnsi,
} from "./projectCliModel";
import "./project-cli.css";

const RUN_PERMISSION = "system:project_cli:run";
const POLL_MS = 700;
const MAX_CHUNKS = 4000;
const HISTORY_PAGE_SIZE = 20;
const ENVIRONMENTS = ["development", "production", "test"];

const RISK_TAGS: Record<CliRisk, { color: string; label: string } | null> = {
  safe: null,
  caution: { color: "orange", label: "谨慎" },
  danger: { color: "red", label: "危险" },
};

const STATUS_TAGS: Record<
  ProjectCliRunStatus,
  { color: string; label: string }
> = {
  running: { color: "processing", label: "运行中" },
  success: { color: "success", label: "成功" },
  failed: { color: "error", label: "失败" },
  cancelled: { color: "default", label: "已停止" },
};

function errorText(error: unknown) {
  return error instanceof Error ? error.message : "操作失败";
}

/** Streams a run's output by polling from the last sequence number. */
function useRunOutput(runId?: number) {
  const [run, setRun] = useState<ProjectCliRunSummary>();
  const [chunks, setChunks] = useState<ProjectCliOutputChunk[]>([]);
  const [error, setError] = useState<string>();

  useEffect(() => {
    setRun(undefined);
    setChunks([]);
    setError(undefined);
    if (!runId) return;
    let stopped = false;
    let after = 0;
    let timer: number | undefined;
    const tick = async () => {
      try {
        const detail = await fetchProjectCliRun(runId, after);
        if (stopped) return;
        const last = detail.output[detail.output.length - 1];
        if (last) after = last.seq;
        setRun(detail.run);
        if (detail.output.length > 0) {
          setChunks((previous) =>
            [...previous, ...detail.output].slice(-MAX_CHUNKS),
          );
        }
        if (detail.run.status === "running") {
          timer = window.setTimeout(tick, POLL_MS);
        }
      } catch (caught) {
        if (!stopped) setError(errorText(caught));
      }
    };
    void tick();
    return () => {
      stopped = true;
      window.clearTimeout(timer);
    };
  }, [runId]);

  return { run, chunks, error };
}

export function ProjectCliPage() {
  const screens = Grid.useBreakpoint();
  const isMobile = screens.md === false;
  const hasPermission = useAuthStore((state) => state.hasPermission);
  const canRun = hasPermission(RUN_PERMISSION);
  const queryClient = useQueryClient();
  const [searchParams, setSearchParams] = useSearchParams();
  const [keyword, setKeyword] = useState("");
  const [historyOpen, setHistoryOpen] = useState(false);
  const [activeRunId, setActiveRunId] = useState<number>();
  const workspaceRef = useRef<HTMLDivElement>(null);

  const catalogQuery = useQuery({
    queryKey: ["project-cli", "catalog"],
    queryFn: () => fetchProjectCliCatalog(),
    staleTime: Number.POSITIVE_INFINITY,
  });
  const catalog = catalogQuery.data;
  const [historyPage, setHistoryPage] = useState(1);
  const pollWhileRunning = (data?: ProjectCliRunPage) =>
    data?.items.some((run) => run.status === "running") ? 2000 : false;
  // The newest page drives the "running" badge; the drawer can page back.
  const latestRunsQuery = useQuery({
    queryKey: ["project-cli", "runs", 1],
    queryFn: () => fetchProjectCliRuns(1, HISTORY_PAGE_SIZE),
    refetchInterval: (query) => pollWhileRunning(query.state.data),
  });
  const runsQuery = useQuery({
    queryKey: ["project-cli", "runs", historyPage],
    queryFn: () => fetchProjectCliRuns(historyPage, HISTORY_PAGE_SIZE),
    enabled: historyOpen,
    placeholderData: keepPreviousData,
    refetchInterval: (query) => pollWhileRunning(query.state.data),
  });
  const runningCount =
    latestRunsQuery.data?.items.filter((run) => run.status === "running")
      .length ?? 0;

  const refresh = useMutation({
    mutationFn: () => fetchProjectCliCatalog(true),
    onSuccess: (data) => {
      queryClient.setQueryData(["project-cli", "catalog"], data);
      message.success(`已重新读取 ${data.commands.length} 条命令`);
    },
    onError: (error) => message.error(errorText(error)),
  });

  const sections = useMemo(
    () => groupCommands(catalog?.commands ?? [], keyword),
    [catalog?.commands, keyword],
  );
  const selectedKey = searchParams.get("cmd") ?? undefined;
  const selected = catalog?.commands.find(
    (command) => commandKey(command.path) === selectedKey,
  );
  const select = (command?: CliCommandSpec) => {
    const next = new URLSearchParams(searchParams);
    if (command) next.set("cmd", commandKey(command.path));
    else next.delete("cmd");
    setSearchParams(next, { replace: !isMobile });
  };

  // Desktop opens on the first runnable command instead of an empty pane.
  // biome-ignore lint/correctness/useExhaustiveDependencies: only on first load
  useEffect(() => {
    if (isMobile || selectedKey || !catalog) return;
    const first = catalog.commands.find((command) => command.runnable);
    if (first) select(first);
  }, [catalog, isMobile]);

  // The workspace fills the window below the admin header.
  useLayoutEffect(() => {
    const element = workspaceRef.current;
    if (!element) return;
    const measure = () => {
      const top = element.getBoundingClientRect().top + window.scrollY;
      element.style.setProperty("--pcli-top", `${Math.round(top)}px`);
    };
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, []);

  const openRun = (run: ProjectCliRunSummary) => {
    setActiveRunId(run.id);
    const command = catalog?.commands.find(
      (entry) => commandKey(entry.path) === commandKey(run.path),
    );
    if (command) select(command);
    setHistoryOpen(false);
  };

  const showList = !isMobile || !selected;
  const showDetail = !isMobile || Boolean(selected);

  return (
    <div className="pcli-page">
      <PageHeader
        title="项目命令"
        subtitle="项目命令行一览：查看用法、拼好参数后直接在服务器上执行"
        icon={<BuildOutlined />}
        breadcrumb={["系统管理", "项目命令"]}
        extra={
          <div className="pcli-header-actions">
            {catalog && !isMobile ? (
              <Tooltip title="命令默认在这个环境下执行">
                <Tag className="pcli-env-tag">环境 {catalog.environment}</Tag>
              </Tooltip>
            ) : null}
            <Badge count={runningCount} size="small">
              <Button
                icon={<HistoryOutlined />}
                onClick={() => setHistoryOpen(true)}
                aria-label="运行记录"
              >
                {isMobile ? null : "运行记录"}
              </Button>
            </Badge>
            <Tooltip title="重新读取全部命令和参数">
              <Button
                icon={<ReloadOutlined />}
                loading={refresh.isPending}
                onClick={() => refresh.mutate()}
                aria-label="刷新命令"
              />
            </Tooltip>
          </div>
        }
      />

      {catalog?.warnings.length ? (
        <Alert
          className="pcli-alert"
          type="warning"
          showIcon
          message="部分命令读取失败"
          description={catalog.warnings.join("；")}
        />
      ) : null}
      {catalogQuery.isError ? (
        <Alert
          className="pcli-alert"
          type="error"
          showIcon
          message={errorText(catalogQuery.error)}
        />
      ) : null}

      <div className="pcli-workspace" ref={workspaceRef}>
        {showList ? (
          <aside className="pcli-list" aria-label="命令列表">
            <div className="pcli-list-search">
              <Input.Search
                allowClear
                placeholder="搜索命令、参数或说明"
                value={keyword}
                onChange={(event) => setKeyword(event.target.value)}
              />
            </div>
            <div className="pcli-list-scroll">
              {catalogQuery.isLoading ? (
                <div className="pcli-list-loading">
                  <Spin />
                </div>
              ) : sections.length === 0 ? (
                <Empty
                  image={Empty.PRESENTED_IMAGE_SIMPLE}
                  description="没有匹配的命令"
                />
              ) : (
                sections.map((section) => (
                  <section key={section.source} className="pcli-section">
                    <div className="pcli-section-title">
                      {section.title}
                      <span>{section.commands.length}</span>
                    </div>
                    {section.commands.map(({ command, depth }) => (
                      <CommandRow
                        key={commandKey(command.path)}
                        command={command}
                        depth={depth}
                        active={selected === command}
                        onSelect={() => select(command)}
                      />
                    ))}
                  </section>
                ))
              )}
            </div>
            {catalog ? (
              <div className="pcli-list-footer">
                共 {catalog.commands.length} 条命令 · 自动读取自{" "}
                <code>{catalog.binary_name}</code>
              </div>
            ) : null}
          </aside>
        ) : null}

        {showDetail ? (
          <section className="pcli-detail">
            {selected && catalog ? (
              <CommandPanel
                key={commandKey(selected.path)}
                command={selected}
                environment={catalog.environment}
                binaryName={catalog.binary_name}
                cargoProject={catalog.cargo_project}
                canRun={canRun}
                isMobile={isMobile}
                onBack={() => select(undefined)}
                onStarted={(run) => {
                  setActiveRunId(run.id);
                  void queryClient.invalidateQueries({
                    queryKey: ["project-cli", "runs"],
                  });
                }}
              />
            ) : (
              <div className="pcli-detail-empty">
                <Empty description="从左侧选择一条命令" />
              </div>
            )}
            <RunConsole
              runId={activeRunId}
              canRun={canRun}
              onClose={() => setActiveRunId(undefined)}
              onChanged={() =>
                void queryClient.invalidateQueries({
                  queryKey: ["project-cli", "runs"],
                })
              }
            />
          </section>
        ) : null}
      </div>

      <Drawer
        title="运行记录"
        open={historyOpen}
        onClose={() => setHistoryOpen(false)}
        width={isMobile ? "100%" : 520}
        className="pcli-history"
      >
        <Typography.Paragraph type="secondary" className="pcli-history-tip">
          运行记录和输出保存在数据库中，保留最近 500 次；单次输出超过 2 MB
          时只保留最新部分。
        </Typography.Paragraph>
        {runsQuery.data?.items.length ? (
          <ul className="pcli-history-list">
            {runsQuery.data.items.map((run) => (
              <li key={run.id}>
                <button
                  type="button"
                  className={`pcli-history-item${run.id === activeRunId ? " is-active" : ""}`}
                  onClick={() => openRun(run)}
                >
                  <span className="pcli-history-line">
                    <code>{run.command_line}</code>
                  </span>
                  <span className="pcli-history-meta">
                    <Tag color={STATUS_TAGS[run.status].color}>
                      {STATUS_TAGS[run.status].label}
                    </Tag>
                    <span>{new Date(run.started_at).toLocaleString()}</span>
                    {run.created_by_name ? (
                      <span>{run.created_by_name}</span>
                    ) : null}
                    {run.duration_ms !== null &&
                    run.duration_ms !== undefined ? (
                      <span>{formatDuration(run.duration_ms)}</span>
                    ) : null}
                  </span>
                  {run.error ? (
                    <span className="pcli-history-error">{run.error}</span>
                  ) : null}
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description="还没有运行过命令"
          />
        )}
        {(runsQuery.data?.total ?? 0) > HISTORY_PAGE_SIZE ? (
          <Pagination
            className="pcli-history-pages"
            size="small"
            simple
            current={historyPage}
            pageSize={HISTORY_PAGE_SIZE}
            total={runsQuery.data?.total ?? 0}
            onChange={setHistoryPage}
          />
        ) : null}
      </Drawer>
    </div>
  );
}

function CommandRow({
  command,
  depth,
  active,
  onSelect,
}: {
  command: CliCommandSpec;
  depth: number;
  active: boolean;
  onSelect: () => void;
}) {
  const risk = RISK_TAGS[command.risk];
  const name =
    command.source === "task"
      ? command.path.slice(1).join(" ")
      : command.path[command.path.length - 1];
  return (
    <button
      type="button"
      className={`pcli-row${active ? " is-active" : ""}${command.runnable ? "" : " is-group"}`}
      style={{ paddingLeft: 12 + depth * 16 }}
      onClick={onSelect}
      aria-current={active ? "true" : undefined}
    >
      <span className="pcli-row-main">
        <code className="pcli-row-name">{name}</code>
        {risk ? (
          <span
            className={`pcli-risk-dot is-${command.risk}`}
            title={risk.label}
          />
        ) : null}
        {command.long_running ? (
          <ClockCircleOutlined className="pcli-row-long" title="持续运行" />
        ) : null}
      </span>
      <span className="pcli-row-desc">{command.note ?? command.about}</span>
    </button>
  );
}

function ArgField({
  arg,
  value,
  onChange,
}: {
  arg: CliArgSpec;
  value: CliValues[string];
  onChange: (value: CliValues[string]) => void;
}) {
  const placeholder = arg.default_value
    ? `默认 ${arg.default_value}`
    : (arg.value_name ?? undefined);
  const list = Array.isArray(value)
    ? value
    : typeof value === "string" && value
      ? [value]
      : [];

  if (arg.kind === "flag") {
    return <Switch checked={value === true} onChange={onChange} />;
  }
  if (arg.kind === "option" && arg.optional_value) {
    const enabled = value !== undefined && value !== false;
    return (
      <div className="pcli-optional-value">
        <Switch
          checked={enabled}
          onChange={(on) => onChange(on ? true : undefined)}
        />
        {enabled ? (
          <Select
            mode="tags"
            className="pcli-field"
            placeholder="可选：填写值，留空表示不带值"
            value={list}
            open={false}
            suffixIcon={null}
            onChange={(next: string[]) =>
              onChange(next.length > 0 ? next : true)
            }
          />
        ) : null}
      </div>
    );
  }
  if (arg.possible_values.length > 0) {
    return (
      <Select
        allowClear
        className="pcli-field"
        mode={arg.multiple ? "multiple" : undefined}
        placeholder={placeholder}
        value={arg.multiple ? list : (list[0] ?? undefined)}
        options={arg.possible_values.map((option) => ({
          value: option,
          label: option,
        }))}
        onChange={(next) => onChange(next ?? undefined)}
      />
    );
  }
  if (arg.multiple) {
    return (
      <Select
        mode="tags"
        className="pcli-field"
        placeholder={`${placeholder ?? "值"}，回车添加多个`}
        value={list}
        open={false}
        suffixIcon={null}
        onChange={(next: string[]) => onChange(next)}
      />
    );
  }
  if (arg.long === "environment") {
    return (
      <AutoComplete
        className="pcli-field"
        value={list[0] ?? ""}
        options={ENVIRONMENTS.map((option) => ({ value: option }))}
        onChange={(next) => onChange(next)}
        placeholder={placeholder}
      />
    );
  }
  if (arg.secret) {
    return (
      <Input.Password
        className="pcli-field"
        autoComplete="new-password"
        value={list[0] ?? ""}
        placeholder={placeholder}
        onChange={(event) => onChange(event.target.value)}
      />
    );
  }
  return (
    <Input
      className="pcli-field"
      value={list[0] ?? ""}
      placeholder={placeholder}
      onChange={(event) => onChange(event.target.value)}
    />
  );
}

function CommandPanel({
  command,
  environment,
  binaryName,
  cargoProject,
  canRun,
  isMobile,
  onBack,
  onStarted,
}: {
  command: CliCommandSpec;
  environment: string;
  binaryName: string;
  cargoProject: boolean;
  canRun: boolean;
  isMobile: boolean;
  onBack: () => void;
  onStarted: (run: ProjectCliRunSummary) => void;
}) {
  const [values, setValues] = useState<CliValues>(() =>
    initialValues(command, environment),
  );
  const [extra, setExtra] = useState("");
  const [program, setProgram] = useState<"cargo" | "binary">(
    cargoProject ? "cargo" : "binary",
  );
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [confirmText, setConfirmText] = useState("");
  const key = commandKey(command.path);
  const risk = RISK_TAGS[command.risk];
  const extraArgs = splitArgs(extra);
  const preview = formatCommandLine(
    program === "cargo" ? "cargo loco" : `./${binaryName}`,
    buildArgs(command, values, extraArgs),
  );
  const missing = missingRequired(command, values);

  const run = useMutation({
    mutationFn: () =>
      runProjectCli({
        path: command.path,
        values: payloadValues(command, values),
        extra_args: extraArgs,
      }),
    onSuccess: (summary) => {
      setConfirmOpen(false);
      onStarted(summary);
    },
    onError: (error) => message.error(errorText(error)),
  });

  const start = () => {
    if (command.risk === "safe") {
      run.mutate();
      return;
    }
    setConfirmText("");
    setConfirmOpen(true);
  };

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(preview);
      message.success("命令已复制");
    } catch {
      message.error("复制失败，请手动选择命令文本");
    }
  };

  const options = command.args.filter((arg) => arg.kind !== "task_param");
  const params = command.args.filter((arg) => arg.kind === "task_param");
  const renderArg = (arg: CliArgSpec) => (
    <div key={arg.id} className={`pcli-arg is-${arg.kind}`}>
      <div className="pcli-arg-head">
        <code className="pcli-arg-label">{argLabel(arg)}</code>
        {arg.required ? <span className="pcli-arg-required">必填</span> : null}
      </div>
      {arg.help ? <div className="pcli-arg-help">{arg.help}</div> : null}
      <ArgField
        arg={arg}
        value={values[arg.id]}
        onChange={(value) =>
          setValues((current) => ({ ...current, [arg.id]: value }))
        }
      />
    </div>
  );

  return (
    <div className="pcli-panel">
      <div className="pcli-panel-scroll">
        <header className="pcli-panel-head">
          {isMobile ? (
            <Button
              type="text"
              icon={<ArrowLeftOutlined />}
              onClick={onBack}
              aria-label="返回命令列表"
            />
          ) : null}
          <div className="pcli-panel-title">
            <code>{key}</code>
            {risk ? <Tag color={risk.color}>{risk.label}</Tag> : null}
            {command.long_running ? (
              <Tag icon={<ClockCircleOutlined />}>持续运行</Tag>
            ) : null}
          </div>
        </header>
        {command.note ? (
          <p className="pcli-panel-note">{command.note}</p>
        ) : null}
        {command.about && command.about !== command.note ? (
          <p className="pcli-panel-about">{command.about}</p>
        ) : null}

        {command.runnable ? (
          <>
            {params.length > 0 ? (
              <div className="pcli-args">
                <div className="pcli-args-title">任务参数</div>
                {params.map(renderArg)}
              </div>
            ) : null}
            {options.length > 0 ? (
              <div className="pcli-args">
                <div className="pcli-args-title">选项</div>
                {options.map(renderArg)}
              </div>
            ) : null}
            <div className="pcli-args">
              <div className="pcli-args-title">附加参数</div>
              <Input
                className="pcli-field"
                value={extra}
                placeholder="可选，原样追加到命令末尾，例如 --help"
                onChange={(event) => setExtra(event.target.value)}
              />
            </div>

            <div className="pcli-preview">
              <div className="pcli-preview-bar">
                {cargoProject ? (
                  <Segmented
                    size="small"
                    value={program}
                    onChange={(next) => setProgram(next as "cargo" | "binary")}
                    options={[
                      { value: "cargo", label: "cargo loco" },
                      { value: "binary", label: "可执行文件" },
                    ]}
                  />
                ) : (
                  <span>命令</span>
                )}
                <Button
                  type="text"
                  size="small"
                  icon={<CopyOutlined />}
                  onClick={() => void copy()}
                >
                  复制
                </Button>
              </div>
              <code className="pcli-preview-line">{preview}</code>
            </div>

            <div className="pcli-actions">
              {canRun ? (
                <Tooltip
                  title={
                    missing.length ? `请先填写 ${missing.join("、")}` : null
                  }
                >
                  <Button
                    type="primary"
                    danger={command.risk === "danger"}
                    icon={<CaretRightOutlined />}
                    disabled={missing.length > 0}
                    loading={run.isPending}
                    onClick={start}
                  >
                    在服务器上执行
                  </Button>
                </Tooltip>
              ) : (
                <Typography.Text type="secondary">
                  没有执行权限，可以复制命令到终端运行
                </Typography.Text>
              )}
              {command.long_running ? (
                <Typography.Text type="secondary" className="pcli-actions-hint">
                  会一直运行，可在下方随时停止
                </Typography.Text>
              ) : null}
            </div>
          </>
        ) : (
          <Alert
            type="info"
            showIcon
            className="pcli-group-hint"
            message="这是一个命令组，请在左侧选择具体的子命令"
          />
        )}
      </div>

      <Modal
        open={confirmOpen}
        title={
          <span className="pcli-confirm-title">
            <WarningOutlined />
            确认执行{command.risk === "danger" ? "危险" : ""}命令
          </span>
        }
        okText="执行"
        okButtonProps={{
          danger: command.risk === "danger",
          disabled: command.risk === "danger" && confirmText.trim() !== key,
          loading: run.isPending,
        }}
        cancelText="取消"
        onOk={() => run.mutate()}
        onCancel={() => setConfirmOpen(false)}
      >
        <p>{command.note ?? command.about}</p>
        <code className="pcli-preview-line">{preview}</code>
        {command.risk === "danger" ? (
          <div className="pcli-confirm-input">
            <span>
              请输入 <code>{key}</code> 确认：
            </span>
            <Input
              autoFocus
              value={confirmText}
              onChange={(event) => setConfirmText(event.target.value)}
              onPressEnter={() => {
                if (confirmText.trim() === key) run.mutate();
              }}
            />
          </div>
        ) : null}
      </Modal>
    </div>
  );
}

function RunConsole({
  runId,
  canRun,
  onClose,
  onChanged,
}: {
  runId?: number;
  canRun: boolean;
  onClose: () => void;
  onChanged: () => void;
}) {
  const { run, chunks, error } = useRunOutput(runId);
  const scrollRef = useRef<HTMLPreElement>(null);
  const consoleRef = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);

  // On phones the console sits below the form; bring it into view.
  useEffect(() => {
    if (runId) {
      stickToBottom.current = true;
      consoleRef.current?.scrollIntoView({
        behavior: "smooth",
        block: "nearest",
      });
    }
  }, [runId]);
  const cancel = useMutation({
    mutationFn: (id: number) => cancelProjectCliRun(id),
    onSuccess: onChanged,
    onError: (caught) => message.error(errorText(caught)),
  });

  // Follow new output unless the user scrolled up to read.
  // biome-ignore lint/correctness/useExhaustiveDependencies: scroll when output grows
  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (element && stickToBottom.current) {
      element.scrollTop = element.scrollHeight;
    }
  }, [chunks]);

  const previousStatus = useRef<ProjectCliRunStatus>();
  useEffect(() => {
    if (previousStatus.current === "running" && run?.status !== "running") {
      onChanged();
    }
    previousStatus.current = run?.status;
  }, [onChanged, run?.status]);

  if (!runId) return null;
  const status = run ? STATUS_TAGS[run.status] : null;

  return (
    <div className="pcli-console" ref={consoleRef}>
      <div className="pcli-console-bar">
        {status ? (
          <Tag color={status.color}>{status.label}</Tag>
        ) : (
          <Spin size="small" />
        )}
        <code className="pcli-console-command" title={run?.command_line}>
          {run?.command_line}
        </code>
        <span className="pcli-console-meta">
          {run?.exit_code !== undefined && run?.exit_code !== null
            ? `退出码 ${run.exit_code}`
            : null}
          {run?.duration_ms ? ` · ${formatDuration(run.duration_ms)}` : null}
        </span>
        {run?.status === "running" && canRun ? (
          <Button
            size="small"
            danger
            icon={<StopOutlined />}
            loading={cancel.isPending}
            onClick={() => cancel.mutate(run.id)}
          >
            停止
          </Button>
        ) : null}
        <Button
          size="small"
          type="text"
          className="pcli-console-close"
          onClick={onClose}
        >
          收起
        </Button>
      </div>
      <pre
        ref={scrollRef}
        className="pcli-console-output"
        onScroll={(event) => {
          const element = event.currentTarget;
          stickToBottom.current =
            element.scrollHeight - element.scrollTop - element.clientHeight <
            24;
        }}
      >
        {run?.truncated ? (
          <span className="is-system">（较早的输出已省略）{"\n"}</span>
        ) : null}
        {chunks.map((chunk) => (
          <span key={chunk.seq} className={`is-${chunk.stream}`}>
            {stripAnsi(chunk.text)}
          </span>
        ))}
        {error ? <span className="is-stderr">{error}</span> : null}
      </pre>
    </div>
  );
}
