//! printcraft-cli — headless PrintCraft.
//!
//! Usage: `printcraft-cli --help`, which prints `USAGE` below.
//!
//! `run` and `mcp` drive the same tool table (`printcraft-automation`). In `run`, values parse as
//! JSON when they can (`pages=[1,3]`, `degrees=90`) and are strings otherwise. A script runs its
//! steps in one session, so `doc_open` returns id 1, the next document id 2, and so on. A step's
//! `"out"` (like `--out`) saves its image there. With `--root DIR`, every path a tool or step
//! names, `"out"` included, must be inside DIR, and relative paths resolve inside it.
//!
//! The MCP server never starts on its own: it runs only when this command is launched (normally
//! by an agent configured to use it), talks only over stdio, and exits when stdin closes.
//!
//! `check` is the robustness harness: every file is opened, inspected and fully rendered in a
//! *separate child process* with a wall-clock timeout, so hangs, panics and aborts in any
//! dependency are observed and reported instead of taking the harness down.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(feature = "mcp")]
mod automation_args;
mod batch;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use printcraft_engine::export::ImageFormat;
use printcraft_render::{PageRenderer, RenderConfig, RenderRequest, RequestKind, inspect};

/// The `mcp` lines of [`USAGE`]: only a build with the `mcp` feature has the command.
#[cfg(feature = "mcp")]
macro_rules! mcp_usage {
    () => {
        "\
printcraft-cli mcp    [--root DIR] [--compact] [--automation-read-root DIR] [--automation-write-root DIR]
                                                           MCP server on stdin/stdout (opt-in)
                                                           --compact lists a core set of tools plus tool_search and tool_call
                                                           --automation-read-root / --automation-write-root confine the files the
                                                           agent's tools read (open, combine, import, fonts, certificates…) and
                                                           write (save, export, extract, split, reports…) to DIR, links followed;
                                                           a root left out grants none of its access. --root, when also given,
                                                           applies as well
"
    };
}
#[cfg(not(feature = "mcp"))]
macro_rules! mcp_usage {
    () => {
        ""
    };
}

mod check;
mod commands;
mod options;
mod run;
mod ui;

use check::{audit, check, check_one};
use commands::{combine, edit, extract, info, render, split, text};
use options::{OptionSpec, flag, validate_options};
#[cfg(feature = "mcp")]
use run::mcp;
use run::{automation, print_output, run, tools};
use ui::ui;

