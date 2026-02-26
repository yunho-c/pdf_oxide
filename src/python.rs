//! Python bindings via PyO3.
//!
//! This module provides Python bindings for the PDF library, exposing the core functionality
//! through a Python-friendly API with proper error handling and type hints.
//!
//! # Architecture
//!
//! - `PyPdfDocument`: Python wrapper around Rust `PdfDocument`
//! - Error mapping: Rust errors → Python exceptions
//! - Default arguments using `#[pyo3(signature = ...)]`
//!
//! # Example
//!
//! ```python
//! from pdf_oxide import PdfDocument
//!
//! doc = PdfDocument("document.pdf")
//! text = doc.extract_text(0)
//! markdown = doc.to_markdown(0, detect_headings=True)
//! ```

use pyo3::exceptions::{PyIOError, PyRuntimeError};
use pyo3::prelude::*;

use crate::converters::ConversionOptions as RustConversionOptions;
use crate::document::PdfDocument as RustPdfDocument;

/// Python wrapper for PdfDocument.
///
/// Provides PDF parsing, text extraction, and format conversion capabilities.
///
/// # Methods
///
/// - `__init__(path)`: Open a PDF file
/// - `version()`: Get PDF version tuple
/// - `page_count()`: Get number of pages
/// - `extract_text(page)`: Extract text from a page
/// - `to_markdown(page, ...)`: Convert page to Markdown
/// - `to_html(page, ...)`: Convert page to HTML
/// - `to_markdown_all(...)`: Convert all pages to Markdown
/// - `to_html_all(...)`: Convert all pages to HTML
use crate::editor::DocumentEditor as RustDocumentEditor;

#[pyclass(name = "PdfDocument", unsendable)]
pub struct PyPdfDocument {
    /// Inner Rust document
    inner: RustPdfDocument,
    /// Path for DOM access (lazy initialization)
    path: String,
    /// Cached editor for DOM access (lazy initialization)
    editor: Option<RustDocumentEditor>,
}

#[pymethods]
impl PyPdfDocument {
    /// Open a PDF file.
    ///
    /// Args:
    ///     path (str): Path to the PDF file
    ///
    /// Returns:
    ///     PdfDocument: Opened PDF document
    ///
    /// Raises:
    ///     IOError: If the file cannot be opened or is not a valid PDF
    ///
    /// Example:
    ///     >>> doc = PdfDocument("sample.pdf")
    ///     >>> print(doc.version())
    ///     (1, 7)
    #[new]
    fn new(path: String) -> PyResult<Self> {
        let doc = RustPdfDocument::open(&path)
            .map_err(|e| PyIOError::new_err(format!("Failed to open PDF: {}", e)))?;

        Ok(PyPdfDocument {
            inner: doc,
            path,
            editor: None,
        })
    }

    /// Get PDF version.
    ///
    /// Returns:
    ///     tuple[int, int]: PDF version as (major, minor), e.g. (1, 7) for PDF 1.7
    ///
    /// Example:
    ///     >>> doc = PdfDocument("sample.pdf")
    ///     >>> version = doc.version()
    ///     >>> print(f"PDF {version[0]}.{version[1]}")
    ///     PDF 1.7
    fn version(&self) -> (u8, u8) {
        self.inner.version()
    }

    /// Authenticate with a password to decrypt an encrypted PDF.
    ///
    /// If the PDF is encrypted, opening it automatically tries an empty password.
    /// Call this method to authenticate with a non-empty password.
    ///
    /// Args:
    ///     password (str): The password to authenticate with
    ///
    /// Returns:
    ///     bool: True if authentication succeeded, False if the password was wrong
    ///
    /// Raises:
    ///     RuntimeError: If encryption initialization fails
    ///
    /// Example:
    ///     >>> doc = PdfDocument("encrypted.pdf")
    ///     >>> doc.authenticate("secret123")
    ///     True
    fn authenticate(&mut self, password: &str) -> PyResult<bool> {
        self.inner
            .authenticate(password.as_bytes())
            .map_err(|e| PyRuntimeError::new_err(format!("Authentication failed: {}", e)))
    }

    /// Get number of pages in the document.
    ///
    /// Returns:
    ///     int: Number of pages
    ///
    /// Raises:
    ///     RuntimeError: If page count cannot be determined
    ///
    /// Example:
    ///     >>> doc = PdfDocument("sample.pdf")
    ///     >>> print(f"Pages: {doc.page_count()}")
    ///     Pages: 42
    fn page_count(&mut self) -> PyResult<usize> {
        self.inner
            .page_count()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to get page count: {}", e)))
    }

    /// Extract text from a page.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///
    /// Returns:
    ///     str: Extracted text from the page
    ///
    /// Raises:
    ///     RuntimeError: If text extraction fails
    ///
    /// Example:
    ///     >>> doc = PdfDocument("sample.pdf")
    ///     >>> text = doc.extract_text(0)
    ///     >>> print(text[:100])
    fn extract_text(&mut self, page: usize) -> PyResult<String> {
        self.inner
            .extract_text(page)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to extract text: {}", e)))
    }

    /// Extract individual characters from a page.
    ///
    /// This is a **low-level API** for character-level granularity. For most use cases,
    /// prefer `extract_text()` or `extract_spans()` which provide complete text strings.
    ///
    /// Characters are sorted in reading order (top-to-bottom, left-to-right) and
    /// overlapping characters (rendered multiple times for effects) are deduplicated.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///
    /// Returns:
    ///     list[TextChar]: Extracted characters with position, font, and style information
    ///
    /// Raises:
    ///     RuntimeError: If character extraction fails
    ///
    /// Example:
    ///     >>> doc = PdfDocument("sample.pdf")
    ///     >>> chars = doc.extract_chars(0)
    ///     >>> for ch in chars:
    ///     ...     print(f"'{ch.char}' at ({ch.bbox.x:.1f}, {ch.bbox.y:.1f})")
    fn extract_chars(&mut self, page: usize) -> PyResult<Vec<PyTextChar>> {
        self.inner
            .extract_chars(page)
            .map(|chars| {
                chars
                    .into_iter()
                    .map(|ch| PyTextChar { inner: ch })
                    .collect()
            })
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to extract characters: {}", e)))
    }

    /// Check if document has a structure tree (Tagged PDF).
    ///
    /// Tagged PDFs contain explicit document structure that defines reading order,
    /// semantic meaning, and accessibility information. This is the PDF-spec-compliant
    /// way to determine reading order.
    ///
    /// Returns:
    ///     bool: True if document has logical structure (Tagged PDF), False otherwise
    ///
    /// Example:
    ///     >>> doc = PdfDocument("sample.pdf")
    ///     >>> if doc.has_structure_tree():
    ///     ...     print("Tagged PDF with logical structure")
    ///     ... else:
    ///     ...     print("Untagged PDF - uses page content order")
    fn has_structure_tree(&mut self) -> bool {
        match self.inner.structure_tree() {
            Ok(Some(_)) => true,
            _ => false,
        }
    }

    /// Convert page to plain text.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///     preserve_layout (bool): Preserve visual layout (default: False) [currently unused]
    ///     detect_headings (bool): Detect headings (default: True) [currently unused]
    ///     include_images (bool): Include images (default: True) [currently unused]
    ///     image_output_dir (str | None): Directory for images (default: None) [currently unused]
    ///
    /// Returns:
    ///     str: Plain text from the page
    ///
    /// Raises:
    ///     RuntimeError: If conversion fails
    ///
    /// Example:
    ///     >>> doc = PdfDocument("paper.pdf")
    ///     >>> text = doc.to_plain_text(0)
    ///     >>> print(text[:100])
    ///
    /// Note:
    ///     Options parameters are accepted for API consistency but currently unused for plain text.
    #[pyo3(signature = (page, preserve_layout=false, detect_headings=true, include_images=true, image_output_dir=None))]
    fn to_plain_text(
        &mut self,
        page: usize,
        preserve_layout: bool,
        detect_headings: bool,
        include_images: bool,
        image_output_dir: Option<String>,
    ) -> PyResult<String> {
        let options = RustConversionOptions {
            preserve_layout,
            detect_headings,
            extract_tables: false,
            include_images,
            image_output_dir,
            ..Default::default()
        };

        self.inner
            .to_plain_text(page, &options)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to convert to plain text: {}", e)))
    }

    /// Convert all pages to plain text.
    ///
    /// Args:
    ///     preserve_layout (bool): Preserve visual layout (default: False) [currently unused]
    ///     detect_headings (bool): Detect headings (default: True) [currently unused]
    ///     include_images (bool): Include images (default: True) [currently unused]
    ///     image_output_dir (str | None): Directory for images (default: None) [currently unused]
    ///
    /// Returns:
    ///     str: Plain text from all pages separated by horizontal rules
    ///
    /// Raises:
    ///     RuntimeError: If conversion fails
    ///
    /// Example:
    ///     >>> doc = PdfDocument("book.pdf")
    ///     >>> text = doc.to_plain_text_all()
    ///     >>> with open("book.txt", "w") as f:
    ///     ...     f.write(text)
    ///
    /// Note:
    ///     Options parameters are accepted for API consistency but currently unused for plain text.
    #[pyo3(signature = (preserve_layout=false, detect_headings=true, include_images=true, image_output_dir=None))]
    fn to_plain_text_all(
        &mut self,
        preserve_layout: bool,
        detect_headings: bool,
        include_images: bool,
        image_output_dir: Option<String>,
    ) -> PyResult<String> {
        let options = RustConversionOptions {
            preserve_layout,
            detect_headings,
            extract_tables: false,
            include_images,
            image_output_dir,
            ..Default::default()
        };

        self.inner.to_plain_text_all(&options).map_err(|e| {
            PyRuntimeError::new_err(format!("Failed to convert all pages to plain text: {}", e))
        })
    }

    /// Convert page to Markdown.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///     preserve_layout (bool): Preserve visual layout (default: False)
    ///     detect_headings (bool): Detect headings based on font size (default: True)
    ///     include_images (bool): Include images in output (default: True)
    ///     image_output_dir (str | None): Directory to save images (default: None)
    ///
    /// Returns:
    ///     str: Markdown text
    ///
    /// Raises:
    ///     RuntimeError: If conversion fails
    ///
    /// Example:
    ///     >>> doc = PdfDocument("paper.pdf")
    ///     >>> markdown = doc.to_markdown(0, detect_headings=True)
    ///     >>> with open("output.md", "w") as f:
    ///     ...     f.write(markdown)
    #[pyo3(signature = (page, preserve_layout=false, detect_headings=true, include_images=true, image_output_dir=None, embed_images=true))]
    fn to_markdown(
        &mut self,
        page: usize,
        preserve_layout: bool,
        detect_headings: bool,
        include_images: bool,
        image_output_dir: Option<String>,
        embed_images: bool,
    ) -> PyResult<String> {
        let options = RustConversionOptions {
            preserve_layout,
            detect_headings,
            extract_tables: false,
            include_images,
            image_output_dir,
            embed_images,
            ..Default::default()
        };

        self.inner
            .to_markdown(page, &options)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to convert to Markdown: {}", e)))
    }

