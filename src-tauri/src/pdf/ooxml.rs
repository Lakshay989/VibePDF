//! A minimal ZIP writer, for the Office Open XML container a `.docx` is.
//!
//! SPEC: P7-OCR-004 (P7.B1a). A `.docx` is a ZIP archive of a handful of small
//! XML parts. Writing one needs local file headers, a central directory and an
//! end-of-central-directory record — and nothing else we use: no Zip64 (the
//! parts are kilobytes), no encryption, no streaming, no data descriptors.
//!
//! **Why not the `zip` crate.** It is in `Cargo.lock` but only as a build
//! dependency of the Tauri bundler — it is not reachable from the normal
//! dependency tree, so taking it would be a genuinely new top-level dependency
//! and several transitive ones. `flate2` (via lopdf) and `crc32fast` (via
//! flate2) are already there, which is everything this needs. The risk of a
//! hand-written container is that a subtle error produces a file Word silently
//! refuses, so the tests read the archive back through an independent parser
//! and the acceptance pass opens it in Word itself.
//!
//! Everything here is written little-endian, as the format requires.

use std::io::Write;

use flate2::write::DeflateEncoder;
use flate2::Compression;

use crate::error::CommandError;

/// One file in the archive, held until the central directory can be written.
struct Entry {
    name: String,
    crc: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    /// Offset of this entry's local header from the start of the file.
    offset: u32,
    /// 0 = stored, 8 = deflate.
    method: u16,
}

/// Builds a ZIP archive in memory.
pub struct ZipWriter {
    out: Vec<u8>,
    entries: Vec<Entry>,
}

/// The signature values the format fixes.
const LOCAL_HEADER: u32 = 0x0403_4b50;
const CENTRAL_HEADER: u32 = 0x0201_4b50;
const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4b50;
/// 2.0 — the minimum that understands deflate.
const VERSION_NEEDED: u16 = 20;

impl ZipWriter {
    #[must_use]
    pub fn new() -> Self {
        Self {
            out: Vec::new(),
            entries: Vec::new(),
        }
    }

    /// Add a file. `deflate` false stores it as-is, which is smaller for data
    /// that is already compressed (a PNG) and required for nothing here.
    ///
    /// # Errors
    /// When the data does not fit a 32-bit ZIP, or deflate fails.
    pub fn add(&mut self, name: &str, data: &[u8], deflate: bool) -> Result<(), CommandError> {
        let uncompressed_size = u32::try_from(data.len())
            .map_err(|_| CommandError::Internal(format!("{name} is too large for a ZIP")))?;
        let crc = crc32fast::hash(data);

        let (body, method) = if deflate {
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
            encoder
                .write_all(data)
                .and_then(|()| encoder.finish())
                .map(|body| (body, 8_u16))
                .map_err(|e| CommandError::Internal(format!("deflate {name}: {e}")))?
        } else {
            (data.to_vec(), 0_u16)
        };
        let compressed_size = u32::try_from(body.len())
            .map_err(|_| CommandError::Internal(format!("{name} is too large for a ZIP")))?;

        let offset = u32::try_from(self.out.len())
            .map_err(|_| CommandError::Internal("archive is too large for a ZIP".into()))?;

        let name_bytes = name.as_bytes();
        let name_len = u16::try_from(name_bytes.len())
            .map_err(|_| CommandError::Internal(format!("path too long: {name}")))?;

        self.out.extend_from_slice(&LOCAL_HEADER.to_le_bytes());
        self.out.extend_from_slice(&VERSION_NEEDED.to_le_bytes());
        self.out.extend_from_slice(&0_u16.to_le_bytes()); // flags
        self.out.extend_from_slice(&method.to_le_bytes());
        // A fixed timestamp keeps the output byte-identical for identical
        // input, which is what makes the tests able to assert on bytes at all.
        // 1980-01-01 00:00 is the epoch the format itself starts at.
        self.out.extend_from_slice(&0_u16.to_le_bytes()); // time
        self.out.extend_from_slice(&33_u16.to_le_bytes()); // date: 1980-01-01
        self.out.extend_from_slice(&crc.to_le_bytes());
        self.out.extend_from_slice(&compressed_size.to_le_bytes());
        self.out.extend_from_slice(&uncompressed_size.to_le_bytes());
        self.out.extend_from_slice(&name_len.to_le_bytes());
        self.out.extend_from_slice(&0_u16.to_le_bytes()); // extra field length
        self.out.extend_from_slice(name_bytes);
        self.out.extend_from_slice(&body);

        self.entries.push(Entry {
            name: name.to_owned(),
            crc,
            compressed_size,
            uncompressed_size,
            offset,
            method,
        });
        Ok(())
    }

