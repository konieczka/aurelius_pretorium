//! The E2E tests' backend: the app's Servers and database commands over HTTP, so its frontend can run in a test
//! browser (Playwright can't drive the macOS webview). `POST /invoke/<command>` with the command's arguments as JSON
//! answers with its result, or a 500 with its error. Built only with the `e2e` feature.

use crate::{db, infra, Res};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

pub fn serve(addr: &str) {
    std::env::set_var("PRETORIUM_NO_KEYCHAIN", "1"); // test logins stay out of the user's Keychain
    let dbs = Arc::new(db::Dbs::default());
    let listener = TcpListener::bind(addr).unwrap_or_else(|e| panic!("e2e harness: can't listen on {addr}: {e}"));
    eprintln!("e2e harness: listening on {addr}");
    for stream in listener.incoming().flatten() {
        let dbs = dbs.clone();
        std::thread::spawn(move || {
            if let Err(e) = handle(stream, &dbs) {
                eprintln!("e2e harness: {e}");
            }
        });
    }
}

fn handle(stream: TcpStream, dbs: &db::Dbs) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
    let mut length = 0;
    loop {
        let mut h = String::new();
        reader.read_line(&mut h)?;
        if h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                length = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    let (status, reply) = match (method.as_str(), path.strip_prefix("/invoke/")) {
        ("OPTIONS", _) => ("204 No Content", String::new()),
        ("POST", Some(cmd)) => {
            let args: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            match tauri::async_runtime::block_on(dispatch(cmd, args, dbs)) {
                Ok(v) => ("200 OK", v.to_string()),
                Err(e) => ("500 Internal Server Error", Value::String(e).to_string()),
            }
        }
        _ => ("404 Not Found", "\"no such route\"".into()),
    };
    let mut out = stream;
    write!(
        out,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: POST, OPTIONS\r\nAccess-Control-Allow-Headers: content-type\r\nConnection: close\r\n\r\n{reply}",
        reply.len()
    )
}

fn json(r: Res<impl Serialize>) -> Res<Value> {
    r.and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
}

fn arg<T: serde::de::DeserializeOwned>(a: &Value, k: &str) -> Res<T> {
    serde_json::from_value(a[k].clone()).map_err(|e| format!("bad {k}: {e}"))
}

async fn dispatch(cmd: &str, a: Value, dbs: &db::Dbs) -> Res<Value> {
    let s = |k: &str| a[k].as_str().unwrap_or_default().to_string();
    let n = |k: &str| a[k].as_i64().unwrap_or_default();
    let b = |k: &str| a[k].as_bool().unwrap_or_default();
    match cmd {
        "infra_scan" => json(infra::scan(HashMap::new()).await),
        "infra_recipes" => json(infra::infra_recipes(s("dir")).await),
        "db_connect" => json(dbs.connect(arg(&a, "target")?).await),
        "db_disconnect" => {
            dbs.disconnect(&s("id"));
            Ok(Value::Null)
        }
        "db_tree" => json(dbs.tree(&s("id"), a["database"].as_str().map(String::from)).await),
        "db_rows" => json(dbs.rows(&s("id"), arg(&a, "browse")?).await),
        "db_query" => json(dbs.query(&s("id"), s("sql"), b("write")).await),
        "db_cancel" => json(dbs.cancel(&s("id")).await),
        "db_edit" => json(dbs.edit(&s("id"), arg(&a, "edit")?, b("apply")).await),
        "db_completion" => json(dbs.completion(&s("id")).await),
        "db_keys" => json(dbs.keys(&s("id"), n("db"), s("pattern"), n("cursor") as u64).await),
        "db_value" => json(dbs.value(&s("id"), n("db"), s("key")).await),
        "db_command" => json(dbs.command(&s("id"), n("db"), s("line"), b("write")).await),
        "db_files" => json(db::db_files(s("dir")).await),
        _ => Err(format!("The e2e harness doesn't serve {cmd}")),
    }
}
