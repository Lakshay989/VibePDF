// SPEC: infrastructure (P3.A1) — the tool lifecycle state machine.
//
// A pure reducer that threads a tool's in-progress draft through a gesture:
// `activate → pointerdown → pointermove → pointerup → commit/cancel`. It owns
// the phase + draft so individual tools stay stateless reducers (see
// `tool-contract.ts`). The DOM binding (P3.A2) feeds it real pointer events and
// commits the result into the annotation store. DOM-free, so it unit-tests
// without a webview — the part most prone to off-by-one gesture bugs.

import type { AnnotationTool, ToolContext } from "./tool-contract";
import type { AnnotationDraft, PagePoint } from "./types";

export type ToolPhase = "idle" | "drawing";

/** The lifecycle's state: which phase, and the live draft (if drawing). */
export interface ToolSession {
  phase: ToolPhase;
  draft: AnnotationDraft | null;
}

export type ToolEvent =
  | { kind: "pointerDown"; pt: PagePoint }
  | { kind: "pointerMove"; pt: PagePoint }
  | { kind: "pointerUp"; pt: PagePoint }
  | { kind: "cancel" };

/** The result of one step: the next session and any draft ready to commit. */
export interface ToolStep {
  session: ToolSession;
  /** A finished draft to commit (the caller assigns id + createdAt), or null. */
  committed: AnnotationDraft | null;
}

/** The starting / reset session. */
export const IDLE: ToolSession = { phase: "idle", draft: null };

/**
 * Advance the lifecycle by one event. Pure: same inputs → same outputs, no DOM.
 *
 * - `pointerDown` while idle starts a draft (ignored if already drawing).
 * - `pointerMove` evolves the draft only while drawing.
 * - `pointerUp` finishes; the tool decides whether a draft commits.
 * - `cancel` (e.g. Escape, tool switch) drops any draft and returns to idle.
 */
export function stepTool(
  tool: AnnotationTool,
  session: ToolSession,
  event: ToolEvent,
  ctx: ToolContext,
): ToolStep {
  if (event.kind === "cancel") {
    return { session: IDLE, committed: null };
  }

  if (event.kind === "pointerDown") {
    if (session.phase !== "idle") return { session, committed: null };
    const draft = tool.onPointerDown(event.pt, ctx);
    if (!draft) return { session: IDLE, committed: null };
    return { session: { phase: "drawing", draft }, committed: null };
  }

  if (event.kind === "pointerMove") {
    if (session.phase !== "drawing" || !session.draft) {
      return { session, committed: null };
    }
    const draft = tool.onPointerMove(session.draft, event.pt, ctx);
    return { session: { phase: "drawing", draft }, committed: null };
  }

  // pointerUp
  if (session.phase !== "drawing" || !session.draft) {
    return { session: IDLE, committed: null };
  }
  const committed = tool.onPointerUp(session.draft, event.pt, ctx);
  return { session: IDLE, committed };
}
