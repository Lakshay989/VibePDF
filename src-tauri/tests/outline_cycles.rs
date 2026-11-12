//! SPEC: P1-VIEW-013 — a bookmark tree that loops back on itself, or nests
//! far deeper than a real one, must not crash or freeze the app (P7.HF2).
//!
//! A bookmark tree is held together by the file's own pointers: `/First` to
//! an item's first child, `/Next` to its next sibling. A damaged or hostile
//! file can aim them anywhere. Before this step:
//!
//! - Save pruned dangling bookmarks with a function that called itself for
//!   each level. An item whose `/First` was itself overflowed the stack, and
//!   a stack overflow closes the app.
//! - Save, Merge and the title reader followed `/Next` with no memory of
//!   where they had been. On a loop they never finished, and the list they
//!   were building grew until memory ran out.
//! - Split by bookmarks did the same through `PDFium`, holding the lock that
//!   every open document needs.
//!
//! Each hostile file here is `bookmarks.pdf` (six pages; "Chapter 1", "2" and
//! "3" on pages 1, 3 and 5) with a pointer or two changed through lopdf. A
//! regression shows up as a test binary that crashes or never finishes, not
//! as a failed assertion.

use std::path::{Path, PathBuf};

use lopdf::{Dictionary, Document, Object, ObjectId};
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::cos::{prune_dangling_destinations, read_top_level_outline_titles};
use vibepdf_lib::pdf::document::open_pdf;
use vibepdf_lib::pdf::merge::merge_documents;
use vibepdf_lib::pdf::split::SplitMode;

// Object numbers from `tests/fixtures/basic/generate-bookmarks.py`.
const OUTLINE: ObjectId = (16, 0);
const CHAPTER_1: ObjectId = (17, 0);
const CHAPTER_2: ObjectId = (18, 0);
const CHAPTER_3: ObjectId = (19, 0);
const PAGE_1: ObjectId = (3, 0);
const PAGE_2: ObjectId = (4, 0);
/// The font: a real object that is not a page, so a destination naming it
/// points nowhere, as one does after its page is deleted.
const NOT_A_PAGE: ObjectId = (9, 0);

const ALL_THREE: [&str; 3] = ["Chapter 1", "Chapter 2", "Chapter 3"];

fn fixture() -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic/bookmarks.pdf");
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn scratch() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-outline-cycles-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

/// `bookmarks.pdf` after `edit`, re-serialized by lopdf (object numbers kept).
fn edited(edit: impl FnOnce(&mut Document)) -> Vec<u8> {
    let mut doc = Document::load_mem(&std::fs::read(fixture()).expect("read fixture")).expect("load fixture");
    edit(&mut doc);
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save");
    out
}

fn point(doc: &mut Document, from: ObjectId, key: &str, to: ObjectId) {
    doc.get_dictionary_mut(from).expect("a dictionary").set(key, Object::Reference(to));
}

fn destination(page: ObjectId) -> Object {
    Object::Array(vec![Object::Reference(page), Object::Name(b"Fit".to_vec())])
}

/// Make `item` a bookmark whose page is gone.
fn dangle(doc: &mut Document, item: ObjectId) {
    doc.get_dictionary_mut(item).expect("a dictionary").set("Dest", destination(NOT_A_PAGE));
}

fn reference(doc: &Document, from: ObjectId, key: &[u8]) -> Option<ObjectId> {
    doc.get_dictionary(from).ok()?.get(key).ok()?.as_reference().ok()
}

fn has_destination(doc: &Document, item: ObjectId) -> bool {
    doc.get_dictionary(item).expect("a dictionary").has(b"Dest")
}

fn titles(bytes: &[u8]) -> Vec<String> {
    read_top_level_outline_titles(bytes).expect("read titles")
}

fn page_count(path: &Path) -> u32 {
    let (doc, meta) = open_pdf(path, None).expect("PDFium opens it");
    drop(doc);
    meta.page_count
}

