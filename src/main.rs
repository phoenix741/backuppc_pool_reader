use backuppc_pool_reader::attribute_file::{Search, SearchTrait};
use backuppc_pool_reader::compress::BackupPCReader;
use backuppc_pool_reader::decode_attribut::{AttributeFile, FileAttributes, FileType};
use backuppc_pool_reader::filesystem::BackupPCFS;
use backuppc_pool_reader::hosts::{Hosts, HostsTrait};
use backuppc_pool_reader::pool::find_file_in_backuppc;
use backuppc_pool_reader::util::{hex_string_to_vec, vec_to_hex_string};

use clap::{Parser, Subcommand};
use log::info;
use std::env;
use std::{
    fs::File,
    io::{Error, Read, Write},
};

const CHUNK_SIZE: usize = 4 * 65536;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    subcommand: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Cat {
        /// The path to the file to read
        path: String,
        /// host
        #[clap(long)]
        host: Option<String>,
        /// backup number
        #[clap(long)]
        number: Option<u32>,
        /// share name
        #[clap(long)]
        share: Option<String>,
    },

    DecodeAttribute {
        /// The path to the file to read
        path: String,
    },

    Ls {
        /// host
        host: String,
        /// backup number
        number: u32,
        /// share name
        share: String,
        /// The path to the file to read
        path: String,
    },

    Hosts {},

    Backups {
        /// host
        host: String,
    },

    Mount {
        /// The path to the file to read
        path: String,
    },
}

fn attrib_to_stdout<R: Read>(mut reader: &mut R) -> Result<(), Error> {
    let attrib = AttributeFile::read_from(&mut reader).unwrap();
    print_ls(attrib.attributes);
    Ok(())
}

fn reader_to_stdout<R: Read>(reader: &mut R) -> Result<(), Error> {
    let mut buffer = vec![0; CHUNK_SIZE];
    loop {
        let count = reader.read(&mut buffer).unwrap();
        if count == 0 {
            return Ok(());
        }
        std::io::stdout().write_all(&buffer[..count]).unwrap();
    }
}

fn uncompress_to(input_file: &str) -> Result<Box<dyn Read>, Error> {
    let input_file = File::open(input_file)?;
    Ok(Box::new(BackupPCReader::new(input_file)))
}

fn plain_to(input_file: &str) -> Result<Box<dyn Read>, Error> {
    let input_file = File::open(input_file)?;
    Ok(Box::new(std::io::BufReader::new(input_file)))
}

fn pool_file_to_stdout(topdir: &str, file_hash: &str) -> Result<Box<dyn Read>, Error> {
    let md5_hash: Vec<u8> = hex_string_to_vec(file_hash);

    match find_file_in_backuppc(topdir, &md5_hash, None) {
        Ok((file_path, is_compressed)) => {
            if is_compressed {
                uncompress_to(&file_path)
            } else {
                plain_to(&file_path)
            }
        }
        Err(message) => Err(Error::new(
            std::io::ErrorKind::InvalidData,
            message.to_string(),
        )),
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
            "{} {} {: <5} {: <5} {: <10} {: <12} {} {}",
            mode,
            attr.nlinks,
            attr.uid,
            attr.gid,
            attr.size,
            attr.mtime,
            attr.name,
            vec_to_hex_string(&attr.bpc_digest.digest)
        );
    }
}

fn read_file_to_stdout(
    search: &dyn SearchTrait,
    topdir: &str,
    hostname: Option<String>,
    number: Option<u32>,
    share: Option<String>,
    file: &str,
) -> Result<(), Error> {
    if hostname.is_some() || number.is_some() || share.is_some() {
        let Some(hostname) = hostname else {
            return Err(Error::new(
                std::io::ErrorKind::InvalidInput,
                "No host specified",
            ));
        };
        let Some(backup_number) = number else {
            return Err(Error::new(
                std::io::ErrorKind::InvalidInput,
                "No backup number specified",
            ));
        };

        let Some(share) = share else {
            return Err(Error::new(
                std::io::ErrorKind::InvalidInput,
                "No share specified",
            ));
        };

        let attrs = search
            .get_file(&hostname, backup_number, &share, file)
            .unwrap();
        if attrs.len() == 1 {
            if attrs[0].bpc_digest.len > 0 {
                let hex = vec_to_hex_string(&attrs[0].bpc_digest.digest);
                info!("Show file with hash {hex}");
                let mut reader = pool_file_to_stdout(topdir, &hex)?;
                reader_to_stdout(&mut reader)?;
            } else {
                return Err(Error::new(std::io::ErrorKind::InvalidData, "No hash found"));
            }
        } else {
            return Err(Error::new(
                std::io::ErrorKind::InvalidData,
                "Multiple or no files found",
            ));
        }
        return Ok(());
    }

    let file_path = std::path::Path::new(&file);
    let mut reader = if file_path.exists() {
        uncompress_to(file)?
    } else {
        pool_file_to_stdout(topdir, file)?
    };

    reader_to_stdout(&mut reader)
}

pub fn read_file_attribute_to_stdout(topdir: &str, file: &str) -> Result<(), Error> {
    let file_path = std::path::Path::new(&file);
    let mut reader = if file_path.exists() {
        uncompress_to(file)?
    } else {
        pool_file_to_stdout(topdir, file)?
    };

    attrib_to_stdout(&mut reader)
}

fn main() {
    env_logger::init();

    let args = Cli::parse();
    let subcommand = args.subcommand.expect("No subcommand provided");

    let topdir = match env::var("BPC_TOPDIR") {
        Ok(value) => value,
        Err(_) => "/var/lib/backuppc".to_string(),
    };
    let search = Search::new(&topdir);
    let hosts = Hosts::new(&topdir);

    match subcommand {
        Commands::Cat {
            path,
            host,
            number,
            share,
        } => {
            read_file_to_stdout(&search, &topdir, host, number, share, &path).unwrap();
        }
        Commands::DecodeAttribute { path } => {
            read_file_attribute_to_stdout(&topdir, &path).unwrap();
        }
        Commands::Ls {
            host,
            number,
            share,
            path,
        } => {
            let attrs = search
                .list_file_from_dir(&host, number, &share, &path)
                .unwrap();
            print_ls(attrs);
        }
        Commands::Hosts {} => {
            let hosts = hosts.list_hosts();
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
        Commands::Backups { host } => {
            let backups = hosts.list_backups(&host);
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
        Commands::Mount { path } => {
            let options = [];

            fuser::mount2(BackupPCFS::new(&topdir), path, &options).unwrap();
        }
    }
}
