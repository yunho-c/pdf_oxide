//! High-level document builder with fluent API.
//!
//! Provides a convenient interface for building PDF documents
//! using method chaining, wrapping the lower-level PdfWriter.
//!
//! # Annotations
//!
//! The fluent API supports adding annotations directly to text elements:
//!
//! ```ignore
//! use pdf_oxide::writer::{DocumentBuilder, PageSize};
//!
//! let mut builder = DocumentBuilder::new();
//! builder
//!     .page(PageSize::Letter)
//!     .at(72.0, 720.0)
//!     .text("Click here for more info")
//!     .link_url("https://example.com")  // Link the previous text
//!     .text("Important note")
//!     .highlight((1.0, 1.0, 0.0))       // Highlight in yellow
//!     .sticky_note("Review this section")
//!     .done();
//! ```

use super::annotation_builder::{Annotation, LinkAnnotation};
use super::font_manager::TextLayout;
use super::freetext::FreeTextAnnotation;
use super::pdf_writer::{PdfWriter, PdfWriterConfig};
use super::stamp::{StampAnnotation, StampType};
use super::text_annotations::TextAnnotation;
use super::text_markup::TextMarkupAnnotation;
use super::watermark::WatermarkAnnotation;
use crate::annotation_types::{TextAnnotationIcon, TextMarkupType};
use crate::elements::{ContentElement, TextContent};
use crate::error::Result;
use crate::geometry::Rect;
use std::path::Path;

/// Metadata for a PDF document.
#[derive(Debug, Clone, Default)]
pub struct DocumentMetadata {
    /// Document title
    pub title: Option<String>,
    /// Document author
    pub author: Option<String>,
    /// Document subject
    pub subject: Option<String>,
    /// Document keywords
    pub keywords: Option<String>,
    /// Creator application
    pub creator: Option<String>,
    /// PDF version (default: "1.7")
    pub version: Option<String>,
}

impl DocumentMetadata {
    /// Create new empty metadata.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set document title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set document author.
    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    /// Set document subject.
    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// Set document keywords.
    pub fn keywords(mut self, keywords: impl Into<String>) -> Self {
        self.keywords = Some(keywords.into());
        self
    }

    /// Set creator application.
    pub fn creator(mut self, creator: impl Into<String>) -> Self {
        self.creator = Some(creator.into());
        self
    }
}

/// Standard page sizes.
#[derive(Debug, Clone, Copy)]
pub enum PageSize {
    /// US Letter (8.5" x 11")
    Letter,
    /// A4 (210mm x 297mm)
    A4,
    /// Legal (8.5" x 14")
    Legal,
    /// A3 (297mm x 420mm)
    A3,
    /// Custom dimensions in points
    Custom(f32, f32),
}

impl PageSize {
    /// Get dimensions in points (1 inch = 72 points).
    pub fn dimensions(&self) -> (f32, f32) {
        match self {
            PageSize::Letter => (612.0, 792.0),
            PageSize::A4 => (595.0, 842.0),
            PageSize::Legal => (612.0, 1008.0),
            PageSize::A3 => (842.0, 1190.0),
            PageSize::Custom(w, h) => (*w, *h),
        }
    }
}

/// Text alignment options.
#[derive(Debug, Clone, Copy, Default)]
pub enum TextAlign {
    /// Left-aligned text (default)
    #[default]
    Left,
    /// Center-aligned text
    Center,
    /// Right-aligned text
    Right,
}

/// Configuration for text rendering.
#[derive(Debug, Clone)]
pub struct TextConfig {
    /// Font name (default: Helvetica)
    pub font: String,
    /// Font size in points (default: 12)
    pub size: f32,
    /// Text alignment
    pub align: TextAlign,
    /// Line height multiplier (default: 1.2)
    pub line_height: f32,
}

impl Default for TextConfig {
    fn default() -> Self {
        Self {
            font: "Helvetica".to_string(),
            size: 12.0,
            align: TextAlign::Left,
            line_height: 1.2,
        }
    }
}

