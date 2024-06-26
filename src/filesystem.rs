use log::{debug, error, info};
use lru::LruCache;
use std::hash::Hasher;
use std::io::Read;
use std::num::NonZeroUsize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use twox_hash::XxHash64;

extern crate fuser;
extern crate libc;

use fuser::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEmpty, ReplyEntry,
    ReplyOpen, Request,
};
use libc::ENOENT;
use std::{collections::HashMap, ffi::OsStr};

use crate::attribute_file::Search;
use crate::decode_attribut::{FileAttributes, FileType as BackupPCFileType};
use crate::hosts::Hosts;
use crate::util::Result;
use crate::view::BackupPC;

const TTL_HOST: Duration = Duration::from_secs(86_400);
const TTL_BACKUPS: Duration = Duration::from_secs(3_600);
const TTL_REST: Duration = Duration::from_secs(1_000_000);

const CACHE_SIZE: usize = 2048;

const CREATE_TIME: SystemTime = UNIX_EPOCH;

#[derive(PartialEq, Default, Debug)]
struct CacheElement {
    pub path: Vec<String>,

    pub parent_ino: u64,
}

const ROOT_ELEMENT: CacheElement = CacheElement {
    path: vec![],
    parent_ino: 0,
};

#[derive(Clone, Debug)]
pub struct BackupPCFileAttribute {
    pub name: String,
    pub attr: FileAttr,
}

impl BackupPCFileAttribute {
    pub fn from_file_attribute(file: FileAttributes, child_ino: u64) -> Self {
        BackupPCFileAttribute {
            name: file.name,
            attr: FileAttr {
                ino: child_ino,
                size: file.size,
                blocks: file.size / 512,
                blksize: 512,
                atime: UNIX_EPOCH + Duration::from_secs(file.mtime),
                mtime: UNIX_EPOCH + Duration::from_secs(file.mtime),
                ctime: UNIX_EPOCH + Duration::from_secs(file.mtime),
                crtime: UNIX_EPOCH + Duration::from_secs(file.mtime),
                kind: match file.type_ {
                    BackupPCFileType::Symlink => FileType::Symlink,
                    BackupPCFileType::Chardev => FileType::CharDevice,
                    BackupPCFileType::Blockdev => FileType::BlockDevice,
                    BackupPCFileType::Dir => FileType::Directory,
                    BackupPCFileType::Fifo => FileType::NamedPipe,
                    BackupPCFileType::Socket => FileType::Socket,
                    _ => FileType::RegularFile,
                },
                perm: file.mode,
                nlink: file.nlinks,
                uid: file.uid,
                gid: file.gid,
                rdev: 0,
                flags: 0,
            },
        }
    }
}

const ROOT_ELEMENT_ATTR: FileAttr = FileAttr {
    ino: 1,
    size: 0,
    blocks: 0,
    blksize: 0,
    atime: CREATE_TIME,
    mtime: CREATE_TIME,
    ctime: CREATE_TIME,
    crtime: CREATE_TIME,
    kind: FileType::Directory,
    perm: 0o755,
    nlink: 1,
    uid: 1,
    gid: 1,
    rdev: 0,
    flags: 0,
};

pub struct OpenedFile {
    pub offset: i64,
    pub reader: Box<dyn Read>,
}

pub struct BackupPCFS {
    view: BackupPC,
    inodes: HashMap<u64, CacheElement>,
    cache: LruCache<u64, Vec<BackupPCFileAttribute>>,
    opened: HashMap<u64, OpenedFile>,
}

impl BackupPCFS {
    pub fn new(topdir: &str) -> Self {
        let hosts = Box::new(Hosts::new(topdir));
        let search = Box::new(Search::new(topdir));

        BackupPCFS {
            inodes: HashMap::new(),
            view: BackupPC::new(topdir, hosts, search),
            cache: LruCache::new(NonZeroUsize::new(CACHE_SIZE).unwrap()),
            opened: HashMap::new(),
        }
    }

    fn generate_new_ino(&self, elt: &CacheElement) -> u64 {
        let mut hasher = XxHash64::with_seed(0);
        let key = elt.path.join("/");
        hasher.write(key.as_bytes());
        let mut hash = hasher.finish();

        // Vérifiez si l'ino est déjà utilisé, si oui, utilisez le sondage quadratique pour trouver un ino libre
        let mut probe = 1;
        while self.inodes.contains_key(&hash) {
            if self.inodes.get(&hash).unwrap_or(&CacheElement::default()) == elt {
                return hash;
            }

            hash += probe * probe;
            probe += 1;
        }

        hash
    }

    fn generate_file_handle(&self) -> u64 {
        // Random file handle not used in opened files
        loop {
            let handle = rand::random::<u64>();
            if !self.opened.contains_key(&handle) {
                return handle;
            }
        }
    }

