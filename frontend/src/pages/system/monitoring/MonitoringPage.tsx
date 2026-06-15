import {
  ClockCircleOutlined,
  CloudServerOutlined,
  DashboardOutlined,
  DeleteOutlined,
  EditOutlined,
  HddOutlined,
  MonitorOutlined,
  PlusOutlined,
  ReloadOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Button,
  Card,
  Col,
  Descriptions,
  Empty,
  Form,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Progress,
  Row,
  Select,
  Space,
  Switch,
  Tabs,
  Tag,
  Typography,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useState } from "react";
import {
  type DiskInfo,
  type GpuInfo,
  type NetworkInfo,
  type ProcessRecord,
  fetchMonitorProcesses,
  fetchMonitoringOverview,
} from "../../../api/admin/monitoring";
import {
  type RateLimitEventRecord,
  type RateLimitRuleRecord,
  type SaveRateLimitRuleParams,
  createRateLimitRule,
  deleteRateLimitRule,
  fetchRateLimitEvents,
  fetchRateLimitRules,
  updateRateLimitRule,
} from "../../../api/admin/rateLimits";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { StatusTag } from "../../../components/admin/StatusTag";
import { WidgetCard } from "../../../components/admin/WidgetCard";
import { formatBytes } from "../uploads/uploadTaskManager";

function formatPercent(value?: number | null) {
  return Number(value ?? 0).toFixed(1);
}

function progressStatus(value: number) {
  if (value >= 90) {
    return "exception";
  }
  if (value >= 75) {
    return "active";
  }
  return "normal";
}

function formatDuration(seconds?: number | null) {
  const total = Math.max(0, Math.floor(seconds ?? 0));
  const days = Math.floor(total / 86_400);
  const hours = Math.floor((total % 86_400) / 3_600);
  const minutes = Math.floor((total % 3_600) / 60);
  if (days > 0) {
    return `${days} 天 ${hours} 小时`;
  }
  if (hours > 0) {
    return `${hours} 小时 ${minutes} 分钟`;
  }
  return `${minutes} 分钟`;
}

function formatTimestamp(seconds?: number | null) {
  if (!seconds) {
    return "-";
  }
  return new Date(seconds * 1000).toLocaleString();
}

function diskColumns(): ColumnsType<DiskInfo> {
  return [
    { title: "名称", dataIndex: "name", width: 160 },
    { title: "挂载点", dataIndex: "mount_point", width: 220 },
    { title: "文件系统", dataIndex: "file_system", width: 140 },
    {
      title: "容量",
      dataIndex: "total_bytes",
      width: 130,
      render: formatBytes,
    },
    {
      title: "已用",
      dataIndex: "used_bytes",
      width: 130,
      render: formatBytes,
    },
    {
      title: "可用",
      dataIndex: "available_bytes",
      width: 130,
      render: formatBytes,
    },
    {
      title: "使用率",
      dataIndex: "used_percent",
      width: 180,
      render: (value: number) => (
        <Progress
          percent={Number(formatPercent(value))}
          size="small"
          status={progressStatus(value)}
        />
      ),
    },
    {
      title: "可移除",
      dataIndex: "removable",
      width: 100,
      render: (value: boolean) => <StatusTag active={value} />,
    },
  ];
}

function networkColumns(): ColumnsType<NetworkInfo> {
  return [
    { title: "接口", dataIndex: "interface", width: 180 },
    {
      title: "接收流量",
      dataIndex: "received_bytes",
      width: 140,
      render: formatBytes,
    },
    {
      title: "发送流量",
      dataIndex: "transmitted_bytes",
      width: 140,
      render: formatBytes,
    },
    { title: "接收包", dataIndex: "received_packets", width: 120 },
    { title: "发送包", dataIndex: "transmitted_packets", width: 120 },
    { title: "接收错误", dataIndex: "received_errors", width: 120 },
    { title: "发送错误", dataIndex: "transmitted_errors", width: 120 },
  ];
}

