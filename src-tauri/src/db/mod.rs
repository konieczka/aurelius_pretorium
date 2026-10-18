//! Database browser: the databases the Servers panel finds on this machine (never a remote one). Lists their tables,
//! pages through rows, runs queries and edits cells, read-only unless the tab allows writes.
//! Postgres and MySQL/MariaDB over the wire (text protocol: every value comes back as text, so nothing needs decoding
//! by type), or `docker exec … psql` into a container that publishes no port; Redis over the wire; Mongo through
//! `mongosh`; SQLite files directly.

mod creds;
mod kv;
mod lite;
mod mongo;
mod my;
mod pg;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

use crate::Res;

#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Postgres,
    Mysql,
    Redis,
    Mongo,
    Sqlite,
}

impl Engine {
    fn port(self) -> u16 {
        match self {
            Engine::Postgres => 5432,
            Engine::Mysql => 3306,
            Engine::Redis => 6379,
            Engine::Mongo => 27017,
            Engine::Sqlite => 0,
        }
    }
}

/// What to connect to: a server on this machine, a container (exec'd into when it publishes no port), or a SQLite file.
#[derive(Deserialize, Clone, Debug)]
pub struct Target {
    pub engine: Engine,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub container: Option<String>,
    pub file: Option<String>,
    pub root: Option<String>, // the project's folder: its .env files may name the credentials
    pub user: Option<String>, // typed into the login form
    pub password: Option<String>,
    pub database: Option<String>, // picked in the tab (or typed into the login form)
    #[serde(default)]
    pub remember: bool, // keep the login in the Keychain
}

impl Target {
    /// Over the wire, else by exec into the container.
    fn wire(&self) -> Option<(&str, u16)> {
        Some((self.host.as_deref().unwrap_or("127.0.0.1"), self.port?))
    }
}

/// Hosts that are this machine: the browser never connects anywhere else.
const LOCAL: [&str; 5] = ["localhost", "127.0.0.1", "::1", "[::1]", "0.0.0.0"];

#[derive(Serialize, Clone, Debug)]
pub struct Connected {
    id: String,
    engine: Engine,
    user: String,
    database: String,
    databases: Vec<String>,
    via: String, // where the login came from: container settings, .env, Keychain, default login, login
    route: String, // localhost:5433, container pretorium-e2e-pg, a file
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Col {
    pub name: String,
    pub kind: String, // the column's type when known (browsing a table), else ""
}

/// One result: rows as text (None = NULL), or what a statement changed.
#[derive(Serialize, Clone, Debug, Default)]
pub struct Grid {
    pub columns: Vec<Col>,
    pub rows: Vec<Vec<Option<String>>>,
    pub key: Vec<String>, // the table's primary key: what an edit is keyed on
    pub total: Option<i64>,
    pub affected: Option<u64>,
    pub truncated: bool, // more rows than were kept
    pub ms: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Table {
    pub schema: String,
    pub name: String,
    pub kind: String, // table, view, collection
    pub rows: Option<i64>, // estimated
}

#[derive(Deserialize, Clone, Debug)]
pub struct Browse {
    pub schema: String,
    pub table: String,
    pub offset: u64,
    pub limit: u64,
    pub sort: Option<String>,
    #[serde(default)]
    pub desc: bool,
    pub filter: Option<String>, // SQL after WHERE; Mongo: a JSON filter
}

/// Set one cell of the row with this primary key.
#[derive(Deserialize, Clone, Debug)]
pub struct Edit {
    pub schema: String,
    pub table: String,
    pub key: Vec<(String, Option<String>)>,
    pub column: String,
    pub value: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Edited {
    sql: String,
    affected: Option<u64>, // None: not applied (a preview)
}

#[derive(Serialize, Clone, Debug)]
pub struct Completion {
    schema: String,
    table: String,
    columns: Vec<String>,
}

/// Rows a query keeps: a result past it is cut off (and says so).
const KEEP: usize = 1000;

/// A connection failed: the login was refused (try the next one, else ask), or anything else (stop).
pub enum Fail {
    Auth(String),
    Other(String),
}

enum Client {
    Pg(pg::Pg),
    My(my::My),
    Kv(kv::Kv),
    Mongo(mongo::Mongo),
    Lite(lite::Lite),
}

struct Session {
    client: Mutex<Client>, // one statement at a time: write mode is switched on and off around it
    cancel: Option<tokio_postgres::CancelToken>, // outside the lock: cancelling mustn't wait for the query
}

/// Open connections by id (a tab's).
#[derive(Default)]
pub struct Dbs {
    sessions: std::sync::Mutex<HashMap<String, Arc<Session>>>,
    next: AtomicU64,
}

fn unsupported() -> String {
    "Not available for this database".into()
}

impl Dbs {
    fn session(&self, id: &str) -> Res<Arc<Session>> {
        self.sessions.lock().unwrap().get(id).cloned().ok_or_else(|| "Not connected: reopen the tab".to_string())
    }

