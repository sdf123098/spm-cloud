//! Storage boundary preserving typed rows and transaction scopes for SQLite and MySQL.
use mysql::prelude::Queryable;
use rusqlite::{
    Error, Result,
    types::{FromSql, ToSql, ToSqlOutput, Value, ValueRef},
};
use std::{
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

pub trait Bindings {
    fn values(self) -> Result<Vec<Value>>;
}
fn value(input: &dyn ToSql) -> Result<Value> {
    match input.to_sql()? {
        ToSqlOutput::Owned(value) => Ok(value),
        ToSqlOutput::Borrowed(ValueRef::Null) => Ok(Value::Null),
        ToSqlOutput::Borrowed(ValueRef::Integer(v)) => Ok(Value::Integer(v)),
        ToSqlOutput::Borrowed(ValueRef::Real(v)) => Ok(Value::Real(v)),
        ToSqlOutput::Borrowed(ValueRef::Text(v)) => {
            Ok(Value::Text(String::from_utf8_lossy(v).into_owned()))
        }
        ToSqlOutput::Borrowed(ValueRef::Blob(v)) => Ok(Value::Blob(v.to_vec())),
        _ => Err(Error::InvalidQuery),
    }
}
impl Bindings for [(); 0] {
    fn values(self) -> Result<Vec<Value>> {
        Ok(Vec::new())
    }
}
impl Bindings for &[&dyn ToSql] {
    fn values(self) -> Result<Vec<Value>> {
        self.iter().map(|v| value(*v)).collect()
    }
}
impl Bindings for Vec<Value> {
    fn values(self) -> Result<Vec<Value>> {
        Ok(self)
    }
}
macro_rules! arrays { ($($n:literal),*) => { $(impl<T: ToSql> Bindings for [T; $n] { fn values(self) -> Result<Vec<Value>> { self.iter().map(|v| value(v)).collect() } })* }; }
arrays!(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32
);

enum Backend {
    Sqlite(rusqlite::Connection),
    Mysql(Mutex<mysql::Conn>),
}
pub struct Connection {
    backend: Backend,
    changes: AtomicU64,
}
pub struct Row {
    values: Vec<Value>,
}
impl Row {
    pub fn get<I: TryInto<usize>, T: FromSql>(&self, index: I) -> Result<T> {
        let index = index
            .try_into()
            .map_err(|_| Error::InvalidColumnIndex(usize::MAX))?;
        let value = self
            .values
            .get(index)
            .ok_or(Error::InvalidColumnIndex(index))?;
        T::column_result(ValueRef::from(value))
            .map_err(|e| Error::FromSqlConversionFailure(index, value.data_type(), Box::new(e)))
    }
}
pub struct Statement<'a> {
    connection: &'a Connection,
    sql: String,
}
impl Statement<'_> {
    pub fn query_map<P: Bindings, T, F: FnMut(&Row) -> Result<T>>(
        &mut self,
        params: P,
        mut map: F,
    ) -> Result<std::vec::IntoIter<Result<T>>> {
        Ok(self
            .connection
            .rows(&self.sql, params)?
            .iter()
            .map(&mut map)
            .collect::<Vec<_>>()
            .into_iter())
    }
}
pub struct Transaction<'a> {
    connection: &'a Connection,
    complete: bool,
}
impl std::ops::Deref for Transaction<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.connection
    }
}
impl Transaction<'_> {
    pub fn commit(mut self) -> Result<()> {
        self.connection.execute_batch("COMMIT")?;
        self.complete = true;
        Ok(())
    }
}
impl Drop for Transaction<'_> {
    fn drop(&mut self) {
        if !self.complete {
            let _ = self.connection.execute_batch("ROLLBACK");
        }
    }
}

