// SPEC: P2-PAGE-001 — the rotate control: a small context menu raised by
// right-clicking a page thumbnail. Presentational only; the parent
// (ThumbnailPanel) owns the position + the rotate action.

import { useEffect } from "react";

export interface RotateMenuProps {
  x: number;
  y: number;
  /** `degrees` is a multiple of 90 (positive = clockwise). */
  onRotate: (degrees: number) => void;
  onClose: () => void;
}

export function RotateMenu({ x, y, onRotate, onClose }: RotateMenuProps) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const choose = (degrees: number) => {
    onRotate(degrees);
    onClose();
  };

  return (
    <>
      {/* Full-screen backdrop: the next click anywhere dismisses the menu. */}
      <div
        className="fixed inset-0 z-40"
        onClick={onClose}
        onContextMenu={(e) => {
          e.preventDefault();
          onClose();
        }}
      />
      <div
        role="menu"
        aria-label="Rotate page"
        className="fixed z-50 min-w-[168px] rounded-md border border-neutral-200 bg-white py-1 text-sm shadow-lg dark:border-neutral-700 dark:bg-neutral-800 dark:text-neutral-100"
        style={{ left: x, top: y }}
      >
        <MenuItem label="Rotate right 90°" onClick={() => choose(90)} />
        <MenuItem label="Rotate left 90°" onClick={() => choose(-90)} />
        <MenuItem label="Rotate 180°" onClick={() => choose(180)} />
      </div>
    </>
  );
}

function MenuItem({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button
      role="menuitem"
      type="button"
      onClick={onClick}
      className="block w-full px-3 py-1.5 text-left hover:bg-neutral-100 dark:hover:bg-neutral-700"
    >
      {label}
    </button>
  );
}
