import type {
  DbCell,
  DbColumn,
  DbObjectKind,
  DbType,
} from "../../../api/admin/dbManager";
import { t } from "../../../i18n";

export const DB_TYPE_LABELS: Record<DbType, string> = {
  postgres: "PostgreSQL",
  mysql: "MySQL",
  sqlite: "SQLite",
};

export const DEFAULT_PORTS: Record<DbType, number | undefined> = {
  postgres: 5432,
  mysql: 3306,
  sqlite: undefined,
};

export function quoteIdent(dbType: DbType, name: string) {
  return dbType === "mysql"
    ? `\`${name.replaceAll("`", "``")}\``
    : `"${name.replaceAll('"', '""')}"`;
}

/** `schema.table`, `database.table` or `table`, quoted. */
export function qualifiedName(
  dbType: DbType,
  table: string,
  database?: string,
  schema?: string,
) {
  if (dbType === "postgres") {
    return `${quoteIdent(dbType, schema || "public")}.${quoteIdent(dbType, table)}`;
  }
  if (dbType === "mysql" && database) {
    return `${quoteIdent(dbType, database)}.${quoteIdent(dbType, table)}`;
  }
  return quoteIdent(dbType, table);
}

export function stringLiteral(dbType: DbType, value: string) {
  const escaped = dbType === "mysql" ? value.replaceAll("\\", "\\\\") : value;
  return `'${escaped.replaceAll("'", "''")}'`;
}

export function selectTemplate(
  dbType: DbType,
  table: string,
  database?: string,
  schema?: string,
) {
  return `SELECT * FROM ${qualifiedName(dbType, table, database, schema)} LIMIT 100;`;
}

export function insertTemplate(
  dbType: DbType,
  table: string,
  columns: DbColumn[],
  database?: string,
  schema?: string,
) {
  const writable = columns.filter((column) => !column.auto_increment);
  const names = writable.map((column) => quoteIdent(dbType, column.name));
  const values = writable.map(() => "?");
  return `INSERT INTO ${qualifiedName(dbType, table, database, schema)} (${names.join(", ")})\nVALUES (${values.join(", ")});`;
}

export function updateTemplate(
  dbType: DbType,
  table: string,
  columns: DbColumn[],
  database?: string,
  schema?: string,
) {
  const keys = columns.filter((column) => column.primary_key);
  const sets = columns
    .filter((column) => !column.primary_key)
    .map((column) => `${quoteIdent(dbType, column.name)} = ?`);
  const where = (keys.length ? keys : columns.slice(0, 1)).map(
    (column) => `${quoteIdent(dbType, column.name)} = ?`,
  );
  return `UPDATE ${qualifiedName(dbType, table, database, schema)}\nSET ${sets.join(",\n    ")}\nWHERE ${where.join(" AND ")};`;
}

export function deleteTemplate(
  dbType: DbType,
  table: string,
  columns: DbColumn[],
  database?: string,
  schema?: string,
) {
  const keys = columns.filter((column) => column.primary_key);
  const where = (keys.length ? keys : columns.slice(0, 1)).map(
    (column) => `${quoteIdent(dbType, column.name)} = ?`,
  );
  return `DELETE FROM ${qualifiedName(dbType, table, database, schema)}\nWHERE ${where.join(" AND ")};`;
}

export function truncateStatement(
  dbType: DbType,
  table: string,
  database?: string,
  schema?: string,
) {
  const name = qualifiedName(dbType, table, database, schema);
  return dbType === "sqlite" ? `DELETE FROM ${name}` : `TRUNCATE TABLE ${name}`;
}

export function dropStatement(
  dbType: DbType,
  kind: DbObjectKind,
  table: string,
  database?: string,
  schema?: string,
) {
  const name = qualifiedName(dbType, table, database, schema);
  const keyword =
    kind === "view"
      ? "VIEW"
      : kind === "materialized_view"
        ? "MATERIALIZED VIEW"
        : kind === "sequence"
          ? "SEQUENCE"
          : kind === "foreign_table"
            ? "FOREIGN TABLE"
            : "TABLE";
  return `DROP ${keyword} ${name}`;
}

export function explainPrefix(dbType: DbType) {
  return dbType === "sqlite" ? "EXPLAIN QUERY PLAN " : "EXPLAIN ";
}

export function isTableLike(kind: DbObjectKind) {
  return (
    kind === "table" ||
    kind === "view" ||
    kind === "materialized_view" ||
    kind === "foreign_table"
  );
}