fn mysql_error(error: mysql::Error) -> Error {
    // Do not expose connection options or server messages containing bound data.
    let detail = match &error {
        mysql::Error::MySqlError(e) => format!("MySQL error {} ({})", e.code, e.state),
        _ => "MySQL connection/query failed".into(),
    };
    Error::ToSqlConversionFailure(Box::new(std::io::Error::other(detail)))
}
impl Connection {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self {
            backend: Backend::Sqlite(rusqlite::Connection::open(path)?),
            changes: AtomicU64::new(0),
        })
    }
    pub fn open_in_memory() -> Result<Self> {
        Ok(Self {
            backend: Backend::Sqlite(rusqlite::Connection::open_in_memory()?),
            changes: AtomicU64::new(0),
        })
    }
    pub fn mysql(url: &str) -> Result<Self> {
        let options = mysql::Opts::from_url(url).map_err(|_| Error::InvalidQuery)?;
        if options.get_db_name().is_none() {
            return Err(Error::InvalidQuery);
        }
        let options = mysql::OptsBuilder::from_opts(options)
            .tcp_connect_timeout(Some(Duration::from_secs(5)))
            .read_timeout(Some(Duration::from_secs(10)))
            .write_timeout(Some(Duration::from_secs(10)));
        let mut connection = mysql::Conn::new(options).map_err(mysql_error)?;
        connection
            .query_drop(
                "SET time_zone='+00:00', sql_mode='STRICT_TRANS_TABLES,NO_ENGINE_SUBSTITUTION'",
            )
            .map_err(mysql_error)?;
        connection
            .query_drop("SET NAMES utf8mb4 COLLATE utf8mb4_bin")
            .map_err(mysql_error)?;
        Ok(Self {
            backend: Backend::Mysql(Mutex::new(connection)),
            changes: AtomicU64::new(0),
        })
    }
    pub fn is_mysql(&self) -> bool {
        matches!(self.backend, Backend::Mysql(_))
    }
    pub fn total_changes(&self) -> u64 {
        self.changes.load(Ordering::Relaxed)
    }
    pub fn external_version(&self) -> Result<i64> {
        match &self.backend {
            Backend::Sqlite(conn) => conn.query_row("PRAGMA data_version", [], |r| r.get(0)),
            Backend::Mysql(_) => Ok(0),
        }
    }
    pub fn pragma_update<T: ToSql>(
        &self,
        schema: Option<&str>,
        name: &str,
        setting: T,
    ) -> Result<()> {
        match &self.backend {
            Backend::Sqlite(conn) => conn.pragma_update(schema, name, setting),
            Backend::Mysql(_) => Ok(()),
        }
    }
    pub fn busy_timeout(&self, timeout: Duration) -> Result<()> {
        match &self.backend {
            Backend::Sqlite(conn) => conn.busy_timeout(timeout),
            Backend::Mysql(_) => Ok(()),
        }
    }
    pub fn transaction(&mut self) -> Result<Transaction<'_>> {
        self.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
    }
    pub fn transaction_with_behavior(
        &mut self,
        behavior: rusqlite::TransactionBehavior,
    ) -> Result<Transaction<'_>> {
        if self.is_mysql() {
            self.execute_batch("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE; START TRANSACTION")?;
        } else {
            self.execute_batch(match behavior {
                rusqlite::TransactionBehavior::Deferred => "BEGIN DEFERRED",
                rusqlite::TransactionBehavior::Exclusive => "BEGIN EXCLUSIVE",
                _ => "BEGIN IMMEDIATE",
            })?;
        }
        Ok(Transaction {
            connection: self,
            complete: false,
        })
    }
    pub fn prepare(&self, sql: &str) -> Result<Statement<'_>> {
        Ok(Statement {
            connection: self,
            sql: sql.into(),
        })
    }
    pub fn query_row<P: Bindings, T, F: FnOnce(&Row) -> Result<T>>(
        &self,
        sql: &str,
        params: P,
        map: F,
    ) -> Result<T> {
        let rows = self.rows(sql, params)?;
        map(rows.first().ok_or(Error::QueryReturnedNoRows)?)
    }
    pub fn execute<P: Bindings>(&self, sql: &str, params: P) -> Result<usize> {
        let values = params.values()?;
        let changed = match &self.backend {
            Backend::Sqlite(conn) => conn.execute(sql, rusqlite::params_from_iter(values))?,
            Backend::Mysql(conn) => {
                let schema_sql =
                    if sql.starts_with("CREATE TABLE") || sql.starts_with("ALTER TABLE") {
                        mysql_schema(sql)
                    } else {
                        sql.to_owned()
                    };
                let (sql, indices) = mysql_sql(&schema_sql);
                let values = indices
                    .into_iter()
                    .map(|i| {
                        values
                            .get(i)
                            .map(mysql_value)
                            .ok_or(Error::InvalidParameterCount(i + 1, values.len()))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let mut conn = conn.lock().unwrap();
                if let Err(error) = conn.exec_drop(&sql, mysql::Params::Positional(values)) {
                    return Err(mysql_error(error));
                }
                conn.affected_rows() as usize
            }
        };
        self.changes.fetch_add(changed as u64, Ordering::Relaxed);
        Ok(changed)
    }
    pub fn execute_batch(&self, sql: &str) -> Result<()> {
        match &self.backend {
            Backend::Sqlite(conn) => conn.execute_batch(sql)?,
            Backend::Mysql(conn) => {
                let mut conn = conn.lock().unwrap();
                for statement in split_statements(sql) {
                    let statement = mysql_schema(&statement);
                    if statement.is_empty() {
                        continue;
                    }
                    if let Err(error) = conn.query_drop(&statement) {
                        // MySQL has no CREATE INDEX IF NOT EXISTS; ignore only its duplicate-name code.
                        if (statement.starts_with("CREATE INDEX")
                            || statement.starts_with("CREATE UNIQUE INDEX"))
                            && matches!(&error, mysql::Error::MySqlError(e) if e.code == 1061)
                        {
                            continue;
                        }
                        return Err(mysql_error(error));
                    }
                }
            }
        }
        self.changes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn rows<P: Bindings>(&self, sql: &str, params: P) -> Result<Vec<Row>> {
        let values = params.values()?;
        match &self.backend {
            Backend::Sqlite(conn) => {
                let mut statement = conn.prepare(sql)?;
                let columns = statement.column_count();
                statement
                    .query_map(rusqlite::params_from_iter(values), |r| {
                        Ok(Row {
                            values: (0..columns).map(|i| r.get(i)).collect::<Result<Vec<_>>>()?,
                        })
                    })?
                    .collect()
            }
            Backend::Mysql(conn) => {
                let (sql, indices) = mysql_sql(sql);
                let values = indices
                    .into_iter()
                    .map(|i| {
                        values
                            .get(i)
                            .map(mysql_value)
                            .ok_or(Error::InvalidParameterCount(i + 1, values.len()))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let rows: Vec<mysql::Row> = match conn
                    .lock()
                    .unwrap()
                    .exec(&sql, mysql::Params::Positional(values))
                {
                    Ok(rows) => rows,
                    Err(error) => return Err(mysql_error(error)),
                };
                rows.into_iter()
                    .map(|row| {
                        Ok(Row {
                            values: row
                                .unwrap()
                                .into_iter()
                                .map(sqlite_value)
                                .collect::<Result<Vec<_>>>()?,
                        })
                    })
                    .collect()
            }
        }
    }
    pub fn snapshot_sqlite(&self, path: &Path) -> Result<()> {
        if !self.is_mysql() {
            return self
                .execute("VACUUM INTO ?1", [path.to_string_lossy().as_ref()])
                .map(|_| ());
        }
        let mut snapshot = Self::open(path)?;
        crate::store::CloudStore::migrate(&snapshot)
            .map_err(|e| Error::ToSqlConversionFailure(Box::new(e)))?;
        snapshot.pragma_update(None, "foreign_keys", "OFF")?;
        self.execute_batch("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ; START TRANSACTION WITH CONSISTENT SNAPSHOT")?;
        let result = (|| {
            let tables = snapshot.rows("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name", [])?;
            let tx = snapshot.transaction()?;
            for table in tables {
                let table: String = table.get(0)?;
                let cols = tx
                    .rows(
                        &format!("SELECT name FROM pragma_table_info('{table}')"),
                        [],
                    )?
                    .into_iter()
                    .map(|r| r.get::<_, String>(0))
                    .collect::<Result<Vec<_>>>()?;
                let columns = cols
                    .iter()
                    .map(|c| format!("`{c}`"))
                    .collect::<Vec<_>>()
                    .join(",");
                tx.execute(&format!("DELETE FROM `{table}`"), [])?;
                let placeholders = (1..=cols.len())
                    .map(|n| format!("?{n}"))
                    .collect::<Vec<_>>()
                    .join(",");
                let mut offset = 0i64;
                loop {
                    let rows = self.rows(
                        &format!("SELECT {columns} FROM `{table}` LIMIT ?1 OFFSET ?2"),
                        [256i64, offset],
                    )?;
                    let count = rows.len();
                    for row in rows {
                        tx.execute(
                            &format!("INSERT INTO `{table}` ({columns}) VALUES ({placeholders})"),
                            row.values,
                        )?;
                    }
                    if count < 256 {
                        break;
                    }
                    offset += 256;
                }
            }
            tx.commit()
        })();
        let end = self.execute_batch(if result.is_ok() { "COMMIT" } else { "ROLLBACK" });
        result.and(end)
    }
    /// Imports the previous local SQLite store once, when the configured MySQL schema is empty.
    pub fn import_sqlite_if_empty(&self, source_path: &Path) -> Result<bool> {
        if !self.is_mysql() || !source_path.is_file() {
            return Ok(false);
        }
        let source = rusqlite::Connection::open(source_path)?;
        let has_accounts: bool = source.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='accounts')",
            [],
            |r| r.get(0),
        )?;
        if !has_accounts {
            return Ok(false);
        }
        let source_accounts: i64 =
            source.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?;
        if source_accounts == 0 {
            return Ok(false);
        }
        let account_count: i64 =
            self.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?;
        let asset_count: i64 = self.query_row("SELECT COUNT(*) FROM assets", [], |r| r.get(0))?;
        if account_count != 0 || asset_count != 0 {
            return Ok(false);
        }

        self.execute_batch("SET FOREIGN_KEY_CHECKS=0; START TRANSACTION")?;
        let result = (|| {
            let tables = source.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?
                .query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            for table in tables {
                let escaped = table.replace('`', "``");
                let columns = source
                    .prepare(&format!(
                        "SELECT name FROM pragma_table_info('{}')",
                        table.replace('\'', "''")
                    ))?
                    .query_map([], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                if columns.is_empty() {
                    continue;
                }
                let quoted = columns
                    .iter()
                    .map(|c| format!("`{}`", c.replace('`', "``")))
                    .collect::<Vec<_>>()
                    .join(",");
                let placeholders = (1..=columns.len())
                    .map(|i| format!("?{i}"))
                    .collect::<Vec<_>>()
                    .join(",");
                let mut offset = 0i64;
                loop {
                    let mut statement = source.prepare(&format!(
                        "SELECT {quoted} FROM `{escaped}` LIMIT 256 OFFSET ?1"
                    ))?;
                    let batch = statement
                        .query_map([offset], |row| {
                            (0..columns.len())
                                .map(|i| row.get::<_, Value>(i))
                                .collect::<rusqlite::Result<Vec<_>>>()
                        })?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    let count = batch.len();
                    for values in batch {
                        self.execute(
                            &format!("REPLACE INTO `{escaped}` ({quoted}) VALUES ({placeholders})"),
                            values,
                        )?;
                    }
                    if count < 256 {
                        break;
                    }
                    offset += 256;
                }
            }
            Ok(())
        })();
        match result {
            Ok(()) => self.execute_batch("COMMIT")?,
            Err(error) => {
                let _ = self.execute_batch("ROLLBACK");
                let _ = self.execute_batch("SET FOREIGN_KEY_CHECKS=1");
                return Err(error);
            }
        }
        self.execute_batch("SET FOREIGN_KEY_CHECKS=1")?;
        Ok(true)
    }
}
fn mysql_value(v: &Value) -> mysql::Value {
    match v {
        Value::Null => mysql::Value::NULL,
        Value::Integer(v) => mysql::Value::Int(*v),
        Value::Real(v) => mysql::Value::Double(*v),
        Value::Text(v) => mysql::Value::Bytes(v.as_bytes().to_vec()),
        Value::Blob(v) => mysql::Value::Bytes(v.clone()),
    }
}
fn sqlite_value(v: mysql::Value) -> Result<Value> {
    Ok(match v {
        mysql::Value::NULL => Value::Null,
        mysql::Value::Int(v) => Value::Integer(v),
        mysql::Value::UInt(v) => Value::Integer(
            i64::try_from(v).map_err(|_| Error::IntegralValueOutOfRange(0, v as i64))?,
        ),
        mysql::Value::Float(v) => Value::Real(v as f64),
        mysql::Value::Double(v) => Value::Real(v),
        mysql::Value::Bytes(v) => Value::Text(String::from_utf8(v).map_err(|e| {
            Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?),
        _ => return Err(Error::InvalidQuery),
    })
}

/// Bind repeated/out-of-order SQLite parameters as separate MySQL positional parameters; skip SQL literals.
pub fn mysql_sql(input: &str) -> (String, Vec<usize>) {
    let mut sql = input
        .replace(
            "datetime('now')",
            "DATE_FORMAT(UTC_TIMESTAMP(),'%Y-%m-%d %H:%i:%s')",
        )
        .replace("INSERT OR IGNORE", "INSERT IGNORE")
        .replace("INSERT OR REPLACE INTO", "REPLACE INTO");
    let conflict =
        regex::Regex::new(r"(?is)\s+ON CONFLICT\s*\([^)]+\)\s+DO UPDATE SET\s+").unwrap();
    sql = conflict
        .replace_all(&sql, " ON DUPLICATE KEY UPDATE ")
        .into_owned();
    let nothing = regex::Regex::new(r"(?is)\s+ON CONFLICT\s*\([^)]+\)\s+DO NOTHING").unwrap();
    if nothing.is_match(&sql) {
        sql = nothing.replace_all(&sql, "").into_owned().replacen(
            "INSERT INTO",
            "INSERT IGNORE INTO",
            1,
        );
    }
    sql = regex::Regex::new(r"excluded\.([A-Za-z_][A-Za-z_0-9]*)")
        .unwrap()
        .replace_all(&sql, "VALUES($1)")
        .into_owned();
    // Audit IDs provide a portable stable tie-breaker where SQLite used its implicit rowid.
    sql = sql.replace("rowid DESC", "event_id DESC");
    if sql.contains("pragma_table_info(") {
        sql=regex::Regex::new(r"pragma_table_info\('([^']+)'\)").unwrap().replace_all(&sql,"(SELECT COLUMN_NAME AS name FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=DATABASE() AND TABLE_NAME='$1') AS columns_info").into_owned();
    }
    // The SQLite-only unique-SHA migration cannot exist in a native MySQL schema.
    if sql.contains("pragma_index_list('asset_revisions')") {
        return ("SELECT 0".into(), Vec::new());
    }
    let chars = sql.char_indices().collect::<Vec<_>>();
    let mut out = String::new();
    let mut indices = Vec::new();
    let mut i = 0;
    let mut quote = '\0';
    let mut anonymous = 0;
    while i < chars.len() {
        let b = chars[i].1;
        if quote != '\0' {
            out.push(b);
            i += 1;
            if b == quote {
                if i < chars.len() && chars[i].1 == quote {
                    out.push(quote);
                    i += 1;
                } else {
                    quote = '\0';
                }
            }
            continue;
        }
        if b == '\'' || b == '"' || b == '`' {
            quote = b;
            out.push(b);
            i += 1;
            continue;
        }
        if b == '?' {
            i += 1;
            let digit_start = i;
            while i < chars.len() && chars[i].1.is_ascii_digit() {
                i += 1;
            }
            let index = if i == digit_start {
                let n = anonymous;
                anonymous += 1;
                n
            } else {
                sql[chars[digit_start].0..chars[i - 1].0 + chars[i - 1].1.len_utf8()]
                    .parse::<usize>()
                    .unwrap()
                    - 1
            };
            indices.push(index);
            out.push('?');
            continue;
        }
        out.push(b);
        i += 1;
    }
    (out, indices)
}
fn split_statements(sql: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut quote = '\0';
    let mut start = 0;
    for (i, ch) in sql.char_indices() {
        if quote == '\0' && (ch == '\'' || ch == '"' || ch == '`') {
            quote = ch;
        } else if ch == quote {
            quote = '\0';
        } else if quote == '\0' && ch == ';' {
            let s = sql[start..i].trim();
            if !s.is_empty() {
                result.push(s.into());
            }
            start = i + 1;
        }
    }
    if !sql[start..].trim().is_empty() {
        result.push(sql[start..].trim().into());
    }
    result
}
fn mysql_schema(sql: &str) -> String {
    let mut sql = if sql.starts_with("CREATE TABLE") || sql.starts_with("ALTER TABLE") {
        sql.to_owned()
    } else {
        mysql_sql(sql).0
    };
    sql = sql.replace("DEFAULT (datetime('now'))", "DEFAULT (CURRENT_TIMESTAMP())");
    if sql.starts_with("CREATE TABLE") && sql.contains("asset_revisions") {
        sql = regex::Regex::new(r",?\s+UNIQUE\s*\(\s*raw_sha256\s*\)")
            .unwrap()
            .replace(&sql, "")
            .into_owned();
    }
    if sql == "BEGIN" {
        return "START TRANSACTION".into();
    }
    if sql.starts_with("CREATE UNIQUE INDEX IF NOT EXISTS idx_scoped_identity_active") {
        return "CREATE UNIQUE INDEX idx_scoped_identity_active ON scoped_identity_bindings(scope_id,world_epoch,approved_entity_uuid)".into();
    }
    sql = sql
        .replace("INDEX IF NOT EXISTS", "INDEX")
        .replace("CREATE VIEW IF NOT EXISTS", "CREATE OR REPLACE VIEW");
    if sql.starts_with("CREATE TABLE") || sql.starts_with("ALTER TABLE") {
        // Identifier fields are bounded and indexable; payloads and paths remain unrestricted text.
        let field = regex::Regex::new(
            r"\b([A-Za-z_][A-Za-z_0-9]*)\s+TEXT\b(\s+NOT NULL)?(?:\s+DEFAULT\s+'([^']*)')?",
        )
        .unwrap();
        sql = field
            .replace_all(&sql, |c: &regex::Captures<'_>| {
                let name = &c[1];
                let typ = if name.ends_with("_json")
                    || ["object_path", "temp_path", "base_url", "reason", "data"].contains(&name)
                {
                    "LONGTEXT"
                } else if [
                    "password_hash",
                    "display_name",
                    "name",
                    "username",
                    "texture_id",
                    "session_path",
                    "authenticate_path",
                ]
                .contains(&name)
                {
                    "VARCHAR(1024)"
                } else if name == "dimension_id" {
                    "VARCHAR(256)"
                } else {
                    "VARCHAR(128)"
                };
                let default = c
                    .get(3)
                    .map_or(String::new(), |v| format!(" DEFAULT '{}'", v.as_str()));
                format!(
                    "{name} {typ}{}{}",
                    c.get(2).map_or("", |v| v.as_str()),
                    default
                )
            })
            .into_owned();
        sql = regex::Regex::new(r"\bINTEGER\b")
            .unwrap()
            .replace_all(&sql, "BIGINT")
            .into_owned()
            .replace("REAL", "DOUBLE");
        sql = sql.replace(
            "BIGINT PRIMARY KEY AUTOINCREMENT",
            "BIGINT AUTO_INCREMENT PRIMARY KEY",
        );
        // MySQL requires table-level foreign keys (inline REFERENCES is ignored in older releases).
        let refs=regex::Regex::new(r"\b([A-Za-z_][A-Za-z_0-9]*)\s+(VARCHAR\([0-9]+\)|BIGINT)([^,]*?)\s+REFERENCES\s+([A-Za-z_][A-Za-z_0-9]*)\(([^)]+)\)(\s+ON DELETE CASCADE)?").unwrap();
        let mut foreign = Vec::new();
        sql = refs
            .replace_all(&sql, |c: &regex::Captures<'_>| {
                foreign.push(format!(
                    "FOREIGN KEY ({}) REFERENCES {}({}){}",
                    &c[1],
                    &c[4],
                    &c[5],
                    c.get(6).map_or("", |v| v.as_str())
                ));
                format!("{} {}{}", &c[1], &c[2], &c[3])
            })
            .into_owned();
        if sql.starts_with("CREATE TABLE") {
            let end = sql.rfind(')').unwrap();
            let extra = if sql.contains("scoped_identity_bindings (") {
                ",approved_entity_uuid VARCHAR(128) GENERATED ALWAYS AS (CASE WHEN status='APPROVED' THEN entity_uuid ELSE NULL END) STORED"
            } else {
                ""
            };
            sql.insert_str(
                end,
                &format!(
                    "{}{}",
                    extra,
                    foreign.iter().map(|v| format!(",{v}")).collect::<String>()
                ),
            );
            sql.push_str(" ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin");
        }
    }
    sql
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordered_bindings_skip_literals_and_keep_unicode() {
        let (sql, params) = mysql_sql("SELECT ?2,?1,?2,'?3 芙宁娜' WHERE name=?1");
        assert_eq!(params, vec![1, 0, 1, 0]);
        assert_eq!(sql, "SELECT ?,?,?,'?3 芙宁娜' WHERE name=?");
    }
    #[test]
    fn translates_upserts_and_schema_without_losing_foreign_keys() {
        let sql=mysql_sql("INSERT INTO assets VALUES(?1) ON CONFLICT(asset_id) DO UPDATE SET visibility=excluded.visibility").0;
        assert!(sql.contains("ON DUPLICATE KEY UPDATE visibility=VALUES(visibility)"));
        let sql = mysql_schema(
            "CREATE TABLE IF NOT EXISTS example (account_id TEXT PRIMARY KEY REFERENCES accounts(account_id), payload_json TEXT NOT NULL)",
        );
        assert!(sql.contains("FOREIGN KEY (account_id) REFERENCES accounts(account_id)"));
        assert!(sql.contains("payload_json LONGTEXT"));
    }
    #[test]
    fn transactions_rollback_and_empty_rows_stay_optional() {
        use rusqlite::OptionalExtension;
        let dir = tempfile::tempdir().unwrap();
        let mut conn = Connection::open(&dir.path().join("db")).unwrap();
        conn.execute_batch("CREATE TABLE sample(id INTEGER PRIMARY KEY,value TEXT)")
            .unwrap();
        {
            let tx = conn.transaction().unwrap();
            tx.execute(
                "INSERT INTO sample VALUES(?1,?2)",
                rusqlite::params![1, "one"],
            )
            .unwrap();
        }
        assert_eq!(
            conn.query_row("SELECT value FROM sample", [], |r| r.get::<_, String>(0))
                .optional()
                .unwrap(),
            None
        );
        let tx = conn.transaction().unwrap();
        tx.execute(
            "INSERT INTO sample VALUES(?1,?2)",
            rusqlite::params![1, "one"],
        )
        .unwrap();
        tx.commit().unwrap();
        assert_eq!(
            conn.query_row("SELECT value FROM sample WHERE id=?1", [1], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "one"
        );
    }
}
