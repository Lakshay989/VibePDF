// SPEC: P6-SEC-005 (P6.B1a) — sign a copy of the document with a certificate.
//
// **This writes a copy, and that is not a stylistic choice.** Saving a document
// re-serialises it, rewriting every byte offset; a signature covers exact
// bytes. Signing in place would hand back a file that the next Save silently
// un-signs — still showing a signature, no longer valid. So the signed document
// leaves as a separate file and the one you have open is untouched. The dialog
// says so, because "why is there a second file?" is a better question for a
// user to have than "why did my signature stop working?".
//
// Distinct from `SignatureDialog`, which manages *pictures* of signatures. That
// one draws; this one signs. Keeping them apart is the point: a picture of a
// signature and a cryptographic signature make very different claims, and a UI
// that blurred them would be the most misleading thing in the app.

import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";

import { basename } from "@/app/paths";
import { reportError } from "@/app/report-error";
import {
  type DocMdpLevel,
  type DocumentId,
  signPdf,
  unsignedSignatureFields,
} from "@/ipc/pdf";
import { pdfDate } from "@/tools/sign/pdf-date";

/**
 * SPEC: P6-SEC-005 — the certification levels, plainest-language first.
 *
 * "Approval" is the default and the common case: it says *this person signed
 * this*, and says nothing about what may happen next. Certification says
 * something stronger about the whole document, so it is opt-in.
 */
const CERTIFY_CHOICES: ReadonlyArray<{ value: DocMdpLevel | ""; label: string }> = [
  { value: "", label: "Sign only — don't restrict later changes" },
  { value: "formFillingAndAnnotations", label: "Certify: allow form filling and comments" },
  { value: "formFilling", label: "Certify: allow form filling only" },
  { value: "noChanges", label: "Certify: allow no changes at all" },
];

interface Props {
  open: boolean;
  documentId: DocumentId;
  /** Name of the open file, used to suggest an output name. */
  documentName?: string;
  onClose: () => void;
}

