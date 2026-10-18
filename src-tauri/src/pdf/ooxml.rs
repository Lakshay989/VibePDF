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

use std::collections::HashMap;
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

/// Where one entry is, from the central directory.
#[derive(Clone, Copy, Debug)]
struct EntryRecord {
    method: u16,
    compressed: usize,
    uncompressed: usize,
    local_header: usize,
}

/// The most one entry may inflate to. A `.docx` part is a few megabytes; a
/// crafted archive claiming gigabytes is refused instead of inflated.
pub const MAX_ENTRY_BYTES: usize = 256 << 20;

/// A ZIP archive read through its **central directory** (P7.C1b1).
///
/// The central directory is the archive's index, and the only reliable one.
/// Many writers — `LibreOffice`, Java and `.NET` packaging, anything streaming —
/// write each entry's sizes *after* its data, in a data descriptor, and leave
/// zeros in the local header; walking local headers reads those as empty.
pub struct ZipArchive<'a> {
    data: &'a [u8],
    entries: HashMap<String, EntryRecord>,
}

fn le16(data: &[u8], at: usize) -> Option<usize> {
    Some(usize::from(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?)))
}

fn le32(data: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?)).ok()
}

impl<'a> ZipArchive<'a> {
    /// Index `data`'s entries.
    ///
    /// # Errors
    /// When there is no end-of-central-directory record, the directory is
    /// damaged, or the archive is ZIP64 (over 4 GB or 65 535 entries).
    pub fn parse(data: &'a [u8]) -> Result<Self, CommandError> {
        let bad = |what: &str| CommandError::InvalidInput(format!("not a readable ZIP archive: {what}"));
        // The record is 22 bytes plus a comment of up to 65 535.
        let last = data.len().checked_sub(22).ok_or_else(|| bad("too short"))?;
        let earliest = last.saturating_sub(65_535);
        let eocd = (earliest..=last)
            .rev()
            .find(|&at| le32(data, at) == usize::try_from(END_OF_CENTRAL_DIRECTORY).ok())
            .ok_or_else(|| bad("no end-of-directory record"))?;
        let count = le16(data, eocd + 10).ok_or_else(|| bad("truncated"))?;
        let offset = le32(data, eocd + 16).ok_or_else(|| bad("truncated"))?;
        if count == 0xFFFF || offset == 0xFFFF_FFFF {
            return Err(bad("ZIP64 archives aren't supported"));
        }
        let mut entries = HashMap::with_capacity(count);
        let mut at = offset;
        for _ in 0..count {
            if le32(data, at) != usize::try_from(CENTRAL_HEADER).ok() {
                return Err(bad("damaged central directory"));
            }
            let field = |o: usize| le16(data, at + o).ok_or_else(|| bad("truncated"));
            let wide = |o: usize| le32(data, at + o).ok_or_else(|| bad("truncated"));
            let (name_len, extra_len, comment_len) = (field(28)?, field(30)?, field(32)?);
            let name = data.get(at + 46..at + 46 + name_len).ok_or_else(|| bad("truncated"))?;
            entries.insert(
                String::from_utf8_lossy(name).into_owned(),
                EntryRecord { method: u16::try_from(field(10)?).unwrap_or(u16::MAX), compressed: wide(20)?, uncompressed: wide(24)?, local_header: wide(42)? },
            );
            at += 46 + name_len + extra_len + comment_len;
        }
        Ok(Self { data, entries })
    }

    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// Entry names, in the order their data appears in the archive.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        let mut named: Vec<(&String, usize)> = self.entries.iter().map(|(n, r)| (n, r.local_header)).collect();
        named.sort_by_key(|(_, at)| *at);
        named.into_iter().map(|(n, _)| n.clone()).collect()
    }

    /// The entry's bytes, inflated.
    ///
    /// # Errors
    /// When it is absent, damaged, uses a method other than stored or deflate,
    /// or would inflate past [`MAX_ENTRY_BYTES`].
    pub fn read(&self, name: &str) -> Result<Vec<u8>, CommandError> {
        self.read_capped(name, MAX_ENTRY_BYTES)
    }

    fn read_capped(&self, name: &str, cap: usize) -> Result<Vec<u8>, CommandError> {
        let record = self.entries.get(name).ok_or_else(|| CommandError::NotFound(format!("{name} is not in the archive")))?;
        let bad = |what: &str| CommandError::InvalidInput(format!("{name}: {what}"));
        let too_big = || CommandError::InvalidInput(format!("{name} is too large when unpacked (over {} MB); it may be a damaged or hostile file.", cap >> 20));
        if record.uncompressed > cap {
            return Err(too_big());
        }
        let at = record.local_header;
        if le32(self.data, at) != usize::try_from(LOCAL_HEADER).ok() {
            return Err(bad("damaged entry header"));
        }
        let body_at = at + 30 + le16(self.data, at + 26).ok_or_else(|| bad("truncated"))? + le16(self.data, at + 28).ok_or_else(|| bad("truncated"))?;
        let body = self.data.get(body_at..body_at + record.compressed).ok_or_else(|| bad("truncated"))?;
        match record.method {
            0 => Ok(body.to_vec()),
            8 => {
                use std::io::Read;
                // The declared size is not trusted: read at most one byte past the cap.
                let mut out = Vec::new();
                flate2::read::DeflateDecoder::new(body)
                    .take(u64::try_from(cap).unwrap_or(u64::MAX) + 1)
                    .read_to_end(&mut out)
                    .map_err(|e| bad(&format!("could not be inflated: {e}")))?;
                if out.len() > cap {
                    return Err(too_big());
                }
                Ok(out)
            }
            other => Err(bad(&format!("uses compression method {other}, which isn't supported"))),
        }
    }
}

