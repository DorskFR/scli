# scli

Token-frugal Slack CLI for coding agents. No MCP, no OAuth flow, no daemon — one
static binary driven by a single user token, with compact line-oriented output so
an LLM (or `grep`) can read it cheaply.

## Install

```sh
# prebuilt binary
curl -L https://github.com/dorskFR/scli/releases/latest/download/scli-linux-amd64 -o scli
chmod +x scli && sudo mv scli /usr/local/bin/

# or from source
cargo install --git https://github.com/dorskFR/scli

# or as a container
docker run --rm -e SLACK_TOKEN ghcr.io/dorskfr/scli read channels
```

## Setup

`scli` needs a Slack **user token** (`xoxp-…`) from a Slack app with the scopes
you intend to use: `channels:read`/`channels:history` (public channels),
`groups:read`/`groups:history` (private channels), `im:read`/`im:history`/`im:write`
(DMs; `read dm` opens the conversation), `mpim:read`/`mpim:history` (group DMs),
`users:read`, `chat:write`, `reactions:read`/`reactions:write`,
`files:read`/`files:write`, `search:read` for `read search`, and
`reminders:read`/`reminders:write` if you use reminders. Without the `groups:*`,
`im:*` or `mpim:*` scopes, `read channels --type private|dm|mpim` (and the
default `all`) fail with `missing_scope`.

```sh
export SLACK_TOKEN=xoxp-...

# or store it (multi-workspace, ~/.config/scli/config.json, mode 0600)
scli write auth myteam xoxp-...
scli read workspaces
scli write default myteam
scli --workspace myteam read channels
```

`SLACK_TOKEN` wins unless you pass `--workspace`.

### Auth methods

`scli` accepts either:

- **A token** — a normal `xoxp-…` user token (or `xoxb-…` bot token) from a Slack
  app you install. Acts as your user (xoxp) and works everywhere.
- **A browser session** — the `xoxc-…` token plus the `d` cookie (`xoxd-…`) copied
  from your logged-in Slack web client (DevTools → Application → Local Storage /
  Cookies). No app required; rides your existing login.

```sh
# token
scli write auth myteam xoxp-...

# session (xoxc token + xoxd cookie)
scli write auth myteam xoxc-... --cookie xoxd-...
# or via env
export SLACK_TOKEN=xoxc-... SLACK_COOKIE=xoxd-...
```

An `xoxc-` token without a cookie is rejected. Note session tokens don't survive a
Slack-side session refresh — re-copy them when they expire.

## Usage

Every Slack operation lives under an explicit `read` or `write` tier, so a
sandbox can gate access with two prefixes (`scli read` / `scli write`).

```
# read tier (nothing mutates Slack)
scli read channels [--type public|private|dm|mpim|all]   # ID<TAB>NAME mapping (DMs as dm:@user)
scli read users                                          # ID<TAB>NAME<TAB>REAL_NAME
scli read workspaces                                     # configured workspaces
scli read messages <channel> [-l N]                      # TS<TAB>USER<TAB>TEXT
scli read thread   <channel> <ts>                        # thread replies, same format
scli read dm       <@user> [-l N]                        # DM history, same format
scli read files    <channel> <ts> [--download DIR]       # list/fetch uploaded files + link attachments
scli read draft    <channel> [text|-] [--thread ts]      # compose locally, no send
scli read ls       <query>                               # search cached channels+users
scli read search   <query> [-l N] [--sort score|timestamp]  # full-text message search

# write tier (changes Slack or local creds)
scli write send   <channel> [text|-] [--thread ts] [-f FILE ...]
scli write react  <channel> <ts> <emoji>
scli write delete <channel> <ts>                         # chat.delete; prints deleted<TAB>ID<TAB>TS
scli write remind list                                   # DEPRECATED by Slack
scli write remind add "text" --at "in 30 minutes"        # DEPRECATED by Slack
scli write auth    <name> <token> [--cookie xoxd-…]      # save a workspace
scli write default <name>                                # set default workspace
scli write sync                                          # refresh id<->name cache
scli write update [--check]                              # self-update to latest release
```

### Output contract

Text output is one record per line, columns separated by a single TAB, so
`cut -f`/`awk -F'\t'` split it reliably even when message text contains runs
of spaces. Empty results print nothing on stdout; a `no messages`/`no channels`
style note goes to **stderr** and the exit code stays 0, so `| wc -l` is a true
count and `cut -f1` never sees a fake id. Message text is flattened to one line;
inline tags (`[thread:N]`, `[files:N]`, `[att: …]`) never contain a TAB.

Exit codes: `0` success (including empty results), `1` any error, `3` Slack
rate limit (HTTP 429 or `ratelimited`) — branch on `$?` rather than parsing the
stderr text, which still names the `Retry-After` when Slack sends one.

### JSON output