/// What `PDFium`, a reader we did not write, counts on the top level. Counted
/// through the binding's guarded iterator, so a chain that still looped would
/// give a wrong number rather than hang the test.
fn pdfium_top_level_count(bytes: &[u8]) -> usize {
    let path = scratch().join("count.pdf");
    std::fs::write(&path, bytes).expect("write");
    let (doc, _meta) = open_pdf(&path, None).expect("PDFium opens it");
    let bookmarks = doc.bookmarks();
    bookmarks.root().map_or(0, |first| 1 + first.iter_siblings().count())
}

/// The last chapter's `/Next` points back at the first: a top-level loop.
fn looping() -> Vec<u8> {
    edited(|doc| point(doc, CHAPTER_3, "Next", CHAPTER_1))
}

/// The last chapter's `/Next` is the outline itself, which is not a
/// bookmark; it has a title here so that reading it as one would show.
fn into_the_outline(doc: &mut Document) {
    point(doc, CHAPTER_3, "Next", OUTLINE);
    doc.get_dictionary_mut(OUTLINE).expect("outline").set("Title", Object::string_literal("Not a bookmark"));
}

// The crash the 2026-11-12 review reproduced, through the real Save path this
// time. Deleting page 3 leaves "Chapter 2" dangling, so Save has pruning to
// do and rewrites the outline around the item that is its own child.
#[tokio::test]
async fn a_bookmark_whose_first_child_is_itself_does_not_stop_a_save() {
    let dir = scratch();
    let input = dir.join("self-loop.pdf");
    std::fs::write(&input, edited(|doc| point(doc, CHAPTER_1, "First", CHAPTER_1))).expect("write");
    let out = dir.join("out.pdf");

    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), input, None).expect("opens");
    handle.delete_pages(vec![2]).await.expect("delete page 3");
    handle.save(Some(out.clone())).await.expect("save");
    drop(handle);

    assert_eq!(titles(&std::fs::read(&out).expect("read")), ["Chapter 1", "Chapter 3"]);
    assert_eq!(page_count(&out), 5);
    let _ = std::fs::remove_dir_all(&dir);
}

// Every way `/First` can lead back into the tree. Each file also has a
// dangling "Chapter 2", so the pruner rewrites it rather than handing it back.
#[test]
fn loops_through_first_do_not_stop_the_pruner() {
    let shapes = [
        ("an item that is its own child", vec![(CHAPTER_1, CHAPTER_1)]),
        ("an item whose child is the outline itself", vec![(CHAPTER_1, OUTLINE)]),
        ("two items that are each other's child", vec![(CHAPTER_1, CHAPTER_3), (CHAPTER_3, CHAPTER_1)]),
    ];
    for (name, pointers) in shapes {
        let bytes = edited(|doc| {
            for (from, to) in pointers {
                point(doc, from, "First", to);
            }
            dangle(doc, CHAPTER_2);
        });
        let pruned = prune_dangling_destinations(bytes);
        assert_eq!(titles(&pruned), ["Chapter 1", "Chapter 3"], "{name}");
        assert_eq!(pdfium_top_level_count(&pruned), 2, "{name}");
    }
}

// "Chapter 2" is dropped from the top level, where its page is gone, but in
// this file it also hangs below "Chapter 1", where a reader still shows it.
// It must not keep a destination to a missing page there. (A first version of
// the worklist shared one visited set with the top level and skipped it; the
// mutation run found that.)
#[test]
fn a_dropped_bookmark_that_is_also_a_child_loses_its_destination() {
    let bytes = edited(|doc| {
        point(doc, CHAPTER_1, "First", CHAPTER_2);
        dangle(doc, CHAPTER_2);
    });
    let pruned = prune_dangling_destinations(bytes);

    assert_eq!(titles(&pruned), ["Chapter 1", "Chapter 3"]);
    let doc = Document::load_mem(&pruned).expect("load");
    assert_eq!(reference(&doc, CHAPTER_1, b"First"), Some(CHAPTER_2), "still shown below Chapter 1");
    assert!(!has_destination(&doc, CHAPTER_2), "a reachable bookmark still points at a missing page");
    assert!(has_destination(&doc, CHAPTER_1) && has_destination(&doc, CHAPTER_3), "live destinations were removed");
}

