# gidoractl: a control CLI for the Gidora browser

## Overview

`gidoractl` lets a command-line agent drive the running Gidora browser the way `agtermctl` drives agterm: open a tab in the user's real signed-in profile, navigate it, read its text or HTML, run its own scripts in it, take a screenshot, click, type and close it.

The feature has two parts. A control server inside the browser process owns a unix socket and executes every command. `gidoractl` is a thin Rust CLI that parses arguments, sends one request, and prints the reply. The tool returns raw material only. The agent does all parsing.

Acceptance scenario: an agent opens its own background tab, searches allegro.pl, reads the listing and one product page, runs its own script to extract offers, takes a screenshot and closes its tabs. The user's active tab and window focus never change. If the site shows a human check, the user opens that tab, solves it, and the agent continues.

A second, independent patch closes a gap of this non-Google-branded build: `--remote-debugging-port` and `--remote-debugging-pipe` currently work on the default profile.

### Non-goals

- No agent skill for `gidoractl` and no MCP server.
- No tab move, pin, group, reorder or window management commands.
- No event stream or subscription.
- No accessibility snapshot, element refs or any other page heuristic.
- No cookie-value command, no clipboard access, no file upload or download commands.
- No key-press command (Enter, Tab, shortcuts). `tab type` inserts text only.
- No full-page screenshot option. `tab shot` captures the viewport.
- No authentication beyond socket permissions: no token, no prompt, no caller signature check.
- The CLI is not put inside the app bundle.
- No Chromium `browser_tests` or `unit_tests` targets.

### Rejected alternatives

- External DevTools protocol over `--remote-debugging-port` or `--remote-debugging-pipe`: the pipe and port 0 set `navigator.webdriver`, a TCP port is open to every local process, and the default-profile guard (second patch) blocks it on the real profile.
- A bundled extension plus a native messaging host: more moving parts, and `chrome.debugger` shows an infobar on every attach.
- Command logic in the CLI: rejected in favour of the agterm pattern, where the server holds all logic and the CLI stays a dumb client, so a new command never needs a matching client release for `--json` users.
- A session attached per command for every tab: an agent tab would stay hidden between commands, where `requestAnimationFrame` does not fire and the first input event takes about 5 s.

## Skills to invoke

Load each skill below and follow its conventions before implementing any task in this plan.

- `rust-style` - all code under `gidoractl/` must follow it. It loads automatically for `.rs` and `Cargo.toml`. If it did not, read `$HOME/.claude/skills/rust-style/SKILL.md`.
- `local-env` - toolchains are pinned in `mise.toml` and every repeatable command is a mise task.
- `writing-style` - commit messages.

## Context (from discovery)

- Repository: fork of `imputnet/helium-macos`, work branch `gidora`. The Chromium tree is `build/src` (157.0.8103.78) with patches applied by quilt. `build/` is git-ignored.
- Existing Gidora patches (order in `patches/series`): `gidora/settings/gidora-settings-page.patch`, `gidora/ui/vertical-pinned-tabs-per-row.patch`, `gidora/ui/copy-link-page-action.patch`, `gidora/core/gidora-identity.patch`.
- The Gidora settings page is `chrome/browser/resources/settings/gidora_page/gidora_page.html` and `gidora_page.ts` (a plain `PolymerElement`, literal strings, placeholder text). New UI strings stay literal: a new grd string changes a header included by thousands of files.
- `chrome/common/pref_names.h` is included very widely. A change to it costs a rebuild of about 17 minutes. New pref names go in a new header instead.
- `chrome/browser/extensions/api/settings_private/prefs_util.cc`: `PrefsUtil::FindServiceForPref` falls back to Local State, so a Local State pref can back a settings toggle once it is in the allowlist (see the `kHeliumVerticalPinnedTabsPerRow` entry).
- `chrome/browser/devtools/remote_debugging_server.cc`, `IsRemoteDebuggingAllowed`: the default-user-data-dir check is constant `true` only under `GOOGLE_CHROME_BRANDING`. Otherwise it reads `g_enable_default_user_data_dir_check_for_chromium_branding_for_testing`.
- In-process DevTools client pattern: `ExtensionDevToolsClientHost` in `chrome/browser/extensions/api/debugger/debugger_api.cc` (`DevToolsAgentHost::AttachClient`, `DispatchProtocolMessage`, JSON in and out). The attach itself shows no infobar.
- `net/socket/unix_domain_server_socket_posix.h`: `UnixDomainServerSocket` with an `AuthCallback` that receives the peer credentials.
- Tab ids: `extensions::ExtensionTabUtil::GetTabId(web_contents)`. Window iteration: `ForEachCurrentBrowserWindowInterfaceOrderedByActivation` in `chrome/browser/ui/browser_window/public/browser_window_interface_iterator.h`.
- Helium adds its own browser sources directly to the `sources` list of `static_library("browser")` in `chrome/browser/BUILD.gn` (see `helium_noise/`).
- Startup hooks: `ChromeBrowserMainParts::PostBrowserStart` and `PostMainMessageLoopRun` in `chrome/browser/chrome_browser_main.cc`. Local State prefs are registered in `RegisterLocalState` in `chrome/browser/prefs/browser_prefs.cc`.
- Measured on the dev build with a hidden background tab: text, scripts, images and a viewport screenshot work. `Input.dispatchMouseEvent` and `Input.insertText` work. Without focus emulation the first click took 5.2 s and `requestAnimationFrame` did not fire. With `Emulation.setFocusEmulationEnabled` the click was immediate and the page reported `visibilityState` `visible` and `hasFocus()` `true`.
- `CFFIXED_USER_HOME=<dir>` moves the Application Support directory of a Foundation process into `<dir>`. It gives a safe "default profile" for tests.
- Versions looked up on 2026-11-16: Rust 1.100.0, `clap` 4.6.7, `serde_json` 1.0.154, `base64` 0.23.1, `anyhow` 1.0.104.

