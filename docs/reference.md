# Reference

## Commands

Every Slack operation is under an explicit `read` or `write` tier, so a sandbox can gate access with two prefixes (`scli read` / `scli write`).

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

`<channel>` accepts a raw ID (`C…/G…/D…`), `#name`, or `@user` (→ DM). `<user>` accepts `Uxxxx`, a `name`, or a display name. Text arguments fall back to stdin when omitted or given as `-`.

## Output contract

Text output is one record per line, columns separated by a single TAB, so `cut -f` and `awk -F'\t'` split it even when message text contains runs of spaces. Message text is flattened to one line; inline tags (`[thread:N]`, `[files:N]`, `[att: …]`) never contain a TAB.

Empty results print nothing on stdout. A `no messages` / `no channels` style note goes to stderr and the exit code stays 0, so `| wc -l` is a true count and `cut -f1` never sees a fake id.

| Exit code | Meaning |
|---|---|
| `0` | success, including empty results |
| `1` | any error |
| `3` | Slack rate limit (HTTP 429 or `ratelimited`); stderr names the `Retry-After` when Slack sends one |

## JSON output

The global `--json` flag on any `read` command prints NDJSON: one compact raw Slack record per line (the message, channel, user, file/attachment or search match object as the API returned it). Fields the text format omits (`thread_ts`, `edited`, `is_private`, `permalink`, file ids, …) stay reachable. `read ls` emits `{"kind":"chan"|"user","id":…,"name":…}` and `read workspaces` emits `{"name":…,"default":bool}`. Empty results print nothing. Text output is unchanged without the flag.

```
scli read messages '#general' -l 50 --json | jq -r 'select(.thread_ts) | .ts'
```

## Search

`read search` is Slack's server-side full-text search (`search.messages`). The query passes through verbatim, so Slack modifiers work: `in:#chan`, `from:@user`, `before:`/`after:`/`on:`, `has:link`, `"exact phrase"`.

Output is `CHANNEL_ID<TAB>CHANNEL<TAB>TS<TAB>USER<TAB>TEXT`, so a hit feeds into `scli read thread <CHANNEL_ID> <TS>`.

Requires the `search:read` scope and a user token (`xoxp-`/`xoxc-`; bot tokens cannot search). The endpoint is rate-limited (Tier 2, ~20 req/min). `scli` never sleeps or retries: on 429 it exits with code 3.

## Delete

`write delete` calls `chat.delete` and removes only messages the token may delete: the user's own with a user token, the bot's own with a bot token. Thread replies have their own `ts`. Slack errors (`cant_delete_message`, `message_not_found`, `channel_not_found`) are printed as-is.

## Drafts

Drafts are not a public Slack API. `read draft` composes the `chat.postMessage` payload locally and prints it as JSON; it never sends. To post, run `write send` with the same arguments. `send` reads stdin as message text, so piping the draft JSON into it posts the JSON literally.

## Reminders

`reminders.add` / `reminders.list` were deprecated by Slack in 2023 and may stop working without notice. `scli` warns on use.

## Files and attachments

Uploads use `files.getUploadURLExternal` + `files.completeUploadExternal` (`files.upload` is deprecated).

Slack messages carry two distinct things: uploaded `files`, and the `attachments` array (link unfurls, bot/app cards whose body lives in `title`/`title_link`/`text`/`fields`). `read messages`, `read thread` and `read dm` tag messages with `[files:N]` and render each attachment inline as `[att: pretext | title link | text | field: value]`. `read files` lists both, printing each link attachment as `attachment<TAB>pretext | title<TAB>link | …`. `--download` fetches uploaded files only.

## Block Kit

Bot/app messages usually carry their content in `blocks`, with an empty or stub `text`. `read messages`, `read thread` and `read dm` flatten section/header/context/rich_text/image blocks to one line (parts joined with ` | `, buttons collapsed to `[actions:N]`). When `text` is empty the block text replaces it; when both are present and differ it is appended as `[blocks: …]`. One physical line per message is preserved:

```
1700000000.000100<TAB>B0BOT<TAB>Deploy | *ok* | env: prod [att: Build #12 https://ci/12 | passed]
1700000000.000200<TAB>U0ALICE<TAB>see the thread [thread:3] [blocks: see the thread | :tada:]
```

## Self-update

`write update` replaces the running binary in place with the matching asset from the latest GitHub release (Linux amd64/arm64, macOS arm64), after checking it against `SHA256SUMS`. `write update --check` only reports whether a newer version exists.

Every other command prints a one-line "newer version available" notice to stderr (piped stdout stays clean), at most once per 24h. `SCLI_NO_UPDATE_CHECK=1` disables it.

## Environment variables

| Variable | Effect |
|---|---|
| `SLACK_TOKEN` | Token to use; wins over the config file unless `--workspace` is passed |
| `SLACK_COOKIE` | `d` cookie (`xoxd-…`) for an `xoxc-` session token |
| `SCLI_NO_UPDATE_CHECK=1` | Disables the update notice |
| `SCLI_API_URL` | Slack Web API base URL (default `https://slack.com/api`); used by the demo's mock server |
