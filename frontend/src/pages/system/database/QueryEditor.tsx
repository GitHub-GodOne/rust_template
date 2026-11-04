import {
  CaretRightOutlined,
  CheckCircleFilled,
  CheckOutlined,
  CloseCircleFilled,
  DownloadOutlined,
  HistoryOutlined,
  NodeIndexOutlined,
  RollbackOutlined,
  SaveOutlined,
  StopOutlined,
  ThunderboltOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery } from "@tanstack/react-query";
import {
  Alert,
  Button,
  Dropdown,
  Empty,
  Input,
  Modal,
  Select,
  Space,
  Tabs,
  Tag,
  Tooltip,
  Typography,
  message,
} from "antd";
import {
  type PointerEvent as ReactPointerEvent,
  useEffect,
  useRef,
  useState,
} from "react";
import {
  type DbExecuteResult,
  type DbSavedQuery,
  type DbType,
  cancelDbSql,
  closeDbSession,
  createDbSavedQuery,
  executeDbSql,
  fetchDbCompletion,
  updateDbSavedQuery,
} from "../../../api/admin/dbManager";
import { t } from "../../../i18n";
import { HistoryDrawer } from "./HistoryDrawer";
import { ResultGrid } from "./ResultGrid";
import { SqlEditor, type SqlEditorHandle } from "./SqlEditor";
import {
  downloadResultCsv,
  downloadResultJson,
  explainPrefix,
  formatDuration,
  newSessionKey,
  statementAt,
} from "./dbUtils";

const MAX_ROW_OPTIONS = [100, 500, 1000, 5000, 10000];
const TIMEOUT_OPTIONS = [30, 60, 300, 900, 3600];

type RunMode = "all" | "current" | "explain";

