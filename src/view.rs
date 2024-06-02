use log::info;
use lru::LruCache;
use std::collections::HashMap;
use std::fs::File;
/// In this application we have
/// - the host list
/// - the backup list of a host
/// - the list of files of a directory of a share in a backup
/// - the ability to read the content of a file
///
/// The goal of this module is to aggregate all this functionally
///
/// The `BackupView` is able to
/// - merge the list of file list from incremental backups
/// - cache the metadata of the files in case of multiple access
///
use std::io::Read;
use std::num::NonZeroUsize;

use crate::compress::BackupPCReader;
use crate::decode_attribut::{FileAttributes, FileType};

#[cfg(not(test))]
use crate::attribute_file::SearchTrait;
#[cfg(not(test))]
use crate::hosts::HostsTrait;

#[cfg(test)]
use crate::attribute_file::SearchTrait;
#[cfg(test)]
use crate::hosts::HostsTrait;
use crate::pool::find_file_in_backuppc;
use crate::util::{unique, vec_to_hex_string, Result};

// Empty md5 digest (Vec<u8>) : d41d8cd98f00b204e9800998ecf8427e
const EMPTY_MD5_DIGEST: [u8; 16] = [
    0xd4, 0x1d, 0x8c, 0xd9, 0x8f, 0x00, 0xb2, 0x04, 0xe9, 0x80, 0x09, 0x98, 0xec, 0xf8, 0x42, 0x7e,
];

pub struct BackupPC {
    topdir: String,
    hosts: Box<dyn HostsTrait>,
    search: Box<dyn SearchTrait>,
    cache: LruCache<String, Vec<FileAttributes>>,
}

fn sanitize_path(path: &str) -> Vec<&str> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .collect::<Vec<&str>>()
}

const CACHE_SIZE: usize = 1000;

impl BackupPC {
    pub fn new(topdir: &str, hosts: Box<dyn HostsTrait>, search: Box<dyn SearchTrait>) -> Self {
        BackupPC {
            topdir: topdir.to_string(),
            hosts,
            search,
            cache: LruCache::new(NonZeroUsize::new(CACHE_SIZE).unwrap()),
        }
    }

    pub fn new_with_capacity(
        topdir: &str,
        hosts: Box<dyn HostsTrait>,
        search: Box<dyn SearchTrait>,
        capacity: usize,
    ) -> Self {
        BackupPC {
            topdir: topdir.to_string(),
            hosts,
            search,
            cache: LruCache::new(NonZeroUsize::new(capacity).unwrap()),
        }
    }

    fn list_file_from_inode(
        &mut self,
        hostname: &str,
        backup_number: u32,
        inode: u64,
    ) -> Result<Vec<FileAttributes>> {
        let inode_dir = inode >> 17 & 0x7F;
        let inode_file = inode >> 10 & 0x7F;
        let attrib_path = format!("inode/{inode_dir:02x}");
        let attrib_file = format!("attrib{inode_file:02x}_");

        let key = format!("{attrib_path}/{attrib_file}");

        info!("List file from inode {inode} with the key {key}");

        if let Some(cached_result) = self.cache.get(&key) {
            return Ok(cached_result.clone());
        }

        let mut result =
            self.search
                .list_attributes(hostname, backup_number, &attrib_path, &attrib_file)?;

        result.sort_by(|a, b| a.name.cmp(&b.name));
        self.cache.put(key, result.clone());

        Ok(result)
    }

    fn get_inode(
        &mut self,
        hostname: &str,
        backup_number: u32,
        inode: u64,
    ) -> Result<Option<FileAttributes>> {
        let mut inode_vec = inode.to_le_bytes().to_vec();
        if let Some(last_non_zero) = inode_vec.iter().rposition(|&x| x != 0) {
            inode_vec.truncate(last_non_zero + 1);
        }

        let inode_str = vec_to_hex_string(&inode_vec);

        info!("Search inode {inode} with the str form {inode_str}");

        let files = self.list_file_from_inode(hostname, backup_number, inode)?;
        let inode = files.iter().find(|i| i.name == inode_str);

        Ok(inode.cloned())
    }

