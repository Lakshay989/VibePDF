//! Per-document undo/redo — the "session history" mechanism.
//!
//! SPEC: P2-PAGE-003 (line 68, "Deletion SHALL be undoable") and the
//! session-history clause of line 126. This module is the generic
//! machinery only; concrete page edits (rotate, delete, …) implement
//! [`Edit`] in the P2.B* steps. See `docs/04_ARCHITECTURE.md`
//! § "Undo/redo (session history)".
//!
//! ## Why generic over `T`
//!
//! The stack mechanics are independent of *what* is being edited, so the
//! type is generic over a target `T`. That lets the invariants (undo↔redo
//! symmetry, redo-cleared-on-new-edit, depth cap) be unit-tested against
//! a trivial target like `i32`, with no live `PdfDocument`. The document
//! actor instantiates `UndoStack<PdfDocument>`; the P2.B* steps add
//! `Edit<PdfDocument>` implementations.

use std::collections::VecDeque;

use serde::Serialize;

use crate::error::CommandError;

/// Maximum number of undoable actions kept per document. A page edit can
/// retain page content for its inverse (e.g. delete must remember the
/// removed page to restore it), so history is capped to bound memory;
/// older actions fall off the bottom.
pub const MAX_UNDO_DEPTH: usize = 100;

/// A reversible edit against a target `T`.
///
/// [`Edit::apply`] performs the edit and returns the edit that *reverses*
/// it — so undo and redo are the same operation run against opposite
/// stacks. The inverse of an inverse is the original, which is how redo
/// reconstructs a forward edit.
pub trait Edit<T> {
    /// Apply this edit to `target`, consuming it and returning its
    /// inverse. A failure leaves `target` unspecified — callers treat an
    /// `Err` as "the edit did not happen" and surface a typed error.
    fn apply(self: Box<Self>, target: &mut T) -> Result<Box<dyn Edit<T>>, CommandError>;

    /// Short label for tracing and the UI ("rotate", "delete", …).
    fn label(&self) -> &'static str;
}

/// Snapshot of history availability, surfaced to the frontend so it can
/// enable/disable the Undo/Redo affordances. Wire type for the
/// `pdf_undo` / `pdf_redo` / `pdf_history_state` commands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryState {
    pub can_undo: bool,
    pub can_redo: bool,
}

/// Per-document undo and redo stacks over a target `T`.
///
/// `undo` is a `VecDeque` so the depth cap can drop the *oldest* action
/// from the front cheaply; `redo` is a `Vec` (LIFO, never capped beyond
/// what `undo` already bounds).
///
/// Each stack entry is paired with the **state id it returns to** — the id
/// of the document state that existed *before* the recorded edit. Combined
/// with the live `current_id`, this lets the actor derive its dirty flag
/// (see [`UndoStack::current_state_id`]) without the false-clean bugs a
/// bare depth counter has (`FABLE_REVIEW` §3.11 / P4.HF12).
pub struct UndoStack<T> {
    undo: VecDeque<(u64, Box<dyn Edit<T>>)>,
    redo: Vec<(u64, Box<dyn Edit<T>>)>,
    /// Unique id of the current document state. `0` is the pristine
    /// (as-opened) state; every recorded edit mints a fresh id from
    /// `next_id`. Ids are never reused, so a state reached by editing after
    /// an undo is distinguishable from the branch it replaced.
    current_id: u64,
    /// Next id to hand out. Starts at `1` (`0` is reserved for pristine)
    /// and only ever increases, even across the depth cap and undo/redo —
    /// that monotonicity is what makes forked history detectable.
    next_id: u64,
}