    /// Convert page to HTML.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///     preserve_layout (bool): Preserve visual layout with CSS positioning (default: False)
    ///     detect_headings (bool): Detect headings based on font size (default: True)
    ///     include_images (bool): Include images in output (default: True)
    ///     image_output_dir (str | None): Directory to save images (default: None)
    ///
    /// Returns:
    ///     str: HTML text
    ///
    /// Raises:
    ///     RuntimeError: If conversion fails
    ///
    /// Example:
    ///     >>> doc = PdfDocument("paper.pdf")
    ///     >>> html = doc.to_html(0, preserve_layout=False)
    ///     >>> with open("output.html", "w") as f:
    ///     ...     f.write(html)
    #[pyo3(signature = (page, preserve_layout=false, detect_headings=true, include_images=true, image_output_dir=None, embed_images=true))]
    fn to_html(
        &mut self,
        page: usize,
        preserve_layout: bool,
        detect_headings: bool,
        include_images: bool,
        image_output_dir: Option<String>,
        embed_images: bool,
    ) -> PyResult<String> {
        let options = RustConversionOptions {
            preserve_layout,
            detect_headings,
            extract_tables: false,
            include_images,
            image_output_dir,
            embed_images,
            ..Default::default()
        };

        self.inner
            .to_html(page, &options)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to convert to HTML: {}", e)))
    }

    /// Convert all pages to Markdown.
    ///
    /// Args:
    ///     preserve_layout (bool): Preserve visual layout (default: False)
    ///     detect_headings (bool): Detect headings based on font size (default: True)
    ///     include_images (bool): Include images in output (default: True)
    ///     image_output_dir (str | None): Directory to save images (default: None)
    ///
    /// Returns:
    ///     str: Markdown text with all pages separated by horizontal rules
    ///
    /// Raises:
    ///     RuntimeError: If conversion fails
    ///
    /// Example:
    ///     >>> doc = PdfDocument("book.pdf")
    ///     >>> markdown = doc.to_markdown_all(detect_headings=True)
    ///     >>> with open("book.md", "w") as f:
    ///     ...     f.write(markdown)
    #[pyo3(signature = (preserve_layout=false, detect_headings=true, include_images=true, image_output_dir=None, embed_images=true))]
    fn to_markdown_all(
        &mut self,
        preserve_layout: bool,
        detect_headings: bool,
        include_images: bool,
        image_output_dir: Option<String>,
        embed_images: bool,
    ) -> PyResult<String> {
        let options = RustConversionOptions {
            preserve_layout,
            detect_headings,
            extract_tables: false,
            include_images,
            image_output_dir,
            embed_images,
            ..Default::default()
        };

        self.inner.to_markdown_all(&options).map_err(|e| {
            PyRuntimeError::new_err(format!("Failed to convert all pages to Markdown: {}", e))
        })
    }

    /// Convert all pages to HTML.
    ///
    /// Args:
    ///     preserve_layout (bool): Preserve visual layout with CSS positioning (default: False)
    ///     detect_headings (bool): Detect headings based on font size (default: True)
    ///     include_images (bool): Include images in output (default: True)
    ///     image_output_dir (str | None): Directory to save images (default: None)
    ///
    /// Returns:
    ///     str: HTML text with all pages wrapped in div.page elements
    ///
    /// Raises:
    ///     RuntimeError: If conversion fails
    ///
    /// Example:
    ///     >>> doc = PdfDocument("book.pdf")
    ///     >>> html = doc.to_html_all(preserve_layout=True)
    ///     >>> with open("book.html", "w") as f:
    ///     ...     f.write(html)
    #[pyo3(signature = (preserve_layout=false, detect_headings=true, include_images=true, image_output_dir=None, embed_images=true))]
    fn to_html_all(
        &mut self,
        preserve_layout: bool,
        detect_headings: bool,
        include_images: bool,
        image_output_dir: Option<String>,
        embed_images: bool,
    ) -> PyResult<String> {
        let options = RustConversionOptions {
            preserve_layout,
            detect_headings,
            extract_tables: false,
            include_images,
            image_output_dir,
            embed_images,
            ..Default::default()
        };

        self.inner.to_html_all(&options).map_err(|e| {
            PyRuntimeError::new_err(format!("Failed to convert all pages to HTML: {}", e))
        })
    }

    /// Get a page for DOM-like navigation and editing.
    ///
    /// Returns a `PdfPage` object that provides hierarchical access to page content,
    /// allowing you to query, navigate, and modify elements.
    ///
    /// Args:
    ///     index (int): Page index (0-based)
    ///
    /// Returns:
    ///     PdfPage: Page object with DOM access
    ///
    /// Raises:
    ///     RuntimeError: If page access fails
    ///
    /// Example:
    ///     >>> doc = PdfDocument("sample.pdf")
    ///     >>> page = doc.page(0)
    ///     >>> for text in page.find_text_containing("Hello"):
    ///     ...     print(f"{text.value} at {text.bbox}")
    fn page(&mut self, index: usize) -> PyResult<PyPdfPage> {
        // Lazy-initialize editor if needed
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }

        let editor = self.editor.as_mut().expect("editor initialized above");
        let page = editor
            .get_page(index)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to get page: {}", e)))?;

