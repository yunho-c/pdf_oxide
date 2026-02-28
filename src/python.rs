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

impl PyPdfDocument {
    /// Ensure the editor is initialized, creating it from the path if needed.
    fn ensure_editor(&mut self) -> PyResult<()> {
        if self.editor.is_none() {
            let editor = RustDocumentEditor::open(&self.path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to open editor: {}", e)))?;
            self.editor = Some(editor);
        }
        Ok(())
    }
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
    #[pyo3(signature = (page, preserve_layout=false, detect_headings=true, include_images=true, image_output_dir=None, embed_images=true, include_form_fields=true))]
    fn to_markdown(
        &mut self,
        page: usize,
        preserve_layout: bool,
        detect_headings: bool,
        include_images: bool,
        image_output_dir: Option<String>,
        embed_images: bool,
        include_form_fields: bool,
    ) -> PyResult<String> {
        let options = RustConversionOptions {
            preserve_layout,
            detect_headings,
            extract_tables: false,
            include_images,
            image_output_dir,
            embed_images,
            include_form_fields,
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
    #[pyo3(signature = (page, preserve_layout=false, detect_headings=true, include_images=true, image_output_dir=None, embed_images=true, include_form_fields=true))]
    fn to_html(
        &mut self,
        page: usize,
        preserve_layout: bool,
        detect_headings: bool,
        include_images: bool,
        image_output_dir: Option<String>,
        embed_images: bool,
        include_form_fields: bool,
    ) -> PyResult<String> {
        let options = RustConversionOptions {
            preserve_layout,
            detect_headings,
            extract_tables: false,
            include_images,
            image_output_dir,
            embed_images,
            include_form_fields,
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
    #[pyo3(signature = (preserve_layout=false, detect_headings=true, include_images=true, image_output_dir=None, embed_images=true, include_form_fields=true))]
    fn to_markdown_all(
        &mut self,
        preserve_layout: bool,
        detect_headings: bool,
        include_images: bool,
        image_output_dir: Option<String>,
        embed_images: bool,
        include_form_fields: bool,
    ) -> PyResult<String> {
        let options = RustConversionOptions {
            preserve_layout,
            detect_headings,
            extract_tables: false,
            include_images,
            image_output_dir,
            embed_images,
            include_form_fields,
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
    #[pyo3(signature = (preserve_layout=false, detect_headings=true, include_images=true, image_output_dir=None, embed_images=true, include_form_fields=true))]
    fn to_html_all(
        &mut self,
        preserve_layout: bool,
        detect_headings: bool,
        include_images: bool,
        image_output_dir: Option<String>,
        embed_images: bool,
        include_form_fields: bool,
    ) -> PyResult<String> {
        let options = RustConversionOptions {
            preserve_layout,
            detect_headings,
            extract_tables: false,
            include_images,
            image_output_dir,
            embed_images,
            include_form_fields,
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

        let editor = self.editor.as_mut().unwrap();
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

        let editor = self.editor.as_mut().unwrap();
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

    // ========================================================================
    // Structured Extraction: Images, Spans, Paths
    // ========================================================================

    /// Extract image metadata from a page.
    ///
    /// Returns metadata for each image on the page (width, height, color space, etc.).
    /// Does NOT return raw image bytes — use `extract_images_to_files()` for that.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///
    /// Returns:
    ///     list[dict]: List of image metadata dictionaries with keys:
    ///         - width (int): Image width in pixels
    ///         - height (int): Image height in pixels
    ///         - color_space (str): Color space (e.g., "DeviceRGB", "DeviceGray")
    ///         - bits_per_component (int): Bits per color component
    ///         - bbox (tuple | None): Bounding box as (x, y, width, height), or None
    ///
    /// Raises:
    ///     RuntimeError: If image extraction fails
    ///
    /// Example:
    ///     >>> doc = PdfDocument("sample.pdf")
    ///     >>> images = doc.extract_images(0)
    ///     >>> for img in images:
    ///     ...     print(f"{img['width']}x{img['height']} {img['color_space']}")
    fn extract_images(&mut self, py: Python<'_>, page: usize) -> PyResult<Py<PyAny>> {
        let images = self
            .inner
            .extract_images(page)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to extract images: {}", e)))?;

        let py_list = pyo3::types::PyList::empty(py);
        for img in &images {
            let dict = pyo3::types::PyDict::new(py);
            dict.set_item("width", img.width())?;
            dict.set_item("height", img.height())?;
            dict.set_item("color_space", format!("{:?}", img.color_space()))?;
            dict.set_item("bits_per_component", img.bits_per_component())?;
            if let Some(bbox) = img.bbox() {
                dict.set_item("bbox", (bbox.x, bbox.y, bbox.width, bbox.height))?;
            } else {
                dict.set_item("bbox", py.None())?;
            }
            py_list.append(dict)?;
        }
        Ok(py_list.into())
    }

    /// Extract text spans from a page.
    ///
    /// Spans are groups of characters that share the same font and style.
    /// This is the recommended method for structured text extraction.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///
    /// Returns:
    ///     list[TextSpan]: List of text spans with position and style info
    ///
    /// Raises:
    ///     RuntimeError: If span extraction fails
    ///
    /// Example:
    ///     >>> doc = PdfDocument("sample.pdf")
    ///     >>> spans = doc.extract_spans(0)
    ///     >>> for span in spans:
    ///     ...     print(f"'{span.text}' font={span.font_name} size={span.font_size}")
    fn extract_spans(&mut self, page: usize) -> PyResult<Vec<PyTextSpan>> {
        self.inner
            .extract_spans(page)
            .map(|spans| {
                spans
                    .into_iter()
                    .map(|s| PyTextSpan { inner: s })
                    .collect()
            })
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to extract spans: {}", e)))
    }

    /// Get the document outline (bookmarks / table of contents).
    ///
    /// Returns:
    ///     list[dict] | None: Outline tree as nested dicts, or None if no outline.
    ///         Each dict has keys:
    ///         - title (str): Bookmark title
    ///         - page (int | None): Target page index (0-based), or None
    ///         - children (list[dict]): Child bookmarks (same structure)
    ///
    /// Raises:
    ///     RuntimeError: If outline extraction fails
    ///
    /// Example:
    ///     >>> doc = PdfDocument("book.pdf")
    ///     >>> outline = doc.get_outline()
    ///     >>> if outline:
    ///     ...     for item in outline:
    ///     ...         print(f"{item['title']} -> page {item['page']}")
    fn get_outline(&mut self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        let outline = self
            .inner
            .get_outline()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to get outline: {}", e)))?;

        match outline {
            None => Ok(None),
            Some(items) => {
                let result = outline_items_to_py(py, &items)?;
                Ok(Some(result))
            }
        }
    }

    /// Get annotations from a page.
    ///
    /// Returns annotation metadata including type, position, content, and form field info.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///
    /// Returns:
    ///     list[dict]: List of annotation dictionaries. Keys vary by type but may include:
    ///         - subtype (str): Annotation type (e.g., "Text", "Link", "Highlight")
    ///         - rect (tuple | None): Bounding rectangle as (x1, y1, x2, y2)
    ///         - contents (str | None): Text contents
    ///         - author (str | None): Author name
    ///         - creation_date (str | None): Creation date
    ///         - modification_date (str | None): Modification date
    ///         - subject (str | None): Subject
    ///         - color (tuple | None): Color as (r, g, b) tuple
    ///         - opacity (float | None): Opacity (0.0 to 1.0)
    ///         - field_type (str | None): Form field type if widget annotation
    ///         - field_name (str | None): Form field name
    ///         - field_value (str | None): Form field value
    ///         - action_uri (str | None): URI if link annotation
    ///
    /// Raises:
    ///     RuntimeError: If annotation extraction fails
    ///
    /// Example:
    ///     >>> doc = PdfDocument("annotated.pdf")
    ///     >>> annotations = doc.get_annotations(0)
    ///     >>> for ann in annotations:
    ///     ...     print(f"{ann['subtype']}: {ann.get('contents', '')}")
    fn get_annotations(&mut self, py: Python<'_>, page: usize) -> PyResult<Py<PyAny>> {
        let annotations = self
            .inner
            .get_annotations(page)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to get annotations: {}", e)))?;

        let py_list = pyo3::types::PyList::empty(py);
        for ann in &annotations {
            let dict = pyo3::types::PyDict::new(py);

            if let Some(ref subtype) = ann.subtype {
                dict.set_item("subtype", subtype)?;
            }
            if let Some(ref contents) = ann.contents {
                dict.set_item("contents", contents)?;
            }
            if let Some(rect) = ann.rect {
                dict.set_item("rect", (rect[0], rect[1], rect[2], rect[3]))?;
            }
            if let Some(ref author) = ann.author {
                dict.set_item("author", author)?;
            }
            if let Some(ref date) = ann.creation_date {
                dict.set_item("creation_date", date)?;
            }
            if let Some(ref date) = ann.modification_date {
                dict.set_item("modification_date", date)?;
            }
            if let Some(ref subject) = ann.subject {
                dict.set_item("subject", subject)?;
            }
            if let Some(ref color) = ann.color {
                if color.len() >= 3 {
                    dict.set_item("color", (color[0], color[1], color[2]))?;
                }
            }
            if let Some(opacity) = ann.opacity {
                dict.set_item("opacity", opacity)?;
            }
            if let Some(ref ft) = ann.field_type {
                dict.set_item("field_type", format!("{:?}", ft))?;
            }
            if let Some(ref name) = ann.field_name {
                dict.set_item("field_name", name)?;
            }
            if let Some(ref val) = ann.field_value {
                dict.set_item("field_value", val)?;
            }
            // Extract URI from link action
            if let Some(ref action) = ann.action {
                if let crate::annotations::LinkAction::Uri(ref uri) = action {
                    dict.set_item("action_uri", uri)?;
                }
            }

            py_list.append(dict)?;
        }
        Ok(py_list.into())
    }

    /// Extract vector paths (lines, curves, shapes) from a page.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///
    /// Returns:
    ///     list[dict]: List of path dictionaries with keys:
    ///         - bbox (tuple): Bounding box as (x, y, width, height)
    ///         - stroke_width (float): Stroke line width
    ///         - stroke_color (tuple | None): Stroke color as (r, g, b), or None
    ///         - fill_color (tuple | None): Fill color as (r, g, b), or None
    ///         - line_cap (str): Line cap style ("butt", "round", "square")
    ///         - line_join (str): Line join style ("miter", "round", "bevel")
    ///         - operations_count (int): Number of path operations
    ///
    /// Raises:
    ///     RuntimeError: If path extraction fails
    ///
    /// Example:
    ///     >>> doc = PdfDocument("vector.pdf")
    ///     >>> paths = doc.extract_paths(0)
    ///     >>> for p in paths:
    ///     ...     print(f"Path at {p['bbox']}, stroke={p['stroke_color']}")
    fn extract_paths(&mut self, py: Python<'_>, page: usize) -> PyResult<Py<PyAny>> {
        let paths = self
            .inner
            .extract_paths(page)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to extract paths: {}", e)))?;

        let py_list = pyo3::types::PyList::empty(py);
        for path in &paths {
            let dict = pyo3::types::PyDict::new(py);
            dict.set_item(
                "bbox",
                (path.bbox.x, path.bbox.y, path.bbox.width, path.bbox.height),
            )?;
            dict.set_item("stroke_width", path.stroke_width)?;

            if let Some(ref color) = path.stroke_color {
                dict.set_item("stroke_color", (color.r, color.g, color.b))?;
            } else {
                dict.set_item("stroke_color", py.None())?;
            }

            if let Some(ref color) = path.fill_color {
                dict.set_item("fill_color", (color.r, color.g, color.b))?;
            } else {
                dict.set_item("fill_color", py.None())?;
            }

            let cap_str = match path.line_cap {
                crate::elements::LineCap::Butt => "butt",
                crate::elements::LineCap::Round => "round",
                crate::elements::LineCap::Square => "square",
            };
            dict.set_item("line_cap", cap_str)?;

            let join_str = match path.line_join {
                crate::elements::LineJoin::Miter => "miter",
                crate::elements::LineJoin::Round => "round",
                crate::elements::LineJoin::Bevel => "bevel",
            };
            dict.set_item("line_join", join_str)?;

            dict.set_item("operations_count", path.operations.len())?;

            py_list.append(dict)?;
        }
        Ok(py_list.into())
    }

    // ========================================================================
    // OCR Text Extraction (feature-gated)
    // ========================================================================

    /// Extract text from a page using OCR (optical character recognition).
    ///
    /// Falls back to native text extraction when the page has digital text.
    /// Requires the `ocr` feature to be enabled at build time.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///     engine (OcrEngine | None): OCR engine instance. Required for scanned pages.
    ///
    /// Returns:
    ///     str: Extracted text from the page
    ///
    /// Raises:
    ///     RuntimeError: If text extraction fails
    ///
    /// Example:
    ///     >>> engine = OcrEngine("det.onnx", "rec.onnx", "dict.txt")
    ///     >>> text = doc.extract_text_ocr(0, engine)
    #[cfg(feature = "ocr")]
    #[pyo3(signature = (page, engine=None))]
    fn extract_text_ocr(
        &mut self,
        page: usize,
        engine: Option<&PyOcrEngine>,
    ) -> PyResult<String> {
        let ocr_engine = engine.map(|e| &e.inner);
        let options = crate::ocr::OcrExtractOptions::default();
        self.inner
            .extract_text_with_ocr(page, ocr_engine, options)
            .map_err(|e| PyRuntimeError::new_err(format!("OCR extraction failed: {}", e)))
    }

    // ========================================================================
    // Form Fields (AcroForm)
    // ========================================================================

    /// Get all form fields from the document.
    ///
    /// Extracts AcroForm fields including text inputs, checkboxes, radio buttons,
    /// dropdowns, and signature fields. Works with tax forms, insurance documents,
    /// government forms, and any PDF with interactive fields.
    ///
    /// Returns:
    ///     list[FormField]: List of form fields with names, types, values, and metadata
    ///
    /// Raises:
    ///     RuntimeError: If form extraction fails
    ///
    /// Example:
    ///     >>> doc = PdfDocument("w2_form.pdf")
    ///     >>> fields = doc.get_form_fields()
    ///     >>> for f in fields:
    ///     ...     print(f"{f.name}: {f.value}")
    fn get_form_fields(&mut self) -> PyResult<Vec<PyFormField>> {
        use crate::extractors::forms::FormExtractor;

        let fields = FormExtractor::extract_fields(&mut self.inner)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to extract form fields: {}", e)))?;

        Ok(fields.into_iter().map(|f| PyFormField { inner: f }).collect())
    }

    /// Get the value of a specific form field by name.
    ///
    /// Args:
    ///     name (str): Full qualified field name (e.g., "topmostSubform[0].Page1[0].f1_01[0]")
    ///
    /// Returns:
    ///     str | bool | list | None: The field value, or None if not found
    ///
    /// Raises:
    ///     RuntimeError: If field lookup fails
    ///
    /// Example:
    ///     >>> val = doc.get_form_field_value("employee_name")
    ///     >>> print(val)  # "John Doe"
    fn get_form_field_value(&mut self, name: &str, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.ensure_editor()?;
        let editor = self.editor.as_mut().unwrap();

        let value = editor
            .get_form_field_value(name)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to get field value: {}", e)))?;

        match value {
            Some(v) => form_field_value_to_python(&v, py),
            None => Ok(py.None()),
        }
    }

    /// Set the value of a form field.
    ///
    /// Args:
    ///     name (str): Full qualified field name
    ///     value (str | bool): New value for the field
    ///
    /// Raises:
    ///     RuntimeError: If the field is not found or value cannot be set
    ///
    /// Example:
    ///     >>> doc.set_form_field_value("employee_name", "Jane Doe")
    ///     >>> doc.save("filled_form.pdf")
    fn set_form_field_value(&mut self, name: &str, value: &Bound<'_, PyAny>) -> PyResult<()> {
        self.ensure_editor()?;
        let editor = self.editor.as_mut().unwrap();

        let field_value = python_to_form_field_value(value)?;

        editor
            .set_form_field_value(name, field_value)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to set field value: {}", e)))
    }

    /// Check if the document contains an XFA form.
    ///
    /// XFA (XML Forms Architecture) is used by some PDF generators (e.g., Adobe LiveCycle).
    /// IRS W-2 and many government forms are hybrid AcroForm + XFA.
    ///
    /// Returns:
    ///     bool: True if the document has XFA form data
    ///
    /// Example:
    ///     >>> if doc.has_xfa():
    ///     ...     print("Document has XFA form data")
    fn has_xfa(&mut self) -> PyResult<bool> {
        use crate::xfa::XfaExtractor;

        XfaExtractor::has_xfa(&mut self.inner)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to check XFA: {}", e)))
    }

    /// Export form data to FDF or XFDF format.
    ///
    /// Args:
    ///     path (str): Output file path
    ///     format (str): Export format, "fdf" or "xfdf" (default: "fdf")
    ///
    /// Raises:
    ///     RuntimeError: If export fails
    ///
    /// Example:
    ///     >>> doc.export_form_data("form_data.fdf")
    ///     >>> doc.export_form_data("form_data.xfdf", format="xfdf")
    #[pyo3(signature = (path, format="fdf"))]
    fn export_form_data(&mut self, path: &str, format: &str) -> PyResult<()> {
        self.ensure_editor()?;
        let editor = self.editor.as_mut().unwrap();

        match format {
            "fdf" => editor
                .export_form_data_fdf(path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to export FDF: {}", e))),
            "xfdf" => editor
                .export_form_data_xfdf(path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to export XFDF: {}", e))),
            _ => Err(PyRuntimeError::new_err(
                format!("Unknown format '{}'. Use 'fdf' or 'xfdf'.", format),
            )),
        }
    }

    // ========================================================================
    // Image Bytes Extraction
    // ========================================================================

    /// Extract image bytes from a page as PNG data.
    ///
    /// Returns actual image pixel data (as PNG), unlike extract_images() which
    /// returns only metadata.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///
    /// Returns:
    ///     list[dict]: List of dicts with keys: width (int), height (int),
    ///         data (bytes, PNG-encoded), format (str, always "png")
    ///
    /// Raises:
    ///     RuntimeError: If extraction or conversion fails
    fn extract_image_bytes(&mut self, py: Python<'_>, page: usize) -> PyResult<Py<PyAny>> {
        let images = self
            .inner
            .extract_images(page)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to extract images: {}", e)))?;

        let py_list = pyo3::types::PyList::empty(py);
        for img in &images {
            let png_data = img
                .to_png_bytes()
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to convert image to PNG: {}", e)))?;

            let dict = pyo3::types::PyDict::new(py);
            dict.set_item("width", img.width())?;
            dict.set_item("height", img.height())?;
            dict.set_item("format", "png")?;
            dict.set_item("data", pyo3::types::PyBytes::new(py, &png_data))?;
            py_list.append(dict)?;
        }
        Ok(py_list.into())
    }

    // ========================================================================
    // Form Flattening
    // ========================================================================

    /// Flatten all form fields into page content.
    ///
    /// After flattening, form field values become static text and are no longer editable.
    ///
    /// Raises:
    ///     RuntimeError: If flattening fails
    fn flatten_forms(&mut self) -> PyResult<()> {
        self.ensure_editor()?;
        if let Some(ref mut editor) = self.editor {
            editor
                .flatten_forms()
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to flatten forms: {}", e)))
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    /// Flatten form fields on a specific page.
    ///
    /// Args:
    ///     page (int): Page index (0-based)
    ///
    /// Raises:
    ///     RuntimeError: If flattening fails
    fn flatten_forms_on_page(&mut self, page: usize) -> PyResult<()> {
        self.ensure_editor()?;
        if let Some(ref mut editor) = self.editor {
            editor
                .flatten_forms_on_page(page)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to flatten forms on page: {}", e)))
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    // ========================================================================
    // PDF Merging
    // ========================================================================

    /// Merge another PDF into this document.
    ///
    /// Accepts either a file path (str) or raw PDF bytes.
    ///
    /// Args:
    ///     source: File path (str) or PDF bytes
    ///
    /// Returns:
    ///     int: Number of pages merged
    ///
    /// Raises:
    ///     RuntimeError: If merge fails
    fn merge_from(&mut self, source: &Bound<'_, PyAny>) -> PyResult<usize> {
        self.ensure_editor()?;
        let editor = self.editor.as_mut().unwrap();

        if let Ok(path) = source.extract::<String>() {
            editor
                .merge_from(&path)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to merge PDF: {}", e)))
        } else if let Ok(data) = source.extract::<Vec<u8>>() {
            editor
                .merge_from_bytes(&data)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to merge PDF: {}", e)))
        } else {
            Err(PyRuntimeError::new_err(
                "source must be a file path (str) or PDF bytes",
            ))
        }
    }

    // ========================================================================
    // File Embedding
    // ========================================================================

    /// Embed a file into the PDF document.
    ///
    /// Args:
    ///     name (str): Display name for the embedded file
    ///     data (bytes): File contents
    ///
    /// Raises:
    ///     RuntimeError: If embedding fails
    fn embed_file(&mut self, name: &str, data: &[u8]) -> PyResult<()> {
        self.ensure_editor()?;
        if let Some(ref mut editor) = self.editor {
            editor
                .embed_file(name, data.to_vec())
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to embed file: {}", e)))
        } else {
            Err(PyRuntimeError::new_err("No document loaded"))
        }
    }

    // ========================================================================
    // Page Labels
    // ========================================================================

    /// Get page label ranges from the document.
    ///
    /// Returns:
    ///     list[dict]: List of dicts with keys: start_page (int), style (str),
    ///         prefix (str | None), start_value (int)
    ///
    /// Raises:
    ///     RuntimeError: If extraction fails
    fn page_labels(&mut self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        use crate::extractors::page_labels::PageLabelExtractor;

        let labels = PageLabelExtractor::extract(&mut self.inner)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to get page labels: {}", e)))?;

        let py_list = pyo3::types::PyList::empty(py);
        for label in &labels {
            let dict = pyo3::types::PyDict::new(py);
            dict.set_item("start_page", label.start_page)?;
            dict.set_item("style", format!("{:?}", label.style))?;
            match &label.prefix {
                Some(p) => dict.set_item("prefix", p)?,
                None => dict.set_item("prefix", py.None())?,
            };
            dict.set_item("start_value", label.start_value)?;
            py_list.append(dict)?;
        }
        Ok(py_list.into())
    }

    // ========================================================================
    // XMP Metadata
    // ========================================================================

    /// Get XMP metadata from the document.
    ///
    /// Returns:
    ///     dict | None: Dict with XMP fields (dc_title, dc_creator, etc.) or None
    ///
    /// Raises:
    ///     RuntimeError: If extraction fails
    fn xmp_metadata(&mut self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        use crate::extractors::xmp::XmpExtractor;

        let metadata = XmpExtractor::extract(&mut self.inner)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to get XMP metadata: {}", e)))?;

        match metadata {
            None => Ok(py.None()),
            Some(xmp) => {
                let dict = pyo3::types::PyDict::new(py);
                if let Some(ref title) = xmp.dc_title {
                    dict.set_item("dc_title", title)?;
                }
                if !xmp.dc_creator.is_empty() {
                    dict.set_item("dc_creator", &xmp.dc_creator)?;
                }
                if let Some(ref desc) = xmp.dc_description {
                    dict.set_item("dc_description", desc)?;
                }
                if !xmp.dc_subject.is_empty() {
                    dict.set_item("dc_subject", &xmp.dc_subject)?;
                }
                if let Some(ref lang) = xmp.dc_language {
                    dict.set_item("dc_language", lang)?;
                }
                if let Some(ref tool) = xmp.xmp_creator_tool {
                    dict.set_item("xmp_creator_tool", tool)?;
                }
                if let Some(ref date) = xmp.xmp_create_date {
                    dict.set_item("xmp_create_date", date)?;
                }
                if let Some(ref date) = xmp.xmp_modify_date {
                    dict.set_item("xmp_modify_date", date)?;
                }
                if let Some(ref producer) = xmp.pdf_producer {
                    dict.set_item("pdf_producer", producer)?;
                }
                if let Some(ref keywords) = xmp.pdf_keywords {
                    dict.set_item("pdf_keywords", keywords)?;
                }
                Ok(dict.into())
            }
        }
    }

    /// String representation of the document.
    ///
    /// Returns:
    ///     str: Representation showing PDF version
    fn __repr__(&self) -> String {
        format!("PdfDocument(version={}.{})", self.inner.version().0, self.inner.version().1)
    }
}

// === Form Field Type ===

use crate::extractors::forms::{
    FieldType as RustFieldType, FieldValue as RustFieldValue,
    FormField as RustFormField, field_flags,
};

/// A form field extracted from a PDF AcroForm.
///
/// Represents interactive fields like text inputs, checkboxes, radio buttons,
/// dropdowns, and signature fields found in PDF forms.
///
/// Properties:
///     name (str): Full qualified field name
///     field_type (str): Field type ("text", "button", "choice", "signature", or "unknown")
///     value (str | bool | list | None): Field value
///     tooltip (str | None): Tooltip/description text
///     bounds (tuple | None): Bounding box as (x1, y1, x2, y2) or None
///     flags (int | None): Raw field flags bitmask
///     max_length (int | None): Maximum length for text fields
///     is_readonly (bool): Whether the field is read-only
///     is_required (bool): Whether the field is required
///
/// Example:
///     >>> fields = doc.get_form_fields()
///     >>> for f in fields:
///     ...     print(f"{f.name} ({f.field_type}): {f.value}")
#[pyclass(name = "FormField", unsendable)]
pub struct PyFormField {
    inner: RustFormField,
}

#[pymethods]
impl PyFormField {
    /// Full qualified field name (e.g., "topmostSubform[0].Page1[0].f1_01[0]").
    #[getter]
    fn name(&self) -> &str {
        &self.inner.full_name
    }

    /// Field type as a string: "text", "button", "choice", "signature", or "unknown".
    #[getter]
    fn field_type(&self) -> &str {
        match &self.inner.field_type {
            RustFieldType::Text => "text",
            RustFieldType::Button => "button",
            RustFieldType::Choice => "choice",
            RustFieldType::Signature => "signature",
            RustFieldType::Unknown(_) => "unknown",
        }
    }

    /// Field value: str for text/name, bool for checkbox, list for multi-select, None if empty.
    #[getter]
    fn value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        field_value_to_python(&self.inner.value, py)
    }

    /// Tooltip or description text, if set.
    #[getter]
    fn tooltip(&self) -> Option<&str> {
        self.inner.tooltip.as_deref()
    }

    /// Bounding box as (x1, y1, x2, y2), or None if not available.
    #[getter]
    fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        self.inner.bounds.map(|b| (b[0], b[1], b[2], b[3]))
    }

    /// Raw field flags bitmask (see PDF spec Table 221).
    #[getter]
    fn flags(&self) -> Option<u32> {
        self.inner.flags
    }

    /// Maximum text length for text fields, or None.
    #[getter]
    fn max_length(&self) -> Option<u32> {
        self.inner.max_length
    }

    /// Whether this field is read-only.
    #[getter]
    fn is_readonly(&self) -> bool {
        self.inner.flags.map_or(false, |f| f & field_flags::READ_ONLY != 0)
    }

    /// Whether this field is required.
    #[getter]
    fn is_required(&self) -> bool {
        self.inner.flags.map_or(false, |f| f & field_flags::REQUIRED != 0)
    }

    fn __repr__(&self) -> String {
        let val_str = match &self.inner.value {
            RustFieldValue::Text(s) => format!("\"{}\"", s),
            RustFieldValue::Boolean(b) => format!("{}", b),
            RustFieldValue::Name(s) => format!("\"{}\"", s),
            RustFieldValue::Array(v) => format!("{:?}", v),
            RustFieldValue::None => "None".to_string(),
        };
        format!(
            "FormField(name=\"{}\", type=\"{}\", value={})",
            self.inner.full_name,
            self.field_type(),
            val_str
        )
    }
}

/// Convert an extractor FieldValue to a Python object.
fn field_value_to_python(value: &RustFieldValue, py: Python<'_>) -> PyResult<Py<PyAny>> {
    match value {
        RustFieldValue::Text(s) => Ok(s.into_pyobject(py)?.into_any().unbind()),
        RustFieldValue::Name(s) => Ok(s.into_pyobject(py)?.into_any().unbind()),
        RustFieldValue::Boolean(b) => Ok(b.into_pyobject(py)?.to_owned().into_any().unbind()),
        RustFieldValue::Array(v) => Ok(v.into_pyobject(py)?.into_any().unbind()),
        RustFieldValue::None => Ok(py.None()),
    }
}

/// Convert an editor FormFieldValue to a Python object.
fn form_field_value_to_python(
    value: &crate::editor::form_fields::FormFieldValue,
    py: Python<'_>,
) -> PyResult<Py<PyAny>> {
    use crate::editor::form_fields::FormFieldValue;
    match value {
        FormFieldValue::Text(s) => Ok(s.into_pyobject(py)?.into_any().unbind()),
        FormFieldValue::Choice(s) => Ok(s.into_pyobject(py)?.into_any().unbind()),
        FormFieldValue::Boolean(b) => Ok(b.into_pyobject(py)?.to_owned().into_any().unbind()),
        FormFieldValue::MultiChoice(v) => Ok(v.into_pyobject(py)?.into_any().unbind()),
        FormFieldValue::None => Ok(py.None()),
    }
}

/// Convert a Python value to a FormFieldValue.
fn python_to_form_field_value(
    value: &Bound<'_, PyAny>,
) -> PyResult<crate::editor::form_fields::FormFieldValue> {
    use crate::editor::form_fields::FormFieldValue;

    if let Ok(b) = value.extract::<bool>() {
        Ok(FormFieldValue::Boolean(b))
    } else if let Ok(s) = value.extract::<String>() {
        Ok(FormFieldValue::Text(s))
    } else if let Ok(v) = value.extract::<Vec<String>>() {
        Ok(FormFieldValue::MultiChoice(v))
    } else if value.is_none() {
        Ok(FormFieldValue::None)
    } else {
        Err(PyRuntimeError::new_err(
            "Value must be str, bool, list[str], or None",
        ))
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

    /// Create a PDF from an image file.
    ///
    /// Args:
    ///     path (str): Path to the image file (PNG, JPEG)
    ///
    /// Returns:
    ///     Pdf: Created PDF document with image as a page
    ///
    /// Raises:
    ///     RuntimeError: If image loading or PDF creation fails
    #[staticmethod]
    fn from_image(path: &str) -> PyResult<Self> {
        use crate::api::Pdf;
        let pdf = Pdf::from_image(path)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create PDF from image: {}", e)))?;
        Ok(PyPdf {
            bytes: pdf.into_bytes(),
        })
    }

    /// Create a multi-page PDF from multiple image files.
    ///
    /// Each image becomes a separate page.
    ///
    /// Args:
    ///     paths (list[str]): List of paths to image files
    ///
    /// Returns:
    ///     Pdf: Created PDF document
    ///
    /// Raises:
    ///     RuntimeError: If image loading or PDF creation fails
    #[staticmethod]
    fn from_images(paths: Vec<String>) -> PyResult<Self> {
        use crate::api::Pdf;
        let pdf = Pdf::from_images(&paths)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create PDF from images: {}", e)))?;
        Ok(PyPdf {
            bytes: pdf.into_bytes(),
        })
    }

    /// Create a PDF from image bytes.
    ///
    /// Args:
    ///     data (bytes): Raw image data (PNG or JPEG)
    ///
    /// Returns:
    ///     Pdf: Created PDF document
    ///
    /// Raises:
    ///     RuntimeError: If image loading or PDF creation fails
    #[staticmethod]
    fn from_image_bytes(data: &[u8]) -> PyResult<Self> {
        use crate::api::Pdf;
        let pdf = Pdf::from_image_bytes(data)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create PDF from image bytes: {}", e)))?;
        Ok(PyPdf {
            bytes: pdf.into_bytes(),
        })
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

// === Text Span Type ===

/// A text span with position and style information.
///
/// Spans are groups of characters that share the same font and style.
/// Use `PdfDocument.extract_spans()` to get a list of these.
///
/// # Attributes
///
/// - `text` (str): The text content
/// - `bbox` (tuple): Bounding box as (x, y, width, height)
/// - `font_name` (str): Font family name
/// - `font_size` (float): Font size in points
/// - `is_bold` (bool): Whether the text is bold
/// - `is_italic` (bool): Whether the text is italic
/// - `color` (tuple): RGB color as (r, g, b) with values 0.0-1.0
#[pyclass(name = "TextSpan")]
#[derive(Clone)]
pub struct PyTextSpan {
    inner: crate::layout::TextSpan,
}

#[pymethods]
impl PyTextSpan {
    /// The text content of the span.
    #[getter]
    fn text(&self) -> &str {
        &self.inner.text
    }

    /// Bounding box as (x, y, width, height).
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
    fn font_name(&self) -> &str {
        &self.inner.font_name
    }

    /// Font size in points.
    #[getter]
    fn font_size(&self) -> f32 {
        self.inner.font_size
    }

    /// Whether the text is bold (font weight >= 700).
    #[getter]
    fn is_bold(&self) -> bool {
        self.inner.font_weight as u16 >= 700
    }

    /// Whether the text is italic.
    #[getter]
    fn is_italic(&self) -> bool {
        self.inner.is_italic
    }

    /// Text color as (r, g, b) with values 0.0-1.0.
    #[getter]
    fn color(&self) -> (f32, f32, f32) {
        (self.inner.color.r, self.inner.color.g, self.inner.color.b)
    }

    fn __repr__(&self) -> String {
        let preview = if self.inner.text.len() > 30 {
            format!("{}...", &self.inner.text[..30])
        } else {
            self.inner.text.clone()
        };
        format!(
            "TextSpan({:?}, font={}, size={:.1})",
            preview, self.inner.font_name, self.inner.font_size
        )
    }
}

// === Outline Helper ===

/// Convert OutlineItem tree to Python nested dicts.
fn outline_items_to_py(py: Python<'_>, items: &[crate::outline::OutlineItem]) -> PyResult<Py<PyAny>> {
    let py_list = pyo3::types::PyList::empty(py);
    for item in items {
        let dict = pyo3::types::PyDict::new(py);
        dict.set_item("title", &item.title)?;

        match &item.dest {
            Some(crate::outline::Destination::PageIndex(idx)) => {
                dict.set_item("page", *idx)?;
            }
            Some(crate::outline::Destination::Named(name)) => {
                dict.set_item("page", py.None())?;
                dict.set_item("dest_name", name)?;
            }
            None => {
                dict.set_item("page", py.None())?;
            }
        }

        let children = outline_items_to_py(py, &item.children)?;
        dict.set_item("children", children)?;

        py_list.append(dict)?;
    }
    Ok(py_list.into())
}

// === OCR Types (feature-gated) ===

/// OCR engine for extracting text from scanned PDF pages.
///
/// Requires the `ocr` feature to be enabled at build time.
///
/// Example:
///     >>> engine = OcrEngine("det.onnx", "rec.onnx", "dict.txt")
///     >>> text = doc.extract_text_ocr(0, engine)
#[cfg(feature = "ocr")]
#[pyclass(name = "OcrEngine", unsendable)]
pub struct PyOcrEngine {
    inner: crate::ocr::OcrEngine,
}

#[cfg(feature = "ocr")]
#[pymethods]
impl PyOcrEngine {
    /// Create a new OCR engine.
    ///
    /// Args:
    ///     det_model_path (str): Path to the text detection ONNX model
    ///     rec_model_path (str): Path to the text recognition ONNX model
    ///     dict_path (str): Path to the character dictionary file
    ///     config (OcrConfig | None): Optional OCR configuration
    ///
    /// Raises:
    ///     RuntimeError: If model loading fails
    ///
    /// Example:
    ///     >>> engine = OcrEngine("det.onnx", "rec.onnx", "dict.txt")
    ///     >>> engine_custom = OcrEngine("det.onnx", "rec.onnx", "dict.txt",
    ///     ...     OcrConfig(det_threshold=0.5))
    #[new]
    #[pyo3(signature = (det_model_path, rec_model_path, dict_path, config=None))]
    fn new(
        det_model_path: &str,
        rec_model_path: &str,
        dict_path: &str,
        config: Option<&PyOcrConfig>,
    ) -> PyResult<Self> {
        let ocr_config = config
            .map(|c| c.inner.clone())
            .unwrap_or_default();
        let engine = crate::ocr::OcrEngine::new(det_model_path, rec_model_path, dict_path, ocr_config)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create OCR engine: {}", e)))?;
        Ok(PyOcrEngine { inner: engine })
    }

    fn __repr__(&self) -> String {
        "OcrEngine(...)".to_string()
    }
}

/// Configuration for OCR processing.
///
/// All parameters are optional and have sensible defaults.
///
/// Example:
///     >>> config = OcrConfig(det_threshold=0.5, num_threads=8)
///     >>> engine = OcrEngine("det.onnx", "rec.onnx", "dict.txt", config)
#[cfg(feature = "ocr")]
#[pyclass(name = "OcrConfig")]
#[derive(Clone)]
pub struct PyOcrConfig {
    inner: crate::ocr::OcrConfig,
}

#[cfg(feature = "ocr")]
#[pymethods]
impl PyOcrConfig {
    /// Create OCR configuration with optional parameters.
    ///
    /// Args:
    ///     det_threshold (float): Detection threshold (0.0-1.0, default: 0.3)
    ///     box_threshold (float): Box threshold (0.0-1.0, default: 0.6)
    ///     rec_threshold (float): Recognition threshold (0.0-1.0, default: 0.5)
    ///     num_threads (int): Number of threads (default: 4)
    ///     max_candidates (int): Max text candidates (default: 1000)
    ///     use_v5 (bool): Use PP-OCRv5 optimized settings (default: False).
    ///         When True, uses high-resolution input for detection (up to 4000px)
    ///         which is required for PP-OCRv5 server models.
    #[new]
    #[pyo3(signature = (det_threshold=None, box_threshold=None, rec_threshold=None, num_threads=None, max_candidates=None, use_v5=false))]
    fn new(
        det_threshold: Option<f32>,
        box_threshold: Option<f32>,
        rec_threshold: Option<f32>,
        num_threads: Option<usize>,
        max_candidates: Option<usize>,
        use_v5: bool,
    ) -> Self {
        let mut config = if use_v5 {
            crate::ocr::OcrConfig::v5()
        } else {
            crate::ocr::OcrConfig::default()
        };
        if let Some(v) = det_threshold {
            config.det_threshold = v;
        }
        if let Some(v) = box_threshold {
            config.box_threshold = v;
        }
        if let Some(v) = rec_threshold {
            config.rec_threshold = v;
        }
        if let Some(v) = num_threads {
            config.num_threads = v;
        }
        if let Some(v) = max_candidates {
            config.max_candidates = v;
        }
        PyOcrConfig { inner: config }
    }

    fn __repr__(&self) -> String {
        format!(
            "OcrConfig(det_threshold={}, rec_threshold={}, threads={})",
            self.inner.det_threshold, self.inner.rec_threshold, self.inner.num_threads
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
    #[allow(dead_code)]
    inner: RustLineCap,
}

#[pymethods]
impl PyLineCap {
    /// Butt cap (default).
    #[staticmethod]
    #[allow(non_snake_case)]
    fn BUTT() -> Self {
        PyLineCap {
            inner: RustLineCap::Butt,
        }
    }

    /// Round cap.
    #[staticmethod]
    #[allow(non_snake_case)]
    fn ROUND() -> Self {
        PyLineCap {
            inner: RustLineCap::Round,
        }
    }

    /// Square cap.
    #[staticmethod]
    #[allow(non_snake_case)]
    fn SQUARE() -> Self {
        PyLineCap {
            inner: RustLineCap::Square,
        }
    }
}

/// Line join styles.
#[pyclass(name = "LineJoin")]
#[derive(Clone)]
pub struct PyLineJoin {
    #[allow(dead_code)]
    inner: RustLineJoin,
}

#[pymethods]
impl PyLineJoin {
    /// Miter join (default).
    #[staticmethod]
    #[allow(non_snake_case)]
    fn MITER() -> Self {
        PyLineJoin {
            inner: RustLineJoin::Miter,
        }
    }

    /// Round join.
    #[staticmethod]
    #[allow(non_snake_case)]
    fn ROUND() -> Self {
        PyLineJoin {
            inner: RustLineJoin::Round,
        }
    }

    /// Bevel join.
    #[staticmethod]
    #[allow(non_snake_case)]
    fn BEVEL() -> Self {
        PyLineJoin {
            inner: RustLineJoin::Bevel,
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
    m.add_class::<PyTextSpan>()?;

    // Form field types
    m.add_class::<PyFormField>()?;

    // OCR types (optional, requires ocr feature)
    #[cfg(feature = "ocr")]
    {
        m.add_class::<PyOcrEngine>()?;
        m.add_class::<PyOcrConfig>()?;
    }

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