impl<T> UndoStack<T> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            current_id: 0,
            next_id: 1,
        }
    }

    /// Current availability for the UI.
    #[must_use]
    pub fn state(&self) -> HistoryState {
        HistoryState {
            can_undo: !self.undo.is_empty(),
            can_redo: !self.redo.is_empty(),
        }
    }

    /// A unique id for the current document state; `0` iff the document is
    /// in its pristine (as-opened) form. The actor remembers this id at
    /// each successful save and treats the document as dirty whenever the
    /// live id differs. Because ids are minted monotonically and never
    /// reused, this stays correct across undo-to-saved, redo, forked
    /// history, and depth-cap eviction (an evicted-past edit leaves
    /// `current_id` at the un-undoable floor, never falsely back at `0`).
    ///
    /// SPEC: P2-SAVE-001 / P2.A2 — see `FABLE_REVIEW` §3.11 (P4.HF12).
    #[must_use]
    pub fn current_state_id(&self) -> u64 {
        self.current_id
    }

    /// Record that a forward edit was just applied: its `inverse` (the
    /// value `Edit::apply` returned) goes onto the undo stack paired with
    /// the id it returns to (the pre-edit state), the current state gets a
    /// fresh id, and the redo stack is cleared because a new action forks
    /// history.
    ///
    /// Used by the P2.B* mutating messages; unused by P2.A3 itself (no
    /// page operations exist yet), but exercised by this module's tests.
    pub fn record(&mut self, inverse: Box<dyn Edit<T>>) {
        let return_to = self.current_id;
        self.current_id = self.next_id;
        self.next_id += 1;
        self.redo.clear();
        self.undo.push_back((return_to, inverse));
        while self.undo.len() > MAX_UNDO_DEPTH {
            self.undo.pop_front();
        }
    }

    /// Undo the most recent action against `target`. A no-op (returns the
    /// unchanged state) when the undo stack is empty.
    pub fn undo(&mut self, target: &mut T) -> Result<HistoryState, CommandError> {
        if let Some((return_to, edit)) = self.undo.pop_back() {
            let inverse = edit.apply(target)?;
            // The state we are leaving is `current_id`; redo restores it.
            self.redo.push((self.current_id, inverse));
            self.current_id = return_to;
        }
        Ok(self.state())
    }

    /// Redo the most recently undone action against `target`. A no-op when
    /// the redo stack is empty. The re-applied action moves back onto the
    /// undo stack (which cannot exceed the cap, since it came from there).
    pub fn redo(&mut self, target: &mut T) -> Result<HistoryState, CommandError> {
        if let Some((restore, edit)) = self.redo.pop() {
            let inverse = edit.apply(target)?;
            // The state we are leaving is `current_id`; undo returns to it.
            self.undo.push_back((self.current_id, inverse));
            self.current_id = restore;
        }
        Ok(self.state())
    }
}