    fn list_files(&mut self, ino: u64, path: Vec<&str>) -> Result<Vec<BackupPCFileAttribute>> {
        let files = self.view.list(&path)?;

        let result = files
            .into_iter()
            .filter_map(move |file| {
                if file.type_ == BackupPCFileType::Unknown
                    || file.type_ == BackupPCFileType::Deleted
                {
                    error!("Unknown or deleted file: {:?}", file);
                    return None;
                }

                let mut path: Vec<String> =
                    path.iter().map(std::string::ToString::to_string).collect();
                path.push(file.name.clone());

                let key = CacheElement {
                    path,
                    parent_ino: ino,
                };
                let child_ino = self.generate_new_ino(&key);

                self.inodes.insert(child_ino, key);

                Some(BackupPCFileAttribute::from_file_attribute(file, child_ino))
            })
            .collect();

        Ok(result)
    }

    fn list_attributes(&mut self, ino: u64) -> Result<Vec<BackupPCFileAttribute>> {
        let binding = ROOT_ELEMENT;
        let cache_element = match ino {
            0 => {
                return Ok(vec![BackupPCFileAttribute {
                    name: "..".to_string(),
                    attr: ROOT_ELEMENT_ATTR,
                }])
            }
            1 => Some(&binding),
            _ => self.inodes.get(&ino),
        }
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "No value"))?;

        let path = cache_element.path.clone();

        match self.list_files(ino, path.iter().map(std::string::String::as_str).collect()) {
            Ok(files) => Ok(files),
            Err(err) => {
                eprintln!("Error listing files of {}: {}", path.join("/"), err);
                Err(err)
            }
        }
    }

    fn list_attributes_with_cache(&mut self, ino: u64) -> Result<Vec<BackupPCFileAttribute>> {
        if let Some(cached_result) = self.cache.get(&ino) {
            return Ok(cached_result.clone());
        }

        let mut result = self.list_attributes(ino)?;
        result.sort_by(|a, b| a.name.cmp(&b.name));
        self.cache.put(ino, result.clone());

        Ok(result)
    }

    fn get_file_attr(&mut self, ino: u64, name: &OsStr) -> Option<(Duration, FileAttr)> {
        let binding = ROOT_ELEMENT;
        let cache_element = match ino {
            1 => Some(&binding),
            _ => self.inodes.get(&ino),
        }?;

        let duration = match cache_element.path.len() {
            0 => TTL_HOST,
            1 => TTL_BACKUPS,
            _ => TTL_REST,
        };

        let attributes = self.list_attributes_with_cache(ino);
        let attribute = match attributes {
            Ok(attrs) => attrs.into_iter().find(|attr| attr.name.as_str() == name),
            Err(_) => None,
        };

        attribute.map(|attr| (duration, attr.attr))
    }

    fn fill_reply_from_files(
        &mut self,
        reply: &mut ReplyDirectory,
        ino: u64,
        offset: i64,
    ) -> Result<()> {
        let elements = self.list_attributes_with_cache(ino)?;

        // Add the "." and ".." entries
        if ino != 1 && offset == 0 {
            let element = self.inodes.get(&ino);
            if let Some(parent) = element {
                let _ = reply.add(ino, 1, FileType::Directory, ".");
                let _ = reply.add(parent.parent_ino, 2, FileType::Directory, "..");
            }
        }

        let offset = usize::try_from(offset)?;
        for (current_offset, cache_element) in elements.iter().enumerate() {
            let current_offset = current_offset + 2;
            if current_offset <= offset {
                continue;
            }

            debug!(
                "Adding entry {} to ino {}, offset: {}, kind: {:?}",
                cache_element.name, cache_element.attr.ino, current_offset, cache_element.attr.kind,
            );
            let result = reply.add(
                cache_element.attr.ino,
                current_offset as i64,
                cache_element.attr.kind,
                &cache_element.name,
            );
            if result {
                break;
            }
        }
        Ok(())
    }

    fn get_attr(&mut self, ino: u64) -> Option<(Duration, FileAttr)> {
        let binding = ROOT_ELEMENT;
        let cache_element = match ino {
            1 => Some(&binding),
            _ => self.inodes.get(&ino),
        }?;

        let duration = match cache_element.path.len() {
            0 => TTL_HOST,
            1 => TTL_BACKUPS,
            _ => TTL_REST,
        };

        let parent_ino = cache_element.parent_ino;

        let attributes = self.list_attributes_with_cache(parent_ino);
        let attribute = match attributes {
            Ok(attrs) => attrs.into_iter().find(|attr| attr.attr.ino == ino),
            Err(_) => None,
        };

        attribute.map(|attr| (duration, attr.attr))
    }

    fn create_reader(&mut self, ino: u64) -> Result<Box<dyn Read>> {
        let binding = ROOT_ELEMENT;
        let cache_element = match ino {
            1 => Some(&binding),
            _ => self.inodes.get(&ino),
        }
        .ok_or(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to get filename",
        ))?;

        let path = cache_element.path.clone();
        let path_refs: Vec<&str> = path.iter().map(std::string::String::as_str).collect();

        match self.view.read_file(&path_refs) {
            Ok(reader) => Ok(reader),
            Err(err) => {
                eprintln!("Can't open the file {}: {}", path.join("/"), err);
                Err(err)
            }
        }
    }

    fn read_link(&mut self, ino: u64) -> Result<Vec<u8>> {
        let mut reader = self.create_reader(ino)?;
        let mut buf = Vec::<u8>::new();
        reader.read_to_end(&mut buf)?;

        Ok(buf)
    }

    fn open(&mut self, ino: u64) -> Result<u64> {
        let reader = self.create_reader(ino)?;
        let fh = self.generate_file_handle();
        self.opened.insert(
            fh,
            OpenedFile {
                offset: 0,
                reader: Box::new(reader),
            },
        );

        Ok(fh)
    }

    fn release(&mut self, fh: u64) {
        self.opened.remove(&fh);
    }

    fn read_ino(&mut self, ino: u64, fh: u64, offset: i64, size: u32) -> Result<Vec<u8>> {
        let opened_file = self
            .opened
            .get(&fh)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "File not opened"))?;

        // If the offset is lesser than the current offset, we need to reset the reader
        if offset < opened_file.offset {
            let reader = self.create_reader(ino)?;

            let opened_file = self.opened.get_mut(&fh).unwrap();
            opened_file.reader = reader;
            opened_file.offset = 0;
        }

        let opened_file = self.opened.get_mut(&fh).unwrap();

        // If the offset is greater that the current offset, we need to fast forward (by reading data by 32k chunk )
        if offset > opened_file.offset {
            let mut buffer = vec![0; 32 * 1024];
            let mut remaining = offset - opened_file.offset;

            while remaining > 0 {
                let to_read = std::cmp::min(remaining, buffer.len() as i64);
                let to_read = usize::try_from(to_read)?;
                let size: usize = opened_file.reader.read(&mut buffer[..to_read])?;
                remaining -= size as i64;
                if size == 0 {
                    info!("End of file reached");
                    break;
                }
            }
            opened_file.offset = offset;
        }

        // Read the data
        let reader = opened_file.reader.as_mut();
        let mut buffer = vec![0; size as usize];

        let size = reader.read(&mut buffer)?;
        opened_file.offset += size as i64;

        // Reduce the size of the buffer to the actual size read
        buffer.truncate(size);

        Ok(buffer)
    }
}