    pub async fn connect(&self, t: Target) -> Res<Connected> {
        if let Some(h) = t.host.as_deref().filter(|_| t.port.is_some()) {
            if !LOCAL.contains(&h) {
                return Err("Only databases on this machine can be browsed".into());
            }
        }
        let route = match (&t.file, t.wire(), &t.container) {
            (Some(f), _, _) => f.clone(),
            (_, Some((h, p)), _) => format!("{h}:{p}"),
            (_, _, Some(c)) => format!("container {c}"),
            _ => return Err("Nothing to connect to".into()),
        };
        let mut refused = None;
        for c in creds::candidates(&t).await {
            let opened = match t.engine {
                Engine::Postgres => pg::Pg::connect(&t, &c).await.map(|(p, cancel)| (Client::Pg(p), cancel)),
                Engine::Mysql => my::My::connect(&t, &c).await.map(|m| (Client::My(m), None)),
                Engine::Redis => kv::Kv::connect(&t, &c).await.map(|k| (Client::Kv(k), None)),
                Engine::Mongo => mongo::Mongo::connect(&t, &c).await.map(|m| (Client::Mongo(m), None)),
                Engine::Sqlite => lite::Lite::open(&t).map(|l| (Client::Lite(l), None)),
            };
            let (mut client, cancel) = match opened {
                Ok(o) => o,
                Err(Fail::Auth(e)) => {
                    refused = Some(e);
                    continue;
                }
                Err(Fail::Other(e)) => return Err(e),
            };
            let databases = match &mut client {
                Client::Pg(p) => p.databases().await.unwrap_or_default(),
                Client::My(m) => m.databases().await.unwrap_or_default(),
                Client::Kv(k) => k.databases().await.unwrap_or_default(),
                Client::Mongo(m) => m.databases.clone(),
                Client::Lite(_) => vec!["main".into()],
            };
            let database = match &client {
                Client::Pg(p) => p.db.clone(),
                Client::My(m) => m.db.clone().unwrap_or_default(),
                Client::Kv(_) => "0".into(),
                Client::Mongo(m) => m.db.clone(),
                Client::Lite(_) => "main".into(),
            };
            if t.remember && c.via == "login" {
                creds::remember(&t, &c).await;
            }
            let id = format!("db{}", self.next.fetch_add(1, Ordering::Relaxed) + 1);
            self.sessions.lock().unwrap().insert(id.clone(), Arc::new(Session { client: Mutex::new(client), cancel }));
            return Ok(Connected { id, engine: t.engine, user: c.user.unwrap_or_default(), database, databases, via: c.via, route });
        }
        // The prefix tells the tab to ask for a login.
        Err(format!("auth: {}", refused.unwrap_or_else(|| "No login found for this database".into())))
    }

    pub fn disconnect(&self, id: &str) {
        self.sessions.lock().unwrap().remove(id);
    }

    pub async fn tree(&self, id: &str, database: Option<String>) -> Res<Vec<Table>> {
        let s = self.session(id)?;
        let mut c = s.client.lock().await;
        match &mut *c {
            Client::Pg(p) => p.tree().await,
            Client::My(m) => m.tree().await,
            Client::Mongo(m) => m.tree(database.as_deref().unwrap_or(&m.db.clone())).await,
            Client::Lite(l) => l.tree().await,
            Client::Kv(_) => Err(unsupported()),
        }
    }

    pub async fn rows(&self, id: &str, b: Browse) -> Res<Grid> {
        let s = self.session(id)?;
        let mut c = s.client.lock().await;
        let started = std::time::Instant::now();
        let mut g = match &mut *c {
            Client::Pg(p) => p.rows(&b).await,
            Client::My(m) => m.rows(&b).await,
            Client::Mongo(m) => m.rows(&b).await,
            Client::Lite(l) => l.rows(&b).await,
            Client::Kv(_) => Err(unsupported()),
        }?;
        g.ms = started.elapsed().as_millis() as u64;
        Ok(g)
    }

    pub async fn query(&self, id: &str, sql: String, write: bool) -> Res<Grid> {
        let s = self.session(id)?;
        let mut c = s.client.lock().await;
        let started = std::time::Instant::now();
        let mut g = match &mut *c {
            Client::Pg(p) => p.run(&sql, write, KEEP).await,
            Client::My(m) => m.run(&sql, write, KEEP).await,
            Client::Lite(l) => l.run(&sql, write, KEEP).await,
            Client::Kv(_) | Client::Mongo(_) => Err(unsupported()),
        }?;
        g.ms = started.elapsed().as_millis() as u64;
        Ok(g)
    }

