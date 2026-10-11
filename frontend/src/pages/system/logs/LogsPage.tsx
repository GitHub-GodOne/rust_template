import {
  DeleteOutlined,
  DownloadOutlined,
  FileSearchOutlined,
  SearchOutlined,
  SettingOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Button,
  DatePicker,
  Descriptions,
  Drawer,
  Input,
  Popconfirm,
  Select,
  Space,
  Tag,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import type { Dayjs } from "dayjs";
import dayjs from "dayjs";
import { useState } from "react";
import {
  type LogFilters,
  type LogRecord,
  deleteLog,
  exportLogs,
  fetchAuditSettings,
  fetchLogs,
} from "../../../api/admin/logs";
import { CrudPage } from "../../../components/admin/CrudPage";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { useAuthStore } from "../../../stores/auth";
import { AuditRulesDrawer } from "./AuditRulesDrawer";
import "./logs.css";

const levelColor: Record<string, string> = {
  info: "blue",
  warn: "orange",
  error: "red",
};

type TimeRange = [Dayjs | null, Dayjs | null] | null;

/** Functions, so "最近 1 小时" is computed when picked, not on page load. */
const rangePresets: { label: string; value: () => [Dayjs, Dayjs] }[] = [
  { label: "最近 1 小时", value: () => [dayjs().subtract(1, "hour"), dayjs()] },
  {
    label: "今天",
    value: () => [dayjs().startOf("day"), dayjs().endOf("day")],
  },
  {
    label: "昨天",
    value: () => [
      dayjs().subtract(1, "day").startOf("day"),
      dayjs().subtract(1, "day").endOf("day"),
    ],
  },
  { label: "最近 7 天", value: () => [dayjs().subtract(7, "day"), dayjs()] },
  { label: "最近 30 天", value: () => [dayjs().subtract(30, "day"), dayjs()] },
  {
    label: "本月",
    value: () => [dayjs().startOf("month"), dayjs().endOf("month")],
  },
];

const typeLabel: Record<string, string> = {
  request: "接口请求",
  login: "登录",
  operation: "操作",
  system: "系统",
  error: "异常",
};

export function LogsPage() {
  const [page, setPage] = useState(1);
  const [keyword, setKeyword] = useState("");
  const [level, setLevel] = useState<string>();
  const [logType, setLogType] = useState<string>();
  const [module, setModule] = useState<string>();
  const [range, setRange] = useState<TimeRange>(null);
  const filters: LogFilters = {
    keyword: keyword || undefined,
    level,
    log_type: logType,
    module,
    start_time: range?.[0]?.format(),
    end_time: range?.[1]?.format(),
  };
  const [detail, setDetail] = useState<LogRecord | null>(null);
  const [rulesOpen, setRulesOpen] = useState(false);
  const canConfigure = useAuthStore((state) =>
    state.hasPermission("system:log:config"),
  );
  const canPurge = useAuthStore((state) =>
    state.hasPermission("system:log:delete"),
  );
  const queryClient = useQueryClient();
  // Feature names for the module column and filter.
  const featuresQuery = useQuery({
    queryKey: ["audit-settings"],
    queryFn: fetchAuditSettings,
    staleTime: 60_000,
  });
  const featureLabel = Object.fromEntries(
    (featuresQuery.data?.features ?? []).map((feature) => [
      feature.key,
      feature.label,
    ]),
  );

  const logsQuery = useQuery({
    queryKey: ["admin-logs", page, filters],
    queryFn: () => fetchLogs({ ...filters, page, page_size: 10 }),
  });

  const exportMutation = useMutation({
    mutationFn: () => exportLogs(filters),
    onSuccess: (count) => {
      message.success(`已导出 ${count} 条日志`);
      queryClient.invalidateQueries({ queryKey: ["admin-logs"] });
    },
    onError: async (error) => {
      // Error bodies arrive as a Blob because of responseType "blob".
      const blob = (error as { response?: { data?: unknown } }).response?.data;
      let text = error instanceof Error ? error.message : "导出失败";
      if (blob instanceof Blob) {
        try {
          const body = JSON.parse(await blob.text());
          text = body.message ?? body.description ?? text;
        } catch {
          // keep the generic message
        }
      }
      message.error(text);
    },
  });

  const deleteMutation = useMutation({
    mutationFn: deleteLog,
    onSuccess: () => {
      message.success("日志已删除");
      queryClient.invalidateQueries({ queryKey: ["admin-logs"] });
    },
  });

  const columns: ColumnsType<LogRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    {
      title: "级别",
      dataIndex: "level",
      width: 100,
      render: (value) => (
        <Tag color={levelColor[value] ?? "default"}>{value}</Tag>
      ),
    },
    {
      title: "类型",
      dataIndex: "log_type",
      width: 100,
      render: (value) => typeLabel[value] ?? value,
    },
    {
      title: "功能",
      dataIndex: "module",
      width: 140,
      render: (value) => featureLabel[value] ?? value,
    },
    { title: "动作", dataIndex: "action", width: 160 },
    { title: "消息", dataIndex: "message", width: 260 },
    { title: "操作人", dataIndex: "operator", width: 180 },
    {
      title: "IP",
      dataIndex: "ip",
      width: 140,
      render: (value) => value ?? "-",
    },
    { title: "状态", dataIndex: "status", width: 100 },
    {
      title: "耗时",
      dataIndex: "duration_ms",
      width: 100,
      render: (value) => (value ? `${value} ms` : "-"),
    },
    { title: "时间", dataIndex: "created_at", width: 220 },
    {
      title: "操作",
      key: "actions",
      width: 170,
      render: (_, record) => (
        <Space>
          <Button size="small" onClick={() => setDetail(record)}>
            详情
          </Button>
          <Popconfirm
            title="确认删除这条日志？"
            onConfirm={() => deleteMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              icon={<DeleteOutlined />}
              permission="system:log:delete"
            >
              删除
            </PermissionButton>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <CrudPage
      className="logs-page"
      title="日志中心"
      subtitle="操作日志、登录日志、异常日志的统一入口"
      breadcrumb={["系统管理", "日志中心"]}
      icon={<FileSearchOutlined />}
      toolbar={
        <Space wrap>
          <Input
            allowClear
            prefix={<SearchOutlined />}
            placeholder="搜索消息、模块、操作人"
            value={keyword}
            onChange={(event) => {
              setPage(1);
              setKeyword(event.target.value);
            }}
            className="admin-search-input"
          />
          <Select
            allowClear
            placeholder="级别"
            value={level}
            onChange={(value) => {
              setPage(1);
              setLevel(value);
            }}
            options={[
              { value: "info", label: "信息" },
              { value: "warn", label: "警告" },
              { value: "error", label: "错误" },
            ]}
            className="admin-filter-select"
          />
          <Select
            allowClear
            placeholder="类型"
            value={logType}
            onChange={(value) => {
              setPage(1);
              setLogType(value);
            }}
            options={[
              { value: "request", label: "接口请求" },
              { value: "login", label: "登录" },
              { value: "operation", label: "操作" },
              { value: "system", label: "系统" },
              { value: "error", label: "异常" },
            ]}
            className="admin-filter-select"
          />
          <Select
            allowClear
            showSearch
            optionFilterProp="label"
            placeholder="功能"
            popupMatchSelectWidth={false}
            value={module}
            onChange={(value) => {
              setPage(1);
              setModule(value);
            }}
            options={(featuresQuery.data?.features ?? []).map((feature) => ({
              value: feature.key,
              label: feature.label,
            }))}
            className="admin-filter-select admin-filter-select-wide"
          />
          <DatePicker.RangePicker
            showTime={{ format: "HH:mm" }}
            format="YYYY-MM-DD HH:mm"
            allowEmpty={[true, true]}
            presets={rangePresets}
            placeholder={["开始时间", "结束时间"]}
            value={range}
            onChange={(value) => {
              setPage(1);
              setRange(value);
            }}
            className="logs-range-picker"
          />
          <PermissionButton
            permission="system:log:export"
            icon={<DownloadOutlined />}
            loading={exportMutation.isPending}
            onClick={() => exportMutation.mutate()}
          >
            导出
          </PermissionButton>
          <Button icon={<SettingOutlined />} onClick={() => setRulesOpen(true)}>
            记录设置
          </Button>
        </Space>
      }
    >
      <DataTable<LogRecord>
        columns={columns}
        dataSource={logsQuery.data?.items ?? []}
        loading={logsQuery.isLoading}
        pagination={{
          current: page,
          total: logsQuery.data?.total ?? 0,
          onChange: setPage,
        }}
      />
      <Drawer
        title="日志详情"
        open={Boolean(detail)}
        onClose={() => setDetail(null)}
        width={640}
      >
        {detail && (
          <Descriptions column={1} bordered size="small">
            <Descriptions.Item label="ID">{detail.id}</Descriptions.Item>
            <Descriptions.Item label="级别">{detail.level}</Descriptions.Item>
            <Descriptions.Item label="类型">
              {detail.log_type}
            </Descriptions.Item>
            <Descriptions.Item label="功能">
              {featureLabel[detail.module] ?? detail.module}
            </Descriptions.Item>
            <Descriptions.Item label="动作">{detail.action}</Descriptions.Item>
            <Descriptions.Item label="消息">{detail.message}</Descriptions.Item>
            <Descriptions.Item label="请求">
              {detail.method ?? "-"} {detail.path ?? "-"}
            </Descriptions.Item>
            <Descriptions.Item label="状态">
              {detail.status ?? "-"}
            </Descriptions.Item>
            <Descriptions.Item label="耗时">
              {detail.duration_ms !== null && detail.duration_ms !== undefined
                ? `${detail.duration_ms} ms`
                : "-"}
            </Descriptions.Item>
            <Descriptions.Item label="操作人">
              {detail.operator ?? "-"}
            </Descriptions.Item>
            <Descriptions.Item label="IP">{detail.ip ?? "-"}</Descriptions.Item>
            <Descriptions.Item label="请求内容">
              {detail.request_summary ? (
                <code className="audit-request-summary">
                  {detail.request_summary}
                </code>
              ) : (
                "-"
              )}
            </Descriptions.Item>
            <Descriptions.Item label="追踪 ID">
              {detail.trace_id ?? "-"}
            </Descriptions.Item>
            <Descriptions.Item label="User Agent">
              {detail.user_agent ?? "-"}
            </Descriptions.Item>
            <Descriptions.Item label="错误">
              {detail.error_message ?? "-"}
            </Descriptions.Item>
            <Descriptions.Item label="时间">
              {detail.created_at}
            </Descriptions.Item>
          </Descriptions>
        )}
      </Drawer>
      <AuditRulesDrawer
        open={rulesOpen}
        canEdit={canConfigure}
        canPurge={canPurge}
        onClose={() => setRulesOpen(false)}
      />
    </CrudPage>
  );
}
