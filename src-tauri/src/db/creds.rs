//! Logins to try, in order: one typed into the tab; the container's own settings (the official images take their
//! credentials from the environment); a URL in the project's .env files that points at this server; one remembered
//! in the Keychain; the defaults a local install accepts (Homebrew's Postgres lets the Mac user in).
//! Only .env files for local development are read: never .env.staging or .env.production.

use super::{Engine, Target, LOCAL};
use crate::Res;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Cred {
    pub user: Option<String>,
    pub password: Option<String>,
    pub database: Option<String>,
    pub via: String,
}

const ENV_FILES: [&str; 4] = [".env", ".env.local", ".env.development", ".env.development.local"];
const KEYCHAIN: &str = "Pretorium databases";

pub async fn candidates(t: &Target) -> Vec<Cred> {
    let t = t.clone();
    crate::blocking(move || Ok(collect(&t))).await.unwrap_or_default()
}

fn collect(t: &Target) -> Vec<Cred> {
    let mut out: Vec<Cred> = Vec::new();
    if t.engine == Engine::Sqlite {
        return vec![cred(None, None, None, "file")];
    }
    if t.user.is_some() || t.password.is_some() {
        out.push(cred(t.user.clone(), t.password.clone(), t.database.clone(), "login"));
    } else {
        if let Some(c) = &t.container {
            out.extend(from_container(t.engine, &inspect(c)));
        }
        // The project's folder, and the ones above it up to its checkout (a Compose file in a subfolder).
        if let (Some(root), Some(port)) = (&t.root, t.port) {
            for dir in Path::new(root).ancestors() {
                for f in ENV_FILES {
                    if let Ok(text) = std::fs::read_to_string(dir.join(f)) {
                        out.extend(from_env_file(t.engine, port, &text, f));
                    }
                }
                if dir.join(".git").exists() {
                    break;
                }
            }
        }
        out.extend(keychain(t));
        out.extend(defaults(t.engine));
    }
    // A database picked in the tab wins over the one each login names.
    if let Some(db) = &t.database {
        for c in &mut out {
            c.database = Some(db.clone());
        }
    }
    let mut seen: Vec<Cred> = Vec::new();
    out.retain(|c| {
        let same = seen.iter().any(|s| s.user == c.user && s.password == c.password && s.database == c.database);
        seen.push(c.clone());
        !same
    });
    out
}

fn cred(user: Option<String>, password: Option<String>, database: Option<String>, via: &str) -> Cred {
    Cred { user, password, database, via: via.into() }
}

/// A container's environment and command line.
fn inspect(container: &str) -> (Vec<String>, Vec<String>) {
    let Some(mut cmd) = crate::infra::docker() else { return Default::default() };
    cmd.args(["inspect", "--format", "{{json .Config.Env}}\n{{json .Config.Cmd}}", container]);
    let Some(out) = crate::usage::output_within(cmd, std::time::Duration::from_secs(5)) else { return Default::default() };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines().map(|l| serde_json::from_str::<Vec<String>>(l).unwrap_or_default());
    (lines.next().unwrap_or_default(), lines.next().unwrap_or_default())
}

