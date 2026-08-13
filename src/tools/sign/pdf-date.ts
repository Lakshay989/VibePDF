// SPEC: P6-SEC-005 (P6.B1a) — the `/M` entry of a signature dictionary.
//
// PDF 32000-1 §7.9.4: `D:YYYYMMDDHHmmSSOHH'mm'`, where `O` is `+`, `-` or `Z`.
// The apostrophes are part of the syntax, not quoting, and the offset is the
// local zone's — a signature that claims the wrong time is a small lie in a
// document whose whole point is that it does not lie.

/** Two digits, zero-padded. */
function pad(n: number): string {
  return n.toString().padStart(2, "0");
}

/**
 * Format `when` as a PDF date string in the local time zone.
 *
 * `getTimezoneOffset` returns *minutes to add to local time to reach UTC*, so
 * it is the negative of the offset the string wants: UTC+1 comes back as -60
 * and must be written `+01'00'`. Getting that sign backwards produces a
 * plausible timestamp two hours off, which nothing downstream would flag.
 */
export function pdfDate(when: Date): string {
  const stamp =
    `${when.getFullYear()}` +
    pad(when.getMonth() + 1) +
    pad(when.getDate()) +
    pad(when.getHours()) +
    pad(when.getMinutes()) +
    pad(when.getSeconds());

  const offsetMinutes = -when.getTimezoneOffset();
  if (offsetMinutes === 0) return `D:${stamp}Z00'00'`;

  const sign = offsetMinutes > 0 ? "+" : "-";
  const abs = Math.abs(offsetMinutes);
  return `D:${stamp}${sign}${pad(Math.floor(abs / 60))}'${pad(abs % 60)}'`;
}