/// Page builder for adding content to a page with fluent API.
pub struct FluentPageBuilder<'a> {
    builder: &'a mut DocumentBuilder,
    page_index: usize,
    cursor_x: f32,
    cursor_y: f32,
    text_config: TextConfig,
    text_layout: TextLayout,
    /// Track the last text element's bounding box for text markup annotations
    last_text_rect: Option<Rect>,
    /// Pending annotations for this page
    pending_annotations: Vec<Annotation>,
}

impl<'a> FluentPageBuilder<'a> {
    /// Set the text configuration for subsequent text operations.
    pub fn text_config(mut self, config: TextConfig) -> Self {
        self.text_config = config;
        self
    }

    /// Set font for subsequent text operations.
    pub fn font(mut self, name: &str, size: f32) -> Self {
        self.text_config.font = name.to_string();
        self.text_config.size = size;
        self
    }

    /// Set cursor position for text placement.
    pub fn at(mut self, x: f32, y: f32) -> Self {
        self.cursor_x = x;
        self.cursor_y = y;
        self
    }

    /// Add text at the current cursor position.
    pub fn text(mut self, text: &str) -> Self {
        let text_width = self.text_layout.font_manager().text_width(
            text,
            &self.text_config.font,
            self.text_config.size,
        );

        // Create the bounding box and track it for potential markup annotations
        let text_rect = Rect::new(self.cursor_x, self.cursor_y, text_width, self.text_config.size);
        self.last_text_rect = Some(text_rect);

        let page = &mut self.builder.pages[self.page_index];
        page.elements.push(ContentElement::Text(TextContent {
            text: text.to_string(),
            bbox: text_rect,
            font: crate::elements::FontSpec {
                name: self.text_config.font.clone(),
                size: self.text_config.size,
            },
            style: Default::default(),
            reading_order: Some(page.elements.len()),
            origin: None,
            rotation_degrees: None,
            matrix: None,
        }));
        // Move cursor down for next line
        self.cursor_y -= self.text_config.size * self.text_config.line_height;
        self
    }

    /// Add a heading (larger, bold text).
    pub fn heading(self, level: u8, text: &str) -> Self {
        let size = match level {
            1 => 24.0,
            2 => 20.0,
            3 => 16.0,
            _ => 14.0,
        };
        let font = match level {
            1 | 2 => "Helvetica-Bold",
            _ => "Helvetica",
        };
        self.font(font, size).text(text)
    }

    /// Add a paragraph of text with automatic word wrapping.
    pub fn paragraph(mut self, text: &str) -> Self {
        // Use FontManager-based word wrapping for accurate metrics
        let page = &mut self.builder.pages[self.page_index];
        let max_width = page.width - self.cursor_x - 72.0; // 72pt right margin

        let lines = self.text_layout.wrap_text(
            text,
            &self.text_config.font,
            self.text_config.size,
            max_width,
        );

        for (line_text, line_width) in lines {
            let page = &mut self.builder.pages[self.page_index];
            page.elements.push(ContentElement::Text(TextContent {
                text: line_text,
                bbox: Rect::new(self.cursor_x, self.cursor_y, line_width, self.text_config.size),
                font: crate::elements::FontSpec {
                    name: self.text_config.font.clone(),
                    size: self.text_config.size,
                },
                style: Default::default(),
                reading_order: Some(page.elements.len()),
                origin: None,
                rotation_degrees: None,
                matrix: None,
            }));
            self.cursor_y -= self.text_config.size * self.text_config.line_height;
        }
        // Add extra space after paragraph
        self.cursor_y -= self.text_config.size * 0.5;
        self
    }

    /// Add vertical space.
    pub fn space(mut self, points: f32) -> Self {
        self.cursor_y -= points;
        self
    }

