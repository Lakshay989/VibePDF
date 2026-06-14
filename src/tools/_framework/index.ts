// SPEC: infrastructure (P3.A1) — public surface of the annotation tool
// framework. Concrete tools (P3.B/C) and the render layer (P3.A2) import from
// here.

export type {
  Annotation,
  AnnotationBase,
  AnnotationDraft,
  PagePoint,
  PdfRect,
  RectAnnotation,
  ToolId,
  ToolOptions,
} from "./types";
export type { AnnotationTool, ToolContext } from "./tool-contract";
export {
  IDLE,
  stepTool,
  type ToolEvent,
  type ToolPhase,
  type ToolSession,
  type ToolStep,
} from "./lifecycle";
export {
  normalizeRect,
  pdfRectFromPoints,
  pdfToScreen,
  screenToPdf,
  type PageGeometry,
  type ScreenPoint,
} from "./coords";
export { allTools, clearTools, getTool, registerTool } from "./registry";
