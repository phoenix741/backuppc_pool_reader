use flate2::bufread::ZlibDecoder;
use std::io::{self, BufRead, BufReader, Read};

/* InterpretAdapter */

/// A struct representing an adapter for interpreting compressed data.
///
/// This struct wraps a generic `BufRead` type and provides additional functionality for interpreting compressed data.
///
/// The compression format of BackupPC is a custom format that is not directly supported by the `flate2` crate.
/// The goal of the InterpredAdapter is to interpret the BackupPC compression format and convert it to a format that
/// can be handled by the `flate2` crate.
///
/// BackupPC format is a serie of chunk of data where some bytes are replaced to define the checksum at the end.
struct InterpretAdapter<R: BufRead> {
    inner: R,
    first: bool,
    temp: Option<Vec<u8>>,
}

impl<R: BufRead> InterpretAdapter<R> {
    /// Creates a new `InterpretAdapter` with the specified inner `BufRead` reader.
    ///
    /// # Arguments
    ///
    /// * `inner` - The inner `BufRead` reader.
    ///
    /// # Returns
    ///
    /// A new `InterpretAdapter` instance.
    fn new(inner: R) -> Self {
        Self {
            inner,
            first: true,
            temp: None,
        }
    }

    fn reset(&mut self) {
        self.first = true;
        self.temp = None;
    }
}

impl<R: BufRead> Read for InterpretAdapter<R> {
    /// Reads bytes from the underlying reader into the specified buffer, and interprets the bytes according to the compression format.
    ///
    /// # Arguments
    ///
    /// * `buf` - A mutable reference to the buffer where the read bytes will be stored.
    ///
    /// # Returns
    ///
    /// The number of bytes read from the underlying reader, or an `io::Result` indicating the error encountered during the read operation.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let len = self.inner.read(buf)?;

        if self.first && len > 0 {
            self.first = false;
            if buf[0] == 0xd6 || buf[0] == 0xd7 {
                buf[0] = 0x78;
            } else if buf[0] == 0xb3 {
                return Ok(0);
            }
        }

        Ok(len)
    }
}

impl<R: BufRead> BufRead for InterpretAdapter<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.temp.is_none() {
            let buf = self.inner.fill_buf()?;
            let mut buf = buf.to_vec();

            if self.first && !buf.is_empty() {
                self.first = false;

                if buf[0] == 0xd6 || buf[0] == 0xd7 {
                    buf[0] = 0x78;
                } else if buf[0] == 0xb3 {
                    // EOF
                    buf = Vec::new();
                }
            }

            self.temp = Some(buf);
        }

        Ok(self.temp.as_ref().unwrap())
    }

    fn consume(&mut self, amt: usize) {
        if amt > 0 {
            self.temp = None;
            self.inner.consume(amt);
        }
    }
}

/* BackupPCReader */

/// A reader that decompresses data from a source using the `BackupPC` compression format.
pub struct BackupPCReader<R: Read> {
    decoder: Option<ZlibDecoder<InterpretAdapter<BufReader<R>>>>,
}

impl<R: Read> BackupPCReader<R> {
    /// Create a new `BackupPCReader` with the given reader.
    ///
    /// This function takes a reader and performs the necessary setup to enable reading compressed data.
    /// It creates a buffer, an interpretation adapter, and a zlib decoder.
    ///
    /// # Arguments
    ///
    /// * `reader` - The reader to be used for reading compressed data.
    ///
    /// # Returns
    ///
    /// A new `BackupPCReader` instance.
    pub fn new(reader: R) -> Self {
        let reader = BufReader::new(reader);
        let reader = InterpretAdapter::new(reader);
        Self {
            decoder: Some(ZlibDecoder::new(reader)),
        }
    }

    /// Reads bytes from the underlying decoder and fills the provided buffer.
    ///
    /// # Arguments
    ///
    /// * `buf` - A mutable reference to the buffer where the read bytes will be stored.
    ///
    /// # Returns
    ///
    /// Returns the number of bytes read and stored in the buffer, or an `io::Error` if an error occurred.
    fn read_some_bytes(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let decoder = self.decoder.as_mut();
            if decoder.is_none() {
                return Ok(0);
            }

            let decoder_read_result = decoder.unwrap().read(buf);

            let count = match decoder_read_result {
                Ok(count) => {
                    // Print to stderr to avoid polluting stdout
                    count
                }
                Err(e) => {
                    return Err(e);
                }
            };

            if count != 0 {
                return Ok(count);
            }

            if count == 0 {
                let decoder = self.decoder.take();
                if let Some(decoder) = decoder {
                    let mut reader = decoder.into_inner();
                    // S'il reste encore des octets à lire dans reader alors on continue, sinon on s'arrête
                    if reader.fill_buf()?.is_empty() {
                        return Ok(0);
                    }
                    reader.reset();

                    self.decoder = Some(ZlibDecoder::new(reader));
                }
            }
        }
    }
}

/// Implements the `Read` trait for `BackupPCReader<R>`.
/// This allows instances of `BackupPCReader<R>` to be used as a source of bytes.
impl<R: Read> Read for BackupPCReader<R> {
    // Read bytes to fill the buffer until the buffer is full or the end of the stream is reached.
    ///
    /// # Arguments
    ///
    /// * `buf` - A mutable reference to the buffer where the read bytes will be stored.
    ///
    /// # Returns
    ///
    /// Returns the number of bytes read and stored in the buffer, or an `io::Error` if an error occurred.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut total_bytes_read = 0;

        while total_bytes_read < buf.len() {
            let bytes_to_read = &mut buf[total_bytes_read..];
            let bytes_read = self.read_some_bytes(bytes_to_read)?;
            total_bytes_read += bytes_read;

            if bytes_read == 0 {
                break;
            }
        }

        Ok(total_bytes_read)
    }
}