## Development Approach

- **Testing approach**: regular (code first, then checks), deliberately light. This was decided before the plan and overrides the default "unit tests for every task" rule.
- The C++ patch has no unit tests. Its test is one end-to-end script, `gidoractl/e2e/e2e.py`, which grows with every server task. A server task is done when its new cases and all earlier cases pass.
- Rust unit tests exist only for the code with logic of its own: argument parsing into wire requests, and output rendering.
- One-off checks (settings toggle, debugging guard) are run once by hand with a throwaway script in the session scratchpad. They are not committed.
- Complete each task fully before the next one. Keep changes small.
- Update this plan when scope changes during implementation.
- Never start a browser on the real profile for a test. Every test launch passes `--user-data-dir=<temp dir>` and `--use-mock-keychain`, or sets `CFFIXED_USER_HOME=<temp dir>`.
- Never use `--remote-debugging-pipe`, `--remote-debugging-port=0`, `--headless` or `--enable-automation` in the e2e script: they mark the browser as automation controlled.
- New files in the patch must not contain domain names. The release tree gets the new patches without the domain substitution pass.

## Code-Quality Rules (verify before marking each task complete)

### Rust (`gidoractl/`)

The `rust-style` skill has no `## Hard rules` block. These are its rules, stated as a gate.

- Gate commands, all green: `cargo fmt --check`, `cargo clippy --all-targets --all-features --locked -- -D warnings`, `cargo test --locked`. They run through `mise run ctl-check`.
- Edition 2024. `Cargo.toml` carries the skill's `[lints.rust]` and `[lints.clippy]` block unchanged (`unsafe_code = "forbid"`, `all` deny, `pedantic` warn, `unwrap_used`, `wildcard_imports`, `needless_for_each`, `undocumented_unsafe_blocks` deny).
- No `#[allow]`. A lint that cannot be fixed in code stops the task and is asked about.
- `for` loops with mutable accumulators, not iterator chains.
- `let ... else` for early exits. `if let` only for a short branch with no else. One let chain instead of nested `if let`.
- Shadow a value through its transformations. No `raw_`, `parsed_` prefixes.
- Newtypes over bare strings and integers where two kinds could be mixed (tab id, socket path). Two-variant enums over `bool` parameters.
- Every `match` lists all variants. No wildcard arm, no `matches!`.
- Destructure structs explicitly, every field.
- The binary uses `anyhow` with `.context(...)` on every `?` that crosses a boundary. `unwrap()` only in tests.
- No narrating comments, no section dividers, no TODO comments.
- Grep checks: `grep -rn "unwrap()\|#\[allow\|matches!\|\.iter()\.map\|\.filter(" gidoractl/src` shows hits only inside `#[cfg(test)]` modules, and none for `#[allow` and `matches!` anywhere.

### C++ (the patches)

- Chromium style. `he format` (clang-format over `quilt diff`) leaves no change.
- No comments except a non-obvious constraint. New files carry the three-line Gidora license header used by `gidora_page.ts`.
- No edit to `chrome/common/pref_names.h`, `chrome/browser/browser_process_impl.h` or any other widely included header.
- All new C++ lives in `chrome/browser/gidora/control/`. Existing files get hook lines only.
- `mise run build` passes and `mise run e2e` passes.

