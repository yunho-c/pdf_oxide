//! OCR (Optical Character Recognition) module for scanned PDF text extraction.
//!
//! This module provides PaddleOCR-based text extraction for scanned PDFs using
//! ONNX Runtime for CPU-only inference. It integrates seamlessly with the existing
//! text extraction pipeline.
//!
//! # Features
//!
//! - **Auto-detect scanned pages**: Automatically identify pages that need OCR
//! - **Unified output**: OCR results match the format of native text extraction
//! - **Style detection**: Infer font sizes and heading styles from OCR geometry
//! - **Fast CPU inference**: Target < 1 second per A4 page on modern CPU
//!
//! # Architecture
//!
//! The OCR pipeline consists of:
//! 1. **Preprocessing**: Image resizing, normalization, tensor conversion
//! 2. **Detection**: DBNet++ model finds text regions (bounding boxes)
//! 3. **Recognition**: SVTR model reads text from cropped regions
//! 4. **Postprocessing**: Convert OCR results to TextSpan format
//!
//! # Example
//!
//! ```ignore
//! use pdf_oxide::{PdfDocument, ocr::OcrEngine};
//!
//! let mut doc = PdfDocument::open("scanned.pdf")?;
//! let engine = OcrEngine::new()?;
//!
//! // Check if page needs OCR
//! if ocr::needs_ocr(&doc, 0)? {
//!     let result = engine.ocr_page(&mut doc, 0)?;
//!     for span in result.spans {
//!         println!("{} at {:?}", span.text, span.bbox);
//!     }
//! }
//! ```

// Sub-modules
mod config;
mod detector;
mod engine;
mod error;
mod postprocessor;
mod preprocessor;
mod recognizer;

// Re-exports
pub use config::{OcrConfig, OcrConfigBuilder};
pub use detector::TextDetector;
pub use engine::{OcrEngine, OcrOutput, OcrSpan};
pub use error::OcrError;
pub use postprocessor::DetectedBox;
pub use preprocessor::{crop_text_region, preprocess_for_detection, preprocess_for_recognition};
pub use recognizer::{RecognitionResult, TextRecognizer};

// High-level OCR functions and types exported at module level:
// PageType, detect_page_type, needs_ocr, ocr_page, ocr_page_spans, extract_text_with_ocr

use crate::{PdfDocument, Result};

/// Check if a PDF page needs OCR (is a scanned page).
///
/// A page is considered "scanned" if:
/// 1. It has no native text (or very little)
/// 2. It contains images (typically a full-page scan)
///
/// # Arguments
///
/// * `doc` - The PDF document
/// * `page` - Page number (0-indexed)
///
/// # Returns
///
/// `true` if the page likely needs OCR, `false` otherwise.
///
/// # Example
///
/// ```ignore
/// use pdf_oxide::{PdfDocument, ocr};
///
/// let mut doc = PdfDocument::open("document.pdf")?;
/// if ocr::needs_ocr(&doc, 0)? {
///     println!("Page 0 is scanned, needs OCR");
/// }
/// ```
/// Result of scanned page detection with granular classification.
#[derive(Debug, Clone, PartialEq)]
pub enum PageType {
    /// Page has native text — no OCR needed.
    NativeText,
    /// Page is fully scanned (large image, no/minimal text) — OCR the whole page.
    ScannedPage,
    /// Page has some native text but also large images that may contain text.
    /// Hybrid merge should be used: native text + OCR for image regions.
    HybridPage,
}

/// Detect the type of a PDF page for OCR purposes.
///
/// Uses multiple heuristics:
/// 1. Native text length — substantial text means NativeText
/// 2. Image coverage — a single image covering >80% of the page area suggests a scan
/// 3. Text density — very sparse text with large images suggests HybridPage
/// 4. Replacement characters — high ratio of U+FFFD suggests garbled OCR layer
///
/// # Arguments
///
/// * `doc` - The PDF document
/// * `page` - Page number (0-indexed)
///
/// # Returns
///
/// The detected [`PageType`].
pub fn detect_page_type(doc: &mut PdfDocument, page: usize) -> Result<PageType> {
    let native_text = doc.extract_text(page).unwrap_or_default();
    let trimmed = native_text.trim();
    let text_len = trimmed.len();

    // Check for replacement characters (garbled OCR layer)
    let replacement_count = trimmed.chars().filter(|&c| c == '\u{FFFD}').count();
    let total_chars = trimmed.chars().count().max(1);
    let replacement_ratio = replacement_count as f32 / total_chars as f32;

    // If text has >20% replacement characters, it's garbled — treat as scanned
    let text_is_garbled = replacement_ratio > 0.20 && total_chars > 10;

    // Check for images
    let images = doc.extract_images(page)?;
    if images.is_empty() {
        // No images — return native text regardless of quality
        return Ok(PageType::NativeText);
    }

    // Calculate image coverage: does a single image cover most of the page?
    // Use standard US Letter page area as baseline (612 × 792 points)
    let page_area: f32 = 612.0 * 792.0;
    let largest_image_area = images
        .iter()
        .map(|img| (img.width() as f32) * (img.height() as f32))
        .fold(0.0_f32, f32::max);

    // Scale image area to PDF points (approximate: assume 72 DPI baseline)
    // Images are in pixels; a full A4 scan at 300 DPI ≈ 2480×3508 pixels
    // Page in points ≈ 612×792. Ratio: image_pixels / (page_points * dpi/72)
    let high_coverage = largest_image_area > page_area * 4.0; // ~72 DPI equivalent

    if text_len <= 50 || text_is_garbled {
        // No substantial (or garbled) text — classify based on images
        if high_coverage {
            Ok(PageType::ScannedPage)
        } else if !images.is_empty() {
            Ok(PageType::ScannedPage) // Small images but no text still needs OCR
        } else {
            Ok(PageType::NativeText)
        }
    } else if high_coverage && text_len < 500 {
        // Some text but a large image covers the page — hybrid
        Ok(PageType::HybridPage)
    } else {
        // Substantial text — native extraction is fine
        Ok(PageType::NativeText)
    }
}