// A top-level loop with something to prune. The survivors must end up chained
// to each other with an end to the chain: the loop is gone from the file, not
// just stepped around while reading it.
#[test]
fn a_top_level_next_loop_is_pruned_and_rechained() {
    let back_to_the_start: fn(&mut Document) = |doc| point(doc, CHAPTER_3, "Next", CHAPTER_1);
    for (name, damage) in [("back to the first item", back_to_the_start), ("into the outline itself", into_the_outline)] {
        let bytes = edited(|doc| {
            damage(doc);
            dangle(doc, CHAPTER_2);
        });
        let pruned = prune_dangling_destinations(bytes);

        assert_eq!(titles(&pruned), ["Chapter 1", "Chapter 3"], "{name}");
        let doc = Document::load_mem(&pruned).expect("load");
        assert_eq!(reference(&doc, OUTLINE, b"First"), Some(CHAPTER_1), "{name}");
        assert_eq!(reference(&doc, OUTLINE, b"Last"), Some(CHAPTER_3), "{name}");
        assert_eq!(doc.get_dictionary(OUTLINE).expect("outline").get(b"Count").and_then(Object::as_i64).expect("count"), 2, "{name}");
        assert_eq!(reference(&doc, CHAPTER_1, b"Next"), Some(CHAPTER_3), "{name}");
        assert_eq!(reference(&doc, CHAPTER_3, b"Prev"), Some(CHAPTER_1), "{name}");
        assert_eq!(reference(&doc, CHAPTER_3, b"Next"), None, "{name}: the chain still has no end");
        assert_eq!(pdfium_top_level_count(&pruned), 2, "{name}");
    }
}

// Nothing dangles, so there is nothing to prune: the walk has to end, each
// title is read once, and the user's bytes come back untouched.
#[test]
fn a_clean_looping_outline_is_returned_byte_for_byte() {
    for (name, bytes) in [("back to the first item", looping()), ("into the outline itself", edited(into_the_outline))] {
        assert_eq!(titles(&bytes), ALL_THREE, "{name}: each title once, and only bookmarks' titles");
        assert!(prune_dangling_destinations(bytes.clone()) == bytes, "{name}: a file with nothing to prune was rewritten");
    }
}

/// `depth` bookmarks under "Chapter 1", each the only child of the one
/// before, all pointing at page 1. Returns the file and the items' ids,
/// outermost first.
fn nested_chain(depth: usize) -> (Document, Vec<ObjectId>) {
    let mut doc = Document::load_mem(&std::fs::read(fixture()).expect("read fixture")).expect("load fixture");
    let mut ids = Vec::with_capacity(depth);
    let mut parent = CHAPTER_1;
    for level in 0..depth {
        let mut item = Dictionary::new();
        item.set("Title", Object::string_literal(format!("Level {level}")));
        item.set("Parent", Object::Reference(parent));
        item.set("Dest", destination(PAGE_1));
        let id = doc.add_object(Object::Dictionary(item));
        point(&mut doc, parent, "First", id);
        point(&mut doc, parent, "Last", id);
        ids.push(id);
        parent = id;
    }
    (doc, ids)
}

fn saved(doc: &mut Document) -> Vec<u8> {
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save");
    out
}

// No loop at all here: an honest chain, just very deep. A visited set would
// not have saved the old recursive walk from this one; only not recursing does.
#[test]
fn a_chain_nested_100_000_deep_is_walked_without_recursion() {
    let (mut doc, _) = nested_chain(100_000);
    let bytes = saved(&mut doc);
    assert!(prune_dangling_destinations(bytes.clone()) == bytes, "a file with nothing to prune was rewritten");

    // And the walk does reach the bottom: the deepest item's dead destination
    // goes, while the live one halfway down stays.
    let (mut doc, ids) = nested_chain(2_000);
    let (halfway, deepest) = (ids[1_000], ids[1_999]);
    dangle(&mut doc, deepest);
    let pruned = Document::load_mem(&prune_dangling_destinations(saved(&mut doc))).expect("load");
    assert!(!has_destination(&pruned, deepest), "the deepest bookmark kept a destination to a missing page");
    assert!(has_destination(&pruned, halfway), "a live destination was removed");
}

