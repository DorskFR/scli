//! scli — token-frugal Slack CLI.
//!
//! One file on purpose (mirrors the `yt` project). Compact, line-oriented output;
//! a single user token (`SLACK_TOKEN`, xoxp-…) drives everything.

use std::io::Read as _;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::Value;

const API: &str = "https://slack.com/api";

#[derive(Parser)]
#[command(
    version,
    about = "Token-frugal Slack CLI (env: SLACK_TOKEN; config: ~/.config/scli/config.json)"
)]
struct Cli {
    /// Named workspace from config to use (overrides default).
    #[arg(long, global = true)]
    workspace: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Save a workspace token to config: scli auth <name> <xoxp-token>
    Auth { name: String, token: String },
    /// List configured workspaces.
    Workspaces,
    /// Set the default workspace: scli default <name>
    Default { name: String },

    /// List channels as `ID\tNAME` (public, private, DMs, group DMs).
    Channels {
        /// public | private | dm | mpim | all
        #[arg(long, default_value = "all")]
        r#type: String,
    },
    /// List users as `ID\tNAME\tREAL_NAME`.
    Users,

    /// Read recent messages in a channel.
    Read {
        /// Channel ID (C…/G…/D…) or #name / @user.
        channel: String,
        #[arg(short, long, default_value_t = 20)]
        limit: u32,
    },
    /// Read a thread's replies: scli thread <channel> <ts>
    Thread { channel: String, ts: String },
    /// Read a DM with a user: scli dm <@user|Uxxxx>
    Dm {
        user: String,
        #[arg(short, long, default_value_t = 20)]
        limit: u32,
    },

    /// List attachments on a message; optionally download them.
    Files {
        channel: String,
        ts: String,
        /// Download files into this directory instead of just listing.
        #[arg(long)]
        download: Option<PathBuf>,
    },

    /// Compose a message locally without sending (prints the payload).
    Draft {
        channel: String,
        /// Message text, or omit / "-" to read from stdin.
        text: Option<String>,
        #[arg(long)]
        thread: Option<String>,
    },
    /// Send a message; optionally in a thread and/or with file attachments.
    Send {
        channel: String,
        /// Message text, or omit / "-" to read from stdin.
        text: Option<String>,
        #[arg(long)]
        thread: Option<String>,
        /// Attach a file (repeatable).
        #[arg(short, long)]
        file: Vec<PathBuf>,
    },

    /// Add a reaction: scli react <channel> <ts> <emoji>
    React {
        channel: String,
        ts: String,
        /// Emoji name without colons, e.g. thumbsup
        emoji: String,
    },

    /// Reminders (DEPRECATED by Slack since 2023 — may stop working).
    #[command(subcommand)]
    Remind(Remind),
}

