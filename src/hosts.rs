use log::{debug, info};
#[cfg(test)]
use mockall::{automock, predicate::*};

use crate::util::Result;

/// This module is used to list all available hosts in the backuppc pool
///
/// The `list_hosts` function is used to list all available hosts in the backuppc pool
///
/// The list of host can be found by loading all folders in the topdir/pc directory.
///
use std::{
    fs::File,
    io::{BufRead, BufReader},
};

///
/// Read all the backup numbers for the hosts.
///
/// Backups are store in the file topdir/pc/<hostname>/backups file.
///
/// The fields are (num type startTime endTime nFiles size nFilesExist sizeExist nFilesNew sizeNew xferErrs xferBadFile xferBadShare tarErrs compress sizeExistComp sizeNewComp noFill fillFromNum mangle xferMethod level charset version inodeLast):
/// - backup number
/// - type of backup (full, incr, etc)
/// - start time
/// - end time
/// - number of files
/// - number of bytes
/// - number of existing files
/// - number of bytes in existing files
/// - number of new files
/// - number of bytes in new files
/// - number of transfert error
/// - number of bad files
/// - number of bad share
/// - number of tar errors
/// - compression level
/// - size of existing files after compression
/// - size of new files after compression
/// - 0 if the backup is full or 1 if the backup is incremental
/// - The number of the backup from which the incremental backup is made
/// - mangle ??
/// - how the backup was made (rsync, tar, etc)
/// - the backup level (incremental by previous backup)
/// - charset used to make the backup
/// - version used to make the backup
/// - inode of the last file

#[derive(Debug, Clone)]
pub struct BackupInformation {
    pub num: u32,
    pub backup_type: String,
    pub start_time: u64,
    pub end_time: u64,
    pub n_files: u32,
    pub size: u64,
    pub n_files_exist: u32,
    pub size_exist: u64,
    pub n_files_new: u32,
    pub size_new: u64,
    pub xfer_errs: u32,
    pub xfer_bad_file: u32,
    pub xfer_bad_share: u32,
    pub tar_errs: u32,
    pub compress: u32,
    pub size_exist_comp: u64,
    pub size_new_comp: u64,
    pub no_fill: u32,
    pub fill_from_num: i32,
    pub mangle: u64,
    pub xfer_method: String,
    pub level: u32,
    pub charset: String,
    pub version: String,
    pub inode_last: u64,
}

#[cfg_attr(test, automock)]
pub trait HostsTrait: Send + Sync {
    fn list_hosts(&self) -> Result<Vec<String>>;
    fn list_backups(&self, hostname: &str) -> Result<Vec<BackupInformation>>;
    fn list_backups_to_fill(&self, hostname: &str, backup_number: u32) -> Vec<BackupInformation>;
}

pub struct Hosts {
    topdir: String,
}

impl Hosts {
    pub fn new(topdir: &str) -> Self {
        Hosts {
            topdir: topdir.to_string(),
        }
    }
}

// Implements trait
impl HostsTrait for Hosts {
    fn list_hosts(&self) -> Result<Vec<String>> {
        info!("Listing hosts in {}", self.topdir);
        let pc_dir = std::path::Path::new(&self.topdir).join("pc");
        let mut hosts = Vec::new();

        for entry in std::fs::read_dir(pc_dir)? {
            match entry {
                Ok(entry) => {
                    let path = entry.path();
                    if path.is_dir() {
                        let host = path
                            .file_name()
                            .unwrap_or_default()
                            .to_str()
                            .unwrap_or_default()
                            .to_string();

                        hosts.push(host);
                    }
                }
                Err(err) => {
                    eprintln!("Error reading pc directory: {err}");
                }
            }
        }

        debug!("Found {} hosts", hosts.len());

        Ok(hosts)
    }

    ///
    /// List all the backups for a given host (used the format separed by tab).
    ///
    /// The backups are stored in the file topdir/pc/<hostname>/backups.
    ///
    fn list_backups(&self, hostname: &str) -> Result<Vec<BackupInformation>> {
        info!("Listing backups for {hostname}");

        let mut backups = Vec::new();
        let path = format!("{}/pc/{hostname}/backups", &self.topdir);

        // Open the file and read each line
        // Fields are separated by tab

        let file = File::open(path)?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line.unwrap();
            let fields: Vec<&str> = line.split('\t').collect();
            let backup = BackupInformation {
                num: fields[0].parse().unwrap_or_default(),
                backup_type: fields[1].to_string(),
                start_time: fields[2].parse().unwrap_or_default(),
                end_time: fields[3].parse().unwrap_or_default(),
                n_files: fields[4].parse().unwrap_or_default(),
                size: fields[5].parse().unwrap_or_default(),
                n_files_exist: fields[6].parse().unwrap_or_default(),
                size_exist: fields[7].parse().unwrap_or_default(),
                n_files_new: fields[8].parse().unwrap_or_default(),
                size_new: fields[9].parse().unwrap_or_default(),
                xfer_errs: fields[10].parse().unwrap_or_default(),
                xfer_bad_file: fields[11].parse().unwrap_or_default(),
                xfer_bad_share: fields[12].parse().unwrap_or_default(),
                tar_errs: fields[13].parse().unwrap_or_default(),
                compress: fields[14].parse().unwrap_or_default(),
                size_exist_comp: fields[15].parse().unwrap_or_default(),
                size_new_comp: fields[16].parse().unwrap_or_default(),
                no_fill: fields[17].parse().unwrap_or_default(),
                fill_from_num: fields[18].parse().unwrap_or(-1),
                mangle: fields[19].parse().unwrap_or_default(),
                xfer_method: fields[20].to_string(),
                level: fields[21].parse().unwrap_or_default(),
                charset: fields[22].to_string(),
                version: fields[23].to_string(),
                inode_last: fields[24].parse().unwrap_or_default(),
            };

            backups.push(backup);
        }

        debug!("Found {} backups", backups.len());

        Ok(backups)
    }

    /// List all the backups until the filled backup for a given backup.
    ///
    /// Used to complete the missing backup
    fn list_backups_to_fill(&self, hostname: &str, backup_number: u32) -> Vec<BackupInformation> {
        let backups = self.list_backups(hostname).unwrap_or_else(|_| Vec::new());
        let backups = backups.iter().filter(|backup| backup.num >= backup_number);
        let mut backups_to_search: Vec<crate::hosts::BackupInformation> = Vec::new();

        for backup in backups {
            backups_to_search.push(backup.clone());

            if backup.no_fill > 0 {
                continue;
            }
            break;
        }
        backups_to_search.reverse();

        backups_to_search
    }
}
