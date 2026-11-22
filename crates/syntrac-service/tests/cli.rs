use std::path::PathBuf;

use syntrac_service::CliArgs;

fn parse(args: &[&str]) -> Result<CliArgs, syntrac_service::CliError> {
    CliArgs::parse(args.iter().map(|a| (*a).to_owned()))
}

#[test]
fn no_arguments_means_run_until_stopped_with_everything_enabled() {
    let args = parse(&[]).unwrap();
    assert_eq!(args, CliArgs::default());
    assert!(!args.no_etw && args.seconds.is_none() && args.db.is_none() && args.config.is_none());
}

#[test]
fn every_option_is_read() {
    let args =
        parse(&["--db", "D:/x.db", "--config", "c.toml", "--seconds", "45", "--no-etw", "--summary-file", "s.txt"])
            .unwrap();
    assert_eq!(args.summary_file, Some(PathBuf::from("s.txt")));
    assert_eq!(args.db, Some(PathBuf::from("D:/x.db")));
    assert_eq!(args.config, Some(PathBuf::from("c.toml")));
    assert_eq!(args.seconds, Some(45));
    assert!(args.no_etw);
}

#[test]
fn the_service_flag_is_what_the_service_control_manager_passes() {
    assert!(parse(&["--service"]).unwrap().service);
    assert!(!parse(&[]).unwrap().service);
}

#[test]
fn the_status_and_ping_commands_and_their_pipe_name_are_read() {
    assert!(parse(&["--status"]).unwrap().status);
    assert!(parse(&["--ping"]).unwrap().ping);
    assert_eq!(parse(&["--status", "--pipe", "syntrac-console"]).unwrap().pipe.as_deref(), Some("syntrac-console"));
    assert!(parse(&["--pipe"]).is_err(), "--pipe needs a name");
}

#[test]
fn help_is_recognised() {
    assert!(parse(&["--help"]).unwrap().help);
    assert!(parse(&["-h"]).unwrap().help);
}

#[test]
fn unknown_options_and_missing_or_bad_values_are_errors() {
    assert!(parse(&["--bogus"]).is_err());
    assert!(parse(&["--db"]).is_err());
    assert!(parse(&["--seconds", "abc"]).is_err());
    assert!(parse(&["--seconds", "0"]).is_err());
}

#[test]
fn check_db_takes_a_file_and_needs_one() {
    let args = parse(&["--check-db", r"D:\Syntrac\data\syntrac.db"]).unwrap();
    assert_eq!(args.check_db, Some(PathBuf::from(r"D:\Syntrac\data\syntrac.db")));
    assert!(matches!(parse(&["--check-db"]), Err(syntrac_service::CliError::MissingValue(_))));
}

#[test]
fn the_version_option_is_read() {
    assert!(parse(&["--version"]).unwrap().version);
    assert!(!parse(&[]).unwrap().version);
}

#[test]
fn db_schema_takes_a_file_and_needs_one() {
    let args = parse(&["--db-schema", r"D:\Syntrac\data\syntrac.db"]).unwrap();
    assert_eq!(args.db_schema, Some(PathBuf::from(r"D:\Syntrac\data\syntrac.db")));
    assert!(matches!(parse(&["--db-schema"]), Err(syntrac_service::CliError::MissingValue(_))));
}
