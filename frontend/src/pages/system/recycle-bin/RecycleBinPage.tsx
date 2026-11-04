import {
  ClearOutlined,
  DeleteOutlined,
  ReloadOutlined,
  RestOutlined,
  UndoOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  Button,
  Input,
  Popconfirm,
  Select,
  Space,
  Tag,
  Typography,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import dayjs from "dayjs";
import { useState } from "react";
import {
  type RecycleEntryRecord,
  emptyRecycleBin,
  fetchRecycleBin,
  fetchRecycleSummary,
  purgeRecycleEntry,
  restoreRecycleEntry,
} from "../../../api/admin/recycleBin";
import { CrudPage } from "../../../components/admin/CrudPage";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { t, tData } from "../../../i18n";

const formatTime = (value?: string | null) =>
  value ? dayjs(value).format("YYYY-MM-DD HH:mm") : "-";

export function RecycleBinPage() {
  const [page, setPage] = useState(1);
  const [resource, setResource] = useState<string>();
  const [keyword, setKeyword] = useState("");
  const [search, setSearch] = useState("");
  const queryClient = useQueryClient();

  const summaryQuery = useQuery({
    queryKey: ["recycle-bin-summary"],
    queryFn: fetchRecycleSummary,
  });
  const listQuery = useQuery({
    queryKey: ["recycle-bin", page, resource, search],
    queryFn: () =>
      fetchRecycleBin({
        page,
        page_size: 20,
        resource,
        keyword: search || undefined,
      }),
  });
  const refresh = () =>
    queryClient.invalidateQueries({ queryKey: ["recycle-bin"] });

  const restoreMutation = useMutation({
    mutationFn: restoreRecycleEntry,
    onSuccess: (entry) => {
      message.success(
        t("已恢复{0}：{1}", tData(entry.resource_label), entry.title),
      );
      void refresh();
      // Restored records reappear in their own pages.
      void queryClient.invalidateQueries();
    },
  });
  const purgeMutation = useMutation({
    mutationFn: purgeRecycleEntry,
    onSuccess: () => {
      message.success(t("已彻底删除"));
      void refresh();
    },
  });
  const emptyMutation = useMutation({
    mutationFn: () => emptyRecycleBin(resource),
    onSuccess: (count) => {
      message.success(t("已彻底删除 {0} 条", count));
      void refresh();
    },
  });

  const retentionDays = summaryQuery.data?.retention_days;
  const resourceOptions = (summaryQuery.data?.resources ?? []).map((item) => ({
    value: item.key,
    label: tData(item.label),
  }));
  const total = listQuery.data?.total ?? 0;

  const columns: ColumnsType<RecycleEntryRecord> = [
    {
      title: t("类型"),
      dataIndex: "resource_label",
      width: 140,
      render: (label: string) => <Tag>{tData(label)}</Tag>,
    },
    {
      title: t("名称"),
      dataIndex: "title",
      render: (title: string, record) => (
        <Space direction="vertical" size={0}>
          <Typography.Text strong>{title}</Typography.Text>
          {record.item_count > 1 ? (
            <Typography.Text type="secondary">
              {t("连同 {0} 条关联数据", record.item_count - 1)}
            </Typography.Text>
          ) : null}
        </Space>
      ),
    },
    {
      title: t("删除人"),
      dataIndex: "deleted_by_name",
      width: 150,
      render: (value) => value || "-",
    },
    {
      title: t("删除时间"),
      dataIndex: "deleted_at",
      width: 160,
      render: formatTime,
    },
    {
      title: t("自动清除"),
      dataIndex: "expires_at",
      width: 160,
      render: (value: string | null) =>
        value ? formatTime(value) : t("永久保留"),
    },
    {
      title: t("操作"),
      key: "actions",
      width: 200,
      render: (_, record) => (
        <Space>
          <PermissionButton
            size="small"
            type="primary"
            icon={<UndoOutlined />}
            permission="system:recycle:restore"
            loading={
              restoreMutation.isPending &&
              restoreMutation.variables === record.id
            }
            onClick={() => restoreMutation.mutate(record.id)}
          >
            {t("恢复")}
          </PermissionButton>
          <Popconfirm
            title={t("彻底删除后无法恢复，确定吗？")}
            okText={t("彻底删除")}
            okButtonProps={{ danger: true }}
            cancelText={t("取消")}
            onConfirm={() => purgeMutation.mutate(record.id)}
          >
            <PermissionButton
              size="small"
              danger
              icon={<DeleteOutlined />}
              permission="system:recycle:purge"
            >
              {t("彻底删除")}
            </PermissionButton>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <CrudPage
      title={t("回收站")}
      icon={<RestOutlined />}
      toolbar={
        <Space wrap>
          <Select
            allowClear
            showSearch
            optionFilterProp="label"
            className="recycle-resource-select"
            placeholder={t("全部类型")}
            value={resource}
            options={resourceOptions}
            onChange={(value) => {
              setResource(value);
              setPage(1);
            }}
          />
          <Input.Search
            allowClear
            value={keyword}
            placeholder={t("名称或删除人")}
            onChange={(event) => setKeyword(event.target.value)}
            onSearch={(value) => {
              setSearch(value.trim());
              setPage(1);
            }}
          />
          <Button
            icon={<ReloadOutlined />}
            loading={listQuery.isFetching}
            onClick={() => void listQuery.refetch()}
          >
            {t("刷新")}
          </Button>
          <Popconfirm
            title={
              resource
                ? t("彻底删除回收站中所有该类型的数据？")
                : t("清空回收站？所有数据将被彻底删除，无法恢复")
            }
            okText={t("清空")}
            okButtonProps={{ danger: true }}
            cancelText={t("取消")}
            disabled={total === 0}
            onConfirm={() => emptyMutation.mutate()}
          >
            <PermissionButton
              danger
              icon={<ClearOutlined />}
              permission="system:recycle:purge"
              disabled={total === 0}
              loading={emptyMutation.isPending}
            >
              {resource ? t("清空该类型") : t("清空回收站")}
            </PermissionButton>
          </Popconfirm>
        </Space>
      }
    >
      {retentionDays !== undefined ? (
        <Alert
          className="recycle-retention-alert"
          type="info"
          showIcon
          message={
            retentionDays > 0
              ? t(
                  "删除的数据会在回收站保留 {0} 天，到期自动彻底删除。恢复时会一并恢复关联数据。",
                  retentionDays,
                )
              : t("删除的数据会一直保留在回收站。恢复时会一并恢复关联数据。")
          }
        />
      ) : null}
      <DataTable<RecycleEntryRecord>
        rowKey="id"
        columns={columns}
        dataSource={listQuery.data?.items ?? []}
        loading={listQuery.isLoading}
        pagination={{
          current: page,
          pageSize: 20,
          total,
          onChange: setPage,
        }}
      />
    </CrudPage>
  );
}
