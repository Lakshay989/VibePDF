//! SPEC: P7-OCR-002 — the twelve languages a build ships, and the two ways a
//! thirteenth arrives: from a file, or by a download the user switched on.
//!
//! The end-to-end case is Russian, because it is the cheapest proof that the
//! non-Latin half works: Cyrillic is outside WinAnsi, so recognising it is only
//! half the job — the words also have to be written with an embedded font, or
//! the "searchable" PDF contains nothing at all.

use std::path::PathBuf;

use vibepdf_lib::error::CommandError;
use vibepdf_lib::ocr::languages::{
    self, download_pack, install_pack, list, remove_pack, PackSource, BUNDLED,
};
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::ocr_text_layer::OcrOptions;

/// Words the Cyrillic fixture was drawn with (see `writes_the_cyrillic_fixture`).
const RUSSIAN_WORDS: &[&str] = &["Счёт", "номер", "документ"];

fn bundled_dir() -> PathBuf {
    PathBuf::from("resources/tessdata")
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vibepdf-lang-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

#[test]
fn the_twelve_spec_languages_are_installed() {
    // SPEC: P7-OCR-002 — "SHALL ship with OCR support for" these twelve. If the
    // fetch script and the code list ever disagree, this is where it shows.
    let installed = vibepdf_lib::ocr::tessdata::installed_languages();
    let missing: Vec<&str> = BUNDLED
        .iter()
        .map(|(code, _)| *code)
        .filter(|code| !installed.contains(&(*code).to_owned()))
        .collect();
    assert!(
        missing.is_empty(),
        "run `npm run fetch-tessdata`; missing {missing:?}"
    );
}

#[test]
fn every_bundled_language_actually_loads() {
    // Present on disk is not the same as usable: a truncated model would pass
    // the test above and fail inside a document.
    for (code, name) in BUNDLED {
        vibepdf_lib::ocr::engine::OcrEngine::with_datapath(&bundled_dir(), code)
            .unwrap_or_else(|e| panic!("{name} ({code}) did not load: {e:?}"));
    }
}

#[test]
fn a_pack_installed_from_a_file_becomes_usable() {
    let user_dir = scratch("install");
    let source = bundled_dir().join("eng.traineddata");
    let copied = user_dir.join("tst.traineddata");
    std::fs::copy(&source, &copied).expect("copy");

    let pack = languages::install_from_file(&user_dir, &copied).expect("installs");
    assert_eq!(pack.code, "tst");
    assert_eq!(pack.source, PackSource::Added);
    assert!(
        list(&user_dir).iter().any(|p| p.code == "tst"),
        "the added pack is not listed"
    );
    vibepdf_lib::ocr::engine::OcrEngine::with_datapath(&user_dir, "tst")
        .expect("the installed pack loads");
    let _ = std::fs::remove_dir_all(&user_dir);
}

#[test]
fn a_file_that_is_not_language_data_is_refused() {
    // The check that matters: Tesseract itself has to accept it. Anything else
    // installs a pack that fails later, inside someone's document.
    let user_dir = scratch("corrupt");
    let err = install_pack(&user_dir, "bad", b"this is not a tesseract model at all")
        .expect_err("must be refused");
    assert!(matches!(err, CommandError::InvalidInput(_)), "got {err:?}");
    assert!(
        !user_dir.join("bad.traineddata").exists(),
        "a refused pack was installed anyway"
    );
    assert!(
        list(&user_dir).is_empty() || list(&user_dir).iter().all(|p| p.code != "bad"),
        "a refused pack is listed"
    );
    let _ = std::fs::remove_dir_all(&user_dir);
}

#[test]
fn a_nonsense_language_code_is_refused() {
    let user_dir = scratch("code");
    for code in ["", "../escape", "a b", &"x".repeat(33)] {
        assert!(
            install_pack(&user_dir, code, b"whatever").is_err(),
            "{code:?} was accepted as a language code"
        );
    }
    let _ = std::fs::remove_dir_all(&user_dir);
}

#[test]
fn added_packs_can_be_removed_and_bundled_ones_cannot() {
    let user_dir = scratch("remove");
    std::fs::copy(bundled_dir().join("eng.traineddata"), user_dir.join("tst.traineddata"))
        .expect("copy");
    remove_pack(&user_dir, "tst").expect("removes an added pack");
    assert!(!user_dir.join("tst.traineddata").exists());

    // Bundled languages are part of the build; there is nothing to remove.
    let err = remove_pack(&user_dir, "eng").expect_err("bundled packs stay");
    assert!(matches!(err, CommandError::NotFound(_)), "got {err:?}");
    let _ = std::fs::remove_dir_all(&user_dir);
}

#[test]
fn downloads_are_refused_until_they_are_switched_on() {
    // The offline-first guarantee: no network call can happen from a default
    // install. This test reaches the guard and stops — it never has a network.
    let user_dir = scratch("download-off");
    let config = scratch("download-off-cfg");
    let err = download_pack(&user_dir, &config, "ell").expect_err("must refuse");
    match err {
        CommandError::PermissionDenied(message) => {
            assert!(
                message.contains("switched off"),
                "the message should say what to do, was: {message}"
            );
        }
        other => panic!("expected PermissionDenied, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&user_dir);
    let _ = std::fs::remove_dir_all(&config);
}

/// OCR a Cyrillic scan and read the words back out of the written PDF.
#[tokio::test]
async fn a_russian_scan_becomes_searchable_in_russian() {
    // SPEC: P7-OCR-002 — the half that is easy to get wrong. Cyrillic is
    // outside WinAnsi, so these words only survive if the text layer embeds a
    // font for them; before that, this page recognised fine and wrote nothing.
    let handle = DocumentActorHandle::spawn(
        None,
        uuid::Uuid::new_v4(),
        fixture("scan-cyrillic.pdf"),
        None,
    )
    .expect("opens");

    let (summary, _) = handle
        .run_ocr(
            vec![0],
            OcrOptions {
                language: "rus".into(),
                ..OcrOptions::default()
            },
        )
        .await
        .expect("ocr runs");
    assert!(summary.words > 0, "nothing was written: {summary:?}");

    let text: String = handle
        .read_text_runs(0)
        .await
        .expect("reads text runs")
        .iter()
        .map(|run| run.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let missing: Vec<&str> = RUSSIAN_WORDS
        .iter()
        .copied()
        .filter(|w| !text.contains(w))
        .collect();
    assert!(
        missing.is_empty(),
        "OCR did not make {missing:?} searchable; the page reads: {text}"
    );
}

#[tokio::test]
async fn a_mixed_script_page_keeps_its_reading_order() {
    // The fixture reads "Счёт номер 4821" then "документ от 14 марта", so the
    // digits sit *inside* Cyrillic lines. Writing every Latin word first and
    // every Cyrillic word after — which is what grouping by font does — puts a
    // page's text in the wrong order for every reader, because extraction
    // follows the content stream. Measured 2026-09-22 with PDF.js, which read
    // "4821 14 Счёт номер документ от марта" before this was fixed.
    let handle = DocumentActorHandle::spawn(
        None,
        uuid::Uuid::new_v4(),
        fixture("scan-cyrillic.pdf"),
        None,
    )
    .expect("opens");
    handle
        .run_ocr(
            vec![0],
            OcrOptions {
                language: "rus".into(),
                ..OcrOptions::default()
            },
        )
        .await
        .expect("ocr runs");

    let text: String = handle
        .read_text_runs(0)
        .await
        .expect("reads text runs")
        .iter()
        .map(|run| run.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let at = |needle: &str| {
        text.find(needle)
            .unwrap_or_else(|| panic!("{needle:?} missing from {text}"))
    };
    assert!(at("Счёт") < at("4821"), "the first line is out of order: {text}");
    assert!(at("4821") < at("документ"), "the lines are out of order: {text}");
}

/// Writes `Sample PDFs/vibepdf-verify-ocr-cyrillic.pdf` for the reader check.
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-ocr-cyrillic.pdf");
    std::fs::copy(fixture("scan-cyrillic.pdf"), &out).expect("copy fixture");
    let handle =
        DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), out.clone(), None).expect("opens");
    let (summary, _) = handle
        .run_ocr(
            vec![0],
            OcrOptions {
                language: "rus".into(),
                ..OcrOptions::default()
            },
        )
        .await
        .expect("ocr runs");
    handle.save(None).await.expect("save");
    drop(handle);
    let _ = std::fs::remove_file(out.with_extension("pdf.bak"));
    println!("wrote {} — {} words", out.display(), summary.words);
}