## Testing Strategy

- **Rust unit tests**: `gidoractl/src/cli.rs` (arguments to wire request) and `gidoractl/src/output.rs` (reply to printed bytes).
- **End-to-end**: `mise run e2e` builds the CLI, starts the dev build on a temporary profile, serves a fixture page from a local HTTP server, and runs every command plus the failure cases listed in the tasks.
- **Acceptance**: the allegro scenario on the signed release build with the real profile (Task 9).

## Progress Tracking

- Mark completed items with `[x]` immediately when done.
- Add newly discovered tasks with a ➕ prefix.
- Document blockers with a ⚠️ prefix.
- Keep the plan in sync with the actual work.

## Solution Overview

The browser starts a `ControlServer` after startup when the Local State pref `gidora.control_socket.enabled` is true, and starts or stops it when the pref changes. The pref is false by default. A toggle on `chrome://settings/gidora` sets it.

The server listens on `<user data dir>/control.sock`. The transport follows agterm's control API: one connection carries one newline-terminated JSON request and one newline-terminated JSON response, then the server closes it. The socket code runs on the browser IO thread. Each request is handed to a dispatcher on the UI thread, which answers asynchronously.

Tab commands use the browser's own tab and navigation APIs. Page commands send DevTools protocol messages through an in-process `DevToolsAgentHostClient` attached to the tab, so no debugging port exists and no automation switch is set.

A tab opened by `tab.new` is an agent tab. It gets a DevTools session with focus emulation right after creation, and the session lives until the tab closes or the server stops. Any other tab gets a session for the duration of a single command, so a user's tab never stays in emulated focus.

`gidoractl` maps each subcommand to one wire command, prints only the payload on stdout, and prints errors on stderr with a non-zero exit code.

## Technical Details

### Socket

- Path: `<user data dir>/control.sock`, from `chrome::DIR_USER_DATA`. For the installed browser this is `$HOME/Library/Application Support/space.pkarpovich.gidora/control.sock`.
- If the path is longer than 103 bytes the server logs an error and does not start.
- On start: delete a stale file at the path, bind, then set mode 0600. One browser owns a user data directory at a time, so deleting is safe.
- The `AuthCallback` accepts a peer only when its uid equals the browser's uid.
- On stop (pref off or shutdown): close the listener, delete the file, drop all page sessions.

### Wire protocol

- Request: `{"cmd":"<name>","args":{...}}` followed by `\n`. `args` may be absent.
- Response: `{"ok":true,"result":{...}}` or `{"ok":false,"error":"<message>"}`, followed by `\n`.
- The request is capped at 1 MiB. Over the cap the reply is the error `request too large`. The response has no cap.
- The server waits 5 s for the full request line, then closes the connection without a reply.
- Malformed JSON, a missing `cmd`, or a wrong argument type gives `invalid request: <reason>`.
- An unknown command gives `unknown command: <name>`.
- There is no handshake. `version` is the preflight.
- A page command that does not finish in 30 s gives `timed out after 30 s`.

| cmd | args | result |
|---|---|---|
| `version` | - | `{"app":"Gidora","version":"<browser version>"}` |
| `tab.list` | - | `{"tabs":[{"id":int,"window":int,"active":bool,"loading":bool,"url":str,"title":str}]}` |
| `tab.new` | `url` | `{"id":int,"url":str,"loading":bool}` |
| `tab.go` | `tab?`, `url` | `{"id":int,"url":str,"loading":bool}` |
| `tab.close` | `tab?` | `{"id":int}` |
| `tab.text` | `tab?` | `{"text":str}` |
| `tab.html` | `tab?` | `{"html":str}` |
| `tab.eval` | `tab?`, `expression` | `{"value":<any JSON>}` |
| `tab.shot` | `tab?` | `{"id":int,"png":"<base64>"}` |
| `tab.click` | `tab?`, and `selector` or `x` with `y` | `{"x":number,"y":number}` |
| `tab.type` | `tab?`, `text` | `{}` |

### Tabs and targets

- A tab id is `ExtensionTabUtil::GetTabId`. It is stable while the browser runs.
- Only tabs of normal browser windows with a regular profile exist for the server. Incognito windows, app windows, popups and DevTools windows are skipped by `tab.list` and cannot be resolved by id.
- `tab` absent means the active tab of the most recently active eligible window. No eligible window gives `no browser window`. No active tab gives `no active tab`.
- An id that does not resolve gives `unknown tab: <id>`.
- `window` in `tab.list` is the session id of the window. `active` is true for the active tab of its window.
- `tab.new` and `tab.go` accept only valid `http` and `https` URLs. Anything else gives `unsupported URL: only http and https are allowed`.
- `tab.new` opens the tab in the most recently active eligible window with the `NEW_BACKGROUND_TAB` disposition, the way `chrome.tabs.create` with `active: false` does. It must not activate the tab, the window or the application.
- `tab.new` and `tab.go` reply when the tab stops loading (`WebContentsObserver::DidStopLoading`) or after 30 s, whichever comes first. `loading` in the result says which. `url` is the last committed URL.
- `tab.close` closes the tab without running a beforeunload prompt.

