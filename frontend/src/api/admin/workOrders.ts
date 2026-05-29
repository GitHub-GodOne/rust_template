import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import type { PageResponse } from "./users";

export type WorkOrderRecord = {
  id: number;
  order_no: string;
  title: string;
  description: string;
  category?: string | null;
  priority: string;
  status: string;
  source: string;
  tenant_id?: number | null;
  creator_id?: number | null;
  assignee_id?: number | null;
  assigned_at?: string | null;
  resolved_at?: string | null;
  closed_at?: string | null;
  due_at?: string | null;
  last_comment_at?: string | null;
  metadata?: string | null;
  created_at: string;
  updated_at: string;
};

export type WorkOrderCommentRecord = {
  id: number;
  tenant_id?: number | null;
  work_order_id: number;
  author_id?: number | null;
  body: string;
  comment_type: string;
  from_status?: string | null;
  to_status?: string | null;
  metadata?: string | null;
  created_at: string;
  updated_at: string;
};

export type WorkOrderAssignmentRecord = {
  id: number;
  tenant_id?: number | null;
  work_order_id: number;
  assignee_id: number;
  assigned_by_id?: number | null;
  note?: string | null;
  created_at: string;
  updated_at: string;
};

export type WorkOrderAttachmentRecord = {
  id: number;
  tenant_id?: number | null;
  work_order_id: number;
  upload_file_id: number;
  uploaded_by_id?: number | null;
  description?: string | null;
  original_name?: string | null;
  url?: string | null;
  created_at: string;
  updated_at: string;
};

export type WorkOrderDetailRecord = WorkOrderRecord & {
  comments: WorkOrderCommentRecord[];
  assignments: WorkOrderAssignmentRecord[];
  attachments: WorkOrderAttachmentRecord[];
};

export type SaveWorkOrderParams = {
  title: string;
  description: string;
  category?: string | null;
  priority?: string | null;
  assignee_id?: number | null;
  due_at?: string | null;
  metadata?: string | null;
  attachment_file_ids?: number[] | null;
};

export async function fetchWorkOrders(params?: {
  page?: number;
  page_size?: number;
  keyword?: string;
  status?: string;
  priority?: string;
  category?: string;
  assignee_id?: number;
  creator_id?: number;
}) {
  const response = await apiClient.get<
    ApiResponse<PageResponse<WorkOrderRecord>>
  >("/admin/work-orders", { params });
  return unwrap(response.data);
}

export async function fetchWorkOrder(id: number) {
  const response = await apiClient.get<ApiResponse<WorkOrderDetailRecord>>(
    `/admin/work-orders/${id}`,
  );
  return unwrap(response.data);
}

export async function createWorkOrder(payload: SaveWorkOrderParams) {
  const response = await apiClient.post<ApiResponse<WorkOrderRecord>>(
    "/admin/work-orders",
    payload,
  );
  return unwrap(response.data);
}

export async function updateWorkOrder(
  id: number,
  payload: SaveWorkOrderParams,
) {
  const response = await apiClient.put<ApiResponse<WorkOrderRecord>>(
    `/admin/work-orders/${id}`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteWorkOrder(id: number) {
  await apiClient.delete(`/admin/work-orders/${id}`);
}

export async function transitionWorkOrder(
  id: number,
  payload: { status: string; comment?: string | null },
) {
  const response = await apiClient.post<ApiResponse<WorkOrderRecord>>(
    `/admin/work-orders/${id}/transition`,
    payload,
  );
  return unwrap(response.data);
}

export async function fetchWorkOrderComments(id: number) {
  const response = await apiClient.get<ApiResponse<WorkOrderCommentRecord[]>>(
    `/admin/work-orders/${id}/comments`,
  );
  return unwrap(response.data);
}

export async function createWorkOrderComment(
  id: number,
  payload: { body: string },
) {
  const response = await apiClient.post<ApiResponse<WorkOrderCommentRecord>>(
    `/admin/work-orders/${id}/comments`,
    payload,
  );
  return unwrap(response.data);
}

export async function assignWorkOrder(
  id: number,
  payload: { assignee_id: number; note?: string | null },
) {
  const response = await apiClient.post<ApiResponse<WorkOrderRecord>>(
    `/admin/work-orders/${id}/assign`,
    payload,
  );
  return unwrap(response.data);
}

export async function fetchWorkOrderAttachments(id: number) {
  const response = await apiClient.get<
    ApiResponse<WorkOrderAttachmentRecord[]>
  >(`/admin/work-orders/${id}/attachments`);
  return unwrap(response.data);
}

export async function addWorkOrderAttachment(
  id: number,
  payload: { upload_file_id: number; description?: string | null },
) {
  const response = await apiClient.post<ApiResponse<WorkOrderAttachmentRecord>>(
    `/admin/work-orders/${id}/attachments`,
    payload,
  );
  return unwrap(response.data);
}

export async function deleteWorkOrderAttachment(
  id: number,
  attachmentId: number,
) {
  await apiClient.delete(
    `/admin/work-orders/${id}/attachments/${attachmentId}`,
  );
}
