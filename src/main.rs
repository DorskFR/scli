//! scli — token-frugal Slack CLI.
//!
//! One file on purpose (mirrors the `yt` project). Compact, line-oriented output;
//! a single user token (`SLACK_TOKEN`, xoxp-…) drives everything.

use std::io::Read as _;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
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
    /// Read-only Slack operations (safe to expose broadly).
    #[command(subcommand)]
    Read(ReadCmd),

    /// State-changing Slack operations (posting, reacting, credentials).
    #[command(subcommand)]
    Write(WriteCmd),

    /// Print a shell completion script: scli completions <bash|zsh|fish|powershell|elvish>
    ///
    /// e.g. `scli completions fish > ~/.config/fish/completions/scli.fish`
    Completions {
        /// Shell to generate completions for.
        shell: Shell,
    },
}

/// Read-only tier: nothing here mutates Slack state.
#[derive(Subcommand)]
enum ReadCmd {
    /// List channels as `ID\tNAME` (public, private, DMs, group DMs).
    Channels {
        /// public | private | dm | mpim | all
        #[arg(long, default_value = "all")]
        r#type: String,
        /// Optional case-insensitive substring filter (client-side).
        filter: Option<String>,
    },
    /// List users as `ID\tNAME\tREAL_NAME`.
    Users {
        /// Optional case-insensitive substring filter (client-side).
        filter: Option<String>,
    },
    /// List configured workspaces.
    Workspaces,

    /// Search cached channels AND users by case-insensitive substring.
    ///
    /// Prints `chan|user\tID\tNAME` — the quick way to find an id.
    Ls { query: String },

    /// Read recent messages in a channel.
    Messages {
        /// Channel ID (C…/G…/D…) or #name / @user.
        channel: String,
        #[arg(short, long, default_value_t = 20)]
        limit: u32,
    },
    /// Read a thread's replies: scli read thread <channel> <ts>
    Thread { channel: String, ts: String },
    /// Read a DM with a user: scli read dm <@user|Uxxxx>
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
}

/// Write tier: everything here changes Slack (or local credential) state.
#[derive(Subcommand)]
enum WriteCmd {
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

    /// Add a reaction: scli write react <channel> <ts> <emoji>
    React {
        channel: String,
        ts: String,
        /// Emoji name without colons, e.g. thumbsup
        emoji: String,
    },

    /// Reminders (DEPRECATED by Slack since 2023 — may stop working).
    #[command(subcommand)]
    Remind(Remind),

    /// Save a workspace to config: scli write auth <name> <token> [--cookie xoxd-…]
    ///
    /// Use a normal token (xoxp-/xoxb-), or a browser-session token (xoxc-…)
    /// together with --cookie <xoxd-…> copied from the Slack web client.
    Auth {
        name: String,
        token: String,
        /// The `d` cookie (xoxd-…) required for an xoxc- session token.
        #[arg(long)]
        cookie: Option<String>,
    },
    /// Set the default workspace: scli write default <name>
    Default { name: String },

    /// Force-refresh the local id<->name cache now.
    Sync,

