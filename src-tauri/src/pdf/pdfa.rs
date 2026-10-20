//! SPEC: P7-OCR-009 (P7.C1d) — PDF/A-2u for "create PDF from files": a PDF
//! meant to be opened, unchanged, decades from now.
//!
//! **What this module adds** to a document built by [`crate::pdf::create`]:
//! an sRGB output intent carrying the ICC's own `sRGB2014.icc`, so every
//! `DeviceRGB` colour has one defined meaning; uncompressed XMP metadata
//! saying which PDF/A part and level the file claims; Info entries that say
//! the same as the XMP (PDF/A checks they agree); and a file identifier.
//!
//! **What the rest of creation guarantees in this mode:** every font embedded
//! with a `/ToUnicode` map (the "u": text stays searchable), no `DeviceCMYK`
//! (a CMYK JPEG is converted to RGB rather than copied in), and no character
//! drawn as the `.notdef` box, which PDF/A forbids — such a character is
//! refused by name instead.
//!
//! **Checked by veraPDF**, the PDF Association's reference validator, on the
//! file as it is finally written, after the `PDFium` save path: see
//! `tests/create_pdfa.rs`.

use std::time::{SystemTime, UNIX_EPOCH};

use lopdf::{dictionary, Document, Object, ObjectId, Stream, StringFormat};

use crate::error::CommandError;

/// The International Color Consortium's sRGB profile (ICC v2, 3,024 bytes),
/// from <https://registry.color.org/rgb-registry/srgbprofiles>. It "may be
/// copied, distributed, embedded, made, used, and sold without restriction";
/// see THIRD-PARTY-NOTICES.md.
pub(crate) const SRGB2014: &[u8] = include_bytes!("../../resources/icc/sRGB2014.icc");

const PRODUCER: &str = "VibePDF";

/// `at` in UTC: year, month, day, hour, minute, second.
fn utc(at: SystemTime) -> [i64; 6] {
    let seconds = at.duration_since(UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Howard Hinnant's civil-from-days, for the proleptic Gregorian calendar.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    [year, month, day, rest / 3600, rest % 3600 / 60, rest % 60]
}

/// A PDF date: `D:YYYYMMDDHHmmSS+00'00'`.
fn pdf_date([y, mo, d, h, mi, s]: [i64; 6]) -> String {
    format!("D:{y:04}{mo:02}{d:02}{h:02}{mi:02}{s:02}+00'00'")
}

/// The same instant as XMP writes it: `YYYY-MM-DDTHH:mm:SS+00:00`.
fn xmp_date([y, mo, d, h, mi, s]: [i64; 6]) -> String {
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}+00:00")
}

fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // XML 1.0 can't carry most control characters at all.
            c if c.is_control() && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// A PDF text string: plain when it's printable ASCII, else UTF-16BE with a
/// byte-order mark, so any title survives.
fn text_string(text: &str) -> Object {
    if text.chars().all(|c| c.is_ascii() && !c.is_ascii_control()) {
        Object::string_literal(text)
    } else {
        let mut bytes = vec![0xFE, 0xFF];
        bytes.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
        Object::String(bytes, StringFormat::Hexadecimal)
    }
}

/// The XMP packet: PDF/A-2u's identification, and the title, tool and dates
/// the Info dictionary also carries.
fn xmp_packet(title: &str, when: [i64; 6]) -> String {
    let (title, date) = (xml_escape(title), xmp_date(when));
    format!(
        "<?xpacket begin=\"\u{FEFF}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
 <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
  <rdf:Description rdf:about=\"\"\n\
    xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\"\n\
    xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n\
    xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"\n\
    xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\">\n\
   <pdfaid:part>2</pdfaid:part>\n\
   <pdfaid:conformance>U</pdfaid:conformance>\n\
   <dc:format>application/pdf</dc:format>\n\
   <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{title}</rdf:li></rdf:Alt></dc:title>\n\
   <xmp:CreatorTool>{PRODUCER}</xmp:CreatorTool>\n\
   <xmp:CreateDate>{date}</xmp:CreateDate>\n\
   <xmp:ModifyDate>{date}</xmp:ModifyDate>\n\
   <xmp:MetadataDate>{date}</xmp:MetadataDate>\n\
   <pdf:Producer>{PRODUCER}</pdf:Producer>\n\
  </rdf:Description>\n\
 </rdf:RDF>\n\
</x:xmpmeta>\n\
<?xpacket end=\"w\"?>"
    )
}

