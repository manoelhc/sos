#![cfg(feature = "std")]

use sos::fs::{fsck_superblock_pair, SosfsFsckReport, SosfsFsckStatus, SOSFS_BLOCK_SIZE};
use std::env;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

fn usage() {
    eprintln!("usage: fsck-sosfs --image <path> [--strict]");
    eprintln!("Exit codes:");
    eprintln!("  0 = clean");
    eprintln!("  1 = warn (non-strict only)");
    eprintln!("  2 = corrupt");
    eprintln!("  3 = io/usage error");
}

fn parse_args(args: &[String]) -> Result<(PathBuf, bool), ()> {
    let mut image: Option<PathBuf> = None;
    let mut strict = false;

    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--image" => {
                i += 1;
                if i >= args.len() {
                    usage();
                    return Err(());
                }
                image = Some(PathBuf::from(&args[i]));
            }
            "--strict" => {
                strict = true;
            }
            "-h" | "--help" => {
                usage();
                return Err(());
            }
            _ => {
                eprintln!("unknown option: {}", args[i]);
                usage();
                return Err(());
            }
        }
        i += 1;
    }

    match image {
        Some(v) => Ok((v, strict)),
        None => {
            usage();
            Err(())
        }
    }
}

fn exit_code_for_status(status: SosfsFsckStatus, strict: bool) -> i32 {
    match status {
        SosfsFsckStatus::Clean => 0,
        SosfsFsckStatus::Warn => {
            if strict {
                2
            } else {
                1
            }
        }
        SosfsFsckStatus::Corrupt => 2,
    }
}

fn run_cli(args: &[String]) -> i32 {
    let (image, strict) = match parse_args(args) {
        Ok(v) => v,
        Err(()) => return 3,
    };

    let mut file = match File::open(&image) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cannot open image {}: {}", image.display(), e);
            return 3;
        }
    };

    let mut sb0 = [0u8; SOSFS_BLOCK_SIZE];
    let mut sb1 = [0u8; SOSFS_BLOCK_SIZE];

    if let Err(e) = file.seek(SeekFrom::Start(0)) {
        eprintln!("seek error: {}", e);
        return 3;
    }
    if let Err(e) = file.read_exact(&mut sb0) {
        eprintln!("read superblock 0 error: {}", e);
        return 3;
    }
    if let Err(e) = file.seek(SeekFrom::Start(SOSFS_BLOCK_SIZE as u64)) {
        eprintln!("seek error: {}", e);
        return 3;
    }
    if let Err(e) = file.read_exact(&mut sb1) {
        eprintln!("read superblock 1 error: {}", e);
        return 3;
    }

    let report = fsck_superblock_pair(&sb0, &sb1, strict);

    print_fsck_report(&report);

    exit_code_for_status(report.status, strict)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    std::process::exit(run_cli(&args));
}