    /// Update scli in place to the latest GitHub release.
    Update {
        /// Only report whether a newer version exists; don't install.
        #[arg(long)]
        check: bool,
    },
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
        // Bold red "error:" via anstream: color is auto-stripped when stderr is
        // not a TTY or NO_COLOR/CLICOLOR say so (agent-safe).
        let red = anstyle::Style::new()
            .fg_color(Some(anstyle::AnsiColor::Red.into()))
            .bold();
        anstream::eprintln!("{red}error:{red:#} {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    // `update` manages the binary itself; no Slack client and no update notice.
    if let Cmd::Write(WriteCmd::Update { check }) = cli.cmd {
        return self_update(check);
    }

    // `completions` emits a shell script to stdout; no Slack client, no notice.
    if let Cmd::Completions { shell } = cli.cmd {
        let mut cmd = Cli::command();
        let name = cmd.get_name().to_string();
        clap_complete::generate(shell, &mut cmd, name, &mut std::io::stdout());
        return Ok(());
    }

    // Best-effort "newer version available" notice on every other command.
    update_notice();

    // Offline commands handled before building a client.
    match &cli.cmd {
        Cmd::Write(WriteCmd::Auth {
            name,
            token,
            cookie,
        }) => return auth(name, token, cookie.as_deref()),
        Cmd::Read(ReadCmd::Workspaces) => return workspaces(),
        Cmd::Write(WriteCmd::Default { name }) => return set_default(name),
        Cmd::Read(ReadCmd::Draft {
            channel,
            text,
            thread,
        }) => {
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
        Cmd::Read(r) => match r {
            ReadCmd::Channels { r#type, filter } => c.channels(&r#type, filter.as_deref()),
            ReadCmd::Users { filter } => c.users(filter.as_deref()),
            ReadCmd::Messages { channel, limit } => c.read(&channel, limit),
            ReadCmd::Thread { channel, ts } => c.thread(&channel, &ts),
            ReadCmd::Dm { user, limit } => c.dm(&user, limit),
            ReadCmd::Files {
                channel,
                ts,
                download,
            } => c.files(&channel, &ts, download),
            ReadCmd::Ls { query } => c.ls(&query),
            // offline reads already handled
            ReadCmd::Workspaces | ReadCmd::Draft { .. } => unreachable!(),
        },
        Cmd::Write(w) => match w {
            WriteCmd::Send {
                channel,
                text,
                thread,
                file,
            } => c.send(&channel, read_text(text)?, thread, &file),
            WriteCmd::React { channel, ts, emoji } => c.react(&channel, &ts, &emoji),
            WriteCmd::Remind(Remind::List) => c.remind_list(),
            WriteCmd::Remind(Remind::Add { text, at }) => c.remind_add(&text, &at),
            WriteCmd::Sync => c.sync(),
            // offline writes already handled
            WriteCmd::Auth { .. } | WriteCmd::Default { .. } | WriteCmd::Update { .. } => {
                unreachable!()
            }
        },
        // self-managed commands already handled
        Cmd::Completions { .. } => unreachable!(),
    }?;
    Ok(())
}

/// Case-insensitive substring test.
fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
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
    /// The `d` cookie value (without the `d=` prefix), required for xoxc- tokens.
    cookie: Option<String>,
    /// Cache key for this workspace: config name, or `env-<hash>` for SLACK_TOKEN.
    workspace: String,
}

impl Client {
    fn resolve(workspace: Option<&str>) -> Result<Client> {
        // env wins when set and no explicit --workspace
        if workspace.is_none() {
            if let Ok(token) = std::env::var("SLACK_TOKEN") {
                if !token.is_empty() {
                    let cookie = std::env::var("SLACK_COOKIE").ok().filter(|s| !s.is_empty());
                    // Synthetic, token-scoped key so distinct env tokens don't share a cache.
                    let key = format!("env-{}", short_hash(&token));
                    return Client::new(token, cookie, key);
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
        let server = servers
            .and_then(|s| s.get(&name))
            .ok_or_else(|| anyhow!("unknown workspace '{name}'"))?;
        let token = server
            .get("token")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("workspace '{name}' has no token"))?
            .to_string();
        let cookie = server
            .get("cookie")
            .and_then(Value::as_str)
            .map(str::to_string);
        Client::new(token, cookie, name)
    }

    /// Build a client, validating that xoxc- session tokens carry a cookie.
    fn new(token: String, cookie: Option<String>, workspace: String) -> Result<Client> {
        if token.starts_with("xoxc-") && cookie.is_none() {
            bail!("xoxc- session token needs a cookie: pass --cookie xoxd-… (or set SLACK_COOKIE)");
        }
        Ok(Client {
            token,
            cookie,
            workspace,
        })
    }

    /// POST application/x-www-form-urlencoded (the Slack Web API convention).
    fn call(&self, method: &str, params: &[(&str, &str)]) -> Result<Value> {
        let mut req = ureq::post(&format!("{API}/{method}"))
            .set("Authorization", &format!("Bearer {}", self.token));
        if let Some(c) = &self.cookie {
            req = req.set("Cookie", &format!("d={c}"));
        }
        read(req.send_form(params), method)
    }

    /// GET an authenticated file URL (url_private), incl. the session cookie.
    fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let mut req = ureq::get(url).set("Authorization", &format!("Bearer {}", self.token));
        if let Some(c) = &self.cookie {
            req = req.set("Cookie", &format!("d={c}"));
        }
        let resp = req.call().with_context(|| format!("GET {url}"))?;
        let mut buf = Vec::new();
        resp.into_reader()
            .read_to_end(&mut buf)
            .context("reading file body")?;
        Ok(buf)
    }

    // --- channels / users -------------------------------------------------

    fn channels(&self, kind: &str, filter: Option<&str>) -> Result<()> {
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
                if filter.map(|q| contains_ci(&name, q)).unwrap_or(true) {
                    println!("{id}\t{name}");
                    n += 1;
                }
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

    fn users(&self, filter: Option<&str>) -> Result<()> {
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
                if filter
                    .map(|q| contains_ci(name, q) || contains_ci(real, q))
                    .unwrap_or(true)
                {
                    println!("{id}\t{name}\t{real}");
                }
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
            if let Some(na) = m["attachments"]
                .as_array()
                .map(|a| a.len())
                .filter(|n| *n > 0)
            {
                tags.push_str(&format!(" [attachments:{na}]"));
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
        let attachments = msg["attachments"].as_array().cloned().unwrap_or_default();
        if files.is_empty() && attachments.is_empty() {
            println!("no files or attachments");
            return Ok(());
        }
        // Link/rich attachments (unfurls, bot/app cards): content lives in the
        // attachments array, not files. Print it in compact, grep-friendly form.
        // Downloading applies to uploaded files only; attachments are always shown.
        for a in &attachments {
            let title = a["title"].as_str().unwrap_or("");
            let link = a["title_link"]
                .as_str()
                .or(a["from_url"].as_str())
                .unwrap_or("");
            let pretext = a["pretext"].as_str().unwrap_or("").replace('\n', " ");
            let text = a["text"].as_str().unwrap_or("").replace('\n', " ");
            let mut parts: Vec<String> = Vec::new();
            if !pretext.is_empty() {
                parts.push(pretext);
            }
            if !title.is_empty() || !link.is_empty() {
                parts.push(format!("{title}\t{link}").trim().to_string());
            }
            if !text.is_empty() {
                parts.push(text);
            }
            for f in a["fields"].as_array().into_iter().flatten() {
                let t = f["title"].as_str().unwrap_or("");
                let val = f["value"].as_str().unwrap_or("").replace('\n', " ");
                parts.push(format!("{t}: {val}").trim().to_string());
            }
            println!("attachment\t{}", parts.join(" | "));
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

    // --- cache ------------------------------------------------------------

    /// id<->name maps for this workspace: fresh cache when available, otherwise
    /// refetch from the API and rewrite. `force` always refetches.
    fn maps(&self, force: bool) -> Result<Maps> {
        if !force {
            if let Some(m) = load_maps(&self.workspace).filter(Maps::is_fresh) {
                return Ok(m);
            }
        }
        let m = self.fetch_maps()?;
        save_maps(&self.workspace, &m).ok();
        Ok(m)
    }

    /// Page channels (public+private) and users into a fresh `Maps`.
    fn fetch_maps(&self) -> Result<Maps> {
        let mut channels = Vec::new();
        let mut cursor = String::new();
        loop {
            let v = self.call(
                "conversations.list",
                &[
                    ("types", "public_channel,private_channel"),
                    ("limit", "200"),
                    ("exclude_archived", "true"),
                    ("cursor", &cursor),
                ],
            )?;
            for ch in v["channels"].as_array().unwrap_or(&vec![]).iter() {
                if let (Some(id), Some(name)) = (ch["id"].as_str(), ch["name"].as_str()) {
                    channels.push(Chan {
                        id: id.to_string(),
                        name: name.to_string(),
                    });
                }
            }
            cursor = next_cursor(&v);
            if cursor.is_empty() {
                break;
            }
        }

        let mut users = Vec::new();
        let mut cursor = String::new();
        loop {
            let v = self.call("users.list", &[("limit", "200"), ("cursor", &cursor)])?;
            for u in v["members"].as_array().unwrap_or(&vec![]).iter() {
                if u["deleted"].as_bool().unwrap_or(false) {
                    continue;
                }
                let Some(id) = u["id"].as_str() else { continue };
                users.push(Usr {
                    id: id.to_string(),
                    name: u["name"].as_str().unwrap_or("").to_string(),
                    real: u["profile"]["real_name"].as_str().unwrap_or("").to_string(),
                    display: u["profile"]["display_name"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                });
            }
            cursor = next_cursor(&v);
            if cursor.is_empty() {
                break;
            }
        }

        Ok(Maps {
            fetched: now_secs(),
            channels,
            users,
        })
    }

    /// `scli write sync` — force-refresh the cache now.
    fn sync(&self) -> Result<()> {
        let m = self.maps(true)?;
        println!(
            "synced {}: {} channels, {} users",
            self.workspace,
            m.channels.len(),
            m.users.len()
        );
        Ok(())
    }

    /// `scli read ls <substr>` — substring search across cached channels AND users.
    fn ls(&self, query: &str) -> Result<()> {
        let m = self.maps(false)?;
        let mut n = 0;
        for c in &m.channels {
            if contains_ci(&c.name, query) {
                println!("chan\t{}\t{}", c.id, c.name);
                n += 1;
            }
        }
        for u in &m.users {
            if contains_ci(&u.name, query)
                || contains_ci(&u.real, query)
                || contains_ci(&u.display, query)
            {
                println!("user\t{}\t{}", u.id, u.name);
                n += 1;
            }
        }
        if n == 0 {
            println!("no match for '{query}'");
        }
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
        // Cache-first: try fresh cache, and on a miss refetch once before failing.
        for force in [false, true] {
            if let Some(c) = self.maps(force)?.channels.iter().find(|c| c.name == name) {
                return Ok(c.id.clone());
            }
        }
        bail!("channel '{s}' not found")
    }

    fn resolve_user(&self, s: &str) -> Result<String> {
        let name = s.strip_prefix('@').unwrap_or(s);
        if name.starts_with('U') || name.starts_with('W') {
            return Ok(name.to_string());
        }
        for force in [false, true] {
            if let Some(u) = self
                .maps(force)?
                .users
                .iter()
                .find(|u| u.name == name || u.display == name)
            {
                return Ok(u.id.clone());
            }
        }
        bail!("user '{s}' not found")
    }
}

// ---------------------------------------------------------------------------
// Local id<->name cache: ~/.cache/scli/<workspace>.json (respects XDG_CACHE_HOME).
// ---------------------------------------------------------------------------

const CACHE_TTL_SECS: u64 = 300;

struct Chan {
    id: String,
    name: String,
}

struct Usr {
    id: String,
    name: String,
    real: String,
    display: String,
}

struct Maps {
    fetched: u64,
    channels: Vec<Chan>,
    users: Vec<Usr>,
}

impl Maps {
    fn is_fresh(&self) -> bool {
        now_secs().saturating_sub(self.fetched) < CACHE_TTL_SECS
    }

    fn to_json(&self) -> Value {
        serde_json::json!({
            "fetched": self.fetched,
            "channels": self.channels.iter()
                .map(|c| serde_json::json!({ "id": c.id, "name": c.name }))
                .collect::<Vec<_>>(),
            "users": self.users.iter()
                .map(|u| serde_json::json!({
                    "id": u.id, "name": u.name, "real": u.real, "display": u.display
                }))
                .collect::<Vec<_>>(),
        })
    }

    fn from_json(v: &Value) -> Maps {
        let s = |x: &Value, k: &str| x[k].as_str().unwrap_or("").to_string();
        Maps {
            fetched: v["fetched"].as_u64().unwrap_or(0),
            channels: v["channels"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|c| Chan {
                    id: s(c, "id"),
                    name: s(c, "name"),
                })
                .collect(),
            users: v["users"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|u| Usr {
                    id: s(u, "id"),
                    name: s(u, "name"),
                    real: s(u, "real"),
                    display: s(u, "display"),
                })
                .collect(),
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// First 8 hex chars of sha256(input) — a short, stable, non-reversible key.
fn short_hash(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(input.as_bytes());
    h.finalize()
        .iter()
        .take(4)
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn cache_path(workspace: &str) -> Result<PathBuf> {
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .context("no HOME/XDG_CACHE_HOME")?;
    Ok(base.join("scli").join(format!("{workspace}.json")))
}

fn load_maps(workspace: &str) -> Option<Maps> {
    let path = cache_path(workspace).ok()?;
    let s = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&s).ok()?;
    Some(Maps::from_json(&v))
}

fn save_maps(workspace: &str, m: &Maps) -> Result<()> {
    let path = cache_path(workspace)?;
    std::fs::create_dir_all(path.parent().unwrap()).context("creating cache dir")?;
    std::fs::write(&path, serde_json::to_string(&m.to_json())?).context("writing cache")?;
    Ok(())
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

fn auth(name: &str, token: &str, cookie: Option<&str>) -> Result<()> {
    if token.starts_with("xoxc-") && cookie.is_none() {
        bail!("xoxc- session token needs --cookie xoxd-… (copy the `d` cookie from the Slack web client)");
    }
    let mut cfg = load_config()?;
    if !cfg["servers"].is_object() {
        cfg["servers"] = serde_json::json!({});
    }
    let mut entry = serde_json::json!({ "token": token });
    if let Some(c) = cookie {
        entry["cookie"] = Value::String(c.to_string());
    }
    cfg["servers"][name] = entry;
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

// ---------------------------------------------------------------------------
// Self-update: `scli write update` + a once-a-day "newer version available" notice.
// Releases live at github.com/dorskFR/scli with assets `scli-{os}-{arch}` and a
// `SHA256SUMS` file (see .github/workflows/release.yml).
// ---------------------------------------------------------------------------

const LATEST_API: &str = "https://api.github.com/repos/dorskFR/scli/releases/latest";
const UA: &str = concat!("scli/", env!("CARGO_PKG_VERSION"));

/// The release-asset name for the host platform, e.g. `scli-linux-amd64`.
fn asset_name() -> Result<String> {
    let os = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "darwin",
        other => bail!("unsupported OS '{other}' (linux/macos only)"),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => bail!("unsupported arch '{other}' (x86_64/aarch64 only)"),
    };
    // macOS Intel is not published; only Apple Silicon (arm64) darwin builds exist.
    if os == "darwin" && arch == "amd64" {
        bail!("no macOS Intel (darwin-amd64) build is published; use Apple Silicon");
    }
    Ok(format!("scli-{os}-{arch}"))
}

/// Parse a `vX.Y.Z` (or `X.Y.Z`) tag into a comparable tuple. Missing/extra parts
/// are tolerated (defaulting to 0 / ignored).
fn parse_ver(s: &str) -> (u64, u64, u64) {
    let s = s.trim().trim_start_matches('v');
    let mut it = s
        .split(['.', '-', '+'])
        .map(|p| p.parse::<u64>().unwrap_or(0));
    (
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    )
}

/// Fetch the latest release JSON from GitHub (short timeout, sends a User-Agent).
fn fetch_latest() -> Result<Value> {
    let resp = ureq::get(LATEST_API)
        .set("User-Agent", UA)
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(10))
        .call();
    match resp {
        Ok(r) => r.into_json().context("parsing release JSON"),
        Err(ureq::Error::Status(code, r)) => {
            let txt = r.into_string().unwrap_or_default();
            bail!("GitHub API HTTP {code}: {txt}")
        }
        Err(e) => Err(e).context("querying GitHub releases"),
    }
}

fn self_update(check_only: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    let want = asset_name()?;
    let rel = fetch_latest()?;
    let tag = rel["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow!("release has no tag_name"))?;

    if parse_ver(tag) <= parse_ver(current) {
        println!("scli is up to date (v{current})");
        return Ok(());
    }

    if check_only {
        println!("{tag} available (you have v{current}) — run 'scli write update'");
        return Ok(());
    }

    eprintln!("updating scli v{current} -> {tag} ({want})");

    // Locate the asset and the checksums file in the release.
    let assets = rel["assets"].as_array().cloned().unwrap_or_default();
    let url_of = |name: &str| -> Option<String> {
        assets
            .iter()
            .find(|a| a["name"].as_str() == Some(name))
            .and_then(|a| a["browser_download_url"].as_str())
            .map(str::to_string)
    };
    let bin_url = url_of(&want).ok_or_else(|| anyhow!("release {tag} has no asset '{want}'"))?;
    let sums_url =
        url_of("SHA256SUMS").ok_or_else(|| anyhow!("release {tag} has no SHA256SUMS file"))?;

    // Download the new binary and verify its checksum before touching anything.
    let bytes = download(&bin_url)?;
    let sums = String::from_utf8(download(&sums_url)?).context("SHA256SUMS not UTF-8")?;
    let want_sum = sums
        .lines()
        .find_map(|l| {
            let (sum, file) = l.split_once("  ").or_else(|| l.split_once(' '))?;
            (file.trim() == want).then(|| sum.trim().to_lowercase())
        })
        .ok_or_else(|| anyhow!("no checksum for '{want}' in SHA256SUMS"))?;
    let got_sum = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(&bytes);
        h.finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    if got_sum != want_sum {
        bail!("checksum mismatch for {want}: expected {want_sum}, got {got_sum} (aborting)");
    }

    // Atomically swap the running binary: write a sibling temp file, then rename.
    let exe = std::env::current_exe().context("locating current executable")?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let dir = exe
        .parent()
        .ok_or_else(|| anyhow!("executable has no parent dir"))?;
    let tmp = dir.join(format!(".scli-update-{}", std::process::id()));
    std::fs::write(&tmp, &bytes).with_context(|| {
        format!(
            "writing {} (need write access to {})",
            tmp.display(),
            dir.display()
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .context("setting executable bit")?;
    }
    if let Err(e) = std::fs::rename(&tmp, &exe) {
        std::fs::remove_file(&tmp).ok();
        return Err(e).with_context(|| {
            format!(
                "replacing {} (different filesystem? install manually)",
                exe.display()
            )
        });
    }
    println!("updated to {tag}");
    Ok(())
}

/// Download a URL to bytes (follows redirects, sends a User-Agent).
fn download(url: &str) -> Result<Vec<u8>> {
    let resp = ureq::get(url)
        .set("User-Agent", UA)
        .timeout(std::time::Duration::from_secs(60))
        .call()
        .with_context(|| format!("downloading {url}"))?;
    let mut buf = Vec::new();
    resp.into_reader()
        .read_to_end(&mut buf)
        .context("reading download body")?;
    Ok(buf)
}

/// Best-effort, non-blocking-feeling notice: at most once/24h, ask GitHub for the
/// latest tag, cache it, and print to stderr if it's newer. Never errors out.
fn update_notice() {
    if std::env::var("SCLI_NO_UPDATE_CHECK")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        return;
    }
    let current = env!("CARGO_PKG_VERSION");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cache = update_cache_path();

    // Read cache; if checked within the last day, just use the cached tag.
    let cached: Value = cache
        .as_ref()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    let last = cached["checked"].as_u64().unwrap_or(0);

    let latest = if now.saturating_sub(last) < 86_400 {
        cached["latest"].as_str().map(str::to_string)
    } else {
        let tag = fetch_latest()
            .ok()
            .and_then(|r| r["tag_name"].as_str().map(str::to_string));
        if let (Ok(p), Some(t)) = (&cache, &tag) {
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::write(
                p,
                serde_json::json!({ "checked": now, "latest": t }).to_string(),
            )
            .ok();
        }
        tag
    };

    if let Some(t) = latest {
        if parse_ver(&t) > parse_ver(current) {
            let dim = anstyle::Style::new().dimmed();
            anstream::eprintln!(
                "{dim}scli: {t} available (you have v{current}) — run 'scli write update'{dim:#}"
            );
        }
    }
}

fn update_cache_path() -> Result<PathBuf> {
    Ok(config_path()?.parent().unwrap().join("update-check.json"))
}