        Ok(PyPdfPage { inner: page })
    }

    /// Save modifications made via page().set_text() back to a file.
    ///
    /// Args:
    ///     path (str): Output file path
    ///     page (PdfPage): The modified page to save
    ///
    /// Raises:
    ///     RuntimeError: If save fails
    ///
    /// Example:
    ///     >>> doc = PdfDocument("input.pdf")
    ///     >>> page = doc.page(0)
    ///     >>> for t in page.find_text_containing("old"):
    ///     ...     page.set_text(t.id, "new")
    ///     >>> doc.save_page(page)
    ///     >>> doc.save("output.pdf")
    fn save_page(&mut self, page: &PyPdfPage) -> PyResult<()> {
        if self.editor.is_none() {
            return Err(PyRuntimeError::new_err("No editor initialized. Call page() first."));
        }

        let editor = self.editor.as_mut().expect("editor initialized above");
        editor
            .save_page(page.inner.clone())
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to save page: {}", e)))
    }

    /// Save the document to a file.
    ///
    /// This saves any modifications made via page().set_text().
    ///
    /// Args:
    ///     path (str): Output file path
    ///
    /// Raises:
    ///     IOError: If save fails
    ///
    /// Example:
    ///     >>> doc = PdfDocument("input.pdf")
    ///     >>> page = doc.page(0)
    ///     >>> page.set_text(text_id, "new text")
    ///     >>> doc.save_page(page)
    ///     >>> doc.save("output.pdf")
    fn save(&mut self, path: &str) -> PyResult<()> {
        use crate::editor::EditableDocument;

        if let Some(ref mut editor) = self.editor {
            editor
                .save(path)
                .map_err(|e| PyIOError::new_err(format!("Failed to save PDF: {}", e)))
        } else {
            Err(PyRuntimeError::new_err(
                "No modifications to save. Use page() and set_text() first.",
            ))
        }
    }

    /// Save the document with password encryption.
    ///
    /// Creates a password-protected PDF using AES-256 encryption (the strongest available).
    ///
    /// Args:
    ///     path (str): Output file path
    ///     user_password (str): Password required to open the document (can be empty string
    ///         for no open password, but still apply owner restrictions)
    ///     owner_password (str): Password for full access and changing security settings.
    ///         If empty, defaults to user_password.
    ///     allow_print (bool): Allow printing (default: True)
    ///     allow_copy (bool): Allow copying text and graphics (default: True)
    ///     allow_modify (bool): Allow modifying the document (default: True)
    ///     allow_annotate (bool): Allow adding annotations (default: True)
    ///
    /// Raises:
    ///     RuntimeError: If no modifications have been made
    ///     IOError: If save fails
    ///
    /// Example:
    /// ```text
    /// >>> doc = PdfDocument("input.pdf")
    /// >>> page = doc.page(0)
    /// >>> page.set_text(text_id, "modified")
    /// >>> doc.save_page(page)
    /// >>> doc.save_encrypted("protected.pdf", "user123", "owner456")
    ///
    /// >>> # View-only PDF (no printing, copying, or modifying):
    /// >>> doc.save_encrypted("readonly.pdf", "", "owner456",
    /// ...     allow_print=False, allow_copy=False, allow_modify=False)
    /// ```
    #[pyo3(signature = (path, user_password, owner_password=None, allow_print=true, allow_copy=true, allow_modify=true, allow_annotate=true))]
    fn save_encrypted(
        &mut self,
        path: &str,
        user_password: &str,
        owner_password: Option<&str>,
        allow_print: bool,
        allow_copy: bool,
        allow_modify: bool,
        allow_annotate: bool,
    ) -> PyResult<()> {
        use crate::editor::{
            EditableDocument, EncryptionAlgorithm, EncryptionConfig, Permissions, SaveOptions,
        };

        if let Some(ref mut editor) = self.editor {
            let owner_pwd = owner_password.unwrap_or(user_password);

            let permissions = Permissions {
                print: allow_print,
                print_high_quality: allow_print,
                modify: allow_modify,
                copy: allow_copy,
                annotate: allow_annotate,
                fill_forms: allow_annotate,
                accessibility: true, // Always allow for compliance
                assemble: allow_modify,
            };

            let config = EncryptionConfig::new(user_password, owner_pwd)
                .with_algorithm(EncryptionAlgorithm::Aes256)
                .with_permissions(permissions);

            let options = SaveOptions::with_encryption(config);
            editor
                .save_with_options(path, options)
                .map_err(|e| PyIOError::new_err(format!("Failed to save encrypted PDF: {}", e)))
        } else {
            Err(PyRuntimeError::new_err(
                "No modifications to save. Use page() and set_text() first.",
            ))
        }
    }

    // === Document Metadata ===

    /// Set the document title.
    ///
    /// Args:
    ///     title (str): Document title
    ///
    /// Example:
    ///     >>> doc.set_title("My Document")
    fn set_title(&mut self, title: &str) -> PyResult<()> {
        // Lazy-initialize editor if needed
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            editor.set_title(title);
        }
        Ok(())
    }

    /// Set the document author.
    ///
    /// Args:
    ///     author (str): Author name
    fn set_author(&mut self, author: &str) -> PyResult<()> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            editor.set_author(author);
        }
        Ok(())
    }

    /// Set the document subject.
    ///
    /// Args:
    ///     subject (str): Document subject
    fn set_subject(&mut self, subject: &str) -> PyResult<()> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            editor.set_subject(subject);
        }
        Ok(())
    }

    /// Set the document keywords.
    ///
    /// Args:
    ///     keywords (str): Comma-separated keywords
    fn set_keywords(&mut self, keywords: &str) -> PyResult<()> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            editor.set_keywords(keywords);
        }
        Ok(())
    }

    // =========================================================================
    // Page Properties: Rotation, Cropping
    // =========================================================================

    /// Get the rotation of a page in degrees (0, 90, 180, 270).
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///
    /// Returns:
    ///     int: Rotation in degrees
    ///
    /// Example:
    ///     >>> rotation = doc.page_rotation(0)
    ///     >>> print(f"Page is rotated {rotation} degrees")
    fn page_rotation(&mut self, page: usize) -> PyResult<i32> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            editor
                .get_page_rotation(page)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get rotation: {}", e)))
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    /// Set the rotation of a page.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///     degrees (int): Rotation in degrees (0, 90, 180, or 270)
    ///
    /// Example:
    ///     >>> doc.set_page_rotation(0, 90)
    ///     >>> doc.save("rotated.pdf")
    fn set_page_rotation(&mut self, page: usize, degrees: i32) -> PyResult<()> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            editor
                .set_page_rotation(page, degrees)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to set rotation: {}", e)))
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    /// Rotate a page by the given degrees (adds to current rotation).
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///     degrees (int): Degrees to rotate (will be normalized to 0, 90, 180, 270)
    ///
    /// Example:
    ///     >>> doc.rotate_page(0, 90)  # Rotate 90 degrees clockwise
    ///     >>> doc.save("rotated.pdf")
    fn rotate_page(&mut self, page: usize, degrees: i32) -> PyResult<()> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            editor
                .rotate_page_by(page, degrees)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to rotate page: {}", e)))
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    /// Rotate all pages by the given degrees.
    ///
    /// Args:
    ///     degrees (int): Degrees to rotate (will be normalized to 0, 90, 180, 270)
    ///
    /// Example:
    ///     >>> doc.rotate_all_pages(180)  # Flip all pages upside down
    ///     >>> doc.save("rotated.pdf")
    fn rotate_all_pages(&mut self, degrees: i32) -> PyResult<()> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            editor
                .rotate_all_pages(degrees)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to rotate pages: {}", e)))
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    /// Get the MediaBox of a page (physical page size).
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///
    /// Returns:
    ///     tuple[float, float, float, float]: (llx, lly, urx, ury) coordinates
    ///
    /// Example:
    ///     >>> llx, lly, urx, ury = doc.page_media_box(0)
    ///     >>> print(f"Page size: {urx - llx} x {ury - lly}")
    fn page_media_box(&mut self, page: usize) -> PyResult<(f32, f32, f32, f32)> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            let box_ = editor
                .get_page_media_box(page)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get MediaBox: {}", e)))?;
            Ok((box_[0], box_[1], box_[2], box_[3]))
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    /// Set the MediaBox of a page (physical page size).
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///     llx (float): Lower-left X coordinate
    ///     lly (float): Lower-left Y coordinate
    ///     urx (float): Upper-right X coordinate
    ///     ury (float): Upper-right Y coordinate
    fn set_page_media_box(
        &mut self,
        page: usize,
        llx: f32,
        lly: f32,
        urx: f32,
        ury: f32,
    ) -> PyResult<()> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            editor
                .set_page_media_box(page, [llx, lly, urx, ury])
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to set MediaBox: {}", e)))
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    /// Get the CropBox of a page (visible/printable area).
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///
    /// Returns:
    ///     tuple[float, float, float, float] | None: (llx, lly, urx, ury) or None if not set
    fn page_crop_box(&mut self, page: usize) -> PyResult<Option<(f32, f32, f32, f32)>> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            let box_ = editor
                .get_page_crop_box(page)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to get CropBox: {}", e)))?;
            Ok(box_.map(|b| (b[0], b[1], b[2], b[3])))
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    /// Set the CropBox of a page (visible/printable area).
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///     llx (float): Lower-left X coordinate
    ///     lly (float): Lower-left Y coordinate
    ///     urx (float): Upper-right X coordinate
    ///     ury (float): Upper-right Y coordinate
    ///
    /// Example:
    /// ```text
    /// >>> # Crop to a 6x9 inch area (72 points = 1 inch)
    /// >>> doc.set_page_crop_box(0, 72, 72, 504, 720)
    /// >>> doc.save("cropped.pdf")
    /// ```
    fn set_page_crop_box(
        &mut self,
        page: usize,
        llx: f32,
        lly: f32,
        urx: f32,
        ury: f32,
    ) -> PyResult<()> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            editor
                .set_page_crop_box(page, [llx, lly, urx, ury])
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to set CropBox: {}", e)))
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    /// Crop margins from all pages.
    ///
    /// Sets the CropBox to be smaller than the MediaBox by the specified margins.
    ///
    /// Args:
    ///     left (float): Left margin in points
    ///     right (float): Right margin in points
    ///     top (float): Top margin in points
    ///     bottom (float): Bottom margin in points
    ///
    /// Example:
    /// ```text
    /// >>> # Crop 0.5 inch from all sides (72 points = 1 inch)
    /// >>> doc.crop_margins(36, 36, 36, 36)
    /// >>> doc.save("cropped.pdf")
    /// ```
    fn crop_margins(&mut self, left: f32, right: f32, top: f32, bottom: f32) -> PyResult<()> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            editor
                .crop_margins(left, right, top, bottom)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to crop margins: {}", e)))
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    // =========================================================================
    // Content Erasing (Whiteout)
    // =========================================================================

    /// Erase a rectangular region on a page by covering it with white.
    ///
    /// This adds a white rectangle overlay that covers the specified region.
    /// The original content is not removed but hidden beneath the white overlay.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///     llx (float): Lower-left X coordinate
    ///     lly (float): Lower-left Y coordinate
    ///     urx (float): Upper-right X coordinate
    ///     ury (float): Upper-right Y coordinate
    ///
    /// Example:
    /// ```text
    /// >>> # Erase a region in the upper-left corner
    /// >>> doc.erase_region(0, 72, 700, 200, 792)
    /// >>> doc.save("output.pdf")
    /// ```
    fn erase_region(
        &mut self,
        page: usize,
        llx: f32,
        lly: f32,
        urx: f32,
        ury: f32,
    ) -> PyResult<()> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            editor
                .erase_region(page, [llx, lly, urx, ury])
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to erase region: {}", e)))
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    /// Erase multiple rectangular regions on a page.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///     rects (list[tuple[float, float, float, float]]): List of (llx, lly, urx, ury) tuples
    ///
    /// Example:
    ///     >>> doc.erase_regions(0, [(72, 700, 200, 792), (300, 300, 500, 400)])
    ///     >>> doc.save("output.pdf")
    fn erase_regions(&mut self, page: usize, rects: Vec<(f32, f32, f32, f32)>) -> PyResult<()> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            let rect_arrays: Vec<[f32; 4]> = rects
                .iter()
                .map(|(llx, lly, urx, ury)| [*llx, *lly, *urx, *ury])
                .collect();
            editor
                .erase_regions(page, &rect_arrays)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to erase regions: {}", e)))
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    /// Clear all pending erase operations for a page.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    fn clear_erase_regions(&mut self, page: usize) {
        if let Some(ref mut editor) = self.editor {
            editor.clear_erase_regions(page);
        }
    }

    // ========================================================================
    // Annotation Flattening
    // ========================================================================

    /// Flatten annotations on a specific page.
    ///
    /// Renders annotation appearance streams into the page content and removes
    /// the annotations. This makes annotations permanent and non-editable.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///
    /// Raises:
    ///     RuntimeError: If page index is out of range
    ///
    /// Example:
    ///     >>> doc.flatten_page_annotations(0)  # Flatten page 0
    ///     >>> doc.save("flattened.pdf")
    fn flatten_page_annotations(&mut self, page: usize) -> PyResult<()> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            editor.flatten_page_annotations(page).map_err(|e| {
                PyRuntimeError::new_err(format!("Failed to flatten annotations: {}", e))
            })
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    /// Flatten annotations on all pages.
    ///
    /// Renders all annotation appearance streams into page content and removes
    /// all annotations from the document.
    ///
    /// Raises:
    ///     RuntimeError: If the operation fails
    ///
    /// Example:
    ///     >>> doc.flatten_all_annotations()
    ///     >>> doc.save("flattened.pdf")
    fn flatten_all_annotations(&mut self) -> PyResult<()> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            editor.flatten_all_annotations().map_err(|e| {
                PyRuntimeError::new_err(format!("Failed to flatten annotations: {}", e))
            })
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    /// Check if a page is marked for annotation flattening.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///
    /// Returns:
    ///     bool: True if the page is marked for flattening
    fn is_page_marked_for_flatten(&self, page: usize) -> bool {
        if let Some(ref editor) = self.editor {
            editor.is_page_marked_for_flatten(page)
        } else {
            false
        }
    }

    /// Unmark a page for annotation flattening.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    fn unmark_page_for_flatten(&mut self, page: usize) {
        if let Some(ref mut editor) = self.editor {
            editor.unmark_page_for_flatten(page);
        }
    }

    // ========================================================================
    // Redaction Application
    // ========================================================================

    /// Apply redactions on a specific page.
    ///
    /// Finds all redaction annotations on the page, draws colored overlays
    /// to hide the content, and removes the redaction annotations.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///
    /// Note:
    ///     This creates visual overlays but does not remove underlying content.
    ///
    /// Raises:
    ///     RuntimeError: If page index is out of range
    ///
    /// Example:
    ///     >>> doc.apply_page_redactions(0)
    ///     >>> doc.save("redacted.pdf")
    fn apply_page_redactions(&mut self, page: usize) -> PyResult<()> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            editor
                .apply_page_redactions(page)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to apply redactions: {}", e)))
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    /// Apply redactions on all pages.
    ///
    /// Finds all redaction annotations throughout the document, draws
    /// colored overlays to hide content, and removes the redaction annotations.
    ///
    /// Raises:
    ///     RuntimeError: If the operation fails
    ///
    /// Example:
    ///     >>> doc.apply_all_redactions()
    ///     >>> doc.save("redacted.pdf")
    fn apply_all_redactions(&mut self) -> PyResult<()> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            editor
                .apply_all_redactions()
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to apply redactions: {}", e)))
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    /// Check if a page is marked for redaction application.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///
    /// Returns:
    ///     bool: True if the page is marked for redaction application
    fn is_page_marked_for_redaction(&self, page: usize) -> bool {
        if let Some(ref editor) = self.editor {
            editor.is_page_marked_for_redaction(page)
        } else {
            false
        }
    }

    /// Unmark a page for redaction application.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    fn unmark_page_for_redaction(&mut self, page: usize) {
        if let Some(ref mut editor) = self.editor {
            editor.unmark_page_for_redaction(page);
        }
    }

    // ===== Image Repositioning & Resizing =====

    /// Get information about all images on a page.
    ///
    /// Returns a list of dictionaries with image information including
    /// name, position, size, and transformation matrix.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///
    /// Returns:
    ///     list[dict]: List of image info dictionaries with keys:
    ///         - name (str): XObject name (e.g., "Im0")
    ///         - x (float): X position
    ///         - y (float): Y position
    ///         - width (float): Image width
    ///         - height (float): Image height
    ///         - matrix (tuple): 6-element transformation matrix (a, b, c, d, e, f)
    fn page_images(&mut self, page: usize, py: Python<'_>) -> PyResult<Py<PyAny>> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            let images = editor.get_page_images(page).map_err(|e| {
                PyRuntimeError::new_err(format!("Failed to get page images: {}", e))
            })?;

            let result = pyo3::types::PyList::empty(py);
            for img in images {
                let dict = pyo3::types::PyDict::new(py);
                dict.set_item("name", &img.name)?;
                dict.set_item("x", img.bounds[0])?;
                dict.set_item("y", img.bounds[1])?;
                dict.set_item("width", img.bounds[2])?;
                dict.set_item("height", img.bounds[3])?;
                dict.set_item(
                    "matrix",
                    (
                        img.matrix[0],
                        img.matrix[1],
                        img.matrix[2],
                        img.matrix[3],
                        img.matrix[4],
                        img.matrix[5],
                    ),
                )?;
                result.append(dict)?;
            }
            Ok(result.into())
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    /// Reposition an image on a page.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///     image_name (str): Name of the image XObject (e.g., "Im0")
    ///     x (float): New X position
    ///     y (float): New Y position
    ///
    /// Raises:
    ///     RuntimeError: If the image is not found or operation fails
    fn reposition_image(&mut self, page: usize, image_name: &str, x: f32, y: f32) -> PyResult<()> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            editor
                .reposition_image(page, image_name, x, y)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to reposition image: {}", e)))
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    /// Resize an image on a page.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///     image_name (str): Name of the image XObject (e.g., "Im0")
    ///     width (float): New width
    ///     height (float): New height
    ///
    /// Raises:
    ///     RuntimeError: If the image is not found or operation fails
    fn resize_image(
        &mut self,
        page: usize,
        image_name: &str,
        width: f32,
        height: f32,
    ) -> PyResult<()> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            editor
                .resize_image(page, image_name, width, height)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to resize image: {}", e)))
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    /// Set both position and size of an image on a page.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///     image_name (str): Name of the image XObject (e.g., "Im0")
    ///     x (float): New X position
    ///     y (float): New Y position
    ///     width (float): New width
    ///     height (float): New height
    ///
    /// Raises:
    ///     RuntimeError: If the image is not found or operation fails
    fn set_image_bounds(
        &mut self,
        page: usize,
        image_name: &str,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> PyResult<()> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        if let Some(ref mut editor) = self.editor {
            editor
                .set_image_bounds(page, image_name, x, y, width, height)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to set image bounds: {}", e)))
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    /// Clear all image modifications for a specific page.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    fn clear_image_modifications(&mut self, page: usize) {
        if let Some(ref mut editor) = self.editor {
            editor.clear_image_modifications(page);
        }
    }

    /// Check if a page has pending image modifications.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///
    /// Returns:
    ///     bool: True if the page has pending image modifications
    fn has_image_modifications(&self, page: usize) -> bool {
        if let Some(ref editor) = self.editor {
            editor.has_image_modifications(page)
        } else {
            false
        }
    }

    // ========================================================================
    // Text Search
    // ========================================================================

    /// Search for text in the document.
    ///
    /// Searches all pages for matches of the given pattern (regex supported).
    ///
    /// Args:
    ///     pattern (str): Search pattern (regex or literal text)
    ///     case_insensitive (bool): Case insensitive search (default: False)
    ///     literal (bool): Treat pattern as literal text, not regex (default: False)
    ///     whole_word (bool): Match whole words only (default: False)
    ///     max_results (int): Maximum number of results, 0 = unlimited (default: 0)
    ///
    /// Returns:
    ///     list[dict]: List of search results, each containing:
    ///         - page (int): Page number (0-indexed)
    ///         - text (str): Matched text
    ///         - x (float): X position of match
    ///         - y (float): Y position of match
    ///         - width (float): Width of match bounding box
    ///         - height (float): Height of match bounding box
    ///
    /// Example:
    /// ```text
    /// >>> results = doc.search("hello")
    /// >>> for r in results:
    /// ...     print(f"Found '{r['text']}' on page {r['page']}")
    ///
    /// >>> # Case insensitive regex search
    /// >>> results = doc.search(r"\\d+\\.\\d+", case_insensitive=True)
    /// ```
    #[pyo3(signature = (pattern, case_insensitive=false, literal=false, whole_word=false, max_results=0))]
    fn search(
        &mut self,
        py: Python<'_>,
        pattern: &str,
        case_insensitive: bool,
        literal: bool,
        whole_word: bool,
        max_results: usize,
    ) -> PyResult<Py<PyAny>> {
        use crate::search::{SearchOptions, TextSearcher};

        let options = SearchOptions::new()
            .with_case_insensitive(case_insensitive)
            .with_literal(literal)
            .with_whole_word(whole_word)
            .with_max_results(max_results);

        let results = TextSearcher::search(&mut self.inner, pattern, &options)
            .map_err(|e| PyRuntimeError::new_err(format!("Search failed: {}", e)))?;

        let py_list = pyo3::types::PyList::empty(py);
        for result in results {
            let dict = pyo3::types::PyDict::new(py);
            dict.set_item("page", result.page)?;
            dict.set_item("text", &result.text)?;
            dict.set_item("x", result.bbox.x)?;
            dict.set_item("y", result.bbox.y)?;
            dict.set_item("width", result.bbox.width)?;
            dict.set_item("height", result.bbox.height)?;
            py_list.append(dict)?;
        }
        Ok(py_list.into())
    }

    /// Search for text on a specific page.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///     pattern (str): Search pattern (regex or literal text)
    ///     case_insensitive (bool): Case insensitive search (default: False)
    ///     literal (bool): Treat pattern as literal text, not regex (default: False)
    ///     whole_word (bool): Match whole words only (default: False)
    ///     max_results (int): Maximum number of results, 0 = unlimited (default: 0)
    ///
    /// Returns:
    ///     list[dict]: List of search results (same format as search())
    ///
    /// Example:
    ///     >>> results = doc.search_page(0, "hello")
    #[pyo3(signature = (page, pattern, case_insensitive=false, literal=false, whole_word=false, max_results=0))]
    fn search_page(
        &mut self,
        py: Python<'_>,
        page: usize,
        pattern: &str,
        case_insensitive: bool,
        literal: bool,
        whole_word: bool,
        max_results: usize,
    ) -> PyResult<Py<PyAny>> {
        use crate::search::{SearchOptions, TextSearcher};

        let options = SearchOptions::new()
            .with_case_insensitive(case_insensitive)
            .with_literal(literal)
            .with_whole_word(whole_word)
            .with_max_results(max_results)
            .with_page_range(page, page);

        let results = TextSearcher::search(&mut self.inner, pattern, &options)
            .map_err(|e| PyRuntimeError::new_err(format!("Search failed: {}", e)))?;

        let py_list = pyo3::types::PyList::empty(py);
        for result in results {
            let dict = pyo3::types::PyDict::new(py);
            dict.set_item("page", result.page)?;
            dict.set_item("text", &result.text)?;
            dict.set_item("x", result.bbox.x)?;
            dict.set_item("y", result.bbox.y)?;
            dict.set_item("width", result.bbox.width)?;
            dict.set_item("height", result.bbox.height)?;
            py_list.append(dict)?;
        }
        Ok(py_list.into())
    }

    /// String representation of the document.
    ///
    /// Returns:
    ///     str: Representation showing PDF version
    fn __repr__(&self) -> String {
        format!("PdfDocument(version={}.{})", self.inner.version().0, self.inner.version().1)
    }
}

