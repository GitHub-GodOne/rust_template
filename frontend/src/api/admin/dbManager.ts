import { type ApiResponse, unwrap } from "../auth";
import { apiClient } from "../client";
import { downloadCsv } from "../download";

export type DbType = "postgres" | "mysql" | "sqlite";

export type DbConnection = {
  id: number;
  name: string;
  db_type: DbType;
  host?: string | null;
  port?: number | null;
  username?: string | null;
  /** Default database; the file path for SQLite. */
  database_name?: string | null;
  ssl_mode?: string | null;
  read_only: boolean;
  group_name?: string | null;
  color?: string | null;
  remark?: string | null;
  sort_order: number;
  password_saved: boolean;
  last_connected_at?: string | null;
  created_at: string;
  updated_at: string;
};

export type SaveDbConnectionParams = {
  /** Only for testing: reuse this connection's saved password. */
  id?: number;
  name: string;
  db_type: DbType;
  host?: string | null;
  port?: number | null;
  username?: string | null;
  /** Empty keeps the saved password. */
  password?: string | null;
  clear_password?: boolean;
  database_name?: string | null;
  ssl_mode?: string | null;
  read_only?: boolean;
  group_name?: string | null;
  color?: string | null;
  remark?: string | null;
  sort_order?: number;
};

export type DbConnectInfo = {
  connection_id: number;
  db_type: DbType;
  version: string;
  databases: string[];
  default_database: string;
  has_schemas: boolean;
  read_only: boolean;
};

export type DbObjectKind =
  | "table"
  | "view"
  | "materialized_view"
  | "foreign_table"
  | "sequence"
  | "function"
  | "procedure";

export type DbObject = {
  name: string;
  kind: DbObjectKind;
  rows?: number | null;
  size_bytes?: number | null;
  comment?: string | null;
  engine?: string | null;
  identity?: string | null;
};

export type DbColumn = {
  name: string;
  data_type: string;
  base_type: string;
  nullable: boolean;
  default_value?: string | null;
  comment?: string | null;
  primary_key: boolean;
  auto_increment: boolean;
  binary: boolean;
};

export type DbIndex = {
  name: string;
  columns: string;
  unique: boolean;
  primary: boolean;
  method?: string | null;
};

export type DbForeignKey = {
  name: string;
  columns: string;
  referenced_table: string;
  referenced_columns: string;
  on_update?: string | null;
  on_delete?: string | null;
};

export type DbTableStructure = {
  name: string;
  kind: DbObjectKind;
  comment?: string | null;
  columns: DbColumn[];
  primary_key: string[];
  indexes: DbIndex[];
  foreign_keys: DbForeignKey[];
  ddl: string;
};

export type DbCell = string | number | boolean | null;

export type DbSort = { column: string; desc: boolean };

export type DbFilterOp =
  | "eq"
  | "ne"
  | "gt"
  | "gte"
  | "lt"
  | "lte"
  | "contains"
  | "not_contains"
  | "starts_with"
  | "ends_with"
  | "is_null"
  | "not_null"
  | "is_empty"
  | "not_empty";

export type DbFilter = { column: string; op: DbFilterOp; value?: string };

export type DbLocation = {
  database?: string;
  schema?: string;
};

export type DbRowsQuery = DbLocation & {
  table: string;
  page?: number;
  page_size?: number;
  sort?: DbSort[];
  filters?: DbFilter[];
  where?: string;
};

export type DbRowsPage = {
  columns: DbColumn[];
  primary_key: string[];
  rows: DbCell[][];
  total: number;
  page: number;
  page_size: number;
  sql: string;
  read_only: boolean;
};

export type DbRowChange =
  | { kind: "insert"; values: Record<string, DbCell> }
  | {
      kind: "update";
      key: Record<string, DbCell>;
      values: Record<string, DbCell>;
    }
  | { kind: "delete"; key: Record<string, DbCell> };

