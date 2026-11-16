# gidoractl

`gidoractl` controls a running Gidora browser from the command line. It is made for agents and scripts: open a tab in the real, signed-in profile, navigate it, read the page, run a script in it, take a screenshot, click, type, and close the tab.

The browser does all the work. `gidoractl` sends one request over a unix socket and prints the answer. It has no page heuristics: it returns text, HTML, script values and images, and the caller decides what to do with them.

## Turning it on

The control socket is off by default.

1. Open `chrome://settings/gidora` in Gidora.
2. Turn on **Agent control socket**.

The browser then listens on `control.sock` in its user data directory (`~/Library/Application Support/space.pkarpovich.gidora/control.sock`). The socket has mode 0600 and accepts only processes of the same user. There is no other authentication: any process that runs as you can control the browser while the switch is on.

## Installing

```sh
brew install --cask pkarpovich/apps/gidora
```

The cask installs Gidora and links `gidoractl` into the Homebrew `bin` folder. The client lives inside the app bundle (`Gidora.app/Contents/MacOS/gidoractl`), so the browser and its client always have the same version, and `brew upgrade` updates both.

When working on the code, `mise run ctl-install` builds the client from this checkout and puts it into `~/.local/bin`.

## Commands

| Command | What it does | Prints |
|---|---|---|
| `gidoractl version` | Checks that the browser answers | the client version and the browser version |
| `gidoractl tab list` | Lists the tabs of normal windows | one line per tab: id, window id, `*` for the active tab or `-`, URL, title, separated by tabs |
| `gidoractl tab new <url>` | Opens a tab in the background and waits for the page to load | the id of the new tab |
| `gidoractl tab go <url>` | Navigates a tab and waits for the page to load | the URL the tab ended on |
| `gidoractl tab close` | Closes a tab | nothing |
| `gidoractl tab text` | Reads the visible text of the page | the text |
| `gidoractl tab html` | Reads the HTML of the page | the HTML |
| `gidoractl tab eval <js>` | Runs a script in the page and waits for a promise | the value as one line of JSON |
| `gidoractl tab shot` | Takes a PNG screenshot of the viewport | the path of the file |
| `gidoractl tab click <selector>` | Scrolls the first matching element into view and clicks its centre | nothing |
| `gidoractl tab click --at <x>,<y>` | Clicks a point of the viewport | nothing |
| `gidoractl tab type <text>` | Inserts text into the focused element | nothing |

Every `tab` command except `list` and `new` takes `--tab <id>`. Without it the command acts on the active tab of the most recently used window, which is the tab the user is looking at.

```sh
tab=$(gidoractl tab new "https://example.com/")
gidoractl tab text --tab "$tab"
gidoractl tab eval --tab "$tab" -r "document.title"
gidoractl tab shot --tab "$tab" -o page.png
gidoractl tab close --tab "$tab"
```

### Behaviour worth knowing

- `tab new` never activates the new tab, its window or the application. The user keeps working in their own tab.
- A tab opened by `tab new` behaves like a visible, focused page for as long as it lives, so pages that render on animation frames keep working in the background. Other tabs are not changed.
- `tab new` and `tab go` accept only `http` and `https` URLs. They answer when the page stops loading, or after 30 seconds. Add `--json` to see the `loading` flag.
- Page commands work only on pages loaded over `http` or `https`. Browser pages such as `chrome://settings` are refused.
- Incognito windows are invisible to the socket.
- A page command that does not finish in 30 seconds fails with `timed out after 30 s`.
- `tab type` inserts text. It does not press keys such as Enter. Submit a form with `tab click` or with `tab eval`.
- If a site shows a human check, open the tab yourself, solve it, and let the caller continue. The tab is an ordinary tab.

## Pipes and scripts

- stdout carries only the payload. Errors go to stderr as `gidoractl: <message>`.
- Exit code 0 means success, 1 means the browser refused the command or could not be reached, 2 means a usage error.
- `gidoractl tab eval -` reads the script from stdin. `gidoractl tab type -` reads the text from stdin.
- `gidoractl tab eval -r` prints a string value without JSON quoting, like `jq -r`.
- `gidoractl tab shot -o -` writes the PNG to stdout. `-o <path>` chooses the file. Without `-o` the file goes to the system temp directory.
- A closed downstream pipe ends the command quietly.
- There is no colour, no progress output and no prompt.

```sh
gidoractl tab list | awk -F'\t' '$3 == "*" { print $4 }'
gidoractl tab eval --tab "$tab" - < extract.js | jq '.offers[]'
gidoractl tab shot --tab "$tab" -o - > page.png
```

## The wire protocol

`--json` prints the response line of the browser unchanged, for every command.

One connection carries one request line and one response line:

```json
{"cmd":"tab.eval","args":{"tab":42,"expression":"document.title"}}
{"ok":true,"result":{"value":"Example Domain"}}
```

A failure is `{"ok":false,"error":"<message>"}`. A request may be up to 1 MiB. The browser waits 5 seconds for the request line. Command names on the wire are `version`, `tab.list`, `tab.new`, `tab.go`, `tab.close`, `tab.text`, `tab.html`, `tab.eval`, `tab.shot`, `tab.click` and `tab.type`.

## Socket path

`--socket <path>` or the environment variable `GIDORA_SOCKET` points the client at another socket, for example the one of a browser started with `--user-data-dir`.

## Claude plugin

The repository is a Claude Code plugin marketplace (`.claude-plugin/marketplace.json`) named `gidora`. It carries the `gidoractl` plugin (`plugins/gidoractl`): a skill that teaches an agent to drive the browser through this tool. The marketplace lives on the `gidora` branch, so name the branch when you add it:

```
claude plugin marketplace add "pkarpovich/gidora#gidora"
claude plugin install gidoractl@gidora
```

To try an edit to the skill before it is pushed, start Claude Code with `claude --plugin-dir ./plugins/gidoractl`, which shadows the installed copy for that session.

## Development

```sh
mise run ctl-check   # cargo fmt, clippy and the unit tests
mise run build       # the development browser
mise run e2e         # every command against the development browser on a temporary profile
```

The browser side lives in `patches/gidora/core/control-socket.patch`, in the directory `chrome/browser/gidora/control/` of the Chromium tree.
