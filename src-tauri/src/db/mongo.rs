//! Mongo, through `mongosh`: the host's when it's installed, else the one inside the container. Lists databases and
//! collections and pages through documents with a JSON filter. It only ever reads (find, count).

use super::creds::Cred;
use super::{Browse, Col, Fail, Grid, Table, Target};
use crate::Res;
use std::process::{Command, Stdio};
use std::time::Duration;

pub struct Mongo {
    exec: Option<String>, // the container mongosh runs in; None: the host's mongosh
    uri: String,
    pub db: String,
    pub databases: Vec<String>,
}

/// A value as a JavaScript literal (JSON is one).
fn js(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

fn host_mongosh() -> Option<std::path::PathBuf> {
    std::env::split_paths(crate::login_path()).map(|d| d.join("mongosh")).find(|p| p.is_file())
}

impl Mongo {
    pub async fn connect(t: &Target, c: &Cred) -> Result<Mongo, Fail> {
        // The host's mongosh reaches a published port; else run the container's own against its own port.
        let (exec, addr) = match (t.wire(), host_mongosh(), &t.container) {
            (Some((h, p)), Some(_), _) => (None, format!("{}:{p}", if h.contains(':') && !h.starts_with('[') { format!("[{h}]") } else { h.to_string() })),
            (_, _, Some(c)) => (Some(c.clone()), "127.0.0.1:27017".into()),
            (Some(_), None, None) => return Err(Fail::Other("Install mongosh to browse this database (brew install mongosh)".into())),
            _ => return Err(Fail::Other("Nothing to connect to".into())),
        };
        let enc = |s: &str| s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
        let login = match (&c.user, &c.password) {
            (Some(u), Some(p)) => format!("{}:{}@", enc(u), enc(p)),
            (Some(u), None) => format!("{}@", enc(u)),
            _ => String::new(),
        };
        let uri = format!("mongodb://{login}{addr}/?authSource=admin&directConnection=true&serverSelectionTimeoutMS=5000");
        let mut m = Mongo { exec, uri, db: String::new(), databases: Vec::new() };
        let names = match m.eval("db.adminCommand({ listDatabases: 1, nameOnly: true }).databases.map(d => d.name)").await {
            Ok(v) => v,
            Err(e) if ["Authentication failed", "requires authentication", "Unauthorized", "not authorized"].iter().any(|s| e.contains(s)) => return Err(Fail::Auth(e)),
            Err(e) => return Err(Fail::Other(e)),
        };
        m.databases = names.as_array().into_iter().flatten().filter_map(|n| n.as_str().map(String::from)).collect();
        let own = m.databases.iter().find(|d| !["admin", "config", "local"].contains(&d.as_str())).cloned();
        m.db = c.database.clone().or(own).unwrap_or_else(|| "test".into());
        Ok(m)
    }

    /// The value of a mongosh expression, as relaxed Extended JSON.
    async fn eval(&self, script: &str) -> Res<serde_json::Value> {
        let mut cmd = match &self.exec {
            Some(container) => {
                let mut c = crate::infra::docker().ok_or("Docker isn't installed")?;
                c.args(["exec", container, "mongosh"]);
                c
            }
            None => {
                let mut c = Command::new(host_mongosh().ok_or("mongosh isn't installed")?);
                c.env_clear().envs(crate::clean_env()).env("PATH", crate::login_path()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
                c
            }
        };
        cmd.args([&self.uri, "--quiet", "--norc", "--json=relaxed", "--eval", script]);
        let out = tauri::async_runtime::spawn_blocking(move || crate::usage::output_within(cmd, Duration::from_secs(20)))
            .await
            .map_err(|e| e.to_string())?
            .ok_or("Mongo didn't answer in time")?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let err = if err.trim().is_empty() { String::from_utf8_lossy(&out.stdout) } else { err };
            // With --json, an error is an object: its message, not its stack.
            let parsed = serde_json::from_str::<serde_json::Value>(err.trim()).ok();
            return Err(parsed.and_then(|v| v["message"].as_str().map(String::from)).unwrap_or_else(|| err.trim().to_string()));
        }
        serde_json::from_slice(&out.stdout).map_err(|e| format!("Unreadable reply from mongosh: {e}"))
    }

    pub async fn tree(&self, database: &str) -> Res<Vec<Table>> {
        let v = self
            .eval(&format!(
                "const d = db.getSiblingDB({}); d.getCollectionInfos().map(c => ({{ name: c.name, type: c.type, n: c.type === 'collection' ? d.getCollection(c.name).estimatedDocumentCount() : null }}))",
                js(database)
            ))
            .await?;
        let mut out: Vec<Table> = v
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| Table {
                schema: database.into(),
                name: c["name"].as_str().unwrap_or_default().into(),
                kind: c["type"].as_str().unwrap_or("collection").into(),
                rows: c["n"].as_i64(),
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub async fn rows(&self, b: &Browse) -> Res<Grid> {
        let filter = b.filter.as_deref().map(str::trim).filter(|f| !f.is_empty()).unwrap_or("{}");
        let sort = match &b.sort {
            Some(f) => format!("{{ [{}]: {} }}", js(f), if b.desc { -1 } else { 1 }),
            None => "{}".into(),
        };
        let script = format!(
            "const c = db.getSiblingDB({db}).getCollection({coll}); let f; \
             try {{ f = EJSON.parse({filter}); }} catch (e) {{ throw new Error('The filter is not JSON: ' + e.message); }} \
             ({{ total: c.countDocuments(f), docs: c.find(f).sort({sort}).skip({skip}).limit({limit}).toArray().map(d => [\
               d._id && d._id.toHexString ? d._id.toHexString() : EJSON.stringify(d._id, {{ relaxed: true }}), \
               EJSON.stringify(d, null, 0, {{ relaxed: true }})]) }})",
            db = js(&b.schema),
            coll = js(&b.table),
            filter = js(filter),
            skip = b.offset,
            limit = b.limit.min(1000),
        );
        let v = self.eval(&script).await?;
        let rows = v["docs"].as_array().into_iter().flatten().map(|d| (0..2).map(|i| d[i].as_str().map(String::from)).collect()).collect();
        Ok(Grid {
            columns: vec![Col { name: "_id".into(), kind: String::new() }, Col { name: "document".into(), kind: "json".into() }],
            rows,
            total: v["total"].as_i64(),
            key: vec!["_id".into()],
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn values_become_javascript_literals() {
        assert_eq!(super::js("it's \"x\"\n"), r#""it's \"x\"\n""#);
    }
}
