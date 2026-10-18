//! Postgres: over the wire with tokio-postgres's simple query protocol (values as text), or with `psql` inside a
//! container whose port isn't published (its socket trusts local logins, so no password is needed).
//! Read-only by default: the session's transactions are read-only unless a statement runs in write mode.

use super::creds::Cred;
use super::{ident, lit, page_sql, parse_csv, Browse, Col, Edit, Fail, Grid, Table, Target, NULL};
use crate::Res;
use futures_util::{pin_mut, TryStreamExt};
use std::time::Duration;
use tokio_postgres::{CancelToken, NoTls, SimpleQueryMessage};

enum Conn {
    Wire(tokio_postgres::Client),
    Exec { container: String, user: String },
}

pub struct Pg {
    conn: Conn,
    pub db: String,
}

const SESSION: &str = "SET default_transaction_read_only = on; SET statement_timeout = '15s'";

/// Logins Postgres refused, or a database it doesn't have (the next login may name another).
fn refused(code: &str) -> bool {
    ["28P01", "28000", "3D000"].contains(&code)
}

fn message(e: &tokio_postgres::Error) -> String {
    match e.as_db_error() {
        Some(d) => {
            let mut m = format!("ERROR: {}", d.message());
            if let Some(h) = d.hint() {
                m += &format!("\nHINT: {h}");
            }
            m
        }
        // "invalid configuration", "error connecting to server": the reason is the source.
        None => match std::error::Error::source(e) {
            Some(s) => format!("{e}: {s}"),
            None => e.to_string(),
        },
    }
}

impl Pg {
    pub async fn connect(t: &Target, c: &Cred) -> Result<(Pg, Option<CancelToken>), Fail> {
        let user = c.user.clone().unwrap_or_else(|| "postgres".into());
        let db = c.database.clone().unwrap_or_else(|| user.clone());
        let Some((host, port)) = t.wire() else {
            let container = t.container.clone().ok_or_else(|| Fail::Other("Nothing to connect to".into()))?;
            let pg = Pg { conn: Conn::Exec { container, user }, db };
            return match pg.run("SELECT 1", false, 1).await {
                Ok(_) => Ok((pg, None)),
                Err(e) if e.contains("does not exist") || e.contains("authentication failed") => Err(Fail::Auth(e)),
                Err(e) => Err(Fail::Other(e)),
            };
        };
        let mut cfg = tokio_postgres::Config::new();
        cfg.host(host.trim_matches(['[', ']'])).port(port).user(&user).dbname(&db).application_name("Pretorium").connect_timeout(Duration::from_secs(5));
        if let Some(p) = &c.password {
            cfg.password(p);
        }
        let (client, connection) = match cfg.connect(NoTls).await {
            Ok(c) => c,
            Err(e) => {
                let code = e.code().map(|c| c.code().to_string()).unwrap_or_default();
                // A server that wants a password we don't have says so before any code.
                let wants_password = message(&e).contains("password missing");
                return Err(if wants_password {
                    Fail::Auth("The server wants a password".into())
                } else if refused(&code) {
                    Fail::Auth(message(&e))
                } else {
                    Fail::Other(message(&e))
                });
            }
        };
        tauri::async_runtime::spawn(connection);
        client.batch_execute(SESSION).await.map_err(|e| Fail::Other(message(&e)))?;
        let cancel = client.cancel_token();
        Ok((Pg { conn: Conn::Wire(client), db }, Some(cancel)))
    }

