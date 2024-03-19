mod attribute_file;
mod compress;
mod decode_attribut;
mod filesystem;
mod hosts;
mod pool;
mod util;
mod view;

use crate::hosts::HostsTrait;
use attribute_file::{Search, SearchTrait};
use clap::Parser;
use decode_attribut::{FileAttributes, FileType};
use std::env;
use std::{
    fs::File,
    io::{Error, Read, Write},
};

const CHUNK_SIZE: usize = 4 * 65536;

#[derive(Parser)]
struct Cli {
    /// The pattern to look for
    command: String,
    /// The path to the file to read
    path: String,
    /// host
    #[arg(long)]
    host: Option<String>,
    /// backup number
    #[arg(short, long)]
    number: Option<u32>,
    /// share name
    #[arg(short, long)]
    share: Option<String>,
}

fn reader_to_stdout<R: Read>(mut reader: R) -> Result<(), Error> {
    let mut buffer = vec![0; CHUNK_SIZE];
    loop {
        let count = reader.read(&mut buffer).unwrap();
        if count == 0 {
            return Ok(());
        }
        std::io::stdout().write_all(&buffer[..count]).unwrap();
    }
}

fn uncompress_to_stdout(input_file: &str) -> Result<(), Error> {
    let input_file = File::open(input_file).unwrap();
    let reader = compress::BackupPCReader::new(input_file);
    reader_to_stdout(reader)
}

fn plain_to_stdout(input_file: &str) -> Result<(), Error> {
    let input_file = File::open(input_file).unwrap();
    let mut reader = std::io::BufReader::new(input_file);
    reader_to_stdout(&mut reader)
}

fn pool_file_to_stdout(topdir: &str, file_hash: &str) -> Result<(), String> {
    let md5_hash: Vec<u8> = util::hex_string_to_vec(file_hash);

    match pool::find_file_in_backuppc(topdir, &md5_hash, None) {
        Ok((file_path, is_compressed)) => {
            if is_compressed {
                uncompress_to_stdout(&file_path).unwrap();
            } else {
                plain_to_stdout(&file_path).unwrap();
            }

            Ok(())
        }
        Err(message) => Err(message.to_string()),
    }
}

fn print_ls(mut attrs: Vec<FileAttributes>) {
    // Print each elements as the "ls -lsh" command will do.
    // Data must be aligned
    // Sorted by name
    attrs.sort_by(|a, b| a.name.cmp(&b.name));
    for attr in attrs {
        // Show the mode in the form drwxr-xr-x with the help of attr.mode and attr.type_
        let mode = match attr.type_ {
            FileType::File | FileType::Hardlink => "-",
            FileType::Symlink => "l",
            FileType::Chardev => "c",
            FileType::Blockdev => "b",
            FileType::Dir => "d",
            FileType::Fifo => "p",
            FileType::Unknown => "?",
            FileType::Socket => "s",
            FileType::Deleted => "D",
        };
        let mode = format!(
            "{}{}{}{}{}{}{}{}{}{}",
            mode,
            if attr.mode & 0o400 != 0 { "r" } else { "-" },
            if attr.mode & 0o200 != 0 { "w" } else { "-" },
            if attr.mode & 0o100 != 0 { "x" } else { "-" },
            if attr.mode & 0o040 != 0 { "r" } else { "-" },
            if attr.mode & 0o020 != 0 { "w" } else { "-" },
            if attr.mode & 0o010 != 0 { "x" } else { "-" },
            if attr.mode & 0o004 != 0 { "r" } else { "-" },
            if attr.mode & 0o002 != 0 { "w" } else { "-" },
            if attr.mode & 0o001 != 0 { "x" } else { "-" }
        );

        println!(
            "{} {} {: <5} {: <5} {: <10} {: <12} {}",
            mode, attr.nlinks, attr.uid, attr.gid, attr.size, attr.mtime, attr.name
        );
    }
}

fn read_file_to_stdout(
    topdir: &str,
    hostname: Option<String>,
    number: Option<u32>,
    share: Option<String>,
    file: &str,
) -> Result<(), String> {
    if hostname.is_some() || number.is_some() || share.is_some() {
        let Some(hostname) = hostname else {
            return Err("No host specified".to_string());
        };
        let Some(backup_number) = number else {
            return Err("No backup number specified".to_string());
        };

        let Some(share) = share else {
            return Err("No share specified".to_string());
        };

        let attrs = Search::get_file(topdir, &hostname, backup_number, &share, file).unwrap();
        if attrs.len() == 1 && attrs[0].bpc_digest.len > 0 {
            let hex = util::vec_to_hex_string(&attrs[0].bpc_digest.digest);
            pool_file_to_stdout(topdir, &hex)?;
        } else {
            return Err("File not found".to_string());
        }
        return Ok(());
    }

    let file_path = std::path::Path::new(&file);
    if file_path.exists() {
        uncompress_to_stdout(file).unwrap();
        Ok(())
    } else {
        pool_file_to_stdout(topdir, file)
    }
}

fn main() {
    let args = Cli::parse();

    let topdir = match env::var("BPC_TOPDIR") {
        Ok(value) => value,
        Err(_) => "/var/lib/backuppc".to_string(),
    };

    let command = args.command;
    let filename = args.path;

    match command.as_str() {
        "cat" => {
            read_file_to_stdout(&topdir, args.host, args.number, args.share, &filename).unwrap();
        }
        "ls" => {
            let Some(hostname) = args.host else {
                println!("No host specified");
                return;
            };
            let Some(backup_number) = args.number else {
                println!("No backup number specified");
                return;
            };

            let Some(share) = args.share else {
                println!("No share specified");
                return;
            };

            let attrs =
                Search::list_file_from_dir(&topdir, &hostname, backup_number, &share, &filename)
                    .unwrap();
            print_ls(attrs);
        }
        "hosts" => {
            let hosts = hosts::Hosts::list_hosts(&topdir);
            match hosts {
                Ok(hosts) => {
                    for host in hosts {
                        println!("{host}");
                    }
                }
                Err(message) => {
                    println!("{message}");
                }
            }
        }
        "backups" => {
            let backups = hosts::Hosts::list_backups(&topdir, &filename);
            match backups {
                Ok(backups) => {
                    for backup in backups {
                        println!("{}", backup.num);
                    }
                }
                Err(message) => {
                    println!("{message}");
                }
            }
        }
        "mount" => {
            let mountpoint = filename;
            let options = [];

            fuser::mount2(filesystem::BackupPCFS::new(&topdir), mountpoint, &options).unwrap();
        }
        _ => println!("Unknown command: {command}"),
    }
}
