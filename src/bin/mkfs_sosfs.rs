#![cfg(feature = "std")]

use sos::fs::{
    build_superblock, derive_default_passkey, SOSFS_BLOCK_SIZE, SOSFS_FLAG_ENCRYPTION_REQUIRED,
    SOSFS_FLAG_VERSIONING_REQUIRED,
};
use std::env;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn usage() {
    eprintln!(
        "usage: mkfs-sosfs --image <path> --blocks <count> [--wal-blocks <n>] [--index-blocks <n>]"
    );
}

fn parse_u64(name: &str, value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("invalid value for {}: {}", name, value))
}

fn pseudo_random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut x = now as u64 ^ 0x9E37_79B9_7F4A_7C15;
    for b in &mut out {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        x = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        *b = (x & 0xFF) as u8;
    }
    out
}

fn parse_args(args: &[String]) -> Result<(PathBuf, u64, u64, u64), i32> {
    let mut image: Option<PathBuf> = None;
    let mut blocks: Option<u64> = None;
    let mut wal_blocks: u64 = 2048;
    let mut index_blocks: u64 = 1024;

    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--image" => {
                i += 1;
                if i >= args.len() {
                    usage();
                    return Err(2);
                }
                image = Some(PathBuf::from(&args[i]));
            }
            "--blocks" => {
                i += 1;
                if i >= args.len() {
                    usage();
                    return Err(2);
                }
                match parse_u64("blocks", &args[i]) {
                    Ok(v) => blocks = Some(v),
                    Err(e) => {
                        eprintln!("{}", e);
                        return Err(2);
                    }
                }
            }
            "--wal-blocks" => {
                i += 1;
                if i >= args.len() {
                    usage();
                    return Err(2);
                }
                match parse_u64("wal-blocks", &args[i]) {
                    Ok(v) => wal_blocks = v,
                    Err(e) => {
                        eprintln!("{}", e);
                        return Err(2);
                    }
                }
            }
            "--index-blocks" => {
                i += 1;
                if i >= args.len() {
                    usage();
                    return Err(2);
                }
                match parse_u64("index-blocks", &args[i]) {
                    Ok(v) => index_blocks = v,
                    Err(e) => {
                        eprintln!("{}", e);
                        return Err(2);
                    }
                }
            }
            "-h" | "--help" => {
                usage();
                return Err(0);
            }
            _ => {
                eprintln!("unknown option: {}", args[i]);
                usage();
                return Err(2);
            }
        }
        i += 1;
    }

    let image = match image {
        Some(v) => v,
        None => {
            usage();
            return Err(2);
        }
    };
    let blocks = match blocks {
        Some(v) if v > 16 => v,
        _ => {
            eprintln!("--blocks must be > 16");
            return Err(2);
        }
    };

    Ok((image, blocks, wal_blocks, index_blocks))
}

