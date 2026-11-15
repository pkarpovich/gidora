use std::path::PathBuf;

use anyhow::{Context, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::Value;

use crate::cli::{Command, ShotOutput, TabCommand};

#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Done(Value),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    pub temp_dir: PathBuf,
    pub unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Output {
    pub stdout: Vec<u8>,
    pub file: Option<FileOutput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOutput {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

pub fn parse_reply(line: &str) -> anyhow::Result<Reply> {
    let reply: Value = serde_json::from_str(line).context("the response is not JSON")?;
    let Some(ok) = reply.get("ok") else {
        bail!("the response has no `ok` field");
    };
    let Some(ok) = ok.as_bool() else {
        bail!("the `ok` field of the response is not a boolean");
    };
    if ok {
        let Some(result) = reply.get("result") else {
            bail!("the response has no `result` field");
        };
        return Ok(Reply::Done(result.clone()));
    }
    let Some(error) = reply.get("error") else {
        bail!("the failed response has no `error` field");
    };
    let Some(error) = error.as_str() else {
        bail!("the `error` field of the response is not a string");
    };
    Ok(Reply::Failed(error.to_string()))
}

pub fn render(
    command: &Command,
    result: &Value,
    environment: &Environment,
) -> anyhow::Result<Output> {
    match command {
        Command::Version => {
            let app = text_field(result, "app")?;
            let version = text_field(result, "version")?;
            Ok(stdout(format!("{app} {version}\n")))
        }
        Command::Tab(tab) => render_tab(tab, result, environment),
    }
}

fn render_tab(
    command: &TabCommand,
    result: &Value,
    environment: &Environment,
) -> anyhow::Result<Output> {
    match command {
        TabCommand::List => tab_list(result),
        TabCommand::New { url: _ } => {
            let id = integer_field(result, "id")?;
            Ok(stdout(format!("{id}\n")))
        }
        TabCommand::Go { target: _, url: _ } => {
            let url = text_field(result, "url")?;
            Ok(stdout(format!("{url}\n")))
        }
        TabCommand::Close { target: _ }
        | TabCommand::Click {
            target: _,
            selector: _,
            at: _,
        }
        | TabCommand::Type { target: _, text: _ } => Ok(Output::default()),
        TabCommand::Text { target: _ } => Ok(stdout(line(text_field(result, "text")?))),
        TabCommand::Html { target: _ } => Ok(stdout(line(text_field(result, "html")?))),
        TabCommand::Eval {
            target: _,
            raw,
            script: _,
        } => {
            let Some(value) = result.get("value") else {
                bail!("the response has no `value` field");
            };
            if *raw && let Some(text) = value.as_str() {
                return Ok(stdout(line(text)));
            }
            Ok(stdout(format!("{value}\n")))
        }
        TabCommand::Shot { target: _, output } => shot(result, output.as_ref(), environment),
    }
}

fn tab_list(result: &Value) -> anyhow::Result<Output> {
    let Some(tabs) = result.get("tabs") else {
        bail!("the response has no `tabs` field");
    };
    let Some(tabs) = tabs.as_array() else {
        bail!("the `tabs` field of the response is not an array");
    };
    let mut lines = String::new();
    for tab in tabs {
        let id = integer_field(tab, "id")?;
        let window = integer_field(tab, "window")?;
        let active = if boolean_field(tab, "active")? {
            '*'
        } else {
            '-'
        };
        let url = single_line(text_field(tab, "url")?);
        let title = single_line(text_field(tab, "title")?);
        let line = format!("{id}\t{window}\t{active}\t{url}\t{title}\n");
        lines.push_str(&line);
    }
    Ok(stdout(lines))
}

fn shot(
    result: &Value,
    output: Option<&ShotOutput>,
    environment: &Environment,
) -> anyhow::Result<Output> {
    let png = text_field(result, "png")?;
    let bytes = STANDARD
        .decode(png)
        .context("the screenshot is not valid base64")?;
    let path = match output {
        Some(ShotOutput::Stdout) => {
            return Ok(Output {
                stdout: bytes,
                file: None,
            });
        }
        Some(ShotOutput::File(path)) => path.clone(),
        None => {
            let id = integer_field(result, "id")?;
            let Environment {
                temp_dir,
                unix_seconds,
            } = environment;
            temp_dir.join(format!("gidora-shot-{id}-{unix_seconds}.png"))
        }
    };
    Ok(Output {
        stdout: format!("{}\n", path.display()).into_bytes(),
        file: Some(FileOutput { path, bytes }),
    })
}

fn stdout(text: String) -> Output {
    Output {
        stdout: text.into_bytes(),
        file: None,
    }
}

fn line(text: &str) -> String {
    if text.ends_with('\n') {
        return text.to_string();
    }
    format!("{text}\n")
}

fn single_line(text: &str) -> String {
    let mut flat = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\t' | '\n' | '\r' => flat.push(' '),
            other => flat.push(other),
        }
    }
    flat
}

fn text_field<'a>(value: &'a Value, name: &str) -> anyhow::Result<&'a str> {
    let Some(field) = value.get(name) else {
        bail!("the response has no `{name}` field");
    };
    let Some(text) = field.as_str() else {
        bail!("the `{name}` field of the response is not a string");
    };
    Ok(text)
}