    /// Write the central directory and return the finished archive.
    ///
    /// # Errors
    /// When the archive exceeds what a 32-bit ZIP can address.
    pub fn finish(mut self) -> Result<Vec<u8>, CommandError> {
        let directory_at = u32::try_from(self.out.len())
            .map_err(|_| CommandError::Internal("archive is too large for a ZIP".into()))?;

        for entry in &self.entries {
            let name_bytes = entry.name.as_bytes();
            #[allow(clippy::cast_possible_truncation)] // checked in `add`
            let name_len = name_bytes.len() as u16;
            self.out.extend_from_slice(&CENTRAL_HEADER.to_le_bytes());
            self.out.extend_from_slice(&VERSION_NEEDED.to_le_bytes()); // version made by
            self.out.extend_from_slice(&VERSION_NEEDED.to_le_bytes()); // version needed
            self.out.extend_from_slice(&0_u16.to_le_bytes()); // flags
            self.out.extend_from_slice(&entry.method.to_le_bytes());
            self.out.extend_from_slice(&0_u16.to_le_bytes()); // time
            self.out.extend_from_slice(&33_u16.to_le_bytes()); // date
            self.out.extend_from_slice(&entry.crc.to_le_bytes());
            self.out.extend_from_slice(&entry.compressed_size.to_le_bytes());
            self.out.extend_from_slice(&entry.uncompressed_size.to_le_bytes());
            self.out.extend_from_slice(&name_len.to_le_bytes());
            self.out.extend_from_slice(&0_u16.to_le_bytes()); // extra
            self.out.extend_from_slice(&0_u16.to_le_bytes()); // comment
            self.out.extend_from_slice(&0_u16.to_le_bytes()); // disk number
            self.out.extend_from_slice(&0_u16.to_le_bytes()); // internal attrs
            self.out.extend_from_slice(&0_u32.to_le_bytes()); // external attrs
            self.out.extend_from_slice(&entry.offset.to_le_bytes());
            self.out.extend_from_slice(name_bytes);
        }

        let directory_size = u32::try_from(self.out.len())
            .map_err(|_| CommandError::Internal("archive is too large for a ZIP".into()))?
            - directory_at;
        let count = u16::try_from(self.entries.len())
            .map_err(|_| CommandError::Internal("too many files for a ZIP".into()))?;

        self.out.extend_from_slice(&END_OF_CENTRAL_DIRECTORY.to_le_bytes());
        self.out.extend_from_slice(&0_u16.to_le_bytes()); // this disk
        self.out.extend_from_slice(&0_u16.to_le_bytes()); // disk with directory
        self.out.extend_from_slice(&count.to_le_bytes());
        self.out.extend_from_slice(&count.to_le_bytes());
        self.out.extend_from_slice(&directory_size.to_le_bytes());
        self.out.extend_from_slice(&directory_at.to_le_bytes());
        self.out.extend_from_slice(&0_u16.to_le_bytes()); // comment length
        Ok(self.out)
    }
}

