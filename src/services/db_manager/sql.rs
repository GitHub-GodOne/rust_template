//! Dialect details: identifier quoting, placeholders, splitting scripts into
//! statements and rendering SQL literals.

use serde_json::Value;

use crate::errors::{ApiError, ApiResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Dialect {
    Postgres,
    MySql,
    Sqlite,
}

impl Dialect {
    pub fn parse(value: &str) -> ApiResult<Self> {
        match value {
            "postgres" => Ok(Self::Postgres),
            "mysql" => Ok(Self::MySql),
            "sqlite" => Ok(Self::Sqlite),
            _ => Err(ApiError::bad_request(format!(
                "不支持的数据库类型：{value}"
            ))),
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::MySql => "mysql",
            Self::Sqlite => "sqlite",
        }
    }

    #[must_use]
    pub const fn default_port(self) -> Option<u16> {
        match self {
            Self::Postgres => Some(5432),
            Self::MySql => Some(3306),
            Self::Sqlite => None,
        }
    }

    /// Whether objects live in schemas inside a database (PostgreSQL).
    #[must_use]
    pub const fn has_schemas(self) -> bool {
        matches!(self, Self::Postgres)
    }

    #[must_use]
    pub fn quote(self, name: &str) -> String {
        match self {
            Self::MySql => format!("`{}`", name.replace('`', "``")),
            Self::Postgres | Self::Sqlite => format!("\"{}\"", name.replace('"', "\"\"")),
        }
    }

    /// `schema.table` (PostgreSQL), `database.table` (`MySQL`) or `table`.
    #[must_use]
    pub fn qualified(self, database: &str, schema: Option<&str>, table: &str) -> String {
        match self {
            Self::Postgres => format!(
                "{}.{}",
                self.quote(schema.unwrap_or("public")),
                self.quote(table)
            ),
            Self::MySql => format!("{}.{}", self.quote(database), self.quote(table)),
            Self::Sqlite => self.quote(table),
        }
    }

    /// Bind placeholder number `index` (1-based).
    #[must_use]
    pub fn placeholder(self, index: usize) -> String {
        match self {
            Self::Postgres => format!("${index}"),
            Self::MySql | Self::Sqlite => "?".to_string(),
        }
    }

    /// A string literal.
    #[must_use]
    pub fn string_literal(self, value: &str) -> String {
        match self {
            Self::MySql => format!("'{}'", value.replace('\\', "\\\\").replace('\'', "''")),
            Self::Postgres | Self::Sqlite => format!("'{}'", value.replace('\'', "''")),
        }
    }

    /// A cell value as a literal for `INSERT` statements.
    #[must_use]
    pub fn literal(self, value: &Value, binary: bool) -> String {
        match value {
            Value::Null => "NULL".to_string(),
            Value::Bool(flag) => match self {
                Self::Postgres => if *flag { "TRUE" } else { "FALSE" }.to_string(),
                Self::MySql | Self::Sqlite => if *flag { "1" } else { "0" }.to_string(),
            },
            Value::Number(number) => number.to_string(),
            Value::String(text) => {
                if binary {
                    if let Some(hex) = text.strip_prefix("0x") {
                        if hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                            return match self {
                                Self::Postgres => format!("'\\x{hex}'"),
                                Self::MySql | Self::Sqlite => format!("X'{hex}'"),
                            };
                        }
                    }
                }
                self.string_literal(text)
            }
            other => self.string_literal(&other.to_string()),
        }
    }
}

/// First keyword of a statement, upper-cased, skipping leading comments.
#[must_use]
pub fn leading_keyword(statement: &str) -> String {
    let mut rest = statement.trim_start();
    loop {
        if let Some(after) = rest.strip_prefix("--") {
            rest = after
                .split_once('\n')
                .map_or("", |(_, tail)| tail)
                .trim_start();
        } else if let Some(after) = rest.strip_prefix('#') {
            rest = after
                .split_once('\n')
                .map_or("", |(_, tail)| tail)
                .trim_start();
        } else if let Some(after) = rest.strip_prefix("/*") {
            rest = after
                .split_once("*/")
                .map_or("", |(_, tail)| tail)
                .trim_start();
        } else {
            break;
        }
    }
    rest.chars()
        .take_while(|c| c.is_ascii_alphabetic() || *c == '_')
        .collect::<String>()
        .to_ascii_uppercase()
}

/// Statements that produce a result set even when it is empty.
#[must_use]
pub fn returns_rows(statement: &str) -> bool {
    matches!(
        leading_keyword(statement).as_str(),
        "SELECT"
            | "WITH"
            | "SHOW"
            | "VALUES"
            | "TABLE"
            | "EXPLAIN"
            | "DESCRIBE"
            | "DESC"
            | "PRAGMA"
    ) || statement.to_ascii_uppercase().contains("RETURNING")
}