### Page sessions

- A page command first checks the last committed URL. A scheme other than `http` or `https` gives `tab <id> does not show a web page`.
- The session is a `content::DevToolsAgentHostClient`. `MayAttachToURL` returns true only for `http`, `https` and `about:blank`, so the session detaches itself if the tab navigates to an internal page.
- Right after attach the session sends `Emulation.setFocusEmulationEnabled` with `enabled: true`.
- Messages carry increasing integer ids. A reply is matched by id. Protocol events are ignored.
- `AgentHostClosed` or a detach fails all pending commands of that session with `tab closed` and removes the session.
- A DevTools protocol error reply gives the error `devtools: <message>`.

| cmd | DevTools messages |
|---|---|
| `tab.text` | `Runtime.evaluate` of `document.body ? document.body.innerText : ""`, `returnByValue` |
| `tab.html` | `Runtime.evaluate` of `document.documentElement.outerHTML`, `returnByValue` |
| `tab.eval` | `Runtime.evaluate` with `awaitPromise`, `returnByValue`, `userGesture`. `exceptionDetails` gives `script error: <exception description or text>`. An `undefined` result is `null` |
| `tab.shot` | `Page.captureScreenshot`, format `png` |
| `tab.click` with `selector` | `Runtime.evaluate` that finds the element, calls `scrollIntoView({block:"center",inline:"center"})` and returns the centre of `getBoundingClientRect()`. No element gives `selector not found: <selector>`. Then the three mouse messages below |
| `tab.click` with `x`,`y` | `Input.dispatchMouseEvent` three times: `mouseMoved`, then `mousePressed` and `mouseReleased` with `button: "left"`, `clickCount: 1` |
| `tab.type` | `Input.insertText` |

### CLI

- Global options: `--json` prints the server's response line unchanged. `--socket <path>` or the environment variable `GIDORA_SOCKET` overrides the socket path.
- `--tab <id>` on every `tab` subcommand except `list` and `new`.
- Exit codes: 0 on `ok: true`, 1 on `ok: false` or a transport failure, 2 on a usage error (clap default).
- A failed connect prints `gidoractl: Gidora control socket is not available at <path>. Turn on "Agent control socket" in Settings > Gidora.` on stderr.
- A broken stdout pipe ends the process quietly with exit code 0.
- No colour, no progress output, no prompts. The client read timeout is 60 s.

| command | stdout without `--json` |
|---|---|
| `gidoractl version` | `gidoractl <crate version>` and `Gidora <browser version>`, one per line |
| `gidoractl tab list` | one line per tab: `id`, `window`, `*` or `-` for active, `url`, `title`, separated by tabs, no header. Tabs and newlines inside a title become spaces |
| `gidoractl tab new <url>` | the tab id |
| `gidoractl tab go <url>` | the committed URL |
| `gidoractl tab close` | nothing |
| `gidoractl tab text` / `tab html` | the string, plus a newline when it does not end with one |
| `gidoractl tab eval <js>` | the value as one line of JSON. With `-r` a string value is printed raw. `-` as `<js>` reads the script from stdin |
| `gidoractl tab shot` | writes the PNG and prints the file path. Default path: `gidora-shot-<tab id>-<unix seconds>.png` in the system temp directory. `-o <path>` sets it. `-o -` writes the PNG bytes to stdout |
| `gidoractl tab click <selector>` / `tab click --at <x>,<y>` | nothing |
| `gidoractl tab type <text>` | nothing. `-` as `<text>` reads the text from stdin |

### Patch workflow

- All `he` and `quilt` commands run as `mise exec -- zsh -c 'source dev.sh && ...'` from the repository root, so the project Python and quilt variables are set.
- Inside `zsh -c` the quilt alias is not expanded. Call `quilt --quiltrc -` explicitly, from `build/src`. Without it patch headers come out with a `src/` prefix.
- New patch: `quilt --quiltrc - new gidora/core/<name>.patch`, then `quilt --quiltrc - add <file>` for every file before its first edit (new files included), then `quilt --quiltrc - refresh` after edits.
- The dev tree is in the merged state (`patches/series.merged`). Before a commit run `he unmerge`. Commit the patch file together with its line in `patches/series`. Then run `he merge`.
- New patches go at the end of `patches/series`, after `gidora/core/gidora-identity.patch`.
- `mise run build` is the dev build. A change to a few `.cc` files rebuilds in 20-30 s.