export function objectKindLabel(kind: DbObjectKind) {
  switch (kind) {
    case "table":
      return t("表");
    case "view":
      return t("视图");
    case "materialized_view":
      return t("物化视图");
    case "foreign_table":
      return t("外部表");
    case "sequence":
      return t("序列");
    case "function":
      return t("函数");
    case "procedure":
      return t("存储过程");
  }
}

export function formatCell(value: DbCell) {
  if (value === null) return "NULL";
  if (typeof value === "boolean") return value ? "true" : "false";
  return String(value);
}

export function formatBytes(bytes?: number | null) {
  if (bytes === null || bytes === undefined) return "";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unit]}`;
}

export function formatDuration(ms: number) {
  if (ms < 1000) return `${ms} ms`;
  return `${(ms / 1000).toFixed(ms < 10_000 ? 2 : 1)} s`;
}

function csvField(value: string) {
  const safe = /^[=+\-@\t\r]/.test(value) ? `'${value}` : value;
  return /[",\n\r]/.test(safe) ? `"${safe.replaceAll('"', '""')}"` : safe;
}

function saveBlob(content: string, name: string, type: string) {
  const url = URL.createObjectURL(new Blob([content], { type }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = name;
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}

/** Downloads a result set as CSV (UTF-8 with BOM, like server exports). */
export function downloadResultCsv(
  columns: string[],
  rows: DbCell[][],
  name: string,
) {
  const lines = [columns.map(csvField).join(",")];
  for (const row of rows) {
    lines.push(
      row
        .map((cell) => (cell === null ? "" : csvField(formatCell(cell))))
        .join(","),
    );
  }
  saveBlob(`﻿${lines.join("\r\n")}\r\n`, `${name}.csv`, "text/csv");
}

export function downloadResultJson(
  columns: string[],
  rows: DbCell[][],
  name: string,
) {
  const items = rows.map((row) =>
    Object.fromEntries(columns.map((column, index) => [column, row[index]])),
  );
  saveBlob(JSON.stringify(items, null, 2), `${name}.json`, "application/json");
}

export type StatementRange = { from: number; to: number; text: string };

/**
 * Splits a script into statements on `;` outside quotes, comments and
 * PostgreSQL dollar quotes (a lighter version of the server's splitter).
 */
export function statementRanges(sql: string, dbType: DbType): StatementRange[] {
  const ranges: StatementRange[] = [];
  let start = 0;
  let index = 0;
  const push = (end: number) => {
    const text = sql.slice(start, end);
    const trimmed = text.trim();
    if (trimmed && !/^(--[^\n]*\n?|\/\*[\s\S]*?\*\/|\s)*$/.test(text)) {
      const offset = text.indexOf(trimmed);
      ranges.push({
        from: start + offset,
        to: start + offset + trimmed.length,
        text: trimmed,
      });
    }
  };
  while (index < sql.length) {
    const char = sql[index];
    const next = sql[index + 1];
    if (
      (char === "-" && next === "-") ||
      (char === "#" && dbType === "mysql")
    ) {
      const end = sql.indexOf("\n", index);
      index = end === -1 ? sql.length : end + 1;
      continue;
    }
    if (char === "/" && next === "*") {
      const end = sql.indexOf("*/", index + 2);
      index = end === -1 ? sql.length : end + 2;
      continue;
    }
    if (char === "'" || char === '"' || char === "`") {
      index += 1;
      while (index < sql.length) {
        if (sql[index] === "\\" && dbType === "mysql") {
          index += 2;
          continue;
        }
        if (sql[index] === char) {
          if (sql[index + 1] === char) {
            index += 2;
            continue;
          }
          break;
        }
        index += 1;
      }
      index += 1;
      continue;
    }
    if (char === "$" && dbType === "postgres") {
      const match = /^\$[A-Za-z_]?[A-Za-z0-9_]*\$/.exec(sql.slice(index));
      if (match && !/\w/.test(sql[index - 1] ?? "")) {
        const end = sql.indexOf(match[0], index + match[0].length);
        index = end === -1 ? sql.length : end + match[0].length;
        continue;
      }
    }
    if (char === ";") {
      push(index);
      start = index + 1;
    }
    index += 1;
  }
  push(sql.length);
  return ranges;
}

/** The statement under the cursor (or the one just before it). */
export function statementAt(sql: string, offset: number, dbType: DbType) {
  const ranges = statementRanges(sql, dbType);
  return (
    ranges.find((range) => offset >= range.from && offset <= range.to + 1) ??
    [...ranges].reverse().find((range) => range.to <= offset) ??
    ranges[0]
  );
}

export function newSessionKey() {
  const random =
    typeof crypto !== "undefined" && "randomUUID" in crypto
      ? crypto.randomUUID()
      : `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  return `tab-${random}`;
}
