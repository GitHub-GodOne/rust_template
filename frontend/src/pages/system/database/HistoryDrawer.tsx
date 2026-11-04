import { CopyOutlined, DeleteOutlined, EditOutlined } from "@ant-design/icons";
import { useMutation, useQuery } from "@tanstack/react-query";
import {
  Button,
  Drawer,
  Empty,
  Input,
  List,
  Pagination,
  Popconfirm,
  Space,
  Tag,
  Tooltip,
  Typography,
  message,
} from "antd";
import dayjs from "dayjs";
import { useEffect, useState } from "react";
import {
  type DbHistoryItem,
  clearDbHistory,
  fetchDbHistory,
} from "../../../api/admin/dbManager";
import { t } from "../../../i18n";
import { formatDuration } from "./dbUtils";

const PAGE_SIZE = 20;

export function HistoryDrawer({
  open,
  connectionId,
  onClose,
  onUse,
}: {
  open: boolean;
  connectionId: number;
  onClose: () => void;
  onUse: (sql: string) => void;
}) {
  const [page, setPage] = useState(1);
  const [keyword, setKeyword] = useState("");
  const [search, setSearch] = useState("");

  const history = useQuery({
    queryKey: ["db-history", connectionId, page, search],
    queryFn: () =>
      fetchDbHistory({
        connection_id: connectionId,
        page,
        page_size: PAGE_SIZE,
        keyword: search || undefined,
      }),
    enabled: open,
  });
  const { refetch } = history;

  useEffect(() => {
    if (open) void refetch();
  }, [open, refetch]);

  const clear = useMutation({
    mutationFn: () => clearDbHistory(connectionId),
    onSuccess: () => {
      setPage(1);
      void refetch();
      message.success(t("历史已清空"));
    },
    onError: (error) => message.error(error.message),
  });

  return (
    <Drawer
      open={open}
      onClose={onClose}
      title={t("执行历史")}
      width={560}
      extra={
        <Popconfirm
          title={t("清空该连接的执行历史？")}
          onConfirm={() => clear.mutate()}
        >
          <Button size="small" danger icon={<DeleteOutlined />}>
            {t("清空")}
          </Button>
        </Popconfirm>
      }
    >
      <Input.Search
        allowClear
        value={keyword}
        placeholder={t("搜索 SQL")}
        onChange={(event) => setKeyword(event.target.value)}
        onSearch={(value) => {
          setSearch(value.trim());
          setPage(1);
        }}
        className="db-history-search"
      />
      {history.data?.items.length ? (
        <List<DbHistoryItem>
          loading={history.isFetching}
          dataSource={history.data.items}
          renderItem={(item) => (
            <List.Item
              className="db-history-item"
              actions={[
                <Tooltip key="use" title={t("载入编辑器")}>
                  <Button
                    size="small"
                    type="text"
                    icon={<EditOutlined />}
                    onClick={() => onUse(item.sql_text)}
                  />
                </Tooltip>,
                <Tooltip key="copy" title={t("复制")}>
                  <Button
                    size="small"
                    type="text"
                    icon={<CopyOutlined />}
                    onClick={() =>
                      void navigator.clipboard
                        ?.writeText(item.sql_text)
                        .then(() => message.success(t("已复制")))
                    }
                  />
                </Tooltip>,
              ]}
            >
              <div className="db-history-body">
                <Space size={6} wrap className="db-history-meta">
                  {item.status === "success" ? (
                    <Tag color="success">{t("成功")}</Tag>
                  ) : (
                    <Tag color="error">{t("失败")}</Tag>
                  )}
                  <Typography.Text type="secondary">
                    {dayjs(item.created_at).format("YYYY-MM-DD HH:mm:ss")}
                  </Typography.Text>
                  {item.database_name ? <Tag>{item.database_name}</Tag> : null}
                  <Typography.Text type="secondary">
                    {formatDuration(item.duration_ms)}
                  </Typography.Text>
                </Space>
                <Typography.Paragraph
                  className="db-history-sql"
                  ellipsis={{ rows: 3, tooltip: item.sql_text.slice(0, 2000) }}
                  onClick={() => onUse(item.sql_text)}
                >
                  {item.sql_text}
                </Typography.Paragraph>
                {item.error_message ? (
                  <Typography.Text type="danger" className="db-history-error">
                    {item.error_message}
                  </Typography.Text>
                ) : null}
              </div>
            </List.Item>
          )}
        />
      ) : (
        <Empty description={t("暂无执行历史")} />
      )}
      {history.data && history.data.total > PAGE_SIZE ? (
        <Pagination
          size="small"
          className="db-history-pagination"
          current={page}
          pageSize={PAGE_SIZE}
          total={history.data.total}
          onChange={setPage}
        />
      ) : null}
    </Drawer>
  );
}