    /// Add a horizontal line.
    pub fn horizontal_rule(mut self) -> Self {
        let page = &mut self.builder.pages[self.page_index];
        let line_y = self.cursor_y + self.text_config.size * 0.5;
        page.elements
            .push(ContentElement::Path(crate::elements::PathContent {
                operations: vec![
                    crate::elements::PathOperation::MoveTo(self.cursor_x, line_y),
                    crate::elements::PathOperation::LineTo(page.width - 72.0, line_y),
                ],
                bbox: Rect::new(self.cursor_x, line_y, page.width - 72.0 - self.cursor_x, 1.0),
                stroke_color: Some(crate::layout::Color {
                    r: 0.5,
                    g: 0.5,
                    b: 0.5,
                }),
                fill_color: None,
                stroke_width: 0.5,
                line_cap: crate::elements::LineCap::Butt,
                line_join: crate::elements::LineJoin::Miter,
                reading_order: None,
            }));
        self.cursor_y -= self.text_config.size;
        self
    }

    /// Add a content element directly.
    pub fn element(self, element: ContentElement) -> Self {
        let page = &mut self.builder.pages[self.page_index];
        page.elements.push(element);
        self
    }

    /// Add multiple content elements.
    pub fn elements(self, elements: Vec<ContentElement>) -> Self {
        let page = &mut self.builder.pages[self.page_index];
        page.elements.extend(elements);
        self
    }

    // ==========================================================================
    // Annotation Methods
    // ==========================================================================

    /// Add a URL link annotation to the last text element.
    ///
    /// The link will cover the bounding box of the most recently added text.
    ///
    /// # Example
    ///
    /// ```ignore
    /// builder.page(PageSize::Letter)
    ///     .at(72.0, 720.0)
    ///     .text("Visit our website")
    ///     .link_url("https://example.com")
    ///     .done();
    /// ```
    pub fn link_url(mut self, url: &str) -> Self {
        if let Some(rect) = self.last_text_rect {
            let link = LinkAnnotation::uri(rect, url);
            self.pending_annotations.push(link.into());
        }
        self
    }

    /// Add an internal page link annotation to the last text element.
    ///
    /// # Arguments
    ///
    /// * `page` - The target page index (0-based)
    ///
    /// # Example
    ///
    /// ```ignore
    /// builder.page(PageSize::Letter)
    ///     .at(72.0, 720.0)
    ///     .text("Go to page 5")
    ///     .link_page(4)  // 0-indexed
    ///     .done();
    /// ```
    pub fn link_page(mut self, page: usize) -> Self {
        if let Some(rect) = self.last_text_rect {
            let link = LinkAnnotation::goto_page(rect, page);
            self.pending_annotations.push(link.into());
        }
        self
    }

    /// Add a named destination link to the last text element.
    ///
    /// # Arguments
    ///
    /// * `destination` - The named destination string
    pub fn link_named(mut self, destination: &str) -> Self {
        if let Some(rect) = self.last_text_rect {
            let link = LinkAnnotation::goto_named(rect, destination);
            self.pending_annotations.push(link.into());
        }
        self
    }

    /// Add a highlight annotation to the last text element.
    ///
    /// # Arguments
    ///
    /// * `color` - RGB color tuple (0.0-1.0 for each component)
    ///
    /// # Example
    ///
    /// ```ignore
    /// builder.page(PageSize::Letter)
    ///     .at(72.0, 720.0)
    ///     .text("Important text")
    ///     .highlight((1.0, 1.0, 0.0))  // Yellow highlight
    ///     .done();
    /// ```
    pub fn highlight(mut self, color: (f32, f32, f32)) -> Self {
        if let Some(rect) = self.last_text_rect {
            let markup = TextMarkupAnnotation::from_rect(TextMarkupType::Highlight, rect)
                .with_color(color.0, color.1, color.2);
            self.pending_annotations.push(markup.into());
        }
        self
    }

    /// Add an underline annotation to the last text element.
    ///
    /// # Arguments
    ///
    /// * `color` - RGB color tuple (0.0-1.0 for each component)
    pub fn underline(mut self, color: (f32, f32, f32)) -> Self {
        if let Some(rect) = self.last_text_rect {
            let markup = TextMarkupAnnotation::from_rect(TextMarkupType::Underline, rect)
                .with_color(color.0, color.1, color.2);
            self.pending_annotations.push(markup.into());
        }
        self
    }

