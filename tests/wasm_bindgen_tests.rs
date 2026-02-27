//! Integration tests for WASM bindings using wasm-bindgen-test.
//!
//! These tests run in a real JS environment (Node.js/browser) and can fully
//! inspect JsValue contents via js_sys::Reflect. They cover what native tests
//! cannot — structured extraction, search results, and error paths.
//!
//! Run with: wasm-pack test --headless --node --features wasm

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

use pdf_oxide::api::{Pdf, PdfBuilder};
use pdf_oxide::wasm::{WasmPdf, WasmPdfDocument};

wasm_bindgen_test_configure!(run_in_browser);

// ============================================================================
// Test Helpers
// ============================================================================

fn make_text_pdf(text: &str) -> Vec<u8> {
    Pdf::from_text(text).unwrap().into_bytes()
}

fn doc_from_text(text: &str) -> WasmPdfDocument {
    WasmPdfDocument::new(&make_text_pdf(text)).unwrap()
}

// ============================================================================
// Constructor — error paths
// ============================================================================

#[wasm_bindgen_test]
fn test_new_invalid_bytes() {
    let result = WasmPdfDocument::new(b"not a pdf");
    assert!(result.is_err());
}

#[wasm_bindgen_test]
fn test_new_empty_bytes() {
    let result = WasmPdfDocument::new(b"");
    assert!(result.is_err());
}

// ============================================================================
// Structured Extraction — inspect JsValue contents
// ============================================================================

#[wasm_bindgen_test]
fn test_extract_chars_returns_array() {
    let mut doc = doc_from_text("ABC");
    let result = doc.extract_chars(0).unwrap();
    assert!(js_sys::Array::is_array(&result), "extract_chars should return an array");
    let arr = js_sys::Array::from(&result);
    assert!(arr.length() > 0, "should have at least one char");

    // Inspect first char object
    let first = arr.get(0);
    let char_val = js_sys::Reflect::get(&first, &JsValue::from_str("char")).unwrap();
    assert!(char_val.is_string(), "char field should be a string");
}

#[wasm_bindgen_test]
fn test_extract_chars_has_bbox() {
    let mut doc = doc_from_text("X");
    let result = doc.extract_chars(0).unwrap();
    let arr = js_sys::Array::from(&result);
    let first = arr.get(0);
    let bbox = js_sys::Reflect::get(&first, &JsValue::from_str("bbox")).unwrap();
    assert!(!bbox.is_undefined(), "char should have a bbox field");
}

#[wasm_bindgen_test]
fn test_extract_chars_has_font_name() {
    let mut doc = doc_from_text("A");
    let result = doc.extract_chars(0).unwrap();
    let arr = js_sys::Array::from(&result);
    let first = arr.get(0);
    let font = js_sys::Reflect::get(&first, &JsValue::from_str("font_name")).unwrap();
    assert!(!font.is_undefined(), "char should have a font_name field");
}

#[wasm_bindgen_test]
fn test_extract_chars_invalid_page() {
    let mut doc = doc_from_text("ABC");
    let result = doc.extract_chars(999);
    assert!(result.is_err());
}

#[wasm_bindgen_test]
fn test_extract_spans_returns_array() {
    let mut doc = doc_from_text("Hello spans test");
    let result = doc.extract_spans(0).unwrap();
    assert!(js_sys::Array::is_array(&result), "extract_spans should return an array");
    let arr = js_sys::Array::from(&result);
    assert!(arr.length() > 0, "should have at least one span");

    // Inspect first span
    let first = arr.get(0);
    let text = js_sys::Reflect::get(&first, &JsValue::from_str("text")).unwrap();
    assert!(text.is_string(), "span should have a text field");
}

#[wasm_bindgen_test]
fn test_extract_spans_has_font_size() {
    let mut doc = doc_from_text("Hello");
    let result = doc.extract_spans(0).unwrap();
    let arr = js_sys::Array::from(&result);
    let first = arr.get(0);
    let font_size = js_sys::Reflect::get(&first, &JsValue::from_str("font_size")).unwrap();
    assert!(!font_size.is_undefined(), "span should have font_size");
}

// ============================================================================
// Search — inspect JsValue result structure
// ============================================================================

#[wasm_bindgen_test]
fn test_search_returns_array() {
    let mut doc = doc_from_text("Hello world search test");
    let result = doc.search("Hello", None, Some(true), None, None).unwrap();
    assert!(js_sys::Array::is_array(&result), "search should return an array");
}

