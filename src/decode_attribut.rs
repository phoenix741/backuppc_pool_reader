use std::error::Error;
use std::hash::Hash;
use std::io::{self, Read};

use byteorder::{BigEndian, ReadBytesExt};
use num_traits::FromPrimitive;

use crate::hosts::BackupInformation;

const BPC_ATTRIB_TYPE_XATTR: u32 = 0x1756_5353;

/// Enum representing the type of a file.
#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub enum FileType {
    /// Regular file.
    File = 0,
    /// Hard link.
    Hardlink = 1,
    /// Symbolic link.
    Symlink = 2,
    /// Character device.
    Chardev = 3,
    /// Block device.
    Blockdev = 4,
    /// Directory.
    Dir = 5,
    /// FIFO (named pipe).
    Fifo = 6,
    /// Unknown file type.
    Unknown = 7,
    /// Socket.
    Socket = 8,
    /// Deleted file.
    Deleted = 9,
}

/// Trait for reading variable-length integers from a `Read` source.
pub trait VarintRead: Read {
    /// Reads a variable-length integer from the source.
    ///
    /// # Returns
    ///
    /// Returns the read integer as a `u64` wrapped in an `io::Result`.
    ///
    /// # Errors
    ///
    /// Returns an `io::Error` if there was an error reading from the source or if the
    /// read integer is too large to fit in a `u64`.
    fn read_varint<T: FromPrimitive>(&mut self) -> io::Result<T> {
        let mut result = 0;
        let mut shift = 0;

        loop {
            let mut buf: [u8; 1] = [0u8; 1];
            self.read_exact(&mut buf)?;

            let byte = buf[0];
            let val = u64::from(byte & 0x7F);
            if shift >= 64 || val << shift >> shift != val {
                eprintln!("Varint too large: probably corrupted data");
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Varint too large: probably corrupted data",
                ));
            }

            result |= val << shift;
            if byte & 0x80 == 0 {
                return T::from_u64(result).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Varint too large for the target type",
                    )
                });
            }
            shift += 7;
        }
    }
}

// Implémenter VarintRead pour tous les types qui implémentent Read
impl<R: Read + ?Sized> VarintRead for R {}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
/// Structure representing an extended attribute entry.
pub struct XattrEntry {
    /// The key of the extended attribute.
    pub key: String,
    /// The value of the extended attribute.
    pub value: String,
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
/// Structure representing a `BackupPC` digest.
pub struct BpcDigest {
    /// Length of the digest.
    pub len: u64,
    /// The digest data.
    pub digest: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
/// Structure representing file attributes.
pub struct FileAttributes {
    /// Name of the file.
    pub name: String,
    /// Type of the file.
    pub type_: FileType,
    /// Compression level of the file.
    pub compress: u64,

    /// File mode.
    pub mode: u16,
    /// User ID of the file owner.
    pub uid: u32,
    /// Group ID of the file owner.
    pub gid: u32,
    /// Number of hard links to the file.
    pub nlinks: u32,

    /// Modification time of the file.
    pub mtime: u64,
    /// Size of the file.
    pub size: u64,
    /// Inode number of the file.
    pub inode: u64,

    /// BackupPC digest of the file.
    pub bpc_digest: BpcDigest,

    /// Number of extended attributes entries.
    pub xattr_num_entries: u64,
    /// List of extended attributes entries.
    pub xattrs: Vec<XattrEntry>,
}

impl FileAttributes {
    pub fn from_host(host: String) -> Self {
        Self {
            name: host,
            type_: FileType::Dir,
            compress: 0,
            mode: 0,
            uid: 0,
            gid: 0,
            nlinks: 0,
            mtime: 0,
            size: 0,
            inode: 0,
            bpc_digest: BpcDigest {
                len: 0,
                digest: Vec::new(),
            },
            xattr_num_entries: 0,
            xattrs: Vec::new(),
        }
    }

    pub fn from_backup(backup: &BackupInformation) -> Self {
        Self {
            name: backup.num.to_string(),
            type_: FileType::Dir,
            compress: 0,
            mode: 0,
            uid: 0,
            gid: 0,
            nlinks: 0,
            mtime: backup.start_time,
            size: 0,
            inode: 0,
            bpc_digest: BpcDigest {
                len: 0,
                digest: Vec::new(),
            },
            xattr_num_entries: 0,
            xattrs: Vec::new(),
        }
    }