impl<T> Default for UndoStack<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// Synthetic reversible edit: add `delta` to a counter; the inverse
    /// subtracts it. Proves the stack mechanics with no `PdfDocument`.
    struct Add(i32);

    impl Edit<i32> for Add {
        fn apply(self: Box<Self>, target: &mut i32) -> Result<Box<dyn Edit<i32>>, CommandError> {
            *target += self.0;
            Ok(Box::new(Add(-self.0)))
        }
        fn label(&self) -> &'static str {
            "add"
        }
    }

    /// Perform a forward edit and record it on the stack (what a B-step
    /// mutating message will do).
    fn apply_and_record(stack: &mut UndoStack<i32>, target: &mut i32, delta: i32) {
        let inverse = Box::new(Add(delta)).apply(target).expect("synthetic edit");
        stack.record(inverse);
    }

    #[test]
    fn undo_then_redo_round_trips() {
        let mut stack = UndoStack::<i32>::new();
        let mut v = 0;
        apply_and_record(&mut stack, &mut v, 5); // 5
        apply_and_record(&mut stack, &mut v, 3); // 8
        assert_eq!(stack.state(), HistoryState { can_undo: true, can_redo: false });

        let after_first_undo = stack.undo(&mut v).unwrap();
        assert!(after_first_undo.can_redo);
        assert_eq!(v, 5);
        stack.undo(&mut v).unwrap();
        assert_eq!(v, 0);
        assert_eq!(stack.state(), HistoryState { can_undo: false, can_redo: true });

        stack.redo(&mut v).unwrap();
        stack.redo(&mut v).unwrap();
        assert_eq!(v, 8);
        assert_eq!(stack.state(), HistoryState { can_undo: true, can_redo: false });
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut stack = UndoStack::<i32>::new();
        let mut v = 0;
        apply_and_record(&mut stack, &mut v, 5);
        stack.undo(&mut v).unwrap(); // v=0, one redo available
        assert!(stack.state().can_redo);

        apply_and_record(&mut stack, &mut v, 2); // forks history
        assert!(!stack.state().can_redo, "a new edit must clear the redo stack");
        assert!(stack.state().can_undo);
        assert_eq!(v, 2);
    }

    #[test]
    fn empty_stack_undo_and_redo_are_noops() {
        let mut stack = UndoStack::<i32>::new();
        let mut v = 7;
        assert_eq!(stack.undo(&mut v).unwrap(), HistoryState::default());
        assert_eq!(stack.redo(&mut v).unwrap(), HistoryState::default());
        assert_eq!(v, 7, "no-op undo/redo must not touch the target");
    }

    #[test]
    fn depth_cap_drops_oldest() {
        let mut stack = UndoStack::<i32>::new();
        let mut v = 0;
        for _ in 0..(MAX_UNDO_DEPTH + 10) {
            apply_and_record(&mut stack, &mut v, 1);
        }
        let mut undos = 0;
        while stack.state().can_undo {
            stack.undo(&mut v).unwrap();
            undos += 1;
            assert!(undos <= MAX_UNDO_DEPTH, "undo stack exceeded the depth cap");
        }
        assert_eq!(undos, MAX_UNDO_DEPTH);
    }

    // --- state-id tracking (FABLE_REVIEW §3.11 / P4.HF12) ---

    #[test]
    fn pristine_state_id_is_zero() {
        let stack = UndoStack::<i32>::new();
        assert_eq!(stack.current_state_id(), 0, "as-opened document is pristine");
    }

    #[test]
    fn each_edit_mints_a_fresh_monotonic_id() {
        let mut stack = UndoStack::<i32>::new();
        let mut v = 0;
        apply_and_record(&mut stack, &mut v, 1);
        let a = stack.current_state_id();
        apply_and_record(&mut stack, &mut v, 1);
        let b = stack.current_state_id();
        assert!(a > 0 && b > a, "ids advance monotonically: 0 < {a} < {b}");
    }

    #[test]
    fn undo_returns_to_prior_state_id_and_redo_restores_it() {
        let mut stack = UndoStack::<i32>::new();
        let mut v = 0;
        apply_and_record(&mut stack, &mut v, 5); // id A
        let a = stack.current_state_id();
        apply_and_record(&mut stack, &mut v, 3); // id B
        let b = stack.current_state_id();

        stack.undo(&mut v).unwrap();
        assert_eq!(stack.current_state_id(), a, "undo returns to the prior state id");
        stack.redo(&mut v).unwrap();
        assert_eq!(stack.current_state_id(), b, "redo restores the exact same id");

        // Undo all the way back reaches pristine again.
        stack.undo(&mut v).unwrap();
        stack.undo(&mut v).unwrap();
        assert_eq!(stack.current_state_id(), 0, "undo-to-open is pristine (id 0)");
    }

    #[test]
    fn new_edit_after_undo_gets_a_fresh_id_not_the_saved_one() {
        // The branch case a bare depth counter gets wrong: "save" at a
        // state, undo, then a *new* edit lands at the same stack depth but
        // must NOT compare equal to the saved id (its content differs).
        let mut stack = UndoStack::<i32>::new();
        let mut v = 0;
        apply_and_record(&mut stack, &mut v, 5);
        let saved = stack.current_state_id();

        stack.undo(&mut v).unwrap();
        apply_and_record(&mut stack, &mut v, 9); // forks history
        assert_ne!(
            stack.current_state_id(),
            saved,
            "a forked edit at the same depth must have a distinct id (no false-clean)"
        );
    }

    #[test]
    fn state_id_after_cap_eviction_is_not_falsely_pristine() {
        // Edit past the cap so the oldest inverses (and the path back to
        // pristine) are evicted, then undo everything still on the stack.
        // The un-undoable floor is a real, edited state — its id must not
        // collapse to 0, or the actor would report a modified doc as saved.
        let mut stack = UndoStack::<i32>::new();
        let mut v = 0;
        for _ in 0..(MAX_UNDO_DEPTH + 5) {
            apply_and_record(&mut stack, &mut v, 1);
        }
        while stack.state().can_undo {
            stack.undo(&mut v).unwrap();
        }
        assert_ne!(
            stack.current_state_id(),
            0,
            "cap-evicted edits leave the floor non-pristine"
        );
    }
}