// === PDF Creation API ===

use crate::api::PdfBuilder as RustPdfBuilder;

/// Python wrapper for PDF creation.
///
/// Provides simple PDF creation from Markdown, HTML, or plain text.
///
/// # Methods
///
/// - `from_markdown(content)`: Create PDF from Markdown
/// - `from_html(content)`: Create PDF from HTML
/// - `from_text(content)`: Create PDF from plain text
/// - `save(path)`: Save PDF to file
///
/// Example:
///     >>> pdf = Pdf.from_markdown("# Hello World")
///     >>> pdf.save("output.pdf")
#[pyclass(name = "Pdf")]
pub struct PyPdf {
    bytes: Vec<u8>,
}

#[pymethods]
impl PyPdf {
    /// Create a PDF from Markdown content.
    ///
    /// Args:
    ///     content (str): Markdown content
    ///     title (str, optional): Document title
    ///     author (str, optional): Document author
    ///
    /// Returns:
    ///     Pdf: Created PDF document
    ///
    /// Raises:
    ///     RuntimeError: If PDF creation fails
    ///
    /// Example:
    ///     >>> pdf = Pdf.from_markdown("# Hello\\n\\nWorld")
    ///     >>> pdf.save("hello.pdf")
    #[staticmethod]
    #[pyo3(signature = (content, title=None, author=None))]
    fn from_markdown(content: &str, title: Option<&str>, author: Option<&str>) -> PyResult<Self> {
        let mut builder = RustPdfBuilder::new();
        if let Some(t) = title {
            builder = builder.title(t);
        }
        if let Some(a) = author {
            builder = builder.author(a);
        }

        let pdf = builder
            .from_markdown(content)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create PDF: {}", e)))?;

        Ok(PyPdf {
            bytes: pdf.into_bytes(),
        })
    }

    /// Create a PDF from HTML content.
    ///
    /// Args:
    ///     content (str): HTML content
    ///     title (str, optional): Document title
    ///     author (str, optional): Document author
    ///
    /// Returns:
    ///     Pdf: Created PDF document
    ///
    /// Example:
    ///     >>> pdf = Pdf.from_html("<h1>Hello</h1><p>World</p>")
    ///     >>> pdf.save("hello.pdf")
    #[staticmethod]
    #[pyo3(signature = (content, title=None, author=None))]
    fn from_html(content: &str, title: Option<&str>, author: Option<&str>) -> PyResult<Self> {
        let mut builder = RustPdfBuilder::new();
        if let Some(t) = title {
            builder = builder.title(t);
        }
        if let Some(a) = author {
            builder = builder.author(a);
        }

        let pdf = builder
            .from_html(content)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create PDF: {}", e)))?;

        Ok(PyPdf {
            bytes: pdf.into_bytes(),
        })
    }

    /// Create a PDF from plain text.
    ///
    /// Args:
    ///     content (str): Plain text content
    ///     title (str, optional): Document title
    ///     author (str, optional): Document author
    ///
    /// Returns:
    ///     Pdf: Created PDF document
    ///
    /// Example:
    ///     >>> pdf = Pdf.from_text("Hello, World!")
    ///     >>> pdf.save("hello.pdf")
    #[staticmethod]
    #[pyo3(signature = (content, title=None, author=None))]
    fn from_text(content: &str, title: Option<&str>, author: Option<&str>) -> PyResult<Self> {
        let mut builder = RustPdfBuilder::new();
        if let Some(t) = title {
            builder = builder.title(t);
        }
        if let Some(a) = author {
            builder = builder.author(a);
        }

        let pdf = builder
            .from_text(content)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create PDF: {}", e)))?;

        Ok(PyPdf {
            bytes: pdf.into_bytes(),
        })
    }

    /// Save the PDF to a file.
    ///
    /// Args:
    ///     path (str): Output file path
    ///
    /// Raises:
    ///     IOError: If the file cannot be written
    ///
    /// Example:
    ///     >>> pdf = Pdf.from_markdown("# Hello")
    ///     >>> pdf.save("output.pdf")
    fn save(&self, path: &str) -> PyResult<()> {
        std::fs::write(path, &self.bytes)
            .map_err(|e| PyIOError::new_err(format!("Failed to save PDF: {}", e)))
    }

    /// Get the PDF as bytes.
    ///
    /// Returns:
    ///     bytes: Raw PDF data
    ///
    /// Example:
    ///     >>> pdf = Pdf.from_markdown("# Hello")
    ///     >>> data = pdf.to_bytes()
    ///     >>> len(data) > 0
    ///     True
    fn to_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Get the size of the PDF in bytes.
    ///
    /// Returns:
    ///     int: Size in bytes
    fn __len__(&self) -> usize {
        self.bytes.len()
    }

    /// String representation.
    fn __repr__(&self) -> String {
        format!("Pdf({} bytes)", self.bytes.len())
    }
}

// === Office Conversion API ===

#[cfg(feature = "office")]
use crate::converters::office::OfficeConverter as RustOfficeConverter;