export type DbChangeSummary = {
  inserted: number;
  updated: number;
  deleted: number;
};

export type DbResultColumn = { name: string; type_name: string };

export type DbStatementResult = {
  sql: string;
  returns_rows: boolean;
  columns: DbResultColumn[];
  rows: DbCell[][];
  truncated: boolean;
  affected_rows: number;
  duration_ms: number;
  error?: string | null;
};

export type DbExecuteResult = {
  statements: DbStatementResult[];
  statement_count: number;
  in_transaction: boolean;
  duration_ms: number;
};

export type DbExecuteParams = {
  database?: string;
  session_key: string;
  sql: string;
  max_rows?: number;
  timeout_seconds?: number;
  stop_on_error?: boolean;
};

export type DbHistoryItem = {
  id: number;
  connection_id: number;
  database_name?: string | null;
  sql_text: string;
  status: "success" | "failed";
  statement_count: number;
  affected_rows: number;
  duration_ms: number;
  error_message?: string | null;
  created_at: string;
};

export type DbHistoryPage = {
  items: DbHistoryItem[];
  page: number;
  page_size: number;
  total: number;
};

export type DbSavedQuery = {
  id: number;
  connection_id: number;
  name: string;
  database_name?: string | null;
  sql_text: string;
  created_at: string;
  updated_at: string;
};

export type SaveDbQueryParams = {
  connection_id: number;
  name: string;
  database_name?: string | null;
  sql_text: string;
};

export type DbExportFormat = "csv" | "json" | "sql";

const base = "/admin/db-manager";

function rowsParams(query: DbRowsQuery) {
  return {
    database: query.database,
    schema: query.schema,
    table: query.table,
    page: query.page,
    page_size: query.page_size,
    sort: query.sort?.length ? JSON.stringify(query.sort) : undefined,
    filters: query.filters?.length ? JSON.stringify(query.filters) : undefined,
    where: query.where?.trim() ? query.where : undefined,
  };
}

export async function fetchDbConnections() {
  const response = await apiClient.get<ApiResponse<DbConnection[]>>(
    `${base}/connections`,
  );
  return unwrap(response.data);
}

export async function createDbConnection(params: SaveDbConnectionParams) {
  const response = await apiClient.post<ApiResponse<DbConnection>>(
    `${base}/connections`,
    params,
  );
  return unwrap(response.data);
}

export async function updateDbConnection(
  id: number,
  params: SaveDbConnectionParams,
) {
  const response = await apiClient.put<ApiResponse<DbConnection>>(
    `${base}/connections/${id}`,
    params,
  );
  return unwrap(response.data);
}

export async function deleteDbConnection(id: number) {
  await apiClient.delete(`${base}/connections/${id}`);
}

export async function testDbConnection(params: SaveDbConnectionParams) {
  const response = await apiClient.post<
    ApiResponse<{ version: string; duration_ms: number }>
  >(`${base}/connections/test`, params, { timeout: 60_000 });
  return unwrap(response.data);
}

export async function connectDb(id: number) {
  const response = await apiClient.post<ApiResponse<DbConnectInfo>>(
    `${base}/connections/${id}/connect`,
    {},
    { timeout: 60_000 },
  );
  return unwrap(response.data);
}

export async function disconnectDb(id: number) {
  await apiClient.post(`${base}/connections/${id}/disconnect`, {});
}

export async function fetchDbSchemas(id: number, database: string) {
  const response = await apiClient.get<ApiResponse<string[]>>(
    `${base}/connections/${id}/schemas`,
    { params: { database } },
  );
  return unwrap(response.data);
}

export async function fetchDbObjects(id: number, location: DbLocation) {
  const response = await apiClient.get<ApiResponse<DbObject[]>>(
    `${base}/connections/${id}/objects`,
    { params: location },
  );
  return unwrap(response.data);
}

