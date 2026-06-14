// SPEC: infrastructure (P3.A1) — a registry of annotation tools by id.
//
// Concrete tools (P3.B/C) register themselves here; the active-tool controller
// (P3.A2) looks them up by the id held in `useToolStore`. A plain module-level
// map — tools are singletons (stateless reducers), so there's no per-document
// instance to manage.

import type { AnnotationTool } from "./tool-contract";
import type { ToolId } from "./types";

const tools = new Map<ToolId, AnnotationTool>();

/** Register (or replace) a tool. */
export function registerTool(tool: AnnotationTool): void {
  tools.set(tool.id, tool);
}

/** The tool registered for `id`, or `undefined`. */
export function getTool(id: ToolId): AnnotationTool | undefined {
  return tools.get(id);
}

/** Every registered tool. */
export function allTools(): AnnotationTool[] {
  return [...tools.values()];
}

/** Clear the registry (test hook). */
export function clearTools(): void {
  tools.clear();
}
