use std::path::Path;

use crate::util;

/// Finds a file in the `BackupPC` pool directory based on its file hash.
///
/// The function takes the top directory path, the file hash as a vector of bytes,
/// and an optional collision ID. It constructs the file path based on the top directory,
/// the first two bytes of the file hash, and the file hash itself. If a collision ID is provided,
/// it is included in the file path as well.
///
/// The function checks if the file exists in the pool directory or the cpool directory.
/// If the file is found in either directory, the function returns the path as a `String`
/// along with a flag indicating if the file is compressed or not. If the file is not found,
/// an error message is returned.
///
/// # Arguments
///
/// * `topdir` - The top directory path where the `BackupPC` pool is located.
/// * `file_hash` - The file hash as a vector of bytes.
/// * `collid` - An optional collision ID.
///
/// # Returns
///
/// * If the file is found, the function returns a tuple containing the path as a `String`
///   and a flag indicating if the file is compressed (`true`) or not (`false`).
/// * If the file is not found, an error message is returned.
///
/// # Examples
///
/// ```
/// use crate::pool::find_file_in_backuppc;
///
/// let topdir = "/home/user/backuppc";
/// let file_hash = vec![0x12, 0x34, 0x56, 0x78];
/// let collid = Some(123);
///
/// let result = find_file_in_backuppc(topdir, &file_hash, collid);
/// match result {
///     Ok((path, is_compressed)) => {
///         if is_compressed {
///             println!("Compressed file found at: {}", path);
///         } else {
///             println!("Uncompressed file found at: {}", path);
///         }
///     },
///     Err(err) => println!("Error: {}", err),
/// }
/// ```
pub fn find_file_in_backuppc(
    topdir: &str,
    file_hash: &Vec<u8>,
    collid: Option<u64>,
) -> Result<(String, bool), String> {
    if file_hash.len() < 2 {
        return Err(format!(
            "File hash {} must be at least 2 bytes long",
            util::vec_to_hex_string(file_hash)
        ));
    }

    let firsts = format!("{:02x}", (file_hash[0] & 0xfe));
    let seconds = format!("{:02x}", (file_hash[1] & 0xfe));
    let file_hash = util::vec_to_hex_string(file_hash);
    let collid = match collid {
        Some(collid) => format!("{collid:02x}"),
        None => String::new(),
    };
    let file_hash = format!("{collid}{file_hash}");

    let pool_path = Path::new(topdir)
        .join("pool")
        .join(&firsts)
        .join(&seconds)
        .join(&file_hash);

    let cpool_path = Path::new(topdir)
        .join("cpool")
        .join(&firsts)
        .join(&seconds)
        .join(&file_hash);

    if pool_path.exists() {
        let path = pool_path.to_str().ok_or("pool path not exists")?;
        Ok((path.to_string(), false))
    } else if cpool_path.exists() {
        let path = cpool_path.to_str().ok_or("cpool path not exists")?;
        Ok((path.to_string(), true))
    } else {
        Err(format!("File {file_hash} does not exist"))
    }
}
