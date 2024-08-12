use log::info;
#[cfg(test)]
use mockall::{automock, predicate::*};

use std::{
    cmp::Ordering,
    fs::File,
    path::{Path, PathBuf},
};

use crate::{
    compress::BackupPCReader,
    decode_attribut::{AttributeFile, FileAttributes},
    pool::find_file_in_backuppc,
    util::{hex_string_to_vec, mangle, mangle_filename, vec_to_osstr, Result},
};

#[cfg_attr(test, automock)]
pub trait SearchTrait: Send + Sync {
    /// Read the attributes from a file
    ///
    /// If the file is compressed, uncompress it with special `BackupPCReader` and read the attributes.
    ///
    /// # Arguments
    ///
    /// * `file` - The path to the file to read.
    /// * `is_compressed` - A boolean to know if the file is compressed.
    ///
    /// # Returns
    ///
    /// A vector of `FileAttributes` containing the list of attributes.
    ///
    /// # Errors
    ///
    /// If the file cannot be read or uncompressed.
    fn read_attrib(&self, file: &Path, is_compressed: bool) -> Result<Vec<FileAttributes>>;
    /// List the attributes for a complete path
    ///
    /// The method will define the attrib path depending on the share and filename.
    /// The attrib file is known as attrib_*
    ///
    /// # Arguments
    ///
    /// * `hostname` - The name of the host to list the attributes.
    /// * `backup_number` - The number of the backup to list the attributes.
    /// * `share` - The share where the file is stored.
    /// * `filename` - The filename to list the attributes.
    ///
    /// # Returns
    ///
    /// A vector of `FileAttributes` containing the list of attributes
    ///
    /// # Errors
    ///
    /// If the file cannot be read or uncompressed.
    /// If the file is not found in the pool.
    fn list_file_from_dir<'a, 'b>(
        &self,
        hostname: &[u8],
        backup_number: u32,
        share: Option<&'a [u8]>,
        filename: Option<&'b [u8]>,
    ) -> Result<Vec<FileAttributes>>;
    /// List the attributes for hostname and backup knowning the attrib file
    ///
    /// This method search the hex of the attrib file (in the filename) and read the corresponding file in the pool.
    ///
    /// # Arguments
    ///
    /// * `hostname` - The name of the host to list the attributes.
    /// * `backup_number` - The number of the backup to list the attributes.
    /// * `attrib_path` - The path to the attributes file.
    /// * `attrib_file` - The prefix of the attributes file (starting attrib_).
    ///
    /// # Returns
    ///
    /// A vector of `FileAttributes` containing the list of attributes (readed from
    /// the atrrib file stored in the pool)
    ///
    /// # Errors
    ///
    /// If the file cannot be read or uncompressed.
    /// If the file is not found in the pool.
    ///
    fn list_attributes(
        &self,
        hostname: &[u8],
        backup_number: u32,
        attrib_path: &str,
        attrib_file: &str,
    ) -> Result<Vec<FileAttributes>>;
    /// Return the attributes of a file
    ///
    /// # Arguments
    ///
    /// * `hostname` - The name of the host to list the attributes.
    /// * `backup_number` - The number of the backup to list the attributes.
    /// * `share` - The share where the file is stored.
    /// * `filename` - The filename to list the attributes.
    ///
    /// # Returns
    ///
    /// A vector of `FileAttributes` containing the list of attributes  
    ///
    /// # Errors
    ///
    /// If the file cannot be read or uncompressed.
    /// If the file is not found in the pool.
    fn get_file(
        &self,
        hostname: &[u8],
        backup_number: u32,
        share: &[u8],
        filename: &[u8],
    ) -> Result<Vec<FileAttributes>>;
}

pub struct Search {
    topdir: PathBuf,
}

impl Search {
    #[must_use]
    pub fn new<P: AsRef<Path>>(topdir: P) -> Self {
        Search {
            topdir: topdir.as_ref().to_path_buf(),
        }
    }