/// Python wrapper for Office to PDF conversion.
///
/// Converts Microsoft Office documents (DOCX, XLSX, PPTX) to PDF.
/// Requires the `office` feature to be enabled.
///
/// # Example
///
/// ```python
/// from pdf_oxide import OfficeConverter
///
/// # Convert a Word document to PDF
/// pdf = OfficeConverter.from_docx("document.docx")
/// pdf.save("document.pdf")
///
/// # Convert from bytes
/// with open("spreadsheet.xlsx", "rb") as f:
///     pdf = OfficeConverter.from_xlsx_bytes(f.read())
///     pdf.save("spreadsheet.pdf")
///
/// # Auto-detect format and convert
/// pdf = OfficeConverter.convert("presentation.pptx")
/// pdf.save("presentation.pdf")
/// ```
#[cfg(feature = "office")]
#[pyclass(name = "OfficeConverter")]
pub struct PyOfficeConverter;

#[cfg(feature = "office")]
#[pymethods]
impl PyOfficeConverter {
    /// Convert a DOCX file to PDF.
    ///
    /// Args:
    ///     path (str): Path to the DOCX file
    ///
    /// Returns:
    ///     Pdf: Created PDF document
    ///
    /// Raises:
    ///     IOError: If the file cannot be read
    ///     RuntimeError: If conversion fails
    ///
    /// Example:
    ///     >>> pdf = OfficeConverter.from_docx("document.docx")
    ///     >>> pdf.save("document.pdf")
    #[staticmethod]
    fn from_docx(path: &str) -> PyResult<PyPdf> {
        let converter = RustOfficeConverter::new();
        let bytes = converter
            .convert_docx(path)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to convert DOCX: {}", e)))?;
        Ok(PyPdf { bytes })
    }

    /// Convert DOCX bytes to PDF.
    ///
    /// Args:
    ///     data (bytes): DOCX file contents
    ///
    /// Returns:
    ///     Pdf: Created PDF document
    ///
    /// Raises:
    ///     RuntimeError: If conversion fails
    ///
    /// Example:
    ///     >>> with open("document.docx", "rb") as f:
    ///     ...     pdf = OfficeConverter.from_docx_bytes(f.read())
    ///     >>> pdf.save("document.pdf")
    #[staticmethod]
    fn from_docx_bytes(data: &[u8]) -> PyResult<PyPdf> {
        let converter = RustOfficeConverter::new();
        let bytes = converter
            .convert_docx_bytes(data)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to convert DOCX: {}", e)))?;
        Ok(PyPdf { bytes })
    }

    /// Convert an XLSX file to PDF.
    ///
    /// Args:
    ///     path (str): Path to the XLSX file
    ///
    /// Returns:
    ///     Pdf: Created PDF document
    ///
    /// Raises:
    ///     IOError: If the file cannot be read
    ///     RuntimeError: If conversion fails
    ///
    /// Example:
    ///     >>> pdf = OfficeConverter.from_xlsx("spreadsheet.xlsx")
    ///     >>> pdf.save("spreadsheet.pdf")
    #[staticmethod]
    fn from_xlsx(path: &str) -> PyResult<PyPdf> {
        let converter = RustOfficeConverter::new();
        let bytes = converter
            .convert_xlsx(path)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to convert XLSX: {}", e)))?;
        Ok(PyPdf { bytes })
    }

    /// Convert XLSX bytes to PDF.
    ///
    /// Args:
    ///     data (bytes): XLSX file contents
    ///
    /// Returns:
    ///     Pdf: Created PDF document
    ///
    /// Raises:
    ///     RuntimeError: If conversion fails
    ///
    /// Example:
    ///     >>> with open("spreadsheet.xlsx", "rb") as f:
    ///     ...     pdf = OfficeConverter.from_xlsx_bytes(f.read())
    ///     >>> pdf.save("spreadsheet.pdf")
    #[staticmethod]
    fn from_xlsx_bytes(data: &[u8]) -> PyResult<PyPdf> {
        let converter = RustOfficeConverter::new();
        let bytes = converter
            .convert_xlsx_bytes(data)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to convert XLSX: {}", e)))?;
        Ok(PyPdf { bytes })
    }

    /// Convert a PPTX file to PDF.
    ///
    /// Args:
    ///     path (str): Path to the PPTX file
    ///
    /// Returns:
    ///     Pdf: Created PDF document
    ///
    /// Raises:
    ///     IOError: If the file cannot be read
    ///     RuntimeError: If conversion fails
    ///
    /// Example:
    ///     >>> pdf = OfficeConverter.from_pptx("presentation.pptx")
    ///     >>> pdf.save("presentation.pdf")
    #[staticmethod]
    fn from_pptx(path: &str) -> PyResult<PyPdf> {
        let converter = RustOfficeConverter::new();
        let bytes = converter
            .convert_pptx(path)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to convert PPTX: {}", e)))?;
        Ok(PyPdf { bytes })
    }

    /// Convert PPTX bytes to PDF.
    ///
    /// Args:
    ///     data (bytes): PPTX file contents
    ///
    /// Returns:
    ///     Pdf: Created PDF document
    ///
    /// Raises:
    ///     RuntimeError: If conversion fails
    ///
    /// Example:
    ///     >>> with open("presentation.pptx", "rb") as f:
    ///     ...     pdf = OfficeConverter.from_pptx_bytes(f.read())
    ///     >>> pdf.save("presentation.pdf")
    #[staticmethod]
    fn from_pptx_bytes(data: &[u8]) -> PyResult<PyPdf> {
        let converter = RustOfficeConverter::new();
        let bytes = converter
            .convert_pptx_bytes(data)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to convert PPTX: {}", e)))?;
        Ok(PyPdf { bytes })
    }

    /// Auto-detect format and convert to PDF.
    ///
    /// Detects the file format based on extension and converts to PDF.
    /// Supports .docx, .xlsx, .xls, and .pptx files.
    ///
    /// Args:
    ///     path (str): Path to the Office document
    ///
    /// Returns:
    ///     Pdf: Created PDF document
    ///
    /// Raises:
    ///     IOError: If the file cannot be read
    ///     RuntimeError: If conversion fails or format is unsupported
    ///
    /// Example:
    ///     >>> pdf = OfficeConverter.convert("document.docx")
    ///     >>> pdf.save("document.pdf")
    #[staticmethod]
    fn convert(path: &str) -> PyResult<PyPdf> {
        let converter = RustOfficeConverter::new();
        let bytes = converter.convert(path).map_err(|e| {
            PyRuntimeError::new_err(format!("Failed to convert Office document: {}", e))
        })?;
        Ok(PyPdf { bytes })
    }
}

// === DOM Access API ===

use crate::editor::{ElementId, PdfElement, PdfPage as RustPdfPage, PdfText as RustPdfText};

/// Python wrapper for PDF page with DOM-like access.
///
/// Provides hierarchical access to page content elements.
///
/// Example:
///     >>> doc = PdfDocument("sample.pdf")
///     >>> page = doc.page(0)
///     >>> for text in page.find_text_containing("Hello"):
///     ...     print(f"{text.value} at {text.bbox}")
#[pyclass(name = "PdfPage", unsendable)]
pub struct PyPdfPage {
    inner: RustPdfPage,
}

#[pymethods]
impl PyPdfPage {
    /// Get the page index.
    ///
    /// Returns:
    ///     int: Zero-based page index
    #[getter]
    fn index(&self) -> usize {
        self.inner.page_index
    }

    /// Get page width.
    ///
    /// Returns:
    ///     float: Page width in points
    #[getter]
    fn width(&self) -> f32 {
        self.inner.width
    }

    /// Get page height.
    ///
    /// Returns:
    ///     float: Page height in points
    #[getter]
    fn height(&self) -> f32 {
        self.inner.height
    }

    /// Get all top-level elements on the page.
    ///
    /// Returns:
    ///     list[PdfElement]: List of child elements
    ///
    /// Example:
    ///     >>> for elem in page.children():
    ///     ...     if elem.is_text():
    ///     ...         print(elem.as_text().value)
    fn children(&self) -> Vec<PyPdfElement> {
        self.inner
            .children()
            .into_iter()
            .map(|e| PyPdfElement { inner: e })
            .collect()
    }

    /// Find all text elements containing the specified string.
    ///
    /// Args:
    ///     needle (str): String to search for
    ///
    /// Returns:
    ///     list[PdfText]: List of matching text elements
    ///
    /// Example:
    ///     >>> texts = page.find_text_containing("Hello")
    ///     >>> for t in texts:
    ///     ...     print(t.value)
    fn find_text_containing(&self, needle: &str) -> Vec<PyPdfText> {
        self.inner
            .find_text_containing(needle)
            .into_iter()
            .map(|t| PyPdfText { inner: t })
            .collect()
    }

    /// Find all images on the page.
    ///
    /// Returns:
    ///     list[PdfImage]: List of image elements
    fn find_images(&self) -> Vec<PyPdfImage> {
        self.inner
            .find_images()
            .into_iter()
            .map(|i| PyPdfImage { inner: i })
            .collect()
    }

    /// Get element by ID.
    ///
    /// Args:
    ///     element_id (str): The element ID as a string
    ///
    /// Returns:
    ///     PdfElement | None: The element if found, None otherwise
    fn get_element(&self, _element_id: &str) -> Option<PyPdfElement> {
        // Note: ElementId is UUID-based, this is a simplified lookup
        // In practice, users would use the ID from an existing element
        None // Simplified - would need proper ID parsing
    }

    /// Set text content for an element by ID.
    ///
    /// Args:
    ///     text_id: The ID of the text element (from PdfText.id)
    ///     new_text (str): New text content
    ///
    /// Raises:
    ///     RuntimeError: If the element is not found or is not a text element
    ///
    /// Example:
    ///     >>> for t in page.find_text_containing("old"):
    ///     ...     page.set_text(t.id, "new")
    fn set_text(&mut self, text_id: &PyPdfTextId, new_text: &str) -> PyResult<()> {
        self.inner
            .set_text(text_id.inner, new_text)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to set text: {}", e)))
    }

    // === Annotations ===

    /// Get all annotations on the page.
    ///
    /// Returns:
    ///     list[PdfAnnotation]: List of annotations
    fn annotations(&self) -> Vec<PyAnnotationWrapper> {
        self.inner
            .annotations()
            .iter()
            .map(|a| PyAnnotationWrapper { inner: a.clone() })
            .collect()
    }

    /// Add a link annotation to the page.
    ///
    /// Args:
    ///     x (float): X coordinate
    ///     y (float): Y coordinate
    ///     width (float): Link width
    ///     height (float): Link height
    ///     url (str): Target URL
    ///
    /// Returns:
    ///     str: Annotation ID
    ///
    /// Example:
    ///     >>> page.add_link(100, 700, 50, 12, "https://example.com")
    fn add_link(&mut self, x: f32, y: f32, width: f32, height: f32, url: &str) -> String {
        use crate::writer::LinkAnnotation;
        let link = LinkAnnotation::uri(crate::geometry::Rect::new(x, y, width, height), url);
        let id = self.inner.add_annotation(link);
        format!("{:?}", id)
    }

    /// Add a text highlight annotation.
    ///
    /// Args:
    ///     x (float): X coordinate
    ///     y (float): Y coordinate
    ///     width (float): Highlight width
    ///     height (float): Highlight height
    ///     color (tuple): RGB color as (r, g, b) where each is 0.0-1.0
    ///
    /// Example:
    ///     >>> page.add_highlight(100, 700, 200, 12, (1.0, 1.0, 0.0))  # Yellow
    fn add_highlight(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: (f32, f32, f32),
    ) -> String {
        use crate::writer::TextMarkupAnnotation;
        use crate::TextMarkupType;
        let rect = crate::geometry::Rect::new(x, y, width, height);
        let highlight = TextMarkupAnnotation::from_rect(TextMarkupType::Highlight, rect)
            .with_color(color.0, color.1, color.2);
        let id = self.inner.add_annotation(highlight);
        format!("{:?}", id)
    }