function GpuCards({
  gpus,
  message,
}: { gpus: GpuInfo[]; message?: string | null }) {
  if (gpus.length === 0) {
    return (
      <Card title="GPU" className="admin-card">
        <Empty
          description={message ?? "未检测到 GPU"}
          image={Empty.PRESENTED_IMAGE_SIMPLE}
        />
      </Card>
    );
  }

  return (
    <Row gutter={[16, 16]}>
      {gpus.map((gpu) => {
        const memoryPercent = gpu.memory_total_bytes
          ? ((gpu.memory_used_bytes ?? 0) / gpu.memory_total_bytes) * 100
          : 0;
        return (
          <Col xs={24} lg={12} key={gpu.name}>
            <Card title={gpu.name} className="admin-card">
              <Space direction="vertical" className="full-width" size="middle">
                <Descriptions size="small" column={1}>
                  <Descriptions.Item label="驱动">
                    {gpu.driver_version ?? "-"}
                  </Descriptions.Item>
                  <Descriptions.Item label="温度">
                    {gpu.temperature_celsius == null
                      ? "-"
                      : `${gpu.temperature_celsius} ℃`}
                  </Descriptions.Item>
                </Descriptions>
                <div>
                  <Typography.Text type="secondary">GPU 使用率</Typography.Text>
                  <Progress
                    percent={Number(formatPercent(gpu.utilization_percent))}
                    status={progressStatus(gpu.utilization_percent ?? 0)}
                  />
                </div>
                <div>
                  <Typography.Text type="secondary">显存使用率</Typography.Text>
                  <Progress
                    percent={Number(formatPercent(memoryPercent))}
                    status={progressStatus(memoryPercent)}
                    format={() =>
                      `${formatBytes(gpu.memory_used_bytes ?? 0)} / ${formatBytes(
                        gpu.memory_total_bytes ?? 0,
                      )}`
                    }
                  />
                </div>
              </Space>
            </Card>
          </Col>
        );
      })}
    </Row>
  );
}

