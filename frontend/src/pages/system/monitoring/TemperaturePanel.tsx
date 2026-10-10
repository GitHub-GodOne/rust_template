import { ReloadOutlined } from "@ant-design/icons";
import { useQuery } from "@tanstack/react-query";
import {
  Alert,
  Button,
  Card,
  Empty,
  Select,
  Space,
  Tag,
  Typography,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useState } from "react";
import {
  type TemperatureReading,
  type TemperatureRecord,
  type TemperatureSnapshot,
  fetchTemperatureHistory,
} from "../../../api/admin/monitoring";
import { DataTable } from "../../../components/admin/DataTable";

function formatTemperature(value: number | null | undefined) {
  return value == null ? "不可用" : `${value.toFixed(1)} ℃`;
}

const sensorColumns: ColumnsType<TemperatureReading> = [
  { title: "传感器", dataIndex: "sensor", width: 260 },
  {
    title: "温度",
    dataIndex: "temperature_celsius",
    width: 150,
    render: (value: number | null, sensor) =>
      value != null &&
      sensor.critical_celsius != null &&
      value >= sensor.critical_celsius ? (
        <Tag color="red">{formatTemperature(value)}</Tag>
      ) : (
        formatTemperature(value)
      ),
  },
  {
    title: "传感器临界温度",
    dataIndex: "critical_celsius",
    width: 180,
    render: (value: number | null) =>
      value == null ? "未提供" : formatTemperature(value),
  },
];

function SensorTable({ sensors }: { sensors: TemperatureReading[] }) {
  return (
    <DataTable<TemperatureReading>
      rowKey="sensor"
      columns={sensorColumns}
      dataSource={sensors}
      pagination={false}
      scroll={{ x: 600, y: 280 }}
      size="small"
    />
  );
}

export function TemperatureCard({
  snapshot,
}: { snapshot?: TemperatureSnapshot }) {
  const readable =
    snapshot?.sensors.filter((sensor) => sensor.temperature_celsius != null) ??
    [];
  const peak = readable.reduce<TemperatureReading | undefined>(
    (highest, sensor) =>
      !highest ||
      (sensor.temperature_celsius ?? Number.NEGATIVE_INFINITY) >
        (highest.temperature_celsius ?? Number.NEGATIVE_INFINITY)
        ? sensor
        : highest,
    undefined,
  );
  return (
    <Card title="主机温度" className="admin-card">
      <Space direction="vertical" className="full-width" size="middle">
        <Space wrap size="large">
          <div>
            <Typography.Text type="secondary">当前最高读数</Typography.Text>
            <Typography.Title level={3} style={{ margin: "4px 0" }}>
              {formatTemperature(peak?.temperature_celsius)}
            </Typography.Title>
            <Typography.Text type="secondary">
              {peak?.sensor ?? "等待有效读数"}
            </Typography.Text>
          </div>
          <Typography.Text type="secondary">
            可读取 {readable.length} / {snapshot?.sensors.length ?? 0} 个传感器
            <br />
            温度采样：
            {snapshot?.captured_at
              ? new Date(snapshot.captured_at).toLocaleString()
              : "等待采样"}
          </Typography.Text>
        </Space>
        {snapshot?.available ? null : (
          <Alert
            type="info"
            showIcon
            message={snapshot?.message ?? "正在读取主机温度传感器"}
          />
        )}
        {snapshot?.sensors.length ? (
          <SensorTable sensors={snapshot.sensors} />
        ) : null}
        <Typography.Text type="secondary">
          每 15 秒刷新温度，每分钟保存一条历史记录，保留最近 7 天。
        </Typography.Text>
      </Space>
    </Card>
  );
}

export function TemperatureHistory() {
  const [page, setPage] = useState(1);
  const [hours, setHours] = useState(24);
  const history = useQuery({
    queryKey: ["admin-temperature-history", page, hours],
    queryFn: () => fetchTemperatureHistory({ page, page_size: 20, hours }),
    refetchInterval: 60_000,
  });
  const columns: ColumnsType<TemperatureRecord> = [
    {
      title: "采样时间",
      dataIndex: "captured_at",
      width: 220,
      render: (value: string) => new Date(value).toLocaleString(),
    },
    { title: "主机", dataIndex: "hostname", width: 180 },
    {
      title: "最高温度",
      dataIndex: "max_temperature_celsius",
      width: 130,
      render: (value: number) => formatTemperature(value),
    },
    { title: "最高读数传感器", dataIndex: "peak_sensor", width: 240 },
    { title: "有效传感器数", dataIndex: "sensor_count", width: 140 },
  ];
  return (
    <Space direction="vertical" className="full-width" size="middle">
      <Space wrap>
        <Select
          value={hours}
          style={{ width: 180 }}
          onChange={(value) => {
            setHours(value);
            setPage(1);
          }}
          options={[
            { value: 1, label: "最近 1 小时" },
            { value: 6, label: "最近 6 小时" },
            { value: 24, label: "最近 24 小时" },
            { value: 168, label: "最近 7 天" },
          ]}
        />
        <Button
          icon={<ReloadOutlined />}
          loading={history.isFetching}
          onClick={() => history.refetch()}
        >
          刷新
        </Button>
        <Typography.Text type="secondary">
          展开记录可查看该次采样的全部传感器读数。
        </Typography.Text>
      </Space>
      {history.isError ? (
        <Alert type="error" showIcon message={history.error.message} />
      ) : null}
      <DataTable<TemperatureRecord>
        rowKey="id"
        columns={columns}
        dataSource={history.data?.items ?? []}
        loading={history.isLoading}
        scroll={{ x: 900 }}
        expandable={{
          expandedRowRender: (record) => (
            <SensorTable sensors={record.sensors} />
          ),
        }}
        locale={{
          emptyText: <Empty description="暂无温度记录，等待首次有效采样" />,
        }}
        pagination={{
          current: page,
          pageSize: 20,
          total: history.data?.total ?? 0,
          onChange: setPage,
          showSizeChanger: false,
        }}
      />
    </Space>
  );
}