    fn search_attrib_file<P: AsRef<Path>>(
        &self,
        backup_dir: P,
        attrib_file: &str,
    ) -> Option<(String, std::path::PathBuf)> {
        // Search for a file starting with the filename "attrib_" in the directory
        let file = std::fs::read_dir(backup_dir.as_ref())
            .ok()?
            .filter_map(|entry| match entry {
                Ok(entry) => entry
                    .file_name()
                    .to_str()
                    .map(|s| (s.to_string(), entry.path())),
                Err(err) => {
                    eprintln!(
                        "Error reading directory: {}, {err}",
                        backup_dir.as_ref().display()
                    );

                    None
                }
            })
            .find(|(name, _)| name.starts_with(attrib_file));

        file
    }
}

impl SearchTrait for Search {
    fn read_attrib(&self, file: &Path, is_compressed: bool) -> Result<Vec<FileAttributes>> {
        info!(
            "Reading attributes from file: {} {}",
            file.display(),
            is_compressed
        );

        let input_file = File::open(file)?;
        if is_compressed {
            let mut reader = BackupPCReader::new(input_file);
            let attrs = AttributeFile::read_from(&mut reader)?;

            Ok(attrs.attributes)
        } else {
            let mut reader = std::io::BufReader::new(input_file);
            let attrs = AttributeFile::read_from(&mut reader)?;

            Ok(attrs.attributes)
        }
    }

    fn list_attributes(
        &self,
        hostname: &[u8],
        backup_number: u32,
        attrib_path: &str,
        attrib_file: &str,
    ) -> Result<Vec<FileAttributes>> {
        let hostname_str = vec_to_osstr(hostname);
        let backup_dir = self
            .topdir
            .join("pc")
            .join(hostname_str)
            .join(backup_number.to_string())
            .join(attrib_path);

        info!("Looking for attributes in {}", backup_dir.display());

        let file = self.search_attrib_file(&backup_dir, attrib_file);

        if let Some((_, file)) = file {
            // Get the hash at the right of the _ symbole
            let file = file.to_str().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Invalid file path: {file:?}"),
                )
            })?;
            let file = file.split('_').collect::<Vec<&str>>();
            let file = file[file.len() - 1];
            if file == "0" {
                return Ok(Vec::new());
            }

            let md5_hash: Vec<u8> = hex_string_to_vec(file);

            match find_file_in_backuppc(&self.topdir, &md5_hash, None) {
                Ok((file_path, is_compressed)) => {
                    let attributes = self.read_attrib(&file_path, is_compressed)?;
                    return Ok(attributes);
                }
                Err(message) => {
                    return Err(
                        std::io::Error::new(std::io::ErrorKind::InvalidData, message).into(),
                    )
                }
            }
        }

        Ok(Vec::new())
    }

    fn list_file_from_dir(
        &self,
        hostname: &[u8],
        backup_number: u32,
        share: Option<&[u8]>,
        filename: Option<&[u8]>,
    ) -> Result<Vec<FileAttributes>> {
        let share = share.map(mangle_filename);
        let filename = filename.map(mangle);

        let attrib_path = [share, filename]
            .iter()
            .filter_map(|f| f.as_deref())
            .collect::<Vec<_>>()
            .join("/");

        self.list_attributes(hostname, backup_number, &attrib_path, "attrib_")
    }

    fn get_file(
        &self,
        hostname: &[u8],
        backup_number: u32,
        share: &[u8],
        filename: &[u8],
    ) -> Result<Vec<FileAttributes>> {
        info!(
            "Looking for file {filename:?} in {}/pc/{hostname:?}/{backup_number}/{}",
            self.topdir.display(),
            mangle_filename(share),
        );

        let backup_dir_parts: Vec<&[u8]> = filename.split(|&byte| byte == b'/').collect();

        let filename = backup_dir_parts.last().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid path {filename:?}"),
            )
        })?;
        let path = backup_dir_parts[..backup_dir_parts.len() - 1].join(&b'/');

        match self.list_file_from_dir(hostname, backup_number, Some(share), Some(&path)) {
            Ok(attributes) => Ok(attributes
                .into_iter()
                .filter(|attr| {
                    let filename = filename.to_vec();
                    attr.name.cmp(&filename) == Ordering::Equal
                })
                .collect()),
            Err(e) => Err(e),
        }
    }
}
