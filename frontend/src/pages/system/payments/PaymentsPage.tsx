import {
  CheckCircleOutlined,
  CreditCardOutlined,
  DeleteOutlined,
  EditOutlined,
  PlusOutlined,
  StopOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Button,
  Descriptions,
  Drawer,
  Form,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Select,
  Space,
  Switch,
  Tabs,
  Tag,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useState } from "react";
import {
  type CreatePaymentOrderParams,
  type PaymentCallbackRecord,
  type PaymentChannelRecord,
  type PaymentOrderDetailRecord,
  type PaymentOrderRecord,
  type PaymentRefundRecord,
  type SavePaymentChannelParams,
  approvePaymentRefund,
  cancelPaymentOrder,
  createPaymentChannel,
  createPaymentOrder,
  createPaymentRefund,
  deletePaymentChannel,
  fetchPaymentCallbacks,
  fetchPaymentChannels,
  fetchPaymentOrder,
  fetchPaymentOrders,
  fetchPaymentRefunds,
  markPaymentOrderPaid,
  markPaymentRefundSucceeded,
  rejectPaymentRefund,
  updatePaymentChannel,
} from "../../../api/admin/payments";
import { CrudPage } from "../../../components/admin/CrudPage";
import { CrudToolbar } from "../../../components/admin/CrudToolbar";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";

const providerOptions = [
  { value: "yipay", label: "易支付" },
  { value: "paypal", label: "PayPal" },
  { value: "stripe", label: "Stripe" },
  { value: "alipay", label: "支付宝" },
  { value: "wechat_pay", label: "微信支付" },
  { value: "tokenpay", label: "TokenPay" },
  { value: "bepusdt", label: "BEpusdt" },
  { value: "epusdt", label: "epusdt" },
  { value: "okpay", label: "OKPay" },
];

const paymentStatusOptions = [
  { value: "pending", label: "待支付" },
  { value: "paying", label: "支付中" },
  { value: "paid", label: "已支付" },
  { value: "failed", label: "失败" },
  { value: "cancelled", label: "已取消" },
  { value: "expired", label: "已过期" },
  { value: "refunding", label: "退款中" },
  { value: "refunded", label: "已退款" },
];

const refundStatusOptions = [
  { value: "pending", label: "待审核" },
  { value: "approved", label: "已通过" },
  { value: "processing", label: "处理中" },
  { value: "succeeded", label: "已成功" },
  { value: "failed", label: "失败" },
  { value: "rejected", label: "已拒绝" },
];

const statusColor: Record<string, string> = {
  pending: "blue",
  paying: "geekblue",
  paid: "green",
  failed: "red",
  cancelled: "default",
  expired: "orange",
  refunding: "purple",
  refunded: "cyan",
  approved: "green",
  processing: "geekblue",
  succeeded: "green",
  rejected: "red",
};

function labelOf(options: { value: string; label: string }[], value: string) {
  return options.find((item) => item.value === value)?.label ?? value;
}

function trimOptional(value?: string | null) {
  const trimmed = value?.trim();
  return trimmed ? trimmed : null;
}

function toChannelPayload(
  values: SavePaymentChannelParams,
): SavePaymentChannelParams {
  return {
    ...values,
    currency: trimOptional(values.currency) ?? "CNY",
    config: values.config || "{}",
    secret_config: trimOptional(values.secret_config),
    notify_url: trimOptional(values.notify_url),
    return_url: trimOptional(values.return_url),
    description: trimOptional(values.description),
  };
}

function toOrderPayload(
  values: CreatePaymentOrderParams,
): CreatePaymentOrderParams {
  return {
    ...values,
    channel_id: values.channel_id ?? null,
    provider: trimOptional(values.provider),
    currency: trimOptional(values.currency),
    merchant_order_no: trimOptional(values.merchant_order_no),
    body: trimOptional(values.body),
    expired_at: trimOptional(values.expired_at),
    client_ip: trimOptional(values.client_ip),
    payer_id: trimOptional(values.payer_id),
    metadata: trimOptional(values.metadata),
  };
}

