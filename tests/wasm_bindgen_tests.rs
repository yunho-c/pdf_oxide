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
use pdf_oxide::geometry::Rect;
use pdf_oxide::wasm::{WasmPdf, WasmPdfDocument};
use pdf_oxide::writer::{CheckboxWidget, ComboBoxWidget, PdfWriter, TextFieldWidget};

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

// ============================================================================
// Form Fields (Issue #172) — getFormFields, hasXfa
// ============================================================================

/// Create a PDF with form fields for WASM testing.
fn make_form_pdf() -> Vec<u8> {
    let mut writer = PdfWriter::new();
    {
        let mut page = writer.add_page(612.0, 792.0);
        page.add_text_field(
            TextFieldWidget::new("name", Rect::new(72.0, 700.0, 200.0, 20.0))
                .with_value("Alice"),
        );
        page.add_checkbox(
            CheckboxWidget::new("agree", Rect::new(72.0, 650.0, 15.0, 15.0)).checked(),
        );
        page.add_combo_box(
            ComboBoxWidget::new("color", Rect::new(72.0, 600.0, 150.0, 20.0))
                .with_options(vec!["Red", "Blue", "Green"])
                .with_value("Blue"),
        );
    }
    writer.finish().expect("Failed to create form PDF")
}

#[wasm_bindgen_test]
fn test_get_form_fields_returns_array() {
    let bytes = make_form_pdf();
    let mut doc = WasmPdfDocument::new(&bytes).unwrap();
    let result = doc.get_form_fields().unwrap();
    assert!(
        js_sys::Array::is_array(&result),
        "getFormFields should return an array"
    );
    let arr = js_sys::Array::from(&result);
    assert!(
        arr.length() >= 3,
        "Should have at least 3 form fields, got {}",
        arr.length()
    );
}

#[wasm_bindgen_test]
fn test_get_form_fields_has_name_and_type() {
    let bytes = make_form_pdf();
    let mut doc = WasmPdfDocument::new(&bytes).unwrap();
    let result = doc.get_form_fields().unwrap();
    let arr = js_sys::Array::from(&result);

    // Check first field has name and field_type
    let first = arr.get(0);
    let name = js_sys::Reflect::get(&first, &JsValue::from_str("name")).unwrap();
    assert!(name.is_string(), "field should have a string 'name'");
    let ft = js_sys::Reflect::get(&first, &JsValue::from_str("field_type")).unwrap();
    assert!(ft.is_string(), "field should have a string 'field_type'");
}

#[wasm_bindgen_test]
fn test_get_form_fields_has_value() {
    let bytes = make_form_pdf();
    let mut doc = WasmPdfDocument::new(&bytes).unwrap();
    let result = doc.get_form_fields().unwrap();
    let arr = js_sys::Array::from(&result);

    // Find the text field — it should have a string value
    for i in 0..arr.length() {
        let field = arr.get(i);
        let ft = js_sys::Reflect::get(&field, &JsValue::from_str("field_type")).unwrap();
        if ft.as_string().as_deref() == Some("text") {
            let value = js_sys::Reflect::get(&field, &JsValue::from_str("value")).unwrap();
            assert!(
                value.is_string(),
                "text field should have a string value"
            );
            return;
        }
    }
    // If no text field found, the test is inconclusive (but it should find one)
}

#[wasm_bindgen_test]
fn test_get_form_fields_empty_on_plain_pdf() {
    let mut doc = doc_from_text("No forms here");
    let result = doc.get_form_fields().unwrap();
    let arr = js_sys::Array::from(&result);
    assert_eq!(
        arr.length(),
        0,
        "Plain text PDF should have no form fields"
    );
}

#[wasm_bindgen_test]
fn test_has_xfa_false_on_plain_pdf() {
    let mut doc = doc_from_text("No XFA");
    let result = doc.has_xfa().unwrap();
    assert!(!result, "Plain text PDF should not have XFA");
}

