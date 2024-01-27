use std::{collections::HashSet, hash::Hash};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Converts a vector of bytes to a hexadecimal string representation.
///
/// # Arguments
///
/// * `vec` - A reference to a vector of bytes.
///
/// # Returns
///
/// A string representing the hexadecimal values of the bytes in the vector.
pub fn vec_to_hex_string(vec: &Vec<u8>) -> String {
    vec.iter()
        .map(|byte| format!("{:02x}", byte))
        .collect::<String>()
}

/// Converts a hexadecimal string to a vector of bytes.
///
/// # Arguments
///
/// * `hex_string` - A reference to a hexadecimal string.
///
/// # Returns
///
/// A vector of bytes representing the hexadecimal values in the string.
pub fn hex_string_to_vec(hex_string: &str) -> Vec<u8> {
    hex_string
        .as_bytes()
        .chunks(2)
        .map(|chunk| u8::from_str_radix(&String::from_utf8(chunk.to_vec()).unwrap(), 16).unwrap())
        .collect()
}

/// Mangles a filename by replacing certain characters with their hexadecimal representation.
///
/// # Arguments
///
/// * `path_um` - The original filename.
///
/// # Returns
///
/// A mangled filename where certain characters are replaced with their hexadecimal representation.
pub fn mangle_filename(path_um: &str) -> String {
    let mut path = String::new();

    if path_um.is_empty() {
        return path;
    }

    path.push('f');

    for c in path_um.chars() {
        if c != '%' && c != '/' && c != '\n' && c != '\r' {
            path.push(c);
        } else {
            path.push('%');
            path.push_str(&format!("{:02x}", c as u8));
        }
    }

    path
}

/// Unmangles a filename by replacing hexadecimal representations with their original characters.
///
/// # Arguments
///
/// * `path_m` - The mangled filename.
///
/// # Returns
///
/// An unmangled filename where hexadecimal representations are replaced with their original characters.
pub fn unmangle_filename(path_m: &str) -> String {
    let mut path = String::new();

    if path_m.is_empty() {
        return path;
    }

    let mut chars = path_m.chars();

    if chars.next().unwrap() != 'f' {
        return path;
    }

    while let Some(c) = chars.next() {
        if c == '%' {
            let hex = chars.next().unwrap().to_string() + &chars.next().unwrap().to_string();
            let byte = u8::from_str_radix(&hex, 16).unwrap();
            path.push(byte as char);
        } else {
            path.push(c);
        }
    }

    path
}

/// Mangles a file path by applying the `mangle_filename` function to each component of the path.
///
/// # Arguments
///
/// * `path_um` - The original file path.
///
/// # Returns
///
/// A mangled file path where each component is mangled using the `mangle_filename` function.
pub fn mangle(path_um: &str) -> String {
    if path_um.is_empty() {
        return String::new();
    }

    let mangled_components: Vec<String> = path_um
        .split('/')
        .map(|component| mangle_filename(component))
        .collect();

    let mangled_path = mangled_components.join("/");

    format!("{}", mangled_path)
}

/// Filter all value to return only unique values
///
/// # Arguments
///
/// * `iterable` - The iterable to filter
///
/// # Returns
///
/// A new iterable with only unique values
pub fn unique<T: Eq + Hash + Clone>(iterable: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut seen = HashSet::new();
    iterable
        .into_iter()
        .filter(|e| seen.insert(e.clone()))
        .collect()
}