    /// Run `sql`, keeping the last statement's result (at most `limit` rows of it).
    pub async fn run(&self, sql: &str, write: bool, limit: usize) -> Res<Grid> {
        match &self.conn {
            Conn::Wire(client) => {
                if write {
                    client.batch_execute("SET default_transaction_read_only = off").await.map_err(|e| message(&e))?;
                }
                let out = wire(client, sql, limit).await;
                if write {
                    // Back to read-only even when the statement failed.
                    client.batch_execute("SET default_transaction_read_only = on").await.map_err(|e| message(&e))?;
                }
                out
            }
            Conn::Exec { container, user } => {
                // A backslash command would run in psql itself (\! runs a shell), not in the database.
                if sql.trim_start().starts_with('\\') {
                    return Err("psql commands (\\…) aren't run here: write SQL".into());
                }
                let mut cmd = crate::infra::docker().ok_or("Docker isn't installed")?;
                let opts = format!("-c default_transaction_read_only={} -c statement_timeout=15s", if write { "off" } else { "on" });
                let null = format!("null={NULL}");
                cmd.args(["exec", "-e", &format!("PGOPTIONS={opts}"), container, "psql", "-X", "-v", "ON_ERROR_STOP=1", "-v", "SHOW_ALL_RESULTS=off", "--csv", "-P", &null]);
                cmd.args(["-U", user, "-d", &self.db, "-c", sql]);
                let out = tauri::async_runtime::spawn_blocking(move || crate::usage::output_within(cmd, Duration::from_secs(20)))
                    .await
                    .map_err(|e| e.to_string())?
                    .ok_or("The query took too long")?;
                if !out.status.success() {
                    let err = String::from_utf8_lossy(&out.stderr);
                    // Not where in our wrapper it was raised.
                    let err: Vec<&str> = err.lines().filter(|l| !l.starts_with("CONTEXT:")).collect();
                    return Err(err.join("\n").trim().replace("ERROR:  ", "ERROR: "));
                }
                let text = String::from_utf8_lossy(&out.stdout);
                // A statement without rows prints its command tag (UPDATE 3, INSERT 0 1, CREATE TABLE).
                let first = text.lines().next().unwrap_or("");
                if text.lines().count() <= 1 && !first.is_empty() && first.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == ' ') {
                    let n = first.rsplit(' ').next().and_then(|n| n.parse().ok()).unwrap_or(0);
                    return Ok(Grid { affected: Some(n), ..Default::default() });
                }
                parse_csv(&text, limit)
            }
        }
    }

    async fn text(&self, sql: &str) -> Res<Vec<Vec<Option<String>>>> {
        Ok(self.run(sql, false, 100_000).await?.rows)
    }

    pub async fn databases(&self) -> Res<Vec<String>> {
        let rows = self.text("SELECT datname FROM pg_database WHERE datallowconn AND NOT datistemplate ORDER BY 1").await?;
        Ok(rows.into_iter().filter_map(|mut r| r.remove(0)).collect())
    }

    pub async fn tree(&self) -> Res<Vec<Table>> {
        let rows = self
            .text(
                "SELECT n.nspname, c.relname, CASE WHEN c.relkind IN ('v', 'm') THEN 'view' ELSE 'table' END, \
                 CASE WHEN c.reltuples < 0 THEN NULL ELSE c.reltuples::bigint END \
                 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                 WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f') AND n.nspname NOT IN ('pg_catalog', 'information_schema') \
                 AND n.nspname NOT LIKE 'pg_toast%' AND n.nspname NOT LIKE 'pg_temp%' ORDER BY 1, 2",
            )
            .await?;
        Ok(rows.into_iter().map(table).collect())
    }

    pub async fn rows(&self, b: &Browse) -> Res<Grid> {
        let from = format!("{}.{}", ident(&b.schema), ident(&b.table));
        let types = self
            .text(&format!(
                "SELECT column_name, data_type FROM information_schema.columns WHERE table_schema = {} AND table_name = {} ORDER BY ordinal_position",
                lit(&b.schema),
                lit(&b.table)
            ))
            .await?;
        let key: Vec<String> = self
            .text(&format!(
                "SELECT a.attname FROM pg_index i JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey) \
                 WHERE i.indrelid = to_regclass({}) AND i.indisprimary ORDER BY array_position(i.indkey::int2[], a.attnum)",
                lit(&from)
            ))
            .await?
            .into_iter()
            .filter_map(|mut r| r.remove(0))
            .collect();
        let (page, count) = page_sql(b, &from, &key, ident);
        let mut g = self.run(&page, false, 1000).await?;
        g.total = self.text(&count).await?.first().and_then(|r| r[0].as_deref()?.parse().ok());
        typed(&mut g, types);
        g.key = key;
        Ok(g)
    }

    /// Run an UPDATE that must change exactly one row, else nothing.
    pub async fn apply(&self, sql: &str) -> Res<()> {
        // In one statement, so it works the same through psql: the block raises (and so rolls back) unless one row changed.
        let block = format!(
            "DO $pretorium$ DECLARE n bigint; BEGIN {sql}; GET DIAGNOSTICS n = ROW_COUNT; \
             IF n <> 1 THEN RAISE EXCEPTION 'The edit would change % rows, not 1: nothing was changed', n; END IF; END $pretorium$"
        );
        self.run(&block, true, 0).await.map(|_| ())
    }

    pub async fn columns(&self) -> Res<Vec<(String, String, String)>> {
        let rows = self
            .text(
                "SELECT table_schema, table_name, column_name FROM information_schema.columns \
                 WHERE table_schema NOT IN ('pg_catalog', 'information_schema') ORDER BY 1, 2, ordinal_position LIMIT 20000",
            )
            .await?;
        Ok(rows.into_iter().filter_map(|r| Some((r[0].clone()?, r[1].clone()?, r[2].clone()?))).collect())
    }
}

pub fn update(e: &Edit) -> String {
    super::update_sql(e, format!("{}.{}", ident(&e.schema), ident(&e.table)), ident, lit)
}

fn table(r: Vec<Option<String>>) -> Table {
    let s = |i: usize| r.get(i).cloned().flatten().unwrap_or_default();
    Table { schema: s(0), name: s(1), kind: s(2), rows: r.get(3).cloned().flatten().and_then(|n| n.parse().ok()) }
}

/// Columns get their types; an empty page still shows its columns.
pub(super) fn typed(g: &mut Grid, types: Vec<Vec<Option<String>>>) {
    let types: Vec<(String, String)> = types.into_iter().filter_map(|r| Some((r.first()?.clone()?, r.get(1)?.clone().unwrap_or_default()))).collect();
    if g.columns.is_empty() {
        g.columns = types.iter().map(|(n, _)| Col { name: n.clone(), kind: String::new() }).collect();
    }
    for c in &mut g.columns {
        if let Some((_, k)) = types.iter().find(|(n, _)| *n == c.name) {
            c.kind = k.clone();
        }
    }
}

async fn wire(client: &tokio_postgres::Client, sql: &str, limit: usize) -> Res<Grid> {
    let stream = client.simple_query_raw(sql).await.map_err(|e| message(&e))?;
    pin_mut!(stream);
    let mut last = Grid::default();
    let mut current: Option<Grid> = None;
    while let Some(m) = stream.try_next().await.map_err(|e| message(&e))? {
        match m {
            SimpleQueryMessage::RowDescription(cols) => {
                current = Some(Grid { columns: cols.iter().map(|c| Col { name: c.name().to_string(), kind: String::new() }).collect(), ..Default::default() });
            }
            SimpleQueryMessage::Row(row) => {
                let g = current.get_or_insert_with(Grid::default);
                if g.rows.len() < limit {
                    g.rows.push((0..row.len()).map(|i| row.get(i).map(String::from)).collect());
                } else {
                    g.truncated = true; // the rest is read and dropped
                }
            }
            SimpleQueryMessage::CommandComplete(n) => {
                last = current.take().unwrap_or(Grid { affected: Some(n), ..Default::default() });
            }
            _ => {}
        }
    }
    Ok(last)
}