Add the global `--json` flag to any `read` command to get NDJSON: one compact
raw Slack record per line (the message, channel, user, file/attachment or
search match object exactly as the API returned it), so fields the text format
omits (`thread_ts`, `edited`, `is_private`, `permalink`, file ids, …) stay
reachable. `read ls` emits `{"kind":"chan"|"user","id":…,"name":…}` and
`read workspaces` emits `{"name":…,"default":bool}`. Empty results print
nothing. Text output is unchanged when the flag is absent.

```
scli read messages '#general' -l 50 --json | jq -r 'select(.thread_ts) | .ts'
```

`<channel>` accepts a raw ID (`C…/G…/D…`), `#name`, or `@user` (→ DM).
`<user>` accepts `Uxxxx`, a `name`, or a display name. Text args fall back to
stdin when omitted or given as `-`.

## Examples

```sh
scli write send '#general' 'deploy finished ✅'
echo "$REPORT" | scli write send @alice -
scli write react '#general' 1700000000.000100 thumbsup
scli write delete '#general' 1700000000.000100
scli read messages '#general' -l 50 | grep deploy
scli write send '#release' 'logs attached' -f build.log
```

## Notes

- **Search** (`read search`) is Slack's server-side full-text search
  (`search.messages`). The query passes through verbatim, so Slack modifiers work:
  `in:#chan`, `from:@user`, `before:`/`after:`/`on:`, `has:link`, `"exact phrase"`.
  Output is `CHANNEL_ID<TAB>CHANNEL<TAB>TS<TAB>USER<TAB>TEXT`, so a hit feeds
  straight into `scli read thread <CHANNEL_ID> <TS>`. Requires the `search:read`
  scope and a **user** token (xoxp-/xoxc-; bot tokens can't search). The endpoint
  is rate-limited (Tier 2, ~20 req/min); scli never sleeps or retries — on 429 it
  exits with code 3 so a calling agent knows to wait (see *Output contract*).
- **Delete** (`write delete`) calls `chat.delete` and only removes messages the
  token may delete: your own with a user token, the bot's own with a bot token.
  Thread replies have their own `ts`. Slack errors (`cant_delete_message`,
  `message_not_found`, `channel_not_found`) are printed as-is.
- **Drafts** aren't a public Slack API — `scli read draft` only composes the
  `chat.postMessage` payload locally and prints it as JSON for inspection; it
  never sends. To post, call `scli write send` with the same arguments (`send`
  reads stdin as message *text*, so piping the draft JSON into it would post the
  JSON literally).
- **Reminders** (`reminders.add`/`reminders.list`) were deprecated by Slack in
  2023 and may stop working without notice; `scli` warns on use.
- File uploads use the current `files.getUploadURLExternal` +
  `files.completeUploadExternal` flow (`files.upload` is deprecated).
- **Attachments vs files**: Slack messages carry two distinct things — uploaded
  `files` and the `attachments` array (link unfurls, bot/app rich cards whose
  body lives in `title`/`title_link`/`text`/`fields`). `read messages`/`read
  thread`/`read dm` tag messages with `[files:N]` and render each attachment
  inline as `[att: pretext | title link | text | field: value]`; `scli read
  files` lists both, printing each link attachment as a compact
  `attachment<TAB>pretext | title<TAB>link | …` line. `--download` fetches uploaded files only.
- **Block Kit**: bot/app messages usually carry their content in `blocks`, with
  an empty or stub `text`. `read messages`/`read thread`/`read dm` flatten
  section/header/context/rich_text/image blocks to one line (parts joined with
  ` | `, buttons collapsed to `[actions:N]`): when `text` is empty the block
  text replaces it; when both are present and differ it is appended as
  `[blocks: …]`. One physical line per message is always preserved, e.g.

  ```
  1700000000.000100<TAB>B0BOT<TAB>Deploy | *ok* | env: prod [att: Build #12 https://ci/12 | passed]
  1700000000.000200<TAB>U0ALICE<TAB>see the thread [thread:3] [blocks: see the thread | :tada:]
  ```
- **Self-update**: `scli write update` replaces the running binary in place with the
  matching asset from the latest GitHub release (Linux amd64/arm64, macOS arm64),
  verifying its `SHA256SUMS` checksum first. `scli write update --check` only reports
  whether a newer version exists. Every other command prints a one-line
  *"newer version available"* notice to **stderr** (so piped stdout stays clean),
  at most once per 24h; set `SCLI_NO_UPDATE_CHECK=1` to disable it.

## Agent setup

Drop this into your `CLAUDE.md` so an agent uses `scli` instead of a Slack MCP:

> Use the `scli` CLI for Slack. `SLACK_TOKEN` is set. Every operation is under a
> `read` or `write` tier. Read with `scli read messages/thread/dm`, map names with
> `scli read channels`/`scli read users`, search message content with
> `scli read search '<query>'` (Slack modifiers like `in:#chan from:@user` work;
> exit code 3 means rate limited — wait before retrying), post with
> `scli write send`, react with `scli write react`, remove your own message with
> `scli write delete <channel> <ts>`. Output is TAB-separated lines, empty
> results print nothing on stdout; add `--json` to any read for one raw Slack
> record per line (NDJSON) when you need fields the text omits.

## License

MIT