    fn list_file_from_dir(
        &mut self,
        hostname: &str,
        backup_number: u32,
        share: Option<&str>,
        filename: Option<&str>,
    ) -> Result<Vec<FileAttributes>> {
        info!(
            "List file from dir: {hostname}/{backup_number}/{}/{}",
            share.unwrap_or_default(),
            filename.unwrap_or_default()
        );
        // First search the next oldest filled backup next to the current backup
        let backups_to_search = self.hosts.list_backups_to_fill(hostname, backup_number);

        // Next search the file from the oldest filled backup to the current backup
        let mut files: HashMap<String, FileAttributes> = HashMap::new();
        for backup in backups_to_search {
            info!("Search in backup: {backup}", backup = backup.num);

            let files_from_backup = self
                .search
                .list_file_from_dir(hostname, backup.num, share, filename)?;

            for mut file in files_from_backup {
                if file.type_ == FileType::Deleted {
                    files.remove(&file.name);
                } else {
                    if file.nlinks > 0 {
                        let inode = file.inode;
                        info!(
                            "File {file} has nlinks {nlinks} (inode: {inode})",
                            file = file.name,
                            nlinks = file.nlinks
                        );
                        let inode_file = self.get_inode(hostname, backup.num, inode)?;
                        if let Some(inode_file) = inode_file {
                            file.bpc_digest = inode_file.bpc_digest.clone();
                        }
                    }

                    files.insert(file.name.clone(), file);
                }
            }
        }

        Ok(files.values().cloned().collect())
    }

    fn list_shares_of(
        &mut self,
        hostname: &str,
        backup_number: u32,
        path: &[&str],
    ) -> Result<(Vec<String>, Option<String>, usize)> {
        info!(
            "List shares of: {hostname}/{backup_number}/{path}",
            path = path.join("/")
        );
        let shares = self.list_file_from_dir(hostname, backup_number, None, None)?;
        let mut shares = shares.iter().map(|share| &share.name).collect::<Vec<_>>();

        let mut selected_share: Option<String> = None;
        let mut share_size = 0;

        // Ensure that shares are sorted by length (longest last) to ensure that the selected share is the share that
        // is the most specific
        shares.sort_by_key(|a| a.len());

        // Filter the shares that are not in the path
        let shares: Vec<String> = shares
            .into_iter()
            .filter_map(|share| {
                let share_array = sanitize_path(share);

                if path.starts_with(&share_array) || path.eq(&share_array) {
                    share_size = share_array.len();
                    selected_share = Some(share.clone());
                    None
                } else if share_array.starts_with(path) {
                    Some(share_array[path.len()..][0].to_string())
                } else {
                    None
                }
            })
            .collect();

        let shares = unique(shares);

        Ok((shares, selected_share, share_size))
    }

    pub fn direct_list(&mut self, path: &[&str]) -> Result<Vec<FileAttributes>> {
        info!("List: {path}", path = path.join("/"));
        match path.len() {
            0 => {
                let hosts = self.hosts.list_hosts()?;
                Ok(hosts.into_iter().map(FileAttributes::from_host).collect())
            }
            1 => {
                let backups = self.hosts.list_backups(path[0]);
                match backups {
                    Ok(backups) => Ok(backups
                        .into_iter()
                        .map(|a| FileAttributes::from_backup(&a))
                        .collect()),
                    Err(err) => {
                        // If the file isn't found, it's because we should return empty vec
                        if let Some(io_err) = err.downcast_ref::<std::io::Error>() {
                            if io_err.kind() == std::io::ErrorKind::NotFound {
                                Ok(Vec::new())
                            } else {
                                Err(err)
                            }
                        } else {
                            Err(err)
                        }
                    }
                }
            }
            _ => {
                let (shares, selected_share, share_size) =
                    self.list_shares_of(path[0], path[1].parse::<u32>().unwrap_or(0), &path[2..])?;

                let shares = shares.into_iter().map(FileAttributes::from_share).collect();

                match selected_share {
                    None => Ok(shares),
                    Some(selected_share) => {
                        let files = self.list_file_from_dir(
                            path[0],
                            path[1].parse::<u32>().unwrap_or(0),
                            Some(&selected_share),
                            Some(&path[(2 + share_size)..].join("/")),
                        )?;

                        // Add detected shares to files
                        let mut files = files
                            .into_iter()
                            .chain(shares)
                            .collect::<Vec<FileAttributes>>();

                        files.sort_by(|a, b| a.name.cmp(&b.name));

                        Ok(files)
                    }
                }
            }
        }
    }

