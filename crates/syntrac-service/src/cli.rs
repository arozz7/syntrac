//! Command-line options for the console host. Deliberately tiny: four options, no dependency.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("unknown option `{0}` (try --help)")]
    UnknownOption(String),
    #[error("option `{0}` needs a value")]
    MissingValue(String),
    #[error("`--seconds` needs a whole number greater than zero, got `{0}`")]
    BadSeconds(String),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CliArgs {
    /// Configuration file; defaults to `config/service.toml` when that exists.
    pub config: Option<PathBuf>,
    /// Database file; overrides the configuration.
    pub db: Option<PathBuf>,
    /// Stop after this many seconds instead of waiting for Ctrl+C.
    pub seconds: Option<u64>,
    /// Skip the ETW trace (no elevation needed): startup snapshot, metrics and storage only.
    pub no_etw: bool,
    /// Also write the run summary to this file, directly (not through a pipe), so it survives a stalled exit.
    pub summary_file: Option<PathBuf>,
    /// Run as a Windows service. Only the Service Control Manager passes this; it reads the installed config.
    pub service: bool,
    pub help: bool,
}

pub const USAGE: &str = "syntrac-service (console mode)

USAGE:
    syntrac-service [OPTIONS]

OPTIONS:
    --config <file>    Configuration file (default: config/service.toml if present)
    --db <file>        Database file (default: data/syntrac-console.db)
    --seconds <n>      Stop after n seconds (default: run until Ctrl+C)
    --no-etw           Do not start the ETW trace; no administrator rights needed
    --summary-file <f> Also write the run summary to this file
    --service          Run as a Windows service (used by the Service Control Manager only)
    -h, --help         Show this help

ETW needs an elevated (Administrator) terminal.";

impl CliArgs {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, CliError> {
        let mut parsed = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--config" => parsed.config = Some(PathBuf::from(value(&mut args, &arg)?)),
                "--db" => parsed.db = Some(PathBuf::from(value(&mut args, &arg)?)),
                "--seconds" => {
                    let text = value(&mut args, &arg)?;
                    parsed.seconds = Some(
                        text.parse::<u64>()
                            .ok()
                            .filter(|n| *n > 0)
                            .ok_or_else(|| CliError::BadSeconds(text.clone()))?,
                    );
                }
                "--no-etw" => parsed.no_etw = true,
                "--service" => parsed.service = true,
                "--summary-file" => parsed.summary_file = Some(PathBuf::from(value(&mut args, &arg)?)),
                "-h" | "--help" => parsed.help = true,
                _ => return Err(CliError::UnknownOption(arg)),
            }
        }
        Ok(parsed)
    }
}

fn value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, CliError> {
    args.next().ok_or_else(|| CliError::MissingValue(option.to_owned()))
}