fn starts_with_ignore_case(chars: &[char], word: &str) -> bool {
    word.chars().enumerate().all(|(index, expected)| {
        chars
            .get(index)
            .is_some_and(|c| c.eq_ignore_ascii_case(&expected))
    })
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

fn is_sqlite_trigger(statement: &str) -> bool {
    let words: Vec<String> = statement
        .split_whitespace()
        .take(4)
        .map(str::to_ascii_uppercase)
        .collect();
    words.first().is_some_and(|word| word == "CREATE") && words.iter().any(|word| word == "TRIGGER")
}

/// Splits a script into statements on `;`.
///
/// Semicolons inside strings, quoted identifiers, comments and PostgreSQL
/// dollar quotes are ignored. `MySQL` scripts may switch the delimiter with
/// `DELIMITER //`; `SQLite` triggers keep their `BEGIN … END;` body.
#[must_use]
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
pub fn split_statements(sql: &str, dialect: Dialect) -> Vec<String> {
    let chars: Vec<char> = sql.chars().collect();
    let mut statements = Vec::new();
    let mut current = String::new();
    // Whether `current` holds anything besides whitespace and comments.
    let mut meaningful = false;
    let mut delimiter: Vec<char> = vec![';'];
    let mut index = 0;
    let push = |current: &mut String, meaningful: &mut bool, statements: &mut Vec<String>| {
        let statement = current.trim();
        if *meaningful && !statement.is_empty() {
            statements.push(statement.to_string());
        }
        current.clear();
        *meaningful = false;
    };

    while index < chars.len() {
        let c = chars[index];
        let next = chars.get(index + 1).copied();

        if dialect == Dialect::MySql
            && !meaningful
            && (index == 0 || chars[index - 1] == '\n' || current.trim().is_empty())
            && starts_with_ignore_case(&chars[index..], "DELIMITER ")
        {
            let end = chars[index..]
                .iter()
                .position(|c| *c == '\n')
                .map_or(chars.len(), |offset| index + offset);
            let value: String = chars[index + 10..end].iter().collect();
            let value = value.trim();
            delimiter = if value.is_empty() {
                vec![';']
            } else {
                value.chars().collect()
            };
            current.clear();
            index = end;
            continue;
        }

        // Line comments.
        if (c == '-' && next == Some('-')) || (c == '#' && dialect == Dialect::MySql) {
            while index < chars.len() && chars[index] != '\n' {
                current.push(chars[index]);
                index += 1;
            }
            continue;
        }
        // Block comments (nested in PostgreSQL).
        if c == '/' && next == Some('*') {
            let mut depth = 0;
            while index < chars.len() {
                if chars[index] == '/' && chars.get(index + 1) == Some(&'*') {
                    depth += 1;
                    current.push_str("/*");
                    index += 2;
                    continue;
                }
                if chars[index] == '*' && chars.get(index + 1) == Some(&'/') {
                    depth -= 1;
                    current.push_str("*/");
                    index += 2;
                    if depth == 0 || dialect != Dialect::Postgres {
                        break;
                    }
                    continue;
                }
                current.push(chars[index]);
                index += 1;
            }
            continue;
        }
        // Quoted strings and identifiers.
        if c == '\'' || c == '"' || c == '`' || (c == '[' && dialect == Dialect::Sqlite) {
            meaningful = true;
            let close = if c == '[' { ']' } else { c };
            let backslash = match dialect {
                Dialect::MySql => c != '`',
                Dialect::Postgres => {
                    c == '\''
                        && index > 0
                        && chars[index - 1].eq_ignore_ascii_case(&'e')
                        && (index < 2 || !is_word_char(chars[index - 2]))
                }
                Dialect::Sqlite => false,
            };
            current.push(c);
            index += 1;
            while index < chars.len() {
                let inner = chars[index];
                current.push(inner);
                index += 1;
                if backslash && inner == '\\' {
                    if let Some(escaped) = chars.get(index) {
                        current.push(*escaped);
                        index += 1;
                    }
                    continue;
                }
                if inner == close {
                    if close != ']' && chars.get(index) == Some(&close) {
                        current.push(close);
                        index += 1;
                        continue;
                    }
                    break;
                }
            }
            continue;
        }
        // PostgreSQL dollar quotes: $$ … $$ or $tag$ … $tag$.
        if c == '$'
            && dialect == Dialect::Postgres
            && (index == 0 || !is_word_char(chars[index - 1]))
        {
            let mut end = index + 1;
            while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
                end += 1;
            }
            let tag_valid = end < chars.len()
                && chars[end] == '$'
                && !chars.get(index + 1).is_some_and(char::is_ascii_digit);
            if tag_valid {
                meaningful = true;
                let tag: Vec<char> = chars[index..=end].to_vec();
                current.extend(&tag);
                index = end + 1;
                while index < chars.len() {
                    if chars[index..].starts_with(&tag) {
                        current.extend(&tag);
                        index += tag.len();
                        break;
                    }
                    current.push(chars[index]);
                    index += 1;
                }
                continue;
            }
        }
        if chars[index..].starts_with(&delimiter) {
            let keep_going = dialect == Dialect::Sqlite
                && delimiter == [';']
                && is_sqlite_trigger(&current)
                && !current
                    .trim_end()
                    .rsplit(|c: char| c.is_whitespace() || c == ';')
                    .next()
                    .is_some_and(|word| word.eq_ignore_ascii_case("END"));
            if keep_going {
                current.push(';');
                index += 1;
                continue;
            }
            push(&mut current, &mut meaningful, &mut statements);
            index += delimiter.len();
            continue;
        }
        if !c.is_whitespace() {
            meaningful = true;
        }
        current.push(c);
        index += 1;
    }
    push(&mut current, &mut meaningful, &mut statements);
    statements
}