fn print_fsck_report(report: &SosfsFsckReport) {
    match report.status {
        SosfsFsckStatus::Clean => {
            println!("fsck: clean");
        }
        SosfsFsckStatus::Warn => {
            println!("fsck: warn");
            for issue in report.issues.iter().flatten() {
                println!("  - {:?}", issue);
            }
        }
        SosfsFsckStatus::Corrupt => {
            println!("fsck: corrupt");
            for issue in report.issues.iter().flatten() {
                println!("  - {:?}", issue);
            }
        }
    }

    if let Some(info) = &report.info {
        println!("  generation={}", info.active_generation);
        println!("  flags=0x{:x}", info.flags);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sos::fs::{
        build_superblock, SOSFS_FLAG_ENCRYPTION_REQUIRED, SOSFS_FLAG_VERSIONING_REQUIRED,
    };
    use std::fs::{remove_file, File};
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn write_superblocks_image(
        sb0: [u8; SOSFS_BLOCK_SIZE],
        sb1: [u8; SOSFS_BLOCK_SIZE],
    ) -> Result<PathBuf, String> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("sos-fsck-test-{}.img", unique));
        let mut f = File::create(&path).map_err(|e| format!("create temp image: {}", e))?;
        f.write_all(&sb0)
            .map_err(|e| format!("write sb0 temp image: {}", e))?;
        f.write_all(&sb1)
            .map_err(|e| format!("write sb1 temp image: {}", e))?;
        Ok(path)
    }

    fn valid_superblock(generation: u64) -> [u8; SOSFS_BLOCK_SIZE] {
        build_superblock(
            0,
            1,
            SOSFS_FLAG_ENCRYPTION_REQUIRED | SOSFS_FLAG_VERSIONING_REQUIRED,
            [7u8; 16],
            [8u8; 32],
            generation,
            2,
            256,
            258,
            128,
            386,
            8192,
            44,
        )
    }

    #[test]
    fn test_exit_code_for_status() {
        assert_eq!(exit_code_for_status(SosfsFsckStatus::Clean, false), 0);
        assert_eq!(exit_code_for_status(SosfsFsckStatus::Warn, false), 1);
        assert_eq!(exit_code_for_status(SosfsFsckStatus::Warn, true), 2);
        assert_eq!(exit_code_for_status(SosfsFsckStatus::Corrupt, false), 2);
    }

    #[test]
    fn test_parse_args_invalid_paths_and_help() {
        let missing_image_value = vec!["fsck-sosfs".to_string(), "--image".to_string()];
        assert!(parse_args(&missing_image_value).is_err());

        let missing_image_flag = vec!["fsck-sosfs".to_string()];
        assert!(parse_args(&missing_image_flag).is_err());

        let unknown = vec!["fsck-sosfs".to_string(), "--wat".to_string()];
        assert!(parse_args(&unknown).is_err());

        let help = vec!["fsck-sosfs".to_string(), "--help".to_string()];
        assert!(parse_args(&help).is_err());
    }

    #[test]
    fn test_parse_args_valid_strict() {
        let valid = vec![
            "fsck-sosfs".to_string(),
            "--image".to_string(),
            "/tmp/a.img".to_string(),
            "--strict".to_string(),
        ];
        let (path, strict) = parse_args(&valid).expect("valid parse args");
        assert_eq!(path, PathBuf::from("/tmp/a.img"));
        assert!(strict);
    }

    #[test]
    fn test_cli_exit_code_clean_warn_corrupt_and_error() {
        let clean = valid_superblock(9);
        let mut corrupt = clean;
        corrupt[0] = 0xFF;

        let clean_path = write_superblocks_image(clean, clean).expect("make clean image");
        let warn_path = write_superblocks_image(clean, corrupt).expect("make warn image");
        let corrupt_path = write_superblocks_image(corrupt, corrupt).expect("make corrupt image");

        let clean_args = vec![
            "fsck-sosfs".to_string(),
            "--image".to_string(),
            clean_path.display().to_string(),
        ];
        let warn_args = vec![
            "fsck-sosfs".to_string(),
            "--image".to_string(),
            warn_path.display().to_string(),
        ];
        let warn_strict_args = vec![
            "fsck-sosfs".to_string(),
            "--image".to_string(),
            warn_path.display().to_string(),
            "--strict".to_string(),
        ];
        let corrupt_args = vec![
            "fsck-sosfs".to_string(),
            "--image".to_string(),
            corrupt_path.display().to_string(),
        ];
        let io_error_args = vec![
            "fsck-sosfs".to_string(),
            "--image".to_string(),
            "/definitely/nonexistent/sosfs.img".to_string(),
        ];

        assert_eq!(run_cli(&clean_args), 0);
        assert_eq!(run_cli(&warn_args), 1);
        assert_eq!(run_cli(&warn_strict_args), 2);
        assert_eq!(run_cli(&corrupt_args), 2);
        assert_eq!(run_cli(&io_error_args), 3);

        let _ = remove_file(clean_path);
        let _ = remove_file(warn_path);
        let _ = remove_file(corrupt_path);
    }

    #[test]
    fn test_cli_exit_code_read_and_seek_errors() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let short_path = std::env::temp_dir().join(format!("sos-fsck-short-{}.img", unique));
        let mut short = File::create(&short_path).expect("create short fsck image");
        short.write_all(&[0u8; 64]).expect("write short fsck image");

        let args = vec![
            "fsck-sosfs".to_string(),
            "--image".to_string(),
            short_path.display().to_string(),
        ];
        assert_eq!(run_cli(&args), 3);

        let _ = remove_file(short_path);
    }
}
