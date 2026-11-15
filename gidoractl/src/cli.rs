use std::io::Read;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, bail};
use clap::{Args, Parser, Subcommand};
use serde_json::{Map, Value, json};

const STDIN_MARKER: &str = "-";

#[derive(Debug, Parser)]
#[command(
    name = "gidoractl",
    about = "Control the Gidora browser from the command line"
)]
pub struct Cli {
    /// Print the response line of the browser unchanged
    #[arg(long, global = true)]
    pub json: bool,

    /// Path of the control socket
    #[arg(long, global = true, env = "GIDORA_SOCKET", value_name = "PATH")]
    pub socket: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print the client and the browser version
    Version,
    /// Work with browser tabs
    #[command(subcommand)]
    Tab(TabCommand),
}

#[derive(Debug, Subcommand)]
pub enum TabCommand {
    /// List the tabs of normal windows, one per line
    List,
    /// Open a URL in a new background tab and print the tab id
    New {
        /// An http or https URL
        url: String,
    },
    /// Navigate a tab and print the committed URL
    Go {
        #[command(flatten)]
        target: Target,
        /// An http or https URL
        url: String,
    },
    /// Close a tab
    Close {
        #[command(flatten)]
        target: Target,
    },
    /// Print the visible text of the page
    Text {
        #[command(flatten)]
        target: Target,
    },
    /// Print the HTML of the page
    Html {
        #[command(flatten)]
        target: Target,
    },
    /// Run a script in the page and print its value as JSON
    Eval {
        #[command(flatten)]
        target: Target,
        /// Print a string value without JSON quoting
        #[arg(short, long)]
        raw: bool,
        /// The script, or `-` to read it from stdin
        script: String,
    },
    /// Save a PNG screenshot of the viewport and print the file path
    Shot {
        #[command(flatten)]
        target: Target,
        /// Where to write the PNG, or `-` for stdout
        #[arg(short, long, value_name = "PATH")]
        output: Option<ShotOutput>,
    },
    /// Click an element by CSS selector, or a point with --at
    Click {
        #[command(flatten)]
        target: Target,
        /// A CSS selector
        #[arg(required_unless_present = "at", conflicts_with = "at")]
        selector: Option<String>,
        /// Viewport coordinates
        #[arg(long, value_name = "X,Y")]
        at: Option<Point>,
    },
    /// Insert text into the focused element
    Type {
        #[command(flatten)]
        target: Target,
        /// The text, or `-` to read it from stdin
        text: String,
    },
}

