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

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut image: Option<PathBuf> = None;
    let mut strict = false;

    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--image" => {
                i += 1;
                if i >= args.len() {
                    usage();
                    std::process::exit(3);
                }
                image = Some(PathBuf::from(&args[i]));
            }
            "--strict" => {
                strict = true;
            }
            "-h" | "--help" => {
                usage();
                return;
            }
            _ => {
                eprintln!("unknown option: {}", args[i]);
                usage();
                std::process::exit(3);
            }
        }
        i += 1;
    }

    let image = match image {
        Some(v) => v,
        None => {
            usage();
            std::process::exit(3);
        }
    };

    let mut file = match File::open(&image) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cannot open image {}: {}", image.display(), e);
            std::process::exit(3);
        }
    };

    let mut sb0 = [0u8; SOSFS_BLOCK_SIZE];
    let mut sb1 = [0u8; SOSFS_BLOCK_SIZE];

    if let Err(e) = file.seek(SeekFrom::Start(0)) {
        eprintln!("seek error: {}", e);
        std::process::exit(3);
    }
    if let Err(e) = file.read_exact(&mut sb0) {
        eprintln!("read superblock 0 error: {}", e);
        std::process::exit(3);
    }
    if let Err(e) = file.seek(SeekFrom::Start(SOSFS_BLOCK_SIZE as u64)) {
        eprintln!("seek error: {}", e);
        std::process::exit(3);
    }
    if let Err(e) = file.read_exact(&mut sb1) {
        eprintln!("read superblock 1 error: {}", e);
        std::process::exit(3);
    }

    let report = fsck_superblock_pair(&sb0, &sb1, strict);

    print_fsck_report(&report);

    match report.status {
        SosfsFsckStatus::Clean => std::process::exit(0),
        SosfsFsckStatus::Warn => {
            if strict {
                std::process::exit(2);
            } else {
                std::process::exit(1);
            }
        }
        SosfsFsckStatus::Corrupt => std::process::exit(2),
    }
}

fn print_fsck_report(report: &SosfsFsckReport) {
    match report.status {
        SosfsFsckStatus::Clean => {
            println!("fsck: clean");
        }
        SosfsFsckStatus::Warn => {
            println!("fsck: warn");
            for issue_opt in &report.issues {
                if let Some(issue) = issue_opt {
                    println!("  - {:?}", issue);
                }
            }
        }
        SosfsFsckStatus::Corrupt => {
            println!("fsck: corrupt");
            for issue_opt in &report.issues {
                if let Some(issue) = issue_opt {
                    println!("  - {:?}", issue);
                }
            }
        }
    }

    if let Some(info) = &report.info {
        println!("  generation={}", info.active_generation);
        println!("  flags=0x{:x}", info.flags);
    }
}
