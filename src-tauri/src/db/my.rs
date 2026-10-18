//! MySQL and MariaDB over the wire (text protocol: values as text). Read-only by default: the session's transactions
//! are read-only unless a statement runs in write mode.

use super::creds::Cred;
use super::{page_sql, Browse, Col, Edit, Fail, Grid, Table, Target};
use crate::Res;
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, OptsBuilder, Row, Value};
use std::time::Duration;

pub struct My {
    conn: Conn,
    pub db: Option<String>,
}

pub fn ident(s: &str) -> String {
    format!("`{}`", s.replace('`', "``"))
}
/// A string literal; backslashes escape in MySQL's default mode.
pub fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "''"))
}

fn message(e: &mysql_async::Error) -> String {
    match e {
        mysql_async::Error::Server(s) => format!("ERROR {}: {}", s.code, s.message),
        e => e.to_string(),
    }
}

fn text(v: &Value) -> Option<String> {
    Some(match v {
        Value::NULL => return None,
        Value::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Double(d) => d.to_string(),
        Value::Date(y, mo, d, h, mi, s, us) => format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}{}", if *us > 0 { format!(".{us:06}") } else { String::new() }),
        Value::Time(neg, d, h, mi, s, _) => format!("{}{:02}:{mi:02}:{s:02}", if *neg { "-" } else { "" }, *d * 24 + *h as u32),
    })
}

impl My {
    pub async fn connect(t: &Target, c: &Cred) -> Result<My, Fail> {
        let (host, port) = t.wire().ok_or_else(|| Fail::Other("This MySQL container publishes no port: publish one to browse it".into()))?;
        let opts = OptsBuilder::default()
            .ip_or_hostname(host.trim_matches(['[', ']']))
            .tcp_port(port)
            .prefer_socket(false)
            .user(c.user.clone())
            .pass(c.password.clone())
            .db_name(c.database.clone())
            .client_found_rows(true); // an UPDATE that sets the same value still counts its row
        let conn = match tokio::time::timeout(Duration::from_secs(5), Conn::new(opts)).await {
            Err(_) => return Err(Fail::Other("The server didn't answer".into())),
            Ok(Err(e)) => {
                // Access denied (to the server or the database), an unknown database.
                let auth = matches!(&e, mysql_async::Error::Server(s) if [1044, 1045, 1049, 1698].contains(&s.code));
                return Err(if auth { Fail::Auth(message(&e)) } else { Fail::Other(message(&e)) });
            }
            Ok(Ok(conn)) => conn,
        };
        let mut my = My { conn, db: c.database.clone() };
        my.conn.query_drop("SET SESSION TRANSACTION READ ONLY").await.map_err(|e| Fail::Other(message(&e)))?;
        // MariaDB's statement limit, then MySQL's: each server knows one of them.
        let _ = my.conn.query_drop("SET SESSION max_statement_time = 15").await;
        let _ = my.conn.query_drop("SET SESSION max_execution_time = 15000").await;
        if my.db.is_none() {
            my.db = my.conn.query_first::<Option<String>, _>("SELECT DATABASE()").await.ok().flatten().flatten();
        }
        Ok(my)
    }

    pub async fn run(&mut self, sql: &str, write: bool, limit: usize) -> Res<Grid> {
        if write {
            self.conn.query_drop("SET SESSION TRANSACTION READ WRITE").await.map_err(|e| message(&e))?;
        }
        let out = self.sets(sql, limit).await;
        if write {
            self.conn.query_drop("SET SESSION TRANSACTION READ ONLY").await.map_err(|e| message(&e))?;
        }
        out
    }

    /// The last result of `sql` (it may hold several statements).
    async fn sets(&mut self, sql: &str, limit: usize) -> Res<Grid> {
        let mut result = self.conn.query_iter(sql).await.map_err(|e| message(&e))?;
        let mut last = Grid { affected: Some(result.affected_rows()), ..Default::default() };
        while !result.is_empty() {
            let columns: Vec<Col> = result
                .columns()
                .map(|cols| cols.iter().map(|c| Col { name: c.name_str().into_owned(), kind: format!("{:?}", c.column_type()).trim_start_matches("MYSQL_TYPE_").to_lowercase() }).collect())
                .unwrap_or_default();
            let rows: Vec<Row> = result.collect().await.map_err(|e| message(&e))?;
            last = if columns.is_empty() {
                Grid { affected: Some(result.affected_rows()), ..Default::default() }
            } else {
                let truncated = rows.len() > limit;
                let rows = rows.iter().take(limit).map(|r| (0..r.len()).map(|i| r.as_ref(i).and_then(text)).collect()).collect();
                Grid { columns, rows, truncated, ..Default::default() }
            };
        }
        Ok(last)
    }