    /// Add a sticky note annotation.
    ///
    /// Args:
    ///     x (float): X coordinate
    ///     y (float): Y coordinate
    ///     text (str): Note content
    ///
    /// Example:
    ///     >>> page.add_note(100, 700, "This is important!")
    fn add_note(&mut self, x: f32, y: f32, text: &str) -> String {
        use crate::writer::TextAnnotation;
        // Create a small rect for the sticky note icon (24x24 is typical)
        let rect = crate::geometry::Rect::new(x, y, 24.0, 24.0);
        let note = TextAnnotation::new(rect, text);
        let id = self.inner.add_annotation(note);
        format!("{:?}", id)
    }

    /// Remove an annotation by index.
    ///
    /// Args:
    ///     index (int): Annotation index
    ///
    /// Returns:
    ///     bool: True if annotation was removed
    fn remove_annotation(&mut self, index: usize) -> bool {
        self.inner.remove_annotation(index).is_some()
    }

    // === Element Manipulation ===

    /// Add a text element to the page.
    ///
    /// Args:
    ///     text (str): Text content
    ///     x (float): X coordinate
    ///     y (float): Y coordinate
    ///     font_size (float): Font size in points (default: 12.0)
    ///
    /// Returns:
    ///     PdfTextId: ID of the new element
    ///
    /// Example:
    ///     >>> text_id = page.add_text("Hello World", 100, 700, 14.0)
    #[pyo3(signature = (text, x, y, font_size=12.0))]
    fn add_text(&mut self, text: &str, x: f32, y: f32, font_size: f32) -> PyPdfTextId {
        use crate::elements::{FontSpec, TextContent, TextStyle};

        let content = TextContent {
            text: text.to_string(),
            bbox: crate::geometry::Rect::new(x, y, text.len() as f32 * font_size * 0.6, font_size),
            font: FontSpec {
                name: "Helvetica".to_string(),
                size: font_size,
            },
            style: TextStyle::default(),
            reading_order: None,
            origin: None,
            rotation_degrees: None,
            matrix: None,
        };

        let id = self.inner.add_text(content);
        PyPdfTextId { inner: id }
    }

    /// Remove an element by ID.
    ///
    /// Args:
    ///     element_id: Element ID (from PdfText.id, etc.)
    ///
    /// Returns:
    ///     bool: True if element was removed
    fn remove_element(&mut self, element_id: &PyPdfTextId) -> bool {
        self.inner.remove_element(element_id.inner)
    }

    /// String representation.
    fn __repr__(&self) -> String {
        format!(
            "PdfPage(index={}, width={:.1}, height={:.1})",
            self.inner.page_index, self.inner.width, self.inner.height
        )
    }
}

/// Python wrapper for text element ID.
///
/// Used to identify text elements for modification.
#[pyclass(name = "PdfTextId")]
#[derive(Clone)]
pub struct PyPdfTextId {
    inner: ElementId,
}

#[pymethods]
impl PyPdfTextId {
    fn __repr__(&self) -> String {
        format!("PdfTextId({:?})", self.inner)
    }
}

/// Python wrapper for text element.
///
/// Provides access to text content, position, and formatting.
///
/// Example:
///     >>> for text in page.find_text_containing("Hello"):
///     ...     print(f"{text.value} at {text.bbox}")
///     ...     print(f"Font: {text.font_name} {text.font_size}pt")
#[pyclass(name = "PdfText")]
#[derive(Clone)]
pub struct PyPdfText {
    inner: RustPdfText,
}

#[pymethods]
impl PyPdfText {
    /// Get the element ID.
    ///
    /// Returns:
    ///     PdfTextId: The unique element ID
    #[getter]
    fn id(&self) -> PyPdfTextId {
        PyPdfTextId {
            inner: self.inner.id(),
        }
    }

    /// Get the text content.
    ///
    /// Returns:
    ///     str: The text content
    #[getter]
    fn value(&self) -> String {
        self.inner.text().to_string()
    }

    /// Get the text content (alias for value).
    #[getter]
    fn text(&self) -> String {
        self.value()
    }

    /// Get the bounding box as (x, y, width, height).
    ///
    /// Returns:
    ///     tuple[float, float, float, float]: Bounding box coordinates
    #[getter]
    fn bbox(&self) -> (f32, f32, f32, f32) {
        let r = self.inner.bbox();
        (r.x, r.y, r.width, r.height)
    }

    /// Get the font name.
    ///
    /// Returns:
    ///     str: Font name
    #[getter]
    fn font_name(&self) -> String {
        self.inner.font_name().to_string()
    }

    /// Get the font size in points.
    ///
    /// Returns:
    ///     float: Font size
    #[getter]
    fn font_size(&self) -> f32 {
        self.inner.font_size()
    }

    /// Check if text is bold.
    ///
    /// Returns:
    ///     bool: True if bold
    #[getter]
    fn is_bold(&self) -> bool {
        self.inner.is_bold()
    }

    /// Check if text is italic.
    ///
    /// Returns:
    ///     bool: True if italic
    #[getter]
    fn is_italic(&self) -> bool {
        self.inner.is_italic()
    }

    /// Check if text contains a substring.
    ///
    /// Args:
    ///     needle (str): String to search for
    ///
    /// Returns:
    ///     bool: True if text contains needle
    fn contains(&self, needle: &str) -> bool {
        self.inner.contains(needle)
    }

    /// Check if text starts with a prefix.
    ///
    /// Args:
    ///     prefix (str): Prefix to check
    ///
    /// Returns:
    ///     bool: True if text starts with prefix
    fn starts_with(&self, prefix: &str) -> bool {
        self.inner.starts_with(prefix)
    }

    /// Check if text ends with a suffix.
    ///
    /// Args:
    ///     suffix (str): Suffix to check
    ///
    /// Returns:
    ///     bool: True if text ends with suffix
    fn ends_with(&self, suffix: &str) -> bool {
        self.inner.ends_with(suffix)
    }

    /// String representation.
    fn __repr__(&self) -> String {
        let text = self.inner.text();
        let preview = if text.len() > 30 {
            format!("{}...", &text[..30])
        } else {
            text.to_string()
        };
        format!("PdfText({:?})", preview)
    }
}

/// Python wrapper for image element.
#[pyclass(name = "PdfImage")]
#[derive(Clone)]
pub struct PyPdfImage {
    inner: crate::editor::PdfImage,
}

#[pymethods]
impl PyPdfImage {
    /// Get the bounding box as (x, y, width, height).
    #[getter]
    fn bbox(&self) -> (f32, f32, f32, f32) {
        let r = self.inner.bbox();
        (r.x, r.y, r.width, r.height)
    }

    /// Get image width in pixels.
    #[getter]
    fn width(&self) -> u32 {
        self.inner.dimensions().0
    }

    /// Get image height in pixels.
    #[getter]
    fn height(&self) -> u32 {
        self.inner.dimensions().1
    }

    /// Get aspect ratio (width / height).
    #[getter]
    fn aspect_ratio(&self) -> f32 {
        self.inner.aspect_ratio()
    }

    fn __repr__(&self) -> String {
        let (w, h) = self.inner.dimensions();
        format!("PdfImage({}x{})", w, h)
    }
}

/// Python wrapper for annotation.
#[pyclass(name = "PdfAnnotation")]
#[derive(Clone)]
pub struct PyAnnotationWrapper {
    inner: crate::editor::AnnotationWrapper,
}

#[pymethods]
impl PyAnnotationWrapper {
    /// Get the annotation subtype (e.g., "Link", "Highlight", "Text").
    #[getter]
    fn subtype(&self) -> String {
        format!("{:?}", self.inner.subtype())
    }

    /// Get the bounding rectangle as (x, y, width, height).
    #[getter]
    fn rect(&self) -> (f32, f32, f32, f32) {
        let r = self.inner.rect();
        (r.x, r.y, r.width, r.height)
    }

    /// Get the annotation contents/text if available.
    #[getter]
    fn contents(&self) -> Option<String> {
        self.inner.contents().map(|s| s.to_string())
    }

    /// Get the annotation color as (r, g, b) if available.
    #[getter]
    fn color(&self) -> Option<(f32, f32, f32)> {
        self.inner.color()
    }

    /// Check if this annotation has been modified.
    #[getter]
    fn is_modified(&self) -> bool {
        self.inner.is_modified()
    }

    /// Check if this is a new annotation (not loaded from PDF).
    #[getter]
    fn is_new(&self) -> bool {
        self.inner.is_new()
    }

    fn __repr__(&self) -> String {
        format!("PdfAnnotation(subtype={:?})", self.inner.subtype())
    }
}

/// Python wrapper for generic PDF element.
///
/// Can be one of: Text, Image, Path, Table, or Structure.
#[pyclass(name = "PdfElement")]
#[derive(Clone)]
pub struct PyPdfElement {
    inner: PdfElement,
}

#[pymethods]
impl PyPdfElement {
    /// Check if this is a text element.
    fn is_text(&self) -> bool {
        self.inner.is_text()
    }

    /// Check if this is an image element.
    fn is_image(&self) -> bool {
        self.inner.is_image()
    }

    /// Check if this is a path element.
    fn is_path(&self) -> bool {
        self.inner.is_path()
    }

    /// Check if this is a table element.
    fn is_table(&self) -> bool {
        self.inner.is_table()
    }

    /// Check if this is a structure element.
    fn is_structure(&self) -> bool {
        self.inner.is_structure()
    }

    /// Get as text element if this is a text element.
    ///
    /// Returns:
    ///     PdfText | None: The text element, or None if not a text element
    fn as_text(&self) -> Option<PyPdfText> {
        if let PdfElement::Text(t) = &self.inner {
            Some(PyPdfText { inner: t.clone() })
        } else {
            None
        }
    }

    /// Get as image element if this is an image element.
    ///
    /// Returns:
    ///     PdfImage | None: The image element, or None if not an image element
    fn as_image(&self) -> Option<PyPdfImage> {
        if let PdfElement::Image(i) = &self.inner {
            Some(PyPdfImage { inner: i.clone() })
        } else {
            None
        }
    }

    /// Get the bounding box.
    #[getter]
    fn bbox(&self) -> (f32, f32, f32, f32) {
        let r = self.inner.bbox();
        (r.x, r.y, r.width, r.height)
    }

    fn __repr__(&self) -> String {
        match &self.inner {
            PdfElement::Text(t) => format!("PdfElement::Text({:?})", t.text()),
            PdfElement::Image(i) => {
                format!("PdfElement::Image({}x{})", i.dimensions().0, i.dimensions().1)
            },
            PdfElement::Path(_) => "PdfElement::Path(...)".to_string(),
            PdfElement::Table(t) => {
                format!("PdfElement::Table({}x{})", t.row_count(), t.column_count())
            },
            PdfElement::Structure(s) => {
                format!("PdfElement::Structure({:?})", s.structure_type())
            },
        }
    }
}

// === Text Extraction Types ===

