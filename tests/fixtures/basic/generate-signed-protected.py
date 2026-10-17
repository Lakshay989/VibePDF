"""Build `signed-protected.pdf` and `signed-protected-rc4.pdf`: `hello.pdf`
protected with an open password, then signed in an incremental update — both
steps by pyHanko.

That order (protect, then sign) is how Acrobat signs a protected document, and
it is a file VibePDF cannot make itself: its Sign refuses protected documents.
pyHanko is an independent implementation, so the fixture also checks VibePDF's
verifier against someone else's idea of a correct signed, encrypted file —
including the rule that a signature's `/Contents` is never encrypted while
every other string in the file is.

Run once and commit the output; it needs pyHanko, which CI does not have:

    "Sample PDFs/tools/pyhanko-venv/bin/python" -I \\
        tests/fixtures/basic/generate-signed-protected.py

Open password `open-me`, owner password `owner-pw`. One file per cipher,
because they fail differently: lopdf decrypts every string it reads, a
signature's `/Contents` included, and the two ciphers differ in what that does
to a value that was never encrypted. AES rejects most such lengths and leaves
the value alone; RC4 always "succeeds" and returns garbage. PDF 2.0's MAC
(ISO 32004) is off: neither PDFium nor lopdf reads it, and these fixtures are
about signatures, not MACs. The signing time and the IVs vary, so the bytes
differ per run.
"""

import io
from pathlib import Path

from pyhanko.pdf_utils.crypt import StandardSecurityHandler
from pyhanko.pdf_utils.crypt.standard import StandardSecuritySettingsRevision
from pyhanko.pdf_utils.incremental_writer import IncrementalPdfFileWriter
from pyhanko.pdf_utils.reader import PdfFileReader
from pyhanko.pdf_utils.writer import copy_into_new_writer
from pyhanko.sign import fields, signers

HERE = Path(__file__).resolve().parent
CERTS = HERE.parent / "certs"
OPEN_PASSWORD = "open-me"
OWNER_PASSWORD = "owner-pw"


def protect(src: Path, cipher: str) -> bytes:
    with src.open("rb") as f:
        writer = copy_into_new_writer(PdfFileReader(f))
    if cipher == "aes256":
        writer.encrypt(OWNER_PASSWORD, OPEN_PASSWORD, pdf_mac=False)
    else:
        # pyHanko keeps legacy encryption off its public writer API on purpose;
        # this is the documented builder plus the writer's own assignment hook.
        handler = StandardSecurityHandler.build_from_pw_legacy(
            StandardSecuritySettingsRevision.RC4_EXTENDED,
            writer.document_id[0],
            OWNER_PASSWORD,
            OPEN_PASSWORD,
            keylen_bytes=16,
            use_aes128=False,
        )
        writer._assign_security_handler(handler)
    out = io.BytesIO()
    writer.write(out)
    return out.getvalue()


def sign(protected: bytes) -> bytes:
    writer = IncrementalPdfFileWriter(io.BytesIO(protected))
    writer.encrypt(OPEN_PASSWORD)
    signer = signers.SimpleSigner.load(
        key_file=str(CERTS / "signer-key.pem"),
        cert_file=str(CERTS / "signer-cert.pem"),
    )
    meta = signers.PdfSignatureMetadata(field_name="Approval", reason="Approved for release")
    fields.append_signature_field(writer, fields.SigFieldSpec("Approval", box=(72, 72, 272, 132)))
    out = io.BytesIO()
    signers.sign_pdf(writer, meta, signer=signer, output=out)
    return out.getvalue()


def main() -> None:
    for cipher, name in (("aes256", "signed-protected.pdf"), ("rc4", "signed-protected-rc4.pdf")):
        signed = sign(protect(HERE / "hello.pdf", cipher))
        target = HERE / name
        target.write_bytes(signed)
        print(f"wrote {target} ({len(signed)} bytes)")


if __name__ == "__main__":
    main()