## What Goes Where

- **Implementation Steps**: the CLI, the two patches, the e2e script, mise tasks, the release build and the acceptance run.
- **Post-Completion**: things only the user can do.

## Implementation Steps

### Task 1: Scaffold the gidoractl crate and its argument parsing

**Files:**
- Create: `gidoractl/Cargo.toml`
- Create: `gidoractl/Cargo.lock`
- Create: `gidoractl/src/main.rs`
- Create: `gidoractl/src/cli.rs`
- Modify: `mise.toml`
- Modify: `.gitignore`

- [x] add `rust = "1.100.0"` to `[tools]` in `mise.toml`. Add the tasks `ctl-check` (the three gate commands, run in `gidoractl/`) and `ctl-install` (`cargo install --locked --path gidoractl --root "$HOME/.local"`, which puts the binary on PATH at `$HOME/.local/bin`)
- [x] add `gidoractl/target/` to `.gitignore`
- [x] create the binary crate `gidoractl`, edition 2024, with the lints block from the Code-Quality Rules and the dependencies `clap` 4.6.7 (features `derive`, `env`), `serde_json` 1.0.154, `base64` 0.23.1, `anyhow` 1.0.104. Before pinning, confirm each is still the current stable release
- [x] in `cli.rs` define the clap tree from the CLI section: global `--json` and `--socket` (env `GIDORA_SOCKET`), `version`, and `tab` with `list`, `new`, `go`, `close`, `text`, `html`, `eval`, `shot`, `click`, `type`
- [x] in `cli.rs` add the conversion from a parsed command to the wire request (`cmd` string plus `args` object) exactly as the wire table defines. `tab click` takes either a selector or `--at <x>,<y>`, never both and never neither. The `-` stdin forms are resolved through a reader passed in as a parameter so tests can supply the input
- [x] write unit tests: every subcommand produces its wire request; `--tab` is carried as an integer; `tab click --at 10,20` gives `x` and `y`; stdin `-` for `eval` and `type`
- [x] write unit tests for usage errors: `tab click` with both forms, with neither, a malformed `--at`, a non-numeric `--tab`, `tab new` without a URL
- [x] run `mise run ctl-check` - must pass before Task 2

### Task 2: Socket client and output rendering in gidoractl

**Files:**
- Create: `gidoractl/src/client.rs`
- Create: `gidoractl/src/output.rs`
- Modify: `gidoractl/src/main.rs`

- [x] in `client.rs` implement one round trip over `std::os::unix::net::UnixStream`: connect, write the request line, read to end of stream, 60 s read timeout. The default socket path is built from `$HOME` as in the Socket section. A failed connect returns the message from the CLI section
- [x] in `output.rs` implement rendering as pure functions from the parsed response and the command to bytes for stdout, following the stdout table. `--json` bypasses rendering and prints the response line unchanged
- [x] `tab shot`: decode the base64, write the file or stdout, print the path only when a file was written
- [x] in `main.rs` wire parsing, the round trip and rendering. `ok: false` prints `gidoractl: <error>` on stderr and exits 1. A `BrokenPipe` write error on stdout exits 0 without output
- [x] `version` prints the client line even when the server cannot be reached, then reports the connect failure on stderr and exits 1
- [x] write unit tests for `output.rs`: the `tab list` line format including a title with a tab and a newline; `eval` JSON output and `-r` on a string and on a non-string; text without and with a trailing newline; an error response
- [x] check by hand with no browser socket present: `gidoractl --socket <missing path> tab list` exits 1 with the hint, and `gidoractl tab list | head -0` does not print a panic
- [x] run `mise run ctl-check` - must pass before Task 3

### Task 3: Control server lifecycle, transport and the `version` command

**Files:**
- Create: `patches/gidora/core/control-socket.patch`
- Create (in `build/src`, inside the patch): `chrome/browser/gidora/control/control_prefs.h`, `control_prefs.cc`, `control_server.h`, `control_server.cc`, `control_dispatcher.h`, `control_dispatcher.cc`
- Modify (inside the patch): `chrome/browser/BUILD.gn`, `chrome/browser/prefs/browser_prefs.cc`, `chrome/browser/chrome_browser_main.cc`
- Modify: `patches/series`
- Create: `gidoractl/e2e/e2e.py`
- Create: `gidoractl/e2e/page.html`
- Modify: `mise.toml`

