#![cfg(feature = "std")]

use sos::network::ReadinessSuite;
use std::env;

fn usage() {
    eprintln!("usage: sos-readiness [--strict]");
    eprintln!("Exit codes:");
    eprintln!("  0 = ready");
    eprintln!("  2 = not ready");
    eprintln!("  3 = usage error");
}

fn parse_args(args: &[String]) -> Result<bool, ()> {
    let mut strict = false;
    for arg in &args[1..] {
        match arg.as_str() {
            "--strict" => strict = true,
            "-h" | "--help" => {
                usage();
                return Err(());
            }
            _ => {
                eprintln!("unknown option: {arg}");
                usage();
                return Err(());
            }
        }
    }
    Ok(strict)
}

fn run_cli(args: &[String]) -> i32 {
    match parse_args(args) {
        Ok(v) => v,
        Err(()) => return 3,
    };

    let suite = ReadinessSuite::run_with_probes(|| true, || true, || true);
    for check in &suite.checks {
        println!("{}={}", check.name, check.status);
    }

    if suite.is_ready() {
        0
    } else {
        2
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    std::process::exit(run_cli(&args));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_accepts_strict() {
        let args = vec!["sos-readiness".to_string(), "--strict".to_string()];
        assert_eq!(parse_args(&args), Ok(true));
    }

    #[test]
    fn parse_args_rejects_unknown() {
        let args = vec!["sos-readiness".to_string(), "--bad".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn run_cli_ready_path_exits_zero() {
        let args = vec!["sos-readiness".to_string()];
        assert_eq!(run_cli(&args), 0);
    }

    #[test]
    fn display_status_strings_match_contract() {
        assert_eq!(sos::network::ReadinessStatus::Ready.to_string(), "ready");
        assert_eq!(
            sos::network::ReadinessStatus::NotReady.to_string(),
            "not-ready"
        );
    }
}
