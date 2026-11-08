# Build, then render from the repo root:
#   cargo build --release
#   docker run --rm -v "$PWD:/vhs" ghcr.io/charmbracelet/vhs docs/demo.tape
Output docs/assets/demo.gif

Set Shell bash
Set FontSize 18
Set Width 1200
Set Height 430
Set Padding 24
Set Theme "Catppuccin Mocha"
Set TypingSpeed 35ms

Hide
Type "cd /vhs && unset $(compgen -e | grep -E '^SLACK_') && export HOME=/tmp/demo SCLI_NO_UPDATE_CHECK=1 SCLI_API_URL=http://127.0.0.1:18432/api PATH=/vhs/target/release:$PATH PS1='$ '"
Enter
Type "python3 docs/demo/mock_slack.py 18432 & sleep 1; scli write auth acme xoxp-demo >/dev/null; clear"
Enter
Sleep 1.5s
Show

Type "scli read channels"
Enter
Sleep 1.5s

Type "scli read messages '#deploys' -l 4"
Enter
Sleep 2s

Type "scli read search 'login page'"
Enter
Sleep 1.5s

Type "scli write send '#deploys' 'prod rollout done'"
Enter
Sleep 4s