#[wasm_bindgen_test]
fn test_has_xfa_false_on_acroform_pdf() {
    let bytes = make_form_pdf();
    let mut doc = WasmPdfDocument::new(&bytes).unwrap();
    let result = doc.has_xfa().unwrap();
    assert!(!result, "PdfWriter-created form should not have XFA");
}

// ============================================================================
// Form Field Get/Set Values — new bindings
// ============================================================================

#[wasm_bindgen_test]
fn test_set_and_get_form_field_value() {
    let bytes = make_form_pdf();
    let mut doc = WasmPdfDocument::new(&bytes).unwrap();

    // Set a text field value
    doc.set_form_field_value("name", JsValue::from_str("Bob")).unwrap();

    // Get it back
    let result = doc.get_form_field_value("name").unwrap();
    assert!(result.is_string(), "text field value should be a string");
    assert_eq!(result.as_string().unwrap(), "Bob");
}

#[wasm_bindgen_test]
fn test_set_checkbox_form_field() {
    let bytes = make_form_pdf();
    let mut doc = WasmPdfDocument::new(&bytes).unwrap();

    // Set checkbox to true
    doc.set_form_field_value("agree", JsValue::from(true)).unwrap();

    // Get it back
    let result = doc.get_form_field_value("agree").unwrap();
    assert_eq!(result.as_bool(), Some(true), "checkbox should be true");
}

#[wasm_bindgen_test]
fn test_get_form_field_value_not_found() {
    let bytes = make_form_pdf();
    let mut doc = WasmPdfDocument::new(&bytes).unwrap();

    let result = doc.get_form_field_value("nonexistent_field").unwrap();
    assert!(result.is_null(), "non-existent field should return null");
}

// ============================================================================
// Image Bytes Extraction — new bindings
// ============================================================================

#[wasm_bindgen_test]
fn test_extract_image_bytes_empty() {
    let mut doc = doc_from_text("No images");
    let result = doc.extract_image_bytes(0).unwrap();
    assert!(js_sys::Array::is_array(&result), "should return an array");
    let arr = js_sys::Array::from(&result);
    assert_eq!(arr.length(), 0, "text-only PDF should have no images");
}

// ============================================================================
// PDF from Images — new bindings
// ============================================================================

/// Create a minimal valid 1x1 white JPEG image (known-good bytes).
fn create_minimal_image() -> Vec<u8> {
    vec![
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00,
        0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06,
        0x07, 0x06, 0x05, 0x08, 0x07, 0x07, 0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D,
        0x0C, 0x0B, 0x0B, 0x0C, 0x19, 0x12, 0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D,
        0x1A, 0x1C, 0x1C, 0x20, 0x24, 0x2E, 0x27, 0x20, 0x22, 0x2C, 0x23, 0x1C, 0x1C, 0x28,
        0x37, 0x29, 0x2C, 0x30, 0x31, 0x34, 0x34, 0x34, 0x1F, 0x27, 0x39, 0x3D, 0x38, 0x32,
        0x3C, 0x2E, 0x33, 0x34, 0x32, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01, 0x00, 0x01,
        0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00, 0x1F, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01,
        0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02,
        0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0xFF, 0xC4, 0x00, 0xB5, 0x10,
        0x00, 0x02, 0x01, 0x03, 0x03, 0x02, 0x04, 0x03, 0x05, 0x05, 0x04, 0x04, 0x00, 0x00,
        0x01, 0x7D, 0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06,
        0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08, 0x23, 0x42,
        0xB1, 0xC1, 0x15, 0x52, 0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A, 0x16,
        0x17, 0x18, 0x19, 0x1A, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x34, 0x35, 0x36, 0x37,
        0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55,
        0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73,
        0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
        0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5,
        0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA,
        0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6,
        0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA,
        0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFF, 0xDA, 0x00, 0x08,
        0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0xFB, 0xD5, 0xDB, 0x20, 0xA8, 0xF9, 0xFF, 0xD9,
    ]
}

