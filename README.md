# scli

Slack CLI for coding agents: one static binary, one user token, one TAB-separated record per line.

[![release](https://img.shields.io/github/v/release/DorskFR/scli)](https://github.com/DorskFR/scli/releases)
[![CI](https://github.com/DorskFR/scli/actions/workflows/ci.yml/badge.svg)](https://github.com/DorskFR/scli/actions/workflows/ci.yml)
[![license](https://img.shields.io/github/license/DorskFR/scli)](LICENSE)

![scli listing channels, reading #deploys, searching and posting against a mock workspace](docs/assets/demo.gif)

Replaces a Slack MCP server for agents. Authenticates with a token from the environment; runs no OAuth flow and no daemon.

```sh
scli read channels                     # ID<TAB>NAME
scli read messages '#general' -l 50    # TS<TAB>USER<TAB>TEXT
scli read thread '#general' <ts>       # thread replies
scli read search 'from:@alice deploy'  # Slack full-text search
scli write send '#general' 'text'      # post, prints the message ts
scli write react '#general' <ts> eyes  # add a reaction
```

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

## Quick start

Requires a Slack user token (`xoxp-…`) from a Slack app. Scopes per command, browser-session auth and multiple workspaces: [docs/setup.md](docs/setup.md).

```sh
export SLACK_TOKEN=xoxp-...
scli read channels
```

## Usage

### Read and write tiers

`read` commands never change Slack; `write` commands change Slack or local credentials. A sandbox can allow `scli read` and gate `scli write`.

### Examples

```sh
scli write send '#general' 'deploy finished ✅'
echo "$REPORT" | scli write send @alice -
scli write react '#general' 1700000000.000100 thumbsup
scli write delete '#general' 1700000000.000100
scli read messages '#general' -l 50 | grep deploy
scli write send '#release' 'logs attached' -f build.log
scli read messages '#general' -l 50 --json | jq -r 'select(.thread_ts) | .ts'
```

`<channel>` is an ID, `#name` or `@user` (DM). Text arguments read stdin when omitted or `-`.

### Output

| Case | Behaviour |
|---|---|
| Records | One per line, columns split by one TAB, message text flattened to one line |
| Empty result | Nothing on stdout, a note on stderr, exit `0` |
| Error | Exit `1` |
| Rate limited | Exit `3` |
| `--json` on a `read` | Raw Slack records as NDJSON |

Full command list, search modifiers, Block Kit and attachment rendering: [docs/reference.md](docs/reference.md).

### Use from an agent

Add to `CLAUDE.md`:

```md
Use the `scli` CLI for Slack. `SLACK_TOKEN` is set. Every operation is under a
`read` or `write` tier. Read with `scli read messages/thread/dm`, map names with
`scli read channels`/`scli read users`, search with `scli read search '<query>'`
(Slack modifiers like `in:#chan from:@user` work; exit code 3 means rate limited,
wait before retrying), post with `scli write send`, react with `scli write react`,
delete own messages with `scli write delete <channel> <ts>`. Output is
TAB-separated lines; empty results print nothing on stdout. Add `--json` to any
read for one raw Slack record per line (NDJSON).
```

## Configuration

| Setting | Effect |
|---|---|
| `SLACK_TOKEN` | Token; wins over stored workspaces unless `--workspace` is passed |
| `SLACK_COOKIE` | `d` cookie for an `xoxc-` browser-session token |
| `--workspace NAME` | Use a workspace saved with `scli write auth` in `~/.config/scli/config.json` |
| `SCLI_NO_UPDATE_CHECK=1` | Disable the once-a-day update notice on stderr |

## Limitations

- Browser-session tokens expire with the Slack web session.
- Search needs a user token and `search:read`; Slack limits it to about 20 requests per minute. No retries: exit `3`.
- `write delete` removes only messages the token owns.
- `read draft` composes locally and never sends (Slack has no public drafts API).
- Reminders use an API Slack deprecated in 2023.

## Development

```sh
git clone https://github.com/DorskFR/scli && cd scli
cargo test

# regenerate docs/assets/demo.gif against the mock server in docs/demo/
cargo build --release
docker run --rm -v "$PWD:/vhs" ghcr.io/charmbracelet/vhs docs/demo.tape
```

## License

[MIT](LICENSE)
