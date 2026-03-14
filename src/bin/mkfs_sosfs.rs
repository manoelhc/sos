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

fn main() {
    let args: Vec<String> = env::args().collect();
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
                    std::process::exit(2);
                }
                image = Some(PathBuf::from(&args[i]));
            }
            "--blocks" => {
                i += 1;
                if i >= args.len() {
                    usage();
                    std::process::exit(2);
                }
                match parse_u64("blocks", &args[i]) {
                    Ok(v) => blocks = Some(v),
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(2);
                    }
                }
            }
            "--wal-blocks" => {
                i += 1;
                if i >= args.len() {
                    usage();
                    std::process::exit(2);
                }
                match parse_u64("wal-blocks", &args[i]) {
                    Ok(v) => wal_blocks = v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(2);
                    }
                }
            }
            "--index-blocks" => {
                i += 1;
                if i >= args.len() {
                    usage();
                    std::process::exit(2);
                }
                match parse_u64("index-blocks", &args[i]) {
                    Ok(v) => index_blocks = v,
                    Err(e) => {
                        eprintln!("{}", e);
                        std::process::exit(2);
                    }
                }
            }
            "-h" | "--help" => {
                usage();
                return;
            }
            _ => {
                eprintln!("unknown option: {}", args[i]);
                usage();
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let image = match image {
        Some(v) => v,
        None => {
            usage();
            std::process::exit(2);
        }
    };
    let blocks = match blocks {
        Some(v) if v > 16 => v,
        _ => {
            eprintln!("--blocks must be > 16");
            std::process::exit(2);
        }
    };

    let data_start_lba = 2 + wal_blocks + index_blocks;
    if data_start_lba >= blocks {
        eprintln!("invalid layout: data region would be empty");
        std::process::exit(2);
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
            std::process::exit(1);
        }
    };

    let total_bytes = blocks * SOSFS_BLOCK_SIZE as u64;
    if let Err(e) = file.set_len(total_bytes) {
        eprintln!("cannot size image: {}", e);
        std::process::exit(1);
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
        std::process::exit(1);
    }
    if file.seek(SeekFrom::Start(SOSFS_BLOCK_SIZE as u64)).is_err()
        || file.write_all(&superblock).is_err()
    {
        eprintln!("failed writing superblock B");
        std::process::exit(1);
    }

    let mut verify = [0u8; SOSFS_BLOCK_SIZE];
    if file.seek(SeekFrom::Start(0)).is_err() || file.read_exact(&mut verify).is_err() {
        eprintln!("failed reading back superblock");
        std::process::exit(1);
    }

    if verify != superblock {
        eprintln!("superblock verification mismatch");
        std::process::exit(1);
    }

    let default_passkey = derive_default_passkey();
    println!("mkfs.sosfs: formatted {}", image.display());
    println!("- block_size={}", SOSFS_BLOCK_SIZE);
    println!("- blocks={}", blocks);
    println!("- wal_blocks={}", wal_blocks);
    println!("- index_blocks={}", index_blocks);
    println!("- data_blocks={}", data_blocks);
    println!("- default_passkey_sha256_sos={:02x?}", default_passkey);
}