/// Check if a PDF page needs OCR (is a scanned page).
///
/// This is a simplified wrapper around [`detect_page_type`] that returns
/// `true` for both `ScannedPage` and `HybridPage` types.
pub fn needs_ocr(doc: &mut PdfDocument, page: usize) -> Result<bool> {
    let page_type = detect_page_type(doc, page)?;
    Ok(matches!(page_type, PageType::ScannedPage | PageType::HybridPage))
}

/// OCR text extraction options.
#[derive(Debug, Clone)]
pub struct OcrExtractOptions {
    /// OCR configuration
    pub config: OcrConfig,
    /// Scale factor for coordinate conversion (image DPI / 72.0)
    /// Default: 300.0 / 72.0 ≈ 4.17 (assumes 300 DPI scan)
    pub scale: f32,
    /// Whether to fall back to native text if OCR fails
    pub fallback_to_native: bool,
}

impl Default for OcrExtractOptions {
    fn default() -> Self {
        Self {
            config: OcrConfig::default(),
            scale: 300.0 / 72.0, // Assume 300 DPI scanned document
            fallback_to_native: true,
        }
    }
}

impl OcrExtractOptions {
    /// Create options with a custom DPI.
    pub fn with_dpi(dpi: f32) -> Self {
        Self {
            scale: dpi / 72.0,
            ..Default::default()
        }
    }
}

/// OCR a single page of a PDF document.
///
/// This function:
/// 1. Extracts the largest image from the page (assumed to be the scan)
/// 2. Converts it to a DynamicImage
/// 3. Runs OCR on the image
/// 4. Returns the recognized text
///
/// # Arguments
///
/// * `doc` - The PDF document
/// * `page` - Page number (0-indexed)
/// * `engine` - The OCR engine to use
/// * `options` - OCR extraction options
///
/// # Returns
///
/// The recognized text from the page.
///
/// # Example
///
/// ```ignore
/// use pdf_oxide::{PdfDocument, ocr::{self, OcrEngine, OcrConfig}};
///
/// let mut doc = PdfDocument::open("scanned.pdf")?;
/// let engine = OcrEngine::new("det.onnx", "rec.onnx", "dict.txt", OcrConfig::default())?;
///
/// let text = ocr::ocr_page(&mut doc, 0, &engine, OcrExtractOptions::default())?;
/// println!("OCR text: {}", text);
/// ```
pub fn ocr_page(
    doc: &mut PdfDocument,
    page: usize,
    engine: &OcrEngine,
    options: &OcrExtractOptions,
) -> Result<String> {
    // Extract images from the page
    let images = doc.extract_images(page)?;

    if images.is_empty() {
        if options.fallback_to_native {
            return doc.extract_text(page);
        }
        return Ok(String::new());
    }

    // Find the largest image (assumed to be the page scan)
    let largest_image = images
        .iter()
        .max_by_key(|img| (img.width() as u64) * (img.height() as u64))
        .expect("images is non-empty");

    // Convert to DynamicImage
    let dynamic_image = largest_image.to_dynamic_image()?;

    // Run OCR
    let ocr_result = engine
        .ocr_image(&dynamic_image)
        .map_err(|e| crate::error::Error::Image(format!("OCR failed: {}", e)))?;

    // Return the text in reading order
    Ok(ocr_result.text_in_reading_order())
}