impl Default for ZipWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Escape text for an XML text node or attribute value.
///
/// Not optional and not cosmetic: a document containing `&` or `<` produces a
/// `.docx` that every reader refuses to open, and the text comes from whatever
/// PDF the user happened to have.
#[must_use]
pub fn escape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // XML 1.0 cannot represent most control characters at all, not even
            // as a numeric reference. Dropping them is the only valid choice;
            // tab, newline and carriage return are the three that are legal.
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Read one entry back out of an archive this module wrote.
///
/// Deliberately minimal — it walks the local headers rather than the central
/// directory, and understands only the two methods [`ZipWriter`] emits. It
/// exists so the tests can assert on what a `.docx` actually contains.
///
/// A reader of mine validating a writer of mine proves nothing about the
/// format, which is why the archive is *also* checked with `unzip -t` and, in
/// the acceptance pass, opened in Word.
///
/// # Errors
/// When the archive is malformed or the entry is absent or unreadable.
pub fn read_entry(archive: &[u8], name: &str) -> Result<Vec<u8>, CommandError> {
    let mut at = 0usize;
    while at + 30 <= archive.len() {
        let signature = u32::from_le_bytes(archive[at..at + 4].try_into().unwrap_or_default());
        if signature != LOCAL_HEADER {
            break;
        }
        let read16 = |offset: usize| -> usize {
            u16::from_le_bytes(
                archive[at + offset..at + offset + 2]
                    .try_into()
                    .unwrap_or_default(),
            ) as usize
        };
        let read32 = |offset: usize| -> usize {
            u32::from_le_bytes(
                archive[at + offset..at + offset + 4]
                    .try_into()
                    .unwrap_or_default(),
            ) as usize
        };
        let method = read16(8);
        let compressed = read32(18);
        let name_len = read16(26);
        let extra_len = read16(28);
        let name_at = at + 30;
        let body_at = name_at + name_len + extra_len;
        if body_at + compressed > archive.len() {
            return Err(CommandError::Internal("truncated ZIP entry".into()));
        }
        let entry_name = std::str::from_utf8(&archive[name_at..name_at + name_len])
            .map_err(|_| CommandError::Internal("non-UTF-8 ZIP entry name".into()))?;
        if entry_name == name {
            let body = &archive[body_at..body_at + compressed];
            return match method {
                0 => Ok(body.to_vec()),
                8 => {
                    use std::io::Read;
                    let mut out = Vec::new();
                    flate2::read::DeflateDecoder::new(body)
                        .read_to_end(&mut out)
                        .map_err(|e| CommandError::Internal(format!("inflate {name}: {e}")))?;
                    Ok(out)
                }
                other => Err(CommandError::Internal(format!(
                    "unsupported ZIP method {other}"
                ))),
            };
        }
        at = body_at + compressed;
    }
    Err(CommandError::NotFound(format!("{name} is not in the archive")))
}

/// Every entry name in the archive, in the order written.
///
/// # Errors
/// When the archive is malformed.
pub fn entry_names(archive: &[u8]) -> Result<Vec<String>, CommandError> {
    let mut names = Vec::new();
    let mut at = 0usize;
    while at + 30 <= archive.len() {
        if u32::from_le_bytes(archive[at..at + 4].try_into().unwrap_or_default()) != LOCAL_HEADER {
            break;
        }
        let read16 = |offset: usize| -> usize {
            u16::from_le_bytes(archive[at + offset..at + offset + 2].try_into().unwrap_or_default())
                as usize
        };
        let compressed =
            u32::from_le_bytes(archive[at + 18..at + 22].try_into().unwrap_or_default()) as usize;
        let name_len = read16(26);
        let extra_len = read16(28);
        let name_at = at + 30;
        names.push(
            String::from_utf8_lossy(&archive[name_at..name_at + name_len]).into_owned(),
        );
        at = name_at + name_len + extra_len + compressed;
    }
    Ok(names)
}