export function MonitoringPage() {
  const [rulePage, setRulePage] = useState(1);
  const [eventPage, setEventPage] = useState(1);
  const [processPage, setProcessPage] = useState(1);
  const [processKeyword, setProcessKeyword] = useState("");
  const [processSort, setProcessSort] = useState("cpu");
  const [processOrder, setProcessOrder] = useState("desc");
  const [editing, setEditing] = useState<RateLimitRuleRecord | null>(null);
  const [open, setOpen] = useState(false);
  const [form] = Form.useForm<SaveRateLimitRuleParams>();
  const queryClient = useQueryClient();

  const overviewQuery = useQuery({
    queryKey: ["admin-monitoring-overview"],
    queryFn: fetchMonitoringOverview,
    refetchInterval: 3000,
  });
  const processQuery = useQuery({
    queryKey: [
      "admin-monitoring-processes",
      processPage,
      processKeyword,
      processSort,
      processOrder,
    ],
    queryFn: () =>
      fetchMonitorProcesses({
        page: processPage,
        page_size: 10,
        keyword: processKeyword || undefined,
        sort: processSort,
        order: processOrder,
      }),
    refetchInterval: 5000,
  });
  const rulesQuery = useQuery({
    queryKey: ["admin-rate-limits", rulePage],
    queryFn: () => fetchRateLimitRules({ page: rulePage, page_size: 10 }),
  });
  const eventsQuery = useQuery({
    queryKey: ["admin-rate-limit-events", eventPage],
    queryFn: () => fetchRateLimitEvents({ page: eventPage, page_size: 10 }),
  });
  const saveMutation = useMutation({
    mutationFn: (values: SaveRateLimitRuleParams) =>
      editing
        ? updateRateLimitRule(editing.id, values)
        : createRateLimitRule(values),
    onSuccess: () => {
      message.success("限流规则已保存");
      setOpen(false);
      setEditing(null);
      queryClient.invalidateQueries({ queryKey: ["admin-rate-limits"] });
    },
  });
  const deleteMutation = useMutation({
    mutationFn: deleteRateLimitRule,
    onSuccess: () => {
      message.success("限流规则已删除");
      queryClient.invalidateQueries({ queryKey: ["admin-rate-limits"] });
    },
  });

  const overview = overviewQuery.data;
  const server = overview?.server;
  const maxDiskUsage = Math.max(
    0,
    ...(server?.disks.map((disk) => disk.used_percent) ?? []),
  );
  const processColumns: ColumnsType<ProcessRecord> = [
    { title: "PID", dataIndex: "pid", width: 110 },
    { title: "名称", dataIndex: "name", width: 180, ellipsis: true },
    {
      title: "状态",
      dataIndex: "status",
      width: 120,
      render: (value) => <Tag>{value}</Tag>,
    },
    {
      title: "CPU",
      dataIndex: "cpu_usage",
      width: 110,
      render: (value: number) => `${formatPercent(value)}%`,
    },
    {
      title: "内存",
      dataIndex: "memory_bytes",
      width: 130,
      render: formatBytes,
    },
    {
      title: "虚拟内存",
      dataIndex: "virtual_memory_bytes",
      width: 130,
      render: formatBytes,
    },
    {
      title: "运行时间",
      dataIndex: "run_time_seconds",
      width: 130,
      render: formatDuration,
    },
    {
      title: "启动时间",
      dataIndex: "start_time_seconds",
      width: 190,
      render: formatTimestamp,
    },
    {
      title: "用户",
      dataIndex: "user_id",
      width: 140,
      render: (value) => value ?? "-",
    },
    { title: "命令", dataIndex: "command", width: 420, ellipsis: true },
  ];
  const ruleColumns: ColumnsType<RateLimitRuleRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    { title: "名称", dataIndex: "name", width: 160 },
    { title: "范围", dataIndex: "scope", width: 100 },
    { title: "路径", dataIndex: "path_pattern", width: 220 },
    {
      title: "方法",
      dataIndex: "method",
      width: 100,
      render: (value) => value ?? "全部",
    },
    { title: "次数", dataIndex: "limit_count", width: 90 },
    { title: "窗口秒", dataIndex: "window_seconds", width: 100 },
    {
      title: "启用",
      dataIndex: "enabled",
      width: 90,
      render: (value) => <StatusTag active={value} />,
    },
    {
      title: "操作",
      key: "actions",
      width: 190,
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:rate_limit:update"
            onClick={() => {
              setEditing(record);
              form.setFieldsValue(record);
              setOpen(true);
            }}
          >
            编辑
          </PermissionButton>
          <Popconfirm
            title="确认删除限流规则？"
            onConfirm={() => deleteMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              icon={<DeleteOutlined />}
              permission="system:rate_limit:delete"
            >
              删除
            </PermissionButton>
          </Popconfirm>
        </Space>
      ),
    },
  ];
  const eventColumns: ColumnsType<RateLimitEventRecord> = [
    { title: "ID", dataIndex: "id", width: 80 },
    { title: "IP", dataIndex: "ip", width: 160 },
    { title: "方法", dataIndex: "method", width: 100 },
    { title: "路径", dataIndex: "path", width: 260 },
    { title: "规则", dataIndex: "rule_id", width: 100 },
    { title: "时间", dataIndex: "occurred_at", width: 220 },
  ];

  return (
    <CrudPage
      title="系统监控"
      subtitle="服务器资源、实时性能、进程列表、健康检查和 IP 限流事件"
      breadcrumb={["系统管理", "系统监控"]}
      icon={<MonitorOutlined />}
    >
      <Tabs
        items={[
          {
            key: "overview",
            label: "监控概览",
            children: (
              <Space direction="vertical" className="full-width" size="middle">
                <Space wrap>
                  <Tag color={overview?.db_ok ? "green" : "red"}>
                    数据库 {overview?.db_ok ? "正常" : "异常"}
                  </Tag>
                  <Typography.Text type="secondary">
                    最后刷新：
                    {server?.captured_at
                      ? new Date(server.captured_at).toLocaleString()
                      : "-"}
                  </Typography.Text>
                  <Button
                    size="small"
                    icon={<ReloadOutlined />}
                    onClick={() => overviewQuery.refetch()}
                    loading={overviewQuery.isFetching}
                  >
                    刷新
                  </Button>
                </Space>
                <Row gutter={[16, 16]}>
                  <Col xs={24} sm={12} xl={6}>
                    <WidgetCard
                      title="CPU 使用率"
                      value={formatPercent(server?.cpu.global_usage)}
                      suffix="%"
                      prefix={<DashboardOutlined />}
                    />
                  </Col>
                  <Col xs={24} sm={12} xl={6}>
                    <WidgetCard
                      title="内存使用率"
                      value={formatPercent(server?.memory.used_percent)}
                      suffix="%"
                      tone="green"
                      prefix={<CloudServerOutlined />}
                    />
                  </Col>
                  <Col xs={24} sm={12} xl={6}>
                    <WidgetCard
                      title="最高磁盘使用率"
                      value={formatPercent(maxDiskUsage)}
                      suffix="%"
                      tone="orange"
                      prefix={<HddOutlined />}
                    />
                  </Col>
                  <Col xs={24} sm={12} xl={6}>
                    <WidgetCard
                      title="运行时间"
                      value={formatDuration(server?.host.uptime_seconds)}
                      tone="red"
                      prefix={<ClockCircleOutlined />}
                    />
                  </Col>
                </Row>
                <Row gutter={[16, 16]}>
                  <Col xs={24} lg={8}>
                    <Card title="CPU" className="admin-card">
                      <Space
                        direction="vertical"
                        className="full-width"
                        size="middle"
                      >
                        <Progress
                          type="dashboard"
                          percent={Number(
                            formatPercent(server?.cpu.global_usage),
                          )}
                          status={progressStatus(server?.cpu.global_usage ?? 0)}
                        />
                        <Descriptions size="small" column={1}>
                          <Descriptions.Item label="型号">
                            {server?.cpu.brand ?? "-"}
                          </Descriptions.Item>
                          <Descriptions.Item label="逻辑核心">
                            {server?.cpu.logical_cores ?? 0}
                          </Descriptions.Item>
                          <Descriptions.Item label="物理核心">
                            {server?.cpu.physical_cores ?? "-"}
                          </Descriptions.Item>
                          <Descriptions.Item label="频率">
                            {server?.cpu.frequency_mhz ?? 0} MHz
                          </Descriptions.Item>
                        </Descriptions>
                      </Space>
                    </Card>
                  </Col>
                  <Col xs={24} lg={8}>
                    <Card title="内存" className="admin-card">
                      <Space
                        direction="vertical"
                        className="full-width"
                        size="middle"
                      >
                        <Progress
                          type="dashboard"
                          percent={Number(
                            formatPercent(server?.memory.used_percent),
                          )}
                          status={progressStatus(
                            server?.memory.used_percent ?? 0,
                          )}
                        />
                        <Descriptions size="small" column={1}>
                          <Descriptions.Item label="总内存">
                            {formatBytes(server?.memory.total_bytes ?? 0)}
                          </Descriptions.Item>
                          <Descriptions.Item label="已用">
                            {formatBytes(server?.memory.used_bytes ?? 0)}
                          </Descriptions.Item>
                          <Descriptions.Item label="可用">
                            {formatBytes(server?.memory.available_bytes ?? 0)}
                          </Descriptions.Item>
                          <Descriptions.Item label="Swap">
                            {formatBytes(server?.memory.swap_used_bytes ?? 0)} /{" "}
                            {formatBytes(server?.memory.swap_total_bytes ?? 0)}
                          </Descriptions.Item>
                        </Descriptions>
                      </Space>
                    </Card>
                  </Col>
                  <Col xs={24} lg={8}>
                    <Card title="服务器信息" className="admin-card">
                      <Descriptions size="small" column={1}>
                        <Descriptions.Item label="主机名">
                          {server?.host.hostname ?? "-"}
                        </Descriptions.Item>
                        <Descriptions.Item label="系统">
                          {server?.host.long_os_version ??
                            server?.host.system_name ??
                            "-"}
                        </Descriptions.Item>
                        <Descriptions.Item label="内核">
                          {server?.host.kernel_version ?? "-"}
                        </Descriptions.Item>
                        <Descriptions.Item label="架构">
                          {server?.host.architecture ?? "-"}
                        </Descriptions.Item>
                        <Descriptions.Item label="进程数">
                          {server?.host.process_count ?? 0}
                        </Descriptions.Item>
                        <Descriptions.Item label="负载">
                          {server?.load
                            ? `${server.load.one.toFixed(2)} / ${server.load.five.toFixed(2)} / ${server.load.fifteen.toFixed(2)}`
                            : "-"}
                        </Descriptions.Item>
                      </Descriptions>
                    </Card>
                  </Col>
                </Row>
                <GpuCards
                  gpus={server?.gpus ?? []}
                  message={server?.gpu_message}
                />
                <Row gutter={[16, 16]}>
                  <Col xs={24} xl={12}>
                    <Card title="磁盘" className="admin-card">
                      <DataTable<DiskInfo>
                        rowKey="mount_point"
                        columns={diskColumns()}
                        dataSource={server?.disks ?? []}
                        loading={overviewQuery.isLoading}
                        pagination={false}
                      />
                    </Card>
                  </Col>
                  <Col xs={24} xl={12}>
                    <Card title="网络" className="admin-card">
                      <DataTable<NetworkInfo>
                        rowKey="interface"
                        columns={networkColumns()}
                        dataSource={server?.networks ?? []}
                        loading={overviewQuery.isLoading}
                        pagination={false}
                      />
                    </Card>
                  </Col>
                </Row>
                <Row gutter={[16, 16]}>
                  <Col xs={24} sm={12} lg={6}>
                    <Card title="任务成功">
                      {overview?.task_success_count ?? 0}
                    </Card>
                  </Col>
                  <Col xs={24} sm={12} lg={6}>
                    <Card title="任务失败">
                      {overview?.task_failed_count ?? 0}
                    </Card>
                  </Col>
                  <Col xs={24} sm={12} lg={6}>
                    <Card title="限流事件">
                      {overview?.rate_limit_event_count ?? 0}
                    </Card>
                  </Col>
                  <Col xs={24} sm={12} lg={6}>
                    <Card title="错误日志">
                      {overview?.error_log_count ?? 0}
                    </Card>
                  </Col>
                </Row>
              </Space>
            ),
          },
          {
            key: "processes",
            label: "进程列表",
            children: (
              <Space direction="vertical" className="full-width" size="middle">
                <div className="crud-toolbar">
                  <Space wrap>
                    <Input.Search
                      allowClear
                      placeholder="搜索 PID、名称或命令"
                      style={{ width: 260 }}
                      onSearch={(value) => {
                        setProcessKeyword(value.trim());
                        setProcessPage(1);
                      }}
                    />
                    <Select
                      value={processSort}
                      style={{ width: 140 }}
                      onChange={(value) => {
                        setProcessSort(value);
                        setProcessPage(1);
                      }}
                      options={[
                        { value: "cpu", label: "按 CPU" },
                        { value: "memory", label: "按内存" },
                        { value: "run_time", label: "按运行时间" },
                        { value: "pid", label: "按 PID" },
                        { value: "name", label: "按名称" },
                      ]}
                    />
                    <Select
                      value={processOrder}
                      style={{ width: 120 }}
                      onChange={(value) => {
                        setProcessOrder(value);
                        setProcessPage(1);
                      }}
                      options={[
                        { value: "desc", label: "降序" },
                        { value: "asc", label: "升序" },
                      ]}
                    />
                    <Button
                      icon={<ReloadOutlined />}
                      onClick={() => processQuery.refetch()}
                      loading={processQuery.isFetching}
                    >
                      刷新
                    </Button>
                  </Space>
                </div>
                <DataTable<ProcessRecord>
                  rowKey="pid"
                  columns={processColumns}
                  dataSource={processQuery.data?.items ?? []}
                  loading={processQuery.isLoading}
                  pagination={{
                    current: processPage,
                    total: processQuery.data?.total ?? 0,
                    onChange: setProcessPage,
                  }}
                />
              </Space>
            ),
          },
          {
            key: "rules",
            label: "限流规则",
            children: (
              <Space direction="vertical" className="full-width" size="middle">
                <CrudToolbar
                  actions={[
                    {
                      key: "create",
                      label: "新增规则",
                      icon: <PlusOutlined />,
                      primary: true,
                      permission: "system:rate_limit:create",
                      onClick: () => {
                        setEditing(null);
                        form.resetFields();
                        form.setFieldsValue({
                          scope: "ip",
                          method: "POST",
                          limit_count: 5,
                          window_seconds: 60,
                          enabled: true,
                        });
                        setOpen(true);
                      },
                    },
                  ]}
                />
                <DataTable<RateLimitRuleRecord>
                  columns={ruleColumns}
                  dataSource={rulesQuery.data?.items ?? []}
                  loading={rulesQuery.isLoading}
                  pagination={{
                    current: rulePage,
                    total: rulesQuery.data?.total ?? 0,
                    onChange: setRulePage,
                  }}
                />
              </Space>
            ),
          },
          {
            key: "events",
            label: "限流事件",
            children: (
              <DataTable<RateLimitEventRecord>
                columns={eventColumns}
                dataSource={eventsQuery.data?.items ?? []}
                loading={eventsQuery.isLoading}
                pagination={{
                  current: eventPage,
                  total: eventsQuery.data?.total ?? 0,
                  onChange: setEventPage,
                }}
              />
            ),
          },
        ]}
      />
      <Modal
        title={editing ? "编辑限流规则" : "新增限流规则"}
        open={open}
        onCancel={() => setOpen(false)}
        onOk={() => form.submit()}
        confirmLoading={saveMutation.isPending}
      >
        <Form
          form={form}
          layout="vertical"
          onFinish={(values) => saveMutation.mutate(values)}
        >
          <Form.Item name="name" label="名称" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="scope" label="范围" rules={[{ required: true }]}>
            <Select
              options={[
                { value: "ip", label: "IP" },
                { value: "user", label: "用户" },
                { value: "global", label: "全局" },
              ]}
            />
          </Form.Item>
          <Form.Item
            name="path_pattern"
            label="路径"
            rules={[{ required: true }]}
          >
            <Input />
          </Form.Item>
          <Form.Item name="method" label="方法">
            <Select
              allowClear
              options={["GET", "POST", "PUT", "DELETE"].map((value) => ({
                value,
                label: value,
              }))}
            />
          </Form.Item>
          <Form.Item
            name="limit_count"
            label="次数"
            rules={[{ required: true }]}
          >
            <InputNumber min={1} className="full-width" />
          </Form.Item>
          <Form.Item
            name="window_seconds"
            label="窗口秒"
            rules={[{ required: true }]}
          >
            <InputNumber min={1} className="full-width" />
          </Form.Item>
          <Form.Item name="description" label="说明">
            <Input.TextArea rows={2} />
          </Form.Item>
          <Form.Item name="enabled" label="启用" valuePropName="checked">
            <Switch />
          </Form.Item>
        </Form>
      </Modal>
    </CrudPage>
  );
}
