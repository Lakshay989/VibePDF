#!/usr/bin/env python3
"""Regenerate tests/fixtures/basic/metadata.pdf.

The fixture for P6-SEC-012 ("Clean document"). It carries one of every thing
that spec line removes, and each one is tagged with a distinctive marker string
so a test can assert the *value is gone from the file* rather than the weaker
"the key we deleted is missing":

    SECRETTITLE / SECRETAUTHOR / SECRETCREATOR /
    SECRETPRODUCER / SECRETSUBJECT / SECRETKEYWORD   /Info, incl. a custom key
    SECRETCUSTOM
    SECRETXMP                                        XMP /Metadata stream
    SECRETBOOKMARK1 / SECRETBOOKMARK2                /Outlines
    SECRETFORMVALUE                                  AcroForm text field /V
    SECRETATTACHMENT                                 embedded file stream
    SECRETCOMMENT                                    /Text sticky note
    SECRETHIDDEN                                     invisible text (3 Tr)

and one string that must **survive** every toggle:

    VisibleBodyText

The two metadata stores are the point of the fixture. A cleaner that empties
/Info and leaves XMP alone looks correct in every reader that reads /Info
first, and hands the author's name to every reader that prefers XMP.

The author name is deliberately duplicated across /Info and XMP for the same
reason.

No external deps. Run from anywhere:
    python3 tests/fixtures/basic/generate-metadata.py
"""
from pathlib import Path

OUT = Path(__file__).parent / "metadata.pdf"

# 1 catalog        | 2 pages         | 3 page1        | 4 font
# 5 content        | 6 sticky note   | 7 file-attach  | 8 filespec
# 9 embedded file  | 10 outlines     | 11,12 items    | 13 acroform
# 14 text field    | 15 XMP          | 16 info

CONTENT = b"""BT /F1 18 Tf 72 700 Td (VisibleBodyText) Tj ET
BT /F1 12 Tf 3 Tr 72 650 Td (SECRETHIDDEN) Tj ET
"""

XMP = b"""<?xpacket begin="\xef\xbb\xbf" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:xmp="http://ns.adobe.com/xap/1.0/">
   <dc:title><rdf:Alt><rdf:li xml:lang="x-default">SECRETXMP Title</rdf:li></rdf:Alt></dc:title>
   <dc:creator><rdf:Seq><rdf:li>SECRETAUTHOR</rdf:li></rdf:Seq></dc:creator>
   <xmp:CreatorTool>SECRETXMP Tool</xmp:CreatorTool>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>
"""

ATTACHMENT = b"SECRETATTACHMENT payload\n"


def stream(dict_body: bytes, data: bytes) -> bytes:
    return (
        b"<< " + dict_body + b" /Length " + str(len(data)).encode() + b" >>\n"
        b"stream\n" + data + b"\nendstream"
    )


objects: list[bytes] = [
    # 1 catalog — every removable store hangs off here
    b"<< /Type /Catalog /Pages 2 0 R /Outlines 10 0 R "
    b"/Names << /EmbeddedFiles << /Names [ (secret.txt) 8 0 R ] >> >> "
    b"/AcroForm 13 0 R /Metadata 15 0 R >>",
    # 2 page tree
    b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    # 3 page — a sticky note, a file attachment, and a form widget
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
    b"/Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R "
    b"/Annots [6 0 R 7 0 R 14 0 R] >>",
    # 4 font
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    # 5 content — one visible run, one invisible (3 Tr) run
    stream(b"", CONTENT),
    # 6 sticky note — a "comment"
    b"<< /Type /Annot /Subtype /Text /Rect [200 700 224 724] "
    b"/Contents (SECRETCOMMENT) /T (Reviewer) >>",
    # 7 file attachment annotation — an "attachment"
    b"<< /Type /Annot /Subtype /FileAttachment /Rect [300 700 320 720] "
    b"/FS 8 0 R /Contents (SECRETATTACHMENT note) >>",
    # 8 file specification, referenced by both the annot and the name tree
    b"<< /Type /Filespec /F (secret.txt) /UF (secret.txt) /EF << /F 9 0 R >> >>",
    # 9 the embedded file itself
    stream(b"/Type /EmbeddedFile /Subtype /text#2Fplain", ATTACHMENT),
    # 10 outline root
    b"<< /Type /Outlines /First 11 0 R /Last 12 0 R /Count 2 >>",
    # 11, 12 bookmarks
    b"<< /Title (SECRETBOOKMARK1) /Parent 10 0 R /Next 12 0 R /Dest [3 0 R /Fit] >>",
    b"<< /Title (SECRETBOOKMARK2) /Parent 10 0 R /Prev 11 0 R /Dest [3 0 R /Fit] >>",
    # 13 AcroForm
    b"<< /Fields [14 0 R] /DA (/Helv 0 Tf 0 g) >>",
    # 14 text field + widget in one object, the common single-widget shape
    b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (secret_field) "
    b"/V (SECRETFORMVALUE) /DV (SECRETFORMVALUE) /Rect [72 600 300 620] "
    b"/F 4 /P 3 0 R >>",
    # 15 XMP — the second metadata store, carrying the same author
    stream(b"/Type /Metadata /Subtype /XML", XMP),
    # 16 /Info, including a custom key
    b"<< /Title (SECRETTITLE) /Author (SECRETAUTHOR) /Creator (SECRETCREATOR) "
    b"/Producer (SECRETPRODUCER) /Subject (SECRETSUBJECT) "
    b"/Keywords (SECRETKEYWORD) /VibeCustomKey (SECRETCUSTOM) "
    b"/CreationDate (D:20260101000000Z) >>",
]

out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
offsets = [0]
for i, body in enumerate(objects, start=1):
    offsets.append(len(out))
    out += str(i).encode() + b" 0 obj\n" + body + b"\nendobj\n"

xref_at = len(out)
n = len(objects) + 1
out += b"xref\n0 " + str(n).encode() + b"\n"
out += b"0000000000 65535 f \n"
for off in offsets[1:]:
    out += f"{off:010d} 00000 n \n".encode()
out += (
    b"trailer\n<< /Size " + str(n).encode() + b" /Root 1 0 R /Info 16 0 R "
    b"/ID [<0123456789ABCDEF0123456789ABCDEF> <0123456789ABCDEF0123456789ABCDEF>] >>\n"
    b"startxref\n" + str(xref_at).encode() + b"\n%%EOF\n"
)

OUT.write_bytes(bytes(out))
print(f"wrote {OUT} ({len(out)} bytes)")