    /// Add a strikeout annotation to the last text element.
    ///
    /// # Arguments
    ///
    /// * `color` - RGB color tuple (0.0-1.0 for each component)
    pub fn strikeout(mut self, color: (f32, f32, f32)) -> Self {
        if let Some(rect) = self.last_text_rect {
            let markup = TextMarkupAnnotation::from_rect(TextMarkupType::StrikeOut, rect)
                .with_color(color.0, color.1, color.2);
            self.pending_annotations.push(markup.into());
        }
        self
    }

    /// Add a squiggly underline annotation to the last text element.
    ///
    /// # Arguments
    ///
    /// * `color` - RGB color tuple (0.0-1.0 for each component)
    pub fn squiggly(mut self, color: (f32, f32, f32)) -> Self {
        if let Some(rect) = self.last_text_rect {
            let markup = TextMarkupAnnotation::from_rect(TextMarkupType::Squiggly, rect)
                .with_color(color.0, color.1, color.2);
            self.pending_annotations.push(markup.into());
        }
        self
    }

    /// Add a sticky note annotation at the current cursor position.
    ///
    /// # Arguments
    ///
    /// * `text` - The note content
    ///
    /// # Example
    ///
    /// ```ignore
    /// builder.page(PageSize::Letter)
    ///     .at(72.0, 720.0)
    ///     .sticky_note("Please review this section")
    ///     .done();
    /// ```
    pub fn sticky_note(mut self, text: &str) -> Self {
        // Place sticky note at current cursor position (small 24x24 icon)
        let rect = Rect::new(self.cursor_x, self.cursor_y, 24.0, 24.0);
        let note = TextAnnotation::new(rect, text);
        self.pending_annotations.push(note.into());
        self
    }

    /// Add a sticky note annotation with a specific icon at the current cursor position.
    ///
    /// # Arguments
    ///
    /// * `text` - The note content
    /// * `icon` - The icon to display
    pub fn sticky_note_with_icon(mut self, text: &str, icon: TextAnnotationIcon) -> Self {
        let rect = Rect::new(self.cursor_x, self.cursor_y, 24.0, 24.0);
        let note = TextAnnotation::new(rect, text).with_icon(icon);
        self.pending_annotations.push(note.into());
        self
    }

    /// Add a sticky note annotation at a specific position.
    ///
    /// # Arguments
    ///
    /// * `x` - X coordinate
    /// * `y` - Y coordinate
    /// * `text` - The note content
    pub fn sticky_note_at(mut self, x: f32, y: f32, text: &str) -> Self {
        let rect = Rect::new(x, y, 24.0, 24.0);
        let note = TextAnnotation::new(rect, text);
        self.pending_annotations.push(note.into());
        self
    }

    /// Add a stamp annotation at the current cursor position.
    ///
    /// # Arguments
    ///
    /// * `stamp_type` - The type of stamp (Approved, Draft, Confidential, etc.)
    ///
    /// # Example
    ///
    /// ```ignore
    /// use pdf_oxide::writer::StampType;
    ///
    /// builder.page(PageSize::Letter)
    ///     .at(72.0, 720.0)
    ///     .stamp(StampType::Approved)
    ///     .done();
    /// ```
    pub fn stamp(mut self, stamp_type: StampType) -> Self {
        // Default stamp size: 150x50 points
        let rect = Rect::new(self.cursor_x, self.cursor_y, 150.0, 50.0);
        let stamp = StampAnnotation::new(rect, stamp_type);
        self.pending_annotations.push(stamp.into());
        self
    }

    /// Add a stamp annotation at a specific position with custom size.
    ///
    /// # Arguments
    ///
    /// * `rect` - The bounding rectangle for the stamp
    /// * `stamp_type` - The type of stamp
    pub fn stamp_at(mut self, rect: Rect, stamp_type: StampType) -> Self {
        let stamp = StampAnnotation::new(rect, stamp_type);
        self.pending_annotations.push(stamp.into());
        self
    }