/// SPEC: P7-OCR-009 — make the document `catalog` and `info` belong to a
/// PDF/A-2u file: the output intent, the XMP metadata, matching Info entries
/// and a file identifier. `title` names the document; `at` is when it was made.
///
/// # Errors
/// When `catalog` or `info` isn't a dictionary in `doc`.
pub(crate) fn mark_archival(doc: &mut Document, catalog: ObjectId, info: ObjectId, title: &str, at: SystemTime) -> Result<(), CommandError> {
    let when = utc(at);

    let profile = doc.add_object(Stream::new(dictionary! { "N" => 3 }, SRGB2014.to_vec()));
    let intent = dictionary! {
        "Type" => "OutputIntent",
        "S" => "GTS_PDFA1",
        "OutputConditionIdentifier" => Object::string_literal("sRGB IEC61966-2.1"),
        "RegistryName" => Object::string_literal("http://www.color.org"),
        "Info" => Object::string_literal("sRGB2014"),
        "DestOutputProfile" => profile,
    };
    // PDF/A: a metadata stream is never compressed, so it can be read as text.
    let metadata = doc.add_object(Stream::new(dictionary! { "Type" => "Metadata", "Subtype" => "XML" }, xmp_packet(title, when).into_bytes()));

    let not_a_dictionary = |what: &str| CommandError::Internal(format!("the new PDF's {what} isn't a dictionary"));
    let catalog = doc.get_object_mut(catalog).and_then(Object::as_dict_mut).map_err(|_| not_a_dictionary("catalog"))?;
    catalog.set("OutputIntents", vec![Object::Dictionary(intent)]);
    catalog.set("Metadata", metadata);

    let date = Object::string_literal(pdf_date(when));
    let info = doc.get_object_mut(info).and_then(Object::as_dict_mut).map_err(|_| not_a_dictionary("information"))?;
    info.set("Title", text_string(title));
    info.set("Producer", Object::string_literal(PRODUCER));
    info.set("Creator", Object::string_literal(PRODUCER));
    info.set("CreationDate", date.clone());
    info.set("ModDate", date);

    // Both halves are the same for a file that has never been changed.
    let id = uuid::Uuid::new_v4().as_bytes().to_vec();
    doc.trailer.set("ID", vec![Object::String(id.clone(), StringFormat::Hexadecimal), Object::String(id, StringFormat::Hexadecimal)]);
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(seconds: u64) -> [i64; 6] {
        utc(UNIX_EPOCH + Duration::from_secs(seconds))
    }

    #[test]
    fn dates_are_utc_civil_time() {
        // Cross-checked with Python's datetime.fromtimestamp(t, timezone.utc).
        assert_eq!(at(0), [1970, 1, 1, 0, 0, 0]);
        assert_eq!(at(1_760_000_000), [2025, 10, 9, 8, 53, 20]);
        assert_eq!(at(951_782_400), [2000, 2, 29, 0, 0, 0], "a leap day");
        assert_eq!(at(4_102_444_799), [2099, 12, 31, 23, 59, 59]);
        assert_eq!(pdf_date(at(1_760_000_000)), "D:20251009085320+00'00'");
        assert_eq!(xmp_date(at(1_760_000_000)), "2025-10-09T08:53:20+00:00");
    }

    #[test]
    fn the_profile_is_the_iccs_srgb() {
        assert_eq!(SRGB2014.len(), 3024);
        assert_eq!(&SRGB2014[36..40], b"acsp", "an ICC profile");
        assert_eq!(&SRGB2014[16..20], b"RGB ", "of an RGB colour space");
        assert_eq!(SRGB2014[8], 2, "version 2, which every PDF/A part accepts");
    }

    #[test]
    fn the_xmp_is_well_formed_and_escapes_the_title() {
        let packet = xmp_packet("Q3 <draft> & \"notes\" \u{7}", at(0));
        let body = packet.split_once("?>").map(|(_, rest)| rest).unwrap().rsplit_once("<?xpacket").unwrap().0;
        crate::pdf::xml_tree::parse(body).expect("well-formed XML");
        assert!(packet.contains("Q3 &lt;draft&gt; &amp; &quot;notes&quot; </rdf:li>"), "{packet}");
        assert!(packet.contains("<pdfaid:part>2</pdfaid:part>") && packet.contains("<pdfaid:conformance>U</pdfaid:conformance>"));
        assert!(packet.contains("<xmp:CreateDate>1970-01-01T00:00:00+00:00</xmp:CreateDate>"));
    }

    #[test]
    fn a_title_outside_ascii_is_written_as_utf16() {
        assert_eq!(text_string("Plain"), Object::string_literal("Plain"));
        let Object::String(bytes, _) = text_string("Café") else { panic!() };
        assert_eq!(bytes, [0xFE, 0xFF, 0, b'C', 0, b'a', 0, b'f', 0, 0xE9]);
    }
}
