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
docker run --rm -e SLACK_TOKEN ghcr.io/dorskfr/scli channels
```

## Setup

`scli` needs a Slack **user token** (`xoxp-…`) from a Slack app with the scopes
you intend to use (`channels:read`, `channels:history`, `users:read`,
`chat:write`, `reactions:read`, `reactions:write`, `files:read`, `files:write`,
`search:read` for `read search`, and `reminders:read`/`reminders:write` if you
use reminders).

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
scli read channels [--type public|private|dm|mpim|all]   # ID<TAB>NAME mapping
scli read users                                          # ID<TAB>NAME<TAB>REAL_NAME
scli read workspaces                                     # configured workspaces
scli read messages <channel> [-l N]                      # recent messages
scli read thread   <channel> <ts>                        # thread replies
scli read dm       <@user> [-l N]                        # DM history
scli read files    <channel> <ts> [--download DIR]       # list/fetch uploaded files + link attachments
scli read draft    <channel> [text|-] [--thread ts]      # compose locally, no send
scli read ls       <query>                               # search cached channels+users
scli read search   <query> [-l N] [--sort score|timestamp]  # full-text message search

# write tier (changes Slack or local creds)
scli write send   <channel> [text|-] [--thread ts] [-f FILE ...]
scli write react  <channel> <ts> <emoji>
scli write remind list                                   # DEPRECATED by Slack
scli write remind add "text" --at "in 30 minutes"        # DEPRECATED by Slack
scli write auth    <name> <token> [--cookie xoxd-…]      # save a workspace
scli write default <name>                                # set default workspace
scli write sync                                          # refresh id<->name cache
scli write update [--check]                              # self-update to latest release
```

`<channel>` accepts a raw ID (`C…/G…/D…`), `#name`, or `@user` (→ DM).
`<user>` accepts `Uxxxx`, a `name`, or a display name. Text args fall back to
stdin when omitted or given as `-`.

## Examples

```sh
scli write send '#general' 'deploy finished ✅'
echo "$REPORT" | scli write send @alice -
scli write react '#general' 1700000000.000100 thumbsup
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
  exits non-zero with `rate limited — retry after Ns` so a calling agent knows to
  wait.
- **Drafts** aren't a public Slack API — `scli draft` composes a payload locally;
  pipe it into `scli send` to actually post.
- **Reminders** (`reminders.add`/`reminders.list`) were deprecated by Slack in
  2023 and may stop working without notice; `scli` warns on use.
- File uploads use the current `files.getUploadURLExternal` +
  `files.completeUploadExternal` flow (`files.upload` is deprecated).
- **Attachments vs files**: Slack messages carry two distinct things — uploaded
  `files` and the `attachments` array (link unfurls, bot/app rich cards whose
  body lives in `title`/`title_link`/`text`/`fields`). `read messages`/`read
  thread`/`read dm` tag messages with `[files:N]` and `[attachments:N]`; `scli
  read files` lists both,
  printing each link attachment as a compact `attachment\t…` line. `--download`
  fetches uploaded files only.
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
> on a `rate limited — retry after Ns` error, wait that long before retrying),
> post with `scli write send`, react with `scli write react`. Output is
> `ID<TAB>...` lines — cheap to parse.

## License

MIT
