# scli

Token-frugal Slack CLI for coding agents. No MCP, no OAuth flow, no daemon — one
static binary driven by a single user token, with compact line-oriented output so
an LLM (or `grep`) can read it cheaply.

## Install

```sh
# prebuilt binary
curl -L https://github.com/dorskFR/scli/releases/latest/download/scli-linux-x86_64 -o scli
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
and `reminders:read`/`reminders:write` if you use reminders).

```sh
export SLACK_TOKEN=xoxp-...

# or store it (multi-workspace, ~/.config/scli/config.json, mode 0600)
scli auth myteam xoxp-...
scli workspaces
scli default myteam
scli --workspace myteam channels
```

`SLACK_TOKEN` wins unless you pass `--workspace`.

## Usage

```
scli channels [--type public|private|dm|mpim|all]   # ID<TAB>NAME mapping
scli users                                           # ID<TAB>NAME<TAB>REAL_NAME
scli read   <channel> [-l N]                         # recent messages
scli thread <channel> <ts>                           # thread replies
scli dm     <@user> [-l N]                           # DM history
scli files  <channel> <ts> [--download DIR]          # list/fetch attachments
scli draft  <channel> [text|-] [--thread ts]         # compose locally, no send
scli send   <channel> [text|-] [--thread ts] [-f FILE ...]
scli react  <channel> <ts> <emoji>
scli remind list                                     # DEPRECATED by Slack
scli remind add "text" --at "in 30 minutes"          # DEPRECATED by Slack
```

`<channel>` accepts a raw ID (`C…/G…/D…`), `#name`, or `@user` (→ DM).
`<user>` accepts `Uxxxx`, a `name`, or a display name. Text args fall back to
stdin when omitted or given as `-`.

## Examples

```sh
scli send '#general' 'deploy finished ✅'
echo "$REPORT" | scli send @alice -
scli react '#general' 1700000000.000100 thumbsup
scli read '#general' -l 50 | grep deploy
scli send '#release' 'logs attached' -f build.log
```

## Notes

- **Drafts** aren't a public Slack API — `scli draft` composes a payload locally;
  pipe it into `scli send` to actually post.
- **Reminders** (`reminders.add`/`reminders.list`) were deprecated by Slack in
  2023 and may stop working without notice; `scli` warns on use.
- File uploads use the current `files.getUploadURLExternal` +
  `files.completeUploadExternal` flow (`files.upload` is deprecated).

## Agent setup

Drop this into your `CLAUDE.md` so an agent uses `scli` instead of a Slack MCP:

> Use the `scli` CLI for Slack. `SLACK_TOKEN` is set. Read with `scli read/thread/dm`,
> map names with `scli channels`/`scli users`, post with `scli send`, react with
> `scli react`. Output is `ID<TAB>...` lines — cheap to parse.

## License

MIT