/// A single character with its position and styling information.
///
/// Low-level character extraction result containing position, font, and style data
/// for each character in a PDF page. Use `extract_chars()` to get a list of these.
///
/// # Attributes
///
/// - `char` (str): The character itself
/// - `bbox` (tuple): Bounding box as (x, y, width, height)
/// - `font_name` (str): Font family name
/// - `font_size` (float): Font size in points
/// - `font_weight` (str): "normal", "bold", "light", etc.
/// - `is_italic` (bool): Whether the character is italic
/// - `color` (tuple): RGB color as (r, g, b) with values 0.0-1.0
#[pyclass(name = "TextChar")]
#[derive(Clone)]
pub struct PyTextChar {
    inner: RustTextChar,
}

#[pymethods]
impl PyTextChar {
    /// The character itself.
    #[getter]
    fn char(&self) -> char {
        self.inner.char
    }

    /// Bounding box of the character.
    ///
    /// Returns:
    ///     tuple[float, float, float, float]: (x, y, width, height)
    #[getter]
    fn bbox(&self) -> (f32, f32, f32, f32) {
        (
            self.inner.bbox.x,
            self.inner.bbox.y,
            self.inner.bbox.width,
            self.inner.bbox.height,
        )
    }

    /// Font name/family.
    #[getter]
    fn font_name(&self) -> String {
        self.inner.font_name.clone()
    }

    /// Font size in points.
    #[getter]
    fn font_size(&self) -> f32 {
        self.inner.font_size
    }

    /// Font weight as a string.
    ///
    /// Returns:
    ///     str: "normal" or "bold"
    #[getter]
    fn font_weight(&self) -> String {
        match self.inner.font_weight {
            FontWeight::Thin => "thin".to_string(),
            FontWeight::ExtraLight => "extra-light".to_string(),
            FontWeight::Light => "light".to_string(),
            FontWeight::Normal => "normal".to_string(),
            FontWeight::Medium => "medium".to_string(),
            FontWeight::SemiBold => "semi-bold".to_string(),
            FontWeight::Bold => "bold".to_string(),
            FontWeight::ExtraBold => "extra-bold".to_string(),
            FontWeight::Black => "black".to_string(),
        }
    }

    /// Whether the character is italic.
    #[getter]
    fn is_italic(&self) -> bool {
        self.inner.is_italic
    }

    /// Text color as RGB tuple.
    ///
    /// Returns:
    ///     tuple: (r, g, b) with values 0.0-1.0
    #[getter]
    fn color(&self) -> (f32, f32, f32) {
        (self.inner.color.r, self.inner.color.g, self.inner.color.b)
    }

    /// Text rotation angle in degrees.
    #[getter]
    fn rotation_degrees(&self) -> f32 {
        self.inner.rotation_degrees
    }

    /// Baseline X position.
    #[getter]
    fn origin_x(&self) -> f32 {
        self.inner.origin_x
    }

    /// Baseline Y position.
    #[getter]
    fn origin_y(&self) -> f32 {
        self.inner.origin_y
    }

    /// Horizontal distance to next character.
    #[getter]
    fn advance_width(&self) -> f32 {
        self.inner.advance_width
    }

    /// Marked Content ID (for Tagged PDFs).
    ///
    /// Returns:
    ///     int | None: MCID if available, None otherwise
    #[getter]
    fn mcid(&self) -> Option<u32> {
        self.inner.mcid
    }

    fn __repr__(&self) -> String {
        format!(
            "TextChar('{}' at ({:.1}, {:.1}), {}pt {})",
            self.inner.char,
            self.inner.bbox.x,
            self.inner.bbox.y,
            self.inner.font_size as i32,
            self.inner.font_name
        )
    }
}

// === Advanced Graphics Types ===

use crate::layout::{Color as RustColor, FontWeight, TextChar as RustTextChar};
use crate::writer::{
    BlendMode as RustBlendMode, LineCap as RustLineCap, LineJoin as RustLineJoin,
    PatternPresets as RustPatternPresets,
};

/// RGB Color for PDF graphics.
///
/// Example:
///     >>> color = Color(1.0, 0.0, 0.0)  # Red
///     >>> color = Color.red()
///     >>> color = Color.from_hex("#FF0000")
#[pyclass(name = "Color")]
#[derive(Clone)]
pub struct PyColor {
    inner: RustColor,
}

#[pymethods]
impl PyColor {
    /// Create a new RGB color.
    ///
    /// Args:
    ///     r (float): Red component (0.0 to 1.0)
    ///     g (float): Green component (0.0 to 1.0)
    ///     b (float): Blue component (0.0 to 1.0)
    #[new]
    fn new(r: f32, g: f32, b: f32) -> Self {
        PyColor {
            inner: RustColor::new(r, g, b),
        }
    }

    /// Create color from hex string.
    ///
    /// Args:
    ///     hex_str (str): Hex color like "#FF0000" or "FF0000"
    ///
    /// Example:
    ///     >>> red = Color.from_hex("#FF0000")
    #[staticmethod]
    fn from_hex(hex_str: &str) -> PyResult<Self> {
        let hex = hex_str.trim_start_matches('#');
        if hex.len() != 6 {
            return Err(PyRuntimeError::new_err("Invalid hex color format"));
        }
        let r = u8::from_str_radix(&hex[0..2], 16)
            .map_err(|_| PyRuntimeError::new_err("Invalid hex color"))?;
        let g = u8::from_str_radix(&hex[2..4], 16)
            .map_err(|_| PyRuntimeError::new_err("Invalid hex color"))?;
        let b = u8::from_str_radix(&hex[4..6], 16)
            .map_err(|_| PyRuntimeError::new_err("Invalid hex color"))?;
        Ok(PyColor {
            inner: RustColor::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0),
        })
    }

    /// Black color.
    #[staticmethod]
    fn black() -> Self {
        PyColor {
            inner: RustColor::black(),
        }
    }

    /// White color.
    #[staticmethod]
    fn white() -> Self {
        PyColor {
            inner: RustColor::white(),
        }
    }

    /// Red color.
    #[staticmethod]
    fn red() -> Self {
        PyColor {
            inner: RustColor::new(1.0, 0.0, 0.0),
        }
    }

    /// Green color.
    #[staticmethod]
    fn green() -> Self {
        PyColor {
            inner: RustColor::new(0.0, 1.0, 0.0),
        }
    }

    /// Blue color.
    #[staticmethod]
    fn blue() -> Self {
        PyColor {
            inner: RustColor::new(0.0, 0.0, 1.0),
        }
    }

    /// Get red component.
    #[getter]
    fn r(&self) -> f32 {
        self.inner.r
    }

    /// Get green component.
    #[getter]
    fn g(&self) -> f32 {
        self.inner.g
    }

    /// Get blue component.
    #[getter]
    fn b(&self) -> f32 {
        self.inner.b
    }

    fn __repr__(&self) -> String {
        format!("Color({}, {}, {})", self.inner.r, self.inner.g, self.inner.b)
    }
}

/// Blend modes for transparency effects.
///
/// Example:
///     >>> gs = ExtGState().blend_mode(BlendMode.MULTIPLY)
#[pyclass(name = "BlendMode")]
#[derive(Clone)]
pub struct PyBlendMode {
    inner: RustBlendMode,
}

#[pymethods]
impl PyBlendMode {
    /// Normal blend mode (default).
    #[staticmethod]
    #[allow(non_snake_case)]
    fn NORMAL() -> Self {
        PyBlendMode {
            inner: RustBlendMode::Normal,
        }
    }

    /// Multiply blend mode.
    #[staticmethod]
    #[allow(non_snake_case)]
    fn MULTIPLY() -> Self {
        PyBlendMode {
            inner: RustBlendMode::Multiply,
        }
    }

    /// Screen blend mode.
    #[staticmethod]
    #[allow(non_snake_case)]
    fn SCREEN() -> Self {
        PyBlendMode {
            inner: RustBlendMode::Screen,
        }
    }

    /// Overlay blend mode.
    #[staticmethod]
    #[allow(non_snake_case)]
    fn OVERLAY() -> Self {
        PyBlendMode {
            inner: RustBlendMode::Overlay,
        }
    }

    /// Darken blend mode.
    #[staticmethod]
    #[allow(non_snake_case)]
    fn DARKEN() -> Self {
        PyBlendMode {
            inner: RustBlendMode::Darken,
        }
    }

    /// Lighten blend mode.
    #[staticmethod]
    #[allow(non_snake_case)]
    fn LIGHTEN() -> Self {
        PyBlendMode {
            inner: RustBlendMode::Lighten,
        }
    }

    /// Color dodge blend mode.
    #[staticmethod]
    #[allow(non_snake_case)]
    fn COLOR_DODGE() -> Self {
        PyBlendMode {
            inner: RustBlendMode::ColorDodge,
        }
    }

    /// Color burn blend mode.
    #[staticmethod]
    #[allow(non_snake_case)]
    fn COLOR_BURN() -> Self {
        PyBlendMode {
            inner: RustBlendMode::ColorBurn,
        }
    }

    /// Hard light blend mode.
    #[staticmethod]
    #[allow(non_snake_case)]
    fn HARD_LIGHT() -> Self {
        PyBlendMode {
            inner: RustBlendMode::HardLight,
        }
    }

    /// Soft light blend mode.
    #[staticmethod]
    #[allow(non_snake_case)]
    fn SOFT_LIGHT() -> Self {
        PyBlendMode {
            inner: RustBlendMode::SoftLight,
        }
    }

    /// Difference blend mode.
    #[staticmethod]
    #[allow(non_snake_case)]
    fn DIFFERENCE() -> Self {
        PyBlendMode {
            inner: RustBlendMode::Difference,
        }
    }

    /// Exclusion blend mode.
    #[staticmethod]
    #[allow(non_snake_case)]
    fn EXCLUSION() -> Self {
        PyBlendMode {
            inner: RustBlendMode::Exclusion,
        }
    }

    fn __repr__(&self) -> String {
        format!("BlendMode.{}", self.inner.as_pdf_name())
    }
}

/// Extended Graphics State for transparency and blend effects.
///
/// Example:
///     >>> gs = ExtGState().alpha(0.5).blend_mode(BlendMode.MULTIPLY)
#[pyclass(name = "ExtGState")]
#[derive(Clone)]
pub struct PyExtGState {
    fill_alpha: Option<f32>,
    stroke_alpha: Option<f32>,
    blend_mode: Option<RustBlendMode>,
}

#[pymethods]
impl PyExtGState {
    /// Create a new ExtGState builder.
    #[new]
    fn new() -> Self {
        PyExtGState {
            fill_alpha: None,
            stroke_alpha: None,
            blend_mode: None,
        }
    }

    /// Set fill opacity (0.0 = transparent, 1.0 = opaque).
    fn fill_alpha(&self, alpha: f32) -> Self {
        PyExtGState {
            fill_alpha: Some(alpha.clamp(0.0, 1.0)),
            stroke_alpha: self.stroke_alpha,
            blend_mode: self.blend_mode,
        }
    }

    /// Set stroke opacity (0.0 = transparent, 1.0 = opaque).
    fn stroke_alpha(&self, alpha: f32) -> Self {
        PyExtGState {
            fill_alpha: self.fill_alpha,
            stroke_alpha: Some(alpha.clamp(0.0, 1.0)),
            blend_mode: self.blend_mode,
        }
    }