fn run_cli(args: &[String]) -> i32 {
    let (image, blocks, wal_blocks, index_blocks) = match parse_args(args) {
        Ok(v) => v,
        Err(code) => return code,
    };

    let data_start_lba = 2 + wal_blocks + index_blocks;
    if data_start_lba >= blocks {
        eprintln!("invalid layout: data region would be empty");
        return 2;
    }
    let data_blocks = blocks - data_start_lba;

    let mut file = match OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(&image)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cannot open image {}: {}", image.display(), e);
            return 1;
        }
    };

    let total_bytes = blocks * SOSFS_BLOCK_SIZE as u64;
    if let Err(e) = file.set_len(total_bytes) {
        eprintln!("cannot size image: {}", e);
        return 1;
    }

    let fs_uuid = pseudo_random_bytes::<16>();
    let fs_salt = pseudo_random_bytes::<32>();
    let flags = SOSFS_FLAG_ENCRYPTION_REQUIRED | SOSFS_FLAG_VERSIONING_REQUIRED;

    let superblock = build_superblock(
        0,
        1,
        flags,
        fs_uuid,
        fs_salt,
        1,
        2,
        wal_blocks,
        2 + wal_blocks,
        index_blocks,
        data_start_lba,
        data_blocks,
        0,
    );

    if file.seek(SeekFrom::Start(0)).is_err() || file.write_all(&superblock).is_err() {
        eprintln!("failed writing superblock A");
        return 1;
    }
    if file.seek(SeekFrom::Start(SOSFS_BLOCK_SIZE as u64)).is_err()
        || file.write_all(&superblock).is_err()
    {
        eprintln!("failed writing superblock B");
        return 1;
    }

    let mut verify = [0u8; SOSFS_BLOCK_SIZE];
    if file.seek(SeekFrom::Start(0)).is_err() || file.read_exact(&mut verify).is_err() {
        eprintln!("failed reading back superblock");
        return 1;
    }

    if verify != superblock {
        eprintln!("superblock verification mismatch");
        return 1;
    }

    let default_passkey = derive_default_passkey();
    println!("mkfs.sosfs: formatted {}", image.display());
    println!("- block_size={}", SOSFS_BLOCK_SIZE);
    println!("- blocks={}", blocks);
    println!("- wal_blocks={}", wal_blocks);
    println!("- index_blocks={}", index_blocks);
    println!("- data_blocks={}", data_blocks);
    println!("- default_passkey_sha256_sos={:02x?}", default_passkey);

    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    std::process::exit(run_cli(&args));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{remove_dir, remove_file};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_image_path(suffix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("sos-mkfs-test-{}-{}.img", suffix, unique))
    }

    #[test]
    fn test_parse_u64_rejects_invalid() {
        assert!(parse_u64("blocks", "nope").is_err());
    }

    #[test]
    fn test_parse_args_paths() {
        let help = vec!["mkfs-sosfs".to_string(), "--help".to_string()];
        assert_eq!(parse_args(&help), Err(0));

        let unknown = vec!["mkfs-sosfs".to_string(), "--wat".to_string()];
        assert_eq!(parse_args(&unknown), Err(2));

        let missing_image = vec![
            "mkfs-sosfs".to_string(),
            "--blocks".to_string(),
            "64".to_string(),
        ];
        assert_eq!(parse_args(&missing_image), Err(2));

        let bad_blocks = vec![
            "mkfs-sosfs".to_string(),
            "--image".to_string(),
            "/tmp/a.img".to_string(),
            "--blocks".to_string(),
            "16".to_string(),
        ];
        assert_eq!(parse_args(&bad_blocks), Err(2));

        let valid = vec![
            "mkfs-sosfs".to_string(),
            "--image".to_string(),
            "/tmp/b.img".to_string(),
            "--blocks".to_string(),
            "64".to_string(),
            "--wal-blocks".to_string(),
            "8".to_string(),
            "--index-blocks".to_string(),
            "8".to_string(),
        ];
        let parsed = parse_args(&valid).expect("valid parse args");
        assert_eq!(parsed.0, PathBuf::from("/tmp/b.img"));
        assert_eq!(parsed.1, 64);
        assert_eq!(parsed.2, 8);
        assert_eq!(parsed.3, 8);
    }

    #[test]
    fn test_cli_exit_codes_usage_and_success() {
        let missing_args = vec!["mkfs-sosfs".to_string()];
        assert_eq!(run_cli(&missing_args), 2);

        let bad_layout = vec![
            "mkfs-sosfs".to_string(),
            "--image".to_string(),
            temp_image_path("bad-layout").display().to_string(),
            "--blocks".to_string(),
            "32".to_string(),
            "--wal-blocks".to_string(),
            "24".to_string(),
            "--index-blocks".to_string(),
            "16".to_string(),
        ];
        assert_eq!(run_cli(&bad_layout), 2);

        let ok_path = temp_image_path("ok");
        let ok_args = vec![
            "mkfs-sosfs".to_string(),
            "--image".to_string(),
            ok_path.display().to_string(),
            "--blocks".to_string(),
            "4096".to_string(),
            "--wal-blocks".to_string(),
            "128".to_string(),
            "--index-blocks".to_string(),
            "128".to_string(),
        ];
        assert_eq!(run_cli(&ok_args), 0);
        let _ = remove_file(ok_path);
    }

    #[test]
    fn test_cli_exit_code_io_error_for_uncreatable_path() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("sos-mkfs-dir-{}", unique));
        std::fs::create_dir_all(&dir).expect("create temp dir");

        let args = vec![
            "mkfs-sosfs".to_string(),
            "--image".to_string(),
            dir.display().to_string(),
            "--blocks".to_string(),
            "64".to_string(),
            "--wal-blocks".to_string(),
            "8".to_string(),
            "--index-blocks".to_string(),
            "8".to_string(),
        ];

        assert_eq!(run_cli(&args), 1);

        let _ = remove_dir(&dir);
    }
}