export function PaymentsPage() {
  const [orderPage, setOrderPage] = useState(1);
  const [channelPage, setChannelPage] = useState(1);
  const [refundPage, setRefundPage] = useState(1);
  const [callbackPage, setCallbackPage] = useState(1);
  const [orderKeyword, setOrderKeyword] = useState("");
  const [orderStatus, setOrderStatus] = useState<string>();
  const [orderProvider, setOrderProvider] = useState<string>();
  const [channelKeyword, setChannelKeyword] = useState("");
  const [channelProvider, setChannelProvider] = useState<string>();
  const [refundStatus, setRefundStatus] = useState<string>();
  const [callbackProvider, setCallbackProvider] = useState<string>();
  const [editingChannel, setEditingChannel] =
    useState<PaymentChannelRecord | null>(null);
  const [channelOpen, setChannelOpen] = useState(false);
  const [orderOpen, setOrderOpen] = useState(false);
  const [refundOpen, setRefundOpen] = useState<PaymentOrderRecord | null>(null);
  const [paidOpen, setPaidOpen] = useState<PaymentOrderRecord | null>(null);
  const [detailId, setDetailId] = useState<number | null>(null);
  const [callbackDetail, setCallbackDetail] =
    useState<PaymentCallbackRecord | null>(null);
  const [channelForm] = Form.useForm<SavePaymentChannelParams>();
  const [orderForm] = Form.useForm<CreatePaymentOrderParams>();
  const [paidForm] = Form.useForm<{
    trade_no?: string;
    payer_id?: string;
    payload?: string;
  }>();
  const [refundForm] = Form.useForm<{ amount: string; reason?: string }>();
  const queryClient = useQueryClient();

  const channelsQuery = useQuery({
    queryKey: [
      "admin-payment-channels",
      channelPage,
      channelKeyword,
      channelProvider,
    ],
    queryFn: () =>
      fetchPaymentChannels({
        page: channelPage,
        page_size: 10,
        keyword: channelKeyword || undefined,
        provider: channelProvider,
      }),
  });
  const ordersQuery = useQuery({
    queryKey: [
      "admin-payment-orders",
      orderPage,
      orderKeyword,
      orderStatus,
      orderProvider,
    ],
    queryFn: () =>
      fetchPaymentOrders({
        page: orderPage,
        page_size: 10,
        keyword: orderKeyword || undefined,
        status: orderStatus,
        provider: orderProvider,
      }),
  });
  const refundsQuery = useQuery({
    queryKey: ["admin-payment-refunds", refundPage, refundStatus],
    queryFn: () =>
      fetchPaymentRefunds({
        page: refundPage,
        page_size: 10,
        status: refundStatus,
      }),
  });
  const callbacksQuery = useQuery({
    queryKey: ["admin-payment-callbacks", callbackPage, callbackProvider],
    queryFn: () =>
      fetchPaymentCallbacks({
        page: callbackPage,
        page_size: 10,
        provider: callbackProvider,
      }),
  });
  const detailQuery = useQuery({
    queryKey: ["admin-payment-order", detailId],
    queryFn: () => fetchPaymentOrder(detailId as number),
    enabled: Boolean(detailId),
  });

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ["admin-payment-channels"] });
    queryClient.invalidateQueries({ queryKey: ["admin-payment-orders"] });
    queryClient.invalidateQueries({ queryKey: ["admin-payment-refunds"] });
    queryClient.invalidateQueries({ queryKey: ["admin-payment-callbacks"] });
    queryClient.invalidateQueries({ queryKey: ["admin-payment-order"] });
  };

  const createChannelMutation = useMutation({
    mutationFn: createPaymentChannel,
    onSuccess: () => {
      message.success("支付通道已创建");
      setChannelOpen(false);
      channelForm.resetFields();
      invalidate();
    },
  });
  const updateChannelMutation = useMutation({
    mutationFn: ({
      id,
      payload,
    }: { id: number; payload: SavePaymentChannelParams }) =>
      updatePaymentChannel(id, payload),
    onSuccess: () => {
      message.success("支付通道已更新");
      setChannelOpen(false);
      setEditingChannel(null);
      channelForm.resetFields();
      invalidate();
    },
  });
  const deleteChannelMutation = useMutation({
    mutationFn: deletePaymentChannel,
    onSuccess: () => {
      message.success("支付通道已删除");
      invalidate();
    },
  });
  const createOrderMutation = useMutation({
    mutationFn: createPaymentOrder,
    onSuccess: () => {
      message.success("支付订单已创建");
      setOrderOpen(false);
      orderForm.resetFields();
      invalidate();
    },
  });
  const markPaidMutation = useMutation({
    mutationFn: ({
      id,
      payload,
    }: {
      id: number;
      payload: {
        trade_no?: string | null;
        payer_id?: string | null;
        payload?: string | null;
      };
    }) => markPaymentOrderPaid(id, payload),
    onSuccess: () => {
      message.success("订单已标记为已支付");
      setPaidOpen(null);
      paidForm.resetFields();
      invalidate();
    },
  });
  const cancelOrderMutation = useMutation({
    mutationFn: cancelPaymentOrder,
    onSuccess: () => {
      message.success("订单已取消");
      invalidate();
    },
  });
  const createRefundMutation = useMutation({
    mutationFn: ({
      id,
      payload,
    }: { id: number; payload: { amount: string; reason?: string | null } }) =>
      createPaymentRefund(id, payload),
    onSuccess: () => {
      message.success("退款申请已创建");
      setRefundOpen(null);
      refundForm.resetFields();
      invalidate();
    },
  });
  const approveRefundMutation = useMutation({
    mutationFn: approvePaymentRefund,
    onSuccess: () => {
      message.success("退款已通过");
      invalidate();
    },
  });
  const rejectRefundMutation = useMutation({
    mutationFn: rejectPaymentRefund,
    onSuccess: () => {
      message.success("退款已拒绝");
      invalidate();
    },
  });
  const markRefundSucceededMutation = useMutation({
    mutationFn: markPaymentRefundSucceeded,
    onSuccess: () => {
      message.success("退款已标记成功");
      invalidate();
    },
  });

  const openChannelEditor = (record?: PaymentChannelRecord) => {
    setEditingChannel(record ?? null);
    channelForm.resetFields();
    channelForm.setFieldsValue(
      record ?? {
        provider: "yipay",
        currency: "CNY",
        config: "{}",
        enabled: true,
        sort_order: 0,
      },
    );
    setChannelOpen(true);
  };

  const orderColumns: ColumnsType<PaymentOrderRecord> = [
    { title: "订单号", dataIndex: "order_no", width: 190 },
    { title: "商户订单号", dataIndex: "merchant_order_no", width: 160 },
    { title: "标题", dataIndex: "subject", width: 180 },
    {
      title: "金额",
      key: "amount",
      width: 120,
      render: (_, record) => `${record.amount} ${record.currency}`,
    },
    {
      title: "通道",
      dataIndex: "provider",
      width: 120,
      render: (value) => labelOf(providerOptions, value),
    },
    {
      title: "状态",
      dataIndex: "status",
      width: 110,
      render: (value) => (
        <Tag color={statusColor[value] ?? "default"}>
          {labelOf(paymentStatusOptions, value)}
        </Tag>
      ),
    },
    { title: "支付时间", dataIndex: "paid_at", width: 220 },
    { title: "创建时间", dataIndex: "created_at", width: 220 },
    {
      title: "操作",
      key: "actions",
      width: 320,
      fixed: "right",
      render: (_, record) => (
        <Space>
          <Button size="small" onClick={() => setDetailId(record.id)}>
            详情
          </Button>
          <PermissionButton
            size="small"
            icon={<CheckCircleOutlined />}
            permission="system:payment_order:action"
            onClick={() => setPaidOpen(record)}
          >
            标记支付
          </PermissionButton>
          <Popconfirm
            title="确认取消订单？"
            onConfirm={() => cancelOrderMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              icon={<StopOutlined />}
              permission="system:payment_order:action"
            >
              取消
            </PermissionButton>
          </Popconfirm>
          <PermissionButton
            size="small"
            permission="system:payment_refund:create"
            onClick={() => setRefundOpen(record)}
          >
            退款
          </PermissionButton>
        </Space>
      ),
    },
  ];

  const channelColumns: ColumnsType<PaymentChannelRecord> = [
    { title: "名称", dataIndex: "name", width: 180 },
    { title: "编码", dataIndex: "channel_code", width: 180 },
    {
      title: "Provider",
      dataIndex: "provider",
      width: 120,
      render: (value) => labelOf(providerOptions, value),
    },
    { title: "币种", dataIndex: "currency", width: 90 },
    {
      title: "启用",
      dataIndex: "enabled",
      width: 90,
      render: (value) => (
        <Tag color={value ? "green" : "default"}>{value ? "启用" : "停用"}</Tag>
      ),
    },
    { title: "排序", dataIndex: "sort_order", width: 90 },
    { title: "更新时间", dataIndex: "updated_at", width: 220 },
    {
      title: "操作",
      key: "actions",
      width: 180,
      fixed: "right",
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            icon={<EditOutlined />}
            permission="system:payment_channel:update"
            onClick={() => openChannelEditor(record)}
          >
            编辑
          </PermissionButton>
          <Popconfirm
            title="确认删除支付通道？"
            onConfirm={() => deleteChannelMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              icon={<DeleteOutlined />}
              permission="system:payment_channel:delete"
            >
              删除
            </PermissionButton>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  const refundColumns: ColumnsType<PaymentRefundRecord> = [
    { title: "退款号", dataIndex: "refund_no", width: 190 },
    { title: "订单 ID", dataIndex: "payment_order_id", width: 100 },
    { title: "金额", dataIndex: "amount", width: 100 },
    {
      title: "状态",
      dataIndex: "status",
      width: 110,
      render: (value) => (
        <Tag color={statusColor[value] ?? "default"}>
          {labelOf(refundStatusOptions, value)}
        </Tag>
      ),
    },
    { title: "申请人", dataIndex: "requested_by", width: 100 },
    { title: "审核人", dataIndex: "reviewed_by", width: 100 },
    { title: "创建时间", dataIndex: "created_at", width: 220 },
    {
      title: "操作",
      key: "actions",
      width: 260,
      fixed: "right",
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            permission="system:payment_refund:review"
            onClick={() => approveRefundMutation.mutate(record.id)}
          >
            通过
          </PermissionButton>
          <PermissionButton
            size="small"
            danger
            permission="system:payment_refund:review"
            onClick={() => rejectRefundMutation.mutate(record.id)}
          >
            拒绝
          </PermissionButton>
          <PermissionButton
            size="small"
            permission="system:payment_refund:review"
            onClick={() => markRefundSucceededMutation.mutate(record.id)}
          >
            标记成功
          </PermissionButton>
        </Space>
      ),
    },
  ];

  const callbackColumns: ColumnsType<PaymentCallbackRecord> = [
    { title: "Provider", dataIndex: "provider", width: 120 },
    { title: "事件", dataIndex: "event_type", width: 140 },
    { title: "订单 ID", dataIndex: "payment_order_id", width: 100 },
    {
      title: "已验证",
      dataIndex: "verified",
      width: 90,
      render: (value) => (
        <Tag color={value ? "green" : "orange"}>{value ? "是" : "否"}</Tag>
      ),
    },
    {
      title: "已处理",
      dataIndex: "processed",
      width: 90,
      render: (value) => (
        <Tag color={value ? "green" : "orange"}>{value ? "是" : "否"}</Tag>
      ),
    },
    { title: "错误", dataIndex: "error_message", width: 220 },
    { title: "创建时间", dataIndex: "created_at", width: 220 },
    {
      title: "操作",
      key: "actions",
      width: 100,
      fixed: "right",
      render: (_, record) => (
        <Button size="small" onClick={() => setCallbackDetail(record)}>
          详情
        </Button>
      ),
    },
  ];

  const detail = detailQuery.data;

  return (
    <CrudPage
      title="支付管理"
      subtitle="统一管理支付通道、订单、回调审计和退款流程"
      breadcrumb={["系统管理", "支付管理"]}
      icon={<CreditCardOutlined />}
    >
      <Tabs
        items={[
          {
            key: "orders",
            label: "支付订单",
            children: (
              <Space direction="vertical" size="middle" className="full-width">
                <Space wrap>
                  <Input.Search
                    allowClear
                    placeholder="搜索订单号、标题"
                    onSearch={(value) => {
                      setOrderPage(1);
                      setOrderKeyword(value);
                    }}
                    className="admin-search-input"
                  />
                  <Select
                    allowClear
                    placeholder="Provider"
                    value={orderProvider}
                    onChange={(value) => {
                      setOrderPage(1);
                      setOrderProvider(value);
                    }}
                    options={providerOptions}
                    className="admin-filter-select"
                  />
                  <Select
                    allowClear
                    placeholder="状态"
                    value={orderStatus}
                    onChange={(value) => {
                      setOrderPage(1);
                      setOrderStatus(value);
                    }}
                    options={paymentStatusOptions}
                    className="admin-filter-select"
                  />
                  <CrudToolbar
                    actions={[
                      {
                        key: "create",
                        label: "创建测试订单",
                        icon: <PlusOutlined />,
                        primary: true,
                        permission: "system:payment_order:create",
                        onClick: () => {
                          orderForm.resetFields();
                          orderForm.setFieldsValue({
                            provider: "yipay",
                            currency: "CNY",
                          });
                          setOrderOpen(true);
                        },
                      },
                    ]}
                  />
                </Space>
                <DataTable<PaymentOrderRecord>
                  columns={orderColumns}
                  dataSource={ordersQuery.data?.items ?? []}
                  loading={ordersQuery.isLoading}
                  pagination={{
                    current: orderPage,
                    total: ordersQuery.data?.total ?? 0,
                    onChange: setOrderPage,
                  }}
                />
              </Space>
            ),
          },
          {
            key: "channels",
            label: "支付通道",
            children: (
              <Space direction="vertical" size="middle" className="full-width">
                <Space wrap>
                  <Input.Search
                    allowClear
                    placeholder="搜索通道名称或编码"
                    onSearch={(value) => {
                      setChannelPage(1);
                      setChannelKeyword(value);
                    }}
                    className="admin-search-input"
                  />
                  <Select
                    allowClear
                    placeholder="Provider"
                    value={channelProvider}
                    onChange={(value) => {
                      setChannelPage(1);
                      setChannelProvider(value);
                    }}
                    options={providerOptions}
                    className="admin-filter-select"
                  />
                  <CrudToolbar
                    actions={[
                      {
                        key: "create",
                        label: "新增通道",
                        icon: <PlusOutlined />,
                        primary: true,
                        permission: "system:payment_channel:create",
                        onClick: () => openChannelEditor(),
                      },
                    ]}
                  />
                </Space>
                <DataTable<PaymentChannelRecord>
                  columns={channelColumns}
                  dataSource={channelsQuery.data?.items ?? []}
                  loading={channelsQuery.isLoading}
                  pagination={{
                    current: channelPage,
                    total: channelsQuery.data?.total ?? 0,
                    onChange: setChannelPage,
                  }}
                />
              </Space>
            ),
          },
          {
            key: "refunds",
            label: "退款记录",
            children: (
              <Space direction="vertical" size="middle" className="full-width">
                <Select
                  allowClear
                  placeholder="退款状态"
                  value={refundStatus}
                  onChange={(value) => {
                    setRefundPage(1);
                    setRefundStatus(value);
                  }}
                  options={refundStatusOptions}
                  className="admin-filter-select"
                />
                <DataTable<PaymentRefundRecord>
                  columns={refundColumns}
                  dataSource={refundsQuery.data?.items ?? []}
                  loading={refundsQuery.isLoading}
                  pagination={{
                    current: refundPage,
                    total: refundsQuery.data?.total ?? 0,
                    onChange: setRefundPage,
                  }}
                />
              </Space>
            ),
          },
          {
            key: "callbacks",
            label: "回调记录",
            children: (
              <Space direction="vertical" size="middle" className="full-width">
                <Select
                  allowClear
                  placeholder="Provider"
                  value={callbackProvider}
                  onChange={(value) => {
                    setCallbackPage(1);
                    setCallbackProvider(value);
                  }}
                  options={providerOptions}
                  className="admin-filter-select"
                />
                <DataTable<PaymentCallbackRecord>
                  columns={callbackColumns}
                  dataSource={callbacksQuery.data?.items ?? []}
                  loading={callbacksQuery.isLoading}
                  pagination={{
                    current: callbackPage,
                    total: callbacksQuery.data?.total ?? 0,
                    onChange: setCallbackPage,
                  }}
                />
              </Space>
            ),
          },
        ]}
      />

      <Modal
        title={editingChannel ? "编辑支付通道" : "新增支付通道"}
        open={channelOpen}
        onCancel={() => setChannelOpen(false)}
        onOk={() => channelForm.submit()}
        confirmLoading={
          createChannelMutation.isPending || updateChannelMutation.isPending
        }
        width={760}
      >
        <Form
          form={channelForm}
          layout="vertical"
          onFinish={(values) => {
            const payload = toChannelPayload(values);
            if (editingChannel) {
              updateChannelMutation.mutate({ id: editingChannel.id, payload });
            } else {
              createChannelMutation.mutate(payload);
            }
          }}
        >
          <Space className="full-width" align="start" wrap>
            <Form.Item name="name" label="名称" rules={[{ required: true }]}>
              <Input />
            </Form.Item>
            <Form.Item
              name="channel_code"
              label="编码"
              rules={[{ required: true }]}
            >
              <Input />
            </Form.Item>
            <Form.Item
              name="provider"
              label="Provider"
              rules={[{ required: true }]}
            >
              <Select
                options={providerOptions}
                className="admin-filter-select"
              />
            </Form.Item>
            <Form.Item name="currency" label="币种">
              <Input placeholder="CNY" />
            </Form.Item>
          </Space>
          <Form.Item
            name="config"
            label="公开配置 JSON"
            rules={[{ required: true }]}
          >
            <Input.TextArea rows={4} placeholder='{"gateway":"https://..."}' />
          </Form.Item>
          <Form.Item name="secret_config" label="密钥配置 JSON">
            <Input.TextArea rows={3} placeholder='{"secret":"******"}' />
          </Form.Item>
          <Space className="full-width" align="start" wrap>
            <Form.Item name="notify_url" label="通知地址">
              <Input />
            </Form.Item>
            <Form.Item name="return_url" label="返回地址">
              <Input />
            </Form.Item>
            <Form.Item name="sort_order" label="排序">
              <InputNumber />
            </Form.Item>
            <Form.Item name="enabled" label="启用" valuePropName="checked">
              <Switch />
            </Form.Item>
          </Space>
          <Form.Item name="description" label="说明">
            <Input.TextArea rows={2} />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title="创建测试支付订单"
        open={orderOpen}
        onCancel={() => setOrderOpen(false)}
        onOk={() => orderForm.submit()}
        confirmLoading={createOrderMutation.isPending}
        width={720}
      >
        <Form
          form={orderForm}
          layout="vertical"
          onFinish={(values) =>
            createOrderMutation.mutate(toOrderPayload(values))
          }
        >
          <Form.Item name="subject" label="标题" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Space className="full-width" align="start" wrap>
            <Form.Item name="amount" label="金额" rules={[{ required: true }]}>
              <Input placeholder="99.00" />
            </Form.Item>
            <Form.Item name="currency" label="币种">
              <Input placeholder="CNY" />
            </Form.Item>
            <Form.Item name="provider" label="Provider">
              <Select
                options={providerOptions}
                className="admin-filter-select"
              />
            </Form.Item>
            <Form.Item name="channel_id" label="通道 ID">
              <InputNumber min={1} />
            </Form.Item>
          </Space>
          <Form.Item name="merchant_order_no" label="商户订单号">
            <Input />
          </Form.Item>
          <Form.Item name="body" label="描述">
            <Input.TextArea rows={2} />
          </Form.Item>
          <Form.Item name="expired_at" label="过期时间 RFC3339">
            <Input placeholder="2026-05-28T12:00:00+08:00" />
          </Form.Item>
          <Form.Item name="metadata" label="扩展数据 JSON">
            <Input.TextArea rows={3} placeholder='{"source":"admin"}' />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title="标记订单已支付"
        open={Boolean(paidOpen)}
        onCancel={() => setPaidOpen(null)}
        onOk={() => paidForm.submit()}
        confirmLoading={markPaidMutation.isPending}
      >
        <Form
          form={paidForm}
          layout="vertical"
          onFinish={(values) => {
            if (paidOpen) {
              markPaidMutation.mutate({
                id: paidOpen.id,
                payload: {
                  trade_no: trimOptional(values.trade_no),
                  payer_id: trimOptional(values.payer_id),
                  payload: trimOptional(values.payload),
                },
              });
            }
          }}
        >
          <Form.Item name="trade_no" label="三方流水号">
            <Input />
          </Form.Item>
          <Form.Item name="payer_id" label="付款方 ID">
            <Input />
          </Form.Item>
          <Form.Item name="payload" label="回调载荷 JSON">
            <Input.TextArea rows={3} placeholder='{"manual":true}' />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title="创建退款申请"
        open={Boolean(refundOpen)}
        onCancel={() => setRefundOpen(null)}
        onOk={() => refundForm.submit()}
        confirmLoading={createRefundMutation.isPending}
      >
        <Form
          form={refundForm}
          layout="vertical"
          onFinish={(values) => {
            if (refundOpen) {
              createRefundMutation.mutate({
                id: refundOpen.id,
                payload: {
                  amount: values.amount,
                  reason: trimOptional(values.reason),
                },
              });
            }
          }}
        >
          <Form.Item
            name="amount"
            label="退款金额"
            rules={[{ required: true }]}
          >
            <Input placeholder={refundOpen?.amount} />
          </Form.Item>
          <Form.Item name="reason" label="退款原因">
            <Input.TextArea rows={3} />
          </Form.Item>
        </Form>
      </Modal>

      <Drawer
        title="支付订单详情"
        open={Boolean(detailId)}
        onClose={() => setDetailId(null)}
        width={760}
      >
        {detail && <OrderDetail detail={detail} />}
      </Drawer>

      <Drawer
        title="回调详情"
        open={Boolean(callbackDetail)}
        onClose={() => setCallbackDetail(null)}
        width={680}
      >
        {callbackDetail && (
          <Space direction="vertical" size="middle" className="full-width">
            <Descriptions bordered size="small" column={2}>
              <Descriptions.Item label="Provider">
                {callbackDetail.provider}
              </Descriptions.Item>
              <Descriptions.Item label="事件">
                {callbackDetail.event_type}
              </Descriptions.Item>
              <Descriptions.Item label="订单 ID">
                {callbackDetail.payment_order_id ?? "-"}
              </Descriptions.Item>
              <Descriptions.Item label="流水号">
                {callbackDetail.trade_no ?? "-"}
              </Descriptions.Item>
              <Descriptions.Item label="已验证">
                {callbackDetail.verified ? "是" : "否"}
              </Descriptions.Item>
              <Descriptions.Item label="已处理">
                {callbackDetail.processed ? "是" : "否"}
              </Descriptions.Item>
              <Descriptions.Item label="错误" span={2}>
                {callbackDetail.error_message ?? "-"}
              </Descriptions.Item>
            </Descriptions>
            <Input.TextArea value={callbackDetail.payload} rows={10} readOnly />
          </Space>
        )}
      </Drawer>
    </CrudPage>
  );
}

function OrderDetail({ detail }: { detail: PaymentOrderDetailRecord }) {
  return (
    <Space direction="vertical" size="large" className="full-width">
      <Descriptions bordered size="small" column={2}>
        <Descriptions.Item label="订单号">{detail.order_no}</Descriptions.Item>
        <Descriptions.Item label="状态">
          <Tag color={statusColor[detail.status] ?? "default"}>
            {labelOf(paymentStatusOptions, detail.status)}
          </Tag>
        </Descriptions.Item>
        <Descriptions.Item label="标题" span={2}>
          {detail.subject}
        </Descriptions.Item>
        <Descriptions.Item label="金额">
          {detail.amount} {detail.currency}
        </Descriptions.Item>
        <Descriptions.Item label="Provider">
          {labelOf(providerOptions, detail.provider)}
        </Descriptions.Item>
        <Descriptions.Item label="通道">
          {detail.channel?.name ?? detail.channel_id ?? "-"}
        </Descriptions.Item>
        <Descriptions.Item label="商户订单号">
          {detail.merchant_order_no ?? "-"}
        </Descriptions.Item>
        <Descriptions.Item label="三方流水号">
          {detail.trade_no ?? "-"}
        </Descriptions.Item>
        <Descriptions.Item label="支付时间">
          {detail.paid_at ?? "-"}
        </Descriptions.Item>
        <Descriptions.Item label="描述" span={2}>
          {detail.body ?? "-"}
        </Descriptions.Item>
      </Descriptions>
      <div>
        <h3>退款记录</h3>
        <Space direction="vertical" className="full-width">
          {detail.refunds.map((refund) => (
            <span key={refund.id}>
              {refund.refund_no} · {refund.amount} ·{" "}
              {labelOf(refundStatusOptions, refund.status)}
            </span>
          ))}
        </Space>
      </div>
      <div>
        <h3>回调记录</h3>
        <Space direction="vertical" className="full-width">
          {detail.callbacks.map((callback) => (
            <span key={callback.id}>
              {callback.event_type} · {callback.provider} ·{" "}
              {callback.created_at}
            </span>
          ))}
        </Space>
      </div>
    </Space>
  );
}
