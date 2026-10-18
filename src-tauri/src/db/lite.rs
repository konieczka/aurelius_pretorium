//! SQLite files in a project. Each call opens the file (read-only unless writing): cheap, and nothing stays locked.

use super::{ident, lit, page_sql, Browse, Col, Edit, Fail, Grid, Table, Target};
use crate::Res;
use rusqlite::fallible_iterator::FallibleIterator;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Lite {
    file: PathBuf,
}

/// Extensions SQLite files go by.
const EXTENSIONS: [&str; 5] = ["sqlite", "sqlite3", "db", "db3", "s3db"];
/// Folders not worth looking in.
const SKIP: [&str; 8] = ["node_modules", "target", ".git", "dist", "build", ".venv", "venv", "vendor"];

fn text(v: ValueRef) -> Option<String> {
    Some(match v {
        ValueRef::Null => return None,
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
        ValueRef::Blob(b) => format!("<{} bytes>", b.len()),
    })
}

impl Lite {
    pub fn open(t: &Target) -> Result<Lite, Fail> {
        let file = PathBuf::from(t.file.as_deref().ok_or_else(|| Fail::Other("No file".into()))?);
        // Opening read-only also checks it's a database at all.
        let l = Lite { file };
        let c = l.conn(false).map_err(Fail::Other)?;
        c.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(())).map_err(|e| Fail::Other(format!("Not a SQLite database: {e}")))?;
        Ok(l)
    }

    fn conn(&self, write: bool) -> Res<Connection> {
        let flags = if write { OpenFlags::SQLITE_OPEN_READ_WRITE } else { OpenFlags::SQLITE_OPEN_READ_ONLY } | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let c = Connection::open_with_flags(&self.file, flags).map_err(|e| e.to_string())?;
        c.busy_timeout(Duration::from_secs(5)).map_err(|e| e.to_string())?;
        Ok(c)
    }

    async fn blocking<T: Send + 'static>(&self, write: bool, f: impl FnOnce(&Connection) -> Res<T> + Send + 'static) -> Res<T> {
        let l = Lite { file: self.file.clone() };
        crate::blocking(move || f(&l.conn(write)?)).await
    }

    pub async fn run(&self, sql: &str, write: bool, limit: usize) -> Res<Grid> {
        let sql = sql.to_string();
        self.blocking(write, move |c| run(c, &sql, limit)).await
    }

    pub async fn tree(&self) -> Res<Vec<Table>> {
        self.blocking(false, |c| {
            let mut s = c.prepare("SELECT name, type FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY name").map_err(|e| e.to_string())?;
            let names: Vec<(String, String)> = s.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|e| e.to_string())?.filter_map(Result::ok).collect();
            Ok(names
                .into_iter()
                .map(|(name, kind)| {
                    let rows = (kind == "table").then(|| c.query_row(&format!("SELECT count(*) FROM {}", ident(&name)), [], |r| r.get(0)).ok()).flatten();
                    Table { schema: "main".into(), name, kind, rows }
                })
                .collect())
        })
        .await
    }

    pub async fn rows(&self, b: &Browse) -> Res<Grid> {
        let b = b.clone();
        self.blocking(false, move |c| {
            let info = table_info(c, &b.table)?;
            let mut key: Vec<(i64, String)> = info.iter().filter(|i| i.2 > 0).map(|i| (i.2, i.0.clone())).collect();
            key.sort();
            let key: Vec<String> = key.into_iter().map(|k| k.1).collect();
            let (page, count) = page_sql(&b, &ident(&b.table), &key, ident);
            let mut g = run(c, &page, 1000)?;
            g.total = c.query_row(&count, [], |r| r.get(0)).ok();
            super::pg::typed(&mut g, info.into_iter().map(|i| vec![Some(i.0), Some(i.1)]).collect());
            g.key = key;
            Ok(g)
        })
        .await
    }

    /// Run an UPDATE that must change exactly one row, else nothing.
    pub async fn apply(&self, sql: &str) -> Res<()> {
        let sql = sql.to_string();
        self.blocking(true, move |c| {
            let tx = c.unchecked_transaction().map_err(|e| e.to_string())?;
            let n = tx.execute(&sql, []).map_err(|e| e.to_string())?;
            if n != 1 {
                return Err(format!("The edit would change {n} rows, not 1: nothing was changed")); // dropping tx rolls back
            }
            tx.commit().map_err(|e| e.to_string())
        })
        .await
    }

    pub async fn columns(&self) -> Res<Vec<(String, String, String)>> {
        let tables = self.tree().await?;
        self.blocking(false, move |c| {
            let mut out = Vec::new();
            for t in tables {
                for (col, _, _) in table_info(c, &t.name)? {
                    out.push(("main".into(), t.name.clone(), col));
                }
            }
            Ok(out)
        })
        .await
    }
}