fn from_container(engine: Engine, (env, cmd): &(Vec<String>, Vec<String>)) -> Vec<Cred> {
    let get = |k: &str| env.iter().find_map(|e| e.strip_prefix(k)?.strip_prefix('=').map(String::from));
    let any = |ks: &[&str]| ks.iter().find_map(|k| get(k));
    let via = "container settings";
    match engine {
        Engine::Postgres => {
            let user = get("POSTGRES_USER").unwrap_or_else(|| "postgres".into());
            let db = get("POSTGRES_DB").unwrap_or_else(|| user.clone());
            // Without a password in the environment (a _FILE secret), exec'ing in still works: the socket trusts.
            vec![cred(Some(user), get("POSTGRES_PASSWORD"), Some(db), via)]
        }
        Engine::Mysql => {
            let db = any(&["MYSQL_DATABASE", "MARIADB_DATABASE"]);
            let mut out = Vec::new();
            if let (Some(u), Some(p)) = (any(&["MYSQL_USER", "MARIADB_USER"]), any(&["MYSQL_PASSWORD", "MARIADB_PASSWORD"])) {
                out.push(cred(Some(u), Some(p), db.clone(), via));
            }
            if let Some(p) = any(&["MYSQL_ROOT_PASSWORD", "MARIADB_ROOT_PASSWORD"]) {
                out.push(cred(Some("root".into()), Some(p), db.clone(), via));
            } else if any(&["MYSQL_ALLOW_EMPTY_PASSWORD", "MARIADB_ALLOW_EMPTY_ROOT_PASSWORD"]).is_some() {
                out.push(cred(Some("root".into()), None, db, via));
            }
            out
        }
        Engine::Redis => {
            let flag = cmd.iter().position(|a| a == "--requirepass").and_then(|i| cmd.get(i + 1)).cloned();
            flag.or_else(|| get("REDIS_PASSWORD")).map(|p| vec![cred(None, Some(p), None, via)]).unwrap_or_default()
        }
        Engine::Mongo => match (get("MONGO_INITDB_ROOT_USERNAME"), get("MONGO_INITDB_ROOT_PASSWORD")) {
            (Some(u), Some(p)) => vec![cred(Some(u), Some(p), get("MONGO_INITDB_DATABASE"), via)],
            _ => Vec::new(),
        },
        Engine::Sqlite => Vec::new(),
    }
}

