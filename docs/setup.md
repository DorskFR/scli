# Setup

## Token scopes

`scli` needs a Slack **user token** (`xoxp-…`) from a Slack app with the scopes for the commands in use:

| Scope | Needed for |
|---|---|
| `channels:read`, `channels:history` | public channels |
| `groups:read`, `groups:history` | private channels |
| `im:read`, `im:history` | DMs (`read dm`, `read messages @user`); add `im:write` for `write send @user` |
| `mpim:read`, `mpim:history` | group DMs |
| `users:read` | user lookup |
| `chat:write` | `write send`, `write delete` |
| `reactions:read`, `reactions:write` | reactions |
| `files:read`, `files:write` | `read files`, `write send -f` |
| `search:read` | `read search` |
| `reminders:read`, `reminders:write` | `write remind` |

Without the `groups:*`, `im:*` or `mpim:*` scopes, `read channels --type private|dm|mpim` (and the default `all`) fail with `missing_scope`.

## Auth methods

| Method | Credentials | Notes |
|---|---|---|
| Token | `xoxp-…` user token, or `xoxb-…` bot token, from an installed Slack app | `xoxp` acts as the user and works everywhere |
| Browser session | `xoxc-…` token plus the `d` cookie (`xoxd-…`) from a logged-in Slack web client (DevTools → Application → Local Storage / Cookies) | No app required. Expires on a Slack-side session refresh; copy both again |

```sh
# token, read from stdin (paste it at the prompt, or pipe it in)
scli write auth myteam
pass show slack/myteam | scli write auth myteam

# session (xoxc token on stdin + xoxd cookie from the env)
SLACK_COOKIE=xoxd-... scli write auth myteam < xoxc-token.txt
# or use both from the env without saving
export SLACK_TOKEN=xoxc-... SLACK_COOKIE=xoxd-...
```

An `xoxc-` token without a cookie is rejected. Passing the token or `--cookie` as arguments still works but prints a warning: arguments land in shell history and the process list.

## Multiple workspaces

Workspaces are stored in `~/.config/scli/config.json` (written atomically with mode 0600 in a 0700 directory; `$XDG_CONFIG_HOME` is honoured).

```sh
scli write auth myteam < token.txt
scli read workspaces
scli write default myteam
scli --workspace myteam read channels
```

Resolution order: `SLACK_TOKEN` (with `SLACK_COOKIE`) wins unless `--workspace` is passed; then the configured default; then the only stored workspace, if there is exactly one.

The id↔name cache lives under `~/.cache/scli/` (`$XDG_CACHE_HOME` is honoured), one file per workspace. Channels, users and DMs are cached separately for an hour; a lookup that misses refetches only the directory it needs. `scli write sync` refreshes channels and users now.