#[wasm_bindgen_test]
fn test_search_result_has_fields() {
    let mut doc = doc_from_text("Hello world");
    let result = doc.search("Hello", None, Some(true), None, None).unwrap();
    let arr = js_sys::Array::from(&result);
    if arr.length() > 0 {
        let first = arr.get(0);
        let page = js_sys::Reflect::get(&first, &JsValue::from_str("page")).unwrap();
        assert!(!page.is_undefined(), "search result should have page field");
        let text = js_sys::Reflect::get(&first, &JsValue::from_str("text")).unwrap();
        assert!(!text.is_undefined(), "search result should have text field");
    }
}

#[wasm_bindgen_test]
fn test_search_not_found_empty_array() {
    let mut doc = doc_from_text("Hello world");
    let result = doc.search("ZZZZZ_NONEXISTENT", None, Some(true), None, None).unwrap();
    let arr = js_sys::Array::from(&result);
    assert_eq!(arr.length(), 0, "search for nonexistent text should return empty array");
}

#[wasm_bindgen_test]
fn test_search_page() {
    let mut doc = doc_from_text("Hello page search");
    let result = doc.search_page(0, "Hello", None, Some(true), None, None).unwrap();
    assert!(js_sys::Array::is_array(&result));
}

#[wasm_bindgen_test]
fn test_search_case_insensitive() {
    let mut doc = doc_from_text("Hello World");
    let result = doc.search("hello", Some(true), Some(true), None, None).unwrap();
    let arr = js_sys::Array::from(&result);
    assert!(arr.length() > 0, "case-insensitive search should find 'hello' in 'Hello World'");
}

// ============================================================================
// Image Info — inspect JsValue structure
// ============================================================================

#[wasm_bindgen_test]
fn test_extract_images_returns_array() {
    let mut doc = doc_from_text("No images");
    let result = doc.extract_images(0).unwrap();
    assert!(js_sys::Array::is_array(&result));
    let arr = js_sys::Array::from(&result);
    // Text-only PDF — expect 0 images
    assert_eq!(arr.length(), 0, "text-only PDF should have no images");
}

#[wasm_bindgen_test]
fn test_extract_images_invalid_page() {
    let mut doc = doc_from_text("Hello");
    let result = doc.extract_images(999);
    assert!(result.is_err());
}

// ============================================================================
// Page properties — JsValue paths
// ============================================================================

#[wasm_bindgen_test]
fn test_page_crop_box_null_when_unset() {
    let mut doc = doc_from_text("Hello");
    let result = doc.page_crop_box(0).unwrap();
    // CropBox is typically not set on generated PDFs
    if result.is_null() {
        // Expected: no crop box
    } else {
        // Some PDFs may set CropBox equal to MediaBox
        assert!(js_sys::Array::is_array(&result));
    }
}

#[wasm_bindgen_test]
fn test_page_rotation_invalid_page() {
    let mut doc = doc_from_text("Hello");
    let result = doc.page_rotation(999);
    assert!(result.is_err());
}

// ============================================================================
// Erase — error validation
// ============================================================================

#[wasm_bindgen_test]
fn test_erase_regions_invalid_length() {
    let mut doc = doc_from_text("Hello");
    let rects = [0.0, 0.0, 100.0]; // Not a multiple of 4
    let result = doc.erase_regions(0, &rects);
    assert!(result.is_err());
}

// ============================================================================
// Page Images — inspect structure
// ============================================================================

#[wasm_bindgen_test]
fn test_page_images_returns_array() {
    let mut doc = doc_from_text("Hello");
    let result = doc.page_images(0).unwrap();
    assert!(js_sys::Array::is_array(&result));
}

// ============================================================================
// Text extraction — error paths
// ============================================================================

#[wasm_bindgen_test]
fn test_extract_text_invalid_page() {
    let mut doc = doc_from_text("Hello");
    let result = doc.extract_text(999);
    assert!(result.is_err());
}

// ============================================================================
// Full roundtrip: create → edit → save → reopen → verify
// ============================================================================

