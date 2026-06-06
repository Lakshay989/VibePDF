# Manual testing — your verification checklist

Things only a human can confirm: cross-reader PDF validity, real-GUI
behavior (the Tauri webview, not jsdom), and CI runs. Automated tests
(`npm run check` / `test` / `test:rust`) are all green; this is the rest.

Tick a box when done. When a step's checks pass, flip its status in
`steps/P2.md` (or `steps/P1.md`) from `[~]` to `[x]`.

---

## A. Cross-reader PDF checks

Open each in **Adobe Acrobat + macOS Preview + a third reader** (Chrome
works as the third). A passing unit test does *not* prove cross-reader
validity.

- [ ] **`~/Desktop/vibepdf-verify-rotated.pdf`** (B1 rotate) — page 1 should
  render **rotated 90°** and the file must not be flagged corrupt.
  → on pass, flip **P2.B1** to `[x]`.
- [ ] **`~/Desktop/vibepdf-verify-deleted.pdf`** (B2 delete) — **2 pages**
  ("Page 1 (link to page 3)" then "Page 3"); page 2 gone; not corrupt.
  → on pass, flip **P2.B2** to `[x]`.
- [ ] **`~/Desktop/vibepdf-verify-inserted.pdf`** (B3 insert) — **4 pages**:
  "Page 1", then a **blank** page, then "Page 2", "Page 3"; not corrupt.
  → on pass, flip **P2.B3** to `[x]`.
- [ ] **`~/Desktop/vibepdf-verify-cropped.pdf`** (B4 crop) — page 1 shows
  only its **centre** (100pt trimmed each edge); pages 2–3 full; not corrupt.
  → on pass, flip **P2.B4** to `[x]`.
- [x] `~/Desktop/vibepdf-verify.pdf` (A1 save) — already verified.

## B. In-app checks (`npm run dev`)

Open a **multi-page** PDF for these (a one-pager hides the interesting bits).

- [ ] **Rotate (B1):** right-click a page thumbnail → Rotate right / left /
  180. The thumbnail updates immediately.
- [ ] **Live preview (pipeline):** scroll to ~page 3, then rotate that page.
  The **main view** should rotate *in place at page 3* — no blank flash, no
  scroll jump. (If it jumps to page 1, tell me — page-restore timing.)
- [ ] **Delete (B2):** right-click a page thumbnail → **Delete page** (or
  focus a thumbnail and press **Delete/Backspace**). The page vanishes, the
  count drops, and the main view updates live. **⌘Z** brings it back in the
  same position. ⌘S → reopen externally → page really gone.
  → on pass, flip **P2.B2** to `[x]`.
- [ ] **Insert blank (B3):** right-click a page thumbnail → **Insert blank
  page after**. A blank page (same size) appears right after it; count goes
  up; main view + thumbnails update. **⌘Z** removes it; ⌘S → reopen → blank
  page present. → on pass, flip **P2.B3** to `[x]`.
- [ ] **Crop (B4):** right-click a page → **Crop page…** → enter margins →
  Apply. The page shows only the cropped region (main view + thumbnail).
  **Reset crop** restores the full page; **⌘Z** undoes; ⌘S → reopen → cropped.
  → on pass, flip **P2.B4** to `[x]`.
- [ ] **Undo/redo (A3):** after a rotate or delete, **⌘Z** reverts both views
  and **⌘⇧Z** re-applies. The Undo/Redo toolbar buttons enable/disable right.
  → on pass, flip **P2.A3** to `[x]`.
- [ ] **Persist on save (B1):** rotate → **⌘S** → reopen the file in Preview
  externally → still rotated. Reopen in VibePDF → rotation persisted.
- [ ] **Save no-op (A1):** ⌘S on an *unedited* doc → toast **"No changes to
  save"**; the file is left untouched.
- [ ] **Dark mode:** toggle theme in the toolbar → main view **and**
  thumbnails both invert and stay readable.

## C. Crash recovery (A2) — now demoable thanks to B1

This one is finicky; follow the order exactly:

- [ ] Open a PDF and **rotate a page** (this makes it "dirty"). **Do not save.**
- [ ] **Wait ~30 seconds** — the autosave tick runs every 30 s, so give it one
  tick to write the recovery copy.
- [ ] **Force-kill** the app (Activity Monitor → Force Quit, or `kill -9`).
  A normal Quit *discards* the recovery copy on purpose — you must hard-kill.
- [ ] Relaunch → a **"Recover unsaved changes?"** dialog should list the file.
  **Recover** reopens the unsaved version; **Discard** drops it.
  → on pass, flip **P2.A2** to `[x]`.
  - Known limitation (BACKLOG): the recovered tab opens from the *autosave*
    path, so ⌘S targets that copy, not the original. Recovering then Save-As
    to the original is the workaround for now.

## D. CI (GitHub → Actions tab)

- [ ] **`ci.yml`** (macOS) — green on the latest push to `main`.
- [ ] **`e2e.yml`** (E5, Linux) — the **first real E2E run**. It was written
  blind from macOS and likely needs a fixup pass (webkit / tauri-driver /
  xvfb). If green → flip **P1.E5** to `[x]`. If red → paste me the failing
  step's log and I'll iterate.

## E. Recent viewer fixes (`npm run dev`)

From the round of GUI bugs found in real use. Rotate was confirmed working;
the rest still want a pass.

- [~] **Rotate fast-path** — instant, 90°/180° both update the main view.
  *(Looked good; re-confirm the switch-back case below.)*
- [ ] **Switch-back consistency** — rotate a page → switch tabs → switch
  back. The **main page and thumbnail must both stay rotated** (the last
  fix). And ⌘S → reopen in Preview → rotation persisted.
- [ ] **Pinch / Ctrl+wheel zoom** — smooth, responsive (not crawling).
- [ ] **Close tabs** — the **×** on each tab closes that PDF.
- [ ] **Doc switch** — clicking between tabs always updates the main view
  (no "invalid pdf", no stale page).

---

## Status flips waiting on the above

| Step | Flips to `[x]` when |
|---|---|
| P2.B1 — Rotate | A (rotated PDF) + B (rotate/persist) pass |
| P2.B2 — Delete | A (deleted PDF) + B (delete) pass |
| P2.B3 — Insert blank | A (inserted PDF) + B (insert) pass |
| P2.B4 — Crop | A (cropped PDF) + B (crop) pass |
| P2.A3 — Undo/redo | B (undo/redo) passes |
| P2.A2 — Auto-save | C (crash recovery) passes |
| P1.E5 — E2E harness | D (`e2e.yml`) goes green |