#[wasm_bindgen_test]
fn test_pdf_from_image_bytes() {
    let png = create_minimal_image();
    let result = WasmPdf::from_image_bytes(&png);
    assert!(result.is_ok(), "fromImageBytes should succeed with valid PNG");
    let pdf = result.unwrap();
    assert!(pdf.size() > 0, "PDF should have content");

    // Verify it's a valid PDF we can reopen
    let mut doc = WasmPdfDocument::new(&pdf.to_bytes()).unwrap();
    assert_eq!(doc.page_count().unwrap(), 1, "should have 1 page");
}

#[wasm_bindgen_test]
fn test_pdf_from_multiple_image_bytes() {
    let png1 = create_minimal_image();
    let png2 = create_minimal_image();
    let arr = js_sys::Array::new();
    arr.push(&js_sys::Uint8Array::from(png1.as_slice()));
    arr.push(&js_sys::Uint8Array::from(png2.as_slice()));

    let result = WasmPdf::from_multiple_image_bytes(arr.into());
    assert!(result.is_ok(), "fromMultipleImageBytes should succeed");
    let pdf = result.unwrap();

    let mut doc = WasmPdfDocument::new(&pdf.to_bytes()).unwrap();
    assert_eq!(doc.page_count().unwrap(), 2, "should have 2 pages");
}

// ============================================================================
// Form Flattening — new bindings
// ============================================================================

#[wasm_bindgen_test]
fn test_flatten_forms() {
    let bytes = make_form_pdf();
    let mut doc = WasmPdfDocument::new(&bytes).unwrap();

    // Verify we have form fields before flattening
    let fields_before = doc.get_form_fields().unwrap();
    let arr_before = js_sys::Array::from(&fields_before);
    assert!(arr_before.length() >= 3, "should have fields before flatten");

    // Flatten
    doc.flatten_forms().unwrap();

    // After flatten + save/reload, form fields should be gone
    let saved = doc.save_to_bytes().unwrap();
    let mut doc2 = WasmPdfDocument::new(&saved).unwrap();
    let fields_after = doc2.get_form_fields().unwrap();
    let arr_after = js_sys::Array::from(&fields_after);
    assert_eq!(arr_after.length(), 0, "should have no fields after flatten");
}

// ============================================================================
// PDF Merging — new bindings
// ============================================================================

#[wasm_bindgen_test]
fn test_merge_from() {
    let bytes1 = make_text_pdf("Document 1");
    let bytes2 = make_text_pdf("Document 2");
    let mut doc = WasmPdfDocument::new(&bytes1).unwrap();

    let count = doc.merge_from(&bytes2).unwrap();
    assert_eq!(count, 1, "should merge 1 page");
}

// ============================================================================
// File Embedding — new bindings
// ============================================================================

#[wasm_bindgen_test]
fn test_embed_file() {
    let mut doc = doc_from_text("Hello");
    doc.embed_file("test.txt", b"Hello embedded").unwrap();

    // Should be able to save without error
    let bytes = doc.save_to_bytes().unwrap();
    assert!(bytes.starts_with(b"%PDF"));
}

// ============================================================================
// Page Labels — new bindings
// ============================================================================

#[wasm_bindgen_test]
fn test_page_labels_empty() {
    let mut doc = doc_from_text("Hello");
    let result = doc.page_labels().unwrap();
    assert!(js_sys::Array::is_array(&result), "should return an array");
}

// ============================================================================
// XMP Metadata — new bindings
// ============================================================================

#[wasm_bindgen_test]
fn test_xmp_metadata_null_or_object() {
    let mut doc = doc_from_text("Hello");
    let result = doc.xmp_metadata().unwrap();
    // Simple generated PDF may or may not have XMP
    assert!(
        result.is_null() || result.is_object(),
        "xmpMetadata should return null or an object"
    );
}