#[wasm_bindgen_test]
fn test_full_roundtrip() {
    // Create a PDF
    let mut doc = doc_from_text("Roundtrip WASM test");

    // Edit metadata
    doc.set_title("WASM Title").unwrap();
    doc.set_author("WASM Author").unwrap();

    // Set rotation
    doc.set_page_rotation(0, 90).unwrap();

    // Save
    let bytes = doc.save_to_bytes().unwrap();
    assert!(bytes.starts_with(b"%PDF"));

    // Reopen
    let mut doc2 = WasmPdfDocument::new(&bytes).unwrap();
    assert_eq!(doc2.page_count().unwrap(), 1);

    // Verify text preserved
    let text = doc2.extract_text(0).unwrap();
    assert!(text.contains("Roundtrip"), "text should survive roundtrip");

    // Verify rotation preserved
    let rotation = doc2.page_rotation(0).unwrap();
    assert_eq!(rotation, 90, "rotation should survive roundtrip");
}

#[wasm_bindgen_test]
fn test_encrypted_roundtrip() {
    let mut doc = doc_from_text("Encrypted content");
    let bytes = doc
        .save_encrypted_to_bytes("mypass", None, None, None, None, None)
        .unwrap();
    assert!(bytes.starts_with(b"%PDF"));

    // Reopen and authenticate
    let mut doc2 = WasmPdfDocument::new(&bytes).unwrap();
    let auth = doc2.authenticate("mypass").unwrap();
    assert!(auth, "should authenticate with correct password");
}

// ============================================================================
// WasmPdf creation — verify content
// ============================================================================

#[wasm_bindgen_test]
fn test_wasm_pdf_from_markdown_roundtrip() {
    let pdf = WasmPdf::from_markdown("# Hello\n\nWorld content", None, None).unwrap();
    let mut doc = WasmPdfDocument::new(&pdf.to_bytes()).unwrap();
    let text = doc.extract_all_text().unwrap();
    assert!(!text.is_empty());
}

#[wasm_bindgen_test]
fn test_wasm_pdf_from_html_roundtrip() {
    let pdf = WasmPdf::from_html("<p>HTML content here</p>", None, None).unwrap();
    let mut doc = WasmPdfDocument::new(&pdf.to_bytes()).unwrap();
    let text = doc.extract_all_text().unwrap();
    assert!(!text.is_empty());
}

#[wasm_bindgen_test]
fn test_wasm_pdf_from_text_roundtrip() {
    let pdf = WasmPdf::from_text("Plain text content", None, None).unwrap();
    let mut doc = WasmPdfDocument::new(&pdf.to_bytes()).unwrap();
    let text = doc.extract_all_text().unwrap();
    assert!(text.contains("Plain"), "should contain source text");
}

// ============================================================================
// Outline, Annotations, Paths — new WASM bindings
// ============================================================================

#[wasm_bindgen_test]
fn test_get_outline_returns_null_or_array() {
    let mut doc = doc_from_text("No outline here");
    let result = doc.get_outline().unwrap();
    // Text-only generated PDF has no outline, expect null
    assert!(
        result.is_null() || js_sys::Array::is_array(&result),
        "getOutline should return null or an array"
    );
}

#[wasm_bindgen_test]
fn test_get_annotations_returns_array() {
    let mut doc = doc_from_text("No annotations");
    let result = doc.get_annotations(0).unwrap();
    assert!(
        js_sys::Array::is_array(&result),
        "getAnnotations should return an array"
    );
    let arr = js_sys::Array::from(&result);
    // Text-only PDF — expect 0 annotations
    assert_eq!(arr.length(), 0, "text-only PDF should have no annotations");
}

#[wasm_bindgen_test]
fn test_get_annotations_invalid_page() {
    let mut doc = doc_from_text("Hello");
    let result = doc.get_annotations(999);
    assert!(result.is_err(), "invalid page should return error");
}

#[wasm_bindgen_test]
fn test_extract_paths_returns_array() {
    let mut doc = doc_from_text("No paths");
    let result = doc.extract_paths(0).unwrap();
    assert!(
        js_sys::Array::is_array(&result),
        "extractPaths should return an array"
    );
}

#[wasm_bindgen_test]
fn test_extract_paths_invalid_page() {
    let mut doc = doc_from_text("Hello");
    let result = doc.extract_paths(999);
    assert!(result.is_err(), "invalid page should return error");
}

// ============================================================================
// PDF creation — metadata verification
// ============================================================================

#[wasm_bindgen_test]
fn test_wasm_pdf_metadata() {
    let pdf = WasmPdf::from_text(
        "With metadata",
        Some("My Title".to_string()),
        Some("My Author".to_string()),
    )
    .unwrap();
    assert!(pdf.size() > 0);
    let bytes = pdf.to_bytes();
    assert!(bytes.starts_with(b"%PDF"));
}
