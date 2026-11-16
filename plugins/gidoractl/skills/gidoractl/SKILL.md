---
name: gidoractl
description: Drive the user's own running Gidora browser with the gidoractl CLI - open a background tab in the real signed-in profile, navigate, read text or HTML, run a script in the page, take a screenshot, click and type. Use when a task needs the user's logged-in session or their real tabs - find or compare products in a shop they have an account in, read a page behind a login, check an order or a dashboard, look at what a tab shows, or when the user says "in my browser", "in Gidora" or names gidoractl. Not for testing a web app under development or any job a fresh headless browser can do (agent-browser), and not for reading a public article (defuddle).
---

# gidoractl

`gidoractl` sends one command to the user's running Gidora browser and prints the answer. The pages load in the user's real profile, with their cookies and logins. The tool returns raw text, HTML, script values and images, and the parsing is yours.

## Before the first command

Run `gidoractl version`. When it reports that the control socket is not available, the switch is off: ask the user to turn on "Agent control socket" at `chrome://settings/gidora`, and stop until they do. That switch is the user's decision, never change it for them.

## Work in your own tab

```sh
tab=$(gidoractl tab new "https://example.com/")   # background tab, prints its id
gidoractl tab text --tab "$tab"
gidoractl tab close --tab "$tab"
```

- `tab new` never activates the tab, the window or the application. The user keeps working.
- Keep the id it prints and pass `--tab <id>` to every later command. Shell variables do not survive between tool calls, so carry the id yourself.
- A command without `--tab` acts on the tab the user is looking at. That is right for "what is on my screen", and wrong for `go`, `close`, `click` and `type`, which would navigate, close or change the user's own tab.
- Reuse one tab for a series of pages with `tab go`. Close every tab you opened when the task ends.
- The tab is an ordinary tab, and the user can close it. `unknown tab: <id>` means it is gone: open a new one and continue.

## Commands

| Command | Result |
|---|---|
| `tab list` | one line per tab: id, window id, `*` for the active tab or `-`, URL, title, tab-separated |
| `tab new <url>` | the id of the new background tab, after the page loaded |
| `tab go --tab <id> <url>` | the URL the tab ended on, after the page loaded |
| `tab close --tab <id>` | nothing |
| `tab text --tab <id>` | the visible text |
| `tab html --tab <id>` | the HTML |
| `tab eval --tab <id> <js>` | the value as one line of JSON; `-r` prints a string raw; `-` reads the script from stdin |
| `tab shot --tab <id> -o <path>` | writes a PNG of the viewport and prints the path |
| `tab click --tab <id> <selector>` | scrolls the first match into view and clicks its centre |
| `tab click --tab <id> --at <x>,<y>` | clicks a viewport point |
| `tab type --tab <id> <text>` | inserts text into the focused element; `-` reads it from stdin |

Errors go to stderr as `gidoractl: <message>` with exit code 1. `--json` prints the browser's response line, `{"ok":true,"result":{...}}` or `{"ok":false,"error":"..."}`.

## Reading a page

- Use `tab text` to see what a page is. It carries menus, banners and recommendation blocks, so numbers taken from it by pattern are often the wrong ones.
- Take data with `tab eval`. Write the script to a file, return one JSON value, and pass it with `gidoractl tab eval --tab "$tab" - < script.js`. A script that works on one page of a site works on the next one through `tab go`.
- Prefer structured sources inside the page: `script[type="application/ld+json"]`, `meta` tags, `data-` attributes. They hold the real price, availability and names.
- Inspect before you guess a selector: one `tab eval` that lists the candidate elements with their attributes costs less than several wrong clicks.
- To look at a page, `tab shot -o <path>` and read the image. It shows the viewport only. Scroll with `tab eval "scrollTo(0, 1200)"` first when needed.

## Waiting

`tab new` and `tab go` answer when the page stops loading, or after 30 s. `tab click` answers right after the click and does not wait for what the click starts.

After a click that navigates or loads content, wait inside the page. `tab eval` awaits a promise:

```js
new Promise(resolve => {
  const started = Date.now();
  const check = () => {
    if (document.querySelectorAll("article h2").length > 0) resolve(location.href);
    else if (Date.now() - started > 20000) resolve("timeout at " + location.href);
    else setTimeout(check, 200);
  };
  check();
})
```

Keep the wait under 30 s: every page command fails with `timed out after 30 s` at that point.

## Forms

- Click the field first, then `tab type`. The text goes to the focused element.
- `tab type` inserts text and presses no keys. Submit with `tab click` on the submit button, or with `tab eval "document.querySelector('form').requestSubmit()"`.
- To replace a value, select it first: `tab eval "document.querySelector('<selector>').select()"`, then `tab type`.

## The user's account is real

Every click acts as the user. Reading, searching and navigating are fine. Do not buy, add to a cart, submit an order or a form, send a message, or change account settings unless the user asked for exactly that action. When a task ends at such a step, stop before it and report what is ready.

When a site shows a human check, tell the user which tab holds it, wait until they solve it, then continue in the same tab.

## Limits

- Only `http` and `https` pages. A browser page such as `chrome://settings` answers `tab <id> does not show a web page`.
- Incognito windows do not exist for the socket.
- No key presses, no file upload, no downloads, no cookie values.
- `tab closed` means the tab went away while a command was running.