    pub async fn cancel(&self, id: &str) -> Res<()> {
        let s = self.session(id)?;
        match &s.cancel {
            Some(t) => t.cancel_query(tokio_postgres::NoTls).await.map_err(|e| e.to_string()),
            None => Err(unsupported()),
        }
    }

    /// The UPDATE that sets the cell; `apply` runs it (in a transaction that only commits if it changed one row).
    pub async fn edit(&self, id: &str, e: Edit, apply: bool) -> Res<Edited> {
        if e.key.is_empty() {
            return Err("This table has no primary key: its rows can't be told apart to edit one".into());
        }
        let s = self.session(id)?;
        let mut c = s.client.lock().await;
        let sql = match &*c {
            Client::Pg(_) => pg::update(&e),
            Client::My(_) => my::update(&e),
            Client::Lite(_) => lite::update(&e),
            _ => return Err(unsupported()),
        };
        if !apply {
            return Ok(Edited { sql, affected: None });
        }
        match &mut *c {
            Client::Pg(p) => p.apply(&sql).await?,
            Client::My(m) => m.apply(&sql).await?,
            Client::Lite(l) => l.apply(&sql).await?,
            _ => unreachable!(),
        }
        Ok(Edited { sql, affected: Some(1) })
    }

    /// Every table's columns, for the query editor's completion.
    pub async fn completion(&self, id: &str) -> Res<Vec<Completion>> {
        let s = self.session(id)?;
        let mut c = s.client.lock().await;
        let pairs = match &mut *c {
            Client::Pg(p) => p.columns().await?,
            Client::My(m) => m.columns().await?,
            Client::Lite(l) => l.columns().await?,
            _ => Vec::new(),
        };
        let mut out: Vec<Completion> = Vec::new();
        for (schema, table, column) in pairs {
            match out.last_mut() {
                Some(c) if c.schema == schema && c.table == table => c.columns.push(column),
                _ => out.push(Completion { schema, table, columns: vec![column] }),
            }
        }
        Ok(out)
    }

    pub async fn keys(&self, id: &str, db: i64, pattern: String, cursor: u64) -> Res<kv::Keys> {
        let s = self.session(id)?;
        let mut c = s.client.lock().await;
        let Client::Kv(k) = &mut *c else { return Err(unsupported()) };
        k.keys(db, &pattern, cursor).await
    }

    pub async fn value(&self, id: &str, db: i64, key: String) -> Res<kv::Value> {
        let s = self.session(id)?;
        let mut c = s.client.lock().await;
        let Client::Kv(k) = &mut *c else { return Err(unsupported()) };
        k.value(db, &key).await
    }

