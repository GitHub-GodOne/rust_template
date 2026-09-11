import {
  DeleteOutlined,
  FileSearchOutlined,
  SearchOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Button,
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
import { useState } from "react";
import { type LogRecord, deleteLog, fetchLogs } from "../../../api/admin/logs";
import { CrudPage } from "../../../components/admin/CrudPage";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";

const levelColor: Record<string, string> = {
  info: "blue",
  warn: "orange",
  error: "red",
};

const typeLabel: Record<string, string> = {
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
  const [detail, setDetail] = useState<LogRecord | null>(null);
  const queryClient = useQueryClient();

  const logsQuery = useQuery({
    queryKey: ["admin-logs", page, keyword, level, logType],
    queryFn: () =>
      fetchLogs({
        page,
        page_size: 10,
        keyword: keyword || undefined,
        level,
        log_type: logType,
      }),
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
    { title: "模块", dataIndex: "module", width: 140 },
    { title: "动作", dataIndex: "action", width: 160 },
    { title: "消息", dataIndex: "message", width: 260 },
    { title: "操作人", dataIndex: "operator", width: 180 },
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
              { value: "login", label: "登录" },
              { value: "operation", label: "操作" },
              { value: "system", label: "系统" },
              { value: "error", label: "异常" },
            ]}
            className="admin-filter-select"
          />
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
            <Descriptions.Item label="模块">{detail.module}</Descriptions.Item>
            <Descriptions.Item label="动作">{detail.action}</Descriptions.Item>
            <Descriptions.Item label="消息">{detail.message}</Descriptions.Item>
            <Descriptions.Item label="请求">
              {detail.method ?? "-"} {detail.path ?? "-"}
            </Descriptions.Item>
            <Descriptions.Item label="状态">
              {detail.status ?? "-"}
            </Descriptions.Item>
            <Descriptions.Item label="操作人">
              {detail.operator ?? "-"}
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
    </CrudPage>
  );
}
