//! Redis: a key browser (SCAN by pattern, each key's type and TTL), a viewer for each value type, and a console that
//! runs read commands only unless the tab allows writes.

use super::creds::Cred;
use super::{Fail, Target};
use crate::Res;
use redis::aio::MultiplexedConnection;
use redis::Value as V;
use serde::Serialize;
use std::time::Duration;

pub struct Kv {
    con: MultiplexedConnection,
}

#[derive(Serialize, Debug)]
pub struct Key {
    key: String,
    kind: String,
    ttl: i64, // ms; -1 none
}

#[derive(Serialize, Debug)]
pub struct Keys {
    cursor: u64, // 0: the scan is done
    keys: Vec<Key>,
}

#[derive(Serialize, Debug)]
pub struct Value {
    kind: String,
    ttl: i64,
    size: i64, // members, fields, entries, or bytes
    value: serde_json::Value, // a string; [field, value] pairs; items; [member, score] pairs; [id, [field, value]…] entries
}

/// Shown at most, of a value with many members.
const SHOW: i64 = 1000;

/// Commands the console runs without write mode: they only read.
const READS: &[&str] = &[
    "get", "mget", "strlen", "getrange", "exists", "type", "ttl", "pttl", "expiretime", "keys", "scan", "randomkey", "dbsize",
    "hget", "hmget", "hgetall", "hkeys", "hvals", "hlen", "hexists", "hscan", "hstrlen", "hrandfield",
    "lrange", "llen", "lindex", "lpos", "smembers", "scard", "sismember", "smismember", "sscan", "srandmember",
    "zrange", "zrevrange", "zrangebyscore", "zscore", "zcard", "zcount", "zrank", "zscan",
    "xrange", "xrevrange", "xlen", "xinfo", "info", "ping", "echo", "time", "memory", "object", "lcs",
];
/// Never from the console: they block it, change the connection under the browser, or stop the server.
const NEVER: &[&str] = &["select", "monitor", "subscribe", "psubscribe", "ssubscribe", "sync", "psync", "shutdown", "debug", "quit"];

fn message(e: &redis::RedisError) -> String {
    e.to_string()
}

impl Kv {
    pub async fn connect(t: &Target, c: &Cred) -> Result<Kv, Fail> {
        let (host, port) = t.wire().ok_or_else(|| Fail::Other("This Redis container publishes no port: publish one to browse it".into()))?;
        let addr = redis::ConnectionAddr::Tcp(host.trim_matches(['[', ']']).into(), port);
        let mut settings = redis::RedisConnectionInfo::default();
        if let Some(p) = &c.password {
            settings = settings.set_password(p);
        }
        if let Some(u) = &c.user {
            settings = settings.set_username(u);
        }
        let info = redis::IntoConnectionInfo::into_connection_info(addr).map_err(|e| Fail::Other(message(&e)))?.set_redis_settings(settings);
        let client = redis::Client::open(info).map_err(|e| Fail::Other(message(&e)))?;
        let config = redis::AsyncConnectionConfig::new().set_connection_timeout(Some(Duration::from_secs(5))).set_response_timeout(Some(Duration::from_secs(15)));
        let mut con = match client.get_multiplexed_async_connection_with_config(&config).await {
            Ok(c) => c,
            Err(e) => return Err(classify(e)),
        };
        // Some servers only ask for the password on the first command.
        redis::cmd("PING").query_async::<String>(&mut con).await.map_err(classify)?;
        Ok(Kv { con })
    }

    async fn select(&mut self, db: i64) -> Res<()> {
        redis::cmd("SELECT").arg(db).query_async::<()>(&mut self.con).await.map_err(|e| message(&e))
    }

    /// Databases with keys (from INFO keyspace), db 0 always.
    pub async fn databases(&mut self) -> Res<Vec<String>> {
        let info: String = redis::cmd("INFO").arg("keyspace").query_async(&mut self.con).await.map_err(|e| message(&e))?;
        Ok(keyspace(&info))
    }