#[derive(Debug, Args)]
pub struct Target {
    /// The tab id from `tab list`; the active tab when omitted
    #[arg(long, value_name = "ID")]
    pub tab: Option<TabId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabId(pub i64);

impl FromStr for TabId {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<TabId, anyhow::Error> {
        let Ok(id) = text.parse::<i64>() else {
            bail!("`{text}` is not a tab id");
        };
        Ok(TabId(id))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl FromStr for Point {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<Point, anyhow::Error> {
        let Some((x, y)) = text.split_once(',') else {
            bail!("`{text}` is not a point, expected X,Y");
        };
        let Ok(x) = x.trim().parse::<f64>() else {
            bail!("`{text}` is not a point, X is not a number");
        };
        let Ok(y) = y.trim().parse::<f64>() else {
            bail!("`{text}` is not a point, Y is not a number");
        };
        Ok(Point { x, y })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShotOutput {
    Stdout,
    File(PathBuf),
}

impl FromStr for ShotOutput {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<ShotOutput, anyhow::Error> {
        if text == STDIN_MARKER {
            return Ok(ShotOutput::Stdout);
        }
        Ok(ShotOutput::File(PathBuf::from(text)))
    }
}

#[derive(Debug, PartialEq)]
pub struct Request {
    pub cmd: &'static str,
    pub args: Map<String, Value>,
}

impl Request {
    pub fn line(&self) -> String {
        let Request { cmd, args } = self;
        let mut line = json!({ "cmd": cmd, "args": args }).to_string();
        line.push('\n');
        line
    }
}

pub fn request(command: &Command, stdin: &mut dyn Read) -> anyhow::Result<Request> {
    match command {
        Command::Version => Ok(Request {
            cmd: "version",
            args: Map::new(),
        }),
        Command::Tab(tab) => tab_request(tab, stdin),
    }
}

fn tab_request(command: &TabCommand, stdin: &mut dyn Read) -> anyhow::Result<Request> {
    match command {
        TabCommand::List => Ok(Request {
            cmd: "tab.list",
            args: Map::new(),
        }),
        TabCommand::New { url } => {
            let mut args = Map::new();
            args.insert("url".to_string(), Value::String(url.clone()));
            Ok(Request {
                cmd: "tab.new",
                args,
            })
        }
        TabCommand::Go { target, url } => {
            let mut args = target_args(target);
            args.insert("url".to_string(), Value::String(url.clone()));
            Ok(Request {
                cmd: "tab.go",
                args,
            })
        }
        TabCommand::Close { target } => Ok(Request {
            cmd: "tab.close",
            args: target_args(target),
        }),
        TabCommand::Text { target } => Ok(Request {
            cmd: "tab.text",
            args: target_args(target),
        }),
        TabCommand::Html { target } => Ok(Request {
            cmd: "tab.html",
            args: target_args(target),
        }),
        TabCommand::Eval {
            target,
            raw: _,
            script,
        } => {
            let mut args = target_args(target);
            let script = text_or_stdin(script, stdin).context("reading the script from stdin")?;
            args.insert("expression".to_string(), Value::String(script));
            Ok(Request {
                cmd: "tab.eval",
                args,
            })
        }
        TabCommand::Shot { target, output: _ } => Ok(Request {
            cmd: "tab.shot",
            args: target_args(target),
        }),
        TabCommand::Click {
            target,
            selector,
            at,
        } => {
            let mut args = target_args(target);
            match (selector, at) {
                (Some(selector), None) => {
                    args.insert("selector".to_string(), Value::String(selector.clone()));
                }
                (None, Some(Point { x, y })) => {
                    args.insert("x".to_string(), json!(x));
                    args.insert("y".to_string(), json!(y));
                }
                (Some(_), Some(_)) | (None, None) => {
                    bail!("give either a selector or --at");
                }
            }
            Ok(Request {
                cmd: "tab.click",
                args,
            })
        }
        TabCommand::Type { target, text } => {
            let mut args = target_args(target);
            let text = text_or_stdin(text, stdin).context("reading the text from stdin")?;
            args.insert("text".to_string(), Value::String(text));
            Ok(Request {
                cmd: "tab.type",
                args,
            })
        }
    }
}

fn target_args(target: &Target) -> Map<String, Value> {
    let Target { tab } = target;
    let mut args = Map::new();
    if let Some(TabId(id)) = tab {
        args.insert("tab".to_string(), json!(id));
    }
    args
}

fn text_or_stdin(value: &str, stdin: &mut dyn Read) -> anyhow::Result<String> {
    if value != STDIN_MARKER {
        return Ok(value.to_string());
    }
    let mut text = String::new();
    stdin.read_to_string(&mut text)?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    fn wire_with_stdin(args: &[&str], stdin: &str) -> Request {
        let mut line = vec!["gidoractl"];
        line.extend_from_slice(args);
        let Cli { command, .. } = Cli::try_parse_from(line).expect("the arguments parse");
        request(&command, &mut stdin.as_bytes()).expect("the request builds")
    }

    fn wire(args: &[&str]) -> Request {
        wire_with_stdin(args, "")
    }

    fn expected(cmd: &'static str, args: Value) -> Request {
        let Value::Object(args) = args else {
            panic!("the expected args must be an object");
        };
        Request { cmd, args }
    }

    fn usage_error(args: &[&str]) -> ErrorKind {
        let mut line = vec!["gidoractl"];
        line.extend_from_slice(args);
        match Cli::try_parse_from(line) {
            Ok(cli) => panic!("expected a usage error, got {cli:?}"),
            Err(error) => error.kind(),
        }
    }

    #[test]
    fn every_subcommand_maps_to_its_wire_command() {
        let cases = [
            (vec!["version"], expected("version", json!({}))),
            (vec!["tab", "list"], expected("tab.list", json!({}))),
            (
                vec!["tab", "new", "https://example.com/"],
                expected("tab.new", json!({"url": "https://example.com/"})),
            ),
            (
                vec!["tab", "go", "https://example.com/a"],
                expected("tab.go", json!({"url": "https://example.com/a"})),
            ),
            (vec!["tab", "close"], expected("tab.close", json!({}))),
            (vec!["tab", "text"], expected("tab.text", json!({}))),
            (vec!["tab", "html"], expected("tab.html", json!({}))),
            (
                vec!["tab", "eval", "1+1"],
                expected("tab.eval", json!({"expression": "1+1"})),
            ),
            (vec!["tab", "shot"], expected("tab.shot", json!({}))),
            (
                vec!["tab", "click", "#go"],
                expected("tab.click", json!({"selector": "#go"})),
            ),
            (
                vec!["tab", "type", "hello"],
                expected("tab.type", json!({"text": "hello"})),
            ),
        ];
        for (args, request) in cases {
            assert_eq!(wire(&args), request, "for {args:?}");
        }
    }

    #[test]
    fn the_tab_option_travels_as_an_integer() {
        let cases = [
            vec!["tab", "go", "--tab", "42", "https://example.com/"],
            vec!["tab", "close", "--tab", "42"],
            vec!["tab", "text", "--tab", "42"],
            vec!["tab", "html", "--tab", "42"],
            vec!["tab", "eval", "--tab", "42", "1"],
            vec!["tab", "shot", "--tab", "42"],
            vec!["tab", "click", "--tab", "42", "a"],
            vec!["tab", "type", "--tab", "42", "x"],
        ];
        for args in cases {
            let Request { args: sent, .. } = wire(&args);
            assert_eq!(sent.get("tab"), Some(&json!(42)), "for {args:?}");
        }
    }

    #[test]
    fn rendering_options_stay_out_of_the_request() {
        assert_eq!(
            wire(&["tab", "eval", "-r", "document.title"]),
            expected("tab.eval", json!({"expression": "document.title"}))
        );
        assert_eq!(
            wire(&["tab", "shot", "-o", "-"]),
            expected("tab.shot", json!({}))
        );
        assert_eq!(
            wire(&["--json", "tab", "list"]),
            expected("tab.list", json!({}))
        );
    }

    #[test]
    fn a_point_click_sends_coordinates_instead_of_a_selector() {
        assert_eq!(
            wire(&["tab", "click", "--at", "10,20.5"]),
            expected("tab.click", json!({"x": 10.0, "y": 20.5}))
        );
    }

    #[test]
    fn a_dash_reads_the_script_or_the_text_from_stdin() {
        assert_eq!(
            wire_with_stdin(&["tab", "eval", "-"], "document.title\n"),
            expected("tab.eval", json!({"expression": "document.title\n"}))
        );
        assert_eq!(
            wire_with_stdin(&["tab", "type", "-"], "typed text"),
            expected("tab.type", json!({"text": "typed text"}))
        );
    }

    #[test]
    fn the_request_line_is_one_json_object_and_a_newline() {
        let line = wire(&["tab", "go", "--tab", "7", "https://example.com/"]).line();
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1);
        let parsed: Value = serde_json::from_str(&line).expect("the line is json");
        assert_eq!(
            parsed,
            json!({"cmd": "tab.go", "args": {"tab": 7, "url": "https://example.com/"}})
        );
    }

    #[test]
    fn the_shot_output_distinguishes_stdout_from_a_file() {
        let Cli { command, .. } =
            Cli::try_parse_from(["gidoractl", "tab", "shot", "-o", "-"]).expect("parses");
        let Command::Tab(TabCommand::Shot { output, .. }) = command else {
            panic!("expected the shot command");
        };
        assert_eq!(output, Some(ShotOutput::Stdout));

        let Cli { command, .. } =
            Cli::try_parse_from(["gidoractl", "tab", "shot", "-o", "page.png"]).expect("parses");
        let Command::Tab(TabCommand::Shot { output, .. }) = command else {
            panic!("expected the shot command");
        };
        assert_eq!(output, Some(ShotOutput::File(PathBuf::from("page.png"))));
    }

    #[test]
    fn a_click_needs_exactly_one_of_selector_and_point() {
        assert_eq!(
            usage_error(&["tab", "click"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            usage_error(&["tab", "click", "#go", "--at", "1,2"]),
            ErrorKind::ArgumentConflict
        );
    }

    #[test]
    fn malformed_values_are_usage_errors() {
        let cases = [
            vec!["tab", "click", "--at", "10"],
            vec!["tab", "click", "--at", "ten,20"],
            vec!["tab", "click", "--at", "10,twenty"],
            vec!["tab", "text", "--tab", "first"],
        ];
        for args in cases {
            assert_eq!(
                usage_error(&args),
                ErrorKind::ValueValidation,
                "for {args:?}"
            );
        }
    }

    #[test]
    fn a_missing_url_is_a_usage_error() {
        assert_eq!(
            usage_error(&["tab", "new"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            usage_error(&["tab", "go"]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn list_and_new_take_no_tab_option() {
        assert_eq!(
            usage_error(&["tab", "list", "--tab", "1"]),
            ErrorKind::UnknownArgument
        );
        assert_eq!(
            usage_error(&["tab", "new", "--tab", "1", "https://example.com/"]),
            ErrorKind::UnknownArgument
        );
    }
}