- [x] start the patch with quilt as the Patch workflow section describes
- [x] `control_prefs`: the pref name constant `gidora.control_socket.enabled` and `RegisterLocalStatePrefs`, default false. Call it from `RegisterLocalState` in `browser_prefs.cc`
- [x] `ControlServer`: a process-wide singleton created on first use, with no member added to any existing class. `ChromeBrowserMainParts::PostBrowserStart` calls its init, which observes the pref with a `PrefChangeRegistrar` on Local State and starts or stops the listener to match. `PostMainMessageLoopRun` calls its shutdown before the threads stop
- [x] the listener and the connections live on the browser IO thread (`content::GetIOThreadTaskRunner`). They implement the Socket and Wire protocol sections: stale file removal, bind, mode 0600, uid check, one request line per connection, the 1 MiB cap, the 5 s read timer, full write of the response, close
- [x] `ControlDispatcher` lives on the UI thread. It takes the request line and a reply callback, validates the envelope, and routes by `cmd` through one table of name to handler. Handlers are asynchronous: each receives the `args` dictionary and a callback for a result dictionary or an error string. Add the `version` handler
- [x] add the new sources to the `sources` list of `static_library("browser")` in `chrome/browser/BUILD.gn`, next to the `helium_noise` entries
- [x] create `e2e.py` (Python standard library only) with: a fixture HTTP server on `127.0.0.1` with a free port serving `page.html`; a browser launcher that makes a temp profile, seeds `Default/Preferences` with `helium.completed_onboarding: true` and, when asked, `Local State` with the pref set to true, and starts `build/src/out/Default/Gidora.app` with `--user-data-dir`, `--use-mock-keychain`, `--no-first-run`, `--no-default-browser-check` and the fixture URL; a helper that runs `gidoractl/target/release/gidoractl` with `GIDORA_SOCKET` set and returns exit code, stdout and stderr; a clean browser shutdown and temp profile removal at the end. A failed check prints the case name and makes the script exit non-zero
- [x] `page.html`: a title, a heading with known text, a button that changes the title when clicked, a text input, and a second button placed far below the first screen
- [x] add the mise task `e2e`: build the CLI in release mode, then run `e2e.py`
- [x] e2e cases: server enabled gives a socket file with mode 0600; `gidoractl version` prints both lines; a raw request with an unknown command returns `unknown command: nope`; malformed JSON returns `invalid request`; a request over 1 MiB returns `request too large`; a connection that sends nothing is closed by the server after about 5 s
- [x] e2e case: a browser started without the pref has no socket file and `gidoractl tab list` exits 1 with the hint
- [x] run `mise run build` and `mise run e2e` - must pass before Task 4

### Task 4: Tab commands `tab.list`, `tab.new`, `tab.go`, `tab.close`

**Files:**
- Create (inside the patch): `chrome/browser/gidora/control/tab_commands.h`, `tab_commands.cc`
- Modify (inside the patch): `chrome/browser/gidora/control/control_dispatcher.cc`, `chrome/browser/BUILD.gn`
- Modify: `gidoractl/e2e/e2e.py`

- [x] implement one tab lookup used by every handler: iterate eligible windows in activation order as the Tabs and targets section defines, and resolve either an id or the active tab. It returns the error strings from that section
- [x] implement `tab.list` with the result shape from the wire table
- [x] implement the URL check and `tab.new`. Confirm in the running dev build that the new tab does not become active and the window does not come to the front
- [x] implement `tab.go` on the resolved tab through its `NavigationController`
- [x] implement the load wait shared by `tab.new` and `tab.go`: a `WebContentsObserver` that replies on `DidStopLoading`, on tab destruction (`tab closed`), or after 30 s
- [x] implement `tab.close`
- [x] e2e cases: `tab list` shows the fixture tab as active; `tab new` prints an id, the new tab is in `tab list` with the fixture URL, and the active tab is still the first one; `tab go --tab <id>` prints the committed URL; `tab close --tab <id>` removes it from the list
- [x] e2e failure cases: `--tab 999999` gives `unknown tab: 999999`; `tab new` with a `chrome://` URL and with a `file://` URL gives the unsupported URL error
- [x] e2e case: a browser started with `--incognito` and the pref on gives an empty `tab list` and `no browser window` for `tab text`
- [x] run `mise run build` and `mise run e2e` - must pass before Task 5

### Task 5: Page sessions and `tab.text`, `tab.html`, `tab.eval`, `tab.shot`