export function QueryEditor({
  connectionId,
  dbType,
  databases,
  initialDatabase,
  schema,
  initialSql,
  savedQuery: initialSavedQuery,
  readOnly,
  onTitleChange,
  onSaved,
}: {
  connectionId: number;
  dbType: DbType;
  databases: string[];
  initialDatabase: string;
  /** PostgreSQL schema used for completion. */
  schema?: string;
  initialSql?: string;
  savedQuery?: DbSavedQuery;
  readOnly: boolean;
  onTitleChange: (title: string) => void;
  onSaved: () => void;
}) {
  const editorRef = useRef<SqlEditorHandle>(null);
  const sessionKey = useRef(newSessionKey());
  const [database, setDatabase] = useState(initialDatabase);
  const [maxRows, setMaxRows] = useState(1000);
  const [timeoutSeconds, setTimeoutSeconds] = useState(60);
  const [result, setResult] = useState<DbExecuteResult | null>(null);
  const [requestError, setRequestError] = useState<string | null>(null);
  const [activeResult, setActiveResult] = useState("messages");
  const [inTransaction, setInTransaction] = useState(false);
  const [savedQuery, setSavedQuery] = useState(initialSavedQuery);
  const [saveOpen, setSaveOpen] = useState(false);
  const [saveName, setSaveName] = useState("");
  const [historyOpen, setHistoryOpen] = useState(false);
  const [editorHeight, setEditorHeight] = useState(240);
  const dragStart = useRef<{ y: number; height: number } | null>(null);

  // Each tab has its own server connection; close it with the tab.
  useEffect(() => {
    const key = sessionKey.current;
    return () => {
      void closeDbSession(connectionId, key).catch(() => undefined);
    };
  }, [connectionId]);

  const completion = useQuery({
    queryKey: ["db-completion", connectionId, database, schema],
    queryFn: () => fetchDbCompletion(connectionId, { database, schema }),
    staleTime: 60_000,
    retry: false,
  });

  const execute = useMutation({
    mutationFn: (sql: string) =>
      executeDbSql(connectionId, {
        database,
        session_key: sessionKey.current,
        sql,
        max_rows: maxRows,
        timeout_seconds: timeoutSeconds,
      }),
    onMutate: () => setRequestError(null),
    onSuccess: (data) => {
      setResult(data);
      setInTransaction(data.in_transaction);
      const firstSet = data.statements.findIndex(
        (statement) => statement.returns_rows && !statement.error,
      );
      const failed = data.statements.some((statement) => statement.error);
      setActiveResult(
        failed || firstSet < 0 ? "messages" : `result-${firstSet}`,
      );
    },
    onError: (error) => setRequestError(error.message),
  });

  const run = (mode: RunMode) => {
    const editor = editorRef.current;
    if (!editor || execute.isPending) return;
    const selection = editor.getSelection().trim();
    let sql = selection || editor.getValue();
    if (!selection && mode !== "all") {
      const statement = statementAt(sql, editor.getCursor(), dbType);
      if (!statement) return;
      editor.select(statement.from, statement.to);
      sql = statement.text;
    }
    if (mode === "explain") {
      sql = `${explainPrefix(dbType)}${sql.replace(/;\s*$/, "")}`;
    }
    if (!sql.trim()) {
      message.info(t("请输入 SQL"));
      return;
    }
    execute.mutate(sql);
  };

  const cancel = useMutation({
    mutationFn: () => cancelDbSql(connectionId, sessionKey.current),
    onSuccess: (cancelled) =>
      cancelled
        ? message.info(t("已请求取消"))
        : message.warning(t("没有正在执行的语句")),
    onError: (error) => message.error(error.message),
  });

  const saveQuery = useMutation({
    mutationFn: (name: string) => {
      const params = {
        connection_id: connectionId,
        name,
        database_name: database || null,
        sql_text: editorRef.current?.getValue() ?? "",
      };
      return savedQuery
        ? updateDbSavedQuery(savedQuery.id, params)
        : createDbSavedQuery(params);
    },
    onSuccess: (query) => {
      setSavedQuery(query);
      setSaveOpen(false);
      onTitleChange(query.name);
      onSaved();
      message.success(t("查询已保存"));
    },
    onError: (error) => message.error(error.message),
  });

  const requestSave = () => {
    if (!(editorRef.current?.getValue() ?? "").trim()) {
      message.info(t("请输入 SQL"));
      return;
    }
    if (savedQuery) {
      saveQuery.mutate(savedQuery.name);
    } else {
      setSaveName("");
      setSaveOpen(true);
    }
  };

  const changeDatabase = (next: string) => {
    if (!inTransaction) {
      setDatabase(next);
      return;
    }
    Modal.confirm({
      title: t("切换数据库会回滚当前事务"),
      content: t("该查询窗口有未提交的事务，切换后将断开原连接并回滚。"),
      okText: t("切换"),
      okButtonProps: { danger: true },
      onOk: () => {
        setDatabase(next);
        setInTransaction(false);
      },
    });
  };

  const onDragStart = (event: ReactPointerEvent) => {
    dragStart.current = { y: event.clientY, height: editorHeight };
    (event.target as HTMLElement).setPointerCapture(event.pointerId);
  };
  const onDragMove = (event: ReactPointerEvent) => {
    if (!dragStart.current) return;
    const next = dragStart.current.height + event.clientY - dragStart.current.y;
    setEditorHeight(Math.min(900, Math.max(100, next)));
  };

  const statements = result?.statements ?? [];
  // Result sets are numbered among themselves, not among all statements.
  const resultNumbers = new Map<number, number>();
  for (const [index, statement] of statements.entries()) {
    if (statement.returns_rows && !statement.error) {
      resultNumbers.set(index, resultNumbers.size + 1);
    }
  }
  const resultTabs = statements.flatMap((statement, index) =>
    statement.returns_rows && !statement.error
      ? [
          {
            key: `result-${index}`,
            label: t("结果 {0}", resultNumbers.get(index) ?? index + 1),
            children: (
              <div className="db-result-pane">
                <div className="db-result-toolbar">
                  <Typography.Text type="secondary">
                    {statement.truncated
                      ? t("显示前 {0} 行（结果已截断）", statement.rows.length)
                      : t("{0} 行", statement.rows.length)}
                    {" · "}
                    {formatDuration(statement.duration_ms)}
                  </Typography.Text>
                  <Dropdown
                    menu={{
                      items: [
                        {
                          key: "csv",
                          label: "CSV",
                          onClick: () =>
                            downloadResultCsv(
                              statement.columns.map((column) => column.name),
                              statement.rows,
                              `result-${resultNumbers.get(index) ?? index + 1}`,
                            ),
                        },
                        {
                          key: "json",
                          label: "JSON",
                          onClick: () =>
                            downloadResultJson(
                              statement.columns.map((column) => column.name),
                              statement.rows,
                              `result-${resultNumbers.get(index) ?? index + 1}`,
                            ),
                        },
                      ],
                    }}
                  >
                    <Button size="small" icon={<DownloadOutlined />}>
                      {t("导出结果")}
                    </Button>
                  </Dropdown>
                </div>
                <ResultGrid
                  columns={statement.columns.map((column) => column.name)}
                  typeNames={statement.columns.map(
                    (column) => column.type_name,
                  )}
                  rows={statement.rows}
                />
              </div>
            ),
          },
        ]
      : [],
  );

  const messagesTab = {
    key: "messages",
    label: t("信息"),
    children: (
      <div className="db-messages">
        {requestError ? (
          <Alert type="error" showIcon message={requestError} />
        ) : null}
        {result ? (
          <>
            <Typography.Text type="secondary">
              {t(
                "共 {0} 条语句，执行 {1} 条，用时 {2}",
                result.statement_count,
                statements.length,
                formatDuration(result.duration_ms),
              )}
            </Typography.Text>
            {statements.map((statement, index) => (
              <div
                // biome-ignore lint/suspicious/noArrayIndexKey: statements keep their order
                key={index}
                className={`db-message${statement.error ? " is-error" : ""}`}
              >
                <div className="db-message-head">
                  {statement.error ? (
                    <CloseCircleFilled className="db-message-error-icon" />
                  ) : (
                    <CheckCircleFilled className="db-message-ok-icon" />
                  )}
                  <code className="db-message-sql">{statement.sql}</code>
                </div>
                <div className="db-message-detail">
                  {statement.error ? (
                    <Typography.Text type="danger">
                      {statement.error}
                    </Typography.Text>
                  ) : statement.returns_rows ? (
                    t("返回 {0} 行", statement.rows.length)
                  ) : (
                    t("{0} 行受影响", statement.affected_rows)
                  )}
                  <span className="db-message-time">
                    {formatDuration(statement.duration_ms)}
                  </span>
                </div>
              </div>
            ))}
          </>
        ) : requestError ? null : (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={t(
              "按 Ctrl+Enter 执行当前语句，选中文本时只执行选中部分",
            )}
          />
        )}
      </div>
    ),
  };

  return (
    <div className="db-query-view">
      <div className="db-toolbar">
        <Space wrap size={6}>
          <Tooltip title={t("执行当前语句或选中部分（Ctrl+Enter）")}>
            <Button
              size="small"
              type="primary"
              icon={<CaretRightOutlined />}
              loading={execute.isPending}
              onClick={() => run("current")}
            >
              {t("运行")}
            </Button>
          </Tooltip>
          <Button
            size="small"
            icon={<ThunderboltOutlined />}
            disabled={execute.isPending}
            onClick={() => run("all")}
          >
            {t("运行全部")}
          </Button>
          <Button
            size="small"
            icon={<NodeIndexOutlined />}
            disabled={execute.isPending}
            onClick={() => run("explain")}
          >
            {t("解释")}
          </Button>
          {dbType !== "sqlite" ? (
            <Button
              size="small"
              danger
              icon={<StopOutlined />}
              disabled={!execute.isPending}
              loading={cancel.isPending}
              onClick={() => cancel.mutate()}
            >
              {t("停止")}
            </Button>
          ) : null}
          <Button
            size="small"
            icon={<SaveOutlined />}
            loading={saveQuery.isPending}
            onClick={requestSave}
          >
            {t("保存")}
          </Button>
          <Button
            size="small"
            icon={<HistoryOutlined />}
            onClick={() => setHistoryOpen(true)}
          >
            {t("历史")}
          </Button>
          {inTransaction ? (
            <>
              <Tag color="orange" className="db-tx-tag">
                {t("事务进行中")}
              </Tag>
              <Button
                size="small"
                icon={<CheckOutlined />}
                disabled={execute.isPending}
                onClick={() => execute.mutate("COMMIT")}
              >
                {t("提交")}
              </Button>
              <Button
                size="small"
                icon={<RollbackOutlined />}
                disabled={execute.isPending}
                onClick={() => execute.mutate("ROLLBACK")}
              >
                {t("回滚")}
              </Button>
            </>
          ) : null}
        </Space>
        <Space wrap size={6}>
          {dbType !== "sqlite" ? (
            <Select
              size="small"
              showSearch
              className="db-database-select"
              value={database}
              onChange={changeDatabase}
              options={databases.map((name) => ({ value: name, label: name }))}
            />
          ) : null}
          <Tooltip title={t("每个结果集最多返回的行数")}>
            <Select
              size="small"
              value={maxRows}
              onChange={setMaxRows}
              options={MAX_ROW_OPTIONS.map((value) => ({
                value,
                label: t("{0} 行", value),
              }))}
            />
          </Tooltip>
          <Tooltip title={t("单条语句超时")}>
            <Select
              size="small"
              value={timeoutSeconds}
              onChange={setTimeoutSeconds}
              options={TIMEOUT_OPTIONS.map((value) => ({
                value,
                label:
                  value >= 60 ? t("{0} 分钟", value / 60) : t("{0} 秒", value),
              }))}
            />
          </Tooltip>
        </Space>
      </div>
      {readOnly ? (
        <Alert
          type="warning"
          showIcon
          className="db-inline-alert"
          message={t("只读连接：写操作会被数据库拒绝")}
        />
      ) : null}
      <div className="db-editor-pane" style={{ height: editorHeight }}>
        <SqlEditor
          handleRef={editorRef}
          initialValue={initialSql ?? savedQuery?.sql_text ?? ""}
          dbType={dbType}
          tables={completion.data}
          placeholderText={t("在这里输入 SQL，Ctrl+Enter 执行")}
          onRun={() => run("current")}
          onSave={requestSave}
        />
      </div>
      <div
        className="db-splitter"
        role="separator"
        aria-orientation="horizontal"
        aria-label={t("调整编辑器高度")}
        aria-valuenow={editorHeight}
        aria-valuemin={100}
        aria-valuemax={900}
        tabIndex={0}
        onKeyDown={(event) => {
          if (event.key === "ArrowUp" || event.key === "ArrowDown") {
            event.preventDefault();
            const step = event.key === "ArrowUp" ? -24 : 24;
            setEditorHeight((height) =>
              Math.min(900, Math.max(100, height + step)),
            );
          }
        }}
        onPointerDown={onDragStart}
        onPointerMove={onDragMove}
        onPointerUp={() => {
          dragStart.current = null;
        }}
      />
      <Tabs
        size="small"
        className="db-results"
        activeKey={activeResult}
        onChange={setActiveResult}
        items={[messagesTab, ...resultTabs]}
      />

      <Modal
        open={saveOpen}
        title={t("保存查询")}
        onCancel={() => setSaveOpen(false)}
        okText={t("保存")}
        confirmLoading={saveQuery.isPending}
        okButtonProps={{ disabled: !saveName.trim() }}
        onOk={() => saveQuery.mutate(saveName.trim())}
        destroyOnClose
      >
        <Input
          autoFocus
          maxLength={100}
          value={saveName}
          placeholder={t("查询名称")}
          onChange={(event) => setSaveName(event.target.value)}
          onPressEnter={() => {
            if (saveName.trim()) saveQuery.mutate(saveName.trim());
          }}
        />
      </Modal>
      <HistoryDrawer
        open={historyOpen}
        connectionId={connectionId}
        onClose={() => setHistoryOpen(false)}
        onUse={(sql) => {
          editorRef.current?.setValue(sql);
          editorRef.current?.focus();
          setHistoryOpen(false);
        }}
      />
    </div>
  );
}
