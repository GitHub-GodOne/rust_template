import {
  CheckOutlined,
  CodeOutlined,
  CopyOutlined,
  KeyOutlined,
  ReloadOutlined,
} from "@ant-design/icons";
import { useQuery } from "@tanstack/react-query";
import {
  Alert,
  Button,
  Space,
  Spin,
  Table,
  Tabs,
  Tag,
  Typography,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import {
  type DbColumn,
  type DbForeignKey,
  type DbIndex,
  type DbType,
  fetchDbStructure,
} from "../../../api/admin/dbManager";
import { t } from "../../../i18n";
import { SqlEditor } from "./SqlEditor";

function copy(text: string) {
  void navigator.clipboard
    ?.writeText(text)
    .then(() => message.success(t("已复制")));
}

/** Read-only SQL with copy and "open in a query tab". */
export function DdlView({
  ddl,
  dbType,
  onOpenQuery,
}: {
  ddl: string;
  dbType: DbType;
  onOpenQuery?: (sql: string) => void;
}) {
  return (
    <div className="db-ddl-view">
      <Space size={6} className="db-ddl-actions">
        <Button size="small" icon={<CopyOutlined />} onClick={() => copy(ddl)}>
          {t("复制")}
        </Button>
        {onOpenQuery ? (
          <Button
            size="small"
            icon={<CodeOutlined />}
            onClick={() => onOpenQuery(ddl)}
          >
            {t("在查询中打开")}
          </Button>
        ) : null}
      </Space>
      {/* Remount when the text changes: the editor only reads its first value. */}
      <SqlEditor key={ddl} initialValue={ddl} dbType={dbType} readOnly />
    </div>
  );
}

const yes = <CheckOutlined className="db-yes" />;

export function StructureView({
  connectionId,
  dbType,
  database,
  schema,
  table,
  onOpenQuery,
}: {
  connectionId: number;
  dbType: DbType;
  database: string;
  schema?: string;
  table: string;
  onOpenQuery: (sql: string) => void;
}) {
  const query = useQuery({
    queryKey: ["db-structure", connectionId, database, schema, table],
    queryFn: () => fetchDbStructure(connectionId, { database, schema, table }),
    retry: false,
  });
  const structure = query.data;

  const columnColumns: ColumnsType<DbColumn> = [
    {
      title: "#",
      key: "index",
      width: 48,
      render: (_, __, index) => index + 1,
    },
    {
      title: t("字段名"),
      dataIndex: "name",
      render: (name: string, column) => (
        <Space size={4}>
          {column.primary_key ? <KeyOutlined className="db-pk-icon" /> : null}
          <Typography.Text strong={column.primary_key}>{name}</Typography.Text>
        </Space>
      ),
    },
    { title: t("类型"), dataIndex: "data_type" },
    {
      title: t("允许 NULL"),
      dataIndex: "nullable",
      width: 90,
      align: "center",
      render: (value: boolean) => (value ? yes : null),
    },
    {
      title: t("默认值"),
      dataIndex: "default_value",
      ellipsis: true,
      render: (value?: string | null) =>
        value ? <Typography.Text code>{value}</Typography.Text> : null,
    },
    {
      title: t("自增"),
      dataIndex: "auto_increment",
      width: 70,
      align: "center",
      render: (value: boolean) => (value ? yes : null),
    },
    { title: t("注释"), dataIndex: "comment", ellipsis: true },
  ];

  const indexColumns: ColumnsType<DbIndex> = [
    { title: t("索引名"), dataIndex: "name" },
    { title: t("字段"), dataIndex: "columns" },
    {
      title: t("类型"),
      key: "type",
      render: (_, index) =>
        index.primary ? (
          <Tag color="gold">PRIMARY</Tag>
        ) : index.unique ? (
          <Tag color="blue">UNIQUE</Tag>
        ) : (
          <Tag>INDEX</Tag>
        ),
    },
    { title: t("方法"), dataIndex: "method" },
  ];

  const foreignKeyColumns: ColumnsType<DbForeignKey> = [
    { title: t("外键名"), dataIndex: "name" },
    { title: t("字段"), dataIndex: "columns" },
    { title: t("引用表"), dataIndex: "referenced_table" },
    { title: t("引用字段"), dataIndex: "referenced_columns" },
    { title: t("更新时"), dataIndex: "on_update" },
    { title: t("删除时"), dataIndex: "on_delete" },
  ];

  if (query.error) {
    return (
      <Alert
        type="error"
        showIcon
        message={(query.error as Error).message}
        action={
          <Button size="small" onClick={() => void query.refetch()}>
            {t("重试")}
          </Button>
        }
      />
    );
  }
  if (!structure) {
    return <Spin className="db-center-spin" />;
  }

  return (
    <div className="db-structure-view">
      <div className="db-toolbar">
        <Space size={8} wrap>
          <Typography.Text strong>{structure.name}</Typography.Text>
          {structure.comment ? (
            <Typography.Text type="secondary">
              {structure.comment}
            </Typography.Text>
          ) : null}
          <Button
            size="small"
            icon={<ReloadOutlined />}
            loading={query.isFetching}
            onClick={() => void query.refetch()}
          >
            {t("刷新")}
          </Button>
        </Space>
      </div>
      <Tabs
        size="small"
        className="db-structure-tabs"
        items={[
          ...(structure.kind === "sequence"
            ? []
            : [
                {
                  key: "columns",
                  label: t("字段（{0}）", structure.columns.length),
                  children: (
                    <Table<DbColumn>
                      size="small"
                      rowKey="name"
                      columns={columnColumns}
                      dataSource={structure.columns}
                      pagination={false}
                      scroll={{ x: 760 }}
                    />
                  ),
                },
                {
                  key: "indexes",
                  label: t("索引（{0}）", structure.indexes.length),
                  children: (
                    <Table<DbIndex>
                      size="small"
                      rowKey="name"
                      columns={indexColumns}
                      dataSource={structure.indexes}
                      pagination={false}
                    />
                  ),
                },
                {
                  key: "foreign-keys",
                  label: t("外键（{0}）", structure.foreign_keys.length),
                  children: (
                    <Table<DbForeignKey>
                      size="small"
                      rowKey="name"
                      columns={foreignKeyColumns}
                      dataSource={structure.foreign_keys}
                      pagination={false}
                      scroll={{ x: 760 }}
                    />
                  ),
                },
              ]),
          {
            key: "ddl",
            label: "DDL",
            children: (
              <DdlView
                ddl={structure.ddl}
                dbType={dbType}
                onOpenQuery={onOpenQuery}
              />
            ),
          },
        ]}
      />
    </div>
  );
}