export function SignDialog({ open, documentId, documentName, onClose }: Props) {
  const [certificate, setCertificate] = useState<string | null>(null);
  const [password, setPassword] = useState("");
  const [reason, setReason] = useState("");
  const [location, setLocation] = useState("");
  const [name, setName] = useState("");
  const [certify, setCertify] = useState<DocMdpLevel | "">("");
  // SPEC: P6-SEC-004 (P6.A5b) — "" means add a new invisible field.
  const [field, setField] = useState("");
  const [fields, setFields] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);

  // Which fields the document offers decides whether this dialog shows a field
  // picker at all. Most documents have none.
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    void unsignedSignatureFields(documentId)
      .then((names) => {
        if (cancelled) return;
        setFields(names);
        // Default to the first empty field when there is one: a document that
        // came with a signature box is asking to be signed in that box, and an
        // invisible signature elsewhere would leave it looking unsigned.
        setField(names[0] ?? "");
      })
      .catch(() => {
        // Not being able to list fields must not stop someone signing.
        if (!cancelled) setFields([]);
      });
    return () => {
      cancelled = true;
    };
  }, [open, documentId]);

  if (!open) return null;

  const canSign = certificate !== null && password.length > 0 && !busy;

  const reset = () => {
    // The password does not outlive the dialog. Nothing here is stored, and
    // keeping a certificate password in memory past its use is free risk.
    setCertificate(null);
    setPassword("");
    setReason("");
    setLocation("");
    setName("");
    setCertify("");
    setField(fields[0] ?? "");
  };

  const chooseCertificate = () => {
    void (async () => {
      try {
        const picked = await openFileDialog({
          multiple: false,
          filters: [{ name: "Certificate", extensions: ["pfx", "p12"] }],
        });
        if (typeof picked === "string") setCertificate(picked);
      } catch (err) {
        reportError("Couldn't open the certificate picker", err);
      }
    })();
  };

  const sign = () => {
    void (async () => {
      if (certificate === null) return;
      setBusy(true);
      try {
        const suggested = documentName ? basename(documentName) : "signed.pdf";
        const path = await saveFileDialog({
          defaultPath: suggested.replace(/(\.pdf)?$/i, "-signed.pdf"),
          filters: [{ name: "PDF", extensions: ["pdf"] }],
        });
        if (!path) return; // cancelled

        await signPdf(documentId, path, certificate, password, {
          signedAt: pdfDate(new Date()),
          reason: reason.length > 0 ? reason : null,
          location: location.length > 0 ? location : null,
          name: name.length > 0 ? name : null,
          certify: certify === "" ? null : certify,
          target:
            field === "" ? { kind: "newField" } : { kind: "existingField", name: field },
        });
        reset();
        onClose();
      } catch (err) {
        // The message never contains the password — see `pdf_sign_document`.
        reportError("Couldn't sign the document", err);
      } finally {
        setBusy(false);
      }
    })();
  };

  return (
    <div
      role="dialog"
      aria-label="Sign with a certificate"
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/40"
    >
      <div className="w-[460px] rounded-lg bg-white p-4 shadow-xl dark:bg-neutral-900">
        <h2 className="mb-1 text-sm font-medium">Sign with a certificate</h2>
        <p className="mb-3 text-xs text-neutral-500">
          Writes a signed copy. The document you have open is not changed — a signature
          covers exact bytes, so saving over it would break it.
        </p>

        <div className="mb-2 flex flex-col gap-0.5">
          <span className="text-xs text-neutral-500">Certificate (.pfx or .p12)</span>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={chooseCertificate}
              className="rounded border border-neutral-300 px-2 py-1 text-xs dark:border-neutral-700"
            >
              Choose…
            </button>
            <span className="truncate text-xs text-neutral-500" title={certificate ?? ""}>
              {certificate ? basename(certificate) : "No certificate chosen"}
            </span>
          </div>
        </div>

        <label className="mb-3 flex flex-col gap-0.5">
          <span className="text-xs text-neutral-500">Certificate password</span>
          <input
            type="password"
            aria-label="Certificate password"
            autoComplete="off"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="w-full rounded border border-neutral-300 px-2 py-1 text-sm dark:border-neutral-700 dark:bg-neutral-900"
          />
        </label>

        <label className="mb-2 flex flex-col gap-0.5">
          <span className="text-xs text-neutral-500">Reason (optional)</span>
          <input
            type="text"
            aria-label="Reason"
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            placeholder="I approve this document"
            className="w-full rounded border border-neutral-300 px-2 py-1 text-sm dark:border-neutral-700 dark:bg-neutral-900"
          />
        </label>

        <label className="mb-2 flex flex-col gap-0.5">
          <span className="text-xs text-neutral-500">Location (optional)</span>
          <input
            type="text"
            aria-label="Location"
            value={location}
            onChange={(e) => setLocation(e.target.value)}
            className="w-full rounded border border-neutral-300 px-2 py-1 text-sm dark:border-neutral-700 dark:bg-neutral-900"
          />
        </label>

        <label className="mb-1 flex flex-col gap-0.5">
          <span className="text-xs text-neutral-500">Name shown on the signature (optional)</span>
          <input
            type="text"
            aria-label="Name shown on the signature"
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="w-full rounded border border-neutral-300 px-2 py-1 text-sm dark:border-neutral-700 dark:bg-neutral-900"
          />
        </label>
        <p className="mb-3 text-xs text-neutral-500">
          A display name only. Who actually signed is decided by the certificate.
        </p>

        {fields.length > 0 ? (
          <label className="mb-3 flex flex-col gap-0.5">
            <span className="text-xs text-neutral-500">Signature field</span>
            <select
              aria-label="Signature field"
              value={field}
              onChange={(e) => setField(e.target.value)}
              className="w-full rounded border border-neutral-300 px-2 py-1 text-sm dark:border-neutral-700 dark:bg-neutral-900"
            >
              {fields.map((name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ))}
              <option value="">Add a new invisible signature</option>
            </select>
          </label>
        ) : null}

        <label className="mb-1 flex flex-col gap-0.5">
          <span className="text-xs text-neutral-500">After signing</span>
          <select
            aria-label="After signing"
            value={certify}
            onChange={(e) => setCertify(e.target.value as DocMdpLevel | "")}
            className="w-full rounded border border-neutral-300 px-2 py-1 text-sm dark:border-neutral-700 dark:bg-neutral-900"
          >
            {CERTIFY_CHOICES.map(({ value, label }) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <p className="mb-3 text-xs text-neutral-500">
          {certify === ""
            ? "Records that you signed. Says nothing about later changes."
            : "Readers that honour this will report the signature as invalid if a " +
              "disallowed change is made. It detects changes rather than preventing them."}
        </p>

        {certificate === null ? (
          <p className="mb-2 text-xs text-amber-700 dark:text-amber-500">
            Choose the certificate to sign with.
          </p>
        ) : null}

        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={sign}
            disabled={!canSign}
            className="rounded bg-blue-600 px-2 py-1 text-xs text-white disabled:opacity-40"
          >
            {busy ? "Signing…" : "Sign…"}
          </button>
          <button
            type="button"
            onClick={() => {
              reset();
              onClose();
            }}
            className="ml-auto rounded border border-neutral-300 px-2 py-1 text-xs dark:border-neutral-700"
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
