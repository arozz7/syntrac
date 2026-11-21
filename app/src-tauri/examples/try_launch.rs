//! Starts a command the way the app's AI help does (docs/PHASE4-PLAN.md, L4c): the same spawner, the same job object, no window, output saved to a
//! log file. Used to find out why a server started by the app behaves differently from one started in a terminal.
//!
//!   cargo run -p syntrac-app --example try_launch -- <program.exe> <working-folder or -> <log-file> <seconds> [arguments...]
//!
//! It waits up to <seconds>, says whether the program ended (and its exit code) or is still running, stops it (with what it started), and
//! prints the last lines of the log.

use std::time::{Duration, Instant};

use syntrac_app_lib::server_process::WindowsSpawner;
use syntrac_core::server_launch::LaunchCommand;
use syntrac_core::server_manager::ChildHandle;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        eprintln!("usage: try_launch <program.exe> <working-folder or -> <log-file> <seconds> [arguments...]");
        std::process::exit(2);
    }
    let command = LaunchCommand {
        program: args[0].clone(),
        working_dir: (args[1] != "-").then(|| args[1].clone()),
        args: args[4..].to_vec(),
    };
    let log = std::path::PathBuf::from(&args[2]);
    let seconds: u64 = args[3].parse().unwrap_or(30);
    println!("command: {}", command.display());
    println!("working folder: {}", command.working_folder().unwrap_or("(none: this program's own folder)"));

    let mut child = match WindowsSpawner::with_log(log.clone()).spawn_job(&command) {
        Ok(child) => child,
        Err(error) => {
            println!("could not start: {error}");
            return;
        }
    };
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut ended = None;
    while Instant::now() < deadline {
        if let Ok(Some(code)) = child.try_exit() {
            ended = Some(code);
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    match ended {
        Some(code) => println!("the program ENDED by itself with exit code {code}"),
        None => {
            println!("the program is still running after {seconds} s; stopping it and what it started");
            if let Err(error) = child.kill() {
                println!("could not stop it: {error}");
            }
        }
    }
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    println!("--- the last lines the program printed ({} in all):", lines.len());
    for line in &lines[lines.len().saturating_sub(12)..] {
        println!("{}", line.chars().take(220).collect::<String>());
    }
}
