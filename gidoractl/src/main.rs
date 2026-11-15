mod cli;
mod client;
mod output;

use std::io::{self, ErrorKind, Write};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use clap::Parser;

use crate::cli::{Cli, Command};
use crate::client::SocketPath;
use crate::output::{Environment, FileOutput, Output, Reply};

const FAILURE: u8 = 1;

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("gidoractl: {error:#}");
            ExitCode::from(FAILURE)
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    let Cli {
        json,
        socket,
        command,
    } = cli;
    let request = cli::request(&command, &mut io::stdin().lock())?;
    let socket = SocketPath::resolve(socket)?;

    if !json {
        match &command {
            Command::Version => {
                print(format!("gidoractl {}\n", env!("CARGO_PKG_VERSION")).as_bytes())?;
            }
            Command::Tab(_) => {}
        }
    }

    let response = client::round_trip(&socket, &request.line())?;
    let reply = output::parse_reply(&response)?;
    if json {
        print(response.as_bytes())?;
        return match reply {
            Reply::Done(_) => Ok(ExitCode::SUCCESS),
            Reply::Failed(_) => Ok(ExitCode::from(FAILURE)),
        };
    }

    let result = match reply {
        Reply::Done(result) => result,
        Reply::Failed(error) => {
            eprintln!("gidoractl: {error}");
            return Ok(ExitCode::from(FAILURE));
        }
    };
    let Output { stdout, file } = output::render(&command, &result, &environment()?)?;
    if let Some(FileOutput { path, bytes }) = file {
        std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
    }
    print(&stdout)?;
    Ok(ExitCode::SUCCESS)
}

fn environment() -> anyhow::Result<Environment> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("the system clock is before 1970")?;
    Ok(Environment {
        temp_dir: std::env::temp_dir(),
        unix_seconds: elapsed.as_secs(),
    })
}

fn print(bytes: &[u8]) -> anyhow::Result<()> {
    let mut stdout = io::stdout().lock();
    let written = match stdout.write_all(bytes) {
        Ok(()) => stdout.flush(),
        Err(error) => Err(error),
    };
    let Err(error) = written else {
        return Ok(());
    };
    if error.kind() == ErrorKind::BrokenPipe {
        std::process::exit(0);
    }
    Err(error).context("writing to stdout")
}