    pub fn from_share(share: String) -> Self {
        Self {
            name: share,
            type_: FileType::Dir,
            compress: 0,
            mode: 0,
            uid: 0,
            gid: 0,
            nlinks: 0,
            mtime: 0,
            size: 0,
            inode: 0,
            bpc_digest: BpcDigest {
                len: 0,
                digest: Vec::new(),
            },
            xattr_num_entries: 0,
            xattrs: Vec::new(),
        }
    }
}

/// Reads file attributes from a reader.
///
/// # Arguments
///
/// * `reader` - A mutable reference to a reader implementing the `Read` and `VarintRead` traits.
///
/// # Returns
///
/// Returns a `Result` containing the `FileAttributes` if the read was successful, or an `io::Error` if an error occurred.
///
/// # Examples
///
/// ```no_run
/// use std::io;
/// use std::fs::File;
/// use std::io::Read;
/// use backuppc_pool_reader::decode_attribut::FileAttributes;
///
/// let mut reader = File::open("test").unwrap();
/// let result = FileAttributes::read_from(&mut reader);
/// match result {
///     Ok(attributes) => {
///         // Handle the file attributes
///     },
///     Err(error) => {
///         // Handle the error
///     }
/// }
/// ```
///
/// # Errors
///
/// This function can return an `io::Error` with the kind `InvalidData` if the file type is invalid.
///
/// # Panics
///
/// This function will panic if the UTF-8 conversion of the file name or xattr key or value fails.
///
/// # Safety
///
/// This function assumes that the reader is properly initialized and that the data being read is valid.
impl FileAttributes {
    pub fn read_from<R: Read + VarintRead>(reader: &mut R) -> io::Result<Self> {
        let filename_len: usize = reader.read_varint()?;
        let mut name = vec![0u8; filename_len];
        reader.read_exact(&mut name)?;
        let name = String::from_utf8(name).unwrap_or_default();

        let xattr_num_entries: u64 = reader.read_varint().unwrap_or_default();
        let type_: FileType = match reader.read_varint().unwrap_or(9) {
            0 => FileType::File,
            1 => FileType::Hardlink,
            2 => FileType::Symlink,
            3 => FileType::Chardev,
            4 => FileType::Blockdev,
            5 => FileType::Dir,
            6 => FileType::Fifo,
            8 => FileType::Socket,
            7 | 9 => FileType::Unknown,
            10 => FileType::Deleted,
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid file type {other}"),
                ))
            }
        };
        let mtime: u64 = reader.read_varint().unwrap_or_default();
        let mode: u16 = reader.read_varint().unwrap_or_default();
        let uid: u32 = reader.read_varint().unwrap_or_default();
        let gid: u32 = reader.read_varint().unwrap_or_default();
        let size: u64 = reader.read_varint().unwrap_or_default();
        let inode: u64 = reader.read_varint().unwrap_or_default();
        let compress: u64 = reader.read_varint().unwrap_or_default();
        let nlinks: u32 = reader.read_varint().unwrap_or_default();

        let digest_len: usize = reader.read_varint().unwrap_or_default();
        let mut digest = vec![0u8; digest_len];
        if digest_len > 0 {
            reader.read_exact(&mut digest)?;
        }

        let mut xattrs = Vec::new();
        for _ in 0..xattr_num_entries {
            let key_len: usize = reader.read_varint().unwrap_or_default();
            let mut key = vec![0u8; key_len];
            reader.read_exact(&mut key)?;
            let key = String::from_utf8(key).unwrap_or_default();

            let value_len: usize = reader.read_varint().unwrap_or_default();
            let mut value = vec![0u8; value_len];
            reader.read_exact(&mut value)?;
            let value = String::from_utf8(value).unwrap_or_default();

            xattrs.push(XattrEntry { key, value });
        }

        Ok(Self {
            name,
            xattr_num_entries,
            type_,
            mtime,
            mode,
            uid,
            gid,
            size,
            inode,
            compress,
            nlinks,
            bpc_digest: BpcDigest {
                len: digest_len as u64,
                digest,
            },
            xattrs,
        })
    }
}

#[derive(Debug)]
pub struct AttributeFile {
    pub attributes: Vec<FileAttributes>,
}

/// Reads an `AttributeFile` from a reader.
///
/// # Arguments
///
/// * `reader` - A mutable reference to a reader implementing `Read` and `VarintRead` traits.
///
/// # Returns
///
/// Returns a `Result` containing the decoded `AttributeFile` if successful, or a boxed `dyn Error` if an error occurs.
///
/// # Examples
///
/// ```no_run
/// use std::io::Cursor;
/// use byteorder::{BigEndian, ReadBytesExt};
/// use backuppc_pool_reader::decode_attribut::AttributeFile;
///
/// let data = vec![0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02];
/// let mut reader = Cursor::new(data);
///
/// let result = AttributeFile::read_from(&mut reader);
/// assert!(result.is_ok());
/// let attribute_file = result.unwrap();
/// ```
impl AttributeFile {
    pub fn read_from<R: Read + VarintRead>(reader: &mut R) -> Result<Self, Box<dyn Error>> {
        let magic: u32 = reader.read_u32::<BigEndian>()?;
        if magic != BPC_ATTRIB_TYPE_XATTR {
            return Err("Invalid magic number".into());
        }

        let mut attributes = Vec::new();
        loop {
            match FileAttributes::read_from(reader) {
                Ok(attr) => attributes.push(attr),
                Err(e) => {
                    if e.kind() == io::ErrorKind::UnexpectedEof {
                        break;
                    }

                    eprintln!("Error reading file attributes: {e}");
                }
            }
        }

        Ok(Self { attributes })
    }
}