    /// Add a FreeText annotation (text displayed directly on page).
    ///
    /// # Arguments
    ///
    /// * `rect` - The bounding rectangle for the text box
    /// * `text` - The text content
    pub fn freetext(mut self, rect: Rect, text: &str) -> Self {
        let freetext = FreeTextAnnotation::new(rect, text);
        self.pending_annotations.push(freetext.into());
        self
    }

    /// Add a FreeText annotation with custom font settings.
    ///
    /// # Arguments
    ///
    /// * `rect` - The bounding rectangle for the text box
    /// * `text` - The text content
    /// * `font` - Font name
    /// * `size` - Font size in points
    pub fn freetext_styled(mut self, rect: Rect, text: &str, font: &str, size: f32) -> Self {
        let freetext = FreeTextAnnotation::new(rect, text).with_font(font, size);
        self.pending_annotations.push(freetext.into());
        self
    }

    /// Add a watermark annotation (appears behind content, optionally print-only).
    ///
    /// # Arguments
    ///
    /// * `text` - The watermark text
    ///
    /// # Example
    ///
    /// ```ignore
    /// builder.page(PageSize::Letter)
    ///     .watermark("DRAFT")
    ///     .done();
    /// ```
    pub fn watermark(mut self, text: &str) -> Self {
        let page = &self.builder.pages[self.page_index];
        // Center the watermark on the page with diagonal orientation
        let rect =
            Rect::new(page.width * 0.1, page.height * 0.3, page.width * 0.8, page.height * 0.4);
        let watermark = WatermarkAnnotation::new(text)
            .with_rect(rect)
            .with_rotation(45.0)
            .with_opacity(0.3)
            .with_font("Helvetica", 72.0);
        self.pending_annotations.push(watermark.into());
        self
    }

    /// Add a "CONFIDENTIAL" watermark with preset styling.
    pub fn watermark_confidential(mut self) -> Self {
        let page = &self.builder.pages[self.page_index];
        let rect =
            Rect::new(page.width * 0.1, page.height * 0.3, page.width * 0.8, page.height * 0.4);
        let watermark = WatermarkAnnotation::confidential().with_rect(rect);
        self.pending_annotations.push(watermark.into());
        self
    }

    /// Add a "DRAFT" watermark with preset styling.
    pub fn watermark_draft(mut self) -> Self {
        let page = &self.builder.pages[self.page_index];
        let rect =
            Rect::new(page.width * 0.1, page.height * 0.3, page.width * 0.8, page.height * 0.4);
        let watermark = WatermarkAnnotation::draft().with_rect(rect);
        self.pending_annotations.push(watermark.into());
        self
    }

    /// Add a custom watermark with full control over positioning and styling.
    pub fn watermark_custom(mut self, watermark: WatermarkAnnotation) -> Self {
        self.pending_annotations.push(watermark.into());
        self
    }

    /// Add a generic annotation.
    ///
    /// This is a low-level method that allows adding any annotation type.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use pdf_oxide::writer::{LinkAnnotation, Annotation};
    /// use pdf_oxide::geometry::Rect;
    ///
    /// let link = LinkAnnotation::uri(
    ///     Rect::new(72.0, 720.0, 100.0, 12.0),
    ///     "https://example.com",
    /// );
    ///
    /// builder.page(PageSize::Letter)
    ///     .add_annotation(link)
    ///     .done();
    /// ```
    pub fn add_annotation<A: Into<Annotation>>(mut self, annotation: A) -> Self {
        self.pending_annotations.push(annotation.into());
        self
    }

    /// Finish building this page and return to the document builder.
    pub fn done(mut self) -> &'a mut DocumentBuilder {
        // Move pending annotations to page data
        let page = &mut self.builder.pages[self.page_index];
        page.annotations.append(&mut self.pending_annotations);
        self.builder
    }
}