/// OCR a page and return TextSpans for layout integration.
///
/// This function is similar to `ocr_page` but returns structured TextSpans
/// that can be used with the existing layout analysis pipeline.
///
/// # Arguments
///
/// * `doc` - The PDF document
/// * `page` - Page number (0-indexed)
/// * `engine` - The OCR engine to use
/// * `options` - OCR extraction options
///
/// # Returns
///
/// Vector of TextSpans from the OCR result.
pub fn ocr_page_spans(
    doc: &mut PdfDocument,
    page: usize,
    engine: &OcrEngine,
    options: &OcrExtractOptions,
) -> Result<Vec<crate::layout::text_block::TextSpan>> {
    // Extract images from the page
    let images = doc.extract_images(page)?;

    if images.is_empty() {
        return Ok(Vec::new());
    }

    // Find the largest image (assumed to be the page scan)
    let largest_image = images
        .iter()
        .max_by_key(|img| (img.width() as u64) * (img.height() as u64))
        .expect("images is non-empty");

    // Convert to DynamicImage
    let dynamic_image = largest_image.to_dynamic_image()?;

    // Run OCR
    let ocr_result = engine
        .ocr_image(&dynamic_image)
        .map_err(|e| crate::error::Error::Image(format!("OCR failed: {}", e)))?;

    // Convert to TextSpans
    Ok(ocr_result.to_text_spans(options.scale))
}

/// Extract text from a page, automatically using OCR if needed.
///
/// This is the main entry point for text extraction that handles both
/// native PDF text and scanned pages transparently.
///
/// # Arguments
///
/// * `doc` - The PDF document
/// * `page` - Page number (0-indexed)
/// * `engine` - The OCR engine to use (optional, only needed for scanned pages)
/// * `options` - OCR extraction options
///
/// # Returns
///
/// The extracted text, either from native PDF text or OCR.
///
/// # Example
///
/// ```ignore
/// use pdf_oxide::{PdfDocument, ocr::{self, OcrEngine, OcrConfig, OcrExtractOptions}};
///
/// let mut doc = PdfDocument::open("mixed.pdf")?;
/// let engine = OcrEngine::new("det.onnx", "rec.onnx", "dict.txt", OcrConfig::default())?;
///
/// // Automatically uses native text or OCR as needed
/// let text = ocr::extract_text_with_ocr(&mut doc, 0, Some(&engine), OcrExtractOptions::default())?;
/// ```
pub fn extract_text_with_ocr(
    doc: &mut PdfDocument,
    page: usize,
    engine: Option<&OcrEngine>,
    options: OcrExtractOptions,
) -> Result<String> {
    let page_type = detect_page_type(doc, page)?;

    match page_type {
        PageType::NativeText => {
            // Native text is sufficient
            doc.extract_text(page)
        },
        PageType::ScannedPage => {
            // Full OCR needed
            if let Some(ocr_engine) = engine {
                match ocr_page(doc, page, ocr_engine, &options) {
                    Ok(ocr_text) => Ok(ocr_text),
                    Err(e) => {
                        log::warn!("OCR failed for scanned page {}: {}", page, e);
                        if options.fallback_to_native {
                            doc.extract_text(page)
                        } else {
                            Err(e)
                        }
                    },
                }
            } else {
                // No OCR engine, return whatever native text exists
                doc.extract_text(page)
            }
        },
        PageType::HybridPage => {
            // Has some native text and large images — merge both sources
            let native_text = doc.extract_text(page).unwrap_or_default();

            if let Some(ocr_engine) = engine {
                match ocr_page(doc, page, ocr_engine, &options) {
                    Ok(ocr_text) => {
                        // Hybrid merge: if OCR produced substantially more text,
                        // it likely captured content from images that native extraction missed.
                        // Use the longer result, preferring native when close.
                        let native_len = native_text.trim().len();
                        let ocr_len = ocr_text.trim().len();

                        if ocr_len > native_len * 2 {
                            // OCR found significantly more content — use it
                            log::debug!(
                                "Hybrid page {}: OCR ({} chars) >> native ({} chars), using OCR",
                                page, ocr_len, native_len
                            );
                            Ok(ocr_text)
                        } else {
                            // Native text is comparable or better — prefer it (higher quality)
                            log::debug!(
                                "Hybrid page {}: native ({} chars) >= OCR ({} chars), using native",
                                page, native_len, ocr_len
                            );
                            Ok(native_text)
                        }
                    },
                    Err(e) => {
                        log::warn!("OCR failed for hybrid page {}: {}, using native text", page, e);
                        Ok(native_text)
                    },
                }
            } else {
                Ok(native_text)
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ocr_module_compiles() {
        let _ = OcrConfig::default();
    }

    #[test]
    fn test_ocr_extract_options_default() {
        let options = OcrExtractOptions::default();
        assert!((options.scale - 300.0 / 72.0).abs() < 0.01);
        assert!(options.fallback_to_native);
    }

    #[test]
    fn test_ocr_extract_options_with_dpi() {
        let options = OcrExtractOptions::with_dpi(200.0);
        assert!((options.scale - 200.0 / 72.0).abs() < 0.01);
    }

    #[test]
    fn test_page_type_enum() {
        assert_eq!(PageType::NativeText, PageType::NativeText);
        assert_ne!(PageType::NativeText, PageType::ScannedPage);
        assert_ne!(PageType::ScannedPage, PageType::HybridPage);
    }
}