    pub async fn command(&self, id: &str, db: i64, line: String, write: bool) -> Res<String> {
        let s = self.session(id)?;
        let mut c = s.client.lock().await;
        let Client::Kv(k) = &mut *c else { return Err(unsupported()) };
        k.command(db, &line, write).await
    }
}

// SQL text: identifiers and literals, quoted for each dialect.
pub(crate) fn ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}
pub(crate) fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}
fn value_lit(v: &Option<String>, lit: fn(&str) -> String) -> String {
    v.as_deref().map(lit).unwrap_or_else(|| "NULL".into())
}
/// `UPDATE t SET c = v WHERE k = v AND …`, in a dialect's quoting.
fn update_sql(e: &Edit, table: String, ident: fn(&str) -> String, lit: fn(&str) -> String) -> String {
    let wh: Vec<String> = e
        .key
        .iter()
        .map(|(k, v)| match v {
            Some(v) => format!("{} = {}", ident(k), lit(v)),
            None => format!("{} IS NULL", ident(k)),
        })
        .collect();
    format!("UPDATE {table} SET {} = {} WHERE {}", ident(&e.column), value_lit(&e.value, lit), wh.join(" AND "))
}

/// Sort and filter clauses of a table page, shared by the SQL dialects.
fn page_sql(b: &Browse, from: &str, key: &[String], ident: fn(&str) -> String) -> (String, String) {
    let wh = b.filter.as_deref().map(str::trim).filter(|f| !f.is_empty()).map(|f| format!(" WHERE ({f})")).unwrap_or_default();
    // Paging needs a stable order: the key when nothing else is asked for.
    let order = match &b.sort {
        Some(c) => format!(" ORDER BY {} {}", ident(c), if b.desc { "DESC" } else { "ASC" }),
        None if !key.is_empty() => format!(" ORDER BY {}", key.iter().map(|k| ident(k)).collect::<Vec<_>>().join(", ")),
        None => String::new(),
    };
    (format!("SELECT * FROM {from}{wh}{order} LIMIT {} OFFSET {}", b.limit.min(1000), b.offset), format!("SELECT count(*) FROM {from}{wh}"))
}

/// Text values a CLI printed as CSV (psql), with NULL printed as this.
const NULL: &str = "\u{1}NULL\u{1}";

fn parse_csv(out: &str, limit: usize) -> Res<Grid> {
    let mut r = csv::ReaderBuilder::new().has_headers(true).flexible(true).from_reader(out.as_bytes());
    let columns = r.headers().map_err(|e| e.to_string())?.iter().map(|h| Col { name: h.to_string(), kind: String::new() }).collect();
    let mut g = Grid { columns, ..Default::default() };
    for rec in r.records() {
        let rec = rec.map_err(|e| e.to_string())?;
        if g.rows.len() == limit {
            g.truncated = true;
            break;
        }
        g.rows.push(rec.iter().map(|v| (v != NULL).then(|| v.to_string())).collect());
    }
    Ok(g)
}

#[tauri::command]
pub async fn db_connect(dbs: State<'_, Dbs>, target: Target) -> Res<Connected> {
    dbs.connect(target).await
}
#[tauri::command]
pub fn db_disconnect(dbs: State<'_, Dbs>, id: String) {
    dbs.disconnect(&id)
}
#[tauri::command]
pub async fn db_tree(dbs: State<'_, Dbs>, id: String, database: Option<String>) -> Res<Vec<Table>> {
    dbs.tree(&id, database).await
}
#[tauri::command]
pub async fn db_rows(dbs: State<'_, Dbs>, id: String, browse: Browse) -> Res<Grid> {
    dbs.rows(&id, browse).await
}
#[tauri::command]
pub async fn db_query(dbs: State<'_, Dbs>, id: String, sql: String, write: bool) -> Res<Grid> {
    dbs.query(&id, sql, write).await
}
#[tauri::command]
pub async fn db_cancel(dbs: State<'_, Dbs>, id: String) -> Res<()> {
    dbs.cancel(&id).await
}
#[tauri::command]
pub async fn db_edit(dbs: State<'_, Dbs>, id: String, edit: Edit, apply: bool) -> Res<Edited> {
    dbs.edit(&id, edit, apply).await
}
#[tauri::command]
pub async fn db_completion(dbs: State<'_, Dbs>, id: String) -> Res<Vec<Completion>> {
    dbs.completion(&id).await
}
#[tauri::command]
pub async fn db_keys(dbs: State<'_, Dbs>, id: String, db: i64, pattern: String, cursor: u64) -> Res<kv::Keys> {
    dbs.keys(&id, db, pattern, cursor).await
}
#[tauri::command]
pub async fn db_value(dbs: State<'_, Dbs>, id: String, db: i64, key: String) -> Res<kv::Value> {
    dbs.value(&id, db, key).await
}
#[tauri::command]
pub async fn db_command(dbs: State<'_, Dbs>, id: String, db: i64, line: String, write: bool) -> Res<String> {
    dbs.command(&id, db, line, write).await
}
/// SQLite databases in a project folder.
#[tauri::command]
pub async fn db_files(dir: String) -> Res<Vec<String>> {
    crate::blocking(move || Ok(lite::files(std::path::Path::new(&dir)))).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_identifiers_and_values() {
        let e = Edit { schema: "public".into(), table: "t".into(), key: vec![("id".into(), Some("7".into()))], column: "na\"me".into(), value: Some("O'Brien".into()) };
        assert_eq!(update_sql(&e, "\"t\"".into(), ident, lit), r#"UPDATE "t" SET "na""me" = 'O''Brien' WHERE "id" = '7'"#);
        let e = Edit { value: None, ..e };
        assert!(update_sql(&e, "\"t\"".into(), ident, lit).contains("= NULL WHERE"));
    }

    #[test]
    fn pages_in_key_order_unless_sorted() {
        let mut b = Browse { schema: "s".into(), table: "t".into(), offset: 100, limit: 50, sort: None, desc: false, filter: Some(" vip ".into()) };
        let (page, count) = page_sql(&b, "\"s\".\"t\"", &["id".into()], ident);
        assert_eq!(page, r#"SELECT * FROM "s"."t" WHERE (vip) ORDER BY "id" LIMIT 50 OFFSET 100"#);
        assert_eq!(count, r#"SELECT count(*) FROM "s"."t" WHERE (vip)"#);
        b.sort = Some("name".into());
        b.desc = true;
        b.filter = None;
        assert_eq!(page_sql(&b, "t", &[], ident).0, r#"SELECT * FROM t ORDER BY "name" DESC LIMIT 50 OFFSET 100"#);
    }

    #[test]
    fn reads_csv_with_nulls() {
        let out = format!("id,name\n1,\"a,b\"\n2,{NULL}\n3,\"\"\n");
        let g = parse_csv(&out, 2).unwrap();
        assert_eq!(g.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["id", "name"]);
        assert_eq!(g.rows, [vec![Some("1".into()), Some("a,b".into())], vec![Some("2".into()), None]]);
        assert!(g.truncated);
    }
}