fn integer_field(value: &Value, name: &str) -> anyhow::Result<i64> {
    let Some(field) = value.get(name) else {
        bail!("the response has no `{name}` field");
    };
    let Some(number) = field.as_i64() else {
        bail!("the `{name}` field of the response is not an integer");
    };
    Ok(number)
}

fn boolean_field(value: &Value, name: &str) -> anyhow::Result<bool> {
    let Some(field) = value.get(name) else {
        bail!("the response has no `{name}` field");
    };
    let Some(flag) = field.as_bool() else {
        bail!("the `{name}` field of the response is not a boolean");
    };
    Ok(flag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use clap::Parser;
    use serde_json::json;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake";

    fn environment() -> Environment {
        Environment {
            temp_dir: PathBuf::from("/var/tmp/shots"),
            unix_seconds: 1_794_000_000,
        }
    }

    fn rendered(args: &[&str], result: &Value) -> Output {
        let mut line = vec!["gidoractl"];
        line.extend_from_slice(args);
        let Cli { command, .. } = Cli::try_parse_from(line).expect("the arguments parse");
        render(&command, result, &environment()).expect("the result renders")
    }

    fn printed(args: &[&str], result: &Value) -> String {
        let Output { stdout, file } = rendered(args, result);
        assert_eq!(file, None);
        String::from_utf8(stdout).expect("the output is text")
    }

    #[test]
    fn a_successful_and_a_failed_response_parse() {
        assert_eq!(
            parse_reply("{\"ok\":true,\"result\":{\"id\":7}}\n").expect("parses"),
            Reply::Done(json!({"id": 7}))
        );
        assert_eq!(
            parse_reply("{\"ok\":false,\"error\":\"unknown tab: 9\"}\n").expect("parses"),
            Reply::Failed("unknown tab: 9".to_string())
        );
    }

    #[test]
    fn a_malformed_response_is_an_error() {
        let cases = [
            "",
            "not json",
            "{}",
            "{\"ok\":\"yes\"}",
            "{\"ok\":true}",
            "{\"ok\":false}",
            "{\"ok\":false,\"error\":5}",
        ];
        for case in cases {
            assert!(parse_reply(case).is_err(), "for `{case}`");
        }
    }

    #[test]
    fn the_version_prints_the_browser_line() {
        let result = json!({"app": "Gidora", "version": "157.0.8103.78"});
        assert_eq!(printed(&["version"], &result), "Gidora 157.0.8103.78\n");
    }

    #[test]
    fn the_tab_list_is_tab_separated_with_one_line_per_tab() {
        let result = json!({"tabs": [
            {"id": 11, "window": 3, "active": true, "loading": false,
             "url": "https://example.com/", "title": "Example"},
            {"id": 12, "window": 3, "active": false, "loading": true,
             "url": "https://example.org/a", "title": "Two\twords\nand a line\r"},
        ]});
        assert_eq!(
            printed(&["tab", "list"], &result),
            "11\t3\t*\thttps://example.com/\tExample\n\
             12\t3\t-\thttps://example.org/a\tTwo words and a line \n"
        );
    }

    #[test]
    fn an_empty_tab_list_prints_nothing() {
        assert_eq!(printed(&["tab", "list"], &json!({"tabs": []})), "");
    }

    #[test]
    fn new_prints_the_id_and_go_prints_the_url() {
        let result = json!({"id": 42, "url": "https://example.com/", "loading": false});
        assert_eq!(
            printed(&["tab", "new", "https://example.com/"], &result),
            "42\n"
        );
        assert_eq!(
            printed(&["tab", "go", "https://example.com/"], &result),
            "https://example.com/\n"
        );
    }

    #[test]
    fn close_click_and_type_print_nothing() {
        assert_eq!(printed(&["tab", "close"], &json!({"id": 42})), "");
        assert_eq!(
            printed(&["tab", "click", "#go"], &json!({"x": 1.0, "y": 2.0})),
            ""
        );
        assert_eq!(printed(&["tab", "type", "x"], &json!({})), "");
    }

    #[test]
    fn text_gets_a_trailing_newline_only_when_it_has_none() {
        assert_eq!(
            printed(&["tab", "text"], &json!({"text": "Hello"})),
            "Hello\n"
        );
        assert_eq!(
            printed(&["tab", "text"], &json!({"text": "Hello\n"})),
            "Hello\n"
        );
        assert_eq!(
            printed(&["tab", "html"], &json!({"html": "<html></html>"})),
            "<html></html>\n"
        );
    }

    #[test]
    fn eval_prints_json_and_raw_unquotes_only_strings() {
        let title = json!({"value": "A \"quoted\" title"});
        assert_eq!(
            printed(&["tab", "eval", "document.title"], &title),
            "\"A \\\"quoted\\\" title\"\n"
        );
        assert_eq!(
            printed(&["tab", "eval", "-r", "document.title"], &title),
            "A \"quoted\" title\n"
        );
        let object = json!({"value": {"offers": [1, 2]}});
        assert_eq!(
            printed(&["tab", "eval", "-r", "x"], &object),
            "{\"offers\":[1,2]}\n"
        );
        assert_eq!(
            printed(&["tab", "eval", "x"], &json!({"value": null})),
            "null\n"
        );
    }

    #[test]
    fn a_shot_goes_to_a_default_file_named_after_the_tab() {
        let result = json!({"id": 42, "png": STANDARD.encode(PNG)});
        let Output { stdout, file } = rendered(&["tab", "shot"], &result);
        let path = PathBuf::from("/var/tmp/shots/gidora-shot-42-1794000000.png");
        assert_eq!(stdout, format!("{}\n", path.display()).into_bytes());
        assert_eq!(
            file,
            Some(FileOutput {
                path,
                bytes: PNG.to_vec()
            })
        );
    }

    #[test]
    fn a_shot_goes_to_the_named_file_or_to_stdout() {
        let result = json!({"id": 42, "png": STANDARD.encode(PNG)});
        let Output { stdout, file } = rendered(&["tab", "shot", "-o", "page.png"], &result);
        assert_eq!(stdout, b"page.png\n");
        assert_eq!(
            file,
            Some(FileOutput {
                path: PathBuf::from("page.png"),
                bytes: PNG.to_vec()
            })
        );

        let Output { stdout, file } = rendered(&["tab", "shot", "-o", "-"], &result);
        assert_eq!(stdout, PNG);
        assert_eq!(file, None);
    }

    #[test]
    fn a_result_without_its_field_is_an_error() {
        let cases = [
            (vec!["version"], json!({"app": "Gidora"})),
            (vec!["tab", "list"], json!({})),
            (vec!["tab", "list"], json!({"tabs": [{"id": 1}]})),
            (vec!["tab", "new", "https://example.com/"], json!({})),
            (vec!["tab", "text"], json!({"text": 5})),
            (vec!["tab", "eval", "1"], json!({})),
            (vec!["tab", "shot"], json!({"id": 1, "png": "***"})),
            (vec!["tab", "shot"], json!({"png": STANDARD.encode(PNG)})),
        ];
        for (args, result) in cases {
            let mut line = vec!["gidoractl"];
            line.extend_from_slice(&args);
            let Cli { command, .. } = Cli::try_parse_from(line).expect("the arguments parse");
            assert!(
                render(&command, &result, &environment()).is_err(),
                "for {args:?} with {result}"
            );
        }
    }
}