export async function fetchDbStructure(
  id: number,
  location: DbLocation & { table: string },
) {
  const response = await apiClient.get<ApiResponse<DbTableStructure>>(
    `${base}/connections/${id}/structure`,
    { params: location },
  );
  return unwrap(response.data);
}

export async function fetchDbRoutine(
  id: number,
  params: DbLocation & { name: string; kind: string; identity?: string | null },
) {
  const response = await apiClient.get<ApiResponse<{ ddl: string }>>(
    `${base}/connections/${id}/routine`,
    { params },
  );
  return unwrap(response.data).ddl;
}

export async function fetchDbCompletion(id: number, location: DbLocation) {
  const response = await apiClient.get<
    ApiResponse<{ tables: Record<string, string[]> }>
  >(`${base}/connections/${id}/completion`, { params: location });
  return unwrap(response.data).tables;
}

export async function fetchDbRows(id: number, query: DbRowsQuery) {
  const response = await apiClient.get<ApiResponse<DbRowsPage>>(
    `${base}/connections/${id}/rows`,
    { params: rowsParams(query), timeout: 120_000 },
  );
  return unwrap(response.data);
}

export function exportDbRows(
  id: number,
  query: DbRowsQuery,
  format: DbExportFormat,
) {
  return downloadCsv(
    `${base}/connections/${id}/export`,
    {
      ...rowsParams({ ...query, page: undefined, page_size: undefined }),
      format,
    },
    `${query.table}.${format}`,
  );
}

export async function applyDbChanges(
  id: number,
  params: DbLocation & { table: string; changes: DbRowChange[] },
) {
  const response = await apiClient.post<ApiResponse<DbChangeSummary>>(
    `${base}/connections/${id}/changes`,
    params,
  );
  return unwrap(response.data);
}

export async function executeDbSql(id: number, params: DbExecuteParams) {
  const response = await apiClient.post<ApiResponse<DbExecuteResult>>(
    `${base}/connections/${id}/execute`,
    params,
    // The server applies its own per-statement timeout.
    { timeout: 0 },
  );
  return unwrap(response.data);
}

export async function cancelDbSql(id: number, sessionKey: string) {
  const response = await apiClient.post<ApiResponse<{ cancelled: boolean }>>(
    `${base}/connections/${id}/sessions/${sessionKey}/cancel`,
    {},
  );
  return unwrap(response.data).cancelled;
}

export async function closeDbSession(id: number, sessionKey: string) {
  await apiClient.delete(`${base}/connections/${id}/sessions/${sessionKey}`);
}

export async function fetchDbHistory(params: {
  connection_id?: number;
  page?: number;
  page_size?: number;
  keyword?: string;
}) {
  const response = await apiClient.get<ApiResponse<DbHistoryPage>>(
    `${base}/history`,
    { params },
  );
  return unwrap(response.data);
}

export async function clearDbHistory(connectionId?: number) {
  await apiClient.delete(`${base}/history`, {
    params: { connection_id: connectionId },
  });
}

export async function fetchDbSavedQueries(connectionId?: number) {
  const response = await apiClient.get<ApiResponse<DbSavedQuery[]>>(
    `${base}/saved-queries`,
    { params: { connection_id: connectionId } },
  );
  return unwrap(response.data);
}

export async function createDbSavedQuery(params: SaveDbQueryParams) {
  const response = await apiClient.post<ApiResponse<DbSavedQuery>>(
    `${base}/saved-queries`,
    params,
  );
  return unwrap(response.data);
}

export async function updateDbSavedQuery(
  id: number,
  params: SaveDbQueryParams,
) {
  const response = await apiClient.put<ApiResponse<DbSavedQuery>>(
    `${base}/saved-queries/${id}`,
    params,
  );
  return unwrap(response.data);
}

export async function deleteDbSavedQuery(id: number) {
  await apiClient.delete(`${base}/saved-queries/${id}`);
}