/// Name, type and position in the primary key (0: not in it) of each column.
fn table_info(c: &Connection, table: &str) -> Res<Vec<(String, String, i64)>> {
    let mut s = c.prepare(&format!("PRAGMA table_info({})", ident(table))).map_err(|e| e.to_string())?;
    let rows = s.query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(5)?))).map_err(|e| e.to_string())?;
    Ok(rows.filter_map(Result::ok).collect())
}

/// Each statement of `sql` in turn; the last one's result.
fn run(c: &Connection, sql: &str, limit: usize) -> Res<Grid> {
    let mut batch = rusqlite::Batch::new(c, sql);
    let mut last = Grid::default();
    while let Some(mut stmt) = batch.next().map_err(|e| e.to_string())? {
        if stmt.column_count() == 0 {
            let n = stmt.raw_execute().map_err(|e| e.to_string())?;
            last = Grid { affected: Some(n as u64), ..Default::default() };
            continue;
        }
        let columns: Vec<Col> = stmt.columns().iter().map(|c| Col { name: c.name().to_string(), kind: c.decl_type().unwrap_or("").to_lowercase() }).collect();
        let n = columns.len();
        let mut g = Grid { columns, ..Default::default() };
        let mut rows = stmt.raw_query();
        while let Some(r) = rows.next().map_err(|e| e.to_string())? {
            if g.rows.len() == limit {
                g.truncated = true;
                break;
            }
            g.rows.push((0..n).map(|i| r.get_ref(i).ok().and_then(text)).collect());
        }
        last = g;
    }
    Ok(last)
}

pub fn update(e: &Edit) -> String {
    super::update_sql(e, ident(&e.table), ident, lit)
}

/// SQLite files in a project, a few levels down, skipping dependencies and build output.
pub fn files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut dirs = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if p.is_dir() {
                if depth < 3 && !SKIP.contains(&name.as_str()) && !name.starts_with('.') {
                    dirs.push((p, depth + 1));
                }
            } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| EXTENSIONS.contains(&x)) && is_sqlite(&p) {
                out.push(p.to_string_lossy().into_owned());
            }
        }
        if out.len() >= 50 {
            break;
        }
    }
    out.sort();
    out
}

/// A SQLite file starts with this header (a .db may be anything).
fn is_sqlite(p: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 16];
    std::fs::File::open(p).and_then(|mut f| f.read_exact(&mut head)).is_ok() && &head == b"SQLite format 3\0"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(file: &Path) -> Target {
        Target {
            engine: super::super::Engine::Sqlite, host: None, port: None, container: None, file: Some(file.to_string_lossy().into()),
            root: None, user: None, password: None, database: None, remember: false,
        }
    }

    #[test]
    fn browses_queries_and_edits_a_sqlite_file() {
        let dir = std::env::temp_dir().join(format!("pretorium-lite-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data/node_modules")).unwrap();
        let file = dir.join("data/app.db");
        let c = Connection::open(&file).unwrap();
        c.execute_batch("CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT, done INT); INSERT INTO notes (body, done) VALUES ('a', 0), ('b', NULL), ('c', 1);").unwrap();
        drop(c);
        std::fs::write(dir.join("data/fake.db"), "not sqlite").unwrap();
        std::fs::copy(&file, dir.join("data/node_modules/dep.sqlite")).unwrap();
        assert_eq!(files(&dir), [file.to_string_lossy()]);

        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let Ok(l) = Lite::open(&target(&file)) else { panic!("didn't open") };
            assert_eq!(l.tree().await.unwrap()[0], Table { schema: "main".into(), name: "notes".into(), kind: "table".into(), rows: Some(3) });
            let b = Browse { schema: "main".into(), table: "notes".into(), offset: 1, limit: 1, sort: Some("body".into()), desc: true, filter: None };
            let g = l.rows(&b).await.unwrap();
            assert_eq!((g.rows.clone(), g.total, g.key.clone()), (vec![vec![Some("2".into()), Some("b".into()), None]], Some(3), vec!["id".to_string()]));
            assert_eq!(g.columns[0], Col { name: "id".into(), kind: "INTEGER".into() });
            // Read-only unless writing.
            assert!(l.run("UPDATE notes SET done = 1", false, 10).await.unwrap_err().contains("readonly"));
            assert_eq!(l.run("UPDATE notes SET done = 1; SELECT count(*) AS n FROM notes WHERE done = 1", true, 10).await.unwrap().rows, [[Some("3".to_string())]]);
            let e = Edit { schema: "main".into(), table: "notes".into(), key: vec![("id".into(), Some("1".into()))], column: "body".into(), value: Some("it's".into()) };
            l.apply(&update(&e)).await.unwrap();
            let all = Edit { key: vec![("done".into(), Some("1".into()))], ..e };
            assert!(l.apply(&update(&all)).await.unwrap_err().contains("3 rows"));
            assert_eq!(l.run("SELECT body FROM notes ORDER BY id", false, 10).await.unwrap().rows[0], [Some("it's".to_string())]);
        });
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