impl Filesystem for BackupPCFS {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let attr = self.get_file_attr(parent, name);
        debug!("Lookup parent: {parent}, name: {name:?}, attr: {attr:?}");

        match attr {
            Some((ttl, attr)) => reply.entry(&ttl, &attr, 0),
            None => reply.error(ENOENT),
        }
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        let attr = self.get_attr(ino);
        debug!("Getattr ino: {ino}, attr: {attr:?}");

        match attr {
            Some((ttl, attr)) => reply.attr(&ttl, &attr),
            None => reply.error(ENOENT),
        }
    }

    fn readlink(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyData) {
        let link_to = self.read_link(ino);
        debug!("Readlink ino: {ino}, attr: {link_to:?}");

        match link_to {
            Ok(data) => reply.data(&data),
            Err(err) => {
                eprintln!("Error reading link ino {ino}: {err}");
                reply.error(ENOENT);
            }
        }
    }

    fn open(&mut self, _req: &Request<'_>, ino: u64, _flags: i32, reply: ReplyOpen) {
        match self.open(ino) {
            Ok(fh) => reply.opened(fh, 0),
            Err(err) => {
                eprintln!("Error opening ino {ino}: {err}");
                reply.error(ENOENT);
            }
        }
    }

    fn read(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        match self.read_ino(ino, fh, offset, size) {
            Ok(data) => reply.data(&data),
            Err(err) => {
                eprintln!("Error reading ino {ino}: {err}");
                reply.error(ENOENT);
            }
        }
    }

    fn release(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        self.release(fh);
        reply.ok();
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        debug!("Readdir ino: {ino}, offset: {offset}");
        // List host and add it to the cache
        match self.fill_reply_from_files(&mut reply, ino, offset) {
            Ok(()) => {
                reply.ok();
            }
            Err(e) => {
                eprintln!("Error reading dir {ino}: {e}");
                reply.error(ENOENT);
            }
        }
    }
}