**Files:**
- Create (inside the patch): `chrome/browser/gidora/control/page_session.h`, `page_session.cc`, `page_commands.h`, `page_commands.cc`
- Modify (inside the patch): `chrome/browser/gidora/control/control_dispatcher.cc`, `tab_commands.cc`, `chrome/browser/BUILD.gn`
- Modify: `gidoractl/e2e/e2e.py`

- [x] `PageSession`: the `DevToolsAgentHostClient` from the Page sessions section. Its interface is one call: send a method name with a params dictionary and get back the result dictionary or an error string. It owns the id counter, the pending map and the detach handling
- [x] a session registry on the UI thread, keyed by tab id. `tab.new` creates a persistent session for the new tab right after the tab exists, before the load finishes. A page command on a tab without a session creates a temporary one and destroys it when the command has replied. Server stop destroys all sessions
- [x] the 30 s command timeout, applied to the whole page command
- [x] implement `tab.text`, `tab.html`, `tab.eval` and `tab.shot` from the DevTools message table, including the web-page check and the `script error` mapping
- [x] e2e cases on an agent tab opened with `tab new`: `tab text` contains the heading text; `tab html` contains the heading tag; `tab eval "1+1"` prints `2`; `tab eval -r "document.title"` prints the raw title; `echo "document.title" | gidoractl tab eval -` works; an expression that returns a promise is awaited; `tab shot -o <file>` writes a file that starts with the PNG signature; `tab shot -o - | wc -c` is non-zero
- [x] e2e case for the reason behind persistent sessions: in an agent tab that was never active, an expression that awaits `requestAnimationFrame` with a 2 s fallback reports that the frame fired, and `document.visibilityState` is `visible`
- [x] e2e failure cases: `tab eval "throw new Error('boom')"` exits 1 with `script error` and `boom`; a page command on a tab closed in the meantime gives `unknown tab`
- [x] e2e case: commands without `--tab` act on the user's active tab, and after the whole run the active tab id is the one from the start
- [x] run `mise run build` and `mise run e2e` - must pass before Task 6

### Task 6: `tab.click` and `tab.type`

**Files:**
- Modify (inside the patch): `chrome/browser/gidora/control/page_commands.cc`, `control_dispatcher.cc`
- Modify: `gidoractl/e2e/e2e.py`

- [ ] implement `tab.click` in both forms from the DevTools message table. The result carries the coordinates that were clicked
- [ ] implement `tab.type`
- [ ] e2e cases on an agent tab: clicking the first button by selector changes the title; clicking the below-the-fold button by selector works (scroll into view); `tab click --at` with the coordinates returned for the first button works; clicking the input and `tab type hello` gives the input the value `hello`; `echo -n world | gidoractl tab type -` appends
- [ ] e2e failure case: `tab click "#missing"` exits 1 with `selector not found: #missing`
- [ ] run `mise run build` and `mise run e2e` - must pass before Task 7

### Task 7: Settings toggle, then commit the control socket

**Files:**
- Modify (inside the patch): `chrome/browser/resources/settings/gidora_page/gidora_page.html`, `gidora_page.ts`, `chrome/browser/resources/settings/settings_main/settings_main.html`, `chrome/browser/extensions/api/settings_private/prefs_util.cc`
- Modify: `patches/series`

- [ ] add `gidora.control_socket.enabled` to the settings allowlist in `prefs_util.cc` as a boolean, using the constant from `control_prefs.h`
- [ ] make `SettingsGidoraPageElement` use `PrefsMixin`, pass `prefs` to it from `settings_main.html`, and replace the placeholder row with a `settings-toggle-button` bound to the pref. Label: `Agent control socket`. Sub-label: `Lets local command-line tools control this browser through gidoractl.` Both strings are literals in the HTML
- [ ] one-off check with a throwaway script (not committed): on a temp profile started with `--remote-debugging-pipe`, open `chrome://settings/gidora`, click the toggle through the page, and confirm that the socket file appears, that `gidoractl version` answers, that a second click removes the socket file, and that `Local State` holds the pref
- [ ] save a screenshot of the page to `build/gidora-settings-control-toggle.png`
- [ ] run `he format`, `mise run build`, `mise run e2e` and `mise run ctl-check`
- [ ] commit in two commits on `gidora`: `gidoractl: add the control CLI` (the crate, `mise.toml`, `.gitignore`) and, after `he unmerge`, `gidora/core: add a control socket for gidoractl` (the patch, `patches/series`, `gidoractl/e2e/`). Run `he merge` afterwards

### Task 8: Restrict remote debugging on the default profile

**Files:**
- Create: `patches/gidora/core/remote-debugging-guard.patch`
- Modify (inside the patch): `chrome/browser/devtools/remote_debugging_server.cc`
- Modify: `patches/series`