    pub async fn keys(&mut self, db: i64, pattern: &str, cursor: u64) -> Res<Keys> {
        self.select(db).await?;
        let pattern = if pattern.trim().is_empty() { "*" } else { pattern.trim() };
        let (next, names): (u64, Vec<String>) = redis::cmd("SCAN").arg(cursor).arg("MATCH").arg(pattern).arg("COUNT").arg(500).query_async(&mut self.con).await.map_err(|e| message(&e))?;
        let mut pipe = redis::pipe();
        for k in &names {
            pipe.cmd("TYPE").arg(k).cmd("PTTL").arg(k);
        }
        let meta: Vec<V> = if names.is_empty() { Vec::new() } else { pipe.query_async(&mut self.con).await.map_err(|e| message(&e))? };
        let mut keys: Vec<Key> = names
            .into_iter()
            .enumerate()
            .map(|(i, key)| Key { key, kind: plain(meta.get(i * 2)), ttl: int(meta.get(i * 2 + 1)) })
            .collect();
        keys.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(Keys { cursor: next, keys })
    }

    pub async fn value(&mut self, db: i64, key: &str) -> Res<Value> {
        self.select(db).await?;
        let (kind, ttl): (String, i64) = redis::pipe().cmd("TYPE").arg(key).cmd("PTTL").arg(key).query_async(&mut self.con).await.map_err(|e| message(&e))?;
        let q = |name: &str| {
            let mut c = redis::cmd(name);
            c.arg(key);
            c
        };
        let con = &mut self.con;
        let err = |e: redis::RedisError| message(&e);
        let (size, value) = match kind.as_str() {
            "none" => return Err(format!("{key} doesn't exist (any more)")),
            "string" => {
                let s: Vec<u8> = q("GET").query_async(con).await.map_err(err)?;
                (s.len() as i64, serde_json::Value::String(String::from_utf8_lossy(&s).into_owned()))
            }
            "hash" => {
                let n: i64 = q("HLEN").query_async(con).await.map_err(err)?;
                let (_, pairs): (u64, Vec<(String, String)>) = q("HSCAN").arg(0).arg("COUNT").arg(SHOW).query_async(con).await.map_err(err)?;
                (n, serde_json::json!(pairs))
            }
            "list" => {
                let n: i64 = q("LLEN").query_async(con).await.map_err(err)?;
                let items: Vec<String> = q("LRANGE").arg(0).arg(SHOW - 1).query_async(con).await.map_err(err)?;
                (n, serde_json::json!(items))
            }
            "set" => {
                let n: i64 = q("SCARD").query_async(con).await.map_err(err)?;
                let (_, mut items): (u64, Vec<String>) = q("SSCAN").arg(0).arg("COUNT").arg(SHOW).query_async(con).await.map_err(err)?;
                items.sort();
                (n, serde_json::json!(items))
            }
            "zset" => {
                let n: i64 = q("ZCARD").query_async(con).await.map_err(err)?;
                let items: Vec<(String, f64)> = q("ZRANGE").arg(0).arg(SHOW - 1).arg("WITHSCORES").query_async(con).await.map_err(err)?;
                (n, serde_json::json!(items))
            }
            "stream" => {
                let n: i64 = q("XLEN").query_async(con).await.map_err(err)?;
                let entries: Vec<(String, Vec<String>)> = q("XRANGE").arg("-").arg("+").arg("COUNT").arg(100).query_async(con).await.map_err(err)?;
                let entries: Vec<_> = entries.into_iter().map(|(id, kv)| serde_json::json!([id, kv.chunks(2).map(|p| p.to_vec()).collect::<Vec<_>>()])).collect();
                (n, serde_json::json!(entries))
            }
            other => (0, serde_json::Value::String(format!("A {other} value: query it in the console"))),
        };
        Ok(Value { kind, ttl, size, value })
    }

    pub async fn command(&mut self, db: i64, line: &str, write: bool) -> Res<String> {
        let args = split(line)?;
        let Some(name) = args.first().map(|a| a.to_lowercase()) else { return Ok(String::new()) };
        if NEVER.contains(&name.as_str()) {
            return Err(format!("{} isn't run from here", name.to_uppercase()));
        }
        // CONFIG GET / CLIENT LIST and the like read; the rest of those families don't.
        let sub = args.get(1).map(|a| a.to_lowercase()).unwrap_or_default();
        let reads = READS.contains(&name.as_str()) || (["config", "client", "command", "cluster", "xinfo"].contains(&name.as_str()) && ["get", "list", "info", "count", "docs", "stream", "groups"].contains(&sub.as_str()));
        if !write && !reads {
            return Err(format!("{} can change data: allow writes to run it", name.to_uppercase()));
        }
        self.select(db).await?;
        let mut c = redis::cmd(&args[0]);
        for a in &args[1..] {
            c.arg(a);
        }
        let v: V = c.query_async(&mut self.con).await.map_err(|e| message(&e))?;
        Ok(format(&v, 0))
    }
}