#[derive(Subcommand)]
enum Remind {
    /// List your reminders.
    List,
    /// Create a reminder: scli remind add "text" --at "in 30 minutes"
    Add {
        text: String,
        #[arg(long)]
        at: String,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    // Offline commands handled before building a client.
    match &cli.cmd {
        Cmd::Auth { name, token } => return auth(name, token),
        Cmd::Workspaces => return workspaces(),
        Cmd::Default { name } => return set_default(name),
        Cmd::Draft {
            channel,
            text,
            thread,
        } => {
            let body = read_text(text.clone())?;
            let mut p = serde_json::json!({ "channel": channel, "text": body });
            if let Some(t) = thread {
                p["thread_ts"] = Value::String(t.clone());
            }
            println!("{p}");
            return Ok(());
        }
        _ => {}
    }

    let c = Client::resolve(cli.workspace.as_deref())?;

    match cli.cmd {
        Cmd::Channels { r#type } => c.channels(&r#type),
        Cmd::Users => c.users(),
        Cmd::Read { channel, limit } => c.read(&channel, limit),
        Cmd::Thread { channel, ts } => c.thread(&channel, &ts),
        Cmd::Dm { user, limit } => c.dm(&user, limit),
        Cmd::Files {
            channel,
            ts,
            download,
        } => c.files(&channel, &ts, download),
        Cmd::Send {
            channel,
            text,
            thread,
            file,
        } => c.send(&channel, read_text(text)?, thread, &file),
        Cmd::React { channel, ts, emoji } => c.react(&channel, &ts, &emoji),
        Cmd::Remind(Remind::List) => c.remind_list(),
        Cmd::Remind(Remind::Add { text, at }) => c.remind_add(&text, &at),
        // offline commands already handled
        Cmd::Auth { .. } | Cmd::Workspaces | Cmd::Default { .. } | Cmd::Draft { .. } => {
            unreachable!()
        }
    }
}

/// Read message text from an arg, or stdin when omitted / "-".
fn read_text(arg: Option<String>) -> Result<String> {
    match arg.as_deref() {
        Some("-") | None => {
            let mut s = String::new();
            std::io::stdin()
                .read_to_string(&mut s)
                .context("reading stdin")?;
            Ok(s.trim_end().to_string())
        }
        Some(t) => Ok(t.to_string()),
    }
}

// ---------------------------------------------------------------------------
// HTTP client
// ---------------------------------------------------------------------------

struct Client {
    token: String,
}

impl Client {
    fn resolve(workspace: Option<&str>) -> Result<Client> {
        // env wins when set and no explicit --workspace
        if workspace.is_none() {
            if let Ok(token) = std::env::var("SLACK_TOKEN") {
                if !token.is_empty() {
                    return Ok(Client { token });
                }
            }
        }
        let cfg = load_config()?;
        let servers = cfg.get("servers").and_then(Value::as_object);
        let name = match workspace {
            Some(n) => n.to_string(),
            None => match cfg.get("default").and_then(Value::as_str) {
                Some(d) => d.to_string(),
                None => match servers {
                    Some(s) if s.len() == 1 => s.keys().next().unwrap().clone(),
                    _ => bail!("no workspace: set SLACK_TOKEN, or `scli auth <name> <token>`"),
                },
            },
        };
        let token = servers
            .and_then(|s| s.get(&name))
            .and_then(|w| w.get("token"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("unknown workspace '{name}'"))?
            .to_string();
        Ok(Client { token })
    }

    /// POST application/x-www-form-urlencoded (the Slack Web API convention).
    fn call(&self, method: &str, params: &[(&str, &str)]) -> Result<Value> {
        let resp = ureq::post(&format!("{API}/{method}"))
            .set("Authorization", &format!("Bearer {}", self.token))
            .send_form(params);
        read(resp, method)
    }

    /// GET with a Bearer header (used for downloading url_private).
    fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let resp = ureq::get(url)
            .set("Authorization", &format!("Bearer {}", self.token))
            .call()
            .with_context(|| format!("GET {url}"))?;
        let mut buf = Vec::new();
        resp.into_reader()
            .read_to_end(&mut buf)
            .context("reading file body")?;
        Ok(buf)
    }

    // --- channels / users -------------------------------------------------

    fn channels(&self, kind: &str) -> Result<()> {
        let types = match kind {
            "public" => "public_channel",
            "private" => "private_channel",
            "dm" => "im",
            "mpim" => "mpim",
            "all" => "public_channel,private_channel,mpim,im",
            other => bail!("unknown type '{other}' (public|private|dm|mpim|all)"),
        };
        let mut cursor = String::new();
        let mut n = 0;
        loop {
            let v = self.call(
                "conversations.list",
                &[
                    ("types", types),
                    ("limit", "200"),
                    ("exclude_archived", "true"),
                    ("cursor", &cursor),
                ],
            )?;
            for ch in v["channels"].as_array().unwrap_or(&vec![]).iter() {
                let id = ch["id"].as_str().unwrap_or("");
                let name = ch["name"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("dm:{}", ch["user"].as_str().unwrap_or("?")));
                println!("{id}\t{name}");
                n += 1;
            }
            cursor = next_cursor(&v);
            if cursor.is_empty() {
                break;
            }
        }
        if n == 0 {
            println!("no channels");
        }
        Ok(())
    }

    fn users(&self) -> Result<()> {
        let mut cursor = String::new();
        loop {
            let v = self.call("users.list", &[("limit", "200"), ("cursor", &cursor)])?;
            for u in v["members"].as_array().unwrap_or(&vec![]).iter() {
                if u["deleted"].as_bool().unwrap_or(false) {
                    continue;
                }
                let id = u["id"].as_str().unwrap_or("");
                let name = u["name"].as_str().unwrap_or("");
                let real = u["profile"]["real_name"].as_str().unwrap_or("");
                println!("{id}\t{name}\t{real}");
            }
            cursor = next_cursor(&v);
            if cursor.is_empty() {
                break;
            }
        }
        Ok(())
    }

    // --- reading ----------------------------------------------------------

    fn read(&self, channel: &str, limit: u32) -> Result<()> {
        let id = self.resolve_channel(channel)?;
        let lim = limit.to_string();
        let v = self.call(
            "conversations.history",
            &[("channel", &id), ("limit", &lim)],
        )?;
        self.print_messages(&v);
        Ok(())
    }

    fn thread(&self, channel: &str, ts: &str) -> Result<()> {
        let id = self.resolve_channel(channel)?;
        let v = self.call("conversations.replies", &[("channel", &id), ("ts", ts)])?;
        self.print_messages(&v);
        Ok(())
    }

    fn dm(&self, user: &str, limit: u32) -> Result<()> {
        let uid = self.resolve_user(user)?;
        let opened = self.call("conversations.open", &[("users", &uid)])?;
        let id = opened["channel"]["id"]
            .as_str()
            .ok_or_else(|| anyhow!("could not open DM"))?
            .to_string();
        self.read(&id, limit)
    }

    fn print_messages(&self, v: &Value) {
        let msgs = v["messages"].as_array().cloned().unwrap_or_default();
        if msgs.is_empty() {
            println!("no messages");
            return;
        }
        // history returns newest-first; show oldest-first for readability.
        for m in msgs.iter().rev() {
            let ts = m["ts"].as_str().unwrap_or("");
            let user = m["user"].as_str().or(m["bot_id"].as_str()).unwrap_or("?");
            let text = m["text"].as_str().unwrap_or("").replace('\n', " ");
            let mut tags = String::new();
            if let Some(r) = m["reply_count"].as_i64() {
                tags.push_str(&format!(" [thread:{r}]"));
            }
            let reacts = reactions_str(m);
            if !reacts.is_empty() {
                tags.push_str(&format!(" [{reacts}]"));
            }
            if m["files"].is_array() {
                let nf = m["files"].as_array().map(|a| a.len()).unwrap_or(0);
                tags.push_str(&format!(" [files:{nf}]"));
            }
            println!("{ts}  {user}  {text}{tags}");
        }
    }

    // --- files ------------------------------------------------------------

    fn files(&self, channel: &str, ts: &str, download: Option<PathBuf>) -> Result<()> {
        let id = self.resolve_channel(channel)?;
        // fetch the single message via replies(limit could include parent); use history around ts
        let v = self.call(
            "conversations.replies",
            &[("channel", &id), ("ts", ts), ("limit", "1")],
        )?;
        let msg = v["messages"]
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .ok_or_else(|| anyhow!("message not found"))?;
        let files = msg["files"].as_array().cloned().unwrap_or_default();
        if files.is_empty() {
            println!("no files");
            return Ok(());
        }
        for f in &files {
            let name = f["name"].as_str().unwrap_or("file");
            let url = f["url_private_download"]
                .as_str()
                .or(f["url_private"].as_str())
                .unwrap_or("");
            match &download {
                None => println!("{name}\t{url}"),
                Some(dir) => {
                    std::fs::create_dir_all(dir).ok();
                    let bytes = self.get_bytes(url)?;
                    let path = dir.join(name);
                    std::fs::write(&path, bytes)
                        .with_context(|| format!("writing {}", path.display()))?;
                    println!("saved {}", path.display());
                }
            }
        }
        Ok(())
    }

    // --- sending ----------------------------------------------------------

    fn send(
        &self,
        channel: &str,
        text: String,
        thread: Option<String>,
        files: &[PathBuf],
    ) -> Result<()> {
        let id = self.resolve_channel(channel)?;
        if files.is_empty() {
            let mut params = vec![("channel", id.as_str()), ("text", text.as_str())];
            if let Some(t) = &thread {
                params.push(("thread_ts", t));
            }
            let v = self.call("chat.postMessage", &params)?;
            println!("{}", v["ts"].as_str().unwrap_or("ok"));
        } else {
            for f in files {
                self.upload(&id, f, &text, thread.as_deref())?;
            }
            println!("ok");
        }
        Ok(())
    }

    /// Three-step external upload flow (files.upload is deprecated).
    fn upload(
        &self,
        channel: &str,
        path: &PathBuf,
        comment: &str,
        thread: Option<&str>,
    ) -> Result<()> {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("upload");
        let len = bytes.len().to_string();

        let v = self.call(
            "files.getUploadURLExternal",
            &[("filename", filename), ("length", &len)],
        )?;
        let upload_url = v["upload_url"]
            .as_str()
            .ok_or_else(|| anyhow!("no upload_url returned"))?;
        let file_id = v["file_id"].as_str().unwrap_or("").to_string();

        ureq::post(upload_url)
            .send_bytes(&bytes)
            .with_context(|| "uploading file bytes")?;

        let files_json = serde_json::json!([{ "id": file_id }]).to_string();
        let mut params = vec![
            ("files", files_json.as_str()),
            ("channel_id", channel),
            ("initial_comment", comment),
        ];
        if let Some(t) = thread {
            params.push(("thread_ts", t));
        }
        self.call("files.completeUploadExternal", &params)?;
        Ok(())
    }

    fn react(&self, channel: &str, ts: &str, emoji: &str) -> Result<()> {
        let id = self.resolve_channel(channel)?;
        let name = emoji.trim_matches(':');
        self.call(
            "reactions.add",
            &[("channel", &id), ("timestamp", ts), ("name", name)],
        )?;
        println!("ok");
        Ok(())
    }

    // --- reminders (deprecated) ------------------------------------------

    fn remind_list(&self) -> Result<()> {
        eprintln!("warning: Slack reminders.* is deprecated and may stop working");
        let v = self.call("reminders.list", &[])?;
        let rs = v["reminders"].as_array().cloned().unwrap_or_default();
        if rs.is_empty() {
            println!("no reminders");
            return Ok(());
        }
        for r in &rs {
            let id = r["id"].as_str().unwrap_or("");
            let time = r["time"].as_i64().unwrap_or(0);
            let text = r["text"].as_str().unwrap_or("");
            println!("{id}\t{time}\t{text}");
        }
        Ok(())
    }

    fn remind_add(&self, text: &str, at: &str) -> Result<()> {
        eprintln!("warning: Slack reminders.* is deprecated and may stop working");
        let v = self.call("reminders.add", &[("text", text), ("time", at)])?;
        println!("{}", v["reminder"]["id"].as_str().unwrap_or("ok"));
        Ok(())
    }

    // --- resolution helpers ----------------------------------------------

    /// Accept a raw ID, a #name, or a @user (resolved to a DM channel).
    fn resolve_channel(&self, s: &str) -> Result<String> {
        if let Some(name) = s.strip_prefix('@') {
            let uid = self.resolve_user(name)?;
            let v = self.call("conversations.open", &[("users", &uid)])?;
            return Ok(v["channel"]["id"].as_str().unwrap_or(s).to_string());
        }
        let name = s.strip_prefix('#').unwrap_or(s);
        if is_channel_id(name) {
            return Ok(name.to_string());
        }
        // look it up by name
        let mut cursor = String::new();
        loop {
            let v = self.call(
                "conversations.list",
                &[
                    ("types", "public_channel,private_channel"),
                    ("limit", "200"),
                    ("cursor", &cursor),
                ],
            )?;
            for ch in v["channels"].as_array().unwrap_or(&vec![]).iter() {
                if ch["name"].as_str() == Some(name) {
                    return Ok(ch["id"].as_str().unwrap_or(name).to_string());
                }
            }
            cursor = next_cursor(&v);
            if cursor.is_empty() {
                break;
            }
        }
        bail!("channel '{s}' not found")
    }

    fn resolve_user(&self, s: &str) -> Result<String> {
        let name = s.strip_prefix('@').unwrap_or(s);
        if name.starts_with('U') || name.starts_with('W') {
            return Ok(name.to_string());
        }
        let mut cursor = String::new();
        loop {
            let v = self.call("users.list", &[("limit", "200"), ("cursor", &cursor)])?;
            for u in v["members"].as_array().unwrap_or(&vec![]).iter() {
                if u["name"].as_str() == Some(name)
                    || u["profile"]["display_name"].as_str() == Some(name)
                {
                    return Ok(u["id"].as_str().unwrap_or(name).to_string());
                }
            }
            cursor = next_cursor(&v);
            if cursor.is_empty() {
                break;
            }
        }
        bail!("user '{s}' not found")
    }
}

fn is_channel_id(s: &str) -> bool {
    matches!(s.chars().next(), Some('C' | 'G' | 'D'))
        && s.chars().all(|c| c.is_ascii_alphanumeric())
}

fn next_cursor(v: &Value) -> String {
    v["response_metadata"]["next_cursor"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

fn reactions_str(m: &Value) -> String {
    m["reactions"]
        .as_array()
        .map(|rs| {
            rs.iter()
                .map(|r| {
                    format!(
                        "{}:{}",
                        r["name"].as_str().unwrap_or("?"),
                        r["count"].as_i64().unwrap_or(0)
                    )
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
}

/// Parse a Slack Web API response: enforce `ok: true`.
fn read(resp: Result<ureq::Response, ureq::Error>, method: &str) -> Result<Value> {
    let body = match resp {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => {
            let txt = r.into_string().unwrap_or_default();
            bail!("{method}: HTTP {code}: {txt}");
        }
        Err(e) => return Err(e).with_context(|| format!("{method} request failed")),
    };
    let v: Value = body
        .into_json()
        .with_context(|| format!("{method}: invalid JSON"))?;
    if !v["ok"].as_bool().unwrap_or(false) {
        let err = v["error"].as_str().unwrap_or("unknown_error");
        bail!("{method}: {err}");
    }
    Ok(v)
}

// ---------------------------------------------------------------------------
// Config: ~/.config/scli/config.json  {"default": name, "servers": {name: {token}}}
// ---------------------------------------------------------------------------

fn config_path() -> Result<PathBuf> {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".config")))
        .context("no HOME/XDG_CONFIG_HOME")?;
    Ok(base.join("scli").join("config.json"))
}

fn load_config() -> Result<Value> {
    let path = config_path()?;
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).context("parsing config.json"),
        Err(_) => Ok(serde_json::json!({ "servers": {} })),
    }
}

fn save_config(v: &Value) -> Result<()> {
    let path = config_path()?;
    std::fs::create_dir_all(path.parent().unwrap()).context("creating config dir")?;
    std::fs::write(&path, serde_json::to_string_pretty(v)?).context("writing config")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).ok();
    }
    Ok(())
}

fn auth(name: &str, token: &str) -> Result<()> {
    let mut cfg = load_config()?;
    if !cfg["servers"].is_object() {
        cfg["servers"] = serde_json::json!({});
    }
    cfg["servers"][name] = serde_json::json!({ "token": token });
    if !cfg["default"].is_string() {
        cfg["default"] = Value::String(name.to_string());
    }
    save_config(&cfg)?;
    println!("saved workspace '{name}'");
    Ok(())
}

fn workspaces() -> Result<()> {
    let cfg = load_config()?;
    let default = cfg["default"].as_str().unwrap_or("");
    match cfg["servers"].as_object() {
        Some(s) if !s.is_empty() => {
            for name in s.keys() {
                let mark = if name == default { " (default)" } else { "" };
                println!("{name}{mark}");
            }
        }
        _ => println!("no workspaces"),
    }
    Ok(())
}

fn set_default(name: &str) -> Result<()> {
    let mut cfg = load_config()?;
    if cfg["servers"].get(name).is_none() {
        bail!("unknown workspace '{name}'");
    }
    cfg["default"] = Value::String(name.to_string());
    save_config(&cfg)?;
    println!("default = {name}");
    Ok(())
}
