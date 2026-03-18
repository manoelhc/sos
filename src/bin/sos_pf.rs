#![cfg(feature = "std")]

use sos::{
    pf_apply_with_runner, pf_dry_run_check_with_runner, pf_export_config_yaml,
    pf_export_running_ruleset_yaml_with_runner, pf_parse_config, PfError, SystemNftRunner,
};
use std::env;
use std::fs;
use std::path::PathBuf;

fn usage() {
    eprintln!("usage:");
    eprintln!("  sos-pf check --config <path>");
    eprintln!("  sos-pf apply --config <path>");
    eprintln!("  sos-pf export --config <path>");
    eprintln!("  sos-pf export-running");
}

fn parse_args(args: &[String]) -> Result<(String, Option<PathBuf>), ()> {
    if args.len() < 2 {
        usage();
        return Err(());
    }

    let cmd = args[1].clone();
    let mut config: Option<PathBuf> = None;

    let mut i = 2usize;
    while i < args.len() {
        match args[i].as_str() {
            "--config" => {
                i += 1;
                if i >= args.len() {
                    usage();
                    return Err(());
                }
                config = Some(PathBuf::from(&args[i]));
            }
            "-h" | "--help" => {
                usage();
                return Err(());
            }
            _ => {
                usage();
                return Err(());
            }
        }
        i += 1;
    }

    match cmd.as_str() {
        "check" | "apply" | "export" => {
            if config.is_none() {
                usage();
                return Err(());
            }
        }
        "export-running" => {
            if config.is_some() {
                usage();
                return Err(());
            }
        }
        _ => {
            usage();
            return Err(());
        }
    }

    Ok((cmd, config))
}

fn run_cli(args: &[String]) -> i32 {
    let (cmd, cfg_path) = match parse_args(args) {
        Ok(v) => v,
        Err(()) => return 3,
    };

    let runner = SystemNftRunner;

    match cmd.as_str() {
        "export-running" => match pf_export_running_ruleset_yaml_with_runner(&runner) {
            Ok(yaml) => {
                print!("{yaml}");
                0
            }
            Err(err) => {
                print_pf_error(&err);
                2
            }
        },
        _ => {
            let path = match cfg_path {
                Some(p) => p,
                None => return 3,
            };
            let yaml = match fs::read_to_string(&path) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("cannot read {}: {}", path.display(), e);
                    return 3;
                }
            };

            match cmd.as_str() {
                "check" => match pf_dry_run_check_with_runner(&yaml, &runner) {
                    Ok(_) => {
                        println!("sos-pf: check ok");
                        0
                    }
                    Err(err) => {
                        print_pf_error(&err);
                        2
                    }
                },
                "apply" => match pf_apply_with_runner(&yaml, &runner) {
                    Ok(()) => {
                        println!("sos-pf: apply ok");
                        0
                    }
                    Err(err) => {
                        print_pf_error(&err);
                        2
                    }
                },
                "export" => {
                    match pf_parse_config(&yaml).and_then(|cfg| pf_export_config_yaml(&cfg)) {
                        Ok(out) => {
                            print!("{out}");
                            0
                        }
                        Err(err) => {
                            print_pf_error(&err);
                            2
                        }
                    }
                }
                _ => 3,
            }
        }
    }
}

fn print_pf_error(err: &PfError) {
    match err {
        PfError::Io(m) => eprintln!("io error: {m}"),
        PfError::Yaml(m) => eprintln!("yaml error: {m}"),
        PfError::Json(m) => eprintln!("json error: {m}"),
        PfError::Schema(m) => eprintln!("schema error: {m}"),
        PfError::Command(m) => eprintln!("command error: {m}"),
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    std::process::exit(run_cli(&args));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    const VALID_YAML: &str = r#"
sos-pf:
  tables:
    - name: filter_table
      family: inet
      chains:
        - name: input_filter
          type: filter
          hook: input
          priority: 0
          policy: drop
          rules:
            - action: accept
"#;

    fn temp_yaml(contents: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("sos-pf-test-{unique}.yaml"));
        let mut f = std::fs::File::create(&path).expect("create temp yaml");
        f.write_all(contents.as_bytes()).expect("write temp yaml");
        path
    }

    #[test]
    fn parse_args_supports_all_commands() {
        assert!(parse_args(&["sos-pf".to_string()]).is_err());

        let check = parse_args(&[
            "sos-pf".to_string(),
            "check".to_string(),
            "--config".to_string(),
            "a.yaml".to_string(),
        ])
        .expect("check args");
        assert_eq!(check.0, "check");

        let apply = parse_args(&[
            "sos-pf".to_string(),
            "apply".to_string(),
            "--config".to_string(),
            "a.yaml".to_string(),
        ])
        .expect("apply args");
        assert_eq!(apply.0, "apply");

        let export = parse_args(&[
            "sos-pf".to_string(),
            "export".to_string(),
            "--config".to_string(),
            "a.yaml".to_string(),
        ])
        .expect("export args");
        assert_eq!(export.0, "export");

        let export_running = parse_args(&["sos-pf".to_string(), "export-running".to_string()])
            .expect("export-running args");
        assert_eq!(export_running.0, "export-running");
        assert!(export_running.1.is_none());
    }

    #[test]
    fn parse_args_rejects_missing_config_for_check() {
        assert!(parse_args(&["sos-pf".to_string(), "check".to_string()]).is_err());
    }

    #[test]
    fn run_cli_invalid_file_returns_three() {
        let args = vec![
            "sos-pf".to_string(),
            "check".to_string(),
            "--config".to_string(),
            "/definitely/not/found.yaml".to_string(),
        ];
        assert_eq!(run_cli(&args), 3);
    }

    #[test]
    fn run_cli_check_invalid_returns_two_before_nft_call() {
        let path = temp_yaml("sos-pf: {}\n");
        let args = vec![
            "sos-pf".to_string(),
            "check".to_string(),
            "--config".to_string(),
            path.display().to_string(),
        ];
        assert_eq!(run_cli(&args), 2);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn run_cli_export_valid_returns_zero() {
        let path = temp_yaml(VALID_YAML);
        let args = vec![
            "sos-pf".to_string(),
            "export".to_string(),
            "--config".to_string(),
            path.display().to_string(),
        ];
        assert_eq!(run_cli(&args), 0);
        let _ = std::fs::remove_file(path);
    }
}