    async fn text(&mut self, sql: &str) -> Res<Vec<Vec<Option<String>>>> {
        Ok(self.sets(sql, 100_000).await?.rows)
    }

    pub async fn databases(&mut self) -> Res<Vec<String>> {
        let rows = self.text("SELECT schema_name FROM information_schema.schemata WHERE schema_name NOT IN ('mysql', 'information_schema', 'performance_schema', 'sys') ORDER BY 1").await?;
        Ok(rows.into_iter().filter_map(|mut r| r.remove(0)).collect())
    }

    pub async fn tree(&mut self) -> Res<Vec<Table>> {
        let rows = self
            .text(
                "SELECT table_schema, table_name, IF(table_type = 'VIEW', 'view', 'table'), table_rows FROM information_schema.tables \
                 WHERE table_schema NOT IN ('mysql', 'information_schema', 'performance_schema', 'sys') ORDER BY 1, 2",
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let s = |i: usize| r.get(i).cloned().flatten().unwrap_or_default();
                Table { schema: s(0), name: s(1), kind: s(2), rows: r.get(3).cloned().flatten().and_then(|n| n.parse().ok()) }
            })
            .collect())
    }

    pub async fn rows(&mut self, b: &Browse) -> Res<Grid> {
        let wh = format!("table_schema = {} AND table_name = {}", lit(&b.schema), lit(&b.table));
        let types = self.text(&format!("SELECT column_name, column_type FROM information_schema.columns WHERE {wh} ORDER BY ordinal_position")).await?;
        let key: Vec<String> = self
            .text(&format!("SELECT column_name FROM information_schema.key_column_usage WHERE {wh} AND constraint_name = 'PRIMARY' ORDER BY ordinal_position"))
            .await?
            .into_iter()
            .filter_map(|mut r| r.remove(0))
            .collect();
        let from = format!("{}.{}", ident(&b.schema), ident(&b.table));
        let (page, count) = page_sql(b, &from, &key, ident);
        let mut g = self.sets(&page, 1000).await?;
        g.total = self.text(&count).await?.first().and_then(|r| r[0].as_deref()?.parse().ok());
        super::pg::typed(&mut g, types);
        g.key = key;
        Ok(g)
    }

    /// Run an UPDATE that must change exactly one row, else nothing.
    pub async fn apply(&mut self, sql: &str) -> Res<()> {
        self.conn.query_drop("START TRANSACTION READ WRITE").await.map_err(|e| message(&e))?;
        let changed = match self.conn.query_iter(sql).await {
            Ok(r) => {
                let n = r.affected_rows();
                r.drop_result().await.map_err(|e| message(&e)).map(|_| n)
            }
            Err(e) => Err(message(&e)),
        };
        let ok = matches!(changed, Ok(1));
        self.conn.query_drop(if ok { "COMMIT" } else { "ROLLBACK" }).await.map_err(|e| message(&e))?;
        match changed {
            Ok(1) => Ok(()),
            Ok(n) => Err(format!("The edit would change {n} rows, not 1: nothing was changed")),
            Err(e) => Err(e),
        }
    }

    pub async fn columns(&mut self) -> Res<Vec<(String, String, String)>> {
        let rows = self
            .text(
                "SELECT table_schema, table_name, column_name FROM information_schema.columns \
                 WHERE table_schema NOT IN ('mysql', 'information_schema', 'performance_schema', 'sys') ORDER BY 1, 2, ordinal_position LIMIT 20000",
            )
            .await?;
        Ok(rows.into_iter().filter_map(|r| Some((r[0].clone()?, r[1].clone()?, r[2].clone()?))).collect())
    }
}

pub fn update(e: &Edit) -> String {
    super::update_sql(e, format!("{}.{}", ident(&e.schema), ident(&e.table)), ident, lit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_for_mysql() {
        assert_eq!(ident("a`b"), "`a``b`");
        assert_eq!(lit(r"it's a \ path"), r"'it''s a \\ path'");
        assert_eq!(text(&Value::Date(2026, 1, 2, 3, 4, 5, 0)).as_deref(), Some("2026-01-02 03:04:05"));
        assert_eq!(text(&Value::NULL), None);
    }
}
