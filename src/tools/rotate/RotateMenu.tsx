// SPEC: P2-PAGE-001 — the rotate control: a small context menu raised by
// right-clicking a page thumbnail. Presentational only; the parent
// (ThumbnailPanel) owns the position + the rotate action.

import { useEffect } from "react";

export interface RotateMenuProps {
  x: number;
  y: number;
  /** `degrees` is a multiple of 90 (positive = clockwise). */
  onRotate: (degrees: number) => void;
  /** SPEC: P2-PAGE-004 — insert a blank page after this one. */
  onInsert: () => void;
  /** SPEC: P2-PAGE-003 — delete this page. */
  onDelete: () => void;
  onClose: () => void;
}

export function RotateMenu({
  x,
  y,
  onRotate,
  onInsert,
  onDelete,
  onClose,
}: RotateMenuProps) {
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
        <div className="my-1 border-t border-neutral-200 dark:border-neutral-700" />
        <MenuItem
          label="Insert blank page after"
          onClick={() => {
            onInsert();
            onClose();
          }}
        />
        <MenuItem
          label="Delete page"
          danger
          onClick={() => {
            onDelete();
            onClose();
          }}
        />
      </div>
    </>
  );
}

function MenuItem({
  label,
  onClick,
  danger = false,
}: {
  label: string;
  onClick: () => void;
  danger?: boolean;
}) {
  return (
    <button
      role="menuitem"
      type="button"
      onClick={onClick}
      className={
        "block w-full px-3 py-1.5 text-left hover:bg-neutral-100 dark:hover:bg-neutral-700 " +
        (danger ? "text-red-600 dark:text-red-400" : "")
      }
    >
      {label}
    </button>
  );
}