- [ ] in `IsRemoteDebuggingAllowed` make the default-user-data-dir check enabled in this build without the Google branding flag. Keep the testing override variable working, so the change is a single condition
- [ ] one-off check, default profile refused: start the dev build with `CFFIXED_USER_HOME=<temp dir>`, `--use-mock-keychain` and `--remote-debugging-port=<free port>`. First confirm that the profile directory was created inside the temp dir. Then confirm that nothing listens on the port
- [ ] one-off check, custom profile allowed: start with `--user-data-dir=<temp dir>` and the same port switch, and confirm that `/json/version` answers on the port
- [ ] run `mise run build` and `mise run e2e`
- [ ] commit after `he unmerge`: `gidora/core: restrict remote debugging on the default profile`. Run `he merge` afterwards

### Task 9: Release build, install and acceptance

**Files:**
- Modify: `mise.toml`

- [ ] add the mise task `release-update`: source `devutils/shared.sh` and run `helium_build chrome/installer/mac` with `GOROOT`, `GOBIN` and `GOTOOLDIR` unset, without `prepare_sources` and without `configure_build`. `mise run release` must not be used for an update: it deletes `out/` and rebuilds from scratch (about 4h45m)
- [ ] add the mise task `sign`: from `build/src`, create a no-op `spctl` script in a directory under `build/`, put that directory first on `PATH`, and run `out/Default/Gidora Packaging/sign_chrome.py --input out/Default --output out/Default/signed --identity "<the Developer ID Application identity found with security find-identity>" --disable-packaging --notarize none`. The shim is needed because `spctl --assess` fails for an app that is not notarized. Then run `codesign --verify --deep --strict` on the signed app
- [ ] commit: `build: add release update and sign tasks`. Push `gidora`
- [ ] in the release worktree (`$HOME/Projects/worktrees/gidora/release`, branch `release`): `git merge --ff-only gidora`, then apply the two new patches to its `build/src` with `patch -p1 --forward`. That tree was prepared without quilt, so quilt is not used there
- [ ] run `mise run release-update` there and record the duration in this plan. The duration of an incremental official build is not measured yet
- [ ] run `mise run sign` there and confirm the signature and `Gidora --version`
- [ ] ⚠️ stop and ask the user before this step: replacing the installed app means quitting his browser. With his go-ahead, quit Gidora gracefully, move `/Applications/Gidora.app` to `build/Gidora.prev.app` in the release worktree, copy the signed app to `/Applications/Gidora.app` with `ditto`, and start it
- [ ] run `mise run ctl-install` and confirm that `gidoractl version` answers from a new shell once the toggle is on (see Post-Completion)
- [ ] acceptance on the real profile: note the active tab from `gidoractl tab list`; `tab new` with an allegro.pl search URL; `tab text`; `tab eval` with a script written for this run that returns the offers as JSON; `tab new` with one product URL; `tab text` and `tab shot` on it; `tab close` on both tabs; confirm that the active tab is the noted one. If a human check page appears, tell the user which tab to open, wait, and continue
- [ ] record the acceptance result in this plan

### Task 10: Verify acceptance criteria

- [ ] every command of the CLI table works on the release build
- [ ] every failure case of Tasks 3-6 is covered by `mise run e2e`, and it passes on the final dev tree
- [ ] `mise run ctl-check` passes
- [ ] the server is off on a fresh profile
- [ ] `navigator.webdriver` is `false` in an agent tab (`gidoractl tab eval "navigator.webdriver"`)
- [ ] the Non-goals list still holds: nothing from it was added

### Task 11: [Final] Update documentation

**Files:**
- Create: `gidoractl/README.md`
- Modify: `docs/plans/20261116-gidoractl.md`

- [ ] write `gidoractl/README.md`: what the tool is, how to turn the socket on, the command table with one example each, the pipe rules, the exit codes, the `--json` envelope, `GIDORA_SOCKET`, and how to run `mise run e2e`. The upstream `README.md` stays untouched to keep merges from upstream clean
- [ ] move this plan to `docs/plans/completed/`
- [ ] commit: `docs: describe gidoractl`. Push `gidora`

## Post-Completion

**The user at the MacBook Pro:**
- Turn on "Agent control socket" in Settings > Gidora on the real profile. The server stays off until then, so Task 9 waits for it.
- Solve a human check in the agent's tab if a site shows one.

**Later, not in this plan:**
- An agent skill that documents `gidoractl` for Claude Code.
- `tasks.start` in `mise.toml` still starts the old app name through `dev.sh`.