/// Read one entry out of an archive.
///
/// # Errors
/// When the archive is malformed or the entry is absent or unreadable.
pub fn read_entry(archive: &[u8], name: &str) -> Result<Vec<u8>, CommandError> {
    ZipArchive::parse(archive)?.read(name)
}

/// Every entry name in the archive, in the order their data appears.
///
/// # Errors
/// When the archive is malformed.
pub fn entry_names(archive: &[u8]) -> Result<Vec<String>, CommandError> {
    Ok(ZipArchive::parse(archive)?.names())
}

#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;

    /// A one-entry archive written the way streaming writers do it: general
    /// purpose bit 3 set, zeros for the CRC and sizes in the local header, and
    /// the real values in a data descriptor after the data. Only the central
    /// directory carries them where a reader can find them first.
    fn with_data_descriptor(name: &str, data: &[u8]) -> Vec<u8> {
        let mut compressed = Vec::new();
        {
            let mut encoder = DeflateEncoder::new(&mut compressed, Compression::default());
            encoder.write_all(data).unwrap();
            encoder.finish().unwrap();
        }
        let crc = crc32fast::hash(data);
        let (csize, usize_) = (u32::try_from(compressed.len()).unwrap(), u32::try_from(data.len()).unwrap());
        let name_len = u16::try_from(name.len()).unwrap();
        let mut out = Vec::new();
        out.extend_from_slice(&LOCAL_HEADER.to_le_bytes());
        out.extend_from_slice(&[20, 0, 8, 0, 8, 0, 0, 0, 0, 0]); // version, flags (bit 3), deflate, time, date
        out.extend_from_slice(&[0; 12]); // CRC and both sizes: deferred
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&compressed);
        out.extend_from_slice(&0x0807_4b50u32.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&csize.to_le_bytes());
        out.extend_from_slice(&usize_.to_le_bytes());
        let central_at = u32::try_from(out.len()).unwrap();
        out.extend_from_slice(&CENTRAL_HEADER.to_le_bytes());
        out.extend_from_slice(&[20, 0, 20, 0, 8, 0, 8, 0, 0, 0, 0, 0]);
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&csize.to_le_bytes());
        out.extend_from_slice(&usize_.to_le_bytes());
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&[0; 12]); // extra, comment, disk, attributes
        out.extend_from_slice(&0u32.to_le_bytes()); // local header at 0
        out.extend_from_slice(name.as_bytes());
        let central_len = u32::try_from(out.len()).unwrap() - central_at;
        out.extend_from_slice(&END_OF_CENTRAL_DIRECTORY.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0, 1, 0, 1, 0]);
        out.extend_from_slice(&central_len.to_le_bytes());
        out.extend_from_slice(&central_at.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn sizes_kept_in_a_data_descriptor_are_found_through_the_directory() {
        let archive = with_data_descriptor("word/document.xml", b"<w:document/> repeated repeated repeated");
        let zip = ZipArchive::parse(&archive).unwrap();
        assert_eq!(zip.read("word/document.xml").unwrap(), b"<w:document/> repeated repeated repeated");
        assert_eq!(zip.names(), vec!["word/document.xml"]);
    }

    // The declared size is not trusted: the inflater itself stops at the cap.
    #[test]
    fn an_entry_that_inflates_past_the_cap_is_refused() {
        let archive = with_data_descriptor("bomb.xml", &vec![0u8; 3 << 20]);
        let zip = ZipArchive::parse(&archive).unwrap();
        let err = zip.read_capped("bomb.xml", 1 << 20).unwrap_err();
        assert!(matches!(err, CommandError::InvalidInput(ref m) if m.contains("too large when unpacked")), "{err:?}");
        assert_eq!(zip.read_capped("bomb.xml", 4 << 20).unwrap().len(), 3 << 20);
    }

    #[test]
    fn something_that_is_not_a_zip_is_an_error() {
        assert!(ZipArchive::parse(b"not an archive at all, just words").is_err());
        assert!(ZipArchive::parse(b"").is_err());
    }

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
