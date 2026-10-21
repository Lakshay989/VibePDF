"""Build `signed-unicode-reason.pdf`: `hello.pdf` signed by pyHanko, unprotected,
with a reason that is not ASCII.

A PDF text string is PDFDocEncoding or UTF-16BE with a byte-order mark (PDF 2.0
adds UTF-8 with one), never bare UTF-8. The check mark in REASON is outside
PDFDocEncoding, so pyHanko has to write the reason as UTF-16BE. VibePDF's
verifier read every text string as UTF-8 until 2026-10-21, which turned this
into garbage. pyHanko is an independent implementation, so the fixture checks
the verifier against someone else's encoding of a real non-ASCII reason.

Before writing, pyHanko reads the reason back and must get REASON exactly.

Run once and commit the output; it needs pyHanko, which CI does not have:

    "Sample PDFs/tools/pyhanko-venv/bin/python" -I \\
        tests/fixtures/basic/generate-signed-unicode-reason.py

The signing time varies, so the bytes differ per run.
"""

import io
from pathlib import Path

from pyhanko.pdf_utils.incremental_writer import IncrementalPdfFileWriter
from pyhanko.pdf_utils.reader import PdfFileReader
from pyhanko.sign import fields, signers

HERE = Path(__file__).resolve().parent
CERTS = HERE.parent / "certs"
REASON = "Freigabe für Druck ✓"


def sign(source: bytes) -> bytes:
    writer = IncrementalPdfFileWriter(io.BytesIO(source))
    signer = signers.SimpleSigner.load(
        key_file=str(CERTS / "signer-key.pem"),
        cert_file=str(CERTS / "signer-cert.pem"),
    )
    meta = signers.PdfSignatureMetadata(field_name="Approval", reason=REASON)
    fields.append_signature_field(writer, fields.SigFieldSpec("Approval", box=(72, 72, 272, 132)))
    out = io.BytesIO()
    signers.sign_pdf(writer, meta, signer=signer, output=out)
    return out.getvalue()


def main() -> None:
    signed = sign((HERE / "hello.pdf").read_bytes())
    (sig,) = PdfFileReader(io.BytesIO(signed)).embedded_signatures
    got = str(sig.sig_object["/Reason"])
    if got != REASON:
        raise SystemExit(f"pyHanko reads the reason back as {got!r}, not {REASON!r}")
    target = HERE / "signed-unicode-reason.pdf"
    target.write_bytes(signed)
    print(f"wrote {target} ({len(signed)} bytes)")


if __name__ == "__main__":
    main()