/// The full usage, printed by `--help` (also after a command, e.g. `render --help`).
const USAGE: &str = concat!(
    "\
printcraft-cli info   <file.pdf> [--password PW]            document summary as JSON
                                                          (includes any declared PDF/A or PDF/UA level)
printcraft-cli render <file.pdf> --page N [--dpi 96] --out x.png   (.png, .jpg, .tif or .pam)
printcraft-cli text   <file.pdf> [--page N]                  extracted text (pages separated by form feeds)
printcraft-cli edit   <in.pdf> --out out.pdf [--rotate 1,3:90] [--delete 2,4] [--move 5:1]
                      [--insert-blank 1] [--title T] [--author A] [--full]
                      (options run in order; page numbers refer to the document after the earlier options)
printcraft-cli combine <a.pdf> <b.pdf> … --out combined.pdf
printcraft-cli extract <in.pdf> --pages 1,3,5-8 --out out.pdf    (pages and ranges, in the order given)
printcraft-cli split   <in.pdf> (--every N | --before 3,7) [--out-dir DIR]
printcraft-cli check  <files or dirs…> [--timeout 20] [--dpi 36] [--json out.json]
printcraft-cli audit  <file.pdf> [--password PW]            find faux redactions (fails when found)
printcraft-cli batch <files or dirs...> --steps convert,ocr,optimize --out-dir DIR
                      [--recursive] [--root DIR] [--stop-on-error]
                      [--ppi 150] [--quality 60] [--dpi 300] [--language en] [--ocr-all-pages]
printcraft-cli tools                                       automation tools and their JSON Schemas
printcraft-cli run    <tool> [key=value …] [--root DIR] [--out image.png]
printcraft-cli run    --script steps.json [--root DIR] [--report]
                      [{\"tool\": \"doc_open\", \"args\": {…}}, …]; --report prints one JSON document for the run
",
    mcp_usage!(),
    "\
printcraft-cli ui     --control FILE <method> [key=value …] [--out shot.png]
                                                           drive a running app started with --control FILE
CivilOps PDF: https://civilops.de"
);

fn main() -> ExitCode {
    // `std::env::args` panics on an argument that is not UTF-8 ("called `Result::unwrap()` on an
    // `Err` value"), which a Latin-1 file name is, and it panics while collecting them: even
    // `--help` ended that way. Such a name cannot be passed on either, since the paths here are
    // `str`, so it is a usage error (photocraft#1108).
    let mut args: Vec<String> = Vec::new();
    for arg in std::env::args_os().skip(1) {
        match arg.into_string() {
            Ok(a) => args.push(a),
            Err(bad) => {
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "printcraft-cli: `{}` is not valid UTF-8, so it cannot be used as an argument",
                    bad.to_string_lossy()
                );
                return ExitCode::FAILURE;
            }
        }
    }
    // `<command> --help` too; only right after the command, so a later `-h` (a value) still reaches it.
    let command = match args.get(1).map(String::as_str) {
        Some("--help" | "-h") => Some("--help"),
        _ => args.first().map(String::as_str),
    };
    let result = match command {
        Some("--help" | "-h" | "help") => stdout_line(format_args!("{USAGE}")),
        None => Err(format!("usage:\n{USAGE}").into()),
        Some("info") => info(&args[1..]),
        Some("render") => render(&args[1..]),
        Some("text") => text(&args[1..]),
        Some("edit") => edit(&args[1..]),
        Some("combine") => combine(&args[1..]),
        Some("extract") => extract(&args[1..]),
        Some("split") => split(&args[1..]),
        Some("check") => check(&args[1..]),
        Some("check-one") => check_one(&args[1..]),
        Some("audit") => audit(&args[1..]),
        Some("tools") => tools(),
        Some("run") => run(&args[1..]),
        Some("batch") => batch::run(&args[1..]),
        Some("ui") => ui(&args[1..]),
        #[cfg(feature = "mcp")]
        Some("mcp") => mcp(&args[1..]),
        #[cfg(not(feature = "mcp"))]
        Some("mcp") => Err("mcp: this build leaves out the MCP server (built without the `mcp` feature)".into()),
        Some("--version") => version(),
        Some(other) => Err(format!("unknown command '{other}'; see printcraft-cli --help").into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Stdout(e)) if e.kind() == std::io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            let _ = writeln!(std::io::stderr().lock(), "printcraft-cli: {e}");
            ExitCode::FAILURE
        }
    }
}

// Keep stdout errors typed: a closed pipe is normal, but file and command errors still fail.
#[derive(Debug)]
enum CliError {
    Message(String),
    Stdout(std::io::Error),
}

impl From<String> for CliError {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

impl From<&str> for CliError {
    fn from(message: &str) -> Self {
        Self::Message(message.to_string())
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Message(message) => f.write_str(message),
            Self::Stdout(error) => write!(f, "stdout: {error}"),
        }
    }
}

fn stdout_line(line: std::fmt::Arguments<'_>) -> Result<(), CliError> {
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{line}").and_then(|()| stdout.flush()).map_err(CliError::Stdout)
}

fn version() -> Result<(), CliError> {
    // CivilOps PDF's version (the workspace crates carry upstream's).
    stdout_line(format_args!("CivilOps PDF (Kommandozeile) {}", printcraft_engine::links::VERSION))?;
    stdout_line(format_args!("Web:     {}", printcraft_engine::links::APP_PAGE))?;
    stdout_line(format_args!("Source:  {}", printcraft_engine::links::GITHUB))?;
    Ok(())
}
