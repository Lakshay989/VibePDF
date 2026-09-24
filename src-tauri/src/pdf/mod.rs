pub mod actor;
pub mod annotation;
pub mod autosave;
pub mod background;
pub mod bates;
pub mod clean;
pub mod compress;
pub mod cos;
pub mod crop;
pub mod delete_page;
pub mod doc_cache;
pub mod document;
pub mod export_docx;
pub mod export_image;
pub mod export_text;
pub mod extract;
pub mod flatten;
pub mod font_embed_cid;
pub mod font_metrics;
pub mod font_resolver;
pub mod form;
pub mod form_data;
pub mod form_flatten;
pub mod form_import;
pub mod header_footer;
pub mod image_edit;
pub mod image_extract;
pub mod image_xobject;
pub mod incremental_save;
pub mod insert_blank;
pub mod insert_from;
pub mod merge;
pub mod ocr_text_layer;
pub mod ooxml;
pub mod page_numbers;
pub mod reflow;
pub mod render;
pub mod reorder;
pub mod resize;
pub mod restore;
pub mod rotate;
pub mod split;
pub mod table_detect;
pub mod text_extract;
pub mod undo;
pub mod watermark;
pub mod xfdf;

/// Register where user-installed OCR language packs live (SPEC: P7-OCR-002).
/// Re-exported here so `lib.rs`'s setup hook has one thing to call.
pub fn ocr_user_tessdata(dir: std::path::PathBuf) {
    crate::ocr::tessdata::set_user_dir(dir);
}