    /// Set both fill and stroke opacity.
    fn alpha(&self, alpha: f32) -> Self {
        let a = alpha.clamp(0.0, 1.0);
        PyExtGState {
            fill_alpha: Some(a),
            stroke_alpha: Some(a),
            blend_mode: self.blend_mode,
        }
    }

    /// Set blend mode.
    fn blend_mode(&self, mode: &PyBlendMode) -> Self {
        PyExtGState {
            fill_alpha: self.fill_alpha,
            stroke_alpha: self.stroke_alpha,
            blend_mode: Some(mode.inner),
        }
    }

    /// Create semi-transparent state (50% opacity).
    #[staticmethod]
    fn semi_transparent() -> Self {
        PyExtGState {
            fill_alpha: Some(0.5),
            stroke_alpha: Some(0.5),
            blend_mode: None,
        }
    }

    fn __repr__(&self) -> String {
        let mut parts = Vec::new();
        if let Some(a) = self.fill_alpha {
            parts.push(format!("fill_alpha={}", a));
        }
        if let Some(a) = self.stroke_alpha {
            parts.push(format!("stroke_alpha={}", a));
        }
        if let Some(ref m) = self.blend_mode {
            parts.push(format!("blend_mode={}", m.as_pdf_name()));
        }
        format!("ExtGState({})", parts.join(", "))
    }
}

/// Linear gradient builder.
///
/// Example:
///     >>> gradient = LinearGradient() \
///     ...     .start(0, 0).end(100, 100) \
///     ...     .add_stop(0.0, Color.red()) \
///     ...     .add_stop(1.0, Color.blue())
#[pyclass(name = "LinearGradient")]
#[derive(Clone)]
pub struct PyLinearGradient {
    start: (f32, f32),
    end: (f32, f32),
    stops: Vec<(f32, RustColor)>,
    extend_start: bool,
    extend_end: bool,
}

#[pymethods]
impl PyLinearGradient {
    /// Create a new linear gradient.
    #[new]
    fn new() -> Self {
        PyLinearGradient {
            start: (0.0, 0.0),
            end: (100.0, 0.0),
            stops: Vec::new(),
            extend_start: true,
            extend_end: true,
        }
    }

    /// Set start point.
    fn start(&self, x: f32, y: f32) -> Self {
        PyLinearGradient {
            start: (x, y),
            end: self.end,
            stops: self.stops.clone(),
            extend_start: self.extend_start,
            extend_end: self.extend_end,
        }
    }

    /// Set end point.
    fn end(&self, x: f32, y: f32) -> Self {
        PyLinearGradient {
            start: self.start,
            end: (x, y),
            stops: self.stops.clone(),
            extend_start: self.extend_start,
            extend_end: self.extend_end,
        }
    }

    /// Add a color stop.
    ///
    /// Args:
    ///     position (float): Position along gradient (0.0 to 1.0)
    ///     color (Color): Color at this position
    fn add_stop(&self, position: f32, color: &PyColor) -> Self {
        let mut stops = self.stops.clone();
        stops.push((position.clamp(0.0, 1.0), color.inner));
        PyLinearGradient {
            start: self.start,
            end: self.end,
            stops,
            extend_start: self.extend_start,
            extend_end: self.extend_end,
        }
    }

    /// Set whether to extend gradient beyond endpoints.
    fn extend(&self, extend: bool) -> Self {
        PyLinearGradient {
            start: self.start,
            end: self.end,
            stops: self.stops.clone(),
            extend_start: extend,
            extend_end: extend,
        }
    }

    /// Create a horizontal gradient.
    #[staticmethod]
    fn horizontal(width: f32, start_color: &PyColor, end_color: &PyColor) -> Self {
        PyLinearGradient {
            start: (0.0, 0.0),
            end: (width, 0.0),
            stops: vec![(0.0, start_color.inner), (1.0, end_color.inner)],
            extend_start: true,
            extend_end: true,
        }
    }

    /// Create a vertical gradient.
    #[staticmethod]
    fn vertical(height: f32, start_color: &PyColor, end_color: &PyColor) -> Self {
        PyLinearGradient {
            start: (0.0, 0.0),
            end: (0.0, height),
            stops: vec![(0.0, start_color.inner), (1.0, end_color.inner)],
            extend_start: true,
            extend_end: true,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "LinearGradient(({}, {}) -> ({}, {}), {} stops)",
            self.start.0,
            self.start.1,
            self.end.0,
            self.end.1,
            self.stops.len()
        )
    }
}

/// Radial gradient builder.
///
/// Example:
///     >>> gradient = RadialGradient.centered(50, 50, 50) \
///     ...     .add_stop(0.0, Color.white()) \
///     ...     .add_stop(1.0, Color.black())
#[pyclass(name = "RadialGradient")]
#[derive(Clone)]
pub struct PyRadialGradient {
    inner_center: (f32, f32),
    inner_radius: f32,
    outer_center: (f32, f32),
    outer_radius: f32,
    stops: Vec<(f32, RustColor)>,
}

#[pymethods]
impl PyRadialGradient {
    /// Create a new radial gradient.
    #[new]
    fn new() -> Self {
        PyRadialGradient {
            inner_center: (50.0, 50.0),
            inner_radius: 0.0,
            outer_center: (50.0, 50.0),
            outer_radius: 50.0,
            stops: Vec::new(),
        }
    }

    /// Create a centered radial gradient.
    #[staticmethod]
    fn centered(cx: f32, cy: f32, radius: f32) -> Self {
        PyRadialGradient {
            inner_center: (cx, cy),
            inner_radius: 0.0,
            outer_center: (cx, cy),
            outer_radius: radius,
            stops: Vec::new(),
        }
    }

    /// Set inner circle.
    fn inner_circle(&self, cx: f32, cy: f32, radius: f32) -> Self {
        PyRadialGradient {
            inner_center: (cx, cy),
            inner_radius: radius,
            outer_center: self.outer_center,
            outer_radius: self.outer_radius,
            stops: self.stops.clone(),
        }
    }

    /// Set outer circle.
    fn outer_circle(&self, cx: f32, cy: f32, radius: f32) -> Self {
        PyRadialGradient {
            inner_center: self.inner_center,
            inner_radius: self.inner_radius,
            outer_center: (cx, cy),
            outer_radius: radius,
            stops: self.stops.clone(),
        }
    }

    /// Add a color stop.
    fn add_stop(&self, position: f32, color: &PyColor) -> Self {
        let mut stops = self.stops.clone();
        stops.push((position.clamp(0.0, 1.0), color.inner));
        PyRadialGradient {
            inner_center: self.inner_center,
            inner_radius: self.inner_radius,
            outer_center: self.outer_center,
            outer_radius: self.outer_radius,
            stops,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "RadialGradient(center=({}, {}), radius={}, {} stops)",
            self.outer_center.0,
            self.outer_center.1,
            self.outer_radius,
            self.stops.len()
        )
    }
}

/// Line cap styles.
#[pyclass(name = "LineCap")]
#[derive(Clone)]
pub struct PyLineCap {
    _inner: RustLineCap,
}

#[pymethods]
impl PyLineCap {
    /// Butt cap (default).
    #[staticmethod]
    #[allow(non_snake_case)]
    fn BUTT() -> Self {
        PyLineCap {
            _inner: RustLineCap::Butt,
        }
    }

    /// Round cap.
    #[staticmethod]
    #[allow(non_snake_case)]
    fn ROUND() -> Self {
        PyLineCap {
            _inner: RustLineCap::Round,
        }
    }

    /// Square cap.
    #[staticmethod]
    #[allow(non_snake_case)]
    fn SQUARE() -> Self {
        PyLineCap {
            _inner: RustLineCap::Square,
        }
    }
}

/// Line join styles.
#[pyclass(name = "LineJoin")]
#[derive(Clone)]
pub struct PyLineJoin {
    _inner: RustLineJoin,
}

#[pymethods]
impl PyLineJoin {
    /// Miter join (default).
    #[staticmethod]
    #[allow(non_snake_case)]
    fn MITER() -> Self {
        PyLineJoin {
            _inner: RustLineJoin::Miter,
        }
    }

    /// Round join.
    #[staticmethod]
    #[allow(non_snake_case)]
    fn ROUND() -> Self {
        PyLineJoin {
            _inner: RustLineJoin::Round,
        }
    }

    /// Bevel join.
    #[staticmethod]
    #[allow(non_snake_case)]
    fn BEVEL() -> Self {
        PyLineJoin {
            _inner: RustLineJoin::Bevel,
        }
    }
}

/// Pattern presets for common fill patterns.
///
/// Example:
///     >>> content = PatternPresets.checkerboard(10, Color.white(), Color.black())
#[pyclass(name = "PatternPresets")]
pub struct PyPatternPresets;

#[pymethods]
impl PyPatternPresets {
    /// Create horizontal stripes pattern.
    #[staticmethod]
    fn horizontal_stripes(width: f32, height: f32, stripe_height: f32, color: &PyColor) -> Vec<u8> {
        RustPatternPresets::horizontal_stripes(width, height, stripe_height, color.inner)
    }

    /// Create vertical stripes pattern.
    #[staticmethod]
    fn vertical_stripes(width: f32, height: f32, stripe_width: f32, color: &PyColor) -> Vec<u8> {
        RustPatternPresets::vertical_stripes(width, height, stripe_width, color.inner)
    }

    /// Create checkerboard pattern.
    #[staticmethod]
    fn checkerboard(size: f32, color1: &PyColor, color2: &PyColor) -> Vec<u8> {
        RustPatternPresets::checkerboard(size, color1.inner, color2.inner)
    }

    /// Create dot pattern.
    #[staticmethod]
    fn dots(spacing: f32, radius: f32, color: &PyColor) -> Vec<u8> {
        RustPatternPresets::dots(spacing, radius, color.inner)
    }

    /// Create diagonal lines pattern.
    #[staticmethod]
    fn diagonal_lines(size: f32, line_width: f32, color: &PyColor) -> Vec<u8> {
        RustPatternPresets::diagonal_lines(size, line_width, color.inner)
    }

    /// Create crosshatch pattern.
    #[staticmethod]
    fn crosshatch(size: f32, line_width: f32, color: &PyColor) -> Vec<u8> {
        RustPatternPresets::crosshatch(size, line_width, color.inner)
    }
}

/// Python module for PDF library.
///
/// This is the internal module (pdf_oxide) that gets imported by the Python package.
#[pymodule]
fn pdf_oxide(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Document reading
    m.add_class::<PyPdfDocument>()?;

    // PDF creation
    m.add_class::<PyPdf>()?;

    // DOM access types
    m.add_class::<PyPdfPage>()?;
    m.add_class::<PyPdfText>()?;
    m.add_class::<PyPdfTextId>()?;
    m.add_class::<PyPdfImage>()?;
    m.add_class::<PyPdfElement>()?;
    m.add_class::<PyAnnotationWrapper>()?;

    // Text extraction types
    m.add_class::<PyTextChar>()?;

    // Advanced graphics
    m.add_class::<PyColor>()?;
    m.add_class::<PyBlendMode>()?;
    m.add_class::<PyExtGState>()?;
    m.add_class::<PyLinearGradient>()?;
    m.add_class::<PyRadialGradient>()?;
    m.add_class::<PyLineCap>()?;
    m.add_class::<PyLineJoin>()?;
    m.add_class::<PyPatternPresets>()?;

    // Office conversion (optional, requires office feature)
    #[cfg(feature = "office")]
    m.add_class::<PyOfficeConverter>()?;

    m.add("VERSION", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