    pub fn list(&mut self, path: &[&str]) -> Result<Vec<FileAttributes>> {
        let key = path
            .iter()
            .filter(|s| !s.is_empty())
            .map(std::string::ToString::to_string)
            .collect::<Vec<String>>()
            .join("/");

        if let Some(cached_result) = self.cache.get(&key) {
            return Ok(cached_result.clone());
        }

        let mut result = self.direct_list(path)?;
        result.sort_by(|a, b| a.name.cmp(&b.name));
        self.cache.put(key, result.clone());

        Ok(result)
    }

    pub fn read_file(&mut self, path: &[&str]) -> Result<Box<dyn Read + Sync + Send>> {
        info!("Read file: {path}", path = path.join("/"));
        let filename = path.last().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to get filename: {}", path.join("/")),
            )
        })?;
        let path = &path[..path.len() - 1];

        let attributes = self.list(path)?;

        let file = attributes
            .into_iter()
            .find(|f| f.name.eq(*filename))
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("File not found (not in attributs): {}", path.join("/")),
                )
            })?;

        if file.bpc_digest.len > 2 && file.bpc_digest.digest.ne(&EMPTY_MD5_DIGEST) {
            let md5_hash = file.bpc_digest.digest;
            match find_file_in_backuppc(&self.topdir, &md5_hash, None) {
                Ok((file_path, is_compressed)) => {
                    if is_compressed {
                        let input_file = File::open(file_path)?;
                        Ok(Box::new(BackupPCReader::new(input_file)))
                    } else {
                        let input_file = File::open(file_path)?;
                        Ok(Box::new(std::io::BufReader::new(input_file)))
                    }
                }
                Err(message) => Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    message.to_string(),
                )
                .into()),
            }
        } else {
            Ok(Box::new(std::io::empty()))
        }
    }
}