/// Internal page data for DocumentBuilder.
struct PageData {
    width: f32,
    height: f32,
    elements: Vec<ContentElement>,
    annotations: Vec<Annotation>,
}

/// High-level document builder with fluent API.
///
/// Provides a convenient way to build PDF documents using method chaining.
///
/// # Example
///
/// ```ignore
/// use pdf_oxide::writer::{DocumentBuilder, PageSize, DocumentMetadata};
///
/// let pdf_bytes = DocumentBuilder::new()
///     .metadata(DocumentMetadata::new().title("My Document"))
///     .page(PageSize::Letter)
///         .at(72.0, 720.0)
///         .heading(1, "Hello, World!")
///         .paragraph("This is a simple PDF document.")
///         .done()
///     .build()?;
/// ```
pub struct DocumentBuilder {
    metadata: DocumentMetadata,
    pages: Vec<PageData>,
}

impl DocumentBuilder {
    /// Create a new document builder.
    pub fn new() -> Self {
        Self {
            metadata: DocumentMetadata::default(),
            pages: Vec::new(),
        }
    }

    /// Set document metadata.
    pub fn metadata(mut self, metadata: DocumentMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// Add a page with the specified size and return a page builder.
    pub fn page(&mut self, size: PageSize) -> FluentPageBuilder<'_> {
        let (width, height) = size.dimensions();
        let page_index = self.pages.len();
        self.pages.push(PageData {
            width,
            height,
            elements: Vec::new(),
            annotations: Vec::new(),
        });
        FluentPageBuilder {
            builder: self,
            page_index,
            cursor_x: 72.0,          // 1 inch margin
            cursor_y: height - 72.0, // Start from top with 1 inch margin
            text_config: TextConfig::default(),
            text_layout: TextLayout::new(),
            last_text_rect: None,
            pending_annotations: Vec::new(),
        }
    }

    /// Add a Letter-sized page.
    pub fn letter_page(&mut self) -> FluentPageBuilder<'_> {
        self.page(PageSize::Letter)
    }

    /// Add an A4-sized page.
    pub fn a4_page(&mut self) -> FluentPageBuilder<'_> {
        self.page(PageSize::A4)
    }

    /// Build the PDF document and return the bytes.
    pub fn build(self) -> Result<Vec<u8>> {
        let mut config = PdfWriterConfig::default();
        if let Some(version) = self.metadata.version {
            config.version = version;
        }
        config.title = self.metadata.title;
        config.author = self.metadata.author;
        config.subject = self.metadata.subject;
        config.keywords = self.metadata.keywords;
        if self.metadata.creator.is_some() {
            config.creator = self.metadata.creator;
        }

        let mut writer = PdfWriter::with_config(config);

        for page_data in &self.pages {
            let mut page = writer.add_page(page_data.width, page_data.height);
            page.add_elements(&page_data.elements);

            // Add annotations to the page
            for annotation in &page_data.annotations {
                page.add_annotation(annotation.clone());
            }

            page.finish();
        }

        writer.finish()
    }

    /// Build and save the PDF to a file.
    pub fn save(self, path: impl AsRef<Path>) -> Result<()> {
        let bytes = self.build()?;
        std::fs::write(path, bytes)?;
        Ok(())
    }
}

