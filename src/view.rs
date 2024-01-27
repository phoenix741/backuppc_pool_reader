use std::fs::File;
/// In this application we have
/// - the host list
/// - the backup list of a host
/// - the list of files of a directory of a share in a backup
/// - the ability to read the content of a file
///
/// The goal of this module is to aggregate all this functionnaly
///
/// The BackupView is able to
/// - merge the list of file list from incremental backups
/// - cache the metadata of the files in case of multiple access
///
use std::io::Read;

use crate::compress::BackupPCReader;
use crate::decode_attribut::FileAttributes;

#[cfg(not(test))]
use crate::attribute_file::{AttributeFileSearch, AttributeFileSearchTrait};
#[cfg(not(test))]
use crate::hosts::{Hosts, HostsTrait};

#[cfg(test)]
use crate::attribute_file::{
    AttributeFileSearchTrait, MockAttributeFileSearchTrait as AttributeFileSearch,
};
#[cfg(test)]
use crate::hosts::{HostsTrait, MockHostsTrait as Hosts};
use crate::pool::find_file_in_backuppc;
use crate::util::{unique, Result};

// Empty md5 digest (Vec<u8>) : d41d8cd98f00b204e9800998ecf8427e
const EMPTY_MD5_DIGEST: [u8; 16] = [
    0xd4, 0x1d, 0x8c, 0xd9, 0x8f, 0x00, 0xb2, 0x04, 0xe9, 0x80, 0x09, 0x98, 0xec, 0xf8, 0x42, 0x7e,
];

pub struct BackupPCView {
    topdir: String,
}

fn sanitize_path(path: &str) -> Vec<&str> {
    path.split("/")
        .filter(|s| !s.is_empty())
        .collect::<Vec<&str>>()
}

impl BackupPCView {
    pub fn new(topdir: &str) -> Self {
        BackupPCView {
            topdir: topdir.to_string(),
        }
    }