//
// Test of the BackupPCView
//
// Test of the method list
//
// Methods fril HostsTrait should be mocked

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribute_file::MockSearchTrait;
    use crate::decode_attribut::FileType;
    use crate::hosts::{BackupInformation, MockHostsTrait};
    use mockall::predicate::*;

    // Befor each test we create all the mock of the view with the following structure
    // /var/lib/backuppc
    // ├── pc-1
    // │   ├── 1
    // │   │   ├── volume1
    // │   │   │   ├── test
    // │   │   │   │   ├── supertest
    // │   │   │   │   │   ├── de
    // │   │   │   │   │   │   ├── test
    // │   │   │   │   │   │   │   ├── file1
    // │   │   │   │   │   │   │   ├── file2
    // │   │   │   │   │   │   │   ├── file3
    // │   │   │   │   │   │   ├── en
    // │   │   │   │   │   │   ├── es
    // │   │   │   │   │   │   └── fr
    // │   │   │   │   │   └── test2
    // │   │   │   │   └── toto
    // │   │   │   └── test2
    // │   └── 2
    // │   │   ├── volume1
    // │   │   │   ├── test
    // │   │   │   └── test2
    // │   │   └── volume2
    // └── pc-2
    // │   ├── 1
    // │   ├── 2
    // │   └── 3
    // └── pc-3
    fn create_mock_backup(num: u32) -> BackupInformation {
        BackupInformation {
            num,
            backup_type: "full".to_string(),
            start_time: 0,
            end_time: 0,
            n_files: 0,
            size: 0,
            n_files_exist: 0,
            size_exist: 0,
            n_files_new: 0,
            size_new: 0,
            xfer_errs: 0,
            xfer_bad_file: 0,
            xfer_bad_share: 0,
            tar_errs: 0,
            compress: 0,
            size_exist_comp: 0,
            size_new_comp: 0,
            no_fill: 0,
            fill_from_num: 0,
            mangle: 0,
            xfer_method: "rsync".to_string(),
            level: 0,
            charset: "utf-8".to_string(),
            version: "4.0.0".to_string(),
            inode_last: 0,
        }
    }

    fn create_file_attributes(name: &str, type_: FileType) -> FileAttributes {
        FileAttributes {
            name: name.to_string(),
            type_,
            compress: 0,

            mode: 0,
            uid: 0,
            gid: 0,
            nlinks: 0,

            mtime: 0,
            size: 0,
            inode: 0,

            bpc_digest: crate::decode_attribut::BpcDigest {
                len: 0,
                digest: Vec::new(),
            },
            xattr_num_entries: 0,
            xattrs: Vec::new(),
        }
    }

    fn create_view() -> BackupPC {
        let topdir = "/var/lib/backuppc";
        let mut hosts_mock = Box::new(MockHostsTrait::new());
        let mut search_mock = Box::new(MockSearchTrait::new());

        let hosts = vec!["pc-1".to_string(), "pc-2".to_string(), "pc-3".to_string()];

        let backups_pc1 = vec![create_mock_backup(1), create_mock_backup(2)];

        let backups_pc2 = vec![
            create_mock_backup(1),
            create_mock_backup(2),
            create_mock_backup(3),
        ];

        let backups_pc3 = Vec::<BackupInformation>::new();

        hosts_mock
            .expect_list_hosts()
            .returning(move || Ok(hosts.clone()));

        hosts_mock
            .expect_list_backups()
            .with(eq("pc-1"))
            .returning(move |_| Ok(backups_pc1.clone()));

        hosts_mock
            .expect_list_backups()
            .with(eq("pc-2"))
            .returning(move |_| Ok(backups_pc2.clone()));

        hosts_mock
            .expect_list_backups()
            .with(eq("pc-3"))
            .returning(move |_| Ok(backups_pc3.clone()));

        hosts_mock
            .expect_list_backups_to_fill()
            .with(eq("pc-1"), eq(1))
            .returning(|_, _| vec![create_mock_backup(1)]);

        search_mock
            .expect_list_file_from_dir()
            .withf(|hostname, backup_number, share, path| {
                hostname == "pc-1" && backup_number == &1 && share.is_none() && path.is_none()
            })
            .returning(move |_, _, _, _| {
                Ok(vec![
                    create_file_attributes("/home", FileType::Dir),
                    create_file_attributes("/volume1/test", FileType::Dir),
                    create_file_attributes("/volume1/test2", FileType::Dir),
                ])
            });

        search_mock
            .expect_list_file_from_dir()
            .withf(|hostname, backup_number, share, path| {
                hostname == "pc-1" && backup_number == &2 && share.is_none() && path.is_none()
            })
            .returning(move |_, _, _, _| {
                Ok(vec![
                    create_file_attributes("/volume1/test", FileType::Dir),
                    create_file_attributes("/volume1/test2", FileType::Dir),
                    create_file_attributes("/volume2", FileType::Dir),
                ])
            });

        search_mock
            .expect_list_file_from_dir()
            .withf(|hostname, backup_number, share, path| {
                hostname == "pc-1"
                    && backup_number == &1
                    && share.is_some_and(|share| share == "/volume1/test")
                    && path.is_some_and(|path| path.is_empty())
            })
            .returning(move |_, _, _, _| {
                Ok(vec![
                    create_file_attributes("supertest", FileType::Dir),
                    create_file_attributes("toto", FileType::Dir),
                ])
            });

        search_mock
            .expect_list_file_from_dir()
            .withf(|hostname, backup_number, share, path| {
                hostname == "pc-1"
                    && backup_number == &1
                    && share.is_some_and(|share| share == "/volume1/test")
                    && path.is_some_and(|path| path == "supertest")
            })
            .returning(move |_, _, _, _| {
                Ok(vec![
                    create_file_attributes("de", FileType::Dir),
                    create_file_attributes("test2", FileType::Dir),
                ])
            });

        search_mock
            .expect_list_file_from_dir()
            .withf(|hostname, backup_number, share, path| {
                hostname == "pc-1"
                    && backup_number == &1
                    && share.is_some_and(|share| share == "/volume1/test")
                    && path.is_some_and(|path| path == "supertest/de")
            })
            .returning(move |_, _, _, _| {
                Ok(vec![
                    create_file_attributes("test", FileType::Dir),
                    create_file_attributes("en", FileType::Dir),
                    create_file_attributes("es", FileType::Dir),
                    create_file_attributes("fr", FileType::Dir),
                ])
            });

        search_mock
            .expect_list_file_from_dir()
            .withf(|hostname, backup_number, share, path| {
                hostname == "pc-1"
                    && backup_number == &1
                    && share.is_some_and(|share| share == "/volume1/test")
                    && path.is_some_and(|path| path == "supertest/de/test")
            })
            .returning(move |_, _, _, _| {
                Ok(vec![
                    create_file_attributes("file1", FileType::File),
                    create_file_attributes("file2", FileType::File),
                    create_file_attributes("file3", FileType::File),
                ])
            });

        BackupPC::new(topdir, hosts_mock, search_mock)
    }

    #[test]
    fn test_list_host_empty() {
        let mut view = create_view();

        let result = view.list(&[]);
        assert!(result.is_ok());

        let mut result = result.unwrap();
        result.sort_by(|a, b| a.name.cmp(&b.name));

        assert_eq!(result.len(), 3);
        assert_eq!(result[0], create_file_attributes("pc-1", FileType::Dir));
        assert_eq!(result[1], create_file_attributes("pc-2", FileType::Dir));
        assert_eq!(result[2], create_file_attributes("pc-3", FileType::Dir));
    }

    #[test]
    fn test_list_host_pc1() {
        let mut view = create_view();

        let result = view.list(&["pc-1"]);
        assert!(result.is_ok());

        let mut result = result.unwrap();
        result.sort_by(|a, b| a.name.cmp(&b.name));

        println!("{:?}", result);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], create_file_attributes("1", FileType::Dir));
        assert_eq!(result[1], create_file_attributes("2", FileType::Dir));
    }

    #[test]
    fn test_list_host_pc1_backup1() {
        let mut view = create_view();

        let result = view.list(&["pc-1", "1"]);
        assert!(result.is_ok());

        let mut result = result.unwrap();
        result.sort_by(|a, b| a.name.cmp(&b.name));

        println!("{:?}", result);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], create_file_attributes("home", FileType::Dir));
        assert_eq!(result[1], create_file_attributes("volume1", FileType::Dir));
    }

    #[test]
    fn test_list_host_pc1_backup1_volume1() {
        let mut view = create_view();

        let result = view.list(&["pc-1", "1", "volume1"]);
        assert!(result.is_ok());

        let mut result = result.unwrap();
        result.sort_by(|a, b| a.name.cmp(&b.name));

        println!("{:?}", result);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], create_file_attributes("test", FileType::Dir));
        assert_eq!(result[1], create_file_attributes("test2", FileType::Dir));
    }

    #[test]
    fn test_list_host_pc1_backup1_volume1_test() {
        let mut view = create_view();

        let result = view.list(&["pc-1", "1", "volume1", "test"]);
        assert!(result.is_ok());

        let mut result = result.unwrap();
        result.sort_by(|a, b| a.name.cmp(&b.name));

        println!("{:?}", result);
        assert_eq!(result.len(), 2);
        assert_eq!(
            result[0],
            create_file_attributes("supertest", FileType::Dir)
        );
        assert_eq!(result[1], create_file_attributes("toto", FileType::Dir));
    }

    #[test]
    fn test_list_host_pc1_backup1_volume1_test_supertest() {
        let mut view = create_view();

        let result = view.list(&["pc-1", "1", "volume1", "test", "supertest"]);
        assert!(result.is_ok());

        let mut result = result.unwrap();
        result.sort_by(|a, b| a.name.cmp(&b.name));

        println!("{:?}", result);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], create_file_attributes("de", FileType::Dir));
        assert_eq!(result[1], create_file_attributes("test2", FileType::Dir));
    }

    #[test]
    fn test_list_host_pc1_backup1_volume1_test_supertest_de() {
        let mut view = create_view();

        let result = view.list(&["pc-1", "1", "volume1", "test", "supertest", "de"]);
        assert!(result.is_ok());

        let mut result = result.unwrap();
        result.sort_by(|a, b| a.name.cmp(&b.name));

        println!("{:?}", result);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], create_file_attributes("en", FileType::Dir));
        assert_eq!(result[1], create_file_attributes("es", FileType::Dir));
        assert_eq!(result[2], create_file_attributes("fr", FileType::Dir));
        assert_eq!(result[3], create_file_attributes("test", FileType::Dir));
    }

    #[test]
    fn test_list_host_pc1_backup1_volume1_test_supertest_de_test() {
        let mut view = create_view();

        let result = view.list(&["pc-1", "1", "volume1", "test", "supertest", "de", "test"]);
        assert!(result.is_ok());

        let mut result = result.unwrap();
        result.sort_by(|a, b| a.name.cmp(&b.name));

        println!("{:?}", result);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], create_file_attributes("file1", FileType::File));
        assert_eq!(result[1], create_file_attributes("file2", FileType::File));
        assert_eq!(result[2], create_file_attributes("file3", FileType::File));
    }
}
