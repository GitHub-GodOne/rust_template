import {
  DeleteOutlined,
  EditOutlined,
  MonitorOutlined,
  PlusOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Card,
  Col,
  Form,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Row,
  Select,
  Space,
  Switch,
  Tabs,
  Tag,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useState } from "react";
import { fetchMonitoringOverview } from "../../../api/admin/monitoring";
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

export function MonitoringPage() {
  const [rulePage, setRulePage] = useState(1);
  const [eventPage, setEventPage] = useState(1);
  const [editing, setEditing] = useState<RateLimitRuleRecord | null>(null);
  const [open, setOpen] = useState(false);
  const [form] = Form.useForm<SaveRateLimitRuleParams>();
  const queryClient = useQueryClient();

  const overviewQuery = useQuery({
    queryKey: ["admin-monitoring-overview"],
    queryFn: fetchMonitoringOverview,
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

  const overview = overviewQuery.data;

  return (
    <CrudPage
      title="系统监控"
      subtitle="健康检查、任务/备份统计和 IP 限流事件"
      breadcrumb={["系统管理", "系统监控"]}
      icon={<MonitorOutlined />}
    >
      <Tabs
        items={[
          {
            key: "overview",
            label: "监控概览",
            children: (
              <Row gutter={[16, 16]}>
                <Col xs={24} sm={12} lg={6}>
                  <Card title="数据库">
                    {overview?.db_ok ? (
                      <Tag color="green">正常</Tag>
                    ) : (
                      <Tag color="red">异常</Tag>
                    )}
                  </Card>
                </Col>
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
                  <Card title="备份成功">
                    {overview?.backup_success_count ?? 0}
                  </Card>
                </Col>
                <Col xs={24} sm={12} lg={6}>
                  <Card title="备份失败">
                    {overview?.backup_failed_count ?? 0}
                  </Card>
                </Col>
                <Col xs={24} sm={12} lg={6}>
                  <Card title="错误日志">{overview?.error_log_count ?? 0}</Card>
                </Col>
                <Col xs={24} sm={12} lg={6}>
                  <Card title="健康检查">
                    {overview?.health_links.join(" / ")}
                  </Card>
                </Col>
              </Row>
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