/// Escapes `%`, `_` and `\` for a `LIKE … ESCAPE '\'` pattern.
#[must_use]
pub fn like_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(c, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_semicolons_outside_quotes_and_comments() {
        let script = "SELECT 'a;b'; -- c;d\nSELECT \"x;y\" /* e;f */ FROM t;;\n  ";
        assert_eq!(
            split_statements(script, Dialect::Postgres),
            vec!["SELECT 'a;b'", "-- c;d\nSELECT \"x;y\" /* e;f */ FROM t"]
        );
        assert!(split_statements("-- only a comment\n", Dialect::Postgres).is_empty());
    }

    #[test]
    fn keeps_postgres_dollar_quoted_bodies() {
        let script = "CREATE FUNCTION f() RETURNS int AS $body$ BEGIN RETURN 1; END; $body$ LANGUAGE plpgsql; SELECT $1";
        let statements = split_statements(script, Dialect::Postgres);
        assert_eq!(statements.len(), 2);
        assert!(statements[0].ends_with("LANGUAGE plpgsql"));
        assert_eq!(statements[1], "SELECT $1");
        assert_eq!(
            split_statements("SELECT E'it\\'s;'; SELECT 2", Dialect::Postgres).len(),
            2
        );
    }

    #[test]
    fn honours_mysql_delimiter_and_escapes() {
        let script = "SELECT 'it\\'s;'; # note;\nDELIMITER //\nCREATE PROCEDURE p() BEGIN SELECT 1; END//\nDELIMITER ;\nSELECT `a;b`";
        let statements = split_statements(script, Dialect::MySql);
        assert_eq!(
            statements,
            vec![
                "SELECT 'it\\'s;'",
                "CREATE PROCEDURE p() BEGIN SELECT 1; END",
                "SELECT `a;b`"
            ]
        );
    }

    #[test]
    fn keeps_sqlite_trigger_bodies() {
        let script = "CREATE TRIGGER t AFTER INSERT ON a BEGIN UPDATE b SET n = n + 1; DELETE FROM c; END; SELECT [x;y] FROM a";
        let statements = split_statements(script, Dialect::Sqlite);
        assert_eq!(statements.len(), 2);
        assert!(statements[0].ends_with("END"));
        assert_eq!(statements[1], "SELECT [x;y] FROM a");
    }

    #[test]
    fn quotes_identifiers_and_literals() {
        assert_eq!(Dialect::Postgres.quote("a\"b"), "\"a\"\"b\"");
        assert_eq!(Dialect::MySql.quote("a`b"), "`a``b`");
        assert_eq!(Dialect::MySql.qualified("db", None, "t"), "`db`.`t`");
        assert_eq!(Dialect::MySql.string_literal("a\\'b"), "'a\\\\''b'");
        assert_eq!(
            Dialect::Postgres.literal(&Value::String("0xff00".into()), true),
            "'\\xff00'"
        );
        assert_eq!(Dialect::Sqlite.literal(&Value::Bool(true), false), "1");
        assert_eq!(like_escape("50%_a\\"), "50\\%\\_a\\\\");
    }

    #[test]
    fn finds_leading_keywords() {
        assert_eq!(leading_keyword("  -- hi\n/* x */ select 1"), "SELECT");
        assert!(returns_rows("WITH a AS (SELECT 1) SELECT * FROM a"));
        assert!(returns_rows("insert into t values (1) returning id"));
        assert!(!returns_rows("UPDATE t SET a = 1"));
    }
}