// SPEC: P2-PAGE-008 still holds for the file beside the damaged one: every
// bookmark of both sources, once each, in order.
#[test]
fn merging_a_file_with_a_looping_outline_returns_and_keeps_the_other_bookmarks() {
    let dir = scratch();
    let both = [ALL_THREE, ALL_THREE].concat();

    for (damage, bytes) in [("a loop back to the first item", looping()), ("a chain into the outline itself", edited(into_the_outline))] {
        let damaged = dir.join("damaged.pdf");
        std::fs::write(&damaged, bytes).expect("write");
        for (order, sources) in [("damaged first", [damaged.clone(), fixture()]), ("damaged second", [fixture(), damaged.clone()])] {
            let out = dir.join("merged.pdf");
            merge_documents(&sources, &out).expect("merge");
            assert_eq!(page_count(&out), 12, "{damage}, {order}");
            let merged = std::fs::read(&out).expect("read");
            assert_eq!(titles(&merged), both, "{damage}, {order}");
            assert_eq!(pdfium_top_level_count(&merged), 6, "{damage}, {order}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Split `bytes` by its top-level bookmarks; the page count of each file
/// written, in order.
async fn split_page_counts(bytes: &[u8]) -> Vec<u32> {
    let dir = scratch();
    let input = dir.join("in.pdf");
    std::fs::write(&input, bytes).expect("write");
    let out_dir = dir.join("out");
    std::fs::create_dir_all(&out_dir).expect("out dir");

    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), input, None).expect("opens");
    let outcome = handle.split_document(SplitMode::Bookmarks, out_dir.clone(), "doc".into()).await.expect("split by bookmarks");
    drop(handle);

    let mut files: Vec<PathBuf> = std::fs::read_dir(&out_dir).expect("read dir").filter_map(|e| e.ok().map(|e| e.path())).collect();
    files.sort();
    assert_eq!(outcome.files.len(), files.len());
    let counts = files.iter().map(|file| page_count(file)).collect();
    let _ = std::fs::remove_dir_all(&dir);
    counts
}

// SPEC: P2-PAGE-007. The same three files as the undamaged fixture gives:
// the loop adds no boundaries, and the split comes back.
#[tokio::test]
async fn splitting_by_bookmarks_on_a_looping_outline_returns() {
    assert_eq!(split_page_counts(&looping()).await, [2, 2, 2]);
}

// SPEC: P2-PAGE-007. In the fixture the first bookmark is on page 1, where a
// file starts anyway, so it could go unread and nothing would show. Here it
// is on page 2, and has to start a file of its own.
#[tokio::test]
async fn splitting_counts_the_first_bookmark_as_well_as_its_siblings() {
    let moved = edited(|doc| doc.get_dictionary_mut(CHAPTER_1).expect("item").set("Dest", destination(PAGE_2)));
    assert_eq!(split_page_counts(&moved).await, [1, 1, 2, 2]);
}

/// `Sample PDFs/scratch/` — the files a person tries in the app
/// (steps/P7-VERIFY.md §1.12): a bookmark that is its own child, a top level
/// that loops, and a copy of the ordinary fixture.
/// `node scripts/cargo-test.mjs --test outline_cycles writes_the_verification -- --ignored`
#[test]
#[ignore = "writes verification files for a human to open"]
fn writes_the_verification_files() {
    let dir = PathBuf::from("../Sample PDFs/scratch");
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("outline-self-loop.pdf"), edited(|doc| point(doc, CHAPTER_1, "First", CHAPTER_1))).expect("write");
    std::fs::write(dir.join("outline-next-loop.pdf"), looping()).expect("write");
    std::fs::copy(fixture(), dir.join("bookmarks.pdf")).expect("copy");
}
