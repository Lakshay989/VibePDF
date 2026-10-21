"""Build the `permissions-only-*.pdf` fixtures: `hello.pdf` protected by pyHanko
with an owner (permissions) password and **no open password**, once per
standard security handler.

Such a file opens for anyone but restricts what a reader may do, and removing
that protection must still need the owner password (P6-SEC-008). lopdf opens
these with the empty password and drops the encryption dictionary, so Unlock
checks the owner password against a dictionary rebuilt from lopdf's decryption
state. pyHanko is an independent implementation: these files check that rebuilt
check against someone else's /O and /U values, for every handler.

Before writing a file, pyHanko itself is asked what each password is: the
owner password must authenticate as OWNER, the empty one as USER, and a wrong
one not at all. A fixture that fails its own oracle is never written.

Run once and commit the output; it needs pyHanko, which CI does not have:

    "Sample PDFs/tools/pyhanko-venv/bin/python" -I \\
        tests/fixtures/basic/generate-permissions-only.py

Owner password `owner-pw`. Printing is allowed and everything else refused, so
/P is a realistic restriction rather than all bits set. PDF 2.0's MAC is off,
as in generate-signed-protected.py. The IVs vary, so the bytes differ per run.

`permissions-only-aes256-length.pdf` keeps pyHanko's default top-level
`/Length 256`, which pyHanko writes for Acrobat's sake and lopdf cannot decrypt
(P6.C2's known gap). It pins that Unlock names that variant instead of calling
the owner password wrong; `permissions-only-aes256.pdf` leaves it out.
"""

import io
from pathlib import Path

from pyhanko.pdf_utils.crypt import StandardSecurityHandler
from pyhanko.pdf_utils.crypt.api import AuthStatus
from pyhanko.pdf_utils.crypt.permissions import StandardPermissions
from pyhanko.pdf_utils.crypt.standard import StandardSecuritySettingsRevision
from pyhanko.pdf_utils.reader import PdfFileReader
from pyhanko.pdf_utils.writer import copy_into_new_writer

HERE = Path(__file__).resolve().parent
OWNER_PASSWORD = "owner-pw"
# Bit 13 (TOLERATE_MISSING_PDF_MAC) is not a restriction: without it pyHanko
# insists on the MAC that these fixtures leave off.
PERMS = (
    StandardPermissions.ALLOW_PRINTING
    | StandardPermissions.ALLOW_HIGH_QUALITY_PRINTING
    | StandardPermissions.ALLOW_ASSISTIVE_TECHNOLOGY
    | StandardPermissions.TOLERATE_MISSING_PDF_MAC
)

# name -> (revision, key length in bytes, AES-128?) for the legacy handlers.
LEGACY = {
    "permissions-only-rc4-40.pdf": (StandardSecuritySettingsRevision.RC4_BASIC, 5, False),
    "permissions-only-rc4-128.pdf": (StandardSecuritySettingsRevision.RC4_EXTENDED, 16, False),
    "permissions-only-aes128.pdf": (StandardSecuritySettingsRevision.RC4_OR_AES128, 16, True),
}

# name -> whether to write the optional top-level /Length for AES-256.
AES256 = {
    "permissions-only-aes256.pdf": False,
    "permissions-only-aes256-length.pdf": True,
}


def writer_for_hello():
    with (HERE / "hello.pdf").open("rb") as f:
        return copy_into_new_writer(PdfFileReader(f))


def write(writer) -> bytes:
    out = io.BytesIO()
    writer.write(out)
    return out.getvalue()


def legacy(revision, keylen_bytes: int, use_aes128: bool) -> bytes:
    writer = writer_for_hello()
    # pyHanko keeps legacy encryption off its public writer API on purpose;
    # this is the documented builder plus the writer's own assignment hook.
    handler = StandardSecurityHandler.build_from_pw_legacy(
        revision,
        writer.document_id[0],
        OWNER_PASSWORD,
        "",
        keylen_bytes=keylen_bytes,
        use_aes128=use_aes128,
        perms=PERMS,
    )
    writer._assign_security_handler(handler)
    return write(writer)


def aes256(with_length: bool) -> bytes:
    writer = writer_for_hello()
    writer.encrypt(OWNER_PASSWORD, "", perms=PERMS, pdf_mac=False, compat_entries=with_length)
    return write(writer)


def status(data: bytes, password: str) -> AuthStatus:
    reader = PdfFileReader(io.BytesIO(data))
    try:
        return reader.decrypt(password).status
    except Exception:  # pyHanko raises on a password it rejects
        return AuthStatus.FAILED


def check(name: str, data: bytes) -> None:
    got = {pw: status(data, pw) for pw in (OWNER_PASSWORD, "", "not-the-owner")}
    want = {OWNER_PASSWORD: AuthStatus.OWNER, "": AuthStatus.USER, "not-the-owner": AuthStatus.FAILED}
    if got != want:
        raise SystemExit(f"{name}: pyHanko disagrees with the fixture's passwords: {got}")


def main() -> None:
    files = {name: legacy(*args) for name, args in LEGACY.items()}
    files |= {name: aes256(with_length) for name, with_length in AES256.items()}
    for name, data in files.items():
        check(name, data)
        target = HERE / name
        target.write_bytes(data)
        print(f"wrote {target} ({len(data)} bytes)")


if __name__ == "__main__":
    main()