impl Default for DocumentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_size_dimensions() {
        assert_eq!(PageSize::Letter.dimensions(), (612.0, 792.0));
        assert_eq!(PageSize::A4.dimensions(), (595.0, 842.0));
        assert_eq!(PageSize::Legal.dimensions(), (612.0, 1008.0));
        assert_eq!(PageSize::Custom(100.0, 200.0).dimensions(), (100.0, 200.0));
    }

    #[test]
    fn test_document_metadata() {
        let meta = DocumentMetadata::new()
            .title("Test Title")
            .author("Test Author")
            .subject("Test Subject");

        assert_eq!(meta.title, Some("Test Title".to_string()));
        assert_eq!(meta.author, Some("Test Author".to_string()));
        assert_eq!(meta.subject, Some("Test Subject".to_string()));
    }

    #[test]
    fn test_document_builder_basic() {
        let mut builder = DocumentBuilder::new();
        builder
            .letter_page()
            .at(72.0, 720.0)
            .text("Hello, World!")
            .done();

        let bytes = builder.build().unwrap();
        let content = String::from_utf8_lossy(&bytes);

        assert!(content.starts_with("%PDF-1.7"));
        assert!(content.contains("%%EOF"));
    }

    #[test]
    fn test_document_builder_with_metadata() {
        let mut builder = DocumentBuilder::new().metadata(
            DocumentMetadata::new()
                .title("Test Document")
                .author("Test Author"),
        );

        builder.letter_page().text("Content").done();

        let bytes = builder.build().unwrap();
        let content = String::from_utf8_lossy(&bytes);

        assert!(content.contains("/Title (Test Document)"));
        assert!(content.contains("/Author (Test Author)"));
    }

    #[test]
    fn test_document_builder_multiple_pages() {
        let mut builder = DocumentBuilder::new();
        builder.letter_page().text("Page 1").done();
        builder.a4_page().text("Page 2").done();

        let bytes = builder.build().unwrap();
        let content = String::from_utf8_lossy(&bytes);

        assert!(content.contains("/Count 2"));
    }

    #[test]
    fn test_fluent_page_builder() {
        let mut builder = DocumentBuilder::new();
        builder
            .letter_page()
            .at(72.0, 720.0)
            .font("Helvetica-Bold", 18.0)
            .text("Title")
            .font("Helvetica", 12.0)
            .text("Body text")
            .space(12.0)
            .text("More text")
            .done();

        let bytes = builder.build().unwrap();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_text_config() {
        let config = TextConfig {
            font: "Times-Roman".to_string(),
            size: 14.0,
            align: TextAlign::Center,
            line_height: 1.5,
        };

        assert_eq!(config.font, "Times-Roman");
        assert_eq!(config.size, 14.0);
    }

    // ==========================================================================
    // Annotation Tests
    // ==========================================================================

    #[test]
    fn test_link_url_annotation() {
        let mut builder = DocumentBuilder::new();
        builder
            .letter_page()
            .at(72.0, 720.0)
            .text("Click here")
            .link_url("https://example.com")
            .done();

        let bytes = builder.build().unwrap();
        let content = String::from_utf8_lossy(&bytes);

        assert!(content.contains("/Subtype /Link"));
        assert!(content.contains("/S /URI"));
        assert!(content.contains("example.com"));
    }

    #[test]
    fn test_link_page_annotation() {
        let mut builder = DocumentBuilder::new();
        builder.letter_page().text("Page 1").done();
        builder
            .letter_page()
            .at(72.0, 720.0)
            .text("Go to page 1")
            .link_page(0)
            .done();

        let bytes = builder.build().unwrap();
        let content = String::from_utf8_lossy(&bytes);

        assert!(content.contains("/Subtype /Link"));
        assert!(content.contains("/Dest"));
    }

    #[test]
    fn test_highlight_annotation() {
        let mut builder = DocumentBuilder::new();
        builder
            .letter_page()
            .at(72.0, 720.0)
            .text("Important text")
            .highlight((1.0, 1.0, 0.0))
            .done();

        let bytes = builder.build().unwrap();
        let content = String::from_utf8_lossy(&bytes);

        assert!(content.contains("/Subtype /Highlight"));
        assert!(content.contains("/QuadPoints"));
    }

    #[test]
    fn test_underline_annotation() {
        let mut builder = DocumentBuilder::new();
        builder
            .letter_page()
            .at(72.0, 720.0)
            .text("Underlined text")
            .underline((1.0, 0.0, 0.0))
            .done();

        let bytes = builder.build().unwrap();
        let content = String::from_utf8_lossy(&bytes);

        assert!(content.contains("/Subtype /Underline"));
    }

    #[test]
    fn test_strikeout_annotation() {
        let mut builder = DocumentBuilder::new();
        builder
            .letter_page()
            .at(72.0, 720.0)
            .text("Deleted text")
            .strikeout((1.0, 0.0, 0.0))
            .done();

        let bytes = builder.build().unwrap();
        let content = String::from_utf8_lossy(&bytes);

        assert!(content.contains("/Subtype /StrikeOut"));
    }

    #[test]
    fn test_sticky_note_annotation() {
        let mut builder = DocumentBuilder::new();
        builder
            .letter_page()
            .at(72.0, 720.0)
            .sticky_note("This is a comment")
            .done();

        let bytes = builder.build().unwrap();
        let content = String::from_utf8_lossy(&bytes);

        assert!(content.contains("/Subtype /Text"));
        assert!(content.contains("This is a comment"));
    }

    #[test]
    fn test_stamp_annotation() {
        let mut builder = DocumentBuilder::new();
        builder
            .letter_page()
            .at(72.0, 720.0)
            .stamp(StampType::Approved)
            .done();

        let bytes = builder.build().unwrap();
        let content = String::from_utf8_lossy(&bytes);

        assert!(content.contains("/Subtype /Stamp"));
        assert!(content.contains("/Name /Approved"));
    }

    #[test]
    fn test_freetext_annotation() {
        let mut builder = DocumentBuilder::new();
        builder
            .letter_page()
            .freetext(Rect::new(100.0, 500.0, 200.0, 50.0), "Free text content")
            .done();

        let bytes = builder.build().unwrap();
        let content = String::from_utf8_lossy(&bytes);

        assert!(content.contains("/Subtype /FreeText"));
        assert!(content.contains("Free text content"));
    }

    #[test]
    fn test_watermark_annotation() {
        let mut builder = DocumentBuilder::new();
        builder.letter_page().watermark("DRAFT").done();

        let bytes = builder.build().unwrap();
        let content = String::from_utf8_lossy(&bytes);

        assert!(content.contains("/Subtype /Watermark"));
    }

    #[test]
    fn test_watermark_presets() {
        let mut builder = DocumentBuilder::new();
        builder.letter_page().watermark_confidential().done();

        let bytes = builder.build().unwrap();
        let content = String::from_utf8_lossy(&bytes);

        assert!(content.contains("/Subtype /Watermark"));
    }

    #[test]
    fn test_multiple_annotations() {
        let mut builder = DocumentBuilder::new();
        builder
            .letter_page()
            .at(72.0, 720.0)
            .text("Linked and highlighted text")
            .link_url("https://example.com")
            .highlight((1.0, 1.0, 0.0))
            .sticky_note("Review this")
            .done();

        let bytes = builder.build().unwrap();
        let content = String::from_utf8_lossy(&bytes);

        // Should have all three annotation types
        assert!(content.contains("/Subtype /Link"));
        assert!(content.contains("/Subtype /Highlight"));
        assert!(content.contains("/Subtype /Text"));
    }

    #[test]
    fn test_add_generic_annotation() {
        let mut builder = DocumentBuilder::new();
        let link =
            LinkAnnotation::uri(Rect::new(100.0, 700.0, 100.0, 20.0), "https://rust-lang.org");
        builder.letter_page().add_annotation(link).done();

        let bytes = builder.build().unwrap();
        let content = String::from_utf8_lossy(&bytes);

        assert!(content.contains("/Subtype /Link"));
        assert!(content.contains("rust-lang.org"));
    }

    #[test]
    fn test_no_annotation_when_no_text() {
        let mut builder = DocumentBuilder::new();
        // Try to add link without any text - should be a no-op
        builder
            .letter_page()
            .at(72.0, 720.0)
            .link_url("https://example.com") // No preceding text
            .done();

        let bytes = builder.build().unwrap();
        let content = String::from_utf8_lossy(&bytes);

        // Should NOT contain a link annotation since there was no text to link
        assert!(!content.contains("/Subtype /Link"));
    }
}