fn classify(e: redis::RedisError) -> Fail {
    let m = message(&e);
    let auth = ["NOAUTH", "WRONGPASS", "invalid password", "without any password configured", "Authentication required", "invalid username-password"].iter().any(|s| m.contains(s));
    if auth { Fail::Auth(m) } else { Fail::Other(m) }
}

fn plain(v: Option<&V>) -> String {
    match v {
        Some(V::SimpleString(s)) => s.clone(),
        Some(V::BulkString(b)) => String::from_utf8_lossy(b).into_owned(),
        Some(V::Okay) => "OK".into(),
        _ => String::new(),
    }
}
fn int(v: Option<&V>) -> i64 {
    match v {
        Some(V::Int(i)) => *i,
        _ => -1,
    }
}

fn keyspace(info: &str) -> Vec<String> {
    let mut dbs: Vec<i64> = info.lines().filter_map(|l| l.strip_prefix("db")?.split_once(':')?.0.parse().ok()).collect();
    dbs.push(0);
    dbs.sort();
    dbs.dedup();
    dbs.into_iter().map(|d| d.to_string()).collect()
}

/// A console line's arguments: words, "double" or 'single' quoted ones (with \" and \\ escapes in double quotes).
fn split(line: &str) -> Res<Vec<String>> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let Some(&first) = chars.peek() else { break };
        let mut word = String::new();
        if first == '"' || first == '\'' {
            chars.next();
            loop {
                match chars.next() {
                    None => return Err("A quote isn't closed".into()),
                    Some(c) if c == first => break,
                    Some('\\') if first == '"' => word.extend(chars.next()),
                    Some(c) => word.push(c),
                }
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                word.push(c);
                chars.next();
            }
        }
        out.push(word);
    }
    Ok(out)
}

/// A reply the way redis-cli shows it.
fn format(v: &V, depth: usize) -> String {
    let pad = "   ".repeat(depth);
    let list = |items: Vec<String>| -> String {
        if items.is_empty() {
            return "(empty array)".into();
        }
        items.iter().enumerate().map(|(i, s)| format!("{}{}) {}", if i == 0 { String::new() } else { pad.clone() }, i + 1, s)).collect::<Vec<_>>().join("\n")
    };
    match v {
        V::Nil => "(nil)".into(),
        V::Int(i) => format!("(integer) {i}"),
        V::BulkString(b) => format!("{:?}", String::from_utf8_lossy(b)),
        V::SimpleString(s) => s.clone(),
        V::Okay => "OK".into(),
        V::Double(d) => format!("(double) {d}"),
        V::Boolean(b) => format!("({b})"),
        V::Array(items) | V::Set(items) => list(items.iter().map(|i| format(i, depth + 1)).collect()),
        V::Map(pairs) => list(pairs.iter().flat_map(|(k, v)| [format(k, depth + 1), format(v, depth + 1)]).collect()),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_console_lines() {
        assert_eq!(split(r#"SET "a key" 'it''s' "say \"hi\"""#).unwrap(), ["SET", "a key", "it", "s", "say \"hi\""]);
        assert!(split("GET \"open").is_err());
        assert!(split("   ").unwrap().is_empty());
    }

    #[test]
    fn formats_replies_like_redis_cli() {
        assert_eq!(format(&V::Nil, 0), "(nil)");
        assert_eq!(format(&V::Array(vec![V::BulkString(b"a".to_vec()), V::Int(2)]), 0), "1) \"a\"\n2) (integer) 2");
        assert_eq!(format(&V::Array(vec![]), 0), "(empty array)");
    }

    #[test]
    fn lists_databases_with_keys() {
        assert_eq!(keyspace("# Keyspace\r\ndb0:keys=5,expires=1\r\ndb3:keys=1,expires=0\r\n"), ["0", "3"]);
        assert_eq!(keyspace("# Keyspace\r\n"), ["0"]);
    }
}
