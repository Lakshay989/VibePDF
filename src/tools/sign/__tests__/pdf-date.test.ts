// SPEC: P6-SEC-005 — the signature timestamp format.

import { afterEach, describe, expect, it, vi } from "vitest";

import { pdfDate } from "@/tools/sign/pdf-date";

/** Run `f` as though the machine's zone reported `offsetMinutes`. */
function inZone<T>(offsetMinutes: number, f: () => T): T {
  const spy = vi.spyOn(Date.prototype, "getTimezoneOffset").mockReturnValue(offsetMinutes);
  try {
    return f();
  } finally {
    spy.mockRestore();
  }
}

afterEach(() => vi.restoreAllMocks());

describe("pdfDate", () => {
  it("formats the date and time in the PDF's own syntax", () => {
    const d = new Date(2026, 7, 13, 10, 45, 0);
    expect(inZone(0, () => pdfDate(d))).toBe("D:20260813104500Z00'00'");
  });

  it("zero-pads every field", () => {
    const d = new Date(2026, 0, 2, 3, 4, 5);
    expect(inZone(0, () => pdfDate(d))).toBe("D:20260102030405Z00'00'");
  });

  // getTimezoneOffset is minutes to *add to reach UTC*, so it is the negative of
  // what the string wants. Reversing it gives a plausible timestamp that is
  // silently wrong by twice the offset, which nothing downstream would flag.
  it("writes an eastern offset as positive", () => {
    const d = new Date(2026, 7, 13, 10, 45, 0);
    // UTC+1 → getTimezoneOffset() === -60
    expect(inZone(-60, () => pdfDate(d))).toBe("D:20260813104500+01'00'");
  });

  it("writes a western offset as negative", () => {
    const d = new Date(2026, 7, 13, 10, 45, 0);
    // UTC-5 → getTimezoneOffset() === 300
    expect(inZone(300, () => pdfDate(d))).toBe("D:20260813104500-05'00'");
  });

  it("handles a zone that is not a whole number of hours", () => {
    const d = new Date(2026, 7, 13, 10, 45, 0);
    // Kathmandu, UTC+05:45
    expect(inZone(-345, () => pdfDate(d))).toBe("D:20260813104500+05'45'");
  });
});