/// URLs in a .env file that point at this server: same scheme family, a local host, the same port.
fn from_env_file(engine: Engine, port: u16, text: &str, file: &str) -> Vec<Cred> {
    let schemes: &[&str] = match engine {
        Engine::Postgres => &["postgres", "postgresql"],
        Engine::Mysql => &["mysql", "mariadb", "mysql2"],
        Engine::Redis => &["redis"],
        Engine::Mongo => &["mongodb"],
        Engine::Sqlite => &[],
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim().trim_start_matches("export ").trim_start();
        let Some((_, value)) = line.split_once('=') else { continue };
        let value = value.trim().trim_matches(['"', '\'']);
        let Ok(url) = tauri::Url::parse(value) else { continue };
        let host = url.host_str().unwrap_or("");
        if !schemes.contains(&url.scheme()) || !(LOCAL.contains(&host) || host == "host.docker.internal") || url.port().unwrap_or(engine.port()) != port {
            continue;
        }
        let decode = |s: &str| percent_decode(s);
        let user = Some(decode(url.username())).filter(|u| !u.is_empty());
        let password = url.password().map(decode);
        let db = Some(url.path().trim_start_matches('/').to_string()).filter(|d| !d.is_empty() && engine != Engine::Redis);
        out.push(cred(user, password, db, file));
    }
    out
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (b[i], b.get(i + 1).copied().and_then(hex), b.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, _, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn defaults(engine: Engine) -> Vec<Cred> {
    let via = "default login";
    match engine {
        Engine::Postgres => {
            let me = std::env::var("USER").unwrap_or_else(|_| "postgres".into());
            vec![cred(Some(me), None, Some("postgres".into()), via), cred(Some("postgres".into()), None, Some("postgres".into()), via)]
        }
        Engine::Mysql => vec![cred(Some("root".into()), None, None, via)],
        Engine::Redis | Engine::Mongo => vec![cred(None, None, None, via)],
        Engine::Sqlite => Vec::new(),
    }
}

/// The Keychain item a server's login is remembered under.
fn account(t: &Target) -> String {
    let engine = serde_json::to_value(t.engine).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
    match (t.wire(), &t.container) {
        (Some((h, p)), _) => format!("{engine}://{h}:{p}"),
        (_, Some(c)) => format!("{engine}://container/{c}"),
        _ => engine,
    }
}

/// The E2E harness keeps its logins out of the user's Keychain.
fn keychain_off() -> bool {
    std::env::var_os("PRETORIUM_NO_KEYCHAIN").is_some()
}

fn keychain(t: &Target) -> Vec<Cred> {
    if keychain_off() || !cfg!(target_os = "macos") {
        return Vec::new();
    }
    let out = std::process::Command::new("/usr/bin/security").args(["find-generic-password", "-s", KEYCHAIN, "-a", &account(t), "-w"]).output();
    let Some(json) = out.ok().filter(|o| o.status.success()).and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok()) else { return Vec::new() };
    let s = |k: &str| json[k].as_str().map(String::from);
    vec![cred(s("user"), s("password"), s("database"), "Keychain")]
}

pub async fn remember(t: &Target, c: &Cred) {
    if keychain_off() || !cfg!(target_os = "macos") {
        return;
    }
    let (account, secret) = (account(t), serde_json::json!({ "user": c.user, "password": c.password, "database": c.database }).to_string());
    let _: Res<()> = crate::blocking(move || {
        // ponytail: the secret is in `security`'s argv for a moment (only this user's processes can read it)
        let _ = std::process::Command::new("/usr/bin/security").args(["add-generic-password", "-U", "-s", KEYCHAIN, "-a", &account, "-w", &secret]).output();
        Ok(())
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(vars: &[&str]) -> (Vec<String>, Vec<String>) {
        (vars.iter().map(|s| s.to_string()).collect(), Vec::new())
    }

    #[test]
    fn reads_logins_from_container_settings() {
        let pg = from_container(Engine::Postgres, &env(&["PATH=/bin", "POSTGRES_PASSWORD=pw", "POSTGRES_DB=shop"]));
        assert_eq!(pg, [cred(Some("postgres".into()), Some("pw".into()), Some("shop".into()), "container settings")]);
        let my = from_container(Engine::Mysql, &env(&["MARIADB_USER=app", "MARIADB_PASSWORD=a", "MARIADB_ROOT_PASSWORD=r", "MARIADB_DATABASE=shop"]));
        assert_eq!(my.iter().map(|c| c.user.clone().unwrap()).collect::<Vec<_>>(), ["app", "root"]);
        let redis = from_container(Engine::Redis, &(vec![], vec!["redis-server".into(), "--requirepass".into(), "s3".into()]));
        assert_eq!(redis[0].password.as_deref(), Some("s3"));
        assert!(from_container(Engine::Mongo, &env(&["MONGO_INITDB_ROOT_USERNAME=root"])).is_empty());
    }

    #[test]
    fn takes_env_urls_only_for_this_local_server() {
        let text = "# db\nDATABASE_URL=\"postgres://app:p%40ss@localhost:5433/shop\"\nexport TEST_DATABASE_URL=postgresql://t:t@127.0.0.1:5433/test\nREMOTE=postgres://a:b@db.example.com:5433/x\nOTHER_PORT=postgres://a:b@localhost:5432/x\nREDIS_URL=redis://localhost:5433\n";
        let c = from_env_file(Engine::Postgres, 5433, text, ".env");
        assert_eq!(c, [cred(Some("app".into()), Some("p@ss".into()), Some("shop".into()), ".env"), cred(Some("t".into()), Some("t".into()), Some("test".into()), ".env")]);
        assert_eq!(from_env_file(Engine::Mysql, 3306, "URL=mysql://root@localhost/app", ".env.local")[0].database.as_deref(), Some("app"));
    }

    #[test]
    fn a_typed_login_replaces_the_rest_and_a_picked_database_applies_to_all() {
        let t = Target {
            engine: Engine::Redis, host: Some("127.0.0.1".into()), port: Some(1), container: None, file: None, root: None,
            user: None, password: Some("x".into()), database: None, remember: false,
        };
        assert_eq!(collect(&t), [cred(None, Some("x".into()), None, "login")]);
        std::env::set_var("PRETORIUM_NO_KEYCHAIN", "1");
        let t = Target { engine: Engine::Postgres, password: None, database: Some("other".into()), ..t };
        assert!(collect(&t).iter().all(|c| c.database.as_deref() == Some("other") && c.via == "default login"));
    }
}