    fn list_shares_of(
        &self,
        hostname: &str,
        backup_number: u32,
        path: &[&str],
    ) -> Result<(Vec<String>, Option<String>, usize)> {
        let shares = Hosts::list_shares(&self.topdir, hostname, backup_number)?;

        let mut selected_share: Option<String> = None;
        let mut share_size = 0;

        // Filter the shares that are not in the path
        let shares: Vec<String> = shares
            .into_iter()
            .filter_map(|share| {
                let share_array = sanitize_path(&share);
                if path.starts_with(&share_array) {
                    selected_share = Some(share.clone());
                    share_size = share_array.len();
                    None
                } else if path.eq(&share_array) {
                    selected_share = Some(share.clone());
                    share_size = share_array.len();
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

    pub fn list(&self, path: &[&str]) -> Result<Vec<FileAttributes>> {
        match path.len() {
            0 => {
                let hosts = Hosts::list_hosts(&self.topdir)?;
                Ok(hosts
                    .into_iter()
                    .map(|h| FileAttributes::from_host(h))
                    .collect())
            }
            1 => {
                let backups = Hosts::list_backups(&self.topdir, path[0]);
                match backups {
                    Ok(backups) => Ok(backups
                        .into_iter()
                        .map(|backup| FileAttributes::from_backup(backup))
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

                let shares = shares
                    .into_iter()
                    .map(|s| FileAttributes::from_share(s))
                    .collect();

                match selected_share {
                    None => Ok(shares),
                    Some(selected_share) => AttributeFileSearch::list_file_from_dir(
                        &self.topdir,
                        path[0],
                        path[1].parse::<u32>().unwrap_or(0),
                        &selected_share,
                        &path[(2 + share_size)..].join("/"),
                    )
                    .map(|files| files.into_iter().map(|file| file).collect()),
                }
            }
        }
    }

    pub fn read_file(&self, path: &[&str]) -> Result<Box<dyn Read>> {
        let filename = path.last().ok_or(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to get filename: {}", path.join("/")),
        ))?;
        let path = &path[..path.len() - 1];

        let attributes = self.list(path)?;

        let file = attributes
            .into_iter()
            .find(|f| f.name.eq(*filename))
            .ok_or(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("File not found (not in attributs): {}", path.join("/")),
            ))?;

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
    use crate::attribute_file::MockAttributeFileSearchTrait;
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
            num: num,
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
            type_: type_,
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

    fn create_view() -> (
        BackupPCView,
        crate::hosts::__mock_MockHostsTrait_HostsTrait::__list_hosts::Context,
        crate::hosts::__mock_MockHostsTrait_HostsTrait::__list_backups::Context,
        crate::hosts::__mock_MockHostsTrait_HostsTrait::__list_backups::Context,
        crate::hosts::__mock_MockHostsTrait_HostsTrait::__list_backups::Context,
        crate::hosts::__mock_MockHostsTrait_HostsTrait::__list_shares::Context,
        crate::hosts::__mock_MockHostsTrait_HostsTrait::__list_shares::Context,
        crate::attribute_file::__mock_MockAttributeFileSearchTrait_AttributeFileSearchTrait::__list_file_from_dir::Context,
        crate::attribute_file::__mock_MockAttributeFileSearchTrait_AttributeFileSearchTrait::__list_file_from_dir::Context,
        crate::attribute_file::__mock_MockAttributeFileSearchTrait_AttributeFileSearchTrait::__list_file_from_dir::Context,
        crate::attribute_file::__mock_MockAttributeFileSearchTrait_AttributeFileSearchTrait::__list_file_from_dir::Context,
    ){
        let topdir = "/var/lib/backuppc";
        let view = BackupPCView::new(topdir);

        let mut hosts = Vec::new();
        hosts.push("pc-1".to_string());
        hosts.push("pc-2".to_string());
        hosts.push("pc-3".to_string());

        let mut backups_pc1 = Vec::<BackupInformation>::new();
        backups_pc1.push(create_mock_backup(1));
        backups_pc1.push(create_mock_backup(2));

        let mut backups_pc2 = Vec::<BackupInformation>::new();
        backups_pc2.push(create_mock_backup(1));
        backups_pc2.push(create_mock_backup(2));
        backups_pc2.push(create_mock_backup(3));

        let backups_pc3 = Vec::<BackupInformation>::new();

        let mut shares_pc1_backup1 = Vec::new();
        shares_pc1_backup1.push("/home".to_string());
        shares_pc1_backup1.push("/volume1/test".to_string());
        shares_pc1_backup1.push("/volume1/test2".to_string());

        let mut shares_pc1_backup2 = Vec::new();
        shares_pc1_backup2.push("/volume1/test".to_string());
        shares_pc1_backup2.push("/volume1/test2".to_string());
        shares_pc1_backup2.push("/volume2".to_string());

        let list_hosts_ctx = MockHostsTrait::list_hosts_context();
        list_hosts_ctx
            .expect()
            .returning(move |_| Ok(hosts.clone()));

        let list_backups_pc1_ctx = MockHostsTrait::list_backups_context();
        list_backups_pc1_ctx
            .expect()
            .with(eq(topdir), eq("pc-1"))
            .returning(move |_, _| Ok(backups_pc1.clone()));

        let list_backups_pc2_ctx = MockHostsTrait::list_backups_context();
        list_backups_pc2_ctx
            .expect()
            .with(eq(topdir), eq("pc-2"))
            .returning(move |_, _| Ok(backups_pc2.clone()));

        let list_backups_pc3_ctx = MockHostsTrait::list_backups_context();
        list_backups_pc3_ctx
            .expect()
            .with(eq(topdir), eq("pc-3"))
            .returning(move |_, _| Ok(backups_pc3.clone()));

        let list_shares_pc1_backup1_ctx = MockHostsTrait::list_shares_context();
        list_shares_pc1_backup1_ctx
            .expect()
            .with(eq(topdir), eq("pc-1"), eq(1))
            .returning(move |_, _, _| Ok(shares_pc1_backup1.clone()));

        let list_shares_pc1_backup2_ctx = MockHostsTrait::list_shares_context();
        list_shares_pc1_backup2_ctx
            .expect()
            .with(eq(topdir), eq("pc-1"), eq(2))
            .returning(move |_, _, _| Ok(shares_pc1_backup2.clone()));

        let list_file_pc1_backup1_volume1_test_ctx =
            MockAttributeFileSearchTrait::list_file_from_dir_context();
        list_file_pc1_backup1_volume1_test_ctx
            .expect()
            .with(eq(topdir), eq("pc-1"), eq(1), eq("/volume1/test"), eq(""))
            .returning(move |_, _, _, _, _| {
                Ok(vec![
                    create_file_attributes("supertest", FileType::Dir),
                    create_file_attributes("toto", FileType::Dir),
                ])
            });

        let list_file_pc1_backup1_volume1_test_supertest_ctx =
            MockAttributeFileSearchTrait::list_file_from_dir_context();
        list_file_pc1_backup1_volume1_test_supertest_ctx
            .expect()
            .with(
                eq(topdir),
                eq("pc-1"),
                eq(1),
                eq("/volume1/test"),
                eq("supertest"),
            )
            .returning(move |_, _, _, _, _| {
                Ok(vec![
                    create_file_attributes("de", FileType::Dir),
                    create_file_attributes("test2", FileType::Dir),
                ])
            });

        let list_file_pc1_backup1_volume1_test_supertest_de_ctx =
            MockAttributeFileSearchTrait::list_file_from_dir_context();
        list_file_pc1_backup1_volume1_test_supertest_de_ctx
            .expect()
            .with(
                eq(topdir),
                eq("pc-1"),
                eq(1),
                eq("/volume1/test"),
                eq("supertest/de"),
            )
            .returning(move |_, _, _, _, _| {
                Ok(vec![
                    create_file_attributes("test", FileType::Dir),
                    create_file_attributes("en", FileType::Dir),
                    create_file_attributes("es", FileType::Dir),
                    create_file_attributes("fr", FileType::Dir),
                ])
            });

        let list_file_pc1_backup1_volume1_test_supertest_de_test_ctx =
            MockAttributeFileSearchTrait::list_file_from_dir_context();
        list_file_pc1_backup1_volume1_test_supertest_de_test_ctx
            .expect()
            .with(
                eq(topdir),
                eq("pc-1"),
                eq(1),
                eq("/volume1/test"),
                eq("supertest/de/test"),
            )
            .returning(move |_, _, _, _, _| {
                Ok(vec![
                    create_file_attributes("file1", FileType::File),
                    create_file_attributes("file2", FileType::File),
                    create_file_attributes("file3", FileType::File),
                ])
            });

        (
            view,
            list_hosts_ctx,
            list_backups_pc1_ctx,
            list_backups_pc2_ctx,
            list_backups_pc3_ctx,
            list_shares_pc1_backup1_ctx,
            list_shares_pc1_backup2_ctx,
            list_file_pc1_backup1_volume1_test_ctx,
            list_file_pc1_backup1_volume1_test_supertest_ctx,
            list_file_pc1_backup1_volume1_test_supertest_de_ctx,
            list_file_pc1_backup1_volume1_test_supertest_de_test_ctx,
        )
    }

    #[test]
    fn test_list_host_empty() {
        let mocks = create_view();
        let view = mocks.0;

        let result = view.list(&[]);
        assert_eq!(result.is_ok(), true);

        let result = result.unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], create_file_attributes("pc-1", FileType::Dir));
        assert_eq!(result[1], create_file_attributes("pc-2", FileType::Dir));
        assert_eq!(result[2], create_file_attributes("pc-3", FileType::Dir));
    }

    #[test]
    fn test_list_host_pc1() {
        let mocks = create_view();
        let view = mocks.0;

        let result = view.list(&["pc-1"]);
        assert_eq!(result.is_ok(), true);

        let result = result.unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], create_file_attributes("1", FileType::Dir));
        assert_eq!(result[1], create_file_attributes("2", FileType::Dir));
    }

    #[test]
    fn test_list_host_pc1_backup1() {
        let mocks = create_view();
        let view = mocks.0;

        let result = view.list(&["pc-1", "1"]);
        assert_eq!(result.is_ok(), true);

        let result = result.unwrap();
        println!("{:?}", result);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], create_file_attributes("home", FileType::Dir));
        assert_eq!(result[1], create_file_attributes("volume1", FileType::Dir));
    }

    #[test]
    fn test_list_host_pc1_backup1_volume1() {
        let mocks = create_view();
        let view = mocks.0;

        let result = view.list(&["pc-1", "1", "volume1"]);
        assert_eq!(result.is_ok(), true);

        let result = result.unwrap();
        println!("{:?}", result);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], create_file_attributes("test", FileType::Dir));
        assert_eq!(result[1], create_file_attributes("test2", FileType::Dir));
    }

    #[test]
    fn test_list_host_pc1_backup1_volume1_test() {
        let mocks = create_view();
        let view = mocks.0;

        let result = view.list(&["pc-1", "1", "volume1", "test"]);
        assert_eq!(result.is_ok(), true);

        let result = result.unwrap();
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
        let mocks = create_view();
        let view = mocks.0;

        let result = view.list(&["pc-1", "1", "volume1", "test", "supertest"]);
        assert_eq!(result.is_ok(), true);

        let result = result.unwrap();
        println!("{:?}", result);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], create_file_attributes("de", FileType::Dir));
        assert_eq!(result[1], create_file_attributes("test2", FileType::Dir));
    }

    #[test]
    fn test_list_host_pc1_backup1_volume1_test_supertest_de() {
        let mocks = create_view();
        let view = mocks.0;

        let result = view.list(&["pc-1", "1", "volume1", "test", "supertest", "de"]);
        assert_eq!(result.is_ok(), true);

        let result = result.unwrap();
        println!("{:?}", result);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], create_file_attributes("test", FileType::Dir));
        assert_eq!(result[1], create_file_attributes("en", FileType::Dir));
        assert_eq!(result[2], create_file_attributes("es", FileType::Dir));
        assert_eq!(result[3], create_file_attributes("fr", FileType::Dir));
    }

    #[test]
    fn test_list_host_pc1_backup1_volume1_test_supertest_de_test() {
        let mocks = create_view();
        let view = mocks.0;

        let result = view.list(&["pc-1", "1", "volume1", "test", "supertest", "de", "test"]);
        assert_eq!(result.is_ok(), true);

        let result = result.unwrap();
        println!("{:?}", result);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], create_file_attributes("file1", FileType::File));
        assert_eq!(result[1], create_file_attributes("file2", FileType::File));
        assert_eq!(result[2], create_file_attributes("file3", FileType::File));
    }
}
