# Phase 5 — in-app sweep

> All 10 P5 steps are `[~]`: shipped, gates green, **never run in the app**.
> The automated suites can't catch a dead overlay, a missed epoch bump, or a
> panel that never mounts — those pass a green test run. This is that pass.
>
> Roadmap rule: Phase 6 doesn't start until Phase 5's acceptance criteria close.
> This checklist is most of that gate.

## Before you start

```bash
python3 scripts/generate-sweep-form.py
```

Writes two files into the git-ignored `Sample PDFs/sweep/`:

| File | Holds |
|---|---|
| `p5-sweep-form.pdf` | One page, one field of every fillable kind, all blank. |
| `p5-sweep-xfa.pdf` | An XFA-only form, for A5's degraded path. |

Then `npm run dev` and open `p5-sweep-form.pdf` **from `Sample PDFs/sweep/`**.

Rerun the generator any time to get back to a blank form — the sweep fills,
edits, and eventually flattens the file in place.

⚠️ Never open a committed fixture from `tests/fixtures/` for this. VibePDF saves
in place, so you'd clobber it.

## Fixed since the first pass — re-check these

The first sweep produced fifteen findings. Ten were real and are now fixed;
re-run these rows specifically:

| Was | Now |
|---|---|
| Filled values showed a ghost duplicate (A2) | Overlay is opaque — PDF.js paints `/V` too, which the old 92%-alpha let through |
| No tooltip on a field (A2) | `/TU` reaches the fill overlay |
| Couldn't multi-select (A4) | Works — needs ⌘-click; the list now says so on hover |
| Radio marks were flat bars (B2) | Circles, in square buttons, whatever the drag shape |
| Dropdown's blank first option errored (B2) | Placeholder is disabled and never commits |
| Combo accepted an invalid default (B2) | It's a picker over the options; backend rejects loudly too |
| Short list box clipped its options (B2) | Grows downward to fit them |
| Signature drew nothing (B2) | Dashed placeholder box |
| Push-button had no caption (B2) | Caption drawn into the appearance |
| Field count stale after ⌘Z (B3) | Re-reads on every edit |

