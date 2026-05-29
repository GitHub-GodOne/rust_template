import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type PaymentChannelRecord = {
  id: number;
  tenant_id?: number | null;
  name: string;
  provider: string;
  channel_code: string;
  currency: string;
  config: string;
  secret_config?: string | null;
  notify_url?: string | null;
  return_url?: string | null;
  enabled: boolean;
  sort_order: number;
  description?: string | null;
  created_by?: number | null;
  updated_by?: number | null;
  created_at: string;
  updated_at: string;
};

export type PaymentChannelSummary = {
  id: number;
  name: string;
  provider: string;
  channel_code: string;
  currency: string;
};

export type PaymentOrderRecord = {
  id: number;
  tenant_id?: number | null;
  channel_id?: number | null;
  order_no: string;
  merchant_order_no?: string | null;
  subject: string;
  body?: string | null;
  amount: string;
  currency: string;
  provider: string;
  status: string;
  paid_at?: string | null;
  expired_at?: string | null;
  client_ip?: string | null;
  payer_id?: string | null;
  trade_no?: string | null;
  metadata?: string | null;
  created_by?: number | null;
  created_at: string;
  updated_at: string;
};

export type PaymentCallbackRecord = {
  id: number;
  tenant_id?: number | null;
  payment_order_id?: number | null;
  provider: string;
  event_type: string;
  trade_no?: string | null;
  payload: string;
  signature?: string | null;
  verified: boolean;
  processed: boolean;
  error_message?: string | null;
  created_at: string;
  updated_at: string;
};

export type PaymentRefundRecord = {
  id: number;
  tenant_id?: number | null;
  payment_order_id: number;
  refund_no: string;
  amount: string;
  reason?: string | null;
  status: string;
  provider_refund_no?: string | null;
  requested_by?: number | null;
  reviewed_by?: number | null;
  reviewed_at?: string | null;
  metadata?: string | null;
  created_at: string;
  updated_at: string;
};

export type PaymentOrderDetailRecord = PaymentOrderRecord & {
  channel?: PaymentChannelSummary | null;
  callbacks: PaymentCallbackRecord[];
  refunds: PaymentRefundRecord[];
};

export type SavePaymentChannelParams = {
  name: string;
  provider: string;
  channel_code: string;
  currency?: string | null;
  config: string;
  secret_config?: string | null;
  notify_url?: string | null;
  return_url?: string | null;
  enabled?: boolean | null;
  sort_order?: number | null;
  description?: string | null;
};

export type CreatePaymentOrderParams = {
  channel_id?: number | null;
  merchant_order_no?: string | null;
  subject: string;
  body?: string | null;
  amount: string;
  currency?: string | null;
  provider?: string | null;
  expired_at?: string | null;
  client_ip?: string | null;
  payer_id?: string | null;
  metadata?: string | null;
};

export async function fetchPaymentChannels(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
  provider?: string;
  enabled?: boolean;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<PaymentChannelRecord>>
  >("/admin/payment-channels", { params });
  return unwrap(response.data);
}

export async function fetchPaymentChannel(id: number) {
  const response = await apiClient.get<ApiResponse<PaymentChannelRecord>>(
    `/admin/payment-channels/${id}`,
  );
  return unwrap(response.data);
}

export async function createPaymentChannel(payload: SavePaymentChannelParams) {
  const response = await apiClient.post<ApiResponse<PaymentChannelRecord>>(
    "/admin/payment-channels",
    payload,
  );
  return unwrap(response.data);
}

export async function updatePaymentChannel(
  id: number,
  payload: SavePaymentChannelParams,
) {
  const response = await apiClient.put<ApiResponse<PaymentChannelRecord>>(
    `/admin/payment-channels/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deletePaymentChannel(id: number) {
  await apiClient.delete(`/admin/payment-channels/${id}`);
}

export async function fetchPaymentOrders(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
  provider?: string;
  status?: string;
  channel_id?: number;
  merchant_order_no?: string;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<PaymentOrderRecord>>
  >("/admin/payment-orders", { params });
  return unwrap(response.data);
}

export async function fetchPaymentOrder(id: number) {
  const response = await apiClient.get<ApiResponse<PaymentOrderDetailRecord>>(
    `/admin/payment-orders/${id}`,
  );
  return unwrap(response.data);
}

export async function createPaymentOrder(payload: CreatePaymentOrderParams) {
  const response = await apiClient.post<ApiResponse<PaymentOrderRecord>>(
    "/admin/payment-orders",
    payload,
  );
  return unwrap(response.data);
}

export async function markPaymentOrderPaid(
  id: number,
  payload: {
    trade_no?: string | null;
    payer_id?: string | null;
    payload?: string | null;
  },
) {
  const response = await apiClient.post<ApiResponse<PaymentOrderRecord>>(
    `/admin/payment-orders/${id}/mark-paid`,
    payload,
  );
  return unwrap(response.data);
}

export async function cancelPaymentOrder(id: number) {
  const response = await apiClient.post<ApiResponse<PaymentOrderRecord>>(
    `/admin/payment-orders/${id}/cancel`,
  );
  return unwrap(response.data);
}

export async function fetchPaymentCallbacks(params?: {
  page?: number;
  page_size?: number;
  provider?: string;
  processed?: boolean;
  payment_order_id?: number;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<PaymentCallbackRecord>>
  >("/admin/payment-callbacks", { params });
  return unwrap(response.data);
}

export async function fetchPaymentCallback(id: number) {
  const response = await apiClient.get<ApiResponse<PaymentCallbackRecord>>(
    `/admin/payment-callbacks/${id}`,
  );
  return unwrap(response.data);
}

export async function fetchPaymentRefunds(params?: {
  page?: number;
  page_size?: number;
  status?: string;
  payment_order_id?: number;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<PaymentRefundRecord>>
  >("/admin/payment-refunds", { params });
  return unwrap(response.data);
}

export async function createPaymentRefund(
  orderId: number,
  payload: { amount: string; reason?: string | null; metadata?: string | null },
) {
  const response = await apiClient.post<ApiResponse<PaymentRefundRecord>>(
    `/admin/payment-orders/${orderId}/refunds`,
    payload,
  );
  return unwrap(response.data);
}

export async function approvePaymentRefund(id: number) {
  const response = await apiClient.post<ApiResponse<PaymentRefundRecord>>(
    `/admin/payment-refunds/${id}/approve`,
  );
  return unwrap(response.data);
}

export async function rejectPaymentRefund(id: number) {
  const response = await apiClient.post<ApiResponse<PaymentRefundRecord>>(
    `/admin/payment-refunds/${id}/reject`,
  );
  return unwrap(response.data);
}

export async function markPaymentRefundSucceeded(id: number) {
  const response = await apiClient.post<ApiResponse<PaymentRefundRecord>>(
    `/admin/payment-refunds/${id}/mark-succeeded`,
  );
  return unwrap(response.data);
}