#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_archive_round_trips() {
        let mut zip = ZipWriter::new();
        zip.add("a.txt", b"hello", true).expect("add");
        zip.add("dir/b.bin", &[0u8, 255, 1, 254], false).expect("add");
        let bytes = zip.finish().expect("finish");

        assert_eq!(read_entry(&bytes, "a.txt").expect("read"), b"hello");
        assert_eq!(read_entry(&bytes, "dir/b.bin").expect("read"), vec![0, 255, 1, 254]);
        assert_eq!(entry_names(&bytes).expect("names"), vec!["a.txt", "dir/b.bin"]);
    }

    #[test]
    fn the_signatures_and_counts_are_where_the_format_says() {
        let mut zip = ZipWriter::new();
        zip.add("a.txt", b"hello", false).expect("add");
        zip.add("b.txt", b"world", false).expect("add");
        let bytes = zip.finish().expect("finish");

        assert_eq!(&bytes[..4], &LOCAL_HEADER.to_le_bytes());
        // The end-of-central-directory record is the last 22 bytes when there
        // is no archive comment, and carries the entry count twice.
        let eocd = bytes.len() - 22;
        assert_eq!(&bytes[eocd..eocd + 4], &END_OF_CENTRAL_DIRECTORY.to_le_bytes());
        assert_eq!(u16::from_le_bytes(bytes[eocd + 8..eocd + 10].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(bytes[eocd + 10..eocd + 12].try_into().unwrap()), 2);

        // The central directory starts where the record says it does.
        let at = u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
        assert_eq!(&bytes[at..at + 4], &CENTRAL_HEADER.to_le_bytes());
    }

    #[test]
    fn a_stored_entry_keeps_its_bytes_and_a_deflated_one_shrinks() {
        let repetitive = "x".repeat(4096);
        let mut zip = ZipWriter::new();
        zip.add("stored", repetitive.as_bytes(), false).expect("add");
        zip.add("deflated", repetitive.as_bytes(), true).expect("add");
        let bytes = zip.finish().expect("finish");
        // Both read back identically...
        assert_eq!(read_entry(&bytes, "stored").unwrap(), repetitive.as_bytes());
        assert_eq!(read_entry(&bytes, "deflated").unwrap(), repetitive.as_bytes());
        // ...but only one of them is small on disk.
        assert!(bytes.len() < repetitive.len() * 2);
    }

    #[test]
    fn a_missing_entry_is_not_found_rather_than_a_panic() {
        let bytes = ZipWriter::new().finish().expect("finish");
        assert!(read_entry(&bytes, "nope.txt").is_err());
        assert!(entry_names(&bytes).expect("names").is_empty());
    }

    #[test]
    fn xml_escaping_covers_what_breaks_a_document() {
        assert_eq!(escape_xml("Revenue & Costs"), "Revenue &amp; Costs");
        assert_eq!(escape_xml("a < b > c"), "a &lt; b &gt; c");
        assert_eq!(escape_xml(r#"say "hi""#), "say &quot;hi&quot;");
        assert_eq!(escape_xml("it's"), "it&apos;s");
        // Already-escaped text is escaped again — the input is text, not XML.
        assert_eq!(escape_xml("&amp;"), "&amp;amp;");
    }

    #[test]
    fn control_characters_are_dropped_not_encoded() {
        // XML 1.0 cannot represent these at all, not even as `&#1;` — a
        // numeric reference to them is itself invalid. Dropping is the only
        // valid choice, and PDFs do contain them.
        assert_eq!(escape_xml("a\u{0}b\u{1}c\u{1f}d"), "abcd");
        // The three that are legal survive.
        assert_eq!(escape_xml("a\tb\nc\rd"), "a\tb\nc\rd");
    }

    #[test]
    fn unicode_survives_escaping() {
        assert_eq!(escape_xml("Счёт αβγ café"), "Счёт αβγ café");
    }
}