Two were **not** bugs: a push-button does nothing because it has no `/A` action
(actions aren't in the P5 spec), and a signature exports while a push-button
doesn't (P5-FORM-008 — a button holds no value). Three were this checklist or
the fixture being wrong, and are corrected above.

## How to mark

Each row: do the action, check the expectation. If it passes, tick the box here
**and** flip that step to `[x]` in [P5.md](P5.md). If it fails, leave both and
tell me what you saw — that's a bug report, not a failed sweep.

The order matters: the file accumulates state as you go, and flatten destroys it.

---

## Track A — detection & filling

### [ ] A1 — Detect AcroForm + Form-mode entry point
Open `p5-sweep-form.pdf`.
- A **"Form mode"** affordance appears, showing **7 fields**.
- It does *not* appear for a form-less PDF (try `Sample PDFs/normal/invoice.pdf`).

### [ ] A2 — Fill text fields
Enter form mode.
- Click **"1. Full name"**, type `Ada Lovelace` → text appears.
- **Tab** → focus moves to *Code*, then *Notes*, in that order.
- **Code** field: type `ABCDEFGH` → only `ABCDE` is kept (`/MaxLen 5`).
- **Notes**: type two lines with Enter → both survive; it doesn't submit or lose the second.
- ⌘Z undoes the last fill; ⌘⇧Z redoes it.

*(A tooltip from `/TU` was in an earlier draft of this list — the fill overlay
never implemented one. `/TU` is only surfaced in the B3 properties panel. Logged
as a gap, not a sweep failure.)*

### [ ] A3 — Fill checkbox / radio
- Click **Agree** → shows checked. Click again → unchecked.
- Click **Red** → selected. Click **Green** → Green selected **and Red clears** (one group).
- Undo after a radio pick returns to the previous option, not to blank.

### [ ] A4 — Fill choice fields
- **Fruit** dropdown lists exactly `Apple / Banana / Cherry`. Pick `Banana`.
- **Tags** list lists `urgent / review / archive`. Select `urgent`, then **⌘-click**
  `archive` — it's a native multi-select, so a plain second click *replaces* the
  selection rather than adding to it. (No affordance says so; that's a real UX
  gap, but it isn't a broken feature.)
- Undo removes the selection.

### [ ] A5 — XFA degraded support
Open `p5-sweep-xfa.pdf` (separate file).
- A notice says XFA editing isn't supported — **no fill UI is offered**.
- The "Convert XFA to flat content (read-only)" action is present and works.
- After converting: the page still renders, and the form notice is gone.

Go back to `p5-sweep-form.pdf` for the rest.

---

## Track B — authoring

> **There is no "form-edit mode".** An earlier draft of this list said there was.
> Two *independent* controls, in two different bars:
>
> | Control | Where | Does |
> |---|---|---|
> | **`Form mode (7 fields)`** | top bar, right of *Open PDF* | **fills** fields (Track A) |
> | **`Form Field`** | markup toolbar, right of *Add Link* | **creates** fields (Track B) |
>
> `Form Field` is a *tool*, not a mode — click it, it highlights blue, you drag
> one box, it deactivates. Click it again for the next field. It works whether
> or not Form mode is on, and turning Form mode off is the easier way to see
> what you're drawing.

### [ ] B1 — Create text field
- Click **Form Field** in the markup toolbar (it turns blue).
- Drag a box anywhere on the page — the empty band below the
  "drag new fields here" line is clear space kept for exactly this.
- On release a popover appears: pick **Text**, then set the config.
- Configure name / default value / max length / multi-line / required — all five stick.
- The new field is immediately fillable after leaving edit mode.
- Undo removes it cleanly (no orphan widget left drawn).

### [ ] B2 — Create the other six kinds
Same tool, one drag each (re-click **Form Field** every time), picking a
different type in the popover: **checkbox**, **radio group** (≥2 options),
**combo**, **list**, **signature**, **push-button**.
- Each renders with a sane default appearance.
- The radio group behaves as *one* group (picking one clears the other).
- The push-button shows its caption.
- The signature field is present but not fillable (correct — signing is Phase 6).

### [ ] B3 — Field properties + tab order
- The field panel lists every field on the page, in tab order.
- Select one → its properties load. Rename it → the list updates, the name sticks.
- ↑ / ↓ reorder → **Tab in fill mode follows the new order**. This is the one that
  a passing unit test can't prove.
- Delete a field → gone from the page *and* the panel.

---

## Track C — data interchange

### [ ] C1 — Export form data
With the form filled (redo your A2–A4 values if you undid them):
- Export **JSON** → open it: every field, with `name` / `type` / `value`.
- Export **CSV**, **FDF**, **XFDF** → all four write without error.
- The push-button from B2 is **absent** from the data; the signature has an empty value.

### [ ] C2 — Import form data
- Regenerate a blank form: `python3 scripts/generate-sweep-form.py`, open it.
- Import the JSON you exported → every value restored, **form still interactive**.
- Hand-edit the JSON: change one `name` to garbage, and one `type` to a wrong kind.
  Import again → the panel **names both**: unmatched, and the type mismatch.
  Neither is applied; the rest still fill.

### [ ] C2 — Flatten
Last, on the filled form (it's destructive):
- **Flatten form** asks for confirmation first.
- After confirming: values are **visible as page content**, nothing is clickable,
  no field highlight, form mode is gone.
- ⌘Z restores the interactive form (in-session undo only).
- Save the flattened file, reopen it → still flat, still shows the values.

---

## Cross-reader check

The one thing only you can do. Open in **Acrobat**, **macOS Preview**, and a
third reader (Okular / Sumatra / Firefox):

- [ ] `Sample PDFs/sweep/p5-sweep-form.pdf` after filling + saving — values show, still interactive
- [ ] `Sample PDFs/verify/p5-forms/vibepdf-verify-form-import.pdf` — filled, interactive
- [ ] `Sample PDFs/verify/p5-forms/vibepdf-verify-form-flatten.pdf` — values baked, nothing clickable

Known cosmetic limit on the flattened file: synthesized text is **top-anchored**
in its box, not vertically centred the way Acrobat draws a live field, and `/Q`
alignment isn't honoured yet. Legible and inside the box. Flag it if it looks
worse than that.

---

## Still open after this sweep

- The roadmap's other acceptance demo needs a **real IRS W-9** — drop one in
  `Sample PDFs/` and it can be wired as `tests/fixtures/acceptance/p5-irs-w9.pdf`.
- `P5-FORM-006b` / `006c` (field properties, tab order) aren't in
  `docs/02_PRODUCT_SPEC.md` yet — B3 ships against a drafted spec line.
